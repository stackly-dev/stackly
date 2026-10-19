# Verify: Provider engine & manifest schema · spec 0009 · updated 2026-10-18

_Langkah diturunkan dari acceptance criteria spec 0009. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27, pada commit bersih. Engine provider adalah library, jadi "menjalankan app" berarti tiga hal nyata: `cargo xtask provider check` dan `cargo xtask codegen` sebagai alat yang dipakai kontributor, binary `stackly-fake-server` asli, dan program scratch di luar repo (tidak di commit) yang memanggil API publik `stackly_core::provider` lewat `MacosFilesystem` lalu menjalankan plan dengan `std::process::Command` seperti executor spec 0010 (tanpa cek miliknya). Skrip scratch dan ledger bukti run 2026-10-18 disimpan di folder catatan sesi (`feature-9-verify/`), bukan di repo. Setiap perubahan sementara pada file repo dikembalikan dengan `git checkout` dan `git status` bersih sesudahnya._

## UI / manual

Tidak ada. Engine provider tidak punya UI; form settings milik fitur 20 dan 29.

## Commands

### Paket contoh, `provider check`, dan codegen

- [x] `cargo xtask provider check` → tiga baris `ok testing/fake 0.1.0 26836aad…`, `ok official/nats 1.0.0 eb02876b…`, `ok official/traefik 1.0.0 b3e039f1…`, exit 0 → AC-14
- [x] Program scratch Python menyalin `providers/fake`, `providers/nats`, `providers/traefik`, dan fixture `fake-command` ke folder scratch, membuat 101 salinan yang diubah (satu perubahan per salinan), lalu menjalankan `target/debug/xtask provider check <folder>` per salinan → 96 dari 97 salinan rusak exit 1 dengan kode dan lokasi yang diharapkan, keempat salinan yang masih valid exit 0, dan satu gagal (lihat "Gagal" di bawah). Contoh baris: `fake: templates/link.tmpl .: symlink_not_allowed`, `fake: provider.yaml .: not_a_regular_file` (FIFO), `file_too_large` (256 KiB + 1 byte dan template ke 33), `missing_file` (`versions.json`), `unexpected_entry` (`notes.txt`, subfolder di `templates/`), `unreferenced_template`, `not_utf8` (BOM dan byte `0xff`), `nats2: … directory_name_mismatch` → AC-1, AC-3
- [x] Salinan dengan `.DS_Store` di root dan `templates/`, file biner di `tests/`, serta `README.md` dan `LICENSE-NOTICES.md` yang bukan UTF-8 → `ok` dengan digest yang sama dengan aslinya → AC-1, AC-12
- [x] Decode ketat: `apiVersion: stackly/v1beta1` plus field asing → hanya `unsupported_api_version`; `kind: Plugin` → `unknown_kind`; `permissions.network`, `permissions.sandbox`, `lifecycle.start.shell` → `unknown_field` dengan path; tag lokal `!custom` → `invalid_value` "yaml tags are not allowed"; merge key → `lifecycle.stop.<<: invalid_value`; key ganda → `4:3: duplicate_key`; field asing di `versions.json` dan di `x-stackly` → ditolak; billion laughs 256 KiB → `yaml_syntax: repetition limit exceeded` → AC-2
- [x] VAL-002: `official/../x`, `Official/Nats`, `nats`, revision kosong, `1..2` → `invalid_name`; `category: testing` di namespace `official` dan `software` yang beda dengan `versions.json` → ditolak → AC-3
- [x] Himpunan tertutup: `runShell` → `unknown_primitive`, probe `script` dan `native` → `unknown_probe`, format `ini` → `unknown_template_format`, sinyal `kill` dan strategi `kill` → `invalid_value`, sinyal reload `term` → `invalid_value` → AC-4
- [x] Capability: `reload` tanpa `lifecycle.reload` → `capability_unsatisfiable`; `lifecycle.reload` tanpa capability → `reload_without_capability`; `apply: reload` di NATS, `metrics` tanpa endpoint monitoring, `tls`, backup `online`, backup `cold` tanpa `persistence`, dan path backup di luar `managedFilesystem` → `capability_unsatisfiable` → AC-5
- [x] Endpoint: `defaultHost: 0.0.0.0` → `endpoint_not_loopback`; socket `.s.PGSQL.${endpoint.http.port}xy` (16 byte) → `socket_name_too_long`, sedangkan `…}x` (15 byte) → `ok`; socket `../x` dan nama `client-tls` → `invalid_name`; probe ke endpoint `nope` dan probe `tcp` ke endpoint `udp` → `unknown_endpoint`; 17 endpoint → `invalid_value` → AC-6
- [x] Interpolasi: `/bin/sh` dan `"${runtime.executable} -c"` → `literal_executable`; `${secret.authToken}` di `args`, `env`, dan template tidak sensitif → `secret_not_allowed_here`; `"${settings.label}"` di awal arg → `argument_injection_risk`; `cwd` dengan `/../config` → `path_escapes_scope`; `cwd` dengan `${settings.label}` → `token_not_allowed_here`; `${instance.logs}` dan `${instance.temp}` saat tidak diizinkan → `path_not_permitted`; `${unknown.x}` → `unknown_token`; `managed: "${settings.logLevel}"` → `token_not_allowed_here`; env `DYLD_INSERT_LIBRARIES` dan `PATH` → `invalid_name`; `$${HOME}` → `ok` → AC-7
- [x] Dialek settings schema: `$ref`, `oneOf`, `format` → `settings_schema_keyword`; tanpa `maxLength`, enum `-x`, secret dengan `default`, default di atas `maxLength`, field managed di `required`, 65 field, object bersarang, dan field `apply: reload` di `args` → `settings_schema_rule` → AC-8
- [x] Template: `{{ settings.routes }}` dan `#each` pada teks → `template_type` dengan baris dan kolom; variabel loop `settings` → `invalid_name`; blok tidak ditutup dan nesting 5 → `template_syntax`; `"prefix-{{ endpoint.web.address }}"` (berkutip) → `rendered_output_invalid`; tanpa kutip → `ok` (YAML valid) → AC-9
- [x] Timeout: `stopSeconds: 0`, `readinessSeconds: 601`, langkah run 61 s dengan `initSeconds` 60, `attemptTimeoutMilliseconds` 30001 dan 6000 dengan `readinessSeconds` 5, `failureThreshold: 11` → `timeout_out_of_range`; `reloadSeconds` tanpa capability → `reload_without_capability`; `render: [nope]` → `unknown_config_file` → AC-10
- [x] `versions.json`: `fixture` di `official` → `fixture_outside_testing`; `available` tanpa `sha256`, versi `2.16.0` di line `2.15`, ID ganda, artefak `linux` → `versions_rule`; `executables: ["nats"]` → `executable_not_in_artifact` → AC-13
- [x] Digest: satu spasi ditambahkan ke template fake → digest berubah (`e4d4b985…`); `versions.json` diformat ulang, `README.md` diubah, dan file di `tests/` → digest tetap `26836aad…` → AC-12
- [x] Satu run dengan NATS valid dan dua salinan fake rusak → baris `ok official/nats …` lalu satu baris per masalah, stderr `provider check: 2 problem(s) in 2 of 3 package(s)`, exit 1 → AC-14
- [x] `cargo xtask codegen --check` → `19 generated files are up to date`, exit 0. `size` entri NATS di `runtimes/inventory.toml` diubah sementara → `out of date: providers/nats/versions.json`, exit 1. Batas `displayName` di `raw.rs` diubah sementara → `out of date: schemas/provider/provider.v1alpha1.schema.json`, exit 1. Setelah dikembalikan → exit 0 → AC-13, AC-17
- [x] Pembaca JSON Schema scratch atas `schemas/provider/*.json` → manifest 25 object dengan `properties` semuanya `additionalProperties: false` dan 6 peta semuanya `propertyNames`; settings 5 dan 2; versions 3; pattern `metadata.id`, `revision`, `software`, `line`, dan `version` sama dengan aturan domain → AC-17
- [x] `grep -rn 'provisional: F9' --include='*.rs' crates xtask apps fuzz` → 0 baris; `xtask/src/runtime/spike/mod.rs` dan `spike/providers.rs` memuat `// provisional: F36` → AC-13

### Engine lewat API publik (program scratch Rust)

- [x] `read_package(&MacosFilesystem, …)` dan `from_files(…, NameCheck::FolderName)` atas keempat paket → key sama dengan `provider check`; capability `fake` reload, multi_instance, persistence, cold_backup; NATS metrics, multi_instance, persistence, cold_backup; Traefik reload, multi_instance; `fake-command` line compatibility `0.1 → 0.2`. `files()` divalidasi ulang dengan `NameCheck::Skip` di folder bernama digest → key sama. Salinan di `TestRoot` dibaca lewat `FakePlatform` → `mutations()` 0 sebelum dan sesudah, `violations()` 0 → AC-1, AC-3, AC-5
- [x] `SettingsSchema::validate`: Traefik `{}` → `{"logLevel":"ERROR","routes":[]}`; key asing → `unknown_setting`; field managed diisi, route `name` ganda, pattern yang tidak cocok penuh (`api; rm -rf`, `xhttp://…`, host bukan loopback), enum asing, `5.0`, `1e3`, teks dengan newline, 256 untuk `crashExitCode`, tipe salah, secret hilang, dan bukan object → `settings_value` dengan path field; `to_json` menyimpan secret sebagai reference dan validasi ulang memberi nilai yang sama; pattern `a|b` menerima `a` dan `b` saja; keenam kelas VAL-012 muncul → AC-8
- [x] Fake dari plan sampai proses: `init_plan` (3 langkah, deadline 30 s) dijalankan dengan `stackly-fake-server` asli → `token.sha256` sama dengan SHA-256 token, config sensitif berisi token, `Debug` dan JSON plan memuat `<secret:authToken>` tanpa reference; `preflight` atas `FAKE_VERSION` asli (`0.1\n`) → `Compatible`; `run_plan` dengan label penuh metakarakter shell dan delay ready 300 ms → probe `http /ready` sukses setelah 4 percobaan (334 ms); `argv-serve.json` sama persis dengan `spawn.args`; env hanya `HOME`, `PATH`, `TMPDIR`; `/health` 200; `reload_plan` dengan greeting baru → `WriteFile`, SIGHUP, verify readiness, `GET /config` → `reloaded by verify`; stop SIGTERM → exit 0 dalam 10 s; tidak ada file `pwned` → AC-11, AC-15, AC-18
- [x] `fake-command` dari plan sampai proses: `materializeSecret` ke `run/token` (0600), `cleanup` hanya file itu dan dihapus di akhir aksi, `peers.yaml` dengan `primary: true`, init exit 0; readiness `tcp`; `FAKE_SOCKET` sama dengan folder run plus `fake.<port>`; `log_files` di folder logs; health `exec` (`version`) exit 0 dengan timeout 2 s; reload `run` lewat alias `fakeCli` exit 0 dengan verify `none`; stop `command` (`shutdown --port=…`) exit 0 lalu proses utama exit 0 dalam anggaran 30 s → AC-11, AC-14
- [x] Error plan: `reload_plan` NATS → `Unsupported { reload }` dengan rule BR-016; port Auto belum ada → `EndpointUnresolved`; host `0.0.0.0` → `EndpointNotLoopback`; path config 4.4 KB → `ArgumentTooLong` di `lifecycle.start.args[1]`; data dengan `"` → `UnrepresentableValue` yang menyebut `instance.data` tanpa nilainya; path config bukan UTF-8 → `PathNotUtf8`; path dengan `..` atau relatif → `PathEscapesScope`; settings Traefik untuk paket NATS → `Settings`; `materialize` config `natsConf` sensitif dengan secret berisi `$` → `UnrepresentableValue { secret.authToken }` tanpa nilainya, resolver gagal → `SecretUnavailable` → AC-9, AC-11
- [x] Preflight: `0.1` dengan penanda `0.1\n` → `Compatible`; penanda hilang dan penanda 5 KiB (dibaca lewat `read_file_limited`) → `MarkerMissing` dan `MarkerUnreadable`; `0.2` dengan tercatat `0.1` → `MarkerMismatch`; isi tidak cocok pattern → `MarkerMismatch { None, None }`; tanpa catatan dengan runtime `0.2` → `Incompatible`; `fake-command` tanpa catatan → `Unknown`, `0.1` di runtime `0.2` → `Compatible` lewat `lineCompatibility`, `0.2` di runtime `0.1` → `Incompatible` → AC-18
- [x] `from_files` dengan `provider.yaml` alias bersarang 262079 byte → `yaml_syntax` dalam 172 ms (build debug) → AC-2
- [x] Traefik dengan tiga route: `reload_plan` hanya merender `routes`, membuat `config/dynamic`, aksi `none`, verify readiness, deadline 30 s; `routes.yaml` memuat tiga `rule` berkutip dengan backtick dan `\"` ter escape; nol route → `http: {}` → AC-9, AC-14

### Dummy server

- [x] `stackly-fake-server` langsung dengan `env -i`: `version` → `stackly-fake-server 0.1.0`; `init --token-stdin` → `token.sha256` sama dengan `shasum`, `FAKE_VERSION` `0.1\n`; init ke folder tidak kosong → exit 3; `--fail=true` → exit 4 dengan hanya `argv-init.json`; flag asing → exit 2; `--host=0.0.0.0` → exit 2; config dengan key asing → exit 1; `/ready` 503 di 0.3 s lalu 200 di 1.1 s (delay 800 ms); `/health` 200, `/config` greeting, path lain dan `POST /ready` 404; SIGHUP membaca ulang config, config rusak tetap memakai yang lama dengan pesan di stderr; `--ignore-term=true` tetap hidup setelah SIGTERM (`ignoring SIGTERM`), SIGINT → exit 0; spam log 60 baris stdout dan 60 stderr, semuanya 100 byte, genap di stdout dan ganjil di stderr; `--crash-after-ms=300 --crash-exit-code=42` → exit 42; `shutdown` → exit 0 dan server exit 0; `shutdown` tanpa server → exit 1 → AC-15

### Fuzz

- [x] `cargo xtask fuzz build` lalu `cargo xtask fuzz run <target> --time 600` untuk keempat target berdampingan pada `f6e7134` bersih, 11:04:29Z sampai 11:14:33Z, mulai dari seed saja → `provider_manifest` 866478 run (1441/s, RSS 397 MB), `provider_template` 161910 (269/s, 456 MB), `provider_interpolation` 1661477 (2764/s, 630 MB), `provider_settings_schema` 4541860 (7557/s, 496 MB); keempatnya `ran for 600 s without a crash`, `fuzz/artifacts/` kosong. Record evidence 10 menit sebelumnya (`evidence/manual/2026-10-18-fuzz-the-four-provider-targets-for-10-minutes-ea.json`, commit `863022a`) tetap berlaku karena `crates/core/src/provider/` tidak berubah sejak itu → AC-16

### Suite test dan aturan repo

- [x] `cargo test --workspace --locked` → 1091 lulus, 0 gagal, 8 ignored; di antaranya `provider_rules` 27, `provider_packages` 17 (golden plan dan render ketiga paket sama byte demi byte), `provider_no_shell` 2, `fuzz_regressions` 4, `dummy_server` 25, testkit `fake_server` 1, xtask `provider_check` 4 → AC-1 sampai AC-15, AC-17, AC-18
- [x] `cargo xtask check-deps` → 13 crate patuh. `stackly-domain` ditambahkan sementara ke dependency `stackly-fake-server` → `stackly-fake-server depends on no Stackly crate, except stackly-testkit as a dev dependency`, exit 1. `cargo tree` → fake-server hanya `serde_json`, `sha2`, `signal-hook`, dev `stackly-testkit`, dan tidak ada crate workspace yang bergantung padanya → AC-15
- [x] `cargo xtask evidence check` → 17 record manual, baseline valid, exit 0; `cargo deny check` → advisories, bans, licenses, sources ok (RUSTSEC-2024-0449 diabaikan dengan alasan dan pemilik fitur 9); `cargo check --target aarch64-pc-windows-msvc` untuk domain, platform, protocol, core (juga core dengan `json-schema`) → exit 0

### Gagal

- [ ] Tag YAML standar diterima diam diam: `displayName: !!str Fake Provider`, `!!binary`, `metadata: !!map`, dan tag verbatim `!<tag:yaml.org,2002:str>` → `ok` (exit 0). Hanya tag lokal `!custom` yang ditolak. `serde_yaml_ng` menyelesaikan tag dengan handle `!!` menjadi URI penuh dan membuangnya, jadi `Value::Tagged` tidak pernah muncul untuknya. Nilainya tetap melewati decode bertipe (tag tidak mengubah tipe), tetapi teks AC-2 berbunyi "tag YAML … ditolak" → AC-2

## Acceptance-criteria coverage

- AC-1 · `read_package` lewat `MacosFilesystem` dan `FakePlatform` tanpa mutasi; 12 jenis paket kotor lewat `provider check` asli; `.DS_Store`, `tests/`, `README.md`, `LICENSE-NOTICES.md` dilewati
- AC-2 · apiVersion lebih dulu, kind, field asing dengan path, tag lokal, merge key, key ganda, billion laughs 172 ms: terpenuhi. Tag standar `!!` dan verbatim: **tidak ditolak** (lihat Gagal)
- AC-3 · lima ID dan revision salah, folder `nats2`, kategori testing, `software` beda, salinan pinned `NameCheck::Skip`
- AC-4 · enam nilai di luar himpunan tertutup ditolak dengan lokasi
- AC-5 · tujuh aturan pemenuhan dan `provider_revision()` dengan `CapabilitySet` dan `LineCompatibility`
- AC-6 · loopback, socket 15 lawan 16 byte, charset socket, nama endpoint, endpoint tidak ada, probe tcp ke udp, 17 endpoint; `EndpointNotLoopback` saat plan
- AC-7 · setiap baris matriks posisi yang diuji ditolak dengan kodenya; `$${HOME}` menjadi `${HOME}` literal dan `$` lain tetap
- AC-8 · dialek lewat salinan rusak; nilai lewat `validate` dengan path field; kelas VAL-012
- AC-9 · cek statis dengan baris dan kolom, `rendered_output_invalid`, `UnrepresentableValue` tanpa nilai, golden lewat test; batas 1 MiB lewat unit test `template` di suite workspace
- AC-10 · batas dan sumber timeout lewat salinan rusak; timeout dan exit code di plan fake dan `fake-command` sesuai tabel sumber
- AC-11 · plan dijalankan tanpa shell dengan argv persis; env tertutup; secret tersamar; seluruh daftar `PlanError` yang relevan terpicu
- AC-12 · digest berubah karena template, tetap untuk `versions.json`, `README.md`, `tests/`
- AC-13 · generator dan `--check` dengan inventory yang diubah; aturan `versions.json`; tidak ada penanda F9
- AC-14 · tiga baris `ok`, `fake-command` lolos dan menjalankan primitive sisanya, salinan rusak satu baris per masalah
- AC-15 · semua perilaku dummy server lewat CLI dan lewat plan; aturan `check-deps` terbukti menolak
- AC-16 · empat target 600 s tanpa temuan pada HEAD sekarang, ditambah record evidence sebelumnya
- AC-17 · tiga file schema, object tertutup, pattern domain, drift dari perubahan tipe mentah tertangkap
- AC-18 · sembilan kombinasi preflight, termasuk pembacaan penanda asli

## Catatan untuk rekonsiliasi spec (bukan kegagalan)

- Contoh di *Critical test scenarios* keliru: `.s.PGSQL.${endpoint.client.port}x` adalah 15 byte (diterima, sesuai AC-6); 16 byte perlu `…}xy`. `prefix-{{ endpoint.web.address }}` hanya tidak valid di dalam kutip; tanpa kutip hasilnya YAML valid.
- `StopPlan::Command.run` dan `ReloadAction::Run` adalah `Box<RunSpec>` (clippy `large_enum_variant`), `RenderedContent` membawa `config_file`, dan enum plan sengaja tidak `#[non_exhaustive]` supaya executor wajib menangani setiap langkah.
- `expectStatus` diserialisasi sebagai `{ "start", "end" }` (bentuk serde untuk `RangeInclusive`).
- JSON Schema: "setiap object `additionalProperties: false`" dibaca sebagai setiap object dengan `properties`; peta (endpoints, configFiles, env) memakai `propertyNames`.

## Catatan untuk `/check review`

- Issue berantai yang menyesatkan: setelah satu config file ditolak (format `ini`, atau destination ke folder yang tidak diizinkan), langkah `renderConfig` yang merujuknya ikut dilaporkan `unknown_config_file: configFiles has no …` padahal config itu ada; setelah settings schema ditolak, template Traefik ikut dilaporkan `#if needs a boolean or a list, and settings.routes is neither`.
- `run_plan` menerima path data yang bukan UTF-8 kalau path itu hanya dipakai sebagai `cwd`, sementara `init_plan` dengan context yang sama mengembalikan `PathNotUtf8`. Teks *Execution plan* menyebut context dengan path bukan UTF-8 menghasilkan `PathNotUtf8`.
- Paket dengan 33 template memakai kode `file_too_large` untuk jumlah template, bukan untuk ukuran.
- Pesan probe tcp ke endpoint udp berbunyi "tcp probes cannot use a metricsudp endpoint of that transport" (nama endpoint ada di posisi jenis transport).
