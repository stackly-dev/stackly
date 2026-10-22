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
| [T. JOURNEY-03: PostgreSQL 17 dan 18 di aplikasi](#t-journey-03-postgresql-17-dan-18-di-aplikasi-fitur-25) | F25 milestone 4, AC-10 di [scope/03-multi-versi-dan-instance.md](scope/03-multi-versi-dan-instance.md) | 30 menit | wajib untuk F25 |

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

## T. JOURNEY-03: PostgreSQL 17 dan 18 di aplikasi (fitur 25)

Untuk F25 milestone 4 (spec 0025 AC-10, REQ-035 dan REQ-043). Record: `evidence/manual/2026-10-22-journey-03-in-the-app-postgresql-18-beside-17-on.json`.

Bagian ini memakai root dev sementara dan tidak memakai jaringan: tombol Get memakai artefak PostgreSQL 17.11 dan 18.6 yang sudah ada di cache xtask (`~/Library/Caches/dev.stackly.xtask/runtime/`). Root sementara memakai keychain file, jadi login Keychain tidak disentuh. Siapkan tiga terminal: A untuk perintah, B untuk daemon, dan C untuk app. Siapkan checkout dan terminal seperti di *Persiapan umum*.

1. Terminal A: `cargo build -p stacklyd -p stackly-cli && cargo xtask build-app`. Tunggu sampai muncul `BUILD SUCCEEDED`.
2. Terminal A: cek bahwa kedua artefak ada di cache:
   - `export CACHE=~/Library/Caches/dev.stackly.xtask/runtime PG17=04b1eff117441c0b6f1cbefa42ded6cf5726d656e54881674defea04b515d6d3 PG18=88bb7533e639e852e5258c293aa42f0abf40aed42d18f0f714cb47a9354b6590`
   - `ls -l $CACHE/$PG17 $CACHE/$PG18` harus menampilkan dua file, 27984415 dan 28311215 byte.
   - Bila salah satu tidak ada, berhenti dan kirim "BLOCKED: artefak hilang di cache". Jangan mengunduh sendiri; unduhan butuh persetujuan.
3. Terminal A: buat root dan catalog: `export R="$(mktemp -d /tmp/stk-pg1718.XXXXXX)"`, lalu `export CAT="$R.catalog" S=target/debug/stackly STACKLY_ROOT="$R"`, lalu `cargo xtask catalog build --out "$CAT"`. Harus mencetak `catalog: revision <n>, 2 provider(s), 4 planned -> ...`.
4. Terminal B: `cd` ke root repo, lalu `STACKLY_ROOT="<nilai R>" STACKLY_CATALOG_DIR="<nilai CAT>" target/debug/stacklyd`. Tulis path itu langsung (ketik `echo $R $CAT` di Terminal A bila perlu), lalu biarkan tetap jalan. Pada start pertama daemon membuat `$R/cache/downloads` dengan mode 0700. Jangan membuat folder itu sendiri dengan `mkdir`: mode bawaan `mkdir` ditolak daemon (`is not a private Stackly folder`).
5. Terminal A: salin artefak ke cache unduhan root dev, tanpa mengganti nama (daemon membaca file bernama sha256 persis):
   - `cp $CACHE/$PG17 $CACHE/$PG18 "$R/cache/downloads/"`
   - `$S runtime list` tidak boleh menampilkan runtime, dan `$S daemon status` harus menyebut daemon berjalan.
6. Terminal C: `STACKLY_ROOT="<nilai R>" target/xcode/Build/Products/Debug/Stackly.app/Contents/MacOS/Stackly`. Bila onboarding muncul, lewati dengan Continue; di langkah Background Agent pilih Not Now. Footer harus menampilkan `stacklyd running`.
7. Pasang PostgreSQL 17:
   1. Pilih Catalog, buka PostgreSQL, lihat matriks versi. Baris 17.11 dan 18.6 sama sama punya tombol Get.
   2. Klik Get di baris 17.11. Progress tampil di sel itu, lalu berubah menjadi Installed dalam sekitar 10 detik.
   3. Pilih Runtimes. Harus ada satu baris PostgreSQL 17.11. Cek `$S runtime list`: satu baris, versi 17.11, state `installed`.
8. Buat dan jalankan `pg17-dev`:
   1. Pilih Instances, klik New Instance di toolbar. Pada Runtime hanya ada `PostgreSQL 17.11`.
   2. Isi Name `pg17-dev`. Pada Superuser password biarkan Generate. Nyalakan Start after creating, lalu klik Create.
   3. Tunggu baris `pg17-dev` menjadi Ready tanpa kamu klik apa pun. Catat PID: `$S instance list` memberi ID, lalu `$S instance get <ID>` memberi PID dan waktu start. Simpan ID sebagai ID17.
9. Pasang PostgreSQL 18 saat `pg17-dev` berjalan:
   1. Pilih Catalog, PostgreSQL, lalu klik Get di baris 18.6.
   2. Selama install berjalan (sekitar 10 detik), pindah ke Instances. Baris `pg17-dev` harus tetap Ready, tanpa pernah berubah menjadi Starting atau Stopped. Ambil tangkapan layar halaman ini bila install masih berjalan, tetapi tidak wajib.
   3. Setelah Installed muncul di Catalog, `$S instance get $ID17` harus menunjukkan PID dan waktu start yang sama dengan langkah 8.3, dan status masih Ready.
   4. Pilih Runtimes: harus ada dua baris, PostgreSQL 17.11 dan 18.6.
10. Klik New Instance lagi. Buka daftar Runtime: harus berisi `PostgreSQL 17.11` dan `PostgreSQL 18.6`. Ambil tangkapan layar sheet dengan daftar terbuka.
11. Buat `pg18-dev`: pilih `PostgreSQL 18.6`, Name `pg18-dev`, password Generate, nyalakan Start after creating, klik Create. Biarkan port seperti bawaan sheet; port kedua instance harus berbeda. Tunggu `pg18-dev` Ready. Baris `pg17-dev` harus tetap Ready, dengan PID yang sama dengan langkah 8.3.
12. Di halaman Instances (lebarkan jendela supaya kolom Provider, Version, dan Endpoints tampil), kolom Version harus menampilkan `17.11` untuk `pg17-dev` dan `18.6` untuk `pg18-dev`, dan kolom Endpoints menampilkan dua port yang berbeda. Ambil tangkapan layar halaman ini.
13. Klik kanan baris `pg18-dev`, pilih Open, lalu buka tab Summary. Kartu Connection harus menampilkan endpoint dan connection `pg18-dev` sendiri, dengan port yang sama dengan kolom Endpoints baris itu. Ambil tangkapan layar tab Summary.
14. Klik Copy di baris Connection string (nilai tersamar, jadi tidak perlu Show). Pasteboard hanya hidup 90 detik, jadi langsung jalankan di Terminal A:
    - `P18=$(ls -d "$R"/data/runtimes/postgresql/18.6/macos-aarch64/*/bin/psql)`
    - `"$P18" "$(pbpaste)" -Atc 'SELECT version()'`

    Keluarannya harus satu baris yang berisi `PostgreSQL 18.6`. Password fixture ikut tampil di `ps` selama perintah ini jalan; itu tidak masalah untuk root dev ini. Bila Keychain atau macOS memunculkan dialog, catat teksnya.
15. Ulangi untuk `pg17-dev`: buka detailnya, tab Summary, klik Copy, lalu `P17=$(ls -d "$R"/data/runtimes/postgresql/17.11/macos-aarch64/*/bin/psql)` dan `"$P17" "$(pbpaste)" -Atc 'SELECT version()'`. Keluarannya harus berisi `PostgreSQL 17.11`.
16. Terminal A: `ps -axww -o pid=,args= | grep "$R" | grep "bin/postgres" | grep -v grep` harus menampilkan dua postmaster, satu dari folder runtime `17.11` dan satu dari `18.6`, masing masing dengan folder data instance sendiri.
17. Bersihkan:
    - `$S instance stop <ID18> --wait` dan `$S instance stop $ID17 --wait`. ID18 dari `$S instance list`.
    - ⌘Q app di Terminal C, lalu Ctrl+C daemon di Terminal B.
    - `ps -axww -o pid=,args= | grep -E "$R|stk-pg1718" | grep -v grep` harus kosong. Bila masih ada proses `postgres`, hentikan lewat PID yang tampil; jangan pakai `pkill -f` atau `killall`.
    - `STACKLY_ROOT="$R" target/debug/stacklyd --print-roots` mencetak `runtime_root`; catat path-nya, lalu `rm -rf "$R" "$CAT"`, dan hapus folder `runtime_root` itu bila masih ada.
    - Cache xtask di `~/Library/Caches/dev.stackly.xtask/runtime/` tidak disentuh: yang kamu hapus hanya salinannya di `$R/cache/downloads`.

**Tangkapan layar:** ambil dengan ⇧⌘4 (simpan di Desktop) dan sebutkan path-nya ke Claude, supaya Claude menyalinnya ke `evidence/manual/files/`: (1) halaman Runtimes dengan dua runtime (langkah 9.4), (2) sheet New Instance dengan daftar Runtime terbuka (langkah 10), (3) halaman Instances dengan kolom Version (langkah 12), (4) tab Summary `pg18-dev` (langkah 13).

**Yang dikirim:** versi macOS, hasil langkah 2 (kedua artefak ada atau tidak), PID `pg17-dev` di langkah 8.3 dan 9.3, apa yang terlihat di baris `pg17-dev` selama langkah 9.2, daftar Runtime di langkah 10, kolom Version di langkah 12, port di tab Summary dan kolom Endpoints (langkah 13), keluaran `SELECT version()` di langkah 14 dan 15, hasil langkah 16, path empat tangkapan layar, serta nomor dan apa yang terlihat untuk setiap langkah yang tidak sesuai atau "semua sesuai".

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
