# Verify: Provider Valkey · spec 0040 · updated 2026-11-12

_Langkah diturunkan dari acceptance criteria spec 0040. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Bagian Milestone 1 memuat langkah 1 sampai 4 Build plan (perubahan core, paket, golden, fixture, spike); Milestone 2 memuat langkah 5 sampai 7 (Valkey 9.1.2 asli lewat daemon)._

## Milestone 1: Perubahan core dan paket, dibuktikan di fixture

### UI / manual

- [ ] Buka `docs/providers/manifest-reference.md` → baris `format` di tabel `configFiles` menyebut `valkeyConf` (teks berkutip ganda dengan `\` dan `"` di escape, boolean `yes` dan `no`) → AC-2
- [ ] Buka `docs/providers/manifest-reference.md` → baris `sensitive` di tabel `configFiles` menyebut `{{ secret.<field> \| sha256Hex }}` (SHA-256 sebagai 64 karakter hex huruf kecil, tanpa escape dan tanpa kutip, satu satunya filter) → AC-3
- [ ] Buka `providers/valkey/README.md` → tabel tiga mode persistence sama dengan *Durability per mode* spec 0040, kalimat bahwa snapshot buatan sendiri (`SAVE`, `BGSAVE`, save point lewat `CONFIG SET`) dimuat lagi di start berikutnya juga di mode `volatile`, user `stackly-readiness` hanya `PING`, `CONFIG SET` hanya sampai restart, dan `redis://` hanya skema klien → AC-1, AC-7
- [ ] Buka `providers/valkey/LICENSE-NOTICES.md` → tiga heading spec 0038; `## Runtime` menyebut `BSD-3-Clause`, link `COPYING` 9.1.2, dan "Built by Stackly maintainers from upstream source" → AC-1
- [ ] Di app: buka form create Valkey → teks field `Persistence` tampil di bawah kontrolnya dan memuat `loses all data` dan `nothing to back up or restore`; buat instance `volatile` → halaman Configuration menampilkan field itu terkunci dengan nilai `volatile` dan file `persistence-volatile.conf` dengan komentar yang sama → AC-7 (langkah manual, file manual lead)
- [ ] Fuzz 10 menit target `provider_template` (`cargo xtask fuzz run provider_template --time 600`) sesudah langkah 1 dan 2 → tanpa crash → AC-2, AC-3 (langkah manual, file manual lead)

### Commands

- [ ] `cargo test -p stackly-core --lib provider::template::tests::valkey_conf` → 3 test lulus: `a"b\c d ü #x` menjadi `"a\"b\\c d ü #x"`, `$`, `'`, backtick, dan kurung kurawal keluar apa adanya, setiap karakter kontrol (U+0000 sampai U+001F, U+007F sampai U+009F) ditolak dan U+00A0 diterima, boolean `yes` dan `no`, tag diterima, output tidak diparse ulang, dan `from_code("valkeyConf")` mengembalikan varian baru (kode lain dengan huruf berbeda tidak) → AC-2
- [ ] `cargo test -p stackly-core --lib secret::delivery::tests::a_placeholder_renders_with_the_escaper_of_its_format` → `render_secret_placeholder` memakai escaper `valkeyConf` dan menolak secret berisi newline → AC-2
- [ ] `cargo test -p stackly-core --test provider_rules valkey_conf_renders_quoted_text_decimal_integers_and_yes_no_booleans` → fixture fake dengan `format: valkeyConf` merender teks berkutip, integer desimal, boolean `yes`/`no`, dan secret lewat `materialize`; secret dan path data dengan karakter kontrol gagal `UnrepresentableValue` dengan nama field, tanpa nilainya → AC-2
- [ ] `cargo test -p stackly-core --test provider_rules an_unknown_format_names_every_known_format_in_its_message` → `format: ini` ditolak `unknown_template_format`, dan pesannya menyebut `valkeyConf` bersama setiap format lain yang dikenal → AC-2
- [ ] `cargo test -p stackly-core --lib provider::template::tests::an_output_tag_takes_filter_names_after_a_bar_with_any_spacing provider::template::tests::an_empty_or_odd_filter_name_is_a_syntax_error_at_the_tag` → filter dibaca sesudah `|` dengan spasi bebas; nama kosong, `||`, `sha-256`, `sha256_hex`, nama berspasi, huruf non ASCII, dan path kosong menjadi kesalahan sintaks di posisi tag → AC-3
- [ ] `cargo test -p stackly-core --test provider_rules a_sha256_hex_filter_writes_the_hash_of_the_secret_with_no_escaper` → template `json` sensitif dengan `{{ secret.authToken|sha256Hex }}` lolos validasi (render contoh dengan hash dari `x` diparse ulang), plan memuat `Segment::SecretHash`, `materialize` menulis hash SHA-256 `abc` mentah di dalam kutip literal template, secret yang tidak ada menjadi `SecretUnavailable`; di `valkeyConf` hash dari nilai berisi `"` dan newline keluar tanpa escaper dan tanpa kutip → AC-3
- [ ] `cargo test -p stackly-core --test provider_rules a_hash_segment_shows_only_the_secret_reference_in_the_plan` → segmen di JSON plan berbentuk `{ "type": "secretHash", "content": { "secret": "<secret:authToken>", "algo": "sha256_hex" } }`, dan `Debug` serta JSON plan tidak memuat referensi secret atau hash → AC-3
- [ ] `cargo test -p stackly-core --test provider_rules a_filter_is_rejected_off_a_secret_path_with_its_code_and_position` → filter sesudah `settings.*` atau `instance.*`, nama `md5` atau `Sha256Hex`, dan dua filter ditolak `template_type` dengan baris dan kolom; `{{ secret.authToken | }}` dan `sha-256` ditolak `template_syntax`; tag dengan filter di format `plain` tetap `template_type`; secret dengan atau tanpa filter di config yang tidak `sensitive` tetap `secret_not_allowed_here` → AC-3
- [ ] `cargo test -p stackly-core --lib secret::delivery::tests::a_hash_algo_serializes_as_its_code` → `HashAlgo::Sha256Hex` diserialisasi sebagai `"sha256_hex"` → AC-3
- [ ] `cargo test -p stackly-core --test provider_compat` → corpus beku lulus tanpa satu pun `frozen.json` berubah (`git status crates/core/tests/fixtures/compat` bersih) → AC-2, AC-3 (syarat additive 3)
- [ ] `cargo test -p stackly-core --test fuzz_regressions` dan `cargo test -p xtask fuzz` → seed `fuzz/seeds/provider_template/format.valkey.conf.tmpl` (selector 6, setup `valkeyConf`) dan `filter.sha256hex.json.tmpl` (selector 0, filter `sha256Hex`) lulus harness tanpa panic, dan `cargo xtask fuzz seed` tidak mengubah `fuzz/seeds/` → AC-2, AC-3 (syarat additive 6)
- [ ] `cargo xtask provider check providers/valkey` → satu baris `ok official/valkey 1.0.0 <sha256>`; `cargo xtask provider check` tanpa argumen → enam baris `ok`, termasuk `official/valkey` → AC-1
- [ ] `cargo xtask codegen --check` → `providers/valkey/versions.json` sesuai inventory, dengan tepat satu artefak `available` `valkey-9.1.2-r2-macos-aarch64` line `9.1` → AC-1
- [ ] `cargo test -p stackly-core --test provider_packages valkey` → `the_valkey_package_reads_without_writes_and_validates` (persis 11 file paket, capability `cold_backup`, `multi_instance`, `persistence`, tanpa data marker), `valkey_plans_match_the_golden_files` (`init.json`, `run.json`, `run-aof.json`, dan `render-<config>.txt` kelima config sama byte demi byte; argv `persistence-rdb.conf` untuk default dan `persistence-aof.conf` untuk `aof`; tidak ada argumen yang sama dengan secret), `valkey_users_conf_holds_only_the_hash_of_the_password_in_the_secrets_folder`, `valkey_programs_are_runtime_executables_with_the_base_env_and_no_redis_fallback`, `the_valkey_persistence_field_is_fixed_after_init_and_says_what_volatile_loses`, `valkey_remedies_match_their_failures_and_render_like_the_golden_file`, `valkey_offers_tableplus_and_valkey_cli_in_terminal` lulus → AC-1, AC-4 (plan), AC-7 (teks), AC-9 (schema), AC-12 (plan), AC-18 (loader)
- [ ] `cargo test -p stackly-core --test provider_packages every_repo_package_reads_its_version_from_the_literal_output_of_its_binary the_repo_packages_declare_their_connection_entries_in_manifest_order every_package_initializes_with_endpoints_that_have_no_port` → baris Valkey (`--version`, `9.1.2`; connection `uri` dan `valkey`; init tanpa port) lulus → AC-1, AC-16, AC-18
- [ ] `cargo test -p stacklyd --test create_form_fixtures` → `protocols/fixtures/instance/create-form-valkey.json` sama dengan form yang dibangun, dan memuat `description` field `persistence` persis → AC-1, AC-7
- [ ] `cargo xtask fuzz seed` → `fuzz/seeds/` tidak berubah (`git status fuzz` bersih) → AC-1
- [ ] `cargo xtask docs check` → `ok docs`, termasuk `providers/valkey/LICENSE-NOTICES.md` → AC-1
- [ ] `cargo test -p xtask runtime::spike` → `valkey_starts_from_its_package_with_the_users_file_and_reads_its_version_line` (recipe dari paket, `users.conf` tanpa password fixture, pattern cocok dengan `Valkey server v=9.1.2 ...` dan tidak cocok dengan `valkey-cli 9.1.2`) dan `the_packaged_providers_are_found_by_their_software` (`valkey` ditemukan) lulus; `grep -n "F40\|struct Valkey" xtask/src/runtime/spike/providers.rs` kosong → AC-16 (spike, tanpa run native)
- [ ] `cargo test -p xtask --test provider_check` → daftar paket repo memuat `official/valkey` → AC-1

### Acceptance-criteria coverage

- AC-1 · paket, `versions.json`, golden, fixture form, `docs check`; test form `StacklyUI` tidak dijalankan sesi developer (tanpa kode Swift baru; lead menjalankan gerbang Swift)
- AC-2 · format `valkeyConf` di core; round trip native di Milestone 2
- AC-3 · filter `sha256Hex` dan `Segment::SecretHash` di core dan di `users.conf` paket (golden berisi hash fixture, bukan nilainya)
- AC-7 · teks `description` di schema dan fixture; tampilan di form dan Configuration adalah langkah manual
- AC-12 · bagian plan (program di root runtime, env basis plus env manifest, tanpa `redis` selain skema URL); bagian native di Milestone 2
- AC-16 · bagian spike (recipe dari paket); version check exec di Milestone 2; run spike native di Milestone 3
- AC-18 · bagian loader (dua connection, dua `openWith`, dua remedy dan golden nya); bagian native di Milestone 2
- AC-4 sampai AC-6, AC-8 sampai AC-11, AC-13 · Milestone 2; AC-14, AC-15, AC-17 · Milestone 3; AC-19 · Milestone 4

## Milestone 2: Valkey 9.1.2 asli lewat daemon

_Semua test native: butuh artefak `valkey-9.1.2-r2-macos-aarch64` di cache runtime xtask; tanpa artefak setiap test berakhir `BLOCKED` dengan `artifact valkey-9.1.2-r2-macos-aarch64 is not in the runtime cache`, owner `@ojiepermana`, tanpa unduhan._

### UI / manual

- [ ] Dengan TablePlus terpasang: pilih `Open in TablePlus` → koneksi Redis terbuka ke port instance dan password dari clipboard diterima; kalau TablePlus menolak URL tanpa password, entri itu dihapus lewat revision paket baru → AC-18 (langkah manual, file manual lead)
- [ ] Pilih `Open valkey-cli in Terminal`, tempel password → prompt `valkey-cli` terbuka, password tidak tampil di `ps` → AC-18 (langkah manual, file manual lead)

### Commands

- [ ] `cargo test -p stacklyd --test valkey -- --ignored lifecycle::` → create dengan password bangkitan, `not_ready` sebelum `ready` dari event, baris log Valkey ber jam UTC lewat `instance.logs_read`, `instance.lock` di fd 3, argv memuat path config instance, stop; adopsi PID dan `startedAt` yang sama sesudah `SIGKILL` ke `stacklyd`; `secret.set` melaporkan `next_start` dan sesudah restart password lama `WRONGPASS`, yang baru diterima → AC-13
- [ ] `cargo test -p stacklyd --test valkey -- --ignored auth::` → satu listener `127.0.0.1:<port>`, tanpa UDP dan tanpa socket Unix; `NOAUTH`; `AUTH <pw>` dan `AUTH default <pw>` dengan password `p w"x\y'z`; `WRONGPASS`; `stackly-readiness` bisa `PING` dan `NOPERM` untuk `GET`, `INFO`, `CONFIG GET`; password tidak ada di `ps`, di file mana pun di root (selain keychain test), di log daemon; `users.conf` 0600 berisi hash dan hilang sesudah stop → AC-4
- [ ] `cargo test -p stacklyd --test valkey -- --ignored readiness::` → salinan paket dengan `--key-load-delay 2000`: `not_ready` dengan detail `LOADING` dan argv probe yang dijalankan sendiri keluar 1 dengan `LOADING`, lalu `ready` dan probe keluar 0; `ACL DELUSER stackly-readiness` → `not_ready` dengan `WRONGPASS` sementara TCP menerima, `ACL SETUSER ...` → `ready` → AC-5
- [ ] `cargo test -p stacklyd --test valkey -- --ignored owned::` → `CONFIG REWRITE` (`The server is running without a config file`), `ACL SAVE`, `CONFIG SET dir`/`dbfilename` (`can't set protected config`) ditolak; `maxmemory 1mb` hilang sesudah restart; folder config sama; port `Auto` tetap → AC-10
- [ ] `cargo test -p stacklyd --test valkey -- --ignored stop::` → klien `BLPOP q 600`, stop di bawah 10 detik, `stopped`, exit 0 tanpa sinyal, log memuat `DB saved on disk` → AC-11
- [ ] `cargo test -p stacklyd --test valkey -- --ignored tools::` → umpan `valkey-server`, `valkey-cli`, `redis-server`, `redis-cli` di depan `PATH` daemon tidak pernah jalan; `INFO server` melaporkan `valkey_version` 9.1.2 dan `executable` di root runtime → AC-12
- [ ] `cargo test -p stacklyd --test valkey -- --ignored format::` → `CONFIG GET proc-title-template` membaca kembali `a"b\c d ü #x` dari salinan paket; `notifyKeyspaceEvents` `K$` start dan `CONFIG GET notify-keyspace-events` memuat `K` dan `$` → AC-2
- [ ] `cargo test -p stacklyd --test valkey -- --ignored persistence::` → tiga record evidence, `actual` masing masing `valkey 9.1.2 rdb: normal stop kept, process crash lost since last snapshot`, `valkey 9.1.2 aof: normal stop kept, process crash kept`, `valkey 9.1.2 volatile: normal stop lost, process crash lost`; `dump.rdb`, `appendonlydir/` (manifest, base, incr), atau folder data kosong; `instance.config.validate` dengan `persistence: aof` pada instance `rdb` → `immutable_after_init` di `settings.persistence` → AC-6, AC-7, AC-8, AC-9
- [ ] `cargo test -p stacklyd --test native_runtime_install -- --ignored valkey` → `valkey_9_1_installs_from_the_cache` lulus: install dari cache tanpa jaringan, exec check versi `9.1.2` lewat `runtime.versionCheck` paket, tanpa `com.apple.quarantine` → AC-16 (version check)
- [ ] `cargo test -p stacklyd --test valkey -- --ignored doctor::` → `instance.get_detail` memuat connection `uri` (`redis://`) dan `valkey` (`valkey://`) dengan `masked` absolut tanpa password, `openActions` `tableplus` dan `cli` dengan `copySecret` dan tanpa password atau token secret; doctor memberi `readiness_user` saat user readiness hilang dan `aof_damaged` (excerpt `Bad file format reading the append only file`) saat file incr dirusak, file itu tidak diubah → AC-18

### Acceptance-criteria coverage

- AC-2 · round trip native (`format::`); bagian core di Milestone 1
- AC-4 · `auth::` (+ plan di Milestone 1)
- AC-5 · `readiness::`
- AC-6, AC-7, AC-8, AC-9 · `persistence::` (teks AC-7 di Milestone 1; tampilan GUI manual)
- AC-10 · `owned::`
- AC-11 · `stop::`
- AC-12 · `tools::` (+ plan di Milestone 1)
- AC-13 · `lifecycle::`; pemeriksaan `stackly instance connection` mencetak connection pertama tidak diuji khusus Valkey (perilaku CLI generik spec 0022)
- AC-16 · version check lewat `native_runtime_install`; run spike native `valkey_9_1_runs_from_managed_artifact` menunggu Milestone 3
- AC-18 · `doctor::` (+ loader di Milestone 1); Open In di GUI manual
- AC-14, AC-15, AC-17 · Milestone 3; AC-19 · Milestone 4
