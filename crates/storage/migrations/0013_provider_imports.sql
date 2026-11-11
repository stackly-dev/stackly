-- Migration 13: imported local provider packages and the trust decision on
-- them (spec 0037 "Model data", build step 1).
--
-- One new table, no change to an existing one. provider_sources gains rows
-- of kind 'local' (an open code set, so no CHECK changes), and
-- provider_revisions.approval and approved_at, which migration 1 already
-- made and its immutable trigger already leaves writable, now hold the
-- decision of a revision pinned from an approved import.
--
-- provider_imports: one row per imported provider ID, the key. source_id is
-- always the one 'local' source row. package_sha256 names the store folder
-- provider-imports/<package_sha256>/ and is unique, because the digest
-- covers metadata.id. decision is an open code set ('approved', 'revoked')
-- and is NULL until the first decision; the five decided_* columns are all
-- set or all NULL. quarantined is NULL when the attribute could not be
-- read.

CREATE TABLE provider_imports (
    provider TEXT PRIMARY KEY NOT NULL CHECK (length(provider) BETWEEN 3 AND 127 AND provider GLOB '[a-z]*/[a-z]*' AND provider NOT GLOB '*/*/*' AND provider NOT GLOB '*[^a-z0-9/-]*'),
    source_id TEXT NOT NULL CHECK (length(source_id) = 36 AND source_id NOT GLOB '*[^0-9a-f-]*' AND substr(source_id,9,1) = '-' AND substr(source_id,14,1) = '-' AND substr(source_id,19,1) = '-' AND substr(source_id,24,1) = '-' AND substr(source_id,15,1) = '7') REFERENCES provider_sources (id) ON DELETE RESTRICT,
    provider_revision TEXT NOT NULL CHECK (length(provider_revision) BETWEEN 1 AND 64 AND provider_revision GLOB '[0-9A-Za-z]*' AND provider_revision NOT GLOB '*[^0-9A-Za-z.+_-]*' AND provider_revision NOT GLOB '*..*'),
    software TEXT NOT NULL CHECK (length(software) BETWEEN 1 AND 63 AND software GLOB '[a-z]*' AND software NOT GLOB '*[^a-z0-9-]*' AND software NOT GLOB '*--*' AND software NOT GLOB '*-'),
    manifest_sha256 TEXT NOT NULL CHECK (length(manifest_sha256) = 64 AND manifest_sha256 NOT GLOB '*[^0-9a-f]*'),
    versions_sha256 TEXT NOT NULL CHECK (length(versions_sha256) = 64 AND versions_sha256 NOT GLOB '*[^0-9a-f]*'),
    package_sha256 TEXT NOT NULL UNIQUE CHECK (length(package_sha256) = 64 AND package_sha256 NOT GLOB '*[^0-9a-f]*'),
    package_bytes INTEGER NOT NULL CHECK (package_bytes BETWEEN 1 AND 4194304),
    origin_path TEXT NOT NULL CHECK (length(CAST(origin_path AS BLOB)) BETWEEN 1 AND 1024),
    quarantined INTEGER CHECK (quarantined IN (0, 1)),
    quarantine_agent TEXT CHECK (length(CAST(quarantine_agent AS BLOB)) BETWEEN 1 AND 128),
    imported_at TEXT NOT NULL CHECK (length(imported_at) = 30 AND imported_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    decision TEXT CHECK (length(decision) BETWEEN 1 AND 64 AND decision GLOB '[a-z]*' AND decision NOT GLOB '*[^a-z0-9_]*'),
    decided_package_sha256 TEXT CHECK (length(decided_package_sha256) = 64 AND decided_package_sha256 NOT GLOB '*[^0-9a-f]*'),
    decided_revision TEXT CHECK (length(decided_revision) BETWEEN 1 AND 64 AND decided_revision GLOB '[0-9A-Za-z]*' AND decided_revision NOT GLOB '*[^0-9A-Za-z.+_-]*' AND decided_revision NOT GLOB '*..*'),
    decided_permissions TEXT CHECK (json_valid(decided_permissions) AND json_type(decided_permissions) = 'object'),
    decided_at TEXT CHECK (length(decided_at) = 30 AND decided_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) = 30 AND created_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    updated_at TEXT NOT NULL CHECK (length(updated_at) = 30 AND updated_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    CHECK ((decision IS NULL) = (decided_package_sha256 IS NULL)
        AND (decision IS NULL) = (decided_revision IS NULL)
        AND (decision IS NULL) = (decided_permissions IS NULL)
        AND (decision IS NULL) = (decided_at IS NULL))
) STRICT;

CREATE INDEX provider_imports_source_id ON provider_imports (source_id);
