-- Add migration script here
CREATE TABLE IF NOT EXISTS users (
  id TEXT NOT NULL PRIMARY KEY ,
  name TEXT,
  notification_token TEXT,
  phone TEXT UNIQUE,
  email TEXT UNIQUE,
  CONSTRAINT check_not_null_fields CHECK (phone IS NOT NULL OR email IS NOT NULL)
);

CREATE TABLE IF NOT EXISTS groups (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT,
  creator_id TEXT NOT NULL,
  created_at TEXT NOT NULL,

  CONSTRAINT fk_creator
    FOREIGN KEY(creator_id) 
	  REFERENCES users(id) 
);


CREATE TABLE IF NOT EXISTS currency (
  id TEXT PRIMARY KEY NOT NULL,
  display_name TEXT NOT NULL,
  symbol TEXT NOT NULL,
  rate REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS group_memberships (
  id TEXT PRIMARY KEY NOT NULL,
  user_id TEXT NOT NULL,
  group_id TEXT NOT NULL,

  CONSTRAINT fk_user
    FOREIGN KEY(user_id) 
	  REFERENCES users(id),

  CONSTRAINT fk_group
    FOREIGN KEY(group_id) 
	  REFERENCES groups(id),

  CONSTRAINT unq UNIQUE (user_id, group_id)
);

CREATE TABLE IF NOT EXISTS expenses (
  id TEXT PRIMARY KEY NOT NULL,
  title TEXT NOT NULL,

  created_at TEXT NOT NULL,
  created_by TEXT NOT NULL,

  group_id TEXT NOT NULL,

  currency_id TEXT NOT NULL,
  amount INTEGER NOT NULL, 


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

CREATE TABLE IF NOT EXISTS split_transactions (
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

CREATE TABLE IF NOT EXISTS payment_modes (
  id TEXT PRIMARY KEY NOT NULL,
  mode TEXT NOT NULL,
  user_id TEXT NOT NULL,
  value TEXT NOT NULL,

  CONSTRAINT fk_user
    FOREIGN KEY(user_id) 
	  REFERENCES users(id)
);
