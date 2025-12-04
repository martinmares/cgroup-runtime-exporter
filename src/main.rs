mod cgroup;
mod config;
mod downward;
mod host;
mod logging;
mod metrics;
mod net;
mod procfs;
mod tcp;

use std::{convert::Infallible, net::SocketAddr, sync::Arc, time::Duration};
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;

use anyhow::Result;
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use prometheus::{Encoder, TextEncoder};
use tokio::net::TcpListener;

use crate::{
    cgroup as cgroup_mod, config::Config, downward as downward_mod, host as host_mod,
    metrics::Metrics, net as net_mod, procfs as procfs_mod, tcp as tcp_mod,
};

struct AppState {
    cfg: Config,
    metrics: Metrics,
}

#[tokio::main]
async fn main() -> Result<()> {
    // tracing/logging init
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let cfg = Config::from_env()?;

    let metrics = Metrics::new(&cfg)?;
    let state = Arc::new(AppState { cfg, metrics });

    // DownwardAPI je nepovinné - pokud není DIR, nic se neděje
    if let Some(ref dir) = state.cfg.downward_dir {
        if let Err(e) = downward_mod::init_downward_info(&state.metrics, dir) {
            log_anyhow_with_source!(e, "init downward api info failed");
        }
    }

    // Background update loop - cache metrik
    {
        let state = state.clone();
        tokio::spawn(async move {
            let interval = Duration::from_secs(state.cfg.update_interval_secs);
            loop {
                if let Err(e) = update_metrics(&state) {
                    log_anyhow_with_source!(e, "updating metrics failed");
                }
                debug!(
                    sleep_secs = interval.as_secs(),
                    "metrics updated, going to sleep"
                );

                tokio::time::sleep(interval).await;
            }
        });
    }

    let addr: SocketAddr = state.cfg.listen_addr;
    info!(
        listen_addr = %addr,
        interval_secs = state.cfg.update_interval_secs,
        "starting"
    );

    // hyper 1.x už nemá "Server::bind"; použijeme TcpListener + http1::Builder
    let listener = TcpListener::bind(addr).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let state_clone = state.clone();

        tokio::spawn(async move {
            let service = service_fn(move |req: Request<Incoming>| {
                let state = state_clone.clone();
                async move { handle_request(req, state).await }
            });

            if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                log_error_display!(e, "serving connection failed");
            }
        });
    }
}

fn update_metrics(state: &AppState) -> Result<()> {
    // Cgroup metrics
    if let Err(e) = cgroup_mod::update(&state.metrics.cgroup, &state.cfg.cgroup_root) {
        log_anyhow_with_source!(e, "updating cgroup metrics failed");
    }

    // Per-PID metrics (pokud je nastaven TARGET_PID)
    if let Some(pid) = state.cfg.target_pid {
        if let Err(e) = procfs_mod::update(&state.metrics.process, pid) {
            log_anyhow_with_source!(e, pid = %pid, "updating proc metrics failed");
        }
    }

    // Host (node) metrics – /proc/stat + /proc/meminfo
    if let Err(e) = host_mod::update(&state.metrics.host) {
        log_anyhow_with_source!(e, "updating host metrics failed");
    }

    // TCP stack metrics – /proc/net/tcp{,6}
    if let Err(e) = tcp_mod::update(&state.metrics.tcp) {
        log_anyhow_with_source!(e, "updating tcp metrics failed");
    }

    // Network metrics (per-interface throughput)
    if let Err(e) = net_mod::update(&state.metrics.net, &state.cfg.net_interface) {
        log_anyhow_with_source!(e, iface = %state.cfg.net_interface, "updating net metrics failed");
    }

    Ok(())
}

async fn handle_request(
    req: Request<Incoming>,
    state: Arc<AppState>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path();

    let resp = match path {
        "/metrics" => metrics_response(&state),
        "/healthz" => healthz_response(),
        _ => not_found_response(),
    };

    Ok(resp)
}

fn metrics_response(state: &AppState) -> Response<Full<Bytes>> {
    debug!("scrape requested");
    let encoder = TextEncoder::new();
    let metric_families = state.metrics.registry.gather();

    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        log_error_display!(e, "could not encode metrics");
    }

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", encoder.format_type())
        .body(Full::new(Bytes::from(buffer)))
        .unwrap()
}

fn healthz_response() -> Response<Full<Bytes>> {
    debug!("healthz requested");
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from_static(b"ok\n")))
        .unwrap()
}

fn not_found_response() -> Response<Full<Bytes>> {
    warn!("not_found requested");
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from_static(b"not found\n")))
        .unwrap()
}
