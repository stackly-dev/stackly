# Langkah manual: `05-saat-instance-gagal`

Langkah yang butuh mata dan tangan user di Mac ini. Tidak ada sesi yang menjalankannya. Kirim hasilnya ke lead (nomor langkah, sesuai atau tidak, versi macOS); lead meneruskannya ke checker yang mencentang `verify.md`.

## F33 · Banner gagal (spec 0033, milestone 4)

Disusun dari laporan `chk-f33-m3` (2026-11-10). Butuh app Debug di atas daemon nyata yang sudah memuat milestone 1 fitur 33 (daemon mengisi `lastFailure`, `suggestedPort`, dan stage `backoff`), di root sementara (`STACKLY_ROOT`), tidak pernah Application Support asli.

1. Buat instance fake yang crash sebelum ready (`crashAfterMs` kecil), tekan Start, buka tab Summary: banner `caution` "Restarting after a crash" dengan "Restart {n} of 5 in {s} s." yang menghitung mundur tiap detik, cuplikan excerpt, dan Show Logs (AC-12).
2. Tunggu sampai budget habis: banner `critical` "Crash loop. Automatic restarts paused.", isi "It crashed 6 times in 5 minutes, so Stackly stopped restarting it and is waiting for you." plus hint, blok kode "supervisor.retry_budget_exhausted · operation {uuid}" lalu excerpt, tombol Retry (utama), Show Logs, Diagnose…, dan tautan Show in Operations (AC-9).
3. Seret pilih teks di blok kode lalu Copy: teksnya bisa dipilih dan disalin (AC-9).
4. Tekan Show Logs: tab Logs instance itu terbuka (AC-16).
5. Tekan Show in Operations: halaman Operations terbuka (AC-16).
6. Tekan Diagnose…: halaman Diagnostics terbuka (AC-16).
7. Pilih instance itu di tabel Instances dengan inspector terbuka: banner ringkas berisi judul crash loop dan excerpt, tanpa operation ID dan tanpa tombol lain (AC-12).
8. Tekan Retry di banner: satu `instance.start` baru, tombol menjadi "Starting…" dengan spinner, lalu banner restart muncul lagi dengan budget baru (AC-13).
9. Hentikan `stacklyd` yang kamu jalankan untuk sesi ini sampai jendela menunjukkan daemon tidak terhubung: Retry dan Use Port nonaktif dengan alasan dari banner daemon; banner kegagalan tetap tampil dengan isi terakhir yang dimuat (AC-13).
10. Pegang port fixed instance PostgreSQL dengan `nc -l 127.0.0.1 <port>`, tekan Start: banner "Couldn’t start: port in use", isi "Port {port} of {endpoint} is held by another process." plus hint, tombol utama "Use Port {n}" (AC-10).
11. Tekan Use Port {n}: sheet Edit Ports terbuka dengan endpoint itu terisi {n} dan verdict dry run langsung tampil; port di registry belum berubah (`stackly instance inspect`) (AC-10).
12. Ketik port lain yang juga dipakai, atau biarkan `nc` kedua memegang {n} sebelum membuka sheet: verdict sheet menolak dan menawarkan saran barunya sendiri (AC-10).
13. Tekan Change Ports di sheet: port berubah; banner menyembunyikan tombol port dan menambah "Its ports changed. Retry to start with them."; tekan Retry: instance start di port baru (AC-10, AC-13).
14. Biarkan instance lain memegang port yang sama, tekan Start pada instance `stopped`: banner "Couldn’t start: port in use" dengan "Port {port} of {endpoint} belongs to {nama instance itu}." dan Use Port; lalu Stop menyembunyikan banner, rename tidak (AC-10, AC-11).
15. Bandingkan dengan `stackly instance inspect <name>` untuk instance yang sama: kode, operation ID, dan port saran sama dengan banner (AC-9, AC-10).
16. Ulangi dengan app di mode gelap dan jendela sempit (lebar konten sekitar 640 pt): tidak ada label tombol atau baris kode yang terpotong; tombol pindah ke dua baris bila tidak muat (AC-17).

## F33 · Daemon yang terlihat (spec 0033, milestone 1), opsional

Dari laporan `dev-f33-m1` (2026-11-10). Root sementara: `export STACKLY_ROOT=$(mktemp -d)`.

1. Jalankan `stacklyd` dan `skeleton_seed`, buat instance fake dengan `--set crashAfterMs=50 --set readyDelayMs=60000`, lalu `stackly instance start <name> --wait`: baris `crashed, restart <n> of 5 at <waktu>` di setiap jeda, exit 7 dengan hint statis dan `operation: <uuid>`; `stackly instance inspect <name> --json` punya `lastFailure.at` dan `exitCode`.
2. Hentikan daemon, jalankan lagi, `inspect --json`: kode tetap `crash_loop`, sedangkan `at`, `exitCode`, dan `signal` null.

## F35 · Halaman Operations (spec 0035, milestone 2)

Dari laporan `dev-f35-m2` (2026-11-10). App Debug di atas daemon nyata dengan provider fake di root sementara.

1. Halaman Operations menggantikan placeholder; start sebuah instance dan lihat progress bertahap berjalan langsung.
2. Install runtime: Cancel nonaktif di tahap Commit dengan teks "can’t be cancelled".
3. Start yang gagal karena port dipegang proses lain, lalu Retry dari halaman Operations.
4. Pilih baris: inspector menampilkan jenis, resource, actor dan client, waktu, tahap, dan error.
5. Operasi `needs_recovery`: Resume dan Abandon di inspector, Abandon dengan dialog destruktif.
6. Baris operasi di Overview dan item O1 membuka operasi itu di halaman Operations.
7. Operasi yang sudah dipangkas menampilkan "This operation is no longer in the history."
8. Daemon dihentikan: data tetap tampil dengan aksi mutasi nonaktif; daemon lebih lama menampilkan "Operations Unavailable".
9. Ulangi di mode terang dan gelap, dan jendela 900 pt.

## F35 · CLI audit (spec 0035, milestone 1)

Dari laporan `dev-f35-m1-s5` (2026-11-10). `stacklyd` asli dan provider fake di root sementara: `export STACKLY_ROOT=$(mktemp -d)`, lalu `cargo run -q -p stacklyd --example skeleton_seed -- --root "$STACKLY_ROOT"` dan perintah create yang dicetaknya.

1. `COLUMNS=100 stackly audit list`: kolom TIME sampai OPERATION utuh, DETAILS terpotong dengan `…` di akhir dan paling sedikit 24 kolom. `COLUMNS=100 stackly audit list | cat` dan `stackly audit list` tanpa `COLUMNS` yang diekspor: tidak ada yang terpotong (AC-16).
2. `stackly operation list` dan `stackly operation inspect <id>` untuk operasi yang sedang berjalan: kolom CANCEL `yes` (atau `no` sejak langkah yang tidak bisa dibalik) dan baris `Cancel` dengan kalimatnya; operasi yang sudah selesai: CANCEL `-` dan tanpa baris `Cancel` (AC-16).

## F35 · Tab Activity, setting, dan langkah 10 (spec 0035, milestone 3 dan 4)

Dari laporan `dev-f35-m3` (2026-11-10). App Debug di atas daemon asli dan provider fake di root sementara (`STACKLY_ROOT=$(mktemp -d)`, `skeleton_seed`).

1. Buka detail sebuah instance: picker "Summary, Configuration, Logs, Metrics, Activity". Tab Activity menampilkan tabel Time, By, Activity, urut terbaru, dengan tooltip tanggal dan jam lengkap (AC-15).
2. Dengan tab terbuka, start dari app lalu stop dari CLI: baris "Started" (App) dan "Stopped" (CLI) muncul sendiri. Start pada port yang dipegang proses lain: "Start failed · supervisor.endpoint_in_use" (AC-15).
3. Ganti port: "Ports changed: main 5432 → 5433"; bandingkan dengan `stackly audit list --instance <nama>` (AC-15, AC-2).
4. Show Password lalu Copy: "Viewed superuser password" dan "Copied superuser password", tanpa nilai secret (AC-15).
5. Klik ganda baris operasi, atau klik kanan lalu Show in Operations: halaman Operations dengan operasi itu terpilih dan inspector terbuka (AC-13).
6. Lebih dari 50 baris: Show More. Daemon dihentikan lalu dijalankan lagi: baris tetap, lalu dimuat ulang. Daemon lama: "Activity Unavailable" (AC-15).
7. Duplicate… lalu Create: "Created from {sumber}"; create biasa: "Created" (AC-18).
8. Settings, Storage, grup Activity: "Keep activity for" 30 days dengan keterangan batas 100,000. Pilih 90: langsung tersimpan. Pilih 7: dialog "Remove activity older than 7 days?" dengan Cancel sebagai default; Remove: `stackly settings get` menulis 7 days. Revision basi: "Changed elsewhere. Try again.". Offline: grup tidak tampil (AC-20, AC-6).
9. Langkah 10 Build plan: install runtime fake dan lihat Cancel mati di Commit; start gagal karena port lalu Retry di Operations; buka operasi dari Overview dan dari tab Activity; ulangi semua di mode terang dan gelap (AC-11, AC-13, AC-15, AC-20).
10. `cargo xtask snapshot review`: PNG lama dan baru berdampingan untuk suite Operations, Activity, dan Settings (AC-19).

## F34 · Check doctor (spec 0034, milestone 1)

Dari laporan `chk-f34-m1` (2026-11-10).

1. Hanya bila SDK macOS berganti: ukur ulang layout `struct socket_fdinfo`, `socket_info`, `in_sockinfo`, dan `tcp_sockinfo` dari `sys/proc_info.h` dengan program C kecil (`offsetof` dan `sizeof`, `xcrun clang -arch arm64`) dan cocokkan dengan assert `const` di `crates/platform-macos/src/process/sys.rs` (`socket_fdinfo` 792 byte, `psi` di 24, `soi_kind` di 232, `soi_proto` di 240 dengan ukuran 528, `in_sockinfo` 80 byte, `tcp_sockinfo` 120 byte dengan `tcpsi_state` di 80) (AC-5).
2. Lintasan GUI nyata dan `binary_replaced` di bawah launchd adalah Manual A dan B milestone 5 fitur 34 (`BLOCKED` sampai dijalankan), ditulis saat milestone itu dibangun (AC-5, AC-8, AC-22).

## F34 · `stackly doctor` (spec 0034, milestone 4)

Dari laporan `dev-f34-m4` (2026-11-10).

1. Di Terminal yang berwarna, jalankan `stackly doctor` terhadap daemon yang berjalan: tanda ✓, !, ✗ berwarna hijau, kuning, merah. `stackly doctor | cat`: kata OK, WARNING, ERROR tanpa kode ANSI (AC-15).

## F34 · Manual A dan B (spec 0034, milestone 5, langkah 16) · `BLOCKED`, owner user

Dari laporan `dev-f34-m5` (2026-11-10).

1. Manual A (launchd asli): dengan build bertanda tangan yang terdaftar sebagai agent, ganti binary lewat `cargo xtask build-app --install`, hentikan daemon, buka Diagnostics dan jalankan `stackly doctor`: baris What to do `binary_replaced` tampil di seksi Background Agent dan di `stackly doctor`, lalu Start stacklyd menyalakan daemon lagi (AC-11, AC-22).
2. Manual B (lintasan GUI): di app nyata, buat port conflict lalu Run Doctor, tekan Use Port {port}: tab Configuration terbuka dengan sheet port dan port saran terisi, tanpa perubahan sampai Apply. Details… pada `pid_reused`: kedua proses dan setiap check identitas. Export Bundle…: daftar entri dulu, Export… membuka save panel di Downloads, lalu "Saved {file} to Downloads." dan Show in Finder (AC-17, AC-18, AC-19, AC-22).
4. Saat Manual B, periksa juga toolbar Diagnostics: Export Bundle… dan Run Doctor; tekan Run Doctor: tombol menjadi Running… dan nonaktif selama run, lalu kembali Run Doctor; saat stacklyd terputus keduanya nonaktif (AC-17; dari `tst-f34-test`, tidak bisa diuji otomatis).
3. Saat Manual B: badge Diagnostics, subjudul toolbar, Diagnose… dari banner gagal (fokus instance, Show All), item S Overview (fokus `disk_space`), Open Settings pada temuan disk (tab Storage) (AC-18, AC-20).
