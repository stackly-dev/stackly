# 0033. Crash loop dan error start yang aman: rationale

[Kembali ke spec](index.md)

## Context

JOURNEY-05 PRD dimulai dari start yang gagal: kamu harus bisa tahu sebabnya, membuka log atau doctor, memperbaiki config atau dependency, lalu mencoba lagi, tanpa Stackly mereset data atau mengganti apa pun diam diam. Supervisor (spec 0010) sudah membatasi crash loop dengan backoff 1, 2, 4, 8, dan 16 detik kali jitter dan budget 5 per 5 menit, tanpa busy loop dan tanpa proses ganda, dan `InstanceSummary` sudah membawa `restart` (attempts, budget, windowSeconds, nextAttemptAt, exhausted) serta `lastFailure` (code, at, exitCode, signal, excerpt). Yang belum ada adalah tampilannya: banner I1 spec 0021 dan 0022 hanya berkata "{name} failed" dengan satu kalimat dan excerpt, inspector tidak membedakan crash loop dari port dipakai, dan CLI tidak menampilkan operation ID atau langkah lanjutan.

Saat menelusuri kode `main` (2026-11-10), ada empat celah yang menentukan desain. Pertama, `lastFailure.at`, `exitCode`, dan `signal` selalu null (`apps/daemon/src/ipc/instance/dto.rs`, komentar "the values of the failure in memory are not kept by the status yet"), jadi GUI dan CLI tidak bisa menampilkan exit code yang sudah dijanjikan DTO. Kedua, kegagalan port fixed di tahap `endpoints` terjadi sebelum `journal`, sehingga instance tetap `stopped` tanpa `lastFailure`: kasus paling umum di PRD 35.3 (port conflict) sama sekali tidak terlihat di halaman instance, hanya di kartu umpan balik yang hilang. Ketiga, katalog kode sudah mengizinkan detail `suggestedPort` di ketiga kode endpoint, dan jalur ganti port spec 0029 sudah mengisinya, tetapi jalur start tidak. Keempat, jeda backoff di dalam operasi start tidak punya stage sendiri, jadi `stackly instance start --wait` diam selama jeda sampai 16 detik.

Batasnya ketat. Brief koordinasi meminta field yang sudah ada diutamakan, `EventKind` tertutup (event kind baru berarti major), saran port wajib lewat alur ganti port spec 0029, data tidak pernah direset otomatis, dan isi doctor serta Operations milik spec 0034 dan 0035 yang ditulis paralel. Pendekatan Journey proyek berarti jalur ini harus selesai lengkap di GUI dan CLI, termasuk state kosong, offline, dan error, sebelum journey berikutnya.

## Options considered

### Keputusan 1: sumber error untuk banner

#### Option 1: operasi terbaru instance lewat `operation.list` dan event operasi (dipilih)

Banner membaca `InstanceSummary` ditambah operasi start terkait instance itu (aturan *Operasi terkait* di spec): satu `operation.list { resource, limit: 10 }` saat dibuka, lalu `OperationCenter.active` dan `recent` yang sudah mengikuti `operation.state_changed`. `failure.error` operasi membawa envelope lengkap: kode, pesan, hint, details (termasuk `suggestedPort`), dan `operationId`.

- Pro: tanpa perubahan kontrak; mencakup kegagalan sebelum `journal`; correlation ID gratis; sumber yang sama dipakai Operations (spec 0035) dan doctor (spec 0034).
- Con: satu request tambahan per instance yang dibuka; kegagalan tanpa operasi (crash setelah ready) tidak punya ID; bila daemon offline sebelum operasi dimuat, banner F7 tidak muncul.

#### Option 2: field baru di `InstanceLastFailure` (`operationId`, `endpoint`, `suggestedPort`)

- Pro: satu sumber di summary, tanpa request tambahan.
- Con: minor bump kontrak; tetap tidak mencakup kegagalan sebelum `journal` kecuali spec 0010 diubah supaya start yang ditolak menulis instance; saran port yang disimpan cepat basi.

#### Option 3: catat kegagalan start sebelum `journal` di registry

- Pro: banner selalu dari summary, bertahan setelah daemon restart.
- Con: membalik keputusan spec 0010 bahwa validasi yang gagal tidak mengubah instance; butuh migrasi; menambah transisi lifecycle baru.

#### Option 4: GUI mengingat error dari kliknya sendiri saja

- Pro: paling sederhana.
- Con: start dari CLI, respawn sistem, dan jendela lain tidak terlihat; hilang saat app ditutup.

### Keputusan 2: saran port bebas

#### Option 1: daemon mengisi `suggestedPort` di error start, banner membuka sheet Ports dengan isian (dipilih)

- Pro: detail sudah ada di katalog; pencarian sama dengan spec 0020 dan 0029; sheet menjalankan dry run lagi, jadi saran yang basi diganti saran baru; konfirmasi tetap di sheet spec 0029.
- Con: `EndpointsModel` mendapat parameter `prefill`; satu klik ekstra dibanding mockup.

#### Option 2: method read only baru `instance.suggest_ports`

- Pro: saran selalu segar saat banner tampil.
- Con: method dan minor bump baru untuk nilai yang sudah bisa dihitung di jalur error.

#### Option 3: banner membuka sheet tanpa saran

- Pro: tanpa perubahan daemon.
- Con: dry run spec 0029 hanya menilai endpoint yang berubah, jadi port yang sama tidak pernah menghasilkan saran; tidak memenuhi scope.

#### Option 4: banner langsung mengirim `change_endpoints` setelah dialog konfirmasi

- Pro: persis mockup (satu klik "Use Port 3308").
- Con: melewati verdict dry run dan sheet spec 0029; saran yang basi bisa diterapkan; dua jalur ganti port.

### Keputusan 3: backoff yang terlihat

#### Option 1: stage `backoff` di operasi start plus field `restart` yang sudah ada (dipilih)

- Pro: tanpa kontrak baru; Operations, `stackly operation inspect`, dan `--wait` langsung melihatnya; spec 0035 sudah memasukkannya ke grup Launch.
- Con: CLI membaca `instance.get` sekali per masuk stage untuk angka percobaan.

#### Option 2: progress `counted` (percobaan dari budget) di operasi start

- Pro: angka ada di operasi.
- Con: GUI menggambar bar yang mundur dan maju; arti `counted` (pekerjaan yang dihitung) menjadi kabur.

#### Option 3: CLI melakukan polling `instance.get` selama menunggu

- Pro: tanpa perubahan daemon.
- Con: polling terus menerus, dan Operations tidak tahu sedang jeda.

#### Option 4: event kind baru `instance.backoff`

- Con: `EventKind` tertutup, jadi major bump untuk informasi yang sudah ada di `instance.health_changed`. Ditolak.

### Keputusan 4: fakta kegagalan (`at`, `exitCode`, `signal`)

#### Option 1: di memori `InstanceStatus`, dipetakan bila kodenya sama dengan registry (dipilih)

- Pro: tanpa migrasi; sesuai janji DTO ("null after a daemon restart").
- Con: hilang setelah daemon restart.

#### Option 2: kolom registry baru (migrasi bebas berikutnya)

- Pro: bertahan setelah restart.
- Con: migrasi dan tulisan registry tambahan di jalur exit untuk nilai yang jarang dibutuhkan setelah restart.

#### Option 3: biarkan null

- Con: GUI dan CLI tidak bisa menampilkan exit code, AC-068 meminta last error terlihat. Ditolak.

### Keputusan 5: view banner

#### Option 1: `InstanceFailureBanner` baru dengan gaya surface yang sama (dipilih)

- Pro: tombol utama, tautan, teks yang bisa dipilih, dan hitung mundur tanpa mengubah komponen milik spec 0012 dan 0067.
- Con: dua view dengan gaya banner yang sama.

#### Option 2: perluas `StateBanner`

- Pro: satu komponen.
- Con: mengubah komponen dasar milik spec lain untuk satu pemakai; mengubah snapshot galeri.

### Keputusan 6: correlation ID yang ditampilkan

#### Option 1: operation ID (dipilih)

- Pro: sudah ada di envelope (`operationId`) dan di setiap baris log daemon operasi itu (spec 0001); sama di GUI, CLI, Operations, dan audit spec 0035.
- Con: kegagalan tanpa operasi tidak punya ID.

#### Option 2: request ID

- Con: hanya ada untuk request dari client itu, tidak untuk respawn sistem atau start dari client lain; CLI hanya mencetaknya dengan `-v`.

#### Option 3: field `correlationId` baru

- Con: duplikat operation ID, kontrak baru.

### Keputusan 7: hint pemulihan

#### Option 1: hint statis dari daemon lewat `codes::supervisor::hint`, plus perintah CLI di client (dipilih)

- Pro: satu teks untuk semua client (VAL-020); perintah dengan nama instance hanya bisa dibuat client.
- Con: dua lapis hint di CLI.

#### Option 2: hint hanya di client

- Con: GUI dan CLI menulis teks sendiri dan bisa berbeda.

### Keputusan 8: tempat perintah lanjutan CLI

#### Option 1: hint di blok error start dan restart, plus blok `Next steps:` di `inspect` (dipilih)

- Pro: muncul tepat saat gagal dan saat kamu memeriksa nanti.
- Con: golden `inspect` bertambah panjang untuk instance gagal.

#### Option 2: command baru `stackly instance why`

- Con: command baru yang harus kamu ketahui; doctor (spec 0034) sudah mengisi peran itu.

### Keputusan 9: Retry

#### Option 1: `instance.start` baru dengan key baru (dipilih)

- Pro: sama dengan aksi utama Retry spec 0022 dan Retry di Operations spec 0035; start pengguna dari `failed` memulai budget baru (spec 0010).
- Con: tidak ada.

#### Option 2: `operation.resume` operasi start yang gagal

- Con: operasi `failed` tidak bisa di resume (spec 0007); resume hanya untuk `needs_recovery`.

### Keputusan 10: berapa lama banner start yang ditolak (F7) tampil

#### Option 1: sampai operasi baru dimulai pada instance itu atau lifecycle berubah (dipilih)

- Pro: selalu menggambarkan keadaan terakhir; tidak butuh state di client.
- Con: instance yang ditolak port kemarin masih menampilkan banner hari ini sampai kamu bertindak.

#### Option 2: batas waktu (misalnya 1 jam) · Option 3: tombol tutup

- Con: banner hilang padahal sebabnya masih ada; tombol tutup butuh state per jendela.

## Rationale

Kekuatan utama di Context adalah bahwa hampir semua data sudah ada: budget dan jeda di `restart`, sebab di `lastFailure.code`, envelope lengkap di operasi, dan detail `suggestedPort` di katalog. Celahnya ada di daemon yang belum mengisi nilai itu dan di client yang belum menggabungkan dua sumber. Karena itu spec ini memilih mengisi yang sudah dijanjikan (Keputusan 4, 2) dan memakai operasi sebagai sumber error (Keputusan 1), bukan menambah field. Hasilnya tanpa perubahan kontrak, jadi tiga spec yang ditulis paralel (0033, 0034, 0035) bisa merge dalam urutan apa pun, dan doctor serta Operations membaca nilai yang sama.

Keputusan 1 juga satu satunya yang mencakup kegagalan sebelum `journal` tanpa membalik spec 0010. Port fixed yang dipakai proses luar adalah sebab paling umum start gagal, dan spec 0010 sengaja tidak mengubah instance untuk validasi yang gagal; mengubahnya (Option 3) menambah transisi lifecycle dan migrasi untuk sesuatu yang sudah tercatat rapi di journal operasi.

Saran port lewat sheet Ports (Keputusan 2) mengikuti aturan scope "bukan mengubah port tanpa sepengetahuan pengguna" secara harfiah: satu jalur penulis port, satu verdict dry run, satu konfirmasi. Mockup menunjukkan satu klik; spec ini menerima satu klik ekstra demi tidak menerapkan saran yang basi.

### Keputusan kecil

- Stage `backoff` diset tanpa langkah journal, karena percobaan kedua dan seterusnya memang tidak menulis langkah (`enter`). Cancel selama `backoff` tetap AC-8 spec 0010.
- Fakta kegagalan dipetakan hanya bila kodenya sama dengan `last_failure` registry, supaya fakta dari kegagalan lama tidak pernah menempel di kegagalan baru.
- Baris `operation:` di blok error CLI berlaku untuk semua command, bukan hanya start, karena NFR-011 berlaku untuk semua error operasi; golden error ikut diperbarui.
- `{window}` dibulatkan ke menit ("5 minutes"), sama dengan baris Restarts CLI (`div_ceil(60)`).
- `{otherName}` diambil dari `InstanceStore`; ID mentah tidak pernah tampil di isi banner.
- Hitung mundur dibulatkan ke atas dan berhenti di 0; saat `nextAttemptAt` null teksnya "Stackly is starting it again.".
- *Operasi terkait* memakai `limit: 10` dan empat jenis operasi (start, restart, stop, ganti port), supaya rename atau edit config yang lebih baru tidak menyembunyikan error start, dan supaya satu halaman kecil hampir selalu cukup.
- Inspector hanya menampilkan judul, excerpt, dan Use Port, mengikuti mockup dan lebar kolom 384 pt.
- Hint doctor CLI dan fokus Diagnostics ditandai `// provisional: F34`; `openOperation` ditandai `// provisional: F35` bila spec 0035 belum di `main`.

### Keputusan yang didelegasikan

User mendelegasikan keputusan ke opsi yang direkomendasikan (brief koordinasi wave 1, 2026-11-10). Semua keputusan di atas diambil dengan cara itu, tanpa pertanyaan ke user. Batas dengan spec 0034 (nama `openDiagnostics(focus:)`, label "Diagnose…", `stackly doctor --instance`) dan spec 0035 (`openOperation(_:)`, Retry sebagai `instance.start` baru, stage `backoff` di grup Launch) dikonfirmasi langsung dengan sesi architect masing masing pada 2026-11-10.

### Cross check

Cross check dengan model lain (Sonnet, read only, 2026-11-10) menemukan 17 hal. Semua dicek ulang ke kode `main` dan ditutup dengan pilihan yang direkomendasikan (keputusan didelegasikan):

1. Crash setelah ready: hanya jeda pertama yang tanpa operasi; setiap respawn adalah operasi sistem `start_instance` (`respawn.rs`). AC-1, AC-2, test, dan Negative diperbaiki: yang tanpa operasi hanya crash keenam yang diputuskan actor setelah proses sempat ready.
2. Urutan `nextAttemptAt`: `show_pause` berjalan di `wait_to_respawn` setelah commit `recovering`. Stage `backoff` kini diset setelah `show_pause` di fungsi yang sama, jadi stage selalu datang bersama `nextAttemptAt` yang terisi (AC-2).
3. Penomoran: `restart.attempts` sudah menghitung crash yang memicu jeda (`RestartHistory::record_crash`). Teks spec ini memakai "{attempts} of {budget}". Isi I7 spec 0022 punya selisih yang sama dan diusulkan ke lead (*Usulan update untuk spec lain*), tanpa diedit di sini.
4. `lastExitCode`: klaim reviewer bahwa detail ini selalu kosong hanya benar di dalam `apply_policy`. Jalur operasi sudah mengisinya lewat `with_last_exit`, jadi tidak ada tugas build baru; AC-1 menyebut jalur itu. Encoding sinyal (nomor sinyal) dinyatakan di AC-3.
5. Saran port di exit dini: jalur itu hanya membaca instance sendiri, jadi spec kini menetapkan satu bacaan `Registry` dan perilaku saat bacaan gagal. AC-4 membatasi `conflict` dan `unavailable` ke tahap `endpoints`.
6 dan 8. Operasi terbaru yang tidak terkait: diganti aturan *Operasi terkait* (limit 10, hanya start, restart, stop, dan ganti port, dicocokkan dengan `lastFailure.at` atau kode). F4 dari `lastFailure.code` saja memberi Change Port… tanpa isian.
7. Balapan summary dan event operasi: aturan naik dari isi `summary` ke isi lengkap, pemicu baca ulang dari `OperationCenter.recent`, dan state loading, gagal, serta offline ditetapkan; *Pemetaan sebab* menjamin judul tidak berubah saat naik.
9. Stop dan cancel selama jeda: tidak memberi banner, `lastFailure` dan fakta kegagalan tetap sampai spawn berikutnya; ada test barunya.
10. Use Port: sheet dipasang halaman Instances untuk detail dan inspector, saran hanya untuk endpoint `Fixed`, sheet berlaku untuk `stopped` dan `failed` (spec 0029 AC-6), dan saran basi ditangani verdict sheet.
11. CLI: `Last operation` saat bacaan gagal atau tanpa operasi, format waktu, sekali per jeda, dan usulan update JSON spec 0008.
12. `endpoint set` adalah aksi eksplisit, dengan saran `--dry-run`.
13. Test untuk baris log dengan operation ID, Retry, navigasi, dan pemeriksaan `instance.lock` serta folder run.
14. Kalimat ganda isi dan hint dihapus dari isi banner.
15. Tabel *Pemetaan sebab* untuk dua bentuk kode, instance lain yang sudah dihapus, dan F2 yang hanya datang dari init.
16. Fakta kegagalan ditimpa setiap commit kegagalan; `crashed { will_retry: true }` memang menulis `last_failure` registry (`apply_lifecycle`).
17. Rujukan baris spec 0012 diganti rujukan isi.

Usulan reviewer untuk kembali ke Option 2 Keputusan 1 (field baru di `InstanceLastFailure`) ditimbang lagi dan tetap ditolak: field itu tidak mencakup kegagalan sebelum `journal` (instance tidak berubah), jadi gabungan dengan operasi tetap dibutuhkan untuk kasus port yang paling umum, dan aturan *Operasi terkait* menutup celah yang ditemukan.
