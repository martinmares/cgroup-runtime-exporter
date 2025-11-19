mod cgroup;
mod config;
mod downward;
mod metrics;
mod procfs;

use std::{convert::Infallible, sync::Arc, time::Duration};

use anyhow::Result;
use hyper::{
    Body, Request, Response, Server, StatusCode,
    service::{make_service_fn, service_fn},
};
use prometheus::{Encoder, TextEncoder};

use crate::{
    cgroup as cgroup_mod, config::Config, downward as downward_mod, metrics::Metrics,
    procfs as procfs_mod,
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

    // DownwardAPI je nepovinné - pokud není DIR, nic se neděje
    if let Some(ref dir) = state.cfg.downward_dir {
        if let Err(e) = downward_mod::init_downward_info(&state.metrics, dir) {
            eprintln!("failed to init downward api info: {:?}", e);
        }
    }

    // Background update loop - cache metrik
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

    // listen_addr si vytáhneme před tím, než state přesuneme do closure
    let addr = state.cfg.listen_addr;

    println!(
        "Starting exporter on {}, update interval {}s",
        addr, state.cfg.update_interval_secs
    );

    let make_svc_state = state.clone();
    let make_svc = make_service_fn(move |_conn| {
        let state = make_svc_state.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                let state = state.clone();
                async move { handle_request(req, state).await }
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }

    Ok(())
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

    Ok(())
}

async fn handle_request(
    req: Request<Body>,
    state: Arc<AppState>,
) -> Result<Response<Body>, Infallible> {
    let path = req.uri().path();

    match path {
        "/metrics" => Ok(metrics_response(&state)),
        "/healthz" => Ok(healthz_response()),
        _ => Ok(not_found_response()),
    }
}

fn metrics_response(state: &AppState) -> Response<Body> {
    let encoder = TextEncoder::new();
    let metric_families = state.metrics.registry.gather();

    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        eprintln!("could not encode metrics: {:?}", e);
    }

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", encoder.format_type())
        .body(Body::from(buffer))
        .unwrap()
}

fn healthz_response() -> Response<Body> {
    // jednoduchý healthcheck - pokud běží proces a jsem schopný odpovědět, je to OK
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(Body::from("ok\n"))
        .unwrap()
}

fn not_found_response() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(Body::from("not found\n"))
        .unwrap()
}
