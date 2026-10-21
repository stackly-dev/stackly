-- Migration 5: what an install records about an artifact (spec 0018 "Data
-- model", build step 6; spec 0005 "Tambahan fitur 18").
--
-- Four ALTER TABLE ... ADD COLUMN on artifacts, no table rebuild. Rows
-- written by earlier builds keep NULL in all four, which is correct: those
-- builds never knew the catalog entry, the build number, the format, or the
-- download size. signature_info, dependency_manifest, minimum_os, and
-- source already exist; this feature starts filling them.
--
-- compatibility_records holds the records copied from the catalog when the
-- artifact was installed, in catalog order. The provider columns are set
-- exactly for the 'provider' scope. SQLite cannot check every element of a
-- JSON array inside a CHECK, so checks and known_limitations are checked
-- as arrays here and element by element in the domain.
--
-- Every CHECK for a column kind is the exact expression from the "Aturan
-- kolom bersama" table of spec 0005, with the column name filled in.

ALTER TABLE artifacts ADD COLUMN catalog_id TEXT CHECK (length(catalog_id) BETWEEN 1 AND 128 AND catalog_id NOT GLOB '*[^a-z0-9.-]*');

ALTER TABLE artifacts ADD COLUMN build INTEGER CHECK (build >= 1);

ALTER TABLE artifacts ADD COLUMN format TEXT CHECK (format IN ('tar_gz', 'zip', 'binary'));

ALTER TABLE artifacts ADD COLUMN size_bytes INTEGER CHECK (size_bytes >= 1);

CREATE TABLE compatibility_records (
    artifact_sha256 TEXT NOT NULL CHECK (length(artifact_sha256) = 64 AND artifact_sha256 NOT GLOB '*[^0-9a-f]*') REFERENCES artifacts (sha256) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    scope TEXT NOT NULL CHECK (scope IN ('runtime', 'provider')),
    provider TEXT CHECK (length(provider) BETWEEN 3 AND 127 AND provider GLOB '[a-z]*/[a-z]*' AND provider NOT GLOB '*/*/*' AND provider NOT GLOB '*[^a-z0-9/-]*'),
    provider_revision TEXT CHECK (length(provider_revision) BETWEEN 1 AND 64 AND provider_revision GLOB '[0-9A-Za-z]*' AND provider_revision NOT GLOB '*[^0-9A-Za-z.+_-]*' AND provider_revision NOT GLOB '*..*'),
    manifest_sha256 TEXT CHECK (length(manifest_sha256) = 64 AND manifest_sha256 NOT GLOB '*[^0-9a-f]*'),
    os TEXT NOT NULL CHECK (os IN ('macos', 'windows', 'linux')),
    os_version TEXT NOT NULL CHECK (length(os_version) BETWEEN 1 AND 16 AND os_version NOT GLOB '*[^0-9.]*'),
    os_build TEXT CHECK (length(os_build) BETWEEN 1 AND 32 AND os_build NOT GLOB '*[^0-9A-Za-z]*'),
    arch TEXT NOT NULL CHECK (arch IN ('aarch64', 'x86_64')),
    result TEXT NOT NULL CHECK (result IN ('pass', 'fail')),
    checks TEXT NOT NULL CHECK (json_valid(checks) AND json_type(checks) = 'array'),
    known_limitations TEXT NOT NULL CHECK (json_valid(known_limitations) AND json_type(known_limitations) = 'array'),
    summary TEXT CHECK (length(summary) BETWEEN 1 AND 256),
    tested_at TEXT NOT NULL CHECK (length(tested_at) = 30 AND tested_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'),
    evidence TEXT NOT NULL CHECK (length(evidence) BETWEEN 1 AND 256 AND evidence GLOB '[a-z]*/*' AND evidence NOT GLOB '*..*'),
    PRIMARY KEY (artifact_sha256, position),
    CHECK ((scope = 'provider' AND provider IS NOT NULL AND provider_revision IS NOT NULL AND manifest_sha256 IS NOT NULL) OR (scope = 'runtime' AND provider IS NULL AND provider_revision IS NULL AND manifest_sha256 IS NULL))
) STRICT;
