# Verify: Onboarding · spec 0016 · updated 2026-10-22

_Langkah diturunkan dari acceptance criteria spec 0016. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Langkah milestone yang belum dibangun belum ada di sini._

## Milestone 1: kerangka onboarding, Welcome, dan Storage

### UI / manual

- [ ] Bangun app (`cargo xtask build-app`), buka dengan argumen launch `-onboarding.finished NO` → jendela utama menampilkan onboarding langkah 1 sebagai seluruh isi jendela: tanpa sidebar, toolbar, judul jendela, footer daemon, atau banner daemon; hanya tombol lampu lalu lintas → AC-1
- [ ] Di langkah 1 baca footer → lima titik langkah (titik aktif lebih lebar), teks "Step 1 of 5 · Welcome", tanpa Back, tombol utama "Continue" → AC-2
- [ ] Tekan Continue, Back, dan klik tiap titik langkah → langkah berpindah langsung ke langkah itu; Back tampil mulai langkah 2; label tombol utama "Not Now" di langkah 2, "Continue" di langkah 3 dan 4, "Get Started" di langkah 5 → AC-2
- [ ] Baca langkah Welcome → ikon app 112 pt, judul "Welcome to Stackly", subjudul, dan baris "Isolated instances" serta "No account needed" masing masing dengan ikon di tile netral; tidak ada baris "Verified runtimes" dan tidak ada baris "Environments as files"; teks isolasi menyebut instance berjalan sebagai user kamu, tanpa sandbox → AC-4, AC-9
- [ ] Di langkah 3 (Storage) tanpa `STACKLY_ROOT` → baris Data, Cache, Logs, dan Secrets "Login Keychain"; path memakai `~` untuk folder home; bila daemon berjalan, path sama dengan `stackly daemon status --json` `info.roots` → AC-6
- [ ] Jalankan app dengan `STACKLY_ROOT` di environment → subjudul memakai varian override, satu baris "Folders" dengan catatan override, dan Secrets "A keychain file inside that folder"; nilai variabel tidak tampil di mana pun → AC-6
- [ ] Tekan Escape di langkah 3 → onboarding tertutup, shell tampil di bagian yang tersimpan (Overview pada peluncuran pertama), tanpa kartu umpan balik; buka app lagi tanpa `-onboarding.finished NO` → shell langsung → AC-1, AC-3
- [ ] Buka onboarding lagi dengan argumen `-onboarding.finished NO`, tekan Get Started di langkah 5 → shell tampil dan kartu umpan balik "Setup complete." muncul sekali dan diumumkan VoiceOver sekali → AC-11
- [ ] Menu Help → "Welcome to Stackly" (tanpa shortcut): saat jendela utama ditutup, membuka jendela dan menampilkan langkah 1; saat onboarding tampil di langkah 3, hanya membawa jendela ke depan dan tetap di langkah 3 → AC-3
- [ ] Di langkah 3 tutup jendela (⌘W) lalu klik Stackly di Dock → onboarding tampil lagi di langkah 3; tutup lagi dan keluar lewat Quit, buka lagi → mulai dari langkah 1, tanpa Get Started tercatat → AC-1
- [ ] Saat onboarding tampil, buka menu View dan Instance → perintah shell nonaktif; ⌘, tetap membuka Settings → AC-3
- [ ] Keyboard saja (Keyboard navigation menyala): Tab melewati titik langkah, Back, dan tombol utama; Space menekannya; Return menekan tombol utama; Escape melewati → AC-10
- [ ] VoiceOver: titik langkah dibaca "Step {n} of 5, {label}" dengan trait terpilih untuk langkah aktif; saat langkah berganti fokus pindah ke judul langkah dan "Step {n} of 5, {label}." terucap sekali → AC-10
- [ ] Jendela 900 × 600 pt, light, dark, Increase Contrast, dan Reduce Transparency → konten tidak terpotong (digulir bila perlu), teks terbaca, tanpa glass atau warna sistem sekunder → AC-10
- [x] Baca ulang semua teks langkah 1 sampai 3 terhadap AC-061 → tidak ada janji sandbox, isolasi dari file user, atau fitur yang belum ada → AC-9

### Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter OnboardingModelTests` → test lulus: record kosong dan true, key `onboarding.finished` di `UserDefaults`, lima langkah dan label, `next` dan `back` berhenti di ujung, titik langkah, label `primaryLabel` per langkah dan per state agent (nil dan keenam state), Back mulai langkah 2, `finish` kedua alasan menulis record yang sama, `present` mulai dari langkah 1 atau menjaga langkah → AC-1, AC-2, AC-3
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter MainWindowRootTests` → test lulus: onboarding menjadi seluruh isi jendela (tanpa split view) sampai selesai, shell saat record true, pindah ke shell saat onboarding selesai, runtime menyimpan langkah, aksi Help "Welcome to Stackly" menampilkan langkah 1 tanpa menulis record dan menjaga langkah yang sedang tampil, baris `welcome` built tanpa shortcut → AC-1, AC-3
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter StorageContentTests` → test lulus: default spec 0004 dengan `~`, roots daemon, daemon dengan override jatuh ke default, varian override tanpa nilai variabel, home `/Users/a` dengan path `/Users/ab/x` tidak disingkat → AC-6
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter OnboardingActionsTests` → test lulus: Get Started memanggil `FeedbackCenter.show` sekali dengan "Setup complete." dan pengumuman sekali; Continue hanya pindah; Escape di setiap langkah menulis record yang sama tanpa umpan balik → AC-2, AC-3, AC-11
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter OnboardingCopyTests` → test lulus: teks sama persis dengan tabel *Copy*, baris Verified runtimes hanya saat `runtimeInstallAvailable`, tanpa baris Environments, tanpa frasa terlarang, tanpa `URLSession` atau literal `http://` dan `https://`, tanpa `MutationPath` dan `publishesCommands`, `announce(` hanya di `OnboardingFlow.swift` → AC-4, AC-9, AC-11
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "ShellStructureTests|ShellLayoutTests|CommandMapTests|CommandSourceRuleTests|ShellCommandsTests"` → lulus: `MainWindowRoot` membuat satu `FeedbackCenter`, split view mengisi jendela terkecil, peta perintah dengan `welcome` built, penerbit shell → AC-3
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter OnboardingSnapshotTests` → test lulus (Welcome dan Storage, lima varian masing masing) terhadap PNG di `__Snapshots__/OnboardingSnapshotTests/` → AC-4, AC-6, AC-10
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` → seluruh suite lulus
- [x] `cargo xtask build-app` → build sukses
- [x] `xcrun swift-format lint --strict -r apps/macos/Packages/StacklyUI/Sources apps/macos/Packages/StacklyUI/Tests apps/macos/Stackly` → tanpa temuan

### Acceptance-criteria coverage

- AC-1 · model, record, dan `MainWindowRoot` (onboarding sebagai seluruh isi jendela); langkah manual tutup dan buka jendela ada di atas
- AC-2 · footer, titik langkah, dan `OnboardingFooter.primaryLabel` dengan `agentState` nil; label Not Now per state agent diisi view di Milestone 2
- AC-3 · Escape, Help "Welcome to Stackly", dan record; bukti log `FakeBackgroundAgentController` dan penghitung `MutationPath` menunggu test jalan lengkap di Milestone 4
- AC-4 · langkah Welcome dan aturan sumber jaringan; baris Verified runtimes menunggu `runtimeInstallAvailable` (fitur 18)
- AC-6 · langkah Storage dengan roots daemon, default, dan varian override
- AC-9 · `OnboardingCopy`, test copy bagian Welcome dan Storage, dan pindaian frasa terlarang; teks langkah lain dan baca ulang AC-061 penuh di Milestone 2, 3, dan 4
- AC-10 · titik langkah, fokus judul, pengumuman langkah, Escape, dan ukuran minimum; pengumuman `not_found` dan aturan fokus aksi kartu di Milestone 2; evidence keyboard dan VoiceOver di Milestone 4
- AC-11 · Get Started dan umpan balik "Setup complete."
- AC-5, AC-7, AC-8 · belum, milik Milestone 2 dan 3

## Milestone 2: langkah Background Agent dan Command Line

### UI / manual

- [ ] Bangun app (`cargo xtask build-app`), buka dengan `-onboarding.finished NO`, pindah ke langkah 2 tanpa menekan apa pun → kartu menampilkan state agent yang sebenarnya di Mac ini (biasanya "Background service is off" dengan tombol Turn On); `stackly daemon status --json` sebelum dan sesudah memberi blok `agent` yang sama → AC-3, AC-5
- [ ] Di langkah 2 baca footer → tombol utama "Not Now" selama state bukan `enabled`, "Continue" saat `enabled` → AC-2
- [ ] Dengan daemon tersambung dan agent `enabled` → kartu "Allowed in Login Items", "stacklyd starts when you log in.", dan status daemon "stacklyd running" di bawahnya; tanpa tombol aksi → AC-5
- [ ] Hentikan daemon (agent `enabled`) → kartu "stacklyd is starting…" dengan status "stacklyd unavailable"; tepat di detik ke 15 berganti ke "Allowed, but stacklyd isn’t running" dengan tombol Start stacklyd, tanpa menunggu polling → AC-5
- [ ] Tekan Turn On hanya setelah persetujuan lewat lead (mendaftarkan agent sungguhan; dicatat bersama record AC-17 spec 0015) → progress dan "stacklyd is starting…" sampai daemon tersambung, tanpa tombol Start stacklyd; hasil `requires_approval` menampilkan "Waiting for approval" dengan Open System Settings… dan fokus keyboard kembali ke tombol itu → AC-5, AC-10
- [ ] Di langkah 2 tanpa `STACKLY_ROOT` dan dengan `STACKLY_ROOT` → varian "STACKLY_ROOT is set" tanpa aksi pada yang kedua → AC-5
- [ ] VoiceOver di langkah 2: masuk langkah hanya mengucapkan "Step 2 of 5, Background Agent."; perubahan state agent diumumkan sekali (oleh app, bukan dua kali); `not_found` saat daemon tidak ada diumumkan sekali dengan "Stackly can’t find its background service." → AC-10
- [ ] Langkah 5 (Command Line) pada akun ini: toggle "Install stackly in ~/.local/bin" mati dan detail "No password needed. Stackly doesn’t change your PATH or shell files."; menyalakan toggle membuka konfirmasi "Install the stackly command?" dan Cancel tidak mengubah apa pun (`ls -l ~/.local/bin/stackly` tetap tidak ada, toggle kembali mati) → AC-8
- [ ] Ulangi dan tekan Install → `ls -l ~/.local/bin/stackly` menunjuk ke `…/Stackly.app/Contents/MacOS/stackly-cli`, `~/.local/bin/stackly --version` jalan, baris PATH `export PATH="$HOME/.local/bin:$PATH"` dengan tombol Copy ("Copied." terucap) dan "Or run it directly: …/Contents/MacOS/stackly-cli" tampil; matikan toggle dan Remove → link hilang, file shell tidak berubah (`stat` sebelum dan sesudah) → AC-8
- [ ] Blok contoh di langkah 5 hanya berisi `stackly daemon status` dan `stackly provider list`, tanpa `/usr/local/bin` dan tanpa "Requires your password once" → AC-8, AC-9
- [ ] Tekan Escape saat konfirmasi Command Line terbuka → hanya konfirmasi tertutup (Cancel); Escape berikutnya melewati onboarding → AC-3, AC-8

### Commands

- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter BackgroundAgentStepContentTests` → test lulus: setiap baris tabel *Kartu Background Agent* dengan jam yang disuntik (14 dan 15 detik), kedua kode koneksi, `ipc.call_timeout` dan `ipc.io_error`, `stopping`, `incompatible`, `untrusted`, `recovery`, `misconfigured` tanpa Start stacklyd, `unavailableDetail` nil dan ada, `isStarting` di 15 detik atau lebih tetap "stacklyd is starting…", `.refreshing(after:)` sebagai baris tersambung, `isWorking`, dan `lastError` → AC-5
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter BackgroundAgentFlowTests` → test lulus di atas `FakeBackgroundAgentController` dan `ManualSleep`: instalasi baru `not_registered` hanya memanggil `status`, Turn On memanggil `register` saja dan kartu menunggu daemon, `.daemonDidNotStart`, Turn On yang berakhir `requires_approval` atau `not_found`, state yang menjadi `enabled` di luar aksi, penantian yang berakhir saat `requires_approval`, Last error diumumkan `DaemonModel` sekali → AC-5, AC-10
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter OnboardingAnnouncementsTests` → test lulus: tabel `agentNotFound` per presence dan per pasangan state, dan untuk setiap presence tepat satu dari onboarding dan `DaemonModel` yang mengumumkan `not_found` → AC-10
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter CardFocusRuleTests` → test lulus: aturan fokus setelah aksi kartu (kembali ke aksi, ke tombol utama, tidak menarik fokus) → AC-10
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter CommandLineStepContentTests` → test lulus: setiap baris tabel *Kontrol Command Line* untuk kelima status, `bundledTool` nil, `isWorking`, error, dan teks blok contoh → AC-8
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter CommandLineFlowTests` → test lulus di home sementara: toggle lalu Cancel tidak menulis, Install membuat link, toggle mati lalu Remove, stale dan broken lewat Repair, conflict nonaktif dan file tidak tersentuh, error folder tidak aman tampil, `didBecomeActive()` membaca ulang status → AC-8
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter "OnboardingSnapshotTests|OnboardingAgentSnapshotTests|OnboardingCommandLineSnapshotTests"` → test lulus terhadap PNG di `__Snapshots__` (langkah 2 dan 5 dan setiap baris kartu dan kontrol, lima varian) → AC-5, AC-8, AC-10
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI` → seluruh suite lulus
- [ ] `cargo xtask build-app` → build sukses

### Acceptance-criteria coverage

- AC-2 · label tombol utama `Not Now` dan `Continue` di langkah 2 dari `DaemonModel.agent?.assessment?.state`
- AC-5 · kartu Background Agent, semua baris tabel, jadwal per detik, aksi hanya lewat `turnOn()`, `openLoginItems()`, `start()`, `refresh()`; bukti alur Turn On sungguhan menunggu persetujuan di Milestone 4
- AC-8 · langkah Command Line dengan toggle berkonfirmasi, Repair, catatan PATH, baris "Or run it directly", dan blok contoh; evidence di akun nyata di Milestone 4
- AC-10 · pengumuman `not_found` oleh `OnboardingFlow`, aturan fokus aksi kartu, dan Last error diumumkan `DaemonModel`; evidence keyboard dan VoiceOver penuh di Milestone 4
- AC-7 · belum, milik Milestone 3

## Milestone 3: Catalog Trust

### UI / manual

- [ ] Bangun app (`cargo xtask build-app`), buka dengan `-onboarding.finished NO` dan daemon berjalan, pindah ke langkah 4 → judul "Catalog sources", subjudul, satu baris berlabel "official" dengan detail "Maintained by the Stackly project and bundled with this app.", baris "Revision {n} · published {tanggal}", toggle menyala dan nonaktif, dan catatan trust di bawahnya; revision dan tanggal sama dengan `stackly provider list` → AC-7
- [ ] Arahkan pointer ke toggle dan baca dengan VoiceOver → help dan label "The official catalog is always on."; toggle tidak bisa diubah → AC-7, AC-10
- [ ] Jalankan stacklyd dengan `STACKLY_CATALOG_DIR` (catalog pengembangan) → label "development", detail menyebut STACKLY_CATALOG_DIR dan bahwa itu bukan catalog official, toggle "The development catalog is always on." → AC-7
- [ ] Buka langkah 4 saat daemon belum berjalan → label "Catalog", detail "Stackly shows where its catalog comes from once stacklyd is running.", tanpa toggle; nyalakan daemon → baris berubah menjadi "official" tanpa pindah langkah → AC-7
- [ ] Baca seluruh teks langkah 4 terhadap AC-061 → tidak ada baris community, tidak ada "Signed manifests", tidak ada kalimat tentang catalog terbaru atau up to date, tidak ada kalimat digest (`runtimeInstallAvailable` masih false) → AC-7, AC-9

### Commands

- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter CatalogTrustContentTests` → test lulus: setiap baris tabel *Baris Catalog Trust* (official, official outdated, development, `source` dan `trust` `unrecognized`, unsupported, error dengan dan tanpa hint, menunggu), kalimat digest mengikuti `runtimeInstallAvailable`, tanpa toggle saat `info` nil, `info` lama menang atas refresh yang gagal, tanpa community dan frasa terlarang, catatan trust sama dengan tabel *Copy*, dan baris dibangun dari `CatalogStore` di atas fixture `protocols/fixtures/catalog/` → AC-7, AC-9
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter OnboardingCopyTests` → lulus, pindaian frasa terlarang meliputi `CatalogTrustContent.swift`, `CatalogTrustStep.swift`, dan bagian Catalog Trust `OnboardingCopy.swift` → AC-9
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter CatalogTrustSnapshotTests` → lulus (tujuh baris dalam satu susunan, lima varian) terhadap PNG di `__Snapshots__/CatalogTrustSnapshotTests/` → AC-7, AC-10
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI` → seluruh suite lulus
- [ ] `cargo xtask build-app` → build sukses
- [ ] `xcrun swift-format lint --strict -r apps/macos/Packages/StacklyUI/Sources apps/macos/Packages/StacklyUI/Tests apps/macos/Stackly` → tanpa temuan

### Acceptance-criteria coverage

- AC-7 · `CatalogTrustContent.make`, `CatalogTrustStep` dan `CatalogTrustRow`; baris yang bergantung pada `runtimeInstallAvailable` (kalimat digest) menunggu fitur 18
- AC-9 · bagian Catalog Trust `OnboardingCopy` dan test per nilai; pembacaan ulang seluruh copy terhadap AC-061 menunggu Milestone 4
- AC-10 · label dan help toggle; snapshot langkah di jendela penuh dan evidence VoiceOver di Milestone 4

## Milestone 4: jalan lengkap dan bukti manual

### UI / manual

_Langkah 8 spec 0016 (evidence manual, kind `native`, status awal `NOT_RUN`). Semuanya di bundle nyata dari `cargo xtask build-app --install` di `~/Applications`, dan hanya kamu yang menjalankannya. Sesi pembangun tidak menjalankan `--install` dan tidak menekan Turn On._

- [ ] `NOT_RUN` Buka bundle dengan `-onboarding.finished NO` → lima langkah tampil; langkah 2 menampilkan state agent yang sebenarnya di Mac ini (biasanya "Background service is off", termasuk untuk bundle yang belum pernah didaftarkan) tanpa menekan Turn On; `stackly daemon status --json` sebelum dan sesudah jalan lengkap memberi blok `agent` yang sama; bila daemon berjalan, Storage menampilkan roots yang sama dengan `info.roots` di output itu, dan Catalog Trust menampilkan revision dan tanggal yang sama dengan `stackly provider list` → AC-3, AC-5, AC-6, AC-7
- [ ] `NOT_RUN` Turn On dari onboarding, hanya setelah persetujuan user lewat lead (mendaftarkan agent sungguhan; dicatat bersama record AC-17 spec 0015, cabang A, B, atau C, dan tetap `BLOCKED` sampai ada signing identity, OQ-012, bila build ad hoc ditolak) → bila hasilnya `enabled`, kartu menampilkan progress dan "stacklyd is starting…" sampai daemon tersambung, tanpa menawarkan Start stacklyd → AC-5
- [ ] `NOT_RUN` Command Line di akun ini: toggle Install lalu Install, `ls -l ~/.local/bin/stackly` menunjuk ke `…/Stackly.app/Contents/MacOS/stackly-cli`, `~/.local/bin/stackly --version` jalan, Remove, dan file shell tidak berubah (`stat` sebelum dan sesudah) → AC-8
- [ ] `NOT_RUN` Keyboard saja dan VoiceOver dari langkah 1 sampai Get Started, lalu Escape dari langkah 3; light, dark, Increase Contrast, Reduce Transparency, dan jendela 900 × 600 → AC-10, AC-11
- [ ] `NOT_RUN` Baca ulang semua copy terhadap AC-061: tidak ada janji sandbox, isolasi dari file user, "up to date", "latest", `/usr/local/bin`, atau "only while the app is open"; catat hasilnya → AC-9
- [ ] `NOT_RUN` `cargo xtask ui-test --only OnboardingUITests` → lulus; test ini tetap `NOT_RUN` sampai kamu menyalakan Automation Mode (docs/manual.md bagian K): tanpa itu task berhenti dengan exit 2 dan tidak ada tes yang jalan. Target `StacklyAppUITests` terkompilasi (`xcodebuild build-for-testing`) → AC-1, AC-2, AC-3, AC-11

### Commands

- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter OnboardingWalkTests` → test lulus: jalan lengkap dengan Continue, Not Now, Back, titik, dan Get Started, lalu Escape di setiap langkah, dengan view nyata di jendela, di atas `FakeBackgroundAgentController`, `CommandLineTool` di home sementara, dan `CountingLink`: log agent hanya `status` (dan setidaknya satu saat langkah 2 tampil), home sementara tidak berubah, link tidak mengirim satu pun call, record ditulis sekali, "Setup complete." hanya sesudah Get Started dan tidak sesudah Escape; menu Help dari shell dan saat onboarding tampil; folder `Onboarding/` tidak menyentuh `shell.section` atau `FileManager` → AC-3, AC-11
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter OnboardingCopyTests` → test lulus: setiap nilai sama dengan tabel *Copy*, tanpa frasa terlarang, tanpa jaringan, dan tidak ada kalimat di luar `OnboardingCopy.swift` → AC-4, AC-9
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter "BackgroundAgentStepContentTests|CatalogTrustContentTests|CommandLineStepContentTests|StorageContentTests"` → test lulus: setiap tabel copy (*Kartu Background Agent*, *Baris Catalog Trust*, *Kontrol Command Line*, Storage) sama persis dengan spec → AC-5, AC-6, AC-7, AC-8, AC-9
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter "OnboardingSnapshotTests|OnboardingAgentSnapshotTests|OnboardingCommandLineSnapshotTests|CatalogTrustSnapshotTests"` → test lulus terhadap PNG: satu per langkah (kelima langkah) dan satu matriks per tabel (kartu agent, baris catalog trust, kontrol Command Line), lima varian masing masing; varian Reduce Transparency sama dengan light karena onboarding tidak memakai material sistem → AC-5, AC-7, AC-8, AC-10
- [ ] `xcodebuild build-for-testing -project apps/macos/Stackly.xcodeproj -scheme Stackly -configuration Debug -destination "platform=macOS,arch=arm64" -derivedDataPath target/xcode -only-testing StacklyAppUITests` → `TEST BUILD SUCCEEDED` dengan `OnboardingUITests` → AC-1, AC-2, AC-3, AC-11
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI` → seluruh suite lulus
- [ ] `cargo xtask build-app` → build sukses

### Acceptance-criteria coverage

- AC-3 · test jalan lengkap tanpa efek samping (Milestone 4), model dan Escape (Milestone 1); XCUITest Escape di langkah 3 `NOT_RUN` sampai Automation Mode
- AC-5 · evidence Turn On sungguhan menunggu persetujuan user dan record AC-17 spec 0015
- AC-8 · evidence di akun nyata menunggu langkah manual di atas
- AC-9 · test copy dan pindaian sumber otomatis; baca ulang AC-061 manual
- AC-10 · preview, snapshot, dan test unit; evidence keyboard dan VoiceOver manual
- AC-11 · test unit dan jalan lengkap; XCUITest Get Started `NOT_RUN` sampai Automation Mode
