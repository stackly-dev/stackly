-- Migration 14: environment manifests (spec 0044 "Model data", build step
-- 1).
--
-- environments.services holds the snapshot of the services the manifest
-- declared when it was last accepted: a JSON array of {alias, provider,
-- ownership, selector?, instanceRef?, instanceName?, dependsOn: [{service,
-- condition, timeoutSeconds?}]}, with no settings, endpoints, or secret
-- references. manifest_location and manifest_sha256, which migration 1
-- made, are now written by import.
--
-- The name of an environment becomes unique, and so does a manifest
-- location that is set: the canonical parent folder plus the file name.

ALTER TABLE environments ADD COLUMN services TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(services) AND json_type(services) = 'array');

DROP INDEX environments_name;

CREATE UNIQUE INDEX environments_name_unique ON environments (name);

CREATE UNIQUE INDEX environments_manifest_location ON environments (manifest_location) WHERE manifest_location IS NOT NULL;
