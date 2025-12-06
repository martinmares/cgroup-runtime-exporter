//! TCP stack metrics based on /proc/net/tcp{,6}.

use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufRead, BufReader},
};

use anyhow::{Context, Result};

use crate::metrics::TcpMetrics;

/// Aktualizuje metriky TCP spojení (podle stavu a IP verze).
///
/// Pozn.: IPv4 spojení vedená přes IPv6 sockety (IPv4-mapped IPv6
/// adresy `::ffff:W.X.Y.Z`) se v /proc/net/tcp6 objevují jako IPv6.
/// Abychom dostali realistické počty IPv4/IPv6 spojení, rozeznáváme
/// tyto adresy a počítáme je jako `ip_version = "4"`.
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

/// Načte /proc/net/tcp{,6} a naplní počty spojení podle stavu a IP verze.
///
/// U `/proc/net/tcp6` navíc detekuje IPv4-mapped IPv6 adresy (prefix
/// `0000000000000000FFFF0000`) a počítá taková spojení jako IPv4.
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
            // Ve /proc/net/tcp6 mohou být IPv4 spojení zabalená jako
            // IPv4-mapped IPv6 (::ffff:W.X.Y.Z). Kernel je pak zapisuje
            // do tcp6 s prefixem 0000000000000000FFFF0000 před IPv4
            // adresou. Takové položky počítáme jako IPv4.
            let mut effective_ip_version = ip_version;

            if ip_version == "6" {
                let local = cols.get(1).copied().unwrap_or_default();
                let remote = cols.get(2).copied().unwrap_or_default();

                if is_ipv4_mapped_addr(local) || is_ipv4_mapped_addr(remote) {
                    effective_ip_version = "4";
                }
            }

            *counts.entry((code, effective_ip_version)).or_insert(0) += 1;
        }
    }

    Ok(())
}

/// Vrací `true`, pokud je adresa z /proc/net/tcp6 ve formátu
/// IPv4-mapped IPv6 (`::ffff:W.X.Y.Z`).
fn is_ipv4_mapped_addr(addr_port: &str) -> bool {
    // Formát je 32 hex znaků + ":" + port, např.:
    // 0000000000000000FFFF00007095FB3A:0050
    // kde prefix 0000000000000000FFFF0000 označuje IPv4-mapped adresu
    // a posledních 8 hex znaků je IPv4 adresa v little-endian.
    let (addr_hex, _) = match addr_port.split_once(':') {
        Some(parts) => parts,
        None => return false,
    };

    if addr_hex.len() < 24 {
        return false;
    }

    addr_hex[..24].eq_ignore_ascii_case("0000000000000000FFFF0000")
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
