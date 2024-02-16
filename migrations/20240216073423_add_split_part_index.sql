-- Add migration script here
CREATE INDEX idx_split_transactions_part_transaction ON split_transactions (part_transaction);
