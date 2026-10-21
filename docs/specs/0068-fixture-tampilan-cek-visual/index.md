# 0068. Fixture tampilan dan cek visual

**Date**: 2026-10-21
**Status**: In Progress

## Summary

Spec ini memberi aplikasi macOS cara mengecek tampilan tanpa daemon nyata: data contoh (fixture) yang dibangun dari tipe kontrak `StacklyClient`, snapshot test (gambar referensi PNG yang dibandingkan piksel per piksel) untuk isi setiap halaman di light, dark, Increase Contrast, Reduce Transparency, dan lebar sempit 640 pt sebagai cek teks besar, plus UI test XCUITest yang menjalankan app Debug di atas fixture untuk membaca label VoiceOver, menekan tombol, memeriksa menu, dan menjalankan audit aksesibilitas Apple. Semuanya buatan sendiri tanpa library pihak ketiga, berjalan di Mac ini tanpa CI, dan perubahan gambar terlihat di git serta di halaman review lokal. Setting sistem asli (Increase Contrast, Reduce Transparency, Differentiate Without Color, Reduce Motion), fokus Tab, dan suara VoiceOver tetap dicek dengan tangan, karena test tidak bisa mengubah setting sistem dan XCUITest macOS tidak bisa membaca fokus keyboard.

## Requirements

**User stories**:

- Sebagai developer layar journey, saya ingin merender halaman saya di setiap state dan setiap tampilan dari data contoh yang bertipe kontrak, supaya saya melihat hasilnya tanpa menjalankan `stacklyd` dan tanpa membuat model data kedua.
- Sebagai reviewer, saya ingin setiap perubahan tampilan muncul sebagai gambar yang berubah di git dan di satu halaman review, supaya regresi warna, kontras, atau teks terpotong terlihat sebelum merge.
- Sebagai pengguna dengan Increase Contrast, Reduce Transparency, atau keyboard dan VoiceOver, saya ingin setiap layar dicek otomatis untuk label yang hilang, target klik yang terlalu kecil, dan kontras, supaya aksesibilitas tidak bergantung pada ingatan reviewer.
- Sebagai pemilik evidence (PRD bagian 28), saya ingin UI test menulis evidence record yang sama dengan test Swift lain, supaya bukti GUI tidak lagi hanya manual.

**Acceptance criteria** (ID lokal spec ini ditulis `AC-n`; ID PRD selalu tiga digit dan disebut "PRD AC-0nn"):

- **AC-1**: Folder `StacklyUI/Fixtures/` (setiap file diawali `#if DEBUG`) memuat katalog `FixtureScenario` yang datanya hanya dibangun dari DTO hasil generate `StacklyClient` (`InstanceSummary`, `OperationSummary`, `ProviderSummary`, `HandshakeAccepted`, `DaemonInfo`, `DaemonHealth`, `ErrorBody`, `Reply`, dan DTO lain yang sudah ada) lewat builder bernilai default, tanpa tipe model kedua untuk data daemon. Semua nilai mengikuti tabel *Nilai skenario*: waktu dari `FixtureClock`, ID dari aturan UUID bernomor kasus, dan tidak ada credential atau nilai secret (secret hanya sebagai reference). Test membuktikan setiap DTO di setiap skenario lolos encode lalu decode `StacklyJSON` dengan hasil sama. Tidak satu file pun di bundle Release (`cargo xtask build-app --release`) memuat string `FixtureScenario`, `STACKLY_UI_SCENARIO`, atau `fixtureAppearance`.
- **AC-2**: Katalog memuat setiap skenario di tabel *Katalog skenario* dengan nama tetap (`FixtureScenario.ID`, raw value kebab case) dan `expectedPresence` nya. Test memetakan setiap kasus di tabel *Kasus instance* lewat `StatusAdapter` dan `StatusAppearance.instance` spec 0012 ke label, tone, dan detail yang tertulis, setiap kasus di tabel *Kasus operasi* lewat `StatusAppearance.operation` ke label dan tone yang tertulis, dan setiap skenario daemon ke `DaemonPresence` dan judul banner baris tabel *Tampilan status daemon* spec 0013 yang sama. Kasus di tabel *Kasus yang menunggu DTO* tidak dibangun spec ini.
- **AC-3**: `ShellBootstrap.fixture(_:)` (`public`, hanya `#if DEBUG`) membuat `ShellRuntime` yang `DaemonModel` nya memakai `LinkHandle.fixture(_:)` dan aliran `SyncSignal` sesuai *Fixture di app Debug*: state koneksi mengikuti naskah skenario, setiap call baca dijawab dari data skenario, dan call mutasi dijawab balasan skenario untuk method itu atau `ClientError.notSent` dengan `ErrorBody` `ipc.not_connected` dari `ClientErrorCatalog`, jadi umpan baliknya sama dengan app nyata yang tidak tersambung. Ia tidak pernah membuat socket, tidak membaca `STACKLY_ROOT`, tidak menjalankan proses, dan tidak menulis UserDefaults atau file. Helper test `settledRuntime(_:timeout:)` menunggu `model.presence` sama dengan `expectedPresence` paling lama 3 detik waktu responsif (hanya waktu saat main actor responsif, dihitung `ResponsiveTimer`, tidak pernah lebih dari waktu dinding). Test membuktikan presence akhir setiap skenario daemon, dan bahwa `daemon-offline` menampilkan state terakhir `connected-mixed` dengan `MutationAvailability.blocked`.
- **AC-4**: Helper snapshot di `StacklyUITests/Support/Snapshot/` (`assertSnapshot`, `async throws`, main actor) merender view di `NSHostingView` di dalam `SnapshotWindow`: subclass `NSWindow` borderless yang tidak pernah tampil dan tidak key, dengan `backingScaleFactor` dikunci 1 dan `colorSpace` `.sRGB`. Tangkapan dibuat dengan `bitmapImageRepForCachingDisplay` plus `cacheDisplay(in:to:)`, lalu digambar ulang ke konteks RGBA 8 bit sRGB; hasil konteks itulah yang disimpan dan dibandingkan. Ukuran kanvas: `.page` 1044 × 748 pt (narrow 640 × 748) dan `.fitting(width:narrowWidth:)` (tinggi dari `fittingSize`; tinggi nol atau lebih dari 4000 pt membuat test gagal dengan pesan yang menyebutnya). Environment render tetap: locale `en_US`, zona waktu UTC, kalender gregorian, `fixtureNow` = `FixtureClock.now`, `loadingDelay` nol, latar `windowBackgroundColor`, dan keempat setting aksesibilitas dipaku lewat `fixtureAppearance` (lihat AC-5). Helper menunggu dengan `Task.sleep` 50 ms di antara tangkapan dan menganggap view stabil setelah paling sedikit 250 ms sejak dipasang dan dua tangkapan berurutan identik piksel; setelah 3 detik tanpa stabil, test gagal dengan pesan "snapshot did not settle", menulis tangkapan terakhir sebagai artefak `actual` (AC-6), dan tidak diulang. Batas 250 ms dan 3 detik itu dihitung sebagai waktu responsif seperti AC-3. Sebelum render, helper memeriksa sesi login GUI (`CGSessionCopyCurrentDictionary()`) dan setting sistem asli (`NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast`, `...ShouldReduceTransparency`, `...ShouldDifferentiateWithoutColor`, `...ShouldReduceMotion`); tanpa sesi GUI atau dengan salah satu setting itu menyala, test tercatat `BLOCKED` (reason "no GUI login session" atau "system accessibility setting on: <nama>", owner user) lewat `Evidence.blocked`, karena kontrol AppKit akan ikut setting itu.
- **AC-5**: Setiap pemanggilan `assertSnapshot` menghasilkan lima gambar per state: `light` dan `dark` di ukuran utama, `hc-light` dan `hc-dark` (Increase Contrast), dan `narrow` (light di lebar sempit, cek teks besar spec 0012). `fixtureAppearance` selalu mengisi keempat nilai: `_colorSchemeContrast` (`.standard` atau `.increased`), `_accessibilityReduceTransparency`, `_accessibilityDifferentiateWithoutColor` (selalu false), dan `_accessibilityReduceMotion` (selalu false). Reduce Transparency dirender juga dan wajib identik piksel dengan `light` tanpa file sendiri, kecuali pemanggil menyatakan `systemMaterial: true`, yang menambah file `rt`. Satu test penjaga merender view uji dan gagal dengan pesan yang menyebut SDK kalau di varian HC view tidak membaca `colorSchemeContrast == .increased`, atau di varian RT tidak membaca `accessibilityReduceTransparency == true`.
- **AC-6**: Pembanding gagal kalau ukuran berbeda atau ada satu piksel saja dengan selisih kanal RGBA lebih dari 2 dari 255. Di awal setiap pemanggilan, helper menghapus artefak gagal lama untuk nama itu. Saat gagal, helper menulis `<nama>.<varian>.actual.png` dan `<nama>.<varian>.diff.png` (piksel beda merah di atas referensi yang diredupkan; untuk RT, pembandingnya referensi `light` dan akhirannya `rt`) ke `target/snapshot-failures/<nama file suite>/`, dan pesan issue menyebut jumlah piksel beda, selisih terbesar, dan path itu. Referensi yang belum ada ditulis lalu test gagal dengan pesan "recorded <path>, review and commit". Dengan `STACKLY_SNAPSHOT_RECORD=1`, referensi yang pikselnya berbeda ditulis ulang (yang sama tidak disentuh) dan test tetap gagal dengan pesan "re-recorded". Saat `STACKLY_EVIDENCE_DIR` terisi, helper tidak pernah menulis referensi: referensi hilang atau beda selalu `FAIL`. PNG lebih dari 1 MB tidak pernah ditulis sebagai referensi dan membuat test gagal, baik saat merekam maupun saat membandingkan.
- **AC-7**: Test aturan sumber (baris komentar `//` dilewati, seperti test aturan sumber spec 0012) gagal kalau: `_colorSchemeContrast`, `_accessibilityReduceTransparency`, `_accessibilityDifferentiateWithoutColor`, atau setter environment lain berawalan `_accessibility` muncul di luar `StacklyUI/Fixtures/FixtureAppearance.swift` dan folder `Tests/`; sebuah file di `StacklyUI/Fixtures/` tidak diawali `#if DEBUG`; `STACKLY_UI_SCENARIO` atau `STACKLY_UI_APPEARANCE` dibaca di luar `apps/macos/Stackly/AppDelegate.swift` atau di luar blok `#if DEBUG`; atau sebuah `struct <Nama>Page: View` tingkat atas yang tidak `private` atau `fileprivate` di `StacklyUI/Sources` (di luar `Gallery/`) tidak disebut, sebagai kata utuh di baris bukan komentar, di file mana pun di `Tests/StacklyUITests/SnapshotTests/`.
- **AC-8**: Suite snapshot yang dibangun spec ini, semuanya lulus dengan referensi di git: setiap halaman galeri desain (Tokens, Status, States, Confirmation, dan Secrets dengan `SecretsClient.fixture`) dalam `.fitting(width: 1044, narrowWidth: 640)`; `DaemonStatusMatrix`, view khusus test yang menumpuk satu baris per presence tabel *Tampilan status daemon* spec 0013 (nama presence, `DaemonFooter` selebar 236 pt, dan `DaemonBanner` atau "no banner") dalam `.fitting(width: 1044, narrowWidth: 640)`; `DiagnosticsPage` dengan `model.diagnostics` dari `settledRuntime` di skenario `connected-mixed`, `daemon-offline`, dan `daemon-untrusted`; dan `UnbuiltPage(section: .overview)`. Halaman galeri Components ditambah begitu halaman itu ada di `main` (lihat *Key invariants*).
- **AC-9**: `cargo xtask snapshot review [--base <rev>]` (default `main`) menghitung `git merge-base <rev> HEAD`, lalu mendaftar PNG di `**/__Snapshots__/**` yang ditambah, diubah, atau dihapus antara merge base dan working tree (`git diff --name-status --no-renames`, ditambah file untracked dari `git ls-files --others --exclude-standard`), urut menurut path. Versi merge base (`git show`) dan versi sekarang disalin ke `target/snapshot-review/base/<path repo>` dan `target/snapshot-review/current/<path repo>`, gambar `actual` dan `diff` dari `target/snapshot-failures/` ikut ditautkan bila ada, dan `target/snapshot-review/index.html` memuat satu baris per file (path yang di escape HTML, gambar lama, gambar baru, dengan `<img>` relatif). Perintah mencetak path absolut halaman itu. Tanpa perubahan, ia mencetak "no snapshot changes" dan keluar 0; rev yang tidak ada membuatnya keluar 1 dengan pesan git. Tidak ada crate baru.
- **AC-10**: Product baru `StacklyXCTestSupport` di `clients/swift` memberi `XCTestCase.evidence(covers:expected:file:line:) throws`, `XCTEvidence.actual(_:)`, dan `XCTEvidence.blocked(reason:owner:) throws -> Never`. Record memakai format `stackly.evidence/v1` yang sama persis dengan trait `.evidence`, karena penulis record dipindah ke target internal `StacklyEvidenceCore` yang dipakai kedua product. Urutan status sama dengan trait Swift: `BLOCKED` kalau `blocked` dipanggil (ia melempar `XCTSkip`), menang atas kegagalan; selain itu `FAIL` kalau test run punya kegagalan atau exception tak terduga, dengan `actual` dari deskripsi issue gagal pertama; selain itu record dibiarkan `NOT_RUN` kalau test di skip tanpa `blocked`; selain itu `PASS` dengan `actual` "all expectations passed" atau teks dari `XCTEvidence.actual`. `kind` selalu `fixture`; saat `STACKLY_EVIDENCE_KIND` bernilai `native`, helper mencatat `XCTFail`, jadi record nya `FAIL`. `test.target` adalah nama modul bundle test dan `test.name` berbentuk `<Kelas>/<method>()`, dengan akhiran selector `AndReturnError:` dan `WithCompletionHandler:` dibuang. Unit test helper membuktikan setiap baris pemetaan dan isi record.
- **AC-11**: `cargo xtask evidence check` menambah aturan Swift: file di folder `apps/macos/` yang namanya berakhiran `UITests` dan memanggil `evidence(covers:` gagal kalau ia juga memanggil `XCTSkip(`, `XCTSkipIf(`, atau `XCTSkipUnless(` langsung, atau menulis `kind:` di argumen `evidence`. Baris komentar `//` dilewati, sama seperti pindaian Swift yang sudah ada.
- **AC-12**: Target XCTest baru `StacklyAppUITests` (folder sinkron `apps/macos/StacklyAppUITests/`, target application `Stackly`) masuk Testables scheme `Stackly`. App Debug membaca `STACKLY_UI_SCENARIO` dan `STACKLY_UI_APPEARANCE` (`light` atau `dark`) hanya di `AppDelegate` dalam `#if DEBUG`, sebelum `runtime` dibuat: dengan skenario, `runtime` berasal dari `ShellBootstrap.fixture` dan `ShellBootstrap.live` tidak pernah dipanggil; nama skenario yang tidak dikenal mencetak `unknown STACKLY_UI_SCENARIO <nama>` ke stderr lalu keluar dengan kode 64 sebelum link apa pun dibuat. Selama skenario aktif, root setiap jendela mendapat `fixtureNow` dan `loadingDelay` nol, dan jendela utama serta jendela galeri tidak restorable dan tanpa nama autosave frame, jadi ukuran dan posisi jendela pengguna tidak dibaca atau ditimpa. Setiap peluncuran UI test juga memakai argumen `-ApplePersistenceIgnoreState YES`.
- **AC-13**: `cargo xtask ui-test [--only <Kelas>[/<method>]]` keluar 1 di luar macOS dan saat `STACKLY_EVIDENCE_DIR` terisi tetapi tidak absolut. Ia membaca stdout `automationmodetool` (tanpa argumen): teks yang memuat "Automation Mode is enabled" berarti lanjut; selain itu (teks "disabled", teks lain, tool tidak ada, atau exit bukan 0) ia tidak menjalankan `xcodebuild`, mencetak keluaran tool dan bahwa user harus mengaktifkan Automation Mode sendiri (tanpa pernah menjalankan perintah pengaktifannya), lalu keluar 2. Layar yang terkunci (`CGSSessionScreenIsLocked` di `CGSessionCopyCurrentDictionary()`) juga keluar 2. Kalau siap, ia menjalankan `xcodebuild test -project apps/macos/Stackly.xcodeproj -scheme Stackly -configuration Debug -destination platform=macOS,arch=arm64 -derivedDataPath target/xcode -resultBundlePath target/ui-tests/<waktu UTC>/result.xcresult -parallel-testing-enabled NO -only-testing:StacklyAppUITests[/...]`, tanpa `-retry-tests-on-failure`; meneruskan `STACKLY_EVIDENCE_DIR` dan `STACKLY_EVIDENCE_KIND` sebagai `TEST_RUNNER_STACKLY_EVIDENCE_DIR` dan `TEST_RUNNER_STACKLY_EVIDENCE_KIND` bila terisi; selalu mengekspor lampiran ke `target/ui-tests/<waktu UTC>/attachments/` lewat `xcrun xcresulttool export attachments` bila result bundle ada, juga saat test gagal; dan keluar dengan exit code `xcodebuild`.
- **AC-14**: XCUITest shell, untuk setiap skenario daemon di *Katalog skenario* (kecuali yang dikecualikan langkah 10): label VoiceOver `daemonFooter.status` ("Status: <teks footer>") dan judul `daemonBanner.title` sesuai tabel spec 0013, atau tanpa banner bila baris itu tanpa banner; aksi banner (`daemonBanner.action.reconnect`, `daemonBanner.action.openDiagnostics`) ada dan aktif sesuai tabel; di `daemon-offline`, setiap instance `connected-mixed` yang tampil di jendela memakai label status yang sama seperti di `connected-mixed`; badge sidebar `sidebar.row.instances` bernilai "{n} instances" saat terhubung dan "{n} instances, last known" saat offline; dan jendela utama yang sudutnya diseret jauh ke kiri atas berhenti dengan frame paling sedikit 900 × 600 pt (toleransi 1 pt). Test hanya membandingkan judul dan aksi banner, tidak kalimat waktunya. Memenuhi PRD AC-024.
- **AC-15**: XCUITest galeri desain, dibuka lewat menu Window > Design Gallery di skenario `connected-empty`: setiap `StatusLabel` di halaman Status terbaca "Status: <label>" ditambah ", <detail>" bila ada; di halaman Confirmation, Return dan Escape pada dialog destructive (sheet) menutupnya tanpa menjalankan aksi ("Nothing ran"), Return pada dialog standard menjalankannya, dan Escape membatalkannya; di halaman States, menekan Show All membuka item sisa, dan dengan "Daemon Unreachable" menyala, tombol gate dan tombol unsupported nonaktif dengan alasan yang terbaca sebagai teks di jendela. Space pada Show All butuh fokus keyboard, jadi tetap cek manual. Memenuhi PRD AC-027.
- **AC-16**: XCUITest menu: setiap baris `CommandMap` spec 0069 yang berstatus `built` ada di menu `menu` nya dengan `title` atau `alternateTitle`, dan aktif atau nonaktif sesuai `scope` dan `requirements` baris itu di skenario `daemon-offline`, `connected-empty`, `connected-mixed`, dan `onboarding` (setelah fitur 16); ⌘, membuka jendela Settings. Suite ini ditulis oleh yang di merge kedua di antara milestone 3 spec ini dan build `CommandMap` spec 0069; kalau `CommandMap` belum di `main` saat fitur 68 diverifikasi, langkah ini tercatat `BLOCKED` dengan owner fitur 69 dan tidak menahan fitur 68.
- **AC-17**: XCUITest audit, satu method test per kombinasi skenario (`connected-mixed`, `connected-empty`, `daemon-offline`) dan appearance (light, dark), yang memilih setiap bagian sidebar (`sidebar.row.<section>`) bergiliran, ditambah satu method per appearance untuk setiap halaman galeri desain: `performAccessibilityAudit(for:)` dengan `.contrast`, `.elementDetection`, `.hitRegion`, `.sufficientElementDescription`, `.action`, dan `.parentChild` tidak menghasilkan issue, kecuali issue yang cocok dengan satu baris `AuditExceptions.swift`. Cocok berarti `auditType` sama dan elemen issue tidak nil serta `identifier` nya sama dengan `element`, atau `label` nya memuat `element`. Setiap baris punya alasan dan spec yang menerimanya; daftar dimulai kosong, dan setiap baris baru diajukan ke lead satu per satu sebelum ditulis. Memenuhi NFR-012 dan PRD AC-027.
- **AC-18**: Setiap test di AC-14 sampai AC-17 melampirkan screenshot jendela yang diuji (jendela utama dicari lewat elemen `sidebar.row.overview`, jendela galeri lewat judul "Design Gallery") setelah assertion langkah itu dan sebelum audit, sebagai `XCTAttachment` dengan lifetime `.keepAlways` dan nama `<method>-<skenario>-<appearance>-<bagian atau halaman>`. AC-13 mengekspornya untuk dilihat reviewer; screenshot tidak dibandingkan piksel.
- **AC-19**: `apps/macos/design.md` mendapat bagian "Testing views" (bahasa Inggris) yang menjelaskan cara menambah kasus dan skenario fixture, menulis suite snapshot halaman, merekam dan mereview referensi, menjalankan `cargo xtask ui-test`, dan menambah baris `AuditExceptions.swift`, beserta batas yang tercatat di spec ini.

## Decision

**Chosen option**: opsi yang direkomendasikan di keputusan K1 sampai K13 di [rationale.md](rationale.md), diterima lead atas delegasi user (2026-10-21), ditambah enam syarat lead: setter bergaris bawah hanya di target test dan kode `#if DEBUG`; Automation Mode hanya diaktifkan user; penyelarasan Done when lewat `/scope`; peta package spec 0001 dan pasangan kontras spec 0012 lewat *Usulan update untuk spec lain*; pengecualian audit menyebut spec yang menerimanya; baris Commands `AGENTS.md` lewat `/sync`. Cross check (Sonnet 5.5) dan semua perbaikannya diterima lead atas delegasi user (2026-10-21) dan tercatat di rationale.md, bagian *Cross check*.

Fitur 68 membangun fixture bertipe kontrak di `StacklyUI`, helper snapshot buatan sendiri di test `StacklyUI`, target XCUITest yang menjalankan app Debug di atas fixture, helper evidence XCTest di `clients/swift`, dan dua perintah xtask (`snapshot review`, `ui-test`), tanpa dependency pihak ketiga.

**Implementation skills**: `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`, untuk dua subcommand xtask)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya: skill Swift Testing menganjurkan Swift Testing di semua tempat, tetapi UI test macOS tetap XCTest karena `XCUIApplication` dan audit aksesibilitas hanya berjalan di bundle UI test XCTest.

## Feature design

### Trace PRD

| ID PRD | Dipenuhi oleh |
| --- | --- |
| REQ-024 (state global shell: loading, empty, error, stale, unsupported, reconnect) | AC-2, AC-3, AC-8, AC-14 |
| REQ-027 (keyboard, VoiceOver, ukuran teks, light dan dark, status selain warna, konfirmasi destructive) | AC-5, AC-15, AC-16, AC-17 |
| NFR-012 (kontras, status, dan error tidak hanya lewat warna) | AC-5, AC-8, AC-17 |
| PRD AC-024 (daemon tidak terjangkau: status koneksi, retry, diagnostics, tanpa menyatakan instance stopped) | AC-14 |
| PRD AC-027 (alur keyboard dan VoiceOver tanpa informasi yang hanya lewat warna atau hover) | AC-15, AC-16, AC-17; alur membuat, menghentikan, dan memeriksa instance dicek suite fitur 20, 21, dan 22 di atas harness ini |
| PRD bagian 28 (evidence record, fixture dibedakan dari native) | AC-10, AC-11; UI test selalu `fixture` |

### Batas dengan spec lain

| Bagian | Pemilik |
| --- | --- |
| Token, `StatusAppearance`, `StatusLabel`, `ToneBadge`, `ViewState`, `StateView`, `DelayedProgress`, galeri desain, tabel Pasangan kontras dan `ContrastTests` | spec 0012 (dipakai apa adanya; dua kunci environment baru dan perubahan kecil galeri ada di *Usulan update untuk spec lain*) |
| `DaemonModel`, `LinkHandle`, `ShellBootstrap.live`, `ShellRuntime`, `AppShell`, `DaemonBanner`, `DaemonFooter`, `DiagnosticsPage`, tabel *Tampilan status daemon*, `AppDelegate`, `StacklyApp` | spec 0013 (spec ini menambah `ShellBootstrap.fixture`, pemilihan runtime di `AppDelegate` Debug, dan environment fixture di root jendela) |
| Format evidence, trait `.evidence`, wrapper `cargo xtask evidence run`, aturan `evidence check` | spec 0002 (spec ini menambah padanan XCTest dan satu aturan pindaian) |
| `ComponentSamples`, halaman galeri Components dan daftar state per komponen, konstanta pertama di `AccessibilityID.swift` | fitur 67 (spec 0067); spec ini men-snapshot dan mengaudit halaman galeri itu |
| `CommandMap`, `CommandEntry`, `SystemShortcuts.reserved`, cek duplikat shortcut (unit test, aturan sumber, cek AX manual) | fitur 69 (spec 0069); spec ini memiliki XCUITest menu (AC-16) |
| Fixture, skenario, `FixtureAppearance`, helper snapshot, `DaemonStatusMatrix`, suite snapshot dasar, target `StacklyAppUITests`, test shell, galeri, menu, audit, helper evidence XCTest, `snapshot review`, `ui-test` | spec ini |
| Suite snapshot dan XCUITest per layar journey, dan kasus fixture yang DTO nya mereka tambah | fitur pemilik layar (16, 17, 18, 20, 21, 22, dan seterusnya), memakai harness ini |
| `docs/manual.md` (langkah Automation Mode) dan baris Commands di `AGENTS.md` | lead, setelah spec di merge dan lewat `/sync` |

### Model data

Fitur ini tidak menyimpan state aplikasi dan tidak menambah schema atau DTO. Semua tipe baru hidup di kode Debug atau test:

| Tipe | Tempat | Isi |
| --- | --- | --- |
| `FixtureClock` | `StacklyUI/Fixtures/`, `nonisolated`, `public` | `static let now = Date(timeIntervalSince1970: 1_792_300_000)` (nilai yang sudah dipakai `ShellPreviewSamples`), `minutesAgo(_:)`, `daysAgo(_:)` |
| `FixtureID` | sama | `static func uuid(kind: UInt8, number: UInt8) -> UUID` lewat `UUID(uuid:)`: byte 0 sampai 5 nol, byte 6 `0x70`, byte 8 `0x80`, byte 14 `kind`, byte 15 `number`. Berbentuk UUID v7 tapi bukan v7 sungguhan; tanpa force unwrap |
| `FixtureScenario` | sama, `Sendable`, `public` | `id: ID` (enum `String` kebab case, `CaseIterable`, `public`), `connection: [FixtureConnectionStep]`, `handshake: HandshakeAccepted?`, `daemonInfo: DaemonInfo?`, `health: DaemonHealth?`, `instances: [InstanceSummary]`, `operations: [OperationSummary]`, `providers: [ProviderSummary]`, `replies: [String: FixtureReply]` (kunci: nama method `<area>.<nama>` dari `ContractNames`), `onboardingFinished: Bool`, `expectedPresence: DaemonPresence?` (nil untuk `onboarding`) |
| `FixtureConnectionStep` | sama | `.state(ConnectionState)`, `.afterSnapshot(ConnectionState)` (lihat *Fixture di app Debug*), `.holdSnapshot` (call `list` store tidak pernah dijawab, untuk `refreshing`) |
| `FixtureReply` | sama | `.reply(Reply)` atau `.error(ClientError)` |
| `FixtureAppearance` | `StacklyUI/Fixtures/FixtureAppearance.swift`, `public` | `scheme: ColorScheme`, `increasedContrast: Bool`, `reduceTransparency: Bool`; `View.fixtureAppearance(_:)` mengisi `colorScheme`, `_colorSchemeContrast`, `_accessibilityReduceTransparency`, `_accessibilityDifferentiateWithoutColor` (false), dan `_accessibilityReduceMotion` (false), dan menjadi satu satunya tempat setter bergaris bawah di `Sources` |
| `SnapshotVariant` | `StacklyUITests/Support/Snapshot/` | `light`, `dark`, `hcLight`, `hcDark`, `narrow`, `reduceTransparency`; akhiran file `light`, `dark`, `hc-light`, `hc-dark`, `narrow`, `rt` |
| `SnapshotSize` | sama | `.page` (1044 × 748, narrow 640 × 748), `.fitting(width: CGFloat, narrowWidth: CGFloat)` |
| `DaemonStatusMatrix` | `StacklyUITests/SnapshotTests/` | view khusus test: satu baris per presence tabel spec 0013, dengan nama presence (`.caption`, monospace), `DaemonFooter` (236 pt), dan `DaemonBanner` atau teks "no banner" |
| `AuditException` | `StacklyAppUITests/AuditExceptions.swift` | `type: XCUIAccessibilityAuditType`, `element: String`, `reason: String`, `acceptedBy: String` (misalnya "spec 0012") |
| `XCTEvidenceObserver` | `clients/swift/Sources/StacklyXCTestSupport/` | `XCTestObservation` dengan state per test (record, flag blocked, `actual`, issue gagal pertama) di `Mutex` dari modul `Synchronization`, karena callback observer tidak berjalan di main actor |

### Katalog skenario

Nama adalah raw value `FixtureScenario.ID` dan nilai `STACKLY_UI_SCENARIO`. Kolom terakhir adalah `expectedPresence`, sama dengan baris tabel *Tampilan status daemon* spec 0013 yang dihasilkan.

| Skenario | Naskah koneksi | Data | Presence spec 0013 |
| --- | --- | --- | --- |
| `connected-empty` | connected, health ready | tanpa instance, tanpa operasi (workspace kosong) | `connected(ready)` |
| `connected-mixed` | connected, health ready | semua kasus di *Kasus instance*, *Kasus operasi*, dan *Kasus provider* | `connected(ready)` |
| `daemon-degraded` | connected, health degraded (subsystem `registry` `degraded`) | data `connected-mixed` | `connected(degraded)` |
| `daemon-health-unrecognized` | connected, status health `unrecognized` | data `connected-mixed` | `connected(unrecognized)` |
| `daemon-refreshing` | connected, `.holdSnapshot` | tidak pernah termuat | `refreshing` |
| `daemon-starting` | connected, health starting | data `connected-empty` | `starting` |
| `daemon-stopping` | connected, health stopping | data `connected-mixed` | `stopping` |
| `daemon-connecting` | connecting (attempt 1) tanpa akhir | tidak ada | `connecting(slow: true)` (setelah 1 detik; awalnya `slow: false`) |
| `daemon-offline` | connected dengan data `connected-mixed`, lalu `.afterSnapshot(unreachable)` | state terakhir `connected-mixed` | `unreachable` dengan `lastKnownAt` = `FixtureClock.now` |
| `daemon-unreachable` | unreachable sejak awal | tidak ada | `unreachable` tanpa `lastKnownAt` |
| `daemon-incompatible-major` | incompatible `.majorMismatch` (server 2.0) | tidak ada | `incompatible(.majorMismatch)` |
| `daemon-incompatible-rejected` | incompatible `.handshakeRejected` | tidak ada | `incompatible(.handshakeRejected)` |
| `daemon-untrusted` | untrusted, reason `group_or_other_access` | tidak ada | `untrusted` |
| `daemon-recovery` | connected, health dengan `RegistryRecovery` (sumber `RecoveryReport` spec 0013) | data `connected-empty` | `recovery` |
| `daemon-misconfigured` | misconfigured `.rootOverride(.empty)` | tidak ada | `misconfigured` |
| `onboarding` | sama dengan `connected-empty`, `onboardingFinished` false | ditambah fitur 16 saat `MainWindowRoot` ada | (onboarding tampil, `expectedPresence` nil) |

**Nilai skenario** (dipakai semua skenario kecuali disebut lain):

| Nilai | Isi |
| --- | --- |
| `HandshakeAccepted` | `protocol` dan `contractDigest` dari `ContractInfo` hasil generate, `serverInstanceId` `FixtureID.uuid(kind: 4, number: 1)`, `daemon` versi `0.0.0-fixture`, `contractDrift` false, `limits` sama dengan fixture kontrak `frame-response-handshake.json`, `head` cursor 0 |
| `ConnectedDaemon` | `accepted` di atas, `info` dan `health` skenario, `connectedAt` `FixtureClock.now` |
| `ErrorBody` unreachable | `ClientErrorCatalog.connectionRefused(path: "/fixture/stackly/stacklyd.sock")`, `since` `FixtureClock.now`, `nextAttempt` `FixtureClock.now` + 5 detik |
| `ErrorBody` untrusted | `ClientErrorCatalog.endpointUntrusted(path: "/fixture/stackly/stacklyd.sock", reason: .groupOrOtherAccess)` |
| `IncompatibleReason` | `.majorMismatch(server: 2.0, error: ErrorBody)` dan `.handshakeRejected(ErrorBody)` dengan kode dan pesan dari fixture kontrak `reject-handshake-major-2.json` |
| `RegistryRecovery` | reason `schema_newer`, `reportPath` `/fixture/stackly/recovery/report.json`, `detectedAt` `FixtureClock.minutesAgo(5)`, `appSchemaVersion` 3, `dbSchemaVersion` 4 |
| Mutasi tanpa balasan skenario | `ClientError.notSent` dengan `ErrorBody` `ipc.not_connected` dari `ClientErrorCatalog` |
| Waktu DTO | `createdAt` `FixtureClock.daysAgo(nomor kasus)`, `observedAt` readiness dan health `FixtureClock.minutesAgo(1)`, `lastFailure.at` `FixtureClock.minutesAgo(3)`, waktu operasi `FixtureClock.minutesAgo(2)` |
| `revision`, `statusSeq` | 1 untuk setiap item |

**Kasus instance** (semua di `connected-mixed`; `#` adalah nomor kasus untuk `FixtureID.uuid(kind: 1, number: #)`; nama dari mockup; label sesuai tabel Status instance spec 0012; runtime ID `FixtureID.uuid(kind: 3, number: #)`):

| # | Kasus | Nama | Provider, versi, port | DTO | Label, tone, detail |
| --- | --- | --- | --- | --- | --- |
| 1 | running dan ready | `pg17-dev` | `official/postgresql` 17.6, 5432 | lifecycle `running`, readiness `ready`, health `healthy`, endpoint `database` bound | Running, positive |
| 2 | starting | `orders-db` | `official/postgresql` 18.1, 5433 | lifecycle `running`, readiness `not_ready` reason `awaiting_probe` | Starting…, caution, Waiting for probe |
| 3 | stopping | `cache` | `official/valkey` 8.1.4, 6379 | lifecycle `stopping` | Stopping…, caution |
| 4 | stopped | `mysql-erp` | `official/mysql` 8.4.6, 3306 | lifecycle `stopped`, desired `stopped` | Stopped, neutral |
| 5 | crash loop | `queue` | `official/nats` 2.11.9, 4222 | lifecycle `failed`, `lastFailure.code` `crash_loop` dengan excerpt dua baris, `restart` 5 dari 5 dan `exhausted` true | Failed, critical |
| 6 | proses keluar | `analytics` | `official/clickhouse` 25.8.3, 8123 | lifecycle `failed`, `lastFailure.code` `process_exited`, `exitCode` 1 | Failed, critical |
| 7 | data dipakai proses lain | `legacy-pg` | `official/postgresql` 16.10, 5434 | lifecycle `stopped`, desired `stopped`, blocker `dataInUse` (bentuk fixture `frame-response-instance-get-data-in-use`) | Stopped, neutral |
| 8 | identitas belum terbukti | `ledger` | `official/postgresql` 17.6, 5435 | lifecycle `unknown`, blocker `identityUnproven` | Unknown, caution, Process identity not confirmed |
| 9 | secret hilang | `billing-db` | `official/postgresql` 17.6, 5436 | lifecycle `stopped`, desired `running`, blocker `needsSecret` | Stopped, neutral |
| 10 | belum siap | `search` | `official/postgresql` 18.1, 5437 | lifecycle `running`, readiness `not_ready` dengan detail "connection refused" | Not Ready, caution, connection refused |
| 11 | degraded | `metrics` | `official/postgresql` 17.6, 5438 | lifecycle `running`, readiness `ready`, health `degraded` | Running, positive, Degraded |

**Kasus operasi** (semua di `connected-mixed`; ID `FixtureID.uuid(kind: 2, number: #)`):

| # | Kasus | DTO | Label dan tone |
| --- | --- | --- | --- |
| 1 | install berjalan | `install_runtime` `running`, progress 42 persen, resource runtime kasus instance 2 | Running, informative |
| 2 | start antri | `start_instance` `queued`, resource instance 4 | Queued, neutral |
| 3 | stop gagal | `stop_instance` `failed` dengan `OperationFailureInfo`, resource instance 6 | Failed, critical |
| 4 | perlu recovery | `create_instance` `needs_recovery` dengan `RecoveryInfo` | Needs Recovery, critical |

**Kasus provider** (semua di `connected-mixed`): `official/postgresql` `available` kategori database; `official/valkey` `planned` dengan `stateReason` `provider_planned`; `official/clickhouse` `unsupported` dengan `stateReason` `no_build_for_host`.

**Kasus yang menunggu DTO** (tidak dibangun spec ini; fitur pemilik menambah kasus dan skenarionya di commit yang sama dengan DTO nya, lihat *Key invariants*):

| Kasus di Done when fitur 68 | Pemilik | Alasan |
| --- | --- | --- |
| konflik port (preflight create dan start yang gagal) | fitur 20 dan 21 | kode error dan baris preflight konflik port belum ada di kontrak `main` |
| drift config | fitur 29 | belum ada DTO revision dan drift |
| environment partially ready | fitur 44 | belum ada DTO environment |
| runtime external | fitur 18 (DTO runtime), fitur 56 (source external) | belum ada DTO runtime |
| onboarding per langkah | fitur 16 | `MainWindowRoot` dan `OnboardingRecordStore` belum di `main` |

### Fixture di app Debug

- `AppDelegate`: `let runtime = AppDelegate.makeRuntime()`, fungsi statis yang di `#if DEBUG` lebih dulu membaca `STACKLY_UI_SCENARIO`. Tidak dikenal berarti tulis `unknown STACKLY_UI_SCENARIO <nama>` ke stderr dan `exit(64)` sebelum link apa pun dibuat. Dikenal berarti `ShellBootstrap.fixture(scenario)`; kosong berarti `ShellBootstrap.live` seperti sekarang. `applicationWillFinishLaunching` mengisi `NSApp.appearance` dari `STACKLY_UI_APPEARANCE` (`light` menjadi `.aqua`, `dark` menjadi `.darkAqua`, nilai lain diabaikan dengan satu baris stderr). `StacklyUI` tetap tidak pernah membaca `ProcessInfo.environment` (aturan spec 0013).
- `StacklyApp` di `#if DEBUG`: saat skenario aktif, modifier `.fixtureWindow()` pada isi jendela utama dan jendela galeri memasang `.environment(\.fixtureNow, FixtureClock.now)` dan `.environment(\.loadingDelay, .zero)`, dan view latar kecil `FixtureWindowSettings` (bukan `MainWindowAccessor`) mengisi `isRestorable = false` dan nama autosave frame kosong pada jendela yang memuatnya *(seperti yang dibangun, 2026-10-21)*. Tanpa skenario, `.fixtureWindow()` tidak mengubah apa pun.
- `ShellBootstrap.fixture(_ scenario: FixtureScenario) -> ShellRuntime`: `DaemonModel(link: .fixture(link), signals: link.signals,` dengan `link = FixtureLink(scenario)` *(seperti yang dibangun)*, endpoint: nil, launchedAt: FixtureClock.now, now: { FixtureClock.now }, ...)`. `rootOverrideActive` false. Onboarding (setelah fitur 16) memakai store di memori dengan `onboardingFinished` skenario.
- Aliran sinyal: setiap kali naskah memancarkan `.connected`, aliran mengirim `.resnapshot(.noCursor)` lalu `.lastFrameAt(FixtureClock.now)`; tidak ada event lain.
- `LinkHandle.fixture(_:)`: `states` memancarkan naskah koneksi; `call` menjawab `instance.list` dan `operation.list` dengan satu halaman berisi semua item skenario, `instance.get` dan `operation.get` per ID (ID tidak ada: `ErrorBody` not found dari fixture kontrak yang sama), `catalog.list_providers` dengan `providers`, `daemon.get_info` dan `daemon.get_health` dengan nilai skenario, method lain dari `replies` atau `notSent`; `start`, `retryNow`, `refresh`, dan `stopNow` tidak melakukan apa pun di luar naskah. `.afterSnapshot(state)` dipancarkan sesudah link menjawab call `list` dari setiap store yang terdaftar di `ShellSync` (hari ini `instance.list` dan `operation.list`; store fitur 17 dan seterusnya menambah method nya ke daftar itu).
- `SecretsClient.fixture(_ scenario:)`: `SecretsClient(send:)` yang menjawab dari `replies` skenario, dan `secret.reveal` selalu `notSent`. Tidak ada nilai secret di fixture.

### Helper snapshot

API di `StacklyUITests/Support/Snapshot/` (internal ke target test):

```swift
func assertSnapshot<V: View>(
    _ view: V,
    named name: String,
    size: SnapshotSize = .page,
    variants: [SnapshotVariant] = SnapshotVariant.standard,   // light, dark, hcLight, hcDark, narrow, reduceTransparency
    systemMaterial: Bool = false,
    sourceLocation: SourceLocation = #_sourceLocation
) async throws
```

- Referensi: `<folder file suite>/__Snapshots__/<nama file suite tanpa .swift>/<name>.<akhiran varian>.png`, folder dari `sourceLocation.filePath`, jadi build tetap tidak boleh me-remap path (aturan `AGENTS.md`). Suite ada di `Tests/StacklyUITests/SnapshotTests/`, jadi `Package.swift` StacklyUI menambah `exclude: ["SnapshotTests/__Snapshots__"]` di `StacklyUITests`.
- `name` hanya huruf kecil ASCII, angka, dan `-`; nama lain membuat issue tanpa render.
- Semua suite snapshot bersarang di satu suite induk `@Suite(.serialized) struct SnapshotTests` (lewat `extension SnapshotTests`), jadi snapshot tidak pernah berjalan bersamaan satu sama lain. Setiap `@Test` nya memakai `.evidence(covers:expected:)` dengan ID dari *Trace PRD*.
- Render: `SnapshotWindow(contentRect:styleMask: [.borderless], backing: .buffered, defer: false)` dengan `appearance` `.aqua` atau `.darkAqua`, `NSHostingView(rootView: view.fixtureAppearance(varian).environment(...))`, lalu putaran `layoutSubtreeIfNeeded()`, tangkap, dan `Task.sleep(for: .milliseconds(50))` sampai stabil (AC-4), pola yang sama dengan `ShellLayoutTests` yang sudah ada.
- View yang membutuhkan model (Diagnostics) dirender dari `settledRuntime` yang sudah mencapai `expectedPresence`, bukan dari runtime yang masih memuat.
- Bandingkan: per piksel di konteks RGBA 8 bit sRGB (AC-6).
- Halaman galeri dirender tanpa `ScrollView` luarnya. *Seperti yang dibangun (2026-10-21):* halaman Tokens, Status, States, Confirmation, dan Components menyusun isinya di `GalleryPageLayout`, yang membungkus kolom halaman dengan `ScrollView` hanya saat kunci environment internal `galleryScrollsPage` bernilai true (default); suite snapshot mengisinya false, jadi halaman yang sama dirender di tinggi penuhnya tanpa view isi terpisah. Halaman Secrets memegang `ScrollView` nya sendiri dan tidak membaca kunci itu, walau suite snapshot tetap mengisinya false. Halaman yang lebih tinggi dari batas fitting 4000 pt (AC-4) dirender per kelompok bagian: States lewat `StatesGalleryPage(parts:)` dengan empat bagian (`shapes`, `live`, `placeholders`, `banners`), dan Components lewat empat kelompok (`BaseComponentsSections`, `SheetComponentsSections`, `InspectorComponentsSections`, `LogComponentsSections`) di dalam `GalleryPageLayout`, masing masing satu nama snapshot (`states-<bagian>`, `components-<kelompok>`).
- Waktu tunggu render dan `settledRuntime` dihitung `ResponsiveTimer` di `StacklyUITests/Support/`: setiap putaran menambah selisih waktu sejak putaran sebelumnya, paling banyak sedikit di atas jeda antar putaran (`settledRuntime` memeriksa presence tiap 10 ms dengan batas 60 ms per putaran; render snapshot menangkap tiap 50 ms dengan batas 100 ms per putaran), jadi main actor yang ditahan test lain (misalnya run loop alert modal) dihitung sebagai satu putaran. Hasilnya waktu responsif yang tidak pernah lebih dari waktu dinding, dan batas di AC-3 dan AC-4 adalah batas waktu ini.

### Kunci environment baru di StacklyUI

Dua kunci kecil, supaya render bisa ditentukan tanpa menunggu waktu nyata. Keduanya ada di build Release dengan nilai nil, jadi perilaku produk tidak berubah:

- `EnvironmentValues.fixtureNow: Date?`: `StateView` dan `DaemonBanner` memakai `fixtureNow ?? context.date` dari `TimelineView(.everyMinute)` untuk teks "Showing the last known state from …". View lain yang menampilkan waktu relatif terhadap sekarang wajib memakai pola yang sama.
- `EnvironmentValues.loadingDelay: Duration?`: `StateView` membuat `DelayedProgress` dengan `loadingDelay ?? DelayedVisibility.defaultDelay`; fixture dan snapshot mengisi `.zero`, jadi spinner selalu tampil di state loading.
- Sampel galeri yang memakai `Date.now` (`StatesGalleryPage` `lastKnown` dan `catalogUpdated`) diganti `FixtureClock.minutesAgo(18)` dan `FixtureClock.minutesAgo(26 * 60)`, dan `ShellPreviewSamples` dipindah ke `Fixtures/` dengan nilai yang sama. Fixture selalu mengisi `lastLoadedAt`, jadi fallback `.now` di `StaleNote` tidak pernah tercapai di snapshot.

### UI test

- Target `StacklyAppUITests`: bundle ID `com.ojiepermana.stackly.uitests`, macOS 27.0, `arm64`, Swift 6, `SWIFT_DEFAULT_ACTOR_ISOLATION = MainActor`, signing ad hoc dan App Sandbox mati seperti app, `TEST_TARGET_NAME = Stackly`, tanpa entitlement. Target, referensi paket lokal `../../clients/swift`, dan Testables scheme ditulis tangan di `project.pbxproj` dan `Stackly.xcscheme` dengan meniru bentuk target app yang ada (grup sinkron), tanpa tool baru, lalu dicek dengan `xcodebuild -list` dan satu build.
- Dependency: product `StacklyXCTestSupport` dan product `StacklyUI` (untuk `FixtureScenario.ID`, `SidebarSection`, `AccessibilityID`, dan `CommandMap`, semuanya `public`).
- Kelas test `@MainActor final class <Area>UITests: XCTestCase`, satu file per area: `ShellUITests`, `GalleryUITests`, `MenuUITests`, `AuditUITests`. Setiap method test `throws` dan memanggil `try evidence(covers:expected:)` di baris pertama.
- Helper peluncuran: `launch(_ scenario: FixtureScenario.ID, appearance: .light|.dark) -> XCUIApplication` mengisi `launchEnvironment` (`STACKLY_UI_SCENARIO`, `STACKLY_UI_APPEARANCE`) dan `launchArguments` (`-ApplePersistenceIgnoreState YES`), lalu menunggu `sidebar.row.overview` paling lama 10 detik. Setiap tunggu elemen lain paling lama 5 detik; `daemon-connecting` menunggu banner informative paling lama 3 detik.
- Elemen dicari lewat `accessibilityIdentifier` berbentuk `<scope>.<part>[.<key>]` (segmen ASCII lowerCamelCase; `<key>` hanya kunci data stabil, tidak pernah copy UI) dari konstanta `AccessibilityID` di `StacklyUI/Accessibility/AccessibilityID.swift`. Spec ini memakai: `sidebar.row.<SidebarSection rawValue>`, `daemonFooter.status`, `daemonFooter.settings`, `daemonBanner.container`, `daemonBanner.title`, `daemonBanner.action.reconnect`, `daemonBanner.action.openDiagnostics`, `gallery.topic.<topic>`, `gallery.confirmation.trigger.<sample>`, `gallery.confirmation.result`, `gallery.states.showAll`, dan `gallery.states.daemonUnreachable`. `StatusLabel` dicari lewat labelnya. Item menu dicari lewat judul dari `CommandMap`, karena SwiftUI tidak meneruskan identifier ke item menu. Fitur 67 membuat file `AccessibilityID.swift` kalau ia dibangun lebih dulu; spec ini menambah scope di atas.
- Dialog konfirmasi di galeri adalah sheet `NSAlert` di jendela galeri (`XCUIElement` bertipe `.sheet`).
- Ukuran minimum: drag dari `window.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1)).withOffset(CGVector(dx: -2, dy: -2))` ke offset (-800, -600), lalu `window.frame` paling sedikit 900 × 600 pt dengan toleransi 1 pt.
- Screenshot: `XCTAttachment(screenshot: window.screenshot())` sesuai AC-18.
- Audit: `try app.performAccessibilityAudit(for: [.contrast, .elementDetection, .hitRegion, .sufficientElementDescription, .action, .parentChild]) { issue in AuditExceptions.matches(issue) }`.

### Helper evidence XCTest

- `clients/swift/Package.swift`: target internal baru `StacklyEvidenceCore` (Foundation dan `Synchronization` saja) berisi `Record.swift`, `Rules.swift`, `RunPlan.swift`, dan `RecordFile` yang dipindah dari `StacklyTestSupport` dengan akses `package`. `StacklyTestSupport` bergantung padanya tanpa perubahan API publik, jadi test yang memakai trait `.evidence` tidak terdampak. Target dan product baru `StacklyXCTestSupport` (`import XCTest`, tanpa Swift Testing) bergantung pada `StacklyEvidenceCore`. `StacklyClient` tidak berubah dan tetap tanpa dependency.
- `XCTestCase.evidence(covers:expected:file:line:) throws` memvalidasi argumen seperti trait (pola PRD ID; argumen salah mencatat `XCTFail` lalu melempar, jadi test yang memanggil `try evidence(...)` berhenti), menulis record `NOT_RUN`, lalu mendaftarkan test ke `XCTEvidenceObserver`. Observer dipasang malas ke `XCTestObservationCenter.shared` pada pemanggilan `evidence` pertama, satu kali per proses. Observer mencatat issue gagal pertama di `testCase(_:didRecord:)` dan menulis status final di `testCaseDidFinish(_:)` dari `testRun.totalFailureCount`, `testRun.unexpectedExceptionCount`, `testRun.hasBeenSkipped`, dan flag blocked, sesuai AC-10.
- Nama test: dari `name` XCTest (`-[<Kelas> <selector>]`), selector tanpa `:` akhir dan tanpa akhiran `AndReturnError` atau `WithCompletionHandler`, ditulis `<Kelas>/<method>()`. Modul dari `String(reflecting: type(of: self))`.
- Tanpa `STACKLY_EVIDENCE_DIR`, helper hanya memvalidasi argumen, sama seperti trait Swift. Variabel itu sampai ke proses runner (bukan ke app yang diuji) lewat awalan `TEST_RUNNER_` yang diisi `cargo xtask ui-test`; runner tidak di sandbox, jadi ia bisa menulis ke `target/evidence/`.

### Nilai asing dan batas yang tercatat

- Warna AppKit yang dijembatani (`Color(nsColor:)`) dan teks di baris `List` atau `Table` tidak ikut varian HC di snapshot. Varian HC membuktikan bagian yang dikendalikan kode kita (token, border, `Color.primary`, warna tint sistem SwiftUI); setting Increase Contrast asli tetap dicek manual.
- Sidebar `NavigationSplitView`, toolbar, dan glass tidak tertangkap `cacheDisplay`, jadi snapshot piksel hanya untuk isi halaman dan bagian shell yang bisa dirender sendiri (banner, footer). Bingkai jendela dilihat lewat screenshot XCUITest (AC-18).
- `XCUIElement` macOS tidak punya key equivalent dan `hasFocus`, jadi urutan fokus Tab, Space pada tombol yang difokus, dan duplikat shortcut tidak dicek XCUITest (duplikat milik fitur 69).
- Prominent button dan seleksi list tergambar dalam rupa jendela tidak aktif di snapshot, jadi accent dari System Settings tidak memengaruhi hasil.
- Audit `.contrast` atas sidebar dan toolbar glass bergantung pada wallpaper dan setting sistem. Issue dari teks sistem di atas glass yang memang tidak bisa diubah kode kita masuk `AuditExceptions.swift` dengan `acceptedBy` "spec 0068" dan alasan itu, hanya setelah lead menyetujui baris itu (lihat *Key invariants*).

### Aksesibilitas dan pasangan kontras

- Tidak ada tabel kontras kedua. Teks dan permukaan di layar mana pun hanya boleh memakai pasangan P1 sampai P4 tabel Pasangan kontras spec 0012, yang sudah dibuktikan `ContrastTests` di empat appearance. Layar yang butuh pasangan baru mengusulkannya sebagai baris baru di spec 0012 (lewat *Usulan update untuk spec lain* spec layar itu), supaya `ContrastTests` mencakupnya sebelum layar di merge.
- Audit `.contrast` di AC-17 menangkap teks yang digambar sistem atau kombinasi yang lolos dari aturan di atas. Issue yang memang diterima (misalnya header `Table` sistem di light, Consequences spec 0012) masuk `AuditExceptions.swift` dengan `acceptedBy` yang menyebut spec itu.

### API surface

| Permukaan | Bentuk | Input | Output | Error |
| --- | --- | --- | --- | --- |
| `FixtureScenario.all`, `FixtureScenario(_ id:)` | Swift, `#if DEBUG`, `public` | `FixtureScenario.ID` | skenario | tidak ada (katalog tetap) |
| `InstanceSummary.fixture(...)`, `OperationSummary.fixture(...)`, `ProviderSummary.fixture(...)` | Swift, `#if DEBUG` | nomor kasus, nama, field yang berbeda dari default | DTO | tidak ada |
| `ShellBootstrap.fixture(_:)`, `SecretsClient.fixture(_:)` | Swift, `#if DEBUG`, `public` | skenario | `ShellRuntime`, `SecretsClient` | tidak ada |
| `View.fixtureAppearance(_:)` | Swift, `#if DEBUG`, `public` | `FixtureAppearance` | view | tidak ada |
| `settledRuntime(_:timeout:)` | Swift, target test | skenario, batas waktu | `ShellRuntime` yang mencapai `expectedPresence` | issue "scenario did not settle" |
| `assertSnapshot(...)` | Swift, target test, `async throws` | view, nama, ukuran, varian | issue bila gagal | `did not settle`, ukuran beda, piksel beda, tinggi fitting tidak sah, referensi hilang, PNG terlalu besar, nama tidak sah; `BLOCKED` tanpa sesi GUI atau dengan setting sistem menyala |
| `XCTestCase.evidence(...)`, `XCTEvidence.actual`, `XCTEvidence.blocked` | Swift, `StacklyXCTestSupport` | covers, expected | record | argumen tidak sah dan kind native menjadi `XCTFail` |
| `STACKLY_UI_SCENARIO`, `STACKLY_UI_APPEARANCE` | env app Debug | nama skenario, `light` atau `dark` | app di atas fixture | nama tidak dikenal: exit 64 |
| `STACKLY_SNAPSHOT_RECORD` | env test | `1` | rekam ulang | diabaikan saat `STACKLY_EVIDENCE_DIR` terisi |
| `cargo xtask snapshot review [--base <rev>]` | xtask | rev git | `target/snapshot-review/index.html` | rev tidak ada: exit 1 dengan pesan git |
| `cargo xtask ui-test [--only <Kelas>[/<method>]]` | xtask | filter | exit code `xcodebuild`, xcresult, lampiran | bukan macOS atau `STACKLY_EVIDENCE_DIR` relatif: exit 1; Automation Mode tidak pasti aktif atau layar terkunci: exit 2 |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| fixture | semua nilai skenario | tabel *Nilai skenario*, *Kasus instance*, *Kasus operasi*, dan *Kasus provider* |
| fixture | ID | `FixtureID.uuid(kind:number:)`: kind 1 instance, 2 operasi, 3 runtime, 4 daemon; nomor dari kolom `#` |
| fixture | teks banner, footer, Diagnostics | `ShellPresentation` dan `ConnectionDiagnostics` spec 0013 dari model, bukan teks di fixture |
| snapshot | path referensi | `sourceLocation.filePath` suite, nama file suite, `name`, akhiran varian |
| snapshot | ukuran kanvas | `.page` 1044 × 748 pt (konstanta harness): jendela default 1280 × 800 (spec 0013) dikurangi sidebar ideal 236 pt dan 52 pt untuk toolbar; narrow 640 pt dari lebar konten minimum spec 0013; footer 236 pt dari sidebar ideal spec 0013 |
| snapshot | skala dan ruang warna | 1 dan sRGB, dikunci `SnapshotWindow` (K3) |
| snapshot | ambang piksel | selisih kanal lebih dari 2 dari 255, nol piksel (K5) |
| snapshot | stabil | paling sedikit 250 ms (lebih dari crossfade 0,2 detik spec 0012), dua tangkapan identik berjarak 50 ms, batas 3 detik; kedua batas dalam waktu responsif `ResponsiveTimer` |
| snapshot | locale, zona waktu, kalender | konstanta helper: `en_US`, UTC, gregorian (copy UI berbahasa Inggris dari mockup) |
| snapshot | mode rekam | `STACKLY_SNAPSHOT_RECORD`, dikalahkan `STACKLY_EVIDENCE_DIR` |
| snapshot | folder gagal | `<root repo>/target/snapshot-failures/`, root dari `SourceTree.repositoryRoot` |
| snapshot | sesi GUI dan setting sistem | `CGSessionCopyCurrentDictionary()` dan properti `accessibilityDisplayShould*` `NSWorkspace.shared` |
| UI test | skenario dan appearance | `launchEnvironment` yang diisi helper peluncuran dari `FixtureScenario.ID` |
| UI test | ekspektasi banner dan footer | tabel *Tampilan status daemon* spec 0013 (teks tetap di test, satu baris per presence) |
| UI test | ekspektasi menu | `CommandMap.entries` (`title`, `alternateTitle`, `menu`, `scope`, `requirements`, `status`) spec 0069 |
| UI test | daftar bagian sidebar dan halaman galeri | `SidebarSection.allCases` dan topik galeri, lewat `AccessibilityID` |
| `ui-test` | status Automation Mode dan layar terkunci | stdout `automationmodetool`; `CGSessionCopyCurrentDictionary()` |
| `ui-test` | folder run | `target/ui-tests/<waktu UTC yyyyMMdd-HHmmss>/` |
| evidence XCTest | `test.target`, `test.name` | modul dari `String(reflecting: type(of: self))`, nama dari `name` XCTest seperti di *Helper evidence XCTest* |
| evidence XCTest | folder run dan kind | `STACKLY_EVIDENCE_DIR` dan `STACKLY_EVIDENCE_KIND` di proses runner, dari awalan `TEST_RUNNER_` |

### Keputusan rekomendasi

Keputusan kecil yang diambil di spec ini (pilihan, alasan, dan runner up):

- Nama folder referensi `__Snapshots__` di samping file suite, karena konvensi itu dikenal dan mudah dikecualikan di `Package.swift`; runner up `Snapshots/` di root test.
- Kanvas `.page` 1044 × 748 pt, karena itu isi halaman di ukuran jendela default; runner up kanvas penuh 1280 × 800.
- Status daemon sebagai satu `DaemonStatusMatrix` (lima PNG), bukan satu snapshot per presence (sekitar 140 PNG), karena lebih mudah direview dan perubahan satu baris tetap terlihat; runner up snapshot per baris.
- Stabil lewat waktu minimum plus dua tangkapan identik, karena SwiftUI butuh beberapa putaran run loop, `.task`, dan transisi; runner up jeda tetap, yang bisa terlalu pendek di Mac yang sibuk.
- Setting sistem yang menyala membuat snapshot `BLOCKED`, bukan gagal, karena kontrol AppKit ikut setting asli dan hasilnya bukan regresi kode; runner up mengabaikannya, yang menghasilkan dinding kegagalan tanpa penjelasan.
- Batas PNG 1 MB, karena halaman galeri terpanjang di 1x jauh di bawah itu dan kanvas yang salah ukuran langsung terlihat; runner up tanpa batas.
- Mutasi di mode fixture dijawab `notSent` dengan `ipc.not_connected`, karena kode itu sudah ada di katalog client dan punya umpan balik sendiri; runner up kode baru `fixture.read_only`, yang menambah kontrak hanya untuk test.
- Exit code 64 untuk skenario tidak dikenal (`EX_USAGE`) dan 2 untuk Automation Mode atau layar, supaya keduanya beda dari kegagalan test; runner up exit 1.
- Urutan status XCTest sama dengan trait Swift (`BLOCKED` menang), supaya satu record berarti sama di kedua helper; runner up `FAIL` menang.
- UI test bergantung pada product `StacklyUI`, supaya nama skenario, identifier, dan `CommandMap` tidak disalin; runner up salinan string di target UI test.

### Key invariants

- Fixture hanya memakai DTO hasil generate; perubahan kontrak yang mematahkan fixture gagal compile, bukan diam diam.
- Mode fixture tidak pernah menyentuh socket, `stacklyd`, proses, Keychain, UserDefaults, state jendela, atau frame jendela milik pengguna.
- Setter environment bergaris bawah hanya ada di `FixtureAppearance.swift` (`#if DEBUG`) dan folder `Tests/`. Kode `Fixtures/` tidak masuk build Release; dua kunci environment (`fixtureNow`, `loadingDelay`) masuk dengan nilai nil.
- Snapshot tidak pernah menulis referensi selama run evidence, dan rekam ulang hanya menulis file yang pikselnya berubah.
- Penantian render adalah polling sampai stabil (paling sedikit 250 ms, dua tangkapan identik, batas 3 detik, keduanya waktu responsif), tidak pernah sleep tetap untuk menutupi hasil yang tidak deterministik. View yang tidak stabil dalam 3 detik membuat test gagal dengan artefak `actual` tangkapan terakhir, dan test tidak pernah diulang otomatis (tanpa retry di helper, di suite, maupun di `ui-test`). Test yang hanya lulus karena sleep tetap atau retry diperlakukan sebagai bug produk, sama seperti aturan program foundation.
- Setiap `struct <Nama>Page` yang tidak private di luar galeri punya suite snapshot.
- Fitur yang menambah DTO untuk kasus di *Kasus yang menunggu DTO*, atau menambah layar, halaman galeri, atau `CommandMap`, menambah kasus fixture, skenario, suite snapshot, dan XCUITest nya di commit yang sama. Kalau bagian itu di merge sebelum milestone terkait spec ini dibangun, fitur 68 yang menambah suite nya; kalau sesudahnya, fitur pemilik.
- Automation Mode dan izin lain yang diminta macOS untuk runner hanya diberikan user; tidak ada kode atau sesi Claude yang menjalankan perintah pengaktifannya.
- Screenshot XCUITest hanya untuk dilihat; yang dibandingkan piksel hanya snapshot in process.
- `AuditExceptions.swift` dimulai kosong. Setiap baris baru saat build diajukan ke lead satu per satu (elemen, jenis audit, alasan, spec yang menerimanya) dan baru ditulis setelah disetujui; developer tidak menambahkannya sendiri.

### Security model

Tidak ada peran pengguna, data sensitif, atau akses baru. Fixture hanya berisi nama dan nilai contoh mockup; field secret hanya berupa reference `local:instance/<id>/...` (spec 0011), tanpa nilai, dan `FixtureCredential` tidak dipakai karena tidak ada nilai secret yang perlu dikirim. Mode fixture hanya ada di build Debug dan dipicu variabel environment yang diisi UI test. XCUITest menutup app Stackly yang sedang berjalan dengan bundle ID yang sama sebelum meluncurkan app Debug (perilaku `XCUIApplication`); daemon dan instance tidak terpengaruh karena menutup app tidak menghentikannya (spec 0013 AC-15). Automation Mode adalah setting sistem milik user.

### Configuration required

Tidak ada env var produk baru. Variabel khusus test dan Debug:

- `STACKLY_UI_SCENARIO`: nama skenario untuk app Debug, diisi UI test.
- `STACKLY_UI_APPEARANCE`: `light` atau `dark` untuk app Debug, diisi UI test.
- `STACKLY_SNAPSHOT_RECORD`: `1` untuk merekam ulang referensi snapshot di laptop.
- `TEST_RUNNER_STACKLY_EVIDENCE_DIR`, `TEST_RUNNER_STACKLY_EVIDENCE_KIND`: diisi `cargo xtask ui-test` dari variabel evidence spec 0002.

Prasyarat manual sekali per Mac: user mengaktifkan Automation Mode (setting sistem dengan password admin, misalnya `automationmodetool enable-automationmode-without-authentication`) dan memberi izin yang diminta macOS saat UI test pertama berjalan. Sampai itu dilakukan, `cargo xtask ui-test` keluar dengan kode 2 dan langkah UI test di `verify.md` tercatat `BLOCKED` dengan reason "Automation Mode is disabled on this Mac" dan owner user. Lead mencatat langkah ini di `docs/manual.md` setelah spec di merge. Snapshot test butuh sesi login GUI dan keempat setting aksesibilitas sistem mati.

### Critical test scenarios

- Happy path: setiap skenario dan kasus memetakan ke label, tone, presence, dan banner yang benar, dan round trip `StacklyJSON` sama, memverifikasi **AC-1**, **AC-2**.
- Happy path: `daemon-offline` lewat `ShellBootstrap.fixture` dan `settledRuntime` menampilkan state terakhir `connected-mixed` dengan mutasi `.blocked`, tanpa socket, memverifikasi **AC-3**.
- Happy path: suite snapshot galeri, `DaemonStatusMatrix`, Diagnostics, dan `UnbuiltPage` lulus di kelima varian dan RT identik dengan light, memverifikasi **AC-5**, **AC-8**.
- Failure case: view yang diubah satu piksel gagal dengan gambar actual dan diff, dan artefak lama terhapus di run berikutnya; referensi hilang ditulis lalu gagal; di run evidence, referensi hilang gagal tanpa ditulis; render dua kali identik piksel; view yang terus berubah gagal "did not settle"; spinner `loadingDelay` nol tampil stabil; tinggi fitting nol gagal, memverifikasi **AC-4**, **AC-6**.
- Failure case: test penjaga setter bergaris bawah gagal kalau view tidak membaca HC atau RT; aturan sumber gagal untuk setter di luar `FixtureAppearance.swift`, file fixture tanpa `#if DEBUG`, variabel skenario di luar `AppDelegate`, dan halaman tanpa suite snapshot, tetapi tidak untuk `private struct UnbuiltScrollPage`, memverifikasi **AC-5**, **AC-7**.
- Failure case: helper evidence XCTest menulis `PASS`, `FAIL` (kegagalan, exception, dan kind native), `BLOCKED` (lewat `blocked`, juga setelah kegagalan), dan `NOT_RUN` (skip tanpa blocked), dengan identitas yang benar untuk method biasa, `throws`, dan `async`; aturan `evidence check` menolak `XCTSkip(`, `XCTSkipIf(`, dan `XCTSkipUnless(` langsung, memverifikasi **AC-10**, **AC-11**.
- Failure case: `ui-test` dengan Automation Mode mati, keluaran tool yang tidak dikenal, atau layar terkunci keluar 2 tanpa menjalankan `xcodebuild`; skenario tidak dikenal membuat app keluar 64 tanpa membuat link, memverifikasi **AC-12**, **AC-13**.
- Aksesibilitas: XCUITest shell, galeri, menu, dan audit lulus di light dan dark dengan daftar pengecualian kosong atau berisi baris yang disetujui, memverifikasi **AC-14** sampai **AC-18**.
- Review: `cargo xtask snapshot review` menampilkan file lama dan baru untuk PNG yang berubah sejak merge base, memverifikasi **AC-9**.
- Auth/permission: tidak berlaku; tidak ada peran pengguna.

### Cek manual di /check verify

Dicatat sebagai evidence manual (spec 0002) di Mac ini:

1. Automation Mode: `automationmodetool` menunjukkan aktif. Kalau mati, langkah 2 dan 3 `BLOCKED` dengan reason "Automation Mode is disabled on this Mac" dan owner user; jangan mengaktifkannya atas nama user.
2. `cargo xtask evidence run --kind fixture -- cargo xtask ui-test`: semua UI test lulus dan report berisi record XCTest (**AC-10**, **AC-13** sampai **AC-17**).
3. Buka lampiran di `target/ui-tests/<run>/attachments/`: sidebar, toolbar, glass, banner, dan footer di light dan dark tampil benar untuk setiap skenario daemon (**AC-18**).
4. `cargo xtask snapshot review --base main` setelah mengubah satu warna token: halaman review menampilkan gambar lama dan baru (**AC-9**).
5. Setting sistem asli (Increase Contrast, Reduce Transparency, Differentiate Without Color, Reduce Motion), urutan fokus Tab, Space pada Show All, dan suara VoiceOver tetap dicek dengan tangan seperti daftar *Cek di /check verify* spec 0012 dan *Cek manual* spec 0013, dikurangi butir yang kini dibuktikan AC-14 dan AC-15.
6. `cargo xtask build-app --release`, lalu `strings` pada setiap file di bundle `target/xcode/Build/Products/Release/Stackly.app`: nol string `FixtureScenario`, `STACKLY_UI_SCENARIO`, dan `fixtureAppearance` (**AC-1**, **AC-12**).

## Build plan

Pendekatan proyek adalah Journey, tapi fitur 68 adalah fondasi pengecekan tanpa journey pengguna sendiri. Jalur pertama yang dibangun lengkap adalah "developer layar merender halamannya di semua state dan tampilan dari fixture dan melihat perubahannya", karena itu yang dipakai setiap layar fase 02. Milestone 2 hanya mengubah `clients/swift` dan `xtask/src/evidence`, dan API publik `StacklyTestSupport` tidak berubah, jadi milestone 1 dan 2 bisa paralel; milestone 3 butuh keduanya.

**Milestone 1: Fixture, harness snapshot, dan suite dasar**

1. Test probe di `StacklyUITests`: `SnapshotWindow` dengan skala 1 dan sRGB, `cacheDisplay` menggambar `List`, `Table`, `TextField`, dan `ProgressView`, `.task` dan `onAppear` berjalan di jendela yang tidak pernah tampil, perubahan model `@Observable` setelah view terpasang ikut tergambar, dua render identik piksel, spinner stabil, dan test penjaga setter HC dan RT. Kalau salah satu gagal di Mac ini, berhenti dan kembali ke `/architect`. Memenuhi **AC-4**, **AC-5**.
2. `FixtureAppearance.swift`, kunci `fixtureNow` dan `loadingDelay` beserta pemakaiannya di `StateView` dan `DaemonBanner`, sampel galeri tanpa `Date.now`, dan aturan sumber setter dan `#if DEBUG`. Memenuhi **AC-5**, **AC-7**.
3. `Fixtures/`: `FixtureClock`, `FixtureID`, builder DTO, `FixtureScenario` dengan semua skenario dan kasus, `LinkHandle.fixture`, aliran sinyal, `ShellBootstrap.fixture`, `SecretsClient.fixture`, pemindahan `ShellPreviewSamples`, `settledRuntime`, dan test round trip, pemetaan status, presence, dan offline. Memenuhi **AC-1**, **AC-2**, **AC-3**.
4. Helper snapshot: render, stabil, bandingkan, rekam, folder gagal dan pembersihannya, batas PNG, `exclude` di `Package.swift`, aturan nama, `BLOCKED` tanpa sesi GUI atau dengan setting sistem menyala, suite induk `.serialized`, dan aturan sumber cakupan halaman, dengan test helper itu sendiri (piksel beda, referensi hilang, run evidence, tinggi fitting tidak sah). Memenuhi **AC-4**, **AC-6**, **AC-7**.
5. Isi halaman galeri sebagai view terpisah, `DaemonStatusMatrix`, lalu suite `GallerySnapshotTests`, `ShellSnapshotTests` (`DaemonStatusMatrix` dan `UnbuiltPage`), dan `DiagnosticsSnapshotTests`, dengan referensi yang direkam dan direview. Referensi yang berubah karena perbaikan dari audit langkah 14 direkam ulang di sana. Memenuhi **AC-8**. *Catatan 2026-10-21:* "isi halaman galeri sebagai view terpisah" dibangun sebagai kunci environment `galleryScrollsPage` di `GalleryPageLayout`, dengan States dan Components dirender per kelompok (*Helper snapshot*).
6. `cargo xtask snapshot review` dengan test xtask untuk merge base, daftar file dari `git diff --name-status --no-renames` dan file untracked, path cermin, dan escape HTML. Memenuhi **AC-9**.

**Milestone 2: Helper evidence XCTest** (paralel dengan milestone 1)

7. Pindahkan `Record`, `Rules`, `RunPlan`, dan `RecordFile` ke target `StacklyEvidenceCore` dengan akses `package`; semua test `StacklyTestSupport` tetap lulus tanpa perubahan perilaku. Memenuhi **AC-10**.
8. `StacklyXCTestSupport`: `evidence`, `XCTEvidence`, `XCTEvidenceObserver`, fungsi status dan nama yang murni, dan test XCTest di `swift test --package-path clients/swift`. Memenuhi **AC-10**.
9. Aturan pindaian baru di `cargo xtask evidence check` dengan test nya. Memenuhi **AC-11**.

**Milestone 3: XCUITest dan audit** (setelah milestone 1 dan 2)

10. Target `StacklyAppUITests`, referensi paket lokal `clients/swift`, Testables scheme `Stackly`, `AppDelegate.makeRuntime`, environment fixture dan jendela tidak restorable di `StacklyApp` Debug, helper peluncuran, dan scope `AccessibilityID` milik spec ini. Langkah ini juga memeriksa apakah spinner tanpa akhir (`daemon-connecting`, `daemon-refreshing`) membuat XCUITest menunggu idle sampai timeout; kalau ya, kedua skenario itu dicek hanya lewat snapshot dan test model, dan AC-14 mengecualikannya dengan catatan di design.md. Memenuhi **AC-12**.
11. `cargo xtask ui-test` dengan cek platform, Automation Mode, layar terkunci, penerusan `TEST_RUNNER_`, result bundle, dan ekspor lampiran, plus test xtask untuk penyusunan argumen, pembacaan keluaran `automationmodetool`, dan penerusan variabel. Memenuhi **AC-13**.
12. `ShellUITests` dan `GalleryUITests` dengan screenshot. Memenuhi **AC-14**, **AC-15**, **AC-18**.
13. `MenuUITests`, setelah `CommandMap` spec 0069 ada di `main` (atau ditulis build spec 0069 kalau ia di merge belakangan). Memenuhi **AC-16**, **AC-18**.
14. `AuditUITests` dan `AuditExceptions.swift` yang dimulai kosong; issue yang muncul diperbaiki di view, atau diajukan ke lead per baris (elemen, jenis, alasan, spec yang menerimanya) dan baru ditulis setelah disetujui. Memenuhi **AC-17**, **AC-18**.
15. Bagian "Testing views" di `apps/macos/design.md`. Memenuhi **AC-19**.

## Consequences

**Positive**:

- Setiap layar fase 02 bisa dirender di semua state dan tampilan tanpa `stacklyd`, dari data yang tidak bisa menyimpang dari kontrak tanpa gagal compile.
- Regresi warna, border Increase Contrast, permukaan yang menjadi tembus pandang, dan teks yang terpotong di 640 pt tertangkap `swift test` biasa, dan perubahannya terlihat sebagai PNG di git serta di halaman review.
- Label VoiceOver, konfirmasi Return dan Escape, banner shell per presence, menu, dan audit Apple teruji otomatis; bukti GUI tidak lagi hanya manual, dan record XCTest masuk report evidence yang sama.
- Tanpa dependency pihak ketiga: `StacklyClient` tetap nol dependency, resolve paket Xcode tetap tanpa jaringan, dan tidak ada lisensi baru.
- Snapshot tidak bergantung pada accent, wallpaper, atau layar yang terpasang.

**Negative / tradeoffs**:

- Setter bergaris bawah (`_colorSchemeContrast`, `_accessibilityReduceTransparency`) bukan API yang didokumentasikan Apple dan bisa hilang atau berubah di SDK berikutnya. Aturan sumber mengurungnya di `FixtureAppearance.swift` (`#if DEBUG`) dan folder `Tests/`, dan test penjaga gagal begitu setter berhenti bekerja; saat itu varian HC dan RT snapshot hilang sampai `/architect` memutuskan pengganti, dan cek manual menjadi satu satunya bukti.
- Varian HC hanya membuktikan bagian yang dikendalikan kode kita; warna AppKit yang dijembatani dan teks baris `List` dan `Table` tetap di warna normal di snapshot.
- Snapshot tidak menangkap sidebar, toolbar, dan glass; bingkai jendela hanya punya screenshot yang dilihat manusia.
- Pembanding persis berarti setiap update macOS atau Xcode yang mengubah render kontrol sistem menuntut rekam ulang semua referensi yang terdampak, dicek lewat `snapshot review`.
- Snapshot berhenti (`BLOCKED`) di Mac yang menyalakan setting aksesibilitas sistem; pengguna setting itu harus mematikannya sementara untuk menjalankan snapshot.
- Repo bertambah PNG (perkiraan awal beberapa MB di 1x, tumbuh per layar dan per rekam ulang). Kalau folder `__Snapshots__` melewati 100 MB, pertimbangkan Git LFS lewat `/architect`.
- XCUITest butuh Automation Mode, mengambil alih keyboard dan mouse selama berjalan, gagal kalau layar terkunci, dan menutup app Stackly yang sedang terbuka; ia dijalankan dengan tangan, tidak di pre commit. Audit kontras atas glass bergantung pada wallpaper.
- Fokus Tab, suara VoiceOver, dan setting sistem asli tetap manual.
- Kode Debug app bertambah satu jalur peluncuran (fixture) yang harus dijaga tidak bocor ke Release, dan `project.pbxproj` diedit tangan untuk target baru.
- Fitur 68 belum menutup AC-16 sampai `CommandMap` ada di `main`.

**Neutral**:

- Target baru: `StacklyAppUITests` (Xcode), `StacklyEvidenceCore` (internal) dan product `StacklyXCTestSupport` (`clients/swift`); referensi paket lokal kedua di `Stackly.xcodeproj`.
- `StacklyUI` mendapat folder `Fixtures/` dan dua kunci environment; `StacklyUITests` mendapat folder `SnapshotTests/` dengan `__Snapshots__/` di dalamnya.
- Dua subcommand xtask baru tanpa crate baru.
- UI test memakai XCTest sementara test lain memakai Swift Testing, karena `XCUIApplication` hanya berjalan di bundle UI test XCTest.

### Usulan update untuk spec lain

Ditulis di sini karena spec ini tidak boleh mengubah spec lain; lead menjadikannya item `architect (update)` atau `/sync`. Semuanya additive.

- **Spec 0001** (peta package dan modul, di luar baris tabel *Batas keputusan* yang diperbarui langsung): `clients/swift` mendapat target internal `StacklyEvidenceCore` dan product `StacklyXCTestSupport`; `Stackly.xcodeproj` mendapat target `StacklyAppUITests` dan referensi paket lokal `../../clients/swift`; arah dependency `StacklyAppUITests → StacklyUI, StacklyXCTestSupport`. Tidak ada dependency pihak ketiga. Diterapkan di spec 0001 (2026-10-21).
- **Spec 0002** (di luar kalimat baris 428 dan Follow-up baris 467 yang diperbarui langsung): baris tabel *Menjalankan test di Mac ini* "run fixture UI" `cargo xtask evidence run --kind fixture -- cargo xtask ui-test` dan "run fixture StacklyUI" `cargo xtask evidence run --kind fixture -- swift test --package-path apps/macos/Packages/StacklyUI`; baris identitas XCTest di tabel *Identitas test* (`target` nama modul bundle, `name` `<Kelas>/<method>()`); aturan pindaian AC-11 di *Aturan pindaian evidence check*; dan catatan bahwa `StacklyTestSupport` kini bergantung pada `StacklyEvidenceCore`. Diterapkan di spec 0002 (2026-10-21).
- **Spec 0012**: (a) Follow-up "`/architect` fitur 68: XCUITest untuk daftar Cek di /check verify, snapshot Increase Contrast dan Reduce Transparency yang mengatur appearance di host, dan pemakaian ulang tabel Pasangan kontras" dijawab spec ini: XCUITest AC-15 dan AC-17, snapshot HC dan RT AC-5 lewat setter environment (appearance HC AppKit terbukti tidak berpengaruh), dan tabel Pasangan kontras dipakai ulang lewat aturan *Aksesibilitas dan pasangan kontras*. (b) Kalimat "XCUITest untuk cek ini adalah urusan fitur 68" di *Cek di /check verify* menjadi rujukan ke AC-14, AC-15, dan AC-17 spec ini, dengan setting sistem asli dan fokus keyboard tetap manual. (c) *API surface* menambah `EnvironmentValues.fixtureNow` dan `loadingDelay` (nil di app nyata). (d) *Galeri desain* menambah aturan isi halaman sebagai view terpisah dan sampel tanpa `Date.now`. (e) Pasangan kontras baru dari layar journey selalu masuk tabel Pasangan kontras spec 0012 lewat usulan spec layar itu; spec ini tidak menambah pasangan. Butir (a) sudah tercentang di spec 0012 sebelum update ini; (b) sampai (e) ditulis di *Cek di /check verify*, *API surface*, *Galeri desain*, dan *Pasangan kontras*. Diterapkan di spec 0012 (2026-10-21).
- **Spec 0013**: (a) `AppDelegate` memilih runtime lewat `makeRuntime()` dan membaca `STACKLY_UI_SCENARIO` dan `STACKLY_UI_APPEARANCE` di Debug, `StacklyApp` Debug memasang environment fixture dan jendela tidak restorable saat skenario aktif, dan `ShellBootstrap.fixture` ditambah di samping `live` (spec ini, AC-12). (b) Consequences "Bukti perilaku GUI sampai fitur 68 masih manual" dan Follow-up "Fitur 68: XCUITest shell dan fixture tampilan dari DTO `StacklyClient`" dijawab AC-1 sampai AC-3 dan AC-14. (c) Butir *Cek manual di /check verify* 1, 2, 5 (bagian ukuran minimum), dan 8 kini juga dibuktikan AC-14 di atas fixture; cek dengan `stacklyd` asli tetap ada. (d) `ShellPreviewSamples` pindah ke `Fixtures/`. Diterapkan di spec 0013 (2026-10-21).
- **Spec 0014**: Consequences "Bukti GUI visual tetap manual sampai fitur 68" dijawab spec ini untuk isi halaman dan shell; bingkai jendela lewat screenshot. Diterapkan di spec 0014 (2026-10-21).
- **Spec 0016**: langkah 7 Build plan dan Follow-up "snapshot setiap langkah dan setiap baris tabel state, di light, dark, Increase Contrast, dan Reduce Transparency": suite `OnboardingSnapshotTests` per langkah dan per baris tabel state memakai `assertSnapshot` (lima varian plus RT), skenario `onboarding` ditambah bersama `MainWindowRoot`, dan XCUITest onboarding mengikuti AC-14. Yang di merge lebih dulu, spec 0016 atau milestone 1 spec ini, menentukan siapa yang menulis suite (lihat *Key invariants*). Diterapkan di spec 0016 (2026-10-21).
- **Spec 0017, 0020, 0021, 0022**: kalimat preview "light, dark, 640 pt, Increase Contrast" diganti suite `<Halaman>SnapshotTests` di atas harness ini, dan Follow-up spec 0022 "Fitur 68: snapshot untuk state kedua halaman" menjadi suite Overview dan Instances yang ditulis fitur 22. Store baru (misalnya `CatalogStore`) menambah method `list` nya ke daftar `.afterSnapshot`. Kasus fixture konflik port milik fitur 20 dan 21 (*Kasus yang menunggu DTO*). Diterapkan di spec 0017, 0020, 0021, dan 0022 (2026-10-21). Ditunda (teks AC-16 spec 0021): langkah 10 spec 0021 sudah menambah suite snapshot, teks AC tidak diubah.
- **Spec 0067**: `ComponentSamples` `#if DEBUG` dipakai preview, halaman galeri Components, dan snapshot spec ini; tabel state per komponen di galeri itu adalah cakupan snapshot komponen; scope `checklist` (bukan `preflight`) untuk komponen checklist; dan `AccessibilityID.swift` dibuat oleh yang lebih dulu dibangun. Disepakati dengan arc-f67-spec (2026-10-21). Ditunda: spec 0067 sedang dibangun di wave ini dan di luar lingkup update ini.
- **Spec 0069**: `CommandMap`, `CommandEntry`, `CommandScope`, dan `CommandRequirement` publik dan read only, dengan `alternateTitle` untuk judul yang berganti; cek duplikat key equivalent tetap di spec 0069; `MenuUITests` ditulis oleh yang di merge kedua. Disepakati dengan arc-f69-spec (2026-10-21). Ditunda: spec 0069 sedang dibangun di wave ini dan di luar lingkup update ini.

## Follow-up

- [ ] `/scope`: selaraskan kalimat Done when fitur 68 di `docs/scope/ui-macos.md` dengan kasus yang menunggu DTO dan dengan batas setting sistem (teks usulan ada di laporan arc-f68-spec ke lead).
- [ ] Lead: tambah langkah "aktifkan Automation Mode dan beri izin runner UI test" ke `docs/manual.md` setelah spec ini di merge.
- [x] `/architect` update spec 0001, 0002, 0012, 0013, 0014, 0016, 0017, 0020, 0021, dan 0022 dengan butir *Usulan update untuk spec lain*. Diterapkan di spec pemiliknya (2026-10-21); spec 0013 terakhir, lewat update gabungan wave 3.
- [ ] Fitur 16, 17, 18, 20, 21, 22, 29, 44, dan 56: tambah kasus fixture, skenario, suite snapshot, dan XCUITest layar masing masing di atas harness ini.
- [x] `/sync` setelah build: `cargo xtask snapshot review`, `cargo xtask ui-test`, dan `swift test --package-path apps/macos/Packages/StacklyUI` dengan mode rekam di baris Commands `AGENTS.md`; target `StacklyAppUITests`, folder `Fixtures/`, dan `SnapshotTests/__Snapshots__/` di `apps/macos/AGENTS.md`; product `StacklyXCTestSupport` di `clients/swift/AGENTS.md`. Diterapkan lewat `/sync` (2026-10-21).
- [ ] Ganti komentar `TestAppearance.environment` ("`colorSchemeContrast` cannot be set on `EnvironmentValues`") saat langkah 2, karena `_colorSchemeContrast` ternyata bisa diisi (rationale, *Bukti probe*).
- [ ] Kalau test penjaga setter gagal di SDK baru: `/architect` update spec ini untuk pengganti varian HC dan RT.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, bukti probe di Mac ini, dan hasil cross check ada di [rationale.md](rationale.md).
