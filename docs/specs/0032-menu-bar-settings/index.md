# 0032. Menu bar dan Settings

**Date**: 2026-11-02
**Status**: Accepted

## Summary

Spec ini menetapkan ikon Stackly di menu bar macOS dan isi jendela Settings. Popover menu bar menampilkan status daemon, jumlah instance yang running dan failed, sampai delapan service dengan tombol start atau stop, progress operasi, serta **Stop All Services…** yang selalu menunjukkan daftar instance yang akan berhenti sebelum berjalan dan tidak pernah menghentikan instance di luar daftar itu; **Quit Stackly** hanya menutup GUI. Setting yang mengubah perilaku daemon (autostart default, lokasi data, log, dan cache, update channel, telemetry) disimpan daemon di registry dan bisa diubah dari GUI maupun CLI, sedangkan Appearance dan notifikasi disimpan app. Mengganti lokasi hanya berlaku untuk instance baru dan tidak pernah memindahkan data; baris setting milik fitur yang belum dibangun disembunyikan. Stop All, suppression, dan autostart memakai kontrak spec 0031, dan retention log tampil read only dari spec 0027.

## Requirements

**User stories**:

- Sebagai developer, saya ingin melihat dari menu bar apakah service saya hidup atau gagal, supaya saya tahu tanpa membuka jendela.
- Sebagai developer, saya ingin menyalakan atau mematikan satu service dari menu bar, supaya pekerjaan harian cepat.
- Sebagai developer, saya ingin menghentikan semua service sekaligus dengan tahu persis apa yang akan berhenti, supaya saya bisa membebaskan resource tanpa kejutan.
- Sebagai developer, saya ingin menutup GUI tanpa mematikan service, supaya database saya tetap jalan.
- Sebagai developer, saya ingin memilih folder lain di disk internal untuk data instance baru, supaya disk utama tidak penuh, tanpa memindahkan data yang sudah ada.
- Sebagai developer, saya ingin mengatur autostart default dan notifikasi di satu tempat, dan mendapat nilai yang sama dari CLI.
- Sebagai pengguna yang peduli privasi, saya ingin yakin bahwa Stackly tidak mengirim apa pun tanpa persetujuan saya.

**Acceptance criteria**:

*Menu bar*

- **AC-1**: Selama proses app hidup, app selalu memasang item menu bar Stackly (`MenuBarExtra` bergaya window, tanpa `isInserted` yang bisa dimatikan app). Kalau kamu sendiri melarang item itu di System Settings > Menu Bar, membuka Stackly dari Finder atau Launchpad tetap membuka jendela utama (`applicationShouldHandleReopen` dan peluncuran ulang). Ikon template monokrom punya tiga varian dengan prioritas: *offline* (presence bukan `connected`, termasuk incompatible, untrusted, dan recovery) > *ada yang gagal* (minimal satu instance dengan lifecycle `failed` di state terakhir) > *normal*. Varian gagal menambah titik sebagai bagian bentuk ikon dan varian offline memakai garis miring, jadi status tidak pernah hanya dibawa warna (REQ-027).
- **AC-2**: Header popover menampilkan "Stackly", teks status daemon yang sama dengan footer sidebar (spec 0013), "{n} running" (instance yang siap, aturan hitung kartu Running spec 0022), dan "{n} failed".
- **AC-3**: Bagian Services menampilkan paling banyak 8 instance dalam empat grup berurutan: (1) `failed`; (2) `starting`, `stopping`, `recovering`, `unknown`; (3) `running`; (4) `stopped`, `uninitialized`, `initializing`. Di dalam grup, urut nama dengan `localizedStandardCompare`, lalu ID kalau nama sama. Setiap baris menampilkan nama, label status spec 0012, dan teks kolom Endpoint tabel Instances spec 0022 apa adanya (termasuk bentuk tanpa endpoint dan banyak endpoint). Kalau instance lebih dari 8, baris "Show all {n} in Stackly…" membuka halaman Instances. Tanpa instance, bagian ini menulis "No instances yet." Klik nama baris membuka jendela utama di detail instance itu.
- **AC-4**: Tombol di setiap baris mengikuti tabel *Tombol baris*: menjalankan `instance.start` atau `instance.stop` yang sama dengan toolbar detail (stop manual spec 0031 AC-8, jadi suppression berlaku) tanpa konfirmasi, atau nonaktif dengan tooltip alasan. Saat offline, daftar menampilkan state terakhir dengan label "last known" dan semua tombol mutasi nonaktif.
- **AC-5**: Operasi yang sedang berjalan dengan kind di daftar *Operasi di popover* tampil sebagai satu baris dengan judul operasi dan persen, atau bar indeterminate dengan "Working…" kalau operasi tidak punya persen, dan hilang saat operasi selesai. Popover tidak menawarkan Cancel.
- **AC-6**: **Stop All Services…** membuka preview di dalam popover dari `instance.preview_stop_all` dengan copy tabel *Copy Stop All*: setiap instance yang akan berhenti dengan sumber demand nya, instance yang sudah berhenti sendiri ("already stopping"), instance yang dilewati beserta alasannya, "Data is kept.", dan kalimat login berikutnya. Cancel adalah tombol default. Kalau tidak ada yang akan berhenti, preview menulis "Nothing is running." dan Stop All nonaktif. Saat offline, Stop All Services… nonaktif.
- **AC-7**: Konfirmasi memanggil `instance.stop_all` spec 0031 (AC-10) dengan `expectedInstanceIds` dari preview. Operasi `stop_all_instances` hanya menghentikan irisan `expectedInstanceIds` dengan pilihan saat ini, jadi tidak pernah menghentikan instance yang tidak tampil di preview. Setiap langkah memakai stop manual spec 0031 (stop normal fitur 26 tanpa `SIGKILL`, plus suppression, jadi reconciler tidak menyalakannya lagi di sesi login yang sama). Popover menampilkan hasil dari `StopAllResult` dengan copy *Copy Stop All*, termasuk sebagian gagal dengan aksi Open Stackly.
- **AC-8**: Kalau himpunan instance yang akan dihentikan (himpunan `stop` dari `stop_all::select` spec 0031, jadi tanpa `alreadyStopping` dan `skipped`) berbeda dari `expectedInstanceIds`, daemon menolak dengan `instance.stop_all_changed` sebelum operasi dibuat, tanpa menghentikan apa pun. Error tidak membawa payload; popover memanggil `instance.preview_stop_all` lagi dan menggambar ulang preview untuk satu konfirmasi lagi.
- **AC-9**: `stackly instance stop --all` memanggil preview, mencetaknya, meminta konfirmasi di TTY (`--yes` melewatinya; tanpa TTY dan tanpa `--yes` keluar 2 dengan `cli.confirmation_required`), lalu mengirim `expectedInstanceIds` yang sama. `instance.stop_all_changed` keluar 3. `--json` menulis satu envelope spec 0008 dengan `result` `{ preview: StopAllPreview, outcome: StopAllResult | null }`.
- **AC-10**: **Quit Stackly** di popover dan ⌘Q di menu app (yang hanya ada selama app `regular`) hanya menutup GUI lewat jalur Quit spec 0013. Daemon, setiap instance, dan setiap operasi yang berjalan tetap hidup, dan tidak ada request yang dikirim ke daemon saat Quit.

*Dock, login, dan jendela*

- **AC-11**: Saat jendela terakhir milik app (jendela utama, Settings, atau galeri Debug) tertutup, app berpindah ke activation policy `accessory`, jadi ikon Dock dan menu app hilang; jendela yang diminimalkan atau berada di Space lain dihitung terbuka. Sebelum jendela mana pun dibuka (dari popover, notifikasi, atau peluncuran ulang), app kembali ke `regular` dan aktif. Selama `accessory`, Settings dan Quit dijangkau dari popover. Notice tutup jendela spec 0013 sekarang menulis "Stackly is still running" dan "Closing the window doesn’t stop stacklyd or your instances. Use the menu bar icon to reopen it."
- **AC-12**: Toggle **Open Stackly at login** (General, default mati) mendaftarkan atau mencabut app sebagai login item lewat `SMAppService.mainApp`, dan state nya selalu dibaca dari OS, termasuk `requiresApproval` dengan aksi Open Login Items. Peluncuran yang dikenali sebagai login item membuka app dalam mode `accessory` tanpa jendela; kalau peluncuran tidak bisa dikenali, app membuka jendela utama seperti biasa. Toggle ini terpisah dari **Start stacklyd at login**; mematikannya tidak memengaruhi daemon atau instance.

*Settings*

- **AC-13**: Jendela Settings (scene `Settings`, ⌘, dari spec 0069 saat app `regular`, Settings… dari popover kapan saja) punya tab General, Instances, Sources, Storage, dan Updates dengan baris di tabel *Baris Settings*. Baris milik fitur yang belum dibangun (Restart Daemon fitur 30, Clean Up cache fitur 55, sumber community dan import fitur 37) tidak tampil. Update channel dan telemetry tampil sebagai preferensi tersimpan dengan catatan yang jujur tentang efeknya hari ini. `SettingsPlaceholder` dihapus.
- **AC-14**: Appearance menawarkan System, Light, dan Dark dengan default System. Pilihan langsung berlaku untuk semua jendela app dan popover, dan bertahan setelah app diluncurkan ulang. Disimpan di UserDefaults app, bukan di daemon.
- **AC-15**: Baris **Start stacklyd at login** memakai `BackgroundAgentModel` spec 0015 dengan enam state, copy tabel *Banner per state* spec 0015, dan Open Login Items. Menyalakan toggle memanggil register (spec 0015). Mematikannya menjalankan urutan *Matikan agent* spec 0031 (AC-20): konfirmasi "Turn Off Background Agent?" dengan copy spec 0031, `daemon.shutdown { mode: agent_off }`, lalu unregister hanya kalau daemon melaporkan `exiting: true`. Di bawahnya, satu kalimat menjelaskan login berikutnya mengikuti *Batas disruption* spec 0031: "At your next login, stacklyd starts, then {m} instances set to start at login." (`m` = instance dengan policy `at_login`), atau teks "Won’t start at login…" spec 0031 kalau agent tidak `enabled`.
- **AC-16**: `settings.get` mengembalikan semua setting daemon beserta `revision`. `settings.update` menerima patch sebagian (`defaultStartup`, `updateChannel`, `telemetry`) dengan `expectedRevision`. Patch kosong atau berisi nilai `Unrecognized` ditolak `request.invalid_params`. Patch yang semua nilainya sama dengan yang tersimpan membalas view yang tidak berubah tanpa operasi dan tanpa event. Selain itu update berjalan sebagai operasi `update_daemon_settings` lewat `commit_guarded`, menaikkan `revision`, dan mengirim `settings.changed { revision }`. Client yang menerima event itu, atau `events.gap`, atau tersambung ulang, memanggil `settings.get` lagi, jadi Settings yang terbuka memperbarui nilainya tanpa dibuka ulang. `stackly settings get [--json]` dan `stackly settings set <key> <value>` setara dengan GUI.
- **AC-17**: `expectedRevision` yang usang ditolak dengan `resource.revision_mismatch`: sinkron di precheck kalau sudah usang saat request tiba, atau sebagai operasi `failed` dengan kode yang sama kalau berubah di antara precheck dan guard. Tidak ada yang ditulis. GUI lalu menampilkan nilai terbaru di baris itu dengan catatan "Changed elsewhere. Try again." `stackly settings set` tidak mengulang sendiri; ia keluar 3 dengan "Settings changed while saving. Run the command again."
- **AC-18**: **Autostart new instances** (Instances, default mati, disimpan sebagai `default_startup` `manual`) menentukan policy `startup` spec 0031 untuk instance baru. `instance.create` tanpa `startup` memakai `default_startup` yang dibaca di dalam guard create. Reply form create dan duplicate (spec 0020, 0024) mendapat `startupDefault`, yang mengisi awal toggle Start at login di sheet New Instance dan Duplicate. `stackly instance create` tanpa `--autostart` atau `--no-autostart` tidak mengirim `startup`. Mengubah default tidak mengubah instance yang sudah ada.
- **AC-19**: **Change…** untuk Instance data, Logs, atau Download cache membuka panel folder, menampilkan konfirmasi "Only instances you create from now on use this folder. Existing data stays where it is." (untuk cache: "New downloads go to this folder. Downloads already saved stay where they are."), lalu menjalankan `storage.set_location`. Daemon memeriksa *Aturan lokasi* berurutan dan menolak aturan pertama yang gagal dengan `storage.location_invalid` (detail `rule`, `path`), dan GUI menampilkan kalimat aturan itu. Folder yang lolos mendapat subfolder `Stackly` dengan mode 0700 dan layout *Layout lokasi*, menjadi lokasi default untuk jenisnya, dan dipakai create (data dan log) atau install runtime (download) berikutnya. **Use Default** (tampil saat lokasi default bukan bawaan) mengembalikan default ke lokasi bawaan. Baris path instance yang sudah ada tidak berubah dan tidak ada data yang dipindah atau dihapus. Lokasi data dan log bukan default yang masih dipakai tampil sebagai "Also used by {n} instance(s)" (lokasi dengan 0 pemakai disembunyikan tapi tidak dihapus); lokasi cache lama tampil sebagai "No longer used for new downloads". Lokasi yang foldernya hilang tampil "Not reachable". `stackly storage locations` dan `stackly storage set-location <data|logs|cache> <path|--default>` setara dengan GUI.
- **AC-20**: Start sebuah instance memeriksa lokasi milik instance itu sendiri (bukan default sekarang) sebelum cek data spec 0025 dan sebelum proses dijalankan. Folder lokasi yang tidak ada gagal dengan `instance.storage_location_missing` (detail `kind`, `path`), dan folder yang ada tapi tidak bisa ditulis gagal dengan `storage.location_invalid` rule `not_writable`. Preflight create menambah baris `storage_location` untuk lokasi default data dan log dengan dua kegagalan yang sama dan kalimat *Aturan lokasi*.
- **AC-21**: Grup Logs di tab Storage menampilkan baris read only "Instance logs" dengan "Kept per stream: {segmentsPerStream} files × {segmentMaxBytes}. Older lines are removed as new ones arrive.", dengan angka dari `DaemonSettingsView.logRetention` yang daemon isi dari konstanta spec 0027. Tidak ada setting retention.
- **AC-22**: Notifikasi mati secara default. Menyalakan toggle meminta izin notifikasi macOS satu kali; sebelum izin diberikan tidak ada notifikasi yang dikirim. Kalau izin ditolak, toggle tetap menyimpan pilihanmu dan baris itu menulis "Notifications are turned off for Stackly in System Settings." dengan tombol Open Notification Settings; izin dibaca ulang setiap kali app aktif. Dengan izin dan toggle menyala, app mengirim tepat empat pemicu di tabel *Pemicu notifikasi* selama proses app hidup dan hanya saat app tidak aktif. State yang sudah ada di snapshot awal tidak memicu notifikasi, dan tidak ada notifikasi pemulihan. Klik notifikasi membuka detail instance, halaman Operations, atau halaman Diagnostics.
- **AC-23**: Telemetry tersimpan mati secara default dan tidak ada data yang dikirim ke mana pun, apa pun nilainya; fitur ini tidak menambah satu pun panggilan jaringan. Catatan barisnya: "Stackly doesn’t collect usage statistics yet. Your choice is saved for when it does. Diagnostic bundles are never uploaded automatically." Bersama nilainya disimpan `telemetry_copy_version` 1, supaya fitur telemetry nanti meminta persetujuan lagi kalau teksnya berubah. Update channel tersimpan dengan catatan "Saved now. Used once Stackly can update itself. App updates never touch instance data." (REQ-046, NFR-016).
- **AC-24**: Saat daemon tidak terjangkau, setiap baris milik daemon (Instances, Sources, Storage, Updates, dan baris stacklyd di General) menampilkan nilai terakhir yang diketahui di sesi app ini dengan label "last known" dan nonaktif, di bawah banner "Can’t connect to stacklyd. Showing the last known settings."; status lokasi menulis "Unknown while stacklyd is offline"; kalau belum pernah tersambung di sesi ini, baris itu menulis "Not available while stacklyd is offline." Appearance, notifikasi, dan Open Stackly at login tetap bisa diubah. Tidak ada perubahan yang diantrekan.

*Data dan tampilan*

- **AC-25**: Migration registry `NNNN_settings_locations.sql` (nomor bebas berikutnya saat build) menambah `daemon_settings` dan `storage_locations`, membangun ulang `instance_paths` dengan `location_id` yang menunjuk lokasi bawaan untuk setiap baris data dan log yang ada, lolos `PRAGMA foreign_key_check` dan cek jumlah baris sebelum commit, dan memakai snapshot `registry/pre-migration` spec 0005. Setelah migration, setiap path instance resolve ke path yang sama persis seperti sebelumnya. Migration yang gagal di tengah tidak mengubah registry. Spec 0005 AC-10 berlaku dalam bentuk *AC-10 per lokasi*.
- **AC-26**: Popover dan Settings mengikuti mockup UI kecuali *Selisih mockup yang disengaja*, dengan fixture spec 0068 dan snapshot light dan dark untuk setiap scenario di *Fixture*.

## Decision

**Chosen option**: rekomendasi di setiap keputusan di [rationale.md](rationale.md), semuanya dipilih engineer: setting dibagi menurut pemilik (daemon untuk perilaku service, app untuk tampilan), lokasi data, log, dan cache bisa diganti untuk instance baru lewat tabel `storage_locations`, ikon Dock hilang saat jendela terakhir ditutup, `MenuBarExtra` bergaya window, Stop All memakai `instance.stop_all` spec 0031 ditambah preview dan `expectedInstanceIds`, retention log read only dari spec 0027, dan semua perbaikan cross check.

**Implementation skills**: `swiftui-expert-skill` (`.claude/skills/swiftui-expert-skill/`) untuk scene `MenuBarExtra`, `Settings`, dan state `@Observable` · `swift-concurrency` (`.claude/skills/swift-concurrency/`) untuk model notifikasi dan login item di luar main actor · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) untuk use case dan registry.

## Feature design

### Trace PRD

| ID | Bagian di spec ini |
|---|---|
| REQ-027 | menu bar status dasar (AC-1 sampai AC-8), notifikasi opsional yang mengikuti izin (AC-22) |
| REQ-046 | telemetry dan upload mati tanpa consent (AC-23) |
| PAGE-14 | Settings (AC-13 sampai AC-21, AC-23, AC-24), bagian yang tersedia di fase 04 |
| NFR-016 | tidak ada telemetry atau upload secara default, tanpa panggilan jaringan baru (AC-23) |

### Prasyarat dan batas dengan spec lain

| Yang dibutuhkan | Dari | Dipakai untuk |
|---|---|---|
| `instance.stop_all`, `StopAllResult`, operasi `stop_all_instances`, `stop_all::select`, `instance.preview_stop_all` dengan DTO `StopAllPreview`, stop manual dengan suppression, policy `startup`, `InstanceAutostart`, `agent_off`, *Batas disruption*, copy autostart | spec 0031 (Proposed, langkah 9 dan 12) | AC-4, AC-6 sampai AC-9, AC-15, AC-18 |
| Stop normal dan serialisasi per instance | fitur 26 | AC-4, AC-7 |
| Konstanta `LOG_SEGMENTS_PER_STREAM`, `LOG_SEGMENT_MAX_BYTES`, `LogRetention` | spec 0027 | AC-21 |
| `BackgroundAgentModel`, `SMAppServiceAgent`, copy state agent | spec 0015 (milestone 1 sampai 3) | AC-15 |
| Scene `Window`, `Settings`, Quit, `WindowOpener`, `CloseNotice`, presence, `OperationCenter`, Diagnostics | spec 0013 | AC-2, AC-5, AC-10, AC-11, AC-22 |
| Command map, ⌘, | spec 0069 | AC-13 |
| Label status, pola konfirmasi | spec 0012 | AC-3, AC-6 |
| Hitungan Running dan teks endpoint | spec 0022 | AC-2, AC-3 |
| Form create, preflight, `Filesystem::volume_facts` | spec 0020, 0024 | AC-18 sampai AC-20 |
| Envelope JSON dan exit code dari kategori | spec 0008 | AC-9, AC-17 |

Fitur 32 dibangun setelah fitur 26 dan 31 (milestone `instance.stop_all` dan `startup`).

Yang **bukan** bagian spec ini: Restart Daemon (fitur 30), Clean Up cache (55), community dan import provider (37), mekanisme update (61), telemetry nyata (belum ada fitur), retention log yang bisa diatur, dampak dan override lease environment di Stop All (fitur 47), memindahkan data instance antar lokasi, menghapus lokasi lama.

### Tambahan pada kontrak spec 0031

**Diterapkan di spec 0031 (2026-11-02)**, bagian *Pilihan stop all*, *Operasi `stop_all_instances`*, dan *Tambahan dari spec 0032*. Aturan pilihan yang berlaku ada di spec 0031, yang menajamkannya untuk start dan restart yang terbuka dan untuk operasi `needs_recovery`, dan fitur 31 juga membangun `instance.preview_stop_all` (untuk konfirmasi Turn Off) serta cancel pada `stop_all_instances`.

Spec 0031 masih Proposed, jadi tambahan ini masuk ke kontraknya sebelum dibangun (*Usulan update untuk spec lain*):

- Fungsi murni `stackly_core::instance::stop_all::select(&RegistryView) -> StopAllSelection { stop, already_stopping, skipped }` adalah satu satunya aturan pilihan, dipakai preview, `instance.stop_all`, dan `agent_off`. `stop` berisi instance yang desired nya `running` atau punya proses hidup, kecuali yang lifecycle nya `stopping` (ke `already_stopping`) dan yang `unknown` atau berlease environment `active` (ke `skipped` dengan kode spec 0031).
- `instance.stop_all` mendapat `expectedInstanceIds: [Uuid]` opsional (`#[serde(default)]`). Kalau ada: precheck (setelah `replay_operation`) membandingkan himpunan `stop` dengan himpunan itu dan menolak `instance.stop_all_changed` kalau berbeda; operasi hanya membuat langkah untuk irisan keduanya. Tanpa field itu, perilaku tetap spec 0031 (dipakai `agent_off`). Fingerprint idempotency request (spec 0007) ikut mencakup field ini.
- `operation.cancel` pada `stop_all_instances` berhenti memulai langkah baru; langkah yang sedang berjalan selesai, dan sisanya masuk `skipped` dengan `operation.cancelled`.
- `instance.create` tanpa `startup` memakai `daemon_settings.default_startup`, bukan `manual` tetap; CLI menambah `--no-autostart`.

### Alur

**Stop All**: popover → `instance.preview_stop_all` → preview inline → Stop All → `instance.stop_all { idempotencyKey, expectedInstanceIds }` → operasi `stop_all_instances` → progress di popover → hasil dari `StopAllResult`, atau `instance.stop_all_changed` lalu preview baru.

**Ganti lokasi**: Storage → Change… → `NSOpenPanel` (pilih satu folder) → konfirmasi → `storage.set_location { kind, target: path, expectedRevision }` → precheck revision → operasi `set_storage_location`: periksa *Aturan lokasi* berurutan, buat `<folder>/Stackly` (0700) dan subfolder layout nya, tulis `.stackly-location` berisi ID lokasi, lalu `commit_guarded` (cek revision lagi, tambah lokasi atau pakai yang sama, jadikan default) → `settings.changed` → Settings memanggil `settings.get` dan `storage.list_locations`.

### Model data

Migration `NNNN_settings_locations.sql` (nomor bebas berikutnya saat build; 0007 sampai 0009 sudah dipakai spec 0028, 0029, dan 0030, dan spec 0031 juga merencanakan migrasi, jadi siapa pun yang build lebih dulu memakai nomor itu).

**`storage_locations`**

| Kolom | Tipe | Aturan |
|---|---|---|
| `id` | TEXT, UUID | PK |
| `kind` | TEXT | `data`, `logs`, atau `cache` (CHECK) |
| `path` | TEXT, nullable | path canonical UTF-8 subfolder `Stackly`; NULL kalau dan hanya kalau `built_in = 1` (CHECK), karena lokasi bawaan resolve lewat `RootSet` dan `STACKLY_ROOT` tetap bekerja |
| `built_in` | INTEGER 0/1 | tepat satu baris bawaan per `kind` (unique index parsial `(kind) WHERE built_in = 1`) |
| `created_at` | TEXT RFC 3339 | |

Unique `(kind, path) WHERE built_in = 0`. Baris tidak pernah diubah atau dihapus oleh fitur ini (tidak ada WriteOp untuk itu). ID tetap lokasi bawaan: data `00000000-0000-7000-8000-000000000001`, logs `00000000-0000-7000-8000-000000000002`, cache `00000000-0000-7000-8000-000000000003`, sebagai konstanta `BUILT_IN_LOCATION_IDS` di `stackly-domain` dan di migration.

**`daemon_settings`** (tepat satu baris, `CHECK (id = 1)`)

| Kolom | Tipe | Default |
|---|---|---|
| `id` | INTEGER | 1 |
| `default_startup` | TEXT, CHECK `IN ('manual','at_login')` | `manual` |
| `update_channel` | TEXT, CHECK `IN ('stable','beta')` | `stable` |
| `telemetry` | INTEGER 0/1 | 0 |
| `telemetry_copy_version` | INTEGER | 1 |
| `data_location_id`, `logs_location_id`, `cache_location_id` | TEXT FK → `storage_locations.id` `ON DELETE RESTRICT` | ID bawaan per jenis |
| `revision` | INTEGER, `Revision` | 1 |
| `updated_at` | TEXT RFC 3339 | waktu migration |

Trigger `BEFORE UPDATE` menolak lokasi yang `kind` nya tidak sama dengan kolomnya; guard registry memeriksa hal yang sama dengan error yang jelas.

**`instance_paths`** (dibangun ulang, tetap `WITHOUT ROWID`, PK `(instance_id, kind)` tetap):

| Kolom | Perubahan |
|---|---|
| `instance_id`, `kind`, `relative_path`, `managed` | tetap, nilai disalin apa adanya |
| `logical_root` | tetap `data`, `logs`, atau `runtime` sesuai kind; untuk baris berlokasi ia menyatakan jenis lokasi |
| `location_id` | baru, TEXT FK → `storage_locations.id` `ON DELETE RESTRICT`; NOT NULL untuk kind `root`, `data`, `config`, `config_history`, `temp` (lokasi `data`) dan `logs` (lokasi `logs`); NULL untuk `run` |

Unique index lama `(logical_root, relative_path)` diganti dua index parsial: `(location_id, relative_path) WHERE location_id IS NOT NULL` dan `(logical_root, relative_path) WHERE location_id IS NULL`. Trigger `BEFORE INSERT` menolak `location_id` yang `kind` lokasinya tidak cocok dengan `logical_root`, dan `CHECK ((location_id IS NULL) = (kind = 'run'))`. Satu instance selalu punya satu lokasi data untuk kelima kind data nya (dijaga `InsertInstance`, yang menulis ketujuh baris dari satu pasangan lokasi). Baris tidak pernah diubah setelah create.

**UserDefaults app** (bukan registry): `appearance` (`system`, `light`, `dark`; default `system`) dan `notificationsEnabled` (default false). Status Open Stackly at login tidak disimpan app; selalu dibaca dari `SMAppService.mainApp.status`.

Registry, provider package, runtime terpasang, dan log daemon tetap di root bawaan dan tidak pernah pindah.

### Layout lokasi

| Jenis | Isi `<folder>/Stackly/` | Path instance |
|---|---|---|
| data | `instances/` | `instances/<id>/`, `instances/<id>/data`, `config`, `config-history`, `temp`, seperti root bawaan |
| logs | `instances/` | `instances/<id>` |
| cache | `downloads/` | `DirectoryLayout::downloads_dir_in(&location)` |

Setiap jenis juga punya `.stackly-location` (JSON `{ "id": "<uuid>", "kind": "<jenis>" }`, mode 0600).

### AC-10 per lokasi

Pengganti kalimat spec 0005 AC-10 tentang path: baris path dihitung registry sendiri lewat `stackly_platform::instance_path_entries(id, data, logs)`, relatif terhadap lokasinya (atau runtime root untuk `run`), dan `LocationSet::resolve` atas setiap baris sama persis dengan `DirectoryLayout::instance_paths_in(id, data, logs)`. Untuk 1000 instance yang tersebar di lokasi bawaan dan dua lokasi kustom, tidak ada path tersimpan yang sama dengan atau berada di bawah path instance lain, dan karena lokasi tidak pernah bertumpuk (*Aturan lokasi*), hal itu juga berlaku antar lokasi. Pasangan (lokasi, path relatif) yang sama ditolak `PathTaken`.

### Perubahan domain dan platform

- `stackly-domain`: `DaemonSettings`, `SettingsPatch`, `StorageLocation`, `StorageLocationId`, `LocationKind`, `StartupPolicyKind` (dari spec 0031), `UpdateChannel`, `BUILT_IN_LOCATION_IDS`, dan `InstancePathEntry.location: Option<StorageLocationId>`.
- `stackly-platform`: `instance_path_entries(id, data, logs)` menggantikan versi satu argumen; `LocationSet::resolve(&InstancePathEntry) -> PathBuf`; `DirectoryLayout::instance_paths_in` dan `downloads_dir_in`; `VolumeFacts` mendapat `internal: bool` (volume di dalam Mac ini, bukan removable atau external). Adapter macOS membaca fakta itu di modul `fs` nya; kalau butuh DiskArbitration, `unsafe` tetap hanya di `stackly-platform-macos` dengan `// SAFETY:`.
- `inventory_instances` (spec 0004) memindai `instances/` di setiap lokasi data yang terjangkau.
- **Pemakai path instance yang harus pindah ke `LocationSet`** (langkah 3 mencari semua pemanggil `DirectoryLayout::instance_paths` dan `RootSet::resolve` untuk path instance dengan grep dan mencatat daftarnya di commit): `InsertInstance` di storage dan `FakeRegistry`, create dan init (spec 0020), run plan dan start (spec 0010, 0021), sink log (spec 0027), `dataPath` di form dan detail (spec 0020, 0021), inventory (spec 0004), dan kompensasi `remove_owned_tree` (spec 0007).
- **WriteOp baru**: `UpdateDaemonSettings { patch, expected }`, `InsertStorageLocation(StorageLocation)`, `SetDefaultLocation { kind, id, expected }`. `InsertInstance` mengambil lokasi data dan log serta `default_startup` dari baris settings di dalam guard yang sama, jadi create yang bersamaan dengan ganti lokasi memakai salah satu pasangan secara konsisten, tidak pernah campuran. **Baca baru**: `Registry::daemon_settings()`, `storage_locations()`, `location_usage() -> Vec<(StorageLocationId, Count)>`.

### Aturan lokasi

Fungsi murni `stackly_core::storage::location::check_location(facts, roots, existing) -> Result<(), LocationRule>`, dengan fakta dari `Filesystem::file_info`, `Filesystem::volume_facts`, dan `UserIdentity`, diperiksa tepat dalam urutan tabel; aturan pertama yang gagal dilaporkan. Path dibandingkan dalam bentuk canonical (`std::fs::canonicalize` di adapter).

| Urutan | `rule` | Kapan | Kalimat |
|---|---|---|---|
| 1 | `not_absolute` | path relatif | "Choose a full folder path." |
| 2 | `missing` | folder tidak ada | "This folder doesn’t exist." |
| 3 | `not_directory` | bukan folder | "Choose a folder, not a file." |
| 4 | `symlink` | path atau leluhur mana pun di bawah `/` (kecuali `/private` milik sistem yang menjadi target `/var` dan `/tmp`) adalah symlink | "Choose the real folder, not an alias or link." |
| 5 | `not_owned` | pemilik bukan user daemon | "This folder belongs to another user." |
| 6 | `too_open` | folder itu, atau leluhurnya yang tidak sticky, bisa ditulis group atau other | "Other users can write to this folder." |
| 7 | `protected_folder` | di dalam `~/Desktop`, `~/Documents`, `~/Downloads`, `~/Library/Mobile Documents`, atau `~/Library/CloudStorage` | "macOS may block stacklyd from this folder. Choose a folder in your home folder or on a disk inside this Mac." |
| 8 | `not_local_volume` | `VolumeFacts.local` false | "Choose a folder on a disk in this Mac." |
| 9 | `not_internal_volume` | `VolumeFacts.internal` false (disk external, termasuk SSD external) | "Choose a folder on a disk inside this Mac. External disks aren’t supported yet." |
| 10 | `overlaps_root` | `<folder>/Stackly` sama dengan, di dalam, atau berisi root bawaan, runtime root, bundle app, atau lokasi lain jenis apa pun | "This folder overlaps another Stackly location." |
| 11 | `foreign_marker` | `<folder>/Stackly` sudah ada, dan tidak berisi `.stackly-location` dengan ID dan `kind` lokasi yang dikenal registry | "This folder already has a Stackly folder that isn’t from this Mac’s Stackly." |
| 12 | `not_writable` | daemon gagal membuat `Stackly/` atau subfolder nya, atau folder itu sudah ada tetapi bukan folder privat user (mode & 0o077, pemilik lain, atau symlink) | "stacklyd can’t write to this folder." |

`<folder>/Stackly` dengan `.stackly-location` yang ID dan `kind` nya cocok dengan baris registry berarti lokasi itu dipakai lagi. `target: builtIn` melewati aturan dan menunjuk lokasi bawaan.

### Method IPC

Protocol naik mayor ke 3.0 di langkah 1, bukan satu minor seperti rencana awal: event `settings.changed` menambah varian ke `EventKind`, yang tertutup (doc di `crates/protocol/src/events.rs`, preseden spec 0030 AC-18), jadi client 2.x gagal decode event itu, dan aturan AGENTS.md untuk perubahan yang memutus berlaku (keputusan lead 2026-11-03). Sesudahnya F31 milestone 4 menambah satu minor, jadi `main` kini 3.1. Fixture dan digest dibuat ulang lewat `cargo xtask codegen`. Semua method baru `recovery refused`.

| Method | Jenis | Input | Output | Error |
|---|---|---|---|---|
| `settings.get` | baca | none | `DaemonSettingsView` | `ipc.*` |
| `settings.update` | operasi `update_daemon_settings` (`replay_operation`, precheck revision, `start_operation`) | `idempotencyKey`, `expectedRevision: Revision`, `patch: SettingsPatch` | `DaemonSettingsView` sesudah operasi selesai, bukan `OperationView` (pola `instance.set_startup`); `DaemonSettingsView` tanpa operasi untuk patch tanpa perubahan (`SettingsCheck::Unchanged`) | `resource.revision_mismatch`, `request.invalid_params` |
| `storage.list_locations` | baca | none | `LocationList { items: [StorageLocationView] }` | |
| `storage.set_location` | operasi `set_storage_location` | `idempotencyKey`, `kind: LocationKind`, `target: LocationTarget`, `expectedRevision` | `StorageLocationView` lokasi default jenis itu sesudah operasi selesai, bukan `OperationView` | `resource.revision_mismatch`, `storage.location_invalid` |
| `instance.preview_stop_all` (spec 0031, dibangun fitur 31 langkah 9) | baca | none | `StopAllPreview` | |
| `instance.stop_all` (spec 0031, diperluas) | operasi `stop_all_instances` | `idempotencyKey`, `expectedInstanceIds` opsional | `StopAllResult`, setelah operasi selesai (spec 0031 *Operasi `stop_all_instances`*); progres lewat event operasi | `instance.stop_all_changed` |

Event baru `settings.changed { revision: Revision }`, dikirim hanya setelah commit yang menaikkan revision.

**DTO** (camelCase, `Unrecognized` di setiap open enum, field opsional `#[serde(default)]`, counter memakai newtype `wire.rs`):

- `DaemonSettingsView { revision: Revision, defaultStartup: StartupPolicyKind, updateChannel: UpdateChannel, telemetry: bool, telemetryCopyVersion: Count, locations: DefaultLocations { data, logs, cache: StorageLocationView }, logRetention: LogRetention, updatedAt: Timestamp }`; `LogRetention` adalah tipe spec 0027.
- `SettingsPatch { defaultStartup?: StartupPolicyKind, updateChannel?: UpdateChannel, telemetry?: bool }`; `Validate` menolak patch tanpa field dan nilai `Unrecognized`.
- `StorageLocationView { id: Uuid, kind: LocationKind, path: String, builtIn: bool, isDefault: bool, instanceCount: Count, reachable: bool }`, dengan `path` selalu path yang sudah resolve dan `reachable` dari `file_info` saat dibaca; `instanceCount` 0 untuk cache.
- `LocationKind`: `data`, `logs`, `cache`, `Unrecognized`. `LocationTarget`: `{ "type": "path", "content": String }` atau `{ "type": "builtIn" }`. `UpdateChannel`: `stable`, `beta`, `Unrecognized`.
- `StopAllPreview { items: [StopAllItem], alreadyStopping: [InstanceRef], skipped: [StopAllSkip], atLoginCount: Count }`, setiap daftar urut nama lalu ID; `atLoginCount` = jumlah `items` dengan policy `at_login`. DTO preview di bawah ini dibangun fitur 31 langkah 9 sesuai definisi di sini.
- `StopAllItem { instanceId: Uuid, name: String, lifecycle: InstanceLifecycle, demand: [DemandSourceKind], autostart: InstanceAutostart }`, dengan `DemandSourceKind` (`manual`, `startup`, `environment`, `Unrecognized`) dari lease `active` spec 0031.
- `InstanceRef { instanceId: Uuid, name: String }`. `StopAllSkip { instanceId: Uuid, name: String, code: String }`.
- Form create dan duplicate (spec 0020, 0024) mendapat `startupDefault: StartupPolicyKind`, `dataLocation: String`, dan `logsLocation: String`; `dataPath` spec 0020 tetap path data instance yang akan dibuat, sekarang dihitung dari `dataLocation`.

**Kode error baru** di `codes` (kategori menentukan exit code CLI spec 0008):

| Kode | Kategori | Exit | Detail |
|---|---|---|---|
| `storage.location_invalid` | `invalid_input` | 2 | `StorageLocationIssue { rule: LocationRule, path: String }` |
| `instance.stop_all_changed` | `conflict` | 3 | none |
| `instance.storage_location_missing` | `unavailable` | 6 | `MissingLocation { kind: LocationKind, path: String }` |

`resource.revision_mismatch` dan `cli.confirmation_required` sudah ada. Fixture `protocols/fixtures/` ditambah untuk setiap DTO, event, dan error, dan seed fuzz dibuat ulang (`cargo xtask fuzz seed`).

### CLI

| Perintah | Perilaku |
|---|---|
| `stackly settings get [--json]` | satu baris per setting, plus lokasi default per jenis dan retention log |
| `stackly settings set <key> <value>` | key `default-autostart` (`on`/`off`), `update-channel` (`stable`/`beta`), `telemetry` (`on`/`off`); mengambil revision lewat `settings.get`, mengirim update, dan keluar 3 pada konflik (AC-17) |
| `stackly storage locations [--json]` | setiap lokasi dengan jenis, path, default atau bukan, jumlah instance, terjangkau atau tidak |
| `stackly storage set-location <data\|logs\|cache> <path\|--default>` | path relatif diubah menjadi absolut dari cwd sebelum dikirim; error menulis kalimat aturan |
| `stackly instance stop --all [--yes] [--json]` | AC-9. Keputusan lokal saat build: preview tanpa `items` menulis "Nothing is running.", tidak bertanya, dan tidak mengirim `instance.stop_all` (`outcome` null); sesudah sebagian gagal, exit mengikuti kategori kode instance pertama di `failed` (misalnya `supervisor.stop_timeout`), atau `operation.cancelled` kalau Stop All dibatalkan; `--all` dan `--wait` saling menolak, karena balasan `instance.stop_all` baru datang setelah operasi selesai |
| `stackly instance create … [--autostart\|--no-autostart]` | tanpa keduanya, `startup` tidak dikirim (AC-18) |

### Baris Settings

| Tab | Grup | Baris | Pemilik | Kontrol |
|---|---|---|---|---|
| General | Appearance | Appearance | app | segmented System, Light, Dark |
| General | Startup | Start stacklyd at login | spec 0015 dan 0031 | toggle, state, Open Login Items, kalimat login berikutnya |
| General | Startup | Open Stackly at login ("Opens in the menu bar, without a window.") | app (`SMAppService.mainApp`) | toggle |
| General | Daemon | stacklyd ("Connected over Unix socket · protocol {x.y}", atau teks footer spec 0013 saat tidak `connected`) | spec 0013 | teks |
| Instances | Defaults | Autostart new instances ("Each instance can override this. Applies to instances you create from now on.") | daemon | toggle |
| Instances | Defaults | Notifications ("Failures, long operations, and lost connection to stacklyd.") | app | toggle, baris izin |
| Sources | Catalog | sumber catalog dari `CatalogInfo` (`source`, revision, `publishedAt`) | spec 0017 | teks trust |
| Sources | Providers | setiap provider dari `catalog.list_providers` dengan publisher | spec 0017 | teks trust |
| Storage | Locations | Instance data, Logs, Download cache: path default, "Used for new instances" | daemon | Change…, Use Default, Reveal in Finder, baris lokasi lama |
| Storage | Logs | Instance logs (AC-21) | spec 0027 | teks |
| Updates | Updates | Channel | daemon | segmented Stable, Beta, dengan catatan AC-23 |
| Updates | Privacy | Share anonymous usage statistics | daemon | toggle, dengan catatan AC-23 |

Ketersediaan baris dicatat di satu tabel statis `SettingsAvailability` di `StacklyUI/Settings/`, satu entri per baris dengan fitur pemiliknya; fitur yang kemudian membangun baris tersembunyi mengubah entrinya. Toggle dan segmented langsung berlaku tanpa tombol Apply (konvensi Settings macOS): baris menampilkan nilai yang diminta dengan indikator kecil selama operasi berjalan, dan kembali ke nilai daemon dengan pesan inline kalau gagal. Change… dan Use Default nonaktif selama `set_storage_location` berjalan.

### Popover menu bar

Urutan dari atas: header (AC-2), Services (AC-3, AC-4), baris operasi (AC-5), preview atau hasil Stop All (AC-6, AC-7), pemisah, lalu Open Stackly, Stop All Services…, Settings…, Close Window (nonaktif bila jendela utama tidak terbuka), Quit Stackly. Popover tidak menampilkan atau mengikat shortcut sendiri. Permukaan memakai material popover sistem, tanpa glass kustom (spec 0012). Model nya `MenuBarModel` (`@Observable`, main actor) di `StacklyUI/MenuBar/`, yang membaca `DaemonModel`, `InstanceStore`, dan `OperationCenter` yang sudah ada, tanpa koneksi kedua. Settings… memanggil `ActivationPolicyController.prepareToShowWindow()` lalu aksi environment `openSettings` dari view popover.

**Tombol baris**

| Kondisi (dicek berurutan) | Tombol | Tooltip |
|---|---|---|
| presence bukan `connected` | nonaktif | "stacklyd isn’t reachable." |
| operasi lain berjalan untuk instance ini | nonaktif | "Another operation is running for this instance." |
| lifecycle `uninitialized` atau `initializing` | nonaktif | "Still being created." |
| lifecycle `unknown` | nonaktif | "Stackly can’t confirm this process. Open Stackly to check it." |
| lifecycle `stopping` | nonaktif | "Stopping…" |
| capability start atau stop tidak didukung | nonaktif | "{provider} doesn’t support this." |
| lifecycle `stopped` atau `failed` | Start | "Start {name}" |
| lifecycle `starting`, `running`, atau `recovering` | Stop | "Stop {name}" |

**Operasi di popover**: kind `install_runtime`, `create_instance`, `start_instance`, `stop_instance`, `restart_instance`, dan `stop_all_instances` (nama kind mengikuti spec 0007 dan spec pemiliknya). Kind lain (termasuk `update_daemon_settings`, `set_storage_location`, `set_instance_startup`) tidak tampil.

**Copy Stop All**

| Tempat | Kondisi | Teks |
|---|---|---|
| judul preview | | "Stop All Services?" |
| baris item | | "{name}" dengan teks sekunder sumber demand digabung ", ": `manual` "Started by you", `startup` "Started at login", `environment` "Used by an environment" |
| baris `alreadyStopping` | | "{name} · already stopping" |
| baris `skipped` | | "{name} · won’t stop: {kalimat kode}", dengan `supervisor.identity_unproven` "Stackly can’t confirm this process", `supervisor.stop_requires_override` "used by an environment", dan `operation.recovery_required` "an operation for it needs your attention in Stackly" |
| kaki | selalu | "Data is kept." |
| kaki | `atLoginCount` 0 | "None of them start at your next login." |
| kaki | `atLoginCount` 1 | "1 of them starts again at your next login." |
| kaki | `atLoginCount` k > 1 | "{k} of them start again at your next login." |
| tombol | | Cancel (default), Stop All. Cancel menjadi tombol default lewat gaya `borderedProminent`, bukan `keyboardShortcut`, karena kebijakan interaksi user 2026-10-24 melarang shortcut |
| kosong | `items` kosong | "Nothing is running." |
| hasil | semua berhenti | "Stopped {n} services. Data is kept." |
| hasil | satu gagal | "Stopped {a} of {b}. {name} didn’t stop." dengan Open Stackly |
| hasil | f > 1 gagal | "Stopped {a} of {b}. {f} didn’t stop: {nama, nama}." dengan Open Stackly |
| hasil | ada `skipped` | ditambah " {s} skipped." |
| hasil | ada `skipped` dengan `operation.cancelled` (dibatalkan dari halaman Operations) | "Stop All was cancelled. Stopped {a} of {b}." dengan Open Stackly |

### Activation policy dan login

`ActivationPolicyController` di `StacklyUI/Shell/` menghitung jendela app yang terbuka (terlihat, diminimalkan, atau di Space lain) dari `NSWindow.willCloseNotification` dan `didBecomeKeyNotification` untuk jendela utama, Settings, dan galeri Debug, dan memanggil `NSApp.setActivationPolicy`. Setiap jalur yang membuka jendela (popover, notifikasi, `applicationShouldHandleReopen`) memanggil `prepareToShowWindow()` (policy `regular` lalu `NSApp.activate()`) dulu. Peluncuran sebagai login item dikenali dari Apple event peluncuran (`keyAELaunchedAsLogInItem`); langkah build 11 memeriksa di macOS 27 apakah `SMAppService.mainApp` masih mengirimnya dan mencatat hasilnya di `apps/macos/design.md`. Kalau tidak bisa dikenali, app membuka jendela utama (AC-12). Hasil langkah 11: SDK macOS 27 masih mendeklarasikan `keyAELaunchedAsLogInItem` tanpa deprecation, tetapi apakah macOS 27 benar mengirimnya untuk peluncuran `SMAppService.mainApp` belum terbukti, karena itu butuh app yang ditandatangani (OQ-012). Jadi fallback jendela utama menjamin perilaku AC-12 sampai bukti langkah 14 ada. `SMAppServiceMainApp.swift` ditaruh di `StacklyUI/Agent/`, folder yang menurut spec 0015 satu satunya pemakai ServiceManagement. Pendaftaran `mainApp` nyata butuh app yang ditandatangani di `~/Applications` atau `/Applications`; tanpa identity, buktinya `BLOCKED` (langkah 14).

### Notifikasi

`NotificationModel` di `StacklyUI/Notifications/`, satu satunya folder yang `import UserNotifications`, di belakang protocol `NotificationPosting` dengan fake untuk test dan fixture. Delegate `UNUserNotificationCenter` dipasang di `AppDelegate.applicationDidFinishLaunching`, sebelum event pertama diproses, supaya klik yang meluncurkan app ikut tertangani. Model mendengar event yang sudah diterima `ShellSync` dan presence `DaemonModel`, dan hanya transisi setelah snapshot awal yang dihitung. Semua pemicu hanya mengirim saat `NSApplication.isActive` false. Isi notifikasi tidak memuat secret, path, atau isi log.

**Pemicu notifikasi**

| Pemicu | Event dan kondisi | Batas | Judul | Isi | Klik |
|---|---|---|---|---|---|
| instance gagal | `instance.state_changed` ke lifecycle `failed` dari lifecycle lain | satu per transisi | "{name} failed" | "Open Stackly to see what happened." | detail instance |
| readiness hilang | `instance.health_changed` dari siap ke tidak siap selagi lifecycle `running` | paling banyak satu per instance per 5 menit | "{name} isn’t ready" | "It stopped accepting connections." | detail instance |
| operasi panjang selesai | `operation.state_changed` ke `succeeded`, `failed`, atau `cancelled` untuk kind di *Operasi di popover*, dengan `updatedAt − startedAt` > 10 detik (`OperationSummary` tidak punya `finishedAt`; `updatedAt` event final adalah waktu operasi selesai) | satu per operasi | "{judul operasi} finished" atau "… failed" atau "… was cancelled" | kalimat tetap tanpa teks error daemon, supaya path dan secret tidak ikut: "Took {durasi}.", "Open Stackly to see what happened.", atau "It was cancelled after {durasi}." | halaman Operations |
| daemon tidak terjangkau | presence keluar dari `connected` dan tidak kembali `connected` dalam 15 detik (reconnecting dihitung tidak terjangkau) | satu per gangguan | "stacklyd isn’t reachable" | "Your instances keep running if stacklyd comes back." | halaman Diagnostics |

Identifier notifikasi: `instance-failed-<id>-<revision>`, `instance-unready-<id>-<statusSeq>` (event health membawa `statusSeq`, bukan revision), `operation-<id>`, `daemon-unreachable-<since>` (detik epoch awal gangguan), jadi pengiriman ulang tidak menggandakan. Tidak ada notifikasi saat fixture scenario berjalan.

### Fixture

Scenario spec 0068 baru: `menubar-normal`, `menubar-failed`, `menubar-offline-last-known`, `menubar-many` (12 instance), `menubar-empty`, `menubar-stop-preview` (dengan already stopping dan skipped), `menubar-stop-partial` (dua gagal), `settings-general`, `settings-instances-denied`, `settings-sources`, `settings-storage-custom` (lokasi kustom, lokasi lama, satu tidak terjangkau), `settings-updates`, `settings-offline`, dan `settings-conflict`, masing masing dengan snapshot light dan dark.

### Value sourcing

| Aksi | Nilai | Sumber |
|---|---|---|
| ikon menu bar | varian | presence `DaemonModel` (spec 0013) dan lifecycle `InstanceStore` |
| header | "{n} running", "{n} failed" | aturan hitung kartu Running spec 0022 atas `InstanceStore`; failed dari lifecycle `failed` |
| header | teks status daemon | teks footer `DaemonFooter` (spec 0013) |
| baris service | nama, label, endpoint, urutan | `InstanceStore`, label spec 0012, teks Endpoint spec 0022, aturan AC-3 |
| baris service | tombol dan tooltip | tabel *Tombol baris* atas presence, `OperationCenter`, lifecycle, capability provider |
| baris operasi | judul, persen | `OperationView` di `OperationCenter`, disaring *Operasi di popover* |
| preview Stop All | item, sumber demand, autostart, already stopping, skipped, `atLoginCount` | `instance.preview_stop_all` dari `stop_all::select` dan lease spec 0031 |
| hasil Stop All | jumlah dan nama | `StopAllResult` (spec 0031) dan nama dari preview |
| Settings General | state agent | `BackgroundAgentModel` (spec 0015) |
| Settings General | kalimat login, `m` | jumlah instance dengan `autostart.policy` `at_login` di `instance.list` (spec 0031) |
| Settings General | konfirmasi Turn Off | copy dan jumlah spec 0031 |
| Settings General | state Open at login | `SMAppService.mainApp.status` |
| Settings General | versi protocol | handshake (spec 0006) |
| Settings Instances | autostart default | `daemon_settings.default_startup` |
| Settings Instances | izin notifikasi | `UNUserNotificationCenter.notificationSettings()` |
| Settings Sources | sumber, revision, tanggal, trust, publisher | `catalog.list_providers` (`CatalogInfo`, `ProviderSummary`) |
| Settings Storage | path, default, jumlah instance, terjangkau | `storage.list_locations` |
| Settings Storage | retention log | `DaemonSettingsView.logRetention` dari konstanta spec 0027 |
| Settings Updates | channel, telemetry | `daemon_settings` |
| semua baris daemon saat offline | nilai terakhir | salinan terakhir `SettingsStore` di memori (spec 0013) |
| sheet New Instance dan Duplicate | Start at login awal, lokasi data dan log | `startupDefault`, `dataLocation`, `logsLocation` di reply form |
| `instance.create` | lokasi, `startup` bila tidak dikirim | baris `daemon_settings` dibaca di dalam guard `InsertInstance` |
| `runtime.install` | folder download | `cache_location_id` dibaca saat operasi mulai |
| start | lokasi instance | `instance_paths.location_id` milik instance |
| notifikasi | durasi, kind | `updatedAt − startedAt` dan `kind` di `OperationSummary` event `operation.state_changed` yang final |
| notifikasi | app aktif atau tidak | `NSApplication.isActive` |

### Key invariants

- Quit, menutup jendela, dan pindah activation policy tidak pernah mengirim request ke daemon.
- Stop All tidak pernah menghentikan instance di luar `expectedInstanceIds`.
- Stop All memakai stop normal; tidak ada `SIGKILL`.
- Baris `instance_paths` tidak pernah berubah setelah create; ganti lokasi hanya mengubah `daemon_settings` dan menambah baris `storage_locations`.
- Tidak ada kode yang memindah, menyalin, atau menghapus data instance karena perubahan lokasi.
- Lokasi yang ditunjuk settings atau dipakai instance tidak bisa dihapus (FK `RESTRICT`), dan baris lokasi tidak pernah diubah.
- Lokasi tidak pernah bertumpuk dengan lokasi atau root lain.
- Satu instance memakai tepat satu lokasi data dan satu lokasi log.
- `daemon_settings` selalu tepat satu baris.
- Telemetry tidak mengirim apa pun di versi mana pun dari spec ini.
- Notifikasi hanya dikirim dengan izin macOS dan toggle menyala.

### Security model

Satu pengguna macOS. Semua method baru memakai pemeriksaan peer socket spec 0006 (uid yang sama dengan daemon); tidak ada role. Folder lokasi harus milik user itu dan tidak bisa ditulis user lain, dan subfolder `Stackly` dibuat 0700. Registry dan notifikasi tidak memuat secret. Path lokasi adalah data privat dan tidak masuk log level info atau di atasnya kecuali sebagai bagian error yang ditujukan ke pengguna. Tidak ada compliance scope.

### Configuration required

Tidak ada variabel environment atau credential baru. `STACKLY_ROOT` tetap menggeser lokasi bawaan.

### Critical test scenarios

- Happy path: set data location ke folder di root test, create instance, path data instance baru ada di lokasi itu dan instance lama tetap di lokasi bawaan; verifies **AC-19**, **AC-25**.
- Happy path: preview Stop All, konfirmasi, semua instance di preview berhenti dan tidak dinyalakan reconciler; verifies **AC-6**, **AC-7**.
- Failure case: instance baru mulai di antara preview dan konfirmasi, daemon menolak `instance.stop_all_changed` dan tidak ada yang berhenti; verifies **AC-8**.
- Failure case: satu instance tidak berhenti sebelum timeout, `StopAllResult.failed` berisi satu entri dan instance lain berhenti; verifies **AC-7**.
- Failure case: CLI mengubah setting saat GUI memegang revision lama, update GUI ditolak `resource.revision_mismatch` dan baris menampilkan nilai baru; verifies **AC-17**.
- Failure case: setiap `rule` lokasi ditolak dengan kode dan urutan yang benar di `FakePlatform`, dan yang bisa dibuat di filesystem asli juga di sana; verifies **AC-19**.
- Failure case: folder lokasi dihapus, start gagal `instance.storage_location_missing` tanpa proses; verifies **AC-20**.
- Failure case: migration settings dan lokasi gagal setelah salinan baris, registry tidak berubah dan snapshot ada; verifies **AC-25**.
- Failure case: Quit saat operasi berjalan, operasi selesai di daemon dan `stackly daemon status` tetap sehat; verifies **AC-10**.
- Privasi: run penuh fixture GUI dan daemon tanpa satu pun koneksi jaringan keluar baru; verifies **AC-23**.
- Auth/permission: notifikasi dengan izin ditolak tidak pernah memanggil `add(_:)`; verifies **AC-22**.

### Usulan update untuk spec lain

- **0004**: `inventory_instances` memindai semua lokasi data; `instance_path_entries` menerima lokasi; `VolumeFacts.internal`.
- **0005**: tabel `daemon_settings` dan `storage_locations`, kolom `instance_paths.location_id`, index parsial dan trigger baru, tiga WriteOp, tiga baca, dan *AC-10 per lokasi*.
- **0012**: test aturan sumber mengizinkan `import UserNotifications` hanya di `Notifications/` dan `import ServiceManagement` hanya di `Agent/`.
- **0013**: `SettingsPlaceholder` diganti; copy notice tutup jendela; activation policy (Follow-up spec 0013 untuk fitur 32 selesai di sini).
- **0015**: baris Settings memakai `BackgroundAgentModel`; `SMAppServiceMainApp` masuk folder `Agent/`.
- **0020** dan **0024**: reply form mendapat `startupDefault`, `dataLocation`, `logsLocation`; preflight mendapat baris `storage_location`; sheet menampilkan lokasi data sebagai baris read only; Duplicate mengisi Start at login dari `startupDefault`, bukan dari instance sumber.
- **0027**: tidak ada perubahan perilaku; `LogRetention` ikut di `DaemonSettingsView`.
- **0031**: *Tambahan pada kontrak spec 0031* (diterapkan 2026-11-02).
- **0069**: tidak ada perintah baru; Settings… dan Quit tetap di menu app.
- `docs/scope/ui-macos.md`: *Selisih mockup yang disengaja*.

### Selisih mockup yang disengaja

- Appearance punya System (default), Light, dan Dark, bukan hanya Light dan Dark.
- Tidak ada Restart Daemon, toggle community, atau Clean Up… sampai fitur 30, 37, dan 55.
- Baris lokasi bisa diubah (Change…, Use Default, Reveal in Finder), bukan teks saja.
- Retention log tampil read only dari spec 0027, bukan pilihan 7, 14, atau 30 hari.
- Telemetry dan channel memakai catatan jujur AC-23.
- Startup punya Open Stackly at login di samping Start stacklyd at login.
- Preview Stop All mendaftar setiap instance dengan sumber demand nya, bukan satu kalimat.
- Popover menambah Quit Stackly, "Show all {n} in Stackly…", dan ikon offline, dan tidak menampilkan shortcut ⌘, dan ⌘W.

## Build plan

Pendekatan Journey: setiap milestone menyelesaikan satu jalur pengguna lengkap (state, error, dan polish) sebelum yang berikutnya. Milestone 1 memberi jalur "ubah setting dari CLI", milestone 2 jalur "ubah setting dari Settings", milestone 3 jalur "kerja harian dari menu bar", milestone 4 notifikasi, milestone 5 bukti visual dan manual. Prasyarat: fitur 26 dan fitur 31 (sampai langkah 9 spec 0031: `startup`, `instance.stop_all`, `stop_all::select`, dan `instance.preview_stop_all`) selesai, spec 0015 milestone 1 sampai 3, dan tambahan spec 0031 sudah diterapkan di spec itu (2026-11-02).

**Milestone 1: setting daemon dan lokasi, sampai CLI**

1. Domain dan protocol: tipe settings dan lokasi, `BUILT_IN_LOCATION_IDS`, `instance_path_entries` dengan lokasi, `VolumeFacts.internal`, DTO, method, event, kode error (termasuk `expectedInstanceIds` dan `instance.stop_all_changed`; DTO preview sudah ada dari spec 0031), kenaikan `ProtocolVersion` (dibangun sebagai mayor ke 3.0, lihat *Method IPC*), `cargo xtask codegen`, fixture, dan `cargo xtask fuzz seed`, satisfies **AC-16**, **AC-17**, **AC-19**.
2. Storage: migration `NNNN_settings_locations.sql` (bangun ulang `instance_paths` dengan salinan baris, `PRAGMA foreign_key_check`, cek jumlah baris, trigger), WriteOp dan baca baru, `FakeRegistry` setara, test migration dari registry di versi skema tepat sebelum migration ini (versi terakhir di `main` saat build) berisi instance, test migration yang gagal di tengah, dan test *AC-10 per lokasi*, satisfies **AC-25**, **AC-16**, **AC-19**.
3. Core: `settings::update`, `storage::set_location` dengan `check_location` (table test per `rule` dan urutan di `FakePlatform`), `LocationSet`, `instance_paths_in`, `downloads_dir_in`, pindahkan setiap pemakai path instance di *Perubahan domain dan platform* ke `LocationSet` (grep, daftar di commit), create memakai lokasi dan `default_startup` dari guard, `inventory_instances` per lokasi, precheck start dan baris preflight `storage_location`, satisfies **AC-18**, **AC-19**, **AC-20**.
4. Daemon: handler `settings.*` dan `storage.*`, event `settings.changed`, form create dan duplicate dengan `startupDefault` dan lokasi, `logRetention` dari konstanta spec 0027, integration test `apps/daemon/tests/settings.rs` dan `storage_locations.rs` di filesystem asli, satisfies **AC-16**, **AC-17**, **AC-18**, **AC-19**, **AC-20**, **AC-21**.
5. CLI: `stackly settings`, `stackly storage`, `--no-autostart`, test output dan exit code, satisfies **AC-16**, **AC-17**, **AC-18**, **AC-19**.

**Milestone 2: jendela Settings**

6. `StacklyUI/Settings/`: `SettingsStore` (settings daemon, last known, konflik, refetch saat event, gap, dan reconnect), `AppPreferences` (Appearance di UserDefaults, diterapkan lewat `NSApp.appearance`), `SettingsAvailability`, tab Updates, Sources, dan Instances (tanpa notifikasi), banner dan copy offline, hapus `SettingsPlaceholder`, satisfies **AC-13**, **AC-14**, **AC-17**, **AC-23**, **AC-24**.
7. Tab General: baris agent dari `BackgroundAgentModel` dengan Turn Off spec 0031, `SMAppServiceMainApp` dan toggle Open Stackly at login, kalimat login berikutnya, perbarui test aturan sumber spec 0012 untuk ServiceManagement, satisfies **AC-12**, **AC-15**.
8. Tab Storage: Change… dengan `NSOpenPanel` dan konfirmasi, Use Default, Reveal in Finder, lokasi lama, baris retention read only; sheet New Instance dan Duplicate memakai `startupDefault` dan menampilkan lokasi, satisfies **AC-18**, **AC-19**, **AC-20**, **AC-21**.

**Milestone 3: menu bar dan Stop All**

9. Stop All di daemon dan CLI, di atas `stop_all::select`, `instance.preview_stop_all`, dan cancel yang sudah dibangun fitur 31 langkah 9: precheck `expectedInstanceIds` di `instance.stop_all` (field dan kode `instance.stop_all_changed` sudah masuk kontrak di langkah 1) sesudah `replay_operation`, irisan langkah, dan sidik jari yang mencakup field itu, test core di fake dan integration test `apps/daemon/tests/stop_all.rs` termasuk perubahan di antara preview dan konfirmasi; `stackly instance stop --all`, satisfies **AC-7**, **AC-8**, **AC-9**.
10. Scene `MenuBarExtra` di `StacklyApp`, ikon tiga varian (dibangun sebagai template image yang digambar di kode, `MenuBarIcon`, bukan aset katalog), `MenuBarModel` dan view popover (header, Services dengan tabel *Tombol baris*, operasi, preview dan hasil dengan *Copy Stop All*, aksi), satisfies **AC-1**, **AC-2**, **AC-3**, **AC-4**, **AC-5**, **AC-6**, **AC-10**.
11. `ActivationPolicyController`, copy notice baru, deteksi peluncuran login item dengan fallback jendela utama (periksa di macOS 27, catat di `design.md`), satisfies **AC-11**, **AC-12**.

**Milestone 4: notifikasi**

12. `StacklyUI/Notifications/`: `NotificationPosting`, `NotificationModel` dengan tabel *Pemicu notifikasi*, delegate di `AppDelegate`, izin, baris izin di tab Instances, klik yang membuka halaman yang benar, test dengan fake, perbarui test aturan sumber spec 0012 untuk UserNotifications, satisfies **AC-22**.

**Milestone 5: tampilan dan bukti**

13. Fixture scenario di *Fixture* dan snapshot light dan dark, satisfies **AC-26**.
14. Evidence manual di Mac ini, setelah persetujuanmu: izin notifikasi sungguhan (izinkan lalu tolak), Open Stackly at login sungguhan (`BLOCKED` tanpa signing identity, reason `no code signing identity on this Mac (OQ-012)`), menu bar light dan dark termasuk item yang dilarang di System Settings > Menu Bar, Quit saat instance jalan, satisfies **AC-1**, **AC-10**, **AC-11**, **AC-12**, **AC-22**.

Langkah 1 sampai 5 berurutan. Setelah langkah 4, milestone 2 dan langkah 9 bisa berjalan sebagai dua sesi paralel (folder berbeda: `StacklyUI/Settings/` dan `CreateInstance/` lawan `crates/core/src/instance/` dan `apps/daemon`). Langkah 10 butuh langkah 9; langkah 12 butuh langkah 6.

## Consequences

**Positive**:

- GUI dan CLI selalu melihat setting yang sama, dan daemon menerapkan autostart default walau GUI tertutup.
- Stop All tidak pernah mengejutkan: yang berhenti selalu bagian dari yang tampil di preview.
- Disk internal lain bisa dipakai untuk instance baru tanpa memindahkan data.
- Ikon Dock tidak mengganggu saat Stackly hanya hidup di menu bar.

**Negative / tradeoffs**:

- Migration yang membangun ulang `instance_paths` menyentuh tabel inti dan banyak pemakai path; test migration dan grep pemakai wajib.
- Instance di beberapa lokasi membuat inventarisasi, backup, dan diagnostik harus memindai lebih dari satu folder.
- Data tidak bisa dipindah, dan disk external tidak didukung sebagai lokasi.
- Fitur 32 menunggu fitur 31, dan menambah field ke kontrak spec 0031 yang belum dibangun.
- Retention log tidak bisa diatur.
- Notifikasi hanya ada selama app hidup; setelah Quit tidak ada notifikasi sama sekali.
- `MenuBarExtra` membatasi ikon pada template, jadi titik gagal berupa bentuk monokrom, bukan merah.

**Neutral**:

- Protocol naik mayor (3.0), karena `EventKind` tertutup.
- Dua folder baru di `StacklyUI` (`MenuBar/`, `Notifications/`) tunduk pada test aturan sumber spec 0012.

## Follow-up

- [x] `/architect` update spec 0031 dengan *Tambahan pada kontrak spec 0031* sebelum fitur 31 membangun `instance.stop_all`: diterapkan di spec 0031 (2026-11-02).
- [ ] `/architect` update spec 0005, 0004, 0012, 0013, 0015, 0020, dan 0024 dengan *Usulan update untuk spec lain* saat milestone terkait masuk.
- [ ] Fitur 47 menambah dampak lease dan override ke preview Stop All.
- [ ] Fitur untuk memindah data instance antar lokasi, dan untuk mendukung disk external, belum ada di scope; pertimbangkan lewat `/scope`.
- [ ] `/sync`: catat `MenuBar/` dan `Notifications/` di `apps/macos/AGENTS.md`, dan selisih mockup di `docs/scope/ui-macos.md`.

## Rationale

Alasan dan opsi: lihat [rationale.md](rationale.md).
