-- Sample data for the frozen schema version 5 fixture: the version 4
-- sample data plus what an install records about an artifact (spec 0018
-- step 6): the four new artifacts columns on the existing row and two
-- compatibility_records rows (one per scope); fixed IDs and times, at
-- least one row in every table. write_registry_fixture runs it after
-- migrations 1 to 5 and the schema_migrations rows.


INSERT INTO registry_info (singleton, registry_id, created_at, created_by_version, generation) VALUES
    (1, '0199f000-0000-7000-8000-000000000001', '2026-01-01T00:00:00.000000000Z', '0.1.0', 14);

INSERT INTO provider_sources (id, kind, location, trust_root, last_sync_at, status, revision, created_at, updated_at) VALUES
    ('0199f000-0000-7000-8000-000000000010', 'builtin', 'stackly:providers', NULL, '2026-01-01T00:00:00.000000000Z', 'trusted', 1, '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z');

INSERT INTO provider_revisions (pk, provider, revision, manifest_sha256, software, schema_version, publisher, capabilities, line_compatibility, source_id, approval, approved_at, created_at) VALUES
    (1, 'official/postgresql', '2026.10.1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'postgresql', 'stackly.provider/v1', 'stackly', '["backup","start","stop"]', '{"16":["16","17"],"17":["17"]}', '0199f000-0000-7000-8000-000000000010', 'approved', '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z'),
    (2, 'official/valkey', '2026.10.1', 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 'valkey', NULL, NULL, '[]', '{}', NULL, NULL, NULL, '2026-01-01T00:00:00.000000000Z');

INSERT INTO artifacts (pk, sha256, software, version, os, arch, minimum_os, source, signature_info, dependency_manifest, provider_revision_pk, catalog_id, build, format, size_bytes, created_at) VALUES
    (1, 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc', 'postgresql', '17.6', 'macos', 'aarch64', '27.0', 'https://example.invalid/postgresql-17.6.tar.zst', '{"team":"EXAMPLE"}', '{"libraries":[]}', 1, 'postgresql-17.6-r2-macos-aarch64', 2, 'tar_gz', 41943040, '2026-01-01T00:00:00.000000000Z');

INSERT INTO compatibility_records (artifact_sha256, position, scope, provider, provider_revision, manifest_sha256, os, os_version, os_build, arch, result, checks, known_limitations, summary, tested_at, evidence) VALUES
    ('cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc', 0, 'provider', 'official/postgresql', '2026.10.1', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'macos', '27.0', '26A428', 'aarch64', 'pass', '["launch_agent","readiness","stop"]', '[]', 'postgresql 17.6 ready in 493 ms', '2026-01-03T04:05:06.123456789Z', 'compatibility/0199f000-0000-7000-8000-000000000090/0199f000-0000-7000-8000-000000000091'),
    ('cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc', 1, 'runtime', NULL, NULL, NULL, 'macos', '26.4', NULL, 'aarch64', 'fail', '["closure"]', '["provider_integration_not_covered"]', NULL, '2026-01-02T03:04:05.123456789Z', 'runtime/0199f000-0000-7000-8000-000000000092/0199f000-0000-7000-8000-000000000093');

INSERT INTO runtimes (id, software, version, line, os, arch, install_mode, artifact_sha256, external_root, observed_sha256, state, installed_at, last_verified_at, revision, created_at, updated_at) VALUES
    ('0199f000-0000-7000-8000-000000000020', 'postgresql', '17.6', '17', 'macos', 'aarch64', 'managed', 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc', NULL, NULL, 'installed', '2026-01-01T00:00:00.000000000Z', '2026-01-02T03:04:05.123456789Z', 2, '2026-01-01T00:00:00.000000000Z', '2026-01-02T03:04:05.123456789Z'),
    ('0199f000-0000-7000-8000-000000000021', 'postgresql', '16.10', '16', 'macos', 'aarch64', 'external', NULL, '/opt/homebrew/opt/postgresql@16', 'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd', 'drifted', NULL, NULL, 1, '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z');

INSERT INTO runtime_dependencies (runtime_id, dependency_runtime_id, version_constraint) VALUES
    ('0199f000-0000-7000-8000-000000000020', '0199f000-0000-7000-8000-000000000021', NULL);

INSERT INTO instances (id, name, display_name, provider_revision_pk, software, runtime_id, data_line, data_version, data_initialized_at, desired, startup, lifecycle, last_failure, config_revision_id, ownership, retained_at, retained_by_operation, created_by_operation, revision, created_at, updated_at) VALUES
    ('0199f000-0000-7000-8000-000000000030', 'orders-db', 'Orders DB', 1, 'postgresql', '0199f000-0000-7000-8000-000000000020', '17', '17.6', '2026-01-01T00:00:00.000000000Z', 'running', 'at_login', 'running', NULL, '0199f000-0000-7000-8000-000000000040', 'created', NULL, NULL, '0199f000-0000-7000-8000-000000000050', 3, '2026-01-01T00:00:00.000000000Z', '2026-01-02T03:04:05.123456789Z'),
    ('0199f000-0000-7000-8000-000000000031', 'orders-db', NULL, 1, 'postgresql', '0199f000-0000-7000-8000-000000000020', '17', '17.6', NULL, 'stopped', 'manual', 'retained', 'start_timeout', NULL, 'created', '2026-01-02T03:04:05.123456789Z', '0199f000-0000-7000-8000-000000000052', '0199f000-0000-7000-8000-000000000051', 4, '2026-01-01T00:00:00.000000000Z', '2026-01-02T03:04:05.123456789Z');

INSERT INTO instance_paths (instance_id, kind, logical_root, relative_path, managed) VALUES
    ('0199f000-0000-7000-8000-000000000030', 'root', 'data', 'instances/0199f000-0000-7000-8000-000000000030', 1),
    ('0199f000-0000-7000-8000-000000000030', 'data', 'data', 'instances/0199f000-0000-7000-8000-000000000030/data', 1),
    ('0199f000-0000-7000-8000-000000000030', 'config', 'data', 'instances/0199f000-0000-7000-8000-000000000030/config', 1),
    ('0199f000-0000-7000-8000-000000000030', 'config_history', 'data', 'instances/0199f000-0000-7000-8000-000000000030/config-history', 1),
    ('0199f000-0000-7000-8000-000000000030', 'temp', 'data', 'instances/0199f000-0000-7000-8000-000000000030/temp', 1),
    ('0199f000-0000-7000-8000-000000000030', 'logs', 'logs', 'instances/0199f000-0000-7000-8000-000000000030', 1),
    ('0199f000-0000-7000-8000-000000000030', 'run', 'runtime', 'i/00000030', 1),
    ('0199f000-0000-7000-8000-000000000031', 'root', 'data', 'retained/0199f000-0000-7000-8000-000000000031', 1),
    ('0199f000-0000-7000-8000-000000000031', 'data', 'data', 'retained/0199f000-0000-7000-8000-000000000031/data', 1),
    ('0199f000-0000-7000-8000-000000000031', 'config', 'data', 'retained/0199f000-0000-7000-8000-000000000031/config', 1),
    ('0199f000-0000-7000-8000-000000000031', 'config_history', 'data', 'retained/0199f000-0000-7000-8000-000000000031/config-history', 1),
    ('0199f000-0000-7000-8000-000000000031', 'temp', 'data', 'retained/0199f000-0000-7000-8000-000000000031/temp', 1),
    ('0199f000-0000-7000-8000-000000000031', 'logs', 'logs', 'instances/0199f000-0000-7000-8000-000000000031', 1),
    ('0199f000-0000-7000-8000-000000000031', 'run', 'runtime', 'i/00000031', 1);

INSERT INTO instance_endpoints (instance_id, position, name, kind, transport, address, port_request, fixed_port, bound_port, socket_name, enabled) VALUES
    ('0199f000-0000-7000-8000-000000000030', 0, 'postgres', 'ip', 'tcp', '127.0.0.1', 'fixed', 5432, 5432, NULL, 1),
    ('0199f000-0000-7000-8000-000000000030', 1, 'socket', 'unix', NULL, NULL, NULL, NULL, NULL, '.s.PGSQL.5432', 1),
    ('0199f000-0000-7000-8000-000000000031', 0, 'postgres', 'ip', 'tcp', '::1', 'auto', NULL, NULL, NULL, 0);

INSERT INTO config_revisions (id, instance_id, config_values, rendered_sha256, previous_id, status, created_by_operation, created_at) VALUES
    ('0199f000-0000-7000-8000-000000000040', '0199f000-0000-7000-8000-000000000030', '{"adminPassword":"local:instance/0199f000-0000-7000-8000-000000000030/admin-password","max_connections":100}', 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', '0199f000-0000-7000-8000-000000000041', 'active', '0199f000-0000-7000-8000-000000000050', '2026-01-02T03:04:05.123456789Z'),
    ('0199f000-0000-7000-8000-000000000041', '0199f000-0000-7000-8000-000000000030', '{"max_connections":50}', NULL, NULL, 'superseded', NULL, '2026-01-01T00:00:00.000000000Z');

INSERT INTO secret_references (id, reference, platform_store_key, owner_kind, owner_id, purpose, value_origin, status, status_observed_at, value_updated_at, revision, created_at, updated_at) VALUES
    ('0199f000-0000-7000-8000-000000000060', 'local:instance/0199f000-0000-7000-8000-000000000030/admin-password', 'macos-keychain:login/stackly-secret/instance/0199f000-0000-7000-8000-000000000030/admin-password', 'instance', '0199f000-0000-7000-8000-000000000030', 'admin-password', 'generated', 'present', '2026-01-02T03:04:05.123456789Z', '2026-01-01T00:00:00.000000000Z', 1, '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z'),
    ('0199f000-0000-7000-8000-000000000061', 'local:erp/postgres/admin', 'macos-keychain:login/stackly-secret/erp/postgres/admin', 'user', NULL, 'user', 'entered', 'unknown', '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z', 2, '2026-01-01T00:00:00.000000000Z', '2026-01-02T03:04:05.123456789Z'),
    ('0199f000-0000-7000-8000-000000000062', 'local:shop/api-token', 'macos-keychain:login/stackly-secret/shop/api-token', 'environment', '0199f000-0000-7000-8000-000000000070', 'api-token', NULL, 'missing', NULL, NULL, 1, '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z');

INSERT INTO environments (id, name, manifest_location, manifest_sha256, lock_sha256, last_operation_id, revision, created_at, updated_at) VALUES
    ('0199f000-0000-7000-8000-000000000070', 'shop', '/Users/example/shop/stackly.yaml', 'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd', 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', '0199f000-0000-7000-8000-000000000053', 2, '2026-01-01T00:00:00.000000000Z', '2026-01-02T03:04:05.123456789Z');

INSERT INTO environment_instances (environment_id, position, alias, instance_id, ownership, dependency_conditions) VALUES
    ('0199f000-0000-7000-8000-000000000070', 0, 'db', '0199f000-0000-7000-8000-000000000030', 'dedicated', '[]');

INSERT INTO secret_reference_uses (reference_id, resource_kind, resource_id, field) VALUES
    ('0199f000-0000-7000-8000-000000000060', 'instance', '0199f000-0000-7000-8000-000000000030', 'adminPassword'),
    ('0199f000-0000-7000-8000-000000000061', 'environment', '0199f000-0000-7000-8000-000000000070', 'erp_admin'),
    ('0199f000-0000-7000-8000-000000000062', 'environment', '0199f000-0000-7000-8000-000000000070', 'api_token');

INSERT INTO instance_demands (id, instance_id, source_kind, source_id, session_id, state, state_changed_at, acquired_at) VALUES
    ('0199f000-0000-7000-8000-000000000080', '0199f000-0000-7000-8000-000000000030', 'environment', '0199f000-0000-7000-8000-000000000070', NULL, 'active', NULL, '2026-01-01T00:00:00.000000000Z'),
    ('0199f000-0000-7000-8000-000000000081', '0199f000-0000-7000-8000-000000000030', 'startup', NULL, 'login-1', 'suppressed', '2026-01-02T03:04:05.123456789Z', '2026-01-01T00:00:00.000000000Z'),
    ('0199f000-0000-7000-8000-000000000082', '0199f000-0000-7000-8000-000000000030', 'manual', NULL, NULL, 'released', '2026-01-02T03:04:05.123456789Z', '2026-01-01T00:00:00.000000000Z');

INSERT INTO operations (id, kind, resource_kind, resource_id, idempotency_key, state, stage, progress_done, progress_total, failure_code, failure_retryable, recovery_hint, started_at, revision, created_at, updated_at) VALUES
    ('0199f000-0000-7000-8000-000000000050', 'create_instance', 'instance', '0199f000-0000-7000-8000-000000000030', 'cli-create-1', 'succeeded', NULL, 5, 5, NULL, NULL, NULL, '2026-01-01T00:00:00.000000000Z', 4, '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z'),
    ('0199f000-0000-7000-8000-000000000051', 'create_instance', 'instance', '0199f000-0000-7000-8000-000000000031', NULL, 'succeeded', NULL, NULL, NULL, NULL, NULL, NULL, NULL, 2, '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z'),
    ('0199f000-0000-7000-8000-000000000052', 'retain_instance', 'instance', '0199f000-0000-7000-8000-000000000031', NULL, 'needs_recovery', 'moving', 1, 3, NULL, NULL, 'resume_retain', '2026-01-02T03:04:05.123456789Z', 3, '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z'),
    ('0199f000-0000-7000-8000-000000000053', 'environment_up', 'environment', '0199f000-0000-7000-8000-000000000070', NULL, 'running', 'starting', NULL, NULL, NULL, NULL, NULL, '2026-01-02T03:04:05.123456789Z', 2, '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z'),
    ('0199f000-0000-7000-8000-000000000054', 'install_runtime', 'runtime', '0199f000-0000-7000-8000-000000000020', NULL, 'failed', 'verifying', NULL, NULL, 'checksum_mismatch', 0, NULL, '2026-01-01T00:00:00.000000000Z', 3, '2026-01-01T00:00:00.000000000Z', '2026-01-01T00:00:00.000000000Z'),
    ('0199f000-0000-7000-8000-000000000055', 'doctor', 'manager', NULL, NULL, 'queued', NULL, NULL, NULL, NULL, NULL, NULL, NULL, 1, '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z'),
    ('0199f000-0000-7000-8000-000000000056', 'create_instance', 'instance', '0199f000-0000-7000-8000-000000000031', 'cli-create-2', 'failed', 'validating', NULL, NULL, 'invalid_settings', 0, NULL, '2026-01-02T03:04:05.123456789Z', 3, '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z');

INSERT INTO process_observations (id, instance_id, pid, start_identity, executable_identity, boot_id, session_id, observed_at) VALUES
    (1, '0199f000-0000-7000-8000-000000000030', 4242, '1700000000.000000', '/Users/example/Library/Application Support/Stackly/runtimes/postgresql/17.6/macos-aarch64/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc/bin/postgres', '6f1d2c3b-4a59-4e8f-9a0b-1c2d3e4f5a6b', '4242', '2026-01-02T03:04:05.123456789Z');

INSERT INTO backups (id, instance_id, runtime_id, runtime_identity, method, scope, consistency, path, checksum_manifest, created_at, verified_at, restore_verified_at) VALUES
    ('0199f000-0000-7000-8000-000000000090', '0199f000-0000-7000-8000-000000000030', '0199f000-0000-7000-8000-000000000020', 'postgresql 17.6 macos-aarch64', 'logical_dump', 'instance', 'consistent', '/Users/example/Backups/orders-db.dump', '{"orders-db.dump":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"}', '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z', NULL);

INSERT INTO audit_events (id, actor, action, resource_kind, resource_id, operation_id, result, safe_details, occurred_at) VALUES
    ('0199f000-0000-7000-8000-0000000000a0', 'user', 'secret.set', 'secret', '0199f000-0000-7000-8000-000000000061', NULL, 'succeeded', '{"clientKind":"cli","peerUid":"501","reference":"local:erp/postgres/admin"}', '2026-01-02T03:04:05.123456789Z'),
    ('0199f000-0000-7000-8000-0000000000a1', 'daemon', 'secret.set', 'secret', '0199f000-0000-7000-8000-000000000060', '0199f000-0000-7000-8000-000000000050', 'succeeded', '{"reference":"local:instance/0199f000-0000-7000-8000-000000000030/admin-password"}', '2026-01-01T00:00:00.000000000Z'),
    ('0199f000-0000-7000-8000-0000000000a2', 'user', 'secret.reveal', NULL, NULL, NULL, 'failed', '{"clientKind":"gui","errorCode":"secret.not_found","purpose":"show","reference":"local:gone/admin"}', '2026-01-02T03:04:05.123456789Z');

INSERT INTO settings (key, typed_value, revision, updated_at) VALUES
    ('retention.operation_days', '30', 1, '2026-01-01T00:00:00.000000000Z');

INSERT INTO operation_journal (operation_id, method, request_fingerprint, trigger_origin, trigger_client, trigger_interactive, resumable, input, expected_revision, waiting_on, point_of_no_return_at, cancel_requested_at, compensation_started_at, recovery_cause, failure_error_code, failure_message, failure_details, failure_items, failure_rule, result) VALUES
    ('0199f000-0000-7000-8000-000000000050', 'instance.create', 'abababababababababababababababababababababababababababababababab', 'user', 'cli', 1, 0, '{"name":"orders-db"}', NULL, NULL, '2026-01-01T00:00:00.000000000Z', NULL, NULL, NULL, NULL, NULL, '[]', '[]', NULL, '{"resources":[]}'),
    ('0199f000-0000-7000-8000-000000000051', NULL, NULL, 'background', NULL, 0, 0, '{}', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, '[]', '[]', NULL, NULL),
    ('0199f000-0000-7000-8000-000000000052', 'instance.remove', 'cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd', 'user', 'gui', 1, 0, '{}', 3, NULL, '2026-01-02T03:04:05.123456789Z', NULL, '2026-01-02T03:04:05.123456789Z', 'compensation_failed', 'operation.compensation_failed', 'undoing the instance folders failed', '[{"key":"item","value":"instance_tree"},{"key":"detail","value":"permission_denied"}]', '[]', NULL, NULL),
    ('0199f000-0000-7000-8000-000000000053', NULL, NULL, 'background', NULL, 0, 1, '{"environment":"shop"}', NULL, '0199f000-0000-7000-8000-000000000052', NULL, '2026-01-02T03:04:05.123456789Z', NULL, NULL, NULL, NULL, '[]', '[]', NULL, NULL),
    ('0199f000-0000-7000-8000-000000000054', 'runtime.install', 'efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef', 'user', 'cli', 0, 1, '{"version":"17.6"}', NULL, NULL, NULL, NULL, '2026-01-01T00:00:00.000000000Z', NULL, 'runtime.checksum_mismatch', 'the artifact digest does not match', '[]', '[]', NULL, NULL),
    ('0199f000-0000-7000-8000-000000000055', NULL, NULL, 'background', NULL, 0, 0, '{}', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, '[]', '[]', NULL, NULL),
    ('0199f000-0000-7000-8000-000000000056', 'instance.create', 'a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1', 'user', 'cli', 0, 0, '{"name":"cache"}', NULL, NULL, NULL, NULL, '2026-01-02T03:04:05.123456789Z', NULL, 'provider.invalid_settings', 'the settings are not valid', '[{"key":"provider","value":"official/postgresql"}]', '[{"kind":"issue","fields":[{"key":"path","value":"/port"},{"key":"detail","value":"omitted"}]},{"kind":"issue","fields":[]}]', 'BR-002', NULL);

INSERT INTO operation_steps (operation_id, seq, name, repeatable, reversible, state, attempts, checkpoint, error_code, started_at, finished_at) VALUES
    ('0199f000-0000-7000-8000-000000000052', 1, 'move', 0, 1, 'done', 1, '{"moved":true}', NULL, '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z'),
    ('0199f000-0000-7000-8000-000000000052', 2, 'rebind', 1, 0, 'failed', 2, NULL, 'instance.rebind_failed', '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z'),
    ('0199f000-0000-7000-8000-000000000053', 1, 'start_members', 1, 1, 'running', 1, NULL, NULL, '2026-01-02T03:04:05.123456789Z', NULL);

INSERT INTO operation_created (operation_id, seq, step_seq, item_kind, item_ref, payload, state, detail, recorded_at, settled_at) VALUES
    ('0199f000-0000-7000-8000-000000000052', 1, 1, 'instance_tree', '0199f000-0000-7000-8000-000000000031', NULL, 'compensation_failed', 'permission_denied', '2026-01-02T03:04:05.123456789Z', '2026-01-02T03:04:05.123456789Z'),
    ('0199f000-0000-7000-8000-000000000053', 1, 1, 'custom', 'supervisor_spawned_process:pid:4242', '{"pid":4242}', 'intended', NULL, '2026-01-02T03:04:05.123456789Z', NULL);

INSERT INTO operation_locks (operation_id, lock_key) VALUES
    ('0199f000-0000-7000-8000-000000000052', 'instance:0199f000-0000-7000-8000-000000000031'),
    ('0199f000-0000-7000-8000-000000000053', 'environment:0199f000-0000-7000-8000-000000000070'),
    ('0199f000-0000-7000-8000-000000000055', 'manager:self');
