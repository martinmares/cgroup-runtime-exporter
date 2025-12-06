use std::{collections::HashMap, env, net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use regex::Regex;
use tracing::warn;

#[derive(Debug, Clone)]
pub enum ProcessTarget {
    /// Původní chování - jeden konkrétní PID (TARGET_PID)
    Single(i32),
    /// Explicitní seznam PIDů (TARGET_PID_LIST)
    PidList(Vec<i32>),
    /// Regex pro výběr procesů podle cmdline/comm (TARGET_PID_REGEXP)
    Regex(Regex),
}

#[derive(Debug, Clone)]
pub struct Config {
    pub listen_addr: SocketAddr,
    pub cgroup_root: PathBuf,
    pub downward_dir: Option<PathBuf>,

    /// Jaké procesy sledovat v /proc (Single PID, list, nebo regexp).
    pub process_target: Option<ProcessTarget>,

    /// Prefix / namespace pro všechny metriky (např. "nac", "kip")
    pub metrics_prefix: Option<String>,

    /// Statické labely nalepené na všechny metriky
    pub static_labels: HashMap<String, String>,

    /// K8s CPU requests/limits v millicores (z env, pokud jsou)
    pub cpu_requests_mcpu: Option<f64>,
    pub cpu_limits_mcpu: Option<f64>,

    /// K8s memory requests/limits v bajtech (z env, pokud jsou)
    pub memory_requests_bytes: Option<f64>,
    pub memory_limits_bytes: Option<f64>,

    /// Interval (v sekundách), jak často se mají metriky aktualizovat na pozadí.
    /// Default 5s, minimum 1s.
    pub update_interval_secs: u64,

    /// Network interface, který chceme sledovat (např. "eth0").
    /// Default: "eth0".
    pub net_interface: String,

    /// Jméno nodu (pokud je k dispozici z env NODE_NAME)
    pub node_name: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Config> {
        // --- základní věci ---
        let listen = env::var("EXPORTER_LISTEN").unwrap_or_else(|_| "0.0.0.0:9100".to_string());
        let listen_addr: SocketAddr = listen.parse().context("EXPORTER_LISTEN parse error")?;

        let cgroup_root = env::var("CGROUP_ROOT").unwrap_or_else(|_| "/sys/fs/cgroup".to_string());

        let downward_dir = env::var("DOWNWARD_API_DIR").ok().map(PathBuf::from);

        // --- Process target selection (PID / LIST / REGEXP) ---
        let target_pid_env = env::var("TARGET_PID").ok().filter(|v| !v.trim().is_empty());
        let target_pid_list_env = env::var("TARGET_PID_LIST")
            .ok()
            .filter(|v| !v.trim().is_empty());
        let target_pid_regexp_env = env::var("TARGET_PID_REGEXP")
            .ok()
            .filter(|v| !v.trim().is_empty());

        // Priorita: TARGET_PID > TARGET_PID_LIST > TARGET_PID_REGEXP
        let process_target = if let Some(pid_str) = target_pid_env {
            if target_pid_list_env.is_some() {
                warn!(
                    "Both TARGET_PID and TARGET_PID_LIST are set - using TARGET_PID and ignoring TARGET_PID_LIST"
                );
            }
            if target_pid_regexp_env.is_some() {
                warn!(
                    "Both TARGET_PID and TARGET_PID_REGEXP are set - using TARGET_PID and ignoring TARGET_PID_REGEXP"
                );
            }

            let pid: i32 = pid_str
                .parse()
                .context("TARGET_PID parse error (expected integer PID)")?;
            Some(ProcessTarget::Single(pid))
        } else if let Some(list_str) = target_pid_list_env {
            if target_pid_regexp_env.is_some() {
                warn!(
                    "Both TARGET_PID_LIST and TARGET_PID_REGEXP are set - using TARGET_PID_LIST and ignoring TARGET_PID_REGEXP"
                );
            }

            let mut pids = Vec::new();
            for part in list_str.split(',') {
                let trimmed = part.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let pid: i32 = trimmed
                    .parse()
                    .with_context(|| format!("TARGET_PID_LIST parse error at '{trimmed}'"))?;
                pids.push(pid);
            }

            if pids.is_empty() {
                warn!(
                    "TARGET_PID_LIST is set but no valid PIDs were parsed - process metrics will be disabled"
                );
                None
            } else {
                Some(ProcessTarget::PidList(pids))
            }
        } else if let Some(re_str) = target_pid_regexp_env {
            let re = Regex::new(&re_str).context("TARGET_PID_REGEXP invalid regex")?;
            Some(ProcessTarget::Regex(re))
        } else {
            None
        };

        // --- Metrics prefix / labels / K8s resource hints ---
        let metrics_prefix = env::var("METRICS_PREFIX")
            .ok()
            .and_then(normalize_prefix)
            .or_else(|| {
                env::var("METRICS_NAMESPACE")
                    .ok()
                    .and_then(normalize_prefix)
            });

        let static_labels =
            parse_static_labels(&env::var("METRICS_STATIC_LABELS").unwrap_or_default());

        let cpu_requests_mcpu = env::var("CPU_REQUESTS_MCPU")
            .ok()
            .and_then(|s| s.parse::<f64>().ok());

        let cpu_limits_mcpu = env::var("CPU_LIMITS_MCPU")
            .ok()
            .and_then(|s| s.parse::<f64>().ok());

        let memory_requests_bytes = env::var("MEMORY_REQUESTS_MIB")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|mb| mb * 1024.0 * 1024.0); // 1 MiB → bajty

        let memory_limits_bytes = env::var("MEMORY_LIMITS_MIB")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|mb| mb * 1024.0 * 1024.0);

        let update_interval_secs = env::var("METRICS_UPDATE_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(5)
            .max(1); // nechceme 0 → busy loop

        let net_interface = env::var("NET_INTERFACE").unwrap_or_else(|_| "eth0".to_string());

        let node_name = env::var("NODE_NAME").ok().filter(|s| !s.is_empty());

        Ok(Self {
            listen_addr,
            cgroup_root: PathBuf::from(cgroup_root),
            downward_dir,
            process_target,
            metrics_prefix,
            static_labels,
            cpu_requests_mcpu,
            cpu_limits_mcpu,
            memory_requests_bytes,
            memory_limits_bytes,
            update_interval_secs,
            net_interface,
            node_name,
        })
    }
}

fn parse_static_labels(s: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if s.trim().is_empty() {
        return map;
    }

    for pair in s.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        if let Some((k, v)) = pair.split_once('=') {
            let key = k.trim();
            let val = v.trim();
            if !key.is_empty() {
                map.insert(key.to_string(), val.to_string());
            }
        }
    }

    map
}

fn normalize_prefix(raw: String) -> Option<String> {
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return None;
    }

    // ořežeme všechny trailing '_' a NEpřidáváme žádný zpátky
    let trimmed = trimmed.trim_end_matches('_');
    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed.to_string())
}
