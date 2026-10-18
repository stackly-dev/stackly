# Verify: Data model registry · spec 0005 · updated 2026-10-18

_Langkah diturunkan dari acceptance criteria spec 0005. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. `$ROOT` adalah folder baru bermode 0700 dengan path kanonik (tanpa symlink, misalnya di bawah `/private/tmp`), tidak pernah folder Application Support asli. Sejak fitur 6 milestone 3, `stacklyd` tetap hidup sampai SIGTERM, jadi setiap langkah daemon menunggu baris `registry_state` di stderr lalu mengirim `kill -TERM`. Langkah `sqlite3` yang mengubah data selalu memakai salinan file registry._

## UI / manual

Tidak ada. Registry tidak punya UI; GUI dan CLI tidak pernah membuka file SQLite (BR-011).

## Commands

### Daemon dan file registry

- [x] `cargo build -p stacklyd`, lalu `STACKLY_ROOT=$ROOT ./target/debug/stacklyd 2> run1.log &` dan SIGTERM setelah baris `registry_state` → log berisi `created a new registry`, `registry open` (`created` true, `schema_version` 1, `sqlite_version` 3.53.2), lalu `registry ready` dengan `registry_state` `ready`, `pruned_rows` 0, `schema_version` 1; setelah SIGTERM `stacklyd stopped` dan exit 0 → AC-3, AC-7, AC-14
- [x] Selama daemon pertama hidup: `stat -f "%Sp" $ROOT/data/registry/stackly.sqlite*` dan ketiga folder registry → `stackly.sqlite`, `-wal`, `-shm` bermode `-rw-------`; `registry`, `pre-migration`, `recovery` bermode `drwx------` → AC-3
- [x] Pada salinan `stackly.sqlite`: `sqlite3 copy.sqlite "PRAGMA application_id; PRAGMA user_version; PRAGMA journal_mode"` → `1398033241` (hex `53544b59`), `1`, `wal`; 20 tabel di `sqlite_schema` dan 20 di antaranya `STRICT`; `pragma_foreign_key_check` kosong; `registry_info` satu baris dengan `registry_id` 36 karakter, `created_at` UTC 30 karakter, versi app, generation 0; `schema_migrations` satu baris versi 1 `registry` dengan sha256 64 karakter → AC-1, AC-2, AC-3
- [x] Pada salinan dengan `PRAGMA foreign_keys=ON`: insert waktu `2026-10-18T05:42:00+07:00` dan `2026-10-18T05:42:00.000Z` → keduanya `CHECK constraint failed`; `UPDATE provider_revisions SET capabilities` → `provider revision is immutable`, sedangkan `UPDATE ... SET approval = 'approved', approved_at = ...` berhasil; lifecycle `paused` → `CHECK constraint failed`; `runtime_dependencies` ke runtime yang tidak ada → `FOREIGN KEY constraint failed`; `UPDATE audit_events` → `audit events are append only` → AC-2, AC-13, AC-15
- [x] Pada salinan: `pragma_table_info('secret_references')` → persis `id, store_key, scope, purpose, status, revision, created_at, updated_at` → AC-12
- [x] Tambal 4 byte di offset 60 file registry asli menjadi 999 (big endian), catat sha256, jalankan `stacklyd` lagi → baris `registry_state` `recovery`, `reason` `schema_newer_than_app`, `report` menunjuk `registry/recovery/<waktu>.json` (schema `stackly.registry-recovery/v1`, `appSchemaVersion` 1, `dbSchemaVersion` 999); daemon tetap hidup sampai SIGTERM lalu exit 0; sha256 `stackly.sqlite` sebelum dan sesudah sama → AC-6, AC-7
- [x] Root baru: start sekali, hentikan, ganti `stackly.sqlite` dengan teks biasa (mode 0600) dan hapus `-wal`/`-shm`, start lagi → `recovery` dengan reason `not_a_registry`, sha256 file tidak berubah → AC-6
- [x] Root baru: start sekali, hentikan, hapus `stackly.sqlite`, `-wal`, `-shm`, buat `data/runtimes/leftover/`, start lagi → `recovery` dengan reason `registry_missing`, dan tidak ada registry baru yang dibuat → AC-6

### Registry lewat API publik

- [x] `cargo test -p stackly-storage --test registry` → 40 test lulus: commit (urutan cek, CAS, generation, error bertipe, `Integrity` di op-nya), pemetaan (setiap aggregate dan setiap kode state bolak balik, JSON terurut, waktu sampai nanodetik, path bukan UTF-8, `Corrupt`), query (`known_instance_ids`, `runtime_references`, `instance_paths`, halaman operasi), prune per 5.000 baris, dan suite `registry_conformance` atas `SqliteRegistry` yang di-seed lewat koneksi kedua → AC-8, AC-9, AC-10, AC-11, AC-12, AC-13, AC-14, AC-15, AC-16
- [x] `cargo test -p stackly-storage --test registry conformance::registry_never_holds_secret_values` → suite berjalan sambil test memegang `FixtureCredential`, lalu `assert_absent` atas byte `stackly.sqlite` dan `-wal` yang dibaca sebelum registry ditutup lulus → AC-12
- [x] `cargo test -p stackly-storage --test registry commit::concurrent_creates_produce_one_instance` → tiga balapan 16 thread (nama sama, artefak sama, revision sama) masing masing tepat satu sukses, sisanya `NameTaken`, `DuplicateRuntime`, `RevisionConflict` → AC-9
- [x] `cargo test -p stackly-storage --test registry query::stored_paths_resolve_to_the_layout_and_never_repeat` → 1000 instance dalam satu batch, 7000 path tersimpan, semuanya resolve ke path layout instance-nya, tidak ada yang sama dengan atau berada di bawah path instance lain → AC-10
- [x] `cargo test -p stackly-storage --test open` → 36 test lulus: create, pragma koneksi tulis dibaca ulang, create bersamaan dan sisa file sementara, migrasi dari fixture v1 dengan snapshot, migrasi gagal di tengah, setiap reason recovery AC-6 tanpa mengubah sha256, fixture beku, dan sidik jari → AC-1, AC-2, AC-3, AC-4, AC-5, AC-6, AC-13, AC-15
- [x] `cargo test -p stacklyd --test root_override` → baris `registry_state` `ready` memuat `pruned_rows` 0, dan registry yang lebih baru dari app tidak disentuh → AC-7
- [x] Program scratch di luar repo (tidak di-commit) yang memakai `open_registry` dengan `MacosPlatform` dan jam asli, dijalankan 2026-10-18 → CAS dan generation (revision lama gagal di op 1 dan tidak ada yang diterapkan; generation lama gagal untuk seluruh batch), nama retained bisa dipakai lagi, short key ganda ditolak, tujuh path resolve ke layout, instance terbaca dengan readiness dan health `unknown`/`daemon_restarted` pada `opened_at`, referensi runtime memuat instance `stopped` dan retained serta operasi `running`, `DELETE` mentah ditolak foreign key, waktu `2026-01-01T00:00:00.123456789Z` tersimpan utuh, prune menghapus operasi selesai berumur 45 hari dan menyimpan `needs_recovery` tanpa menaikkan generation, balapan 16 thread memberi 1 sukses dan 15 `NameTaken`, dan setelah ditutup lalu dibuka lagi generation serta keempat instance tetap ada → AC-8, AC-9, AC-10, AC-11, AC-13, AC-14, AC-16
- [x] Program scratch yang sama dengan `OpenOptions::with_migrations`: fixture `registry-v1.sql` ditambah migrasi test `0002_rebuild_provider_revisions.sql` → `migrated_from` 1, schema 2, snapshot `pre-migration/v1-to-v2-<waktu>.sqlite` ada, setiap baris tetap; open kedua tanpa migrasi dan tanpa snapshot baru. Dengan `0002_fails_midway.sql` → `migration_failed { from: 1, to: 2, failed_version: 2 }`, snapshot dan satu laporan ada, jumlah baris tiap tabel dan `user_version` 1 tetap, open kedua recovery lagi tanpa laporan baru → AC-4, AC-5
- [x] Program scratch yang sama setelah `UPDATE operations SET kind = 'teleport_instance'` pada satu baris → `operations` gagal dengan `Corrupt { table: "operations", column: "kind" }` dan detail tanpa nilai yang tersimpan → AC-15

## Acceptance-criteria coverage

- AC-1 · 20 tabel `STRICT` dan `foreign_key_check` kosong di file buatan daemon; test schema untuk kolom dan index foreign key
- AC-2 · `application_id`, WAL, dan penolakan foreign key di file asli; pembacaan ulang pragma per koneksi lewat test `open` (tidak terlihat dari luar proses)
- AC-3 · create oleh daemon, mode file dan folder, `registry_info` dan `schema_migrations`; create bersamaan dan sisa file sementara lewat test `open`
- AC-4 · migrasi fixture v1 dengan snapshot lewat API publik dan test `open`; batas tiga snapshot lewat test `open`
- AC-5 · migrasi gagal di tengah lewat API publik dan test `open`; `backup_failed` lewat test `open`
- AC-6 · `schema_newer_than_app`, `not_a_registry`, `registry_missing` lewat daemon asli; reason lain lewat test `open`
- AC-7 · daemon asli: ready dengan prune, recovery tanpa mengubah file. Perilaku keluar sekarang mengikuti fitur 6 (tetap hidup sampai SIGTERM, `--print-roots`), bukan teks AC-7 (lihat catatan untuk rekonsiliasi di laporan verify)
- AC-8 · CAS dan generation lewat API publik, test, dan suite conformance
- AC-9 · balapan 16 thread lewat API publik, test, dan suite conformance
- AC-10 · nama, short key, path, rename lewat API publik dan test
- AC-11 · `known_instance_ids` dan `runtime_references` lewat API publik dan test
- AC-12 · kolom `secret_references` di file asli dan smoke test `FixtureCredential`
- AC-13 · CHECK waktu di file asli dan nanodetik lewat API publik
- AC-14 · prune di start daemon, lewat API publik, dan test (batas umur, batas jumlah, batch 5.000)
- AC-15 · trigger dan set tertutup di file asli; `Corrupt` lewat API publik
- AC-16 · bolak balik aggregate dan readiness `unknown` lewat API publik; setiap kode enum lewat test
