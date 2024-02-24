COMMIT;

PRAGMA foreign_keys = OFF;

BEGIN TRANSACTION;
-- Step 1: Add the new column 'category' with a default value

ALTER TABLE expenses
ADD COLUMN updated_at TEXT;

ALTER TABLE split_transactions
ADD COLUMN updated_at TEXT;

-- Step 2: Update the existing rows 
UPDATE expenses
SET updated_at = created_at;

UPDATE split_transactions
SET updated_at = created_at;

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


CREATE TABLE IF NOT EXISTS new_split_transactions (
  id TEXT PRIMARY KEY NOT NULL,
  expense_id TEXT,
  amount INTEGER NOT NULL,
  currency_id TEXT NOT NULL,
  from_user TEXT NOT NULL,
  to_user TEXT NOT NULL,
  transaction_type TEXT NOT NULL,
  part_transaction TEXT,
  created_at TEXT NOT NULL,
  created_by TEXT NOT NULL,
  group_id TEXT NOT NULL,
  with_group_id TEXT,
  updated_at TEXT NOT NULL,
  image_id TEXT,
  note TEXT,
  
  CONSTRAINT fk_from_user
    FOREIGN KEY(from_user) 
	  REFERENCES users(id) DEFERRABLE INITIALLY DEFERRED,

  CONSTRAINT fk_currency
    FOREIGN KEY(currency_id)
    REFERENCES currency(id) DEFERRABLE INITIALLY DEFERRED,

  CONSTRAINT fk_to_user
    FOREIGN KEY(to_user) 
	  REFERENCES users(id) DEFERRABLE INITIALLY DEFERRED,
 
  CONSTRAINT fk_expense
    FOREIGN KEY(expense_id) 
	  REFERENCES expenses(id) DEFERRABLE INITIALLY DEFERRED,

  
  CONSTRAINT fk_creator
    FOREIGN KEY(created_by) 
	  REFERENCES users(id) DEFERRABLE INITIALLY DEFERRED,


  CONSTRAINT fk_group
    FOREIGN KEY(group_id) 
	  REFERENCES groups(id) DEFERRABLE INITIALLY DEFERRED,


  CONSTRAINT fk_with_group
    FOREIGN KEY(with_group_id) 
	  REFERENCES groups(id) DEFERRABLE INITIALLY DEFERRED
);

-- Step 4: Copy data from the old table to the new table
INSERT INTO new_expenses SELECT id, title, created_at, created_by, group_id, currency_id, amount, category, updated_at, image_id, note FROM expenses;
INSERT INTO new_split_transactions SELECT id, expense_id, amount, currency_id, from_user, to_user, transaction_type, part_transaction, created_at, created_by, group_id, with_group_id, updated_at, image_id, note FROM split_transactions;


-- Step 5: Rename the new table to the original table name
DROP TABLE expenses;
ALTER TABLE new_expenses RENAME TO expenses;

DROP TABLE split_transactions;
ALTER TABLE new_split_transactions RENAME TO split_transactions;

COMMIT;

PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;