# Verify: Provider SDK & test harness · spec 0036 · updated 2026-11-11

_Langkah diturunkan dari acceptance criteria spec 0036. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27, dengan `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`. Probe yang mengubah file sementara selalu dikembalikan sesudahnya (`git status` bersih lagi)._

## Milestone 1: Validator dan kebijakan

Bagian ini baru mencakup Build plan langkah 1 (`core::provider::check` dan `core::provider::scenarios`). Langkah 2 sampai 5 milestone ini menambah bagiannya sendiri di bawah.

### UI / manual

- [ ] Fuzz 10 menit (langkah manual user, AC-3): `cargo xtask fuzz run provider_scenarios --time 600` → baris terakhir `fuzz run: provider_scenarios ran for 600 s without a crash`, tidak ada file baru di `fuzz/artifacts/provider_scenarios/`, dan `stat::peak_rss_mb` jauh di bawah batas libFuzzer 2048 MB. Toolchain `nightly-2026-10-17` dan cargo-fuzz 0.13.2 sudah terpasang di Mac ini; smoke run 30 detik saat build lulus (137 ribu eksekusi, peak RSS 323 MB).

### Commands

- [ ] `cargo test -p stackly-core --lib provider::check` → 3 test lulus: `SUPPORTED_API_VERSIONS` berisi tepat `stackly/v1alpha1` berstatus `current`; pesan `apiVersion "stackly/v2" is not supported; this Stackly supports: stackly/v1alpha1` dan `<not a string>`; tanda kutip dan karakter kontrol di nilai di escape, nilai di atas 128 karakter dipotong dengan `...` → AC-1
- [ ] `cargo test -p stackly-core --lib provider::scenarios` → 6 test lulus: urutan dan default (`expect: ready`, settings kosong), `schema` lain berhenti sebelum sisa dokumen di decode, nama `InstanceName` paling banyak 24 karakter dan unik, 0 atau 17 skenario ditolak, field asing, key ganda, tag, merge key, `expect` asing, dan settings bukan mapping ditolak, skenario tersirat `default` → AC-3
- [ ] `cargo test -p stackly-core --test provider_check` → 8 test lulus, semuanya lewat `FakePlatform` tanpa satu pun mutasi filesystem: paket `fake`, `nats`, `postgresql`, `traefik` `publishable` dengan skenario tersirat `default` dan key yang sama dengan `from_files`; `fake-command` `loadable` tetapi tidak `publishable` dengan satu finding `authoring` `token_not_allowed_here` yang sama dengan `init_port_issues`; `apiVersion: stackly/v2` (dengan settings schema dan template yang juga rusak) memberi tepat satu finding `package` `unsupported_api_version` di `provider.yaml` `apiVersion`, sama dengan error `from_files`; `apiVersion` bukan string dan yang hilang; paket yang tidak terbaca atau tidak valid hanya punya finding `package`; skenario valid tidak mengubah `manifest_sha256` dan tidak dibaca `read_package`; setiap aturan skenario yang dilanggar menjadi finding `scenarios` di lokasinya; file 65 KiB, symlink, FIFO, folder, `tests/` symlink, dan bukan UTF-8 ditolak → AC-1 (bagian core), AC-2, AC-3
- [ ] `cargo test -p stackly-core --test provider_rules another_api_version_or_kind_stops_before_the_rest_is_read` → lulus tanpa perubahan: satu issue `unsupported_api_version` → AC-1
- [ ] `cargo test -p stackly-core --test fuzz_regressions` dan `cargo test -p stackly-core --lib provider::fuzzing` → lulus; `provider_scenarios` ada di daftar target dan dua seed nya (`fixture.fake.yaml`, `fixture.invalid.yaml`) melewati harness → AC-3
- [ ] `cargo xtask fuzz list` → memuat `provider_scenarios`; `cargo xtask fuzz build provider_scenarios` → exit 0 dengan toolchain pin; `cargo xtask fuzz seed` lalu `git status --short fuzz/seeds` → bersih (seed `provider_scenarios` sudah ter-commit; seed dari `providers/example/tests/scenarios.yaml` dan `providers/fake/tests/scenarios.yaml` ikut otomatis begitu file itu ada) → AC-3
- [ ] `cargo xtask provider check` → empat baris `ok` tidak berubah (format lama tetap sampai langkah 2) → AC-2
- [ ] `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` dan `cargo xtask check-deps` → lulus; `check` dan `scenarios` hanya memakai port `Filesystem` → AC-2

### Acceptance-criteria coverage

- AC-1 · bagian core: tabel, pesan, satu finding di `check_package` dan `from_files` (jadi juga di muat catalog yang memakai `from_files`); `provider.validate` menunggu langkah 3, `cargo xtask provider check` langkah 2, catalog `invalid` lewat test daemon langkah 3, harness langkah 12
- AC-2 · lengkap di core; dipakai `cargo xtask provider check` (langkah 2) dan `provider.validate` (langkah 3)
- AC-3 · lengkap kecuali fuzz 10 menit (langkah manual di atas); seed dari `providers/example` menunggu langkah 6
- AC-4 sampai AC-16 · belum, milik langkah 2 dan seterusnya
