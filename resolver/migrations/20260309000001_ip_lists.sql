-- Migration: 20260309000001_ip_lists.sql
CREATE TABLE IF NOT EXISTS ip_rules (
    subnet TEXT NOT NULL PRIMARY KEY,
    interface TEXT
);

CREATE TABLE IF NOT EXISTS ip_lists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    url TEXT NOT NULL UNIQUE,
    update_interval_seconds INTEGER NOT NULL,
    last_updated DATETIME,
    interface TEXT
);

CREATE TABLE IF NOT EXISTS list_ips (
    subnet TEXT NOT NULL,
    list_id INTEGER NOT NULL,
    FOREIGN KEY(list_id) REFERENCES ip_lists(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_list_ips_subnet ON list_ips(subnet);
