# 0039. Provider NATS dan JetStream

**Date**: 2026-11-12
**Status**: In Progress

## Summary

Spec ini menutup pekerjaan NATS yang ditinggalkan fitur 23: setiap instance NATS baru memakai password client, readiness tetap jujur tanpa pernah membaca Keychain saat probe, dan dua instance dengan JetStream terbukti hanya melihat stream miliknya setelah restart. Caranya hampir seluruhnya lewat paket `providers/nats` revision `1.1.0` (satu user `stackly` dengan password dari secret, satu user readiness tanpa password di akun NATS terpisah yang hanya boleh bertanya ke API JetStream, dan batas resource dalam MiB dan KiB). Satu satunya perubahan core adalah escaper (pengubah teks supaya aman ditulis ke file) format `natsConf`, yang sekarang menerima tanda kutip, garis miring terbalik, dan tanda dolar. Tidak ada perubahan probe, protokol IPC, migrasi, daemon, CLI, atau kode Swift di luar fixture.

## Requirements

**User stories**:

- Sebagai developer, saya ingin server NATS lokal saya meminta password, supaya proses lain di Mac ini tidak bisa membaca atau menulis pesan aplikasi saya.
- Sebagai developer, saya ingin menjalankan dua NATS dengan JetStream berdampingan, supaya dua proyek tidak pernah saling melihat stream, dan data stream file tetap ada setelah restart.
- Sebagai developer, saya ingin kegagalan start karena port monitoring dipakai proses lain menunjuk endpoint yang tepat, supaya saya tahu port mana yang harus diganti dan proses lain itu tidak disentuh.
- Sebagai maintainer Stackly, saya ingin bukti bahwa NATS lengkap bisa dibangun dengan paket ditambah perubahan core yang terhitung, supaya RISK-07 terjawab dengan angka.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-036):

- **AC-1** (paket `1.1.0`, REQ-036): `providers/nats` berisi persis *Paket NATS 1.1.0*: `metadata.revision` `1.1.0`, settings schema baru, template baru, `configFiles.natsConf` yang `sensitive` di `${instance.secrets}/nats.conf`, argumen start yang menunjuk file itu, `run` di `managedFilesystem`, entri `connection` `uri` dengan `${secret.clientPassword}`, `tests/scenarios.yaml`, dan README baru. `cargo xtask provider check` mencetak `ok official/nats 1.1.0 <manifest_sha256 baru>` tanpa temuan penulis. Golden `init_plan`, `run_plan`, dan render NATS di `crates/core/tests/provider_packages.rs` ditulis ulang, dan golden itu tidak pernah memuat nilai secret (hanya `<secret:clientPassword>`). Entri corpus `crates/core/tests/fixtures/compat/nats/1.0.1/` tidak berubah satu byte pun.
- **AC-2** (escaper `natsConf`, satu perubahan core): untuk format `natsConf`, teks ditulis berkutip ganda dengan `"` menjadi `\"` dan `\` menjadi `\\`; `$` dan karakter cetak lain keluar apa adanya; karakter kontrol (U+0000 sampai U+001F, U+007F sampai U+009F) tetap ditolak `unrepresentable_value` yang menyebut field atau token, tidak pernah nilainya. Aturan yang sama berlaku untuk segmen secret saat `RenderedContent::materialize`. Unit test di `template.rs` membuktikan setiap baris tabel *Escaper `natsConf`*; test `provider_compat` tetap lulus tanpa mengubah `frozen.json` mana pun; dan test native AC-3 membuktikan nilai yang memuat ketiga karakter itu terbaca sama oleh `nats-server` 2.15.1.
- **AC-3** (auth client, REQ-036): pada instance yang `ready`, koneksi client dengan user `stackly` dan password instance itu mendapat `PONG` dan bisa membuat stream serta publish. Password salah mendapat `-ERR 'Authorization Violation'`. Koneksi tanpa credential diterima sebagai user `stackly-readiness` di akun `STACKLY_READINESS` dan hanya bisa: `PING`, publish ke `$JS.API.INFO`, dan subscribe ke `_INBOX.stackly.>`; publish ke subject lain, subscribe ke `>`, dan `$JS.API.STREAM.CREATE.X` masing masing mendapat `-ERR` yang diawali `'Permissions Violation`. Koneksi itu tidak pernah menyentuh akun aplikasi: setelah user `stackly` membuat stream `ORDERS` (subject `orders.>`) dengan satu pesan, `PUB $JS.API.INFO orders.evil 0` dari koneksi tanpa credential tidak menambah pesan di `ORDERS`, dan balasan `$JS.API.INFO` untuk koneksi itu melaporkan `streams` 0. Pada instance dengan `jetstream: false`, koneksi tanpa credential juga ditolak publish ke `$JS.API.INFO`. Satu instance yang dibuat dengan password berliteral Rust `"p\"q\\r$s-fixture"` (huruf `p`, tanda kutip ganda, `q`, satu garis miring terbalik, `r`, `$`, lalu `s-fixture`) menerima login dengan password itu (bukti AC-2 pada binary asli).
- **AC-4** (password tidak pernah bocor): selama instance berjalan, byte password tidak ada di argv dan environment `nats-server`, di file mana pun di bawah folder data, config, dan log instance (termasuk `config-history`), di log store instance, di balasan `/varz`, `/connz?auth=1&subs=1`, `/jsz`, `/accountz`, dan `/subsz` endpoint `monitoring`, dan di field `masked` entri `connection` dari `instance.get_detail`. `nats.conf` hanya ada di `<run instance>/secrets/nats.conf` dengan mode `0600` di folder `0700`, dan hilang setelah stop bersih. `instance.config.get` mengembalikan `effectiveFiles` kosong dan `drift` kosong untuk instance NATS `1.1.0`.
- **AC-5** (readiness jujur dengan auth, tanpa perubahan probe): instance `1.1.0` menjadi `ready` lewat `nats-readiness` di kedua mode JetStream. Tidak ada file di `crates/core/src/provider/probe/` dan `crates/core/src/supervisor/probe/` yang berubah. Di test in process, selama 60 detik setelah `ready`, `total_connections` di `/varz` naik paling sedikit 3 (probe rutin tetap berjalan) sementara `FakeSecretStore::count(SecretOp::Read)` tidak bertambah (probe tidak pernah membaca secret). Check `honest_readiness`, `identity_check`, `jetstream_mode`, dan `adoption` milik spec 0023 lulus lagi pada paket `1.1.0` (PRD 22.2).
- **AC-6** (dua instance terisolasi, AC-036): dua instance `a` dan `b` dari paket `1.1.0` dengan JetStream aktif, kedua endpoint `Auto`, dan password berbeda. Di masing masing dibuat stream file bernama sama `ORDERS`; `a` menerima 3 pesan `a-<n>` dan `b` 5 pesan `b-<n>`. Setelah `instance.restart` keduanya dan `ready` lagi: `a` melaporkan tepat 3 pesan dengan isi miliknya dan `b` tepat 5; `/varz` di endpoint `monitoring` masing masing melaporkan `server_name` sendiri dan `jetstream.config.store_dir` di bawah folder data sendiri; password `a` ditolak `b` dan sebaliknya; dan `lsof` menunjukkan setiap proses hanya mendengar di dua port efektifnya sendiri, keduanya di `127.0.0.1` (monitoring tidak terbuka ke LAN).
- **AC-7** (durability yang jujur, AC-077 bagian NATS): pada satu instance dibuat stream file `F` dan stream memory `M`, masing masing dengan 10 pesan yang di ack. Setelah `instance.restart`: `F` berisi 10 pesan dan `M` tidak ada lagi (`stream not found`). Setelah `SIGKILL` ke `nats-server` dan instance kembali `ready` (lewat respawn restart policy, atau `instance.start` kalau instance berakhir `failed`): `F` masih ada, dan jumlah pesannya (0 sampai 10) dicatat di `actual` evidence tanpa assertion sama dengan 10. README dan deskripsi field `jetstream` menyatakan hal yang sama: stream file bertahan melewati restart normal, stream memory tidak, dan tulisan terakhir bisa hilang saat mati listrik.
- **AC-8** (bentrok port monitoring, AC-010): test membuat instance dengan endpoint `monitoring` `Fixed` `P` (port dari `allocate_port`, masih bebas saat create) dan `client` `Auto`, lalu memegang `TcpListener` di `127.0.0.1:P`, lalu memanggil `instance.start`. Operasi start berakhir `failed` di stage `endpoints` dengan kode `supervisor.endpoint_in_use` dan detail `endpoint` `monitoring`, `port` `P`, dan `suggestedPort` (port bebas sesudah `P`). Instance tetap `stopped` tanpa proses, `lastFailure` tetap kosong (penolakan sebelum journal, spec 0033), port `monitoring` tetap `P`, dan listener test masih menerima koneksi baru sesudahnya (proses luar tidak dihentikan). Setelah `instance.change_endpoints` memindahkan `monitoring` ke `suggestedPort`, start berikutnya `ready`, dan `/varz` terbaca di port baru itu.
- **AC-9** (batas resource dalam MiB dan KiB, REQ-036): dengan nilai default, `/varz` melaporkan `max_payload` 1048576, `jetstream.config.max_memory` 1073741824, dan `jetstream.config.max_storage` 10737418240. Setelah `instance.config.apply` dengan `maxPayloadKiB` 2048 (dampak restart), `/varz` melaporkan `max_payload` 2097152. Nilai di luar rentang schema ditolak validasi settings yang sudah ada.
- **AC-10** (konformitas): `providers/nats/tests/scenarios.yaml` berisi skenario `default` dan `no-jetstream`. `nats_2_15_passes_conformance` lulus untuk kedua skenario, termasuk langkah `form`, `create` (secret dibangkitkan daemon), `connection` (dua entri, `masked` URI absolut), `second_instance`, `restart`, `leftovers`, dan `start_again`. `cargo xtask provider test providers/nats` keluar 0.
- **AC-11** (journey CLI dan form): `nats_journey` lulus dengan paket `1.1.0`: create dengan password yang dibangkitkan, create dengan JetStream mati, start, `inspect --json`, `connection` (URI ter mask), dan stop. Fixture `protocols/fixtures/instance/create-form-nats.json` ditulis ulang dari paket asli dan memuat `serverName` (managed, terkunci), `jetstream` (toggle), `clientPassword` (secret, basic), dan tiga integer advanced dengan nama dan batas baru; test kontrak Rust dan Swift serta test render form Swift lulus dengan assertion yang diperbarui. Di GUI, sheet New Instance membuat instance NATS dengan password hasil Generate, dan Summary menampilkan Client URL ter mask dengan tombol salin.
- **AC-12** (evidence, catalog, corpus): setiap test native fitur ini memasang guard evidence `native` dengan `covers` `REQ-036`, ditambah `AC-036` (AC-6), `AC-077` (AC-7), dan `AC-010` (AC-8). `nats_compatibility` menulis `compatibility.json` berbentuk *Evidence kompatibilitas* dengan `providerRevision` `1.1.0`. Urutannya mengikuti spec 0023 AC-12: commit paket dan test, run native dari `HEAD` bersih, `cargo xtask runtime compat-record`, lalu satu commit berisi `evidence/compatibility/`, `providers/nats/versions.json` dari `cargo xtask codegen`, ledger catalog, dan golden serta fixture yang memuat status dukungan NATS. Commit paket sendiri sudah membawa kenaikan `revision` dan `published_at` di `providers/catalog.toml` dan entri corpus `crates/core/tests/fixtures/compat/nats/1.1.0/` dari `cargo xtask provider freeze providers/nats`, supaya `cargo xtask provider check` dan test `xtask` lulus di setiap commit. Paket `1.1.0` tidak masuk `main` sebelum commit evidence itu ada. Tanpa artefak di cache, setiap test native `BLOCKED` dengan reason `artifact nats-2.15.1-macos-aarch64 is not in the runtime cache` dan owner `@ojiepermana`, tanpa unduhan.
- **AC-13** (handover): `HANDOVER_PROVEN` tidak berubah (`official/nats` line `1` sudah ada dan mencakup `1.1.0`). Test native launchd `nats_survives_planned_restart_and_sigkill_and_returns_after_kickstart_and_bootout` lulus dengan paket `1.1.0`, dan ID record evidencenya dicatat di `verify.md`. Run itu memuat job launchd di sesi login kamu, jadi ia langkah manual; sampai dijalankan statusnya `BLOCKED` dengan owner `@ojiepermana`. Kalau gagal, build berhenti dan kembali ke `/architect`.
- **AC-14** (ukuran RISK-07): `verify.md` mencatat commit dasar dan daftar commit build. Baris yang ditambah ke kode sumber non test dan memuat `nats` (tanpa peduli huruf besar) hanya ada di allowlist *Ukuran RISK-07*. Kode sumber non test berarti file `.rs` dan `.swift` di luar folder `tests/` dan `Tests/`, di luar modul `#[cfg(test)]`, dan bukan file paket, fixture, golden, atau evidence. `ProtocolVersion::CURRENT` dan nomor migrasi terakhir tidak berubah. `git diff --numstat` untuk setiap baris tabel dicatat, dan selisih dengan tabel kembali ke `/architect`.

Status test semua AC mulai dari `NOT_RUN`. Lulus di fixture bukan evidence native, dan artefak, sesi GUI, atau launchd yang tidak tersedia dicatat `BLOCKED`.

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md), yaitu opsi yang direkomendasikan (user mendelegasikan setiap keputusan ke opsi itu, 2026-11-12):

1. Auth selalu aktif dengan satu user tetap `stackly` dan password dari field secret `clientPassword`.
2. Password sampai ke server lewat `nats.conf` yang seluruhnya diperlakukan sebagai config `sensitive` di folder `secrets/` (mekanisme 5 spec 0011), bukan hash, env, atau include.
3. Readiness memakai user `stackly-readiness` tanpa password di akun NATS tersendiri (`STACKLY_READINESS`), dipasang sebagai `no_auth_user` dengan izin minimum, jadi `nats-readiness` tidak berubah, tidak pernah butuh secret, dan koneksi tanpa credential tidak pernah menyentuh akun aplikasi.
4. Escaper `natsConf` dilonggarkan (escape `"` dan `\`, `$` literal), supaya password apa pun yang bisa dicetak tidak menggagalkan start.
5. Batas resource memakai field MiB dan KiB yang dirender dengan suffix `MiB` dan `KiB`.
6. Revision paket `1.1.0`, tetap di line `1`, tanpa migrasi instance lama.
7. Bentrok port monitoring memakai jalur spec 0033 yang sudah ada, dibuktikan test native NATS, tanpa mekanisme atau `remedies` baru.
8. Test BR-025 tetap `nats_compatibility`, bukan test konformitas; bukti baru masuk sebagai check tambahan di test itu.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Skill async relevan untuk client uji NATS di test (satu deadline per permintaan, tanpa task latar).

## Feature design

### Batas fitur

- **Di dalam**: paket `providers/nats` revision `1.1.0`; escaper `natsConf`; test native auth, isolasi, persistence, durability, dan bentrok port monitoring; skenario konformitas; journey CLI; fixture form dan test yang mengikutinya; evidence BR-025 baru; revision catalog; entri corpus `1.1.0`.
- **Di luar**: TLS, cluster, leafnode, gateway, websocket, MQTT, akun NATS selain `$G`, user tambahan, dan auth NKey atau JWT; reload NATS (spec 0026 mencatat NATS tanpa reload); memindahkan instance `1.0.1` ke `1.1.0`; metrics dari `/jsz` (Follow-up spec 0023 untuk fitur 28); backup dan restore JetStream (fase 08); adoption data lama (fase 09); environment (fitur 57); rename instance saat berjalan (Follow-up spec 0023 untuk fitur 29 dan 30); simulasi mati listrik; dan unduhan runtime.

### Fakta runtime NATS 2.15.1

Direkam sesi architect pada 2026-11-12 dengan `nats-server` dari cache runtime xtask (sha256 `ed0fe392…5ffd`), loopback, tanpa jaringan. Setiap fakta diuji ulang oleh AC yang disebut; kalau satu fakta tidak terulang saat build, build berhenti dan kembali ke `/architect`.

| No | Fakta | Akibat | Diuji ulang |
| --- | --- | --- | --- |
| 1 | Dengan user ber password dan `no_auth_user` (di `authorization.users` maupun di `accounts`), INFO pertama tidak memuat `auth_required`. INFO kedua (sesudah CONNECT, dengan `connect_info`) memuat `auth_required: true` dan datang sesudah `PONG`. | `Exchange` spec 0023 membaca field boolean yang tidak ada sebagai `false` dan mengabaikan INFO di tengah exchange, jadi probe lulus tanpa perubahan. | AC-5 |
| 2 | User di `users` tanpa field `password` diterima server. Koneksi tanpa credential dipetakan ke user itu; login atas namanya dengan password kosong juga diterima, dengan password lain ditolak. | User readiness tidak butuh nilai rahasia sama sekali. | AC-3 |
| 3 | Izin `publish.allow ["$JS.API.INFO"]` dan `subscribe.allow ["_INBOX.stackly.>"]` cukup untuk langkah 7 probe, juga saat user itu ada di akun lain dengan `jetstream: enabled`; publish lain, subscribe `>`, dan `$JS.API.STREAM.CREATE.X` mendapat `Permissions Violation`. `publish.deny ">"` dan `subscribe.deny ">"` masih mengizinkan `PING`. | Izin user readiness di template. | AC-3 |
| 4 | Di string berkutip ganda `nats.conf`, `\"` dan `\\` adalah escape, dan `$` literal (juga di awal nilai dan di `store_dir`). | Escaper AC-2. | AC-2, AC-3 |
| 5 | `password: $VAR` dari environment diparse sebagai nilai config: `12345678`, `true`, dan `1k` membuat server gagal start, dan `x{y}` membuat login gagal diam diam. | Env ditolak sebagai mekanisme (rationale, Keputusan 2). | tidak |
| 6 | `include` dengan path absolut gagal, karena path digabung dengan folder file yang meng include. | Include ditolak sebagai mekanisme. | tidak |
| 7 | Password bcrypt (`$2a$`, `$2b$`, `$2y$`) diterima; login memakan sekitar 75 ms per koneksi pada cost 10. | Dicatat untuk Follow-up hash. | tidak |
| 8 | Suffix `KiB`, `MiB`, `GiB` (dan `MB`, `GB`) biner; `K`, `M`, `G` desimal. `1024KiB` menjadi 1048576 dan `1024MiB` menjadi 1073741824, ditulis menempel pada angka. | Template AC-9. | AC-9 |
| 9 | Port monitoring yang dipakai proses lain membuat server keluar dengan kode 1 dalam sekitar 20 ms dan baris `[FTL] Can't start monitoring: can't listen to the monitor port: … address already in use`; listener luar tidak terpengaruh. | Jalur exit dini spec 0033 tetap berlaku kalau port direbut sesudah tahap `endpoints`. | AC-8 (tahap `endpoints`) |
| 10 | Stream file bertahan melewati `SIGTERM` dan, pada run ini, juga `SIGKILL` (10 dari 10 pesan); stream memory hilang setelah restart. `sync_interval` default 2 menit. | AC-7 tidak menjanjikan angka setelah crash. | AC-7 |
| 11 | Password tidak muncul di `/varz`, `/connz?auth=1&subs=1`, `/jsz`, `/accountz`, `/subsz`, atau log server. | AC-4. | AC-4 |
| 12 | Kalau user readiness satu akun dengan user aplikasi, koneksi tanpa credential bisa mengirim `PUB $JS.API.INFO orders.evil 0`: server mengirim balasannya ke `orders.evil`, dan stream aplikasi yang menangkap `orders.>` menyimpan balasan itu sebagai pesan (1 menjadi 2 pesan). `$JS.API.INFO` juga melaporkan jumlah stream dan pemakaian akun aplikasi. Dengan user readiness di akun sendiri, pesan stream tetap 1 dan ringkasannya `streams` 0. | Akun `STACKLY_READINESS` terpisah (rationale, Keputusan 3). | AC-3 |
| 13 | Password yang berbentuk hash bcrypt (misalnya `$2a$10$abc`) dibaca server sebagai hash, jadi login dengan teks itu ditolak. `$2zzz` dan `x$2a$10$abc` bekerja biasa. | Batas yang dinyatakan di README dan *Edge cases*. | tidak |
| 14 | Susunan `nats.conf` persis *Paket NATS 1.1.0*, di kedua mode JetStream dan dengan password ber escape, diterima server: probe lulus, login `stackly` berhasil, dan `/varz` melaporkan batas default. | Template. | AC-1, AC-3, AC-9 |

### Paket NATS 1.1.0

`provider.yaml`, hanya bagian yang berubah dari `1.0.1` (bagian lain, termasuk `runtime`, `capabilities`, `resources`, `lifecycle.initialize`, `stop`, `readiness`, `timeouts`, `backup`, dan `openWith`, tetap byte demi byte):

```yaml
metadata:
  revision: "1.1.0"
configFiles:
  natsConf:
    template: templates/nats.conf.tmpl
    format: natsConf
    sensitive: true
    destination: "${instance.secrets}/nats.conf"
lifecycle:
  start:
    executable: "${runtime.executable}"
    args: ["--config", "${instance.secrets}/nats.conf"]
    cwd: "${instance.data}"
permissions:
  managedFilesystem: [data, config, logs, temp, run]
  executeRuntimeBinary: true
connection:
  - name: uri
    title: Client URL
    template: "nats://stackly:${secret.clientPassword}@${endpoint.client.address}"
  - name: monitoring
    title: Monitoring URL
    template: "http://${endpoint.monitoring.address}"
```

`templates/nats.conf.tmpl` (kurung kurawal literal selalu dipisah spasi, supaya tidak pernah membentuk tag):

```text
# Written by Stackly from the official/nats provider; edits are replaced.
server_name: {{ settings.serverName }}
listen: {{ endpoint.client.address }}
http: {{ endpoint.monitoring.address }}
max_payload: {{ settings.maxPayloadKiB }}KiB
{{#if settings.jetstream}}
jetstream {
  store_dir: {{ instance.data }}
  max_memory_store: {{ settings.jetstreamMaxMemoryMiB }}MiB
  max_file_store: {{ settings.jetstreamMaxFileMiB }}MiB
}
{{/if}}
accounts {
  APP: {
{{#if settings.jetstream}}
    jetstream: enabled
{{/if}}
    users: [ { user: "stackly", password: {{ secret.clientPassword }} } ]
  }
  STACKLY_READINESS: {
{{#if settings.jetstream}}
    jetstream: enabled
    users: [ { user: "stackly-readiness", permissions: { publish: { allow: ["$JS.API.INFO"] }, subscribe: { allow: ["_INBOX.stackly.>"] } } } ]
{{else}}
    users: [ { user: "stackly-readiness", permissions: { publish: { deny: ">" }, subscribe: { deny: ">" } } } ]
{{/if}}
  }
}
no_auth_user: "stackly-readiness"
```

`schemas/settings.schema.json`: `serverName` dan `jetstream` tetap, kecuali deskripsi `jetstream`. Field lain:

| Field | Tipe dan batas | Default | `x-stackly` | Title dan description |
| --- | --- | --- | --- | --- |
| `jetstream` | boolean | `true` | `apply: restart` | "JetStream" · "Store streams and key value buckets in the instance's data folder. File streams survive a restart, memory streams do not. Off means core messaging only, and nothing is stored." |
| `clientPassword` | string, `minLength` 8, `maxLength` 128 | tidak ada, masuk `required` | `apply: restart`, `secret: true`, `printableOnly: true` (visibility basic) | "Client password" · "Password of the stackly user that clients sign in with. A new value takes effect at the next restart." |
| `jetstreamMaxMemoryMiB` | integer 1 sampai 1048576 | 1024 | `apply: restart`, `visibility: advanced` | "JetStream memory limit (MiB)" · "Most memory JetStream memory streams may use." |
| `jetstreamMaxFileMiB` | integer 1 sampai 1048576 | 10240 | `apply: restart`, `visibility: advanced` | "JetStream file limit (MiB)" · "Most disk space JetStream file streams may use in the data folder." |
| `maxPayloadKiB` | integer 1 sampai 65536 | 1024 | `apply: restart`, `visibility: advanced` | "Largest message (KiB)" · "Largest message payload a client may publish." |

Urutan `properties`: `serverName`, `jetstream`, `clientPassword`, `jetstreamMaxMemoryMiB`, `jetstreamMaxFileMiB`, `maxPayloadKiB`. Field `jetstreamMaxMemoryBytes`, `jetstreamMaxFileBytes`, dan `maxPayloadBytes` dihapus; default dan batas atasnya sama dengan nilai lama dalam satuan baru.

`tests/scenarios.yaml`:

```yaml
schema: stackly.provider-scenarios/v1
scenarios:
  - name: default
  - name: no-jetstream
    settings: { jetstream: false }
```

`README.md` (bahasa Inggris) diganti dan memuat: user `stackly` dan password di Keychain; cara mengganti password (`stackly secret set` untuk reference field itu, lalu restart instance); user `stackly-readiness` dan bahwa client yang lupa credential tetap tersambung tetapi mendapat `Permissions Violation` saat publish atau subscribe, bukan `Authorization Violation` saat connect; monitoring tanpa auth di loopback dan apa yang bisa dibaca di sana; pernyataan durability AC-7; satuan MiB dan KiB; `nats.conf` berada di folder `secrets/` runtime, hanya ada selama instance berjalan, dan tidak tampil sebagai Effective File; instance yang dibuat dari `1.0.1` tetap tanpa auth, dan menduplikasinya menghasilkan instance tanpa auth lagi (duplikat memakai revision sumbernya), jadi buat instance baru; dan password yang berbentuk hash bcrypt (`$2a$`, `$2b$`, `$2x$`, atau `$2y$` diikuti dua digit dan `$`) tidak bisa dipakai, karena NATS membacanya sebagai hash. Bagian "Limits of this version" kehilangan kalimat tanpa auth dan tetap memuat store penuh yang tidak terdeteksi readiness.

Tidak ada blok `remedies` (rationale, Keputusan 7). `LICENSE-NOTICES.md` dan `versions.json` (selain record compatibility baru dari codegen) tidak berubah.

### Escaper `natsConf`

Perubahan di `TemplateFormat::escape_text`, arm `NatsConf`, di `crates/core/src/provider/template.rs`:

| Karakter di nilai | Sebelum | Sesudah |
| --- | --- | --- |
| `"` | ditolak `unrepresentable_value` | `\"` |
| `\` | ditolak | `\\` |
| `$` | ditolak | keluar apa adanya |
| karakter kontrol (C0, DEL, C1) | ditolak | tetap ditolak |
| karakter lain, termasuk spasi, `#`, `{`, `}`, `'`, dan non ASCII | keluar apa adanya | sama |

Hasilnya tetap satu string berkutip ganda. Integer dan boolean tidak berubah. Ini perubahan additive menurut *Kebijakan kompatibilitas* spec 0036: render setiap paket yang sudah valid tidak berubah (nilai yang dulu ditolak sekarang diterima), jadi tidak ada `frozen.json` yang berubah. Test `a_quote_in_the_data_folder_fails_the_nats_plan_without_the_value` diganti dua test: tanda kutip di folder data sekarang dirender sebagai `\"`, dan karakter kontrol tetap gagal tanpa menyebut nilainya.

### Model auth

| User | Akun NATS | Password | Izin | Dipakai oleh |
| --- | --- | --- | --- | --- |
| `stackly` | `APP` (JetStream `enabled` saat `jetstream` aktif) | nilai secret `clientPassword` | penuh di akun `APP` | aplikasi kamu, lewat Client URL |
| `stackly-readiness` | `STACKLY_READINESS` (JetStream `enabled` saat `jetstream` aktif, tanpa stream) | tidak ada | JetStream aktif: publish `$JS.API.INFO`, subscribe `_INBOX.stackly.>`. JetStream mati: tidak ada publish atau subscribe | probe `nats-readiness`, dan koneksi apa pun tanpa credential (`no_auth_user`) |

- Dua akun NATS memisahkan ruang subject: apa pun yang dikirim atau dipicu koneksi tanpa credential, termasuk balasan API JetStream ke subject pilihan pengirim, tinggal di akun `STACKLY_READINESS` dan tidak pernah sampai ke subject atau stream akun `APP` (fakta 12). `$JS.API.INFO` di akun readiness tetap dijawab API JetStream server yang sama, jadi langkah 7 probe tetap bermakna.
- Batas `max_memory_store` dan `max_file_store` adalah batas server; akun `APP` memakainya tanpa batas akun tambahan (`jetstream: enabled`).
- Nama user dan nama akun literal di template, dan `stackly` juga di template `connection`; tidak ada field settings untuk nama user, karena `connection.template` menolak `settings.*`.
- Mengganti password: `SecretService` spec 0011 menyimpan nilai baru di reference yang sama, dan start berikutnya merender `nats.conf` dari nilai itu. Tidak ada config revision baru, karena settings hanya menyimpan reference.
- Setiap start membaca Keychain sekali lewat `resolve_for_start` (interaksi `AllowPrompt` untuk aksi pengguna, `Never` untuk autostart dan respawn). Keychain yang terkunci saat autostart memberi blocker `NeedsSecret` yang sudah ada (spec 0011), bukan start tanpa auth.

### Data model

Tidak ada tabel, kolom, migrasi, DTO, method IPC, event, atau kode error baru. Untuk instance NATS `1.1.0`:

| Data | Tempat | Isi |
| --- | --- | --- |
| provider revision | registry | `official/nats@1.1.0#<sha>` |
| config revision | registry (`config_revisions`) | `jetstream`, tiga batas, dan reference secret `clientPassword`; `serverName` dihitung saat plan |
| password | Keychain (atau file keychain di root override) | reference `local:instance/<id>/client-password` (purpose turunan spec 0011) |
| config | `<run instance>/secrets/nats.conf`, `0600` | ditulis sebelum setiap spawn, dihapus setelah stop bersih, disapu saat daemon start untuk instance yang tidak berjalan |
| store JetStream | `<data instance>/jetstream/` | milik NATS |

### API surface

Tidak ada method baru. Method yang dipakai dan bagian yang relevan:

| Method | Dipakai untuk | Milik |
| --- | --- | --- |
| `instance.get_create_form`, `instance.preflight`, `instance.create` | form dengan field secret, create dengan `SecretSource::Generate` atau nilai | spec 0020 |
| `instance.start`, `instance.restart`, `instance.stop` | lifecycle; kegagalan `supervisor.endpoint_in_use` dengan `endpoint` dan `suggestedPort` | spec 0021, 0026, 0033 |
| `instance.change_endpoints` | memindahkan port monitoring | spec 0029 |
| `instance.get_detail` | entri `connection` ter mask, reveal dan salin | spec 0021 |
| `instance.config.get`, `instance.config.apply` | settings MiB dan KiB; `effectiveFiles` kosong | spec 0029 |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| render `nats.conf` | `server_name` | `settings.serverName`, managed dari `${instance.name}` |
| render | alamat `listen` dan `http` | endpoint `client` dan `monitoring` dengan port yang dipilih tahap `endpoints` start |
| render | `max_payload`, `max_memory_store`, `max_file_store` | `settings.maxPayloadKiB`, `jetstreamMaxMemoryMiB`, `jetstreamMaxFileMiB` dari config revision aktif, dengan suffix literal di template |
| render | `store_dir` | `${instance.data}` |
| render | password user `stackly` | `ResolvedSecrets` dari `resolve_for_start`, field `clientPassword` |
| render | nama user, nama akun, `jetstream: enabled` per akun, izin readiness, `no_auth_user` | literal di template, dipilih `settings.jetstream` |
| create | nilai password | dibangkitkan daemon (`generate_password`, `[A-Za-z0-9]`) atau diketik pengguna (spec 0020) |
| probe | `serverName`, `jetstream` | param probe dari plan (spec 0023); tidak ada credential |
| `connection` `uri` | user, password, alamat | literal `stackly`, `${secret.clientPassword}` (di percent encode engine), `${endpoint.client.address}` |
| start gagal | detail `endpoint`, `port`, `suggestedPort` di error operasi | tahap `endpoints` dan `suggest_for_start` (spec 0033); `lastFailure` tidak diisi untuk penolakan ini |
| AC-5 | jumlah koneksi probe | `total_connections` di `/varz`, dibaca saat `ready` dan 60 detik kemudian |
| test native | password instance | nilai fixture yang dikirim test lewat `SecretInput::Value` saat create (`stackly-test-nats-a`, `stackly-test-nats-b`, dan literal Rust `"p\"q\\r$s-fixture"` AC-3) |
| `compatibility.json` | `version`, `digest`, `manifestSha256` | `/varz` server berjalan, `ArtifactRecord.sha256`, record `pin_package` (spec 0023) |
| `actual` AC-7 | jumlah pesan setelah crash | `$JS.API.STREAM.INFO.F`, field `state.messages` |

### Test native

Semua di `apps/daemon/tests/nats/`, bertanda `#[ignore = "native: needs the managed NATS 2.15.1 artifact in the runtime cache"]`, dengan `TestRoot`, port dari daemon (`Auto`) atau `allocate_port`, dan guard yang me reap proses.

| Test atau check | Daemon | Membuktikan |
| --- | --- | --- |
| `nats_auth_in_process` (file baru `auth.rs`) | in process, `FakeSecretStore` | AC-3 (termasuk percobaan suntik balasan), AC-4, AC-5 (jumlah `read` secret dan `total_connections`), password dengan `"`, `\`, `$` |
| `nats_lifecycle_in_process`, `nats_applies_a_setting…`, `lifecycle_ops`, `metrics` (disesuaikan) | in process | AC-9, dan lokasi baru `nats.conf` |
| `nats_compatibility`, check baru `client_auth` | `SpawnedDaemon` | AC-3 lewat daemon biner (file keychain root override) |
| check baru `isolation` | `SpawnedDaemon` | AC-6 |
| check baru `persistence` | `SpawnedDaemon` | AC-6 (restart), AC-7 |
| check baru `monitoring_conflict` | `SpawnedDaemon` | AC-8 |
| check lama `init`, `start`, `readiness`, `honest_readiness`, `identity_check`, `jetstream_mode`, `adoption`, `stop` | `SpawnedDaemon` | AC-5 |
| `nats_2_15_passes_conformance` (tidak diubah) | harness | AC-10 |
| `nats_journey` (`apps/cli/tests/nats_journey.rs`) | `SpawnedDaemon` | AC-11 bagian CLI |

- **Client uji** `apps/daemon/tests/nats/client.rs`: kode test saja, tanpa dependency baru, berbicara protokol client NATS mentah lewat `tokio::net::TcpStream`: `connect(addr, Option<(user, pass)>)` yang mengembalikan baris pertama sesudah `CONNECT` dan `PING`, `request(subject, payload)` dengan inbox `_INBOX.test.<hex>` (sengaja di luar ruang inbox probe) dan satu deadline 5 detik, `publish_with_reply(subject, reply)` mentah untuk percobaan suntik AC-3, dan pembungkus `stream_create(name, subject, storage)`, `publish(subject, payload)` (menunggu ack JetStream), `stream_info(name)`, dan `stream_get(name, seq)`. Ia tidak dipakai kode produk.
- Harness `create_params` selalu mengirim `clientPassword` sebagai `SecretInput::Value` nilai fixture, dan mendapat helper path `<run instance>/secrets/nats.conf`.
- Server asing AC-10 spec 0023 tetap memakai `allocate_port` dan argv yang sama.
- `SIGKILL` di AC-7 hanya dikirim ke PID `nats-server` yang dimulai daemon test itu.

### Evidence kompatibilitas

```json
{
  "schema": "stackly.compatibility-evidence/v1",
  "providerRevision": { "id": "official/nats", "revision": "1.1.0", "manifestSha256": "<hex>" },
  "checks": ["adoption", "client_auth", "honest_readiness", "identity_check", "init", "isolation", "jetstream_mode", "monitoring_conflict", "persistence", "readiness", "start", "stop"],
  "knownLimitations": ["power_loss_not_covered"]
}
```

Tiga kode lama (`client_auth_not_configured`, `jetstream_persistence_not_covered`, `multi_instance_not_covered`) hilang karena sekarang terbukti. `power_loss_not_covered` masuk karena tidak ada simulasi mati listrik di Mac ini (AC-077 "simulation yang tersedia"). Evidence `1.0.0` dan `1.0.1` tetap sebagai riwayat.

### Ukuran RISK-07

| No | Perubahan | Jenis | Tempat | Dipakai provider lain |
| --- | --- | --- | --- | --- |
| 1 | Escaper `natsConf` menerima `"`, `\`, dan `$` | generik (format config), additive | `crates/core/src/provider/template.rs`, satu arm `match` dan unit testnya | ya, setiap paket ber format `natsConf` |

Jumlah perubahan core: **satu**, generik. Perubahan core khusus NATS: **nol**. Probe, plan, supervisor, secret, storage, protokol, daemon, CLI, dan platform tidak disentuh. Pembanding: fitur 23 butuh satu perluasan generik (probe `native`) dan satu perubahan khusus NATS (`nats-readiness`).

**Allowlist AC-14** (file sumber non test yang boleh menerima baris baru yang memuat `nats`): `crates/core/src/provider/template.rs`, dan `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Fixtures/FixtureConfiguration.swift` (data fixture tampilan yang memuat nama field settings NATS).

### File bersama yang disentuh

Daftar persis dari file bersama brief umum scope 07, dan file lain yang juga dipakai provider lain:

| File | Disentuh | Catatan |
| --- | --- | --- |
| `providers/catalog.toml`, `providers/catalog-revisions.json` | ya | `revision` berikutnya saat commit, `published_at` baru, `cargo xtask catalog check --update` |
| `crates/core/src/provider/template.rs` (modul format config) | ya | satu arm; fitur 40, 42, dan 43 menambah format di file yang sama |
| `crates/core/tests/provider_packages.rs` dan golden NATS | ya | test NATS, dan test bersama yang membangun setiap paket dengan settings `{}` (sekitar baris 986) mendapat nilai secret NATS |
| `crates/core/tests/config_evaluate.rs`, `crates/core/tests/connection_render.rs`, `crates/core/tests/golden/provider/nats/connection.json` | ya | settings `{}` NATS sekarang butuh secret; golden `connection` NATS memuat user dan secret ter mask |
| `crates/core/src/secret/delivery/mod.rs` | ya | hanya modul `#[cfg(test)]` nya: test yang hari ini menuntut `natsConf` menolak `a"b` |
| `xtask/tests/provider_check.rs` | ya | assertion `ok official/nats 1.0.1` dan test `provider freeze providers/nats` terhadap corpus repo |
| `protocols/fixtures/` dan golden CLI `provider_list.json`, `provider_inspect_nats.*` | ya | `instance/create-form-nats.json`, `instance/commands.json`, `ipc/frame-response-instance-get-create-form-unknown-values.json`, dan setiap fixture catalog yang memuat revision atau digest NATS |
| `crates/protocol/tests/create_fixtures.rs` | ya | test saja, nama field settings NATS |
| `fuzz/seeds/` | ya | lewat `cargo xtask fuzz seed` |
| `apps/daemon/tests/native_launchd.rs` | mungkin | hanya kalau helper harness NATS berubah tanda tangan |
| `runtimes/inventory.toml` | tidak | artefak NATS sama |
| `xtask/src/check_deps/source_scan.rs` (`PROVIDER_WORDS`) | tidak | `nats` sudah tercantum |
| `protocols/fixtures/instance/index.json` | tidak | entri NATS sudah ada |
| `apps/daemon/tests/provider_conformance.rs` | tidak | skenario dibaca dari paket |
| `crates/core/src/supervisor/handover.rs` (`HANDOVER_PROVEN`) | tidak | line `1` sudah ada |
| `crates/protocol/src/` | tidak | tidak ada perubahan kontrak |
| modul probe dan hash di `crates/core` | tidak | |

Nomor protokol dan migrasi: fitur ini tidak butuh keduanya. Kalau `cargo xtask codegen --check` ternyata melaporkan digest kontrak berubah, pakai nomor bebas berikutnya, ditetapkan lead saat build.

### Edge cases dan kegagalan

- **Password dengan karakter kontrol**: ditolak saat diisi oleh `printableOnly` (spec 0011, 0019), jadi tidak pernah sampai ke render.
- **Keychain terkunci atau nilai hilang saat start**: `secret.needs_secret` dan blocker `NeedsSecret` (spec 0011); instance tidak start tanpa auth.
- **Respawn setelah crash saat Keychain terkunci**: respawn memakai interaksi `Never`; tanpa nilai secret, percobaan itu gagal `config_render_failed` dengan blocker `NeedsSecret`, dan instance tidak pernah dijalankan dengan config lama atau tanpa auth. Itu perilaku spec 0010 dan 0011 yang sudah ada, tidak diubah di sini.
- **Password berbentuk hash bcrypt**: diterima saat diisi, tetapi login dengan teks itu ditolak NATS (fakta 13). Field secret tidak punya `pattern`, jadi ini batas yang dinyatakan di README; password yang dibangkitkan Stackly hanya `[A-Za-z0-9]` dan tidak pernah terkena.
- **Duplikat instance `1.0.1`**: memakai revision sumbernya, jadi tetap tanpa auth; README mengarahkan ke create baru.
- **Client tanpa credential**: tersambung sebagai `stackly-readiness` di akun `STACKLY_READINESS`, lalu `Permissions Violation` di publish atau subscribe pertama. README menjelaskannya.
- **Port monitoring direbut sesudah tahap `endpoints`**: server keluar dini (fakta 9), dan jalur exit dini spec 0033 melaporkan `endpoint_in_use`. Tidak ada test native untuk balapan ini, karena tidak bisa dibuat pasti; test fake server spec 0033 mencakupnya.
- **Instance `1.0.1` yang sudah ada**: tetap memakai paket pinned nya, tanpa auth, dengan `nats.conf` di folder config. Escaper baru tidak mengubah render nya (corpus).
- **Planned daemon restart**: proses dipertahankan (line `1`), `nats.conf` di `secrets/` tidak disapu karena instance berjalan, dan NATS tidak membaca ulang config.
- **Stop paksa atau crash daemon**: `nats.conf` bisa tertinggal di `secrets/` sampai sweep saat daemon start berikutnya (aturan spec 0011 yang sudah ada).
- **Store JetStream penuh**: readiness tetap `ready` (spec 0023, fakta 3); tidak berubah.
- **Server asing bernama sama di port yang sama**: tetap tidak terbedakan oleh probe (residual spec 0023).

### Asumsi pada spec lain

- Spec 0009 dan 0011: config `sensitive` di `${instance.secrets}` dirender ulang sebelum setiap spawn dan dihapus setelah stop bersih; `{{ secret.<field> }}` di template `sensitive` memakai escaper format itu.
- Spec 0020: form create dan `instance.create` sudah menangani field secret (`Generate` atau nilai), seperti PostgreSQL.
- Spec 0021: `connection.template` menerima `secret.<field>` dan mem percent encode nilainya.
- Spec 0033 AC-4: tahap `endpoints` memeriksa setiap endpoint, termasuk `monitoring`, dan menyebut namanya di detail error.
- Spec 0036: harness membaca skenario dari paket dan membangkitkan secret; spike dari paket (`PackageRecipe`) memberi nilai fixture untuk setiap field secret dan menulis langkah render.
- Root override memakai file keychain (`SecretStoreTarget::File`), jadi test daemon biner tidak pernah menyentuh login Keychain.

Kalau satu asumsi ternyata salah saat build, build berhenti di langkah itu dan kembali ke `/architect`.

### Jawaban follow-up dari spec lain

| Sumber | Baris | Jawaban |
| --- | --- | --- |
| Spec 0023 | auth client lewat secret dan template `sensitive`, termasuk credential di `uri` | AC-1, AC-3, AC-4; *Model auth* |
| Spec 0023 | `nats-readiness` dengan auth tanpa membaca Keychain tiap 10 detik | AC-5; user `stackly-readiness` lewat `no_auth_user`, probe tidak berubah |
| Spec 0023 | dua instance terisolasi, persistence setelah restart | AC-6, AC-7 |
| Spec 0023 | bentrok port monitoring | AC-8 |
| Spec 0023 | hapus `knownLimitations` yang terbukti | AC-12, *Evidence kompatibilitas* |
| Spec 0023 | satuan MiB setelah suffix dibuktikan | fakta 8, AC-9 |
| Spec 0023 | residual risk tanpa auth | tertutup untuk instance `1.1.0`; residual baru di *Security model* |
| Spec 0021 | credential NATS di template `uri` | AC-1 |
| Spec 0030 | test native launchd sebelum `HANDOVER_PROVEN` | NATS sudah masuk sejak 2026-11-02; AC-13 menjalankan ulang test itu pada `1.1.0` |
| Spec 0036 | paket lewat `provider new`, `PROVIDER_WORDS`, fixture form, test konformitas per line, `provider freeze`, dan keputusan test BR-025 | paket dan entri sudah ada sejak fitur 23 dan 36; fixture form ditulis ulang (AC-11); skenario ditambah (AC-10); freeze `1.1.0` (AC-12); test BR-025 tetap `nats_compatibility` (Keputusan 8) |
| Spec 0017 | aturan masuk catalog | NATS sudah `[[provider]]`; fitur ini hanya menaikkan revision catalog (AC-12) |
| Spec 0034 | remedy untuk provider lain | tidak ada remedy NATS di `1.1.0` (rationale, Keputusan 7) |

### Usulan update spec lain

Spec ini tidak mengubah spec lain. Lead membawa butir ini ke pemiliknya setelah build:

1. **Spec 0009**: baris `natsConf` di tabel escaper *Template config* menjadi aturan AC-2; satu baris di tabel *Perluasan additive*; catatan rationale "escaper `natsConf` konservatif" ditutup dengan rujukan ke fakta 4.
2. **Spec 0023**: centang dua baris Follow-up fitur 39 (auth dan lain lain, serta satuan MiB) dengan rujukan ke spec ini.
3. **Spec 0021**: centang Follow-up "Fitur 39: credential NATS di template `uri`".
4. **Spec 0030, 0036, 0017, 0034**: catat bagian fitur 39 di baris Follow-up masing masing sebagai terjawab; baris itu tetap terbuka untuk fitur 40 sampai 43.

### Key invariants

- Server NATS `1.1.0` tidak pernah berjalan tanpa `authorization`; start tanpa nilai password gagal dengan `NeedsSecret`.
- Nilai password hanya ada di Keychain dan di `<run>/secrets/nats.conf` selama instance berjalan; tidak pernah di argv, env, folder data, folder config, `config-history`, log, plan, event, atau golden.
- Probe tidak pernah mengirim atau menerima secret, dan tidak pernah memicu bacaan Keychain.
- Koneksi tanpa credential hidup di akun `STACKLY_READINESS` dan tidak pernah bisa membaca atau menulis subject akun `APP`, membuat stream, membaca pesan stream, atau membuat server menulis ke akun `APP` atas namanya.
- Setiap instance memakai `store_dir` di bawah folder datanya sendiri dan `server_name` sama dengan nama instance.
- Kedua listener selalu di loopback.
- Satu `metadata.revision` yang sudah terbit selalu menunjuk satu isi; `1.0.1` tidak diubah.
- Kegagalan start karena port tidak pernah menghentikan proses luar dan tidak pernah mengganti port sendiri (spec 0033).

### Security model

- **Yang dijaga**: proses lokal lain (juga milik user lain di Mac ini) yang mencoba membaca atau menulis pesan dan stream tanpa password; password yang bocor lewat argv, env, log, monitoring, atau file yang ikut backup; dan probe yang butuh secret.
- **Residual yang dinyatakan**:
  - Koneksi tanpa credential bisa `PING` dan, saat JetStream aktif, membaca ringkasan akun `STACKLY_READINESS` yang selalu kosong, serta berlangganan `_INBOX.stackly.>` di akun itu. Ia tidak melihat apa pun dari akun `APP`.
  - Endpoint monitoring NATS tidak punya auth: proses lokal bisa membaca `/varz`, `/connz`, `/jsz`, dan `/subsz` (statistik, nama stream, subject langganan, nama user), tidak pernah password atau isi pesan. Listener nya hanya di loopback.
  - Selama instance berjalan, proses lain milik user yang sama bisa membaca `nats.conf` di `secrets/` (residual mekanisme 5 spec 0011).
  - Password berbentuk hash bcrypt tidak bisa dipakai (fakta 13).
  - Instance `1.0.1` yang sudah ada, dan duplikatnya, tetap tanpa auth.
- Tidak ada `unsafe` baru dan tidak ada dependency baru (client uji memakai `tokio` dan `serde_json` yang sudah ada di dev dependency `stacklyd`).

### Configuration required

Tidak ada variabel environment, credential, atau port baru untuk daemon dan CLI. Test native butuh artefak `nats-2.15.1-macos-aarch64` (sha256 `ed0fe39222ec2e317c4d0e1a9cdc16c63ec523d94f9355d24a902a4db2655ffd`) di cache runtime xtask; per 2026-11-12 file itu ada di Mac ini. Tidak ada test yang mengunduh dari jaringan.

### Critical test scenarios

- Escaper: setiap baris tabel *Escaper `natsConf`*, segmen secret lewat `materialize`, corpus tidak berubah. Memverifikasi **AC-2**.
- Paket: `provider check`, golden plan dan render (tanpa nilai secret), render tanpa JetStream memuat izin `deny`, `init_plan` tetap hanya `EnsureDirectories`. Memverifikasi **AC-1**.
- Auth: login benar, password salah, tanpa credential dengan setiap batas izin di kedua mode JetStream, percobaan suntik balasan ke subject aplikasi, ringkasan akun readiness yang kosong, password dengan `"`, `\`, `$`. Memverifikasi **AC-3**, **AC-2**.
- Kebocoran: pencarian byte password di argv, env, folder instance, log store, halaman monitoring, dan `masked`; mode file dan folder; file hilang setelah stop. Memverifikasi **AC-4**.
- Readiness: `ready` di kedua mode, nol `read` secret selama 60 detik sementara `total_connections` naik, check spec 0023 lulus lagi. Memverifikasi **AC-5**.
- Isolasi dan persistence: dua instance, stream bernama sama, restart, jumlah dan isi pesan, password silang, `lsof`. Memverifikasi **AC-6**.
- Durability: stream file dan memory melewati restart, lalu `SIGKILL`, dengan jumlah yang dicatat. Memverifikasi **AC-7**.
- Bentrok port: create dengan port bebas, listener test merebut port monitoring, start gagal di stage `endpoints` dengan detail `monitoring`, instance tetap `stopped` tanpa `lastFailure`, listener tetap hidup, pindah port lalu `ready`. Memverifikasi **AC-8**.
- Batas resource: `/varz` untuk default dan setelah apply. Memverifikasi **AC-9**.
- Konformitas dan journey: dua skenario lewat harness, CLI, fixture form, test Swift, lalu pemeriksaan GUI manual. Memverifikasi **AC-10**, **AC-11**.
- Evidence dan catalog: cache kosong `BLOCKED`; run bersih menulis `compatibility.json` yang lolos `compat-record`; `catalog check` dan `provider check` lolos setelah freeze. Memverifikasi **AC-12**.
- Handover dan RISK-07: test launchd manual, lalu baris yang ditambah per commit terhadap allowlist. Memverifikasi **AC-13**, **AC-14**.

## Build plan

Urutan mengikuti pendekatan Journey proyek: jalur "dua instance NATS yang aman dan terisolasi, dari create sampai restart" dibuat lengkap, termasuk kegagalan port dan bukti, sebelum pekerjaan lain. Milestone 1 menyiapkan paket dan semua yang terbukti tanpa server asli, milestone 2 membuktikan jalur itu pada NATS 2.15.1 lewat daemon, dan milestone 3 merekam evidence dan menutup catalog.

**Milestone 1: Paket `1.1.0` dan escaper, di fixture**

1. Escaper: arm `NatsConf` di `crates/core/src/provider/template.rs` sesuai *Escaper `natsConf`*, unit test per baris tabel dan untuk segmen secret (test penolakan lama di `template.rs` sekitar baris 1191 dan di modul test `crates/core/src/secret/delivery/mod.rs` sekitar baris 150 berubah menjadi test escape), dua test pengganti di `crates/core/tests/provider_packages.rs`, `cargo test -p stackly-core --test provider_compat`, dan `cargo xtask fuzz run provider_template --time 600` (seed target itu ditulis `cargo xtask fuzz seed` dari template paket, jadi tidak ada seed tulisan tangan; tanpa toolchain nightly: `BLOCKED`, owner `@ojiepermana`). Langkah ini boleh masuk `main` sendiri. Memenuhi **AC-2** (bagian fixture).
2. Paket, dalam satu commit: `providers/nats/provider.yaml`, `schemas/settings.schema.json`, `templates/nats.conf.tmpl`, `tests/scenarios.yaml`, dan `README.md` sesuai *Paket NATS 1.1.0*. Lalu, di commit yang sama: golden NATS (`STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test provider_packages`; test NATS di file itu menyesuaikan nama field dan lokasi `nats.conf`, menambah render tanpa JetStream dengan izin `deny`, dan test bersama yang membangun setiap paket dengan settings `{}` memberi nilai secret NATS lewat `ResolvedSecrets::for_tests`); `crates/core/tests/config_evaluate.rs`, `crates/core/tests/connection_render.rs`, dan `crates/core/tests/golden/provider/nats/connection.json`; `STACKLY_UPDATE_GOLDEN=1 cargo test -p stacklyd --test create_form_fixtures` (dan `duplicate_form_fixtures` kalau memuat NATS); setiap fixture dan golden di luar `docs/` dan `evidence/` yang `git grep -l 8e3060924c35` temukan (digest `1.0.1`) atau yang memuat `providerRevision` atau status dukungan NATS, termasuk `protocols/fixtures/instance/commands.json`, `protocols/fixtures/ipc/frame-response-instance-get-create-form-unknown-values.json`, `crates/protocol/tests/create_fixtures.rs`, dan golden CLI `provider_list.json` dan `provider_inspect_nats.*` (sampai langkah 8, NATS 2.15.1 tampil `experimental`, karena record `provider` untuk `1.1.0` belum ada); kenaikan `revision` dan `published_at` di `providers/catalog.toml` dengan `cargo xtask catalog check --update`; `cargo xtask provider freeze providers/nats` (entri `compat/nats/1.1.0/`) dan `xtask/tests/provider_check.rs` (baris `ok official/nats 1.1.0` dan test freeze); `cargo xtask fuzz seed`; `cargo xtask provider check`; dan `cargo xtask codegen --check`. Selama entri `1.1.0` belum ada di `main`, perubahan paket di langkah 4 sampai 7 menulis ulang entri itu (hapus foldernya lalu freeze lagi) di commit yang sama dengan perubahannya; sesudah di `main`, isi paket hanya berubah dengan revision baru. Memenuhi **AC-1**, **AC-9** (schema), **AC-10** (file skenario), **AC-11** (fixture dan test kontrak Rust), **AC-12** (catalog dan corpus).
3. Swift: `clients/swift/Tests/StacklyClientTests/Contract/CreateContractTests.swift`, `apps/macos/Packages/StacklyUI/Tests/StacklyUITests/CreateInstance/SchemaFormModelTests.swift`, `ProviderFactsTests.swift`, dan `Sources/StacklyUI/Fixtures/FixtureConfiguration.swift` mengikuti nama field, field secret, revision, dan digest baru; `swift test` kedua paket. Tidak ada kode view atau model yang berubah. Memenuhi **AC-11** (bagian Swift).

**Milestone 2: NATS 2.15.1 asli lewat daemon**

4. Harness dan client uji: `apps/daemon/tests/nats/harness.rs` (`create_params` dengan `clientPassword` fixture, helper path `secrets/nats.conf`), file baru `apps/daemon/tests/nats/client.rs`, dan penyesuaian `lifecycle.rs`, `config.rs`, `lifecycle_ops.rs`, `metrics.rs`, serta `apps/daemon/tests/native_launchd.rs` kalau tanda tangan helper berubah. Memenuhi **AC-9**, **AC-4** (lokasi dan mode file, file hilang setelah stop).
5. `nats_auth_in_process` di file baru `apps/daemon/tests/nats/auth.rs` (dideklarasikan di `apps/daemon/tests/nats.rs`): login, batas izin di kedua mode, password dengan `"`, `\`, `$`, pencarian byte password, catatan `read` `FakeSecretStore`, `/connz`, dan `instance.config.get`. Memenuhi **AC-3**, **AC-4**, **AC-5**, **AC-2** (bagian native).
6. `nats_compatibility` di `apps/daemon/tests/nats/compatibility.rs`: check `client_auth`, `isolation`, `persistence`, dan `monitoring_conflict` ditambah berurutan sesudah check lama yang relevan dan sebelum `stop`, masing masing masuk daftar hanya setelah assertion nya lulus; `covers` guard dan isi `compatibility.json` sesuai AC-12. Memenuhi **AC-5** (check lama), **AC-6**, **AC-7**, **AC-8**, **AC-12** (bagian test).
7. Konformitas dan CLI: jalankan `nats_2_15_passes_conformance` dan `cargo xtask provider test providers/nats`; sesuaikan `apps/cli/tests/nats_journey.rs` (create dengan secret menurut sintaks spec 0020, perintah `connection`). Memenuhi **AC-10**, **AC-11** (bagian CLI).

**Milestone 3: Evidence, catalog, corpus, dan ukuran RISK-07**

8. Urutan AC-12: dari `HEAD` bersih jalankan `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test nats --locked -- --ignored`, lalu `cargo xtask runtime compat-record <report.json>`, lalu satu commit berisi folder evidence baru, `providers/nats/versions.json` dari `cargo xtask codegen`, golden dan fixture yang memuat status dukungan atau record compatibility NATS (yang langkah 2 tulis sebagai `experimental` kembali `supported`), dan `providers/catalog-revisions.json` dari `cargo xtask catalog check --update --amend` (selama revision langkah 2 belum di `main`; kalau sudah, revision berikutnya dan `published_at` baru di `providers/catalog.toml` dengan `--update`). Entri corpus tidak berubah di sini, karena `manifest_sha256` tidak memuat `versions.json`. Memenuhi **AC-12**.
9. Catatan `verify.md`: commit dasar, daftar commit build, baris yang ditambah per commit terhadap allowlist, `git diff --numstat` baris tabel RISK-07, dan langkah manual (pemeriksaan GUI AC-11 dan run launchd AC-13) di bagian `### UI / manual`. Memenuhi **AC-13**, **AC-14**, **AC-11** (bagian GUI).

**Prasyarat**:

- Semua fitur prasyarat sudah `done` di `main`: 9, 10, 11, 17, 18, 20, 21, 23, 26, 29, 33, dan 36.
- Langkah 2 butuh langkah 1 (render dengan password yang memuat karakter khusus dipakai test paket). Langkah 3 butuh fixture langkah 2.
- Langkah 4 sampai 7 butuh langkah 2 dan artefak NATS di cache runtime xtask (selain itu `BLOCKED`).
- Langkah 8 butuh langkah 2 sampai 7 ter commit dan `HEAD` bersih. Commit langkah 2 sampai 8 masuk `main` bersama: paket `1.1.0` tanpa record evidence menurunkan status NATS ke `experimental` di catalog, dan perubahan paket sesudah terbit butuh revision baru.
- Langkah 9 bagian launchd dan GUI butuh kamu (manual).

**Langkah manual** (bukan untuk sesi developer; masuk file manual lead):

- Run launchd AC-13: `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test native_launchd -- --ignored --test-threads 1 nats_survives`, di sesi login GUI.
- Pemeriksaan GUI AC-11: buat instance NATS lewat sheet New Instance dengan Generate, start, lalu lihat Client URL ter mask dan tombol salin di Summary.

**Sesi paralel**: di dalam fitur ini, satu sesi developer untuk langkah 1, 2, dan 4 sampai 9 adalah pilihan yang jujur, karena langkah itu berbagi `providers/nats/`, fixture, dan `apps/daemon/tests/nats/`. Langkah 3 (Swift) bisa sesi kedua setelah commit langkah 2, karena hanya menyentuh `clients/swift/Tests/` dan `apps/macos/Packages/StacklyUI/`. Terhadap fitur 40 sampai 43, fitur ini berbagi lima hal dan tidak aman berjalan bersamaan pada langkah yang menyentuhnya: `crates/core/src/provider/template.rs` (langkah 1; fitur 40, 42, dan 43 menambah format di file itu), `crates/core/tests/provider_packages.rs` (langkah 1 dan 2), `providers/catalog.toml` dan ledger nya (langkah 2 dan 8), fixture catalog di `protocols/fixtures/` beserta golden CLI daftar provider (langkah 2 dan 8; setiap provider yang pindah ke `[[provider]]` juga menulis ulang fixture itu), dan `xtask/tests/provider_check.rs` (langkah 2). Saran urutannya: langkah 1 masuk `main` lebih dulu sebagai commit sendiri, supaya `template.rs` bebas untuk provider lain; langkah 2 sampai 8 masuk bersama dalam satu giliran catalog yang diatur lead, dan provider lain rebase sesudahnya. Nomor revision catalog selalu diambil saat rebase terakhir sebelum merge.

## Consequences

**Positive**:

- Instance NATS baru tidak lagi terbuka untuk setiap proses lokal; residual tanpa auth dari spec 0023 tertutup.
- Readiness tetap memeriksa identitas, mode JetStream, `PONG`, dan API JetStream, tanpa satu baris kode probe baru dan tanpa bacaan Keychain berkala.
- AC-036, AC-010, dan bagian NATS dari AC-077 mendapat evidence native, dan `knownLimitations` NATS berkurang dari tiga menjadi satu.
- RISK-07 untuk NATS lengkap: satu perubahan core generik kecil, nol perubahan khusus NATS.
- Form menampilkan batas dalam MiB dan KiB, bukan angka byte panjang.
- Path home atau data yang memuat `"`, `\`, atau `$` sekarang bisa dipakai paket ber format `natsConf`.

**Negative / tradeoffs**:

- `nats.conf` pindah ke folder `secrets/`, jadi instance NATS `1.1.0` tidak punya Effective File, salinan `config-history`, atau deteksi drift; tab Configuration hanya menampilkan form. Password tersimpan sebagai teks biasa di file `0600` itu selama instance berjalan.
- Setiap start NATS membaca Keychain. Autostart saat Keychain terkunci berakhir `NeedsSecret`, bukan server yang jalan.
- Auth tidak bisa dimatikan. Aplikasi yang dulu memakai `nats://127.0.0.1:4222` harus memakai Client URL dengan password.
- Client yang lupa credential mendapat `Permissions Violation` saat publish, bukan penolakan saat connect; ini bisa membingungkan sampai README dibaca.
- Endpoint monitoring masih memberi statistik, nama stream, dan subject ke proses lokal, karena NATS tidak punya auth untuk monitoring.
- User aplikasi hidup di akun NATS `APP`, bukan akun bawaan `$G`; alat yang menampilkan nama akun akan menunjukkannya.
- Password berbentuk hash bcrypt tidak bisa dipakai.
- Instance `1.0.1` tidak ikut aman; pengguna harus membuat instance baru, dan tidak ada alur pindah revision.
- Nama field settings berubah, jadi fixture form, fixture catalog, golden CLI, seed fuzz, dan beberapa test Swift ditulis ulang, dan satu revision catalog baru terbit.
- `nats_compatibility` bertambah empat check dan beberapa restart, jadi lebih lama.
- Fitur ini tidak bisa `done` sebelum kamu menjalankan test launchd dan pemeriksaan GUI.

**Neutral**:

- `HANDOVER_PROVEN`, `PROVIDER_WORDS`, inventory runtime, dan protokol tidak berubah.
- Test mendapat client NATS kecil buatan sendiri di `apps/daemon/tests/nats/client.rs`.
- Hash bcrypt di `nats.conf` tetap mungkin nanti, setelah filter hash template ada (Follow-up).
- Tidak ada `remedies` NATS; kegagalan port memakai banner dan doctor yang sudah ada.

## Follow-up

- [ ] Lead: bawa *Usulan update spec lain* ke pemilik spec 0009, 0023, 0021, 0030, 0036, 0017, dan 0034.
- [ ] Fitur provider pertama yang menambah filter hash template (fitur 43 untuk `sha256_hex`, atau fitur 41 untuk bcrypt): tinjau ulang password NATS sebagai hash bcrypt, supaya `nats.conf` tidak lagi memuat teks biasa. Mengembalikan Effective File butuh hash di config biasa dan pratinjau spec 0029 tanpa Keychain, jadi itu keputusan `/architect` tersendiri.
- [ ] `/scope`: alur memindahkan instance ke revision paket yang lebih baru (hari ini instance `1.0.1` hanya bisa dibuat ulang).
- [ ] Fitur 28 (sudah di Follow-up spec 0023): pemakaian store JetStream dari `/jsz`.
- [ ] Pemilik `StacklyUI`: `FixtureConfiguration.swift` menampilkan Effective File `nats.conf` di fixture tampilan; instance NATS `1.1.0` asli tidak punya. Putuskan apakah fixture itu pindah ke provider lain.
- [ ] `/sync` setelah build: catat di root `AGENTS.md` bahwa NATS memakai password client dengan `nats.conf` di folder `secrets/`, user `stackly-readiness`, dan escaper `natsConf` baru.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, keputusan kecil, dan hasil cross check ada di [rationale.md](rationale.md).
