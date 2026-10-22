# Verify: Menu aplikasi, perintah & shortcut keyboard · spec 0069 · updated 2026-10-21

_Langkah diturunkan dari acceptance criteria spec 0069. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27. Probe memakai accessibility API dan event tombol ke proses app, jadi proses pemeriksa butuh izin Accessibility; jangan mengubah System Settings._

## Milestone 1: Command map and probe

### UI / manual

- [x] Buka `apps/macos/design.md` bagian "Commands and shortcuts" → tabel hasil probe P1 sampai P7 dan P3b memuat versi macOS 27.0.1 (26A434) dan Xcode 27.0 (27A266a), dan setiap baris punya hasil dan akibatnya untuk spec → AC-2, AC-3
- [x] Bangun app Debug (`cargo xtask build-app`), jalankan dengan `STACKLY_ROOT` ke folder sementara, lalu dump semua menu bar lewat accessibility API (`AXMenuItemCmdChar`, `AXMenuItemCmdModifiers`) → tidak ada dua item dengan key equivalent yang sama; selain tabel A hanya ⌥⌘Q (Quit and Keep Windows) dan keluarga 🌐 (🌐D, 🌐E, 🌐F, 🌐⌃F, 🌐⌃C, 🌐⌃R) yang tampil, dan ⌥⌘Q ada di `SystemShortcuts.blocked` → AC-2
- [ ] Ulangi probe P2 sampai P7 dan P3b di app stand in atau di app Stackly setelah milestone 2 → semuanya sama dengan hasil di `design.md`; kalau macOS atau Xcode naik versi, catat versi baru → AC-2, AC-3, AC-10

### Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter CommandMapTests` → semua test peta lulus: satu baris untuk setiap `CommandID` dengan raw value yang tetap, shortcut persis tabel *Peta perintah*, tidak ada dua baris dengan shortcut yang sama (built dan planned), tidak ada baris di tabel A atau aturan D, baris di tabel B hanya dengan makna sama atau `exception`, baris di tabel C hanya dengan `exception`, akhiran "…" sama dengan `needsInput`, tabel A sampai C tidak bentrok satu sama lain, tampilan "⇧⌘R" dan "⌘?", `helpText` dengan dan tanpa shortcut, dan `layout(menu:submenu:)` untuk status awal (kosong) dan untuk peta dengan semua baris built (kelompok berurut, tanpa kelompok kosong) → AC-1, AC-2
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter CommandSourceRuleTests` → aturan sumber lulus: `.keyboardShortcut(` dengan argumen selain `.defaultAction`, `.cancelAction`, atau `CommandMap`, dan `CommandGroup`, `CommandMenu`, `.commands` tidak muncul di luar `Commands/` dan `StacklyApp.swift`; setiap contoh terlarang tertangkap; `CommandID` built dipakai tepat satu kali oleh `CommandItem` (system oleh scene), planned tidak dipakai → AC-1
- [x] Ubah sementara satu shortcut di `CommandMap.swift` ke ⌘Q, jalankan `swift test ... --filter CommandMapTests` → gagal di `noRowBreaksAShortcutRule`; kembalikan → AC-2
- [x] Tambah sementara `.keyboardShortcut("x")` di salah satu file `Components/` atau halaman, jalankan `--filter CommandSourceRuleTests` → gagal di `sourcesFollowTheRule`; kembalikan → AC-1
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` → seluruh suite lulus (test Swift integration berakhir `BLOCKED` kalau `stacklyd` dan `skeleton_seed` belum dibangun) dan `cargo xtask build-app` berhasil → AC-1, AC-2
- [x] `xcrun swift-format lint --recursive apps/macos/Packages/StacklyUI/Sources/StacklyUI/Commands apps/macos/Packages/StacklyUI/Tests/StacklyUITests/Commands` → tidak ada peringatan → AC-1

### Hasil /check verify milestone 1 · 2026-10-21 · PASS (satu langkah belum dijalankan)

Diperiksa di macOS 27.0.1 (26A434), Xcode 27.0 (27A266a), dari merge `524ec3f`. Semua langkah yang bertanda `[x]` dijalankan ulang oleh checker, bukan disalin dari laporan developer.

- Langkah 1: `design.md` memuat versi 27.0.1 (26A434) dan 27.0 (27A266a), dan tiap baris P1 sampai P7 dan P3b punya kolom hasil dan akibat.
- Langkah 2: app Debug dari `cargo xtask build-app` dijalankan langsung dengan `STACKLY_ROOT` ke folder sementara (dihapus sesudahnya), menu bar didump lewat AX (`AXMenuItemCmdChar`, `AXMenuItemCmdModifiers`). 22 item ber-key equivalent di luar menu Apple, tidak ada pasangan yang sama. Di luar tabel A hanya ⌥⌘Q (Quit and Keep Windows, ada di `SystemShortcuts.blocked`), keluarga 🌐 (bit 16 pada `AXMenuItemCmdModifiers`: 🌐D, 🌐E, 🌐F, 🌐⌃F Fill, 🌐⌃C Center, 🌐⌃R, dan juga 🌐⌃ dengan panah serta kombinasi Arrange di Window > Move & Resize), dan item Help bawaan "Stackly Help ⌘?" yang akan diganti `CommandGroup(replacing: .help)`. Dump: scratchpad sesi `ax/p1-dump.txt`.
- Langkah 3 (ulang P2 sampai P7 dan P3b) tetap `[ ]`: checker mengulang P1, P2, P3, dan P3b di app Debug sungguhan dengan perubahan sementara di `StacklyApp.swift` (`.commands` dengan `replacing: .newItem`, `.textEditing`, `.help`, `CommandMenu("Instance")`, dan `.keyboardShortcut("0")` pada `Window`; dikembalikan lewat `git checkout`, working tree bersih). Hasilnya sama dengan `design.md`: File berisi New Instance… ⌘N, Close ⌘W, Close All tanpa "New Window"; Edit berisi Find ▸ Find… ⌘F dan item sistem; Help berisi kolom pencarian sistem dan Welcome to Stackly; menu Instance duduk di antara View dan Window; Window > Stackly ⌘0 membuka jendela lagi sesudah ⌘W (lewat tombol dan lewat menu) dan mengembalikannya dari minimize. P3b diulang tanpa `replacing: .textEditing`: menu Edit tanpa item Find, jadi rencana `replacing: .textEditing` benar. P4 sampai P7 (focused value, sheet, `.searchFocused`, shortcut di field pencarian, state basi) tidak bisa diulang di app sungguhan karena shell belum punya `.searchable`, `focusedSceneValue`, atau `StacklyCommands`; hasil developer di app stand in diterima sebagai dasar milestone 1 dan langkah ini diulang di milestone 2 langkah 5.
- Langkah 4: `CommandMapTests` 25 test lulus (53 kasus argumen pada `aRowOnABlockedShortcutIsRefused`). Langkah 5: `CommandSourceRuleTests` 9 test lulus.
- Langkah 6: shortcut Overview diubah ke ⌘Q → `noRowBreaksAShortcutRule` dan `rowsCarryTheShortcutsOfTheSpecTable` gagal; dikembalikan.
- Langkah 7: `.keyboardShortcut("x")` ditambah di `Shell/UnbuiltPage.swift` → `sourcesFollowTheRule` gagal pada aturan "keyboard shortcut outside the command map"; dikembalikan. (Folder `Components/` tidak ada; `Shell/` dipakai.)
- Langkah 8: `swift test --package-path apps/macos/Packages/StacklyUI` → 349 test di 40 suite lulus tanpa `BLOCKED`; `cargo xtask build-app` → BUILD SUCCEEDED. Langkah 9: `swift-format lint` bersih.
- Catatan, bukan kegagalan: daftar 🌐 di `design.md` baris P1 tidak menyebut item panah 🌐⌃ dan kombinasi Arrange di Window > Move & Resize; aturan E sudah mencakupnya. Semua 30 baris peta `planned` seperti yang direncanakan (`everyRowStartsPlannedUntilItsMenuItemExists`).

### Acceptance-criteria coverage

- AC-1 · bagian peta dan aturan sumber: `CommandMap`, `CommandEntry`, `layout`, `helpText`, dan aturan `.keyboardShortcut`, `CommandGroup`, `CommandMenu`, `.commands`, serta pemakaian `CommandID`; seluruh baris berstatus `planned` karena `CommandItem` dan `StacklyCommands` baru ada di milestone 2, yang juga mengubah status baris menjadi `built` dan menambah cek "tidak ada `provisional: F69`" (AC-16)
- AC-2 · bagian test unit atas `CommandMap` dan `SystemShortcuts` dan hasil probe P1 di app Stackly saat ini; probe atas menu bar yang sudah memakai peta (setiap baris built tampil dengan shortcut persis peta) menunggu milestone 2 langkah 5
- AC-3 sampai AC-16 · belum, milik Milestone 2 dan 3 (probe P2 sampai P7 sudah memberi dasarnya untuk AC-3, AC-4, AC-5, AC-10, AC-13)

## Milestone 2: Shell commands

Langkah build 3 sampai 5 dari spec 0069. `newInstance` (menunggu langkah 14 spec 0020) dan `welcome` (menunggu langkah 2 milestone 1 spec 0016) belum built, jadi cek yang bergantung pada keduanya tercatat `NOT_RUN` dan pindah ke saat fitur itu di merge. Record evidence ada di `evidence/manual/2026-10-21-spec-0069-manual-check-*.json` (kind `fixture`: aturan `evidence check` menolak `native` PASS tanpa subject runtime, dan pemeriksaan GUI tidak punya subject).

### UI / manual

- [x] Bangun app Debug (`cargo xtask build-app`), jalankan `target/debug/stacklyd` dengan `STACKLY_ROOT` ke folder sementara dan app dengan root yang sama, dump semua menu lewat accessibility API (`AXMenuItemCmdChar`, `AXMenuItemCmdModifiers`) saat jendela utama key → Stackly berisi About, Settings… ⌘,, Reconnect to stacklyd, Services, Hide, Quit; Edit berisi Find ▸ Find… ⌘F tanpa Spelling, Substitutions, Transformations, atau Speech; View berisi item sidebar lalu Overview ⌘1 sampai Diagnostics ⌘7; Window berisi Stackly ⌘0; Help hanya kolom pencarian sistem; tidak ada "New Window", tidak ada Stackly Help bawaan, tidak ada dua key equivalent yang sama → AC-2, AC-3 (record check 1, PASS; File dan Instance belum tampil karena baris plannednya)
- [x] Dengan jendela utama key tekan ⌘1 sampai ⌘7 → bagian sidebar berganti satu per satu; ⌘W menutup jendela utama dan notice tampil; ⌘0 membukanya lagi; ⌘M lalu ⌘0 mengembalikannya; ⌘, membuka Settings, lalu ⌘2 tidak melakukan apa pun sampai ⌘0 mengembalikan jendela utama → AC-4, AC-7, AC-13 (record check 2, PASS)
- [ ] Help > Welcome to Stackly: ⌘1 dan ⌘N nonaktif selama onboarding, ⌘, dan ⌘0 tetap bekerja, dan sesudah onboarding perintah shell aktif lagi → AC-4, AC-13 (record check 3, `NOT_RUN`: baris `welcome` planned sampai langkah 2 milestone 1 spec 0016 di merge)
- [x] Hentikan `stacklyd` → Reconnect to stacklyd aktif tanpa subtitle dan banner jendela menjelaskan alasannya; jalankan `stacklyd` lagi lalu pilih Reconnect → jendela tersambung dan item nonaktif lagi → AC-5, AC-8 (record check 4, PASS)
- [ ] Dengan `stacklyd` berhenti, File > New Instance… (⌘N) nonaktif tanpa subtitle; saat onboarding dan saat jendela utama bukan key window juga nonaktif → AC-5, AC-9 (record check 4b, `NOT_RUN`: baris `newInstance` planned sampai langkah 14 spec 0020 di merge)
- [ ] Ulangi probe P4 (sheet dan `NSAlert` sheet terbuka di jendela utama), P5 (`.searchFocused`), P6 (⌘R, ⇧⌘R, ⌥⌘R, ⌘O, ⌘D, ⌘1, ⌘[ di field pencarian), dan P7 (state berubah sekitar penekanan tombol) di app Stackly → belum bisa: shell belum punya sheet, `.searchable`, atau perintah halaman. Yang sudah diulang di app Stackly: P2 (⌘0 membuka jendela sesudah ⌘W dan dari minimize), P3 (File tanpa New Window, Edit tanpa submenu ejaan, Help tanpa item bawaan, Instance belum tampil), P3b (tidak berlaku, rencana tetap), dan bagian P4 untuk Settings dan jendela lain (⌘2 nonaktif saat Settings key). Sisanya jatuh ke milestone 3 langkah 7 dan ke fitur 20 (sheet) → AC-2, AC-3, AC-10
- [ ] VoiceOver: buka menu Stackly, View, Edit dan Window dengan ⌃F2 → setiap item terbaca dengan judul, shortcut, dan "dimmed" saat nonaktif (`NOT_RUN`, butuh VoiceOver; langkah user) → AC-15

### Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter CommandItemStateTests` → resolve untuk scope `.shell` dan `.page`, key window true dan false, penerbit ada dan nil, bagian cocok dan tidak, gate blocked, busy, alasan, judul dinamis, centang, `run` lewat host (konfirmasi memanggil `present`, bukan `perform`), dan penjaga yang menolak saat window tidak key lagi → AC-4, AC-5, AC-6
- [x] `swift test ... --filter MainWindowKeyTests` → `isKey` dan `isKeyNow` mengikuti notifikasi window sendiri saja (key, resign, close), notifikasi window lain tidak mengubahnya, `track` dua kali tidak menggandakan pengamat → AC-4, AC-5
- [x] `swift test ... --filter ShellCommandsTests` → `AppShell` menerbitkan ⌘1 sampai ⌘7 dengan urutan sidebar untuk setiap baris status daemon, Reconnect hanya tanpa koneksi (busy saat reconnecting, alasan "Relaunch Stackly after fixing the problem." saat misconfigured), tidak ada action yang mengubah data, dan `host.gate` yang diterbitkan sama dengan `presentation.mutationAvailability` untuk setiap baris status daemon pada `AppShell` yang dirender dalam window → AC-5, AC-7, AC-8
- [x] `swift test ... --filter CommandItemTests` dan `--filter CommandMapTests` → pemisah hanya di antara kelompok built, konversi shortcut (⌘? tampil "⌘?"), status built persis `reconnect`, `find`, `showOverview` sampai `showDiagnostics`, dan `showMainWindow` → AC-1, AC-3
- [x] `swift test ... --filter CommandSourceRuleTests` → setiap baris built dipakai tepat satu kali oleh `CommandItem` (`showMainWindow` oleh scene), planned tidak dipakai, dan tidak ada `.keyboardShortcut` literal atau `CommandGroup` di luar `Commands/` dan `StacklyApp.swift` → AC-1, AC-16
- [x] `grep -rn "provisional: F69" apps/` → kosong → AC-16
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` (dengan `STACKLY_DAEMON_BIN` dan `STACKLY_SEED_BIN`), `swift test --package-path clients/swift`, `cargo xtask build-app`, `xcrun swift-format lint --strict` atas file yang diubah, dan `cargo xtask evidence check` → semuanya lulus → AC-1 sampai AC-16

### Hasil /check verify milestone 2 · 2026-10-21 · PASS (empat langkah belum bisa dijalankan)

Diperiksa di macOS 27.0.1 (26A434), Xcode 27.0 (27A266a), dari merge `e2a8ce4`. Semua langkah yang developer tandai `[x]` dijalankan ulang oleh checker. App Debug dari `cargo xtask build-app` dijalankan langsung dengan `STACKLY_ROOT` ke folder sementara dan `stacklyd` dengan root yang sama (folder dan proses dibersihkan sesudahnya); menu dibaca lewat accessibility API (`AXMenuItemCmdChar`, `AXMenuItemCmdModifiers`, `AXEnabled`) dan tombol dikirim sebagai penekanan tombol nyata. Dump, jendela, dan screenshot ada di scratchpad sesi `ax/`.

- Langkah 1 (menu): Stackly berisi About, Settings… ⌘,, Reconnect to stacklyd, Services, Hide, Hide Others, Quit; Edit berisi Find ▸ Find… ⌘F tanpa Spelling, Substitutions, Transformations, atau Speech; View berisi Show Sidebar ⌃⌘S lalu Overview ⌘1 sampai Diagnostics ⌘7; Window berisi Stackly ⌘0 dan Design Gallery (Debug); Help hanya kolom pencarian sistem. Tidak ada "New Window", tidak ada Stackly Help, dan tidak ada pasangan key equivalent yang sama (`menus-2.txt`). File dan Instance belum tampil karena barisnya `planned`.
- Langkah 2 (keyboard): ⌘2 sampai ⌘7 lalu ⌘1 mengganti judul jendela Instances, Environments, Catalog, Runtimes, Operations, Diagnostics, Overview (`keys-1to7.txt`). ⌘W menutup jendela utama dan notice "Stackly is still running" tampil pada penutupan pertama (screenshot `after-cmdw.png`); item View jadi nonaktif; ⌘0 membuka jendela lagi; ⌘M lalu ⌘0 mengembalikannya dari minimize (`window-keys2.txt`: `minimized=true` lalu `false`, jendela utama key lagi); ⌘, membuka Settings, ⌘2 tidak mengubah jendela utama (tetap Overview) dan ⌘1 sampai ⌘7 nonaktif di menu, ⌘0 mengembalikan jendela utama, lalu ⌘3 bekerja lagi.
- Langkah 4 (Reconnect): saat terhubung item nonaktif. Sesudah `stacklyd` dihentikan item aktif tanpa subtitle dan banner "Can't connect to stacklyd." menjelaskan alasannya (`offline.png`); ⌘1 sampai ⌘7 tetap aktif karena tidak mengubah data. Sesudah `stacklyd` dijalankan lagi dan Reconnect dipilih dari menu, jendela tersambung (pid daemon baru di Connection, banner hilang, `reconnected.png`) dan item nonaktif lagi.
- Langkah 5 (probe, sebagian): P4 untuk jendela lain diulang di app Stackly: saat Design Gallery key, ⌘3 tidak mengganti bagian jendela utama dan ⌘1 sampai ⌘7 nonaktif, Window > Stackly ⌘0 tetap aktif (`gallery.png`). ⌘Q tetap keluar saat Design Gallery key. Sisa P4 (sheet dan `NSAlert` sheet terbuka di jendela utama), P5, P6, dan P7 belum bisa diulang: jendela utama belum punya sheet, `.searchable`, atau perintah halaman (`grep` atas `Sources` dan `Stackly`: tidak ada `.searchable` atau `.sheet` di shell, `focusedSceneValue` hanya di `PublishedCommands`). Sheet dan dialog hanya ada di Design Gallery, yang jendela terpisah. Langkah tetap `[ ]`, jatuh ke milestone 3 langkah 7 dan fitur 20.
- Langkah 3 (Welcome) dan langkah 4b (New Instance, ⌘N): `di luar lingkup`, barisnya `planned` jadi belum ada item di menu (menu File dan item Help Welcome tidak tampil). Tetap `NOT_RUN`. VoiceOver (⌃F2) `NOT_RUN`, langkah user; VoiceOver tidak dinyalakan.
- Perintah: `CommandItemStateTests` 20 test, `MainWindowKeyTests` 7, `ShellCommandsTests` 12, `CommandItemTests` 8, `CommandMapTests` 25, `CommandSourceRuleTests` 9, semuanya lulus. `grep -rn "provisional: F69" apps/` kosong. `swift test --package-path apps/macos/Packages/StacklyUI` (dengan `STACKLY_DAEMON_BIN` dan `STACKLY_SEED_BIN`) 540 test di 57 suite lulus tanpa `BLOCKED`, termasuk `theInstanceStoreFollowsChangesAnotherClientMakes` (tidak gagal di run ini); `swift test --package-path clients/swift` 154 test lulus; `cargo xtask build-app` BUILD SUCCEEDED; `swift-format lint --strict` atas `Commands/`, `Shell/`, test `Commands/`, dan `StacklyApp.swift` bersih; `cargo xtask evidence check` lulus (44 record).
- Catatan, bukan kegagalan: saat daemon baru dijalankan lagi, halaman Diagnostics masih menampilkan "Last error: ipc.endpoint_missing" dari percobaan lama selagi status sudah tersambung (bagian Connection spec 0013, bukan lingkup F69). Status daemon "degraded" karena catalog tidak ada di root sementara, juga bukan lingkup F69.

### Acceptance-criteria coverage

- AC-1 · `CommandSourceRuleTests`, `CommandMapTests`, dan `CommandItemTests` (milestone 1 dan 2)
- AC-2 · record check 1 (menu bar app nyata) dan test peta; probe lengkap di app Stackly menunggu milestone 3
- AC-3 · record check 1 dan `theMenusShowOnlyTheBuiltRows`; File dan Instance tampil sejak baris pertamanya built (fitur 20, 21, 22), Help dengan Welcome sejak spec 0016 dan Stackly Help sejak fitur 66
- AC-4 · `CommandItemStateTests`, `MainWindowKeyTests`, record check 2; onboarding (check 3) dan sheet menunggu spec 0016 dan 0020
- AC-5 · `CommandItemStateTests`, `ShellCommandsTests` (gate per status daemon), record check 4; `newInstance` (4b) menunggu spec 0020
- AC-6 · `CommandItemStateTests` (konfirmasi lewat host palsu); dialog nyata menunggu aksi dengan `ActionConfirmation` (fitur 26, 54)
- AC-7 · `ShellCommandsTests` dan record check 2; hook `onReselect` ada di `AppShell.select(_:)`, diisi spec 0021
- AC-8 · `ShellCommandsTests` dan record check 4
- AC-9 · menunggu langkah 14 spec 0020 (`newInstance` planned)
- AC-10 · menunggu milestone 3 (belum ada halaman yang menerbitkan `find`; Find… tampil nonaktif)
- AC-11, AC-12, AC-14 · menunggu milestone 3
- AC-13 · ⌘0 dan Settings di record check 2; Welcome to Stackly menunggu spec 0016, Stackly Help menunggu fitur 66
- AC-15 · menunggu milestone 3 langkah 7 dan VoiceOver
- AC-16 · bagian "Commands and shortcuts" di `design.md` (struktur menu, peta, shortcut sistem, aturan menambah perintah, hasil probe) dan `grep` kosong; tidak ada `CommandGroup` atau `.commands` di luar `Commands/` dan `StacklyApp.swift` (source rule)

## Milestone 3: Page and instance commands

Langkah build 6 dan 7 dari spec 0069. Saat milestone ini dimulai, sebagian besar penerbit sudah ada di `main`: `CommandMenu("Instance")` dan `InstancesCommandActions` datang dengan fitur 22 dan 21 (`InstancesCommands` sudah tidak ada), Catalog menerbitkan `find` sejak fitur 17, dan baris `showOverview` sampai `welcome` sudah built. Yang milestone ini kerjakan: baris `back`, `openInstance`, dan `copyConnectionString` menjadi `built` di tabel `design.md` (di `CommandMap` sudah built sejak fitur 21), Find tidak diterbitkan selama detail instance terbuka (AC-10), tooltip tombol aksi utama inspector lewat `CommandMap.helpText` (AC-14), dan test penerbit. Evidence manual (langkah 7) adalah langkah user dan tercatat `NOT_RUN`.

### UI / manual

- [ ] Hanya keyboard, dengan `stacklyd` dan app Debug yang memakai `STACKLY_ROOT` sementara yang sama (`skeleton_seed` menyiapkan root; jangan pakai Application Support asli): ⌘N lalu isi sheet dan Return membuat instance; ⌘2, Tab ke tabel, panah ke baris, ⇧⌘R menghentikannya; ⌘R menjalankannya; ⌘O membuka detail; ⇧⌘C menyalin connection string; ⌘[ kembali ke tabel; ⌥⌘I menyembunyikan dan menampilkan inspector; View > Sort Instances By mengurutkan dan menandai kolom dan arah aktif; saat sheet New Instance terbuka ⌘1 dan ⌘R tidak melakukan apa pun → AC-4, AC-9, AC-11, AC-12, AC-15 (cek manual 5, `NOT_RUN`: langkah user)
- [ ] ⌘F di Instances dan Catalog memfokuskan field pencarian toolbar; Edit > Find > Find… nonaktif di Overview, selama detail instance terbuka, dan di halaman tanpa pencarian; menekan ⌘F saat field sudah fokus tidak mengubah teksnya → AC-10 (cek manual 5, `NOT_RUN`: langkah user)
- [ ] Probe P4 (sheet dan `NSAlert` sheet terbuka di jendela utama: shortcut halaman tidak jalan), P5 (`.searchFocused`), P6 (⌘R, ⇧⌘R, ⌥⌘R, ⌘O, ⌘D, ⌘1, ⌘[ di field pencarian toolbar sampai ke menu tanpa mengubah teks), dan P7 (state berubah sekitar penekanan tombol) diulang di app Stackly sekarang setelah shell punya sheet, `.searchable`, dan perintah halaman. Sisa langkah ke-6 milestone 2 → AC-2, AC-3, AC-10 (`NOT_RUN`: langkah user)
- [ ] Skenario fixture: dengan `STACKLY_UI_SCENARIO=connected-mixed`, menu Instance aktif dan nonaktif sesuai baris yang dipilih (Start untuk instance berhenti, Stop untuk instance berjalan, Stop nonaktif dengan alasan untuk instance `unknown`, semua nonaktif tanpa pilihan); dengan `daemon-offline`, Start, Stop, dan New Instance… nonaktif tanpa subtitle karena gate, sedangkan Open, Copy Connection String, Back, Find, urutan, dan inspector tetap mengikuti ada tidaknya target (record oleh developer di laporan sesi, bukan record evidence) → AC-5, AC-11, AC-12 (`BLOCKED` di sesi developer 2026-10-22: layar terkunci, `CGSSessionScreenIsLocked` true, jadi app Debug tidak dijalankan; yang tercakup tanpa layar: `ShellCommandsTests` per baris status daemon untuk gate dan `InstancesPageTests` per lifecycle untuk Start dan Stop)
- [ ] VoiceOver: buka menu Instance, View, dan Edit dengan ⌃F2 → setiap item terbaca dengan judul, shortcut, dan "dimmed" saat nonaktif; ⌘F di Instances dan Catalog memfokuskan field pencarian dan VoiceOver membacakannya; tombol Start, Stop, dan Show atau Hide Inspector terbaca dengan tooltip yang menyebut shortcut (cek manual 6, `NOT_RUN`: langkah user, VoiceOver tidak dinyalakan oleh developer) → AC-10, AC-14, AC-15

### Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter InstancesPageTests` → penerbit Instances: Find, inspector, tujuh baris urut (centang untuk kolom dan arah aktif, sort mempertahankan arah dan kolom lainnya), Start atau Stop dengan alasan dan busy yang sama dengan tombol aksi utama, Open, Back, dan Copy Connection String hanya saat targetnya ada, Find tidak diterbitkan selama detail terbuka, dan tooltip inspector menyebut shortcut → AC-10, AC-11, AC-12, AC-14
- [x] `swift test ... --filter InstanceDetailCommandsTests` → penerbit target detail (Open, Back, Copy Connection String, tooltip aksi utama detail) → AC-11, AC-12, AC-14
- [x] `swift test ... --filter CommandMapTests`, `--filter CommandSourceRuleTests`, `--filter ShellCommandsTests`, dan `--filter ShellStructureTests` → tetap hijau; status built persis sama dengan `design.md` (peta tidak berubah di milestone ini) dan tidak ada `.keyboardShortcut` literal di view → AC-1, AC-2, AC-16
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` (dengan `STACKLY_DAEMON_BIN` dan `STACKLY_SEED_BIN`), `cargo xtask build-app`, `xcrun swift-format lint --strict` atas file yang diubah, dan `cargo xtask evidence check` → semuanya lulus

### Acceptance-criteria coverage

- AC-2 · test peta (milestone 1); probe di app Stackly dan cek manual 1 menunggu langkah user (`NOT_RUN`)
- AC-10 · `InstancesPageTests` (Find tidak diterbitkan selama detail terbuka), `CatalogPage` menerbitkan `find`; fokus nyata ke field `.searchable` menunggu cek manual (`NOT_RUN`)
- AC-11 · `InstancesPageTests` dan `InstanceDetailCommandsTests`; item Restart, Reload, Duplicate…, dan Remove… tampil sejak fitur 26, 24, dan 54 membangunnya
- AC-12 · `InstancesPageTests` (urut, inspector, Back) dan `InstanceDetailCommandsTests`
- AC-14 · `InstancesPageTests`, `InstanceDetailCommandsTests`, dan `CommandMapTests` (`helpText`)
- AC-15 · menunggu cek manual 5 dan 6 (`NOT_RUN`: keyboard saja dan VoiceOver)
