# Verify: Provider SDK & test harness · spec 0036 · updated 2026-11-11

_Langkah diturunkan dari acceptance criteria spec 0036. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27, dengan `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`. Probe yang mengubah file sementara selalu dikembalikan sesudahnya (`git status` bersih lagi)._

## Milestone 1: Validator dan kebijakan

Bagian ini mencakup Build plan langkah 1 (`core::provider::check` dan `core::provider::scenarios`), langkah 2 (`cargo xtask provider check` di atas `check_package`), langkah 3 (`provider.validate`, protokol 3.6), langkah 4 (`stackly provider validate`), dan langkah 5 (corpus kompatibilitas dan `cargo xtask provider freeze`).

### UI / manual

- [ ] Fuzz 10 menit (langkah manual user, AC-3): `cargo xtask fuzz run provider_scenarios --time 600` → baris terakhir `fuzz run: provider_scenarios ran for 600 s without a crash`, tidak ada file baru di `fuzz/artifacts/provider_scenarios/`, dan `stat::peak_rss_mb` jauh di bawah batas libFuzzer 2048 MB. Toolchain `nightly-2026-10-17` dan cargo-fuzz 0.13.2 sudah terpasang di Mac ini; smoke run 30 detik saat build lulus (137 ribu eksekusi, peak RSS 323 MB).

### Commands

- [x] `cargo test -p stackly-core --lib provider::check` → 3 test lulus: `SUPPORTED_API_VERSIONS` berisi tepat `stackly/v1alpha1` berstatus `current`; pesan `apiVersion "stackly/v2" is not supported; this Stackly supports: stackly/v1alpha1` dan `<not a string>`; tanda kutip dan karakter kontrol di nilai di escape, nilai di atas 128 karakter dipotong dengan `...` → AC-1
- [x] `cargo test -p stackly-core --lib provider::scenarios` → 6 test lulus: urutan dan default (`expect: ready`, settings kosong), `schema` lain berhenti sebelum sisa dokumen di decode, nama `InstanceName` paling banyak 24 karakter dan unik, 0 atau 17 skenario ditolak, field asing, key ganda, tag, merge key, `expect` asing, dan settings bukan mapping ditolak, skenario tersirat `default` → AC-3
- [x] `cargo test -p stackly-core --test provider_check` → 8 test lulus, semuanya lewat `FakePlatform` tanpa satu pun mutasi filesystem: paket `fake`, `nats`, `postgresql`, `traefik` `publishable` dengan skenario tersirat `default` dan key yang sama dengan `from_files`; `fake-command` `loadable` tetapi tidak `publishable` dengan satu finding `authoring` `token_not_allowed_here` yang sama dengan `init_port_issues`; `apiVersion: stackly/v2` (dengan settings schema dan template yang juga rusak) memberi tepat satu finding `package` `unsupported_api_version` di `provider.yaml` `apiVersion`, sama dengan error `from_files`; `apiVersion` bukan string dan yang hilang; paket yang tidak terbaca atau tidak valid hanya punya finding `package`; skenario valid tidak mengubah `manifest_sha256` dan tidak dibaca `read_package`; setiap aturan skenario yang dilanggar menjadi finding `scenarios` di lokasinya; file 65 KiB, symlink, FIFO, folder, `tests/` symlink, dan bukan UTF-8 ditolak → AC-1 (bagian core), AC-2, AC-3
- [x] `cargo test -p stackly-core --test provider_rules another_api_version_or_kind_stops_before_the_rest_is_read` → lulus tanpa perubahan: satu issue `unsupported_api_version` → AC-1
- [x] `cargo test -p stackly-core --test fuzz_regressions` dan `cargo test -p stackly-core --lib provider::fuzzing` → lulus; `provider_scenarios` ada di daftar target dan dua seed nya (`fixture.fake.yaml`, `fixture.invalid.yaml`) melewati harness → AC-3
- [x] `cargo xtask fuzz list` → memuat `provider_scenarios`; `cargo xtask fuzz build provider_scenarios` → exit 0 dengan toolchain pin; `cargo xtask fuzz seed` lalu `git status --short fuzz/seeds` → bersih (seed `provider_scenarios` sudah ter-commit; seed dari `providers/example/tests/scenarios.yaml` dan `providers/fake/tests/scenarios.yaml` ikut otomatis begitu file itu ada) → AC-3
- [x] `cargo xtask provider check` → empat baris `ok` (`testing/fake`, `official/nats`, `official/postgresql`, `official/traefik`), exit 0 → AC-4
- [x] `cargo test -p xtask --test provider_check` → 7 test lulus: format baris lama tetap untuk error `package` dan `authoring` (`init_port_issues`); `apiVersion: stackly/v2` mencetak tepat satu baris `fake: provider.yaml apiVersion: unsupported_api_version: apiVersion "stackly/v2" is not supported; this Stackly supports: stackly/v1alpha1`; `tests/scenarios.yaml` yang rusak mencetak baris `fake: tests/scenarios.yaml scenarios[0].settings.readyDelayMs: settings_value: ...` tanpa baris `ok`; exit 1 → AC-4, AC-1 (bagian xtask). Baris `warning: ` belum bisa dipicu karena belum ada versi atau item `deprecated`; tanpa argumen folder, warning dihitung sebagai masalah (`check_packages(.., warnings_fail = true)`)
- [x] `cargo test -p stackly-protocol --lib provider` dan `cargo test -p stackly-protocol --test provider_fixtures` → lulus: path 1 sampai 4096 byte, diawali `/`, tanpa NUL, selain itu `invalid_format` di `path`; ketiga enum terbuka membaca nilai asing sebagai `unrecognized`; setiap file di `protocols/fixtures/provider/` terdaftar di `index.json`, fixture `roundtrip` sama persis dengan bentuk tulisan Rust, `omitted` = `findingCount` dikurangi panjang `findings`; frame request, reject path relatif, dan response di `protocols/fixtures/ipc/` → AC-5
- [x] `cargo test -p stackly-protocol` → lulus, termasuk `ipc_fixtures` (setiap varian `Call` dan `Reply` punya fixture roundtrip), `limits` (`providerPathMax`, `validationFindingsMax`), recovery allowed untuk `provider.validate`, dan `ProtocolVersion::CURRENT` 3.6 → AC-5
- [x] `cargo xtask codegen --check` → "20 generated files are up to date"; `protocols/contract-versions.json` mencatat 3.6 → AC-5
- [x] `cargo test -p stacklyd --test provider_validate` → 6 test lulus: salinan `providers/nats` `publishable` dengan identitas dari `ValidatedPackage::key()`, generation registry dan isi root sama sebelum dan sesudah; `apiVersion: stackly/v2` dan `fake-command` menghasilkan persis fixture `validation-unsupported-api-version.json` dan `validation-authoring.json`; folder yang tidak ada dan folder dengan 150 entri asing tetap balasan sukses (`loadable: false`, 100 finding, `findingCount` 101, `omitted` 1); path relatif, kosong, dan ber-NUL ditolak `request.invalid_params` di `path`; method dijawab dalam recovery mode tanpa menyentuh registry; entri catalog yang `provider.yaml` nya `stackly/v2` berstatus `invalid` dengan tepat satu issue `unsupported_api_version` → AC-5 (bagian daemon), AC-1 (bagian method dan catalog)
- [x] `cargo test -p stacklyd --lib ipc::provider` → 150 finding dipotong ke 100 dengan `omitted` 50 → AC-5
- [x] `swift test --package-path clients/swift` → lulus, termasuk `ProviderValidateContractTests` (5 test: request, frame balasan, tiga fixture, nilai asing `.unrecognized`), `IpcFixtureTests` (62 method), dan `ContractTests` (protokol 3.6) → AC-5
- [x] `cargo xtask fuzz seed` lalu `git status --short fuzz/seeds` → bersih; seed `ipc_request_decode` dan `ipc_server_frame_decode` memuat ketiga frame `provider.validate` → AC-5
- [x] `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` dan `cargo xtask check-deps` → lulus; `check` dan `scenarios` hanya memakai port `Filesystem` → AC-2
- [x] `cargo test -p stackly-cli --test provider validate` → 3 test lulus terhadap `stacklyd` sungguhan: `providers/nats` mencetak `ok official/nats <revision> <sha256>` lalu `0 error(s), 0 warning(s)`, exit 0, dan `--json` berisi `ProviderValidation` di `result`; salinan `stackly/v2` yang diberikan sebagai path relatif `./pkgs/../pkgs/./mytool` dari folder kerja lain mencetak tepat satu baris `mytool: provider.yaml apiVersion: unsupported_api_version: ...` dan `1 error(s), 0 warning(s)`, exit 2 dengan `cli.provider_invalid` (kategori `invalid_input`, detail `errors` 1 dan `warnings` 0) dan `result` tetap terisi; `fake-command` mencetak baris `token_not_allowed_here` tanpa baris `ok`, dan folder yang tidak ada memberi finding `missing_file`, keduanya exit 2 → AC-5 (bagian CLI), AC-1
- [x] `cargo test -p stackly-cli --bin stackly` → lulus, termasuk normalisasi path leksikal (`.` dibuang, `..` melipat komponen sebelumnya, tanpa menyentuh filesystem) dan satu konstruktor per kode `cli.*` (`cli.provider_invalid` ada) → AC-5
- [x] `cargo test -p stackly-protocol --lib codes` → tabel kode `cli` spec 0008 memuat `cli.provider_invalid` (`invalid_input`, detail `errors`, `warnings`); `cargo xtask codegen --check` lulus dan `protocols/error-codes.json` memuatnya → AC-5
- [x] `cargo test -p stackly-core --test provider_compat` → 3 test lulus: setiap entri `crates/core/tests/fixtures/compat/<nama>/<revision>/` (kini `nats/1.0.1` dan `postgresql/1.0.2`) termuat dengan `NameCheck::Skip` dari salinan di test root, `manifest_sha256` sama dengan `frozen.json`, dan `frozen_facts` (argv start, nama key env start, SHA-256 config yang dirender dalam *Context beku*) sama persis dengan `frozen.json`; setiap entri hanya berisi `package/` dan `frozen.json` → AC-6
- [x] `cargo xtask provider freeze providers/nats` → `already frozen <repo>/crates/core/tests/fixtures/compat/nats/1.0.1`, exit 0, tanpa perubahan `git status`; `cargo xtask provider freeze providers/fake` → exit 1 `only official/ packages are frozen` → AC-6
- [x] `cargo test -p xtask --test provider_check` → 9 test lulus, termasuk freeze ulang yang tidak mengubah apa pun, `testing/` yang ditolak, dan salinan `nats` dengan byte lain di revision sama yang ditolak dengan `raise metadata.revision` → AC-6
- [x] `cargo test -p xtask freeze` dan `cargo test -p xtask not_frozen` → lulus: entri baru berisi byte paket dan `frozen.json`, freeze kedua `already frozen`, digest lain di revision sama ditolak tanpa menulis dan tanpa folder `.partial` tersisa, `testing/` dan paket yang tidak `publishable` ditolak tanpa menulis; `[[provider]]` tanpa entri corpus memberi tepat satu baris `nats: not frozen: run cargo xtask provider freeze providers/nats` dan hilang sesudah freeze → AC-4, AC-6
- [x] Probe: ubah satu karakter di `providers/nats/templates/nats.conf.tmpl` tanpa menaikkan revision, jalankan `cargo xtask provider check` → baris `nats: not frozen: run cargo xtask provider freeze providers/nats`, exit 1; kembalikan → exit 0 lagi → AC-4

### Hasil /check verify 2026-11-11 (Milestone 1, commit 3ecd7bb9)

**Verdict: PASS**, dengan satu langkah manual yang belum jalan (fuzz 10 menit, `NOT_RUN`, owner user). Semua 24 langkah Commands di atas dijalankan di worktree checker dengan `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0` dan lulus. Tidak ada temuan FAIL.

| AC | Verdict | Bukti |
| --- | --- | --- |
| AC-1 (core, xtask, method, catalog) | terpenuhi | `provider::check` 3 test, `provider_check` core 8 test lewat `FakePlatform` tanpa mutasi, `provider_rules` 1 test, `provider_check` xtask 9 test, `provider_validate` daemon 6 test (termasuk entri catalog `invalid` dengan satu issue). Probe `stacklyd` sungguhan di bawah. Bagian harness menunggu langkah 12 |
| AC-2 | terpenuhi | `provider_check` core: `fake-command` `loadable` tetapi tidak `publishable`; `cargo check --target aarch64-pc-windows-msvc` dan `cargo xtask check-deps` lulus (13 crate) |
| AC-3 | terpenuhi kecuali fuzz 10 menit | `provider::scenarios` 6 test, `fuzz_regressions` 5 test, `provider::fuzzing` 3 test, `cargo xtask fuzz build provider_scenarios` exit 0, `fuzz seed` lalu `git status` bersih. Fuzz 10 menit `NOT_RUN`, owner user |
| AC-4 | terpenuhi | `cargo xtask provider check` exit 0; probe `not frozen` di bawah |
| AC-5 | terpenuhi | `stackly-protocol` seluruhnya lulus (protokol 3.6, digest `9360304e…`), `codegen --check` 20 file, `provider_validate` daemon 6 test, `ipc::provider` 1 test, `stackly-cli --test provider validate` 3 test, `--bin stackly` 298 test, `swift test` `clients/swift` 273 test (`ProviderValidateContractTests`, `IpcFixtureTests`, `ContractTests` lulus), `protocols/error-codes.json` memuat `cli.provider_invalid` |
| AC-6 | terpenuhi | `provider_compat` 3 test; corpus berisi `nats/1.0.1` (`8e306092…`) dan `postgresql/1.0.2` (`389b77ee…`), sama dengan digest baris `ok`; `diff -r` paket hidup dan `package/` beku hanya berbeda di `README.md` dan `LICENSE-NOTICES.md`, yang memang tidak dibaca |

**Probe terhadap `stacklyd` sungguhan** (binary hasil build, `STACKLY_ROOT=$(mktemp -d)`, daemon dihentikan dengan SIGTERM dan root dihapus sesudahnya; Application Support asli tidak disentuh):

- `stackly provider validate <repo>/providers/example` → `ok testing/example 0.1.0 287cdc78…`, lalu `0 error(s), 0 warning(s)`, exit 0. Path relatif `./mytool/../example` dari folder kerja lain memberi hasil yang sama.
- Salinan paket contoh dengan `apiVersion: stackly/v2` di folder `mytool`, diberikan sebagai `pk/./mytool` → tepat satu baris `mytool: provider.yaml apiVersion: unsupported_api_version: apiVersion "stackly/v2" is not supported; this Stackly supports: stackly/v1alpha1`, lalu `1 error(s), 0 warning(s)`, exit 2 dengan `cli.provider_invalid`. `--json` berisi `result` terisi (`loadable: false`, `apiVersion: "stackly/v2"`, satu finding) dan `error` berkategori `invalid_input` dengan detail `errors` 1 dan `warnings` 0, juga exit 2.
- `apiVersion: 7` → `<not a string>`; nilai 300 karakter dipotong di 128 dengan `...`; `apiVersion` dihapus → `missing_field`.
- Folder yang tidak ada → `missing_file`, exit 2. Folder dengan 150 entri asing → 100 baris, `1 more finding(s) not shown`, `101 error(s)`, exit 2.
- `tests/scenarios.yaml` dengan key secret `authToken`, `readyDelayMs: 70000`, dan key asing → tiga finding `scenarios` (`secret_not_allowed_here`, `settings_value`, `unknown_setting`) di `scenarios[<i>].settings.<key>`; nilai secret dan nilai teks `greeting` tidak muncul di output teks maupun `--json`. `schema` lain → satu finding `invalid_value`. File 66 KiB → `file_too_large`. Symlink → `symlink_not_allowed`.
- Hash isi root data dan root runtime (semua file kecuali log dan file WAL) sama sebelum dan sesudah semua panggilan.
- `cargo xtask provider check` pada dua salinan yang sama mencetak baris yang sama dengan CLI, exit 1.

**Probe `not frozen`:** satu baris ditambah ke `providers/nats/templates/nats.conf.tmpl` → `cargo xtask provider check` mencetak `nats: not frozen: run cargo xtask provider freeze providers/nats`, exit 1; dengan argumen folder (`provider check providers/nats`) tidak ada baris itu, exit 0; `cargo xtask provider freeze providers/nats` menolak dengan `raise metadata.revision` tanpa menulis (tidak ada folder `.partial`). Sesudah `git checkout` exit 0 lagi dan `git status` bersih.

**Tafsiran developer yang dinilai** (semua diterima):

- `validate()` berhenti sesudah `unsupported_api_version`, juga di jalur `from_files` dan catalog: sesuai AC-1 ("tepat satu finding", "tanpa mendecode sisa dokumen", "di setiap jalur muat"). Lihat temuan rendah 2 soal kalimat spec yang berseberangan.
- Pesan di escape dan dipotong 128 karakter: untuk nilai biasa pesannya persis teks AC-1. Lihat temuan rendah 3.
- `api_version` `None` untuk nilai bukan string: sesuai tipe wire `string, opsional`.
- Settings skenario: key secret ditolak dan field secret diberi referensi pengganti saat validasi: sesuai *File skenario* ("divalidasi sesudah key secret diperiksa") dan aturan bahwa harness memakai `SecretSource::Generate`.
- `schema` lain berhenti sebelum sisa file: sesuai, satu finding `invalid_value`.
- Semua kesalahan `path` berkode `invalid_format`, dan pemotongan 100 finding: persis *Kontrak wire*.
- `provider check` tanpa argumen menghitung warning sebagai masalah: persis *Kebijakan kompatibilitas*.
- Context beku memakai versi dan line artefak pertama `versions.json`: spec tidak menyebut versi runtime, dan `versions.json` ikut beku di `package/`, jadi hasilnya stabil.
- `not frozen` hanya tanpa argumen folder dan hanya paket `[[provider]]`: persis AC-4. Corpus `nats 1.0.1` dan `postgresql 1.0.2`: persis AC-6, karena `traefik` masih `[[planned]]`.
- `xtask` menyalakan fitur `test-support` core: tercantum di *Configuration required*, dan `check-deps` lulus.

**Temuan rendah** (tidak menggagalkan AC; untuk lead dan penulis):

1. Status `deprecated` belum menghasilkan finding `warning`. Tabel *Versi dan status* menyebut `check_package` menambah satu `warning` per paket yang memakai versi `deprecated`, tetapi `crates/core/src/provider/check.rs` tidak punya jalur itu (`Severity::Warning` tidak pernah dibuat). Hari ini tidak bisa terpicu karena tabel hanya berisi `current`. Saran: bangun bersama versi `deprecated` pertama, atau catat di spec bahwa itu ditunda.
2. Kalimat "`from_files` tidak berubah" di AC-2, *Batas dengan spec lain*, dan *Key invariants* tidak lagi tepat: `from_files` kini berhenti sesudah `unsupported_api_version` dan tidak lagi melaporkan issue file lain. Paket itu tetap ditolak seperti sebelumnya dan paket yang valid tidak terpengaruh, jadi maksud invariant (tidak ada pengetatan untuk salinan pinned) tetap terjaga. Saran: rapikan kalimat spec lewat `/architect`.
3. Escape tanda kutip dan karakter kontrol serta batas 128 karakter di pesan `unsupported_api_version` belum tertulis di AC-1. Saran: tambahkan satu kalimat di spec.
4. `stackly provider validate` mencetak satu baris tambahan `<n> more finding(s) not shown` saat `omitted` lebih dari 0. Bagian *CLI* hanya menyebut baris AC-4 dan baris ringkasan. Berguna, tetapi belum tertulis.
5. `cargo xtask provider freeze` tidak memeriksa apakah paket tercantum sebagai `[[provider]]`. Menjalankannya pada `providers/traefik` (masih `[[planned]]`) akan menulis entri corpus yang tidak bisa dihapus lagi. Tidak dijalankan di verify ini. AC-6 tidak menuntut penolakan, jadi ini catatan saja.
6. Teks dua langkah di atas sudah tertinggal dari kode: `cargo xtask provider check` kini mencetak lima baris `ok` (`testing/example` pertama), bukan empat; dan `cargo test -p xtask --test provider_check` kini 9 test, bukan 7. Keduanya lulus.

**Belum dijalankan:** fuzz 10 menit (`cargo xtask fuzz run provider_scenarios --time 600`), `NOT_RUN`, owner user.

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

- [x] `cargo xtask provider check providers/example` → `ok testing/example 0.1.0 <sha256>` tanpa finding → AC-7 (bagian paket)
- [x] `cargo test -p stackly-core --test provider_packages` → lulus, termasuk `the_example_package_reads_without_writes_and_validates` (ID `testing/example`, revision `0.1.0`, software `fake`, capability `reload`, `multiInstance`, `persistence`, field `greeting`, `readyDelayMs`, `authToken` secret), `the_example_package_is_publishable_with_its_two_scenarios` (`check_package` tanpa finding, skenario `default` dengan settings kosong dan `slow-ready` dengan `readyDelayMs: 1500`, keduanya `ready`), `the_example_package_has_the_artifacts_of_the_fake_package` (artefak `versions.json` sama dengan milik `providers/fake`, yang versinya dijaga `provider_no_shell` terhadap crate `stackly-fake-server`), dan `example_plans_match_the_golden_files` (golden `crates/core/tests/golden/provider/example/`: init tanpa render, run me-render `serverConfig` sebelum spawn) → AC-7 (bagian paket)
- [x] `cargo xtask fuzz seed` lalu `git status --short fuzz/seeds` → bersih; `fuzz/seeds/provider_scenarios/example.scenarios.yaml` ter-commit → AC-3
- [x] `cargo xtask docs check` → `ok docs`; `providers/example/README.md` ikut diperiksa: blok `bash` nya memuat `cargo xtask provider new mytool` dan `cargo xtask provider check providers/mytool` (keduanya ada di `USAGE`), sedangkan `docs/providers/README.md` dan perintah `test` dan `freeze` tetap code span (bukan link atau blok `bash`) sampai halaman dan perintah itu ada → AC-7
- [x] `cargo test -p xtask scaffold` → 9 test lulus: nama yang melanggar aturan bagian nama `ProviderName` (kosong, huruf besar, angka di awal, `--`, `-` di akhir, `/`, `_`, 64 karakter) dan `example` ditolak dengan pesan aturannya; huruf pertama dikapitalkan; penggantian hanya untuk baris utuh yang muncul tepat sekali; argumen `<name> [--out <dir>]`; hasil scaffold `mytool` berbeda dari paket contoh hanya pada tiga baris (`# Mytool provider`, `  id: testing/mytool`, `  displayName: Mytool`), file yang sama, folder sementara hilang, dan baris `ok testing/mytool 0.1.0 <sha256>`; folder tujuan yang sudah ada, nama tidak valid, contoh yang drift (`0 times`), dan salinan yang tidak `publishable` tidak meninggalkan file apa pun → AC-7
- [x] Jalankan `cargo xtask provider new mytool` → exit 0, baris `ok testing/mytool 0.1.0 <sha256>` lalu `next:` dengan `provider check`, `provider test`, README, dan `STACKLY_UPDATE_GOLDEN=1 cargo test -p stacklyd --test create_form_fixtures`; `cargo xtask provider check providers/mytool` → baris `ok` yang sama; ulangi `provider new mytool` → exit 1 `... already exists; nothing was written`; `cargo xtask provider new My` → exit 1 dengan aturan nama; `cargo xtask provider new other --out <folder sementara>` → exit 0 tanpa baris `create_form_fixtures`. Hapus `providers/mytool` dan folder sementara (`git status` bersih) → AC-7
- [x] `cargo test -p xtask` (seluruhnya) → lulus, termasuk `the_provider_words_cover_every_provider_folder_and_catalog_id` (`example` ada di `PROVIDER_WORDS`) dan `the_repo_packages_pass_with_one_ok_line_each` (lima baris `ok`, `testing/example` pertama); `cargo xtask check-deps` → bersih (`instance::create` dan folder Swift `CreateInstance/` tidak memuat kata `example`)
- [x] `cargo xtask catalog check` → revision catalog tidak berubah (namespace `testing` tidak masuk catalog); `cargo xtask codegen --check` → up to date
- [x] `cargo test -p stacklyd --test create_form_fixtures` → lulus; satu fixture `protocols/fixtures/instance/create-form-<folder>.json` untuk setiap folder `providers/` (`example`, `fake`, `nats`, `postgresql`, `traefik`) dari listing folder; nama tampil dari `metadata.displayName`, versi dan line dari artefak pertama `versions.json` untuk target host → AC-8
- [x] Probe drift: ubah `"default": "hello"` di `providers/example/schemas/settings.schema.json`, jalankan test di atas → gagal `example: rerun with STACKLY_UPDATE_GOLDEN=1`; kembalikan. Hapus sementara `create-form-traefik.json` → gagal dengan path file itu; kembalikan (`git status` bersih) → AC-8
- [x] `cargo test -p stackly-protocol --test create_fixtures` dan `swift test --package-path clients/swift --filter 'CreateContractTests|CreateClientTests'` → lulus pada fixture yang ditulis ulang → AC-8
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter 'EveryCreateFormFixtureTests|SchemaFormModelTests|CreateInstanceCopyTests|CreateInstanceModelTests'` → lulus; `EveryCreateFormFixtureTests` membaca setiap `create-form-*.json` (paling sedikit lima), membangun `SchemaFormModel`, dan gagal bila ada field ber kind `.unrecognized` atau baris ber control `.unrecognized` → AC-8

### Hasil /check verify 2026-11-11 (Milestone 2, commit 3ecd7bb9)

**Verdict: PASS.** Semua 12 langkah Commands di atas dijalankan dan lulus. Tidak ada temuan FAIL.

| AC | Verdict | Bukti |
| --- | --- | --- |
| AC-7 (paket contoh dan scaffold) | terpenuhi, dengan satu bagian README yang menunggu spec 0038 | `cargo xtask provider check providers/example` → `ok testing/example 0.1.0 287cdc78…`; `provider_packages` 39 test (empat test paket contoh lulus); `versions.json` sama byte demi byte dengan milik `providers/fake`; `cargo test -p xtask scaffold` 9 test; probe scaffold di bawah. Scaffold yang lulus `provider test` menunggu langkah 13 |
| AC-8 | terpenuhi | `create_form_fixtures` 1 test atas lima folder dari listing `providers/`; probe drift di bawah; `create_fixtures` 16 test; `swift test` `clients/swift` lulus (`CreateContractTests`, `CreateClientTests`); `StacklyUI` 75 test di 4 suite, `EveryCreateFormFixtureTests` membaca folder fixture dan menolak kind atau control `.unrecognized` |
| AC-3 (seed) | terpenuhi | `fuzz/seeds/provider_scenarios/example.scenarios.yaml` ter-commit; `fuzz seed` tidak mengubah apa pun |

**Probe scaffold:** `cargo xtask provider new mytool` → exit 0, `ok testing/mytool 0.1.0 3823b7c6…`, lalu `next:` dengan `provider check`, `provider test`, README, dan `STACKLY_UPDATE_GOLDEN=1 cargo test -p stacklyd --test create_form_fixtures`. `cargo xtask provider check providers/mytool` mencetak baris `ok` yang sama. `diff -r providers/example providers/mytool` hanya menunjukkan tiga baris (`# Mytool provider`, `  id: testing/mytool`, `  displayName: Mytool`). Ulang → exit 1 `... already exists; nothing was written`. `provider new My` → exit 1 dengan aturan nama. `provider new example` → exit 1 `the example package already has this name`. `provider new other --out <scratchpad>` → exit 0, tanpa baris `create_form_fixtures`, dan folder `--out` hanya berisi `other` (folder sementara hilang). `providers/mytool` dan folder scratchpad dihapus, `git status` bersih.

**Probe drift form:** `"default": "hello"` diubah di `providers/example/schemas/settings.schema.json` → test gagal `example: rerun with STACKLY_UPDATE_GOLDEN=1`. `create-form-traefik.json` dipindah sementara → test gagal dengan path file itu. Sesudah dikembalikan test lulus dan `git status` bersih.

**Lain lain:** `cargo xtask docs check` → `ok docs 17`; `cargo xtask catalog check` → `ok catalog revision 11` (tidak berubah); `cargo test -p xtask` seluruhnya lulus (511 unit test, plus test integrasi; test native tetap `ignored`).

**Tafsiran developer yang dinilai** (semua diterima):

- README paket contoh memakai code span untuk halaman dan perintah yang belum ada: benar untuk saat ini, karena `cargo xtask docs check` menolak link rusak dan perintah yang tidak ada di `USAGE`. Lihat temuan rendah 1.
- Fixture form `fake`, `nats`, dan `postgresql` ditulis ulang: diminta AC-8.
- `example` ditambahkan ke `PROVIDER_WORDS`: menjaga kode generik tidak menyebut paket itu; `check-deps` lulus.
- Scaffold lewat folder sementara lalu rename: persis *Scaffold* langkah 3 dan 4.

**Temuan rendah:**

1. *Paket contoh* meminta README "dengan link ke `docs/providers/`". Saat ini `docs/providers/README.md` ditulis sebagai code span, karena halaman itu baru ada di langkah 9 spec 0038. Bagian AC-7 ini belum terpenuhi sampai link itu dipasang. Owner: penulis langkah 9 spec 0038 atau `dev-f36-m3`, sesudah halaman ada. Catatan: `provider freeze` sudah ada di `USAGE`, jadi boleh pindah ke blok `bash`; `provider test` menunggu langkah 13.
2. `provider new` mencetak `cargo xtask provider test <dir>` sebagai langkah berikutnya, padahal perintah itu baru ada di langkah 13. Sesuai teks *Scaffold*, jadi bukan cacat; cukup diingat sampai langkah 13 masuk.

### Acceptance-criteria coverage

- AC-7 · lengkap (langkah 6 dan 7); scaffold yang lulus `provider test` menunggu harness langkah 10 sampai 13
- AC-8 · lengkap (langkah 8)
- AC-3 · seed `example.scenarios.yaml` ada; fuzz 10 menit tetap langkah manual Milestone 1
- AC-9 (paket contoh lulus harness) · menunggu langkah 10

## Milestone 3: Harness konformitas

Bagian ini mencakup Build plan langkah 9 sampai 13. Langkah 13 menambah example `provider_harness`, `cargo xtask provider test` dengan `--cleanup` dan `--keep`, dan test `xtask` scaffold ke harness.

### UI / manual

- Tidak ada langkah manual untuk langkah 9 sampai 12. Semua run memakai artefak `fixture` (`stackly-fake-server`), tanpa unduhan dan tanpa Keychain.

### Commands

- [x] `cargo test -p stackly-testkit --lib test_root` → 14 test lulus: `TestRoot::new_marked` menulis `.stackly-test-run` (`stackly.test-run/v1`, `runId`, `uid`, `pid`, `createdAt`) di kedua folder dan tidak terhapus saat drop; `remove_checked` menghapus kedua folder run nya sendiri; penanda run lain, penanda hilang, `uid` lain, schema lain, symlink, folder bersarang, dan nama tanpa awalan ditolak dengan reason; folder yang argumennya masih dimuat sebuah proses ditolak sampai proses itu berakhir; `keep_data` menyimpan folder data dan menghapus folder run; `sweep_marked` menghapus root yang `pid` nya mati dan membiarkan yang hidup; root tanpa penanda ditolak dan tetap terhapus saat drop → AC-12 (bagian testkit)
- [x] `cargo build -p stackly-fake-server` lalu `cargo test -p stacklyd --test provider_conformance` → 17 test lulus, satu per satu lewat mutex statis, sekitar 70 detik → AC-9 sampai AC-12, AC-13 (bagian report), AC-1 (bagian harness)
  - `the_example_package_passes_conformance`: skenario `default` dan `slow-ready` menjalankan 16 langkah persis dalam urutan AC-9; report `stackly.provider-conformance/v1`, `kind: fixture`, `runtime.source: fixture`, `handover: unproven`, `verdict: pass`, file report sama dengan nilai di memori, tidak memuat `$HOME` atau nilai settings teks; `cleanup.rootRemoved: true`, `processesLeft: []` → AC-9, AC-13
  - `the_fake_package_passes_conformance`: skenario tersirat `default` lulus, `connection`, `reload`, dan `start_again` `pass` → AC-9
  - `a_server_that_closes_fd_3_fails_the_process_contract`, `a_server_that_re_execs_fails_the_process_contract`: gagal di `process` dengan `lock_not_held` dan `re_exec`; `a_server_that_goes_to_the_background_fails_start_and_leaves_nothing_behind`: gagal di `start`, `leftovers` `fail` dengan reason `["lock_still_held", "process_left"]`, proses anak tercatat di `cleanup.processesLeft` lalu diakhiri; ketiganya `rootRemoved: true` dan tidak ada `stackly-fake-server` di root uji yang tersisa → AC-10
  - unit test `conformance::process::tests` (empat test): dua sampel buatan dengan PID, `startedAt`, atau pemimpin group berbeda memberi `not_foreground`; fd 3 lain atau `adoptable: false` memberi `lock_not_held`; executable lain memberi `re_exec`; parser `lsof -F Di` → AC-10
  - `a_slow_ready_scenario_passes_within_the_readiness_limit` (langkah `start` ≥ 1500 ms), `a_crashing_scenario_fails_at_start_unless_the_scenario_expects_it` (`readyDelayMs: 60000`, `crashAfterMs: 300`: `start_fails` lulus dengan `leftovers` `pass` dan `health` `skipped`; `ready` gagal di `start` dengan `failure.code` terisi), `a_log_spamming_scenario_still_completes` (`logLinesPerSecond: 2000`, halaman log penuh) → AC-11
  - `an_unsupported_api_version_ends_before_any_lifecycle`: salinan paket contoh dengan `apiVersion: stackly/v2` berakhir di `validate` (`failure.step: validate`), satu finding `unsupported_api_version` yang menyebut `stackly/v1alpha1`, `scenarios: []`, `runtime: null`, tidak ada root berpenanda baru, folder paket tetap ada → AC-1 (bagian harness)
  - `a_root_without_this_runs_marker_is_never_removed`: penanda dengan `runId` run lain membuat `remove_checked` menolak dengan `the marker belongs to another run` dan kedua folder tetap → AC-12
- [x] Sesudah run di atas: `ps -axww -o pid=,args= | awk '/\/stackly-test-[^ ]*\/Test Root / && !/awk/'` kosong, dan tidak ada folder `$TMPDIR/stackly-test-*` atau `/private/tmp/skt-*` yang memuat `.stackly-test-run` → AC-12
- [x] `cargo test -p stackly-fake-server` → lulus tanpa perubahan; flag `--close-lock-fd`, `--re-exec`, `--background` default `false` dan hanya dipakai paket `testing/fake-rogue` (`apps/daemon/tests/conformance/packages/fake-rogue/`, `cargo xtask provider check <folder itu>` → baris `ok testing/fake-rogue 0.1.0 ...`) → AC-10
- [x] `cargo clippy -p stacklyd -p stackly-testkit -p stackly-fake-server --all-targets --locked -- -D warnings`, `cargo xtask check-deps`, `cargo xtask evidence check` → bersih

- [x] Langkah 13, dari checkout tanpa file untracked: `cargo xtask provider check providers/example` lalu `cargo xtask provider test providers/example` → baris `ok`, stderr berakhir `provider test report: <repo>/target/provider-test/<runId>/report.json`, exit 0, report `verdict: pass` dengan `runId` sama dengan nama folder nya; `git status --porcelain` sama sebelum dan sesudah → AC-9, AC-13
- [x] `cargo xtask provider test apps/daemon/tests/conformance/packages/fake-rogue --report <tmp>/r.json` → report di `<tmp>/r.json`, baris terakhir stderr menyebutnya → AC-13
- [x] Interupsi: jalankan `cargo xtask provider test providers/example --report <tmp>/i.json`, tunggu sampai proses `provider_harness` muncul (`ps -axo pid=,comm= | awk '$2 ~ /provider_harness$/'`), kirim satu `kill -INT <pid>` → stderr `provider_harness: stopping after the current step`, exit 130, report `interrupted: true`, `verdict: fail`, `cleanup.rootRemoved: true` → AC-13
- [x] `cargo xtask provider test --cleanup` → exit 0, baris terakhir stderr `provider test --cleanup: removed <n> folder(s)`; hanya root berpenanda milik uid ini yang `pid` nya mati yang terhapus (diuji `sweep_removes_a_root_whose_run_is_gone_and_keeps_one_whose_run_lives`) → AC-12
- [x] `cargo test -p xtask --test provider_test` → 2 test lulus: `a_scaffold_outside_the_repo_passes_check_and_test_without_changing_the_repo` (`provider new mytool --out <TestRoot>`, `provider check`, `provider test --report`, report `pass`, baris terakhir stderr, `git status --porcelain` repo sama) dan `cleanup_runs_alone_and_exits_0` → AC-14, AC-12
- [x] `cargo test -p stacklyd --example provider_harness` (unit test argumen) dan `cargo test -p xtask provider::harness` → lulus
- [x] `cargo xtask docs check` → `ok docs`; blok `bash` README paket contoh memuat `provider new`, `provider check`, dan `provider test` → AC-7

### Hasil /check verify 2026-11-11 (Milestone 3, commit 5343f0c7)

**Verdict: FAIL pada satu butir** (batas waktu langkah, temuan 1). Semua AC lain di milestone ini terpenuhi dengan bukti. Ke 12 langkah Commands dijalankan dengan `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0` dan lulus. Diverifikasi pada `5343f0c7`; `main` sesudah itu menambah fitur 37, yang tidak dibaca dan tidak diuji di sini.

| AC | Verdict | Bukti |
| --- | --- | --- |
| AC-9 | terpenuhi, kecuali batas waktu langkah (temuan 1) | `provider_conformance` 17 test lulus (74 detik); `cargo xtask provider test providers/example` dan `providers/fake` exit 0, `verdict: pass`, 16 langkah per skenario dalam urutan AC-9 |
| AC-10 | terpenuhi | tiga test rogue dan empat unit test `conformance::process`; `provider check` paket `fake-rogue` → `ok testing/fake-rogue 0.1.0 b564c4ad…` |
| AC-11 | terpenuhi (dengan bunyi "satu halaman penuh: 1000 baris atau batas 64 KiB" yang sudah diputuskan) | tiga test fault lulus |
| AC-12 | terpenuhi | `test_root` 14 test; probe `--keep` dan `--cleanup` di bawah; tidak ada root berpenanda atau proses dari run ini yang tersisa |
| AC-13 | terpenuhi | probe report, exit code, baris terakhir stderr, dan interupsi di bawah |
| AC-14 | terpenuhi | `cargo test -p xtask --test provider_test` 2 test (45 detik) |
| AC-1 (harness) | terpenuhi | test `an_unsupported_api_version_ends_before_any_lifecycle` dan probe di bawah |
| AC-7 (scaffold lulus `provider test`) | terpenuhi | probe `provider new probetool` lalu `provider test` di bawah; `docs check` → `ok docs 20`, blok `bash` README memuat `new`, `check`, `test` |

Perintah lain: `cargo test -p stackly-fake-server` (19 dan 36 test), `cargo clippy -p stacklyd -p stackly-testkit -p stackly-fake-server --all-targets --locked -- -D warnings`, `cargo xtask check-deps` (13 crate), `cargo xtask evidence check` (63 record manual, baseline sah), `cargo test -p stacklyd --example provider_harness` (8 test), `cargo test -p xtask provider::harness` (1 test), `cargo fmt --all --check`: semua bersih.

**Probe** (semua dengan artefak `fixture`; sisa diperiksa per `runId` karena sesi lain menjalankan test di Mac yang sama; proses `stackly-fake-server` PID 39307 milik sesi lain tidak disentuh dan tidak dihitung):

- `cargo xtask provider test providers/example` → exit 0; stderr berakhir `default: pass`, `slow-ready: pass`, `verdict: pass`, `provider test report: <repo>/target/provider-test/01a1c989-…/report.json`; `runId` report sama dengan nama folder; `kind: fixture`, `handover: unproven`, `cleanup.rootRemoved: true`, `processesLeft: []`; report tidak memuat `/Users/` atau teks `hello`; `sourceCommit` terisi, `dirty: false`; `git status --porcelain` sama sebelum dan sesudah. `providers/fake` → exit 0, `verdict: pass`, `handover: proven`.
- `cargo xtask provider new probetool` → `ok testing/probetool 0.1.0 80c6c4d0…`; `cargo xtask provider test providers/probetool --report <scratchpad>/probetool.json` → exit 0, dua skenario `pass`, report di path itu. Folder dihapus, `git status` bersih.
- Paket `fake-rogue` dengan `--report <scratchpad>/r.json` → exit 0, report di path itu, baris terakhir stderr menyebutnya.
- `--keep` → exit 0, stderr `kept $TMPDIR/stackly-test-Hsiqn5`, folder data tetap ada dengan penanda `runId` run itu, folder run `/private/tmp/skt-*` hilang, `cleanup.kept` terisi dan `rootRemoved: false`. Lalu `cargo xtask provider test --cleanup` → exit 0, stdout `removed .../stackly-test-Hsiqn5`, stderr `provider test --cleanup: removed 1 folder(s)`. Tiga folder palsu di sampingnya tidak tersentuh: `$TMPDIR/stackly-test-chkfake` dan `/private/tmp/skt-chkfake` tanpa penanda, dan `$TMPDIR/stackly-test-chklive` dengan penanda ber `pid` hidup. Ketiganya lalu dihapus tangan.
- Interupsi: `kill -INT` ke proses `provider_harness` di tengah langkah `second_instance` → stderr `provider_harness: stopping after the current step`, exit 130 dalam satu detik, report `interrupted: true`, `verdict: fail`, `failure.reason: interrupted`, `cleanup.rootRemoved: true`; tidak ada root ber `runId` itu dan tidak ada proses dari run itu yang tersisa.
- Salinan paket contoh dengan `apiVersion: stackly/v2` → exit 1, `validate: fail`, satu finding `unsupported_api_version`, `scenarios: []`, `runtime: null`, tidak ada root berpenanda baru, folder paket tetap ada.
- `cargo xtask provider test providers/postgresql` tanpa `--line` → exit 2, `blocked: the package has more than one runnable line; pick one of 18, 17`. Salinan `nats` dengan `sha256` artefak yang tidak ada di cache → exit 2, `blocked: artifact nats-2.15.1-macos-aarch64 is not in the runtime cache`, owner `runtime maintainer`, tanpa unduhan dan tanpa root.

**Cek ulang dua fix dari temuan Milestone 1:**

- `provider freeze` menolak paket yang bukan `[[provider]]` (`eeac71a7`): **PASS**. `cargo xtask provider freeze providers/traefik` → exit 1, `official/traefik is not a [[provider]] of providers/catalog.toml ...`, `git status` sama, corpus tetap `nats` dan `postgresql`, tidak ada `.partial`. `providers/nats` tetap `already frozen`. `cargo test -p xtask freeze` 4 test.
- Finding `warning` untuk `apiVersion` `deprecated` (`2a29ff89`): **sebagian**. Finding `deprecated_api_version` berseveritas `warning` dibuat (unit test, `IssueCode::ALL` kini 59, `codegen --check` tetap 20 file). Tetapi lihat temuan 3.

**Tafsiran developer yang dinilai:**

- `ReapGuard` dan pencarian sisa proses lewat folder ASCII `stackly-test-*` dan `skt-*`, dan hanya dua folder root run itu: diterima; `ps` memang menulis ulang byte non ASCII (terlihat di output `ps` saat verify).
- `leftovers` untuk start gagal memakai process group pemegang `instance.lock` bila `lastFailure` tidak memberi PID: diterima; lebih ketat dari teks spec, bukan lebih longgar.
- Timeout dibaca dari `provider.yaml` lewat pembaca baris kecil: **tidak diterima**, lihat temuan 1.
- Panggilan socket sebagai JSON yang di decode ke `Call`: diterima; nama method yang salah ditolak sebelum dikirim.
- `remove_checked` dijalankan sebelum report ditulis: diterima; tanpa itu `cleanup.rootRemoved` tidak bisa diisi. Urutan di spec perlu disesuaikan.
- Report paket yang ditolak di `validate` punya `scenarios: []` dan field `failure` di tingkat atas: diterima; perlu masuk tabel *Report*.
- `runId` dibuat xtask dan diteruskan lewat `--run-id`: diterima; folder report default jadi sama dengan `runId`.
- xtask mengabaikan SIGINT dan SIGTERM selama harness berjalan: diterima; handler dipasang lewat `ctrlc`, jadi proses anak tidak mewarisi sinyal yang diabaikan.
- Owner `BLOCKED` `runtime maintainer` untuk artefak yang tidak ada di cache: persis teks spec.

**Temuan yang perlu fix atau keputusan:**

1. **Batas waktu langkah tidak dibaca dari manifest untuk ketiga paket resmi** (`apps/daemon/tests/conformance/steps.rs`, `Limits::of` dan `block_value`). Pembaca itu hanya mengenali blok YAML bergaya blok (baris `timeouts:` lalu key ber indent dua spasi). `providers/nats`, `providers/postgresql`, dan `providers/traefik` menulis `timeouts: { ... }` di satu baris, jadi nilainya tidak terbaca dan harness memakai default (init 120, readiness 60, stop 30, reload 30). Untuk PostgreSQL manifest berkata init 180, readiness 120, stop 60; jadi langkah `create`, `start`, dan `stop` dibatasi lebih pendek dari yang paket izinkan (150, 70, dan 40 detik, bukan 210, 130, dan 70). Untuk NATS dan Traefik batas `start` jadi lebih longgar (70, bukan 40 detik). Ini menyimpang dari tabel *Langkah skenario* dan *Value sourcing* ("blok `timeouts`, `readiness`, dan `health` manifest"). Di Mac ini semua run tetap lulus, tetapi di mesin lambat PostgreSQL bisa gagal palsu. Saran: ambil nilai dari model `ValidatedPackage` atau dari plan, bukan dari teks; test `a_block_value_is_read_only_directly_under_its_block` hanya menguji gaya blok.
2. **`--scenario <nama yang tidak ada>` berakhir `verdict: pass`, exit 0, dengan `scenarios: []`.** Probe: `cargo xtask provider test providers/example --scenario nope`. Salah ketik nama skenario memberi hasil hijau tanpa satu pun langkah berjalan. Saran: nama yang tidak dikenal menjadi kesalahan argumen atau `fail` di `validate`.
3. **Finding `deprecated_api_version` ber origin `package`, sedangkan `loadable()` berarti "tidak ada finding origin `package`"** (AC-2, `crates/core/src/provider/check.rs`). Begitu ada versi `deprecated`, paket yang memakainya akan `publishable: true` tetapi `loadable: false`, padahal tabel *Versi dan status* berkata paket itu "masih termuat dan berjalan". Hari ini tidak terpicu, dan tidak ada test yang memeriksa `loadable()` bersama warning itu. Saran: beri origin `authoring`, atau buat `loadable()` hanya menghitung `error`; lalu catat pilihan itu di spec. Kalimat spec "tidak menambah `IssueCode`" juga perlu disesuaikan dengan kode baru ini.

**Temuan rendah:**

4. `leftovers` sesudah `stop` memakai process group dari sampel pertama (sebelum `restart`), sesuai *Value sourcing*. Sesudah `restart` proses utama punya group baru, jadi anak yang tertinggal dari proses hasil restart hanya tertangkap lewat pemeriksaan lock dan port, sampai `start_again` membaca group yang baru. Ini kelemahan teks spec, bukan kode.
5. `--keep` mencetak `kept $TMPDIR/stackly-test-…`, yaitu path yang sudah disanitasi, bukan path literal. Bisa dipakai di shell yang sama, tetapi AC-12 berkata "mencetak path nya".
6. Di run yang diinterupsi, langkah `leftovers` tercatat `fail` dengan durasi 0 dan tanpa reason, padahal ia tidak sempat berjalan; `cleanup` di report yang sama bersih.
7. Example `provider_harness` keluar 2 untuk kesalahan argumen, sama dengan kode `blocked`; dan saat report gagal ditulis ia tetap mencetak baris `provider test report: <path>` untuk file yang tidak ada.

### Fix sesudah verify 2026-11-11 (`dev-f36-fix`)

Diverifikasi ulang oleh checker, lihat *Cek ulang* di bawah. Commit di `claude/arc-f36-spec-e2dee8`:

- Temuan 1 (batas waktu langkah), `b9908248`: `Limits::of` membaca `ValidatedPackage::manifest().timings()`, accessor publik baru `Manifest::timings` dengan `ManifestTimings` di `crates/core/src/provider/timeouts.rs` (default spec 0009 sudah terisi), jadi gaya YAML blok atau satu baris sama saja. Pembaca baris `block_value` dan test nya dihapus. Test `step_limits_come_from_the_checked_manifest_in_any_yaml_style`: PostgreSQL init 180, readiness 120, stop 60, reload 30; NATS readiness 30 dengan init default 120.
- Temuan 2 (`--scenario` tidak dikenal), `3a16bb1c`: nama yang tidak ada di paket membuat run berakhir `fail` di langkah `validate` (exit 1, kode `fail` spec ini) sebelum root dibuat, dengan `failure.message` `unknown scenario <nama>; the package has <nama>, ...`. Test `an_unknown_scenario_name_fails_at_validate_with_the_names_the_package_has`; probe `cargo xtask provider test providers/example --scenario nope` di bawah.
- Temuan 3 (deprecated dan `loadable`), `9eed1691`: keputusan lead dijalankan: `loadable()` hanya menghitung finding origin `package` ber severity `error`, `publishable()` hanya `error`; origin `deprecated_api_version` tetap `package`. `check_package_with_versions` (doc hidden, hanya untuk test) menerima tabel versi yang disuntikkan. Test gabungan `a_package_on_a_deprecated_version_still_loads_and_publishes_with_one_warning` (`crates/core/tests/provider_check.rs`): `providers/fake` dengan `stackly/v1alpha1` deprecated tetap `loadable` dan `publishable` dengan tepat satu warning. Aturan import fitur 37: `ImportCheck::valid` (`catalog/local/rules.rs`) sudah hanya menghitung `error`, tetapi `catalog::local::load` (`load.rs`) masih menganggap setiap finding `package` membuat entri `invalid`; file itu milik `dev-f37-m2` dan sudah dikabari, tidak diubah di sini.
- Temuan 6 (leftovers di run yang diinterupsi), `a1d43591`: sesudah sinyal, `leftovers` tercatat `skipped` dengan reason `a signal stopped the run; the cleanup checks for leftovers`, bukan `fail` 0 ms.
- Temuan 7 bagian kedua (baris report saat gagal tulis), `a1d43591`: `provider_harness` hanya mencetak `provider test report: <path>` untuk report yang tertulis; selain itu baris terakhirnya `the report could not be written to <path>`. Bagian pertama temuan 7 (exit 2 untuk kesalahan argumen) tidak diubah.

### Cek ulang 2026-11-11 (commit `b8dd0141`, fix `b9908248`, `3a16bb1c`, `9eed1691`, `a1d43591`)

**Verdict akhir Milestone 3: PASS.** Tidak ada temuan yang menggagalkan AC tersisa. Diuji pada `main` `2bfc4941`, yang memuat `b8dd0141`.

- Temuan 1 (batas waktu langkah): **tertutup.** `Limits::of` kini membaca `Manifest::timings()` dari manifest yang sudah divalidasi, dan pembaca baris `block_value` hilang. Test `step_limits_come_from_the_checked_manifest_in_any_yaml_style` lulus: PostgreSQL init 180, readiness 120, stop 60, reload 30 detik; NATS readiness 30 dengan init default 120.
- Temuan 2 (`--scenario` tidak dikenal): **tertutup.** `cargo xtask provider test providers/example --scenario nope` → `validate: fail (unknown scenario nope; the package has default, slow-ready)`, `verdict: fail`, exit 1, `scenarios: []`, tanpa root. `--scenario default --scenario nope` sama. `--scenario slow-ready` tetap berjalan dan `pass`. Exit 1 sesuai teks spec: exit 2 hanya untuk `blocked`, dan gagal di `validate` adalah verdict `fail`.
- Temuan 3 (warning `deprecated` dan `loadable()`): **tertutup.** `loadable()` kini hanya menghitung finding origin `package` yang `error`. Test gabungan `a_package_on_a_deprecated_version_still_loads_and_publishes_with_one_warning` lulus: satu warning `deprecated_api_version`, paket tetap `loadable` dan `publishable`, skenario terisi. `docs/providers/testing.md` sudah mengikuti (`b6691c9e`). Kalimat AC-2 ("`loadable()` benar kalau tidak ada finding origin `package`") dan kalimat "tidak menambah `IssueCode`" masih perlu disesuaikan lewat `/architect`.
- Temuan rendah 6 dan 7 (bagian kedua): fix nya dibaca di kode (`leftovers` tercatat `skipped` sesudah sinyal; baris report hanya untuk report yang tertulis); tidak diprobe ulang.
- `cargo test -p stacklyd --test provider_conformance` → 18 test lulus, 4 native `ignored`, 82 detik. `cargo test -p stackly-core --test provider_check` 9 test dan `--lib provider::check` 4 test lulus. Tidak ada root berpenanda yang tersisa, `git status` bersih.

Yang masih terbuka, semuanya rendah dan tidak menggagalkan AC: temuan 4 (group proses sesudah `restart`, kelemahan teks spec), temuan 5 (`kept` mencetak path ber `$TMPDIR`), temuan 7 bagian pertama (exit 2 untuk kesalahan argumen), dan satu catatan baru: `program_path` masih membaca `runtime.executable` lewat pembaca baris `block_text`, jadi manifest yang menulis blok `runtime` dalam satu baris akan gagal palsu di langkah `process`. Semua paket yang ada menulisnya bergaya blok.

### Acceptance-criteria coverage

- AC-9 · lengkap, lewat test dan `cargo xtask provider test providers/example`
- AC-10, AC-11 · lengkap. AC-11 memakai tafsiran yang diterima lead (2026-11-11): halaman `instance.logs_read` dibatasi 64 KiB satu batch log selain `limit: 1000`; langkah `logs` mengulang sampai halaman penuh (1000 baris, atau kurang dengan cursor `earlier`)
- AC-12 · lengkap; `--keep` menyimpan folder data lewat `keep_data` dan mencetak `kept <path>`
- AC-13 · lengkap. `--report` yang relatif dihitung dari folder kerja; xtask membuat `runId` dan meneruskannya lewat `--run-id` supaya folder report default sama dengan `runId` report
- AC-14 · lengkap
- AC-1 · bagian harness lengkap
- AC-15 · Milestone 4

## Milestone 4: Konformitas native

Bagian ini mencakup Build plan langkah 14: `nats_2_15_passes_conformance`, `postgresql_17_passes_conformance`, `postgresql_18_passes_conformance`, dan `traefik_3_7_passes_conformance` di `apps/daemon/tests/provider_conformance.rs`, masing masing `#[ignore = "native: ..."]` dengan guard evidence `Kind::Native` di badan fungsinya. Lulus harness tidak menambah `HANDOVER_PROVEN` (spec 0030); report keempatnya mencatat `handover: unproven`.

### UI / manual

- [ ] Run native dengan bukti (langkah manual user, AC-15): `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test provider_conformance -- --ignored --test-threads 1` → empat test `PASS` dengan subject `nats 2.15.1`, `postgresql 17.11`, `postgresql 18.6`, `traefik 3.7.15` dan digest artefak dari `versions.json`; artefak yang tidak ada di cache xtask (`~/Library/Caches/dev.stackly.xtask/runtime/<sha256>`) membuat test nya `BLOCKED` dengan reason `artifact <id> is not in the runtime cache` dan owner `runtime maintainer`, tanpa unduhan. Saat build (2026-11-11) keempat artefak ada di cache, dan run langsung tanpa wrapper evidence (`cargo test -p stacklyd --test provider_conformance -- --ignored --test-threads 1`) lulus 4 dari 4 dalam 76 detik; record evidence native yang resmi menunggu run dengan wrapper di atas.

### Commands

- [x] `cargo test -p stacklyd --test provider_conformance` → 17 test fixture lulus, empat test native `ignored`
- [x] `cargo xtask evidence check` → bersih (penanda native ada di badan setiap test native)

### Hasil /check verify 2026-11-11 (Milestone 4, commit 5343f0c7)

**Verdict: FAIL pada satu klausa AC-15** (format `actual`, temuan 1). Sisanya terpenuhi di kode. Record evidence native resmi tetap `NOT_RUN`, owner user (langkah manual di atas).

- `cargo test -p stacklyd --test provider_conformance` → 17 test fixture lulus, empat test native `ignored`. `cargo xtask evidence check` → bersih.
- Catatan run langsung, bukan record: atas izin lead, `cargo test -p stacklyd --test provider_conformance -- --ignored --test-threads 1` dijalankan dari cache runtime, tanpa wrapper evidence dan tanpa unduhan. Hasil: 4 dari 4 lulus dalam 72,6 detik (`nats_2_15`, `postgresql_17`, `postgresql_18`, `traefik_3_7`). Daftar proses `nats-server`, `postgres`, dan `traefik` sama sebelum dan sesudah (layanan Homebrew dan Herd milik user tidak tersentuh), tidak ada root berpenanda tersisa, `git status` bersih.
- AC-15 butir demi butir: satu test native per line dengan `#[ignore = "native: ..."]` dan `covers = ["AC-029", "REQ-029"]`: terpenuhi. Subject dari versi `runtime.get` dan digest artefak: terpenuhi di kode (`ev.subject(name, version, sha256, Source::Managed)`). `BLOCKED` tanpa unduhan untuk artefak yang tidak ada di cache: terpenuhi (probe salinan `nats` di Milestone 3). `handover` hanya `proven` atau `unproven`, tanpa menambah `HANDOVER_PROVEN`: terpenuhi.

**Temuan:**

1. **`actual` tidak berformat seperti AC-15.** AC-15 meminta `<provider> <versi> ready in <ms> ms, stopped in <ms> ms`. Kode menulis `verdict pass; default:catalog=pass, default:runtime=pass, ...` (`apps/daemon/tests/provider_conformance.rs`, fungsi `native`). Perlu fix di kode, atau keputusan mengubah bunyi AC-15.
2. Temuan 1 Milestone 3 (batas waktu langkah) berlaku juga di sini: ketiga paket native menulis `timeouts` di satu baris, jadi run native memakai batas default, bukan batas manifest.

### Fix sesudah verify 2026-11-11 (`dev-f36-fix`)

Diverifikasi ulang oleh checker, lihat *Cek ulang* di bawah.

- Temuan 1 (`actual` AC-15), `a5a49b00`: fungsi `native` menulis `actual` lewat `actual()`: `<provider> <versi> ready in <ms> ms, stopped in <ms> ms` dari durasi langkah `start` dan `stop` skenario `default` (atau skenario pertama). `the_example_package_passes_conformance` memeriksa bentuk itu pada run fixture. Run native dari cache tanpa unduhan (`cargo test -p stacklyd --test provider_conformance -- --ignored --test-threads 1`): 4 dari 4 lulus dalam 72 detik. Dengan `STACKLY_EVIDENCE_DIR` di folder sementara (bukan record resmi): `nats 2.15.1 ready in 836 ms, stopped in 218 ms` dan `postgresql 18.6 ready in 1149 ms, stopped in 218 ms`. Record native resmi tetap langkah manual user di atas.
- Temuan 2: diperbaiki oleh fix temuan 1 Milestone 3; run native kini memakai batas manifest.

### Cek ulang 2026-11-11 (commit `b8dd0141`, fix `a5a49b00`)

**Verdict akhir Milestone 4: PASS untuk kode AC-15.** Record evidence native resmi tetap `NOT_RUN`, owner user (langkah manual di atas).

- Temuan 1 (format `actual`): **tertutup.** `actual()` menulis `<provider> <versi> ready in <ms> ms, stopped in <ms> ms`, dan test fixture `the_example_package_passes_conformance` memeriksa bentuk itu. Catatan run langsung, bukan record: `nats_2_15_passes_conformance` dijalankan dari cache dengan `STACKLY_EVIDENCE_DIR` menunjuk scratchpad checker (tidak ada yang masuk `evidence/`); record nya `PASS`, `kind` `native`, `covers` `AC-029` dan `REQ-029`, `actual` `nats 2.15.1 ready in 1779 ms, stopped in 458 ms`, subject `nats 2.15.1` dengan digest `ed0fe392…`.
- Temuan 2 (batas waktu langkah di run native): tertutup oleh fix temuan 1 Milestone 3.
- Catatan run langsung, bukan record: `cargo test -p stacklyd --test provider_conformance -- --ignored --test-threads 1` → 4 dari 4 lulus dalam 69 detik, dari cache tanpa unduhan; proses `nats-server`, `postgres`, dan `traefik` milik user sama sebelum dan sesudah.

### Acceptance-criteria coverage

- AC-15 · kode lengkap; bukti native resmi = langkah manual di atas

## Milestone 5: Spike dari paket

Bagian ini mencakup Build plan langkah 15: `xtask/src/runtime/spike/package_recipe.rs` (`PackageRecipe`), `xtask/src/runtime/spike/closure_probes.rs`, tanda tangan baru `recipe(repo, provider) -> Result<Box<dyn Recipe>>` dan `version_args(&self) -> Vec<String>`, `tokio` di `xtask`, satu entri `EXPOSE_ALLOWED`, penghapusan recipe NATS, PostgreSQL, dan Traefik, dan penanda `// provisional: F40`, `F42`, `F43`.

### UI / manual

- [ ] Run spike native dari paket (langkah manual user, status `NOT_RUN`, owner @ojiepermana; butuh sesi login GUI, artefak di cache xtask, dan bisa mengunduh artefak yang belum ada): dari checkout bersih, tanpa run test lain yang aktif, jalankan `cargo xtask evidence run --kind native -- cargo test -p xtask --test native_runtime -- --ignored`. Harapan: `nats`, `postgresql` 17, `postgresql` 18, dan `traefik` `PASS` dengan langkah `recipe`, `init`, `start`, `ready`, `version`, `probes`, `runtime audit`, dan `stop` dari paket; `observedVersion` `2.15.1`, `17.11`, `18.6`, dan `3.7.15`; listener hanya port loopback yang dialokasikan (dua untuk NATS dan Traefik, satu untuk PostgreSQL); probe PostgreSQL `icu`, `pgcrypto`, `lz4`, `zstd` `PASS` dengan `detail` `icu` berbentuk `stackly_icu_probe: provider i, locale und, collversion <v>`; probe NATS `jetstream` `PASS`; `mysql`, `clickhouse`, dan `valkey` tetap memakai recipe tulisan tangan dengan hasil yang sama seperti sebelumnya. Kalau SQL probe `icu` perlu disesuaikan, itu update spec 0003, bukan perubahan diam diam di kode. Sesudah lulus, rekam lewat alur dua commit dan `cargo xtask runtime record <path>` spec 0003 bila record baru diinginkan sebagai bukti AC-7 probe baru. Tanpa artefak di cache atau tanpa sesi GUI, test itu `BLOCKED` dengan alasannya.

### Commands

- [x] `cargo test -p xtask --bin xtask runtime::spike::package_recipe` → 6 test lulus: `find_package` memetakan `nats`, `postgresql`, `traefik` ke folder paketnya dan `mysql`, `clickhouse`, `valkey` ke tidak ada; paket yang tidak `publishable` (`fake-command`) ditolak dengan `package fake-command is not publishable` dan paket tanpa `runtime.versionCheck` dengan `package has no versionCheck`; NATS dibangun dari paket (program `nats-server` di root artefak, `--config` ke file yang memuat kedua port yang dialokasikan, env kosong AC-6 utuh, `version_args` `--version`, versi `2.15.1` dari `pattern`); PostgreSQL 17 dan 18 merencanakan `initdb` dengan `-Upostgres` dan `materializeSecret`, start `bin/postgres`, password fixture ber awalan `stackly-fixture-credential-`, endpoint `socket` di folder run instance di bawah `/tmp/sk-*`, versi `17.11` dan `18.6` dari `postgres --version`; Traefik start dengan `--configFile` yang ada dan versi `3.7.15` dari baris `Version:`; paket fake menjalankan init (stdin secret), start, ready (probe HTTP lewat `stackly_core::supervisor::probe::run`), versi, dan stop SIGTERM terhadap `stackly-fake-server` sungguhan → AC-16
- [x] `cargo test -p xtask --bin xtask runtime::spike::closure_probes` → 5 test lulus tanpa server: PostgreSQL memilih empat probe dalam urutan tetap `icu`, `pgcrypto`, `lz4`, `zstd`, setiap panggilan memakai `psql -X -w -v ON_ERROR_STOP=1 -At -h <socket> -p <client> -U postgres -d postgres` (atau `pg_dump -w` dengan host, port, dan user yang sama) dengan `PGPASSWORD` dari `superuserPassword` dan tanpa `stackly_readiness`; probe `icu` mengirim `create database stackly_icu_probe template template0 locale_provider icu icu_locale 'und'` dan `select datlocprovider ...` dalam dua panggilan; keluar 0 dengan `i` berarti `PASS`, `b` atau `CREATE` yang gagal berarti `FAIL` dengan statement nya di `detail`, dan probe lain tetap berjalan; dump `zstd` kosong atau tidak ada `FAIL`; NATS memeriksa `/healthz?js-enabled-only=true` di port endpoint `monitoring`; Traefik dan Valkey tanpa probe → AC-16 (spec 0003 AC-7)
- [x] `cargo test -p xtask --bin xtask runtime::spike::providers` → lulus: recipe tulisan tangan MySQL, ClickHouse, dan Valkey tidak berubah (normalisasi versi dan `uses_socket_dir` sama), provider tanpa langkah gagal dengan `the spike has no steps for provider redis` → AC-16
- [x] `grep -rn "provisional: F36" xtask/src/runtime/` → kosong; `grep -rn "provisional: F4" xtask/src/runtime/spike/providers.rs` → tepat `F43` (ClickHouse), `F42` (MySQL), `F40` (Valkey) → AC-16
- [x] `cargo xtask check-deps` → lulus; `EXPOSE_ALLOWED` memuat satu entri baru `("xtask", ["runtime::spike::package_recipe"])` dengan komentar alasan, dan test `the_expose_allowlist_is_the_call_sites_that_exist` memuatnya → AC-16
- [x] `cargo test -p xtask --no-fail-fast` → lulus seluruhnya (test native `native_runtime` tetap `ignored` tanpa `--ignored`) → AC-16

### Hasil /check verify 2026-11-11 (Milestone 5, commit f7f8519c)

**Verdict: PASS untuk bagian kode AC-16.** Bukti native per provider `NOT_RUN`, owner user (langkah manual di atas tidak dijalankan: butuh izin user, sesi login GUI, dan bisa mengunduh serta mengganti bukti). Keenam langkah Commands dijalankan dengan `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0` dan lulus. Tidak ada temuan FAIL.

| Perintah | Hasil |
| --- | --- |
| `cargo test -p xtask --bin xtask runtime::spike::package_recipe` | 6 lulus |
| `cargo test -p xtask --bin xtask runtime::spike::closure_probes` | 5 lulus |
| `cargo test -p xtask --bin xtask runtime::spike::providers` | 5 lulus |
| `grep -rn "provisional: F36" xtask/src/runtime/` | kosong; `providers.rs` memuat tepat `F43` (ClickHouse), `F42` (MySQL), `F40` (Valkey) |
| `cargo xtask check-deps` | 13 crate lulus; `the_expose_allowlist_is_the_call_sites_that_exist` lulus |
| `cargo test -p xtask --no-fail-fast` | 520 unit test plus test integrasi lulus; 8 test `native_runtime` tetap `ignored` |
| `cargo xtask runtime check` (tambahan) | `8 artifact(s), 7 evidence run(s), and 5 compatibility run(s) follow the rules` |
| `cargo clippy -p xtask --all-targets --locked -- -D warnings`, `cargo fmt --all --check` (tambahan) | bersih |

**AC-16 butir demi butir:**

- Init, start, ready, versi, dan stop dibangun dari paket lewat `PackageRecipe`: terpenuhi di kode (`xtask/src/runtime/spike/package_recipe.rs`) dan di unit test pembentukan langkah untuk NATS, PostgreSQL 17 dan 18, dan Traefik.
- Struct recipe NATS, PostgreSQL, dan Traefik dihapus: terpenuhi. `providers.rs` hanya berisi `ClickHouse`, `MySql`, dan `Valkey`, dan isinya tidak berubah selain tanda tangan `version_args`.
- Hasil spike tetap `stackly.runtime-spike-result/v1` dengan field yang sama: terpenuhi, `result.rs` tidak berubah di commit ini.
- Penanda `// provisional: F36` hilang; `F40`, `F42`, `F43` ada: terpenuhi.
- `xtask` memakai `stackly-core` dengan `test-support`, dan `tokio` dengan fitur `rt`, `net`, `time`: terpenuhi.
- `EXPOSE_ALLOWED`: satu entri baru `("xtask", ["runtime::spike::package_recipe"])` dengan komentar alasan. Ketiga panggilan `expose()` di `xtask/src` ada di modul itu saja (env plan, stdin langkah init, file secret 0600). Nilainya selalu `FIXTURE_CREDENTIAL_PREFIX` ditambah UUID buatan recipe itu sendiri. `closure_probes.rs` tidak memanggil `expose()`; ia menerima nilai fixture itu sebagai teks.
- Test unit worker dengan paket fake: terpenuhi. Test menjalankan init (secret lewat stdin), start, ready lewat `stackly_core::supervisor::probe::run`, versi, dan stop SIGTERM terhadap `stackly-fake-server` sungguhan.

**Sisa di sistem:** tidak ada folder `/tmp/sk-*` sebelum test, sesudah test `package_recipe`, dan sesudah `cargo test -p xtask` seluruhnya. Tidak ada proses `stackly-fake-server` baru; satu proses yang terlihat sudah ada sebelum run ini dan milik sesi lain (root nya di scratchpad sesi `dev-f70-m2`), jadi tidak disentuh.

**Probe closure terhadap spec 0003** (dibaca di kode dan dibuktikan unit test tanpa server): superuser literal `postgres`; `psql -X -w -v ON_ERROR_STOP=1 -At -h <socket> -p <client> -U postgres -d postgres` dengan `PGPASSWORD` dari `superuserPassword`; probe `icu` membuat `stackly_icu_probe` lalu membaca `datlocprovider` dalam dua panggilan; urutan tetap `icu`, `pgcrypto`, `lz4`, `zstd`, dan setiap probe berjalan apa pun hasil probe lain; `pg_dump` memakai host, port, user, dan env yang sama; role `stackly_readiness` tidak dipakai; NATS memeriksa `/healthz?js-enabled-only=true` di endpoint `monitoring`.

**Tafsiran developer yang dinilai** (semua diterima):

- `uses_socket_dir` selalu `true`: diterima. Baris *context* spec 0036 menaruh root runtime di `/tmp/sk-<8 hex>` untuk setiap paket, jadi folder itu memang harus dibuat worker. Baris `uses_socket_dir` di tabel yang sama ("ada endpoint `unix` di paket") berseberangan dengan itu. Lihat temuan rendah 1.
- Port dialokasikan di `init`: diterima, karena plan init sudah me-render config yang memuat port dan context harus sama di init dan start. Lihat temuan rendah 2.
- `CleanEnv` AC-6 dulu, lalu env plan hanya untuk key yang belum diset: persis spec 0003 ("tambahan, bukan pengganti"). Validasi manifest juga menolak key `DYLD_` dan `LD_`, jadi plan tidak bisa menyelipkannya.
- Folder instance dibuat mode 0700 sebelum init: diterima; ini yang daemon lakukan saat create, dan spike tidak punya daemon.
- Nilai `psql` dibaca dari baris tak kosong terakhir stdout: diterima; dengan beberapa `-c`, `psql -At` juga mencetak tag perintah (`CREATE EXTENSION`, `INSERT 0 1`), jadi hasil ada di baris terakhir. `pg_dump -w`: diterima; sejalan dengan alasan `-w` di spec 0003.
- Panggilan ketiga probe `icu` hanya informasi: persis spec 0003 (`datlocale` dan `datcollversion` bukan syarat lulus). Kalau panggilan itu gagal, `detail` tetap `stackly_icu_probe: provider i`.
- `find_package` memilih `official/` lebih dulu: diterima. Lihat temuan rendah 3.
- `SpikeFacts.settings` belum dibaca: sesuai petunjuk spec 0003 (probe memakai `postgres` secara tetap).

**Yang hanya bisa dibuktikan run native** (jadi belum terbukti di sini):

- `initdb` paket PostgreSQL 17 dan 18 benar benar jalan di context spike: password fixture lewat file secret, auth `scram-sha-256`, path data berisi spasi dan karakter non ASCII.
- Probe readiness paket (lewat `probe::run`) lulus terhadap server sungguhan dalam 120 detik, untuk NATS, PostgreSQL, dan Traefik.
- SQL probe `icu` diterima PostgreSQL 17 dan 18 (`create database ... locale_provider icu icu_locale 'und'`, kolom `datlocale`), dan tiga probe lain lulus lewat socket dengan password.
- `observedVersion` dari binary sungguhan cocok dengan `pattern` paket (`2.15.1`, `17.11`, `18.6`, `3.7.15`).
- Audit listener: hanya port loopback yang dialokasikan (dua untuk NATS dan Traefik, satu untuk PostgreSQL), termasuk risiko port yang dialokasikan di `init` dipakai proses lain sebelum start.
- Audit runtime `lsof`, stop dalam 60 detik dengan sinyal dari paket, JetStream NATS, dan config Traefik di path berisi spasi.
- Semua itu di bawah LaunchAgent dengan environment kosong AC-6.

**Temuan rendah** (tidak menggagalkan AC-16):

1. Spec 0036 tabel *Spike dari paket*: baris `uses_socket_dir` ("ada endpoint `unix` di paket") tidak cocok dengan baris *context* (root runtime selalu di `/tmp/sk-<8 hex>`). Kode mengikuti baris *context*. Saran: perbaiki baris `uses_socket_dir` lewat `/architect`.
2. Spec 0003 *Langkah spike per provider* berkata port dialokasikan "tepat sebelum setiap start", dan komentar `Recipe::start` di `providers.rs` masih berkata `start` yang mengalokasikan port. `PackageRecipe` mengalokasikannya di `init`. Jeda antara alokasi dan bind jadi lebih panjang (selama `initdb`). Saran: catat pengecualian ini di spec 0003 dan di komentar trait.
3. `find_package` mengenali paket `official/` dengan mencari teks `id: official/` di `provider.yaml` mentah, bukan dari ID hasil parse. Cukup untuk paket hari ini; teks yang sama di komentar atau field lain bisa menyesatkan.
4. `PackageRecipe::probes` mengembalikan daftar kosong tanpa pesan kalau context gagal dibangun. Dalam praktik tidak terjangkau, karena `init` dan `start` sudah membangun context yang sama lebih dulu; tetapi kalau terjadi, spike PostgreSQL akan tampil tanpa probe AC-7, bukan `FAIL`.
5. Stop lewat command (`StopPlan::Command`) dijalankan di salinan `Ctx`, jadi ekor log perintah stop itu tidak masuk hasil spike. Belum ada paket yang memakai stop lewat command.

### Acceptance-criteria coverage

- AC-16 · lengkap di kode dan unit test; bukti native per provider menunggu langkah manual di atas (`NOT_RUN`, owner user)

## Verify penuh fitur 36

### Hasil `/check verify` 2026-11-11 (seluruh spec, commit `f1f43ea4`)

**Verdict: PASS.** AC-1 sampai AC-16 terpenuhi di kode dan bukti fixture; tidak ada butir spec tanpa kode. Yang belum ada hanya langkah manual di daftar di bawah (`NOT_RUN`, owner user). Langkah Commands kelima milestone sudah dicentang di verify per milestone hari ini; di sini hanya yang tersentuh merge sesudahnya (protokol 4.0 fitur 37, CLI dan SwiftUI fitur 37, `dev-f36-fix`) yang dijalankan ulang, pada `main` `f1f43ea4`, dengan `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`.

| Perintah (dijalankan ulang) | Hasil |
| --- | --- |
| `cargo xtask provider check`, `check-deps`, `codegen --check`, `docs check`, `fuzz seed` lalu `git status fuzz/seeds` | bersih |
| `cargo test -p stackly-core --lib provider::` dan `--test provider_check --test provider_compat --test provider_packages --test provider_rules --test fuzz_regressions` | 127 dan 122 lulus |
| `cargo test -p stackly-protocol --lib provider`, `--test provider_fixtures` | lulus |
| `cargo test -p stacklyd --test provider_validate --test create_form_fixtures`, `--lib ipc::provider` | 7 dan 1 lulus |
| `cargo test -p stackly-cli --test provider validate`, `--bin stackly provider` | 3 dan 14 lulus |
| `cargo test -p xtask --test provider_check --test provider_test`, `--bin xtask provider::`, `--bin xtask runtime::spike` | 12, 16, dan 75 lulus |
| `swift test` `StacklyUI` `EveryCreateFormFixtureTests\|SchemaFormModelTests`; `clients/swift` `ProviderValidateContractTests\|CreateContractTests` | 17 dan 12 lulus |
| `cargo test -p stacklyd --test provider_conformance` (dijalankan di cek ulang fix pada `2bfc4941`; path fitur 36 tidak berubah sejak itu) | 18 lulus, 4 native `ignored` |
| Probe `stackly provider validate` terhadap `stacklyd` nyata di root sementara, protokol 4.0 | contoh: `ok testing/example 0.1.0 287cdc78…`, exit 0; `stackly/v2`: satu baris `unsupported_api_version`, exit 2 `cli.provider_invalid`; hash isi root sama |
| `cargo xtask provider test providers/example` | exit 0, `verdict: pass`, 16 langkah per skenario, `rootRemoved: true`, `git status --porcelain` sama |
| `cargo xtask provider test` pada paket `acme/fakeish` yang `publishable` | exit 1, `validate: fail (the harness runs only the official and testing namespaces, not acme)`, tanpa root |

**Verdict per AC:**

| AC | Verdict | Bukti utama |
| --- | --- | --- |
| AC-1 | terpenuhi | satu finding `unsupported_api_version` di `check_package`, `provider.validate`, `provider check`, entri catalog `invalid`, dan harness (`validate`, tanpa root); probe daemon nyata |
| AC-2 | terpenuhi | `provider_check` core; `fake-command` `loadable` tetapi tidak `publishable`. Sesudah keputusan lead, `loadable()` hanya menghitung `error` ber origin `package`; kalimat AC-2 belum mengikuti |
| AC-3 | terpenuhi kecuali fuzz 10 menit | aturan file skenario lewat test dan probe; seed fuzz bersih. Fuzz 10 menit `NOT_RUN` |
| AC-4 | terpenuhi | format baris, `warning: `, dan `not frozen` (probe Milestone 1) |
| AC-5 | terpenuhi | kontrak, daemon, CLI, Swift; di atas protokol 4.0 |
| AC-6 | terpenuhi | corpus `nats 1.0.1` dan `postgresql 1.0.2`; `provider_compat`; `freeze` menolak digest lain dan paket di luar `[[provider]]` |
| AC-7 | terpenuhi | paket contoh, README dengan link ke `docs/providers/README.md`, scaffold tiga baris, scaffold lulus `provider test` |
| AC-8 | terpenuhi, dengan satu catatan | fixture form per folder dan test `StacklyUI` generik. Catatan: paket baru di `providers/` juga butuh satu entri `PROVIDER_WORDS` di `xtask` dan satu baris `index.json`, jadi kalimat "tidak menambah kode Swift atau Rust" tepat untuk core dan SwiftUI, tidak untuk `xtask` |
| AC-9 | terpenuhi | `provider test` contoh dan fake `pass`, 16 langkah berurutan; namespace lain ditolak di `validate` (probe di atas) |
| AC-10 | terpenuhi | tiga test rogue, unit test `not_foreground` |
| AC-11 | terpenuhi | tiga test fault, dengan bunyi "halaman penuh: 1000 baris atau 64 KiB" yang sudah diputuskan |
| AC-12 | terpenuhi | `test_root` 14 test; probe `--keep`, `--cleanup`, folder tanpa penanda, penanda ber `pid` hidup |
| AC-13 | terpenuhi, satu klausa tanpa bukti runtime | report, exit code 0, 1, 2, 130, baris terakhir stderr, tulis atomik (file sementara lalu rename). Checkout bersih: lihat di bawah. Klausa "tidak membuka koneksi selain ke loopback" tidak diukur saat run; hanya terbaca dari kode |
| AC-14 | terpenuhi | `provider_test` xtask: scaffold di luar tree, `check`, `test`, `git status` sama |
| AC-15 | terpenuhi di kode | empat test native, `actual` berformat AC-15, `BLOCKED` tanpa unduhan. Record evidence resmi `NOT_RUN` |
| AC-16 | terpenuhi di kode | `PackageRecipe`, `closure_probes`, recipe lama hilang, penanda `F40`, `F42`, `F43`. Run spike native `NOT_RUN` |

**Done when, kalimat demi kalimat:**

1. "Manifest dengan versi schema yang tidak didukung ditolak dengan penjelasan tanpa menjalankan lifecycle": **terpenuhi** (AC-1). Pesan menyebut versi yang didukung; tidak ada root, proses, atau tulisan.
2. "Provider fake server baru bisa install, create, start, health, dan tampil sebagai form di GUI tanpa perubahan Rust core atau SwiftUI": **terpenuhi di tingkat fixture.** Paket `providers/example/` dan paket hasil scaffold melewati langkah `runtime`, `form`, `create`, `start`, dan `health` harness. Commit yang menambah paket contoh (`d4198570`, `94b55c5b`, `0a5fb499`, `2f8c9d0d`, `74f4092d`, `847d8c96`) tidak menyentuh `crates/core/src`, `apps/daemon/src`, atau source `StacklyUI`: hanya folder paket, golden test, fixture, dan `xtask`. Form di GUI dibuktikan `EveryCreateFormFixtureTests` atas `create-form-example.json`. Sesi GUI nyata adalah langkah manual.
3. "Harness berjalan di test root ephemeral dan cleanup memeriksa ownership test run": **terpenuhi** (AC-12).
4. "Harness menghasilkan report dari repository bersih": **terpenuhi.** `cargo xtask docs walkthrough` (fitur 38, dijalankan checker pada `9a9e97fe`) membuat clone baru tanpa `target/` dan tanpa file untracked, lalu menjalankan `provider check providers/example` dan `provider test providers/example`: baris `ok`, report `pass`, dan `git status` clone hanya berisi paket hasil scaffold. Itu jalur yang sama dengan klausa checkout bersih AC-13, jadi clone kedua tidak dibuat. Yang tidak tercakup: tanpa cache crate cargo (jalur unduhan crate pertama).

**Trace:** REQ-030 terpenuhi (AC-1, AC-2, AC-5, AC-6, AC-7, AC-9, *Kebijakan kompatibilitas*). AC-030 terpenuhi. AC-034 terpenuhi (AC-13, AC-14; record manual walkthrough ada di spec 0038). AC-029 (akhir) terpenuhi di tingkat fixture (AC-8, AC-9, AC-14); bukti native nya (AC-15) `NOT_RUN`.

**Pencarian butir yang tertinggal:**

- `// provisional: F36`: tidak ada lagi di repo. Penanda spike yang tersisa `F40`, `F42`, `F43`.
- Test bernama di tabel spec: semuanya ada dan lulus.
- Follow-up spec lain yang dijawab spec 0036 masih `[ ]` dan menunggu `/sync` atau `/architect`: spec 0009 baris "Fitur 36" (kebijakan kompatibilitas), spec 0010 baris "Fitur 36" (kontrak proses), spec 0023 baris "Fitur 36" (probe native), dan di spec 0036 sendiri baris update spec 0009, 0010, 0023, 0030, 0014, 0006, 0008, 0038, serta catatan `/sync` untuk `AGENTS.md` (perintah `provider new`, `provider test`, `provider freeze`). Spec 0014 tidak punya baris yang menyebut fitur 36. Baris spec 0030 ("Fitur 36 sampai 43") memang tetap terbuka sampai provider terakhir. Baris spec 0003 untuk run native tetap terbuka (owner user).
- Kalimat spec 0036 yang belum mengikuti kode: AC-2 dan *Batas* tentang `loadable()` dan "tidak menambah `IssueCode`"; "`from_files` tidak berubah"; escape dan batas 128 karakter pesan; baris `uses_socket_dir`; urutan `remove_checked` dan report; field `failure` dan `scenarios: []` di report; `runId` dari xtask; bunyi AC-11 (64 KiB); AC-8 ("tidak menambah kode Swift atau Rust").

**Langkah manual (tidak dijalankan):**

| Langkah | Status | Reason | Owner |
| --- | --- | --- | --- |
| Fuzz 10 menit `cargo xtask fuzz run provider_scenarios --time 600` (AC-3) | `NOT_RUN` | langkah manual menurut brief; smoke run 30 detik developer lulus | user |
| Record evidence native konformitas `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test provider_conformance -- --ignored --test-threads 1` (AC-15, AC-029) | `NOT_RUN` | record resmi butuh run dengan wrapper oleh user; run langsung checker 4 dari 4 lulus dari cache, sebagai catatan | user |
| Run spike native dari paket `cargo xtask evidence run --kind native -- cargo test -p xtask --test native_runtime -- --ignored` (AC-16, spec 0003 AC-7 probe `icu`) | `NOT_RUN` | butuh izin user dan sesi login GUI, bisa mengunduh dan mengganti bukti | user |
| Sesi GUI nyata: form New Instance untuk paket contoh (Done when kalimat 2) | `NOT_RUN` | sesi GUI manual tidak boleh dijalankan sub sesi; test `StacklyUI` generik lulus | user |
| Checkout bersih tanpa cache crate cargo (AC-13, klausa jaringan) | `NOT_RUN` | butuh unduhan crate dari jaringan; jalur dengan cache hangat terbukti lewat walkthrough | user |

**Temuan rendah yang masih terbuka** (tidak menggagalkan AC): `leftovers` sesudah `restart` memakai process group lama (teks spec); `--keep` mencetak path ber `$TMPDIR`; `provider_harness` keluar 2 untuk kesalahan argumen; `program_path` masih membaca `runtime.executable` lewat pembaca baris; CLI mencetak baris `<n> more finding(s) not shown` yang belum ada di spec; `find_package` mengenali `official/` lewat teks; `PackageRecipe::probes` kosong tanpa pesan bila context gagal; ekor log stop lewat command tidak masuk hasil spike.
