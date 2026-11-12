# 0042. Provider MySQL

**Date**: 2026-11-12
**Status**: In Progress

## Summary

Spec ini memutuskan paket provider resmi `official/mysql` untuk MySQL 8.4 dan 9.7. Data dibuat dengan satu proses `mysqld --initialize-insecure` dari runtime yang cocok, yang di proses yang sama memberi password ke akun `root` dan membuat akun `stackly_readiness` lewat SQL dari stdin, sehingga tidak pernah ada server berjalan dengan `root` tanpa password. Ready berarti query `SELECT 1` dijawab lewat socket privat instance oleh akun tanpa hak itu, bukan sekadar port terbuka. Port, alamat, socket, dan semua path dikirim lewat argumen `mysqld`, variabel `SET PERSIST` tidak dimuat saat start, dan data dari lini lain ditolak sebelum proses dijalankan lewat pemeriksaan data yang sama dengan PostgreSQL. Semua ini muat di manifest `stackly/v1alpha1` dengan satu perubahan core: format template `mysqlCnf`.

## Requirements

**User stories**:

- Sebagai developer, saya ingin membuat database MySQL 8.4 atau 9.7 lokal yang langsung aman (root dengan password, hanya loopback), supaya saya tidak perlu menjalankan `mysqld --initialize` dan `ALTER USER` sendiri.
- Sebagai developer, saya ingin status ready berarti server benar benar menjawab query, supaya aplikasi saya tidak gagal konek saat Stackly bilang siap.
- Sebagai developer, saya ingin dua instance MySQL dengan port dan data berbeda tidak pernah saling melihat data, dan data saya tidak pernah dijalankan (atau di upgrade diam diam) oleh binary lini lain (BR-009).
- Sebagai developer fitur 51, 52, 57, dan 58, saya ingin provider MySQL yang hanya berupa paket di atas engine fitur 9, supaya backup, adoption, dan rebind tidak butuh kode khusus MySQL di daemon.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-081):

- **AC-1** (paket dan catalog; REQ-055, bagian manifest REQ-029): paket `providers/mysql` (ID `official/mysql`, software `mysql`, revision `1.0.0`, category `database`) berisi persis file di *Paket*. `cargo xtask provider check providers/mysql` mencetak satu baris `ok official/mysql 1.0.0 <manifest_sha256>` (jadi `init_port_issues` kosong). `cargo xtask codegen --check` lolos dengan `versions.json` yang dibangkitkan dari inventory dan memuat tepat dua artefak `available`: `mysql-8.4.11-macos-aarch64` (line `8.4`) dan `mysql-9.7.2-macos-aarch64` (line `9.7`). Render `my.cnf`, `init_plan`, dan `run_plan` untuk context golden spec 0009 sama byte demi byte dengan file golden di `crates/core/tests/golden/provider/mysql/`. Di commit yang membuktikan lifecycle ujung ke ujung untuk kedua lini (langkah 11), entri MySQL di `providers/catalog.toml` pindah dari `[[planned]]` ke `[[provider]]` (mark `My`, tile `teal`), `revision` dan `published_at` catalog naik, `cargo xtask catalog check --update` mencatatnya di `providers/catalog-revisions.json`, lalu `cargo xtask provider freeze providers/mysql` menyalin revision itu ke corpus kompatibilitas.
- **AC-2** (format `mysqlCnf`): format template `mysqlCnf` mengeluarkan teks sebagai `"..."` dengan `\` menjadi `\\` dan `"` menjadi `\"`, integer sebagai desimal, dan boolean sebagai `ON` atau `OFF`. Teks yang memuat karakter kontrol (U+0000 sampai U+001F, U+007F sampai U+009F) ditolak dengan `unrepresentable_value` yang menyebut field atau token, tanpa nilainya. Tidak ada parse ulang. Untuk teks `a"b\c d # e ü 空 'q' ; z` yang dirender sebagai nilai `general_log_file` di bawah `[mysqld]`, `my_print_defaults --defaults-file=<file> mysqld` dari runtime 8.4.11 mencetak `--general_log_file=` diikuti teks yang persis sama. `TemplateFormat::from_code("mysqlCnf")` mengembalikan varian baru, pesan validasi format menyebut `mysqlCnf`, dan `render_secret_placeholder` memakai escaper yang sama untuk format ini.
- **AC-3** (init; BR-008): init sebuah instance menjalankan langkah dalam urutan *Inisialisasi data*: folder dibuat, file `root-password` (0600) dibuat di `<run>/secrets/`, lalu satu proses `mysqld --initialize-insecure` membuat folder data dan menjalankan SQL akun dari stdin. Setelah init, berhasil atau gagal, file `root-password` tidak ada lagi dan tidak ada proses `mysqld` yang tersisa. Selama init tidak ada port TCP dan tidak ada socket yang dibuka, dan sesudahnya folder data tidak memuat file `binlog.*`. Kalau SQL akun gagal (dipaksa test lewat salinan paket yang satu barisnya diganti statement yang salah), `mysqld` keluar 1, init `failed` dengan `init_step_failed` dan cuplikan error MySQL, dan instance tidak pernah di start. Kalau folder data sudah berisi file saat `--initialize-insecure` berjalan (dipaksa lewat kait test sesudah pemeriksaan isi spec 0020), `mysqld` keluar 1, file yang ada tidak berubah, dan init `failed` dengan `init_step_failed`.
- **AC-4** (auth; REQ-055 bagian bind dan auth, PRD 22.6, VAL-018): setelah init, `SELECT CONCAT(user, '@', host) FROM mysql.user WHERE account_locked = 'N'` mengembalikan tepat empat akun: `root@localhost`, `root@127.0.0.1`, `root@::1` (ketiganya dengan password dari Keychain), dan `stackly_readiness@localhost`. Daftar hak `SHOW GRANTS` ketiga akun `root` sama, kecuali `AUDIT_ABORT_EXEMPT` dan `FIREWALL_EXEMPT` yang hanya dimiliki `root@localhost`. `root` tanpa password ditolak lewat socket dan lewat TCP, dan dengan nilai secret diterima di keduanya. `stackly_readiness` ditolak lewat TCP, hanya punya `GRANT USAGE ON *.*`, gagal menjalankan `CREATE DATABASE x`, dan dibatasi `MAX_USER_CONNECTIONS 3`. Password yang memuat spasi, `'`, `"`, `\`, dan huruf Unicode (misalnya `pa'ss"w\ord ü空 x`) bisa login lewat socket dan TCP. Nilai password tidak pernah muncul di argv proses mana pun, di spool, di log daemon, atau di file config.
- **AC-5** (endpoint dan parameter milik Stackly): `mysqld` hanya listen di host dan port endpoint `client` dan di socket `<run>/mysql.sock`; `lsof` tidak menunjukkan listener lain (X Protocol dan admin interface mati). `port`, `bind_address`, `socket`, `datadir`, `tmpdir`, `pid_file`, `skip_name_resolve`, `secure_file_priv`, `general_log_file`, dan `slow_query_log_file` datang dari argv. Setelah `SET PERSIST max_connections = 77` dan `SET PERSIST_ONLY skip_name_resolve = OFF` lalu stop dan start, `@@max_connections` sama dengan nilai settings, `@@skip_name_resolve` bernilai 1, readiness lolos, dan `stackly_readiness` tetap ditolak lewat TCP. Untuk endpoint `client` ber mode port `Auto`, sesudah start `instance.get` menunjukkan port konkret dengan `bound` benar, dan `@@port` sama dengan port itu.
- **AC-6** (path terisolasi; REQ-055 bagian path): pada instance yang berjalan, `@@datadir` ada di folder data instance, `@@tmpdir` sama dengan folder temp instance, `@@general_log_file` dan `@@slow_query_log_file` ada di folder logs instance, `@@pid_file` dan `@@socket` ada di folder run instance, `@@secure_file_priv` `NULL`, dan `@@log_error` `stderr`. Setiap file biasa yang dibuka proses `mysqld` untuk ditulis (`lsof -a -p <pid> -F fatn`, entri ber fd angka dengan mode akses `w` atau `u` dan jenis `REG`) ada di dalam folder data, temp, logs, atau run instance itu; file yang hanya dibaca atau di map (binary, dylib, cache dyld) tidak diperiksa. Satu pengecualian yang sempit, dengan path persis: `instance.lock` di folder instance, yang dibuka Stackly dan diwariskan ke `mysqld` di fd 3 (`3u REG <instance>/instance.lock`), karena AC-13 justru menuntut lock itu dipegang. Baris error log MySQL (dengan cap waktu UTC) terbaca lewat `instance.logs_read`.
- **AC-7** (readiness; REQ-055 bagian readiness, PRD 22.6): readiness memakai probe `exec` yang menjalankan `mysql` dari runtime instance lewat socket instance sebagai `stackly_readiness` dengan query `SELECT 1`, dengan argv persis di *Readiness dan health*. Instance baru `ready` setelah query itu berhasil. Saat port TCP tetap menerima koneksi tetapi akun dikunci (`ALTER USER 'stackly_readiness'@'localhost' ACCOUNT LOCK`), readiness menjadi `not_ready` setelah 2 kegagalan berturut turut dengan detail yang memuat pesan MySQL (`Access denied for user 'stackly_readiness'@'localhost'`), dan kembali `ready` setelah `ACCOUNT UNLOCK`.
- **AC-8** (stop; REQ-055 bagian shutdown): stop normal mengirim `SIGTERM` ke `mysqld` yang identitasnya terverifikasi. Dengan satu sesi klien yang sedang menjalankan `SELECT SLEEP(600)`, stop selesai dalam 30 detik (di bawah `stopSeconds` 120) dan instance `stopped`, tanpa `SIGKILL`. File pid, file socket, dan file lock socket hilang, log stop memuat `Shutdown complete`, dan log start berikutnya tidak memuat `Database was not shutdown normally`.
- **AC-9** (isolasi sentinel; AC-081 tanpa restore): untuk setiap lini, dua instance MySQL dengan folder data dan port berbeda masing masing mendapat satu baris sentinel yang berbeda di tabel `stackly_sentinel.marker`. Setelah keduanya di stop lalu di start lagi, query lewat port TCP dan lewat socket setiap instance hanya mengembalikan sentinel miliknya, dan sentinel instance lain tidak ada. Satu test tambahan menjalankan satu instance 8.4 dan satu instance 9.7 berdampingan dengan hasil yang sama.
- **AC-10** (preflight kompatibilitas; BR-009, AC-081 bagian "runtime tidak dapat direbind ke data yang tidak kompatibel"): paket mendeklarasikan `compatibility.dataMarker` dengan `file: mysql_upgrade_history` dan pattern di *Manifest*, tanpa `lineCompatibility`. Instance yang diinisialisasi dengan runtime 8.4 lalu diikat ke runtime 9.7 (lewat penulisan registry khusus test yang hanya mengganti ikatan runtime dan membiarkan `data_line` tetap `8.4`, seperti `apps/daemon/tests/postgresql/mismatch.rs`) gagal start dengan `supervisor.data_incompatible` (`outcome` `incompatible`, `data` `8.4`, `runtime` `9.7`) sebelum spawn: tidak ada observasi proses baru, tidak ada proses `mysqld`, dan byte `mysql_upgrade_history` serta `mysql.ibd` tidak berubah (tidak ada upgrade di tempat). Arah 9.7 ke 8.4 sama. `mysql_upgrade_history` yang dihapus menghasilkan `supervisor.data_unavailable` (reason `the data marker is missing`, spec 0030 AC-4), dan yang versinya diubah menjadi `8.0.40` menghasilkan `marker_mismatch`. `stackly_core::instance::rebind::check` untuk pasangan yang sama memberi outcome yang sama, karena keduanya memanggil `check_data_compatibility`; paket dan daemon tidak punya pemeriksaan lain.
- **AC-11** (data yang sudah ada selalu ditolak; REQ-055 tanpa adoption): tidak ada jalur yang membuat instance di atas data yang sudah ada. Create yang folder datanya berisi salinan folder data MySQL 8.4 yang sah (disisipkan lewat kait test spec 0020 AC-8) ditolak dengan `instance.data_not_empty` sebelum langkah `init`: nol spawn, dan setiap file salinan itu sama byte demi byte sesudahnya. Manifest tidak punya langkah, setting, atau capability yang menerima folder data dari luar.
- **AC-12** (tool dari runtime, env tertutup; PRD 22.6): setiap program di plan init, run, dan probe adalah `ExecutablePath` di dalam root runtime instance (`bin/mysqld`, `bin/mysql`), dan argumen pertama setiap argv adalah `--no-defaults` atau `--defaults-file=<config instance>/my.cnf`. Dengan daemon yang dijalankan memakai `PATH` berisi folder umpan (`mysql` dan `mysqld` palsu yang menulis file penanda), create, start, readiness, dan stop berhasil dan file penanda tidak pernah dibuat. Dengan file `.my.cnf` berisi opsi yang tidak dikenal di folder temp instance (yang menjadi `HOME` proses anak, ditulis test sesudah create), start, readiness, dan stop tetap berhasil.
- **AC-13** (lifecycle lewat daemon per lini): lewat daemon untuk MySQL 8.4 dan 9.7 masing masing: `instance.create` dengan secret `rootPassword` bangkitan, init sukses dengan `data.initializedLine` di `instance-manifest.json` sama dengan line runtime, `instance.start` membuat lifecycle `running` dengan readiness `not_ready` lalu `ready`, `instance.get_detail` memuat satu connection `uri` tersedia yang `masked` nya URI absolut, baris yang ditulis sebelum stop terbaca lagi setelah start berikutnya, `instance.restart` memberi PID baru, dan `instance.stop` membuat `stopped`. Setelah `maxConnections` diubah lewat alur edit config spec 0029 dan instance direstart, `@@max_connections` sama dengan nilai baru. Dengan `binaryLog: true`, `@@log_bin` bernilai 1; dengan default, 0. Daemon yang dibunuh `SIGKILL` saat instance `ready` lalu dijalankan lagi mengadopsi `mysqld` yang sama (PID dan waktu start sama), karena `mysqld` memegang lock instance di fd 3.
- **AC-14** (konformitas, spike, dan install dari paket; AC-029, REQ-029, spec 0003, spec 0018 AC-20): `apps/daemon/tests/provider_conformance.rs` punya `mysql_8_4_passes_conformance` dan `mysql_9_7_passes_conformance` (`#[ignore = "native: ..."]`, `covers = ["AC-029", "REQ-029"]`) yang lulus untuk skenario `default` dan `binlog`. Worker spike membangun init, start, ready, versi, dan stop MySQL dari paket lewat `PackageRecipe`: struct `MySql` dan penanda `// provisional: F42` hilang dari `xtask/src/runtime/spike/providers.rs`, test unit di `xtask/src/runtime/spike/package_recipe.rs` yang kini menuntut `find_package("mysql")` kosong diubah menjadi menuntut paket itu ditemukan, dan kedua test MySQL di `xtask/tests/native_runtime.rs` lulus dari paket. `mysql_8_4_installs_from_the_cache` dan `mysql_9_7_installs_from_the_cache` di `apps/daemon/tests/native_runtime_install.rs` menjadi test sungguhan (bukan `blocked_line_test!`), dengan konstanta lini `MYSQL_8_4` dan `MYSQL_9_7` di `apps/daemon/tests/native_runtime_install/harness.rs`. `protocols/fixtures/instance/index.json` mendapat entri `create-form-mysql.json`, dan `cargo test -p xtask` lulus tanpa mengubah `PROVIDER_WORDS` (kata `mysql` sudah ada).
- **AC-15** (version check): manifest mendeklarasikan `runtime.versionCheck { args: ["--no-defaults", "--version"], pattern: "Ver ([0-9]+\\.[0-9]+\\.[0-9]+) for ", timeoutSeconds: 10 }`, dan exec check install spec 0018 lolos untuk kedua artefak dengan capture group sama dengan `version` artefak (`8.4.11`, `9.7.2`). Pattern dicari per baris tanpa jangkar, karena baris `mysqld --version` diawali path binary (`<root>/bin/mysqld  Ver 8.4.11 for macos15 on arm64 (MySQL Community Server - GPL)`).
- **AC-16** (evidence; BR-025): setiap test native fitur ini memasang guard evidence `native` spec 0002 dengan `covers` ID PRD yang dibuktikannya (REQ-055, AC-081, BR-009, BR-008). Untuk setiap artefak, satu test menulis evidence `native` dengan `covers` BR-025, subject `{ provider: mysql, version, digest, source: managed }` (`version` dari `SELECT VERSION()` pada server yang berjalan, `digest` dari `ArtifactRecord.sha256`), status `PASS`, dan `compatibility.json` di `artifact_dir()` dengan check (urut abjad, seperti `apps/daemon/tests/postgresql/compat.rs`) `adoption`, `init`, `isolation`, `line_mismatch_refusal`, `readiness`, `restart`, `start`, dan `stop`. `adoption` di sini berarti daemon baru mengambil kembali proses yang sedang berjalan (AC-13), bukan adoption data luar fitur 57. Evidence dihasilkan dari `provider.yaml` final yang masuk catalog. Artefak yang tidak ada di cache runtime membuat test `BLOCKED` dengan alasan `artifact <id> is not in the runtime cache` dan owner `@ojiepermana`, tanpa unduhan.
- **AC-17** (handover; spec 0030 AC-8): `apps/daemon/tests/native_launchd.rs` mendapat `mysql_8_4_survives_planned_restart_and_sigkill_and_returns_after_kickstart_and_bootout` dan padanan 9.7 nya. Entri `HandoverProof::new("official/mysql", "1", "<ID evidence>")` masuk `HANDOVER_PROVEN` hanya di commit yang membawa record `PASS` kedua test itu. Tanpa sesi GUI test itu `BLOCKED`, entri tidak ditambah, harness melaporkan `handover: unproven`, dan instance MySQL memakai jalur `stop_restore` saat restart daemon terencana.
- **AC-18** (connection, Open In, dan remedy; spec 0021, 0070, 0034): manifest membawa blok `connection`, `openWith`, dan `remedies` di *Manifest*. `instance.get_detail` mengembalikan tiga `openActions` (`tableplus`, `sequel_ace`, `mysql`) tanpa nilai secret, dan aksi `mysql` memakai `bin/mysql` dari runtime instance. Setelah `DROP USER 'stackly_readiness'@'localhost'`, `doctor.run` untuk instance itu memuat remedy `readiness_account` dengan perintah yang sudah dirender, dan Stackly tidak menjalankan apa pun.

Status test semua AC mulai dari `NOT_RUN`. Lulus di fixture bukan evidence native, dan artefak, sesi GUI, atau signing yang tidak tersedia dicatat `BLOCKED`.

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md): password `root` dipasang di dalam proses `mysqld --initialize-insecure` itu sendiri, yang membaca SQL dari stdin dan membaca password dari file sekali pakai lewat `LOAD_FILE`; readiness lewat probe `exec` yang menjalankan `mysql` milik runtime sebagai akun `stackly_readiness@localhost` tanpa password yang hanya terjangkau lewat socket privat (karena `skip_name_resolve` menyala); tiga akun `root` (`localhost`, `127.0.0.1`, `::1`); parameter milik Stackly di argv dan `--persisted-globals-load=OFF`; `SIGTERM` untuk stop; tanpa capability reload; penanda data `mysql_upgrade_history` dengan BR-009 ketat; binary log mati secara default; format `mysqlCnf` untuk `my.cnf`; dan kedua lini inventory (8.4 dan 9.7) didukung sejak revision pertama.

Paket MySQL hanya data (manifest, schema, template, skenario). Satu satunya perubahan engine di core adalah format template `mysqlCnf`, yang sudah direncanakan spec 0009. Itu ukuran RISK-07 fitur ini: **1 perubahan core**, tanpa probe baru, primitive baru, method IPC, atau migrasi.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Skill async relevan hanya untuk test native yang menunggu readiness dan event; perubahan core di sini sync dan tanpa IO.

## Feature design

### Batas fitur

- **Di dalam**: paket `providers/mysql`, format template `mysqlCnf`, golden test, suite native lewat daemon untuk 8.4 dan 9.7, test konformitas native per lini, spike dan install dari paket (termasuk menghapus recipe tulisan tangan), evidence kompatibilitas, entri catalog, fixture form, test native launchd, dan remedy doctor.
- **Di luar**: backup logical dan restore (fase 09, fitur 51 dan 52), adoption data luar (fitur 57), rebind (fitur 58), upgrade lini 8.4 ke 9.7 (tidak pernah dengan menukar binary; butuh prosedur migrasi teruji, PRD 22.6), replication, PITR, group replication, X Protocol, TLS yang dikelola Stackly, bind LAN, akun dan database buatan Stackly selain yang disebut di sini, dan tampilan slow log di GUI.

### Paket

```text
providers/mysql/
├── provider.yaml
├── versions.json                 # dibangkitkan cargo xtask codegen dari runtimes/inventory.toml
├── schemas/
│   └── settings.schema.json
├── templates/
│   └── my.cnf.tmpl               # format mysqlCnf
├── tests/
│   └── scenarios.yaml            # dibaca validator, harness, dan spike
├── README.md                     # tidak dibaca loader
└── LICENSE-NOTICES.md            # tidak dibaca loader; wajib (spec 0038)
```

`README.md` menjelaskan akun `stackly_readiness` (jangan dihapus atau dikunci), tiga akun `root` yang berbagi password awal, parameter yang dimiliki Stackly di argv, bahwa `my.cnf` dikelola Stackly dan nilainya diubah di halaman Configuration instance, bahwa suntingan tangan di sana adalah drift spec 0029 (start berikutnya gagal `config.drift_detected`, suntingannya tetap ada di file, dan instance tidak start sampai drift diselesaikan), bahwa `SET PERSIST` tidak bertahan melewati restart, dan bahwa binary log mati secara default. Kalimat lama "hand edits there are lost" sudah diganti sebelum paket terbit (`a046da9f`, lihat catatan di *Template config*). `LICENSE-NOTICES.md` mengikuti bentuk paket PostgreSQL: paket `MIT OR Apache-2.0`, runtime `GPL-2.0-only WITH Universal-FOSS-exception-1.0` dari unduhan upstream, dan library bawaan artefak seperti tercatat di `runtimes/inventory.toml`.

Paket dibuat lewat `cargo xtask provider new mysql` lalu diubah mengikuti urutan *Dari contoh ke paket resmi* spec 0036.

### Manifest

```yaml
apiVersion: stackly/v1alpha1
kind: Provider
metadata:
  id: official/mysql
  software: mysql
  displayName: MySQL
  revision: "1.0.0"
  category: database
  description: MySQL relational database. Each instance has its own data folder, a password protected root account, a loopback port, and a private Unix socket.
compatibility:
  platforms:
    - { os: macos, arch: aarch64 }
  dataMarker:
    file: mysql_upgrade_history
    pattern: '.*"version":"([0-9]+\.[0-9]+)\.[0-9]+[^"]*"[^\]]*\]\}'
runtime:
  executable: bin/mysqld
  executionMode: foreground
  versionCheck:
    args: ["--no-defaults", "--version"]
    pattern: 'Ver ([0-9]+\.[0-9]+\.[0-9]+) for '
    timeoutSeconds: 10
capabilities:
  multiInstance: true
  persistence: true
  backup: [cold]
resources:
  endpoints:
    client: { transport: tcp, purpose: client, defaultHost: "127.0.0.1", defaultPort: 3306 }
    socket: { transport: unix, purpose: client, socketName: "mysql.sock" }
configFiles:
  myCnf:
    template: templates/my.cnf.tmpl
    format: mysqlCnf
    destination: "${instance.config}/my.cnf"
lifecycle:
  initialize:
    - { primitive: createManagedDirectories }
    - { primitive: materializeSecret, secret: rootPassword, destination: root-password }
    - primitive: runExecutable
      executable: "${runtime.executable}"
      args:
        - "--no-defaults"
        - "--initialize-insecure"
        - "--disable-log-bin"
        - "--basedir=${runtime.root}"
        - "--datadir=${instance.data}"
        - "--tmpdir=${instance.temp}"
        - "--replica-load-tmpdir=${instance.secrets}"
        - "--init-file=/dev/stdin"
      cwd: "${instance.temp}"
      timeoutSeconds: 120
      stdin:
        text: |
          SET @pw = CONVERT(LOAD_FILE(CONCAT(@@replica_load_tmpdir, '/root-password')) USING utf8mb4);
          SET @sql = IF(@pw IS NULL OR CHAR_LENGTH(@pw) < 8, 'SELECT stackly_root_password_is_missing()', CONCAT('ALTER USER ''root''@''localhost'' IDENTIFIED BY ', QUOTE(@pw)));
          PREPARE stmt FROM @sql;
          EXECUTE stmt;
          SET @sql = CONCAT('CREATE USER ''root''@''127.0.0.1'' IDENTIFIED BY ', QUOTE(@pw));
          PREPARE stmt FROM @sql;
          EXECUTE stmt;
          SET @sql = CONCAT('CREATE USER ''root''@''::1'' IDENTIFIED BY ', QUOTE(@pw));
          PREPARE stmt FROM @sql;
          EXECUTE stmt;
          DEALLOCATE PREPARE stmt;
          GRANT ALL PRIVILEGES ON *.* TO 'root'@'127.0.0.1', 'root'@'::1' WITH GRANT OPTION;
          GRANT PROXY ON ''@'' TO 'root'@'127.0.0.1', 'root'@'::1' WITH GRANT OPTION;
          CREATE USER 'stackly_readiness'@'localhost' IDENTIFIED BY '' WITH MAX_USER_CONNECTIONS 3 COMMENT 'Stackly readiness probe. Dropping or locking this account stops the instance from becoming ready.';
  start:
    executable: "${runtime.executable}"
    args:
      - "--defaults-file=${instance.config}/my.cnf"
      - "--persisted-globals-load=OFF"
      - "--basedir=${runtime.root}"
      - "--datadir=${instance.data}"
      - "--tmpdir=${instance.temp}"
      - "--bind-address=${endpoint.client.host}"
      - "--port=${endpoint.client.port}"
      - "--socket=${endpoint.socket.socket}"
      - "--pid-file=${instance.run}/mysqld.pid"
      - "--mysqlx=OFF"
      - "--skip-name-resolve=ON"
      - "--secure-file-priv=NULL"
      - "--general-log-file=${instance.logs}/general.log"
      - "--slow-query-log-file=${instance.logs}/slow.log"
    cwd: "${instance.data}"
  stop: { strategy: gracefulProcess, signal: term }
readiness:
  probe:
    type: exec
    command:
      executable: "${runtime.executables.mysql}"
      args:
        - "--no-defaults"
        - "--protocol=socket"
        - "--socket=${endpoint.socket.socket}"
        - "--user=stackly_readiness"
        - "--connect-timeout=2"
        - "--batch"
        - "--skip-column-names"
        - "--execute=SELECT 1"
      cwd: "${instance.temp}"
    successExitCodes: [0]
  intervalMilliseconds: 500
  attemptTimeoutMilliseconds: 3000
health:
  intervalSeconds: 10
  attemptTimeoutMilliseconds: 3000
  failureThreshold: 3
timeouts: { initSeconds: 180, readinessSeconds: 120, recoverySeconds: 600, stopSeconds: 120 }
backup:
  cold: { paths: [data] }
permissions:
  managedFilesystem: [data, config, logs, temp, run]
  executeRuntimeBinary: true
  additionalExecutables: { mysql: bin/mysql }
connection:
  - name: uri
    title: Connection string
    template: "mysql://root:${secret.rootPassword}@${endpoint.client.address}/"
openWith:
  - name: tableplus
    title: TablePlus
    kind: app
    app:
      bundleIds: ["com.tinyapp.TablePlus"]
      url: "mysql://root@${endpoint.client.address}/"
    copySecret: rootPassword
  - name: sequel_ace
    title: Sequel Ace
    kind: app
    app:
      bundleIds: ["com.sequel-ace.sequel-ace"]
      url: "mysql://root@${endpoint.client.address}/"
    copySecret: rootPassword
  - name: mysql
    title: mysql
    kind: terminal
    terminal:
      executable: "${runtime.executables.mysql}"
      args: ["--no-defaults", "--protocol=socket", "--socket=${endpoint.socket.socket}", "--user=root", "--password"]
    copySecret: rootPassword
remedies:
  - name: readiness_account
    when:
      readinessDetailContains: "'stackly_readiness'@'localhost'"
    title: "The readiness account stackly_readiness is missing or changed"
    steps:
      - text: "Stackly checks that this instance is ready by signing in as the account stackly_readiness, which the instance’s init created. That sign in fails now."
      - text: "Run this in Terminal to create the account again and unlock it. mysql asks for the root password, and Stackly never puts it in the command."
        command: '${runtime.executables.mysql} --no-defaults --protocol=socket --socket=${endpoint.socket.socketDir}/mysql.sock --user=root --password --execute="CREATE USER IF NOT EXISTS ''stackly_readiness''@''localhost'' IDENTIFIED BY '''' WITH MAX_USER_CONNECTIONS 3; ALTER USER ''stackly_readiness''@''localhost'' IDENTIFIED BY '''' ACCOUNT UNLOCK"'
      - text: "The instance becomes ready again on its next readiness check."
```

Catatan per bagian:

- `--no-defaults` dan `--defaults-file` wajib menjadi argumen pertama, karena MySQL hanya mengenalinya di posisi itu. Dengan salah satunya, `/etc/my.cnf`, `~/.my.cnf`, dan file opsi lain tidak pernah dibaca (AC-12).
- Init sengaja tidak memakai `my.cnf`: settings fitur ini tidak mempengaruhi isi folder data awal, dan init tanpa `renderConfig` tidak butuh port (aturan *Pemeriksaan port saat initialize* spec 0009). `my.cnf` ditulis render sebelum spawn (`RunPlan.render`) sebelum setiap start.
- Init hanya satu proses `mysqld --initialize-insecure`, yang tidak membuka port atau socket dan selalu keluar sendiri. `--disable-log-bin` mencegah statement akun tercatat di file binary log di folder data.
- `--replica-load-tmpdir` di langkah init hanya dipakai sebagai cara memberi tahu SQL di mana folder `secrets/` instance berada (dibaca sebagai `@@replica_load_tmpdir`). Variabel itu milik replication, yang tidak dipakai; saat `--initialize`, MySQL mengosongkan `secure_file_priv`, jadi variabel itu tidak bisa dipakai untuk tujuan ini. Start tidak membawa argumen ini.
- `--mysqlx=OFF` dan `--socket` di start wajib, karena tanpa itu MySQL membuat `/tmp/mysqlx.sock` dan `/tmp/mysql.sock`, yaitu path global yang akan bentrok antar instance.
- `logs` ada di `managedFilesystem` karena argv menunjuk file general log dan slow log ke folder logs instance. Keduanya mati secara default dan tidak dideklarasikan di `logs.files`; kalau pengguna menyalakannya lewat `SET GLOBAL`, file nya tetap di dalam instance. Error log tidak diberi `--log-error`, jadi keluar di stderr dan ditangkap spool spec 0010 serta store spec 0027.
- `capabilities.reload` tidak ada, jadi `lifecycle.reload` juga tidak ada dan semua settings ber `apply: restart`. `SIGHUP` di MySQL tidak membaca ulang file opsi.
- Blok `connection` hanya punya entri `uri`. URI socket MySQL tidak punya bentuk yang dipahami bersama oleh library klien, jadi entri socket tidak ditulis; path socket tetap tampil sebagai endpoint.
- `remedies` memakai `${endpoint.socket.socketDir}/mysql.sock`, karena posisi remedy hanya menerima token `socketDir` (matriks posisi spec 0009).
- Tidak ada argumen di plan mana pun yang memuat secret. `ensure_not_in_argv` spec 0011 memeriksa argv init saat init dan argv start di setiap start (argv probe tidak diperiksa, karena probe tidak menyelesaikan secret). Ia menolak password yang sama persis dengan satu argumen utuh atau dengan nilai sesudah `=` (saat init misalnya `/dev/stdin`; saat start misalnya `127.0.0.1`, yang lolos init lalu membuat setiap start gagal), dan password 16 byte ke atas yang merupakan potongan dari sebuah argumen (misalnya `--initialize-insecure` atau potongan path instance). Itu residual yang dinyatakan; password bangkitan (32 karakter alfanumerik) tidak pernah terkena.

### Settings schema

| Field | Tipe | Default | Batas | `apply` | Visibility | Dipakai di |
| --- | --- | --- | --- | --- | --- | --- |
| `rootPassword` | string, `secret: true`, `printableOnly: true` | tidak ada (wajib; create memakai `generate` kecuali pemanggil memberi nilai) | panjang 8 sampai 128 | `immutableAfterInit` | basic | `materializeSecret` untuk SQL akun |
| `maxConnections` | integer | 151 | 10 sampai 10000 | `restart` | advanced | `max_connections` |
| `innodbBufferPoolSizeMb` | integer | 128 | 64 sampai 65536 | `restart` | advanced | `innodb_buffer_pool_size` (dalam MB) |
| `binaryLog` | boolean | `false` | | `restart` | advanced | `log_bin` atau `disable_log_bin` |

`title` dan `description` (bahasa Inggris, ditulis untuk pengguna): `Root password` ("Password of the root account, set once when the instance is created."), `Max connections` ("Most client connections the server accepts at once."), `InnoDB buffer pool (MB)` ("Memory the server reserves for caching table and index data."), dan `Binary log` ("Keeps a log of every change. Change data capture tools need it. Uses more disk."). Root `title` `MySQL settings`, `required: ["rootPassword"]`, `additionalProperties: false`.

Nilai bawaan di luar settings sengaja tetap: nama akun `root`, character set `utf8mb4` dan collation bawaan lini itu, zona waktu `+00:00`, dan semua parameter milik Stackly. Masing masing bisa menjadi setting lewat update spec ini secara additive (Follow-up).

### Template config

`templates/my.cnf.tmpl` (format `mysqlCnf`):

```text
{{! Rendered before every start. }}
# Managed by Stackly. Change these values on the instance's Configuration page.
# Stackly does not overwrite an edit made here: it reports the file as changed
# and the instance does not start until you resolve it.
# Port, bind address, socket, data folder, temp folder, and log files are set
# on the mysqld command line by Stackly and cannot be changed here.
# Variables saved with SET PERSIST are not loaded when the instance starts.
[mysqld]
max_connections = {{ settings.maxConnections }}
innodb_buffer_pool_size = {{ settings.innodbBufferPoolSizeMb }}M
default_time_zone = "+00:00"
{{#if settings.binaryLog}}
log_bin = binlog
{{else}}
disable_log_bin
{{/if}}
```

- **Catatan 2026-11-12 (sudah dibetulkan sebelum terbit)**: komentar template yang pertama dibangun berbunyi "Edits to this file are lost on the next start.", dan README memuat kalimat yang sama ("hand edits there are lost"). Verify menemukan itu salah: `my.cnf` yang disunting tangan adalah drift (spec 0029). Start berikutnya gagal `config.drift_detected` (`config files changed outside stackly: my.cnf`), suntingan itu tetap ada di file, dan Stackly tidak menimpanya sampai kamu menyelesaikan drift (baris *Edge cases* "`my.cnf` disunting tangan" sudah benar sejak awal). Lead memutuskan perbaikannya masuk revision `1.0.0`, karena saat itu paket belum dibekukan dan belum terbit. Commit `a046da9f` mengganti komentar itu dengan tiga baris `#` di listing di atas, mengganti kalimat README, dan menulis ulang golden render dan plan MySQL. Listing di atas menyalin isi paket final apa adanya. Paket yang dibekukan (`crates/core/tests/fixtures/compat/mysql/1.0.0/`), evidence kompatibilitas `01a1cef9-f323-7534-81ff-caad7326c723`, dan catalog revision 16 semuanya memakai paket yang sudah dibetulkan, dengan `manifest_sha256` `77a41842621898d0e4ec378ae2cea628a5de122444b08862ab45e2d83f7a950c`. Tidak ada revision `1.0.0` berkomentar lama yang pernah terbit.
- `innodb_buffer_pool_size = 128M` keluar tanpa kutip, bentuk yang diterima MySQL. `log_bin = binlog` membuat file `binlog.NNNNNN` di folder data.
- Tidak ada path dan tidak ada teks dari pengguna di file ini, jadi path dengan spasi (`Application Support`) tidak pernah butuh escape. Escaper teks `mysqlCnf` tetap didefinisikan dan diuji (AC-2), supaya setting teks yang ditambah kelak langsung aman.

### Format `mysqlCnf`

| Jenis nilai | Keluaran |
| --- | --- |
| Teks | `"` + isi + `"`, dengan `\` menjadi `\\` dan `"` menjadi `\"`; karakter lain apa adanya sebagai UTF-8 |
| Teks dengan karakter kontrol (U+0000 sampai U+001F, U+007F sampai U+009F) | ditolak, `unrepresentable_value` dengan nama field atau token, tanpa nilainya |
| Integer | desimal |
| Boolean | `ON`, `OFF` |

Tag blok dan tag output diterima (`allows_tags` benar). Tanpa parse ulang, seperti `natsConf` dan `postgresqlConf`; kebenarannya dijaga test terhadap `my_print_defaults` runtime asli. Pembaca file opsi MySQL membuang kutip luar yang berpasangan, memproses `\\` dan `\"`, dan tidak memperlakukan `#` atau `;` di dalam kutip sebagai komentar; spike lokal membuktikan round trip untuk teks AC-2.

### Inisialisasi data

Urutan langkah `InitPlan` (dijalankan `execute_init` spec 0010 di bawah lock instance dan `initSeconds` 180, di atas batas langkah 120 detik):

| No | Langkah | Hasil | Gagal |
| --- | --- | --- | --- |
| 1 | `createManagedDirectories` | `data`, `config`, `logs`, `temp`, dan folder run ada, 0700 | `init_step_failed` |
| 2 | `materializeSecret` `rootPassword` | `<run>/secrets/root-password` 0600 berisi byte password persis | `secret.*` dari spec 0011 |
| 3 | `mysqld --initialize-insecure` dengan SQL akun dari stdin | folder data baru; `mysql_upgrade_history` memuat versi runtime; tiga akun `root` ber password dan akun `stackly_readiness`; proses keluar 0 (sekitar 1,5 detik di Mac ini) | `mysqld` keluar 1: `init_step_failed` dengan cuplikan error. Statement SQL yang gagal membuat MySQL membatalkan init saat itu juga; folder data yang sudah berisi file ditolak tanpa mengubah isinya |
| akhir | guard `SecretFile` dilepas | file `root-password` dihapus, berhasil atau gagal | |

- MySQL membuat `root@localhost` tanpa password lebih dulu, lalu menjalankan file init di proses yang sama. Tidak ada server yang menerima koneksi selama itu: `--initialize` tidak membuka port atau socket, dan proses selalu keluar sendiri. Kalau file init gagal, MySQL keluar 1 dan meninggalkan folder data yang tidak lengkap; instance `failed` tidak pernah di start (BR-008), dan kompensasi create spec 0020 menghapus folder instance itu.
- `LOAD_FILE` membaca `<@@replica_load_tmpdir>/root-password`, yaitu file langkah 2. File 0600 milik user yang sama terbaca (dibuktikan spike lokal, di kedua lini).
- `QUOTE()` membuat literal SQL yang aman dari isi password, jadi kutip dan backslash tidak butuh escaper di Stackly. Penjaga `IF` membuat statement sengaja gagal kalau file tidak terbaca atau lebih pendek dari 8 karakter, sehingga `root` tidak pernah berakhir dengan password kosong.
- Kegagalan selalu tertutup: MySQL berhenti di statement pertama yang gagal dan keluar 1, jadi tidak ada jalan di mana langkah keluar 0 dengan akun yang belum lengkap.
- `GRANT ALL` tidak mencakup `AUDIT_ABORT_EXEMPT` dan `FIREWALL_EXEMPT`, dan kedua nama itu belum terdaftar saat `--initialize` (memberinya di file init gagal dengan error sintaks). Karena itu akun `root` ber host IP tidak memilikinya. Keduanya hanya berarti untuk plugin audit dan firewall edisi Enterprise.
- File init MySQL menerima statement multi baris; paket tetap menulis satu statement per baris. Teks ini 1 sampai 16384 byte, tanpa `${`, dan tanpa karakter kontrol selain `\n`, sesuai aturan `stdin.text` spec 0019.
- Folder data yang tidak kosong sudah ditolak create spec 0020 (`instance.data_not_empty`) sebelum init; penolakan `--initialize-insecure` adalah lapis kedua.

### Start dan parameter milik Stackly

| Parameter | Sumber | Kenapa di argv |
| --- | --- | --- |
| file opsi | `--defaults-file=${instance.config}/my.cnf` | hanya file buatan Stackly yang dibaca |
| variabel persisted | `--persisted-globals-load=OFF` | `mysqld-auto.cnf` (tempat `SET PERSIST` menulis) mengalahkan argv, jadi satu satunya cara menjaga parameter di bawah ini adalah tidak memuatnya |
| folder data, temp | `--datadir`, `--tmpdir` | path utuh tanpa escape |
| alamat dan port | `--bind-address=${endpoint.client.host}`, `--port=${endpoint.client.port}` | cocok dengan registry |
| socket, file pid | `${endpoint.socket.socket}`, `${instance.run}/mysqld.pid` | di folder run 0700 |
| X Protocol | `--mysqlx=OFF` | tanpa port 33060 dan tanpa `/tmp/mysqlx.sock` |
| `skip_name_resolve` | `ON` | akun ber host `localhost` hanya cocok lewat socket (lihat *Auth lokal*) |
| `secure_file_priv` | `NULL` | impor dan ekspor file sisi server mati, sama dengan bawaan build tarball upstream |
| general log, slow log | file di `${instance.logs}` | kalau dinyalakan pengguna, tetap di dalam instance |

Spike lokal membuktikan: MySQL menolak `SET PERSIST` untuk `port`, `bind_address`, `socket`, `datadir`, `tmpdir`, `pid_file`, `secure_file_priv`, `log_error`, dan `persisted_globals_load` ("non persistent read only variable"), tetapi menerima `SET PERSIST_ONLY skip_name_resolve = OFF` dan `lower_case_table_names = 1`, dan nilai persisted mengalahkan argv saat `--defaults-file` dipakai. `lower_case_table_names = 1` yang persisted bahkan membuat server gagal start. Dengan `--persisted-globals-load=OFF` semua itu tidak berpengaruh. Akibatnya nilai `SET PERSIST` buatan pengguna berlaku sampai restart berikutnya saja; settings yang bertahan diubah lewat Stackly.

Panjang path socket MySQL paling banyak 103 byte. Folder run berasal dari runtime root spec 0004 yang sudah dianggarkan untuk itu, dan nama `mysql.sock` (10 byte) di bawah `SOCKET_NAME_MAX`.

### Auth lokal

| Akun | Cara masuk | Hak |
| --- | --- | --- |
| `root@localhost` | `caching_sha2_password` dengan password, lewat socket instance | semua, pemilik server |
| `root@127.0.0.1`, `root@::1` | password yang sama, lewat TCP loopback | `ALL PRIVILEGES ... WITH GRANT OPTION` dan `PROXY`; sama dengan `root@localhost` kecuali `AUDIT_ABORT_EXEMPT` dan `FIREWALL_EXEMPT` |
| `stackly_readiness@localhost` | tanpa password, hanya lewat socket instance | `USAGE` saja; `MAX_USER_CONNECTIONS 3` |
| akun buatan pengguna | sesuai pengguna | urusan pengguna |

- MySQL untuk macOS tidak membawa plugin `auth_socket` (peer credential), jadi padanan `peer` PostgreSQL tidak ada. Dengan `skip_name_resolve = ON`, koneksi TCP dari `127.0.0.1` tidak pernah dipetakan ke nama `localhost`, sehingga akun ber host `localhost` hanya cocok untuk koneksi socket. Spike lokal membuktikan: `stackly_readiness` lewat TCP ditolak saat `ON`, dan diterima saat `OFF`. Karena itu `skip_name_resolve` adalah parameter milik Stackly.
- Batas sebenarnya untuk akun readiness adalah folder run 0700: proses mana pun milik user OS pemilik folder itu bisa login sebagai `stackly_readiness`. Itu bukan eskalasi hak, karena user itu sudah memiliki folder data, dan akun itu tidak bisa membaca data apa pun.
- Karena `skip_name_resolve = ON`, klien TCP butuh akun ber host IP. Itu sebabnya init membuat `root@127.0.0.1` dan `root@::1`. Ketiga akun `root` adalah akun terpisah di MySQL: mengganti password satu akun tidak mengganti yang lain (README menyebutnya).
- Tidak ada akun anonim dan tidak ada database `test` (bawaan `--initialize`).

### Readiness dan health

- Probe `exec` menjalankan `${runtime.executables.mysql}` dengan argv di manifest. Sukses berarti exit 0 (`SELECT 1` dijawab). Socket yang belum ada (`ERROR 2002`), akun yang hilang (`ERROR 1045`), dan akun terkunci (`Access denied ... Account is locked`) masing masing membuat `mysql` keluar 1 dengan pesan di stderr.
- Socket diperiksa, bukan TCP, karena akun readiness hanya cocok lewat socket. `mysqld` mengikat port TCP sebelum menerima koneksi apa pun dan keluar kalau bind gagal, jadi query yang berhasil lewat socket juga berarti listener TCP aktif.
- `--connect-timeout=2` membuat `mysql` menyerah sebelum `attemptTimeoutMilliseconds` 3000; selebihnya aturan pembantu spec 0010 berlaku.
- Env child tertutup (spec 0009) dan `HOME` adalah folder temp instance, jadi `MYSQL_PWD`, `MYSQL_HOST`, `~/.my.cnf`, dan `~/.mylogin.cnf` milik pengguna tidak pernah terbaca; `--no-defaults` menjaga hal yang sama.
- `health` tanpa `probe` memakai probe readiness. Detail kegagalan mengikuti format probe `exec` spec 0019: baris tidak kosong pertama stderr lewat `Redactor`, dipotong 256 karakter. Contoh pesan `mysql`: `ERROR 1045 (28000): Access denied for user 'stackly_readiness'@'localhost' (using password: NO)`. Di detail readiness yang kamu lihat, ekornya tampil `(using password: ****)`, karena `Redactor` menutup kata sesudah `password:`. Tidak ada secret di sana (akun itu tanpa password); remedy mencocokkan `'stackly_readiness'@'localhost'` dan AC-7 mencocokkan `Access denied for user 'stackly_readiness'@'localhost'`, yang keduanya ada sebelum bagian yang ditutup, jadi tidak terpengaruh.

### Stop

`gracefulProcess` dengan `signal: term`, yaitu shutdown normal MySQL: koneksi ditutup, InnoDB menulis halaman yang tertunda, lalu `mysqld` keluar 0 dan menghapus file pid, socket, dan lock socket. Spike lokal: sekitar 3 detik dengan satu sesi `SLEEP(600)` terbuka. `stopSeconds` 120 memberi ruang untuk buffer pool besar. Setelah batas itu operasi gagal `supervisor.stop_timeout` dan instance tetap `stopping` (spec 0010); force stop milik spec 0026.

### Preflight data

- `dataMarker` `mysql_upgrade_history` adalah file JSON satu baris yang ditulis MySQL 8.4 ke atas, misalnya `{"file_format":"1","upgrade_history":[{"date":"2026-11-12 00:53:09","version":"8.4.11","maturity":"LTS","initialize":true}]}`. Pattern dicocokkan penuh sesudah `trim_end`; `.*` yang rakus membuat capture group mengambil entri `version` terakhir (versi yang terakhir menulis data), lalu dua komponen pertamanya menjadi `RuntimeLine` (`8.4` atau `9.7`).
- Tanpa `lineCompatibility`, data hanya boleh dijalankan runtime dengan line yang sama (BR-009 ketat). Ini penting untuk MySQL: binary 9.7 yang dijalankan di data 8.4 akan meng upgrade data di tempat tanpa bertanya, dan binary 8.4 di data 9.7 keluar dengan `Cannot downgrade from 90702 to 80411`. Stackly menolak keduanya sebelum spawn.
- Pemanggilnya tetap `check_data_compatibility` (spec 0025): start sebelum spawn, dan `rebind::check` fitur 58. Fitur ini tidak menambah pemeriksaan sendiri.
- Batas yang dinyatakan: penanda dibaca paling banyak 4096 byte (`DATA_MARKER_MAX_LEN`). Setiap upgrade patch menambah sekitar 95 byte, jadi sesudah kira kira 40 upgrade patch pada satu folder data penanda menjadi `marker_unreadable` dan start ditolak. Itu gagal tertutup, dan dicatat di Follow-up.

### Skenario konformitas

`providers/mysql/tests/scenarios.yaml`:

```yaml
schema: stackly.provider-scenarios/v1
scenarios:
  - name: default
  - name: binlog
    settings: { binaryLog: true }
```

Secret tidak ditulis di sini; harness meminta daemon membangkitkan `rootPassword`. Langkah `reload` harness tercatat `skipped`, dan `handover` tercatat `unproven` sampai AC-17 lulus.

### Perubahan core dan file bersama

Ukuran RISK-07 untuk MySQL: **1 perubahan core**.

| No | Perubahan | Tempat | Isi | Alasan MySQL butuh |
| --- | --- | --- | --- | --- |
| 1 | Format template `mysqlCnf` | `crates/core/src/provider/template.rs` (varian `TemplateFormat::MysqlCnf`, `from_code`, `escape_text`, `boolean`, `allows_tags`, dan arm di `output_parses` bersama format tanpa parse ulang), `crates/core/src/provider/validate.rs` (pesan daftar format), `crates/core/src/provider/fuzzing.rs` (nomor selector baru untuk format ini) dan penjelasan selector di `xtask/src/fuzz.rs`, `docs/providers/manifest-reference.md` (baris `format`) | aturan di *Format `mysqlCnf`*; `render_secret_placeholder` di `secret::delivery` memanggil `escape_text` format itu, jadi ikut tanpa perubahan | `my.cnf` (direncanakan spec 0009) |

Bukan perubahan engine, tetapi menyentuh core: satu entri data di `HANDOVER_PROVEN` (`crates/core/src/supervisor/handover.rs`), hanya bersama record evidence `PASS` AC-17.

Di luar ukuran ini (dicatat 2026-11-12): test lifecycle MySQL menemukan balapan start di supervisor yang sudah ada sebelum fitur ini. `run_attempt` meng-commit `spawned` (lifecycle `running`) sebelum slot proses dan `awaiting_probe` ditulis, jadi `running` sesaat terlihat dengan readiness `unknown`, bukan `not_ready` seperti janji spec 0004 dan spec 0010 AC-3 dan AC-4. Perbaikannya di core, commit `0de32eda` (`crates/core/src/supervisor/startup.rs`), dengan test regresi `6bf3bde0`, dan berlaku untuk setiap provider. Itu perbaikan cacat spec 0010, bukan perluasan engine yang MySQL butuhkan, jadi tidak dihitung sebagai ukuran RISK-07 MySQL, yang tetap 1.

File bersama yang disentuh fitur ini (hanya boleh diubah satu sesi pada satu waktu):

| File | Perubahan | Langkah |
| --- | --- | --- |
| modul format di `crates/core` (`provider/template.rs`, `provider/validate.rs`, `provider/fuzzing.rs`) dan `xtask/src/fuzz.rs` | varian `MysqlCnf` | 1 |
| `docs/providers/manifest-reference.md` | nama format baru | 1 |
| `crates/core/tests/provider_packages.rs` | fungsi test golden paket `mysql` | 2 |
| `protocols/fixtures/instance/index.json` (dan file baru `create-form-mysql.json`, seed fuzz lewat `cargo xtask fuzz seed`) | satu entri | 3 |
| `xtask/src/runtime/spike/providers.rs`, `xtask/src/runtime/spike/package_recipe.rs` | hapus struct `MySql` dan penandanya; test `find_package("mysql")` | 4 |
| `apps/daemon/tests/native_runtime_install.rs`, `apps/daemon/tests/native_runtime_install/harness.rs` | dua test MySQL menjadi sungguhan; konstanta lini | 4 |
| `apps/daemon/tests/provider_conformance.rs` | dua test native, dan daftar paket di test batas langkah | 10 |
| `providers/catalog.toml`, `providers/catalog-revisions.json` | MySQL ke `[[provider]]` | 11 |
| `crates/core/tests/fixtures/compat/` | hasil `provider freeze` | 11 |
| `apps/daemon/tests/native_launchd.rs`, `crates/core/src/supervisor/handover.rs` | test launchd dan entri bukti | 13 |

Tidak disentuh: `runtimes/inventory.toml` (kedua artefak MySQL sudah `available`), `xtask/src/check_deps/source_scan.rs` (`mysql` sudah ada di `PROVIDER_WORDS`), dan `crates/protocol/` (tidak ada DTO, method, atau kode error baru). Nomor protokol dan migrasi: tidak ada yang dibutuhkan; kalau build ternyata membutuhkannya, yang dipakai adalah nomor bebas berikutnya, ditetapkan lead saat build.

### Evidence kompatibilitas

Bentuk dari spec 0018, ditulis test native per artefak:

```json
{
  "schema": "stackly.compatibility-evidence/v1",
  "providerRevision": { "id": "official/mysql", "revision": "1.0.0", "manifestSha256": "<hex>" },
  "checks": ["adoption", "init", "isolation", "line_mismatch_refusal", "readiness", "restart", "start", "stop"],
  "knownLimitations": []
}
```

`cargo xtask runtime compat-record` menyalin record itu ke `evidence/compatibility/`, dan `cargo xtask codegen` mengubah PASS terbaru menjadi compatibility record di `versions.json`. Test BR-025 adalah suite native `apps/daemon/tests/mysql.rs`, bukan test konformitas, karena check `isolation` dan `line_mismatch_refusal` tidak ada di harness.

### Data model

Tidak ada tabel, kolom, atau migrasi baru. Yang disimpan per instance sudah dimodelkan spec 0004, 0005, 0010, dan 0011:

| Data | Tempat | Isi untuk MySQL |
| --- | --- | --- |
| provider revision | registry, `provider_revision_dir` | `official/mysql@1.0.0#<sha>`, capability `multi_instance`, `persistence`, `cold_backup` |
| instance | registry | `data_line` dan `data_version` dari record runtime saat create (`8.4` atau `9.7`) |
| config revision | registry (`config_revisions`) | nilai settings; `rootPassword` sebagai teks reference |
| secret | Keychain plus registry `secret_references` | reference `local:instance/<id>/root-password` |
| endpoint | registry (`instance_endpoints`) | `client` (tcp, host, `Fixed` atau `Auto`, `bound`) dan `socket` (unix, `mysql.sock`) |
| data | folder data instance | milik MySQL, termasuk `mysql_upgrade_history`, `mysqld-auto.cnf` (kalau pengguna memakai `SET PERSIST`), dan akun `stackly_readiness` |
| config | folder config instance | `my.cnf`, ditulis ulang sebelum setiap spawn |
| log tambahan | folder logs instance | `general.log` dan `slow.log`, hanya kalau dinyalakan pengguna |

### API surface

Tidak ada method IPC, command CLI, DTO, atau kode error baru. Permukaannya adalah paket dan fungsi berikut:

| Fungsi atau artefak | Tempat | Input utama | Output utama | Pemanggil | Error utama |
| --- | --- | --- | --- | --- | --- |
| paket `providers/mysql` | repo | file paket | `ValidatedPackage` | catalog, create, harness, spike | `manifest.invalid` |
| `TemplateFormat::MysqlCnf` | `core::provider::template` | nilai context | teks ter escape | renderer, `render_secret_placeholder` | `UnrepresentableValue` |
| `check_data_compatibility` (sudah ada) | `core::instance::data_compat` | line tercatat, isi penanda, line runtime | `Ok` atau `DataOutcome` | start, `rebind::check` | `supervisor.data_incompatible`, `marker_mismatch` |
| suite native | `apps/daemon/tests/mysql.rs` dan `apps/daemon/tests/mysql/` | `SpawnedDaemon` atau server in process, cache runtime | evidence `native` dan `compatibility.json` | `cargo test -p stacklyd --test mysql -- --ignored` | `BLOCKED` tanpa artefak |
| test konformitas | `apps/daemon/tests/provider_conformance.rs` | paket, line | report `stackly.provider-conformance/v1` | `cargo test -p stacklyd --test provider_conformance -- --ignored mysql` | `BLOCKED` tanpa artefak |
| test launchd | `apps/daemon/tests/native_launchd.rs` | sesi GUI, cache runtime | evidence untuk `HANDOVER_PROVEN` | `cargo test -p stacklyd --test native_launchd -- --ignored --test-threads 1 mysql` | `BLOCKED` tanpa sesi GUI |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| create | password root | `SecretService::put(PutTarget::New, Generate)` kecuali `params.secrets` memberi `Value` atau `CopyFrom` (spec 0011, 0020) |
| create | reference secret | `local:instance/<id>/root-password`, purpose dari nama field (spec 0011) |
| create | permintaan port `client` | `params.endpoints`, default `Auto`; host dari `defaultHost` `127.0.0.1` |
| create | `data_line`, `data_version`, `data.initializedLine` | record runtime yang dipilih (spec 0018, 0020) |
| init | path file password | `${instance.secrets}/root-password` = `instance_secrets_dir(short key)` |
| init | byte password | `ResolvedSecrets` dari `resolve_for_start`, ditulis `SecretFile::create` |
| init | folder file password untuk `LOAD_FILE` | argv `--replica-load-tmpdir=${instance.secrets}`, dibaca SQL sebagai `@@replica_load_tmpdir` |
| init | SQL akun | `stdin.text` literal manifest |
| init | nama akun, host akun, batas koneksi | literal SQL di manifest |
| init, start | root runtime, folder data, temp, run | `runtime.root`, `InstancePaths`, `instance_run_dir(short key)` (spec 0004) |
| start, probe | path socket | `${endpoint.socket.socket}` = folder run ditambah `mysql.sock` |
| start | host dan port | endpoint `client` (spec 0004, 0010) |
| start | file general log dan slow log | `${instance.logs}` ditambah nama literal |
| render | `max_connections`, `innodb_buffer_pool_size`, pilihan binary log | settings tervalidasi dari config revision aktif |
| render | zona waktu | literal template |
| probe | program | root runtime ditambah `bin/mysql` dari `additionalExecutables` |
| probe | akun, query, timeout koneksi | literal argv manifest |
| probe | detail kegagalan | stderr pembantu (spec 0010), lewat `Redactor` |
| stop | sinyal | `signal: term` di manifest |
| preflight | line tercatat | `data_line` instance di registry |
| preflight | line penanda | isi `<data>/mysql_upgrade_history` lewat `read_file_limited` 4 KiB dan pattern manifest |
| preflight | line runtime | record runtime yang diikat |
| connection | connection string | blok `connection` paket, dirender daemon dan disusun client dengan `rootPassword` (spec 0021) |
| Open In | URL aplikasi, argv terminal | blok `openWith` paket (spec 0070); password hanya lewat clipboard tersembunyi |
| doctor | teks remedy | blok `remedies` paket yang dipin (spec 0034, 0037) |
| catalog | kartu provider | `metadata` paket dan `versions.json` (spec 0017); mark `My` dan tile `teal` dari `providers/catalog.toml` |
| `versions.json` | artefak | `runtimes/inventory.toml` lewat `cargo xtask codegen` |
| evidence | `version` subject | `SELECT VERSION()` pada instance yang berjalan |
| evidence | `digest` subject | `ArtifactRecord.sha256` |
| evidence | `providerRevision` | `ValidatedPackage::key()` |
| test native | file artefak | `stackly_testkit::RuntimeCache::open()` lalu `entry(sha256, size)`, dengan `sha256` dan `size` dari `versions.json` paket |
| test native | sentinel | nilai acak per instance yang dibuat test, ditulis lewat `bin/mysql` runtime dengan password dari `FakeSecretStore` lewat env `MYSQL_PWD` |
| spike | settings dan secret | skenario `default` paket dan `ResolvedSecrets::for_tests` (spec 0036 *Spike dari paket*) |

### Edge cases dan kegagalan

| Kasus | Yang terjadi | Penanganan |
| --- | --- | --- |
| Folder data tidak kosong saat create | spec 0020 | `instance.data_not_empty` sebelum init; `--initialize-insecure` adalah lapis kedua |
| SQL akun gagal di tengah | MySQL berhenti di statement itu dan keluar 1 | `init_step_failed` dengan cuplikan error; instance `failed` dan tidak pernah di start (BR-008); kompensasi create menghapus folder instance |
| Langkah init melewati 120 detik | aturan pembantu spec 0010 (TERM lalu KILL setelah 5 detik) | `init_step_timeout`, tanpa cuplikan; instance `failed` |
| Daemon mati saat init | rekonsiliasi spec 0010 | `init_failed { interrupted }`; proses `--initialize` selalu keluar sendiri (berhasil atau gagal) dan memegang lock di fd 3 sampai keluar, jadi tidak ada proses yatim yang menetap dan tidak ada init kedua di atasnya |
| Password berisi baris baru | ditolak saat `put` | `secret.invalid_value` (`reason: control_characters`), karena `printableOnly` |
| Password sama persis dengan satu argumen atau nilai sesudah `=` (`/dev/stdin` saat init, `127.0.0.1` saat start), atau password 16 byte ke atas yang merupakan potongan sebuah argumen | `ensure_not_in_argv` menolak | init atau start gagal `secret.delivery_rejected` (lihat *Catatan per bagian*); residual yang dinyatakan |
| Port `Fixed` dipakai proses lain | spec 0010 | `supervisor.endpoint_in_use` sebelum spawn |
| Shutdown tidak bersih | crash recovery InnoDB saat start | `readinessSeconds` 120 dan `recoverySeconds` 600 |
| Folder data dipakai `mysqld` lain | InnoDB gagal mengunci `ibdata1` dan proses keluar | crash loop berakhir `failed` dengan cuplikan; lock instance Stackly mencegahnya untuk proses yang dikelola |
| Akun `stackly_readiness` dihapus atau dikunci | probe gagal dengan pesan MySQL | readiness `not_ready`, detail menyebut akun; remedy `readiness_account` |
| Password `root` diubah lewat `ALTER USER` | readiness tidak terpengaruh | connection string Stackly basi; pengguna memperbarui Keychain lewat `secret.set` (spec 0011). Hanya akun yang disebut di `ALTER USER` yang berubah |
| `SET PERSIST` oleh pengguna | berlaku sampai restart | tidak dimuat saat start; dijelaskan di README dan komentar `my.cnf` |
| `my.cnf` disunting tangan | drift (spec 0029) | render diblokir sampai pengguna menyelesaikan drift |
| Data root di volume case sensitive | `lower_case_table_names` menjadi 0 saat init, bukan 2 | ditetapkan MySQL saat init dan tidak pernah diubah Stackly |
| OS membersihkan folder run saat instance jalan | socket hilang | readiness `not_ready`; restart membuat ulang |
| `mysql` menggantung | timeout percobaan | aturan pembantu spec 0010; dihitung sebagai kegagalan probe |
| Stop melewati 120 detik | spec 0010 | `supervisor.stop_timeout`, tetap `stopping`; force stop spec 0026 |
| `mysql_upgrade_history` melewati 4096 byte | `marker_unreadable` | start ditolak; Follow-up |

### Asumsi pada spec lain

- **Assumed (spec 0009, 0019)**: `stdin.text`, `materializeSecret`, `printableOnly`, probe `exec` dan format detail kegagalannya, `dataMarker`, `runtime.versionCheck`, dan aturan `init_port_issues` sudah dibangun dan dipakai apa adanya.
- **Assumed (spec 0010)**: `execute_init` menjalankan langkah `Run` dengan lock di fd 3, mengirim `stdin.text` lewat pipe lalu menutupnya, melaporkan exit di luar `successExitCodes` sebagai `init_step_failed` dengan cuplikan stderr, dan menghentikan langkah yang melewati `timeoutSeconds` dengan aturan pembantu (`init_step_timeout`); executor menulis ulang config sebelum setiap spawn; argumen literal `/dev/stdin` tidak ditolak aturan argv mana pun.
- **Assumed (spec 0025, 0030)**: `check_data_compatibility` dan pemeriksaan folder `check_instance_folder` berperilaku seperti untuk PostgreSQL (penanda hilang menjadi `supervisor.data_unavailable`).
- **Assumed (spec 0036)**: harness, `PackageRecipe`, dan `provider freeze` bekerja untuk paket dengan field secret dan endpoint `unix` seperti untuk PostgreSQL; `PackageRecipe` menaruh root runtime di folder pendek sehingga batas 103 byte socket terpenuhi.
- **Assumed (spec 0070)**: aplikasi Sequel Ace memakai bundle ID `com.sequel-ace.sequel-ace` dan menerima URL `mysql://`; entri yang aplikasinya tidak terpasang tidak tampil. Ini dicek di langkah manual.

### Jawaban follow-up dari spec lain

- **Spec 0009** (format `mysqlCnf`, satu perubahan core): dijawab *Format `mysqlCnf`* dan AC-2.
- **Spec 0003** (hapus recipe spike dan penanda `// provisional: F42`; baris MySQL tabel *Langkah spike per provider*): dijawab AC-14 dan langkah 4. Sejak itu init, start, ready, versi, dan stop spike MySQL dibangun dari paket; kolom probe tambahan MySQL memang kosong. Konfigurasi tanpa password di tabel lama adalah konfigurasi spike, bukan default instance.
- **Spec 0036** ("Fitur 39 sampai 43" dan *Dari contoh ke paket resmi*): paket lewat `provider new`, fixture form, satu test konformitas native per lini, `provider freeze` sesudah catalog, recipe spike dihapus (AC-1, AC-14). Test BR-025 adalah suite native sendiri (*Evidence kompatibilitas*).
- **Spec 0030** (test native launchd sebelum `HANDOVER_PROVEN`): AC-17.
- **Spec 0017** (*Aturan masuk catalog*): AC-1 dan langkah 11.
- **Spec 0018** (AC-20, test install dari cache untuk MySQL): AC-14.
- **Spec 0034** (remedy provider lain): entri `readiness_account` (AC-18).
- **Spec 0070** (`openWith` MySQL): TablePlus, Sequel Ace, dan `mysql` (AC-18).
- **Spec 0011** (kunci sensitif `mysql_pwd`): paket tidak memakai `MYSQL_PWD`; hanya test native yang memakainya untuk login sebagai `root`.

### Key invariants

- Tidak pernah ada server MySQL yang terjangkau lewat TCP dengan akun tanpa password, dan `root` selalu ber password sejak init selesai.
- Probe readiness dan health tidak pernah login sebagai `root` dan tidak pernah membaca secret.
- Setiap program yang dijalankan untuk instance MySQL berasal dari root runtime yang diikat instance itu, dan tidak pernah membaca file opsi di luar folder config instance.
- Port, alamat, socket, dan semua path selalu dari argv, dan variabel persisted tidak dimuat.
- Data dengan line `mysql_upgrade_history` berbeda dari line runtime tidak pernah dijalankan, jadi tidak pernah di upgrade di tempat.
- Create tidak pernah memakai folder data yang sudah berisi.
- Password tidak pernah ada di argv, config, spool, log, atau registry; file password hanya ada selama aksi init.

### Security model

- Ancaman yang dijaga: login tanpa password lewat jaringan (tiga akun `root` ber password; akun tanpa password hanya cocok lewat socket privat), proses lokal yang menebak port (TCP hanya loopback dan selalu butuh password), password di argv atau log (file sekali pakai spec 0011, `QUOTE()` di dalam server, `ensure_not_in_argv`, `Redactor`), tool dan file opsi dari luar (program hanya dari runtime, `--no-defaults` atau `--defaults-file`, env tertutup), dan pergeseran parameter lewat `SET PERSIST` (`--persisted-globals-load=OFF`).
- `secure_file_priv` `NULL` saat berjalan, jadi akun ber hak `FILE` tidak bisa membaca atau menulis file lewat SQL. Saat `--initialize`, MySQL mengosongkan `secure_file_priv`, tetapi SQL yang berjalan hanyalah teks tetap di manifest, yang hanya membaca file `root-password` di folder `secrets/` instance, dan tidak ada koneksi klien selama itu.
- Residual risk yang dinyatakan: proses lain milik user OS yang sama bisa login sebagai `stackly_readiness` lewat socket (tanpa hak atas data); `root` bisa menyalakan general log atau mengubah variabel dinamis saat berjalan, termasuk menunjuk general log ke path lain, karena `root` memang berkuasa penuh atas server; koneksi TCP memakai TLS dengan certificate yang dibangkitkan MySQL sendiri dan klien tidak memverifikasinya; user OS lain bisa mengikat port loopback instance yang sedang berhenti, sehingga password yang dikirim klien ke server palsu itu bisa terbaca kalau klien mengizinkan auth cleartext; tiga akun `root` bisa berbeda password kalau pengguna hanya mengganti satu.

### Configuration required

Tidak ada variabel environment, credential, atau port baru untuk daemon dan CLI. Test native butuh artefak MySQL 8.4.11 dan 9.7.2 di cache runtime xtask (keduanya ada di Mac ini saat spec ditulis). Build memakai `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`; setiap instance MySQL memakai ratusan MiB disk (folder data baru sekitar 90 MiB ditambah redo log) dan ratusan MiB memori, jadi test yang menjalankan beberapa instance berjalan satu per satu.

### Critical test scenarios

- Golden: render `my.cnf` (default dan `binaryLog: true`), `init_plan`, dan `run_plan` di context golden spec 0009; argumen pertama setiap argv; tidak ada entri argv yang memuat nilai secret. Memverifikasi **AC-1**, **AC-12**.
- Format: tabel escaper `mysqlCnf` (kutip ganda, backslash, `#`, `;`, kutip tunggal, Unicode, karakter kontrol ditolak, boolean, integer negatif) dan round trip `my_print_defaults` dari runtime 8.4.11. Memverifikasi **AC-2**.
- Init: urutan langkah, file `root-password` hilang sesudahnya, tidak ada proses sisa, tidak ada file `binlog.*`, dan tidak ada socket atau port selama init; salinan paket dengan SQL yang salah berakhir `init_step_failed` dengan exit 1 dan cuplikan error; file sisipan membuat `--initialize-insecure` gagal dan file itu utuh. Memverifikasi **AC-3**.
- Auth: daftar akun dari `mysql.user`; `root` tanpa password ditolak di socket dan TCP; `stackly_readiness` ditolak lewat TCP, hanya `USAGE`, `CREATE DATABASE` gagal; password berkarakter khusus login di socket dan TCP; argv semua proses tanpa nilai password. Memverifikasi **AC-4**.
- Endpoint: `lsof -a -p <pid> -i -U` hanya port endpoint dan socket instance; `SET PERSIST` dan `SET PERSIST_ONLY` lalu restart tidak mengubah nilai. Memverifikasi **AC-5**.
- Path: nilai variabel path dan `lsof` file terbuka; baris error log di `instance.logs_read`. Memverifikasi **AC-6**.
- Readiness jujur: `ACCOUNT LOCK` dengan TCP tetap terbuka membuat `not_ready` setelah 2 kegagalan dengan detail berisi nama akun; `ACCOUNT UNLOCK` membuat `ready`. Memverifikasi **AC-7**.
- Stop: sesi `SLEEP(600)` terbuka, `instance.stop` selesai dalam 30 detik tanpa `SIGKILL`; file pid dan socket hilang; start berikutnya tanpa pesan crash recovery. Memverifikasi **AC-8**.
- Sentinel: dua instance per lini dan satu pasangan lintas lini, restart, lalu query lewat TCP dan socket. Memverifikasi **AC-9**.
- Line salah: data 8.4 diikat ke runtime 9.7 dan sebaliknya, penanda dihapus, penanda diubah ke `8.0.40`; byte `mysql_upgrade_history` dan `mysql.ibd` dibandingkan sebelum dan sesudah; `rebind::check` memberi outcome yang sama. Memverifikasi **AC-10**.
- Data yang sudah ada: salinan folder data sah disisipkan lewat kait test, create ditolak `instance.data_not_empty`, nol spawn, file sama byte demi byte. Memverifikasi **AC-11**.
- Tool dari runtime: daemon dengan `PATH` umpan dan `HOME` ber `.my.cnf` rusak. Memverifikasi **AC-12**.
- Lifecycle dan adopsi per lini, edit `maxConnections` lalu restart, `binaryLog`. Memverifikasi **AC-13**.
- Konformitas native per lini, spike dari paket, install dari cache. Memverifikasi **AC-14**, **AC-15**.
- Evidence: cache kosong membuat `BLOCKED` dengan alasan dan owner yang benar; run lengkap menulis `compatibility.json` yang lolos `cargo xtask runtime compat-record`. Memverifikasi **AC-16**.
- Launchd: restart terencana dan `SIGKILL` daemon di bawah LaunchAgent dengan instance MySQL `ready`. Memverifikasi **AC-17**.
- Detail dan doctor: `openActions` tiga entri tanpa secret; `DROP USER` lalu `doctor.run` memuat remedy. Memverifikasi **AC-18**.

## Build plan

Urutan mengikuti pendekatan Journey proyek: satu jalur dibuat lengkap dulu, yaitu "instance MySQL 8.4 dari paket sampai ready lalu stop, termasuk semua jalur gagalnya". Milestone 1 membangun semua yang bisa dibuktikan tanpa server nyata, milestone 2 membuktikan jalur itu pada MySQL 8.4 asli lewat daemon, milestone 3 memperluasnya ke 9.7, penolakan line, konformitas, evidence, dan catalog, dan milestone 4 menutup handover.

**Milestone 1: Format, paket, dan spike dari paket**

1. Format `mysqlCnf`: varian `TemplateFormat::MysqlCnf` di `crates/core/src/provider/template.rs` dengan unit test escaper, arm di `output_parses`, pesan di `crates/core/src/provider/validate.rs`, nomor selector baru di `crates/core/src/provider/fuzzing.rs` dengan penjelasannya di `xtask/src/fuzz.rs`, unit test bahwa `render_secret_placeholder` memakai escaper format ini, dan baris `format` di `docs/providers/manifest-reference.md`. Memenuhi **AC-2** (tanpa round trip native).
2. Paket `providers/mysql` lewat `cargo xtask provider new mysql`, lalu diubah menjadi isi *Paket* (`provider.yaml`, `schemas/settings.schema.json`, `templates/my.cnf.tmpl`, `tests/scenarios.yaml`, `README.md`, `LICENSE-NOTICES.md`), `versions.json` lewat `cargo xtask codegen`, `cargo xtask provider check providers/mysql`, fungsi test golden paket `mysql` di `crates/core/tests/provider_packages.rs` (mengikuti fungsi paket PostgreSQL di file itu), dan golden di `crates/core/tests/golden/provider/mysql/` lewat `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test provider_packages`. Memenuhi **AC-1** (tanpa catalog), **AC-12** (bagian plan), **AC-15** (bagian manifest).
3. Fixture form: `protocols/fixtures/instance/create-form-mysql.json` dan entri di `protocols/fixtures/instance/index.json` lewat `STACKLY_UPDATE_GOLDEN=1 cargo test -p stacklyd --test create_form_fixtures`, lalu `cargo xtask fuzz seed`, `cargo xtask codegen --check`, dan `cargo test -p xtask`. Memenuhi **AC-14** (bagian fixture).
4. Hapus struct `MySql` dan penanda `// provisional: F42` dari `xtask/src/runtime/spike/providers.rs` (pemilihan recipe jatuh ke `PackageRecipe` karena paket kini ada), ubah test unit di `xtask/src/runtime/spike/package_recipe.rs` yang menuntut `find_package("mysql")` kosong, dan ubah `mysql_8_4_installs_from_the_cache` serta `mysql_9_7_installs_from_the_cache` di `apps/daemon/tests/native_runtime_install.rs` menjadi test sungguhan seperti test PostgreSQL di file itu, dengan konstanta `MYSQL_8_4` dan `MYSQL_9_7` di `apps/daemon/tests/native_runtime_install/harness.rs`. Jalankan `cargo test -p xtask`, kedua test install itu, dan kedua test MySQL di `xtask/tests/native_runtime.rs` dari cache. Memenuhi **AC-14** (spike dan install), **AC-15**.

**Milestone 2: MySQL 8.4 asli lewat daemon**

5. Harness native `apps/daemon/tests/mysql.rs` dan `apps/daemon/tests/mysql/harness.rs`, mengikuti `apps/daemon/tests/postgresql/harness.rs`: runtime lewat `RuntimeCache` dan `install_cached_artifact`, paket lewat `pin_package`, `FakeSecretStore`, pembantu `sql(instance, user, statement)` yang menjalankan `bin/mysql` runtime dengan `MYSQL_PWD`, dan `BLOCKED` tanpa cache. Memenuhi **AC-16** (jalur `BLOCKED`).
6. Test 8.4 untuk init, auth, endpoint, path, dan round trip format: `apps/daemon/tests/mysql/{init,auth,endpoints,paths,format}.rs`. Memenuhi **AC-2**, **AC-3**, **AC-4**, **AC-5**, **AC-6**.
7. Test 8.4 untuk readiness, stop, tool dari runtime, lifecycle, adopsi, edit config, dan binary log: `apps/daemon/tests/mysql/{readiness,stop,tools,lifecycle,adoption}.rs`. Memenuhi **AC-7**, **AC-8**, **AC-12**, **AC-13**.
8. Test 8.4 untuk detail dan doctor: `apps/daemon/tests/mysql/{open_with,doctor}.rs`. Memenuhi **AC-18**.

**Milestone 3: MySQL 9.7, isolasi, penolakan line, konformitas, evidence, dan catalog**

9. Suite milestone 2 untuk MySQL 9.7 (satu fungsi test per lini), test sentinel `apps/daemon/tests/mysql/sentinel.rs`, penolakan line `apps/daemon/tests/mysql/mismatch.rs`, dan data yang sudah ada `apps/daemon/tests/mysql/existing_data.rs`. Memenuhi **AC-3** sampai **AC-13** untuk 9.7, **AC-9**, **AC-10**, **AC-11**.
10. Dua test konformitas native di `apps/daemon/tests/provider_conformance.rs` dan `mysql` di daftar paket test batas langkah di file itu. Memenuhi **AC-14** (konformitas).
11. Di satu commit yang menutup bukti lifecycle kedua lini: entri MySQL di `providers/catalog.toml` pindah ke `[[provider]]`, `revision` dan `published_at` naik, `cargo xtask catalog check --update`, lalu `cargo xtask provider freeze providers/mysql`. Memenuhi **AC-1** (bagian catalog).
12. Guard evidence `native` dengan `covers` ID PRD dan `compatibility.json` per artefak di `apps/daemon/tests/mysql/compat.rs`, lalu `cargo xtask runtime compat-record` dan `cargo xtask codegen`. Evidence dihasilkan dari `provider.yaml` final; perubahan paket sesudahnya mewajibkan run evidence ulang. Memenuhi **AC-16**.

**Milestone 4: Handover**

13. Dua test launchd di `apps/daemon/tests/native_launchd.rs` (memakai harness langkah 5 lewat `#[path]`, seperti PostgreSQL), dan, hanya bersama record `PASS` keduanya, entri `official/mysql` di `HANDOVER_PROVEN` (`crates/core/src/supervisor/handover.rs`). Tanpa sesi GUI: test `BLOCKED`, entri tidak ditambah, dan itu dicatat di laporan. Memenuhi **AC-17**.

**Prasyarat**: semua fitur fase 02 sampai 06 yang dipakai sudah `done` di `main` (spec 0009, 0010, 0011, 0018, 0019, 0020, 0021, 0025, 0029, 0030, 0034, 0036, 0070). Langkah 2 butuh langkah 1. Langkah 5 sampai 12 butuh artefak MySQL di cache runtime. Langkah 11 butuh langkah 6 sampai 10 lulus. Langkah 12 butuh langkah 11, karena `manifestSha256` evidence harus sama dengan revision yang masuk catalog. Langkah 13 butuh langkah 5 dan sesi GUI.

**Test native dari cache xtask dan apa yang `BLOCKED`**: suite `mysql`, test konformitas, test install, dan spike MySQL berjalan dari cache tanpa unduhan (kedua artefak ada di cache Mac ini). Yang `BLOCKED` kalau syaratnya tidak ada: artefak yang hilang dari cache (`reason` `artifact <id> is not in the runtime cache`, `owner` `@ojiepermana`), dan test launchd tanpa sesi GUI (`reason` dari `gui_session`, `owner` `@ojiepermana`).

**Langkah manual** (bukan untuk sesi; masuk `verify.md` bagian `### UI / manual` saat build):

- Di aplikasi: buat instance MySQL dari Catalog, start, salin connection string, lalu Open In TablePlus, Sequel Ace, dan mysql (Terminal). Ini juga memeriksa bundle ID Sequel Ace dan bahwa kedua aplikasi menerima URL `mysql://`.
- Di aplikasi: ubah Max connections di halaman Configuration dan lihat dampak restart ditampilkan.
- Kalau test launchd `BLOCKED` di sesi: jalankan `cargo test -p stacklyd --test native_launchd -- --ignored --test-threads 1 mysql` di sesi GUI, lalu tambahkan entri `HANDOVER_PROVEN`.

**Sesi paralel**: fitur ini berjalan bersama fitur 39, 40, 41, dan 43, dan beberapa filenya milik bersama. Langkah 1 menyentuh modul format (`template.rs`, `validate.rs`, `fuzzing.rs`, dan `xtask/src/fuzz.rs`) dan `docs/providers/manifest-reference.md`, yang juga akan disentuh fitur 40 (`valkeyConf`) dan 43 (`xml`): ketiga perubahan itu menambah varian ke enum dan `match` yang sama, jadi harus berurutan, satu sesi pada satu waktu, dengan merge `main` di antaranya. Langkah 3 (`index.json`), langkah 4 (`providers.rs` spike, juga disentuh fitur 40 dan 43; `native_runtime_install.rs`, sama), langkah 10 (`provider_conformance.rs`, disentuh kelima fitur), langkah 11 (`catalog.toml` dan ledger, disentuh fitur 40, 41, 43; setiap perubahan butuh nomor `revision` sendiri), dan langkah 13 (`native_launchd.rs` dan `handover.rs`, disentuh kelima fitur) juga harus berurutan antar fitur. Langkah 2 menambah fungsi ke `crates/core/tests/provider_packages.rs`, yang juga disentuh setiap fitur provider lain, jadi bagian itu berurutan juga; sisanya di langkah 2 (folder `providers/mysql/` dan folder golden nya) aman paralel. Yang aman berjalan paralel dengan provider lain hanyalah folder paket itu dan langkah 5 sampai 9 serta 12 (folder `apps/daemon/tests/mysql/` dan `mysql.rs`). Di dalam fitur ini sendiri, langkah 6, 7, dan 8 menyentuh file test berbeda dan bisa paralel sesudah langkah 5, tetapi masing masing menjalankan `mysqld`, jadi disk dan memori membatasi sampai dua sesi.

## Consequences

**Positive**:

- MySQL lokal langsung aman: `root` selalu ber password, TCP hanya loopback, password tidak pernah lewat argv, dan tidak ada jendela waktu dengan `root` tanpa password di jaringan.
- Ready berarti server menjawab query sebagai akun tanpa hak, dan pesan MySQL sampai ke pengguna saat tidak ready.
- Data tidak pernah dijalankan binary lini lain, jadi upgrade di tempat yang diam diam oleh 9.7 tidak bisa terjadi.
- Port, socket, dan path tidak bisa bergeser lewat `SET PERSIST`, dan nilai persisted yang merusak (`lower_case_table_names`) tidak bisa membuat instance gagal start.
- Satu perubahan core saja, dan itu generik: `mysqlCnf` juga cocok untuk MariaDB dan Percona kelak.
- Recipe spike tulisan tangan terakhir untuk MySQL hilang; satu sumber kebenaran cara menjalankan MySQL adalah paketnya.

**Negative / tradeoffs**:

- Akun `stackly_readiness` tidak punya password. Yang menjaganya adalah folder run 0700 dan `skip_name_resolve = ON`; ini lebih lemah daripada `peer` PostgreSQL, karena MySQL macOS tidak punya `auth_socket`.
- Init bergantung pada perilaku MySQL yang dibuktikan spike di kedua lini tetapi bukan kontrak tertulis: `--init-file=/dev/stdin`, `LOAD_FILE` atas file 0600 lewat `@@replica_load_tmpdir`, dan statement akun (`ALTER USER`, `CREATE USER`, `GRANT`) di file init `--initialize`, yang menurut manual MySQL dibatasi. Kalau salah satunya berubah di rilis baru, init gagal tertutup (exit 1) dan test native menangkapnya, tetapi artefak baru itu tidak bisa masuk sebelum paket diperbarui. Jalur cadangannya (server sementara) dicatat di rationale.
- Akun `root` ber host IP tidak punya `AUDIT_ABORT_EXEMPT` dan `FIREWALL_EXEMPT`.
- `SET PERSIST` tidak bertahan melewati restart, berbeda dari MySQL biasa. Pengguna yang terbiasa dengannya harus memakai settings Stackly, dan variabel di luar empat settings belum bisa diubah secara tetap.
- Tiga akun `root` terpisah: mengganti password lewat SQL harus dilakukan untuk ketiganya.
- Tanpa reload: setiap perubahan settings butuh restart.
- `secure_file_priv` `NULL`: `LOAD DATA INFILE` dan `SELECT ... INTO OUTFILE` sisi server tidak tersedia.
- Binary log mati secara default; alat CDC butuh `binaryLog: true` dan restart.
- Setiap instance menjalankan satu proses `mysql` pendek setiap 10 detik selama hidup.
- Dua lini MySQL menggandakan waktu test native, dan setiap instance memakai ratusan MiB memori.
- Penanda data bergantung pada file `mysql_upgrade_history` milik MySQL, yang tumbuh di setiap upgrade patch dan dibaca paling banyak 4096 byte.

**Neutral**:

- `providers/mysql` menjadi provider kedua yang memakai `runExecutable`, `materializeSecret`, `stdin.text`, `dataMarker`, dan endpoint `unix`, dan yang pertama tanpa capability reload di antara provider database.
- Zona waktu server tetap `+00:00`, dan nama tabel mengikuti `lower_case_table_names` yang dipilih MySQL saat init (2 di volume macOS bawaan).
- `mysqladmin` ada di artefak tetapi tidak dipakai paket.
- Test lifecycle MySQL adalah yang pertama menangkap balapan start supervisor (`running` terlihat sesaat dengan readiness `unknown`), karena `mysqld` cukup lambat siap untuk memperlihatkannya. Cacat itu diperbaiki di core untuk semua provider (`0de32eda`, `crates/core/src/supervisor/startup.rs`), di luar ukuran RISK-07 fitur ini. Akibat perbaikan yang dicatat: penanda log `Started` dan `status_changed` kini terbit sebelum commit `running`, dan kalau commit identitas gagal, penanda itu tertulis untuk proses yang dibuang.
- Komentar `my.cnf` dan satu kalimat README yang pertama dibangun mengatakan suntingan tangan hilang, padahal yang terjadi adalah drift. Keduanya dibetulkan sebelum paket terbit (`a046da9f`), jadi revision `1.0.0` yang terbit sudah memuat teks yang benar. Perilakunya tidak pernah berubah: tidak ada suntingan yang ditimpa diam diam.

## Follow-up

- [ ] `/architect` update spec 0009 (pemiliknya): catat `mysqlCnf` di tabel escaper *Template config* dan centang bagian fitur 42 di kalimat format yang direncanakan.
- [ ] `/architect` update spec 0003 (pemiliknya): baris MySQL tabel *Langkah spike per provider* tidak dipakai lagi; bagian fitur 42 di Follow-up "Fitur 40, 42, dan 43" dicentang.
- [ ] Lewat lead, untuk spec 0036, 0030, 0017, 0018, 0034, dan 0070: centang bagian fitur 42 di baris Follow-up masing masing sesudah build.
- [ ] Fitur 51 dan 52: dump logical melewati akun `stackly_readiness` dan ketiga akun `root`; instance hasil restore membuat akun itu lewat init biasa. Bagian restore AC-081 dibuktikan di sana.
- [ ] Fitur 57: adoption data MySQL luar membuat akun `stackly_readiness` dan `root` ber host IP sebelum start pertama, dan memutuskan apa yang terjadi pada `mysqld-auto.cnf` bawaan data itu.
- [ ] Fitur 58: rebind patch dalam satu lini (8.4.11 ke 8.4.x) memakai `rebind::check` yang ada; pindah lini tetap ditolak sampai ada prosedur migrasi teruji.
- [ ] Pemilik `stackly-core` (preflight): penanda `mysql_upgrade_history` yang melewati 4096 byte membuat start ditolak; pertimbangkan batas yang lebih besar atau pembacaan ekor file.
- [ ] Settings tambahan kalau pengguna membutuhkannya, masing masing additive lewat update spec ini: `sqlMode`, character set, zona waktu, slow query log (dengan tampilan di log instance), dan folder impor untuk `secure_file_priv`.
- [ ] Remedy tambahan untuk folder data yang dikunci `mysqld` lain, sesudah pesan error nya di macOS dicatat dari run nyata.
- [ ] `/sync` setelah build: catat paket MySQL, akun readiness, `--persisted-globals-load=OFF`, dan perintah test native di root `AGENTS.md`.
- [x] Pemilik paket `providers/mysql`: baris komentar kedua `templates/my.cnf.tmpl` ("Edits to this file are lost on the next start.") dibetulkan di `a046da9f`, sebelum langkah 11 (catalog dan freeze), jadi masuk revision `1.0.0` (keputusan lead 2026-11-12, cara yang sama dengan langkah 7a spec 0041). Golden render dan plan MySQL dan fixture `create-form-mysql.json` ikut ditulis ulang, dan paket yang dibekukan memuat komentar baru (`manifest_sha256` `77a41842621898d0e4ec378ae2cea628a5de122444b08862ab45e2d83f7a950c`). Dari verify 2026-11-12, temuan rendah.
- [x] Pemilik paket `providers/mysql` (README): "hand edits there are lost" diganti dengan penjelasan drift yang sama, di commit `a046da9f`.
- [ ] Pemilik paket `providers/postgresql` (revision paket berikutnya sesudah `1.0.2`): kalimat salah yang sama, "Edits to this file are lost on the next start.", masih ada di `templates/pg_hba.conf.tmpl`, `templates/pg_ident.conf.tmpl`, dan `templates/postgresql.conf.tmpl`. Ganti dengan kalimat drift seperti di `my.cnf.tmpl`. Template ikut `manifest_sha256`, jadi ini butuh revision baru dan golden render yang ditulis ulang. `providers/postgresql/README.md` juga memuat "hand edits there are lost"; README tidak ikut digest, jadi boleh dibetulkan tanpa revision. Pemilik paket `providers/clickhouse` (revision paket berikutnya sesudah `1.0.0`): kalimat yang sama ada di `templates/config.xml.tmpl`; Follow-up nya ditulis di spec 0043. Ditemukan saat komentar `my.cnf` dibetulkan, 2026-11-12.
- [ ] Pemilik spec 0004 dan spec 0010 (lewat `/architect`, bukan spec ini): readiness sesudah lifecycle event `adopted` tidak sama di dua spec. Tabel readiness spec 0004 memberi `not_ready`, sedangkan spec 0010 menulis `unknown` dengan reason `daemon_restarted` untuk adopsi sesudah restart daemon, dan itulah yang dibangun. Putuskan satu, lalu samakan spec yang lain. Saudara dari balapan start yang diperbaiki `0de32eda`; dicatat debugger `dbg-f42-lifecycle`, tidak diubah.
- [ ] Pemilik spec 0010 (lewat `/architect`, lalu `/debug` kalau diputuskan cacat): di jalur exit, `finish_process` mengosongkan slot proses sebelum commit exit, jadi ada jendela pendek di mana instance masih `running` di registry tanpa proses di slot. Ini cermin dari balapan start yang sudah diperbaiki. Putuskan apakah urutannya dibalik seperti di `startup.rs`. Dicatat debugger `dbg-f42-lifecycle`, tidak diubah.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, keputusan yang diambil dengan opsi rekomendasi, dan hasil spike lokal ada di [rationale.md](rationale.md).
