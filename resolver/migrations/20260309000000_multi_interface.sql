-- Add interface column to domain_rules and domain_lists
ALTER TABLE domain_rules ADD COLUMN interface TEXT;
ALTER TABLE domain_lists ADD COLUMN interface TEXT;
