use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use hickory_resolver::config::{ResolverConfig, ResolverOpts, NameServerConfig, ProtocolConfig};
use ipnet::{Ipv4Net, Ipv6Net};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use hickory_proto::http::DEFAULT_DNS_QUERY_PATH;
use log::{info, warn};

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct InterfaceConfig {
    pub name: String,
    pub fwmark: u32,
    pub table_id: u8,
    pub tcp_mss_clamp: Option<u32>,
    #[schema(value_type = Option<String>)]
    pub ipv4_snat: Option<IpAddr>,
    #[schema(value_type = Option<String>)]
    pub ipv6_snat: Option<IpAddr>,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Config {
    #[serde(default)]
    pub interfaces: Vec<InterfaceConfig>,
    #[serde(default)]
    pub default_interface: String,
    #[schema(value_type = String)]
    pub ipv4_subnet: Ipv4Net,
    #[schema(value_type = String)]
    pub ipv6_subnet: Ipv6Net,
    pub upstream_resolver: UpstreamResolverConfig,
    #[serde(default)]
    pub export_enabled: bool,

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
                name: "wg0".to_string(),
                fwmark: 1,
                table_id: 100,
                tcp_mss_clamp: Some(1280),
                ipv4_snat: Some(IpAddr::V4(Ipv4Addr::new(10, 10, 10, 4))),
                ipv6_snat: None,
            }],
            default_interface: "wg0".to_string(),
            ipv4_subnet: Ipv4Net::from_str("198.18.0.0/15").unwrap(),
            ipv6_subnet: Ipv6Net::from_str("fd32:bfcc:fba0:1337::/64").unwrap(),
            upstream_resolver: UpstreamResolverConfig::Quad9Https,
            export_enabled: false,
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
        let mut config: Config = toml::from_str(&content)?;

        // Migration logic for backwards compatibility
        if config.interfaces.is_empty() {
            let name = config.iface.clone().unwrap_or_else(|| "wg0".to_string());
            config.interfaces.push(InterfaceConfig {
                name: name.clone(),
                fwmark: 1,
                table_id: config.table_id.unwrap_or(100),
                tcp_mss_clamp: config.tcp_mss_clamp,
                ipv4_snat: config.ipv4_snat,
                ipv6_snat: config.ipv6_snat,
            });
            if config.default_interface.is_empty() {
                config.default_interface = name;
            }
        } else if config.default_interface.is_empty() {
            config.default_interface = config.interfaces[0].name.clone();
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
        std::env::var("MONADNS_DNS_BIND")
            .unwrap_or_else(|_| "[::]:5553".to_string())
    }

    pub fn get_http_bind() -> String {
        std::env::var("MONADNS_HTTP_BIND")
            .unwrap_or_else(|_| "[::]:8080".to_string())
    }

    pub fn get_metrics_bind() -> Option<String> {
        std::env::var("MONADNS_METRICS_BIND").ok()
    }

    pub fn patch(&self, patch: PatchConfig) -> Self {
        Self {
            interfaces: patch.interfaces.unwrap_or_else(|| self.interfaces.clone()),
            default_interface: patch.default_interface.unwrap_or_else(|| self.default_interface.clone()),
            ipv4_subnet: patch.ipv4_subnet.unwrap_or(self.ipv4_subnet),
            ipv6_subnet: patch.ipv6_subnet.unwrap_or(self.ipv6_subnet),
            upstream_resolver: patch.upstream_resolver.unwrap_or_else(|| self.upstream_resolver.clone()),
            export_enabled: patch.export_enabled.unwrap_or(self.export_enabled),
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

        self.interfaces.iter()
            .find(|i| i.name == *name)
            .or_else(|| self.interfaces.iter().find(|i| i.name == self.default_interface))
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
            UpstreamResolverConfig::Quad9Https => (ResolverConfig::quad9_https(), ResolverOpts::default()),
            UpstreamResolverConfig::CloudflareHttps => (ResolverConfig::cloudflare_https(), ResolverOpts::default()),
            UpstreamResolverConfig::GoogleHttps => (ResolverConfig::google_https(), ResolverOpts::default()),
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
                                server_name: ns.tls_dns_name.clone()
                                    .unwrap_or_else(|| "".to_string()).into()
                            },
                            ResolverProtocol::Https => ProtocolConfig::Https {
                                server_name: ns.tls_dns_name.clone()
                                    .unwrap_or_else(|| "".to_string()).into(),
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
