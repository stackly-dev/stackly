# 0015. Background agent dan instalasi CLI

**Date**: 2026-10-19
**Status**: Proposed

## Summary

Spec ini memutuskan cara `stacklyd` dan `stackly` ikut di dalam `Stackly.app` dan cara daemon berjalan sendiri di latar sebagai LaunchAgent per user yang didaftarkan lewat `SMAppService` (API Apple untuk mendaftarkan helper dan login item), dengan persetujuan pengguna di System Settings. GUI dan CLI sama sama membaca status pendaftaran itu, menampilkannya sebagai enam state yang jujur (termasuk `requires_approval`, `disabled`, dan `not_registered`), dan hanya mengubah pendaftaran saat pengguna menekan tombol atau menjalankan perintah; start, stop, dan restart tidak pernah menjalankan proses lain, karena stop dan restart lewat IPC dan start lewat daftar ulang `SMAppService`. CLI dipasang sebagai symlink yang dikelola di `~/.local/bin`, tanpa admin dan tanpa mengubah PATH atau file shell. Semua yang butuh signing identity nyata, pendaftaran `SMAppService` nyata, dan penolakan izin sungguhan di System Settings dikumpulkan di milestone 4 dan tercatat `BLOCKED` sampai identity ada (OQ-012); langkah yang mendaftarkan agent di Mac ini, mengubah Login Items, atau memakai akun macOS kedua hanya berjalan setelah persetujuan kamu, tidak pernah tanpa diawasi. Sisanya dibangun dan diuji tanpa pengawasan dengan fake, CLI, dan fixture GUI.

## Requirements

**User stories**:

- Sebagai pengguna Mac, saya ingin daemon Stackly berjalan di latar sejak login dan tetap jalan saat jendela ditutup, dengan izin yang saya berikan lewat mekanisme macOS sendiri, supaya instance saya tetap diawasi tanpa saya membuka aplikasi.
- Sebagai pengguna yang menolak atau mematikan izin itu, saya ingin GUI dan `stackly daemon status` menyebut keadaannya dengan jujur beserta langkah pemulihannya, dan tidak ada yang mengubah setting itu diam diam, supaya saya tetap memegang keputusan.
- Sebagai pengguna Terminal, saya ingin `stackly daemon install`, `start`, `stop`, dan `restart` dengan `--json` dan exit code yang stabil, plus completion shell dan man page, supaya automation saya bisa mengendalikan daemon tanpa GUI.
- Sebagai pengguna yang memasang CLI dari aplikasi, saya ingin satu tombol yang membuat command `stackly` tersedia tanpa password admin dan tanpa file shell saya diubah, supaya tidak ada yang berubah di luar yang saya setujui.
- Sebagai release owner, saya ingin bundling, signing development, dan registrasi helper terbukti sejak fase ini dengan bukti yang jelas, termasuk apa yang masih `BLOCKED`, supaya masalah packaging tidak baru ketahuan saat rilis (RISK-10).

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-014):

- **AC-1**: `cargo xtask build-app` (Debug dan `--release`) menghasilkan `Stackly.app` dengan isi persis seperti tabel *Isi bundle*: `Contents/MacOS/Stackly`, `Contents/MacOS/stacklyd`, dan `Contents/MacOS/stackly` (ketiganya mode 0755, arm64), plist LaunchAgent di `Contents/Library/LaunchAgents/com.ojiepermana.stackly.daemon.plist` yang byte nya sama dengan golden file dan lolos `plutil -lint`, man page di `Contents/Resources/man/man1/`, dan catalog dari fitur 17 di `Contents/Resources/catalog/`. Bundle tidak memuat `stackly-fake-server` atau path apa pun dengan segmen `testing` di bawah `Contents/Resources/catalog/`. `CFBundleShortVersionString` app sama dengan versi workspace yang dicetak `stacklyd --version` dan `stackly --version`. Langkah mana pun yang gagal menghentikan build dengan exit bukan 0 sebelum signing, tanpa bundle setengah jadi di path keluaran. Identifier di tabel *Identifier sementara* ada di satu modul Rust dan satu file Swift, dan keduanya lolos fixture `protocols/fixtures/bundle/identifiers.json` (OQ-001, OQ-006, PRD bagian 25).
- **AC-2**: Signing berjalan dari dalam ke luar setelah assembly: setiap Mach-O lain di `Contents/MacOS` dan `Contents/Frameworks` kalau ada, lalu `stacklyd` dengan identifier `com.ojiepermana.stackly.daemon`, `stackly` dengan `com.ojiepermana.stackly.cli`, lalu app, semuanya dengan hardened runtime (`--options runtime`) dan tanpa entitlement. Identity diambil dari `--identity`, lalu `STACKLY_SIGN_IDENTITY`, lalu ad hoc (`-`) sebagai default. Dengan signing ad hoc, `codesign --verify --strict --deep` lulus untuk app, dan xtask mencetak per binary identifier, `ad hoc`, dan designated requirement. AC ini bisa dibuktikan sekarang tanpa identity (RISK-10).
- **AC-3**: `stackly-platform` punya trait `BackgroundAgent` (`status`, `register`, `unregister`, `relaunch`) beserta tipenya, port lewat `Platform::with_background_agent` dengan default `UnsupportedBackgroundAgent`, dan `stackly-testkit` punya `FakeBackgroundAgent` yang dipasang `FakePlatform`. `MacosPlatform::new` dan `MacosPlatform::with_roots` memasang `MacosBackgroundAgent` dan melaporkan capability `background_agent` supported; `UnsupportedPlatform` melaporkannya not supported. `cargo xtask check-deps` gagal kalau `objc2`, `objc2-foundation`, atau `objc2-service-management` menjadi dependency langsung crate selain `stackly-platform-macos`, atau `clap_complete` dan `clap_mangen` selain `stackly-cli` (NFR-010).
- **AC-4**: Satu fungsi murni `classify` di `stackly-platform` dan satu padanannya di `StacklyUI` memetakan hasil probe ke `BackgroundAgentState` (`enabled`, `requires_approval`, `disabled`, `not_registered`, `not_found`, `unavailable`), alasan `unavailable`, dan langkah pemulihan, persis seperti tabel *State background agent*, dan keduanya lolos semua baris `protocols/fixtures/agent/states.json`. `disabled` hanya muncul untuk `requiresApproval` yang punya launch record dan bukan hasil pendaftaran di aksi yang sama. `STACKLY_ROOT` yang aktif selalu menghasilkan `unavailable` alasan `root_override` tanpa satu panggilan pun ke OS (REQ-014, PRD bagian 25).
- **AC-5**: `stacklyd` menentukan konteks launch sekali di `main`: `launchd` kalau dan hanya kalau parent PID 1 dan `XPC_SERVICE_NAME` sama dengan `com.ojiepermana.stackly.daemon` atau diawali `dev.stackly.test.agent.`, selain itu `manual`. `DaemonInfo.launch` membawa konteks, label, dan path bundle (null di luar bundle). Hanya daemon berkonteks `launchd` dengan label produksi dan tanpa `STACKLY_ROOT` yang menulis launch record `<data>/background-agent.json` secara atomik setelah lock didapat; tulis yang gagal hanya menjadi satu warning log dan tidak menghentikan start.
- **AC-6**: Method baru `daemon.shutdown { mode }` (ditandai `recovery allowed`) membalas `ShutdownAccepted` lalu menjalankan urutan berhenti spec 0006 dengan reason `requested` (nilai yang sudah ada di `ShutdownReason`) untuk `stop` atau `restart_requested` (nilai baru) untuk `restart`. `stop` keluar 0. `restart` keluar 75 dan hanya diterima daemon berkonteks `launchd`; daemon `manual` menjawab `daemon.restart_unavailable` (kategori `unsupported`) tanpa berhenti. Request kedua selama berhenti mendapat `daemon.shutting_down`. Event `daemon.shutting_down` membawa reason itu. `ProtocolVersion::CURRENT` naik satu minor dan fixture baru lolos di Rust dan Swift (REQ-028, REQ-047).
- **AC-7**: `stackly daemon status` selalu menampilkan blok agent (state, alasan, langkah pemulihan, label, path bundle, waktu launch terakhir dari launch record), juga saat daemon tidak terjangkau, dengan baris teks `Next` dari tabel *Langkah pemulihan CLI* kalau state nya butuh tindakan. Dengan `--json`, `result` adalah `{ info: DaemonInfo | null, health: DaemonHealth | null, client: {...} | null, agent: AgentReport }`; saat daemon tidak jalan, `result` terisi bersama `error` (`ipc.endpoint_missing` atau `ipc.connection_refused`) dan exit tetap 6. Perubahan tipe ini menaikkan `CLI_SCHEMA_VERSION` menjadi 2 di commit yang sama, dengan golden file dan contoh diperbarui (aturan stabilitas spec 0008). Daemon yang berjalan dari copy app lain menambah warning `cli.agent_bundle_mismatch` (REQ-028, REQ-014).
- **AC-8**: `stackly daemon install` hanya memanggil `register` kalau state `not_registered`. State `enabled` tidak memanggil OS dan keluar 0. `requires_approval` dan `disabled` gagal dengan `cli.agent_requires_approval` atau `cli.agent_disabled` (exit 4) tanpa memanggil `register`, sehingga izin tidak pernah dibuka diam diam. Hasil `register` dipetakan lewat tabel *Hasil pendaftaran*: `enabled` menunggu daemon paling lama 30 detik lalu keluar 0 (dengan warning `cli.daemon_not_running` kalau daemon belum menjawab), `requiresApproval` gagal exit 4, dan setiap kode error `SMAppService` yang dikenal mendapat kode dan exit nya sendiri. `STACKLY_ROOT` aktif gagal `cli.agent_root_override` (exit 2), CLI di luar `Stackly.app` gagal `cli.agent_not_bundled` (exit 5) dengan hint yang menyebut path `stackly` di dalam bundle kalau path canonical nya berada di bundle. Setiap kegagalan menulis hint dari tabel *Langkah pemulihan CLI* dan, dengan `--json`, hasil parsial `{ agent, info: null }` bersama `error` (REQ-028, REQ-014).
- **AC-9**: `stackly daemon start` keluar 0 tanpa memanggil OS kalau daemon sudah menjawab handshake. Kalau socket tidak ada atau ditolak dan state `enabled`: bila `lastLaunchedAt` di launch record kurang dari 15 detik yang lalu, CLI lebih dulu menunggu socket paling lama 15 detik tanpa mengubah apa pun; kalau daemon tetap tidak ada, CLI memanggil `relaunch`, menunggu handshake dengan `serverInstanceId` apa pun paling lama 30 detik, lalu keluar 0, atau gagal `cli.daemon_wait_timeout` (exit 8). State lain gagal tanpa mengubah pendaftaran: `not_registered` dengan `cli.agent_not_registered` (exit 6, hint `stackly daemon install`), `requires_approval` dan `disabled` exit 4, `not_found` exit 6, `unavailable` dengan kode alasannya. Error koneksi lain (endpoint tidak dipercaya, major berbeda) dilaporkan apa adanya tanpa daftar ulang. Kegagalan memberi hasil parsial `{ alreadyRunning: false, agent, info: null }` (REQ-028, BR-011).
- **AC-10**: `stackly daemon stop` mengirim `daemon.shutdown { mode: stop }` ke daemon root aktif (konteks apa pun), lalu menunggu EOF di koneksi yang mengirim request itu (daemon menutup koneksi tepat sebelum melepas lock) dan memastikan socket sudah hilang, paling lama 20 detik total, lalu keluar 0; lewat batas itu `cli.daemon_wait_timeout` (exit 8). Jawaban `daemon.shutting_down` dianggap "sudah sedang berhenti" dan penantian yang sama berlanjut. Daemon yang tidak jalan juga keluar 0 dengan `wasRunning: false`. `stackly daemon restart` mengirim `mode: restart` (`daemon.shutting_down` diperlakukan sama), menunggu handshake dengan `serverInstanceId` baru paling lama 30 detik, dan keluar 0; daemon `manual` gagal `daemon.restart_unavailable` (exit 5) tanpa berhenti; daemon yang tidak jalan diperlakukan seperti `start`. Daemon lama yang tidak mengenal method itu menghasilkan exit 5 dengan hint update. Ctrl+C selama menunggu keluar 130 lewat `cli.interrupted` dan tidak membatalkan apa pun. Kegagalan memberi hasil parsial dengan `info` null (REQ-028).
- **AC-11**: Pendaftaran tidak pernah berubah tanpa aksi pengguna. Di Rust, `register` hanya dipanggil dari `daemon install` dan `relaunch` hanya dari `daemon start` dan `restart` untuk daemon yang tidak jalan; test dengan `FakeBackgroundAgent` membuktikan `daemon status`, `stop`, `restart` untuk daemon yang jalan, setiap hint error, dan setiap command lain hanya memanggil `status`. Di Swift, `register` hanya dipanggil dari tombol Turn On dan `relaunch` hanya dari Start stacklyd, dan test model membuktikan refresh, aktivasi app, dan polling hanya memanggil `status`; test aturan sumber membatasi `import ServiceManagement` ke satu file (REQ-014, AC-014).
- **AC-12**: `stackly completion <bash|zsh|fish>` mencetak script completion dari definisi clap ke stdout (dengan `--json`, `result` `{ shell, script }`). Perintah tersembunyi `stackly manpage` mencetak semua halaman man dari `clap_mangen` (`--json` memberi `{ pages: [{ file, roff }] }`), dan `build-app` menulis halaman itu ke bundle. Hint lama `start it with stacklyd` di CLI diganti hint dari tabel *Langkah pemulihan CLI*, dan test memastikan teks lama tidak ada lagi di `apps/cli` (REQ-028).
- **AC-13**: Saat presence GUI `unreachable` dengan kode `ipc.endpoint_missing` atau `ipc.connection_refused`, banner jendela, teks `ConnectionHint`, dan seksi Diagnostics *Background Agent* mengikuti tabel *Banner per state* untuk setiap state, termasuk ambang 15 detik sejak `since` sebelum aksi Start stacklyd muncul untuk state `enabled`. Turn On memanggil `register`; hasil `requiresApproval` membuka Login Items sekali. Start stacklyd memanggil `relaunch` dan, kalau daemon belum menjawab dalam 30 detik, menampilkan `AgentActionError.timedOut` di Last error. Open Login Items memanggil `SMAppService.openSystemSettingsLoginItems()`. Status dibaca ulang saat app aktif kembali, setiap 5 detik selama tidak terjangkau dan app aktif, dan setelah setiap aksi. Petunjuk `Start it with stacklyd` (`// provisional: F15`) hilang. Setiap aksi bisa dijalankan dengan keyboard, punya label VoiceOver, dan perubahan state diumumkan lewat `announce(_:)`; tampilan mengikuti Dynamic Type dan kontras spec 0012 (REQ-014, REQ-027, PRD bagian 25).
- **AC-14**: Instalasi CLI hanya membuat symlink `~/.local/bin/stackly` dan symlink man page `~/.local/share/man/man1/stackly*.1` ke file di bundle yang sedang berjalan, setelah konfirmasi eksplisit, dan hanya membuat folder `~/.local/bin` dan `~/.local/share/man/man1` bila belum ada. Ia tidak pernah menulis file shell (`.zshrc`, `.zprofile`, `.bash_profile`, `config.fish`), `/etc/paths.d`, atau `/usr/local/bin`, dan tidak pernah meminta hak admin. Path yang sudah berisi file atau symlink yang bukan link terkelola dilaporkan `conflict` dan tidak disentuh. Status `installed`, `stale`, `broken`, `conflict`, dan `not_installed` mengikuti aturan *Instalasi CLI*, dibuktikan test Swift di home sementara (PRD bagian 25).
- **AC-15**: Di Mac ini, tanpa signing, test native dengan LaunchAgent sementara di domain `gui/<uid>` (plist nya diturunkan dari golden file produksi dengan `Label` dan `BundleProgram` diganti dan `STACKLY_ROOT` ditambah, dan test memastikan semua key lain sama dengan golden file) membuktikan: `DaemonInfo.launch.context` bernilai `launchd`; `daemon.shutdown restart` memunculkan daemon baru (`serverInstanceId` berbeda) dalam 30 detik; `stop` membuat daemon tetap mati minimal 20 detik (lebih dari `ThrottleInterval`); `SIGKILL` ke daemon memunculkan daemon baru dalam 20 detik; dan agent yang start saat daemon manual memegang lock keluar 3 berulang lalu mengambil alih paling lama 20 detik setelah daemon manual berhenti. Semua agent test dicabut di setiap jalur keluar, termasuk lewat `--cleanup`. Test ini menjalankan job launchd sementara di sesi kamu, jadi ia hanya berjalan di run native yang kamu setujui, seperti spike spec 0003 (REQ-014, REQ-028, PRD bagian 32.2, gate G0).
- **AC-16**: Test native yang sama membuktikan proses instance (fake provider) yang berjalan di session sendiri tetap hidup dengan PID dan waktu start yang sama saat job daemon di `launchctl kickstart -k` dan saat di `launchctl bootout`, lalu diadopsi dengan identitas terverifikasi oleh daemon berikutnya, tanpa `AbandonProcessGroup`. Kalau gagal, fitur kembali ke `/architect` untuk keputusan survival (spec 0010 Follow-up) (REQ-013, AC-013).
- **AC-17**: Dengan build yang ditandatangani identity Apple Development nyata dan dipasang di `~/Applications`, record manual `native` (dijalankan setelah persetujuan kamu) membuktikan alur `SMAppService` sungguhan: `not_registered` terlihat di GUI dan CLI; Turn On atau `stackly daemon install` lalu mengikuti salah satu cabang, dan cabang yang terjadi dicatat. Cabang A: hasilnya `enabled` (biasanya dengan notifikasi Background Items Added), daemon jalan dengan konteks `launchd` dan launch record tertulis; izin dimatikan di System Settings dan Diagnostics serta `stackly daemon status` menampilkan `disabled`. Cabang B: hasilnya `requires_approval` tanpa launch record; izin dibiarkan mati dan state tidak berubah sendiri; izin dinyalakan dan daemon jalan dengan konteks `launchd`; izin dimatikan lagi dan state menjadi `disabled`. Di kedua cabang, perilaku daemon saat izin dimatikan dicatat (kalau macOS menghentikannya, banner `disabled` tampil dan instance tetap hidup); izin dinyalakan lagi dan instance diadopsi; Start stacklyd dan `stackly daemon start` setelah `stop` menyalakan daemon. Kalau macOS meminta persetujuan lagi setelah `relaunch`, GUI dan CLI menampilkan `requires_approval` dengan langkah pemulihan, dan temuan itu membawa cara start kembali ke `/architect`. Selama Mac ini tanpa identity, record ini `BLOCKED` (reason `no code signing identity on this Mac (OQ-012)`, owner `release owner (OQ-012)`) (REQ-014, AC-014, NFR-015, RISK-10, gate G0).
- **AC-18**: Dengan identity yang sama, record manual `native` (setelah persetujuan kamu; `BLOCKED` dengan reason `no code signing identity on this Mac (OQ-012)` dan owner `release owner (OQ-012)` sampai identity ada) membuktikan item login Keychain yang dibuat saat signing ad hoc tetap bisa dipakai setelah helper ditandatangani: aksi pengguna pertama memunculkan satu dialog per item (Always Allow, atau tulis ulang lewat `stackly secret set`), pemicu latar sebelum itu menghasilkan `access_denied` tanpa dialog, dan setelah itu build ulang dengan identity yang sama tidak memunculkan dialog lagi. Keputusan tetap login Keychain (bukan data protection keychain) tercatat di spec 0011 lewat `/sync` (REQ-016, AC-062, spec 0011 Follow-up).
- **AC-19**: Dengan identity yang sama, record manual `native` (setelah persetujuan kamu) membuktikan bagian NFR-015 milik fitur ini di akun macOS baru yang bukan developer: clean install dari `~/Applications`, approval denied, reinstall di path yang sama, upgrade (bundle diganti saat daemon jalan, lalu `stackly daemon restart` menjalankan versi baru, terlihat dari `DaemonInfo.version`), dan login ulang yang menyalakan daemon lewat `RunAtLoad`. Untuk reinstall dan upgrade, state agent sesudahnya dicatat; kalau macOS meminta persetujuan lagi, GUI dan CLI menampilkan `requires_approval` dengan langkah pemulihan. Record ini punya dua prasyarat yang masing masing tercatat sendiri saat belum ada: identity nyata (`BLOCKED`, owner `release owner (OQ-012)`) dan akun macOS kedua yang dibuat pengguna dengan hak admin (`BLOCKED`, reason `no second macOS user account on this Mac`, owner `user`). Uninstall dengan data tetap milik fitur 62 (NFR-015, REQ-014, PRD bagian 32.2).
- **AC-20**: Dengan identity Apple Development nyata, `cargo xtask build-app --release --identity <identity>` menghasilkan bundle yang `codesign -dv` nya menampilkan `TeamIdentifier` yang sama di app, `stacklyd`, dan `stackly`, designated requirement yang tidak memuat `cdhash`, dan `codesign --verify --strict --deep` lulus. Bukti ini `native` manual dan tercatat `BLOCKED` (reason `no code signing identity on this Mac (OQ-012)`, owner `release owner (OQ-012)`) sampai identity ada (OQ-012, RISK-10, gate G0).
- **AC-21**: Sebelum langkah CLI dan GUI milestone 1 dan 3 dibangun, spike manual ad hoc (setelah persetujuan kamu, karena ia mendaftarkan agent sungguhan di Mac ini) menjawab dan mencatat di rationale dengan record manual `native`: apakah build ad hoc bisa didaftarkan dan status yang dihasilkan; status dari tool di `Contents/MacOS` dan lewat symlink di luar bundle; apakah `unregister` lalu `register` menyalakan job tanpa persetujuan baru; apakah `XPC_SERVICE_NAME` sama dengan label; status setelah izin dimatikan; dan apakah `unregister` menghentikan job yang sedang jalan. Hasil yang membantah asumsi spec ini membawa bagian terkait kembali ke `/architect` sebelum langkah 5 dan 6. Hasil "ad hoc ditolak" adalah hasil yang sah dan dicatat, bukan kegagalan build (REQ-014, RISK-10, gate G0).
- **AC-22**: `cargo xtask build-app --install [<dir>]` (default `~/Applications`) menolak `<dir>/Stackly.app` yang `CFBundleIdentifier` nya bukan `com.ojiepermana.stackly` tanpa menyentuhnya, mengganti bundle Stackly yang ada lewat staging dan rename sehingga `<dir>/Stackly.app` tidak pernah setengah jadi, tidak meninggalkan folder `.Stackly.app.*` setelah berhasil, dan mencetak hint `stackly daemon restart` kalau socket daemon root default ada. Diuji dengan folder tujuan sementara, tanpa mendaftarkan apa pun (PRD bagian 25, NFR-015).

## Decision

**Chosen option**: pilihan yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md): `SMAppService.agent` dengan LaunchAgent di dalam bundle, ketiga binary di `Contents/MacOS`, stop dan restart lewat IPC dengan `KeepAlive` yang hanya menyalakan ulang saat exit gagal, start lewat `relaunch` (daftar ulang `SMAppService`) di belakang port `BackgroundAgent` yang dipanggil CLI (objc2) dan padanan Swift di GUI, `disabled` diturunkan dari launch record, assembly dan signing satu pintu di `cargo xtask build-app`, CLI lewat symlink terkelola di `~/.local/bin`, tetap login Keychain, dan verifikasi tanda tangan peer ditunda ke fitur 60 dan 63.

Pendaftaran helper selalu lewat mekanisme macOS dan hanya berubah saat pengguna bertindak, sedangkan kendali harian daemon lewat IPC yang sudah ada, sehingga tidak ada satu pun bagian Stackly yang menjalankan `launchctl` atau proses lain di produk.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `signing-entitlements` (`openai/plugins`, `.claude/skills/signing-entitlements/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. `signing-entitlements` dipakai untuk memeriksa hasil `codesign` dan Gatekeeper saat milestone 1 dan 4, bukan untuk memilih entitlement (spec ini tidak memakai entitlement).

## Feature design

### Batas dengan fitur lain

| Bagian | Pemilik |
| --- | --- |
| Identifier sementara, isi bundle, plist LaunchAgent, assembly dan signing development, port `BackgroundAgent`, state agent, `daemon.shutdown`, `stackly daemon install`, `start`, `stop`, `restart`, blok agent di `daemon status`, completion dan man page, banner dan Diagnostics agent di GUI, instalasi CLI, bukti signing dan Keychain | spec ini |
| Envelope, handshake, urutan start dan berhenti daemon, lock, socket | spec 0006 (dipakai, ditambah method dan exit code di sini) |
| Kontrak `--json`, exit code, `Session`, `Confirm`, pola command group | spec 0008 (dipakai; `CLI_SCHEMA_VERSION` naik ke 2 di sini) |
| Proses instance di session sendiri, adopsi setelah daemon restart | spec 0010 (dibuktikan di LaunchAgent nyata di sini) |
| Login Keychain dan dialog untuk aksi pengguna | spec 0011 (dibuktikan setelah signing di sini) |
| Shell, presence, banner, Diagnostics Connection | spec 0013 (ditambah seksi dan baris banner di sini) |
| Catalog di `Contents/Resources/catalog/` dan builder nya | fitur 17 (spec 0017), dipanggil `build-app` |
| Layar onboarding Background Agent dan Command Line | fitur 16, memakai model dan aksi dari spec ini |
| Setting autostart daemon, matikan background service, perilaku logout | fitur 31 |
| Baris background startup dan CLI di Settings, menu bar | fitur 32 |
| Restart terencana dengan jendela disruption | fitur 30 |
| Status job launchd dan kegagalan start di doctor | fitur 34 |
| Developer ID, timestamp aman, notarization, installer | fitur 60 |
| Update app dan identitas helper lintas versi | fitur 61 |
| Uninstall, unregister saat uninstall, data tetap | fitur 62 |

### Identifier sementara (OQ-001)

Semua nilai di bawah sementara sampai OQ-001 ditutup sebelum rilis publik. Satu sumber di Rust (`stackly-platform::bundle`, konstanta publik) dan satu di Swift (`StacklyUI/Agent/AgentIdentifiers.swift`), keduanya diuji terhadap `protocols/fixtures/bundle/identifiers.json`. Mengganti nilai berarti satu commit yang mengubah fixture dan kedua sumber.

| Nama | Nilai |
| --- | --- |
| Bundle ID app | `com.ojiepermana.stackly` (spec 0001) |
| Label launchd job | `com.ojiepermana.stackly.daemon` |
| Nama file plist | `com.ojiepermana.stackly.daemon.plist` |
| Identifier code signing `stacklyd` | `com.ojiepermana.stackly.daemon` |
| Identifier code signing `stackly` | `com.ojiepermana.stackly.cli` |
| Awalan label agent test | `dev.stackly.test.agent.` |
| Folder instalasi development | `~/Applications/Stackly.app` |

### Isi bundle

```text
Stackly.app/Contents/
├── Info.plist                         # dari Xcode; CFBundleShortVersionString = versi workspace
├── MacOS/
│   ├── Stackly                        # app SwiftUI
│   ├── stacklyd                       # daemon, BundleProgram LaunchAgent
│   └── stackly                        # CLI, target symlink ~/.local/bin/stackly
├── Library/LaunchAgents/
│   └── com.ojiepermana.stackly.daemon.plist
└── Resources/
    ├── catalog/                       # fitur 17, tanpa namespace testing/
    └── man/man1/stackly*.1            # dari stackly manpage
```

- `stacklyd` dan `stackly` adalah Mach-O polos tanpa Info.plist sendiri. Info.plist app tidak memakai `LSFileQuarantineEnabled`, dan tidak ada kode yang memanggil API quarantine (fitur 18 bergantung pada ini).
- `AppBundle::enclosing(exe: &Path) -> Option<AppBundle>` (murni, di `stackly-platform::bundle`) menerima path canonical dan mengembalikan bundle hanya kalau bentuknya persis `<X>.app/Contents/MacOS/<file>` (akhiran `.app` dibandingkan tanpa peduli huruf besar). `AppBundle` memberi `root()`, `contents_dir()`, `macos_dir()`, `resources_dir()`, `launch_agents_dir()`, `daemon_executable()`, `cli_executable()`, dan `daemon_plist()`. Fitur 17 menurunkan folder catalog dari `resources_dir().join("catalog")`.
- Bin membaca `std::env::current_exe()` sekali di `main`, membuatnya canonical, lalu memanggil `AppBundle::enclosing`. Library tidak pernah memanggil `current_exe`.

### Plist LaunchAgent

Dirender `xtask` dari konstanta di atas (tidak ditulis tangan), diuji terhadap golden file `xtask/tests/golden/com.ojiepermana.stackly.daemon.plist`, dan dicek `plutil -lint` saat build.

| Key | Nilai | Alasan singkat |
| --- | --- | --- |
| `Label` | `com.ojiepermana.stackly.daemon` | identitas job di launchd |
| `BundleProgram` | `Contents/MacOS/stacklyd` | path relatif bundle, hanya untuk plist yang didaftarkan `SMAppService` |
| `AssociatedBundleIdentifiers` | `[com.ojiepermana.stackly]` | Login Items menampilkan Stackly, bukan nama binary |
| `RunAtLoad` | true | daemon jalan saat didaftarkan, diizinkan, atau login |
| `KeepAlive` | `{ SuccessfulExit: false }` | dinyalakan ulang hanya kalau exit bukan 0 atau mati oleh sinyal; stop (exit 0) tetap mati |
| `ThrottleInterval` | 10 | default launchd, ditulis eksplisit karena waktu tunggu CLI dihitung dari angka ini |
| `ExitTimeOut` | 30 | lebih panjang dari urutan berhenti spec 0006 (paling lama sekitar 12 detik) |
| `ProcessType` | `Standard` | tanpa pembatasan latar, karena proses instance (database) diturunkan dari daemon |

Tidak ada `ProgramArguments`, `EnvironmentVariables`, `StandardOutPath`, `StandardErrorPath`, `MachServices`, `Sockets`, `AbandonProcessGroup`, atau `LimitLoadToSessionType`. Daemon di bawah launchd memakai root default, karena environment job tidak membawa `STACKLY_ROOT`.

### Build, assembly, dan signing

`cargo xtask build-app [--release] [--identity <identity>] [--install [<dir>]]` adalah satu satunya perakit bundle yang lengkap:

0. Hapus folder `target/app/.stage-*` sisa build sebelumnya (hanya di dalam `target/app`).
1. `cargo build --locked --message-format=json -p stacklyd -p stackly-cli` (dengan `--release` untuk `--release`, tanpa `--target`, jadi target host `aarch64-apple-darwin`); path kedua executable diambil dari pesan `compiler-artifact` di output JSON, bukan ditebak.
2. `xcodebuild` seperti sekarang ditambah `CODE_SIGNING_ALLOWED=NO`, `ENABLE_DEBUG_DYLIB=NO` (supaya build Debug tidak menaruh `Stackly.debug.dylib` dan `__preview.dylib` di `Contents/MacOS`), dan `MARKETING_VERSION=<versi workspace>`; hasilnya di `target/xcode/Build/Products/<Config>/Stackly.app` adalah bahan mentah tanpa signature, lalu disalin dengan `ditto` ke staging `target/app/.stage-<pid>/Stackly.app`.
3. Assembly di staging: salin kedua binary (mode 0755), tulis plist, panggil `xtask::catalog::build` milik fitur 17 di dalam proses dengan `--out <bundle>/Contents/Resources/catalog`, lalu jalankan `stackly manpage --json` dari binary yang baru dibangun dan tulis setiap halaman ke `Contents/Resources/man/man1/`. Langkah assembly adalah daftar berurutan di `xtask/src/build_app/assemble.rs`; fitur 17 menambah langkah catalog di sana. Sebelum fitur 17 dibangun, folder catalog tidak ada dan daemon melaporkan catalog tidak tersedia (spec 0017).
4. Cek isi: `Contents/MacOS` hanya berisi `Stackly`, `stacklyd`, dan `stackly`; tidak ada `stackly-fake-server`; tidak ada segmen `testing` di bawah `Resources/catalog`; mode dan arsitektur ketiga binary (`lipo -archs` sama dengan `arm64`).
5. Signing dari dalam ke luar: setiap Mach-O di `Contents/Frameworks` kalau ada (package Swift lokal biasanya terhubung statis, jadi folder itu kosong), lalu `codesign --force --sign <identity> --options runtime --timestamp=none --identifier <id> <file>` untuk `stacklyd` lalu `stackly`, lalu `codesign --force --sign <identity> --options runtime --timestamp=none <bundle>`. Tanpa entitlement. Identity dari `--identity`, lalu `STACKLY_SIGN_IDENTITY`, lalu `-`.
6. Verifikasi: `codesign --verify --strict --deep <bundle>`, lalu cetak per binary identifier, Team ID (`ad hoc` kalau tidak ada), dan designated requirement (`codesign -d -r-`).
7. Ganti `target/app/<Config>/Stackly.app` dengan staging lewat rename (bundle lama dipindah ke samping lalu dihapus), sehingga path keluaran tidak pernah berisi bundle setengah jadi. Kegagalan di langkah mana pun menghapus staging dan keluar bukan 0. xtask mencetak path keluaran ini, bukan path produk Xcode.
8. `--install [<dir>]` (default `~/Applications`): tolak kalau `<dir>/Stackly.app` ada dan `CFBundleIdentifier` nya bukan `com.ojiepermana.stackly`; salin dengan `ditto` ke `<dir>/.Stackly.app.staging-<pid>`, pindahkan bundle lama ke `<dir>/.Stackly.app.old-<pid>`, rename staging ke `Stackly.app`, hapus bundle lama. Setelah itu xtask mencetak `run stackly daemon restart to use the new stacklyd` kalau socket daemon root default ada.

Build dari Xcode IDE (Run di Xcode) tetap ditandatangani ad hoc oleh Xcode dan tidak memuat helper, plist, catalog, atau man page. GUI menampilkan state `unavailable` alasan `helper_missing` untuk build itu.

Signing tidak pernah memakai `--deep` (hanya verifikasi yang memakainya), tidak pernah meminta password, dan tidak pernah menulis ke Keychain. Developer ID, timestamp aman, dan notarization milik fitur 60.

### Port `BackgroundAgent` (ditambah ke spec 0004)

```rust
// stackly-platform, modul `agent`
pub trait BackgroundAgent: Send + Sync {
    /// Reads the registration state from the OS. Never prompts and never changes it.
    fn status(&self) -> Result<AgentProbe, BackgroundAgentError>;
    /// Registers the agent; the result may need the user's approval.
    fn register(&self) -> Result<AgentProbe, BackgroundAgentError>;
    /// Unregisters the agent; launchd stops launching it.
    fn unregister(&self) -> Result<AgentProbe, BackgroundAgentError>;
    /// Asks the OS to start the registered job now. On macOS: unregister, then register.
    fn relaunch(&self) -> Result<AgentProbe, BackgroundAgentError>;
}
```

- `AgentProbe` (`#[non_exhaustive]`): `Registration { registration: AgentRegistration, bundle: PathBuf }` (`bundle` adalah path `NSBundle.mainBundle` yang dipakai OS), `NotBundled { executable: PathBuf }`, `HelperMissing { bundle: PathBuf }`. `AgentRegistration`: `Enabled`, `RequiresApproval`, `NotRegistered`, `NotFound`, `Unrecognized(i64)` (nilai status OS yang tidak dikenal).
- `BackgroundAgentError` (`#[non_exhaustive]`, thiserror): `NotSupported { capability, os }` dan `Os { op: AgentOp, kind: AgentOsErrorKind, domain: String, code: i64, message: String }`, dengan `AgentOp` `status`, `register`, `unregister`, atau `relaunch`, dan `AgentOsErrorKind` dari tabel *Hasil pendaftaran*.
- `relaunch` adalah satu satunya cara start, supaya mekanismenya bisa diganti di adapter tanpa mengubah CLI atau GUI kalau spike AC-21 membantah daftar ulang sebagai start.
- `Platform::with_background_agent(Arc<dyn BackgroundAgent>)` dan getter `background_agent() -> &dyn BackgroundAgent`, default `UnsupportedBackgroundAgent` yang menjawab `NotSupported { capability: background_agent, os }`.
- `MacosBackgroundAgent` (`stackly-platform-macos::agent`, satu satunya pemakai `objc2-service-management`): setiap method di dalam `objc2::rc::autoreleasepool`. Langkahnya: baca `NSBundle.mainBundle`; kalau `bundleIdentifier` bukan `com.ojiepermana.stackly`, `NotBundled` dengan path executable; kalau plist atau `Contents/MacOS/stacklyd` tidak ada di bundle itu, `HelperMissing`; selain itu `SMAppService.agentServiceWithPlistName("com.ojiepermana.stackly.daemon.plist")` lalu `status`, `registerAndReturnError`, `unregisterAndReturnError`, atau keduanya berurutan untuk `relaunch`, dan baca `status` lagi sebagai hasil. `NSError` dipetakan lewat tabel *Hasil pendaftaran*. Setiap blok `unsafe` diberi komentar `// SAFETY:`.
- `FakeBackgroundAgent` (testkit): state awal, path bundle yang bisa disetel, transisi untuk `register`, `unregister`, dan `relaunch` yang bisa disetel (misalnya `NotRegistered` menjadi `RequiresApproval`), `fail_next(op, error)`, dan log panggilan berurutan.

**Hasil pendaftaran.** Kode `NSError` di domain `SMAppServiceErrorDomain` dipetakan berdasarkan nama simbol di `ServiceManagement/SMErrors.h` (nilai angkanya diambil dari header SDK saat build dan diuji terhadap konstanta itu, bukan ditulis tangan). Kode di domain lain dan simbol lain menjadi `Other`.

| Hasil | `AgentOsErrorKind` | Perlakuan CLI | Perlakuan GUI |
| --- | --- | --- | --- |
| `kSMErrorAlreadyRegistered` | `AlreadyRegistered` | dianggap berhasil, status dibaca ulang | sama |
| `kSMErrorLaunchDeniedByUser` | `LaunchDenied` | state dari `classify` setelah status dibaca ulang (`requires_approval` atau `disabled`), exit 4 | banner state itu |
| `kSMErrorJobMustBeEnabled` | `MustBeEnabled` | sama dengan baris di atas | sama |
| `kSMErrorInvalidSignature` | `InvalidSignature` | `cli.agent_invalid_signature`, exit 4, hint signing nyata | Last error "macOS rejected the signature of this build." |
| `kSMErrorJobNotFound`, `kSMErrorJobPlistNotFound` | `NotFound` | `cli.agent_not_found`, exit 6 | banner `not_found` |
| lainnya | `Other` | `cli.agent_os_error`, exit 7 | Last error dengan domain dan kode |
| berhasil, status sesudahnya `Enabled` atau `RequiresApproval` | | lanjut sesuai AC-8 dan AC-9 | lanjut |
| berhasil, status sesudahnya `NotFound` | | `cli.agent_not_found`, exit 6 | banner `not_found` |
| berhasil, status sesudahnya `NotRegistered` atau `Unrecognized` | | `cli.agent_os_error` dengan `osCode` null, exit 7 | Last error "macOS didn’t keep the registration." |
- Port ini dipanggil `stackly-cli`. `stacklyd` memasangnya lewat `MacosPlatform` hanya supaya capability jujur, dan tidak pernah memanggilnya.

### State background agent

`classify(input: &AgentInput) -> AgentAssessment` (murni, `stackly-platform::agent`), dengan `AgentInput { root_override: bool, probe: ProbeOutcome, launch_record: bool, just_registered: bool }`. `ProbeOutcome` adalah `NotChecked` (hanya saat root override, karena OS tidak dipanggil), `Unsupported`, `Failed { code: Option<i64> }` (error panggilan status dengan kode nya, atau `None` untuk timeout), atau `Probe(AgentProbe)`. Sebelum `classify`, CLI mengganti `Registration` yang `bundle` nya berbeda dari bundle canonical CLI itu sendiri dengan `NotBundled`, supaya laporan dan keputusan selalu tentang bundle yang sama (kasus symlink). `AgentAssessment { state, reason: Option<UnavailableReason>, recovery: RecoveryStep }`. Baris dievaluasi dari atas, yang pertama cocok menang:

| Kondisi | `state` | `reason` | `recovery` |
| --- | --- | --- | --- |
| `root_override` | `unavailable` | `root_override` | `manual_daemon_for_override` |
| `Unsupported` | `unavailable` | `unsupported_os` | `none` |
| `Failed`, atau `NotChecked` tanpa root override | `unavailable` | `status_failed` | `check_again` |
| `NotBundled` | `unavailable` | `not_bundled` | `use_bundled_cli` |
| `HelperMissing` | `unavailable` | `helper_missing` | `build_with_xtask` |
| `Unrecognized` | `unavailable` | `status_unrecognized` | `check_again` |
| `Enabled` | `enabled` | | `none` |
| `NotRegistered` | `not_registered` | | `register` |
| `NotFound` | `not_found` | | `move_or_reinstall_app` |
| `RequiresApproval` dan (`!launch_record` atau `just_registered`) | `requires_approval` | | `approve_in_login_items` |
| `RequiresApproval` dan `launch_record` | `disabled` | | `enable_in_login_items` |

- `classify` tidak tahu apakah daemon terjangkau. Pemanggil yang tahu daemon tidak terjangkau dan mendapat state `enabled` memakai langkah `start` (CLI: hint `stackly daemon start`; GUI: aksi Start stacklyd setelah ambang 15 detik).
- Nilai wire setiap enum ditulis snake_case dan stabil, karena muncul di `--json` CLI. Enum di Swift memakai raw value yang sama.
- `launch_record` berarti file `<data>/background-agent.json` root default ada dan berupa file biasa. Daemon hanya bisa menulisnya setelah launchd menjalankannya, jadi file itu bukti bahwa izin pernah diberikan. `just_registered` true hanya di dalam satu aksi pendaftaran (CLI `install` atau `start`, tombol GUI) dan sampai status berikutnya berbeda dari `requiresApproval`.
- `protocols/fixtures/agent/states.json` memuat satu baris per kombinasi masukan (`rootOverride`, `probe`, `launchRecord`, `justRegistered`) dengan `state`, `reason`, dan `recovery` yang diharapkan, dibaca test Rust dan Swift.

### Konteks launch dan launch record di daemon

- `LaunchContext::detect(xpc_service_name: Option<&OsStr>, parent_pid: u32) -> LaunchContext` (murni, `stackly-platform::agent`): `Launchd { label }` kalau `parent_pid == 1` dan nilai itu sama dengan `com.ojiepermana.stackly.daemon` atau diawali `dev.stackly.test.agent.`; selain itu `Manual`. `main` membaca `XPC_SERVICE_NAME` sekali dan `std::os::unix::process::parent_id()`.
- `DaemonInfo` mendapat field opsional `launch: DaemonLaunch { context: LaunchContextKind, label: Option<String>, bundlePath: Option<String> }`, dengan `LaunchContextKind` enum terbuka (`launchd`, `manual`, `unrecognized`). Baris log start menambah `launch_context`, `launch_label`, dan `bundle_path`.
- Plumbing di `stacklyd`: `StartConfig` mendapat `xpc_service_name: Option<OsString>`, `parent_pid: u32`, dan `current_exe: Option<PathBuf>` yang dibaca `main`; startup menghitung `LaunchContext` dan `AppBundle` lalu memasukkannya ke `DaemonDeps` (`launch`, `bundle`), yang juga bisa diisi langsung oleh test dalam proses. `DaemonHandle::stop_requested()` menunggu permintaan `daemon.shutdown` dan memberi `ShutdownMode`; startup menunggu itu bersama sinyal, menjalankan urutan berhenti yang sama, lalu keluar 0 untuk `stop` dan 75 (`EXIT_RESTART`) untuk `restart`.
- Launch record: hanya untuk `Launchd` berlabel produksi tanpa `STACKLY_ROOT`, setelah lock didapat (langkah 6 urutan start spec 0006) dan sebelum bind socket, daemon menulis `<data>/background-agent.json` lewat `write_file_atomic` (mode 0600): `{ "schema": 1, "firstLaunchedAt", "lastLaunchedAt", "bundlePath", "version" }`. `firstLaunchedAt` diambil dari file lama kalau terbaca, selain itu sama dengan `lastLaunchedAt`. File lama yang tidak terbaca ditimpa. Kegagalan apa pun dicatat sekali di level `warn` dan start berlanjut. Path nya `DirectoryLayout::agent_launch_record()` (spec 0004). CLI dan GUI hanya memeriksa keberadaannya dan membaca `lastLaunchedAt` (paling banyak 4 KiB).

### Method `daemon.shutdown` dan exit code

| Bagian | Isi |
| --- | --- |
| Params | `ShutdownParams { mode: ShutdownMode }`; `ShutdownMode` enum terbuka `stop`, `restart`, `unrecognized`; `Validate` menolak `unrecognized` dengan `request.invalid_params` |
| Reply | `ShutdownAccepted { serverInstanceId, mode }`, diantrekan sebelum langkah berhenti pertama sehingga ikut dikirim di langkah flush |
| Recovery | `recovery allowed` |
| Saat `starting` | diterima (tidak butuh subsystem) |
| Saat berhenti | request baru mendapat `daemon.shutting_down` seperti spec 0006 |
| `restart` di konteks `manual` | error `daemon.restart_unavailable` (kategori `unsupported`, `details.launchContext`), daemon tetap jalan |
| Efek | urutan berhenti spec 0006 yang sama dengan SIGTERM, dengan reason `requested` (stop) atau `restart_requested` (restart) di event `daemon.shutting_down` dan di log |
| Exit | `stop` 0; `restart` 75 (`EXIT_RESTART`), yang membuat launchd menyalakan daemon lagi lewat `KeepAlive` |

Tabel exit code `stacklyd` menjadi: 0 berhenti normal, 1 gagal start, 2 argumen salah, 3 lock dipegang daemon lain, 75 berhenti untuk restart. Di bawah launchd, exit 1 dan 3 juga dinyalakan ulang setiap `ThrottleInterval`; untuk exit 3 itu berarti agent mengambil alih otomatis setelah daemon manual berhenti.

Instance tidak dihentikan oleh `stop` maupun `restart` (spec 0006 dan 0010); proses nya tetap hidup di session sendiri dan diadopsi daemon berikutnya. Restart terencana yang menghentikan instance terdampak adalah keputusan fitur 30.

### Perintah CLI

Group `daemon` menjadi folder `apps/cli/src/commands/daemon/` (`mod.rs`, `status.rs`, `install.rs`, `start.rs`, `stop.rs`, `restart.rs`, `agent.rs`, `wait.rs`), ditambah `commands/completion.rs` dan `commands/manpage.rs`, mengikuti pola command group spec 0008. Logika keputusan setiap perintah ada di fungsi yang menerima `&dyn BackgroundAgent` dan trait kecil `DaemonProbe` (connect plus handshake), sehingga diuji dengan `FakeBackgroundAgent` dan daemon skrip.

- Panggilan port berjalan lewat `spawn_blocking` dengan batas `AGENT_CALL_TIMEOUT` 10 detik (lewat dari itu `cli.agent_call_timeout`, exit 8). Panggilan OS yang macet tidak bisa dibatalkan, jadi `main` CLI membuat runtime tokio sendiri dan, di setiap jalur keluar, mematikannya dengan `shutdown_timeout(Duration::ZERO)` supaya CLI tidak ikut menunggu thread yang macet. Kalau spike AC-21 menunjukkan `SMAppService` harus dipanggil di thread utama, CLI memanggil port di thread utama sebelum runtime tokio dibuat, dengan batas yang sama.
- Penantian daemon (`wait.rs`): percobaan connect plus handshake setiap 250 ms sampai batas perintah. Ctrl+C selama menunggu gagal `cli.interrupted` (exit 130) dengan `details.state` `waiting_for_daemon`.
- `STACKLY_ROOT` aktif: `install`, `start`, dan `restart` untuk daemon yang tidak jalan gagal `cli.agent_root_override` (exit 2) sebelum menyentuh OS; `stop`, `restart` untuk daemon yang jalan, dan `status` tetap bekerja lewat IPC ke daemon root itu.
- `stop` dan `restart`: setelah `ShutdownAccepted`, CLI membaca koneksi yang sama sampai EOF (daemon menutup koneksi di langkah 5 urutan berhenti spec 0006, tepat sebelum melepas lock), lalu memastikan percobaan connect berikutnya mendapat `ipc.endpoint_missing`. Jawaban `daemon.shutting_down` untuk request itu berarti daemon sudah sedang berhenti, dan penantian yang sama berlanjut. `restart` lalu menunggu handshake dengan `serverInstanceId` berbeda dari `ShutdownAccepted`.
- `start` untuk state `enabled`: kalau launch record menunjukkan `lastLaunchedAt` kurang dari 15 detik yang lalu (daemon mungkin sedang start atau menunggu `ThrottleInterval`), CLI menunggu socket paling lama 15 detik lebih dulu; baru setelah itu `relaunch`.

| Perintah | Hasil `--json` saat berhasil | Hasil `--json` saat gagal (bersama `error`) | Exit dan error utama |
| --- | --- | --- | --- |
| `daemon status` | `{ info, health, client, agent: AgentReport }` | `info`, `health`, dan `client` null saat daemon tidak terjangkau; `agent` selalu terisi | 0 saat daemon menjawab; 6, 4, atau 5 seperti spec 0008 |
| `daemon install` | `{ agent: AgentReport, info: DaemonInfo \| null }` | `{ agent, info: null }` | 0; kode dari tabel di bawah |
| `daemon start` | `{ alreadyRunning: bool, agent: AgentReport, info: DaemonInfo \| null }` | `{ alreadyRunning: false, agent, info: null }` | 0; 8 `cli.daemon_wait_timeout`; kode dari tabel di bawah |
| `daemon stop` | `{ wasRunning: bool, serverInstanceId: string \| null }` | sama, dengan nilai yang sudah diketahui | 0; 8 `cli.daemon_wait_timeout`; 5 daemon lama |
| `daemon restart` | `{ previousServerInstanceId: string \| null, agent: AgentReport \| null, info: DaemonInfo \| null }` | sama, `info` null | 0; 5 `daemon.restart_unavailable`; 8 `cli.daemon_wait_timeout`; kode dari tabel di bawah untuk daemon yang tidak jalan |
| `completion <shell>` | `{ shell, script }` | tidak ada | 0; 2 shell tidak dikenal (clap) |
| `manpage` (tersembunyi) | `{ pages: [{ file, roff }] }` | tidak ada | 0 |

`agent` di `restart` terisi hanya saat daemon tidak jalan (jalur `start`). `info` di `start` hanya null saat gagal; saat berhasil selalu terisi.

`AgentReport { state, reason, recovery, label, bundlePath, lastLaunchedAt }`; `recovery` adalah `RecoveryStep` dari `classify` (dengan `start` untuk `enabled` saat daemon tidak terjangkau), `bundlePath` dari `AppBundle` CLI itu sendiri, `lastLaunchedAt` dari launch record atau null. Teks manusia `daemon status` menambah baris `Agent` (state dengan tone: `enabled` positive, `requires_approval`, `disabled`, `not_registered` caution, `not_found` critical, `unavailable` neutral), `Launched` (`launchd (<label>)` atau `by hand`), `App` (path bundle), dan `Next` (teks tabel *Langkah pemulihan CLI*, hanya kalau `recovery` bukan `none`). Saat daemon tidak jalan, blok agent ditulis ke stdout dan error ke stderr.

**Langkah pemulihan CLI.** Satu tabel untuk tiga tempat: baris `Next` di `daemon status`, baris `hint:` untuk `ipc.endpoint_missing` dan `ipc.connection_refused` di semua perintah (menggantikan `start it with stacklyd`), dan baris `hint:` untuk setiap error `cli.agent_*` di bawah. Hanya di teks manusia; `ErrorBody` di `--json` tidak berubah.

| `recovery` | Teks |
| --- | --- |
| `start` | `start it with stackly daemon start` |
| `register` | `install it with stackly daemon install` |
| `approve_in_login_items`, `enable_in_login_items` | `turn on Stackly in System Settings > General > Login Items & Extensions` |
| `move_or_reinstall_app` | `move Stackly to the Applications folder or install it again` |
| `use_bundled_cli` | `use the stackly inside Stackly.app (<path canonical bila ada>), or start stacklyd yourself` |
| `manual_daemon_for_override` | `start stacklyd yourself for STACKLY_ROOT` |
| `build_with_xtask` | `build the app with cargo xtask build-app` |
| `check_again` | `try again; if it keeps failing, check the background service in Stackly's Diagnostics` |

Kode error saat `install`, `start`, atau `restart` (untuk daemon yang tidak jalan) berhenti karena state: `not_registered` `cli.agent_not_registered` (6), `requires_approval` `cli.agent_requires_approval` (4), `disabled` `cli.agent_disabled` (4), `not_found` `cli.agent_not_found` (6), `unavailable` alasan `root_override` `cli.agent_root_override` (2), `not_bundled` `cli.agent_not_bundled` (5), `helper_missing` `cli.agent_helper_missing` (6), dan `unsupported_os`, `status_failed`, atau `status_unrecognized` `cli.agent_status_unavailable` (6). Kegagalan panggilan OS di tengah aksi mengikuti tabel *Hasil pendaftaran*, atau `cli.agent_call_timeout` (8).

Kode warning baru (daftar tetap spec 0008): `cli.agent_bundle_mismatch` (`info.launch.bundlePath` berbeda dari bundle CLI), `cli.daemon_not_running` (`install` berhasil tetapi daemon belum menjawab dalam 30 detik), `cli.daemon_relaunched` (`stop` melihat `serverInstanceId` baru, karena daemon keluar dengan error dan launchd menyalakannya lagi).

### Kode error baru

Semua `retryable` false. Kode `cli.*` mengikuti aturan area `cli` spec 0008: hanya dibuat `stackly-cli`, tidak pernah lewat wire, dan tidak ikut digest kontrak. Tidak ada area error baru, jadi tabel area spec 0006 dan aturan spec 0008 "CLI hanya membuat error di area `cli`" tidak berubah. GUI tidak memakai kode ini; ia menampilkan state dan `AgentActionError` (lihat *GUI*).

| Kode | Kategori | Kapan | Kunci `details` |
| --- | --- | --- | --- |
| `daemon.restart_unavailable` | `unsupported` | `restart` ke daemon `manual` (dikirim daemon) | `launchContext` |
| `cli.agent_root_override` | `invalid_input` | perintah agent dengan `STACKLY_ROOT` aktif | `variable` |
| `cli.agent_not_bundled` | `unsupported` | CLI di luar `Stackly.app`, atau bundle OS berbeda dari bundle CLI | `executable` |
| `cli.agent_helper_missing` | `unavailable` | bundle tanpa plist atau `stacklyd` | `bundlePath` |
| `cli.agent_not_registered` | `unavailable` | `start` saat belum terdaftar | `label` |
| `cli.agent_requires_approval` | `unauthorized` | menunggu persetujuan pengguna | `label` |
| `cli.agent_disabled` | `unauthorized` | dimatikan di Login Items setelah pernah diizinkan | `label` |
| `cli.agent_not_found` | `unavailable` | OS tidak menemukan service untuk bundle ini | `label`, `bundlePath` |
| `cli.agent_invalid_signature` | `unauthorized` | OS menolak signature bundle | `label`, `osCode` |
| `cli.agent_os_error` | `failed` | `register`, `unregister`, atau `relaunch` gagal di OS | `op`, `osDomain`, `osCode` |
| `cli.agent_status_unavailable` | `unavailable` | status tidak bisa dibaca atau nilainya tidak dikenal | `op`, `osCode` |
| `cli.agent_call_timeout` | `timeout` | panggilan OS lewat 10 detik | `op`, `seconds` |
| `cli.daemon_wait_timeout` | `timeout` | daemon tidak muncul atau tidak hilang dalam batas | `action`, `seconds` |

### GUI

Modul baru `StacklyUI/Agent/`:

- `AgentIdentifiers` (konstanta, diuji fixture), `BackgroundAgentState`, `UnavailableReason`, `RecoveryStep`, dan `BackgroundAgentClassifier.classify(_:)` (fixture `states.json`).
- `BackgroundAgentControlling` (protocol, `Sendable`): `status() async throws(AgentActionError) -> AgentProbe`, `register() async throws(AgentActionError) -> AgentProbe`, `unregister() async throws(AgentActionError) -> AgentProbe`, `relaunch() async throws(AgentActionError) -> AgentProbe` (unregister lalu register), `openApprovalSettings()`. `AgentProbe` Swift berbentuk sama dengan Rust, termasuk path bundle. `SMAppServiceAgent` adalah satu satunya file yang `import ServiceManagement`; method nya `nonisolated`, berjalan di luar main actor dengan batas 10 detik (hasil yang terlambat dibuang, panggilan OS nya dibiarkan selesai sendiri), memeriksa `Bundle.main.bundleIdentifier`, plist, dan `stacklyd` seperti adapter Rust, lalu memakai `SMAppService.agent(plistName:)`. `NSError` dipetakan dengan tabel *Hasil pendaftaran* yang sama. `FakeBackgroundAgentController` untuk test dan preview.
- `AgentActionError` (enum `Sendable`, `Equatable`): `.os(op:, kind:, domain:, code:)`, `.timedOut(op:)`, `.daemonDidNotStart(seconds:)`, dan `.registrationNotKept`. Teks nya di Diagnostics: "macOS returned {domain} {code} while trying to {op}.", "macOS didn’t answer in time.", "stacklyd didn’t start within {seconds} seconds.", dan "macOS didn’t keep the registration."; `kind` `InvalidSignature` memakai "macOS rejected the signature of this build.". GUI tidak membuat `ErrorBody` untuk agent.
- `LaunchRecordReader`: memeriksa `<home>/Library/Application Support/Stackly/background-agent.json`, dengan `<home>` dari `getpwuid_r(getuid())` seperti spec 0004 (bukan `HOME`); tidak dipakai saat `STACKLY_ROOT` aktif.
- `BackgroundAgentModel` (`@MainActor @Observable`, dimiliki `DaemonModel`): `assessment`, `lastCheckedAt`, `lastError: AgentActionError?`, `isWorking`, `justRegistered`; `refresh()`, `turnOn()`, `openLoginItems()`, `start()`. Satu aksi berjalan dalam satu waktu (tombol nonaktif selama `isWorking`). `refresh()` dipanggil saat launch, saat `ShellRuntime.didBecomeActive()`, saat presence menjadi `unreachable`, setiap 5 detik selama presence `unreachable` dan app aktif, dan setelah setiap aksi. `turnOn()` memanggil `register`; hasil `requiresApproval` memanggil `openApprovalSettings()` sekali. `start()` memanggil `relaunch`, lalu `DaemonModel.reconnect()`, dan menampilkan progress paling lama 30 detik; kalau daemon belum tersambung, `lastError` menjadi `.daemonDidNotStart(seconds: 30)`.

**Banner per state** (presence `unreachable` dengan kode `ipc.endpoint_missing` atau `ipc.connection_refused`; kode lain memakai banner spec 0013). Footer dan `MutationAvailability` tetap seperti spec 0013. Semua aksi `StateAction(mutates: false)`.

| State | Tone | Judul | Pesan | Aksi |
| --- | --- | --- | --- | --- |
| `not_registered` | caution | "Stackly’s background service is off." | "Turn it on so stacklyd can run your instances, even when this window is closed. macOS may ask you to allow it." | Turn On, Open Diagnostics |
| `requires_approval` | caution | "Allow Stackly to run in the background." | "Turn on Stackly in System Settings > General > Login Items & Extensions. Stackly doesn’t change this setting for you." | Open Login Items, Check Again |
| `disabled` | caution | "Stackly’s background service is turned off." | "Turn on Stackly in System Settings > General > Login Items & Extensions to run stacklyd again." | Open Login Items, Check Again |
| `enabled`, kurang dari 15 detik sejak `since` | informative | "stacklyd is starting…" | "Actions are available once it’s ready." | |
| `enabled`, 15 detik atau lebih | caution | "stacklyd isn’t running." | "It was stopped or it exited. Start it again here, or run stackly daemon start in Terminal." | Start stacklyd, Open Diagnostics |
| `not_found` | critical | "Stackly can’t find its background service." | "macOS couldn’t find stacklyd for this copy of Stackly. Move Stackly to the Applications folder, or install it again." | Open Diagnostics |
| `unavailable` `helper_missing` atau `not_bundled` | caution | "This build doesn’t include stacklyd." | "Start stacklyd yourself, or build the app with cargo xtask build-app." | Reconnect, Open Diagnostics |
| `unavailable` `root_override` | caution | "STACKLY_ROOT is set." | "The background service only runs the default Stackly folder. Start stacklyd yourself for this folder." | Reconnect, Open Diagnostics |
| `unavailable` lain | caution | "Stackly couldn’t check its background service." | "macOS didn’t answer ({detail}). Check again in a moment." dengan `{detail}` "timed out" untuk `Failed { code: None }`, "error {code}" untuk kode yang ada, dan "unknown status {value}" untuk `Unrecognized` | Check Again, Open Diagnostics |

- `ConnectionHint` untuk kedua kode itu memakai judul dan pesan baris yang sama; teks `Start it with stacklyd` dan penanda `// provisional: F15` dihapus.
- Seksi Diagnostics `BackgroundAgentDiagnosticsSection` di bawah seksi Connection, kartu `.cardSurface()` dengan baris `LabeledContent`: State, What to do (dari `RecoveryStep`), App (path bundle), Label, Last launched by launchd (`lastLaunchedAt` lewat `DiagnosticsTime`, atau "Never"), Last checked, dan Last error (teks `AgentActionError`, atau "None"). Aksi sesuai state seperti banner, ditambah Check Again dan Copy Details.
- Pengumuman VoiceOver sekali per perubahan `state`: "Background service needs your approval.", "Background service is off.", "Background service is turned off in Login Items.", "Background service is on.", dan "Stackly couldn’t check its background service."
- Seksi Diagnostics `CommandLineToolSection` (`// provisional: F16` dan `F32`) memakai `CommandLineTool` di bawah: Status, Command (`~/.local/bin/stackly`), Points to, PATH note dengan baris `export PATH="$HOME/.local/bin:$PATH"` yang bisa disalin; aksi Install, Repair, dan Remove, masing masing lewat konfirmasi spec 0012 yang menyebut file yang akan dibuat atau dihapus dan bahwa PATH serta file shell tidak diubah.

### Instalasi CLI

`CommandLineTool` (`StacklyUI/Agent/CommandLineTool.swift`, satu satunya file `StacklyUI` yang memakai `createSymbolicLink`, `createDirectory`, `rename`, atau `removeItem`, dijaga test aturan sumber). Home dari `getpwuid_r` dan bisa disuntik untuk test. Penggantian link memakai `rename(2)` dari Darwin, karena `FileManager.moveItem` gagal kalau tujuan sudah ada.

- **Link terkelola**: `~/.local/bin/stackly` menunjuk ke `<bundle>/Contents/MacOS/stackly`, dan `~/.local/share/man/man1/<halaman>` menunjuk ke `<bundle>/Contents/Resources/man/man1/<halaman>` untuk setiap halaman di bundle. Sebuah path adalah link terkelola kalau ia symlink yang target nya berakhiran `.app/Contents/MacOS/stackly` atau `.app/Contents/Resources/man/man1/stackly*.1`.
- **Status** (dihitung dari link CLI; halaman man ikut dilaporkan per file): `installed` (link terkelola ke bundle yang sedang berjalan), `stale` (link terkelola ke bundle lain), `broken` (link terkelola yang target nya tidak ada), `conflict` (file, folder, atau symlink lain di path itu), `not_installed`.
- **Install dan Repair**: buat `~/.local/bin` dan `~/.local/share/man/man1` dengan mode 0755 kalau belum ada; folder yang sudah ada (atau symlink ke folder) harus milik user dan tidak bisa ditulis group atau other, selain itu gagal tanpa menulis apa pun. Setiap link ditulis sebagai symlink sementara di folder yang sama lalu di rename, sehingga link lama terganti atomik. Path `conflict` tidak pernah ditimpa dan dilaporkan dengan isi nya. Link man yang tidak lagi punya halaman di bundle (link terkelola yang rusak) dihapus.
- **Remove**: hapus hanya link terkelola; folder tidak dihapus.
- **PATH**: GUI tidak bisa membaca PATH shell pengguna, jadi ia tidak pernah mengklaim `stackly` sudah ada di PATH. Ia selalu menampilkan baris `export` untuk disalin dan path lengkap `<bundle>/Contents/MacOS/stackly` sebagai alternatif. Tidak ada file shell, `/etc/paths`, `/etc/paths.d`, atau `/usr/local/bin` yang disentuh, dan tidak ada permintaan hak admin.

Mockup UI menulis "Install stackly in /usr/local/bin, requires your password once"; perilaku itu tidak dipakai (scope dan spec menang atas mockup), dan fitur 16 menyesuaikan teks langkah Command Line.

### Keychain setelah signing (jawaban spec 0011)

Tetap login Keychain lewat `security-framework` seperti spec 0011; data protection keychain tidak dipakai di baseline. Item legacy dipercaya lewat designated requirement `stacklyd`; selama ad hoc requirement itu berubah setiap build, dan dengan identity Apple Development requirement nya stabil antar build. Perpindahan pertama (ad hoc ke Apple Development) dan kedua (Apple Development ke Developer ID di fitur 60) masing masing butuh satu persetujuan per item lewat aksi pengguna, atau penulisan ulang lewat `stackly secret set`. Tidak ada migrasi otomatis dan tidak ada ACL yang dilonggarkan. AC-18 merekam dialog yang benar benar muncul (Always Allow atau permintaan password keychain).

### Verifikasi tanda tangan peer (jawaban spec 0006 dan 0013)

Ditunda ke fitur 60, dengan review di fitur 63. Tanpa Team ID, requirement yang bisa diperiksa client hanya cdhash yang berubah setiap build, jadi verifikasi tidak memberi perlindungan nyata sekarang, dan batas BR-012 (satu akun user) tetap batas yang diklaim. Rancangan yang dicatat untuk nanti: client membaca audit token peer lewat `getsockopt(LOCAL_PEERTOKEN)`, membuat `SecCode` lewat `SecCodeCopyGuestWithAttributes` dengan `kSecGuestAttributeAudit`, lalu `SecCodeCheckValidity` dengan requirement `anchor apple generic and certificate leaf[subject.OU] = "<Team ID client itu sendiri>" and identifier "com.ojiepermana.stackly.daemon"`, hanya kalau client sendiri ditandatangani dengan Team ID.

### Peta crate dan modul

| Crate atau tempat | Dependency langsung baru | Isi baru |
| --- | --- | --- |
| `stackly-platform` | tidak ada | `bundle` (konstanta, `AppBundle`), `agent` (trait, tipe, `UnsupportedBackgroundAgent`, `classify`, `LaunchContext`), `DirectoryLayout::agent_launch_record()`, `Platform::with_background_agent` |
| `stackly-platform-macos` | `objc2`, `objc2-foundation`, `objc2-service-management` (versi caret terbaru saat build, dicatat di `Cargo.lock`) | `agent` (`MacosBackgroundAgent`, peta *Hasil pendaftaran* yang diuji terhadap konstanta `SMErrors.h`); `info` melaporkan `background_agent` supported |
| `stackly-testkit` | tidak ada | `FakeBackgroundAgent`, `LaunchAgentFixture` (plist sementara dari golden file produksi, bootstrap dan bootout `gui/<uid>`, guard drop) |
| `stackly-protocol` | tidak ada | `DaemonLaunch`, `LaunchContextKind`, `ShutdownParams`, `ShutdownMode`, `ShutdownAccepted`, nilai `ShutdownReason::RestartRequested` (`restart_requested`), method `daemon.shutdown`, kode `daemon.restart_unavailable`, dan kode `cli.*` baru di `codes::cli` |
| `stacklyd` | tidak ada | `StartConfig` dengan `xpc_service_name`, `parent_pid`, `current_exe`; `DaemonDeps` dengan `launch` dan `bundle`; `DaemonHandle::stop_requested()`; handler shutdown; launch record; exit 75 |
| `stackly-cli` | `clap_complete`, `clap_mangen` | modul di *Perintah CLI*; runtime tokio yang dimatikan dengan `shutdown_timeout(Duration::ZERO)`; `CLI_SCHEMA_VERSION` 2 |
| `xtask` | tidak ada | `build_app/` (`assemble.rs`, `sign.rs`, `install.rs`, `plist.rs`), aturan `check-deps` baru, langkah pencabutan agent test di `runtime spike --cleanup` |
| `StacklyUI` | framework `ServiceManagement` | modul `Agent/`, seksi Diagnostics, baris banner |
| `protocols/fixtures/` | | `bundle/identifiers.json`, `agent/states.json`, fixture `daemon.shutdown` dan `DaemonInfo` dengan `launch` |

### API surface

| Permukaan | Bentuk | Input utama | Output utama | Siapa | Error utama |
| --- | --- | --- | --- | --- | --- |
| `daemon.shutdown` | method IPC | `mode` (wajib) | `ShutdownAccepted` | client mana pun yang lolos peer uid (BR-012) | `daemon.restart_unavailable`, `daemon.shutting_down`, `request.invalid_params` |
| `daemon.get_info` | method IPC (ada) | | `DaemonInfo` dengan `launch` | sama | seperti spec 0006 |
| `BackgroundAgent::status`, `register`, `unregister`, `relaunch` | port sync | tidak ada | `AgentProbe` | CLI (port), GUI (padanan Swift) | `BackgroundAgentError` |
| `classify` | fungsi murni | `AgentInput` | `AgentAssessment` | CLI, GUI (padanan) | tidak ada |
| `stackly daemon status`, `install`, `start`, `stop`, `restart`, `completion`, `manpage` | CLI | lihat *Perintah CLI* | envelope `--json` v2 | pengguna dan script | lihat *Kode error baru* |
| `cargo xtask build-app` | xtask | `--release`, `--identity`, `--install [<dir>]` | `Stackly.app` | developer dan release owner | build atau signing gagal, exit bukan 0 |
| Tombol GUI Turn On, Open Login Items, Check Again, Start stacklyd, Install, Repair, Remove | aksi pengguna | | state baru | pengguna | `AgentActionError` di Last error |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| semua | label, nama plist, bundle ID, identifier signing | konstanta `stackly-platform::bundle` dan `AgentIdentifiers`, dijaga fixture |
| CLI agent | bundle CLI | `current_exe()` canonical lewat `AppBundle::enclosing` di `main` |
| adapter Rust dan Swift | bundle yang dipakai OS | `NSBundle.mainBundle` (`Bundle.main`), dibawa di `AgentProbe::Registration.bundle` |
| CLI agent | bundle yang dilaporkan | bundle CLI; `Registration` dengan bundle lain diganti `NotBundled` sebelum `classify` |
| `classify` | `registration` | `SMAppService.status` lewat port atau `SMAppServiceAgent` |
| `classify` | `launch_record` | keberadaan `<data root default>/background-agent.json` |
| `classify` | `just_registered` | aksi yang sedang berjalan (CLI) atau `BackgroundAgentModel.justRegistered` (GUI) |
| `classify` | `root_override` | `STACKLY_ROOT` yang sudah dibaca `env.rs` CLI atau `ShellBootstrap` GUI |
| `AgentReport.recovery`, baris `Next`, hint | langkah dan teks nya | `classify` ditambah `start` saat daemon tidak terjangkau; teks dari tabel *Langkah pemulihan CLI* |
| hasil `register`, `unregister`, `relaunch` | kode error dan exit | tabel *Hasil pendaftaran* dari kode `NSError` dan status yang dibaca ulang |
| daemon | `launch.context`, `launch.label` | `LaunchContext::detect` dari `StartConfig.xpc_service_name` dan `StartConfig.parent_pid` |
| daemon | `launch.bundlePath` | `AppBundle::enclosing` dari `StartConfig.current_exe` yang sudah canonical |
| daemon | exit 0 atau 75 | `ShutdownMode` dari `DaemonHandle::stop_requested()` |
| launch record | `firstLaunchedAt`, `lastLaunchedAt` | file lama dan `Clock` daemon |
| launch record | `version` | `daemon_info().version` (versi workspace) |
| `daemon start` | perlu menunggu sebelum `relaunch` | `lastLaunchedAt` launch record kurang dari 15 detik dari jam CLI |
| `daemon start`, `restart` | daemon sudah naik | handshake berhasil (`start`) atau `serverInstanceId` berbeda dari reply `ShutdownAccepted` (`restart`) |
| `daemon stop` | daemon sudah turun | EOF di koneksi yang mengirim `daemon.shutdown`, lalu `ipc.endpoint_missing` dari percobaan connect berikutnya |
| batas waktu | 10, 15, 20, dan 30 detik, polling 250 ms | konstanta `AGENT_CALL_TIMEOUT`, `RECENT_LAUNCH_GRACE`, `DAEMON_STOP_WAIT`, `DAEMON_START_WAIT` (juga untuk restart), `DAEMON_POLL_INTERVAL` di `commands/daemon/wait.rs` |
| GUI | ambang 15 detik, polling 5 detik, tunggu start 30 detik | konstanta `AgentTiming.startGrace`, `AgentTiming.pollInterval`, `AgentTiming.startWait`; `since` dari `ConnectionState.unreachable` spec 0013 |
| GUI | teks Last error | `AgentActionError` |
| GUI | path symlink dan target | home dari `getpwuid_r`, target dari `Bundle.main.bundleURL` |
| `build-app` | versi app | `[workspace.package] version` lewat `cargo metadata` |
| `build-app` | path binary Rust | pesan `compiler-artifact` dari `cargo build --message-format=json` |
| `build-app` | identity signing | `--identity`, `STACKLY_SIGN_IDENTITY`, lalu `-` |
| `build-app` | halaman man | `stackly manpage --json` dari binary yang baru dibangun |
| `--cleanup` | label agent test | `launchctl list`, baris yang labelnya diawali `dev.stackly.test.agent.` |

### Key invariants

- Tidak ada kode produk (daemon, CLI, GUI, library) yang menjalankan `launchctl`, `open`, `osascript`, `codesign`, atau proses lain untuk mengendalikan agent. `launchctl` hanya dipakai `LaunchAgentFixture` di test dan `runtime spike --cleanup` di xtask.
- Pendaftaran hanya berubah lewat `register`, `unregister`, dan `relaunch`, dan ketiganya hanya dipanggil dari aksi pengguna (`daemon install`, `daemon start`, `daemon restart` untuk daemon yang tidak jalan, Turn On, Start stacklyd). State `requires_approval` dan `disabled` tidak pernah memicu `register` atau `relaunch`.
- Stop yang diminta selalu exit 0, dan restart yang diminta selalu exit 75, jadi `KeepAlive { SuccessfulExit: false }` tidak pernah membatalkan stop.
- Agent selalu melayani root default. Root override selalu daemon manual.
- Launch record hanya ditulis daemon berkonteks `launchd` berlabel produksi, tidak pernah oleh CLI atau GUI.
- Bundle tidak pernah memuat paket `testing/` atau `stackly-fake-server`, `Contents/MacOS` hanya berisi tiga executable, dan binary di bundle selalu ditandatangani sebelum app.
- Instalasi CLI tidak pernah menimpa path yang bukan link terkelola dan tidak pernah menyentuh PATH, file shell, atau path global.
- `classify` Rust dan Swift selalu sama untuk setiap baris fixture.
- Tidak ada langkah build atau test tanpa pengawasan yang mendaftarkan agent `SMAppService`, menyalakan atau mematikan Login Items, atau memakai akun macOS lain.

### Security model

- Batasnya tetap satu akun user (BR-012). `daemon.shutdown` bisa dipanggil client mana pun milik user yang sama; itu sama kuatnya dengan sinyal SIGTERM yang sudah bisa dikirim proses user itu, dan tercatat di log dengan pid peer.
- Persetujuan background dikelola macOS. Stackly hanya membaca status, mendaftar saat diminta, dan membuka halaman Login Items; ia tidak pernah mencoba melewati atau menyalakan izin.
- Hardened runtime di ketiga binary tanpa entitlement; App Sandbox tetap mati (spec 0001) dan tidak ada setting yang menurun ke child. Job launchd tanpa environment tambahan, jadi daemon tidak mewarisi variabel shell.
- Binary helper dan CLI berada di dalam bundle yang ditandatangani; selama ad hoc, seal hanya membuktikan "tidak berubah sejak build lokal ini". Pemeriksaan seal saat runtime dan verifikasi tanda tangan peer ditunda ke fitur 60 dan 63.
- Symlink CLI hanya di bawah home pengguna, folder induk wajib milik user tanpa izin tulis group atau other, dan path yang bukan milik Stackly tidak pernah disentuh.
- Panggilan `objc2` adalah permukaan `unsafe` baru di `stackly-platform-macos`, diaudit di fitur 63.

### Configuration required

- `STACKLY_SIGN_IDENTITY` (xtask saja, opsional): nama atau hash SHA-1 identity code signing untuk `build-app`; kosong berarti ad hoc. Tidak ada variabel baru untuk daemon, CLI, atau GUI. `XPC_SERVICE_NAME` diset launchd, bukan pengguna.
- Prasyarat di luar repo untuk milestone 4: identity Apple Development nyata di login Keychain Mac ini (OQ-012). Akun Apple ID gratis dengan Personal Team di Xcode biasanya cukup untuk sertifikat Apple Development lokal; Developer ID dan notarization butuh Apple Developer Program dan milik fitur 60. Keputusan akun ada di release owner.
- Prasyarat untuk AC-19: akun macOS kedua yang bukan developer, dibuat pengguna dengan hak admin.

### Critical test scenarios

- Bundle: build Debug dan Release menghasilkan isi persis (tanpa `Stackly.debug.dylib`), plist sama dengan golden, tanpa `testing` atau `stackly-fake-server`, versi sama di tiga tempat; langkah catalog yang sengaja gagal tidak meninggalkan bundle di path keluaran. Memverifikasi **AC-1**.
- Signing ad hoc: `codesign --verify --strict --deep` lulus dan output menyebut `ad hoc` untuk ketiga binary dengan identifier yang benar. Memverifikasi **AC-2**.
- Install development: folder tujuan sementara dengan bundle asing ditolak tanpa berubah; bundle Stackly lama terganti tanpa sisa folder staging. Memverifikasi **AC-22**.
- Port dan capability: `FakePlatform` memakai `FakeBackgroundAgent`, `UnsupportedPlatform` not supported, `MacosPlatform` supported; `check-deps` menolak crate fixture yang bergantung pada `objc2-service-management` atau `clap_mangen` di tempat salah; peta *Hasil pendaftaran* cocok dengan konstanta header. Memverifikasi **AC-3**.
- Fixture state: setiap baris `states.json` lulus di Rust dan Swift, termasuk `requiresApproval` dengan launch record dan `justRegistered`, dan `Failed` tanpa kode. Memverifikasi **AC-4**.
- Launch: test dalam proses dengan `DaemonDeps.launch` berisi konteks `launchd` palsu menulis launch record, konteks `manual` dan root override tidak menulis; tulis yang gagal lewat `FakeFilesystem` tetap start. Memverifikasi **AC-5**.
- Shutdown: daemon dalam proses menerima `stop` (`stop_requested()` memberi `stop`, exit 0), `restart` dengan konteks `launchd` (exit 75), `restart` manual (error, daemon tetap jalan), request kedua saat berhenti, dan mode recovery. Memverifikasi **AC-6**.
- CLI dengan fake: tabel semua state dan setiap baris *Hasil pendaftaran* untuk `install`, `start`, `restart`, dan `status`, dengan log panggilan `FakeBackgroundAgent`; tunggu sebelum `relaunch` saat `lastLaunchedAt` baru; bundle berbeda menjadi `not_bundled`; hasil parsial `--json` saat gagal; `stop` dan `restart` terhadap `SpawnedDaemon` manual, termasuk `daemon.shutting_down` sebagai "sudah berhenti" dan EOF sebelum socket hilang; golden `--json` v2; hint per `recovery`; teks lama hilang; CLI keluar walau panggilan port fake ditahan lebih lama dari batasnya. Memverifikasi **AC-7** sampai **AC-12**.
- GUI dengan fake: setiap baris *Banner per state*, ambang 15 detik dengan jam yang disuntik, Turn On yang membuka Login Items sekali, Start stacklyd yang memanggil `relaunch` dan menampilkan `daemonDidNotStart` setelah 30 detik, refresh yang tidak pernah mendaftar, aturan sumber `ServiceManagement`. Memverifikasi **AC-11**, **AC-13**.
- Instalasi CLI di home sementara: install, stale ke repair, broken, conflict dengan file biasa dan symlink asing yang tidak berubah, remove, folder induk yang bisa ditulis group ditolak, tidak ada file lain di home yang berubah. Memverifikasi **AC-14**.
- LaunchAgent sementara (setelah persetujuan): restart, stop, crash, dan pengambilalihan lock dengan plist turunan golden; instance fake bertahan di `kickstart -k` dan `bootout`. Memverifikasi **AC-15**, **AC-16**.
- Spike ad hoc (setelah persetujuan) dan alur `SMAppService` nyata, Keychain setelah signing, Team ID, dan bagian NFR-015 dengan akun baru: record manual. Memverifikasi **AC-17** sampai **AC-21**.
- Izin: CLI dan GUI dalam state `requires_approval` atau `disabled` tidak pernah memanggil `register` atau `relaunch`, dibuktikan log panggilan fake. Memverifikasi **AC-8**, **AC-9**, **AC-11**.

### Menjalankan test di Mac ini

- **Tanpa pengawasan** (developer atau sesi build menjalankannya kapan saja): run fixture Rust dan Swift lewat wrapper spec 0002, termasuk semua test logika CLI dan GUI dengan fake, test `build-app` dan `--install` ke folder sementara, dan signing ad hoc. Tidak satu pun dari test ini mendaftarkan agent, membuka System Settings, atau menyentuh Login Items.
- **Setelah persetujuan kamu lewat lead** (run native yang kamu mulai, seperti spike spec 0003): test LaunchAgent sementara di `apps/daemon/tests/native_launchd.rs` (`#[ignore = "native: launchd gui domain"]`, `Kind::Native`). Mereka butuh sesi login GUI; tanpa sesi itu `BLOCKED`. Label agent `dev.stackly.test.agent.<pid>-<n>`, plist di `TestRoot` (tidak pernah di `~/Library/LaunchAgents`), diturunkan dari golden file dengan `ProgramArguments` ke binary `stacklyd` hasil `cargo_bin` dan `EnvironmentVariables` hanya `STACKLY_ROOT` milik `TestRoot`. `cargo xtask runtime spike --cleanup` mendapat langkah tambahan yang tidak butuh lock spike: baca `launchctl list`, lalu `launchctl bootout gui/<uid>/<label>` untuk setiap label yang diawali `dev.stackly.test.agent.`; langkah itu juga berjalan walau spike sedang dikunci.
- **Setelah persetujuan kamu, manual** (mendaftarkan agent `SMAppService` sungguhan atau mengubah Login Items): spike ad hoc AC-21 di langkah 0, dan record AC-17, AC-18, AC-19, AC-20. Semuanya record manual lewat `cargo xtask evidence new --kind native`, dengan bukti (output `codesign`, `stackly daemon status --json`, tangkapan layar System Settings dan banner) di `evidence/manual/files/<id>/`. Selama Mac ini tanpa identity, AC-17, AC-18, AC-19, dan AC-20 `BLOCKED` dengan reason `no code signing identity on this Mac (OQ-012)` dan owner `release owner (OQ-012)`; AC-19 juga `BLOCKED` dengan reason `no second macOS user account on this Mac` dan owner `user` sampai akun itu ada. Setelah spike, pendaftaran ad hoc dicabut dengan `unregister` dan dicatat.
- Evidence `covers` selalu memakai ID PRD utuh (misalnya `REQ-014`, `AC-014`, `NFR-015`), bukan bagian dari ID.
- Tidak ada test yang menjalankan `sfltool` atau mengubah System Settings secara otomatis; langkah System Settings selalu manual dan dicatat.

### Jawaban untuk Follow-up spec lain

| Spec sumber | Follow-up | Jawaban di spec ini |
| --- | --- | --- |
| 0004 | port fitur 15 lewat `with_<port>`, capability supported | *Port `BackgroundAgent`*, diterapkan langsung di spec 0004 (additive) |
| 0006 | tanda tangan `stacklyd` lewat audit token; `stackly daemon start` | *Verifikasi tanda tangan peer* (ditunda ke fitur 60 dan 63); *Perintah CLI* dan *Langkah pemulihan CLI* |
| 0008 | allowlist `check-deps` untuk `launchctl`; completion; man page; hint | tidak butuh allowlist karena CLI tidak menjalankan proses (AC-11 spec 0008 tetap); AC-12 |
| 0009 | jangan membundel `testing/` | *Build, assembly, dan signing* langkah 4, AC-1 |
| 0010 | survival di LaunchAgent nyata | AC-16 |
| 0011 | Keychain setelah signing; data protection keychain | *Keychain setelah signing*, AC-18 |
| 0013 | alur start dari GUI; audit token di client Swift | *GUI* (Start stacklyd), AC-13; *Verifikasi tanda tangan peer* |

### Usulan update untuk spec lain

Spec ini tidak mengedit spec 0001, 0002, 0003, 0006, 0008, atau 0013. Teks berikut adalah usulan yang diterapkan pemilik masing masing lewat `/architect` update atau `/sync`, sebaiknya di commit yang sama dengan langkah build yang mengubah perilakunya.

| Spec | Bagian | Usulan |
| --- | --- | --- |
| 0008 | *Kontrak `--json`*, field `schemaVersion` ("sekarang 1") dan tabel hasil per perintah | `CLI_SCHEMA_VERSION` menjadi 2 saat langkah 6 spec ini masuk; baris `daemon status` menjadi `{ info: DaemonInfo \| null, health: DaemonHealth \| null, client \| null, agent: AgentReport }` dengan hasil parsial saat daemon tidak terjangkau; tambah baris `daemon install`, `start`, `stop`, `restart`, `completion`, dan `manpage` dari tabel *Perintah CLI* spec ini |
| 0008 | AC-6 | daemon yang tidak jalan tetap exit 6, tetapi `result` berisi `agent` (dan `info`, `health`, `client` null) bersama `error` |
| 0008 | *Kode error area `cli`* dan daftar kode warning | tambah semua kode `cli.agent_*`, `cli.daemon_wait_timeout`, dan warning `cli.agent_bundle_mismatch`, `cli.daemon_not_running`, `cli.daemon_relaunched` |
| 0008 | *Koneksi ke daemon* dan *Output manusia* | ganti hint `start it with stacklyd` dengan tabel *Langkah pemulihan CLI* spec ini |
| 0008 | *Batas dengan spec lain* | baris fitur 15 menunjuk spec 0015 |
| 0006 | *Daemon*, tabel exit code `stacklyd` | tambah 75 "berhenti untuk restart (`daemon.shutdown` mode `restart`)" |
| 0006 | *Event dan cursor*, `ShutdownReason` | tambah `restart_requested`; `requested` dipakai untuk `daemon.shutdown` mode `stop` |
| 0006 | *API surface* dan tabel method | tambah `daemon.shutdown` (`recovery allowed`) dan field `DaemonInfo.launch` |
| 0006 | *CLI `stackly daemon status`* | hint `start it with stacklyd` diganti, sesuai baris 0008 di atas |
| 0013 | *Tampilan status daemon* dan *Diagnostics koneksi* | baris banner per state agent untuk `ipc.endpoint_missing` dan `ipc.connection_refused`, seksi `BackgroundAgentDiagnosticsSection` dan `CommandLineToolSection`, dan `ConnectionHint` tanpa `// provisional: F15` |
| 0013 | test aturan sumber | `import ServiceManagement` hanya di `SMAppServiceAgent.swift`; API tulis file hanya di `CommandLineTool.swift` |
| 0002 dan 0003 | *Menjalankan test di Mac ini* dan *Mekanisme LaunchAgent dan pembersihan* | `runtime spike --cleanup` juga mencabut job `dev.stackly.test.agent.*` tanpa lock spike |
| 0001 | *Batas keputusan* dan *Peta crate* | baris fitur 15 diputuskan di spec 0015; catat `objc2`, `objc2-foundation`, `objc2-service-management` (`stackly-platform-macos` saja), `clap_complete`, dan `clap_mangen` (`stackly-cli` saja) |

## Build plan

Urutan mengikuti pendekatan Journey: jalur "daemon terpasang di bundle, dikendalikan lewat CLI, terlihat di GUI" dibuat lengkap tanpa signing lebih dulu, lalu bukti yang butuh identity nyata dikumpulkan di milestone terakhir. Fakta macOS yang paling berisiko diuji lebih dulu lewat spike di langkah 0. Milestone 1 butuh fitur 8 (kerangka CLI, `01-foundation.md`) dibangun; milestone 2 butuh fitur 10 dan 14 (`01-foundation.md`); milestone 3 butuh milestone "Wiring, instances, and operations" fitur 13 dan komponen fitur 12 (`ui-macos.md`); milestone 4 butuh identity nyata. Langkah yang ditandai **setelah persetujuan** mendaftarkan job launchd atau agent di Mac ini, mengubah Login Items, atau memakai akun macOS kedua; sesi build menunggu persetujuan kamu lewat lead untuk langkah itu dan tidak menjalankannya tanpa pengawasan.

**Milestone 1: Bundle, port, shutdown, dan CLI tanpa signing**

0. **Setelah persetujuan, manual**: spike ad hoc AC-21 dengan bundle sementara yang dirakit tangan di folder scratch (app hasil `xcodebuild` ditambah `stacklyd` dan plist) dan tool kecil sementara di `Contents/MacOS` yang memanggil `SMAppService`; kode spike tidak masuk repo, hasil dicatat di rationale dan record manual, dan pendaftaran dicabut di akhir. Memenuhi **AC-21**.
1. `stackly-platform::bundle` (konstanta, `AppBundle`) dan fixture `protocols/fixtures/bundle/identifiers.json` dengan test Rust. Memenuhi **AC-1**.
2. Port `BackgroundAgent`: trait (`status`, `register`, `unregister`, `relaunch`) dan tipe, `UnsupportedBackgroundAgent`, `Platform::with_background_agent`, `classify`, `LaunchContext::detect`, `DirectoryLayout::agent_launch_record()`, `FakeBackgroundAgent` dan pemasangannya di `FakePlatform`, fixture `protocols/fixtures/agent/states.json` dengan test Rust. Memenuhi **AC-3**, **AC-4**, **AC-5**.
3. `stackly-protocol`: `daemon.shutdown` beserta tipenya, `DaemonLaunch`, nilai `restart_requested`, kode `daemon.restart_unavailable`, kode `cli.*` baru, fixture, `cargo xtask codegen`, dan naik satu minor `ProtocolVersion::CURRENT`. Memenuhi **AC-6**.
4. `stacklyd`: `StartConfig` dan `DaemonDeps` baru, konteks launch, `DaemonInfo.launch`, launch record, handler `daemon.shutdown`, `DaemonHandle::stop_requested()`, exit 75, dan test dalam proses serta binary. Memenuhi **AC-5**, **AC-6**.
5. `MacosBackgroundAgent` dengan `objc2-service-management` dan peta *Hasil pendaftaran*, pemasangan di `MacosPlatform`, capability supported, aturan `check-deps` untuk `objc2*`, `clap_complete`, dan `clap_mangen`, plus integration test yang hanya memeriksa jalur `NotBundled` (binary test di luar bundle, tanpa mendaftarkan apa pun). Memenuhi **AC-3**.
6. CLI: folder `commands/daemon/`, `status` dengan blok agent dan `CLI_SCHEMA_VERSION` 2, `install`, `start`, `stop`, `restart`, `wait.rs`, tabel *Langkah pemulihan CLI*, warning baru, `completion`, `manpage`, runtime dengan `shutdown_timeout`, dan test tabel dengan `FakeBackgroundAgent` serta `SpawnedDaemon`. Memenuhi **AC-7**, **AC-8**, **AC-9**, **AC-10**, **AC-11**, **AC-12**.
7. `cargo xtask build-app`: build Rust, `xcodebuild` tanpa signing dan tanpa debug dylib, assembly (dengan titik langkah catalog untuk fitur 17), cek isi, signing ad hoc dari dalam ke luar, verifikasi, rename atomik, `--identity`, `--install` ke folder sementara di test, golden plist. Memenuhi **AC-1**, **AC-2**, **AC-12**, **AC-22**.

Sesi paralel: langkah 0 berjalan dulu (atau bersamaan dengan langkah 1 sampai 4, yang tidak bergantung pada hasilnya); hasil yang membantah asumsi menahan langkah 5 dan 6 sampai `/architect` update. Langkah 1 dan 3 tidak saling menyentuh dan bisa jalan bersamaan. Langkah 2 setelah langkah 1. Langkah 4 setelah langkah 2 dan 3. Langkah 5 setelah langkah 0 dan 2. Langkah 6 setelah langkah 0, 3, 4, dan 5 serta fitur 8. Langkah 7 setelah langkah 1 dan 6 (butuh `stackly manpage`).

**Milestone 2: launchd nyata tanpa signing (setelah persetujuan)**

8. `LaunchAgentFixture` dari golden plist di testkit, langkah pencabutan agent test di `runtime spike --cleanup`, dan test native restart, stop, crash, dan pengambilalihan lock, dijalankan di run native yang kamu setujui. Memenuhi **AC-15**.
9. Test native survival instance fake di `kickstart -k` dan `bootout` dengan adopsi oleh daemon berikutnya, di run native yang sama. Memenuhi **AC-16**. Kalau gagal, berhenti dan kembali ke `/architect` (spec 0010).

**Milestone 3: GUI dan instalasi CLI**

10. `StacklyUI/Agent/`: identifier, state, classifier dengan fixture Swift, `BackgroundAgentControlling`, `SMAppServiceAgent`, `AgentActionError`, fake, `LaunchRecordReader`, `BackgroundAgentModel`, dan aturan sumber. Memenuhi **AC-4**, **AC-11**.
11. Banner per state, `ConnectionHint`, seksi `BackgroundAgentDiagnosticsSection`, pengumuman VoiceOver, dan cek keyboard serta Dynamic Type. Memenuhi **AC-13**.
12. `CommandLineTool` dan `CommandLineToolSection` dengan konfirmasi, test di home sementara. Memenuhi **AC-14**.

Sesi paralel: langkah 10 sampai 12 (Swift) bisa berjalan bersamaan dengan milestone 2 (Rust) karena tidak menyentuh file yang sama. Langkah 11 dan 12 setelah langkah 10.

**Milestone 4: signing dan persetujuan nyata (butuh identity, `BLOCKED` sampai OQ-012, setiap langkah setelah persetujuan)**

13. Build dengan identity Apple Development nyata dan bukti Team ID serta designated requirement. Memenuhi **AC-20**.
14. Alur `SMAppService` nyata dengan cabang A atau B, penolakan sungguhan di System Settings, `disabled`, dan survival saat izin dimatikan. Memenuhi **AC-17**.
15. Bukti Keychain setelah signing. Memenuhi **AC-18**.
16. Bagian NFR-015 di akun macOS kedua: clean install, approval denied, reinstall, upgrade, dan login. Memenuhi **AC-19** (juga butuh akun itu, owner `user`).

## Consequences

**Positive**:

- Tidak ada `launchctl`, `open`, atau proses lain di kode produk, jadi aturan "CLI dan GUI tidak menjalankan proses" (spec 0001, 0008, 0013) tetap utuh tanpa pengecualian.
- Persetujuan background sepenuhnya milik macOS, dan Stackly tidak pernah mengubahnya tanpa aksi pengguna, sesuai REQ-014 dan PRD bagian 25.
- Stop dan restart memakai jalur berhenti yang sudah teruji di spec 0006, sehingga perilaku daemon sama di bawah launchd, dari Terminal, dan di test.
- Bundle, plist, dan signing dibuat satu perintah sejak sekarang, jadi masalah packaging muncul di fase ini (RISK-10), dan fitur 60 hanya menambah Developer ID dan notarization.
- Semua logika keputusan (state, hint, perintah) teruji dengan fake dan fixture tanpa signing, dan bagian yang tidak bisa diuji jelas tercatat `BLOCKED`.
- Fakta OS yang paling berisiko diuji spike di awal, sehingga cara start bisa diganti di adapter (`relaunch`) sebelum CLI dan GUI dibangun di atasnya.

**Negative / tradeoffs**:

- Beberapa perilaku OS belum terkonfirmasi sumber resmi dan bertumpu pada spike dan test native: `relaunch` (unregister lalu register) sebagai cara start tanpa persetujuan baru, status setelah izin dimatikan, `XPC_SERVICE_NAME` sama dengan label, `SMAppService` dari CLI di dalam bundle (termasuk lewat symlink), dan apakah build ad hoc bisa didaftarkan sama sekali. Kalau salah satunya gagal, spec ini kembali ke `/architect` untuk bagian itu.
- `disabled` adalah turunan dari launch record, bukan status OS. Pendaftaran baru setelah pernah diizinkan bisa tampil `disabled`; teks dan langkah pemulihannya netral (Login Items), jadi salah label nya tidak berbahaya, tetapi tetap tidak sepresisi status OS.
- `relaunch` bisa memunculkan notifikasi "Background Items Added" lagi dari macOS.
- **Konsekuensi untuk spec 0008 (fitur 8 masih dibangun program foundation)**: `CLI_SCHEMA_VERSION` naik ke 2 saat langkah 6 spec ini masuk, jadi golden file setiap perintah yang sudah ada ikut berubah di commit itu, dan teks spec 0008 perlu update di *Usulan update untuk spec lain*. Kalau fitur 8 masih terbuka saat itu, kedua build perlu berurutan supaya golden file tidak bentrok.
- Kegagalan start yang menetap di bawah launchd (misalnya root yang izinnya terlalu terbuka) diulang launchd setiap 10 detik tanpa sebab yang terlihat di GUI sampai doctor fitur 34; yang terlihat hanya "stacklyd isn’t running".
- Saat daemon dihentikan atau izin dimatikan, instance tetap hidup tetapi tidak diawasi (tanpa restart policy dan health) sampai daemon kembali.
- Build dari Xcode IDE tidak memuat helper, jadi alur agent hanya bisa dicoba dari `cargo xtask build-app --install`.
- `~/.local/bin` tidak ada di PATH default macOS, jadi pengguna tetap harus menambahkan satu baris sendiri.
- Klasifikasi state ditulis dua kali (Rust dan Swift); fixture bersama menjaga kesamaannya, tetapi perubahan selalu menyentuh dua tempat.
- Binding `objc2` menambah dependency dan blok `unsafe` di `stackly-platform-macos`.
- Beberapa langkah tidak bisa dijalankan tanpa kehadiran kamu (spike, run native LaunchAgent, milestone 4), jadi build fitur ini tidak bisa selesai sepenuhnya oleh sesi tanpa pengawasan.

**Neutral**:

- Exit code daemon bertambah 75, dan `stacklyd` mendapat satu method dan satu field `DaemonInfo`; protokol naik satu minor.
- `~/Applications/Stackly.app` menjadi lokasi development yang disarankan, supaya path yang dicatat macOS tidak hilang saat `target/` dibersihkan.
- Tidak ada area error baru; semua kode yang dibuat CLI tetap di area `cli`.
- Path keluaran `build-app` pindah dari `target/xcode/Build/Products/<Config>/` ke `target/app/<Config>/`.

## Follow-up

- [ ] Terapkan *Usulan update untuk spec lain* (spec 0001, 0002, 0003, 0006, 0008, 0013) lewat pemilik masing masing saat langkah build terkait masuk.
- [x] `docs/scope/ui-macos.md`: tambahkan selisih mockup yang sudah diketahui untuk onboarding: Command Line bukan `/usr/local/bin` dengan password, dan Background Agent tidak berjalan "only while the app is open" tanpa izin. Fitur 16 menyesuaikan teks nya. Diterapkan di `ui-macos.md` (2026-10-20).
- [ ] Fitur 16: langkah Background Agent memakai `BackgroundAgentModel` dan aksi nya; langkah Command Line memakai `CommandLineTool`.
- [ ] Fitur 31: setting autostart daemon dan aksi mematikan background service di atas `register` dan `unregister`, termasuk perilaku saat logout.
- [ ] Fitur 32: baris background startup dan command line tool di Settings memakai model yang sama; `CommandLineToolSection` di Diagnostics dipindah atau dipertahankan.
- [ ] Fitur 34: doctor membaca status job launchd dan menampilkan sebab kegagalan start yang diulang launchd.
- [ ] Fitur 60: Developer ID, timestamp aman, notarization, persetujuan ulang item Keychain, dan verifikasi tanda tangan peer lewat audit token (rancangan di spec ini), bersama review fitur 63.
- [ ] Fitur 61: update app mengganti bundle, memuat ulang agent (`relaunch`), dan menjaga identitas BTM saat identifier final OQ-001 berlaku.
- [ ] Fitur 62: uninstall melepas agent lewat `unregister`, menghapus link terkelola CLI dan man, dan menjelaskan launch record yang tertinggal.
- [ ] Fitur 63: audit blok `unsafe` `objc2` dan risiko `daemon.shutdown` dari proses user yang sama.
- [ ] OQ-001: ganti identifier sementara sebelum rilis publik lewat fixture dan dua sumber konstanta.
- [ ] OQ-012: release owner menyediakan identity Apple Development di Mac ini (Personal Team Apple ID gratis kemungkinan cukup untuk development) untuk membuka milestone 4.
- [ ] Agent Skill atau MCP server untuk `SMAppService`, launchd, atau `objc2` belum dicari (ditunda): skill yang cocok akan membantu adapter dan test native milestone 1 dan 2.
- [ ] `/sync` setelah build: catat flag `build-app` baru, path `target/app/`, `STACKLY_SIGN_IDENTITY`, `~/Applications/Stackly.app`, `stackly completion`, dan skill `signing-entitlements` di `AGENTS.md` dan `apps/macos/AGENTS.md`.

## Rationale

Konteks, opsi per keputusan, alasan, keputusan kecil, hasil cek web, cross check, dan referensi ada di [rationale.md](rationale.md).
