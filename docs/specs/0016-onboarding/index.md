# 0016. Onboarding

**Date**: 2026-10-20
**Status**: Proposed

## Summary

Spec ini menetapkan onboarding Stackly: lima langkah (Welcome, Background Agent, Storage, Catalog Trust, Command Line) yang tampil di jendela utama saat aplikasi pertama kali dibuka, menggantikan shell sampai pengguna menekan Get Started atau melewatinya dengan Escape. Onboarding hanya membaca: ia tidak pernah mendaftarkan background agent, memasang CLI, atau mengirim mutasi ke daemon kecuali pengguna menekan tombol aksi di dalam langkahnya, dan tombol itu memakai model dan aksi yang sama dengan Diagnostics (spec 0015). Teksnya jujur soal isolasi (instance terpisah per folder, port, dan secret, tetapi berjalan sebagai user kamu tanpa sandbox), tidak menjanjikan fitur yang belum ada di build itu, dan bisa dibuka lagi dari menu Help tanpa mengubah state apa pun. Yang diingat hanya satu preferensi GUI, apakah onboarding sudah selesai, di `UserDefaults` aplikasi, bukan di daemon.

## Requirements

**User stories**:

- Sebagai pengguna yang baru membuka Stackly, saya ingin satu alur singkat yang menjelaskan apa itu Stackly, izin background, lokasi data, trust catalog, dan CLI, tanpa akun cloud, supaya saya tahu apa yang akan terjadi di Mac saya sebelum memakai apa pun.
- Sebagai pengguna yang belum memberi izin background, saya ingin melihat state izin yang sebenarnya beserta langkah pemulihannya, dan bisa lanjut dengan Not Now tanpa ada yang diubah diam diam, supaya keputusan izin tetap di tangan saya.
- Sebagai pengguna yang sudah paham, saya ingin melewati onboarding kapan saja dan membukanya lagi nanti tanpa mengubah daemon, instance, atau CLI, supaya onboarding tidak pernah menjadi penghalang atau sumber efek samping.
- Sebagai pengguna yang peduli keamanan, saya ingin teks yang tidak melebih lebihkan isolasi dan trust, supaya saya tidak mengira provider native disandbox atau tidak bisa membaca file user saya (AC-061).
- Sebagai pengguna keyboard dan VoiceOver, saya ingin semua langkah dan aksi bisa dipakai tanpa mouse dan diumumkan dengan jelas, supaya onboarding sama bergunanya untuk saya.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-061):

- **AC-1**: Saat app dibuka dan `OnboardingRecordStore` tidak menyimpan `onboarding.finished` bernilai true, jendela utama menampilkan onboarding di langkah 1 sebagai seluruh isi jendela: tanpa sidebar, toolbar, judul jendela yang terlihat, footer daemon, atau banner daemon. Saat nilainya true, jendela utama menampilkan shell spec 0013 seperti biasa. Nilai itu hanya ditulis true oleh Get Started dan oleh lewati (AC-3), tidak pernah oleh menutup jendela, Quit, atau navigasi antar langkah. Selama satu peluncuran, menutup lalu membuka lagi jendela utama saat onboarding belum selesai menampilkan onboarding di langkah yang sama; peluncuran berikutnya mulai lagi dari langkah 1. Tidak ada bagian onboarding yang menyimpan state ke daemon, registry, atau file di root data (PAGE-01, JOURNEY-01).
- **AC-2**: Onboarding punya tepat lima langkah berurutan dengan label `Welcome`, `Background Agent`, `Storage`, `Catalog Trust`, dan `Command Line`. Footer menampilkan lima titik langkah (titik aktif lebih lebar), teks `Step {n} of 5 · {label}`, tombol Back mulai langkah 2, dan satu tombol utama dengan label: `Get Started` di langkah 5; `Not Now` di langkah 2 selama `BackgroundAgentModel.assessment` bernilai nil atau state nya bukan `enabled`; `Continue` di langkah lain. Continue, Not Now, Back, dan titik langkah hanya memindahkan langkah. Get Started menulis record lalu menampilkan shell. Return menekan tombol utama, dan setiap titik langkah adalah tombol yang pindah langsung ke langkah itu (PAGE-01).
- **AC-3**: Onboarding bisa dilewati dan dibuka lagi tanpa mengubah state daemon. Escape (`onExitCommand`) dan aksi escape VoiceOver di langkah mana pun menutup onboarding dan menulis record yang sama seperti Get Started, tanpa pengumuman "Setup complete." (AC-11). Item menu Help "Welcome to Stackly" membuka jendela utama kalau tertutup dan menampilkan onboarding di langkah 1, atau, kalau onboarding sedang tampil, hanya membawa jendela itu ke depan. Menampilkan, menavigasi, melewati, dan menyelesaikan onboarding tanpa menekan tombol aksi langkah dibuktikan test dengan fake: log `FakeBackgroundAgentController` hanya berisi `status`, home sementara `CommandLineTool` tidak berubah, `MutationPath.perform` tidak pernah dipanggil, dan tidak ada call IPC selain baca yang memang dijalankan `ShellSync`. Setelah onboarding tertutup, shell menampilkan bagian yang tersimpan di `@SceneStorage("shell.section")` tanpa diubah, jadi peluncuran pertama berakhir di Overview (PAGE-01, JOURNEY-01).
- **AC-4**: Langkah Welcome menampilkan ikon app 112 pt, judul "Welcome to Stackly" dengan `.largeTitle.bold()`, subjudul, dan baris fitur dari tabel *Copy* (Isolated instances, Verified runtimes, No account needed), masing masing dengan ikon bertint. Baris "Environments as files" dari mockup tidak tampil sampai fitur 44 menambahkannya. Tidak ada langkah yang meminta akun, login, atau jaringan, dan folder `Onboarding/` tidak memakai `URLSession` atau literal `http://` dan `https://`, dijaga test aturan sumber (PAGE-01, AC-061).
- **AC-5**: Langkah Background Agent menampilkan satu kartu menurut tabel *Kartu Background Agent* untuk setiap kombinasi `assessment`, presence daemon, dan ambang `AgentTiming.startGrace` (15 detik sejak `since` presence `unreachable`), termasuk `StatusLabel` daemon dari footer spec 0013 di setiap baris `enabled`. Aksi kartu hanya memanggil `BackgroundAgentModel.turnOn()`, `openLoginItems()`, `start()`, dan `refresh()`. Muncul nya langkah ini hanya memanggil `refresh()`. Selama `isWorking`, tombol aksi kartu nonaktif dan progress tertunda tampil; `lastError` tampil di bawah kartu dengan teks `AgentActionError` spec 0015. Teks "Without it, the daemon runs only while the app is open." tidak ada di mana pun (REQ-014 lewat spec 0015, PAGE-01, JOURNEY-01).
- **AC-6**: Langkah Storage menampilkan baris Data, Cache, Logs, dan Secrets dengan judul langkah `.title.bold()` dan copy dari tabel *Copy*. Tanpa `STACKLY_ROOT`, nilai Data, Cache, dan Logs diambil dari `DaemonInfo.roots` daemon terakhir yang diketahui kalau ada dan `rootOverride` nya false, selain itu dari `StorageLocations.defaults(home:)` yang mengikuti tabel default macOS spec 0004, dengan awalan home ditulis `~`. Dengan `STACKLY_ROOT`, ketiga baris itu diganti satu baris catatan override, dan nilai variabel itu tidak pernah tampil (spec 0013). Secrets selalu "Login Keychain" (PAGE-01).
- **AC-7**: Langkah Catalog Trust menampilkan satu baris untuk catalog yang dimuat daemon menurut tabel *Baris Catalog Trust*, bersumber dari `CatalogStore.info` (`source`, `trust`, `revision`, `publishedAt`, `outdated`), `CatalogStore.error`, dan `CatalogStore.unsupported` spec 0017, plus catatan trust dari tabel *Copy*. Tidak ada baris atau toggle community, tidak ada teks "Signed manifests", dan tidak ada teks yang menyebut catalog terbaru, up to date, atau tanggal yang lebih baru dari `publishedAt`. Nilai enum `unrecognized` memakai teks netral spec 0017 (PAGE-01, AC-061).
- **AC-8**: Langkah Command Line menampilkan baris "Install stackly in ~/.local/bin" dengan kontrol menurut tabel *Kontrol Command Line* untuk setiap status `CommandLineTool` (`not_installed`, `installed`, `stale`, `broken`, `conflict`) dan untuk bundle tanpa `Contents/MacOS/stackly`. Menyalakan toggle, mematikannya, dan Repair masing masing membuka konfirmasi spec 0012 dari `CommandLineToolPrompts`, dan hanya tombol aksi konfirmasi itu yang memanggil `install`, `remove`, atau `repair`; Cancel tidak mengubah apa pun dan toggle kembali ke status sebenarnya. Status `installed` menampilkan baris PATH `export PATH="$HOME/.local/bin:$PATH"` dengan tombol Copy dan path lengkap `stackly` di bundle. Blok contoh hanya berisi perintah di `OnboardingCopy.commandExamples`. Teks `/usr/local/bin` dan "Requires your password once" tidak ada di mana pun, dan penanda `CommandLineToolSection` menjadi `// provisional: F32` saja (PAGE-01, PRD bagian 25 lewat spec 0015).
- **AC-9**: Semua teks onboarding ada di satu file `OnboardingCopy.swift`, dan test copy memastikan setiap nilai sama persis dengan tabel *Copy*, *Kartu Background Agent*, *Baris Catalog Trust*, dan *Kontrol Command Line*. Test aturan sumber atas folder `Onboarding/` gagal kalau ada frasa "never touching", "sandboxed", "can’t read", "cannot read", "Signed manifests", "only while the app is open", "/usr/local/bin", "password once", "up to date", atau "latest". Langkah `/check verify` membaca ulang semua copy terhadap AC-061 dan mencatat hasilnya (AC-061).
- **AC-10**: Onboarding bisa dipakai penuh dari keyboard: Tab melewati titik langkah, aksi kartu, toggle, tombol Copy, Back, dan tombol utama; Return menekan tombol utama; Escape melewati. Setiap titik langkah punya label VoiceOver "Step {n} of 5, {label}" dan trait selected untuk langkah aktif. Saat langkah berganti, fokus VoiceOver pindah ke judul langkah dan `announce(_:)` mengucapkan "Step {n} of 5, {label}." sekali. Perubahan state kartu Background Agent diumumkan dengan teks spec 0015. Tampilan memakai token, text style, dan permukaan spec 0012 (tanpa glass kustom, ukuran font tetap, atau warna sekunder sistem), terbaca dengan Increase Contrast dan Reduce Transparency, dan tidak terpotong di ukuran jendela minimum 900 × 600 pt (konten digulir kalau perlu) (PAGE-01, NFR-012).
- **AC-11**: Setelah Get Started, `announce(_:)` mengucapkan "Setup complete." sekali, dan kalau komponen umpan balik fitur 67 sudah ada, teks yang sama tampil lewat komponen itu. Lewati dengan Escape tidak mengumumkan apa pun selain perpindahan ke shell (PAGE-01).

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md): onboarding sebagai isi jendela utama yang menggantikan shell, satu nilai `onboarding.finished` di `UserDefaults` app, lewati lewat Escape tanpa tombol baru, buka lagi lewat menu Help, toggle Command Line yang mencerminkan status nyata dan bekerja langsung lewat konfirmasi, satu baris catalog tanpa community, hanya fitur dan perintah yang ada di build itu, teks isolasi di Welcome dan Catalog Trust, path Storage dari daemon dengan default spec 0004 sebagai cadangan, dan ikon app di Welcome.

Onboarding adalah layar baca yang memakai model milik fitur lain: setiap aksi yang mengubah sesuatu adalah tombol yang sama dengan Diagnostics, sehingga tidak ada jalur efek samping baru.

**Implementation skills**: `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang.

## Feature design

### Batas dengan fitur lain

| Bagian | Pemilik |
| --- | --- |
| Lima langkah, model dan record onboarding, `MainWindowRoot`, item menu Help, copy, kontrol Command Line di onboarding, `CommandLineToolPrompts` | spec ini |
| `BackgroundAgentModel`, state agent, `AgentActionError`, `AgentTiming`, `CommandLineTool`, banner dan Diagnostics agent | spec 0015 (dipakai; tambahan kecil di *Usulan untuk spec lain*) |
| `CatalogStore`, `CatalogInfo`, teks nilai asing catalog | spec 0017 (dipakai apa adanya) |
| Jendela utama, `ShellRuntime`, `DaemonModel`, presence, footer, `WindowOpener`, `MainWindowAccessor`, `@SceneStorage("shell.section")` | spec 0013 (dipakai; jendela utama kini bisa berisi onboarding) |
| Token, text style, `StatusLabel`, `DelayedProgress`, konfirmasi, `announce(_:)` | spec 0012 (dipakai apa adanya) |
| Komponen umpan balik setelah aksi, komponen dasar lain | fitur 67 (opsional, dipakai kalau sudah ada) |
| Snapshot dan audit aksesibilitas otomatis | fitur 68 |
| Peta perintah dan shortcut, termasuk "Welcome to Stackly" | fitur 69 (memindahkan item menu ke peta itu) |
| Baris "Show Welcome…" di Settings | fitur 32 |
| Isi Overview setelah Get Started | fitur 22 |
| Baris Environments di Welcome, contoh perintah `env`, `doctor`, dan `instance list` | fitur 44, 46, 34, 22 |
| Sumber catalog community atau lokal | fitur 37 (dan Deferred) |

### Model data

Tidak ada schema, tidak ada DTO baru, dan tidak ada perubahan protokol. Satu preferensi GUI:

| Nama | Tempat | Tipe | Arti |
| --- | --- | --- | --- |
| `onboarding.finished` | `UserDefaults.standard` app `com.ojiepermana.stackly` | Bool, tidak ada berarti false | onboarding sudah ditutup lewat Get Started atau Escape |

- `OnboardingRecordStore` (protocol, `@MainActor`): `var isFinished: Bool { get }`, `func markFinished()`. `UserDefaultsOnboardingRecordStore` untuk produksi, `InMemoryOnboardingRecordStore` untuk test dan preview. Untuk run manual, argumen launch `-onboarding.finished NO` (domain argumen `UserDefaults`) menampilkan onboarding lagi tanpa menghapus apa pun.
- `OnboardingStep` (enum `Int`, `CaseIterable`): `welcome`, `backgroundAgent`, `storage`, `catalogTrust`, `commandLine`, dengan `label` dari tabel *Copy*.
- `OnboardingModel` (`@MainActor @Observable`, dimiliki `ShellRuntime`): `isPresented: Bool`, `step: OnboardingStep`, `next()`, `back()`, `go(to:)`, `present()`, `finish(_ reason: OnboardingFinish)` dengan `OnboardingFinish` `getStarted` atau `skipped`. Dibuat dengan `isPresented = !store.isFinished` dan `step = .welcome`.

### State onboarding

```text
hidden ──present() (menu Help)──▶ presented(welcome)
presented(s) ──next / back / go(to:)──▶ presented(s')        (tanpa efek lain)
presented(s) ──finish(getStarted | skipped)──▶ hidden        (store.markFinished())
presented(s) ──jendela ditutup, dibuka lagi──▶ presented(s)  (record tidak berubah)
launch ──store.isFinished false──▶ presented(welcome)
launch ──store.isFinished true──▶ hidden
```

`present()` saat sudah `presented` tidak mengubah langkah. `finish` saat `hidden` tidak melakukan apa pun.

### Tampilan di jendela utama

- `MainWindowRoot` (`StacklyUI/Onboarding/`) menjadi isi `Window("Stackly", id: WindowOpener.mainWindowID)`: `OnboardingFlow` kalau `onboarding.isPresented`, selain itu `AppShell`, dengan `StacklyMotion.crossfade` spec 0012 (tetap dipakai saat Reduce Motion, sesuai aturan motion spec itu). `MainWindowAccessor`, penangkapan `@Environment(\.openWindow)` ke `WindowOpener`, dan pelaporan `windowWillClose` pindah dari `AppShell` ke `MainWindowRoot`, supaya notice tutup jendela spec 0013 tetap bekerja selama onboarding. `@SceneStorage("shell.section")` tetap di `AppShell`.
- `OnboardingFlow`: latar `StacklyColor.surfaceWindow`, kolom isi 520 pt di tengah dengan jarak antar elemen 14 pt, di dalam `ScrollView` vertikal supaya tidak terpotong di 900 × 600 pt, dan footer di bawah garis pemisah dengan titik, teks langkah, Back, dan tombol utama (`.borderedProminent`, `.keyboardShortcut(.defaultAction)`). Toolbar dan judul jendela disembunyikan (`.toolbar(removing: .title)`, `.toolbarBackgroundVisibility(.hidden, for: .windowToolbar)`), jadi hanya tombol lampu lalu lintas yang terlihat seperti di mockup.
- Judul Welcome `.largeTitle.bold()`, judul langkah lain `.title.bold()`, subjudul `.body` dengan `textSecondary` (baris tipografi spec 0012), judul baris `.body.weight(.semibold)`, teks kecil `.callout`, path dan perintah `.stacklyCode()`. Kartu dan daftar baris memakai `.cardSurface()`.
- Item menu: `CommandGroup(after: .help) { Button("Welcome to Stackly") { … } }` di `StacklyApp`, memanggil `onboarding.present()` lalu `WindowOpener.openMain()`. Tanpa shortcut sampai fitur 69 memutuskan peta nya.
- Lewati: `onExitCommand` dan `.accessibilityAction(.escape)` di `OnboardingFlow` memanggil `finish(.skipped)`. Konfirmasi yang sedang terbuka menerima Escape lebih dulu (Cancel), jadi Escape di dialog tidak pernah melewati onboarding.
- Perintah shell lain yang butuh shell (fitur 69 nanti) nonaktif selama onboarding tampil. Settings (⌘,) tetap bisa dibuka.

### Langkah Welcome

Ikon app dari `NSApplication.shared.applicationIconImage` 112 pt (dekoratif, `accessibilityHidden(true)`), judul, subjudul, lalu tiga baris fitur. Ikon baris adalah tile 34 pt dengan `toneSurface` spec 0012 dan simbol: Isolated instances memakai `SectionIcon(.instances)` (hijau), Verified runtimes `SectionIcon(.runtimes)` (teal), No account needed simbol `laptopcomputer` dengan tone `neutral`.

### Kartu Background Agent

Satu kartu `.cardSurface()` dengan ikon status, judul, detail, dan aksi di kanan, dipilih dari baris pertama yang cocok. Fungsi murni `BackgroundAgentStepContent.make(assessment:presence:unreachableSince:now:lastError:isWorking:)` menghasilkan isinya. Presence dan `since` dari `DaemonModel` (spec 0013), ambang dari `AgentTiming.startGrace` (spec 0015).

| Kondisi | Tone dan ikon | Judul | Detail | Aksi |
| --- | --- | --- | --- | --- |
| `assessment` nil | progress tertunda | "Checking the background service…" | | |
| `enabled`, presence bukan `unreachable` | positive, `checkmark.circle.fill` | "Allowed in Login Items" | "stacklyd starts when you log in." | |
| `enabled`, `unreachable` kurang dari 15 detik | informative | "Allowed in Login Items" | "stacklyd is starting…" | |
| `enabled`, `unreachable` 15 detik atau lebih | caution | "Allowed, but stacklyd isn’t running" | "It was stopped or it exited. Start it again here, or run stackly daemon start in Terminal." | Start stacklyd (`start()`) |
| `not_registered` | caution | "Background service is off" | "Turn it on so stacklyd can run your instances, even when this window is closed. macOS may ask you to allow it." | Turn On (`turnOn()`) |
| `requires_approval` | caution, `clock` | "Waiting for approval" | "Turn on Stackly in System Settings > General > Login Items & Extensions. stacklyd can’t run until you do." | Open System Settings… (`openLoginItems()`) |
| `disabled` | caution | "Turned off in Login Items" | "Turn on Stackly in System Settings > General > Login Items & Extensions to run stacklyd again." | Open System Settings… (`openLoginItems()`) |
| `not_found` | critical | "Stackly can’t find its background service" | "macOS couldn’t find stacklyd for this copy of Stackly. Move Stackly to the Applications folder, or install it again." | |
| `unavailable` `helper_missing` atau `not_bundled` | caution | "This build doesn’t include stacklyd" | "Start stacklyd yourself, or build the app with cargo xtask build-app." | |
| `unavailable` `root_override` | caution | "STACKLY_ROOT is set" | "The background service only runs the default Stackly folder. Start stacklyd yourself for this folder." | |
| `unavailable` lain | caution | "Stackly couldn’t check its background service" | "macOS didn’t answer ({detail}). Check again in a moment." dengan `{detail}` sama dengan banner spec 0015 | Check Again (`refresh()`) |

- Baris yang kolom ikonnya hanya menyebut tone memakai `Tone.symbol` tone itu (spec 0012).
- Setiap baris `enabled` menambah satu `StatusLabel` konteks `.row` di bawah detail, dengan `ShellPresentation.footer` spec 0013 (misalnya "stacklyd running" atau "stacklyd unavailable"), sehingga status daemon terlihat di langkah ini.
- `lastError` bukan nil: satu baris `.callout` di bawah kartu dengan simbol tone `critical` dan teks `AgentActionError` spec 0015.
- `isWorking`: aksi kartu nonaktif lewat `.disabled(reason:)` spec 0012 dengan alasan "Working…", ditambah `DelayedProgress`.
- Subjudul langkah: "stacklyd supervises your instances when the window is closed. macOS asks you to allow it in Login Items."
- Langkah ini tidak pernah memanggil `register`, `unregister`, atau `relaunch` sendiri; hanya lewat `turnOn()` dan `start()` milik `BackgroundAgentModel`, sehingga aturan AC-11 spec 0015 tetap berlaku.

### Langkah Storage

`StorageContent.make(roots: DaemonRoots?, home: String, rootOverride: Bool)` (murni):

| Kondisi | Baris |
| --- | --- |
| tanpa override, `roots` ada dan `roots.rootOverride` false | Data `roots.data`, Cache `roots.cache`, Logs `roots.logs`, Secrets "Login Keychain" |
| tanpa override, selain itu (`roots` nil, atau `roots.rootOverride` true dari daemon lain) | Data `~/Library/Application Support/Stackly`, Cache `~/Library/Caches/Stackly`, Logs `~/Library/Logs/Stackly` dari `StorageLocations.defaults(home:)`, Secrets "Login Keychain" |
| override aktif | satu baris "Folders" dengan teks "STACKLY_ROOT is set, so stacklyd keeps data, cache, and logs inside that folder.", lalu Secrets "Login Keychain" |

- `home` dari `getpwuid_r(getuid())`, helper yang sama dengan `LaunchRecordReader` spec 0015, tidak pernah dari `HOME`. Path yang diawali home ditulis dengan `~`; path lain ditulis utuh. Nilai path bisa dipilih (`textSelection(.enabled)`) dan dibungkus tanpa terpotong.
- `roots` dari `DaemonModel.lastDaemon?.info?.roots` (daemon terakhir yang diketahui, spec 0013). `rootOverride` dari nilai yang sudah diteruskan target app ke `ShellBootstrap` (spec 0013).

### Baris Catalog Trust

`CatalogTrustContent.make(info: CatalogInfo?, error: ErrorBody?, unsupported: Bool, now: Date)` (murni). Satu baris berisi label sumber, detail, baris revision, dan toggle native yang menyala dan nonaktif dengan `.help` dan label VoiceOver "The official catalog is always on." (atau "The development catalog is always on.").

| Kondisi | Label | Detail | Baris revision |
| --- | --- | --- | --- |
| `info` nil, tanpa error, tidak unsupported | "official" | "Maintained by the Stackly project and bundled with this app. Runtimes are checked against pinned digests." | "Details appear once stacklyd is running." |
| `info.source` `bundled` | "official" | sama dengan baris di atas | "Revision {revision} · published {tanggal}" |
| `info.source` `development` | "development" | "A development catalog from STACKLY_CATALOG_DIR. It isn’t the official catalog." | "Revision {revision} · published {tanggal}" |
| `info.source` `unrecognized` | "Unknown source" | "This version of Stackly doesn’t recognize this catalog source." | "Revision {revision} · published {tanggal}" |
| `info.outdated` true | sama | sama | ditambah " · older than 180 days" |
| `error` ada | "official" | "The catalog isn’t available: {error.message}." | `error.hint` kalau ada |
| `unsupported` | "official" | "This version of stacklyd doesn’t provide the catalog. Update Stackly." | |

- `{tanggal}` gaya `.abbreviated` locale dan zona waktu sistem, sama dengan subjudul Catalog spec 0017. Trust `unrecognized` menambah "Unknown trust" di detail.
- Judul langkah "Catalog sources", subjudul "Providers describe how to install and run each server. Only trusted sources appear in the catalog.", dan catatan trust di bawah baris dari tabel *Copy*.
- Data dari `CatalogStore` milik `DaemonModel` (spec 0017); langkah ini tidak memanggil method apa pun sendiri.

### Kontrol Command Line

`CommandLineStepContent.make(status:bundledTool:lastError:)` (murni). `bundledTool` adalah `URL?` dari `CommandLineTool.bundledTool` (nil kalau `<bundle>/Contents/MacOS/stackly` bukan file executable).

| Kondisi | Detail | Kontrol |
| --- | --- | --- |
| `bundledTool` nil | "This build doesn’t include the stackly command. Build the app with cargo xtask build-app." | toggle mati, `.disabled(reason:)` dengan teks detail |
| `not_installed` | "No password needed. Stackly doesn’t change your PATH or shell files." | toggle mati |
| `installed` | "~/.local/bin/stackly points to this copy of Stackly." | toggle menyala |
| `stale` | "~/.local/bin/stackly points to another copy of Stackly." | tombol Repair… |
| `broken` | "~/.local/bin/stackly points to a copy of Stackly that’s gone." | tombol Repair… |
| `conflict` | "~/.local/bin/stackly already exists and wasn’t made by Stackly. Stackly won’t change it." | toggle mati, `.disabled(reason:)` dengan teks detail |

- Toggle memakai `Binding` buatan: getter dari status, setter tidak mengubah apa pun selain membuka konfirmasi. Menyala membuka prompt Install, mati membuka prompt Remove. Status dibaca ulang setelah aksi, saat langkah muncul, dan saat app aktif kembali.
- `CommandLineToolPrompts` (`StacklyUI/Agent/`, dipakai juga oleh `CommandLineToolSection` spec 0015), semua dengan peran standard:

| Prompt | Judul | Pesan | Tombol |
| --- | --- | --- | --- |
| install | "Install the stackly command?" | "Stackly creates a link at ~/.local/bin/stackly and links its manual pages in ~/.local/share/man/man1. Your PATH and shell files stay as they are." | Install |
| repair | "Repair the stackly command?" | "Stackly points ~/.local/bin/stackly and its manual pages to this copy of Stackly. Your PATH and shell files stay as they are." | Repair |
| remove | "Remove the stackly command?" | "Stackly deletes only the links it made in ~/.local/bin and ~/.local/share/man/man1. Stackly itself stays installed." | Remove |

- Status `installed` menambah catatan PATH: "If Terminal can’t find stackly, add this line to your shell profile:" lalu baris `export PATH="$HOME/.local/bin:$PATH"` (`.stacklyCode()`) dengan tombol Copy, lalu "Or run it directly: {path bundle}/Contents/MacOS/stackly". Copy menulis ke `NSPasteboard` dan mengumumkan "Copied.".
- Kegagalan aksi (`CommandLineTool` melempar error, misalnya folder bisa ditulis group) tampil sebagai satu baris `.callout` tone `critical` dengan pesan error itu, dan toggle kembali ke status sebenarnya.
- Blok contoh di bawah baris: `OnboardingCopy.commandExamples`, satu perintah per baris dengan awalan `$ `, `.stacklyCode()`, bisa dipilih. Isi awal: `stackly daemon status` dan `stackly provider list`. Fitur 22, 34, dan 46 menambah `stackly instance list`, `stackly doctor`, dan `stackly env up ~/Code/erp` di commit build nya.

### Copy

Semua teks di `OnboardingCopy.swift`. Teks kartu, baris catalog, dan kontrol Command Line ada di tabel masing masing di atas.

| Tempat | Teks |
| --- | --- |
| Label langkah | "Welcome", "Background Agent", "Storage", "Catalog Trust", "Command Line" |
| Footer | "Step {n} of 5 · {label}", "Back", "Continue", "Not Now", "Get Started" |
| Welcome judul | "Welcome to Stackly" |
| Welcome subjudul | "Run local databases, queues and proxies side by side, each with its own data, port and secret." |
| Baris 1 | "Isolated instances" · "Each instance has its own data folder, port, and secret. Instances run as your user, not in a sandbox, so other apps you run can still read their files." |
| Baris 2 | "Verified runtimes" · "Downloaded, checked against a pinned digest, then committed." |
| Baris 3 | "No account needed" · "Stackly and stacklyd run on this Mac. Nothing here asks you to sign in." |
| Background Agent judul | "Keep services running in the background" |
| Storage judul | "Where your data lives" |
| Storage subjudul | "Everything stays in your user Library. Instance data is kept by stable ID, so renaming never moves files." |
| Storage label | "Data", "Cache", "Logs", "Secrets", "Folders"; nilai Secrets "Login Keychain" |
| Catalog Trust catatan | "Trust tells you where a provider comes from. It isn’t a sandbox: every server runs with your user’s permissions." |
| Command Line judul | "Command line tool" |
| Command Line subjudul | "The app and the CLI talk to the same daemon, so changes in one show up in the other." |
| Command Line baris | "Install stackly in ~/.local/bin" |
| Pengumuman | "Step {n} of 5, {label}.", "Setup complete.", "Copied." |
| Menu Help | "Welcome to Stackly" |

### API surface

| Permukaan | Bentuk | Input utama | Output utama | Siapa | Error utama |
| --- | --- | --- | --- | --- | --- |
| `OnboardingModel.present()`, `next()`, `back()`, `go(to:)`, `finish(_:)` | method model | langkah, alasan selesai | `isPresented`, `step` | `MainWindowRoot`, menu Help | tidak ada |
| `OnboardingRecordStore.isFinished`, `markFinished()` | protocol | | Bool | `OnboardingModel` | tidak ada (`UserDefaults` tidak melempar) |
| `BackgroundAgentStepContent.make`, `StorageContent.make`, `CatalogTrustContent.make`, `CommandLineStepContent.make` | fungsi murni | state dari model lain | isi tampilan | view langkah, test | tidak ada |
| Tombol kartu Background Agent | aksi pengguna | | memanggil `BackgroundAgentModel` | pengguna | `AgentActionError` di bawah kartu |
| Toggle dan Repair Command Line | aksi pengguna lewat konfirmasi | | memanggil `CommandLineTool` | pengguna | error `CommandLineTool` di bawah baris |
| Menu Help "Welcome to Stackly" | perintah menu | | onboarding tampil | pengguna | tidak ada |

Tidak ada method IPC, event, kode error wire, atau perintah CLI baru.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| launch | tampil atau tidak | `OnboardingRecordStore.isFinished` (`UserDefaults` `onboarding.finished`) |
| Get Started, Escape | record selesai | `OnboardingRecordStore.markFinished()` |
| footer | label langkah, `{n}` | `OnboardingStep.label` dan `rawValue + 1` |
| footer | label tombol utama | langkah, ditambah `BackgroundAgentModel.assessment?.state` di langkah 2 |
| kartu agent | state, reason, langkah pemulihan | `BackgroundAgentModel.assessment` (spec 0015) |
| kartu agent | presence dan `since` | `DaemonModel.presence` dan `ConnectionState.unreachable(since:)` (spec 0013) |
| kartu agent | ambang 15 detik | `AgentTiming.startGrace` (spec 0015) |
| kartu agent | teks `{detail}` | aturan banner `unavailable` lain spec 0015 |
| kartu agent | status daemon | `ShellPresentation.footer` (spec 0013) |
| kartu agent | Last error, sedang bekerja | `BackgroundAgentModel.lastError`, `isWorking` |
| Storage | path daemon | `DaemonModel.lastDaemon?.info?.roots` |
| Storage | path default | `StorageLocations.defaults(home:)`, tabel macOS default spec 0004 |
| Storage | home dan `~` | `getpwuid_r(getuid())`, helper `LaunchRecordReader` spec 0015 |
| Storage | override aktif | `rootOverride` yang diteruskan target app ke `ShellBootstrap` (spec 0013) |
| Catalog Trust | sumber, trust, revision, tanggal, outdated | `CatalogStore.info` (`CatalogInfo`, spec 0017) |
| Catalog Trust | error dan unsupported | `CatalogStore.error`, `CatalogStore.unsupported` (spec 0017) |
| Catalog Trust | format tanggal | `.abbreviated`, locale dan zona waktu sistem, seperti spec 0017 |
| Command Line | status | `CommandLineTool.status()` (spec 0015) |
| Command Line | CLI di bundle ada | `CommandLineTool.bundledTool` (tambahan spec ini) |
| Command Line | path bundle | `Bundle.main.bundleURL` |
| Command Line | baris PATH | konstanta spec 0015 |
| Command Line | contoh perintah | `OnboardingCopy.commandExamples` |
| setelah tertutup | bagian shell | `@SceneStorage("shell.section")` (spec 0013), tidak diubah |
| pengumuman, semua teks | teks | `OnboardingCopy` |

### Key invariants

- Onboarding tidak pernah mengirim mutasi ke daemon dan tidak punya jalur ke `MutationPath`.
- `register`, `unregister`, dan `relaunch` dari onboarding hanya lewat `BackgroundAgentModel.turnOn()` dan `start()`, yang hanya dipanggil tombol Turn On dan Start stacklyd. Muncul nya langkah Background Agent hanya memicu `refresh()`; Continue, Not Now, Back, titik langkah, Escape, dan Get Started tidak memanggil model agent sama sekali.
- File di home hanya berubah lewat tombol aksi konfirmasi `CommandLineToolPrompts`.
- Satu satunya yang ditulis onboarding sendiri adalah `onboarding.finished`, dan hanya oleh Get Started dan Escape.
- Teks onboarding tidak pernah menyatakan provider disandbox, tidak bisa membaca file user, atau catalog terbaru, dan tidak menyebut fitur atau perintah yang tidak ada di build itu.
- Nilai `STACKLY_ROOT` tidak pernah tampil.
- Onboarding tidak pernah menyentuh jaringan atau meminta akun.

### Security model

- Batasnya tetap satu akun user (BR-012). Onboarding tidak membaca atau menampilkan secret, tidak membuat ID atau path, dan tidak menjalankan proses (aturan sumber spec 0013 tetap berlaku untuk folder `Onboarding/`).
- Persetujuan background tetap milik macOS: onboarding hanya membaca status dan membuka Login Items saat pengguna menekan tombolnya (spec 0015).
- Instalasi CLI hanya lewat `CommandLineTool` dengan konfirmasi, sehingga aturan symlink spec 0015 (hanya link terkelola di home, tanpa admin, tanpa PATH atau file shell) berlaku tanpa pengecualian.
- `onboarding.finished` bukan data sensitif; mengubahnya dari luar hanya membuat onboarding tampil atau tidak.
- Copy yang jujur adalah bagian dari model keamanan: AC-061 meminta pengguna tidak dibuat percaya ada sandbox yang tidak ada (BR-012, BR-020, REQ-032).

### Configuration required

- Tidak ada variabel environment, secret, atau credential baru. Key `UserDefaults` `onboarding.finished` (lihat *Model data*) bisa diset lewat argumen launch untuk run manual.

### Critical test scenarios

- Peluncuran pertama dengan store kosong menampilkan onboarding langkah 1 tanpa sidebar; store true menampilkan shell; menutup jendela di langkah 3 lalu membuka lagi tetap di langkah 3; Quit tidak menulis record. Memverifikasi **AC-1**.
- Label tombol utama per langkah dan per state agent (nil, setiap state, `enabled`), Back hanya mulai langkah 2, titik langkah pindah langsung. Memverifikasi **AC-2**.
- Jalan lengkap dengan Continue, Not Now, Back, titik, Get Started, lalu sekali lagi dengan Escape di setiap langkah: log fake agent hanya `status`, home sementara tidak berubah, `MutationPath` tidak dipanggil, bagian shell tidak berubah; menu Help saat tersembunyi dan saat tampil. Memverifikasi **AC-3**.
- Isi Welcome sama dengan tabel *Copy*, tanpa baris Environments; aturan sumber jaringan. Memverifikasi **AC-4**.
- Setiap baris *Kartu Background Agent* dengan jam yang disuntik (14 dan 15 detik), aksi memanggil method model yang benar, `isWorking` menonaktifkan aksi, `lastError` tampil. Memverifikasi **AC-5**.
- Storage dengan roots daemon, tanpa roots, override aktif, dan home yang tidak menjadi awalan path. Memverifikasi **AC-6**.
- Setiap baris *Baris Catalog Trust*, termasuk `outdated`, `development`, `unrecognized`, error dengan hint, dan unsupported; tidak ada community. Memverifikasi **AC-7**.
- Setiap baris *Kontrol Command Line* di home sementara: toggle menyala lalu Cancel tidak menulis, lalu Install membuat link; mati lalu Remove; stale dan broken lewat Repair; conflict dan bundle tanpa CLI nonaktif; error folder bisa ditulis group tampil. Memverifikasi **AC-8**.
- Test copy dan pindaian frasa terlarang. Memverifikasi **AC-9**.
- Keyboard, VoiceOver, pengumuman per langkah, kontras, Reduce Transparency, ukuran minimum: preview dan evidence manual. Memverifikasi **AC-10**, **AC-11**.

### Cek manual di /check verify

Dicatat sebagai evidence manual (spec 0002), kind `native`, status awal `NOT_RUN`:

1. Bundle dari `cargo xtask build-app --install` di `~/Applications`, dibuka dengan `-onboarding.finished NO`: lima langkah tampil, langkah 2 menampilkan state agent yang sebenarnya di Mac ini (biasanya `not_registered`) tanpa menekan Turn On, dan `stackly daemon status --json` sebelum dan sesudah jalan lengkap memberi blok `agent` yang sama. Kalau daemon berjalan, Storage menampilkan roots yang sama dengan `info.roots` di output itu, dan Catalog Trust menampilkan revision dan tanggal catalog bawaan yang sama dengan `stackly provider list` (**AC-3**, **AC-5**, **AC-6**, **AC-7**).
2. Tekan Turn On dari onboarding hanya setelah persetujuan kamu lewat lead, karena itu mendaftarkan agent sungguhan; hasilnya dicatat bersama record AC-17 spec 0015 dan tetap `BLOCKED` sampai identity signing ada (OQ-012) kalau build ad hoc ditolak (**AC-5**).
3. Command Line di akun ini: Install, cek `ls -l ~/.local/bin/stackly`, Remove, dan pastikan file shell tidak berubah (`stat` sebelum dan sesudah) (**AC-8**).
4. Keyboard saja dan VoiceOver dari langkah 1 sampai Get Started, lalu Escape dari langkah 3; light, dark, Increase Contrast, Reduce Transparency, dan jendela 900 × 600 (**AC-10**, **AC-11**).
5. Baca ulang semua copy terhadap AC-061 (**AC-9**).

### Jawaban untuk Follow-up spec lain

| Spec sumber | Follow-up | Jawaban di spec ini |
| --- | --- | --- |
| 0015 | Background Agent memakai `BackgroundAgentModel` dan aksinya; Command Line memakai `CommandLineTool` | *Kartu Background Agent*, *Kontrol Command Line*, AC-5, AC-8 |
| 0015 (baris 68) | layar onboarding Background Agent dan Command Line milik fitur 16 | spec ini |
| 0015 (baris 319) | `CommandLineToolSection` dengan `// provisional: F16` | seksi tetap di Diagnostics, prompt dipakai bersama lewat `CommandLineToolPrompts`, penanda menjadi `// provisional: F32` (AC-8) |
| 0015 (baris 331, 554) | teks `/usr/local/bin` dengan password dan "only while the app is open" tidak dipakai | *Copy*, *Kartu Background Agent*, *Kontrol Command Line*, AC-5, AC-8, AC-9 |
| 0017 | Catalog Trust memakai `CatalogInfo.source`, `trust`, dan `publishedAt` | *Baris Catalog Trust*, AC-7 |
| 0012 | tipografi judul onboarding, token, `ViewState`, konfirmasi | *Tampilan di jendela utama*, AC-8, AC-10 |
| 0013 | tempat onboarding di shell dan cara membukanya lagi | *Tampilan di jendela utama*, AC-1, AC-3 |

### Usulan untuk spec lain

Spec ini tidak mengedit spec lain. Teks berikut adalah usulan untuk pemilik masing masing lewat `/architect` update atau `/sync`, sebaiknya di commit yang sama dengan langkah build yang mengubah perilakunya.

| Spec | Bagian | Usulan |
| --- | --- | --- |
| 0013 | *Shell jendela* | isi `Window` utama menjadi `MainWindowRoot` (fitur 16), yang menampilkan onboarding atau `AppShell`; `MainWindowAccessor`, penangkapan `openWindow`, dan `windowWillClose` pindah ke `MainWindowRoot`; `ShellRuntime` memegang `OnboardingModel`; item menu Help "Welcome to Stackly" |
| 0013 | *Model data* dan *Key invariants* | kalimat "Fitur ini tidak menyimpan state ke disk" dan invariant bahwa shell dan client tidak menulis state ke disk ditambah pengecualian: fitur 16 menyimpan satu preferensi `onboarding.finished` di `UserDefaults` |
| 0015 | *GUI*, `CommandLineToolSection` | penanda menjadi `// provisional: F32`; konfirmasi Install, Repair, dan Remove memakai `CommandLineToolPrompts` dari spec 0016 |
| 0015 | *Instalasi CLI* | `CommandLineTool.bundledTool: URL?` (nil kalau `<bundle>/Contents/MacOS/stackly` tidak ada atau bukan executable), dan Install serta Repair di Diagnostics juga nonaktif dengan alasan saat nil, supaya build Xcode IDE tidak membuat link rusak |
| 0015 | *Follow-up* | baris fitur 16 dijawab spec 0016 |

Usulan untuk scope (lead, lewat `/scope`): `docs/scope/ui-macos.md` *Selisih yang sudah diketahui* untuk onboarding ditambah: Welcome memakai ikon app, bukan tile accent; baris "Isolated instances" memakai teks isolasi logis spec 0016, bukan "never touching each other’s data"; baris "Environments as files" dan contoh `env up`, `doctor`, dan `instance list` baru tampil saat fiturnya ada, dan baris "No account needed" mengisi tempatnya; Catalog Trust berjudul "Catalog sources", tanpa toggle community dan tanpa "Signed manifests"; toggle Command Line mencerminkan status nyata dan bekerja langsung lewat konfirmasi, bukan saat Get Started.

## Build plan

Urutan mengikuti pendekatan Journey: onboarding adalah langkah pertama JOURNEY-01, jadi kerangka dan langkah yang tidak bergantung pada fitur lain dibangun lengkap lebih dulu, lalu tiga langkah yang memakai model fitur 15 dan 17 dibangun begitu model itu ada, dan bukti manual menutupnya. Milestone 1 butuh milestone "Wiring, instances, and operations" fitur 13 (`ui-macos.md`). Milestone 2 butuh milestone 3 fitur 15 (GUI dan instalasi CLI). Milestone 3 butuh `CatalogStore` dari milestone 3 fitur 17 (langkah build 8 spec 0017). Milestone 2 dan 3 bisa berjalan sebagai dua sesi paralel karena tidak menyentuh file yang sama (kecuali `OnboardingCopy.swift`, yang dibagi per bagian dan digabung di milestone 4). Komponen fitur 67 dipakai kalau sudah ada, tidak ditunggu.

**Milestone 1: kerangka onboarding, Welcome, dan Storage**

1. `OnboardingStep`, `OnboardingRecordStore` dengan implementasi `UserDefaults` dan memori, `OnboardingModel` dengan aturan label tombol utama, dan test model. Memenuhi **AC-1**, **AC-2**, **AC-3**.
2. `MainWindowRoot`, pemindahan `MainWindowAccessor` dan penangkapan `openWindow` dari `AppShell`, `OnboardingModel` di `ShellRuntime`, `StacklyApp` memakai `MainWindowRoot`, item menu Help, Escape dan aksi escape VoiceOver, fokus dan pengumuman per langkah. Memenuhi **AC-1**, **AC-3**, **AC-10**.
3. `OnboardingFlow` dengan footer, langkah Welcome, langkah Storage (`StorageLocations`, `StorageContent`), placeholder untuk tiga langkah lain yang hanya menampilkan judul dan subjudul langkahnya, `OnboardingCopy` untuk bagian ini, test copy, pindaian frasa terlarang, dan aturan sumber jaringan. Memenuhi **AC-4**, **AC-6**, **AC-9**, **AC-10**, **AC-11**.

**Milestone 2: Background Agent dan Command Line**

4. `BackgroundAgentStepContent` dan kartunya, label Not Now dari `assessment` nyata, test per baris dengan `FakeBackgroundAgentController` dan jam yang disuntik. Memenuhi **AC-2**, **AC-5**.
5. `CommandLineTool.bundledTool`, `CommandLineToolPrompts` (dipakai juga oleh `CommandLineToolSection`, penandanya menjadi `// provisional: F32`), `CommandLineStepContent`, toggle dengan konfirmasi, Repair, catatan PATH dengan Copy, blok contoh, dan test di home sementara. Memenuhi **AC-8**.

**Milestone 3: Catalog Trust**

6. `CatalogTrustContent` dan barisnya dari `CatalogStore`, catatan trust, dan test per baris dengan fixture `protocols/fixtures/catalog/` spec 0017. Memenuhi **AC-7**.

**Milestone 4: jalan lengkap dan bukti manual**

7. Test jalan lengkap tanpa efek samping (fake agent, home sementara, penghitung `MutationPath`), test copy untuk semua tabel, dan preview setiap langkah di light, dark, dan 900 × 600 (snapshot fitur 68 kalau sudah ada). Memenuhi **AC-3**, **AC-9**, **AC-10**.
8. Evidence manual *Cek manual di /check verify* langkah 1, 3, 4, dan 5 di bundle nyata; langkah 2 hanya setelah persetujuan kamu lewat lead. Memenuhi **AC-3**, **AC-5**, **AC-8**, **AC-9**, **AC-10**, **AC-11**.

Sesi paralel: langkah 1 dan 3 bisa dimulai bersamaan (3 memakai model dari 1 lewat stub sampai 1 selesai, jadi lebih aman berurutan dalam satu sesi). Langkah 2 setelah langkah 1. Milestone 2 (langkah 4 dan 5) dan milestone 3 (langkah 6) bisa berjalan sebagai dua sesi setelah milestone 1, masing masing hanya menulis file langkahnya sendiri dan bagian copy nya. Langkah 7 dan 8 setelah semuanya.

## Consequences

**Positive**:

- Onboarding tidak punya jalur efek samping sendiri: semua aksi lewat model fitur 15 yang sudah diuji, jadi aturan "izin tidak diubah tanpa sepengetahuan pengguna" tetap satu tempat.
- Melewati dan membuka lagi aman secara konstruksi, karena satu satunya yang ditulis onboarding adalah preferensi GUI.
- Teks isolasi dan trust yang jujur ditulis sekali dan dijaga test, sehingga AC-061 untuk onboarding bisa diperiksa mesin dan manusia.
- Onboarding tetap berguna saat daemon belum berjalan (kasus normal di peluncuran pertama): Storage memakai default spec 0004 dan Catalog Trust menjelaskan bahwa detail muncul setelah stacklyd berjalan.
- Tidak ada perubahan protokol, Rust, atau CLI, jadi fitur ini tidak menahan atau ditahan program foundation selain shell dan model GUI.

**Negative / tradeoffs**:

- Onboarding menggantikan shell di jendela utama, jadi selama tampil banner daemon dan sidebar tidak terlihat; status daemon hanya tampil di langkah Background Agent.
- Tidak ada tombol Skip yang terlihat; melewati sekaligus hanya lewat Escape atau titik langkah terakhir lalu Get Started. Pengguna mouse yang tidak tahu Escape tetap perlu dua klik.
- Record di `UserDefaults` hilang kalau preferensi app dihapus, dan onboarding tampil lagi; tidak ada yang rusak, tetapi itu bisa membingungkan.
- Toggle yang bekerja lewat konfirmasi sedikit tidak biasa: toggle tidak langsung berubah saat diklik, baru setelah konfirmasi.
- Copy berbeda dari mockup di beberapa tempat (isolasi, catalog, CLI, Environments), jadi mockup perlu catatan selisih sampai diperbarui.
- `OnboardingCopy.commandExamples` dan baris Welcome bergantung pada fitur lain menambah baris nya; kalau fitur itu lupa, onboarding tetap jujur tetapi lebih sedikit isinya.
- Path Storage saat daemon belum pernah berjalan dihitung GUI dari aturan spec 0004 tanpa fixture bersama; kalau aturan itu berubah, dua tempat harus diubah.

**Neutral**:

- Jendela utama kini punya akar `MainWindowRoot`; `AppShell` tidak lagi memegang accessor jendela.
- Satu key `UserDefaults` baru di domain app.
- Item menu Help ditambah sekarang dan dipindah ke peta perintah saat fitur 69 dibangun.

## Follow-up

- [ ] Terapkan *Usulan untuk spec lain* (spec 0013 dan 0015) lewat pemilik masing masing saat langkah build terkait masuk.
- [ ] `docs/scope/ui-macos.md` (lewat `/scope`): catat selisih mockup onboarding di *Usulan untuk spec lain*.
- [ ] Fitur 22: tambahkan `stackly instance list` ke `OnboardingCopy.commandExamples` saat perintah itu dibangun.
- [ ] Fitur 34: tambahkan `stackly doctor` ke `OnboardingCopy.commandExamples`.
- [ ] Fitur 44: tambahkan baris "Environments as files" dari mockup ke langkah Welcome.
- [ ] Fitur 46: tambahkan `stackly env up ~/Code/erp` ke `OnboardingCopy.commandExamples`.
- [ ] Fitur 32: baris "Show Welcome…" di tab General Settings yang memanggil `OnboardingModel.present()`.
- [ ] Fitur 37: putuskan apakah langkah Catalog Trust menampilkan sumber lokal atau community dan trust per provider.
- [ ] Fitur 67: umpan balik "Setup complete." dan "Copied." lewat komponen umpan balik.
- [ ] Fitur 68: snapshot setiap langkah dan setiap baris tabel state, di light, dark, Increase Contrast, dan Reduce Transparency.
- [ ] Fitur 69: daftarkan "Welcome to Stackly" di peta perintah, dan nonaktifkan perintah shell selama onboarding tampil.
- [ ] `/sync` setelah build: catat folder `Onboarding/`, `MainWindowRoot`, dan key `onboarding.finished` di `apps/macos/AGENTS.md`.

## Rationale

Konteks, opsi per keputusan, dan alasan ada di [rationale.md](rationale.md).
