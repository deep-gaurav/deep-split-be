-- Add migration script here
ALTER TABLE split_transactions
ADD COLUMN transaction_metadata TEXT;