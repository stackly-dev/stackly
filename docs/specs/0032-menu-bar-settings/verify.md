# Verify: Menu bar dan Settings · spec 0032 · updated 2026-11-03

_Langkah diturunkan dari acceptance criteria spec 0032. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah._

## Milestone 1: Setting daemon dan lokasi, sampai CLI

Langkah build 1 sampai 5 (commit `6f1d95dd`, `2e9fb672`, `48f2f33b`, `de7ea0e8`, `9d0a543d`). Bagian AC yang belum: tab Settings di GUI (milestone 2), Stop All dan precheck `expectedInstanceIds` (langkah 9, milestone 3), menu bar, notifikasi, dan semua evidence native.

### UI / manual

- [x] Buka `crates/storage/migrations/0011_settings_locations.sql` → tabel `storage_locations` (tiga baris bawaan dengan ID tetap, trigger tanpa update dan delete), `daemon_settings` (CHECK `id = 1`, trigger lokasi harus sejenis), `instance_paths` dibangun ulang dengan `location_id`, cek jumlah baris yang gagal kalau salinan kurang, dua index parsial → AC-25
- [x] Buka `crates/core/src/storage/location.rs` → `check_location` fungsi murni yang menolak aturan pertama yang gagal sesuai urutan tabel; `prepare_location` membuat `Stackly` 0700, subfolder layout, dan `.stackly-location` 0600 → AC-19
- [x] Buka `crates/core/src/layout.rs` → `LocationBook` di dalam `PreparedLayout`: setiap path instance dihitung dari lokasi instance itu, instance yang tidak dikenal jatuh ke lokasi bawaan → AC-19, AC-25
- [x] Buka `crates/core/src/instance/create/work.rs` → langkah `layout` mencatat pasangan lokasi sebelum membuat folder, langkah `register` membangun instance di dalam guard dan menulisnya dengan `InsertInstanceAt` → AC-18, AC-19
- [x] Buka `apps/daemon/src/ipc/settings.rs` → `settings.update` dan `storage.set_location` mengikuti `instance.set_startup`: precheck, operasi, tunggu, balas; `settings.changed` sekali per perubahan yang diterima → AC-16, AC-17
- [x] Jalankan `target/debug/stackly settings --help` dan `target/debug/stackly storage --help` → verb `get`, `set`, `locations`, `set-location` → AC-16, AC-19
- [x] Jalankan `target/debug/stackly instance create --help` → `--autostart` dan `--no-autostart` saling menolak → AC-18

### Commands

- [x] `cargo test -p stackly-protocol` → fixture round trip untuk lima method baru, event `settings.changed`, tiga kode error, `expectedInstanceIds`, form dengan `startupDefault` dan lokasi, nilai tidak dikenal, dan penolakan patch kosong → AC-16, AC-17, AC-18, AC-19
- [x] `cargo xtask codegen --check` → "20 generated files are up to date"; `ProtocolVersion::CURRENT` 3.0, `protocols/contract-versions.json` tanpa entri 2.6 → AC-16
- [x] `cargo test -p stackly-domain location` dan `cargo test -p stackly-platform layout` → ID bawaan tetap, patch yang tidak mengubah apa pun tidak menaikkan revision, 1000 instance di tiga lokasi resolve persis seperti `instance_paths_in` dan tidak ada path yang sama atau bertumpuk (*AC-10 per lokasi*) → AC-25
- [x] `cargo test -p stackly-storage` → migrasi 11 (fingerprint, fixture beku `registry-v11.sql`), v10 ke v11 dengan semua path sama persis dan `location_id` bawaan, migrasi 11 yang gagal sesudah salinan tidak mengubah registry dan snapshot ada, aturan tabel (satu baris pengaturan, satu lokasi bawaan per jenis, lokasi tidak diubah atau dihapus, jenis lokasi cocok dengan akar path), konformasi `settings` untuk SQLite dan fake → AC-25, AC-16, AC-19
- [x] `cargo test -p stackly-core --test storage_location` → tabel uji per `rule` dan urutannya di `FakePlatform` (relatif, hilang, bukan folder, symlink, bukan milik user, terlalu terbuka, folder yang dijaga macOS, bukan lokal, bukan internal, menimpa root, penanda asing atau dipakai lagi) dan `prepare_location` → AC-19
- [x] `cargo test -p stackly-core --test create_operation --test settings_ops --test supervisor_start` → create sesudah ganti lokasi masuk ke lokasi baru dan instance lama tetap, create tanpa `startup` memakai default dan yang eksplisit menang, folder lokasi yang hilang menolak create, start memeriksa lokasi instance sebelum data (`instance.storage_location_missing`, `storage.location_invalid`), precheck settings, inventory lokasi kustom → AC-18, AC-19, AC-20
- [x] `cargo test -p stacklyd --test supervisor settings storage_locations` → defaults dan retensi log (AC-21), update parsial dan satu event, revision usang tidak menulis apa pun, patch tanpa perubahan tanpa operasi dan tanpa event, patch kosong ditolak, default startup untuk create dan form, folder baru untuk data dan cache di filesystem asli, tujuh aturan yang bisa dilanggar folder asli, folder yang dicabut menghentikan create dan start → AC-16, AC-17, AC-18, AC-19, AC-20, AC-21
- [x] `cargo test -p stackly-cli --test settings` dan `--test instance create` → output dan params `settings get` dan `set`, exit 3 dengan "Settings changed while saving. Run the command again." tanpa mengulang, kalimat aturan di `set-location`, path relatif menjadi absolut, `--autostart`, `--no-autostart`, dan tanpa flag tidak mengirim `startup` → AC-16, AC-17, AC-18, AC-19
- [x] `swift test --package-path clients/swift` dan `swift test --package-path apps/macos/Packages/StacklyUI` → 247 test client lulus; StacklyUI lulus kecuali `CreateInstanceModelTests` yang flaky dan lulus kalau dijalankan sendiri; hanya tipe hasil codegen dan perbaikan mekanis (form dengan tiga field baru, dua judul operasi) → AC-16
- [x] `xcrun swift-format lint --strict` dan `cargo xtask build-app` → bersih
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, `cargo xtask evidence check`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → bersih
- [ ] Volume external dan network sungguhan untuk aturan `not_internal_volume` dan `not_local_volume` → `BLOCKED`, reason `no external or network volume on this Mac`. Aturan itu dibuktikan di `FakePlatform`; pembacaan `NSURLVolumeIsInternalKey` di macOS dibuktikan oleh test daemon di filesystem asli, yang lolos aturan itu di volume internal → AC-19

### Acceptance-criteria coverage

- AC-16 · `settings.get`, `settings.update`, `settings.changed`, `stackly settings get` dan `set`; refetch di client menunggu milestone 2
- AC-17 · precheck dan guard memeriksa revision (`resource.revision_mismatch`), CLI keluar 3 tanpa mengulang; baris GUI "Changed elsewhere" menunggu milestone 2
- AC-18 · `defaultStartup` dibaca di dalam guard create, `startupDefault` di form create dan duplicate, `--no-autostart`; mengubah default tidak mengubah instance yang ada
- AC-19 · `storage.set_location`, `storage.list_locations`, dua belas aturan, layout lokasi, Use Default, jumlah pemakai, status terjangkau; panel folder dan konfirmasi menunggu milestone 2
- AC-20 · start memeriksa lokasi instance sebelum cek data, preflight create punya baris `storage_location`
- AC-21 · `logRetention` dari konstanta log store; baris di tab Storage menunggu milestone 2
- AC-25 · migrasi 11, test migrasi, *AC-10 per lokasi*
- AC-7, AC-8, AC-9 · hanya kontrak (`expectedInstanceIds`, `instance.stop_all_changed`); precheck dan CLI di langkah 9

### Catatan

- **Bentuk balasan.** `settings.update` membalas `DaemonSettingsView` dan `storage.set_location` membalas `StorageLocationView` lokasi default jenis itu, mengikuti `instance.set_startup`, karena `OperationResult` tidak membawa payload bertipe. Spec menulis `OperationView`; spec perlu mengikuti.
- **Versi protokol.** Naik mayor ke 3.0, bukan minor 2.6 seperti teks spec (baris 205, 428, 478). Keputusan lead atas delegasi user: `EventKind` tertutup, jadi client 2.x yang membaca `settings.changed` gagal decode, dan break harus naik mayor (spec 0030 AC-18 sama). Versi 2.6 tidak pernah masuk `main`, jadi entrinya dihapus dari `protocols/contract-versions.json` dan 3.0 dicatat dengan digest yang sama. Teks spec diselaraskan lead lewat `/sync`.
- **Waktu baris awal.** Migrasi tidak boleh membaca jam, jadi tiga lokasi bawaan dan baris pengaturan pertama memakai epoch Unix, bukan "waktu migration" seperti di spec. `updatedAt` baru berarti sejak perubahan pertama.
- **`InsertInstanceAt`.** Create membuat folder di langkah `layout` sebelum commit, jadi pasangan lokasi harus ikut ke commit. WriteOp baru `InsertInstanceAt { instance, data, logs }` dipakai create; `InsertInstance` tetap mengambil pasangan dari baris pengaturan di dalam transaksi yang sama. Create yang bersamaan dengan ganti lokasi memakai satu pasangan utuh, tidak pernah campuran.
- **Satu `LocationBook`.** Daripada mengoper `LocationSet` ke setiap pemakai, `PreparedLayout` membawa buku lokasi bersama, diisi saat registry siap dan saat `storage.set_location` selesai. `LocationSet::resolve` mengembalikan `Result` karena lokasi yang tidak dikenal tidak boleh diam diam jatuh ke lokasi bawaan. Daftar pemakai yang pindah ada di pesan commit langkah 3.
- **`check_location`.** Mengembalikan `LocationVerdict` (`New` atau `Reuse(id)`) dan mencakup aturan 1 sampai 11; aturan 12 `not_writable` datang dari `prepare_location`. Symlink ke folder lolos aturan 3 supaya dilaporkan sebagai `symlink` (aturan 4). Leluhur yang tidak bisa dibaca tidak dihitung sebagai symlink.
- **Folder kustom adalah root yang dimiliki.** `check_owned_path` dan `remove_owned_tree` menerima folder lokasi kustom; `OwnedPath` kini membawa folder root nya.
- **Unduhan.** Instal runtime membaca folder unduhan sekali saat operasi mulai, supaya perubahan lokasi cache di tengah jalan tidak memecah satu instal.
- **Perintah setara.** Perintah setara di sheet create semula hanya menulis `--autostart`. Sesudah milestone 2, `CommandDraft` punya `noAutostart` dan perintahnya menulis `--no-autostart` kalau draft meminta `manual` sementara default daemon `at_login` (lihat Milestone 2).
- **Lokasi file test.** Test daemon ada di `apps/daemon/tests/supervisor/settings.rs` dan `storage_locations.rs` karena rig `Site` dan `Api` ada di sana.
- **Tidak terkirim.** Pesan ke `dev-f31-m3` soal blok kecil di `apps/daemon/src/server.rs` gagal terkirim (sesi itu tidak terhubung ke sesi ini). Perubahannya satu blok sesudah `core.set_registry(RegistryState::Ready { .. })`.

## Milestone 2: Jendela Settings

Langkah build 6 sampai 8 (commit `37fda004`, `001d6e73`, `4b6d0ee3`). Tindak lanjut sesudah merge `main` (F31 milestone 4, kontrak 3.1): jalur matikan agent dari baris General dan `--no-autostart` di perintah setara (bagian *Tindak lanjut* di bawah). Bagian AC yang belum: Stop All dan menu bar (milestone 3), notifikasi dan baris izin (milestone 4), fixture, snapshot, dan semua evidence native (milestone 5).

### UI / manual

- [x] Buka `clients/swift/Sources/StacklyClient/Settings/SettingsClient.swift` → empat method bertipe (`get`, `update`, `locations`, `setLocation`), balasan jenis lain menjadi `protocolViolation` → AC-16, AC-19
- [x] Buka `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Settings/SettingsStore.swift` → salinan terakhir, satu perubahan pada satu waktu dengan revision salinan itu, `resource.revision_mismatch` menjadi "Changed elsewhere. Try again." lalu baca ulang, jawaban lebih lama tidak menimpa yang lebih baru → AC-16, AC-17, AC-24
- [x] Buka `Connection/DaemonModel.swift` → `settings` membaca ulang pada `.resnapshot` dan `.caughtUp` (gap dan sambung ulang) dan pada event `settings.changed` → AC-16
- [x] Buka `Settings/SettingsAvailability.swift` → satu entri per baris dengan fitur pemiliknya; Notifications (milestone 4), Clean Up (fitur 55), sumber community dan import (fitur 37) tidak tampil → AC-13
- [x] Buka `Settings/AppPreferences.swift` → Appearance System, Light, Dark di `UserDefaults`, diterapkan lewat `NSApp.appearance` saat app mulai dan saat berubah → AC-14
- [x] Buka tab Instances, Sources, Updates di `Settings/SettingsTabs.swift` → toggle dan segmented langsung berlaku tanpa Apply, indikator kecil selama operasi, banner "Can’t connect to stacklyd.", "last known", "Not available while stacklyd is offline.", dan catatan jujur channel dan telemetry → AC-13, AC-23, AC-24
- [x] Buka `Settings/StartupRowContent.swift` dan `StartupSettingsSection.swift` → baris agent untuk enam state, kalimat login berikutnya dengan jumlah instance `at_login`, atau teks "Won’t start at login…" → AC-15
- [x] Buka `Agent/SMAppServiceMainApp.swift`, `LoginItemModel.swift` → toggle Open Stackly at login terpisah dari agent, state dibaca dari OS setiap app aktif, `requiresApproval` dengan Open Login Items; aturan sumber spec 0012 mengizinkan `import ServiceManagement` di file ini → AC-12
- [x] Buka `Settings/StorageSettingsTab.swift` dan `LocationRowContent.swift` → Change… (panel folder, lalu konfirmasi), Use Default hanya saat default bukan bawaan, Reveal in Finder, lokasi lama, "Not reachable", "Unknown while stacklyd is offline", baris retention read only → AC-19, AC-21, AC-24
- [x] Buka `CreateInstance/CreateInstanceModel.swift` dan `NewInstanceSheet.swift` → toggle Start at login mulai dari `startupDefault` form (create dan duplicate), pilihan orang tidak ditimpa, `startup` selalu eksplisit (`at_login` atau `manual`), baris "Data folder" → AC-18, AC-19

### Commands

- [x] `swift test --package-path clients/swift` → `SettingsClientTests` (6 test) lulus bersama suite lain; lihat hasil akhir di bawah → AC-16, AC-19
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` → `SettingsStoreTests`, `AppPreferencesTests`, `SettingsAvailabilityTests`, `StartupRowContentTests`, `LoginItemModelTests`, `LocationRowContentTests`, `StorageSettingsModelTests`, tiga test baru di `CreateInstanceModelTests`, dan `SourceRuleTests` → AC-12 sampai AC-19, AC-21, AC-23, AC-24
- [x] `xcrun swift-format lint --strict` dan `cargo xtask build-app` → bersih
- [ ] Open Stackly at login sungguhan (mendaftarkan `SMAppService.mainApp` dari app yang ditandatangani di `/Applications`) → `BLOCKED`, reason `no code signing identity on this Mac (OQ-012)`. Adapter dan model dibuktikan dengan fake → AC-12
- [ ] Panel folder dan Reveal in Finder sungguhan, serta jendela Settings terlihat → langkah manual milestone 5 (tidak dijalankan di sini, app tidak dibuka) → AC-14, AC-19

### Acceptance-criteria coverage

- AC-12 · adapter, model, dan toggle selesai dengan fake; pendaftaran nyata `BLOCKED`; deteksi peluncuran sebagai login item di milestone 3 (langkah 11)
- AC-13 · lima tab; baris fitur yang belum ada tidak tampil lewat `SettingsAvailability`; `SettingsPlaceholder` sudah tidak ada
- AC-14 · Appearance di `UserDefaults`, berlaku lewat `NSApp.appearance` untuk semua jendela
- AC-15 · baris agent untuk enam state, kalimat login berikutnya, Turn On, Open Login Items; jalur matikan menunggu kontrak 3.1
- AC-16 · refetch saat event, gap, dan sambung ulang di `DaemonModel`
- AC-17 · "Changed elsewhere. Try again." di baris dan di baris lokasi
- AC-18 · `startupDefault` mengisi toggle di sheet New Instance dan Duplicate
- AC-19 · Change…, konfirmasi, Use Default, Reveal in Finder, lokasi lama, "Not reachable"; panel folder nyata di manual
- AC-20 · sisi GUI hanya menampilkan `storage.location_invalid` dan "Not reachable"; sisi daemon di milestone 1
- AC-21 · baris "Instance logs" dari `DaemonSettingsView.logRetention`
- AC-23 · catatan channel dan telemetry persis seperti spec
- AC-24 · banner, "last known", "Unknown while stacklyd is offline", "Not available while stacklyd is offline."

### Catatan

- **`startup` selalu eksplisit.** Sheet create mengirim `at_login` atau `manual`, tidak lagi `nil`. Kalau tidak, default `at_login` dengan toggle dimatikan orang akan tetap membuat instance `at_login`. Test lama yang mengharapkan `nil` diubah menjadi `manual`.
- **Retention.** Ukuran segmen ditulis dalam MiB, sama dengan CLI (`mebibytes`), bukan satuan desimal katalog.
- **Reveal in Finder.** Memakai `OpenInPorts.system.opener.reveal` yang sudah ada; folder lokasi ditampilkan terpilih di folder induknya.
- **Graf pengetahuan.** `graphify-out/` ada di `.gitignore` dan tidak ada di worktree ini, jadi `graphify update .` tidak dijalankan sebelum commit.
- **Snapshot.** Baris "Data folder" menambah satu baris di sheet New Instance, jadi 24 PNG `NewInstanceSnapshotTests` (light, dark, narrow) direkam ulang dan dilihat; tidak ada snapshot lain yang berubah. Snapshot jendela Settings baru dan fixture nya di milestone 5 (langkah 13).
- **Hasil suite penuh.** Client 253 test lulus. StacklyUI 1757 test: dua test `CreateInstanceModelTests` (`aRefusedCreateLeavesAnEnterSecretOffUntilTheTextIsTypedAgain`, `startAtLoginIsOffUntilChosenAndGoesToThePreflightAndTheCreate`) gagal pada run penuh dan lulus (45 dari 45) kalau dijalankan sendiri, flaky yang sudah diketahui; 24 snapshot sheet gagal sebelum direkam ulang dan lulus sesudahnya.

### Tindak lanjut: Turn Off dari General dan `--no-autostart`

Sesudah `git merge main` (F31 milestone 4, kontrak 3.1: `ShutdownMode::AgentOff`, `ShutdownAccepted { exiting, stopAll }`, `TurnOffModel`, `TurnOffPlan`, tombol Turn Off… di Diagnostics).

- [x] Buka `Settings/StartupSettingsSection.swift` → mematikan toggle Start stacklyd at login memanggil `model.turnOff.begin(from: .settings)`, konfirmasi "Turn Off Background Agent?" yang sama dengan Diagnostics (`ConfirmationRequest`, destructive), lalu `confirm()` menjalankan `agent_off` dan unregister hanya kalau daemon melaporkan `exiting: true`; pesan penolakan dari `TurnOffPlan.refusal` tampil di bawah baris → AC-15
- [x] Buka `Agent/TurnOffModel.swift` dan `Diagnostics/BackgroundAgentDiagnosticsSection.swift` → `TurnOffModel.Origin` (`diagnostics` atau `settings`) supaya konfirmasi hanya muncul di jendela yang memintanya, bukan di Diagnostics dan Settings sekaligus; diff F31 kecil (satu enum, satu parameter, satu kondisi) → AC-15
- [x] `StartupCopy.turnOffPending` dan penanda `// provisional: F31 milestone 4` dihapus; agent yang masih `requiresApproval` dicabut di Login Items (`StartupCopy.removeInLoginItems`), karena Turn Off menghentikan daemon yang sedang jalan dan di sana tidak ada → AC-15
- [x] Buka `crates/protocol/src/instance/command.rs`, `crates/core/src/instance/create/command.rs`, `apps/daemon/src/map/create.rs`, `apps/cli/src/commands/instance/create.rs` → `CommandDraft.noAutostart` (`#[serde(default)]`), `--no-autostart` ditulis sesudah `--autostart` dan sebelum `--start`, `CommandSpec.no_autostart` benar hanya bila draft meminta `manual` dan default daemon `at_login`, CLI mengisi field itu dari `--no-autostart` → AC-18
- [x] `cargo xtask codegen --check` → "20 generated files are up to date": `CommandDraft` tidak ada di kontrak (bukan typeshare dan tidak ada di skema), jadi `ProtocolVersion` tetap 3.1 dan tidak ada tipe Swift yang berubah → AC-18
- [x] `cargo test -p stackly-protocol` → render `--no-autostart`, draft lama tanpa field itu didecode dengan `false`, fixture `protocols/fixtures/instance/commands.json` dengan dua kasus baru (`autostart`, `no autostart against an at login default`) dan setiap draft membawa `noAutostart` → AC-18
- [x] `cargo test -p stackly-core --test create_evaluate` → `the_command_says_no_autostart_only_against_an_at_login_default` (manual lawan manual, manual lawan at_login, at_login, tanpa policy) → AC-18
- [x] `cargo test -p stackly-cli --test instance` → round trip perintah setara kembali ke draft yang sama untuk sembilan kasus fixture → AC-18
- [x] `cargo test -p stacklyd --test supervisor a_create_without_a_startup_takes_the_default…` → preflight lewat daemon: `--no-autostart` hanya saat default `at_login` dan draft `manual`, `--autostart` untuk `at_login`, tidak ada flag tanpa policy atau saat default `manual` → AC-18
- [x] `cargo xtask fuzz seed` → tidak ada seed yang berubah

## Milestone 3: Menu bar dan Stop All

Langkah build 9 (daemon dan CLI), 10 (scene `MenuBarExtra`, `MenuBarModel`, popover), dan 11 (`ActivationPolicyController`, notice tutup jendela, deteksi login item). Langkah manual GUI dilewati user, jadi AC yang butuh app yang berjalan (ikon di menu bar nyata, Quit saat instance jalan, login item nyata) menunggu langkah 14.

### UI / manual

- [x] Buka `crates/core/src/instance/stop_all.rs` → `confirmed_steps` membandingkan himpunan `stop` dari `select` dengan `expectedInstanceIds` sebagai himpunan (urutan dan ulangan tidak berarti), menolak dengan `StopAllChanged` kalau berbeda, dan mengembalikan irisan dalam urutan nama → AC-7, AC-8
- [x] Buka `apps/daemon/src/ipc/stop_all.rs` → urutan `replay_operation`, pilihan, `confirmed_steps`, baru `start_operation`; `instance.stop_all_changed` dikirim sebelum operasi dibuat, jadi kunci idempotency tidak terikat; field itu diurutkan dan dibuang ulangannya sebelum sidik jari → AC-7, AC-8
- [x] Jalankan `target/debug/stackly instance stop --help` → `--all`, dan `--yes` berlaku untuk `--force` dan `--all` → AC-9
- [x] Jalankan `target/debug/stackly instance stop --all` di terminal dengan dua instance berjalan → baris preview di stderr ("alpha · Started by you"), pertanyaan "Stop All Services?", dan kalimat "Data is kept. …"; `y` menghentikan, Enter menolak dengan exit 9 → AC-9

- [x] Buka `apps/macos/Packages/StacklyUI/Sources/StacklyUI/MenuBar/MenuBarRows.swift` → urutan empat grup, batas delapan, dan tabel *Tombol baris* dalam urutan spec (offline, operasi lain, dibuat, unknown, stopping, lalu Start atau Stop); baris kapabilitas start/stop belum bisa terjadi karena kontrak hanya punya `reload` (ditandai `provisional`) → AC-3, AC-4
- [x] Buka `MenuBar/MenuBarModel.swift` → Stop All membaca `instance.preview_stop_all`, mengirim `instance.stop_all` dengan `expectedInstanceIds` dari preview dan kunci baru, dan pada `instance.stop_all_changed` membaca preview lagi tanpa menghentikan apa pun; tidak ada request saat popover ditutup → AC-6, AC-7, AC-8, AC-10
- [x] Buka `Shell/ActivationPolicyController.swift` → jendela dihitung dari `didBecomeKey` dan `willClose` (panel, sheet, dan popover tidak dihitung); jendela terakhir tutup menjadi `accessory`, `prepareToShowWindow()` menjadi `regular` dan aktif; deteksi `keyAELaunchedAsLogInItem` dengan fallback jendela utama → AC-11, AC-12
- [x] Buka `apps/macos/design.md` bagian *Menu bar* dan *Activation policy and login item* → hasil pemeriksaan macOS 27 dari header SDK dan kode, dan apa yang belum terbukti → AC-12
- [ ] Jalankan `Stackly.app` Debug, lihat ikon menu bar tiga varian di bar terang dan gelap, buka popover, Stop All Services…, Settings… dari mode `accessory`, tutup jendela terakhir (ikon Dock hilang), Quit saat instance jalan → `BLOCKED` untuk sesi ini (user melewati langkah manual GUI); dicatat di langkah 14 → AC-1, AC-6, AC-10, AC-11

### Commands

- [x] `cargo test -p stackly-core --test stop_all_select` → enam test baru untuk `confirmed_steps`: tanpa field, himpunan sama dalam urutan lain dengan ulangan, instance yang bergabung sejak preview, yang keluar, `already_stopping` tidak dihitung, dan himpunan kosong → AC-7, AC-8
- [x] `cargo test -p stacklyd --test stop_all` → lima test di daemon dan filesystem asli: konfirmasi preview menghentikan persis isinya dan replay (urutan lain, dengan ulangan) menjawab sama; instance yang menyala sesudah preview ditolak `instance.stop_all_changed`, tidak ada yang berhenti, dan kunci yang sama dipakai lagi dengan preview baru; instance yang berhenti sesudah preview, ID bukan instance, dan himpunan kosong di atas instance berjalan ditolak; kunci yang sama dengan himpunan lain, atau tanpa field, ditolak `operation.idempotency_key_reused` → AC-7, AC-8
- [x] `cargo test -p stackly-cli --bin stackly stop` → teks preview, kaki, hasil, dan kegagalan mengikuti tabel *Copy Stop All* → AC-9
- [x] `cargo test -p stackly-cli --test instance stop_all` → `--yes` mengirim `expectedInstanceIds` dari preview dengan kunci; `--json` menulis satu envelope `{ preview, outcome }`; tidak ada yang berjalan menulis "Nothing is running." tanpa mengirim stop; tanpa `--yes` dan tanpa terminal (juga dengan `--json`) keluar 2 `cli.confirmation_required` sebelum stop dikirim; di terminal `y` menghentikan dan Enter keluar 9; `instance.stop_all_changed` keluar 3 tanpa mengulang; instance yang gagal berhenti disebut namanya dan keluar sesuai kodenya; `--all` menolak nama instance, `--force`, dan `--wait` → AC-8, AC-9
- [x] `swift test --package-path clients/swift --filter DaemonRestartClientTests` → `stopAll` mengirim `expectedInstanceIds` dengan kunci di bawah batas 190 detik, `previewStopAll` tanpa kunci, dan penolakan lewat apa adanya → AC-7, AC-8
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "MenuBarRowsTests|StopAllCopyTests|MenuBarModelTests|ActivationPolicyControllerTests"` → urutan grup dan tie ID, tabel tombol, prioritas ikon (offline, gagal, normal), template image tiga varian, persen dari progress, setiap baris *Copy Stop All*, alur Stop All (hasil, daftar berubah, penolakan lain, kosong, offline, batal), delapan baris dan "Show all 12 in Stackly…", kebijakan aktivasi dengan host palsu, pengenalan login item dari Apple event, dan antrean tujuan → AC-1, AC-3, AC-4, AC-6, AC-7, AC-8, AC-11, AC-12
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "AutostartTests|ShellStructureTests"` → toggle Autostart di Summary mengirim `instance.set_startup` dengan revision baris yang tampil (perbaikan F31, spec 0031 *GUI*), dan notice tutup jendela memakai kalimat menu bar → AC-11
- [x] `swift test --package-path clients/swift` dan `swift test --package-path apps/macos/Packages/StacklyUI` penuh, `xcrun swift-format lint --strict`, dan `cargo xtask build-app` → lihat laporan untuk angka dan test yang dikenal flaky
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, `cargo xtask codegen --check`, `cargo xtask evidence check`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → bersih

### Acceptance-criteria coverage

- AC-1 · tiga varian template image dan prioritas offline > gagal > normal (diuji); item selalu terpasang (`MenuBarExtra` tanpa `isInserted`); `applicationShouldHandleReopen` memanggil `prepareToShowWindow()`; ikon di bar nyata menunggu langkah 14
- AC-2, AC-3, AC-4 · header, delapan baris, urutan, teks Endpoint tabel Instances, tombol dan tooltip, label "last known" saat offline
- AC-5 · baris operasi dengan persen atau "Working…", hanya kind yang terdaftar, tanpa Cancel
- AC-6 · preview dan hasil di popover dengan *Copy Stop All*; Cancel tombol default lewat gaya `borderedProminent`, bukan shortcut, karena keputusan 2026-10-24 melarang `keyboardShortcut` (aturan sumber `NativeInteractionPolicyTests`)
- AC-10 · Quit memanggil `NSApp.terminate` dan tidak mengirim request; membuktikan instance tetap jalan menunggu langkah 14
- AC-11, AC-12 · kebijakan aktivasi, notice baru, deteksi login item dengan fallback; pendaftaran dan login nyata menunggu langkah 14 (`BLOCKED` tanpa signing identity)
- AC-7 · konfirmasi mengirim `expectedInstanceIds` dari preview, operasi hanya membuat langkah untuk irisan, hasil `StopAllResult` dengan `failed` dan `skipped`; popover menggambar hasil dari `StopAllResult`
- AC-8 · `instance.stop_all_changed` sebelum operasi dibuat, tanpa payload; CLI keluar 3 dan menyuruh menjalankan ulang; popover membaca preview lagi dan menggambar ulang
- AC-9 · `stackly instance stop --all [--yes] [--json]` dengan preview, konfirmasi, exit 2 tanpa terminal dan tanpa `--yes`, dan envelope `{ preview, outcome }`

### Catatan

- **Hasil kosong.** Kalau preview tidak punya `items`, CLI menulis "Nothing is running.", tidak meminta konfirmasi, dan tidak mengirim `instance.stop_all`; `outcome` null. Spec tidak menyebut kasus ini untuk CLI.
- **Keluar sesudah sebagian gagal.** `stop --all` keluar dengan kategori kode instance pertama di `failed` (misalnya `supervisor.stop_timeout`), atau `operation.cancelled` kalau Stop All dibatalkan. `skipped` biasa (lease environment, `unknown`) tidak membuat perintah gagal karena sudah tampil di preview. Spec hanya menetapkan exit 3 untuk `instance.stop_all_changed`; tidak ada kode baru yang ditambahkan.
- **Kirim ulang.** Pada `ipc.call_timeout` CLI mengirim ulang dengan kunci yang sama, sampai tiga kali (spec 0031, *Operasi `stop_all_instances`* langkah 9).
- **`--all` dan `--wait`.** Keduanya saling menolak, karena balasan `instance.stop_all` baru datang setelah operasi selesai.
- **Lokasi test daemon.** `apps/daemon/tests/stop_all.rs` memakai rig `Site` dan `Api` dari `tests/supervisor/support.rs` lewat `#[path]`, supaya tidak menyentuh `supervisor/main.rs` milik sesi lain. Test `supervisor/stop_all.rs` milik fitur 31 tetap menguji `instance.stop_all` tanpa field.
- **Berkas CLI.** `StopArgs` ada di `action.rs`, jadi flag `--all` dan penyaluran ke `stop.rs` mengubah `action.rs` dan `mod.rs` selain `stop.rs`.
- **Ikon.** Tiga template image digambar di kode (`MenuBarIcon`), bukan aset katalog: tiga batang bertumpuk, titik di pojok untuk gagal, garis miring untuk offline. Tidak ada file biner baru.
- **Antrean tujuan.** Klik nama service atau "Show all" menitipkan tujuan di `ShellDestinationQueue`; `AppShell` mengambilnya, juga saat jendela baru dibuka karena permintaan itu. `AppShell` dan `MainWindowRoot` berubah sedikit untuk ini.
- **Ke `AppDelegate`.** Tiga tambahan kecil: `applicationDidFinishLaunching` (mulai kebijakan aktivasi dan deteksi login item), `prepareToShowWindow()` di reopen, dan `menuBarActions`. Dilaporkan ke lead karena `AppDelegate` juga disentuh `dev-f32-m4`.
- **Fix F31.** `InstanceDetailService.setStartup` kini membawa `UInt64?` revision dan `InstanceDetailModel` mengirim revision summary yang diamati.
- **Client Swift.** `DaemonRestartClient` mendapat `previewStopAll()` dan `stopAll(expectedInstanceIds:idempotencyKey:)` (batas 190 detik, kunci wajib); `DaemonModel.stopAllClient` membukanya untuk menu bar.
- **Fixture dan snapshot.** Scenario `menubar-*` dan snapshotnya adalah langkah 13, bukan bagian ini.


## Milestone 4: Notifikasi

Langkah build 12. Bagian AC yang belum: izin notifikasi sungguhan di macOS (dan satu notifikasi sungguhan per pemicu), yang masuk evidence manual milestone 5 (langkah 14); menu bar dan Stop All (milestone 3).

### UI / manual

- [x] Buka `Notifications/NotificationPosting.swift` → port `NotificationPosting` (permission, request, post, openSystemSettings, installClickHandler), `NotificationTarget` (instance, operations, diagnostics) yang ikut sebagai `userInfo`, dan `AppNotification`; folder ini satu satunya yang `import UserNotifications` → AC-22
- [x] Buka `Notifications/UserNotificationPoster.swift` → poster nyata di atas `UNUserNotificationCenter`; center dibaca saat dipanggil dan tidak disimpan, jadi membuat poster di luar app bundle tidak menyentuh apa pun; delegate center menerima klik dan tidak menampilkan notifikasi saat app aktif → AC-22
- [x] Buka `Notifications/NotificationModel.swift` → empat pemicu sesuai tabel *Pemicu notifikasi*; setiap kiriman hanya bila toggle menyala, izin `authorized`, dan `NSApplication.isActive` false; gambaran lifecycle dan readiness diisi dari store sesudah setiap snapshot, jadi state yang sudah ada di snapshot tidak memicu dan pemulihan tidak pernah memicu → AC-22
- [x] Buka `Notifications/NotificationContent.swift` → judul, isi, dan identifier (`instance-failed-<id>-<revision>`, `instance-unready-<id>-<statusSeq>`, `operation-<id>`, `daemon-unreachable-<since>`); tanpa secret, path, atau isi log → AC-22
- [x] Buka `Settings/NotificationsSettingsRow.swift` → baris Notifications di tab Instances: toggle milik app (bisa diubah saat stacklyd offline), meminta izin sekali saat pertama dinyalakan, "Notifications are turned off for Stackly in System Settings." dengan Open Notification Settings kalau ditolak, izin dibaca ulang setiap app aktif dan saat tab terbuka → AC-22
- [x] Buka `Connection/ShellRuntime.swift`, `Shell/AppShell.swift`, `apps/macos/Stackly/AppDelegate.swift` → klik membuka jendela utama lewat `WindowOpener` lalu `AppShell` menampilkan halaman dari `NotificationModel.pending` (detail instance, Operations, atau Diagnostics); delegate dipasang di `applicationDidFinishLaunching` sebelum event pertama diproses → AC-22
- [x] Buka `Tests/StacklyUITests/SourceRuleTests.swift` → aturan baru: `UserNotifications` dan `UNUserNotificationCenter` hanya di `Notifications/` → AC-22

### Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter NotificationModelTests` → 36 test: keempat pemicu dan batasnya (satu per transisi, 5 menit per instance, lebih dari 10 detik dan hanya kind popover, 15 detik di luar jangkauan dan sekali per gangguan), tiga gerbang (toggle, izin, app aktif), izin tidak pernah diminta ulang dan penolakan tidak pernah memanggil `post`, klik, dan dua test yang memasang model ke `ShellSync` nyata → AC-22
- [x] `swift test … --filter AppPreferencesTests` dan `SettingsAvailabilityTests` → toggle mati secara default dan tersimpan, baris Notifications tampil di tab Instances → AC-22
- [x] `swift test … --filter SourceRuleTests` → aturan sumber spec 0012 untuk `UserNotifications` → AC-22
- [x] Semua test memakai `FakeNotificationPoster` atau `NotificationModel.inert`; tidak ada test yang meminta izin ke sistem → AC-22
- [ ] Izin notifikasi sungguhan (izinkan lalu tolak) dan satu notifikasi per pemicu di macOS → `BLOCKED` untuk sesi ini, langkah manual milestone 5 yang dilewati user (app tidak dibuka, dialog izin sistem tidak dipicu) → AC-22

### Acceptance-criteria coverage

- AC-22 · notifikasi mati secara default; menyalakan toggle meminta izin satu kali; sebelum izin tidak ada kiriman; izin ditolak tetap menyimpan pilihan dengan teks dan tombol Open Notification Settings; izin dibaca ulang setiap app aktif; empat pemicu saja dan hanya saat app tidak aktif; snapshot awal tidak memicu dan tidak ada notifikasi pemulihan; klik membuka detail instance, Operations, atau Diagnostics. Izin dan notifikasi sungguhan di macOS belum dicoba.

### Catatan

- **Durasi operasi.** `OperationSummary` tidak punya `finishedAt`; spec menulis `finishedAt − startedAt`. Model memakai `updatedAt` dari event ke state akhir (waktu perubahan state itu) dikurangi `startedAt`. Spec perlu mengikuti, atau kontrak menambah `finishedAt`.
- **Ringkasan hasil operasi.** Isi notifikasi operasi memakai kalimat tetap ("Took 42 seconds.", "Open Stackly to see what happened.", "It was cancelled after 1 minute.") dan tidak membawa teks error daemon, supaya tidak ada path atau secret yang ikut.
- **Readiness hilang.** Hanya perpindahan ke `not_ready` yang dihitung (bukan `unknown`), dan hanya saat lifecycle `running` menurut gambaran model sendiri; event `instance.health_changed` tidak membawa lifecycle.
- **Klik dan jendela.** Klik memanggil `WindowOpener.openMain()` dan `NSApplication.activate()`. Sesudah penggabungan dengan milestone 3, klik memanggil `ActivationPolicyController.prepareToShowWindow()` sebelum `openMain()`, jadi app kembali `regular` dan aktif walau jendela terakhir sudah ditutup (AC-11).
- **Pemakaian bersama dengan milestone 3.** `ShellRuntime`, `AppDelegate`, `AppShell`, dan `MainWindowRoot` mendapat diff kecil (parameter `notifications`, `applicationDidFinishLaunching`, `.onChange` untuk `pending`, `.environment`). Kalau `dev-f32-m3-s9` atau sesi menu bar menyentuh file yang sama, gabungkan keduanya.
- **Fixture.** Runtime fixture tidak memasang model notifikasi (`notifications: nil` memakai model inert yang tidak pernah mengirim), jadi tidak ada notifikasi saat scenario berjalan.

## Milestone 5: Tampilan dan bukti

Langkah build 13 (fixture dan snapshot) dikerjakan. Langkah 14 (bukti manual di Mac ini) dicatat saja dan tidak dijalankan: user melewati langkah manual GUI, dan Open Stackly at login `BLOCKED`.

### UI / manual

- [x] Buka `Sources/StacklyUI/Fixtures/FixtureScenario+MenuBarSettings.swift` → empat belas scenario *Fixture* spec dengan nama tetap: `menubar-normal`, `menubar-failed`, `menubar-offline-last-known`, `menubar-many` (12 instance), `menubar-empty`, `menubar-stop-preview` (dengan already stopping dan dua yang dilewati), `menubar-stop-partial` (dua gagal), `settings-general`, `settings-instances-denied`, `settings-sources`, `settings-storage-custom` (lokasi kustom, lokasi lama, satu tidak terjangkau), `settings-updates`, `settings-offline`, dan `settings-conflict` → AC-26
- [x] Buka `Fixtures/ShellBootstrap+Fixture.swift` → scenario memakai suite `UserDefaults` sendiri dan `FakeNotificationPoster`, jadi tidak membaca atau menulis defaults app dan tidak meminta izin ke macOS → AC-26, AC-22
- [x] Buka `Tests/StacklyUITests/SnapshotTests/__Snapshots__/MenuBarSnapshotTests/` dan `SettingsSnapshotTests/` → 28 PNG, satu light dan satu dark per scenario, sudah ditinjau satu per satu bentuknya (popover 340 pt, tab Settings 560 × 440 pt) → AC-26
- [ ] Izin notifikasi sungguhan (izinkan lalu tolak) → `BLOCKED` untuk sesi ini: langkah manual dilewati user, dialog izin sistem tidak dipicu. Perilaku izin dibuktikan oleh `FakeNotificationPoster` di milestone 4 dan snapshot `settings-instances-denied` → AC-22
- [ ] Menu bar light dan dark di bar sungguhan, termasuk item yang dilarang di System Settings > Menu Bar (jendela utama tetap terbuka dari Finder atau Launchpad lewat `applicationShouldHandleReopen`) → dilewati user (app tidak dibuka) → AC-1
- [ ] Quit Stackly saat instance berjalan (daemon, instance, dan operasi tetap hidup, `stackly daemon status` sehat, tidak ada request saat Quit) → dilewati user (app tidak dibuka) → AC-10
- [ ] Open Stackly at login sungguhan, dan peluncuran login item tanpa jendela → `BLOCKED`, reason `no code signing identity on this Mac (OQ-012)` → AC-11, AC-12

### Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "MenuBarSnapshotTests|SettingsSnapshotTests"` → 14 test, 28 gambar light dan dark cocok dengan referensi, dijalankan dua kali berturut turut tanpa beda → AC-26
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "FixtureScenarioTests|FixtureShellTests"` → katalog 35 scenario dengan nama tetap, setiap scenario baru berhenti di presence yang diharapkan (offline di dua scenario, terhubung di sisanya), dan setiap DTO round trip → AC-26
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` penuh, `xcrun swift-format lint --strict`, dan `cargo xtask build-app` → lihat laporan untuk angka dan test yang dikenal flaky

### Acceptance-criteria coverage

- AC-26 · empat belas scenario dan 28 snapshot light dan dark; popover dan tab Settings mengikuti spec dan mockup kecuali *Selisih mockup yang disengaja*
- AC-1, AC-10, AC-11, AC-12, AC-22 · sisi manual dicatat (dilewati atau `BLOCKED`) di atas; sisi kodenya dibuktikan di milestone 3 dan 4

### Catatan

- **Bentuk scenario.** Scenario Settings menjawab `settings.get` dan `storage.list_locations` lewat `replies`; `settings-conflict` menjawab `settings.update` dengan `resource.revision_mismatch`, dan snapshot nya diambil sesudah `SettingsStore.apply`. `settings-instances-denied` memakai izin `denied` dengan toggle menyala lewat dua field baru di `FixtureScenario` (`notificationPermission`, `notificationsEnabled`).
- **Popover.** `MenuBarPopover` mendapat init internal dengan `clearsFinishedStopAll`, supaya snapshot hasil Stop All tidak dihapus oleh `onAppear` yang membersihkan hasil lama saat popover dibuka lagi.
- **Tinggi snapshot Settings.** Tab digambar dalam ukuran jendela (560 × 440), jadi tab Storage yang lebih panjang terpotong di bawah seperti pada jendela sebelum di-scroll.
- **Perubahan di luar Fixtures.** `FixtureScenarioTests` dan `FixtureShellTests` menambah nama dan judul banner scenario baru (katalog spec 0068 dipin di sana).

