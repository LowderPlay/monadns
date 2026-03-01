CREATE TABLE IF NOT EXISTS domain_rules (
    domain TEXT PRIMARY KEY,
    include_subdomains BOOLEAN NOT NULL DEFAULT 0
);
