-- Add migration script here

-- Step 1: Create a new temporary table
CREATE TABLE IF NOT EXISTS temp_groups (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT,
  creator_id TEXT NOT NULL,
  created_at TEXT NOT NULL,

  CONSTRAINT fk_creator
    FOREIGN KEY(creator_id) 
      REFERENCES users(id) 
);

-- Step 2: Copy data from the existing table to the new table
INSERT INTO temp_groups (id, name, creator_id, created_at)
    SELECT id, name, creator_id, created_at
    FROM groups;

-- Step 3: Drop the old table
DROP TABLE IF EXISTS groups;

-- Step 4: Rename the new table to the original table name
ALTER TABLE temp_groups RENAME TO groups;
