-- Migration 15: environment up (spec 0046 "Model data", build steps 1 and
-- 2).
--
-- environments.last_up holds the report of the last Up of the environment:
-- a JSON object {operationId, state, startedAt, finishedAt?, services:
-- [{alias, phase, stage?, code?, dependency?, reason?, operationId?,
-- references?}]}, or NULL before the first Up.
--
-- operation_journal.parent_id names the coordinator that started a child
-- operation. It has no foreign key, so pruning a coordinator never holds
-- back or deletes its children, and it never changes once written.

ALTER TABLE environments ADD COLUMN last_up TEXT CHECK (last_up IS NULL OR (json_valid(last_up) AND json_type(last_up) = 'object'));

ALTER TABLE operation_journal ADD COLUMN parent_id TEXT CHECK (parent_id IS NULL OR (length(parent_id) = 36 AND parent_id NOT GLOB '*[^0-9a-f-]*' AND substr(parent_id,9,1) = '-' AND substr(parent_id,14,1) = '-' AND substr(parent_id,19,1) = '-' AND substr(parent_id,24,1) = '-' AND substr(parent_id,15,1) = '7'));
