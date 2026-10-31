-- Migration 6: how the main process of an instance last ended (spec 0026
-- "Model data", build step 2; spec 0005 "Tambahan fitur 26").
--
-- One ALTER TABLE ... ADD COLUMN on instances, no table rebuild. Rows
-- written by earlier builds keep NULL, which reads as "never recorded":
-- the migration does not guess how an earlier process ended. The exit
-- handler writes the column in the same commit as the lifecycle event.

ALTER TABLE instances ADD COLUMN last_exit TEXT CHECK (last_exit IN ('clean', 'forced', 'crashed'));
