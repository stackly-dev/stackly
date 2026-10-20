# 0007. Rationale: operation engine

[Kembali ke spec](index.md)

## Context

Setiap mutasi di Stackly (install runtime, create, start, stop, backup, restore, purge, env up) berjalan lama, menyentuh disk dan proses, dan bisa terputus kapan saja: client ditutup, Mac tidur atau mati, daemon di restart, disk penuh. PRD menuntut tujuh state operasi yang sama untuk semua pekerjaan async (bagian 18.2), client yang putus tidak membatalkan pekerjaan, dan cancel pada langkah yang tidak bisa dibalik tidak pernah dilaporkan sebagai rollback yang berhasil (REQ-054). Tanpa mesin bersama, setiap fitur akan menulis sendiri caranya mencatat progress, membatalkan, dan membersihkan, dengan aturan yang berbeda beda.

Risiko terbesarnya adalah efek ganda dan hapus yang salah sasaran. GUI dan CLI bisa mengirim perintah yang sama bersamaan (AC-048), dan pengguna wajar mengulang perintah setelah koneksi putus tanpa tahu apakah perintah pertama sudah jalan. Di sisi lain, pembersihan setelah gagal atau cancel adalah satu satunya tempat Stackly menghapus data secara otomatis; BR-023 dan VAL-007 melarang hapus apa pun yang bukan buatan operasi itu, termasuk lewat symlink atau race.

Spec yang sudah diterima membatasi ruang desain. Spec 0004 sudah membangun state machine operasi dan `OperationKind` di domain, serta `check_owned_path`. Spec 0005 memiliki tabel dasar `operations`, CAS revision per resource, generation per registry, dan retensi 30 hari atau 100.000 baris, dan menyerahkan journal, step, dan unique index idempotency ke spec ini lewat migrasi sendiri. Spec 0006 memiliki envelope, kategori error, `idempotencyKey` di request, event lewat `publish_resource` dengan revision, `resource.busy`, dan slot start. REQ-048 melarang SQLite dipakai sebagai mutex selama download atau startup, dan NFR-006 meminta acknowledgement di bawah 500 ms. NFR-004 meminta semua antrian berbatas dan terlihat.

Spec 0010 (supervisor) ditulis paralel dan akan memakai API engine untuk memulai operasi, menandai langkah, dan mengambil lock. Fitur 18, 20, 49, dan 51 mewarisi aturan resume. Kalau keputusan ini tertunda, fitur fitur itu tidak bisa dibangun tanpa menebak kontraknya.

## Options considered

### Keputusan 1: idempotency key untuk mutasi

#### Option 1: Wajib untuk method yang memulai operasi

Method yang membalas `OperationAccepted` menolak request tanpa key. CLI dan GUI membuat satu key per aksi pengguna.

**Pros**:

- Mengulang setelah `ipc.connection_closed` selalu aman, karena spec 0006 sudah menyatakan retry hanya aman dengan key yang sama.
- Satu jalur kode di engine, tanpa cabang "tanpa deduplikasi".

**Cons**:

- Script yang memakai client mentah wajib membuat key sendiri.

#### Option 2: Opsional

**Pros**:

- Client paling sederhana.

**Cons**:

- Retry tanpa key bisa menjalankan pekerjaan dua kali; lock dan CAS hanya mencegah duplikat yang punya kunci alami, bukan efek seperti backup ganda.

### Keputusan 2: umur ikatan key

#### Option 1: Selama baris operasi ada

**Pros**:

- Tidak ada kolom atau aturan waktu baru; retensi spec 0005 sudah membatasi pertumbuhan.
- Retry yang terlambat berjam jam tetap mendapat operasi yang sama.

**Cons**:

- Key yang dipakai ulang secara tidak sengaja dalam 30 hari mendapat operasi lama, bukan yang baru.

#### Option 2: 24 jam setelah selesai

**Pros**:

- Pola yang dikenal dari API publik.

**Cons**:

- Kolom dan aturan kedaluwarsa tambahan untuk alat lokal yang tidak butuh.

#### Option 3: Hanya selama aktif

**Pros**:

- Key bisa dipakai ulang segera.

**Cons**:

- Retry setelah sukses mengulang efek samping, persis yang ingin dicegah.

### Keputusan 3: key sama dengan isi berbeda

#### Option 1: Tolak dengan `operation.idempotency_key_reused`

**Pros**:

- Bug client langsung terlihat dengan `operationId` operasi pertama.

**Cons**:

- Butuh sidik jari request yang stabil dan aman dari secret.

#### Option 2: Kembalikan operasi pertama

**Pros**:

- Tidak perlu sidik jari.

**Cons**:

- Client yang keliru diam diam mendapat hasil operasi lain.

### Keputusan 4: operasi pada resource yang terkunci

#### Option 1: Antri FIFO, dengan pilihan tolak per jenis

**Pros**:

- Cocok dengan state `queued` PRD dan AC-073 (operasi diserialisasi).
- Klik stop saat start berjalan tetap dijalankan setelahnya, tanpa pengguna mengulang.

**Cons**:

- Dispatcher lebih rumit (keadilan FIFO per key, `waitingOn`).

#### Option 2: Langsung tolak `resource.busy`

**Pros**:

- Paling sederhana.

**Cons**:

- Pengguna dan script harus menangani retry sendiri untuk kasus yang sangat umum.

### Keputusan 5: batas berjalan dan antri

#### Option 1: 8 berjalan, 64 antri

**Pros**:

- Env up enam service plus satu dua operasi lain muat tanpa antri.

**Cons**:

- Beban puncak (beberapa download dan initdb) bisa terasa di Mac kecil.

#### Option 2: 4 dan 32

**Pros**:

- Lebih hemat sumber daya.

**Cons**:

- Env up enam service sebagian menunggu.

#### Option 3: 16 dan 256

**Pros**:

- Jarang menolak.

**Cons**:

- Beban puncak lebih berat, dan antrian besar menyembunyikan masalah.

### Keputusan 6: jenis operasi yang resumable

#### Option 1: Tabel per jenis

Resumable: install runtime, start, stop, restart, reload, env up dan down, backup.

**Pros**:

- Jenis yang langkahnya alami bisa diulang (download ke scratch, evaluasi ulang terhadap rekonsiliasi) tidak pernah membebani pengguna.
- Fitur pemilik mendapat default yang jelas.

**Cons**:

- Fitur 18, 26, 46, dan 49 wajib benar benar membuat langkahnya `repeatable`.

#### Option 2: Hanya install runtime

**Pros**:

- Paling konservatif.

**Cons**:

- Start atau stop yang terputus selalu butuh tindakan pengguna.

#### Option 3: Tidak ada di fitur 7

**Pros**:

- Keputusan ditunda ke fitur pemilik.

**Cons**:

- Fitur 18, 20, 49, dan 51 kehilangan pegangan yang memang diminta dari spec ini.

### Keputusan 7: operasi tidak resumable yang terputus sebelum titik tak bisa dibalik

#### Option 1: Kompensasi otomatis lalu `failed`

**Pros**:

- Pengguna tidak pernah meminta setengah create; yang dihapus hanya milik operasi itu karena konstruksi.
- Resource tidak terkunci lama.

**Cons**:

- Jejak setengah jadi hilang sebelum sempat diperiksa; journal tetap menyimpan langkah dan catatannya.

#### Option 2: Tetap `needs_recovery`

**Pros**:

- Paling transparan.

**Cons**:

- Resource terkunci sampai pengguna bertindak, walau tidak ada yang perlu dipertimbangkan.

### Keputusan 8: aksi pemulihan di IPC

#### Option 1: `resume` dan `abandon`

**Pros**:

- Setiap `needs_recovery` punya jalan keluar di fitur 7, jadi lock tidak tertahan selamanya.

**Cons**:

- Abandon setelah titik tak bisa dibalik meninggalkan sisa yang harus dijelaskan dengan baik.

#### Option 2: `resume` saja

**Pros**:

- Permukaan lebih kecil.

**Cons**:

- Operasi tidak resumable tertahan sampai fitur 30 atau 34.

#### Option 3: Belum ada

**Pros**:

- Paling kecil.

**Cons**:

- Fitur 8 tidak bisa memberi aksi apa pun untuk operasi yang macet.

### Keputusan 9: cancel setelah titik tak bisa dibalik

#### Option 1: Lanjut sampai aman, lapor apa adanya

**Pros**:

- Sesuai REQ-054 ("continuing safe shutdown"); state akhir selalu benar tentang dunia nyata.

**Cons**:

- Pengguna yang menekan cancel bisa melihat `succeeded`; DTO harus menampilkan cancel terlambat dengan jelas.

#### Option 2: Berhenti di titik aman, `cancelled`

**Pros**:

- Cancel terasa dihormati.

**Cons**:

- `cancelled` dengan efek tidak bisa dibalik yang sudah terjadi adalah laporan palsu.

### Keputusan 10: shutdown dengan operasi berjalan

#### Option 1: Grace 10 detik per jenis

**Pros**:

- Operasi resumable lanjut bersih, operasi pendek sebelum titik tak bisa dibalik dibersihkan, sisanya diberi waktu.

**Cons**:

- Shutdown bisa memakan sampai 10 detik.

#### Option 2: Langsung keluar

**Pros**:

- Shutdown cepat.

**Cons**:

- Lebih banyak operasi berakhir `needs_recovery` setelah restart terencana.

### Keputusan 11: primitive hapus rekursif

#### Option 1: `remove_tree` berbasis file descriptor di port

**Pros**:

- Tahan race tukar folder menjadi symlink di komponen mana pun, karena tidak ada path yang di resolve ulang setelah dicek.
- Satu primitive untuk kompensasi dan purge fitur 54.

**Cons**:

- Kode `unsafe` baru di adapter macOS yang harus direview.

#### Option 2: `std::fs::remove_dir_all` setelah `check_owned_path`

**Pros**:

- Hampir tanpa kode; turunan sudah aman dari symlink di std.

**Cons**:

- Folder induk path masih bisa ditukar di antara cek dan hapus.

#### Option 3: Walker di core

**Pros**:

- Portabel dan mudah diuji dengan fake.

**Cons**:

- Jendela race antara `symlink_metadata` dan hapus di setiap level.

### Keputusan 12: lingkup kompensasi

#### Option 1: Milik karena konstruksi

**Pros**:

- Path dihitung ulang dari ID yang hanya mungkin dibuat operasi itu, jadi crash di antara buat dan catat tidak pernah membuat hapus salah sasaran, dan registry yang dirusak tidak bisa mengarahkan hapus.

**Cons**:

- Fitur yang menyentuh path lain (misalnya folder runtime yang dialamatkan lewat digest) harus memakai pola staging lalu rename atomik.

#### Option 2: Semua path tercatat

**Pros**:

- Fleksibel untuk fitur.

**Cons**:

- Path yang sudah ada sebelumnya bisa tercatat sebagai buatan operasi setelah crash, lalu terhapus.

### Keputusan 13: frekuensi commit progress

#### Option 1: Paling banyak sekali per detik

**Pros**:

- Sesuai asumsi spec 0005; bentrok generation jarang; cukup halus.

**Cons**:

- Progress bar melompat per detik.

#### Option 2: Empat kali per detik

**Pros**:

- Lebih halus.

**Cons**:

- Empat kali lebih banyak `fullfsync` dan bentrok generation.

#### Option 3: Hanya saat stage berubah

**Pros**:

- Paling hemat.

**Cons**:

- Download besar terlihat diam.

### Keputusan 14: bentuk event

#### Option 1: Dua jenis berisi snapshot lengkap

**Pros**:

- Client cukup menimpa snapshot; nama `operation.progress` sesuai PRD 18.3.

**Cons**:

- Payload event lebih besar dari event tipis.

#### Option 2: Satu `operation.updated`

**Pros**:

- Lebih sedikit jenis.

**Cons**:

- Tidak memakai nama PRD; filter client kurang tajam.

#### Option 3: Event tipis

**Pros**:

- Payload kecil.

**Cons**:

- Setiap event memicu request `operation.get`.

### Keputusan 15: retensi

#### Option 1: Ikut spec 0005 dengan cascade

**Pros**:

- Satu kebijakan retensi untuk seluruh registry; tidak ada pruner baru.

**Cons**:

- Journal operasi sukses ikut disimpan sampai 30 hari.

#### Option 2: Journal dipangkas setelah 7 hari

**Pros**:

- Registry lebih kecil.

**Cons**:

- Inspect operasi lama kehilangan langkah; pruner tambahan.

### Keputusan 16: tempat data milik engine

#### Option 1: Tabel 1:1 `operation_journal`

**Pros**:

- Tabel dan aggregate spec 0005 dan 0004 nyaris tidak berubah.

**Cons**:

- Satu join tambahan saat membaca operasi lengkap.

#### Option 2: Kolom baru di `operations`

**Pros**:

- Satu baris per operasi.

**Cons**:

- Mengubah tabel dan tipe milik spec lain.

### Keputusan 17: cara mencatat yang dibuat

#### Option 1: Niat dulu, lalu konfirmasi

**Pros**:

- Tidak ada folder yatim yang tidak dikenal kompensasi setelah crash.

**Cons**:

- Satu commit tambahan per item.

#### Option 2: Catat setelah berhasil

**Pros**:

- Lebih sedikit commit.

**Cons**:

- Crash di antara buat dan catat meninggalkan sisa yang tidak bisa dibersihkan otomatis.

### Keputusan 18: penyimpanan lock key

#### Option 1: Tabel `operation_locks`

**Pros**:

- Bisa di CHECK dan di index; membangun ulang lock saat start sederhana.

**Cons**:

- Satu tabel lagi.

#### Option 2: Array JSON di journal

**Pros**:

- Lebih sedikit tabel.

**Cons**:

- Tanpa CHECK per key dan tanpa index.

### Keputusan 19: catatan langkah

#### Option 1: Langkah plus checkpoint kecil

**Pros**:

- Resume tahu persis langkah mana yang selesai dan mana yang tidak boleh diulang; inspect punya riwayat.

**Cons**:

- Satu commit per batas langkah.

#### Option 2: Hanya nama langkah terakhir

**Pros**:

- Ringkas.

**Cons**:

- Tidak ada tempat checkpoint resume dan tidak ada riwayat.

### Keputusan 20: cancel pada operasi berstate akhir

#### Option 1: Balasan biasa dengan disposisi

**Pros**:

- Cancel idempoten; GUI dan CLI yang membatalkan bersamaan sama sama mendapat jawaban yang jujur.

**Cons**:

- Client harus membaca disposisi, bukan hanya sukses atau gagal.

#### Option 2: Error conflict

**Pros**:

- Tegas.

**Cons**:

- Cancel ganda berakhir error di salah satu client.

### Keputusan 21: otorisasi aksi operasi

#### Option 1: Semua client user yang sama

**Pros**:

- Batas kepercayaan sudah ditegakkan spec 0006; operasi latar tetap bisa dibatalkan.

**Cons**:

- Proses lain milik user yang sama bisa membatalkan operasi (sesuai model ancaman PRD 18.1).

#### Option 2: Hanya jenis client pemulai

**Pros**:

- Lebih ketat.

**Cons**:

- Operasi GUI tidak bisa dibatalkan saat GUI tertutup.

### Keputusan 22: `expectedRevision` untuk resume dan abandon

#### Option 1: Abandon wajib, resume opsional

**Pros**:

- Aksi yang menghapus dan melepas lock selalu berdasar snapshot yang dilihat pengguna.

**Cons**:

- Script CLI harus inspect dulu sebelum abandon.

#### Option 2: Keduanya wajib

**Pros**:

- Paling ketat.

**Cons**:

- Resume yang aman diulang jadi merepotkan.

#### Option 3: Keduanya opsional

**Pros**:

- Paling longgar.

**Cons**:

- Abandon dari snapshot basi.

### Keputusan 23: tempat metrik engine

#### Option 1: `operation.get_stats` plus subsystem `operations`

**Pros**:

- DTO spec 0006 tidak disentuh.

**Cons**:

- Satu method lagi.

#### Option 2: Field baru di `DaemonCounters`

**Pros**:

- Satu tempat.

**Cons**:

- Mengubah tipe milik spec 0006.

### Keputusan 24: key yang dipegang operasi `needs_recovery`

#### Option 1: Tolak `operation.recovery_required`

**Pros**:

- Tidak ada yang menunggu tanpa batas di belakang operasi yang tidak bergerak sendiri.

**Cons**:

- Pengguna harus memulihkan dulu sebelum mengubah resource itu.

#### Option 2: Antri di belakangnya

**Pros**:

- Konsisten dengan lock biasa.

**Cons**:

- Antrian menumpuk tanpa kepastian.

### Keputusan 25: `expectedRevision` basi setelah antri

#### Option 1: Gagal `resource.revision_mismatch`

**Pros**:

- Niat yang dibuat atas snapshot lama tidak dijalankan diam diam (VAL-015).

**Cons**:

- Lebih banyak kegagalan yang harus diulang pengguna.

#### Option 2: Jalan terus

**Pros**:

- Lebih sedikit kegagalan.

**Cons**:

- Stop yang diklik saat start berjalan dieksekusi atas keadaan berbeda.

### Keputusan 26: operasi antri yang belum mulai saat daemon mati

#### Option 1: Tetap dijalankan

**Pros**:

- Client sudah menerima `operationId`; PRD menyatakan client putus tidak membatalkan.

**Cons**:

- Operasi bisa berjalan lama setelah diminta; pemeriksaan `expectedRevision` menjadi pengaman.

#### Option 2: Gagal kalau lebih tua dari satu jam

**Pros**:

- Mencegah kejutan setelah jeda panjang.

**Cons**:

- Satu aturan waktu lagi yang sulit dijelaskan.

### Keputusan 27: panic dan tulisan journal yang gagal

#### Option 1: Kompensasi lalu `failed`, dengan ulang commit akhir

**Pros**:

- Kegagalan teknis yang umum (disk penuh sesaat) tidak meninggalkan resource terkunci.

**Cons**:

- Daemon menyimpan state akhir di memori sampai commit berhasil.

#### Option 2: Langsung `needs_recovery`

**Pros**:

- Paling hati hati.

**Cons**:

- Setiap panic atau disk penuh butuh tangan pengguna.

### Keputusan 28: isi `FailureError` yang disimpan (2026-10-20)

Review fresh model 2026-10-20 (temuan Minor "`FailureError` membuang `hint`, `items`, dan `rule`") menemukan tiga bagian spec yang tidak cocok: *Model domain* memberi `FailureError` field `hint` dan `items`, *Kegagalan dan error* meminta `rule` di `ErrorBody`, tetapi tabel `operation_journal` (migrasi `0002_operation_journal.sql`) hanya punya `failure_error_code`, `failure_message`, dan `failure_details`. Kode mengikuti tabel: `FailureError` domain hanya menyimpan kode, pesan, dan details, `settle` tidak pernah membaca `OperationError::rule()`, dan `failure_body` di `stacklyd` tidak punya apa pun untuk `with_hint`, `with_items`, atau `with_rule`. Bukan bug hari ini, karena belum ada pekerjaan nyata yang gagal dengan item atau rule, tetapi spec 0011 AC-11 dan spec 0020 AC-7 butuh `secret.needs_secret` dengan satu item per reference dari operasi start dan create, dan spec 0006 *Hint* sudah menetapkan bahwa hint selalu teks statis dari `codes::<area>::hint(code, details)`.

Fakta rilis: belum ada rilis app atau daemon Stackly; satu satunya rilis GitHub adalah tiga pre release artefak runtime (`runtime/postgresql-17.11-r2-macos-aarch64`, `runtime/postgresql-18.6-r2-macos-aarch64`, `runtime/valkey-9.1.2-r2-macos-aarch64`). Tetapi migrasi 0003 sudah dibangun di atas 0002, fixture beku `registry-v2.sql` dan `registry-v3.sql` tercatat di `FIXTURE_SHA256`, dan test `frozen_fixtures_are_unchanged` memperlakukan setiap fixture yang sudah di merge sebagai rilis.

#### Option 1: Simpan `items` dan `rule` di dua kolom baru lewat migrasi 4, hint dihitung saat dipetakan

`failure_items` (JSON array, paling banyak 65 536 byte) dan `failure_rule` (`BR-NNN`) ditambah lewat `0004_operation_failure.sql`; `hint` dicabut dari `FailureError`, dan `stacklyd` memanggil fungsi hint area yang sama dengan error langsung.

**Pros**:

- Tidak ada yang hilang: kode, pesan, details, item, dan rule semuanya selamat melewati restart, dan hint bisa dihitung ulang karena ia fungsi murni dari kode dan details yang tersimpan.
- Satu sumber hint untuk error langsung dan error tersimpan, dan teks hint yang diperbaiki berlaku juga untuk operasi lama.
- Mengikuti aturan spec 0005 (migrasi baru, sidik jari baru, fixture beku baru) dan preseden migrasi 3; migrasi 0002 dan fixture v1 sampai v3 tidak disentuh.
- Kolom bertipe dengan CHECK sendiri, sehingga SQL tetap menjadi penjaga terakhir untuk bentuk item dan pola rule.

**Cons**:

- Satu migrasi dan satu fixture beku lagi, dan nomornya harus diatur dengan migrasi spec 0018 (K11: 0004 di sini, 0005 di sana).
- Ringkasan operasi `failed` bisa membesar sampai sekitar 64 KiB, yang ikut terkirim di setiap `get`, event, dan halaman `list`.

#### Option 2: Satu kolom JSON `failure_extra` berisi hint, item, dan rule

Saran alternatif review: satu objek JSON untuk semua tambahan, lewat migrasi yang sama.

**Pros**:

- Field tambahan berikutnya tidak butuh migrasi.

**Cons**:

- CHECK SQL hanya bisa memeriksa bahwa isinya objek; pola rule dan bentuk item pindah seluruhnya ke Rust.
- Menyimpan hint menduplikasi teks yang sudah ditentukan katalog, dan teks itu basi saat katalog diperbaiki.
- Tetap butuh migrasi dan fixture baru, jadi keunggulannya hanya untuk field yang belum ada.

#### Option 3: Edit migrasi 0002 di tempat

Karena app belum pernah dirilis, kolom ditambah langsung ke `CREATE TABLE operation_journal`.

**Pros**:

- Tidak ada migrasi baru, dan nomor 4 tetap bebas untuk spec 0018.

**Cons**:

- Melanggar aturan spec 0005 dan test fixture beku: `registry-v2.sql`, `registry-v3.sql`, dan kedua sha256 nya harus ditulis ulang.
- Registry yang sudah dibuka build lama di Mac ini mencatat checksum 0002 yang lama dan masuk recovery mode `migration_history_mismatch`.
- Merusak asumsi sesi paralel yang sudah memesan v4 dan v5 (spec 0018, spec 0020).

#### Option 4: Rule diturunkan dari katalog, bukan disimpan

`ErrorSpec` mendapat field rule, dan `stacklyd` mengisinya dari kode.

**Pros**:

- Tidak ada kolom `failure_rule`.

**Cons**:

- Mengubah katalog, jadi digest kontrak berubah dan minor protokol naik, padahal `ErrorBody` sudah punya `with_rule`.
- Satu kode bisa ditegakkan lebih dari satu aturan; error domain dan core sudah membawa `rule()` per error (`Option<Rule>`), dan informasi itu hilang.

#### Option 5: Cabut hint, item, dan rule dari spec

`FailureError` tetap kode, pesan, dan details.

**Pros**:

- Tidak ada pekerjaan dan tidak ada migrasi.

**Cons**:

- `secret.needs_secret` dari operasi kehilangan daftar reference nya, jadi spec 0011 AC-11 dan spec 0020 AC-7 tidak bisa dipenuhi lewat operasi.
- Error dari jalur sinkron dan jalur operasi berbeda isi untuk kode yang sama.

## Rationale

Pilihan di setiap keputusan berangkat dari dua bahaya di Context: efek ganda dan hapus yang salah sasaran. Idempotency key wajib dengan sidik jari, lock di memori dengan antrian FIFO, CAS, dan satu commit akhir yang atomik saling menutup celah: key mencegah pengulangan dari client, lock mencegah dua operasi menyentuh resource yang sama, CAS menangkap keadaan yang berubah di antara baca dan tulis, dan commit akhir membuat hasil terlihat sekaligus atau tidak sama sekali. Lock tidak pernah ditulis sebagai mutex di SQLite, dan registry tidak punya transaksi yang dibuka pemanggil (spec 0005), jadi REQ-048 terpenuhi karena konstruksi, bukan karena disiplin.

Untuk hapus, keputusan terpentingnya adalah "milik karena konstruksi": kompensasi tidak pernah mempercayai path yang tersimpan, melainkan menghitung ulang path dari ID yang dibuat operasi itu sendiri, dan menghapusnya lewat primitive yang tidak mengikuti symlink di komponen mana pun. Pola niat dulu memastikan crash di antara buat dan catat tidak meninggalkan sisa yang tidak dikenal, dan karena ID itu baru, catatan niat tidak pernah menunjuk sesuatu milik orang lain. Kompensasi otomatis berhenti di titik tak bisa dibalik, karena setelah itu dunia nyata sudah setengah berubah dan pembersihan buta bisa menghapus satu satunya salinan (misalnya data lama di staging restore). Di sana keputusan diserahkan ke pengguna lewat `needs_recovery`, dengan resume dan abandon sebagai jalan keluar yang selalu ada.

Sisanya memilih yang paling sederhana yang tetap jujur ke pengguna. Retensi ikut spec 0005, event membawa snapshot lengkap supaya client tidak perlu request tambahan, progress satu kali per detik sesuai asumsi spec 0005, batas 8 dan 64 cukup untuk enam service sekaligus, dan cancel yang terlambat dilaporkan apa adanya. API engine dibuat eksplisit dan sempit (begin, langkah bertanda, catatan dibuat, finalize, kompensator, factory, lock internal) karena spec 0010 dan fitur 18, 20, 49, 51 dibangun di atasnya secara paralel.

Keputusan 28 (2026-10-20) memilih Option 1, sama dengan jawaban K11 lead foundation. Yang menentukan adalah perbedaan antara data dan turunan: item dan rule adalah fakta tentang satu kegagalan yang tidak bisa dibangun ulang setelah restart, sedangkan hint adalah fungsi murni dari kode dan details (spec 0006 *Hint*), jadi menyimpannya hanya menambah salinan yang bisa basi. Kolom baru lewat migrasi 4, bukan edit 0002, karena aturan fixture beku sudah berlaku untuk v2 dan v3 walau app belum dirilis, dan karena registry di Mac pengembang yang sudah membuka 0002 akan masuk recovery mode kalau checksum nya berubah. Batas 65 536 byte untuk `failure_items` dipilih supaya satu ringkasan `failed` tetap jauh di bawah frame kontrol 1 MiB di `get` dan event, yang tidak punya anggaran halaman seperti `list`, sementara 100 item dengan beberapa field pendek, kasus nyata `secret.needs_secret` dan issue manifest, muat dengan longgar; yang terpotong tetap terhitung di `omitted`, mekanisme yang sudah dipakai `ErrorBody::with_items`. `OperationError::rule()` menjadi `Option<Rule>` seperti `rule()` di error domain dan core, karena satu satunya override hari ini adalah test daemon, dan tipe domain membuat CHECK `BR-NNN` dan baca ulang dari registry pasti cocok. Fungsi peta bersama untuk `failure_body` dan `rejected()` menutup risiko kode yang sama membawa hint atau item di satu jalur (sinkron) tetapi tidak di jalur lain (tersimpan), dan urutan details, item, rule membuat `omitted` dari engine dan dari `ItemSpec.max` menyatu tanpa logika khusus.

### Keputusan kecil

Diambil saat menulis spec, masing masing dengan pilihan kedua:

- **Sinyal cancel**: `tokio::sync::watch` per operasi, tanpa crate baru. Kedua: `tokio_util::sync::CancellationToken`, lebih rapi tapi menambah dependency.
- **Bentuk pekerjaan**: trait object dengan `BoxFuture` buatan sendiri, karena async fn di trait belum dyn compatible. Kedua: crate `async-trait`, menambah dependency dan makro.
- **Commit registry terakhir**: efek filesystem dulu di path milik operasi, registry di satu batch akhir lewat `finalize` yang bisa diulang saat `GenerationConflict`. Kedua: commit bertahap per langkah, yang menuntut kompensator untuk setiap baris registry.
- **Satu penulis per operasi**: mutex async per operasi untuk semua tulisan baris operasinya, sehingga cancel dan progress tidak bentrok CAS. Kedua: actor per operasi lewat channel, lebih berat tanpa manfaat tambahan.
- **Gerbang begin**: satu mutex async untuk semua `begin`, supaya cek key, hitungan antrian, dan insert atomik. Kedua: peta key yang sedang dibuat dengan lock halus, lebih cepat tapi lebih rawan bug di jalur yang jarang.
- **Sidik jari**: SHA-256 atas nama method dan params kanonis dengan key diurutkan, `SecretText` diganti penanda lewat flag thread local. Kedua: serializer serde pembungkus yang mencegat nama newtype, ratusan baris boilerplate untuk hasil yang sama.
- **Representasi kegagalan**: `failure_code` domain tetap alasan engine (pola `ReasonCode`, cocok dengan CHECK `code` spec 0005), dan kode katalog lengkap disimpan di `operation_journal.failure_error_code`, sehingga `stacklyd` bisa membangun `ErrorBody` utuh setelah restart. Kedua: mengubah pola `FailureCode` agar memuat titik, yang memaksa rebuild tabel spec 0005.
- **Kegagalan setelah titik tak bisa dibalik**: memakai event `compensation_failed` yang sudah ada (diperluas artinya menjadi "gagal atau tidak mungkin"), sehingga state machine spec 0004 tidak berubah. Kedua: event domain baru.
- **Titik tak bisa dibalik**: saat langkah pertama `reversible: false` dimulai, di commit sebelum langkah itu berjalan. Kedua: ditandai setelah langkah selesai, yang salah untuk crash di tengah langkah itu.
- **Lock internal**: `try_lock_internal` tanpa menunggu untuk bagian kritis supervisor. Kedua: supervisor membuat operasi untuk setiap transisi lifecycle, yang memenuhi journal dengan operasi kecil.
- **Cursor list**: teks opaque dengan waktu 9 digit dari registry, karena `Timestamp` wire hanya 3 digit dan bisa melompati baris. Kedua: cursor berupa ID operasi, yang gagal kalau operasi itu sudah di prune.
- **Urutan di slot start**: `recover` setelah registry dibuka dan sebelum rekonsiliasi supervisor; dispatcher dan kompensasi otomatis baru setelah `ready`, supaya pekerjaan yang dilanjutkan dan pembersihan melihat keadaan proses yang sudah direkonsiliasi. Kedua: kompensasi di slot start, yang bisa menghapus folder milik proses yatim yang belum diadopsi.
- **Pembersih scratch**: hanya folder ber UUID milik operasi berstate akhir yang `succeeded` atau belum melewati titik tak bisa dibalik; yang tidak dikenal dibiarkan dan dilaporkan. Kedua: hapus semua folder scratch tanpa operasi aktif, yang bisa menghapus sisa yang sengaja disimpan setelah abandon.
- **Aturan hint**: empat nilai (`auto_compensate`, `resume`, `abandon`, `manual`) dihitung dari titik tak bisa dibalik, sifat jenis, factory, dan langkah yang terputus. Kedua: satu hint generik, yang tidak memberi tahu pengguna apa yang aman dilakukan.
- **Letak `ScriptedWork`**: `#[doc(hidden)] pub mod testing` di `stackly-core`, karena `stackly-testkit` tidak boleh bergantung pada core (spec 0001) dan test `stacklyd` juga butuh. Kedua: fitur Cargo `test-support`, menambah konfigurasi.
- **Abandon tidak memakai slot berjalan**: pemulihan yang diminta pengguna tidak boleh tertahan antrian penuh. Kedua: abandon antri seperti operasi biasa.

## Cross check

Spec ini dibaca ulang oleh model lain (Sonnet 5.5) dalam pass read only pada 2026-10-18. Ia menemukan 16 celah keputusan dan 12 masalah soundness. Kamu menyetujui semua perbaikan yang direkomendasikan, dan semuanya sudah diterapkan di [index.md](index.md):

- **Operasi antri setelah restart**: `input` wajib untuk semua jenis, setiap jenis mendaftarkan `WorkFactory`, `expected_revision` disimpan di journal, dan operasi antri yang tidak bisa dibangun ulang ditutup `failed` dengan alasan `interrupted`.
- **Resource untuk jenis create**: `creates_resource` dan `claim_resource`; ID baru dibuat handler dengan short key yang belum dipakai.
- **Sumber `interactive`**: `ClientInfo.interactive` di handshake (Assumed spec 0006, juga dibutuhkan spec 0011).
- **Urutan error `begin`** dan **reservasi antrian di bawah mutex scheduler**, tanpa peta key di memori yang bisa basi setelah prune.
- **Revision operasi** lewat `Operation::touch` sekali per batch, dan **`Outcome::Failed` membawa failure** supaya `resolve(failed)` lolos CHECK spec 0005.
- **Baca list batch** dan batas byte halaman 900 KiB.
- **Rincian kompensasi**: alasan di `not_created`, catatan `intended` saat sukses sebagai bug, pemetaan sisa abandon ke `PartialOutcome`, kompensasi lanjut ke semua catatan dengan commit per catatan, folder scratch disimpan kalau ada yang gagal dan dihapus tepat setelah sukses.
- **Crash di tengah kompensasi**: `compensation_started_at` membuat operasi itu tidak pernah dilanjutkan.
- **Linearisasi** cancel, titik tak bisa dibalik, dan finalize di bawah mutex penulis per operasi; cancel pada operasi lanjutan yang sudah lewat titik itu melanjutkan pekerjaan.
- **Penunggu key `needs_recovery`** ditutup `failed` dengan `operation.recovery_required`.
- **Bagian kritis di task engine** dan flag klaim kompensasi untuk keamanan pembatalan request dan abandon.
- **Klaim milik karena konstruksi** dipersempit untuk secret (reference turunan ID milik operasi, probe dulu) dan item kustom (kompensator membuktikan kepemilikan, dengan `compensator_conformance`).
- **Parkir dan shutdown**: `begin_step` selalu menghormati parkir dan cancel; operasi `cancel_requested` menyelesaikan kompensasi dalam grace; sinyal kedua tidak menunggu engine.
- **Migrasi**: pengosongan key ganda, backfill journal, upsert `ON CONFLICT DO UPDATE`, dan prune per 500 operasi.
- **Nama testkit** (`FakeFilesystem`, `MacosFilesystem`) dan semantik `FakeFilesystem::remove_tree` yang portabel; resep test `CrossesDevice` lewat `hdiutil`.
- Detail kecil: throttle progress leading dan trailing, batch `waiting_on` per pass, penghalang tertua, `resource.busy` tanpa `operationId` untuk pemegang internal, alasan baru di `not_resumable` dan `not_abandonable`, `RecoveryInfo.cause` dan `error`, ulang commit `start`, definisi tipe yang belum ada, dan API `ScriptedWork`.

## Perubahan 2026-10-18 (rekonsiliasi lintas spec, ronde 2)

Pengecekan silang dengan spec 0008, 0010, dan 0013 dan keputusan engineer di pass rekonsiliasi:

| Selisih | Perbaikan | Alasan |
| --- | --- | --- |
| Tabel resume menandai start, stop, restart, dan reload bisa dilanjutkan, dan kompensator spawn menghentikan proses, padahal spec 0010 (AC-9, AC-21) menjanjikan proses selamat dari restart daemon dan diadopsi rekonsiliasi | kebijakan baru `InterruptPolicy::CloseInterrupted` untuk keempat jenis itu: `recover` menutupnya `failed interrupted` tanpa kompensasi sebelum rekonsiliasi, dan shutdown membiarkannya tanpa sinyal | rekonsiliasi supervisor adalah satu satunya yang tahu apakah proses masih hidup; kompensasi yang menghentikan proses melanggar janji survival |
| Fallback enum terbuka masih `unknown` | `unrecognized` (spec 0006) | `unknown` adalah state asli di spec 0004 dan 0011 |
| `FailureError` tidak bisa membawa daftar | field `items` dan `hint` | `secret.needs_secret` dari operasi start tidak boleh kehilangan daftar reference nya |
| Filter list tidak bisa memilih operasi `update_manager` | `ResourceKind` `manager` dengan id `self` | spec 0008 mengizinkan `--resource manager` |
| Method pemula operasi selalu membuat operasi | boleh menjawab tanpa operasi untuk permintaan yang tidak mengubah apa pun, setelah pencarian key | spec 0010 menjawab `alreadyInState` tanpa operasi |
| Perilaku dalam recovery mode registry tidak disebut | `recover` dan `start()` dilewati, subsystem `operations` `unavailable` | registry tidak boleh dibaca atau ditulis dalam recovery |
| Asumsi `remove_file` dan `remove_empty_dir` | `remove_tree` satu satunya primitive (spec 0004) | keputusan engineer ronde 2 |

Cross check model lain atas ronde 2 menambah, atas persetujuan engineer: untuk jenis `CloseInterrupted`, titik tak bisa dibalik hanya menahan kompensasi (kegagalan selesai `failed`, cancel tetap sampai ke pekerjaan), shutdown tidak menunggu runner nya, `recover` menutup yang `cancel_requested` sebagai `cancelled` dan mengosongkan `waiting_on` operasi antri, hook `precheck` di `BeginRequest` (`Reject`, `NoOp`, `Proceed`) dengan baris operasi `succeeded` tanpa langkah untuk `NoOp` yang menulis sesuatu, `NotReady` sampai `start()`, dan filter `manager`.

## Perubahan 2026-10-20 (keputusan 28)

Menutup baris keputusan di Follow-up dari review 2026-10-20. Bagian [index.md](index.md) yang berubah: AC-16, AC-17, AC-18; *Peta modul* (storage); *Model domain* (`FailureError` tanpa `hint`, dengan `rule`, dan baris `FailureItem` baru dengan `kind` bertipe `ReasonCode`); *Model data* (kolom `failure_items` dan `failure_rule`, dua CHECK, paragraf migrasi `0004_operation_failure.sql`); *Port registry*; *API engine* (`OperationError::items()` dan `rule()` bertipe `Option<Rule>`); *Kegagalan dan error* (penyimpanan, pemangkasan, dan fungsi peta bersama di `stacklyd`); *Batas*; asumsi spec 0011; *Value sourcing*; *Critical test scenarios*; *Build plan* milestone 5 langkah 16; *Consequences*; dan *Follow-up* (baris keputusan dicentang, baris baru untuk spec 0005, 0006, 0011, dan sisa antrian update spec ini). Tidak ada perubahan di `crates/protocol`: `ErrorBody` sudah punya `with_hint`, `with_items`, dan `with_rule`.
