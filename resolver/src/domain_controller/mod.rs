pub mod sqlite;

use async_trait::async_trait;
use std::collections::HashSet;
use std::fmt::{Display, Formatter};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolicyKind {
    DomainRule = 1,
    DomainList = 2,
    IpRule = 3,
    IpList = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolicyId {
    DomainRule(i64),
    DomainList(i64),
    IpRule(i64),
    IpList(i64),
}

impl PolicyId {
    const ID_BITS: u64 = 56;
    const MAX_ID: i64 = (1_i64 << Self::ID_BITS) - 1;
    const NFT_ID_BITS: u32 = 24;
    const NFT_MAX_ID: i64 = (1_i64 << Self::NFT_ID_BITS) - 1;

    pub fn kind(self) -> PolicyKind {
        match self {
            PolicyId::DomainRule(_) => PolicyKind::DomainRule,
            PolicyId::DomainList(_) => PolicyKind::DomainList,
            PolicyId::IpRule(_) => PolicyKind::IpRule,
            PolicyId::IpList(_) => PolicyKind::IpList,
        }
    }

    pub fn raw_id(self) -> i64 {
        match self {
            PolicyId::DomainRule(id)
            | PolicyId::DomainList(id)
            | PolicyId::IpRule(id)
            | PolicyId::IpList(id) => id,
        }
    }

    pub fn nft_key(self) -> anyhow::Result<u32> {
        let id = self.raw_id();
        anyhow::ensure!(id >= 0, "policy id must be non-negative");
        anyhow::ensure!(
            id <= Self::NFT_MAX_ID,
            "policy id is too large for nftables policy map"
        );

        Ok(((self.kind() as u32) << Self::NFT_ID_BITS) | id as u32)
    }
}

impl TryFrom<PolicyId> for u64 {
    type Error = anyhow::Error;

    fn try_from(value: PolicyId) -> Result<Self, Self::Error> {
        let id = value.raw_id();
        anyhow::ensure!(id >= 0, "policy id must be non-negative");
        anyhow::ensure!(id <= PolicyId::MAX_ID, "policy id is too large");

        Ok(((value.kind() as u64) << PolicyId::ID_BITS) | id as u64)
    }
}

impl Display for PolicyId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyId::DomainRule(id) => write!(f, "domain-rule:{}", id),
            PolicyId::DomainList(id) => write!(f, "domain-list:{}", id),
            PolicyId::IpRule(id) => write!(f, "ip-rule:{}", id),
            PolicyId::IpList(id) => write!(f, "ip-list:{}", id),
        }
    }
}

pub struct Intercept {
    pub interface: String,
    pub policy_id: PolicyId,
}

#[async_trait]
pub trait DomainController: Send + Sync {
    async fn should_intercept(&self, domain: &str) -> Option<Intercept>;
}

#[allow(dead_code)]
pub struct DummyDomainController {
    domains: HashSet<String>,
}

#[allow(dead_code)]
impl DummyDomainController {
    pub fn new(domains: Vec<String>) -> DummyDomainController {
        DummyDomainController {
            domains: domains.into_iter().collect(),
        }
    }
}

#[async_trait]
impl DomainController for DummyDomainController {
    async fn should_intercept(&self, domain: &str) -> Option<Intercept> {
        if self.domains.contains(domain) {
            Some(Intercept {
                interface: "default".to_string(),
                policy_id: PolicyId::DomainList(0),
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PolicyId, PolicyKind};

    #[test]
    fn policy_ids_include_kind_namespace() {
        let domain_rule = u64::try_from(PolicyId::DomainRule(1)).unwrap();
        let domain_list = u64::try_from(PolicyId::DomainList(1)).unwrap();
        let ip_rule = u64::try_from(PolicyId::IpRule(1)).unwrap();
        let ip_list = u64::try_from(PolicyId::IpList(1)).unwrap();

        assert_ne!(domain_rule, domain_list);
        assert_ne!(domain_rule, ip_rule);
        assert_ne!(ip_rule, ip_list);
        assert_eq!(domain_rule >> 56, PolicyKind::DomainRule as u64);
        assert_eq!(domain_list >> 56, PolicyKind::DomainList as u64);
        assert_eq!(ip_rule >> 56, PolicyKind::IpRule as u64);
        assert_eq!(ip_list >> 56, PolicyKind::IpList as u64);
    }

    #[test]
    fn nft_policy_keys_include_kind_namespace() {
        let domain_rule = PolicyId::DomainRule(1).nft_key().unwrap();
        let domain_list = PolicyId::DomainList(1).nft_key().unwrap();
        let ip_rule = PolicyId::IpRule(1).nft_key().unwrap();
        let ip_list = PolicyId::IpList(1).nft_key().unwrap();

        assert_ne!(domain_rule, domain_list);
        assert_ne!(domain_rule, ip_rule);
        assert_ne!(ip_rule, ip_list);
        assert_eq!(domain_rule >> 24, PolicyKind::DomainRule as u32);
        assert_eq!(domain_list >> 24, PolicyKind::DomainList as u32);
        assert_eq!(ip_rule >> 24, PolicyKind::IpRule as u32);
        assert_eq!(ip_list >> 24, PolicyKind::IpList as u32);
    }
}
