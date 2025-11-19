use std::path::Path;

use anyhow::{Context, Result};

use crate::metrics::CgroupMetrics;

fn read_to_string(path: &Path) -> Result<String> {
    Ok(std::fs::read_to_string(path)?.trim().to_string())
}

pub fn update(metrics: &CgroupMetrics, root: &Path) -> Result<()> {
    // cpu.stat
    let cpu_stat = read_to_string(&root.join("cpu.stat")).context("read cpu.stat")?;

    let mut usage_usec = None;
    let mut user_usec = None;
    let mut system_usec = None;
    let mut nr_periods = None;
    let mut nr_throttled = None;
    let mut throttled_usec = None;

    for line in cpu_stat.lines() {
        let mut parts = line.split_whitespace();
        let key = parts.next().unwrap_or("");
        let val = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
        match key {
            "usage_usec" => usage_usec = Some(val),
            "user_usec" => user_usec = Some(val),
            "system_usec" => system_usec = Some(val),
            "nr_periods" => nr_periods = Some(val),
            "nr_throttled" => nr_throttled = Some(val),
            "throttled_usec" => throttled_usec = Some(val),
            _ => {}
        }
    }

    if let Some(v) = usage_usec {
        metrics.cpu_usage_seconds.set(v as f64 / 1_000_000.0);
    }
    if let Some(v) = user_usec {
        metrics.cpu_user_seconds.set(v as f64 / 1_000_000.0);
    }
    if let Some(v) = system_usec {
        metrics.cpu_system_seconds.set(v as f64 / 1_000_000.0);
    }
    if let Some(v) = nr_periods {
        metrics.cpu_nr_periods.set(v as i64);
    }
    if let Some(v) = nr_throttled {
        metrics.cpu_nr_throttled.set(v as i64);
    }
    if let Some(v) = throttled_usec {
        metrics.cpu_throttled_seconds.set(v as f64 / 1_000_000.0);
    }

    // cpu.max
    let cpu_max = read_to_string(&root.join("cpu.max")).context("read cpu.max")?;
    let parts: Vec<&str> = cpu_max.split_whitespace().collect();
    if parts.len() >= 2 {
        if parts[0] == "max" {
            metrics.cpu_limit_cores.set(f64::INFINITY);
        } else if let (Ok(quota), Ok(period)) = (parts[0].parse::<u64>(), parts[1].parse::<u64>()) {
            if period > 0 {
                let cores = quota as f64 / period as f64;
                metrics.cpu_limit_cores.set(cores);
            }
        }
    }

    // memory.*
    if let Ok(s) = read_to_string(&root.join("memory.current")) {
        if let Ok(v) = s.parse::<u64>() {
            metrics.mem_current_bytes.set(v as f64);
        }
    }
    if let Ok(s) = read_to_string(&root.join("memory.peak")) {
        if let Ok(v) = s.parse::<u64>() {
            metrics.mem_peak_bytes.set(v as f64);
        }
    }
    if let Ok(s) = read_to_string(&root.join("memory.max")) {
        if s == "max" {
            metrics.mem_max_bytes.set(f64::INFINITY);
        } else if let Ok(v) = s.parse::<u64>() {
            metrics.mem_max_bytes.set(v as f64);
        }
    }
    if let Ok(s) = read_to_string(&root.join("memory.high")) {
        if s == "max" {
            metrics.mem_high_bytes.set(f64::INFINITY);
        } else if let Ok(v) = s.parse::<u64>() {
            metrics.mem_high_bytes.set(v as f64);
        }
    }
    if let Ok(s) = read_to_string(&root.join("memory.low")) {
        if s == "max" {
            metrics.mem_low_bytes.set(f64::INFINITY);
        } else if let Ok(v) = s.parse::<u64>() {
            metrics.mem_low_bytes.set(v as f64);
        }
    }

    if let Ok(ev) = read_to_string(&root.join("memory.events")) {
        for line in ev.lines() {
            let mut parts = line.split_whitespace();
            let key = parts.next().unwrap_or("");
            let val = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
            if !key.is_empty() {
                metrics
                    .mem_events_total
                    .with_label_values(&[key])
                    .set(val as i64);
            }
        }
    }

    Ok(())
}
