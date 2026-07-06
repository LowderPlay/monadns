use hickory_proto::http::DEFAULT_DNS_QUERY_PATH;
use hickory_resolver::config::{NameServerConfig, ProtocolConfig, ResolverConfig, ResolverOpts};
use ipnet::{Ipv4Net, Ipv6Net};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

fn deserialize_health_check_hosts<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Hosts {
        One(String),
        Many(Vec<String>),
    }

    match Hosts::deserialize(deserializer)? {
        Hosts::One(host) => Ok(vec![host]),
        Hosts::Many(hosts) => Ok(hosts),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FailoverMode {
    Global,
    Disabled,
    Custom,
}

impl Default for FailoverMode {
    fn default() -> Self {
        Self::Global
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct InterfaceConfig {
    pub name: String,
    pub fwmark: u32,
    pub table_id: u8,
    pub tcp_mss_clamp: Option<u32>,
    #[schema(value_type = Option<String>)]
    pub ipv4_snat: Option<IpAddr>,
    #[schema(value_type = Option<String>)]
    pub ipv6_snat: Option<IpAddr>,
    pub health_check_enabled: bool,
    #[serde(
        alias = "health_check_host",
        deserialize_with = "deserialize_health_check_hosts"
    )]
    pub health_check_hosts: Vec<String>,
    pub health_check_latency_threshold_ms: f64,
    pub health_check_packet_loss_threshold_percent: f64,
    pub failover_mode: FailoverMode,
    pub failover_interfaces: Vec<String>,
}

impl Default for InterfaceConfig {
    fn default() -> Self {
        Self {
            name: "wg0".to_string(),
            fwmark: 1,
            table_id: 100,
            tcp_mss_clamp: None,
            ipv4_snat: None,
            ipv6_snat: None,
            health_check_enabled: true,
            health_check_hosts: vec!["1.1.1.1".to_string(), "2606:4700:4700::1111".to_string()],
            health_check_latency_threshold_ms: 500.0,
            health_check_packet_loss_threshold_percent: 50.0,
            failover_mode: FailoverMode::Global,
            failover_interfaces: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct Config {
    pub interfaces: Vec<InterfaceConfig>,
    pub default_interface: String,
    #[schema(value_type = String)]
    pub ipv4_subnet: Ipv4Net,
    #[schema(value_type = String)]
    pub ipv6_subnet: Ipv6Net,
    pub upstream_resolver: UpstreamResolverConfig,
    pub export_enabled: bool,
    pub health_check_interval_seconds: u64,
    pub health_check_timeout_seconds: u64,
    pub health_check_ping_count: u32,
    pub failover_recovery_delay_seconds: u64,
    pub failover_interfaces: Vec<String>,

    // Backwards compatibility fields
    #[serde(skip_serializing, default)]
    pub table_id: Option<u8>,
    #[serde(skip_serializing, default)]
    pub iface: Option<String>,
    #[serde(skip_serializing, default)]
    pub tcp_mss_clamp: Option<u32>,
    #[serde(skip_serializing, default)]
    pub ipv4_snat: Option<IpAddr>,
    #[serde(skip_serializing, default)]
    pub ipv6_snat: Option<IpAddr>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            interfaces: vec![InterfaceConfig {
                tcp_mss_clamp: Some(1280),
                ipv4_snat: Some(IpAddr::V4(Ipv4Addr::new(10, 10, 10, 4))),
                ..InterfaceConfig::default()
            }],
            default_interface: "wg0".to_string(),
            ipv4_subnet: Ipv4Net::from_str("198.18.0.0/15").unwrap(),
            ipv6_subnet: Ipv6Net::from_str("fd32:bfcc:fba0:1337::/64").unwrap(),
            upstream_resolver: UpstreamResolverConfig::Quad9Https,
            export_enabled: false,
            health_check_interval_seconds: 15,
            health_check_timeout_seconds: 12,
            health_check_ping_count: 5,
            failover_recovery_delay_seconds: 60,
            failover_interfaces: vec!["wg0".to_string()],
            table_id: None,
            iface: None,
            tcp_mss_clamp: None,
            ipv4_snat: None,
            ipv6_snat: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, utoipa::ToSchema)]
pub struct PatchConfig {
    pub interfaces: Option<Vec<InterfaceConfig>>,
    pub default_interface: Option<String>,
    #[schema(value_type = Option<String>)]
    pub ipv4_subnet: Option<Ipv4Net>,
    #[schema(value_type = Option<String>)]
    pub ipv6_subnet: Option<Ipv6Net>,
    pub upstream_resolver: Option<UpstreamResolverConfig>,
    pub export_enabled: Option<bool>,
    pub health_check_interval_seconds: Option<u64>,
    pub health_check_timeout_seconds: Option<u64>,
    pub health_check_ping_count: Option<u32>,
    pub failover_recovery_delay_seconds: Option<u64>,
    pub failover_interfaces: Option<Vec<String>>,
}

impl Config {
    pub fn get_path() -> PathBuf {
        std::env::var("MONADNS_CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/opt/monadns/config.toml"))
    }

    pub fn load() -> anyhow::Result<Self> {
        let path = Self::get_path();
        if !path.exists() {
            info!("Config file not found at {:?}, creating default", path);
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }

        let content = std::fs::read_to_string(&path)?;
        let interfaces_configured = toml::from_str::<toml::Value>(&content)?
            .as_table()
            .is_some_and(|table| table.contains_key("interfaces"));
        let failover_interfaces_configured = toml::from_str::<toml::Value>(&content)?
            .as_table()
            .is_some_and(|table| table.contains_key("failover_interfaces"));
        let mut config: Config = toml::from_str(&content)?;

        // Migration logic for backwards compatibility
        if !interfaces_configured {
            config.interfaces.clear();
        }
        if config.interfaces.is_empty() {
            let name = config.iface.clone().unwrap_or_else(|| "wg0".to_string());
            config.interfaces.push(InterfaceConfig {
                name: name.clone(),
                table_id: config.table_id.unwrap_or(100),
                tcp_mss_clamp: config.tcp_mss_clamp,
                ipv4_snat: config.ipv4_snat,
                ipv6_snat: config.ipv6_snat,
                ..InterfaceConfig::default()
            });
            if config.default_interface.is_empty() {
                config.default_interface = name;
            }
        } else if config.default_interface.is_empty() {
            config.default_interface = config.interfaces[0].name.clone();
        }
        if !failover_interfaces_configured || config.failover_interfaces.is_empty() {
            config.failover_interfaces = config
                .interfaces
                .iter()
                .map(|interface| interface.name.clone())
                .collect();
        }

        Ok(config)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::get_path();
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        info!("Config saved to {:?}", path);
        Ok(())
    }

    pub fn get_db_path() -> PathBuf {
        std::env::var("MONADNS_DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/opt/monadns/db.sqlite"))
    }

    pub fn get_dns_bind() -> String {
        std::env::var("MONADNS_DNS_BIND").unwrap_or_else(|_| "[::]:5553".to_string())
    }

    pub fn get_http_bind() -> String {
        std::env::var("MONADNS_HTTP_BIND").unwrap_or_else(|_| "[::]:8080".to_string())
    }

    pub fn get_metrics_bind() -> Option<String> {
        std::env::var("MONADNS_METRICS_BIND").ok()
    }

    pub fn patch(&self, patch: PatchConfig) -> Self {
        Self {
            interfaces: patch.interfaces.unwrap_or_else(|| self.interfaces.clone()),
            default_interface: patch
                .default_interface
                .unwrap_or_else(|| self.default_interface.clone()),
            ipv4_subnet: patch.ipv4_subnet.unwrap_or(self.ipv4_subnet),
            ipv6_subnet: patch.ipv6_subnet.unwrap_or(self.ipv6_subnet),
            upstream_resolver: patch
                .upstream_resolver
                .unwrap_or_else(|| self.upstream_resolver.clone()),
            export_enabled: patch.export_enabled.unwrap_or(self.export_enabled),
            health_check_interval_seconds: patch
                .health_check_interval_seconds
                .unwrap_or(self.health_check_interval_seconds),
            health_check_timeout_seconds: patch
                .health_check_timeout_seconds
                .unwrap_or(self.health_check_timeout_seconds),
            health_check_ping_count: patch
                .health_check_ping_count
                .unwrap_or(self.health_check_ping_count),
            failover_recovery_delay_seconds: patch
                .failover_recovery_delay_seconds
                .unwrap_or(self.failover_recovery_delay_seconds),
            failover_interfaces: patch
                .failover_interfaces
                .unwrap_or_else(|| self.failover_interfaces.clone()),
            table_id: None,
            iface: None,
            tcp_mss_clamp: None,
            ipv4_snat: None,
            ipv6_snat: None,
        }
    }

    pub fn resolve_interface(&self, interface_name: Option<&str>) -> &InterfaceConfig {
        let name = match interface_name {
            Some("default") | None => &self.default_interface,
            Some(name) => name,
        };

        self.interfaces
            .iter()
            .find(|i| i.name == *name)
            .or_else(|| {
                self.interfaces
                    .iter()
                    .find(|i| i.name == self.default_interface)
            })
            .unwrap_or_else(|| {
                if !self.interfaces.is_empty() {
                    &self.interfaces[0]
                } else {
                    panic!("No interfaces configured")
                }
            })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema, PartialEq)]
pub enum ResolverProtocol {
    Plain,
    Tls,
    Https,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CustomNameserverConfig {
    pub addr: String,
    pub protocol: ResolverProtocol,
    pub tls_dns_name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "type")]
pub enum UpstreamResolverConfig {
    Quad9Https,
    CloudflareHttps,
    GoogleHttps,
    Custom {
        nameservers: Vec<CustomNameserverConfig>,
    },
}

impl Default for UpstreamResolverConfig {
    fn default() -> Self {
        Self::Quad9Https
    }
}

impl UpstreamResolverConfig {
    pub fn to_resolver_parts(&self) -> (ResolverConfig, ResolverOpts) {
        match self {
            UpstreamResolverConfig::Quad9Https => {
                (ResolverConfig::quad9_https(), ResolverOpts::default())
            }
            UpstreamResolverConfig::CloudflareHttps => {
                (ResolverConfig::cloudflare_https(), ResolverOpts::default())
            }
            UpstreamResolverConfig::GoogleHttps => {
                (ResolverConfig::google_https(), ResolverOpts::default())
            }
            UpstreamResolverConfig::Custom { nameservers } => {
                let mut config = ResolverConfig::from_parts(None, vec![], vec![]);
                for ns in nameservers {
                    let socket_addr = if let Ok(addr) = SocketAddr::from_str(&ns.addr) {
                        addr
                    } else if let Ok(ip) = IpAddr::from_str(&ns.addr) {
                        let port = match ns.protocol {
                            ResolverProtocol::Plain => 53,
                            ResolverProtocol::Tls => 853,
                            ResolverProtocol::Https => 443,
                        };
                        SocketAddr::new(ip, port)
                    } else {
                        warn!("Invalid nameserver address: {}", ns.addr);
                        continue;
                    };

                    config.add_name_server(NameServerConfig {
                        socket_addr,
                        protocol: match ns.protocol {
                            ResolverProtocol::Plain => ProtocolConfig::Udp,
                            ResolverProtocol::Tls => ProtocolConfig::Tls {
                                server_name: ns
                                    .tls_dns_name
                                    .clone()
                                    .unwrap_or_else(|| "".to_string())
                                    .into(),
                            },
                            ResolverProtocol::Https => ProtocolConfig::Https {
                                server_name: ns
                                    .tls_dns_name
                                    .clone()
                                    .unwrap_or_else(|| "".to_string())
                                    .into(),
                                path: Arc::from(DEFAULT_DNS_QUERY_PATH),
                            },
                        },
                        trust_negative_responses: false,
                        bind_addr: None,
                    });
                }
                (config, ResolverOpts::default())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_legacy_single_health_check_host() {
        let config: Config = toml::from_str(
            r#"
default_interface = "wg0"
ipv4_subnet = "198.18.0.0/15"
ipv6_subnet = "fd32:bfcc:fba0:1337::/64"

[[interfaces]]
name = "wg0"
fwmark = 1
table_id = 100
health_check_host = "1.1.1.1"

[upstream_resolver]
type = "Quad9Https"
"#,
        )
        .unwrap();

        assert_eq!(config.interfaces[0].health_check_hosts, ["1.1.1.1"]);
        assert_eq!(config.health_check_interval_seconds, 15);
        assert_eq!(config.health_check_timeout_seconds, 12);
        assert_eq!(config.health_check_ping_count, 5);
    }

    #[test]
    fn loads_multiple_health_check_hosts() {
        let config: Config = toml::from_str(
            r#"
default_interface = "wg0"
ipv4_subnet = "198.18.0.0/15"
ipv6_subnet = "fd32:bfcc:fba0:1337::/64"
health_check_interval_seconds = 30
health_check_timeout_seconds = 8
health_check_ping_count = 3

[[interfaces]]
name = "wg0"
fwmark = 1
table_id = 100
health_check_hosts = ["1.1.1.1", "2606:4700:4700::1111"]

[upstream_resolver]
type = "Quad9Https"
"#,
        )
        .unwrap();

        assert_eq!(
            config.interfaces[0].health_check_hosts,
            ["1.1.1.1", "2606:4700:4700::1111"]
        );
        assert_eq!(config.health_check_interval_seconds, 30);
        assert_eq!(config.health_check_timeout_seconds, 8);
        assert_eq!(config.health_check_ping_count, 3);
    }

    #[test]
    fn preserves_empty_optional_interface_fields_after_toml_round_trip() {
        let mut config = Config::default();
        config.interfaces[0].tcp_mss_clamp = None;
        config.interfaces[0].ipv4_snat = None;
        config.interfaces[0].ipv6_snat = None;

        let serialized = toml::to_string_pretty(&config).unwrap();
        let loaded: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(loaded.interfaces[0].tcp_mss_clamp, None);
        assert_eq!(loaded.interfaces[0].ipv4_snat, None);
        assert_eq!(loaded.interfaces[0].ipv6_snat, None);
    }
}
