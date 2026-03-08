pub mod nftables;

use std::net::IpAddr;
use async_trait::async_trait;

#[async_trait]
pub trait RouteController: Send + Sync {
    async fn add_mapping(&self, fake_ip: IpAddr, real_ip: IpAddr, fwmark: u32) -> anyhow::Result<()>;
    async fn cleanup(&self) -> anyhow::Result<()>;
    async fn fetch_metrics(&self) -> anyhow::Result<()>;
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct DummyRouteController;

#[async_trait]
impl RouteController for DummyRouteController {
    async fn add_mapping(&self, fake_ip: IpAddr, real_ip: IpAddr, fwmark: u32) -> anyhow::Result<()> {
        println!("mapping {} -> {} (fwmark {})", fake_ip, real_ip, fwmark);
        Ok(())
    }

    async fn cleanup(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn fetch_metrics(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
