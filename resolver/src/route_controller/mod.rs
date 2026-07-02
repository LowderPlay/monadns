pub mod nftables;
mod sweepable;

use crate::config::InterfaceConfig;
use crate::domain_controller::PolicyId;
use async_trait::async_trait;
use std::net::IpAddr;

#[async_trait]
pub trait RouteController: Send + Sync {
    async fn add_mapping(
        &self,
        fake_ip: IpAddr,
        real_ip: IpAddr,
        policy_id: PolicyId,
        fwmark: u32,
    ) -> anyhow::Result<()>;
    async fn set_policy_mark(&self, policy_id: PolicyId, fwmark: u32) -> anyhow::Result<()>;
    async fn update_interfaces(&self, interfaces: Vec<InterfaceConfig>) -> anyhow::Result<()>;
    async fn cleanup(&self) -> anyhow::Result<()>;
    async fn fetch_metrics(&self) -> anyhow::Result<()>;
    async fn sync_subnets(&self, subnets: Vec<(String, u32, i64, PolicyId)>) -> anyhow::Result<()>;
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct DummyRouteController;

#[async_trait]
impl RouteController for DummyRouteController {
    async fn add_mapping(
        &self,
        fake_ip: IpAddr,
        real_ip: IpAddr,
        policy_id: PolicyId,
        fwmark: u32,
    ) -> anyhow::Result<()> {
        println!(
            "mapping {} -> {} (policy {}, fwmark {})",
            fake_ip, real_ip, policy_id, fwmark
        );
        Ok(())
    }

    async fn set_policy_mark(&self, policy_id: PolicyId, fwmark: u32) -> anyhow::Result<()> {
        println!("policy {} -> fwmark {}", policy_id, fwmark);
        Ok(())
    }

    async fn update_interfaces(&self, interfaces: Vec<InterfaceConfig>) -> anyhow::Result<()> {
        println!("updating {} interfaces", interfaces.len());
        Ok(())
    }

    async fn cleanup(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn fetch_metrics(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn sync_subnets(&self, subnets: Vec<(String, u32, i64, PolicyId)>) -> anyhow::Result<()> {
        println!("syncing {} subnets", subnets.len());
        Ok(())
    }
}
