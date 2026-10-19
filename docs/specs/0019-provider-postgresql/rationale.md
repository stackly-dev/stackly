# 0019. Rationale: provider PostgreSQL

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: scope fitur 19 menyebut "readiness lewat query terautentikasi dengan role berhak minimum", dan brief sesi ini menyebut password untuk role readiness di bawah spec 0011. Mengikuti itu apa adanya berarti daemon membaca password dari Keychain setiap 10 detik selama instance hidup, atau menyimpannya di memori daemon tanpa batas waktu, padahal spec 0011 hanya menyelesaikan secret per aksi. Yang sebenarnya dibutuhkan adalah login yang terbukti (bukan port terbuka) sebagai role tanpa hak berbahaya. Peer auth lewat socket privat memberi login sungguhan (koneksi, autentikasi, query) tanpa password sama sekali, dibatasi folder socket 0700 milik user, jadi spec ini memakai itu dan tetap memenuhi kalimat scope. Fitur 19 juga dibangun di atas tiga fitur yang belum selesai (10, 11, dan 18); ketergantungan itu ditulis sebagai baris Assumed dan prasyarat build, bukan diputuskan di sini.

PostgreSQL adalah provider nyata pertama Stackly dan titik akhir JOURNEY-01: pengguna memasang runtime, membuat instance, menunggu ready, lalu menyalin connection. PRD bagian 22.1 meminta cluster dibuat dengan `initdb` dari runtime yang dipilih, credential bootstrap, locale, encoding, auth policy, dan database awal yang eksplisit, tanpa `trust` yang tidak disengaja, port, alamat, socket, dan file config per instance, readiness yang memeriksa query terautentikasi dengan role probe berhak minimum, tool admin dari runtime yang cocok (bukan `psql` dari `PATH`), dan larangan menjalankan binary mayor lain pada data yang ada (BR-009). REQ-035 menambah shutdown terkontrol, log, dan persistence; backup logical milik scope 09.

Kekuatan yang membatasi desain: manifest `stackly/v1alpha1` (spec 0009) adalah himpunan primitive tertutup tanpa shell, hook, atau kode provider, dan setiap perubahan core untuk satu provider dihitung sebagai biaya RISK-07. Supervisor (spec 0010) menjalankan proses di env tertutup, memeriksa ulang setiap sinyal, tidak pernah eskalasi stop, dan mengadopsi proses lewat lock instance yang diwariskan di fd 3. Spec 0011 melarang secret di argv (`ensure_not_in_argv` menolak argumen yang sama persis dengan nilai secret atau berbentuk `<apa pun>=<nilai>`) dan menyelesaikan secret sekali per aksi. Path socket Unix di macOS dibatasi 104 byte, dan spec 0004 menaruh socket instance di folder run pendek. Runtime PostgreSQL 17.11 dan 18.6 dibangun maintainer (spec 0003) dengan `bin/postgres`, `bin/initdb`, `bin/psql`, dan `bin/pg_dump`, ICU, OpenSSL, lz4, dan zstd, dan spike nya memakai `trust` lewat socket 0700 yang secara eksplisit bukan default instance.

Kalau ini tidak diputuskan, fitur 20 (create), 21 (connection), dan 23 (NATS, pembuktian engine kedua) tidak punya provider nyata untuk dibangun, dan perilaku auth, readiness, dan stop akan ditebak saat build, persis bagian yang paling berbahaya bila salah: database lokal yang bisa dimasuki tanpa password, status ready yang bohong, stop yang menggantung, atau data yang dibuka binary mayor lain.

## Options considered

### Keputusan 1: cara readiness melakukan query terautentikasi

#### Option 1: Probe `exec` dengan `psql` milik runtime, peer auth untuk role readiness di socket privat

Probe `exec` spec 0009 menjalankan `bin/psql` dari runtime instance ke socket instance sebagai `stackly_readiness` dan menjalankan `SELECT 1`. `pg_hba.conf` mengizinkan role itu hanya lewat `peer` dengan peta ke role itu saja, hanya ke database `postgres`.

**Pros**:

- Muat di primitive yang ada; tidak ada kode protokol PostgreSQL di daemon.
- Tanpa secret: tidak ada pembacaan Keychain berkala dan tidak ada password di memori daemon.
- Login sungguhan lewat jalur autentikasi PostgreSQL (`getpeereid` plus peta ident), dan readiness tetap jalan walau pengguna mengganti password superuser.
- Memakai `psql` dari runtime yang sama, jadi sekaligus memenuhi aturan tool admin dari runtime yang cocok.

**Cons**:

- Satu proses `psql` per 10 detik per instance.
- Proses lain milik user OS yang sama bisa login sebagai role readiness.
- Butuh `pg_hba.conf` dan `pg_ident.conf` buatan Stackly, dan role yang dibuat saat init.

#### Option 2: Probe native di daemon dengan password role readiness

Registry probe native spec 0009 mendapat `postgresql-readiness`: daemon berbicara protokol PostgreSQL (misalnya lewat crate `tokio-postgres`), login SCRAM sebagai role readiness dengan password di Keychain, lalu `SELECT 1`.

**Pros**:

- Tanpa spawn proses per probe; probe paling murah.
- Bisa memeriksa listener TCP yang dipakai klien secara langsung.

**Cons**:

- Dependency besar baru di daemon dan kontrak probe native yang dibangun lebih awal dari fitur 23.
- Password harus dibaca setiap probe atau disimpan di memori selama instance hidup, bertentangan dengan resolve per aksi spec 0011, dan Keychain yang terkunci membuat readiness gagal walau database sehat.
- Masih butuh role readiness yang dibuat saat init, sekarang dengan password di dalam SQL.

#### Option 3: `pg_isready` atau probe `tcp`

**Pros**:

- Paling sederhana, tanpa role dan tanpa config tambahan.

**Cons**:

- Tidak melakukan login atau query, jadi melanggar PRD 22.1 dan Done when fitur 19 ("readiness gagal kalau query gagal walau TCP terbuka").

#### Option 4: `psql` sebagai superuser dengan password

**Pros**:

- Tanpa role tambahan.

**Cons**:

- Melanggar "jangan memakai superuser default untuk setiap health check" (PRD 22.1), dan probe `exec` tidak boleh membawa secret (spec 0009), jadi butuh perubahan core yang sama beratnya dengan Option 2.

### Keputusan 2: cara membuat role readiness saat init

#### Option 1: Single user mode dengan stdin literal

Setelah `initdb`, langkah `runExecutable` menjalankan `postgres --single -D <data> -c exit_on_error=on template1` dengan stdin berisi SQL statis dari manifest. Ini butuh tambahan `stdin.text` di `runExecutable`.

**Pros**:

- Tanpa server berjalan, tanpa port, dan tanpa proses latar.
- `exit_on_error=on` membuat error SQL mana pun keluar 1 (dibuktikan di spike).
- Tambahan core kecil dan generik (MySQL dan provider lain bisa memakainya).

**Cons**:

- Satu perubahan core (RISK-07).
- SQL di manifest adalah teks literal yang harus ditinjau seperti args.

#### Option 2: Server sementara saat init

`pg_ctl -w start`, `psql -c ...`, lalu `pg_ctl stop` sebagai langkah init.

**Pros**:

- Pola umum (image Docker resmi).

**Cons**:

- `pg_ctl` melepas postmaster ke latar, keluar dari pengawasan supervisor dan melanggar `executionMode: foreground`; `pg_ctl` juga tidak ada di daftar `executables` artefak.
- Membuka listener selama init.

#### Option 3: Hook setelah start pertama

**Pros**:

- Bisa memakai `psql` biasa.

**Cons**:

- Tidak ada hook di v1alpha1, dan hook baru adalah permukaan yang jauh lebih besar daripada stdin literal; readiness pertama juga tidak bisa lolos sebelum role ada.

### Keputusan 3: kebijakan auth lokal

#### Option 1: SCRAM untuk semua role, peer hanya untuk role readiness

**Pros**:

- Tidak ada role berhak yang masuk tanpa password, lewat socket maupun TCP.
- Sesuai PRD 22.1 dan menjawab Follow-up spec 0003.

**Cons**:

- Pengguna selalu butuh password, termasuk lewat socket.

#### Option 2: `trust` lewat socket 0700 (seperti spike spec 0003)

**Pros**:

- Paling mudah dipakai; folder 0700 membatasinya ke user yang sama.

**Cons**:

- Siapa pun di user itu masuk sebagai role apa pun, termasuk superuser, tanpa password; spec 0003 sendiri menyatakan ini bukan default instance.

#### Option 3: Peer untuk semua koneksi lokal

**Pros**:

- Tanpa password lewat socket.

**Cons**:

- Peer tanpa peta mewajibkan nama role sama dengan nama user OS, dan dengan peta regex ke `postgres` sama saja dengan superuser tanpa password bagi setiap proses user itu.

### Keputusan 4: locale dan encoding cluster

#### Option 1: Locale provider builtin `C.UTF-8`, encoding UTF8, `LC_*` `C`

**Pros**:

- Urutan collation stabil lintas versi (dijamin PostgreSQL untuk provider builtin), tidak bergantung pada data locale macOS atau versi ICU, jadi index tidak perlu `REINDEX` setelah update OS atau runtime.
- Huruf besar dan kecil Unicode benar (berbeda dengan `C` polos), dan tersedia di kedua lini (17 dan 18).
- Pesan error berbahasa Inggris dan seragam, mudah dicocokkan test dan doctor.

**Cons**:

- Urutan code point, bukan urutan alfabet bahasa manusia.

#### Option 2: ICU `und` (seperti spike spec 0003)

**Pros**:

- Urutan alfabet yang natural.

**Cons**:

- Versi collation terikat ke versi ICU yang dibundel runtime; update runtime dengan ICU baru menghasilkan peringatan versi collation dan butuh `REINDEX`.

#### Option 3: Locale libc `en_US.UTF-8`

**Pros**:

- Pola umum di Linux dan Postgres.app.

**Cons**:

- Bergantung pada data locale macOS (bisa berubah saat update OS), dan di macOS hasilnya hampir sama dengan urutan byte, jadi biayanya tanpa manfaat.

### Keputusan 5: pembagian parameter antara argv dan file config

#### Option 1: Parameter milik Stackly di argv, sisanya di `postgresql.conf` di folder config

**Pros**:

- Argv mengalahkan `postgresql.auto.conf`, jadi `ALTER SYSTEM` tidak bisa menggeser port atau socket (dibuktikan di spike).
- Path hanya di argv, jadi tidak ada escape path di file config.
- Config buatan Stackly terpisah dari folder data, sehingga `pg_ctl -D` manual memakai file buatan `initdb` yang tetap aman.

**Cons**:

- Argv lebih panjang, dan ada dua set file config (yang dipakai di folder config, yang tidak dipakai di folder data).

#### Option 2: Semua config dirender ke folder data, menimpa file `initdb`

**Pros**:

- Satu set file, `postgres -D <data>` saja.

**Cons**:

- `ALTER SYSTEM SET port` bisa membuat Stackly kehilangan endpoint; path di dalam config butuh escape; config ikut di dalam backup data.

#### Option 3: Semua parameter di argv tanpa `postgresql.conf`

**Pros**:

- Tanpa template config sama sekali.

**Cons**:

- Reload (`SIGHUP`) tidak bisa mengubah parameter yang datang dari argv, jadi kelas field `apply: reload` tidak bisa didukung.

### Keputusan 6: strategi stop

#### Option 1: `SIGINT` (fast shutdown)

**Pros**:

- Selesai dalam hitungan detik walau ada klien terbuka (spike: sekitar 0,1 detik tanpa klien), dan menulis checkpoint shutdown sehingga start berikutnya tanpa recovery. Ini mode default `pg_ctl`.

**Cons**:

- Transaksi yang sedang berjalan dibatalkan.

#### Option 2: `SIGTERM` (smart shutdown)

**Pros**:

- Menunggu klien selesai dengan sendirinya.

**Cons**:

- Menggantung selama ada klien (spike: stop menunggu sampai sesi `pg_sleep` selesai), jadi stop biasa sering berakhir `stop_timeout`.

#### Option 3: `SIGQUIT` (immediate shutdown)

**Pros**:

- Paling cepat.

**Cons**:

- Tanpa checkpoint, start berikutnya menjalankan crash recovery; cocok untuk force stop (fitur 26), bukan stop normal.

### Keputusan 7: nama superuser dan database awal

#### Option 1: Tetap `postgres` dan `postgres`

**Pros**:

- Sesuai kebiasaan, tutorial, dan default tool klien; connection string sederhana.

**Cons**:

- Pengguna yang ingin database bernama aplikasi membuatnya sendiri.

#### Option 2: Settings `superuserName` dan `databaseName`

**Pros**:

- Fleksibel seperti `POSTGRES_USER` dan `POSTGRES_DB` di image Docker.

**Cons**:

- Database tambahan butuh SQL berisi nilai pengguna saat init (escape identifier dan interpolasi di stdin), dan setiap nilai menjadi `immutableAfterInit` baru; bisa ditambah additive nanti.

### Keputusan 8: file `pg_hba.conf` dan `pg_ident.conf` yang statis

#### Option 1: Format template baru `plain`

**Pros**:

- Nama format jujur, dan cek statis menjamin tidak ada nilai yang pernah dikeluarkan tanpa escaper yang benar.
- Generik untuk file statis provider lain.

**Cons**:

- Satu perubahan core lagi (kecil).

#### Option 2: Pakai `postgresqlConf` untuk ketiga file

**Pros**:

- Tanpa format baru.

**Cons**:

- Kutip di `pg_hba.conf` berbeda dengan `postgresql.conf`; tag yang kelak ditambahkan ke template hba akan di escape dengan aturan yang salah tanpa ada yang menolaknya.

### Keputusan 9: password dengan karakter kontrol

#### Option 1: Flag `x-stackly.printableOnly` yang dicek `SecretService::put`

**Pros**:

- Error jelas saat create, bukan password yang terpotong diam diam oleh `initdb --pwfile` (baris pertama saja).
- Opt in per field, jadi secret berisi PEM atau teks multi baris di provider lain tidak terpengaruh, dan `generate` selalu lolos.

**Cons**:

- Menyentuh dua spec (0009 dan 0011) dan menambah kunci `details` di kontrak.

#### Option 2: Terima dan dokumentasikan

**Pros**:

- Tanpa perubahan core.

**Cons**:

- Password di Keychain berbeda dengan password di database tanpa tanda apa pun; connection string Stackly gagal dan pengguna tidak tahu sebabnya.

### Keputusan 10: runtime untuk test native

#### Option 1: Fixture installer fitur 18 (`install_cached_artifact`) dari cache runtime xtask

**Pros**:

- Satu installer saja (pipeline asli: digest, ekstraksi, validasi, commit), tanpa jaringan, dan `BLOCKED` yang jelas kalau cache kosong. Disepakati dengan sesi `arc-f18-spec`.

**Cons**:

- Milestone native fitur 19 menunggu milestone 2 fitur 18.

#### Option 2: Helper ekstraksi sendiri di fitur 19

**Pros**:

- Tidak menunggu fitur 18.

**Cons**:

- Dua installer dengan aturan keamanan yang bisa berbeda; kode yang dibuang setelah fitur 18 ada.

## Rationale

**Keputusan 1** jatuh ke probe `exec` dengan peer auth karena kekuatan yang paling mengikat adalah model secret spec 0011 dan biaya RISK-07. Option 2 memang lebih murah per probe, tetapi ia menukar satu spawn kecil setiap 10 detik dengan dependency protokol di daemon, password yang hidup di memori, dan readiness yang ikut gagal saat Keychain terkunci. Dengan peta regex, peer auth secara fungsi setara `trust` yang dibatasi ke satu role tanpa hak di satu socket privat; tidak ada rahasia yang bisa bocor, dan risiko bahwa user OS yang sama bisa login sebagai role readiness tidak menambah apa pun karena user itu sudah memiliki folder data. Peer dipilih daripada baris `trust` untuk role itu supaya invariant "tidak ada `trust` di mana pun" tetap sederhana diaudit, dan supaya peta bisa dipersempit ke user OS yang persis kalau kelak ada token untuk nama user. Runner up: Option 2, kalau pengukuran fitur 64 menunjukkan spawn `psql` terlalu mahal untuk jumlah instance yang realistis.

**Keputusan 2 dan 8** sama sama memilih tambahan core kecil yang generik daripada memaksa primitive yang ada. Server sementara saat init melanggar aturan foreground yang menjadi dasar adopsi spec 0010, dan memakai `postgresqlConf` untuk file hba membuat escaper yang salah suatu hari dipakai tanpa ada yang menolak. Kedua tambahan bisa dipakai provider lain (MySQL butuh SQL bootstrap, Valkey dan ClickHouse punya file statis), jadi biaya RISK-07 nya terbayar lebih dari sekali. Runner up untuk Keputusan 2: server sementara lewat fitur hook kalau suatu hari v1alpha1 mendapat hook untuk alasan lain.

**Keputusan 3** mengikuti PRD 22.1 dan Follow-up spec 0003 secara langsung: `trust` dan peer untuk semua role sama sama membiarkan superuser masuk tanpa password. **Keputusan 4** memilih builtin `C.UTF-8` karena dev database lokal hidup lama di Mac yang terus di update; collation yang berubah diam diam setelah update OS atau ICU adalah penyebab index rusak yang sulit dilacak, dan ICU tetap tersedia per kolom untuk yang butuh urutan bahasa (runner up: ICU `und`). **Keputusan 5** didorong oleh kebutuhan bahwa endpoint yang tercatat di registry selalu benar: hanya argv yang tidak bisa digeser `ALTER SYSTEM`, dan memisahkan config Stackly dari folder data juga membuat file `initdb` tetap aman untuk pemakaian manual (runner up: render ke folder data). **Keputusan 6** memakai fast shutdown karena spec 0010 tidak pernah eskalasi stop, jadi strategi yang bisa menggantung (smart) akan sering meninggalkan instance di `stopping`.

**Keputusan 7, 9, dan 10** memilih yang paling sederhana dan paling jujur: nama `postgres` mengikuti kebiasaan dan bisa diperluas additive, `printableOnly` mengubah kegagalan diam diam menjadi error yang jelas, dan satu installer menghindari dua jalur keamanan. `ALTER SYSTEM` sengaja tetap boleh (runner up: `allow_alter_system = off`) karena developer memakainya untuk extension seperti `pg_stat_statements` lewat `shared_preload_libraries`, dan parameter yang wajib dimiliki Stackly sudah aman di argv.

## Keputusan kecil

| Keputusan | Pilihan | Alasan | Runner up |
| --- | --- | --- | --- |
| Penulisan superuser di argv `initdb` | `-Upostgres` (menempel) | `--username=postgres` atau `-U postgres` ditolak `ensure_not_in_argv` kalau password pengguna `postgres` | nama superuser lain |
| Database untuk single user mode | `template1` | menghindari argumen `postgres` utuh; perintah role berlaku untuk seluruh cluster | `postgres` |
| Atribut role readiness | `LOGIN`, semua hak lain eksplisit `NO...`, `CONNECTION LIMIT 3`, read only, `statement_timeout` 2 detik, komentar role | berhak minimum dan menjelaskan dirinya di `\du`; limit 3 memberi ruang readiness, health, dan satu percobaan yang belum dibersihkan | tanpa limit |
| Database probe | lewat env `PGDATABASE=postgres`, bukan `--dbname=postgres` | argv probe ikut diperiksa `ensure_not_in_argv` saat dry run start, dan `=postgres` ditolak kalau password pengguna `postgres` | `-dpostgres` (menempel) |
| Tujuan log | `--logging-collector=off` dan `--log-destination=stderr` di argv | `ALTER SYSTEM SET logging_collector = on` tidak boleh memindahkan log keluar dari spool | di `postgresql.conf` |
| Query readiness | `SELECT 1` | cluster primary tunggal; koneksi sendiri sudah ditolak selama startup dan recovery | `pg_is_in_recovery()` |
| Timeout koneksi probe | `PGCONNECT_TIMEOUT=2`, percobaan 3000 ms | 2 detik adalah minimum libpq dan muat di bawah batas percobaan | percobaan 2000 ms |
| Timeout fase | init 180 (`initdb` 90, role 30, sisanya ruang render dan secret), readiness 120, recovery 300, stop 60, reload 30 detik | `initdb` dengan checksum selesai dalam beberapa detik; ruang untuk crash recovery dan checkpoint besar | default spec 0009 |
| Checksum data | aktif di kedua lini | 18 sudah aktif secara default, 17 belum; dibuat sama supaya perilaku tidak bergantung lini | default per lini |
| Zona waktu | `timezone` dan `log_timezone` `UTC` | `initdb` memilih zona sistem walau env tertutup (spike: `Asia/Jakarta`), jadi tanpa ini hasilnya bergantung Mac; UTC sejalan dengan log Stackly | settings `timezone` |
| Log | stderr, `logging_collector = off`, `log_line_prefix = '%m [%p] %q%u@%d '` | spool spec 0010 sudah menangkap, menyamarkan, dan memutar stderr | file log PostgreSQL di `${instance.logs}` |
| `cluster_name` | nama instance | terlihat di judul proses (`ps`) saat banyak instance berjalan | tanpa |
| `ssl` | `off` | hanya loopback; TLS di Deferred | sertifikat lokal |
| `ALTER SYSTEM` | tetap boleh | lihat Rationale | `allow_alter_system = off` |
| Capability | `reload`, `multiInstance`, `persistence`, backup `cold` (`data`) | semuanya didukung PostgreSQL dan primitive v1alpha1; config dibangkitkan ulang dari settings, jadi tidak ikut backup | backup tanpa capability dulu |
| Purpose endpoint socket | `client` | klien lokal boleh memakainya (`psql -h <folder>`) | `admin` |
| Batas settings | `maxConnections` 5 sampai 1000, `sharedBuffersMb` 16 sampai 16384, `logMinDurationStatementMs` -1 sampai 3600000 | masuk akal untuk Mac development; semaphore System V macOS jauh di atas kebutuhan 1000 koneksi | batas lebih longgar |
| Panjang password | 8 sampai 128 | di bawah 8 byte tidak disamarkan andal oleh `Redactor` (spec 0011); `generate` 32 karakter | 1 sampai 128 dengan peringatan |
| Isi teks `stdin.text` | 1 sampai 16384 byte, tanpa `${`, kontrol hanya `\n` dan `\t` | cukup untuk SQL bootstrap; `${` ditolak supaya penulis tidak mengira ada interpolasi | interpolasi terbatas |

## Spike lokal 2026-10-19

Dijalankan di Mac ini dengan artefak dari cache runtime xtask (`postgresql-18.6-r2-macos-aarch64` sha256 `88bb7533...6590`, `postgresql-17.11-r2-macos-aarch64` sha256 `04b1eff1...d6d3`, keduanya dicek ulang dengan `shasum -a 256`), diekstrak ke scratchpad sesi di bawah folder bernama `Test Root Ünï`, semua proses dengan `env -i` dan `PATH=/usr/bin:/bin:/usr/sbin:/sbin`. Ini bukan evidence; ia hanya memastikan asumsi desain sebelum spec ditulis.

| Pemeriksaan | Hasil |
| --- | --- |
| `initdb` 18 dan 17 dengan flag di manifest | exit 0; `locale provider: builtin`, `default collation: C.UTF-8`, semua `LC_*` `C`, checksum aktif; `PG_VERSION` berisi `18\n` dan `17\n`; `pg_hba.conf` buatan `initdb` hanya `scram-sha-256`; zona waktu yang dipilih `initdb` `Asia/Jakarta` walau env tertutup |
| `-Upostgres` dan password `postgres` | diterima `initdb` |
| Password berisi spasi, `'`, `"`, dan `\` lewat `--pwfile` | login superuser berhasil |
| `--pwfile` kosong | `initdb` gagal `password file ... is empty` |
| `initdb` ke folder tidak kosong | exit 1, `directory ... exists but is not empty` |
| `postgres --single ... -c exit_on_error=on` dengan SQL lewat stdin | exit 0 dan role terbuat; SQL yang error (`role already exists`) exit 1; `template1` juga berhasil |
| Start dengan `-D`, `--config-file`, `--hba-file`, `--ident-file`, `--listen-addresses`, `--port`, `--unix-socket-directories`, `--unix-socket-permissions=0700` | listen hanya di `127.0.0.1:<port>` dan `/tmp/<run>/.s.PGSQL.<port>` (mode `srwx------`), plus file `.s.PGSQL.<port>.lock` |
| `psql` dengan argv probe sebagai `stackly_readiness` (peer, peta regex) | `1`, exit 0 |
| Role readiness ke `template1` | ditolak `pg_hba.conf rejects connection`, exit 2 |
| `CREATE TABLE` sebagai role readiness | `cannot execute CREATE TABLE in a read-only transaction`, exit 1 |
| Superuser tanpa password lewat socket | `fe_sendauth: no password supplied`, exit 2 |
| Superuser dengan password lewat socket dan TCP | berhasil; `datlocprovider b`, `datlocale C.UTF-8`, `UTF8`, `C`, `C`; `data_checksums on`, `TimeZone UTC`, `hba_file` dari argv |
| `ALTER SYSTEM SET port = 6000` lalu start ulang | tersimpan di `postgresql.auto.conf`, tetapi server tetap listen di port argv |
| `SIGINT` | shutdown dalam sekitar 0,1 detik; `postmaster.pid`, socket, dan lock socket terhapus |
| `SIGTERM` dengan satu sesi `pg_sleep(4)` terbuka | masih berjalan 2 detik setelah sinyal, baru keluar setelah sesi selesai |
| `SIGHUP` setelah `log_statement` diubah di file | `SHOW log_statement` menjadi `all` |
| Probe saat server mati | `psql: error: connection to server on socket ... failed: No such file or directory`, exit 2 |
| Binary 17 pada data 18 | `FATAL: database files are incompatible with server`, exit 1 (sesudah spawn) |
| Lock di fd 3 saat start | postmaster dan kedelapan child nya memegang fd 3 ke file lock |
| `postgres -C` dengan `'a''b\\c d'` dan `'ü 空 a''b\\c d'` | membaca kembali `a'b\c d` dan `ü 空 a'b\c d` persis |
| `sysctl kern.sysv` | `shmmni 32`, `shmseg 8`, `semmni 87381`, `semmns 87381` |

## Pertukaran dengan sesi lain

- `arc-f18-spec` (2026-10-19): mengonfirmasi validasi `executables` dan dependency saat install serta record runtime dengan `line` dan `version`; menetapkan `install_cached_artifact` dan `runtime_cache::cached_artifact` sebagai satu satunya jalur pasang runtime untuk test native, dengan `BLOCKED` tanpa cache; mengusulkan `runtime.versionCheck` dengan pattern PostgreSQL; dan memberi bentuk `compatibility.json`.
- `arc-f17-spec` (2026-10-19): entri `providers/catalog.toml` dipindah dari `[[planned]]` ke `[[provider]]` oleh build fitur ini, dan `metadata.description` menjadi teks kartu catalog, jadi ditulis untuk pengguna.

## Cross check

Dijalankan 2026-10-19 sebagai pembacaan read only oleh model lain (Fable 5.1), setelah percobaan pertama terputus batas pemakaian. Temuan dan penyelesaiannya (opsi yang direkomendasikan diambil, sesuai aturan user di brief sesi ini):

| No | Temuan | Jenis | Penyelesaian |
| --- | --- | --- | --- |
| 1 | `--dbname=postgres` di argv probe ditolak `ensure_not_in_argv` saat dry run start kalau password pengguna `postgres` (blocker) | soundness | database probe lewat env `PGDATABASE`; golden memeriksa tidak ada argv `postgres` atau `=postgres` di plan init, start, dan probe |
| 2 | `logging_collector` dan `log_destination` di `postgresql.conf` bisa digeser `ALTER SYSTEM`, memindahkan log keluar dari spool | soundness | keduanya pindah ke argv |
| 3 | `initSeconds` 120 sama dengan jumlah batas langkah 90 dan 30, sehingga `initdb` yang lambat membuat langkah role kehabisan waktu dengan error di langkah yang salah | soundness | `initSeconds` 180 |
| 4 | Ringkasan menjual peer auth sebagai identitas yang diperiksa kernel, padahal peta regex menerima user mana pun | keputusan | tetap peer dengan peta regex; teks Summary, *Auth lokal*, dan rationale diperjelas bahwa batasnya folder socket 0700; runner up: satu baris `trust` khusus role readiness tanpa `pg_ident.conf` |
| 5 | Log PostgreSQL di `TailSink` tidak bisa diamati lewat IPC minimal | keputusan | bagian log AC-13 berjalan in process lewat `stacklyd::server::start` dengan `LogSink` perekam (kait test di langkah 6) |
| 6 | Sumber `data_line` untuk AC-13 tidak disebut | keputusan | `data.initializedLine` di `instance-manifest.json` |
| 7 | Detail kegagalan probe `exec` bersyarat pada keadaan build spec 0010 | keputusan | format menjadi milik spec ini tanpa syarat; Follow-up mencatatnya di spec 0010 |
| 8 | "Jauh sebelum `stopSeconds`" tidak objektif | keputusan | batas 10 detik di AC-10 |
| 9 | `CopyFrom` harus membaca Keychain, jadi "tanpa menyentuh Keychain" salah | perbaikan teks | "tanpa menulis ke Keychain" |
| 10 | Spec 0017 dan 0018 belum ada di repo | perbaikan teks | signature dan perintah yang diharapkan ditulis di baris Assumed, dengan catatan update sebelum milestone 2 kalau berbeda |
| 11 | Port golden dan bentuk endpoint `socket` tidak disebut | perbaikan teks | port 14000 dan `ResolvedEndpoint::Unix` di skenario golden |
| 12 | Verifikasi reload tidak membuktikan nilai berlaku | perbaikan teks | baris baru di *Edge cases* |
| 13 | Single user mode memotong statement per baris | perbaikan teks | satu statement per baris dicatat di AC-3 |
| 14 | File `.s.PGSQL.<port>.lock` tidak disebut | perbaikan teks | disebut di AC-8, AC-10, dan *Edge cases* |
| 15 | `printableOnly` bisa dibalik menjadi default untuk semua secret | catatan | dicatat di Follow-up update spec 0011 |

Pemeriksaan validator oleh reviewer: semua aturan spec 0009 lolos kecuali tepat empat tambahan yang dideklarasikan (`format: plain`, `format: postgresqlConf`, `stdin.text`, `x-stackly.printableOnly`), sehingga langkah 4 Build plan memang harus menunggu langkah 1 sampai 3.

## Konsistensi lintas spec 2026-10-20

Pemeriksaan lead atas spec 0015, 0017, 0018, dan 0019 setelah keempatnya masuk `main` menemukan empat hal milik spec ini, masing masing dikonfirmasi reviewer kedua. Perbaikan yang direkomendasikan diterapkan sesuai aturan user.

| No | Temuan | Perbaikan |
| --- | --- | --- |
| 1 | `runtime.versionCheck` (dulu langkah 11) masuk sesudah evidence provider (dulu langkah 10). Field itu ada di `provider.yaml` dan ikut `manifest_sha256`, sehingga evidence BR-025 terikat ke digest lama dan `support_status` spec 0018 tidak pernah memberi `supported` | `versionCheck` pindah ke milestone 2 sebagai langkah 7, sebelum test native, dengan prasyarat langkah 1 spec 0018; langkah evidence (sekarang 11) dan AC-14 mewajibkan evidence dari `provider.yaml` final dan run ulang setelah paket berubah |
| 2 | Versi subject evidence diambil dari `ArtifactRecord`, padahal spec 0002 meminta versi yang dibaca dari binary yang berjalan | `version` dari `SHOW server_version` pada instance yang berjalan, `digest` dari `ArtifactRecord.sha256` (Value sourcing, AC-14) |
| 3 | `Stdin::Text` tidak punya jalur ke `SpawnRequest`, karena port spec 0010 hanya punya `StdinSource::Null` dan `Bytes(SecretValue)` | executor init mengirim teks sebagai `StdinSource::Bytes(SecretValue::new(text))` tanpa varian port baru; detail probe disebut sebagai baris pertama dari buffer ekor stderr yang ditangkap; keduanya masuk Follow-up update spec 0010 |
| 4 | Langkah evidence butuh `cargo xtask runtime compat-record`, yang baru dibangun langkah 13 spec 0018 (milestone 3), tetapi prasyarat tidak menyebutnya | prasyarat langkah 11 menyebut langkah 13 spec 0018 |

Asumsi pada spec 0017 dan 0018 juga diperiksa terhadap teks di `main`: `runtime::fixture::install_cached_artifact` (langkah 9, milestone 2 spec 0018), `runtime_cache::cached_artifact`, bentuk `compatibility.json` dan aturan `compat-record` cocok; field TOML catalog adalah `published_at` dengan ledger `providers/catalog-revisions.json`, dan *Aturan masuk catalog* spec 0017 diikuti langkah 9. Aturan pencocokan pattern `versionCheck` (full match) sedang diperbaiki di spec 0018, jadi pattern PostgreSQL diberi `\n?` di akhir supaya cocok dengan keluaran satu baris `postgres --version` di bawah aturan mana pun.
