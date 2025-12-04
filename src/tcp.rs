//! TCP stack metrics based on /proc/net/tcp{,6}.

use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufRead, BufReader},
};

use anyhow::{Context, Result};

use crate::metrics::TcpMetrics;

/// Aktualizuje metriky TCP spojení (podle stavu a IP verze).
pub fn update(metrics: &TcpMetrics) -> Result<()> {
    let mut counts: HashMap<(u8, &'static str), i64> = HashMap::new();

    collect_from_path("/proc/net/tcp", "4", &mut counts).context("read /proc/net/tcp")?;

    // IPv6 může být vypnuté - chybu ENOENT ignorujeme.
    match collect_from_path("/proc/net/tcp6", "6", &mut counts) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).context("read /proc/net/tcp6"),
    }

    const IP_VERSIONS: [&str; 2] = ["4", "6"];
    const TCP_STATE_CODES: [u8; 12] = [
        0x01, // ESTABLISHED
        0x02, // SYN_SENT
        0x03, // SYN_RECV
        0x04, // FIN_WAIT1
        0x05, // FIN_WAIT2
        0x06, // TIME_WAIT
        0x07, // CLOSE
        0x08, // CLOSE_WAIT
        0x09, // LAST_ACK
        0x0A, // LISTEN
        0x0B, // CLOSING
        0x0C, // NEW_SYN_RECV
    ];

    for &code in &TCP_STATE_CODES {
        let state = tcp_state_name(code);
        for &ip_version in &IP_VERSIONS {
            let value = *counts.get(&(code, ip_version)).unwrap_or(&0);
            metrics
                .connections
                .with_label_values(&[state, ip_version])
                .set(value);
        }
    }

    Ok(())
}

fn collect_from_path(
    path: &str,
    ip_version: &'static str,
    counts: &mut HashMap<(u8, &'static str), i64>,
) -> io::Result<()> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    for (idx, line_res) in reader.lines().enumerate() {
        let line = line_res?;
        if idx == 0 {
            // hlavička
            continue;
        }

        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() <= 3 {
            continue;
        }

        let st_hex = cols[3];
        if let Ok(code) = u8::from_str_radix(st_hex, 16) {
            *counts.entry((code, ip_version)).or_insert(0) += 1;
        }
    }

    Ok(())
}

fn tcp_state_name(code: u8) -> &'static str {
    match code {
        0x01 => "ESTABLISHED",
        0x02 => "SYN_SENT",
        0x03 => "SYN_RECV",
        0x04 => "FIN_WAIT1",
        0x05 => "FIN_WAIT2",
        0x06 => "TIME_WAIT",
        0x07 => "CLOSE",
        0x08 => "CLOSE_WAIT",
        0x09 => "LAST_ACK",
        0x0A => "LISTEN",
        0x0B => "CLOSING",
        0x0C => "NEW_SYN_RECV",
        _ => "UNKNOWN",
    }
}
