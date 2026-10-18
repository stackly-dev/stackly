-- Test migration 2 that fails after its first statement succeeded.

ALTER TABLE runtimes ADD COLUMN notes TEXT;

INSERT INTO table_that_does_not_exist (id) VALUES (1);
