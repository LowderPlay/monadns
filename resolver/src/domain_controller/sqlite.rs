use std::path::PathBuf;
use std::sync::Arc;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{FromRow, Row, SqlitePool};
use log::{info, error};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use chrono::{DateTime, Utc};
use tokio::time::Instant;
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
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);

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
                match self.fetch_and_cache_list(&client, &list).await {
                    Ok(_) => {
                        sqlx::query("UPDATE domain_lists SET last_updated = ? WHERE id = ?")
                            .bind(now)
                            .bind(list.id)
                            .execute(&self.pool)
                            .await?;
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
        info!("Syncing domain list {}", list.id.unwrap());
        let start = Instant::now();
        let response = client.get(&list.url).send().await?.text().await?;
        
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM list_domains WHERE list_id = ?")
            .bind(list_id)
            .execute(&mut *tx)
            .await?;

        let lines: Vec<String> = response.lines()
            .map(|l| l.trim().trim_end_matches('.').to_string())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();

        let mut count = 0;
        for chunk in lines.chunks(1000) {
            let mut query_builder: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new("INSERT INTO list_domains (domain, list_id) ");
            query_builder.push_values(chunk, |mut b, domain| {
                b.push_bind(domain)
                 .push_bind(list_id);
            });
            query_builder.build().execute(&mut *tx).await?;
            count += chunk.len();
        }

        tx.commit().await?;
        info!("Successfully synced list {} in {:?}: {} entries", list_id, start.elapsed(), count);
        metrics::gauge!("list_domain_count", "list_id" => list_id.to_string()).set(count as f64);
        Ok(())
    }
}

#[async_trait]
impl DomainController for SqliteDomainController {
    async fn should_intercept(&self, domain: &str) -> bool {
        let domain = domain.trim_end_matches('.');
        let mut check_domains = vec![domain.to_string()];
        
        let parts: Vec<&str> = domain.split('.').collect();
        for i in 1..parts.len() {
            check_domains.push(parts[i..].join("."));
        }

        // 1. Check domain_rules
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new("SELECT domain, include_subdomains FROM domain_rules WHERE domain IN (");
        let mut separated = qb.separated(", ");
        for d in &check_domains {
            separated.push_bind(d);
        }
        separated.push_unseparated(")");
        
        let rules_result = qb.build().fetch_all(&self.pool).await;
        if let Ok(rows) = rules_result {
            for row in rows {
                let rule_domain: String = row.get(0);
                let include_subdomains: bool = row.get(1);
                if rule_domain == domain || include_subdomains {
                    metrics::counter!("domain_hits", "subdomain" => (rule_domain != domain).to_string(), "domain" => rule_domain).increment(1);
                    return true;
                }
            }
        } else if let Err(e) = rules_result {
            error!("Error querying domain_rules: {}", e);
        }

        // 2. Check list domains
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT list_domains.list_id, list_domains.domain, domain_lists.include_subdomains 
             FROM list_domains 
             JOIN domain_lists ON list_domains.list_id = domain_lists.id 
             WHERE list_domains.domain IN ("
        );
        let mut separated = qb.separated(", ");
        for d in &check_domains {
            separated.push_bind(d);
        }
        separated.push_unseparated(")");

        let list_result = qb.build().fetch_all(&self.pool).await;
        if let Ok(rows) = list_result {
            for row in rows {
                let list_id: i64 = row.get(0);
                let hit_domain: String = row.get(1);
                let include_subdomains: bool = row.get(2);
                if hit_domain == domain || include_subdomains {
                    metrics::counter!("list_hits", "list_id" => list_id.to_string(), "subdomain" => (hit_domain != domain).to_string()).increment(1);
                    return true;
                }
            }
        } else if let Err(e) = list_result {
            error!("Error querying list_domains: {}", e);
        }

        false
    }
}
