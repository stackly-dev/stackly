# Verify: Provider MySQL · spec 0042 · updated 2026-11-12

_Langkah diturunkan dari acceptance criteria spec 0042. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Bagian Milestone 1 memuat langkah 1 sampai 4 Build plan, bagian Milestone 2 langkah 5 sampai 8. Test native butuh artefak di cache runtime xtask (`~/Library/Caches/dev.stackly.xtask/runtime/`); tanpa artefak test berakhir `BLOCKED`, tanpa unduhan._

## Milestone 1: Format, paket, dan spike dari paket

### UI / manual

- [x] Buka `docs/providers/manifest-reference.md` → baris `format` di tabel `configFiles` menyebut `mysqlCnf` (kutip seperti `valkeyConf`, boolean `ON` dan `OFF`) → AC-2

### Commands

- [x] `cargo test -p stackly-core --lib provider::template::tests::mysql_cnf` → 3 test lulus: `a"b\c d # e ü 空 'q' ; z` menjadi `"a\"b\\c d # e ü 空 'q' ; z"`, `$` dan backtick apa adanya, setiap karakter kontrol (U+0000 sampai U+001F, U+007F sampai U+009F) ditolak dan U+00A0 diterima, boolean `ON` dan `OFF`, tag diterima, output tidak diparse ulang, dan `from_code("mysqlCnf")` mengembalikan varian baru (`mysql` dan `MysqlCnf` tidak) → AC-2
- [x] `cargo test -p stackly-core --lib secret::delivery::tests::a_placeholder_renders_with_the_escaper_of_its_format` → `render_secret_placeholder` memakai escaper `mysqlCnf` dan menolak secret berisi DEL → AC-2
- [x] `cargo test -p stackly-core --test provider_rules mysql_cnf_renders_quoted_text_decimal_integers_and_on_off_booleans` → fixture fake dengan `format: mysqlCnf` merender `general_log_file` dengan teks AC-2, integer desimal, boolean `ON`/`OFF`, dan secret lewat `materialize`; secret dan path data dengan karakter kontrol gagal `UnrepresentableValue` dengan nama field, tanpa nilainya → AC-2
- [x] `cargo test -p stackly-core --test provider_rules an_unknown_format_names_every_known_format_in_its_message` → pesan `unknown_template_format` menyebut `mysqlCnf` → AC-2
- [x] `cargo test -p stackly-core --test provider_compat` → corpus beku lulus tanpa satu pun `frozen.json` berubah → AC-2 (syarat additive 3)
- [x] `cargo test -p stackly-core --test fuzz_regressions` dan `cargo test -p xtask fuzz` → seed `fuzz/seeds/provider_template/format.mysql.cnf.tmpl` (selector 7, setup `mysqlCnf`) lulus harness tanpa panic, dan `cargo xtask fuzz seed` tidak mengubah `fuzz/seeds/` → AC-2 (syarat additive 6)
- [x] `cargo xtask provider check providers/mysql` → satu baris `ok official/mysql 1.0.0 <manifest_sha256>` → AC-1
- [x] `cargo xtask codegen --check` → lolos; `providers/mysql/versions.json` memuat tepat dua artefak `available`, `mysql-8.4.11-macos-aarch64` (line `8.4`) dan `mysql-9.7.2-macos-aarch64` (line `9.7`) → AC-1
- [x] `cargo test -p stackly-core --test provider_packages mysql` → 9 test lulus: paket terbaca dan valid tanpa `init_port_issues` dengan kedua artefak (AC-1); `init.json`, `run.json`, `render-myCnf.txt`, dan `render-myCnf-binlog.txt` sama dengan golden di `crates/core/tests/golden/provider/mysql/` (AC-1); setiap program init, run, readiness, dan health adalah `bin/mysqld` atau `bin/mysql` di root runtime dengan argumen pertama `--no-defaults` atau `--defaults-file=<config>/my.cnf`, tanpa secret di argv (AC-12); SQL akun adalah `Stdin::Text` 14 statement dan file `root-password` dibersihkan (AC-3); version check `--no-defaults --version` membaca `8.4.11` dan `9.7.2` dari baris yang diawali path binary (AC-15); penanda `mysql_upgrade_history` memberi line entri terakhir dan menolak 8.4 di 9.7, 9.7 di 8.4, penanda hilang, `8.0.40`, dan teks rusak (AC-10, bagian paket); remedy `readiness_account` cocok untuk `ERROR 1045` dan `ERROR 3118` dan sama dengan `remedies.json` (AC-18, bagian paket) → AC-1, AC-12, AC-15
- [x] `cargo test -p stackly-core --test provider_packages` → semua test lulus, termasuk `every_repo_package_reads_its_version_from_the_literal_output_of_its_binary`, `every_package_initializes_with_endpoints_that_have_no_port`, dan `every_folder_in_providers_has_no_init_port_issue` yang kini memuat `mysql` → AC-1, AC-15
- [x] `cargo test -p stacklyd --test create_form_fixtures` → `protocols/fixtures/instance/create-form-mysql.json` sama dengan form yang dibangun dari paket, dan `index.json` memuat entri `create-form-mysql.json` (`CreateForm`) → AC-14
- [x] `cargo xtask fuzz seed` lalu `git status --short fuzz/` → tidak ada perubahan → AC-14
- [x] `cargo test -p xtask` → semua lulus tanpa mengubah `PROVIDER_WORDS`; `runtime::spike::package_recipe::tests::the_packaged_providers_are_found_by_their_software` menemukan `mysql` sebagai paket → AC-14
- [x] `grep -n "MySql\|provisional: F42" xtask/src/runtime/spike/providers.rs` → tidak ada hasil → AC-14
- [x] `cargo test -p xtask --test native_runtime -- --ignored mysql --test-threads 1` → `mysql_8_4_runs_from_managed_artifact` dan `mysql_9_7_runs_from_managed_artifact` lulus dari paket lewat `PackageRecipe` (`PASS, version 8.4.11` dan `9.7.2`); butuh sesi GUI login (worker LaunchAgent) dan artefak di cache → AC-14
- [x] `cargo test -p stacklyd --test native_runtime_install -- --ignored mysql` → `mysql_8_4_installs_from_the_cache` dan `mysql_9_7_installs_from_the_cache` lulus: daemon memasang artefak dari cache tanpa jaringan, exec check `--no-defaults --version` mencetak versi artefak → AC-14, AC-15

### Acceptance-criteria coverage

- AC-1 · paket, `provider check`, `versions.json`, dan golden plan; bagian catalog (`[[provider]]`, ledger, `provider freeze`) menunggu Milestone 3 langkah 11
- AC-2 · format `mysqlCnf` di core; round trip `my_print_defaults` runtime 8.4.11 ada di Milestone 2
- AC-12 · bagian plan (program dan argumen pertama) di sini; `PATH` umpan dan `.my.cnf` rusak di Milestone 2
- AC-14 · fixture form, spike dari paket, dan install dari cache di sini; dua test konformitas menunggu Milestone 3 langkah 10
- AC-15 · manifest dan exec check install kedua artefak
- AC-3 sampai AC-11, AC-13, AC-16 sampai AC-18 · Milestone 2 dan sesudahnya

## Milestone 2: MySQL 8.4 asli lewat daemon

### UI / manual

- [ ] Di aplikasi: buat instance MySQL 8.4 dari Catalog, start, salin connection string, lalu Open In TablePlus, Sequel Ace, dan mysql (Terminal); mysql menanyakan password root dan menjawab `SELECT 1` dengan password yang ditempel. Ini juga memeriksa bundle ID Sequel Ace `com.sequel-ace.sequel-ace` dan bahwa kedua aplikasi menerima URL `mysql://` → AC-18
- [ ] Di aplikasi: ubah Max connections di halaman Configuration → dampaknya tampil sebagai restart, dan sesudah apply instance ready lagi → AC-13

### Commands

Semua test di bawah native (`#[ignore = "native: ..."]`), satu instance per test, dari artefak `mysql-8.4.11-macos-aarch64` di cache runtime. Jalankan satu per satu karena setiap `mysqld` memakai ratusan MiB.

- [x] `cargo test -p stacklyd --test mysql` → `blocked::an_artifact_that_is_not_in_the_runtime_cache_is_named_without_a_download` lulus: alasan `BLOCKED` adalah `artifact <id> is not in the runtime cache` dengan owner `@ojiepermana` → AC-16 (jalur `BLOCKED`)
- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 format::` → `my_print_defaults --defaults-file=<config>/my.cnf mysqld` runtime 8.4.11 mencetak `--general_log_file=` diikuti teks `a"b\c d # e ü 空 'q' ; z` persis → AC-2
- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 init::` → 3 test lulus: tepat satu proses `--initialize-insecure` tanpa listener TCP dan tanpa socket, sesudahnya tidak ada `root-password`, `mysqld`, atau `binlog.*`; salinan paket dengan `CREATE USER 'root'@'localhost'` membuat `mysqld` keluar 1 dengan `init_step_failed` di langkah `2` dan pesan `Operation CREATE USER failed` di log sink; file yang disisipkan kait `before_init` membuat `mysqld` keluar 1 dengan `init_step_failed` dan file itu tetap sama byte dan waktu ubahnya → AC-3
- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 auth::` → 2 test lulus: empat akun tidak terkunci, akun `root` ber host IP sama dengan `root@localhost` kecuali `AUDIT_ABORT_EXEMPT` dan `FIREWALL_EXEMPT`, `root` butuh password di socket dan TCP, `stackly_readiness` ditolak TCP, hanya `GRANT USAGE`, gagal `CREATE DATABASE`, `MAX_USER_CONNECTIONS 3`; password `pa'ss"w\ord ü空 x ...` login di socket dan TCP; password tidak ada di argv `ps`, file di bawah root test dan root runtime (termasuk log daemon binary), atau log sink → AC-4
- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 endpoints::` → `lsof` hanya satu `LISTEN` di `127.0.0.1:<port>` dan socket `<run>/mysql.sock`, plugin `mysqlx` tidak aktif, `@@admin_address` `NULL`; sesudah `SET PERSIST max_connections = 77` dan `SET PERSIST_ONLY skip_name_resolve = OFF` lalu stop dan start: `@@max_connections` 151, `@@skip_name_resolve` 1, ready, `stackly_readiness` ditolak TCP; port `Auto` `bound` dan sama dengan `@@port` → AC-5
- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 paths::` → variabel path ada di folder data, temp, logs, dan run instance, `secure_file_priv` `NULL`, `log_error` `stderr`; setiap file `REG` yang dibuka `mysqld` dengan mode `w` atau `u` ada di folder itu (kecuali `instance.lock` milik Stackly di fd 3); baris `[System] ... ready for connections` terbaca lewat `instance.logs_read` → AC-6
- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 readiness::` → sesudah `ACCOUNT LOCK` instance `not_ready` dengan detail `Access denied for user 'stackly_readiness'@'localhost'` sementara port TCP menerima koneksi; sesudah `ACCOUNT UNLOCK` ready lagi → AC-7
- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 stop::` → dengan satu sesi `SELECT SLEEP(600)` stop selesai di bawah 30 detik, `mysqld.pid`, `mysql.sock`, dan `mysql.sock.lock` hilang, log memuat `Shutdown complete`, dan start berikutnya tidak memuat `Database was not shutdown normally` → AC-8
- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 tools::` → daemon binary dengan `mysql`, `mysqld`, dan `mysqladmin` umpan di depan `PATH` dan `.my.cnf` berisi opsi tidak dikenal di folder temp instance: create, start, readiness, dan stop berhasil, file penanda umpan tidak pernah dibuat → AC-12
- [ ] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 lifecycle::` → 2 test lulus: create dengan `rootPassword` bangkitan, `data.initializedLine` `8.4`, start `not_ready` lalu `ready`, satu connection `uri` tersedia yang `masked` nya URI absolut `mysql://root:...@127.0.0.1:<port>/`, baris tetap ada sesudah stop dan start, restart memberi PID baru, `maxConnections` 77 lewat edit config spec 0029 dan restart memberi `@@max_connections` 77; `binaryLog: true` memberi `@@log_bin` 1 dan file `binlog.*` (tidak ada sesudah init), default `@@log_bin` 0 → AC-13
- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 adoption::` → `mysqld` memegang `instance.lock` di fd 3, tetap jalan sesudah `stacklyd` di `SIGKILL`, dan daemon berikutnya mengadopsi PID dan waktu start yang sama sebagai ready → AC-13
- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 open_with::` → `instance.get_detail` memuat `openActions` `tableplus`, `sequel_ace`, `mysql` berurutan, masing masing `copySecret` `rootPassword` tanpa nilai, dan `mysql` menjalankan `bin/mysql` runtime instance lewat socket instance → AC-18
- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 doctor::` → sesudah `DROP USER stackly_readiness` instance `not_ready`, `doctor.run` memuat remedy `readiness_account` dari `official/mysql` dengan perintah yang sudah dirender (mysql runtime dan socket instance, tanpa password), dan akun tetap tidak ada sesudahnya → AC-18

### Acceptance-criteria coverage

- AC-2 · round trip `my_print_defaults` runtime 8.4.11 (`format::`)
- AC-3 · `init::` untuk 8.4; 9.7 menunggu Milestone 3 langkah 9
- AC-4, AC-5, AC-6, AC-7, AC-8 · `auth::`, `endpoints::`, `paths::`, `readiness::`, `stop::` untuk 8.4; 9.7 menunggu langkah 9. AC-7 diperiksa sebagai `not_ready` dengan detail MySQL; jumlah dua kegagalan berturut turut adalah perilaku supervisor yang sama untuk setiap provider dan tidak dihitung terpisah di sini
- AC-12 · `tools::` (`PATH` umpan dan `.my.cnf` rusak) plus golden plan Milestone 1
- AC-13 · `lifecycle::` dan `adoption::` untuk 8.4; 9.7 menunggu langkah 9
- AC-16 · jalur `BLOCKED` (`blocked::`); guard evidence `native` sudah dipasang di setiap test native dengan `covers` `REQ-055`, `BR-008`, `VAL-018`; evidence BR-025 dan `compatibility.json` per artefak menunggu Milestone 3 langkah 12
- AC-18 · `open_with::` dan `doctor::`
- AC-9, AC-10 (daemon), AC-11 · Milestone 3 langkah 9
- AC-1 (catalog), AC-14 (konformitas) · Milestone 3 langkah 10 dan 11
- AC-17 · Milestone 4

## Milestone 3: MySQL 9.7, isolasi, penolakan line, konformitas, evidence, dan catalog

_Langkah 9 dan 10, lalu langkah 11 (catalog dan `provider freeze`) dan 12 (evidence `compatibility.json`) di giliran catalog sesudah perbaikan lifecycle `de5d0e14` di `main`. Komentar baris kedua `my.cnf.tmpl` dan README dibetulkan di revision `1.0.0` yang sama (commit `a046da9f`: suntingan tangan adalah drift, bukan hilang), jadi `manifest_sha256` menjadi `77a41842…`; run evidence `01a1cef9-f323-7534-81ff-caad7326c723` dari `HEAD` bersih `a046da9f`, 37 record native `PASS`, menggantikan run `01a1cecf…` di atas paket lama._

### UI / manual

- Tidak ada untuk langkah 9 dan 10.

### Commands

Semua test native, satu per satu (`--test-threads 1`), dari `mysql-8.4.11-macos-aarch64` dan `mysql-9.7.2-macos-aarch64` di cache runtime; tanpa artefak test berakhir `BLOCKED`.

- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 mysql_9_7 --skip sentinel --skip mismatch` → 13 test lulus (di run evidence `01a1cef9…` di atas paket final): suite Milestone 2 (init, auth, endpoints, paths, readiness, stop, tools, lifecycle, binary log, adoption) untuk runtime 9.7.2, dengan hasil yang sama seperti 8.4 → AC-3 sampai AC-8, AC-12, AC-13 untuk 9.7
- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 sentinel::` → 3 test lulus: dua instance 8.4, dua instance 9.7, dan satu 8.4 di samping satu 9.7, masing masing dengan satu baris di `stackly_sentinel.marker`; sesudah semuanya stop lalu start, query lewat TCP dan socket setiap instance hanya mengembalikan barisnya sendiri → AC-9
- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 mismatch::` → 2 test lulus: data 8.4 yang diikat ke runtime 9.7 (dan 9.7 ke 8.4) gagal start dengan `supervisor.data_incompatible` (`outcome` `incompatible`, `data` dan `runtime` sesuai) sebelum spawn, tanpa `mysqld`, byte `mysql_upgrade_history` dan `mysql.ibd` tidak berubah, dan `rebind::check` memberi outcome yang sama; di runtime sendiri penanda yang dihapus memberi `supervisor.data_unavailable` (`reason` `the data marker is missing`), versi `8.0.40` memberi `marker_mismatch`, dan sesudah penanda kembali instance ready → AC-10
- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 existing_data::` → salinan folder data MySQL 8.4 yang sah, disisipkan kait `after_layout`, membuat create gagal `instance.data_not_empty` tanpa langkah `init`, tanpa `mysqld`, dan setiap file salinan sama byte demi byte → AC-11
- [x] `cargo test -p stacklyd --test provider_conformance -- --ignored --test-threads 1 mysql` → `mysql_8_4_passes_conformance` dan `mysql_9_7_passes_conformance` lulus untuk skenario `default` dan `binlog`, report native ber verdict pass, tanpa proses dan root tersisa (dijalankan ulang di atas paket final `77a41842…`, lulus) → AC-14
- [x] `cargo test -p stacklyd --test provider_conformance every_official_package_has_the_step_and_health_limits_its_manifest_gives` → batas `mysql` init 180, readiness 120, stop 120, reload 30 (bawaan, tanpa capability reload), health 43000 ms → AC-14

### Acceptance-criteria coverage

- AC-3 sampai AC-8, AC-12, AC-13 · 9.7 lewat fungsi test yang sama dengan 8.4 (`mysql_9_7_*`); AC-2 round trip tetap hanya 8.4.11 sesuai teks AC, dan AC-18 (`open_with::`, `doctor::`) hanya 8.4
- AC-9 · `sentinel::`
- AC-10 · `mismatch::` kedua arah, termasuk `rebind::check`
- AC-11 · `existing_data::` dengan data 8.4
- AC-14 · konformitas native kedua lini; `scenarios.yaml` tidak berubah
- AC-1 (catalog) · `[[provider]]` di catalog revision 16, ledger, dan `provider freeze`; AC-16 · `compat::` lulus dan record `compatibility` per artefak di `providers/mysql/versions.json` (lihat di bawah)
- AC-17 · Milestone 4

### Commands (langkah 11 dan 12)

- [x] `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1 compat::` → `mysql_8_4_runs_the_package_and_records_what_it_proved` dan `mysql_9_7_runs_the_package_and_records_what_it_proved` lulus dari cache: lewat binary `stacklyd`, `SELECT VERSION()` 8.4.11 dan 9.7.2 sebagai subject, dua instance satu lini dengan baris `stackly_compat.marker` sendiri, restart dengan `mysqld` baru di data yang sama, adopsi PID dan `startedAt` keduanya sesudah `SIGKILL` daemon, stop tanpa proses, lalu data diikat ke runtime lini lain ditolak `supervisor.data_incompatible` tanpa spawn; `compatibility.json` per artefak memuat `adoption`, `init`, `isolation`, `line_mismatch_refusal`, `readiness`, `restart`, `start`, `stop` → AC-16
- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test mysql --locked -- --ignored --test-threads 1` dari `HEAD` bersih `a046da9f` → 37 record native `PASS`, 0 problem, `sourceDirty` `false`; run `01a1cef9-f323-7534-81ff-caad7326c723`; lalu `cargo xtask runtime compat-record target/evidence/01a1cef9-f323-7534-81ff-caad7326c723/report.json` → `evidence/compatibility/01a1cef9-f323-7534-81ff-caad7326c723/`, dan `cargo xtask codegen` → `providers/mysql/versions.json` mendapat satu record scope `provider` `pass` per artefak (8.4.11 dan 9.7.2) dengan delapan check, tanpa `knownLimitations`, `manifestSha256` `77a41842…` → AC-16
- [x] Entri MySQL pindah ke `[[provider]]` (mark `My`, tile `teal`, tanpa catatan platform karena spec tidak menyebutnya) dengan catalog revision 16; `cargo xtask catalog check --update --amend` → `ok catalog revision 16 2b818694…`; `cargo xtask provider freeze providers/mysql` (sesudah folder lama dihapus) menulis `crates/core/tests/fixtures/compat/mysql/1.0.0/`; `cargo test -p xtask`, `cargo test -p stackly-core`, `cargo test -p stackly-cli`, dan `cargo test -p stacklyd` lulus tanpa perubahan fixture → AC-1

## Milestone 4: Handover

### UI / manual

- [ ] `NOT_RUN` · `BLOCKED` di sesi build (`reason` test launchd butuh sesi login GUI user, `owner` `@ojiepermana`) · AC-17, di sesi GUI yang login. Kedua test gagal di assert `Keep/HandoverProven` sampai baris handover ada, dan `cargo xtask evidence run` hanya memberi record yang sah dari `HEAD` bersih, jadi commit dulu dua baris di `crates/core/src/supervisor/handover.rs`: `HandoverProof::new("official/mysql", "1", "spec-0042-ac-17"),` di `HANDOVER_PROVEN`, dan `("official/mysql", "1.0.0", true)` di tabel test modul itu yang memuat `("official/clickhouse", "1.0.0", false)`; `git commit -m "feat(core): spec 0042 step 13, official/mysql in HANDOVER_PROVEN"`. Lalu dari `HEAD` itu `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test native_launchd --locked -- --ignored --test-threads 1 mysql_`. Harapan: `mysql_8_4_survives_planned_restart_and_sigkill_and_returns_after_kickstart_and_bootout` dan `mysql_9_7_survives_planned_restart_and_sigkill_and_returns_after_kickstart_and_bootout` lulus (PID dan `startedAt` sama sesudah planned restart dan `SIGKILL` daemon, proses baru sesudah `kickstart -k` dan `bootout`). PASS keduanya: commit `handover.rs` tetap, dan ID run evidence dicatat di sini. Salah satu FAIL atau BLOCKED: `git revert` commit `handover.rs`; AC-17 bagian entri tetap terbuka.

### Commands

- [ ] Kedua test launchd MySQL di `apps/daemon/tests/native_launchd.rs` → ditulis, tidak dijalankan sesi build (lihat UI / manual); tanpa entri `HANDOVER_PROVEN` restart daemon terencana memakai `stop_restore` dengan `handover_unproven` dan harness konformitas melaporkan `handover: unproven` → AC-17

### Acceptance-criteria coverage

- AC-17 · bagian test ditulis; entri `HANDOVER_PROVEN` hanya bersama record `PASS` kedua test dari langkah manual
