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

## Milestone 2: NATS 2.15.1 asli lewat daemon

Commit build: `5330872d` (langkah 4, harness dan client uji), `2fde5147` (langkah 5, `nats_auth_in_process`), `aa440b21` (langkah 6, empat check baru `nats_compatibility`), `b25cd657` (langkah 7, `nats_journey`).

_Hasil developer 2026-11-12: semua langkah Commands di bawah dijalankan pada `nats-server` 2.15.1 dari cache runtime xtask (sha256 `ed0fe392…5ffd`), tanpa unduhan, dan lulus._

### UI / manual

- [ ] Tidak ada langkah manual di milestone ini; pemeriksaan GUI AC-11 ada di Milestone 3.

### Commands

- [x] `cargo test -p stacklyd --test nats -- --ignored nats_lifecycle_in_process` → instance `1.1.0` ready lewat `nats-readiness`; `nats.conf` tidak ada sesudah create, ada di `<run>/secrets/nats.conf` dengan mode `0600` di folder `0700` dan memuat `password: "stackly-test-nats-a"`, tidak pernah di folder `config`, dan hilang setelah stop bersih; `/varz` melaporkan `max_memory` 1073741824 dan `max_storage` 10737418240 → AC-4, AC-5, AC-9
- [x] `cargo test -p stacklyd --test nats -- --ignored nats_applies_a_setting` → `/varz` `max_payload` 1048576 dengan default, `instance.config.apply` `maxPayloadKiB` 2048 berdampak restart dan `/varz` melaporkan 2097152, lalu kembali ke 1024 sebagai revision 3 → AC-9
- [x] `cargo test -p stacklyd --test nats -- --ignored nats_auth_in_process` → `stackly` dengan password membuat stream `ORDERS` dan publish; password salah `-ERR 'Authorization Violation'`; koneksi tanpa credential: publish `orders.two`, subscribe `>`, dan `$JS.API.STREAM.CREATE.X` mendapat `-ERR 'Permissions Violation`; `PUB $JS.API.INFO orders.evil 0` tidak menambah pesan `ORDERS` (tetap 1); `$JS.API.INFO` dari `_INBOX.stackly.*` melaporkan `streams` 0; dengan `jetstream: false` publish `$JS.API.INFO` ditolak; password `p"q\r$s-fixture` login berhasil dan `p"q\r` ditolak; password tidak ada di `ps -E` (argv dan env), di file mana pun di bawah data root dan folder run selain `nats.conf`, di `/varz`, `/connz?auth=1&subs=1`, `/jsz`, `/accountz`, `/subsz`, dan `masked` `nats://stackly:****@127.0.0.1:<port>`; `instance.config.get` mengembalikan `effectiveFiles` kosong dan `drift` kosong; 60 detik setelah `ready`, `total_connections` naik paling sedikit 3 dan `FakeSecretStore::count(SecretOp::Read)` tidak bertambah → AC-2 (native), AC-3, AC-4, AC-5
- [x] `cargo test -p stacklyd --test nats -- --ignored nats_compatibility` → check lama `init`, `start`, `readiness`, `honest_readiness`, `identity_check`, `jetstream_mode`, `adoption` lulus lagi pada `1.1.0`; check baru `client_auth` (AC-3 lewat daemon biner dan file keychain root override), `isolation` (dua instance `iso-a` dan `iso-b`, stream file `ORDERS` masing masing 3 dan 5 pesan, `server_name` dan `store_dir` sendiri, password silang ditolak, `lsof` hanya dua port efektif di `127.0.0.1`), `persistence` (setelah `instance.restart` tepat 3 dan 5 pesan dengan isi `a-<n>` dan `b-<n>`; stream file `F` 10 pesan, stream memory `M` `stream not found` setelah restart; setelah `SIGKILL` ke `nats-server` `F` masih ada), `monitoring_conflict` (listener test di port monitoring `Fixed`: start gagal di stage `endpoints` dengan `supervisor.endpoint_in_use`, `endpoint` `monitoring`, `port`, dan `suggestedPort` sesudahnya; instance tetap `stopped` tanpa proses dan tanpa `lastFailure`, port tetap, listener masih menerima koneksi; setelah `instance.change_endpoints` ke `suggestedPort` start `ready` dan `/varz` terbaca di sana) → AC-3, AC-5, AC-6, AC-7, AC-8
- [x] `cargo test -p stacklyd --test nats -- --ignored nats_refuses_reload nats_reports_one` → reload ditolak, restart, dan metrics tetap lulus pada `1.1.0` → AC-5
- [x] `cargo test -p stacklyd --test provider_conformance -- --ignored nats_2_15_passes_conformance` → lulus untuk skenario `default` dan `no-jetstream` → AC-10
- [x] `cargo xtask provider test providers/nats` → exit 0, `default: pass`, `no-jetstream: pass`; di kedua skenario langkah `form`, `create`, `connection`, `second_instance`, `restart`, `handover`, `leftovers`, dan `start_again` `pass`, `reload` `skipped` → AC-10
- [x] `cargo test -p stackly-cli --test nats_journey -- --ignored` → create dengan `--secret clientPassword=generate` (default dan `--set jetstream=false`), start, `inspect --json` ready, `instance connection <name>` mencetak `nats://stackly:****@127.0.0.1:<client port>`, dan stop → AC-11

### Acceptance-criteria coverage

- AC-2 · bagian native: password `p"q\r$s-fixture` di `nats_auth_in_process`
- AC-3 · `nats_auth_in_process` dan check `client_auth`
- AC-4 · `nats_lifecycle_in_process` (lokasi, mode, hapus setelah stop) dan `nats_auth_in_process` (pencarian byte, monitoring, `masked`, `effectiveFiles`, `drift`)
- AC-5 · `nats_auth_in_process` (`total_connections` dan bacaan secret), check lama `nats_compatibility`, ready di kedua mode JetStream; `git diff 90dc30dc HEAD -- crates/core/src/provider/probe crates/core/src/supervisor/probe` kosong
- AC-6, AC-7, AC-8 · check `isolation`, `persistence`, `monitoring_conflict`
- AC-9 · `nats_lifecycle_in_process` dan `nats_applies_a_setting…`
- AC-10 · konformitas dan `cargo xtask provider test`
- AC-11 · bagian CLI di `nats_journey`; bagian GUI di Milestone 3 (manual)

## Milestone 3: Evidence, catalog, corpus, dan ukuran RISK-07

Commit build: `18dc346b` (langkah 8, test reload dan restart NATS mencatat subject evidence, supaya run native tanpa record tidak valid), `e6c2ccdf` (langkah 8, evidence, `versions.json`, ledger catalog, fixture dan golden status dukungan kembali `supported`, dan tiga expectation `provisional: F39 step 8` dikembalikan).

_Hasil developer 2026-11-12: run native langkah 8 dijalankan dari `HEAD` bersih `18dc346b`. Record `nats_compatibility` `01a1cd97-ea52-7721-8b06-7d7ad8760438` mencatat `actual` "file stream F held 10 of 10 messages after SIGKILL of nats-server"._

### UI / manual

- [ ] Run launchd AC-13, di sesi login GUI kamu: `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test native_launchd -- --ignored --test-threads 1 nats_survives` → `nats_survives_planned_restart_and_sigkill_and_returns_after_kickstart_and_bootout` lulus dengan paket `1.1.0`; catat ID record evidencenya di sini. Sampai dijalankan `BLOCKED`, owner `@ojiepermana`. Kalau gagal, build berhenti dan kembali ke `/architect` → AC-13
- [ ] Pemeriksaan GUI AC-11: di app, buka sheet New Instance, pilih NATS Server, biarkan Client password pada Generate, buat instance, start, lalu buka Summary → Client URL tampil `nats://stackly:****@127.0.0.1:<port>` dengan tombol salin, dan hasil salin memuat password yang dibangkitkan → AC-11

### Commands

- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test nats --locked -- --ignored` dari `HEAD` bersih → 6 record native `PASS`, 0 problem; run `01a1cd97-b592-729a-83ed-1ab017f2ddb7` → AC-12
- [x] `cargo xtask runtime compat-record target/evidence/01a1cd97-b592-729a-83ed-1ab017f2ddb7/report.json` → `evidence/compatibility/01a1cd97-b592-729a-83ed-1ab017f2ddb7/`; `compatibility.json` berisi `providerRevision` `official/nats` `1.1.0` `1a9b16f6…`, check `adoption`, `client_auth`, `honest_readiness`, `identity_check`, `init`, `isolation`, `jetstream_mode`, `monitoring_conflict`, `persistence`, `readiness`, `start`, `stop`, dan `knownLimitations` `["power_loss_not_covered"]` → AC-12
- [x] `cargo xtask codegen` lalu `cargo xtask codegen --check` → `providers/nats/versions.json` mendapat record `provider` `1.1.0`; "20 generated files are up to date" → AC-12
- [x] `cargo xtask catalog check --update --amend` lalu `cargo xtask catalog check` → `ok catalog revision 12 aa5fe68d…` (revision 12 belum di `main`, jadi diamend) → AC-12
- [x] `cargo xtask provider check` → `ok official/nats 1.1.0 1a9b16f6…`; `git diff c545bb33 HEAD -- crates/core/tests/fixtures/compat/nats` kosong (entri corpus tidak berubah di langkah 8) → AC-12
- [x] `cargo test -p stackly-core`, `cargo test -p stackly-protocol`, `cargo test -p xtask`, `cargo test -p stacklyd --lib`, `cargo test -p stackly-cli --test provider`, `swift test --package-path clients/swift`, `swift test --package-path apps/macos/Packages/StacklyUI` → lulus; NATS 2.15.1 kembali `supported (provider_tested)` di fixture catalog, golden CLI, dan snapshot (`git diff main -- …/__Snapshots__/CatalogSnapshotTests …/RuntimesSnapshotTests` kosong); `crates/core/src/provider/versions.rs` (modul test) kini mengharapkan empat record `provider` NATS → AC-12
- [x] Ukuran RISK-07: commit dasar `90dc30dc`; commit build tanpa merge `13347faf`, `7262515c`, `c545bb33`, `d83230a4`, `5330872d`, `2fde5147`, `aa440b21`, `b25cd657`, `18dc346b`, `e6c2ccdf`. Baris yang ditambah ke file `.rs` dan `.swift` di luar `tests/`, `Tests/`, fixture, dan golden yang memuat `nats` (tanpa peduli huruf besar) hanya ada di `13347faf`: `crates/core/src/provider/template.rs` (allowlist; doc comment, komentar arm, dan unit test) dan modul `#[cfg(test)]` `crates/core/src/secret/delivery/mod.rs` (bukan kode sumber non test). `FixtureConfiguration.swift` tidak mendapat baris yang memuat `nats`. `git diff --numstat 90dc30dc HEAD -- crates/core/src/provider/template.rs` → `75 13` (satu arm `match` plus unit test, sesuai baris 1 tabel). `crates/protocol/src/version.rs`, migrasi, `crates/core/src/provider/probe/`, `crates/core/src/supervisor/probe/`, `crates/core/src/supervisor/handover.rs`, dan `runtimes/inventory.toml` tidak berubah → AC-14

### Acceptance-criteria coverage

- AC-12 · evidence, `versions.json`, ledger catalog, corpus `1.1.0` dari langkah 2 tidak berubah, `covers` guard `REQ-036` plus `AC-036`, `AC-077`, `AC-010` di `nats_compatibility`
- AC-13 · `HANDOVER_PROVEN` tidak berubah; run launchd menunggu langkah manual di atas (`BLOCKED`, owner `@ojiepermana`)
- AC-14 · dicatat di atas, tanpa selisih dengan tabel RISK-07
- AC-11 · bagian GUI menunggu langkah manual di atas
