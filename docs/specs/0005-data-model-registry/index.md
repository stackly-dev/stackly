# 0005. Data model registry

**Date**: 2026-10-18
**Status**: In Progress

## Summary

Spec ini mengunci skema SQLite registry Stackly: 18 tabel untuk semua entitas PRD bagian 17, tabel `secret_reference_uses` milik spec 0011, dan dua tabel infrastruktur, lengkap dengan foreign key, constraint, dan revision per resource untuk compare and swap (CAS, update yang hanya berhasil kalau versi yang tersimpan masih sama dengan yang dibaca pemanggil). Migrasi hanya maju, semua migrasi tertunda berjalan dalam satu transaksi, dan sebelum migrasi registry selalu disalin dulu ke snapshot yang sudah diverifikasi. Kalau migrasi gagal atau schema tidak cocok saat daemon start, registry tidak disentuh dan daemon masuk recovery mode dengan laporan dan path backup. Kode hanya bisa menyentuh registry lewat trait `Registry` di `stackly-domain`, yang diimplementasikan crate baru `stackly-storage` dengan satu thread SQLite dan oleh fake di memori di `stackly-testkit`.

## Requirements

**User stories**:

- Sebagai pengguna, saya ingin update Stackly yang gagal di tengah migrasi registry tidak pernah menghapus atau membuat ulang registry saya, supaya instance dan datanya tetap bisa dipulihkan.
- Sebagai developer fitur 6, 7, 10, dan 20, saya ingin satu port registry dengan CAS dan error yang bertipe, supaya GUI dan CLI yang mengirim perintah bersamaan tidak pernah menghasilkan runtime atau instance ganda.
- Sebagai developer fitur yang belum punya spec (9, 11, 18, 29, 35, 49), saya ingin tabel dan constraint entitas saya sudah ada, supaya saya cukup menambah kolom lewat migrasi additive tanpa membongkar relasi.
- Sebagai penulis test, saya ingin fake registry dan suite conformance yang sama dengan SQLite, supaya test core tidak butuh database asli dan tetap setia pada perilaku aslinya.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-065):

- **AC-1**: Migrasi versi 1 membuat 18 tabel PRD bagian 17, tabel `secret_reference_uses` (spec 0011), dan dua tabel infrastruktur (`schema_migrations`, `registry_info`), semuanya `STRICT`. Test schema membaca `sqlite_schema` dan mencocokkan setiap tabel dan kolom dengan *Data model* dan *Peta field PRD* di spec ini; setiap foreign key punya index yang kolom awalnya adalah kolom anak foreign key itu, sesuai urutannya; `PRAGMA foreign_key_check` kosong (PRD bagian 17).
- **AC-2**: Koneksi tulis berjalan dengan `foreign_keys=ON`, `journal_mode=WAL`, `synchronous=FULL`, `fullfsync=ON`, `checkpoint_fullfsync=ON`, `busy_timeout=5000`, dan `trusted_schema=OFF`, dan `application_id` file bernilai `0x53544B59`; koneksi inspeksi dan verifikasi snapshot memakai set pragma read only di *Open, migrasi, dan recovery*. Open membaca ulang setiap pragma koneksi tulis dan gagal dengan `StorageError::PragmaNotApplied { name, expected, actual }` kalau ada yang tidak berlaku. Insert yang melanggar foreign key ditolak, dan batch yang gagal di operasi ke N tidak meninggalkan satu baris pun dari operasi sebelumnya (PRD bagian 17, NFR-005).
- **AC-3**: Di root data yang folder standarnya kosong, open membuat registry lewat file sementara `.stackly.sqlite.creating` (mode 0600, rollback journal, dikunci eksklusif selama create), menerapkan semua migrasi dalam satu transaksi, mengisi `registry_info` (registry ID baru, `created_at` dari `Clock`, versi app, generation 0) dan baris `schema_migrations` dengan sha256 dan `applied_at`, lalu mempublikasikannya sebagai `stackly.sqlite` lewat `hard_link` (tidak pernah menimpa file yang sudah ada), menghapus file sementara, sync folder, membuka dalam WAL, dan mengembalikan `Ready` dengan `created: true` tanpa snapshot. File `-wal` dan `-shm` bermode 0600. File sementara sisa create yang terputus (kuncinya bebas) dihapus pada open berikutnya; file sementara yang kuncinya masih dipegang proses lain menghasilkan `StorageError::CreateInProgress`.
- **AC-4**: Dari fixture beku `registry-v1.sql` dan set migrasi test versi 2, open membuat snapshot `registry/pre-migration/v1-to-v2-<waktu>.sqlite` lewat `VACUUM INTO`, memverifikasi snapshot itu (`quick_check` ok, `user_version` 1, `application_id` benar, sha256 dicatat), menerapkan migrasi tertunda dalam satu transaksi dengan `foreign_key_check` bersih dan sidik jari schema sama dengan nilai yang diharapkan untuk versi 2, lalu mengembalikan `Ready` dengan `migrated_from: 1`. Data fixture tetap ada. Open kedua tidak membuat snapshot baru. Hanya tiga snapshot terbaru yang disimpan, kecuali snapshot yang disebut laporan recovery. Setiap versi schema punya fixture beku yang sha256-nya tercatat, dan fixture yang berubah membuat test gagal (AC-065 bagian migrasi, PRD bagian 32.3).
- **AC-5**: Set migrasi test yang migrasi terakhirnya gagal setelah perintah sebelumnya berjalan membuat open mengembalikan `Recovery` dengan reason `migration_failed { from, to, failed_version, error }`. Isi registry sama seperti sebelum percobaan (versi, sidik jari, dan jumlah baris tiap tabel), snapshot ada dan lolos verifikasi, dan laporan JSON tertulis atomik di `registry/recovery/` dengan path dan sha256 snapshot. Open kedua (daemon restart) mencoba migrasi lagi, memakai ulang snapshot dari laporan terakhir karena sha256 `stackly.sqlite` dan `-wal` sama, mengembalikan `Recovery` lagi, dan tidak membuat registry baru, snapshot baru, laporan baru, atau menghapus snapshot. Kalau snapshot gagal dibuat, reason-nya `backup_failed` dan tidak ada migrasi yang dicoba (AC-065, NFR-005).
- **AC-6**: Setiap kondisi berikut menghasilkan `Recovery` dengan reason yang sesuai, dan sha256 file `stackly.sqlite` serta `-wal` sebelum dan sesudah open sama: file bukan SQLite atau `application_id` lain (`not_a_registry`); path registry berupa symlink atau bukan file biasa (`unsafe_registry_file`); file ada tetapi tidak bisa dibuka (`open_failed`); `quick_check` gagal atau `registry_info` hilang atau rusak (`integrity_failed`); versi schema lebih baru dari app (`schema_newer_than_app`); riwayat migrasi bolong, berisi versi yang tidak dikenal, atau checksum berbeda (`migration_history_mismatch`); tabel, kolom, index, atau trigger diubah di luar runner (`schema_fingerprint_mismatch`); file registry tidak ada tetapi ada `-wal`, `-shm`, snapshot, laporan recovery, atau isi di folder data standar mana pun (`registry_missing`); proses lain memegang transaksi tulis melewati busy timeout (`registry_locked`).
- **AC-7**: Integration test `stacklyd` dengan `STACKLY_ROOT` ke `TestRoot`: start pertama menjalankan `prepare_layout`, membuat registry, mencatat `registry_state=ready` beserta versi schema dan versi SQLite yang dibundel, menjalankan prune retensi, lalu keluar 0. Setelah test menambal 4 byte `user_version` di offset 60 header file registry menjadi 999, start berikutnya mencatat `registry_state=recovery`, kode reason `schema_newer_than_app`, dan path laporan, keluar dengan kode 3 (`// provisional: F6`), dan sha256 file registry tidak berubah. Tidak ada test daemon yang menjalankan `prepare_layout` atau open registry tanpa `STACKLY_ROOT`; test resolusi home memakai flag tersembunyi `--roots-only` (`// provisional: F6`) yang berhenti setelah root di resolve.
- **AC-8**: Setiap update aggregate membawa expected revision. Kalau sama dengan yang tersimpan, update berhasil dan revision menjadi expected ditambah 1; kalau tidak, `RevisionConflict { resource, expected, actual }` dan tidak ada operasi lain di batch itu yang diterapkan. Batch dengan `expected_generation` yang sudah lewat gagal dengan `GenerationConflict`. Setiap commit menaikkan generation tepat 1, dan setiap baca mengembalikan generation snapshot-nya. API registry tidak punya transaksi yang dibuka pemanggil, jadi tidak ada transaksi SQLite yang hidup di luar satu perintah (REQ-048).
- **AC-9**: Suite `registry_conformance` lulus untuk `FakeRegistry` dan `SqliteRegistry`. Di dalamnya: 16 thread yang bersamaan memasukkan instance bernama sama menghasilkan tepat satu sukses dan sisanya `NameTaken`; 16 thread yang memasukkan runtime managed untuk artefak yang sama menghasilkan tepat satu sukses dan sisanya `DuplicateRuntime`; 16 thread yang mengupdate instance yang sama dengan expected revision yang sama menghasilkan tepat satu sukses (NFR-005, REQ-048).
- **AC-10**: Nama instance unik di antara instance yang tidak retained (`NameTaken`), dan nama instance retained boleh dipakai lagi. Short key (8 hex terakhir ID) unik di semua instance (`ShortKeyTaken`). Rename hanya mengubah `name`, `revision`, dan `updated_at`; ID, runtime, config, dan ketujuh baris `instance_paths` tetap. Baris path dihitung registry sendiri dari ID lewat `stackly_platform::instance_path_entries`, relatif terhadap root bernama, dan hasil `RootSet::resolve`-nya sama persis dengan `DirectoryLayout::instance_paths(id)`. Untuk 1000 instance, tidak ada path tersimpan yang sama dengan atau berada di bawah path instance lain di root yang sama, dan pasangan (root, path relatif) yang sama ditolak (`PathTaken`) (BR-002, BR-003).
- **AC-11**: `known_instance_ids` mengembalikan ID semua instance, termasuk retained, urut naik. `runtime_references(id)` mengembalikan instance di semua lifecycle (termasuk `stopped` dan `retained`), runtime yang bergantung padanya, operasi yang belum selesai pada runtime itu, dan backup yang menahannya, masing masing urut ID naik; ID runtime yang tidak dikenal menghasilkan `NotFound`. `DELETE` mentah pada runtime yang masih dirujuk ditolak foreign key (BR-007).
- **AC-12**: `secret_references` hanya punya kolom `id`, `reference`, `platform_store_key`, `owner_kind`, `owner_id`, `purpose`, `value_origin`, `status`, `status_observed_at`, `value_updated_at`, `revision`, `created_at`, dan `updated_at`, dan `secret_reference_uses` hanya `reference_id`, `resource_kind`, `resource_id`, dan `field` (model spec 0011); tidak ada tabel lain dengan kolom untuk nilai secret; dan tidak ada tipe parameter di trait `Registry` atau `WriteOp` yang bisa membawa nilai secret (hanya ID dan teks reference). Sebagai smoke test, setelah suite conformance berjalan di `SqliteRegistry` sambil test memegang `FixtureCredential`, `assert_absent` atas `String::from_utf8_lossy` byte `stackly.sqlite` dan `-wal`, dibaca sebelum registry ditutup, lulus (BR-018).
- **AC-13**: Setiap kolom waktu dijaga CHECK bentuk UTC 30 karakter dengan sembilan digit pecahan dan akhiran `Z`. Nilai dengan offset lokal (`+07:00`) atau presisi lain ditolak. Nilai yang ditulis lalu dibaca kembali sama persis dengan `jiff::Timestamp` aslinya sampai nanodetik. Tidak ada kolom yang memakai waktu dari SQLite (`CURRENT_TIMESTAMP` atau default waktu); semua waktu berasal dari `Clock`, jadi nilai `FakeClock` muncul apa adanya.
- **AC-14**: `prune(policy, now)` dengan `RetentionPolicy::DEFAULT` menghapus operasi berstate akhir dan audit event yang waktunya lebih kecil dari `now` dikurangi 30 hari atau di luar 100.000 baris terbaru yang boleh dihapus, lease `released` yang lebih tua dari 30 hari, dan process observation di luar 20 terbaru per instance. Operasi yang belum selesai atau `needs_recovery` dan lease `active` atau `suppressed` tidak pernah dihapus dan tidak dihitung dalam batas jumlah. Prune tidak menaikkan generation. Hasilnya jumlah baris yang dihapus per tabel (NFR-011).
- **AC-15**: `UPDATE` pada field manifest `provider_revisions` ditolak trigger, sedangkan `approval` dan `approved_at` boleh diubah; `UPDATE` pada `audit_events` ditolak. Kode di luar set tertutup (misalnya lifecycle `paused`) ditolak CHECK. Kode set terbuka yang tidak dikenal saat dibaca menghasilkan `Corrupt { table, column, detail }` (BR-017).
- **AC-16**: Setiap aggregate yang ditulis lalu dibaca kembali sama dengan aslinya, termasuk urutan endpoint dan member, kecuali readiness dan health yang dibaca `unknown` dengan reason `daemon_restarted` dan `observed_at` sama dengan waktu open registry. Setiap kode state spec 0004 (state runtime, lifecycle, desired, startup, state operasi, holder dan state lease, transport, install mode) bolak balik lewat kolomnya, diuji menyeluruh per nilai enum.
- **AC-17**: Setiap varian `RegistryError` dan recovery mode punya kode di tabel *Kode error di wire*, dengan kategori, `retryable`, dan kunci `details` persis seperti tabel itu di katalog `stackly-protocol::codes::registry`; test di `stacklyd` memetakan setiap varian dan memastikan tidak ada yang jatuh ke kode lain (spec 0006 AC-8).

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md): semua entitas PRD 17 sekarang, migrasi maju saja dengan snapshot terverifikasi, semua migrasi tertunda dalam satu transaksi, deteksi lewat versi, checksum, dan sidik jari schema, recovery mode tanpa menyentuh registry, WAL dengan `synchronous=FULL` dan `fullfsync`, path instance disimpan relatif terhadap root bernama, port `Registry` di domain dengan fake di testkit, dan satu thread SQLite untuk baca dan tulis.

Registry adalah satu file SQLite yang hanya dibuka `stackly-storage` di dalam daemon; semua kode lain melihatnya lewat trait `Registry` yang sync dan object safe, dengan CAS per resource dan generation per registry.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya API registry yang sync dan dipanggil lewat `spawn_blocking`, bukan async.

## Feature design

### Peta crate dan dependency

| Crate | Perubahan |
| --- | --- |
| `stackly-domain` | modul baru `registry`: trait `Registry`, `RegistryError`, `BatchError`, `WriteBatch`, `WriteOp`, `Committed`, `Versioned<T>`, `Generation`, `ResourceRef`, filter dan query baca, `OperationCursor`, `RetentionPolicy`, `PruneReport`, `ArtifactRecord`, `ProviderRevisionRecord`; value type path (`LogicalRoot`, `InstancePathKind`, `RelativePath`, `InstancePathEntry`); ID `RegistryId`; konstruktor `restore` untuk aggregate; field baru di aggregate (lihat *Pemetaan aggregate*). Tetap tanpa crate Stackly lain, tokio, atau libc |
| `stackly-platform` | fungsi murni `instance_path_entries(InstanceId) -> [InstancePathEntry; 7]` (tidak butuh root), `DirectoryLayout::registry_snapshots_dir()` (`registry/pre-migration`) dan `registry_recovery_dir()` (`registry/recovery`), `RootSet::resolve(LogicalRoot, &RelativePath) -> PathBuf` |
| `stackly-storage` (baru, `crates/storage`) | adapter SQLite: open dan recovery, runner migrasi, file SQL migrasi, sidik jari, snapshot, `SqliteRegistry`. Dependency: `stackly-domain`, `stackly-platform`, `rusqlite` (fitur `bundled` saja), `sha2`, `serde`, `serde_json`, `jiff`, `thiserror`, `tracing`. Dev: `stackly-testkit`, `stackly-evidence`, `tempfile` bila perlu |
| `stackly-testkit` | `FakeRegistry`, trait `RegistrySeed` khusus test, builder fixture aggregate, suite `registry_conformance` |
| `stackly-protocol` | submodul katalog `codes::registry` (AC-17), dibangun setelah milestone 1 spec 0006 |
| `stacklyd` | dependency `stackly-storage`; start memanggil `prepare_layout`, `open_registry`, lalu `prune`; flag tersembunyi `--roots-only` (`// provisional: F6`) |
| workspace `Cargo.toml` | `stackly-storage` sebagai member dan di `[workspace.dependencies]`; `rusqlite` versi stabil terbaru saat build dengan fitur `bundled`, dikunci `Cargo.lock` (SQLite yang dibundel minimal 3.45) |

Aturan baru di `cargo xtask check-deps`:

- `stackly-storage` hanya boleh bergantung langsung pada `stackly-domain` dan `stackly-platform` di antara crate Stackly (dev: `stackly-testkit`, `stackly-evidence`). Tidak pernah `stackly-core`, `stackly-protocol`, atau `stackly-platform-macos`.
- `rusqlite` hanya boleh menjadi dependency langsung `stackly-storage`.
- Di antara bin, hanya `stacklyd` yang boleh bergantung pada `stackly-storage` (aturan CLI yang lama tetap).

`stackly-storage` tidak memakai `unsafe` dan tidak ikut cek lintas target Windows (build `bundled` mengompilasi C untuk target).

### Aturan kolom bersama

Semua tabel `STRICT`. Ekspresi CHECK ini ditulis apa adanya di SQL migrasi untuk setiap kolom yang memakai jenisnya:

| Jenis | Tipe SQL | CHECK |
| --- | --- | --- |
| `uuid` | `TEXT` | `length(c) = 36 AND c NOT GLOB '*[^0-9a-f-]*' AND substr(c,9,1) = '-' AND substr(c,14,1) = '-' AND substr(c,19,1) = '-' AND substr(c,24,1) = '-' AND substr(c,15,1) = '7'` |
| `ts` (UTC) | `TEXT` | `length(c) = 30 AND c GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]T[0-2][0-9]:[0-5][0-9]:[0-6][0-9].[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]Z'` |
| `sha256` | `TEXT` | `length(c) = 64 AND c NOT GLOB '*[^0-9a-f]*'` |
| `code` | `TEXT` | `length(c) BETWEEN 1 AND 64 AND c GLOB '[a-z]*' AND c NOT GLOB '*[^a-z0-9_]*'` |
| `name` | `TEXT` | `length(c) BETWEEN 1 AND 63 AND c GLOB '[a-z]*' AND c NOT GLOB '*[^a-z0-9-]*' AND c NOT GLOB '*--*' AND c NOT GLOB '*-'` (VAL-001) |
| `version` | `TEXT` | `length(c) BETWEEN 3 AND 64 AND c GLOB '[0-9]*.[0-9]*' AND c NOT GLOB '*[^0-9A-Za-z.+-]*'` (penjaga kasar; `ExactVersion` di Rust yang menentukan) |
| `line` | `TEXT` | `length(c) BETWEEN 1 AND 16 AND c GLOB '[0-9]*' AND c NOT GLOB '*[^0-9.]*'` |
| `relpath` | `TEXT` | `length(c) BETWEEN 1 AND 1024 AND c NOT GLOB '/*' AND c NOT GLOB '*/' AND c NOT GLOB '*//*' AND ('/' \|\| c \|\| '/') NOT GLOB '*/./*' AND ('/' \|\| c \|\| '/') NOT GLOB '*/../*'` |
| `abspath` | `TEXT` | `length(c) BETWEEN 2 AND 4096 AND c GLOB '/*'` |
| `bool` | `INTEGER` | `c IN (0, 1)` |
| `json` | `TEXT` | `json_valid(c)`, plus `json_type(c) = 'array'` atau `'object'` bila disebut |
| `revision` | `INTEGER` | `NOT NULL CHECK (c >= 1)` |

- Waktu ditulis dengan tepat sembilan digit pecahan dan `Z` (misalnya `format!("{ts:.9}")` di jiff; build memastikan bentuknya lewat test bolak balik), dibaca dengan `Timestamp::from_str`. Semua nilai waktu dari `Clock`, tidak pernah dari SQLite.
- ID ditulis lowercase lewat `Display` tipe ID domain, dibaca lewat `FromStr` (spec 0001).
- Kolom yang disebut "set tertutup" memakai `CHECK (c IN (...))` dengan kode stabil spec 0004. Kolom "set terbuka" memakai CHECK `code` saja dan divalidasi Rust oleh fitur pemiliknya; kode yang tidak dikenal saat baca menghasilkan `Corrupt`.
- Nama tabel mengikuti PRD bagian 17 apa adanya, supaya trace tetap utuh. Nama kolom yang bentrok dengan kata kunci SQL diganti: `values` menjadi `config_values`, `constraint` menjadi `version_constraint`.

### Data model

`*` berarti wajib (`NOT NULL`). Setiap tabel yang punya `revision` adalah aggregate yang bisa berubah; baris anaknya berubah bersama revision induk dalam batch yang sama.

**`registry_info`** (satu baris, infrastruktur)

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `singleton` | `INTEGER` PK | `CHECK (singleton = 1)` |
| `registry_id`* | `uuid` | dibuat saat registry dibuat; tidak pernah berubah |
| `created_at`* | `ts` | |
| `created_by_version`* | `TEXT` | versi app, 1 sampai 64 karakter |
| `generation`* | `INTEGER` | `>= 0`, naik tepat 1 di setiap commit batch |

**`schema_migrations`** (infrastruktur): `version` `INTEGER` PK `>= 1` · `name`* `TEXT` (`[a-z0-9_]`, 1 sampai 64) · `sha256`* · `applied_at`* `ts` · `app_version`* `TEXT`. Versi berurutan dari 1 tanpa lubang.

**`provider_sources`** (kode milik fitur 17 dan 37)

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `id` | `uuid` PK | |
| `kind`* | `code` | set terbuka |
| `location`* | `TEXT` | 1 sampai 4096 |
| `trust_root` | `TEXT` | |
| `last_sync_at` | `ts` | |
| `status`* | `code` | set terbuka; sumber tidak dianggap trusted hanya karena HTTPS |
| `revision`*, `created_at`*, `updated_at`* | | |

Unik `(kind, location)`.

**`provider_revisions`**

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `pk` | `INTEGER` PK | kunci internal storage, tidak pernah keluar dari crate |
| `provider`* | `TEXT` | `ProviderName`: 3 sampai 127, tepat satu `/`, `GLOB '[a-z]*/[a-z]*'`, hanya `[a-z0-9/-]` |
| `revision`* | `TEXT` | `RevisionLabel`: 1 sampai 64, `GLOB '[0-9A-Za-z]*'`, hanya `[0-9A-Za-z.+_-]`, tanpa `..` |
| `manifest_sha256`* | `sha256` | |
| `software`* | `name` | |
| `schema_version` | `TEXT` | apiVersion manifest, diisi fitur 9 |
| `publisher` | `TEXT` | fitur 37 |
| `capabilities`* | `json` array | kode `Capability` |
| `line_compatibility`* | `json` object | `{ "<data line>": ["<runtime line>", ...] }` |
| `source_id` | `uuid` | FK `provider_sources(id)` restrict |
| `approval` | `code` | set terbuka, fitur 37 |
| `approved_at` | `ts` | |
| `created_at`* | `ts` | |

Unik `(provider, revision, manifest_sha256)`, index `source_id`. Trigger `provider_revisions_immutable`: `BEFORE UPDATE OF` semua kolom kecuali `approval` dan `approved_at` menjalankan `RAISE(ABORT, 'provider revision is immutable')` (BR-017).

**`artifacts`** (diisi penuh fitur 18)

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `pk` | `INTEGER` PK | internal |
| `sha256`* | `sha256` | unik; satu artefak konkret |
| `software`* | `name` | |
| `version`* | `version` | exact version, bukan selector |
| `os`* | `TEXT` | set tertutup `macos`, `windows`, `linux` |
| `arch`* | `TEXT` | set tertutup `aarch64`, `x86_64` |
| `minimum_os` | `TEXT` | |
| `source` | `TEXT` | asal unduhan |
| `signature_info` | `json` object | |
| `dependency_manifest` | `json` object | |
| `provider_revision_pk` | `INTEGER` | FK `provider_revisions(pk)` restrict |
| `created_at`* | `ts` | |

Index `provider_revision_pk`.

**`runtimes`**

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `id` | `uuid` PK | `RuntimeId` |
| `software`* | `name` | |
| `version`* | `version` | |
| `line`* | `line` | |
| `os`*, `arch`* | `TEXT` | set tertutup seperti `artifacts` |
| `install_mode`* | `TEXT` | set tertutup `managed`, `external` |
| `artifact_sha256` | `sha256` | FK `artifacts(sha256)` restrict |
| `external_root` | `abspath` | |
| `observed_sha256` | `sha256` | |
| `state`* | `TEXT` | set tertutup: `absent`, `resolving`, `downloading`, `verifying`, `installing`, `installed`, `failed`, `missing`, `drifted` |
| `installed_at`, `last_verified_at` | `ts` | |
| `revision`*, `created_at`*, `updated_at`* | | |

CHECK bentuk: managed berarti `artifact_sha256` terisi dan `external_root` serta `observed_sha256` kosong; external berarti `artifact_sha256` kosong dan `external_root` terisi. Unique index parsial `artifact_sha256 WHERE install_mode = 'managed'` (satu runtime managed per artefak) dan `external_root WHERE install_mode = 'external'`, plus index biasa `artifact_sha256` untuk foreign key. Path runtime managed diturunkan `DirectoryLayout::runtime_dir`, tidak disimpan.

**`runtime_dependencies`**: `runtime_id`* FK `runtimes` cascade · `dependency_runtime_id`* FK `runtimes` restrict (menahan GC) · `version_constraint` `TEXT`. PK `(runtime_id, dependency_runtime_id)`, CHECK keduanya berbeda, index `dependency_runtime_id`. Diisi dari `Runtime::depends_on`.

**`instances`**

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `id` | `uuid` PK | `InstanceId` |
| `name`* | `name` | |
| `display_name` | `TEXT` | 1 sampai 128 karakter |
| `provider_revision_pk`* | `INTEGER` | FK `provider_revisions(pk)` restrict |
| `software`* | `name` | |
| `runtime_id`* | `uuid` | FK `runtimes(id)` restrict (BR-007) |
| `data_line`* | `line` | |
| `data_version`* | `version` | |
| `data_initialized_at` | `ts` | |
| `desired`* | `TEXT` | set tertutup `running`, `stopped` |
| `startup`* | `TEXT` | set tertutup `manual`, `at_login` |
| `lifecycle`* | `TEXT` | set tertutup: `uninitialized`, `initializing`, `stopped`, `starting`, `running`, `stopping`, `recovering`, `failed`, `unknown`, `retained` |
| `last_failure` | `code` | |
| `config_revision_id` | `uuid` | config aktif |
| `ownership`* | `code` | set terbuka; fitur 5 hanya menulis `created` (fitur 51 dan 57 menambah) |
| `retained_at` | `ts` | |
| `retained_by_operation` | `uuid` | tanpa FK |
| `created_by_operation`* | `uuid` | tanpa FK, sama dengan manifest |
| `revision`*, `created_at`*, `updated_at`* | | |

CHECK: `(lifecycle = 'retained') = (retained_at IS NOT NULL)`; `(retained_at IS NULL) = (retained_by_operation IS NULL)`; lifecycle `stopped`, `starting`, `running`, `stopping`, `recovering`, atau `unknown` mewajibkan `data_initialized_at` (BR-008). FK komposit `(id, config_revision_id)` ke `config_revisions(instance_id, id)`, `DEFERRABLE INITIALLY DEFERRED`, supaya config aktif selalu milik instance itu. Index: unique `name WHERE retained_at IS NULL`, unique ekspresi `substr(id, 29)` (short key), `runtime_id`, `provider_revision_pk`. Readiness dan health tidak disimpan.

**`instance_paths`** (`WITHOUT ROWID`)

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `instance_id`* | `uuid` | FK `instances` cascade |
| `kind`* | `TEXT` | set tertutup `root`, `data`, `config`, `config_history`, `temp`, `logs`, `run` |
| `logical_root`* | `TEXT` | set tertutup `data`, `logs`, `runtime` |
| `relative_path`* | `relpath` | |
| `managed`* | `bool` | `1` untuk semua path fitur ini |

PK `(instance_id, kind)`, unik `(logical_root, relative_path)`. Isi untuk instance aktif: `root` `data:instances/<id>`, `data` `data:instances/<id>/data`, `config` `data:instances/<id>/config`, `config_history` `data:instances/<id>/config-history`, `temp` `data:instances/<id>/temp`, `logs` `logs:instances/<id>`, `run` `runtime:i/<short-key>`.

**`instance_endpoints`**

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `instance_id`* | `uuid` | FK `instances` cascade |
| `position`* | `INTEGER` | `>= 0`, urutan di `Instance::endpoints` |
| `name`* | `TEXT` | `EndpointName` (pola `name`, maksimal 32) |
| `kind`* | `TEXT` | set tertutup `ip`, `unix` |
| `transport` | `TEXT` | set tertutup `tcp`, `udp` |
| `address` | `TEXT` | bentuk teks `IpAddr` dari Rust |
| `port_request` | `TEXT` | set tertutup `fixed`, `auto` |
| `fixed_port`, `bound_port` | `INTEGER` | 1 sampai 65535 |
| `socket_name` | `TEXT` | `SocketName`: 1 sampai 15 byte, hanya `[A-Za-z0-9._-]`, bukan `.` atau `..` |
| `enabled`* | `bool` | default 1 |

PK `(instance_id, name)`, unik `(instance_id, position)`. CHECK bentuk: `ip` mewajibkan `transport`, `address`, `port_request`, dan `fixed_port` terisi tepat saat `fixed`, dengan `socket_name` kosong; `unix` mewajibkan `socket_name` dan semua kolom IP kosong. Unik `(instance_id, socket_name)`. Registry tidak memeriksa konflik endpoint lintas instance (BR-004, BR-005): pemanggil (fitur 20) menjalankan `endpoints_conflict` dan `resolve_port` domain atas data yang dibacanya, lalu commit dengan `expected_generation` dari baca itu, sehingga perubahan apa pun di antaranya membuat commit gagal dan diulang.

**`config_revisions`** (kode `status` milik fitur 29)

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `id` | `uuid` PK | |
| `instance_id`* | `uuid` | FK `instances` cascade |
| `config_values`* | `json` object | field secret hanya berisi teks reference `local:<path>` (spec 0011), tidak pernah nilai |
| `rendered_sha256` | `sha256` | |
| `previous_id` | `uuid` | FK `config_revisions(id)` set null |
| `status`* | `code` | set terbuka |
| `created_by_operation` | `uuid` | tanpa FK |
| `created_at`* | `ts` | |

Unik `(instance_id, id)` untuk FK komposit dari `instances`, CHECK `previous_id <> id`, index `previous_id`.

**`secret_references`** (model logis milik spec 0011): `id` `uuid` PK · `reference`* `TEXT` unik, 9 sampai 255 byte, `GLOB 'local:*'` · `platform_store_key`* `TEXT` 1 sampai 512 byte · `owner_kind`* set tertutup `instance`, `environment`, `user` · `owner_id` `uuid` tanpa FK (polimorfik), CHECK `(owner_kind = 'user') = (owner_id IS NULL)` · `purpose`* `TEXT` 1 sampai 64, `GLOB '[a-z0-9]*'`, hanya `[a-z0-9._-]` · `value_origin` set tertutup `generated`, `entered`, `copied` · `status`* set tertutup `present`, `missing`, `unknown` · `status_observed_at` `ts` · `value_updated_at` `ts` · `revision`*, `created_at`*, `updated_at`*. Index `(owner_kind, owner_id)`. Trigger `secret_references_owner_fixed` menolak `UPDATE` yang mengubah `owner_kind` atau `owner_id`, kecuali perpindahan ke `owner_kind` `user` dengan `owner_id` NULL (satu satunya perpindahan pemilik di spec 0011). Tidak ada kolom nilai. Format `reference`, `platform_store_key`, dan purpose milik spec 0011; `revision` hanya naik lewat `put` dan pemulihan setelah delete gagal, tidak lewat probe.

**`secret_reference_uses`** (spec 0011): `reference_id`* `uuid` FK `secret_references(id)` `ON DELETE RESTRICT` · `resource_kind`* set tertutup `instance`, `environment` · `resource_id`* `uuid` tanpa FK (polimorfik) · `field`* `TEXT` 1 sampai 64, `GLOB '[a-z]*'`, hanya `[A-Za-z0-9_-]`. PK `(reference_id, resource_kind, resource_id, field)`, index `(resource_kind, resource_id)`. Ditulis dalam transaksi yang sama dengan config revision atau environment yang memakainya. Karena `resource_id` tidak punya foreign key, purge instance (fitur 54) dan penghapusan environment menghapus baris pemakaian resource itu secara eksplisit di batch yang sama.

**`environments`**: `id` `uuid` PK · `name`* `name` (index biasa; keunikan diputuskan fitur 44) · `manifest_location` `abspath` · `manifest_sha256` `sha256` · `lock_sha256` `sha256` · `last_operation_id` `uuid` tanpa FK · `revision`*, `created_at`*, `updated_at`*.

**`environment_instances`**: `environment_id`* FK `environments` cascade · `position`* `INTEGER >= 0` (urutan di `Environment::members`) · `alias`* `name` · `instance_id`* FK `instances` restrict · `ownership`* set tertutup `dedicated`, `shared` · `dependency_conditions`* `json` array, default `'[]'` (fitur 46). PK `(environment_id, alias)`, unik `(environment_id, position)`, unik `(environment_id, instance_id)`, unique index `instance_id WHERE ownership = 'dedicated'` (satu pemilik data per instance), index `instance_id`.

**`instance_demands`** (Lease di spec 0004)

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `id` | `uuid` PK | `LeaseId` |
| `instance_id`* | `uuid` | FK `instances` cascade |
| `source_kind`* | `TEXT` | set tertutup `environment`, `manual`, `startup` |
| `source_id` | `uuid` | FK `environments(id)` restrict |
| `session_id` | `TEXT` | `SessionId`: 1 sampai 64, hanya `[!-~]` |
| `state`* | `TEXT` | set tertutup `active`, `suppressed`, `released` |
| `state_changed_at` | `ts` | |
| `acquired_at`* | `ts` | |

CHECK: `(source_kind = 'environment') = (source_id IS NOT NULL)`; holder `environment` tanpa session; holder `startup` wajib session; `(state = 'active') = (state_changed_at IS NULL)`; `suppressed` hanya untuk `startup`. Unique index `(instance_id, source_id) WHERE source_kind = 'environment' AND state = 'active'`. Index `(instance_id, state)` dan `source_id`. Lease tidak punya revision; perubahannya ikut batch yang membawa CAS instance bila perlu.

**`operations`** (tabel dasar; journal dan step milik spec 0007)

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `id` | `uuid` PK | `OperationId` |
| `kind`* | `code` | set terbuka (`OperationKind` `#[non_exhaustive]`) |
| `resource_kind`* | `TEXT` | set tertutup `runtime`, `instance`, `environment`, `manager` |
| `resource_id` | `uuid` | tanpa FK, supaya riwayat tetap ada setelah resource hilang |
| `idempotency_key` | `TEXT` | 1 sampai 128, hanya `[!-~]` |
| `state`* | `TEXT` | set tertutup `queued`, `running`, `succeeded`, `failed`, `cancel_requested`, `cancelled`, `needs_recovery` |
| `stage` | `code` | maksimal 32 |
| `progress_done`, `progress_total` | `INTEGER` | keduanya kosong berarti indeterminate |
| `failure_code` | `code` | |
| `failure_retryable` | `bool` | |
| `recovery_hint` | `code` | |
| `started_at` | `ts` | |
| `revision`*, `created_at`*, `updated_at`* | | |

CHECK: `(resource_kind = 'manager') = (resource_id IS NULL)`; `progress_done` dan `progress_total` terisi bersama dengan `0 <= done <= total`; `failure_code` dan `failure_retryable` terisi bersama; `failed` mewajibkan `failure_code`. Index: `(resource_kind, resource_id)`, `idempotency_key`, `(created_at, id)` untuk halaman, `(state, updated_at)` untuk retensi, dan index parsial `created_at WHERE state IN ('queued', 'running', 'cancel_requested', 'needs_recovery')`.

**`process_observations`** (diisi fitur 10): `id` `INTEGER` PK · `instance_id`* FK `instances` cascade · `pid`* `INTEGER > 0` · `start_identity`* `TEXT` 1 sampai 256 · `executable_identity`* `TEXT` 1 sampai 1024 · `boot_id` `TEXT` · `session_id` `TEXT` · `observed_at`* `ts`. Index `(instance_id, observed_at)`. Bukan sumber kebenaran tunggal dan tidak pernah mengotorisasi kill.

**`backups`** (kode milik fitur 49)

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `id` | `uuid` PK | `BackupId` |
| `instance_id` | `uuid` | FK `instances` set null; backup tetap ada setelah purge |
| `runtime_id` | `uuid` | FK `runtimes` restrict; runtime yang dibutuhkan untuk restore |
| `runtime_identity`* | `TEXT` | 1 sampai 256, salinan teks software, versi, dan target |
| `method`*, `scope`*, `consistency`* | `code` | set terbuka |
| `path`* | `abspath` | tujuan yang dipilih pengguna |
| `checksum_manifest` | `json` object | |
| `created_at`* | `ts` | |
| `verified_at`, `restore_verified_at` | `ts` | dibuat terpisah dari restore yang terverifikasi |

Index `instance_id` dan `runtime_id`.

**`audit_events`** (baris pertama ditulis fitur 11 untuk aksi secret, tampilan di fitur 35): `id` `uuid` PK · `actor`* `code` · `action`* `TEXT` 1 sampai 64, `GLOB '[a-z]*'`, hanya `[a-z0-9_.]` · `resource_kind` `code` · `resource_id` `uuid` · `operation_id` `uuid` tanpa FK · `result`* `code` (`succeeded` atau `failed`; kode error kegagalan ditulis di `safe_details.errorCode`, karena kode wire memuat titik) · `safe_details`* `json` object, default `'{}'` · `occurred_at`* `ts`. Index `occurred_at` dan `operation_id`. Trigger `audit_events_append_only` menolak setiap `UPDATE`; baris hanya hilang lewat prune. Bukan klaim tamper proof.

**`settings`** (kunci milik fitur 32): `key` `TEXT` PK, 1 sampai 128, `GLOB '[a-z]*'`, hanya `[a-z0-9_.]` · `typed_value`* `json` · `revision`* · `updated_at`*.

### Peta field PRD

Field PRD yang tidak menjadi kolom dengan nama yang sama:

| Tabel | Field PRD | Di skema |
| --- | --- | --- |
| `provider_sources` | `last_sync` | `last_sync_at` |
| `provider_revisions` | `provider_id`, `manifest_digest` | `provider` (`ProviderName`) plus kunci internal `pk`, `manifest_sha256` |
| `artifacts` | `id`, `provider_revision`, `digest` | `pk`, `provider_revision_pk`, `sha256` |
| `runtimes` | `artifact_id`, `resolved_path`, `last_verified` | `artifact_sha256`; path managed diturunkan layout, path external di `external_root`; `last_verified_at` |
| `runtime_dependencies` | `constraint` | `version_constraint` |
| `instances` | `provider_revision`, `config_revision`, `desired_state`, `startup_policy` | `provider_revision_pk`, `config_revision_id`, `desired`, `startup` |
| `instance_paths` | `resolved_path`, `owner` | `relative_path` terhadap `logical_root`, di resolve saat dipakai; pemilik selalu user yang menjalankan daemon dan diperiksa lewat `FileInfo.owner` saat dipakai |
| `instance_endpoints` | `port`, `requested_mode` | `fixed_port` dan `bound_port`, `port_request` |
| `config_revisions` | `values`, `rendered_digest` | `config_values`, `rendered_sha256` |
| `secret_references` | `scope` | `owner_kind` plus `owner_id` (spec 0011) |
| `environments` | `manifest_digest`, `lock_digest`, `last_operation` | `manifest_sha256`, `lock_sha256`, `last_operation_id` |
| `instance_demands` | `session_policy` | `session_id` plus aturan holder |
| `operations` | `type`, `resource_id`, `status`, `progress`, `error_code` | `kind`, `resource_kind` plus `resource_id`, `state`, `progress_done` plus `progress_total`, `failure_code` plus `failure_retryable` |
| `process_observations` | `boot/session_id` | `boot_id`, `session_id` |
| `audit_events` | `resource`, `timestamp` | `resource_kind` plus `resource_id`, `occurred_at` |

### Open, migrasi, dan recovery

`stackly_storage::open_registry(platform: &Platform, layout: &DirectoryLayout, app_version: &str) -> Result<RegistryOpen, StorageError>`. `RegistryOpen` adalah `Ready { registry: SqliteRegistry, summary: OpenSummary }` atau `Recovery(RecoveryReport)`. Pembagiannya: masalah pada isi atau identitas file registry menjadi `Recovery`; masalah pada lingkungan atau file milik storage sendiri menjadi `StorageError`, dan daemon keluar 1.

`StorageError` (`#[non_exhaustive]`): `RegistryDirUnusable { path, reason }` (folder `registry`, `pre-migration`, atau `recovery` bukan folder, berupa symlink, milik user lain, atau terbuka untuk group atau other), `CreateInProgress { path }`, `CreateFailed { detail }` (termasuk disk penuh saat create; sisa file sementara dibersihkan di start berikutnya), `PragmaNotApplied { name, expected, actual }`, `Io { op, path, detail }`.

Set pragma per jenis koneksi:

| Koneksi | Flag open | Pragma |
| --- | --- | --- |
| inspeksi | `READ_ONLY`, `NOFOLLOW` | `query_only=ON`, `trusted_schema=OFF`, `busy_timeout=5000` |
| create (file sementara) | `READ_WRITE`, `NOFOLLOW` | `journal_mode=DELETE`, `synchronous=FULL`, `fullfsync=ON`, `foreign_keys=OFF` saat migrasi lalu `ON`, `trusted_schema=OFF` |
| tulis (registry siap atau migrasi) | `READ_WRITE`, `NOFOLLOW`, tanpa `CREATE` | `journal_mode=WAL`, `synchronous=FULL`, `fullfsync=ON`, `checkpoint_fullfsync=ON`, `foreign_keys=ON`, `busy_timeout=5000`, `trusted_schema=OFF` |
| verifikasi snapshot | `READ_ONLY`, `NOFOLLOW` | `query_only=ON`, `trusted_schema=OFF` |

`fullfsync=ON` sudah berlaku untuk semua sync termasuk checkpoint; `checkpoint_fullfsync=ON` dipasang juga sebagai pengaman kalau `fullfsync` kelak diubah sendirian.

Urutan open:

1. **Siapkan folder dan bersihkan sisa milik storage.** `registry/` sudah disiapkan `prepare_layout`. `pre-migration/` dan `recovery/` dibuat dengan `create_dir_private` kalau belum ada, dan kalau ada diperiksa dengan aturan `prepare_layout` (`RegistryDirUnusable`). File sementara create (`.stackly.sqlite.creating` dan journal-nya) dicoba dikunci eksklusif dengan `File::try_lock`: kalau berhasil, ia sisa create yang mati dan dihapus; kalau tidak, `CreateInProgress`. File `pre-migration/.*.sqlite.tmp` dihapus.
2. **Preflight file.** `symlink_metadata` untuk `stackly.sqlite`, `-wal`, `-shm`, dan `-journal`. Yang ada tetapi bukan file biasa: `unsafe_registry_file`.
3. **Registry belum ada.** Kalau ada `-wal`, `-shm`, `-journal`, snapshot di `pre-migration/`, laporan di `recovery/`, atau entri selain `.DS_Store` di folder data standar mana pun (`providers`, `runtimes`, `instances`, `environments`, `operations`, `backups`, `retained`): `registry_missing` dengan daftar bukti. Kalau bersih: buat `.stackly.sqlite.creating` dengan `create_new` mode 0600, kunci eksklusif, buka sebagai koneksi create, terapkan semua migrasi dalam satu transaksi, isi `registry_info`, tutup, sync file, publikasikan dengan `std::fs::hard_link` ke `stackly.sqlite` (gagal kalau tujuan sudah ada, dan itu berarti `CreateInProgress`), hapus file sementara, sync folder, lalu lanjut ke langkah 4.
4. **Inspeksi read only.** Buka koneksi inspeksi. Gagal membuka: `open_failed`. Header bukan SQLite atau `application_id` lain: `not_a_registry`. `PRAGMA quick_check` tidak `ok`, atau baris `registry_info` hilang atau tidak valid: `integrity_failed`. `user_version` di atas versi terbaru set migrasi: `schema_newer_than_app`. Versi di `schema_migrations` bolong, tidak dikenal, beda dengan `user_version`, atau sha256 beda dengan migrasi tertanam: `migration_history_mismatch`. Sidik jari schema beda dengan nilai untuk versi itu: `schema_fingerprint_mismatch`. Koneksi inspeksi tidak pernah menulis file registry; WAL yang ada hanya dibaca. Koneksi ini ditutup sebelum langkah berikutnya.
5. **Buka untuk tulis.** Buka koneksi tulis, set pragma, baca ulang semuanya (`PragmaNotApplied`), lalu periksa ulang `application_id` dan `user_version` (beda dari hasil inspeksi berarti file diganti di antaranya: `open_failed`). Probe `BEGIN IMMEDIATE; ROLLBACK`; `SQLITE_BUSY` setelah busy timeout: `registry_locked`.
6. **Migrasi.** Kalau ada migrasi tertunda: siapkan snapshot (bawah), gagal berarti `backup_failed`. Lalu `PRAGMA foreign_keys = OFF` di luar transaksi, `BEGIN IMMEDIATE`, jalankan setiap migrasi berurutan beserta baris `schema_migrations` dan `PRAGMA user_version`, `PRAGMA foreign_key_check` wajib kosong, sidik jari wajib sama dengan nilai versi target, `COMMIT`, lalu `foreign_keys = ON` dan baca ulang. Kegagalan apa pun: `ROLLBACK`, `migration_failed`. Setiap open berikutnya mencoba lagi dari awal; kegagalan yang deterministik kembali menjadi recovery dengan snapshot dan laporan yang sama.
7. **Rapikan snapshot.** Setelah migrasi sukses, snapshot diurutkan menurut waktu dari namanya lalu versi tujuan, dan yang di luar tiga terbaru dihapus, kecuali yang disebut laporan recovery mana pun.
8. **Siap.** Catat `opened_at = Clock::now`, mulai thread registry, kembalikan `Ready` dengan `OpenSummary { created, migrated_from, schema_version, snapshot, sqlite_version, registry_id, opened_at }`.

Semua operasi file storage (create, lock, `hard_link`, hapus, sync file dan folder, chmod) ada di satu modul privat `files` yang memakai `std::fs` di bawah `#[cfg(unix)]` dan menolak path apa pun di luar folder `registry/` milik layout. Port `Filesystem` dipakai untuk `symlink_metadata`, `read_dir`, `create_dir_private`, dan `write_file_atomic` (laporan).

**Aturan migrasi**:

- **Kebijakan forward**: migrasi hanya maju, berurutan dari versi 1 tanpa lubang, dan semua migrasi tertunda berjalan dalam satu transaksi sesudah snapshot terverifikasi ada.
- **Kebijakan rollback**: tidak ada down migration. Rollback berarti memasang app versi lama lalu memulihkan snapshot `pre-migration` yang cocok dengan versi schema app itu; perubahan registry setelah snapshot hilang. Alur pengguna untuk ini milik fitur 48 (restore) dan fitur 61 (update dan rollback app). App yang menemukan registry lebih baru dari dirinya tidak pernah menulisnya (`schema_newer_than_app`).
- File SQL di `crates/storage/migrations/NNNN_<nama>.sql`, ditanam lewat `include_str!` dalam `MigrationSet::builtin()`, yang berisi `migrations: &[Migration { version, name, sql }]` dan `fingerprints: &[(u32, &str)]`. sha256 dihitung dari teks SQL.
- Migrasi hanya boleh memakai `CREATE`, `DROP`, `ALTER TABLE ... ADD COLUMN`, `INSERT`, `UPDATE`, `DELETE`, dan rebuild tabel 12 langkah SQLite (buat tabel baru, salin, drop, rename tabel baru ke nama lama, buat ulang index dan trigger). `RENAME COLUMN` dan `DROP COLUMN` dilarang, karena SQLite menulis ulang teks schema yang tersimpan dan hasilnya bisa bergantung pada versi SQLite yang dibundel. Rename tabel hanya di langkah terakhir rebuild.
- Migrasi yang sudah dirilis tidak pernah diedit. Menambah migrasi berarti menambah file, sidik jari versi baru, dan fixture beku versi itu.
- Set migrasi lain hanya bisa dipasang lewat `OpenOptions::with_migrations(MigrationSet)`, `#[doc(hidden)]`, untuk test. `MigrationSet` membawa sidik jari setiap versinya sendiri.

**Sidik jari schema**: sha256 atas baris `type \t name \t tbl_name \t sql \n` dari `sqlite_schema` (tanpa nama berawalan `sqlite_`), urut `type` lalu `name`. Setiap `sql` dinormalisasi: semua tanda kutip `"` dibuang (SQLite menambah kutip saat rename tabel), setiap deretan whitespace diringkas menjadi satu spasi, lalu dipangkas. Nilai yang diharapkan adalah konstanta di `MigrationSet`, tidak dihitung ulang saat open, karena registry pengguna dibuat oleh SQLite versi lama. Test `fingerprints_match_migrations` membangun setiap versi dari nol untuk memastikan konstanta itu, dan upgrade rusqlite wajib lulus test ini serta test fixture.

**Snapshot sebelum migrasi**:

- Kalau laporan recovery terbaru menyebut snapshot dengan versi `from` yang sama dan sha256 `stackly.sqlite` serta `-wal` sama dengan isi laporan itu, snapshot tersebut dipakai ulang setelah diverifikasi ulang.
- Selain itu `VACUUM INTO` ke `pre-migration/.v<from>-to-v<to>-<waktu>.sqlite.tmp`, sync file, chmod 0600, publikasi lewat `hard_link` ke `v<from>-to-v<to>-<waktu>.sqlite` (kalau nama itu sudah ada, akhiran `-2`, `-3`, dan seterusnya), hapus file sementara, sync folder.
- Verifikasi lewat koneksi verifikasi snapshot: `quick_check` ok, `user_version` sama dengan `from`, `application_id` benar; lalu hitung sha256.
- `<waktu>` adalah `Clock::now` dalam bentuk `YYYYMMDDTHHMMSSZ`.

**Laporan recovery**: JSON UTF-8 camelCase, schema `stackly.registry-recovery/v1`, ditulis lewat `write_file_atomic` ke `recovery/<waktu>.json` (akhiran `-2` dan seterusnya kalau nama sudah ada). Isi: `schema`, `detectedAt`, `reason` (`code` plus detail per reason), `appVersion`, `appSchemaVersion`, `dbSchemaVersion` (null kalau tidak terbaca), `sqliteVersion`, `files` (`name`, `len`, `sha256` untuk `stackly.sqlite` dan `-wal` yang ada, plus `len` saja untuk `-shm`), `snapshot` (`path`, `sha256`, `schemaVersion`, `createdAt`) atau null, dan `message`. Kalau laporan terbaru punya reason code, kedua versi schema, dan sha256 `stackly.sqlite` serta `-wal` yang sama, tidak ada file baru; `RecoveryReport` menunjuk file lama itu. Laporan untuk `backup_failed` dan `migration_failed` ditulis setelah koneksi tulis ditutup, jadi sha256 yang dicatat adalah keadaan file yang dilihat open berikutnya. Laporan tidak dihapus otomatis dan tidak berisi secret.

### Port registry

Di `stackly-domain::registry`. Sync, object safe, `Send + Sync`; daemon memanggilnya lewat `spawn_blocking`.

```rust
pub trait Registry: Send + Sync {
    fn provider_revision(&self, key: &ProviderRevisionKey) -> Result<Versioned<Option<ProviderRevisionRecord>>, RegistryError>;
    fn artifact(&self, sha256: &Sha256Digest) -> Result<Versioned<Option<ArtifactRecord>>, RegistryError>;
    fn runtime(&self, id: RuntimeId) -> Result<Versioned<Option<Runtime>>, RegistryError>;
    fn runtimes(&self) -> Result<Versioned<Vec<Runtime>>, RegistryError>;
    fn runtime_references(&self, id: RuntimeId) -> Result<Versioned<RuntimeReferences>, RegistryError>;
    fn instance(&self, id: InstanceId) -> Result<Versioned<Option<Instance>>, RegistryError>;
    fn instance_by_name(&self, name: &InstanceName) -> Result<Versioned<Option<Instance>>, RegistryError>;
    fn instances(&self, filter: InstanceFilter) -> Result<Versioned<Vec<Instance>>, RegistryError>;
    fn known_instance_ids(&self) -> Result<Versioned<Vec<InstanceId>>, RegistryError>;
    fn instance_paths(&self, id: InstanceId) -> Result<Versioned<Option<Vec<InstancePathEntry>>>, RegistryError>;
    fn environment(&self, id: EnvironmentId) -> Result<Versioned<Option<Environment>>, RegistryError>;
    fn environments(&self) -> Result<Versioned<Vec<Environment>>, RegistryError>;
    fn leases(&self, instance: InstanceId, filter: LeaseFilter) -> Result<Versioned<Vec<Lease>>, RegistryError>;
    fn operation(&self, id: OperationId) -> Result<Versioned<Option<Operation>>, RegistryError>;
    fn operations(&self, query: OperationQuery) -> Result<Versioned<OperationPage>, RegistryError>;
    fn commit(&self, batch: WriteBatch) -> Result<Committed, BatchError>;
    fn prune(&self, policy: &RetentionPolicy, now: jiff::Timestamp) -> Result<PruneReport, RegistryError>;
}
```

**Baca**:

- `Versioned<T> { value: T, generation: Generation }`: generation dibaca di transaksi baca yang sama dengan datanya. `Generation` adalah newtype `u64`.
- `instance_by_name` hanya mencari instance yang tidak retained. `InstanceFilter { include_retained: bool }`, urut `name` lalu `id`. `known_instance_ids` mencakup retained, urut naik.
- `runtime_references` untuk ID yang tidak ada menghasilkan `NotFound`; keempat daftarnya urut ID naik. `runtimes` dan `environments` urut ID naik. `LeaseFilter`: `All` atau `Unreleased` (active dan suppressed), urut `acquired_at` lalu `id`.
- `OperationQuery { active_only: bool, resource: Option<OperationResource>, limit: u16, before: Option<OperationCursor> }`, dengan `OperationResource` tipe resource operasi di domain (spec 0004) dan `OperationCursor { created_at: Timestamp, id: OperationId }`. `limit` di luar 1 sampai 500 menghasilkan `InvalidQuery`. Urutan `(created_at, id)` turun; `OperationPage { items, next: Option<OperationCursor> }`, `next` terisi kalau masih ada baris.
- Satu baris dengan kode set terbuka yang tidak dikenal membuat seluruh baca yang memuatnya gagal dengan `Corrupt`. Itu disengaja: data yang tidak bisa dipahami tidak boleh hilang diam diam dari daftar; doctor (fitur 34) yang menampilkannya.

**Tulis**:

- `WriteBatch { expected_generation: Option<Generation>, ops: Vec<WriteOp> }`, diterapkan dalam satu transaksi `BEGIN IMMEDIATE`. Gagal di mana pun berarti tidak ada yang diterapkan. `Committed { generation, revisions: Vec<(ResourceRef, ResourceRevision)> }`, satu entri per op yang membuat atau mengubah aggregate.
- `BatchError { op_index: Option<usize>, error: RegistryError }`. `op_index` kosong untuk error tingkat batch (`GenerationConflict`, `Busy`, `NoSpace`, `Unavailable`, `Io`).
- Urutan cek: `expected_generation` lebih dulu; lalu setiap op berurutan, dan di dalam satu op urutannya tetap: bentuk nilai (`InvalidRevision`, `NotUtf8Path`), keberadaan (`NotFound` untuk update, `DuplicateResource` untuk insert dengan ID yang sudah ada), revision (`RevisionConflict`), referensi (`MissingReference`), lalu aturan unik (`DuplicateRuntime`, `NameTaken`, `ShortKeyTaken`, `PathTaken`, `DedicatedOwnerExists`, `ActiveLeaseExists`, `SoftwareMismatch`). Error pertama yang ditemukan yang dikembalikan, jadi fake dan SQLite selalu memberi varian yang sama.
- Setiap aturan dicek lebih dulu dengan `SELECT` di dalam transaksi yang sama untuk menghasilkan error bertipe. Constraint SQL adalah penjaga terakhir; kalau constraint SQL yang menolak, hasilnya `Integrity { detail }`, yang berarti cek lebih dulu terlewat.
- Insert mewajibkan `revision` aggregate bernilai 1. Update membandingkan `expected` dengan revision tersimpan dan mewajibkan `revision` aggregate baru sama dengan `expected + 1` (domain yang menaikkannya). Selain itu `InvalidRevision` (bug pemanggil).
- Generation naik tepat 1 per commit yang berhasil, apa pun isinya. Pemanggil yang memakai `expected_generation` mencoba ulang baca dan commit paling banyak lima kali saat `GenerationConflict`, lalu menyerah dengan error itu.

`WriteOp` (`#[non_exhaustive]`; fitur pemilik menambah varian lewat update spec ini):

| Op | Yang ditulis | Cek khusus |
| --- | --- | --- |
| `InsertProviderRevision(ProviderRevisionRecord)` | satu baris `provider_revisions` | `DuplicateResource` kalau kunci (provider, revision, sha256) sudah ada |
| `InsertArtifact(ArtifactRecord)` | satu baris `artifacts` | `DuplicateResource` kalau sha256 sudah ada |
| `InsertRuntime(Runtime)` | `runtimes` dan `runtime_dependencies` dari `depends_on` | managed: artefak ada (`MissingReference`), satu runtime per artefak (`DuplicateRuntime`); external: satu runtime per `external_root` (`DuplicateRuntime`); dependency ada |
| `UpdateRuntime { runtime, expected }` | baris runtime, dan `runtime_dependencies` ditulis ulang | sama dengan insert untuk dependency |
| `InsertInstance(Instance)` | `instances`, `instance_endpoints` dari `endpoints` (dengan `position`), tujuh baris `instance_paths` dari `instance_path_entries(id)` | runtime dan provider revision ada; `software` instance sama dengan software runtime dan provider revision (`SoftwareMismatch`); nama, short key, dan path |
| `UpdateInstance { instance, expected }` | baris instance dan endpoint ditulis ulang; path tidak disentuh | nama kalau berubah; runtime ada dan software sama kalau `runtime_id` berubah |
| `InsertEnvironment(Environment)` | `environments` dan `environment_instances` (dengan `position`) | instance member ada; `DedicatedOwnerExists` |
| `UpdateEnvironment { environment, expected }` | baris environment dan member ditulis ulang | sama dengan insert |
| `InsertLease(Lease)` | satu baris `instance_demands` | instance dan environment ada; `ActiveLeaseExists` |
| `UpdateLease(Lease)` | baris lease | lease ada (`NotFound`); `ActiveLeaseExists` |
| `InsertOperation(Operation)` | satu baris `operations` | |
| `UpdateOperation { operation, expected }` | baris operasi | |

- `ProviderRevisionRecord { revision: ProviderRevision, schema_version: Option<String>, publisher: Option<String>, approval: Option<String> }` dan `ArtifactRecord { sha256, software, version, target, minimum_os: Option<String>, source: Option<String> }`. Field yang belum dipakai fitur ini (`source_id`, `approved_at`, `signature_info`, `dependency_manifest`, `provider_revision_pk` artefak) belum ada di record dan ditulis NULL.

`RegistryError` (`#[non_exhaustive]`, pesan lowercase): `NotFound { resource }`, `MissingReference { resource }`, `DuplicateResource { resource }`, `RevisionConflict { resource, expected, actual }`, `GenerationConflict { expected, actual }`, `InvalidRevision { resource, expected, actual }`, `InvalidQuery { reason }`, `NotUtf8Path { field }`, `NameTaken { name: InstanceName, existing: InstanceId }`, `ShortKeyTaken { key: ShortKey, existing: InstanceId }`, `PathTaken { root: LogicalRoot, path: RelativePath, existing: InstanceId }`, `DuplicateRuntime { existing: RuntimeId }`, `SoftwareMismatch { instance, runtime, provider_revision }` (tiga `SoftwareName`), `DedicatedOwnerExists { instance: InstanceId, environment: EnvironmentId }`, `ActiveLeaseExists { instance: InstanceId, environment: EnvironmentId }`, `Integrity { detail }`, `Corrupt { table, column, detail }`, `NoSpace`, `Busy`, `Unavailable`, `Io { detail }`.

`ResourceRef` (`#[non_exhaustive]`): `Runtime(RuntimeId)`, `Instance(InstanceId)`, `Environment(EnvironmentId)`, `Operation(OperationId)`, `Lease(LeaseId)`, `ProviderRevision(ProviderRevisionKey)`, `Artifact(Sha256Digest)`.

`WriteOp` yang ditambah fitur lain lewat update spec ini, dengan cek yang mengikuti urutan di atas: fitur 11 (spec 0011, langkah 0 build plan nya) menambah method baca untuk `secret_references`, `secret_reference_uses`, dan `audit_events`, varian `ResourceRef` untuk secret, dan op untuk `secret_references` (insert, update dengan `expected`, delete dengan `expected`), op yang mengganti seluruh baris `secret_reference_uses` satu resource, dan op insert `audit_events`, sehingga `put`, delete, dan pemakaian secret tertulis dalam satu batch dengan audit nya; fitur 20 dan 29 menambah op `config_revisions`, dan spec 0011 memasang penggantian pemakaian secret di batch yang sama. Nama dan cek op itu ditulis saat update spec ini oleh fitur pemiliknya.

**Kode error di wire** (katalog `stackly-protocol::codes::registry`, area `registry` milik spec ini; dipetakan `stacklyd` di modul `map`). Semua `retryable` false: kegagalan registry di tengah satu request tidak menjamin request itu belum berefek apa pun, dan konflik generation sudah dicoba ulang lima kali oleh pemanggil. Resource yang punya `ResourceKind` di spec 0006 dikirim di `ErrorBody.resource`.

| Varian | Kode | Kategori | Kunci `details` |
| --- | --- | --- | --- |
| `NotFound` | `resource.not_found` (spec 0006) | `not_found` | |
| `RevisionConflict` | `resource.revision_mismatch` (spec 0006) | `conflict` | `expected`, `actual` |
| `MissingReference` | `registry.missing_reference` | `conflict` | |
| `DuplicateResource` | `registry.duplicate_resource` | `conflict` | |
| `GenerationConflict` | `registry.generation_conflict` | `conflict` | `expected`, `actual` |
| `NameTaken` | `registry.name_taken` | `conflict` | `name`, `existing` |
| `ShortKeyTaken` | `registry.short_key_taken` | `conflict` | `key`, `existing` |
| `PathTaken` | `registry.path_taken` | `conflict` | `root`, `path`, `existing` |
| `DuplicateRuntime` | `registry.duplicate_runtime` | `conflict` | `existing` |
| `DedicatedOwnerExists` | `registry.dedicated_owner_exists` | `conflict` | `instance`, `environment` |
| `ActiveLeaseExists` | `registry.active_lease_exists` | `conflict` | `instance`, `environment` |
| `SoftwareMismatch` | `registry.software_mismatch` | `invalid_input` | `instance`, `runtime`, `providerRevision` |
| `NotUtf8Path` | `registry.path_not_utf8` | `invalid_input` | `field` |
| `Busy` | `registry.busy` | `unavailable` | |
| `Unavailable` | `registry.unavailable` | `unavailable` | |
| `NoSpace` | `registry.no_space` | `failed` | |
| `Io` | `registry.io_error` | `failed` | |
| `Corrupt` | `registry.corrupt` | `internal` | `table`, `column` |
| `Integrity` | `registry.integrity` | `internal` | |
| `InvalidRevision`, `InvalidQuery` | `internal.unexpected` (spec 0006) | `internal` | |
| recovery mode (spec 0006 AC-18) | `registry.recovery_mode` | `unavailable` | `reason`, `reportPath` |

Teks `detail` di `Integrity`, `Corrupt`, dan `Io` hanya masuk log, tidak pernah ke `message` wire. `OperationCursor` menyimpan `created_at` dengan presisi nanodetik dan tidak pernah dikirim sebagai `Timestamp` wire (yang hanya tiga digit pecahan); spec 0007 mengirimnya sebagai string opaque yang memuat waktu sembilan digit dan ID.

**Retensi**: `RetentionPolicy` berisi `operation_max_age`, `audit_max_age`, dan `released_lease_max_age` bertipe `std::time::Duration`, serta `operation_max_rows`, `audit_max_rows`, dan `observations_per_instance` bertipe `u32`. `RetentionPolicy::DEFAULT` (konstanta): 30 hari, 30 hari, 30 hari, 100.000, 100.000, dan 20. Batas umur: baris dengan waktu lebih kecil dari `now` dikurangi umur maksimum dihapus (`updated_at` untuk operasi berstate akhir, `occurred_at` untuk audit, `state_changed_at` untuk lease `released`). Batas jumlah hanya menghitung baris yang boleh dihapus, diurutkan menurut waktu itu lalu `id` turun, dan yang di luar batas dihapus. Observasi diurutkan `observed_at` lalu `id` turun per instance. Prune menghapus paling banyak 5.000 baris per transaksi (beberapa perintah berturut turut) dan tidak menaikkan generation. `PruneReport` berisi jumlah baris terhapus per tabel.

### Pemetaan aggregate

- Field aggregate selalu menang. Kolom yang tidak punya field di domain diisi registry: `created_at` saat insert dan `updated_at` setiap tulis, dari satu `clock.now()` per commit; kolom lain ditulis dengan nilai yang tertulis di tabel ini dan diabaikan saat baca sampai fitur pemiliknya menambah field domain.

| Tabel | Kolom tanpa field domain di fitur ini | Ditulis |
| --- | --- | --- |
| `runtimes` | `created_at`, `updated_at` | waktu commit |
| `runtime_dependencies` | `version_constraint` | NULL |
| `instances` | `updated_at` | waktu commit |
| `instances` | `config_revision_id` | NULL |
| `instances` | `ownership` | `created` |
| `instance_endpoints` | `enabled` | 1 |
| `instance_paths` | `managed` | 1 |
| `environments` | `manifest_location`, `lock_sha256`, `last_operation_id` | NULL |
| `environments` | `created_at`, `updated_at` | waktu commit |
| `operations` | `recovery_hint` | NULL |
| `provider_revisions` | `created_at` | waktu commit |
| `provider_revisions` | `source_id`, `approved_at` | NULL |
| `artifacts` | `created_at` | waktu commit |
| `artifacts` | `signature_info`, `dependency_manifest`, `provider_revision_pk` | NULL |

- Field yang ditambahkan ke domain oleh spec ini (lewat Follow-up spec 0004): `revision: ResourceRevision` di `Runtime`, `Environment`, dan `Operation` (mulai 1, naik tepat 1 per mutasi); `retained_by: Option<OperationId>` dan `data_version: ExactVersion` di `Instance`.
- `CapabilitySet` disimpan sebagai array JSON kode yang terurut dan unik. `LineCompatibility` disimpan sebagai object JSON dengan kunci terurut dan array nilai terurut.
- `PathBuf` (misalnya `External { root }`) yang bukan UTF-8 ditolak saat tulis dengan `NotUtf8Path`.
- Konstruktor `restore` di domain (`Runtime::restore`, `Instance::restore`, `Operation::restore`, `Lease::restore`, `Environment::restore`, `ProviderRevision::restore`) menerima semua field yang tersimpan dan memeriksa invariant antar field (misalnya retained wajib punya `retained_at` dan `retained_by`, lifecycle yang butuh data wajib punya `data_initialized_at`). `Instance::restore` memasang readiness dan health `unknown` dengan reason `daemon_restarted` dan `observed_at` sama dengan parameter `loaded_at`, yang selalu `OpenSummary::opened_at`, supaya observasi probe setelah open tidak dianggap basi.

### SqliteRegistry

- Satu thread bernama `stackly-registry` memegang satu koneksi tulis. `SqliteRegistry` adalah handle yang bisa di clone, berisi `SyncSender<Command>` dengan kapasitas `REGISTRY_QUEUE_CAPACITY = 64`; pengiriman memblokir saat penuh (backpressure, NFR-004). Setiap perintah membawa channel balasan berkapasitas 1. Handle juga memegang `Arc<dyn Clock>` yang diambil dari `Platform` saat open.
- Baca dijalankan dalam transaksi baca sendiri, tulis dalam `BEGIN IMMEDIATE`. Satu perintah adalah satu transaksi; tidak ada transaksi yang hidup di antara dua perintah.
- Handle terakhir yang di drop mengirim `Shutdown`, thread menjalankan `PRAGMA optimize` lalu menutup koneksi, dan handle menunggu thread selesai. Thread yang mati (panic) membuat setiap panggilan berikutnya `Unavailable`; daemon memperlakukannya sebagai fatal (fitur 6).
- Error SQLite dipetakan: `SQLITE_FULL` menjadi `NoSpace`, `SQLITE_BUSY` menjadi `Busy`, constraint menjadi `Integrity`, error IO lain menjadi `Io`.
- Log: open, migrasi (dari, ke, durasi, path snapshot), dan recovery tercatat di level info atau error lewat `tracing`; isi baris dan nilai kolom tidak pernah dicatat.

### Fake dan conformance

- `FakeRegistry::new(clock: Arc<dyn Clock>, loaded_at: Timestamp)` di `stackly-testkit`: state di memori di balik satu `Mutex`, menerapkan batch secara atomik (salin, terapkan, tukar), dengan generation, CAS, urutan cek, dan semua error bertipe yang sama dengan `SqliteRegistry`. Fault injection `fail_next_commit(error)`.
- `RegistrySeed` (trait di testkit, khusus test): `seed_backup(BackupSeed)`, `seed_audit_events(count, at)`, `seed_observations(instance, count, at)`, `seed_secret_reference(SecretReferenceSeed)`, untuk tabel yang belum punya `WriteOp`. `FakeRegistry` mengimplementasikannya di memori; test storage mengimplementasikannya untuk pembungkus yang menulis lewat koneksi rusqlite kedua ke file yang sama. Seeding tidak menaikkan generation.
- Builder fixture di testkit: `RuntimeBuilder`, `InstanceBuilder`, `EnvironmentBuilder`, `LeaseBuilder`, `OperationBuilder`, `ProviderRevisionBuilder`, masing masing dengan default yang valid dan ID baru per panggilan.
- `registry_conformance(registry: &dyn Registry, seed: &dyn RegistrySeed, clock: &FakeClock)` menjalankan satu set skenario yang sama untuk implementasi apa pun dan panic dengan nama skenario yang gagal. Registry yang diuji dibuat dengan `FakeClock` yang sama. Testkit menjalankannya untuk `FakeRegistry`; `stackly-storage` menjalankannya untuk `SqliteRegistry` di `crates/storage/tests/`.

### Fixture registry versi sebelumnya

- `crates/storage/tests/fixtures/registry-v<N>.sql`: dump SQL teks dari registry versi N berisi data contoh (`PRAGMA application_id`, `PRAGMA user_version`, `CREATE` sesuai `sqlite_schema`, lalu `INSERT` per tabel urut kunci). Ditulis oleh test `write_registry_fixture` yang langsung selesai kecuali `STACKLY_WRITE_REGISTRY_FIXTURE=1` di-set (dijalankan dengan tangan saat versi baru dibuat; bukan `#[ignore]`, karena spec 0002 hanya mengizinkan `#[ignore]` untuk test native) yang menerapkan migrasi 1 sampai N, menjalankan `tests/fixtures/seed/v<N>.sql` dengan ID dan waktu tetap, lalu men-dump-nya dengan dumper kecil di test (tanpa tool luar).
- `FIXTURE_SHA256: &[(u32, &str)]` di test mencatat sha256 setiap fixture; fixture yang berubah membuat test gagal. Test lain memastikan setiap versi di `MigrationSet::builtin()` punya fixture.
- Di fitur ini hanya ada v1. Jalur migrasi dibuktikan dengan dua set migrasi test: v2 yang menambah satu kolom dan me-rebuild `provider_revisions` dengan 12 langkah (tabel itu punya trigger dan menjadi induk foreign key, jadi index, trigger, dan foreign key ke sana harus utuh setelahnya), dan v2 yang gagal setelah perintah pertamanya berhasil.

### Start daemon

`stacklyd` (sampai fitur 6 membangun server): resolve root seperti sekarang, `prepare_layout` (lebih awal dari rencana fitur 6, karena registry butuh foldernya), `open_registry(platform, prepared.layout(), daemon_info().version)`, lalu:

- `Ready`: `prune(&RetentionPolicy::DEFAULT, clock.now())`, catat `registry_state=ready`, `schema_version`, `created`, `migrated_from`, `snapshot`, `sqlite_version`, dan jumlah prune, keluar 0 (`// provisional: F6`).
- `Recovery`: catat `registry_state=recovery`, kode reason, dan path laporan di level error, keluar 3 (`// provisional: F6`; fitur 6 membuat daemon tetap hidup dalam recovery mode).
- `StorageError` atau kegagalan `prepare_layout`: catat dan keluar 1.
- Flag tersembunyi `--roots-only` (`// provisional: F6`): catat keempat root seperti sekarang lalu keluar 0 tanpa `prepare_layout` dan tanpa registry. Hanya test resolusi home tanpa `STACKLY_ROOT` yang memakainya, supaya tidak ada test yang menulis ke folder Application Support asli.
- Test di `apps/daemon/tests/root_override.rs` diperbarui: test home memakai `--roots-only`; test override menerima beberapa baris log dan memeriksa baris root serta baris `registry_state`, bukan satu baris saja, dan tidak lagi mengharapkan root kosong.

### Recovery mode (kontrak untuk fitur 6)

Saat open menghasilkan `Recovery`, daemon tidak membaca atau menulis data registry, tidak mengadopsi, menyinyal, atau menjalankan proses instance apa pun, dan tidak membuat registry baru. Ia hanya menjawab method yang bertanda `allowed_in_recovery` (spec 0006 AC-18): handshake, status daemon (dengan `RecoveryReport` sebagai `DaemonInfo.recovery`), dan doctor saat fitur 34 menambahkannya; setiap method lain ditolak dengan `registry.recovery_mode`. Reason `registry_locked` dicoba ulang daemon tiga kali sebelum masuk recovery mode (spec 0006). Jalan keluar ada di fitur 48 (restore dari snapshot atau backup) dan fitur 57 (adoption), dan daemon baru keluar dari recovery lewat restart setelah registry diperbaiki.

### State open registry

| Dari | Kondisi | Ke |
| --- | --- | --- |
| start | file registry ada dan semua cek lolos tanpa migrasi tertunda | `ready` |
| start | file ada, ada migrasi tertunda, snapshot dan migrasi sukses | `ready` (`migrated_from`) |
| start | file tidak ada dan root bersih | `ready` (`created`) |
| start | cek atau migrasi gagal, atau file tidak ada dengan sisa | `recovery(reason)` |
| `recovery` | restart daemon setelah registry diperbaiki dari luar | start lagi |

### API surface

| Fungsi | Tempat | Input utama | Output utama | Pemanggil | Error utama |
| --- | --- | --- | --- | --- | --- |
| `open_registry` | storage | `&Platform`, `&DirectoryLayout`, versi app | `Ready { registry, summary }` atau `Recovery(report)` | `stacklyd` saat start | `StorageError` (fatal) |
| `Registry::*` baca | domain (trait), storage, testkit | ID, nama, filter, query | `Versioned<T>` | core dan daemon (fitur 6, 7, 10, 14, 20, 55) | `NotFound`, `Corrupt`, `Unavailable`, `Busy` |
| `Registry::commit` | domain, storage, testkit | `WriteBatch` | `Committed` | engine operasi, use case create, rename, start | `BatchError { op_index, error }` dengan error bertipe di *Port registry* |
| `Registry::prune` | domain, storage, testkit | `RetentionPolicy`, `now` | `PruneReport` | daemon saat start, lalu berkala (fitur 6, 35) | `Unavailable`, `Io` |
| `instance_path_entries` | platform | `InstanceId` | tujuh `InstancePathEntry` | `SqliteRegistry` dan `FakeRegistry` saat `InsertInstance` | tidak ada (murni) |
| `RootSet::resolve` | platform | `LogicalRoot`, `&RelativePath` | `PathBuf` | siapa pun yang memakai path tersimpan | tidak ada (murni) |
| `registry_conformance` | testkit | `&dyn Registry`, `&dyn RegistrySeed`, `&FakeClock` | panic dengan nama skenario | test testkit dan storage | tidak ada |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| open | path registry, `-wal`, `-shm` | `DirectoryLayout::registry_db()` ditambah akhiran |
| open | folder snapshot dan laporan | `registry_snapshots_dir()`, `registry_recovery_dir()` |
| open | bukti `registry_missing` | `Filesystem::symlink_metadata` untuk `-wal`, `-shm`, `-journal`, dan `Filesystem::read_dir` atas folder data standar dari `DirectoryLayout` serta kedua folder di atas |
| open | versi app | `stackly_core::daemon_info().version`, diteruskan `stacklyd` |
| open | versi schema app, sha256 migrasi | `MigrationSet::builtin().migrations`, sha256 teks SQL |
| open | sidik jari yang diharapkan | `MigrationSet::builtin().fingerprints` |
| open | `application_id` | konstanta `0x53544B59` |
| open | versi SQLite | `rusqlite::version()` |
| create | `registry_id` | `RegistryId::generate()` |
| create, migrasi, laporan, snapshot | waktu | `Clock::now` dari `Platform` |
| snapshot | sha256 | sha256 file snapshot setelah sync |
| laporan | sha256 dan ukuran file | file registry di disk saat deteksi; untuk `backup_failed` dan `migration_failed` setelah koneksi tulis ditutup |
| open | `opened_at` | `Clock::now` di langkah 8 |
| `InsertInstance` | baris path | `stackly_platform::instance_path_entries(id)`, dihitung registry sendiri |
| `InsertInstance` | `provider_revision_pk` | dicari dari `ProviderRevisionKey` instance di transaksi yang sama, `MissingReference` kalau tidak ada |
| `InsertInstance` | `ownership` | konstanta `created` di fitur ini |
| setiap commit | `created_at`, `updated_at` yang tidak ada di aggregate | satu `clock.now()` per commit dari `Arc<dyn Clock>` milik registry |
| `InsertInstance`, `UpdateEnvironment` | `position` endpoint dan member | indeks di `Vec` aggregate |
| `UpdateX` | `expected` | revision yang dibaca pemanggil (dari client lewat fitur 6, atau dari baca sendiri) |
| `commit` | generation baru | `registry_info.generation` ditambah 1 di transaksi yang sama |
| baca | `generation` | `registry_info.generation` di transaksi baca yang sama |
| `known_instance_ids` | daftar ID | semua `instances.id` |
| `runtime_references` | `instances` | `instances.runtime_id = id`, semua lifecycle |
| `runtime_references` | `dependent_runtimes` | `runtime_dependencies.runtime_id` dengan `dependency_runtime_id = id` |
| `runtime_references` | `active_operations` | `operations` dengan `resource_kind = 'runtime'`, `resource_id = id`, state bukan state akhir |
| `runtime_references` | `recovery_snapshots` | `backups.runtime_id = id` |
| restore instance | readiness, health | `unknown`, reason `daemon_restarted`, `observed_at` = `OpenSummary::opened_at` (fake: `loaded_at` di konstruktornya) |
| prune | `now` | `Clock::now` di daemon |
| prune | batas | `RetentionPolicy::DEFAULT` sampai Settings (fitur 32, 35) mengisinya |
| tampilan waktu lokal | zona waktu | bukan di registry: CLI (fitur 8, tzdb sistem lewat jiff) dan GUI (fitur 13) |

### Key invariants

- Hanya `stackly-storage` yang membuka file registry, dan hanya `stacklyd` yang memakai `stackly-storage` (dijaga `check-deps`, BR-011).
- Registry yang ada tidak pernah dibuat ulang, dikosongkan, atau ditimpa oleh open; registry baru dipublikasikan lewat `hard_link` yang gagal kalau tujuan ada. Satu satunya file yang dihapus storage adalah file sementara create yang kuncinya bebas, file `.tmp` snapshot miliknya sendiri, dan snapshot lama di luar tiga terbaru yang tidak disebut laporan recovery.
- Keputusan recovery diambil dari koneksi read only, jadi open yang berakhir di recovery tidak mengubah byte `stackly.sqlite` atau `-wal`.
- Migrasi berjalan seluruhnya atau tidak sama sekali, dan hanya setelah snapshot terverifikasi ada.
- Setiap commit adalah satu transaksi yang menaikkan generation tepat 1; setiap update aggregate memakai CAS revision.
- Foreign key aktif di setiap koneksi tulis di luar langkah migrasi; durability `synchronous=FULL` dengan `fullfsync` tidak boleh dilemahkan, termasuk untuk demo atau test performa.
- Tidak ada kolom yang menyimpan nilai secret; secret hanya muncul sebagai baris `secret_references`, pemakaiannya di `secret_reference_uses`, dan teks reference di `config_values`.
- Semua waktu UTC 30 karakter dari `Clock`; tidak ada waktu lokal atau waktu dari SQLite.
- Path instance tersimpan relatif terhadap root bernama, dan resolve-nya untuk instance di fitur ini selalu sama dengan `DirectoryLayout`; perbedaan adalah drift yang dilaporkan, bukan dipakai diam diam.
- Readiness dan health tidak pernah dibaca dari registry.

### Security model

- Batasnya satu akun user OS (BR-012). File registry, `-wal`, `-shm`, snapshot, dan laporan bermode 0600 di folder 0700 milik user.
- Registry dibuka dengan `SQLITE_OPEN_NOFOLLOW`, dan preflight menolak symlink atau file yang bukan file biasa, supaya user lain tidak bisa membelokkan tulisan lewat symlink.
- `trusted_schema=OFF` mencegah fungsi atau view di schema yang diubah orang lain berjalan dengan hak daemon; sidik jari schema mendeteksi perubahan itu.
- Registry tidak berisi secret, jadi snapshot dan laporan recovery aman disalin untuk diagnosis, tetapi tetap berisi nama instance dan path, sehingga diperlakukan privat.
- CLI dan GUI tidak pernah membuka file SQLite (BR-011).

### Critical test scenarios

- Create bersih: open di `TestRoot` membuat registry, pragma terbaca benar, mode file 0600, `registry_info` terisi dengan waktu `FakeClock`, schema cocok dengan peta PRD. Memverifikasi **AC-1**, **AC-2**, **AC-3**.
- Create terputus dan bersamaan: file `.stackly.sqlite.creating` sisa yang kuncinya bebas dihapus lalu create berjalan normal; file sementara yang dikunci thread lain menghasilkan `CreateInProgress`; dua open bersamaan di root bersih menghasilkan paling banyak satu `created: true` dan tidak ada registry yang tertimpa. Memverifikasi **AC-3**.
- Migrasi maju: fixture v1 ditambah set migrasi test v2 membuat snapshot terverifikasi, data tetap, sidik jari v2 cocok; open kedua tanpa snapshot baru; lima migrasi berturut turut menyisakan tiga snapshot. Memverifikasi **AC-4**.
- Migrasi gagal di tengah: v2 yang gagal setelah satu `ALTER` berhasil meninggalkan registry sama persis dengan v1, snapshot dan laporan ada, open kedua tetap recovery. Snapshot gagal (folder `pre-migration` dibuat 0500 sehingga tidak bisa ditulis) menghasilkan `backup_failed` tanpa migrasi. Open kedua memakai ulang snapshot dan laporan yang sama. Memverifikasi **AC-5**.
- Deteksi: setiap reason di AC-6 dipicu (file teks biasa, symlink, file 0000 untuk `open_failed`, halaman rusak lewat penulisan byte acak di salinan, baris `registry_info` dihapus, `user_version` 999, baris `schema_migrations` dihapus, checksum diubah, `CREATE INDEX` manual, registry dihapus dengan `runtimes/` berisi, koneksi lain memegang `BEGIN IMMEDIATE` dengan busy timeout test yang pendek), dan sha256 `stackly.sqlite` serta `-wal` sebelum dan sesudah sama, termasuk saat ada `-wal` yang belum di checkpoint. Memverifikasi **AC-6**.
- Daemon: `stacklyd` dengan `STACKLY_ROOT` keluar 0 dan mencatat ready; setelah 4 byte `user_version` di offset 60 ditambal menjadi 999, keluar 3 dan sha256 file tidak berubah; `--roots-only` tanpa `STACKLY_ROOT` tidak membuat folder apa pun. Memverifikasi **AC-7**.
- CAS dan generation: update dengan revision lama ditolak tanpa efek pada operasi lain di batch; batch dengan generation lama ditolak; generation naik tepat 1 per commit. Memverifikasi **AC-8**.
- Balapan: 16 thread untuk nama sama, artefak sama, dan revision sama di kedua implementasi lewat `registry_conformance`. Memverifikasi **AC-9**.
- Nama, short key, dan path: nama instance retained bisa dipakai lagi; dua ID dengan 8 hex terakhir sama ditolak; rename tidak mengubah path; path tersimpan resolve sama dengan layout. Memverifikasi **AC-10**.
- Referensi runtime: instance `stopped` dan `retained`, runtime dependen, operasi `running`, dan backup semuanya muncul; `DELETE` runtime mentah ditolak foreign key. Memverifikasi **AC-11**.
- Secret: kolom `secret_references` dan `secret_reference_uses` persis daftar AC-12, tidak ada kolom nilai secret di tabel lain, dan smoke test `FixtureCredential` atas file registry yang masih terbuka lulus. Memverifikasi **AC-12**.
- Waktu: insert `2026-10-18T05:42:00+07:00` dan bentuk milidetik ditolak; seribu `Timestamp` acak bolak balik tanpa beda. Memverifikasi **AC-13**.
- Retensi: data dengan umur dan jumlah di atas batas dipangkas sesuai kebijakan, operasi `needs_recovery` berumur 90 hari tetap ada. Memverifikasi **AC-14**.
- Immutability dan kode: `UPDATE provider_revisions SET capabilities` ditolak, `approval` boleh; `UPDATE audit_events` ditolak; lifecycle `paused` ditolak; `kind` operasi tak dikenal yang disisipkan mentah dibaca sebagai `Corrupt`. Memverifikasi **AC-15**.
- Bolak balik: setiap aggregate dan setiap nilai enum state ditulis lalu dibaca sama. Memverifikasi **AC-16**.

Test yang menulis evidence (semuanya `fixture`, spec 0002):

| Test | `covers` |
| --- | --- |
| `failed_migration_enters_recovery_without_touching_the_registry`, `half_applied_schema_is_detected_at_start` | `AC-065` |
| `stale_revision_is_rejected_and_nothing_in_the_batch_applies`, `concurrent_creates_produce_one_instance` | `REQ-048`, `NFR-005` |
| `stored_paths_resolve_to_the_layout_and_never_repeat` | `BR-002` |
| `renaming_keeps_id_runtime_and_stored_paths` | `BR-003` |
| `runtime_references_include_stopped_and_retained_instances` | `BR-007` |
| `registry_never_holds_secret_values` | `BR-018` |
| `provider_revision_manifest_fields_are_immutable` | `BR-017` |
| `prune_keeps_unfinished_operations_and_bounds_history` | `NFR-011` |

## Build plan

Urutan mengikuti pendekatan Journey: jalur "daemon start membuka registry dengan aman" (buat, migrasi, recovery) selesai utuh lebih dulu, lalu jalur "daemon menyimpan dan membaca aggregate dengan aman". Guard evidence dipasang di setiap test yang membuktikan ID PRD saat test itu ditulis (langkah 1 spec 0002 sudah dibangun).

**Milestone 1: Membuka registry dari ujung ke ujung** (bisa mulai sekarang; hanya butuh milestone 1 spec 0004 yang sudah selesai)

1. Crate `stackly-storage` (lints workspace, `//!` doc), `rusqlite` dengan `bundled` di workspace, dan aturan `check-deps` baru beserta test crate fixture yang melanggar. Memenuhi **AC-1**, **AC-2**.
2. Domain: `RegistryId` dan value type path (`LogicalRoot`, `InstancePathKind`, `RelativePath`, `InstancePathEntry`) di modul baru. Platform: fungsi murni `instance_path_entries`, `RootSet::resolve`, `registry_snapshots_dir`, `registry_recovery_dir`. Memenuhi **AC-10**.
3. Modul `files` storage dan pembuka koneksi: set pragma per jenis koneksi beserta verifikasinya, create lewat file sementara yang dikunci lalu `hard_link`, `application_id`. Memenuhi **AC-2**, **AC-3**.
4. Migrasi `0001_registry.sql` (21 tabel termasuk `secret_reference_uses`, index, trigger, CHECK), `MigrationSet`, runner (`schema_migrations`, satu transaksi, `foreign_key_check`), sidik jari dengan normalisasi, test schema terhadap *Data model* dan *Peta field PRD*, dan test index foreign key. Memenuhi **AC-1**, **AC-13**, **AC-15**.
5. Alur open: pembersihan sisa milik storage, preflight, `registry_missing`, inspeksi read only, open tulis dengan probe `BEGIN IMMEDIATE`, snapshot (pakai ulang, verifikasi, nama tanpa menimpa, batas tiga), laporan recovery (dedupe, sha256 setelah koneksi ditutup), `StorageError`, dan `OpenSummary` dengan `opened_at`. Handle `SqliteRegistry` dengan thread, channel berbatas, shutdown, dan pemetaan error SQLite sudah dibuat di sini, tanpa method `Registry`. Memenuhi **AC-2**, **AC-3**, **AC-4**, **AC-5**, **AC-6**.
6. Fixture `registry-v1.sql`, seed v1, dumper, `write_registry_fixture`, `FIXTURE_SHA256`, dan dua set migrasi test v2 (tambah kolom plus rebuild `provider_revisions`, dan yang gagal di tengah). Memenuhi **AC-4**, **AC-5**.
7. `stacklyd`: `prepare_layout`, `open_registry`, log, kode keluar 0, 1, dan 3, flag `--roots-only`, pembaruan test yang ada di `apps/daemon/tests/root_override.rs`, dan test baru dengan tambalan `user_version` di header. Memenuhi **AC-7**. Kalau `stacklyd --print-roots` (spec 0006 langkah 13) sudah ada di main saat langkah ini dibangun, pakai flag itu dan jangan tambah `--roots-only`.

**Milestone 2: Port registry dan fake** (butuh milestone 3 spec 0004: model domain dan state machine)

8. Domain: modul `registry` (trait, `RegistryError`, `BatchError`, batch, `Versioned`, `ResourceRef`, filter, query, cursor, `RetentionPolicy`, `PruneReport`, record), konstruktor `restore`, `revision` di `Runtime`, `Environment`, dan `Operation`, serta `retained_by` dan `data_version` di `Instance`. Memenuhi **AC-8**, **AC-16**.
9. Testkit: `FakeRegistry`, `RegistrySeed`, builder fixture, dan `registry_conformance` (urutan cek, CAS, generation, nama, short key, path, referensi runtime, retensi, balapan 16 thread), dijalankan untuk fake. Memenuhi **AC-8**, **AC-9**, **AC-10**, **AC-11**, **AC-14**.

**Milestone 3: Registry SQLite** (butuh milestone 1 dan langkah 8)

10. `Registry` untuk `SqliteRegistry`: dispatch perintah, batch dalam satu transaksi, urutan cek, generation, dan stempel waktu commit. Memenuhi **AC-8**.
11. Pemetaan baris ke aggregate dan sebaliknya untuk runtime, instance (endpoint dengan `position` dan path), provider revision, artefak, environment (member dengan `position`), lease, dan operasi, termasuk JSON terurut, path bukan UTF-8, dan cek bertipe. Memenuhi **AC-10**, **AC-13**, **AC-15**, **AC-16**.
12. Query: `known_instance_ids`, `runtime_references`, `instance_paths`, `operations` berhalaman, dan `prune` per 5.000 baris. Memenuhi **AC-11**, **AC-14**.
13. `registry_conformance` untuk `SqliteRegistry` dengan `RegistrySeed` lewat koneksi samping, test balapan, smoke test `FixtureCredential` atas file registry yang masih terbuka, dan `stacklyd` memanggil `prune` setelah `Ready`. Memenuhi **AC-7**, **AC-9**, **AC-12**, **AC-14**.
14. Katalog `codes::registry` di `stackly-protocol` dan pemetaan `RegistryError` ke `ErrorBody` di `stacklyd::map`, dengan test yang memetakan setiap varian (butuh milestone 1 spec 0006). Memenuhi **AC-17**.

**Sesi paralel**: Milestone 1 bisa berjalan di satu sesi sekarang, bersamaan dengan milestone 3 spec 0004. Langkah 2 menaruh tipe domain baru di modul baru dan memakai makro `uuid_id!` yang sudah ada, sehingga bentrok dengan sesi spec 0004 hanya mungkin di baris `mod` di `lib.rs`. Setelah langkah 8 masuk, langkah 9 (testkit) dan langkah 10 sampai 12 (storage) bisa berjalan di dua sesi terpisah karena menyentuh crate berbeda; langkah 13 menunggu keduanya.

## Consequences

**Positive**:

- Update yang gagal tidak pernah merusak atau mengganti registry; selalu ada snapshot terverifikasi dan laporan yang menjelaskan apa yang terjadi (AC-065).
- Runtime atau instance ganda dicegah dua lapis: cek bertipe di satu thread penulis dan constraint SQL sebagai penjaga terakhir (NFR-005).
- Fitur 9, 11, 18, 29, 35, dan 49 mendapat tabel dan relasinya sekarang, dan cukup menambah kolom lewat migrasi additive.
- Core dan engine operasi diuji dengan fake yang perilakunya dijaga suite conformance yang sama dengan SQLite.
- Registry bisa dipulihkan di profil atau root lain, karena path instance tersimpan relatif terhadap root bernama.

**Negative / tradeoffs**:

- Tabel untuk fitur tanpa spec bisa salah tebak; setiap koreksi butuh migrasi, dan rebuild tabel kalau menyentuh CHECK.
- Menambah kode ke set tertutup (misalnya state lifecycle baru) butuh rebuild tabel lewat migrasi.
- Rollback ke app lama berarti memulihkan snapshot dan kehilangan perubahan setelah migrasi; tidak ada down migration.
- `fullfsync` membuat setiap commit lebih lambat beberapa milidetik; semua tulis antre di satu thread, jadi baca panjang (misalnya export audit di fitur 35) menunda tulis.
- Sidik jari schema mengikat migrasi pada aturan SQL yang ketat (tanpa `RENAME COLUMN` dan `DROP COLUMN`), dan upgrade rusqlite harus menjalankan ulang test sidik jari dan fixture.
- Recovery mode menghentikan semua pekerjaan sampai pengguna memulihkan registry; sampai fitur 48 dan 57 ada, jalan keluarnya manual.
- Registry yang hilang dengan sisa data tidak dibuat ulang otomatis, jadi developer yang menghapus file registry harus memakai root test baru.
- Model domain harus punya konstruktor `restore`, yang membuka jalan membuat aggregate di state apa pun; pemakaiannya dibatasi ke storage dan testkit lewat dokumentasi dan review.
- `stackly-storage` tidak diuji untuk target Windows sampai fase Windows.
- Storage memakai `std::fs` langsung untuk file miliknya sendiri (create, kunci, `hard_link`, hapus, sync), jadi bagian itu tidak lewat fault injection `FakeFilesystem`; test-nya memakai folder dan izin asli di `TestRoot`.
- Satu generation untuk seluruh registry berarti commit apa pun, termasuk progress operasi, membuat commit lain yang memakai `expected_generation` gagal dan diulang; ini dibatasi lewat batas ulang lima kali dan pembatasan commit progress di spec 0007.

**Neutral**:

- Dependency baru: `rusqlite` (`bundled`) di storage; `sha2`, `serde_json`, `jiff`, `thiserror`, `tracing` dipakai ulang dari workspace.
- `stacklyd` memanggil `prepare_layout` lebih awal dari rencana spec 0004 (fitur 6).
- Kode keluar 3 untuk recovery bersifat sementara sampai server daemon fitur 6.

## Follow-up

Assumed (keputusan milik spec lain yang dipakai di sini):

- [x] Assumed (spec 0006): ada kategori error stabil untuk registry dalam recovery, konflik (revision dan generation), tidak ditemukan, dan tidak tersedia; `stacklyd` memetakan `RegistryError` ke kategori itu. Status daemon membawa `RecoveryReport` sebagai DTO. Cursor event tidak harus sama dengan generation registry. Diselesaikan 2026-10-18: kodenya milik spec ini (*Kode error di wire*, AC-17), recovery mode dan `DaemonInfo.recovery` di spec 0006 AC-18.
- [ ] Assumed (spec 0007): spec 0007 menambah tabel journal dan step serta unique index idempotency (dengan cakupan kuncinya) lewat migrasi versi 2 atau sesudahnya, memakai CAS revision dan generation dari port ini, tidak pernah memegang transaksi SQLite selama langkah panjang, dan membatasi commit yang hanya mengubah progress (misalnya paling banyak satu per operasi per detik) supaya pemanggil yang memakai `expected_generation` tidak terus bentrok.
- [x] Assumed (spec 0009): `capabilities` dan `line_compatibility` diisi dari manifest, `schema_version` adalah apiVersion manifest, dan provider revision fake provider didaftarkan lewat `InsertProviderRevision`. Cocok dengan spec 0009 (dicek 2026-10-18).
- [x] Assumed (spec 0011): referensi secret adalah baris `secret_references` dengan model logis spec 0011 (kolom di *Data model*, plus `secret_reference_uses`); field secret di `config_values` berisi teks reference `local:<path>`, bukan ID; `safe_details` audit sudah diredaksi pemanggil, dan `result` audit hanya `succeeded` atau `failed`. Diselaraskan 2026-10-18.

Perubahan yang dibutuhkan di spec 0001 (lewat `/architect`, bukan dari sesi paralel):

- [x] *Batas keputusan*: tandai baris Fitur 5 sudah diputuskan di spec 0005; port registry ada di `stackly-domain::registry`. Diterapkan di spec 0001 (2026-10-18).
- [x] *Peta crate*: `stackly-storage` bergantung pada `stackly-domain`, `stackly-platform`, `rusqlite` (`bundled`), `sha2`, `serde`, `serde_json`, `jiff`, `thiserror`, dan `tracing`, tidak pada `stackly-core`; baris domain menyebut modul port `registry`; baris testkit menyebut `FakeRegistry` dan `registry_conformance`; `stacklyd` bergantung pada `stackly-storage`. Diterapkan di spec 0001 (2026-10-18).
- [x] *Aturan arah dependency*: tambah tiga aturan `check-deps` di atas, dan hapus pengecualian `unsafe` "`stackly-storage` bila terbukti perlu" karena storage tidak memakai `unsafe`. Diterapkan di spec 0001 (2026-10-18).
- [x] Baris *Registry* di *Proposed stack*: tambahkan WAL, `synchronous=FULL`, `fullfsync`, dan satu thread untuk baca dan tulis. Diterapkan di spec 0001 (2026-10-18).

Perubahan yang dibutuhkan di spec 0004 (lewat `/architect`, bukan dari sesi paralel):

- [x] Modul domain `registry` (port, error, batch, record, retensi) dan ID `RegistryId`. Dicatat di spec 0004 sebagai titik perluasan fitur 5 (2026-10-18).
- [x] Field `revision: ResourceRevision` di `Runtime`, `Environment`, dan `Operation` (mulai 1, naik tepat 1 per mutasi), dan `retained_by: Option<OperationId>` di `Instance` (cermin `retained.byOperation` di manifest). Dicatat di spec 0004 sebagai titik perluasan fitur 5 (2026-10-18).
- [x] Nama tipe resource operasi di domain: `OperationResource` (`runtime`, `instance`, `environment`, `manager`), dipakai `OperationQuery`. Sudah dibangun fitur 4 dan dicatat di spec 0004 (2026-10-18).
- [x] Value type `LogicalRoot`, `InstancePathKind`, `RelativePath`, `InstancePathEntry`; fungsi murni `instance_path_entries(InstanceId)`, fungsi layout `registry_snapshots_dir` dan `registry_recovery_dir`, dan `RootSet::resolve`. Dicatat di spec 0004 sebagai titik perluasan fitur 5 (2026-10-18).
- [x] Konstruktor `restore` untuk `Runtime`, `Instance`, `Operation`, `Lease`, `Environment`, dan `ProviderRevision`, termasuk readiness dan health `unknown` reason `daemon_restarted` saat dimuat. Dicatat di spec 0004 sebagai titik perluasan fitur 5 (2026-10-18).
- [x] Field `data_version: ExactVersion` di `Instance` (cermin `data.initializedVersion` di manifest), yang diubah `record_data_migration`. Sudah dibangun fitur 4 dan dicatat di spec 0004 (2026-10-18).
- [x] Tandai follow-up fitur 5 di spec 0004 selesai: kode state dipetakan ke tabel, `ResourceRevision` dipakai untuk CAS, `known` dari `known_instance_ids`, dan `RuntimeReferences` dari `runtime_references`. Dicentang di spec 0004 (2026-10-18).
- [x] Catat bahwa `stacklyd` memanggil `prepare_layout` sejak fitur 5. Dicatat di spec 0004 sebagai titik perluasan fitur 5 (2026-10-18).

Untuk fitur lain:

- [ ] Fitur 6: server daemon tetap hidup dalam recovery mode sesuai kontrak di atas (mengganti kode keluar 3), jalankan `prune` berkala, dan perlakukan `Unavailable` sebagai fatal. Dirancang spec 0006 (AC-18, langkah 18a).
- [ ] Fitur 8 dan 13: tampilkan semua waktu registry dalam zona waktu lokal pengguna (bagian "ditampilkan sesuai locale" di Done when fitur 5 dipenuhi di sana).
- [ ] Fitur 20: panggil `known_instance_ids` sebelum `create_instance_layout`, lalu `InsertInstance` dalam batch yang sama dengan progress operasi; cek konflik endpoint dengan `endpoints_conflict` atas data yang dibaca, commit dengan `expected_generation` dari baca itu, dan ulangi paling banyak lima kali saat `GenerationConflict`.
- [ ] Fitur 29: putuskan kode `status` config revision dan apakah endpoint boleh dinonaktifkan (kolom `enabled`; domain `Endpoint` belum punya field itu).
- [ ] Fitur 32 dan 35: simpan `RetentionPolicy` di `settings` dan tampilkan di Settings; fitur 32 memakai `logical_root` baru kalau path data untuk instance baru boleh diganti.
- [ ] Fitur 48: restore registry dari snapshot `pre-migration` atau backup, dan tampilkan `RecoveryReport` di GUI dan CLI.
- [ ] Fitur 54: tambah `retained_instance_path_entries` di platform dan `WriteOp` untuk retain (menulis ulang baris path ke `retained/<id>` bersama `retained_at` dan `retained_by`) serta purge.
- [ ] Fitur 61: rollback app memakai snapshot `pre-migration` dan menolak rollback buta saat `schema_newer_than_app`.
- [ ] Ukur NFR-001 dengan 100 instance di checkpoint performa akhir fase 04; tambah pool baca hanya kalau satu thread terbukti menjadi hambatan.
- [ ] `/sync` setelah build: catat crate `stackly-storage`, aturan migrasi (tanpa `RENAME COLUMN` dan `DROP COLUMN`, fixture beku per versi), dan aturan `check-deps` baru di root `AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, dan keputusan kecil ada di [rationale.md](rationale.md).
