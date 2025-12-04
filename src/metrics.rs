use std::collections::HashMap;

use anyhow::{Context, Result};
use prometheus::{Gauge, GaugeVec, IntGauge, IntGaugeVec, Opts, Registry};

use crate::config::Config;

#[allow(dead_code)]
pub struct ResourceMetrics {
    pub cpu_requests_mcpu: Gauge,
    pub cpu_limits_mcpu: Gauge,
    pub memory_requests_bytes: Gauge,
    pub memory_limits_bytes: Gauge,
}

pub struct CgroupMetrics {
    pub cpu_usage_seconds: Gauge,
    pub cpu_user_seconds: Gauge,
    pub cpu_system_seconds: Gauge,
    pub cpu_nr_periods: IntGauge,
    pub cpu_nr_throttled: IntGauge,
    pub cpu_throttled_seconds: Gauge,
    pub cpu_limit_cores: Gauge,

    pub mem_current_bytes: Gauge,
    pub mem_peak_bytes: Gauge,
    pub mem_max_bytes: Gauge,
    pub mem_high_bytes: Gauge,
    pub mem_low_bytes: Gauge,
    pub mem_events_total: IntGaugeVec,
}

pub struct ProcessMetrics {
    pub cpu_user_seconds: Gauge,
    pub cpu_system_seconds: Gauge,
    pub start_time_seconds: Gauge,

    pub mem_rss_bytes: Gauge,
    pub mem_vms_bytes: Gauge,
    pub mem_swap_bytes: Gauge,

    // IO z /proc/<pid>/io
    pub io_rchar_bytes_total: Gauge,
    pub io_wchar_bytes_total: Gauge,
    pub io_syscr_total: Gauge,
    pub io_syscw_total: Gauge,
    pub io_read_bytes_total: Gauge,
    pub io_write_bytes_total: Gauge,
    pub io_cancelled_write_bytes_total: Gauge,

    pub uptime_seconds: Gauge, // <- NOVÉ
}

/// Síťové metriky pro jeden interface (NET_INTERFACE).
pub struct NetMetrics {
    pub rx_bytes_total: Gauge,
    pub tx_bytes_total: Gauge,
    pub rx_packets_total: Gauge,
    pub tx_packets_total: Gauge,
    pub rx_errors_total: Gauge,
    pub tx_errors_total: Gauge,
    pub rx_dropped_total: Gauge,
    pub tx_dropped_total: Gauge,
}
#[allow(dead_code)]
pub struct HostMetrics {
    /// CPU time per mode as reported by /proc/stat (seconds).
    /// Labels: cpu="all", mode="user|nice|system|idle|iowait|irq|softirq|steal|guest|guest_nice"
    pub cpu_seconds_total: GaugeVec,

    /// Memory totals from /proc/meminfo (bytes).
    pub memory_total_bytes: Gauge,
    pub memory_free_bytes: Gauge,
    pub memory_available_bytes: Gauge,
    pub memory_cached_bytes: Gauge,
    pub memory_buffers_bytes: Gauge,
    pub swap_total_bytes: Gauge,
    pub swap_free_bytes: Gauge,
}

/// TCP connection counters per state and IP version as seen in /proc/net/tcp{,6}.
/// Labels:
///   state="ESTABLISHED|SYN_SENT|...|CLOSING|LISTEN|UNKNOWN"
///   ip_version="4" or "6"
#[allow(dead_code)]
pub struct TcpMetrics {
    pub connections: IntGaugeVec,
}

pub struct Metrics {
    pub registry: Registry,
    pub cgroup: CgroupMetrics,
    pub process: ProcessMetrics,
    pub net: NetMetrics,
    #[allow(dead_code)]
    pub host: HostMetrics,
    #[allow(dead_code)]
    pub tcp: TcpMetrics,
    /// DownwardAPI info: field + value, vždy 1 sample
    pub downward_info: IntGaugeVec,
    #[allow(dead_code)]
    pub resources: Option<ResourceMetrics>, // může být None, když env chybí
}

fn gauge_with_const_label(
    registry: &Registry,
    cfg: &Config,
    name: &str,
    help: &str,
    extra_label: Option<(&str, &str)>,
) -> Result<Gauge> {
    // vyrobíme kopii static_labels a případně přidáme node_name
    let mut labels = cfg.static_labels.clone();
    if let Some((k, v)) = extra_label {
        labels.insert(k.to_string(), v.to_string());
    }

    let opts = make_opts(name, help, cfg.metrics_prefix.clone(), labels);
    let g = Gauge::with_opts(opts).context(format!("create gauge {}", name))?;
    registry
        .register(Box::new(g.clone()))
        .context(format!("register gauge {}", name))?;
    Ok(g)
}

fn gauge_vec_with_const_label(
    registry: &Registry,
    cfg: &Config,
    name: &str,
    help: &str,
    label_names: &[&str],
    extra_label: Option<(&str, &str)>,
) -> Result<GaugeVec> {
    let mut labels = cfg.static_labels.clone();
    if let Some((k, v)) = extra_label {
        labels.insert(k.to_string(), v.to_string());
    }

    let opts = make_opts(name, help, cfg.metrics_prefix.clone(), labels);
    let v = GaugeVec::new(opts, label_names).context(format!("create gauge vec {}", name))?;
    registry
        .register(Box::new(v.clone()))
        .context(format!("register gauge vec {}", name))?;
    Ok(v)
}

impl Metrics {
    pub fn new(cfg: &Config) -> Result<Self> {
        let registry = Registry::new_custom(None, None)?;

        let cgroup = CgroupMetrics::new(&registry, cfg)?;
        let process = ProcessMetrics::new(&registry, cfg)?;
        let net = NetMetrics::new(&registry, cfg)?;
        let host = HostMetrics::new(&registry, cfg)?;
        let tcp = TcpMetrics::new(&registry, cfg)?;
        let downward_info = downward_info_metric(&registry, cfg)?;
        let resources = ResourceMetrics::new(&registry, cfg)?; // Option<…>

        Ok(Self {
            registry,
            cgroup,
            process,
            net,
            host,
            tcp,
            downward_info,
            resources,
        })
    }
}

impl CgroupMetrics {
    pub fn new(registry: &Registry, cfg: &Config) -> Result<Self> {
        let cpu_usage_seconds = gauge(
            registry,
            cfg,
            "cgroup_cpu_usage_seconds",
            "Total CPU time consumed by current cgroup (usage_usec / 1e6)",
        )?;

        let cpu_user_seconds = gauge(
            registry,
            cfg,
            "cgroup_cpu_user_seconds",
            "User CPU time for current cgroup (user_usec / 1e6)",
        )?;

        let cpu_system_seconds = gauge(
            registry,
            cfg,
            "cgroup_cpu_system_seconds",
            "System CPU time for current cgroup (system_usec / 1e6)",
        )?;

        let cpu_nr_periods = int_gauge(
            registry,
            cfg,
            "cgroup_cpu_nr_periods_total",
            "Number of elapsed enforcement periods for current cgroup",
        )?;

        let cpu_nr_throttled = int_gauge(
            registry,
            cfg,
            "cgroup_cpu_nr_throttled_total",
            "Number of throttled periods for current cgroup",
        )?;

        let cpu_throttled_seconds = gauge(
            registry,
            cfg,
            "cgroup_cpu_throttled_seconds",
            "Total time duration the cgroup has been throttled (throttled_usec / 1e6)",
        )?;

        let cpu_limit_cores = gauge(
            registry,
            cfg,
            "cgroup_cpu_limit_cores",
            "Effective CPU limit in cores derived from cpu.max (quota/period), +Inf if unlimited",
        )?;

        let mem_current_bytes = gauge(
            registry,
            cfg,
            "cgroup_memory_current_bytes",
            "Current memory usage in bytes (memory.current)",
        )?;

        let mem_peak_bytes = gauge(
            registry,
            cfg,
            "cgroup_memory_peak_bytes",
            "Peak memory usage in bytes (memory.peak)",
        )?;

        let mem_max_bytes = gauge(
            registry,
            cfg,
            "cgroup_memory_max_bytes",
            "Memory limit in bytes (memory.max or +Inf)",
        )?;

        let mem_high_bytes = gauge(
            registry,
            cfg,
            "cgroup_memory_high_bytes",
            "High memory threshold in bytes (memory.high)",
        )?;

        let mem_low_bytes = gauge(
            registry,
            cfg,
            "cgroup_memory_low_bytes",
            "Low memory threshold in bytes (memory.low)",
        )?;

        let mem_events_total = int_gauge_vec(
            registry,
            cfg,
            "cgroup_memory_events_total",
            "Cumulative memory events from memory.events",
            &["type"],
        )?;

        Ok(Self {
            cpu_usage_seconds,
            cpu_user_seconds,
            cpu_system_seconds,
            cpu_nr_periods,
            cpu_nr_throttled,
            cpu_throttled_seconds,
            cpu_limit_cores,
            mem_current_bytes,
            mem_peak_bytes,
            mem_max_bytes,
            mem_high_bytes,
            mem_low_bytes,
            mem_events_total,
        })
    }
}

impl ProcessMetrics {
    pub fn new(registry: &Registry, cfg: &Config) -> Result<Self> {
        let cpu_user_seconds = gauge(
            registry,
            cfg,
            "process_cpu_user_seconds",
            "User CPU time for observed process (/proc/<pid>/stat)",
        )?;

        let cpu_system_seconds = gauge(
            registry,
            cfg,
            "process_cpu_system_seconds",
            "System CPU time for observed process",
        )?;

        let start_time_seconds = gauge(
            registry,
            cfg,
            "process_start_time_seconds",
            "Start time of observed process since epoch seconds",
        )?;

        let mem_rss_bytes = gauge(
            registry,
            cfg,
            "process_memory_rss_bytes",
            "Resident set size of observed process",
        )?;

        let mem_vms_bytes = gauge(
            registry,
            cfg,
            "process_memory_vms_bytes",
            "Virtual memory size of observed process",
        )?;

        let mem_swap_bytes = gauge(
            registry,
            cfg,
            "process_memory_swap_bytes",
            "Swap usage of observed process",
        )?;

        let io_rchar_bytes_total = gauge(
            registry,
            cfg,
            "process_io_rchar_bytes_total",
            "Characters read (rchar) from /proc/<pid>/io",
        )?;

        let io_wchar_bytes_total = gauge(
            registry,
            cfg,
            "process_io_wchar_bytes_total",
            "Characters written (wchar) from /proc/<pid>/io",
        )?;

        let io_syscr_total = gauge(
            registry,
            cfg,
            "process_io_syscr_total",
            "Number of read syscalls (syscr) from /proc/<pid>/io",
        )?;

        let io_syscw_total = gauge(
            registry,
            cfg,
            "process_io_syscw_total",
            "Number of write syscalls (syscw) from /proc/<pid>/io",
        )?;

        let io_read_bytes_total = gauge(
            registry,
            cfg,
            "process_io_read_bytes_total",
            "Bytes read from storage (read_bytes) from /proc/<pid>/io",
        )?;

        let io_write_bytes_total = gauge(
            registry,
            cfg,
            "process_io_write_bytes_total",
            "Bytes written to storage (write_bytes) from /proc/<pid>/io",
        )?;

        let io_cancelled_write_bytes_total = gauge(
            registry,
            cfg,
            "process_io_cancelled_write_bytes_total",
            "Bytes of cancelled write IO (cancelled_write_bytes) from /proc/<pid>/io",
        )?;

        let uptime_seconds = gauge(
            registry,
            cfg,
            "process_uptime_seconds",
            "Time in seconds the observed process has been running",
        )?;

        Ok(Self {
            cpu_user_seconds,
            cpu_system_seconds,
            start_time_seconds,
            mem_rss_bytes,
            mem_vms_bytes,
            mem_swap_bytes,
            io_rchar_bytes_total,
            io_wchar_bytes_total,
            io_syscr_total,
            io_syscw_total,
            io_read_bytes_total,
            io_write_bytes_total,
            io_cancelled_write_bytes_total,
            uptime_seconds, // <- přidat
        })
    }
}

impl NetMetrics {
    pub fn new(registry: &Registry, cfg: &Config) -> Result<Self> {
        let rx_bytes_total = gauge(
            registry,
            cfg,
            "pod_network_receive_bytes_total",
            "Network bytes received on NET_INTERFACE as seen from container (/sys/class/net/<iface>/statistics/rx_bytes)",
        )?;
        let tx_bytes_total = gauge(
            registry,
            cfg,
            "pod_network_transmit_bytes_total",
            "Network bytes transmitted on NET_INTERFACE (/sys/class/net/<iface>/statistics/tx_bytes)",
        )?;

        let rx_packets_total = gauge(
            registry,
            cfg,
            "pod_network_receive_packets_total",
            "Network packets received on NET_INTERFACE (/sys/class/net/<iface>/statistics/rx_packets)",
        )?;
        let tx_packets_total = gauge(
            registry,
            cfg,
            "pod_network_transmit_packets_total",
            "Network packets transmitted on NET_INTERFACE (/sys/class/net/<iface>/statistics/tx_packets)",
        )?;

        let rx_errors_total = gauge(
            registry,
            cfg,
            "pod_network_receive_errors_total",
            "Receive errors on NET_INTERFACE (/sys/class/net/<iface>/statistics/rx_errors)",
        )?;
        let tx_errors_total = gauge(
            registry,
            cfg,
            "pod_network_transmit_errors_total",
            "Transmit errors on NET_INTERFACE (/sys/class/net/<iface>/statistics/tx_errors)",
        )?;

        let rx_dropped_total = gauge(
            registry,
            cfg,
            "pod_network_receive_dropped_total",
            "Dropped receive packets on NET_INTERFACE (/sys/class/net/<iface>/statistics/rx_dropped)",
        )?;
        let tx_dropped_total = gauge(
            registry,
            cfg,
            "pod_network_transmit_dropped_total",
            "Dropped transmit packets on NET_INTERFACE (/sys/class/net/<iface>/statistics/tx_dropped)",
        )?;

        Ok(Self {
            rx_bytes_total,
            tx_bytes_total,
            rx_packets_total,
            tx_packets_total,
            rx_errors_total,
            tx_errors_total,
            rx_dropped_total,
            tx_dropped_total,
        })
    }
}

impl ResourceMetrics {
    pub fn new(registry: &Registry, cfg: &Config) -> Result<Option<Self>> {
        // pokud není nastaveno vůbec nic, metriky ani nevytvářej
        if cfg.cpu_requests_mcpu.is_none()
            && cfg.cpu_limits_mcpu.is_none()
            && cfg.memory_requests_bytes.is_none()
            && cfg.memory_limits_bytes.is_none()
        {
            return Ok(None);
        }

        let cpu_requests_mcpu = gauge(
            registry,
            cfg,
            "k8s_cpu_requests_millicores",
            "Kubernetes CPU requests for this container in millicores",
        )?;

        let cpu_limits_mcpu = gauge(
            registry,
            cfg,
            "k8s_cpu_limits_millicores",
            "Kubernetes CPU limits for this container in millicores",
        )?;

        let memory_requests_bytes = gauge(
            registry,
            cfg,
            "k8s_memory_requests_bytes",
            "Kubernetes memory requests for this container in bytes",
        )?;

        let memory_limits_bytes = gauge(
            registry,
            cfg,
            "k8s_memory_limits_bytes",
            "Kubernetes memory limits for this container in bytes",
        )?;

        // naplníme konstantní hodnoty (pokud existují)
        if let Some(v) = cfg.cpu_requests_mcpu {
            cpu_requests_mcpu.set(v);
        }
        if let Some(v) = cfg.cpu_limits_mcpu {
            cpu_limits_mcpu.set(v);
        }
        if let Some(v) = cfg.memory_requests_bytes {
            memory_requests_bytes.set(v);
        }
        if let Some(v) = cfg.memory_limits_bytes {
            memory_limits_bytes.set(v);
        }

        Ok(Some(Self {
            cpu_requests_mcpu,
            cpu_limits_mcpu,
            memory_requests_bytes,
            memory_limits_bytes,
        }))
    }
}

impl HostMetrics {
    pub fn new(registry: &Registry, cfg: &Config) -> Result<Self> {
        // Pokud máme NODE_NAME, budeme ho lepit jako const label node_name="..."
        let node_label = cfg.node_name.as_deref().map(|v| ("node_name", v));

        let cpu_seconds_total = gauge_vec_with_const_label(
            registry,
            cfg,
            "host_cpu_seconds_total",
            "Host CPU time per mode as read from /proc/stat (seconds)",
            &["cpu", "mode"],
            node_label,
        )?;

        let memory_total_bytes = gauge_with_const_label(
            registry,
            cfg,
            "host_memory_total_bytes",
            "MemTotal from /proc/meminfo (bytes)",
            node_label,
        )?;

        let memory_free_bytes = gauge_with_const_label(
            registry,
            cfg,
            "host_memory_free_bytes",
            "MemFree from /proc/meminfo (bytes)",
            node_label,
        )?;

        let memory_available_bytes = gauge_with_const_label(
            registry,
            cfg,
            "host_memory_available_bytes",
            "MemAvailable from /proc/meminfo (bytes)",
            node_label,
        )?;

        let memory_cached_bytes = gauge_with_const_label(
            registry,
            cfg,
            "host_memory_cached_bytes",
            "Cached from /proc/meminfo (bytes)",
            node_label,
        )?;

        let memory_buffers_bytes = gauge_with_const_label(
            registry,
            cfg,
            "host_memory_buffers_bytes",
            "Buffers from /proc/meminfo (bytes)",
            node_label,
        )?;

        let swap_total_bytes = gauge_with_const_label(
            registry,
            cfg,
            "host_swap_total_bytes",
            "SwapTotal from /proc/meminfo (bytes)",
            node_label,
        )?;

        let swap_free_bytes = gauge_with_const_label(
            registry,
            cfg,
            "host_swap_free_bytes",
            "SwapFree from /proc/meminfo (bytes)",
            node_label,
        )?;

        Ok(Self {
            cpu_seconds_total,
            memory_total_bytes,
            memory_free_bytes,
            memory_available_bytes,
            memory_cached_bytes,
            memory_buffers_bytes,
            swap_total_bytes,
            swap_free_bytes,
        })
    }
}

impl TcpMetrics {
    pub fn new(registry: &Registry, cfg: &Config) -> Result<Self> {
        let connections = int_gauge_vec(
            registry,
            cfg,
            "pod_tcp_connections",
            "Number of TCP connections for this pod by state and IP version from /proc/net/tcp{,6}",
            &["state", "ip_version"],
        )?;

        Ok(Self { connections })
    }
}

fn downward_info_metric(registry: &Registry, cfg: &Config) -> Result<IntGaugeVec> {
    let opts = make_opts(
        "kubernetes_downward_info",
        "Downward API fields exposed as labels; value is always 1.",
        cfg.metrics_prefix.clone(),
        cfg.static_labels.clone(),
    );

    let gauge_vec =
        IntGaugeVec::new(opts, &["field", "value"]).context("create downward_info gauge vec")?;

    registry
        .register(Box::new(gauge_vec.clone()))
        .context("register downward_info")?;

    Ok(gauge_vec)
}

// ---- helpers na tvorbu metrik ----

fn make_opts(
    name: &str,
    help: &str,
    namespace: Option<String>,
    const_labels: HashMap<String, String>,
) -> Opts {
    let mut opts = Opts::new(name, help);
    if let Some(ns) = namespace {
        if !ns.is_empty() {
            opts = opts.namespace(ns);
        }
    }
    if !const_labels.is_empty() {
        opts = opts.const_labels(const_labels);
    }
    opts
}

fn gauge(registry: &Registry, cfg: &Config, name: &str, help: &str) -> Result<Gauge> {
    let opts = make_opts(
        name,
        help,
        cfg.metrics_prefix.clone(),
        cfg.static_labels.clone(),
    );
    let g = Gauge::with_opts(opts).context(format!("create gauge {}", name))?;
    registry
        .register(Box::new(g.clone()))
        .context(format!("register gauge {}", name))?;
    Ok(g)
}

fn int_gauge(registry: &Registry, cfg: &Config, name: &str, help: &str) -> Result<IntGauge> {
    let opts = make_opts(
        name,
        help,
        cfg.metrics_prefix.clone(),
        cfg.static_labels.clone(),
    );
    let g = IntGauge::with_opts(opts).context(format!("create int gauge {}", name))?;
    registry
        .register(Box::new(g.clone()))
        .context(format!("register int gauge {}", name))?;
    Ok(g)
}

fn int_gauge_vec(
    registry: &Registry,
    cfg: &Config,
    name: &str,
    help: &str,
    labels: &[&str],
) -> Result<IntGaugeVec> {
    let opts = make_opts(
        name,
        help,
        cfg.metrics_prefix.clone(),
        cfg.static_labels.clone(),
    );
    let v = IntGaugeVec::new(opts, labels).context(format!("create int gauge vec {}", name))?;
    registry
        .register(Box::new(v.clone()))
        .context(format!("register int gauge vec {}", name))?;
    Ok(v)
}
