CREATE TABLE IF NOT EXISTS domain_lists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    url TEXT NOT NULL UNIQUE,
    update_interval_seconds INTEGER NOT NULL,
    include_subdomains BOOLEAN NOT NULL DEFAULT 0,
    last_updated DATETIME
);

CREATE TABLE IF NOT EXISTS list_domains (
    domain TEXT NOT NULL,
    list_id INTEGER NOT NULL,
    FOREIGN KEY(list_id) REFERENCES domain_lists(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_list_domains_domain ON list_domains(domain);
