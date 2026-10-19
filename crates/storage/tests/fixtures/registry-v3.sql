-- Frozen registry fixture, schema version 3.
-- Written by write_registry_fixture; never edit it by hand.
PRAGMA application_id = 1398033241;
PRAGMA user_version = 3;
BEGIN;
CREATE TABLE artifacts (
    pk INTEGER PRIMARY KEY NOT NULL,
    sha256 TEXT NOT NULL UNIQUE CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    software TEXT NOT NULL CHECK (length(software) BETWEEN 1 AND 63 AND software GLOB '[a-z]*' AND software NOT GLOB '*[^a-z0-9-]*' AND software NOT GLOB '*--*' AND software NOT GLOB '*-'),
    version TEXT NOT NULL CHECK (length(version) BETWEEN 3 AND 64 AND version GLOB '[0-9]*.[0-9]*' AND version NOT GLOB '*[^0-9A-Za-z.+-]*'),
    os TEXT NOT NULL CHECK (os IN ('macos', 'windows', 'linux')),
    arch TEXT NOT NULL CHECK (arch IN ('aarch64', 'x86_64')),
    minimum_os TEXT,
    source TEXT,
    signature_info TEXT CHECK (json_valid(signature_info) AND json_type(signature_info) = 'object'),
    dependency_manifest TEXT CHECK (json_valid(dependency_manifest) AND json_type(dependency_manifest) = 'object'),
    provider_revision_pk INTEGER REFERENCES provider_revisions (pk) ON DELETE RESTRICT,
    created_at TEXT NOT NULL CHECK (length(created_at) = 30 AND created_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z')
) STRICT;
CREATE TABLE audit_events (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*' AND substr(id,9,1) = '-' AND substr(id,14,1) = '-' AND substr(id,19,1) = '-' AND substr(id,24,1) = '-' AND substr(id,15,1) = '7'),
    actor TEXT NOT NULL CHECK (length(actor) BETWEEN 1 AND 64 AND actor GLOB '[a-z]*' AND actor NOT GLOB '*[^a-z0-9_]*'),
    action TEXT NOT NULL CHECK (length(action) BETWEEN 1 AND 64 AND action GLOB '[a-z]*' AND action NOT GLOB '*[^a-z0-9_.]*'),
    resource_kind TEXT CHECK (length(resource_kind) BETWEEN 1 AND 64 AND resource_kind GLOB '[a-z]*' AND resource_kind NOT GLOB '*[^a-z0-9_]*'),
    resource_id TEXT CHECK (length(resource_id) = 36 AND resource_id NOT GLOB '*[^0-9a-f-]*' AND substr(resource_id,9,1) = '-' AND substr(resource_id,14,1) = '-' AND substr(resource_id,19,1) = '-' AND substr(resource_id,24,1) = '-' AND substr(resource_id,15,1) = '7'),
    operation_id TEXT CHECK (length(operation_id) = 36 AND operation_id NOT GLOB '*[^0-9a-f-]*' AND substr(operation_id,9,1) = '-' AND substr(operation_id,14,1) = '-' AND substr(operation_id,19,1) = '-' AND substr(operation_id,24,1) = '-' AND substr(operation_id,15,1) = '7'),
    result TEXT NOT NULL CHECK (length(result) BETWEEN 1 AND 64 AND result GLOB '[a-z]*' AND result NOT GLOB '*[^a-z0-9_]*'),
    safe_details TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(safe_details) AND json_type(safe_details) = 'object'),
    occurred_at TEXT NOT NULL CHECK (length(occurred_at) = 30 AND occurred_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z')
) STRICT;
CREATE TABLE backups (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*' AND substr(id,9,1) = '-' AND substr(id,14,1) = '-' AND substr(id,19,1) = '-' AND substr(id,24,1) = '-' AND substr(id,15,1) = '7'),
    instance_id TEXT CHECK (length(instance_id) = 36 AND instance_id NOT GLOB '*[^0-9a-f-]*' AND substr(instance_id,9,1) = '-' AND substr(instance_id,14,1) = '-' AND substr(instance_id,19,1) = '-' AND substr(instance_id,24,1) = '-' AND substr(instance_id,15,1) = '7') REFERENCES instances (id) ON DELETE SET NULL,
    runtime_id TEXT CHECK (length(runtime_id) = 36 AND runtime_id NOT GLOB '*[^0-9a-f-]*' AND substr(runtime_id,9,1) = '-' AND substr(runtime_id,14,1) = '-' AND substr(runtime_id,19,1) = '-' AND substr(runtime_id,24,1) = '-' AND substr(runtime_id,15,1) = '7') REFERENCES runtimes (id) ON DELETE RESTRICT,
    runtime_identity TEXT NOT NULL CHECK (length(runtime_identity) BETWEEN 1 AND 256),
    method TEXT NOT NULL CHECK (length(method) BETWEEN 1 AND 64 AND method GLOB '[a-z]*' AND method NOT GLOB '*[^a-z0-9_]*'),
    scope TEXT NOT NULL CHECK (length(scope) BETWEEN 1 AND 64 AND scope GLOB '[a-z]*' AND scope NOT GLOB '*[^a-z0-9_]*'),
    consistency TEXT NOT NULL CHECK (length(consistency) BETWEEN 1 AND 64 AND consistency GLOB '[a-z]*' AND consistency NOT GLOB '*[^a-z0-9_]*'),
    path TEXT NOT NULL CHECK (length(path) BETWEEN 2 AND 4096 AND path GLOB '/*'),
    checksum_manifest TEXT CHECK (json_valid(checksum_manifest) AND json_type(checksum_manifest) = 'object'),
    created_at TEXT NOT NULL CHECK (length(created_at) = 30 AND created_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    verified_at TEXT CHECK (length(verified_at) = 30 AND verified_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    restore_verified_at TEXT CHECK (length(restore_verified_at) = 30 AND restore_verified_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z')
) STRICT;
CREATE TABLE config_revisions (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*' AND substr(id,9,1) = '-' AND substr(id,14,1) = '-' AND substr(id,19,1) = '-' AND substr(id,24,1) = '-' AND substr(id,15,1) = '7'),
    instance_id TEXT NOT NULL CHECK (length(instance_id) = 36 AND instance_id NOT GLOB '*[^0-9a-f-]*' AND substr(instance_id,9,1) = '-' AND substr(instance_id,14,1) = '-' AND substr(instance_id,19,1) = '-' AND substr(instance_id,24,1) = '-' AND substr(instance_id,15,1) = '7') REFERENCES instances (id) ON DELETE CASCADE,
    config_values TEXT NOT NULL CHECK (json_valid(config_values) AND json_type(config_values) = 'object'),
    rendered_sha256 TEXT CHECK (length(rendered_sha256) = 64 AND rendered_sha256 NOT GLOB '*[^0-9a-f]*'),
    previous_id TEXT CHECK (length(previous_id) = 36 AND previous_id NOT GLOB '*[^0-9a-f-]*' AND substr(previous_id,9,1) = '-' AND substr(previous_id,14,1) = '-' AND substr(previous_id,19,1) = '-' AND substr(previous_id,24,1) = '-' AND substr(previous_id,15,1) = '7') REFERENCES config_revisions (id) ON DELETE SET NULL,
    status TEXT NOT NULL CHECK (length(status) BETWEEN 1 AND 64 AND status GLOB '[a-z]*' AND status NOT GLOB '*[^a-z0-9_]*'),
    created_by_operation TEXT CHECK (length(created_by_operation) = 36 AND created_by_operation NOT GLOB '*[^0-9a-f-]*' AND substr(created_by_operation,9,1) = '-' AND substr(created_by_operation,14,1) = '-' AND substr(created_by_operation,19,1) = '-' AND substr(created_by_operation,24,1) = '-' AND substr(created_by_operation,15,1) = '7'),
    created_at TEXT NOT NULL CHECK (length(created_at) = 30 AND created_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    UNIQUE (instance_id, id),
    CHECK (previous_id <> id)
) STRICT;
CREATE TABLE environment_instances (
    environment_id TEXT NOT NULL CHECK (length(environment_id) = 36 AND environment_id NOT GLOB '*[^0-9a-f-]*' AND substr(environment_id,9,1) = '-' AND substr(environment_id,14,1) = '-' AND substr(environment_id,19,1) = '-' AND substr(environment_id,24,1) = '-' AND substr(environment_id,15,1) = '7') REFERENCES environments (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    alias TEXT NOT NULL CHECK (length(alias) BETWEEN 1 AND 63 AND alias GLOB '[a-z]*' AND alias NOT GLOB '*[^a-z0-9-]*' AND alias NOT GLOB '*--*' AND alias NOT GLOB '*-'),
    instance_id TEXT NOT NULL CHECK (length(instance_id) = 36 AND instance_id NOT GLOB '*[^0-9a-f-]*' AND substr(instance_id,9,1) = '-' AND substr(instance_id,14,1) = '-' AND substr(instance_id,19,1) = '-' AND substr(instance_id,24,1) = '-' AND substr(instance_id,15,1) = '7') REFERENCES instances (id) ON DELETE RESTRICT,
    ownership TEXT NOT NULL CHECK (ownership IN ('dedicated', 'shared')),
    dependency_conditions TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(dependency_conditions) AND json_type(dependency_conditions) = 'array'),
    PRIMARY KEY (environment_id, alias),
    UNIQUE (environment_id, position),
    UNIQUE (environment_id, instance_id)
) STRICT;
CREATE TABLE environments (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*' AND substr(id,9,1) = '-' AND substr(id,14,1) = '-' AND substr(id,19,1) = '-' AND substr(id,24,1) = '-' AND substr(id,15,1) = '7'),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 63 AND name GLOB '[a-z]*' AND name NOT GLOB '*[^a-z0-9-]*' AND name NOT GLOB '*--*' AND name NOT GLOB '*-'),
    manifest_location TEXT CHECK (length(manifest_location) BETWEEN 2 AND 4096 AND manifest_location GLOB '/*'),
    manifest_sha256 TEXT CHECK (length(manifest_sha256) = 64 AND manifest_sha256 NOT GLOB '*[^0-9a-f]*'),
    lock_sha256 TEXT CHECK (length(lock_sha256) = 64 AND lock_sha256 NOT GLOB '*[^0-9a-f]*'),
    last_operation_id TEXT CHECK (length(last_operation_id) = 36 AND last_operation_id NOT GLOB '*[^0-9a-f-]*' AND substr(last_operation_id,9,1) = '-' AND substr(last_operation_id,14,1) = '-' AND substr(last_operation_id,19,1) = '-' AND substr(last_operation_id,24,1) = '-' AND substr(last_operation_id,15,1) = '7'),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) = 30 AND created_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    updated_at TEXT NOT NULL CHECK (length(updated_at) = 30 AND updated_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z')
) STRICT;
CREATE TABLE instance_demands (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*' AND substr(id,9,1) = '-' AND substr(id,14,1) = '-' AND substr(id,19,1) = '-' AND substr(id,24,1) = '-' AND substr(id,15,1) = '7'),
    instance_id TEXT NOT NULL CHECK (length(instance_id) = 36 AND instance_id NOT GLOB '*[^0-9a-f-]*' AND substr(instance_id,9,1) = '-' AND substr(instance_id,14,1) = '-' AND substr(instance_id,19,1) = '-' AND substr(instance_id,24,1) = '-' AND substr(instance_id,15,1) = '7') REFERENCES instances (id) ON DELETE CASCADE,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('environment', 'manual', 'startup')),
    source_id TEXT CHECK (length(source_id) = 36 AND source_id NOT GLOB '*[^0-9a-f-]*' AND substr(source_id,9,1) = '-' AND substr(source_id,14,1) = '-' AND substr(source_id,19,1) = '-' AND substr(source_id,24,1) = '-' AND substr(source_id,15,1) = '7') REFERENCES environments (id) ON DELETE RESTRICT,
    session_id TEXT CHECK (length(session_id) BETWEEN 1 AND 64 AND session_id NOT GLOB '*[^!-~]*'),
    state TEXT NOT NULL CHECK (state IN ('active', 'suppressed', 'released')),
    state_changed_at TEXT CHECK (length(state_changed_at) = 30 AND state_changed_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    acquired_at TEXT NOT NULL CHECK (length(acquired_at) = 30 AND acquired_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    CHECK ((source_kind = 'environment') = (source_id IS NOT NULL)),
    CHECK (source_kind <> 'environment' OR session_id IS NULL),
    CHECK (source_kind <> 'startup' OR session_id IS NOT NULL),
    CHECK ((state = 'active') = (state_changed_at IS NULL)),
    CHECK (state <> 'suppressed' OR source_kind = 'startup')
) STRICT;
CREATE TABLE instance_endpoints (
    instance_id TEXT NOT NULL CHECK (length(instance_id) = 36 AND instance_id NOT GLOB '*[^0-9a-f-]*' AND substr(instance_id,9,1) = '-' AND substr(instance_id,14,1) = '-' AND substr(instance_id,19,1) = '-' AND substr(instance_id,24,1) = '-' AND substr(instance_id,15,1) = '7') REFERENCES instances (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 63 AND name GLOB '[a-z]*' AND name NOT GLOB '*[^a-z0-9-]*' AND name NOT GLOB '*--*' AND name NOT GLOB '*-' AND length(name) <= 32),
    kind TEXT NOT NULL CHECK (kind IN ('ip', 'unix')),
    transport TEXT CHECK (transport IN ('tcp', 'udp')),
    address TEXT,
    port_request TEXT CHECK (port_request IN ('fixed', 'auto')),
    fixed_port INTEGER CHECK (fixed_port BETWEEN 1 AND 65535),
    bound_port INTEGER CHECK (bound_port BETWEEN 1 AND 65535),
    socket_name TEXT CHECK (length(CAST(socket_name AS BLOB)) BETWEEN 1 AND 15 AND socket_name NOT GLOB '*[^A-Za-z0-9._-]*' AND socket_name NOT IN ('.', '..')),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    PRIMARY KEY (instance_id, name),
    UNIQUE (instance_id, position),
    UNIQUE (instance_id, socket_name),
    CHECK ((kind = 'ip' AND transport IS NOT NULL AND address IS NOT NULL AND port_request IS NOT NULL AND (port_request = 'fixed') = (fixed_port IS NOT NULL) AND socket_name IS NULL) OR (kind = 'unix' AND socket_name IS NOT NULL AND transport IS NULL AND address IS NULL AND port_request IS NULL AND fixed_port IS NULL AND bound_port IS NULL))
) STRICT;
CREATE TABLE instance_paths (
    instance_id TEXT NOT NULL CHECK (length(instance_id) = 36 AND instance_id NOT GLOB '*[^0-9a-f-]*' AND substr(instance_id,9,1) = '-' AND substr(instance_id,14,1) = '-' AND substr(instance_id,19,1) = '-' AND substr(instance_id,24,1) = '-' AND substr(instance_id,15,1) = '7') REFERENCES instances (id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('root', 'data', 'config', 'config_history', 'temp', 'logs', 'run')),
    logical_root TEXT NOT NULL CHECK (logical_root IN ('data', 'logs', 'runtime')),
    relative_path TEXT NOT NULL CHECK (length(relative_path) BETWEEN 1 AND 1024 AND relative_path NOT GLOB '/*' AND relative_path NOT GLOB '*/' AND relative_path NOT GLOB '*//*' AND ('/' || relative_path || '/') NOT GLOB '*/./*' AND ('/' || relative_path || '/') NOT GLOB '*/../*'),
    managed INTEGER NOT NULL CHECK (managed IN (0, 1)),
    PRIMARY KEY (instance_id, kind),
    UNIQUE (logical_root, relative_path)
) STRICT, WITHOUT ROWID;
CREATE TABLE instances (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*' AND substr(id,9,1) = '-' AND substr(id,14,1) = '-' AND substr(id,19,1) = '-' AND substr(id,24,1) = '-' AND substr(id,15,1) = '7'),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 63 AND name GLOB '[a-z]*' AND name NOT GLOB '*[^a-z0-9-]*' AND name NOT GLOB '*--*' AND name NOT GLOB '*-'),
    display_name TEXT CHECK (length(display_name) BETWEEN 1 AND 128),
    provider_revision_pk INTEGER NOT NULL REFERENCES provider_revisions (pk) ON DELETE RESTRICT,
    software TEXT NOT NULL CHECK (length(software) BETWEEN 1 AND 63 AND software GLOB '[a-z]*' AND software NOT GLOB '*[^a-z0-9-]*' AND software NOT GLOB '*--*' AND software NOT GLOB '*-'),
    runtime_id TEXT NOT NULL CHECK (length(runtime_id) = 36 AND runtime_id NOT GLOB '*[^0-9a-f-]*' AND substr(runtime_id,9,1) = '-' AND substr(runtime_id,14,1) = '-' AND substr(runtime_id,19,1) = '-' AND substr(runtime_id,24,1) = '-' AND substr(runtime_id,15,1) = '7') REFERENCES runtimes (id) ON DELETE RESTRICT,
    data_line TEXT NOT NULL CHECK (length(data_line) BETWEEN 1 AND 16 AND data_line GLOB '[0-9]*' AND data_line NOT GLOB '*[^0-9.]*'),
    data_version TEXT NOT NULL CHECK (length(data_version) BETWEEN 3 AND 64 AND data_version GLOB '[0-9]*.[0-9]*' AND data_version NOT GLOB '*[^0-9A-Za-z.+-]*'),
    data_initialized_at TEXT CHECK (length(data_initialized_at) = 30 AND data_initialized_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    desired TEXT NOT NULL CHECK (desired IN ('running', 'stopped')),
    startup TEXT NOT NULL CHECK (startup IN ('manual', 'at_login')),
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('uninitialized', 'initializing', 'stopped', 'starting', 'running', 'stopping', 'recovering', 'failed', 'unknown', 'retained')),
    last_failure TEXT CHECK (length(last_failure) BETWEEN 1 AND 64 AND last_failure GLOB '[a-z]*' AND last_failure NOT GLOB '*[^a-z0-9_]*'),
    config_revision_id TEXT CHECK (length(config_revision_id) = 36 AND config_revision_id NOT GLOB '*[^0-9a-f-]*' AND substr(config_revision_id,9,1) = '-' AND substr(config_revision_id,14,1) = '-' AND substr(config_revision_id,19,1) = '-' AND substr(config_revision_id,24,1) = '-' AND substr(config_revision_id,15,1) = '7'),
    ownership TEXT NOT NULL CHECK (length(ownership) BETWEEN 1 AND 64 AND ownership GLOB '[a-z]*' AND ownership NOT GLOB '*[^a-z0-9_]*'),
    retained_at TEXT CHECK (length(retained_at) = 30 AND retained_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    retained_by_operation TEXT CHECK (length(retained_by_operation) = 36 AND retained_by_operation NOT GLOB '*[^0-9a-f-]*' AND substr(retained_by_operation,9,1) = '-' AND substr(retained_by_operation,14,1) = '-' AND substr(retained_by_operation,19,1) = '-' AND substr(retained_by_operation,24,1) = '-' AND substr(retained_by_operation,15,1) = '7'),
    created_by_operation TEXT NOT NULL CHECK (length(created_by_operation) = 36 AND created_by_operation NOT GLOB '*[^0-9a-f-]*' AND substr(created_by_operation,9,1) = '-' AND substr(created_by_operation,14,1) = '-' AND substr(created_by_operation,19,1) = '-' AND substr(created_by_operation,24,1) = '-' AND substr(created_by_operation,15,1) = '7'),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) = 30 AND created_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    updated_at TEXT NOT NULL CHECK (length(updated_at) = 30 AND updated_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    CHECK ((lifecycle = 'retained') = (retained_at IS NOT NULL)),
    CHECK ((retained_at IS NULL) = (retained_by_operation IS NULL)),
    CHECK (lifecycle NOT IN ('stopped', 'starting', 'running', 'stopping', 'recovering', 'unknown') OR data_initialized_at IS NOT NULL),
    FOREIGN KEY (id, config_revision_id) REFERENCES config_revisions (instance_id, id) DEFERRABLE INITIALLY DEFERRED
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
CREATE TABLE operation_locks (
    operation_id TEXT NOT NULL CHECK (length(operation_id) = 36 AND operation_id NOT GLOB '*[^0-9a-f-]*' AND substr(operation_id,9,1) = '-' AND substr(operation_id,14,1) = '-' AND substr(operation_id,19,1) = '-' AND substr(operation_id,24,1) = '-' AND substr(operation_id,15,1) = '7') REFERENCES operations (id) ON DELETE CASCADE,
    lock_key TEXT NOT NULL CHECK (length(lock_key) BETWEEN 3 AND 1100 AND lock_key GLOB '[a-z]*:?*'),
    PRIMARY KEY (operation_id, lock_key)
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
CREATE TABLE operations (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*' AND substr(id,9,1) = '-' AND substr(id,14,1) = '-' AND substr(id,19,1) = '-' AND substr(id,24,1) = '-' AND substr(id,15,1) = '7'),
    kind TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 64 AND kind GLOB '[a-z]*' AND kind NOT GLOB '*[^a-z0-9_]*'),
    resource_kind TEXT NOT NULL CHECK (resource_kind IN ('runtime', 'instance', 'environment', 'manager')),
    resource_id TEXT CHECK (length(resource_id) = 36 AND resource_id NOT GLOB '*[^0-9a-f-]*' AND substr(resource_id,9,1) = '-' AND substr(resource_id,14,1) = '-' AND substr(resource_id,19,1) = '-' AND substr(resource_id,24,1) = '-' AND substr(resource_id,15,1) = '7'),
    idempotency_key TEXT CHECK (length(idempotency_key) BETWEEN 1 AND 128 AND idempotency_key NOT GLOB '*[^!-~]*'),
    state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'cancel_requested', 'cancelled', 'needs_recovery')),
    stage TEXT CHECK (length(stage) BETWEEN 1 AND 64 AND stage GLOB '[a-z]*' AND stage NOT GLOB '*[^a-z0-9_]*' AND length(stage) <= 32),
    progress_done INTEGER,
    progress_total INTEGER,
    failure_code TEXT CHECK (length(failure_code) BETWEEN 1 AND 64 AND failure_code GLOB '[a-z]*' AND failure_code NOT GLOB '*[^a-z0-9_]*'),
    failure_retryable INTEGER CHECK (failure_retryable IN (0, 1)),
    recovery_hint TEXT CHECK (length(recovery_hint) BETWEEN 1 AND 64 AND recovery_hint GLOB '[a-z]*' AND recovery_hint NOT GLOB '*[^a-z0-9_]*'),
    started_at TEXT CHECK (length(started_at) = 30 AND started_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) = 30 AND created_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    updated_at TEXT NOT NULL CHECK (length(updated_at) = 30 AND updated_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    CHECK ((resource_kind = 'manager') = (resource_id IS NULL)),
    CHECK ((progress_done IS NULL) = (progress_total IS NULL)),
    CHECK (progress_done IS NULL OR (progress_done >= 0 AND progress_done <= progress_total)),
    CHECK ((failure_code IS NULL) = (failure_retryable IS NULL)),
    CHECK (state <> 'failed' OR failure_code IS NOT NULL)
) STRICT;
CREATE TABLE process_observations (
    id INTEGER PRIMARY KEY NOT NULL,
    instance_id TEXT NOT NULL CHECK (length(instance_id) = 36 AND instance_id NOT GLOB '*[^0-9a-f-]*' AND substr(instance_id,9,1) = '-' AND substr(instance_id,14,1) = '-' AND substr(instance_id,19,1) = '-' AND substr(instance_id,24,1) = '-' AND substr(instance_id,15,1) = '7') REFERENCES instances (id) ON DELETE CASCADE,
    pid INTEGER NOT NULL CHECK (pid > 0),
    start_identity TEXT NOT NULL CHECK (length(start_identity) BETWEEN 1 AND 256),
    executable_identity TEXT NOT NULL CHECK (length(executable_identity) BETWEEN 1 AND 1024),
    boot_id TEXT,
    session_id TEXT,
    observed_at TEXT NOT NULL CHECK (length(observed_at) = 30 AND observed_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z')
) STRICT;
CREATE TABLE provider_revisions (
    pk INTEGER PRIMARY KEY NOT NULL,
    provider TEXT NOT NULL CHECK (length(provider) BETWEEN 3 AND 127 AND provider GLOB '[a-z]*/[a-z]*' AND provider NOT GLOB '*/*/*' AND provider NOT GLOB '*[^a-z0-9/-]*'),
    revision TEXT NOT NULL CHECK (length(revision) BETWEEN 1 AND 64 AND revision GLOB '[0-9A-Za-z]*' AND revision NOT GLOB '*[^0-9A-Za-z.+_-]*' AND revision NOT GLOB '*..*'),
    manifest_sha256 TEXT NOT NULL CHECK (length(manifest_sha256) = 64 AND manifest_sha256 NOT GLOB '*[^0-9a-f]*'),
    software TEXT NOT NULL CHECK (length(software) BETWEEN 1 AND 63 AND software GLOB '[a-z]*' AND software NOT GLOB '*[^a-z0-9-]*' AND software NOT GLOB '*--*' AND software NOT GLOB '*-'),
    schema_version TEXT,
    publisher TEXT,
    capabilities TEXT NOT NULL CHECK (json_valid(capabilities) AND json_type(capabilities) = 'array'),
    line_compatibility TEXT NOT NULL CHECK (json_valid(line_compatibility) AND json_type(line_compatibility) = 'object'),
    source_id TEXT CHECK (length(source_id) = 36 AND source_id NOT GLOB '*[^0-9a-f-]*' AND substr(source_id,9,1) = '-' AND substr(source_id,14,1) = '-' AND substr(source_id,19,1) = '-' AND substr(source_id,24,1) = '-' AND substr(source_id,15,1) = '7') REFERENCES provider_sources (id) ON DELETE RESTRICT,
    approval TEXT CHECK (length(approval) BETWEEN 1 AND 64 AND approval GLOB '[a-z]*' AND approval NOT GLOB '*[^a-z0-9_]*'),
    approved_at TEXT CHECK (length(approved_at) = 30 AND approved_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    created_at TEXT NOT NULL CHECK (length(created_at) = 30 AND created_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    UNIQUE (provider, revision, manifest_sha256)
) STRICT;
CREATE TABLE provider_sources (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*' AND substr(id,9,1) = '-' AND substr(id,14,1) = '-' AND substr(id,19,1) = '-' AND substr(id,24,1) = '-' AND substr(id,15,1) = '7'),
    kind TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 64 AND kind GLOB '[a-z]*' AND kind NOT GLOB '*[^a-z0-9_]*'),
    location TEXT NOT NULL CHECK (length(location) BETWEEN 1 AND 4096),
    trust_root TEXT,
    last_sync_at TEXT CHECK (length(last_sync_at) = 30 AND last_sync_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    status TEXT NOT NULL CHECK (length(status) BETWEEN 1 AND 64 AND status GLOB '[a-z]*' AND status NOT GLOB '*[^a-z0-9_]*'),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) = 30 AND created_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    updated_at TEXT NOT NULL CHECK (length(updated_at) = 30 AND updated_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    UNIQUE (kind, location)
) STRICT;
CREATE TABLE registry_info (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    registry_id TEXT NOT NULL CHECK (length(registry_id) = 36 AND registry_id NOT GLOB '*[^0-9a-f-]*' AND substr(registry_id,9,1) = '-' AND substr(registry_id,14,1) = '-' AND substr(registry_id,19,1) = '-' AND substr(registry_id,24,1) = '-' AND substr(registry_id,15,1) = '7'),
    created_at TEXT NOT NULL CHECK (length(created_at) = 30 AND created_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    created_by_version TEXT NOT NULL CHECK (length(created_by_version) BETWEEN 1 AND 64),
    generation INTEGER NOT NULL CHECK (generation >= 0)
) STRICT;
CREATE TABLE runtime_dependencies (
    runtime_id TEXT NOT NULL CHECK (length(runtime_id) = 36 AND runtime_id NOT GLOB '*[^0-9a-f-]*' AND substr(runtime_id,9,1) = '-' AND substr(runtime_id,14,1) = '-' AND substr(runtime_id,19,1) = '-' AND substr(runtime_id,24,1) = '-' AND substr(runtime_id,15,1) = '7') REFERENCES runtimes (id) ON DELETE CASCADE,
    dependency_runtime_id TEXT NOT NULL CHECK (length(dependency_runtime_id) = 36 AND dependency_runtime_id NOT GLOB '*[^0-9a-f-]*' AND substr(dependency_runtime_id,9,1) = '-' AND substr(dependency_runtime_id,14,1) = '-' AND substr(dependency_runtime_id,19,1) = '-' AND substr(dependency_runtime_id,24,1) = '-' AND substr(dependency_runtime_id,15,1) = '7') REFERENCES runtimes (id) ON DELETE RESTRICT,
    version_constraint TEXT,
    PRIMARY KEY (runtime_id, dependency_runtime_id),
    CHECK (runtime_id <> dependency_runtime_id)
) STRICT;
CREATE TABLE runtimes (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*' AND substr(id,9,1) = '-' AND substr(id,14,1) = '-' AND substr(id,19,1) = '-' AND substr(id,24,1) = '-' AND substr(id,15,1) = '7'),
    software TEXT NOT NULL CHECK (length(software) BETWEEN 1 AND 63 AND software GLOB '[a-z]*' AND software NOT GLOB '*[^a-z0-9-]*' AND software NOT GLOB '*--*' AND software NOT GLOB '*-'),
    version TEXT NOT NULL CHECK (length(version) BETWEEN 3 AND 64 AND version GLOB '[0-9]*.[0-9]*' AND version NOT GLOB '*[^0-9A-Za-z.+-]*'),
    line TEXT NOT NULL CHECK (length(line) BETWEEN 1 AND 16 AND line GLOB '[0-9]*' AND line NOT GLOB '*[^0-9.]*'),
    os TEXT NOT NULL CHECK (os IN ('macos', 'windows', 'linux')),
    arch TEXT NOT NULL CHECK (arch IN ('aarch64', 'x86_64')),
    install_mode TEXT NOT NULL CHECK (install_mode IN ('managed', 'external')),
    artifact_sha256 TEXT CHECK (length(artifact_sha256) = 64 AND artifact_sha256 NOT GLOB '*[^0-9a-f]*') REFERENCES artifacts (sha256) ON DELETE RESTRICT,
    external_root TEXT CHECK (length(external_root) BETWEEN 2 AND 4096 AND external_root GLOB '/*'),
    observed_sha256 TEXT CHECK (length(observed_sha256) = 64 AND observed_sha256 NOT GLOB '*[^0-9a-f]*'),
    state TEXT NOT NULL CHECK (state IN ('absent', 'resolving', 'downloading', 'verifying', 'installing', 'installed', 'failed', 'missing', 'drifted')),
    installed_at TEXT CHECK (length(installed_at) = 30 AND installed_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    last_verified_at TEXT CHECK (length(last_verified_at) = 30 AND last_verified_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) = 30 AND created_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    updated_at TEXT NOT NULL CHECK (length(updated_at) = 30 AND updated_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    CHECK ((install_mode = 'managed' AND artifact_sha256 IS NOT NULL AND external_root IS NULL AND observed_sha256 IS NULL) OR (install_mode = 'external' AND artifact_sha256 IS NULL AND external_root IS NOT NULL))
) STRICT;
CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY NOT NULL CHECK (version >= 1),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 64 AND name NOT GLOB '*[^a-z0-9_]*'),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    applied_at TEXT NOT NULL CHECK (length(applied_at) = 30 AND applied_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    app_version TEXT NOT NULL CHECK (length(app_version) BETWEEN 1 AND 64)
) STRICT;
CREATE TABLE secret_reference_uses (
    reference_id TEXT NOT NULL CHECK (length(reference_id) = 36 AND reference_id NOT GLOB '*[^0-9a-f-]*' AND substr(reference_id,9,1) = '-' AND substr(reference_id,14,1) = '-' AND substr(reference_id,19,1) = '-' AND substr(reference_id,24,1) = '-' AND substr(reference_id,15,1) = '7') REFERENCES secret_references (id) ON DELETE RESTRICT,
    resource_kind TEXT NOT NULL CHECK (resource_kind IN ('instance', 'environment')),
    resource_id TEXT NOT NULL CHECK (length(resource_id) = 36 AND resource_id NOT GLOB '*[^0-9a-f-]*' AND substr(resource_id,9,1) = '-' AND substr(resource_id,14,1) = '-' AND substr(resource_id,19,1) = '-' AND substr(resource_id,24,1) = '-' AND substr(resource_id,15,1) = '7'),
    field TEXT NOT NULL CHECK (length(field) BETWEEN 1 AND 64 AND field GLOB '[a-z]*' AND field NOT GLOB '*[^A-Za-z0-9_-]*'),
    PRIMARY KEY (reference_id, resource_kind, resource_id, field)
) STRICT;
CREATE TABLE secret_references (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*' AND substr(id,9,1) = '-' AND substr(id,14,1) = '-' AND substr(id,19,1) = '-' AND substr(id,24,1) = '-' AND substr(id,15,1) = '7'),
    reference TEXT NOT NULL UNIQUE CHECK (length(CAST(reference AS BLOB)) BETWEEN 9 AND 255 AND reference GLOB 'local:*'),
    platform_store_key TEXT NOT NULL CHECK (length(CAST(platform_store_key AS BLOB)) BETWEEN 1 AND 512),
    owner_kind TEXT NOT NULL CHECK (owner_kind IN ('instance', 'environment', 'user')),
    owner_id TEXT CHECK (length(owner_id) = 36 AND owner_id NOT GLOB '*[^0-9a-f-]*' AND substr(owner_id,9,1) = '-' AND substr(owner_id,14,1) = '-' AND substr(owner_id,19,1) = '-' AND substr(owner_id,24,1) = '-' AND substr(owner_id,15,1) = '7'),
    purpose TEXT NOT NULL CHECK (length(purpose) BETWEEN 1 AND 64 AND purpose GLOB '[a-z0-9]*' AND purpose NOT GLOB '*[^a-z0-9._-]*'),
    value_origin TEXT CHECK (value_origin IN ('generated', 'entered', 'copied')),
    status TEXT NOT NULL CHECK (status IN ('present', 'missing', 'unknown')),
    status_observed_at TEXT CHECK (length(status_observed_at) = 30 AND status_observed_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    value_updated_at TEXT CHECK (length(value_updated_at) = 30 AND value_updated_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) = 30 AND created_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    updated_at TEXT NOT NULL CHECK (length(updated_at) = 30 AND updated_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    -- The owner is polymorphic, so it has no foreign key: a user owner has
    -- no ID, an instance or environment owner always has one.
    CHECK ((owner_kind = 'user') = (owner_id IS NULL))
) STRICT;
CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL CHECK (length(key) BETWEEN 1 AND 128 AND key GLOB '[a-z]*' AND key NOT GLOB '*[^a-z0-9_.]*'),
    typed_value TEXT NOT NULL CHECK (json_valid(typed_value)),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    updated_at TEXT NOT NULL CHECK (length(updated_at) = 30 AND updated_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z')
) STRICT;
CREATE INDEX artifacts_provider_revision_pk ON artifacts (provider_revision_pk);
CREATE INDEX audit_events_occurred_at ON audit_events (occurred_at);
CREATE INDEX audit_events_operation_id ON audit_events (operation_id);
CREATE INDEX backups_instance_id ON backups (instance_id);
CREATE INDEX backups_runtime_id ON backups (runtime_id);
CREATE INDEX config_revisions_previous_id ON config_revisions (previous_id);
CREATE UNIQUE INDEX environment_instances_dedicated ON environment_instances (instance_id) WHERE ownership = 'dedicated';
CREATE INDEX environment_instances_instance_id ON environment_instances (instance_id);
CREATE INDEX environments_name ON environments (name);
CREATE UNIQUE INDEX instance_demands_active_environment ON instance_demands (instance_id, source_id) WHERE source_kind = 'environment' AND state = 'active';
CREATE INDEX instance_demands_instance_state ON instance_demands (instance_id, state);
CREATE INDEX instance_demands_source_id ON instance_demands (source_id);
CREATE UNIQUE INDEX instances_active_name ON instances (name) WHERE retained_at IS NULL;
CREATE INDEX instances_provider_revision_pk ON instances (provider_revision_pk);
CREATE INDEX instances_runtime_id ON instances (runtime_id);
CREATE UNIQUE INDEX instances_short_key ON instances (substr(id, 29));
CREATE INDEX operation_created_step ON operation_created (operation_id, step_seq);
CREATE INDEX operation_locks_key ON operation_locks (lock_key);
CREATE INDEX operations_active ON operations (created_at) WHERE state IN ('queued', 'running', 'cancel_requested', 'needs_recovery');
CREATE INDEX operations_created ON operations (created_at, id);
CREATE UNIQUE INDEX operations_idempotency_key ON operations (idempotency_key) WHERE idempotency_key IS NOT NULL;
CREATE INDEX operations_resource ON operations (resource_kind, resource_id);
CREATE INDEX operations_state_updated ON operations (state, updated_at);
CREATE INDEX process_observations_instance_observed ON process_observations (instance_id, observed_at);
CREATE INDEX provider_revisions_source_id ON provider_revisions (source_id);
CREATE INDEX runtime_dependencies_dependency_runtime_id ON runtime_dependencies (dependency_runtime_id);
CREATE INDEX runtimes_artifact_sha256 ON runtimes (artifact_sha256);
CREATE UNIQUE INDEX runtimes_external_root ON runtimes (external_root) WHERE install_mode = 'external';
CREATE UNIQUE INDEX runtimes_managed_artifact ON runtimes (artifact_sha256) WHERE install_mode = 'managed';
CREATE INDEX secret_reference_uses_resource ON secret_reference_uses (resource_kind, resource_id);
CREATE INDEX secret_references_owner ON secret_references (owner_kind, owner_id);
CREATE TRIGGER audit_events_append_only
BEFORE UPDATE ON audit_events
BEGIN
    SELECT RAISE(ABORT, 'audit events are append only');
END;
CREATE TRIGGER provider_revisions_immutable
BEFORE UPDATE OF pk, provider, revision, manifest_sha256, software, schema_version, publisher, capabilities, line_compatibility, source_id, created_at ON provider_revisions
BEGIN
    SELECT RAISE(ABORT, 'provider revision is immutable');
END;
CREATE TRIGGER secret_references_owner_fixed
BEFORE UPDATE OF owner_kind, owner_id ON secret_references
WHEN (NEW.owner_kind IS NOT OLD.owner_kind OR NEW.owner_id IS NOT OLD.owner_id)
    AND NOT (NEW.owner_kind = 'user' AND NEW.owner_id IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'the owner of a secret reference is fixed');
END;
INSERT INTO artifacts (pk, sha256, software, version, os, arch, minimum_os, source, signature_info, dependency_manifest, provider_revision_pk, created_at) VALUES (1, 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc', 'postgresql', '17.6', 'macos', 'aarch64', '27.0', 'https://example.invalid/postgresql-17.6.tar.zst', '{"team":"EXAMPLE"}', '{"libraries":[]}', 1, '2026-01-01T00:00:00.000000000Z');
INSERT INTO audit_events (id, actor, action, resource_kind, resource_id, operation_id, result, safe_details, occurred_at) VALUES ('0199f000-0000-7000-8000-0000000000a0', 'user', 'secret.set', 'secret', '0199f000-0000-7000-8000-000000000061', NULL, 'succeeded', '{"clientKind":"cli","peerUid":"501","reference":"local:erp/postgres/admin"}', '2026-01-02T03:04:05.123456789Z');
INSERT INTO audit_events (id, actor, action, resource_kind, resource_id, operation_id, result, safe_details, occurred_at) VALUES ('0199f000-0000-7000-8000-0000000000a1', 'daemon', 'secret.set', 'secret', '0199f000-0000-7000-8000-000000000060', '0199f000-0000-7000-8000-000000000050', 'succeeded', '{"reference":"local:instance/0199f000-0000-7000-8000-000000000030/admin-password"}', '2026-01-01T00:00:00.000000000Z');
INSERT INTO audit_events (id, actor, action, resource_kind, resource_id, operation_id, result, safe_details, occurred_at) VALUES ('0199f000-0000-7000-8000-0000000000a2', 'user', 'secret.reveal', NULL, NULL, NULL, 'failed', '{"clientKind":"gui","errorCode":"secret.not_found","purpose":"show","reference":"local:gone/admin"}', '2026-01-02T03:04:05.123456789Z');
INSERT INTO backups (id, instance_id, runtime_id, runtime_identity, method, scope, consistency, path, checksum_manifest, created_at, verified_at, restore_verified_at) VALUES ('0199f000-0000-7000-8000-000000000090', '0199f000-0000-7000-8000-000000000030', '0199f000-0000-7000-8000-000000000020', 'postgresql 17.6 macos-aarch64', 'logical_dump', 'instance', 'consistent', '/Users/example/Backups/orders-db.dump', '{"orders-db.dump":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"}', '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z', NULL);
INSERT INTO config_revisions (id, instance_id, config_values, rendered_sha256, previous_id, status, created_by_operation, created_at) VALUES ('0199f000-0000-7000-8000-000000000040', '0199f000-0000-7000-8000-000000000030', '{"adminPassword":"local:instance/0199f000-0000-7000-8000-000000000030/admin-password","max_connections":100}', 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', '0199f000-0000-7000-8000-000000000041', 'active', '0199f000-0000-7000-8000-000000000050', '2026-01-02T03:04:05.123456789Z');
INSERT INTO config_revisions (id, instance_id, config_values, rendered_sha256, previous_id, status, created_by_operation, created_at) VALUES ('0199f000-0000-7000-8000-000000000041', '0199f000-0000-7000-8000-000000000030', '{"max_connections":50}', NULL, NULL, 'superseded', NULL, '2026-01-01T00:00:00.000000000Z');
INSERT INTO environment_instances (environment_id, position, alias, instance_id, ownership, dependency_conditions) VALUES ('0199f000-0000-7000-8000-000000000070', 0, 'db', '0199f000-0000-7000-8000-000000000030', 'dedicated', '[]');
INSERT INTO environments (id, name, manifest_location, manifest_sha256, lock_sha256, last_operation_id, revision, created_at, updated_at) VALUES ('0199f000-0000-7000-8000-000000000070', 'shop', '/Users/example/shop/stackly.yaml', 'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd', 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', '0199f000-0000-7000-8000-000000000053', 2, '2026-01-01T00:00:00.000000000Z', '2026-01-02T03:04:05.123456789Z');
INSERT INTO instance_demands (id, instance_id, source_kind, source_id, session_id, state, state_changed_at, acquired_at) VALUES ('0199f000-0000-7000-8000-000000000080', '0199f000-0000-7000-8000-000000000030', 'environment', '0199f000-0000-7000-8000-000000000070', NULL, 'active', NULL, '2026-01-01T00:00:00.000000000Z');
INSERT INTO instance_demands (id, instance_id, source_kind, source_id, session_id, state, state_changed_at, acquired_at) VALUES ('0199f000-0000-7000-8000-000000000081', '0199f000-0000-7000-8000-000000000030', 'startup', NULL, 'login-1', 'suppressed', '2026-01-02T03:04:05.123456789Z', '2026-01-01T00:00:00.000000000Z');
INSERT INTO instance_demands (id, instance_id, source_kind, source_id, session_id, state, state_changed_at, acquired_at) VALUES ('0199f000-0000-7000-8000-000000000082', '0199f000-0000-7000-8000-000000000030', 'manual', NULL, NULL, 'released', '2026-01-02T03:04:05.123456789Z', '2026-01-01T00:00:00.000000000Z');
INSERT INTO instance_endpoints (instance_id, position, name, kind, transport, address, port_request, fixed_port, bound_port, socket_name, enabled) VALUES ('0199f000-0000-7000-8000-000000000030', 0, 'postgres', 'ip', 'tcp', '127.0.0.1', 'fixed', 5432, 5432, NULL, 1);
INSERT INTO instance_endpoints (instance_id, position, name, kind, transport, address, port_request, fixed_port, bound_port, socket_name, enabled) VALUES ('0199f000-0000-7000-8000-000000000030', 1, 'socket', 'unix', NULL, NULL, NULL, NULL, NULL, '.s.PGSQL.5432', 1);
INSERT INTO instance_endpoints (instance_id, position, name, kind, transport, address, port_request, fixed_port, bound_port, socket_name, enabled) VALUES ('0199f000-0000-7000-8000-000000000031', 0, 'postgres', 'ip', 'tcp', '::1', 'auto', NULL, NULL, NULL, 0);
INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed) VALUES ('0199f000-0000-7000-8000-000000000030', 'config', 'data', 'instances/0199f000-0000-7000-8000-000000000030/config', 1);
INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed) VALUES ('0199f000-0000-7000-8000-000000000030', 'config_history', 'data', 'instances/0199f000-0000-7000-8000-000000000030/config-history', 1);
INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed) VALUES ('0199f000-0000-7000-8000-000000000030', 'data', 'data', 'instances/0199f000-0000-7000-8000-000000000030/data', 1);
INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed) VALUES ('0199f000-0000-7000-8000-000000000030', 'logs', 'logs', 'instances/0199f000-0000-7000-8000-000000000030', 1);
INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed) VALUES ('0199f000-0000-7000-8000-000000000030', 'root', 'data', 'instances/0199f000-0000-7000-8000-000000000030', 1);
INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed) VALUES ('0199f000-0000-7000-8000-000000000030', 'run', 'runtime', 'i/00000030', 1);
INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed) VALUES ('0199f000-0000-7000-8000-000000000030', 'temp', 'data', 'instances/0199f000-0000-7000-8000-000000000030/temp', 1);
INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed) VALUES ('0199f000-0000-7000-8000-000000000031', 'config', 'data', 'retained/0199f000-0000-7000-8000-000000000031/config', 1);
INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed) VALUES ('0199f000-0000-7000-8000-000000000031', 'config_history', 'data', 'retained/0199f000-0000-7000-8000-000000000031/config-history', 1);
INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed) VALUES ('0199f000-0000-7000-8000-000000000031', 'data', 'data', 'retained/0199f000-0000-7000-8000-000000000031/data', 1);
INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed) VALUES ('0199f000-0000-7000-8000-000000000031', 'logs', 'logs', 'instances/0199f000-0000-7000-8000-000000000031', 1);
INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed) VALUES ('0199f000-0000-7000-8000-000000000031', 'root', 'data', 'retained/0199f000-0000-7000-8000-000000000031', 1);
INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed) VALUES ('0199f000-0000-7000-8000-000000000031', 'run', 'runtime', 'i/00000031', 1);
INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed) VALUES ('0199f000-0000-7000-8000-000000000031', 'temp', 'data', 'retained/0199f000-0000-7000-8000-000000000031/temp', 1);
INSERT INTO instances (id, name, display_name, provider_revision_pk, software, runtime_id, data_line, data_version, data_initialized_at, desired, startup, lifecycle, last_failure, config_revision_id, ownership, retained_at, retained_by_operation, created_by_operation, revision, created_at, updated_at) VALUES ('0199f000-0000-7000-8000-000000000030', 'orders-db', 'Orders DB', 1, 'postgresql', '0199f000-0000-7000-8000-000000000020', '17', '17.6', '2026-01-01T00:00:00.000000000Z', 'running', 'at_login', 'running', NULL, '0199f000-0000-7000-8000-000000000040', 'created', NULL, NULL, '0199f000-0000-7000-8000-000000000050', 3, '2026-01-01T00:00:00.000000000Z', '2026-01-02T03:04:05.123456789Z');
INSERT INTO instances (id, name, display_name, provider_revision_pk, software, runtime_id, data_line, data_version, data_initialized_at, desired, startup, lifecycle, last_failure, config_revision_id, ownership, retained_at, retained_by_operation, created_by_operation, revision, created_at, updated_at) VALUES ('0199f000-0000-7000-8000-000000000031', 'orders-db', NULL, 1, 'postgresql', '0199f000-0000-7000-8000-000000000020', '17', '17.6', NULL, 'stopped', 'manual', 'retained', 'start_timeout', NULL, 'created', '2026-01-02T03:04:05.123456789Z', '0199f000-0000-7000-8000-000000000052', '0199f000-0000-7000-8000-000000000051', 4, '2026-01-01T00:00:00.000000000Z', '2026-01-02T03:04:05.123456789Z');
INSERT INTO operation_created (operation_id, seq, step_seq, item_kind, item_ref, payload, state, detail, recorded_at, settled_at) VALUES ('0199f000-0000-7000-8000-000000000052', 1, 1, 'instance_tree', '0199f000-0000-7000-8000-000000000031', NULL, 'compensation_failed', 'permission_denied', '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z');
INSERT INTO operation_created (operation_id, seq, step_seq, item_kind, item_ref, payload, state, detail, recorded_at, settled_at) VALUES ('0199f000-0000-7000-8000-000000000053', 1, 1, 'custom', 'supervisor_spawned_process:pid:4242', '{"pid":4242}', 'intended', NULL, '2026-01-02T03:04:05.123456789Z', NULL);
INSERT INTO operation_journal (operation_id, method, request_fingerprint, trigger_origin, trigger_client, trigger_interactive, resumable, input, expected_revision, waiting_on, point_of_no_return_at, cancel_requested_at, compensation_started_at, recovery_cause, failure_error_code, failure_message, failure_details, result) VALUES ('0199f000-0000-7000-8000-000000000050', 'instance.create', 'abababababababababababababababababababababababababababababababab', 'user', 'cli', 1, 0, '{"name":"orders-db"}', NULL, NULL, '2026-01-01T00:00:00.000000000Z', NULL, NULL, NULL, NULL, NULL, '[]', '{"resources":[]}');
INSERT INTO operation_journal (operation_id, method, request_fingerprint, trigger_origin, trigger_client, trigger_interactive, resumable, input, expected_revision, waiting_on, point_of_no_return_at, cancel_requested_at, compensation_started_at, recovery_cause, failure_error_code, failure_message, failure_details, result) VALUES ('0199f000-0000-7000-8000-000000000051', NULL, NULL, 'background', NULL, 0, 0, '{}', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, '[]', NULL);
INSERT INTO operation_journal (operation_id, method, request_fingerprint, trigger_origin, trigger_client, trigger_interactive, resumable, input, expected_revision, waiting_on, point_of_no_return_at, cancel_requested_at, compensation_started_at, recovery_cause, failure_error_code, failure_message, failure_details, result) VALUES ('0199f000-0000-7000-8000-000000000052', 'instance.remove', 'cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd', 'user', 'gui', 1, 0, '{}', 3, NULL, '2026-01-02T03:04:05.123456789Z', NULL, '2026-01-02T03:04:05.123456789Z', 'compensation_failed', 'operation.compensation_failed', 'undoing the instance folders failed', '[{"key":"item","value":"instance_tree"},{"key":"detail","value":"permission_denied"}]', NULL);
INSERT INTO operation_journal (operation_id, method, request_fingerprint, trigger_origin, trigger_client, trigger_interactive, resumable, input, expected_revision, waiting_on, point_of_no_return_at, cancel_requested_at, compensation_started_at, recovery_cause, failure_error_code, failure_message, failure_details, result) VALUES ('0199f000-0000-7000-8000-000000000053', NULL, NULL, 'background', NULL, 0, 1, '{"environment":"shop"}', NULL, '0199f000-0000-7000-8000-000000000052', NULL, '2026-01-02T03:04:05.123456789Z', NULL, NULL, NULL, NULL, '[]', NULL);
INSERT INTO operation_journal (operation_id, method, request_fingerprint, trigger_origin, trigger_client, trigger_interactive, resumable, input, expected_revision, waiting_on, point_of_no_return_at, cancel_requested_at, compensation_started_at, recovery_cause, failure_error_code, failure_message, failure_details, result) VALUES ('0199f000-0000-7000-8000-000000000054', 'runtime.install', 'efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef', 'user', 'cli', 0, 1, '{"version":"17.6"}', NULL, NULL, NULL, NULL, '2026-01-01T00:00:00.000000000Z', NULL, 'runtime.checksum_mismatch', 'the artifact digest does not match', '[]', NULL);
INSERT INTO operation_journal (operation_id, method, request_fingerprint, trigger_origin, trigger_client, trigger_interactive, resumable, input, expected_revision, waiting_on, point_of_no_return_at, cancel_requested_at, compensation_started_at, recovery_cause, failure_error_code, failure_message, failure_details, result) VALUES ('0199f000-0000-7000-8000-000000000055', NULL, NULL, 'background', NULL, 0, 0, '{}', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, '[]', NULL);
INSERT INTO operation_locks (operation_id, lock_key) VALUES ('0199f000-0000-7000-8000-000000000052', 'instance:0199f000-0000-7000-8000-000000000031');
INSERT INTO operation_locks (operation_id, lock_key) VALUES ('0199f000-0000-7000-8000-000000000053', 'environment:0199f000-0000-7000-8000-000000000070');
INSERT INTO operation_locks (operation_id, lock_key) VALUES ('0199f000-0000-7000-8000-000000000055', 'manager:self');
INSERT INTO operation_steps (operation_id, seq, name, repeatable, reversible, state, attempts, checkpoint, error_code, started_at, finished_at) VALUES ('0199f000-0000-7000-8000-000000000052', 1, 'move', 0, 1, 'done', 1, '{"moved":true}', NULL, '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z');
INSERT INTO operation_steps (operation_id, seq, name, repeatable, reversible, state, attempts, checkpoint, error_code, started_at, finished_at) VALUES ('0199f000-0000-7000-8000-000000000052', 2, 'rebind', 1, 0, 'failed', 2, NULL, 'instance.rebind_failed', '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z');
INSERT INTO operation_steps (operation_id, seq, name, repeatable, reversible, state, attempts, checkpoint, error_code, started_at, finished_at) VALUES ('0199f000-0000-7000-8000-000000000053', 1, 'start_members', 1, 1, 'running', 1, NULL, NULL, '2026-01-02T03:04:05.123456789Z', NULL);
INSERT INTO operations (id, kind, resource_kind, resource_id, idempotency_key, state, stage, progress_done, progress_total, failure_code, failure_retryable, recovery_hint, started_at, revision, created_at, updated_at) VALUES ('0199f000-0000-7000-8000-000000000050', 'create_instance', 'instance', '0199f000-0000-7000-8000-000000000030', 'cli-create-1', 'succeeded', NULL, 5, 5, NULL, NULL, NULL, '2026-01-01T00:00:00.000000000Z', 4, '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z');
INSERT INTO operations (id, kind, resource_kind, resource_id, idempotency_key, state, stage, progress_done, progress_total, failure_code, failure_retryable, recovery_hint, started_at, revision, created_at, updated_at) VALUES ('0199f000-0000-7000-8000-000000000051', 'create_instance', 'instance', '0199f000-0000-7000-8000-000000000031', NULL, 'succeeded', NULL, NULL, NULL, NULL, NULL, NULL, NULL, 2, '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z');
INSERT INTO operations (id, kind, resource_kind, resource_id, idempotency_key, state, stage, progress_done, progress_total, failure_code, failure_retryable, recovery_hint, started_at, revision, created_at, updated_at) VALUES ('0199f000-0000-7000-8000-000000000052', 'retain_instance', 'instance', '0199f000-0000-7000-8000-000000000031', NULL, 'needs_recovery', 'moving', 1, 3, NULL, NULL, 'resume_retain', '2026-01-02T03:04:05.123456789Z', 3, '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z');
INSERT INTO operations (id, kind, resource_kind, resource_id, idempotency_key, state, stage, progress_done, progress_total, failure_code, failure_retryable, recovery_hint, started_at, revision, created_at, updated_at) VALUES ('0199f000-0000-7000-8000-000000000053', 'environment_up', 'environment', '0199f000-0000-7000-8000-000000000070', NULL, 'running', 'starting', NULL, NULL, NULL, NULL, NULL, '2026-01-02T03:04:05.123456789Z', 2, '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z');
INSERT INTO operations (id, kind, resource_kind, resource_id, idempotency_key, state, stage, progress_done, progress_total, failure_code, failure_retryable, recovery_hint, started_at, revision, created_at, updated_at) VALUES ('0199f000-0000-7000-8000-000000000054', 'install_runtime', 'runtime', '0199f000-0000-7000-8000-000000000020', NULL, 'failed', 'verifying', NULL, NULL, 'checksum_mismatch', 0, NULL, '2026-01-01T00:00:00.000000000Z', 3, '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z');
INSERT INTO operations (id, kind, resource_kind, resource_id, idempotency_key, state, stage, progress_done, progress_total, failure_code, failure_retryable, recovery_hint, started_at, revision, created_at, updated_at) VALUES ('0199f000-0000-7000-8000-000000000055', 'doctor', 'manager', NULL, NULL, 'queued', NULL, NULL, NULL, NULL, NULL, NULL, NULL, 1, '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z');
INSERT INTO process_observations (id, instance_id, pid, start_identity, executable_identity, boot_id, session_id, observed_at) VALUES (1, '0199f000-0000-7000-8000-000000000030', 4242, '1700000000.000000', '/Users/example/Library/Application Support/Stackly/runtimes/postgresql/17.6/macos-aarch64/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc/bin/postgres', '6f1d2c3b-4a59-4e8f-9a0b-1c2d3e4f5a6b', '4242', '2026-01-02T03:04:05.123456789Z');
INSERT INTO provider_revisions (pk, provider, revision, manifest_sha256, software, schema_version, publisher, capabilities, line_compatibility, source_id, approval, approved_at, created_at) VALUES (1, 'official/postgresql', '2026.10.1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'postgresql', 'stackly.provider/v1', 'stackly', '["backup","start","stop"]', '{"16":["16","17"],"17":["17"]}', '0199f000-0000-7000-8000-000000000010', 'approved', '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z');
INSERT INTO provider_revisions (pk, provider, revision, manifest_sha256, software, schema_version, publisher, capabilities, line_compatibility, source_id, approval, approved_at, created_at) VALUES (2, 'official/valkey', '2026.10.1', 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 'valkey', NULL, NULL, '[]', '{}', NULL, NULL, NULL, '2026-01-01T00:00:00.000000000Z');
INSERT INTO provider_sources (id, kind, location, trust_root, last_sync_at, status, revision, created_at, updated_at) VALUES ('0199f000-0000-7000-8000-000000000010', 'builtin', 'stackly:providers', NULL, '2026-01-01T00:00:00.000000000Z', 'trusted', 1, '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z');
INSERT INTO registry_info (singleton, registry_id, created_at, created_by_version, generation) VALUES (1, '0199f000-0000-7000-8000-000000000001', '2026-01-01T00:00:00.000000000Z', '0.1.0', 13);
INSERT INTO runtime_dependencies (runtime_id, dependency_runtime_id, version_constraint) VALUES ('0199f000-0000-7000-8000-000000000020', '0199f000-0000-7000-8000-000000000021', NULL);
INSERT INTO runtimes (id, software, version, line, os, arch, install_mode, artifact_sha256, external_root, observed_sha256, state, installed_at, last_verified_at, revision, created_at, updated_at) VALUES ('0199f000-0000-7000-8000-000000000020', 'postgresql', '17.6', '17', 'macos', 'aarch64', 'managed', 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc', NULL, NULL, 'installed', '2026-01-01T00:00:00.000000000Z', '2026-01-02T03:04:05.123456789Z', 2, '2026-01-01T00:00:00.000000000Z', '2026-01-02T03:04:05.123456789Z');
INSERT INTO runtimes (id, software, version, line, os, arch, install_mode, artifact_sha256, external_root, observed_sha256, state, installed_at, last_verified_at, revision, created_at, updated_at) VALUES ('0199f000-0000-7000-8000-000000000021', 'postgresql', '16.10', '16', 'macos', 'aarch64', 'external', NULL, '/opt/homebrew/opt/postgresql@16', 'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd', 'drifted', NULL, NULL, 1, '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z');
INSERT INTO schema_migrations (version, name, sha256, applied_at, app_version) VALUES (1, 'registry', '16516d62906535f47e620e6335111ddb400aa61a03789b524effe75eabd72827', '2026-01-01T00:00:00.000000000Z', '0.1.0');
INSERT INTO schema_migrations (version, name, sha256, applied_at, app_version) VALUES (2, 'operation_journal', 'd7446c4b0d51e5b677abae969b7903964d7205d263f1dc61445a15ed35e42059', '2026-01-01T00:00:00.000000000Z', '0.1.0');
INSERT INTO schema_migrations (version, name, sha256, applied_at, app_version) VALUES (3, 'secret_references', '0f900e0b95ba87c25b7d625b28523b6142a286a1cb3b7a2d690db6a26454c114', '2026-01-01T00:00:00.000000000Z', '0.1.0');
INSERT INTO secret_reference_uses (reference_id, resource_kind, resource_id, field) VALUES ('0199f000-0000-7000-8000-000000000060', 'instance', '0199f000-0000-7000-8000-000000000030', 'adminPassword');
INSERT INTO secret_reference_uses (reference_id, resource_kind, resource_id, field) VALUES ('0199f000-0000-7000-8000-000000000061', 'environment', '0199f000-0000-7000-8000-000000000070', 'erp_admin');
INSERT INTO secret_reference_uses (reference_id, resource_kind, resource_id, field) VALUES ('0199f000-0000-7000-8000-000000000062', 'environment', '0199f000-0000-7000-8000-000000000070', 'api_token');
INSERT INTO secret_references (id, reference, platform_store_key, owner_kind, owner_id, purpose, value_origin, status, status_observed_at, value_updated_at, revision, created_at, updated_at) VALUES ('0199f000-0000-7000-8000-000000000060', 'local:instance/0199f000-0000-7000-8000-000000000030/admin-password', 'macos-keychain:login/stackly-secret/instance/0199f000-0000-7000-8000-000000000030/admin-password', 'instance', '0199f000-0000-7000-8000-000000000030', 'admin-password', 'generated', 'present', '2026-01-02T03:04:05.123456789Z', '2026-01-01T00:00:00.000000000Z', 1, '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z');
INSERT INTO secret_references (id, reference, platform_store_key, owner_kind, owner_id, purpose, value_origin, status, status_observed_at, value_updated_at, revision, created_at, updated_at) VALUES ('0199f000-0000-7000-8000-000000000061', 'local:erp/postgres/admin', 'macos-keychain:login/stackly-secret/erp/postgres/admin', 'user', NULL, 'user', 'entered', 'unknown', '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z', 2, '2026-01-01T00:00:00.000000000Z', '2026-01-02T03:04:05.123456789Z');
INSERT INTO secret_references (id, reference, platform_store_key, owner_kind, owner_id, purpose, value_origin, status, status_observed_at, value_updated_at, revision, created_at, updated_at) VALUES ('0199f000-0000-7000-8000-000000000062', 'local:shop/api-token', 'macos-keychain:login/stackly-secret/shop/api-token', 'environment', '0199f000-0000-7000-8000-000000000070', 'api-token', NULL, 'missing', NULL, NULL, 1, '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z');
INSERT INTO settings (key, typed_value, revision, updated_at) VALUES ('retention.operation_days', '30', 1, '2026-01-01T00:00:00.000000000Z');
COMMIT;
