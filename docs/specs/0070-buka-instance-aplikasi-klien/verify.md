# Verify: Buka instance di aplikasi klien · spec 0070 · updated 2026-11-02

_Langkah diturunkan dari acceptance criteria spec 0070. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri. Milestone 3 (bukti native, AC-15) menambah bagiannya sendiri; jangan ubah bagian milestone lain._

## Milestone 2: GUI

_Dijalankan oleh `dev-f70-m2` pada 2026-11-02 di branch `claude/dev-f70-m2-100be9`. Jalur NSWorkspace asli (membuka TablePlus, Postico, Terminal) tidak dijalankan di sini: itu bukti native Milestone 3, dan semua tes di bawah memakai `AppLocator` dan `WorkspaceOpener` palsu (`FixtureOpenIn`)._

### UI / manual

- [ ] Jalankan `cargo xtask build-app`, buka Stackly, pilih instance PostgreSQL yang `running` dan `ready` → di inspector Instances ada tombol menu `arrow.up.forward.app` di samping Open, dan menu Instance di menu bar punya submenu "Open In" tanpa shortcut → AC-5
- [ ] Buka detail instance itu → toolbar punya tombol menu Open In di samping aksi utama dengan isi yang sama: "Open in TablePlus" (dan "Open in Postico" bila terpasang), "Open psql in Terminal" → AC-5
- [ ] Hentikan instance → item menu nonaktif dengan teks "Start the instance to open it."; item `reveal` (bila provider punya) tetap aktif → AC-6
- [ ] Matikan daemon (menu Stackly > Reconnect tidak berhasil) → semua item Open In nonaktif mengikuti banner gate → AC-6
- [ ] Pilih instance MySQL → tombol Open In dan submenu tidak tampil (provider tanpa blok `openWith`) → AC-5
- [ ] Pilih instance baru dibuka (detail belum termuat) → menu berisi satu item nonaktif "Loading…"; tanpa instance terpilih submenu nonaktif → AC-5
- [ ] Hapus password instance dari Keychain lewat Enter Secret, lalu pilih "Open in TablePlus" → aplikasi tetap terbuka, kartu caution "Opened without copying the password." menawarkan "Enter Secret…" yang membuka sheet → AC-10 (bagian yang menyentuh aplikasi asli tetap bukti Milestone 3)

### Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter OpenInModelTests` → 23 test lulus: label sama dengan `detail-open-actions.json`, daftar dan urutan aksi, entri `app` tanpa aplikasi terpasang tersembunyi, gate (stopped, belum ready, `reveal`, alasan daemon menang, `Unrecognized`), salin password lalu buka (URL tanpa password), Terminal lewat `prepare_terminal` (semua penolakan dan reload), browser, Finder, password `missing`, Keychain terkunci, kegagalan NSWorkspace, dan sentinel pada model → AC-5, AC-6, AC-7, AC-8, AC-9, AC-10, AC-11, AC-14
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter OpenInMenuTests` → 4 test lulus: submenu membaca publikasi halaman Instances, mati tanpa jendela key atau halaman lain, item berhenti saat gate mutasi tertutup, tombol tersembunyi saat tidak ada aksi → AC-5, AC-6
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "InstancesSnapshotTests|InstanceDetailSnapshotTests"` → 36 test lulus, termasuk `instances-no-open-in` dan `instances-open-in-loading` baru, dan inspector `instances-ready`, `instances-failed-chosen`, `instances-blocked`, `instances-not-ready`, `instance-detail-with-inspector` yang direkam ulang (terang, gelap, sempit); lihat `cargo xtask snapshot review` → AC-5, AC-6
- [x] `cargo xtask build-app` → BUILD SUCCEEDED
- [x] `xcrun swift-format lint --strict --configuration .swift-format` pada semua file Swift yang berubah → tanpa peringatan
- [x] `CARGO_INCREMENTAL=0 cargo test -p stacklyd --test supervisor open_with` → 6 test lulus, termasuk `the_fixture_credential_is_in_no_detail_reply_script_or_log_of_an_open_in`: prefix fixture credential tidak ada di balasan `instance.get_detail`, balasan `instance.prepare_terminal`, isi skrip, argv, env, dan log daemon → AC-14
- [x] `CARGO_INCREMENTAL=0 cargo test -p stackly-cli --test instance open` → 6 test lulus, termasuk `a_daemon_that_would_reveal_the_password_never_gets_asked_and_no_output_holds_it`: output teks dan `--json` tanpa prefix, dan CLI tidak memanggil `secret.reveal` atau `instance.prepare_terminal` → AC-14
- [x] `cargo xtask check-deps` → lulus

### Acceptance-criteria coverage

- AC-5 · tes `OpenInModelTests`, `OpenInMenuTests`, snapshot inspector; langkah toolbar dan menu bar manual di atas
- AC-6 · `OpenInModelTests` (gate, alasan, `Unrecognized`, `mutates: true` untuk gate shell), `OpenInMenuTests`
- AC-7, AC-8, AC-9 · `OpenInModelTests` dengan `FixtureOpenIn`; pembukaan nyata menunggu Milestone 3
- AC-10, AC-11 · `OpenInModelTests`
- AC-14 · sentinel daemon (`open_with.rs`), CLI (`open.rs`), dan model Swift (`OpenInModelTests`); `crates/core/tests/open_with_render.rs` tidak diubah karena render hanya menerima reference. Skrip bermode 0700, milik pengguna, tanpa karantina sudah dicek di `a_ready_instance_gets_a_private_script_in_its_temp_folder_without_a_secret`. Audit `secret.reveal` per salin adalah perilaku spec 0011 yang dipakai apa adanya
- AC-15 · menunggu Milestone 3

## Milestone 3: Evidence on this Mac

_Dijalankan oleh `dev-f70-m3` pada 2026-11-02 di branch `claude/dev-f70-m2-100be9` pada Mac ini (Apple M1 Max, macOS 27.0.1 build 26A434), dengan artefak PostgreSQL 18.6 dan NATS 2.15.1 dari cache xtask (`~/Library/Caches/dev.stackly.xtask/runtime/`, tanpa unduhan). Membuka Terminal, TablePlus, Postico, dan browser sungguhan tidak dilakukan oleh agen: langkahnya ada di "UI / manual" dan menunggu pengguna._

### UI / manual

Siapkan root sementara dan app seperti bagian T di `docs/manual.md` (langkah 1 sampai 5: root `$R`, catalog, daemon, salin artefak dari cache xtask, tanpa jaringan). Root sementara memakai keychain file, jadi login Keychain tidak disentuh. Pasang PostgreSQL 18 dari halaman Runtimes, buat instance `pg-open` dari halaman Create, lalu Start sampai `ready`.

- [ ] "Open psql in Terminal" pada `pg-open` membuka jendela Terminal baru tanpa dialog Gatekeeper, `psql` meminta `Password for user postgres:`, ⌘V menempel password (kartu "Password copied. Paste it when psql asks."), dan `SELECT 1;` menjawab satu baris `1` → AC-8, AC-15
- [ ] Selama sesi `psql` masih terbuka, di terminal lain jalankan `ps -ww -axo pid,ppid,command | grep psql`: baris `psql --host=… --port=… --username=postgres postgres` tidak memuat password, dan `ps -wwE -p <pid psql>` tidak memuat `PGPASSWORD` → AC-14, AC-15
- [ ] Matikan instance dari terminal lain (`stackly instance stop pg-open --wait`), lalu jalankan ulang skrip dengan dobel klik dari Finder (`$R/data/instances/<id>/temp/open-psql.command`): `psql` gagal konek, Terminal menampilkan "psql exited with status 2. Press Return to close." dan menunggu Return → AC-15
- [ ] Start lagi, pilih "Open in TablePlus": TablePlus terbuka ke host `127.0.0.1`, port instance, user `postgres`, dan meminta password; kartu "Opening “pg-open” in TablePlus. Password copied. Paste it when TablePlus asks." muncul, ⌘V menempel password yang benar, dan koneksi berhasil → AC-7, AC-15
- [ ] Pilih "Open in Postico" bila Postico terpasang; kalau tidak, menu tidak memuat item itu (entri `app` tanpa aplikasi terpasang tersembunyi) → AC-5, AC-15 (di Mac ini `BLOCKED`, lihat bawah)
- [ ] Pasang NATS 2.15.1, buat instance `nats-open`, Start sampai `ready`, pilih "Open Monitoring in Browser": browser default membuka `http://127.0.0.1:<port monitoring>/` dengan halaman awal monitoring NATS (daftar `varz`, `connz`, `routez`, …) dan kartu "Opening Monitoring for “nats-open” in your browser." → AC-9, AC-15
- [ ] Traefik: paketnya belum masuk katalog (`[[planned]]` di `providers/catalog.toml`), jadi instance Traefik tidak bisa dibuat dari app; "Reveal Routes File in Finder" tidak bisa dicoba → AC-9 (`BLOCKED`, lihat bawah)

### Commands

- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test postgresql --locked -- --ignored` → 47 PASS dengan manifest baru, hanya sebagai cek regresi dan tidak disimpan sebagai evidence (termasuk `compat::postgresql_17_runs_the_package_and_records_what_it_proved` dan `compat::postgresql_18_…`); lalu `cargo xtask evidence run … -- … --ignored compat::` → 2 PASS dan `cargo xtask runtime compat-record` menyimpannya sebagai `evidence/compatibility/01a19a76-50ca-761d-abd1-4d4c61091da3` → AC-15 (dasar `versions.json`)
- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test nats --locked -- --ignored nats_compatibility nats_lifecycle_in_process` → 2 PASS, disimpan sebagai `evidence/compatibility/01a19a74-dab1-715d-9449-2d0b0e289fac`. Run tanpa filter juga menjalankan `lifecycle_ops::nats_refuses_reload_without_an_operation_and_restarts_with_a_new_process`, yang berakhir tanpa subject (`a native PASS needs at least one subject`), jadi tidak dipakai sebagai rekaman BR-025 (milik F26, bukan F70) → AC-15 (dasar `versions.json`)
- [x] `cargo xtask codegen` → `providers/postgresql/versions.json` dan `providers/nats/versions.json` memuat record `provider` baru dengan `manifestSha256` `c8287f00…` (PostgreSQL 1.0.0) dan `8e306092…` (NATS 1.0.1), sama dengan digest manifest sekarang; `cargo xtask codegen --check`, `cargo xtask runtime check` (5 compatibility run), `cargo xtask provider check`, dan `cargo xtask catalog check` (revisi 9) lulus
- [x] Probe native PostgreSQL 18.6 (tes sementara, tidak di-commit; harness `apps/daemon/tests/postgresql/`): instance dibuat dengan password acak, `instance.get_detail` menampilkan `openActions` `tableplus` dan `postico` (url `postgresql://postgres@127.0.0.1:<port>/postgres`, tanpa password) dan `psql` (command `…/bin/psql` runtime instance, `--host=<socketDir>`, `--port`, `--username=postgres`, `PGAPPNAME=stackly-terminal`), semuanya `available`; `instance.prepare_terminal psql` menulis `open-psql.command` bermode 0700 milik pengguna (xattr hanya `com.apple.provenance`, tanpa `com.apple.quarantine`) → AC-4, AC-14, AC-15
- [x] Skrip `open-psql.command` yang sama dijalankan di pseudo terminal oleh agen: `Password for user postgres:` muncul, password dari `ConcealedPasteboard` yang ditempel menghasilkan `psql (18.6)`, prompt `postgres=#`, dan `SELECT 1;` menjawab `(1 row)`; `ps -ww -axo pid,ppid,command` selama sesi memuat `/bin/sh …/open-psql.command` dan `…/bin/psql --host=… --port=… --username=postgres postgres` tanpa password, dan isi skrip serta `openActions` tidak memuat password → AC-14, AC-15 (Terminal.app dan Gatekeeper tetap bukti manual di atas)
- [x] Path socket instance di root uji: `/var/folders/sx/…/stackly-<8 hex>/i/<8 hex>/.s.PGSQL.<port>` sepanjang 91 byte, di bawah batas 104 byte → AC-15
- [x] Probe native NATS 2.15.1 (tes sementara, tidak di-commit): `openActions` `monitoring` merender `http://127.0.0.1:<port>/`, dan `GET /` ke URL itu menjawab `HTTP/1.0 200 OK` `text/html` dengan halaman awal monitoring NATS → AC-9, AC-15 (bagian "browser membuka halaman" tetap manual di atas)
- [x] `cargo test -p stackly-core --lib catalog::status`, `cargo test -p stackly-cli --test provider`, dan `swift test --package-path apps/macos/Packages/StacklyUI` → hijau setelah digest dan status fixture NATS (`8e306092…`, `experimental`/`runtime_tested`) dipakai di golden `provider_inspect_nats.json`, needle `inspect.rs`, `ProviderFactsTests`, `CatalogViewsTests`, dan enam snapshot katalog dan runtime direkam ulang; satu pengecualian di bawah

### BLOCKED

- **Postico**: `at.eggerapps.Postico2` dan `at.eggerapps.Postico` tidak terpasang di Mac ini (`mdfind` kosong). Alasan: aplikasi tidak ada. Owner: pengguna (pasang Postico 2, lalu jalankan langkah "Open in Postico"). Akibat: asumsi bundle ID Postico di spec (*Entri per paket*) belum terbukti.
- **TablePlus**: terpasang (`/Applications/TablePlus.app`, bundle ID `com.tinyapp.TablePlus`, cocok dengan manifest), tetapi membukanya butuh jendela aplikasi asli. Owner: pengguna, langkah manual di atas.
- **Terminal.app, Gatekeeper, dan browser**: agen tidak membuka aplikasi GUI pihak ketiga. Owner: pengguna, langkah manual di atas.
- **Traefik**: tidak ada instance Traefik karena paketnya masih `[[planned]]` di katalog. Owner: fitur provider Traefik (belum ada di katalog), bukan F70.

### Acceptance-criteria coverage

- AC-15 · dasar native ada di sini (compat run, versions.json, probe PostgreSQL, probe NATS); pembukaan aplikasi asli (Terminal, TablePlus, Postico, browser) menunggu langkah manual di atas, jadi AC-15 belum `PASS` penuh
- AC-14 · sentinel Milestone 2, ditambah cek native bahwa `openActions`, skrip, dan `ps -ww` tidak memuat password instance PostgreSQL 18.6 sungguhan
