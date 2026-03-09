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
    pub interface: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, ToSchema)]
pub struct DomainList {
    pub id: Option<i64>,
    pub url: String,
    pub update_interval_seconds: i64,
    pub include_subdomains: bool,
    pub last_updated: Option<DateTime<Utc>>,
    pub interface: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, ToSchema)]
pub struct IpRule {
    pub subnet: String,
    pub interface: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, ToSchema)]
pub struct IpList {
    pub id: Option<i64>,
    pub url: String,
    pub update_interval_seconds: i64,
    pub last_updated: Option<DateTime<Utc>>,
    pub interface: Option<String>,
}

#[async_trait]
pub trait SyncableList: Send + Sync + for<'a> FromRow<'a, sqlx::sqlite::SqliteRow> + Unpin {
    fn id(&self) -> Option<i64>;
    fn url(&self) -> &str;
    fn update_interval_seconds(&self) -> i64;
    fn last_updated(&self) -> Option<DateTime<Utc>>;
    fn table_name() -> &'static str;
    fn entries_table_name() -> &'static str;
    fn entry_column_name() -> &'static str;
    fn process_line(line: &str) -> String;
}

impl SyncableList for DomainList {
    fn id(&self) -> Option<i64> { self.id }
    fn url(&self) -> &str { &self.url }
    fn update_interval_seconds(&self) -> i64 { self.update_interval_seconds }
    fn last_updated(&self) -> Option<DateTime<Utc>> { self.last_updated }
    fn table_name() -> &'static str { "domain_lists" }
    fn entries_table_name() -> &'static str { "list_domains" }
    fn entry_column_name() -> &'static str { "domain" }
    fn process_line(line: &str) -> String { line.trim().trim_end_matches('.').to_string() }
}

impl SyncableList for IpList {
    fn id(&self) -> Option<i64> { self.id }
    fn url(&self) -> &str { &self.url }
    fn update_interval_seconds(&self) -> i64 { self.update_interval_seconds }
    fn last_updated(&self) -> Option<DateTime<Utc>> { self.last_updated }
    fn table_name() -> &'static str { "ip_lists" }
    fn entries_table_name() -> &'static str { "list_ips" }
    fn entry_column_name() -> &'static str { "subnet" }
    fn process_line(line: &str) -> String { line.trim().to_string() }
}

pub struct SqliteController {
    pub(crate) pool: SqlitePool,
}

impl SqliteController {
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

        info!("SQLite controller initialized");
        Ok(Self { pool })
    }

    pub fn start_sync_worker(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Err(e) = self.sync_lists::<DomainList>().await {
                    error!("Error syncing domain lists: {}", e);
                }
                if let Err(e) = self.sync_lists::<IpList>().await {
                    error!("Error syncing IP lists: {}", e);
                }
            }
        });
    }

    pub async fn add_rule(&self, domain: &str, include_subdomains: bool, interface: Option<String>) -> anyhow::Result<()> {
        let domain = domain.trim_end_matches('.');
        sqlx::query(
            "INSERT OR REPLACE INTO domain_rules (domain, include_subdomains, interface) VALUES (?, ?, ?)"
        )
        .bind(domain)
        .bind(include_subdomains)
        .bind(interface)
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
        let rules = sqlx::query_as::<_, DomainRule>("SELECT domain, include_subdomains, interface FROM domain_rules")
            .fetch_all(&self.pool)
            .await?;
        Ok(rules)
    }

    pub async fn add_domain_list(&self, list: DomainList) -> anyhow::Result<i64> {
        let res = sqlx::query(
            "INSERT INTO domain_lists (url, update_interval_seconds, include_subdomains, interface) VALUES (?, ?, ?, ?)"
        )
        .bind(&list.url)
        .bind(list.update_interval_seconds)
        .bind(list.include_subdomains)
        .bind(list.interface)
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
        let lists = sqlx::query_as::<_, DomainList>("SELECT id, url, update_interval_seconds, include_subdomains, last_updated, interface FROM domain_lists")
            .fetch_all(&self.pool)
            .await?;
        Ok(lists)
    }

    pub async fn sync_list_by_id_generic<T: SyncableList>(&self, id: i64) -> anyhow::Result<()> {
        let query = format!("SELECT * FROM {} WHERE id = ?", T::table_name());
        let list = sqlx::query_as(&query)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("List with ID {} not found in {}", id, T::table_name()))?;

        let client = reqwest::Client::new();
        self.fetch_and_cache_generic::<T>(&client, &list).await?;

        let update_query = format!("UPDATE {} SET last_updated = ? WHERE id = ?", T::table_name());
        sqlx::query(&update_query)
            .bind(Utc::now())
            .bind(id)
            .execute(&self.pool)
            .await?;
        
        Ok(())
    }

    pub async fn sync_list_by_id(&self, id: i64) -> anyhow::Result<()> {
        self.sync_list_by_id_generic::<DomainList>(id).await
    }

    pub async fn add_ip_rule(&self, subnet: &str, interface: Option<String>) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO ip_rules (subnet, interface) VALUES (?, ?)"
        )
        .bind(subnet)
        .bind(interface)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn remove_ip_rule(&self, subnet: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM ip_rules WHERE subnet = ?")
            .bind(subnet)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_ip_rules(&self) -> anyhow::Result<Vec<IpRule>> {
        let rules = sqlx::query_as::<_, IpRule>("SELECT subnet, interface FROM ip_rules")
            .fetch_all(&self.pool)
            .await?;
        Ok(rules)
    }

    pub async fn add_ip_list(&self, list: IpList) -> anyhow::Result<i64> {
        let res = sqlx::query(
            "INSERT INTO ip_lists (url, update_interval_seconds, interface) VALUES (?, ?, ?)"
        )
        .bind(&list.url)
        .bind(list.update_interval_seconds)
        .bind(list.interface)
        .execute(&self.pool)
        .await?;
        let id = res.last_insert_rowid();
        
        Ok(id)
    }

    pub async fn remove_ip_list(&self, id: i64) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM ip_lists WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_ip_lists(&self) -> anyhow::Result<Vec<IpList>> {
        let lists = sqlx::query_as::<_, IpList>("SELECT id, url, update_interval_seconds, last_updated, interface FROM ip_lists")
            .fetch_all(&self.pool)
            .await?;
        Ok(lists)
    }

    pub async fn sync_ip_list_by_id(&self, id: i64) -> anyhow::Result<()> {
        self.sync_list_by_id_generic::<IpList>(id).await
    }

    async fn sync_lists<T: SyncableList>(&self) -> anyhow::Result<()> {
        let query = format!("SELECT * FROM {}", T::table_name());
        let lists = sqlx::query_as::<_, T>(&query)
            .fetch_all(&self.pool)
            .await?;
        let client = reqwest::Client::new();
        
        for list in lists {
            let now = Utc::now();
            let should_update = match list.last_updated() {
                None => true,
                Some(last) => (now - last).num_seconds() >= list.update_interval_seconds(),
            };

            if should_update {
                match self.fetch_and_cache_generic::<T>(&client, &list).await {
                    Ok(_) => {
                        let update_query = format!("UPDATE {} SET last_updated = ? WHERE id = ?", T::table_name());
                        sqlx::query(&update_query)
                            .bind(now)
                            .bind(list.id())
                            .execute(&self.pool)
                            .await?;
                    }
                    Err(e) => {
                        error!("Failed to sync list {}: {}", list.url(), e);
                    }
                }
            }
        }
        Ok(())
    }

    async fn fetch_and_cache_generic<T: SyncableList>(&self, client: &reqwest::Client, list: &T) -> anyhow::Result<()> {
        let list_id = list.id().expect("List must have an ID");
        info!("Syncing {} list {}", T::entry_column_name(), list_id);
        let start = Instant::now();
        let response = client.get(list.url()).send().await?.text().await?;
        
        let mut tx = self.pool.begin().await?;
        let delete_query = format!("DELETE FROM {} WHERE list_id = ?", T::entries_table_name());
        sqlx::query(&delete_query)
            .bind(list_id)
            .execute(&mut *tx)
            .await?;

        let lines: Vec<String> = response.lines()
            .map(T::process_line)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();

        let mut count = 0;
        for chunk in lines.chunks(1000) {
            let mut query_builder: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new(format!(
                "INSERT INTO {} ({}, list_id) ", 
                T::entries_table_name(), 
                T::entry_column_name()
            ));
            query_builder.push_values(chunk, |mut b, entry| {
                b.push_bind(entry)
                 .push_bind(list_id);
            });
            query_builder.build().execute(&mut *tx).await?;
            count += chunk.len();
        }

        tx.commit().await?;
        info!("Successfully synced {} list {} in {:?}: {} entries", T::entry_column_name(), list_id, start.elapsed(), count);
        let metric_name = format!("list_{}_count", T::entry_column_name());
        metrics::gauge!(metric_name, "list_id" => list_id.to_string()).set(count as f64);
        Ok(())
    }

    pub async fn get_all_subnets(&self) -> anyhow::Result<Vec<(String, Option<String>)>> {
        let mut subnets = Vec::new();

        // From ip_rules
        let rules = self.list_ip_rules().await?;
        for rule in rules {
            subnets.push((rule.subnet, rule.interface));
        }

        // From ip_lists
        let rows = sqlx::query(
            "SELECT list_ips.subnet, ip_lists.interface 
             FROM list_ips 
             JOIN ip_lists ON list_ips.list_id = ip_lists.id"
        )
        .fetch_all(&self.pool)
        .await?;

        for row in rows {
            let subnet: String = row.get(0);
            let interface: Option<String> = row.get(1);
            subnets.push((subnet, interface));
        }

        Ok(subnets)
    }

    pub async fn get_all_domains(&self) -> anyhow::Result<Vec<String>> {
        let mut domains = Vec::new();

        // From domain_rules
        let rules = self.list_rules().await?;
        for rule in rules {
            domains.push(rule.domain);
        }

        // From list_domains
        let rows = sqlx::query("SELECT domain FROM list_domains")
            .fetch_all(&self.pool)
            .await?;

        for row in rows {
            let domain: String = row.get(0);
            domains.push(domain);
        }

        Ok(domains)
    }

    pub async fn update_metrics(&self) -> anyhow::Result<()> {
        let domain_rules: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM domain_rules").fetch_one(&self.pool).await?;
        let domain_lists: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM domain_lists").fetch_one(&self.pool).await?;
        let ip_rules: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM ip_rules").fetch_one(&self.pool).await?;
        let ip_lists: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM ip_lists").fetch_one(&self.pool).await?;
        let total_list_ips: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM list_ips").fetch_one(&self.pool).await?;
        let total_list_domains: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM list_domains").fetch_one(&self.pool).await?;

        metrics::gauge!("total_domain_rules_count").set(domain_rules.0 as f64);
        metrics::gauge!("total_domain_lists_count").set(domain_lists.0 as f64);
        metrics::gauge!("total_ip_rules_count").set(ip_rules.0 as f64);
        metrics::gauge!("total_ip_lists_count").set(ip_lists.0 as f64);
        metrics::gauge!("total_list_ips_count").set(total_list_ips.0 as f64);
        metrics::gauge!("total_list_domains_count").set(total_list_domains.0 as f64);

        Ok(())
    }
}


#[async_trait]
impl DomainController for SqliteController {
    async fn should_intercept(&self, domain: &str) -> Option<String> {
        let domain = domain.trim_end_matches('.');
        let mut check_domains = vec![domain.to_string()];
        
        let parts: Vec<&str> = domain.split('.').collect();
        for i in 1..parts.len() {
            check_domains.push(parts[i..].join("."));
        }

        // 1. Check domain_rules
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new("SELECT domain, include_subdomains, interface FROM domain_rules WHERE domain IN (");
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
                let interface: Option<String> = row.get(2);
                if rule_domain == domain || include_subdomains {
                    let interface = interface.unwrap_or_else(|| "default".to_string());
                    metrics::counter!("domain_hits", 
                        "subdomain" => (rule_domain != domain).to_string(), 
                        "domain" => rule_domain, 
                        "interface" => interface.to_string()).increment(1);
                    return Some(interface);
                }
            }
        } else if let Err(e) = rules_result {
            error!("Error querying domain_rules: {}", e);
        }

        // 2. Check list domains
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT list_domains.list_id, list_domains.domain, domain_lists.include_subdomains, domain_lists.interface 
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
                let interface: Option<String> = row.get(3);
                if hit_domain == domain || include_subdomains {
                    let interface = interface.unwrap_or_else(|| "default".to_string());
                    metrics::counter!("list_hits", 
                        "list_id" => list_id.to_string(), 
                        "subdomain" => (hit_domain != domain).to_string(), 
                        "interface" => interface.to_string()).increment(1);
                    return Some(interface);
                }
            }
        } else if let Err(e) = list_result {
            error!("Error querying list_domains: {}", e);
        }

        None
    }
}
