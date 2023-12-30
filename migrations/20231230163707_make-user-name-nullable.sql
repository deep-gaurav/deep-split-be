-- Add migration script here
CREATE TABLE users_temp (
  id TEXT NOT NULL PRIMARY KEY,
  name TEXT,
  notification_token TEXT,
  phone TEXT UNIQUE,
  email TEXT UNIQUE,
  CONSTRAINT check_not_null_fields CHECK (phone IS NOT NULL OR email IS NOT NULL)
);

-- Copy data from the existing table to the temporary table
INSERT INTO users_temp (id, name, notification_token, phone, email)
SELECT id, name, notification_token, phone, email FROM users;

-- Drop the existing users table
DROP TABLE users;

-- Rename the temporary table to users
ALTER TABLE users_temp RENAME TO users;
