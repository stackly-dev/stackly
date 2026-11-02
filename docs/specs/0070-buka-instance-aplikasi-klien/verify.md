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
