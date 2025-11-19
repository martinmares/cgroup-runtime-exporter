use std::path::PathBuf;

use anyhow::Result;

use crate::metrics::NetMetrics;

fn read_u64_lossy(path: &PathBuf) -> Option<u64> {
    let s = std::fs::read_to_string(path).ok()?;
    s.trim().parse::<u64>().ok()
}

pub fn update(metrics: &NetMetrics, iface: &str) -> Result<()> {
    if iface.is_empty() {
        // monitoring vypnutý
        return Ok(());
    }

    let base = PathBuf::from(format!("/sys/class/net/{}/statistics", iface));
    if !base.exists() {
        // interface v tomhle net namespace neexistuje - ticho po pěšině
        return Ok(());
    }

    if let Some(v) = read_u64_lossy(&base.join("rx_bytes")) {
        metrics.rx_bytes_total.set(v as f64);
    }
    if let Some(v) = read_u64_lossy(&base.join("tx_bytes")) {
        metrics.tx_bytes_total.set(v as f64);
    }
    if let Some(v) = read_u64_lossy(&base.join("rx_packets")) {
        metrics.rx_packets_total.set(v as f64);
    }
    if let Some(v) = read_u64_lossy(&base.join("tx_packets")) {
        metrics.tx_packets_total.set(v as f64);
    }
    if let Some(v) = read_u64_lossy(&base.join("rx_errors")) {
        metrics.rx_errors_total.set(v as f64);
    }
    if let Some(v) = read_u64_lossy(&base.join("tx_errors")) {
        metrics.tx_errors_total.set(v as f64);
    }
    if let Some(v) = read_u64_lossy(&base.join("rx_dropped")) {
        metrics.rx_dropped_total.set(v as f64);
    }
    if let Some(v) = read_u64_lossy(&base.join("tx_dropped")) {
        metrics.tx_dropped_total.set(v as f64);
    }

    Ok(())
}
