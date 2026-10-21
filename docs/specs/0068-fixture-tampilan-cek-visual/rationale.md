# Rationale: 0068. Fixture tampilan dan cek visual

## Context

Aplikasi macOS Stackly adalah client SwiftUI untuk `stacklyd`. Fondasinya sudah ada: token, status, pola state, dan konfirmasi (spec 0012), lalu shell, transport, dan store (spec 0013). Fase 02 akan menambah banyak layar sekaligus (onboarding, catalog, create, detail, overview, instances), dan setiap layar punya tabel state yang panjang: loading, empty, failed, stale, partial, unsupported, plus setiap status instance dan operasi. Hari ini satu satunya cara melihat state itu adalah `#Preview` dengan data tulisan tangan dan galeri Debug, sedangkan bukti GUI di spec 0012, 0013, dan 0014 masih manual. Verify fitur 12 (2026-10-18) bahkan harus memakai skrip AX buatan sendiri di luar repo, dan empat cek setting sistem berakhir `BLOCKED`.

Kekuatan yang bermain:

- **Kontrak sebagai sumber data.** Done when fitur 68 meminta fixture memakai tipe kontrak yang sama dengan Swift client, bukan model terpisah. DTO hasil typeshare sudah publik dan `Sendable`, dan spec 0013 membuat `StacklyUI` bergantung langsung pada `StacklyClient` justru supaya layar dan fixture memakai DTO itu.
- **Tidak ada CI.** Semua cek berjalan dengan tangan di Mac ini (`AGENTS.md`, Tooling). Mac ini adalah mesin kerja harian dengan layar utama 1x (1680 × 1050); hasil test tidak boleh bergantung pada layar, accent, atau wallpaper.
- **Batas platform macOS 27.** Text style tidak ikut `dynamicTypeSize`, jadi cek teks besar adalah lebar konten 640 pt (spec 0012 langkah 4). Appearance high contrast AppKit di jendela test tidak mengubah `colorSchemeContrast` (verify.md spec 0012 baris 35 sampai 38).
- **Dependency.** Proyek sengaja tanpa dependency Swift pihak ketiga: transport ditulis di atas socket POSIX supaya `StacklyClient` tetap nol dependency (spec 0013), dan spec 0001 membiarkan baris "Library snapshot test Swift" terbuka untuk fitur ini.
- **Evidence.** PRD bagian 28 meminta setiap AC punya evidence record, dan spec 0002 menunda helper XCTest sampai XCUITest pertama datang di fitur ini.
- **Aksesibilitas per layar.** Scope meminta cek aksesibilitas berjalan saat layar dibangun, bukan diaudit di akhir: label yang hilang, target klik terlalu kecil, dan kontras.

Tanpa keputusan ini, setiap layar fase 02 akan menulis preview dan cek sendiri dengan data sendiri, regresi tampilan baru terlihat saat `/check verify`, dan bukti aksesibilitas tetap bergantung pada reviewer.

## Options considered

### Keputusan K1: library snapshot

#### Option 1: helper buatan sendiri di `StacklyUITests/Support/` (dipilih)

**Pros**:

- Jalur render memang harus khusus (jendela dengan skala dikunci, setter environment, latar jendela), jadi library pun tidak menghemat bagian itu.
- Sisa pekerjaan (banding piksel, rekam, artefak gagal) sekitar 200 baris dan sepenuhnya di bawah kendali proyek.
- Tanpa dependency pihak ketiga, tanpa lisensi baru, resolve paket Xcode tetap tanpa jaringan.

**Cons**:

- Proyek merawat kode banding dan rekam sendiri, termasuk bug nya.
- Tidak ada fitur matang seperti inline snapshot atau strategi selain gambar.

#### Option 2: pointfreeco/swift-snapshot-testing 1.18.x (MIT) di test target saja

**Pros**:

- Library paling dikenal untuk snapshot Swift, dengan mode rekam, diff, dan dukungan Swift Testing.
- Strategi selain gambar (teks, dump) tersedia gratis.

**Cons**:

- Menarik pin swift-syntax, swift-custom-dump, dan xctest-dynamic-overlay ke `Package.resolved` StacklyUI dan ke resolve paket Xcode, jadi build pertama butuh jaringan.
- Strategi `NSView` nya mengikuti skala layar, jadi kode render tetap harus ditulis sendiri.
- Lisensi dan supply chain Swift tidak dijaga `cargo deny`; harus direview tangan setiap naik versi.

#### Option 3: tanpa snapshot piksel

**Pros**:

- Tidak ada referensi PNG di repo dan tidak ada rekam ulang.

**Cons**:

- Regresi warna, border HC, dan teks terpotong hanya tertangkap mata manusia; Done when fitur 68 tidak terpenuhi.

### Keputusan K2: Increase Contrast dan Reduce Transparency di snapshot

#### Option 1: setter environment bergaris bawah plus appearance jendela light atau dark (dipilih)

**Pros**:

- Probe membuktikan `_colorSchemeContrast` dan `_accessibilityReduceTransparency` mencapai view: token kita, `Color.primary`, warna tint sistem, bezel tombol, dan material ikut berubah.
- Satu tempat (`FixtureAppearance.swift`) dipakai preview dan snapshot.

**Cons**:

- API bergaris bawah bukan API yang didokumentasikan; SDK berikutnya bisa menghapusnya.
- Warna AppKit yang dijembatani dan teks baris `List` atau `Table` tidak ikut HC.

#### Option 2: tanpa varian HC dan RT di snapshot

**Pros**:

- Tanpa API bergaris bawah.

**Cons**:

- Border HC dan permukaan yang menjadi tembus pandang hanya tercek manual, padahal itu janji AC-7 spec 0012.

### Keputusan K3: skala gambar

#### Option 1: 1x yang dikunci (dipilih)

**Pros**:

- Empat kali lebih sedikit piksel, jadi repo tumbuh lebih lambat; layar utama Mac ini juga 1x.
- Dikunci lewat `backingScaleFactor`, jadi hasil tidak bergantung pada layar yang terpasang.

**Cons**:

- Kurang tajam saat dilihat di layar retina; detail di bawah satu titik tidak terlihat.

#### Option 2: 2x yang dikunci

**Pros**:

- Lebih tajam saat review dan menangkap detail setengah titik.

**Cons**:

- Ukuran file sekitar empat kali lipat untuk setiap state dan varian.

### Keputusan K4: matriks varian per state

#### Option 1: lima file plus RT sebagai kesamaan dengan light (dipilih)

**Pros**:

- Memenuhi Done when (light, dark, teks besar, Increase Contrast, Reduce Transparency) dengan lima file, bukan enam.
- RT yang wajib identik dengan light membuktikan aturan spec 0012 AC-7 ("semua permukaan kustom opak sejak awal") secara langsung.

**Cons**:

- View yang sah memakai material sistem harus menyatakan `systemMaterial: true`.

#### Option 2: enam file penuh

**Pros**:

- Paling sederhana dipahami.

**Cons**:

- File RT hampir selalu identik dengan light, jadi repo menyimpan duplikat.

#### Option 3: light dan dark per state, varian aksesibilitas hanya untuk satu state per layar

**Pros**:

- Paling hemat ruang.

**Cons**:

- Border HC dan teks terpotong di state lain (banner, partial, failed) tidak tercek, padahal di situ teks paling panjang.

### Keputusan K5: pembanding

#### Option 1: persis (nol piksel dengan selisih kanal lebih dari 2/255) (dipilih)

**Pros**:

- Render terbukti identik byte antar proses, jadi pembanding persis tidak berisik di Mac ini.
- Label yang berubah dari "Running" menjadi "Runn…" (beberapa ratus piksel) tidak lolos.

**Cons**:

- Setiap update macOS atau Xcode yang mengubah render kontrol sistem menuntut rekam ulang.

#### Option 2: toleransi 0,1% piksel atau perceptual

**Pros**:

- Lebih tahan terhadap perubahan antialias kecil.

**Cons**:

- 0,1% dari kanvas halaman adalah sekitar 780 piksel, cukup untuk melewatkan teks yang terpotong.

### Keputusan K6: rekam dan review

#### Option 1: referensi di git, rekam saat hilang, mode rekam lewat env, folder gagal, dan `cargo xtask snapshot review` (dipilih)

**Pros**:

- Tanpa CI dan tanpa PR wajib, halaman review lokal membuat perubahan gambar terlihat sebelum lead merge.
- Run evidence tidak pernah merekam, jadi bukti tidak bisa lulus karena referensi baru saja ditulis.

**Cons**:

- Satu subcommand xtask lagi untuk dirawat.

#### Option 2: tanpa xtask, cukup `git diff` dan folder gagal

**Pros**:

- Lebih sedikit kode.

**Cons**:

- `git diff` hanya menulis "Binary files differ"; reviewer harus membuka file satu per satu.

### Keputusan K7: bentuk fixture

#### Option 1: skenario Swift di `StacklyUI/Fixtures/` (`#if DEBUG`) dari DTO hasil generate (dipilih)

**Pros**:

- Perubahan kontrak langsung gagal compile di fixture.
- Dipakai `#Preview`, snapshot, galeri, dan app Debug tanpa membaca file.

**Cons**:

- Rust tidak bisa membaca fixture ini; kebenaran wire nya dijaga round trip `StacklyJSON` di Swift saja.

#### Option 2: file JSON skenario di `protocols/fixtures/ui/`

**Pros**:

- Bisa ikut divalidasi test Rust seperti fixture kontrak lain.

**Cons**:

- Preview dan app Debug harus membaca file repo lewat `#filePath`, dan file JSON bisa tertinggal dari kontrak tanpa gagal compile.

#### Option 3: hanya fixture `ipc/` yang sudah ada

**Pros**:

- Tidak ada data baru.

**Cons**:

- Fixture itu satu kasus per file (dua instance di `instance-list`), tidak ada skenario dengan banyak state.

### Keputusan K8: kasus yang DTO nya belum ada

#### Option 1: bangun yang bisa dinyatakan kontrak hari ini, sisanya milik fitur pemilik DTO (dipilih)

**Pros**:

- Fitur 68 bisa selesai sebelum fase 02, dan setiap kasus baru lahir bersama DTO nya, jadi tidak ada data tebakan.

**Cons**:

- Kalimat Done when fitur 68 perlu diselaraskan lewat `/scope`, dan disiplin "kasus fixture ikut commit DTO" bergantung pada review.

#### Option 2: fitur 68 menunggu fitur pemilik

**Pros**:

- Done when tetap seperti sekarang.

**Cons**:

- Fitur 68 baru bisa selesai setelah fitur 44, padahal layar fase 02 membutuhkannya sekarang.

### Keputusan K9: XCUITest sekarang

#### Option 1: target XCTest `StacklyAppUITests`, dijalankan dengan tangan lewat `cargo xtask ui-test` (dipilih)

**Pros**:

- Jalur yang didukung Apple, termasuk `performAccessibilityAudit` untuk label, target klik, dan kontras.
- Menggantikan skrip AX di luar repo yang dipakai verify fitur 12.

**Cons**:

- Butuh Automation Mode (password admin sekali), mengambil alih keyboard dan mouse, dan menutup app Stackly yang sedang terbuka.
- Butuh helper evidence XCTest (K12).

#### Option 2: pemeriksa AX buatan sendiri di xtask

**Pros**:

- Bisa membaca atribut AX apa pun, termasuk key equivalent menu.

**Cons**:

- Butuh izin Accessibility untuk proses pemeriksa dan menulis ulang apa yang sudah diberi XCUITest, tanpa audit Apple.

#### Option 3: tanpa UI test otomatis

**Pros**:

- Tanpa prasyarat sistem.

**Cons**:

- Bukti GUI tetap manual, bertentangan dengan Done when.

### Keputusan K10: data app saat XCUITest

#### Option 1: skenario fixture lewat `LinkHandle` palsu, dipilih lewat `STACKLY_UI_SCENARIO` (dipilih)

**Pros**:

- Deterministik: crash loop, offline, dan incompatible tampil sama setiap kali.
- Tidak menyentuh socket, daemon, UserDefaults, atau state jendela pengguna.

**Cons**:

- Jalur transport dan sinkronisasi nyata tidak ikut teruji di UI test (sudah teruji di test end to end spec 0013).

#### Option 2: `stacklyd` asli dengan root dari `skeleton_seed`

**Pros**:

- Ujung ke ujung sungguhan.

**Cons**:

- State seperti crash loop dan offline bergantung waktu, jadi test lambat dan rawan berkedip.

### Keputusan K11: audit aksesibilitas

#### Option 1: keenam jenis audit macOS, light dan dark, dengan daftar pengecualian yang menyebut spec penerima (dipilih)

**Pros**:

- `.action` dan `.parentChild` menangkap elemen yang tidak bisa dijalankan atau pohon AX yang rusak, di luar tiga yang disebut Done when.
- Setiap pengecualian punya alasan dan spec yang menerimanya, jadi tidak ada penerimaan diam diam.

**Cons**:

- Audit Apple bisa melaporkan issue dari kontrol sistem yang tidak bisa kita ubah, dan setiap issue seperti itu butuh keputusan.

#### Option 2: hanya tiga jenis yang disebut Done when

**Pros**:

- Lebih sedikit issue yang harus diputuskan.

**Cons**:

- Elemen tanpa aksi dan hubungan parent child yang salah tidak tertangkap.

### Keputusan K12: helper evidence XCTest

#### Option 1: product `StacklyXCTestSupport` di `clients/swift`, berbagi penulis record lewat target internal `StacklyEvidenceCore` (dipilih)

**Pros**:

- Satu implementasi format record untuk Swift Testing dan XCTest; bundle UI test tidak perlu Swift Testing.
- `StacklyClient` tidak berubah.

**Cons**:

- Pemindahan file dari `StacklyTestSupport` ke target baru, dan peta package spec 0001 berubah.

#### Option 2: helper di dalam target UI test saja

**Pros**:

- Tidak menyentuh `clients/swift`.

**Cons**:

- Format record ditulis dua kali di Swift dan bisa menyimpang tanpa ketahuan.

### Keputusan K13: cek manual yang pindah ke XCUITest

#### Option 1: yang terbaca dari pohon AX dan keyboard pindah, sisanya tetap manual (dipilih)

**Pros**:

- Label VoiceOver, Return dan Escape, alasan nonaktif, banner shell, ukuran minimum, dan menu teruji setiap run.

**Cons**:

- Setting sistem asli, fokus Tab, dan suara VoiceOver tetap butuh orang.

#### Option 2: semua tetap manual sampai layar journey pertama

**Pros**:

- Lebih sedikit test sekarang.

**Cons**:

- Galeri dan shell yang sudah ada tidak mendapat bukti otomatis.

## Rationale

Keputusan inti ada di K1, K2, dan K9, dan ketiganya digerakkan oleh fakta yang diukur di Mac ini, bukan oleh selera. Probe menunjukkan bahwa satu satunya jalur render yang menggambar kontrol AppKit (List, Table, TextField) adalah `NSHostingView` plus `cacheDisplay`, dan bahwa jalur itu deterministik antar proses. Karena kode render, skala, dan environment harus ditulis sendiri untuk Mac ini apa pun library nya, library pihak ketiga hanya akan menambah pin, lisensi, dan resolve berjaringan untuk sekitar 200 baris kode banding. Itu bertentangan langsung dengan pola proyek yang memilih socket POSIX daripada dependency demi `StacklyClient` tanpa dependency.

Increase Contrast tidak bisa dipaksa lewat appearance AppKit (sudah terbukti di verify fitur 12 dan diulang di probe), tetapi setter environment bergaris bawah bekerja untuk semua bagian yang dikendalikan kode kita, yaitu tepat yang dijanjikan AC-7 spec 0012. Risiko API bergaris bawah diterima dengan tiga pagar: hanya di satu file Debug dan di test, test penjaga yang langsung merah bila setter berhenti bekerja, dan cek setting asli yang tetap manual. Reduce Transparency dibuktikan sebagai kesamaan dengan light, sehingga aturan "semua permukaan kustom opak" menjadi assertion, bukan janji.

XCUITest dipilih sekarang karena audit aksesibilitas Apple adalah jawaban langsung untuk "elemen tanpa label, target terlalu kecil, kontras" di Done when, dan karena galeri serta shell sudah ada untuk diuji. Prasyarat Automation Mode adalah setting sistem milik user, jadi spec ini hanya memeriksanya dan menandai `BLOCKED`, tidak pernah mengaktifkannya. Fixture Swift dari DTO (K7) dan skenario lewat `LinkHandle` palsu (K10) membuat hasil UI test dan snapshot deterministik dan memakai jalur model asli (`DaemonModel`, `ShellPresentation`), sementara jalur transport sungguhan tetap dibuktikan test end to end spec 0013.

## Bukti probe

Dijalankan 2026-10-21 di macOS 27.0.1 (26A434), Xcode 27.0 (27A266a), layar utama 1680 × 1050 di 1x. Kode probe ada di scratchpad sesi arc-f68-spec, bukan di repo; ringkasan juga ada di catatan bersama `arc-f68-probe-findings.md`.

- `ImageRenderer`: `List`, `Table`, `TextField`, dan `ProgressView` menjadi placeholder kuning bertanda dilarang.
- `NSHostingView` di `NSWindow` yang tidak pernah tampil, ditangkap `cacheDisplay(in:to:)`: Text, tombol bordered dan prominent, Toggle, SF Symbol, `List`, `Table`, `TextField`, dan spinner tergambar. `CALayer.render(in:)` memberi isi yang sama.
- Tujuh belas gambar dari dua proses terpisah identik byte, termasuk frame spinner.
- `cacheDisplay` mengikuti skala jendela; subclass `NSWindow` yang mengunci `backingScaleFactor` ke 2 menghasilkan 1280 × 824 piksel untuk 640 × 412 pt.
- Tidak tertangkap: baris sidebar `NavigationSplitView` (kosong, juga saat jendela dipesan ke layar di luar area tampil), item toolbar, dan `.glassEffect()`.
- Jendela tidak key: tombol prominent dan seleksi list tergambar abu abu (rupa tidak aktif).
- SDK `SwiftUICore.swiftinterface` macOS 27: setter publik `_colorSchemeContrast`, `_accessibilityReduceTransparency`, `_accessibilityDifferentiateWithoutColor`, `_accessibilityReduceMotion`, `_accessibilityInvertColors`, `_accessibilityShowButtonShapes`; nama tanpa garis bawah hanya getter.
- `_colorSchemeContrast = .increased`: view yang membaca `colorSchemeContrast`, `Color.primary`, tint sistem `.orange`, dan bezel tombol berubah ke varian HC; `Color(nsColor: .labelColor)`, `.secondaryLabelColor`, dan teks baris `List` atau `Table` tidak berubah.
- `window.appearance = .accessibilityHighContrastAqua`: gambar identik byte dengan light biasa. `.darkAqua` bekerja.
- `_accessibilityReduceTransparency = true`: `.regularMaterial` menjadi abu abu opak; permukaan opak tidak berubah.
- `automationmodetool`: "Automation Mode is disabled. This device requires user authentication to enable Automation Mode."
- `XCUIAutomation.framework` macOS: jenis audit `.contrast`, `.elementDetection`, `.hitRegion`, `.sufficientElementDescription`, `.action`, `.parentChild`; tanpa `.textClipped` dan `.dynamicType`. `XCUIElement` macOS tanpa key equivalent dan tanpa `hasFocus`.
- `Stackly.xcodeproj` hanya punya target app; scheme `Stackly` tanpa Testables.

## Kesepakatan dengan sesi saudara (2026-10-21)

- **arc-f67-spec (spec 0067)**: komponen fitur 67 menerima nilai UI biasa; `ComponentSamples` `#if DEBUG` di `Gallery/` dipakai `#Preview`, halaman galeri Components, dan snapshot spec ini; spec 0067 memuat tabel state per komponen yang wajib tampil di galeri; konvensi `accessibilityIdentifier` `<scope>.<part>[.<key>]` dengan konstanta di `StacklyUI/Accessibility/AccessibilityID.swift`, dibuat oleh yang lebih dulu dibangun; scope komponen checklist adalah `checklist`.
- **arc-f69-spec (spec 0069)**: `CommandMap` publik dan read only (`CommandID`, `CommandEntry` dengan `title`, `alternateTitle`, `menu`, `shortcut`, `owner`, `status`, `scope`, `requirements`); cek duplikat key equivalent tetap di spec 0069 (unit test, aturan sumber, cek AX manual), karena `XCUIElement` macOS tidak membaca key equivalent; XCUITest menu (keberadaan, judul, aktif atau nonaktif per skenario, smoke `typeKey`) milik spec ini.

## Keputusan lead (2026-10-21)

Lead menerima rekomendasi K1 sampai K13 atas delegasi user, dengan enam syarat yang sudah masuk index.md: setter bergaris bawah hanya di target test dan kode `#if DEBUG` dengan aturan sumber, test penjaga, dan risiko di Consequences; Automation Mode hanya diaktifkan user dan tercatat `BLOCKED` dengan owner user bila mati, dengan `docs/manual.md` diperbarui lead; penyelarasan Done when fitur 68 diusulkan lewat laporan untuk `/scope`; product `StacklyXCTestSupport` dan perubahan peta package spec 0001 di *Usulan update untuk spec lain*, sementara kalimat baris 428 dan Follow-up 467 spec 0002 diperbarui langsung; pasangan kontras baru masuk spec 0012 lewat usulan, dan pengecualian audit menyebut spec penerima; subcommand xtask dan target Xcode baru ada di Build plan, dan baris Commands `AGENTS.md` diperbarui lead lewat `/sync`.

## Cross check

Dijalankan 2026-10-21 oleh model lain (Sonnet 5.5), read only, atas draft pertama index.md dan rationale.md. Lead menerima semua resolusi di bawah dan menerima spec ini atas delegasi user (2026-10-21), dengan dua aturan tambahan yang kini tertulis di *Key invariants* index.md: penantian render adalah polling sampai stabil yang gagal tanpa retry, bukan sleep tetap, dan setiap baris `AuditExceptions.swift` diajukan ke lead per baris.

**Gap kelengkapan keputusan**

| # | Temuan | Resolusi di spec |
| --- | --- | --- |
| G1 | Pemicu `.afterSnapshot`, aliran `SyncSignal`, cara menunggu presence, dan nilai `ConnectedDaemon`, `ErrorBody`, health asing, `RegistryRecovery`, serta kode error mutasi fixture tidak bernama | tabel *Nilai skenario*; aliran sinyal dan pemicu `.afterSnapshot` di *Fixture di app Debug*; `expectedPresence` dan `settledRuntime` (AC-3); mutasi memakai `ipc.not_connected` yang sudah ada di `ClientErrorCatalog` |
| G2 | Nomor kasus, ID operasi dan provider, provider, versi, port, dan selisih waktu tidak bernama | kolom `#` dan provider di tabel kasus, `FixtureID` (kind dan nomor), *Kasus provider*, baris waktu di *Nilai skenario* |
| G3 | "Animasi mati" tanpa mekanisme, batas `.fitting`, lebar banner dan footer, client halaman Secrets, model Diagnostics | animasi ditangani waktu stabil minimum, `.fitting` nol atau lebih dari 4000 pt gagal, `DaemonStatusMatrix` dengan footer 236 pt, `SecretsClient.fixture`, Diagnostics dari `settledRuntime` (AC-4, AC-8) |
| G4 | Light dan dark tidak memaku setting aksesibilitas lain; setting sistem asli bocor ke kontrol AppKit | `fixtureAppearance` selalu mengisi keempat nilai; setting sistem menyala membuat snapshot `BLOCKED` (AC-4, AC-5) |
| G5 | Artefak gagal lama, nama artefak RT, nama folder gagal, kapan batas 1 MB berlaku | AC-6 |
| G6 | Aturan halaman langsung gagal untuk `private struct UnbuiltScrollPage` | hanya struct tingkat atas yang tidak private, kata utuh di baris bukan komentar (AC-7) |
| G7 | Components tanpa syarat, lebar per halaman | AC-8 dan *Key invariants* |
| G8 | Nama selector `throws` dan `async`, tempat observer, teks `actual`, kind native, urutan blocked | AC-10 dan *Helper evidence XCTest*; `BLOCKED` tetap menang seperti trait Swift |
| G9 | `XCTSkipIf` dan `XCTSkipUnless` lolos, akar pindaian | AC-11 |
| G10 | `runtime` di `AppDelegate` dibuat eager lewat `live`; `exit(64)` dan appearance tanpa waktu; cara menambah target Xcode; visibilitas | `makeRuntime()`, `applicationWillFinishLaunching`, `pbxproj` ditulis tangan meniru target app, tipe `public` (AC-12, *UI test*) |
| G11 | Keluaran `automationmodetool` selain "disabled", konfigurasi, destination, paralel, path evidence relatif, ekspor saat gagal, layar terkunci | AC-13 |
| G12 | Identifier elemen, batas waktu, Space tanpa fokus, cara mengukur ukuran minimum, gate di shell yang mungkin kosong, cara membuka galeri, waktu di banner | daftar identifier dan batas waktu di *UI test*; Space dipindah ke cek manual; gate dicek di galeri (AC-15); waktu banner tidak dibandingkan (AC-14) |
| G13 | `CommandMap` belum ada, ID PRD AC-16, fitur 68 tertahan | AC-16 ditulis yang di merge kedua, `BLOCKED` owner fitur 69 bila belum ada; REQ-027 di *Trace PRD* |
| G14 | Arti cocok di `AuditExceptions`, unit test audit | AC-17 |
| G15 | Nama lampiran dan jendela yang dipotret | AC-18 |
| G16 | Base diff, rename, tabrakan nama, embed gambar, urutan | AC-9 |

**Soundness**

| # | Temuan | Resolusi |
| --- | --- | --- |
| S1 | `assertSnapshot` sinkron menahan main actor, jadi `.task` dan model tidak berjalan | `async throws` dengan `Task.sleep` di antara tangkapan (pola `ShellLayoutTests`); probe langkah 1 menambah `.task`, `onAppear`, dan perubahan `@Observable` |
| S2 | Dua tangkapan identik bisa stabil terlalu dini | waktu minimum 250 ms, batas 3 detik, dan `settledRuntime` sebelum memasang view bermodel |
| S3 | `.serialized` hanya per suite | satu suite induk `SnapshotTests` |
| S4 | Ruang warna jendela, setting sistem, `.now` di `StaleNote` | `colorSpace` sRGB, preflight setting sistem, fixture selalu mengisi `lastLoadedAt`, aturan `fixtureNow` untuk view waktu lain |
| S5 | Observer XCTest rapuh; opsi xcresult lebih sederhana | observer dipertahankan (keputusan K12 lead) dengan `Mutex`, pemasangan malas, dan tanpa retry; evidence dari xcresult dicatat sebagai alternatif yang ditolak karena `covers` dan `expected` tetap butuh tempat di kode test |
| S6 | `TEST_RUNNER_` benar; path harus absolut | AC-13 menolak path relatif |
| S7 | Dependency UI test ke `StacklyUI` butuh `public` | tipe yang dipakai ditandai `public` |
| S8 | Klaim Release terlalu lebar | dua kunci environment masuk Release dengan nil; cek `strings` di semua file bundle (AC-1, cek manual 6) |
| S9 | DWC di `FixtureAppearance` tanpa varian; path `exclude` salah; autosave frame jendela | DWC dipaku false tanpa varian (Status galeri sudah punya kolom DWC); `exclude` `SnapshotTests/__Snapshots__`; jendela fixture tidak restorable dan tanpa autosave (AC-12) |
| S10 | Spinner tanpa akhir, glass dan wallpaper di audit, layar terkunci | cek di langkah 10 dengan jalan keluar tercatat; pengecualian glass lewat lead; layar terkunci keluar 2 |
| S11 | Paralel milestone 1 dan 2, urutan rekam dan audit, ketergantungan 0067 dan 0069 | catatan di Build plan dan *Key invariants* |
| S12 | 140 PNG status daemon | `DaemonStatusMatrix`, lima PNG |

Nitpick yang diterapkan langsung: satu parameter `sourceLocation`, nama `XCTEvidenceObserver` seragam, tinggi `.page` pasti 748 pt, UUID disebut "berbentuk v7", cek manual 6 merujuk AC-1 dan AC-12.

## Perubahan 2026-10-21 (update wave 3)

Update `/architect` sesi `arc-f1368-update` setelah milestone 1 dan 2 dibangun dan diverifikasi (`chk-f68-m1`). Spec kini menggambarkan helper seperti yang dibangun. ID AC tetap, dan langkah Build plan tidak berubah selain satu catatan di langkah 5.

| Butir | Perubahan | Sumber |
| --- | --- | --- |
| AC-3, AC-4 | batas tunggu 3 detik (dan 250 ms di AC-4) dihitung sebagai waktu responsif `ResponsiveTimer`: hanya waktu saat main actor responsif, tidak pernah lebih dari waktu dinding. Teks AC disetujui user lewat lead (2026-10-21) | penyimpangan developer `dev-f68-m1` yang diterima checker `chk-f68-m1`, dengan saran klarifikasi kata "3 detik" |
| *Helper snapshot*, *Value sourcing*, *Key invariants* | cara `ResponsiveTimer` menghitung waktu, dan bahwa batas di AC-3 dan AC-4 adalah batas waktu ini | sama |
| *Helper snapshot*, langkah 5 | "isi halaman galeri sebagai view terpisah" ditulis seperti yang dibangun: kunci environment `galleryScrollsPage` di `GalleryPageLayout`, dan halaman States serta Components yang melebihi batas fitting 4000 pt dirender per kelompok | kode `GallerySupport.swift` dan `GallerySnapshotTests` |
| `verify.md` milestone 1, langkah perubahan token | menyebut `STACKLY_SNAPSHOT_RECORD=1` sebelum `snapshot review`, karena tanpa rekam ulang referensi di git tidak berubah dan review mencetak "no snapshot changes" | catatan checker di langkah itu |
| *Usulan update untuk spec lain* dan *Follow-up* | butir spec 0013 diterapkan di spec 0013 (update gabungan wave 3), jadi Follow-up `/architect` update dicentang | update spec 0013 di sesi yang sama |

**Kenapa waktu responsif, bukan waktu dinding.** Di run penuh, test lain bisa menahan main actor beberapa detik (misalnya run loop alert modal). Batas waktu dinding akan menggagalkan test fixture atau snapshot karena jeda yang tidak ia sebabkan, padahal view nya stabil. Setiap putaran polling menambah paling banyak sedikit di atas jedanya, jadi jeda panjang dihitung sebagai satu putaran. Aturan "tanpa retry dan tanpa sleep tetap" tetap berlaku: view yang benar benar tidak stabil tetap gagal setelah 3 detik waktu responsif. Runner up: batas waktu dinding yang lebih panjang, yang menutupi masalah tanpa menghilangkan kegagalan palsu di Mac yang sibuk.

**Cross check update ini (2026-10-21, model lain: Sonnet 5.5, baca saja).** Diperbaiki langsung: halaman Secrets memegang `ScrollView` sendiri dan tidak membaca `galleryScrollsPage`; jendela fixture dibuat tidak restorable oleh `FixtureWindowSettings` lewat `.fixtureWindow()`, bukan oleh `MainWindowAccessor`; `ShellBootstrap.fixture` memakai `.fixture(link)` dengan `FixtureLink(scenario)`; angka putaran `ResponsiveTimer` (10 ms dengan batas 60 ms untuk `settledRuntime`, 50 ms dengan batas 100 ms untuk render). Kalimat AC-8 tentang halaman galeri Components tetap, karena syaratnya kini terpenuhi dan suite nya sudah ada.

**Keputusan lead (2026-10-22).** Cross check Sonnet dan perbaikannya diterima lead atas delegasi user, dan spec ini diterima tanpa perubahan Status (`In Progress`).

## Perubahan 2026-10-22 (update dari usulan spec 0015, wave 5)

Update `/architect` sesi `arc-f1368-0015`. Sumbernya baris 0068 di *Usulan update untuk spec lain* spec 0015 (*Skenario* dan harapan banner fixture 2026-10-22) dan paragraf **Fixture (spec 0068)** di spec 0015 *GUI*, ditambah tiga butir log lead: aturan spinner `loadingDelay` (perbaikan `9180a9a`, merge `386f512`), `FixtureContract` (merge `dc88025`), dan tiga Follow-up yang mungkin sudah selesai. Perilaku agent ditulis sebagai yang berlaku mulai langkah 11 dan 12 milestone 3 fitur 15. ID AC tetap dan teks AC tidak diubah (lihat *Teks AC* di bawah).

| Butir | Perubahan | Sumber |
| --- | --- | --- |
| Batas dengan spec lain | baris spec 0015 untuk `AgentBanner`, model, dan kedua fake | spec 0015 *GUI* |
| Model data | `FixtureContract`, `FixtureAgent`, `FixtureClock.secondsAgo(_:)`, `FixtureScenario.agent`, `AgentBannerMatrix`; `DaemonStatusMatrix` tidak berubah | spec 0015 paragraf **Fixture**; K14 di bawah; merge `dc88025` |
| Katalog skenario | agent `.fresh` di setiap skenario dan banner jendela `not_registered` untuk `daemon-offline` dan `daemon-unreachable`; *Skenario agent* (K14) | spec 0015 baris 0068 |
| Nilai skenario | `HandshakeAccepted` dan `DaemonInfo` dari `FixtureContract` (dulu tertulis `ContractInfo`); nilai agent dan Command Line di runtime fixture | kode `FixtureBuilders.swift` di `main`; spec 0015 paragraf **Fixture** |
| Kasus yang menunggu DTO | fitur pemilik memakai `HandshakeAccepted.fixture` dan `DaemonInfo.fixture`, tidak pernah `ContractInfo` | butir log lead 2 |
| *Kontrak fixture* (subbagian baru) | aturan pin 1.8, `pinningFixtureContract()`, dan test penjaga nya | merge `dc88025` |
| Fixture di app Debug | fake agent dan Command Line, assessment awal sinkron, jam `DaemonModel`, tanpa `SMAppService` dan tanpa file pengguna | spec 0015 paragraf **Fixture** |
| Kunci environment | `fixtureNow` juga untuk jam 15 detik agent; aturan `loadingDelay` untuk setiap spinner tertunda dan spinner di frame pertama untuk delay nol | butir log lead 1 |
| UI test | identifier banner seperti yang dibangun: hanya `daemonBanner.container`, judul dari label nya, aksi dari judul tombol | kode `AccessibilityID.swift` dan `ShellUITests.swift` di `main` |
| API surface, Value sourcing, Key invariants, Critical test scenarios | baris untuk kontrak fixture, agent, jam 15 detik, dan spinner | turunan |
| Build plan | tanpa langkah baru; langkah 11 dan 12 fitur 15 membangun bagian agent | *Key invariants* (fitur pemilik setelah milestone dibangun) |
| Usulan update untuk spec lain | butir spec 0015 (seam fake, launch record, jam, `bundledTool`, teks langkah 11 dan 12, selisih 10 dan 12 detik) dan spec 0067 (diterapkan) | turunan |
| Follow-up | tiga butir dicentang dengan bukti; dua butir baru | `c4c0bfe`, `9a02771`, `c6f6ae7` |

### Keputusan K14: skenario per state agent (Option A, dipilih user lewat lead 2026-10-22)

Spec 0015 menyerahkan skenario tambahan per state agent untuk snapshot dan XCUITest ke spec ini. Yang harus diputuskan: state mana yang jadi skenario runtime, apa yang masuk snapshot dan apa yang masuk XCUITest, siapa yang membangun, dan bagaimana ambang 15 detik dibuat deterministik.

Fakta yang membatasi pilihan: `AgentBanner.content(_:now:)` adalah fungsi murni dengan `now` sebagai input; `DaemonBanner` tanpa `DaemonModel` di environment menggambar konten yang diberikan; runtime fixture memakai `FixtureClock.now` sebagai jam model dan `fixtureNow` jendela; `since` datang dari naskah koneksi skenario; `justRegistered` selalu mulai false di model baru (spec 0015), jadi `not_found` hanya bisa dicapai lewat aksi Turn On; dan `rootOverrideActive` selalu false di fixture.

#### Option A: matriks murni untuk semua baris banner, ditambah lima skenario runtime untuk alur (direkomendasikan)

- `AgentBannerMatrix` (satu nama snapshot, lima PNG) memuat setiap baris *Banner per state*, termasuk `enabled` di detik 14 dan 15 dan dengan `isStarting`, semua alasan `unavailable`, dan baris dengan awalan waktu.
- Lima skenario runtime (`agent-requires-approval`, `agent-disabled`, `agent-enabled`, `agent-not-found`, `agent-unavailable`) untuk XCUITest seperti AC-14 dan tiga alur (Turn On ke `requires_approval`, Turn On ke `not_found`, Start stacklyd ke "starting"), plus satu snapshot Diagnostics baru (`diagnostics-agent-disabled`).
- Jam: matriks memakai `secondsAgo(14)` dan `secondsAgo(15)` dengan `now` tetap; runtime memakai `since` tetap dari naskah, jadi tidak ada jam nyata.
- Dibangun langkah 11 dan 12 milestone 3 fitur 15 di commit yang sama dengan perubahan banner.
- Kelebihan: setiap baris terlihat di satu gambar yang mudah direview, ambang 15 detik dibuktikan piksel tanpa menunggu, tombol baru dicoba di app sungguhan (label VoiceOver, aktif atau tidak), dan tidak ada seam fixture yang melanggar aturan `justRegistered`. Kekurangan: lima nama skenario baru (katalog 21), dan baris `root_override` serta `enabled` di bawah 15 detik hanya ada di matriks.

#### Option B: satu skenario runtime per baris banner, tanpa matriks

- Sekitar sembilan skenario (termasuk `enabled` sebelum 15 detik, `not_found`, dan tiga alasan `unavailable`), masing masing dengan snapshot Diagnostics dan XCUITest.
- Butuh seam fixture untuk mengisi `justRegistered` true (melanggar aturan spec 0015 bahwa model baru selalu mulai false) dan `rootOverrideActive` true di fixture, sekitar 45 PNG Diagnostics, dan XCUITest yang lebih lama.
- Kelebihan: setiap state terlihat di app sungguhan tanpa aksi. Kekurangan: banyak gambar yang isinya hampir sama, seam khusus fixture yang mengubah aturan spec 0015, dan biaya rekam ulang lebih besar setiap kali teks berubah.

#### Option C: matriks saja, tanpa skenario runtime baru

- `AgentBannerMatrix` ditambah harapan baru `daemon-offline` dan `daemon-unreachable`; alur aksi hanya dibuktikan test model spec 0015.
- Kelebihan: paling murah, katalog tetap 16. Kekurangan: Open Login Items, Check Again, dan Start stacklyd tidak pernah tampil di app yang diuji XCUITest dan audit, jadi label VoiceOver dan status aktif tombol itu hanya dicek manual.

**Rekomendasi: Option A.** Ia membuktikan setiap baris secara piksel dengan biaya satu gambar, membuat jam 15 detik deterministik tanpa seam baru, dan tetap mencoba setiap tombol baru di app sungguhan. Option B menambah seam yang bertentangan dengan spec 0015 untuk hasil yang sebagian besar sudah ditangkap matriks; Option C meninggalkan tombol baru tanpa XCUITest.

**Siapa yang membangun.** Langkah 11 dan 12 milestone 3 fitur 15 (milik scope 02), karena *Key invariants* spec ini menyerahkan suite bagian yang di merge sesudah milestone spec ini ke fitur pemilik, dan spec 0015 meminta harapan fixture, XCUITest, dan referensi Diagnostics diperbarui di commit yang sama dengan perubahan banner. Runner up: langkah 16 baru di fitur 68 sesudah langkah 11, yang membuat `FixtureShellTests` dan `ShellUITests` merah di antara dua commit itu.

### Teks AC

Commit pertama (`a6f17d3`) tidak mengubah teks AC; AC-14 diubah sesudahnya atas persetujuan user (lihat *Keputusan user* di bawah). AC-2 tetap benar karena ia memetakan skenario ke presence dan `presentation.banner`, yang tidak berubah oleh fitur 15. AC-3 tetap benar karena fake agent dan Command Line ada di memori. AC-8 menyebut suite yang dibangun spec ini, dan suite agent dibangun fitur 15 sesuai *Key invariants*. AC-14 dibaca dengan tabel *Tampilan status daemon* spec 0013 beserta butir *Tambahan fase 02* di bawah nya (yang sejak AC-9 spec 0013 menunjuk tabel spec 0015 untuk kedua kode), dan dengan cara mencari banner seperti yang dibangun (label `daemonBanner.container`, tombol lewat judul). Karena teks AC-14 masih menyebut `daemonBanner.title` dan `daemonBanner.action.*`, yang tidak dibangun, satu klarifikasi teks AC-14 yang opsional dikirim ke lead, dan user menyetujuinya.

**Keputusan kecil yang diambil update ini.**

- Skenario agent memakai `ipc.endpoint_missing`, sedangkan `daemon-offline` dan `daemon-unreachable` memakai `ipc.connection_refused`, supaya kedua kode yang memicu baris agent sama sama dicoba. Runner up: satu kode untuk semua.
- `FixtureAgent.fresh` menjawab `register()` dengan `requiresApproval` (cabang B AC-17 spec 0015 untuk build bertanda tangan), supaya Turn On di skenario dasar punya hasil yang terlihat dan tidak menyalakan daemon yang tidak ada. Runner up: `enabled`, yang hanya memberi "stacklyd is starting…" lalu "isn’t running" setelah 30 detik.
- Aturan spinner ditulis untuk spinner baru, dan `DelayedProgress` di `StateView` dibiarkan seperti yang dibangun karena stabil di semua suite; kalau suatu saat tidak stabil, ia diselaraskan, bukan diberi retry.
- Selisih identifier di luar banner (`daemonFooter.settings`, `gallery.confirmation.*`, `gallery.states.*` tidak ada di `AccessibilityID.swift`) dicatat sebagai Follow-up, karena di luar lingkup update ini.

**Cross check update ini (2026-10-22, model lain: Sonnet 5.5, baca saja).** Diperbaiki langsung: `ShellBootstrap.fixture` membangun model agent dan Command Line sendiri tanpa `assemble`; nilai awal yang diisi sinkron adalah hasil probe (bukan hanya assessment), supaya `unavailableDetail` ikut terisi di `agent-unavailable`; kalimat "baris dengan awalan waktu hanya ada di matriks" bertentangan dengan `daemon-offline` dan diperbaiki; bagian yang bergantung pada K14 diberi tanda *(K14)*; *Kontrak fixture* tidak lagi mengklaim kenaikan protokol tidak mengubah apa pun, karena pesan banner `majorMismatch` sisi app masih membaca `ContractInfo` (belum ada referensi yang menampilkannya). Dikirim ke lead sebagai gap keputusan bersama rekomendasi: nilai Command Line fixture yang tetap (home, halaman man, baris yang tampil), `afterRegister` untuk `agent-unavailable` dan skenario untuk `diagnostics-agent-disabled`, perilaku tombol `AgentBannerMatrix` tanpa model, dan butir jam dan teardown yang dicatat di rationale spec 0013.

**Keputusan lead atas gap cross check (2026-10-22): "apply all".** Yang ditulis di spec ini: (7) nilai Command Line fixture yang tetap (home `/fixture/home`, status `not_installed` tanpa link, `bundledTool` di bundle fake, satu halaman man `stackly.1` tanpa link), `afterRegister` `agent-unavailable` sama dengan `.fresh`, dan `diagnostics-agent-disabled` dari skenario `agent-disabled`; (8) tombol `AgentBannerMatrix` tanpa model tampil aktif tanpa aksi. K14 dan klarifikasi teks AC-14 masih menunggu jawaban user lewat lead; butir 7 dan 8 yang menyangkut skenario agent ikut berubah kalau K14 berubah.

**Keputusan user (2026-10-22, lewat lead).** K14 Option A dipilih: `AgentBannerMatrix` ditambah lima skenario runtime, XCUITest per skenario ditambah tiga alur, dan `diagnostics-agent-disabled`, dibangun langkah 11 dan 12 milestone 3 fitur 15. Tanda "menunggu keputusan" dihapus dari spec, dan tanda *(K14)* tetap sebagai penunjuk asal keputusan. Klarifikasi teks AC-14 disetujui persis seperti usulan dan diterapkan; ID tetap.

| AC | Teks lama (bagian yang berubah) | Teks baru |
| --- | --- | --- |
| AC-14 | "label VoiceOver `daemonFooter.status` ("Status: <teks footer>") dan judul `daemonBanner.title` sesuai tabel spec 0013, atau tanpa banner bila baris itu tanpa banner; aksi banner (`daemonBanner.action.reconnect`, `daemonBanner.action.openDiagnostics`) ada dan aktif sesuai tabel;" | "label VoiceOver `daemonFooter.status` ("Status: <teks footer>") dan judul banner (label `daemonBanner.container`) sesuai tabel spec 0013, atau tanpa banner bila baris itu tanpa banner, dan mulai langkah 11 milestone 3 fitur 15 sesuai baris *Banner per state* spec 0015 untuk skenario dengan `ipc.endpoint_missing` atau `ipc.connection_refused`; setiap aksi banner (tombol dengan judul nya di dalam `daemonBanner.container`) ada dan aktif sesuai baris itu;" |

Alasannya: `daemonBanner.title` dan `daemonBanner.action.*` tidak pernah dibangun (XCUITest mencari banner lewat label `daemonBanner.container` dan tombol lewat judul), dan setelah langkah 11 skenario dengan kedua kode menampilkan baris agent spec 0015, misalnya Turn On di `daemon-offline`.
