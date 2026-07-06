use crate::config::{Config, FailoverMode, InterfaceConfig};
use crate::health_check::InterfaceHealthRegistry;

pub fn resolve_effective_interface(
    config: &Config,
    interface_name: Option<&str>,
    health_status: &InterfaceHealthRegistry,
) -> InterfaceConfig {
    resolve_effective_interface_at(
        config,
        interface_name,
        health_status,
        chrono::Utc::now().timestamp_millis(),
    )
}

fn resolve_effective_interface_at(
    config: &Config,
    interface_name: Option<&str>,
    health_status: &InterfaceHealthRegistry,
    now_ms: i64,
) -> InterfaceConfig {
    let desired = config.resolve_interface(interface_name);
    if is_interface_eligible(config, &desired.name, health_status, now_ms)
        || matches!(desired.failover_mode, FailoverMode::Disabled)
    {
        return desired.clone();
    }

    let failover_interfaces = match desired.failover_mode {
        FailoverMode::Global => &config.failover_interfaces,
        FailoverMode::Custom => &desired.failover_interfaces,
        FailoverMode::Disabled => unreachable!(),
    };

    let candidates = if failover_interfaces.is_empty() {
        config
            .interfaces
            .iter()
            .map(|interface| interface.name.as_str())
            .collect::<Vec<_>>()
    } else {
        failover_interfaces
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    };

    candidates
        .into_iter()
        .filter_map(|name| {
            config
                .interfaces
                .iter()
                .find(|interface| interface.name == name)
        })
        .find(|interface| is_interface_eligible(config, &interface.name, health_status, now_ms))
        .cloned()
        .unwrap_or_else(|| desired.clone())
}

pub fn is_interface_healthy(
    config: &Config,
    interface_name: &str,
    health_status: &InterfaceHealthRegistry,
) -> bool {
    let Some(interface) = config
        .interfaces
        .iter()
        .find(|interface| interface.name == interface_name)
    else {
        return false;
    };

    if !interface.health_check_enabled || interface.health_check_hosts.is_empty() {
        return true;
    }

    interface.health_check_hosts.iter().all(|host| {
        health_status
            .get(&(interface.name.clone(), host.clone()))
            .is_none_or(|status| status.healthy)
    })
}

fn is_interface_eligible(
    config: &Config,
    interface_name: &str,
    health_status: &InterfaceHealthRegistry,
    now_ms: i64,
) -> bool {
    if !is_interface_healthy(config, interface_name, health_status) {
        return false;
    }

    let recovery_delay_ms = config.failover_recovery_delay_seconds.saturating_mul(1000) as i64;
    if recovery_delay_ms == 0 {
        return true;
    }

    let Some(interface) = config
        .interfaces
        .iter()
        .find(|interface| interface.name == interface_name)
    else {
        return false;
    };

    if !interface.health_check_enabled || interface.health_check_hosts.is_empty() {
        return true;
    }

    interface.health_check_hosts.iter().all(|host| {
        health_status
            .get(&(interface.name.clone(), host.clone()))
            .is_none_or(|status| {
                if status.last_unhealthy_at_ms.is_none() {
                    return true;
                }

                status
                    .healthy_since_ms
                    .is_some_and(|healthy_since_ms| now_ms - healthy_since_ms >= recovery_delay_ms)
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FailoverMode;
    use crate::health_check::InterfaceHealthStatus;
    use std::sync::Arc;

    fn config() -> Config {
        Config {
            interfaces: vec![
                InterfaceConfig {
                    name: "warp1".to_string(),
                    fwmark: 1,
                    health_check_hosts: vec!["1.1.1.1".to_string()],
                    ..InterfaceConfig::default()
                },
                InterfaceConfig {
                    name: "warp0".to_string(),
                    fwmark: 2,
                    health_check_hosts: vec!["1.1.1.1".to_string()],
                    ..InterfaceConfig::default()
                },
                InterfaceConfig {
                    name: "eth0".to_string(),
                    fwmark: 3,
                    health_check_hosts: vec!["1.1.1.1".to_string()],
                    ..InterfaceConfig::default()
                },
            ],
            default_interface: "warp0".to_string(),
            failover_recovery_delay_seconds: 60,
            failover_interfaces: vec!["warp1".to_string(), "warp0".to_string(), "eth0".to_string()],
            ..Config::default()
        }
    }

    fn registry_with(statuses: &[(&str, bool)]) -> InterfaceHealthRegistry {
        let registry = Arc::new(dashmap::DashMap::new());
        for (interface, healthy) in statuses {
            registry.insert(
                ((*interface).to_string(), "1.1.1.1".to_string()),
                InterfaceHealthStatus {
                    interface: (*interface).to_string(),
                    host: "1.1.1.1".to_string(),
                    enabled: true,
                    healthy: *healthy,
                    latency_ms: Some(10.0),
                    packet_loss_percent: if *healthy { 0.0 } else { 100.0 },
                    healthy_since_ms: if *healthy { Some(1) } else { None },
                    last_unhealthy_at_ms: if *healthy { None } else { Some(1) },
                    updated_at_ms: 1,
                    error: None,
                },
            );
        }
        registry
    }

    #[test]
    fn global_failover_uses_preference_order() {
        let config = config();
        let registry = registry_with(&[("warp1", true), ("warp0", false), ("eth0", true)]);

        let effective = resolve_effective_interface(&config, Some("warp0"), &registry);

        assert_eq!(effective.name, "warp1");
        assert_eq!(effective.fwmark, 1);
    }

    #[test]
    fn custom_failover_overrides_global_order() {
        let mut config = config();
        config.interfaces[1].failover_mode = FailoverMode::Custom;
        config.interfaces[1].failover_interfaces = vec!["eth0".to_string()];
        let registry = registry_with(&[("warp1", true), ("warp0", false), ("eth0", true)]);

        let effective = resolve_effective_interface(&config, Some("warp0"), &registry);

        assert_eq!(effective.name, "eth0");
        assert_eq!(effective.fwmark, 3);
    }

    #[test]
    fn disabled_failover_keeps_unhealthy_selected_interface() {
        let mut config = config();
        config.interfaces[1].failover_mode = FailoverMode::Disabled;
        let registry = registry_with(&[("warp1", true), ("warp0", false), ("eth0", true)]);

        let effective = resolve_effective_interface(&config, Some("warp0"), &registry);

        assert_eq!(effective.name, "warp0");
        assert_eq!(effective.fwmark, 2);
    }

    #[test]
    fn recovered_interface_waits_for_recovery_delay() {
        let config = config();
        let registry = Arc::new(dashmap::DashMap::new());
        registry.insert(
            ("warp1".to_string(), "1.1.1.1".to_string()),
            InterfaceHealthStatus {
                interface: "warp1".to_string(),
                host: "1.1.1.1".to_string(),
                enabled: true,
                healthy: true,
                latency_ms: Some(10.0),
                packet_loss_percent: 0.0,
                healthy_since_ms: Some(1_000),
                last_unhealthy_at_ms: None,
                updated_at_ms: 61_000,
                error: None,
            },
        );
        registry.insert(
            ("warp0".to_string(), "1.1.1.1".to_string()),
            InterfaceHealthStatus {
                interface: "warp0".to_string(),
                host: "1.1.1.1".to_string(),
                enabled: true,
                healthy: true,
                latency_ms: Some(10.0),
                packet_loss_percent: 0.0,
                healthy_since_ms: Some(30_000),
                last_unhealthy_at_ms: Some(20_000),
                updated_at_ms: 61_000,
                error: None,
            },
        );

        let effective = resolve_effective_interface_at(&config, Some("warp0"), &registry, 61_000);

        assert_eq!(effective.name, "warp1");
    }

    #[test]
    fn recovered_interface_switches_back_after_recovery_delay() {
        let config = config();
        let registry = Arc::new(dashmap::DashMap::new());
        registry.insert(
            ("warp0".to_string(), "1.1.1.1".to_string()),
            InterfaceHealthStatus {
                interface: "warp0".to_string(),
                host: "1.1.1.1".to_string(),
                enabled: true,
                healthy: true,
                latency_ms: Some(10.0),
                packet_loss_percent: 0.0,
                healthy_since_ms: Some(30_000),
                last_unhealthy_at_ms: Some(20_000),
                updated_at_ms: 91_000,
                error: None,
            },
        );

        let effective = resolve_effective_interface_at(&config, Some("warp0"), &registry, 91_000);

        assert_eq!(effective.name, "warp0");
    }
}
