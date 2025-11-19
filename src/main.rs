mod cgroup;
mod config;
mod downward;
mod metrics;
mod net;
mod procfs;

use std::{convert::Infallible, net::SocketAddr, sync::Arc, time::Duration};

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
    cgroup as cgroup_mod, config::Config, downward as downward_mod, metrics::Metrics,
    net as net_mod, procfs as procfs_mod,
};

struct AppState {
    cfg: Config,
    metrics: Metrics,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Config::from_env()?;
    let metrics = Metrics::new(&cfg)?;
    let state = Arc::new(AppState { cfg, metrics });

    // DownwardAPI je nepovinné – pokud není DIR, nic se neděje
    if let Some(ref dir) = state.cfg.downward_dir {
        if let Err(e) = downward_mod::init_downward_info(&state.metrics, dir) {
            eprintln!("failed to init downward api info: {:?}", e);
        }
    }

    // Background update loop – cache metrik
    {
        let state = state.clone();
        tokio::spawn(async move {
            let interval = Duration::from_secs(state.cfg.update_interval_secs);
            loop {
                if let Err(e) = update_metrics(&state) {
                    eprintln!("error updating metrics: {:?}", e);
                }
                tokio::time::sleep(interval).await;
            }
        });
    }

    let addr: SocketAddr = state.cfg.listen_addr;
    println!(
        "Starting exporter on {}, update interval {}s",
        addr, state.cfg.update_interval_secs
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

            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}

fn update_metrics(state: &AppState) -> Result<()> {
    // Cgroup metrics
    if let Err(e) = cgroup_mod::update(&state.metrics.cgroup, &state.cfg.cgroup_root) {
        eprintln!("error updating cgroup metrics: {:?}", e);
    }

    // Per-PID metrics (pokud je nastaven TARGET_PID)
    if let Some(pid) = state.cfg.target_pid {
        if let Err(e) = procfs_mod::update(&state.metrics.process, pid) {
            eprintln!("error updating proc metrics for pid {}: {:?}", pid, e);
        }
    }

    // Network metrics
    if let Err(e) = net_mod::update(&state.metrics.net, &state.cfg.net_interface) {
        eprintln!(
            "error updating net metrics for iface {}: {:?}",
            state.cfg.net_interface, e
        );
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
    let encoder = TextEncoder::new();
    let metric_families = state.metrics.registry.gather();

    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        eprintln!("could not encode metrics: {:?}", e);
    }

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", encoder.format_type())
        .body(Full::new(Bytes::from(buffer)))
        .unwrap()
}

fn healthz_response() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from_static(b"ok\n")))
        .unwrap()
}

fn not_found_response() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from_static(b"not found\n")))
        .unwrap()
}
