# 0069. Menu aplikasi, perintah, dan shortcut keyboard

**Date**: 2026-10-21
**Status**: Proposed

## Summary

Spec ini menetapkan menu bar Stackly (Stackly, File, Edit, View, Instance, Window, Help), satu peta perintah (`CommandMap`) yang menjadi satu satunya sumber judul menu dan shortcut, dan cara setiap layar menyambungkan aksinya ke menu. Layar menerbitkan aksi yang sama dengan tombolnya (`StacklyAction` dari fitur 67) lewat focused scene value (nilai yang dibaca menu dari jendela yang sedang aktif), jadi perintah menu nonaktif tepat saat tombolnya nonaktif dan menjalankan jalur yang sama, termasuk dialog konfirmasi. Perintah shell hanya hidup saat jendela utama menjadi jendela aktif dan shell tampil, jadi otomatis mati selama onboarding, saat jendela Settings aktif, atau saat sheet terbuka. Peta juga mencadangkan shortcut untuk fitur yang belum dibangun, dan test memastikan tidak ada shortcut yang bentrok satu sama lain atau dengan shortcut sistem.

## Requirements

**User stories**:

- Sebagai pengguna keyboard, saya ingin membuat, menghentikan, dan memeriksa instance lewat menu dan shortcut tanpa mouse, supaya alur harian tetap cepat.
- Sebagai pengguna macOS, saya ingin menu dan shortcut Stackly mengikuti idiom macOS (⌘, untuk Settings, ⌘W, ⌘F, ⌘N) dan tidak menimpa shortcut sistem, supaya kebiasaan saya tetap berlaku.
- Sebagai pengguna yang daemon nya mati, saya ingin Reconnect di menu, dan perintah yang mengubah data nonaktif seperti tombolnya, beserta alasannya.
- Sebagai pengguna VoiceOver, saya ingin setiap perintah punya judul teks dan status nonaktif yang terbaca.
- Sebagai pengembang fitur journey, saya ingin satu tempat untuk mendaftarkan perintah dengan judul, menu, dan shortcut, supaya bentrok ketahuan oleh test, bukan oleh pengguna.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-027):

- **AC-1** (peta perintah): `CommandMap` di `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Commands/` (public, read only, `nonisolated`, `Sendable`) memberi tepat satu `CommandEntry` untuk setiap `CommandID` lewat `switch` yang lengkap, berisi menu, judul, judul alternatif, shortcut, `needsInput`, scope, requirements, fitur pemilik, status `built` atau `planned`, makna standar, dan alasan pengecualian, persis seperti tabel *Peta perintah*. Setiap item menu Stackly dibangun lewat `CommandItem(.id)` yang mengambil judul dan shortcut dari peta. Source rule test gagal kalau `.keyboardShortcut(` dengan argumen selain `.defaultAction`, `.cancelAction`, atau nilai dari `CommandMap`, atau `CommandGroup`, `CommandMenu`, atau `.commands`, muncul di luar folder `Commands/` dan `apps/macos/Stackly/StacklyApp.swift`; dan kalau sebuah `CommandID` berstatus `built` tidak dipakai tepat satu kali oleh `CommandItem` di `Commands/` (kecuali baris ber kind `system`: `showMainWindow` dipakai tepat satu kali oleh `.keyboardShortcut` scene di `StacklyApp.swift`), atau berstatus `planned` tetapi dipakai.
- **AC-2** (tidak bentrok): Test unit atas `CommandMap` dan `SystemShortcuts` gagal kalau dua baris (built maupun planned) punya shortcut yang sama setelah dinormalisasi; kalau sebuah baris memakai shortcut di tabel *Shortcut yang diblok*; kalau baris memakai shortcut di tabel *Shortcut bermakna standar* tanpa `standardMeaning` yang sama dan tanpa `exception` berisi alasan; kalau baris memakai shortcut di tabel *Shortcut format standar* tanpa `exception` yang berisi alasan; kalau baris memakai kunci penyuntingan teks atau modifier 🌐 (aturan D dan E); dan kalau akhiran "…" pada judul tidak sama dengan `needsInput`. Probe accessibility di langkah 1 dan di cek manual 1 membuktikan di macOS 27 bahwa seluruh menu bar app, termasuk item yang ditambah sistem dan SwiftUI, tidak punya dua item dengan key equivalent yang sama, dan setiap baris built tampil dengan shortcut persis peta.
- **AC-3** (struktur menu): Menu bar berisi menu dan item di tabel *Struktur menu*, dalam urutan dan kelompok itu: Stackly (About, Settings… ⌘,, Reconnect to stacklyd, Services, Hide, Quit), File (New Instance… ⌘N, Close ⌘W), Edit (item edit sistem, lalu submenu Find dengan Find… ⌘F), View (item sidebar ⌃⌘S dari `SidebarCommands`, judulnya milik sistem; Overview ⌘1 sampai Diagnostics ⌘7, Back ⌘[, Show Inspector atau Hide Inspector ⌥⌘I, Sort Instances By, item tampilan sistem), Instance, Window (item sistem, Stackly ⌘0, Design Gallery di build Debug), dan Help (Welcome to Stackly, lalu Stackly Help ⌘? setelah fitur 66). Grup bawaan `.newItem`, `.textEditing`, dan `.help` diganti, jadi tidak ada "New Window", submenu Spelling and Grammar, Substitutions, Transformations, Speech, atau item "Stackly Help" bawaan yang memunculkan "Help isn’t available". Baris berstatus planned tidak tampil, dan menu Instance tampil sejak baris `.instance` pertama berstatus built. Item tidak pernah disembunyikan karena state; yang tidak berlaku sedang nonaktif.
- **AC-4** (cakupan aktif): Perintah ber scope `.app` (Settings, Welcome to Stackly, Stackly Help, Window > Stackly, Quit) selalu bisa dipakai, juga saat jendela utama tertutup atau minimized, saat jendela lain aktif, dan saat onboarding tampil. Perintah ber scope `.shell` dan `.page` aktif hanya kalau `AppShell` menerbitkan `PublishedCommands` dengan `isWindowKey` true (dari `MainWindowKey.isKey` jendela itu: key window aplikasi adalah jendela utama itu sendiri, bukan sheet, dialog, Settings, Design Gallery, atau popover) (jadi tidak selama `MainWindowRoot` menampilkan onboarding); `.page(S)` juga butuh halaman yang tampil termasuk S. Saat sheet New Instance, sheet Enter Secret, atau dialog konfirmasi terbuka di jendela utama, ⌘1 sampai ⌘7, ⌘N, dan perintah Instance nonaktif.
- **AC-5** (nonaktif sama dengan tombol): Item ber scope `.shell` atau `.page` aktif hanya kalau penerbit yang tepat menerbitkan `StacklyAction` untuk `CommandID` itu dan `action.availability(gate: host.gate)` milik spec 0067 sama dengan `.enabled`, dengan `host.gate` adalah `MutationAvailability` di environment penerbit (dibaca `.actionHost()`); `ActionButton` dan menu di dalam jendela memakai fungsi yang sama atas action yang sama. Jadi perintah yang `mutates` nonaktif saat daemon offline atau sedang dimulai, perintah yang `isBusy` nonaktif selama operasinya berjalan, dan perintah dengan `disabledReason` (capability unsupported, state yang tidak cocok) nonaktif. Subtitle mengikuti aturan menu spec 0067: `.disabled(reason, showsReason: true)` menampilkan `reason`, `.busy` menampilkan "In progress", dan `.disabled(showsReason: false)` (gate) tanpa subtitle karena banner jendela sudah menjelaskannya; semuanya sesuai hasil cek menu macOS 27 di spec 0067 milestone 1 langkah 1 (kalau subtitle tidak tampil di menu utama, item tanpa subtitle, dan alasan tetap terlihat di halaman). Test unit `CommandItemState.resolve` mencakup setiap kombinasi scope, key window, penerbit, gate, busy, dan alasan, dan `run` yang menolak saat state berubah sebelum dijalankan (key window hilang atau action tidak lagi `.enabled`).
- **AC-6** (jalur yang sama): Memilih item menu, lewat klik atau shortcut, memanggil `host.run(action)` (yaitu `action.invoke(gate: host.gate, present: host.present)` spec 0067) dengan `ActionHost` yang diterbitkan bersama action, tidak pernah `perform` langsung. Aksi yang `role` nya membawa `ActionConfirmation` membuka dialog `.stacklyConfirmation` spec 0012 yang sama dengan tombolnya, sebagai sheet di jendela utama, dengan aturan Return dan Escape spec 0012 (Return tidak menjalankan aksi destruktif); aksi tanpa konfirmasi berjalan lewat jalur mutasi spec 0013 yang sama dengan tombolnya. Tidak ada aksi destruktif yang punya shortcut.
- **AC-7** (pindah bagian): View > Overview ⌘1, Instances ⌘2, Environments ⌘3, Catalog ⌘4, Runtimes ⌘5, Operations ⌘6, dan Diagnostics ⌘7 memilih bagian sidebar lewat `AppShell.select(_:)`, fungsi yang juga dipanggil setter binding pilihan sidebar, sehingga hasilnya sama dengan memilih baris sidebar: pilihan tersimpan di `@SceneStorage("shell.section")` (dan tercermin ke `ShellNavigator` kalau sudah ada). Memilih bagian yang sedang tampil masuk ke cabang pilih ulang `select(_:)`, yang memanggil hook `onReselect(section)`; spec 0021 mengisi hook itu (Instances kembali ke tabel) dan membuat ketukan pada baris sidebar yang sudah terpilih juga memanggil `select(_:)`, karena `List(selection:)` tidak memanggil setter untuk nilai yang sama. Sebelum fitur 21, pilih ulang tidak mengubah apa pun. Perintah ini tidak mengubah data dan tidak terkena gate mutasi.
- **AC-8** (Reconnect): Stackly > "Reconnect to stacklyd" (tanpa shortcut) memanggil `DaemonModel.reconnect()` dan aktif tepat saat tombol Reconnect di seksi Connection Diagnostics aktif: diterbitkan `AppShell` hanya selama `presentation.hasConnection` false, nonaktif dengan alasan "Relaunch Stackly after fixing the problem." selama misconfigured, dan nonaktif (busy) selama `presentation.isReconnecting`. Selama terhubung item nonaktif tanpa subtitle.
- **AC-9** (New Instance): File > "New Instance…" ⌘N menjalankan `newInstanceAction` (spec 0020 dan 0022) lewat `StacklyAction` yang diterbitkan `AppShell` dengan `mutates` true, jadi nonaktif saat `newInstanceAction` nil, saat gate mutasi blocked, saat onboarding tampil, dan saat jendela utama bukan key window. `CommandGroup` sementara bertanda `// provisional: F69` dari spec 0020 dihapus.
- **AC-10** (Find): Edit > Find > "Find…" ⌘F memfokuskan field `.searchable` halaman yang tampil lewat `.searchFocused`, untuk setiap halaman yang menerbitkan `find` (Instances, Catalog, dan halaman lain yang punya pencarian), dan nonaktif di halaman tanpa pencarian dan selama detail instance terbuka (field pencarian tidak ada di toolbar detail). Kalau probe P3b menunjukkan Find bawaan sudah memenuhi kalimat ini, grup text editing bawaan dipertahankan (lihat *Cadangan probe*). Menekan ⌘F saat field sudah fokus tidak mengubah teksnya.
- **AC-11** (perintah instance): Menu Instance berisi Open ⌘O, lalu Start ⌘R, Stop ⇧⌘R, Restart ⌥⌘R, dan Reload, lalu Copy Connection String ⇧⌘C, lalu Duplicate… ⌘D dan Remove…, masing masing tampil sejak fitur pemiliknya membangunnya. Targetnya adalah instance yang halaman detailnya terbuka di Instances, selain itu baris terpilih tabel Instances; satu satunya penerbit adalah halaman Instances. Start diterbitkan saat aksi utama `InstanceAction.resolve` (spec 0022) adalah Start atau Retry, atau sibuk memulai; Stop saat aksi utama Stop atau sibuk menghentikan; keduanya membawa alasan dan status sibuk yang sama dengan tombol aksi utama. Copy Connection String aktif tepat saat item More "Copy Connection String" di detail (spec 0021) aktif, dan memakai jalur reveal dan pasteboard yang sama. Open aktif saat ada baris terpilih dan detail belum terbuka. Tanpa pilihan, atau di halaman selain Instances, semua item Instance nonaktif tanpa subtitle.
- **AC-12** (perintah halaman Instances di View): View > Back ⌘[ kembali dari detail ke tabel (aktif hanya saat detail terbuka); View > Show Inspector atau Hide Inspector ⌥⌘I membalik binding inspector yang sama dengan tombol toolbar (`.stacklyInspector(isPresented:)` spec 0067), dengan judul mengikuti state; View > Sort Instances By berisi Name, Provider, Version, Endpoints, Status, lalu Ascending dan Descending sebagai item bertanda centang untuk urutan yang aktif, memakai urutan tabel yang sama. Ketiganya menggantikan `InstancesCommands` spec 0022 dan aktif hanya saat Instances tampil di jendela utama yang key.
- **AC-13** (Help, Welcome, jendela): Help > "Welcome to Stackly" (tanpa shortcut, scope `.app`) memanggil `OnboardingModel.present()` lalu `WindowOpener.openMain()` (spec 0016) dan menggantikan `CommandGroup(after: .help)` sementara. Tidak ada item "Stackly Help" sampai fitur 66 membangun baris `stacklyHelp` (⌘?, sudah dicadangkan); kolom pencarian Help milik sistem tetap ada. Window > Stackly ⌘0 membuka jendela utama, membawanya ke depan, atau mengembalikannya dari Dock kalau minimized, juga saat onboarding dan setelah jendela ditutup. Settings ⌘, tetap terbuka selama onboarding. Close ⌘W menutup key window; menutup jendela utama tetap memunculkan notice spec 0013.
- **AC-14** (bantuan di tombol): Tooltip tombol toolbar yang punya padanan perintah ber shortcut memakai `CommandMap.helpText(for:)` ("{judul} ({shortcut})", misalnya "Hide Inspector (⌥⌘I)"), dan menu di dalam jendela (More, menu konteks) memakai judul dari peta tanpa memasang atau menampilkan shortcut.
- **AC-15** (PRD AC-027, REQ-027, NFR-012): Alur membuat, menghentikan, dan memeriksa instance bisa diselesaikan hanya dengan keyboard lewat menu dan shortcut (⌘N lalu isi sheet dan Return; ⌘2, Tab ke tabel, panah, ⇧⌘R; ⌘O dan ⌘[), dan dengan VoiceOver setiap item menu dibacakan dengan judul teks, shortcut, dan status nonaktifnya (beserta alasan bila subtitle tampil). Dibuktikan sebagai evidence manual setelah fitur 20, 21, dan 22 dibangun; sebelum itu `NOT_RUN`.
- **AC-16** (dokumentasi dan pemindahan): `apps/macos/design.md` mendapat bagian "Commands and shortcuts" yang memuat tabel peta (built dan planned), tabel shortcut sistem, aturan *Menambah perintah*, dan hasil probe langkah 1 beserta versi macOS dan Xcode. Setelah milestone 2 tidak ada `CommandGroup` atau `.commands` di luar `Commands/` dan `StacklyApp.swift`, dan `grep -rn "provisional: F69" apps/` kosong.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan K1 sampai K13 di [rationale.md](rationale.md), ditambah semua perbaikan cross check G1 sampai G11 (tabel *Cross check* di rationale), disetujui lead atas delegasi user (2026-10-21): satu `CommandMap` data murni dengan baris planned yang mencadangkan shortcut; layar menerbitkan `StacklyAction` milik fitur 67 lewat focused scene value; perintah shell dan halaman hanya aktif saat jendela utama adalah key window dan shell tampil; menu objek baru "Instance" di antara View dan Window; ⌘1 sampai ⌘7 untuk bagian; keluarga R untuk lifecycle (Start ⌘R, Stop ⇧⌘R, Restart ⌥⌘R); Reconnect di menu Stackly; Find milik Stackly menggantikan grup text editing; Help tanpa item dokumentasi sampai fitur 66; ⌘0 untuk jendela utama; alasan nonaktif sebagai subtitle; cek bentrok lewat test peta, source rule, dan probe accessibility, dengan XCUITest menu berulang milik fitur 68; dan fitur yang sudah di merge dipindah ke peta oleh build ini.

Menu adalah jalan kedua ke aksi yang sama dengan tombol: tidak ada perintah yang punya logika aktif atau jalur eksekusi sendiri, dan shortcut hanya ada di satu tabel yang dites.

**Implementation skills**: `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya: `focusedSceneValue` tidak cukup sendirian untuk menentukan aktif (sheet tetap bagian dari scene yang sama), jadi state key window dari `MainWindowKey` jendela itu ikut diterbitkan dan menentukan.

## Feature design

### Trace PRD

| ID PRD | Dipenuhi oleh |
| --- | --- |
| REQ-027 (keyboard navigation, VoiceOver labels, konfirmasi destructive) | AC-3 sampai AC-15 |
| AC-027 (buat, hentikan, periksa instance tanpa info yang hanya lewat warna atau hover) | AC-11, AC-15 |
| NFR-012 (critical journeys lulus review keyboard dan VoiceOver) | AC-5, AC-15 |

### Batas dengan spec lain

| Bagian | Pemilik |
| --- | --- |
| `StacklyAction` (id, title, symbol, role `ActionRole` dengan `.standard(confirmation:)` atau `.destructive(ActionConfirmation)`, mutates, disabledReason, isBusy, perform), `ActionAvailability` (`.enabled`, `.busy`, `.disabled(reason:showsReason:)`), `availability(gate:)`, `invoke(gate:present:)`, `ActionHost` (`gate`, `present`, `run(_:)`), `EnvironmentValues.actionHost`, `.actionHost()` di `AppShell` dan `StacklySheet`, `ActionButton`, `ActionMenu`, `ActionMenuItems`, cek subtitle menu di macOS 27 | spec 0067 milestone 1 |
| `.stacklyInspector(isPresented:)` dan `InspectorToggle` | spec 0067 milestone 3 |
| XCUITest menu berulang: setiap baris built ada di menu yang benar dengan judul yang sama, aktif atau nonaktif per skenario fixture (`connected-empty`, `connected-mixed`, `daemon-offline`, `onboarding`), dan smoke shortcut lewat `typeKey` | spec 0068 |
| `CommandMap`, `SystemShortcuts`, `StacklyCommands`, `PublishedCommands`, `.publishesCommands`, `CommandItem`, `CommandItemState`, `MainWindowKey`, test peta, source rule, probe accessibility, bagian "Commands and shortcuts" di `design.md` | spec ini |
| Scene, `AppShell`, `ShellRuntime`, `DaemonModel.reconnect()`, `ShellPresentation`, `MutationAvailability`, `.stacklyConfirmation` | spec 0013 dan 0012 (dipakai apa adanya, tambahan di *Usulan update untuk spec lain*) |
| `MainWindowRoot`, `OnboardingModel.present()` | spec 0016 |
| `newInstanceAction` dan sheet New Instance | spec 0020 |
| Halaman detail, Open, Copy Connection String, `instanceRequest`, aturan klik ulang sidebar | spec 0021 |
| `InstanceActionModel`, `InstanceAction.resolve`, `ShellNavigator`, urutan tabel, state inspector | spec 0022 |
| Pencarian Catalog | spec 0017 |
| Restart dan Reload (26), Duplicate (24), Remove (54), Stackly Help (66): perintahnya sendiri, memakai baris planned di peta | fitur pemilik |
| Popover menu bar, Stop All Services, isi Settings | fitur 32 |

### Struktur menu

Urutan item sistem (About, Services, Hide, Undo sampai Select All, AutoFill, Dictation, Emoji and Symbols, Full Screen, Minimize, Zoom, ubin jendela, Bring All to Front, daftar jendela, kolom pencarian Help) ditentukan macOS dan SwiftUI; probe langkah 1 mencatat urutan nyatanya di `design.md`. Tabel ini menetapkan tempat item Stackly. "(pemisah)" adalah garis pemisah menu.

| Menu | Isi (judul, shortcut, penempatan SwiftUI) |
| --- | --- |
| Stackly | About Stackly · (pemisah) · Settings… ⌘, (scene `Settings`) · (pemisah) · Reconnect to stacklyd (`CommandGroup(after: .appSettings)`) · (pemisah) · Services · (pemisah) · Hide Stackly ⌘H · Hide Others ⌥⌘H · Show All · (pemisah) · Quit Stackly ⌘Q |
| File | New Instance… ⌘N (`CommandGroup(replacing: .newItem)`) · (pemisah) · Close ⌘W |
| Edit | Undo ⌘Z · Redo ⇧⌘Z · (pemisah) · Cut ⌘X · Copy ⌘C · Paste ⌘V · Paste and Match Style ⌥⇧⌘V · Delete · Select All ⌘A · (pemisah) · Find ▸ Find… ⌘F (`CommandGroup(replacing: .textEditing)` berisi `Menu("Find")`) · item yang ditambah sistem |
| View | item sidebar ⌃⌘S dari `SidebarCommands()` (judul milik sistem) · (pemisah) · Overview ⌘1 · Instances ⌘2 · Environments ⌘3 · Catalog ⌘4 · Runtimes ⌘5 · Operations ⌘6 · Diagnostics ⌘7 · (pemisah) · Back ⌘[ · (pemisah) · Show Inspector atau Hide Inspector ⌥⌘I · Sort Instances By ▸ (Name, Provider, Version, Endpoints, Status, (pemisah), Ascending, Descending) (semua lewat `CommandGroup(after: .sidebar)`) · (pemisah) · item toolbar dan Full Screen milik sistem |
| Instance | Open ⌘O · (pemisah) · Start ⌘R · Stop ⇧⌘R · Restart ⌥⌘R · Reload · (pemisah) · Copy Connection String ⇧⌘C · (pemisah) · Duplicate… ⌘D · Remove… (`CommandMenu("Instance")`, SwiftUI menaruhnya di antara View dan Window) |
| Window | Minimize ⌘M · Zoom · item ubin sistem · (pemisah) · Stackly ⌘0 (item otomatis scene `Window`, shortcut lewat `.keyboardShortcut` scene) · Design Gallery (Debug) · (pemisah) · Bring All to Front · daftar jendela |
| Help | kolom pencarian sistem · Stackly Help ⌘? (fitur 66, sebelum itu tidak ada) · (pemisah) · Welcome to Stackly (`CommandGroup(replacing: .help)`) |

Pemisah di sekitar item yang belum dibangun tidak dirender dua kali: builder merender dari `CommandMap.layout(menu:submenu:)` dan menaruh `Divider()` hanya di antara kelompok yang punya paling sedikit satu item built (kolom Kel di *Peta perintah*).

### Peta perintah

Status di kolom terakhir adalah status yang ditulis langkah 2. Baris milik fitur yang sudah di merge saat milestone 2 atau 3 berjalan menjadi built di langkah 4 atau 6; baris planned menjadi built oleh build fitur pemiliknya. Urutan baris di tabel adalah urutan deklarasi `CommandID`, dan urutan item di dalam satu kelompok mengikuti urutan itu. Kolom Req: `D` daemon (action `mutates`), `S` selection (instance terpilih atau detail terbuka), `T` detail (detail terbuka), `F` search (halaman punya pencarian), `N` noConnection. Kolom Kel adalah `group`: nomor kelompok di dalam menu atau submenu, dipisah garis pemisah.

| `CommandID` | Menu (submenu) | Kel | Jenis | Judul | Shortcut | `needsInput` | Scope | Req | Makna standar atau pengecualian | Pemilik | Status awal |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `reconnect` | app | 1 | button | Reconnect to stacklyd | | tidak | `.shell` | N | | 69 | built (langkah 4) |
| `newInstance` | file | 1 | button | New Instance… | ⌘N | ya | `.shell` | D | makna `new` | 20 | planned |
| `find` | edit (`find`) | 1 | button | Find… | ⌘F | ya | `.page(semua bagian)` | F | makna `find` | 69 | built (langkah 4) |
| `showOverview` | view | 1 | button | Overview | ⌘1 | tidak | `.shell` | | | 69 | built (langkah 4) |
| `showInstances` | view | 1 | button | Instances | ⌘2 | tidak | `.shell` | | | 69 | built (langkah 4) |
| `showEnvironments` | view | 1 | button | Environments | ⌘3 | tidak | `.shell` | | | 69 | built (langkah 4) |
| `showCatalog` | view | 1 | button | Catalog | ⌘4 | tidak | `.shell` | | | 69 | built (langkah 4) |
| `showRuntimes` | view | 1 | button | Runtimes | ⌘5 | tidak | `.shell` | | | 69 | built (langkah 4) |
| `showOperations` | view | 1 | button | Operations | ⌘6 | tidak | `.shell` | | | 69 | built (langkah 4) |
| `showDiagnostics` | view | 1 | button | Diagnostics | ⌘7 | tidak | `.shell` | | | 69 | built (langkah 4) |
| `back` | view | 2 | button | Back | ⌘[ | tidak | `.page([.instances])` | T | makna `back` | 21 | planned |
| `toggleInspector` | view | 3 | button | Show Inspector (alternatif: Hide Inspector) | ⌥⌘I | tidak | `.page([.instances])` | | | 22 | planned |
| `sortInstancesByName` | view (`sortInstancesBy`) | 1 | toggle | Name | | tidak | `.page([.instances])` | | | 22 | planned |
| `sortInstancesByProvider` | view (`sortInstancesBy`) | 1 | toggle | Provider | | tidak | `.page([.instances])` | | | 22 | planned |
| `sortInstancesByVersion` | view (`sortInstancesBy`) | 1 | toggle | Version | | tidak | `.page([.instances])` | | | 22 | planned |
| `sortInstancesByEndpoints` | view (`sortInstancesBy`) | 1 | toggle | Endpoints | | tidak | `.page([.instances])` | | | 22 | planned |
| `sortInstancesByStatus` | view (`sortInstancesBy`) | 1 | toggle | Status | | tidak | `.page([.instances])` | | | 22 | planned |
| `sortInstancesAscending` | view (`sortInstancesBy`) | 2 | toggle | Ascending | | tidak | `.page([.instances])` | | | 22 | planned |
| `sortInstancesDescending` | view (`sortInstancesBy`) | 2 | toggle | Descending | | tidak | `.page([.instances])` | | | 22 | planned |
| `openInstance` | instance | 1 | button | Open | ⌘O | tidak | `.page([.instances])` | S | makna `open` | 21 | planned |
| `startInstance` | instance | 2 | button | Start | ⌘R | tidak | `.page([.instances])` | S, D | pengecualian: ⌘R adalah "Show Ruler" di menu Format; Stackly tidak punya menu Format, ruler, atau teks bergaya | 22 | planned |
| `stopInstance` | instance | 2 | button | Stop | ⇧⌘R | tidak | `.page([.instances])` | S, D | | 22 | planned |
| `restartInstance` | instance | 2 | button | Restart | ⌥⌘R | tidak | `.page([.instances])` | S, D | | 26 | planned |
| `reloadInstance` | instance | 2 | button | Reload | | tidak | `.page([.instances])` | S, D | | 26 | planned |
| `copyConnectionString` | instance | 3 | button | Copy Connection String | ⇧⌘C | tidak | `.page([.instances])` | S | pengecualian: ⇧⌘C adalah "Show Colors" di menu Format; Stackly tidak punya menu Format atau panel warna | 21 | planned |
| `duplicateInstance` | instance | 4 | button | Duplicate… | ⌘D | ya | `.page([.instances])` | S, D | makna `duplicate` | 24 | planned |
| `removeInstance` | instance | 4 | button | Remove… | | ya | `.page([.instances])` | S, D | | 54 | planned |
| `stacklyHelp` | help | 1 | button | Stackly Help | ⌘? (⇧⌘/) | tidak | `.app` | | makna `help` | 66 | planned |
| `welcome` | help | 2 | button | Welcome to Stackly | | tidak | `.app` | | | 16 | planned |
| `showMainWindow` | window | | system | Stackly | ⌘0 | tidak | `.app` | | pengecualian: ⌘0 adalah "Actual Size" (tabel B); Stackly tidak punya zoom, dan Mail memakai ⌘0 untuk jendela utamanya | 69 | built (langkah 4) |

- Jenis (`kind`): `button` dirender sebagai `Button`, `toggle` sebagai `Toggle` menu dengan tanda centang dari `PublishedCommands.checked`, dan `system` adalah item yang dibuat SwiftUI atau macOS sendiri, misalnya item scene `Window` (peta hanya memegang shortcut nya dan test bentrok tetap memeriksanya). Jenis ditentukan peta, tidak pernah oleh ada atau tidaknya penerbit.
- Submenu (`submenu`): `find` adalah Edit > Find, dan `sortInstancesBy` adalah View > Sort Instances By, yang duduk di kelompok 3 View bersama `toggleInspector`.
- `CommandMap.layout(menu:submenu:)` memberi baris built per kelompok, urut kelompok lalu urut deklarasi, tanpa kelompok kosong. Builder menu merender dari fungsi ini dan menaruh `Divider()` hanya di antara dua kelompok yang ada, jadi pemisah tidak pernah ganda atau menggantung. Test peta memeriksa hasilnya untuk status awal dan untuk peta dengan semua baris built.
- `Requirements` di peta bersifat deskriptif: ia menjelaskan kapan penerbit mengirim action dan menjadi ekspektasi test fitur 68. Yang menentukan aktif di runtime tetap `CommandItemState.resolve` atas action yang diterbitkan. Test penerbit (langkah 4 dan 6) memastikan setiap baris dengan `D` diterbitkan dengan `mutates` true, dan baris tanpa `D` dengan `mutates` false.
- `Find…` memakai "…" karena perintah itu meminta masukan (aturan HIG untuk ellipsis), bukan karena membuka jendela. "Welcome to Stackly" dan "Show Inspector" tidak meminta masukan.
- Baris yang membuka konfirmasi (Remove…) tidak punya shortcut, sesuai aturan *Menambah perintah* butir 3.

### Shortcut sistem yang dicadangkan

`SystemShortcuts` di `Commands/SystemShortcuts.swift` memuat tabel A sampai C sebagai data (`SystemShortcut { shortcut, title, source, meaning }`) dan aturan D dan E sebagai fungsi. Sumber: tabel shortcut standar di Apple Human Interface Guidelines bagian Keyboards (HIG), daftar shortcut Mac di Apple Support "Mac keyboard shortcuts" (macOS), key binding standar AppKit untuk field teks (AppKit), dan perintah bawaan SwiftUI (SwiftUI). Daftar ini disusun dari pengetahuan saat spec ditulis; probe langkah 1 membandingkannya dengan menu nyata macOS 27, dan item sistem yang muncul di probe tetapi belum ada di tabel A ditambahkan di langkah itu (dicatat di `design.md`).

**A. Shortcut yang diblok** (tidak boleh dipakai baris mana pun):

| Shortcut | Arti | Sumber |
| --- | --- | --- |
| ⌘Q, ⌘H, ⌥⌘H, ⌘, | Quit, Hide, Hide Others, Settings | HIG, macOS |
| ⌘. | Cancel (dialog, operasi, field) | HIG, AppKit |
| ⌘W, ⌥⌘W, ⌘M, ⌥⌘M | Close, Close All, Minimize, Minimize All | HIG, macOS |
| ⌘\`, ⇧⌘\`, ⌘Tab, ⇧⌘Tab | jendela berikutnya dan sebelumnya, pengalih app | macOS |
| ⌘Space, ⌥⌘Space, ⌃Space, ⌃⌥Space | Spotlight, jendela pencarian Finder, sumber input | macOS |
| ⇧⌘3, ⇧⌘4, ⇧⌘5, ⇧⌘6 | screenshot | macOS |
| ⌥⌘Esc, ⌃⌘Q, ⇧⌘Q, ⌥⇧⌘Q | Force Quit, Lock Screen, Log Out | macOS |
| ⌥⌘D, ⌃⌘D, ⌃⌘Space | sembunyikan Dock, Look Up, Emoji and Symbols | macOS |
| ⌃⌘F, ⌃⌘S, ⌥⌘T | Full Screen, Show Sidebar, Show Toolbar | HIG, SwiftUI (`SidebarCommands`) |
| ⌘Z, ⇧⌘Z, ⌘X, ⌘C, ⌘V, ⌥⇧⌘V, ⌘A | Undo, Redo, Cut, Copy, Paste, Paste and Match Style, Select All | HIG, AppKit |
| ⌘;, ⌘: | Check Spelling, Spelling and Grammar | HIG |
| ⌃F1 sampai ⌃F8, ⌃↑, ⌃↓, ⌃←, ⌃→ | fokus keyboard (menu bar, Dock, dan lainnya), Mission Control, Spaces | macOS |

**B. Shortcut bermakna standar** (boleh untuk baris yang `standardMeaning` nya sama; untuk makna lain hanya dengan `exception` berisi alasan):

| Shortcut | Makna (`StandardMeaning`) | Sumber | Dipakai |
| --- | --- | --- | --- |
| ⌘N | `new` | HIG | `newInstance` |
| ⌘O | `open` | HIG | `openInstance` |
| ⌘D | `duplicate` | HIG, Finder | `duplicateInstance` |
| ⌘F | `find` | HIG | `find` |
| ⌘G, ⇧⌘G | `findNext`, `findPrevious` | HIG | belum (untuk fitur 27 bila log punya pencarian) |
| ⌘E, ⌘J | `useSelectionForFind`, `jumpToSelection` | HIG | belum |
| ⌘[, ⌘] | `back`, `forward` | Finder, Safari | `back` |
| ⌘? (⇧⌘/) | `help` | HIG | `stacklyHelp` |
| ⌘S, ⇧⌘S, ⌘P, ⇧⌘P | `save`, `saveAs`, `print`, `pageSetup` | HIG | belum |
| ⌘+, ⌘- | `zoomIn`, `zoomOut` | HIG | belum |
| ⌘0 | `actualSize` | HIG | `showMainWindow` lewat pengecualian (makna lain) |

**C. Shortcut format standar** (milik menu Format yang tidak pernah ada di Stackly; boleh hanya dengan `exception` berisi alasan):

| Shortcut | Arti | Sumber | Pengecualian di peta |
| --- | --- | --- | --- |
| ⌘B, ⌘I, ⌘U | Bold, Italic, Underline | HIG | tidak ada |
| ⌘T | Show Fonts | HIG | tidak ada |
| ⇧⌘C | Show Colors | HIG | `copyConnectionString` |
| ⌘R | Show Ruler | HIG | `startInstance` |
| ⌥⌘C, ⌥⌘V | Copy Style, Paste Style | HIG | tidak ada |
| ⌘{, ⌘}, ⌘\| | Align Left, Align Right, Center | HIG | tidak ada |

**D. Kunci penyuntingan teks** (aturan, bukan daftar): baris tidak boleh memakai panah atau Delete dan Forward Delete dengan ⌘ atau ⌥, ⌃ dengan huruf tanpa ⌘ (binding Emacs AppKit seperti ⌃A, ⌃E, ⌃K), atau Return, Escape, Tab, dan Space dengan atau tanpa modifier. Alasannya: key equivalent menu diproses sebelum field teks menerima tombol, jadi shortcut seperti ⌘⌫ akan mencuri "hapus sampai awal baris" dari field pencarian. ⌘R, ⇧⌘R, ⌥⌘R, ⌘O, ⌘D, dan ⌘1 sampai ⌘7 tidak punya binding di field teks AppKit; probe langkah 1 (P6) membuktikannya di field pencarian toolbar.

**E. Modifier 🌐 (fn)**: `CommandModifiers` tidak punya modifier fungsi, jadi shortcut 🌐 macOS (misalnya 🌐E, 🌐F, 🌐Q) tidak bisa dipakai.

Normalisasi untuk perbandingan: kunci huruf disimpan huruf kecil dengan ⇧ eksplisit, dan tanda baca bershift disimpan sebagai kunci dasarnya dengan ⇧, jadi ⌘? disimpan sebagai `/` dengan ⇧⌘, dan ⌘{ sebagai `[` dengan ⇧⌘. Tampilan memakai urutan modifier Apple ⌃⌥⇧⌘ lalu kunci huruf besar ("⇧⌘R"), dan tanda baca bershift tampil sebagai karakter bershiftnya tanpa ⇧ ("⌘?"). Konversi ke `KeyboardShortcut` dan normalisasi hasil probe memakai aturan yang sama (lihat *Model dan API*).

### Menambah perintah

Aturan untuk setiap fitur yang menambah perintah sesudah spec ini, juga dicatat di `design.md`:

1. Aksi utama tetap tombol di jendela (toolbar, inspector, baris) yang memakai `StacklyAction`. Perintah menu adalah jalan kedua, tidak pernah satu satunya jalan.
2. Tempat: membuat atau mengimpor sesuatu yang baru ke File; navigasi, tampilan, dan urutan ke View; aksi pada instance terpilih ke Instance; aksi tingkat app (daemon, update) ke Stackly; dokumentasi ke Help. Objek lain (environment, runtime, operasi) tidak mendapat menu sendiri sampai ada paling sedikit tiga aksi pada pilihan yang sering dipakai; sampai itu cukup tombol dan menu konteks.
3. Shortcut hanya untuk perintah yang sering dipakai dan tidak pernah untuk aksi destruktif (Remove, Purge, Force Stop, Stop All Services, Overwrite). Urutan pilihan: ⌘ dengan huruf yang bermakna, lalu ⇧⌘ untuk kebalikannya, lalu ⌥⌘ untuk variannya. Tabel A, aturan D, dan aturan E tidak pernah boleh; tabel B hanya dengan makna yang sama; tabel C hanya dengan pengecualian tertulis di spec fitur itu.
4. Cara mendaftar: tambah baris di `CommandMap` (atau ubah baris planned menjadi built), tambah `CommandItem(.id)` di builder menu yang tepat di `StacklyCommands`, terbitkan `StacklyAction` dari halaman lewat `.publishesCommands(.page(bagian), …)` (atau dari `AppShell` untuk `.shell`), dan perbarui tabel di `design.md`. Test peta dan source rule berjalan sendiri.
5. Menu di dalam jendela (More, menu konteks) memakai action dan judul yang sama tanpa shortcut, dan tooltip tombol toolbar memakai `CommandMap.helpText(for:)`.

Usulan tempat untuk aksi mockup yang menu nya diputuskan fitur pemiliknya (bukan keputusan spec ini; tidak ada baris di peta sampai fitur itu memutuskan):

| Aksi di mockup | Fitur | Usulan |
| --- | --- | --- |
| Import Provider…, Register External Runtime…, Import Environment… | 37, 56, 44 | File, tanpa shortcut |
| Export Diagnostic Bundle… | 34 | File, tanpa shortcut; Run Doctor tetap tombol halaman |
| Back Up Now, Restore… | 52 | Instance, tanpa shortcut |
| Bring Up, Bring Down, Validate environment | 46, 47, 44 | tombol halaman; menu Environment bila butir 2 terpenuhi |
| Cancel dan Retry operasi | 35 | tombol halaman |
| Stop All Services…, Check for Updates… | 32, 61 | Stackly, tanpa shortcut |
| Follow log, Load Earlier, Find Next | 27 | View dan Edit > Find (⌘G dari tabel B) |

### Model dan API

Semua di `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Commands/`. Data peta `nonisolated` dan `Sendable` supaya target UI test fitur 68 bisa membacanya tanpa `@testable`; yang memegang closure ada di main actor. Tipe aksi milik spec 0067 dipakai apa adanya:

```swift
// Milik spec 0067 (dikutip, tidak didefinisikan di sini).
nonisolated public enum ActionRole: Equatable, Sendable { case standard(confirmation: ActionConfirmation?), destructive(ActionConfirmation) }
nonisolated public enum ActionAvailability: Equatable, Sendable { case enabled, busy, disabled(reason: String, showsReason: Bool) }
public struct StacklyAction: Identifiable {   // main actor: id, title, symbol, role, mutates, disabledReason, isBusy, perform
    public func availability(gate: MutationAvailability) -> ActionAvailability
    public func invoke(gate: MutationAvailability, present: (ConfirmationRequest) -> Void)
}
public struct ActionHost {                    // main actor; .actionHost() menyusunnya ulang setiap render, jadi gate selalu terkini
    public let gate: MutationAvailability
    public let present: @MainActor (ConfirmationRequest) -> Void
    public func run(_ action: StacklyAction)  // = action.invoke(gate: gate, present: present)
}
extension EnvironmentValues { @Entry public var actionHost: ActionHost? = nil }
```

```swift
// CommandMap.swift: data murni, public, read only.
nonisolated public enum CommandID: String, CaseIterable, Hashable, Sendable { … }  // raw value tetap, dipakai test fitur 68
nonisolated public enum CommandMenuID: String, CaseIterable, Sendable { case app, file, edit, view, instance, window, help }
nonisolated public enum CommandSubmenuID: String, CaseIterable, Sendable { case find, sortInstancesBy }
nonisolated public enum CommandKind: String, Hashable, Sendable { case button, toggle, system }
nonisolated public struct CommandModifiers: OptionSet, Hashable, Sendable { command, shift, option, control }
nonisolated public struct CommandShortcut: Hashable, Sendable {
    public let key: String                   // kunci dasar, satu karakter huruf kecil: "r", "1", ",", "[", "/"
    public let modifiers: CommandModifiers   // ⇧ eksplisit, juga untuk tanda baca bershift ("?" = "/" dengan ⇧⌘)
    public var display: String               // "⇧⌘R", "⌘?"
}
nonisolated public enum CommandScope: Hashable, Sendable { case app, shell, page(Set<SidebarSection>) }
nonisolated public enum CommandRequirement: String, Hashable, Sendable { case daemon, selection, detail, search, noConnection }
nonisolated public enum CommandStatus: Hashable, Sendable { case built, planned }
nonisolated public enum StandardMeaning: String, Hashable, Sendable { case new, open, duplicate, find, findNext, findPrevious, useSelectionForFind, jumpToSelection, back, forward, help, save, saveAs, print, pageSetup, zoomIn, zoomOut, actualSize }
nonisolated public struct CommandEntry: Hashable, Identifiable, Sendable {
    public let id: CommandID
    public let menu: CommandMenuID
    public let submenu: CommandSubmenuID?
    public let group: Int                    // kelompok di dalam menu atau submenu (0 untuk kind .system)
    public let kind: CommandKind
    public let title: String
    public let alternateTitle: String?       // judul kedua untuk perintah yang judulnya berganti (Hide Inspector)
    public let shortcut: CommandShortcut?
    public let needsInput: Bool              // judul berakhir "…"
    public let scope: CommandScope
    public let requirements: Set<CommandRequirement>
    public let owner: Int                    // nomor fitur
    public let status: CommandStatus
    public let standardMeaning: StandardMeaning?
    public let exception: String?            // alasan memakai shortcut tabel C, atau tabel B dengan makna lain
}
nonisolated public enum CommandMap {
    public static func entry(for id: CommandID) -> CommandEntry   // switch lengkap atas CommandID
    public static var entries: [CommandEntry]                     // CommandID.allCases.map(entry(for:))
    public static func layout(menu: CommandMenuID, submenu: CommandSubmenuID?) -> [[CommandEntry]]  // baris built per kelompok
    public static func helpText(for id: CommandID, title: String? = nil) -> String  // "Hide Inspector (⌥⌘I)", atau judul saja
}

// SystemShortcuts.swift
nonisolated public struct SystemShortcut: Hashable, Sendable { shortcut: CommandShortcut, title: String, source: Source, meaning: StandardMeaning? }
nonisolated public enum SystemShortcuts {
    public static let blocked: [SystemShortcut]      // tabel A
    public static let standard: [SystemShortcut]     // tabel B
    public static let formatting: [SystemShortcut]   // tabel C
    public static func isTextEditingKey(_ shortcut: CommandShortcut) -> Bool  // aturan D
}

// MainWindowKey.swift: satu per jendela utama, bukan global.
@MainActor @Observable public final class MainWindowKey {
    public private(set) var isKey: Bool      // jendela ini adalah key window
    public var isKeyNow: Bool                // window?.isKeyWindow ?? false, dibaca saat perintah dijalankan
    func track(_ window: NSWindow)           // MainWindowAccessor memanggilnya; mendengar didBecomeKey, didResignKey, willClose milik window itu saja
}
extension EnvironmentValues { @Entry var mainWindowKey: MainWindowKey? = nil }

// PublishedCommands.swift: main actor.
public enum CommandLevel: Hashable, Sendable { case shell, page(SidebarSection) }
@MainActor public struct PublishedCommands {
    public let level: CommandLevel
    public let actions: [CommandID: StacklyAction]
    public let checked: Set<CommandID>       // item toggle yang bertanda centang (urutan aktif)
    let host: ActionHost                     // dari @Environment(\.actionHost) penerbit; host.gate adalah gate penerbit
    let isWindowKey: Bool                    // dari @Environment(\.mainWindowKey)?.isKey
    let windowKey: MainWindowKey?            // untuk penjaga saat dijalankan
}
extension FocusedValues {
    @Entry var shellCommands: PublishedCommands?
    @Entry var pageCommands: PublishedCommands?
}
extension View {
    /// Publishes the actions the menu commands of `level` run, with this view's action host and window key state.
    public func publishesCommands(_ level: CommandLevel, _ actions: [CommandID: StacklyAction], checked: Set<CommandID> = []) -> some View
}

// CommandItemState.swift: fungsi murni yang dites.
@MainActor struct CommandItemState {
    let title: String
    let isEnabled: Bool
    let subtitle: String?
    let isChecked: Bool
    let run: (@MainActor () -> Void)?
    static func resolve(_ entry: CommandEntry, shell: PublishedCommands?, page: PublishedCommands?) -> CommandItemState
}

// StacklyCommands.swift
public struct StacklyCommands: Commands {
    public init(runtime: ShellRuntime)
    static func appActions(runtime: ShellRuntime) -> [CommandID: @MainActor () -> Void]  // aksi item .app ber kind .button
    // @FocusedValue(\.shellCommands), @FocusedValue(\.pageCommands)
}
```

- `.publishesCommands` membaca `@Environment(\.actionHost)` dan `@Environment(\.mainWindowKey)` lalu memasang `.focusedSceneValue(\.shellCommands, …)` atau `\.pageCommands`. Tanpa `actionHost` di environment ia tidak menerbitkan apa pun dan memanggil `assertionFailure("menu commands need .actionHost()")`, sama dengan aturan host spec 0067. Setiap halaman memanggilnya satu kali dengan semua action nya; `AppShell` memanggilnya satu kali untuk `.shell`. Satu halaman tampil pada satu waktu (`switch` bagian di `AppShell`), jadi `pageCommands` punya satu penerbit.
- Urutan modifier di `AppShell` (G10): `.publishesCommands(.shell, …)` dipasang di dalam cakupan `.actionHost()`, yang dipasang di dalam `.environment(\.mutationAvailability, …)` dan `.environment(\.mainWindowKey, …)`, jadi penerbit membaca gate dan state key window jendela itu, bukan nilai default. Test struktur shell memastikan `PublishedCommands.host.gate` sama dengan `presentation.mutationAvailability` untuk setiap baris status daemon.
- `CommandItemState.resolve`: (1) kind `.system` dan scope `.app` tidak lewat fungsi ini; (2) `shell` nil atau `shell.isWindowKey` false → nonaktif, tanpa subtitle (berlaku juga untuk `.page`, karena halaman hanya ada di dalam shell); (3) penerbit = `shell` untuk `.shell`, `page` untuk `.page(S)` dengan `level` `.page(s)` dan `s ∈ S`, selain itu nonaktif tanpa subtitle; (4) action = `penerbit.actions[id]`, nil → nonaktif tanpa subtitle; (5) `a = action.availability(gate: penerbit.host.gate)`: aktif hanya kalau `a == .enabled`; subtitle = `reason` untuk `.disabled(reason, showsReason: true)`, "In progress" untuk `.busy`, tidak ada untuk `.disabled(showsReason: false)` (gate; banner jendela sudah menulisnya), semuanya hanya kalau cek subtitle spec 0067 milestone 1 lulus; (6) judul = `action.title` kalau baris punya `alternateTitle` (harus sama dengan `title` atau `alternateTitle`, dijaga test penerbit), selain itu `entry.title`; (7) `isChecked = penerbit.checked.contains(id)` untuk kind `.toggle`; (8) `run` (G2) mengecek ulang saat dijalankan: `penerbit.windowKey?.isKeyNow == true` dan `action.availability(gate: penerbit.host.gate) == .enabled`; kalau salah satunya gagal, `run` tidak berbuat apa pun; kalau lolos, `penerbit.host.run(action)`.
- `CommandItem(_ id:)` (internal view) merender dari `entry.kind`: `.button` sebagai `Button`, `.toggle` sebagai `Toggle` menu, dengan judul dan subtitle dari state, `.keyboardShortcut` dari `entry.shortcut` lewat konversi di main actor, dan `.disabled(!isEnabled)`. Item `.app` ber kind `.button` memakai `StacklyCommands.appActions(runtime:)` dan selalu aktif.
- Konversi shortcut (G9): untuk kunci huruf, angka, dan tanda baca tanpa shift, `KeyboardShortcut(KeyEquivalent(key), modifiers:)` apa adanya. Untuk tanda baca bershift (saat ini hanya ⌘? dan, di tabel, ⌘{ ⌘} ⌘\|), konversi memakai karakter bershift tanpa ⇧ (`"?"` dengan ⌘), supaya menu menampilkan "⌘?" dan bukan "⇧⌘/". Probe accessibility menormalkan `AXMenuItemCmdChar` dan `AXMenuItemCmdModifiers` dengan aturan yang sama sebelum membandingkan dengan peta.
- `MainWindowKey` (G2, menggantikan state global): satu instance per jendela utama, dibuat dan dipegang view akar jendela (`AppShell` hari ini sebagai `@State`, `MainWindowRoot` setelah langkah 2 milestone 1 spec 0016) dan dipasang di environment `mainWindowKey`. `MainWindowAccessor` memanggil `track(_:)` dengan window nya; objek itu mendengar `didBecomeKeyNotification`, `didResignKeyNotification`, dan `willCloseNotification` hanya untuk window itu dan menulis `isKey`. Sheet dan `NSAlert` sheet punya window sendiri yang menjadi key window, jadi saat keduanya terbuka `isKey` false. Menu hanya membaca focused value; perubahan `isKey` membuat penerbit menerbitkan nilai baru, yang menghitung ulang menu.
- `StacklyCommands` dipasang satu kali: `.commands { StacklyCommands(runtime: appDelegate.runtime) }` pada scene `Window` utama di `StacklyApp`; perintah SwiftUI berlaku untuk seluruh app, apa pun scene yang aktif. Scene utama juga mendapat `.keyboardShortcut` dari `CommandMap.entry(for: .showMainWindow)`.
- `appActions`: `welcome` memanggil `runtime.onboarding.present()` lalu `runtime.windowOpener.openMain()`; `stacklyHelp` ditambah fitur 66. Test peta memastikan setiap baris `.app` ber kind `.button` yang built punya aksi di `appActions`.

### Model data

Tidak ada tabel, file, kolom registry, atau preferensi baru, dan tidak ada state machine. `CommandMap` dan `SystemShortcuts` adalah data kode yang dikompilasi; `PublishedCommands` dan `MainWindowKey` hanya hidup di memori selama app berjalan. State yang dipulihkan sistem tetap `@SceneStorage` yang sudah ada.

### Penerbit

| Penerbit | Level | Action yang diterbitkan |
| --- | --- | --- |
| `AppShell` | `.shell` | `showOverview` sampai `showDiagnostics` (selalu, `mutates` false, `perform` = `select(bagian)`); `reconnect` (hanya saat `!presentation.hasConnection`; `disabledReason` "Relaunch Stackly after fixing the problem." saat `presentation.isMisconfigured`; `isBusy` = `presentation.isReconnecting`; `perform` = closure `reconnect`); `newInstance` (hanya saat `newInstanceAction` tidak nil; `mutates` true; `perform` = `newInstanceAction()`) |
| Halaman Instances (fitur 22, 21) | `.page(.instances)` | `find` (`.searchFocused`; tidak diterbitkan selama detail terbuka, karena field pencarian tidak ada di toolbar detail); `toggleInspector` (judul "Show Inspector" atau "Hide Inspector"); tujuh baris urut (`checked` untuk kolom dan arah aktif); untuk target (detail terbuka, selain itu baris terpilih): `openInstance` (hanya tabel dengan pilihan), `back` (hanya detail terbuka), `startInstance` dan `stopInstance` dari `InstanceAction.resolve` (aturan AC-11), `copyConnectionString` (action yang sama dengan item More detail, atau baris Connection inspector bila detail belum terbuka); kelak `restartInstance`, `reloadInstance`, `duplicateInstance`, `removeInstance` oleh fitur pemiliknya |
| Halaman Catalog (fitur 17) | `.page(.catalog)` | `find` |
| Halaman lain dengan pencarian | `.page(bagian)` | `find` |

Pemetaan aksi utama ke Start dan Stop (dari tabel *Aksi instance* spec 0022): Start atau Retry yang aktif → `startInstance` aktif; Retry nonaktif karena belum terinisialisasi, atau Start nonaktif karena `uninitialized` → `startInstance` dengan `disabledReason` yang sama; `inFlight` atau `pending` start, "Initializing…", "Starting…" → `startInstance` dengan `isBusy`; Stop aktif → `stopInstance`; Stop nonaktif karena `unknown` → `stopInstance` dengan `disabledReason` yang sama; `inFlight` atau `pending` stop, "Stopping…" → `stopInstance` dengan `isBusy`. Yang tidak diterbitkan tampil nonaktif tanpa subtitle.

### Alur sebuah perintah

1. Pengguna menekan ⇧⌘R. AppKit mencocokkan key equivalent di main menu sebelum field teks menerima tombol (karena itu aturan D).
2. Item `stopInstance` di menu Instance sudah dihitung `CommandItemState.resolve` dari `@FocusedValue(\.shellCommands)` (yang membawa `isWindowKey`), `@FocusedValue(\.pageCommands)` (penerbit halaman Instances), dan `action.availability(gate: host.gate)`; setiap perubahan state membuat penerbit menerbitkan nilai baru, dan SwiftUI menghitung ulang menu.
3. Kalau nonaktif, AppKit tidak menjalankan apa pun (bunyi sistem). Kalau aktif, `run` mengecek ulang `windowKey.isKeyNow` dan `availability` (penjaga kalau menu belum dihitung ulang), lalu memanggil `host.run(action)`: dengan konfirmasi di `role` (misalnya stop instance yang di lease, fitur 26 dan 47) dialog spec 0012 tampil sebagai sheet di jendela utama; tanpa konfirmasi `perform` berjalan lewat `InstanceActionModel.perform` dan jalur mutasi spec 0013.
4. Tombol aksi utama di halaman ikut sibuk karena status lokal `InstanceActionModel` yang sama; umpan balik memakai presenter fitur 67.

### Onboarding, jendela lain, dan sheet

- Selama onboarding `MainWindowRoot` menampilkan `OnboardingFlow`, bukan `AppShell`, jadi tidak ada `shellCommands` dan `pageCommands`: semua perintah `.shell` dan `.page` nonaktif tanpa kode khusus (follow up spec 0016). `.app` tetap: Settings ⌘,, Welcome to Stackly, Window > Stackly ⌘0, Quit. Escape tetap milik `OnboardingFlow` (spec 0016) dan tidak ada di peta.
- Saat Settings, Design Gallery, atau jendela lain fitur berikutnya menjadi key window, focused value dari jendela utama hilang (diharapkan P4) dan `MainWindowKey.isKey` jendela utama false, jadi perintah shell nonaktif; `isWindowKey` tetap menjaga kalau probe menunjukkan focused value tidak hilang.
- Popover yang punya kontrol fokus (misalnya popover menu bar fitur 32) menjadi key window, jadi selama popover itu terbuka perintah shell nonaktif. Ini diterima: popover menutup sendiri saat pengguna kembali ke jendela utama.
- Saat jendela utama tertutup atau minimized, jendela itu bukan key window, jadi perintah shell nonaktif; ⌘0 membukanya lagi.
- Saat sheet atau dialog konfirmasi terbuka, key window adalah sheet nya, jadi perintah shell nonaktif. Tombol `.defaultAction` dan `.cancelAction` di footer sheet (spec 0067) dan Return serta Escape di dialog (spec 0012) tetap bekerja karena bukan bagian peta.
- Saat app tidak aktif tidak ada key window; menu juga tidak bisa dipakai, jadi tidak ada perbedaan yang terlihat.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| setiap item | judul, menu, urutan, shortcut | `CommandMap.entry(for:)`; judul dinamis dari `action.title` hanya untuk baris dengan `alternateTitle` |
| setiap item `.shell` dan `.page` | aktif | `CommandItemState.resolve`: `PublishedCommands` dari `@FocusedValue` (termasuk `isWindowKey`), dan `action.availability(gate: host.gate) == .enabled` spec 0067 |
| setiap item `.shell` dan `.page` | gate mutasi | `host.gate` dari `.actionHost()` di cakupan penerbit, yang membaca `@Environment(\.mutationAvailability)` (diset `AppShell` dari `ShellPresentation`, spec 0013) |
| setiap item | subtitle alasan | `ActionAvailability`: `reason` untuk `.disabled(showsReason: true)`, "In progress" untuk `.busy`, tidak ada untuk gate; hanya kalau cek subtitle spec 0067 milestone 1 lulus |
| setiap item | yang dijalankan | `host.run(action)`, `host` dari `@Environment(\.actionHost)` penerbit (spec 0067) |
| item bertanda centang | centang | `PublishedCommands.checked` dari penerbit |
| ⌘1 sampai ⌘7 | bagian tujuan | `CommandID` → `SidebarSection` tetap di `AppShell` (urutan `SidebarSection.allCases`) |
| Reconnect | aktif, alasan, busy | `ShellPresentation.hasConnection`, `isMisconfigured`, `isReconnecting` (spec 0013); teks alasan sama dengan `ConnectionDiagnosticsSection` |
| New Instance… | ada atau tidak | `EnvironmentValues.newInstanceAction` (spec 0022 dan 0020) |
| Find… | field yang difokus | `@FocusState` pencarian halaman lewat `.searchFocused` |
| perintah instance | instance target | path detail halaman Instances (`InstanceRoute`, spec 0021), selain itu `@SceneStorage("instances.selection")` (spec 0022) |
| Start, Stop | aktif, alasan, busy, judul aksi | `InstanceAction.resolve(summary:local:)` (spec 0022) |
| Copy Connection String | aktif, alasan, isi pasteboard | action Copy detail dan inspector (spec 0021, `RevealableSecretText` spec 0011) |
| Show/Hide Inspector | state | binding `.stacklyInspector(isPresented:)` dan `@SceneStorage("instances.inspector")` (spec 0067, 0022) |
| Sort Instances By | urutan aktif | `sortOrder` tabel Instances (spec 0022) |
| Welcome to Stackly | aksi | `runtime.onboarding.present()` dan `runtime.windowOpener.openMain()` (spec 0016, 0013) |
| Window > Stackly | judul | judul scene `Window("Stackly", id: "main")` |
| key window | `isWindowKey`, `isKeyNow` | `MainWindowKey` jendela itu, dari notifikasi key window nya sendiri lewat `MainWindowAccessor`, dan `NSWindow.isKeyWindow` saat dijalankan |
| item `.app` | aksi | `StacklyCommands.appActions(runtime:)` |
| pemisah menu | kelompok | `CommandEntry.group` lewat `CommandMap.layout(menu:submenu:)` |
| test bentrok | daftar sistem | tabel A sampai C dan aturan D, E di spec ini, ditambah hasil probe langkah 1 |

### Key invariants

- Shortcut hanya ada di `CommandMap`. Tidak ada `.keyboardShortcut` dengan kunci literal di luar `Commands/` (selain `.defaultAction` dan `.cancelAction`), dan tombol di jendela tidak memasang shortcut perintah.
- Tidak ada dua baris dengan shortcut sama, tidak ada baris di tabel A atau aturan D dan E, tabel B hanya dengan makna sama atau pengecualian, tabel C hanya dengan pengecualian.
- Item menu tidak punya logika aktif sendiri: aktif hanya dari scope, key window, action yang diterbitkan, dan `availability(gate:)` spec 0067. Item menu tidak pernah memanggil `perform` langsung.
- Perintah `.shell` dan `.page` hanya diterbitkan `AppShell` dan halaman di dalamnya.
- Item tidak pernah disembunyikan karena state; hanya baris planned yang tidak dirender.
- Aksi destruktif tidak punya shortcut dan selalu lewat konfirmasi spec 0012.
- Raw value `CommandID` tidak pernah berubah setelah dipakai (dibaca test fitur 68).

### Security model

Tidak ada data atau izin baru. Perintah menu memakai action yang sama dengan tombol, jadi semua mutasi tetap lewat daemon dan jalur mutasi spec 0013 di socket milik uid pengguna (spec 0006). Copy Connection String lewat menu memakai jalur `secret.reveal { purpose: copy }` dan `ConcealedPasteboard` yang sama dengan tombol (spec 0021 dan 0011), jadi tidak ada jalur secret baru. Judul dan subtitle menu hanya berisi copy dari peta dan alasan nonaktif dari action, tidak pernah nilai secret. Probe accessibility langkah 1 butuh izin Accessibility untuk proses pemeriksa di Mac ini; izin itu diberikan user di System Settings dan tidak menjadi bagian app.

### Configuration required

Tidak ada env var, secret, atau preferensi baru. `@SceneStorage` yang sudah ada (`shell.section`, `instances.selection`, `instances.inspector`) dipakai apa adanya.

### Critical test scenarios

- Peta: setiap `CommandID` punya satu baris; tidak ada shortcut ganda (termasuk planned); setiap baris di tabel A gagal; ⌘N tanpa `standardMeaning` `new` gagal; ⇧⌘C tanpa `exception` gagal; ⌘⌫, ⌥←, ⌃A, dan Return gagal lewat aturan D; akhiran "…" sesuai `needsInput`; `helpText` dengan dan tanpa shortcut; tampilan "⇧⌘R" dan "⌘?". Memverifikasi **AC-1**, **AC-2**.
- Source rule: `.keyboardShortcut("x")` di file halaman gagal; `.keyboardShortcut(.defaultAction)` lolos; `CommandGroup` di luar `Commands/` gagal; `CommandID` built yang tidak dipakai `CommandItem`, atau planned yang dipakai, gagal; tidak ada `provisional: F69`. Memverifikasi **AC-1**, **AC-16**.
- Resolve: untuk scope `.shell` dan `.page`, kombinasi key window true dan false, penerbit ada dan nil, bagian cocok dan tidak, action ada dan nil, gate available dan blocked, `mutates` true dan false, busy, `disabledReason`; subtitle hanya saat action ada dan alasan berupa teks; judul dinamis inspector; `run` memanggil `host.run`, bukan `perform` (host palsu mencatat panggilan). Memverifikasi **AC-4**, **AC-5**, **AC-6**.
- Key window: `MainWindowKey` dengan `NSWindow` test yang menjadi key, kehilangan key ke sheet, kehilangan key ke jendela lain, dan ditutup; `isKey` dan `isKeyNow` sesuai, dan notifikasi window lain tidak mengubahnya. `run` menolak saat `isKeyNow` false walaupun state lama bilang aktif. Memverifikasi **AC-4**, **AC-5**.
- Layout menu: `CommandMap.layout` untuk status awal dan untuk peta dengan semua baris built, tanpa kelompok kosong; `appActions` punya aksi untuk setiap baris `.app` ber kind `.button` yang built; struktur `AppShell` memastikan `PublishedCommands.host.gate` sama dengan `presentation.mutationAvailability` untuk setiap baris status daemon (G10). Memverifikasi **AC-3**, **AC-5**, **AC-13**.
- Penerbit shell: untuk setiap baris tabel status daemon spec 0013, `AppShell` menerbitkan `reconnect` hanya tanpa koneksi, dengan alasan misconfigured dan busy saat reconnecting; ⌘1 sampai ⌘7 memanggil `select` yang sama dengan binding sidebar; `newInstance` hanya saat `newInstanceAction` ada dan `mutates` true. Memverifikasi **AC-7**, **AC-8**, **AC-9**.
- Penerbit Instances (langkah 6): setiap baris tabel *Aksi instance* spec 0022 menghasilkan Start atau Stop dengan aktif, alasan, dan busy yang sama dengan tombol; target detail mengalahkan pilihan tabel; tanpa pilihan tidak ada action instance; Copy Connection String nonaktif dengan alasan yang sama dengan item More; checked untuk urutan aktif; Back hanya saat detail terbuka; Find memfokuskan pencarian. Memverifikasi **AC-10**, **AC-11**, **AC-12**, **AC-14**.
- Konfirmasi: action dengan `confirmation` yang dijalankan lewat menu memanggil `present` host, tidak `perform`; host palsu menerima satu request. Memverifikasi **AC-6**.
- Onboarding: `MainWindowRoot` dengan onboarding tampil tidak menerbitkan `shellCommands`; Welcome to Stackly memanggil `present()` lalu `openMain()`. Memverifikasi **AC-4**, **AC-13**.
- Probe dan cek manual: lihat langkah 1 dan *Cek manual di /check verify*. Memverifikasi **AC-2**, **AC-3**, **AC-13**, **AC-15**.
- Auth atau permission: tidak ada peran pengguna; perintah yang mengubah data nonaktif saat gate blocked, dan daemon tetap memeriksa setiap call (spec 0006, 0013).

### Cek manual di /check verify

Dicatat sebagai evidence manual spec 0002 (kind `native`, status awal `NOT_RUN`), di app Debug dengan `stacklyd` asli dan `STACKLY_ROOT` ke folder sementara:

1. Probe accessibility: dump semua menu bar app dengan `AXMenuItemCmdChar` dan `AXMenuItemCmdModifiers`; tidak ada key equivalent ganda, setiap baris built tampil di menu dan kelompok yang benar dengan shortcut persis peta, dan tidak ada item "New Window", Spelling, Substitutions, Speech, atau "Stackly Help" bawaan (**AC-2**, **AC-3**). Diulang setiap macOS naik versi.
2. Hanya keyboard: ⌘1 sampai ⌘7 memilih bagian; ⌘W menutup jendela utama (notice tampil); ⌘0 membukanya lagi; minimize lalu ⌘0 mengembalikannya; ⌘, membuka Settings, lalu ⌘2 tidak melakukan apa pun sampai jendela utama aktif lagi (**AC-4**, **AC-7**, **AC-13**).
3. Onboarding (Help > Welcome to Stackly): ⌘1 dan ⌘N nonaktif, ⌘, dan ⌘0 tetap bekerja; setelah selesai, perintah shell aktif lagi (**AC-4**, **AC-13**).
4. Hentikan `stacklyd`: New Instance… nonaktif tanpa subtitle (alasan gate ada di banner jendela), Reconnect to stacklyd aktif dan menyambung lagi setelah `stacklyd` jalan; saat terhubung Reconnect nonaktif (**AC-5**, **AC-8**, **AC-9**).
5. Setelah fitur 20, 21, dan 22: ⌘N lalu isi sheet dan Return membuat instance; ⌘2, Tab ke tabel, panah, ⇧⌘R menghentikannya; ⌘R menjalankannya; ⌘O membuka detail; ⇧⌘C menyalin connection string; ⌘[ kembali; ⌥⌘I menyembunyikan dan menampilkan inspector; View > Sort Instances By mengurutkan; saat sheet New Instance terbuka ⌘1 dan ⌘R tidak melakukan apa pun (**AC-4**, **AC-9**, **AC-11**, **AC-12**, **AC-15**).
6. VoiceOver: buka setiap menu dengan ⌃F2 (atau VO+M); setiap item terbaca dengan judul, shortcut, dan "dimmed" saat nonaktif; ⌘F di Instances dan Catalog memfokuskan field pencarian dan VoiceOver membacakannya (**AC-10**, **AC-15**; PRD AC-027).

## Build plan

Pendekatan proyek adalah Journey. Fitur ini jalur paralel UI: peta dan perintah shell dibangun dulu supaya setiap journey berikutnya mendaftar di peta yang sudah ada, lalu perintah halaman disambung saat halamannya ada. Probe macOS 27 jalan pertama karena empat keputusan bergantung pada perilaku SwiftUI dan AppKit yang hanya bisa dibuktikan di Mac ini. File shell (`StacklyApp.swift`, `AppShell.swift`) juga diubah langkah 2 milestone 1 spec 0016, langkah 14 milestone 3 spec 0020, dan langkah 6 sampai 7 spec 0022, jadi tidak ada dua di antaranya yang berjalan sebagai sesi bersamaan (aturan file shell spec 0022); yang di merge belakangan memindahkan hasil yang lain ke peta.

**Milestone 1: Peta perintah dan probe** (needs fitur 13 milestone 3 (shell, sudah selesai) di `ui-macos.md`; tidak menunggu fitur lain; bisa paralel dengan fitur 67 milestone 1)

1. Probe di macOS 27 dan Xcode 27 di app Debug, lewat accessibility API dan tombol nyata, hasilnya (dengan nomor build) dicatat di `design.md` bagian "Commands and shortcuts": P1 dump menu bar app saat ini dengan key equivalent setiap item (item sistem dan SwiftUI), dibandingkan dengan tabel A, dan item baru ditambahkan ke tabel A; P2 `Window(...).keyboardShortcut` memberi Window > Stackly ⌘0 yang membuka jendela setelah ditutup dan mengembalikannya dari minimize; P3 hasil `CommandGroup(replacing:)` untuk `.newItem`, `.textEditing`, dan `.help` (apa yang hilang, kolom pencarian Help tetap) dan tempat `CommandMenu("Instance")`; P4 `focusedSceneValue` dari view di jendela utama terbaca di `Commands`, hilang saat Settings key, dan tetap ada saat sheet atau `NSAlert` sheet terbuka, sedangkan `NSApp.keyWindow` saat itu adalah window sheet; P3b dengan grup `.textEditing` bawaan dibiarkan, apakah Find… ⌘F bawaan memfokuskan field `.searchable` toolbar dan nonaktif di halaman tanpa pencarian; P5 `.searchFocused` memfokuskan field `.searchable` toolbar dari perintah menu; P6 ⌘R, ⇧⌘R, ⌥⌘R, ⌘O, dan ⌘D di field pencarian toolbar sampai ke menu tanpa mengubah teks, Button di `Commands` dengan judul yang berganti ikut berganti di menu, dan `KeyEquivalent("?")` dengan ⌘ tampil "⌘?"; P7 saat menu terbuka atau tepat sebelum key equivalent ditekan, state berubah (sheet terbuka, gate menjadi blocked): menu dihitung ulang, dan kalau belum, penjaga `run` menolak tanpa menjalankan action. Kalau sebuah probe gagal, berhenti dan kirim ke `/architect` dengan cadangan di *Cadangan probe*. Memenuhi **AC-2**, **AC-3**, **AC-4**, **AC-5**, **AC-10**, **AC-13**.
2. `CommandMap.swift` (semua tipe dan semua baris *Peta perintah* dengan status awalnya), `SystemShortcuts.swift` (tabel A sampai C, aturan D dan E, ditambah item dari P1), test unit peta di `StacklyUITests/Commands/`, dan aturan baru di `SourceRuleTests` (`.keyboardShortcut` dengan kunci literal, `CommandGroup`, `CommandMenu`, `.commands`, `CommandID` built dipakai tepat sekali, planned tidak dipakai). Memenuhi **AC-1**, **AC-2**.

**Milestone 2: Perintah shell** (after milestone 1; needs fitur 67 milestone 1 di `ui-macos.md` (`StacklyAction`, `ActionRole`, `ActionAvailability`, `availability(gate:)`, `invoke(gate:present:)`, `ActionHost`, `.actionHost()` terpasang di `AppShell` (langkah 6 dan 7 spec 0067), dan hasil cek subtitle menu (langkah 1 spec 0067)); after langkah 2 milestone 1 spec 0016 dan langkah 14 milestone 3 spec 0020 bila salah satunya sedang berjalan)

3. `PublishedCommands`, `CommandLevel`, `FocusedValues.shellCommands` dan `pageCommands`, `.publishesCommands`, `MainWindowKey` per jendela (dibuat view akar jendela, dipasang di environment, diberi window oleh `MainWindowAccessor`), `CommandItemState.resolve` dengan penjaga `run`, `CommandItem` per `kind`, `CommandMap.layout`, `appActions`, dan test unit resolve, key window, penjaga, layout, dan konfirmasi lewat host palsu. Memenuhi **AC-4**, **AC-5**, **AC-6**.
4. `StacklyCommands` dan `.commands { StacklyCommands(runtime:) }` plus `.keyboardShortcut` scene di `StacklyApp`: menu Stackly (Reconnect), File (pengganti `.newItem`; New Instance… kalau langkah 14 spec 0020 sudah di merge, dengan baris `newInstance` menjadi built), Edit (Find), View (`SidebarCommands()` dan ⌘1 sampai ⌘7), Window (⌘0), Help (pengganti `.help`; Welcome to Stackly kalau langkah 2 spec 0016 sudah di merge, dengan baris `welcome` menjadi built). `AppShell` mendapat `select(_:)` (dipanggil setter binding sidebar dan perintah, dengan cabang pilih ulang yang memanggil hook `onReselect`, kosong sampai fitur 21), memegang `MainWindowKey`, dan menerbitkan `.shell` (bagian, Reconnect, New Instance) di dalam cakupan `.actionHost()` dan `.environment(\.mutationAvailability, …)`. Semua `CommandGroup` sementara bertanda `// provisional: F69` yang ada di main dipindah ke `StacklyCommands` (termasuk `InstancesCommands` spec 0022 kalau sudah ada, yang dipindah utuh ke langkah 6). Bagian "Commands and shortcuts" di `design.md` (peta, tabel sistem, aturan *Menambah perintah*, hasil probe). Test penerbit shell. Memenuhi **AC-3**, **AC-4**, **AC-7**, **AC-8**, **AC-9**, **AC-10**, **AC-13**, **AC-16**.
5. Evidence manual milestone 2: cek manual 1 sampai 4 dengan perintah yang sudah ada. Memenuhi **AC-2**, **AC-3**, **AC-4**, **AC-8**, **AC-13**.

**Milestone 3: Perintah halaman dan instance** (after milestone 2; needs fitur 22 milestone 3 (halaman Instances, `InstanceActionModel`, urutan, inspector) dan fitur 21 milestone 3 (detail, Open, Copy Connection String) di `02-instalasi-pertama.md`, fitur 17 milestone 3 (pencarian Catalog) di `02-instalasi-pertama.md`, dan fitur 67 milestone 3 (`.stacklyInspector`) di `ui-macos.md`. Halaman yang di merge setelah milestone 2 mendaftarkan perintahnya sendiri dengan aturan *Menambah perintah* (usulan update spec 0017, 0021, 0022), dan langkah 6 hanya mengerjakan yang belum terdaftar)

6. Menu Instance (`CommandMenu("Instance")`) dan penerbit halaman: halaman Instances menerbitkan `find`, `toggleInspector`, tujuh baris urut, `openInstance`, `back`, `startInstance`, `stopInstance`, dan `copyConnectionString` sesuai *Penerbit*; Catalog menerbitkan `find`; `InstancesCommands` dihapus; baris terkait menjadi built; tooltip tombol inspector dan aksi utama lewat `helpText`; test penerbit Instances. Memenuhi **AC-10**, **AC-11**, **AC-12**, **AC-14**.
7. Evidence manual milestone 3: cek manual 1, 5, dan 6 (keyboard saja dan VoiceOver untuk buat, hentikan, periksa). Memenuhi **AC-2**, **AC-15**.

### Cadangan probe

| Probe gagal | Cadangan yang dibawa ke `/architect` |
| --- | --- |
| P2: scene `Window` tidak menerima `.keyboardShortcut` | `CommandGroup(before: .windowList)` dengan item "Stackly" ⌘0 yang memanggil `WindowOpener.openMain()` |
| P3: `replacing: .textEditing` juga membuang Cut, Copy, Paste | Find milik Stackly ditaruh lewat `CommandGroup(after: .pasteboard)` dan grup `.textEditing` dibiarkan; kalau item Find… bawaan (juga ⌘F) tidak bisa dihilangkan, ⌘F tetap milik submenu bawaan dan AC-10 diubah lewat `/architect` |
| P4: focused value tidak terbaca di `Commands` | penerbit menulis ke objek `@Observable` per jendela di `ShellRuntime` (opsi alternatif K1) |
| P5: `.searchFocused` tidak bisa memfokuskan field toolbar | Find… menjadi item tanpa aksi milik Stackly dan ⌘F memakai perilaku standar `.searchable`; AC-10 diubah |
| P3b: Find bawaan sudah memfokuskan field `.searchable` dan nonaktif di halaman tanpa pencarian | grup `.textEditing` bawaan dipertahankan (Spelling dan lainnya tetap), baris `find` menjadi kind `system` (peta hanya mencatat ⌘F), halaman tidak menerbitkan `find`, dan AC-10 dipenuhi oleh perilaku bawaan; kalau Find bawaan aktif di halaman tanpa pencarian, rencana `replacing: .textEditing` tetap |
| P6: shortcut perintah mengubah teks field | ganti shortcut itu di peta lewat `/architect` |
| P7: menu tidak dihitung ulang sebelum key equivalent | penjaga `run` tetap menolak (tidak ada aksi yang salah jalan); kalau item yang basi juga tampil aktif terlalu lama, tambahkan `NSMenuDelegate` lewat `/architect` |

## Consequences

**Positive**:

- Satu tabel menentukan semua shortcut, dan test menolak bentrok sebelum app dibuild, termasuk untuk fitur yang belum ada.
- Menu tidak bisa berbeda dengan tombol: keduanya membaca action dan fungsi nonaktif yang sama, dan menjalankan host yang sama, jadi konfirmasi dan umpan balik tidak terduplikasi.
- Onboarding, Settings, sheet, dan dialog mematikan perintah shell lewat konstruksi, bukan daftar pengecualian yang harus dirawat.
- Fitur journey berikutnya cukup menambah baris, `CommandItem`, dan action; tidak ada `.commands` baru di tempat lain.
- Alur buat, hentikan, dan periksa instance punya jalan keyboard penuh untuk PRD AC-027.

**Negative / tradeoffs**:

- Menu bar mendapat menu Instance yang tidak ada di mockup; dicatat sebagai selisih mockup.
- ⌘R dan ⇧⌘C memakai shortcut format standar HIG dengan pengecualian; kalau Stackly kelak punya teks bergaya (tidak direncanakan), keduanya harus dipindah.
- ⇧⌘R menghentikan instance tanpa konfirmasi, sama dengan tombol Stop; salah tekan bisa menghentikan database. Aturan konfirmasi spec 0012 tetap berlaku untuk stop yang mengganggu pemilik lain.
- Perintah shell mati saat jendela utama tertutup; pengguna harus menekan ⌘0 dulu sebelum ⌘N.
- Submenu Spelling, Substitutions, Transformations, dan Speech hilang dari Edit untuk semua field.
- Help tidak punya item dokumentasi sampai fitur 66.
- Selama popover dengan kontrol fokus terbuka (fitur 32), perintah shell nonaktif.
- Item menu yang nonaktif karena konteks (tanpa pilihan, halaman lain) tidak menjelaskan alasannya; hanya action yang diterbitkan yang membawa alasan.
- Tergantung pada fitur 67 milestone 1 sebelum perintah shell bisa dibangun, dan pada lima probe platform.
- `CommandID` memuat baris planned yang belum punya kode; status nya harus diubah oleh build pemiliknya.

**Neutral**:

- Folder baru `Commands/` di `StacklyUI` dengan `CommandMap`, `SystemShortcuts`, `PublishedCommands`, `MainWindowKey`, `CommandItemState`, `CommandItem`, dan `StacklyCommands`; view akar jendela (`AppShell`, lalu `MainWindowRoot`) memegang `MainWindowKey`; `AppShell` mendapat `select(_:)` dengan hook `onReselect` dan satu `.publishesCommands`.
- Bagian baru "Commands and shortcuts" di `apps/macos/design.md`.
- Target UI test fitur 68 bergantung pada product `StacklyUI` untuk membaca peta.

### Usulan update untuk spec lain

Ditulis di sini karena spec ini tidak boleh mengubah spec lain; lead menjadikannya item `architect (update)` atau `/sync`. Semua additive kecuali yang disebut "mengganti".

- **Spec 0013** (gabungkan ke update fase 02 yang masih terbuka di `docs/scope/coordination/02-instalasi-pertama.usulan-fondasi.md` bagian "Spec 0013", terutama butir 1, 5, 8, dan P20):
  1. *Batas dengan fitur lain*, baris "Peta perintah dan shortcut (termasuk perintah reconnect) | 69": tambahkan "spec 0069: `StacklyCommands`, `CommandMap`, `PublishedCommands`, `MainWindowKey`; `AppShell` menerbitkan perintah shell lewat `.publishesCommands(.shell, …)` di dalam cakupan `.actionHost()` dan `.environment(\.mutationAvailability, …)`".
  2. *Shell jendela*: view akar jendela (`AppShell` hari ini, `MainWindowRoot` setelah spec 0016 langkah 2) memegang `@State MainWindowKey` dan memasangnya di environment `mainWindowKey`; `MainWindowAccessor` memanggil `track(_:)` dengan window nya, di samping laporan `windowWillClose` yang sudah ada. `ShellRuntime` tidak berubah.
  3. *Shell jendela*: `AppShell.select(_ section:)` adalah satu satunya jalan memilih bagian, dipanggil setter binding sidebar, perintah ⌘1 sampai ⌘7, dan Open Diagnostics di banner; saat bagian itu sudah tampil, `select(_:)` memanggil hook `onReselect(section)` (diisi spec 0021). `ShellNavigator` (kalau ada) tetap dicerminkan dua arah.
  4. Butir 8 dan P20 usulan fondasi ("`.commands`: Help Welcome to Stackly, dan File > New Instance… ⌘N bertanda `// provisional: F69`") mengganti: kalau update spec 0013 belum ditulis saat spec ini diterima, tulis `.commands { StacklyCommands(runtime:) }` (spec 0069) sebagai satu satunya `.commands`, tanpa `CommandGroup` sementara.
  5. *Scenes and window* di `design.md`: tooltip `SettingsLink` footer tetap "Settings (⌘,)"; shortcut nya berasal dari tabel A spec 0069.
- **Spec 0016**:
  1. Baris 95 (item menu `CommandGroup(after: .help)`): kalau langkah 2 milestone 1 dibangun sebelum milestone 2 spec 0069, tambahkan penanda `// provisional: F69` dan pakai `CommandGroup(replacing: .help)` supaya item "Stackly Help" bawaan tidak tampil; milestone 2 spec 0069 memindahkannya ke `CommandID.welcome` (scope `.app`). Kalau dibangun sesudahnya, ubah baris `welcome` menjadi built dan tambah `CommandItem(.welcome)` di `StacklyCommands`.
  2. Baris 99 ("Perintah shell lain … nonaktif selama onboarding tampil"): tambahkan "lewat konstruksi spec 0069: `OnboardingFlow` tidak menerbitkan `shellCommands`, jadi tidak ada kode penonaktifan di onboarding".
  3. Baris 403 (Neutral): "dipindah ke peta perintah" menjadi "dipindah ke `StacklyCommands` dan `CommandMap` spec 0069".
  4. *Tampilan di jendela utama* (`MainWindowRoot`, baris 91 dan 92): `MainWindowRoot` mengambil alih `@State MainWindowKey` dari `AppShell` bersama `MainWindowAccessor`, memasangnya di environment `mainWindowKey`, dan `MainWindowAccessor` memanggil `track(_:)` dengan window nya (spec 0069 *Model dan API*). `OnboardingFlow` tidak menerbitkan perintah.
- **Spec 0017** baris 410: ganti dengan "halaman Catalog menerbitkan `find` lewat `.publishesCommands(.page(.catalog), …)` dengan `.searchFocused` (spec 0069 AC-10); pindah bagian lewat ⌘4 milik shell. Kalau milestone 3 fitur ini dibangun sebelum milestone 2 spec 0069, `.searchable` memakai perilaku standar dan milestone 3 spec 0069 menyambungnya."
- **Spec 0020**:
  1. Baris 108 (tabel batas): "⌘N didaftarkan di peta itu" menjadi "baris `newInstance` di `CommandMap` spec 0069 menjadi built; `AppShell` menerbitkan action dari `newInstanceAction` (spec 0069 *Penerbit*)".
  2. AC-20 dan *Titik masuk* (baris 472): "perintah menu File > New Instance… (⌘N)" tetap; tambahkan "nonaktif saat jendela utama bukan key window atau onboarding tampil (spec 0069 AC-4, AC-9)".
  3. Langkah 14: kalau dibangun sebelum milestone 2 spec 0069, pakai `CommandGroup(replacing: .newItem)` bertanda `// provisional: F69` yang memanggil `newInstanceAction` dari runtime; kalau sesudahnya, tidak ada `CommandGroup`, cukup ubah baris `newInstance` dan terbitkan action di `AppShell`.
- **Spec 0021**: *Halaman detail dan navigasi* dan AC-12 dan AC-15: halaman Instances menerbitkan `openInstance` (Open di inspector), `back` (⌘[, hanya saat detail terbuka), dan `copyConnectionString` (action yang sama dengan item More dan baris Connection inspector), dengan target detail mengalahkan pilihan tabel (spec 0069 AC-11, AC-12). Aturan klik ulang sidebar diisi lewat hook `onReselect(section)` di `AppShell.select(_:)` (spec 0069 AC-7), dan ketukan pada baris sidebar yang sudah terpilih juga memanggil `select(_:)` (misalnya lewat `simultaneousGesture` pada baris, karena `List(selection:)` tidak memanggil setter untuk nilai yang sama), sehingga klik ulang dan ⌘2 di detail sama sama kembali ke tabel. Tooltip tombol More dan aksi utama memakai `CommandMap.helpText(for:)`. Item More tidak memasang shortcut.
- **Spec 0022**:
  1. Baris 312 (`InstancesCommands`): mengganti dengan "perintah Sort Instances By (tujuh baris `sortInstances…`) dan Show/Hide Inspector ⌥⌘I adalah baris `CommandMap` spec 0069 yang diterbitkan halaman Instances lewat `.publishesCommands(.page(.instances), …)`; tidak ada `InstancesCommands`. Kalau milestone 3 dibangun sebelum milestone 2 spec 0069, `InstancesCommands` diberi penanda `// provisional: F69`." Kalimat "Pencarian ⌘F memakai perilaku standar `.searchable` sampai fitur 69" menjadi "halaman menerbitkan `find` dengan `.searchFocused`".
  2. *Aksi instance*: tambahkan pemetaan aksi utama ke `startInstance` dan `stopInstance` dari *Penerbit* spec 0069, dibangun di langkah 7 (atau langkah 6 spec 0069 kalau langkah 7 lebih dulu di merge).
  3. Toolbar (baris 205): tombol inspector tidak memasang `.keyboardShortcut`; tooltip nya `CommandMap.helpText(for: .toggleInspector, title:)`.
  4. AC-13: "aksi instance di Instances dijalankan lewat tombol utama inspector" tambahkan "atau menu Instance (Start ⌘R, Stop ⇧⌘R)".

### Jawaban untuk follow up spec lain

| Follow up | Dijawab oleh |
| --- | --- |
| 0013: daftarkan perintah reconnect ke `DaemonModel.reconnect()` dan perintah pindah bagian sidebar | AC-7, AC-8 |
| 0016: daftarkan "Welcome to Stackly" di peta perintah, dan nonaktifkan perintah shell selama onboarding tampil | AC-4, AC-13 |
| 0017 baris 410: ⌘F dan pindah bagian lewat peta perintah | AC-7, AC-10 |
| 0020: daftarkan New Instance… (⌘N) di peta perintah dan hapus `CommandGroup` sementara | AC-9, AC-16 |
| 0021 baris 90: komponen dasar dan peta perintah | AC-11, AC-12, AC-14 |
| 0022: daftarkan perintah sort, inspector, dan ⌘F di peta perintah | AC-10, AC-12 |
| Usulan fondasi spec 0013 butir 5 dan P20 (`.commands` ⌘N, `ShellNavigator`) | AC-7, AC-9, usulan spec 0013 butir 3 dan 4 |
| Menu Help membuka dokumentasi fitur 66 | AC-13 (tanpa item sampai fitur 66; ⌘? dicadangkan) |

## Follow-up

- [ ] `/architect (update)` spec 0013, 0016, 0017, 0020, 0021, dan 0022 dengan *Usulan update untuk spec lain* (untuk 0013, digabung ke update fase 02 yang masih terbuka).
- [ ] `docs/scope/ui-macos.md` *Selisih yang sudah diketahui* (lewat `/scope`): menu bar mendapat menu Instance di antara View dan Window, di luar enam menu yang digambar mockup; Help tidak berisi Stackly Help sampai fitur 66.
- [ ] Fitur 24, 26, dan 54: ubah baris `duplicateInstance`, `restartInstance`, `reloadInstance`, dan `removeInstance` menjadi built, tambah `CommandItem`, dan terbitkan action dari halaman Instances; Remove dan Force Stop lewat konfirmasi tanpa shortcut.
- [ ] Fitur 66: putuskan tujuan dokumentasi (Help Book atau URL), lalu ubah baris `stacklyHelp` (⌘?) menjadi built sebagai item pertama Help.
- [ ] Fitur 32: Stop All Services… dan Check for Updates… (fitur 61) di menu Stackly tanpa shortcut bila ditambahkan ke menu utama; popover menu bar memakai judul dan shortcut dari peta ("Settings… ⌘,", "Close Window ⌘W").
- [ ] Fitur 27, 34, 35, 37, 44, 46, 47, 52, 56: putuskan perintah menu masing masing dengan aturan *Menambah perintah* dan usulan tempat di spec ini.
- [ ] Fitur 68: XCUITest menu berulang membaca `CommandMap` (baris built ada di menu dengan judul benar, aktif per skenario `connected-empty`, `connected-mixed`, `daemon-offline`, `onboarding`), plus smoke `typeKey` untuk ⌘,, ⌘N, ⌘1.
- [ ] `/sync` setelah build: folder `Commands/` dan aturan *Menambah perintah* di `apps/macos/AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, dan catatan cross check ada di [rationale.md](rationale.md).
