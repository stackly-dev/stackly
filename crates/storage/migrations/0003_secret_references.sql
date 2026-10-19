-- Migration 3: the secret references of spec 0011 ("Model data") and
-- spec 0005 ("Tambahan fitur 11").
--
-- Version 1 shipped secret_references with columns that predate spec
-- 0011 (store_key, scope, purpose, status). No production code ever wrote
-- that table, only test seeding, so it is dropped and created again with
-- the spec 0011 columns, without copying rows. Then secret_reference_uses
-- records which resource needs which reference for which field.
--
-- BR-018: no column holds a secret value. A row names the value through
-- its reference text and the platform store key.
--
-- Every CHECK for a column kind is the exact expression from the "Aturan
-- kolom bersama" table of spec 0005, with the column name filled in.

DROP TABLE secret_references;

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

CREATE INDEX secret_references_owner ON secret_references (owner_kind, owner_id);

-- The owner is set when the row is born. The one move spec 0011 allows is
-- to the user, when the owner is removed while another resource still
-- uses the reference.
CREATE TRIGGER secret_references_owner_fixed
BEFORE UPDATE OF owner_kind, owner_id ON secret_references
WHEN (NEW.owner_kind IS NOT OLD.owner_kind OR NEW.owner_id IS NOT OLD.owner_id)
    AND NOT (NEW.owner_kind = 'user' AND NEW.owner_id IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'the owner of a secret reference is fixed');
END;

-- A use is written in the same batch as the config revision or the
-- environment that needs it. resource_id has no foreign key, because the
-- resource is polymorphic: removing an instance or environment deletes
-- its uses in the same batch.
CREATE TABLE secret_reference_uses (
    reference_id TEXT NOT NULL CHECK (length(reference_id) = 36 AND reference_id NOT GLOB '*[^0-9a-f-]*' AND substr(reference_id,9,1) = '-' AND substr(reference_id,14,1) = '-' AND substr(reference_id,19,1) = '-' AND substr(reference_id,24,1) = '-' AND substr(reference_id,15,1) = '7') REFERENCES secret_references (id) ON DELETE RESTRICT,
    resource_kind TEXT NOT NULL CHECK (resource_kind IN ('instance', 'environment')),
    resource_id TEXT NOT NULL CHECK (length(resource_id) = 36 AND resource_id NOT GLOB '*[^0-9a-f-]*' AND substr(resource_id,9,1) = '-' AND substr(resource_id,14,1) = '-' AND substr(resource_id,19,1) = '-' AND substr(resource_id,24,1) = '-' AND substr(resource_id,15,1) = '7'),
    field TEXT NOT NULL CHECK (length(field) BETWEEN 1 AND 64 AND field GLOB '[a-z]*' AND field NOT GLOB '*[^A-Za-z0-9_-]*'),
    PRIMARY KEY (reference_id, resource_kind, resource_id, field)
) STRICT;

CREATE INDEX secret_reference_uses_resource ON secret_reference_uses (resource_kind, resource_id);
