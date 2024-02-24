-- Add migration script here
COMMIT;

PRAGMA foreign_keys = OFF;

BEGIN TRANSACTION;
-- Step 1: Add the new column 'category' with a default value

ALTER TABLE expenses
ADD COLUMN transaction_at TEXT;

-- Step 2: Update the existing rows 
UPDATE expenses
SET transaction_at = created_at;

-- Step 3: Create a new table with the desired schema
CREATE TABLE new_expenses (
  id TEXT PRIMARY KEY NOT NULL,
  title TEXT NOT NULL,
  created_at TEXT NOT NULL,
  created_by TEXT NOT NULL,
  group_id TEXT NOT NULL,
  currency_id TEXT NOT NULL,
  amount INTEGER NOT NULL,
  category TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  transaction_at TEXT NOT NULL,
  image_id TEXT,
  note TEXT,

  CONSTRAINT fk_currency
    FOREIGN KEY(currency_id)
    REFERENCES currency(id),
  CONSTRAINT fk_user
    FOREIGN KEY(created_by) 
    REFERENCES users(id),
  CONSTRAINT fk_group
    FOREIGN KEY(group_id) 
    REFERENCES groups(id)
);

-- Step 4: Copy data from the old table to the new table
INSERT INTO new_expenses SELECT id, title, created_at, created_by, group_id, currency_id, amount, category, updated_at, transaction_at, image_id, note FROM expenses;

-- Step 5: Rename the new table to the original table name
DROP TABLE expenses;
ALTER TABLE new_expenses RENAME TO expenses;

COMMIT;

PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;

CREATE INDEX idx_expense_transaction_at ON expenses (transaction_at);
