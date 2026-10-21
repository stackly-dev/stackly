# Verify: Install runtime managed · spec 0018 · updated 2026-10-21

_Langkah diturunkan dari acceptance criteria spec 0018. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Bagian Milestone 1 ditulis sesi milestone itu._

## Milestone 2: Install pipeline in core

### UI / manual

- [ ] Buka `crates/storage/migrations/0005_runtime_install.sql` → empat `ADD COLUMN` di `artifacts` (`catalog_id`, `build`, `format`, `size_bytes`, semuanya nullable) dan tabel `compatibility_records` dengan CHECK `provider`/`provider_revision`/`manifest_sha256` terisi tepat saat `scope = 'provider'` → AC-13, AC-15
- [ ] Buka `crates/storage/tests/fixtures/registry-v5.sql` → baris `artifacts` yang membawa `catalog_id`, `build`, `format`, `size_bytes`, dan dua baris `compatibility_records` (satu per scope); file itu beku dan hash nya ada di `FIXTURE_SHA256` → AC-13, AC-15

### Commands

- [ ] `cargo test -p stackly-domain` → test `Runtime::committed` (installed di revision 1 dengan `installed_at` dan `last_verified_at`, runtime external ditolak), `RuntimeSelector` (`stable`, `sha256:` huruf kecil 64 hex, versi dan line, teks tidak sah), `TeamId`, dan `CompatibilityRecordRow` lulus → AC-1, AC-13, AC-15
- [ ] `cargo test -p stackly-storage` → fixture v1 sampai v5 tidak berubah, sidik jari skema versi 5 cocok, migrasi v4 ke v5 menjaga semua baris, conformance `SqliteRegistry` memuat `artifact_install_details` → AC-13, AC-15
- [ ] `cargo test -p stackly-testkit` → conformance `FakeRegistry` memuat skenario yang sama dengan `SqliteRegistry` → AC-13, AC-15
- [ ] `cargo test -p stackly-platform` → `HttpsUrl` menolak `http`, userinfo, host kosong, spasi, dan panjang lebih dari 2048 → AC-5

### Acceptance-criteria coverage

- AC-1 · bagian `RuntimeSelector` di domain (langkah 6); resolusi atas catalog menunggu langkah 9
- AC-13 · bagian registry dan `Runtime::committed` (langkah 6); rename dan commit menunggu langkah 9
- AC-15 · bagian penyimpanan `compatibility_records` (langkah 6); `support_status` menunggu langkah 9
- AC-3, AC-4, AC-5, AC-6 · langkah 7; AC-10, AC-11, AC-12 · langkah 8; AC-1, AC-2, AC-13, AC-14, AC-15 · langkah 9 (diisi saat langkah itu landed)
