# 0042. Provider MySQL: rationale

Catatan keputusan untuk [index.md](index.md). `/develop` tidak perlu membaca file ini; isinya menjelaskan kenapa spec berbentuk seperti itu.

## Context

- Scope fitur 42 (`docs/scope/07-stack-enam-service.md`) meminta path data, config, temp, dan log yang terisolasi, bind dan auth eksplisit, readiness lewat query terautentikasi berhak minimum, dan shutdown terkontrol. Selesainya diukur dengan dua instance yang hanya membaca sentinel miliknya setelah restart, penolakan data yang tidak kompatibel lewat preflight, dan penolakan data yang sudah ada sampai fitur 57. Trace: REQ-055 tanpa adoption, AC-081 tanpa restore, PRD bagian 22.6.
- `runtimes/inventory.toml` sudah memuat dua artefak MySQL `available`: 8.4.11 dan 9.7.2 (yang kedua `stable`). `cargo xtask codegen` menulis `versions.json` paket dari inventory, jadi paket yang masuk `providers/` otomatis menawarkan kedua lini.
- Manifest `stackly/v1alpha1` membatasi cara secret sampai ke proses: tidak pernah di argv; lewat stdin, env, atau file sekali pakai hanya di `lifecycle.start` dan langkah `runExecutable` init; dan probe `exec` sama sekali tidak bisa membaca secret (spec 0009, 0011). Setiap langkah init adalah satu executable dari runtime yang harus keluar sendiri; tidak ada shell dan tidak ada proses yang hidup melewati satu langkah.
- MySQL berbeda dari PostgreSQL di empat hal yang menentukan desain: tidak ada single user mode untuk menjalankan SQL tanpa server; manual MySQL menyebut file init `mysqld --initialize` terbatas untuk statement pengelolaan akun (spike di bawah menunjukkan 8.4.11 dan 9.7.2 menerimanya); build macOS tidak membawa plugin `auth_socket`; dan variabel yang disimpan `SET PERSIST` mengalahkan argumen command line.
- Binary MySQL 9.7 yang dijalankan di data 8.4 meng upgrade data itu di tempat tanpa bertanya. BR-009 dan PRD 22.6 melarang itu tanpa preflight dan prosedur migrasi teruji.
- RISK-07 mengukur berapa banyak perubahan core yang dibutuhkan setiap provider baru. Spec 0009 sudah merencanakan satu untuk MySQL: format config `mysqlCnf`.
- User mendelegasikan setiap keputusan ke opsi yang direkomendasikan (brief umum scope 07), jadi tidak ada wawancara; setiap keputusan di bawah diambil dengan opsi rekomendasi dan dilaporkan ke lead.

## Hasil spike lokal

Dijalankan 2026-11-12 di Mac ini dari cache runtime xtask (tanpa unduhan), di folder sementara yang lalu dihapus. Artefak: `mysql-8.4.11-macos-aarch64` dan `mysql-9.7.2-macos-aarch64`.

| No | Yang diuji | Hasil |
| --- | --- | --- |
| 1 | `mysqld --no-defaults --initialize-insecure` | exit 0 dalam sekitar 1,3 detik; `root@localhost` tanpa password; tidak ada akun anonim; folder data sekitar 90 MiB |
| 2 | Penanda versi di folder data | tidak ada `mysql_upgrade_info`; ada `mysql_upgrade_history` berisi JSON satu baris dengan `"version":"8.4.11"` (9.7: `"version":"9.7.2"`), format sama di kedua lini |
| 3 | `--initialize-insecure` di folder berisi satu file | exit 1, `--initialize specified but the data directory has files in it`, file itu utuh |
| 4 | Server biasa dengan `--skip-networking --init-file=/dev/stdin`, SQL dari file dan dari pipe | SQL dijalankan; `SHUTDOWN` sebagai statement terakhir membuat server keluar 0 dalam sekitar 1,3 detik; socket dan file pid hilang |
| 5 | Statement yang salah di file init | MySQL mencatat error, melewati sisa file (termasuk `SHUTDOWN`), dan tetap berjalan |
| 6 | `LOAD_FILE` atas file 0600 di folder `--secure-file-priv` | terbaca; `@@secure_file_priv` berakhir `/` |
| 7 | `ALTER USER` dan `CREATE USER` lewat `PREPARE` dengan `QUOTE(@pw)`, password `pa'ss"w\ord ü空 x` | login `root` dengan password itu berhasil lewat socket dan TCP, di 8.4 dan 9.7 |
| 8 | Plugin auth | `lib/plugin/` tidak memuat `auth_socket.so` di kedua lini; `mysql_native_password` `DISABLED` di 8.4 |
| 9 | `stackly_readiness@localhost` tanpa password, `--skip-name-resolve=ON` | lewat socket `SELECT 1` berhasil; lewat TCP `Access denied for user 'stackly_readiness'@'127.0.0.1'`; hanya `GRANT USAGE`; `CREATE DATABASE` ditolak |
| 10 | Akun yang sama dengan name resolve bawaan (`OFF`) | lewat TCP **diterima** sebagai `stackly_readiness@localhost` |
| 11 | `root` lewat TCP dengan `skip_name_resolve = ON` | cocok dengan `root@127.0.0.1`, bukan `root@localhost` |
| 12 | Listener | hanya port TCP dan satu socket dengan `--mysqlx=OFF` |
| 13 | fd 3 | `mysqld` mewarisi dan tetap memegang fd 3 (lock) |
| 14 | `SET PERSIST` dan `SET PERSIST_ONLY` | ditolak untuk `port`, `bind_address`, `socket`, `datadir`, `basedir`, `tmpdir`, `pid_file`, `secure_file_priv`, `log_error`, `admin_address`, `persisted_globals_load`; diterima untuk `skip_name_resolve`, `max_connections`, `general_log_file`, `local_infile`, `lower_case_table_names` |
| 15 | Restart dengan `--no-defaults` | `mysqld-auto.cnf` tidak dibaca sama sekali |
| 16 | Restart dengan `--defaults-file` | nilai persisted mengalahkan file opsi dan argv (`max_connections` 77, `skip_name_resolve` 0); `lower_case_table_names = 1` yang persisted membuat server gagal start |
| 17 | Restart dengan `--defaults-file` dan `--persisted-globals-load=OFF` | nilai persisted diabaikan; nilai file opsi dan argv berlaku |
| 18 | `SIGTERM` dengan sesi `SELECT SLEEP(600)` terbuka | `Shutdown complete` dalam sekitar 3 detik |
| 19 | Binary 8.4 di data 9.7 | exit 1, `Invalid MySQL server downgrade: Cannot downgrade from 90702 to 80411` |
| 20 | `mysqld --no-defaults --version` | `<root>/bin/mysqld  Ver 8.4.11 for macos15 on arm64 (MySQL Community Server - GPL)` (diawali path) |
| 21 | Pembaca file opsi, nilai `"a\"b\\c d # e ü空 'q' ; z"` | `my_print_defaults` mencetak `a"b\c d # e ü空 'q' ; z` |
| 22 | Panjang path socket | di atas 103 byte ditolak (`The socket file path is too long (> 103)`) |
| 23 | Probe saat server mati atau akun dihapus | `ERROR 2002 ... Can't connect ... through socket` dan `ERROR 1045 ... Access denied for user 'stackly_readiness'@'localhost'`, exit 1 |

Putaran kedua, sesudah cross check (2026-11-12, artefak yang sama):

| No | Yang diuji | Hasil |
| --- | --- | --- |
| 24 | `--initialize-insecure --init-file=/dev/stdin` dengan `ALTER USER`, `CREATE USER`, `GRANT`, `PREPARE` dan `EXECUTE` (pipe) | diterima di 8.4.11 dan 9.7.2; exit 0 dalam sekitar 1,5 detik; login `root` ber password lewat socket dan TCP berhasil dengan password `pa'ss"w\ord ü空 x`; `root` tanpa password ditolak; akun tidak terkunci tepat `root@localhost`, `root@127.0.0.1`, `root@::1`, `stackly_readiness@localhost` |
| 25 | Statement yang salah di file init `--initialize` | MySQL berhenti di statement itu dan keluar 1 (`The designated data directory ... is unusable`), statement sesudahnya tidak dijalankan |
| 26 | `@@secure_file_priv` saat `--initialize` | kosong walau `--secure-file-priv=<dir>` diberikan, jadi tidak bisa membawa path |
| 27 | `LOAD_FILE(CONCAT(@@replica_load_tmpdir, '/root-password'))` saat `--initialize` dengan `--replica-load-tmpdir=<dir>` | file 0600 terbaca |
| 28 | File binary log sesudah init | tanpa `--disable-log-bin`, statement akun membuat `binlog.000001` dan `binlog.index` di folder data; dengan `--disable-log-bin` tidak ada |
| 29 | `SHOW GRANTS` `root@localhost` dibanding `root@127.0.0.1` sesudah `GRANT ALL` dan `GRANT PROXY` | sama kecuali `AUDIT_ABORT_EXEMPT` dan `FIREWALL_EXEMPT`; `GRANT` kedua nama itu di file init gagal dengan error sintaks (belum terdaftar saat `--initialize`) |
| 30 | Akun terkunci | `ERROR 3118 (HY000): Access denied for user 'stackly_readiness'@'localhost'. Account is locked.` |
| 31 | `--initialize-insecure` dengan `--datadir` yang memuat spasi dan huruf non ASCII | folder data dibuat dan file init dijalankan (run itu berakhir di statement baris 29, jadi start di path seperti itu belum diuji) |

Yang tidak diuji spike: binary 9.7 di data 8.4 (akan mengubah data; perilaku upgrade di tempat adalah perilaku MySQL yang didokumentasikan), dan start di folder data dengan spasi atau huruf non ASCII di path. Keduanya ditutup test native (harness konformitas memakai folder data seperti itu).

## Options considered

### Keputusan 1: cara memasang password `root` saat init

Kendala: tidak ada server saat init, secret tidak boleh di argv, dan setiap langkah harus keluar sendiri.

- **Option 1 (dipilih): satu proses `mysqld --initialize-insecure --init-file=/dev/stdin` yang menjalankan SQL statis dari stdin dan membaca password dari file `materializeSecret` lewat `LOAD_FILE`.**
  - Pro: nol perubahan core dan satu langkah init. Tidak ada server yang menerima koneksi selama `root` belum ber password. Password tidak pernah ada di file config, argv, atau teks SQL manifest; file nya dihapus executor di akhir init. `QUOTE()` membuat literal di dalam server, jadi Stackly tidak butuh escaper SQL. SQL yang gagal membuat proses keluar 1 saat itu juga, dengan pesan MySQL di cuplikan (spike baris 24, 25, 27).
  - Kontra: manual MySQL menyebut statement akun terbatas di file init `--initialize`; kedua artefak menerimanya, tetapi itu bukan kontrak tertulis. Path folder secret dibawa lewat variabel replication (`replica_load_tmpdir`), karena `secure_file_priv` dikosongkan saat `--initialize` (baris 26).
- **Option 2 (cadangan): `--initialize-insecure`, lalu server sementara tanpa jaringan (`--skip-networking`, socket privat) yang membaca SQL yang sama dari stdin dan diakhiri `SHUTDOWN`.**
  - Pro: statement akun di server biasa adalah jalur yang didokumentasikan (prosedur reset password root memakai `--init-file`); dibuktikan spike (baris 4, 6, 7).
  - Kontra: dua langkah; statement yang gagal membuat server tetap berjalan (baris 5), jadi kegagalan hanya terlihat lewat batas waktu, tanpa cuplikan, dan server itu bisa tertinggal kalau daemon mati di tengah; butuh `--disable-log-bin`, `--socket`, dan `--mysqlx=OFF` supaya tidak meninggalkan file atau socket global.
- **Option 3: file SQL sensitif di `${instance.secrets}` yang dirender template dengan `{{ secret.rootPassword }}`.**
  - Pro: lebih mudah dibaca; mekanisme 5 spec 0011 sudah ada.
  - Kontra: butuh escaper literal SQL (perubahan core kedua, atau memakai ulang escaper format lain untuk SQL, yang rapuh). `RunPlan.render` menulis ulang setiap config sebelum setiap spawn, jadi password `root` akan tinggal sebagai teks polos di folder run selama instance ada.
- **Option 4: core menghitung hash `caching_sha2_password` dan init menulisnya sebagai literal hex.**
  - Pro: tidak ada password polos di luar Keychain dan memori daemon.
  - Kontra: implementasi KDF khusus MySQL di core (perubahan core besar yang sulit ditinjau).
- **Option 5: `--initialize` (password acak sementara) lalu mengganti password saat start pertama.**
  - Pro: tidak butuh file init.
  - Kontra: password sementara tercetak di log; mengganti password butuh klien dan secret saat start; ada jendela waktu dengan password yang tidak dikenal Keychain.

### Keputusan 2: akun readiness

Kendala: probe `exec` tidak bisa membaca secret, dan scope meminta query terautentikasi berhak minimum.

- **Option 1 (dipilih): akun `stackly_readiness@localhost` tanpa password dan tanpa hak, dengan `skip_name_resolve = ON` sehingga hanya cocok lewat socket di folder run 0700.**
  - Pro: nol perubahan core; tidak ada secret yang dibaca setiap 10 detik; perilakunya dibuktikan spike (baris 9, 10). Sama fungsinya dengan `peer` ber peta `/^.*$` di paket PostgreSQL.
  - Kontra: akun tanpa password ada di server; keamanannya bergantung pada `skip_name_resolve`, yang karena itu harus dijaga dari `SET PERSIST`.
- **Option 2: akun dengan `REQUIRE X509`, probe memakai certificate klien yang dibangkitkan MySQL di folder data.**
  - Pro: autentikasi lewat kepemilikan kunci; tidak bergantung pada `skip_name_resolve`.
  - Kontra: certificate bawaan kedaluwarsa dan bisa diganti pengguna, sehingga readiness rusak tanpa sebab yang jelas; batas keamanannya tetap folder milik user yang sama; lebih banyak bagian yang bisa salah.
- **Option 3: probe native yang berbicara protokol MySQL di core.**
  - Pro: tanpa proses `mysql` setiap 10 detik.
  - Kontra: perubahan core besar (handshake dan auth `caching_sha2_password`), dan tetap butuh akun tanpa password atau secret di memori daemon.
- **Option 4: `mysqladmin ping`.**
  - Pro: paling sederhana.
  - Kontra: berhasil walau akses ditolak, jadi bukan query terautentikasi; tidak memenuhi scope.

### Keputusan 3: akun `root` untuk TCP

- **Option 1 (dipilih): `root@localhost`, `root@127.0.0.1`, dan `root@::1` dengan password yang sama.**
  - Pro: `mysql -h 127.0.0.1 -u root -p` langsung bekerja; tidak ada host wildcard.
  - Kontra: tiga akun terpisah; mengganti password satu tidak mengganti yang lain.
- **Option 2: `root@localhost` dan `root@'%'`.**
  - Pro: dua akun; sama dengan image container resmi.
  - Kontra: `%` menjadi berbahaya begitu bind LAN ada; melanggar prinsip bind dan auth eksplisit.
- **Option 3: hanya `root@localhost` dan name resolve bawaan.**
  - Pro: satu akun.
  - Kontra: membuat akun readiness terjangkau lewat TCP oleh user OS lain (spike baris 10).

### Keputusan 4: variabel persisted

- **Option 1 (dipilih): `--persisted-globals-load=OFF`.**
  - Pro: parameter milik Stackly tidak bisa bergeser; instance tidak bisa dibuat gagal start oleh nilai persisted (spike baris 16, 17).
  - Kontra: `SET PERSIST` tidak bertahan melewati restart.
- **Option 2: biarkan persisted dimuat.**
  - Pro: perilaku MySQL biasa.
  - Kontra: `skip_name_resolve` bisa dimatikan (membuka akun readiness ke TCP), dan `lower_case_table_names` yang persisted mematikan instance.
- **Option 3: `--no-defaults` saat start dan semua settings di argv.**
  - Pro: persisted juga tidak dibaca (spike baris 15), dan format `mysqlCnf` tidak dibutuhkan.
  - Kontra: tidak ada file config yang bisa dilihat pengguna di halaman Configuration; setting teks kelak tetap butuh aturan escape; menyimpang dari rencana spec 0009.

### Keputusan 5: format `my.cnf`

- **Option 1 (dipilih): format baru `mysqlCnf` (1 perubahan core).**
  - Pro: direncanakan spec 0009; escaper teks benar untuk file opsi MySQL dan dibuktikan terhadap `my_print_defaults`; setting teks kelak langsung aman.
  - Kontra: satu varian enum dan `match` baru di modul yang juga disentuh fitur 40 dan 43.
- **Option 2: pakai format yang ada (`natsConf` atau `postgresqlConf`), karena template hari ini hanya memuat integer dan satu blok boolean.**
  - Pro: nol perubahan core.
  - Kontra: nama format menyesatkan, dan teks pertama yang ditambah akan di escape dengan aturan bahasa lain (`''` PostgreSQL tidak sah di file opsi MySQL). Itu jebakan untuk kontributor berikutnya.

### Keputusan 6: penanda data dan kompatibilitas lini

- **Option 1 (dipilih): `dataMarker` `mysql_upgrade_history`, tanpa `lineCompatibility`.**
  - Pro: file ditulis MySQL sendiri di kedua lini; pemeriksaan memakai `check_data_compatibility` yang sudah ada; BR-009 ketat mencegah upgrade di tempat.
  - Kontra: file itu tumbuh dan dibaca paling banyak 4096 byte; formatnya milik MySQL dan bisa berubah di lini baru.
- **Option 2: tanpa penanda, hanya `data_line` registry.**
  - Pro: tidak bergantung pada file MySQL.
  - Kontra: data yang ditukar di luar Stackly tidak terdeteksi; adoption dan restore kelak tidak punya sumber line.
- **Option 3: `lineCompatibility { "8.4": ["9.7"] }`.**
  - Pro: mengikuti jalur upgrade LTS ke LTS yang didukung upstream.
  - Kontra: melanggar PRD 22.6 (upgrade mayor tidak dengan menukar binary tanpa preflight dan prosedur teruji), dan tidak bisa dibalik.

### Keputusan 7: stop

- **Option 1 (dipilih): `gracefulProcess` dengan `SIGTERM`.**
  - Pro: shutdown normal MySQL yang didokumentasikan; tanpa credential; dibuktikan spike (baris 18).
  - Kontra: tidak bisa memilih mode shutdown.
- **Option 2: strategi `command` dengan `mysqladmin shutdown`.**
  - Pro: sama dengan recipe spike lama.
  - Kontra: butuh akun ber hak `SHUTDOWN`, dan command stop tidak bisa membaca secret; berarti memberi hak itu ke akun tanpa password.

### Keputusan 8: binary log

- **Option 1 (dipilih): mati secara default, setting `binaryLog`.**
  - Pro: lebih hemat disk dan lebih cepat untuk development; replication dan PITR di luar scope.
  - Kontra: berbeda dari bawaan MySQL; alat CDC butuh satu setting dan restart.
- **Option 2: bawaan MySQL (menyala, kedaluwarsa 30 hari).**
  - Pro: tanpa kejutan untuk pengguna MySQL.
  - Kontra: folder data tumbuh tanpa manfaat untuk kebanyakan instance lokal.

### Keputusan 9: lini yang didukung

- **Option 1 (dipilih): 8.4 dan 9.7 sejak revision `1.0.0`.**
  - Pro: sama dengan inventory; `versions.json` dibangkitkan dan tidak bisa dipangkas tangan; penolakan line dua arah bisa diuji.
  - Kontra: waktu test native dua kali.
- **Option 2: 8.4 saja.**
  - Pro: lebih cepat.
  - Kontra: butuh mengubah inventory atau aturan codegen, dan AC-10 kehilangan lini kedua yang nyata.

## Rationale

- **Keputusan 1.** Dua kendala Context yang menentukan: secret tidak boleh menetap di file, dan RISK-07 menghargai nol perubahan core. Option 1 dan 2 memenuhi keduanya; Option 1 menang karena hanya satu proses yang selalu keluar sendiri, tidak pernah ada server dengan `root` tanpa password, dan kegagalan terlihat seketika dengan pesan MySQL. Draft pertama spec ini memilih Option 2 karena percaya pada kalimat manual; cross check meminta klaim itu diuji, dan spike putaran kedua membuktikan Option 1 di kedua lini. Option 3 meninggalkan password polos di folder run sepanjang umur instance, dan Option 4 menaruh kriptografi khusus satu vendor di core. Ketergantungan pada perilaku yang tidak tertulis diterima karena artefak dipin dengan digest, setiap artefak baru melewati test native sebelum masuk inventory, kegagalannya tertutup, dan Option 2 tersedia sebagai cadangan yang sudah dibuktikan kalau upstream menutup jalur ini.
- **Keputusan 2 dan 3.** Tanpa `auth_socket` di macOS, satu satunya cara membuat akun yang hanya cocok lewat socket adalah host `localhost` bersama `skip_name_resolve = ON`. Itu memaksa akun `root` ber host IP untuk TCP. Option 1 di keduanya menjaga invariant yang mudah diuji: tidak ada akun tanpa password yang terjangkau lewat TCP.
- **Keputusan 4.** Spike baris 16 menunjukkan bahwa membiarkan persisted dimuat membatalkan Keputusan 2 dan bisa mematikan instance. Menjaga registry tetap benar dan instance tetap bisa start lebih penting daripada kenyamanan `SET PERSIST`; settings yang bertahan punya jalur resmi di Stackly.
- **Keputusan 5.** Spec 0009 sudah menghitung satu perubahan core untuk MySQL, dan halaman Configuration (spec 0029) butuh file config yang nyata. Menghemat perubahan itu dengan meminjam format bahasa lain akan memindahkan risikonya ke kontributor berikutnya.
- **Keputusan 6.** PRD 22.6 dan BR-009 melarang upgrade diam diam; hanya penanda di folder data yang menangkap data yang ditukar di luar Stackly. Brief juga mewajibkan `check_data_compatibility`, bukan pemeriksaan sendiri.
- **Keputusan 7, 8, 9.** Pilihan paling sederhana yang memenuhi scope: sinyal yang didokumentasikan, bawaan yang hemat untuk development, dan lini yang memang sudah ada di inventory.

## Keputusan yang diambil dengan opsi rekomendasi

User mendelegasikan keputusan, jadi daftar ini yang dilaporkan ke lead untuk disetujui bersama spec:

1. Password `root` dipasang di dalam `mysqld --initialize-insecure` lewat SQL dari stdin dan `LOAD_FILE`; server sementara menjadi cadangan (Keputusan 1).
2. Akun `stackly_readiness@localhost` tanpa password, hanya lewat socket (Keputusan 2).
3. Tiga akun `root` (Keputusan 3).
4. `--persisted-globals-load=OFF` (Keputusan 4).
5. Format `mysqlCnf` sebagai satu satunya perubahan core (Keputusan 5).
6. Penanda `mysql_upgrade_history`, BR-009 ketat (Keputusan 6).
7. Stop dengan `SIGTERM`, tanpa capability reload (Keputusan 7).
8. Binary log mati secara default (Keputusan 8).
9. Dua lini, 8.4 dan 9.7 (Keputusan 9).
10. Settings awal: `rootPassword`, `maxConnections`, `innodbBufferPoolSizeMb`, `binaryLog`. Runner up: menambah `sqlMode` dan character set sekarang; ditunda supaya permukaan awal kecil.
11. `secure_file_priv` `NULL` saat berjalan. Runner up: folder impor per instance; ditunda ke Follow-up.
12. Zona waktu `+00:00`, seperti PostgreSQL yang dipatok ke UTC. Runner up: `SYSTEM`.
13. General log dan slow log ditunjuk ke folder logs instance tetapi tidak dideklarasikan di `logs.files`. Runner up: membiarkan bawaan (di folder data).
14. Connection hanya `uri` (TCP); Open In: TablePlus, Sequel Ace, dan `mysql` di Terminal. Runner up: entri socket; ditolak karena URI socket MySQL tidak seragam antar klien.
15. Satu remedy (`readiness_account`). Runner up: remedy untuk folder data terkunci; ditunda sampai pesannya dicatat dari run nyata.
16. Test BR-025 adalah suite native sendiri, bukan test konformitas.
17. `backup: [cold]` dideklarasikan seperti PostgreSQL; alurnya sendiri milik fase 09.
18. `timeouts`: init 180, readiness 120, recovery 600, stop 120 detik; batas langkah init 120 detik.
19. Akun `root` ber host IP tidak diberi `AUDIT_ABORT_EXEMPT` dan `FIREWALL_EXEMPT` (tidak bisa diberikan saat `--initialize`, dan hanya berarti untuk plugin Enterprise).

## Cross check

Dijalankan 2026-11-12 oleh model lain (Opus), hanya membaca. Ia memeriksa manifest terhadap validator dan spec 0009, 0010, 0011, 0034, 0036, dan 0070. Yang ia nyatakan benar: posisi setiap token, aturan `stdin.text`, argumen `/dev/stdin`, `init_port_issues` kosong, pattern `dataMarker` dan `versionCheck`, batas timeout, tidak adanya reload, blok `connection`, `openWith`, `remedies`, dan skenario.

Temuan dan apa yang dilakukan (semua diterapkan dengan rekomendasi, sesuai delegasi keputusan):

| No | Temuan | Tindakan |
| --- | --- | --- |
| 1 | Langkah yang melewati batas waktu berakhir `init_step_timeout` tanpa cuplikan, bukan `init_step_failed` | Init diubah menjadi satu langkah yang gagal dengan exit 1 (`init_step_failed` dengan cuplikan); baris `init_step_timeout` ditambah di *Edge cases* |
| 2 | Server sementara menulis file binary log berisi statement akun | `--disable-log-bin` di langkah init; AC-3 memeriksa tidak ada `binlog.*` |
| 3 | Test dan file yang terlewat: `package_recipe.rs` (test `find_package("mysql")`), `native_runtime_install/harness.rs`, `provider_packages.rs` | Masuk AC-14, langkah 2 dan 4, dan tabel file bersama |
| 4 | Daftar file perubahan core keliru (`secret/delivery` tidak butuh arm; `output_parses`, selector fuzz, `xtask/src/fuzz.rs` terlewat) | Tabel *Perubahan core* dan langkah 1 dibetulkan |
| 5 | Daftar benturan `ensure_not_in_argv` keliru (argv probe tidak diperiksa; aturan 16 byte) | *Catatan per bagian* dan *Edge cases* dibetulkan |
| 6 | Klaim "`--initialize` menolak statement akun" tidak diuji | Diuji (spike baris 24 sampai 29) dan ternyata diterima; Keputusan 1 berubah ke init satu langkah |
| 7 | AC-4, AC-5, AC-6, AC-12 tidak bisa diuji seperti tertulis | Query akun, pemeriksaan port `Auto`, jenis fd `lsof`, dan letak `.my.cnf` umpan ditulis persis |
| 8 | Server sementara bisa menjadi proses yatim | Hilang bersama desain server sementara; `--initialize` selalu keluar sendiri |
| 9 | Setup test AC-10 kurang jelas | AC-10 menyebut hanya ikatan runtime yang diganti |
| 10 | Urutan check `compatibility.json` dan arti `adoption` | AC-16 dan contoh diurutkan; arti `adoption` dijelaskan |
