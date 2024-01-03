-- Add migration script here
CREATE INDEX idx_users_email ON users (email);

CREATE INDEX idx_group_membership_user_id ON group_memberships (user_id);
CREATE INDEX idx_group_membership_group_id ON group_memberships (group_id);

CREATE INDEX idx_expenses_created_by ON expenses (created_by);
CREATE INDEX idx_expenses_group_id ON expenses (group_id);


CREATE INDEX idx_split_transactions_group_id ON split_transactions (group_id);
CREATE INDEX idx_split_transactions_from_user ON split_transactions (from_user);
CREATE INDEX idx_split_transactions_to_user ON split_transactions (to_user);
CREATE INDEX idx_split_transactions_expense_id ON split_transactions (expense_id);
CREATE INDEX idx_split_transactions_created_at ON split_transactions (created_at);

CREATE INDEX idx_payment_modes_user_id ON payment_modes (user_id);