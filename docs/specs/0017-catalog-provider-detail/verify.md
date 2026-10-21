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

## Milestone 3: Layar Catalog

Prasyarat untuk langkah yang memakai daemon asli: `cargo build -p stacklyd -p stackly-fake-server` dan `cargo build -p stacklyd --example skeleton_seed`. Langkah snapshot dan test view model berjalan dari fixture di `protocols/fixtures/catalog/` dan `protocols/fixtures/ipc/`, tanpa daemon.

### UI / manual

Langkah ini milik user: tidak ada yang mengubah setelan sistem, tetapi Differentiate Without Color, Increase Contrast, Reduce Transparency, dan VoiceOver hanya bisa dinyalakan user. Lead memasukkannya ke `docs/manual.md`. Hasilnya dicatat `native` dengan status `NOT_RUN` sampai dijalankan.

- [ ] Buat catalog dev (`cargo xtask catalog build --out $STACKLY_ROOT/catalog`), jalankan `STACKLY_ROOT=$(mktemp -d) STACKLY_CATALOG_DIR=<folder itu> cargo run -q -p stacklyd`, buka app Debug (`cargo xtask build-app`), pilih Catalog (⌘4) → judul "Catalog", subjudul "Development catalog · updated {tanggal}", enam kartu planned (Pg, My, Na, Ch, Vk, Tr) redup tanpa opacity, picker bersegmen All, Databases, Messaging, Analytics, Cache, Proxy, dan tidak ada tombol Get, New Instance, atau Import Provider → AC-10
- [ ] Ketik "post" di field "Search providers" lalu "POST", lalu "postgresql" → hanya kartu PostgreSQL tersisa, pilihan pindah ke kartu itu, dan panel detail menulis catatan planned "This provider isn’t available yet. It will appear here once packaging and tests pass." tanpa matrix, capability, atau tombol → AC-10
- [ ] Ketik "zzz" → "No Results for “zzz”", pesan "No providers match “zzz”.", dan tombol Clear Filters mengosongkan pencarian; pilih Messaging tanpa entri → pesan yang sama dengan nama segmen; pilih All → semua kartu kembali → AC-10
- [ ] Tekan ⌘F dari kartu mana pun → field pencarian di toolbar mendapat fokus, dan menekan ⌘F lagi saat sudah fokus tidak mengubah teksnya (Edit > Find > Find… aktif di Catalog, nonaktif di Overview) → AC-10, spec 0069 AC-10
- [ ] Pindah ke Instances lalu kembali ke Catalog, tutup dan buka jendela → pilihan kartu yang sama tetap terpilih; filter dan teks pencarian kembali kosong → AC-10
- [ ] Dengan catalog fixture (`STACKLY_CATALOG_DIR` ke folder dengan `official/nats` dan entri fixture `linuxonly`, `intelonly`, `blockedhost`, satu paket rusak, lewat `CatalogBuilder` atau salinan `crates/core/tests/fixtures/catalog/`): kartu NATS "Available", panel detail menulis matrix versi (macOS Unverified, Linux Unsupported, Windows Planned) dengan catatan alasan di bawahnya, tiga baris dasar dan delapan capability ("Reload without restart · not supported"), baris License, Source, Download, Disk ("Not listed"), Requires ("macOS 13.0 or later on Apple Silicon"), Dependencies ("None declared"), Publisher, Revision, dan Trust; kartu `intelonly` "Unsupported" dengan baris alasan; `blockedhost` "Blocked: …"; paket rusak "Invalid" dengan banner "This provider failed validation" dan kode `manifest.invalid`; banner di atas grid "{n} providers failed validation." → AC-10
- [ ] Perkecil jendela ke 900 × 600 dengan inspector terbuka → grid satu kolom, picker kategori menjadi menu, tidak ada teks terpotong di kartu atau panel, inspector paling lebar 280 pt; matrix versi berubah menjadi daftar per OS → AC-10, AC-11
- [ ] Hentikan `stacklyd` → banner jendela "Can’t connect to stacklyd." dan halaman Catalog menampilkan state tunggu; mulai lagi → catalog kembali tanpa memuat ulang app → AC-10
- [ ] Nyalakan Full Keyboard Access (System Settings > Keyboard), lalu hanya dengan keyboard: Tab ke field pencarian, picker kategori, setiap kartu (Return atau Space memilih), dan isi panel detail termasuk link lisensi → semua dapat dicapai dan fokus terlihat → AC-11
- [ ] Nyalakan VoiceOver (⌘F5): kartu dibaca "NATS Server, Messaging, Available", sel matrix "macOS, Unverified: Not tested on this Mac yet.", capability "Reload without restart, not supported" dan "Metrics, supported", tile tidak dibaca → AC-11
- [ ] Nyalakan Differentiate Without Color → badge status menulis simbol tone di depan label dan tidak ada status yang hanya berbeda warna; nyalakan Increase Contrast dan Reduce Transparency, ulangi di light dan dark → semua teks terbaca, kartu planned tidak redup → AC-11
- [ ] Atur jam Mac atau pakai catalog dengan `published_at` lebih dari 180 hari → banner "Showing the last known state from {tanggal}." dengan "the development catalog last updated {tanggal}." (penulisan kecil "the" ditandai di laporan, lihat catatan) dan tidak ada teks "latest", "up to date", atau "newer catalog" di layar → AC-8, AC-10

### Commands

- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter "CatalogAppearanceTests|CatalogCopyTests"` → label, tone, simbol, dan detail setiap `SupportStatus` dan `CatalogEntryState` termasuk `unrecognized`, setiap teks alasan, nama OS dan arsitektur, nilai asing, ukuran desimal, dan subjudul "Official · updated Oct 20, 2026"; `CatalogAppearance.swift` tanpa `default:` → AC-8, AC-12
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter "CatalogFilterTests|CatalogStoreTests|ProviderFactsTests"` → filter kategori, pencarian tanpa huruf besar dan diakritik, tidak ada segmen Testing, pilihan default dan pilihan yang tersaring, urutan daemon dan `CatalogInfo` setelah snapshot, store `unsupported` untuk daemon lama, cache detail per instance daemon, provider, dan revision (dibuang saat snapshot baru), Retry setelah gagal, dan baris License, Source, Download, Disk, Requires, Dependencies yang hanya membaca artefak host → AC-9, AC-10
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter "CatalogViewsTests|CatalogPageStateTests"` → catatan di bawah matrix, daftar capability dengan tiga baris dasar, label VoiceOver kartu dan sel, banner issue paling banyak lima, dan state halaman (loading, unsupported, failed, empty, no results dengan kutipan query, stale `.outdated` dengan `publishedAt` dan source, partial dengan ID entri invalid) → AC-8, AC-10, AC-11
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter CatalogSnapshotTests` → 10 test lulus tanpa `STACKLY_SNAPSHOT_RECORD`: available, unsupported, blocked, planned, invalid, no results, empty, outdated, failed, dan daemon tanpa method, masing masing light, dark, hc-light, hc-dark, narrow, dan Reduce Transparency identik dengan light → AC-10, AC-11
- [ ] `cargo xtask ui-test --only <kelas menu>` (XCUITest menu spec 0068 AC-16) → item Edit > Find > Find… aktif di Catalog dan nonaktif di Overview untuk setiap skenario yang memuat catalog. Menunggu: tetap `NOT_RUN` sampai user menyalakan Automation Mode (`docs/manual.md` bagian K), exit 2 selama itu → AC-10, spec 0069 AC-10
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter "CommandMapTests|CommandSourceRuleTests"` → lulus: baris `find` sudah `built` di `CommandMap` (scope `.page(semua bagian)`, requirement `.search`, tercatat di tabel "Commands and shortcuts" `design.md`), dan halaman Catalog tidak memakai `keyboardShortcut` atau `CommandGroup` → AC-10
- [ ] `cargo xtask snapshot review` → hanya PNG `CatalogSnapshotTests` yang berubah dibanding main; tidak ada PNG `ShellSnapshotTests`, `DiagnosticsSnapshotTests`, atau `GallerySnapshotTests` yang berubah → AC-10
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter SourceRuleTests` → 21 test lulus, termasuk "every page outside the gallery has a snapshot suite" untuk `CatalogPage` → AC-10
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI` (setelah dua `cargo build` di atas) → seluruh suite lulus
- [ ] `swift test --package-path clients/swift` → lulus (`ProviderSummary` menjadi `Identifiable` di `StacklyClient`)
- [ ] `cargo xtask build-app` → build sukses
- [ ] `xcrun swift-format lint --strict` pada file Swift yang berubah → bersih
- [ ] `git status --short` setelah semua langkah → bersih

### Acceptance-criteria coverage

- AC-9: bagian GUI (view model Swift membaca fixture `protocols/fixtures/catalog/` yang sama dengan golden CLI: status per sel dan teks ukuran). Golden CLI dan perintah `stackly provider` ada di milestone 2.
- AC-10: test filter, store, state halaman, snapshot, dan langkah UI manual. Subjudul, pencarian, picker, grid, panel detail, matrix, capability, baris key value, state planned dan invalid, hasil kosong, dan `ViewState.resolve`. Perintah Find lewat `.publishesCommands(.page(.catalog), …)`.
- AC-11: label VoiceOver di test view model, tata letak tanpa pemotongan di snapshot narrow, dan langkah manual keyboard, VoiceOver, Differentiate Without Color, Increase Contrast. Kontras AA diperiksa snapshot hc-light dan hc-dark, tetapi pengukuran kontras dan Differentiate Without Color (yang tidak dirender snapshot, spec 0068 AC-5) tetap langkah manual.
- AC-12: `CatalogAppearanceTests`, tabel di `apps/macos/design.md`, dan teks nilai asing di `CatalogCopyTests`.
- AC-13: milestone 1.
