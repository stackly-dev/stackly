# 0040. Provider Valkey

**Date**: 2026-11-12
**Status**: Proposed

## Summary

Spec ini memutuskan paket provider resmi `official/valkey` untuk Valkey 9.1: satu server standalone per instance, selalu dengan password, hanya mendengar di loopback, dan dengan satu pilihan persistence yang jujur (`rdb`, `aof`, atau `volatile`) yang dipilih sekali saat instance dibuat. Password tidak pernah ditulis ke disk sebagai teks: file user Valkey hanya memuat hash SHA-256 nya. Ready berarti Valkey menjawab `PING` sesudah data selesai dimuat, lewat `valkey-cli` milik runtime sebagai user `stackly-readiness` yang hanya boleh `PING`. Semua ini butuh dua tambahan kecil di core: format config `valkeyConf` (milik fitur ini) dan filter hash `sha256Hex` di template (dipakai bersama fitur 43, ClickHouse). Tidak ada kontrak wire, migrasi, atau kode Swift baru.

## Requirements

**User stories**:

- Sebagai developer, saya ingin membuat cache atau store Valkey lokal yang langsung aman (password, loopback) dan siap dipakai, supaya saya tidak perlu menulis `valkey.conf` atau aturan ACL sendiri.
- Sebagai developer, saya ingin tahu dengan jujur apa yang terjadi pada data saya saat instance berhenti, crash, atau Mac mati, supaya saya memilih mode persistence dengan sadar dan tidak mengira semua key selalu selamat.
- Sebagai developer, saya ingin status ready berarti Valkey benar benar menjawab perintah sesudah data dimuat, bukan sekadar port terbuka.
- Sebagai developer, saya ingin yang berjalan selalu Valkey dari runtime yang saya pasang, tidak pernah Redis atau tool lain dari `PATH`.
- Sebagai pemilik scope, saya ingin Valkey masuk sebagai paket (manifest, schema, template) di atas engine fitur 9 dan harness fitur 36, dengan jumlah perubahan core yang tercatat sebagai ukuran RISK-07.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-038):

- **AC-1** (paket, REQ-038 bagian manifest): Paket `providers/valkey` (ID `official/valkey`, software `valkey`, revision `1.0.0`, category `cache`) berisi persis file di *Paket*. `cargo xtask provider check providers/valkey` mencetak satu baris `ok official/valkey 1.0.0 <manifest_sha256>`. `cargo xtask codegen --check` lolos dengan `versions.json` yang dibangkitkan dari inventory dan memuat tepat satu artefak `available` (`valkey-9.1.2-r2-macos-aarch64`, line `9.1`). Render kelima config dan `init_plan` serta `run_plan` untuk context golden spec 0009 sama byte demi byte dengan file golden di `crates/core/tests/golden/provider/valkey/`. `protocols/fixtures/instance/create-form-valkey.json` ada, tercantum di `protocols/fixtures/instance/index.json`, dan test `create_form_fixtures` serta test form `StacklyUI` lolos tanpa kode Swift baru. `cargo xtask docs check` lolos dengan `LICENSE-NOTICES.md` paket ini.
- **AC-2** (format `valkeyConf`): Format template `valkeyConf` mengeluarkan teks sebagai `"..."` dengan `\` menjadi `\\` dan `"` menjadi `\"`, integer sebagai desimal, dan boolean sebagai `yes` atau `no`. Teks yang memuat karakter kontrol (U+0000 sampai U+001F, U+007F sampai U+009F) ditolak dengan `unrepresentable_value` yang menyebut field atau token, tanpa nilainya. Output tidak diparse ulang. Untuk teks `a"b\c d ü #x` yang dirender ke parameter teks bebas `proc-title-template`, `CONFIG GET proc-title-template` pada Valkey 9.1.2 membaca kembali nilai yang persis sama. Kode format yang tidak dikenal tetap ditolak saat validasi (`unknown_template_format`), dan pesannya menyebut `valkeyConf` di daftar format yang dikenal.
- **AC-3** (filter hash, kontrak bersama fitur 43): Template menerima `{{ secret.<field> | sha256Hex }}`: tepat satu filter, hanya sesudah path `secret.*`, hanya di template milik config `sensitive: true`. Spasi di sekitar `|` bebas. Penolakan saat validasi, semuanya dengan baris dan kolom: filter sesudah path yang bukan `secret.*`, nama filter yang tidak dikenal, dan lebih dari satu filter memakai `template_type`; tag yang tidak bisa diparse (nama filter kosong seperti `{{ secret.x | }}`, atau karakter di luar `[A-Za-z0-9]` di nama filter) memakai `template_syntax` seperti kesalahan parse lain; tag apa pun di format `plain` tetap `template_type` seperti sekarang; dan secret, dengan atau tanpa filter, di config yang tidak `sensitive` tetap `secret_not_allowed_here` (aturan yang sudah ada, tidak dipindah). Outputnya 64 karakter hex huruf kecil dari SHA-256 atas byte secret persis, ditulis mentah tanpa escaper format. Plan memuat `Segment::SecretHash { secret: PlanSecret, algo: HashAlgo }`, yang di JSON plan (golden dan `frozen.json`) berbentuk `{ "type": "secretHash", "content": { "secret": "<secret:<field>>", "algo": "sha256_hex" } }`; `HashAlgo` mendapat `Serialize` yang menulis `as_code()`. `RenderedContent::materialize` mengisinya lewat `secret::delivery::hash(HashAlgo::Sha256Hex, ..)`; render contoh saat validasi memakai hash dari teks `x`; `Debug` dan `Serialize` plan tidak pernah memuat hash atau nilai secret. Paket yang sudah valid tetap valid dengan digest dan plan yang sama (test corpus spec 0036 AC-6 lulus tanpa mengubah satu pun `frozen.json`). Filter ini dibangun fitur 40 (langkah 2); fitur 43 hanya memakainya (keputusan lead, 2026-11-12).
- **AC-4** (bind dan auth, REQ-038): Sesudah start, `valkey-server` hanya mendengar di host dan port endpoint `client`; `lsof` tidak menunjukkan listener lain, TCP maupun socket Unix. Perintah tanpa login ditolak `NOAUTH`. User `default` diterima dengan nilai secret dari Keychain lewat `AUTH <password>` dan `AUTH default <password>`, dan ditolak `WRONGPASS` dengan nilai lain. Password berisi spasi, `"`, `'`, dan `\` berhasil login. User `stackly-readiness` hanya bisa `PING`: `GET`, `INFO`, dan `CONFIG GET` sebagai user itu ditolak `NOPERM`. Nilai password tidak pernah muncul di argv proses mana pun, di file config mana pun, di spool, di log daemon, atau di registry; `${instance.secrets}/users.conf` (mode 0600) hanya memuat hash nya dan hilang sesudah stop bersih.
- **AC-5** (readiness jujur, REQ-038): Readiness memakai probe `exec` dengan argv dan env persis di *Readiness dan health*. Instance baru `ready` sesudah percobaan pertama yang keluar 0. Selama Valkey memuat dataset, percobaan probe keluar 1 dengan stderr berawalan `LOADING`, dan `instance.get` menampilkan readiness `not_ready` dengan detail berawalan `LOADING` sebelum `ready`. Test native membuktikannya dengan cara di *Test LOADING*, yang membuat jendela muat beberapa detik dari 2000 key saja. Saat port TCP tetap menerima koneksi tetapi `PING` gagal (`ACL DELUSER stackly-readiness`), readiness menjadi `not_ready` sesudah 2 kegagalan berturut turut dengan detail yang memuat `WRONGPASS`, dan kembali `ready` sesudah user itu dipulihkan.
- **AC-6** (persistence aktif, AC-038 bagian pertama): Untuk `persistence` `rdb` dan `aof` masing masing: satu key sentinel ditulis, `instance.stop` (stop normal), `instance.start`, lalu sentinel terbaca lagi dengan nilai yang sama. Mode `rdb` meninggalkan `dump.rdb`; mode `aof` meninggalkan folder `appendonlydir/` berisi file manifest, base, dan incr. Semua file itu ada di dalam folder data instance, dan tidak ada file Valkey di luar folder data, config, temp, dan run instance.
- **AC-7** (mode volatile jujur, AC-038 bagian kedua, REQ-038): Dengan `persistence` `volatile` dan tanpa perintah admin yang menulis file (`SAVE`, `BGSAVE`, `CONFIG SET save`, `CONFIG SET appendonly`), sentinel hilang sesudah `instance.stop` lalu `instance.start`, dan folder data tidak berisi file apa pun. README menyatakan pengecualiannya: snapshot yang ditulis pengguna sendiri dimuat Valkey di start berikutnya (*Edge cases*). Field `persistence` membawa `description` persis di *Settings schema*, yang memuat kalimat bahwa mode `volatile` kehilangan semua data pada setiap stop, restart, atau crash dan bahwa tidak ada yang bisa di backup atau di restore. Teks itu sampai ke klien lewat `instance.get_create_form` (fixture `create-form-valkey.json`), dan form create menampilkannya di bawah kontrol field itu, yaitu di tempat mode dipilih, tanpa kode khusus Valkey. Sesudah instance dibuat, halaman Configuration menampilkan field itu terkunci dengan nilai `volatile` dan file `persistence-volatile.conf` yang komentarnya mengatakan hal yang sama.
- **AC-8** (durability dibedakan per policy, AC-077 bagian Valkey): Test native mencatat hasil per mode dan per kejadian, bukan satu janji umum. Sesudah `SIGKILL` ke `valkey-server` yang sudah `ready` (aturan exit dini `classify_early_exit` spec 0010 hanya berlaku sebelum ready) dan respawn oleh supervisor: mode `aof` masih punya sentinel yang ditulis lebih dari 1 detik sebelum crash; mode `rdb` kehilangan sentinel yang ditulis sesudah snapshot terakhir dan masih punya sentinel dari sebelum stop normal terakhir; mode `volatile` tidak punya apa pun. `actual` evidence nya berbentuk `valkey <versi> <mode>: normal stop <kept|lost>, process crash <kept|lost since last snapshot|lost>`, satu record per mode. Kehilangan daya host tidak disimulasikan dan dicatat sebagai batas di `knownLimitations`.
- **AC-9** (mode tidak bisa diganti): `persistence` ber kelas `immutableAfterInit`. `instance.config.validate` dengan nilai `persistence` lain untuk instance yang sudah diinisialisasi membalas `valid: false` dengan `fieldErrors` `settings.persistence` ber kode `immutable_after_init`, dan tidak ada config yang berubah.
- **AC-10** (parameter milik Stackly): Port, alamat bind, folder data, mode persistence, `protected-mode`, `daemonize`, dan `set-proc-title` datang dari argv `valkey-server`, dan server berjalan tanpa argumen file config. Karena itu `CONFIG REWRITE` ditolak (`The server is running without a config file`), `ACL SAVE` ditolak, dan `CONFIG SET dir` serta `CONFIG SET dbfilename` ditolak (`can't set protected config`). Sesudah `CONFIG SET maxmemory 1mb` lalu stop dan start, `CONFIG GET maxmemory` kembali bernilai settings Stackly. Port `Auto` tersimpan dan dipakai lagi di start berikutnya.
- **AC-11** (stop, REQ-038): Stop normal mengirim `SIGTERM` ke proses yang identitasnya terverifikasi. Dengan satu klien yang sedang menunggu di `BLPOP q 600`, stop selesai dalam 10 detik (jauh di bawah `stopSeconds` 60), instance `stopped`, exit code 0, tanpa `SIGKILL`. Di mode `rdb` log stop memuat `DB saved on disk`.
- **AC-12** (selalu Valkey dari runtime, Done when scope): Setiap program di plan run dan probe adalah `ExecutablePath` di dalam root runtime instance (`bin/valkey-server`, `bin/valkey-cli`), dan env child hanya env basis spec 0009 ditambah env manifest. Dengan daemon yang dijalankan memakai `PATH` berisi folder umpan (`valkey-server`, `valkey-cli`, `redis-server`, dan `redis-cli` palsu yang menulis file penanda), create, start, readiness, dan stop berhasil dan file penanda tidak pernah dibuat. `INFO server` pada instance yang berjalan melaporkan `valkey_version` sama dengan versi artefak dan `executable` di dalam root runtime. Paket tidak punya jalur pengganti: tidak ada nama `redis` di `provider.yaml` selain skema URL `redis://` di blok `connection` dan `openWith`.
- **AC-13** (lifecycle lewat daemon): Lewat daemon untuk Valkey 9.1: `instance.create` dengan secret `password` bangkitan, `instance.start` membuat lifecycle `running` dengan readiness `not_ready` lalu `ready` (dibaca dari urutan event instance lewat `events.subscribe`, seperti helper `assert_not_ready_before_ready` di `apps/daemon/tests/nats/harness.rs`, bukan dari polling, karena Valkey ready dalam milidetik), baris log Valkey bercap waktu UTC terbaca lewat `instance.logs_read`, dan `instance.stop` membuat `stopped`. Proses utama memegang lock instance di fd 3 dan argv nya tetap memuat path instance (karena `set-proc-title no`). Daemon yang dibunuh `SIGKILL` saat instance `ready` lalu dijalankan lagi mengadopsi proses yang sama (PID dan waktu start sama). `secret.set` yang mengganti nilai `password` melaporkan efek `next_start`; sesudah restart, password lama ditolak `WRONGPASS` dan password baru diterima.
- **AC-14** (konformitas, spec 0036): `tests/scenarios.yaml` memuat skenario `default`, `aof`, dan `volatile`. `cargo xtask provider test providers/valkey` berakhir dengan report `pass` untuk ketiganya saat artefak ada di cache runtime xtask, dengan langkah `reload` `skipped` dan `handover` `unproven`. `apps/daemon/tests/provider_conformance.rs` punya test native `valkey_9_1_passes_conformance` (`#[ignore = "native: ..."]`, `covers = ["AC-029", "REQ-029"]`). Artefak yang tidak ada di cache membuat test `BLOCKED` dengan alasan `artifact valkey-9.1.2-r2-macos-aarch64 is not in the runtime cache`, tanpa unduhan.
- **AC-15** (evidence): Setiap test native fitur ini memasang guard evidence `native` spec 0002 dengan `covers` ID PRD yang dibuktikannya (REQ-038, AC-038, AC-077). Satu test menulis evidence `native` dengan `covers` BR-025, subject `{ provider: valkey, version, digest, source: managed }` dengan `version` dari `INFO server` (`valkey_version`) instance yang berjalan dan `digest` dari `ArtifactRecord.sha256`, dan file `compatibility.json` berbentuk *Evidence kompatibilitas*. Evidence ini dihasilkan dari `provider.yaml` final yang masuk catalog; perubahan paket sesudahnya mewajibkan run ulang.
- **AC-16** (version check dan spike dari paket, Follow-up spec 0003): Manifest mendeklarasikan `runtime.versionCheck` di *Manifest*, dan exec check install spec 0018 lolos untuk artefak ini dengan capture group `9.1.2`. Struct recipe `Valkey` dan penanda `// provisional: F40` hilang dari `xtask/src/runtime/spike/providers.rs`; `recipe(repo, "valkey")` memilih `PackageRecipe`; dan `valkey_9_1_runs_from_managed_artifact` di `xtask/tests/native_runtime.rs` lulus dari paket (listener hanya di port yang dialokasikan, stop dalam 60 detik).
- **AC-17** (catalog dan corpus, spec 0017 dan 0036): Di commit yang membuktikan lifecycle ujung ke ujung (install, create, start, readiness, stop lewat integration test di Mac ini), entri Valkey di `providers/catalog.toml` pindah dari `[[planned]]` ke `[[provider]]` (mark `Vk`, tile `red`), `revision` dan `published_at` catalog naik, dan `cargo xtask catalog check --update` mencatatnya di `providers/catalog-revisions.json`. Sesudah itu `cargo xtask provider freeze providers/valkey` menulis `crates/core/tests/fixtures/compat/valkey/1.0.0/`, dan `cargo xtask provider check` tanpa argumen lolos.
- **AC-18** (connection, Open In, remedy): `instance.get_detail` memuat dua connection (`uri` dan `valkey`) yang `masked` nya URI absolut, dan `stackly instance connection` mencetak yang pertama. Menu Open In memuat `Open in TablePlus` (hanya kalau aplikasinya terpasang) dan `Open valkey-cli in Terminal`, keduanya dengan `copySecret: password` dan tanpa token secret. Temuan doctor `not_ready` dengan detail readiness yang memuat `WRONGPASS` mendapat remedy `readiness_user`, dan kegagalan start dengan excerpt `Bad file format reading the append only file` mendapat remedy `aof_damaged`. Remedy hanya teks.
- **AC-19** (handover, Follow-up spec 0030): `apps/daemon/tests/native_launchd.rs` punya test `valkey_9_1_survives_planned_restart_and_sigkill` (`#[ignore = "native: launchd gui domain"]`) yang membuktikan instance Valkey bertahan dengan PID dan `startedAt` yang sama melewati `stackly daemon restart` dan `SIGKILL` ke `stacklyd`. Entri `HandoverProof::new("official/valkey", "1", "spec-0040-ac-19")` baru ditambahkan ke `HANDOVER_PROVEN` sesudah record test itu `PASS`. Sampai saat itu `daemon.restart_preview` melaporkan instance Valkey sebagai `stop_restore` dengan alasan `handover_unproven`, dan itu sah untuk masuk catalog.

Status test semua AC mulai dari `NOT_RUN`. Lulus di fixture bukan evidence native, dan artefak, sesi GUI, atau launchd yang tidak tersedia dicatat `BLOCKED`.

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md): password selalu wajib dan dikirim sebagai hash SHA-256 di file ACL terpisah; readiness lewat probe `exec` yang menjalankan `valkey-cli` milik runtime sebagai user `stackly-readiness` yang hanya boleh `PING`; satu setting `persistence` ber tiga nilai (`rdb`, `aof`, `volatile`) yang tidak bisa diganti sesudah init; mode itu dipilih lewat file include yang namanya memuat nilai setting; parameter milik Stackly di argv dan server berjalan tanpa argumen file config; peringatan mode volatile lewat `description` field; stop lewat `SIGTERM`; tanpa reload; dan entri handover hanya sesudah test native launchd lulus.

Paket Valkey hanya data (manifest, schema, template). Yang tidak muat di primitive `stackly/v1alpha1` hari ini masuk sebagai dua perubahan core additive yang generik: format `valkeyConf` dan filter `sha256Hex`.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Skill async relevan hanya untuk test native yang menunggu readiness dan event; perubahan core di sini sync dan tanpa IO.

## Feature design

### Batas fitur

- **Di dalam**: paket `providers/valkey`, format template `valkeyConf`, filter template `sha256Hex` (bersama fitur 43), golden test, fixture form, test native lewat daemon, skenario dan test konformitas, evidence kompatibilitas, spike dari paket (hapus recipe tulisan tangan), entri catalog dan corpus, blok `connection`, `openWith`, dan `remedies`, dan test native launchd untuk handover.
- **Di luar**: backup dan restore (fase 09), adoption data Valkey atau Redis lama (fitur 57), environment (fase 08), TLS, replikasi, cluster, Sentinel, module, socket Unix untuk klien, reload config tanpa restart, ganti mode persistence sesudah init, dan Redis sebagai provider (PRD 22.4: provider terpisah).

### Paket

```text
providers/valkey/
├── provider.yaml
├── versions.json                         # dibangkitkan cargo xtask codegen dari runtimes/inventory.toml
├── schemas/
│   └── settings.schema.json
├── templates/
│   ├── valkey.conf.tmpl                  # format valkeyConf
│   ├── users.conf.tmpl                   # format valkeyConf, sensitive
│   ├── persistence-rdb.conf.tmpl         # format plain
│   ├── persistence-aof.conf.tmpl         # format plain
│   └── persistence-volatile.conf.tmpl    # format plain
├── tests/
│   └── scenarios.yaml
├── README.md
└── LICENSE-NOTICES.md
```

- Paket dimulai dengan `cargo xtask provider new valkey`, lalu isinya diganti dengan isi spec ini (urutan *Dari contoh ke paket resmi* spec 0036). `versions.json` tulisan tangan hasil scaffold dihapus dan dibangkitkan `cargo xtask codegen`.
- `README.md` (bahasa Inggris) menjelaskan: tiga mode persistence dan apa yang hilang di tiap kejadian (tabel *Durability per mode*); bahwa user `stackly-readiness` ditulis ulang setiap start dan hanya boleh `PING`; bahwa port, bind, folder data, dan mode persistence dimiliki Stackly di argv; bahwa `CONFIG SET` hanya berlaku sampai restart berikutnya, kecuali file yang ditulisnya: snapshot yang kamu tulis sendiri (`SAVE`, `BGSAVE`, atau save point lewat `CONFIG SET`) dimuat lagi di start berikutnya, juga di mode `volatile`, sampai `dump.rdb` dihapus saat instance berhenti; bahwa URL `redis://` adalah skema yang dipakai kebanyakan library klien dan servernya tetap Valkey; dan bahwa mengganti mode persistence berarti membuat instance baru.
- `LICENSE-NOTICES.md` mengikuti tiga heading spec 0038: `## Package` (`MIT OR Apache-2.0`), `## Runtime` (Valkey, `BSD-3-Clause`, link lisensi dari inventory, dibangun maintainer dan di host Stackly, dengan komponen vendored yang lisensinya ada di `LICENSES/` artefak), dan `## Third party material in this package` (`None.`).

### Manifest

```yaml
apiVersion: stackly/v1alpha1
kind: Provider
metadata:
  id: official/valkey
  software: valkey
  displayName: Valkey
  revision: "1.0.0"
  category: cache
  description: Valkey in-memory key-value store. Each instance is one standalone server with a password, a loopback port, and its own persistence mode.
compatibility:
  platforms:
    - { os: macos, arch: aarch64 }
runtime:
  executable: bin/valkey-server
  executionMode: foreground
  versionCheck:
    args: ["--version"]
    pattern: '^Valkey server v=([0-9]+\.[0-9]+\.[0-9]+) '
    timeoutSeconds: 10
capabilities:
  multiInstance: true
  persistence: true
  backup: [cold]
resources:
  endpoints:
    client: { transport: tcp, purpose: client, defaultHost: "127.0.0.1", defaultPort: 6379 }
configFiles:
  valkeyConf:
    template: templates/valkey.conf.tmpl
    format: valkeyConf
    destination: "${instance.config}/valkey.conf"
  persistenceRdb:
    template: templates/persistence-rdb.conf.tmpl
    format: plain
    destination: "${instance.config}/persistence-rdb.conf"
  persistenceAof:
    template: templates/persistence-aof.conf.tmpl
    format: plain
    destination: "${instance.config}/persistence-aof.conf"
  persistenceVolatile:
    template: templates/persistence-volatile.conf.tmpl
    format: plain
    destination: "${instance.config}/persistence-volatile.conf"
  users:
    template: templates/users.conf.tmpl
    format: valkeyConf
    destination: "${instance.secrets}/users.conf"
    sensitive: true
lifecycle:
  initialize:
    - { primitive: createManagedDirectories }
  start:
    executable: "${runtime.executable}"
    args:
      - "--include"
      - "${instance.config}/valkey.conf"
      - "--include"
      - "${instance.config}/persistence-${settings.persistence}.conf"
      - "--include"
      - "${instance.secrets}/users.conf"
      - "--bind"
      - "${endpoint.client.host}"
      - "--port"
      - "${endpoint.client.port}"
      - "--dir"
      - "${instance.data}"
      - "--daemonize"
      - "no"
      - "--protected-mode"
      - "yes"
      - "--set-proc-title"
      - "no"
      - "--always-show-logo"
      - "no"
      - "--tcp-backlog"
      - "128"
      - "--enable-protected-configs"
      - "no"
      - "--enable-debug-command"
      - "no"
      - "--enable-module-command"
      - "no"
    cwd: "${instance.data}"
    env: { TZ: UTC }
  stop: { strategy: gracefulProcess, signal: term }
readiness:
  probe:
    type: exec
    command:
      executable: "${runtime.executables.cli}"
      args:
        - "-e"
        - "-h"
        - "${endpoint.client.host}"
        - "-p"
        - "${endpoint.client.port}"
        - "--user"
        - "stackly-readiness"
        - "-t"
        - "2"
        - "PING"
      cwd: "${instance.temp}"
      env: { VALKEYCLI_AUTH: nopass }
    successExitCodes: [0]
  intervalMilliseconds: 500
  attemptTimeoutMilliseconds: 3000
health:
  intervalSeconds: 10
  attemptTimeoutMilliseconds: 3000
  failureThreshold: 3
timeouts: { readinessSeconds: 120, recoverySeconds: 300, stopSeconds: 60 }
backup:
  cold: { paths: [data] }
permissions:
  managedFilesystem: [data, config, temp, run]
  executeRuntimeBinary: true
  additionalExecutables: { cli: bin/valkey-cli }
connection:
  - name: uri
    title: Connection URL
    template: "redis://default:${secret.password}@${endpoint.client.address}/0"
  - name: valkey
    title: Valkey URL
    template: "valkey://default:${secret.password}@${endpoint.client.address}/0"
openWith:
  - name: tableplus
    title: TablePlus
    kind: app
    app:
      bundleIds: ["com.tinyapp.TablePlus"]
      url: "redis://default@${endpoint.client.address}/0"
    copySecret: password
  - name: cli
    title: valkey-cli
    kind: terminal
    terminal:
      executable: "${runtime.executables.cli}"
      args: ["-u", "valkey://default@${endpoint.client.address}/0", "--askpass", "--no-auth-warning"]
    copySecret: password
remedies:
  - name: readiness_user
    when:
      readinessDetailContains: "WRONGPASS"
    title: "The readiness user stackly-readiness is missing or changed"
    steps:
      - text: "Stackly checks that this instance is ready by sending PING as the user stackly-readiness, which may run PING and nothing else. That sign in fails now."
      - text: "Restart the instance. Stackly writes this user again before every start."
  - name: aof_damaged
    when:
      failureCodes: [crash_loop, process_exited]
      excerptContains: "Bad file format reading the append only file"
    title: "Valkey found a damaged append only file"
    steps:
      - text: "Valkey stops when the append only file in this instance’s data folder is damaged. Stackly never repairs or deletes data on its own."
      - text: "Copy the data folder somewhere safe first."
        path: "${instance.data}"
      - text: "Valkey’s own repair tool is valkey-check-aof, next to valkey-server in the runtime’s bin folder. Run it with --fix on this manifest file, then start the instance again. The repair drops every write after the damaged part."
        path: "${instance.data}/appendonlydir/appendonly.aof.manifest"
```

Catatan per bagian:

- **`runtime` ditulis gaya blok**, dengan `executable` di barisnya sendiri. Langkah `process` harness masih membaca `runtime.executable` lewat pembaca baris (temuan terbuka spec 0036), jadi blok satu baris akan gagal palsu.
- **Tanpa argumen file config.** `valkey-server` menerima paling banyak satu file config sebagai argumen pertama. Paket ini tidak memberinya, dan memuat ketiga file lewat `--include`. Akibatnya Valkey menganggap dirinya berjalan tanpa file config, sehingga `CONFIG REWRITE` ditolak dan tidak pernah menulis ke folder config (yang akan terbaca sebagai drift spec 0029). Opsi argv diproses menurut urutan, jadi `--bind`, `--port`, `--dir`, dan sisanya mengalahkan isi file include.
- **`--include` untuk mode persistence** memuat token `${settings.persistence}`. Field itu `enum`, bukan di awal argumen, dan bukan `apply: reload`, jadi aturan `argument_injection_risk` dan aturan field reload spec 0009 tidak terkena. Ketiga file mode selalu dirender; argv memilih satu.
- **`--set-proc-title no`** menjaga argv proses tetap seperti yang dijalankan. Tanpa itu Valkey mengganti judul prosesnya menjadi `valkey-server 127.0.0.1:<port>`, dan pembersihan root harness yang mencari proses lewat path root di argumen (spec 0036 AC-12) tidak lagi menemukannya.
- **`--tcp-backlog 128`** sama dengan `kern.ipc.somaxconn` bawaan macOS, supaya log start tidak memuat peringatan backlog di setiap start.
- **`--enable-protected-configs`, `--enable-debug-command`, `--enable-module-command` `no`** adalah bawaan Valkey; ditulis eksplisit supaya `dir` dan `dbfilename` tetap tidak bisa digeser `CONFIG SET` walau bawaan upstream kelak berubah.
- **`env: { TZ: UTC }`** membuat cap waktu log Valkey UTC, hasil yang sama dengan PostgreSQL (yang memakai `log_timezone` di config nya, spec 0019). Valkey tidak punya setting zona waktu sendiri, jadi caranya lewat env.
- **Probe**: `-e` membuat `valkey-cli` keluar 1 saat balasan server adalah error (`LOADING`, `NOAUTH`, `WRONGPASS`, `NOPERM`). `--user` butuh password; user `stackly-readiness` ber `nopass`, jadi nilai apa pun diterima, dan `VALKEYCLI_AUTH: nopass` adalah teks tetap, bukan secret. `-t 2` membuat `valkey-cli` menyerah sebelum `attemptTimeoutMilliseconds` 3000.
- **`logs`** sengaja tidak ada di `managedFilesystem`: `logfile` bawaan kosong dan `daemonize no`, jadi Valkey menulis log ke stdout, yang ditangkap spool spec 0010 dan store spec 0027.
- **`capabilities.persistence: true`** berlaku untuk paket, bukan untuk setiap instance: instance `volatile` tidak menyimpan apa pun, dan itu dinyatakan di field `persistence`. `metrics` dan `reload` tidak dideklarasikan (tidak ada endpoint monitoring, dan lihat *Reload*).
- **`openWith`**: TablePlus mendaftarkan skema `redis` (dicek di Mac ini, rationale *Fakta runtime*). `valkey-cli` memakai `--askpass`, jadi password diketik atau ditempel dari clipboard tersembunyi dan tidak pernah ada di argv; `--no-auth-warning` membuang peringatan yang muncul karena `-u`.
- **Tanpa `compatibility.dataMarker`**: Valkey tidak menulis file penanda versi di folder data. Dengan satu line (`9.1`) dan tanpa `lineCompatibility`, `check_data_compatibility` tetap menolak runtime line lain lewat `data_line` di registry (BR-009 ketat).

### Settings schema

| Field | Tipe | Default | Batas | `apply` | Visibility | Dipakai di |
| --- | --- | --- | --- | --- | --- | --- |
| `password` | string, `secret: true`, `printableOnly: true` | tidak ada (wajib; create memakai `generate` kecuali pemanggil memberi nilai) | panjang 8 sampai 128 | `restart` | basic | hash di `users.conf`; `connection`; `copySecret` |
| `persistence` | string, `enum` `rdb`, `aof`, `volatile` | `rdb` | | `immutableAfterInit` | basic | nama file include di argv start |
| `maxMemoryMb` | integer | 0 | 0 sampai 1048576 | `restart` | basic | `maxmemory` (dalam MB; 0 berarti tanpa batas) |
| `maxMemoryPolicy` | string, `enum` `noeviction`, `allkeys-lru`, `allkeys-lfu`, `allkeys-random`, `volatile-lru`, `volatile-lfu`, `volatile-random`, `volatile-ttl` | `noeviction` | | `restart` | advanced | `maxmemory-policy` |
| `logLevel` | string, `enum` `debug`, `verbose`, `notice`, `warning` | `notice` | | `restart` | advanced | `loglevel` |
| `notifyKeyspaceEvents` | string | `""` (kosong) | panjang 0 sampai 16, `pattern` `[KEg$lshzxetdmnA]*` | `restart` | advanced | `notify-keyspace-events` |

Urutan deklarasi `properties` sama dengan urutan tabel. Root `title` `Valkey settings`, `required: ["password"]`, `additionalProperties: false`.

`title` dan `description` (bahasa Inggris, persis):

| Field | `title` | `description` |
| --- | --- | --- |
| `password` | `Password` | `Password of the default user. Every client needs it. A new value takes effect at the next start.` |
| `persistence` | `Persistence` | `What survives a stop. rdb: snapshots on disk now and then and on a normal stop; a crash loses the writes since the last snapshot. aof: every write is logged and synced about once a second. volatile: memory only; every stop, restart, or crash loses all data, and there is nothing to back up or restore. Set once when the instance is created.` |
| `maxMemoryMb` | `Max memory (MB)` | `Most memory Valkey uses for data. 0 means no limit.` |
| `maxMemoryPolicy` | `Eviction policy` | `What Valkey does when the memory limit is reached. noeviction refuses new writes; the others remove keys.` |
| `logLevel` | `Log level` | `How much Valkey writes to the instance log.` |
| `notifyKeyspaceEvents` | `Keyspace notifications` | `Which key events Valkey publishes, as Valkey flag letters, for example Ex for expired keys. Empty turns them off.` |

- `password` ber `apply: restart`, tidak seperti PostgreSQL (`immutableAfterInit`): hash di `users.conf` dirender ulang dari Keychain sebelum setiap start, jadi `secret.set` yang mengganti nilainya melaporkan efek `next_start` (spec 0011) dan berlaku sesudah restart. Ini secret field resmi pertama dengan kelas `restart`; AC-13 membuktikannya.
- `printableOnly: true` karena klien membaca password sebagai satu baris (`valkey-cli --askpass`, URL), walau hash sendiri menerima byte apa pun.
- `notifyKeyspaceEvents` adalah satu satunya setting teks bebas. Ia ada karena library yang umum (misalnya penyimpanan sesi yang menunggu event key kedaluwarsa) membutuhkannya, dan huruf flag nya memuat `$`, yang ditolak escaper `natsConf`; inilah nilai yang membuat format `valkeyConf` punya pekerjaan nyata di paket ini (rationale Keputusan 14).
- Nilai di luar settings sengaja tetap: nama user `default`, jumlah database 16, save point mode `rdb`, dan `appendfsync everysec` mode `aof`. Masing masing bisa menjadi setting lewat update spec ini secara additive (Follow-up).

### Template config

`templates/valkey.conf.tmpl` (format `valkeyConf`):

```text
{{! Rendered before every start. }}
# Managed by Stackly. Change these values on the instance's Configuration
# page, not here. The port, the bind address, the data folder, and the
# persistence mode are set on the valkey-server command line by Stackly.
maxmemory {{ settings.maxMemoryMb }}mb
maxmemory-policy {{ settings.maxMemoryPolicy }}
loglevel {{ settings.logLevel }}
notify-keyspace-events {{ settings.notifyKeyspaceEvents }}
```

`templates/users.conf.tmpl` (format `valkeyConf`, `sensitive: true`):

```text
{{! Rendered before every start. Holds a hash of the password, never the password. }}
# Managed by Stackly. Written again before every start, removed after a clean stop.
user default on #{{ secret.password | sha256Hex }} ~* &* +@all
user stackly-readiness on nopass -@all +ping
```

`templates/persistence-rdb.conf.tmpl` (format `plain`):

```text
# Managed by Stackly. Persistence mode: rdb (snapshots).
# Used only when the instance's Persistence setting is rdb.
# Valkey writes dump.rdb in the data folder after 3600 s with 1 change, after
# 300 s with 100 changes, after 60 s with 10000 changes, and on a normal stop.
# A crash or a power loss loses every write since the last snapshot.
save 3600 1 300 100 60 10000
appendonly no
```

`templates/persistence-aof.conf.tmpl` (format `plain`):

```text
# Managed by Stackly. Persistence mode: aof (append only file).
# Used only when the instance's Persistence setting is aof.
# Valkey logs every write in appendonlydir/ in the data folder and syncs the
# log to disk about once a second. A power loss can lose about the last second.
save ""
appendonly yes
appendfsync everysec
aof-use-rdb-preamble yes
```

`templates/persistence-volatile.conf.tmpl` (format `plain`):

```text
# Managed by Stackly. Persistence mode: volatile (memory only).
# Used only when the instance's Persistence setting is volatile.
# Nothing is written to disk. Every stop, restart, or crash loses all data.
save ""
appendonly no
```

- `maxmemory {{ settings.maxMemoryMb }}mb` menghasilkan `256mb` atau `0mb` tanpa kutip; `0mb` dibaca Valkey sebagai 0 (tanpa batas). Dua nilai enum dan teks flag keluar berkutip (`"noeviction"`, `"notice"`, `""`, `"K$"`), bentuk yang diterima Valkey.
- `user default on #<hex> ~* &* +@all`: menambah hash password menghapus sifat `nopass` bawaan user `default`, jadi tanpa login setiap perintah ditolak `NOAUTH`. Bentuk `#<hex>` tidak boleh berkutip, karena itu filter `sha256Hex` menulis mentah.
- Tidak ada path dan tidak ada port di template mana pun, jadi path dengan spasi atau Unicode tidak butuh escape, dan `init_port_issues` (spec 0009 AC-21) kosong.
- `tests/scenarios.yaml`:

```yaml
# The scenarios cargo xtask provider test runs for this package (spec 0036).
# Settings never hold a secret: the daemon generates the password.
schema: stackly.provider-scenarios/v1
scenarios:
  - name: default
  - name: aof
    settings: { persistence: aof }
  - name: volatile
    settings: { persistence: volatile }
```

### Durability per mode

Tabel ini adalah isi README, dasar teks `description`, dan harapan test AC-6 sampai AC-8. Baris stop normal dan crash proses diuji; baris kehilangan daya berasal dari cara kerja Valkey dan tidak disimulasikan.

| Mode | Yang ada di disk | Stop normal lalu start | Crash proses (`SIGKILL`) lalu respawn | Kehilangan daya host (tidak diuji) |
| --- | --- | --- | --- | --- |
| `rdb` | `dump.rdb` | semua data kembali (snapshot terakhir ditulis saat stop) | data sampai snapshot terakhir; tulisan sesudahnya hilang | sama dengan crash proses |
| `aof` | `appendonlydir/` (manifest, base, incr) | semua data kembali | semua tulisan yang sudah diterima Valkey kembali | sampai kira kira satu detik terakhir bisa hilang |
| `volatile` | tidak ada | semua data hilang | semua data hilang | semua data hilang |

Restart daemon terencana juga menghentikan lalu menyalakan lagi instance Valkey selama `HANDOVER_PROVEN` belum memuat Valkey (jalur `stop_restore`, AC-19). Untuk instance `volatile` itu berarti semua data hilang, sama dengan stop normal.

### Inisialisasi

Init hanya `createManagedDirectories`: folder `data`, `config`, `temp`, dan folder run ada, 0700. Tidak ada config yang dirender saat init: `RunPlan.render` spec 0010 menulis kelima file sebelum setiap spawn, dan port endpoint baru dipilih saat start. Folder data yang kosong adalah keadaan awal yang sah untuk ketiga mode. Pemeriksaan folder data tidak kosong saat create tetap milik spec 0020 (`instance.data_not_empty`); data Valkey atau Redis lama tidak pernah diadopsi diam diam (fitur 57).

### Start dan parameter milik Stackly

| Parameter | Sumber | Kenapa di argv |
| --- | --- | --- |
| file yang dimuat | tiga `--include` | server berjalan tanpa file config utama, jadi `CONFIG REWRITE` tidak bisa menulis |
| `bind` | `${endpoint.client.host}` (loopback, dijaga plan spec 0009) | mengalahkan isi file include |
| `port` | `${endpoint.client.port}` (`Fixed` atau `Auto` spec 0010) | sama |
| `dir` | `${instance.data}` | path utuh tanpa escape; `dir` tidak bisa digeser `CONFIG SET` (protected config) |
| mode persistence | nama file include dari `settings.persistence` | satu sumber, tidak bisa tertukar oleh isi `valkey.conf` |
| `daemonize`, `protected-mode`, `set-proc-title` | `no`, `yes`, `no` | kontrak proses spec 0010 dan batas loopback |

`CONFIG SET` oleh user `default` tetap bekerja untuk parameter yang bisa diubah saat jalan (misalnya `maxmemory`, `port`, `appendonly`), dan hanya berlaku sampai proses berhenti: start berikutnya kembali ke argv dan settings Stackly. Itu disengaja dan dicatat di README dan di *Edge cases*.

### Auth lokal

| User | Login | Hak |
| --- | --- | --- |
| `default` | password dari Keychain; server hanya menyimpan hash SHA-256 nya | semua perintah, semua key dan channel |
| `stackly-readiness` | `nopass` (password apa pun diterima) | hanya `PING`; tanpa key, tanpa channel |
| user buatan pengguna (`ACL SETUSER` saat jalan) | sesuai pengguna | hilang saat restart, karena tidak ada file ACL yang ditulis Valkey |

User `stackly-readiness` bisa dipakai proses mana pun yang bisa menjangkau port loopback itu, termasuk user OS lain di Mac yang sama. Yang ia dapat hanya jawaban `PONG`, sama kelasnya dengan `/healthz` NATS dan `/ping` Traefik yang sudah dinyatakan sebagai residual. Ia sengaja tanpa password supaya probe yang berjalan setiap 10 detik tidak membaca Keychain dan tidak menyimpan password di memori daemon (alasan yang sama dengan role `stackly_readiness` PostgreSQL, spec 0019).

### Readiness dan health

- Probe `exec` menjalankan `${runtime.executables.cli}` dengan argv dan env di manifest. Sukses berarti exit 0, yaitu Valkey menjawab `PONG` sebagai `stackly-readiness`.
- `PING` ditolak `LOADING` selama dataset dimuat (`rdb` atau `aof` yang besar), jadi `ready` berarti data sudah ada di memori. Koneksi yang ditolak, `NOAUTH`, `WRONGPASS`, dan `LOADING` masing masing membuat `valkey-cli -e` keluar 1 dengan pesan di stderr.
- Detail kegagalan memakai format probe `exec` spec 0019 (baris tidak kosong pertama stderr lewat `Redactor`, dipotong 256 karakter). Contoh: `LOADING Valkey is loading the dataset in memory`, `AUTH failed: WRONGPASS invalid username-password pair or user is disabled.`.
- `health` tanpa `probe` memakai probe readiness, dengan interval 10 detik.
- Env child tertutup (spec 0009) dan `HOME` adalah `${instance.temp}`, jadi `~/.valkeyclirc` dan `VALKEYCLI_AUTH` milik pengguna tidak pernah terbaca.

### Test LOADING

Dataset besar membuat jendela `LOADING` yang pendek dan tidak stabil (628 MiB hanya memberi sekitar 1,5 detik di Mac ini). Test AC-5 memakai cara yang stabil dan kecil:

1. Test menyalin paket ke `TestRoot` (pola `copy_tree` di `apps/daemon/tests/postgresql/format.rs`) dan menambah empat entri di akhir `lifecycle.start.args` salinan itu: `--key-load-delay`, `2000`, `--loading-process-events-interval-bytes`, `1024`. Keduanya opsi debug Valkey: yang pertama menunda 2000 mikrodetik per key saat memuat, yang kedua membuat Valkey melayani klien setiap 1024 byte yang dimuat (nilai terkecil yang diterima).
2. Instance mode `rdb` dibuat dari salinan itu lewat daemon, di start, diisi 2000 key lewat `valkey-cli` sebagai user `default`, lalu di stop (snapshot ditulis).
3. Start berikutnya memuat snapshot selama kira kira 4 sampai 6 detik. Selama itu test membaca `instance.get` setiap 200 ms dan menuntut paling sedikit satu readiness `not_ready` dengan detail berawalan `LOADING`, lalu `ready`; dan satu percobaan argv probe yang dijalankan test sendiri keluar 1 dengan stderr berawalan `LOADING`.

Mode `aof` tidak dipakai untuk test ini, karena saat memuat AOF Valkey baru melayani klien setiap 1024 perintah, sehingga probe menunggu tanpa jawaban. Salinan paket hanya untuk test ini; paket resmi tidak pernah membawa opsi debug itu.

### Stop

`gracefulProcess` dengan `signal: term`. Valkey menjadwalkan shutdown: di mode `rdb` ia menulis snapshot terakhir (`Saving the final RDB snapshot before exiting`, `DB saved on disk`), di mode `aof` ia men sync file AOF, lalu keluar 0. Klien yang terbuka diputus. Sesudah `stopSeconds` 60, operasi gagal `supervisor.stop_timeout` dan instance tetap `stopping`; force stop milik spec 0026. Strategi `command` (`valkey-cli SHUTDOWN`) tidak dipakai, karena perintah itu butuh password user `default` dan stop `command` tidak menerima secret.

### Reload

Tidak ada. Valkey tidak membaca ulang file config lewat sinyal, dan `CONFIG SET` butuh login user `default`, sedangkan reload `run` tidak menerima secret. Semua setting ber kelas `restart` atau `immutableAfterInit`, dan halaman Configuration menampilkan dampak restart itu (spec 0029).

### Perubahan core additive

Dua perubahan, keduanya generik dan dihitung sebagai ukuran RISK-07 untuk Valkey. Tidak ada yang mengubah perilaku paket yang sudah ada.

| No | Perubahan | Tempat | Isi | Kenapa Valkey butuh | Pemilik |
| --- | --- | --- | --- | --- | --- |
| 1 | Format template `valkeyConf` | `crates/core/src/provider/template.rs` (`TemplateFormat::ValkeyConf`, kode `valkeyConf`), pesan `unknown_template_format` di `validate.rs` | escaper di AC-2; `allows_tags` benar; tanpa parse ulang (seperti `natsConf` dan `postgresqlConf`), dijaga test native terhadap `CONFIG GET` | sintaks kutip Valkey berbeda dari format yang ada (`\"` dan `\\` di dalam kutip ganda, `yes` dan `no`); nilai `notifyKeyspaceEvents` bisa memuat `$`, yang ditolak `natsConf`; dan template `users.conf` butuh format yang menerima tag | fitur 40 |
| 2 | Filter template `sha256Hex` | `template.rs` (parser tag, cek statis), `crates/core/src/provider/plan.rs` (`Segment::SecretHash`, `materialize_with`), memakai `secret::delivery::hash` yang sudah ada | aturan di AC-3 | mekanisme `hash` spec 0011 (urutan 2) untuk password ACL Valkey; sintaks nya belum pernah dibangun | dipakai bersama fitur 43; dibangun fitur 40 (keputusan lead, 2026-11-12) |

Kontrak filter `sha256Hex` disepakati dengan sesi `arc-f43-spec` pada 2026-11-12, dan AC-2 spec 0043 menulis aturan yang sama (sintaks, tempat yang sah, kode penolakan, output mentah, segmen plan). Spec ini menambah dua hal yang spec 0043 belum sebut dan yang sudah dikabarkan ke sesi itu: bentuk JSON segmen (`secretHash`, `algo` `sha256_hex`) dan `template_syntax` untuk nama filter kosong. Syarat perubahan additive spec 0036 berlaku untuk keduanya: nilai yang tidak dikenal ditolak saat validasi; test corpus AC-6 lulus tanpa mengubah `frozen.json`; schema editor di `schemas/provider/` tidak berubah (`format` di sana bertipe string tanpa daftar nilai, dan filter ada di template), jadi `cargo xtask codegen --check` tetap lolos tanpa file baru; corpus fuzz `provider_template` mendapat contoh format baru dan contoh filter; dan paket `providers/valkey` memakai keduanya. Tidak ada kode issue baru, jadi test jumlah kode di `issue.rs` tidak berubah.

Yang tidak berubah: `crates/protocol/` (tidak ada method, DTO, kode error, atau event baru), registry (tidak ada migrasi), `stackly-platform`, daemon, CLI, dan Swift. Kalau build menemukan bahwa salah satu harus berubah, itu perubahan spec lewat `/architect`, dan nomor protokol atau migrasinya adalah nomor bebas berikutnya yang ditetapkan lead saat build.

### Evidence kompatibilitas

Bentuk dari spec 0018, ditulis test native untuk artefak ini:

```json
{
  "schema": "stackly.compatibility-evidence/v1",
  "providerRevision": { "id": "official/valkey", "revision": "1.0.0", "manifestSha256": "<hex>" },
  "checks": ["init", "start", "readiness", "stop", "persistence_rdb", "persistence_aof", "volatile", "crash_recovery", "adoption"],
  "knownLimitations": ["power_loss_not_simulated"]
}
```

`knownLimitations` berisi kode snake_case (paling panjang 64 karakter, aturan `is_code` di `xtask/src/runtime/compatibility.rs`), bukan kalimat. `cargo xtask runtime compat-record` menyalin record itu ke `evidence/compatibility/`, dan `cargo xtask codegen` mengubah PASS terbaru menjadi compatibility record di `versions.json`. Evidence BR-025 ditulis suite native paket ini (`apps/daemon/tests/valkey/compat.rs`), bukan test konformitas, karena check persistence dan crash tidak ada di langkah harness (jawaban Follow-up spec 0036).

### Data model

Tidak ada tabel, kolom, atau migrasi baru. Yang disimpan per instance sudah dimodelkan spec 0004, 0005, 0010, dan 0011:

| Data | Tempat | Isi untuk Valkey |
| --- | --- | --- |
| provider revision | registry, `provider_revision_dir` | `official/valkey@1.0.0#<sha>`, capability `multi_instance`, `persistence`, `cold_backup` |
| instance | registry | `data_line` `9.1` dan `data_version` dari record runtime saat create |
| config revision | registry (`config_revisions`) | nilai settings; `password` sebagai teks reference |
| secret | Keychain plus registry `secret_references` | reference `local:instance/<id>/password` |
| endpoint | registry (`instance_endpoints`) | `client` (tcp, host, `Fixed` atau `Auto`, `bound`) |
| data | folder data instance | milik Valkey: `dump.rdb` (mode `rdb`), `appendonlydir/` (mode `aof`), kosong (mode `volatile`) |
| config | folder config instance | empat file render tidak sensitif, ditulis ulang sebelum setiap spawn, ikut `config-history` |
| file user | `<run>/secrets/users.conf` | hash password dan user readiness; di luar backup, `config-history`, dan diagnostics (spec 0011) |

### API surface

Tidak ada method IPC, command CLI, atau DTO baru. Permukaannya adalah paket dan fungsi berikut:

| Fungsi atau artefak | Tempat | Input utama | Output utama | Pemanggil | Error utama |
| --- | --- | --- | --- | --- | --- |
| paket `providers/valkey` | repo | file paket | `ValidatedPackage` | catalog, create, harness, spike, test | `manifest.invalid` |
| `TemplateFormat::ValkeyConf` | `core::provider::template` | nilai context | teks ter escape | renderer, `render_secret_placeholder` | `UnrepresentableValue`, `unknown_template_format` |
| filter `sha256Hex` dan `Segment::SecretHash` | `core::provider::template`, `core::provider::plan` | `PlanSecret`, `ResolvedSecrets` | 64 hex di file sensitif | `RenderedContent::materialize`, `materialize_sample`, `frozen_facts` | `template_type` saat validasi; `SecretUnavailable` saat materialize |
| test native | `apps/daemon/tests/valkey.rs` dan `apps/daemon/tests/valkey/` | daemon, cache runtime | evidence `native` dan `compatibility.json` | `cargo test -p stacklyd --test valkey -- --ignored` | `BLOCKED` tanpa artefak |
| test konformitas | `apps/daemon/tests/provider_conformance.rs` | paket, cache runtime | report `stackly.provider-conformance/v1` | `cargo test -p stacklyd --test provider_conformance -- --ignored valkey` | `BLOCKED` tanpa artefak |
| test install | `apps/daemon/tests/native_runtime_install.rs` | cache runtime | runtime `installed` | `cargo test -p stacklyd --test native_runtime_install -- --ignored valkey` | `BLOCKED` tanpa artefak |
| test handover | `apps/daemon/tests/native_launchd.rs` | sesi GUI, launchd | record evidence AC-19 | kamu, lewat `cargo test -p stacklyd --test native_launchd -- --ignored valkey` | `BLOCKED` tanpa sesi GUI |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| create | password | `SecretService::put(PutTarget::New, Generate)` kecuali `params.secrets` memberi `Value` atau `CopyFrom` (spec 0010, 0011) |
| create | reference secret | `local:instance/<id>/password`, purpose dari nama field (spec 0011) |
| create | mode persistence | `params.settings.persistence`, default `rdb` dari schema |
| create | permintaan port `client` | `params.endpoints`, default `Auto`; host dari `defaultHost` `127.0.0.1` |
| create | `data_line`, `data_version` | record runtime yang dipilih (fitur 18) |
| start | host dan port | endpoint `client` (spec 0004, 0010) |
| start | folder data, config, secrets | `InstancePaths` dan `instance_secrets_dir(short key)` (spec 0004) |
| start | nama file include mode | literal `persistence-`, nilai `settings.persistence` dari config revision aktif, literal `.conf` |
| start | zona waktu log | env literal manifest `TZ=UTC` |
| start | sisa argv | literal manifest |
| render | `maxmemory`, `maxmemory-policy`, `loglevel`, `notify-keyspace-events` | settings tervalidasi dari config revision aktif |
| render | hash password | `secret::delivery::hash(Sha256Hex, nilai dari ResolvedSecrets)`, hasil `resolve_for_start` |
| render | aturan user `default` dan `stackly-readiness` | literal template `users.conf.tmpl` |
| render | save point, `appendfsync` | literal template mode |
| probe | program | root runtime ditambah `bin/valkey-cli` dari `additionalExecutables` |
| probe | host, port | endpoint `client` |
| probe | user, perintah, timeout | literal argv manifest |
| probe | password user readiness | env literal manifest `VALKEYCLI_AUTH=nopass` (bukan secret) |
| probe | detail kegagalan | stderr `valkey-cli` (spec 0010), lewat `Redactor` |
| stop | sinyal | `signal: term` di manifest |
| preflight data | line tercatat dan line runtime | `data_line` instance di registry dan record runtime yang diikat (tanpa penanda di folder data) |
| connection | URL | blok `connection`, dirender daemon dan disusun klien dengan `password` (spec 0021) |
| Open In | URL TablePlus, argv `valkey-cli` | blok `openWith` (spec 0070); password lewat clipboard tersembunyi |
| catalog | kartu provider | `metadata` paket dan `versions.json` (spec 0017); mark `Vk` dan tile `red` dari `providers/catalog.toml` |
| `versions.json` | artefak | `runtimes/inventory.toml` lewat `cargo xtask codegen` |
| evidence | `version` subject | baris `valkey_version:` dari `INFO server` instance yang berjalan, dibaca test sebagai user `default` |
| evidence | `digest` subject | `ArtifactRecord.sha256` |
| evidence | `providerRevision` | `ValidatedPackage::key()` |
| test native | file artefak | `stackly_testkit::RuntimeCache::open()` lalu `entry(sha256, size)` dari `versions.json` paket |
| test native | password untuk perintah klien | nilai fixture ber awalan credential fixture spec 0002 yang diberikan test di `params.secrets`, dikirim ke `valkey-cli` lewat env `VALKEYCLI_AUTH`, tidak pernah lewat argv |
| test AC-5 | jendela `LOADING` | salinan paket dengan `--key-load-delay 2000` dan `--loading-process-events-interval-bytes 1024`, mode `rdb`, 2000 key (*Test LOADING*) |
| spike | settings dan secret | skenario `default` paket dan `ResolvedSecrets::for_tests` (spec 0036 *Spike dari paket*) |

### Edge cases dan kegagalan

| Kasus | Yang terjadi | Penanganan |
| --- | --- | --- |
| Port `Fixed` dipakai proses lain | spec 0010 | `supervisor.endpoint_in_use` sebelum spawn |
| Password berisi baris baru | ditolak saat `put` | `secret.invalid_value` (`reason: control_characters`) sebelum Keychain |
| Password sama persis dengan literal argv (misalnya `stackly-readiness` atau `--protected-mode`) | `ensure_not_in_argv` menolak | start gagal `secret.delivery_rejected`; residual yang dinyatakan |
| Dataset besar saat start | `PING` dijawab `LOADING` | tetap `not_ready` sampai selesai; `readinessSeconds` 120 dan `recoverySeconds` 300; deadline lewat membuat operasi `readiness_timeout` sementara proses tetap jalan (spec 0010) |
| File AOF rusak di tengah | Valkey keluar 1 dengan `Bad file format reading the append only file` | crash loop berakhir `failed` dengan cuplikan; remedy `aof_damaged`; Stackly tidak memperbaiki atau menghapus apa pun |
| Ekor AOF terpotong (crash saat menulis) | Valkey memuat sampai bagian yang utuh (`aof-load-truncated yes`, bawaan) | start berhasil; tulisan terakhir yang terpotong hilang, sesuai tabel durability |
| Snapshot gagal ditulis (disk penuh) di mode `rdb` | Valkey menolak tulisan baru dengan `MISCONF` (`stop-writes-on-bgsave-error yes`, bawaan) | terlihat di klien dan di log instance; readiness tetap `ready` karena `PING` dijawab |
| Snapshot terakhir saat stop gagal | Valkey membatalkan shutdown | `supervisor.stop_timeout` sesudah 60 detik, instance tetap `stopping`; force stop spec 0026 |
| Pengguna menghapus user `stackly-readiness` (`ACL DELUSER`) | probe gagal `WRONGPASS` | `not_ready` dengan detail; remedy `readiness_user`; restart menulis ulang user itu |
| Pengguna menjalankan `CONFIG SET port <lain>` | listener pindah, probe gagal konek | `not_ready`; stop tetap bekerja (sinyal ke PID); start berikutnya kembali ke port Stackly |
| Pengguna menjalankan `CONFIG SET appendonly` atau `save` saat jalan | Valkey menulis file mode lain sampai proses berhenti | start berikutnya kembali ke setelan mode instance. File yang tertinggal tetap di folder data: `appendonlydir/` diabaikan oleh mode `rdb` dan `volatile`, tetapi `dump.rdb` dimuat oleh keduanya. Dicatat di README; setting mode instance tidak pernah berubah |
| Pengguna menjalankan `SAVE` atau `BGSAVE` di mode `volatile` | Valkey menulis `dump.rdb`, dan start berikutnya memuatnya (diuji, rationale F28) | bukan data yang hilang, tetapi mode `volatile` lalu tidak kosong sesudah restart. README mengatakannya dan menyebut jalan keluarnya: hapus `dump.rdb` saat instance berhenti. Stackly tidak menghapusnya dan tidak melarang perintah itu (rationale Keputusan 13) |
| Pengguna menjalankan `FLUSHALL` lalu stop di mode `rdb` | snapshot terakhir kosong | data hilang sesuai perintah pengguna; bukan kegagalan |
| Pengguna menjalankan `CONFIG SET bind 0.0.0.0` | Valkey ikut mendengar di semua interface sampai proses berhenti; `protected-mode` tidak menghalangi karena user `default` ber password | residual yang dinyatakan di *Security model*; start berikutnya kembali ke loopback |
| Pengguna mengganti password lewat `ACL SETUSER default` saat jalan | readiness tidak terpengaruh | connection string Stackly basi sampai restart, lalu password Keychain berlaku lagi |
| `CONFIG REWRITE` atau `ACL SAVE` | ditolak Valkey | tidak ada file config yang berubah, jadi tidak ada drift |
| OS membersihkan folder run saat instance jalan | `users.conf` hilang | tidak berpengaruh sampai restart (Valkey membacanya sekali saat start); render sebelum spawn menulis ulang |
| Daemon restart terencana tanpa entri `HANDOVER_PROVEN` | instance dihentikan lalu dinyalakan lagi (`stop_restore`) | `rdb` dan `aof` pulih; `volatile` kosong, sesuai janjinya |
| Dua instance | port, folder data, dan user masing masing | langkah `second_instance` harness; tidak ada file bersama |
| `valkey-cli` menggantung | timeout percobaan | aturan pembantu spec 0010; dihitung sebagai kegagalan probe |
| Folder data berisi file Redis atau Valkey lama saat create | spec 0020 | `instance.data_not_empty`; adoption milik fitur 57 |
| TablePlus tidak terpasang | spec 0070 | entri `tableplus` tidak ditawarkan |

### Asumsi pada spec lain

- **Assumed (spec 0009)**: `--include` dengan token folder dan token setting `enum` dalam satu entri `args` diterima (dicek di `crates/core/src/provider/interpolate.rs` pada 2026-11-12); config `sensitive` hanya di `${instance.secrets}` (AC-20); `RunPlan.render` menulis setiap config sebelum spawn; nilai `env` literal diterima di start dan probe `exec`.
- **Assumed (spec 0010)**: file config sensitif dirender ulang sebelum setiap spawn dan dihapus sesudah stop bersih; proses utama memegang lock di fd 3; `first_ready` menandai `ready` hanya sesudah percobaan yang sukses.
- **Assumed (spec 0011)**: `secret::delivery::hash(HashAlgo::Sha256Hex, ..)` ada (dicek di kode); efek `next_start` untuk field ber kelas `restart`; `printableOnly`; `ensure_not_in_argv`.
- **Assumed (spec 0018)**: `install_cached_artifact`, exec check `runtime.versionCheck` per baris tanpa pembungkus, `cargo xtask runtime compat-record`.
- **Assumed (spec 0036)**: `cargo xtask provider new`, `provider test`, `provider freeze`, `PackageRecipe`, dan `frozen_facts` memakai `RenderedContent::materialize`, sehingga hash ikut di `rendered` corpus tanpa kode baru di xtask.
- **Assumed (spec 0043, ditulis paralel)**: bentuk filter `sha256Hex` sama dengan AC-3, dan fitur 43 memakai filter yang dibangun langkah 2 fitur ini. Kalau spec 0043 berubah sebelum langkah 2 dibangun, lead menyamakan keduanya.

### Jawaban follow-up dari spec lain

- **Spec 0009** (format `valkeyConf` belum ada, satu perubahan core): dibangun langkah 1 (AC-2). Di samping itu filter `sha256Hex` (AC-3) adalah perubahan kedua, bersama fitur 43. Baris tabel *Perluasan additive* spec 0009 untuk keduanya ada di *Usulan update spec lain*.
- **Spec 0003** (hapus recipe spike tulisan tangan dan `// provisional: F40`; baris Valkey tabel *Langkah spike per provider*): langkah 4 (AC-16). Sesudah itu baris Valkey tabel tersebut tidak dipakai lagi; init, start, ready, versi, dan stop dibangun dari paket. Stop spike berubah dari `valkey-cli shutdown nosave` menjadi `SIGTERM` dari plan stop paket, dan spike berjalan dengan password fixture, bukan tanpa auth.
- **Spec 0030** (test native launchd sebelum `HANDOVER_PROVEN`): AC-19 dan langkah 11 dan 12.
- **Spec 0036** ("Fitur 39 sampai 43" dan "Dari contoh ke paket resmi"): paket lewat `cargo xtask provider new` (langkah 3); `valkey` sudah ada di `PROVIDER_WORDS`, jadi `xtask/src/check_deps/source_scan.rs` tidak berubah; fixture form dan entri index (langkah 3); satu test native di `provider_conformance.rs` (langkah 8); catalog lalu `provider freeze` (langkah 10); recipe spike dihapus (langkah 4). Test BR-025 adalah test suite paket, bukan test konformitas.
- **Spec 0017** (*Aturan masuk catalog*): langkah 10, di commit yang sama dengan bukti lifecycle ujung ke ujung.
- **Spec 0011** (urutan mekanisme pengiriman secret): Valkey memakai mekanisme 2 (`hash`), bukan mekanisme 5 (`config_plaintext`) yang ditulis kalimat Consequences spec 0011 untuk Valkey. Usulan koreksi kalimat itu ada di *Usulan update spec lain*.
- **Spec 0070** (entri `openWith` Valkey): `tableplus` dan `cli` di *Manifest*. Redis Insight tidak dimasukkan (rationale Keputusan 10).
- **Spec 0034** (remedy provider lain): dua entri `remedies` di *Manifest* (AC-18).

### Usulan update spec lain

Spec ini tidak mengubah spec lain. Lead atau pemilik spec menerapkan butir ini:

- **Spec 0009**, tabel *Perluasan additive*: baris "Format template `valkeyConf` | 0040 | fitur 40 langkah 1 | 0040 AC-2" dan baris "Filter template `sha256Hex` dan `Segment::SecretHash` | 0040, 0043 | fitur 40 langkah 2 atau fitur 43, yang masuk lebih dulu | 0040 AC-3". Tabel escaper *Template config* mendapat baris `valkeyConf`, dan grammar template mendapat `{{ secret.<field> | sha256Hex }}`. Baris Follow-up "Format config `valkeyConf`" dicentang untuk bagian Valkey.
- **Spec 0011**, Consequences: kalimat "Config plaintext tetap ada untuk Valkey dan NATS" menjadi hanya NATS; Valkey memakai mekanisme 2. Kalimat mekanisme 2 menyebut Valkey di samping ClickHouse.
- **Spec 0003**: baris Follow-up "Fitur 40, 42, dan 43" dicentang untuk bagian Valkey, dan catatan di bawah tabel *Langkah spike per provider* menyebut baris Valkey tidak dipakai lagi.
- **Spec 0030**: baris Follow-up "Fitur 36 sampai 43" mendapat catatan bagian Valkey (AC-19).
- **Spec 0036**, **0034**, dan **0070**: baris Follow-up yang menyebut fitur 40 dicentang untuk bagian Valkey.
- **Spec 0018** AC-20: test `valkey_9_1_installs_from_the_cache` ditambah fitur ini (langkah 7).

### Key invariants

- Tidak ada perintah yang diterima Valkey tanpa login, kecuali `PING` sebagai `stackly-readiness`.
- Password tidak pernah ada di argv, config, spool, log, atau registry; di disk hanya ada hash nya, di folder `secrets/` runtime root.
- Probe readiness dan health tidak pernah login sebagai `default` dan tidak pernah membaca secret.
- Setiap program yang dijalankan untuk instance Valkey berasal dari root runtime yang diikat instance itu; tidak pernah Redis dan tidak pernah dari `PATH`.
- Stackly selalu menjalankan Valkey dengan port, alamat bind loopback, folder data, dan mode persistence dari argv; Valkey tidak pernah menulis ke folder config.
- Mode persistence sebuah instance tidak pernah berubah sesudah init.
- Tidak ada path dan tidak ada port di dalam file config.
- Stackly tidak pernah memperbaiki, menghapus, atau mengganti file data Valkey.

### Security model

- Ancaman yang dijaga: akses tanpa password (user `default` selalu ber password, `protected-mode yes`), akses dari jaringan (setiap start mengikat hanya host endpoint loopback), password di argv, config, atau log (hash di file sensitif, `ensure_not_in_argv`, `Redactor`), tool dari `PATH` (program hanya dari runtime, env tertutup), dan konfigurasi yang bergeser diam diam (parameter milik Stackly di argv, tanpa file config utama, protected config).
- Hash SHA-256 tanpa salt bukan nilai secret menurut spec 0011, tetapi password lemah yang diketik pengguna bisa ditebak offline oleh proses lain milik user yang sama yang membaca `users.conf` selama instance berjalan. Password bangkitan (32 karakter alfanumerik) tidak terkena. Ini residual yang dinyatakan spec 0011 untuk mekanisme `hash`.
- Residual risk lain yang dinyatakan: user OS lain di Mac yang sama bisa mengirim `PING` sebagai `stackly-readiness` dan bisa mengikat port loopback instance yang sedang berhenti (klien lalu mengirim password ke server palsu itu; kelas yang sama dengan catatan 46 audit 2026-11); tanpa TLS karena hanya loopback; user `default` bisa mengubah parameter saat jalan lewat `CONFIG SET`, termasuk `port` dan `bind` (misalnya `bind 0.0.0.0`, yang membuka instance ke jaringan sampai restart berikutnya; hanya pemegang password yang bisa melakukannya); skrip Lua dan fungsi berjalan dengan hak proses Valkey (user OS yang sama).
- Tidak ada data teregulasi dan tidak ada compliance scope baru.

### Configuration required

Tidak ada variabel environment, credential, atau port baru untuk daemon dan CLI. Test native butuh artefak `valkey-9.1.2-r2-macos-aarch64` di cache runtime xtask (`~/Library/Caches/dev.stackly.xtask/runtime/`; sudah ada di Mac ini pada 2026-11-12). Tanpa artefak itu test `BLOCKED` dengan `reason` `artifact valkey-9.1.2-r2-macos-aarch64 is not in the runtime cache` dan `owner` `@ojiepermana`; sesi build tidak pernah mengunduh.

### Critical test scenarios

- Golden: render kelima config (termasuk `users.conf` dengan secret fixture, yang memuat hash dan bukan nilai), `init_plan`, dan `run_plan` di context golden; tidak ada entri argv yang sama dengan nilai secret fixture; argv start memuat `persistence-rdb.conf` untuk default dan `persistence-aof.conf` untuk settings `aof`. Memverifikasi **AC-1**, **AC-12**.
- Format: tabel escaper `valkeyConf` (kutip, backslash, `$`, Unicode, `#`, karakter kontrol ditolak, boolean `yes` dan `no`, integer negatif); instance dengan `notifyKeyspaceEvents` `K$` start dan `CONFIG GET notify-keyspace-events` memuat kedua huruf itu; dan round trip `CONFIG GET proc-title-template` lewat salinan paket yang template nya diberi setting teks bebas (pola `apps/daemon/tests/postgresql/format.rs`). Memverifikasi **AC-2**.
- Filter: `secret_not_allowed_here` di config tidak sensitif; `template_type` untuk filter di `settings.x`, nama filter lain, dua filter, dan tag di format `plain`; `template_syntax` untuk nama filter kosong; plan JSON memuat segmen `secretHash` dengan `algo` `sha256_hex`; hash teks `abc` sama dengan vektor uji standar; `Debug` plan tidak memuat hash; corpus `provider_compat` lulus tanpa perubahan. Memverifikasi **AC-3**.
- Auth: tanpa login `NOAUTH`; password benar lewat dua bentuk `AUTH`; password salah `WRONGPASS`; password `p w"x\y'z` bisa login; `stackly-readiness` ditolak `NOPERM` untuk `GET`, `INFO`, `CONFIG GET`; `lsof -a -p <pid> -i -U` hanya satu listener loopback; isi `users.conf` tidak memuat password dan mode nya 0600; file itu hilang sesudah stop; grep nilai password di spool, log daemon, dan `ps` kosong. Memverifikasi **AC-4**.
- Readiness jujur: dengan salinan paket *Test LOADING*, `instance.get` menampilkan `not_ready` dengan detail `LOADING` lalu `ready`, dan argv probe yang dijalankan langsung keluar 1 dengan `LOADING`, lalu 0; `ACL DELUSER stackly-readiness` membuat `not_ready` sesudah 2 kegagalan dengan detail `WRONGPASS` sementara connect TCP berhasil; `ACL SETUSER stackly-readiness on nopass -@all +ping` membuat `ready` lagi. Memverifikasi **AC-5**.
- Persistence: untuk `rdb` dan `aof`, sentinel bertahan melewati stop dan start, dan file yang diharapkan ada di folder data; untuk `volatile`, sentinel hilang dan folder data kosong. Memverifikasi **AC-6**, **AC-7**.
- Teks peringatan: `description` field `persistence` di `instance.get_create_form` sama persis dengan *Settings schema* dan memuat `loses all data` dan `nothing to back up or restore`. Memverifikasi **AC-7**.
- Crash: per mode, sentinel pertama ditulis, stop dan start normal, sentinel kedua ditulis, tunggu 1,5 detik, `SIGKILL` ke `valkey-server`, tunggu respawn sampai `ready`, lalu baca keduanya; harapan di tabel *Durability per mode*. Memverifikasi **AC-8**.
- Mode tetap: `instance.config.validate` dengan `persistence: aof` pada instance `rdb` membalas `immutable_after_init`. Memverifikasi **AC-9**.
- Parameter milik Stackly: `CONFIG REWRITE`, `ACL SAVE`, `CONFIG SET dir`, dan `CONFIG SET dbfilename` ditolak dengan pesan di AC-10; `CONFIG SET maxmemory 1mb` hilang sesudah restart; isi folder config sama sebelum dan sesudah. Memverifikasi **AC-10**.
- Stop: klien `BLPOP q 600` terbuka, `instance.stop` selesai dalam 10 detik tanpa `SIGKILL`, exit 0, log mode `rdb` memuat `DB saved on disk`. Memverifikasi **AC-11**.
- Tool dari runtime: daemon dengan `PATH` berisi empat umpan; file penanda tidak pernah dibuat; `INFO server` melaporkan versi artefak dan `executable` di root runtime. Memverifikasi **AC-12**.
- Lifecycle dan adopsi: create, start, log UTC lewat `instance.logs_read`, stop; `lsof -d 3` menunjukkan `instance.lock`; `SIGKILL` ke daemon saat `ready`, daemon baru mengadopsi PID yang sama; `secret.set` melaporkan `next_start`, lalu restart menukar password yang diterima. Memverifikasi **AC-13**.
- Konformitas: report `pass` untuk tiga skenario; cache kosong membuat `BLOCKED`. Memverifikasi **AC-14**.
- Evidence: run lengkap menulis `compatibility.json` yang lolos `cargo xtask runtime compat-record`. Memverifikasi **AC-15**.
- Version check dan spike: unit test pattern cocok dengan `Valkey server v=9.1.2 sha=00000000:1 malloc=libc bits=64 build=991715b0e5b85611` dan tidak cocok dengan `valkey-cli 9.1.2`; exec check install lolos; `recipe(repo, "valkey")` adalah `PackageRecipe`; spike native lulus. Memverifikasi **AC-16**.
- Catalog dan corpus: `cargo xtask catalog check`, `cargo xtask provider check` tanpa argumen, dan `provider_compat` lolos. Memverifikasi **AC-17**.
- Connection, Open In, remedy (anggarkan sekitar 40 detik untuk kasus AOF rusak, karena start mengulang spawn sebelum berakhir `crash_loop`): dua connection dengan `masked` URI; `openActions` di `instance.get_detail` (spec 0070) memuat dua entri tanpa token secret; AOF yang dirusak membuat start gagal dan doctor memberi remedy `aof_damaged`; `WRONGPASS` memberi `readiness_user`. Memverifikasi **AC-18**.
- Handover: `daemon.restart_preview` melaporkan `handover_unproven` untuk Valkey sebelum entri ada; test launchd dijalankan kamu. Memverifikasi **AC-19**.

## Build plan

Urutan mengikuti pendekatan Journey proyek: satu jalur dibuat lengkap dulu, yaitu "instance Valkey dari paket sampai ready, menyimpan data sesuai modenya, lalu stop, termasuk semua jalur gagalnya". Milestone 1 membangun semua yang bisa dibuktikan tanpa server nyata. Milestone 2 membuktikan jalur itu pada Valkey 9.1.2 asli lewat daemon. Milestone 3 menutupnya dengan konformitas, evidence, dan catalog. Milestone 4 adalah bukti handover, yang butuh kamu.

**Milestone 1: Perubahan core dan paket, dibuktikan di fixture**

1. Format `valkeyConf`. File: `crates/core/src/provider/template.rs` (`TemplateFormat::ValkeyConf`, `from_code`, `escape_text`, `boolean`, `allows_tags`, `output_parses`, unit test tabel escaper), `crates/core/src/provider/validate.rs` (daftar format di pesan `unknown_template_format`), `crates/core/src/provider/fuzzing.rs` (render ke format baru di target `provider_template`), `crates/core/src/secret/delivery/mod.rs` (test `render_secret_placeholder` untuk format baru), `crates/core/tests/provider_rules.rs` (test aturan format), `docs/providers/manifest-reference.md` (baris `format`). Memenuhi **AC-2** (tanpa round trip native).
2. Filter `sha256Hex`. Fitur 40 yang membangunnya; fitur 43 menunggu langkah ini di `main` (keputusan lead, 2026-11-12). File: `crates/core/src/provider/template.rs` (parser tag dengan satu filter, cek statis, emisi segmen), `crates/core/src/provider/plan.rs` (`Segment::SecretHash`, cabang di `materialize_with`), `crates/core/src/secret/delivery/mod.rs` (`Serialize` untuk `HashAlgo`), `crates/core/tests/provider_rules.rs` (test aturan filter), `crates/core/src/provider/fuzzing.rs` dan satu contoh corpus fuzz `provider_template`, dan satu kalimat di `docs/providers/manifest-reference.md` (bagian template). Teks AC-2 spec 0043 dan AC-3 spec ini menggambarkan kontrak yang sama. Memenuhi **AC-3**.
3. Paket `providers/valkey`: `cargo xtask provider new valkey`, lalu `provider.yaml`, `schemas/settings.schema.json`, lima template, `tests/scenarios.yaml`, `README.md`, `LICENSE-NOTICES.md`; `versions.json` lewat `cargo xtask codegen`; test `valkey_plans_match_the_golden_files` (settings default dan settings `persistence: aof`) dan `valkey_remedies_match_their_failures_and_render_like_the_golden_file` di `crates/core/tests/provider_packages.rs`, mengikuti test PostgreSQL di file yang sama, ditambah entri Valkey di daftar paket test file itu yang memuat paket per nama; satu baris Valkey di tabel test `every_repo_package_reads_its_version_from_the_literal_output_of_its_binary` di file yang sama (argumen `--version`, output `Valkey server v=9.1.2 sha=00000000:1 malloc=libc bits=64 build=991715b0e5b85611\n`, hasil `9.1.2`); golden di `crates/core/tests/golden/provider/valkey/` (`init.json`, `run.json`, `run-aof.json`, `remedies.json`, dan `render-<config>.txt` untuk kelima config) lewat `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test provider_packages`; fixture `protocols/fixtures/instance/create-form-valkey.json` dan entri nya di `protocols/fixtures/instance/index.json` lewat test `create_form_fixtures`; `cargo xtask fuzz seed`. Memenuhi **AC-1**, **AC-7** (bagian teks), **AC-12** (bagian plan), **AC-18** (bagian loader).
4. Spike dari paket: hapus struct `Valkey`, cabang `"valkey"` di `recipe()`, dan penanda `// provisional: F40` dari `xtask/src/runtime/spike/providers.rs`; sesuaikan test di `xtask/src/runtime/spike/package_recipe.rs` yang mendaftar `valkey` sebagai provider tanpa paket. Memenuhi **AC-16** (bagian spike, tanpa run native).

**Milestone 2: Valkey 9.1.2 asli lewat daemon**

5. Harness dan test native inti di `apps/daemon/tests/valkey.rs` dan `apps/daemon/tests/valkey/` (`harness.rs` mengikuti `apps/daemon/tests/nats/harness.rs` dan `conformance/site.rs`: `RuntimeCache`, `install_cached_artifact`, `BLOCKED` tanpa cache; lalu `auth.rs`, `readiness.rs`, `owned.rs`, `stop.rs`, `tools.rs`, `lifecycle.rs`, `format.rs`). Memenuhi **AC-2** (round trip), **AC-4**, **AC-5**, **AC-10**, **AC-11**, **AC-12**, **AC-13**.
6. Test durability di `apps/daemon/tests/valkey/persistence.rs`: tiga mode, stop normal, crash proses, dan mode yang tidak bisa diganti. Memenuhi **AC-6**, **AC-7**, **AC-8**, **AC-9**.
7. Test install dan remedy: di `apps/daemon/tests/native_runtime_install.rs` baris `blocked_line_test!(valkey_9_1_installs_from_the_cache, "Valkey 9.1.2")` diganti `cache_line_test!(valkey_9_1_installs_from_the_cache, VALKEY, ...)` dengan konstanta `Line` `VALKEY` baru di modul `apps/daemon/tests/native_runtime_install/` (macro `blocked_line_test!` dan `NO_PACKAGE` tetap untuk MySQL dan ClickHouse sampai fitur 42 dan 43); unit test pattern; dan `apps/daemon/tests/valkey/doctor.rs` untuk kedua remedy serta connection dan Open In. Memenuhi **AC-16** (bagian version check), **AC-18**.

**Milestone 3: Konformitas, evidence, dan catalog**

8. Test native `valkey_9_1_passes_conformance` di `apps/daemon/tests/provider_conformance.rs`, satu baris Valkey di test `every_official_package_has_the_step_and_health_limits_its_manifest_gives` di file yang sama (init 120, readiness 120, stop 60, reload 30 detik; health 43 000 ms), dan `cargo xtask provider test providers/valkey` lulus untuk tiga skenario. Memenuhi **AC-14**.
9. Guard evidence `native` di setiap test native dan `apps/daemon/tests/valkey/compat.rs` yang menulis `compatibility.json`; lalu `cargo xtask runtime compat-record` dan `cargo xtask codegen` (compatibility record di `versions.json`). Run spike native `valkey_9_1_runs_from_managed_artifact` dari cache. Memenuhi **AC-15**, **AC-16** (run native).
10. Catalog dan corpus, di commit yang menutup bukti lifecycle ujung ke ujung: entri Valkey di `providers/catalog.toml` pindah ke `[[provider]]` (mark `Vk`, tile `red`), `revision` catalog menjadi nomor bebas berikutnya yang ditetapkan lead saat build dan `published_at` naik, `cargo xtask catalog check --update` menulis `providers/catalog-revisions.json`, lalu `cargo xtask provider freeze providers/valkey` menulis `crates/core/tests/fixtures/compat/valkey/1.0.0/`. Memenuhi **AC-17**.

**Milestone 4: Handover**

11. Test `valkey_9_1_survives_planned_restart_and_sigkill` di `apps/daemon/tests/native_launchd.rs`, dan test bahwa `daemon.restart_preview` melaporkan `handover_unproven` untuk Valkey. Memenuhi **AC-19** (bagian test).
12. Sesudah kamu menjalankan test langkah 11 dan record nya `PASS`: entri `official/valkey` line `1` dengan evidence `spec-0040-ac-19` di `crates/core/src/supervisor/handover.rs`. Memenuhi **AC-19** (bagian entri).

**Prasyarat**: semua langkah butuh fase 06 selesai (spec 0036, 0037, 0038 di `main`; sudah). Langkah 3 butuh langkah 1 dan 2. Langkah 5 sampai 9 butuh artefak di cache runtime xtask. Langkah 10 butuh langkah 5 sampai 9 lulus. Langkah 12 butuh langkah 11 dijalankan kamu.

**File bersama** (hanya satu sesi pada satu waktu; brief build menyebutnya di ownership):

| File | Langkah | Dipakai juga oleh |
| --- | --- | --- |
| `crates/core/src/provider/template.rs`, `plan.rs`, `validate.rs`, `fuzzing.rs` | 1, 2 | fitur 42 (`mysqlCnf`), fitur 43 (`xml`, `sha256Hex`), mungkin fitur 39 dan 41 |
| `crates/core/src/secret/delivery/mod.rs` | 1 | fitur 41 (bcrypt), fitur 43 |
| `crates/core/tests/provider_rules.rs`, `crates/core/tests/provider_packages.rs` | 1, 3 | setiap fitur provider |
| `docs/providers/manifest-reference.md`, `docs/providers/authoring.md` | 1, 2 | setiap fitur yang menambah format atau probe |
| `protocols/fixtures/instance/index.json`, `fuzz/seeds/` | 3 | fitur 42, 43 (paket baru) |
| `xtask/src/runtime/spike/providers.rs`, `package_recipe.rs` | 4 | fitur 42, 43 |
| `apps/daemon/tests/native_runtime_install.rs` (blok macro) dan modul `native_runtime_install/` | 7 | fitur 42, 43 |
| setiap `providers/*/versions.json` (ditulis ulang `cargo xtask codegen`) dan `evidence/compatibility/` | 3, 9 | setiap fitur yang menjalankan codegen atau `compat-record` |
| `apps/daemon/tests/provider_conformance.rs` | 8 | fitur 39 sampai 43 |
| `providers/catalog.toml`, `providers/catalog-revisions.json` | 10 | fitur 41, 42, 43 |
| `apps/daemon/tests/native_launchd.rs`, `crates/core/src/supervisor/handover.rs` | 11, 12 | fitur 41, 42, 43 |

Tidak disentuh fitur ini: `runtimes/inventory.toml` (artefak sudah ada), `xtask/src/check_deps/source_scan.rs` (`valkey` sudah di `PROVIDER_WORDS`), dan `crates/protocol/`.

**Sesi paralel**: di dalam fitur ini, langkah 1 dan 2 menyentuh `template.rs` yang sama, jadi satu sesi berurutan. Langkah 3 menunggu keduanya. Langkah 4 (xtask) bisa berjalan di sesi lain sesudah langkah 3, karena hanya menyentuh `xtask/`. Langkah 5, 6, dan 7 menulis file test yang berbeda di `apps/daemon/tests/valkey/`, tetapi berbagi `harness.rs`, jadi aman diparalelkan hanya sesudah `harness.rs` langkah 5 masuk; satu sesi lebih sederhana. Antar fitur, hampir semua langkah milestone 1 dan milestone 3 menyentuh file bersama di tabel di atas, jadi fitur 40 tidak bisa dibangun bebas berdampingan dengan fitur 42 dan 43: lead mengurutkan langkah 1 dan 2 terhadap langkah format mereka, menjadwalkan langkah 2 (filter `sha256Hex`, dibangun fitur 40) sebelum langkah fitur 43 yang memakainya, dan mengurutkan langkah 8 dan 10 terhadap test konformitas dan revision catalog provider lain. Yang benar benar bebas paralel dengan fitur lain hanyalah isi `providers/valkey/` dan `apps/daemon/tests/valkey/`.

**Langkah manual** (bukan untuk sesi build; masuk `verify.md` bagian `### UI / manual`):

- Jalankan `cargo test -p stacklyd --test native_launchd -- --ignored valkey` di sesi GUI kamu (langkah 11), lalu setujui entri `HANDOVER_PROVEN` (langkah 12).
- Buka form create Valkey di app dan pastikan teks `Persistence` tampil di bawah kontrolnya; lalu buat instance `volatile` dan pastikan halaman Configuration menampilkan field itu terkunci dengan nilai `volatile` dan file `persistence-volatile.conf`.
- Dengan TablePlus terpasang: pilih `Open in TablePlus` dan pastikan koneksi Redis terbuka ke port instance dan password dari clipboard diterima. Kalau TablePlus menolak URL tanpa password, entri itu dihapus dari paket lewat revision baru.
- Pilih `Open valkey-cli in Terminal`, tempel password, dan pastikan prompt terbuka.
- Fuzz 10 menit target `provider_template` sesudah langkah 1 dan 2.

## Consequences

**Positive**:

- Valkey lokal langsung aman: selalu ber password, hanya loopback, dan password tidak pernah ada di disk sebagai teks atau di argv.
- Ready berarti data sudah dimuat dan server menjawab perintah, dan pesan Valkey (`LOADING`, `WRONGPASS`) sampai ke pengguna saat tidak ready.
- Durability dinyatakan per mode dan dibuktikan per kejadian. Mode `volatile` menyebut dirinya apa adanya di form, di halaman Configuration, dan di nama nilainya.
- Mode persistence tidak bisa bergeser diam diam: tidak lewat edit config, tidak lewat `CONFIG REWRITE`, dan tidak lewat restart.
- Filter `sha256Hex` membuat mekanisme `hash` spec 0011 nyata untuk provider mana pun, dan dipakai dua provider sejak awal.
- Tanpa kontrak wire, migrasi, atau kode Swift baru: form, connection, Open In, doctor, dan harness bekerja dari paket.

**Negative / tradeoffs**:

- Dua perubahan core untuk satu provider (RISK-07): format `valkeyConf` dan filter `sha256Hex`. Yang kedua dibagi dengan ClickHouse, sehingga urutan build fitur 40 dan 43 saling bergantung dan butuh keputusan lead.
- Mengganti mode persistence berarti membuat instance baru dan memindahkan data sendiri. Ini lebih kaku dari Valkey biasa, tetapi uji di binary menunjukkan ganti mode lewat restart menghilangkan data (rationale *Fakta runtime*).
- Peringatan mode `volatile` hanya teks `description` yang selalu tampil di form create, bukan banner yang muncul saat nilai itu dipilih. Halaman Configuration hari ini tidak menampilkan `description` field, jadi sesudah create yang terlihat hanyalah nilai `volatile` dan komentar file mode nya; daftar instance tidak menampilkannya.
- Ketiga file mode selalu dirender, jadi halaman Configuration menampilkan tiga file `persistence-*.conf` walau hanya satu yang dipakai; komentar tiap file menyebut kapan ia dipakai.
- Setiap instance menjalankan satu proses `valkey-cli` pendek setiap 10 detik selama hidup.
- User `stackly-readiness` terlihat di `ACL LIST` dan menjadi kontrak: menghapusnya membuat instance `not_ready` sampai restart.
- Tidak ada reload: setiap perubahan setting butuh restart, dan perubahan lewat `CONFIG SET` hilang saat restart.
- User ACL buatan pengguna tidak bertahan melewati restart, karena tidak ada file ACL.
- Selama `HANDOVER_PROVEN` belum memuat Valkey, restart daemon terencana menghentikan instance Valkey, dan instance `volatile` kehilangan datanya.
- Hash tanpa salt bisa ditebak offline untuk password lemah yang diketik pengguna.
- Password yang sama persis dengan literal argv ditolak saat start.

**Neutral**:

- `providers/valkey` menjadi paket resmi pertama yang memakai config `sensitive`, filter hash, secret ber kelas `restart`, dan pemilihan file include lewat nilai setting.
- Spike runtime Valkey kini berjalan dengan auth dan berhenti lewat `SIGTERM`, bukan `shutdown nosave`.
- Kalimat spec 0011 tentang config plaintext Valkey tidak lagi benar dan perlu dikoreksi pemiliknya.
- `valkey-check-aof` dan `valkey-check-rdb` ada di artefak tetapi tidak di daftar `executables` inventory, jadi remedy hanya menyebut namanya.

## Follow-up

- [ ] Lead: terapkan *Usulan update spec lain* (spec 0011, 0003, 0030, 0036, 0034, 0070, 0018). Baris *Perluasan additive* spec 0009 dikerjakan sesi `arc-f9-update` (2026-11-12).
- [x] Lead: putuskan fitur mana (40 atau 43) yang membangun filter `sha256Hex`. Diputuskan 2026-11-12: fitur 40 membangun (langkah 2), fitur 43 hanya memakai; kontraknya AC-3 spec ini dan AC-2 spec 0043, termasuk bentuk segmen `secretHash` dan `template_syntax`.
- [ ] Kamu: jalankan test native launchd Valkey dan setujui entri `HANDOVER_PROVEN` (AC-19).
- [ ] Kamu: langkah manual GUI di *Langkah manual* (form, Configuration, TablePlus, Terminal).
- [ ] Lead atau `/scope`: halaman Configuration menampilkan `description` field seperti form create (perubahan Swift generik, bukan bagian fitur ini), supaya peringatan mode `volatile` juga tampil sesudah create.
- [ ] Fase 09 (backup dan restore): backup instance `volatile` harus mengatakan bahwa tidak ada data; backup `aof` mengambil seluruh `appendonlydir/`, bukan satu nama file (PRD 22.4); restore memeriksa mode persistence instance tujuan.
- [ ] Fitur 57 (adoption): data Valkey atau Redis lama, termasuk pemeriksaan versi RDB, karena paket ini tidak punya `dataMarker`.
- [ ] Saat line Valkey kedua masuk inventory: putuskan `lineCompatibility` atau penanda versi, karena RDB hanya bisa dibaca versi yang sama atau lebih baru.
- [ ] Artefak Valkey berikutnya: tambah `bin/valkey-check-aof` dan `bin/valkey-check-rdb` ke `executables` inventory, supaya remedy `aof_damaged` bisa membawa perintah siap salin lewat alias `additionalExecutables`.
- [ ] Settings tambahan kalau pengguna membutuhkannya, masing masing additive lewat update spec ini: jumlah database, save point, `appendfsync`, dan user ACL tambahan.
- [ ] Open In Redis Insight, sesudah bundle ID dan deep link nya dibuktikan di Mac yang memasangnya (rationale Keputusan 10).
- [ ] Kalau pengguna butuh ganti mode persistence: alur terpandu sendiri (`CONFIG SET` saat jalan, tunggu tulis ulang, lalu ganti setting), lewat spec baru.
- [ ] `/sync` setelah build: catat di root `AGENTS.md` paket Valkey, user `stackly-readiness`, format `valkeyConf`, filter `sha256Hex`, dan perintah test native Valkey.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, fakta runtime dari uji lokal pada binary cache, dan catatan cross check ada di [rationale.md](rationale.md).
