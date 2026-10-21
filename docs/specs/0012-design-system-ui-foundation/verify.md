# Verify: Design system dan fondasi UI · spec 0012

**Dijalankan**: 2026-10-18, macOS 27.0 (26A428), Xcode 27.0 (27A266a), build Debug `target/xcode/Build/Products/Debug/Stackly.app` dari branch `claude/f12-verify-test-cfb188`. Dijalankan ulang 2026-10-21 di macOS 27.0.1 (26A434), Xcode 27.0 (27A266a), dari `main` `d1338e4` (branch `claude/nice-banach-103b7f`): semua baris `## Commands`, plus langkah UI yang jalurnya berubah sejak 2026-10-18. Yang berubah hanya cangkang galeri (halaman Secrets spec 0011 di `DesignGallery.swift`, bantuan Narrow Width dan `GalleryLayout.narrowWidth` final di `GallerySupport.swift`), `StacklyApp.swift` (jendela utama spec 0013, galeri menerima runtime daemon), dan `design.md` (bagian App shell). Kode `Tokens/`, `Status/`, `State/`, dan `Confirmation/` tidak berubah, jadi langkah lainnya tidak diulang. App Debug dijalankan dengan `STACKLY_ROOT` ke folder sementara, tidak pernah ke Application Support asli. Bukti ulang ada di folder catatan bersama session koordinator wave 1 (`chk-f12-verify-evidence/`).

**Hasil**: BLOCKED. Semua cek yang bisa dijalankan lulus, juga pada jalan ulang 2026-10-21. Empat cek yang butuh setting sistem asli (Increase Contrast, Reduce Transparency, Differentiate Without Color, Reduce Motion) dijalankan pengguna sendiri (keputusan pengguna 2026-10-21) dan hasilnya belum masuk. Karena itu kotak Verify it di scope belum dicentang.

**Cara menjalankan**: app Debug dibuka sebagai instance kedua, lalu dikendalikan lewat pohon aksesibilitas (AX) dan tombol keyboard nyata (CGEvent), dengan tangkapan jendela (`screencapture -l`) dan rekaman layar pendek (`screencapture -v`) yang frame nya dianalisis. Skrip pemeriksa dan bukti mentah ada di folder catatan bersama session koordinator (`feature-12-verify-harness.swift` dan `feature-12-verify-evidence/`), bukan di repo. Appearance light dipaksa per app dengan argumen `-NSRequiresAquaSystemAppearance YES`; argumen `-AppleInterfaceStyle` tidak berpengaruh di macOS 27.

## UI / manual

- [x] Menu Window di app Debug berisi "Design Gallery", dan perintahnya membuka jendela galeri dengan halaman Tokens, Status, States, Confirmation → AC-18 · ulang 2026-10-21: lulus; sidebar galeri sekarang Tokens, Status, States, Confirmation, Secrets (halaman Secrets milik spec 0011, di luar AC-18)
- [x] Halaman Tokens menampilkan setiap token warna (empat token kustom plus `decorative`), lima tone, dua belas peran tipografi dengan text style dan ukuran macOS nya, skala spacing, radius, ukuran berskala, dan motion → AC-18
- [x] Halaman Status menampilkan `StatusLabel` untuk ketujuh belas contoh tabel Status instance dalam konteks standard dan row, kolom tampilan Differentiate Without Color, `ToneBadge` untuk kedelapan state operasi, dan ketujuh ikon bagian, di light dan dark → AC-4, AC-5, AC-18, AC-19
- [x] VoiceOver (pohon AX): setiap `StatusLabel` adalah satu elemen "Status: <label>" ditambah ", <detail>" (misalnya "Status: Starting…, Waiting for probe"); 51 elemen untuk 17 contoh dalam tiga kolom → AC-5, AC-17 · ulang 2026-10-21: 51 elemen, 17 label unik
- [x] Ikon bagian tidak muncul di pohon AX, hanya labelnya (Overview sampai Diagnostics) → AC-19
- [x] Konteks row: label dan detail memakai warna utama; baris terpilih di daftar yang difokus (seleksi biru) membuat titik dan simbol status ikut putih → AC-5
- [x] Narrow Width (640 pt): tidak ada label status yang terpotong, detail terpotong lebih dulu dengan "…", teks catatan membungkus; di keempat halaman → AC-5, AC-18 · ulang 2026-10-21: lulus, area halaman 640 pt lebar (AX), bantuan tombol "Show the page at 640 pt, the minimum content width"
- [x] Halaman States menampilkan setiap bentuk: loading, empty no data (dua aksi), empty no results ("No Results for “mysql-legacy-archive”" dengan Clear Filters), failed (judul, pesan daemon, kode monospace, Open Diagnostics, Retry), unsupported (judul, alasan, tanpa tombol), loaded, loaded stale, empty stale outdated, dan loaded partial → AC-8, AC-10, AC-11, AC-12, AC-13, AC-14
- [x] Placeholder dan banner adalah satu grup AX yang labelnya judul; banner per tone tampil dengan simbol dan judul → AC-11, AC-17
- [x] Aksi utama placeholder tampil `.borderedProminent` (biru saat jendela aktif) dan aksi sekunder `.bordered`; jendela galeri tidak punya tombol default (`AXDefaultButton` none) → AC-10
- [x] Waktu banner stale mengikuti region Mac ini ("12.17", "17/10/2026, 10.35") → AC-12
- [x] Refreshing: kedua tombol Retry di banner stale menjadi nonaktif dan dua spinner kecil muncul; Retry di placeholder failed tetap aktif → AC-12
- [x] Daemon Unreachable: New Instance…, Start, Back Up Now, dan Use Port 3308 nonaktif dengan bantuan "stacklyd is unavailable. Actions are disabled until the daemon is reachable."; Browse Catalog, Clear Filters, Retry, Open Diagnostics, Open, Show Logs, Review Changes, Reconnect, dan Copy Endpoint tetap aktif → AC-20
- [x] Back Up Now unsupported nonaktif dengan alasan sebagai teks yang terlihat di bawahnya dan sebagai bantuan → AC-13
- [x] Partial: lima item tampil, lalu "and 2 more" dan Show All (area klik 50 × 24 pt); Show All membuka dua item sisanya di tempat, lewat AXPress dan lewat Space → AC-14, AC-17
- [x] Keyboard (Keyboard navigation ON di Mac ini): Tab mencapai setiap tombol placeholder, Retry di banner, Open dan Start di item partial, Show All, tombol banner, dan tombol gate; Space menekan Show All → AC-17
- [x] Loading: dari rekaman 60 fps, spinner mulai tampil sekitar 0,33 detik setelah bentuk masuk loading, dan masuk loading lagi memulai jeda dari awal → AC-9
- [x] Pergantian bentuk (Loaded, Loading, Empty, Failed, Loaded): setiap pergantian adalah crossfade sekitar 0,17 sampai 0,2 detik; isi lama memudar dan isi baru muncul di posisi yang sama, tanpa gerak atau skala → AC-16
- [x] Pengumuman VoiceOver (AXObserver `AXAnnouncementRequested`): membuka halaman States mengumumkan "Couldn’t start: port in use" (banner critical) dan "Couldn’t Load Instances" masing masing sekali; setiap pindah ke Failed mengumumkan "Couldn’t Load Instances" tepat sekali → AC-11, AC-17
- [x] Konfirmasi destructive ("Remove pg17-dev…", "Stop mysql-erp…"): sheet alert, Cancel adalah tombol default; Return menutup dan "Nothing ran"; Escape menutup dan "Nothing ran" → AC-15
- [x] Konfirmasi standard ("Bring Down erp-development…"): Bring Down adalah tombol default; Return menjalankan aksi; Escape membatalkan → AC-15
- [x] Menekan tombol Remove menjalankan aksi → AC-15
- [x] Tab lalu Space: di dialog destructive Tab memindahkan fokus dari Cancel ke Remove dan Space menjalankannya; di dialog standard Tab memindahkan fokus dari Cancel ke Bring Down dan Space menjalankannya → AC-15, AC-17
- [x] Jalur tanpa jendela (`runModal()`, `ConfirmationPresenter` tanpa anchor, dicek dengan test sementara): alert tampil app modal dengan judul dan pesan request; Return dan Escape memberi jawaban yang benar untuk kedua role saat alert aktif; klik Remove mengonfirmasi; `close()` mengakhiri modal tanpa jawaban → AC-15
- [ ] Differentiate Without Color dengan setting sistem asli: titik berganti simbol di semua `StatusLabel`, simbol tone di `ToneBadge` → AC-5, AC-18 · **BLOCKED** (2026-10-18 dilewati; tampilan yang sama sudah terlihat lewat kolom galeri yang memaksanya). 2026-10-21: dijalankan pengguna sendiri, hasil belum masuk
- [ ] Increase Contrast dengan setting sistem asli: teks HC, border 1 pt di kartu, banner, dan badge → AC-7 · **BLOCKED** (2026-10-18 dilewati; appearance HC milik AppKit di jendela test tidak mengubah `colorSchemeContrast`, jadi tidak bisa diganti cara lain). 2026-10-21: dijalankan pengguna sendiri, hasil belum masuk
- [ ] Reduce Transparency dengan setting sistem asli: permukaan kustom tidak berubah, glass sistem opak → AC-6, AC-7 · **BLOCKED** (2026-10-18 dilewati). 2026-10-21: dijalankan pengguna sendiri, hasil belum masuk
- [ ] Reduce Motion dengan setting sistem asli: tidak ada gerak, crossfade tetap, progress tanpa animasi → AC-16 · **BLOCKED** (2026-10-18 dilewati). 2026-10-21: dijalankan pengguna sendiri, hasil belum masuk

## Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI` → 2026-10-21: 315 test di 38 suite lulus, seluruh paket (2026-10-18: 99 test di 13 suite); keempat belas suite F12 lulus: ColorTokenTests, ConfirmationPresentationTests, ConfirmationTests, ContrastTests, DelayedVisibilityTests, LayoutTokenTests, SourceRuleTests, StaleNoteTests, StateViewPartsTests, StatusAppearanceTests, StatusViewTests, ToneTests, TypographyTests, ViewStateTests → AC-2, AC-3, AC-4, AC-6, AC-8, AC-9, AC-12, AC-15
- [x] `cargo xtask build-app` → BUILD SUCCEEDED (Debug), juga 2026-10-21 → AC-18
- [x] `cargo xtask build-app --release`, lalu `strings` pada setiap file di bundle Release → 0 string "Design Gallery" atau `design-gallery` (Debug: 2), sama pada 2026-10-21 → AC-18
- [x] `grep '^## ' apps/macos/design.md` → Typography, Color, Spacing and radius, Materials, Status, State patterns, Confirmation, Motion, Accessibility → AC-1 · 2026-10-21: kesembilan bagian itu ada, ditambah App shell (spec 0013) di akhir
- [x] `AccentColor.colorset/Contents.json` → hanya `idiom` universal, tanpa nilai warna → AC-6 · sama pada 2026-10-21
- [x] `grep -c '#Preview'` pada keempat file halaman galeri → 3 per halaman (Light, Dark, Narrow width) → AC-18 · sama pada 2026-10-21 (halaman Secrets spec 0011 punya 1, di luar AC-18)

## Acceptance-criteria coverage

- AC-1 met ✅ (design.md dengan sembilan bagian)
- AC-2 met ✅ (test aturan sumber)
- AC-3 met ✅ (ContrastTests di empat appearance)
- AC-4 met ✅ (test per baris dan galeri)
- AC-5 sebagian: label, VoiceOver, konteks row, seleksi, lebar sempit met ✅; Differentiate Without Color dengan setting asli BLOCKED ⚠️
- AC-6 met ✅ untuk aturan sumber dan AccentColor; bagian Reduce Transparency BLOCKED ⚠️
- AC-7 BLOCKED ⚠️ (Increase Contrast dan Reduce Transparency butuh setting asli)
- AC-8 met ✅
- AC-9 met ✅ (rekaman dan DelayedVisibilityTests)
- AC-10 met ✅
- AC-11 met ✅
- AC-12 met ✅
- AC-13 met ✅
- AC-14 met ✅
- AC-15 met ✅ (jalur sheet di app dan jalur `runModal()`)
- AC-16 sebagian: crossfade tanpa gerak met ✅; Reduce Motion BLOCKED ⚠️
- AC-17 met ✅
- AC-18 met ✅ (pengguna memutuskan 2026-10-18 bahwa preview Narrow width 640 pt adalah cara teks besar yang dicatat langkah 4)
- AC-19 met ✅
- AC-20 met ✅

## Untuk /check review

- `ToneBadge` muncul di pohon AX dengan role `AXUnknown` (labelnya benar). VoiceOver tetap membaca labelnya, tapi role statis teks lebih rapi.
- Setelah Show All ditekan dengan Space, fokus keyboard melompat kembali ke kontrol pertama halaman, karena tombol Show All hilang. Lebih enak kalau fokus pindah ke item pertama yang baru terbuka.
- `GalleryLayout.narrowWidth` masih bertanda `// provisional: F13`, padahal spec 0013 sudah menetapkan konten tidak pernah lebih sempit dari 640 pt.
