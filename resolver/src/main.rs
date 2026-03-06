mod fake_ip;
mod handler;
mod domain_controller;
mod route_controller;
mod config;
mod app;
mod api;
pub mod error;

use std::sync::Arc;
use env_logger::Env;
use hickory_server::ServerFuture;
use log::info;
use tokio::net::UdpSocket;
use crate::app::App;
use crate::config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .init();
    
    let config = Config::load()?;

    if let Some(metrics_bind) = Config::get_metrics_bind() {
        info!("starting prometheus exporter on {}", metrics_bind);
        metrics_exporter_prometheus::PrometheusBuilder::new()
            .with_http_listener(metrics_bind.parse::<std::net::SocketAddr>()?)
            .install()?;
    }

    let app = Arc::new(App::new(config).await?);
    let handler = app.handler();
    
    let mut server = ServerFuture::new(handler.clone());
    let socket = UdpSocket::bind(Config::get_dns_bind()).await?;
    server.register_socket(socket);

    let api_app = api::create_router(app.clone());

    let listener = tokio::net::TcpListener::bind(Config::get_http_bind()).await?;
    info!("listening on {}", listener.local_addr()?);

    tokio::select! {
        res = server.block_until_done() => {
            res?;
        }
        _ = axum::serve(listener, api_app) => {
            info!("API server stopped");
        }
        _ = tokio::signal::ctrl_c() => {
            info!("received SIGINT");
        }
    }

    // Cleanup: we might want to trigger cleanup of current state
    let state = handler.state.load();
    state.route_controller.cleanup().await?;

    info!("bye!");
    Ok(())
}
