# 0034. Doctor dan diagnostic bundle

**Date**: 2026-11-10
**Status**: Proposed

## Summary

Spec ini menambah doctor: satu pemeriksaan read only (hanya membaca, tidak mengubah apa pun) yang dijalankan daemon atas izin folder, runtime, paket provider, konflik port beserta proses pemiliknya, bukti identitas proses (PID record yang kini menunjuk proses lain), file instance, kegagalan start, drift config, secret, operasi yang butuh pemulihan, registry, dan ruang disk. Hasilnya tampil di halaman Diagnostics dan `stackly doctor` dengan tingkat Error, Warning, atau OK, penjelasan, dan paling banyak satu aksi perbaikan yang aman (navigasi atau alur yang sudah ada, tidak pernah aksi baru yang mengubah data). Doctor tidak pernah menghapus lock, menghapus data, mengambil lock, atau mengirim sinyal ke proses, juga saat registry rusak (recovery mode). Di sampingnya ada diagnostic bundle: file zip yang disiapkan daemon, bisa kamu lihat isinya lebih dulu, lalu disimpan ke path pilihanmu lewat GUI atau `stackly diagnostics export`; secret disamarkan, path di folder home ditulis `~`, dan bundle tidak pernah diunggah.

## Requirements

**User stories**:

- Sebagai developer, saya ingin satu tempat yang menjelaskan kenapa instance saya tidak mau start (port dipakai proses lain, folder tidak bisa ditulis, runtime hilang, PID lama), supaya saya tidak menebak dari log.
- Sebagai developer, saya ingin doctor tidak pernah menghapus atau menghentikan apa pun sendiri, supaya diagnosis tidak memperparah keadaan data saya.
- Sebagai developer, saya ingin aksi perbaikan hanya muncul kalau aman, dan membawa saya ke alur yang sudah saya kenal, supaya saya tetap memegang keputusan.
- Sebagai pengguna yang meminta bantuan maintainer, saya ingin melihat isi bundle sebelum menyimpannya dan yakin tidak ada password di dalamnya, supaya saya bisa membagikannya dengan tenang.
- Sebagai penulis script, saya ingin `stackly doctor --json` dan exit code yang stabil, supaya CI atau script setup bisa berhenti saat ada Error.

**Acceptance criteria**:

- **AC-1** (`doctor.run`): method baru `doctor.run { instanceId?, checks }` (`recovery allowed`, bukan operasi spec 0007) membalas `DoctorReport` seperti *Laporan doctor*: satu `DoctorCheck` per check daemon di tabel *Daftar check* dalam urutan tabel itu (atau hanya yang diminta `checks`), masing masing dengan `severity` (`error`, `warning`, `ok`), judul, ringkasan, dan paling banyak 100 temuan (`omitted` menghitung sisanya). Severity check adalah yang terberat dari temuannya; check tanpa temuan `ok`. Setiap check dibatasi 5 detik dan seluruh run 15 detik; check yang lewat batas menjadi `warning` dengan satu temuan `check_unavailable` reason `timeout`, dan run tetap membalas (aturan pembatalan di *Runner doctor*). Setiap call memulai run baru; paling banyak 2 run berjalan bersamaan (semaphore, termasuk run di dalam preview bundle), dan call ketiga menunggu slot paling lama 15 detik sebelum run nya mulai. `instanceId` membatasi check yang berlingkup instance ke instance itu dan tetap menjalankan check global; ID yang tidak ada menghasilkan `resource.not_found`. `checks` dengan ID client (`connection`, `protocol`, `background_agent`), `unrecognized`, atau ganda ditolak `request.invalid_params`. `errors` dan `warnings` menghitung temuan (bukan check) dengan severity itu, termasuk yang dihitung di `omitted` (REQ-019).
- **AC-2** (read only): selama `doctor.run` dan `diagnostics.preview_bundle`, daemon tidak menulis registry (generation registry tidak berubah), tidak membuat operasi, tidak menulis, memindah, atau menghapus file di root data, runtime, dan log (`FakePlatform::mutations()` kosong, kecuali file staging bundle di `<cache>/diagnostics/` untuk preview), tidak pernah memanggil `lock_instance`, `probe_lock`, `signal`, `force_kill`, atau spawn proses apa pun lewat `ProcessSupervision`, dan tidak pernah membaca nilai secret lewat `expose`. Folder scratch, file `instance.lock`, `postmaster.pid`, dan data yang dilaporkan tetap identik byte demi byte sesudahnya (REQ-019, PRD bagian 35.3).
- **AC-3** (PID record menunjuk proses lain): check `processes` memberi temuan menurut tabel *Hasil check processes*. Untuk instance yang observasi prosesnya mencatat PID dari boot yang sama yang kini hidup tetapi `startMatches`, `executableMatches`, atau `userMatches` gagal, temuannya `pid_reused` (Error) dengan `evidence`: `IdentityEvidence` hasil pemeriksaan pasif (ketujuh check spec 0010 tanpa probe lock, jadi `lock` diisi dari blocker tersimpan atau null), proses yang tercatat (PID, waktu start, executable), dan proses yang kini memegang PID itu (PID, waktu start, executable dengan `~`, atau `sameUser` false dan executable null kalau prosesnya milik user lain). Proses itu tetap hidup dengan PID dan waktu start yang sama, file data dan `instance.lock` tetap ada dan identik, dan lifecycle instance tidak berubah. `data_in_use` dan `identity_unproven` hanya berasal dari blocker tersimpan ditambah fakta pasif baru, tidak pernah dari probe lock (REQ-019, AC-019, AC-067).
- **AC-4** (recovery mode): saat registry dibuka dalam recovery mode (spec 0005), `doctor.run` tetap menjawab dengan `recovery: true`. Check `registry` menjadi Error `recovery_mode` dengan reason dan pesan `RecoveryReport` yang sudah dipegang daemon sejak open, tanpa `quick_check` dan tanpa membuka koneksi; check `daemon`, `permissions` (hanya root), dan `disk_space` berjalan biasa; check lain menjadi Warning `check_unavailable` reason `recovery_mode`. Tidak ada satu pun baca atau tulis registry (REQ-019, spec 0005 Follow-up baris 651).
- **AC-5** (konflik port): check `endpoints` memeriksa setiap endpoint IP berport tetap atau sudah diberi port milik instance yang lifecycle nya bukan `running`, `starting`, atau `stopping` dan tidak sedang dipegang operasi aktif, lewat `instance::create::port::judge_change` (spec 0029 AC-14) dengan port endpoint itu sendiri, `own_bound` None, transport endpoint itu (TCP atau UDP sesuai manifest), dan `SocketPortProbe`. Masalah yang dikembalikan memberi `port_in_use`: Error kalau desired instance `running` atau lifecycle `failed`, Warning kalau `stopped`, dengan details `endpoint`, `address`, `port`, `problem`, field `portOwner` dari port platform baru `ListenerLookup` (proses dengan PID, nama, dan path executable ber `~`; instance Stackly lain kalau PID itu adalah proses tercatat instance itu dan waktu start nya cocok; atau `unknown` kalau tidak ada proses user ini yang memegangnya), dan `suggestedPort` dari saran `judge_change`. Aksi `change_port` membawa endpoint dan port saran, dan tidak dikirim kalau tidak ada saran. Doctor tidak pernah mengubah port sendiri (REQ-019, PRD bagian 35.3 baris port conflict).
- **AC-6** (operasi): check `operations` memberi satu Error `needs_recovery` per operasi `needs_recovery` dengan `operationId`, jenis, `recoveryHint`, `recoveryCause`, kode error penyebab, dan `path` kalau kodenya `runtime.install_conflict`, serta aksi `show_operation`; dan satu Warning `scratch_leftover` per folder di `<data>/operations/` yang tidak punya baris operasi (atau namanya bukan UUID), dengan path relatif `operations/<nama>`; folder yang waktu ubahnya kurang dari 10 menit atau namanya milik operasi yang sedang aktif di engine dilewati, supaya operasi yang baru dibuat tidak terhitung. Doctor tidak menghapus folder itu. GUI menampilkan aksi `show_operation` menurut AC-18 (spec 0007 Follow-up baris 833).
- **AC-7** (runtime dan paket): check `runtimes` melaporkan runtime managed dengan state registry `missing`, `drifted`, atau `failed` (Error kalau dipakai instance, selain itu Warning) dan, untuk runtime `installed`, pemeriksaan pasif bahwa folder install dan setiap executable di `RuntimeDetail.executables` ada sebagai file biasa milik user yang bisa dijalankan (`runtime_files_missing`, severity sama). Doctor tidak mengubah state runtime, tidak menghitung ulang digest, dan tidak menawarkan reinstall. Check `provider_packages` memberi Error `package_unreadable` per paket pinned yang dipakai instance tetapi tidak lolos loader (spec 0009), dengan provider, revision, reason, dan instance yang memakainya (REQ-019, spec 0018, spec 0021).
- **AC-8** (registry): di luar recovery mode, check `registry` menjalankan `PRAGMA quick_check` lewat koneksi baca registry dan memindai setiap kolom kode set terbuka yang terdaftar di `CODED_COLUMNS` (*Pindai kode tidak dikenal*). Hasil gagal memberi Error `quick_check_failed` dengan paling banyak 10 baris pesan; error SQLite saat menjalankannya memberi `check_unavailable` reason `io`; setiap nilai yang tidak dikenal build ini memberi Error `unrecognized_value` dengan `table`, `rowId`, `column`, dan `value` (paling banyak 64 byte), paling banyak 64 baris per kolom. Tanpa masalah, ringkasan OK menyebut `quick_check` dan versi schema dari `registry_info` (spec 0005 Follow-up baris 430).
- **AC-9** (instance): check `instance_files` memanggil `supervisor::integrity::data_is_missing` dan `runtime_is_missing` (fungsi rekonsiliasi spec 0030 yang hanya membaca; doctor tidak memancarkan `ReconcileFinding`) untuk setiap instance yang `integrity::applies`, dan memberi Error `data_missing` atau `runtime_missing`. Check `instance_failures` memberi Error `start_failed` untuk setiap instance `failed`, dengan `lastFailure` (kode, `at`, `exitCode`, `signal`, dan `excerpt` apa adanya dari `InstanceSummary`, spec 0033), atau dengan penjelasan "The last failure isn’t known since stacklyd restarted." kalau `lastFailure` null; dan Warning `not_ready` untuk instance `running` yang readiness state nya `not_ready` (bukan `unknown`), dengan `reason` dan `detail` readiness; keduanya dengan aksi `show_instance` dan remedy provider kalau cocok (AC-10). Check `config_drift` memberi Warning `config_drift` per instance dengan blocker `configDrift` atau hasil `supervisor::drift::check` yang tidak bersih, dengan file yang berubah dan aksi `review_config`. Check `secrets` memberi Error `secret_missing` per field dari blocker `needsSecret` (field dan reference, tanpa nilai) (REQ-019).
- **AC-10** (remedy provider): manifest `stackly/v1alpha1` menerima blok opsional `remedies` (1 sampai 8 entri) seperti *Blok remedies*. Loader menolak `name` di luar pola `^[a-z][a-z0-9_]{0,31}$` atau ganda, `when` kosong, teks kosong atau di atas batas, dan token selain `${instance.name}`, `${instance.data}`, `${instance.config}`, `${endpoint.<n>.port}`, `${endpoint.<n>.socketDir}`, `${runtime.executable}`, dan `${runtime.executables.<alias>}` (tanpa `secret.*` dan `settings.*`), juga endpoint atau alias yang tidak ada di paket itu, dengan kode issue spec 0009. Temuan `start_failed` atau `not_ready` mendapat `remedy` dari entri pertama dalam urutan manifest yang `when` nya cocok dengan `lastFailure.code` dan `excerpt`, atau dengan `reason` dan `detail` readiness. Entri dibaca dari revision paket provider yang sama yang tampil di catalog (spec 0017); kalau revision itu tidak ada atau tidak terbaca, dari paket pinned instance; kalau keduanya tidak terbaca, tanpa remedy (temuan `package_unreadable` sudah menjelaskannya). Entri dengan `evidence.pidFile` membuat doctor membuka file itu dengan `O_NOFOLLOW` setelah memastikan folder induk canonical nya ada di bawah folder `data/` atau `config/` canonical instance, membaca paling banyak 4 KiB, mengurai baris pertama hanya sebagai bilangan desimal `u32` (isi lain tidak pernah ditampilkan), dan memeriksa PID nya secara pasif: tidak ada, hidup dengan executable runtime instance, atau hidup dengan executable lain. Paket PostgreSQL mendapat dua entri: `stale_postmaster_pid` dan `readiness_role` seperti *Entri PostgreSQL*. Remedy hanya teks langkah, perintah untuk disalin, dan path; Stackly tidak pernah menjalankan atau menghapus apa pun karena remedy (spec 0019 Follow-up baris 632).
- **AC-11** (gagal start daemon dan agent): saat `stacklyd` keluar 1 setelah root data diketahui dan `<data>` adalah folder milik user tanpa bit tulis group atau other, ia menulis `<data>/daemon-start-failure.json` (mode 0600, atomik, paling banyak 4 KiB) seperti *Catatan gagal start daemon*, tanpa membuat folder; daemon menghapus file itu setelah socket ter bind di start berikutnya. CLI dan GUI membaca file itu (aturan baca launch record spec 0015, ditambah: file yang bukan file biasa, symlink, milik uid lain, atau bermode lebih longgar dari 0600 dianggap tidak ada) dan check client `background_agent` menampilkannya sebagai Error `daemon_start_failed` selama daemon tidak terjangkau, dengan waktu dan pesan. Saat state agent `enabled`, daemon tidak terjangkau (`ipc.endpoint_missing` atau `ipc.connection_refused`), launch record ada, tidak ada catatan gagal start yang lebih baru dari `lastLaunchedAt`, dan waktu ubah `Contents/MacOS/stacklyd` di bundle agent lebih dari 2 detik sesudah `lastLaunchedAt` (keduanya UTC), check yang sama memberi Warning `binary_replaced` dengan langkah Start stacklyd (GUI) atau `stackly daemon start` (CLI). Tidak ada kode produk yang menjalankan `launchctl` (spec 0015 Follow-up baris 722).
- **AC-12** (`diagnostics.preview_bundle`): method baru (`recovery allowed`) menjalankan doctor penuh (AC-1), membangun bundle zip di `<cache>/diagnostics/<previewId>.zip` (mode 0600) dengan isi tabel *Isi bundle*, dan membalas `BundlePreview` (`previewId`, `createdAt`, `expiresAt` 15 menit kemudian, `suggestedFileName`, daftar entri dengan path, jenis, deskripsi, ukuran, jumlah baris, dan tanda terpotong, total ukuran sebelum dan sesudah kompresi). Semua entri kecuali log instance melewati `Redactor` (spec 0011); log instance disalin dari store spec 0027 tanpa lewat `Redactor` lagi, karena store hanya ditulis setelah redaksi. Sesudah itu, di semua entri, folder home diganti `~` menurut *Penggantian folder home*. Batas dan urutan anggaran ada di *Isi bundle*; yang terpotong atau tidak ikut ditandai di entri dan di `README.txt`. Preview disimpan menurut *Staging preview*. Dalam recovery mode, entri yang butuh registry diganti catatan di `README.txt` (NFR-016, REQ-019).
- **AC-13** (`diagnostics.export_bundle`): method baru `diagnostics.export_bundle { previewId, destination, replace }` (`recovery allowed`, bukan operasi) menyalin zip preview itu persis (sha256 sama) ke `destination`: path absolut UTF-8 paling banyak 1024 byte, tanpa komponen `.` atau `..` (ditolak, tidak dinormalkan), nama file paling banyak 200 byte, dan folder induk yang ada. Daemon menulis `.stackly-diagnostics-<uuid>.partial` di folder induk (rename tidak bisa lintas volume, jadi partial ada di folder tujuan) dengan create exclusive mode 0600, menyalin dari fd zip preview yang sudah terbuka, `fsync`, lalu `lstat` tujuan tepat sebelum `rename_no_replace` (atau `rename` kalau `replace` true dan tujuan adalah file biasa atau symlink, yang diganti sebagai symlink itu sendiri), lalu `fsync` folder induk; kegagalan menghapus file partial. Error: `diagnostics.preview_not_found` (preview tidak ada atau kedaluwarsa), `diagnostics.destination_exists`, `diagnostics.destination_invalid` (reason `not_absolute`, `invalid_name`, `parent_missing`, `parent_not_directory`, `is_directory`, `not_regular`), `diagnostics.write_failed` (reason `no_space`, `permission`, `io`). Sukses membalas `{ path, bytes, sha256 }` dan menulis satu baris log `diagnostics_exported` dengan path ber `~`. Bundle tidak pernah dikirim ke jaringan (NFR-016).
- **AC-14** (sentinel): doctor dan bundle tidak pernah memanggil `expose` atau membaca Keychain; `Redactor` memakai nilai yang sudah dipelajari daemon sejak start (setiap secret yang di resolve untuk start atau aksi lain, spec 0011) ditambah pola bawaannya (userinfo URL, `PASSWORD=`). Dengan nilai secret sentinel yang dipelajari lewat resolve start instance fixture dan ditanam di output proses instance (lewat log store), di baris log daemon, di pesan error operasi yang gagal, di `excerpt` kegagalan, dan di detail readiness, nilai itu, bentuk JSON escape nya, dan bentuk percent encode di URL tidak muncul di `DoctorReport`, di `BundlePreview`, di setiap entri zip hasil export setelah didekompresi, di output `stackly doctor` dan `stackly diagnostics export` (teks dan `--json`), maupun di log daemon yang ditulis selama run (spec 0011 Follow-up baris 612, AC-016).
- **AC-15** (`stackly doctor`): perintah `stackly doctor [--instance <instance>] [--check <id>]... [--json]` menjalankan check client `connection`, `protocol`, dan `background_agent` lebih dulu, lalu `doctor.run` kalau daemon terjangkau dan kompatibel, dan menampilkan semuanya seperti *CLI*. `--instance` memakai `TargetArg` dan `InstanceTarget::resolve` (spec 0022). `--check` hanya menerima ID check daemon (clap menolak yang lain dengan exit 2) dan menyaring check daemon saja; ketiga check client selalu berjalan. Exit 0 kalau tidak ada temuan Error (Warning saja tetap 0); ada Error memberi `cli.doctor_found_errors` (kategori `failed`, exit 7) dengan `result` tetap terisi; daemon tidak terjangkau atau tidak kompatibel memberi error handshake atau koneksinya dengan exit dari kategorinya (misalnya exit 6 untuk `ipc.endpoint_missing`, spec 0006 dan 0008) dengan `result.local` terisi dan `result.report` null, dan error itu menang atas exit 7. `--json` membalas `{ local: [DoctorCheck], report: DoctorReport | null }`. Validasi dan semantik error sama dengan GUI karena keduanya memakai method yang sama (REQ-028, AC-028).
- **AC-16** (`stackly diagnostics`): `stackly diagnostics preview [--json]` memanggil `diagnostics.preview_bundle` dan menampilkan daftar entri. `stackly diagnostics export [--output <path>] [--force] [--yes] [--json]` memanggil preview, menulis daftar entri ke stderr, meminta konfirmasi lewat `Confirm` spec 0008 (mode noninteractive tanpa `--yes` gagal seperti aturan spec 0008), lalu memanggil export dengan `destination` absolut dari `--output` (default `./<suggestedFileName>` dari folder kerja) dan `replace` dari `--force`. `--json` membalas `{ preview, exported }`. Error export memetakan exit dari kategorinya (REQ-028, NFR-016).
- **AC-17** (halaman Diagnostics): `UnbuiltPage(.diagnostics)` diganti seksi Doctor di bawah `ConnectionDiagnosticsSection`, `BackgroundAgentDiagnosticsSection`, dan `CommandLineToolSection`, lalu seksi Diagnostic Bundle, seperti *GUI*. Seksi Doctor memakai `CheckList(.single)`: satu `CheckItem` per temuan (`error` ke `failed`, `warning` ke `warning`, `unrecognized` ke `unrecognized`) dan satu `CheckItem` `passed` per check tanpa temuan, dengan judul dan penjelasan dari daemon, dan paling banyak satu tombol: aksi aman temuan itu, atau "Details…" untuk temuan yang punya bukti, excerpt, atau remedy (sheet detail memuat aksi aman itu). Toolbar memuat "Export Bundle…" dan "Run Doctor" ("Running…" nonaktif selama run). Baris meta menulis "Last run today at {jam} · {n} issues" atau teks berjalan dari mockup. State loading, gagal, unsupported, dan daemon tidak terhubung mengikuti *State seksi Doctor* (PAGE-13, REQ-019).
- **AC-18** (aksi aman): GUI hanya memetakan aksi ke navigasi atau alur yang sudah ada: `show_instance` ke `ShellNavigator.openInstance`, `review_config` ke tab Configuration, `change_port` ke tab Configuration dengan `EndpointsSheet` yang field port endpoint itu sudah terisi port saran (tidak ada yang berubah sampai kamu menekan Apply di sheet itu), `show_operation` ke `ShellNavigator.openOperation` (spec 0035), dan `open_storage_settings` ke tab Storage Settings (spec 0032). Aksi `unrecognized`, atau aksi yang halaman tujuannya belum ada di build ini, tidak tampil. Tidak ada tombol yang menghapus lock, file, atau data, atau menghentikan proses. CLI menulis aksi sebagai baris `Next:` berisi perintah setara (*CLI*) (REQ-019, PRD bagian 35.3).
- **AC-19** (sheet bundle): "Export Bundle…" dan "Preview Bundle…" membuka `StacklySheet` "Export Diagnostic Bundle" yang memanggil `diagnostics.preview_bundle` dan menampilkan daftar entri (path monospace, deskripsi, ukuran) dengan catatan redaksi; "Export…" membuka `NSSavePanel` di folder Downloads dengan `suggestedFileName`, lalu memanggil `diagnostics.export_bundle` dengan `replace` true hanya kalau panel itu sudah meminta konfirmasi Replace, lalu menutup sheet dan menampilkan `FeedbackCard` "Saved {file} to {folder}." dengan "Show in Finder". `diagnostics.preview_not_found` memuat ulang preview dengan pesan "The preview expired. Review the new one before you export."; error lain tampil di sheet tanpa menutupnya. GUI sendiri tidak menulis file (PAGE-13, NFR-016).
- **AC-20** (sambungan GUI): `ShellDestination.diagnostics` mendapat `focus: DiagnosticsFocus?` (`.instance(UUID)` atau `.check(DoctorCheckId)`) dan `ShellNavigator.openDiagnostics(focus:)`, yang dipakai banner spec 0033 ("Diagnose…") dan item S Overview (fokus `disk_space`). Fokus instance menyaring seksi Doctor ke temuan instance itu ditambah temuan global Error, dengan tombol "Show All". GUI menjalankan doctor penuh (tanpa `instanceId`, juga untuk fokus instance; penyaringan di client) sekali setiap kali `ShellSync` mencapai `.caughtUp` untuk `serverInstanceId` baru, saat Diagnostics tampil tanpa laporan atau dengan laporan lebih dari 5 menit, saat Run Doctor ditekan, dan saat `openDiagnostics` dipanggil; pemicu yang datang selama run berjalan diabaikan, kecuali Run Doctor, yang memulai satu run lagi setelah run itu selesai. Badge sidebar Diagnostics menghitung temuan Error laporan terakhir selama terhubung (tanpa badge untuk 0), subjudul toolbar menulis "{n} issues", "No issues", atau "Not run yet", dan `CapacityReport` dari check `disk_space` diberikan ke `CapacityModel` dengan aturan `checkedAt` spec 0022. `OnboardingCopy.commandExamples` mendapat `stackly doctor` (spec 0016 Follow-up baris 456, spec 0022 Follow-up baris 591, spec 0013 Follow-up baris 734).
- **AC-21** (fixture tampilan): skenario fixture spec 0068 untuk Diagnostics: belum pernah run, berjalan tanpa laporan, laporan campuran (Error port conflict dengan Use Port, Warning drift dengan Review, OK lainnya), laporan semua OK, recovery mode, fokus instance, daemon tidak terhubung dengan laporan lama, unsupported, sheet detail `pid_reused` dengan bukti, sheet detail `start_failed` dengan excerpt dan remedy, dan sheet bundle (memuat, daftar, error), masing masing light dan dark, lolos snapshot (PAGE-13).
- **AC-22** (bukti native): test integration daemon di `apps/daemon/tests/` dengan `TestRoot`, port dari `allocate_port`, dan proses `sleep` sungguhan membuktikan AC-2, AC-3, AC-5, AC-6, AC-12, AC-13, dan AC-14 terhadap filesystem dan proses asli; `binary_replaced` di bawah launchd sungguhan dan satu lintasan GUI manual dicatat sebagai langkah manual (`BLOCKED` sampai dijalankan) (REQ-019, AC-019).

## Decision

**Chosen option**: Option 1: doctor sebagai method daemon read only yang membalas laporan terstruktur, ditambah tiga check client untuk keadaan yang hanya terlihat dari luar daemon, dan bundle yang dibangun daemon saat preview lalu disalin persis saat export.

Daemon memiliki semua pemeriksaan yang butuh registry, proses, dan filesystem Stackly; CLI dan GUI hanya menambah koneksi, protokol, dan background agent, lalu menampilkan teks yang dikirim daemon, dan aksi perbaikan hanya membuka alur yang sudah ada.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · skill Swift di [apps/macos/AGENTS.md](../../../apps/macos/AGENTS.md) (`swiftui-expert-skill`, `swift-concurrency`, `swift-testing-pro`)

## Rationale

Alasan setiap keputusan, opsi yang dipertimbangkan, dan runner up ada di [rationale.md](rationale.md).

## Feature design

### Batas dengan fitur lain

| Hal | Pemilik |
| --- | --- |
| Doctor, check daemon dan client, remedy provider, catatan gagal start daemon, bundle, `stackly doctor`, `stackly diagnostics`, seksi Doctor dan Diagnostic Bundle | spec ini |
| Banner crash loop dan error start, isi `InstanceSummary.restart` dan `lastFailure` | spec 0033 (doctor membacanya apa adanya) |
| Halaman Operations, `ShellNavigator.openOperation`, tab Activity, audit | spec 0035 (doctor dan export bukan operasi dan tidak diaudit) |
| Verifikasi ulang digest runtime, transisi state `missing` dan `drifted`, reinstall atau repair runtime | fitur 55 (scope 10) |
| Runtime external (Register External), pin dan drift nya | fitur 56 (scope 10) |
| Keluar dari recovery mode (restore registry) | fitur 48; adoption fitur 57 |
| Upload bundle, telemetry | tidak ada (NFR-016); di luar baseline |
| Bundle saat daemon mati (dibangun client) | Follow-up |
| Peringatan kapasitas shared memory PostgreSQL | fitur 28 Follow-up spec 0019, bukan spec ini |

### Daftar check

Check daemon berjalan di `doctor.run` dalam urutan ini. Check client dihitung CLI dan GUI sendiri dan tidak pernah dikirim daemon.

| `DoctorCheckId` | Judul | Di mana | Butuh registry | Kode temuan (severity) | Aksi |
| --- | --- | --- | --- | --- | --- |
| `connection` | Connection | client | tidak | kode `ipc.*` saat tidak terjangkau (Error) | aksi seksi Connection yang ada |
| `protocol` | Protocol | client | tidak | `protocol_incompatible` (Error) | tidak ada |
| `background_agent` | Background agent | client | tidak | state agent spec 0015 yang butuh tindakan (Error atau Warning sesuai tabel spec 0015), `daemon_start_failed` (Error), `binary_replaced` (Warning) | aksi agent yang ada |
| `daemon` | stacklyd | daemon | tidak | `subsystem_degraded` (Warning) | tidak ada |
| `registry` | Registry | daemon | ya, lewat koneksi baca | `recovery_mode`, `quick_check_failed`, `unrecognized_value` (Error) | tidak ada |
| `permissions` | Permissions | daemon | root tidak, instance ya | `root_not_private`, `path_not_writable`, `path_foreign_owner` (Error) | `show_instance` |
| `disk_space` | Disk space | daemon | tidak | `volume_filling`, `volume_low` (Warning), `volume_critical` (Error) | `open_storage_settings` |
| `runtimes` | Runtimes | daemon | ya | `runtime_state`, `runtime_files_missing` (Error kalau dipakai, selain itu Warning) | tidak ada |
| `provider_packages` | Provider packages | daemon | ya | `package_unreadable` (Error) | `show_instance` |
| `endpoints` | Endpoints | daemon | ya | `port_in_use` (Error atau Warning, AC-5) | `change_port` |
| `processes` | Processes | daemon | ya | `pid_reused`, `data_in_use`, `identity_unproven` (Error), `process_suspended` (Warning) | `show_instance` |
| `instance_files` | Instance files | daemon | ya | `data_missing`, `runtime_missing` (Error) | `show_instance` |
| `instance_failures` | Start failures | daemon | ya | `start_failed` (Error), `not_ready` (Warning) | `show_instance` |
| `config_drift` | Configuration revisions | daemon | ya | `config_drift` (Warning) | `review_config` |
| `secrets` | Secrets | daemon | ya | `secret_missing` (Error) | `show_instance` |
| `operations` | Operations | daemon | ya | `needs_recovery` (Error), `scratch_leftover` (Warning) | `show_operation` |

Semua check daemon bisa memberi `check_unavailable` (Warning) dengan reason `timeout`, `recovery_mode`, `not_supported`, atau `io`, dan pesan error yang sudah lewat `Redactor`.

Aturan per check yang tidak tertulis di AC:

- `daemon`: setiap subsistem `DaemonHealth` yang degraded (misalnya `log_sink`) menjadi satu temuan. Ringkasan OK: "stacklyd {versi} · protocol {major.minor} · {launchd atau manual}".
- `permissions`: setiap root di `RootSet` (spec 0004) harus folder milik uid ini tanpa bit tulis group atau other (`root_not_private`). Untuk setiap instance, folder `data/`, `config/`, `config-history/`, dan `temp/` harus folder milik uid ini dengan bit tulis dan eksekusi owner; isi `data/` diperiksa sampai kedalaman 2 tanpa mengikuti symlink: folder tanpa `u+w` atau `u+x` memberi `path_not_writable`, dan entri milik uid lain memberi `path_foreign_owner`, dengan path relatif ke folder instance dan mode oktal. Paling banyak 5000 entri per instance dan 10 temuan per instance; sisanya dihitung di `omitted`.
- `disk_space`: memakai fungsi pembacaan yang sama dengan `storage.get_capacity` (single flight yang sama) dan level `stackly_domain::capacity::level`; `DoctorCheck.capacity` membawa `CapacityReport` nya. Ringkasan: "{usedPercent}% used · {available} free".
- `processes`: diperiksa untuk setiap instance yang punya observasi proses tercatat atau blocker identitas, menurut *Hasil check processes*.
- Ringkasan OK per check: `permissions` "All Stackly folders are private and writable by you."; `runtimes` "{n} runtimes. Every executable is in place."; `provider_packages` "Every pinned package loads."; `endpoints` "No conflicts across {n} configured endpoints."; `processes` "Every recorded process matches its instance."; `instance_files` "Every instance has its data folder and runtime."; `instance_failures` "No instance has failed."; `config_drift` "All effective configs match their revision."; `secrets` "Every secret an instance needs is in the Keychain."; `operations` "No operation needs recovery."; `registry`, `daemon`, dan `disk_space` seperti di atas. `{n}` dihitung dari yang diperiksa.
- Urutan temuan dalam satu check: severity (Error dulu), lalu temuan tanpa instance, lalu nama instance, lalu kode, lalu `subject` (endpoint, path, atau ID operasi).
- Teks: judul temuan berbentuk "{judul masalah} · {nama instance}" seperti mockup ("Port conflict · mysql-legacy"); penjelasan satu sampai tiga kalimat bahasa Inggris dari tabel teks di `stacklyd::ipc::doctor::copy`, satu sumber untuk CLI dan GUI.

### Hasil check processes

Fakta dari `observe_passive` atas observasi tercatat; blocker dari registry. Baris pertama yang cocok menang.

| Keadaan | Temuan |
| --- | --- |
| blocker `dataInUse` tersimpan | `data_in_use` (Error), bukti blocker ditambah bukti pasif baru |
| blocker `identityUnproven` tersimpan | `identity_unproven` (Error), bukti blocker ditambah bukti pasif baru |
| `bootMatches` gagal (observasi dari boot sebelumnya) dan lifecycle `unknown` | `identity_unproven` (Error), penjelasan "Recorded before this Mac last restarted." |
| `bootMatches` gagal dan lifecycle lain | bukan temuan |
| PID tidak ada dan lifecycle `unknown` | `identity_unproven` (Error), penjelasan "The recorded process is gone." |
| PID tidak ada dan lifecycle lain | bukan temuan |
| PID hidup, `startMatches`, `executableMatches`, atau `userMatches` gagal | `pid_reused` (Error) |
| PID hidup, enam check identitas lulus, `holdsLock` gagal | `identity_unproven` (Error), penjelasan "The process matches but doesn’t hold the instance lock." |
| PID hidup dan `IdentityEvidence.stopped` true | `process_suspended` (Warning) |
| semua lulus | bukan temuan |

### Runner doctor

- Setiap check berjalan di `spawn_blocking` dengan bendera batal bersama (`AtomicBool`) yang diperiksa check di antara item (per instance, per PID, per entri folder). Lewat 5 detik, runner menyetel bendera, mengembalikan `check_unavailable` reason `timeout`, dan tidak menunggu task itu.
- `quick_check` dan pindai registry memakai `InterruptHandle` rusqlite koneksi baca; batas waktu memanggil `interrupt()`.
- Kalau task check yang sama dari run sebelumnya masih berjalan setelah batas waktunya, run baru langsung memberi `check_unavailable` reason `timeout` untuk check itu tanpa memulai task lain, jadi paling banyak satu task tertinggal per check.

### Pindai kode tidak dikenal

`stackly-storage` memegang `CODED_COLUMNS: &[CodedColumn]`, satu entri per kolom kode set terbuka (`table`, `column`, `known: fn() -> &'static [&'static str]`), di modul yang sama dengan mapper baris. Mapper baris untuk kolom itu mengambil fungsi `known` dan parse nya dari entri ini, jadi kolom berkode tidak bisa dibaca tanpa terdaftar. `scan_unrecognized` menjalankan `SELECT rowid, <column> FROM <table> WHERE <column> NOT IN (<known>) LIMIT 64` per entri lewat koneksi baca. Test storage memastikan setiap entri: nilai asing membuat baca biasa gagal `Corrupt` dan muncul di pindai.

### Laporan doctor

DTO baru di `stackly-protocol` modul `doctor` (camelCase, count dengan newtype `wire.rs`, enum terbuka dengan `Unrecognized`, field opsional `#[serde(default)]`, tanpa `BTreeMap`):

| Tipe | Field |
| --- | --- |
| `DoctorParams` | `instanceId: Uuid?`, `checks: [DoctorCheckId]` (kosong berarti semua); `Validate` menurut AC-1 |
| `DoctorReport` | `runId: Uuid` (v7), `startedAt`, `finishedAt`, `recovery: bool`, `scope: DoctorInstanceRef?`, `checks: [DoctorCheck]`, `errors: Count`, `warnings: Count` |
| `DoctorCheck` | `id: DoctorCheckId`, `severity: DoctorSeverity`, `title`, `summary` (1 sampai 200 karakter), `findings: [DoctorFinding]` (paling banyak 100), `omitted: Count`, `capacity: CapacityReport?` (hanya `disk_space`) |
| `DoctorFinding` | `code: DoctorFindingCode`, `severity`, `title` (paling banyak 120), `explanation` (paling banyak 600), `subject: String?`, `instance: DoctorInstanceRef?`, `operationId: Uuid?`, `details: [DoctorDetail]` (paling banyak 16), `evidence: DoctorEvidence?`, `portOwner: PortOwner?`, `excerpt: [String]` (paling banyak 10 baris, 256 karakter), `remedy: DoctorRemedy?`, `action: DoctorAction?` |
| `DoctorInstanceRef` | `id: Uuid`, `name` |
| `DoctorDetail` | `key` (camelCase, paling banyak 32 byte), `value` (paling banyak 512 byte) |
| `DoctorEvidence` | `identity: IdentityEvidence` (DTO spec 0010), `recorded: ProcessRef?`, `current: ProcessRef?`, `observedAt` |
| `ProcessRef` | `pid: u32`, `startedAt: Timestamp?`, `executable: String?` (dengan `~`), `sameUser: bool?` |
| `DoctorRemedy` | `provider`, `revision`, `name`, `title`, `steps: [RemedyStep]`, `pidFile: PidFileEvidence?` |
| `PortOwner` | `kind: PortOwnerKind` (`process`, `stackly_instance`, `unknown`, `unrecognized`), `process: ProcessRef?`, `instance: DoctorInstanceRef?` |
| `RemedyStep` | `text`, `command: String?`, `path: String?` |
| `PidFileEvidence` | `path`, `pid: u32?`, `state: PidFileState` (`missing`, `unreadable`, `not_running`, `runtime_process`, `other_process`, `unrecognized`), `process: ProcessRef?` |
| `DoctorAction` | `kind: DoctorActionKind`, `instanceId: Uuid?`, `operationId: Uuid?`, `endpoint: String?`, `port: u16?` |
| `DoctorSeverity` | `error`, `warning`, `ok`, `unrecognized` |
| `DoctorActionKind` | `show_instance`, `review_config`, `change_port`, `show_operation`, `open_storage_settings`, `unrecognized` |
| `DoctorCheckId` | semua ID tabel *Daftar check*, `unrecognized` |
| `DoctorFindingCode` | semua kode tabel *Daftar check* ditambah `check_unavailable`, `unrecognized` |

`DoctorAction` sengaja struct datar dengan `kind` terbuka, bukan enum bertag, supaya aksi baru menjadi minor dan client lama menyembunyikannya. Temuan dalam satu check diurutkan severity, lalu nama instance, lalu kode.

### Method IPC

| Method | Recovery | Params | Reply | Error utama |
| --- | --- | --- | --- | --- |
| `doctor.run` | allowed | `DoctorParams` | `DoctorReport` | `request.invalid_params`, `resource.not_found` |
| `diagnostics.preview_bundle` | allowed | tanpa params | `BundlePreview` | `diagnostics.bundle_failed` (reason `no_space`, `permission`, `io`) |
| `diagnostics.export_bundle` | allowed | `ExportBundleParams { previewId, destination, replace }` | `ExportedBundle { path, bytes, sha256 }` | `diagnostics.preview_not_found`, `destination_exists`, `destination_invalid`, `write_failed` |

`BundlePreview { previewId, createdAt, expiresAt, suggestedFileName, entries: [BundleEntry], totalBytes: Count, zipBytes: Count, recovery: bool }`, `BundleEntry { path, kind: BundleEntryKind, description, bytes: Count, lines: Count?, truncated: bool }`, `BundleEntryKind` terbuka (`readme`, `doctor`, `system`, `instances`, `runtimes`, `operations`, `daemon_log`, `instance_log`, `agent`, `unrecognized`). `suggestedFileName` adalah `stackly-diagnostics-<YYYYMMDD>T<HHMMSS>Z.zip` dari `createdAt` UTC.

Kontrak ini menaikkan `ProtocolVersion::CURRENT` satu minor (minor bebas berikutnya, sementara 3.5, ditetapkan lead saat build) dengan codegen, fixture di `protocols/fixtures/`, dan entri katalog error. Tidak ada event kind baru dan tidak ada migrasi registry.

### Kode error baru

| Kode | Kategori | Exit CLI | Details |
| --- | --- | --- | --- |
| `diagnostics.preview_not_found` | `not_found` | 10 | `previewId` |
| `diagnostics.destination_exists` | `conflict` | 3 | `path` |
| `diagnostics.destination_invalid` | `invalid_input` | 2 | `reason`, `path` |
| `diagnostics.write_failed` | `failed` | 7 | `reason`, `path` |
| `diagnostics.bundle_failed` | `failed` | 7 | `reason` |
| `cli.doctor_found_errors` | `failed` | 7 | `errors`, `warnings` |

Hint: `destination_exists` "Choose another path, or pass --force to replace it."; `preview_not_found` "Preview the bundle again."; `doctor_found_errors` "Each error above says what to do next.".

### Bukti identitas pasif

`gather_evidence` (`stackly_core::supervisor::evidence`) dipecah menjadi dua langkah dengan satu jalur kode: `observe_passive(platform, lock_path, recorded) -> PassiveEvidence` menjalankan ketujuh check spec 0010 (`boot_matches` sampai `holds_lock`, yang membaca fd proses lewat `proc_pidfdinfo`, bukan lock) dan mengembalikan `IdentityEvidence` dengan `lock` null ditambah `ProcessFacts` proses yang kini memegang PID itu; `gather_evidence` memanggilnya lalu menambah `probe_lock` dan klasifikasi seperti hari ini, sehingga perilaku supervisor tidak berubah. Untuk PID milik user lain, `inspect` tidak memberi fakta; `ProcessRef.sameUser` false dan `executable` null, dan penjelasan menulis "PID {pid} now belongs to a process of another user." Ia tidak memanggil `probe_lock` atau `lock_instance`, sehingga doctor tidak pernah memegang `flock` atas `instance.lock`, bahkan sesaat. `lock` di bukti diisi dari blocker tersimpan (`IdentityBlocker.evidence.lock`) kalau ada, beserta `observedAt` blocker itu. Instance yang sedang dipegang operasi aktif tetap diperiksa, karena pemeriksaan pasif tidak bersaing dengan start atau stop.

### Pemilik port

Trait baru `ListenerLookup` di `stackly-platform` (`fn owners(&self, transport: Transport, address: IpAddr, port: u16) -> Result<Vec<ListenerOwner>, ProcessError>`), dengan `ListenerOwner { pid, executable: Option<PathBuf>, start: Option<StartTime> }`. Implementasi macOS di modul `process` `stackly-platform-macos`: `proc_listpids(PROC_UID_ONLY, uid)`, lalu `PROC_PIDLISTFDS` dan `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)` per PID untuk socket TCP `LISTEN` atau UDP terikat di port itu dengan alamat yang sama atau wildcard; paling banyak 4096 PID dan 2 detik. Hanya proses milik user ini yang terlihat, jadi pemilik root atau user lain dilaporkan `owner` `unknown` dengan kalimat "No process of yours holds it; another user or the system may." Implementasi `FakeProcessSupervision` di testkit dan `Unsupported` untuk target Windows. Setiap blok `unsafe` diberi `// SAFETY:`.

### Blok remedies

```yaml
remedies:
  - name: stale_postmaster_pid
    when:
      failureCodes: [crash_loop, process_exited]   # opsional; cocok dengan lastFailure.code
      excerptContains: 'lock file "postmaster.pid" already exists'
    evidence:
      pidFile: "${instance.data}/postmaster.pid"
    title: "PostgreSQL found an old postmaster.pid"
    steps:
      - text: "Stackly never removes this file, because it also guards the data folder against a second server."
      - text: "If the PID below isn’t a PostgreSQL server for this data folder, remove the file yourself, then start the instance again."
        path: "${instance.data}/postmaster.pid"
```

- `when` memuat paling sedikit satu dari: `failureCodes` (1 sampai 8 kode `lastFailure`), `excerptContains` (1 sampai 200 karakter, dicocokkan peka huruf terhadap setiap baris `excerpt`), `readinessReasons` (1 sampai 8), `readinessDetailContains` (1 sampai 200 karakter). Semua yang ada harus cocok.
- `title` 1 sampai 80 karakter, `steps` 1 sampai 6, `text` 1 sampai 300, `command` 1 sampai 500, `path` 1 sampai 300; tanpa karakter kontrol. Token hanya di `command`, `path`, dan `evidence.pidFile`, dirender dengan resolver interpolasi spec 0009 lewat posisi baru `ValuePosition::RemedyText`, dengan cek folder managed yang sudah ada; `evidence.pidFile` harus di bawah `${instance.data}` atau `${instance.config}`.
- Blok ini ikut `manifest_sha256` dan schema editor di `schemas/provider/` lewat `cargo xtask codegen`. Karena remedy hanya teks dan tidak pernah dijalankan, doctor memakai revision catalog supaya instance yang di pin ke paket lama tetap mendapat remedy baru.

### Entri PostgreSQL

`providers/postgresql/provider.yaml` mendapat dua entri:

- `stale_postmaster_pid` seperti contoh di atas. Penjelasan bukti `pidFile`: `not_running` berarti "No process has PID {pid}. The file is stale."; `other_process` berarti "PID {pid} now belongs to {executable}, not PostgreSQL."; `runtime_process` berarti "PID {pid} is a PostgreSQL server from this runtime. Stop it before you remove the file." (spec 0019 baris 463).
- `readiness_role`: `when.readinessDetailContains: 'role "stackly_readiness"'`; langkah menjelaskan bahwa Stackly memeriksa kesiapan lewat role itu, lalu `command` untuk dijalankan sendiri di Terminal dengan `${runtime.executables.psql}`, `--host` socket instance, `--port=${endpoint.client.port}`, dan user superuser instance, berisi `CREATE ROLE` dan `ALTER ROLE` yang sama dengan init paket itu. Password tidak pernah masuk perintah; `psql` memintanya sendiri (spec 0019 baris 464).

### Catatan gagal start daemon

`DirectoryLayout::daemon_start_failure_record()` adalah `<data>/daemon-start-failure.json`: `{ "schema": 1, "at", "exitCode": 1, "code", "message", "version", "launch": "launchd" | "manual", "bundlePath" }`, dengan `code` berupa kode katalog error start itu kalau ada, selain itu `daemon.start_failed`, dan `message` berupa teks error itu lewat pola bawaan `Redactor` (belum ada nilai yang dipelajari saat itu) dan dipotong 512 karakter. Ditulis `main` `stacklyd` lewat `write_file_atomic` hanya untuk exit 1 setelah root diketahui; exit 2, 3, dan 75 tidak ditulis. Dua start yang gagal bersamaan saling menimpa secara atomik; yang terakhir menang. CLI membaca file itu di root aktif (termasuk `STACKLY_ROOT`), GUI di root default, dengan aturan baca launch record (paling banyak 4 KiB, yang tidak terbaca berarti tidak ada). File yang lebih tua dari `DaemonInfo.startedAt` daemon yang sedang jalan tidak ditampilkan.

### Isi bundle

| Path | Isi | Sumber | Redaksi |
| --- | --- | --- | --- |
| `README.txt` | waktu dibuat, versi Stackly dan protokol, daftar entri, apa yang terpotong, apa yang tidak pernah ikut | dibangun saat preview | `Redactor`, `~` |
| `doctor.json` | `DoctorReport` dari run preview | `doctor.run` | `Redactor`, `~` |
| `system.json` | versi Stackly dan protokol, versi dan build macOS, arsitektur, konteks launch, `startedAt`, recovery, `CapacityReport`, path root | `DaemonInfo`, `DaemonHealth`, `PlatformInfo::host_facts()` (sumber yang sama dengan compatibility record spec 0018, tanpa spawn), `RootSet` | `Redactor`, `~` |
| `instances.json` | per instance: ID, nama, provider dan revision, runtime, lifecycle, desired, readiness, restart, `lastFailure`, blocker, endpoint, nomor revision config, settings (secret hanya reference), startup | fungsi core yang sama dengan handler `instance.list` dan `instance.get_detail`, dipetakan ke DTO yang sama | `Redactor`, `~` |
| `runtimes.json` | per runtime: software, versi, source, state, sha256, path | fungsi core di balik `runtime.list` | `Redactor`, `~` |
| `operations.json` | gabungan 100 operasi terbaru (`OperationQuery` limit 100) dan semua `needs_recovery`, tanpa duplikat ID, urut `(createdAt, id)` turun, DTO operasi apa adanya | registry operasi | `Redactor`, `~` |
| `logs/stacklyd.log` | 2000 baris terakhir dari `stacklyd.log` sampai `stacklyd.log.4` (rotasi spec 0006 AC-12, di `DirectoryLayout::daemon_log_dir()`), paling banyak 4 MiB | file log daemon | `Redactor`, `~` |
| `logs/instances/<nama>.log` | 2000 baris terakhir per instance dari semua stream urut `seq`, satu baris per entri `<RFC 3339> <stream> <teks>`, paling banyak 1 MiB | baca log store spec 0027 (fungsi di balik `instance.logs_read`) | tanpa `Redactor` lagi, `~` |
| `agent/background-agent.json`, `agent/daemon-start-failure.json` | launch record dan catatan gagal start bila ada | file di `<data>` | `Redactor`, `~` |

Nama instance mengikuti VAL-001 (huruf kecil, angka, dan tanda hubung), unik, dan aman sebagai nama file, jadi dipakai apa adanya. Batas 64 MiB dihitung sebelum kompresi dengan urutan anggaran: `README.txt`, `doctor.json`, `system.json`, `instances.json`, `runtimes.json`, `operations.json`, `agent/`, `logs/stacklyd.log`, lalu log instance urut entri log terbaru. Entri JSON tidak pernah dibuang; log yang tidak muat atau instance di luar 50 pertama tidak ikut dan disebut di `README.txt`. Setiap entri zip memakai waktu `createdAt` preview supaya isi yang sama memberi byte yang sama.

Tidak pernah ikut: file config yang dirender (bisa memuat secret plaintext, spec 0011 residual), folder data, file registry, nilai Keychain, environment proses, dan audit spec 0035. Zip memakai crate `zip` yang sudah ada (deflate lewat `flate2`), satu file per entri, tanpa path absolut atau `..`.

### Penggantian folder home

`HOME` dibaca sekali saat daemon start, dalam bentuk mentah dan canonical. Kalau `HOME` tidak ada, relatif, atau `/`, tidak ada penggantian. Penggantian berjalan sesudah `Redactor` dan mengganti setiap kemunculan bentuk mentah atau canonical yang diikuti `/`, akhir teks, atau karakter selain huruf, angka, `.`, `_`, dan `-`, jadi `/Users/ojie` tidak pernah mengganti bagian dari `/Users/ojiepermana`. Bentuk JSON escape dan percent encode tidak diganti.

### Staging preview

- Folder `<cache>/diagnostics/` (mode 0700) dibuat saat preview pertama dan dikosongkan setiap daemon start.
- Daemon memegang peta preview di memori: `previewId` ke `Arc` berisi fd zip yang terbuka, sha256 yang dihitung saat build, `createdAt`, dan `expiresAt`.
- Kedaluwarsa dicek saat lookup dan setiap preview baru dibuat; preview yang kedaluwarsa atau tergusur (preview ketiga menggusur yang paling lama) dihapus dari peta dan file nya di unlink, sedangkan export yang sedang berjalan tetap menyalin dari fd nya.
- Dua export dari preview yang sama boleh berjalan bersamaan; masing masing punya file partial sendiri.

### CLI

```text
$ stackly doctor
Connection        ✓ Connected to stacklyd 0.9.0 over the Unix socket
Protocol          ✓ App and daemon both speak protocol 3.5
Background agent  ✓ Login item registered and allowed

Endpoints         ✗ Port conflict · mysql-legacy
                    127.0.0.1:3307 is held by mysqld (PID 812, ~/homebrew/bin/mysqld). Not owned by Stackly.
                    Next: stackly instance endpoint set mysql-legacy --port mysql=3308
Disk space        ! Data volume is 82% full · 41 GB free
                    Next: stackly storage list
Registry          ✓ quick_check passed · schema 11
...
2 errors, 1 warning · 13 checks · run 0193…
```

- Simbol `✓`, `!`, `✗` di TTY berwarna, teks `OK`, `WARNING`, `ERROR` tanpa warna (aturan warna spec 0008). Bukti, excerpt, dan remedy ditulis menjorok di bawah temuannya.
- Baris `Next:` per aksi: `show_instance` → `stackly instance inspect <nama>`, `review_config` → `stackly instance config get <nama>`, `change_port` → `stackly instance endpoint set <nama> --port <endpoint>=<port>`, `show_operation` → `stackly operation inspect <id>`, `open_storage_settings` → `stackly storage list`. Aksi `unrecognized` tidak ditulis.
- `stackly diagnostics preview` menulis tabel path, deskripsi, dan ukuran, lalu "Secrets and connection strings are redacted. Paths in your home folder are shown as ~. Nothing is uploaded." `export` menulis daftar yang sama ke stderr, lalu `Save this bundle to <path>?` dan, setelah sukses, `Saved <path> (<ukuran>).` ke stdout.
- Progress ("Running the doctor…", "Preparing the bundle…") ke stderr. Golden file untuk teks dan `--json`. `--check` menerima nilai snake_case `DoctorCheckId` daemon.

### GUI

Folder baru `StacklyUI/Diagnostics/Doctor/` dan `StacklyUI/Diagnostics/Bundle/`:

- `DoctorModel` (`@MainActor @Observable`, dimiliki `DaemonModel` seperti `CapacityModel`): `report: DoctorReport?`, `running: Bool`, `failure: ErrorBody?`, `unsupported: Bool`, `lastRunAt: Date?`; `run()` dengan paling banyak satu call berjalan; aturan auto run di AC-20; `errorCount` untuk badge.
- `DoctorSection`: header "Doctor" dengan baris meta, `CheckList(.single)` dari `DoctorPresentation.items(report:, focus:, actions:)` (fungsi murni, diuji tanpa view), tombol `CopyButton` "Copy Report" yang menyalin teks laporan seperti CLI tanpa warna.
- `DoctorFindingSheet` (`StacklySheet`): penjelasan, tabel bukti (Recorded process, Current process, tiap check identitas dengan Passed, Failed, atau Not checked, Lock, Suspended, Observed), excerpt dalam `ConsoleBlock`, langkah remedy dengan `ConsoleBlock` varian perintah dan `CopyButton` untuk `command` dan `path`, lalu aksi aman sebagai tombol primary (atau "Done").
- `DiagnosticBundleSection`: judul "Diagnostic Bundle", teks "A bundle holds the doctor report, the instance list, recent logs, runtime digests, and system facts. Secrets and connection strings are redacted. The bundle is saved locally and never uploaded.", dan "Preview Bundle…".
- `BundleSheet` dan `BundleModel`: alur AC-19. Path dari `NSSavePanel` dikirim apa adanya; "Show in Finder" lewat `NSWorkspace.activateFileViewerSelecting`.
- `BackgroundAgentDiagnosticsSection` (spec 0015) mendapat baris "Last start failure" (waktu dan pesan dari catatan gagal start) dan teks What to do untuk `binary_replaced`; tombol Start stacklyd yang sudah ada tetap aksinya.
- `DoctorActionRoute` memetakan `DoctorActionKind` ke label dan tujuan: "Show Instance", "Review", "Use Port {port}", "Show Operation", "Open Settings". `change_port` memakai `InstanceRequest` dengan prefill endpoint dan port untuk `EndpointsSheet`.

**State seksi Doctor**: belum ada laporan dan sedang run: `DelayedProgress` dengan "Checking permissions, runtimes, endpoints, locks, disk and registry…"; gagal tanpa laporan: `StatePlaceholder(.section)` "Couldn’t Run the Doctor" dengan pesan envelope dan Retry; `request.unknown_method`: "Doctor Unavailable" dengan "Update stacklyd to run the doctor."; daemon tidak terhubung: laporan terakhir tetap tampil (tanpa diredupkan, spec 0012) dengan meta "Last run {waktu} · stacklyd isn’t connected", Run Doctor dan Export Bundle nonaktif; tanpa laporan dan tidak terhubung: "Connect to stacklyd to run the doctor."; recovery: banner critical recovery spec 0013 tetap, dan laporan menampilkan check `check_unavailable` sebagai Warning. Fokus instance: label "Showing “{nama}”" dan "Show All".

Mockup menggambar daftar isi bundle langsung di halaman; di sini daftar itu ada di sheet, karena isi nyata hanya ada setelah daemon membangun preview. Selisih ini dicatat untuk `docs/scope/ui-macos.md` (*Usulan update*).

### Runbook PRD bagian 35.3 di doctor

| Kondisi runbook | Yang doctor tunjukkan |
| --- | --- |
| Daemon tidak terhubung | check client `connection`, `protocol`, `background_agent`, catatan gagal start, `binary_replaced`; tidak pernah menghapus registry |
| Port conflict | `port_in_use` dengan pemilik dan port saran; ganti port hanya lewat `EndpointsSheet` |
| Runtime missing atau drifted | `runtime_state`, `runtime_files_missing`; tidak ada rebind ke `latest` dan tidak ada reinstall (fitur 55) |
| Database start gagal | `start_failed` dengan excerpt, remedy provider, `permissions`, `disk_space`, `processes`; tidak pernah reset data atau hapus PID |
| Disk penuh | `disk_space` dengan Open Settings; tidak pernah purge |
| Crash loop | `start_failed` dengan `crash_loop` (spec 0033) |

Backup, restore, update, provider revoked, dan uninstall milik fitur pemiliknya masing masing.

### Peta crate dan modul

| Crate | Tambahan |
| --- | --- |
| `stackly-domain` | `doctor` modul: `DoctorSeverity`, `DoctorCheckId`, `DoctorFindingCode`, aturan severity check (fungsi murni), batas temuan |
| `stackly-platform` | trait `ListenerLookup`, `ListenerOwner`, unsupported impl; `DirectoryLayout::daemon_start_failure_record()` dan `diagnostics_cache_dir()` |
| `stackly-platform-macos` | `process::listeners` (unsafe, `SAFETY`), tanpa spawn |
| `stackly-storage` | `Registry::quick_check()` dan `Registry::scan_unrecognized()` lewat koneksi baca |
| `stackly-core` | `doctor` modul (satu fungsi per check, runner dengan batas waktu, bendera batal, dan semaphore 2), `supervisor::evidence::observe_passive`, `diagnostics::bundle` (builder, staging, export, batas), `provider` raw type dan validasi `remedies`, `ValuePosition::RemedyText` |
| `stackly-protocol` | modul `doctor` dan `diagnostics` (DTO di atas), tiga method di `methods!`, kode error, bump minor |
| `stacklyd` | `ipc::doctor` (handler, mapping domain ke DTO, tabel teks `copy`), `ipc::diagnostics`, penulisan catatan gagal start di `main`, pembersihan `<cache>/diagnostics/` saat start |
| `stackly-cli` | `commands/doctor.rs`, `commands/diagnostics.rs`, check client `connection`, `protocol`, `background_agent` di `commands/doctor/local.rs` |
| `StacklyUI` | `Diagnostics/Doctor/`, `Diagnostics/Bundle/`, `DiagnosticsFocus`, `ShellNavigator.openDiagnostics(focus:)`, perluasan `BackgroundAgentDiagnosticsSection`, fixture spec 0068 |
| `providers/postgresql` | dua entri `remedies` |

`cargo xtask check-deps` tetap utuh: tidak ada spawn baru, `SIGKILL` baru, atau `expose` baru, dan `stackly-core` tidak memanggil API macOS.

### Data model sketch

- Registry: tidak ada tabel, kolom, atau migrasi baru. Doctor hanya membaca.
- File baru milik daemon: `<data>/daemon-start-failure.json` (*Catatan gagal start daemon*, satu file, ditimpa atomik, dihapus setelah start berhasil) dan `<cache>/diagnostics/<previewId>.zip` (paling banyak 2, kedaluwarsa 15 menit, folder dikosongkan saat daemon start).
- Di memori daemon: peta preview `previewId → { path, sha256, createdAt, expiresAt }` dan semaphore 2 slot run doctor. Hilang saat daemon restart, dan preview yang hilang memberi `diagnostics.preview_not_found`.
- Di memori GUI: `DoctorModel` dan `BundleModel`; tidak ada `@AppStorage` atau file.
- Bentuk wire ada di *Laporan doctor* dan *Method IPC*.

**State transitions**: tidak ada state machine persisten. `DoctorModel`: `idle` (tanpa laporan) → `running` → `ready(report)` atau `failed(error)`, dan `ready` → `running` lagi dengan laporan lama tetap tampil; `unsupported` tetap sampai `ShellSync` mendapat daemon baru. `BundleModel`: `loading` → `previewed(preview)` → `exporting` → selesai (sheet ditutup) atau kembali ke `previewed` dengan error; `preview_not_found` → `loading`.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| `doctor.run` | daftar instance, lifecycle, desired, observasi proses, blocker, `restart`, `lastFailure`, readiness | registry dan state supervisor yang sama dengan `instance.list` dan `instance.get_detail` |
| `doctor.run` | bukti identitas pasif, proses yang kini memegang PID | `observe_passive` atas `ProcessIdentity` tercatat dan path `instance.lock` dari `InstancePaths` |
| `doctor.run` | pemilik port, port saran | `ListenerLookup`, `instance::create::port::judge_change` dengan `SocketPortProbe` |
| `doctor.run` | operasi `needs_recovery`, hint, cause | registry operasi (spec 0007) |
| `doctor.run` | folder scratch yatim | daftar `<data>/operations/` dibanding baris operasi |
| `doctor.run` | state dan executable runtime | registry runtime dan `RuntimeDetail.executables` |
| `doctor.run` | paket yang tidak terbaca | loader paket pinned (spec 0009) |
| `doctor.run` | remedy | blok `remedies` dari revision catalog provider, selain itu paket pinned |
| `doctor.run` | drift config | blocker `configDrift` dan `supervisor::drift::check` |
| `doctor.run` | kapasitas | pembacaan `storage.get_capacity` |
| `doctor.run` | quick_check, nilai tidak dikenal, versi schema | `Registry::quick_check`, `scan_unrecognized`, `registry_info` |
| `doctor.run` | judul dan penjelasan | tabel teks `stacklyd::ipc::doctor::copy` dari kode dan details |
| check client | state agent, launch record, waktu ubah binary, catatan gagal start | `BackgroundAgent` spec 0015, `<data>/background-agent.json`, metadata `Contents/MacOS/stacklyd` di `bundlePath`, `<data>/daemon-start-failure.json` |
| `preview_bundle` | folder home untuk `~` | `HOME` yang dibaca dan dibuat canonical sekali saat daemon start |
| `preview_bundle` | `previewId`, `createdAt`, `expiresAt` | dibuat daemon (uuid v7, jam daemon, 15 menit) |
| `export_bundle` | `destination` | input client: path dari `NSSavePanel` atau `--output` yang dibuat absolut oleh CLI |
| GUI | waktu "Last run" | `DoctorReport.finishedAt` diformat `DiagnosticsTime.format` |
| GUI | badge, subjudul | `errors` dan `warnings` laporan terakhir |
| GUI `change_port` | endpoint dan port yang diisi | `DoctorAction.endpoint` dan `.port`; revision base dibaca `EndpointsSheet` sendiri |

### Key invariants

- Doctor dan preview bundle tidak pernah mengubah state Stackly: tidak ada tulisan registry, operasi, sinyal, spawn, `flock` atas `instance.lock`, atau perubahan file di root selain staging bundle.
- Tidak ada aksi baru yang mengubah data: setiap aksi adalah navigasi atau membuka alur yang sudah ada dengan konfirmasinya sendiri.
- Secret tidak pernah ada di laporan, bundle, atau output CLI; satu satunya entri yang tidak lewat `Redactor` adalah log instance yang sudah disamarkan saat ditulis.
- Export menyalin byte preview persis; daemon tidak pernah menimpa file tanpa `replace` dan tidak pernah meninggalkan file partial.
- Check client tidak pernah menjalankan `launchctl` atau proses lain.
- Laporan selalu kembali dalam 15 detik plus waktu serialisasi, walau ada check yang macet.

### Security model

- Semua method mengikuti handshake dan pemeriksaan peer spec 0006 (uid yang sama). App tidak di sandbox (`ENABLE_APP_SANDBOX = NO`), jadi path dari `NSSavePanel` bisa ditulis daemon; kalau app kelak di sandbox, alur ini perlu diputuskan ulang. Export menulis ke path pilihan client dengan hak user yang sama dengan client itu, jadi tidak ada kenaikan hak; tujuan symlink tidak diikuti untuk komponen terakhir (rename mengganti symlink itu sendiri).
- Bundle memuat nama instance, settings, port, dan path ber `~`, yang adalah data milik user sendiri; preview menunjukkan semuanya sebelum disimpan, dan tidak ada jalur upload (NFR-016, REQ-046).
- `ListenerLookup` hanya melihat proses user ini; ia tidak pernah membaca memori atau argumen proses lain.
- Remedy dari manifest adalah teks yang dirender tanpa `secret.*`; perintah di remedy hanya ditampilkan dan disalin, tidak pernah dijalankan Stackly.

### Configuration required

Tidak ada env var, setting, atau credential baru.

### Critical test scenarios

- Happy path: daemon dengan dua instance fake, satu `failed` karena port dipegang listener test, `doctor.run` memberi `port_in_use` Error dengan PID listener dan port saran, check lain OK, dan `stackly doctor` keluar 7, verifies **AC-1**, **AC-5**, **AC-15**
- PID dipakai ulang: observasi instance ditulis dengan PID proses `sleep` milik test dan waktu start lain; doctor memberi `pid_reused` dengan bukti; `sleep` tetap hidup, `instance.lock` dan file data identik (hash sebelum dan sesudah), lifecycle tidak berubah, `FakeProcessSupervision` mencatat nol `probe_lock`, `lock_instance`, `signal`, verifies **AC-2**, **AC-3**
- Recovery: registry dengan `registry_info` rusak; `doctor.run` membalas `recovery: true`, `registry` Error, `disk_space` OK, check lain `check_unavailable`; `preview_bundle` berhasil tanpa entri registry, verifies **AC-4**, **AC-12**
- Operasi dan scratch: satu operasi `needs_recovery` hint `abandon` dan satu folder scratch tanpa baris; dua temuan, folder tetap ada, verifies **AC-6**
- Nilai kode tidak dikenal: satu baris dengan `lifecycle` `zzz` ditulis langsung lewat SQL di test root; `unrecognized_value` dengan tabel, baris, kolom, verifies **AC-8**
- Remedy: excerpt fixture memuat teks `postmaster.pid`; `start_failed` membawa remedy dengan `pidFile` `other_process`; paket dengan token `secret.password` di remedy ditolak loader, verifies **AC-9**, **AC-10**
- Batas waktu: check yang ditahan fake lebih dari 5 detik menjadi `check_unavailable` timeout dan laporan kembali sebelum 15 detik (jam tokio dijeda), verifies **AC-1**
- Sentinel: alur AC-14 di daemon integration test dan golden CLI, verifies **AC-14**
- Export: tujuan sudah ada tanpa `replace` memberi `destination_exists` dan file lama utuh; `replace` mengganti; folder induk read only memberi `write_failed` `permission` tanpa file partial; preview kedaluwarsa (jam fake) memberi `preview_not_found`; sha256 file sama dengan zip staging, verifies **AC-13**
- Gagal start daemon: `STACKLY_ROOT` override bermode 0777 (ditolak spec 0004 AC-7) dengan `data/` 0700 di dalamnya; daemon keluar 1 dan menulis catatan; catatan bermode 0644 atau milik symlink diabaikan reader; `stackly doctor` menampilkan `daemon_start_failed` dan keluar 6; start berikutnya yang berhasil menghapus catatan, verifies **AC-11**
- GUI: `DoctorPresentation` untuk tiap kombinasi severity, aksi, fokus, dan aksi `unrecognized`; `BundleModel` dengan daemon skrip untuk preview, kedaluwarsa, dan error export; snapshot fixture, verifies **AC-17**, **AC-18**, **AC-19**, **AC-20**, **AC-21**
- Parameter tidak sah: `checks` berisi `connection` atau ganda ditolak `request.invalid_params`; `instanceId` asing `resource.not_found`, verifies **AC-1**

### Menjalankan test di Mac ini

Semua test Rust dan Swift di atas berjalan otomatis dengan `TestRoot`, `allocate_port`, `FakePlatform`, dan proses `sleep` atau listener TCP milik test yang di reap guard. Dua bukti butuh tangan atau mata kamu dan dicatat `BLOCKED` sampai dijalankan:

- Manual A (launchd asli): dengan build bertanda tangan yang terdaftar sebagai agent, ganti binary lewat `cargo xtask build-app --install`, bunuh daemon, buka Diagnostics dan jalankan `stackly doctor`; `binary_replaced` tampil, lalu Start stacklyd menyalakan daemon lagi.
- Manual B (GUI): satu lintasan nyata di app: port conflict dengan Use Port sampai `EndpointsSheet`, sheet detail `pid_reused`, export bundle ke Downloads lewat save panel, dan Show in Finder.

### Jawaban untuk Follow-up spec lain

| Follow-up | Jawaban di spec ini |
| --- | --- |
| 0007 baris 833 | AC-6 |
| 0010 baris 745 dan 681 | AC-2, AC-3, *Bukti identitas pasif* |
| 0015 baris 722, 76, 694 | AC-11 (tanpa `launchctl`; `spawn failed` dan last exit 78 dikenali sebagai `binary_replaced`; sebab launchd lain tetap tidak terlihat, *Consequences*) |
| 0016 baris 456 | AC-20 |
| 0019 baris 632, 463, 464 | AC-10, *Entri PostgreSQL* |
| 0022 baris 591 | AC-20 (`CapacityModel`, item S ke fokus `disk_space`), padanan CLI di check `disk_space` `stackly doctor` |
| 0013 baris 734 | AC-17 |
| 0067 baris 833 | AC-17 (`CheckList(.single)`); `CopyButton` dipakai "Copy Report" |
| 0011 baris 612 | AC-12, AC-14 |
| 0005 baris 430 dan 651 | AC-8, AC-4 |
| 0018 baris 68, 287, 610 | AC-7 dan AC-6 (spec ini melaporkan state, file yang hilang, dan `install_conflict`; verifikasi ulang, transisi state, dan perbaikan tetap fitur 55; external fitur 56) |
| 0021 baris 777 | AC-7 (`package_unreadable`) membuat teks yang menyuruh `stackly doctor` benar |

### Usulan update untuk spec lain

Diterapkan lead lewat `/sync`, bukan oleh spec ini:

1. Spec 0013: `ShellDestination.diagnostics(focus:)`, `openDiagnostics(focus:)`, badge Diagnostics, dan hilangnya `UnbuiltPage(.diagnostics)`.
2. Spec 0015: baris "Last start failure" dan `binary_replaced` di `BackgroundAgentDiagnosticsSection` dan `stackly daemon status`; catatan gagal start ditulis `main` (`DirectoryLayout::daemon_start_failure_record`).
3. Spec 0009: blok `remedies` dan `ValuePosition::RemedyText`.
4. Spec 0004: `ListenerLookup`, `daemon_start_failure_record`, `diagnostics_cache_dir`.
5. Spec 0022: item S menuju Diagnostics dengan fokus `disk_space`.
6. Spec 0033: tombol "Diagnose…" dan hint `stackly doctor --instance` tidak lagi provisional.
7. `docs/scope/ui-macos.md`: daftar isi bundle di sheet, bukan di halaman; badge Diagnostics hanya menghitung Error.

## Build plan

Approach Journey: satu jalur pengguna (instance gagal, lalu doctor menjelaskan sebabnya, lalu bundle untuk minta bantuan) dibangun lengkap dengan semua state sebelum fitur berikutnya. Urutannya dari dalam ke luar: kontrak dan check daemon dulu supaya CLI dan GUI bisa dibangun di atasnya, lalu bundle, lalu kedua client, lalu bukti native dan manual.

**Milestone 1: kontrak dan check daemon**

1. Domain `doctor` (severity, ID check dan kode temuan, aturan severity), DTO `stackly-protocol` modul `doctor`, method `doctor.run` (`recovery allowed`), kode error, bump minor, codegen, dan fixture kontrak, satisfies **AC-1**
2. `ListenerLookup` di `stackly-platform` dan `stackly-platform-macos` (unsafe dengan `SAFETY`), fake di testkit, unsupported untuk Windows, integration test dengan listener TCP dan UDP sungguhan, satisfies **AC-5**
3. `Registry::quick_check` dan `scan_unrecognized` di `stackly-storage` dengan test baris rusak, satisfies **AC-8**
4. `stackly-core::doctor`: runner (batas 5 dan 15 detik, bendera batal dan interrupt SQLite, semaphore 2 slot), `observe_passive`, dan check `daemon`, `registry`, `permissions`, `disk_space`, `runtimes`, `provider_packages`, `endpoints`, `processes`, `instance_files`, `instance_failures` (tanpa remedy), `config_drift`, `secrets`, `operations`, dengan unit test terhadap fake testkit, termasuk penghitung nol `probe_lock`, `lock_instance`, `signal`, spawn, dan `mutations()`, satisfies **AC-1**, **AC-2**, **AC-3**, **AC-4**, **AC-5**, **AC-6**, **AC-7**, **AC-8**, **AC-9**
5. `stacklyd::ipc::doctor` (handler, mapping, tabel teks) dan integration test di `apps/daemon/tests/doctor.rs` dengan `TestRoot`, proses `sleep` sungguhan, listener test, dan registry recovery, satisfies **AC-1**, **AC-2**, **AC-3**, **AC-4**, **AC-5**, **AC-6**, **AC-22**

**Milestone 2: remedy provider dan catatan gagal start**

6. Blok `remedies` di raw type, validasi, dan `ValuePosition::RemedyText` di `stackly-core::provider`, issue code, `manifest_sha256`, schema editor lewat `cargo xtask codegen`, pencocokan di `instance_failures`, bukti `pidFile`, satisfies **AC-10**, **AC-9**
7. Dua entri remedy di `providers/postgresql/provider.yaml`, golden paket (`STACKLY_UPDATE_GOLDEN=1`), `cargo xtask provider check`, satisfies **AC-10**
8. Catatan gagal start di `main` `stacklyd` dan penghapusannya setelah bind, `DirectoryLayout::daemon_start_failure_record`, test binary dengan root mode 0777, satisfies **AC-11**

**Milestone 3: bundle**

9. `stackly-core::diagnostics::bundle`: builder entri, `Redactor` dan `~`, batas, staging di `<cache>/diagnostics/`, kedaluwarsa dan batas 2 preview, export partial lalu rename, pembersihan saat start; DTO dan dua method `diagnostics.*` (`recovery allowed`), kode error, codegen, satisfies **AC-12**, **AC-13**
10. Handler `stacklyd::ipc::diagnostics`, baris log `diagnostics_exported`, integration test export (ada, replace, folder read only, kedaluwarsa, sha256) dan suite sentinel lintas laporan, bundle, dan log daemon, satisfies **AC-12**, **AC-13**, **AC-14**, **AC-22**

**Milestone 4: CLI** (needs milestone 1; bagian `diagnostics` needs milestone 3)

11. `stackly doctor` dengan check client `connection`, `protocol`, `background_agent` (catatan gagal start, `binary_replaced`), `TargetArg`, `--check`, exit code, `cli.doctor_found_errors`, golden teks dan JSON, test daemon skrip dan `stacklyd` sungguhan, satisfies **AC-11**, **AC-15**, **AC-18**
12. `stackly diagnostics preview` dan `export` dengan `Confirm`, `--yes`, `--output`, `--force`, golden, dan sentinel di output CLI, satisfies **AC-14**, **AC-16**

**Milestone 5: GUI** (needs milestone 1; sheet bundle needs milestone 3; `show_operation` after spec 0035 `openOperation`, sampai itu aksi nya tidak tampil)

13. `DoctorModel`, `DoctorPresentation`, `DoctorSection`, `DoctorFindingSheet`, `DoctorActionRoute`, `DiagnosticsFocus`, `openDiagnostics(focus:)`, badge dan subjudul, `CapacityModel` dari `disk_space`, item S Overview, perluasan `BackgroundAgentDiagnosticsSection`, `OnboardingCopy.commandExamples`, test Swift Testing terhadap fixture kontrak, satisfies **AC-11**, **AC-17**, **AC-18**, **AC-20**
14. `DiagnosticBundleSection`, `BundleSheet`, `BundleModel` dengan `NSSavePanel` dan `FeedbackCard`, test dengan daemon skrip, satisfies **AC-19**
15. Skenario fixture spec 0068 dan snapshot light dan dark, satisfies **AC-21**

**Milestone 6: bukti manual** (langkah manual terpisah untuk file manual lead)

16. Manual A launchd asli dan Manual B lintasan GUI, dicatat di `verify.md` spec ini, `BLOCKED` sampai dijalankan, satisfies **AC-11**, **AC-17**, **AC-19**, **AC-22**

**Sesi paralel**: setelah milestone 1 masuk, milestone 2 (provider dan `main` daemon), milestone 3 (`diagnostics` di core dan daemon), dan milestone 5 langkah 13 (Swift saja) tidak berbagi file dan bisa dibangun paralel. Milestone 4 langkah 11 (CLI) juga terpisah dari Swift dan bisa jalan bersama langkah 13. Langkah 2 (platform macOS) dan langkah 3 (storage) bisa paralel di dalam milestone 1 sebelum langkah 4. Codegen dan bump protokol hanya di langkah 1 dan 9, jadi kedua langkah itu berurutan.

## Consequences

**Positive**:

- Satu sumber kebenaran untuk diagnosis: CLI dan GUI menampilkan laporan yang sama dengan teks yang sama, dan daemon baru bisa menambah check atau temuan tanpa merusak client lama (enum terbuka dan teks dari daemon).
- AC-019 dibuktikan dengan proses dan file asli, dan doctor tidak pernah memegang lock instance, jadi diagnosis tidak bisa bersaing dengan start.
- Banner spec 0033, item S Overview, dan teks `stackly doctor` di spec 0021 akhirnya punya tujuan yang berguna.
- Penulis provider bisa menambah panduan perbaikan tanpa kode Rust atau Swift.

**Negative / tradeoffs**:

- Daemon kini menulis satu file di luar root (tujuan export). Ini sengaja dan dibatasi create exclusive plus rename, tetapi menambah permukaan yang harus direview fitur 63.
- Tanpa `launchctl`, sebab kegagalan launchd selain binary yang diganti dan exit 1 yang tercatat tetap tidak terlihat; `binary_replaced` adalah inferensi dari waktu ubah file, jadi bisa muncul setelah update app lalu `stackly daemon stop` yang disengaja. Karena itu tingkatnya Warning dan teksnya menyebut "may".
- Daemon yang mati di tengah export meninggalkan `.stackly-diagnostics-<uuid>.partial` tersembunyi di folder tujuan; tidak ada pembersihan otomatis di luar root.
- `Redactor` hanya mengenali nilai yang sudah dipelajari daemon yang sedang jalan ditambah polanya. Baris log daemon lama yang memuat secret yang tidak pernah di resolve sejak start, dan tidak cocok pola, tidak tersamar di bundle; log daemon sendiri tidak pernah mencatat params (spec 0006), jadi kasus ini sempit.
- Bind probe di check `endpoints` bisa bertabrakan sesaat dengan start yang dimulai di detik yang sama, sama seperti preflight create hari ini; doctor melewati instance yang dipegang operasi aktif untuk mengecilkan jendela itu.
- Blok `remedies` mengubah `manifest_sha256` paket PostgreSQL; instance yang di pin tetap mendapat remedy lewat revision catalog, tetapi golden paket dan evidence yang memakai sha itu perlu diperbarui.
- Bundle hanya bisa dibuat saat daemon hidup; saat daemon mati, yang tersedia hanya check client.
- Laporan tidak disimpan daemon; GUI dan CLI masing masing menjalankan run sendiri.

**Neutral**:

- Tidak ada migrasi registry dan tidak ada event kind baru; kontrak naik satu minor.
- `CommandLineToolSection` tetap di halaman sampai keputusan spec 0032 tentang tempatnya; seksi Doctor ada di bawahnya.

## Follow-up

- [ ] Bundle saat daemon tidak terjangkau, dibangun client dari log daemon dan catatan gagal start, setelah diputuskan siapa yang boleh membaca log itu.
- [ ] Fitur 55: `runtime.verify` dan transisi `missing` dan `drifted` muncul di check `runtimes` tanpa perubahan kontrak; aksi reinstall ditambah saat itu sebagai `DoctorActionKind` baru.
- [ ] Fitur 63: review keamanan export ke path pilihan client, `ListenerLookup`, dan parser zip yang kini juga menulis.
- [ ] Fitur 35 (sudah disepakati): kalau audit perlu ikut bundle, entri `audit.json` dari `audit.list` ditambah sebagai `BundleEntryKind` baru.
- [ ] Remedy untuk provider lain (MySQL, Valkey, ClickHouse, NATS, Traefik) ditambah saat provider itu dibangun.
- [ ] Spec 0013, 0015, 0009, 0004, 0022, 0033, dan `ui-macos.md`: terapkan *Usulan update untuk spec lain* lewat `/sync`.
