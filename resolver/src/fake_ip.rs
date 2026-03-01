use dashmap::DashMap;
use ipnet::IpNet;
use lru::LruCache;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::num::NonZeroUsize;
use std::sync::Arc;
use log::info;
use tokio::sync::Mutex;
use crate::route_controller::RouteController;

pub struct IpManager {
    real_to_fake: DashMap<IpAddr, IpAddr>,
    fake_to_real: DashMap<IpAddr, IpAddr>,
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

    pub async fn get_or_assign_ip(&self, real: &IpAddr) -> anyhow::Result<IpAddr> {
        if let Some(ip) = self.real_to_fake.get(real).map(|r| *r) {
            let mut state = self.state.lock().await;
            state.lru.get(&ip);
            return Ok(ip);
        }

        let ip = {
            let mut state = self.state.lock().await;

            if state.next_host_index < state.total_hosts {
                let ip = self.get_nth_host(state.next_host_index);
                state.next_host_index += 1;

                if let Some((old_ip, _)) = state.lru.push(ip, ()) {
                    if let Some((_, old_real)) = self.fake_to_real.remove(&old_ip) {
                        self.real_to_fake.remove(&old_real);
                    }
                }
                
                self.real_to_fake.insert(*real, ip);
                self.fake_to_real.insert(ip, *real);
                ip
            } else {
                let (old_ip, _) = state.lru.pop_lru().expect("Pool should not be empty");
                if let Some((_, old_real)) = self.fake_to_real.remove(&old_ip) {
                    self.real_to_fake.remove(&old_real);
                }

                self.real_to_fake.insert(*real, old_ip);
                self.fake_to_real.insert(old_ip, *real);
                state.lru.put(old_ip, ());
                old_ip
            }
        };

        self.controller.add_mapping(ip, *real).await?;

        Ok(ip)
    }
}
