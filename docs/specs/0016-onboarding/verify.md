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
- [ ] Baca ulang semua teks langkah 1 sampai 3 terhadap AC-061 → tidak ada janji sandbox, isolasi dari file user, atau fitur yang belum ada → AC-9

### Commands

- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter OnboardingModelTests` → test lulus: record kosong dan true, key `onboarding.finished` di `UserDefaults`, lima langkah dan label, `next` dan `back` berhenti di ujung, titik langkah, label `primaryLabel` per langkah dan per state agent (nil dan keenam state), Back mulai langkah 2, `finish` kedua alasan menulis record yang sama, `present` mulai dari langkah 1 atau menjaga langkah → AC-1, AC-2, AC-3
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter MainWindowRootTests` → test lulus: onboarding menjadi seluruh isi jendela (tanpa split view) sampai selesai, shell saat record true, pindah ke shell saat onboarding selesai, runtime menyimpan langkah, aksi Help "Welcome to Stackly" menampilkan langkah 1 tanpa menulis record dan menjaga langkah yang sedang tampil, baris `welcome` built tanpa shortcut → AC-1, AC-3
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter StorageContentTests` → test lulus: default spec 0004 dengan `~`, roots daemon, daemon dengan override jatuh ke default, varian override tanpa nilai variabel, home `/Users/a` dengan path `/Users/ab/x` tidak disingkat → AC-6
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter OnboardingActionsTests` → test lulus: Get Started memanggil `FeedbackCenter.show` sekali dengan "Setup complete." dan pengumuman sekali; Continue hanya pindah; Escape di setiap langkah menulis record yang sama tanpa umpan balik → AC-2, AC-3, AC-11
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter OnboardingCopyTests` → test lulus: teks sama persis dengan tabel *Copy*, baris Verified runtimes hanya saat `runtimeInstallAvailable`, tanpa baris Environments, tanpa frasa terlarang, tanpa `URLSession` atau literal `http://` dan `https://`, tanpa `MutationPath` dan `publishesCommands`, `announce(` hanya di `OnboardingFlow.swift` → AC-4, AC-9, AC-11
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter "ShellStructureTests|ShellLayoutTests|CommandMapTests|CommandSourceRuleTests|ShellCommandsTests"` → lulus: `MainWindowRoot` membuat satu `FeedbackCenter`, split view mengisi jendela terkecil, peta perintah dengan `welcome` built, penerbit shell → AC-3
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter OnboardingSnapshotTests` → test lulus (Welcome dan Storage, lima varian masing masing) terhadap PNG di `__Snapshots__/OnboardingSnapshotTests/` → AC-4, AC-6, AC-10
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI` → seluruh suite lulus
- [ ] `cargo xtask build-app` → build sukses
- [ ] `xcrun swift-format lint --strict -r apps/macos/Packages/StacklyUI/Sources apps/macos/Packages/StacklyUI/Tests apps/macos/Stackly` → tanpa temuan

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
