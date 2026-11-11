# Verify: Provider SDK & test harness · spec 0036 · updated 2026-11-11

_Langkah diturunkan dari acceptance criteria spec 0036. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27, dengan `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`. Probe yang mengubah file sementara selalu dikembalikan sesudahnya (`git status` bersih lagi)._

## Milestone 1: Validator dan kebijakan

Bagian ini mencakup Build plan langkah 1 (`core::provider::check` dan `core::provider::scenarios`), langkah 2 (`cargo xtask provider check` di atas `check_package`), langkah 3 (`provider.validate`, protokol 3.6), langkah 4 (`stackly provider validate`), dan langkah 5 (corpus kompatibilitas dan `cargo xtask provider freeze`).

### UI / manual

- [ ] Fuzz 10 menit (langkah manual user, AC-3): `cargo xtask fuzz run provider_scenarios --time 600` → baris terakhir `fuzz run: provider_scenarios ran for 600 s without a crash`, tidak ada file baru di `fuzz/artifacts/provider_scenarios/`, dan `stat::peak_rss_mb` jauh di bawah batas libFuzzer 2048 MB. Toolchain `nightly-2026-10-17` dan cargo-fuzz 0.13.2 sudah terpasang di Mac ini; smoke run 30 detik saat build lulus (137 ribu eksekusi, peak RSS 323 MB).

### Commands

- [ ] `cargo test -p stackly-core --lib provider::check` → 3 test lulus: `SUPPORTED_API_VERSIONS` berisi tepat `stackly/v1alpha1` berstatus `current`; pesan `apiVersion "stackly/v2" is not supported; this Stackly supports: stackly/v1alpha1` dan `<not a string>`; tanda kutip dan karakter kontrol di nilai di escape, nilai di atas 128 karakter dipotong dengan `...` → AC-1
- [ ] `cargo test -p stackly-core --lib provider::scenarios` → 6 test lulus: urutan dan default (`expect: ready`, settings kosong), `schema` lain berhenti sebelum sisa dokumen di decode, nama `InstanceName` paling banyak 24 karakter dan unik, 0 atau 17 skenario ditolak, field asing, key ganda, tag, merge key, `expect` asing, dan settings bukan mapping ditolak, skenario tersirat `default` → AC-3
- [ ] `cargo test -p stackly-core --test provider_check` → 8 test lulus, semuanya lewat `FakePlatform` tanpa satu pun mutasi filesystem: paket `fake`, `nats`, `postgresql`, `traefik` `publishable` dengan skenario tersirat `default` dan key yang sama dengan `from_files`; `fake-command` `loadable` tetapi tidak `publishable` dengan satu finding `authoring` `token_not_allowed_here` yang sama dengan `init_port_issues`; `apiVersion: stackly/v2` (dengan settings schema dan template yang juga rusak) memberi tepat satu finding `package` `unsupported_api_version` di `provider.yaml` `apiVersion`, sama dengan error `from_files`; `apiVersion` bukan string dan yang hilang; paket yang tidak terbaca atau tidak valid hanya punya finding `package`; skenario valid tidak mengubah `manifest_sha256` dan tidak dibaca `read_package`; setiap aturan skenario yang dilanggar menjadi finding `scenarios` di lokasinya; file 65 KiB, symlink, FIFO, folder, `tests/` symlink, dan bukan UTF-8 ditolak → AC-1 (bagian core), AC-2, AC-3
- [ ] `cargo test -p stackly-core --test provider_rules another_api_version_or_kind_stops_before_the_rest_is_read` → lulus tanpa perubahan: satu issue `unsupported_api_version` → AC-1
- [ ] `cargo test -p stackly-core --test fuzz_regressions` dan `cargo test -p stackly-core --lib provider::fuzzing` → lulus; `provider_scenarios` ada di daftar target dan dua seed nya (`fixture.fake.yaml`, `fixture.invalid.yaml`) melewati harness → AC-3
- [ ] `cargo xtask fuzz list` → memuat `provider_scenarios`; `cargo xtask fuzz build provider_scenarios` → exit 0 dengan toolchain pin; `cargo xtask fuzz seed` lalu `git status --short fuzz/seeds` → bersih (seed `provider_scenarios` sudah ter-commit; seed dari `providers/example/tests/scenarios.yaml` dan `providers/fake/tests/scenarios.yaml` ikut otomatis begitu file itu ada) → AC-3
- [ ] `cargo xtask provider check` → empat baris `ok` (`testing/fake`, `official/nats`, `official/postgresql`, `official/traefik`), exit 0 → AC-4
- [ ] `cargo test -p xtask --test provider_check` → 7 test lulus: format baris lama tetap untuk error `package` dan `authoring` (`init_port_issues`); `apiVersion: stackly/v2` mencetak tepat satu baris `fake: provider.yaml apiVersion: unsupported_api_version: apiVersion "stackly/v2" is not supported; this Stackly supports: stackly/v1alpha1`; `tests/scenarios.yaml` yang rusak mencetak baris `fake: tests/scenarios.yaml scenarios[0].settings.readyDelayMs: settings_value: ...` tanpa baris `ok`; exit 1 → AC-4, AC-1 (bagian xtask). Baris `warning: ` belum bisa dipicu karena belum ada versi atau item `deprecated`; tanpa argumen folder, warning dihitung sebagai masalah (`check_packages(.., warnings_fail = true)`)
- [ ] `cargo test -p stackly-protocol --lib provider` dan `cargo test -p stackly-protocol --test provider_fixtures` → lulus: path 1 sampai 4096 byte, diawali `/`, tanpa NUL, selain itu `invalid_format` di `path`; ketiga enum terbuka membaca nilai asing sebagai `unrecognized`; setiap file di `protocols/fixtures/provider/` terdaftar di `index.json`, fixture `roundtrip` sama persis dengan bentuk tulisan Rust, `omitted` = `findingCount` dikurangi panjang `findings`; frame request, reject path relatif, dan response di `protocols/fixtures/ipc/` → AC-5
- [ ] `cargo test -p stackly-protocol` → lulus, termasuk `ipc_fixtures` (setiap varian `Call` dan `Reply` punya fixture roundtrip), `limits` (`providerPathMax`, `validationFindingsMax`), recovery allowed untuk `provider.validate`, dan `ProtocolVersion::CURRENT` 3.6 → AC-5
- [ ] `cargo xtask codegen --check` → "20 generated files are up to date"; `protocols/contract-versions.json` mencatat 3.6 → AC-5
- [ ] `cargo test -p stacklyd --test provider_validate` → 6 test lulus: salinan `providers/nats` `publishable` dengan identitas dari `ValidatedPackage::key()`, generation registry dan isi root sama sebelum dan sesudah; `apiVersion: stackly/v2` dan `fake-command` menghasilkan persis fixture `validation-unsupported-api-version.json` dan `validation-authoring.json`; folder yang tidak ada dan folder dengan 150 entri asing tetap balasan sukses (`loadable: false`, 100 finding, `findingCount` 101, `omitted` 1); path relatif, kosong, dan ber-NUL ditolak `request.invalid_params` di `path`; method dijawab dalam recovery mode tanpa menyentuh registry; entri catalog yang `provider.yaml` nya `stackly/v2` berstatus `invalid` dengan tepat satu issue `unsupported_api_version` → AC-5 (bagian daemon), AC-1 (bagian method dan catalog)
- [ ] `cargo test -p stacklyd --lib ipc::provider` → 150 finding dipotong ke 100 dengan `omitted` 50 → AC-5
- [ ] `swift test --package-path clients/swift` → lulus, termasuk `ProviderValidateContractTests` (5 test: request, frame balasan, tiga fixture, nilai asing `.unrecognized`), `IpcFixtureTests` (62 method), dan `ContractTests` (protokol 3.6) → AC-5
- [ ] `cargo xtask fuzz seed` lalu `git status --short fuzz/seeds` → bersih; seed `ipc_request_decode` dan `ipc_server_frame_decode` memuat ketiga frame `provider.validate` → AC-5
- [ ] `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` dan `cargo xtask check-deps` → lulus; `check` dan `scenarios` hanya memakai port `Filesystem` → AC-2
- [ ] `cargo test -p stackly-cli --test provider validate` → 3 test lulus terhadap `stacklyd` sungguhan: `providers/nats` mencetak `ok official/nats <revision> <sha256>` lalu `0 error(s), 0 warning(s)`, exit 0, dan `--json` berisi `ProviderValidation` di `result`; salinan `stackly/v2` yang diberikan sebagai path relatif `./pkgs/../pkgs/./mytool` dari folder kerja lain mencetak tepat satu baris `mytool: provider.yaml apiVersion: unsupported_api_version: ...` dan `1 error(s), 0 warning(s)`, exit 2 dengan `cli.provider_invalid` (kategori `invalid_input`, detail `errors` 1 dan `warnings` 0) dan `result` tetap terisi; `fake-command` mencetak baris `token_not_allowed_here` tanpa baris `ok`, dan folder yang tidak ada memberi finding `missing_file`, keduanya exit 2 → AC-5 (bagian CLI), AC-1
- [ ] `cargo test -p stackly-cli --bin stackly` → lulus, termasuk normalisasi path leksikal (`.` dibuang, `..` melipat komponen sebelumnya, tanpa menyentuh filesystem) dan satu konstruktor per kode `cli.*` (`cli.provider_invalid` ada) → AC-5
- [ ] `cargo test -p stackly-protocol --lib codes` → tabel kode `cli` spec 0008 memuat `cli.provider_invalid` (`invalid_input`, detail `errors`, `warnings`); `cargo xtask codegen --check` lulus dan `protocols/error-codes.json` memuatnya → AC-5
- [ ] `cargo test -p stackly-core --test provider_compat` → 3 test lulus: setiap entri `crates/core/tests/fixtures/compat/<nama>/<revision>/` (kini `nats/1.0.1` dan `postgresql/1.0.2`) termuat dengan `NameCheck::Skip` dari salinan di test root, `manifest_sha256` sama dengan `frozen.json`, dan `frozen_facts` (argv start, nama key env start, SHA-256 config yang dirender dalam *Context beku*) sama persis dengan `frozen.json`; setiap entri hanya berisi `package/` dan `frozen.json` → AC-6
- [ ] `cargo xtask provider freeze providers/nats` → `already frozen <repo>/crates/core/tests/fixtures/compat/nats/1.0.1`, exit 0, tanpa perubahan `git status`; `cargo xtask provider freeze providers/fake` → exit 1 `only official/ packages are frozen` → AC-6
- [ ] `cargo test -p xtask --test provider_check` → 9 test lulus, termasuk freeze ulang yang tidak mengubah apa pun, `testing/` yang ditolak, dan salinan `nats` dengan byte lain di revision sama yang ditolak dengan `raise metadata.revision` → AC-6
- [ ] `cargo test -p xtask freeze` dan `cargo test -p xtask not_frozen` → lulus: entri baru berisi byte paket dan `frozen.json`, freeze kedua `already frozen`, digest lain di revision sama ditolak tanpa menulis dan tanpa folder `.partial` tersisa, `testing/` dan paket yang tidak `publishable` ditolak tanpa menulis; `[[provider]]` tanpa entri corpus memberi tepat satu baris `nats: not frozen: run cargo xtask provider freeze providers/nats` dan hilang sesudah freeze → AC-4, AC-6
- [ ] Probe: ubah satu karakter di `providers/nats/templates/nats.conf.tmpl` tanpa menaikkan revision, jalankan `cargo xtask provider check` → baris `nats: not frozen: run cargo xtask provider freeze providers/nats`, exit 1; kembalikan → exit 0 lagi → AC-4

### Acceptance-criteria coverage

- AC-1 · bagian core (tabel, pesan, satu finding di `check_package` dan `from_files`), xtask (`cargo xtask provider check`), method (`provider.validate`), dan catalog (entri `invalid` dengan satu issue); bagian harness menunggu langkah 12
- AC-2 · lengkap
- AC-3 · lengkap kecuali fuzz 10 menit (langkah manual di atas); seed dari `providers/example` menunggu langkah 6
- AC-4 · lengkap, termasuk pemeriksaan `not frozen` tanpa argumen folder
- AC-5 · lengkap (daemon, kontrak, dan CLI)
- AC-6 · lengkap untuk `official/nats` dan `official/postgresql`; paket lain yang naik ke `[[provider]]` dibekukan saat itu (pemeriksaan `not frozen` menjaganya)
- AC-7 sampai AC-16 · milik milestone berikutnya

## Milestone 2: Paket contoh, scaffold, dan form

Bagian ini mencakup Build plan langkah 6 (paket `providers/example/`), langkah 7 (`cargo xtask provider new`), dan langkah 8 (fixture form per folder dan test `StacklyUI` generik).

### UI / manual

- Tidak ada langkah manual untuk langkah 6 sampai 8.

### Commands

- [ ] `cargo xtask provider check providers/example` → `ok testing/example 0.1.0 <sha256>` tanpa finding → AC-7 (bagian paket)
- [ ] `cargo test -p stackly-core --test provider_packages` → lulus, termasuk `the_example_package_reads_without_writes_and_validates` (ID `testing/example`, revision `0.1.0`, software `fake`, capability `reload`, `multiInstance`, `persistence`, field `greeting`, `readyDelayMs`, `authToken` secret), `the_example_package_is_publishable_with_its_two_scenarios` (`check_package` tanpa finding, skenario `default` dengan settings kosong dan `slow-ready` dengan `readyDelayMs: 1500`, keduanya `ready`), `the_example_package_has_the_artifacts_of_the_fake_package` (artefak `versions.json` sama dengan milik `providers/fake`, yang versinya dijaga `provider_no_shell` terhadap crate `stackly-fake-server`), dan `example_plans_match_the_golden_files` (golden `crates/core/tests/golden/provider/example/`: init tanpa render, run me-render `serverConfig` sebelum spawn) → AC-7 (bagian paket)
- [ ] `cargo xtask fuzz seed` lalu `git status --short fuzz/seeds` → bersih; `fuzz/seeds/provider_scenarios/example.scenarios.yaml` ter-commit → AC-3
- [ ] `cargo xtask docs check` → `ok docs`; `providers/example/README.md` ikut diperiksa: blok `bash` nya memuat `cargo xtask provider new mytool` dan `cargo xtask provider check providers/mytool` (keduanya ada di `USAGE`), sedangkan `docs/providers/README.md` dan perintah `test` dan `freeze` tetap code span (bukan link atau blok `bash`) sampai halaman dan perintah itu ada → AC-7
- [ ] `cargo test -p xtask scaffold` → 9 test lulus: nama yang melanggar aturan bagian nama `ProviderName` (kosong, huruf besar, angka di awal, `--`, `-` di akhir, `/`, `_`, 64 karakter) dan `example` ditolak dengan pesan aturannya; huruf pertama dikapitalkan; penggantian hanya untuk baris utuh yang muncul tepat sekali; argumen `<name> [--out <dir>]`; hasil scaffold `mytool` berbeda dari paket contoh hanya pada tiga baris (`# Mytool provider`, `  id: testing/mytool`, `  displayName: Mytool`), file yang sama, folder sementara hilang, dan baris `ok testing/mytool 0.1.0 <sha256>`; folder tujuan yang sudah ada, nama tidak valid, contoh yang drift (`0 times`), dan salinan yang tidak `publishable` tidak meninggalkan file apa pun → AC-7
- [ ] Jalankan `cargo xtask provider new mytool` → exit 0, baris `ok testing/mytool 0.1.0 <sha256>` lalu `next:` dengan `provider check`, `provider test`, README, dan `STACKLY_UPDATE_GOLDEN=1 cargo test -p stacklyd --test create_form_fixtures`; `cargo xtask provider check providers/mytool` → baris `ok` yang sama; ulangi `provider new mytool` → exit 1 `... already exists; nothing was written`; `cargo xtask provider new My` → exit 1 dengan aturan nama; `cargo xtask provider new other --out <folder sementara>` → exit 0 tanpa baris `create_form_fixtures`. Hapus `providers/mytool` dan folder sementara (`git status` bersih) → AC-7
- [ ] `cargo test -p xtask` (seluruhnya) → lulus, termasuk `the_provider_words_cover_every_provider_folder_and_catalog_id` (`example` ada di `PROVIDER_WORDS`) dan `the_repo_packages_pass_with_one_ok_line_each` (lima baris `ok`, `testing/example` pertama); `cargo xtask check-deps` → bersih (`instance::create` dan folder Swift `CreateInstance/` tidak memuat kata `example`)
- [ ] `cargo xtask catalog check` → revision catalog tidak berubah (namespace `testing` tidak masuk catalog); `cargo xtask codegen --check` → up to date
- [ ] `cargo test -p stacklyd --test create_form_fixtures` → lulus; satu fixture `protocols/fixtures/instance/create-form-<folder>.json` untuk setiap folder `providers/` (`example`, `fake`, `nats`, `postgresql`, `traefik`) dari listing folder; nama tampil dari `metadata.displayName`, versi dan line dari artefak pertama `versions.json` untuk target host → AC-8
- [ ] Probe drift: ubah `"default": "hello"` di `providers/example/schemas/settings.schema.json`, jalankan test di atas → gagal `example: rerun with STACKLY_UPDATE_GOLDEN=1`; kembalikan. Hapus sementara `create-form-traefik.json` → gagal dengan path file itu; kembalikan (`git status` bersih) → AC-8
- [ ] `cargo test -p stackly-protocol --test create_fixtures` dan `swift test --package-path clients/swift --filter 'CreateContractTests|CreateClientTests'` → lulus pada fixture yang ditulis ulang → AC-8
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter 'EveryCreateFormFixtureTests|SchemaFormModelTests|CreateInstanceCopyTests|CreateInstanceModelTests'` → lulus; `EveryCreateFormFixtureTests` membaca setiap `create-form-*.json` (paling sedikit lima), membangun `SchemaFormModel`, dan gagal bila ada field ber kind `.unrecognized` atau baris ber control `.unrecognized` → AC-8

### Acceptance-criteria coverage

- AC-7 · lengkap (langkah 6 dan 7); scaffold yang lulus `provider test` menunggu harness langkah 10 sampai 13
- AC-8 · lengkap (langkah 8)
- AC-3 · seed `example.scenarios.yaml` ada; fuzz 10 menit tetap langkah manual Milestone 1
- AC-9 (paket contoh lulus harness) · menunggu langkah 10

## Milestone 3: Harness konformitas

Bagian ini mencakup Build plan langkah 9 sampai 12. Langkah 13 (example `provider_harness`, `cargo xtask provider test` dengan `--cleanup` dan `--keep`, test `xtask` scaffold ke harness) menambah bagiannya sendiri; sampai itu harness dijalankan lewat test `provider_conformance`.

### UI / manual

- Tidak ada langkah manual untuk langkah 9 sampai 12. Semua run memakai artefak `fixture` (`stackly-fake-server`), tanpa unduhan dan tanpa Keychain.

### Commands

- [ ] `cargo test -p stackly-testkit --lib test_root` → 14 test lulus: `TestRoot::new_marked` menulis `.stackly-test-run` (`stackly.test-run/v1`, `runId`, `uid`, `pid`, `createdAt`) di kedua folder dan tidak terhapus saat drop; `remove_checked` menghapus kedua folder run nya sendiri; penanda run lain, penanda hilang, `uid` lain, schema lain, symlink, folder bersarang, dan nama tanpa awalan ditolak dengan reason; folder yang argumennya masih dimuat sebuah proses ditolak sampai proses itu berakhir; `keep_data` menyimpan folder data dan menghapus folder run; `sweep_marked` menghapus root yang `pid` nya mati dan membiarkan yang hidup; root tanpa penanda ditolak dan tetap terhapus saat drop → AC-12 (bagian testkit)
- [ ] `cargo build -p stackly-fake-server` lalu `cargo test -p stacklyd --test provider_conformance` → 17 test lulus, satu per satu lewat mutex statis, sekitar 70 detik → AC-9 sampai AC-12, AC-13 (bagian report), AC-1 (bagian harness)
  - `the_example_package_passes_conformance`: skenario `default` dan `slow-ready` menjalankan 16 langkah persis dalam urutan AC-9; report `stackly.provider-conformance/v1`, `kind: fixture`, `runtime.source: fixture`, `handover: unproven`, `verdict: pass`, file report sama dengan nilai di memori, tidak memuat `$HOME` atau nilai settings teks; `cleanup.rootRemoved: true`, `processesLeft: []` → AC-9, AC-13
  - `the_fake_package_passes_conformance`: skenario tersirat `default` lulus, `connection`, `reload`, dan `start_again` `pass` → AC-9
  - `a_server_that_closes_fd_3_fails_the_process_contract`, `a_server_that_re_execs_fails_the_process_contract`: gagal di `process` dengan `lock_not_held` dan `re_exec`; `a_server_that_goes_to_the_background_fails_start_and_leaves_nothing_behind`: gagal di `start`, `leftovers` `fail` dengan reason `["lock_still_held", "process_left"]`, proses anak tercatat di `cleanup.processesLeft` lalu diakhiri; ketiganya `rootRemoved: true` dan tidak ada `stackly-fake-server` di root uji yang tersisa → AC-10
  - unit test `conformance::process::tests` (empat test): dua sampel buatan dengan PID, `startedAt`, atau pemimpin group berbeda memberi `not_foreground`; fd 3 lain atau `adoptable: false` memberi `lock_not_held`; executable lain memberi `re_exec`; parser `lsof -F Di` → AC-10
  - `a_slow_ready_scenario_passes_within_the_readiness_limit` (langkah `start` ≥ 1500 ms), `a_crashing_scenario_fails_at_start_unless_the_scenario_expects_it` (`readyDelayMs: 60000`, `crashAfterMs: 300`: `start_fails` lulus dengan `leftovers` `pass` dan `health` `skipped`; `ready` gagal di `start` dengan `failure.code` terisi), `a_log_spamming_scenario_still_completes` (`logLinesPerSecond: 2000`, halaman log penuh) → AC-11
  - `an_unsupported_api_version_ends_before_any_lifecycle`: salinan paket contoh dengan `apiVersion: stackly/v2` berakhir di `validate` (`failure.step: validate`), satu finding `unsupported_api_version` yang menyebut `stackly/v1alpha1`, `scenarios: []`, `runtime: null`, tidak ada root berpenanda baru, folder paket tetap ada → AC-1 (bagian harness)
  - `a_root_without_this_runs_marker_is_never_removed`: penanda dengan `runId` run lain membuat `remove_checked` menolak dengan `the marker belongs to another run` dan kedua folder tetap → AC-12
- [ ] Sesudah run di atas: `ps -axww -o pid=,args= | awk '/\/stackly-test-[^ ]*\/Test Root / && !/awk/'` kosong, dan tidak ada folder `$TMPDIR/stackly-test-*` atau `/private/tmp/skt-*` yang memuat `.stackly-test-run` → AC-12
- [ ] `cargo test -p stackly-fake-server` → lulus tanpa perubahan; flag `--close-lock-fd`, `--re-exec`, `--background` default `false` dan hanya dipakai paket `testing/fake-rogue` (`apps/daemon/tests/conformance/packages/fake-rogue/`, `cargo xtask provider check <folder itu>` → baris `ok testing/fake-rogue 0.1.0 ...`) → AC-10
- [ ] `cargo clippy -p stacklyd -p stackly-testkit -p stackly-fake-server --all-targets --locked -- -D warnings`, `cargo xtask check-deps`, `cargo xtask evidence check` → bersih

### Acceptance-criteria coverage

- AC-9 · lengkap lewat test; `cargo xtask provider test providers/example` (exit 0 dan report di `<target dir>/provider-test/<runId>/`) menunggu langkah 13
- AC-10, AC-11 · lengkap. Tafsiran AC-11: halaman `instance.logs_read` dibatasi 64 KiB satu batch log selain `limit: 1000`, jadi baris 100 byte memenuhi halaman di sekitar 197 baris; langkah `logs` mengulang sampai halaman penuh (1000 baris, atau kurang dengan cursor `earlier` yang menandakan masih ada baris lama), bukan sampai tepat 1000 baris
- AC-12 · bagian testkit dan harness lengkap; `--cleanup` (`sweep_marked`) dan `--keep` (`keep_data`) lewat `xtask` menunggu langkah 13
- AC-13 · bagian report lengkap (schema, sanitasi `~` dan `$TMPDIR`, tulis atomik); baris terakhir stderr, exit code 0/1/2/130, `interrupted` lewat sinyal, dan folder report default menunggu langkah 13
- AC-1 · bagian harness lengkap
- AC-14 · menunggu langkah 13; AC-15 · menunggu langkah 14
