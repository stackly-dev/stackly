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
