# 0013. App shell SwiftUI dan Swift client

**Date**: 2026-10-18
**Status**: In Progress

## Summary

Spec ini menetapkan dua hal yang dipakai setiap layar GUI: Swift client yang bicara dengan `stacklyd` lewat socket Unix, dan shell jendela macOS yang membungkus semua layar. Client menghitung path socket dengan algoritma yang sama dengan daemon (dijaga fixture bersama), memeriksa socket sebelum mengirim satu byte pun, lalu memakai framing, handshake, dan resep snapshot plus cursor dari spec 0006 tanpa dependency pihak ketiga. Shell menampilkan sidebar, toolbar, status daemon, satu banner untuk keadaan daemon, diagnostics koneksi, dan gate mutasi, sehingga app tetap berguna dan jujur saat daemon mati: state terakhir tetap tampil beserta waktunya, tidak ada instance yang disebut berhenti, dan semua aksi yang mengubah data nonaktif. Menutup jendela tidak menghentikan apa pun; app tetap hidup dan memberi tahu pengguna sekali.

## Requirements

**User stories**:

- Sebagai pengguna Mac, saya ingin GUI tetap terbuka dan menjelaskan apa yang terjadi saat `stacklyd` mati, lengkap dengan tombol Reconnect dan diagnostics, supaya saya tahu langkah berikutnya tanpa mengira instance saya berhenti.
- Sebagai pengguna yang daemonnya sempat restart, saya ingin GUI menyusul sendiri ke state terbaru tanpa saya muat ulang, supaya yang saya lihat selalu sama dengan yang dijalankan daemon.
- Sebagai pengguna dengan dua akun di satu Mac, saya ingin GUI tidak pernah mengirim apa pun ke socket yang bukan milik daemon saya, supaya credential dan perintah saya tidak bocor ke akun lain.
- Sebagai pengguna yang menutup jendela, saya ingin instance dan daemon tetap berjalan dan diberi tahu bahwa Stackly masih hidup, supaya saya tidak bingung apakah service saya mati.
- Sebagai developer layar journey (fitur 17 sampai 56), saya ingin client, store berbasis snapshot plus cursor, adapter status, jalur baca dan mutasi, gate mutasi, dan bingkai halaman sudah tersedia, supaya layar saya hanya mengurus isinya sendiri.

**Acceptance criteria** (ID lokal spec ini ditulis `AC-n`; ID PRD selalu tiga digit dan disebut "PRD AC-0nn"):

- **AC-1**: `SocketPathResolver` di `StacklyClient` menurunkan runtime root dan path socket daemon dengan algoritma spec 0004: tanpa override, `<temp>/stackly`; dengan override, `<temp>/stackly-<fnv8>` dari byte mentah path canonical hasil `realpath`; lalu `/stacklyd.sock`. `<temp>` adalah hasil `confstr(_CS_DARWIN_USER_TEMP_DIR)` dengan satu garis miring di ujung dibuang. Override kosong, relatif, atau yang gagal di canonicalize ditolak dengan alasan yang jelas, dan runtime root lebih dari 76 byte ditolak. Setiap kasus di `protocols/fixtures/layout/socket-paths.json` lulus di test Rust (lewat `RootSet::under_override`, `default_runtime_root`, dan `DirectoryLayout::daemon_socket`) dan di test Swift. Integration test membuktikan path hitungan Swift sama dengan `DaemonInfo.socketPath` dari `stacklyd` asli yang dijalankan dengan override yang sama.
- **AC-2**: Sebelum connect, client memeriksa path socket lewat `lstat` (tanpa mengikuti symlink) dan folder induknya, lalu setelah connect membaca uid server lewat `getpeereid`. Keputusan cek dan urutannya, serta pemetaan errno ke `details.kind`, mengikuti fixture bersama `protocols/fixtures/endpoint/checks.json` yang juga dibaca test Rust `stackly-ipc`. Kegagalan menghasilkan `ipc.endpoint_untrusted` dengan `details.reason` dari daftar tetap spec 0006, dan server palsu di test menghitung nol byte diterima. Path yang tidak ada menghasilkan `ipc.endpoint_missing`, connect yang ditolak `ipc.connection_refused`, dan error OS lain `ipc.io_error` dengan `op` dan `kind`. Path yang tidak muat di `sun_path` ditolak sebelum connect. Socket selalu memakai `SO_NOSIGPIPE` dan `FD_CLOEXEC`, dan `lstat`, `connect`, serta `getpeereid` berjalan di antrian khusus, bukan di main thread atau cooperative pool.
- **AC-3**: `FrameDecoder` Swift lulus setiap vektor di `protocols/fixtures/framing/vectors.json`. Panjang 0 menghasilkan `ipc.frame_empty` dan panjang di atas batas menghasilkan `ipc.frame_too_large` begitu header terbaca, sebelum buffer payload dialokasikan; puncak isi buffer (yang dibuka untuk test) tidak pernah melebihi `max + 4 + 65 536` byte. Batas sebelum handshake 1 048 576 byte, sesudahnya `limits.maxFrameBytes` dari `HandshakeAccepted`. Payload keluar yang melebihi batas ditolak sebelum ditulis.
- **AC-4**: Frame daemon dibaca dalam tiga tahap seperti `decode_server_frame` spec 0006: cek kedalaman JSON (lebih dari `limits.maxJsonDepth`), baca `type` frame, untuk event baca `EventProbe` dulu, dan decode penuh hanya untuk jenis yang ada di `ContractNames.eventKinds`. Jenis event asing menjadi `SkippedEvent` yang tetap memajukan cursor. `type` frame asing, kedalaman berlebih, decode gagal untuk jenis yang dikenal, `requestId` yang tidak ditunggu dan tidak ada di set ID yang ditinggalkan, serta tag reply yang berbeda dengan nama method call asalnya (`ContractNames`) menjadi `ipc.protocol_violation` dan menutup koneksi.
- **AC-5**: Frame pertama setiap koneksi adalah `daemon.handshake` dengan `ContractInfo.protocolVersion`, `ContractInfo.digest`, dan `ClientInfo { kind: gui, version: CFBundleShortVersionString, pid, interactive: true }` (GUI selalu bisa menampilkan dialog, spec 0006 AC-19), dan client tidak menulis frame lain sebelum `HandshakeAccepted` diterima. Connect plus handshake dibatasi 5 detik (`ipc.call_timeout` dengan `method` `daemon.handshake` di sisi client, sama dengan CLI; `ipc.handshake_timeout` khusus daemon). `protocol.major_mismatch` membuat status koneksi `incompatible` dengan versi daemon dari `details`; error `request.*` untuk handshake juga membuat `incompatible` dengan kode itu. `contractDrift: true` tidak memutus koneksi; ia dicatat di log dan tampil di Diagnostics.
- **AC-6**: Setiap request punya `requestId` UUID huruf kecil yang unik per koneksi, balasan dicocokkan lewat `requestId` walau datang tidak berurutan, dan request berjalan dibatasi `max(1, limits.maxInFlightRequests)` (sisanya menunggu antrian). Batas waktu default per call 10 detik. Timeout atau cancel hanya menggagalkan call itu: `outcomeUnknown` (kode `ipc.call_timeout`, atau `ipc.connection_closed` kalau koneksi tutup) kalau frame nya sudah ditulis, `notSent` kalau belum; `requestId` nya masuk set ID yang ditinggalkan (paling banyak dua kali batas request berjalan, yang tertua dibuang) supaya balasan yang telat dibuang diam diam. Call saat tidak tersambung gagal langsung sebagai `notSent` dengan error koneksi terakhir atau `ipc.not_connected`, tanpa menyentuh socket. Kalau tulis gagal, client masih membaca frame tertunda paling lama 1 detik dan melaporkan frame error daemon bila ada. Frame error tanpa `requestId` sebelum handshake menjadi `rejected`; sesudah handshake, semua call tertunda menjadi `outcomeUnknown`. Setiap error sisi client memakai kode, kategori, `retryable`, dan kunci `details` yang sama dengan `protocols/error-codes.json`.
- **AC-7**: `DaemonLink` memegang satu status koneksi: `connecting`, `connected`, `unreachable`, `incompatible`, `untrusted`, atau `misconfigured`. Status hanya berubah karena error saat membuka koneksi atau handshake, kegagalan transport, timeout handshake atau heartbeat, dan `daemon.shutting_down`, menurut tabel per kode di *Status koneksi dan reconnect*; reply `rejected` dari daemon untuk call biasa tidak pernah mengubah status. Dari `unreachable`, coba lagi otomatis setelah 0,5, 1, 2, dan 4 detik, lalu tiap 5 detik selama app hidup; dari `incompatible` dan `untrusted` tiap 30 detik; dari `misconfigured` tidak pernah. Tombol Reconnect, app aktif lagi, dan Mac bangun dari sleep langsung memicu percobaan baru kalau sedang tidak tersambung, tanpa mengubah urutan jeda; jeda kembali ke 0,5 detik hanya setelah handshake berhasil. Selama tersambung, heartbeat `daemon.get_health` dikirim tiap 15 detik (tiap 1 detik selama health `starting`); heartbeat yang timeout menutup koneksi menjadi `unreachable`. GUI tidak pernah menjalankan `stacklyd` atau binary lain.
- **AC-8**: Setelah handshake, shell subscribe satu kali ke semua jenis event dengan `since` berisi cursor yang tersimpan (atau null), lalu mengikuti resep spec 0006: tanpa cursor, atau kalau event pertama adalah gap, setiap store mengambil snapshot lengkap (semua halaman); event yang datang selama snapshot ditahan lalu diterapkan setelahnya; event dengan `sequence` yang sudah diterapkan dilewati; event resource diterapkan menurut aturan resep spec 0006 langkah 4: revision lebih kecil dilewati, lebih besar diterapkan, dan revision yang sama hanya membawa observasi yang diterapkan per jenis kalau waktu observasinya sama atau lebih baru, kecuali field status instance (`instance.health_changed`, dan field status `InstanceSummary` di `instance.state_changed`, hasil `fetchOne`, serta event yang ditahan selama snapshot) yang diterapkan kalau `statusSeq` nya lebih besar dari yang dipegang, tanpa melihat revision dan tanpa mengubah revision yang dipegang, dengan batas `statusSeq` di reset saat gap `server_restarted` (spec 0006 langkah 4, spec 0010, K16); gap berikutnya (`overflow`, `cursor_expired`, `server_restarted`) memicu snapshot ulang tanpa memutus stream. Cursor adalah (`serverInstanceId` dari `HandshakeAccepted`, `sequence` terakhir yang diterima), dan event gap ikut memajukan `sequence` itu. Snapshot yang sedang berjalan saat gap baru datang dibatalkan lewat token generasi lalu diulang. Setelah daemon direstart, state di GUI sama dengan snapshot daemon baru dalam satu siklus reconnect.
- **AC-9**: Kalau daemon tidak terjangkau saat app dibuka, jendela tetap terbuka (PRD AC-024): selama percobaan pertama halaman menampilkan loading; setelah percobaan pertama gagal, footer menampilkan "stacklyd unavailable", banner jendela menampilkan "Can’t connect to stacklyd." dan "Actions are disabled until the daemon is reachable." dengan aksi Reconnect dan Open Diagnostics, halaman yang belum punya data menampilkan placeholder failed spec 0012 berjudul "Waiting for stacklyd" dengan Retry (memanggil reconnect) dan Open Diagnostics, `MutationAvailability` bernilai `.blocked`, dan tidak ada instance yang ditampilkan dengan status apa pun, apalagi Stopped. Percobaan ulang berikutnya tidak membuat banner atau footer berkedip.
- **AC-10**: Kalau daemon hilang setelah state pernah dimuat, setiap item tetap menampilkan status terakhirnya (tidak pernah diubah menjadi Stopped atau Unknown karena koneksi putus), banner menambah kalimat "Showing the last known state from {waktu}." dengan waktu frame terakhir yang diterima (heartbeat ikut dihitung), dan setiap view `.mutationGated()` nonaktif dengan alasan "stacklyd is unavailable.". Saat koneksi pulih, banner, gate, dan pengumuman ditahan sampai snapshot semua store selesai; setelah itu banner hilang, gate kembali `.available`, dan VoiceOver mengumumkan "Reconnected. State refreshed." sekali.
- **AC-11**: Status daemon selain tersambung normal dan tidak terjangkau tampil sesuai tabel *Tampilan status daemon*: `incompatible`, `untrusted`, registry dalam recovery mode (dari `RecoveryReport` di status daemon), dan `misconfigured` mendapat banner critical masing masing dan memblokir mutasi; `starting`, daemon yang sedang berhenti, dan percobaan pertama yang lebih dari 1 detik mendapat banner informative dan memblokir mutasi; `degraded` dan health asing (`unknown`) hanya terlihat di footer dan tidak memblokir mutasi.
- **AC-12**: Halaman Diagnostics selalu punya seksi Connection yang bisa dibuka walau daemon mati: status, path socket (dibungkus, tidak dipotong), kode error terakhir beserta kategori, pesan, dan alasannya, petunjuk pemulihan, waktu terakhir tersambung, versi, pid, waktu start, dan `serverInstanceId` daemon terakhir yang diketahui (diberi label "last known" saat tidak tersambung), protokol daemon dan app, status digest kontrak, serta health dan subsystem yang tidak `ok`. Seksi ini punya tombol Reconnect (nonaktif dengan alasan saat `misconfigured`), Refresh, dan Copy Details yang menyalin baris yang sama persis, dan tidak pernah menampilkan nilai `STACKLY_ROOT`.
- **AC-13**: Jendela utama adalah satu `Window` dengan `NavigationSplitView`: sidebar berisi Overview, Instances, Environments, grup Library (Catalog, Runtimes), dan grup System (Operations, Diagnostics), masing masing dengan `SectionIcon` spec 0012. Badge Instances adalah jumlah instance yang bukan `retained` dari snapshot terakhir (K16; tetap tampil saat offline, dengan label VoiceOver "last known"), dan badge Operations adalah jumlah operasi `running` dan `cancel_requested` yang hanya tampil saat tersambung; badge lain tidak tampil sampai fitur pemiliknya mengisinya, dan badge tidak pernah menampilkan angka yang tidak berasal dari snapshot. Footer sidebar menampilkan status daemon dan tombol Settings yang membuka scene Settings (⌘,). Setiap halaman menetapkan judul, subjudul, dan aksi toolbarnya sendiri. Halaman yang fiturnya belum dibangun menampilkan placeholder netral "Coming in a later build.". Pilihan sidebar diingat per jendela lewat restorasi scene sistem, default Overview.
- **AC-14**: Jendela bisa diubah ukurannya dengan ukuran minimum 900 × 600 pt dan ukuran awal 1280 × 800 pt. Kolom sidebar lebarnya minimal 200, ideal 236, dan maksimal 260 pt, sehingga kolom konten tidak pernah kurang dari 640 pt.
- **AC-15**: Menutup jendela utama tidak menghentikan proses app, koneksi ke daemon, daemon, atau instance mana pun, dan tidak mengirim apa pun ke daemon. Penutupan pertama di setiap peluncuran (bukan karena Quit) menampilkan notice opak "Stackly is still running" dengan "Closing the window doesn’t stop stacklyd or your instances. Click Stackly in the Dock to reopen it." selama 4 detik, yang juga diumumkan VoiceOver. Klik ikon Dock membuka jendela lagi. Quit (⌘Q) hanya menutup GUI, menutup socket secara sinkron, dan tidak mengirim apa pun ke daemon.
- **AC-16**: Adapter memetakan DTO lifecycle, readiness, health, dan state operasi ke `LifecycleStatus`, `ReadinessStatus`, `HealthStatus`, dan `OperationStatus` spec 0012 lewat `switch` exhaustive tanpa `default`. Readiness `not_ready` dengan reason `awaiting_probe` menjadi `.awaitingProbe`, reason lain menjadi `.notReady(detail:)`. Test mencakup setiap case DTO, dan test aturan sumber gagal kalau folder `Adapters/` memuat `default:`.
- **AC-17**: Sebelum fitur 22, halaman Instances menampilkan daftar instance sementara (`// provisional: F22`) dari `InstanceStore`: nama, provider dan versi, serta `StatusLabel` konteks `.row`, urut nama. Perubahan state instance yang dikirim lewat event tampil tanpa memuat ulang seluruh daftar. Bentuknya mengikuti `ViewState` spec 0012, dengan empty "No Instances" dan pesan "Create one with stackly instance create.", dan subjudul "1 instance" atau "{n} instances". Instance `retained` tidak pernah tampil dan tidak dihitung (K16). Daftar ini tidak punya aksi selama `newInstanceAction` nil. Bila fitur 20 mengisinya, daftar menampilkan tombol toolbar "New Instance" dan aksi kedua "New Instance…" di empty state, keduanya `.mutationGated()`, sampai fitur 22 menggantikan daftar (K17).
- **AC-18**: `OperationCenter` menyimpan operasi yang belum mencapai state akhir (`queued`, `running`, `cancel_requested`, `needs_recovery`) dari snapshot plus event area `operation.`, memberi badge Operations dan subjudul halaman Operations ("{n} running" atau "No running operations") dari jumlah operasi `running` dan `cancel_requested`, dan `updates(for:)` yang mengalirkan perubahan satu operasi sampai state akhir untuk layar yang memulainya.
- **AC-19**: Semua mutasi GUI lewat `MutationPath.perform`, yang mengirim call ke daemon dengan `idempotencyKey` baru per aksi pengguna. Saat tidak tersambung, hasilnya `notSent` tanpa menyentuh socket, dengan copy umpan balik "stacklyd is unavailable. Nothing was sent." Hasil `outcomeUnknown` menyimpan key itu supaya percobaan ulang memakai key yang sama. Task yang di cancel saat menunggu menghasilkan `outcomeUnknown` kalau frame nya sudah ditulis dan `notSent` kalau belum.
- **AC-20**: Test kontrak Swift lulus untuk enum (termasuk nilai asing di enum terbuka menjadi `unrecognized`), optional field (null dan tidak ada), timestamp (offset `+07:00`, tanpa pecahan, tiga digit), UUID huruf besar, dan error code: setiap kode di `protocols/error-codes.json` terbaca dengan kategori yang dikenal Swift, daftar kategori Swift sama persis dengan sepuluh kategori tertutup spec 0006, dan tabel kode sisi client Swift cocok dengan katalog (kategori, `retryable`, kunci `details`). Batas di fixture `HandshakeAccepted` sama dengan konstanta Swift. Vektor framing, fixture path socket, dan fixture endpoint lulus.
- **AC-21**: Shell bisa dipakai penuh dari keyboard (sidebar dengan panah, Tab ke konten, tombol banner, tombol Settings, Reconnect), setiap elemen punya label VoiceOver, perubahan status koneksi diumumkan sekali per transisi ("Can’t connect to stacklyd." dan "Reconnected. State refreshed."), banner dan teks Diagnostics (termasuk path socket) membungkus tanpa terpotong di lebar konten 640 pt, dan file shell lulus test aturan sumber spec 0012 (tanpa glass kustom, ukuran font tetap, atau warna sekunder sistem).
- **AC-22**: Semua waktu di shell (banner, Diagnostics) ditampilkan dengan zona waktu dan locale sistem lewat fungsi format murni; test dengan locale `en_US` dan locale 24 jam serta dua zona waktu tetap membuktikannya. Ini juga bagian GUI dari "ditampilkan sesuai locale pengguna" di Done when fitur 5.
- **AC-23**: Test aturan sumber (dengan batas kata) gagal kalau kode non test di `clients/swift/Sources`, `apps/macos/Packages/StacklyUI/Sources`, atau `apps/macos/Stackly` memakai `Process(`, `Process.run`, `NSTask`, `posix_spawn`, `fork(`, `execv`, atau `system(` yang bukan anggota (`ProcessInfo` dan `.system(` tetap boleh), dan kalau `clients/swift/Sources` memakai `NWConnection`, `NWListener`, `URLSession`, `CFSocket`, atau `AF_INET`.

## Decision

**Chosen option**: pilihan yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md), semuanya dikonfirmasi engineer: `StacklyUI` bergantung pada `StacklyClient`; transport POSIX socket plus `DispatchIO` tanpa dependency; reconnect dengan jeda 0,5 sampai 5 detik; app tetap hidup setelah jendela ditutup dan memberi notice; daftar instance sementara di halaman Instances; state terakhir hanya di memori; operation center berupa store plus badge; diagnostics koneksi di halaman Diagnostics plus aksi banner; placeholder netral untuk halaman yang belum dibangun; scene Settings placeholder; banner plus placeholder halaman saat belum ada state; bukti GUI lewat Swift Testing plus evidence manual; test client terhadap `stacklyd` asli plus server palsu Swift; ukuran minimum 900 × 600 pt. Semua perbaikan hasil cross check diterapkan dengan persetujuan engineer (rationale.md, *Cross check*).

Client Swift meniru kontrak spec 0006 byte per byte di atas API Darwin, satu aktor memegang setiap koneksi dan satu aktor memegang status, heartbeat, serta reconnect, sedangkan shell di main actor hanya membaca status itu, menyimpan state terakhir di store, dan mengisi gate mutasi spec 0012.

**Implementation skills**: `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`, untuk fixture path socket dan endpoint di Rust)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya: aktor tanpa `MainActor` di `StacklyClient` walau skill SwiftUI menyarankan view model di main actor, dan `confirmationDialog` hanya lewat modifier spec 0012.

## Feature design

### Batas dengan fitur lain

| Bagian | Pemilik |
| --- | --- |
| Envelope, kode dan kategori error, protokol 1.0, handshake, framing, `StacklyJSON`, resep snapshot plus cursor, tipe hasil generate | spec 0006 (dipakai apa adanya; tambahan yang diminta ada di *Asumsi lintas spec*) |
| Token, palet status, enum status UI, `StatusLabel`, `SectionIcon`, `ViewState`, `StateBanner`, `StatePlaceholder`, `MutationAvailability`, `.mutationGated()`, konfirmasi | spec 0012 (dipakai apa adanya) |
| Path socket di Swift, verifikasi endpoint, transport, decode frame, koneksi, heartbeat, reconnect, sinkronisasi, store, adapter, jalur baca dan mutasi | 13 |
| Shell jendela: scene, sidebar, badge, footer, bingkai toolbar, banner daemon, placeholder halaman, seksi Connection di Diagnostics, scene Settings placeholder, notice tutup jendela | 13 |
| Daftar instance sementara dan `InstanceStore` | 13, diganti layar fitur 22 (store tetap dipakai) |
| `OperationCenter` dan badge Operations | 13; DTO operasi milik spec 0007, halaman Operations milik fitur 35, kartu operasi Overview milik fitur 22 |
| Overview, Instances lengkap, detail instance, Environments, Catalog, Runtimes | 22, 21, 44, 17, 18 |
| Hasil doctor di Diagnostics | 34, di bawah seksi Connection |
| Tampilan laporan recovery registry dan alur restore | 48 |
| Isi Settings, menu bar, copy notice versi menu bar, activation policy | 32 |
| Umpan balik setelah aksi (merender `MutationOutcome`) | 67 |
| Peta perintah dan shortcut (termasuk perintah reconnect) | 69 |
| XCUITest, snapshot, dan audit aksesibilitas otomatis per layar | 68 |
| Menjalankan dan mendaftarkan daemon, `stackly daemon start` | 15 |
| `MainWindowRoot(runtime:windowWillClose:)` sebagai isi `Window` utama, menggantikan `LiveShell` (onboarding atau `AppShell`) | spec 0016, berlaku mulai langkah 2 milestone 1 fitur 16 (K18); teks *Shell jendela* dan *Model shell* untuk perubahan itu ditulis di commit langkah tersebut |

### Peta package dan modul

| Tempat | Dependency atau perubahan manifest | Isi baru |
| --- | --- | --- |
| `clients/swift/Package.swift` | product baru `.library(name: "StacklyTestSupport", targets: ["StacklyTestSupport"])`; `StacklyClientTests` bergantung pada `StacklyClient` dan `StacklyTestSupport` | |
| `clients/swift/Sources/StacklyClient/` | tidak ada (hanya Foundation, Dispatch, Darwin, os) | `Endpoint/` (`SocketPathResolver`, `DarwinTempDir`, `FNV1a`, `EndpointCheck`, `ConnectPolicy`), `Transport/` (`FrameDecoder`, `FrameEncoder`, `UnixSocket`, `SocketChannel`), `Wire/` (`JSONDepth`, `ServerFrameDecoder`, `DecodedFrame`, `ContractLimits`), `Connection/` (`DaemonConnection`, `CallOptions`, `ClientError`, `ClientErrorCatalog`), `Link/` (`DaemonLink`, `ConnectionState`, `ReconnectPolicy`, `Heartbeat`), `Sync/` (`SyncEngine`, `SyncSignal`), di samping `Generated/` dan `StacklyJSON` dari spec 0006 |
| `clients/swift/Sources/StacklyClient/Generated/ContractNames.swift` | ditulis `cargo xtask codegen` (kalau fitur 6 belum menulisnya) dan masuk `--check` | `ContractNames.eventKinds: Set<String>` dari `EventKind::KINDS`, `Call.methodName` dan `Reply.methodName` (nama method `<area>.<nama>` per varian) |
| `clients/swift/Tests/StacklyClientTests/` | | `Contract/`, `Transport/`, `Link/`, `Sync/`, `Integration/` (tag `.daemon`), `Support/` (`FakeDaemonServer`, `SpawnedDaemon`, `ShortTempRoot`, `ManualClock`, `RepoPaths`), `SourceRuleTests` |
| `apps/macos/Packages/StacklyUI/Package.swift` | `dependencies: [.package(path: "../../../../clients/swift")]`; target `StacklyUI` bergantung pada `.product(name: "StacklyClient", package: "swift")`; `StacklyUITests` juga pada `.product(name: "StacklyTestSupport", package: "swift")` (identitas package lokal adalah nama foldernya, `swift`) | |
| `apps/macos/Packages/StacklyUI/Sources/StacklyUI/` | | `Connection/` (`DaemonModel`, `ShellSync`, `ResourceStore`, `MutationPath`, `MutationOutcome`, `ShellBootstrap`, `ShellRuntime`), `Adapters/` (`StatusAdapter`), `Shell/` (`AppShell`, `LiveShell`, `SidebarSection`, `DaemonFooter`, `DaemonBanner`, `ShellPresentation`, `PageChrome`, `UnbuiltPage`, `CloseNotice`, `WindowOpener`, `MainWindowAccessor`), `Diagnostics/` (`ConnectionDiagnosticsSection`, `ConnectionHint`, `DiagnosticsTime`), `Instances/` (`InstanceStore`, `ProvisionalInstanceList`), `Operations/` (`OperationCenter`), `Settings/` (`SettingsPlaceholder`); `RootView.swift` dihapus |
| `apps/macos/Packages/StacklyUI/Tests/StacklyUITests/` | | test model shell, presentasi, adapter, store, format waktu, aturan sumber tambahan |
| `apps/macos/Stackly/` | | `StacklyApp.swift` ditulis ulang (scene `Window`, `Settings`, galeri Debug tetap), `AppDelegate.swift` baru |
| `protocols/fixtures/layout/` | | `socket-paths.json` |
| `protocols/fixtures/endpoint/` | | `checks.json` |
| `crates/platform/` | tidak ada | `pub fn default_runtime_root(temp: &Path) -> PathBuf` di modul `layout` (portabel), dipakai `stackly-platform-macos` `dirs` menggantikan join lokalnya; test `tests/socket_path_fixture.rs` |
| `crates/ipc/tests/` | tidak ada | `endpoint_fixture.rs` yang membaca `checks.json` terhadap evaluator murni `verify_path` dan pemetaan errno di `stackly-ipc` |

Arah dependency: target app → `StacklyUI` → `StacklyClient`. Target app hanya mengimpor `StacklyUI`, jadi `project.pbxproj` tidak perlu product package baru. `StacklyClient` tetap tanpa default isolation `MainActor`; `StacklyUI` tetap `.defaultIsolation(MainActor.self)`.

Tidak ada lagi nama yang bentrok: tipe status daemon di `StacklyClient` bernama `DaemonHealthStatus` (spec 0006), jadi `HealthStatus` di `StacklyUI` selalu berarti health instance (spec 0012).

### Model data

Fitur ini tidak menyimpan state ke disk dan tidak menambah schema. Semua state hidup di memori selama proses app berjalan; setelah app dibuka ulang, belum ada state sampai snapshot pertama berhasil. Satu satunya pengecualian adalah restorasi scene milik sistem (`@SceneStorage` untuk pilihan sidebar), yang disimpan macOS, bukan oleh kode kita.

| Tipe | Tempat | Isi |
| --- | --- | --- |
| `SocketEndpoint` | client | `runtimeRoot: String`, `socketPath: String` (untuk tampilan), `socketPathBytes: [UInt8]` (untuk `sun_path`), `rootOverride: Bool` |
| `RootOverrideProblem` | client | `.empty`, `.notAbsolute`, `.doesNotExist`, `.unreadable(errno: Int32)`, `.tempDirUnavailable`, `.runtimeRootTooLong(length: Int)` |
| `ConnectionState` | client, `Sendable` | `.connecting(attempt: Int)`, `.connected(ConnectedDaemon)`, `.unreachable(ErrorBody, since: Date, nextAttempt: Date)`, `.incompatible(IncompatibleReason, since: Date, nextAttempt: Date)`, `.untrusted(ErrorBody, since: Date, nextAttempt: Date)`, `.misconfigured(MisconfigurationReason)` |
| `IncompatibleReason` | client | `.majorMismatch(server: ProtocolVersion, error: ErrorBody)`, `.handshakeRejected(ErrorBody)` |
| `MisconfigurationReason` | client | `.rootOverride(RootOverrideProblem)`, `.invalidClientVersion` |
| `ConnectedDaemon` | client | `accepted: HandshakeAccepted`, `info: DaemonInfo?`, `health: DaemonHealth?`, `connectedAt: Date` |
| `ClientError` | client, `Error` | `.notSent(ErrorBody)`, `.rejected(ErrorBody)`, `.outcomeUnknown(ErrorBody)`; `body` memberi `ErrorBody` nya |
| `SyncSignal` | client | `.resnapshot(ResnapshotReason)` (`noCursor` atau `gap(GapReason)`), `.event(Event)`, `.lastFrameAt(Date)` (paling sering sekali per detik) |
| `DaemonPresence` | UI | `.connecting(slow: Bool)`, `.connected(health: DaemonHealthLevel)` dengan `ready`, `degraded`, atau `unrecognized`, `.refreshing`, `.starting`, `.stopping`, `.recovery`, `.unreachable(lastKnownAt: Date?)`, `.incompatible(IncompatibleReason)`, `.untrusted(reason: String, path: String)`, `.misconfigured(MisconfigurationReason)` |
| `ResourceStore<Item>` | UI, `@MainActor @Observable` | `items: [Item.ID: Item]`, `revisions: [Item.ID: UInt64]`, `tombstones: [Item.ID: UInt64]`, `loadedAt: Date?`, `error: ErrorBody?`, `isRefreshing: Bool`, `unsupported: Bool`, buffer event selama snapshot (paling banyak 4096), token generasi snapshot |
| `MutationOutcome` | UI | `.completed(Reply)`, `.rejected(ErrorBody)`, `.notSent(ErrorBody)`, `.outcomeUnknown(ErrorBody, key: String)` |
| `ShellPresentation` | UI, nilai murni | presence, `lastKnownAt`, footer (`StatusAppearance`), banner (tone, judul, pesan, aksi) atau nil, baris Diagnostics; diturunkan dari `DaemonModel`, dipakai view shell |

`DaemonModel` menyimpan di luar `ConnectionState`: `lastDaemon: ConnectedDaemon?` (daemon terakhir yang diketahui, tetap ada setelah putus), `lastError: ErrorBody?` (error koneksi terakhir), dan `lastConnectedAt: Date?`. `lastKnownAt` adalah waktu frame terakhir (response, event, atau heartbeat) yang diterima di koneksi yang snapshot pertamanya sudah selesai; null kalau belum pernah ada snapshot yang selesai. Cursor disimpan `SyncEngine` di memori dan tetap ada lintas reconnect selama app hidup.

### Path socket

`SocketPathResolver.resolve(rootOverride: String?, userTempDir: [UInt8], canonicalize: ([UInt8]) throws(RootOverrideProblem) -> [UInt8]) throws(RootOverrideProblem) -> SocketEndpoint`, fungsi murni. Pembungkus produksi `SocketPathResolver.current(rootOverride:)` memberi `DarwinTempDir.current()` (`confstr(_CS_DARWIN_USER_TEMP_DIR)`, gagal berarti `.tempDirUnavailable`) dan `realpath(3)` yang mengembalikan byte mentah.

1. `base` adalah byte temp dir dengan satu `/` di ujung dibuang.
2. Tanpa override: runtime root `base + "/stackly"` (sama dengan `default_runtime_root`).
3. Dengan override: kosong ditolak `.empty`; tidak diawali `/` ditolak `.notAbsolute`; `realpath` gagal dengan `ENOENT` atau `ENOTDIR` menjadi `.doesNotExist`, errno lain `.unreadable`. Runtime root `base + "/stackly-" + fnv8(canonical)`, sama dengan `RootSet::under_override`.
4. `fnv8` adalah 8 hex huruf kecil dari 32 bit bawah FNV-1a 64 bit (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`) atas byte mentah canonical, persis seperti Rust menghitung hash `as_encoded_bytes`. Byte tidak pernah lewat `String`, `URL`, atau `FileManager` sebelum di hash.
5. Runtime root lebih dari 76 byte ditolak `.runtimeRootTooLong`. Path socket `runtimeRoot + "/stacklyd.sock"`; `socketPath` untuk tampilan dibuat dengan `String(decoding:as: UTF8.self)`.

Hanya target app yang membaca environment: `ProcessInfo.processInfo.environment["STACKLY_ROOT"]` sekali saat launch, diteruskan ke `ShellBootstrap`. Library tidak pernah membaca environment. Validasi kepemilikan dan mode folder override tidak diulang di GUI, karena verifikasi endpoint (AC-2) sudah menolak socket yang tidak dipercaya, dan GUI tidak pernah membuat folder.

Fixture `protocols/fixtures/layout/socket-paths.json`:

```json
{
  "fnv1a64": [ { "input": "a", "hex": "af63dc4c8601ec8c" } ],
  "cases": [
    { "name": "default_root", "tempDir": "/var/folders/zz/abc/T/", "override": null,
      "runtimeRoot": "/var/folders/zz/abc/T/stackly", "socketPath": "/var/folders/zz/abc/T/stackly/stacklyd.sock" },
    { "name": "override_ascii", "tempDir": "...", "override": "/Users/dev/stackly-root", "runtimeRoot": "...", "socketPath": "..." },
    { "name": "runtime_root_too_long", "tempDir": "...", "override": null, "error": "runtime_root_too_long" }
  ]
}
```

Kasus minimal: root default, temp dir dengan dan tanpa `/` di ujung, override ASCII, override dengan spasi dan Unicode NFC, override dengan Unicode NFD (byte berbeda, hash berbeda), override di `/private/var/...`, serta runtime root 76 byte (diterima) dan 77 byte (ditolak). Nilai string fixture dibaca sebagai byte UTF-8 apa adanya di kedua bahasa. `override` di fixture sudah berupa path canonical; canonicalization dibuktikan integration test. Nilai harapan ditulis dari implementasi Rust saat fixture dibuat dan dijaga vektor FNV di atas.

### Verifikasi endpoint

Semua langkah di bawah berjalan di satu `DispatchQueue` serial milik `UnixSocket`, bukan di main thread atau cooperative pool, lalu hasilnya dikembalikan ke aktor pemanggil.

1. `socketPathBytes.count` harus lebih kecil dari ukuran `sun_path` (104); kalau tidak, `ipc.io_error` dengan `op: connect`, `kind: invalid_filename`, sebelum `lstat`.
2. `lstat(path)`: `ENOENT` menjadi `ipc.endpoint_missing` (`details.path`); errno lain `ipc.io_error` (`op: stat`).
3. `lstat(dirname(path))` untuk folder induk; errno menjadi `ipc.io_error` (`op: stat`).
4. `EndpointCheck.evaluate(path: FileStat, parent: FileStat, uid: uid_t) -> UntrustedReason?`, fungsi murni. Urutan cek tetap: symlink (`symlink`), bukan socket (`not_a_socket`), pemilik lain (`wrong_owner`), `mode & 0o077 != 0` (`group_or_other_access`), folder induk bukan folder atau milik uid lain (`parent_wrong_owner`), folder induk dengan bit group atau other (`parent_group_or_other_access`).
5. `socket(AF_UNIX, SOCK_STREAM, 0)`, `FD_CLOEXEC`, `SO_NOSIGPIPE`, lalu `connect`. `ECONNREFUSED` menjadi `ipc.connection_refused`, `ENOENT` (socket hilang di tengah) `ipc.endpoint_missing`, errno lain `ipc.io_error` (`op: connect`).
6. `getpeereid(fd)`: gagal menjadi `peer_credentials_unavailable`, uid berbeda dengan `policy.peerUID` menjadi `peer_uid_mismatch`. Di produksi `ownerUID` dan `peerUID` sama dengan `getuid()`; test memberi `peerUID` berbeda untuk menguji langkah ini.
7. Baru setelah semua lolos, fd diubah ke `O_NONBLOCK` dan diserahkan ke `SocketChannel`. Gagal di langkah mana pun menutup fd tanpa satu byte ditulis.

Fixture `protocols/fixtures/endpoint/checks.json` mengunci paritas dengan Rust: `{ "checks": [ { "name", "path": { "type", "uid", "mode" }, "parent": { "type", "uid", "mode" }, "uid", "reason": string | null } ], "errno": [ { "errno": "EACCES", "kind": "permission_denied" } ] }`. `type` bernilai `socket`, `file`, `dir`, atau `symlink`. Test Rust memanggil evaluator murni `verify_path` di `stackly-ipc` dan pemetaan `io::ErrorKind` ke snake_case dengan setiap kasus; test Swift memanggil `EndpointCheck.evaluate` dan tabel errno Swift. Nilai `kind` diambil dari hasil Rust saat fixture dibuat (nama `io::ErrorKind` dalam snake_case), minimal untuk `EACCES`, `EPERM`, `ENOENT`, `ENAMETOOLONG`, `ENOTDIR`, `EMFILE`, `ENFILE`, `ELOOP`, dan satu errno lain yang menjadi kind cadangan.

### Transport dan framing

- `FrameDecoder(maxLength:)`: `push(_ bytes:) throws(FrameError)` langsung gagal begitu header lengkap mengumumkan 0 atau di atas `maxLength`; `nextFrame() -> [UInt8]?`; `peakBufferedCount` (internal, untuk test). `FrameError.empty` menjadi `ipc.frame_empty`, `.tooLarge(length, max)` menjadi `ipc.frame_too_large`.
- `FrameEncoder.encode(_ payload:, maxLength:) throws(FrameError)` menulis panjang `UInt32` big endian lalu payload.
- `SocketChannel` adalah `final class` `@unchecked Sendable` yang seluruh state nya hanya disentuh di satu antrian serial privat (alasan `Sendable` ditulis di doc comment). Ia memakai `DispatchIO(type: .stream)` dengan `setLimit(lowWater: 1)` dan cleanup handler yang menutup fd. Satu `read` dengan panjang 65 536 dijadwalkan pada satu waktu; data dari handler dikumpulkan, dan continuation hanya di resume saat `done` bernilai true. Read berikutnya baru dijadwalkan setelah `DaemonConnection` selesai memproses potongan itu, jadi daemon yang cepat tidak bisa membanjiri memori (backpressure). Tulis diantrikan berurutan lewat `DispatchIO.write`, satu continuation per tulis yang di resume saat `done`.
- EOF di antara frame atau di tengah frame, dan error baca, menutup channel dengan `ipc.connection_closed`. `close()` bisa dipanggil sinkron dari thread mana pun (dipakai saat terminate).
- `ContractLimits` berisi konstanta Swift sebelum handshake: `controlFrameMax` 1 048 576, `maxJsonDepth` 32, `readChunk` 65 536. Test kontrak membandingkan dua yang pertama dengan nilai di fixture `HandshakeAccepted` spec 0006. Setelah handshake, decoder dan encoder memakai `limits.maxFrameBytes` dan `limits.maxJsonDepth` dari daemon.

### Decode frame daemon

`ServerFrameDecoder.decode(_ payload: [UInt8], maxDepth:) throws(ClientError) -> DecodedFrame`:

1. `JSONDepth.exceeds(payload, maxDepth)`, pemindai yang paham string dan escape. Lebih dalam berarti `ipc.protocol_violation`.
2. Decode `FrameProbe { type: String }`. Nilai selain `response`, `error`, dan `event` berarti `ipc.protocol_violation`.
3. Untuk `event`: decode `EventProbe` hasil generate. Kalau `kind.type` tidak ada di `ContractNames.eventKinds`, hasilnya `.skipped(subscriptionId, sequence, kind)`.
4. Decode penuh `ServerFrame` lewat `StacklyJSON.decoder()`. Gagal untuk jenis yang dikenal berarti `ipc.protocol_violation`.

`DecodedFrame` bernilai `.response(Response)`, `.error(ErrorFrame)`, `.event(Event)`, atau `.skipped(subscriptionId:sequence:kind:)`.

### Koneksi, handshake, dan request

`DaemonConnection` adalah `actor` yang memegang satu `SocketChannel`, peta request tertunda (`requestId` ke continuation dan status "sudah ditulis"), set ID yang ditinggalkan, antrian slot request berjalan, dan subscription. Setiap method memeriksa ulang apakah koneksi masih terbuka setelah setiap `await`, karena aktor bisa dimasuki ulang.

- `static open(endpoint:policy:client:options:) async throws(ClientError) -> (DaemonConnection, HandshakeAccepted)`: verifikasi endpoint, lalu kirim handshake dan tunggu `HandshakeAccepted` dengan batas 5 detik untuk connect plus handshake (`ipc.call_timeout` (method `daemon.handshake`)). Frame error untuk handshake dilaporkan sebagai `.rejected(body)` lalu koneksi ditutup.
- `call(_ call: Call, idempotencyKey: String?, options: CallOptions = .standard) async throws(ClientError) -> Reply`: buat `requestId` dari `UUID().uuidString.lowercased()`, tunggu slot (ukuran `max(1, limits.maxInFlightRequests)`), tulis frame, tunggu balasan dengan `requestId` itu, dan periksa `Reply.methodName` sama dengan `Call.methodName`. Frame error daemon menjadi `.rejected(body)`. `CallOptions` berisi `timeout` (default 10 detik) dan `clock` yang bisa disuntik; batas waktu termasuk antri slot.
- Timeout atau cancel task pemanggil: hanya call itu yang gagal. Kalau frame sudah ditulis, hasilnya `.outcomeUnknown(ipc.call_timeout)` (cancel memakai kode yang sama) dan `requestId` masuk set ID yang ditinggalkan; kalau belum ditulis, `.notSent(ipc.call_timeout)`. Set ID yang ditinggalkan paling banyak `2 × max(1, limits.maxInFlightRequests)` entri, yang tertua dibuang. Balasan untuk ID di set itu dibuang tanpa error.
- Tulis gagal: baca frame tertunda paling lama 1 detik. Kalau ada frame error daemon untuk call itu atau tanpa `requestId`, call gagal dengan error itu; kalau tidak, `.outcomeUnknown(ipc.connection_closed)`.
- Frame error dengan `requestId` null: sebelum handshake diterima, error itu dilaporkan `.rejected`; sesudahnya, semua call tertunda yang sudah ditulis gagal `.outcomeUnknown(body)` dan yang belum ditulis `.notSent(body)`, lalu koneksi ditutup.
- Koneksi tutup karena sebab apa pun: call tertunda yang sudah ditulis gagal `.outcomeUnknown(ipc.connection_closed)`, yang belum ditulis `.notSent(ipc.connection_closed)`.
- `subscribe(_ params: SubscribeParams) async throws(ClientError) -> (Subscribed, AsyncThrowingStream<DecodedFrame, ClientError>)`: event dan skipped untuk `subscriptionId` itu, berurutan. Stream berakhir saat koneksi tutup.
- `close()` idempoten; `termination` memberi alasan tutup ke `DaemonLink`.

`ClientErrorCatalog` membuat `ErrorBody` sisi client untuk kode `ipc.endpoint_missing`, `ipc.endpoint_untrusted`, `ipc.connection_refused`, `ipc.connection_closed`, `ipc.io_error`, `ipc.protocol_violation`, `ipc.frame_empty`, `ipc.frame_too_large`, `ipc.not_connected`, dan `ipc.call_timeout`, dengan kategori, `retryable`, dan kunci `details` yang dites sama dengan `error-codes.json`. Pesan tetapnya (huruf kecil, tanpa titik di akhir): "no socket at the daemon path", "the daemon socket failed a security check", "the daemon refused the connection", "the daemon closed the connection", "an os error stopped the connection", "the daemon sent a frame the client could not read", "the daemon sent an empty frame", "the daemon sent a frame above the size limit", "the client is not connected to the daemon", dan "the daemon did not answer in time".

Logging lewat `os.Logger(subsystem: "com.ojiepermana.stackly", category: "client")`: transisi status, kode error, nama method, dan durasi. Params, reply, isi frame, dan teks error decode tidak pernah dicatat; path socket dicatat dengan privasi default.

### Status koneksi dan reconnect

`DaemonLink` adalah `actor` dengan `states: AsyncStream<ConnectionState>` (multicast ke setiap pendengar, nilai terbaru diulang ke pendengar baru), `start()`, `retryNow()`, `stop()` (async), `stopNow()` (sinkron, `nonisolated`, hanya menutup channel), dan `call(_:idempotencyKey:options:)` yang meneruskan ke koneksi aktif atau gagal `.notSent(lastError ?? ipc.not_connected)`. Waktu `Date` diambil dari `now: @Sendable () -> Date` yang bisa disuntik, dan jeda dari `Clock` yang bisa disuntik.

| Dari | Pemicu | Ke |
| --- | --- | --- |
| awal | `start()` dengan endpoint valid dan versi client valid | `connecting(1)` |
| awal | `start()` dengan `RootOverrideProblem` atau versi bundle yang bukan 1 sampai 64 karakter ASCII tercetak | `misconfigured` (final) |
| `connecting` | `HandshakeAccepted`, lalu `daemon.get_info` dan `daemon.get_health` | `connected` |
| `connecting` | `protocol.major_mismatch` | `incompatible(.majorMismatch)` |
| `connecting` | error `request.*` untuk handshake | `incompatible(.handshakeRejected)` |
| `connecting` | `ipc.endpoint_untrusted`, `ipc.peer_rejected` | `untrusted` |
| `connecting` | `ipc.endpoint_missing`, `ipc.connection_refused`, `ipc.io_error`, `ipc.call_timeout` (method `daemon.handshake`), `ipc.connection_closed`, `ipc.too_many_connections`, `ipc.protocol_violation`, `ipc.frame_empty`, `ipc.frame_too_large`, `daemon.shutting_down`, atau kode lain | `unreachable` |
| `connected` | koneksi tertutup (EOF, error baca atau tulis, `ipc.protocol_violation`, `ipc.frame_*`, frame error tanpa `requestId`), heartbeat timeout, event `daemon.shutting_down` | `unreachable` |
| `unreachable`, `incompatible`, `untrusted` | timer `nextAttempt`, `retryNow()` | `connecting(n + 1)` |

- Reply `rejected` untuk call biasa (termasuk `request.unknown_method`, `daemon.starting`, `resource.*`, `internal.unexpected`) tidak pernah mengubah status; pemanggilnya yang menangani.
- `since` adalah waktu masalah pertama kali terjadi dan tidak berubah selama percobaan ulang yang gagal dengan status yang sama. `nextAttempt` dipakai baris Diagnostics.
- `ReconnectPolicy.standard`: jeda `[0.5, 1, 2, 4]` detik lalu 5 detik seterusnya untuk `unreachable`; 30 detik untuk `incompatible` dan `untrusted`. Indeks jeda kembali ke awal hanya setelah handshake berhasil. `retryNow()` memulai percobaan sekarang tanpa mengubah indeks; dari `connecting` ia tidak memulai percobaan kedua; dari `misconfigured` ia tidak melakukan apa pun.
- `Heartbeat`: selama `connected`, `daemon.get_health` dengan timeout 10 detik dikirim tiap 15 detik, atau tiap 1 detik selama `health.status` bernilai `starting`. Hasilnya memperbarui `ConnectedDaemon.health`; timeout menutup koneksi menjadi `unreachable` (kode `ipc.call_timeout`).
- Event `daemon.shutting_down` langsung membuat status `unreachable` (kode `daemon.shutting_down`) tanpa menunggu EOF.
- `get_info` dipanggil sekali setelah setiap handshake dan saat Refresh di Diagnostics ditekan.
- Pemicu dari sistem (`NSApplication.didBecomeActiveNotification`, `NSWorkspace.didWakeNotification`) diamati `ShellRuntime` (lihat *Model shell*) dan diteruskan ke `retryNow()`.

### Sinkronisasi snapshot plus cursor

`SyncEngine` (actor di client) berjalan per koneksi tersambung:

1. `events.subscribe { since: cursor tersimpan atau null, kinds: [], resource: null }`.
2. Tanpa cursor, atau event pertama adalah `events.gap`: kirim `.resnapshot`.
3. Cursor adalah (`serverInstanceId` dari `HandshakeAccepted` koneksi ini, `sequence` terakhir yang diterima). Event, skipped, dan gap memajukan `sequence`; gap `server_restarted` dan `cursor_expired` mengganti `sequence` dengan `sequence` gap itu. Event dengan `sequence` yang tidak lebih besar dari `sequence` terakhir untuk `serverInstanceId` yang sama dilewati.
4. Event yang lolos dikirim sebagai `.event`; skipped hanya memajukan cursor.
5. Gap berikutnya: `.resnapshot(gap(reason))`, stream tetap jalan.
6. `daemon.shutting_down`: diteruskan ke `DaemonLink` (lihat di atas).
7. `.lastFrameAt(now())` dikirim paling sering sekali per detik untuk setiap frame yang diterima.

`ShellSync` (main actor di `StacklyUI`) membaca `SyncSignal` dalam satu loop berurutan di satu `Task`, dan menyediakan `register(_ store:)` untuk store yang dipakai shell dan layar journey:

- `.resnapshot`: naikkan token generasi, lalu untuk setiap store terdaftar `beginResnapshot(generation)` (event berikutnya untuk store itu ditahan), ambil semua halaman lewat fetcher store, lalu `completeResnapshot(items, generation)`: hasil dengan generasi lama dibuang; isi store diganti hasil snapshot, tombstone yang revision nya tidak lebih besar dari item snapshot dihapus, lalu event yang ditahan diterapkan urut dengan aturan revision. Buffer yang melewati 4096 event memulai snapshot store itu dari awal, paling banyak 3 kali berturut turut dengan jeda 1, 2, dan 4 detik; setelah itu store menyimpan error dan mencoba lagi di gap atau reconnect berikutnya.
- `.event`: diarahkan ke store yang mendaftarkan awalan jenis itu (misalnya `instance.`), lalu ke `reduce` store.
- Snapshot gagal dengan `daemon.starting`: coba lagi tiap 1 detik. `request.unknown_method` (daemon lebih lama): store ditandai `unsupported`. Error lain: store menyimpan error dan data lamanya (bentuk stale di `ViewState`).
- `refresh()` per store (tombol Retry halaman saat tersambung) menjalankan langkah resnapshot yang sama hanya untuk store itu.

`ResourceStore<Item: Identifiable & Sendable>` dibuat dengan:

- `kinds: [String]` (awalan jenis event),
- `fetchPage: (PageToken?) async throws(ClientError) -> Page<Item>` (pagination berbasis token dari pemilik method list, lihat *Asumsi lintas spec*),
- `fetchOne: (Item.ID) async throws(ClientError) -> Item?`,
- `revision: (Item) -> UInt64`,
- `reduce: (Event) -> StoreEffect<Item>` dengan `StoreEffect` bernilai `.upsert(Item)`, `.patch(id, revision, (inout Item) -> Void)`, `.delete(id, revision)`, `.refetch(id, revision)`, `.observe(id, revision, apply: (inout Item) -> Bool)` *(K16, dibangun langkah 16)*, atau `.ignore`,
- `merge: (held: Item, incoming: Item) -> Item` *(K16, dibangun langkah 16, opsional; default mengambil `incoming`)*.

Aturan: `.upsert`, `.patch`, dan `.delete` hanya diterapkan kalau `revision` lebih besar dari revision yang dipegang (atau dari tombstone). Item utuh yang menggantikan item yang dipegang (`.upsert` dari event, hasil `fetchOne` saat refetch, `offer`, dan event yang ditahan lalu diterapkan setelah snapshot) melewati `merge`; snapshot sendiri mengganti item apa adanya. `.observe` adalah efek observasi tanpa revision: untuk item yang dipegang, `apply` dijalankan dan mengembalikan apakah item berubah (`false` berarti observasi itu tidak lebih baru dan diabaikan), dan `revisions[id]` serta tombstone tidak pernah berubah; untuk id yang tidak dipegang, efek ini menjadi `.refetch(id, revision)` dengan revision event dan aturan revision biasa, sehingga instance yang sudah dihapus tidak dihidupkan lagi. `.delete` menghapus item dan mencatat tombstone. `.patch` untuk id yang belum dikenal menjadi `.refetch`. `.refetch` memanggil `fetchOne` dan menerapkan hasilnya dengan aturan yang sama; `fetchOne` yang memberi nil menjadi delete pada revision itu. Tombstone dibersihkan setiap kali resnapshot store itu selesai. Aturan observasi umum per jenis dengan revision yang sama (waktu observasi, resep spec 0006 langkah 4) belum dibangun: store fase 02 pertama yang mempublikasikan observasi seperti itu yang membangunnya (K16); hari ini hanya pengecualian `statusSeq` milik `InstanceStore` yang ada.

Saat sinyal `.resnapshot(.gap(.serverRestarted))`, `ShellSync` memberi tahu setiap store terdaftar sebelum snapshot dimulai, dan store dengan batas observasi (hari ini `InstanceStore`) me reset batas itu untuk semua item yang dipegang *(K16, dibangun langkah 16)*. Karena `statusSeq` mulai lagi dari 1 di setiap run daemon (spec 0010), tanpa reset ini snapshot yang gagal setelah restart (`fail()` menyimpan data lama) membuat setiap `instance.health_changed` run baru terbuang sampai snapshot berikutnya berhasil.

### Model shell di main actor

`ShellBootstrap.live(rootOverride: String?, clientVersion: String, now:) -> ShellRuntime` adalah satu satunya pintu publik dari target app. Ia memanggil `ShellBootstrap.assemble(rootOverride:clientVersion:now:announce:closeNotice:) -> LiveShellParts` (internal, dibangun langkah 15), yang membuat `SocketEndpoint`, `DaemonLink`, `SyncEngine`, `DaemonModel`, dan `ShellRuntime`, lalu mengembalikan `LiveShellParts { runtime, link, engine }` supaya test ujung ke ujung bisa menghentikan link dan membaca cursor; `live` hanya mengembalikan `runtime`. Mulai langkah 2 milestone 1 fitur 16, `live` dan `assemble` juga menerima `onboardingStore` (teksnya ditulis commit itu, K18). Bool baca saja `DaemonModel.rootOverrideActive` diisi `assemble` sekali dari `rootOverride != nil`, tanpa parameter baru di `live` atau `assemble`; ia ditambahkan oleh yang lebih dulu memakainya di antara langkah 10 milestone 3 fitur 15 dan langkah 2 milestone 1 fitur 16 (K18). `StacklyUI` tidak pernah membaca `ProcessInfo.environment`, dan nilai override tidak pernah ditampilkan. `ShellRuntime` memegang `model: DaemonModel`, `windowOpener: WindowOpener`, `closeNotice: CloseNotice`, dan hook lifecycle: `didBecomeActive()`, `didWake()`, `willTerminate()` (sinkron, memanggil `DaemonLink.stopNow()`), serta `mainWindowWillClose(isTerminating:)`. `ShellRuntime` sendiri yang mendaftar ke notifikasi app aktif dan wake.

`DaemonModel` (`@MainActor @Observable`) memegang `DaemonLink`, `SyncEngine`, `ShellSync`, `InstanceStore`, dan `OperationCenter`, dan memberi:

- `presence: DaemonPresence`, fungsi murni dari (status stabil terakhir, ada percobaan yang sedang berjalan, semua store sudah selesai resnapshot sejak tersambung, `health.status`, `RecoveryReport`). Percobaan yang sedang berjalan setelah kegagalan tidak mengubah presence (tetap `unreachable`, `incompatible`, atau `untrusted`), jadi banner dan footer tidak berkedip; tombol Reconnect menampilkan progress kecil selama percobaan. Percobaan pertama setelah launch adalah `connecting(slow:)`, dengan `slow` true setelah 1 detik. Setelah handshake, presence `refreshing` sampai semua store selesai resnapshot, lalu baru `connected`, `starting`, `stopping`, atau `recovery` menurut health.
- `lastKnownAt: Date?` dari `SyncSignal.lastFrameAt` setelah snapshot selesai.
- `mutationAvailability: MutationAvailability` sesuai tabel di bawah, dipasang di environment root shell lewat setter environment publik dari spec 0012.
- `lastDaemon`, `lastError`, `lastConnectedAt` untuk Diagnostics.
- `reconnect()`, `refreshDiagnostics()`, `diagnostics: ConnectionDiagnostics`, `copyDiagnostics() -> String`.
- `read(_ call: Call, options:) async -> Result<Reply, ClientError>`: jalur baca untuk layar yang butuh call tanpa efek di luar store (misalnya detail satu item); tidak memeriksa gate.
- Pengumuman VoiceOver lewat `announce(_:)` spec 0012 sekali per transisi presence: ke `unreachable` dari `connected` atau dari percobaan pertama ("Can’t connect to stacklyd."), dari `refreshing` ke `connected` setelah pernah `unreachable` ("Reconnected. State refreshed."), Reconnect manual yang gagal ("Still unreachable.").

### Tampilan status daemon

Banner memakai `StateBanner(tone:title:message:actions:base: .window)` spec 0012, satu untuk seluruh jendela, di atas konten kolom detail di bawah toolbar. Semua aksi banner adalah `StateAction(mutates: false)`, jadi gate tidak menonaktifkannya. Footer memakai `StatusLabel` konteks `.row` dengan `StatusAppearance(label:tone:symbol:detail:)`, dengan `symbol` dari `Tone.symbol` tone footer itu.

| Presence | Footer (tone, teks) | Banner (tone, judul, pesan) | Aksi banner | `MutationAvailability` |
| --- | --- | --- | --- | --- |
| `connected(ready)` | positive, "stacklyd running" | tidak ada | | `.available` |
| `connected(degraded)` | caution, "stacklyd degraded" | tidak ada (detail di Diagnostics) | | `.available` |
| `connected(unrecognized)` (nilai health asing) | caution, "stacklyd status unknown" | tidak ada | | `.available` |
| `refreshing` | caution, "Refreshing…" | tidak ada; banner sebelumnya tetap tampil sampai selesai | | sama dengan sebelumnya (`.blocked` setelah putus, atau "Loading the latest state." setelah launch) |
| `starting` | caution, "stacklyd starting…" | informative, "stacklyd is starting.", "Actions are available once it’s ready." | | `.blocked("stacklyd is starting.", since: waktu handshake)` |
| `stopping` | caution, "stacklyd stopping…" | informative, "stacklyd is shutting down.", "Stackly reconnects when it’s back." | Open Diagnostics | `.blocked("stacklyd is shutting down.", since:)` |
| `connecting(slow: false)` | caution, "Connecting…" | tidak ada | | `.blocked("Connecting to stacklyd…", since: waktu launch)` |
| `connecting(slow: true)` | caution, "Connecting…" | informative, "Connecting to stacklyd…", "Actions are available once it’s connected." | Open Diagnostics | `.blocked("Connecting to stacklyd…", since: waktu launch)` |
| `unreachable` dengan `lastKnownAt` | critical, "stacklyd unavailable" | caution, "Can’t connect to stacklyd.", "Showing the last known state from {waktu}. Actions are disabled until the daemon is reachable." | Reconnect, Open Diagnostics | `.blocked("stacklyd is unavailable.", since:)` |
| `unreachable` tanpa `lastKnownAt` | critical, "stacklyd unavailable" | caution, "Can’t connect to stacklyd.", "Actions are disabled until the daemon is reachable." | Reconnect, Open Diagnostics | `.blocked("stacklyd is unavailable.", since:)` |
| `incompatible(.majorMismatch)` | critical, "stacklyd incompatible" | critical, "This version of Stackly can’t talk to stacklyd.", "stacklyd speaks protocol {M.m}; this app speaks {M.m}. Install matching versions of Stackly and stacklyd." | Reconnect, Open Diagnostics | `.blocked("This version of Stackly can’t talk to stacklyd.", since:)` |
| `incompatible(.handshakeRejected)` | critical, "stacklyd incompatible" | critical, "This version of Stackly can’t talk to stacklyd.", "stacklyd rejected the handshake ({code}). Install matching versions of Stackly and stacklyd." | Reconnect, Open Diagnostics | sama dengan baris di atas |
| `untrusted` | critical, "stacklyd untrusted" | critical, "Stackly didn’t connect to stacklyd.", "The socket failed a security check: {alasan}. Nothing was sent." | Reconnect, Open Diagnostics | `.blocked("stacklyd’s socket failed a security check.", since:)` |
| `recovery` | critical, "stacklyd in recovery" | critical, "Stackly’s registry needs recovery.", "stacklyd started in recovery mode and didn’t change your data. Instances can’t be managed until the registry is repaired." | Open Diagnostics | `.blocked("Stackly’s registry needs recovery.", since:)` |
| `misconfigured` | critical, "Stackly misconfigured" | critical, judul dan pesan per alasan di bawah | Open Diagnostics | `.blocked(judul banner, since: waktu launch)` |

- `{waktu}` dari `StaleNote.formattedSince(now:calendar:locale:timeZone:)` spec 0012 dengan nilai sistem.
- Teks `{alasan}` untuk `untrusted` dari `ConnectionHint`: `not_a_socket` "the path isn’t a socket", `symlink` "the path is a symbolic link", `wrong_owner` "the socket belongs to another user", `group_or_other_access` "other users can access the socket", `parent_wrong_owner` "the socket’s folder isn’t a folder owned by you", `parent_group_or_other_access` "other users can access the socket’s folder", `peer_uid_mismatch` "the daemon runs as another user", `peer_credentials_unavailable` "macOS didn’t report who runs the daemon"; `ipc.peer_rejected` (tanpa `details.reason`) "stacklyd rejected this app’s user"; alasan lain "unexpected reason {reason}".
- `misconfigured`: `.rootOverride(.empty)` judul "STACKLY_ROOT is invalid.", pesan "STACKLY_ROOT is set but empty. Fix the variable and relaunch Stackly."; `.notAbsolute` "STACKLY_ROOT must be an absolute path. Fix the variable and relaunch Stackly."; `.doesNotExist` "The folder in STACKLY_ROOT doesn’t exist. Fix the variable and relaunch Stackly."; `.unreadable` "Stackly couldn’t read the folder in STACKLY_ROOT. Fix the variable and relaunch Stackly."; `.runtimeRootTooLong` "The socket path for STACKLY_ROOT would be too long. Use a shorter folder and relaunch Stackly."; `.tempDirUnavailable` judul "Stackly can’t find its socket folder.", pesan "macOS didn’t report a temporary folder for your user. Restart your Mac and try again."; `.invalidClientVersion` judul "This build of Stackly is invalid.", pesan "Its version number can’t be sent to stacklyd. Install a release build." Nilai variabel tidak pernah ditampilkan.
- Halaman yang belum punya data saat presence bukan `connected`, `refreshing`, atau `starting`, dan bukan percobaan pertama, menampilkan `.failed` lewat `ViewState.resolve` dengan `FailureContent(title: "Waiting for stacklyd", message: "Stackly shows this page once the daemon is reachable.", code: kode lastError)`. Tombol Retry milik placeholder spec 0012 memanggil `reconnect()` saat tidak tersambung dan `refresh()` store saat tersambung; Open Diagnostics membuka halaman Diagnostics. Error presence hanya diteruskan ke `ViewState.resolve` saat store belum punya data, supaya halaman yang punya data tidak menambah banner stale di bawah banner jendela.
- Status operasi atau instance tidak pernah diturunkan dari presence: item yang sudah ada tetap memakai status terakhirnya.

### Adapter DTO ke enum UI

`StatusAdapter` di `StacklyUI/Adapters/`, `nonisolated` dan murni:

- `lifecycle(_:) -> LifecycleStatus`, `readiness(_:) -> ReadinessStatus`, `health(_:) -> HealthStatus`, `operation(_:) -> OperationStatus`, masing masing satu `switch` exhaustive atas enum DTO hasil generate tanpa `default` atau `@unknown default`, sehingga case baru di protokol gagal compile.
- Readiness: `ready` menjadi `.ready`; `not_ready` dengan reason `awaiting_probe` menjadi `.awaitingProbe`; `not_ready` dengan reason lain menjadi `.notReady(detail: dto.detail)`; `unknown` menjadi `.unknown`.
- Lifecycle, readiness, health, dan state operasi wajib enum tertutup di protokol, karena `unknown` di tiga yang pertama adalah nilai domain, bukan sentinel enum terbuka (lihat *Asumsi lintas spec*).
- Status tampil dibuat lewat `StatusAppearance.instance(lifecycle:readiness:health:)` dan `.operation(state:partial:)` spec 0012.

### Shell jendela

Scene di `StacklyApp.swift`:

```swift
Window("Stackly", id: WindowOpener.mainWindowID) {
    LiveShell(runtime: appDelegate.runtime,
              windowWillClose: { appDelegate.mainWindowWillClose($0) })
}
    .defaultSize(width: 1280, height: 800)
    .windowResizability(.contentMinSize)
Settings { SettingsPlaceholder() }   // provisional: F32
#if DEBUG
    Window("Design Gallery", id: "design-gallery") { DesignGallery() }
        .defaultSize(width: 1100, height: 780)
#endif
```

- `LiveShell(runtime:windowWillClose:)` (dibangun langkah 15) adalah isi jendela utama di app yang berjalan: ia membuat `AppShell(presentation:diagnostics:counts:windowOpener:windowWillClose:reconnect:refreshDiagnostics:)` dari `runtime.model` (`presentation`, `diagnostics`, `sidebarCounts`, `reconnect()`, `refreshDiagnostics()`) dan memasang `DaemonModel` di environment untuk halaman fitur berikutnya. Runtime milik `AppDelegate`, bukan jendela, jadi menutup jendela tidak menghentikan apa pun. Mulai langkah 2 milestone 1 fitur 16, `MainWindowRoot` spec 0016 menggantikan `LiveShell` sebagai isi `Window` (K18, *Batas dengan fitur lain*).
- `AppShell` memasang `.frame(minWidth: 900, minHeight: 600)` (`AppShell.minimumSize`). Sidebar `.navigationSplitViewColumnWidth(min: 200, ideal: 236, max: 260)`, detail `min: 640`.
- `AppShell` menangkap `@Environment(\.openWindow)` ke `WindowOpener` yang diterimanya dari `ShellRuntime` lewat `LiveShell`, dan `MainWindowAccessor` (`NSViewRepresentable` kecil) memberi `NSWindow` jendela utama, yang `willCloseNotification` nya diteruskan ke `windowWillClose` (`AppDelegate.mainWindowWillClose`) untuk notice. Jendela utama dikenali lewat accessor ini, bukan lewat identifier buatan SwiftUI.
- `AppDelegate` (`NSApplicationDelegateAdaptor`): `applicationShouldTerminateAfterLastWindowClosed` false; `applicationShouldHandleReopen(_:hasVisibleWindows:)` memanggil `WindowOpener.openMain()` kalau tidak ada jendela yang terlihat; `applicationShouldTerminate` menandai sedang keluar sebelum jendela ditutup, supaya notice tidak muncul saat Quit; `applicationWillTerminate` memanggil `ShellRuntime.willTerminate()` yang menutup socket secara sinkron. Langkah pertama milestone 3 memeriksa perilaku ini di macOS 27 dan mencatat hasilnya di `apps/macos/design.md`.
- `SidebarSection` (`overview`, `instances`, `environments`, `catalog`, `runtimes`, `operations`, `diagnostics`) dengan judul, `SectionStyle` spec 0012, dan grup (tanpa judul, "Library", "System"). Sidebar memakai `List(selection:)` gaya sidebar; setiap baris `Label` dengan `SectionIcon` dan `.badge(_:)` native. Pilihan disimpan `@SceneStorage("shell.section")`, default `.overview`.
- Badge: Instances `InstanceStore.count` kalau store sudah pernah memuat snapshot, dengan `accessibilityValue` "{n} instances" saat tersambung dan "{n} instances, last known" saat tidak; Operations `OperationCenter.runningCount` dengan `.badgeProminence(.increased)`, hanya saat presence `connected`; lainnya nil. Badge 0 tidak tampil.
- Footer: `DaemonFooter` di `.safeAreaInset(edge: .bottom)` sidebar, berisi `StatusLabel` dan `SettingsLink` dengan simbol `slider.horizontal.3`, `.help("Settings (⌘,)")`, label VoiceOver "Settings".
- Bingkai halaman: modifier `.pageChrome(title:subtitle:)` memasang `.navigationTitle` dan `.navigationSubtitle`; aksi toolbar ditambah halaman dengan `.toolbar`. Aksi yang mengubah data wajib `.mutationGated()`.
- `UnbuiltPage(section:)`: `StatePlaceholder` ukuran `.page` dengan simbol bagian, judul bagian, dan "Coming in a later build.", tanpa aksi dan tanpa subjudul. Overview (`// provisional: F22`), Environments (`F44`), Catalog (`F17`), Runtimes (`F18`), Operations (`F35`, dengan subjudul dari `OperationCenter`), Diagnostics (`F34`, di bawah seksi Connection).
- Banner `DaemonBanner` tampil di atas konten setiap halaman lewat satu `safeAreaInset(edge: .top)` di kolom detail.
- `SettingsPlaceholder`: satu tab General berisi "Coming in a later build." (`// provisional: F32`).
- `CloseNotice`: saat `ShellRuntime.mainWindowWillClose(isTerminating: false)` pertama kali di peluncuran itu, tampilkan `NSPanel` tanpa border dan tanpa aktivasi, di tengah bawah layar tempat jendela tadi berada, berisi view SwiftUI opak (`.cardSurface(radius: .large)`, tanpa glass) dengan judul "Stackly is still running" dan isi "Closing the window doesn’t stop stacklyd or your instances. Click Stackly in the Dock to reopen it." (`// provisional: F32`). Panel hilang setelah 4 detik atau saat diklik, dengan crossfade spec 0012, dan teksnya diumumkan lewat `announce(_:)`.
- Placeholder `RootView` spec 0001 dihapus. `GalleryLayout.narrowWidth` tetap 640 sebagai lebar konten minimum dari spec ini; penanda `// provisional: F13` dan teks galeri "the minimum window width" diganti "the minimum content width" setelah milestone 2 sampai 4 fitur 12 masuk main.

### Diagnostics koneksi

`ConnectionDiagnosticsSection` di halaman Diagnostics, kartu `.cardSurface()` dengan baris `LabeledContent`. Copy Details menyalin baris yang sama persis, satu baris teks per baris tabel (`Label: nilai`).

| Baris | Isi |
| --- | --- |
| Status | teks footer presence |
| Socket | `socketPath`, `.stacklyCode()`, bisa dipilih, membungkus di batas karakter |
| Last error | kode (monospace), kategori, dan pesan apa adanya dari `lastError`, ditambah `details.reason` bila ada; "None" kalau tidak ada |
| What to do | `ConnectionHint` untuk kode itu, lalu `ErrorBody.hint` daemon bila ada |
| Next attempt | waktu `nextAttempt` saat menunggu percobaan ulang |
| Last connected | `lastConnectedAt`, atau "Never" |
| stacklyd | versi, pid, waktu start, dan `serverInstanceId` dari `lastDaemon.info`, dengan akhiran "(last known)" saat tidak tersambung |
| Protocol | "stacklyd {M.m} · Stackly {M.m}" (versi daemon dari handshake terakhir atau dari `details` mismatch) |
| Contract | "Matches" atau "Different build (contract drift)" |
| Health | `DaemonHealth.status` dan subsystem yang tidak `ok` beserta `detail`, dari heartbeat terakhir |

`ConnectionHint`: `ipc.endpoint_missing` "stacklyd isn’t running. Start it with stacklyd, then click Reconnect." (`// provisional: F15`); `ipc.connection_refused` "A stale socket is left from a stopped stacklyd. Start stacklyd, then click Reconnect."; `ipc.io_error` "Stackly couldn’t reach the socket ({op}: {kind})."; `ipc.endpoint_untrusted` dan `ipc.peer_rejected` teks alasan di atas ditambah "Stackly sent nothing to this socket."; `protocol.major_mismatch` dan handshake yang ditolak "Install matching versions of Stackly and stacklyd."; `ipc.call_timeout` dengan method `daemon.handshake` "stacklyd didn’t answer within 5 seconds."; `ipc.call_timeout` "stacklyd stopped answering."; `ipc.connection_closed` "stacklyd closed the connection."; `daemon.shutting_down` "stacklyd is shutting down."; `ipc.protocol_violation` "stacklyd sent a message Stackly couldn’t read. Check that both come from the same build."; recovery "See the recovery report in stacklyd’s log folder."; `misconfigured` pesan banner nya; kode lain tanpa petunjuk.

Waktu di Diagnostics memakai `DiagnosticsTime.format(_ date: Date, now:, calendar:, locale:, timeZone:)`, fungsi murni yang memberi tanggal singkat dan jam dengan detik (`Date.FormatStyle(date: .abbreviated, time: .standard)`), dipanggil dengan nilai sistem.

Aksi: Reconnect (nonaktif dengan `.disabled(reason: "Relaunch Stackly after fixing the problem.")` saat `misconfigured`, menampilkan progress kecil selama percobaan), Refresh (saat tersambung, memanggil `get_info` dan `get_health`), dan Copy Details. Seksi ini tidak memanggil method lain dan tidak membuka file apa pun.

### Daftar instance sementara (fitur 14)

`InstanceStore` adalah `ResourceStore<InstanceSummary>` dengan snapshot dari `instance.list` dan event area `instance.` (spec 0010). Aturannya *(K16, dibangun langkah 16)*:

- `fetchPage` memanggil `instance.list { limit: 200, before }` dan membuang summary dengan lifecycle `retained`; `fetchOne` memanggil `instance.get` dan memberi nil untuk summary `retained`, sehingga refetch (dan retry fitur 22) memperlakukannya sebagai delete dengan tombstone pada revision event.
- `reduce`: `instance.state_changed` dengan lifecycle `retained` menjadi `.delete(id, revision: event.revision)`; selain itu `.upsert(summary)`. `instance.health_changed` menjadi `.observe(id, revision: event.revision)` yang menerapkan `readiness`, `health`, `blocker`, `restart`, `process`, `logsUnavailable`, dan `statusSeq` hanya kalau `statusSeq` event lebih besar dari yang dipegang. `instance.removed` (DTO dari langkah 12 spec 0010, K4; dikirim mulai fitur 20) menjadi `.delete(id, revision: event.revision)`, dengan revision event sama dengan revision terakhir ditambah 1.
- `merge`: semua field diambil dari summary yang masuk, kecuali field status (`readiness`, `health`, `blocker`, `restart`, `process`, `logsUnavailable`, `statusSeq`), yang diambil dari summary dengan `statusSeq` lebih besar (yang dipegang menang kalau sama). Jadi `instance.state_changed` menerapkan field non status menurut revision dan field status menurut `statusSeq`, dan `instance.health_changed` tidak pernah menaikkan revision yang dipegang, sehingga `instance.state_changed` pada revision itu tidak pernah terbuang.
- Reset batas `statusSeq` saat gap `server_restarted` mengikuti *Sinkronisasi snapshot plus cursor*.
- `count` adalah jumlah item di store, yang tidak pernah memuat instance `retained`; badge Instances dan subjudul memakai nilai ini.

`ProvisionalInstanceList` (`// provisional: F22`) di halaman Instances:

- `ViewState.resolve` dengan `value` dari store, `error` dari store, atau dari presence hanya saat store belum punya data (judul "Waiting for stacklyd"), atau "Couldn’t Load Instances" untuk error store lain; `lastLoadedAt` dari store.
- Empty: simbol `server.rack`, judul "No Instances", pesan "Create one with stackly instance create.". Tanpa aksi selama `newInstanceAction` nil (K17).
- Loaded: `List` dengan baris nama (`.body.weight(.semibold)`), "{provider} {versi}" (`.callout`, `textSecondary`), dan `StatusLabel` konteks `.row` di kanan; urut nama dengan `localizedStandardCompare`; tanpa seleksi.
- Aksi *(K17)*: langkah 16 membangun daftar ini tanpa aksi dan tidak mendefinisikan `NewInstanceAction`. Langkah 14 milestone 3 spec 0020 mendefinisikan kunci environment `newInstanceAction: NewInstanceAction?` kalau belum ada konsumen yang lebih dulu, lalu menambah tombol toolbar "New Instance" dan aksi kedua "New Instance…" di empty state, keduanya `.mutationGated()`, hanya saat nilainya tidak nil, bertanda `// provisional: F22`. Kalau milestone 3 fitur 22 di merge lebih dulu, fitur 20 hanya mengisi halaman fitur 22 dan tidak menyentuh daftar ini.
- Subjudul toolbar "1 instance" atau "{n} instances" (inflection otomatis Foundation) setelah snapshot termuat.

### Operation center

`OperationCenter` adalah `ResourceStore` untuk DTO operasi spec 0007 (`OperationSnapshot` adalah alias tipe DTO itu), dengan snapshot dari method list operasi yang difilter ke state tidak akhir dan event area `operation.`:

- `active` berisi operasi `queued`, `running`, `cancel_requested`, dan `needs_recovery`.
- `runningCount` adalah jumlah `running` dan `cancel_requested`; ia memberi badge dan subjudul halaman Operations ("{n} running" atau "No running operations", copy mockup).
- Operasi yang mencapai state akhir keluar dari `active` dan disimpan di `recent` selama 5 menit, supaya `updates(for:)` masih bisa memberi state akhir ke layar yang terlambat mendengarkan.
- `updates(for id:) -> AsyncStream<OperationSnapshot>` memberi nilai saat ini lalu setiap perubahan, dan berakhir setelah state akhir.

### Jalur mutasi

`MutationPath` (`@MainActor`, dari `DaemonModel`):

```swift
public func perform(_ call: Call, idempotencyKey: String = MutationPath.newKey()) async -> MutationOutcome
```

- Tidak tersambung: `.notSent(lastError ?? ipc.not_connected)` tanpa memanggil daemon.
- Tersambung: `DaemonLink.call(call, idempotencyKey:)`; reply menjadi `.completed`, `ClientError` dipetakan satu satu ke `.rejected`, `.notSent`, atau `.outcomeUnknown(body, key:)`. Daemon tetap memvalidasi setiap mutasi, termasuk saat `starting`.
- Task yang di cancel: hasilnya mengikuti aturan cancel di *Koneksi, handshake, dan request* (`outcomeUnknown` kalau sudah ditulis, `notSent` kalau belum).
- `newKey()` adalah `UUID().uuidString.lowercased()` (36 karakter ASCII tercetak, lolos format spec 0006). Spec 0007 menentukan method mana yang mewajibkannya; `MutationPath` selalu mengirimnya.
- Copy umpan balik (dirender fitur 67): `MutationFeedback.notSent` "stacklyd is unavailable. Nothing was sent." dan `MutationFeedback.outcomeUnknown` "Lost the connection to stacklyd. The action may have run; check Operations before trying again."

### Aksesibilitas

- Sidebar `List` native: panah memindah pilihan, Tab masuk ke konten; VoiceOver membaca judul bagian dan badge (dengan "last known" saat offline); ikon tersembunyi (spec 0012).
- Footer: VoiceOver membaca "Status: stacklyd running" (aturan `StatusLabel`), tombol Settings berlabel "Settings".
- Banner: satu kelompok VoiceOver dengan judul sebagai label (spec 0012); tombol Reconnect dan Open Diagnostics bisa difokus.
- Pengumuman sekali per transisi seperti di *Model shell*; notice tutup jendela diumumkan.
- Lebar konten 640 pt: judul dan isi banner serta semua nilai Diagnostics, termasuk path socket, membungkus tanpa dipotong.
- File baru di `Shell/`, `Diagnostics/`, `Instances/`, `Operations/`, dan `Settings/` tunduk pada test aturan sumber spec 0012.

### Asumsi lintas spec

- **Assumed (spec 0006)**: `ClientKind` punya nilai `gui`; Swift sudah punya `StacklyJSON`, `EventProbe`, `ContractInfo`, `OpenEnums.swift`, dan test fixture dari `index.json`; vektor framing dan fixture `HandshakeAccepted` ada di `protocols/fixtures/`; kode `ipc.*` di katalog dipakai juga untuk error sisi client Swift dengan arti yang sama; katalog `ipc` mendapat dua kode baru (minor naik): `ipc.not_connected` (`unavailable`, retryable, tanpa `details`: client tidak tersambung dan tidak mengirim apa pun) dan `ipc.call_timeout` (`timeout`, tidak retryable, `details` `method`: client berhenti menunggu balasan); `cargo xtask codegen` menulis `ContractNames.swift` (daftar jenis event dan nama method per varian `Call` dan `Reply`); `stackly-ipc` membuka evaluator murni untuk aturan `verify_path` dan pemetaan `io::ErrorKind` ke snake_case yang bisa dites dengan fixture `protocols/fixtures/endpoint/checks.json`, dengan urutan cek seperti di *Verifikasi endpoint*.
- **Assumed (spec 0007, ditulis paralel)**: ada method list operasi berhalaman dengan token halaman yang bisa difilter ke state tidak akhir, method get operasi, dan event area `operation.` yang membawa `resource` dan `revision`; DTO operasi membawa id, judul atau jenis, state dengan tujuh nilai sebagai enum tertutup, penanda `partial`, progress (tahap, selesai, total), dan `startedAt`; reply method yang memulai operasi membawa `operationId`.
- **Assumed (spec 0010 atau build fitur 14, mana yang lebih dulu menambah method instance)**: method list instance berhalaman dengan token halaman yang mengembalikan ringkasan instance (id, nama, nama tampilan provider, versi exact, lifecycle, readiness dengan state, reason, dan detail, health, revision) dan method get instance; event `instance.state_changed` dan `instance.health_changed` lewat `publish_resource` membawa nilai lifecycle, readiness, dan health yang baru; instance baru dan instance yang dihapus terlihat lewat event di area `instance.` (event untuk id yang belum dikenal memicu pengambilan satu item); enum DTO lifecycle, readiness, dan health tertutup.
- **Spec 0005 dan 0006** (diselaraskan 2026-10-18): `DaemonHealth.recovery: Option<RegistryRecovery { reason, reportPath, detectedAt, appSchemaVersion, dbSchemaVersion }>` terisi hanya dalam recovery mode, sehingga heartbeat tahu tanpa memanggil method registry; isi laporannya ditampilkan fitur 48.
- **Assumed (spec 0012)**: `StateBanner(tone:title:message:actions:base:)` dengan tone informative, `StatePlaceholder` dengan isi bebas, `MutationAvailability` dengan setter environment publik yang bisa dipasang dari luar paket (nama key tidak ditulis spec 0012), `StatusAppearance(label:tone:symbol:detail:)` publik, `Tone.symbol`, `SectionStyle`, `StaleNote.formattedSince`, `StateAction(mutates:)`, `.disabled(reason:)`, dan `announce(_:)` tersedia seperti tertulis.
- **Assumed (spec 0002)**: test Swift yang membuktikan ID PRD memakai trait `.evidence` dari `StacklyTestSupport`; binary `stacklyd` yang tidak ada membuat test integrasi `BLOCKED` lewat `Evidence.blocked`; evidence manual dicatat lewat `cargo xtask evidence new`; menambah product `StacklyTestSupport` tidak mengubah aturan bahwa hanya test yang memakainya.

### API surface

API Swift yang dipakai fitur lain. Semua method daemon yang dipanggil fitur ini milik spec 0006 (`daemon.handshake`, `daemon.get_info`, `daemon.get_health`, `events.subscribe`) atau diasumsikan dari spec 0007 dan 0010 (list dan get instance dan operasi). Tidak ada method mutasi baru.

| API | Bentuk | Input utama | Output | Error utama |
| --- | --- | --- | --- | --- |
| `SocketPathResolver.resolve` / `.current` | fungsi statis | override, byte temp dir, canonicalize | `SocketEndpoint` | `RootOverrideProblem` |
| `EndpointCheck.evaluate` | fungsi murni | stat path, stat induk, uid | `UntrustedReason?` | |
| `UnixSocket.connectVerified` | fungsi async | `SocketEndpoint`, `ConnectPolicy` | fd terverifikasi | `ipc.endpoint_missing`, `ipc.endpoint_untrusted`, `ipc.connection_refused`, `ipc.io_error` |
| `DaemonConnection.open` | async statis | endpoint, policy, `ClientInfo`, options | koneksi, `HandshakeAccepted` | di atas, `protocol.major_mismatch`, `ipc.call_timeout` (method `daemon.handshake`) |
| `DaemonConnection.call` | async | `Call`, `idempotencyKey?`, `CallOptions` | `Reply` | `ClientError` |
| `DaemonConnection.subscribe` | async | `SubscribeParams` | `Subscribed`, stream `DecodedFrame` | `events.too_many_subscriptions` |
| `DaemonLink` | actor | endpoint atau masalah konfigurasi, `ClientInfo`, `ReconnectPolicy`, clock, `now` | `states`, `call`, `retryNow`, `stop`, `stopNow` | `ClientError.notSent` saat tidak tersambung |
| `SyncEngine` | actor | `DaemonLink` | stream `SyncSignal` | |
| `ShellBootstrap.live` | fungsi | override, versi app, `now` | `ShellRuntime` | |
| `ShellRuntime` | main actor | | `model`, `windowOpener`, hook lifecycle | |
| `DaemonModel` | `@Observable` main actor | | `presence`, `lastKnownAt`, `mutationAvailability`, `diagnostics`, `reconnect()`, `read(_:)` | |
| `ShellSync.register` | main actor | `ResourceStore` | | |
| `ResourceStore` | `@Observable` main actor | `kinds`, `fetchPage`, `fetchOne`, `revision`, `reduce`, `merge` (K16) | `items`, `loadedAt`, `error`, `unsupported`, `refresh()` | |
| `InstanceStore`, `OperationCenter` | store | | `count`, `active`, `runningCount`, `updates(for:)` | |
| `MutationPath.perform` | async main actor | `Call`, key | `MutationOutcome` | |
| `StatusAdapter` | fungsi murni | enum DTO | enum UI spec 0012 | |
| `.pageChrome(title:subtitle:)` | modifier | judul, subjudul | judul dan subjudul toolbar | |
| `UnbuiltPage`, `AppShell`, `LiveShell`, `DaemonBanner`, `DaemonFooter`, `ConnectionDiagnosticsSection`, `CloseNotice` | view | `ShellPresentation` atau section | | |

### Value sourcing

| Aksi atau view | Nilai | Sumber |
| --- | --- | --- |
| path socket | temp dir user | `confstr(_CS_DARWIN_USER_TEMP_DIR)` lewat `DarwinTempDir`, sebagai byte |
| path socket | override | `STACKLY_ROOT` dari environment proses, dibaca target app sekali saat launch |
| path socket | path canonical override | byte mentah `realpath(3)` |
| path socket | `fnv8`, batas 76 byte, nama `stacklyd.sock` | algoritma dan konstanta spec 0004, dijaga fixture bersama |
| verifikasi endpoint | uid harapan | `getuid()` |
| verifikasi endpoint | jenis, pemilik, mode path dan folder induk | `lstat` |
| verifikasi endpoint | uid server | `getpeereid` |
| verifikasi endpoint | `details.reason`, urutan cek, `details.kind` | daftar alasan spec 0006 dan fixture `protocols/fixtures/endpoint/checks.json` |
| error sisi client | kode, kategori, `retryable`, kunci `details`, pesan | `ClientErrorCatalog`, dites terhadap `protocols/error-codes.json`; pesan tetap di spec ini |
| handshake | `protocol`, `contractDigest` | `ContractInfo.protocolVersion`, `ContractInfo.digest` (hasil codegen spec 0006) |
| handshake | `client.kind` | konstanta `gui` |
| handshake | `client.version` | `CFBundleShortVersionString` bundle app (`MARKETING_VERSION`), diteruskan target app |
| handshake | `client.pid` | `ProcessInfo.processInfo.processIdentifier` |
| decode | jenis event yang dikenal, nama method call dan reply | `ContractNames` hasil codegen |
| request | `requestId` | `UUID().uuidString.lowercased()` per request |
| mutasi | `idempotencyKey` | `MutationPath.newKey()` per aksi pengguna, dipakai ulang saat percobaan ulang |
| batas | frame, kedalaman, request berjalan | `ContractLimits` sebelum handshake, `HandshakeAccepted.limits` sesudahnya (request berjalan minimal 1) |
| reconnect | jeda | `ReconnectPolicy.standard` di spec ini |
| heartbeat | interval dan timeout | 15 detik (1 detik saat `starting`), timeout 10 detik, di spec ini |
| footer, banner | presence | `ConnectionState`, percobaan yang berjalan, status resnapshot store, `DaemonHealth.status` dari heartbeat, `DaemonHealth.recovery` (spec 0006) |
| banner | waktu state terakhir | `lastKnownAt`: `now()` saat frame terakhir diterima setelah snapshot selesai |
| banner | teks waktu | `StaleNote.formattedSince` spec 0012 dengan kalender, locale, dan zona waktu sistem |
| banner | versi protokol daemon untuk `incompatible` | `details.serverMajor` dan `details.serverMinor` dari `protocol.major_mismatch` |
| banner, Diagnostics | alasan untrusted dan petunjuk | `details.reason`, kode error, dan `ErrorBody.hint`, dipetakan `ConnectionHint` |
| `MutationAvailability` | `.blocked(reason, since)` | tabel *Tampilan status daemon*; `since` dari `ConnectionState` atau waktu launch |
| Diagnostics | versi, pid, `startedAt`, `serverInstanceId`, `socketPath` daemon | `lastDaemon.info` dari `daemon.get_info` |
| Diagnostics | health dan subsystem | `DaemonHealth` dari heartbeat terakhir |
| Diagnostics | status digest kontrak | `HandshakeAccepted.contractDrift` |
| Diagnostics | waktu terakhir tersambung, percobaan berikutnya | `lastConnectedAt`, `nextAttempt` |
| Diagnostics | teks waktu | `DiagnosticsTime.format` dengan nilai sistem |
| sidebar | judul, simbol, warna bagian | `SidebarSection` dan `SectionStyle` spec 0012 |
| sidebar | badge Instances | `InstanceStore.count` dari snapshot terakhir, tanpa instance `retained` (K16) |
| sidebar | badge Operations, subjudul Operations | `OperationCenter.runningCount`, badge hanya saat `connected` |
| daftar instance | nama, provider, versi, lifecycle, readiness, health | DTO ringkasan instance (Assumed spec 0010 atau fitur 14), dipetakan `StatusAdapter` lalu `StatusAppearance.instance` |
| daftar instance | urutan, teks jumlah | nama dengan `localizedStandardCompare`; inflection Foundation untuk "instance" |
| operation center | state, partial, progress | DTO operasi (Assumed spec 0007) |
| store | revision per resource, token halaman | field `revision` di DTO snapshot dan event (aturan revision spec 0006); token halaman dari method list pemilik |
| cursor | `serverInstanceId`, `sequence` | `HandshakeAccepted.serverInstanceId`, lalu `sequence` event, skipped, dan gap yang diterima |
| ukuran jendela | minimum, awal, lebar kolom | konstanta di spec ini (900 × 600, 1280 × 800, sidebar 200, 236, 260, konten 640) |
| notice tutup jendela | teks, durasi 4 detik, sekali per peluncuran, bukan saat Quit | spec ini; copy dari mockup dengan kalimat Dock sampai fitur 32; jendela dari `MainWindowAccessor` |
| Copy Details | isi | baris seksi Connection yang sedang tampil |

### Key invariants

- Tidak ada satu byte pun ditulis ke socket yang gagal pemeriksaan path atau peer, dan tidak ada frame selain handshake sebelum `HandshakeAccepted`.
- GUI tidak pernah menjalankan `stacklyd` atau binary lain, dan `StacklyClient` tidak pernah membuka socket jaringan.
- Status instance atau operasi di GUI hanya berasal dari DTO daemon. Koneksi yang putus tidak pernah mengubah status item; ia hanya menambah banner dan memblokir mutasi.
- Reply `rejected` untuk call biasa tidak pernah mengubah status koneksi.
- `MutationAvailability` bernilai `.available` hanya saat presence `connected` (health `ready`, `degraded`, atau nilai asing) tanpa recovery; dalam keadaan lain `.blocked`, dan `MutationPath` tidak menyentuh socket saat tidak tersambung.
- Store tidak pernah menerapkan event yang `sequence` nya sudah diterapkan, yang `revision` nya lebih kecil dari yang dipegang, atau observasi yang lebih lama dari yang dipegang (per jenis, atau per `statusSeq` untuk status instance); snapshot lama tidak pernah menimpa event yang lebih baru, dan hasil snapshot dengan generasi lama selalu dibuang. Observasi `statusSeq` tidak pernah mengubah revision atau tombstone yang dipegang (K16).
- Instance `retained` tidak pernah ada di `InstanceStore`, dari jalur mana pun (`fetchPage`, `fetchOne`, `reduce`, `offer`, refetch), sehingga daftar, badge, dan jumlah tidak pernah menghitungnya (K16).
- Badge dan jumlah hanya berasal dari snapshot yang sudah termuat; tidak pernah ada angka nol palsu, dan badge Operations tidak tampil saat tidak tersambung.
- `StacklyClient` tidak memakai `MainActor`; IO socket tidak pernah berjalan di main thread atau cooperative pool.
- Shell dan client tidak menulis state atau cache ke disk; restorasi scene milik sistem (`@SceneStorage`) adalah satu satunya yang disimpan, oleh macOS.
- Menutup jendela atau keluar dari app tidak pernah mengirim apa pun ke daemon.

### Security model

- Batas kepercayaan satu akun user OS (BR-012), sama dengan spec 0006. GUI tidak mengklaim melindungi dari proses jahat dengan uid yang sama.
- Lapisan client: `lstat` path dan folder induk, lalu `getpeereid`, sebelum menulis apa pun (AC-070 lewat spec 0006), dengan aturan yang sama persis dengan Rust lewat fixture bersama. Credential dan mutasi hanya dikirim setelah handshake diterima.
- `SO_NOSIGPIPE` mencegah daemon yang menutup socket mematikan app lewat SIGPIPE; `FD_CLOEXEC` mencegah fd bocor ke proses lain.
- Tidak ada TCP, tidak ada IPC jarak jauh, dan tidak ada spawn proses dari GUI (AC-23 spec ini, dan PRD AC-024 "tanpa menjalankan binary langsung").
- Log GUI tidak memuat params, reply, isi frame, atau nilai secret. Pesan error yang tampil sudah diredaksi daemon (spec 0011).
- Nilai `STACKLY_ROOT` tidak pernah ditampilkan. Copy Details berisi baris Diagnostics yang sama persis (status, path socket, error, petunjuk, waktu, versi, pid, `serverInstanceId`, protokol, digest, health), tanpa secret.
- App Sandbox tetap mati (spec 0001), supaya temp dir user dan socket daemon sama dengan milik daemon.
- Daemon tetap memvalidasi setiap mutasi walau tombol sudah nonaktif (PRD bagian 11.2).

### Configuration required

- `STACKLY_ROOT` (sudah ada, spec 0004): kini juga dibaca target app sekali saat launch, dengan aturan kosong ditolak. Untuk development dan test saja, misalnya lewat environment scheme Xcode atau `open --env` dari terminal. Nilainya tidak pernah ditampilkan.
- `MARKETING_VERSION` target app (sudah 0.1.0) menjadi `client.version` handshake dan wajib 1 sampai 64 karakter ASCII tercetak; nilai lain membuat app `misconfigured`.
- `STACKLY_DAEMON_BIN` (baru, hanya untuk test): path absolut binary `stacklyd` untuk integration test Swift. Tanpa nilai, test memakai `target/debug/stacklyd` relatif terhadap akar repo dari `#filePath`; kalau binary tidak ada, test `BLOCKED` dengan petunjuk `cargo build -p stacklyd`.

### Critical test scenarios

- Path socket: semua kasus fixture di Rust dan Swift, vektor FNV, override kosong, relatif, dan tidak ada, lalu integration test yang membandingkan path Swift dengan `DaemonInfo.socketPath` daemon asli di root sementara. Memverifikasi **AC-1**, **AC-20**.
- Endpoint: semua kasus `checks.json` di Rust dan Swift; di filesystem asli socket 0666, socket di folder 0755, symlink ke socket asli, file biasa, path kosong, socket basi, dan path di atas 103 byte; `peerUID` berbeda terhadap daemon asli dengan server penghitung byte yang mencatat nol byte. Memverifikasi **AC-2**.
- Framing dan decode: semua vektor, frame 1 MiB persis dan 1 MiB plus 1 dengan `peakBufferedCount` di bawah batas, frame terpotong per byte, JSON 33 tingkat, `type` asing, event asing menjadi skipped dengan cursor maju, tag reply salah, `requestId` tak dikenal, balasan telat untuk ID yang ditinggalkan dibuang tanpa menutup koneksi. Memverifikasi **AC-3**, **AC-4**.
- Handshake dan request lewat server palsu: major 2 menjadi `incompatible`, handshake dengan versi client salah ditolak `request.invalid_params` menjadi `incompatible(.handshakeRejected)`, digest berbeda tetap tersambung dengan drift, frame kedua tidak pernah ditulis sebelum handshake diterima, balasan tidak berurutan, request ke 33 menunggu slot, timeout 10 detik dengan jam tiruan hanya menggagalkan call itu sebagai `outcomeUnknown(ipc.call_timeout)` sementara call lain tetap berhasil, cancel task sebelum tulis memberi `notSent`, frame error tanpa `requestId` setelah handshake memberi `outcomeUnknown`, tulis gagal setelah daemon mengirim `daemon.shutting_down` memberi error daemon itu. Memverifikasi **AC-5**, **AC-6**, **AC-19**.
- Reconnect dengan `ManualClock` dan `now` tiruan: urutan jeda 0,5, 1, 2, 4, 5, 5 detik, `retryNow` tidak mengubah indeks, jeda 30 detik untuk `incompatible` dan `untrusted`, reset setelah handshake, `misconfigured` tidak pernah mencoba, `rejected` untuk `request.unknown_method` tidak mengubah status, heartbeat timeout menjadi `unreachable`, heartbeat tiap 1 detik selama `starting`. Memverifikasi **AC-7**.
- Sinkronisasi dengan server palsu: event ganda dilewati, event dengan revision lama setelah snapshot dilewati, event selama snapshot ditahan lalu diterapkan, gap `overflow` di tengah snapshot membuang hasil generasi lama dan mengulang, gap `server_restarted` memajukan cursor ke `sequence` gap, hapus tidak dihidupkan snapshot lama, buffer penuh diulang paling banyak 3 kali. Dengan daemon asli: kill lalu start lagi memberi `server_restarted` dan state yang sama dengan snapshot baru. Memverifikasi **AC-8**.
- Model shell tanpa daemon (unit test `DaemonModel` dengan link tiruan dan dengan path socket kosong sungguhan): percobaan pertama loading, lalu `unreachable` dengan banner tanpa kalimat waktu, halaman `failed` "Waiting for stacklyd", gate `.blocked`, tidak ada item dengan status, percobaan ulang tidak mengubah presence. Lalu dengan state termuat dan koneksi putus: item tetap, banner dengan waktu, pengumuman sekali; saat pulih, banner dan gate baru berubah setelah semua store selesai resnapshot. Memverifikasi **AC-9**, **AC-10** (PRD AC-024 lewat trait `.evidence`).
- Presentasi: satu test per baris tabel *Tampilan status daemon* untuk footer, banner, aksi, dan gate, termasuk teks setiap alasan `misconfigured` dan `untrusted`. Memverifikasi **AC-11**.
- Diagnostics: setiap kode di `ConnectionHint`, baris "last known" setelah putus, Reconnect nonaktif saat `misconfigured`, Copy Details sama persis dengan baris dan tidak memuat nilai override. Memverifikasi **AC-12**.
- Shell: struktur sidebar, badge nil dan nol tidak tampil, badge Operations hilang saat offline, label "last known" badge Instances, placeholder per bagian, `SceneStorage` default. Memverifikasi **AC-13**.
- Adapter: setiap case DTO, readiness dengan reason `awaiting_probe` dan reason lain, test aturan sumber tanpa `default:`. Memverifikasi **AC-16**.
- Store instance dan operasi: event perubahan state memperbarui satu item tanpa snapshot ulang; event untuk id baru memicu `fetchOne`; operasi selesai keluar dari `active` dan `updates(for:)` berakhir; `needs_recovery` tidak dihitung badge. Memverifikasi **AC-17**, **AC-18**.
- Kontrak: fixture `error-codes.json`, tabel `ClientErrorCatalog`, kategori tertutup, batas `HandshakeAccepted`. Memverifikasi **AC-20**.
- Format waktu: `en_US` dan locale 24 jam, dua zona waktu, hari ini dan kemarin, untuk banner dan Diagnostics. Memverifikasi **AC-22**.
- Aturan sumber: spawn proses dan API jaringan ditolak, `ProcessInfo` dan `.system(` lolos. Memverifikasi **AC-23**.
- Auth atau permission: tidak ada peran pengguna di GUI; akses dibatasi lewat uid socket (skenario endpoint di atas).

### Cek manual di /check verify

Dicatat sebagai evidence manual (spec 0002), di app Debug dan `stacklyd` asli dengan `STACKLY_ROOT` ke folder sementara:

1. Buka app tanpa daemon: jendela terbuka, footer "stacklyd unavailable", banner tanpa kalimat waktu, Instances "Waiting for stacklyd", tidak ada instance berstatus apa pun, banner tidak berkedip selama beberapa percobaan (**AC-9**, PRD AC-024).
2. Jalankan `stacklyd`: dalam 5 detik banner hilang tanpa disentuh, footer "stacklyd running" (**AC-7**).
3. Setelah fitur 14: dengan instance fake berjalan, hentikan daemon (SIGTERM). Instance tetap "Running", banner menyebut waktu, tombol mutasi nonaktif dengan alasan. Start daemon lagi: state disegarkan dan VoiceOver mengumumkannya (**AC-8**, **AC-10**, **AC-17**).
4. Tutup jendela: notice tampil sekali, `stackly daemon status` dan `ps` menunjukkan daemon dan instance tetap hidup; klik Dock membuka jendela; tutup lagi tanpa notice; ⌘Q menutup app saja tanpa notice (**AC-15**).
5. Perkecil jendela: berhenti di 900 × 600, sidebar 200 sampai 260 pt, banner dan Diagnostics (termasuk path socket) membungkus di lebar konten 640 pt (**AC-14**, **AC-21**).
6. Keyboard dan VoiceOver: sidebar, banner, footer, Settings, Diagnostics (**AC-21**).
7. Ganti socket dengan socket 0666 atau symlink: banner untrusted, log daemon tidak mencatat koneksi dari GUI (**AC-2**, **AC-11**).
8. `STACKLY_ROOT` kosong di scheme: banner misconfigured, tidak ada percobaan connect, Reconnect nonaktif dengan alasan (**AC-11**, **AC-12**).

## Build plan

Pendekatan proyek adalah Journey. Jalur yang dibangun lengkap di fitur ini adalah "buka Stackly, lihat status daemon, kehilangan daemon tanpa kehilangan state, tersambung lagi lewat snapshot plus cursor, lalu tutup jendela tanpa menghentikan apa pun", termasuk semua jalur gagal. Client dibangun lebih dulu karena setiap bagian shell bergantung padanya, tetapi kerangka UI shell tidak butuh client dan bisa berjalan bersamaan. Test yang membuktikan ID PRD memakai trait `.evidence` spec 0002.

Prasyarat dari fitur lain:

- Milestone 1 butuh milestone 1 fitur 6 di main (tipe hasil generate, `StacklyJSON`, fixture `ipc/`, vektor framing). Langkah integration test butuh milestone 4 fitur 6 (daemon server). Langkah 4 butuh dua kode `ipc` baru dan `ContractNames.swift`; kalau fitur 6 belum menambahkannya, langkah 4 menambahkannya di `stackly-protocol` dan xtask codegen dengan kenaikan minor protokol, lalu Follow-up spec 0006 mencatatnya. Langkah 3 butuh evaluator murni `verify_path` di `stackly-ipc` (milestone 2 fitur 6); kalau belum terbuka, langkah 3 membukanya.
- Milestone 3 butuh milestone 2 sampai 4 fitur 12 di main (`StatusLabel`, `SectionIcon`, `StateBanner`, `StatePlaceholder`, `ViewState`, `MutationAvailability`).
- Milestone 4 butuh DTO operasi dari build fitur 7 dan DTO instance dari build fitur 10 atau 14 di main.

Milestone yang bisa berjalan sebagai sesi paralel: **Milestone 1** dan **Milestone 3** bisa berjalan bersamaan setelah prasyarat masing masing ada, karena folder mereka terpisah (milestone 1: `clients/swift`, `protocols/fixtures/layout`, `protocols/fixtures/endpoint`, `crates/platform`, `crates/ipc/tests`; milestone 3: folder `Shell/`, `Diagnostics/`, `Settings/` di `StacklyUI` dan target app, tanpa menyentuh `Package.swift`). **Milestone 2** butuh milestone 1, dan boleh berjalan bersamaan dengan milestone 3 selama milestone 2 tidak menyentuh `Shell/` atau `Diagnostics/` dan milestone 3 tidak menyentuh `Package.swift` atau `Connection/`. **Milestone 4** berjalan terakhir.

**Milestone 1: Transport dan kontrak Swift** (`clients/swift`, `protocols/fixtures/layout/`, `protocols/fixtures/endpoint/`, `crates/platform`, `crates/platform-macos/src/dirs.rs`, `crates/ipc/tests/`)

1. `default_runtime_root` di `stackly-platform` (dipakai `stackly-platform-macos`), fixture `socket-paths.json` beserta test Rust `crates/platform/tests/socket_path_fixture.rs`, lalu `SocketPathResolver`, `DarwinTempDir`, `FNV1a` atas byte, dan canonicalization `realpath` di Swift dengan test fixture dan test override yang ditolak, satisfies **AC-1**, **AC-20**.
2. `FrameDecoder` (dengan `peakBufferedCount`), `FrameEncoder`, dan `JSONDepth` dengan test semua vektor framing dan kasus batas, satisfies **AC-3**, **AC-20**.
3. Fixture `endpoint/checks.json` dengan test Rust `crates/ipc/tests/endpoint_fixture.rs`, lalu `EndpointCheck.evaluate`, `UnixSocket.connectVerified` di antrian khusus (cek `sun_path`, `lstat`, `SO_NOSIGPIPE`, `FD_CLOEXEC`, connect, `getpeereid`, tabel errno), dan `SocketChannel` di atas `DispatchIO` dengan backpressure, satisfies **AC-2**, **AC-3**, **AC-20**.
4. `ContractNames.swift` dan dua kode `ipc` baru bila belum ada, `ServerFrameDecoder` tiga tahap dengan `DecodedFrame`, `ContractLimits`, `ClientErrorCatalog`, dan test kontrak `error-codes.json`, tabel kode client, kategori tertutup, dan batas `HandshakeAccepted` (fixture `index.json` sudah dites milestone 1 fitur 6; test ini memastikan semuanya lulus), satisfies **AC-4**, **AC-6**, **AC-20**.
5. `DaemonConnection` (handshake, `call` dengan peta tertunda, slot, timeout dan cancel per call, set ID yang ditinggalkan, pembacaan frame tertunda saat tulis gagal, error tanpa `requestId`, `subscribe`, `close`) dan `ClientError`, satisfies **AC-5**, **AC-6**.
6. Product `StacklyTestSupport`, support test (`FakeDaemonServer` dengan penghitung byte dan frame berskrip, `SpawnedDaemon` dengan root pendek sementara dan pembersihan proses, `ManualClock`, `RepoPaths`), test transport dengan server palsu, integration test terhadap `stacklyd` asli (tag `.daemon`, `BLOCKED` tanpa binary), dan `SourceRuleTests` untuk spawn proses dan API jaringan, satisfies **AC-1**, **AC-2**, **AC-5**, **AC-6**, **AC-23**.

**Milestone 2: Koneksi dan sinkronisasi** (`clients/swift/Sources/StacklyClient/Link`, `Sync`, `StacklyUI/Package.swift`, `StacklyUI/Sources/StacklyUI/Connection`)

7. `DaemonLink`, `ConnectionState`, `ReconnectPolicy`, `Heartbeat`, tabel transisi per kode, `misconfigured` (override dan versi client), multicast `states`, `stopNow`, dan test dengan `ManualClock`, `now` tiruan, dan server palsu, satisfies **AC-7**, **AC-11**.
8. `SyncEngine` (subscribe dengan cursor, aturan `sequence` termasuk gap, skipped, `daemon.shutting_down`, `lastFrameAt` yang digabung) dengan test server palsu, satisfies **AC-8**.
9. `StacklyUI` bergantung pada `StacklyClient`; `ResourceStore` (reducer, buffer selama snapshot, token generasi, batas ulang, aturan revision, tombstone, `fetchPage`, `fetchOne`) dan `ShellSync` dengan `register`, dengan test store generik memakai DTO tiruan, satisfies **AC-8**.
10. `DaemonModel` (presence sebagai fungsi murni, `refreshing`, `lastKnownAt`, `lastDaemon`, `lastError`, tabel `MutationAvailability`, pengumuman, `reconnect`, `read`), `ShellRuntime` dan `ShellBootstrap`, `ShellPresentation`, serta `MutationPath` dan `MutationOutcome` beserta copy, satisfies **AC-9**, **AC-10**, **AC-11**, **AC-19**.

**Milestone 3: UI shell** (`StacklyUI/Sources/StacklyUI/Shell`, `Diagnostics`, `Settings`, `apps/macos/Stackly`, `apps/macos/design.md`; boleh paralel dengan milestone 1 dan 2)

11. Periksa di macOS 27 pembukaan ulang scene `Window` dari Dock lewat `openWindow` yang ditangkap, perilaku jendela terakhir yang ditutup, penanda Quit sebelum jendela ditutup, dan ukuran minimum lewat `.windowResizability(.contentMinSize)`, catat di `design.md`; lalu tulis ulang `StacklyApp.swift` (scene `main`, `Settings` placeholder, galeri Debug tetap), `AppDelegate`, `WindowOpener`, `MainWindowAccessor`, dan hapus `RootView`, satisfies **AC-13**, **AC-14**, **AC-15**.
12. `AppShell`, `SidebarSection`, sidebar dengan `SectionIcon` dan badge opsional (aturan offline dan label "last known"), `DaemonFooter` dengan `SettingsLink`, `.pageChrome`, `UnbuiltPage` per bagian, dan `SceneStorage`, dengan preview light, dark, dan lebar 640 pt dari `ShellPresentation` contoh, satisfies **AC-13**, **AC-14**, **AC-21**.
13. `DaemonBanner` per baris tabel presentasi, `ConnectionDiagnosticsSection` dengan `ConnectionHint`, baris "last known", Reconnect yang bisa nonaktif, dan Copy Details, serta `DiagnosticsTime` dan pemanggilan `formattedSince` dengan test locale dan zona waktu, satisfies **AC-9**, **AC-10**, **AC-11**, **AC-12**, **AC-21**, **AC-22**.
14. `CloseNotice` (panel opak, sekali per peluncuran, bukan saat Quit, 4 detik, pengumuman) dan penggantian penanda 640 pt di galeri setelah milestone 2 sampai 4 fitur 12 masuk main, satisfies **AC-15**, **AC-21**.

**Milestone 4: Penyambungan, status, instance, operasi, dan bukti ujung ke ujung** (setelah milestone 1 sampai 3, DTO operasi fitur 7, dan DTO instance fitur 10 atau 14)

15. Penyambungan target app: baca `STACKLY_ROOT` sekali, `ShellBootstrap.live` dengan versi bundle, environment `DaemonModel` dan `MutationAvailability`, pemetaan ke `ShellPresentation`, hook `AppDelegate` ke `ShellRuntime`, satisfies **AC-7**, **AC-9**, **AC-10**, **AC-11**, **AC-15**.
16. `StatusAdapter` dengan test per case DTO dan test aturan sumber tanpa `default:`, lalu `InstanceStore` dan `ProvisionalInstanceList` (`// provisional: F22`) dengan badge dan subjudul Instances, satisfies **AC-13**, **AC-16**, **AC-17**.
17. `OperationCenter` dengan `runningCount`, badge dan subjudul Operations, serta `updates(for:)`, satisfies **AC-13**, **AC-18**.
18. Test ujung ke ujung tingkat model terhadap `stacklyd` asli (app tanpa daemon, daemon dimatikan lalu dinyalakan, gap `server_restarted`, event state instance setelah method fitur 14 ada) dan evidence manual cek di /check verify, satisfies **AC-8**, **AC-9**, **AC-10**, **AC-15**, **AC-17**, **AC-21**.

**Tambahan langkah 16 dari keputusan K16 dan K17** *(2026-10-20)*. Langkah 16 menunggu langkah 12 spec 0010 (`InstanceSummary` dengan `statusSeq`, kedua event instance, DTO `instance.removed`, dan fixture resep `statusSeq` di `protocols/fixtures/`), yang menurut rencana fitur 10 datang setelah milestone 2 fitur 10. Langkah 12 spec 0010 berhenti di decode dan test kontrak Swift dan tidak menyentuh `ResourceStore.swift`; langkah 16 yang menjalankan resepnya. Selain isi langkah di atas, langkah 16 membangun:

- `StoreEffect.observe`, `merge`, dan pemberitahuan `server_restarted` dari `ShellSync` di `ResourceStore` (*Sinkronisasi snapshot plus cursor*), lalu aturan `InstanceStore` di *Daftar instance sementara*: `limit` 200, penyaringan `retained` di `fetchPage`, `fetchOne`, dan `reduce`, efek observasi `statusSeq`, `merge` field status, reset batas, dan pemetaan `instance.removed`. `SidebarCounts.instances` diisi dari `InstanceStore.count` setelah snapshot pertama.
- Test: fixture resep `statusSeq` AC-16 spec 0010 dijalankan lewat `InstanceStore` (bagian Swift dari AC-16 spec 0010); test regresi bahwa `instance.health_changed` tidak menaikkan revision, sehingga `instance.state_changed` pada revision itu tetap diterapkan; reset batas setelah gap `server_restarted` dengan snapshot yang gagal; `offer` dan refetch tidak pernah menahan item `retained`; `instance.state_changed` `retained` dan `instance.removed` menghapus baris dengan tombstone; badge dan subjudul tidak menghitung `retained`.
- `ProvisionalInstanceList` tanpa aksi dan tanpa `NewInstanceAction` (K17).

Milestone 3 fitur 22 (`itemFailures` dan `retry`) dan langkah 2 milestone 1 fitur 16 mengubah `ResourceStore.swift`, `AppShell.swift`, `LiveShell.swift`, `StacklyApp.swift`, atau `ShellRuntime.swift`, jadi semuanya berjalan setelah langkah 16 di merge, tidak bersamaan dengannya (K16 SEQ 6, K18 SEQ 4).

## Consequences

**Positive**:

- Setiap layar journey mendapat client, store yang sudah benar terhadap gap dan revision, adapter status, jalur baca dan mutasi, gate mutasi, dan bingkai halaman, jadi layar hanya mengurus isinya sendiri.
- Daemon yang mati, restart, macet, atau berbeda versi tidak pernah membuat GUI kosong atau berbohong: state terakhir tetap ada dengan waktunya, heartbeat menangkap daemon yang macet, dan alasan serta langkah berikutnya selalu terlihat (PRD AC-024, RISK-09).
- Kontrak Swift dan Rust dijaga fixture yang sama untuk framing, path socket, verifikasi endpoint, error code, dan DTO, jadi drift ketahuan di test, bukan di tangan pengguna.
- Tanpa dependency pihak ketiga, tanpa jaringan, dan tanpa spawn proses, permukaan review keamanan GUI kecil dan dijaga test aturan sumber.
- Timeout atau cancel di satu layar tidak memutus koneksi untuk layar lain.
- Fitur 14 bisa membuktikan perubahan state lewat event di GUI tanpa menunggu fitur 22.
- Milestone 1 dan 3 bisa dibangun paralel.

**Negative / tradeoffs**:

- Transport, verifikasi endpoint, dan decode frame ditulis dua kali (Rust dan Swift). Setiap perubahan aturan di spec 0006 harus diterapkan di kedua sisi; fixture bersama hanya menangkap yang bisa diuji lewat data.
- Fitur 13 meminta tambahan kecil di kontrak spec 0006 (dua kode `ipc`, `ContractNames.swift`, evaluator `verify_path` yang terbuka), jadi protokol naik minor dan sesi fitur 6 atau langkah 3 dan 4 di sini harus mengerjakannya.
- API POSIX dan `DispatchIO` butuh kode pointer, errno, dan siklus hidup continuation yang teliti, dan test dengan socket asli butuh path pendek di temp dir.
- Heartbeat tiap 15 detik adalah lalu lintas tetap kecil selama app hidup, juga saat jendela tertutup.
- State terakhir hilang saat app dibuka ulang tanpa daemon; pengguna hanya melihat banner dan placeholder sampai daemon kembali.
- App tetap hidup setelah jendela ditutup, jadi pengguna yang ingin menutupnya harus ⌘Q. Sampai fitur 32, satu satunya jalan kembali adalah Dock.
- Notice opak tutup jendela dan accessor jendela adalah kode AppKit kecil buatan sendiri, karena SwiftUI tidak punya presentasi setelah jendela tertutup.
- Badge memakai gaya native, bukan kapsul biru dan angka merah di mockup; mockup perlu dicatat untuk diperbarui.
- Daftar instance sementara dan placeholder halaman adalah kode yang sengaja dibuang fitur 17, 18, 22, 34, 35, dan 44.
- Milestone 3 menunggu milestone 2 sampai 4 fitur 12, dan milestone 4 menunggu DTO dari fitur 7 dan fitur 10 atau 14, jadi fitur 13 baru bisa `done` setelah semuanya ada di main.
- Bukti perilaku GUI sampai fitur 68 masih manual, jadi regresi tampilan shell baru tertangkap saat `/check verify`.

**Neutral**:

- `StacklyUI` kini bergantung pada `StacklyClient`, dan `swift test` untuk `StacklyUI` ikut membangun client.
- `StacklyTestSupport` menjadi product package `clients/swift`, tetap hanya dipakai test.
- Placeholder `RootView` spec 0001 hilang, diganti `AppShell`; `WindowGroup` diganti `Window`.
- `GalleryLayout.narrowWidth` 640 menjadi nilai final (lebar konten minimum).
- `default_runtime_root` pindah ke `stackly-platform` sebagai fungsi murni.
- `STACKLY_DAEMON_BIN` adalah variabel baru yang hanya dibaca test.

## Follow-up

- [x] `/architect` update spec 0001 (lewat sesi rekonsiliasi): tandai baris "Transport IPC di Swift dan shell aplikasi" di *Batas keputusan* sudah diputuskan di spec 0013; catat arah dependency target app → `StacklyUI` → `StacklyClient`, product `StacklyTestSupport`, dan bahwa `StacklyClient` tanpa dependency pihak ketiga dan tanpa `MainActor`; ganti kalimat `RootView` di satu `WindowGroup` dengan scene `Window` utama, `Settings`, dan galeri Debug; catat folder fixture baru `protocols/fixtures/layout/` dan `protocols/fixtures/endpoint/` serta output codegen `ContractNames.swift` di *Alur kontrak*; catat larangan spawn proses dan API jaringan di GUI yang dijaga test aturan sumber. Diterapkan di spec pemiliknya (2026-10-18).
- [x] `/architect` update spec 0004 (lewat sesi rekonsiliasi): centang Follow-up fitur 13 setelah langkah 1 dibangun, dengan fixture `protocols/fixtures/layout/socket-paths.json` dan test `crates/platform/tests/socket_path_fixture.rs`; tambah `default_runtime_root` ke *API surface* `stackly-platform`; catat bahwa target app juga membaca `STACKLY_ROOT` sekali saat launch dengan aturan kosong ditolak, tanpa menampilkannya. Diterapkan di spec pemiliknya (2026-10-18).
- [x] `/architect` update spec 0002: kalimat "Fitur pertama yang menambah XCUITest (fitur 13 atau 68)" diganti menjadi fitur 68, karena fitur 13 memakai Swift Testing plus evidence manual; catat product `StacklyTestSupport` yang kini dipakai test `StacklyUI`, `STACKLY_DAEMON_BIN`, dan pola `BLOCKED` untuk integration test Swift yang butuh `stacklyd`. Diterapkan di spec pemiliknya (2026-10-18).
- [x] `/architect` update spec 0006 (pemilik fitur 6): tambah `ipc.not_connected` (`unavailable`, retryable) dan `ipc.call_timeout` (`timeout`, `details` `method`, `seconds`, `idempotencyKey`) ke katalog dengan kenaikan minor; tambah `ContractNames.swift` ke output codegen dan `--check`; catat evaluator murni `verify_path`, urutan cek, pemetaan `io::ErrorKind` ke snake_case, dan fixture `protocols/fixtures/endpoint/checks.json`; pastikan `ClientKind` punya `gui`; tetapkan kode client untuk timeout CLI (`ipc.call_timeout`) supaya CLI dan GUI memakai kode yang sama. Diterapkan di spec pemiliknya (2026-10-18).
- [x] `/architect` update spec 0012 (pemilik fitur 12): catat lebar sempit 640 pt sebagai lebar konten minimum final dari spec 0013; tulis nama setter environment `MutationAvailability`; catat status daemon di footer sidebar memakai `StatusLabel` konteks `.row`, banner informative untuk starting, connecting lambat, dan shutting down, serta banner critical untuk incompatible, untrusted, recovery, dan misconfigured. Diterapkan di spec pemiliknya (2026-10-18).
- [x] Spec 0007: konfirmasi atau koreksi asumsi DTO operasi, pagination token, filter state tidak akhir, event area `operation.`, dan `operationId` di reply. Dicek 2026-10-18; selisih diperbaiki di spec 0007 dan 0010 (cursor opaque, `runtimeVersion`, enum tertutup, instance retained lewat `instance.state_changed`).
- [x] Spec 0010 atau build fitur 14: konfirmasi atau koreksi asumsi method list dan get instance, pagination token, isi event `instance.state_changed` dan `instance.health_changed`, event instance baru dan dihapus, dan enum lifecycle, readiness, serta health yang tertutup. Dicek 2026-10-18; selisih diperbaiki di spec 0007 dan 0010 (cursor opaque, `runtimeVersion`, enum tertutup, instance retained lewat `instance.state_changed`).
- [x] Spec 0005 atau build fitur 5 dan 6: tetapkan bahwa `RecoveryReport` dikirim sebagai field opsional `recovery` di `DaemonHealth` (atau tempat lain yang setara di status daemon), terisi hanya dalam recovery mode. Diterapkan di spec pemiliknya (2026-10-18).
- [ ] Fitur 15: ganti petunjuk "Start it with stacklyd" (`// provisional: F15`) dengan alur start daemon dari GUI, dan pertimbangkan verifikasi tanda tangan peer lewat audit token di client Swift.
- [ ] Fitur 32: ganti copy notice tutup jendela dengan copy menu bar dari mockup, isi Settings, putuskan activation policy saat jendela ditutup.
- [ ] Fitur 48: tampilkan isi `RecoveryReport` dan alur restore; ganti petunjuk recovery di Diagnostics.
- [ ] Fitur 22, 34, 35, 44, 17, 18: ganti `UnbuiltPage` dan daftar instance sementara masing masing; fitur 34 menaruh hasil doctor di bawah seksi Connection; fitur 35 memakai `OperationCenter`.
- [ ] Fitur 67: render `MutationOutcome` dengan copy `MutationFeedback`. Fitur 69: daftarkan perintah reconnect ke `DaemonModel.reconnect()` dan perintah pindah bagian sidebar. Fitur 68: XCUITest shell dan fixture tampilan dari DTO `StacklyClient`.
- [ ] `/scope`: di `docs/scope/ui-macos.md` bagian Selisih yang sudah diketahui, catat badge sidebar native, aksi Open Diagnostics di banner offline, banner informative dan critical untuk status daemon lain, copy notice dengan Dock sampai fitur 32, dan ukuran minimum 900 × 600.
- [ ] `/sync` setelah build: perbarui `clients/swift/AGENTS.md` (folder `Endpoint/`, `Transport/`, `Wire/`, `Connection/`, `Link/`, `Sync/`, `ContractNames.swift`, product `StacklyTestSupport`, integration test dengan `STACKLY_DAEMON_BIN`, aturan tanpa jaringan dan tanpa spawn), `apps/macos/AGENTS.md` (`StacklyUI` → `StacklyClient`, struktur shell, scene `Window`, app tetap hidup setelah jendela ditutup, spec 0013 di Related specs), dan perintah test integrasi Swift di root `AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, keputusan kecil, dan hasil cross check ada di [rationale.md](rationale.md).
