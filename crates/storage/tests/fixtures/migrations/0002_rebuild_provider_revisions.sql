-- Test migration 2: adds a column to runtimes, then rebuilds
-- provider_revisions with SQLite's 12 step procedure (the runner turns off
-- foreign keys, opens the transaction, and checks foreign keys after).
-- provider_revisions has a trigger, an index, and is the parent of foreign
-- keys, so all of them must survive.

ALTER TABLE runtimes ADD COLUMN notes TEXT;

CREATE TABLE provider_revisions_new (
    pk INTEGER PRIMARY KEY NOT NULL,
    provider TEXT NOT NULL CHECK (length(provider) BETWEEN 3 AND 127 AND provider GLOB '[a-z]*/[a-z]*' AND provider NOT GLOB '*/*/*' AND provider NOT GLOB '*[^a-z0-9/-]*'),
    revision TEXT NOT NULL CHECK (length(revision) BETWEEN 1 AND 64 AND revision GLOB '[0-9A-Za-z]*' AND revision NOT GLOB '*[^0-9A-Za-z.+_-]*' AND revision NOT GLOB '*..*'),
    manifest_sha256 TEXT NOT NULL CHECK (length(manifest_sha256) = 64 AND manifest_sha256 NOT GLOB '*[^0-9a-f]*'),
    software TEXT NOT NULL CHECK (length(software) BETWEEN 1 AND 63 AND software GLOB '[a-z]*' AND software NOT GLOB '*[^a-z0-9-]*' AND software NOT GLOB '*--*' AND software NOT GLOB '*-'),
    schema_version TEXT,
    publisher TEXT,
    homepage TEXT,
    capabilities TEXT NOT NULL CHECK (json_valid(capabilities) AND json_type(capabilities) = 'array'),
    line_compatibility TEXT NOT NULL CHECK (json_valid(line_compatibility) AND json_type(line_compatibility) = 'object'),
    source_id TEXT CHECK (length(source_id) = 36 AND source_id NOT GLOB '*[^0-9a-f-]*' AND substr(source_id,9,1) = '-' AND substr(source_id,14,1) = '-' AND substr(source_id,19,1) = '-' AND substr(source_id,24,1) = '-' AND substr(source_id,15,1) = '7') REFERENCES provider_sources (id) ON DELETE RESTRICT,
    approval TEXT CHECK (length(approval) BETWEEN 1 AND 64 AND approval GLOB '[a-z]*' AND approval NOT GLOB '*[^a-z0-9_]*'),
    approved_at TEXT CHECK (length(approved_at) = 30 AND approved_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    created_at TEXT NOT NULL CHECK (length(created_at) = 30 AND created_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    UNIQUE (provider, revision, manifest_sha256)
) STRICT;

INSERT INTO provider_revisions_new (pk, provider, revision, manifest_sha256, software, schema_version, publisher, capabilities, line_compatibility, source_id, approval, approved_at, created_at)
SELECT pk, provider, revision, manifest_sha256, software, schema_version, publisher, capabilities, line_compatibility, source_id, approval, approved_at, created_at
FROM provider_revisions;

DROP TABLE provider_revisions;

ALTER TABLE provider_revisions_new RENAME TO provider_revisions;

CREATE INDEX provider_revisions_source_id ON provider_revisions (source_id);

CREATE TRIGGER provider_revisions_immutable
BEFORE UPDATE OF pk, provider, revision, manifest_sha256, software, schema_version, publisher, homepage, capabilities, line_compatibility, source_id, created_at ON provider_revisions
BEGIN
    SELECT RAISE(ABORT, 'provider revision is immutable');
END;
