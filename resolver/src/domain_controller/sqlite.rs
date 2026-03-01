use std::path::PathBuf;
use std::sync::Arc;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{FromRow, SqlitePool};
use log::{debug, info, error};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use chrono::{DateTime, Utc};
use crate::domain_controller::DomainController;

#[derive(Debug, Serialize, Deserialize, FromRow, ToSchema)]
pub struct DomainRule {
    pub domain: String,
    pub include_subdomains: bool,
}

#[derive(Debug, Serialize, Deserialize, FromRow, ToSchema)]
pub struct DomainList {
    pub id: Option<i64>,
    pub url: String,
    pub update_interval_seconds: i64,
    pub include_subdomains: bool,
    pub last_updated: Option<DateTime<Utc>>,
}

pub struct SqliteDomainController {
    pub(crate) pool: SqlitePool,
}

impl SqliteDomainController {
    pub async fn new(db_path: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = db_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let options = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true);

        let pool = SqlitePool::connect_with(options).await?;

        // Initialize schema using migrations
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await?;

        info!("SQLite domain controller initialized");
        Ok(Self { pool })
    }

    pub fn start_sync_worker(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Err(e) = self.sync_domain_lists().await {
                    error!("Error syncing domain lists: {}", e);
                }
            }
        });
    }

    pub async fn add_rule(&self, domain: &str, include_subdomains: bool) -> anyhow::Result<()> {
        let domain = domain.trim_end_matches('.');
        sqlx::query(
            "INSERT OR REPLACE INTO domain_rules (domain, include_subdomains) VALUES (?, ?)"
        )
        .bind(domain)
        .bind(include_subdomains)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn remove_rule(&self, domain: &str) -> anyhow::Result<()> {
        let domain = domain.trim_end_matches('.');
        sqlx::query("DELETE FROM domain_rules WHERE domain = ?")
            .bind(domain)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_rules(&self) -> anyhow::Result<Vec<DomainRule>> {
        let rules = sqlx::query_as::<_, DomainRule>("SELECT domain, include_subdomains FROM domain_rules")
            .fetch_all(&self.pool)
            .await?;
        Ok(rules)
    }

    pub async fn add_domain_list(&self, list: DomainList) -> anyhow::Result<i64> {
        let res = sqlx::query(
            "INSERT INTO domain_lists (url, update_interval_seconds, include_subdomains) VALUES (?, ?, ?)"
        )
        .bind(&list.url)
        .bind(list.update_interval_seconds)
        .bind(list.include_subdomains)
        .execute(&self.pool)
        .await?;
        let id = res.last_insert_rowid();
        
        // Update after added
        if let Err(e) = self.sync_list_by_id(id).await {
            error!("Failed to initial sync for list {}: {}", id, e);
        }
        
        Ok(id)
    }

    pub async fn remove_domain_list(&self, id: i64) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM domain_lists WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_domain_lists(&self) -> anyhow::Result<Vec<DomainList>> {
        let lists = sqlx::query_as::<_, DomainList>("SELECT id, url, update_interval_seconds, include_subdomains, last_updated FROM domain_lists")
            .fetch_all(&self.pool)
            .await?;
        Ok(lists)
    }

    pub async fn sync_list_by_id(&self, id: i64) -> anyhow::Result<()> {
        let list = sqlx::query_as::<_, DomainList>("SELECT id, url, update_interval_seconds, include_subdomains, last_updated FROM domain_lists WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("List with ID {} not found", id))?;

        let client = reqwest::Client::new();
        self.fetch_and_cache_list(&client, &list).await?;
        
        sqlx::query("UPDATE domain_lists SET last_updated = ? WHERE id = ?")
            .bind(Utc::now())
            .bind(id)
            .execute(&self.pool)
            .await?;
        
        Ok(())
    }

    async fn sync_domain_lists(&self) -> anyhow::Result<()> {
        let lists = self.list_domain_lists().await?;
        let client = reqwest::Client::new();
        
        for list in lists {
            let now = Utc::now();
            let should_update = match list.last_updated {
                None => true,
                Some(last) => (now - last).num_seconds() >= list.update_interval_seconds,
            };

            if should_update {
                debug!("Syncing domain list {}", list.id.unwrap());
                match self.fetch_and_cache_list(&client, &list).await {
                    Ok(_) => {
                        sqlx::query("UPDATE domain_lists SET last_updated = ? WHERE id = ?")
                            .bind(now)
                            .bind(list.id)
                            .execute(&self.pool)
                            .await?;
                        info!("Successfully synced list {}", list.id.unwrap());
                    }
                    Err(e) => {
                        error!("Failed to sync list {}: {}", list.url, e);
                    }
                }
            }
        }
        Ok(())
    }

    async fn fetch_and_cache_list(&self, client: &reqwest::Client, list: &DomainList) -> anyhow::Result<()> {
        let list_id = list.id.expect("List must have an ID");
        let response = client.get(&list.url).send().await?.text().await?;
        
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM list_domains WHERE list_id = ?")
            .bind(list_id)
            .execute(&mut *tx)
            .await?;

        for line in response.lines() {
            let domain = line.trim().trim_end_matches('.');
            if !domain.is_empty() && !domain.starts_with('#') {
                sqlx::query("INSERT INTO list_domains (domain, list_id) VALUES (?, ?)")
                    .bind(domain)
                    .bind(list_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }
}

#[async_trait]
impl DomainController for SqliteDomainController {
    async fn should_intercept(&self, domain: &str) -> bool {
        let domain = domain.trim_end_matches('.');

        // Check singular rules (exact match or subdomain match)
        let result = sqlx::query("SELECT 1 FROM domain_rules WHERE domain = ?")
            .bind(domain)
            .fetch_optional(&self.pool)
            .await;

        match result {
            Ok(Some(_)) => return true,
            Ok(None) => {},
            Err(e) => {
                debug!("Error querying domain_rules for exact match: {}", e);
            }
        }

        // Check list domains (exact match)
        let list_result = sqlx::query("SELECT 1 FROM list_domains WHERE domain = ?")
            .bind(domain)
            .fetch_optional(&self.pool)
            .await;

        match list_result {
            Ok(Some(_)) => return true,
            Ok(None) => {},
            Err(e) => {
                debug!("Error querying list_domains for exact match: {}", e);
            }
        }

        let parts: Vec<&str> = domain.split('.').collect();
        for i in 1..parts.len() {
            let parent = parts[i..].join(".");
            
            // Check singular rule with include_subdomains=1
            let parent_rule = sqlx::query("SELECT 1 FROM domain_rules WHERE domain = ? AND include_subdomains = 1")
                .bind(&parent)
                .fetch_optional(&self.pool)
                .await;

            match parent_rule {
                Ok(Some(_)) => return true,
                Ok(None) => {},
                Err(e) => {
                    debug!("Error querying domain_rules for parent {}: {}", parent, e);
                }
            }

            // Check if it belongs to any list with include_subdomains=1
            let parent_list = sqlx::query(
                "SELECT 1 FROM list_domains 
                 JOIN domain_lists ON list_domains.list_id = domain_lists.id 
                 WHERE list_domains.domain = ? AND domain_lists.include_subdomains = 1"
            )
            .bind(&parent)
            .fetch_optional(&self.pool)
            .await;

            match parent_list {
                Ok(Some(_)) => return true,
                Ok(None) => {},
                Err(e) => {
                    debug!("Error querying list_domains for parent {}: {}", parent, e);
                }
            }
        }

        false
    }
}
