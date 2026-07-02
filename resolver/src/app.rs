use crate::config::{Config, PatchConfig};
use crate::domain_controller::sqlite::SqliteController;
use crate::domain_controller::{DomainController, PolicyId};
use crate::fake_ip::IpManager;
use crate::handler::{FakeIpHandler, HandlerState};
use crate::health_check::{self, HealthCheckSettings};
use crate::route_controller::RouteController;
use crate::route_controller::nftables::NetworkManager;
use arc_swap::ArcSwap;
use hickory_resolver::TokioResolver;
use hickory_resolver::name_server::TokioConnectionProvider;
use ipnet::IpNet;
use log::{error, info};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct App {
    handler: FakeIpHandler,
    config: Arc<ArcSwap<Config>>,
    controller: Arc<SqliteController>,
    health_check_tasks: std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl App {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        let controller = Arc::new(SqliteController::new(Config::get_db_path()).await?);
        controller.clone().start_sync_worker();
        let config = Arc::new(ArcSwap::from(Arc::new(config)));
        let state = Self::create_state(config.clone(), controller.clone()).await?;
        let handler = FakeIpHandler::new(state);

        let app = Self {
            handler,
            config,
            controller,
            health_check_tasks: std::sync::Mutex::new(Vec::new()),
        };
        app.start_metrics_worker();
        app.start_subnet_sync_worker();
        app.restart_health_checks();
        Ok(app)
    }

    fn restart_health_checks(&self) {
        let mut tasks = self.health_check_tasks.lock().unwrap();
        for task in tasks.drain(..) {
            task.abort();
        }

        let config = self.config.load();
        let settings = HealthCheckSettings {
            interval_seconds: config.health_check_interval_seconds.max(1),
            timeout_seconds: config.health_check_timeout_seconds.max(1),
            ping_count: config.health_check_ping_count.max(1),
        };
        tasks.extend(config.interfaces.iter().flat_map(|interface| {
            interface.health_check_hosts.iter().cloned().map({
                let interface = interface.clone();
                move |host| health_check::spawn(interface.clone(), host, settings)
            })
        }));
        info!("started {} interface health check workers", tasks.len());
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
                        let config = state.config.load();
                        for (subnet, interface, priority, policy_id) in subnets {
                            let iface = config.resolve_interface(interface.as_deref());
                            sync_list.push((subnet, iface.fwmark, priority, policy_id));
                        }

                        let mut last = last_synced.lock().await;
                        if sync_list != *last {
                            info!(
                                "Subnets changed, syncing to nftables ({} entries)",
                                sync_list.len()
                            );
                            if let Err(e) =
                                state.route_controller.sync_subnets(sync_list.clone()).await
                            {
                                error!("Failed to sync subnets to nftables: {}", e);
                            } else {
                                *last = sync_list;
                            }
                            info!("Synced subnets");
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

    fn build_upstream_resolver(config: &Config) -> TokioResolver {
        let (resolver_config, resolver_opts) = config.upstream_resolver.to_resolver_parts();
        TokioResolver::builder_with_config(resolver_config, TokioConnectionProvider::default())
            .with_options(resolver_opts)
            .build()
    }

    async fn sync_subnets_once(
        config: &Config,
        controller: &SqliteController,
        route_controller: Arc<dyn RouteController>,
    ) -> anyhow::Result<()> {
        let subnets = controller.get_all_subnets().await?;
        let mut sync_list = Vec::new();
        for (subnet, interface, priority, policy_id) in subnets {
            let iface = config.resolve_interface(interface.as_deref());
            sync_list.push((subnet, iface.fwmark, priority, policy_id));
        }
        route_controller.sync_subnets(sync_list).await
    }

    async fn sync_domain_policy_marks(
        config: &Config,
        controller: &SqliteController,
        route_controller: Arc<dyn RouteController>,
    ) -> anyhow::Result<()> {
        for rule in controller.list_rules().await? {
            let Some(id) = rule.id else {
                continue;
            };
            let iface = config.resolve_interface(rule.interface.as_deref());
            route_controller
                .set_policy_mark(PolicyId::DomainRule(id), iface.fwmark)
                .await?;
        }

        for list in controller.list_domain_lists().await? {
            let Some(id) = list.id else {
                continue;
            };
            let iface = config.resolve_interface(list.interface.as_deref());
            route_controller
                .set_policy_mark(PolicyId::DomainList(id), iface.fwmark)
                .await?;
        }

        Ok(())
    }

    async fn create_state(
        config: Arc<ArcSwap<Config>>,
        controller: Arc<SqliteController>,
    ) -> anyhow::Result<HandlerState> {
        let current_config = config.load();
        let route_controller = NetworkManager::new(current_config.interfaces.clone());
        route_controller.init().await?;

        let upstream = Self::build_upstream_resolver(&current_config);

        let route_controller_arc: Arc<dyn RouteController> = Arc::new(route_controller);
        if let Err(e) =
            Self::sync_subnets_once(&current_config, &controller, route_controller_arc.clone())
                .await
        {
            error!("Failed to initial sync subnets to nftables: {}", e);
        }
        if let Err(e) = Self::sync_domain_policy_marks(
            &current_config,
            &controller,
            route_controller_arc.clone(),
        )
        .await
        {
            error!("Failed to initial sync domain policies to nftables: {}", e);
        }

        let state = HandlerState {
            config: config.clone(),
            v4: IpManager::new(
                route_controller_arc.clone(),
                IpNet::V4(current_config.ipv4_subnet),
            ),
            v6: IpManager::new(
                route_controller_arc.clone(),
                IpNet::V6(current_config.ipv6_subnet),
            ),
            upstream: Arc::new(ArcSwap::from(Arc::new(upstream))),
            domain_controller: controller as Arc<dyn DomainController>,
            route_controller: route_controller_arc,
        };

        Ok(state)
    }

    pub async fn update_config(&self, new_config: Config) -> anyhow::Result<()> {
        let old_config = self.config.load_full();
        anyhow::ensure!(
            old_config.ipv4_subnet == new_config.ipv4_subnet
                && old_config.ipv6_subnet == new_config.ipv6_subnet,
            "changing fake IP subnets requires restart because existing fake-IP mappings cannot be preserved"
        );

        new_config.save()?;

        let state = self.handler.state.load();
        state
            .route_controller
            .update_interfaces(new_config.interfaces.clone())
            .await?;
        state
            .upstream
            .store(Arc::new(Self::build_upstream_resolver(&new_config)));
        self.config.store(Arc::new(new_config.clone()));

        Self::sync_subnets_once(
            &new_config,
            &self.controller,
            state.route_controller.clone(),
        )
        .await?;
        Self::sync_domain_policy_marks(
            &new_config,
            &self.controller,
            state.route_controller.clone(),
        )
        .await?;
        self.restart_health_checks();

        Ok(())
    }

    pub async fn patch_config(&self, patch: PatchConfig) -> anyhow::Result<()> {
        let current = self.config.load();
        let new_config = current.patch(patch);
        self.update_config(new_config).await
    }
}
