# 0027. Log per instance: rationale

[Kembali ke spec](index.md)

## Context

Daemon sudah menangkap stdout dan stderr proses utama ke spool di runtime root, menyamarkan setiap baris, lalu menyerahkannya ke `LogSink` (spec 0010). Satu satunya sink saat ini adalah `TailSink`: 64 baris per stream di memori untuk cuplikan kegagalan. Setelah itu baris hilang. Tidak ada method log di protokol, tidak ada `stackly instance logs`, dan tidak ada tab Logs; `LogView` sudah ada sebagai komponen (spec 0067) dan menunggu data.

Gaya yang membentuk keputusan ini: REQ-017 dan AC-017 (follow, cursor, filter, rotasi, client lambat tidak menahan proses dan tidak menumbuhkan memori), NFR-004 (setiap buffer dibatasi, kehilangan terlihat), PRD 35.2 (5 file × 20 MiB per stream per instance), spec 0006 (satu socket, frame kontrol 1 MiB, batch log 64 KiB, event bus dengan ring bersama 4096 event), spec 0008 (perintah streaming menulis record `item` lalu `result`), dan spec 0011 (secret tidak pernah sampai ke penyimpanan tahan lama).

Batasan dari kode yang ada: spool adalah buffer antara proses dan daemon, dibatasi 256 MiB per stream dan dibuang dengan penanda `truncated`; sink harus menjawab `Busy` alih alih memblokir; setelah daemon crash, spool bisa mengirim ulang sampai sekitar 1 MiB (satu satuan punch plus satu chunk baca) dengan penanda `resumed`; dan potongan 64 KiB untuk baris tanpa newline disamarkan sendiri sendiri. `${instance.logs}` di manifest provider menunjuk folder `<logs>/instances/<id>/` yang sama dengan tempat store ini akan tinggal.

Tanpa keputusan ini, `/develop` harus menebak format file, arti cursor, cara follow tanpa menahan siapa pun, dan perilaku saat disk penuh, sementara GUI dan CLI bisa berakhir dengan dua jalur baca yang berbeda.

## Options considered

### Keputusan 1: jalur follow

#### Option 1: subscription yang membaca dari disk, frame `log` baru (dipilih)

Writer menerbitkan `head` per instance lewat `watch`; task follow membaca dari store setelah cursor nya dan mendorong frame `log`.

**Pros**: client lambat hanya tertinggal di disk, memori per follower satu frame; tidak ada baris yang hilang kecuali dirotasi, dan itu ditandai; satu jalur baca untuk halaman dan follow.
**Cons**: varian frame server baru dan aturan subscription bersama di spec 0006; setiap bangun membaca disk (murah karena cache halaman OS).

#### Option 2: long poll `logs_read { after, waitMs }`

**Pros**: tanpa frame baru. **Cons**: request yang selalu terbuka per client, loop di setiap client, dan jeda tambahan setiap putaran.

#### Option 3: event `instance.log_appended` di event bus

**Pros**: paling sedikit kabel baru. **Cons**: instance yang cerewet mengusir event state dari ring bersama 4096, sehingga client lain mendapat gap `cursor_expired` untuk hal yang tidak berhubungan.

#### Option 4: polling pendek dari client

**Pros**: paling sederhana. **Cons**: request konstan per layar terbuka, dan jeda sampai satu interval.

### Keputusan 2: format segmen

#### Option 1: JSON Lines (dipilih)

**Pros**: serde_json sudah ada; newline selalu di escape, jadi batas record bisa dicari dari offset sembarang (pencarian biner di segmen); bisa dibaca `jq`; mudah ditambah kunci opsional. **Cons**: sekitar 60 byte overhead per baris.

#### Option 2: teks dengan prefix

**Pros**: paling kecil, bisa dibaca `less`. **Cons**: penanda dan flag butuh aturan escape sendiri, dan parser buatan sendiri.

#### Option 3: biner berawalan panjang

**Pros**: cepat dan padat. **Cons**: tidak bisa dibaca tanpa Stackly, pemulihan torn write lebih sulit.

### Keputusan 3: file per stream

#### Option 1: file terpisah per stream, satu `seq` per instance (dipilih)

**Pros**: sesuai PRD 35.2 (batas per stream); stdout yang spam tidak mengusir error di stderr; `seq` bersama membuat All cukup merge dua urutan. **Cons**: penanda run ditulis dua kali, dan pembaca harus merge.

#### Option 2: satu file gabungan

**Pros**: urutan trivial. **Cons**: satu stream bisa mengusir stream lain, dan batas tidak lagi per stream.

### Keputusan 4: duplikat setelah crash

#### Option 1: buang tepat lewat offset spool (dipilih)

**Pros**: cursor dan script tidak pernah melihat baris ganda; kuncinya (pid, waktu start, offset) sudah diketahui pembaca spool. **Cons**: mengubah tipe internal spec 0010 dan menambah `src` di setiap record.

#### Option 2: simpan semua, tambah penanda `resumed`

**Pros**: tanpa perubahan spec 0010. **Cons**: sampai sekitar 1 MiB baris ganda per crash, dan pengguna harus menebak mana yang ganda.

### Keputusan 5: level baris

#### Option 1: aturan kata kunci umum (dipilih)

**Pros**: langsung benar untuk PostgreSQL (`ERROR:`, `FATAL:`) dan NATS (`[ERR]`, `[WRN]`) tanpa perubahan manifest; salah tebak hanya mengubah ikon. **Cons**: heuristik, bisa salah untuk provider dengan format unik.

#### Option 2: pola dari manifest provider

**Pros**: paling tepat. **Cons**: perubahan schema spec 0009 dan pekerjaan per provider untuk manfaat yang kecil.

#### Option 3: tanpa level

**Pros**: tanpa heuristik. **Cons**: simbol tone `LogView` tidak terpakai, error tidak menonjol.

#### Option 4: stderr berarti warning

**Pros**: sederhana. **Cons**: salah untuk PostgreSQL yang menulis semua ke stderr.

### Keputusan 6: filter

#### Option 1: substring tanpa membedakan huruf, di daemon (dipilih)

**Pros**: Load Earlier dan `--grep` memberi halaman yang berisi hasil; tanpa biaya regex di daemon. **Cons**: tidak sekuat regex; butuh batas pindai.

#### Option 2: substring plus level · Option 3: regex · Option 4: hanya di client

Plus level menunggu kebutuhan nyata (Follow-up). Regex butuh batas waktu dan ukuran pola di daemon. Filter di client membuat Load Earlier tidak pernah menemukan hasil lama.

### Keputusan 7: ukuran rotasi

#### Option 1: konstanta sekarang (dipilih)

**Pros**: tanpa permukaan setting di journey ini; angka tampil di footer. **Cons**: pengguna dengan disk kecil belum bisa menurunkannya.

#### Option 2: env var daemon · Option 3: setting per instance

Env var menambah konfigurasi startup untuk manfaat test saja (test sudah memakai `DurableLogConfig`). Setting per instance menarik registry dan UI yang belum dibutuhkan.

### Keputusan 8: file log milik provider

#### Option 1: ditunda (dipilih)

**Pros**: tidak ada provider saat ini yang menulis file sendiri; journey tetap kecil. **Cons**: Follow-up spec 0010 untuk fitur 27 baru selesai sebagian.

#### Option 2: tail baca saja · Option 3: salin penuh dengan rotasi

Keduanya membangun jalur yang belum ada penggunanya; salin penuh juga menggandakan disk.

### Keputusan 9: lokasi store

#### Option 1: subfolder `captured/` di folder log instance, nama itu dicadangkan di spec 0009 (dipilih)

**Pros**: tetap di root log (bukan runtime root yang bisa dibersihkan macOS); tidak bisa bertabrakan dengan file provider; ikut terhapus atau tersimpan bersama folder log instance. **Cons**: satu aturan validasi manifest baru.

#### Option 2: file langsung di folder log instance dengan nama yang dicadangkan

**Pros**: tanpa subfolder. **Cons**: daftar nama cadangan lebih rumit dan bercampur dengan file provider.

#### Option 3: `${instance.logs}` dipindah ke subfolder `provider/`

**Pros**: pemisahan paling bersih. **Cons**: mengubah arti variabel interpolasi yang sudah dipakai paket dan golden test.

### Keputusan 10: gagal tulis

#### Option 1: dorong balik ke spool (dipilih)

**Pros**: gangguan singkat tidak menghilangkan baris; batas tetap dari spool yang sudah ada; kehilangan tetap ditandai `truncated`. **Cons**: spool tumbuh di runtime root selama gangguan.

#### Option 2: buang langsung dengan penanda

**Pros**: spool tetap kecil. **Cons**: kehilangan baris walau gangguan hanya sesaat.

### Keputusan 11: redaksi di batas potong

#### Option 1: tutup sekarang dengan overlap (dipilih)

**Pros**: begitu ada store tahan lama, celah ini akan menulis potongan secret ke disk; perbaikannya kecil dan bisa diuji. **Cons**: perubahan di pembaca spool yang sudah teruji.

#### Option 2: biarkan sebagai residual risk

**Pros**: tanpa perubahan spec 0010. **Cons**: melanggar spec 0011 untuk baris di atas 64 KiB tanpa newline.

### Keputusan 12 sampai 15: perilaku CLI dan GUI

- Ctrl+C saat follow keluar 0 dengan `result` (dipilih) dibanding 130: mengakhiri follow adalah cara normal, dan script mendapat cursor untuk melanjutkan. Kekurangannya, pengecualian dari aturan spec 0008.
- Batas subscription dipakai bersama (dipilih) dibanding batas sendiri 4: tidak ada batas dan kode baru; kekurangannya, delapan follow bisa menghabiskan slot event satu koneksi.
- `--tail` default 100 (dipilih) dibanding 20 atau semua: cukup untuk awal error PostgreSQL tanpa membanjiri terminal.
- GUI menyimpan 10 000 baris dengan waktu `HH:mm:ss.SSS` lokal dan pemisah hari (dipilih) dibanding 2 000 atau 50 000 dan tanggal di setiap baris: cukup untuk startup yang ramai, ringan untuk daftar lazy, dan sesuai template `LogView` serta mockup.
- Follow tetap hidup melewati stop dan start, dan penanda start dan exit ditulis (dipilih): sama dengan `tail -F`, dan run yang berbeda terlihat di dalam log.

## Rationale

Inti masalah fitur ini adalah memisahkan tiga kecepatan: proses yang menulis, disk yang menyimpan, dan client yang membaca. Spec 0010 sudah memisahkan yang pertama lewat spool berbatas. Pilihan di sini memisahkan dua lainnya dengan cara yang sama: writer menjawab `Busy` alih alih menunggu, dan follower membaca dari disk dengan cursor alih alih menerima dorongan dari memori. Hasilnya, satu satunya buffer yang tumbuh saat ada masalah adalah spool yang sudah punya batas dan penanda, sesuai AC-017 dan NFR-004. Event bus sengaja tidak dipakai karena ringnya dipakai bersama state semua resource.

JSON Lines dengan `seq` per instance dipilih karena membuat setiap kebutuhan lain menjadi sederhana: cursor cukup satu angka plus `storeId`, halaman cukup pencarian biner di segmen yang namanya sudah memuat `seq` pertama, merge All cukup membandingkan `seq`, dan pemulihan torn write cukup memangkas sampai newline terakhir. Segmen per stream menjaga batas PRD 35.2 apa adanya dan melindungi stderr, tempat error biasanya ada.

Buang duplikat yang tepat dan overlap redaksi sama sama menyentuh spec 0010, tetapi keduanya hanya penting begitu ada penyimpanan tahan lama: tanpa itu, cursor akan bisa melihat baris ganda dan disk bisa menyimpan potongan secret. Mengubahnya sekarang lebih murah daripada membersihkan store nanti. Keputusan lain (konstanta rotasi, provider files ditunda, filter substring, level kata kunci) menjaga journey ini kecil tanpa menutup jalan: setiap perluasan punya tempat yang jelas di format dan protokol (kunci opsional, enum terbuka, `level` yang sudah tersimpan).
