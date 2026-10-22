# Verify: Provider PostgreSQL · spec 0019 · updated 2026-10-21

_Langkah diturunkan dari acceptance criteria spec 0019. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Milestone 1 tidak butuh server PostgreSQL, artefak runtime, atau cache runtime: semuanya dibuktikan di fixture._

## Milestone 1: Package and additive core changes on fixtures

### UI / manual

- [x] Buka `providers/postgresql/` → berisi persis `provider.yaml`, `versions.json`, `schemas/settings.schema.json`, `templates/pg_hba.conf.tmpl`, `templates/pg_ident.conf.tmpl`, `templates/postgresql.conf.tmpl`, dan `README.md`; `provider.yaml` belum memuat blok `connection` (milik spec 0021) dan belum memuat `runtime.versionCheck` (milik langkah 7) → AC-1
- [x] Buka `providers/postgresql/versions.json` → tepat dua artefak `available`: `postgresql-18.6-r2-macos-aarch64` line `18` dan `postgresql-17.11-r2-macos-aarch64` line `17` → AC-1
- [x] Buka `providers/postgresql/schemas/settings.schema.json` → `superuserPassword` bertanda `secret: true` dan `printableOnly: true`, panjang 8 sampai 128, `apply` `immutableAfterInit`; field lain sesuai tabel *Settings schema* → AC-4
- [x] Buka `crates/core/tests/golden/provider/postgresql/` → `init.json`, `run.json`, `reload.json`, dan tiga `render-*.txt`; di `init.json` langkah `postgres --single` membawa `stdin` bertipe `text` dengan empat statement, satu per baris → AC-3, AC-12

### Commands

- [x] `cargo xtask provider check` → satu baris `ok official/postgresql 1.0.0 <sha256>` di antara baris paket lain → AC-1
- [x] `cargo xtask codegen --check` → "up to date", termasuk `providers/postgresql/versions.json` dan `schemas/provider/settings.v1alpha1.schema.json` (memuat `printableOnly`) → AC-1, AC-4
- [x] `cargo test -p stackly-core --lib provider::template` → escaper `postgresqlConf` (kutip, backslash, Unicode, boolean `on` atau `off`, karakter kontrol ditolak) dan format `plain` (tanpa tag output dan blok) lulus → AC-2
- [x] `cargo test -p stackly-core --test provider_rules` → 47 test lulus, termasuk `postgresql_conf_renders_quoted_text_decimal_integers_and_on_off_booleans`, `plain_templates_keep_text_and_comments_and_reject_output_and_block_tags`, `stdin_text_is_literal_one_key_bounded_and_free_of_tokens_and_most_control_characters`, `stdin_text_reaches_the_plan_exactly_and_changes_the_digest`, dan `the_settings_dialect_rejects_keywords_and_rules_outside_it` (termasuk `printableOnly` pada field bukan secret ditolak `settings_schema_rule` di `properties.label.x-stackly.printableOnly`) → AC-2, AC-3, AC-4
- [x] `cargo test -p stackly-core --test secret_service printable` → 8 test lulus, termasuk `from_setting_fills_printable_only_from_the_settings_schema` dan `create_with_a_schema_printable_only_field_refuses_control_characters` (nilai ber `\n` ditolak `secret.invalid_value` tanpa panggilan ke `FakeSecretStore`, nilai ber spasi, `'`, `"`, dan `\` lolos) → AC-4
- [x] `cargo test -p stackly-core --test provider_packages postgresql` → 7 test lulus: paket terbaca tanpa tulis dan valid, plan dan tiga render sama dengan golden, program plan adalah `bin/initdb`, `bin/postgres`, `bin/postgres`, `bin/psql` di root runtime dan tidak ada argumen `postgres` atau `…=postgres`, `Stdin::Text` empat baris, `pg_hba.conf` tanpa `trust` dan `superuser-password` ada di `cleanup`, `superuserPassword` menghasilkan `SecretFieldRule` dengan `printable_only`, dan nilai teks di `postgresql.conf` terkutip → AC-1, AC-3, AC-4, AC-12
- [x] `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test provider_packages postgresql`, lalu `git status` → tidak ada perubahan pada file golden (golden stabil) → AC-1
- [x] `cargo xtask fuzz seed` → seed `provider_template` tidak berubah tak terduga; harness template sudah memuat format `postgresqlConf` dan `plain` sejak langkah 1 → AC-2
- [x] `cargo xtask check-deps` → lulus; `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → exit 0 → AC-1

### Acceptance-criteria coverage

- AC-1 · bagian paket: `provider check`, `versions.json` dua artefak, golden render dan plan; bagian catalog (`providers/catalog.toml`) menunggu Milestone 3 langkah 9
- AC-2 · bagian format dan validasi `plain`: langkah 1; round trip `postgres -C log_line_prefix` menunggu Milestone 2 langkah 8
- AC-3 · bagian plan: langkah 2 (validasi, `Stdin::Text`, digest) dan golden langkah 4; arm executor init dan byte yang diterima proses menunggu Milestone 2 langkah 5 dan 8
- AC-4 · bagian fixture: `printableOnly` di model settings dan `SecretFieldRule::from_setting` dengan `FakeSecretStore`; login dengan password berkarakter khusus menunggu Milestone 2 langkah 8
- AC-12 · bagian plan: program `ExecutablePath` di root runtime; `PATH` umpan menunggu Milestone 2 langkah 8
- AC-5 sampai AC-11, AC-13 sampai AC-15 · belum, milik Milestone 2 dan 3

## Milestone 2: PostgreSQL 18 end to end through the daemon

_Semua langkah di bagian ini native: butuh kedua artefak PostgreSQL (17.11 dan 18.6) di cache runtime xtask (`~/Library/Caches/dev.stackly.xtask/runtime/<sha256>`, spec 0003), dan tidak mengunduh apa pun. Tanpa artefak, test berakhir `BLOCKED` dengan alasan `artifact <id> is not in the runtime cache` dan owner `@ojiepermana`. Setiap test memakai root, port, dan guard `ReapGuard` sendiri. Test fungsi `postgresql_18_*` hanya mencakup PostgreSQL 18; PostgreSQL 17 menunggu Milestone 3._

### UI / manual

- [x] Baca `target/evidence/<runId>/report.json` dari perintah evidence di bawah → 13 record `native`, semuanya `PASS`, 0 problem, masing masing dengan subject `postgresql` versi dari `SHOW server_version` (`18.6`) dan digest artefak → AC-14 (bagian guard; `compatibility.json` menunggu Milestone 3)
- [x] Cek proses setelah run: `ps -axo command | grep stackly-test` → tidak ada proses `postgres` milik test yang tersisa (guard `ReapGuard` dan stop di akhir tiap test) → AC-13

### Commands

- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test postgresql --locked -- --ignored` → `13 passed`, `evidence: 13 native records (… PASS 13, FAIL 0, BLOCKED 0 …), 0 problems` → AC-2, AC-4, AC-5, AC-6, AC-7, AC-8, AC-9, AC-10, AC-12, AC-13, AC-15
- [x] `cargo test -p stacklyd --test postgresql -- --ignored lifecycle` → `postgresql_18_is_created_started_served_and_stopped_through_the_daemon` lulus: `data.initializedLine` `18` di `instance-manifest.json`, `instance.get` memisahkan lifecycle, desired, readiness, health, dan process, `LogSink` perekam menerima baris bercap `UTC`, baris tabel `kept` masih ada setelah stop dan start → AC-5, AC-13
- [x] `cargo test -p stacklyd --test postgresql -- --ignored security` → tiga test lulus: tanpa `trust` di kedua `pg_hba.conf`, superuser tanpa password ditolak di socket dan TCP, `stackly_readiness` ditolak lewat TCP dan ke `template1`, koneksi `replication=true` ditolak dengan `no pg_hba.conf entry for replication connection` dan `stackly_readiness` dengan `replication=database` ditolak, password tidak ada di argv, file, atau log; password `<hex> 'x"y\z` bisa login dan password ber baris baru ditolak `secret.invalid_value` tanpa panggilan tulis ke Keychain; locale `C.UTF-8` builtin, `UTF8`, checksum `on`, zona `UTC` di bawah `LANG=de_DE.UTF-8` dan `TZ=Asia/Jakarta` → AC-4, AC-6, AC-7
- [x] `cargo test -p stacklyd --test postgresql -- --ignored endpoints` → satu listener TCP di `127.0.0.1:<port>` dan satu socket 0700 di folder 0700 dengan file lock; `ALTER SYSTEM SET port` tidak menggeser port; port `Auto` tersimpan dan nama socket mengikutinya → AC-8
- [x] `cargo test -p stacklyd --test postgresql -- --ignored readiness` → atribut role `stackly_readiness` sesuai, `CREATE TABLE` oleh role itu gagal, `ALTER ROLE … NOLOGIN` membuat `not_ready` dengan `not permitted to log in` selagi TCP tetap menerima, `LOGIN` mengembalikan `ready` → AC-9
- [x] `cargo test -p stacklyd --test postgresql -- --ignored stop` → dengan `SELECT pg_sleep(600)` berjalan, stop selesai di bawah 10 detik, `postmaster.pid`, socket, dan lock nya hilang, log memuat `received fast shutdown request`, start berikutnya mencatat `database system was shut down at` tanpa `not properly shut down` → AC-10
- [x] `cargo test -p stacklyd --test postgresql -- --ignored failure` → folder data berisi `stray.txt` membuat `initdb` gagal, operasi `needs_recovery` dengan `supervisor.init_failed`, instance `failed` dengan `init_step_failed`, file utuh, `superuser-password` tidak ada, start ditolak `supervisor.not_initialized`, lalu abandon → AC-5
- [x] `cargo test -p stacklyd --test postgresql -- --ignored tools` → dengan `psql`, `initdb`, `postgres`, dan `pg_ctl` palsu di depan `PATH` daemon, create, start, readiness, dan stop berhasil dan file penanda tidak pernah dibuat → AC-12
- [x] `cargo test -p stacklyd --test postgresql -- --ignored adoption` → `SIGKILL` ke `stacklyd` saat `ready`, daemon baru mengadopsi postmaster yang sama (PID dan waktu start sama) dan readiness kembali `ready`; `lsof -d 3` pada postmaster dan child nya menunjukkan `instance.lock` → AC-13
- [x] `cargo test -p stacklyd --test postgresql -- --ignored reload` → setelah revisi config baru dengan `logStatement` `all`, `execute_reload` membuat `SHOW log_statement` bernilai `all` dengan `pg_postmaster_start_time()` dan PID yang sama → AC-13
- [x] `cargo test -p stacklyd --test postgresql -- --ignored format` → `postgres -C log_line_prefix` dari runtime 18 membaca kembali `a'b\c d` dan `ü 空` persis seperti yang diberikan ke format `postgresqlConf` → AC-2
- [x] `cargo test -p stacklyd --test postgresql -- --ignored install` → kedua artefak terpasang lewat pipeline install dengan exec check `runtime.versionCheck`, dan `bin/postgres --version` mencetak `postgres (PostgreSQL) 17.11` dan `18.6` → AC-15
- [x] `cargo test -p stackly-core --test provider_packages postgresql` → 8 test lulus, termasuk `the_postgresql_version_check_reads_the_version_of_each_artifact_from_its_line` (pattern cocok dengan `postgres (PostgreSQL) 18.6` dan `17.11`, tidak cocok dengan awalan, akhiran, atau versi tanpa menit) → AC-15
- [x] `cargo test -p stackly-core --lib supervisor::steps` dan `cargo test -p stackly-core --test supervisor_start text_stdin` → arm executor init `Stdin::Text` menulis byte persis, teks yang tidak bisa menjadi `SecretValue` gagal tanpa menyebut teksnya, dan langkah yang gagal menjadi `init_step_failed` → AC-3
- [x] Jalur `BLOCKED`: `HOME=/nonexistent target/debug/deps/postgresql-<hash> --ignored lifecycle` (binary test yang sudah dibangun) → test lulus karena ditandai `BLOCKED` oleh guard evidence, tanpa mengunduh apa pun → AC-14
- [x] `cargo fmt --all --check`, `cargo clippy -p stacklyd -p stackly-core --all-targets --locked -- -D warnings`, `cargo xtask check-deps` → lulus → AC-12

### Acceptance-criteria coverage

- AC-2 · round trip `postgres -C log_line_prefix` untuk `a'b\c d` dan `ü 空` (`format`); Milestone 1 sudah membuktikan escaper
- AC-3 · arm executor init dan byte yang diterima langkah (`supervisor::steps` unit test, `supervisor_start` `text_stdin`) dan role `stackly_readiness` yang benar terbentuk di cluster nyata (`readiness`)
- AC-4 · login dengan password berspasi, `'`, `"`, dan `\`, dan penolakan baris baru lewat create (`security`); evidence pemeriksaan di `put` ada di spec 0011
- AC-5 · init berurutan dan `superuser-password` hilang setelah init (`lifecycle`); init gagal di langkah `initdb` dengan operasi `needs_recovery` lalu abandon (`failure`). Test `failure` menyisipkan file di folder data setelah tahap `layout` lewat watcher, karena pemeriksaan isi folder spec 0020 belum ada; langkah 9 spec 0020 mengubahnya ke operasi `failed` dengan kompensasi penuh (D9)
- AC-6 · `security`: koneksi `replication=true` ditolak dan `stackly_readiness` tidak bisa membuka `replication=database`
- AC-7 · `security` (daemon biner dengan `LANG` dan `TZ` asing)
- AC-8 · `endpoints`
- AC-9 · `readiness`; bagian detail `check_failed` memakai pesan PostgreSQL asli
- AC-10 · `stop`
- AC-12 · `tools` (daemon biner dengan `PATH` umpan) dan golden Milestone 1
- AC-13 · `lifecycle`, `reload`, `adoption`, dan `tools`; baris log lewat `DaemonConfig.extra_log_sinks`
- AC-14 · guard `native` pada setiap test dan jalur `BLOCKED`; `compatibility.json`, `covers` BR-025 per mayor, dan `cargo xtask runtime compat-record` menunggu Milestone 3 langkah 11
- AC-15 · pattern (`provider_packages`) dan exec check install untuk kedua artefak (`install`)
- AC-1 bagian catalog, AC-5 sampai AC-13 untuk PostgreSQL 17, AC-11 · belum, milik Milestone 3

### Penyimpangan dari spec (dicatat untuk lead)

- `covers` memakai `REQ-035`, `AC-035`, `BR-009`, `VAL-018`, dan `BR-025`; `JOURNEY-01` ditolak guard evidence (bukan ID PRD tiga digit).
- Reload tidak punya method IPC sebelum fitur 29, jadi test menghentikan daemon dalam proses (postmaster tetap hidup), membangun `Supervisor` milik test di registry yang sama, mengadopsi postmaster lewat `reconcile`, menulis revisi config lewat registry, dan memanggil `execute_reload`.

## Milestone 3: PostgreSQL 17, major mismatch refusal, evidence, and catalog

_Langkah 9, 10, dan 11 Build plan. Semua langkah native butuh kedua artefak (17.11 dan 18.6) di cache runtime xtask dan tidak mengunduh apa pun; tanpa artefak, test berakhir `BLOCKED` dengan alasan `artifact <id> is not in the runtime cache` dan owner `@ojiepermana`. Test dasar Milestone 2 kini ada dalam dua fungsi per test, `postgresql_17_*` dan `postgresql_18_*`, dari satu skenario yang sama; filter nama di Milestone 2 (`lifecycle`, `security`, dan seterusnya) kini mencakup keduanya._

### UI / manual

- [x] Buka `providers/catalog.toml` → `official/postgresql` ada sebagai `[[provider]]` dengan `mark = "Pg"` dan `tile = "blue"`, tidak lagi di `[[planned]]`, `revision = 2` dan `published_at` baru; buka `providers/catalog-revisions.json` → ada entri `revision` 2 dengan `contentSha256` yang sama dengan keluaran `cargo xtask catalog check` → AC-1 (bagian catalog)
- [x] Baca `target/evidence/<runId>/report.json` dari perintah evidence di bawah → 29 record `native`, semuanya `PASS`, 0 problem; 14 record bersubject `postgresql` `17.11` dan 14 `18.6` (versi dari `SHOW server_version`) dengan digest artefak masing masing, dan satu record `install` dengan kedua subject → AC-5 sampai AC-11, AC-13, AC-14 (bagian guard)
- [x] Buka `evidence/compatibility/<runId>/report.json` (run yang dikeep `compat-record`) → 2 record `native` `PASS` dengan `covers` `BR-025`: satu bersubject `postgresql` `17.11` (digest `04b1eff1…`) dan satu `18.6` (digest `88bb7533…`), `sourceDirty` `false`, `sourceCommit` sama dengan commit yang diuji; tiap folder `artifacts/<id>/compatibility.json` berisi `checks` `adoption`, `init`, `major_mismatch_refusal`, `readiness`, `reload`, `start`, `stop`, `knownLimitations` kosong, dan `providerRevision.manifestSha256` sama dengan hash di keluaran `cargo xtask provider check providers/postgresql` → AC-14
- [x] Buka `providers/postgresql/versions.json` → kedua artefak punya satu record `provider` `pass` untuk `official/postgresql` `1.0.0` dengan `manifestSha256` itu, dan `evidence` yang menunjuk folder `compatibility/<runId>/<id>` → AC-14
- [x] Cek proses setelah run: `ps -axo command | grep stackly-test` dan `pgrep -x postgres` dibanding sebelum run → tidak ada proses `postgres` milik test yang tersisa → AC-13

### Commands

- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test postgresql --locked -- --ignored` → `29 passed`, `evidence: 29 native records (… PASS 29, FAIL 0, BLOCKED 0 …), 0 problems` → AC-5, AC-6, AC-7, AC-8, AC-9, AC-10, AC-11, AC-13
- [x] `cargo test -p stacklyd --test postgresql -- --ignored postgresql_17` → 15 test lulus: skenario Milestone 2 pada PostgreSQL 17.11 (`lifecycle`, `security` tiga, `endpoints`, `readiness`, `stop`, `failure`, `tools`, `adoption`, `reload`, `format`), penolakan mayor, evidence kompatibilitas, dan exec check install kedua artefak; `data.initializedLine` `17`, `SHOW server_version` `17.11` → AC-5, AC-6, AC-7, AC-8, AC-9, AC-10, AC-13
- [x] `cargo test -p stacklyd --test postgresql -- --ignored postgresql_18` → 14 test lulus (skenario yang sama pada 18.6, penolakan mayor, dan evidence kompatibilitas) → AC-5, AC-6, AC-7, AC-8, AC-9, AC-10, AC-11, AC-13
- [x] `cargo test -p stacklyd --test postgresql -- --ignored mismatch` → dua test lulus: data 17 pada runtime 18 dan data 18 pada runtime 17 gagal start dengan `supervisor.data_incompatible` (`outcome` `incompatible`, `data` dan `runtime` sesuai arahnya), tanpa proses `postgres` untuk folder data itu dan tanpa `postmaster.pid`; `PG_VERSION` yang dihapus menghasilkan `marker_missing`, yang diubah menjadi `16` menghasilkan `marker_mismatch`, dan sesudah `PG_VERSION` dikembalikan instance start sampai `ready` → AC-11
- [x] `cargo test -p stacklyd --test postgresql -- --ignored runs_the_package` → dua test lulus: PostgreSQL 17.11 dan 18.6 dengan paket final dibuat, dijalankan sampai `ready`, di reload tanpa restart (`log_statement` `all`, PID dan waktu start sama), diadopsi daemon baru sesudah `SIGKILL` (PID dan waktu start sama), di stop (tanpa proses), dan datanya ditolak runtime mayor lain; `compatibility.json` ditulis hanya sesudah semua check lulus → AC-14
- [x] `cargo xtask runtime check` dan `cargo xtask codegen --check` → `... compatibility run(s) follow the rules` dan `codegen: 20 generated files are up to date` (`versions.json` PostgreSQL memuat record `provider` dari run itu) → AC-14
- [ ] `cargo xtask runtime compat-record target/evidence/<runId>/report.json` dari run native bersih di commit yang sama (hanya bila `evidence/compatibility/` belum memuat run itu) → `kept the evidence in evidence/compatibility/<runId>`; run kotor, run fixture, atau run commit lain ditolak → AC-14
- [x] `cargo test -p stacklyd --test postgresql -- --ignored postgresql_17_has_no_trust` → lulus pada 17.11 dengan pesan PostgreSQL yang dikutip spec: `no pg_hba.conf entry for replication connection` untuk `replication=true`, `permission denied to start WAL sender` untuk `stackly_readiness` dengan `replication=database` lewat socket, dan `pg_hba.conf rejects connection` lewat TCP → AC-6 (teks AC-6 setelah update spec 2026-10-22; pesan di 18.6 dan 17.11 sama)
- [x] `cargo test -p stackly-core --test provider_packages data_marker` → dua test lulus, termasuk `the_postgresql_data_marker_names_the_major_and_refuses_the_other_one`: isi `PG_VERSION` asli (`17\n`, `18\n`) menjadi line, data 17 pada runtime 17 `Compatible`, 17 pada 18 dan 18 pada 17 `Incompatible`, marker hilang `MarkerMissing`, `16\n` `MarkerMismatch`, dan `17.11`, `v17`, `17 18`, teks kosong `MarkerMismatch` tanpa line → AC-11
- [x] `cargo xtask catalog check` → `ok catalog revision 2 <sha256>`; ubah `tile` entri PostgreSQL di `providers/catalog.toml` lalu jalankan lagi tanpa menaikkan `revision` → `catalog content changed: bump revision and published_at ...` (kembalikan sesudahnya) → AC-1 (bagian catalog, *Aturan masuk catalog* spec 0017)
- [x] Jalur `BLOCKED`: `HOME=/nonexistent STACKLY_EVIDENCE_DIR=<folder yang ada> STACKLY_EVIDENCE_KIND=native target/debug/deps/postgresql-<hash> --ignored postgresql_17_is_created` → test lulus, record `BLOCKED` dengan alasan `artifact postgresql-17.11-r2-macos-aarch64 is not in the runtime cache` dan owner `@ojiepermana`, tanpa mengunduh apa pun → AC-14
- [x] `cargo fmt --all --check`, `cargo clippy -p stacklyd -p stackly-core --all-targets --locked -- -D warnings`, `cargo xtask check-deps`, `cargo test --workspace --locked --no-fail-fast` (setelah `cargo build -p stacklyd -p stackly-fake-server`) → lulus → AC-12

### Acceptance-criteria coverage

- AC-1 · bagian catalog: `providers/catalog.toml` dan ledger, `cargo xtask catalog check`; bagian paket sudah di Milestone 1
- AC-5 sampai AC-10, AC-13 · skenario Milestone 2 pada PostgreSQL 17.11 dan 18.6, satu fungsi test per mayor (`postgresql_17_*`, `postgresql_18_*`)
- AC-6 · tambahan teks setelah update spec 2026-10-22: `replication=true` ditolak dan `stackly_readiness` ditolak untuk `replication=database` lewat socket dan TCP, pesan dicek di 17.11 dan 18.6
- AC-11 · `mismatch` pada kedua arah, kedua kasus marker, dan unit test preflight dengan isi `PG_VERSION` asli
- AC-14 · guard `native` pada setiap test, jalur `BLOCKED` untuk 17, test `runs_the_package` per mayor yang menulis `compatibility.json` (7 check) untuk paket final, `cargo xtask runtime compat-record`, dan record `provider` di `versions.json` lewat `cargo xtask codegen`. Test install (AC-15) tidak lagi memuat `BR-025`, karena ia tidak menulis `compatibility.json`
- AC-1 bagian paket, AC-2, AC-3, AC-4, AC-12 dan AC-15 · sudah dicakup Milestone 1 dan 2 (suite 17 mengulang AC-4 dan AC-12 pada artefak 17.11 lewat `security` dan `tools`)
