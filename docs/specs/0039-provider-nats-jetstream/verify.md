# Verify: Provider NATS & JetStream · spec 0039 · updated 2026-11-12

_Langkah diturunkan dari acceptance criteria spec 0039. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri. Jalankan dari root repo dengan `export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`._

## Milestone 1: Paket `1.1.0` dan escaper, di fixture

Commit dasar: `90dc30dc` (main sesudah spec 0039 dan log `dev-f39-m1`). Commit build: `13347faf` dan `7262515c` (langkah 1, escaper dan test `materialize`), `c545bb33` (langkah 2, paket, golden, fixture, catalog revision 12, corpus `1.1.0`), dan commit langkah 3 (test, fixture, dan snapshot Swift).

_Hasil developer 2026-11-12: semua langkah Commands di bawah dijalankan dan lulus. `swift test` StacklyUI memakai `STACKLY_DAEMON_BIN` dan `STACKLY_SEED_BIN` dari `target/debug` checkout utama (protokol sama). Snapshot `catalog-available`, `catalog-outdated`, `runtime-get-*`, dan `config-nats` direkam ulang: NATS 2.15.1 tampil `experimental (runtime_tested)` sampai langkah 8, dan form NATS memuat `Client password`. Langkah 8 merekam ulang snapshot catalog dan runtime serta mengembalikan tiga expectation bertanda `provisional: F39 step 8` (`crates/core/src/catalog/status.rs`, `CatalogContractTests.swift`, `CatalogViewsTests.swift`)._

### UI / manual

- [ ] Tidak ada langkah manual di milestone ini. Pemeriksaan GUI AC-11 dan run launchd AC-13 masuk Milestone 3 (langkah 9).

### Commands

- [x] `cargo test -p stackly-core --lib provider::template::tests::nats_conf` → enam test lulus: `"` menjadi `\"`, `\` menjadi `\\`, `$` keluar apa adanya, gabungan `p"q\r$s-fixture` menjadi `"p\"q\\r$s-fixture"`, setiap karakter kontrol C0, DEL, dan C1 (`\n`, `\t`, `\r`, U+0000, U+001F, U+007F, U+0085, U+009F) ditolak, dan karakter cetak lain (spasi, `#`, `{`, `}`, `'`, non ASCII, U+00A0, string kosong) keluar apa adanya di dalam kutip ganda → AC-2
- [x] `cargo test -p stackly-core --lib secret::delivery` → `render_secret_placeholder` untuk `natsConf` menulis `a"b` dan `p"q\r$s-fixture` ber escape dan menolak `a\nb` → AC-2
- [x] `cargo test -p stackly-core --test provider_rules nats_conf` → segmen secret lewat `RenderedContent::materialize` menulis `p"q\r$s-fixture` ber escape, secret dengan tab gagal `unrepresentable_value` di `secret.authToken` tanpa nilainya, path dengan `"`, `\`, `$` dirender, dan path dengan U+007F gagal di `instance.data` tanpa nilainya → AC-2
- [x] `cargo test -p stackly-core --test provider_compat` → 3 test lulus, `git status crates/core/tests/fixtures/compat/nats/1.0.1` bersih (tidak ada `frozen.json` yang berubah) → AC-1, AC-2
- [x] `cargo xtask fuzz run provider_template --time 600` → selesai tanpa crash → AC-2
- [x] `cargo xtask provider check` → baris `ok official/nats 1.1.0 1a9b16f60a26f19b9c3583e234d5cae65b13d8a1174ecbb4c520c0a171155621` tanpa temuan → AC-1
- [x] `cargo test -p stackly-core --test provider_packages` → lulus tanpa `STACKLY_UPDATE_GOLDEN`: revision `1.1.0`; golden `init.json` hanya `ensureDirectories` (sekarang juga folder run), `run.json` menulis `<run>/secrets/nats.conf` dengan `sensitive: true` dan argumen `--config <run>/secrets/nats.conf`; `render-natsConf.txt` memuat akun `APP` dan `STACKLY_READINESS`, `no_auth_user`, `1024KiB`, `1024MiB`, `10240MiB`; `render-natsConf-no-jetstream.txt` tanpa `jetstream` dan dengan izin `deny: ">"`; kutip di folder data dirender `\"`, karakter kontrol gagal tanpa nilainya → AC-1, AC-2, AC-9
- [x] `git grep -n "password:" crates/core/tests/golden/provider/nats/` → hanya `<secret:clientPassword>`, tidak pernah nilai → AC-1
- [x] `cargo test -p stackly-core --test connection_render` → golden `nats/connection.json`: `masked` `nats://stackly:****@127.0.0.1:14000` dengan bagian secret `clientPassword` → AC-1
- [x] `cargo test -p stackly-core --test config_evaluate --test instance_duplicate --test provider_check` → lulus; NATS punya field secret `clientPassword` (duplikat menawarkan `generate`), dan `check_package` membaca skenario `default` dan `no-jetstream` (`{ jetstream: false }`) → AC-9, AC-10
- [x] `cargo test -p stackly-core` → seluruh suite lulus. Catatan: `catalog::status::tests::an_available_host_artifact_takes_its_support_from_its_records_and_notes_fill_empty_columns` sementara mengharapkan `experimental (runtime_tested)` untuk NATS 2.15.1 (`// provisional: F39 step 8`), karena record `provider` untuk `1.1.0` baru ada di langkah 8
- [x] `cargo test -p stackly-protocol` → `create_fixtures` membaca form NATS dengan urutan `serverName`, `jetstream`, `clientPassword` (secret, wajib, `printableOnly`, basic, restart), lalu tiga integer advanced; `commands.json` memakai `maxPayloadKiB=2048` → AC-11
- [x] `cargo test -p stacklyd --test create_form_fixtures --test duplicate_form_fixtures` dan `cargo test -p stacklyd --lib map::catalog` → fixture `instance/create-form-nats.json` dan `catalog/provider-nats.json` sama dengan paket asli (revision `1.1.0`, digest baru, NATS 2.15.1 `experimental` sampai langkah 8) → AC-11, AC-12
- [x] `cargo test -p stackly-cli --test provider` → golden `provider_list.json`, `provider_inspect_nats.*` dan `provider validate` mencetak `ok official/nats 1.1.0 1a9b16f6…` → AC-12
- [x] `cargo test -p xtask` → `provider_check.rs` mencetak `ok official/nats 1.1.0` dan `provider freeze providers/nats` menjawab `already frozen … compat/nats/1.1.0` → AC-12
- [x] `cargo xtask catalog check` → `ok catalog revision 12 …`; `git diff main -- crates/core/tests/fixtures/compat/nats/1.0.1` kosong dan `crates/core/tests/fixtures/compat/nats/1.1.0/` ada → AC-12
- [x] `cargo xtask codegen --check` → "20 generated files are up to date" (tidak ada digest kontrak yang berubah) → AC-14
- [x] `cargo xtask fuzz seed` lalu `git status fuzz/seeds` → bersih → AC-11
- [x] `swift test --package-path clients/swift --filter CreateContractTests` → form NATS dengan `clientPassword` secret basic dan tiga integer advanced → AC-11
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` → 2124 test lulus: baris form NATS (password sebagai kontrol secret basic, batas `From 1 to 1048576.` dan `From 1 to 65536.`), revision `1.1.0 · 1a9b16f60a26`, catatan matrix macOS sementara "The runtime ran on this Mac; the provider is not tested with it yet.", dan snapshot `config-nats` dengan baris Client password terkunci → AC-11
- [x] `cargo xtask build-app` → `Stackly.app` terbangun → AC-11

### Acceptance-criteria coverage

- AC-1 · paket, golden plan dan render, golden `connection`, corpus `1.0.1` tidak berubah: Milestone 1
- AC-2 · bagian fixture (unit test escaper, `materialize`, test paket, `provider_compat`, fuzz): Milestone 1; bagian native (password `p"q\r$s-fixture` pada `nats-server` 2.15.1) menunggu Milestone 2 langkah 5
- AC-9 · schema MiB dan KiB dan render suffix: Milestone 1; `/varz` dan `instance.config.apply` menunggu Milestone 2 langkah 4
- AC-10 · file skenario dan `check_package`: Milestone 1; harness dan `cargo xtask provider test providers/nats` menunggu Milestone 2 langkah 7
- AC-11 · fixture form, test kontrak Rust dan Swift, test form Swift: Milestone 1; journey CLI menunggu langkah 7, pemeriksaan GUI menunggu langkah 9 (manual)
- AC-12 · catalog revision 12 dan corpus `1.1.0`: Milestone 1; evidence dan status `supported` kembali menunggu Milestone 3 langkah 8
- AC-3 sampai AC-8, AC-13, AC-14 · menunggu Milestone 2 dan 3
