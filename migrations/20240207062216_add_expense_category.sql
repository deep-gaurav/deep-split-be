-- Step 1: Add the new column 'category' with a default value
ALTER TABLE expenses
ADD COLUMN category TEXT DEFAULT 'MISC';

-- Step 2: Update the existing rows to set the category to 'MISC'
UPDATE expenses
SET category = 'MISC'
WHERE category IS NULL;

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
INSERT INTO new_expenses SELECT * FROM expenses;

-- Step 5: Rename the new table to the original table name
DROP TABLE expenses;
ALTER TABLE new_expenses RENAME TO expenses;
