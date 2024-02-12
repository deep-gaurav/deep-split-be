ALTER TABLE expenses ADD COLUMN image_id TEXT;
ALTER TABLE expenses ADD COLUMN note TEXT;

ALTER TABLE split_transactions ADD COLUMN image_id TEXT;
ALTER TABLE split_transactions ADD COLUMN note TEXT;