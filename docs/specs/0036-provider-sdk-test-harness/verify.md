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

- [ ] Langkah 13, dari checkout tanpa file untracked: `cargo xtask provider check providers/example` lalu `cargo xtask provider test providers/example` → baris `ok`, stderr berakhir `provider test report: <repo>/target/provider-test/<runId>/report.json`, exit 0, report `verdict: pass` dengan `runId` sama dengan nama folder nya; `git status --porcelain` sama sebelum dan sesudah → AC-9, AC-13
- [ ] `cargo xtask provider test apps/daemon/tests/conformance/packages/fake-rogue --report <tmp>/r.json` → report di `<tmp>/r.json`, baris terakhir stderr menyebutnya → AC-13
- [ ] Interupsi: jalankan `cargo xtask provider test providers/example --report <tmp>/i.json`, tunggu sampai proses `provider_harness` muncul (`ps -axo pid=,comm= | awk '$2 ~ /provider_harness$/'`), kirim satu `kill -INT <pid>` → stderr `provider_harness: stopping after the current step`, exit 130, report `interrupted: true`, `verdict: fail`, `cleanup.rootRemoved: true` → AC-13
- [ ] `cargo xtask provider test --cleanup` → exit 0, baris terakhir stderr `provider test --cleanup: removed <n> folder(s)`; hanya root berpenanda milik uid ini yang `pid` nya mati yang terhapus (diuji `sweep_removes_a_root_whose_run_is_gone_and_keeps_one_whose_run_lives`) → AC-12
- [ ] `cargo test -p xtask --test provider_test` → 2 test lulus: `a_scaffold_outside_the_repo_passes_check_and_test_without_changing_the_repo` (`provider new mytool --out <TestRoot>`, `provider check`, `provider test --report`, report `pass`, baris terakhir stderr, `git status --porcelain` repo sama) dan `cleanup_runs_alone_and_exits_0` → AC-14, AC-12
- [ ] `cargo test -p stacklyd --example provider_harness` (unit test argumen) dan `cargo test -p xtask provider::harness` → lulus
- [ ] `cargo xtask docs check` → `ok docs`; blok `bash` README paket contoh memuat `provider new`, `provider check`, dan `provider test` → AC-7

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

- [ ] `cargo test -p stacklyd --test provider_conformance` → 17 test fixture lulus, empat test native `ignored`
- [ ] `cargo xtask evidence check` → bersih (penanda native ada di badan setiap test native)

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
