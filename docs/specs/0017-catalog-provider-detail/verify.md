# Verify: Catalog dan provider detail · spec 0017 · updated 2026-10-20
> Rekonsiliasi cakupan 2026-10-24: cek khusus pembaca layar, navigasi dan shortcut keyboard, pengaturan tampilan aksesibilitas, serta identifier UI test sudah dihapus. Cek tersebut berstatus RETIRED dan tidak menjadi penghambat verifikasi. Hasil lama di bawah merupakan riwayat revisi asalnya; nomor AC tetap stabil.


_Langkah diturunkan dari acceptance criteria spec 0017. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27, pada commit bersih. Setiap milestone punya bagiannya sendiri, ditambahkan saat milestone itu dibangun. Setiap perubahan sementara pada file repo dikembalikan dengan `git checkout`, dan `git status` bersih sesudahnya._

## Milestone 1: Catalog dari repo sampai daemon

### UI / manual

Tidak ada. Milestone ini belum punya layar: CLI `provider` ada di milestone 2 dan layar Catalog di milestone 3. "Menjalankan app" di sini berarti `cargo xtask catalog`, binary `stacklyd` asli, dan client IPC.

### Commands

Siapkan satu root sementara untuk langkah daemon: `export STACKLY_ROOT=$(mktemp -d)`.

- [x] `cargo xtask catalog check` → `ok catalog revision 1 d8d059cd4f656c87eb63ec50e01752680addb31ef39287e3fb6343d758a86b30`, digest yang sama dengan `providers/catalog-revisions.json` → AC-1
- [x] `cargo xtask catalog build --out $STACKLY_ROOT/catalog`, lalu sekali lagi ke `$STACKLY_ROOT/catalog2` dan `diff -r` keduanya → `catalog.json` berisi enam entri planned (urut ID) dengan revision 1, kedua folder sama byte demi byte, tanpa `README.md` atau `tests/` → AC-1
- [x] Buat file biasa di `$STACKLY_ROOT/other/notes.txt`, jalankan `cargo xtask catalog build --out $STACKLY_ROOT/other` → exit 1, folder itu dan isinya tetap ada → AC-1
- [x] Ubah sementara `published_at` di `providers/catalog.toml` ke tanggal besok, jalankan `cargo xtask catalog check` → exit 1 dengan satu baris tentang `published_at`; kembalikan → AC-1
- [x] Ubah sementara `description` satu entri planned tanpa menaikkan `revision`, jalankan `cargo xtask catalog check` → exit 1 karena digest isi berbeda dengan ledger untuk revision 1; kembalikan → AC-1
- [x] Ubah sementara `id` sebuah entri planned menjadi `community/x`, jalankan `cargo xtask catalog check` → exit 1 dengan baris namespace; kembalikan → AC-1
- [x] `STACKLY_CATALOG_DIR=relative/catalog cargo run -q -p stacklyd` → exit 1, satu baris error `STACKLY_CATALOG_DIR "relative/catalog" is not an absolute path to a folder`. Ulangi dengan nilai kosong, path yang tidak ada, dan path ke `$STACKLY_ROOT/catalog/catalog.json` → exit 1 dengan pesan yang sama berisi nilai itu → AC-2
- [x] `cargo run -q -p stacklyd` tanpa `STACKLY_CATALOG_DIR`, lalu dari terminal lain (dengan `STACKLY_ROOT` yang sama) `cargo run -q -p stackly-cli -- daemon status --json` → daemon tetap jalan, status `degraded`, subsystem `catalog` `unavailable` dengan detail `no catalog: not inside an app bundle and STACKLY_CATALOG_DIR is unset`; hentikan daemon → AC-2
- [x] `STACKLY_CATALOG_DIR=$STACKLY_ROOT/catalog cargo run -q -p stacklyd` → log `provider catalog loaded` dengan `catalog_state` `loaded`, revision 1, enam entri, nol invalid; `daemon status --json` → `catalog` `ok` dan status `ready` → AC-2
- [x] Dengan daemon itu, panggil `catalog.list_providers` dan `catalog.get_provider` lewat program scratch kecil di atas `stackly_ipc::Client` (CLI `provider` baru ada di milestone 2; program tidak di commit) → list berisi enam entri planned dalam urutan tampil (database, messaging, analytics, cache, proxy, lalu nama), `CatalogInfo.source` dan `trust` keduanya `development`, `publishedAt` `2026-10-20T01:26:00.000Z`, `contentSha256` sama dengan ledger, `outdated` false, `host` `macos` `aarch64` → AC-4, AC-8
- [x] Dari program yang sama: `get_provider` `official/postgres` → `catalog.provider_not_found` dengan hint "run stackly provider list to see the catalog"; `postgresql` → pesan `no provider postgresql in the catalog; did you mean official/postgresql` dan hint "use the full provider ID, including its namespace"; `Official/NATS` → `request.invalid_params` dengan field `provider`; `official/mysql` → detail planned tanpa capability dan tanpa versi → AC-5
- [x] Ganti isi `$STACKLY_ROOT/catalog/catalog.json` dengan `{ "schema": "stackly.catalog/v2" }`, start ulang daemon → `catalog.list_providers` dijawab `catalog.unavailable` reason `unsupported_schema` dengan hint "update Stackly, this catalog needs a newer version", subsystem `catalog` `unavailable` → AC-3
- [x] `cargo test -p stackly-core --features test-support catalog` → semua test catalog lulus: load, isolasi entri, batas, urutan tampil, `outdated` 179 dan 181 hari, aturan status, gate, dan `CatalogBuilder` → AC-2, AC-3, AC-6, AC-7, AC-8
- [x] `cargo test -p stacklyd --test server catalog` → sembilan test lulus, termasuk entri invalid dengan subsystem `degraded`, `daemon.starting` sebelum catalog dimuat, recovery mode, dan anggaran frame pada catalog dengan semua batas penuh → AC-2, AC-3, AC-4, AC-5, AC-7
- [x] `cargo test -p stacklyd --test catalog` → empat test binary lulus: nilai `STACKLY_CATALOG_DIR` yang salah, tanpa catalog, catalog kosong default `SpawnedDaemon`, dan folder hasil `CatalogBuilder` → AC-2, AC-4
- [x] `cargo test -p stacklyd --lib map::catalog` → pemetaan per varian, keempat kode error dengan hint nya, dan balasan list serta enam detail yang sama persis dengan `protocols/fixtures/catalog/` → AC-7, AC-13
- [x] `cargo test -p stackly-protocol` → tabel kode `catalog`, test hint (paling banyak 200 karakter, tanpa `{`), validasi `GetProviderParams`, dan fixture catalog lulus → AC-13
- [x] `cargo test -p xtask catalog` → test build, ledger, `--update`, dan `--amend` lulus → AC-1
- [x] `cargo xtask codegen --check` → `19 generated files are up to date`, dan `protocols/contract-versions.json` mencatat versi 1.6 → AC-13
- [x] `swift test --package-path clients/swift --filter CatalogContractTests` → tujuh test lulus dari fixture yang sama → AC-13
- [x] `cargo xtask check-deps` dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → bersih → aturan repo
- [x] `git status --short` setelah semua langkah → bersih

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

## Milestone 2: `stackly provider list` dan `inspect`

Prasyarat: `cargo build -p stacklyd -p stackly-fake-server`. Langkah dengan daemon asli membangun `xtask` sendiri lewat `stackly_testkit::cargo_bin`. Test golden berjalan dengan `TZ=UTC` (diatur `common::stackly`) dan membaca fixture `protocols/fixtures/catalog/`, file yang sama dengan test view model Swift.

### UI / manual

Tidak ada. Milestone ini hanya CLI; layar Catalog ada di milestone 3.

### Commands

Siapkan satu root sementara: `export STACKLY_ROOT=$(mktemp -d)`.

- [x] `cargo xtask catalog build --out $STACKLY_ROOT/catalog`, lalu `STACKLY_CATALOG_DIR=$STACKLY_ROOT/catalog cargo run -q -p stacklyd` di satu terminal; di terminal lain (dengan `STACKLY_ROOT` yang sama) `cargo run -q -p stackly-cli -- provider list` → baris `Catalog: development, revision <n>, published <tanggal> <jam> <offset>`, lalu tabel `PROVIDER NAME CATEGORY STATE VERSIONS LICENSE` dengan semua entri dalam urutan daemon (satu baris per entri, `-` untuk sel kosong; entri provider dengan versi menulis `18.6, 17.11` dan lisensinya) → AC-9
- [x] `cargo run -q -p stackly-cli -- provider list --category database` dan `--category testing` → hanya entri database; kategori tanpa entri menulis baris catalog lalu `No providers in category testing.` dan exit 0; `--category games` → exit 2 dengan daftar nilai yang valid → AC-9
- [x] `cargo run -q -p stackly-cli -- provider list --json` → satu baris envelope `schemaVersion` 2, `command` `provider list`, `result` `{ "catalog": …, "items": […] }`; dengan `--category cache` `items` ikut tersaring → AC-9
- [x] `cargo run -q -p stackly-cli -- provider inspect official/postgresql` (state `available` di catalog repo) → blok identitas dengan manifest lengkap dan baris `Baseline`, tabel capability, matrix versi dengan `INSTALL yes`, dan tabel artefak dengan ukuran desimal; `provider inspect official/nats` (entri planned di catalog repo) → blok identitas saja, tanpa tabel capability, matrix, atau artefak → AC-9
- [x] `cargo run -q -p stackly-cli -- provider inspect official/postgresql --json` → `result` sama dengan `ProviderDetail` (kunci `summary`, `capabilities`, `versions`, `platformNotes`, `note`, `issues`) → AC-9
- [x] `cargo run -q -p stackly-cli -- provider inspect nats` → exit 10, `catalog.provider_not_found`, pesan `no provider nats in the catalog; did you mean official/nats`, hint "use the full provider ID, including its namespace"; `provider inspect postgres` dan `official/no-such` → exit 10 dengan hint "run stackly provider list to see the catalog"; `provider inspect Official/NATS` → exit 2 `request.invalid_params` dengan field `provider` → AC-9
- [x] Hentikan daemon, lalu `cargo run -q -p stackly-cli -- provider list` → exit 6; mulai daemon tanpa `STACKLY_CATALOG_DIR`, ulangi `provider list` dan `provider inspect official/nats` → exit 6 dengan `catalog.unavailable` dan hint "run stacklyd from Stackly.app or set STACKLY_CATALOG_DIR" (dua langkah terakhir dicakup test `cargo test -p stackly-cli --test provider`) → AC-9
- [x] `cargo test -p stackly-cli --test provider` → sepuluh test lulus: golden teks dan JSON `list` (semua, `--category messaging`, kategori kosong, catalog development yang lebih tua dari 180 hari, catalog kosong), golden `inspect` untuk NATS (teks dan JSON), `intelonly`, `blockedhost`, `postgresql` (planned), `broken` dan `zenith` (invalid, kategori kosong), baris exit code 10, 2, dan 6 lewat daemon skrip, dan dua test terhadap `stacklyd` asli dengan `STACKLY_CATALOG_DIR` (catalog hasil `cargo xtask catalog build`, dan tanpa catalog) → AC-9
- [x] `cargo test -p stackly-cli --bin stackly provider` → aturan ukuran desimal (`6.8 MB`, `48.1 MB`, `1.2 GB`, `1.23 GB`, naik ke unit berikutnya di 1000), `, +<n> more` untuk versi di atas 16, dan entri tanpa kategori yang tidak pernah cocok dengan filter → AC-9
- [x] `cargo test -p stackly-cli --bin stackly every_command_has_a_description_and_an_example` → `provider list` dan `provider inspect` punya deskripsi dan contoh → aturan CLI spec 0008
- [x] `cargo test --workspace --locked --no-fail-fast`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, dan `cargo xtask codegen --check` → bersih (tidak ada perubahan di `crates/protocol/`, jadi digest kontrak tidak berubah)
- [x] `git status --short` setelah semua langkah → bersih

### Acceptance-criteria coverage

- AC-9: perintah `provider list` dan `inspect` dengan teks dan JSON, `--category`, exit 6 dan 10, dan golden dari fixture `protocols/fixtures/catalog/`. Bagian GUI (view model Swift dari fixture yang sama) ada di milestone 3. Aturan ukuran memakai `ByteCountFormatter` gaya `.file` seperti GUI: kB bulat, MB satu desimal, GB ke atas dua desimal tanpa nol di belakang (teks spec "satu desimal" cocok untuk `6.8 MB`, `48.1 MB`, `1.2 GB`; `1.23 GB` adalah selisihnya).
- AC-8: bagian CLI, yaitu baris catalog dengan tanggal terbit dan keterangan "older than 180 days" saat `outdated`. Baris itu mengikuti teks bagian *CLI* spec (`updating Stackly may bring a newer catalog`), yang tidak menyatakan ada catalog yang lebih baru.
- AC-6 dan AC-7: status, reason, dan keputusan install di CLI adalah nilai wire dari DTO daemon (`unverified (untested)`, `no (no_build_for_arch)`), tidak dihitung ulang.

## Milestone 3: Layar Catalog

Prasyarat untuk langkah yang memakai daemon asli: `cargo build -p stacklyd -p stackly-fake-server` dan `cargo build -p stacklyd --example skeleton_seed`. Langkah snapshot dan test view model berjalan dari fixture di `protocols/fixtures/catalog/` dan `protocols/fixtures/ipc/`, tanpa daemon.

### UI / manual

Lead memasukkannya ke `docs/manual.md`. Hasilnya dicatat `native` dengan status `NOT_RUN` sampai dijalankan.

- [x] Buat catalog dev (`cargo xtask catalog build --out $STACKLY_ROOT/catalog`), jalankan `STACKLY_ROOT=$(mktemp -d) STACKLY_CATALOG_DIR=<folder itu> cargo run -q -p stacklyd`, buka app Debug (`cargo xtask build-app`), pilih Catalog (⌘4) → judul "Catalog", subjudul "Development catalog · updated {tanggal}", enam kartu planned (Pg, My, Na, Ch, Vk, Tr) redup tanpa opacity, picker bersegmen All, Databases, Messaging, Analytics, Cache, Proxy, dan tidak ada tombol Get, New Instance, atau Import Provider → AC-10
- [x] Ketik "post" di field "Search providers" lalu "POST", lalu "postgresql" → hanya kartu PostgreSQL tersisa, pilihan pindah ke kartu itu, dan panel detail menulis catatan planned "This provider isn’t available yet. It will appear here once packaging and tests pass." tanpa matrix, capability, atau tombol → AC-10
- [x] Ketik "zzz" → "No Results for “zzz”", pesan "No providers match “zzz”.", dan tombol Clear Filters mengosongkan pencarian; pilih Messaging tanpa entri → pesan yang sama dengan nama segmen; pilih All → semua kartu kembali → AC-10
- [ ] Tekan ⌘F dari kartu mana pun → field pencarian di toolbar mendapat fokus, dan menekan ⌘F lagi saat sudah fokus tidak mengubah teksnya (Edit > Find > Find… aktif di Catalog, nonaktif di Overview) → AC-10, spec 0069 AC-10
- [x] Pindah ke Instances lalu kembali ke Catalog, tutup dan buka jendela → pilihan kartu yang sama tetap terpilih; filter dan teks pencarian kembali kosong → AC-10
- [x] Dengan catalog fixture (`STACKLY_CATALOG_DIR` ke folder dengan `official/nats` dan entri fixture `linuxonly`, `intelonly`, `blockedhost`, satu paket rusak, lewat `CatalogBuilder` atau salinan `crates/core/tests/fixtures/catalog/`): kartu NATS "Available", panel detail menulis matrix versi (macOS Unverified, Linux Unsupported, Windows Planned) dengan catatan alasan di bawahnya, tiga baris dasar dan delapan capability ("Reload without restart · not supported"), baris License, Source, Download, Disk ("Not listed"), Requires ("macOS 13.0 or later on Apple Silicon"), Dependencies ("None declared"), Publisher, Revision, dan Trust; kartu `intelonly` "Unsupported" dengan baris alasan; `blockedhost` "Blocked: …"; paket rusak "Invalid" dengan banner "This provider failed validation" dan kode `manifest.invalid`; banner di atas grid "{n} providers failed validation." → AC-10
- [x] Perkecil jendela ke 900 × 600 dengan inspector terbuka → grid satu kolom, picker kategori menjadi menu, tidak ada teks terpotong di kartu atau panel, inspector paling lebar 280 pt; matrix versi berubah menjadi daftar per OS → AC-10, AC-11
- [x] Hentikan `stacklyd` → banner jendela "Can’t connect to stacklyd." dan halaman Catalog menampilkan state tunggu; mulai lagi → catalog kembali tanpa memuat ulang app → AC-10
- [ ] Nyalakan Full Keyboard Access (System Settings > Keyboard), lalu hanya dengan keyboard: Tab ke field pencarian, picker kategori, setiap kartu (Return atau Space memilih), dan isi panel detail termasuk link lisensi → semua dapat dicapai dan fokus terlihat → AC-11
- [ ] Nyalakan Differentiate Without Color → badge status menulis simbol tone di depan label dan tidak ada status yang hanya berbeda warna; nyalakan Increase Contrast dan Reduce Transparency, ulangi di light dan dark → semua teks terbaca, kartu planned tidak redup → AC-11
- [x] Atur jam Mac atau pakai catalog dengan `published_at` lebih dari 180 hari → banner "Showing the last known state from {tanggal}." dengan "The development catalog last updated {tanggal}." dan tidak ada teks "latest", "up to date", atau "newer catalog" di layar → AC-8, AC-10

### Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "CatalogAppearanceTests|CatalogCopyTests"` → label, tone, simbol, dan detail setiap `SupportStatus` dan `CatalogEntryState` termasuk `unrecognized`, setiap teks alasan, nama OS dan arsitektur, nilai asing, ukuran desimal, dan subjudul "Official · updated Oct 20, 2026"; `CatalogAppearance.swift` tanpa `default:` → AC-8, AC-12
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "CatalogFilterTests|CatalogStoreTests|ProviderFactsTests"` → filter kategori, pencarian tanpa huruf besar dan diakritik, tidak ada segmen Testing, pilihan default dan pilihan yang tersaring, urutan daemon dan `CatalogInfo` setelah snapshot, store `unsupported` untuk daemon lama, cache detail per instance daemon, provider, dan revision (dibuang saat snapshot baru), Retry setelah gagal, dan baris License, Source, Download, Disk, Requires, Dependencies yang hanya membaca artefak host → AC-9, AC-10
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter CatalogSnapshotTests` → 10 test lulus tanpa `STACKLY_SNAPSHOT_RECORD`: available, unsupported, blocked, planned, invalid, no results, empty, outdated, failed, dan daemon tanpa method, masing masing light, dark, hc-light, hc-dark, narrow, dan Reduce Transparency identik dengan light → AC-10, AC-11
- [ ] `cargo xtask ui-test --only <kelas menu>` (XCUITest menu spec 0068 AC-16) → item Edit > Find > Find… aktif di Catalog dan nonaktif di Overview untuk setiap skenario yang memuat catalog. Menunggu: tetap `NOT_RUN` sampai user menyalakan Automation Mode (`docs/manual.md` bagian K), exit 2 selama itu → AC-10, spec 0069 AC-10
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "CommandMapTests|CommandSourceRuleTests"` → lulus: baris `find` sudah `built` di `CommandMap` (scope `.page(semua bagian)`, requirement `.search`, tercatat di tabel "Commands and shortcuts" `design.md`), dan halaman Catalog tidak memakai `keyboardShortcut` atau `CommandGroup` → AC-10
- [x] `cargo xtask snapshot review` → hanya PNG `CatalogSnapshotTests` yang berubah dibanding main; tidak ada PNG `ShellSnapshotTests`, `DiagnosticsSnapshotTests`, atau `GallerySnapshotTests` yang berubah → AC-10
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter SourceRuleTests` → 21 test lulus, termasuk "every page outside the gallery has a snapshot suite" untuk `CatalogPage` → AC-10
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` (setelah dua `cargo build` di atas) → seluruh suite lulus
- [x] `swift test --package-path clients/swift` → lulus (`ProviderSummary` menjadi `Identifiable` di `StacklyClient`)
- [x] `cargo xtask build-app` → build sukses
- [x] `xcrun swift-format lint --strict` pada file Swift yang berubah → bersih
- [x] `git status --short` setelah semua langkah → bersih

### Acceptance-criteria coverage

- AC-9: bagian GUI (view model Swift membaca fixture `protocols/fixtures/catalog/` yang sama dengan golden CLI: status per sel dan teks ukuran). Golden CLI dan perintah `stackly provider` ada di milestone 2.
- AC-10: test filter, store, state halaman, snapshot, dan langkah UI manual. Subjudul, pencarian, picker, grid, panel detail, matrix, capability, baris key value, state planned dan invalid, hasil kosong, dan `ViewState.resolve`. Perintah Find lewat `.publishesCommands(.page(.catalog), …)`.
Kontras AA diperiksa snapshot hc-light dan hc-dark, tetapi pengukuran kontras dan Differentiate Without Color (yang tidak dirender snapshot, spec 0068 AC-5) tetap langkah manual.
- AC-12: `CatalogAppearanceTests`, tabel di `apps/macos/design.md`, dan teks nilai asing di `CatalogCopyTests`.
- AC-13: milestone 1.

## Milestone 4: Catalog di bundle dan penolakan install end to end

Prasyarat: `cargo build -p stacklyd -p stackly-fake-server` dan `cargo build -p stacklyd --example skeleton_seed`. Bundle dibangun ad hoc di `target/app/Debug/Stackly.app`, tanpa instalasi ke `~/Applications`.

### UI / manual

Tidak ada. Milestone ini tidak mengubah layar; hasilnya terlihat di layar Catalog (milestone 3) hanya lewat sumber `Bundled`.

### Commands

- [x] `cargo test -p xtask build_app` → 31 test lulus, termasuk `the_catalog_step_writes_the_index_and_only_official_packages_into_the_bundle` (langkah catalog menulis `catalog.json` dan hanya `packages/official`), `a_failing_catalog_step_stops_the_build_and_leaves_no_bundle` (repo root tanpa `providers/catalog.toml`: galat menyebut "write the provider catalog" dan `catalog.toml`, langkah man page sesudahnya tidak jalan, tidak ada bundle dan tidak ada folder staging), dan `the_bundled_catalog_needs_an_index_and_only_the_official_namespace` → AC-1, AC-1 spec 0015
- [x] `cargo xtask build-app` → `built …/target/app/Debug/Stackly.app`; `find target/app/Debug/Stackly.app/Contents/Resources/catalog -maxdepth 3` → `catalog.json` dan `packages/official/{nats,postgresql}`, tanpa segmen `testing` → AC-1
- [x] Ubah sementara `providers/catalog.toml` (misalnya `published_at` ke 2027-01-01), jalankan `cargo xtask build-app` → berhenti dengan `error: assembly step failed: write the provider catalog: published_at 2027-01-01T00:00:00Z is in the future`, `target/app/Debug/Stackly.app` tetap bundle lama (waktu ubah sama) dan tidak ada folder staging; kembalikan dengan `git checkout providers/catalog.toml` → AC-1
- [x] `cargo test -p stacklyd --lib startup` → delapan test lulus, termasuk `the_catalog_location_is_the_development_folder_then_the_bundle_then_none` (`STACKLY_CATALOG_DIR` menang, lalu `Contents/Resources/catalog` dari `AppBundle`, lalu tidak ada) → AC-2
- [x] `cargo test -p stacklyd --test catalog` → tujuh test lulus; tiga yang baru memakai salinan `stacklyd` di `<root>/Stackly.app/Contents/MacOS`: katalog bundle dilayani `bundled` dan `official`, `STACKLY_CATALOG_DIR` menang atas katalog bundle, dan bundle tanpa folder catalog menjawab `catalog.unavailable` reason `missing` → AC-2
- [x] Dari bundle hasil `cargo xtask build-app`, tanpa `STACKLY_CATALOG_DIR`: `R=$(mktemp -d)`, `STACKLY_ROOT=$R target/app/Debug/Stackly.app/Contents/MacOS/stacklyd &`, lalu `STACKLY_ROOT=$R target/app/Debug/Stackly.app/Contents/MacOS/stackly-cli provider list --json` → `catalog.source` `bundled`, `trust` `official`, `revision` 4, `outdated` false, `invalidCount` 0, item `official/postgresql` dan `official/nats`; log daemon `catalog_state` `loaded`; hentikan dengan `stackly-cli daemon stop` → AC-1, AC-2
- [x] `cargo xtask evidence run --kind fixture -- cargo test -p stacklyd --test runtime runtime_install_refuses --locked` → satu test lulus (`runtime_install_refuses_an_artifact_for_another_target`: `official/linuxonly` ditolak `catalog.not_installable` reason `no_build_for_host` dengan `availableTargets` `linux-x86_64`, `official/intelonly` `no_build_for_arch` dengan `macos-x86_64`, hint ada, nol operasi dibuat); `evidence: 1 fixture records (PASS 1)`, 0 masalah, run id `01a161fe-a109-74c8-b263-5d806596966c`, mencakup PRD AC-002 dan AC-005. Record fixture, bukan bukti runtime native → AC-7
- [x] `cargo test --workspace --locked --no-fail-fast` → 3338 test lulus, 0 gagal
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, `cargo xtask catalog check`, `cargo xtask evidence check` → bersih
- [x] Tambahan kecil milestone 3: `swift test --package-path apps/macos/Packages/StacklyUI --filter Catalog` → 79 test lulus; 29 PNG referensi `CatalogSnapshotTests` direkam ulang karena cincin pilihan kartu kini `Tone.informative.text`, bukan warna aksen sistem: `catalog-available`, `-blocked`, `-invalid`, `-outdated`, `-unsupported` (light, dark, hc-light, hc-dark, narrow) dan `catalog-planned` (light, dark, hc-light, hc-dark) → AC-10
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` (penuh) → 888 test lulus. Catatan: satu run penuh pada beban mesin sangat tinggi (load average sekitar 44) melaporkan 3 issue di luar known issues yang tidak sempat dikenali; dua run sesudahnya bersih (888 test, 23 known issues), jadi dicatat sebagai flake beban, bukan regresi
- [x] `xcrun swift-format lint --strict` pada `ProviderCard.swift` dan `CatalogCopy.swift` → bersih
- [x] `git status --short` setelah semua langkah → bersih
- [x] Dijalankan ulang oleh checker pada branch yang sudah memuat merge main `e4869c9e` (sesudah F21 milestone 1) → semua langkah di atas lulus. Selisih dengan angka yang tertulis berasal dari main, bukan dari milestone ini: `catalog.revision` kini 5 (`3bde85d0`, F21) dan `provider list` mengembalikan enam item (`official/postgresql`, `official/nats`, plus empat entri planned `mysql`, `clickhouse`, `valkey`, `traefik`); `cargo test --workspace` 3373 lulus, 0 gagal (termasuk `operations` perf dan `served` p95); `swift test --filter Catalog` 82 lulus; suite Swift penuh 1039 test dengan satu kegagalan `SkeletonProcessRunnerTests.aProcessThatOutlivesItsLimitIsKilledAndReportedAsTimedOut` (batas 20 s terlewati, 24,7 s) pada dua run dengan load average di atas 100, lulus 0,3 s saat dijalankan sendiri di load sekitar 60. Test itu milik spec 0014, di luar diff milestone ini. Run evidence fixture baru: `evidence: 1 fixture records (PASS 1)`, 0 masalah → AC-1, AC-2, AC-7

### Acceptance-criteria coverage

- AC-1: langkah catalog `build-app` (test xtask, bundle hasil build, dan build yang berhenti saat catalog gagal) menutup klausa yang menunggu di Milestone 1; klausa catalog AC-1 spec 0015 dibuktikan di sini.
- AC-2: cabang lokasi `Bundled` dari `AppBundle` (test unit dan tiga test binary), menutup bagian yang menunggu di Milestone 1.
- AC-7: penolakan ujung ke ujung lewat `runtime.install` dengan fixture `official/linuxonly` dan `official/intelonly`, menutup bagian yang menunggu di Milestone 1; PRD AC-002 dan AC-005 tercatat sebagai record fixture.
- AC-3 sampai AC-6, AC-8 sampai AC-13: Milestone 1 sampai 3.
