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
- [x] `cargo test -p stacklyd --test postgresql -- --ignored security` → tiga test lulus: tanpa `trust` di kedua `pg_hba.conf`, superuser tanpa password ditolak di socket dan TCP, `stackly_readiness` ditolak lewat TCP dan ke `template1`, replication fisik ditolak, password tidak ada di argv, file, atau log; password `<hex> 'x"y\z` bisa login dan password ber baris baru ditolak `secret.invalid_value` tanpa panggilan tulis ke Keychain; locale `C.UTF-8` builtin, `UTF8`, checksum `on`, zona `UTC` di bawah `LANG=de_DE.UTF-8` dan `TZ=Asia/Jakarta` → AC-4, AC-6, AC-7
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
- AC-6 · `security`. Replication: lihat catatan penyimpangan di bawah
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

- AC-6 dan *Critical test scenarios* menyebut koneksi `replication=database` ditolak. Itu tidak benar untuk PostgreSQL: koneksi `replication=database` dicocokkan dengan aturan database nya (`postgres`), jadi superuser dengan password diterima seperti login biasa. Yang tidak punya aturan adalah replication fisik (`replication=true`). Test memeriksa replication fisik ditolak (`no pg_hba.conf entry for replication connection`) dan `replication=database` oleh `stackly_readiness` ditolak. Teks AC-6 perlu dikoreksi lewat `/architect`.
- `covers` memakai `REQ-035`, `AC-035`, `BR-009`, `VAL-018`, dan `BR-025`; `JOURNEY-01` ditolak guard evidence (bukan ID PRD tiga digit).
- Reload tidak punya method IPC sebelum fitur 29, jadi test menghentikan daemon dalam proses (postmaster tetap hidup), membangun `Supervisor` milik test di registry yang sama, mengadopsi postmaster lewat `reconcile`, menulis revisi config lewat registry, dan memanggil `execute_reload`.
