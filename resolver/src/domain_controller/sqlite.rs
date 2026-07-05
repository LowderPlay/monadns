use crate::domain_controller::{DomainController, Intercept, PolicyId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use log::{error, info};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{FromRow, Row, SqlitePool};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::Instant;
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, FromRow, ToSchema)]
pub struct DomainRule {
    pub id: Option<i64>,
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
    pub priority: i64,
}

#[derive(Debug, Serialize, Deserialize, FromRow, ToSchema)]
pub struct IpRule {
    pub id: Option<i64>,
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
    pub priority: i64,
}

#[derive(Debug, Serialize, Deserialize, FromRow, ToSchema)]
pub struct GeoSource {
    pub id: Option<i64>,
    pub url: String,
    pub r#type: String, // 'geosite' or 'geoip'
    pub update_interval_seconds: i64,
    pub last_updated: Option<DateTime<Utc>>,
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
    fn id(&self) -> Option<i64> {
        self.id
    }
    fn url(&self) -> &str {
        &self.url
    }
    fn update_interval_seconds(&self) -> i64 {
        self.update_interval_seconds
    }
    fn last_updated(&self) -> Option<DateTime<Utc>> {
        self.last_updated
    }
    fn table_name() -> &'static str {
        "domain_lists"
    }
    fn entries_table_name() -> &'static str {
        "list_domains"
    }
    fn entry_column_name() -> &'static str {
        "domain"
    }
    fn process_line(line: &str) -> String {
        line.trim().trim_end_matches('.').to_string()
    }
}

impl SyncableList for IpList {
    fn id(&self) -> Option<i64> {
        self.id
    }
    fn url(&self) -> &str {
        &self.url
    }
    fn update_interval_seconds(&self) -> i64 {
        self.update_interval_seconds
    }
    fn last_updated(&self) -> Option<DateTime<Utc>> {
        self.last_updated
    }
    fn table_name() -> &'static str {
        "ip_lists"
    }
    fn entries_table_name() -> &'static str {
        "list_ips"
    }
    fn entry_column_name() -> &'static str {
        "subnet"
    }
    fn process_line(line: &str) -> String {
        line.trim().to_string()
    }
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
        sqlx::migrate!("./migrations").run(&pool).await?;

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
                if let Err(e) = self.sync_geo_sources().await {
                    error!("Error syncing geo sources: {}", e);
                }
            }
        });
    }

    pub async fn add_rule(
        &self,
        domain: &str,
        include_subdomains: bool,
        interface: Option<String>,
    ) -> anyhow::Result<i64> {
        let domain = domain.trim_end_matches('.');
        sqlx::query(
            "INSERT INTO domain_rules (domain, include_subdomains, interface) VALUES (?, ?, ?)
             ON CONFLICT(domain) DO UPDATE SET
                include_subdomains = excluded.include_subdomains,
                interface = excluded.interface",
        )
        .bind(domain)
        .bind(include_subdomains)
        .bind(interface)
        .execute(&self.pool)
        .await?;
        let (id,): (i64,) = sqlx::query_as("SELECT rowid FROM domain_rules WHERE domain = ?")
            .bind(domain)
            .fetch_one(&self.pool)
            .await?;
        Ok(id)
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
        let rules = sqlx::query_as::<_, DomainRule>(
            "SELECT rowid AS id, domain, include_subdomains, interface FROM domain_rules",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rules)
    }

    pub async fn add_domain_list(&self, list: DomainList) -> anyhow::Result<i64> {
        if let Some(id) = list.id {
            sqlx::query(
                "UPDATE domain_lists
                 SET url = ?, update_interval_seconds = ?, include_subdomains = ?, interface = ?, priority = ?
                 WHERE id = ?",
            )
            .bind(&list.url)
            .bind(list.update_interval_seconds)
            .bind(list.include_subdomains)
            .bind(list.interface)
            .bind(list.priority)
            .bind(id)
            .execute(&self.pool)
            .await?;

            return Ok(id);
        }

        let res = sqlx::query(
            "INSERT INTO domain_lists (url, update_interval_seconds, include_subdomains, interface, priority) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(&list.url)
        .bind(list.update_interval_seconds)
        .bind(list.include_subdomains)
        .bind(list.interface)
        .bind(list.priority)
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
        let lists = sqlx::query_as::<_, DomainList>("SELECT id, url, update_interval_seconds, include_subdomains, last_updated, interface, priority FROM domain_lists ORDER BY priority DESC, id ASC")
            .fetch_all(&self.pool)
            .await?;
        Ok(lists)
    }

    pub async fn reorder_domain_lists(&self, ids: Vec<i64>) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        for (i, id) in ids.into_iter().enumerate() {
            sqlx::query("UPDATE domain_lists SET priority = ? WHERE id = ?")
                .bind(-(i as i64)) // Lower index = higher priority (using negative to keep 0 as default lowest)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn sync_list_by_id_generic<T: SyncableList>(&self, id: i64) -> anyhow::Result<()> {
        let query = format!("SELECT * FROM {} WHERE id = ?", T::table_name());
        let list = sqlx::query_as(&query)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!("List with ID {} not found in {}", id, T::table_name())
            })?;

        let client = reqwest::Client::new();
        self.fetch_and_cache_generic::<T>(&client, &list).await?;

        let update_query = format!(
            "UPDATE {} SET last_updated = ? WHERE id = ?",
            T::table_name()
        );
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

    pub async fn add_ip_rule(
        &self,
        subnet: &str,
        interface: Option<String>,
    ) -> anyhow::Result<i64> {
        sqlx::query(
            "INSERT INTO ip_rules (subnet, interface) VALUES (?, ?)
             ON CONFLICT(subnet) DO UPDATE SET
                interface = excluded.interface",
        )
        .bind(subnet)
        .bind(interface)
        .execute(&self.pool)
        .await?;
        let (id,): (i64,) = sqlx::query_as("SELECT rowid FROM ip_rules WHERE subnet = ?")
            .bind(subnet)
            .fetch_one(&self.pool)
            .await?;
        Ok(id)
    }

    pub async fn remove_ip_rule(&self, subnet: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM ip_rules WHERE subnet = ?")
            .bind(subnet)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_ip_rules(&self) -> anyhow::Result<Vec<IpRule>> {
        let rules =
            sqlx::query_as::<_, IpRule>("SELECT rowid AS id, subnet, interface FROM ip_rules")
                .fetch_all(&self.pool)
                .await?;
        Ok(rules)
    }

    pub async fn add_ip_list(&self, list: IpList) -> anyhow::Result<i64> {
        if let Some(id) = list.id {
            sqlx::query(
                "UPDATE ip_lists
                 SET url = ?, update_interval_seconds = ?, interface = ?, priority = ?
                 WHERE id = ?",
            )
            .bind(&list.url)
            .bind(list.update_interval_seconds)
            .bind(list.interface)
            .bind(list.priority)
            .bind(id)
            .execute(&self.pool)
            .await?;

            return Ok(id);
        }

        let res = sqlx::query(
            "INSERT INTO ip_lists (url, update_interval_seconds, interface, priority) VALUES (?, ?, ?, ?)"
        )
        .bind(&list.url)
        .bind(list.update_interval_seconds)
        .bind(list.interface)
        .bind(list.priority)
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
        let lists = sqlx::query_as::<_, IpList>("SELECT id, url, update_interval_seconds, last_updated, interface, priority FROM ip_lists ORDER BY priority DESC, id ASC")
            .fetch_all(&self.pool)
            .await?;
        Ok(lists)
    }

    pub async fn reorder_ip_lists(&self, ids: Vec<i64>) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        for (i, id) in ids.into_iter().enumerate() {
            sqlx::query("UPDATE ip_lists SET priority = ? WHERE id = ?")
                .bind(-(i as i64))
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn sync_ip_list_by_id(&self, id: i64) -> anyhow::Result<()> {
        self.sync_list_by_id_generic::<IpList>(id).await
    }

    async fn sync_lists<T: SyncableList>(&self) -> anyhow::Result<()> {
        let query = format!(
            "SELECT * FROM {} ORDER BY priority DESC, id ASC",
            T::table_name()
        );
        let lists = sqlx::query_as::<_, T>(&query).fetch_all(&self.pool).await?;
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
                        let update_query = format!(
                            "UPDATE {} SET last_updated = ? WHERE id = ?",
                            T::table_name()
                        );
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

    async fn fetch_and_cache_generic<T: SyncableList>(
        &self,
        client: &reqwest::Client,
        list: &T,
    ) -> anyhow::Result<()> {
        let list_id = list.id().expect("List must have an ID");
        let url = list.url();

        if url.starts_with("geosite://") || url.starts_with("geoip://") {
            info!("Skipping fetch for virtual geo list {} ({})", list_id, url);
            return Ok(());
        }

        info!("Syncing {} list {}", T::entry_column_name(), list_id);
        let start = Instant::now();
        let response = client.get(url).send().await?.text().await?;

        let mut tx = self.pool.begin().await?;
        let delete_query = format!("DELETE FROM {} WHERE list_id = ?", T::entries_table_name());
        sqlx::query(&delete_query)
            .bind(list_id)
            .execute(&mut *tx)
            .await?;

        let lines: Vec<String> = response
            .lines()
            .map(T::process_line)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();

        let mut count = 0;
        for chunk in lines.chunks(1000) {
            let mut query_builder: sqlx::QueryBuilder<sqlx::Sqlite> =
                sqlx::QueryBuilder::new(format!(
                    "INSERT INTO {} ({}, list_id) ",
                    T::entries_table_name(),
                    T::entry_column_name()
                ));
            query_builder.push_values(chunk, |mut b, entry| {
                b.push_bind(entry).push_bind(list_id);
            });
            query_builder.build().execute(&mut *tx).await?;
            count += chunk.len();
        }

        tx.commit().await?;
        info!(
            "Successfully synced {} list {} in {:?}: {} entries",
            T::entry_column_name(),
            list_id,
            start.elapsed(),
            count
        );
        let metric_name = format!("list_{}_count", T::entry_column_name());
        metrics::gauge!(metric_name, "list_id" => list_id.to_string()).set(count as f64);
        Ok(())
    }

    pub async fn get_all_subnets(
        &self,
    ) -> anyhow::Result<Vec<(String, Option<String>, i64, PolicyId)>> {
        let mut subnets = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // 1. IP Rules - Give them very high priority (e.g. 1000000)
        let rules = self.list_ip_rules().await?;
        for rule in rules {
            if seen.insert(rule.subnet.clone()) {
                let id = rule
                    .id
                    .ok_or_else(|| anyhow::anyhow!("IP rule {} is missing an id", rule.subnet))?;
                subnets.push((rule.subnet, rule.interface, 1000000, PolicyId::IpRule(id)));
            }
        }

        // 2. IP Lists and GeoIP (Combined and ordered by priority)
        let rows = sqlx::query(
            "SELECT subnet, interface, priority, id FROM (
                SELECT list_ips.subnet, ip_lists.interface, ip_lists.priority, ip_lists.id
                FROM list_ips 
                JOIN ip_lists ON list_ips.list_id = ip_lists.id
                
                UNION ALL
                
                SELECT geoip_data.subnet, ip_lists.interface, ip_lists.priority, ip_lists.id
                FROM geoip_data 
                JOIN ip_lists ON ip_lists.url = 'geoip://' || geoip_data.category
            )
            ORDER BY priority DESC, id ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        for row in rows {
            let subnet: String = row.get(0);
            let interface: Option<String> = row.get(1);
            let priority: i64 = row.get(2);
            let list_id: i64 = row.get(3);
            if seen.insert(subnet.clone()) {
                subnets.push((subnet, interface, priority, PolicyId::IpList(list_id)));
            }
        }

        Ok(subnets)
    }

    pub async fn get_all_domains(&self) -> anyhow::Result<Vec<String>> {
        let mut domains = Vec::new();
        let mut seen = std::collections::HashSet::new();

        let rules = self.list_rules().await?;
        for rule in rules {
            if seen.insert(rule.domain.clone()) {
                domains.push(rule.domain);
            }
        }

        let rows = sqlx::query(
            "SELECT domain FROM (
                SELECT list_domains.domain, domain_lists.priority, domain_lists.id
                FROM list_domains 
                JOIN domain_lists ON list_domains.list_id = domain_lists.id
                
                UNION ALL
                
                SELECT geosite_data.domain, domain_lists.priority, domain_lists.id
                FROM geosite_data 
                JOIN domain_lists ON domain_lists.url = 'geosite://' || geosite_data.category
                WHERE geosite_data.type IN (2, 3)
            )
            ORDER BY priority DESC, id ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        for row in rows {
            let domain: String = row.get(0);
            if seen.insert(domain.clone()) {
                domains.push(domain);
            }
        }

        Ok(domains)
    }

    pub async fn update_metrics(&self) -> anyhow::Result<()> {
        let domain_rules: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM domain_rules")
            .fetch_one(&self.pool)
            .await?;
        let domain_lists: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM domain_lists")
            .fetch_one(&self.pool)
            .await?;
        let ip_rules: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM ip_rules")
            .fetch_one(&self.pool)
            .await?;
        let ip_lists: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM ip_lists")
            .fetch_one(&self.pool)
            .await?;
        let total_list_ips: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM list_ips")
            .fetch_one(&self.pool)
            .await?;
        let total_list_domains: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM list_domains")
            .fetch_one(&self.pool)
            .await?;
        let geosite_entries: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM geosite_data")
            .fetch_one(&self.pool)
            .await?;
        let geoip_entries: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM geoip_data")
            .fetch_one(&self.pool)
            .await?;

        metrics::gauge!("total_domain_rules_count").set(domain_rules.0 as f64);
        metrics::gauge!("total_domain_lists_count").set(domain_lists.0 as f64);
        metrics::gauge!("total_ip_rules_count").set(ip_rules.0 as f64);
        metrics::gauge!("total_ip_lists_count").set(ip_lists.0 as f64);
        metrics::gauge!("total_list_ips_count").set(total_list_ips.0 as f64);
        metrics::gauge!("total_list_domains_count").set(total_list_domains.0 as f64);
        metrics::gauge!("total_geosite_entries_count").set(geosite_entries.0 as f64);
        metrics::gauge!("total_geoip_entries_count").set(geoip_entries.0 as f64);

        Ok(())
    }

    pub async fn add_geo_source(&self, source: GeoSource) -> anyhow::Result<i64> {
        let res = sqlx::query(
            "INSERT INTO geo_sources (url, type, update_interval_seconds) VALUES (?, ?, ?)",
        )
        .bind(&source.url)
        .bind(&source.r#type)
        .bind(source.update_interval_seconds)
        .execute(&self.pool)
        .await?;
        Ok(res.last_insert_rowid())
    }

    pub async fn remove_geo_source(&self, id: i64) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM geo_sources WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_geo_sources(&self) -> anyhow::Result<Vec<GeoSource>> {
        let sources = sqlx::query_as::<_, GeoSource>(
            "SELECT id, url, type, update_interval_seconds, last_updated FROM geo_sources",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(sources)
    }

    pub async fn sync_geo_source_by_id(&self, id: i64) -> anyhow::Result<()> {
        let source = sqlx::query_as::<_, GeoSource>("SELECT * FROM geo_sources WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Geo source with ID {} not found", id))?;

        let client = reqwest::Client::new();
        self.fetch_and_cache_geo(&client, &source).await?;

        sqlx::query("UPDATE geo_sources SET last_updated = ? WHERE id = ?")
            .bind(Utc::now())
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn sync_geo_sources(&self) -> anyhow::Result<()> {
        let sources = self.list_geo_sources().await?;
        let client = reqwest::Client::new();

        for source in sources {
            let now = Utc::now();
            let should_update = match source.last_updated {
                None => true,
                Some(last) => (now - last).num_seconds() >= source.update_interval_seconds,
            };

            if should_update {
                match self.fetch_and_cache_geo(&client, &source).await {
                    Ok(_) => {
                        sqlx::query("UPDATE geo_sources SET last_updated = ? WHERE id = ?")
                            .bind(now)
                            .bind(source.id)
                            .execute(&self.pool)
                            .await?;
                    }
                    Err(e) => {
                        error!("Failed to sync geo source {}: {}", source.url, e);
                    }
                }
            }
        }
        Ok(())
    }

    async fn fetch_and_cache_geo(
        &self,
        client: &reqwest::Client,
        source: &GeoSource,
    ) -> anyhow::Result<()> {
        let source_id = source.id.expect("Source must have an ID");
        info!("Syncing geo source {} ({})", source_id, source.url);
        let start = Instant::now();
        let response = client.get(&source.url).send().await?.bytes().await?;

        let mut tx = self.pool.begin().await?;

        if source.r#type == "geosite" {
            let list = geosite_rs::decode_geosite(&response)?;
            sqlx::query("DELETE FROM geosite_data WHERE source_id = ?")
                .bind(source_id)
                .execute(&mut *tx)
                .await?;

            let mut all_entries = Vec::new();
            for entry in list.entry {
                for domain in entry.domain {
                    all_entries.push((entry.country_code.clone(), domain.value, domain.r#type));
                }
            }

            let mut count = 0;
            for chunk in all_entries.chunks(1000) {
                let mut qb = sqlx::QueryBuilder::new(
                    "INSERT INTO geosite_data (source_id, category, domain, type) ",
                );
                qb.push_values(chunk, |mut b, (category, domain, r#type)| {
                    b.push_bind(source_id)
                        .push_bind(category)
                        .push_bind(domain)
                        .push_bind(r#type);
                });
                qb.build().execute(&mut *tx).await?;
                count += chunk.len();
            }
            info!(
                "Successfully synced geosite source {} in {:?}: {} entries",
                source_id,
                start.elapsed(),
                count
            );
        } else if source.r#type == "geoip" {
            let list = geosite_rs::decode_geoip(&response)?;
            sqlx::query("DELETE FROM geoip_data WHERE source_id = ?")
                .bind(source_id)
                .execute(&mut *tx)
                .await?;

            let mut all_entries = Vec::new();
            for entry in list.entry {
                for cidr in entry.cidr {
                    let ip = if cidr.ip.len() == 4 {
                        std::net::Ipv4Addr::from([cidr.ip[0], cidr.ip[1], cidr.ip[2], cidr.ip[3]])
                            .to_string()
                    } else if cidr.ip.len() == 16 {
                        let mut arr = [0u8; 16];
                        arr.copy_from_slice(&cidr.ip);
                        std::net::Ipv6Addr::from(arr).to_string()
                    } else {
                        continue;
                    };
                    let subnet = format!("{}/{}", ip, cidr.prefix);
                    all_entries.push((entry.country_code.clone(), subnet));
                }
            }

            let mut count = 0;
            for chunk in all_entries.chunks(1000) {
                let mut qb = sqlx::QueryBuilder::new(
                    "INSERT INTO geoip_data (source_id, category, subnet) ",
                );
                qb.push_values(chunk, |mut b, (category, subnet)| {
                    b.push_bind(source_id).push_bind(category).push_bind(subnet);
                });
                qb.build().execute(&mut *tx).await?;
                count += chunk.len();
            }
            info!(
                "Successfully synced geoip source {} in {:?}: {} entries",
                source_id,
                start.elapsed(),
                count
            );
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn list_geosite_categories(&self) -> anyhow::Result<Vec<String>> {
        let rows = sqlx::query("SELECT DISTINCT category FROM geosite_data ORDER BY category ASC")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| r.get(0)).collect())
    }

    pub async fn list_geoip_categories(&self) -> anyhow::Result<Vec<String>> {
        let rows = sqlx::query("SELECT DISTINCT category FROM geoip_data ORDER BY category ASC")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| r.get(0)).collect())
    }
}

#[async_trait]
impl DomainController for SqliteController {
    async fn should_intercept(&self, domain: &str) -> Option<Intercept> {
        let domain = domain.trim_end_matches('.');
        let mut check_domains = vec![domain.to_string()];

        let parts: Vec<&str> = domain.split('.').collect();
        for i in 1..parts.len() {
            check_domains.push(parts[i..].join("."));
        }

        // Stricter rules (more specific domains) should be matched first.
        // specificity = length of the matching domain.
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT rowid AS id, domain, include_subdomains, interface
             FROM domain_rules
             WHERE domain IN (",
        );
        let mut separated = qb.separated(", ");
        for d in &check_domains {
            separated.push_bind(d);
        }
        separated.push_unseparated(") ORDER BY length(domain) DESC");

        let rules_result = qb.build().fetch_all(&self.pool).await;
        if let Ok(rows) = rules_result {
            for row in rows {
                let rule_id: i64 = row.get(0);
                let rule_domain: String = row.get(1);
                let include_subdomains: bool = row.get(2);
                let interface: Option<String> = row.get(3);
                if rule_domain == domain || include_subdomains {
                    let interface = interface.unwrap_or_else(|| "default".to_string());
                    metrics::counter!("domain_hits", 
                        "subdomain" => (rule_domain != domain).to_string(), 
                        "domain" => rule_domain, 
                        "interface" => interface.to_string())
                    .increment(1);
                    return Some(Intercept {
                        interface,
                        policy_id: PolicyId::DomainRule(rule_id),
                    });
                }
            }
        } else if let Err(e) = rules_result {
            error!("Error querying domain_rules: {}", e);
        }

        // 2. Check list domains and geosite lists (Combined and ordered by list priority)
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT list_id, domain, include_subdomains, interface, priority, hit_type, category FROM (
                SELECT 
                    list_domains.list_id, 
                    list_domains.domain, 
                    domain_lists.include_subdomains, 
                    domain_lists.interface, 
                    domain_lists.priority,
                    -1 as hit_type,
                    '' as category,
                    domain_lists.id as sorting_id
                FROM list_domains 
                JOIN domain_lists ON list_domains.list_id = domain_lists.id 
                WHERE list_domains.domain IN ("
        );
        let mut separated = qb.separated(", ");
        for d in &check_domains {
            separated.push_bind(d);
        }
        separated.push_unseparated(
            ")
                UNION ALL
                SELECT 
                    domain_lists.id as list_id, 
                    geosite_data.domain, 
                    1 as include_subdomains,
                    domain_lists.interface, 
                    domain_lists.priority,
                    geosite_data.type as hit_type,
                    geosite_data.category,
                    domain_lists.id as sorting_id
                FROM geosite_data 
                JOIN domain_lists ON domain_lists.url = 'geosite://' || geosite_data.category 
                WHERE geosite_data.domain IN (",
        );
        let mut separated = qb.separated(", ");
        for d in &check_domains {
            separated.push_bind(d);
        }
        separated.push_unseparated(
            ")
            ) ORDER BY priority DESC, length(domain) DESC, sorting_id ASC",
        );

        let result = qb.build().fetch_all(&self.pool).await;
        if let Ok(rows) = result {
            for row in rows {
                let list_id: i64 = row.get(0);
                let hit_domain: String = row.get(1);
                let include_subdomains: bool = row.get(2);
                let interface: Option<String> = row.get(3);
                let _priority: i64 = row.get(4);
                let hit_type: i32 = row.get(5);
                let category: String = row.get(6);

                let matches = if hit_type == -1 {
                    // Regular list domain
                    hit_domain == domain || include_subdomains
                } else {
                    // Geosite entry: Plain = 0, Regex = 1, Domain = 2, Full = 3
                    if hit_type == 3 {
                        // Full
                        hit_domain == domain
                    } else if hit_type == 2 {
                        // Domain (suffix)
                        true // Since it's in check_domains, it's either the domain itself or a parent.
                    } else {
                        false
                    }
                };

                if matches {
                    let interface = interface.unwrap_or_else(|| "default".to_string());
                    if hit_type == -1 {
                        metrics::counter!("list_hits", 
                            "list_id" => list_id.to_string(), 
                            "subdomain" => (hit_domain != domain).to_string(), 
                            "interface" => interface.to_string())
                        .increment(1);
                    } else {
                        metrics::counter!("geosite_hits", 
                            "list_id" => list_id.to_string(), 
                            "category" => category, 
                            "interface" => interface.to_string())
                        .increment(1);
                    }
                    return Some(Intercept {
                        interface,
                        policy_id: PolicyId::DomainList(list_id),
                    });
                }
            }
        } else if let Err(e) = result {
            error!("Error querying combined list domains: {}", e);
        }

        None
    }
}
