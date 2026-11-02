-- Migration 10: daemon login sessions (spec 0031 "Model data", build step 3).
--
-- One row per login session the launchd daemon ran in, so a new session can
-- be told from a daemon that restarted in the same one. The registry holds
-- at most one open row, through a partial unique index, and the newest 50
-- rows (the prune is a write op, PruneSessions). id is the SessionId:
-- <boot UUID, lowercase>/<audit session, decimal>. The same migration adds
-- the unique rule that gives an instance at most one startup lease per
-- session that is not released; instance_demands.session_id has no foreign
-- key, so a pruned session never blocks a lease.

CREATE TABLE daemon_sessions (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 64 AND id NOT GLOB '*[^!-~]*'),
    boot_id TEXT NOT NULL CHECK (length(boot_id) = 36 AND boot_id NOT GLOB '*[^0-9a-f-]*' AND substr(boot_id,9,1) = '-' AND substr(boot_id,14,1) = '-' AND substr(boot_id,19,1) = '-' AND substr(boot_id,24,1) = '-'),
    audit_session INTEGER NOT NULL CHECK (audit_session BETWEEN 1 AND 4294967295),
    started_at TEXT NOT NULL CHECK (length(started_at) = 30 AND started_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    ended_at TEXT CHECK (length(ended_at) = 30 AND ended_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    end_reason TEXT CHECK (end_reason IN ('clean', 'interrupted', 'unknown')),
    CHECK ((ended_at IS NULL) = (end_reason IS NULL))
) STRICT;

CREATE UNIQUE INDEX daemon_sessions_open ON daemon_sessions ((1)) WHERE ended_at IS NULL;
CREATE INDEX daemon_sessions_started ON daemon_sessions (started_at, id);

CREATE UNIQUE INDEX instance_demands_startup_session ON instance_demands (instance_id, session_id) WHERE source_kind = 'startup' AND state IN ('active', 'suppressed');
