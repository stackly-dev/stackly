-- Migration 7: numbered config revisions with an endpoint snapshot, an
-- origin, and ready_at, the last known good marker (spec 0029 "Model data",
-- build step 2; spec 0005 "fitur 29 melengkapi").
--
-- ALTER TABLE ... ADD COLUMN only, no table rebuild. Existing rows are
-- numbered per instance from 1 in ID order (UUID v7, so creation order),
-- with the active revision last so it always holds the highest number;
-- every other row becomes superseded. Each row is origin created with
-- ready_at NULL: the migration does not guess which revision was ever
-- ready. The endpoint snapshot is copied from the instance's IP endpoints;
-- an instance without one gets {}.

ALTER TABLE config_revisions ADD COLUMN number INTEGER NOT NULL DEFAULT 1 CHECK (number >= 1);
ALTER TABLE config_revisions ADD COLUMN endpoints TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(endpoints) AND json_type(endpoints) = 'object' AND substr(endpoints, 1, 1) = '{');
ALTER TABLE config_revisions ADD COLUMN ready_at TEXT CHECK (length(ready_at) = 30 AND ready_at GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z');
ALTER TABLE config_revisions ADD COLUMN origin TEXT NOT NULL DEFAULT 'created' CHECK (length(origin) BETWEEN 1 AND 64 AND origin GLOB '[a-z]*' AND origin NOT GLOB '*[^a-z0-9_]*');
ALTER TABLE config_revisions ADD COLUMN source_number INTEGER CHECK (source_number >= 1);

UPDATE config_revisions SET number = ordered.n
FROM (
    SELECT c.id AS id, row_number() OVER (
        PARTITION BY c.instance_id
        ORDER BY (c.id IS i.config_revision_id), c.id
    ) AS n
    FROM config_revisions AS c JOIN instances AS i ON i.id = c.instance_id
) AS ordered
WHERE config_revisions.id = ordered.id;

UPDATE config_revisions SET status = 'superseded'
WHERE id NOT IN (SELECT config_revision_id FROM instances WHERE config_revision_id IS NOT NULL);

UPDATE config_revisions SET endpoints = (
    SELECT json_group_object(
        e.name,
        CASE WHEN e.port_request = 'fixed'
            THEN json_object('type', 'fixed', 'content', e.fixed_port)
            ELSE json_object('type', 'auto')
        END
    )
    FROM instance_endpoints AS e
    WHERE e.instance_id = config_revisions.instance_id AND e.kind = 'ip'
);

CREATE UNIQUE INDEX config_revisions_instance_number ON config_revisions (instance_id, number);
