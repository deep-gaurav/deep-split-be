-- Add migration script here
INSERT into currency(id,display_name,symbol,rate,decimals) VALUES (
  'USD',
  'United States Dollar',
  '$',
  1,
  2
);

INSERT into currency(id,display_name,symbol,rate,decimals) VALUES (
  'INR',
  'Indian Rupee',
  '₹',
  83.18,
  2
);