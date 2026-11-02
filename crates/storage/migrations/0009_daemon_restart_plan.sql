-- Migration 9: daemon restart plans (spec 0030 "Model data", build step 3).
--
-- A plan records which instances a planned daemon restart keeps running
-- and which it stops and starts again, so the next daemon can finish it
-- after the old one exited or crashed. The registry holds at most one open
-- plan (stopping, handed_over, restoring), through a partial unique index.
-- instance_id has no foreign key on purpose: removing an instance is never
-- held up by a plan, and the row stays for its skipped outcome. A plan goes
-- with its operation when the operation is pruned, and the prune keeps the
-- newest final plans (RetentionPolicy.restart_plans). A plan never stores
-- the desired state of an instance.

CREATE TABLE daemon_restart_plan (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*' AND substr(id,9,1) = '-' AND substr(id,14,1) = '-' AND substr(id,19,1) = '-' AND substr(id,24,1) = '-' AND substr(id,15,1) = '7'),
    operation_id TEXT NOT NULL CHECK (length(operation_id) = 36 AND operation_id NOT GLOB '*[^0-9a-f-]*' AND substr(operation_id,9,1) = '-' AND substr(operation_id,14,1) = '-' AND substr(operation_id,19,1) = '-' AND substr(operation_id,24,1) = '-' AND substr(operation_id,15,1) = '7') REFERENCES operations (id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (state IN ('stopping', 'handed_over', 'restoring', 'finished', 'aborted', 'expired')),
    digest TEXT NOT NULL CHECK (length(digest) = 64 AND digest NOT GLOB '*[^0-9a-f]*'),
    boot_id TEXT NOT NULL CHECK (length(boot_id) = 36 AND boot_id NOT GLOB '*[^0-9a-f-]*' AND substr(boot_id,9,1) = '-' AND substr(boot_id,14,1) = '-' AND substr(boot_id,19,1) = '-' AND substr(boot_id,24,1) = '-'),
    server_instance_id_before TEXT NOT NULL CHECK (length(server_instance_id_before) = 36 AND server_instance_id_before NOT GLOB '*[^0-9a-f-]*' AND substr(server_instance_id_before,9,1) = '-' AND substr(server_instance_id_before,14,1) = '-' AND substr(server_instance_id_before,19,1) = '-' AND substr(server_instance_id_before,24,1) = '-'),
    server_instance_id_after TEXT CHECK (length(server_instance_id_after) = 36 AND server_instance_id_after NOT GLOB '*[^0-9a-f-]*' AND substr(server_instance_id_after,9,1) = '-' AND substr(server_instance_id_after,14,1) = '-' AND substr(server_instance_id_after,19,1) = '-' AND substr(server_instance_id_after,24,1) = '-'),
    created_at TEXT NOT NULL CHECK (length(created_at) = 30 AND created_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    handed_over_at TEXT CHECK (length(handed_over_at) = 30 AND handed_over_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    reconciled_at TEXT CHECK (length(reconciled_at) = 30 AND reconciled_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    finished_at TEXT CHECK (length(finished_at) = 30 AND finished_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    close_reason TEXT CHECK (close_reason IN ('stop_timeout', 'boot_changed')),
    CHECK ((state IN ('stopping', 'handed_over', 'restoring')) = (finished_at IS NULL)),
    CHECK ((state IN ('aborted', 'expired')) = (close_reason IS NOT NULL))
) STRICT;

CREATE UNIQUE INDEX daemon_restart_plan_open ON daemon_restart_plan ((1)) WHERE state IN ('stopping', 'handed_over', 'restoring');
CREATE INDEX daemon_restart_plan_created ON daemon_restart_plan (created_at, id);
CREATE INDEX daemon_restart_plan_operation ON daemon_restart_plan (operation_id);

CREATE TABLE daemon_restart_plan_instance (
    plan_id TEXT NOT NULL REFERENCES daemon_restart_plan (id) ON DELETE CASCADE,
    instance_id TEXT NOT NULL CHECK (length(instance_id) = 36 AND instance_id NOT GLOB '*[^0-9a-f-]*' AND substr(instance_id,9,1) = '-' AND substr(instance_id,14,1) = '-' AND substr(instance_id,19,1) = '-' AND substr(instance_id,24,1) = '-' AND substr(instance_id,15,1) = '7'),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 63 AND name GLOB '[a-z]*' AND name NOT GLOB '*[^a-z0-9-]*' AND name NOT GLOB '*--*' AND name NOT GLOB '*-'),
    decision TEXT NOT NULL CHECK (decision IN ('keep', 'stop_restore')),
    reason TEXT NOT NULL CHECK (reason IN ('handover_proven', 'handover_unproven', 'respawn_pending', 'identity_unproven', 'supervisor_unavailable')),
    stopped_at TEXT CHECK (length(stopped_at) = 30 AND stopped_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    ready_at TEXT CHECK (length(ready_at) = 30 AND ready_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    outcome TEXT CHECK (outcome IN ('adopted', 'restored', 'failed', 'skipped')),
    failure_code TEXT CHECK (length(failure_code) BETWEEN 1 AND 64 AND failure_code GLOB '[a-z]*' AND failure_code NOT GLOB '*[^a-z0-9_]*'),
    PRIMARY KEY (plan_id, instance_id),
    CHECK (decision = 'stop_restore' OR stopped_at IS NULL),
    CHECK (failure_code IS NULL OR outcome = 'failed')
) STRICT;
