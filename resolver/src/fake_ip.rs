use crate::domain_controller::PolicyId;
use crate::route_controller::RouteController;
use dashmap::DashMap;
use ipnet::IpNet;
use log::info;
use lru::LruCache;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Copy)]
struct AssignedIp {
    fake_ip: IpAddr,
    fwmark: u32,
}

pub struct IpManager {
    real_to_fake: DashMap<(IpAddr, PolicyId), AssignedIp>,
    fake_to_real: DashMap<IpAddr, (IpAddr, PolicyId)>,
    state: Mutex<IpManagerState>,
    network: IpNet,
    controller: Arc<dyn RouteController>,
}

struct IpManagerState {
    lru: LruCache<IpAddr, ()>,
    next_host_index: u64,
    total_hosts: u64,
}

impl IpManager {
    pub fn new(controller: Arc<dyn RouteController>, network: IpNet) -> Self {
        let total_hosts = match network {
            IpNet::V4(v4) => {
                let prefix = v4.prefix_len();
                if prefix >= 31 {
                    0
                } else {
                    (1u64 << (32 - prefix)) - 2
                }
            }
            IpNet::V6(v6) => {
                let prefix = v6.prefix_len();
                if prefix >= 128 {
                    0
                } else {
                    let bits = 128 - prefix;
                    if bits >= 64 {
                        u64::MAX
                    } else {
                        (1u64 << bits) - 1
                    }
                }
            }
        };

        // Cap LRU capacity to prevent excessive memory allocation for massive networks.
        // A capacity of 1,000,000 entries uses approximately 40-64MB of memory.
        let capacity = (total_hosts as usize).min(1_000_000).max(1);
        let lru = LruCache::new(NonZeroUsize::new(capacity).unwrap());

        info!(
            "{} pool initialized: {} total hosts, LRU tracking capacity: {}",
            network, total_hosts, capacity
        );

        Self {
            real_to_fake: DashMap::new(),
            fake_to_real: DashMap::new(),
            state: Mutex::new(IpManagerState {
                lru,
                next_host_index: 0,
                total_hosts,
            }),
            network,
            controller,
        }
    }

    fn get_nth_host(&self, n: u64) -> IpAddr {
        match self.network {
            IpNet::V4(v4) => {
                let start = u32::from(v4.network()) + 1;
                IpAddr::V4(Ipv4Addr::from(start + n as u32))
            }
            IpNet::V6(v6) => {
                let start = u128::from(v6.network()) + 1;
                IpAddr::V6(Ipv6Addr::from(start + n as u128))
            }
        }
    }

    pub async fn get_or_assign_ip(
        &self,
        real: &IpAddr,
        policy_id: PolicyId,
        fwmark: u32,
    ) -> anyhow::Result<IpAddr> {
        let key = (*real, policy_id);
        if let Some(assigned) = self.real_to_fake.get(&key).map(|r| *r) {
            if assigned.fwmark != fwmark {
                self.controller.set_policy_mark(policy_id, fwmark).await?;
                self.real_to_fake.insert(
                    key,
                    AssignedIp {
                        fake_ip: assigned.fake_ip,
                        fwmark,
                    },
                );
            }

            self.state.lock().await.lru.get(&assigned.fake_ip);
            return Ok(assigned.fake_ip);
        }

        // Keep allocation and installation coordinated. In particular, do not publish
        // a fake IP in the in-memory indexes until nftables accepted the mapping.
        let mut state = self.state.lock().await;

        // Another request may have installed this key while this one waited for the lock.
        if let Some(assigned) = self.real_to_fake.get(&key).map(|r| *r) {
            state.lru.get(&assigned.fake_ip);
            return Ok(assigned.fake_ip);
        }

        let (ip, replace_existing) = if state.next_host_index < state.total_hosts {
            let ip = self.get_nth_host(state.next_host_index);
            state.next_host_index += 1;
            (ip, false)
        } else {
            let (old_ip, _) = state.lru.peek_lru().expect("Pool should not be empty");
            (*old_ip, true)
        };

        if let Err(error) = self
            .controller
            .add_mapping(ip, *real, policy_id, replace_existing)
            .await
        {
            if !replace_existing {
                state.next_host_index -= 1;
            }
            return Err(error);
        }

        if replace_existing {
            state.lru.pop_lru();
            if let Some((_, old_key)) = self.fake_to_real.remove(&ip) {
                self.real_to_fake.remove(&old_key);
            }
        }

        self.real_to_fake.insert(
            key,
            AssignedIp {
                fake_ip: ip,
                fwmark,
            },
        );
        self.fake_to_real.insert(ip, key);
        state.lru.put(ip, ());
        Ok(ip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::InterfaceConfig;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FailOnceController {
        mapping_attempts: AtomicUsize,
    }

    #[async_trait]
    impl RouteController for FailOnceController {
        async fn add_mapping(
            &self,
            _fake_ip: IpAddr,
            _real_ip: IpAddr,
            _policy_id: PolicyId,
            _replace_existing: bool,
        ) -> anyhow::Result<()> {
            if self.mapping_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                anyhow::bail!("injected mapping failure");
            }
            Ok(())
        }

        async fn set_policy_mark(&self, _policy_id: PolicyId, _fwmark: u32) -> anyhow::Result<()> {
            Ok(())
        }

        async fn update_interfaces(&self, _interfaces: Vec<InterfaceConfig>) -> anyhow::Result<()> {
            Ok(())
        }

        async fn cleanup(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn fetch_metrics(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn sync_subnets(
            &self,
            _subnets: Vec<(String, u32, i64, PolicyId)>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn failed_mapping_is_not_published_and_is_retried() {
        let controller = Arc::new(FailOnceController {
            mapping_attempts: AtomicUsize::new(0),
        });
        let manager = IpManager::new(controller.clone(), "198.18.0.0/30".parse().unwrap());
        let real: IpAddr = "203.0.113.10".parse().unwrap();
        let policy_id = PolicyId::DomainRule(1);

        assert!(manager.get_or_assign_ip(&real, policy_id, 7).await.is_err());
        assert!(manager.real_to_fake.get(&(real, policy_id)).is_none());

        let fake = manager.get_or_assign_ip(&real, policy_id, 7).await.unwrap();

        assert_eq!(fake, "198.18.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(controller.mapping_attempts.load(Ordering::SeqCst), 2);
        assert_eq!(
            manager
                .real_to_fake
                .get(&(real, policy_id))
                .map(|entry| entry.fake_ip),
            Some(fake)
        );
    }
}
