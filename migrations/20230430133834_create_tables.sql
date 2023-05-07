-- Add migration script here
CREATE TABLE IF NOT EXISTS users (
  id TEXT NOT NULL PRIMARY KEY ,
  name TEXT NOT NULL,
  notification_token TEXT,
  phone TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS groups (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  creator_id TEXT NOT NULL,
  created_at TEXT NOT NULL,

  CONSTRAINT fk_creator
    FOREIGN KEY(creator_id) 
	  REFERENCES users(id) 
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

  amount INTEGER NOT NULL, 

  CONSTRAINT fk_user
    FOREIGN KEY(created_by) 
	  REFERENCES users(id),

  CONSTRAINT fk_group
    FOREIGN KEY(group_id) 
	  REFERENCES groups(id)
);

CREATE TABLE IF NOT EXISTS split_transactions (
  id TEXT PRIMARY KEY NOT NULL,
  expense_id TEXT NOT NULL,
  amount INTEGER NOT NULL,
  from_user TEXT NOT NULL,
  to_user TEXT NOT NULL,
  amount_settled INTEGER NOT NULL,

  CONSTRAINT fk_from_user
    FOREIGN KEY(from_user) 
	  REFERENCES users(id),

  CONSTRAINT fk_to_user
    FOREIGN KEY(to_user) 
	  REFERENCES users(id),
 
  CONSTRAINT fk_expense
    FOREIGN KEY(expense_id) 
	  REFERENCES expenses(id)
)