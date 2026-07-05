use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use log::{debug, error, warn};
use serde::Serialize;
use tokio::process::Command;
use tokio::task::JoinHandle;
use utoipa::ToSchema;

use crate::config::InterfaceConfig;

#[derive(Clone, Copy)]
pub struct HealthCheckSettings {
    pub interval_seconds: u64,
    pub timeout_seconds: u64,
    pub ping_count: u32,
}

#[derive(Debug, PartialEq)]
struct PingMeasurements {
    latency_ms: Option<f64>,
    packet_loss_percent: f64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct InterfaceHealthStatus {
    pub interface: String,
    pub host: String,
    pub enabled: bool,
    pub healthy: bool,
    pub latency_ms: Option<f64>,
    pub packet_loss_percent: f64,
    pub updated_at_ms: i64,
    pub error: Option<String>,
}

pub type InterfaceHealthRegistry = Arc<DashMap<(String, String), InterfaceHealthStatus>>;

pub fn spawn(
    interface: InterfaceConfig,
    host: String,
    settings: HealthCheckSettings,
    registry: InterfaceHealthRegistry,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let labels = [
            ("interface", interface.name.clone()),
            ("host", host.clone()),
        ];

        if !interface.health_check_enabled {
            metrics::gauge!("interface_healthy", &labels).set(0.0);
            metrics::gauge!("interface_latency_milliseconds", &labels).set(0.0);
            metrics::gauge!("interface_packet_loss_percent", &labels).set(0.0);
            update_registry(&registry, &interface, &host, false, false, None, 0.0, None);
            debug!("health check disabled for interface {}", interface.name);
            return;
        }

        let mut interval =
            tokio::time::interval(Duration::from_secs(settings.interval_seconds.max(1)));
        loop {
            interval.tick().await;
            metrics::counter!("interface_health_checks_total", &labels).increment(1);

            match check(&interface, &host, settings).await {
                Ok(measurements) => {
                    let healthy = measurements.packet_loss_percent
                        <= interface.health_check_packet_loss_threshold_percent
                        && measurements.latency_ms.is_some_and(|latency| {
                            latency <= interface.health_check_latency_threshold_ms
                        });

                    metrics::gauge!("interface_packet_loss_percent", &labels)
                        .set(measurements.packet_loss_percent);
                    metrics::gauge!("interface_latency_milliseconds", &labels)
                        .set(measurements.latency_ms.unwrap_or(0.0));
                    metrics::gauge!("interface_healthy", &labels).set(if healthy {
                        1.0
                    } else {
                        0.0
                    });
                    update_registry(
                        &registry,
                        &interface,
                        &host,
                        true,
                        healthy,
                        measurements.latency_ms,
                        measurements.packet_loss_percent,
                        None,
                    );

                    if healthy {
                        debug!(
                            "interface {} health check to {} passed: latency={:?}ms loss={:.1}%",
                            interface.name,
                            host,
                            measurements.latency_ms,
                            measurements.packet_loss_percent
                        );
                    } else {
                        metrics::counter!("interface_health_check_failures_total", &labels)
                            .increment(1);
                        warn!(
                            "interface {} is unhealthy: host={} latency={:?}ms (limit={}ms) loss={:.1}% (limit={}%)",
                            interface.name,
                            host,
                            measurements.latency_ms,
                            interface.health_check_latency_threshold_ms,
                            measurements.packet_loss_percent,
                            interface.health_check_packet_loss_threshold_percent
                        );
                    }
                }
                Err(err) => {
                    metrics::counter!("interface_health_check_failures_total", &labels)
                        .increment(1);
                    metrics::gauge!("interface_packet_loss_percent", &labels).set(100.0);
                    metrics::gauge!("interface_latency_milliseconds", &labels).set(0.0);
                    metrics::gauge!("interface_healthy", &labels).set(0.0);
                    update_registry(
                        &registry,
                        &interface,
                        &host,
                        true,
                        false,
                        None,
                        100.0,
                        Some(err.to_string()),
                    );
                    error!(
                        "interface {} health check to {} failed: {}",
                        interface.name, host, err
                    );
                }
            }
        }
    })
}

fn update_registry(
    registry: &InterfaceHealthRegistry,
    interface: &InterfaceConfig,
    host: &str,
    enabled: bool,
    healthy: bool,
    latency_ms: Option<f64>,
    packet_loss_percent: f64,
    error: Option<String>,
) {
    registry.insert(
        (interface.name.clone(), host.to_string()),
        InterfaceHealthStatus {
            interface: interface.name.clone(),
            host: host.to_string(),
            enabled,
            healthy,
            latency_ms,
            packet_loss_percent,
            updated_at_ms: chrono::Utc::now().timestamp_millis(),
            error,
        },
    );
}

async fn check(
    interface: &InterfaceConfig,
    host: &str,
    settings: HealthCheckSettings,
) -> anyhow::Result<PingMeasurements> {
    let timeout_seconds = settings.timeout_seconds.max(1);
    let mut command = Command::new("ping");
    command
        .env("LC_ALL", "C")
        .args([
            "-n",
            "-m",
            &interface.fwmark.to_string(),
            "-I",
            &interface.name.clone(),
            "-c",
            &settings.ping_count.max(1).to_string(),
            "-W",
            &timeout_seconds.to_string(),
            "-w",
            &timeout_seconds.to_string(),
            host,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = tokio::time::timeout(
        Duration::from_secs(timeout_seconds.saturating_add(1)),
        command.output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("ping timed out"))??;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(measurements) = parse_ping_output(&stdout) {
        return Ok(measurements);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let is_unreachable = [
        "Network is unreachable",
        "No route to host",
        "Address family not supported by protocol",
    ]
    .iter()
    .any(|message| stderr.contains(message));
    if is_unreachable {
        return Ok(PingMeasurements {
            latency_ms: None,
            packet_loss_percent: 100.0,
        });
    }

    anyhow::bail!(
        "could not parse ping output (status {}): {}",
        output.status,
        stderr.trim()
    )
}

fn parse_ping_output(output: &str) -> Option<PingMeasurements> {
    let packet_loss_percent = output.lines().find_map(|line| {
        let marker = "% packet loss";
        let marker_index = line.find(marker)?;
        let before_marker = &line[..marker_index];
        let value = before_marker.rsplit_once(',')?.1.trim();
        value.parse::<f64>().ok()
    })?;

    let latency_ms = output.lines().find_map(|line| {
        if !(line.contains("min/avg/max") || line.contains("round-trip")) {
            return None;
        }
        let values = line.split_once('=')?.1.trim().split_whitespace().next()?;
        values.split('/').nth(1)?.parse::<f64>().ok()
    });

    Some(PingMeasurements {
        latency_ms,
        packet_loss_percent,
    })
}
