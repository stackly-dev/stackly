# 0040. Provider Valkey: rationale

Catatan keputusan untuk [index.md](index.md). `/develop` tidak perlu membaca file ini; isinya menjelaskan kenapa spec berbunyi seperti itu.

## Context

Valkey adalah salah satu dari enam provider wajib (PRD 5.2). Sampai fitur ini, ia hanya ada sebagai artefak runtime (`valkey-9.1.2-r2-macos-aarch64` di `runtimes/inventory.toml`, dibangun maintainer) dan sebagai entri `[[planned]]` di catalog. Satu satunya tempat yang tahu cara menjalankannya adalah recipe spike tulisan tangan di xtask, yang berjalan tanpa auth dan tanpa persistence.

Yang dituntut scope fitur 40 (REQ-038, AC-038, AC-077 bagian Valkey, PRD 22.4):

- config, bind dan auth, `PING`, direktori persistent, kebijakan RDB dan AOF, log, dan stop;
- penjelasan jujur soal mode volatile lawan durable: dengan persistence aktif data pulih sesudah shutdown normal, dan mode volatile menyatakan bahwa data bisa hilang dan tidak menjanjikan restore;
- durability dibedakan per kebijakan dan per kejadian (stop normal, crash proses, kehilangan daya), bukan satu janji umum;
- Valkey tidak pernah diganti Redis atau container diam diam.

Gaya yang menekan desain:

- **Aturan secret spec 0011.** Nilai secret hanya di Keychain, tidak pernah di argv, dan penulis paket memilih mekanisme pengiriman tertinggi yang didukung tool (stdin, hash, file sekali pakai, env, lalu config plaintext). Probe `exec`, stop `command`, dan reload `run` tidak menerima secret sama sekali.
- **Kontrak proses spec 0010 dan harness spec 0036.** Proses utama foreground, memegang lock di fd 3, tidak me re exec; harness mencari proses yang tertinggal lewat path root di argumen.
- **Drift spec 0029.** File di folder config yang berubah di luar Stackly memblokir render berikutnya, jadi provider tidak boleh menulis ke sana.
- **Template tanpa ekspresi (spec 0009).** `#if` hanya untuk boolean dan array; tidak ada percabangan atas nilai enum.
- **RISK-07.** Setiap perubahan core untuk satu provider dihitung; provider seharusnya hanya data.
- **Wave paralel.** Empat spec provider lain ditulis bersamaan, dan modul format, probe, dan hash di core adalah file bersama.

Tidak ada data teregulasi. Workspace: repo tunggal, keputusan khusus paket `providers/valkey` ditambah dua tambahan generik di `stackly-core`.

## Fakta runtime

Diuji 2026-11-12 pada artefak `valkey-9.1.2-r2-macos-aarch64` dari cache runtime xtask (sha256 cocok dengan inventory), di port loopback sementara, di folder scratch di luar repo. Ini uji lokal architect, bukan evidence; evidence dihasilkan test native saat build.

| No | Yang diuji | Hasil |
| --- | --- | --- |
| F1 | `valkey-server --version` | `Valkey server v=9.1.2 sha=00000000:1 malloc=libc bits=64 build=991715b0e5b85611` |
| F2 | Isi `bin/` artefak | `valkey-server`, `valkey-cli`, `valkey-check-aof`, `valkey-check-rdb`, `valkey-benchmark` (inventory hanya mendaftar dua yang pertama di `executables`) |
| F3 | `valkey-cli -e` | exit 1 untuk balasan error (`NOAUTH`, `NOPERM`, `LOADING`) dan untuk gagal konek; tanpa `-e` exit 0 walau balasannya error; pesan error ada di stderr |
| F4 | `VALKEYCLI_AUTH` | dibaca `valkey-cli` (nama lama `REDISCLI_AUTH` juga masih dikenal); `--user` butuh password |
| F5 | User `stackly-readiness on nopass -@all +ping` | `PING` dijawab `PONG`; `GET`, `INFO`, `CONFIG GET` ditolak `NOPERM` |
| F6 | `user default on #<sha256 hex> ~* &* +@all` di file include | tanpa login `NOAUTH`; `AUTH <password>` dan `AUTH default <password>` diterima; `CONFIG GET requirepass` kosong; `ACL LIST` menampilkan hash |
| F7 | `--include <file>` di argv, tanpa argumen file config | dimuat; opsi argv sesudahnya menang; `CONFIG REWRITE` ditolak `ERR The server is running without a config file`; `ACL SAVE` ditolak |
| F8 | `CONFIG SET dir`, `CONFIG SET dbfilename` | ditolak `can't set protected config` (`enable-protected-configs no`, bawaan) |
| F9 | `CONFIG SET port` saat jalan | berhasil; listener pindah ke port baru |
| F10 | Mode `rdb` (`save 3600 1 300 100 60 10000`) | stop `SIGTERM` menulis `dump.rdb` (`Saving the final RDB snapshot before exiting`, `DB saved on disk`); data kembali sesudah start; sesudah `SIGKILL` tulisan sejak snapshot terakhir hilang |
| F11 | Mode `aof` (`appendonly yes`, `appendfsync everysec`, `save ""`) | folder `appendonlydir/` berisi `appendonly.aof.1.base.rdb`, `appendonly.aof.1.incr.aof`, `appendonly.aof.manifest`; data kembali sesudah stop normal dan sesudah `SIGKILL` (tulisan 1,5 detik sebelum kill) |
| F12 | Mode tanpa persistence (`save ""`, `appendonly no`) | folder data kosong; semua data hilang sesudah stop |
| F13 | Ganti mode lewat restart: `rdb` ke `aof` | **data hilang**: Valkey membuat AOF kosong dan tidak memuat `dump.rdb` |
| F14 | Ganti mode lewat restart: `aof` ke tanpa persistence, lalu ke `rdb` | data tidak dimuat; saat stop, mode `rdb` menulis snapshot kosong |
| F15 | Stop `SIGTERM` dengan klien di `BLPOP q 600` | keluar 0 dalam 0,03 detik |
| F16 | Start dengan 628 MiB AOF | `PING` sebagai `stackly-readiness` dijawab `LOADING Valkey is loading the dataset in memory` (exit 1) sekitar 1,5 detik, lalu `PONG` |
| F17 | `ACL DELUSER stackly-readiness` saat jalan | probe keluar 1 dengan stderr `AUTH failed: WRONGPASS invalid username-password pair or user is disabled.` |
| F18 | File incr AOF dirusak di awal | start keluar 1 dengan `Bad file format reading the append only file appendonly.aof.1.incr.aof: make a backup of your AOF file, then use ./valkey-check-aof --fix <filename.manifest>` |
| F19 | Kutip config: `proc-title-template "a\"b\\c d ü #x"` | `CONFIG GET` membaca kembali `a"b\c d ü #x` |
| F20 | `maxmemory 0mb`, `maxmemory-policy "allkeys-lru"` | diterima; `0mb` dibaca 0 |
| F21 | Judul proses | bawaan: `ps` menampilkan `valkey-server 127.0.0.1:<port>`; dengan `--set-proc-title no` argv asli tetap |
| F22 | Lock di fd 3 | tetap terbuka di proses server (`lsof -d 3`) |
| F23 | Listener | satu TCP di `127.0.0.1:<port>`; tanpa socket Unix |
| F24 | Cap waktu log | waktu lokal; dengan `TZ=UTC` menjadi UTC |
| F25 | URL | `valkey-cli -u redis://default:<pw>@host:port/0` dan `valkey://...` keduanya diterima, password ter percent encode; `-u valkey://default@host:port/0 --askpass` membaca password dari stdin |
| F26 | TablePlus di Mac ini | bundle ID `com.tinyapp.TablePlus`; mendaftarkan skema URL `redis` |
| F27 | Peringatan `tcp-backlog` | setiap start mencatat `WARNING: The TCP backlog setting of 511 cannot be enforced because kern.ipc.somaxconn is set to the lower value of 128` |
| F28 | `SAVE` oleh pengguna di mode tanpa persistence, lalu stop dan start | `dump.rdb` tertulis dan dimuat lagi: key kembali |
| F29 | `notify-keyspace-events "K$"` | diterima; `CONFIG GET` membalas `$K` |
| F30 | Jendela `LOADING` yang stabil | mode `rdb`, 2000 key, `--key-load-delay 2000 --loading-process-events-interval-bytes 1024`: 18 jawaban `LOADING` selama 6,4 detik lalu `PONG`. Di mode `aof` dengan `key-load-delay`, probe menunggu tanpa jawaban sampai muat selesai. Nilai interval di bawah 1024 ditolak Valkey |

Yang tidak diuji: kehilangan daya host, TablePlus benar benar membuka URL `redis://` tanpa password, dan perilaku di bawah launchd.

## Options considered

Setiap keputusan mencantumkan pilihan, alasan singkat, dan pilihan kedua. User mendelegasikan setiap keputusan ke opsi yang direkomendasikan (aturan koordinasi scope 07), jadi yang tertulis di sini adalah rekomendasi yang diambil.

### Keputusan 1: auth

- **Option 1 (dipilih): password selalu wajib untuk user `default`.** Sama dengan PostgreSQL (spec 0019): aman sejak awal, dan connection string selalu punya bentuk yang sama. Kekurangan: tool yang mengira Redis lokal tanpa password butuh password itu.
- **Option 2: setting `requireAuth` yang bisa dimatikan.** Nyaman untuk tool lama. Kekurangan: dua jalur di setiap template, test, dan connection string, dan instance tanpa password di loopback bisa dibaca user OS lain di Mac yang sama.
- **Option 3: tanpa auth, hanya loopback.** Paling sederhana dan sama dengan recipe spike. Kekurangan: bertentangan dengan "bind dan auth" di REQ-038 dan dengan sikap PostgreSQL.

### Keputusan 2: cara password sampai ke Valkey

- **Option 1 (dipilih): hash SHA-256 di file ACL terpisah (`user default on #<hex> ...`), lewat filter template `sha256Hex`.** Ini mekanisme 2 spec 0011, yang lebih tinggi dari config plaintext, dan Valkey mendukungnya (F6). Password tidak pernah ada di disk sebagai teks, dan escaper format tidak perlu menangani karakter aneh di password. Kekurangan: butuh sintaks filter di template (perubahan core kedua), yang juga dipakai ClickHouse, jadi urutan build dua fitur saling bergantung.
- **Option 2: `requirepass "<password>"` di file include plaintext (mekanisme 5).** Tanpa perubahan core selain format. Kalimat Consequences spec 0011 memang mengira Valkey memakai ini. Kekurangan: password teks ada di disk selama instance berjalan, padahal tool mendukung hash; itu melanggar aturan "pilih mekanisme tertinggi" di spec yang sama.
- **Option 3: file ACL (`aclfile`) yang ditulis Stackly.** Memungkinkan `ACL SAVE`. Kekurangan: Valkey lalu boleh menulis ulang file itu, format ACL butuh format template sendiri, dan `aclfile` tidak boleh dicampur dengan baris `user` di config.

### Keputusan 3: readiness

- **Option 1 (dipilih): probe `exec` dengan `valkey-cli -e ... PING` sebagai user `stackly-readiness` ber `nopass` yang hanya boleh `PING`.** Tanpa perubahan core, tanpa secret di probe, dan `LOADING` membuat ready berarti data sudah dimuat (F16). Polanya sama dengan role `stackly_readiness` PostgreSQL. Kekurangan: satu proses pendek setiap 10 detik, dan siapa pun di loopback bisa mengirim `PING`.
- **Option 2: probe native `valkey-ping` (state machine RESP di core).** Tanpa proses anak. Kekurangan: perubahan core ketiga dengan fuzz target sendiri, dan tetap butuh user tanpa password karena param probe native menolak secret; spec 0036 mensyaratkan probe native hanya kalau `tcp`, `http`, atau `exec` tidak cukup, dan di sini `exec` cukup.
- **Option 3: probe `tcp`.** Paling murah. Kekurangan: port terbuka sebelum data dimuat, jadi ready tidak jujur.
- **Option 4: `PING` tanpa login dan menerima `NOAUTH` sebagai tanda hidup.** Tanpa user tambahan. Kekurangan: Valkey memeriksa login sebelum status loading, jadi `NOAUTH` juga keluar saat data belum dimuat.

### Keputusan 4: bentuk setting persistence

- **Option 1 (dipilih): satu enum `persistence` dengan `rdb`, `aof`, `volatile`; default `rdb`; `immutableAfterInit`.** Tiga nilai menutup cerita yang jujur, nama `volatile` menjelaskan dirinya, dan `rdb` sama dengan bawaan upstream. Tidak bisa diganti karena ganti mode lewat restart menghilangkan data (F13, F14). Kekurangan: lebih kaku dari Valkey biasa, dan gabungan RDB dengan AOF tidak ditawarkan.
- **Option 2: dua boolean (`snapshots`, `appendOnlyFile`).** Muat di `#if` template tanpa trik, dan menawarkan gabungan. Kekurangan: mode volatile menjadi keadaan tersirat (keduanya mati), yang justru tidak jujur; empat kombinasi untuk diuji.
- **Option 3: enum yang bisa diganti (`apply: restart`).** Fleksibel. Kekurangan: F13 dan F14; Stackly akan menghilangkan data pengguna dengan satu klik.
- **Default `aof`** adalah pilihan kedua untuk default: paling aman terhadap crash, tetapi berbeda dari bawaan upstream dan menulis ke disk terus menerus untuk pemakaian yang kebanyakan cache.

### Keputusan 5: dari nilai enum ke config Valkey

- **Option 1 (dipilih): tiga file statis ber format `plain`, dan argv memuat `--include .../persistence-${settings.persistence}.conf`.** Tanpa perubahan core; setiap mode adalah file yang bisa dibaca dengan komentarnya sendiri. Kekurangan: ketiga file selalu dirender dan tampil di halaman Configuration.
- **Option 2: percabangan enum di template (`#if` atas nilai, atau `#switch`).** Satu file. Kekurangan: perubahan grammar template di core untuk satu provider.
- **Option 3: nilai enum langsung berupa nilai config (`yes`, `no`).** Tanpa file tambahan. Kekurangan: butuh dua field lagi dan nama nilainya tidak berarti bagi pengguna.

### Keputusan 6: parameter milik Stackly

- **Option 1 (dipilih): port, bind, `dir`, dan mode di argv; tiga file dimuat lewat `--include`; tanpa argumen file config.** Argv menang atas file (F7), `dir` terkunci (F8), dan `CONFIG REWRITE` tidak bisa menulis ke folder config (F7), jadi tidak ada drift spec 0029. Path tidak butuh escape. Kekurangan: argv panjang, dan `CONFIG SET` tetap bisa mengubah parameter sampai restart (F9).
- **Option 2: semuanya di `valkey.conf` sebagai argumen file config.** Satu file yang lengkap. Kekurangan: `CONFIG REWRITE` menulis ulang file itu, path butuh escape, dan port masuk template sehingga render init terkena `init_port_issues`.
- **Option 3: melarang `CONFIG SET` untuk user `default` (`-config|set`).** Menjaga parameter saat jalan juga. Kekurangan: mematahkan pemakaian development yang umum (misalnya `notify-keyspace-events`) dan tool yang memanggil `CONFIG`.

### Keputusan 7: peringatan mode volatile

- **Option 1 (dipilih): teks `description` field `persistence`, nama nilai `volatile`, dan komentar file mode.** Tanpa perubahan core, protokol, atau Swift; teksnya tampil di form create di tempat mode dipilih. Kekurangan: teksnya selalu tampil, bukan hanya saat `volatile` dipilih, dan halaman Configuration tidak menampilkan `description`.
- **Option 2: anotasi baru `x-stackly` berisi catatan per nilai enum, dikirim di DTO form dan dirender Swift saat nilai itu dipilih.** Peringatan yang tepat sasaran. Kekurangan: perubahan core, kenaikan minor protokol, dan kode Swift untuk satu provider, bertentangan dengan sasaran AC-029.
- **Option 3: capability atau status per instance (`durability`).** Bisa tampil di daftar instance. Kekurangan: model domain, registry, dan UI baru; terlalu besar untuk fitur ini.

### Keputusan 8: stop

- **Option 1 (dipilih): `gracefulProcess` dengan `SIGTERM`.** Valkey menulis snapshot terakhir atau men sync AOF lalu keluar 0 (F10, F15), tanpa secret. Kekurangan: kalau snapshot terakhir gagal, Valkey membatalkan shutdown dan stop habis waktu.
- **Option 2: stop `command` dengan `valkey-cli SHUTDOWN`.** Cara resmi. Kekurangan: butuh password user `default`, dan stop `command` tidak menerima secret.

### Keputusan 9: reload

- **Option 1 (dipilih): tanpa reload.** Jujur terhadap apa yang didukung. Kekurangan: setiap perubahan setting butuh restart.
- **Option 2: reload `run` dengan `valkey-cli CONFIG SET ...`.** Tanpa restart untuk `maxmemory`. Kekurangan: butuh secret di reload `run`, yang ditolak spec 0009; jalan keluarnya adalah user admin kedua tanpa password, yang membuka `CONFIG SET` bagi siapa pun di loopback.

### Keputusan 10: connection dan Open In

- **Option 1 (dipilih): `redis://` sebagai connection utama, `valkey://` kedua; Open In `valkey-cli` dan TablePlus.** `redis://` diterima hampir semua library klien; `valkey://` untuk tool Valkey (F25). TablePlus terbukti mendaftarkan skema `redis` (F26). Kekurangan: kata `redis` muncul di paket, walau hanya sebagai nama skema.
- **Option 2: hanya `valkey://`.** Bersih secara nama. Kekurangan: banyak library menolak skema itu.
- **Redis Insight** (disebut Follow-up spec 0070) tidak dimasukkan: bundle ID dan deep link nya tidak bisa dibuktikan di Mac ini, dan paket resmi tidak boleh memuat entri yang belum pernah dijalankan. Ia tercatat di Follow-up.

### Keputusan 11: penanda versi data

- **Option 1 (dipilih): tanpa `dataMarker`.** Valkey tidak menulis file penanda, dan dengan satu line registry sudah cukup untuk BR-009. Kekurangan: saat line kedua datang, kompatibilitas RDB harus diputuskan.
- **Option 2: file penanda buatan Stackly di folder data.** Menjaga sejak sekarang. Kekurangan: butuh primitive init baru untuk menulis file ke folder data.

### Keputusan 12: bukti BR-025 dan handover

- **Option 1 (dipilih): evidence kompatibilitas ditulis suite native paket; entri `HANDOVER_PROVEN` hanya sesudah test launchd lulus.** Check persistence dan crash tidak ada di harness, dan spec 0030 melarang entri tanpa record. Kekurangan: sampai kamu menjalankan test launchd, restart daemon terencana menghentikan instance Valkey.
- **Option 2: test konformitas sebagai evidence BR-025.** Satu test lebih sedikit. Kekurangan: evidence tidak mencakup hal yang paling penting untuk Valkey, yaitu durability.

### Keputusan 13: file yang ditulis pengguna sendiri di mode volatile

Ditemukan cross check: user `default` bisa menjalankan `SAVE`, dan Valkey memuat `dump.rdb` di start berikutnya walau mode nya `volatile` (F28).

- **Option 1 (dipilih): nyatakan dengan jujur, jangan dilarang.** AC-7 berlaku untuk instance tanpa perintah admin yang menulis file; README dan tabel *Edge cases* menyebut pengecualiannya dan jalan keluarnya. Kekurangan: mode `volatile` bukan jaminan teknis; pengguna bisa membuatnya menyimpan data.
- **Option 2: larang `SAVE`, `BGSAVE`, `BGREWRITEAOF`, dan `SHUTDOWN` untuk user `default` di mode `volatile`**, lewat file user per mode. Menutup jalur yang paling mudah. Kekurangan: tiga file sensitif, dan jalurnya tetap terbuka lewat `CONFIG SET save` atau `CONFIG SET appendonly`, kecuali `CONFIG SET` ikut dilarang.
- **Option 3: larang juga `CONFIG SET` di mode `volatile`.** Benar benar tertutup. Kekurangan: mematahkan library yang menjalankan `CONFIG SET notify-keyspace-events` saat start, justru di mode yang paling sering dipakai sebagai cache.

### Keputusan 14: format `valkeyConf` atau format yang sudah ada

Ditemukan cross check: nilai yang dirender draft pertama (satu integer, dua enum, satu hash mentah) juga lolos escaper `natsConf`, jadi format baru belum punya pekerjaan.

- **Option 1 (dipilih): tetap `valkeyConf`, dan beri ia pekerjaan nyata lewat setting `notifyKeyspaceEvents`.** Setting itu memang dibutuhkan pengguna, dan nilainya bisa memuat `$`, yang ditolak `natsConf` (F29). Nama format lalu cocok dengan file yang ditulisnya, dan spec 0009 memang merencanakan `valkeyConf` sebagai satu perubahan core fitur 40. Kekurangan: RISK-07 tetap mencatat satu format baru.
- **Option 2: pakai `natsConf` untuk kedua file Valkey dan tunda `valkeyConf`.** Satu perubahan core lebih sedikit hari ini. Kekurangan: file Valkey berlabel format NATS, aturan tolak `$` milik NATS ikut berlaku tanpa alasan, dan setting teks pertama yang datang tetap memaksa format baru beserta revision paket.
- **Option 3: semua setting lewat argv, tanpa `valkey.conf`.** Tanpa template biasa sama sekali. Kekurangan: `users.conf` tetap butuh format yang menerima tag, dan halaman Configuration kehilangan file yang menunjukkan nilai yang berlaku.

## Rationale

**Password selalu wajib, sebagai hash.** REQ-038 menyebut "bind dan auth", dan PostgreSQL sudah menetapkan sikap proyek: instance lokal aman sejak dibuat. Spec 0011 lalu menentukan caranya: mekanisme tertinggi yang didukung tool. Valkey menerima hash SHA-256 di aturan ACL (F6), jadi config plaintext bukan pilihan yang bisa dibela, walau kalimat lama spec 0011 mengiranya. Harga pilihan ini adalah filter `sha256Hex`, tetapi filter itu memang sudah direncanakan spec 0011 untuk ClickHouse ("ditambah fitur provider pertama yang butuh"), dan sesi `arc-f43-spec` menulis kontrak yang sama pada hari yang sama. Jadi untuk proyek ini bukan perubahan tambahan, melainkan perubahan yang dibagi dua provider.

**Probe `exec` dengan user khusus.** Probe tidak boleh memegang secret, dan itu aturan yang baik: probe berjalan setiap 10 detik selama instance hidup. Jalan keluar yang sama dengan PostgreSQL berlaku di sini: identitas terpisah tanpa password yang hampir tidak punya hak. Bedanya, di Valkey batasnya adalah daftar perintah (`+ping` saja), bukan socket privat, jadi residu nya dinyatakan: siapa pun di loopback bisa mendapat `PONG`. Probe native ditolak karena spec 0036 meminta alasan kenapa `exec` tidak cukup, dan tidak ada alasan itu: `valkey-cli -e` sudah membedakan `LOADING`, `NOAUTH`, dan sukses lewat exit code (F3, F16).

**Mode persistence yang tidak bisa diganti.** Ini keputusan yang paling terasa bagi pengguna, dan dasarnya adalah F13 dan F14: di Valkey 9.1.2, mengganti `appendonly` lewat restart membuat server mengabaikan file mode lama, dan data hilang tanpa pesan error. Cara aman mengganti mode butuh server yang sedang jalan, login admin, dan menunggu tulis ulang selesai, yaitu alur terpandu yang bukan bagian paket data. Daripada menawarkan tombol yang menghilangkan data, spec ini mengunci mode dan menulis alur ganti mode sebagai Follow-up. Ini juga yang membuat janji AC-038 dan AC-077 bisa diuji: satu instance punya satu kebijakan seumur hidupnya.

**File include per mode.** Template spec 0009 sengaja tanpa ekspresi, dan menambah percabangan enum hanya untuk Valkey adalah jenis perubahan yang RISK-07 ukur. Nama file dari nilai setting memakai aturan interpolasi yang sudah ada (dicek di `interpolate.rs`), dan hasil sampingnya baik: setiap mode punya file yang menjelaskan dirinya.

**Tanpa file config utama.** Tiga aturan proyek bertemu di sini: drift spec 0029 (Valkey tidak boleh menulis ke folder config), "parameter milik Stackly tidak bisa digeser" dari PostgreSQL, dan pemeriksaan port saat init (template tanpa port). `--include` di argv memenuhi ketiganya sekaligus (F7, F8). `CONFIG SET` dibiarkan terbuka karena melarangnya merugikan pemakaian development yang wajar, dan perubahan itu hilang saat restart.

**Peringatan lewat `description`.** AC-038 meminta UI menunjukkan bahwa data bisa hilang saat mode volatile dipilih. Form create menampilkan `description` di bawah kontrol (dicek di `SchemaFormView.swift`), jadi teks itu ada tepat di tempat pilihan dibuat. Mekanisme peringatan per nilai akan lebih tajam, tetapi harganya protokol dan Swift baru untuk satu field satu provider. Yang jujur dicatat sebagai kekurangan: halaman Configuration belum menampilkan `description`, dan itu diusulkan sebagai perbaikan generik, bukan bagian fitur ini.

**`set-proc-title no`, `TZ=UTC`, `tcp-backlog 128`.** Tiga detail kecil dari uji binary. Yang pertama menjaga kontrak pembersihan harness (F21). Yang kedua menyamakan cap waktu log dengan PostgreSQL (F24). Yang ketiga membuang satu baris `WARNING` dari setiap start, yang akan terlihat seperti masalah di tab Logs (F27).

**Secret ber kelas `restart`.** Karena hash dirender ulang dari Keychain di setiap start, mengganti password cukup `secret.set` lalu restart. Itu lebih jujur daripada `immutableAfterInit` (yang akan menampilkan konfirmasi `stored_only` padahal nilainya memang akan berlaku). Risikonya: ini pemakaian resmi pertama jalur `next_start`, jadi AC-13 mengujinya secara native.

**Mode volatile dan `CONFIG SET` dibiarkan terbuka.** Dua temuan cross check punya akar yang sama: user `default` adalah admin, dan admin Valkey bisa menulis snapshot (F28) atau mengikat alamat lain (`CONFIG SET bind`). Menutup keduanya berarti melarang `CONFIG SET`, dan itu mematahkan pemakaian development yang nyata. Spec ini memilih batas yang bisa dijaga dan dinyatakan: Stackly menjamin apa yang ia jalankan (argv di setiap start), bukan apa yang pemegang password lakukan sesudahnya. Kalimat invariant dan security model ditulis ulang supaya tidak menjanjikan lebih dari itu.

**`valkeyConf` dengan pekerjaan nyata.** Menghapus format ini akan menurunkan angka RISK-07 dari dua ke satu, dan itu menggoda. Tetapi label `natsConf` di file Valkey adalah utang yang jatuh tempo pada setting teks pertama. `notifyKeyspaceEvents` adalah setting yang memang layak ada sejak awal dan yang nilainya membedakan kedua escaper, jadi format baru masuk dengan alasan, test, dan pemakai sekaligus.

**Ukuran RISK-07.** Dua perubahan core: satu milik fitur ini (`valkeyConf`), satu dibagi dengan fitur 43 (`sha256Hex`). Tidak ada primitive, probe, kode issue, method, DTO, migrasi, atau kode Swift baru. Readiness, stop, connection, Open In, remedy, harness, spike, dan catalog semuanya memakai yang sudah ada.

## Keputusan yang diambil atas delegasi user

User mendelegasikan setiap keputusan ke opsi yang direkomendasikan. Daftar ini adalah keputusan yang dalam alur biasa akan ditanyakan satu per satu:

1. Password selalu wajib (Keputusan 1).
2. Hash SHA-256 lewat filter `sha256Hex`, kontrak bersama fitur 43 (Keputusan 2).
3. Probe `exec` dengan user `stackly-readiness` yang hanya `PING` (Keputusan 3).
4. Satu enum `persistence`, default `rdb`, tidak bisa diganti sesudah init (Keputusan 4).
5. File include per mode (Keputusan 5).
6. Parameter di argv dan tanpa file config utama; `CONFIG SET` tetap terbuka (Keputusan 6).
7. Peringatan volatile lewat `description`, tanpa mekanisme UI baru (Keputusan 7).
8. Stop `SIGTERM`, tanpa reload (Keputusan 8 dan 9).
9. `redis://` utama dan `valkey://` kedua; Open In `valkey-cli` dan TablePlus; Redis Insight ditunda (Keputusan 10).
10. Tanpa `dataMarker` (Keputusan 11).
11. Evidence BR-025 dari suite paket; entri handover sesudah test launchd (Keputusan 12).
12. Settings awal: `password`, `persistence`, `maxMemoryMb`, `maxMemoryPolicy`, `logLevel`, `notifyKeyspaceEvents`; default `maxMemoryMb` 0 dan `noeviction` (bawaan upstream).
13. Bagian References tidak ditulis (opsi yang direkomendasikan skill: tanpa references).
14. File yang ditulis pengguna sendiri di mode `volatile` dinyatakan, tidak dilarang (Keputusan 13).
15. Format `valkeyConf` tetap ada, dengan setting `notifyKeyspaceEvents` sebagai pemakainya (Keputusan 14).
16. `CONFIG SET bind` dicatat sebagai residual, tidak dilarang.

## Cross check

Dijalankan 2026-11-12 oleh model lain (Opus), read only, terhadap `index.md`, `rationale.md`, spec yang dirujuk, dan kode. Semua temuan diterapkan dengan rekomendasi architect, sesuai delegasi user; tidak ada yang dibiarkan terbuka.

| No | Temuan | Yang diubah |
| --- | --- | --- |
| 1 | Mode `volatile` tidak benar benar kosong kalau pengguna menjalankan `SAVE` | diuji (F28); AC-7, README, dan *Edge cases* menyatakannya; Keputusan 13 |
| 2 | Klaim "hanya loopback" terlalu kuat: `CONFIG SET bind` bisa mengubahnya saat jalan | AC-4, *Key invariants*, dan *Security model* ditulis ulang; `bind` masuk residual |
| 3 | `knownLimitations` berisi kalimat, padahal `compat-record` menuntut kode snake_case | menjadi `power_loss_not_simulated` |
| 4 | Bentuk JSON segmen hash dan kode untuk nama filter kosong belum ditetapkan | AC-3 menetapkan `secretHash`, `algo` `sha256_hex`, dan `template_syntax`; dikabarkan ke `arc-f43-spec` |
| 5 | Kode untuk filter di config tidak sensitif seharusnya `secret_not_allowed_here` yang sudah ada | AC-3 diselaraskan dengan AC-2 spec 0043 |
| 6 | Test `LOADING` dengan data 500 MiB akan tidak stabil | *Test LOADING*: salinan paket dengan dua opsi debug, mode `rdb`, 2000 key (F30) |
| 7 | Sumber observasi `not_ready` lalu `ready`, dan bentuk `actual` evidence AC-8, belum disebut | AC-13 menyebut urutan event; AC-8 menetapkan bentuk `actual` |
| 8 | `valkey_9_1_installs_from_the_cache` sudah ada sebagai placeholder `blocked_line_test!` | langkah 7 menggantinya, bukan menambah |
| 9 | Dua test ber daftar tetap butuh baris Valkey | langkah 3 (tabel version check) dan langkah 8 (batas langkah harness) |
| 10 | Daftar file bersama kurang `versions.json` semua paket dan `evidence/compatibility/` | ditambah ke tabel *File bersama* |
| 11 | Schema editor tidak berubah karena format baru (`format` bertipe string) | klaim codegen dihapus dari langkah 1 dan tabel file bersama |
| 12 | PostgreSQL memakai `log_timezone`, bukan env `TZ` | catatan `TZ` diperbaiki |
| 13 | Paket draft tidak butuh `valkeyConf`; `natsConf` sudah cukup | setting `notifyKeyspaceEvents` ditambah (F29); Keputusan 14 |
| 14 | Kasus `FLUSHALL` lalu stop, dan waktu sekitar 31 detik sampai `crash_loop` di kasus AOF rusak | baris *Edge cases* dan anggaran waktu test AC-18 |

Yang diperiksa dan bertahan: token folder dan enum dalam satu entri `args`; `${instance.secrets}` di `args`; env literal di start dan probe `exec`; `run` di `managedFilesystem`; panjang dan token `remedies`; token `openWith` dan `connection`; secret ber kelas `restart` memberi `next_start` (`crates/core/src/secret/rules.rs`); `instance.config.validate` memberi `immutable_after_init`; `not_ready` sesudah 2 kegagalan berturut turut (sekitar 20 detik pada interval 10 detik); file sensitif dihapus di setiap exit; `frozen_facts` dan `PackageRecipe` menulis file sensitif; `versions.json` di luar `manifest_sha256`; `valkey` sudah di `PROVIDER_WORDS`; tanpa kenaikan protokol; respawn sesudah `SIGKILL` pada proses yang sudah ready tanpa setelan restart policy; dan langkah harness cocok dengan manifest ini.
