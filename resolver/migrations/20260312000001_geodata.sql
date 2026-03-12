-- Migration: 20260312000001_geodata.sql
CREATE TABLE IF NOT EXISTS geo_sources (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    url TEXT NOT NULL UNIQUE,
    type TEXT NOT NULL, -- 'geosite' or 'geoip'
    update_interval_seconds INTEGER NOT NULL,
    last_updated DATETIME
);

CREATE TABLE IF NOT EXISTS geosite_data (
    source_id INTEGER NOT NULL,
    category TEXT NOT NULL,
    domain TEXT NOT NULL,
    type INTEGER NOT NULL, -- matching type from geosite-rs
    FOREIGN KEY(source_id) REFERENCES geo_sources(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_geosite_data_category ON geosite_data(category);
CREATE INDEX IF NOT EXISTS idx_geosite_data_domain ON geosite_data(domain);

CREATE TABLE IF NOT EXISTS geoip_data (
    source_id INTEGER NOT NULL,
    category TEXT NOT NULL,
    subnet TEXT NOT NULL,
    FOREIGN KEY(source_id) REFERENCES geo_sources(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_geoip_data_category ON geoip_data(category);
CREATE INDEX IF NOT EXISTS idx_geoip_data_subnet ON geoip_data(subnet);
