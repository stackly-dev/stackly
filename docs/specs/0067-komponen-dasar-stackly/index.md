# 0067. Komponen dasar Stackly

**Date**: 2026-10-21
**Status**: In Progress

## Summary

Spec ini menetapkan kumpulan view SwiftUI bersama di `StacklyUI/Components/` yang dipakai semua layar journey: tile provider, kartu seksi dengan baris key value, tombol Copy, blok konsol, aksi dan menu More, umpan balik setelah aksi, sheet dengan footer, checklist, progress bertahap, view log, view diff, dan inspector. Semuanya dibangun di atas token, status, state, gate mutasi, dan konfirmasi fitur 12, tanpa membangun ulang bagian itu dan tanpa glass kustom. Fitur 67 adalah satu satunya pembangun komponen ini; spec fase 02 (0017, 0018, 0020, 0022) tidak lagi membuat versi sementara, dan milestone fase 02 yang terpaksa mulai lebih dulu membangun komponen itu persis menurut spec ini. Toast mockup diganti kartu umpan balik opak di bawah area konten yang diumumkan ke VoiceOver, hilang sendiri hanya untuk kabar baik tanpa aksi, dan tetap tampil untuk peringatan dan error.

## Requirements

**User stories**:

- Sebagai developer layar journey (fitur 16 sampai 56), saya ingin tile, key value, sheet, umpan balik, checklist, progress, log, diff, menu aksi, dan inspector sudah tersedia di `StacklyUI`, supaya saya merakit layar dari bagian yang sama tanpa memutuskan ulang tampilan, keyboard, atau VoiceOver.
- Sebagai pengguna yang baru menekan sebuah tombol, saya ingin tahu hasilnya (berhasil, ditolak, tidak terkirim, atau tidak pasti) tanpa pesan error yang hilang sebelum saya sempat membacanya.
- Sebagai pengguna dengan provider yang tidak mendukung sebuah capability, saya ingin tombolnya tetap terlihat tapi nonaktif dengan alasan yang tertulis, supaya saya tidak mengira fitur itu bekerja.
- Sebagai pengguna keyboard atau VoiceOver, saya ingin setiap komponen bisa difokus, dijalankan, dan dibacakan, termasuk menu, sheet, dan inspector, supaya alur membuat, menghentikan, dan memeriksa instance selesai tanpa mouse (AC-027).

**Acceptance criteria** (ID lokal spec ini ditulis `AC-n`; ID PRD selalu tiga digit dan disebut "PRD AC-0nn"):

- **AC-1** (pemakaian ulang dan lokasi): Semua komponen spec ini ada di `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Components/` dengan nama dan signature persis seperti bagian *Signature Swift*. Tidak ada komponen yang menggambar status, banner, placeholder, gate, atau dialog sendiri: status lewat `StatusLabel` atau `ToneBadge`, error per item lewat `StateBanner`, kosong lewat `StatePlaceholder` atau `InlineEmptyRow`, gate lewat `.mutationGated()` atau `.disabled(reason:)` dan `DisabledReasonText`, konfirmasi lewat `.stacklyConfirmation(item:)`, permukaan lewat `.cardSurface()`, `toneSurface`, dan `StacklyColor`, semuanya dari spec 0012. `apps/macos/design.md` mendapat bagian "Components" yang menulis setiap aturan tampilan dan perilaku spec ini dalam bahasa Inggris, termasuk tabel *Status checklist* dan *State tahap*.
- **AC-2** (tile provider): `ProviderTile` menggambar persegi bersudut membulat (radius `size × 0.27`, gaya continuous) berwarna sistem menurut tabel *Warna tile*, dengan mark putih tebal di tengah. `tile` nil atau `.unrecognized` menjadi abu abu. `mark` nil memakai `ProviderMark.defaultMark(for: name)` dengan aturan default spec 0017 (dua karakter ASCII huruf atau angka pertama dari `name`, yang pertama huruf besar dan yang kedua huruf kecil, atau "?" kalau tidak ada). Tile selalu `accessibilityHidden(true)` dan selalu didampingi nama provider dalam teks di dekatnya. Ukuran font mark datang dari helper `.stacklyTileMark(tileSize:)` di `Tokens/Typography.swift`.
- **AC-3** (kartu seksi dan key value): `CardSection` menggambar judul `.headline` opsional dengan aksesori di kanannya, lalu satu kartu `.cardSurface()` berisi setiap subview isi sebagai satu baris yang dipisah garis pemisah, dengan inset baris 11 vertikal dan 14 horizontal dan tinggi minimum `ScaledSize.rowMinHeight`, lalu footer opsional. `.labeledContentStyle(.stackly)` menggambar label `.body` warna teks utama dan nilai `.body` `textSecondary` yang bisa dipilih: satu baris dengan nilai rata kanan kalau muat, selain itu label di atas dan nilai yang membungkus rata kiri di bawahnya, tanpa memotong teks. Gaya ini hanya dipasang pada baris read only, tidak pernah pada container `Form`. VoiceOver membaca setiap `LabeledContent` sebagai satu elemen "{label}, {value}", dan tombol baris (misalnya `CopyButton`) berada di luar `LabeledContent` sebagai elemen sendiri.
- **AC-4** (tombol Copy): `CopyButton` menulis teksnya ke pasteboard umum sebagai string biasa hanya saat ditekan, lalu selama 2 detik menampilkan "Copied" dengan simbol `checkmark` (atau hanya `checkmark` untuk gaya ikon) tanpa mengubah lebar tombol, lalu kembali ke "Copy" atau `doc.on.doc`. Setiap tekanan menulis lagi dan memulai 2 detik dari awal. Label VoiceOver "Copy {what}", dan setelah menulis, `announce` mengucapkan teks pengumuman dari layar (default "{What} copied."). Tombol tidak pernah diberi gate mutasi. `NSPasteboard` hanya dipakai di `Components/Copy/PlainPasteboard.swift` dan `Secrets/ConcealedPasteboard.swift` (test aturan sumber), dan `ConnectionDiagnosticsSection` pindah ke `PlainPasteboard`.
- **AC-5** (blok konsol): `ConsoleBlock` menggambar teks `.callout.monospaced()` warna teks utama di atas `.consoleSurface()` (`surfaceConsole`, sudut `Radius.medium`, garis tepi hairline `separatorColor` yang menjadi 1 pt saat Increase Contrast, padding 12 vertikal dan 14 horizontal). Teks bisa dipilih dan membungkus tanpa scroll horizontal. Varian perintah menampilkan "$ " `textSecondary` di depan perintah dan `CopyButton` yang menyalin perintah tanpa "$ ". VoiceOver membaca teks sebagai satu elemen dengan label dari layar dan nilai teksnya, tanpa membacanya dua kali; tombol Copy elemen sendiri.
- **AC-6** (deskriptor aksi): `StacklyAction.availability(gate:)` memberi `.disabled(reason: disabledReason, showsReason: true)` kalau `disabledReason` ada, lalu `.disabled(reason: alasan gate, showsReason: false)` kalau `mutates` dan gate `.blocked`, lalu `.busy` kalau `isBusy`, selain itu `.enabled`. `invoke(gate:present:)` tidak berbuat apa pun kalau availability bukan `.enabled`; kalau role membawa konfirmasi, ia memanggil `present` dengan `ConfirmationRequest` yang judul, pesan, dan label tombolnya dari `ActionConfirmation`, role nya `.destructive` untuk `ActionRole.destructive` dan `.standard` untuk `ActionRole.standard`, dan `perform` nya `perform` action; kalau tidak, ia memanggil `perform`. `ActionRole.destructive` selalu membawa `ActionConfirmation`, jadi aksi destruktif tanpa konfirmasi tidak bisa ditulis. `.actionHost()` memegang satu konfirmasi per jendela atau sheet, membaca `mutationAvailability` dari environment tempat ia dipasang, dan mengisi environment `actionHost`; tombol, item menu, dan perintah fitur 69 menjalankan action lewat `ActionHost.run(_:)`, jadi aksi yang sama nonaktif dan dikonfirmasi dengan cara yang sama di semua tempat.
- **AC-7** (tombol aksi dan capability unsupported, REQ-026): `ActionButton` memakai `.borderedProminent` untuk prominence `.primary` dan `.bordered` untuk `.secondary`; action destruktif memakai `role: .destructive` dan tidak pernah `.borderedProminent` atau tombol default. Saat `.disabled(showsReason: true)`, tombol nonaktif lewat `.disabled(reason:)` dan `DisabledReasonText` dengan alasan yang sama tampil tepat di bawahnya; dengan `showsReason: false` (wajib untuk tombol di toolbar), layar menaruh `DisabledReasonText` yang sama di kontennya (header, footer seksi, atau banner item). Saat gate `.blocked`, tombol nonaktif dengan alasan gate sebagai help saja, karena banner jendela yang menulisnya. Saat `.busy`, tombol nonaktif, label tetap, dan `ProgressView` kecil tampil di sampingnya dengan nilai VoiceOver "In progress". Tombol capability unsupported tidak pernah disembunyikan.
- **AC-8** (menu aksi): `ActionMenu` (menu More di toolbar, label "More", simbol `ellipsis.circle`) dan `ActionMenuItems` (isi `.contextMenu`) merender bagian action yang dipisah `Divider`, dengan role destruktif untuk `ActionRole.destructive`. Item yang `.disabled(showsReason: true)` atau `.busy` nonaktif dan menampilkan alasannya (atau "In progress") sebagai subtitle item yang terlihat dan dibacakan VoiceOver; item yang nonaktif karena gate tanpa subtitle. Item tidak memasang dan tidak menampilkan shortcut keyboard (milik fitur 69). Langkah 1 memeriksa subtitle ini di macOS 27 untuk menu toolbar, menu konteks, dan menu utama dengan VoiceOver; kalau subtitle tidak tampil atau tidak dibacakan, alasan tampil sebagai item nonaktif tambahan berisi teks alasan tepat di bawah item itu, dan design.md mencatat jalur yang dipakai.
- **AC-9** (tampilan umpan balik, WCAG 2.2.1, 2.4.11, dan 4.1.3): `.feedbackPresenter()` menampilkan `FeedbackCenter.current` lewat `.safeAreaInset(edge: .bottom)` di kolom detail jendela (konten bergulir di atasnya dan kontrol yang difokus tidak pernah tertutup), sebagai kartu opak di tengah, 16 pt dari bawah, lebar maksimum 520 pt dan padding samping 22 pt: simbol tone, pesan `.body` warna teks utama, detail `.callout` `textSecondary`, kode `.callout.monospaced()` `textSecondary` yang bisa dipilih, paling banyak satu tombol aksi, dan tombol Dismiss (simbol `xmark`, label "Dismiss", area klik minimal 24 × 24 pt), di atas `toneSurface(tone, on: .card, strength: .banner)` bersudut `Radius.large` dengan bayangan lembut. Muncul dan hilangnya kartu hanya crossfade `StacklyMotion.crossfade`. Setiap umpan balik diumumkan sekali lewat `announce` (pesan lalu detail) saat dikirim. Umpan balik bertone `positive`, `informative`, atau `neutral` tanpa aksi bersifat sementara: ia hilang setelah `visibleDuration` (4 detik, ditambah untuk teks panjang, paling lama 10 detik), dan hitungannya hanya berjalan selama kartu tampil, jendela aktif, pointer tidak di atas kartu, dan fokus keyboard tidak di dalamnya, lalu dimulai dari awal setiap kali salah satu syarat itu kembali terpenuhi. Umpan balik bertone `caution` atau `critical`, atau yang punya aksi, tetap tampil sampai Dismiss atau diganti. Umpan balik baru menggantikan yang tampil, kecuali umpan balik sementara saat yang tampil bersifat tetap: yang sementara itu hanya diumumkan.
- **AC-10** (umpan balik dari hasil mutasi): `Feedback.outcome(_:success:showOperations:)` memetakan `MutationOutcome` persis seperti tabel *Pemetaan hasil mutasi*: `completed` memberi umpan balik sukses dari layar atau tidak ada; `rejected` memberi `critical` dengan pesan dan hint daemon yang dijadikan kalimat oleh `DaemonText.sentence` dan kode error; `notSent` memberi `caution` dengan `MutationFeedback.notSent`; `outcomeUnknown` memberi `caution` dengan `MutationFeedback.outcomeUnknown` dan aksi "Show Operations" (`mutates` false) kalau layar memberi jalan ke Operations. `Feedback.failure(_:)` memberi umpan balik `critical` yang sama dari `ErrorBody` operasi yang gagal. `RevealModel` dan `SecretEntryModel` memakai `DaemonText.sentence` yang sama.
- **AC-11** (pemasangan di jendela): Satu `FeedbackCenter` per jendela utama dibuat oleh view akar jendela (`LiveShell`, lalu `MainWindowRoot` setelah langkah 2 milestone 1 spec 0016) dan dipasang di environment `feedbackCenter`; `AppShell` memasang `.feedbackPresenter()` di kolom detail dan `.actionHost()` di akarnya, di dalam `.environment(\.mutationAvailability, …)`. Umpan balik yang dikirim sebelum presenter terlihat (misalnya "Setup complete." dari onboarding) tampil saat presenter muncul. View di dalam sheet tidak mengirim ke `FeedbackCenter`; sheet menampilkan hasilnya sendiri dengan `StateBanner`, dan layar pemilik sheet mengirim umpan balik setelah sheet tertutup.
- **AC-12** (sheet dengan footer): `StacklySheet` menggambar latar opak `surfaceWindow`, header (tile provider 40 pt opsional, judul `.title3.weight(.semibold)`, subjudul `.callout` `textSecondary`), isi yang bergulir dengan tinggi paling banyak tinggi layar yang terlihat dikurangi 160 pt, dan footer tetap berisi alasan di kiri, Cancel, dan aksi utama. Alasan footer adalah `DisabledReasonText` aksi utama saat `.disabled(showsReason: true)`, atau alasan gate saat aksi utama `mutates` dan gate `.blocked` (sheet menutupi banner jendela). Cancel menerima Escape (`.cancelAction`), dan aksi utama menerima Return (`.defaultAction`) kecuali role nya destruktif, ketika Return tidak menjalankan apa pun. Aksi utama yang nonaktif atau sibuk tidak berjalan lewat Return. Sheet memasang `.actionHost()` sendiri sehingga konfirmasi dari dalam sheet tampil di atas sheet itu. Langkah 9 memeriksa Return dan Escape dengan tombol sungguhan di macOS 27 (fokus di `TextField`, di `Toggle`, aksi utama destruktif, aksi utama nonaktif), konfirmasi dari dalam sheet, dan latar opak, lalu mencatat hasilnya di design.md; kalau footer gagal, footer pindah ke placement toolbar `.cancellationAction` dan `.confirmationAction`, alasan pindah ke baris terakhir isi, dan cek yang sama diulang.
- **AC-13** (checklist): `CheckList` menampilkan setiap `CheckItem` dengan simbol, tone, dan kata VoiceOver persis seperti tabel *Status checklist*, judul `.callout` warna teks utama, detail `.subheadline` `textSecondary` yang membungkus penuh, dan aksi opsional. Tata letak `.adaptive` memakai grid dengan kolom minimum 200 pt dan jarak 16 pt (dua kolom di sheet 560 pt, satu kolom di lebar sempit), dan `.single` satu kolom. Status `pending` menampilkan spinner kecil hanya setelah 300 ms (`DelayedVisibility` spec 0012). VoiceOver membaca simbol, judul, dan detail setiap baris sebagai satu elemen "{title}, {status}: {detail}", dan aksi baris elemen sendiri.
- **AC-14** (pemetaan progress bertahap): `StagedProgressValue.from(operation:stages:)` adalah fungsi murni yang memberi state tiap tahap, `fraction`, `percent`, `currentTitle`, dan `outcome` persis seperti tabel *Pemetaan operasi ke tahap*. `percent` dihitung dengan bilangan bulat, `(i × total + min(done, total)) × 100 / (n × total)` untuk `counted` dan `i × 100 / n` untuk `indeterminate` atau `total` 0, dibatasi paling banyak 99 sampai `succeeded`; `fraction` untuk bar memakai pembilang dan penyebut yang sama.
- **AC-15** (tampilan progress bertahap): `StagedProgress` punya tata letak `.compact` (teks `.callout` `textSecondary` dengan angka monospace, lalu `ProgressView` linear selama `outcome` `running` atau `cancelling`, indeterminate saat `fraction` nil) dan `.stages` (bar yang sama lalu semua tahap yang membungkus ke baris berikutnya, masing masing dengan simbol dan teks menurut tabel *State tahap*). Teks dan nilai VoiceOver per `outcome` mengikuti tabel *Teks progress*, dengan nomor tahap mulai dari 1. Dengan `announcesStages` true, setiap kali tahap berjalan berganti (tidak saat view pertama muncul), `announce` mengucapkan "{judul tahap}." sekali. Dengan Reduce Motion, perubahan nilai bar tidak dianimasikan.
- **AC-16** (view log): `LogView` menampilkan `LogLine` secara lazy dengan kolom waktu dan sumber monospace `textSecondary` yang lebar minimumnya sama di semua baris (sumber yang lebih panjang tampil utuh dan menggeser teks baris itu), simbol tone untuk baris error dan warning (selalu tampil, tidak hanya saat Differentiate Without Color), dan teks `.callout.monospaced()` berwarna teks utama, teks tone `caution` untuk warning, atau teks tone `critical` untuk error, di atas `.consoleSurface()`. Teks membungkus dan bisa dipilih per baris. Saat `follow` true, baris baru membuat view menggulir ke baris terakhir; gulir oleh pengguna yang berakhir lebih dari 24 pt di atas dasar mengubah `follow` menjadi false; mengubah `follow` menjadi true menggulir ke bawah. VoiceOver membaca setiap baris sebagai "{waktu}, {sumber}, {Error | Warning}: {teks}", dengan bagian yang kosong dan tingkat normal dilewati.
- **AC-17** (view diff): `DiffView` menampilkan setiap `DiffLine` secara lazy dengan kolom tanda yang selalu terlihat ("+" untuk ditambah dan "−" untuk dihapus dalam teks tone `positive` dan `critical`, kosong untuk tidak berubah), teks `.callout.monospaced()` warna teks utama untuk baris yang berubah dan `textSecondary` untuk baris yang tidak berubah, dan latar `toneSurface(.positive atau .critical, on: .card, strength: .banner)` per baris yang berubah, di dalam `.cardSurface()`. Tanpa baris, view menampilkan `emptyText` dari layar. VoiceOver membaca "Added: {teks}", "Removed: {teks}", atau "Unchanged: {teks}".
- **AC-18** (inspector): `.stacklyInspector(isPresented:help:content:)` memasang `.inspector` native dengan lebar kolom minimum 240 dan ideal 280 pt, dan maksimum 360 pt di jendela 1000 pt atau lebih atau 280 pt di jendela yang lebih sempit (sehingga kolom halaman di jendela 900 pt tidak kurang dari 384 pt), isi di `ScrollView` di atas latar opak `surfaceWindow`, dan `InspectorToggle` di toolbar dengan label "Show Inspector" atau "Hide Inspector", simbol `sidebar.trailing`, dan help dari layar atau label yang sama. `InspectorHeader` menyusun tile 40 pt, judul `.title2.weight(.semibold)`, subjudul, `StatusLabel` konteks `.standard`, dan slot aksi opsional di tengah kolom. `InspectorPlaceholder` menampilkan judul (default "No Selection") dan pesan dari layar di tengah kolom sebagai satu kelompok VoiceOver. Di jendela 900 pt dengan sidebar dan inspector terbuka, tidak ada teks yang terpotong. `.inspector(` hanya dipakai di `Components/Inspector/` (test aturan sumber). Langkah 14 lebih dulu memeriksa di macOS 27 sambungan latar opak di kolom inspector dan perilaku kolom di jendela 900 pt, lalu mencatatnya di design.md.
- **AC-19** (aksesibilitas, NFR-012, PRD AC-027): Setiap tombol dan item di komponen spec ini bisa difokus dengan Tab dan dijalankan dengan Space saat Keyboard navigation menyala di System Settings, punya label VoiceOver, dan area klik kustom (Dismiss, ikon Copy, toggle inspector) minimal 24 × 24 pt. Tidak ada informasi yang hanya lewat warna atau hover: tone selalu berpasangan dengan simbol atau kata, dan simbol yang membawa arti memakai warna fill tone atau `textSecondary`, tidak pernah `StacklyColor.decorative`. Semua teks kustom berada di pasangan kontras spec 0012 (tidak ada pasangan baru, karena sheet dan inspector memakai latar opak `surfaceWindow`). Saat Increase Contrast, kartu, banner, dan baris tint mendapat border 1 pt spec 0012; saat Reduce Transparency, tidak ada yang berubah karena semua permukaan kustom opak; saat Reduce Motion, tidak ada gerak, hanya crossfade. Di lebar konten 640 pt, label dan judul tidak terpotong dan teks membungkus.
- **AC-20** (galeri dan kontrak cek visual): Setiap komponen punya `#Preview` light, dark, dan lebar 640 pt yang memakai `ComponentSamples` (`#if DEBUG`, satu sumber nilai contoh dari mockup tanpa kredensial nyata). Galeri desain mendapat halaman "Components" yang menampilkan setiap state di tabel *State di galeri*, yang menjadi daftar state yang dirender snapshot fitur 68; `/check verify` mencocokkan halaman itu dengan tabel. `StacklyUI/Accessibility/AccessibilityID.swift` memuat konstanta identifier di tabel *Accessibility identifier* dengan bentuk `<scope>.<part>[.<key>]` yang disepakati fitur 68.
- **AC-21** (aturan sumber): Test aturan sumber spec 0012 tetap lulus untuk `Components/` (tanpa pengecualian baru), dan dua aturan baru gagal untuk contoh terlarangnya: `NSPasteboard` di luar `Components/Copy/PlainPasteboard.swift` dan `Secrets/ConcealedPasteboard.swift`, dan `.inspector(` di luar `Components/Inspector/`.

## Decision

**Chosen option**: per keputusan di [rationale.md](rationale.md), semua memakai opsi yang direkomendasikan (keputusan lead atas delegasi user, 2026-10-21), ditambah keputusan kecil di bagian *Keputusan kecil* rationale, semua perbaikan cross check yang diterima lead, dan kesepakatan batas dengan fitur 68 dan 69 di wave yang sama.

Fitur 67 membangun satu set komponen bersama di `StacklyUI/Components/` di atas fondasi spec 0012, menjadi satu satunya pembangun versi pertama setiap komponen, mengganti toast mockup dengan kartu umpan balik opak per jendela, dan memberi satu deskriptor aksi (`StacklyAction`) yang dipakai tombol, menu, dan perintah fitur 69.

**Implementation skills**: `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

## Feature design

### Trace PRD

| ID PRD | Dipenuhi oleh |
| --- | --- |
| REQ-024 (pola state) | komponen menerima state dari pola spec 0012 dan tidak membuat bentuk state sendiri (AC-1); umpan balik hasil mutasi termasuk `notSent` saat daemon tidak terjangkau (AC-10) |
| REQ-026 (capability unsupported; config diff, logs, progress) | tombol dan item menu capability unsupported tampil nonaktif dengan alasan terlihat (AC-6, AC-7, AC-8); view diff dan log untuk fitur 27 dan 29 (AC-16, AC-17); progress bertahap untuk install, create, backup, dan restore (AC-14, AC-15) |
| REQ-027 (keyboard, VoiceOver, copy endpoint, konfirmasi destruktif) | AC-4 (copy), AC-6 (konfirmasi lewat satu host), AC-19 |
| NFR-012, PRD AC-027 | AC-19, plus kata dan simbol per status di AC-8, AC-13, AC-15, AC-16, AC-17 |

Notifikasi sistem macOS (REQ-027 "Notifikasi opsional") bukan bagian spec ini.

### Batas dengan fitur lain

| Bagian | Pemilik |
| --- | --- |
| Token, `Tone`, `toneSurface`, `.cardSurface()`, tipografi, `StatusLabel`, `ToneBadge`, `ViewState`, `StateView`, `StatePlaceholder`, `InlineEmptyRow`, `StateBanner`, `DelayedProgress`, `DelayedVisibility`, `MutationAvailability`, `.mutationGated()`, `.disabled(reason:)`, `DisabledReasonText`, `ConfirmationRequest`, `.stacklyConfirmation(item:)`, `announce`, galeri desain | spec 0012 (dipakai apa adanya) |
| `MutationPath`, `MutationOutcome`, `MutationFeedback`, `AppShell`, `LiveShell`, banner daemon, pengisian `MutationAvailability` | spec 0013 (dipakai; build F67 menambah presenter umpan balik dan `.actionHost()` di `AppShell` dan `FeedbackCenter` di `LiveShell`, lihat *Usulan update untuk spec lain*) |
| `SecretField`, `SecretEntrySheet`, `RevealableSecretText`, `ConcealedPasteboard` | spec 0011 (dipakai; copy secret tidak pernah lewat `CopyButton`) |
| Semua komponen di *Signature Swift*, `AccessibilityID` untuk scope komponen, `ComponentSamples`, halaman galeri "Components", bagian "Components" di design.md | spec ini |
| View per layar: kartu provider, matrix versi, kartu ringkasan Overview, tabel Instances dan Runtimes, `SchemaForm`, baris endpoint, isi inspector per halaman, pembungkus `PreflightSection` dan `CreateProgressSection`, `InstallingRow` | fitur pemilik layar (17, 18, 20, 21, 22, dan seterusnya), dirakit dari komponen spec ini |
| `CommandID`, `CommandMap`, menu aplikasi, shortcut, `.publishesCommands(_:)`, `PublishedCommands`, perintah View > Show/Hide Inspector | spec 0069 (membaca `StacklyAction`, `availability(gate:)`, dan `ActionHost` spec ini) |
| Snapshot, XCUITest, audit aksesibilitas otomatis, fixture kontrak untuk layar, konstanta `AccessibilityID` scope layar | spec 0068 (merender halaman galeri "Components" dan memakai `ComponentSamples`) |
| Data, filter, Load Earlier, cursor, dan batas jumlah baris log | fitur 27 |
| Sumber diff config dan drift | fitur 29 |
| Popover menu bar beserta umpan balik dan konfirmasinya sendiri | fitur 32 |
| Notifikasi sistem macOS | di luar spec ini |

**Siapa membangun versi pertama.** Fitur 67 adalah satu satunya pembangun setiap komponen di *Signature Swift*. Kalau sebuah milestone fase 02 harus mulai sebelum milestone fitur 67 yang memegang komponen itu di merge, milestone fase 02 itu membangun komponen tersebut di path, nama, dan signature spec ini, beserta AC, test, preview, entri galeri, dan bagian design.md nya; milestone fitur 67 lalu hanya memeriksa dan mencentang langkah itu. Tidak ada versi sementara di folder layar dan tidak ada penanda `// provisional: F67`. Per 2026-10-21 belum ada kode `ProviderTile`, `PreflightSection`, `CreateProgressSection`, atau `ActionFeedbackBanner` di `main` atau branch mana pun, jadi tidak ada yang perlu diadopsi.

### Peta folder

| Tempat | Isi |
| --- | --- |
| `StacklyUI/Components/Tile/` | `TileSpec`, `ProviderTile`, `ProviderMark` |
| `StacklyUI/Components/KeyValue/` | `CardSection`, `StacklyLabeledContentStyle` (`.stackly`) |
| `StacklyUI/Components/Copy/` | `PlainPasteboard`, `CopyConfirmation`, `CopyButton`, `ConsoleCopy`, `ConsoleBlock`, `.consoleSurface()` |
| `StacklyUI/Components/Actions/` | `ActionConfirmation`, `ActionRole`, `ActionAvailability`, `StacklyAction`, `ActionHost`, `.actionHost()`, `ActionProminence`, `ActionButton`, `ActionMenu`, `ActionMenuItems` |
| `StacklyUI/Components/Feedback/` | `Feedback`, `FeedbackCenter`, `FeedbackHold`, `.feedbackPresenter()`, `FeedbackCard`, `DaemonText` |
| `StacklyUI/Components/Sheet/` | `StacklySheet` |
| `StacklyUI/Components/Checklist/` | `CheckStatus`, `CheckItem`, `CheckListLayout`, `CheckList` |
| `StacklyUI/Components/Progress/` | `StageDefinition`, `StageState`, `ProgressStage`, `ProgressOutcome`, `StagedProgressValue`, `StagedProgressLayout`, `StagedProgress`, `StageFlowLayout` |
| `StacklyUI/Components/Log/` | `LogLevel`, `LogLine`, `LogView` |
| `StacklyUI/Components/Diff/` | `DiffKind`, `DiffLine`, `DiffView` |
| `StacklyUI/Components/Inspector/` | `.stacklyInspector`, `InspectorToggle`, `InspectorHeader`, `InspectorPlaceholder` |
| `StacklyUI/Accessibility/AccessibilityID.swift` | enum bersarang per scope; fitur 68 menambah scope layar |
| `StacklyUI/Gallery/` | `ComponentSamples` (satu file per milestone: `ComponentSamples+Base.swift`, `+Sheet.swift`, `+Inspector.swift`, `+Log.swift`), halaman `ComponentsGalleryPage` dengan bagian per milestone di file sendiri |
| `StacklyUI/Tokens/Typography.swift` | helper `.stacklyTileMark(tileSize:)` |
| `Tests/StacklyUITests/Components/` | test unit per komponen |

### Model data

Fitur ini tidak menyimpan data dan tidak menambah schema atau DTO. Modelnya adalah nilai di `StacklyUI`, dengan bentuk persis di *Signature Swift*.

Isolasi mengikuti spec 0012: target memakai `.defaultIsolation(MainActor.self)`. Nilai murni tanpa closure (`TileSpec`, `ProviderMark`, `ActionConfirmation`, `ActionRole`, `ActionAvailability`, `FeedbackHold`, `ConsoleCopy`, `CheckStatus`, `StageDefinition`, `StageState`, `ProgressStage`, `ProgressOutcome`, `StagedProgressValue`, `LogLevel`, `LogLine`, `DiffKind`, `DiffLine`) ditandai `nonisolated` dan `Sendable`, termasuk enum yang bersarang di dalamnya. Tipe yang membawa closure (`StacklyAction`, `ActionHost`, `Feedback`, `CheckItem`) dan model `@Observable` (`FeedbackCenter`, `CopyConfirmation`) tetap di main actor, dan closure nya `@MainActor`.

### Signature Swift

```swift
// Components/Tile/
nonisolated public struct TileSpec: Equatable, Sendable {
    public var mark: String?
    public var tile: TileColor?
    public var name: String?
    public init(mark: String?, tile: TileColor?, name: String? = nil)
}
public struct ProviderTile: View {
    public static let smallSize: CGFloat = 18    // tabel Instances (spec 0022)
    public static let rowSize: CGFloat = 26      // baris Overview, baris Installing Runtimes
    public static let mediumSize: CGFloat = 40   // kartu Catalog, header sheet, header inspector
    public static let largeSize: CGFloat = 52    // panel detail Catalog
    public init(mark: String?, tile: TileColor?, name: String? = nil, size: CGFloat)
    public init(_ spec: TileSpec, size: CGFloat)
}
nonisolated public enum ProviderMark {
    public static func defaultMark(for name: String?) -> String
}
extension View { public func stacklyTileMark(tileSize: CGFloat) -> some View }   // di Tokens/Typography.swift

// Components/KeyValue/
public struct CardSection<Accessory: View, Footer: View, Content: View>: View {
    public init(_ title: String? = nil,
                @ViewBuilder accessory: () -> Accessory,
                @ViewBuilder footer: () -> Footer,
                @ViewBuilder content: () -> Content)
}
extension CardSection where Accessory == EmptyView, Footer == EmptyView {
    public init(_ title: String? = nil, @ViewBuilder content: () -> Content)
}
extension CardSection where Accessory == EmptyView, Footer == Text {
    public init(_ title: String? = nil, footer: String, @ViewBuilder content: () -> Content)
}
extension CardSection where Footer == EmptyView {
    public init(_ title: String? = nil, @ViewBuilder accessory: () -> Accessory, @ViewBuilder content: () -> Content)
}
public struct StacklyLabeledContentStyle: LabeledContentStyle { public init() }
extension LabeledContentStyle where Self == StacklyLabeledContentStyle { public static var stackly: Self { get } }

// Components/Copy/
public enum PlainPasteboard { public static func write(_ text: String) }
@Observable public final class CopyConfirmation {
    public private(set) var isConfirming: Bool
    public init(write: @escaping @MainActor (String) -> Void = PlainPasteboard.write,
                announce: @escaping @MainActor (String) -> Void = announce(_:),
                sleep: @escaping @Sendable (Duration) async throws -> Void = { try await Task.sleep(for: $0) })
    public func copy(_ text: String, announcement: String) async
}
public enum CopyButtonStyle { case text, icon }
public struct CopyButton: View {
    public init(_ text: String, what: String, style: CopyButtonStyle = .text, announcement: String? = nil, key: String? = nil)
}
nonisolated public struct ConsoleCopy: Equatable, Sendable {
    public var what: String
    public var announcement: String?
    public init(what: String, announcement: String? = nil)
}
public struct ConsoleBlock: View {
    public init(command: String, what: String = "command", announcement: String = "Command copied.",
                accessibilityLabel: String = "Equivalent command")
    public init(text: String, accessibilityLabel: String, copy: ConsoleCopy? = nil)
}
extension View { public func consoleSurface() -> some View }

// Components/Actions/
nonisolated public struct ActionConfirmation: Equatable, Sendable {
    public var title: String
    public var message: String
    public var actionTitle: String
    public init(title: String, message: String, actionTitle: String)
}
nonisolated public enum ActionRole: Equatable, Sendable {
    case standard(confirmation: ActionConfirmation?)
    case destructive(ActionConfirmation)
}
nonisolated public enum ActionAvailability: Equatable, Sendable {
    case enabled
    case busy
    case disabled(reason: String, showsReason: Bool)
}
public struct StacklyAction: Identifiable {
    public var id: String
    public var title: String
    public var symbol: String?
    public var role: ActionRole
    public var mutates: Bool
    public var disabledReason: String?
    public var isBusy: Bool
    public var perform: @MainActor () -> Void
    public init(id: String, title: String, symbol: String? = nil, role: ActionRole = .standard(confirmation: nil),
                mutates: Bool, disabledReason: String? = nil, isBusy: Bool = false,
                perform: @escaping @MainActor () -> Void)
    public var confirmation: ActionConfirmation? { get }
    public func availability(gate: MutationAvailability) -> ActionAvailability
    public func invoke(gate: MutationAvailability, present: (ConfirmationRequest) -> Void)
}
public struct ActionHost {
    public let gate: MutationAvailability
    public let present: @MainActor (ConfirmationRequest) -> Void
    public func run(_ action: StacklyAction)   // action.invoke(gate: gate, present: present)
}
extension EnvironmentValues { @Entry public var actionHost: ActionHost? = nil }
extension View { public func actionHost() -> some View }
public enum ActionProminence { case primary, secondary }
public struct ActionButton: View {
    public init(_ action: StacklyAction, prominence: ActionProminence = .secondary, showsReason: Bool = true)
}
public struct ActionMenu: View {
    public init(_ sections: [[StacklyAction]], title: String = "More", symbol: String = "ellipsis.circle", help: String? = nil)
}
public struct ActionMenuItems: View { public init(_ sections: [[StacklyAction]]) }

// Components/Feedback/
public struct Feedback: Identifiable {
    public let id: UUID
    public var tone: Tone
    public var message: String
    public var detail: String?
    public var code: String?
    public var action: StacklyAction?
    public init(tone: Tone, message: String, detail: String? = nil, code: String? = nil, action: StacklyAction? = nil)
    public var isPersistent: Bool { get }        // tone caution atau critical, atau action tidak nil
    public var visibleDuration: Duration { get }
    public static func success(_ message: String, detail: String? = nil) -> Feedback   // positive
    public static func started(_ message: String, detail: String? = nil) -> Feedback   // informative
    public static func neutral(_ message: String, detail: String? = nil) -> Feedback   // neutral
    public static func failure(_ body: ErrorBody) -> Feedback                          // critical
    public static func outcome(_ outcome: MutationOutcome, success: Feedback?,
                               showOperations: (@MainActor () -> Void)?) -> Feedback?
}
@Observable public final class FeedbackCenter {
    public private(set) var current: Feedback?
    public init(announce: @escaping @MainActor (String) -> Void = announce(_:),
                sleep: @escaping @Sendable (Duration) async throws -> Void = { try await Task.sleep(for: $0) })
    public func show(_ feedback: Feedback)
    public func dismiss()
    public func countdown(_ id: Feedback.ID) async   // tidur visibleDuration lalu mengosongkan kalau id masih current
}
nonisolated struct FeedbackHold: Equatable {
    var isHovered: Bool
    var isFocused: Bool
    var isWindowActive: Bool
    var countsDown: Bool { get }   // !isHovered && !isFocused && isWindowActive
}
extension EnvironmentValues { @Entry public var feedbackCenter: FeedbackCenter? = nil }
extension View { public func feedbackPresenter() -> some View }
enum DaemonText { static func sentence(_ text: String) -> String }

// Components/Sheet/
public struct StacklySheet<Content: View>: View {
    public init(title: String, subtitle: String? = nil, tile: TileSpec? = nil, primary: StacklyAction,
                cancel: StacklyAction? = nil, minWidth: CGFloat = 480, @ViewBuilder content: () -> Content)
}

// Components/Checklist/
nonisolated public enum CheckStatus: Equatable, Sendable { case passed, warning, failed, pending, unrecognized }
public struct CheckItem: Identifiable {
    public var id: String
    public var status: CheckStatus
    public var title: String
    public var detail: String?
    public var action: StacklyAction?
    public init(id: String, status: CheckStatus, title: String, detail: String? = nil, action: StacklyAction? = nil)
}
public enum CheckListLayout { case adaptive, single }
public struct CheckList: View { public init(_ items: [CheckItem], layout: CheckListLayout = .adaptive) }

// Components/Progress/
nonisolated public struct StageDefinition: Equatable, Sendable {
    public var key: String     // nilai OperationSummary.stage
    public var title: String
    public init(key: String, title: String)
}
nonisolated public enum StageState: Equatable, Sendable { case pending, running, done, failed, cancelled }
nonisolated public struct ProgressStage: Equatable, Sendable { public var key: String; public var title: String; public var state: StageState }
nonisolated public enum ProgressOutcome: Equatable, Sendable {
    case waiting, running, cancelling, succeeded, failed, needsRecovery, cancelled
}
nonisolated public struct StagedProgressValue: Equatable, Sendable {
    public var stages: [ProgressStage]
    public var runningIndex: Int?   // index tahap yang berjalan, gagal, atau dibatalkan
    public var fraction: Double?    // nil berarti indeterminate atau tanpa bar
    public var percent: Int?
    public var currentTitle: String
    public var outcome: ProgressOutcome
    public static func from(operation: OperationSummary, stages: [StageDefinition]) -> StagedProgressValue
}
public enum StagedProgressLayout { case compact, stages }
public struct StagedProgress: View {
    public init(_ value: StagedProgressValue, layout: StagedProgressLayout,
                accessibilityLabel: String = "Progress", announcesStages: Bool = false)
}

// Components/Log/
nonisolated public enum LogLevel: Equatable, Sendable { case normal, warning, error }
nonisolated public struct LogLine: Identifiable, Equatable, Sendable {
    public var id: String
    public var timeText: String?
    public var source: String?
    public var text: String
    public var level: LogLevel
    public init(id: String, timeText: String? = nil, source: String? = nil, text: String, level: LogLevel = .normal)
}
public struct LogView: View {
    public init(_ lines: [LogLine], follow: Binding<Bool>, accessibilityLabel: String = "Log",
                timeTemplate: String = "00:00:00.000", sourceTemplate: String = "stderr")
}

// Components/Diff/
nonisolated public enum DiffKind: Equatable, Sendable { case added, removed, unchanged }
nonisolated public struct DiffLine: Identifiable, Equatable, Sendable {
    public var id: Int
    public var kind: DiffKind
    public var text: String
    public init(id: Int, kind: DiffKind, text: String)
}
public struct DiffView: View { public init(_ lines: [DiffLine], emptyText: String) }

// Components/Inspector/
extension View {
    public func stacklyInspector<C: View>(isPresented: Binding<Bool>, help: String? = nil,
                                          @ViewBuilder content: () -> C) -> some View
}
public struct InspectorToggle: View { public init(isPresented: Binding<Bool>, help: String? = nil) }
public struct InspectorHeader<Actions: View>: View {
    public init(title: String, subtitle: String? = nil, tile: TileSpec? = nil, status: StatusAppearance? = nil,
                @ViewBuilder actions: () -> Actions)
}
extension InspectorHeader where Actions == EmptyView {
    public init(title: String, subtitle: String? = nil, tile: TileSpec? = nil, status: StatusAppearance? = nil)
}
public struct InspectorPlaceholder: View { public init(title: String = "No Selection", message: String? = nil) }
// ditambah 2026-10-21 (keputusan user atas Temuan langkah 14)
nonisolated public enum StacklyInspectorWidth {
    public static let compactWindowWidth: CGFloat = 1000
    public static func maxWidth(windowWidth: CGFloat?) -> CGFloat   // 280 di bawah 1000 pt, selain itu (dan nil) 360
}
extension EnvironmentValues { public var stacklyWindowWidth: CGFloat? }   // @Entry, diisi view akar jendela
```

Detail kecil yang tidak mengubah perilaku (nama parameter internal, urutan modifier) boleh disesuaikan builder; nama publik, tipe, dan default di atas tidak.

### ProviderTile

- Persegi `size × size`, `RoundedRectangle(cornerRadius: size × 0.27, style: .continuous)` (mockup 11 untuk 40, 14 untuk 52), mark `Color.white` lewat `.stacklyTileMark(tileSize: size)` (`.system(size: size × 0.37, weight: .bold)`, satu baris, `minimumScaleFactor(0.5)`). Ukuran mockup lain (20 di tabel Runtimes, 22 di menu bar, 28 di Operations) memakai parameter `size` langsung.
- `TileColor` adalah enum hasil generate spec 0017 dari `StacklyClient`.
- Mark adalah dekorasi: tile `accessibilityHidden(true)`, tidak dihitung di tabel kontras (spec 0017 AC-11), dan nama provider selalu tertulis di dekatnya.

**Warna tile** (fill dari warna sistem, varian Increase Contrast diurus macOS):

| `TileColor` | Fill |
| --- | --- |
| `blue` | `systemBlue` |
| `cyan` | `systemCyan` |
| `teal` | `systemTeal` |
| `green` | `systemGreen` |
| `yellow` | `systemYellow` |
| `orange` | `systemOrange` |
| `red` | `systemRed` |
| `brown` | `systemBrown` |
| `indigo` | `systemIndigo` |
| `gray`, `unrecognized`, nil | `systemGray` |

### Kartu seksi dan key value

- `CardSection`: judul `.headline` warna teks utama dengan aksesori di kanan (misalnya tombol), jarak 6 pt ke kartu; kartu `.cardSurface()` berisi setiap subview `content` sebagai satu baris (lewat `Group(subviews:)`, jadi `ForEach` menghasilkan satu baris per elemen), dipisah `Divider()` dengan inset kiri 14 pt; padding baris 11 vertikal dan 14 horizontal dan `minHeight` `ScaledSize.rowMinHeight`. Satu baris adalah satu view: isi baris yang terdiri dari beberapa view dibungkus `HStack` atau `VStack` oleh layar. Footer `String` tampil `.footnote` `textSecondary` di bawah kartu; footer view dipakai untuk `DisabledReasonText`.
- `.labeledContentStyle(.stackly)` lewat `ViewThatFits` dengan dua alternatif tetap: pertama `HStack` berisi label, `Spacer(minLength: 12)`, dan nilai, keduanya satu baris tanpa terpotong (`fixedSize(horizontal: true, vertical: false)`); kedua `VStack(alignment: .leading, spacing: 2)` berisi label lalu nilai yang membungkus. Label `.body` warna teks utama; nilai `.body` `textSecondary` (layar boleh memberi font sendiri, misalnya `.callout.monospaced()` untuk endpoint, path, dan digest; warna tetap `textSecondary`), `textSelection(.enabled)`; elemen VoiceOver `.combine`. Pilihan teks yang sedang aktif boleh hilang saat tata letak berganti.
- Gaya dipasang layar pada `CardSection` yang semua barisnya read only, pada inspector, atau pada satu baris read only di dalam `Form`; tidak pernah pada container `Form`, karena kontrol `Form` (field, picker, toggle) bisa memakai gaya `LabeledContent` di dalamnya. Langkah 4 memeriksa ini di macOS 27 dan mencatatnya di design.md.
- Aksesori per baris (misalnya `CopyButton`) ditaruh layar di luar `LabeledContent`: `HStack { LabeledContent(...); CopyButton(...) }`.

### CopyButton

- `.text`: tombol `.bordered` `.controlSize(.small)` berlabel "Copy". `.icon`: tombol `.borderless` dengan simbol `doc.on.doc` dan area klik 24 × 24 pt. Help dan label VoiceOver "Copy {what}". Identifier `copy.button.<key>` kalau layar memberi `key`.
- Tekan: `CopyConfirmation.copy(text, announcement:)` memanggil `PlainPasteboard.write(text)` (`clearContents()` lalu `setString(_:forType: .string)`), lalu `announce(announcement)` dengan `announcement ?? "{What} copied."` ("{What}" adalah `what` dengan huruf pertama besar), lalu `isConfirming` true selama 2 detik. Selama itu label menjadi "Copied" dengan simbol `checkmark` (gaya ikon: simbol `checkmark`), dan lebar tombol dicadangkan untuk label terpanjang supaya baris tidak bergeser. Tekanan berikutnya membatalkan hitungan lama, menulis lagi, dan memulai 2 detik dari awal.
- Copy tidak mengubah data, jadi tidak pernah diberi `.mutationGated()`. Copy nilai secret selalu lewat `RevealableSecretText` dan `ConcealedPasteboard` spec 0011.
- Copy pengumuman dari spec pemilik layar: 0021 "{title} copied.", "Address copied.", "ID copied."; 0020 "Command copied."; 0016 "Copied.".

### ConsoleBlock

- `.consoleSurface()`: `surfaceConsole`, `Radius.medium`, hairline `separatorColor` (1 pt saat Increase Contrast), padding 12 vertikal dan 14 horizontal. Hanya tersusun dari token spec 0012; tidak ada token atau pasangan kontras baru.
- `ConsoleBlock(command:)`: satu baris "$ " `textSecondary` lalu perintah, dengan `CopyButton` gaya `.text` di kanan yang menyalin `command` saja.
- `ConsoleBlock(text:accessibilityLabel:copy:)`: teks banyak baris (contoh perintah onboarding, isi diagnostic bundle), dengan `CopyButton` opsional di pojok kanan atas.
- Keduanya: `.callout.monospaced()` warna teks utama, `textSelection(.enabled)`, membungkus tanpa scroll horizontal. Teks (termasuk "$ ") ada di satu `Text` dengan `.accessibilityElement(children: .ignore)`, label dari layar dan nilai teksnya tanpa "$ "; container `.accessibilityElement(children: .contain)`, jadi tombol Copy elemen sendiri. Cuplikan kegagalan di dalam `StateBanner` tetap memakai blok kode `StateBanner` spec 0012.

### Aksi: StacklyAction, ActionHost, ActionButton, ActionMenu

- `StacklyAction` adalah satu satunya cara layar mendeskripsikan aksi yang tampil sebagai tombol, item menu, atau perintah. `disabledReason` berisi alasan capability unsupported (copy per layar dari `CapabilityStatus` spec 0017 atau capability instance), alasan state ("This instance isn’t initialized."), atau alasan validasi ("Fix the errors above."); `isBusy` diisi layar selama aksinya berjalan; `confirmation` diturunkan dari `role`.
- `availability(gate:)` dan `invoke(gate:present:)` mengikuti AC-6. Urutan alasan: alasan action dulu (paling spesifik dan perlu terlihat), lalu gate (banner jendela sudah menulisnya), lalu sibuk.
- Judul action yang role nya membawa konfirmasi diakhiri "…" (idiom macOS untuk langkah lanjutan); dijaga `assert` dengan pesan yang menyebut aturannya, karena ini aturan copy, bukan keamanan.
- `.actionHost()` memegang `@State ConfirmationRequest?`, memasang `.stacklyConfirmation(item:)` sekali, membaca `@Environment(\.mutationAvailability)`, dan mengisi `EnvironmentValues.actionHost` dengan `ActionHost` baru setiap render (jadi `gate` selalu terkini). Dipasang di akar `AppShell` (di dalam `.environment(\.mutationAvailability, …)`), di setiap `StacklySheet`, dan nanti di popover menu bar fitur 32.
- `ActionButton` dan item menu membaca `actionHost` dan `mutationAvailability` dari environment untuk availability, lalu menjalankan lewat `host.run(action)`. Tanpa host: action tanpa konfirmasi tetap dijalankan lewat `invoke(gate:present:)` dengan gate environment; action dengan konfirmasi memanggil `assertionFailure("an action that confirms needs .actionHost()")` dan tidak menjalankan apa pun.
- `ActionButton`: `Label(title, systemImage: symbol)` atau `Text(title)`, gaya sesuai AC-7, `.disabled(reason:)` untuk alasan action dan gate, `DisabledReasonText` di bawah tombol saat `showsReason` dan alasan action ada, `ProgressView` kecil saat sibuk, identifier `action.button.<id>`. `ActionButton` tidak memasang `keyboardShortcut`.
- `ActionMenu`: `Menu` native dengan label ikon, help `help ?? title`, identifier `action.more`. `ActionMenuItems` merender item yang sama untuk `.contextMenu`. Setiap item `Button(role:)` dengan judul dan subtitle alasan (AC-8), dan semua item non `.enabled` nonaktif. Bagian dipisah `Divider`, dan bagian berisi aksi destruktif ditaruh terakhir. Tooltip yang menyebut shortcut dipasang halaman lewat `CommandMap.helpText(for:)` spec 0069, bukan oleh komponen ini.

### Umpan balik setelah aksi

**Pemetaan hasil mutasi** (`Feedback.outcome(_:success:showOperations:)`):

| `MutationOutcome` | Tone | Pesan | Detail | Kode | Aksi |
| --- | --- | --- | --- | --- | --- |
| `completed` | dari `success` | dari `success` (nil berarti tanpa umpan balik) | dari `success` | tidak ada | dari `success` |
| `rejected(body)` | `critical` | `DaemonText.sentence(body.message)` | `body.hint` lewat `DaemonText.sentence`, atau tidak ada | `body.code` | tidak ada |
| `notSent(_)` | `caution` | `MutationFeedback.notSent` ("stacklyd is unavailable. Nothing was sent.") | tidak ada | tidak ada | tidak ada |
| `outcomeUnknown(_, key:)` | `caution` | `MutationFeedback.outcomeUnknown` ("Lost the connection to stacklyd. The action may have run; check Operations before trying again.") | tidak ada | tidak ada | `StacklyAction` "Show Operations" (`mutates` false) kalau `showOperations` tidak nil |

- `Feedback.failure(_:)` memberi baris `rejected` yang sama untuk operasi yang berakhir `failed` (`OperationFailureInfo.error`). `.success`, `.started`, dan `.neutral` memberi tone `positive`, `informative`, dan `neutral` tanpa aksi (misalnya `.started("Installing PostgreSQL 18.1. Staged, verified, then committed.")`, `.neutral` untuk operasi yang dibatalkan). Copy sukses dan copy dibatalkan milik spec layar.
- `DaemonText.sentence(_:)` menjadikan huruf pertama besar dan menambah titik kalau belum ada (pesan daemon diawali huruf kecil tanpa titik, spec 0006). Fungsi ini pindah dari `RevealModel.sentence`; `RevealModel` dan `SecretEntryModel` memanggil yang baru.
- `visibleDuration`: `min(10, max(4, ceil(jumlah karakter pesan dan detail / 15)))` detik, kira kira kecepatan baca 15 karakter per detik dengan batas bawah 4 detik.
- `FeedbackCenter.show(_:)`: kalau `current` bersifat tetap dan umpan balik baru bersifat sementara, umpan balik baru hanya diumumkan dan `current` tidak berubah; selain itu `current` diganti dan umpan balik baru diumumkan. Pengumuman berisi pesan lalu detail (kode tidak dibacakan). `dismiss()` mengosongkan `current`. `countdown(_:)` langsung kembali untuk umpan balik tetap; untuk yang sementara ia tidur `visibleDuration` lewat `sleep` yang bisa diganti, lalu mengosongkan `current` hanya kalau `current.id` masih sama; cancel saat tidur tidak mengosongkan apa pun.
- `FeedbackCard` memegang `@State` hover (`.onHover`), fokus (`@FocusState` tombol kartu), dan membaca `@Environment(\.controlActiveState)` (aktif saat `.key` atau `.active`) ke dalam `FeedbackHold`, lalu menjalankan `.task(id: [feedback.id, hold.countsDown]) { if hold.countsDown { await center.countdown(feedback.id) } }`. Kartu yang hilang membuang state tahanannya, jadi tahanan tidak pernah tersangkut.
- Tata letak kartu: `HStack(alignment: .firstTextBaseline, spacing: 12)` berisi simbol tone (warna fill tone, `.stacklySymbol`), lalu kelompok teks (pesan, detail, kode bertumpuk) sebagai satu elemen VoiceOver dengan label pesan dan nilai detail dan kode, lalu tombol aksi `ActionButton` `.secondary` `.controlSize(.small)`, lalu Dismiss `.borderless`. Padding 12 vertikal dan 16 horizontal, `toneSurface(tone, on: .card, strength: .banner, in: RoundedRectangle(cornerRadius: Radius.large))`, `.shadow(color: .black.opacity(0.18), radius: 12, y: 4)`. Container `.accessibilityElement(children: .contain)`. Identifier `feedback.card`, `feedback.action`, `feedback.dismiss`.
- `.feedbackPresenter()` membaca `EnvironmentValues.feedbackCenter` dan memasang kartu lewat `.safeAreaInset(edge: .bottom, spacing: 0)` dengan transisi `StacklyMotion.crossfade`. Tanpa center di environment (preview), presenter tidak menggambar apa pun.
- Aturan pemakaian di design.md: umpan balik hanya untuk hasil aksi pengguna, bukan perubahan status yang terjadi sendiri; kabar baik yang hasilnya juga terlihat di layar boleh dilewati (misalnya Start yang diterima cukup dengan status dan pengumuman spec 0022); kegagalan selalu juga terlihat di tempatnya (Needs Attention, banner item, Operations), sehingga umpan balik tidak pernah satu satunya catatan.

### StacklySheet

- `cancel` nil berarti action "Cancel" (`mutates` false) yang memanggil `dismiss` environment. Layar memberi `cancel` sendiri kalau Cancel punya arti lain (misalnya `operation.cancel` dengan konfirmasi di mode progress spec 0020).
- Latar: `.presentationBackground(StacklyColor.surfaceWindow)` (mockup juga menggambar sheet opak). Layar yang memakai `Form` di dalam sheet memberi `.scrollContentBackground(.hidden)` supaya latar sheet terlihat dan baris `Form` tetap memakai latar native nya.
- Header: padding atas 20, samping 22, bawah 6; `ProviderTile` `mediumSize` lalu judul dan subjudul. Isi: `ScrollView` dengan padding atas 12, samping 22, bawah 16, `VStack(spacing: 16)`; tinggi `ScrollView` adalah tinggi isi (diukur lewat `onGeometryChange`) dibatasi tinggi `visibleFrame` layar jendela dikurangi 160 pt, jadi sheet pendek tidak bergulir dan sheet panjang bergulir tanpa memotong teks. Footer di luar `ScrollView`, padding atas 12, samping 22, bawah 18: alasan (AC-12), `Spacer`, Cancel, aksi utama. Identifier `sheet.cancel` dan `sheet.primary`.
- Lebar `minWidth` (spec 0020 memberi 560).
- Aksi utama `.borderedProminent` dengan `.keyboardShortcut(.defaultAction)`, kecuali destruktif (`.bordered`, `role: .destructive`, tanpa shortcut). Cancel `.keyboardShortcut(.cancelAction)` dan tetap aktif selama aksi utama sibuk. Hanya dua shortcut ini yang dipasang komponen spec ini (aturan sumber spec 0069).

### CheckList

Layar membungkus `CheckList` dengan `CardSection` (misalnya judul "Preflight").

**Status checklist** (tabel ini menggantikan tabel status preflight spec 0020 di design.md):

| `CheckStatus` | Kata VoiceOver | Tone | Simbol |
| --- | --- | --- | --- |
| `passed` | Passed | `positive` | `checkmark.circle.fill` |
| `warning` | Warning | `caution` | `exclamationmark.triangle.fill` |
| `failed` | Failed | `critical` | `xmark.circle.fill` |
| `pending` | Checking | `neutral` | `ProgressView` kecil setelah 300 ms, ruang simbol kosong sebelum itu |
| `unrecognized` | Unrecognized status | `caution` | `questionmark.diamond.fill` |

- Grid `.adaptive`: `LazyVGrid(columns: [GridItem(.adaptive(minimum: 200), spacing: 16, alignment: .topLeading)], alignment: .leading, spacing: 8)`. Di sheet 560 pt, lebar isi kartu 488 pt memberi dua kolom.
- Baris: `HStack(alignment: .firstTextBaseline, spacing: 8)` berisi simbol (warna fill tone, `.stacklySymbol(size: 15, relativeTo: .callout)`) lalu judul dan detail bertumpuk; ketiganya satu elemen VoiceOver (`.ignore` dengan label AC-13). `ActionButton` sekunder kecil opsional ada di bawahnya, di luar elemen itu. Bentuk simbol berbeda per status, jadi Differentiate Without Color tidak mengubah apa pun. Perubahan status memakai crossfade. Identifier `checklist.row.<id>`.
- Ringkasan dan pengumuman perubahan status adalah urusan layar (spec 0020 mengumumkan perubahan preflight).

### StagedProgress

**Pemetaan operasi ke tahap** (`i` adalah index `operation.stage` di `stages`, mulai 0; `n` jumlah tahap):

| `operation.state` dan `stage` | Tahap | `runningIndex` | `fraction` dan `percent` | `currentTitle` | `outcome` |
| --- | --- | --- | --- | --- | --- |
| `queued` | semua `pending` | nil | nil, nil | "Queued" | `waiting` |
| `running`, stage dikenal | sebelum `i` `done`, `i` `running`, sesudahnya `pending` | `i` | rumus AC-14 | judul tahap `i` | `running` |
| `cancelRequested`, stage dikenal | sama dengan baris `running` | `i` | rumus AC-14 | "Cancelling…" | `cancelling` |
| `running` atau `cancelRequested`, `stage` nil | tahap pertama `running`, sisanya `pending` | 0 | 0, 0 | judul tahap pertama (atau "Cancelling…") | `running` (atau `cancelling`) |
| `running` atau `cancelRequested`, stage tidak dikenal, atau `n` 0 | semua `pending` | nil | nil, nil | "Working…" (atau "Cancelling…") | `running` (atau `cancelling`) |
| `succeeded` | semua `done` | nil | 1, 100 | "Done" | `succeeded` |
| `failed`, stage dikenal | sebelum `i` `done`, `i` `failed`, sesudahnya `pending` | `i` | nil, nil | "Failed at {judul tahap i}" | `failed` |
| `failed`, stage nil, tidak dikenal, atau `n` 0 | semua `pending` | nil | nil, nil | "Failed" | `failed` |
| `needsRecovery` | seperti dua baris `failed` | seperti `failed` | nil, nil | "Needs Recovery" | `needsRecovery` |
| `cancelled`, stage dikenal | sebelum `i` `done`, `i` `cancelled`, sesudahnya `pending` | `i` | nil, nil | "Cancelled" | `cancelled` |
| `cancelled`, stage nil, tidak dikenal, atau `n` 0 (termasuk batal saat antre) | semua `pending` | nil | nil, nil | "Cancelled" | `cancelled` |

Untuk baris yang memakai rumus AC-14, `fraction` adalah `Double` dari pembilang dan penyebut yang sama dengan `percent` (`succeeded` memberi 1). Definisi tahap datang dari spec pemilik operasi (spec 0018: lima tahap install; spec 0020: enam tahap create; fitur 35, 50, 52 untuk operasi lain).

**Teks progress** (tata letak `.compact` dan nilai VoiceOver; `k` adalah `runningIndex + 1`):

| `outcome` | Teks `.compact` | Bar | Nilai VoiceOver |
| --- | --- | --- | --- |
| `waiting` | "Queued" | tidak ada | "Queued" |
| `running` dengan `percent` | "{judul} · {percent}%" | determinate | "Stage {k} of {n}, {judul}, {percent} percent" |
| `running` tanpa `percent` | "Working…" | indeterminate | "Working" |
| `cancelling` | "Cancelling… · {percent}%" atau "Cancelling…" | seperti `running` | "Cancelling, stage {k} of {n}, {percent} percent" atau "Cancelling" |
| `succeeded` | simbol `checkmark.circle.fill` (fill `positive`) dan "Done" | tidak ada | "Done" |
| `failed` | simbol `xmark.circle.fill` (fill `critical`) dan `currentTitle` | tidak ada | `currentTitle` |
| `needsRecovery` | simbol `xmark.circle.fill` (fill `critical`) dan "Needs Recovery" | tidak ada | "Needs Recovery" |
| `cancelled` | simbol `minus.circle.fill` (fill `neutral`) dan "Cancelled" | tidak ada | "Cancelled" |

**State tahap** (tata letak `.stages`):

| `StageState` | Simbol | Warna simbol | Teks |
| --- | --- | --- | --- |
| `done` | `checkmark.circle.fill` | fill `positive` | warna teks utama |
| `running` | `circle.inset.filled` | fill `informative` | warna teks utama, semibold |
| `pending` | `circle` | `textSecondary` | `textSecondary` |
| `failed` | `xmark.circle.fill` | fill `critical` | teks tone `critical`, semibold |
| `cancelled` | `minus.circle.fill` | fill `neutral` | `textSecondary` |

- Tahap di `.stages` disusun `StageFlowLayout` (membungkus, jarak 16 horizontal dan 6 vertikal) dengan teks `.callout`. Bar `ProgressView(value:)` linear native; animasi nilai lewat `StacklyMotion.progress(reduceMotion:)`. Seluruh komponen satu elemen VoiceOver dengan label `accessibilityLabel` dan nilai dari tabel *Teks progress*. Identifier `progress.view`.

### LogView

- `ScrollView` dengan `LazyVStack(spacing: 0)` di atas `.consoleSurface()` (padding per baris 2 vertikal dan 14 horizontal). Identifier `log.list`.
- Kolom: waktu (`timeText`, diformat fitur 27) dan sumber, keduanya `.callout.monospaced()` `textSecondary`, dengan lebar minimum dari teks templat tersembunyi (`timeTemplate`, `sourceTemplate`) supaya baris sejajar; sumber yang lebih panjang tampil utuh dan menggeser teks baris itu. Lalu simbol untuk `warning` (`exclamationmark.triangle.fill`, fill `caution`) dan `error` (`xmark.circle.fill`, fill `critical`), dengan ruang kosong selebar simbol untuk `normal`. Lalu teks.
- Follow memakai posisi scroll per `LogLine.id` tanpa animasi. `onScrollPhaseChange` membedakan gulir pengguna (fase interacting dan decelerating) dari gulir program; saat gulir pengguna kembali ke idle dengan jarak dari dasar lebih dari 24 pt, `follow` menjadi false. Kembali ke dasar tidak menyalakan `follow`; itu tugas toggle fitur 27.
- Pemilihan teks hanya per baris (batas `textSelection` SwiftUI); copy beberapa baris sekaligus adalah aksi fitur 27.

### DiffView

- `.cardSurface()` dengan padding 8, `LazyVStack(spacing: 2)`; baris `HStack(spacing: 8)` berisi kolom tanda selebar satu karakter monospace dan teks, padding 3 vertikal dan 8 horizontal, latar `toneSurface(_, on: .card, strength: .banner, in: RoundedRectangle(cornerRadius: Radius.small))` untuk baris yang berubah. Teks bisa dipilih dan membungkus. Identifier `diff.list`.
- Pasangan warnanya sudah dibuktikan spec 0012: teks tone di atas tint tone yang sama (P3) untuk tanda, dan warna teks utama di atas tint tone `.banner` (P4) untuk isi.

### Inspector

- `.stacklyInspector` memasang `.inspector(isPresented:)` berisi `ScrollView { VStack(alignment: .leading, spacing: 16) }` dengan padding 16 dan latar `StacklyColor.surfaceWindow` yang mengisi kolom, plus `.inspectorColumnWidth(min: 240, ideal: 280, max: StacklyInspectorWidth.maxWidth(windowWidth: stacklyWindowWidth))`, dan `ToolbarItem(placement: .primaryAction) { InspectorToggle(isPresented:help:) }`.
- *Lebar dan kolom halaman (2026-10-21, keputusan user atas Temuan langkah 14):* di jendela minimum 900 pt dengan sidebar (236 pt) dan inspector (280 pt) terbuka, kolom halaman 384 pt (424 pt dengan inspector 240 pt). Itu diterima: setiap halaman dengan inspector bekerja di kolom halaman 384 pt (tabel dan kartu membungkus atau bergulir, tanpa teks terpotong), dan jaminan konten minimum 640 pt spec 0013 hanya berlaku saat inspector tertutup. Supaya 384 pt benar benar batas bawah, maksimum inspector 280 pt di jendela di bawah `StacklyInspectorWidth.compactWindowWidth` (1000 pt) dan 360 pt di jendela 1000 pt atau lebih. Lebar jendela datang dari `EnvironmentValues.stacklyWindowWidth`, yang diisi view akar jendela (`AppShell` hari ini, `MainWindowRoot` setelah langkah 2 milestone 1 spec 0016) lewat `onGeometryChange(for: CGFloat.self) { $0.size.width }`; nil (galeri, preview, sheet) berarti 360 pt.
- `InspectorToggle`: label "Show Inspector" atau "Hide Inspector" sesuai binding, simbol `sidebar.trailing`, help `help ?? label`, identifier `inspector.toggle`. Fitur 69 memasang perintah View > Show/Hide Inspector ke binding yang sama.
- Layar menyimpan binding di `@SceneStorage("<halaman>.inspector")` dengan default terbuka (aturan design.md).
- `InspectorHeader`: kolom rata tengah, jarak 6, teks rata tengah dan membungkus; aksi di slot `actions` (biasanya `ActionButton`).
- `InspectorPlaceholder`: judul `.title3.weight(.semibold)` warna teks utama dan pesan `.callout` `textSecondary`, rata tengah di kolom, identifier `inspector.placeholder`.
- Isi inspector dirakit layar dari `CardSection`, `.labeledContentStyle(.stackly)`, `StateBanner`, dan `ActionButton`.

### Aksesibilitas

- Urutan fokus mengikuti urutan baca; tidak ada komponen yang mengubah urutan fokus layar. Kartu umpan balik tidak mengambil fokus saat muncul; tombolnya bisa dicapai dengan Tab, dan selama fokus ada di sana kartu tidak hilang.
- Label VoiceOver: tile tersembunyi; baris key value "{label}, {value}"; Copy "Copy {what}"; blok konsol label dari layar; tombol aksi judulnya, ditambah nilai "In progress" saat sibuk dan help berisi alasan saat nonaktif; menu "More"; kartu umpan balik pesannya dengan detail dan kode sebagai nilai; baris checklist, progress, log, dan diff menurut AC-13, AC-15, AC-16, AC-17; toggle inspector labelnya.
- Pengumuman hanya dari: `CopyButton` (setelah menulis), `FeedbackCenter.show`, dan `StagedProgress` dengan `announcesStages`. Komponen lain tidak mengumumkan apa pun supaya tidak ada pengumuman ganda dengan layar.
- Cek teks besar adalah cek lebar sempit 640 pt spec 0012, karena text style macOS 27 tidak ikut `dynamicTypeSize`.

### Galeri, sampel, dan accessibility identifier

**State di galeri** (halaman "Components", urutan bagian tetap; setiap baris adalah state yang dirender snapshot fitur 68):

| Komponen | State yang tampil |
| --- | --- |
| `ProviderTile` | enam tile mockup (Pg blue, My teal, Na green, Ch yellow, Vk red, Tr cyan) di 18, 26, 40, dan 52 pt; mark nil dengan nama; tile nil; `unrecognized` |
| `CardSection` dan key value | baris biasa, nilai monospace, nilai panjang yang turun ke bawah label di 640 pt, baris dengan `CopyButton` di luar `LabeledContent`, footer teks, footer `DisabledReasonText` |
| `CopyButton` | `.text` diam, `.text` "Copied", `.icon` diam, `.icon` terkonfirmasi |
| `ConsoleBlock` | perintah dengan Copy, perintah panjang yang membungkus, teks banyak baris |
| `ActionButton` | primary, secondary, destruktif dengan konfirmasi, sibuk, nonaktif dengan alasan terlihat, nonaktif karena gate `.blocked` |
| `ActionMenu` | tombol More (isi menu dicek manual dan lewat XCUITest fitur 68) |
| `FeedbackCard` | satu kartu per tone, dengan detail dan kode, dengan aksi, pesan panjang di 640 pt |
| `StacklySheet` | isi sheet create dengan tile, aksi utama nonaktif dengan alasan, alasan gate `.blocked` di footer, aksi utama sibuk, aksi utama destruktif (dirender sebagai view, plus tombol yang membuka sheet sungguhan) |
| `CheckList` | setiap `CheckStatus`, baris dengan aksi, detail panjang, `.adaptive` dan `.single` |
| `StagedProgress` | `.compact` 38%, `.compact` stage tidak dikenal, `.compact` queued, `.stages` running, cancelling, failed, needs recovery, cancelled saat antre, succeeded |
| `LogView` | baris normal, warning, dan error, baris panjang yang membungkus, sumber panjang, waktu dan sumber kosong |
| `DiffView` | tambah, hapus, tidak berubah, dan kosong |
| Inspector | `InspectorHeader` dengan tile, status, dan aksi; `InspectorHeader` tanpa aksi; `InspectorPlaceholder`; satu `CardSection`; plus toggle untuk `.stacklyInspector` sungguhan |

**Accessibility identifier** (`AccessibilityID`, bentuk `<scope>.<part>[.<key>]` milik spec 0068, key hanya kunci data stabil):

| Konstanta | Nilai |
| --- | --- |
| `AccessibilityID.Feedback.card`, `.action`, `.dismiss` | `feedback.card`, `feedback.action`, `feedback.dismiss` |
| `AccessibilityID.Sheet.cancel`, `.primary` | `sheet.cancel`, `sheet.primary` |
| `AccessibilityID.Inspector.toggle`, `.placeholder` | `inspector.toggle`, `inspector.placeholder` |
| `AccessibilityID.Action.more`, `.button(_ id:)` | `action.more`, `action.button.<id>` |
| `AccessibilityID.Checklist.row(_ id:)` | `checklist.row.<id>` |
| `AccessibilityID.Progress.view` | `progress.view` |
| `AccessibilityID.Log.list` | `log.list` |
| `AccessibilityID.Diff.list` | `diff.list` |
| `AccessibilityID.Copy.button(_ key:)` | `copy.button.<key>` (hanya kalau layar memberi `key`) |

Identifier tidak mengubah label atau urutan VoiceOver.

### Aturan sumber baru

Ditambahkan ke `SourceRuleTests` di langkah 2 dengan contoh terlarang dan contoh yang boleh (folder yang diizinkan boleh belum ada, sama seperti aturan dialog spec 0012):

| Aturan | Pola | Boleh di |
| --- | --- | --- |
| pasteboard di luar helper copy | `\bNSPasteboard\b` | `Components/Copy/PlainPasteboard.swift`, `Secrets/ConcealedPasteboard.swift` |
| inspector di luar komponen inspector | `\.inspector\(` | `Components/Inspector/` |

Ukuran font mark tile ada di `Tokens/Typography.swift`, file yang sudah diizinkan aturan ukuran font spec 0012, jadi aturan itu tidak berubah.

### API surface

| API `StacklyUI` | Bentuk | Input utama | Output | Catatan |
| --- | --- | --- | --- | --- |
| `TileSpec`, `ProviderTile`, `ProviderMark.defaultMark(for:)` | struct, view, fungsi murni | mark, `TileColor`, nama, ukuran | tile dekoratif | AC-2 |
| `.stacklyTileMark(tileSize:)` | modifier di `Typography.swift` | ukuran tile | font mark | satu satunya ukuran tetap baru |
| `CardSection` | view, empat initializer | judul, aksesori, footer, isi | kartu berbaris | AC-3 |
| `.labeledContentStyle(.stackly)` | `LabeledContentStyle` | label, nilai | baris key value read only | AC-3 |
| `PlainPasteboard.write(_:)` | fungsi | string | pasteboard umum | AC-4 |
| `CopyConfirmation`, `CopyButton` | model, view | teks, `what`, gaya, pengumuman, key | copy plus konfirmasi | AC-4 |
| `ConsoleBlock`, `ConsoleCopy`, `.consoleSurface()` | view, struct, modifier | perintah atau teks | blok monospace | AC-5 |
| `StacklyAction`, `ActionRole`, `ActionConfirmation`, `ActionAvailability` | struct, enum | lihat *Signature Swift* | `availability(gate:)`, `invoke(gate:present:)` | AC-6, dipakai spec 0069 |
| `ActionHost`, `.actionHost()`, `EnvironmentValues.actionHost` | struct, modifier, environment | gate dari environment | satu konfirmasi per jendela atau sheet | AC-6 |
| `ActionButton` | view | action, prominence, `showsReason` | tombol | AC-7 |
| `ActionMenu`, `ActionMenuItems` | view | bagian action | menu More, isi menu konteks | AC-8 |
| `Feedback` dan fungsi statisnya | struct, fungsi murni | tone, teks, `MutationOutcome`, `ErrorBody` | umpan balik atau nil | AC-10 |
| `FeedbackCenter`, `FeedbackHold`, `EnvironmentValues.feedbackCenter`, `.feedbackPresenter()`, `FeedbackCard` | model, nilai, environment, modifier, view | `Feedback` | kartu umpan balik | AC-9, AC-11 |
| `DaemonText.sentence(_:)` | fungsi internal | teks daemon | kalimat | AC-10 |
| `StacklySheet` | view | judul, subjudul, tile, aksi utama, cancel, lebar, isi | sheet dengan footer | AC-12 |
| `CheckStatus`, `CheckItem`, `CheckList` | enum, struct, view | item | checklist | AC-13 |
| `StageDefinition`, `StagedProgressValue.from(operation:stages:)` | struct, fungsi murni | `OperationSummary` spec 0007 | state tahap, fraksi, persen, judul, outcome | AC-14 |
| `StagedProgress` | view | nilai, tata letak | progress | AC-15 |
| `LogLine`, `LogView` | struct, view | baris, binding follow, templat kolom | log | AC-16 |
| `DiffLine`, `DiffView` | struct, view | baris, teks kosong | diff | AC-17 |
| `.stacklyInspector`, `InspectorToggle`, `InspectorHeader`, `InspectorPlaceholder`, `StacklyInspectorWidth`, `EnvironmentValues.stacklyWindowWidth` | modifier, view, fungsi murni, environment | binding, help, isi, lebar jendela | inspector dengan lebar maksimum menurut lebar jendela | AC-18 |
| `AccessibilityID` | enum | | konstanta identifier | AC-20 |
| `ComponentSamples`, `ComponentsGalleryPage` | Debug saja | | nilai contoh, halaman galeri | AC-20 |

Kesalahan tidak dilempar oleh API ini; error datang dari daemon lewat `MutationOutcome`, `ErrorBody`, dan `OperationSummary`.

### Value sourcing

| Komponen atau aksi | Nilai | Sumber |
| --- | --- | --- |
| `ProviderTile` | mark, warna | `ProviderSummary.mark` dan `.tile` dari `CatalogStore` spec 0017 (diberikan layar lewat `TileSpec`); default mark dari `ProviderMark.defaultMark(for: name)` dengan `name` dari nama tampil atau `software` instance (spec 0022); fill dari tabel *Warna tile* |
| `ProviderTile` | ukuran | konstanta bernama di *Signature Swift* (mockup 18, 26, 40, 52); ukuran mockup lain dari layar |
| Key value | label, nilai | copy dan format dari spec layar (0017, 0018, 0021, 0022) |
| `CopyButton` | teks yang disalin | layar (alamat, ID, digest, perintah setara dari `equivalent_command` spec 0020) |
| `CopyButton` | pengumuman | spec layar (lihat *CopyButton*), default "{What} copied." |
| `CopyButton` | lama konfirmasi | 2 detik, spec ini |
| `ConsoleBlock` | teks | layar (perintah dari daemon spec 0020, contoh `OnboardingCopy` spec 0016, isi bundle fitur 34) |
| `StacklyAction` | judul, simbol, role dan konfirmasi, `mutates` | spec layar |
| `StacklyAction` | `disabledReason` | capability dari `CapabilityStatus` spec 0017 atau capability instance (fitur 21 atau 22), state dari DTO layar, copy alasan dari spec layar |
| `StacklyAction` | `isBusy` | view model layar (misalnya `inFlight` dan `pending` spec 0022) |
| `ActionAvailability`, `ActionHost.gate` | alasan gate | `MutationAvailability` di environment, diisi shell spec 0013 |
| `ActionConfirmation` | judul, pesan, label | spec layar mengikuti aturan Konfirmasi spec 0012 |
| `Feedback` dari `rejected` | pesan, hint, kode | `ErrorBody.message`, `.hint`, `.code` (spec 0006, sudah diredaksi spec 0011) |
| `Feedback` dari `notSent`, `outcomeUnknown` | pesan | `MutationFeedback` spec 0013 |
| `Feedback` sukses, mulai, netral | pesan | copy spec layar (misalnya spec 0018 "Installing {name} {version}. Staged, verified, then committed.", spec 0016 "Setup complete.") |
| `Feedback` | lama tampil | rumus `visibleDuration` di spec ini |
| `FeedbackHold` | hover, fokus, jendela aktif | `.onHover`, `@FocusState`, `controlActiveState` di `FeedbackCard` |
| `FeedbackCenter` | jendela | view akar jendela (`LiveShell`, lalu `MainWindowRoot` spec 0016) |
| `StacklySheet` | judul, subjudul, tile, lebar | spec layar (spec 0020: "New Instance", subjudul mockup, 560 pt) |
| `StacklySheet` | tinggi maksimum isi | `visibleFrame` layar jendela dikurangi 160 pt |
| `StacklySheet` | alasan footer | `ActionAvailability` aksi utama dan gate environment |
| `CheckItem` | status, judul, detail | DTO layar (misalnya `PreflightCheck` spec 0020, hasil doctor fitur 34), dipetakan layar ke `CheckStatus` |
| `StagedProgressValue` | tahap sekarang, progress, state | `OperationSummary.stage`, `.progress`, `.state` spec 0007 dari `OperationCenter.updates(for:)` spec 0013 |
| `StagedProgressValue` | judul tahap | `StageDefinition` dari spec pemilik operasi (spec 0018 tabel *Alur install*, spec 0020 enam tahap create) |
| `LogLine` | waktu, sumber, teks, tingkat | fitur 27 (format waktu dan tingkat ditentukan di sana) |
| `DiffLine` | baris | fitur 29 |
| Inspector | isi, binding, help | layar; `@SceneStorage` per halaman; help dengan shortcut dari `CommandMap.helpText(for:)` spec 0069 |
| Inspector | lebar maksimum kolom | `StacklyInspectorWidth.maxWidth(windowWidth:)` dari `EnvironmentValues.stacklyWindowWidth`, diisi view akar jendela lewat `onGeometryChange` (2026-10-21) |
| Warna, font, inset, radius | nilai | token spec 0012 dan tabel di spec ini |
| Identifier | string | tabel *Accessibility identifier* |

### Key invariants

- Komponen spec ini tidak pernah memakai glass atau `Material`; semua permukaannya opak dan warnanya dari token spec 0012 atau warna sistem, kecuali putih untuk mark tile dekoratif dan hitam transparan untuk bayangan kartu umpan balik.
- Tone tidak pernah satu satunya pembawa arti: setiap tone datang dengan simbol atau kata, dan simbol yang membawa arti tidak pernah memakai `StacklyColor.decorative`.
- Satu jendela punya paling banyak satu kartu umpan balik dan satu konfirmasi pada satu waktu.
- Umpan balik `caution`, `critical`, atau yang punya aksi tidak pernah hilang sendiri, dan tidak pernah digantikan umpan balik sementara.
- Aksi destruktif selalu membawa konfirmasi (dijaga tipe `ActionRole`) dan tidak pernah tombol default.
- Tombol capability unsupported tidak pernah disembunyikan, dan alasannya terlihat tanpa hover di tombol (teks di bawahnya atau di konten halaman) dan di menu (subtitle).
- Copy hanya menulis saat ditekan, tidak pernah untuk secret, dan tidak pernah di gate.
- Tidak ada versi kedua komponen ini di folder layar.

### Security model

Tidak ada peran pengguna, data sensitif, atau akses baru. Komponen hanya menampilkan yang diberikan layar dari daemon, yang sudah meredaksi pesan error, detail, dan cuplikan log (spec 0011). `CopyButton` dan `PlainPasteboard` menulis pasteboard umum biasa, jadi copy secret wajib lewat `ConcealedPasteboard` spec 0011 (dijaga aturan sumber pasteboard dan aturan design.md). Daemon tetap memvalidasi setiap aksi walau tombolnya nonaktif. `ComponentSamples` dan preview hanya memakai data contoh mockup tanpa kredensial nyata.

### Configuration required

Tidak ada env var, secret, atau setting baru.

### Critical test scenarios

- Happy path: `ProviderMark.defaultMark(for:)` untuk "PostgreSQL", "9front", "Á", nama kosong, dan nil, serta setiap baris tabel *Warna tile*, memverifikasi **AC-2**.
- Happy path: `StagedProgressValue.from` untuk setiap baris tabel *Pemetaan operasi ke tahap*, termasuk `counted` dengan `total` 0, `done` di atas `total`, 29 dari 100 di tahap pertama dari satu tahap (29%), stage tidak dikenal, daftar tahap kosong, batal saat antre, dan persen yang tidak mencapai 100 sebelum `succeeded`; dan teks serta nilai VoiceOver setiap baris tabel *Teks progress*, memverifikasi **AC-14**, **AC-15**.
- Happy path: `Feedback.outcome` untuk keempat case dan `Feedback.failure`, termasuk hint nil, pesan yang sudah bertitik, dan `showOperations` nil; `visibleDuration` untuk pesan pendek, 100 karakter, dan 300 karakter; `isPersistent` per tone dan dengan aksi, memverifikasi **AC-9**, **AC-10**.
- Failure case: `FeedbackCenter` dengan announce dan sleep tiruan: `countdown` mengosongkan umpan balik sementara setelah `visibleDuration`, tidak mengosongkan kalau id sudah berganti, tidak mengosongkan saat dibatalkan di tengah tidur, dan langsung kembali untuk umpan balik tetap; `show` sementara di atas umpan balik tetap hanya mengumumkan; `show` lain mengganti dan mengumumkan sekali; `dismiss` mengosongkan. `FeedbackHold.countsDown` untuk setiap kombinasi hover, fokus, dan jendela aktif, memverifikasi **AC-9**, **AC-11**.
- Failure case: `availability(gate:)` untuk setiap kombinasi `disabledReason`, `mutates`, gate, dan `isBusy`; `invoke(gate:present:)` tidak berbuat apa pun saat tidak `.enabled`, memanggil `present` dengan request (judul, pesan, label, role) yang benar untuk `.standard(confirmation:)` dan `.destructive`, dan `perform` langsung tanpa konfirmasi; `ActionHost.run` memakai gate host, memverifikasi **AC-6**.
- Failure case: `CopyConfirmation` dengan writer, announce, dan sleep tiruan menulis sekali per tekanan, mengumumkan sekali, dan memulai ulang 2 detik pada tekanan kedua, memverifikasi **AC-4**.
- Failure case: teks VoiceOver baris checklist, log (termasuk waktu dan sumber kosong dan tingkat normal), dan diff dari fungsi murni, memverifikasi **AC-13**, **AC-16**, **AC-17**.
- Aturan sumber: contoh terlarang untuk pasteboard dan inspector gagal, contoh yang boleh lulus, dan aturan spec 0012 tetap lulus untuk `Components/`, memverifikasi **AC-21**, **AC-1**.
- Struktur: test pemindai sumber memastikan `AppShell.swift` memasang `.feedbackPresenter()` dan `.actionHost()` sebelum `.environment(\.mutationAvailability` di rantai modifier, `LiveShell.swift` membuat satu `FeedbackCenter`, dan `StacklySheet.swift` memasang `.actionHost()`, memverifikasi **AC-11**, **AC-12**; perilakunya di app nyata dicek di /check verify.
- Aksesibilitas dan tampilan: cek di /check verify (di bawah), memverifikasi **AC-3**, **AC-5**, **AC-7**, **AC-8**, **AC-12**, **AC-18**, **AC-19**, **AC-20**.
- Auth/permission: tidak berlaku; tidak ada peran pengguna di GUI.

### Cek di /check verify

Dicek di jendela Design Gallery app Debug (halaman Components) dan di app dengan daemon development, dengan setting sistem yang diubah sungguhan oleh user (Claude tidak mengubah setting sistem):

- Keyboard: Tab dan Space mencapai dan menjalankan setiap tombol komponen (Copy, aksi, More dan itemnya, Dismiss dan aksi kartu, footer sheet, toggle inspector); Return dan Escape di sheet sesuai AC-12 (AC-7, AC-8, AC-12, AC-19).
- VoiceOver: teks setiap baris dan tombol menurut *Aksesibilitas*; subtitle alasan di menu dibacakan; pengumuman Copy, umpan balik, dan tahap progress terdengar sekali (AC-4, AC-8, AC-9, AC-13, AC-15, AC-16, AC-17, AC-19).
- Umpan balik di app: daemon dihentikan lalu aksi mutasi ditekan memunculkan kartu `caution` "stacklyd is unavailable. Nothing was sent." yang tetap sampai Dismiss; kartu `positive` hilang setelah 4 detik, tetap selama pointer di atasnya, selama fokus di tombolnya, dan selama app tidak aktif; kontrol yang difokus di bawah halaman tidak tertutup kartu (AC-9, AC-11).
- Increase Contrast, Reduce Transparency, Reduce Motion, Differentiate Without Color, light, dark: aturan AC-19, termasuk latar opak sheet dan inspector.
- Lebar 640 pt dan jendela 900 pt dengan inspector terbuka: tidak ada label yang terpotong (AC-3, AC-18, AC-19).
- Halaman galeri Components memuat setiap baris tabel *State di galeri* (AC-20).

Snapshot dan XCUITest untuk cek ini adalah urusan fitur 68.

### Asumsi lintas spec

- **Disepakati** spec 0069 (arc-f69-spec, 2026-10-21): `StacklyAction`, `ActionRole`, `ActionAvailability`, `availability(gate:)`, `invoke(gate:present:)`, `ActionHost` (dengan `gate` sendiri), dan `.actionHost()` milik spec ini, dengan signature di *Signature Swift*; `CommandID`, `CommandMap` (shortcut hanya di sana), `.publishesCommands(_:)` yang membaca `actionHost` dan `mutationAvailability` dari environment dan menerbitkan `PublishedCommands { actions, host, gate }`, perintah View > Show/Hide Inspector, dan tooltip dengan shortcut milik spec 0069. Hasil cek subtitle menu utama di langkah 1 dipakai spec 0069.
- **Disepakati** spec 0068 (arc-f68-spec, 2026-10-21): data contoh komponen di `ComponentSamples`, halaman galeri "Components" sebagai daftar state snapshot, konvensi `AccessibilityID`, scope komponen checklist bernama `checklist`, dan F67 membuat `AccessibilityID.swift` kalau membangun lebih dulu.
- **Dipakai** spec 0013: `MutationOutcome` dan `MutationFeedback` sesuai kode di `Connection/MutationPath.swift`; `LiveShell` sebagai akar jendela sampai `MainWindowRoot` spec 0016 menggantikannya (K18).
- **Dipakai** spec 0007: `OperationSummary.stage: String?`, `.progress` (`indeterminate` atau `counted { done, total }`), dan `OperationState` tertutup tujuh nilai.
- **Dipakai** spec 0017: `TileColor` sepuluh nilai plus `unrecognized` dan aturan mark default.

### Usulan update untuk spec lain

Ditulis di sini karena spec ini tidak boleh mengubah spec lain; lead menjadikannya item `architect (update)` atau `/sync`. Semuanya mengikuti keputusan D2 (fitur 67 satu satunya pembangun) dan D14 (pemasangan di shell). Baris yang mengubah teks AC spec lain ditandai **(teks AC)** dan butuh persetujuan user lewat lead.

- **Spec 0012** (`docs/specs/0012-design-system-ui-foundation/index.md`): Diterapkan di spec 0012 (2026-10-21).
  - Baris batas :61: ganti catatan fase 02 dengan "Fitur 67 membangun versi pertama setiap komponen ini (spec 0067); spec fase 02 tidak membuat versi sementara." Diterapkan di spec 0012 (2026-10-21).
  - :109 (`surfaceWindow` untuk halaman kartu, halaman dengan `Table`, `List`, atau `Form` native memakai latar sistem): tambahkan pengecualian "sheet lewat `StacklySheet` dan isi inspector lewat `.stacklyInspector` (spec 0067) memakai `surfaceWindow` opak, supaya teks di atasnya tetap di pasangan P1 dan P2". Diterapkan di spec 0012 (2026-10-21).
  - :226: overlay toast mockup menjadi kartu umpan balik spec 0067 lewat `.safeAreaInset(edge: .bottom)`, bukan overlay. Diterapkan di spec 0012 (2026-10-21).
  - :322: tetap; tambahkan penunjuk "kartu umpan balik spec 0067 (`Feedback.outcome`)". Diterapkan di spec 0012 (2026-10-21).
  - *Key invariants* :433 sampai :440: tidak ada token atau pasangan kontras baru. `.consoleSurface()` spec 0067 hanya menyusun `surfaceConsole`, `Radius.medium`, dan hairline yang sudah ada. Catat dua pengecualian warna dekoratif spec 0067 (putih mark tile, bayangan kartu) dan aturan bahwa simbol yang membawa arti memakai fill tone atau `textSecondary`, bukan `StacklyColor.decorative` (WCAG 1.4.11). Diterapkan di spec 0012 (2026-10-21).
  - Follow-up :532 terjawab oleh spec 0067 (centang lewat `/sync`). Baris itu sudah tercentang di spec 0012 sebelum update ini. Diterapkan di spec 0012 (2026-10-21).
- **Spec 0011** (`docs/specs/0011-secret-store-redaction/index.md`): tidak ada perubahan teks. Kode saja: `RevealModel.sentence` pindah ke `DaemonText.sentence`, dan `RevealModel` serta `SecretEntryModel` (`Secrets/SecretEntryModel.swift:137`) memanggilnya. Ditunda: tidak ada perubahan teks, dan spec 0011 di luar lingkup update ini.
- **Spec 0013** (`docs/specs/0013-app-shell-swift-client/index.md`). Gabungkan dengan update fase 02 yang masih terbuka di `docs/scope/coordination/02-instalasi-pertama.usulan-fondasi.md` bagian *Spec 0013* (butir 1 dan 8 di sana menyentuh baris batas yang sama dan `MainWindowRoot`): Ditunda: spec 0013 diperbarui terpisah setelah verify penuh F13, digabung dengan update fase 02 yang masih terbuka.
  - Baris batas :72 "Umpan balik setelah aksi (merender `MutationOutcome`) | 67": tambahkan "`FeedbackCenter` dibuat view akar jendela (`LiveShell`, lalu `MainWindowRoot`), `.feedbackPresenter()` dan `.actionHost()` dipasang di `AppShell` oleh build spec 0067". Ditunda: spec 0013 diperbarui terpisah setelah verify penuh F13, digabung dengan update fase 02 yang masih terbuka.
  - Peta package :87: tambahkan folder `Components/` dan `Accessibility/` (spec 0067) di baris `StacklyUI/Sources/StacklyUI/`. Ditunda: spec 0013 diperbarui terpisah setelah verify penuh F13, digabung dengan update fase 02 yang masih terbuka.
  - *Shell jendela*: kolom detail `AppShell` mendapat `.feedbackPresenter()` (`.safeAreaInset(edge: .bottom)`), akar `AppShell` mendapat `.actionHost()` di dalam `.environment(\.mutationAvailability, …)`, dan `LiveShell` memegang `@State FeedbackCenter` di environment `feedbackCenter`. Ditunda: spec 0013 diperbarui terpisah setelah verify penuh F13, digabung dengan update fase 02 yang masih terbuka.
  - *Diagnostics koneksi*: tombol copy di `ConnectionDiagnosticsSection` menulis lewat `PlainPasteboard` spec 0067. Ditunda: spec 0013 diperbarui terpisah setelah verify penuh F13, digabung dengan update fase 02 yang masih terbuka.
  - :399: tambahkan "pemetaan lengkap di `Feedback.outcome` spec 0067". Ditunda: spec 0013 diperbarui terpisah setelah verify penuh F13, digabung dengan update fase 02 yang masih terbuka.
  - Follow-up :652 bagian fitur 67 terjawab. Ditunda: spec 0013 diperbarui terpisah setelah verify penuh F13, digabung dengan update fase 02 yang masih terbuka.
- **Spec 0016** (`docs/specs/0016-onboarding/index.md`): Diterapkan di spec 0016 (2026-10-21).
  - **(teks AC)** AC-11 :32: "Setelah Get Started, `FeedbackCenter.show(.success("Setup complete."))` menampilkan kartu umpan balik di shell dan mengumumkannya sekali (pengumuman datang dari `FeedbackCenter`, bukan panggilan `announce` terpisah)." Alasan: dengan komponen fitur 67, panggilan `announce` kedua membuat pengumuman ganda. Disetujui user lewat lead (2026-10-21); kalimat kedua AC-11 (Escape) dan trace PAGE-01 tetap. Diterapkan di spec 0016 (2026-10-21).
  - Baris batas :55: "Komponen umpan balik dan komponen dasar lain | fitur 67 (spec 0067): `FeedbackCenter`, `CopyButton` (copy PATH, pengumuman "Copied."), `ConsoleBlock` (blok contoh), `CardSection`". Diterapkan di spec 0016 (2026-10-21).
  - AC-8 :29 dan blok contoh: baris PATH memakai `CopyButton`, blok contoh memakai `ConsoleBlock(text:accessibilityLabel:)`. Ditulis di *Kontrol Command Line*; teks AC-8 tidak berubah karena sudah cocok. Diterapkan di spec 0016 (2026-10-21).
  - Build plan :354: ganti "Komponen fitur 67 dipakai kalau sudah ada, tidak ditunggu" dengan "Milestone 1 needs feature 67 milestone 1 (`ui-macos.md`), atau membangun komponen yang dipakainya menurut spec 0067 (aturan *Siapa membangun versi pertama*)". Diterapkan di spec 0016 (2026-10-21).
  - Follow-up :416 terjawab. Diterapkan di spec 0016 (2026-10-21).
- **Spec 0017** (`docs/specs/0017-catalog-provider-detail/index.md`): Diterapkan di spec 0017 (2026-10-21).
  - :398: hapus `ProviderTile` dari daftar folder `Catalog/` dan kalimat "Kalau fitur 67 sudah menyediakan...". Ganti dengan "Tile, baris key value, dan banner memakai `ProviderTile`, `.labeledContentStyle(.stackly)` dan `CardSection` spec 0067, serta `StateBanner` spec 0012". Diterapkan di spec 0017 (2026-10-21).
  - :405: kartu memakai `ProviderTile` `mediumSize` (40 pt, mockup); :406: header panel `ProviderTile` `largeSize` (52 pt) dan baris License sampai Trust sebagai `LabeledContent` gaya `.stackly`. Diterapkan di spec 0017 (2026-10-21).
  - Milestone 3 :599: ganti "memakai komponen fitur 67 kalau sudah ada" dengan "needs feature 67 milestone 1 (`ui-macos.md`) atau membangun `ProviderTile` dan key value menurut spec 0067". Diterapkan di spec 0017 (2026-10-21).
- **Spec 0018** (`docs/specs/0018-install-runtime-managed/index.md`): Diterapkan di spec 0018 (2026-10-21).
  - Peta package :84: `InstallingRow` tetap view layar yang memakai `ProviderTile` `rowSize` dan `StagedProgress(.compact)`. Diterapkan di spec 0018 (2026-10-21).
  - :531: tanda provider memakai `ProviderTile` (`TileSpec` dari `ProviderSummary`, `name` dari `displayName`); label tahap, persen, dan bar dari `StagedProgressValue.from(operation:stages:)` dengan lima `StageDefinition` dari tabel *Alur install*. Rumus persen keseluruhan sama dengan :531; yang baru dari spec 0067 adalah aturan tampil persen (bilangan bulat, dibulatkan ke bawah, paling banyak 99 sebelum `succeeded`). Menu konteks Cancel Install memakai `ActionMenuItems` dengan `disabledReason` "Can’t be cancelled during Commit". Diterapkan di spec 0018 (2026-10-21).
  - :533: inspector memakai `.stacklyInspector` dan `InspectorHeader`. Diterapkan di spec 0018 (2026-10-21).
  - :542: hapus `ActionFeedbackBanner` dan `StateBanner` sementara serta penanda `// provisional: F67`. Sukses memulai memakai `FeedbackCenter.show(.started("Installing {name} {version}. Staged, verified, then committed."))`, selesai memakai `.success("{name} {version} installed and verified.")` (pengumuman datang dari `FeedbackCenter`), gagal tetap `StateBanner` critical di panel detail; `notSent` dan `outcomeUnknown` lewat `Feedback.outcome`. Diterapkan di spec 0018 (2026-10-21).
  - :556: pengumuman perubahan tahap memakai `StagedProgress(announcesStages: true)`. Diterapkan di spec 0018 (2026-10-21).
  - Milestone 4 :681 (judul tanpa klausa prasyarat): tambahkan klausa "needs feature 67 milestones 1, 2, and 3 (`ui-macos.md`), atau membangun komponen itu menurut spec 0067". Diterapkan di spec 0018 (2026-10-21).
- **Spec 0020** (`docs/specs/0020-create-instance/index.md`): Diterapkan di spec 0020 (2026-10-21).
  - Baris batas :107: "Komponen dasar (checklist, sheet dengan footer, progress bertahap, tile, umpan balik) | fitur 67 (spec 0067); `PreflightSection` dan `CreateProgressSection` adalah pembungkus layar di `CreateInstance/` di atas `CheckList` dan `StagedProgress`". Diterapkan di spec 0020 (2026-10-21).
  - :457: `NewInstanceSheet` memakai `StacklySheet` (lebar 560; `Form` di dalamnya memberi `.scrollContentBackground(.hidden)`); `PreflightSection` memetakan `PreflightCheck` ke `CheckItem` di dalam `CardSection("Preflight")`; `CreateProgressSection` memakai `StagedProgress(.stages)` dengan enam `StageDefinition` create. Diterapkan di spec 0020 (2026-10-21).
  - :460: header memakai `tile` `StacklySheet`. Diterapkan di spec 0020 (2026-10-21).
  - Perintah setara :468: `ConsoleBlock(command:)` dengan pengumuman "Command copied.". Diterapkan di spec 0020 (2026-10-21).
  - Footer :469: aksi utama Create adalah `StacklyAction` dengan `disabledReason` dari aturan Create aktif (copy alasan ditulis spec 0020); Cancel di mode progress (:470) adalah `StacklyAction` dengan `ActionRole.standard(confirmation:)` "Stop creating this instance?". Diterapkan di spec 0020 (2026-10-21).
  - :499 (status preflight sebagai status set baru): diganti tabel *Status checklist* spec 0067, jadi langkah 14 tidak menambah tabel itu ke design.md. Diterapkan di spec 0020 (2026-10-21).
  - Selesai :471: hasil create setelah sheet tertutup dikirim ke `FeedbackCenter` oleh layar pemilik sheet. Diterapkan di spec 0020 (2026-10-21).
  - Milestone 3 :1050: ganti "komponen fitur 67 ... kalau sudah ada" dengan "needs feature 67 milestones 1 and 2 (`ui-macos.md`), atau membangun komponen itu menurut spec 0067". Diterapkan di spec 0020 (2026-10-21).
  - Follow-up :1106 terjawab. Diterapkan di spec 0020 (2026-10-21).
- **Spec 0021** (`docs/specs/0021-start-readiness-connection/index.md`): Diterapkan di spec 0021 (2026-10-21).
  - Baris batas :90 tetap. Diterapkan di spec 0021 (2026-10-21).
  - *Tab Summary* :228 sampai :237: kartu memakai `CardSection` dan `.labeledContentStyle(.stackly)`; Copy endpoint memakai `CopyButton(style: .icon, announcement: "Address copied.")` di luar `LabeledContent`; Copy Instance ID `CopyButton(announcement: "ID copied.")`; Copy connection tanpa secret `CopyButton(announcement: "{title} copied.")`. Diterapkan di spec 0021 (2026-10-21).
  - Toolbar :267: menu More memakai `ActionMenu` dengan `StacklyAction` "Copy Connection String" (`mutates` false, `disabledReason` dari copy spec 0021); tombol aksi utama di toolbar memakai `ActionButton(showsReason: false)`, dan alasannya tampil di halaman. Diterapkan di spec 0021 (2026-10-21).
  - Milestone 3 :722: ganti "memakai komponen fitur 67 ... kalau sudah ada" dengan "needs feature 67 milestones 1 and 3 (`ui-macos.md`)". Diterapkan di spec 0021 (2026-10-21).
- **Spec 0022** (`docs/specs/0022-overview-daftar-instance/index.md`): Diterapkan di spec 0022 (2026-10-21).
  - Baris batas :68: `ProviderTile` milik spec 0067 dengan parameter tambahan `name` (atau `TileSpec`). Diterapkan di spec 0022 (2026-10-21).
  - Baris batas :76: "Umpan balik setelah aksi | fitur 67 (`FeedbackCenter`, `Feedback.outcome`)". Diterapkan di spec 0022 (2026-10-21).
  - :203: hapus `ActionFeedbackBanner` dari folder `Instances/`. Diterapkan di spec 0022 (2026-10-21).
  - :227 sampai :229: inspector memakai `.stacklyInspector` (lebar 240, 280, 360 tetap), header memakai `InspectorHeader`, isi memakai `CardSection` dan `.labeledContentStyle(.stackly)`. Diterapkan di spec 0022 (2026-10-21).
  - Aksi instance: tombol Start, Stop, dan Retry adalah `StacklyAction` lewat `ActionButton`; menu konteks memakai `ActionMenuItems`. Diterapkan di spec 0022 (2026-10-21).
  - :274: ganti dengan "Umpan balik lewat `FeedbackCenter`: `Feedback.outcome` untuk `rejected`, `notSent`, `outcomeUnknown`, `Feedback.failure` untuk operasi `failed`, dan `Feedback.neutral` dengan copy spec ini untuk `cancelled`". Diterapkan di spec 0022 (2026-10-21).
  - Langkah 7 :501: hapus `ActionFeedbackBanner` (`// provisional: F67`). Diterapkan di spec 0022 (2026-10-21).
  - :529: hapus "umpan balik aksi memakai banner sementara". Diterapkan di spec 0022 (2026-10-21).
  - Usulan :545: signature final `ProviderTile(mark:tile:name:size:)` dan `ProviderTile(_:size:)` dengan `TileSpec`. Diterapkan di spec 0022 (2026-10-21).
  - Milestone 3 :498: "spec 0017 milestone 3 (`CatalogStore`, `ProviderTile`)" menjadi "spec 0017 milestone 3 (`CatalogStore`)", lalu tambahkan "needs feature 67 milestones 1 and 3 (`ui-macos.md`), atau membangun komponen itu menurut spec 0067". Diterapkan di spec 0022 (2026-10-21).
  - Follow-up :560 bagian fitur 67 terjawab. Diterapkan di spec 0022 (2026-10-21).
- **Spec 0068 dan 0069**: tidak ada perubahan teks dari spec ini; kesepakatan di *Asumsi lintas spec* ditulis di spec masing masing oleh architect nya di wave ini. Ditunda: tidak ada perubahan teks yang diusulkan, dan spec 0068 serta 0069 di luar daftar target update ini.

## Build plan

Pendekatan proyek adalah Journey, tapi fitur 67 adalah fondasi tanpa journey sendiri. Urutannya mengikuti kebutuhan layar fase 02 yang paling awal: milestone 1 memegang semua yang dipakai Catalog, Onboarding, Overview, dan Instances, plus deskriptor aksi yang ditunggu fitur 69; milestone 2 memegang yang dipakai sheet create dan halaman Runtimes; milestone 3 inspector; milestone 4 log dan diff untuk fase 04. Setiap komponen dibangun lengkap dengan semua state, aksesibilitas, preview, entri galeri, dan bagian design.md nya. Milestone 2, 3, dan 4 bisa berjalan sebagai sesi paralel setelah milestone 1, karena masing masing hanya menyentuh foldernya di `Components/`, file `ComponentSamples+<bagian>.swift`, file bagian galerinya, file test nya, dan satu subbagian design.md yang judulnya sudah dibuat langkah 2.

**Milestone 1: Komponen dasar, aksi, dan umpan balik** (needs feature 12 build and feature 13 milestone 4 code (`ui-macos.md`); dibutuhkan feature 16 milestone 1, feature 17 milestone 3, dan feature 22 milestone 3 di `02-instalasi-pertama.md`, dan milestone fitur 69 yang memakai `StacklyAction` di `ui-macos.md`)

1. Cek subtitle item menu di macOS 27 sebelum menulis `ActionMenu`: `Menu` di toolbar, `.contextMenu`, dan `CommandMenu` di menu utama, masing masing dengan item nonaktif yang labelnya dua `Text`, dicek dengan mata dan VoiceOver. Catat hasilnya di design.md (bagian Components, Menus) dan di rationale.md (*Temuan langkah 1*), lalu pilih jalur subtitle atau cadangan item nonaktif tambahan. Memenuhi **AC-8**.
2. Kerangka: folder `Components/` dan `Accessibility/AccessibilityID.swift` (scope `feedback`, `sheet`, `inspector`, `action`, `checklist`, `progress`, `log`, `diff`, `copy`); `ComponentSamples+Base.swift`; `GalleryTopic.components` dengan `ComponentsGalleryPage` dan file bagian kosong per milestone (`// provisional: F67 M2`, `F67 M3`, `F67 M4`); bagian "Components" di design.md dengan judul subbagian dan satu baris penanda "Filled by milestone N." per subbagian milestone lain; `.stacklyTileMark(tileSize:)` di `Typography.swift`; `Components/Copy/PlainPasteboard.swift` dan pemindahan copy `ConnectionDiagnosticsSection` ke sana; dua aturan sumber baru (pasteboard dan inspector). Memenuhi **AC-1**, **AC-20**, **AC-21**.
3. `TileSpec`, `ProviderTile`, dan `ProviderMark` dengan test mark default dan warna tile, preview, dan bagian galeri. Memenuhi **AC-2**, **AC-19**, **AC-20**.
4. `CardSection` dan `.labeledContentStyle(.stackly)` dengan preview baris panjang di 640 pt dan cek macOS 27 bahwa kontrol `Form` tidak berubah saat gaya dipasang pada baris read only (hasil di design.md). Memenuhi **AC-3**, **AC-19**.
5. `CopyConfirmation`, `CopyButton`, `ConsoleCopy`, `.consoleSurface()`, dan `ConsoleBlock` di atas `PlainPasteboard`, dengan test writer, announce, dan sleep tiruan. Memenuhi **AC-4**, **AC-5**, **AC-19**.
6. `ActionConfirmation`, `ActionRole`, `ActionAvailability`, `StacklyAction` dengan `availability(gate:)` dan `invoke(gate:present:)`, `ActionHost`, `.actionHost()`, `ActionButton`, `ActionMenu`, dan `ActionMenuItems` di jalur hasil langkah 1, dengan test availability, invoke, dan host. Memenuhi **AC-6**, **AC-7**, **AC-8**, **AC-19**.
7. `Feedback` dengan fungsi statisnya, `DaemonText.sentence` (dan `RevealModel` serta `SecretEntryModel` memakainya), `FeedbackCenter` dengan announce dan sleep tiruan, `FeedbackHold`, `FeedbackCard`, `.feedbackPresenter()`, lalu pemasangan: `LiveShell` membuat `FeedbackCenter`, `AppShell` memasang presenter di kolom detail dan `.actionHost()` di akar di dalam `.environment(\.mutationAvailability, …)`, dengan test pemindai sumber struktur shell. Memenuhi **AC-9**, **AC-10**, **AC-11**, **AC-19**.
8. Bagian galeri dan subbagian design.md untuk semua komponen milestone ini, termasuk aturan pemakaian umpan balik dan copy. Memenuhi **AC-1**, **AC-20**.

**Milestone 2: Sheet, checklist, dan progress bertahap** (after milestone 1; dibutuhkan feature 20 milestone 3 dan feature 18 milestone 4 di `02-instalasi-pertama.md`; bisa paralel dengan milestone 3 dan 4)

9. Cek sheet di macOS 27 dengan tombol sungguhan sebelum menulis `StacklySheet`, seperti langkah 17 spec 0012: Return dan Escape dengan fokus di `TextField` dan di `Toggle`, aksi utama destruktif, aksi utama nonaktif, Tab ke Cancel dan aksi utama dengan Keyboard navigation menyala, konfirmasi `.stacklyConfirmation` dari dalam sheet (tampil di atas sheet), dan `.presentationBackground` opak. Catat di design.md dan rationale.md (*Temuan langkah 9*), lalu pilih footer eksplisit atau placement toolbar. Memenuhi **AC-12**.
10. `StacklySheet` dengan `.actionHost()` sendiri, alasan footer, tinggi isi, preview, bagian galeri, dan test pemindai sumber untuk `.actionHost()`. Memenuhi **AC-12**, **AC-19**.
11. `CheckStatus`, `CheckItem`, `CheckListLayout`, dan `CheckList` dengan test teks VoiceOver dan tabel *Status checklist* di design.md. Memenuhi **AC-13**, **AC-19**.
12. `StageDefinition`, `StagedProgressValue.from(operation:stages:)` dengan test per baris kedua tabel, `StageFlowLayout`, dan `StagedProgress` dengan kedua tata letak. Memenuhi **AC-14**, **AC-15**, **AC-19**.
13. Bagian galeri dan subbagian design.md milestone ini. Memenuhi **AC-1**, **AC-20**.

**Milestone 3: Inspector** (after milestone 1; dibutuhkan feature 22 milestone 3, feature 18 milestone 4, dan feature 21 milestone 3 di `02-instalasi-pertama.md`, dan perintah inspector fitur 69 di `ui-macos.md`; bisa paralel dengan milestone 2 dan 4)

14. Cek inspector di macOS 27 lebih dulu (sambungan latar opak `surfaceWindow` di kolom inspector, dan perilaku kolom di jendela 900 pt dengan sidebar dan inspector terbuka), catat di design.md dan rationale.md (*Temuan langkah 14*), lalu `.stacklyInspector`, `InspectorToggle`, `InspectorHeader`, dan `InspectorPlaceholder`, dengan preview, bagian galeri, dan subbagian design.md (termasuk aturan `@SceneStorage`). Memenuhi **AC-18**, **AC-19**, **AC-20**.

*Tambahan langkah 14 (2026-10-21, keputusan user atas Temuan langkah 14; langkah 14 yang sudah dibangun tidak diubah):* `StacklyInspectorWidth` dengan test fungsi murni (lebar jendela 999 dan 1000 pt, dan nil); `EnvironmentValues.stacklyWindowWidth` yang diisi view akar jendela lewat `onGeometryChange`, dengan test pemindai sumber bahwa view akar memasangnya; `.stacklyInspector` memakai `maxWidth(windowWidth:)`; cek di macOS 27 bahwa mengecilkan jendela dari 1000 pt atau lebih ke 900 pt dengan inspector 360 pt menjepit kolom inspector ke 280 pt dan kolom halaman tetap 384 pt (hasil di design.md dan rationale.md, *Temuan langkah 14*); dan aturan kolom halaman 384 pt di subbagian design.md Inspector. Memenuhi **AC-18**, **AC-19**.

**Milestone 4: View log dan diff** (after milestone 1; dibutuhkan feature 27 dan feature 29 di `04-kerja-harian.md`; bisa paralel dengan milestone 2 dan 3)

15. `LogLevel`, `LogLine`, dan `LogView` dengan follow, kolom sejajar, dan test teks VoiceOver. Memenuhi **AC-16**, **AC-19**.
16. `DiffKind`, `DiffLine`, dan `DiffView` dengan test teks VoiceOver. Memenuhi **AC-17**, **AC-19**.
17. Bagian galeri dan subbagian design.md milestone ini. Memenuhi **AC-1**, **AC-20**.

## Consequences

**Positive**:

- Layar fase 02 dan seterusnya dirakit dari bagian yang sama, jadi tile, baris, sheet, umpan balik, dan menu terlihat dan berperilaku sama di semua halaman, dan tidak ada kode sementara yang harus diadopsi atau dihapus belakangan.
- Satu deskriptor aksi membuat tombol, item menu, menu konteks, dan perintah menu utama nonaktif dan dikonfirmasi dengan aturan yang sama, termasuk alasan capability unsupported (REQ-026), dan tipe `ActionRole` membuat aksi destruktif tanpa konfirmasi tidak bisa ditulis.
- Umpan balik memenuhi kebutuhan pengguna VoiceOver (diumumkan), pengguna yang lambat membaca (ditahan saat hover, fokus, atau app tidak aktif; error dan pesan beraksi tidak hilang sendiri), dan pengguna keyboard (kontrol yang difokus tidak tertutup).
- Tidak ada pasangan kontras baru: semua komponen memakai permukaan dan teks yang sudah dibuktikan test spec 0012.
- Fungsi murni (pemetaan progress, teks progress, pemetaan hasil mutasi, availability, mark default, hitungan umpan balik) dites tanpa view, dan fitur 68 merender satu halaman galeri untuk snapshot.
- Milestone 2 sampai 4 bisa dibangun paralel.

**Negative / tradeoffs**:

- Milestone 1 menjadi prasyarat layar fase 02 pertama (Catalog, Onboarding, Overview, Instances). Kalau fase 02 berjalan lebih cepat, milestone fase 02 harus membangun komponen menurut spec ini, yang menambah pekerjaan di milestone itu.
- Selama kartu umpan balik tampil, area konten di bawahnya menyempit (`.safeAreaInset`), dan umpan balik yang lebih baru menggantikan yang lama, jadi kegagalan harus juga tercatat di tempatnya (aturan design.md).
- Latar konsol di view diff mockup diganti kartu biasa; di dark mode diff sedikit lebih terang daripada mockup. Mockup perlu dicatat.
- Inspector dan sheet memakai latar opak `surfaceWindow`, bukan glass, demi bukti kontras. Inspector mockup perlu dicatat.
- Alasan nonaktif di menu bergantung pada subtitle item menu macOS 27; kalau cek langkah 1 gagal, cadangan item tambahan membuat menu lebih panjang.
- Footer sheet memegang shortcut Return dan Escape sendiri yang harus dicek ulang di versi macOS baru, seperti konfirmasi spec 0012.
- Pemilihan teks di `LogView` hanya per baris; copy banyak baris butuh aksi terpisah di fitur 27. Pilihan teks di baris key value bisa hilang saat tata letak baris berganti.
- Durasi 4 detik dan rumus panjang teks adalah perkiraan kecepatan baca, bukan hasil uji pengguna.
- Komponen yang belum dipakai layar mana pun (log, diff) dibangun sebelum fitur pemiliknya, dengan risiko API nya berubah saat fitur 27 dan 29 ditulis.
- Signature yang dikunci di spec ini membuat perubahan nama publik harus lewat `/architect`.

**Neutral**:

- `StacklyUI` mendapat folder `Components/` dan `Accessibility/`, satu halaman galeri, dan dua aturan sumber.
- `RevealModel.sentence` pindah ke `DaemonText.sentence`; `ConnectionDiagnosticsSection` pindah ke `PlainPasteboard`.
- design.md mendapat bagian Components dan dua tabel status baru (checklist dan tahap), plus tabel teks progress.
- Spec 0012, 0013, 0016, 0017, 0018, 0020, 0021, dan 0022 perlu update kecil (*Usulan update untuk spec lain*), satu di antaranya teks AC spec 0016.

## Follow-up

- [ ] Lead: jadikan *Usulan update untuk spec lain* item `architect (update)` untuk spec 0012, 0013 (digabung dengan update fase 02 yang terbuka), 0016, 0017, 0018, 0020, 0021, dan 0022; teks AC-11 spec 0016 butuh persetujuan user.
- [ ] `/scope`: catat di `docs/scope/ui-macos.md` bagian *Selisih yang sudah diketahui* bahwa toast mockup menjadi kartu umpan balik opak di bawah area konten (error dan pesan beraksi tetap sampai ditutup), view diff memakai kartu biasa (bukan latar konsol), inspector memakai latar opak, dan detail checklist membungkus (bukan elipsis).
- [ ] `/sync` setelah build: folder `Components/` dan `Accessibility/` di tabel Key files `apps/macos/AGENTS.md`, spec 0067 di Related specs, dan aturan "aksi lewat `StacklyAction`, umpan balik lewat `FeedbackCenter`, copy lewat `CopyButton`" di Conventions.
- [ ] `/sync`: centang Follow-up spec 0012 :532, 0013 :652 (fitur 67), 0016 :416, 0020 :1106, dan 0022 :560 (fitur 67) yang dijawab spec ini.
- [ ] Fitur 27 dan 29: tulis spec nya di atas `LogView` dan `DiffView`; ubah API komponen itu lewat `/architect` spec ini kalau kebutuhannya berbeda.
- [ ] Fitur 32: popover menu bar memasang `.actionHost()` sendiri dan memutuskan presentasi umpan baliknya (kartu di popover atau tanpa kartu).
- [ ] Fitur 34 dan 15: hasil doctor memakai `CheckList(.single)`; tombol Copy Details di seksi Diagnostics agent (spec 0015) memakai `CopyButton`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, dan catatan cross check ada di [rationale.md](rationale.md).
