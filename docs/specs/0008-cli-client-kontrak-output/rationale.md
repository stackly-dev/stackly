# 0008. Rationale: CLI client dan kontrak output

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: Done when fitur 8 menagih `stackly operation list`, `inspect`, dan `cancel` yang bekerja lewat daemon, padahal method, DTO, dan engine operasi milik spec 0007 yang ditulis paralel dan belum dibangun. Fitur 8 karena itu tidak bisa ditutup sebelum engine spec 0007 ada di `stacklyd`. Spec ini menuliskan pemakaian DTO itu sebagai baris Assumed, membangun mesin `--wait` dan perintah operasi terhadap daemon skrip lebih dulu, lalu membuktikannya terhadap daemon sungguhan saat spec 0007 sudah dibangun. Hal yang sama berlaku untuk AC-028 PRD: buktinya yang penuh (operasi setara dari GUI dan CLI) butuh create instance di fitur 20, jadi fitur 8 hanya membuktikan bagian output dan exit code.

Stackly punya dua client untuk satu daemon: app SwiftUI dan CLI `stackly`. CLI harus setara dengan GUI untuk administrasi baseline, dengan help, output JSON, exit code stabil, mode noninteractive, progress ke stderr, dan input secret yang aman, tanpa pernah menjadi supervisor kedua (REQ-028, BR-011). Dua jenis pengguna memakainya sekaligus: orang di Terminal yang butuh tabel, warna, waktu lokal, dan progress yang terlihat, dan script atau CI yang butuh kontrak yang tidak berubah saat tampilan diperbaiki. PRD bagian 12 mengusulkan delapan exit code untuk delapan jenis kegagalan, dan meminta `--json` tanpa ANSI atau progress yang bercampur serta perintah async yang mengembalikan operation ID dengan `--wait` untuk menunggu hasil akhir (AC-028).

Banyak hal sudah dikunci spec lain dan tidak dibahas ulang. Spec 0006 menetapkan satu envelope error dengan sepuluh kategori tertutup dan kode `area.alasan`, menulis bahwa exit code CLI hanya diturunkan dari kategori, menyerahkan nilai `not_found` dan `internal` ke fitur ini, membangun `stackly daemon status` dengan output dan exit code sementara, dan menetapkan bahwa CLI tidak pernah menyalakan daemon. Spec 0011 menetapkan kontrak input secret (TTY, `--stdin`, `--from-file`, `--from-env`, tidak pernah argv) dan meminta fitur ini menyediakan mode noninteractive. Spec 0004 menetapkan `STACKLY_ROOT` yang dibaca sekali dan meminta root aktif terlihat di `daemon status`. Spec 0005 menyerahkan tampilan waktu lokal di CLI ke fitur ini. Spec 0012 menetapkan arti lima tone status dan aturan bahwa aksi destruktif tidak pernah menjadi pilihan default. AGENTS.md mewajibkan clap v4 derive, tokio, thiserror, error yang dicek sekali saat start, tanpa `unwrap()` di luar test, dan test integrasi terhadap sistem nyata dengan root sementara.

Kekuatan yang saling tarik: kontrak harus stabil cukup lama untuk script, tapi produk masih jauh dari rilis stabil dan DTO protokol akan terus bertambah; operasi async bisa berjalan lama (unduhan runtime, readiness 60 detik), tapi script di CI tidak boleh menggantung; pengguna menekan Ctrl+C dan daemon bisa restart di tengah penantian, tapi operasi tidak boleh ikut batal atau kehilangan jejak; dan teks dari daemon (path, pesan provider) ditampilkan di terminal pengguna. Kalau tidak diputuskan sekarang, fitur 11, 14, 15, 20, dan seterusnya masing masing akan memilih format output, aturan prompt, dan exit code sendiri, dan script yang ditulis untuk satu perintah tidak akan bekerja untuk perintah berikutnya.

## Options considered

### Keputusan 1: exit code untuk `not_found`

#### Option 1: 10, kode sendiri

**Pros**:

- Script bisa membedakan "resource belum ada" dari "input salah" tanpa membaca JSON, misalnya `inspect || create`.
- Aman untuk masa depan: memisah dua arti nanti adalah perubahan breaking, sedangkan kode terpisah sejak awal tidak pernah memaksa apa pun.

**Cons**:

- Di luar tabel PRD bagian 12, jadi harus didokumentasikan sendiri.

#### Option 2: 2, sama dengan invalid input

**Pros**:

- Tetap di dalam tabel PRD, sama dengan nilai sementara spec 0006.

**Cons**:

- ID yang salah ketik dan resource yang baru dihapus pihak lain tidak bisa dibedakan dari exit code.

#### Option 3: 3, sama dengan conflict

**Pros**:

- Cocok untuk resource yang hilang karena perubahan bersamaan.

**Cons**:

- Menyesatkan untuk ID yang memang tidak pernah ada.

### Keputusan 2: exit code untuk `internal`

#### Option 1: 1

**Pros**:

- Konvensi Unix untuk kegagalan umum tool, tidak bentrok dengan kategori lain.
- Panic yang ditangkap ikut 1, bukan 101 bawaan Rust yang tidak dikenali script.

**Cons**:

- Exit 1 juga dipakai banyak tool untuk kegagalan biasa, jadi pengguna bisa salah membaca sebagai "gagal wajar".

#### Option 2: 70 (`EX_SOFTWARE`)

**Pros**:

- Nilai sysexits yang spesifik untuk error internal.

**Cons**:

- Jarang dikenali, dan tidak ada nilai sysexits lain yang dipakai di tabel ini.

#### Option 3: 7, sama dengan failed

**Pros**:

- Satu kode lebih sedikit.

**Cons**:

- Bug Stackly tidak bisa dibedakan dari operasi yang gagal wajar.

### Keputusan 3: Ctrl+C saat menunggu

#### Option 1: 130

**Pros**:

- Konvensi shell (128 + SIGINT) yang dikenali semua tool.
- Tidak menyiratkan bahwa operasi dibatalkan, karena memang tidak.

**Cons**:

- Satu satunya exit yang tidak diturunkan dari kategori.

#### Option 2: 9, kategori cancelled

**Pros**:

- Konsisten dengan aturan "exit dari kategori".

**Cons**:

- Script mengira operasinya batal padahal masih berjalan.

### Keputusan 4: bagian output yang stabil

#### Option 1: `--json` dan exit code saja

**Pros**:

- Tampilan untuk manusia bisa diperbaiki terus tanpa merusak script.
- Satu kontrak yang jelas untuk diuji dengan golden file.

**Cons**:

- Pengguna yang terlanjur parse teks akan rusak saat teks berubah.

#### Option 2: termasuk teks manusia

**Pros**:

- Aman untuk script yang parse teks (seperti porcelain git).

**Cons**:

- Setiap perbaikan tampilan menjadi perubahan breaking; dua kontrak harus dijaga.

#### Option 3: belum ada yang dikunci sampai rilis stabil

**Pros**:

- Paling fleksibel selama pengembangan.

**Cons**:

- Fitur 11, 14, dan seterusnya membangun di atas kontrak yang bisa bergeser.

### Keputusan 5: bentuk `--json`

#### Option 1: envelope stabil satu baris per record

`{schemaVersion, type, command, result, error, requestId, warnings}` di stdout, termasuk saat gagal. Perintah streaming memakai JSON Lines dengan `type` `item` lalu satu `result`.

**Pros**:

- Satu tempat parse untuk sukses dan gagal, dan tidak rancu dengan field `error` milik DTO operasi yang gagal.
- Warning terbaca mesin; versi envelope terpisah dari versi protokol.
- Bentuk streaming sudah diputuskan sebelum fitur 27 membutuhkannya.

**Cons**:

- Path jq lebih panjang; satu tipe lagi yang dirawat.

#### Option 2: DTO mentah di stdout, error di stderr

**Pros**:

- Path jq pendek, mirip kubectl dan docker; sama dengan versi sementara spec 0006.

**Cons**:

- Script harus menangkap stderr terpisah untuk membaca error, padahal stderr juga berisi progress; warning tidak terbaca mesin.

#### Option 3: DTO mentah, `{"error": ...}` di stdout saat gagal

**Pros**:

- Satu tempat parse, seperti npm.

**Cons**:

- Bentuk sukses dan gagal berbeda, dan `.error` rancu dengan `OperationDto.error` pada `inspect` yang sukses untuk operasi gagal.

### Keputusan 6: tampilan progress

#### Option 1: baris hidup di TTY, baris per tahap di luar TTY

**Pros**:

- Manusia melihat progress yang rapi; log CI tetap punya jejak tahap tanpa `\r` atau ANSI.

**Cons**:

- Dua mode render yang harus diuji.

#### Option 2: hanya di TTY

**Pros**:

- Log CI sepi.

**Cons**:

- Perintah yang lama atau macet di CI tidak meninggalkan jejak tahap.

#### Option 3: JSON Lines di stderr saat `--json`

**Pros**:

- Progress terbaca mesin.

**Cons**:

- Kontrak ketiga yang harus dijaga, padahal GUI mendapat progress lewat event, bukan CLI.

### Keputusan 7: warna

#### Option 1: auto, `--color`, dan konvensi environment lewat `anstream`

**Pros**:

- `anstream` dan `anstyle` sudah ada di lock lewat clap, dan menangani TTY, `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, dan `TERM=dumb`.
- `--color always` berguna saat output dipipe ke pager.

**Cons**:

- Satu flag global lagi.

#### Option 2: auto dan `NO_COLOR` tanpa flag

**Pros**:

- Lebih sedikit flag.

**Cons**:

- Tidak bisa memaksa warna ke pager.

#### Option 3: tanpa warna

**Pros**:

- Paling sederhana.

**Cons**:

- Status penting lebih sulit dilihat sekilas.

### Keputusan 8: waktu di output manusia

#### Option 1: lokal absolut di inspect, umur di tabel

**Pros**:

- Memenuhi tampilan sesuai locale dari fitur 5; kolom tabel tetap sempit.

**Cons**:

- Dua format waktu; golden harus menormalkan keduanya.

#### Option 2: lokal absolut di semua tempat

**Pros**:

- Satu format.

**Cons**:

- Kolom tabel lebar dan lebih sulit dibaca sekilas.

#### Option 3: UTC di semua tempat

**Pros**:

- Sama dengan JSON dan log daemon.

**Cons**:

- Tidak memenuhi tampilan sesuai locale dari fitur 5.

### Keputusan 9: kapan noninteractive

#### Option 1: tanpa TTY, `--no-input`, atau `STACKLY_NO_INPUT`

**Pros**:

- Sesuai rumusan spec 0011 (stdin dan stderr TTY); script yang dijalankan dari terminal bisa mematikan prompt secara eksplisit.

**Cons**:

- Script yang lupa flag dan dijalankan dari terminal masih bisa bertemu prompt.

#### Option 2: ditambah `CI`

**Pros**:

- Runner CI otomatis aman.

**Cons**:

- Variabel `CI` yang tersetel di shell lokal mengejutkan pengguna.

#### Option 3: ditambah `--json`

**Pros**:

- Automation dari terminal aman tanpa flag tambahan.

**Cons**:

- `stackly secret set --json` di terminal tidak bisa meminta nilai lewat TTY.

### Keputusan 10: konfirmasi di CLI

#### Option 1: `--yes` per perintah, default tidak

**Pros**:

- clap menolak `--yes` di perintah yang tidak butuh konfirmasi, jadi script tidak bisa menyetujui aksi destruktif baru tanpa sengaja.
- Enter tidak pernah menjalankan aksi, sama dengan aturan Return spec 0012.

**Cons**:

- Script harus tahu perintah mana yang menerima `--yes`.

#### Option 2: `--yes` global

**Pros**:

- Mudah diingat.

**Cons**:

- Script yang selalu memasang `--yes` ikut menyetujui aksi destruktif yang ditambah kemudian.

#### Option 3: aksi destruktif wajib mengetik nama

**Pros**:

- Paling aman.

**Cons**:

- Berat untuk semua aksi; fitur 54 tetap bisa menambahkannya di atas Option 1 untuk purge.

### Keputusan 11: perilaku default perintah async

#### Option 1: kembali dengan operation ID, `--wait` untuk menunggu

**Pros**:

- Sesuai scope dan PRD bagian 12; perilaku sama di TTY dan script.

**Cons**:

- Pengguna di Terminal sering perlu mengetik `--wait`.

#### Option 2: menunggu default, `--no-wait` untuk kembali cepat

**Pros**:

- Ramah untuk manusia.

**Cons**:

- Berbeda dari scope; script yang lupa `--no-wait` bisa tertahan lama.

### Keputusan 12: batas default `--wait`

#### Option 1: tanpa batas, `--timeout` opsional

**Pros**:

- Operasi sudah punya batas sendiri di daemon (readiness, langkah), dan daemon yang mati memutus koneksi.
- Instalasi lewat jaringan lambat tidak gagal palsu.

**Cons**:

- Operasi yang macet karena bug bisa membuat CLI menunggu lama tanpa `--timeout`.

#### Option 2: 10 menit default

**Pros**:

- CI tidak pernah menggantung tanpa akhir.

**Cons**:

- Unduhan runtime yang lambat gagal palsu.

#### Option 3: default per perintah

**Pros**:

- Paling tepat per jenis operasi.

**Cons**:

- Setiap fitur harus memilih dan merawat angka sendiri.

### Keputusan 13: koneksi putus saat menunggu

#### Option 1: sambung ulang sampai 30 detik

**Pros**:

- Restart daemon (fitur 30) tidak membuat script gagal; operasi tersimpan di journal sehingga penantian bisa dilanjutkan.

**Cons**:

- Ada jeda sampai 30 detik sebelum CLI menyerah kalau daemon memang mati.

#### Option 2: langsung exit 6

**Pros**:

- Paling sederhana dan jujur.

**Cons**:

- Setiap script harus menangani restart daemon sendiri.

#### Option 3: sambung ulang tanpa batas

**Pros**:

- Tidak pernah menyerah selama `--timeout` belum lewat.

**Cons**:

- Tanpa `--timeout`, CLI bisa menunggu selamanya untuk daemon yang tidak kembali.

### Keputusan 14: jawaban `daemon.starting`

#### Option 1: ulang otomatis sampai 10 detik

**Pros**:

- Kode itu dijamin tanpa efek; script yang dijalankan tepat setelah login tidak gagal palsu.

**Cons**:

- Perintah bisa tertahan sampai 10 detik sebelum gagal.

#### Option 2: tidak pernah mengulang

**Pros**:

- Paling sederhana.

**Cons**:

- Setiap script harus mengulang sendiri.

#### Option 3: ulang semua kode retryable

**Pros**:

- Menutup lebih banyak kasus sementara.

**Cons**:

- Daemon yang memang tidak jalan (`ipc.endpoint_missing`) tertutupi beberapa detik.

### Keputusan 15: exit `operation cancel --wait`

#### Option 1: `cancelled` 0, akhir lain 3

**Pros**:

- Diukur dari tujuan perintah; cancel yang terlambat (langkah irreversible sudah selesai) dilaporkan jujur sebagai conflict (REQ-054).

**Cons**:

- Pemetaan berbeda dengan wait biasa, jadi `Waiter` butuh mode.

#### Option 2: sama dengan wait biasa

**Pros**:

- Satu pemetaan.

**Cons**:

- Cancel yang berhasil exit 9 dan cancel yang terlambat exit 0, kebalikan dari harapan script.

#### Option 3: cancel tanpa `--wait`

**Pros**:

- Lebih sedikit aturan.

**Cons**:

- Script butuh dua perintah.

### Keputusan 16: idempotency key

#### Option 1: dibuat CLI, bisa diisi `--idempotency-key`

**Pros**:

- Script yang mengulang perintah setelah crash tidak membuat operasi ganda (NFR-005).

**Cons**:

- Satu flag lagi di setiap perintah async.

#### Option 2: selalu dibuat CLI

**Pros**:

- Tanpa flag.

**Cons**:

- Mengulang perintah yang sama dari script menghasilkan operasi kedua.

### Keputusan 17: renderer tabel

#### Option 1: renderer sendiri dengan `unicode-width`

**Pros**:

- Sekitar 80 baris kode; `unicode-width` kecil dan dipakai luas; tidak ada fitur yang tidak dibutuhkan.

**Cons**:

- Kode sendiri yang harus dirawat.

#### Option 2: `comfy-table`

**Pros**:

- Matang, dengan wrapping dan border.

**Cons**:

- Menambah dependency tree (crossterm) untuk fitur yang tidak dipakai.

#### Option 3: `tabled`

**Pros**:

- Tabel lewat derive.

**Cons**:

- API besar dan makro yang menambah waktu compile.

### Keputusan 18: karakter kontrol dari daemon

#### Option 1: selalu di escape di output manusia

**Pros**:

- Menutup terminal injection dan teks terbalik lewat bidi, termasuk saat output disimpan lalu di cat.

**Cons**:

- Nilai yang memang berisi tab atau newline tampil sebagai escape.

#### Option 2: hanya saat TTY

**Pros**:

- Lebih setia ke data di pipe.

**Cons**:

- File yang nanti di cat ke terminal tetap membawa escape.

#### Option 3: tidak di escape

**Pros**:

- Paling sederhana.

**Cons**:

- Path dan pesan provider tidak divalidasi daemon, jadi celah tetap terbuka.

### Keputusan 19: pemilik modul `secret_input`

#### Option 1: spec 0011 langkah 8

**Pros**:

- Sesuai build plan spec 0011; tidak ada dua pemilik untuk satu modul.

**Cons**:

- Fitur 8 hanya menyediakan tempat menempel, jadi input secret baru terbukti di fitur 11.

#### Option 2: fitur 8

**Pros**:

- Pembaca input tersedia lebih awal.

**Cons**:

- Butuh perubahan build plan spec 0011 yang sudah diterima.

### Keputusan 20: penjaga BR-011

#### Option 1: pindaian spawn proses di `check-deps`

**Pros**:

- CLI yang menjalankan proses lewat std tertangkap sebelum merge, bukan hanya lewat review.

**Cons**:

- Fitur 15 perlu allowlist kalau `stackly daemon start` memanggil `launchctl`.

#### Option 2: larangan dependency saja

**Pros**:

- Lebih sedikit kode xtask.

**Cons**:

- `std::process::Command` tidak butuh dependency apa pun, jadi lolos.

### Keputusan 21: cara menguji

#### Option 1: `stacklyd` sungguhan, daemon skrip, dan pty

**Pros**:

- Jalur utama terbukti terhadap daemon nyata; semua kategori error, putus koneksi, dan starting bisa dipicu dengan deterministik; perilaku TTY terbukti di pty nyata (AC-028).

**Cons**:

- Daemon skrip bisa menyimpang dari daemon sungguhan; test pty bergantung pada `/usr/bin/script`.

#### Option 2: hanya `stacklyd` sungguhan

**Pros**:

- Paling dekat dengan produksi.

**Cons**:

- Banyak kategori error dan putus koneksi sulit dipicu, jadi matriks exit code tidak lengkap.

#### Option 3: unit test lapisan output saja

**Pros**:

- Cepat.

**Cons**:

- Tidak membuktikan stdout bersih di proses nyata.

### Keputusan 22: menunggu operasi yang sudah dimulai

#### Option 1: `operation inspect <id> --wait`

**Pros**:

- Tetap di nama perintah PRD bagian 12; sekaligus permukaan untuk menguji `--wait` sebelum ada perintah create atau start.

**Cons**:

- `inspect` dengan dan tanpa `--wait` punya exit berbeda untuk operasi yang gagal.

#### Option 2: verb `operation wait <id>`

**Pros**:

- Lebih jelas di script.

**Cons**:

- Verb di luar tabel PRD bagian 12.

#### Option 3: tidak ada

**Pros**:

- Lebih sedikit permukaan.

**Cons**:

- Script harus polling sendiri.

### Keputusan 23: bentuk operation ID di argumen

#### Option 1: UUID lengkap saja

**Pros**:

- Tanpa ambiguitas; awalan UUID v7 berisi waktu, jadi singkatan dari depan hampir selalu rancu.

**Cons**:

- Panjang untuk diketik; pengguna menyalin dari tabel.

#### Option 2: UUID lengkap atau 8 karakter akhir

**Pros**:

- Nyaman diketik, seperti short key instance di spec 0005.

**Cons**:

- Spec 0007 harus menyediakan pencarian akhiran dan menolak yang rancu.

### Keputusan 24: default `operation list`

#### Option 1: 50 terbaru dengan `--limit`, `--active`, `--resource`, `--before`

**Pros**:

- Operasi yang baru gagal langsung terlihat; batas mengikuti query registry (1 sampai 500).

**Cons**:

- Riwayat panjang butuh beberapa panggilan dengan `--before`.

#### Option 2: hanya yang aktif

**Pros**:

- Lebih sepi.

**Cons**:

- Operasi yang baru gagal tidak terlihat tanpa flag.

#### Option 3: semua halaman

**Pros**:

- Lengkap.

**Cons**:

- Bisa ribuan baris (retensi 100.000).

### Keputusan 25: root aktif di `daemon status`

#### Option 1: baris root dan warning

**Pros**:

- `STACKLY_ROOT` yang tertinggal di shell langsung ketahuan (konsekuensi spec 0004).

**Cons**:

- Warning muncul di setiap status selama development dengan root test.

#### Option 2: baris root saja

**Pros**:

- Output lebih sepi.

**Cons**:

- Mudah terlewat.

## Rationale

Kekuatan utama di Context adalah dua jenis pengguna yang berbagi satu biner. Pilihan di setiap keputusan memisahkan keduanya dengan tegas: script mendapat kontrak sempit yang bisa diuji mesin (satu baris envelope di stdout, exit code dari kategori, tidak ada prompt tanpa izin), sedangkan manusia mendapat tampilan yang boleh terus diperbaiki (tabel, warna, waktu lokal, progress) di stream yang tidak pernah dibaca script. Itu sebabnya hanya `--json` dan exit code yang stabil, error ikut masuk stdout dalam bentuk envelope, dan progress tidak pernah menyentuh stdout. Envelope dipilih di atas DTO mentah karena `inspect` pada operasi yang gagal adalah sukses yang membawa `error` di dalam DTO nya; tanpa pemisah `result` dan `error`, script tidak bisa membedakan keduanya tanpa exit code.

Untuk operasi async, kekuatan yang menentukan adalah bahwa operasi milik daemon, bukan milik koneksi (PRD bagian 18.2, spec 0007). Maka tidak ada jalur di CLI yang membatalkan operasi kecuali `operation cancel`: batas waktu, Ctrl+C, dan koneksi putus hanya menghentikan penantian dan selalu meninggalkan operation ID. Batas default tidak dipasang karena batas yang benar ada di daemon (readiness, langkah), dan batas klien yang salah menghasilkan kegagalan palsu yang lebih berbahaya daripada penantian panjang; script di CI tetap bisa memberi `--timeout`. Sambung ulang 30 detik menutup restart daemon terencana tanpa menutupi daemon yang benar benar mati.

Exit code mengikuti spec 0006 apa adanya, dengan dua nilai yang diserahkan ke fitur ini dipilih supaya kontraknya bisa bertahan: `not_found` mendapat kode sendiri karena memisah arti nanti adalah perubahan breaking yang tidak pernah bisa ditarik, dan `internal` mendapat 1 karena itulah arti konvensional "ada yang rusak di tool ini". Ctrl+C mengikuti konvensi shell 130 karena kategori `cancelled` akan berbohong tentang operasi yang masih berjalan. Untuk alat, setiap pilihan memakai ulang yang sudah ada di proyek (clap, `anstream` yang sudah ikut clap, jiff, tokio, `stackly-ipc`, pola golden spec 0009) dan hanya menambah `unicode-width`, karena setiap dependency CLI adalah kode yang dijalankan di mesin pengguna pada setiap perintah.

### Keputusan kecil yang diambil saat menulis spec

- **Kode `cli.*` di katalog `stackly-protocol`**, bukan di `apps/cli`: satu katalog untuk semua kode yang bisa dilihat pengguna, test katalog spec 0006 berlaku seragam, dan `error-codes.json` lengkap untuk dokumentasi. Runner up: daftar sendiri di `apps/cli`, yang menghindari kenaikan minor protokol tapi membuat katalog terbelah.
- **Field `type` dan `requestId` di envelope**: `type` membuat record stream dan record akhir bisa dibedakan tanpa menebak dari key; `requestId` menghubungkan error di script dengan baris log daemon (NFR-011). Runner up: tanpa keduanya, dengan korelasi hanya lewat `-v`.
- **Warning di mode `--json` hanya di envelope**: stderr tetap untuk progress, jadi automation tidak perlu memfilter teks. Runner up: menulis keduanya.
- **JSON compact satu baris**: cocok dengan JSON Lines dan `jq`; manusia yang ingin rapi bisa pipe ke `jq`. Runner up: pretty print saat stdout TTY, yang membuat output tergantung terminal.
- **`command` sebagai path dipisah spasi** (`operation inspect`): sama dengan yang diketik pengguna. Runner up: titik (`operation.inspect`), yang mudah tertukar dengan nama method daemon.
- **State ditulis dengan nilai wire**: sama dengan JSON dan flag filter, mudah di grep. Runner up: label GUI spec 0012 (`Needs Recovery`), yang berbeda dari yang dipakai di script.
- **Tone terminal dari arti lima tone spec 0012**, termasuk `succeeded` netral: konsisten dengan GUI. Runner up: hijau untuk sukses seperti kebanyakan CLI.
- **`Waiter` memakai event sebagai pemicu dan `operation.get` sebagai kebenaran, ditambah tick 5 detik**: tidak bergantung pada isi payload event spec 0007, tetap benar kalau ada event yang tidak dipublikasikan, mengikuti resep snapshot plus cursor yang diwajibkan spec 0006 untuk client fitur 8, dan menguji event operasi spec 0007 sejak awal. Runner up yang diangkat cross check: polling `operation.get` setiap 1 detik tanpa subscription, yang lebih sederhana (tanpa urutan subscribe, gap, dan sambung ulang stream) dengan latensi yang masih wajar untuk CLI; tidak dipilih karena bertentangan dengan resep wajib spec 0006 dan kehilangan nilai uji terhadap event.
- **`AsyncArgs` dan `Context::submit` dibangun fitur 14**: fitur ini tidak punya perintah async untuk mengujinya, dan kode tanpa test melanggar aturan proyek. Runner up: membangunnya sekarang dengan perintah test tersembunyi di biner produksi.
- **`Client::call_detailed` dengan `CallOptions` di `stackly-ipc`, dan `requestId` dibuat CLI**: ID harus sudah diketahui sebelum request dikirim (baris `-v` pertama) dan tetap ada walau request tidak pernah dibalas (`cli.request_timeout`); tambahan API tanpa perubahan wire. Runner up: `stackly-ipc` mengembalikan ID setelah selesai, yang kehilangan ID tepat pada kasus timeout.
- **`--timeout` diparse format friendly jiff**: jiff sudah dipakai, jadi tidak perlu `humantime`. Runner up: `humantime`.
- **`STACKLY_NO_INPUT` menolak nilai yang tidak dikenal**: sama dengan `STACKLY_ROOT` kosong yang ditolak, supaya salah setel tidak diam diam berarti interaktif. Runner up: nilai apa pun selain kosong berarti aktif.
- **Konfirmasi yang ditolak keluar 9** (`cli.confirmation_declined`, kategori `cancelled`): pengguna membatalkan perintahnya sendiri. Runner up: exit 0 tanpa melakukan apa pun, yang membuat script tidak tahu aksinya tidak terjadi.
- **Ctrl+C di luar `--wait` tidak ditangani**: request kontrol paling lama 10 detik, dan sinyal bawaan sudah memberi 130. Runner up: penangan global, yang menambah kode untuk kasus tanpa operasi yang ditinggalkan.
- **Broken pipe diam dengan exit sesuai hasil**: `stackly operation list | head` adalah pemakaian normal. Runner up: exit 141 seperti proses yang mati oleh SIGPIPE.
- **`cli.output_failed` sebagai warning di stderr, bukan error katalog**: mutasi yang sudah berhasil tidak boleh dilaporkan gagal hanya karena stdout tidak bisa ditulis. Runner up: error kategori `failed` (exit 7), yang membuat script mengulang mutasi yang sudah terjadi.
- **`cli.platform_unavailable` berkategori `unavailable`**: kegagalan lingkungan, bukan bug dan bukan input salah. Runner up: `internal`.
- **Konfirmasi dibaca dari stdin**: dalam mode interaktif stdin pasti TTY, jadi `/dev/tty` tidak menambah apa pun selain satu cara gagal baru (proses tanpa terminal pengendali). Runner up: `/dev/tty` seperti `rpassword`.
- **Progress menulis langsung ke stderr lewat `StderrSink`**: `anstream` membuang semua escape saat warna mati, termasuk kode hapus baris, dan satu pemilik stderr mencegah baris `-v` atau warning bercampur dengan baris hidup. Runner up: progress lewat `anstream`, yang meninggalkan sisa baris saat `NO_COLOR`.
- **Baris hidup dipotong 60 kolom tanpa membaca lebar terminal**: membaca lebar butuh `ioctl` lewat `libc` atau crate tambahan, sedangkan isi baris (state, tahap, angka, durasi) jarang lebih panjang. Runner up: crate `terminal_size`.
- **Timeout per panggilan (`Control` 10 detik, `Interactive` 200 detik)**: antrean `SecretWorker` spec 0011 menunggu sampai 180 detik untuk dialog Keychain, jadi satu batas 10 detik akan memutus panggilan yang sah dan meninggalkan hasil yang tidak diketahui. Runner up: satu batas 200 detik untuk semua, yang membuat daemon macet baru terdeteksi lama.
- **Ctrl+C lewat `signal(SignalKind::interrupt())` yang didaftarkan sebelum baris waiting**: `tokio::signal::ctrl_c()` baru mendaftar saat pertama di poll, jadi SIGINT yang datang lebih awal mematikan proses tanpa output. Runner up: `ctrlc` crate dengan handler global.
- **Footer halaman di stderr**: tabel di stdout tetap bersih saat dipipe. Runner up: di stdout setelah tabel.
- **Tabel tanpa pemotongan dan dengan UUID lengkap**: nilai selalu bisa disalin. Runner up: memotong ke lebar terminal dengan flag `--wide`.
- **Panic tanpa isi pesan, lewat hook sendiri**: hook bawaan Rust mencetak pesan sebelum `catch_unwind` sempat menangkap, dan isi pesan panic bisa memuat data; hook sendiri hanya menyimpan lokasi untuk `-v`. Runner up: mencetak pesan panic seperti bawaan Rust.
- **Agent Skills dan MCP untuk `anstream`, `anstyle`, dan `unicode-width` tidak dicari** atas pilihan pengguna pada 2026-10-18; dicatat untuk baris `Declined:` lewat `/sync`.

## Cross check

Spec ini diperiksa satu pass kritik read only oleh model lain (Sonnet 5.5) pada 2026-10-18. Engineer memilih menerapkan semua perbaikan yang direkomendasikan. Yang berubah:

- Celah keputusan yang ditutup: `--help` dan `--version` selalu teks dengan exit 0, bentuk `cli.usage` (baris pertama clap, peta `ErrorKind` dengan `other`, `command` kosong, pindaian argv sampai `--` yang juga mengatur warna clap); `requestId` dibuat CLI sebelum mengirim; `--timeout` dan Ctrl+C membungkus seluruh penantian dengan `state` `unknown` sebelum DTO pertama; satu `operation.get` berjalan dengan jeda 250 ms dan klasifikasi error di `--wait` (hanya koneksi putus yang memicu sambung ulang); `ErrorBody` dari daemon tidak pernah diubah dan hint CLI hanya di teks manusia; timeout per panggilan untuk dialog Keychain; flag persetujuan `Confirm` dari pemanggil dengan pilihan membiarkan daemon menjawab; urutan clap lalu environment, grammar `--timeout`, dan `STACKLY_NO_INPUT` tanpa peduli huruf besar; detail tampilan (sel kosong, durasi, warna ANSI, footer yang mengulang flag); `idempotencyKey` dikembalikan di hasil async dan di `cli.request_timeout`; test tambahan untuk `--yes` di perintah lain, kode sintetis, dan `codes::cli` di luar CLI.
- Perbaikan soundness: helper pty dengan stdout ke file dan exit code dari file; progress langsung ke stderr lewat `StderrSink` dengan potongan 60 kolom dan `TERM=dumb`; SIGINT didaftarkan sebelum baris waiting; fitur clap `string`; konfirmasi dibaca dari stdin; pindaian BR-011 yang menutup alias modul, glob, dependency yang menjalankan proses, dan makro `print!`; hook panic sendiri dengan envelope `cli.panic` dan `JoinError`; `Envelope<T>` generik supaya urutan field hasil tetap; kegagalan tulis stdout tidak mengubah exit.
- Saran yang tidak diambil: polling saja tanpa event (lihat keputusan kecil `Waiter`), dan mengeluarkan kode `cli.*` dari digest kontrak, yang aturannya milik spec 0006 sehingga dicatat sebagai Follow-up untuk pemiliknya.
