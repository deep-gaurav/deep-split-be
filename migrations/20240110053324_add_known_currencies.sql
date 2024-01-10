-- Add migration script here
INSERT into currency(id,display_name,symbol,rate) VALUES (
  'USD',
  'United States Dollar',
  '$',
  1
);

INSERT into currency(id,display_name,symbol,rate) VALUES (
  'INR',
  'Indian Rupee',
  '₹',
  83.18
);