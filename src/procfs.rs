use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use tracing::{debug, info};

use crate::config::ProcessTarget;
use crate::metrics::ProcessMetrics;

fn read_to_string(path: &PathBuf) -> Result<String> {
    Ok(std::fs::read_to_string(path)?.trim().to_string())
}

#[derive(Default)]
struct ProcSample {
    cpu_user_seconds: f64,
    cpu_system_seconds: f64,
    start_time_seconds: Option<f64>,

    mem_rss_bytes: f64,
    mem_vms_bytes: f64,
    mem_swap_bytes: f64,

    io_rchar_bytes_total: f64,
    io_wchar_bytes_total: f64,
    io_syscr_total: f64,
    io_syscw_total: f64,
    io_read_bytes_total: f64,
    io_write_bytes_total: f64,
    io_cancelled_write_bytes_total: f64,
}

/// Původní API - jeden konkrétní PID.
/// Interně jen volá agregaci nad jedním PIDem.
#[allow(dead_code)]
pub fn update(metrics: &ProcessMetrics, pid: i32) -> Result<()> {
    update_for_pids(metrics, &[pid])
}

/// Aktualizuje metriky pro skupinu PIDů.
///
/// - CPU a IO „countery“ se prostě sečtou.
/// - paměťové hodnoty se také sečtou.
/// - start_time_seconds = nejstarší start time ze skupiny.
/// - uptime_seconds = now - min(start_time).
pub fn update_for_pids(metrics: &ProcessMetrics, pids: &[i32]) -> Result<()> {
    let mut agg = ProcSample::default();
    let mut oldest_start: Option<f64> = None;
    let mut any = false;

    for &pid in pids {
        let sample = read_proc_sample(pid)?;
        any = true;

        agg.cpu_user_seconds += sample.cpu_user_seconds;
        agg.cpu_system_seconds += sample.cpu_system_seconds;

        agg.mem_rss_bytes += sample.mem_rss_bytes;
        agg.mem_vms_bytes += sample.mem_vms_bytes;
        agg.mem_swap_bytes += sample.mem_swap_bytes;

        agg.io_rchar_bytes_total += sample.io_rchar_bytes_total;
        agg.io_wchar_bytes_total += sample.io_wchar_bytes_total;
        agg.io_syscr_total += sample.io_syscr_total;
        agg.io_syscw_total += sample.io_syscw_total;
        agg.io_read_bytes_total += sample.io_read_bytes_total;
        agg.io_write_bytes_total += sample.io_write_bytes_total;
        agg.io_cancelled_write_bytes_total += sample.io_cancelled_write_bytes_total;

        if let Some(start) = sample.start_time_seconds {
            oldest_start = Some(match oldest_start {
                Some(cur) if cur <= start => cur,
                _ => start,
            });
        }
    }

    if !any {
        // Skupina je prázdná → všechno vynulujeme, ať je to jasně vidět.
        metrics.cpu_user_seconds.set(0.0);
        metrics.cpu_system_seconds.set(0.0);
        metrics.start_time_seconds.set(0.0);
        metrics.uptime_seconds.set(0.0);

        metrics.mem_rss_bytes.set(0.0);
        metrics.mem_vms_bytes.set(0.0);
        metrics.mem_swap_bytes.set(0.0);

        metrics.io_rchar_bytes_total.set(0.0);
        metrics.io_wchar_bytes_total.set(0.0);
        metrics.io_syscr_total.set(0.0);
        metrics.io_syscw_total.set(0.0);
        metrics.io_read_bytes_total.set(0.0);
        metrics.io_write_bytes_total.set(0.0);
        metrics.io_cancelled_write_bytes_total.set(0.0);

        return Ok(());
    }

    metrics.cpu_user_seconds.set(agg.cpu_user_seconds);
    metrics.cpu_system_seconds.set(agg.cpu_system_seconds);

    metrics.mem_rss_bytes.set(agg.mem_rss_bytes);
    metrics.mem_vms_bytes.set(agg.mem_vms_bytes);
    metrics.mem_swap_bytes.set(agg.mem_swap_bytes);

    metrics.io_rchar_bytes_total.set(agg.io_rchar_bytes_total);
    metrics.io_wchar_bytes_total.set(agg.io_wchar_bytes_total);
    metrics.io_syscr_total.set(agg.io_syscr_total);
    metrics.io_syscw_total.set(agg.io_syscw_total);
    metrics.io_read_bytes_total.set(agg.io_read_bytes_total);
    metrics.io_write_bytes_total.set(agg.io_write_bytes_total);
    metrics
        .io_cancelled_write_bytes_total
        .set(agg.io_cancelled_write_bytes_total);

    if let Some(start_time) = oldest_start {
        metrics.start_time_seconds.set(start_time);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        metrics.uptime_seconds.set(now - start_time);
    }

    Ok(())
}

/// Aktualizace metrik podle ProcessTarget:
///  - Single(pid)  → agregace nad jedním PIDem (kompatibilní s TARGET_PID)
///  - PidList([...]) → agregace nad explicitním seznamem PIDů
///  - Regex(re) → najdeme PIDy v /proc podle regexu a agregujeme přes ně
pub fn update_for_target(metrics: &ProcessMetrics, target: &ProcessTarget) -> Result<()> {
    match target {
        ProcessTarget::Single(pid) => update_for_pids(metrics, &[*pid]),
        ProcessTarget::PidList(pids) => update_for_pids(metrics, pids),
        ProcessTarget::Regex(re) => {
            let pids = find_pids_by_regex(re)?;
            update_for_pids(metrics, &pids)
        }
    }
}

fn read_proc_sample(pid: i32) -> Result<ProcSample> {
    let mut sample = ProcSample::default();

    // --- /proc/<pid>/stat ---
    let stat_path = PathBuf::from(format!("/proc/{}/stat", pid));
    let content = read_to_string(&stat_path).context("read /proc/<pid>/stat")?;
    let parts: Vec<&str> = content.split_whitespace().collect();

    if parts.len() > 21 {
        // proc(5): utime=14, stime=15, starttime=22 (indexy 13,14,21)
        let utime_ticks: f64 = parts[13].parse::<u64>().unwrap_or(0) as f64;
        let stime_ticks: f64 = parts[14].parse::<u64>().unwrap_or(0) as f64;
        let start_ticks: f64 = parts[21].parse::<u64>().unwrap_or(0) as f64;

        let ticks_per_sec = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
        if ticks_per_sec > 0.0 {
            sample.cpu_user_seconds = utime_ticks / ticks_per_sec;
            sample.cpu_system_seconds = stime_ticks / ticks_per_sec;

            // boot time z /proc/stat (btime)
            let boot_time = std::fs::read_to_string("/proc/stat")?
                .lines()
                .find(|l| l.starts_with("btime "))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);

            let start_time = boot_time as f64 + start_ticks / ticks_per_sec;
            sample.start_time_seconds = Some(start_time);
        }
    }

    // --- /proc/<pid>/status ---
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

    sample.mem_rss_bytes = (rss_kb * 1024) as f64;
    sample.mem_vms_bytes = (vms_kb * 1024) as f64;
    sample.mem_swap_bytes = (swap_kb * 1024) as f64;

    // --- /proc/<pid>/io ---
    let io_path = PathBuf::from(format!("/proc/{}/io", pid));
    let content = match read_to_string(&io_path) {
        Ok(c) => c,
        Err(_) => String::new(), // některá prostředí /proc/<pid>/io nemají - IO metriky zůstanou 0
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

    sample.io_rchar_bytes_total = rchar as f64;
    sample.io_wchar_bytes_total = wchar as f64;
    sample.io_syscr_total = syscr as f64;
    sample.io_syscw_total = syscw as f64;
    sample.io_read_bytes_total = read_bytes as f64;
    sample.io_write_bytes_total = write_bytes as f64;
    sample.io_cancelled_write_bytes_total = cancelled_write_bytes as f64;

    Ok(sample)
}

fn grab_kb(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
}

/// Jak často logovat info o počtu matchnutých PIDů.
const REGEX_LOG_THROTTLE: Duration = Duration::from_secs(300); // 5 minut

static LAST_REGEX_LOG: Lazy<Mutex<Option<Instant>>> = Lazy::new(|| Mutex::new(None));

fn should_log_regex_match() -> bool {
    let now = Instant::now();
    let mut guard = LAST_REGEX_LOG
        .lock()
        .expect("LAST_REGEX_LOG mutex poisoned");

    match *guard {
        None => {
            // ještě jsme nikdy nelogovali → teď ano
            *guard = Some(now);
            true
        }
        Some(last) => {
            if now.duration_since(last) >= REGEX_LOG_THROTTLE {
                // od posledního logu uplynulo >= 5 minut → logneme a obnovíme čas
                *guard = Some(now);
                true
            } else {
                // moc brzo → nelogovat
                false
            }
        }
    }
}

fn find_pids_by_regex(re: &regex::Regex) -> Result<Vec<i32>> {
    let mut result = Vec::new();

    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();

        // Zajímá nás jen čistě číselný název adresáře = PID
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        let pid: i32 = match name.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Nejprve zkusíme cmdline
        let cmdline_path = format!("/proc/{}/cmdline", pid);
        let cmdline = fs::read_to_string(&cmdline_path).unwrap_or_default();
        let cmdline_pretty = cmdline.replace('\0', " ");

        debug!(pid, ?cmdline_pretty, "testing pid against regex");

        if re.is_match(&cmdline_pretty) {
            result.push(pid);
            continue;
        }

        // Fallback na /proc/<pid>/comm - typicky obsahuje „nginx“ atd.
        let comm_path = format!("/proc/{}/comm", pid);
        let comm = fs::read_to_string(&comm_path).unwrap_or_default();
        let comm_trimmed = comm.trim();

        debug!(pid, ?comm_trimmed, "testing comm against regex");

        if re.is_match(comm_trimmed) {
            result.push(pid);
        }
    }

    // INFO log max. 1× za 5 minut
    if should_log_regex_match() {
        info!(
            regex = %re.as_str(),
            matched = result.len(),
            "TARGET_PID_REGEXP matched processes"
        );
    }

    Ok(result)
}
