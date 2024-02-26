-- Add migration script here
COMMIT;

PRAGMA foreign_keys = OFF;

BEGIN TRANSACTION;

-- Create a new table with the UNIQUE constraint
CREATE TABLE IF NOT EXISTS payment_modes_new (
  id TEXT PRIMARY KEY NOT NULL,
  mode TEXT NOT NULL,
  user_id TEXT NOT NULL,
  value TEXT NOT NULL,

  CONSTRAINT fk_user
    FOREIGN KEY(user_id) 
    REFERENCES users(id),

  UNIQUE (mode, user_id, value)
);

-- Transfer data without duplicates
INSERT INTO payment_modes_new
SELECT *
FROM payment_modes;

-- Drop the old table and rename the new one
DROP TABLE payment_modes;
ALTER TABLE payment_modes_new RENAME TO payment_modes;

COMMIT;

PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;
