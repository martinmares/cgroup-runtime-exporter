//! Host-level metrics (CPU + memory) based on /proc.

use std::{
    fs::File,
    io::{BufRead, BufReader},
};

use anyhow::{Context, Result, bail};

use crate::metrics::HostMetrics;

/// Aktualizuje všechny host metriky (CPU + paměť).
pub fn update(metrics: &HostMetrics) -> Result<()> {
    update_cpu(metrics)?;
    update_memory(metrics)?;
    Ok(())
}

/// Přepočet jiffies -> sekundy.
fn ticks_per_second() -> f64 {
    // Bezpečný fallback, kdyby sysconf selhal.
    let t = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if t <= 0 { 100.0 } else { t as f64 }
}

/// Parsuje agregovaný řádek "cpu  ..." z /proc/stat a uloží ho do metrik.
fn update_cpu(metrics: &HostMetrics) -> Result<()> {
    let file = File::open("/proc/stat").context("open /proc/stat")?;
    let reader = BufReader::new(file);

    let mut cpu_line: Option<String> = None;

    for line_res in reader.lines() {
        let line = line_res.context("read /proc/stat line")?;
        if line.starts_with("cpu ") {
            cpu_line = Some(line);
            break;
        }
    }

    let line = match cpu_line {
        Some(l) => l,
        None => bail!("no aggregated 'cpu ' line in /proc/stat"),
    };

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        bail!("invalid /proc/stat cpu line: {}", line);
    }

    // Hodnoty v jiffies.
    let mut values: Vec<f64> = Vec::with_capacity(parts.len() - 1);
    for s in &parts[1..] {
        match s.parse::<f64>() {
            Ok(v) => values.push(v),
            Err(_) => values.push(0.0),
        }
    }

    // Podle dokumentace jádra:
    // user nice system idle iowait irq softirq steal guest guest_nice
    const MODES: [&str; 10] = [
        "user",
        "nice",
        "system",
        "idle",
        "iowait",
        "irq",
        "softirq",
        "steal",
        "guest",
        "guest_nice",
    ];

    let ticks = ticks_per_second();
    let cpu_label = "all";

    for (idx, mode) in MODES.iter().enumerate() {
        let raw = values.get(idx).copied().unwrap_or(0.0);
        let seconds = raw / ticks;
        metrics
            .cpu_seconds_total
            .with_label_values(&[cpu_label, mode])
            .set(seconds);
    }

    Ok(())
}

/// Parsuje /proc/meminfo a uloží vybrané položky.
fn update_memory(metrics: &HostMetrics) -> Result<()> {
    let file = File::open("/proc/meminfo").context("open /proc/meminfo")?;
    let reader = BufReader::new(file);

    let mut mem_total = None;
    let mut mem_free = None;
    let mut mem_available = None;
    let mut mem_cached = None;
    let mut mem_buffers = None;
    let mut swap_total = None;
    let mut swap_free = None;

    for line_res in reader.lines() {
        let line = line_res.context("read /proc/meminfo line")?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        let key = parts[0].trim_end_matches(':');
        let value_kb: f64 = parts[1].parse().unwrap_or(0.0);
        let value_bytes = value_kb * 1024.0;

        match key {
            "MemTotal" => mem_total = Some(value_bytes),
            "MemFree" => mem_free = Some(value_bytes),
            "MemAvailable" => mem_available = Some(value_bytes),
            "Cached" => mem_cached = Some(value_bytes),
            "Buffers" => mem_buffers = Some(value_bytes),
            "SwapTotal" => swap_total = Some(value_bytes),
            "SwapFree" => swap_free = Some(value_bytes),
            _ => {}
        }
    }

    metrics.memory_total_bytes.set(mem_total.unwrap_or(0.0));
    metrics.memory_free_bytes.set(mem_free.unwrap_or(0.0));
    metrics
        .memory_available_bytes
        .set(mem_available.unwrap_or(0.0));
    metrics.memory_cached_bytes.set(mem_cached.unwrap_or(0.0));
    metrics.memory_buffers_bytes.set(mem_buffers.unwrap_or(0.0));
    metrics.swap_total_bytes.set(swap_total.unwrap_or(0.0));
    metrics.swap_free_bytes.set(swap_free.unwrap_or(0.0));

    Ok(())
}
