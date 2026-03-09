use std::sync::Arc;
use arc_swap::ArcSwap;
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::TokioResolver;
use ipnet::IpNet;
use crate::config::{Config, PatchConfig};
use crate::domain_controller::DomainController;
use crate::domain_controller::sqlite::SqliteController;
use crate::fake_ip::IpManager;
use crate::handler::{FakeIpHandler, HandlerState};
use crate::route_controller::nftables::NetworkManager;
use log::{error, info};
use crate::route_controller::RouteController;
use tokio::sync::Mutex;

pub struct App {
    handler: FakeIpHandler,
    config: ArcSwap<Config>,
    controller: Arc<SqliteController>,
}

impl App {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        let controller = Arc::new(SqliteController::new(Config::get_db_path()).await?);
        controller.clone().start_sync_worker();
        let state = Self::create_state(&config, controller.clone()).await?;
        let handler = FakeIpHandler::new(state);

        let app = Self { 
            handler,
            config: ArcSwap::from(Arc::new(config)),
            controller,
        };
        app.start_metrics_worker();
        app.start_subnet_sync_worker();
        Ok(app)
    }

    fn start_metrics_worker(&self) {
        let handler = self.handler.clone();
        let controller = self.controller.clone();
        info!("starting nftables metrics worker");
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                let state = handler.state.load();
                if let Err(e) = state.route_controller.fetch_metrics().await {
                    error!("failed to fetch nft metrics: {}", e);
                }
                if let Err(e) = controller.update_metrics().await {
                    error!("failed to update database metrics: {}", e);
                }
            }
        });
    }

    fn start_subnet_sync_worker(&self) {
        let handler = self.handler.clone();
        let controller = self.controller.clone();
        let last_synced = Arc::new(Mutex::new(Vec::new()));

        info!("starting subnet sync worker");
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                let state = handler.state.load();
                match controller.get_all_subnets().await {
                    Ok(subnets) => {
                        let mut sync_list = Vec::new();
                        for (subnet, interface) in subnets {
                             let iface = state.config.resolve_interface(interface.as_deref());
                             sync_list.push((subnet, iface.fwmark));
                        }

                        let mut last = last_synced.lock().await;
                        if sync_list != *last {
                            info!("Subnets changed, syncing to nftables ({} entries)", sync_list.len());
                            if let Err(e) = state.route_controller.sync_subnets(sync_list.clone()).await {
                                error!("Failed to sync subnets to nftables: {}", e);
                            } else {
                                *last = sync_list;
                            }
                        }
                    }
                    Err(e) => error!("Failed to fetch subnets from DB: {}", e),
                }
            }
        });
    }

    pub fn handler(&self) -> FakeIpHandler {

        self.handler.clone()
    }

    pub fn current_config(&self) -> Arc<Config> {
        self.config.load_full()
    }

    pub fn controller(&self) -> Arc<SqliteController> {
        self.controller.clone()
    }

    async fn create_state(config: &Config, controller: Arc<SqliteController>) -> anyhow::Result<HandlerState> {
        let route_controller = NetworkManager::new(config.interfaces.clone());
        route_controller.init().await?;

        // Initial subnet sync
        match controller.get_all_subnets().await {
            Ok(subnets) => {
                let mut sync_list = Vec::new();
                for (subnet, interface) in subnets {
                    let iface = config.resolve_interface(interface.as_deref());
                    sync_list.push((subnet, iface.fwmark));
                }
                if let Err(e) = route_controller.sync_subnets(sync_list).await {
                    error!("Failed to initial sync subnets to nftables: {}", e);
                }
            }
            Err(e) => error!("Failed to initial fetch subnets from DB: {}", e),
        }

        let (resolver_config, resolver_opts) = config.upstream_resolver.to_resolver_parts();
        let upstream = TokioResolver::builder_with_config(resolver_config, TokioConnectionProvider::default())
            .with_options(resolver_opts)
            .build();

        let route_controller_arc: Arc<dyn RouteController> = Arc::new(route_controller);
        
        let state = HandlerState {
            config: Arc::new(config.clone()),
            v4: IpManager::new(route_controller_arc.clone(), IpNet::V4(config.ipv4_subnet)),
            v6: IpManager::new(route_controller_arc.clone(), IpNet::V6(config.ipv6_subnet)),
            upstream,
            domain_controller: controller as Arc<dyn DomainController>,
            route_controller: route_controller_arc,
        };

        Ok(state)
    }

    pub async fn update_config(&self, new_config: Config) -> anyhow::Result<()> {
        new_config.save()?;
        self.handler.state.load().route_controller.cleanup().await?;

        let new_state = Self::create_state(&new_config, self.controller.clone()).await?;
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
