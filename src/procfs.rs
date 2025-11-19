use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::metrics::ProcessMetrics;
use std::time::{SystemTime, UNIX_EPOCH};

fn read_to_string(path: &PathBuf) -> Result<String> {
    Ok(std::fs::read_to_string(path)?.trim().to_string())
}

pub fn update(metrics: &ProcessMetrics, pid: i32) -> Result<()> {
    update_stat(metrics, pid)?;
    update_status(metrics, pid)?;
    update_io(metrics, pid)?;
    Ok(())
}

fn update_stat(metrics: &ProcessMetrics, pid: i32) -> Result<()> {
    let stat_path = PathBuf::from(format!("/proc/{}/stat", pid));
    let content = read_to_string(&stat_path).context("read /proc/<pid>/stat")?;
    let parts: Vec<&str> = content.split_whitespace().collect();

    if parts.len() <= 21 {
        return Ok(()); // nedostatek dat, nic neupdatujeme
    }

    // proc(5): utime=14, stime=15, starttime=22 (indexy 13,14,21)
    let utime_ticks: f64 = parts[13].parse::<u64>().unwrap_or(0) as f64;
    let stime_ticks: f64 = parts[14].parse::<u64>().unwrap_or(0) as f64;
    let start_ticks: f64 = parts[21].parse::<u64>().unwrap_or(0) as f64;

    let ticks_per_sec = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    if ticks_per_sec <= 0.0 {
        return Ok(());
    }

    metrics.cpu_user_seconds.set(utime_ticks / ticks_per_sec);
    metrics.cpu_system_seconds.set(stime_ticks / ticks_per_sec);

    // boot time z /proc/stat (btime)
    let boot_time = std::fs::read_to_string("/proc/stat")?
        .lines()
        .find(|l| l.starts_with("btime "))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    let start_time = boot_time as f64 + start_ticks / ticks_per_sec;
    metrics.start_time_seconds.set(start_time);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    metrics.uptime_seconds.set(now - start_time);

    Ok(())
}

fn update_status(metrics: &ProcessMetrics, pid: i32) -> Result<()> {
    let status_path = PathBuf::from(format!("/proc/{}/status", pid));
    let content = read_to_string(&status_path).context("read /proc/<pid>/status")?;

    let mut rss_kb = 0u64;
    let mut vms_kb = 0u64;
    let mut swap_kb = 0u64;

    for line in content.lines() {
        if line.starts_with("VmRSS:") {
            rss_kb = grab_kb(line);
        } else if line.starts_with("VmSize:") {
            vms_kb = grab_kb(line);
        } else if line.starts_with("VmSwap:") {
            swap_kb = grab_kb(line);
        }
    }

    metrics.mem_rss_bytes.set((rss_kb * 1024) as f64);
    metrics.mem_vms_bytes.set((vms_kb * 1024) as f64);
    metrics.mem_swap_bytes.set((swap_kb * 1024) as f64);

    Ok(())
}

fn grab_kb(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
}

fn update_io(metrics: &ProcessMetrics, pid: i32) -> Result<()> {
    let io_path = PathBuf::from(format!("/proc/{}/io", pid));
    let content = match read_to_string(&io_path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // některá prostředí /proc/io nemají
    };

    let mut rchar = 0u64;
    let mut wchar = 0u64;
    let mut syscr = 0u64;
    let mut syscw = 0u64;
    let mut read_bytes = 0u64;
    let mut write_bytes = 0u64;
    let mut cancelled_write_bytes = 0u64;

    for line in content.lines() {
        let mut parts = line.split_whitespace();
        let key = parts.next().unwrap_or("");
        let val = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
        match key {
            "rchar:" => rchar = val,
            "wchar:" => wchar = val,
            "syscr:" => syscr = val,
            "syscw:" => syscw = val,
            "read_bytes:" => read_bytes = val,
            "write_bytes:" => write_bytes = val,
            "cancelled_write_bytes:" => cancelled_write_bytes = val,
            _ => {}
        }
    }

    metrics.io_rchar_bytes_total.set(rchar as f64);
    metrics.io_wchar_bytes_total.set(wchar as f64);
    metrics.io_syscr_total.set(syscr as f64);
    metrics.io_syscw_total.set(syscw as f64);
    metrics.io_read_bytes_total.set(read_bytes as f64);
    metrics.io_write_bytes_total.set(write_bytes as f64);
    metrics
        .io_cancelled_write_bytes_total
        .set(cancelled_write_bytes as f64);

    Ok(())
}
