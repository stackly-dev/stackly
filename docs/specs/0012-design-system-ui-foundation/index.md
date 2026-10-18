# 0012. Design system dan fondasi UI

**Date**: 2026-10-18
**Status**: In Progress

## Summary

Spec ini menetapkan bahasa visual aplikasi macOS Stackly dan pola yang dipakai semua layar: tipografi dari text style bawaan macOS, warna netral dari sistem kecuali teks sekunder dan permukaan kartu, palet status lima tone dengan warna teks yang lolos WCAG AA (dan 7:1 saat Increase Contrast), Liquid Glass hanya dari kontrol native, lima bentuk state halaman (loading, empty, failed, unsupported, dan loaded yang bisa membawa keterangan stale atau partial), serta satu pola konfirmasi di atas `NSAlert` native (bukan `confirmationDialog`, yang gagal cek Return dan Escape di macOS 27). Aturannya ditulis untuk builder di `apps/macos/design.md` (bahasa Inggris). Kodenya ada di package `StacklyUI`: token, `StatusLabel`, `ToneBadge`, `ViewState` beserta view state, gate mutasi lewat environment, modifier konfirmasi, dan galeri, dengan test kontras dan test aturan sumber yang otomatis. Fitur 13, 14, 67 sampai 69, dan semua layar journey membangun di atasnya tanpa memutuskan ulang warna, state, atau konfirmasi.

## Requirements

**User stories**:

- Sebagai pengguna dengan Increase Contrast, Reduce Transparency, Differentiate Without Color, atau teks besar, saya ingin setiap layar tetap terbaca dan status tetap bisa dibedakan tanpa warna, supaya saya bisa memakai Stackly sepenuhnya.
- Sebagai developer layar journey (fitur 16 sampai 56), saya ingin token, status, state, dan konfirmasi sudah tersedia sebagai API `StacklyUI`, supaya saya merakit layar tanpa memutuskan ulang warna atau perilaku state.
- Sebagai pengguna yang akan menghapus atau menimpa data, saya ingin selalu melihat dialog yang sama yang menyebut akibatnya, supaya saya tidak kehilangan data karena salah klik atau menekan Return.
- Sebagai pengguna saat daemon tidak terjangkau, saya ingin tetap melihat state terakhir beserta waktunya dan tahu kenapa aksi nonaktif, tanpa aplikasi mengklaim instance sudah berhenti.

**Acceptance criteria** (ID lokal spec ini ditulis `AC-n`; ID PRD selalu tiga digit dan disebut "PRD AC-0nn"):

- **AC-1**: `apps/macos/design.md` ada, berbahasa Inggris, dan memuat bagian Typography, Color, Spacing and radius, Materials, Status, State patterns, Confirmation, Motion, dan Accessibility. Setiap token menyebut nilai atau token mockup asalnya dan nama API Swift nya, dan setiap aturan tampilan di spec ini tertulis di sana.
- **AC-2**: Teks di `StacklyUI` dan target app hanya memakai text style semantik dari tabel Tipografi. Ukuran tetap hanya boleh di helper `.stacklySymbol(size:relativeTo:)` untuk simbol. Angka (hitungan, port, waktu, ukuran) memakai `.monospacedDigit()`, dan path, endpoint, digest, serta kode error memakai `.monospaced()`. Test aturan sumber gagal kalau ada `.font(.system(size:`, `Font.system(size:`, `.custom(`, atau `NSFont(` di luar helper itu.
- **AC-3**: Untuk setiap baris tabel Pasangan kontras, kontras memenuhi batas normal di appearance light dan dark dan batas HC di appearance high contrast light dan dark. `StacklyUITests` membuktikannya dengan menghitung dari tabel token dan dari warna sistem yang di resolve per `NSAppearance`. Test aturan sumber gagal kalau `.secondary`, `.tertiary`, `secondaryLabelColor`, `tertiaryLabelColor`, `Color.accentColor`, atau `.tint` dipakai sebagai warna teks di luar file token.
- **AC-4**: `StatusAppearance.instance(lifecycle:readiness:health:)` menghasilkan label, tone, simbol, dan detail persis seperti tabel Status instance untuk setiap baris, dan `StatusAppearance.operation(state:partial:)` menghasilkan label dan tone persis seperti tabel Status operasi. Test mencakup setiap baris, dan setiap nama SF Symbol di spec ini ada di macOS 27.
- **AC-5**: `StatusLabel` selalu menampilkan label teks. Normalnya titik berwarna ada di depan label; saat Differentiate Without Color aktif, titik diganti simbol status. VoiceOver membaca satu elemen "Status: <label>" ditambah detail bila ada. Dalam konteks `.row` (baris List dan Table, sidebar, menu, popover), label dan detail memakai `.primary`, dan saat `backgroundProminence == .increased` titik dan simbol juga memakai `.primary`. Dalam konteks `.standard`, warna teks mengikuti tabel Tone. Pada lebar terkecil yang diuji, label tidak terpotong; detail yang dipotong lebih dulu.
- **AC-6**: Tidak ada `.glassEffect`, `GlassEffectContainer`, `.buttonStyle(.glass)`, `.buttonStyle(.glassProminent)`, atau `Material` kustom di `StacklyUI` dan target app; glass hanya datang dari sidebar, toolbar, sheet, popover, menu, dialog, dan inspector native. Tidak ada setting accent atau mode glass di dalam aplikasi, dan `AccentColor.colorset` tetap tanpa nilai warna.
- **AC-7**: Saat Increase Contrast aktif, teks tone dan `textSecondary` memakai varian HC, dan `.cardSurface()`, `StateBanner`, serta `ToneBadge` mendapat border 1 pt (`separatorColor` untuk kartu, warna fill tone untuk banner dan badge). Warna teks dan border membaca sumber yang sama (`colorSchemeContrast`). Saat Reduce Transparency aktif, tidak ada permukaan kustom yang tembus pandang (semua permukaan kustom opak sejak awal), dan permukaan sistem diurus macOS.
- **AC-8**: `ViewState.resolve` memilih bentuk dengan urutan tetap: unsupported, lalu failed (hanya saat belum ada data), lalu loading (belum ada data), lalu empty, lalu loaded. Kalau data pernah berhasil dimuat, refresh yang gagal menghasilkan empty atau loaded dengan keterangan stale, tidak pernah failed. Halaman tidak pernah menampilkan empty sebelum ada data yang berhasil dimuat (PRD AC-024).
- **AC-9**: Loading pertama menampilkan `ProgressView` di tengah hanya setelah 300 ms; kalau data datang lebih cepat, spinner tidak pernah tampil. Jeda dimulai ulang setiap kali bentuk masuk ke loading. Refresh berikutnya tidak mengganti konten dengan spinner. Logika jeda dites dengan fungsi sleep yang diganti, tanpa menunggu waktu nyata.
- **AC-10**: Empty punya dua jenis: no data (simbol, judul, penjelasan, dan sampai dua aksi) dan no results (judul "No Results for “<query>”", saran, dan aksi Clear Filters). Aksi utama memakai `.borderedProminent`, aksi sekunder `.bordered`, dan tidak ada yang menjadi tombol default. `StatePlaceholder` punya ukuran halaman (judul `.title2`) dan ukuran seksi (judul `.title3`), dan baris kosong satu kalimat (misalnya "Nothing running.") memakai `InlineEmptyRow`.
- **AC-11**: Failed di tingkat halaman menampilkan simbol critical, judul, pesan dari daemon, kode error monospace (bila ada), tombol Retry, dan Open Diagnostics. Error per item tampil sebagai `StateBanner` critical di item itu (judul, isi, blok kode opsional, sampai tiga aksi), dan bagian lain halaman tetap tampil.
- **AC-12**: Stale yang khusus halaman (refresh gagal, data kedaluwarsa) menampilkan `StateBanner` caution di atas konten, untuk bentuk empty maupun loaded, dengan kalimat "Showing the last known state from <waktu>." beserta alasannya dan tombol Retry, tanpa meredupkan konten. Selama `isRefreshing`, tombol Retry nonaktif dan menampilkan progress kecil. Daemon tidak terjangkau tidak termasuk stale halaman: shell (spec 0013) menampilkan satu banner untuk seluruh jendela dan mengisi `MutationAvailability`.
- **AC-13**: Unsupported tingkat halaman atau seksi menampilkan simbol `nosign`, judul, dan alasannya, tanpa tombol aksi untuk capability itu. Tombol capability unsupported di tempat lain tampil nonaktif lewat `.disabled(reason:)`, dengan alasan sebagai teks yang terlihat (`DisabledReasonText`) dan lewat `.help`, yang di macOS juga menjadi bantuan VoiceOver; alasan tidak pernah hanya ada di tooltip (PRD AC-027). Tombol capability baru digambar setelah data layar, termasuk capability, selesai dimuat.
- **AC-14**: Partial menampilkan `StateBanner` caution dengan ringkasan (misalnya "5 of 6 services ready") dan daftar item yang belum siap beserta alasannya dan aksi opsional per item. Banner menampilkan paling banyak lima item, lalu "and N more" dengan tombol Show All yang membuka sisanya di tempat. Item yang sukses tetap tampil normal.
- **AC-15**: Setiap konfirmasi di aplikasi lewat satu modifier `.stacklyConfirmation(item:)`. Peran destructive: tombol aksi destruktif (merah, digambar sistem lewat `hasDestructiveAction`), gaya alert critical, dan Cancel adalah tombol default yang menerima Return dan Escape, jadi Return tidak pernah menjalankan aksi destruktif. Peran standard: tombol aksi adalah default, Cancel menerima Escape. Judul berupa pertanyaan yang menyebut objek dan tidak pernah kosong, pesan menyebut akibat dan apa yang tetap aman, dan label tombol berupa kata kerja dari judul (tidak pernah "OK" atau "Yes"), dan tombol batal selalu "Cancel". Tombol aksi destruktif dicapai dengan Tab lalu Space saat Keyboard navigation menyala di System Settings. Test aturan sumber gagal kalau ada `confirmationDialog(`, `.alert(`, atau `NSAlert` di luar folder `Confirmation/`.
- **AC-16**: Tidak ada view fitur 12 yang memakai animasi gerak atau skala (`.move`, `.slide`, `.scale`, `.offset` yang dianimasikan) di setting apa pun; perubahan status, banner, dan bentuk state memakai crossfade 0,2 detik. Dengan Reduce Motion aktif, perubahan nilai progress tidak dianimasikan dan crossfade tetap boleh. Spinner tetap berputar.
- **AC-17**: Semua view fitur 12 bisa dipakai penuh dari keyboard (setiap tombol di placeholder, banner, dan dialog bisa difokus dengan Tab dan dijalankan dengan Space saat Keyboard navigation menyala di System Settings, seperti kontrol macOS lainnya), punya label VoiceOver, dan area klik kustom (bila ada, misalnya tombol Show All) minimal 24 × 24 pt.
- **AC-18**: Galeri desain di `StacklyUI` menampilkan setiap token warna dan tipografi, `StatusLabel` untuk semua baris tabel Status instance dalam kedua konteks, `ToneBadge` untuk semua baris tabel Status operasi, setiap bentuk `ViewState`, `StateBanner` per tone, dan contoh konfirmasi destructive dan standard. Setiap halaman galeri punya `#Preview` light, dark, dan lebar sempit 640 pt sebagai cek teks besar, karena langkah 4 membuktikan text style tidak membesar dengan `dynamicTypeSize` di macOS 27; 640 pt adalah lebar konten minimum yang ditetapkan spec 0013. Build Debug app membuka galeri di jendela "Design Gallery" supaya setting aksesibilitas sistem bisa dicek di app nyata. Galeri tidak ikut build Release.
- **AC-19**: `SectionStyle` memberi simbol dan warna sistem per bagian (Overview, Instances, Environments, Catalog, Runtimes, Operations, Diagnostics) sesuai tabel Ikon bagian, dan view `SectionIcon` menggambarnya dengan `accessibilityHidden(true)` karena label bagian yang membawa arti.
- **AC-20**: `MutationAvailability` di environment bernilai `.available` atau `.blocked(reason:since:)`. View yang diberi `.mutationGated()` dan `StateAction` dengan `mutates` true nonaktif selama nilainya `.blocked`, dengan `.help(reason)`, sementara tombol yang tidak mengubah data (copy, navigasi, Retry, Open Diagnostics) tetap aktif.

## Decision

**Chosen option**: per keputusan di [rationale.md](rationale.md), semua memakai opsi yang direkomendasikan, ditambah keputusan kecil di bagian *Keputusan rekomendasi* dan perbaikan hasil cross check.

Fitur 12 membangun lapisan fondasi `StacklyUI` (token, palet status, `StatusLabel`, `ToneBadge`, `ViewState` dan view state, gate mutasi, `.disabled(reason:)`, modifier konfirmasi, dan galeri) di atas warna sistem dan kontrol native, dengan nilai warna kustom yang dibuktikan lolos WCAG AA oleh test, dan aturannya ditulis di `apps/macos/design.md`.

**Implementation skills**: `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`)

## Feature design

### Batas dengan fitur lain

| Bagian | Pemilik |
| --- | --- |
| Token (tipografi, warna, spacing, radius, aturan material dan motion), `Tone`, `toneSurface`, `.cardSurface()`, helper simbol, motion, dan pengumuman | 12 |
| `StatusAppearance`, `StatusLabel`, `ToneBadge`, `SectionStyle`, `SectionIcon` | 12 |
| `ViewState`, `StateView`, `StatePlaceholder`, `InlineEmptyRow`, `StateBanner`, `DelayedProgress`, `MutationAvailability`, `.mutationGated()`, `.disabled(reason:)`, `DisabledReasonText` | 12 |
| `ConfirmationRequest` dan `.stacklyConfirmation(item:)` | 12 |
| Galeri desain dan jendela Debug "Design Gallery" | 12 |
| Komponen lain: tile provider, key value, sheet dengan footer, umpan balik setelah aksi, progress bertahap, checklist preflight, view log, view diff, menu aksi, inspector | 67, memakai ulang milik 12 |
| Banner daemon tidak terjangkau, pengisian `MutationAvailability`, status koneksi, adapter DTO ke enum UI, pemasangan `SectionIcon` di sidebar, lebar minimum jendela | 13 |
| Snapshot, XCUITest, dan audit aksesibilitas per layar | 68 |
| Setting Appearance (System, Light, Dark) | 32 |

### Model data

Fitur ini tidak menyimpan data dan tidak menambah schema. Model tipenya adalah nilai di `StacklyUI`: `ColorToken` dan `Tone` (bagian Warna dan Tone), enum status UI, `StatusDetail`, dan `StatusAppearance` (bagian Tone dan status), `ViewState` beserta isinya dan `MutationAvailability` (bagian Pola state), dan `ConfirmationRequest` (bagian Konfirmasi).

Isolasi: target `StacklyUI` dan test target nya memakai `.defaultIsolation(MainActor.self)`. Nilai murni tanpa closure (`ColorToken`, `Tone`, enum status, `StatusDetail`, `StatusAppearance`, `EmptyKind`, `FailureContent`, `UnsupportedContent`, `StaleNote`, `MutationAvailability`) ditandai `nonisolated` dan `Sendable`, supaya adapter spec 0013 bisa membuatnya di luar main actor. Tipe yang membawa closure (`StateAction`, `EmptyContent`, `PartialNote`, `PartialItem`, `ContentNotes`, `ViewState`, `ConfirmationRequest`) tetap di main actor, dan closure nya `@MainActor`.

### Tipografi

Hanya text style semantik macOS. Ukuran mockup jatuh ke style terdekat; berat huruf hanya regular, medium (dipakai kontrol native), semibold, dan bold.

| Peran | Mockup (px, berat) | Text style | Ukuran macOS |
| --- | --- | --- | --- |
| Judul utama onboarding | 28, 700 | `.largeTitle.bold()` | 26 |
| Angka ringkasan Overview | 28, 600 | `.largeTitle.weight(.semibold).monospacedDigit()` | 26 |
| Judul langkah onboarding | 24, 700 | `.title.bold()` | 22 |
| Nilai metrik | 20, 600 | `.title.weight(.semibold).monospacedDigit()` | 22 |
| Judul placeholder halaman, judul panel detail | 17, 600 sampai 700 | `.title2.weight(.semibold)` | 17 |
| Judul placeholder seksi, judul kartu besar | 15, 600 sampai 700 | `.title3.weight(.semibold)` | 15 |
| Judul seksi ("Needs Attention") | 13, 700 | `.headline` | 13 |
| Teks isi | 13, 400 | `.body` | 13 |
| Judul baris yang ditekankan | 13, 600 | `.body.weight(.semibold)` | 13 |
| Teks sekunder, isi banner, sel tabel monospace | 12 | `.callout` (monospace: `.callout.monospaced()`) | 12 |
| Subjudul toolbar, header grup sidebar, tag, keterangan | 11 | `.subheadline` (tag dan header: `.weight(.semibold)`) | 11 |
| Metadata terkecil | 10 sampai 10,5 | `.footnote` | 10 |

- Helper: `.stacklyNumeric()` sama dengan `.monospacedDigit()`, `.stacklyCode()` sama dengan `.callout.monospaced()` dengan `textSelection(.enabled)`.
- `.stacklySymbol(size:relativeTo:)` adalah satu satunya tempat ukuran font tetap: ukurannya dari `@ScaledMetric(relativeTo:)` dan dipakai hanya untuk `Image(systemName:)`. Simbol yang tidak butuh ukuran khusus memakai text style di sekitarnya.
- Teks besar: langkah 4 memeriksa di macOS 27 (Xcode 27.0) apakah text style membesar dengan `.dynamicTypeSize`, dan hasilnya tidak (`.body` tetap 13 pt dari `.xSmall` sampai `.accessibility5`, lewat `Font.resolve(in:)` maupun `Text` yang dirender); `@ScaledMetric` juga tidak menskalakan. Jadi tidak ada lapisan skala buatan sendiri, dan cek teks besar adalah cek lebar sempit: halaman galeri dan preview ketiganya dirender di 640 pt, lebar konten minimum yang ditetapkan spec 0013 (jendela minimum 900 pt dikurangi sidebar terlebar), dengan label dan detail terpanjang, untuk membuktikan label tidak terpotong dan teks membungkus. `TypographyTests.textStylesIgnoreDynamicTypeSizeOnMacOS` menjaga temuan ini; kalau test itu gagal, macOS sudah menskalakan text style, jadi preview ketiga kembali ke `.dynamicTypeSize(.accessibility3)` lewat `/architect`.
- Judul dan isi boleh membungkus ke baris berikutnya. Label status memakai `fixedSize(horizontal: true, vertical: false)`; detail status dan teks sekunder dalam baris tabel dipotong di ujung (`.tail`).

### Warna

Warna netral utama dari sistem: `labelColor` untuk teks utama, `separatorColor`, `windowBackgroundColor`, `controlBackgroundColor`, dan latar kontrol native. `tertiaryLabelColor` hanya untuk teks nonaktif dan ikon dekoratif (simbol placeholder, chevron), lewat token `StacklyColor.decorative` di file token. Kode kita tidak memakai `.secondary`, `.tertiary`, `secondaryLabelColor`, atau `tertiaryLabelColor` langsung, karena `secondaryLabelColor` di light hanya sekitar 3,95:1. API native yang menerima style untuk teks sekundernya (misalnya `LabeledContent`, footer `Section`) diberi `textSecondary`. Teks yang digambar sistem sendiri tanpa jalan untuk diganti (header kolom `Table`, judul toolbar) diterima sebagai urusan macOS. Accent tidak pernah menjadi warna teks kustom; accent hanya dipakai lewat kontrol native, tint seleksi, dan link native.

Token kustom (`ColorToken`, empat nilai sRGB per token: light, dark, high contrast light, high contrast dark):

| Token | Mockup asal | Light | Dark | HC light | HC dark |
| --- | --- | --- | --- | --- | --- |
| `surfaceWindow` | `--win` | #F5F5F7 | #1E1E20 | #F5F5F7 | #1E1E20 |
| `surfaceCard` | `--card` | #FFFFFF | #2A2A2D | #FFFFFF | #2A2A2D |
| `surfaceConsole` | `--console` | #FFFFFF | #18181A | #FFFFFF | #18181A |
| `textSecondary` | `--text2` | #606065 | #AEAEB3 | #4E4E52 | #C0C0C4 |

- `surfaceWindow` adalah latar area konten untuk halaman yang dirakit dari kartu kustom (Overview, Summary, Diagnostics, dan sejenisnya), supaya kartu selalu lebih terang dari latarnya di dark mode seperti mockup. Halaman yang memakai `Table`, `List`, atau `Form` native tetap memakai latar sistem.
- `.cardSurface(radius: Radius = .medium)` menggambar `surfaceCard`, sudut `radius`, dan garis tepi hairline `separatorColor` (mockup: ring 0,5 px) yang menjadi 1 pt saat Increase Contrast.
- Implementasi: `ColorToken` adalah tabel murni dengan fungsi `value(scheme: ColorScheme, contrast: ColorSchemeContrast) -> Color` dan conform ke `ShapeStyle` lewat `resolve(in:)` yang membaca `colorScheme` dan `colorSchemeContrast` dari environment. Satu sumber ini dipakai teks, latar, dan border, jadi semuanya sepakat juga saat Appearance dipaksa lewat `preferredColorScheme` (fitur 32). Test mengecek tabel lewat `value(scheme:contrast:)` dan mengecek `resolve(in:)` untuk light dan dark lewat `EnvironmentValues`; `NSAppearance` hanya dipakai test untuk me resolve warna sistem. Tidak ada accessor `NSColor` untuk token kustom.
- Pencampuran tint dihitung di ruang sRGB dengan gamma (sama dengan CSS `color-mix(in srgb, ...)` di mockup): `hasil = fill × a + dasar × (1 − a)`.
- `apps/macos/Stackly/Assets.xcassets` hanya berisi `AppIcon` dan `AccentColor` kosong. Warna kustom tidak ditaruh di asset catalog mana pun.

### Tone dan status

Lima tone dengan arti tetap:

| Tone | Arti | Fill (titik, ikon, dasar tint) | Teks light | Teks dark | Teks HC light | Teks HC dark | Simbol tone |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `positive` | berjalan sesuai harapan (running dan ready, verified, tested) | `systemGreen` | #18712D | #4ADE74 | #135A24 | #4ADE74 | `checkmark.circle.fill` |
| `caution` | transisi atau perlu perhatian tapi tidak gagal (starting, recovering, unknown, drift, partial, stale, experimental) | `systemOrange` | #965000 | #FFB340 | #774000 | #FFB340 | `exclamationmark.triangle.fill` |
| `critical` | gagal atau memblokir (failed, error, konflik) | `systemRed` | #B8160C | #FF867F | #9D120A | #FFAAA5 | `xmark.circle.fill` |
| `neutral` | diam atau informasi biasa (stopped, retained, done, cancelled, not tested) | `systemGray` | #606065 | #AEAEB3 | #4E4E52 | #C0C0C4 | `info.circle.fill` |
| `informative` | sedang dikerjakan atas permintaan pengguna (operasi berjalan, progress install) | `systemBlue` | #0759BA | #62AEFF | #064C9E | #8EC5FF | `ellipsis.circle.fill` |

- Teks `neutral` sama dengan `textSecondary`.
- Fill memakai warna sistem, jadi varian Increase Contrast nya diurus macOS. Fill bukan teks dan selalu didampingi label, jadi tidak dihitung sebagai pembawa arti tunggal.
- `toneSurface(_ tone: Tone, on base: SurfaceToken = .card, strength: ToneStrength)` menggambar latar opak: warna `base` (`SurfaceToken` hanya `.card` atau `.window`) dicampur fill tone 16% (`.badge`) atau 12% (`.banner`). Saat Increase Contrast, ditambah border 1 pt warna fill tone. Tone surface tidak pernah di atas glass atau latar sistem.
- Teks tone hanya boleh di atas permukaan polos atau di atas tint tone yang sama, tidak pernah di atas tint tone lain.
- Status set baru (misalnya state runtime di fitur 18) memetakan statenya ke lima tone ini menurut kolom Arti, lalu menambahkan tabelnya ke design.md.

**Enum status UI** (diisi adapter spec 0013 dari DTO):

| Enum | Case |
| --- | --- |
| `LifecycleStatus` | `uninitialized`, `initializing`, `stopped`, `starting`, `running`, `recovering`, `stopping`, `failed`, `unknown`, `retained` (sepuluh state spec 0004) |
| `ReadinessStatus` | `ready`; `awaitingProbe` (readiness `not_ready` dengan reason `awaiting_probe`); `notReady(detail: String?)` (readiness `not_ready` dengan reason lain, detail dari daemon); `unknown` (readiness `unknown`, termasuk hasil `effective` yang stale) |
| `HealthStatus` | `unknown`, `healthy`, `degraded`, `unhealthy` |
| `OperationStatus` | `queued`, `running`, `cancelRequested`, `succeeded`, `failed`, `cancelled`, `needsRecovery` |

Daemon yang menerapkan `effective(now, max_age)` spec 0004 sebelum mengirim readiness dan health, jadi GUI tidak menghitung kesegaran sendiri.

`StatusAppearance` berisi `label: String`, `tone: Tone`, `symbol: String`, dan `detail: StatusDetail?`, dengan `StatusDetail { text: String, tone: Tone }`. Paling banyak satu detail.

**Status instance** (readiness dan health hanya dibaca saat `running`; health hanya dibaca saat readiness `ready`):

| Lifecycle | Readiness dan health | Label | Tone | Simbol status | Detail (tone detail) |
| --- | --- | --- | --- | --- | --- |
| `uninitialized` | | Not Initialized | `neutral` | `circle.dashed` | tidak ada |
| `initializing` | | Initializing… | `caution` | `hourglass.circle.fill` | tidak ada |
| `stopped` | | Stopped | `neutral` | `stop.circle.fill` | tidak ada |
| `starting` | | Starting… | `caution` | `hourglass.circle.fill` | tidak ada |
| `running` | `awaitingProbe` | Starting… | `caution` | `hourglass.circle.fill` | Waiting for probe (`caution`) |
| `running` | `notReady(detail)` | Not Ready | `caution` | `exclamationmark.circle.fill` | detail dari daemon, atau "Readiness check failed" kalau kosong (`caution`) |
| `running` | `unknown` | Running | `caution` | `questionmark.circle.fill` | Readiness unknown (`caution`) |
| `running` | `ready`, health `healthy` atau `unknown` | Running | `positive` | `checkmark.circle.fill` | tidak ada |
| `running` | `ready`, health `degraded` | Running | `positive` | `checkmark.circle.fill` | Degraded (`caution`) |
| `running` | `ready`, health `unhealthy` | Running | `positive` | `checkmark.circle.fill` | Unhealthy (`critical`) |
| `recovering` | | Recovering… | `caution` | `arrow.clockwise.circle.fill` | tidak ada |
| `stopping` | | Stopping… | `caution` | `hourglass.circle.fill` | tidak ada |
| `failed` | | Failed | `critical` | `xmark.circle.fill` | tidak ada (alasan kegagalan ditampilkan banner layar, fitur 33) |
| `unknown` | | Unknown | `caution` | `questionmark.circle.fill` | Process identity not confirmed (`caution`) |
| `retained` | | Retained | `neutral` | `archivebox.circle.fill` | tidak ada |

**Status operasi** (state dari spec 0004; `partial` hanya berpengaruh saat `succeeded` dan diabaikan di state lain):

| State | Label | Tone |
| --- | --- | --- |
| `queued` | Queued | `neutral` |
| `running` | Running | `informative` |
| `cancelRequested` | Cancelling… | `caution` |
| `succeeded` | Done | `neutral` |
| `succeeded` dengan `partial` true | Partial | `caution` |
| `failed` | Failed | `critical` |
| `cancelled` | Cancelled | `neutral` |
| `needsRecovery` | Needs Recovery | `critical` |

Status operasi tampil sebagai `ToneBadge`, jadi `symbol` di `StatusAppearance.operation` adalah simbol tone dan `detail` selalu kosong.

Copy label di luar lima label mockup (Running, Stopped, Starting…, Stopping…, Failed, dan Waiting for probe) adalah copy baru yang ditetapkan spec ini.

**`StatusLabel(_ appearance: StatusAppearance, context: StatusContext = .standard)`**:

- Isi: titik 7 pt (`ScaledSize.statusDot`) berwarna fill tone, lalu label, lalu detail opsional. Saat Differentiate Without Color aktif, titik diganti simbol status dengan warna fill tone, dan detail diawali simbol tone detail.
- Konteks `.standard` (kartu, banner, sheet, panel): label berwarna teks tone untuk `critical` dan `textSecondary` untuk tone lain (mockup: `stFg`), detail berwarna teks tone detail.
- Konteks `.row` (baris `List` dan `Table`, sidebar, menu, popover): label dan detail memakai `.primary`, karena latar seleksi yang tidak aktif dan latar glass tidak bisa dijamin untuk teks tone (seleksi tidak aktif di dark menurunkan teks merah ke sekitar 4,03:1). Saat `backgroundProminence == .increased`, titik dan simbol juga memakai `.primary`.
- Aksesibilitas: anak digabung (`accessibilityElement(children: .combine)`) dengan label "Status: <label>" ditambah ", <detail>" bila ada.
- Perubahan status memakai crossfade `StacklyMotion.crossfade`.

**`ToneBadge(_ appearance: StatusAppearance)`**: kapsul dengan tinggi minimum 20 pt (`ScaledSize.badgeHeight`), padding horizontal 9 pt, label `.subheadline.weight(.semibold)` berwarna teks tone, latar `toneSurface(tone, strength: .badge)`. Saat Differentiate Without Color aktif, simbol tone tampil di depan label. Label VoiceOver sama dengan teks label.

### Ikon bagian

`SectionStyle` memberi `symbol` dan `color`; `SectionIcon(_ section: SectionStyle)` menggambarnya dengan `accessibilityHidden(true)` dan dipasang di sidebar oleh spec 0013:

| Bagian | Simbol | Warna sistem |
| --- | --- | --- |
| Overview | `square.grid.2x2` | `systemBlue` |
| Instances | `server.rack` | `systemGreen` |
| Environments | `square.3.layers.3d` | `systemPurple` |
| Catalog | `book` | `systemOrange` |
| Runtimes | `shippingbox` | `systemTeal` |
| Operations | `arrow.triangle.2.circlepath` | `systemIndigo` |
| Diagnostics | `waveform.path.ecg` | `systemPink` |

### Spacing, radius, dan ukuran

- `Spacing`: `s2` (2), `s4` (4), `s6` (6), `s8` (8), `s12` (12), `s14` (14), `s16` (16), `s22` (22). Padding memakai angka tetap dari skala ini, tidak diskalakan.
- Inset bernama: halaman 22 horizontal (`s22`), jarak antar seksi 22, padding kartu 14 vertikal dan 16 horizontal, baris daftar 11 vertikal dan 14 horizontal (mockup), baris rapat 9 vertikal dan 14 horizontal.
- `Radius` untuk permukaan kustom saja: `small` 8 (blok kode, baris tabel kustom), `medium` 14 (daftar berkelompok, kartu, banner), `large` 16 (kartu ringkasan, banner kegagalan, panel). Mockup 12 dipetakan ke `medium`, mockup 18 ke `large`, dan mockup 24 sampai 26 (sheet dan dialog) digambar sistem. `ToneBadge` memakai `Capsule`.
- `ScaledSize` berisi nilai default bernama yang dipakai di property wrapper `@ScaledMetric` tiap view: `statusDot` 7 relatif `.body`, `bannerSymbol` 20 relatif `.body`, `badgeHeight` 20 relatif `.subheadline`, `placeholderPageSymbol` 44 relatif `.title2`, `placeholderSectionSymbol` 34 relatif `.title3`, `rowMinHeight` 34 relatif `.body`. Di macOS 27 `@ScaledMetric` tidak menskalakan (langkah 4), jadi nilai ini tampil di ukuran dasarnya; wrapper tetap dipakai supaya ukuran ikut membesar tanpa perubahan kode kalau macOS kelak menskalakannya.
- Baris tidak pernah punya tinggi tetap, hanya `minHeight`. Kontrol native (tombol, segmented control, toggle, field, table) tidak di restyle; bentuk dan ukurannya dari sistem.
- Area klik kustom minimal 24 × 24 pt lewat `frame(minWidth:minHeight:)` dan `contentShape`, walau gambarnya lebih kecil.

### Material dan Liquid Glass

- Glass hanya dari kontainer dan kontrol native: sidebar `NavigationSplitView`, item toolbar, sheet, popover, menu, dialog, dan inspector. Tinted atau clear, Reduce Transparency, dan Increase Contrast untuk glass ini diurus macOS mengikuti System Settings.
- Tidak ada `.glassEffect`, `GlassEffectContainer`, gaya tombol glass, atau `Material` kustom di konten. Semua permukaan kustom opak (`surfaceWindow`, `surfaceCard`, `surfaceConsole`, `toneSurface`).
- Latar jendela tembus pandang di mockup (`--win-glass`, wallpaper) tidak ditiru; jendela memakai perilaku standar macOS.
- Overlay yang di mockup memakai glass (toast, dialog konfirmasi, menu More) diganti presentasi native: dialog lewat `.stacklyConfirmation`, menu lewat `Menu`, dan umpan balik setelah aksi oleh fitur 67 dengan permukaan opak atau presentasi sistem.

### Motion

- `StacklyMotion.crossfade` adalah transisi `.opacity` dengan animasi 0,2 detik, dipakai untuk perubahan status, munculnya banner, dan pergantian bentuk `ViewState`, di semua setting.
- Tidak ada `.move`, `.slide`, `.scale`, atau `.offset` yang dianimasikan di view fitur 12.
- `StacklyMotion.progress(reduceMotion:)` mengembalikan animasi nilai progress, atau tanpa animasi saat `accessibilityReduceMotion` aktif. Spinner tetap berputar.
- `announce(_ text: String)` memposting `AccessibilityNotification.Announcement` sekali; dipakai `StateView` dan `StateBanner`.

### Pola state

```swift
public enum ViewState<Value> {
    case loading
    case empty(EmptyContent, notes: ContentNotes)
    case failed(FailureContent)
    case unsupported(UnsupportedContent)
    case loaded(Value, notes: ContentNotes)
}
```

| Tipe | Field |
| --- | --- |
| `EmptyContent` | `kind: EmptyKind` (`.noData` atau `.noResults(query: String)`), `symbol: String`, `title: String`, `message: String`, `primaryAction: StateAction?`, `secondaryAction: StateAction?` |
| `FailureContent` | `title: String`, `message: String`, `code: String?` |
| `UnsupportedContent` | `title: String`, `reason: String` |
| `ContentNotes` | `stale: StaleNote?`, `partial: PartialNote?`, `isRefreshing: Bool` |
| `StaleNote` | `since: Date`, `reason` (`.refreshFailed(FailureContent)` atau `.outdated(source: String)`) |
| `PartialNote` | `summary: String`, `items: [PartialItem]` dengan `PartialItem(name: String, reason: String, action: StateAction?)` |
| `StateAction` | `title: String`, `mutates: Bool`, `perform: @MainActor () -> Void` |

`ViewState.resolve(unsupported:value:error:lastLoadedAt:isRefreshing:emptyContent:outdated:partial:)` adalah satu satunya cara layar membuat `ViewState`. Input: `unsupported: UnsupportedContent?`, `value: Value?` (data terakhir yang berhasil, belum difilter), `error: FailureContent?` (error load terakhir), `lastLoadedAt: Date?`, `isRefreshing: Bool`, `emptyContent: (Value) -> EmptyContent?`, `outdated: (since: Date, source: String)?`, `partial: PartialNote?`.

1. `unsupported` tidak nil: `.unsupported`.
2. `value` nil dan `error` tidak nil: `.failed`.
3. `value` nil: `.loading`.
4. `emptyContent(value)` tidak nil: `.empty` dengan `ContentNotes`.
5. Selain itu: `.loaded` dengan `ContentNotes`.

`ContentNotes.stale`: kalau `value` dan `error` sama sama ada, `StaleNote(since: lastLoadedAt, reason: .refreshFailed(error))`; kalau tidak ada error tapi `outdated` ada, `StaleNote(since: outdated.since, reason: .outdated(source))`. Refresh gagal menang atas outdated. `isRefreshing` diteruskan apa adanya.

**Kontrak view model layar** (dipakai fitur layar, dicatat di design.md):

- Simpan data terakhir yang berhasil (`value`) dan `lastLoadedAt` selama refresh dan saat filter atau pencarian berubah. Filter diterapkan di atas `value` dan diputuskan lewat `emptyContent` (no data kalau `value` kosong, no results kalau hasil filter kosong); filter tidak pernah mengosongkan `value`.
- Saat refresh mulai: `isRefreshing` true. Saat berhasil: `value` dan `lastLoadedAt` baru, `error` nil. Saat gagal: `error` diisi, `value` tetap.
- Retry dari `.failed` mengosongkan `error` lalu memuat ulang, jadi bentuk kembali ke loading.

Tampilan per bentuk (`StateView(state:retry:openDiagnostics:content:)`, `retry` dan `openDiagnostics` adalah closure `@MainActor`):

| Bentuk | Tampilan | Aksi |
| --- | --- | --- |
| `loading` | `DelayedProgress`: `ProgressView` di tengah setelah 300 ms | tidak ada |
| `empty` `.noData` | banner `stale` dan `partial` bila ada, lalu `StatePlaceholder` (simbol `StacklyColor.decorative`, judul, penjelasan `textSecondary`) | sampai dua dari `EmptyContent` (mockup: Browse Catalog, New Instance…) |
| `empty` `.noResults` | banner bila ada, lalu `StatePlaceholder` dengan simbol `magnifyingglass`, judul "No Results for “<query>”", pesan dari layar (mockup: "Check the spelling or try another status.") | Clear Filters dari layar sebagai `primaryAction` |
| `failed` | `StatePlaceholder` dengan simbol tone `critical`, judul dari layar, pesan daemon, kode monospace | Retry, Open Diagnostics |
| `unsupported` | `StatePlaceholder` dengan simbol `nosign`, judul (misalnya "Backups Aren’t Supported"), alasan | tidak ada |
| `loaded` | banner `stale` dan `partial` bila ada, lalu konten | Retry di banner stale; aksi item di banner partial |

- `StatePlaceholder` punya ukuran `.page` (judul `.title2.weight(.semibold)`, simbol `placeholderPageSymbol`, padding vertikal 80) dan `.section` (judul `.title3.weight(.semibold)`, simbol `placeholderSectionSymbol`, padding vertikal 34, di dalam kartu). Aksi utama `.borderedProminent`, aksi sekunder `.bordered`, tidak ada yang default. `InlineEmptyRow(text)` adalah satu baris `textSecondary` di dalam daftar berkelompok.
- `StateBanner(tone:title:message:code:items:actions:base:)`: simbol tone selalu tampil (`bannerSymbol`), judul semibold, isi `.callout` `textSecondary`, blok kode opsional di `surfaceConsole` dengan `Radius.small`, daftar item opsional (paling banyak lima, lalu "and N more" dengan tombol Show All yang membuka sisanya di tempat), sampai tiga aksi native, latar `toneSurface(tone, on: base, strength: .banner)` dengan `base` default `.card`.
- Banner stale halaman: judul "Showing the last known state from <waktu>." (mockup), isi "Couldn’t refresh: <pesan>." untuk refresh gagal atau "<source> last updated <waktu>." untuk outdated, aksi Retry yang memanggil `retry`. Selama `isRefreshing`, Retry nonaktif dan menampilkan `ProgressView` kecil.
- Waktu banner dari fungsi murni `StaleNote.formattedSince(now:calendar:locale:timeZone:)`: jam dan menit pendek sesuai locale (14:02 di locale 24 jam, 2:02 PM di en_US), ditambah tanggal pendek kalau bukan hari ini. View memanggilnya dengan nilai sistem; test memanggilnya dengan nilai tetap.
- Konten tidak pernah diredupkan saat stale.
- `DelayedProgress` memakai model kecil `DelayedVisibility` dengan fungsi sleep yang bisa diganti (`sleep: @Sendable (Duration) async throws -> Void`, default `Task.sleep(for:)`) dan `.task(id:)` yang dimulai ulang setiap kali bentuk masuk ke loading.
- Error per item (instance gagal, konflik port, drift) tidak memakai `ViewState`; layar menaruh `StateBanner` di item itu (mockup: banner gagal di Summary, drift di Configuration).
- Placeholder dan banner adalah satu kelompok VoiceOver dengan judul sebagai label; perubahan ke `failed` dan munculnya banner critical memanggil `announce` sekali.

**Gate mutasi**: `MutationAvailability` (`.available` atau `.blocked(reason: String, since: Date)`) adalah nilai environment yang diisi shell (spec 0013) saat daemon tidak terjangkau atau sebab global lain. `.mutationGated()` menonaktifkan view selama `.blocked` dan memasang `.help(reason)`. `StatePlaceholder` dan `StateBanner` menerapkan gate yang sama ke `StateAction` dengan `mutates` true. Alasan yang terlihat untuk keadaan ini adalah banner jendela milik shell. Tombol yang tidak mengubah data tidak diberi `.mutationGated()`.

**Unsupported di luar halaman**: `.disabled(reason: String?)` menonaktifkan view saat `reason` tidak nil dan memasang `.help(reason)`, yang di macOS menjadi tooltip dan bantuan VoiceOver. Layar wajib menaruh `DisabledReasonText(reason)` (simbol `info.circle`, `.footnote`, `textSecondary`) di dekat tombol, di footer seksi, atau di placeholder unsupported. Tombol capability unsupported tidak pernah disembunyikan, dan tombol capability baru digambar setelah data layar beserta capability nya termuat, jadi tidak ada keadaan "capability belum diketahui" di view yang sudah loaded.

### Konfirmasi

```swift
public struct ConfirmationRequest {
    public enum Role { case destructive, standard }
    public var title: String        // "Remove “pg17-dev”?"
    public var message: String      // akibat dan apa yang tetap aman
    public var actionTitle: String  // "Remove"
    public var role: Role
    public var perform: @MainActor () -> Void
}

extension View {
    public func stacklyConfirmation(item: Binding<ConfirmationRequest?>) -> some View
}
```

- Dibangun di atas `NSAlert`, bukan `confirmationDialog`. Langkah 17 memeriksa `confirmationDialog` SDK 27 di macOS 27.0 (26A428) dengan tombol Return dan Escape sungguhan, dan hasilnya gagal: satu tombol hanya memegang satu key equivalent, jadi Cancel destructive dengan `keyboardShortcut(.defaultAction)` menerima Return tapi Escape tidak berbuat apa apa, sedangkan tanpa shortcut itu Escape membatalkan tapi Return tidak berbuat apa apa. Request yang diganti saat dialog terbuka juga tetap memakai judul, pesan, dan severity lama.
- Destructive: gaya `.critical`, tombol Cancel ditambahkan lebih dulu (tempat tombol default di tepi kanan) dengan key equivalent Return, tombol aksi `hasDestructiveAction` tanpa key equivalent, dan monitor `keyDown` lokal memetakan Escape ke Cancel selama alert terbuka: hanya untuk destructive, hanya event di jendela alert itu, hanya Escape tanpa ⌘, ⌥, ⌃, atau ⇧, event nya dihabiskan, dan monitor dilepas saat pengguna menjawab atau alert ditutup. Mengisi `defaultButtonCell` jendela tidak dipakai, karena menimpa key equivalent Cancel dengan Return. Standard: gaya `.informational`, tombol aksi lebih dulu dengan key equivalent Return, dan Cancel dengan Escape.
- Window diambil dari anchor `NSViewRepresentable` kecil di view yang memasang modifier: kalau anchor punya window, alert tampil sebagai sheet di window itu (`beginSheetModal(for:)`); kalau tidak, dipakai `runModal()`. Modifier menunda tampil satu giliran (`Task`) setelah binding berubah, supaya anchor sudah ada di window nya dan tidak salah jatuh ke `runModal()`. Request yang diganti menutup alert lama tanpa menjalankannya lalu menampilkan alert baru, dan binding yang diisi `nil` menutup alert tanpa menjalankan aksi. Saat pengguna menjawab, modifier mengisi binding dengan `nil` dulu, lalu memanggil `perform` hanya untuk tombol aksi. Saat view yang memasang modifier hilang, alert ditutup tanpa menjalankan aksi dan binding diisi `nil`.
- Kembali ke `confirmationDialog` hanya lewat `/architect`, setelah cek Return, Escape, dan Tab lalu Space yang sama lulus di versi macOS yang baru.
- Hanya satu konfirmasi pada satu waktu per modifier; request baru mengganti binding.
- Konfirmasi wajib untuk aksi yang menghapus atau menimpa data, menghapus record atau runtime, mematikan proses tanpa stop yang wajar, atau mengganggu pemilik lain. Role destructive: remove instance (54), purge data (54), remove runtime (55), restore overwrite (52), force stop (26), stop instance yang dipakai lease environment atau shared (26, 47), Stop All Services (32). Role standard: bring down environment (47), karena hanya menghentikan instance yang tidak dibutuhkan pihak lain dan data tetap ada. Tanpa konfirmasi: start, stop, dan restart tanpa dependen, reload, copy, cancel operasi, dan clean up cache yang bisa diunduh ulang. Fitur lain menerapkan aturan yang sama dan menambah baris di design.md.
- Copy: judul pertanyaan dengan nama objek dalam kutip lengkung ("Stop “mysql-erp”?"), pesan satu atau dua kalimat yang menyebut akibat lalu apa yang aman ("Data is kept."), label tombol kata kerja dari judul. Copy contoh dari mockup.
- Aksi dijalankan lewat jalur mutasi spec 0013. Kalau daemon tidak terjangkau saat tombol ditekan, tidak ada yang dikirim dan umpan balik fitur 67 menampilkan "stacklyd is unavailable. Nothing was sent." (mockup).

### Galeri desain

- `StacklyUI/Gallery/` berisi `DesignGallery` dengan empat halaman: Tokens, Status, States, Confirmation. Setiap halaman punya `#Preview` light, dark, dan lebar sempit 640 pt (cek teks besar, lihat Tipografi), dengan data contoh dari mockup tanpa kredensial nyata. Seluruh folder dibungkus `#if DEBUG`, jadi tidak ikut build Release.
- Build Debug target app menambah scene `Window("Design Gallery", id: "design-gallery")` dengan perintah di menu Window, di dalam `#if DEBUG`.

### Pasangan kontras

| Grup | Teks | Permukaan | Batas normal | Batas HC |
| --- | --- | --- | --- | --- |
| P1 | `labelColor`, `textSecondary` | `surfaceWindow`, `surfaceCard`, `surfaceConsole`, `windowBackgroundColor`, `controlBackgroundColor` | 4,5:1 | 7:1 |
| P2 | teks lima tone | semua permukaan P1 | 4,5:1 | 7:1 |
| P3 | teks tone X | `toneSurface(X)` `.badge` dan `.banner` di atas `.card` dan `.window` | 4,5:1 | 4,5:1 |
| P4 | `labelColor`, `textSecondary` | `toneSurface` setiap tone, `.banner`, di atas `.card` dan `.window` | 4,5:1 | 4,5:1 |

Batas normal berlaku di appearance light dan dark, batas HC di appearance high contrast light dan dark. Warna sistem di resolve per `NSAppearance` di test. Teks dalam konteks `.row` memakai `.primary` sistem dan tidak masuk tabel. Hitungan awal ada di rationale.md, bagian *Bukti kontras*.

Kalau di macOS 27 warna sistem yang di resolve membuat satu pasangan gagal, nilai token digeser seminimal mungkin sampai lolos, design.md mencatat nilai baru, dan spec ini diperbarui lewat `/architect`.

### Cek di /check verify

Bagian yang tidak bisa dibuktikan `swift test` dicek di jendela Design Gallery app Debug, dengan setting sistem yang diubah sungguhan:

- Differentiate Without Color: titik berganti simbol di `StatusLabel`, simbol tone muncul di `ToneBadge` (AC-5, AC-18).
- Increase Contrast: teks pindah ke varian HC, border 1 pt muncul di kartu, banner, dan badge (AC-7).
- Reduce Transparency: permukaan kustom tidak berubah, glass sistem menjadi opak (AC-6, AC-7).
- Reduce Motion: tidak ada gerakan, progress tanpa animasi (AC-16).
- Light, dark, dan Appearance yang dipaksa: warna teks dan border sepakat (AC-7).
- Keyboard: Tab dan Space mencapai dan menjalankan setiap tombol placeholder, banner, Show All, dan dialog; Return dan Escape di dialog destructive dan standard (AC-15, AC-17).
- VoiceOver: "Status: Running", kelompok placeholder dan banner, pengumuman failed (AC-5, AC-11, AC-17).
- Lebar sempit 640 pt, cara teks besar dari langkah 4 (AC-5, AC-18).

XCUITest untuk cek ini adalah urusan fitur 68.

### Asumsi lintas spec

- **Assumed** spec 0004 (sudah ditulis): state lifecycle instance, readiness dengan reason `awaiting_probe` dan detail maksimal 256 karakter tanpa secret, health, dan state operasi persis seperti tabel di spec itu.
- **Assumed** spec 0006 (protokol IPC, ditulis paralel): DTO membawa lifecycle, readiness (state, reason, detail) dan health yang sudah melalui `effective(now, max_age)`, dan state operasi; error daemon membawa kategori, kode, dan pesan yang bisa ditampilkan.
- **Assumed** spec 0009 (provider engine, ditulis paralel): capability provider sampai ke client bersama data layar, sehingga layar tahu capability yang unsupported dan alasannya.
- **Assumed** spec 0011 (secret dan redaction, ditulis paralel): pesan error dan detail readiness sudah bebas secret saat sampai di GUI.
- **Assumed** spec 0013 (app shell, ditulis berikutnya): shell memegang status koneksi, waktu snapshot terakhir, satu banner daemon tidak terjangkau memakai `StateBanner` dengan `base: .window`, pengisian `MutationAvailability`, adapter dari DTO ke `LifecycleStatus`, `ReadinessStatus`, `HealthStatus`, dan `OperationStatus` dengan switch exhaustive, lebar minimum jendela, keputusan apakah `StacklyUI` bergantung pada `StacklyClient`, dan pemasangan `SectionIcon` di sidebar.
- **Assumed** spec 0007 (operation engine, belum ditulis): hasil operasi membawa penanda partial untuk label Partial.
- **Assumed** fitur 32: setting Appearance diterapkan dengan `preferredColorScheme` di scene, jadi token mengikuti tanpa kode tambahan.

### API surface

| API `StacklyUI` | Bentuk | Input utama | Output | Catatan |
| --- | --- | --- | --- | --- |
| `ColorToken`, `StacklyColor` | struct `ShapeStyle`, namespace statis | empat nilai sRGB | `value(scheme:contrast:)`, `resolve(in:)` | `surfaceWindow`, `surfaceCard`, `surfaceConsole`, `textSecondary`, `decorative` |
| `Tone` | enum | `positive`, `caution`, `critical`, `neutral`, `informative` | `fill`, `text`, `symbol` | |
| `toneSurface(_:on:strength:)` | modifier | tone, `.card` atau `.window`, `.badge` atau `.banner` | latar opak plus border saat Increase Contrast | |
| `.cardSurface(radius:)` | modifier | `Radius` | kartu opak dengan hairline | |
| `.stacklySymbol(size:relativeTo:)` | modifier | ukuran, text style acuan | ukuran simbol yang diskalakan | satu satunya ukuran tetap |
| `StacklyMotion`, `announce(_:)` | namespace, fungsi | | transisi, animasi progress, pengumuman | |
| `LifecycleStatus`, `ReadinessStatus`, `HealthStatus`, `OperationStatus` | enum UI | diisi adapter spec 0013 | | lihat Enum status UI |
| `StatusAppearance`, `StatusDetail` | struct | `instance(lifecycle:readiness:health:)`, `operation(state:partial:)` | `label`, `tone`, `symbol`, `detail` | tabel status |
| `StatusLabel` | view | `StatusAppearance`, `StatusContext` | titik atau simbol, label, detail | |
| `ToneBadge` | view | `StatusAppearance` | kapsul tag | |
| `SectionStyle`, `SectionIcon` | enum, view | tujuh bagian | `symbol`, `color`; ikon tersembunyi dari VoiceOver | |
| `Spacing`, `Radius`, `ScaledSize` | namespace statis | | `CGFloat` | |
| `.stacklyNumeric()`, `.stacklyCode()` | modifier | | | |
| `ViewState` dan tipe isinya | enum, struct | `resolve(...)` | satu bentuk | urutan tetap |
| `StateView` | view | `ViewState`, `retry`, `openDiagnostics`, `content` | | |
| `StatePlaceholder`, `InlineEmptyRow`, `StateBanner`, `DelayedProgress` | view | isi state, tone, item, aksi | | |
| `MutationAvailability`, `.mutationGated()` | nilai environment, modifier | `.available` atau `.blocked(reason:since:)` | nonaktif plus `.help` | diisi spec 0013 |
| `.disabled(reason:)`, `DisabledReasonText` | modifier, view | `String?` | nonaktif plus `.help` | |
| `ConfirmationRequest`, `.stacklyConfirmation(item:)` | struct, modifier | binding opsional | dialog native | satu satunya konfirmasi |
| `DesignGallery` | view, Debug saja | | empat halaman | |

Kesalahan tidak dilempar oleh API ini; semuanya view dan nilai. Error datang dari daemon lewat `FailureContent` dan `StaleNote`.

### Value sourcing

| Aksi atau view | Nilai | Sumber |
| --- | --- | --- |
| `StatusLabel`, `ToneBadge` | lifecycle, readiness (state, reason, detail), health, state operasi | DTO daemon lewat `StacklyClient` (spec 0006), dipetakan ke enum UI oleh adapter spec 0013; arti state dari spec 0004; kesegaran dihitung daemon |
| `StatusLabel`, `ToneBadge` | label, tone, simbol, detail tetap | tabel Status instance dan Status operasi di spec ini; lima label mockup plus copy baru dari spec ini |
| `StatusLabel` detail Not Ready | teks detail | `ReadinessStatus.notReady(detail:)` dari `Readiness.detail` daemon (spec 0004, maksimal 256 karakter, tanpa secret oleh spec 0011); fallback "Readiness check failed" |
| `ToneBadge` Partial | penanda partial | hasil operasi dari spec 0007 |
| Semua warna kustom | nilai hex | tabel Warna dan Tone di spec ini |
| Warna fill dan netral sistem | nilai warna | warna sistem macOS per appearance |
| Pilihan varian warna kustom | light, dark, high contrast | environment SwiftUI `colorScheme` dan `colorSchemeContrast`; override Appearance dari fitur 32 lewat `preferredColorScheme` |
| Simbol pengganti titik, border, animasi | setting aksesibilitas | environment `accessibilityDifferentiateWithoutColor`, `colorSchemeContrast`, `accessibilityReduceTransparency`, `accessibilityReduceMotion`; ukuran teks tidak dibaca karena text style macOS 27 tidak ikut `dynamicTypeSize` (lihat Tipografi) |
| Warna di baris terpilih | prominence | parameter `context: .row` dari layar, plus environment `backgroundProminence` |
| `ViewState` | `value`, `error`, `lastLoadedAt`, `isRefreshing`, `unsupported`, `outdated`, `partial` | view model layar dari panggilan `StacklyClient` (fitur layar, mengikuti Kontrak view model), disatukan oleh `ViewState.resolve` |
| `EmptyContent` | simbol, judul, pesan, aksi | copy dan aksi layar, contoh dari mockup |
| `FailureContent` | judul | copy layar (misalnya "Couldn’t Load Instances"); untuk daemon tidak terjangkau sebelum data pertama, copy dari spec 0013 |
| `FailureContent` | pesan dan kode | envelope error daemon: kategori, kode, pesan (spec 0006), sudah diredaksi (spec 0011) |
| `StaleNote.since` | waktu state terakhir | refresh gagal: `lastLoadedAt` dari view model layar; outdated: waktu pembaruan dari daemon (misalnya catalog, spec 0009 dan fitur 17) |
| Teks waktu banner | jam pendek, plus tanggal bila bukan hari ini | `StaleNote.formattedSince(now:calendar:locale:timeZone:)` dengan nilai sistem |
| `MutationAvailability` | `.blocked(reason:since:)` | shell spec 0013 (status koneksi dan waktu snapshot terakhir) |
| `UnsupportedContent.reason`, `.disabled(reason:)` | alasan | capability provider dari daemon (spec 0004 `CapabilitySet`, dikirim spec 0009 dan 0006); operasi berjalan dari lock operasi (spec 0007); copy per layar |
| `PartialNote` | ringkasan, item, aksi item | DTO layar (readiness environment di fitur 44 dan 46, hasil operasi di spec 0007); aksi dari layar |
| `ConfirmationRequest` | judul, pesan, label, role | layar yang meminta, mengikuti aturan Konfirmasi; nama objek dari DTO |
| Hasil konfirmasi saat offline | umpan balik "Nothing was sent" | jalur mutasi spec 0013 dan komponen umpan balik fitur 67 |
| `SectionStyle` | simbol dan warna | tabel Ikon bagian di spec ini |
| Lebar sempit untuk cek teks | lebar konten minimum | spec 0013: 640 pt, final |

### Keputusan rekomendasi

- `surfaceWindow` dipakai sebagai latar halaman yang dirakit dari kartu kustom, supaya kartu dark (#2A2A2D) selalu lebih terang dari latarnya. Alternatif yang ditolak: latar sistem, yang di dark mode bisa lebih terang dari kartu.
- Skala spacing ditambah 14, karena padding horizontal baris dan kartu mockup hampir selalu 14. Alternatif: membulatkan ke 12 atau 16, yang menggeser setiap baris dari mockup.
- Status operasi ikut dipetakan di sini, dan `ToneBadge` dibangun di sini, supaya tag operasi seragam di Overview, Operations, dan menu bar. Alternatif: diserahkan ke fitur 35 dan 67, yang berisiko memberi tone berbeda untuk state yang sama.
- Simbol status memakai bentuk berbeda per state (centang, silang, stop, jam pasir, tanda tanya, arsip) supaya Differentiate Without Color benar benar membedakan. Alternatif: satu simbol per tone, yang membuat Stopped dan Retained sama.
- Daemon tidak terjangkau diwakili `MutationAvailability` di environment, diisi shell sekali, bukan keterangan stale di setiap `ViewState`. Alternatif: setiap layar membawa alasan offline di `resolve`, yang membuat dua sumber kebenaran dan dua banner.
- Test aturan sumber di `StacklyUITests` memindai `apps/macos/Packages/StacklyUI/Sources` dan `apps/macos/Stackly` (akar repo dicari dari `#filePath` file test) untuk pola terlarang di AC-2, AC-3, AC-6, dan AC-15, dengan daftar pengecualian file yang eksplisit (file token, helper simbol, folder `Confirmation/`). Alternatif: grep manual saat verify, yang tidak menahan regresi.
- Jendela Debug "Design Gallery" di app supaya `/check verify` bisa mengubah setting aksesibilitas sistem di app nyata; preview tidak bisa mengaktifkan Increase Contrast atau Reduce Transparency.

### Key invariants

- Status tidak pernah hanya warna: setiap `StatusLabel` punya teks, setiap banner punya simbol dan judul, setiap `ToneBadge` punya label.
- Tidak ada glass kustom dan tidak ada permukaan kustom yang tembus pandang.
- Teks tone hanya di atas permukaan polos di tabel Pasangan kontras atau di atas tint tone yang sama; di baris, sidebar, menu, dan popover, teks status memakai `.primary`.
- Accent tidak pernah menjadi warna teks kustom, dan warna sekunder sistem tidak dipakai langsung.
- Empty tidak pernah tampil sebelum ada load yang berhasil; failed tidak pernah tampil kalau masih ada data terakhir.
- Hanya satu jalur konfirmasi, dan aksi destruktif tidak pernah menjadi tombol default.
- Capability unsupported tidak pernah disembunyikan dan alasannya selalu terlihat tanpa hover.
- Tidak ada ukuran font tetap di luar helper simbol dan tidak ada tinggi baris tetap.

### Security model

Tidak ada peran pengguna atau data sensitif baru; semua kontrol akses ada di daemon, dan daemon tetap memvalidasi setiap aksi walau tombolnya sudah nonaktif (PRD bagian 11.2). GUI hanya menampilkan yang dikirim daemon: pesan error dan detail readiness sudah diredaksi sebelum dikirim (spec 0011). `StacklyUI` tidak menyimpan state ke disk dan tidak mencatat isi `ViewState`. Galeri dan preview hanya memakai data contoh mockup tanpa kredensial nyata (AGENTS.md).

### Configuration required

Tidak ada env var, secret, atau setting baru. Accent dan mode Liquid Glass mengikuti System Settings, dan setting Appearance dimiliki fitur 32.

### Critical test scenarios

- Happy path: setiap baris tabel Pasangan kontras lolos di empat appearance, dan setiap baris tabel Status instance dan Status operasi menghasilkan nilai yang benar, memverifikasi **AC-3**, **AC-4**.
- Happy path: `ViewState.resolve` dengan data, kosong, hasil filter kosong, unsupported, outdated, dan partial menghasilkan bentuk dan keterangan yang benar, memverifikasi **AC-8**, **AC-10**, **AC-12**, **AC-13**, **AC-14**.
- Failure case: refresh gagal saat data sudah ada menghasilkan loaded dengan stale `.refreshFailed` yang menang atas outdated, bukan failed; error tanpa data menghasilkan failed, bukan empty; retry dari failed kembali ke loading, memverifikasi **AC-8**, **AC-11**, **AC-12**.
- Failure case: dengan sleep tiruan, data yang datang sebelum jeda selesai tidak pernah memunculkan spinner, data setelah jeda memunculkannya, dan masuk loading lagi memulai jeda dari awal, memverifikasi **AC-9**.
- Failure case: `MutationAvailability.blocked` menonaktifkan aksi `mutates` dan view `.mutationGated()`, tapi Retry dan Open Diagnostics tetap aktif, memverifikasi **AC-20**.
- Failure case: `formattedSince` dengan locale en_US dan locale 24 jam, hari ini dan kemarin, menghasilkan teks yang benar, memverifikasi **AC-12**.
- Failure case: di dialog destructive, Return dan Escape menutup dialog tanpa menjalankan aksi; di dialog standard, Return menjalankan aksi (cek verify), memverifikasi **AC-15**.
- Aksesibilitas: cek di /check verify untuk Differentiate Without Color, Increase Contrast, Reduce Transparency, Reduce Motion, keyboard, dan VoiceOver, memverifikasi **AC-5**, **AC-7**, **AC-16**, **AC-17**.
- Aturan sumber: test pemindai sumber gagal untuk ukuran font tetap, warna sekunder atau accent sebagai teks, glass kustom, dan dialog di luar modifier, memverifikasi **AC-2**, **AC-3**, **AC-6**, **AC-15**.
- Auth/permission: tidak berlaku; tidak ada peran pengguna di GUI.

## Build plan

Pendekatan proyek adalah Journey, tapi fitur 12 adalah fondasi tanpa journey pengguna sendiri. Urutannya dibuat supaya jalur pertama yang dipakai fitur 13 dan 14 (token, status, state) lengkap dengan semua state dan aksesibilitasnya, dan supaya pekerjaan setelah milestone 1 bisa paralel. Milestone 1 harus selesai dulu dan memegang semua helper bersama. Milestone 2, 3, dan 4 masing masing bisa dijalankan di session terpisah secara paralel, karena masing masing hanya menyentuh foldernya sendiri di `StacklyUI` (`Status/`, `State/`, `Confirmation/`), file test sendiri, file halaman galeri sendiri yang sudah disiapkan sebagai stub, dan satu bagian design.md yang judulnya sudah dibuat milestone 1.

**Milestone 1: Token dasar, helper bersama, bukti kontras, dan kerangka design.md**

1. Tambah test target `StacklyUITests` dengan `.defaultIsolation(MainActor.self)` di `Package.swift` StacklyUI, plus helper test bersama di `Tests/StacklyUITests/Support/` (rumus kontras WCAG, resolver warna sistem per `NSAppearance`, pembaca file sumber), dan pastikan `swift test --package-path apps/macos/Packages/StacklyUI` berjalan. Memenuhi **AC-3**.
2. `ColorToken` (tabel murni, `value(scheme:contrast:)`, `ShapeStyle`), token `surfaceWindow`, `surfaceCard`, `surfaceConsole`, `textSecondary`, `decorative`, `.cardSurface(radius:)`, dan test `resolve(in:)` untuk light dan dark. Memenuhi **AC-3**, **AC-7**.
3. `Tone` dengan fill sistem, teks normal dan HC, dan simbol tone, `toneSurface(_:on:strength:)` dengan border Increase Contrast, dan test kontras untuk setiap baris tabel Pasangan kontras di empat appearance. Memenuhi **AC-3**, **AC-7**.
4. Tipografi: `.stacklyNumeric()`, `.stacklyCode()`, dan `.stacklySymbol(size:relativeTo:)`, lalu cek apakah `.dynamicTypeSize` membesarkan text style di macOS 27 dan catat hasil serta cara cek teks besar di design.md. Memenuhi **AC-2**, **AC-18**.
5. `Spacing`, `Radius`, `ScaledSize`, `StacklyMotion`, dan `announce(_:)`. Memenuhi **AC-16**, **AC-17**.
6. Test aturan sumber (pola terlarang AC-2, AC-3, AC-6, AC-15 dengan pengecualian eksplisit). Memenuhi **AC-2**, **AC-3**, **AC-6**, **AC-15**.
7. `DesignGallery` di dalam `#if DEBUG` dengan halaman Tokens yang lengkap, file halaman Status, States, dan Confirmation sebagai stub (`// provisional: M2`, `M3`, `M4`), dan scene Debug "Design Gallery" di `StacklyApp.swift`. Memenuhi **AC-18**.
8. `apps/macos/design.md` dengan semua judul bagian; Typography, Color, Spacing and radius, Materials, Motion, dan Accessibility terisi; Status, State patterns, dan Confirmation masing masing berisi satu baris penanda "Filled by milestone N." dengan baris kosong di antaranya supaya edit paralel tidak bentrok. Memenuhi **AC-1**, **AC-6**.

**Milestone 2: Palet status** (paralel setelah milestone 1)

9. Enum UI `LifecycleStatus`, `ReadinessStatus`, `HealthStatus`, `OperationStatus`, `StatusDetail`, dan `StatusAppearance.instance` serta `.operation` sesuai tabel, dengan satu test per baris dan test keberadaan setiap nama SF Symbol. Memenuhi **AC-4**.
10. `StatusLabel` dengan konteks `.standard` dan `.row`: titik atau simbol, label, detail bertone, label VoiceOver gabungan, warna seleksi, prioritas pemotongan, crossfade. Memenuhi **AC-5**, **AC-16**, **AC-17**.
11. `ToneBadge`, `SectionStyle`, `SectionIcon`, dan test nama simbol bagian. Memenuhi **AC-4**, **AC-7**, **AC-19**.
12. Halaman galeri Status dan bagian Status di design.md. Memenuhi **AC-1**, **AC-18**.

**Milestone 3: Pola state** (paralel setelah milestone 1)

13. `ViewState` beserta tipe isinya, `ViewState.resolve`, dan `StaleNote.formattedSince`, dengan test urutan prioritas, refresh gagal, outdated, dan format waktu. Memenuhi **AC-8**, **AC-12**.
14. `DelayedVisibility` dan `DelayedProgress` dengan test sleep tiruan, lalu `StatePlaceholder` (`.page` dan `.section`), `InlineEmptyRow`, `StateBanner` (item, Show All, Retry dengan progress), dan `StateView`, termasuk pengumuman VoiceOver. Memenuhi **AC-9**, **AC-10**, **AC-11**, **AC-12**, **AC-14**, **AC-16**, **AC-17**.
15. `MutationAvailability`, `.mutationGated()`, `.disabled(reason:)`, `DisabledReasonText`, dan placeholder unsupported. Memenuhi **AC-13**, **AC-20**.
16. Halaman galeri States, bagian State patterns di design.md, dan Kontrak view model. Memenuhi **AC-1**, **AC-18**.

**Milestone 4: Konfirmasi** (paralel setelah milestone 1)

17. Cek perilaku Return dan Escape untuk kedua role di macOS 27 dengan `confirmationDialog` di jendela Design Gallery, lalu bangun `ConfirmationRequest` dan `.stacklyConfirmation(item:)` di atas jalur yang lolos (`confirmationDialog`, atau `NSAlert` dengan anchor window dan `runModal()` tanpa window), termasuk judul yang disimpan selama dialog ditutup. Memenuhi **AC-15**, **AC-17**.
18. Halaman galeri Confirmation (contoh destructive dan standard dari mockup) dan bagian Confirmation di design.md, termasuk daftar aksi yang wajib dikonfirmasi. Memenuhi **AC-1**, **AC-15**, **AC-18**.

## Consequences

**Positive**:

- Kontras AA dijamin test, bukan ingatan reviewer; token yang turun di bawah batas langsung membuat test gagal.
- Layar journey, shell, dan komponen 67 tidak memutuskan ulang warna, state, status, atau konfirmasi, jadi tampilan dan perilaku seragam di seluruh aplikasi.
- Glass, accent, Reduce Transparency, dan sebagian besar Increase Contrast diurus macOS karena hanya kontrol native yang memakai glass.
- Satu sumber varian warna (environment SwiftUI) membuat teks dan border selalu sepakat, juga saat Appearance dipaksa.
- Fitur 14 bisa menampilkan status dan state tanpa menunggu fitur 67.
- Milestone 2 sampai 4 bisa dibangun paralel.

**Negative / tradeoffs**:

- Beberapa warna menyimpang dari mockup: teks status light lebih gelap, teks merah dark lebih terang, teks status di baris tabel memakai warna utama, dan konten tidak diredupkan saat stale. Mockup perlu dicatat untuk diperbarui.
- Tanpa glass kustom, toast mockup dan dialog glass berubah menjadi presentasi native; fitur 67 tidak boleh memakai glass untuk umpan balik.
- Cancel sebagai tombol default untuk aksi destruktif membuat pengguna keyboard harus Tab ke tombol aksi; ini sengaja.
- Enum status UI menduplikasi enum DTO. Adapter di spec 0013 wajib switch exhaustive supaya case baru di protokol gagal compile, dan tabel status di spec ini perlu diperbarui setiap kali state domain berubah.
- Token kustom tidak punya accessor `NSColor`; kode AppKit (konfirmasi di `NSAlert`) hanya memakai warna sistem.
- Konfirmasi memegang kode AppKit sendiri (alert, anchor window, monitor Escape) yang harus dicek ulang dengan keyboard sungguhan di setiap versi macOS baru, karena `confirmationDialog` yang lebih sederhana gagal di macOS 27.
- Text style di macOS 27 tidak ikut `dynamicTypeSize` (langkah 4), jadi cek teks besar diganti cek lebar sempit 640 pt, yang lebih lemah dari cek teks besar di iOS.
- Teks sekunder yang digambar sistem sendiri (header `Table`) tetap memakai warna sistem yang di light di bawah 4,5:1.
- Test pemindai sumber berbasis teks bisa salah tangkap komentar; pengecualian ditulis eksplisit di test.
- `surfaceWindow` membuat halaman kartu sedikit berbeda dari latar sistem di versi macOS yang mengubah warna jendela.

**Neutral**:

- design.md berbahasa Inggris sementara spec ini berbahasa Indonesia; design.md yang dibaca builder, spec ini yang menyimpan alasan.
- `AccentColor.colorset` tetap kosong dan Assets.xcassets tidak bertambah warna.
- Target app mendapat scene Debug tambahan, dan galeri tidak ikut build Release.
- `StacklyUI` mendapat test target pertamanya.

## Follow-up

- [ ] `/sync`: tambah `design.md` ke tabel Key files dan spec 0012 ke Related specs di `apps/macos/AGENTS.md`, dan aturan "warna, state, dan konfirmasi dari design.md" di Conventions.
- [ ] Spec 0013: adapter DTO ke enum status UI dengan switch exhaustive, satu banner daemon tidak terjangkau dengan `StateBanner`, pengisian `MutationAvailability`, umpan balik offline, lebar minimum jendela, pemasangan `SectionIcon`, dan keputusan dependency `StacklyUI` ke `StacklyClient`.
- [ ] `/architect` fitur 67: pakai ulang `StatusLabel`, `ToneBadge`, `StateBanner`, `StatePlaceholder`, `.mutationGated()`, `.disabled(reason:)`, dan `.stacklyConfirmation`; umpan balik setelah aksi tanpa glass kustom; tile provider dekoratif di samping nama provider.
- [ ] `/architect` fitur 68: XCUITest untuk daftar Cek di /check verify, snapshot Increase Contrast dan Reduce Transparency yang mengatur appearance di host, dan pemakaian ulang tabel Pasangan kontras.
- [ ] `/scope`: kalimat Done when fitur 67 di `docs/scope/ui-macos.md` masih menyebut label status (titik dan teks), banner error, warning, dan info, empty state dengan aksi, dan dialog konfirmasi (aksi destruktif merah dan bukan tombol default). Semuanya sudah dibangun spec 0012, jadi selaraskan kalimat itu supaya fitur 67 memakai ulang `StatusLabel`, `StateBanner`, `StatePlaceholder`, dan `.stacklyConfirmation`, bukan membangun ulang.
- [ ] Hapus tanda `// provisional: F13` di `GalleryLayout.narrowWidth` dan kalimat "provisional until spec 0013 records the real minimum" di design.md (Accessibility), karena 640 pt sudah final, dan ganti kata "minimum window width" menjadi "minimum content width" di design.md, `GallerySupport.swift`, dan bantuan toggle di `DesignGallery.swift` (jendela minimum 900 pt, konten minimum 640 pt).
- [ ] Fitur 32 (pemanggil konfirmasi tanpa jendela pertama, popover menu bar): cek ulang jalur `runModal()` sebelum memakainya. Tiga risiko yang belum teruji: popover punya window sendiri sehingga mungkin jatuh ke jalur sheet; app yang tidak aktif tidak menerima Return dan Escape, jadi mungkin perlu mengaktifkan app sebelum alert tampil; request yang diganti saat `runModal()` berjalan bisa membuat modal bersarang yang menahan alert lama. Ubah aturan di spec ini lewat `/architect` kalau salah satunya terjadi, lalu ulangi cek keyboard dan VoiceOver.
- [ ] `/scope`: catat di `docs/scope/ui-macos.md` bagian Selisih yang sudah diketahui bahwa warna teks status, teks status di baris tabel, konten stale yang tidak diredupkan, dan dialog konfirmasi native menggantikan nilai mockup.
- [ ] Spec 0007: penanda partial di hasil operasi untuk label Partial.

## Rationale

Alasan dan opsi setiap keputusan: lihat [rationale.md](rationale.md).
