# Verify: Process supervisor · spec 0010

_Langkah diturunkan dari acceptance criteria spec 0010. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Bagian Milestone 3 di bawah baru memuat langkah 10 (spool, `LogSink`, `SinkSet`, `TailSink`); langkah 9 dan 11 menambah bagiannya sendiri di bawah judul yang sama._

## Milestone 3: Crash, spool, and reconciliation

### UI / manual

- [ ] Langkah 10 tidak punya permukaan UI atau langkah manual: pembaca spool belum dipasang di actor (titik panggil menunggu `dev-f10-m2-s78`), jadi tidak ada perilaku yang bisa dilihat dari CLI atau app → AC-12

### Commands

- [ ] `cargo test -p stackly-core --test supervisor_spool` → 16 test lulus di atas spool nyata di `TestRoot` dengan pembaca spool fake: akhiran baris `\r\n` dibuang dan byte bukan UTF-8 menjadi U+FFFD; baris yang terpotong antar pembacaan ditahan sampai newline datang (chunk 5 byte); `read_at` batch memakai `Clock` daemon; nilai secret yang melintasi batas potongan 16 KiB pada baris 40 KiB tersamar sebelum dipotong dan tidak ada potongan yang memuat sebagian secret; baris di atas `LOG_LINE_HARD_MAX` keluar sebagai potongan `partial` dengan sisanya `continued`; sink `Busy` menahan batch tanpa membebaskan byte lalu batch yang sama terkirim sekali; `release_below` memakai ujung baris terakhir yang diterima (baris yang ditahan tidak pernah dibebaskan) dibulatkan ke satuan punch; simulasi 121 tick kali 1000 baris (lebih dari 6 MB) berakhir dengan `first_data_offset()` sama dengan offset yang dibebaskan di setiap tick; backlog di atas batas dibuang dari yang tertua dengan `truncated` sehingga byte yang dibuang ditambah byte yang terkirim sama dengan ukuran file; `resume` dari offset di atas 0 membuang byte sampai newline pertama dan batch pertama `resumed`, sedangkan dari offset 0 baris pertama tetap ada; `finish` mengirim baris tanpa newline sebagai `partial`; kegagalan `release_below` tidak menghentikan baris; `SinkSet` dengan `TailSink` → AC-12
- [ ] `cargo test -p stackly-core --lib supervisor::` → unit test `TailSink` (hanya `TAIL_LINES` terbaru, cuplikan stderr dulu lalu stdout dalam batas baris, baris dipotong 256 karakter, potongan `continued` tidak disimpan, `forget`), `SinkSet` (satu sink `Busy` membuat set `Busy`, ulangan batch yang sama melewati sink yang sudah menerima), dan pemotongan pada batas karakter UTF-8 → AC-12
- [ ] `cargo clippy -p stackly-core --all-targets --locked -- -D warnings` dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → keduanya lulus; `cargo xtask check-deps` → lulus

### Acceptance-criteria coverage

- AC-12 · bagian core (langkah 10): pemotongan baris, redaksi baris utuh sebelum dipotong, `LogSink` yang `Busy`, `release_below` ke ujung baris yang diterima, backlog dengan penanda truncation, resume dengan penanda `resumed`, `TailSink` untuk `lastFailure.excerpt`. Yang menunggu: sampel `st_blocks` di bawah 2 MiB selama 60 detik dengan `logLinesPerSecond` 2000 (butuh adapter macOS yang benar benar punch hole dan dummy server, jalan bersama test daemon), pembacaan oleh actor setiap `SPOOL_DRAIN_INTERVAL` dan pembacaan terakhir saat exit (titik panggil di actor setelah branch `dev-f10-m2-s78` masuk), dan `resume` saat adopsi (langkah 11)
- AC-9, AC-21, AC-22 · bagian spool dan redaksi log bergantung pada titik panggil actor dan rekonsiliasi; belum
