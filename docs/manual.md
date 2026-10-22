# Langkah manual yang menunggu kamu

File ini mengumpulkan semua langkah yang hanya bisa kamu kerjakan sendiri di Mac ini. Langkah seperti ini butuh tangan, mata, atau dialog macOS sungguhan. Semua pekerjaan otomatis sudah selesai. Begitu hasil langkah di bawah masuk, F11 dan F14 di [scope/01-foundation.md](scope/01-foundation.md) bisa ditutup, dan F12, F13, F67, F68, serta F69 di [scope/ui-macos.md](scope/ui-macos.md) bisa diverifikasi penuh.

Kamu boleh mengerjakannya kapan saja dan tidak harus sekaligus. Setiap bagian berdiri sendiri. Kirim hasilnya ke Claude per bagian. Claude yang mengisi record di `evidence/manual/` dan mencentang `verify.md`, jadi kamu tidak perlu menyunting file apa pun.

## Ringkasan

| Bagian | Untuk | Kira kira | Sifat |
| --- | --- | --- | --- |
| [E. Preview komponen secret di Xcode](#e-preview-komponen-secret-di-xcode) | F11 milestone 6 | 5 menit | wajib untuk F11 |
| [A. Deny dan Cancel di login Keychain](#a-deny-dan-cancel-di-login-keychain) | F11 milestone 4 | 10 menit | wajib untuk F11 |
| [B. Alur GUI secret](#b-alur-gui-secret) | F11 milestone 6 | 25 menit, ditambah 10 menit bila langkah 12 opsional diikutkan | wajib untuk F11 |
| [C. Walking skeleton lewat CLI dan GUI](#c-walking-skeleton-lewat-cli-dan-gui) | F14 milestone 4, AC-001 | 20 menit | wajib untuk F14 |
| [D. Keychain terkunci terlihat di app](#d-keychain-terkunci-terlihat-di-app) | F14 milestone 5 | 5 menit, tepat sesudah C | wajib untuk F14 |
| [F. Keyboard dan VoiceOver di shell app](#f-keyboard-dan-voiceover-di-shell-app) | F13 di [scope/ui-macos.md](scope/ui-macos.md) | 15 menit, bisa bersamaan dengan C | di luar 01-foundation |
| [I. Setting aksesibilitas sistem di Design Gallery](#i-setting-aksesibilitas-sistem-di-design-gallery) | F12 `Verify it`, F67, F68 milestone 1 di [scope/ui-macos.md](scope/ui-macos.md) | 20 menit | wajib untuk F12 dan F67 |
| [J. Keyboard navigation dan VoiceOver di galeri dan menu](#j-keyboard-navigation-dan-voiceover-di-galeri-dan-menu) | F67 dan F69 di [scope/ui-macos.md](scope/ui-macos.md) | 20 menit, bisa bersamaan dengan F | wajib untuk F67 dan F69 |
| [K. Automation Mode dan UI test](#k-automation-mode-dan-ui-test) | F68 milestone 3 di [scope/ui-macos.md](scope/ui-macos.md) | 10 menit, butuh password admin | wajib untuk F68 |
| [G. Dua akun macOS](#g-dua-akun-macos) | F6 (sudah done) | 30 menit, butuh password admin | opsional |
| [H. Folder temp setelah beberapa hari idle](#h-folder-temp-setelah-beberapa-hari-idle) | F6 (sudah done) | beberapa hari menunggu | opsional |

Urutan yang aku sarankan: E dulu karena paling singkat, lalu A, lalu B. Setelah itu kerjakan C, D, dan F sekali jalan, karena ketiganya memakai root dan jendela app yang sama. Untuk `ui-macos`: F dan J sekali jalan (keduanya menyalakan Keyboard navigation dan VoiceOver), lalu I, lalu K.

## Persiapan umum

- Kerjakan dari root repo di checkout utama, `/Users/ojiepermana/Development/ojiepermana/Stackly`, di `main` yang bersih (minimal `970b05d`). Cek dengan `git status --short`; hasilnya harus kosong.
- Pakai Terminal sungguhan di sesi login kamu (bukan lewat SSH), supaya dialog Keychain dan jendela app bisa muncul.
- Gunakan hanya nilai fixture yang disebut di bawah. Jangan pernah memakai password asli.
- Bila satu langkah tidak sesuai, lanjutkan saja ke langkah berikutnya, lalu catat nomornya dan apa yang kamu lihat.

## A. Deny dan Cancel di login Keychain

Untuk F11 langkah 7 (AC-3). Record: `evidence/manual/2026-10-20-spec-0011-step-7-deny-and-cancel-on-the-login-ke.json` dan `evidence/manual/2026-10-20-spec-0011-step-7-keychain-dialog-after-an-ad-hoc.json`.

Bagian ini memakai root default (`~/Library/Application Support/Stackly`, dibuat bila belum ada), dan item uji masuk ke login Keychain. Langkah 7 menghapusnya lagi.

1. Bangun daemon dan CLI: `cargo build -q -p stacklyd -p stackly-cli`
2. Di Terminal A, jalankan `cargo run -q -p stacklyd` dan biarkan tetap jalan.
3. Di Terminal B, jalankan `cargo run -q -p stackly-cli -- secret set local:user/manual-keychain-check --generate`. Perintah harus berakhir dengan exit 0. Catat bila muncul dialog.
4. Di Terminal A, tekan Ctrl-C, lalu jalankan `touch apps/daemon/src/main.rs && cargo build -q -p stacklyd && cargo run -q -p stacklyd`. Build ulang ini mengubah tanda tangan ad hoc, sehingga Keychain bertanya lagi.
5. Di Terminal B, jalankan `cargo run -q -p stackly-cli -- secret reveal local:user/manual-keychain-check` sebanyak tiga kali:
   1. Saat dialog muncul, klik Deny.
   2. Saat dialog muncul, klik Cancel atau tekan Escape.
   3. Tambahkan `</dev/null | cat` di akhir perintah. Perintah harus langsung gagal, tanpa dialog.

   Setiap kali, catat teks dialog, tombolnya, exit code, dan kode error yang tercetak.
6. Jalankan `cargo run -q -p stackly-cli -- secret status local:user/manual-keychain-check`, lalu `cargo run -q -p stackly-cli -- secret list`. Keduanya harus berjalan tanpa dialog dan menampilkan status `present`.
7. Bersihkan:
   - `cargo run -q -p stackly-cli -- secret delete local:user/manual-keychain-check`
   - `security delete-generic-password -s stackly-secret -a user/manual-keychain-check` (abaikan pesan "could not be found")
   - Ctrl-C daemon di Terminal A.

**Yang dikirim:** hasil langkah 3, 5.1, 5.2, 5.3, dan 6, plus versi macOS.

## B. Alur GUI secret

Untuk F11 langkah 11 (AC-16, AC-19, AC-24). Record: `evidence/manual/2026-10-20-spec-0011-step-11-secret-entry-and-reveal-flows.json`.

Langkah 1 sampai 11 memakai root sementara dengan keychain file, jadi login Keychain tidak disentuh. Hanya langkah 12 yang opsional memakai login Keychain.

1. Di Terminal A, bangun daemon dan app: `cargo build -q -p stacklyd && cargo xtask build-app`. Tunggu sampai muncul `BUILD SUCCEEDED`.
2. Di Terminal A, buat root sementara: `export ROOT=$(mktemp -d /tmp/stk.XXXXXX); echo $ROOT`
3. Di Terminal A, jalankan daemon: `STACKLY_ROOT=$ROOT cargo run -q -p stacklyd 2>&1 | tee $ROOT.daemon.log`, lalu biarkan tetap jalan.
4. Di Terminal B, jalankan `export ROOT=<path dari langkah 2>`, lalu salin fixture credential ke pasteboard: `printf 'stackly-fixture-credential-%s' $(uuidgen | tr -d - | tr A-Z a-z) | pbcopy`
5. Di Terminal B, buka app: `STACKLY_ROOT=$ROOT target/xcode/Build/Products/Debug/Stackly.app/Contents/MacOS/Stackly`. Jendela utama harus tersambung ke daemon.
6. Buka Window > Design Gallery > Secrets, lalu:
   1. Klik Check Status.
   2. Klik Open Entry Sheet, pilih Enter, lalu ketik `short`. Harus muncul "Use at least 8 characters.", dan Save tidak aktif.
   3. Coba Show, lalu Hide.
   4. Tempel nilai dengan Cmd-V, klik Save, lalu Done.
   5. Klik Check Status lagi. Hasilnya harus `present`, revision 1.
   6. Buka sheet lagi, ketik `another-fixture-value-1`, klik Save, lalu Replace, lalu Cancel.
7. Di baris connection string, klik Show. Teks harus tersembunyi lagi sendiri setelah sekitar 30 detik; ukur waktunya. Teks juga harus tersembunyi saat kamu klik Hide, saat app kehilangan fokus, dan saat jendela Gallery ditutup lalu dibuka lagi.
8. Klik Copy, lalu uji pasteboard:
   1. `pbpaste` di Terminal B harus menampilkan URI lengkap.
   2. 90 detik setelah klik, `pbpaste` harus kosong.
   3. Klik Copy lagi, lalu langsung jalankan `echo something-else | pbcopy`. Setelah 90 detik, `pbpaste` harus tetap menampilkan `something-else`.
   4. Klik Copy sekali lagi, lalu keluar dari app dalam 90 detik. `pbpaste` harus kosong. Sesudahnya, buka app lagi seperti langkah 5.
9. Cek VoiceOver (Cmd-F5):
   - Teks tersamar dibaca "Password, hidden".
   - Tombol dibaca "Show Password", "Copy Password", lalu "Hide Password".
   - Saat nilai tampil, VoiceOver membaca nilainya sebagai value baris itu.
   - Setelah 30 detik terdengar "Password hidden".

   Lalu cek dengan keyboard saja (System Settings › Keyboard › Keyboard navigation menyala): Tab dan Space harus bisa menjangkau dan menjalankan semuanya, dan Escape membatalkan sheet. Terakhir, lihat halaman ini dengan Increase contrast menyala dan dengan lebar jendela paling sempit.
10. Pindai kebocoran:
    - `grep -rl --exclude-dir=keychain 'stackly-fixture-credential-' $ROOT $ROOT.daemon.log; echo exit=$?` harus menghasilkan `exit=1` tanpa nama file.
    - `log show --last 20m --predicate 'process == "Stackly"' | grep -c 'stackly-fixture-credential-'` harus menghasilkan `0`.
11. Bersihkan:
    - Klik Delete Reference…, lalu konfirmasi.
    - Keluar dari app dengan Cmd-Q.
    - Ctrl-C daemon di Terminal A.
    - `rm -rf $ROOT $ROOT.daemon.log`
12. Opsional: dialog Keychain asli (langkah 22 di record). Langkah ini memakai login Keychain dan folder Application Support yang asli, jadi jalankan hanya bila kamu tidak keberatan ada item `stackly-secret` sementara di sana.
    1. Di Terminal A, bangun ulang supaya tanda tangan ad hoc berubah dan Keychain bertanya lagi: `touch apps/daemon/src/main.rs && cargo build -q -p stacklyd`. Lalu jalankan daemon tanpa `STACKLY_ROOT`: `cargo run -q -p stacklyd`.
    2. Di Terminal B, buka app tanpa `STACKLY_ROOT`: `target/xcode/Build/Products/Debug/Stackly.app/Contents/MacOS/Stackly`. Buka Window > Design Gallery > Secrets, buka entry sheet, pilih Generate untuk `local:debug/gallery/password`, lalu Save. Klik Allow bila dialog muncul.
    3. Hentikan daemon, lalu ulangi touch, build, dan start seperti 12.1, supaya pembacaan berikutnya memunculkan dialog lagi.
    4. Klik Show di baris connection string. Saat dialog Keychain muncul, Stackly kehilangan fokus. Setelah kamu klik Allow, nilai tidak boleh tampil sendiri, dan baris menampilkan teks merah `Hidden because Stackly went to the background. Press Show to try again.` Klik Show lagi; nilai harus tampil tanpa dialog.
    5. Ulangi build sekali lagi, lalu klik Copy. Selagi dialog masih terbuka, jalankan `echo something-else | pbcopy` di Terminal B, lalu klik Allow. Baris harus menampilkan pesan merah `Not copied because the clipboard changed. Press Copy to try again.`, dan `pbpaste` tetap `something-else`.
    6. Ulangi build sekali lagi, klik Copy, lalu klik Allow tanpa menyentuh clipboard. Harus muncul `Copied. The clipboard clears in 90 seconds.`, dan `pbpaste` menampilkan URI lengkap.
    7. Bersihkan: Delete Reference…, konfirmasi, keluar dari app, hentikan daemon, lalu pastikan Keychain Access tidak menyimpan item `stackly-secret` yang tersisa. Bila dialog ternyata tidak membuat app kehilangan fokus di Mac ini, catat saja itu.

**Yang dikirim:** versi macOS, waktu terukur untuk cek 30 detik dan 90 detik, serta nomor dan apa yang terlihat untuk setiap langkah yang tidak sesuai. Untuk langkah 12, kirim hasilnya atau tulis "12 dilewati".

## C. Walking skeleton lewat CLI dan GUI

Untuk F14 langkah 10 (AC-001, AC-013, AC-029, spec 0014 AC-16). Record: `evidence/manual/2026-10-20-walking-skeleton-through-the-cli-and-gui.json`.

Bagian ini memakai root sementara, jadi login Keychain tidak disentuh. Siapkan tiga terminal: A untuk perintah, B untuk daemon, dan C untuk app.

1. Terminal A: `cargo build -p stacklyd -p stackly-cli -p stackly-fake-server && cargo build -p stacklyd --example skeleton_seed && cargo xtask build-app`
2. Terminal A: `export R="$(mktemp -d /tmp/stk-skel.XXXXXX)"; S=target/debug/stackly`, lalu `target/debug/examples/skeleton_seed --root "$R"`. Catat UUID di baris `runtime` sebagai RT.
3. Terminal B: `STACKLY_ROOT="$R" target/debug/stacklyd`. Biarkan tetap jalan.
4. Terminal C: `STACKLY_ROOT="$R" target/xcode/Build/Products/Debug/Stackly.app/Contents/MacOS/Stackly`, lalu pilih Instances. Footer harus menampilkan `stacklyd running`, dan halaman menampilkan "No Instances".
5. Terminal A: `STACKLY_ROOT="$R" $S instance create demo --runtime $RT --provider testing/fake --revision 0.3.0 --set-json readyDelayMs=3000 --wait`. Baris `demo` harus muncul dengan status Stopped tanpa diklik. Catat UUID-nya sebagai ID.
6. `STACKLY_ROOT="$R" $S instance start $ID --wait`. Baris harus berubah dari Starting ke Running (siap dalam sekitar 3 detik) tanpa diklik. Lalu jalankan `STACKLY_ROOT="$R" $S instance get $ID` dan catat PID-nya. Cek juga `ps -axww -o pid=,args= | grep "$R" | grep "stackly-fake-server serve" | grep -v grep`: hasilnya satu baris dengan PID yang sama.
7. Tutup jendela app dengan tombol merah. Notice "Stackly is still running" harus muncul, dan PID di `ps` serta `instance get` tidak berubah. Klik Stackly di Dock: jendela kembali dengan baris `demo` Running.
8. Tekan ⌘Q. `instance get $ID` harus tetap `running` dengan PID yang sama. Buka app lagi seperti langkah 4: baris `demo` harus Running.
9. Terminal B: tekan Ctrl+C. Banner "Can’t connect to stacklyd." harus muncul, baris `demo` tetap ada, dan proses fake masih hidup. Jalankan daemon lagi dengan perintah langkah 3: banner hilang, dan `instance get $ID` menunjukkan PID yang sama dengan status `adopted`.
10. Pastikan dulu dengan `pgrep -fl stacklyd` bahwa hanya daemon ini yang berjalan. Lalu jalankan `kill -9 $(pgrep -f target/debug/stacklyd)` dan jalankan daemon lagi. Hasilnya harus: PID sama, `adopted`, satu proses `serve`, dan tepat satu baris `demo`.
11. `STACKLY_ROOT="$R" $S instance create broken --runtime $RT --provider testing/fake --revision 0.3.0 --set-json failInit=true --wait` harus berakhir dengan exit 7 dan `supervisor.init_failed`, dan baris `broken` berstatus Failed. Catat operation ID-nya sebagai OP1, lalu jalankan `STACKLY_ROOT="$R" $S operation abandon $OP1 --yes`. Catat apakah baris `broken` masih ada.
12. `STACKLY_ROOT="$R" $S instance create slow --runtime $RT --provider testing/fake --revision 0.3.0 --set-json initDelayMs=20000`, tanpa `--wait`.
    - Tunggu sampai `ps -axww -o pid=,args= | grep "$R" | grep -v grep | grep "stackly-fake-server init"` menampilkan baris.
    - Begitu baris itu muncul, `kill -9` daemon, lalu jalankan lagi.
    - Hasilnya harus: satu baris `slow` berstatus Failed, `operation inspect <op>` menunjukkan `needs_recovery`, dan helper `init` hilang sendiri dalam 20 detik.
    - Terakhir, jalankan `operation abandon <op> --yes`.
13. `STACKLY_ROOT="$R" $S instance stop $ID --wait`. Baris `demo` harus menjadi Stopped tanpa diklik, dan `ps -axww -o pid=,args= | grep stackly-fake-server | grep -v grep` kosong.

Jangan bersihkan dulu bila kamu akan lanjut ke D dan F. Kalau tidak, bersihkan seperti langkah D.5.

**Yang dikirim:** versi macOS, plus nomor dan apa yang terlihat untuk setiap langkah yang tidak sesuai, atau "semua sesuai".

## D. Keychain terkunci terlihat di app

Untuk F14 milestone 5 (spec 0014 AC-19), yaitu satu baris di `docs/specs/0014-walking-skeleton/verify.md` milestone 5. Bagian CLI-nya sudah lulus; yang tersisa hanya pengamatan di jendela app. Jalankan tepat sesudah C, dengan root `$R` dan instance `demo` yang sama.

1. Pastikan `demo` berhenti: `STACKLY_ROOT="$R" $S instance stop $ID --wait`.
2. Kunci keychain file root itu: `security lock-keychain "$R/data/keychain/stackly.keychain-db"`.
3. `STACKLY_ROOT="$R" $S instance start $ID --wait </dev/null`. Perintah ini harus gagal dengan `secret.needs_secret`, tanpa dialog. Catat juga apakah pesannya menyebut reference yang perlu diisi. `STACKLY_ROOT="$R" $S instance get $ID` harus menampilkan `Blocker` `needs secret`. Di app, baris `demo` harus tetap Stopped.
4. Hentikan `stacklyd` (Ctrl+C di Terminal B), jalankan lagi, lalu ulangi start tanpa `</dev/null`. Start harus berhasil, dan baris `demo` di app menjadi Running.
5. Bersihkan (bila F tidak dikerjakan sekarang):
   - `STACKLY_ROOT="$R" $S instance stop $ID --wait`
   - Ctrl+C di daemon, ⌘Q di app.
   - Pastikan `ps -axww -o pid=,args= | grep -E "stackly|$R" | grep -v grep` kosong.
   - `rm -rf "$R"`

**Yang dikirim:** apakah baris app tetap Stopped di langkah 3, dan apakah start di langkah 4 berhasil.

## E. Preview komponen secret di Xcode

Untuk F11 milestone 6 (AC-19), yaitu satu butir UI di `docs/specs/0011-secret-store-redaction/verify.md` milestone 6.

1. Buka `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Secrets/SecretPreviews.swift` di Xcode.
2. Lihat keempat `#Preview`: Secret field, Secret field dark, Secret entry sheet, dan Revealable secret text.
3. Untuk tiap preview, catat tiga hal: apakah tampilannya terbaca di light dan dark, apakah status tidak hanya dibedakan lewat warna, dan apakah ada teks yang terpotong.

**Yang dikirim:** catatan per preview, atau "semua baik".

## F. Keyboard dan VoiceOver di shell app

Bagian ini untuk F13 di [scope/ui-macos.md](scope/ui-macos.md) (spec 0013 cek 6, langkah 16, dan cek 3; AC-8, AC-10, AC-13, AC-16, AC-17, AC-21; PRD REQ-027, NFR-012). Record: `evidence/manual/2026-10-19-spec-0013-manual-check-6-keyboard-and-voiceover.json`. Bagian ini bukan bagian dari 01-foundation, tapi paling hemat dikerjakan bersamaan dengan C, saat root `$R`, daemon, dan app masih menyala dengan instance `demo`.

Sudah dijalankan Claude lewat accessibility API pada 2026-10-21 (`a6baa41`): bagian "baris `demo` yang Running tetap Running" di butir 3 dan semua butir 4 kecuali pembacaan VoiceOver per baris. Yang masih butuh kamu: butir 1 sampai 3 (keyboard dan pengumuman VoiceOver) dan baris VoiceOver di butir 4. Keyboard navigation di Mac ini saat ini mati, jadi butir 1 memang perlu dinyalakan.

1. Nyalakan System Settings › Keyboard › Keyboard navigation, lalu VoiceOver (⌘F5).
2. Di app:
   - Tombol panah memindah pilihan di sidebar.
   - Tab masuk ke konten.
   - Tombol di banner, Settings (⌘,), Diagnostics, dan Reconnect bisa dipakai dari keyboard.
   - VoiceOver membaca label untuk setiap elemen.
3. Hentikan `stacklyd` (Ctrl+C di Terminal B), lalu nyalakan lagi. VoiceOver harus mengumumkan "Can’t connect to stacklyd." dan "Reconnected. State refreshed." masing masing tepat sekali. Selama daemon mati, baris `demo` yang Running tetap Running, lalu disegarkan setelah tersambung lagi (cek 3).
4. Di halaman Instances:
   - Satu baris per instance (nama, label provider, status), urut nama.
   - Subjudul "1 instance" atau "{n} instances", dan badge sidebar sama dengan jumlah itu.
   - `STACKLY_ROOT="$R" $S instance start $ID` (atau stop) mengubah status barisnya tanpa memuat ulang daftar.
   - VoiceOver membaca satu elemen per baris, misalnya "demo, (nama provider), Status: Running".
   - Bagian "instance yang dihapus hilang dari daftar" dilewati dulu, karena CLI belum punya perintah hapus.
5. Diagnostics dengan VoiceOver (setelah perbaikan `dev-f13-fix` masuk): matikan `stacklyd`, buka halaman Diagnostics, lalu nyalakan `stacklyd` lagi. Setelah tersambung, VoiceOver membaca nilai baru di setiap baris (Status, Last connected, pid stacklyd, Last error "None"), bukan nilai sebelum tersambung.
6. Pilihan sidebar diingat (AC-13): pilih Operations di sidebar, keluar dengan ⌘Q, lalu buka app lagi; pilihan Operations kembali. Bila System Settings › Desktop & Dock › "Close windows when quitting an application" menyala, pilihan tidak diingat oleh macOS; catat setting itu bersama hasilmu (jangan diubah hanya untuk cek ini kecuali kamu mau).
7. Klik ikon Stackly di Dock setelah jendela ditutup dengan ⌘W: jendela utama terbuka lagi.
8. Matikan VoiceOver, lalu bersihkan seperti langkah D.5.

**Yang dikirim:** versi macOS, plus nomor dan apa yang terlihat untuk setiap butir yang tidak sesuai, atau "semua sesuai", dan keadaan setting "Close windows when quitting an application".

## I. Setting aksesibilitas sistem di Design Gallery

Untuk F12 (spec 0012 `Verify it`: empat baris setting sistem asli; AC-5, AC-6, AC-7, AC-16), F67 (spec 0067 langkah setting sistem milestone 1 sampai 4; AC-19), dan F68 (spec 0068 milestone 1, langkah Reduce Motion; AC-5). Claude tidak mengubah System Settings; kamu yang menyalakan dan mematikannya.

1. Di checkout utama jalankan `cargo xtask build-app`, lalu buka app Debug dengan root sementara: `STACKLY_ROOT="$(mktemp -d)" target/xcode/Build/Products/Debug/Stackly.app/Contents/MacOS/Stackly`. Jendela utama boleh menyebut stacklyd tidak tersedia. Buka **Window › Design Gallery**.
2. Nyalakan setting **satu per satu**, lihat halaman galeri yang disebut, lalu matikan lagi sebelum setting berikutnya. Cek 1 sampai 3 ada di **System Settings › Accessibility › Display**, cek 4 di **Accessibility › Motion**.
   - **I.1 Differentiate without color.** Halaman Status: titik warna di kolom Standard dan di baris berganti simbol, setiap `ToneBadge` menampilkan simbol tone, label teks tetap ada. Halaman Components: tone di kartu umpan balik, checklist, baris diff, dan tingkat baris log selalu berpasangan dengan simbol atau kata.
   - **I.2 Increase contrast.** Halaman Status: badge Operation Status bergaris tepi 1 pt warna tone, teks detail oranye dan merah lebih gelap. Tokens: kartu bergaris tepi abu abu 1 pt. States: banner bergaris tepi 1 pt. Components: kartu, banner, dan baris bertint mendapat garis tepi 1 pt. Inspector demo (Components › Open Inspector Demo): teks sekunder halaman demo dan baris kartu Summary di kolom inspector lebih gelap dibanding saat setting mati, dan label status di header inspector memakai warna teks tone HC; di layar Retina garis kartu Summary juga menebal dari hairline ke 1 pt. Ini cek F68 K15 (spec 0068 `verify.md` bagian K15): app nyata membaca Increase Contrast asli di dalam inspector tanpa relay. Opsional: ulangi sekilas dalam mode Dark.
   - **I.3 Reduce transparency.** Sidebar dan toolbar galeri menjadi opak; kartu, banner, badge, sheet demo, dan inspector demo terlihat sama persis seperti sebelumnya. Blok konsol dan view log tetap opak.
   - **I.4 Reduce motion.** States › Shape Changes: klik Loaded, Loading, Empty, Failed, Loaded; setiap pergantian hanya memudar, spinner muncul sekitar sepertiga detik kemudian dan tetap berputar. Tokens › Motion: Show Banner dan Hide Banner memudar tanpa gerak. Components: kartu umpan balik hanya crossfade. Components: bar `StagedProgress` melompat ke nilai baru tanpa animasi, dan inspector demo membuka dan menutup tanpa gerak selain crossfade bawaan. **Selama Reduce Motion masih menyala**, jalankan di Terminal kedua `swift test --package-path apps/macos/Packages/StacklyUI --filter GallerySnapshotTests`; test snapshot harus tercatat BLOCKED dengan alasan "system accessibility setting on: Reduce Motion", bukan gagal.
3. Tanpa setting apa pun, ganti tampilan sistem ke Dark (System Settings › Appearance) dan kembali ke Light: halaman Components, sheet demo (Open Sheet), dan inspector demo (Open Inspector Demo) terbaca di keduanya, tanpa teks hilang atau kontras rendah.
4. Kembalikan keempat setting dan tampilan ke keadaan semula, lalu tutup app dengan ⌘Q.

**Yang dikirim:** versi macOS, lalu per butir I.1 sampai I.4 "sesuai" atau nomor dan yang terlihat, plus hasil test snapshot di I.4 (BLOCKED atau tidak).

## J. Keyboard navigation dan VoiceOver di galeri dan menu

Untuk F67 (spec 0067 milestone 1 sampai 4: Tab, Space, fokus yang menahan umpan balik, VoiceOver; AC-4, AC-5, AC-7, AC-9, AC-12, AC-13, AC-15, AC-18, AC-19) dan F69 (spec 0069 milestone 2: menu dibaca VoiceOver; AC-15). Paling hemat dikerjakan bersamaan dengan bagian F, karena keduanya menyalakan Keyboard navigation dan VoiceOver. Keyboard navigation di Mac ini saat ini mati.

1. Nyalakan System Settings › Keyboard › Keyboard navigation, lalu VoiceOver (⌘F5). Buka app Debug seperti I.1, lalu **Window › Design Gallery › Components**.
2. Keyboard:
   - Tab mencapai dan Space menjalankan setiap tombol komponen: Copy, tombol aksi, More beserta itemnya, Dismiss dan aksi kartu umpan balik, Open Sheet (lalu Cancel dan aksi utama di sheet), tombol aksi baris checklist, dan toggle inspector.
   - Tampilkan kartu umpan balik positive, lalu Tab ke tombol Dismiss-nya dan biarkan fokus di sana: kartu tidak hilang selama fokus di Dismiss, dan hilang sekitar 4 detik setelah fokus pindah.
   - Di bagian Log: Tab mencapai Add Line dan Follow dan Space menjalankannya; teks satu baris log bisa dipilih.
   - Dengan mouse atau trackpad (bukan keyboard): gulir view log ke atas saat Follow menyala; Follow mati sendiri, dan baris baru dari Add Line tidak menggulir ke bawah sampai Follow dinyalakan lagi (AC-16).
3. VoiceOver di halaman Components:
   - Baris key value dibaca "{label}, {value}"; tombol Copy "Copy {what}" dan setelah ditekan terdengar "{What} copied." sekali; blok konsol dibaca label lalu perintah; tombol sibuk dibaca dengan nilai "In progress"; umpan balik diumumkan sekali (pesan lalu detail).
   - Baris checklist dibaca "{title}, {status}: {detail}" dengan tombol aksinya sebagai elemen sendiri; `StagedProgress` dibaca "Progress" dengan nilainya.
   - Toggle inspector dibaca "Hide Inspector" atau "Show Inspector"; header inspector dibaca judul sebagai heading, lalu "Status: Running", lalu tombol aksinya.
   - Baris diff dibaca dengan "Added" atau "Removed".
   - Menu More: item nonaktif dibaca bersama alasannya (baris alasan di bawah item).
   - Sheet demo (Open Sheet): saat aksi utama nonaktif, alasan di footer ikut dibaca; `StagedProgress` dengan pengumuman tahap mengumumkan tahap baru sekali saat berganti.
   - Inspector demo tanpa pilihan: `InspectorPlaceholder` dibaca sebagai satu kelompok (judul lalu pesan).
   - View log: setiap baris dibaca waktu, sumber, tingkat, lalu teks.
4. VoiceOver di menu bar (F69): tekan ⌃F2, buka menu Stackly, File, Edit, View, dan Window; setiap item terbaca dengan judul, shortcut, dan "dimmed" saat nonaktif (misalnya ⌘1 sampai ⌘7 saat Design Gallery di depan).
5. Matikan VoiceOver dan Keyboard navigation bila tadinya mati, lalu tutup app.

**Yang dikirim:** versi macOS, plus nomor dan yang terlihat untuk setiap butir yang tidak sesuai, atau "semua sesuai".

## K. Automation Mode dan UI test

Untuk F68 milestone 3 (spec 0068 AC-12 sampai AC-19: XCUITest shell, galeri, menu, dan audit aksesibilitas). Tanpa langkah ini, semua UI test tercatat BLOCKED dengan owner kamu. Claude tidak pernah menyalakan Automation Mode.

1. Di Terminal jalankan `automationmodetool`. Bila keluarannya bukan "Automation Mode is enabled", nyalakan sendiri dengan `automationmodetool enable-automationmode-without-authentication` (meminta password administrator). Bila macOS bertanya, izinkan runner UI test (Xcode Helper atau `StacklyAppUITests-Runner`) di System Settings › Privacy & Security.
2. Pastikan layar tidak terkunci, tutup app Stackly yang sedang terbuka (daemon dan instance tidak terpengaruh), lalu dari root repo jalankan `cargo xtask evidence run --kind fixture -- cargo xtask ui-test`. Jangan memakai keyboard dan mouse sampai selesai; app akan terbuka dan tertutup berkali kali. Exit 0 berarti semua lulus.
3. Hasilnya ada di `target/ui-tests/<waktu UTC>/` (`result.xcresult` dan `attachments/`, satu screenshot jendela per test). Buka beberapa screenshot dan lihat apakah sidebar, toolbar, banner, dan footer tampak benar di light dan dark.
4. Setelah selesai, Automation Mode boleh kamu matikan lagi (perintahnya tercantum di `automationmodetool help`) atau dibiarkan menyala untuk run berikutnya.

**Yang dikirim:** keluaran `automationmodetool`, exit code `ui-test`, nama test yang gagal (bila ada) beserta baris error pertamanya, dan catatanmu tentang screenshot.

## G. Dua akun macOS

Opsional. Untuk F6, yang sudah `done`. Record: `evidence/manual/2026-10-19-two-macos-accounts-the-second-account-cannot-rea.json` (AC-079, REQ-047, BR-012). Bagian otomatisnya sudah lulus; ini bukti di Mac nyata dengan akun kedua. Butuh password administrator untuk membuat akun.

1. Buat satu akun macOS standar kedua di Mac ini, lalu login ke akun itu sekali.
2. Sebagai akun pertama, jalankan `stacklyd` tanpa `STACKLY_ROOT`, lalu catat path socket dari `stackly daemon status`.
3. Sebagai akun kedua, coba buka socket akun pertama dengan `ls -l <path>` dan `nc -U <path>`. Hasilnya harus permission denied, karena runtime root akun pertama 0700 dan socketnya 0600.
4. Sebagai akun kedua, jalankan `stacklyd` miliknya sendiri. Socket dan root-nya harus terpisah, dan `stackly daemon status` di akun itu menunjukkan daemon kedua.
5. Hentikan kedua daemon. Akun kedua boleh kamu hapus lagi.

**Yang dikirim:** hasil langkah 3 dan 4, dan path socket kedua akun.

## H. Folder temp setelah beberapa hari idle

Opsional. Untuk F6, yang sudah `done`. Record yang masih terbuka: `evidence/manual/2026-10-19-when-macos-27-cleans-the-user-temporary-folder-t.json` (REQ-047). Bagian reboot sudah terjawab pada 2026-10-21 (record `2026-10-20-macos-27-empties-the-user-temporary-folder-at-a.json`): setelah reboot, isi folder temp user kosong. Yang belum terjawab adalah pembersihan otomatis macOS setelah beberapa hari idle tanpa reboot.

1. Jalankan `getconf DARWIN_USER_TEMP_DIR` dan catat path-nya sebagai T.
2. Buat file penanda: `mkdir -p "$T/stackly-marker" && date > "$T/stackly-marker/created"`.
3. Biarkan Mac menyala tanpa reboot paling sedikit 4 hari. Kamu tetap boleh memakainya seperti biasa.
4. Cek apakah `$T/stackly-marker/created` masih ada, dan catat tanggalnya.
5. Bila `stacklyd` sedang berjalan selama periode itu, cek juga log daemon untuk `socket_rebound`, dan pastikan `stackly daemon status` masih berfungsi.

**Yang dikirim:** tanggal penanda dibuat, tanggal dicek, apakah penanda masih ada, dan hasil langkah 5 bila ada.

## Cara mengirim hasil

Tempel ke Claude dengan bentuk seperti ini. Bagian yang belum dikerjakan cukup ditulis "belum".

```text
macOS: <versi>

A: 3 exit <n>, dialog ya/tidak · 5.1 teks "<...>", exit <n>, kode <...> · 5.2 exit <n>, kode <...> · 5.3 exit <n>, kode <...> · 6 present tanpa dialog ya/tidak
B: sembunyi otomatis <detik> · pasteboard kosong <detik> · langkah tidak sesuai: <nomor dan yang terlihat> atau "semua sesuai" · 12: <hasil> atau "dilewati"
C: <nomor dan yang terlihat> atau "semua sesuai"
D: baris tetap Stopped ya/tidak · start setelah restart berhasil ya/tidak
E: <catatan per preview> atau "semua baik"
F: <nomor dan yang terlihat> atau "semua sesuai"
I: I.1 <sesuai atau yang terlihat> · I.2 <...> · I.3 <...> · I.4 <...>, snapshot BLOCKED ya/tidak
J: <nomor dan yang terlihat> atau "semua sesuai"
K: automationmodetool <keluaran> · ui-test exit <n> · gagal: <test dan error> atau "tidak ada" · screenshot: <catatan>
G: belum
H: belum
```
