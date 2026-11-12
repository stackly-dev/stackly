# Verify: Provider ClickHouse · spec 0043 · updated 2026-11-12

_Langkah diturunkan dari acceptance criteria spec 0043. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Bagian Milestone 1 di bawah baru memuat langkah 1 Build plan (format `xml` di core). Langkah 2 (filter `sha256Hex`) dibangun spec 0040 langkah 2 (commit `0d111375`); test AC-2 nya tercatat di `docs/specs/0040-provider-valkey/verify.md` Milestone 1 dan dipakai di sini oleh test `xml` di bawah. Langkah paket dan golden (Build plan langkah 3 sampai 5) ada di bagian yang sama; Milestone 2 (langkah 6 sampai 9) dan langkah 10 Milestone 3 ada di bawahnya. Test native butuh artefak `clickhouse-26.8.30.8-macos-aarch64` (sha256 `097b41ad…7402`) di cache runtime xtask; tanpa itu test berakhir `BLOCKED` dengan owner `@ojiepermana`, tidak pernah mengunduh._

## Milestone 1: core dan paket (tanpa binary)

### UI / manual

- [ ] Buka `docs/providers/manifest-reference.md` → baris `format` di tabel `configFiles` menyebut `xml` (tanpa kutip, `&`, `<`, `>`, `"`, dan `'` menjadi entity, contoh `<path>{{ instance.data }}/</path>`) → AC-1
- [ ] Buka `providers/clickhouse/` → isinya persis *Paket ClickHouse* spec 0043: `provider.yaml`, `schemas/settings.schema.json` (`defaultPassword`, `logLevel`), `templates/config.xml.tmpl`, `templates/client.xml.tmpl`, `templates/users.xml.tmpl`, `tests/scenarios.yaml` (satu skenario `default`), `README.md`, `LICENSE-NOTICES.md`, dan `versions.json` dari codegen → AC-3

### Commands

- [ ] `cargo test -p stackly-core --lib provider::template::tests::xml` → 3 test lulus: `a&b<c>d"e'f ü 空 $x {y}` menjadi `a&amp;b&lt;c&gt;d&quot;e&apos;f ü 空 $x {y}` tanpa kutip, `&amp;` di escape lagi, setiap karakter kontrol (U+0000 sampai U+001F, U+007F sampai U+009F), U+FFFE, dan U+FFFF ditolak sedangkan U+00A0, U+FFFD, dan emoji diterima, boolean `true` dan `false`, tag diterima, output tidak diparse ulang, `from_code("xml")` mengembalikan varian baru (`XML` tidak) → AC-1
- [ ] `cargo test -p stackly-core --lib secret::delivery::tests::a_placeholder_renders_with_the_escaper_of_its_format` → `render_secret_placeholder` memakai escaper `xml` dan menolak secret berisi U+FFFF → AC-1
- [ ] `cargo test -p stackly-core --test provider_rules xml_writes_values_as_element_content_touching_literal_text` → fixture fake dengan `format: xml` merender tag yang menempel pada teks (`<path>…/</path>`), entity untuk teks, integer desimal, boolean `true`/`false`, secret lewat `materialize`, dan `{{ secret.authToken | sha256Hex }}` sebagai hash mentah; secret berisi U+FFFE dan path data berisi ESC gagal `UnrepresentableValue` dengan nama field, tanpa nilainya → AC-1, AC-2
- [ ] `cargo test -p stackly-core --test provider_rules an_unknown_format_names_every_known_format_in_its_message` → pesan `unknown_template_format` menyebut `xml` bersama ketujuh format lain → AC-1
- [ ] `cargo test -p stackly-core --test provider_compat` → corpus beku lulus tanpa satu pun `frozen.json` berubah (`git status crates/core/tests/fixtures/compat` bersih) → AC-1
- [ ] `cargo xtask codegen --check` → lulus; `schemas/provider/` tidak berubah, karena `format` di sana string tanpa daftar nilai → AC-1
- [ ] `cargo xtask provider check providers/clickhouse` → satu baris `ok official/clickhouse 1.0.0 <sha256>`, tanpa `init_port_issues` dan tanpa finding `warning`; `cargo xtask provider check` (semua folder) → satu baris `ok` per paket, `official/clickhouse` di depan → AC-3
- [ ] `cargo xtask codegen --check` → `providers/clickhouse/versions.json` sama dengan tulisan codegen dari `runtimes/inventory.toml` (satu artefak `macos` `aarch64`) → AC-3, AC-13
- [ ] `cargo test -p stackly-core --test provider_packages clickhouse` → lulus: paket terbaca tanpa tulis dan valid sebagai `official/clickhouse` `1.0.0` tanpa `dataMarker`; golden `init.json`, `run.json`, `reload.json`, `render-serverConfig.txt`, `render-clientConfig.txt`, `render-usersConfig.txt` di `crates/core/tests/golden/provider/clickhouse/` cocok; `init_plan` tanpa port hanya `EnsureDirectories` dan `run_plan` menulis `client.xml` dan `config.xml` di folder config serta `users.xml` di `run/secrets/`; key terlarang tidak ada di template maupun render dan `listen_host` hanya satu; setiap path render di bawah `data` atau `temp` instance; folder data berisi `&<>"'` menjadi entity, berisi ESC gagal `UnrepresentableValue` tanpa nilainya; `users.xml` hasil `materialize` memuat SHA-256 hex password fixture dan bukan password nya, tiga user dibatasi `127.0.0.1` dan `::1`, tiga grant persis; argv server, `CLICKHOUSE_WATCHDOG_ENABLE=0`, argv probe readiness dan reload → AC-1, AC-2, AC-3, AC-4, AC-5, AC-6, AC-7, AC-10
- [ ] `cargo test -p stackly-core --test provider_packages a_windows_host_never_installs_clickhouse_even_with_a_linux_artifact` → manifest dan `versions.json` hanya `macos`; host `windows-aarch64` dengan catalog uji yang punya artefak `macos` dan `linux` mendapat `no_build_for_host` dan `native_support` false → AC-13
- [ ] `cargo test -p stackly-core --test connection_render --test open_with_render` → golden `connection.json` (`http` lalu `native`, masked `****`, secret hanya sebagai reference) dan `open_with.json` (`play` browser, `client` terminal dengan `--ask-password` dan `copySecret`) cocok → AC-11
- [ ] `cargo test -p stackly-core --test provider_check` → `clickhouse` ada di daftar paket repo yang publishable tanpa finding, dengan skenario `default` → AC-3
- [ ] `cargo test -p xtask --test provider_check` → daftar ID paket repo memuat `official/clickhouse` → AC-3
- [ ] `cargo test -p stacklyd --test create_form_fixtures` → `protocols/fixtures/instance/create-form-clickhouse.json` sama dengan form yang daemon bangun; `index.json` memuat entrinya → AC-16
- [ ] `swift test --package-path clients/swift` dan `swift test --package-path apps/macos/Packages/StacklyUI --filter EveryCreateFormFixtureTests` → fixture form ClickHouse terbaca dan membangun form dengan field yang dikenal → AC-16
- [ ] `cargo test -p stackly-core --test fuzz_regressions` dan `cargo test -p xtask fuzz` → seed `fuzz/seeds/provider_template/format.clickhouse.xml.tmpl` (selector 8, setup `xml`, dengan filter `sha256Hex`) lulus harness tanpa panic; selector target `provider_template` kini modulus 9; `cargo xtask fuzz seed` tidak mengubah `fuzz/seeds/` → AC-1

### Acceptance-criteria coverage

- AC-1 · format `xml` di core, escape path paket di test `provider_packages`; render golden diterima binary asli di Milestone 2
- AC-2 · filter `sha256Hex` dibangun spec 0040 langkah 2; pemakaian di `users.xml` paket dan hash di hasil `materialize` di `provider_packages`
- AC-3 · `provider check`, codegen, golden plan dan render, `provider_check` core dan xtask
- AC-4 · bagian template: key terlarang dan path di render; bagian listener dan file di Milestone 2
- AC-6 · bagian render: hash dan `<networks>` di `users.xml`; bagian login di Milestone 2
- AC-11 · bagian render: golden connection dan open action; bagian `instance.get_detail` di Milestone 2
- AC-13 · test host Windows; catatan platform catalog menunggu langkah 11
- AC-16 · bagian fixture form; catalog, corpus, dan record BR-025 menunggu langkah 11 dan 12
- AC-5, AC-7 sampai AC-10, AC-12, AC-14 · Milestone 2; AC-15 · Milestone 3 langkah 10; AC-17 · langkah 13

## Milestone 2: ClickHouse 26.8.30.8 asli lewat daemon

### UI / manual

- [ ] `NOT_RUN` · owner `@ojiepermana` · Run spike native dari paket: `clickhouse_runs_from_managed_artifact` di `xtask/tests/native_runtime.rs` lewat wrapper native di tree bersih (bisa mengunduh, butuh sesi login GUI) → AC-15
- [ ] `NOT_RUN` · owner `@ojiepermana` · Fuzz 10 menit target template sesudah format `xml` dan filter masuk: `cargo xtask fuzz run provider_template --time 600` → AC-1, AC-2
- [ ] `NOT_RUN` · owner `@ojiepermana` · Sesi GUI: buat instance ClickHouse dari form, start, salin connection, buka Play UI, buka `clickhouse client` di Terminal dan tempel password (membuktikan `--ask-password`), lalu stop → AC-11

### Commands

Semua test native: `cargo test -p stacklyd --test clickhouse -- --ignored --test-threads=1` (satu thread, karena tiap test root memasang runtime 892 MB). Tiap test memakai `TestRoot` dengan data root `Test Root Ünï cödé`, port dari `allocate_port`, dan password fixture.

- [ ] `... clickhouse_lifecycle_in_process` → create, start dengan `not_ready` sebelum `ready` (`probe_ok`), `healthy` lewat `/ping`, `SELECT version()` = `26.8.30.8`, macro = ID instance, `instance.get_detail` dengan connection `http` lalu `native` (masked, tanpa password) dan open action `play`, `client`, stop dengan proses hilang, file `status` dan `users.xml` hilang, kedua port tertutup, `lastFailure` kosong, start kedua dengan baris tabel `default.kept` masih ada → AC-7, AC-9, AC-11
- [ ] `... clickhouse_runs_as_one_foreground_process` → tanpa proses anak, fd 3 `instance.lock`, entri `txt` pertama binary runtime, PID dan argv sama sampai stop → AC-5
- [ ] `... two_clickhouse_instances_keep_their_own_events_table` → `default.events` di kedua instance, `(1, 'alpha')` dan `(2, 'beta')`, restart bergantian, tiap instance hanya membaca barisnya, empat port berbeda, audit dua listener untuk keduanya bersamaan, stop instance pertama tidak mengubah PID dan baris instance kedua → AC-8, AC-4
- [ ] `... clickhouse_only_listens_on_its_two_registry_ports` → tepat dua socket `LISTEN` di `127.0.0.1` pada port registry, tanpa UDP; setiap path `system.server_settings` di bawah folder instance kecuali empat path Keeper; sesudah start, stop, start tidak ada file baru di luar `data`, `config`, `temp`, `run`, log store, dan `config-history/` (salinan render milik Stackly, spec 0029); tidak ada `.clickhouse*` di `HOME` proses → AC-4
- [ ] `... clickhouse_default_user_needs_the_keychain_password` → password Keychain diterima di HTTP dan native, password salah dan kosong ditolak (`Authentication failed`); `users.xml` dan `data/preprocessed_configs/` hanya memuat hash; password tidak ada di file mana pun di `TestRoot`, di log instance, atau di argv `ps` → AC-6
- [ ] `... clickhouse_managed_users_can_do_nothing_else` → `stackly_readiness`: `SHOW DATABASES` hanya `system`, tabel pengguna, `system.users`, `system.processes`, `file()`, `url()`, `CREATE`, `SETTINGS`, dan `SYSTEM` ditolak; `stackly_reload`: `SHOW GRANTS` hanya `GRANT SYSTEM RELOAD CONFIG, SYSTEM RELOAD USERS ON *.*`, `SHOW DATABASES` kosong, `SELECT 1`, `numbers()`, dan `SYSTEM RELOAD CONFIG` boleh, tabel pengguna, `SYSTEM SHUTDOWN`, `SYSTEM DROP DNS CACHE`, dan `SYSTEM FLUSH LOGS` ditolak → AC-6
- [ ] `... clickhouse_reload_changes_the_log_level_and_the_password` → tanpa baris `<Debug>` di `information`; apply `logLevel` `debug` (reload) dengan PID sama dan `ready`, lalu baris `<Debug>` muncul di `instance.logs_read`; `secret.set` `defaultPassword` lalu reload: password lama ditolak dan yang baru diterima di native dan HTTP begitu operasi selesai, PID sama; `config.xml` rusak membuat `SYSTEM RELOAD CONFIG` sebagai `stackly_reload` exit bukan 0 dan server tetap menjawab → AC-10
- [ ] `... the_readiness_probe_fails_against_a_clickhouse_with_another_id` → `ProbeSpec` readiness dari `run_plan` instance lewat `stackly_core::supervisor::probe::run`: sukses terhadap server instance, gagal terhadap server asing dengan `stackly_readiness` dan ID lain, sukses terhadap server asing yang macro nya memuat ID instance → AC-7
- [ ] `... a_port_in_use_refuses_the_start_and_leaves_the_foreign_process` → server asing di port `native` fixed; start gagal di tahap `endpoints` dengan `supervisor.endpoint_in_use`, `suggestedPort` ada, tidak ada proses instance, port tidak diubah, server asing hidup dan menjawab `/ping` → AC-12
- [ ] `... a_data_folder_in_use_fails_the_start_and_is_left_alone` → server asing dengan port sendiri di folder data instance yang berhenti; start gagal, `lastFailure` `process_exited` dengan `exitCode` 76, lalu sesudah budget restart habis instance `failed` dengan `crash_loop` dan `exitCode` 76; baris `Cannot lock file … status` di log instance; server asing dan file `status` utuh → AC-12
- [ ] `cargo test -p stacklyd --test native_runtime_install -- --ignored clickhouse_installs_from_the_cache` → ClickHouse 26.8.30.8 terpasang dari cache tanpa jaringan sampai `installed`, exec check mencetak versi artefak, tanpa `com.apple.quarantine` → AC-14
- [ ] `clickhouse_26_8_passes_conformance` di `apps/daemon/tests/provider_conformance.rs` → `NOT_RUN`, ditunda ke giliran catalog: file itu di luar ownership sesi build ini → AC-14

### Acceptance-criteria coverage

- AC-4 · `clickhouse_only_listens_on_its_two_registry_ports`, `two_clickhouse_instances_keep_their_own_events_table`
- AC-5 · `clickhouse_runs_as_one_foreground_process`; langkah `process` harness konformitas menunggu test konformitas
- AC-6 · `clickhouse_default_user_needs_the_keychain_password`, `clickhouse_managed_users_can_do_nothing_else`
- AC-7 · `clickhouse_lifecycle_in_process` (happy path), `the_readiness_probe_fails_against_a_clickhouse_with_another_id`
- AC-8 · `two_clickhouse_instances_keep_their_own_events_table`
- AC-9 · `clickhouse_lifecycle_in_process`
- AC-10 · `clickhouse_reload_changes_the_log_level_and_the_password`
- AC-11 · `clickhouse_lifecycle_in_process` (`instance.get_detail`); `--ask-password` di langkah manual
- AC-12 · `a_port_in_use_refuses_the_start_and_leaves_the_foreign_process`, `a_data_folder_in_use_fails_the_start_and_is_left_alone`; tafsiran: kode akhir `crash_loop` sesudah `process_exited`, karena engine menjalankan restart policy pada exit yang tidak diminta
- AC-14 · `clickhouse_installs_from_the_cache`; `clickhouse_26_8_passes_conformance` menunggu giliran catalog

## Milestone 3: catalog, corpus, spike, dan ukuran

### UI / manual

- Tidak ada langkah manual untuk langkah 10 selain run spike native di Milestone 2.

### Commands

- [ ] `grep -n "ClickHouse\|F43" xtask/src/runtime/spike/providers.rs` → tidak ada hasil: struct recipe `ClickHouse`, fungsi `xml`, cabang `"clickhouse"`, dan penanda `// provisional: F43` hilang → AC-15
- [ ] `cargo test -p xtask --bin xtask spike` → `the_packaged_providers_are_found_by_their_software` menemukan `providers/clickhouse`, dan `clickhouse_starts_from_its_package_with_the_users_file_and_reads_its_version_line` memulai ClickHouse dari paket: dua port di `config.xml`, `users.xml` di `secrets/` dengan hash dan tanpa password fixture, `CLICKHOUSE_WATCHDOG_ENABLE=0`, `version_args` `server --version`, dan versi dari baris `ClickHouse server version 26.8.30.8 (official build).` → AC-15

### Acceptance-criteria coverage

- AC-15 · langkah 10; bentuk hasil spike tidak berubah, run native nya di langkah manual Milestone 2
- AC-16 (catalog, corpus, BR-025), AC-13 (catatan platform), AC-17 · langkah 11 sampai 13, belum
