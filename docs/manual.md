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
| [L. F15 Background agent dan install CLI](#l-f15-background-agent-dan-install-cli) | F15 milestone 1 dan 3 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md) | 40 menit, butuh persetujuanmu untuk agent dan `~/Applications` | wajib untuk F15 |
| [M. F16 Onboarding](#m-f16-onboarding) | F16 milestone 1 sampai 3 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md) | 45 menit, sesudah L | wajib untuk F16 |
| [N. F17 Catalog dan provider detail](#n-f17-catalog-dan-provider-detail) | F17 milestone 3 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md) | 30 menit | wajib untuk F17 |
| [O. F18 Install runtime managed](#o-f18-install-runtime-managed) | F18 milestone 4 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md) | 35 menit, memakai jaringan atau cache xtask | wajib untuk F18 |
| [P. F20 Create instance](#p-f20-create-instance) | F20 milestone 3 dan 4 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md) | 35 menit, sesudah O | wajib untuk F20 |
| [Q. F21 Start, readiness, dan connection](#q-f21-start-readiness-dan-connection) | F21 milestone 3 dan 4 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md) | 40 menit, ditambah 20 menit run offline | wajib untuk F21 |
| [R. F22 Overview dan daftar instance](#r-f22-overview-dan-daftar-instance) | F22 milestone 3 dan 4 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md) | 40 menit | wajib untuk F22 |
| [S. F23 Provider NATS](#s-f23-provider-nats) | F23 milestone 4 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md) | 20 menit, sesudah O | wajib untuk F23 |

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
5. Perintah halaman dan instance (F69 milestone 3, spec 0069 cek manual 1, 5, dan 6; AC-2, AC-4, AC-9, AC-10, AC-11, AC-12, AC-14, AC-15), dengan app yang tersambung ke daemon di root sementara hasil `skeleton_seed` (seperti bagian C), hanya keyboard:
   - ⌘N, isi sheet, Return membuat instance. Lalu ⌘2, Tab ke tabel, panah memilih baris, ⇧⌘R menghentikan instance, ⌘R menjalankannya lagi, ⌘O membuka detail, ⇧⌘C menyalin connection string, ⌘[ kembali ke tabel, ⌥⌘I menyembunyikan dan menampilkan inspector.
   - View › Sort Instances By menandai kolom dan arah yang aktif.
   - Saat sheet New Instance terbuka, ⌘1 dan ⌘R tidak melakukan apa pun.
6. Find (F69, AC-10): ⌘F memfokuskan field pencarian di Instances dan Catalog; Find nonaktif di Overview, saat halaman detail terbuka, dan di halaman tanpa pencarian; ⌘F saat field sudah fokus tidak mengubah teksnya.
7. Probe P4 sampai P7 di app Stackly (spec 0069, AC-2, AC-3): shortcut tidak melakukan apa pun saat sheet atau sheet `NSAlert` terbuka di jendela utama (P4); `.searchFocused` memfokuskan field pencarian di toolbar (P5); ⌘R, ⇧⌘R, ⌥⌘R, ⌘O, ⌘D, ⌘1, dan ⌘[ di field pencarian sampai ke menu tanpa mengubah teks (P6); menu dan penjaga `run` tetap benar saat state berubah tepat di sekitar tombol ditekan (P7).
8. VoiceOver (⌃F2) di menu Instance, View, dan Edit: setiap item terbaca dengan judul, shortcut, dan "dimmed"; ⌘F di Instances dan Catalog diumumkan; tombol Start, Stop, dan inspector membacakan tooltip nya beserta shortcut (AC-14, AC-15).
9. Matikan VoiceOver dan Keyboard navigation bila tadinya mati, lalu tutup app.

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

## L. F15 Background agent dan install CLI

Untuk F15 (spec 0015) milestone 1 dan 3 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md). Sumber: `docs/specs/0015-background-agent-cli-install/verify.md`. Bagian L sampai S dikerjakan dengan urutan yang aku sarankan ini: L dulu, karena M langkah 3 sampai 5 butuh agent yang sudah terdaftar; lalu M, N, dan O (O memasang runtime yang dipakai P, Q, dan S); lalu P, Q, S, dan R. Automation Mode (K) cukup dinyalakan sekali untuk kelompok Automation Mode di bawah (langkah 1 dan 2) dan N langkah 13.

Mendaftarkan agent sungguhan mengubah Login Items di akunmu, dan `--install` menaruh bundle ke `~/Applications`. Hanya kamu yang menyetujui dan menjalankannya; Claude tidak pernah. Langkah Turn On bergantung pada signing (OQ-012): verify.md mencatat bukti agent sungguhan dengan signing nyata (AC-17 sampai AC-20) sebagai `BLOCKED` sampai OQ-012. Build di Mac ini bertanda tangan ad hoc, jadi macOS bisa saja menolaknya. Catat apa yang muncul, apa adanya.

**Persiapan bersama L sampai S.** Kerjakan dari root repo di checkout utama (lihat *Persiapan umum*). Kecuali disebut lain, bagian L sampai S memakai bundle Debug dan root sementara berikut. Sejak F15, `cargo xtask build-app` menaruh bundle di `target/app/Debug/Stackly.app`; bagian B, C, dan I di atas masih menyebut `target/xcode/...`, jadi pakai path mana pun yang benar-benar ada di Mac ini.

```bash
cargo build -p stacklyd -p stackly-fake-server -p stackly-cli && cargo build -p stacklyd --example skeleton_seed && cargo xtask build-app
```

```bash
export R="$(mktemp -d /tmp/stk-s02.XXXXXX)"; APP=target/app/Debug/Stackly.app/Contents/MacOS; echo $R
```

- Daemon dari bundle, dengan catalog bawaan bundle: Terminal B, `STACKLY_ROOT="$R" $APP/stacklyd`. Ulangi dua `export` di atas di setiap terminal baru.
- App: Terminal C, `STACKLY_ROOT="$R" $APP/Stackly`.
- CLI dari bundle: `STACKLY_ROOT="$R" $APP/stackly-cli <perintah>`. Ia memberi blok `agent`; `target/debug/stackly` menjawab `not_bundled`.
- Skenario fixture tanpa daemon (hanya build Debug): `STACKLY_UI_SCENARIO=<nama> $APP/Stackly`. Nama yang dipakai di bawah ada di `FixtureScenario.ID`.
- Daemon development dengan runtime fake (dipakai Q langkah 3 dan R): siapkan root, `skeleton_seed`, dan `target/debug/stacklyd` seperti C langkah 2 dan 3, tetapi buka app dari `$APP/Stackly`.
- Bersihkan: ⌘Q di app, Ctrl-C di daemon, `ps -axww -o pid=,args= | grep -E "stackly|$R" | grep -v grep` kosong, lalu `rm -rf "$R"` (jangan dihapus dulu bila kamu lanjut ke bagian berikutnya yang memakai root yang sama).

**Dengan persetujuanmu, agent sungguhan dan `~` sungguhan** (`docs/specs/0015-background-agent-cli-install/verify.md:11`, `:53`, `:54`, `:111`). Catat keadaan awal: System Settings › General › Login Items & Extensions (apakah Stackly sudah ada), `ls -l ~/.local/bin/stackly ~/.local/share/man/man1/stackly*.1`, dan `stat -f '%N %m' ~/.zshrc ~/.zprofile ~/.zshenv ~/.bash_profile` (file yang tidak ada boleh diabaikan).

1. `cargo xtask build-app --install` menaruh bundle ke `~/Applications` dan mengganti bundle Stackly yang sudah ada di sana. Cek `ls ~/Applications/Stackly.app/Contents/MacOS` berisi `Stackly`, `stacklyd`, `stackly-cli` (`docs/specs/0015-background-agent-cli-install/verify.md:11`).
2. Buat symlink terkelola dengan tangan, dengan bentuk yang sama dengan hasil Install di Diagnostics (lihat M langkah 9 di Milestone 2): `mkdir -p ~/.local/bin && ln -s ~/Applications/Stackly.app/Contents/MacOS/stackly-cli ~/.local/bin/stackly`. Jalankan `ls -l ~/.local/bin/stackly` lebih dulu; bila sudah ada, lihat tujuannya sebelum menimpa (`docs/specs/0015-background-agent-cli-install/verify.md:11`).
3. Jalankan `stackly daemon install` lewat symlink itu (PATH tidak kamu ubah; panggil `~/.local/bin/stackly daemon install`). Bila statusnya `requires_approval` atau `disabled`, perintah selesai dengan exit 4 tanpa mendaftar; setujui switch Stackly di Login Items bila kamu mau mendaftarkannya, lalu ulangi. Catat keluaran, exit code, dan `~/.local/bin/stackly daemon status --json` (blok `agent`) (`docs/specs/0015-background-agent-cli-install/verify.md:11`).
4. Build ulang dan install lagi selagi daemon berjalan: `cargo xtask build-app --install`, lalu `~/.local/bin/stackly daemon restart`. Yang diharapkan: `cli.agent_relaunched`, dan `DaemonInfo.launch.bundlePath` (di bagian `info` pada `daemon status --json`) menunjuk bundle baru (`docs/specs/0015-background-agent-cli-install/verify.md:11`).
5. Matikan sendiri switch Stackly di Login Items, lalu catat `~/.local/bin/stackly daemon status --json` (state agent) dan apa yang tampil di banner app bila ia terbuka (`docs/specs/0015-background-agent-cli-install/verify.md:11`).
6. GUI dengan agent sungguhan: buka `~/Applications/Stackly.app` (tanpa `STACKLY_ROOT`, karena dengan `STACKLY_ROOT` app tidak pernah memanggil agent) dan catat teks banner pada setiap langkah:
   1. Tekan Turn On bila tersedia, setujui di Login Items bila diminta, lalu Start stacklyd bila ditawarkan. Banner harus berjalan sampai "stacklyd is starting…" lalu daemon tersambung.
   2. Selagi state agent belum `enabled`, buka Diagnostics, lalu di Terminal jalankan `~/.local/bin/stackly daemon install`. State harus menjadi `enabled` di luar aksi: banner "stacklyd is starting…" tanpa ada `register` dari GUI. Verify.md tidak menyebut cara mengembalikan agent ke state belum terdaftar; bila kamu tidak bisa sampai ke state itu, tulis "dilewati" (`docs/specs/0015-background-agent-cli-install/verify.md:53`).
7. Seksi Command Line Tool di Diagnostics terhadap `~` sungguhan: tekan Install, Repair, dan Remove (mulai dengan Remove bila link dari langkah 2 sudah ada). Sesudahnya bandingkan keadaan awal: hanya `~/.local/bin/stackly` dan `~/.local/share/man/man1/stackly*.1` yang boleh berubah, dan `stat` file shell tidak berubah (`docs/specs/0015-background-agent-cli-install/verify.md:54`).
8. VoiceOver dan Last error (⌘F5): di app Debug tanpa `STACKLY_ROOT`, tekan Start stacklyd tanpa daemon yang menjawab selama 30 detik, lalu Turn On dengan build yang tanda tangannya ditolak; VoiceOver harus mengumumkan teks Last error sekali. Lalu matikan izin di System Settings selagi "stacklyd is starting…" tampil: banner harus berganti ke Open Login Items yang aktif tanpa menunggu 30 detik (`docs/specs/0015-background-agent-cli-install/verify.md:111`).

**Dengan Automation Mode, keyboard, dan VoiceOver, tanpa mendaftarkan agent.** Nyalakan Automation Mode dan siapkan runner UI test seperti K langkah 1 dan 2 (bagian K tidak diulang di sini). Layar jangan terkunci dan keyboard serta mouse jangan disentuh selama run.

1. `cargo xtask ui-test --only ShellUITests`. Ia memeriksa lima skenario agent (`agent-requires-approval`, `agent-disabled`, `agent-enabled`, `agent-not-found`, `agent-unavailable`) dengan footer "stacklyd unavailable", judul dan aksi baris tabel *Banner per state*; `daemon-offline` dan `daemon-unreachable` yang menampilkan "Stackly’s background service is off." dengan Turn On dan Open Diagnostics; dan tiga alur: Turn On di `daemon-unreachable` memberi banner `requires_approval`, Turn On di `agent-not-found` memberi banner critical `not_found` dengan Open Diagnostics saja, Start stacklyd di `agent-enabled` memberi "stacklyd is starting…" tanpa aksi (tanpa menunggu batas 30 detik). Skenario ini memakai fake, jadi tidak ada `SMAppService` yang dipanggil. Catat exit code dan nama test yang gagal (`docs/specs/0015-background-agent-cli-install/verify.md:49`).
2. `cargo xtask ui-test --only AuditUITests`, lalu `cargo xtask ui-test --only MenuUITests`. Keduanya harus tetap lulus dengan banner agent baru (audit `daemon-offline` kini mencakup tombol Turn On) (`docs/specs/0015-background-agent-cli-install/verify.md:50`).
3. Keyboard dan VoiceOver (keduanya menyala seperti F dan J): `STACKLY_UI_SCENARIO=daemon-unreachable $APP/Stackly`. Tab melewati Turn On dan Open Diagnostics di banner dan Space menekannya. Dengan VoiceOver, perubahan state agent diumumkan satu kali ("Background service needs your approval."), banner critical `not_found` diumumkan oleh `StateBanner`, dan fokus tetap di tombol yang baru ditekan karena hanya banner critical yang dibuat ulang (`docs/specs/0015-background-agent-cli-install/verify.md:51`).

**Yang dikirim:** versi macOS; untuk langkah 1 sampai 5: keluaran, exit code, dan state agent; untuk langkah 6 dan 7: teks banner per langkah dan hasil perbandingan `ls` dan `stat`, atau "dilewati"; untuk langkah 8 dan tiga langkah Automation Mode: lulus atau nomor dan yang terlihat, serta keluaran `automationmodetool` bila K belum dikirim.

## M. F16 Onboarding

Untuk F16 (spec 0016) milestone 1 sampai 3 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md). Sumber: `docs/specs/0016-onboarding/verify.md`. Milestone 4 belum ada di verify.md dan ditambahkan nanti. Pakai *Persiapan bersama L sampai S* di bagian L. Onboarding dibuka dengan argumen launch `-onboarding.finished NO`:

```bash
STACKLY_ROOT="$R" $APP/Stackly -onboarding.finished NO
```

Keyboard navigation dan VoiceOver menyala seperti F dan J; Increase Contrast dan Reduce Transparency seperti I. Langkah yang menekan Escape atau Get Started menulis record selesai, tetapi argumen di atas tetap membuka onboarding di peluncuran berikutnya. Yang bertanda "agent" bergantung pada agent sungguhan dari L; bila belum terdaftar, tulis "dilewati".

**Milestone 1: kerangka, Welcome, dan Storage** (`docs/specs/0016-onboarding/verify.md:9` sampai `:22`).

1. Buka app dengan argumen di atas: jendela utama menampilkan onboarding langkah 1 sebagai seluruh isi jendela, tanpa sidebar, toolbar, judul jendela, footer daemon, atau banner daemon; hanya tombol lampu lalu lintas (`docs/specs/0016-onboarding/verify.md:9`).
2. Di langkah 1 baca footer: lima titik langkah (titik aktif lebih lebar), teks "Step 1 of 5 · Welcome", tanpa Back, tombol utama "Continue" (`docs/specs/0016-onboarding/verify.md:10`).
3. Tekan Continue, Back, dan klik tiap titik langkah: langkah berpindah langsung ke langkah itu. Back tampil mulai langkah 2; label tombol utama "Not Now" di langkah 2, "Continue" di langkah 3 dan 4, "Get Started" di langkah 5 (`docs/specs/0016-onboarding/verify.md:11`).
4. Baca langkah Welcome: ikon app 112 pt, judul "Welcome to Stackly", subjudul, dan baris "Isolated instances" serta "No account needed" masing-masing dengan ikon di tile netral. Tidak ada baris "Verified runtimes" dan tidak ada baris "Environments as files"; teks isolasi menyebut instance berjalan sebagai user kamu, tanpa sandbox (`docs/specs/0016-onboarding/verify.md:12`).
5. Di langkah 3 (Storage) tanpa `STACKLY_ROOT` (jalankan app tanpa variabel itu): baris Data, Cache, Logs, dan Secrets "Login Keychain"; path memakai `~` untuk folder home. Bila daemon berjalan, path sama dengan `stackly daemon status --json` `info.roots`. Daemon tanpa `STACKLY_ROOT` memakai root asli di Application Support dan login Keychain (lihat A), jadi jalankan hanya bila kamu tidak keberatan (`docs/specs/0016-onboarding/verify.md:13`).
6. Jalankan app dengan `STACKLY_ROOT` di environment: subjudul memakai varian override, satu baris "Folders" dengan catatan override, dan Secrets "A keychain file inside that folder"; nilai variabel tidak tampil di mana pun (`docs/specs/0016-onboarding/verify.md:14`).
7. Tekan Escape di langkah 3: onboarding tertutup, shell tampil di bagian yang tersimpan (Overview pada peluncuran pertama), tanpa kartu umpan balik. Buka app lagi tanpa `-onboarding.finished NO`: shell langsung (`docs/specs/0016-onboarding/verify.md:15`).
8. Buka onboarding lagi dengan argumen di atas dan tekan Get Started di langkah 5: shell tampil dan kartu umpan balik "Setup complete." muncul sekali dan diumumkan VoiceOver sekali (`docs/specs/0016-onboarding/verify.md:16`).
9. Menu Help › "Welcome to Stackly" (tanpa shortcut): saat jendela utama ditutup, ia membuka jendela dan menampilkan langkah 1; saat onboarding tampil di langkah 3, ia hanya membawa jendela ke depan dan tetap di langkah 3 (`docs/specs/0016-onboarding/verify.md:17`).
10. Di langkah 3 tutup jendela (⌘W) lalu klik Stackly di Dock: onboarding tampil lagi di langkah 3. Tutup lagi dan keluar lewat Quit, lalu buka lagi: mulai dari langkah 1, tanpa Get Started tercatat (`docs/specs/0016-onboarding/verify.md:18`).
11. Saat onboarding tampil, buka menu View dan Instance: perintah shell nonaktif; ⌘, tetap membuka Settings (`docs/specs/0016-onboarding/verify.md:19`).
12. Keyboard saja: Tab melewati titik langkah, Back, dan tombol utama; Space menekannya; Return menekan tombol utama; Escape melewati onboarding (`docs/specs/0016-onboarding/verify.md:20`).
13. VoiceOver: titik langkah dibaca "Step {n} of 5, {label}" dengan trait terpilih untuk langkah aktif. Saat langkah berganti, fokus pindah ke judul langkah dan "Step {n} of 5, {label}." terucap sekali (`docs/specs/0016-onboarding/verify.md:21`).
14. Jendela 900 × 600 pt, light, dark, Increase Contrast, dan Reduce Transparency (setting dinyalakan satu per satu seperti I): konten tidak terpotong (digulir bila perlu), teks terbaca, tanpa glass atau warna sistem sekunder (`docs/specs/0016-onboarding/verify.md:22`).

**Milestone 2: langkah Background Agent dan Command Line** (`docs/specs/0016-onboarding/verify.md:54` sampai `:64`).

1. Pindah ke langkah 2 tanpa menekan apa pun: kartu menampilkan state agent yang sebenarnya di Mac ini (biasanya "Background service is off" dengan tombol Turn On). `STACKLY_ROOT="$R" $APP/stackly-cli daemon status --json` sebelum dan sesudah memberi blok `agent` yang sama (`docs/specs/0016-onboarding/verify.md:54`).
2. Di langkah 2 baca footer: tombol utama "Not Now" selama state bukan `enabled`, "Continue" saat `enabled` (`docs/specs/0016-onboarding/verify.md:55`).
3. Agent: dengan daemon tersambung dan agent `enabled`, kartu "Allowed in Login Items", "stacklyd starts when you log in.", dan status daemon "stacklyd running" di bawahnya; tanpa tombol aksi (`docs/specs/0016-onboarding/verify.md:56`).
4. Agent: hentikan daemon (agent `enabled`), misalnya dengan `stackly daemon stop`. Kartu "stacklyd is starting…" dengan status "stacklyd unavailable"; tepat di detik ke 15 berganti ke "Allowed, but stacklyd isn’t running" dengan tombol Start stacklyd, tanpa menunggu polling (`docs/specs/0016-onboarding/verify.md:57`).
5. Agent: tekan Turn On. Langkah ini mendaftarkan agent sungguhan dan bergantung pada signing (OQ-012); kerjakan hanya bila kamu menyetujuinya. Verify.md menulis "setelah persetujuan lewat lead" dan menyatakan hasilnya dicatat bersama record AC-17 spec 0015. Yang diharapkan: progress dan "stacklyd is starting…" sampai daemon tersambung, tanpa tombol Start stacklyd. Hasil `requires_approval` menampilkan "Waiting for approval" dengan Open System Settings… dan fokus keyboard kembali ke tombol itu (`docs/specs/0016-onboarding/verify.md:58`).
6. Buka langkah 2 tanpa `STACKLY_ROOT` dan dengan `STACKLY_ROOT`: varian "STACKLY_ROOT is set" tanpa aksi pada yang kedua (`docs/specs/0016-onboarding/verify.md:59`).
7. VoiceOver di langkah 2: masuk langkah hanya mengucapkan "Step 2 of 5, Background Agent."; perubahan state agent diumumkan sekali (oleh app, bukan dua kali); `not_found` saat daemon tidak ada diumumkan sekali dengan "Stackly can’t find its background service." (`docs/specs/0016-onboarding/verify.md:60`).
8. Langkah 5 (Command Line) pada akun ini, terhadap `~` sungguhan: toggle "Install stackly in ~/.local/bin" mati dan detail "No password needed. Stackly doesn’t change your PATH or shell files.". Menyalakan toggle membuka konfirmasi "Install the stackly command?" dan Cancel tidak mengubah apa pun (`ls -l ~/.local/bin/stackly` tetap tidak ada, toggle kembali mati). Bila link dari L langkah 2 masih ada, hapus dulu lewat Remove (`docs/specs/0016-onboarding/verify.md:61`).
9. Ulangi dan tekan Install: `ls -l ~/.local/bin/stackly` menunjuk ke `…/Stackly.app/Contents/MacOS/stackly-cli`, `~/.local/bin/stackly --version` jalan, baris PATH `export PATH="$HOME/.local/bin:$PATH"` dengan tombol Copy ("Copied." terucap) dan "Or run it directly: …/Contents/MacOS/stackly-cli" tampil. Matikan toggle dan Remove: link hilang, file shell tidak berubah (`stat -f '%N %m' ~/.zshrc ~/.zprofile ~/.zshenv ~/.bash_profile` sebelum dan sesudah) (`docs/specs/0016-onboarding/verify.md:62`).
10. Blok contoh di langkah 5 hanya berisi `stackly daemon status` dan `stackly provider list`, tanpa `/usr/local/bin` dan tanpa "Requires your password once" (`docs/specs/0016-onboarding/verify.md:63`).
11. Tekan Escape saat konfirmasi Command Line terbuka: hanya konfirmasi tertutup (Cancel); Escape berikutnya melewati onboarding (`docs/specs/0016-onboarding/verify.md:64`).

**Milestone 3: Catalog Trust** (`docs/specs/0016-onboarding/verify.md:90` sampai `:94`).

1. Daemon berjalan (Terminal B dari bundle), buka app dengan argumen di atas dan pindah ke langkah 4: judul "Catalog sources", subjudul, satu baris berlabel "official" dengan detail "Maintained by the Stackly project and bundled with this app.", baris "Revision {n} · published {tanggal}", toggle menyala dan nonaktif, dan catatan trust di bawahnya. Revision dan tanggal sama dengan `STACKLY_ROOT="$R" $APP/stackly-cli provider list` (`docs/specs/0016-onboarding/verify.md:90`).
2. Arahkan pointer ke toggle dan baca dengan VoiceOver: help dan label "The official catalog is always on."; toggle tidak bisa diubah (`docs/specs/0016-onboarding/verify.md:91`).
3. Hentikan daemon, lalu jalankan dengan catalog pengembangan: `cargo xtask catalog build --out "$R/catalog"`, kemudian `STACKLY_ROOT="$R" STACKLY_CATALOG_DIR="$R/catalog" target/debug/stacklyd`. Label "development", detail menyebut `STACKLY_CATALOG_DIR` dan bahwa itu bukan catalog official, toggle "The development catalog is always on." (`docs/specs/0016-onboarding/verify.md:92`).
4. Buka langkah 4 saat daemon belum berjalan: label "Catalog", detail "Stackly shows where its catalog comes from once stacklyd is running.", tanpa toggle. Nyalakan daemon: baris berubah menjadi "official" tanpa pindah langkah (`docs/specs/0016-onboarding/verify.md:93`).
5. Baca seluruh teks langkah 4 terhadap AC-061: tidak ada baris community, tidak ada "Signed manifests", tidak ada kalimat tentang catalog terbaru atau up to date, tidak ada kalimat digest (`docs/specs/0016-onboarding/verify.md:94`).

**Yang dikirim:** versi macOS, dan per milestone nomor serta apa yang terlihat untuk setiap langkah yang tidak sesuai, atau "semua sesuai"; tulis "dilewati" untuk langkah agent yang tidak bisa kamu capai.

## N. F17 Catalog dan provider detail

Untuk F17 (spec 0017) milestone 3 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md): halaman Catalog di app nyata. Sumber: `docs/specs/0017-catalog-provider-detail/verify.md`. Differentiate Without Color, Increase Contrast, Reduce Transparency, dan VoiceOver hanya bisa kamu nyalakan (setting seperti I; keyboard dan VoiceOver seperti F dan J). Siapkan catalog pengembangan dan daemon, lalu app, dengan `R` dan `APP` dari bagian L:

```bash
cargo xtask catalog build --out "$R/catalog"
```

```bash
STACKLY_ROOT="$R" STACKLY_CATALOG_DIR="$R/catalog" target/debug/stacklyd
```

Buka app dengan `STACKLY_ROOT="$R" $APP/Stackly`, lalu pilih Catalog (⌘4). Catalog repo sudah berubah sejak verify.md ditulis (PostgreSQL dan NATS kini `available`, empat entri lain `planned`: `official/mysql`, `clickhouse`, `valkey`, `traefik`); catat apa yang kamu lihat bila itu berbeda dari teks langkah.

1. Judul "Catalog", subjudul "Development catalog · updated {tanggal}", kartu provider redup tanpa opacity untuk entri planned (verify.md menulis enam: Pg, My, Na, Ch, Vk, Tr), picker bersegmen All, Databases, Messaging, Analytics, Cache, Proxy, dan tidak ada tombol Get, New Instance, atau Import Provider (`docs/specs/0017-catalog-provider-detail/verify.md:90`).
2. Ketik "post" di field "Search providers" lalu "POST", lalu "postgresql": hanya kartu PostgreSQL tersisa, pilihan pindah ke kartu itu. Verify.md menulis panel detail dengan catatan planned "This provider isn’t available yet. It will appear here once packaging and tests pass." tanpa matrix, capability, atau tombol; karena PostgreSQL kini `available`, ulangi dengan "mysql" untuk memeriksa catatan planned itu (`docs/specs/0017-catalog-provider-detail/verify.md:91`).
3. Ketik "zzz": "No Results for “zzz”", pesan "No providers match “zzz”.", dan tombol Clear Filters mengosongkan pencarian. Pilih Messaging tanpa entri: pesan yang sama dengan nama segmen. Pilih All: semua kartu kembali (`docs/specs/0017-catalog-provider-detail/verify.md:92`).
4. Tekan ⌘F dari kartu mana pun: field pencarian di toolbar mendapat fokus, dan menekan ⌘F lagi saat sudah fokus tidak mengubah teksnya. Edit › Find › Find… aktif di Catalog dan nonaktif di Overview (`docs/specs/0017-catalog-provider-detail/verify.md:93`).
5. Pindah ke Instances lalu kembali ke Catalog, tutup dan buka jendela: kartu yang sama tetap terpilih; filter dan teks pencarian kembali kosong (`docs/specs/0017-catalog-provider-detail/verify.md:94`).
6. Dengan catalog fixture: kartu NATS "Available", panel detail menulis matrix versi (macOS Unverified, Linux Unsupported, Windows Planned) dengan catatan alasan di bawahnya, tiga baris dasar dan delapan capability ("Reload without restart · not supported"), baris License, Source, Download, Disk ("Not listed"), Requires ("macOS 13.0 or later on Apple Silicon"), Dependencies ("None declared"), Publisher, Revision, dan Trust. Kartu `intelonly` "Unsupported" dengan baris alasan; `blockedhost` "Blocked: …"; paket rusak "Invalid" dengan banner "This provider failed validation" dan kode `manifest.invalid`; banner di atas grid "{n} providers failed validation.". Folder catalog fixture ini belum ada sebagai file: verify.md menyebut `CatalogBuilder` (tes Rust) atau salinan `crates/core/tests/fixtures/catalog/`, yang tidak ada di repo (paket fixture `blockedhost`, `intelonly`, dan `linuxonly` ada di `crates/core/src/catalog/fixtures/`). Minta Claude menyiapkan folder `STACKLY_CATALOG_DIR` untuk langkah ini lebih dulu; bila belum ada, tulis "dilewati" (`docs/specs/0017-catalog-provider-detail/verify.md:95`).
7. Perkecil jendela ke 900 × 600 pt dengan inspector terbuka: grid satu kolom, picker kategori menjadi menu, tidak ada teks terpotong di kartu atau panel, inspector paling lebar 280 pt; matrix versi berubah menjadi daftar per OS (`docs/specs/0017-catalog-provider-detail/verify.md:96`).
8. Hentikan `stacklyd`: banner jendela "Can’t connect to stacklyd." dan halaman Catalog menampilkan state tunggu. Mulai lagi: catalog kembali tanpa memuat ulang app (`docs/specs/0017-catalog-provider-detail/verify.md:97`).
9. Nyalakan Full Keyboard Access (System Settings › Keyboard), lalu hanya dengan keyboard: Tab ke field pencarian, picker kategori, setiap kartu (Return atau Space memilih), dan isi panel detail termasuk link lisensi. Semua dapat dicapai dan fokus terlihat (`docs/specs/0017-catalog-provider-detail/verify.md:98`).
10. VoiceOver (⌘F5): kartu dibaca "NATS Server, Messaging, Available", sel matrix "macOS, Unverified: Not tested on this Mac yet.", capability "Reload without restart, not supported" dan "Metrics, supported"; tile tidak dibaca (`docs/specs/0017-catalog-provider-detail/verify.md:99`).
11. Nyalakan Differentiate Without Color: badge status menulis simbol tone di depan label dan tidak ada status yang hanya berbeda warna. Nyalakan Increase Contrast dan Reduce Transparency, lalu ulangi di light dan dark: semua teks terbaca, kartu planned tidak redup (`docs/specs/0017-catalog-provider-detail/verify.md:100`).
12. Atur jam Mac atau pakai catalog dengan `published_at` lebih dari 180 hari: banner "Showing the last known state from {tanggal}." dengan "The development catalog last updated {tanggal}." dan tidak ada teks "latest", "up to date", atau "newer catalog" di layar. Mengubah jam Mac memengaruhi hal lain di sistem; kembalikan ke otomatis sesudahnya (`docs/specs/0017-catalog-provider-detail/verify.md:101`).
13. UI test menu (Automation Mode seperti K): `cargo xtask ui-test --only MenuUITests`. Verify.md menulis `--only <kelas menu>`; kelas menu yang dikenal repo adalah `MenuUITests` (lihat L, kelompok Automation Mode, langkah 2). Item Edit › Find › Find… aktif di Catalog dan nonaktif di Overview untuk setiap skenario yang memuat catalog. Tanpa Automation Mode, exit 2 (`docs/specs/0017-catalog-provider-detail/verify.md:109`).

**Yang dikirim:** versi macOS, dan nomor serta apa yang terlihat untuk setiap langkah yang tidak sesuai, atau "semua sesuai"; untuk langkah 13 exit code dan nama test yang gagal bila ada.

## O. F18 Install runtime managed

Untuk F18 (spec 0018) milestone 4 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md): halaman Runtimes dan tombol Get. Sumber: `docs/specs/0018-install-runtime-managed/verify.md`. Langkah 1 sampai 5 memakai skenario fixture dan tidak butuh daemon:

```bash
STACKLY_UI_SCENARIO=connected-mixed $APP/Stackly
```

Langkah 6 sampai 9 memakai Catalog dengan daemon dari bundle (`STACKLY_ROOT="$R" $APP/stacklyd`, app `STACKLY_ROOT="$R" $APP/Stackly`); langkah 8 memasang runtime sungguhan dari jaringan. Keyboard dan VoiceOver seperti F dan J.

1. Pilih Runtimes di sidebar: subjudul "4 installed", bagian Installing dengan satu baris "Install PostgreSQL 18.1" (tanda Pg, tahap dan persen, bar), tabel MySQL, NATS Server, PostgreSQL, dan Traefik dengan kolom Runtime, Source, Digest, dan Used By, tombol hapus abu-abu di tiap baris, dan catatan kaki mockup di bawah (`docs/specs/0018-install-runtime-managed/verify.md:122`).
2. Pilih satu baris lalu tekan Return atau pilih Show Inspector: inspector membuka dengan ID (tombol salin), Software, Version, Line, Target, Source, State, Support dan record penentunya, Digest penuh, ukuran, Path, Signature, Installed, Verified, Used By, Executables, Dependencies, dan Warnings (`docs/specs/0018-install-runtime-managed/verify.md:123`).
3. Arahkan pointer ke tombol hapus: bantuan "Used by 1 instance" untuk runtime yang dipakai dan "Removing runtimes arrives in a later build" untuk yang tidak dipakai; tombol tetap nonaktif (`docs/specs/0018-install-runtime-managed/verify.md:124`).
4. Klik kanan baris Installing (atau pilih lewat keyboard dan buka menu konteksnya): Cancel Install. Selama tahap Commit item nonaktif dengan alasan "Can’t be cancelled during Commit" (`docs/specs/0018-install-runtime-managed/verify.md:125`).
5. Persempit jendela ke 900 pt dengan sidebar dan inspector terbuka (kolom halaman 384 pt): kolom Source dan Digest hilang, teks Runtime dan Used By membungkus, bagian Installing dan catatan kaki membungkus tanpa teks terpotong (`docs/specs/0018-install-runtime-managed/verify.md:126`).
6. Buka Catalog, pilih provider `available` dengan runtime terpasang: kartu menampilkan "{n} installed" dengan nada positive (bukan Available), dan tidak tampil untuk provider tanpa runtime. Pakai skenario fixture di atas atau root yang sudah berisi runtime dari langkah 8 (`docs/specs/0018-install-runtime-managed/verify.md:127`).
7. Di panel detail Catalog, kolom Runtime di matrix versi: versi tanpa runtime menampilkan tombol Get, versi yang sedang dipasang menampilkan persen dan bar kecil 76 pt, versi yang terpasang menampilkan "Installed" (`docs/specs/0018-install-runtime-managed/verify.md:128`).
8. Klik Get lalu pindah ke Overview sebelum selesai. Ini mengunduh dari jaringan ke `$R`: NATS 2.15.1 sekitar 6,8 MB (selesai dalam hitungan detik, jadi sulit ditinggalkan di tengah jalan); ukuran PostgreSQL ada di baris Download panel detail. Kartu "Installing {name} {version}. Staged, verified, then committed." muncul saat mulai, dan "{name} {version} installed and verified." muncul di jendela yang mana pun saat selesai. Ulangi dan batalkan lewat Cancel Install saat masih mengunduh: "Install of {name} {version} was cancelled. Nothing was kept.". Untuk PostgreSQL ada jalan tanpa jaringan: salin artefak dari cache xtask seperti T langkah 2 sampai 5 (tanpa `mkdir` untuk `$R/cache/downloads`), lalu klik Get; unduhan apa pun lewat jaringan tetap kamu yang menyetujui. Pasang PostgreSQL 18 dan 17 serta NATS 2.15.1 di sini dan jangan hapus `$R` bila kamu lanjut ke P, Q, dan S (`docs/specs/0018-install-runtime-managed/verify.md:129`).
9. Klik Get saat daemon tidak tersambung (hentikan daemon): tombol nonaktif dengan alasan gerbang mutasi. Bagian "saat daemon menolak (misalnya `runtime.selector_changed`)": panel detail menampilkan banner critical dengan pesan, hint, kode, dan Show Operations, tanpa kartu. Penolakan itu sulit dipancing dengan tangan; bila kamu tidak bisa memicunya, tulis "bagian penolakan dilewati" (`docs/specs/0018-install-runtime-managed/verify.md:130`).
10. Keyboard dan VoiceOver: Tab sampai tabel, panah memilih baris, VoiceOver membaca "PostgreSQL 17.6, managed, installed, verified, used by pg17-dev"; baris Installing membacakan "Stage 1 of 5, Download, 8 percent" dan mengumumkan tiap tahap sekali (`docs/specs/0018-install-runtime-managed/verify.md:131`).

**Yang dikirim:** versi macOS, dan nomor serta apa yang terlihat untuk setiap langkah yang tidak sesuai, atau "semua sesuai"; untuk langkah 8 ukuran unduhan yang terlihat dan apakah kartu muncul di Overview.

## P. F20 Create instance

Untuk F20 (spec 0020) milestone 3 dan 4 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md): sheet New Instance dan create PostgreSQL di app nyata. Sumber: `docs/specs/0020-create-instance/verify.md`. Butuh PostgreSQL 18 dan 17 terpasang (O langkah 8). Pakai daemon dari bundle dan app dengan `R` dan `APP` dari bagian L, root sementara. Keyboard dan VoiceOver seperti F dan J. Cari port bebas dengan `lsof -nP -iTCP:<port> -sTCP:LISTEN` (kosong berarti bebas).

1. Buka app dengan daemon development (root uji), pasang satu runtime, lalu ⌘N: sheet terbuka dengan fokus di Name dan teksnya terpilih. Urutan Tab: kategori, runtime, nama, baris port (Fixed atau Automatic, angka, tombol Use), field settings, Advanced, toggle, baris preflight, Copy, Cancel, Create. Return membuat saat Create aktif dan Escape menutup sheet (`docs/specs/0020-create-instance/verify.md:104`).
2. Dengan VoiceOver hidup, jalankan seluruh alur create sampai instance terpilih di Instances. Setiap field dibaca "{judul}, {kontrol}, required" dengan "set once at creation" untuk field yang tidak bisa berubah; error dibacakan di field-nya; setiap baris preflight dibaca "{judul}, Passed|Warning|Failed: {detail}"; perubahan ringkasan preflight diumumkan sekali ("Preflight passed.", "Preflight passed with {n} warnings.", "{n} preflight checks failed."); tahap progress diumumkan; hasil create diumumkan lewat kartu umpan balik (`docs/specs/0020-create-instance/verify.md:105`).
3. Port yang dipegang listener uji: `nc -l 127.0.0.1 <port>` di Terminal lain, lalu buka sheet dan isi port itu. Baris Port memuat simbol silang, "In use by another process" atau nama instance, "Fixed ports are never changed silently." dan tombol "Use {n}" yang hanya mengisi angka saat ditekan. Create mati dengan alasan "Fix the errors above.". Hentikan `nc`: baris menjadi centang dalam satu kali debounce (`docs/specs/0020-create-instance/verify.md:106`, `docs/specs/0020-create-instance/verify.md:144`).
4. Pasang PostgreSQL 18 lewat Catalog, lalu pilih New Instance… dari panel detail Catalog: sheet terbuka dengan runtime 18 terpilih. Isi nama `pg18-gui`, port Fixed dengan angka bebas, biarkan password `Generate`, nyalakan "Start after creating", tekan Create. Progress tahap: Checking, Pinning provider package, Storing secrets in Keychain, Creating folders, Registering, Initializing; lalu sheet menutup dan `pg18-gui` terpilih di Instances dengan status Running dan Ready. `STACKLY_ROOT="$R" $APP/stackly-cli instance get pg18-gui --json` memberi `endpoints[0].bound` true dan `socketPath` yang berakhir `.s.PGSQL.<port>`; bila perintah menolak nama, pakai ID dari `instance list` (`docs/specs/0020-create-instance/verify.md:142`).
5. Ulangi dengan PostgreSQL 17 (`pg17-gui`) di port lain: hasil yang sama, dan `SHOW server_version` lewat `psql` dari runtime 17 memberi 17.11. Cari `psql` dengan `find "$R" -path '*bin/psql'` dan pakai string connection penuh dari `instance connection pg17-gui --show-password` (itu mencetak password instance uji ke Terminal) (`docs/specs/0020-create-instance/verify.md:143`).
6. Jalankan alur 18 dari awal sampai instance Ready hanya dengan keyboard (urutan fokus seperti langkah 1, Return untuk Create, Escape untuk Cancel): tidak ada langkah yang butuh mouse (`docs/specs/0020-create-instance/verify.md:145`).
7. Ulangi alur 18 dengan VoiceOver hidup: label, nilai, error, dan status setiap baris preflight terbaca sebagai teks, perubahan ringkasan preflight diumumkan sekali, tahap progress diumumkan, dan hasil create diumumkan. Sheet di lebar minimum 560 pt tidak memotong teks pada text size terbesar (System Settings › Accessibility › Display › Text size) (`docs/specs/0020-create-instance/verify.md:146`).
8. Catatan evidence (langkah 3 sampai 7): Claude yang membuat record, bukan kamu, dengan `cargo xtask evidence new --covers AC-027 NFR-012 --kind native --title "spec 0020 manual check: PostgreSQL create from the New Instance sheet" --expected "PostgreSQL 18 and 17 are created from the New Instance sheet with the keyboard and with VoiceOver, and the real port conflict shows in the sheet" --subject postgresql@18.6 --subject postgresql@17.11`. Record `PASS` perlu reviewer. Kirim hasilmu supaya record bisa diisi (`docs/specs/0020-create-instance/verify.md:147`).

**Yang dikirim:** versi macOS, dan nomor serta apa yang terlihat untuk setiap langkah yang tidak sesuai, atau "semua sesuai"; untuk langkah 4 dan 5 keluaran `instance get` dan `SHOW server_version`.

## Q. F21 Start, readiness, dan connection

Untuk F21 (spec 0021) milestone 3 dan 4 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md): halaman detail instance di app nyata dan run offline. Sumber: `docs/specs/0021-start-readiness-connection/verify.md`. Langkah 1 sampai 3 memakai instance fake (siapkan seperti C langkah 1 sampai 5, tetapi dengan app dari `$APP/Stackly`); langkah 4 dan 5 memakai PostgreSQL 18 dan NATS 2.15.1 yang terpasang (O langkah 8) dan dibuat lewat sheet New Instance (P langkah 4; NATS ada di S). Keyboard dan VoiceOver seperti F dan J.

1. Alur Start sampai Copy dengan keyboard saja: buka Instances, panah ke baris, Return membuka halaman detail, Tab melewati tombol kembali, aksi utama, More, banner dan tombolnya, setiap Copy di kartu Endpoints, Show Password dan Copy di kartu Connection, lalu Copy ID dan Recalculate data size di kartu Details. ⌘[ kembali ke tabel dengan baris tetap terpilih; ⌘R menjalankan Start (`docs/specs/0021-start-readiness-connection/verify.md:93`).
2. Alur yang sama dengan VoiceOver: Process dan Readiness dibacakan sebagai dua elemen, tombol Copy endpoint berlabel "Copy {nama endpoint} address", hasil copy diumumkan, dan "{name} is ready." diumumkan sekali setelah readiness bertahan 2 detik (`docs/specs/0021-start-readiness-connection/verify.md:94`).
3. Dengan daemon development dan instance fake `readyDelayMs` panjang (misalnya `--set-json readyDelayMs=20000` pada `instance create`, agar kamu sempat melihatnya): header "Starting…", baris Process "Running (PID …)" dan Readiness "Waiting for probe", lalu "Running" dan "Ready" tanpa memuat ulang halaman (`docs/specs/0021-start-readiness-connection/verify.md:95`).
4. Start dari GUI sampai `ready`, dengan app dan daemon pada root sementara: untuk instance PostgreSQL 18 buka halaman detail dari Instances dan tekan Start. Header "Starting…" lalu "Running", Readiness "Ready" tanpa memuat ulang halaman. Di kartu Connection, Show Password menampilkan string `uri` penuh dan Copy Connection String menaruh string penuh di pasteboard. Tempel ke Terminal dan jalankan `psql "<string tempel>" -c "SELECT 1"` dengan `psql` milik runtime (`find "$R" -path '*bin/psql'`): keluaran `1`. String tersamar di kartu sama persis dengan keluaran `STACKLY_ROOT="$R" $APP/stackly-cli instance connection <nama>` (`docs/specs/0021-start-readiness-connection/verify.md:145`).
5. Untuk NATS: kartu Connection menunjukkan `nats://127.0.0.1:<port>` dan `http://127.0.0.1:<monitoring>`; `nc 127.0.0.1 <port>` mencetak baris `INFO {…"version":"2.15.1"…}` (keluar dengan Ctrl-C) (`docs/specs/0021-start-readiness-connection/verify.md:146`).
6. Run offline. Mematikan jaringan juga memutus sesi Claude yang sedang berjalan, jadi lakukan ini saat tidak ada sesi yang bekerja. Verify.md juga menyebut sesi lain memakai Mac yang sama. Kerjakan dari root repo di checkout utama pada `main` yang bersih, dengan instance PostgreSQL dan NATS dari langkah 4 dan 5 di root yang sama (`docs/specs/0021-start-readiness-connection/verify.md:147`, `docs/specs/0021-start-readiness-connection/verify.md:154`):
   1. Sebelum mematikan jaringan, bangun dulu: `cargo test -p stacklyd --test connection_proof --locked --no-run`
   2. Matikan Wi-Fi dan Ethernet (hanya kamu yang melakukannya), lalu pastikan `route -n get default` gagal.
   3. Jalankan: `STACKLY_PROOF_OFFLINE=1 cargo xtask evidence run --kind native --out evidence/connection-tmp -- cargo test -p stacklyd --test connection_proof --locked offline -- --ignored`. Dua test, `postgresql_18_offline_start_status_connection_and_inspect` dan `nats_offline_start_status_connection_and_inspect`, harus PASS. Test menolak jalan bila `STACKLY_PROOF_OFFLINE` tidak di-set atau `route -n get default` masih berhasil, jadi run online tidak pernah terhitung sebagai bukti offline.
   4. Masih tanpa jaringan, buka app dengan root instance tadi, Start instance PostgreSQL dan NATS dari GUI, buka tab Summary, Show Password, dan Copy Connection String: semuanya bekerja seperti online dan tidak ada banner offline atau error jaringan.
   5. Nyalakan jaringan lagi. Folder `evidence/connection-tmp` biarkan di tempatnya; Claude yang memindahkannya ke `evidence/connection/<runId>` dan mencatat di verify.md.

   Verify.md menulis "nyalakan jaringan lagi" sebelum langkah GUI di kalimat yang sama dengan "masih tanpa jaringan"; aku menaruh langkah GUI sebelum jaringan dinyalakan.

**Yang dikirim:** versi macOS, dan nomor serta apa yang terlihat untuk setiap langkah yang tidak sesuai, atau "semua sesuai"; untuk langkah 6 keluaran ringkas `evidence run` (jumlah PASS dan masalah) dan hasil langkah 6.4.

## R. F22 Overview dan daftar instance

Untuk F22 (spec 0022) milestone 3 dan 4 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md): Overview dan Instances di app nyata. Sumber: `docs/specs/0022-overview-daftar-instance/verify.md`. Enam catatan evidence manual sudah ada sebagai `NOT_RUN` ("Spec 0022 manual check 1" sampai "6" di `evidence/manual/`); Claude yang mengisinya dari hasilmu. Keyboard dan VoiceOver seperti F dan J; Increase Contrast seperti I.

**Milestone 3, skenario fixture** (`docs/specs/0022-overview-daftar-instance/verify.md:73` sampai `:78`). Jalankan `STACKLY_UI_SCENARIO=connected-mixed $APP/Stackly` dan buka Overview.

1. Empat kartu menjadi tiga (Running, Stopped, Failed; kartu Runtimes dicakup milestone 4), Running memuat "{n} not ready", angka Failed merah, subtitle toolbar "{ready} running · {stopped} stopped · ...", Needs Attention memuat enam item dengan "Show {n} More", Instances enam baris dengan "Show All {n} Instances", dan Operations hanya operasi yang berjalan (`docs/specs/0022-overview-daftar-instance/verify.md:73`).
2. Klik kartu Failed: Instances terbuka dengan filter Failed, pencarian kosong, dan jumlah baris sama dengan angka kartu. Ulangi untuk Running (jumlah baris sama dengan `ready` ditambah `notReady`) dan Stopped (`docs/specs/0022-overview-daftar-instance/verify.md:74`).
3. Di Instances, ketik "5432 pg" di pencarian: hanya instance yang cocok di kedua kata. Klik header Version dua kali: urutan naik lalu turun dengan nilai kosong selalu di akhir. Menu View › Sort Instances By memberi checkmark di kolom dan arah yang aktif. ⌥⌘I menyembunyikan dan menampilkan inspector (`docs/specs/0022-overview-daftar-instance/verify.md:75`).
4. Pilih instance gagal: inspector memuat banner "{name} failed" dengan cuplikan output per baris. Pilih instance running: tombol Stop. Menu Instance › Stop (⇧⌘R) menghentikannya tanpa konfirmasi dan tombolnya sibuk sampai operasi selesai (`docs/specs/0022-overview-daftar-instance/verify.md:76`).
5. Hanya keyboard: Tab ke kartu Failed, Space, pilih baris dengan panah, ⌥⌘I, Tab ke tombol aksi di inspector, Space (`docs/specs/0022-overview-daftar-instance/verify.md:77`).
6. Lebar konten 640 pt: Overview satu kolom tanpa teks terpotong; Instances dengan inspector tertutup menampilkan Name dan Status tanpa menggulir; jendela 900 pt dengan inspector terbuka menampilkan Name, Status, dan aksi (`docs/specs/0022-overview-daftar-instance/verify.md:78`).

**Milestone 4, daemon nyata** (`docs/specs/0022-overview-daftar-instance/verify.md:120` sampai `:125`). Siapkan daemon development dengan runtime fake seperti C langkah 1 sampai 5, dengan app dari `$APP/Stackly`. Setting `providers/fake` yang berguna: `readyDelayMs` (not ready), `crashAfterMs` (crash loop), `failInit`.

1. Siapkan instance fake di kelompok ready, not ready (lewat `readyDelayMs`), stopped, failed (crash loop), dan data in use (lock dipegang proses lain, aturan I3). Satu cara menahan lock: `find "$R" -name instance.lock`, lalu `python3 -c "import fcntl,sys,time; f=open(sys.argv[1],'a'); fcntl.flock(f, fcntl.LOCK_EX); time.sleep(3600)" <path>` (cara ini saranku, bukan dari verify.md; lock instance adalah `flock` eksklusif). Kartu, subjudul, Needs Attention, dan `stackly instance list` menunjukkan jumlah dan kelompok yang sama. Kartu Runtimes menunjukkan jumlah yang sama dengan subjudul "{n} installed" halaman Runtimes dan membuka Runtimes (`docs/specs/0022-overview-daftar-instance/verify.md:120`).
2. Disk kecil: `hdiutil create -size 3g -fs APFS -volname StacklySmall ~/stk-small.dmg`, `hdiutil attach ~/stk-small.dmg`, `STACKLY_ROOT` di dalam `/Volumes/StacklySmall`. Overview menampilkan S2 (di bawah 5 GB tersedia). Isi image sampai kurang dari 2 GB tersedia (misalnya `dd if=/dev/zero of=/Volumes/StacklySmall/filler bs=1m count=1100`, cek dengan `df -h`): S1 muncul dalam 60 detik. Sesudahnya hentikan daemon, `hdiutil detach /Volumes/StacklySmall`, dan hapus `~/stk-small.dmg` (`docs/specs/0022-overview-daftar-instance/verify.md:121`).
3. Hanya keyboard: Overview, kartu Failed, Instances, pilih baris, inspector, Stop lewat tombol inspector, urutkan lewat menu View. Kartu Runtimes bisa difokus dan dijalankan dengan Space (`docs/specs/0022-overview-daftar-instance/verify.md:122`).
4. VoiceOver, alur yang sama: setiap status terbaca sebagai teks, kartu Runtimes terbaca "Runtimes, {n}" dengan hint "Shows Runtimes.", dan baris install terbaca "Install {nama} {versi}, Running, {tahap}, {n} percent" (`docs/specs/0022-overview-daftar-instance/verify.md:123`).
5. Lebar konten 640 pt: Overview satu kolom tanpa teks terpotong (empat kartu), Instances dengan inspector terbuka menampilkan Name dan Status tanpa menggulir; light, dark, dan Increase Contrast; lalu jendela 900 pt dengan inspector terbuka (kolom halaman 384 pt) (`docs/specs/0022-overview-daftar-instance/verify.md:124`).
6. Hentikan `stacklyd`: kedua halaman tetap menampilkan state terakhir, tombol aksi nonaktif dengan alasan. Jalankan lagi: state disegarkan (`docs/specs/0022-overview-daftar-instance/verify.md:125`).

**Yang dikirim:** versi macOS, dan nomor serta apa yang terlihat untuk setiap langkah yang tidak sesuai, atau "semua sesuai"; untuk langkah 2 milestone 4 level yang muncul (S1 atau S2) dan berapa detik sampai S1.

## S. F23 Provider NATS

Untuk F23 (spec 0023) milestone 4 di [scope/02-instalasi-pertama.md](scope/02-instalasi-pertama.md): langkah GUI AC-13. Sumber: `docs/specs/0023-provider-nats-manifest/verify.md`. Butuh NATS 2.15.1 terpasang (O langkah 8), daemon dari bundle dan app dengan `R` dan `APP` dari bagian L, root sementara. Keyboard dan VoiceOver seperti F dan J.

1. Sheet New Instance untuk NATS: pilih runtime NATS 2.15.1 yang terpasang, isi nama, dan lihat baris "Server name" terkunci (read only, tidak bisa diketik), toggle JetStream aktif, grup Advanced tertutup dengan tiga angka dan batasnya, dan dua baris endpoint Client (4222) dan Monitoring (8222) dengan pilihan Fixed atau Automatic. Biarkan Automatic, nyalakan "Start after creating", tekan Create: sheet mengikuti operasi sampai selesai, instance muncul di Instances (`docs/specs/0023-provider-nats-manifest/verify.md:117`).
2. Buka halaman detail instance NATS itu: tab Summary menampilkan Process "Running" dan Readiness "Ready" sebagai dua baris terpisah, kartu Endpoints menunjuk port yang dipilih daemon, dan kartu Connection `nats://127.0.0.1:<port>`. Ulangi dengan JetStream dimatikan: readiness tetap "Ready" (`docs/specs/0023-provider-nats-manifest/verify.md:118`).
3. Seluruh alur di atas dengan keyboard saja (Tab melewati nama, runtime, setiap baris form, Advanced, kedua baris endpoint, toggle Start after creating, Create, dan Cancel; Return membuka Advanced; Escape membatalkan) dan dengan VoiceOver (baris managed dibacakan sebagai read only dengan nilainya, toggle JetStream sebagai switch, setiap batas integer dibacakan, pilihan Fixed atau Automatic berlabel per endpoint, dan hasil Create diumumkan) (`docs/specs/0023-provider-nats-manifest/verify.md:119`).

**Yang dikirim:** versi macOS, dan nomor serta apa yang terlihat untuk setiap langkah yang tidak sesuai, atau "semua sesuai".

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
