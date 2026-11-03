-- Migration 11: daemon settings and storage locations (spec 0032 "Model data",
-- build step 2).
--
-- storage_locations holds the three places new instance data, logs, and
-- downloads go: one built in row per kind (fixed IDs, no path, they resolve
-- through the root set) and one row per folder the person chose. A row is
-- never changed or deleted. daemon_settings is the one row of settings the
-- daemon keeps for every client. instance_paths is rebuilt with location_id,
-- which points every data and log path of an existing instance at the built
-- in location of its kind, so each path resolves to the same folder as
-- before. A migration never reads the clock, so the built in rows and the
-- first settings row carry the Unix epoch as their time.

CREATE TABLE storage_locations (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*' AND substr(id,9,1) = '-' AND substr(id,14,1) = '-' AND substr(id,19,1) = '-' AND substr(id,24,1) = '-' AND substr(id,15,1) = '7'),
    kind TEXT NOT NULL CHECK (kind IN ('data', 'logs', 'cache')),
    path TEXT CHECK (length(path) BETWEEN 2 AND 4096 AND path GLOB '/*'),
    built_in INTEGER NOT NULL CHECK (built_in IN (0, 1)),
    created_at TEXT NOT NULL CHECK (length(created_at) = 30 AND created_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    CHECK ((path IS NULL) = (built_in = 1))
) STRICT;

CREATE UNIQUE INDEX storage_locations_built_in ON storage_locations (kind) WHERE built_in = 1;
CREATE UNIQUE INDEX storage_locations_path ON storage_locations (kind, path) WHERE built_in = 0;

CREATE TRIGGER storage_locations_no_update BEFORE UPDATE ON storage_locations
BEGIN
    SELECT RAISE(ABORT, 'a storage location is never changed');
END;

CREATE TRIGGER storage_locations_no_delete BEFORE DELETE ON storage_locations
BEGIN
    SELECT RAISE(ABORT, 'a storage location is never deleted');
END;

INSERT INTO storage_locations (id, kind, path, built_in, created_at) VALUES
    ('00000000-0000-7000-8000-000000000001', 'data', NULL, 1, '1970-01-01T00:00:00.000000000Z'),
    ('00000000-0000-7000-8000-000000000002', 'logs', NULL, 1, '1970-01-01T00:00:00.000000000Z'),
    ('00000000-0000-7000-8000-000000000003', 'cache', NULL, 1, '1970-01-01T00:00:00.000000000Z');

CREATE TABLE daemon_settings (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    default_startup TEXT NOT NULL CHECK (default_startup IN ('manual', 'at_login')),
    update_channel TEXT NOT NULL CHECK (update_channel IN ('stable', 'beta')),
    telemetry INTEGER NOT NULL CHECK (telemetry IN (0, 1)),
    telemetry_copy_version INTEGER NOT NULL CHECK (telemetry_copy_version >= 1),
    data_location_id TEXT NOT NULL REFERENCES storage_locations (id) ON DELETE RESTRICT,
    logs_location_id TEXT NOT NULL REFERENCES storage_locations (id) ON DELETE RESTRICT,
    cache_location_id TEXT NOT NULL REFERENCES storage_locations (id) ON DELETE RESTRICT,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    updated_at TEXT NOT NULL CHECK (length(updated_at) = 30 AND updated_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z')
) STRICT;

CREATE INDEX daemon_settings_data_location ON daemon_settings (data_location_id);
CREATE INDEX daemon_settings_logs_location ON daemon_settings (logs_location_id);
CREATE INDEX daemon_settings_cache_location ON daemon_settings (cache_location_id);

CREATE TRIGGER daemon_settings_location_kinds BEFORE UPDATE ON daemon_settings
WHEN (SELECT kind FROM storage_locations WHERE id = NEW.data_location_id) IS NOT 'data'
  OR (SELECT kind FROM storage_locations WHERE id = NEW.logs_location_id) IS NOT 'logs'
  OR (SELECT kind FROM storage_locations WHERE id = NEW.cache_location_id) IS NOT 'cache'
BEGIN
    SELECT RAISE(ABORT, 'a default location has another kind than its column');
END;

CREATE TRIGGER daemon_settings_no_delete BEFORE DELETE ON daemon_settings
BEGIN
    SELECT RAISE(ABORT, 'the daemon settings row is never deleted');
END;

INSERT INTO daemon_settings (id, default_startup, update_channel, telemetry, telemetry_copy_version, data_location_id, logs_location_id, cache_location_id, revision, updated_at) VALUES
    (1, 'manual', 'stable', 0, 1, '00000000-0000-7000-8000-000000000001', '00000000-0000-7000-8000-000000000002', '00000000-0000-7000-8000-000000000003', 1, '1970-01-01T00:00:00.000000000Z');

ALTER TABLE instance_paths RENAME TO instance_paths_before_11;

CREATE TABLE instance_paths (
    instance_id TEXT NOT NULL CHECK (length(instance_id) = 36 AND instance_id NOT GLOB '*[^0-9a-f-]*' AND substr(instance_id,9,1) = '-' AND substr(instance_id,14,1) = '-' AND substr(instance_id,19,1) = '-' AND substr(instance_id,24,1) = '-' AND substr(instance_id,15,1) = '7') REFERENCES instances (id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('root', 'data', 'config', 'config_history', 'temp', 'logs', 'run')),
    logical_root TEXT NOT NULL CHECK (logical_root IN ('data', 'logs', 'runtime')),
    relative_path TEXT NOT NULL CHECK (length(relative_path) BETWEEN 1 AND 1024 AND relative_path NOT GLOB '/*' AND relative_path NOT GLOB '*/' AND relative_path NOT GLOB '*//*' AND ('/' || relative_path || '/') NOT GLOB '*/./*' AND ('/' || relative_path || '/') NOT GLOB '*/../*'),
    managed INTEGER NOT NULL CHECK (managed IN (0, 1)),
    location_id TEXT REFERENCES storage_locations (id) ON DELETE RESTRICT,
    PRIMARY KEY (instance_id, kind),
    CHECK ((location_id IS NULL) = (kind = 'run'))
) STRICT, WITHOUT ROWID;

INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed, location_id)
SELECT instance_id, kind, logical_root, relative_path, managed,
       CASE logical_root
            WHEN 'data' THEN '00000000-0000-7000-8000-000000000001'
            WHEN 'logs' THEN '00000000-0000-7000-8000-000000000002'
            ELSE NULL
       END
FROM instance_paths_before_11;

-- The copy must hold every row: this insert fails, and with it the whole
-- migration, when the counts differ.
CREATE TABLE migration_11_check (ok INTEGER NOT NULL CHECK (ok = 1)) STRICT;
INSERT INTO migration_11_check (ok)
SELECT (SELECT count(*) FROM instance_paths) = (SELECT count(*) FROM instance_paths_before_11);
DROP TABLE migration_11_check;

DROP TABLE instance_paths_before_11;

CREATE INDEX instance_paths_location ON instance_paths (location_id);
CREATE UNIQUE INDEX instance_paths_location_path ON instance_paths (location_id, relative_path) WHERE location_id IS NOT NULL;
CREATE UNIQUE INDEX instance_paths_root_path ON instance_paths (logical_root, relative_path) WHERE location_id IS NULL;

CREATE TRIGGER instance_paths_location_kind BEFORE INSERT ON instance_paths
WHEN NEW.location_id IS NOT NULL
 AND EXISTS (SELECT 1 FROM storage_locations WHERE id = NEW.location_id)
 AND (SELECT kind FROM storage_locations WHERE id = NEW.location_id) IS NOT NEW.logical_root
BEGIN
    SELECT RAISE(ABORT, 'an instance path names a location of another kind than its root');
END;

CREATE TRIGGER instance_paths_location_kind_update BEFORE UPDATE OF location_id, logical_root ON instance_paths
WHEN NEW.location_id IS NOT NULL
 AND EXISTS (SELECT 1 FROM storage_locations WHERE id = NEW.location_id)
 AND (SELECT kind FROM storage_locations WHERE id = NEW.location_id) IS NOT NEW.logical_root
BEGIN
    SELECT RAISE(ABORT, 'an instance path names a location of another kind than its root');
END;
