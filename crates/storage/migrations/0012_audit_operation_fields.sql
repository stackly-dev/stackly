-- Migration 12: the operation fields of an audit row and how long audit rows
-- are kept (spec 0035 "Model data", build step 1).
--
-- Four ALTER TABLE ... ADD COLUMN on audit_events and one on
-- daemon_settings, no table rebuild. Rows written by earlier builds read
-- NULL in every new audit column, which is correct because those builds
-- never stored an operation's kind, instance, revision, or client. The
-- trigger audit_events_append_only still refuses every UPDATE. The
-- settings row reads 30 days, the default the daemon used before.
--
-- instance_id: the instance a row is about, with no foreign key, so the
-- history stays after the instance is removed.
--
-- operation_kind: the OperationKind code of an operation.* row.
--
-- config_revision: the number of the instance's config revision (not the
-- row ID). SQLite tests a column CHECK that names another column against
-- the rows that exist when the column is added, so the check is written
-- to hold for NULL.
--
-- client: the client kind from the handshake, such as cli or gui.

ALTER TABLE audit_events ADD COLUMN instance_id TEXT CHECK (instance_id IS NULL OR (length(instance_id) = 36 AND instance_id NOT GLOB '*[^0-9a-f-]*' AND substr(instance_id,9,1) = '-' AND substr(instance_id,14,1) = '-' AND substr(instance_id,19,1) = '-' AND substr(instance_id,24,1) = '-' AND substr(instance_id,15,1) = '7'));

ALTER TABLE audit_events ADD COLUMN operation_kind TEXT CHECK (operation_kind IS NULL OR (length(operation_kind) BETWEEN 1 AND 64 AND operation_kind GLOB '[a-z]*' AND operation_kind NOT GLOB '*[^a-z0-9_]*'));

ALTER TABLE audit_events ADD COLUMN config_revision INTEGER CHECK (config_revision IS NULL OR (config_revision >= 1 AND instance_id IS NOT NULL));

ALTER TABLE audit_events ADD COLUMN client TEXT CHECK (client IS NULL OR (length(client) BETWEEN 1 AND 64 AND client GLOB '[a-z]*' AND client NOT GLOB '*[^a-z0-9_]*'));

CREATE INDEX audit_events_instance ON audit_events (instance_id, occurred_at, id) WHERE instance_id IS NOT NULL;

ALTER TABLE daemon_settings ADD COLUMN audit_retention_days INTEGER NOT NULL DEFAULT 30 CHECK (audit_retention_days IN (7, 30, 90));
