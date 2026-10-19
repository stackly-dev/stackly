# 0019. Provider PostgreSQL

**Date**: 2026-10-19
**Status**: Proposed

## Summary

Spec ini memutuskan paket provider resmi `official/postgresql` untuk PostgreSQL 17 dan 18: cluster dibuat dengan `initdb` dari runtime yang cocok, superuser `postgres` selalu login dengan password (SCRAM), dan readiness memakai query sungguhan `SELECT 1` lewat `psql` milik runtime itu sebagai role khusus `stackly_readiness` yang berhak minimum dan hanya bisa masuk tanpa password lewat socket privat instance (peer auth di folder 0700 yang hanya bisa dijangkau user OS pemiliknya). Port, alamat, socket, dan lokasi file config yang dimiliki Stackly dikirim lewat argumen command line `postgres`, sehingga `ALTER SYSTEM` tidak bisa menggesernya, dan stop memakai fast shutdown (SIGINT). Data directory membawa penanda versi mayor (`PG_VERSION`), sehingga start dengan binary mayor lain ditolak sebelum proses dijalankan. Semua ini muat di manifest `stackly/v1alpha1` dengan empat tambahan kecil dan additive di core yang dirinci di sini, ditambah satu field dari spec 0018.

## Requirements

**User stories**:

- Sebagai developer, saya ingin membuat database PostgreSQL 17 atau 18 lokal yang langsung aman (password, bukan `trust`) dan siap dipakai, supaya saya tidak perlu menyetel `pg_hba.conf` atau `initdb` sendiri.
- Sebagai developer, saya ingin status ready berarti database benar benar menjawab query, bukan sekadar port terbuka, supaya aplikasi saya tidak gagal konek saat Stackly bilang siap.
- Sebagai developer, saya ingin stop selesai cepat walau masih ada koneksi terbuka, dan data saya tidak pernah dijalankan oleh binary mayor lain, supaya data lokal saya tidak rusak (BR-009).
- Sebagai developer fitur 20, 21, 23, dan 26, saya ingin provider PostgreSQL yang hanya berupa paket (manifest, settings schema, template) di atas engine fitur 9, supaya form, connection, dan aksi lain tidak butuh kode khusus PostgreSQL.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-035):

- **AC-1**: Paket `providers/postgresql` (ID `official/postgresql`, software `postgresql`, revision `1.0.0`, category `database`) berisi persis file di *Paket*, dan `cargo xtask provider check` mencetak satu baris `ok official/postgresql 1.0.0 <manifest_sha256>`. `cargo xtask codegen --check` lolos dengan `versions.json` yang dibangkitkan dari inventory dan memuat tepat dua artefak `available` (`postgresql-17.11-r2-macos-aarch64` line `17`, `postgresql-18.6-r2-macos-aarch64` line `18`). Render ketiga config dan `init_plan`, `run_plan`, serta `reload_plan` untuk context golden spec 0009 sama byte demi byte dengan file golden. Setelah fitur 17 membuat `providers/catalog.toml`, entri PostgreSQL pindah dari `[[planned]]` ke `[[provider]]` dan `cargo xtask catalog check` lolos (REQ-035, bagian manifest dari REQ-029).
- **AC-2**: Format template `postgresqlConf` mengeluarkan teks sebagai `'...'` dengan `'` menjadi `''` dan `\` menjadi `\\`, integer sebagai desimal, dan boolean sebagai `on` atau `off`; teks yang memuat karakter kontrol (U+0000 sampai U+001F, U+007F sampai U+009F) ditolak dengan `unrepresentable_value` yang menyebut field atau token, tanpa nilainya. Untuk teks `a'b\c d` dan teks Unicode `ü 空` yang dirender ke parameter teks bebas `log_line_prefix`, `postgres -C log_line_prefix` dari runtime 18.6 membaca kembali nilai yang persis sama. Format `plain` menolak setiap tag output dan tag blok saat validasi (`template_type`, dengan baris dan kolom), menerima komentar `{{! }}` dan `\{{`, dan mengeluarkan isi template apa adanya.
- **AC-3**: Langkah `runExecutable` menerima `stdin: { text: <literal> }` sebagai pilihan selain `stdin: { secret: <nama> }` (tepat satu key). Teks 1 sampai 16384 byte UTF-8; `${` di mana pun ditolak (`token_not_allowed_here`), karakter kontrol selain `\n` dan `\t` ditolak (`invalid_value`), dan `stdin` di probe `exec`, stop `command`, atau reload `run` tetap ditolak. Plan memuat `Stdin::Text(String)`, executor init menulis byte teks itu persis lalu menutup stdin, dan `Debug` serta `Serialize` plan menampilkan teksnya (bukan secret). Mengubah satu byte teks mengubah `manifest_sha256`. Untuk single user mode PostgreSQL setiap baris adalah satu statement, jadi teks SQL di paket menulis satu statement per baris.
- **AC-4**: `x-stackly.printableOnly: true` hanya sah pada field `secret: true` (selain itu `settings_schema_rule`). `SecretService::put` dengan konteks field itu menolak nilai `Value` atau `CopyFrom` yang memuat karakter kontrol (U+0000 sampai U+001F, U+007F sampai U+009F) dengan `secret.invalid_value` ber `details` `field` dan `reason: control_characters`, tanpa menyebut nilai dan tanpa menulis ke Keychain (`Value` bahkan ditolak sebelum Keychain disentuh; `CopyFrom` harus membaca nilai sumber lebih dulu); `Generate` selalu lolos. Nilai yang lolos sampai ke `initdb` utuh: login superuser dengan password berisi spasi, `'`, `"`, dan `\` berhasil.
- **AC-5**: Init sebuah instance menjalankan langkah dalam urutan *Inisialisasi cluster*: folder dibuat, ketiga config dirender, file `superuser-password` (0600) dibuat di `<run>/secrets/`, `initdb` berjalan, lalu `postgres --single` membuat role `stackly_readiness`. Setelah init, berhasil atau gagal, file `superuser-password` tidak ada lagi. Kalau folder data sudah berisi file, `initdb` menolak, file yang ada tidak berubah, dan init berakhir `failed` dengan reason `init_step_failed`, langkah `initdb`, exit code 1, dan cuplikan error. Kalau langkah role gagal, init juga `failed` dan instance tidak pernah di start (BR-008).
- **AC-6**: Tidak ada `trust` yang tidak disengaja: `${instance.config}/pg_hba.conf` dan `pg_hba.conf` buatan `initdb` di folder data tidak memuat baris dengan metode `trust`. Superuser `postgres` tanpa password ditolak lewat socket dan lewat TCP, dan dengan nilai secret dari Keychain diterima di keduanya. `stackly_readiness` ditolak lewat TCP dan ke database selain `postgres`, koneksi replication ditolak, dan nilai password tidak pernah muncul di argv proses mana pun, spool, atau log daemon (REQ-035, PRD 22.1, VAL-018).
- **AC-7**: Locale, encoding, dan zona waktu eksplisit dan sama di kedua lini: database `postgres` dan `template1` punya `datlocprovider = 'b'`, `datlocale = 'C.UTF-8'`, encoding `UTF8`, `datcollate` dan `datctype` `C`; `SHOW data_checksums` `on`; `TimeZone` dan `log_timezone` `UTC`; `lc_messages` `C`. Hasilnya tetap sama walau daemon dijalankan dengan `LANG=de_DE.UTF-8` dan `TZ=Asia/Jakarta`.
- **AC-8**: Postmaster hanya listen di host dan port endpoint `client` dan di socket `<run>/.s.PGSQL.<port>` (file socket 0700 di folder 0700, dengan file lock `<run>/.s.PGSQL.<port>.lock` buatan PostgreSQL di sampingnya); `lsof` tidak menunjukkan listener lain. `port`, `listen_addresses`, `unix_socket_directories`, `unix_socket_permissions`, `config_file`, `hba_file`, `ident_file`, `logging_collector`, dan `log_destination` datang dari argv: setelah `ALTER SYSTEM SET port = <port lain>` lalu stop dan start, `SHOW port` tetap port Stackly dan readiness lolos. Port `Auto` tersimpan dan nama socket mengikutinya.
- **AC-9**: Readiness memakai probe `exec` yang menjalankan `psql` dari runtime instance lewat socket instance sebagai `stackly_readiness` ke database `postgres` dengan query `SELECT 1`, dengan argv persis di *Readiness dan health*. Instance baru `ready` setelah query itu berhasil. Saat port TCP tetap menerima koneksi tetapi query gagal (`ALTER ROLE stackly_readiness NOLOGIN`), readiness menjadi `not_ready` setelah 2 kegagalan berturut turut dengan detail yang memuat pesan PostgreSQL, dan kembali `ready` setelah `LOGIN` dipulihkan. Role itu bukan superuser, tanpa `CREATEDB`, `CREATEROLE`, `REPLICATION`, dan `BYPASSRLS`, dengan connection limit 3, `default_transaction_read_only = on`, dan `statement_timeout = 2s`; `CREATE TABLE` sebagai role itu gagal.
- **AC-10**: Stop normal mengirim `SIGINT` (fast shutdown) ke postmaster yang identitasnya terverifikasi. Dengan satu sesi klien yang sedang menjalankan `SELECT pg_sleep(600)`, stop selesai dalam 10 detik (jauh di bawah `stopSeconds` 60) dan instance `stopped`, tanpa `SIGKILL`. `postmaster.pid`, file socket, dan file lock socket hilang, dan start berikutnya mencatat `database system was shut down at` (tanpa crash recovery).
- **AC-11**: Paket mendeklarasikan `compatibility.dataMarker { file: PG_VERSION, pattern: "([0-9]+)" }` tanpa `lineCompatibility`. Instance yang diinisialisasi dengan runtime 17 lalu (lewat penulisan registry khusus test) diikat ke runtime 18 gagal start dengan `supervisor.data_incompatible` (`outcome` `incompatible`, `data` `17`, `runtime` `18`) sebelum spawn: tidak ada observasi proses baru dan tidak ada proses `postgres`. Arah 18 ke 17 sama. `PG_VERSION` yang dihapus menghasilkan `marker_missing`, dan yang diubah menjadi `16` menghasilkan `marker_mismatch` (BR-009, bagian penolakan AC-035).
- **AC-12**: Setiap program di plan init, run, dan probe adalah `ExecutablePath` di dalam root runtime instance (`bin/initdb`, `bin/postgres`, `bin/psql`), dan env child hanya env basis spec 0009 ditambah env manifest. Dengan daemon yang dijalankan memakai `PATH` berisi folder umpan (`psql`, `initdb`, dan `postgres` palsu yang menulis file penanda), create, start, readiness, dan stop berhasil dan file penanda tidak pernah dibuat (PRD 22.1, tool admin dari runtime yang cocok).
- **AC-13**: Lewat daemon (`SpawnedDaemon`) untuk PostgreSQL 17 dan 18 masing masing: `instance.create` dengan secret `superuserPassword` bangkitan, init sukses dengan `data.initializedLine` di `instance-manifest.json` sama dengan line runtime, `instance.start` membuat lifecycle `running` dengan readiness `not_ready` lalu `ready`, `instance.get` menampilkan lifecycle, desired, readiness, health, dan proses secara terpisah, baris log PostgreSQL (dengan cap waktu UTC) sampai ke `LogSink` perekam test (test ini berjalan in process lewat `stacklyd::server::start`, karena IPC minimal belum punya method log), baris yang ditulis sebelum stop terbaca lagi setelah start berikutnya, dan `instance.stop` membuat `stopped`. Reload lewat `execute_reload` setelah `logStatement` diubah menjadi `all` membuat `SHOW log_statement` bernilai `all` dengan PID postmaster yang sama. Daemon yang dibunuh `SIGKILL` saat instance `ready` lalu dijalankan lagi mengadopsi postmaster yang sama (PID dan waktu start sama), karena postmaster dan semua child nya memegang lock instance di fd 3 (bukti provider nyata spec 0010 AC-9 dan AC-14).
- **AC-14**: Setiap test native fitur ini memasang guard evidence `native` spec 0002 dengan `covers` ID PRD yang dibuktikannya (REQ-035, BR-009, AC-035, JOURNEY-01). Untuk setiap artefak, satu test per mayor menulis evidence `native` dengan `covers` BR-025, subject `{ provider: postgresql, version, digest, source: managed }`, status `PASS`, dan file `compatibility.json` di `artifact_dir()` berbentuk *Evidence kompatibilitas*, dengan check `init`, `start`, `readiness`, `stop`, `reload`, `adoption`, dan `major_mismatch_refusal`. Artefak yang tidak ada di cache runtime membuat test `BLOCKED` dengan alasan `artifact <id> is not in the runtime cache` dan owner `@ojiepermana`, tanpa unduhan.
- **AC-15**: Setelah update spec 0009 dari fitur 18 menambah `runtime.versionCheck`, manifest mendeklarasikan `versionCheck { args: ["--version"], pattern: "postgres \\(PostgreSQL\\) ([0-9]+\\.[0-9]+)", timeoutSeconds: 10 }`, dan untuk kedua artefak capture group nya sama dengan `version` artefak (`17.11`, `18.6`).

Status test semua AC mulai dari `NOT_RUN`. Lulus di fixture bukan evidence native, dan artefak, sesi GUI, atau signing yang tidak tersedia dicatat `BLOCKED`.

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md): readiness lewat probe `exec` yang menjalankan `psql` milik runtime dengan peer auth untuk role `stackly_readiness` di socket privat; role itu dibuat saat init lewat single user mode (`postgres --single`) dengan stdin literal; SCRAM untuk semua role lain dan tanpa `trust`; locale provider builtin `C.UTF-8` dengan encoding UTF8 dan checksum data; parameter milik Stackly di argv dan sisanya di `postgresql.conf` yang dirender ke folder config; `ALTER SYSTEM` tetap boleh; fast shutdown lewat `SIGINT`; superuser dan database awal tetap bernama `postgres`; format `plain` untuk file statis; flag `printableOnly` untuk password; zona waktu tetap UTC; dan runtime untuk test native dipasang lewat fixture installer fitur 18.

Paket PostgreSQL hanya data (manifest, schema, template). Semua yang tidak muat di primitive `stackly/v1alpha1` hari ini masuk sebagai empat perubahan core additive yang generik (bukan kode khusus PostgreSQL) dan dihitung sebagai ukuran RISK-07.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Skill async relevan hanya untuk test native yang menunggu readiness dan event; perubahan core di sini sync dan tanpa IO.

## Feature design

### Batas fitur

- **Di dalam**: paket `providers/postgresql`, format template `postgresqlConf` dan `plain`, `stdin.text` di `runExecutable`, flag `printableOnly` untuk secret field, detail kegagalan probe `exec`, fixture `pin_package`, golden test, test native lewat daemon untuk 17 dan 18, evidence kompatibilitas, dan entri catalog.
- **Di luar**: form create dan preflight folder kosong (fitur 20), connection string dan tombol copy (fitur 21), force stop (fitur 26), edit config (fitur 29), backup logical dan restore (fitur 51 dan 52), adoption data luar (fitur 57), rebind (fitur 58), `pg_upgrade` (Deferred, PRD 22.1), TLS, replication, dan bind LAN.

### Paket

```text
providers/postgresql/
├── provider.yaml
├── versions.json                 # dibangkitkan cargo xtask codegen dari runtimes/inventory.toml
├── schemas/
│   └── settings.schema.json
├── templates/
│   ├── pg_hba.conf.tmpl          # format plain
│   ├── pg_ident.conf.tmpl        # format plain
│   └── postgresql.conf.tmpl      # format postgresqlConf
└── README.md                     # catatan untuk pengguna dan kontributor, tidak dibaca loader
```

`README.md` menjelaskan role `stackly_readiness` (jangan dihapus atau diubah), parameter yang dimiliki Stackly di argv, file di folder config yang ditulis ulang sebelum setiap start, dan bahwa `ALTER SYSTEM` tetap bisa dipakai untuk parameter lain.

### Manifest

```yaml
apiVersion: stackly/v1alpha1
kind: Provider
metadata:
  id: official/postgresql
  software: postgresql
  displayName: PostgreSQL
  revision: "1.0.0"
  category: database
  description: PostgreSQL relational database. Each instance is its own cluster with a password protected superuser, a loopback port, and a private Unix socket.
compatibility:
  platforms:
    - { os: macos, arch: aarch64 }
  dataMarker: { file: PG_VERSION, pattern: "([0-9]+)" }
runtime:
  executable: bin/postgres
  executionMode: foreground
capabilities:
  reload: true
  multiInstance: true
  persistence: true
  backup: [cold]
resources:
  endpoints:
    client: { transport: tcp, purpose: client, defaultHost: "127.0.0.1", defaultPort: 5432 }
    socket: { transport: unix, purpose: client, socketName: ".s.PGSQL.${endpoint.client.port}" }
configFiles:
  pgHba:
    template: templates/pg_hba.conf.tmpl
    format: plain
    destination: "${instance.config}/pg_hba.conf"
  pgIdent:
    template: templates/pg_ident.conf.tmpl
    format: plain
    destination: "${instance.config}/pg_ident.conf"
  postgresqlConf:
    template: templates/postgresql.conf.tmpl
    format: postgresqlConf
    destination: "${instance.config}/postgresql.conf"
lifecycle:
  initialize:
    - { primitive: createManagedDirectories }
    - { primitive: renderConfig, file: pgHba }
    - { primitive: renderConfig, file: pgIdent }
    - { primitive: renderConfig, file: postgresqlConf }
    - { primitive: materializeSecret, secret: superuserPassword, destination: superuser-password }
    - primitive: runExecutable
      executable: "${runtime.executables.initdb}"
      args:
        - "--pgdata=${instance.data}"
        - "-Upostgres"
        - "--pwfile=${instance.secrets}/superuser-password"
        - "--auth-local=scram-sha-256"
        - "--auth-host=scram-sha-256"
        - "--encoding=UTF8"
        - "--locale=C"
        - "--locale-provider=builtin"
        - "--builtin-locale=C.UTF-8"
        - "--data-checksums"
        - "--no-instructions"
      cwd: "${instance.temp}"
      timeoutSeconds: 90
    - primitive: runExecutable
      executable: "${runtime.executable}"
      args: ["--single", "-D", "${instance.data}", "-c", "exit_on_error=on", "template1"]
      cwd: "${instance.temp}"
      timeoutSeconds: 30
      stdin:
        text: |
          CREATE ROLE stackly_readiness WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 3;
          ALTER ROLE stackly_readiness SET default_transaction_read_only = on;
          ALTER ROLE stackly_readiness SET statement_timeout = '2s';
          COMMENT ON ROLE stackly_readiness IS 'Stackly readiness probe. Dropping or changing this role stops the instance from becoming ready.';
  start:
    executable: "${runtime.executable}"
    args:
      - "-D"
      - "${instance.data}"
      - "--config-file=${instance.config}/postgresql.conf"
      - "--hba-file=${instance.config}/pg_hba.conf"
      - "--ident-file=${instance.config}/pg_ident.conf"
      - "--listen-addresses=${endpoint.client.host}"
      - "--port=${endpoint.client.port}"
      - "--unix-socket-directories=${endpoint.socket.socketDir}"
      - "--unix-socket-permissions=0700"
      - "--logging-collector=off"
      - "--log-destination=stderr"
    cwd: "${instance.data}"
  stop: { strategy: gracefulProcess, signal: int }
  reload:
    render: [pgHba, pgIdent, postgresqlConf]
    action: signal
    signal: hup
    verify: readiness
readiness:
  probe:
    type: exec
    command:
      executable: "${runtime.executables.psql}"
      args:
        - "--no-psqlrc"
        - "--no-password"
        - "--quiet"
        - "--tuples-only"
        - "--no-align"
        - "--set=ON_ERROR_STOP=1"
        - "--host=${endpoint.socket.socketDir}"
        - "--port=${endpoint.client.port}"
        - "--username=stackly_readiness"
        - "--command=SELECT 1"
      cwd: "${instance.temp}"
      env: { PGDATABASE: postgres, PGCONNECT_TIMEOUT: "2", PGAPPNAME: stackly-readiness }
    successExitCodes: [0]
  intervalMilliseconds: 500
  attemptTimeoutMilliseconds: 3000
health:
  intervalSeconds: 10
  attemptTimeoutMilliseconds: 3000
  failureThreshold: 3
timeouts: { initSeconds: 180, readinessSeconds: 120, recoverySeconds: 300, stopSeconds: 60, reloadSeconds: 30 }
backup:
  cold: { paths: [data] }
permissions:
  managedFilesystem: [data, config, temp, run]
  executeRuntimeBinary: true
  additionalExecutables: { initdb: bin/initdb, psql: bin/psql }
```

Catatan per bagian:

- `runtime.versionCheck` ditambahkan begitu update spec 0009 dari fitur 18 masuk (AC-15); sebelum itu field ini tidak ditulis, karena v1alpha1 menolak field yang tidak dikenal.
- `health` tanpa `probe` memakai probe readiness. Karena interval health sama dengan `READINESS_REFRESH_INTERVAL` spec 0010 (10 detik), satu percobaan `psql` dipakai untuk keduanya.
- `logs` sengaja tidak ada di `managedFilesystem`: PostgreSQL menulis log ke stderr (`--logging-collector=off` dan `--log-destination=stderr` di argv, sehingga `ALTER SYSTEM` tidak bisa memindahkan log ke folder data), dan stderr ditangkap spool spec 0010.
- Superuser ditulis `-Upostgres` (opsi dan nilai menempel), single user mode memakai database `template1`, dan probe memilih database lewat env `PGDATABASE`, supaya tidak ada argumen di plan mana pun (init, start, probe) yang sama persis dengan `postgres` atau berbentuk `<apa pun>=postgres`. `ensure_not_in_argv` spec 0011 menolak argumen seperti itu kalau password pengguna kebetulan `postgres`, dan password itu umum di lingkungan development; `resolve_for_start` menyelesaikan password superuser di setiap start, jadi argv probe di dry run `run_plan` ikut diperiksa.
- Semua path (folder data, file config, folder socket) hanya ada di argv, tidak pernah di dalam file config, jadi path dengan spasi, `'`, atau Unicode tidak butuh escape apa pun.

### Settings schema

| Field | Tipe | Default | Batas | `apply` | Visibility | Dipakai di |
| --- | --- | --- | --- | --- | --- | --- |
| `superuserPassword` | string, `secret: true`, `printableOnly: true` | tidak ada (wajib; create memakai `generate` kecuali pemanggil memberi nilai) | panjang 8 sampai 128 | `immutableAfterInit` | basic | `materializeSecret` untuk `initdb --pwfile` |
| `maxConnections` | integer | 100 | 5 sampai 1000 | `restart` | advanced | `max_connections` |
| `sharedBuffersMb` | integer | 128 | 16 sampai 16384 | `restart` | advanced | `shared_buffers` (dalam MB) |
| `logStatement` | string, `enum` `none`, `ddl`, `mod`, `all` | `none` | | `reload` | basic | `log_statement` |
| `logMinDurationStatementMs` | integer | -1 | -1 sampai 3600000 | `reload` | advanced | `log_min_duration_statement` (-1 berarti mati) |

`title` dan `description` (bahasa Inggris, ditulis untuk pengguna): `Superuser password` ("Password of the postgres superuser, set once when the cluster is created."), `Max connections` ("Most client connections the server accepts at once."), `Shared buffers (MB)` ("Memory the server reserves for caching data pages."), `Log statements` ("Which SQL statements are written to the instance log. Statements that set passwords are logged as typed."), dan `Log slow statements (ms)` ("Log every statement that runs at least this long; -1 turns it off."). Root `title` `PostgreSQL settings`, `required: ["superuserPassword"]`, `additionalProperties: false`.

Nilai bawaan di luar settings sengaja tetap: nama superuser `postgres`, database awal `postgres`, encoding dan locale, checksum data, zona waktu `UTC`, dan semua parameter yang dimiliki Stackly. Masing masing bisa menjadi settings lewat update spec ini secara additive kalau pengguna membutuhkannya (Follow-up).

### Template config

`templates/postgresql.conf.tmpl` (format `postgresqlConf`):

```text
{{! Rendered before every start and reload. }}
# Managed by Stackly. Edits to this file are lost on the next start.
# Port, listen address, socket folder, and the hba and ident files are set
# on the postgres command line by Stackly and cannot be changed here.
cluster_name = {{ instance.name }}
max_connections = {{ settings.maxConnections }}
shared_buffers = {{ settings.sharedBuffersMb }}MB
password_encryption = 'scram-sha-256'
ssl = off
log_line_prefix = '%m [%p] %q%u@%d '
log_timezone = 'UTC'
timezone = 'UTC'
log_statement = {{ settings.logStatement }}
log_min_duration_statement = {{ settings.logMinDurationStatementMs }}
datestyle = 'iso, mdy'
lc_messages = 'C'
lc_monetary = 'C'
lc_numeric = 'C'
lc_time = 'C'
default_text_search_config = 'pg_catalog.english'
```

`templates/pg_hba.conf.tmpl` (format `plain`):

```text
# Managed by Stackly. Edits to this file are lost on the next start.
# Only the readiness probe role logs in without a password, and only through
# the instance's private Unix socket, as the OS user that owns that socket.
# TYPE  DATABASE  USER               ADDRESS        METHOD
local   postgres  stackly_readiness                 peer map=stackly
local   all       stackly_readiness                 reject
host    all       stackly_readiness  all            reject
local   all       all                               scram-sha-256
host    all       all                127.0.0.1/32   scram-sha-256
host    all       all                ::1/128        scram-sha-256
```

`templates/pg_ident.conf.tmpl` (format `plain`):

```text
# Managed by Stackly. Edits to this file are lost on the next start.
# Any OS user that can reach the private socket folder (only its owner)
# maps to the readiness probe role, and to no other role.
# MAPNAME  SYSTEM-USERNAME  PG-USERNAME
stackly    /^.*$            stackly_readiness
```

- Tidak ada baris `replication` di `pg_hba.conf`, sehingga koneksi replication selalu ditolak (`all` di kolom database tidak mencakup replication).
- Peta `stackly` memakai regex karena manifest tidak tahu nama user OS. Batasnya bukan regex itu, tetapi izin folder socket 0700 milik user (spec 0004) dan `unix_socket_permissions = 0700`.
- `shared_buffers = {{ settings.sharedBuffersMb }}MB` menghasilkan `128MB` tanpa kutip, bentuk yang diterima PostgreSQL.

### Inisialisasi cluster

Urutan langkah `InitPlan` (dijalankan `execute_init` spec 0010 di bawah lock instance dan `initSeconds` 180, cukup untuk kedua batas langkah, 90 dan 30 detik, karena spec 0010 memberi setiap langkah `min(batas langkah, sisa deadline)`):

| No | Langkah | Hasil | Gagal |
| --- | --- | --- | --- |
| 1 | `createManagedDirectories` | `data`, `config`, `temp`, dan folder run ada, 0700 | `init_step_failed` |
| 2 sampai 4 | `renderConfig` `pgHba`, `pgIdent`, `postgresqlConf` | tiga file 0600 di folder config | render gagal sebelum efek samping (nilai yang tidak bisa direpresentasikan) |
| 5 | `materializeSecret` `superuserPassword` | `<run>/secrets/superuser-password` 0600 berisi byte password persis | `secret.*` dari spec 0011 |
| 6 | `initdb` | cluster baru di folder data; `PG_VERSION` berisi mayor; superuser `postgres` dengan password SCRAM; `pg_hba.conf` di folder data tanpa `trust` | `init_step_failed` (exit code dan cuplikan); `initdb` membersihkan isi folder data yang ia buat |
| 7 | `postgres --single ... template1` dengan stdin teks | role `stackly_readiness` beserta setelannya; `exit_on_error=on` membuat error SQL mana pun exit 1 | `init_step_failed` |
| akhir | guard `SecretFile` dilepas | file `superuser-password` dihapus, berhasil atau gagal | |

- `initdb` membaca baris pertama `--pwfile` lalu membuang `\r\n` di akhirnya. Karena itu field password memakai `printableOnly`: nilai dengan baris baru akan terpotong diam diam, sehingga password di Keychain berbeda dengan password di database.
- `initdb` menemukan `postgres` dan `share/postgresql/` relatif terhadap dirinya sendiri, jadi selalu memakai runtime yang sama (spec 0003 membuktikan relokasinya).
- Single user mode dijalankan setelah `initdb` keluar, dengan config bawaan `initdb` di folder data (tidak listen di port mana pun). `template1` dipakai karena `CREATE ROLE`, `ALTER ROLE ... SET`, dan `COMMENT ON ROLE` berlaku untuk seluruh cluster.

### Start dan parameter milik Stackly

| Parameter | Sumber | Kenapa di argv |
| --- | --- | --- |
| data directory | `-D ${instance.data}` | path utuh tanpa escape |
| `config_file`, `hba_file`, `ident_file` | file di `${instance.config}` | file buatan Stackly, bukan file buatan `initdb` di folder data |
| `listen_addresses` | `${endpoint.client.host}` (loopback, dijaga plan spec 0009) | tidak bisa digeser `ALTER SYSTEM` |
| `port` | `${endpoint.client.port}` (`Fixed` atau `Auto` spec 0010) | sama; nama socket `.s.PGSQL.<port>` diturunkan PostgreSQL dari port ini |
| `unix_socket_directories` | `${endpoint.socket.socketDir}` (folder run) | sama |
| `unix_socket_permissions` | `0700` | sama |
| `logging_collector`, `log_destination` | `off`, `stderr` | log harus tetap di stderr supaya spool, cuplikan kegagalan, dan redaksi spec 0010 bekerja |

Parameter dari command line mengalahkan `postgresql.conf` dan `postgresql.auto.conf` (tempat `ALTER SYSTEM` menulis), jadi parameter di atas selalu milik Stackly, sedangkan parameter lain yang diubah pengguna lewat `ALTER SYSTEM` tetap berlaku seperti biasa di PostgreSQL. `RunPlan.render` (spec 0010) menulis ulang ketiga file config sebelum setiap spawn, jadi suntingan tangan di folder config hilang pada start berikutnya.

Folder run (`instance_run_dir`) berada di runtime root spec 0004 yang dibangun dari temp dir user atau hash FNV-1a, jadi path nya tidak memuat koma, kutip, atau spasi, yang akan memecah daftar `unix_socket_directories`. Golden test dan unit test layout menjaganya.

### Auth lokal

| Role | Login | Cara masuk | Hak |
| --- | --- | --- | --- |
| `postgres` | ya | `scram-sha-256` lewat socket instance dan TCP loopback | superuser, pemilik cluster |
| `stackly_readiness` | ya | `peer` dengan peta `stackly`, hanya lewat socket instance dan hanya ke database `postgres`; `reject` di tempat lain; tanpa password, jadi SCRAM selalu gagal | hanya hak `PUBLIC` bawaan; read only secara default; connection limit 3; `statement_timeout` 2 detik |
| role buatan pengguna | sesuai pengguna | `scram-sha-256` lewat socket dan TCP loopback | urusan pengguna |

Karena peta `stackly` menerima user OS mana pun, batas sebenarnya untuk role readiness adalah folder socket 0700 dan `unix_socket_permissions = 0700`: proses mana pun milik user OS pemilik folder itu (dan root) bisa login sebagai `stackly_readiness`. Secara fungsi ini setara `trust` yang dibatasi ke satu role tanpa hak dan ke satu socket privat. Peer tetap dipilih karena invariant "tidak ada baris `trust` di mana pun" tetap sederhana untuk diaudit dan di test, dan peta bisa dipersempit ke nama user OS yang persis kalau kelak ada token untuk itu. Itu bukan eskalasi hak: user OS itu sudah bisa membaca dan menulis folder data secara langsung. Yang dijaga adalah bahwa tidak ada role berhak yang bisa masuk tanpa password, dan bahwa probe yang berjalan setiap 10 detik tidak memakai superuser.

### Readiness dan health

- Probe `exec` menjalankan `${runtime.executables.psql}` dengan argv dan env di manifest; database `postgres` dipilih lewat `PGDATABASE`, bukan argv (lihat catatan manifest). Sukses berarti exit 0 (`SELECT 1` berhasil dijawab). Koneksi yang ditolak, server yang masih `starting up`, socket yang belum ada, dan role yang tidak bisa login masing masing membuat `psql` keluar bukan 0.
- Socket diperiksa, bukan TCP, karena hanya socket yang mendukung peer auth. Listener TCP dibuat postmaster sebelum ia menerima koneksi apa pun, dan bind yang gagal pada satu satunya `listen_addresses` membuat postmaster keluar `FATAL`, jadi query yang berhasil lewat socket juga berarti listener TCP sudah aktif.
- `PGCONNECT_TIMEOUT=2` (nilai minimum libpq) membuat `psql` menyerah sebelum `attemptTimeoutMilliseconds` 3000; selebihnya aturan pembantu spec 0010 (TERM lalu KILL setelah 5 detik) berlaku. `PGAPPNAME` membuat koneksi probe terlihat sebagai `stackly-readiness` di `pg_stat_activity`.
- Env child tertutup (spec 0009), jadi `PGHOST`, `PGPASSWORD`, `~/.pgpass`, dan `~/.psqlrc` milik pengguna tidak pernah terbaca (`HOME` adalah `${instance.temp}`, dan `--no-psqlrc`).
- **Detail kegagalan** (format ini milik spec ini, untuk semua probe `exec`): observasi readiness dan health yang gagal karena probe `exec` membawa `ObservationDetail` berisi baris tidak kosong pertama dari stderr pembantu, lewat `Redactor`, dipotong 256 karakter; kalau stderr kosong, `exit <kode>`; kalau timeout, `timeout`. Contoh: `psql: error: connection to server on socket "<run>/.s.PGSQL.5432" failed: FATAL:  role "stackly_readiness" is not permitted to log in`.

### Stop

`gracefulProcess` dengan `signal: int`, yaitu fast shutdown: transaksi yang berjalan dibatalkan, klien diputus, checkpoint shutdown ditulis, lalu postmaster keluar 0 dan menghapus `postmaster.pid` serta file socket nya. `SIGTERM` (smart shutdown) menunggu semua klien memutus diri dan bisa menggantung selamanya, dan `SIGQUIT` (immediate) memaksa crash recovery di start berikutnya. Setelah `stopSeconds` 60, operasi gagal `supervisor.stop_timeout` dan instance tetap `stopping` (spec 0010 tanpa eskalasi); force stop milik fitur 26.

### Reload

`render: [pgHba, pgIdent, postgresqlConf]`, `SIGHUP` ke postmaster yang terverifikasi, lalu verifikasi readiness dalam `reloadSeconds` 30. Field `apply: reload` (`logStatement`, `logMinDurationStatementMs`) langsung berlaku; parameter `restart` yang ikut berubah di file dicatat PostgreSQL sebagai butuh restart dan baru berlaku setelah restart, sesuai kelas field nya.

### Preflight data

- `dataMarker` `PG_VERSION` dengan pattern `([0-9]+)`: isi `17\n` atau `18\n` menjadi `RuntimeLine` `17` atau `18`. Tanpa `lineCompatibility`, data hanya boleh dijalankan runtime dengan line yang sama (BR-009 ketat).
- Pemanggil `preflight` tetap milik spec 0009 dan 0010 (start sebelum spawn), fitur 58 (rebind), 57 (adoption), dan 51 (restore). PostgreSQL sendiri juga menolak data mayor lain (`database files are incompatible with server`), tetapi itu terjadi setelah spawn; Stackly menolak lebih dulu.

### Perubahan core additive

Semua tambahan berikut generik, ditinjau, dan dihitung sebagai ukuran RISK-07 untuk PostgreSQL. Tidak ada yang mengubah perilaku paket yang sudah ada.

| No | Perubahan | Tempat | Isi | Alasan PostgreSQL butuh |
| --- | --- | --- | --- | --- |
| 1 | Format template `postgresqlConf` | `core::provider::template` (`TemplateFormat`), JSON Schema | escaper di AC-2; tanpa parse ulang (seperti `natsConf`), dijaga test terhadap `postgres -C` | `postgresql.conf` (direncanakan spec 0009) |
| 2 | Format template `plain` | sama | tag output dan tag blok ditolak saat validasi; isi keluar apa adanya | `pg_hba.conf` dan `pg_ident.conf` statis yang tidak boleh terkena escaper format lain |
| 3 | `runExecutable.stdin.text` | `core::provider::{raw, validate, plan}`, executor init `core::supervisor::init`, JSON Schema | aturan di AC-3; `Stdin::Text(String)` di plan | membuat role readiness lewat single user mode, satu satunya cara menjalankan SQL saat init tanpa server berjalan |
| 4 | `x-stackly.printableOnly` | `core::provider::settings`, `core::secret::service` (`put`), JSON Schema, katalog kode `secret.invalid_value` | aturan di AC-4 | `initdb --pwfile` hanya membaca baris pertama |
| 5 | Format detail kegagalan probe `exec` | `core::supervisor::probe` | aturan di *Readiness dan health*; spec 0010 sudah menangkap dan menyamarkan output pembantu, tambahan ini menetapkan baris mana yang menjadi `ObservationDetail` | pesan PostgreSQL sampai ke pengguna (Follow-up spec 0004 `ObservationDetail`) |
| 6 | `runtime.versionCheck` | milik spec 0018 (update spec 0009) | dipakai apa adanya (AC-15) | validasi binary saat install |

Kunci `details` baru `reason` pada `secret.invalid_value` adalah perubahan kontrak additive: bump minor `ProtocolVersion::CURRENT` di commit yang sama dan `cargo xtask codegen` (spec 0006). Kalau DTO field settings (fitur 17 atau 20) membawa flag secret field ke klien, `printableOnly` ikut sebagai field opsional di sana, diputuskan pemilik DTO itu.

### Evidence kompatibilitas

Bentuk dari spec 0018, ditulis test native per artefak:

```json
{
  "schema": "stackly.compatibility-evidence/v1",
  "providerRevision": { "id": "official/postgresql", "revision": "1.0.0", "manifestSha256": "<hex>" },
  "checks": ["init", "start", "readiness", "stop", "reload", "adoption", "major_mismatch_refusal"],
  "knownLimitations": []
}
```

`cargo xtask runtime compat-record` (fitur 18) menyalin record itu ke `evidence/compatibility/`, dan `cargo xtask codegen` mengubah PASS terbaru menjadi compatibility record di `versions.json`.

### Data model

Tidak ada tabel, kolom, atau migrasi baru. Yang disimpan per instance sudah dimodelkan spec 0004, 0005, 0010, dan 0011:

| Data | Tempat | Isi untuk PostgreSQL |
| --- | --- | --- |
| provider revision | registry, `provider_revision_dir` | `official/postgresql@1.0.0#<sha>`, capability `reload`, `multi_instance`, `persistence`, `cold_backup` |
| instance | registry | `data_line` dan `data_version` dari record runtime saat create (`17` atau `18`) |
| config revision | registry (`config_revisions`) | nilai settings; `superuserPassword` sebagai teks reference |
| secret | Keychain plus registry `secret_references` | reference `local:instance/<id>/superuser-password` |
| endpoint | registry (`instance_endpoints`) | `client` (tcp, host, `Fixed` atau `Auto`, `bound`) dan `socket` (unix, `.s.PGSQL.${endpoint.client.port}`) |
| cluster | folder data instance | milik PostgreSQL, termasuk `PG_VERSION`, `postgresql.auto.conf`, dan role `stackly_readiness` |
| config | folder config instance | tiga file render, ditulis ulang sebelum setiap spawn dan reload |

### API surface

Tidak ada method IPC, command CLI, atau DTO baru selain kunci `details` di atas. Permukaannya adalah paket dan fungsi berikut:

| Fungsi atau artefak | Tempat | Input utama | Output utama | Pemanggil | Error utama |
| --- | --- | --- | --- | --- | --- |
| paket `providers/postgresql` | repo | file paket | `ValidatedPackage` | catalog (fitur 17), create (fitur 20), test | `manifest.invalid` |
| `TemplateFormat::{PostgresqlConf, Plain}` | `core::provider::template` | nilai context | teks ter escape | renderer | `UnrepresentableValue`, `template_type` |
| `Stdin::Text` | `core::provider::plan`, `core::supervisor::init` | teks literal manifest | stdin proses langkah init | `execute_init` | `token_not_allowed_here`, `invalid_value` saat validasi |
| `printableOnly` | `core::provider::settings`, `SecretService::put` | `SecretInput` dan konteks field | nilai tersimpan atau penolakan | `instance.create`, `secret.set` | `secret.invalid_value` (`reason: control_characters`) |
| `supervisor::fixture::pin_package(platform, layout, dir)` | core fitur `fixture` | folder paket | `ProviderRevisionRecord` untuk di commit test | test; `install_fake` memakainya | panic dengan pesan di test |
| `runtime::fixture::install_cached_artifact` | core fitur `fixture`, milik spec 0018 | paket, ID artefak, file cache | `Runtime`, `ArtifactRecord` | test native fitur ini | `BLOCKED` kalau cache tidak ada |
| test native | `apps/daemon/tests/postgresql.rs` | `SpawnedDaemon`, cache runtime | evidence `native` dan `compatibility.json` | `cargo test -p stacklyd --test postgresql -- --ignored` | `BLOCKED` tanpa artefak |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| create | password superuser | `SecretService::put(PutTarget::New, Generate)` kecuali `params.secrets` memberi `Value` atau `CopyFrom` (spec 0010 dan 0011) |
| create | reference secret | `local:instance/<id>/superuser-password`, purpose dari nama field (spec 0011) |
| create | permintaan port `client` | `params.endpoints`, default `Auto`; host dari `defaultHost` `127.0.0.1` |
| create | `data_line`, `data_version`, `data.initializedLine` di `instance-manifest.json` | record runtime yang dipilih (fitur 18) |
| init | path file password | `${instance.secrets}/superuser-password` = `instance_secrets_dir(short key)` |
| init | byte password | `ResolvedSecrets` dari `resolve_for_start`, ditulis `SecretFile::create` |
| init | nama superuser, encoding, locale, checksum | literal manifest |
| init | SQL role readiness | `stdin.text` literal manifest |
| init, start | folder data, config, temp | `InstancePaths` (spec 0004) |
| start | host dan port | endpoint `client` (spec 0004, 0010) |
| start | folder socket | `instance_run_dir(short key)` |
| start | nama file socket | diturunkan PostgreSQL dari port (`.s.PGSQL.<port>`), sama dengan `socketName` |
| render | `cluster_name` | `instance.name` |
| render | `max_connections`, `shared_buffers`, `log_statement`, `log_min_duration_statement` | settings tervalidasi dari config revision aktif |
| render | zona waktu, locale pesan, `ssl`, logging | literal template |
| probe | program | root runtime ditambah `bin/psql` dari `additionalExecutables` |
| probe | role, query | literal argv manifest |
| probe | database, timeout koneksi, nama aplikasi | env literal manifest (`PGDATABASE`, `PGCONNECT_TIMEOUT`, `PGAPPNAME`) |
| probe | detail kegagalan | stderr pembantu (spec 0010), lewat `Redactor` |
| stop | sinyal | `signal: int` di manifest |
| preflight | line tercatat | `data_line` instance di registry |
| preflight | line penanda | isi `<data>/PG_VERSION` lewat `read_file_limited` 4 KiB |
| preflight | line runtime | record runtime yang diikat |
| reload | nilai baru | config revision aktif (test menulisnya lewat jalur registry sampai fitur 29 ada) |
| catalog | kartu provider | `metadata` paket dan `versions.json` (spec 0017) |
| `versions.json` | artefak | `runtimes/inventory.toml` lewat `cargo xtask codegen` |
| evidence | `digest`, `version` | record artefak (`ArtifactRecord`) |
| evidence | `providerRevision` | `ValidatedPackage::key()` |
| test native | file artefak | `stackly_testkit::runtime_cache::cached_artifact(sha256)` (spec 0018) |
| connection (fitur 21) | user, database, password, endpoint | `postgres`, `postgres`, field `superuserPassword`, endpoint `client` atau `socket` (Follow-up) |

### Edge cases dan kegagalan

| Kasus | Yang terjadi | Penanganan |
| --- | --- | --- |
| Folder data tidak kosong saat init | `initdb` keluar 1 | init `failed` (`init_step_failed`, step `initdb`); fitur 20 menolak lebih awal lewat preflight |
| Disk penuh saat `initdb` | `initdb` gagal dan membersihkan isinya | init `failed` dengan cuplikan |
| Daemon mati saat init | rekonsiliasi spec 0010 | `init_failed { interrupted }`, instance di purge (fitur 54), tidak pernah init ulang (BR-008) |
| Password berisi baris baru | ditolak saat `put` | `secret.invalid_value` (`reason: control_characters`) sebelum Keychain |
| Password sama persis dengan literal argv lain (misalnya `scram-sha-256`) | `ensure_not_in_argv` menolak | init gagal `secret.delivery_rejected`; residual yang dinyatakan, `postgres` sengaja dihindari |
| Port `Fixed` dipakai proses lain | spec 0010 | `supervisor.endpoint_in_use` sebelum spawn |
| Shutdown tidak bersih (crash, listrik mati) | crash recovery saat start | `readinessSeconds` 120 dan `recoverySeconds` 300; deadline lewat membuat operasi `readiness_timeout` sementara proses tetap jalan sampai ready (spec 0010) |
| `postmaster.pid` basi dengan PID yang kini milik proses lain user yang sama (setelah listrik mati lalu boot) | postmaster keluar `FATAL lock file "postmaster.pid" already exists` | crash loop berakhir `failed` dengan cuplikan; Stackly tidak menghapus file itu karena ia juga penjaga terhadap postmaster luar; perbaikan terpandu di fitur 34 (Follow-up) |
| Role `stackly_readiness` dihapus atau diubah pengguna | probe gagal dengan pesan PostgreSQL | readiness `not_ready` atau tidak pernah ready, detail menyebut role; dijelaskan di README; perbaikan di fitur 34 |
| Password superuser diubah lewat `ALTER ROLE` | readiness tidak terpengaruh (peer) | connection string Stackly basi; pengguna memperbarui Keychain lewat `secret.set` dengan konfirmasi `stored_only` (spec 0011) |
| `ALTER SYSTEM` pada parameter milik Stackly | tersimpan di `postgresql.auto.conf` tetapi kalah dari argv | tidak berpengaruh; parameter lain berlaku seperti biasa |
| Batas shared memory System V macOS (`kern.sysv.shmmni` 32 segmen di Mac ini) | postmaster ke 33 dan seterusnya keluar `FATAL could not create shared memory segment` | crash loop dengan cuplikan; batas dicatat di Consequences; peringatan kapasitas milik fitur 28 |
| OS membersihkan folder run saat instance jalan | socket hilang | readiness `not_ready` dengan detail; restart membuat ulang (PostgreSQL menyentuh file socket berkala supaya tidak tua) |
| `psql` menggantung | timeout percobaan | aturan pembantu spec 0010; dihitung sebagai kegagalan probe |
| Reload dengan nilai yang ditolak PostgreSQL | PostgreSQL mencatat `configuration file contains errors` dan memakai nilai lama | verifikasi readiness tetap lolos, karena reload yang sukses hanya berarti sinyal terkirim dan server masih menjawab; hanya `SHOW` (test AC-13) yang membuktikan nilai berlaku. Nilai dari schema yang tervalidasi seharusnya tidak pernah ditolak |
| Port `Auto` berubah antar start | socket dan file `.s.PGSQL.<port>.lock` port lama tertinggal di folder run | tidak berbahaya; pembersihan socket basi spec 0010 dan pembersihan runtime root oleh OS menyapunya |
| Stop melewati 60 detik (checkpoint besar) | spec 0010 | `supervisor.stop_timeout`, tetap `stopping`; force stop fitur 26 |
| `log_statement = all` | statement `CREATE ROLE ... PASSWORD '...'` tercatat apa adanya oleh PostgreSQL | `Redactor` hanya menyamarkan nilai yang dikenal; residual yang dinyatakan dan disebut di deskripsi field |

### Asumsi pada spec lain

- **Assumed (spec 0009)**: v1alpha1 dibangun sesuai spec itu, termasuk `materializeSecret` ke `${instance.secrets}`, probe `exec` dengan env teks, `RunPlan.render`, `dataMarker`, dan JSON Schema yang dibangkitkan; empat tambahan di atas masuk lewat build fitur ini.
- **Assumed (spec 0010)**: `execute_init` menjalankan langkah `Run` dengan lock di fd 3, executor menulis ulang config sebelum setiap spawn, probe `exec` memakai aturan pembantu, `preflight` dipanggil di tahap `verify`, `instance.create` minimal menerima `secrets` dengan `SecretInput`, dan `execute_reload` bisa dipanggil test. Fitur 20 memperluas create secara additive.
- **Assumed (spec 0011)**: `SecretService::put` dengan konteks field, `generate` 32 karakter alfanumerik, `SecretFile`, dan `ensure_not_in_argv` dengan aturan persis dan bentuk `<apa pun>=<nilai>`.
- **Assumed (spec 0018)**, ditulis paralel dan dikonfirmasi sesi `arc-f18-spec` 2026-10-19: `stackly_core::runtime::fixture::install_cached_artifact(platform, layout, registry, package: &ValidatedPackage, artifact_id, cache_file: &Path)` di fitur dev `fixture`, yang menjalankan pipeline install asli dari file lokal dan mengembalikan `Runtime` dan `ArtifactRecord`; `stackly_testkit::runtime_cache::cached_artifact(sha256) -> Option<PathBuf>` yang mencari di `~/Library/Caches/dev.stackly.xtask/runtime/<sha256>`; validasi Mach-O dan dependency setiap path `executables` saat install; record runtime dengan `line`, `version`, `target`, dan sha256 artefak; field manifest `runtime.versionCheck { args, pattern, timeoutSeconds }` lewat update spec 0009; dan `cargo xtask runtime compat-record <report.json>`. Kalau spec 0018 yang di merge berbeda, spec ini di update sebelum milestone 2.
- **Assumed (spec 0017)**, ditulis paralel dan dikabarkan sesi `arc-f17-spec` 2026-10-19: `providers/catalog.toml` dengan entri `[[planned]]` PostgreSQL, yang dipindah build fitur ini ke `[[provider]] id = "official/postgresql"` dengan `revision` dan `publishedAt` catalog naik, di commit yang sama dengan bukti lifecycle ujung ke ujung, lalu `cargo xtask catalog check --update`.

### Jawaban follow-up dari spec lain

- **Spec 0003** (`trust` di spike adalah konfigurasi spike, bukan default instance): default instance memakai `initdb --auth-local=scram-sha-256 --auth-host=scram-sha-256`, `pg_hba.conf` buatan Stackly tanpa `trust`, dan peer hanya untuk role readiness di socket privat (AC-6). Spike spec 0003 tidak berubah.
- **Spec 0004** (socket PostgreSQL muat di batas macOS; `ObservationDetail` untuk probe): nama socket `.s.PGSQL.<port>` paling panjang 14 byte di bawah `SOCKET_NAME_MAX` 15, di folder run yang dianggarkan spec 0004; detail probe mengikuti aturan di *Readiness dan health*.
- **Spec 0009** (manifest, primitive, strategi stop, jenis probe, template): semuanya muat di primitive yang ada kecuali tambahan 1 sampai 4 di *Perubahan core additive*; probe native tidak dibutuhkan, jadi kontrak probe native tetap dibangun fitur 23.
- **Spec 0010** (bukti dengan provider nyata; `instance.create` minimal): AC-10, AC-11, dan AC-13 adalah bukti native untuk init, startup transaction, readiness, stop tanpa `SIGKILL`, render sebelum spawn, reload, preflight, dan adopsi lewat lock di fd 3; create minimal dipakai apa adanya.
- **Spec 0011** (secret field, `SecretInput`, `SecretService::put`): satu secret field, `superuserPassword`, dengan `generate` sebagai default. Role readiness sengaja tanpa password: peer auth lewat socket privat menghindari pembacaan Keychain setiap 10 detik dan menyimpan password di memori daemon selama instance hidup, yang bertentangan dengan model resolve per aksi spec 0011.

### Key invariants

- Tidak ada baris `trust` di `pg_hba.conf` mana pun milik instance, dan tidak ada role berhak yang bisa login tanpa password.
- Probe readiness dan health tidak pernah login sebagai superuser dan tidak pernah membaca secret.
- Setiap program yang dijalankan untuk instance PostgreSQL berasal dari root runtime yang diikat instance itu.
- Port, alamat, socket, lokasi file config, dan tujuan log selalu dari argv, jadi tidak bisa digeser config atau `ALTER SYSTEM`.
- Data dengan line `PG_VERSION` berbeda dari line runtime tidak pernah dijalankan.
- Tidak ada path di dalam file config, dan tidak ada nilai di template `plain`.
- Password tidak pernah ada di argv, config, spool, log, atau registry; file password hanya ada selama aksi init.

### Security model

- Ancaman yang dijaga: login tanpa password (dijawab SCRAM dan tanpa `trust`), proses lokal yang menebak port (TCP hanya loopback dan selalu SCRAM), password di argv atau log (stdin dan file sekali pakai spec 0011, `ensure_not_in_argv`, `Redactor`), dan tool dari `PATH` yang tidak dikenal (program hanya dari runtime, env tertutup).
- Peer auth untuk role readiness bergantung pada folder socket 0700 milik user (spec 0004) dan `unix_socket_permissions = 0700`. Role itu bukan batas keamanan terhadap user OS yang sama, karena user itu sudah memiliki folder data.
- Data checksum aktif supaya kerusakan halaman data terdeteksi.
- Residual risk yang dinyatakan: proses lain milik user yang sama bisa login sebagai `stackly_readiness`; `log_statement = all` mencatat statement yang memuat password baru; `ssl = off` karena hanya loopback (TLS ada di Deferred); nilai `ALTER SYSTEM` buatan pengguna tidak terlihat di settings Stackly.

### Configuration required

Tidak ada variabel environment, credential, atau port baru untuk daemon dan CLI. Test native butuh artefak PostgreSQL 17.11 dan 18.6 di cache runtime xtask (`cargo xtask runtime spike --id <id>` sekali, spec 0003), dan memakai `STACKLY_ROOT` lewat `SpawnedDaemon`.

### Critical test scenarios

- Happy path per mayor: `install_cached_artifact` 17 atau 18, `pin_package` PostgreSQL, `instance.create` dengan password bangkitan, start sampai `ready`, query superuser lewat socket dan TCP, tulis satu baris, stop, start lagi, baris masih ada; varian in process (`stacklyd::server::start` dengan `LogSink` perekam) melihat baris log PostgreSQL bercap waktu UTC. Memverifikasi **AC-5**, **AC-6**, **AC-13**.
- Golden: render ketiga config, `init_plan`, `run_plan`, dan `reload_plan` di context golden spec 0009 (endpoint `client` mendapat port 14000, jadi socket `.s.PGSQL.14000`, dan endpoint `socket` menjadi `ResolvedEndpoint::Unix`); tidak ada entri argv di plan init, start, maupun probe yang sama persis dengan `postgres` atau berakhir `=postgres`. Memverifikasi **AC-1**, **AC-12**.
- Tanpa `trust`: grep metode `trust` di kedua `pg_hba.conf` kosong; superuser tanpa password ditolak di socket dan TCP; `stackly_readiness` ditolak lewat TCP dan ke `template1`; koneksi `replication=database` ditolak. Memverifikasi **AC-6**.
- Locale: query `pg_database` dan `SHOW` untuk kedua mayor dengan daemon ber `LANG=de_DE.UTF-8` dan `TZ=Asia/Jakarta`. Memverifikasi **AC-7**.
- Endpoint: `lsof -a -p <postmaster> -i -U` hanya port endpoint dan socket instance; mode socket dan folder 0700; `ALTER SYSTEM SET port` lalu restart tetap di port Stackly. Memverifikasi **AC-8**.
- Readiness jujur: setelah `ready`, `ALTER ROLE stackly_readiness NOLOGIN` dengan TCP tetap terbuka (test connect TCP berhasil) membuat `not_ready` setelah 2 kegagalan dengan detail berisi `not permitted to log in`; `LOGIN` lagi membuat `ready`; atribut role dan `CREATE TABLE` yang gagal. Memverifikasi **AC-9**.
- Stop: sesi `pg_sleep(600)` terbuka, `instance.stop` selesai dalam 10 detik tanpa `SIGKILL`, `postmaster.pid`, socket, dan lock socket hilang, log start berikutnya memuat `was shut down at`. Memverifikasi **AC-10**.
- Mayor salah: data 17 diikat ke runtime 18 dan sebaliknya, `PG_VERSION` dihapus, `PG_VERSION` diubah ke `16`; masing masing ditolak di tahap `verify` tanpa observasi proses baru. Memverifikasi **AC-11**.
- Tool dari runtime: daemon dengan `PATH` berisi folder umpan; file penanda umpan tidak pernah dibuat. Memverifikasi **AC-12**.
- Adopsi: `SIGKILL` ke daemon saat `ready`, daemon baru mengadopsi postmaster yang sama dan readiness kembali `ready`; `lsof -d 3` pada postmaster dan child nya menunjukkan `instance.lock`. Memverifikasi **AC-13**.
- Init gagal: folder data diisi satu file sebelum init (lewat test, melewati preflight fitur 20) menghasilkan `init_step_failed` di langkah `initdb` dan file itu utuh; file `superuser-password` tidak ada. Memverifikasi **AC-5**.
- Format: tabel escaper `postgresqlConf` (kutip, backslash, Unicode, karakter kontrol ditolak, boolean, integer negatif) dan round trip `postgres -C`; template `plain` dengan `{{ x }}` atau `{{#if}}` ditolak dengan baris dan kolom. Memverifikasi **AC-2**.
- Stdin teks: `stdin.text` di probe atau stop ditolak, `${` di teks ditolak, teks 16385 byte ditolak, plan golden memuat `Stdin::Text`, dan byte yang diterima proses sama persis (dummy server fitur 9 atau `cat` dari runtime fake). Memverifikasi **AC-3**.
- Printable only: `put` dengan nilai berisi `\n` atau `\u{85}` ditolak tanpa nilai di pesan dan tanpa panggilan Keychain di `FakeSecretStore`; `generate` lolos; `printableOnly` pada field bukan secret ditolak; password dengan spasi, `'`, `"`, dan `\` bisa login. Memverifikasi **AC-4**.
- Evidence: cache kosong membuat `BLOCKED` dengan alasan dan owner yang benar; run lengkap menulis `compatibility.json` yang lolos `cargo xtask runtime compat-record`. Memverifikasi **AC-14**.
- Version check: `postgres --version` kedua artefak cocok pattern dan capture nya sama dengan `version`. Memverifikasi **AC-15**.

## Build plan

Urutan mengikuti pendekatan Journey proyek: satu jalur dibuat lengkap dulu, yaitu "instance PostgreSQL 18 dari paket sampai ready lalu stop, termasuk semua jalur gagalnya", karena itulah bagian provider dari JOURNEY-01. Milestone 1 membangun semua yang bisa dibuktikan tanpa server nyata, milestone 2 membuktikan jalur itu pada PostgreSQL 18 asli lewat daemon, dan milestone 3 memperluasnya ke PostgreSQL 17, penolakan mayor, evidence, dan catalog.

**Milestone 1: Paket dan perubahan core, dibuktikan di fixture**

1. Format `postgresqlConf` dan `plain` di `core::provider::template`, enum di raw dan JSON Schema, unit test escaper dan cek statis, render ke kedua format di fuzz target `provider_template`. Memenuhi **AC-2** (tanpa round trip native).
2. `stdin.text` di `core::provider::raw`, `validate`, dan `plan` (`Stdin::Text`), JSON Schema, golden plan, dan test validasi. Memenuhi **AC-3** (bagian plan).
3. `x-stackly.printableOnly` di `core::provider::settings` dan JSON Schema, cek di `SecretService::put` dengan konteks field, kunci `details` `reason` di katalog kode dengan bump minor protocol dan `codegen`, test dengan `FakeSecretStore`. Memenuhi **AC-4** (bagian fixture).
4. Paket `providers/postgresql` (manifest, settings schema, tiga template, README), `versions.json` lewat `cargo xtask codegen`, `cargo xtask provider check`, golden render dan plan di `crates/core/tests/golden/provider/postgresql/`, dan seed fuzz lewat `cargo xtask fuzz seed`. Memenuhi **AC-1** (tanpa catalog), **AC-12** (bagian plan).

**Milestone 2: PostgreSQL 18 asli lewat daemon**

5. Executor init menulis `Stdin::Text` lalu menutup stdin (`core::supervisor::init`), dan format detail kegagalan probe `exec` di `core::supervisor::probe` sesuai *Readiness dan health* (ditambah, atau disesuaikan kalau build spec 0010 sudah mengisi detail dengan bentuk lain). Memenuhi **AC-3**, **AC-9** (bagian detail).
6. `supervisor::fixture::pin_package` (dipecah dari `install_fake`, yang lalu memakainya), helper test native di `apps/daemon/tests/postgresql.rs` yang memasang runtime lewat `install_cached_artifact` dan menandai `BLOCKED` tanpa cache, dan kait test untuk menambah `LogSink` perekam ke `SinkSet` server in process (di balik fitur `fixture`, kalau `DaemonConfig` spec 0010 belum punya). Memenuhi **AC-13** (bagian log), **AC-14** (jalur `BLOCKED`).
7. Test native PostgreSQL 18: happy path, tanpa `trust`, locale, endpoint dan `ALTER SYSTEM`, readiness jujur, stop dengan sesi terbuka, init gagal, `PATH` umpan, reload, adopsi, round trip `postgres -C`, dan login dengan password berkarakter khusus. Memenuhi **AC-2**, **AC-4**, **AC-5** sampai **AC-10**, **AC-12**, **AC-13**.

**Milestone 3: PostgreSQL 17, penolakan mayor, evidence, dan catalog**

8. Suite milestone 2 untuk PostgreSQL 17 (satu fungsi test per mayor), dan di commit yang sama, yang menutup bukti lifecycle ujung ke ujung untuk kedua lini, entri PostgreSQL di `providers/catalog.toml` pindah dari `[[planned]]` ke `[[provider]]` dengan revision dan `publishedAt` catalog naik, lalu `cargo xtask catalog check --update` (aturan spec 0017: catalog menampilkan provider hanya setelah lifecycle nya terbukti). Memenuhi **AC-1** (bagian catalog), **AC-5** sampai **AC-10**, **AC-13**.
9. Test penolakan mayor dua arah dan penanda hilang atau berubah, plus unit test preflight dengan isi `PG_VERSION` asli. Memenuhi **AC-11**.
10. Guard evidence `native` dengan `covers` ID PRD dan `compatibility.json` per artefak, lalu `cargo xtask runtime compat-record`. Memenuhi **AC-14**.
11. `runtime.versionCheck` di manifest dan test nya. Memenuhi **AC-15**.

**Prasyarat**: langkah 1, 2, dan 4 butuh fitur 9 (spec 0009) sudah dibangun; langkah 3 butuh langkah 4 spec 0011 (`SecretService::put`). Milestone 2 butuh build fitur 10 selesai sampai milestone 4 (daemon, `instance.create`, `execute_reload`, `install_fake`), fitur 11 selesai, dan milestone 2 spec 0018 (`install_cached_artifact`). Langkah 8 juga butuh build fitur 17 (`providers/catalog.toml`), dan langkah 11 butuh update spec 0009 dari fitur 18.

**Sesi paralel yang aman**: langkah 1, 2, dan 3 menyentuh file berbeda (template, plan dan raw, settings dan secret service), jadi bisa tiga sesi kalau masing masing hanya menambah satu varian enum di `raw.rs` lewat commit kecil terpisah; langkah 4 menunggu ketiganya karena memakai format dan field baru. Di milestone 3, langkah 11 tidak menyentuh file test dan bisa jalan sendiri; langkah 8 sampai 10 sebaiknya satu sesi karena menyentuh file test yang sama.

## Consequences

**Positive**:

- PostgreSQL lokal langsung aman: tanpa `trust`, superuser selalu dengan password, TCP hanya loopback, dan password tidak pernah lewat argv.
- Ready berarti database menjawab query sebagai role berhak minimum, bukan port terbuka, dan pesan PostgreSQL sampai ke pengguna saat tidak ready.
- Readiness tidak bergantung pada password superuser, jadi pengguna yang mengganti password di dalam database tidak merusak status instance.
- Port, socket, dan lokasi config tidak bisa bergeser diam diam, walau pengguna memakai `ALTER SYSTEM`.
- Data tidak pernah dijalankan binary mayor lain, dan locale builtin `C.UTF-8` tidak bergantung pada data locale macOS atau versi ICU, jadi index tidak rusak karena update OS atau runtime.
- Tambahan core generik: `plain`, `stdin.text`, dan `printableOnly` bisa dipakai MySQL, Valkey, dan provider berikutnya.

**Negative / tradeoffs**:

- Empat perubahan core untuk satu provider (RISK-07): dua format template, `stdin.text`, dan `printableOnly`, ditambah detail probe kalau spec 0010 belum mengisinya.
- Setiap instance menjalankan satu proses `psql` pendek setiap 10 detik selama hidup; lebih mahal dari probe native, tetapi tanpa kode protokol PostgreSQL di daemon.
- Urutan teks memakai urutan code point (`C.UTF-8`), bukan urutan alfabet bahasa; pengguna yang butuh urutan bahasa memakai collation ICU per kolom (ICU ikut di runtime).
- Zona waktu server tetap UTC; klien yang tidak menyetel zona waktu sesinya melihat waktu UTC.
- Nilai `ALTER SYSTEM` dan suntingan tangan tidak terlihat di settings Stackly, dan suntingan di folder config hilang saat start berikutnya.
- Role `stackly_readiness` terlihat di `\du` dan menjadi kontrak: dump logical, adoption, dan restore harus memperhitungkannya.
- macOS membatasi shared memory System V (32 segmen di Mac ini), sehingga sekitar 32 instance PostgreSQL bisa berjalan bersamaan, kurang kalau aplikasi lain juga memakainya.
- Password superuser yang sama persis dengan literal argv lain (misalnya `scram-sha-256`) ditolak saat init.

**Neutral**:

- `providers/postgresql` menjadi provider nyata pertama yang memakai `runExecutable`, `materializeSecret`, `dataMarker`, endpoint `unix`, dan reload `signal`.
- Folder data tetap memuat `postgresql.conf` dan `pg_hba.conf` buatan `initdb` yang tidak dipakai (tanpa `trust`), sehingga `pg_ctl -D` manual oleh pengguna tetap aman.
- Kontrak probe native tetap milik fitur 23.

## Follow-up

- [ ] `/architect` update spec 0009 (sesudah sesi `f9-closeout` selesai): catat format `postgresqlConf` dan `plain`, `stdin.text`, dan `printableOnly` sebagai tambahan additive dari spec 0019, supaya himpunan tertutup di AC-4 dan *Template config* spec itu tidak basi.
- [ ] `/architect` update spec 0011: catat `printableOnly` di `put` dan kunci `details` `reason` pada `secret.invalid_value`, dan timbang apakah kebalikannya (karakter kontrol ditolak untuk semua secret, dengan `multiline: true` untuk field seperti PEM) lebih baik sebagai default.
- [ ] `/architect` update spec 0010: catat format detail kegagalan probe `exec` dari spec ini di *Probe, readiness, dan health*.
- [ ] Fitur 21: bentuk connection string PostgreSQL (`postgresql://postgres:<password>@<host>:<port>/postgres` dan varian socket) dan tempatnya di manifest, menjawab Follow-up spec 0009 tentang `purpose` endpoint.
- [ ] Fitur 26: force stop PostgreSQL sebaiknya `SIGQUIT` (immediate shutdown) sebelum eskalasi lain, karena `SIGKILL` ke postmaster meninggalkan backend yatim.
- [ ] Fitur 34: perbaikan terpandu untuk `postmaster.pid` basi dengan PID yang dipakai ulang, dan untuk role `stackly_readiness` yang hilang atau berubah.
- [ ] Fitur 51 dan 52: dump dan restore logical melewati role `stackly_readiness`, dan instance hasil restore membuatnya lewat init biasa.
- [ ] Fitur 57: adoption data PostgreSQL luar membuat role `stackly_readiness` (single user mode saat server berhenti) dan memasang folder config Stackly sebelum start pertama.
- [ ] Fitur 28: peringatan kapasitas untuk batas shared memory System V saat jumlah instance PostgreSQL yang berjalan mendekati batas.
- [ ] Settings tambahan kalau pengguna membutuhkannya, masing masing additive lewat update spec ini: nama database awal, zona waktu, dan locale (`immutableAfterInit`).
- [ ] `/sync` setelah build: catat paket PostgreSQL, role readiness, dan perintah test native di root `AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, keputusan kecil, hasil spike lokal, dan cross check ada di [rationale.md](rationale.md).
