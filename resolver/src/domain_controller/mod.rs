pub mod sqlite;

use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use async_trait::async_trait;

pub struct Intercept {
    pub interface: String,
    pub reason: InterceptReason
}

pub enum InterceptReason {
    List(i64),
    Domain,
}

impl Display for InterceptReason {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            InterceptReason::List(id) => write!(f, "{}", id),
            InterceptReason::Domain => write!(f, "domain"),
        }
    }
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
            Some(Intercept { interface: "default".to_string(), reason: InterceptReason::List(0) })
        } else {
            None
        }
    }
}
