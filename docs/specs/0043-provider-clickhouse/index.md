# 0043. Provider ClickHouse

**Date**: 2026-11-12
**Status**: In Progress

## Summary

Spec ini membuat ClickHouse menjadi provider resmi lewat paket `providers/clickhouse`: satu server single node per instance, dengan HTTP dan native TCP di loopback, user `default` ber password, dan semua path di dalam folder instance. Cluster, replikasi, dan Keeper tidak pernah dinyalakan, dan tidak ada listener selain dua port yang tercatat di registry. Core mendapat dua perubahan kecil yang ditinjau: format config `xml` (milik fitur ini) dan filter template `sha256Hex` (dipakai bersama fitur 40, Valkey), supaya password hanya tersimpan sebagai hash. Readiness memakai probe `exec` yang sudah ada: `clickhouse client` menjalankan satu query sebagai user khusus tanpa hak, dan query itu gagal kalau server yang menjawab bukan server instance ini. Reload memakai perintah `SYSTEM RELOAD CONFIG` lewat user khusus kedua, karena `SIGHUP` di ClickHouse tercatat sebagai perintah membuka ulang log dan tidak terbukti memuat ulang config. Semua keputusan di sini sudah dicoba pada binary 26.8.30.8 dari cache runtime (catatan di [rationale.md](rationale.md)).

## Requirements

**User stories**:

- Sebagai developer, saya ingin membuat server ClickHouse lokal lewat alur yang sama dengan PostgreSQL (catalog, install, create, start, connection, stop), supaya saya tidak perlu menulis `config.xml`, memilih port, atau memasang apa pun lewat Homebrew.
- Sebagai developer, saya ingin dua instance ClickHouse berjalan berdampingan tanpa saling menyentuh data, port, atau user, supaya dua proyek dengan nama tabel yang sama tidak tercampur.
- Sebagai developer, saya ingin yakin bahwa instance hanya membuka port yang Stackly tampilkan, supaya tidak ada listener replikasi atau protokol lain yang terbuka diam diam.
- Sebagai maintainer Stackly, saya ingin jumlah perubahan core untuk ClickHouse tercatat, supaya RISK-07 (schema provider terlalu sempit) terukur.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-037):

- **AC-1** (format `xml`, perubahan core milik fitur ini): `configFiles.*.format` menerima `xml`. Setiap `{{ }}` teks di escape sebagai isi XML: `&` menjadi `&amp;`, `<` menjadi `&lt;`, `>` menjadi `&gt;`, `"` menjadi `&quot;`, `'` menjadi `&apos;`, tanpa kutip pembungkus, jadi tag boleh menempel pada teks literal (`<path>{{ instance.data }}/</path>`). Teks yang memuat karakter kontrol (U+0000 sampai U+001F, U+007F sampai U+009F), U+FFFE, atau U+FFFF ditolak dengan `unrepresentable_value` yang menyebut field atau token, tidak pernah nilainya. Integer desimal, boolean `true` atau `false`. Tag blok dan tag output diizinkan. Output tidak diparse ulang. Format yang tidak dikenal tetap ditolak `unknown_template_format`, dan pesannya kini menyebut `xml`. Setiap paket yang valid sebelumnya tetap valid dengan digest dan plan yang sama (corpus beku spec 0036 AC-6 tidak berubah).
- **AC-2** (filter `sha256Hex`, perubahan core bersama spec 0040): template menerima `{{ secret.<field> | sha256Hex }}`. Spasi di sekitar `|` bebas. Filter hanya sah sesudah path `secret.*`: filter di path lain, nama filter lain, atau lebih dari satu filter ditolak saat validasi dengan `template_type` beserta baris dan kolom. Di template milik config yang tidak `sensitive`, token `secret.*` dengan atau tanpa filter tetap ditolak dengan kode yang sudah ada, `secret_not_allowed_here`. Hasilnya 64 karakter hex huruf kecil dari SHA-256 atas byte secret persis (`secret::delivery::hash(HashAlgo::Sha256Hex, ..)`), ditulis mentah tanpa escaper format. Plan membawa segmen `Segment::SecretHash { secret, algo }` yang baru dihitung `RenderedContent::materialize`; render contoh saat validasi memakai hash dari teks `x`; `Debug` plan tidak pernah menampilkan hash atau nilai. Bentuk JSON segmen itu di plan dan golden mengikuti `Segment` yang ada: `{ "type": "secretHash", "content": { "secret": "<secret:<field>>", "algo": "sha256_hex" } }`, dengan `HashAlgo` yang `Serialize` nya menulis `as_code()`. Nama filter kosong (`{{ secret.x | }}`) atau nama filter dengan karakter di luar `[A-Za-z0-9]` ditolak `template_syntax`, seperti kesalahan parse lain. Tidak ada kontrak wire baru.
- **AC-3** (paket): `cargo xtask provider check providers/clickhouse` mencetak `ok official/clickhouse 1.0.0 <sha256>` untuk paket yang isinya persis *Paket ClickHouse*, tanpa `init_port_issues` dan tanpa finding `warning`. `versions.json` nya ditulis `cargo xtask codegen` dari `runtimes/inventory.toml` (bukan tulisan tangan). `init_plan` dengan kedua endpoint tanpa port berhasil dan hanya berisi `EnsureDirectories`; `run_plan` menulis `config.xml`, `client.xml`, dan `users.xml`. Golden plan dan render (termasuk golden connection dan open action) ada di `crates/core/tests/golden/provider/clickhouse/`.
- **AC-4** (listener dan path, REQ-037, AC-037 bagian listener): pada instance `running`, `lsof` atas PID proses utama menunjukkan tepat dua socket TCP `LISTEN`, keduanya di `127.0.0.1`, dengan port yang sama dengan endpoint `http` dan `native` di registry, dan tidak ada socket UDP. `config.xml` hasil render tidak memuat satu pun key di *Key yang dilarang*. Query `SELECT name, value FROM system.server_settings WHERE value LIKE '/%'` hanya mengembalikan path di bawah folder instance (`DirectoryLayout::instance_paths` dan folder run instance itu) dan empat path Keeper di *Path*. Setelah start, stop, dan start lagi, daftar file di dalam `TestRoot` di luar folder `data`, `config`, `temp`, dan `run` instance itu serta log store Stackly tidak bertambah, dan tidak ada `.clickhouse*` di folder `HOME` proses (folder temp instance). Semua ini berlaku untuk config yang Stackly render; batasnya ada di *Key yang dilarang*.
- **AC-5** (kontrak proses): proses yang di spawn adalah server itu sendiri (watchdog ClickHouse mati lewat `CLICKHOUSE_WATCHDOG_ENABLE=0`): tidak ada proses anak, fd 3 tetap lock instance, program tidak berganti, dan PID sama sampai stop. Langkah `process` harness konformitas lulus.
- **AC-6** (auth): user `default` hanya bisa masuk dengan password dari Keychain, lewat HTTP maupun native TCP. Password yang salah atau kosong ditolak. Pembatasan `<networks>` ke `127.0.0.1` dan `::1` dibuktikan golden render `users.xml`, karena server memang hanya mendengarkan di loopback. Nilai password tidak pernah ada di file mana pun di bawah folder instance, di log, atau di argv; yang tersimpan hanya hash SHA-256 di `users.xml` (folder `secrets/` runtime) dan salinan yang ClickHouse tulis sendiri di `data/preprocessed_configs/`. User `stackly_readiness` tanpa password hanya punya `GRANT SELECT ON system.one`, profil `readonly`, dan tidak bisa membaca tabel pengguna, `system.users`, file, atau URL. User `stackly_reload` tanpa password hanya punya `GRANT SYSTEM RELOAD CONFIG` dan `GRANT SYSTEM RELOAD USERS`, dan tidak bisa membaca tabel pengguna atau menjalankan `SYSTEM` lain.
- **AC-7** (readiness dan health, REQ-037): instance menjadi `ready` hanya setelah `clickhouse client` lewat endpoint `native` berhasil menjalankan query identitas (*Readiness*) sebagai `stackly_readiness`. Probe readiness instance itu, dijalankan terhadap server ClickHouse lain yang punya user `stackly_readiness` tetapi ID instance berbeda, gagal (exit bukan 0), jadi server yang salah tidak pernah dianggap `ready`. Health memakai `GET /ping` di endpoint `http`.
- **AC-8** (isolasi, AC-037): dua instance dari runtime yang sama, masing masing dengan tabel `default.events` berisi baris berbeda, di restart satu per satu; sesudahnya tiap instance hanya membaca barisnya sendiri, port keduanya berbeda, dan AC-4 berlaku untuk keduanya pada saat yang sama. Menghentikan instance pertama tidak mengubah instance kedua.
- **AC-9** (persistence dan stop): stop mengirim `SIGTERM`, proses keluar dengan kode 0 dalam `stopSeconds` tanpa force stop, kedua port tertutup, dan tidak ada proses tersisa. Start berikutnya di data yang sama berhasil dan data yang ditulis sebelum stop masih ada.
- **AC-10** (reload): reload menjalankan `SYSTEM RELOAD CONFIG` sebagai `stackly_reload` dan baru berhasil setelah perintah itu exit 0 dan readiness `ready`, dengan PID yang sama. Mengubah `logLevel` dari `information` ke `debug` lalu reload membuat baris `<Debug>` muncul di `instance.logs_read` untuk query sesudahnya, padahal sebelumnya tidak ada. Mengganti nilai secret `defaultPassword` lewat `secret.set` lalu reload membuat password lama ditolak dan password baru diterima begitu operasi reload selesai, tanpa menunggu.
- **AC-11** (connection dan Open In): `instance.get_detail` membawa dua connection (`http` sebagai utama, lalu `native`) dan dua open action (`play`, `client`) seperti *Paket ClickHouse*; bentuk masked tidak memuat password, dan open action tidak pernah memuat nilai secret.
- **AC-12** (kegagalan start): port yang sudah dipakai proses lain membuat start ditolak di tahap `endpoints` sebelum spawn (`supervisor.endpoint_in_use`, spec 0033), tanpa perbaikan otomatis, dan proses asing itu tidak pernah diberi sinyal. Folder data yang sedang dipakai server ClickHouse lain di luar Stackly (server asing itu memakai port lain) membuat start berakhir `failed` dengan `lastFailure.code` `process_exited` dan `exitCode` 76; Stackly tidak menghentikan proses itu dan tidak menghapus file `status`, dan baris `Cannot lock file` terbaca di log instance.
- **AC-13** (AC-074, tanpa label native palsu): manifest hanya menyatakan `{ os: macos, arch: aarch64 }`, `versions.json` hanya memuat artefak macOS, dan entri catalog membawa catatan platform `windows` berstatus `unsupported` yang menyebut WSL. Untuk host Windows, entri ClickHouse tidak pernah `installable` dan `native_support` bernilai false, juga ketika catalog uji diberi artefak `linux`. Tidak ada kode, manifest, atau dokumen di fitur ini yang memasang atau memanggil WSL atau container.
- **AC-14** (konformitas dan install native): `clickhouse_26_8_passes_conformance` lulus dengan report `pass` untuk setiap skenario paket; `clickhouse_installs_from_the_cache` memasang 26.8.30.8 dari cache tanpa jaringan sampai `installed`. Artefak yang tidak ada di cache xtask membuat keduanya `BLOCKED` dengan reason dan owner, tidak pernah mengunduh.
- **AC-15** (spike dari paket, Follow-up spec 0003): struct recipe `ClickHouse` dan penanda `// provisional: F43` hilang dari `xtask/src/runtime/spike/providers.rs`; `recipe("clickhouse")` mengembalikan `PackageRecipe` dari `providers/clickhouse`; test `xtask` yang menyebut recipe tulisan tangan ClickHouse disesuaikan. Bentuk hasil spike tidak berubah.
- **AC-16** (catalog, fixture, corpus): di commit yang sama dengan bukti native AC-14 dan AC-8, `official/clickhouse` pindah dari `[[planned]]` ke `[[provider]]` di `providers/catalog.toml` dengan revision catalog baru dan ledger dari `cargo xtask catalog check --update`; `protocols/fixtures/instance/create-form-clickhouse.json` dan entrinya di `index.json` ada; `cargo xtask provider freeze providers/clickhouse` menulis `crates/core/tests/fixtures/compat/clickhouse/`; record `compatibility.json` (BR-025) untuk 26.8.30.8 masuk `evidence/compatibility/` lewat `cargo xtask runtime compat-record`, dijalankan sesi build dari cache tanpa jaringan. Kalau artefak tidak ada di cache, record itu `BLOCKED` dan entri catalog tetap `[[planned]]`.
- **AC-17** (ukuran RISK-07): `verify.md` mencatat, dari `git diff --numstat` terhadap commit dasar build, bahwa perubahan non test di `crates/` dan `apps/` untuk fitur ini hanya dua: format `xml` (AC-1) dan filter `sha256Hex` (AC-2, dihitung sekali untuk fitur 40 dan 43). Tidak ada jenis probe, probe native, primitive, token, DTO, method, event, migrasi, atau kode Swift baru.

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md): config XML lewat format `xml` baru, password lewat hash `sha256Hex`, readiness lewat probe `exec` dengan query identitas, reload lewat `SYSTEM RELOAD CONFIG`, log lewat stderr, dan handover tetap `unproven`.

ClickHouse dibangun sebagai paket `providers/clickhouse` di atas engine yang ada, dengan dua perubahan core additive yang ditinjau (format `xml`, filter `sha256Hex`) dan tanpa probe, primitive, protokol, atau migrasi baru.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

## Feature design

### Trace PRD

| ID | Dipenuhi oleh |
| --- | --- |
| REQ-037 | AC-3 sampai AC-12 |
| AC-037 | AC-4, AC-8 |
| AC-074 | AC-13 (bagian ClickHouse; aturan umum `native_support` milik spec 0004 AC-14) |
| PRD 22.3 | *Paket ClickHouse*, *Path*, *Key yang dilarang*, *Readiness* |
| PRD 32.2 baris ClickHouse | AC-4, AC-8, AC-14 (satu versi konkret, dua instance, HTTP dan native TCP, audit listener) |

### Batas fitur

Di dalam: paket, dua perubahan core, test native dari cache xtask, entri catalog, fixture form, corpus beku, dan penghapusan recipe spike.

Di luar, sengaja: backup dan restore (fase 09; `backup: [cold]` hanya menyatakan bahwa salinan folder `data` saat instance berhenti adalah cadangan yang sah, pembuktiannya di fase 09), adoption data ClickHouse yang sudah ada (fitur 57), environment (fase 08), TLS, cluster, replikasi, Keeper, dictionary dari library, tabel log sistem (`query_log` dan kawan kawan), batas memori sebagai setting, dan `HANDOVER_PROVEN`. Lini kedua ClickHouse juga di luar: inventaris hanya punya 26.8.

### Paket ClickHouse

Dibuat dengan `cargo xtask provider new clickhouse`, lalu diubah mengikuti urutan *Dari contoh ke paket resmi* spec 0036. Isi akhirnya:

```text
providers/clickhouse/
├── provider.yaml
├── versions.json                  # dari cargo xtask codegen
├── schemas/settings.schema.json
├── templates/config.xml.tmpl
├── templates/client.xml.tmpl
├── templates/users.xml.tmpl
├── tests/scenarios.yaml
├── README.md
└── LICENSE-NOTICES.md
```

`provider.yaml`:

```yaml
apiVersion: stackly/v1alpha1
kind: Provider
metadata:
  id: official/clickhouse
  software: clickhouse
  displayName: ClickHouse
  revision: "1.0.0"
  category: analytics
  description: ClickHouse columnar analytics database. One single node server per instance, with HTTP and native TCP on loopback and a password protected default user.
compatibility:
  platforms:
    - { os: macos, arch: aarch64 }
runtime:
  executable: clickhouse
  executionMode: foreground
  versionCheck:
    args: ["server", "--version"]
    pattern: '^ClickHouse server version ([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+) \(official build\)\.$'
    timeoutSeconds: 30
capabilities:
  reload: true
  multiInstance: true
  persistence: true
  backup: [cold]
resources:
  endpoints:
    http: { transport: tcp, purpose: client, defaultHost: "127.0.0.1", defaultPort: 8123 }
    native: { transport: tcp, purpose: client, defaultHost: "127.0.0.1", defaultPort: 9000 }
configFiles:
  serverConfig:
    template: templates/config.xml.tmpl
    format: xml
    destination: "${instance.config}/config.xml"
  clientConfig:
    template: templates/client.xml.tmpl
    format: plain
    destination: "${instance.config}/client.xml"
  usersConfig:
    template: templates/users.xml.tmpl
    format: xml
    sensitive: true
    destination: "${instance.secrets}/users.xml"
lifecycle:
  initialize:
    - { primitive: createManagedDirectories }
  start:
    executable: "${runtime.executable}"
    args:
      - "server"
      - "--config-file=${instance.config}/config.xml"
      - "--"
      - "--user_directories.users_xml.path=${instance.secrets}/users.xml"
    cwd: "${instance.data}"
    env: { CLICKHOUSE_WATCHDOG_ENABLE: "0" }
  stop: { strategy: gracefulProcess, signal: term }
  reload:
    render: [serverConfig, usersConfig]
    action: run
    command:
      executable: "${runtime.executable}"
      args:
        - "client"
        - "--config-file"
        - "${instance.config}/client.xml"
        - "--host"
        - "${endpoint.native.host}"
        - "--port"
        - "${endpoint.native.port}"
        - "--user"
        - "stackly_reload"
        - "--query"
        - "SYSTEM RELOAD CONFIG"
      cwd: "${instance.temp}"
    successExitCodes: [0]
    verify: readiness
readiness:
  probe:
    type: exec
    command:
      executable: "${runtime.executable}"
      args:
        - "client"
        - "--config-file"
        - "${instance.config}/client.xml"
        - "--host"
        - "${endpoint.native.host}"
        - "--port"
        - "${endpoint.native.port}"
        - "--user"
        - "stackly_readiness"
        - "--query"
        - "SELECT throwIf(getMacro('stackly_instance_id') != '${instance.id}')"
      cwd: "${instance.temp}"
    successExitCodes: [0]
  intervalMilliseconds: 500
  attemptTimeoutMilliseconds: 5000
health:
  probe: { type: http, endpoint: http, path: /ping }
  intervalSeconds: 10
  attemptTimeoutMilliseconds: 3000
  failureThreshold: 3
timeouts: { readinessSeconds: 120, stopSeconds: 60, reloadSeconds: 30 }
backup:
  cold: { paths: [data] }
permissions:
  managedFilesystem: [data, config, temp, run]
  executeRuntimeBinary: true
connection:
  - name: http
    title: HTTP URL
    template: "http://default:${secret.defaultPassword}@${endpoint.http.address}/"
  - name: native
    title: Native TCP
    template: "clickhouse://default:${secret.defaultPassword}@${endpoint.native.address}/default"
openWith:
  - name: play
    title: Play UI
    kind: browser
    browser:
      url: "http://${endpoint.http.address}/play"
  - name: client
    title: clickhouse client
    kind: terminal
    terminal:
      executable: "${runtime.executable}"
      args: ["client", "--host", "127.0.0.1", "--port", "${endpoint.native.port}", "--user", "default", "--ask-password"]
    copySecret: defaultPassword
```

Catatan untuk hal yang tidak terbaca dari YAML:

- Tidak ada `renderConfig` di `initialize`: `config.xml` memuat port, yang baru dipilih saat start, dan render sebelum spawn (`RunPlan.render`) menulis kedua file sebelum setiap spawn (aturan `init_port_issues` spec 0009). ClickHouse membuat isi folder data sendiri pada start pertama, jadi tidak ada langkah `runExecutable`.
- Tidak ada `dataMarker`: folder data ClickHouse tidak punya file versi yang stabil saat server berhenti (`status` dihapus saat stop bersih). Pemeriksaan kompatibilitas data (`check_data_compatibility`) lalu memakai line yang tercatat di registry, dan itu cukup selama hanya ada satu line.
- Tidak ada `logs.files`: logger hanya menulis ke stderr (`<console>1</console>`), yang ditangkap supervisor dan masuk log store spec 0027 setelah redaksi. ClickHouse tidak menulis file log sendiri.
- `--host` di open action `client` adalah literal `127.0.0.1`, karena posisi `openWith` tidak menerima `endpoint.<n>.host` (spec 0070) dan endpoint selalu memakai `defaultHost` manifest (spec 0020 AC-4).
- Argumen sesudah `--` adalah cara resmi ClickHouse menimpa satu key config dari command line. Path `users.xml` dikirim lewat sana supaya `config.xml` di folder config tidak memuat path folder run. Nilainya path, bukan secret, jadi aturan "secret tidak pernah di argv" tidak tersentuh.
- Flag `--ask-password` di open action `client` belum dicoba (butuh terminal interaktif); langkah manual 3 membuktikannya, dan kalau flag itu ditolak perbaikannya lewat update spec ini.
- `client.xml` adalah file statis berisi `<config>` kosong. Probe dan perintah reload menunjuknya lewat `--config-file`, supaya `clickhouse client` tidak membaca `/etc/clickhouse-client/config.xml` atau `~/.clickhouse-client/config.xml`. Di percobaan, file client yang memuat `<password>` membuat login `stackly_readiness` gagal walau `--user` diberi di argv.
- Blok `<user_directories>` harus ada di `config.xml` (di sini dengan `local_directory`). Tanpa blok itu di file, key `user_directories.users_xml.path` dari argv tidak dipakai dan server gagal start dengan `Settings profile default not found` (dicoba).
- Reload memakai `action: run`, bukan `signal`. Di percobaan, server mencatat `SIGHUP` sebagai perintah membuka ulang log, dan tidak ada bukti ia memuat ulang `config.xml`; perubahan yang tadinya terlihat "sesudah `SIGHUP`" ternyata dikerjakan pengawas file. `SYSTEM RELOAD CONFIG` terbukti: exit 0, `users.xml` baru berlaku seketika, dan untuk config yang rusak exit bukan 0 sambil server tetap melayani config lama. Efeknya pada level log belum terukur di percobaan (pengukuran saya keliru), jadi bagian itu dibuktikan test native AC-10.
- Tidak ada blok `remedies`. Satu satunya kandidat (folder data terkunci server lain) tidak bisa dicocokkan: kutipan log kegagalan hanya memuat 10 baris terakhir, dan ClickHouse menulis baris `Cannot lock file` di awal, diikuti lebih dari 10 baris stack trace (dicoba: baris 2 dari 16).
- Satu satunya `env` adalah `CLICKHOUSE_WATCHDOG_ENABLE=0`. Tanpa itu `clickhouse server` di macOS menjadi proses `clickhouse-watchdog` yang menjalankan server sebagai proses anak, dan kedua listener dimiliki anak itu (terlihat di percobaan); itu melanggar kontrak proses spec 0036.

`schemas/settings.schema.json` (dua field, urutan ini):

| Field | Tipe | Aturan | `x-stackly` |
| --- | --- | --- | --- |
| `defaultPassword` | string | `title` "Default user password", `description` "Password of the default user. ClickHouse only ever stores its SHA-256 hash.", `minLength` 8, `maxLength` 128, `required` | `apply: reload`, `visibility: basic`, `secret: true`, `printableOnly: true` |
| `logLevel` | string | `title` "Log level", `description` "How much the server writes to the instance log.", `enum` `error`, `warning`, `information`, `debug`, `trace`; `default` `information` | `apply: reload`, `visibility: basic` |

`defaultPassword` berkelas `reload` karena `users.xml` dirender ulang dari Keychain pada setiap start dan reload, jadi nilai baru memang berlaku pada reload berikutnya (`EffectKind::NextReload` di `secret::rules`). Aturan "field `reload` hanya dirujuk template di `reload.render`" hanya memeriksa token `settings.*`, jadi pemakaian `secret.defaultPassword` di `connection` dan `copySecret` tidak melanggarnya.

`templates/config.xml.tmpl`:

```xml
<!-- Managed by Stackly. Edits to this file are lost on the next start. -->
<clickhouse>
    <logger>
        <level>{{ settings.logLevel }}</level>
        <console>1</console>
    </logger>
    <macros>
        <stackly_instance_id>{{ instance.id }}</stackly_instance_id>
    </macros>
    <disable_internal_dns_cache>1</disable_internal_dns_cache>
    <listen_host>{{ endpoint.http.host }}</listen_host>
    <http_port>{{ endpoint.http.port }}</http_port>
    <tcp_port>{{ endpoint.native.port }}</tcp_port>
    <path>{{ instance.data }}/</path>
    <tmp_path>{{ instance.temp }}/clickhouse/</tmp_path>
    <user_files_path>{{ instance.data }}/user_files/</user_files_path>
    <format_schema_path>{{ instance.data }}/format_schemas/</format_schema_path>
    <user_scripts_path>{{ instance.data }}/user_scripts/</user_scripts_path>
    <user_defined_path>{{ instance.data }}/user_defined/</user_defined_path>
    <dictionaries_lib_path>{{ instance.data }}/dictionaries_lib/</dictionaries_lib_path>
    <top_level_domains_path>{{ instance.data }}/top_level_domains/</top_level_domains_path>
    <google_protos_path>{{ instance.data }}/google_protos/</google_protos_path>
    <dynamic_user_defined_executable_functions_path>{{ instance.data }}/dynamic_user_defined_executable_functions/</dynamic_user_defined_executable_functions_path>
    <user_directories>
        <local_directory>
            <path>{{ instance.data }}/access/</path>
        </local_directory>
    </user_directories>
</clickhouse>
```

`templates/users.xml.tmpl`:

```xml
<!-- Managed by Stackly. Rendered again from the Keychain on every start and reload. -->
<clickhouse>
    <profiles>
        <default/>
        <stackly_readiness>
            <readonly>1</readonly>
            <max_execution_time>2</max_execution_time>
            <max_memory_usage>16777216</max_memory_usage>
            <max_threads>1</max_threads>
        </stackly_readiness>
        <stackly_reload>
            <max_execution_time>10</max_execution_time>
            <max_memory_usage>16777216</max_memory_usage>
            <max_threads>1</max_threads>
        </stackly_reload>
    </profiles>
    <quotas>
        <default/>
    </quotas>
    <users>
        <default>
            <password_sha256_hex>{{ secret.defaultPassword | sha256Hex }}</password_sha256_hex>
            <networks>
                <ip>127.0.0.1</ip>
                <ip>::1</ip>
            </networks>
            <profile>default</profile>
            <quota>default</quota>
            <access_management>1</access_management>
            <named_collection_control>1</named_collection_control>
            <show_named_collections>1</show_named_collections>
            <show_named_collections_secrets>1</show_named_collections_secrets>
        </default>
        <stackly_readiness>
            <password></password>
            <networks>
                <ip>127.0.0.1</ip>
                <ip>::1</ip>
            </networks>
            <profile>stackly_readiness</profile>
            <quota>default</quota>
            <grants>
                <query>GRANT SELECT ON system.one</query>
            </grants>
        </stackly_readiness>
        <stackly_reload>
            <password></password>
            <networks>
                <ip>127.0.0.1</ip>
                <ip>::1</ip>
            </networks>
            <profile>stackly_reload</profile>
            <quota>default</quota>
            <grants>
                <query>GRANT SYSTEM RELOAD CONFIG ON *.*</query>
                <query>GRANT SYSTEM RELOAD USERS ON *.*</query>
            </grants>
        </stackly_reload>
    </users>
</clickhouse>
```

`templates/client.xml.tmpl` (format `plain`, tanpa tag):

```xml
<config>
</config>
```

Blok `<grants>` wajib ada: user dari `users.xml` tanpa blok itu mendapat semua hak. `stackly_reload` tidak `readonly`, karena ClickHouse menolak setiap `SYSTEM` di mode `readonly` (dicoba: kode 164); haknya dibatasi oleh dua grant itu saja. `listen_host` memakai host endpoint `http`; kedua endpoint menyatakan `defaultHost` yang sama dan endpoint selalu memakai `defaultHost`, jadi satu nilai cukup.

`tests/scenarios.yaml`: satu skenario, karena ClickHouse single node tidak punya mode lain yang didukung fitur ini.

```yaml
schema: stackly.provider-scenarios/v1
scenarios:
  - name: default
```

`README.md` (bahasa Inggris) menjelaskan: dua endpoint, user `default` dan cara membuat user lain lewat SQL (tersimpan di `data/access/`), user `stackly_readiness` dan `stackly_reload` dan kenapa jangan diubah, bahwa file di `config.d/` tidak dikelola Stackly dan bisa membuka listener yang tidak tercatat, bahwa cluster, replikasi, Keeper, dan protokol lain mati, bahwa tabel log sistem tidak aktif, bahwa instance diam memakai sekitar 0,5 GB RAM, dan bahwa di Windows tidak ada build native (upstream memakai WSL, yang tidak pernah Stackly pasang). `LICENSE-NOTICES.md` memuat lisensi Apache 2.0 ClickHouse dengan URL lisensi dari inventaris.

### Path

| Yang ClickHouse tulis atau baca | Tempat |
| --- | --- |
| Data, metadata, `store/`, `access/` (user buatan SQL), `user_files/`, `format_schemas/`, `user_scripts/`, `user_defined/`, `preprocessed_configs/`, `status`, `uuid`, `flags/` | `${instance.data}` |
| File sementara query | `${instance.temp}/clickhouse/` |
| `config.xml`, `client.xml` | `${instance.config}` |
| `users.xml` | `${instance.secrets}` (folder run, 0600, dihapus setelah stop bersih, spec 0011) |
| Log server | stderr, lalu log store Stackly |
| `HOME` dan `TMPDIR` proses server dan probe | `${instance.temp}` (aturan plan yang ada), dan `--config-file` eksplisit, jadi `clickhouse client` tidak pernah membaca config client milik pengguna atau sistem |

Lima key lookup (`user_defined_path`, `dictionaries_lib_path`, `top_level_domains_path`, `google_protos_path`, `dynamic_user_defined_executable_functions_path`) ditulis eksplisit karena default nya menunjuk `/var/lib/clickhouse/` atau `/usr/share/clickhouse/`, yaitu di luar instance. Empat nilai yang tersisa di `system.server_settings` dan diawali `/` (`default_replica_path`, `series_keeper_path`, `distributed_ddl.path`, `distributed_ddl.replicas_path`) adalah path di dalam Keeper, bukan path file, dan tidak dipakai tanpa Keeper.

### Key yang dilarang

`config.xml.tmpl` tidak boleh memuat key ini, dan satu test core menjaganya dari teks template dan dari hasil render golden: `mysql_port`, `postgresql_port`, `grpc_port`, `interserver_http_port`, `interserver_https_port`, `https_port`, `tcp_port_secure`, `tcp_with_proxy_port`, `prometheus`, `keeper_server`, `zookeeper`, `remote_servers`, `distributed_ddl`, `listen_try`, `include_from`, dan `listen_host` kedua. Karena `--config-file` membuat ClickHouse tidak memakai config bawaannya, key yang tidak ditulis berarti listener nya tidak ada.

Batas yang dinyatakan: ClickHouse tetap menggabungkan file di `config.d/` dan `conf.d/` di samping `config.xml` (dan `users.d/` di samping `users.xml`), dan pengawas filenya menerapkan `config.xml` yang diubah saat server berjalan. Stackly tidak pernah membuat folder itu, dan drift spec 0029 hanya melihat file yang ia render. Jadi invariant "hanya dua listener" berlaku untuk config yang Stackly render; pengguna yang menaruh file sendiri di `config.d/` bisa membuka listener yang tidak tercatat. README paket menyebut ini, dan pemeriksaan doctor untuk folder itu ada di Follow-up.

### Readiness

Satu percobaan menjalankan `clickhouse client` dari runtime instance itu sendiri, lewat native TCP, sebagai `stackly_readiness`, dengan query:

```sql
SELECT throwIf(getMacro('stackly_instance_id') != '<instance id>')
```

- Exit 0 berarti listener native menerima koneksi, autentikasi dan pipeline query bekerja, dan server yang menjawab memuat `config.xml` instance ini (identitas).
- Server lain di port itu membuat `throwIf` gagal (exit bukan 0), jadi percobaan gagal. Port tertutup, user yang tidak ada, atau timeout juga gagal.
- Di percobaan, HTTP mulai mendengarkan sebelum native TCP dan keduanya sebelum baris `Ready for connections`, jadi native yang menjawab berarti HTTP juga sudah terbuka. Health rutin memakai `GET /ping` di HTTP, yang murah dan tidak men spawn proses.
- Probe tidak menerima secret, tidak menulis apa pun, berjalan dengan `HOME` di folder temp instance, dan memakai `client.xml` instance lewat `--config-file`.
- Selama start, query identitas lulus begitu server memuat config. Sesudah reload, `verify: readiness` hanya memastikan server masih menjawab; bukti bahwa config baru berlaku datang dari exit 0 `SYSTEM RELOAD CONFIG`.

Kenapa bukan probe `http` saja, atau probe native baru, ada di rationale (Keputusan 3).

### Kelakuan runtime yang sudah dicoba

Dicoba 2026-11-12 pada `clickhouse` 26.8.30.8 dari cache xtask, di folder sementara dengan nama ber spasi, `&`, dan huruf Unicode:

| Fakta | Hasil |
| --- | --- |
| `clickhouse server --version` | `ClickHouse server version 26.8.30.8 (official build).` |
| Start sampai `/ping` 200 | sekitar 0,5 detik; eksekusi pertama binary sesudah ekstraksi sekitar 6 detik |
| Proses | dengan watchdog mati: satu proses, tanpa anak, fd 3 warisan tetap terbuka, entri `txt` pertama adalah binary itu |
| Listener | tepat dua socket TCP di `127.0.0.1`, tanpa UDP |
| Probe readiness | exit 0 dalam sekitar 60 ms; ID instance lain exit 139 |
| `stackly_readiness` | `SHOW DATABASES` hanya `system`; tabel pengguna, `system.users`, `system.processes`, `file()`, `url()`, `CREATE`, `SETTINGS`, dan `SYSTEM` ditolak |
| `stackly_reload` | `SHOW GRANTS` hanya dua grant `SYSTEM RELOAD`; `SHOW DATABASES` kosong; `SYSTEM SHUTDOWN` ditolak; `SELECT 1` dan `numbers()` boleh |
| Port dipakai | exit 210, log `Listen [127.0.0.1]:<port> failed ... Address already in use` |
| Folder data dipakai server lain | exit 76, log `Cannot lock file <data>/status. Another server instance in same directory is already running.` di baris 2 dari 16; 10 baris terakhir hanya stack trace |
| `SIGHUP` | dicatat server sebagai buka ulang log (`Logging to console but received signal to close log file (ignoring)`); tidak terbukti memuat ulang `config.xml` |
| `SYSTEM RELOAD CONFIG` sebagai `stackly_reload` | exit 0; `users.xml` baru berlaku seketika (password lama ditolak), PID sama, key dari command line tetap berlaku; config rusak: exit 91 dan server tetap melayani config lama; sebagai user `readonly`: ditolak kode 164 |
| File config berubah tanpa perintah | pengawas file ClickHouse memuat ulang sendiri dalam beberapa detik (default 2 detik) |
| Config client asing | file client yang memuat `<password>` menggagalkan login user tanpa password walau `--user` di argv; `--config-file` ke `<config>` kosong menghindarinya |
| `config.xml` tanpa blok `user_directories` | key users dari argv tidak dipakai, server gagal start |
| `SIGTERM` | keluar dengan kode 0 dalam sekitar 1 detik, file `status` hilang |
| Tanpa `disable_internal_dns_cache` | log error DNS berulang tiap 15 detik di Mac yang hostname nya tidak bisa di resolve |
| Instance diam | sekitar 0,5 GB RSS, sekitar 170 thread |
| `data/preprocessed_configs/` | ClickHouse menyalin `config.xml` dan `users.xml` hasil olahannya ke sini, termasuk hash password |

### Perubahan core

Dua perubahan, keduanya additive menurut *Kebijakan kompatibilitas* spec 0036 (tujuh syaratnya berlaku untuk masing masing).

**1. Format `xml`** (`crates/core/src/provider/template.rs`): varian `TemplateFormat::Xml`, kode manifest `xml`, `escape_text` menurut AC-1, `boolean` `true` atau `false`, `allows_tags` true, `output_parses` true. Pesan `unknown_template_format` di `crates/core/src/provider/validate.rs` menyebut `xml`. Harness fuzz template di `crates/core/src/provider/fuzzing.rs` mendapat `xml` sebagai pilihan format baru; pemilihnya memakai `selector % 6`, jadi modulus naik ke 7 dan arti byte pertama corpus lama bergeser. Itu diterima: `cargo xtask fuzz seed` menulis ulang seed, dan replay regresi dijalankan sesudahnya. Tidak ada crate baru.

**2. Filter `sha256Hex`** (bersama spec 0040, bentuknya disepakati dengan sesi `arc-f40-spec` 2026-11-12): parser template di `template.rs` menerima satu filter sesudah path `secret.*`; pemeriksaan statis menolak pemakaian lain; `Segment::SecretHash { secret: PlanSecret, algo: HashAlgo }` di plan; `RenderedContent::materialize` di `crates/core/src/provider/plan.rs` memanggil `secret::delivery::hash`. Modul `secret::delivery` sudah ada di `EXPOSE_ALLOWED`, jadi daftar itu tidak berubah. Fitur yang masuk `main` lebih dulu (40 atau 43, menurut urutan lead) membangun filter ini; yang kedua hanya memakainya dan tidak menulisnya ulang. Teks AC-2 di sini adalah bentuk yang mengikat untuk kedua spec.

`cargo xtask codegen` dijalankan sesudah keduanya; schema editor di `schemas/provider/` hanya berubah kalau ia mendaftar nilai format. `docs/providers/manifest-reference.md` (baris `format`, dan satu kalimat untuk filter) diperbarui di langkah yang sama, karena `cargo xtask docs check` memeriksa cakupan schema.

### Ukuran RISK-07

| Jenis perubahan core | Jumlah untuk ClickHouse |
| --- | --- |
| Format template | 1 (`xml`, milik fitur ini) |
| Filter template | 1 (`sha256Hex`, bersama fitur 40; dihitung satu kali untuk keduanya) |
| Jenis probe, probe native, kind param | 0 |
| Primitive initialize, token interpolasi, field manifest | 0 |
| DTO, method, event, kode error di wire, migrasi | 0 |
| Kode Swift | 0 |

Selain itu hanya kode test, paket, fixture, catalog, dan penghapusan recipe di `xtask`.

### Test native

File `apps/daemon/tests/clickhouse.rs` dengan modul di `apps/daemon/tests/clickhouse/` (`harness.rs`, `lifecycle.rs`, `isolation.rs`, `security.rs`, `reload.rs`, `failure.rs`, `compatibility.rs`), mengikuti pola `apps/daemon/tests/nats.rs`: setiap test `#[ignore = "native: needs the managed ClickHouse 26.8.30.8 artifact in the runtime cache"]`, memakai `stackly_testkit::TestRoot` dengan root data yang namanya memuat spasi dan Unicode, port dari `allocate_port`, runtime dari `stackly_testkit::RuntimeCache` (tanpa jaringan), dan hanya password fixture. Harness menjalankan SQL dengan men spawn `clickhouse client` dari runtime terpasang (kode test, bukan daemon), dengan password fixture lewat env `CLICKHOUSE_PASSWORD` milik proses anak itu (cadangan kalau binary tidak membacanya: `--password`, karena ini credential fixture).

| Test | Membuktikan |
| --- | --- |
| `clickhouse_lifecycle_in_process` | create, start, ready, health, dua connection, stop, start lagi dengan data utuh (AC-7, AC-9, AC-11) |
| `clickhouse_only_listens_on_its_two_registry_ports` | audit `lsof` TCP dan UDP, `system.server_settings`, dan tidak ada file di luar instance (AC-4) |
| `clickhouse_runs_as_one_foreground_process` | tanpa anak, fd 3, program sama (AC-5) |
| `two_clickhouse_instances_keep_their_own_events_table` | AC-037 persis: `CREATE TABLE default.events (id UInt64, v String) ENGINE = MergeTree ORDER BY id` di keduanya, satu baris berbeda per instance (`(1, 'alpha')` dan `(2, 'beta')`), restart bergantian, `SELECT * FROM default.events` tiap instance hanya barisnya sendiri, audit listener keduanya (AC-8) |
| `clickhouse_default_user_needs_the_keychain_password` | password salah dan kosong ditolak di HTTP dan native; pindaian sentinel password di folder instance, log, dan `ps` (AC-6) |
| `clickhouse_managed_users_can_do_nothing_else` | daftar penolakan `stackly_readiness` dan `stackly_reload` di AC-6 |
| `the_readiness_probe_fails_against_a_clickhouse_with_another_id` | pola `apps/daemon/tests/nats/compatibility.rs`: `ProbeSpec` readiness hasil `run_plan` instance dijalankan lewat `stackly_core::supervisor::probe::run` terhadap server asing dari binary yang sama, yang punya `stackly_readiness` tetapi macro ID lain; hasilnya gagal. Terhadap server instance sendiri hasilnya sukses (AC-7) |
| `a_port_in_use_refuses_the_start_and_leaves_the_foreign_process` | server asing di port `native`; start ditolak `supervisor.endpoint_in_use`, PID asing tetap hidup (AC-12) |
| `clickhouse_reload_changes_the_log_level_and_the_password` | AC-10, termasuk config rusak: file `config.xml` yang diubah menjadi XML tidak sah lalu `SYSTEM RELOAD CONFIG` dijalankan harness sendiri berakhir exit bukan 0 dan instance tetap menjawab |
| `a_data_folder_in_use_fails_the_start_and_is_left_alone` | server asing di folder data instance yang sedang berhenti, dengan port sendiri dari `allocate_port`; start berakhir `failed`, `exitCode` 76, file `status` dan PID asing utuh (AC-12) |
| `clickhouse_compatibility` | lewat `SpawnedDaemon`: menulis `compatibility.json` dengan check yang lulus (BR-025), sumber AC-16. ID check, urut: `lifecycle`, `readiness_identity`, `listeners`, `auth`, `isolation`, `reload`, `persistence`, `stop` (bentuk record mengikuti `apps/daemon/tests/nats/harness.rs`) |

Ditambah `clickhouse_26_8_passes_conformance` di `apps/daemon/tests/provider_conformance.rs` (dan baris `("clickhouse", (120, 120, 60, 30, 43_000))` di test tabel batas waktu file itu: init default 120, readiness 120, stop 60, reload 30, health `10 × (3 + 1) + 3` detik) dan `clickhouse_installs_from_the_cache` di `apps/daemon/tests/native_runtime_install.rs` (keluar dari `blocked_line_test!`, dengan `CLICKHOUSE: Line` baru di `apps/daemon/tests/native_runtime_install/harness.rs`).

Keputusan untuk Follow-up spec 0036 ("apakah test BR-025 adalah test konformitas"): bukan. Record BR-025 ClickHouse datang dari `clickhouse_compatibility`, karena record itu harus memuat check khusus provider (tabel sama nama, audit listener) yang tidak ada di harness generik.

**Yang `BLOCKED`.** Setiap test native di atas berakhir `BLOCKED` dengan reason `the managed ClickHouse 26.8.30.8 artifact is not in the runtime cache` dan owner `@ojiepermana` kalau entri cache `097b41ad…7402` tidak ada atau rusak. Di Mac ini entri itu ada per 2026-11-12, jadi sesi build bisa menjalankannya tanpa unduhan.

### Langkah manual (bukan pekerjaan sesi)

Ditulis developer di `verify.md` bagian `### UI / manual`, status awal `NOT_RUN`, owner `@ojiepermana`:

1. Run spike native dari paket: `clickhouse_runs_from_managed_artifact` di `xtask/tests/native_runtime.rs` lewat wrapper native di tree bersih (bisa mengunduh, butuh sesi login GUI).
2. Fuzz 10 menit untuk target template sesudah format `xml` dan filter masuk (`cargo xtask fuzz run provider_template --time 600`).
3. Sesi GUI: buat instance ClickHouse dari form, start, salin connection, buka Play UI, buka `clickhouse client` di Terminal dan tempel password, lalu stop.

### Data model

Tidak ada tabel, kolom, atau migrasi baru. Instance ClickHouse memakai baris `instances`, `endpoints`, `config_revisions`, dan `secret_uses` yang ada.

### API surface

Tidak ada method, DTO, atau event baru. `ProtocolVersion` tidak berubah. Kalau saat build ternyata ada kontrak wire yang harus berubah, nomornya adalah nomor bebas berikutnya, ditetapkan lead saat build; hal yang sama berlaku untuk nomor migrasi dan revision catalog.

| Permukaan | Perubahan |
| --- | --- |
| Manifest `stackly/v1alpha1` | nilai `format: xml`; filter `\| sha256Hex` di template `sensitive` |
| IPC dan CLI | tidak ada; `official/clickhouse` muncul lewat method catalog, runtime, dan instance yang ada |
| GUI | tidak ada kode; form dari settings schema, tile `Ch` yellow dari catalog |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| render `config.xml` | level log | `settings.logLevel` (default `information`) |
| render `config.xml` | ID di macro | `instance.id` (dibuat daemon saat create) |
| render `config.xml` | host dan dua port | `endpoint.http.host`, `endpoint.http.port`, `endpoint.native.port` (dipilih tahap `endpoints` start, spec 0010) |
| render `config.xml` | semua path | `instance.data`, `instance.temp` (layout spec 0004) |
| render `users.xml` | hash password | `secret.defaultPassword` dari Keychain lewat `ResolvedSecrets`, SHA-256 hex |
| start | path `users.xml` di argv | `instance.secrets` |
| probe readiness | host, port, ID harapan | `endpoint.native.host`, `endpoint.native.port`, `instance.id` |
| probe health | alamat | endpoint `http` |
| reload | host, port, user | `endpoint.native.host`, `endpoint.native.port`, literal `stackly_reload` |
| ganti password | nilai baru | `secret.set` pada reference secret `defaultPassword` instance itu (spec 0011) |
| versi teramati | `26.8.30.8` | capture group `runtime.versionCheck.pattern` atas keluaran `server --version` |
| connection | user | literal `default`; password `secret.defaultPassword`; alamat dari endpoint |
| catalog | mark, tile, kategori | `Ch`, `yellow`, `analytics` (isi `[[planned]]` yang ada, spec 0017) |
| catalog | revision dan `published_at` | nomor bebas berikutnya (revision di `main` saat spec ini ditulis: 11), ditetapkan lead saat build; waktu commit |
| catalog | catatan platform | `linux` `planned` "Linux releases exist upstream; Stackly supports macOS first."; `windows` `unsupported` "Upstream runs ClickHouse on Windows only through WSL, which Stackly never installs or uses." |
| test native | artefak | `runtimes/inventory.toml` entri `clickhouse-26.8.30.8-macos-aarch64`, sha256 `097b41ad…7402`, lewat `RuntimeCache` |
| test native | password | fixture ber awalan credential fixture spec 0002 |

### Edge cases dan kegagalan

- **Port bentrok**: tahap `endpoints` start menolak sebelum spawn (`supervisor.endpoint_in_use`) dan mengisi `suggestedPort` seperti provider lain (spec 0033). Kalau port direbut di antara pemeriksaan itu dan bind, ClickHouse keluar dengan kode 210 dan start berakhir `failed`. Port hanya berubah lewat `instance.change_endpoints`.
- **Server ClickHouse asing sempat menjawab di port native**: query identitas gagal, jadi instance tidak pernah `ready` karena server yang salah.
- **Folder data terkunci proses luar**: exit 76, `lastFailure` `process_exited`; tidak ada remedy (lihat catatan paket). Lock instance Stackly sendiri (`instance.lock`) sudah mencegah dua instance Stackly memakai folder yang sama.
- **Nilai yang tidak bisa ditulis sebagai XML**: path atau nilai dengan karakter kontrol gagal saat plan dengan `UnrepresentableValue` yang menyebut token, sebelum proses apa pun di spawn.
- **Password dengan karakter khusus**: hash dihitung dari byte persis, jadi `&`, `<`, kutip, dan spasi aman; `printableOnly` menolak karakter kontrol saat nilai disimpan.
- **Reload dan pengawas file ClickHouse**: ClickHouse juga memuat ulang sendiri dalam sekitar 2 detik saat file berubah. Render reload menulis file lalu menjalankan `SYSTEM RELOAD CONFIG`; dua muat ulang yang berdekatan menghasilkan keadaan yang sama. Kalau perintah itu exit bukan 0 (config ditolak), operasi reload gagal dengan aturan reload spec 0026 dan server tetap memakai config terakhir yang sah.
- **Drift**: `config.xml` yang diubah pengguna di folder config adalah drift biasa (spec 0029). ClickHouse akan memuat perubahan itu sendiri; Stackly tetap tidak menimpanya dan menahan render sampai drift diselesaikan.
- **`users.xml` hilang saat instance berjalan** (misalnya macOS membersihkan folder temp): server tetap berjalan dengan user yang sudah dimuat; reload atau start berikutnya merender ulang (spec 0011).
- **Eksekusi pertama lambat**: binary 892 MB diperiksa macOS saat pertama dijalankan (sekitar 6 detik di percobaan), karena itu `versionCheck.timeoutSeconds` 30 dan `readinessSeconds` 120.
- **Disk penuh atau data rusak**: ClickHouse gagal start atau query gagal; readiness tidak mendeteksi disk penuh setelah ready. Batas yang diketahui, sama dengan NATS.

### Jawaban Follow-up dari spec lain

| Sumber | Jawaban di spec ini |
| --- | --- |
| Spec 0009, format `xml` | AC-1, *Perubahan core* butir 1 |
| Spec 0009 dan 0011, bentuk manifest mekanisme `hash` | AC-2, *Perubahan core* butir 2 (filter template di config `sensitive`, bersama spec 0040) |
| Spec 0003, Follow-up "Fitur 40, 42, dan 43" dan baris ClickHouse tabel *Langkah spike per provider* | AC-15: recipe dan penanda `F43` dihapus; baris tabel itu tidak dipakai lagi. Config spike kini dari paket, jadi spike berjalan dengan user `default` ber password fixture, bukan tanpa password |
| Spec 0018 AC-20 | `clickhouse_installs_from_the_cache` tidak lagi `BLOCKED` (AC-14) |
| Spec 0017, *Aturan masuk catalog* | AC-16: pindah ke `[[provider]]` di commit bukti native |
| Spec 0036, "Fitur 39 sampai 43" dan *Dari contoh ke paket resmi* | Build plan mengikuti urutan itu; `clickhouse` sudah ada di `PROVIDER_WORDS`; keputusan BR-025 di *Test native* |
| Spec 0030, test native launchd sebelum `HANDOVER_PROVEN` | Tidak dikerjakan fitur ini. ClickHouse tetap `unproven` dan memakai jalur `stop_restore`; test `handover.rs` yang menyatakan `official/clickhouse` belum terbukti tetap benar. Baris Follow-up spec 0030 tetap terbuka |
| Spec 0034, remedy provider lain | tidak ada remedy di revision ini; alasannya di catatan paket (kutipan 10 baris tidak memuat baris kuncinya) |
| Spec 0070, `openWith` ClickHouse | `play` (browser) dan `client` (terminal) (AC-11) |
| Spec 0021 dan 0020 | blok `connection` dan form tanpa kode khusus (AC-11, AC-16) |

### Usulan update spec lain

Untuk dibawa lead ke pemilik spec (sesi ini tidak mengubahnya):

- Spec 0009: dua baris baru di tabel *Perluasan additive* (format `xml`, AC pemilik 0043 AC-1; filter `sha256Hex`, AC pemilik 0040 dan 0043 AC-2); daftar format di *Template config* dan baris escaper `xml`; grammar `{{ secret.<field> | sha256Hex }}`; kalimat "Mekanisme `hash` belum punya bentuk" diganti rujukan ke filter itu.
- Spec 0011: baris mekanisme 2 menyebut filter `sha256Hex`, dan bahwa config yang memuat hash tetap `sensitive` di `${instance.secrets}`; residual baru: ClickHouse menyalin hash ke `data/preprocessed_configs/`, yang ikut cold backup.
- Spec 0003: centang bagian fitur 43 di Follow-up "Fitur 40, 42, dan 43".
- Spec 0018: AC-20 dan langkah 16, ClickHouse keluar dari daftar `BLOCKED`.
- Spec 0070: centang bagian ClickHouse di Follow-up nya. Spec 0034: catat bahwa ClickHouse tidak menambah remedy, dan bahwa kutipan 10 baris terakhir tidak cukup untuk tool yang menulis stack trace sesudah pesan error.

### Key invariants

- Instance ClickHouse yang berjalan dari config hasil render Stackly hanya punya dua listener, keduanya loopback, keduanya tercatat di registry.
- Tidak ada key cluster, replikasi, Keeper, atau protokol lain di config hasil render.
- Proses yang diawasi adalah server itu sendiri, bukan watchdog.
- Password `default` tidak pernah ditulis ke disk, log, atau argv; hanya hash nya.
- `stackly_readiness` tidak pernah punya hak selain `SELECT ON system.one`, dan `stackly_reload` tidak pernah punya hak selain dua `SYSTEM RELOAD`.
- Readiness tidak pernah `ready` karena server lain.
- Stackly tidak pernah memberi sinyal pada proses ClickHouse yang bukan miliknya dan tidak pernah menghapus `status`.
- Tidak ada artefak, catatan, atau teks yang menyebut ClickHouse native di Windows.

### Security model

- Satu user OS. Kedua port hanya di `127.0.0.1`; user `default` dibatasi ke `127.0.0.1` dan `::1` dan wajib password.
- Password hanya di Keychain. Di disk hanya hash SHA-256 tanpa salt: di `users.xml` (folder run, 0600) dan di salinan `data/preprocessed_configs/` buatan ClickHouse, yang ikut cold backup. Password lemah bisa ditebak offline dari hash; residual ini sudah dinyatakan spec 0011.
- `stackly_readiness` tanpa password bisa dipakai proses mana pun di Mac itu yang bisa menjangkau loopback, termasuk user OS lain. Yang ia dapat: `SELECT` atas `system.one`, fungsi tanpa hak seperti `version()`, dan nama macro instance. Ia tidak bisa membaca data, user, proses, file, atau URL, dan dibatasi 2 detik, 16 MiB, satu thread per query. `/ping` dan halaman statis `/play` juga terbuka tanpa login, seperti di setiap ClickHouse. Kelasnya sama dengan monitoring NATS.
- `stackly_reload` tanpa password juga terjangkau proses lokal mana pun. Yang bisa ia lakukan: meminta server memuat ulang file yang Stackly tulis, dan menjalankan query tanpa tabel (`SELECT 1`, `numbers()`) dengan batas profil 10 detik, 16 MiB, satu thread. Batas itu bisa diubah user itu untuk sesinya sendiri, karena ia tidak `readonly`; ia tetap tidak bisa membaca data, user, atau file, dan tidak bisa menghentikan server.
- User `default` punya `access_management`, jadi pengguna bisa membuat user dan role lewat SQL; hasilnya di `data/access/`.
- Probe, perintah reload, dan open action tidak pernah menerima secret. Terminal memakai binary runtime instance itu sendiri dan `--ask-password`, dengan password hanya lewat clipboard tersembunyi GUI (spec 0070).
- Tidak ada data teregulasi. Tidak ada audit baru: create, start, stop, dan reload sudah diaudit engine operasi.

### Configuration required

Tidak ada env var, credential, atau akun baru. Test native butuh artefak ClickHouse di cache xtask (sudah ada di Mac ini).

### Critical test scenarios

- Happy path: `clickhouse_lifecycle_in_process` dan `clickhouse_26_8_passes_conformance`, verifies **AC-3**, **AC-5**, **AC-7**, **AC-9**, **AC-11**, **AC-14**
- Isolasi: `two_clickhouse_instances_keep_their_own_events_table`, verifies **AC-8**, **AC-4**
- Listener: `clickhouse_only_listens_on_its_two_registry_ports` dan test core key yang dilarang, verifies **AC-4**
- Auth: password salah, kosong, dan user readiness yang ditolak di semua hal lain, verifies **AC-6**
- Server yang salah: `the_readiness_probe_fails_against_a_clickhouse_with_another_id`, verifies **AC-7**
- Kegagalan start: port dipakai dan folder data dipakai, proses asing tidak disentuh, verifies **AC-12**
- Reload: level log dan rotasi password dengan PID sama, dan config rusak yang ditolak tanpa menjatuhkan server, verifies **AC-10**
- Format: escaper `xml` untuk lima karakter, penolakan karakter kontrol tanpa nilai, tag menempel pada teks, dan render golden yang diterima binary asli, verifies **AC-1**
- Filter: vektor uji SHA-256, `secret_not_allowed_here` di config tidak `sensitive`, `template_type` di path bukan secret, filter tak dikenal, dan dua filter; `Debug` plan tanpa hash, verifies **AC-2**
- Platform: host Windows dengan catalog uji yang punya artefak `macos` dan `linux`, verifies **AC-13**
- Spike: `recipe("clickhouse")` adalah `PackageRecipe`, verifies **AC-15**

## Build plan

Urutan mengikuti pendekatan Journey proyek: jalur "instance ClickHouse dari catalog sampai ready, dipakai, lalu stop" dibuat lengkap dengan semua state dan kegagalannya sebelum entri catalog dibuka. Milestone 1 menyiapkan dua perubahan core dan paket, yang semuanya terbukti tanpa binary. Milestone 2 membuktikan journey pada binary asli lewat daemon. Milestone 3 membuka catalog, membekukan paket, dan membersihkan spike.

**Milestone 1: core dan paket (tanpa binary)**

1. Format `xml`: `TemplateFormat::Xml` di `crates/core/src/provider/template.rs` (kode, escaper, boolean, `allows_tags`, `output_parses`) dengan unit test di file itu; pesan format di `crates/core/src/provider/validate.rs`; pilihan format baru di `crates/core/src/provider/fuzzing.rs` dan doc seed di `xtask/src/fuzz.rs`; test aturan di `crates/core/tests/provider_rules.rs` (tag menempel, karakter kontrol ditolak tanpa nilai, corpus `frozen.json` tidak berubah); baris `format` di `docs/providers/manifest-reference.md`; `cargo xtask codegen` dan `cargo xtask fuzz seed`. Memenuhi **AC-1**.
2. Filter `sha256Hex` (lewati kalau fitur 40 sudah memasukkannya ke `main`; lalu hanya pastikan test AC-2 ada): grammar dan pemeriksaan statis di `template.rs`, `Segment::SecretHash` dan `materialize` di `crates/core/src/provider/plan.rs`, render contoh validasi, `Debug` tanpa hash, test di `crates/core/tests/provider_rules.rs` dan unit test `template.rs`, contoh corpus fuzz template, satu kalimat di `docs/providers/manifest-reference.md`, dan satu pemakaian di paket fixture `crates/core/tests/fixtures/packages/` (syarat 7 kebijakan additive). Memenuhi **AC-2**.
3. Paket: `cargo xtask provider new clickhouse`, lalu `providers/clickhouse/provider.yaml`, `schemas/settings.schema.json`, `templates/config.xml.tmpl`, `templates/client.xml.tmpl`, `templates/users.xml.tmpl`, `tests/scenarios.yaml`, `README.md`, `LICENSE-NOTICES.md` persis *Paket ClickHouse*; hapus `versions.json` tulisan tangan dan jalankan `cargo xtask codegen`; `cargo xtask provider check`. Memenuhi **AC-3**.
4. Test core paket di `crates/core/tests/provider_packages.rs`: golden `init`, `run`, dan render kedua file di `crates/core/tests/golden/provider/clickhouse/` (`STACKLY_UPDATE_GOLDEN=1`), `init_plan` tanpa port, test *Key yang dilarang* atas template dan render, test bahwa `users.xml` hasil `materialize` memuat hash fixture dan bukan nilainya, `clickhouse` di daftar paket `crates/core/tests/provider_check.rs`, golden open action di `crates/core/tests/open_with_render.rs` dan golden connection di `crates/core/tests/connection_render.rs`, dan test platform AC-13 (host Windows, catalog uji dengan artefak `macos` dan `linux`, lewat `installability` dan `native_support`). Memenuhi **AC-3**, **AC-4** (bagian template), **AC-6** (bagian render), **AC-11** (bagian render), **AC-13**.
5. Fixture form: `protocols/fixtures/instance/create-form-clickhouse.json` lewat `STACKLY_UPDATE_GOLDEN=1 cargo test -p stacklyd --test create_form_fixtures`, entri di `protocols/fixtures/instance/index.json`, `cargo xtask fuzz seed`, `cargo test -p xtask`, dan `swift test --package-path clients/swift` serta test form generik `StacklyUI`. Memenuhi **AC-16** (bagian fixture).

**Milestone 2: ClickHouse 26.8.30.8 asli lewat daemon**

6. Harness `apps/daemon/tests/clickhouse.rs` dan `apps/daemon/tests/clickhouse/harness.rs` (root Unicode, `RuntimeCache` dengan jalur `BLOCKED`, helper SQL lewat `clickhouse client`, helper `lsof`, helper server asing dengan guard yang me reap prosesnya), lalu `clickhouse_lifecycle_in_process` dan `clickhouse_runs_as_one_foreground_process`. Memenuhi **AC-5**, **AC-7** (bagian happy path), **AC-9**, **AC-11**.
7. `apps/daemon/tests/clickhouse/isolation.rs` dan `security.rs`: `two_clickhouse_instances_keep_their_own_events_table`, `clickhouse_only_listens_on_its_two_registry_ports`, `clickhouse_default_user_needs_the_keychain_password`, `clickhouse_managed_users_can_do_nothing_else`. Memenuhi **AC-4**, **AC-6**, **AC-8**.
8. `apps/daemon/tests/clickhouse/reload.rs` dan `failure.rs`: `clickhouse_reload_changes_the_log_level_and_the_password`, `the_readiness_probe_fails_against_a_clickhouse_with_another_id`, `a_port_in_use_refuses_the_start_and_leaves_the_foreign_process`, `a_data_folder_in_use_fails_the_start_and_is_left_alone`. Memenuhi **AC-7**, **AC-10**, **AC-12**.
9. Konformitas dan install: `clickhouse_26_8_passes_conformance` dan baris tabel batas waktu di `apps/daemon/tests/provider_conformance.rs`; `clickhouse_installs_from_the_cache` di `apps/daemon/tests/native_runtime_install.rs` pindah dari `blocked_line_test!` ke test line yang nyata, dengan `CLICKHOUSE: Line` di `apps/daemon/tests/native_runtime_install/harness.rs`. Jalankan semua test native langkah 6 sampai 9 dari cache; catat hasil di `verify.md`. Memenuhi **AC-14**.

**Milestone 3: catalog, corpus, spike, dan ukuran**

10. Spike dari paket: hapus struct `ClickHouse`, fungsi `xml`, cabang `"clickhouse"`, dan penanda `// provisional: F43` di `xtask/src/runtime/spike/providers.rs`; sesuaikan test `hand_written("clickhouse")` di file itu dan daftar `find_package` di `xtask/src/runtime/spike/package_recipe.rs`; `cargo test -p xtask`. Memenuhi **AC-15**.
11. Catalog dan evidence, dalam urutan ini: commit paket final dan test; `clickhouse_compatibility` (`apps/daemon/tests/clickhouse/compatibility.rs`) dari `HEAD` bersih dan `cargo xtask runtime compat-record`, dijalankan sesi build dari cache; lalu satu commit berisi `evidence/compatibility/`, `providers/clickhouse/versions.json` (field `compatibility` dari codegen), pindahnya `official/clickhouse` ke `[[provider]]` di `providers/catalog.toml` dengan dua catatan platform di *Value sourcing*, revision catalog baru (nomor bebas berikutnya, ditetapkan lead saat build), dan `providers/catalog-revisions.json` dari `cargo xtask catalog check --update`. Fixture catalog di `protocols/fixtures/catalog/` yang memuat daftar provider bawaan ikut diperbarui kalau test nya menuntut. Memenuhi **AC-16** (bagian catalog dan evidence), **AC-13** (catatan platform).
12. `cargo xtask provider freeze providers/clickhouse` (menulis `crates/core/tests/fixtures/compat/clickhouse/`), lalu `cargo test -p stackly-core --test provider_packages`. Memenuhi **AC-16** (bagian corpus).
13. Ukuran RISK-07 dan langkah manual: `git diff --numstat` terhadap commit dasar, dikelompokkan seperti tabel *Ukuran RISK-07*, dicatat di `verify.md`; tiga langkah *Langkah manual* ditulis di `verify.md` bagian `### UI / manual`. Memenuhi **AC-17**.

**Prasyarat**:

- Langkah 1 dan 2 butuh baris spec 0009 di tabel *Perluasan additive* (syarat pertama kebijakan additive spec 0036). Lead menjadwalkan update spec 0009 itu sebelum atau bersama langkah 1; isinya di *Usulan update spec lain*.
- Langkah 2 bergantung pada urutan lead antara fitur 40 dan 43. Kalau fitur 40 lebih dulu, langkah 3 butuh `main` yang sudah memuat filter itu.
- Langkah 6 sampai 9 butuh artefak ClickHouse di cache xtask; tanpa itu test nya `BLOCKED` dan langkah 11 tidak boleh memindahkan entri catalog.
- Langkah 11 dan 12 sesudah langkah 9 lulus (`provider freeze` menolak paket di luar `[[provider]]`).

**Sesi paralel.** Fitur ini tidak bisa dibangun bebas berdampingan dengan fitur 39 sampai 42, karena file berikut dipakai bersama dan hanya boleh diubah satu sesi pada satu waktu:

| File bersama | Langkah | Juga disentuh |
| --- | --- | --- |
| `crates/core/src/provider/template.rs` | 1, 2 | fitur 40 (`valkeyConf`, filter), fitur 42 (`mysqlCnf`), fitur 41 (bcrypt) |
| `crates/core/src/provider/validate.rs` (pesan daftar format) | 1 | fitur 40, 42 |
| `crates/core/src/provider/plan.rs` | 2 | fitur 40 (filter), fitur 41 |
| `crates/core/src/provider/fuzzing.rs`, `xtask/src/fuzz.rs` | 1, 2 | fitur 40, 42 |
| `crates/core/tests/provider_rules.rs`, `crates/core/tests/provider_packages.rs`, `crates/core/tests/provider_check.rs`, `crates/core/tests/open_with_render.rs`, `crates/core/tests/connection_render.rs` | 1, 2, 4 | semua fitur provider |
| `docs/providers/manifest-reference.md` | 1, 2 | fitur 40, 42 |
| `protocols/fixtures/instance/index.json` | 5 | semua fitur provider baru |
| `apps/daemon/tests/provider_conformance.rs`, `apps/daemon/tests/native_runtime_install.rs`, `apps/daemon/tests/native_runtime_install/harness.rs` | 9 | fitur 39 sampai 42 |
| `xtask/src/runtime/spike/providers.rs`, `xtask/src/runtime/spike/package_recipe.rs` | 10 | fitur 40, 42 |
| `providers/catalog.toml`, `providers/catalog-revisions.json`, `protocols/fixtures/catalog/` | 11 | fitur 40 sampai 42 |

Tidak disentuh fitur ini: `runtimes/inventory.toml` (artefak sudah ada), `xtask/src/check_deps/source_scan.rs` (`clickhouse` sudah di `PROVIDER_WORDS`, `EXPOSE_ALLOWED` tidak berubah), `crates/core/src/supervisor/handover.rs`, `crates/protocol/`, migrasi, dan kode Swift. File milik fitur ini saja: `providers/clickhouse/`, `apps/daemon/tests/clickhouse.rs`, `apps/daemon/tests/clickhouse/`, `crates/core/tests/golden/provider/clickhouse/`, `crates/core/tests/fixtures/compat/clickhouse/`, dan `protocols/fixtures/instance/create-form-clickhouse.json`. Di dalam fitur ini, langkah 6 sampai 8 bisa dibagi ke sesi terpisah sesudah harness langkah 6 masuk, karena tiap langkah menulis file modul sendiri; langkah 10 bisa berjalan sejajar dengan milestone 2.

## Consequences

**Positive**:

- ClickHouse berjalan dengan dua perubahan core kecil, satu di antaranya dipakai bersama Valkey, dan tanpa probe, primitive, protokol, migrasi, atau kode Swift baru. Itu jawaban terukur untuk RISK-07.
- Format `xml` dan filter `sha256Hex` tersedia untuk provider berikutnya.
- Tidak ada lagi dua tempat yang tahu cara menjalankan ClickHouse: recipe spike hilang.
- Listener, path, dan identitas server dibuktikan pada binary asli, bukan diasumsikan.

**Negative / tradeoffs**:

- Setiap percobaan readiness men spawn binary 892 MB (sekitar 60 ms di percobaan). Health memakai HTTP supaya biaya itu hanya ada selama start dan reload.
- Dua user tanpa password (`stackly_readiness`, `stackly_reload`) bisa dipakai proses lokal mana pun, dengan hak yang sangat sempit (lihat *Security model*).
- File di `config.d/` yang ditaruh pengguna tidak terlihat Stackly dan bisa membuka listener yang tidak tercatat.
- Kegagalan "folder data dipakai server lain" tidak punya remedy, karena kutipan log 10 baris tidak memuat pesannya.
- Hash SHA-256 tanpa salt ikut tersalin ke `data/preprocessed_configs/` dan cold backup.
- Format `xml` tidak memeriksa bahwa hasil render adalah XML yang sah; template yang salah baru ketahuan saat ClickHouse menolak start. Paket resmi dijaga test native.
- Tanpa `dataMarker`, kecocokan data dan runtime hanya bersandar pada line di registry.
- ClickHouse tetap `unproven` untuk restart daemon terencana, jadi instance nya dihentikan lalu dipulihkan (`stop_restore`).
- Instance diam memakai sekitar 0,5 GB RAM dan runtime memakai 892 MB disk.
- Tabel log sistem ClickHouse tidak aktif; pengguna yang butuh `query_log` belum bisa menyalakannya dari Stackly.
- Banyak file inti dan file test dipakai bersama fitur provider lain (tabel *Sesi paralel*), jadi urutan merge diatur lead.

**Neutral**:

- Satu line saja (26.8). Line berikutnya menambah entri inventaris, test konformitas, dan record BR-025 sendiri.
- Spike ClickHouse sekarang berjalan dengan password fixture.
- Catalog bawaan naik satu revision.

## Follow-up

- [ ] Lead: tetapkan urutan fitur 40 dan 43 untuk filter `sha256Hex`, dan bawa *Usulan update spec lain* ke pemilik spec 0009, 0011, 0003, 0018, 0034, dan 0070.
- [ ] Spec 0030: test native launchd ClickHouse sebelum `HANDOVER_PROVEN` (tetap terbuka).
- [ ] Fase 09: bukti cold backup dan restore ClickHouse; pertimbangkan `BACKUP` upstream untuk backup online.
- [ ] Pemeriksaan doctor untuk folder `config.d/`, `conf.d/`, dan `users.d/` yang tidak dikelola (perubahan core, bukan fitur ini).
- [ ] Remedy untuk tool yang menulis stack trace sesudah pesan error butuh kutipan log yang lebih panjang atau pencocokan di log store (usulan untuk spec 0034).
- [ ] Setting lanjutan yang ditunda: batas memori server, tabel log sistem, dan TLS.
- [ ] Open action untuk klien desktop (misalnya TablePlus) setelah skema URL nya untuk ClickHouse diverifikasi.
- [ ] `/sync` setelah build: AGENTS.md menyebut format `xml` dan filter `sha256Hex`, dan daftar provider yang sudah punya paket.
- [ ] Kamu: tiga langkah manual di *Langkah manual*.

## Rationale

Alasan, opsi yang ditimbang, dan catatan percobaan: lihat [rationale.md](rationale.md).
