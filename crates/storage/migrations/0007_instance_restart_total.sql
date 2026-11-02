-- Migration 7: how many spawns of an instance followed a crash since it was
-- created (spec 0028 "Model data", build step 2; spec 0005 "Tambahan fitur
-- 28").
--
-- One ALTER TABLE ... ADD COLUMN on instances, no table rebuild. Rows
-- written by earlier builds read 0: the migration does not guess the
-- restarts that happened before it. The supervisor raises the column in
-- the same commit as the spawned event that followed a crash.

ALTER TABLE instances ADD COLUMN restart_total INTEGER NOT NULL DEFAULT 0 CHECK (restart_total >= 0);
