# Langkah manual untuk user: `01-foundation`

Empat langkah manual menahan F11 milestone 4 dan 6, serta F14 milestone 4 dan 5. Jalankan dari root repo di checkout utama (`main` ≥ `cdd1434`), di Terminal sungguhan dengan sesi GUI. Kirim hasilnya ke sesi lead. Lead meneruskannya ke sub sesi yang mengisi record di `evidence/manual/`.

## A. Deny dan Cancel di login Keychain (F11 langkah 7)

Record: `evidence/manual/2026-10-20-spec-0011-step-7-deny-and-cancel-on-the-login-ke.json` dan `...-keychain-dialog-after-an-ad-hoc.json`. Daemon memakai root default (`~/Library/Application Support/Stackly`, dibuat bila belum ada), dan item uji masuk ke login Keychain. Langkah 7 di bawah menghapusnya lagi.

1. Bangun daemon dan CLI: `cargo build -q -p stacklyd -p stackly-cli`
2. Di Terminal A, jalankan `cargo run -q -p stacklyd` dan biarkan tetap jalan.
3. Di Terminal B, jalankan `cargo run -q -p stackly-cli -- secret set local:user/manual-keychain-check --generate`. Perintah harus berakhir dengan exit 0; catat bila muncul dialog.
4. Di Terminal A, tekan Ctrl-C, lalu jalankan `touch apps/daemon/src/main.rs && cargo build -q -p stacklyd && cargo run -q -p stacklyd`.
5. Di Terminal B, jalankan `cargo run -q -p stackly-cli -- secret reveal local:user/manual-keychain-check` sebanyak tiga kali:
   - (a) Klik Deny.
   - (b) Klik Cancel, atau tekan Escape.
   - (c) Tambahkan `</dev/null | cat` di akhir perintah. Hasilnya harus langsung gagal tanpa dialog.

   Setiap kali, catat teks dialog, tombolnya, exit code, dan kode error.
6. Jalankan `cargo run -q -p stackly-cli -- secret status local:user/manual-keychain-check`, lalu `cargo run -q -p stackly-cli -- secret list`. Keduanya harus berjalan tanpa dialog dan menampilkan status `present`.
7. Bersihkan:
   - `cargo run -q -p stackly-cli -- secret delete local:user/manual-keychain-check`
   - `security delete-generic-password -s stackly-secret -a user/manual-keychain-check` (abaikan pesan "could not be found")
   - Ctrl-C daemon di Terminal A.

Yang dikirim: hasil langkah 3, 5a, 5b, 5c, dan 6, plus versi macOS.

## B. Alur GUI secret (F11 langkah 11)

Record: `evidence/manual/2026-10-20-spec-0011-step-11-secret-entry-and-reveal-flows.json`. Root sementara dengan keychain file dipakai, jadi login Keychain tidak disentuh. Gunakan hanya fixture credential, jangan password asli.

1. Di Terminal A, bangun daemon dan app: `cargo build -q -p stacklyd && cargo xtask build-app`. Tunggu sampai muncul `BUILD SUCCEEDED`.
2. Di Terminal A, buat root sementara: `export ROOT=$(mktemp -d /tmp/stk.XXXXXX); echo $ROOT`
3. Di Terminal A, jalankan daemon: `STACKLY_ROOT=$ROOT cargo run -q -p stacklyd 2>&1 | tee $ROOT.daemon.log` dan biarkan tetap jalan.
4. Di Terminal B, set `export ROOT=<path dari langkah 2>`, lalu salin fixture credential ke pasteboard: `printf 'stackly-fixture-credential-%s' $(uuidgen | tr -d - | tr A-Z a-z) | pbcopy`
5. Di Terminal B, buka app: `STACKLY_ROOT=$ROOT target/xcode/Build/Products/Debug/Stackly.app/Contents/MacOS/Stackly`. Jendela utama harus tersambung ke daemon.
6. Buka Window > Design Gallery > Secrets, lalu:
   1. Klik Check Status.
   2. Klik Open Entry Sheet, pilih Enter, lalu ketik `short`. Harus muncul "Use at least 8 characters.", dan Save tidak aktif.
   3. Coba Show lalu Hide.
   4. Tempel nilai dengan Cmd-V, klik Save, lalu Done.
   5. Klik Check Status lagi. Harus `present`, revision 1.
   6. Buka sheet lagi, ketik `another-fixture-value-1`, klik Save, lalu Replace, lalu Cancel.
7. Di baris connection string, klik Show. Teks harus tersembunyi lagi sendiri setelah sekitar 30 detik; ukur waktunya. Teks juga harus tersembunyi saat kamu klik Hide, saat app kehilangan fokus, dan saat jendela Gallery ditutup lalu dibuka lagi.
8. Klik Copy, lalu uji pasteboard:
   1. `pbpaste` di Terminal B harus menampilkan URI lengkap.
   2. 90 detik setelah klik, `pbpaste` harus kosong.
   3. Klik Copy lagi, lalu langsung jalankan `echo something-else | pbcopy`. Setelah 90 detik, `pbpaste` harus tetap menampilkan `something-else`.
   4. Klik Copy sekali lagi, lalu keluar dari app dalam 90 detik. `pbpaste` harus kosong. Sesudahnya buka app lagi seperti langkah 5.
9. Cek VoiceOver (Cmd-F5):
   - Teks tersamar dibaca "Password, hidden".
   - Tombol dibaca "Show Password", "Copy Password", lalu "Hide Password".
   - Setelah 30 detik terdengar "Password hidden".

   Lalu cek keyboard saja, dengan Keyboard navigation menyala di System Settings: Tab dan Space harus bisa menjangkau dan menjalankan semuanya, dan Escape membatalkan sheet. Terakhir, lihat halaman ini dengan Increase contrast menyala dan dengan lebar jendela paling sempit.
10. Pindai kebocoran:
    - `grep -rl --exclude-dir=keychain 'stackly-fixture-credential-' $ROOT $ROOT.daemon.log; echo exit=$?` harus `exit=1` tanpa nama file.
    - `log show --last 20m --predicate 'process == "Stackly"' | grep -c 'stackly-fixture-credential-'` harus `0`.
11. Bersihkan:
    - Klik Delete Reference… lalu konfirmasi.
    - Keluar dari app dengan Cmd-Q.
    - Ctrl-C daemon di Terminal A.
    - `rm -rf $ROOT $ROOT.daemon.log`

Yang dikirim: versi macOS, waktu terukur untuk cek 30 detik dan 90 detik, serta nomor dan apa yang terlihat untuk setiap langkah yang tidak sesuai.

## C. Walking skeleton lewat CLI dan GUI (F14 langkah 10, AC-001)

Record: `evidence/manual/2026-10-20-walking-skeleton-through-the-cli-and-gui.json` (kind `fixture` sesuai spec 0014 AC-16). Pakai root sementara; login Keychain tidak disentuh. Jalankan di `main` ≥ `8ecf808` dengan tree bersih. Siapkan tiga terminal: A untuk perintah, B untuk daemon, C untuk app.

1. Terminal A: `cargo build -p stacklyd -p stackly-cli -p stackly-fake-server && cargo build -p stacklyd --example skeleton_seed && cargo xtask build-app`
2. Terminal A: `export R="$(mktemp -d /tmp/stk-skel.XXXXXX)"; S=target/debug/stackly`, lalu `target/debug/examples/skeleton_seed --root "$R"`. Catat UUID di baris `runtime` sebagai RT.
3. Terminal B: `STACKLY_ROOT="$R" target/debug/stacklyd`. Biarkan tetap jalan.
4. Terminal C: `STACKLY_ROOT="$R" target/xcode/Build/Products/Debug/Stackly.app/Contents/MacOS/Stackly`, lalu pilih Instances. Footer harus menampilkan `stacklyd running` dan halaman menampilkan "No Instances".
5. Terminal A: `STACKLY_ROOT="$R" $S instance create demo --runtime $RT --provider testing/fake --revision 0.2.0 --set-json readyDelayMs=3000 --wait`. Baris `demo` harus muncul dengan status Stopped tanpa diklik. Catat UUID-nya sebagai ID.
6. `STACKLY_ROOT="$R" $S instance start $ID --wait`. Baris harus berubah dari Starting ke Running (siap dalam sekitar 3 detik) tanpa diklik. Lalu jalankan `STACKLY_ROOT="$R" $S instance get $ID` dan catat PID-nya. Cek juga `ps -axww -o pid=,args= | grep "$R" | grep "stackly-fake-server serve" | grep -v grep`: hasilnya satu baris dengan PID yang sama.
7. Tutup jendela app dengan tombol merah. Notice "Stackly is still running" harus muncul, dan PID di `ps` serta `instance get` tidak berubah. Klik Stackly di Dock: jendela kembali dengan baris `demo` Running.
8. Tekan ⌘Q. `instance get $ID` harus tetap `running` dengan PID yang sama. Buka app lagi seperti langkah 4: baris `demo` Running.
9. Terminal B: tekan Ctrl+C. Banner "Can’t connect to stacklyd." harus muncul, baris `demo` tetap ada, dan proses fake masih hidup. Jalankan daemon lagi dengan perintah langkah 3: banner hilang, dan `instance get $ID` menunjukkan PID yang sama dengan status `adopted`.
10. Cek dulu dengan `pgrep -fl stacklyd` bahwa hanya daemon ini yang berjalan. Lalu `kill -9 $(pgrep -f target/debug/stacklyd)` dan jalankan daemon lagi. Hasilnya: PID sama, `adopted`, satu proses `serve`, dan tepat satu baris `demo`.
11. `STACKLY_ROOT="$R" $S instance create broken --runtime $RT --provider testing/fake --revision 0.2.0 --set-json failInit=true --wait` harus berakhir dengan exit 7 dan `supervisor.init_failed`, dan baris `broken` berstatus Failed. Catat operation ID-nya sebagai OP1, lalu jalankan `STACKLY_ROOT="$R" $S operation abandon $OP1 --yes`. Catat apakah baris `broken` masih ada.
12. `STACKLY_ROOT="$R" $S instance create slow --runtime $RT --provider testing/fake --revision 0.2.0 --set-json initDelayMs=20000`, tanpa `--wait`.
    - Tunggu sampai `ps -axww -o pid=,args= | grep "$R" | grep -v grep | grep "stackly-fake-server init"` menampilkan baris.
    - Begitu baris itu muncul, `kill -9` daemon dan jalankan lagi.
    - Hasilnya: satu baris `slow` berstatus Failed, `operation inspect <op>` menunjukkan `needs_recovery`, dan helper `init` hilang sendiri dalam 20 detik.
    - Terakhir jalankan `operation abandon <op> --yes`.
13. `STACKLY_ROOT="$R" $S instance stop $ID --wait`. Baris `demo` harus menjadi Stopped tanpa diklik, dan `ps ... | grep stackly-fake-server | grep -v grep` kosong.
14. Bersihkan:
    - Tekan Ctrl+C di daemon dan ⌘Q di app.
    - Pastikan `ps -axww -o pid=,args= | grep -E "stackly|$R" | grep -v grep` kosong.
    - Jalankan `rm -rf "$R"`, lalu cek `ls "$TMPDIR" | grep '^stackly-'`.

Yang dikirim: untuk setiap langkah yang tidak sesuai, nomornya dan apa yang terlihat, plus versi macOS. Lead meneruskannya ke sub sesi yang mengisi record.

## D. Keychain terkunci terlihat di app (F14 milestone 5, AC-19)

Satu baris di `docs/specs/0014-walking-skeleton/verify.md` Milestone 5 (bagian UI / manual, butir ketiga). Bagian CLI-nya sudah lulus oleh checker; yang tersisa hanya pengamatan di jendela app. Bisa dijalankan tepat sesudah bagian C (root `$R` dan instance `demo` yang sama), sebelum langkah pembersihan C.14.

1. Pastikan `demo` berhenti: `STACKLY_ROOT="$R" $S instance stop $ID --wait`.
2. Kunci keychain file root itu: `security lock-keychain "$R/data/keychain/stackly.keychain-db"`.
3. `STACKLY_ROOT="$R" $S instance start $ID --wait </dev/null`. Perintah ini harus gagal dengan `secret.needs_secret` dan tanpa dialog. `STACKLY_ROOT="$R" $S instance get $ID` harus menampilkan `Blocker` `needs secret`. Di app, baris `demo` harus tetap Stopped.
4. Hentikan `stacklyd` (Ctrl+C di Terminal B), jalankan lagi, lalu ulangi start tanpa `</dev/null`. Start sekarang harus berhasil, dan baris `demo` di app menjadi Running.

Yang dikirim: apakah baris app tetap Stopped di langkah 3, dan apakah start di langkah 4 berhasil.
