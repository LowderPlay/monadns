pub mod sqlite;

use std::collections::HashSet;
use async_trait::async_trait;

#[async_trait]
pub trait DomainController: Send + Sync {
    async fn should_intercept(&self, domain: &str) -> bool;
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
    async fn should_intercept(&self, domain: &str) -> bool {
        self.domains.contains(domain)
    }
}
