-- Migration 4: the items and the business rule of a stored operation
-- error (spec 0007 "Model data", build step 16, decision 28).
--
-- Two ALTER TABLE ... ADD COLUMN on operation_journal, no table rebuild.
-- Rows written by earlier builds get '[]' and NULL, which is correct
-- because those builds never stored items or a rule. SQLite tests a
-- column CHECK that names another column against the rows that exist
-- when the column is added, so both new checks hold for the old rows.
--
-- failure_items: a compact JSON array of {"kind": .., "fields":
-- [{"key": .., "value": ..}]}, in order, at most 65536 bytes (the
-- engine keeps at most 100 items inside that size). It stays '[]' on a
-- row without an error.
--
-- failure_rule: a PRD business rule ID, BR-NNN. The domain parses it into
-- a Rule on read, so an ID it does not know is a corrupt row. It stays
-- NULL on a row without an error.

ALTER TABLE operation_journal ADD COLUMN failure_items TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(failure_items) AND json_type(failure_items) = 'array' AND length(failure_items) <= 65536 AND (failure_items = '[]' OR failure_error_code IS NOT NULL));

ALTER TABLE operation_journal ADD COLUMN failure_rule TEXT CHECK (failure_rule IS NULL OR (failure_rule GLOB 'BR-[0-9][0-9][0-9]' AND failure_error_code IS NOT NULL));
