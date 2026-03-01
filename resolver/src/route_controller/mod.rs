pub mod nftables;

use std::net::IpAddr;
use async_trait::async_trait;

#[async_trait]
pub trait RouteController: Send + Sync {
    async fn add_mapping(&self, fake_ip: IpAddr, real_ip: IpAddr) -> anyhow::Result<()>;
    async fn cleanup(&self) -> anyhow::Result<()>;
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct DummyRouteController;

#[async_trait]
impl RouteController for DummyRouteController {
    async fn add_mapping(&self, fake_ip: IpAddr, real_ip: IpAddr) -> anyhow::Result<()> {
        println!("mapping {} -> {}", fake_ip, real_ip);
        Ok(())
    }

    async fn cleanup(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
