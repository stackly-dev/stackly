# Verify: Catalog dan provider detail · spec 0017 · updated 2026-10-20

_Langkah diturunkan dari acceptance criteria spec 0017. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27, pada commit bersih. Setiap milestone punya bagiannya sendiri, ditambahkan saat milestone itu dibangun. Setiap perubahan sementara pada file repo dikembalikan dengan `git checkout`, dan `git status` bersih sesudahnya._

## Milestone 1: Catalog dari repo sampai daemon

### UI / manual

Tidak ada. Milestone ini belum punya layar: CLI `provider` ada di milestone 2 dan layar Catalog di milestone 3. "Menjalankan app" di sini berarti `cargo xtask catalog`, binary `stacklyd` asli, dan client IPC.

### Commands

Siapkan satu root sementara untuk langkah daemon: `export STACKLY_ROOT=$(mktemp -d)`.

- [ ] `cargo xtask catalog check` → `ok catalog revision 1 d8d059cd4f656c87eb63ec50e01752680addb31ef39287e3fb6343d758a86b30`, digest yang sama dengan `providers/catalog-revisions.json` → AC-1
- [ ] `cargo xtask catalog build --out $STACKLY_ROOT/catalog`, lalu sekali lagi ke `$STACKLY_ROOT/catalog2` dan `diff -r` keduanya → `catalog.json` berisi enam entri planned (urut ID) dengan revision 1, kedua folder sama byte demi byte, tanpa `README.md` atau `tests/` → AC-1
- [ ] Buat file biasa di `$STACKLY_ROOT/other/notes.txt`, jalankan `cargo xtask catalog build --out $STACKLY_ROOT/other` → exit 1, folder itu dan isinya tetap ada → AC-1
- [ ] Ubah sementara `published_at` di `providers/catalog.toml` ke tanggal besok, jalankan `cargo xtask catalog check` → exit 1 dengan satu baris tentang `published_at`; kembalikan → AC-1
- [ ] Ubah sementara `description` satu entri planned tanpa menaikkan `revision`, jalankan `cargo xtask catalog check` → exit 1 karena digest isi berbeda dengan ledger untuk revision 1; kembalikan → AC-1
- [ ] Ubah sementara `id` sebuah entri planned menjadi `community/x`, jalankan `cargo xtask catalog check` → exit 1 dengan baris namespace; kembalikan → AC-1
- [ ] `STACKLY_CATALOG_DIR=relative/catalog cargo run -q -p stacklyd` → exit 1, satu baris error `STACKLY_CATALOG_DIR "relative/catalog" is not an absolute path to a folder`. Ulangi dengan nilai kosong, path yang tidak ada, dan path ke `$STACKLY_ROOT/catalog/catalog.json` → exit 1 dengan pesan yang sama berisi nilai itu → AC-2
- [ ] `cargo run -q -p stacklyd` tanpa `STACKLY_CATALOG_DIR`, lalu dari terminal lain (dengan `STACKLY_ROOT` yang sama) `cargo run -q -p stackly-cli -- daemon status --json` → daemon tetap jalan, status `degraded`, subsystem `catalog` `unavailable` dengan detail `no catalog: not inside an app bundle and STACKLY_CATALOG_DIR is unset`; hentikan daemon → AC-2
- [ ] `STACKLY_CATALOG_DIR=$STACKLY_ROOT/catalog cargo run -q -p stacklyd` → log `provider catalog loaded` dengan `catalog_state` `loaded`, revision 1, enam entri, nol invalid; `daemon status --json` → `catalog` `ok` dan status `ready` → AC-2
- [ ] Dengan daemon itu, panggil `catalog.list_providers` dan `catalog.get_provider` lewat program scratch kecil di atas `stackly_ipc::Client` (CLI `provider` baru ada di milestone 2; program tidak di commit) → list berisi enam entri planned dalam urutan tampil (database, messaging, analytics, cache, proxy, lalu nama), `CatalogInfo.source` dan `trust` keduanya `development`, `publishedAt` `2026-10-20T01:26:00.000Z`, `contentSha256` sama dengan ledger, `outdated` false, `host` `macos` `aarch64` → AC-4, AC-8
- [ ] Dari program yang sama: `get_provider` `official/postgres` → `catalog.provider_not_found` dengan hint "run stackly provider list to see the catalog"; `postgresql` → pesan `no provider postgresql in the catalog; did you mean official/postgresql` dan hint "use the full provider ID, including its namespace"; `Official/NATS` → `request.invalid_params` dengan field `provider`; `official/mysql` → detail planned tanpa capability dan tanpa versi → AC-5
- [ ] Ganti isi `$STACKLY_ROOT/catalog/catalog.json` dengan `{ "schema": "stackly.catalog/v2" }`, start ulang daemon → `catalog.list_providers` dijawab `catalog.unavailable` reason `unsupported_schema` dengan hint "update Stackly, this catalog needs a newer version", subsystem `catalog` `unavailable` → AC-3
- [ ] `cargo test -p stackly-core --features test-support catalog` → semua test catalog lulus: load, isolasi entri, batas, urutan tampil, `outdated` 179 dan 181 hari, aturan status, gate, dan `CatalogBuilder` → AC-2, AC-3, AC-6, AC-7, AC-8
- [ ] `cargo test -p stacklyd --test server catalog` → sembilan test lulus, termasuk entri invalid dengan subsystem `degraded`, `daemon.starting` sebelum catalog dimuat, recovery mode, dan anggaran frame pada catalog dengan semua batas penuh → AC-2, AC-3, AC-4, AC-5, AC-7
- [ ] `cargo test -p stacklyd --test catalog` → empat test binary lulus: nilai `STACKLY_CATALOG_DIR` yang salah, tanpa catalog, catalog kosong default `SpawnedDaemon`, dan folder hasil `CatalogBuilder` → AC-2, AC-4
- [ ] `cargo test -p stacklyd --lib map::catalog` → pemetaan per varian, keempat kode error dengan hint nya, dan balasan list serta enam detail yang sama persis dengan `protocols/fixtures/catalog/` → AC-7, AC-13
- [ ] `cargo test -p stackly-protocol` → tabel kode `catalog`, test hint (paling banyak 200 karakter, tanpa `{`), validasi `GetProviderParams`, dan fixture catalog lulus → AC-13
- [ ] `cargo test -p xtask catalog` → test build, ledger, `--update`, dan `--amend` lulus → AC-1
- [ ] `cargo xtask codegen --check` → `19 generated files are up to date`, dan `protocols/contract-versions.json` mencatat versi 1.6 → AC-13
- [ ] `swift test --package-path clients/swift --filter CatalogContractTests` → tujuh test lulus dari fixture yang sama → AC-13
- [ ] `cargo xtask check-deps` dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → bersih → aturan repo
- [ ] `git status --short` setelah semua langkah → bersih

### Acceptance-criteria coverage

- AC-1: langkah `catalog check`, `catalog build`, dan test xtask. Klausa `cargo xtask build-app` menunggu milestone 4 langkah 11.
- AC-2: langkah `STACKLY_CATALOG_DIR`, daemon tanpa catalog, test load tanpa efek samping, dan test binary. Cabang lokasi `Bundled` menunggu milestone 4 langkah 11 (`// provisional: F17 step 11`).
- AC-3: langkah schema lain, test load core, dan test server untuk entri invalid serta index yang tidak bisa dipakai.
- AC-4: langkah list lewat client IPC dan test anggaran frame.
- AC-5: langkah `get_provider`, test capability NATS dan fake, dan test saran ID lengkap.
- AC-6: test status core.
- AC-7: test gate core, `CatalogVersion.install` di detail, dan pemetaan `catalog.not_installable`. Penolakan ujung ke ujung lewat `runtime.install` menunggu fitur 18 (milestone 4 langkah 12).
- AC-8: field `CatalogInfo` dan test `outdated` core. Bagian GUI dan CLI menunggu milestone 2 dan 3.
- AC-9 sampai AC-12: milestone 2 dan 3, tidak dicakup bagian ini.
- AC-13: tabel kode, test pemetaan per varian di `stacklyd`, fixture Rust dan Swift, dan `codegen --check`.
