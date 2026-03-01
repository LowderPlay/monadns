use std::sync::Arc;
use arc_swap::ArcSwap;
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::TokioResolver;
use ipnet::IpNet;
use crate::config::{Config, PatchConfig};
use crate::domain_controller::DomainController;
use crate::domain_controller::sqlite::SqliteDomainController;
use crate::fake_ip::IpManager;
use crate::handler::{FakeIpHandler, HandlerState};
use crate::route_controller::nftables::NetworkManager;
use crate::route_controller::RouteController;

pub struct App {
    handler: FakeIpHandler,
    config: ArcSwap<Config>,
    domain_controller: Arc<SqliteDomainController>,
}

impl App {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        let domain_controller = Arc::new(SqliteDomainController::new(Config::get_db_path()).await?);
        domain_controller.clone().start_sync_worker();
        let state = Self::create_state(&config, domain_controller.clone()).await?;
        let handler = FakeIpHandler::new(state);
        
        Ok(Self { 
            handler,
            config: ArcSwap::from(Arc::new(config)),
            domain_controller,
        })
    }

    pub fn handler(&self) -> FakeIpHandler {
        self.handler.clone()
    }

    pub fn current_config(&self) -> Arc<Config> {
        self.config.load_full()
    }

    pub fn domain_controller(&self) -> Arc<SqliteDomainController> {
        self.domain_controller.clone()
    }

    async fn create_state(config: &Config, domain_controller: Arc<SqliteDomainController>) -> anyhow::Result<HandlerState> {
        let mut route_controller = NetworkManager::new(config.table_id, &config.iface);
        route_controller.set_tcp_mss_clamp(config.tcp_mss_clamp)
            .set_ipv4_snat(config.ipv4_snat)
            .set_ipv6_snat(config.ipv6_snat);
        route_controller.init().await?;

        let (resolver_config, resolver_opts) = config.upstream_resolver.to_resolver_parts();
        let upstream = TokioResolver::builder_with_config(resolver_config, TokioConnectionProvider::default())
            .with_options(resolver_opts)
            .build();

        let route_controller: Arc<dyn RouteController> = Arc::new(route_controller);
        
        let state = HandlerState {
            v4: IpManager::new(route_controller.clone(), IpNet::V4(config.ipv4_subnet)),
            v6: IpManager::new(route_controller.clone(), IpNet::V6(config.ipv6_subnet)),
            upstream,
            domain_controller: domain_controller as Arc<dyn DomainController>,
            route_controller,
        };

        Ok(state)
    }

    pub async fn update_config(&self, new_config: Config) -> anyhow::Result<()> {
        new_config.save()?;
        self.handler.state.load().route_controller.cleanup().await?;

        let new_state = Self::create_state(&new_config, self.domain_controller.clone()).await?;
        self.handler.state.swap(Arc::new(new_state));
        self.config.store(Arc::new(new_config));

        Ok(())
    }

    pub async fn patch_config(&self, patch: PatchConfig) -> anyhow::Result<()> {
        let current = self.config.load();
        let new_config = current.patch(patch);
        self.update_config(new_config).await
    }
}
