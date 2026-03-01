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
pub struct Config {
    pub table_id: u8,
    pub iface: String,
    pub tcp_mss_clamp: Option<u32>,
    #[schema(value_type = Option<String>)]
    pub ipv4_snat: Option<IpAddr>,
    #[schema(value_type = Option<String>)]
    pub ipv6_snat: Option<IpAddr>,
    #[schema(value_type = String)]
    pub ipv4_subnet: Ipv4Net,
    #[schema(value_type = String)]
    pub ipv6_subnet: Ipv6Net,
    pub upstream_resolver: UpstreamResolverConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            table_id: 100,
            iface: "wg0".to_string(),
            tcp_mss_clamp: Some(1280),
            ipv4_snat: Some(IpAddr::V4(Ipv4Addr::new(10, 10, 10, 4))),
            ipv6_snat: None,
            ipv4_subnet: Ipv4Net::from_str("198.18.0.0/15").unwrap(),
            ipv6_subnet: Ipv6Net::from_str("fd32:bfcc:fba0:1337::/64").unwrap(),
            upstream_resolver: UpstreamResolverConfig::Quad9Https,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, utoipa::ToSchema)]
pub struct PatchConfig {
    pub table_id: Option<u8>,
    pub iface: Option<String>,
    pub tcp_mss_clamp: Option<Option<u32>>,
    #[schema(value_type = Option<Option<String>>)]
    pub ipv4_snat: Option<Option<IpAddr>>,
    #[schema(value_type = Option<Option<String>>)]
    pub ipv6_snat: Option<Option<IpAddr>>,
    #[schema(value_type = Option<String>)]
    pub ipv4_subnet: Option<Ipv4Net>,
    #[schema(value_type = Option<String>)]
    pub ipv6_subnet: Option<Ipv6Net>,
    pub upstream_resolver: Option<UpstreamResolverConfig>,
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
        let config: Config = toml::from_str(&content)?;
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
            table_id: patch.table_id.unwrap_or(self.table_id),
            iface: patch.iface.unwrap_or_else(|| self.iface.clone()),
            tcp_mss_clamp: patch.tcp_mss_clamp.unwrap_or(self.tcp_mss_clamp),
            ipv4_snat: patch.ipv4_snat.unwrap_or(self.ipv4_snat),
            ipv6_snat: patch.ipv6_snat.unwrap_or(self.ipv6_snat),
            ipv4_subnet: patch.ipv4_subnet.unwrap_or(self.ipv4_subnet),
            ipv6_subnet: patch.ipv6_subnet.unwrap_or(self.ipv6_subnet),
            upstream_resolver: patch.upstream_resolver.unwrap_or_else(|| self.upstream_resolver.clone()),
        }
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
