-- Migration 2: the operation journal of spec 0007 ("Model data").
--
-- An idempotency key now binds one operation while its row exists: the
-- migration clears every key a newer operation also uses (by created_at,
-- then id), and the plain index on the key becomes a unique partial
-- index. Then it adds the four journal tables, all deleted with their
-- operation, and backfills one journal row per existing operation: a
-- background trigger, no request, input '{}', and resumable from its kind
-- (the resume table of spec 0007).
--
-- Every CHECK for a column kind is the exact expression from the "Aturan
-- kolom bersama" table of spec 0005, plus the errcode kind of spec 0007,
-- with the column name filled in.

UPDATE operations SET idempotency_key = NULL
WHERE idempotency_key IS NOT NULL AND EXISTS (
    SELECT 1 FROM operations AS newer
    WHERE newer.idempotency_key = operations.idempotency_key
    AND (newer.created_at, newer.id) > (operations.created_at, operations.id)
);

DROP INDEX operations_idempotency_key;
CREATE UNIQUE INDEX operations_idempotency_key ON operations (idempotency_key) WHERE idempotency_key IS NOT NULL;

CREATE TABLE operation_journal (
    operation_id TEXT PRIMARY KEY NOT NULL CHECK (length(operation_id) = 36 AND operation_id NOT GLOB '*[^0-9a-f-]*' AND substr(operation_id,9,1) = '-' AND substr(operation_id,14,1) = '-' AND substr(operation_id,19,1) = '-' AND substr(operation_id,24,1) = '-' AND substr(operation_id,15,1) = '7') REFERENCES operations (id) ON DELETE CASCADE,
    method TEXT CHECK (length(method) BETWEEN 3 AND 64 AND method GLOB '[a-z]*.[a-z]*' AND method NOT GLOB '*[^a-z0-9_.]*' AND method NOT GLOB '*.*.*'),
    request_fingerprint TEXT CHECK (length(request_fingerprint) = 64 AND request_fingerprint NOT GLOB '*[^0-9a-f]*'),
    trigger_origin TEXT NOT NULL CHECK (trigger_origin IN ('user', 'background')),
    trigger_client TEXT CHECK (length(trigger_client) BETWEEN 1 AND 64 AND trigger_client GLOB '[a-z]*' AND trigger_client NOT GLOB '*[^a-z0-9_]*'),
    trigger_interactive INTEGER NOT NULL CHECK (trigger_interactive IN (0, 1)),
    resumable INTEGER NOT NULL CHECK (resumable IN (0, 1)),
    input TEXT NOT NULL CHECK (json_valid(input) AND json_type(input) = 'object' AND length(input) <= 16384),
    expected_revision INTEGER CHECK (expected_revision >= 1),
    waiting_on TEXT CHECK (length(waiting_on) = 36 AND waiting_on NOT GLOB '*[^0-9a-f-]*' AND substr(waiting_on,9,1) = '-' AND substr(waiting_on,14,1) = '-' AND substr(waiting_on,19,1) = '-' AND substr(waiting_on,24,1) = '-' AND substr(waiting_on,15,1) = '7'),
    point_of_no_return_at TEXT CHECK (length(point_of_no_return_at) = 30 AND point_of_no_return_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    cancel_requested_at TEXT CHECK (length(cancel_requested_at) = 30 AND cancel_requested_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    compensation_started_at TEXT CHECK (length(compensation_started_at) = 30 AND compensation_started_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    recovery_cause TEXT CHECK (length(recovery_cause) BETWEEN 1 AND 64 AND recovery_cause GLOB '[a-z]*' AND recovery_cause NOT GLOB '*[^a-z0-9_]*'),
    failure_error_code TEXT CHECK (length(failure_error_code) BETWEEN 3 AND 96 AND failure_error_code GLOB '[a-z]*.[a-z]*' AND failure_error_code NOT GLOB '*[^a-z0-9_.]*' AND failure_error_code NOT GLOB '*.*.*'),
    failure_message TEXT CHECK (length(failure_message) BETWEEN 1 AND 200),
    failure_details TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(failure_details) AND json_type(failure_details) = 'array'),
    result TEXT CHECK (json_valid(result) AND json_type(result) = 'object' AND length(result) <= 16384),
    CHECK ((method IS NULL) = (request_fingerprint IS NULL)),
    CHECK ((failure_error_code IS NULL) = (failure_message IS NULL))
) STRICT;

CREATE TABLE operation_steps (
    operation_id TEXT NOT NULL CHECK (length(operation_id) = 36 AND operation_id NOT GLOB '*[^0-9a-f-]*' AND substr(operation_id,9,1) = '-' AND substr(operation_id,14,1) = '-' AND substr(operation_id,19,1) = '-' AND substr(operation_id,24,1) = '-' AND substr(operation_id,15,1) = '7') REFERENCES operations (id) ON DELETE CASCADE,
    seq INTEGER NOT NULL CHECK (seq BETWEEN 1 AND 256),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 64 AND name GLOB '[a-z]*' AND name NOT GLOB '*[^a-z0-9_]*' AND length(name) <= 32),
    repeatable INTEGER NOT NULL CHECK (repeatable IN (0, 1)),
    reversible INTEGER NOT NULL CHECK (reversible IN (0, 1)),
    state TEXT NOT NULL CHECK (state IN ('running', 'done', 'failed', 'interrupted')),
    attempts INTEGER NOT NULL CHECK (attempts >= 1),
    checkpoint TEXT CHECK (json_valid(checkpoint) AND length(checkpoint) <= 4096),
    error_code TEXT CHECK (length(error_code) BETWEEN 3 AND 96 AND error_code GLOB '[a-z]*.[a-z]*' AND error_code NOT GLOB '*[^a-z0-9_.]*' AND error_code NOT GLOB '*.*.*'),
    started_at TEXT NOT NULL CHECK (length(started_at) = 30 AND started_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    finished_at TEXT CHECK (length(finished_at) = 30 AND finished_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    PRIMARY KEY (operation_id, seq),
    UNIQUE (operation_id, name),
    CHECK ((state = 'running') = (finished_at IS NULL)),
    CHECK (error_code IS NULL OR state = 'failed'),
    CHECK (repeatable = 1 OR attempts = 1)
) STRICT;

CREATE TABLE operation_created (
    operation_id TEXT NOT NULL CHECK (length(operation_id) = 36 AND operation_id NOT GLOB '*[^0-9a-f-]*' AND substr(operation_id,9,1) = '-' AND substr(operation_id,14,1) = '-' AND substr(operation_id,19,1) = '-' AND substr(operation_id,24,1) = '-' AND substr(operation_id,15,1) = '7') REFERENCES operations (id) ON DELETE CASCADE,
    seq INTEGER NOT NULL CHECK (seq BETWEEN 1 AND 1024),
    step_seq INTEGER NOT NULL,
    item_kind TEXT NOT NULL CHECK (length(item_kind) BETWEEN 1 AND 64 AND item_kind GLOB '[a-z]*' AND item_kind NOT GLOB '*[^a-z0-9_]*'),
    item_ref TEXT NOT NULL CHECK (length(item_ref) BETWEEN 1 AND 512 AND item_ref NOT GLOB '*[^!-~]*'),
    payload TEXT CHECK (json_valid(payload) AND length(payload) <= 4096),
    state TEXT NOT NULL CHECK (state IN ('intended', 'created', 'compensated', 'kept', 'compensation_failed', 'not_owned')),
    detail TEXT CHECK (length(detail) BETWEEN 1 AND 64 AND detail GLOB '[a-z]*' AND detail NOT GLOB '*[^a-z0-9_]*'),
    recorded_at TEXT NOT NULL CHECK (length(recorded_at) = 30 AND recorded_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    settled_at TEXT CHECK (length(settled_at) = 30 AND settled_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    PRIMARY KEY (operation_id, seq),
    UNIQUE (operation_id, item_kind, item_ref),
    FOREIGN KEY (operation_id, step_seq) REFERENCES operation_steps (operation_id, seq) ON DELETE CASCADE,
    CHECK ((state IN ('intended', 'created')) = (settled_at IS NULL)),
    CHECK (payload IS NULL OR item_kind = 'custom')
) STRICT;

CREATE INDEX operation_created_step ON operation_created (operation_id, step_seq);

CREATE TABLE operation_locks (
    operation_id TEXT NOT NULL CHECK (length(operation_id) = 36 AND operation_id NOT GLOB '*[^0-9a-f-]*' AND substr(operation_id,9,1) = '-' AND substr(operation_id,14,1) = '-' AND substr(operation_id,19,1) = '-' AND substr(operation_id,24,1) = '-' AND substr(operation_id,15,1) = '7') REFERENCES operations (id) ON DELETE CASCADE,
    lock_key TEXT NOT NULL CHECK (length(lock_key) BETWEEN 3 AND 1100 AND lock_key GLOB '[a-z]*:?*'),
    PRIMARY KEY (operation_id, lock_key)
) STRICT;

CREATE INDEX operation_locks_key ON operation_locks (lock_key);

INSERT INTO operation_journal (operation_id, trigger_origin, trigger_interactive, resumable, input)
SELECT id, 'background', 0,
    CASE WHEN kind IN ('install_runtime', 'start_instance', 'stop_instance', 'restart_instance', 'reload_instance', 'environment_up', 'environment_down', 'backup_instance') THEN 1 ELSE 0 END,
    '{}'
FROM operations;
