# 0023. Provider NATS lewat manifest

**Date**: 2026-10-20
**Status**: Proposed

## Summary

Spec ini membuat NATS menjadi provider nyata kedua Stackly hanya lewat paket `providers/nats` (manifest, settings schema, template) di atas engine fitur 9, ditambah satu perubahan core yang ditinjau: jenis probe `native` dengan probe pertamanya, `nats-readiness`. Probe itu berbicara protokol client NATS ke endpoint `client` instance: ia memeriksa bahwa server yang menjawab memang server instance ini (`server_name`), bahwa mode JetStream sama dengan yang diminta settings, bahwa server memproses perintah (PING dan PONG), dan, kalau JetStream diminta, bahwa API JetStream menjawab. Jadi port yang terbuka saja, atau proses lain di port itu, tidak pernah dianggap ready. Selebihnya (install, create, start, stop, form GUI, CLI) memakai jalur yang sama dengan PostgreSQL tanpa kode Rust atau Swift khusus NATS, dan semua perubahan core dicatat sebagai ukuran RISK-07.

## Requirements

**User stories**:

- Sebagai developer, saya ingin membuat server NATS lokal dengan JetStream lewat alur yang sama dengan PostgreSQL (catalog, install, create, start, stop), supaya saya tidak perlu menulis `nats.conf` atau memilih port sendiri.
- Sebagai developer, saya ingin status ready berarti server NATS milik instance itu benar benar menjawab protokol client, dengan JetStream aktif kalau saya memintanya, supaya aplikasi saya tidak gagal konek ke proses yang salah atau ke JetStream yang belum siap.
- Sebagai maintainer Stackly, saya ingin bukti bahwa provider kedua cukup dengan paket ditambah satu probe yang ditinjau, dengan daftar perubahan core yang terukur, supaya RISK-07 (schema terlalu sempit) dijawab sebelum fitur lain menumpuk di atas engine.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-029):

- **AC-1**: Manifest menerima `type: native` dengan field `name`, `endpoint`, dan `params` (di `readiness.probe` dan `health.probe`), mengikuti *Jenis probe native*. Nama di luar registry ditolak `unknown_probe` dengan pesan yang menyebut nama yang ada; `endpoint` yang tidak ada atau transport nya tidak ada di descriptor ditolak `missing_field` atau `unknown_endpoint`; param yang tidak dikenal `unknown_field`, param wajib yang tidak ada `missing_field`, literal yang tipenya salah atau string kosong `invalid_value`, token folder, root runtime, atau executable `token_not_allowed_here`, token `secret.*` `secret_not_allowed_here`, dan token ke field settings `apply: reload` `settings_schema_rule`, masing masing dengan path field. `path`, `expectStatus`, `command`, dan `successExitCodes` pada probe `native`, serta `name` dan `params` pada probe `tcp`, `http`, dan `exec`, ditolak. `type: script` tetap `unknown_probe`. JSON Schema editor (`schemas/provider/provider.v1alpha1.schema.json`) dibangkitkan ulang dan `cargo xtask codegen --check` lolos (REQ-029, RISK-08).
- **AC-2**: Registry probe native di `stackly_core::provider::probe::native` adalah enum tertutup `NativeProbeKind` dengan tepat satu varian, `NatsReadiness` (nama `nats-readiness`, transport endpoint `tcp`, param `serverName` teks wajib 1 sampai 255 byte dan `jetstream` boolean wajib). Descriptor dan runner setiap varian dipilih lewat `match` yang exhaustive, jadi varian tanpa descriptor atau tanpa runner gagal dikompilasi; unit test memastikan nama unik dan cocok `^[a-z][a-z0-9-]{0,31}$`.
- **AC-3**: `run_plan` membangun `ProbeSpec::Native(NativeProbeSpec { target, probe })` dengan `target` `ProbeTarget::Ip` dari endpoint yang dirujuk dan `probe` `NativeProbe::NatsReadiness { server_name, jetstream }` dari param yang sudah diinterpolasi. Param teks yang setelah render kosong, di atas batas descriptor, atau memuat karakter kontrol menghasilkan `PlanError::UnrepresentableValue { field }` dengan `field` `readiness.probe.params.<nama>` (atau `health.probe...`), tanpa nilainya. `Serialize` plan menulis `{"type":"native","content":{"target":...,"probe":{"name":"nats-readiness","params":{"serverName":...,"jetstream":...}}}}`, dan `ProbePlan` native memakai `attemptTimeoutMilliseconds` blok yang memakainya.
- **AC-4**: Paket `providers/nats` memakai probe readiness di *Paket NATS* dan README yang diperbarui; selain itu isi paket tidak berubah (template, settings schema, endpoint, stop, timeout, dan `metadata.revision` `1.0.0` tetap; baris `runtime.versionCheck` milik fitur 18). `cargo xtask provider check` mencetak `ok official/nats 1.0.0 <manifest_sha256 baru>`, render golden `nats.conf` tidak berubah, golden `run_plan` NATS hanya berubah di `readiness.probe` dan `health.probe`, dan seed fuzz dibangkitkan ulang lewat `cargo xtask fuzz seed`.
- **AC-5**: Satu percobaan `nats-readiness` mengikuti *Algoritma satu percobaan* persis, dan terhadap server tiruan berskrip (test core dengan `tokio::net::TcpListener`) setiap hasil terbukti: sukses dengan JetStream aktif dan tidak aktif; `connect_failed`; `timeout` saat INFO, PONG, atau balasan `$JS.API.INFO` tidak datang (percobaan selesai paling lama `attempt_timeout` ditambah 100 ms); `protocol_error` untuk baris pertama yang bukan `INFO`, baris di atas 16 KiB, JSON rusak, `-ERR`, `MSG` dengan subject atau sid lain, payload di atas 64 KiB, dan total baca di atas 128 KiB; `identity_mismatch`; `tls_required`; `auth_required`; `lame_duck`; `jetstream_disabled`; `jetstream_unexpected`; dan `jetstream_unavailable` untuk balasan ber `error` atau ber `type` lain. `PING` dari server dijawab `PONG`, dan `INFO` serta `+OK` di tengah exchange diabaikan. Byte yang dikirim probe sama persis dengan *Algoritma satu percobaan* (CONNECT tanpa credential apa pun). Setiap detail kegagalan paling panjang 256 karakter, nilai dari server dipotong 64 karakter, dan karakter kontrol diganti `?`.
- **AC-6**: Supervisor menjalankan `ProbeSpec::Native` di jalur readiness, readiness rutin, dan health yang sama dengan probe lain (interval, deadline, debounce `READINESS_FAILURE_THRESHOLD`, dan ambang health spec 0010). Percobaan gagal menjadi observasi `not_ready` (atau `degraded` dan `unhealthy` untuk health) dengan `reason` sama dengan kode hasil probe dan `detail` sama dengan detailnya; percobaan sukses menjadi `ready` atau `healthy`. Dibuktikan dengan `FakeProcessSupervision` dan server tiruan berskrip di bawah waktu tokio yang dijeda.
- **AC-7**: Fuzz target `nats_readiness_protocol` memberi byte acak, dipotong potong secara acak, ke state machine exchange (tanpa IO) untuk kedua mode JetStream dan berjalan 10 menit di Mac ini tanpa crash, hang, atau kehabisan memori; seed berasal dari transcript server di `crates/core/tests/fixtures/nats-protocol/`, dan input yang pernah crash diputar ulang di stable lewat `crates/core/tests/fuzz_regressions.rs` (NFR-009).
- **AC-8**: Lewat daemon in process (`stacklyd::server::start`) dengan `TestRoot` di bawah folder bernama `Test Root Ünï cödé` (jadi `store_dir` di `nats.conf` memuat spasi dan Unicode) dan runtime NATS 2.15.1 dari cache runtime xtask: `install_cached_artifact`, `pin_package` paket NATS, `instance.create` dengan settings `{}` dan kedua endpoint `Auto`, init sukses, `instance.start` membuat lifecycle `running` dengan readiness `not_ready` lalu `ready` lewat `nats-readiness`, dan `instance.get` menampilkan lifecycle, desired, readiness, health, dan proses secara terpisah. `lsof` hanya menunjukkan dua listener proses itu, di host dan port endpoint `client` dan `monitoring` (loopback). `GET /varz` di endpoint `monitoring` (HTTP biasa dari test) melaporkan `server_name` sama dengan nama instance, `jetstream.config.store_dir` di bawah folder data instance, serta `max_memory` dan `max_storage` sama dengan settings default; folder `<data instance>/jetstream` ada. Baris log `Server is ready` sampai ke `LogSink` perekam, dan selama 60 detik setelah ready spool tidak bertambah satu baris per percobaan probe. `instance.stop` mengirim `SIGTERM`, proses keluar dalam 10 detik tanpa `SIGKILL` (status exit dicatat di evidence), instance `stopped`, lalu start berikutnya `ready` lagi. Tidak ada jalur kode daemon yang berbeda untuk NATS (AC-029 bagian provider nyata, REQ-036 dasar).
- **AC-9**: Readiness jujur: setelah `ready`, test mengirim `SIGSTOP` ke `nats-server`. Connect TCP ke endpoint `client` tetap berhasil (dibuktikan test), tetapi readiness menjadi `not_ready` dengan reason `timeout` setelah 2 kegagalan berturut turut, dan health turun sesuai ambangnya. Tidak ada restart atau stop yang dipicu. Setelah `SIGCONT`, readiness kembali `ready` (REQ-012, Done when "bukan TCP terbuka saja"). Test ini memakai `DaemonConfig` dengan `READINESS_REFRESH_INTERVAL` yang dipendekkan.
- **AC-10**: Identitas dan mode terhadap server asing: test menjalankan `nats-server` dari runtime yang sama dengan argumen sendiri, lalu memanggil runner langsung. Server dengan `server_name` `foreign` dan harapan `golden` menghasilkan `identity_mismatch`; harapan JetStream aktif terhadap server tanpa `-js` menghasilkan `jetstream_disabled`; harapan tidak aktif terhadap server dengan `-js` menghasilkan `jetstream_unexpected`; server dengan `--user` dan `--pass` menghasilkan `auth_required`; nama dan mode yang cocok menghasilkan sukses. Lewat daemon, instance dengan settings `{ "jetstream": false }` menjadi `ready`, `GET /varz` tidak memuat `jetstream.config`, dan folder `<data instance>/jetstream` tidak dibuat (PRD 22.2 "mode JetStream yang diminta").
- **AC-11**: Daemon yang dibunuh `SIGKILL` saat instance NATS `ready` lalu dijalankan lagi mengadopsi `nats-server` yang sama (PID dan waktu start sama, tanpa spawn baru), `lsof -d 3` pada proses itu menunjukkan `instance.lock`, dan readiness kembali `ready` lewat `nats-readiness` ke port yang tersimpan (bukti provider nyata spec 0010 AC-9 dengan binary Go).
- **AC-12**: Setiap test native fitur ini memasang guard evidence `native` spec 0002 dengan `covers` `REQ-029`, `REQ-036`, dan `AC-029`. Satu test menulis evidence `native` dengan `covers` `BR-025`, subject `{ provider: nats, version: 2.15.1, digest, source: managed }`, dan `compatibility.json` di `artifact_dir()` berbentuk *Evidence kompatibilitas*, lalu `cargo xtask runtime compat-record` menyalinnya ke `evidence/compatibility/`. Di commit yang sama dengan bukti lifecycle ujung ke ujung (AC-8 sampai AC-11 lulus), entri NATS di `providers/catalog.toml` pindah dari `[[planned]]` ke `[[provider]]` sesuai *Entri catalog*, `revision` dan `published_at` naik, dan `cargo xtask catalog check --update` lolos (aturan masuk catalog spec 0017). Tanpa artefak di cache, setiap test native `BLOCKED` dengan alasan `artifact nats-2.15.1-macos-aarch64 is not in the runtime cache` dan owner `@ojiepermana`, tanpa unduhan.
- **AC-13**: Journey yang sama dari CLI dan GUI, setelah fitur 20 dan 21 dibangun: `stackly instance create` untuk `official/nats` dengan nilai default dan dengan JetStream dimatikan (sintaks fitur 20), `stackly instance start`, `stackly instance get --json` dengan readiness `ready`, dan `stackly instance stop` lulus terhadap NATS asli lewat `SpawnedDaemon`. Fixture form create yang dibangkitkan dari paket NATS asli (DTO fitur 20) lolos test kontrak Rust dan Swift, dan test Swift membuktikan model form generik merender `serverName` sebagai field managed yang terkunci, JetStream sebagai toggle, ketiga field integer advanced dengan batasnya, serta endpoint `client` dan `monitoring` dengan pilihan Fixed atau Auto. Sheet New Instance di GUI membuat dan menjalankan instance NATS sampai Summary menampilkan readiness `ready` terpisah dari status proses, dan alur itu bisa diselesaikan dengan keyboard dan VoiceOver (AC-029 bagian form UI untuk provider nyata).
- **AC-14**: Ukuran RISK-07 tercatat dan benar: commit build fitur ini (dicatat per milestone di `verify.md`) tidak menyentuh file sumber non test di `apps/`, `clients/`, `crates/domain/`, `crates/platform/`, `crates/platform-macos/`, `crates/protocol/`, `crates/storage/`, dan `crates/ipc/`, dan `ProtocolVersion::CURRENT` tidak berubah. File sumber non test yang disentuh build ini dan memuat kata `nats` (tanpa peduli huruf besar) hanya file di *Ukuran RISK-07*. Tabel di bagian itu cocok dengan hasil build, dan selisihnya kembali ke `/architect` lewat update spec ini.

Status test semua AC mulai dari `NOT_RUN`. Lulus di fixture bukan evidence native, dan artefak, sesi GUI, atau fitur prasyarat yang tidak tersedia dicatat `BLOCKED`.

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md): `nats-readiness` berbicara protokol client NATS ke endpoint `client` (INFO untuk identitas dan mode, CONNECT lalu PING dan PONG, dan `$JS.API.INFO` saat JetStream diminta); nilai harapan per instance masuk lewat `params` probe yang menerima token interpolasi dan diselesaikan di plan; registry native adalah enum tertutup dengan `match` exhaustive, bukan trait; logika protokol adalah state machine tanpa IO buatan sendiri dengan pembungkus tokio tipis, bukan crate client NATS; fitur ini tidak mengonfigurasi auth (milik fitur 39); RISK-07 diukur lewat tabel perubahan dan pemeriksaan diff per commit; dan form GUI dibuktikan dengan fixture dari paket asli ditambah pemeriksaan manual.

Paket NATS tetap data saja. Satu satunya kode Rust yang tahu NATS adalah descriptor dan runner `nats-readiness`, sesuai kalimat scope "probe `nats-readiness` sebagai satu perubahan core yang ditinjau"; jenis `native` sendiri adalah perluasan generik yang sudah direncanakan spec 0009.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Skill async relevan untuk pembungkus tokio runner (satu deadline untuk seluruh percobaan, tanpa task latar) dan test dengan waktu tokio yang dijeda.

## Feature design

### Batas fitur

- **Di dalam**: jenis probe `native` di manifest, validasi, plan, dan JSON Schema; registry `NativeProbeKind` dengan `nats-readiness`; runner dan state machine protokol; pemasangan `ProbeSpec::Native` di loop probe supervisor; perubahan probe di paket `providers/nats` dan README nya; golden; fuzz target; test native lewat daemon dengan NATS 2.15.1 dari cache; evidence kompatibilitas; entri catalog; test journey CLI; fixture form dan test Swift nya.
- **Di luar**: credential dan auth NATS, dua instance yang terisolasi, persistence stream setelah restart, dan bentrok port monitoring (fitur 39, AC-036); connection string NATS dan tombol copy (fitur 21); form dan preflight create (fitur 20); reload (fitur 26 mencatat NATS sebagai provider tanpa reload); metrics dari endpoint monitoring (fitur 28); backup JetStream (fitur 53); TLS, cluster, leafnode, gateway, websocket, dan MQTT; unduhan runtime dari jaringan (`nats_installs_from_the_network` tetap langkah 16a spec 0018).

### Jenis probe native

Bentuk manifest (tambahan additive pada *Probe* spec 0009):

```yaml
probe:
  type: native
  name: nats-readiness          # nama di registry, ^[a-z][a-z0-9-]{0,31}$
  endpoint: client              # endpoint yang transport nya ada di descriptor
  params:                       # peta nama param ke nilai skalar
    serverName: "${settings.serverName}"
    jetstream: "${settings.jetstream}"
```

Aturan validasi (di `validate::probe`, cabang `native`; kode issue yang sudah ada, tanpa kode baru):

| Masalah | Kode | Lokasi |
| --- | --- | --- |
| `name` tidak ada | `missing_field` | `<blok>.probe.name` |
| `name` tidak ada di registry | `unknown_probe`, pesan menyebut nama yang ada | `<blok>.probe.name` |
| `endpoint` tidak ada | `missing_field` | `<blok>.probe.endpoint` |
| endpoint tidak dikenal, atau transport nya tidak ada di `endpoint_transports` descriptor | `unknown_endpoint` | `<blok>.probe.endpoint` |
| key param tidak dikenal descriptor | `unknown_field` | `<blok>.probe.params.<key>` |
| param wajib tidak ada (`params` yang tidak ada dibaca sebagai peta kosong) | `missing_field` | `<blok>.probe.params.<nama>` |
| nilai bukan skalar, literal dengan tipe salah, string literal kosong, atau literal teks di atas batas descriptor | `invalid_value` | `<blok>.probe.params.<nama>` |
| token folder, root runtime, atau executable; param boolean yang berisi teks selain tepat satu token boolean | `token_not_allowed_here` | sama |
| token `secret.*` | `secret_not_allowed_here` | sama |
| token `settings.<field>` dengan `apply: reload` | `settings_schema_rule` | sama |
| token yang tidak dikenal | `unknown_token` | sama |
| `path`, `expectStatus`, `command`, atau `successExitCodes` di probe `native`; `name` atau `params` di probe `tcp`, `http`, atau `exec` | sama dengan penolakan field lintas jenis yang sudah ada di `validate::probe` | field itu |

Jenis nilai param (posisi baru di matriks interpolasi spec 0009, baris "nilai `params` probe native"):

| Kind param | Literal YAML yang diterima | String bertoken yang diterima | Hasil di plan |
| --- | --- | --- | --- |
| `text { max_bytes }` | string 1 sampai `max_bytes` byte tanpa karakter kontrol | teks literal plus token teks, integer, atau boolean (seperti entri `args`) | `String`, dicek ulang di plan: 1 sampai `max_bytes` byte, tanpa karakter kontrol, selain itu `UnrepresentableValue` |
| `boolean` | `true` atau `false` | tepat satu token boolean tanpa teks lain | `bool` |

Kind `integer` dan param opsional belum ada, karena belum ada probe yang memakainya; probe native berikutnya yang butuh menambahkannya secara additive. Field settings `apply: reload` ditolak di `params` karena probe rutin memakai plan yang dibangun saat start, jadi nilai baru tidak akan pernah sampai ke probe lewat reload (aturan yang sama dengan spec 0009 untuk `args` dan `env`).

Kode (perkiraan bentuk; nama publik di bawah adalah kontrak):

```rust
// stackly_core::provider::probe::native (tanpa IO)
#[non_exhaustive]
pub enum NativeProbeKind { NatsReadiness }

pub struct NativeProbeDescriptor {
    pub name: &'static str,                          // "nats-readiness"
    pub endpoint_transports: &'static [EndpointTransport],
    pub params: &'static [NativeParamDescriptor],
}
pub struct NativeParamDescriptor { pub name: &'static str, pub kind: NativeParamKind }
pub enum NativeParamKind { Text { max_bytes: usize }, Boolean }

impl NativeProbeKind {
    pub const ALL: &'static [NativeProbeKind] = &[NativeProbeKind::NatsReadiness];
    pub fn descriptor(self) -> &'static NativeProbeDescriptor;   // match exhaustive
    pub fn from_name(name: &str) -> Option<NativeProbeKind>;
}

// stackly_core::provider::plan (tambahan)
pub enum ProbeSpec { Tcp { .. }, Http { .. }, Exec(RunSpec), Native(NativeProbeSpec) }
pub struct NativeProbeSpec { pub target: ProbeTarget, pub probe: NativeProbe }
#[non_exhaustive]
pub enum NativeProbe { NatsReadiness { server_name: String, jetstream: bool } }
```

- `ProbeDef::Native { kind, endpoint, params }` di `manifest.rs` menyimpan nilai param yang sudah divalidasi sebagai literal atau ekspresi interpolasi; plan builder mengubahnya ke `NativeProbe` lewat `match` exhaustive atas `NativeProbeKind`.
- `NativeProbe` di `Serialize` dengan tag `name` (nilai `nats-readiness`) dan konten `params` ber key camelCase sama dengan manifest.
- Endpoint `unix` untuk probe native belum diterima (descriptor `nats-readiness` hanya `tcp`); `ProbeTarget::Unix` tetap ada untuk probe berikutnya.

### Paket NATS

Satu satunya perubahan di `providers/nats/provider.yaml` adalah blok `readiness`:

```yaml
readiness:
  probe:
    type: native
    name: nats-readiness
    endpoint: client
    params:
      serverName: "${settings.serverName}"
      jetstream: "${settings.jetstream}"
```

- `intervalMilliseconds` (500) dan `attemptTimeoutMilliseconds` (2000) tetap default. `health` tetap tidak ada, jadi health memakai probe readiness, dan karena interval health default (10 detik) sama dengan `READINESS_REFRESH_INTERVAL` spec 0010, satu percobaan dipakai untuk keduanya.
- `timeouts`, endpoint `client` (4222) dan `monitoring` (8222), template `nats.conf.tmpl`, settings schema, stop `gracefulProcess` `term`, capability, dan `permissions` tidak berubah. `metadata.revision` tetap `1.0.0`; `manifest_sha256` berubah karena isi manifest berubah.
- `serverName` adalah field `managed` (`${instance.name}`), jadi nilai yang dirender ke `server_name` dan nilai yang diharapkan probe datang dari ekspresi yang sama di plan yang sama.
- `README.md` diganti: readiness lewat `nats-readiness` (identitas, mode JetStream, PING, dan API JetStream), client bernama `stackly-readiness` yang terhubung setiap 10 detik, instance tanpa auth di loopback sampai fitur 39, monitoring hanya di loopback, dan store JetStream di `<data>/jetstream`.

### Algoritma satu percobaan

Runner `stackly_core::supervisor::probe::nats` terdiri dari state machine exchange tanpa IO (`protocol`) dan pembungkus async tipis. Satu deadline, `now + attempt_timeout`, berlaku untuk seluruh percobaan; setiap baca dan tulis memakai sisa deadline itu, dan tidak ada task latar.

1. Connect TCP ke `target` (selalu loopback, dijaga plan spec 0009). Gagal: `connect_failed`.
2. Baca satu baris yang berakhir `\r\n`. Baris harus diawali `INFO` (nama operasi tanpa peduli huruf besar), satu atau lebih spasi atau tab, lalu satu object JSON. Field yang dibaca: `server_name`, `server_id`, `version`, `jetstream`, `auth_required`, `tls_required`, `ldm`; field lain diabaikan, dan field boolean yang tidak ada dibaca `false`. Selain itu: `protocol_error`.
3. Pemeriksaan berurutan, hasil gagal pertama yang menang: `server_name` tidak ada atau berbeda dengan `server_name` harapan (perbandingan byte persis) `identity_mismatch`; `tls_required` `tls_required`; `auth_required` `auth_required`; `ldm` `lame_duck`; `jetstream` `false` padahal harapan `true` `jetstream_disabled`; `jetstream` `true` padahal harapan `false` `jetstream_unexpected`.
4. Tulis tepat `CONNECT {"verbose":false,"pedantic":false,"tls_required":false,"name":"stackly-readiness","lang":"rust","version":"<versi crate stackly-core>","protocol":1,"echo":false,"headers":false,"no_responders":false}\r\nPING\r\n` (urutan field persis ini, tanpa spasi di JSON).
5. Baca baris sampai `PONG`. Selama menunggu (juga di langkah 7): `PING` dijawab `PONG\r\n`, `INFO` dan `+OK` diabaikan, `-ERR <teks>` menjadi `protocol_error` dengan teks itu, dan baris lain menjadi `protocol_error`.
6. Harapan JetStream `false`: sukses, tutup koneksi.
7. Harapan `true`: buat inbox `_INBOX.stackly.<32 hex>` dari 16 byte `getrandom`, tulis `SUB <inbox> 1\r\nPUB $JS.API.INFO <inbox> 0\r\n\r\n`, lalu baca sampai baris `MSG <inbox> 1 <n>` (argumen dipisah spasi atau tab; subject atau sid lain `protocol_error`; `n` di atas `NATS_REPLY_MAX` `protocol_error`), baca `n` byte payload dan `\r\n`, lalu parse object JSON. Object dengan field `error` menjadi `jetstream_unavailable` dengan `code` dan `description` nya; `type` selain `io.nats.jetstream.api.v1.account_info_response` juga `jetstream_unavailable`; selain itu sukses. Tutup koneksi.
8. Deadline lewat di langkah mana pun: `timeout`, dengan detail yang menyebut apa yang ditunggu.

Batas (konstanta di modul runner, bukan `SupervisorConfig`, karena ini batas protokol): `NATS_LINE_MAX` 16 KiB per baris, `NATS_REPLY_MAX` 64 KiB per payload, `NATS_READ_MAX` 128 KiB total baca per percobaan (lewat: `protocol_error`), `NATS_DETAIL_VALUE_MAX` 64 karakter untuk setiap nilai dari server di detail.

**Hasil gagal** (kode `ReasonCode` snake_case; generik supaya probe native berikutnya bisa memakai ulang yang cocok):

| Kode | Arti | Contoh detail |
| --- | --- | --- |
| `connect_failed` | connect TCP ditolak atau error | `connect to 127.0.0.1:4222 failed: connection refused` |
| `timeout` | deadline percobaan lewat | `no PONG from the server within 2000 ms` |
| `protocol_error` | balasan yang tidak sesuai protokol, termasuk `-ERR` | `expected INFO, got "HTTP/1.1 400 Bad Request"` |
| `identity_mismatch` | server lain di port itu | `server_name is "foreign", expected "golden"` |
| `tls_required` | server menuntut TLS, yang tidak pernah dikonfigurasi paket ini | `the server requires TLS` |
| `auth_required` | server menuntut auth, yang tidak pernah dikonfigurasi paket ini | `the server requires authentication` |
| `lame_duck` | server sedang lame duck (menuju berhenti) | `the server is in lame duck mode` |
| `jetstream_disabled` | JetStream mati padahal diminta | `JetStream is off, the instance asks for it` |
| `jetstream_unexpected` | JetStream aktif padahal tidak diminta | `JetStream is on, the instance asks for it off` |
| `jetstream_unavailable` | API JetStream menjawab error | `$JS.API.INFO error 503: JetStream system temporarily unavailable` |

Detail ditulis dalam bahasa Inggris, paling panjang 256 karakter (`ObservationDetail` spec 0004); nilai dari server (nama, teks `-ERR`, `description`, cuplikan baris) dipotong ke `NATS_DETAIL_VALUE_MAX` karakter dengan `...` dan karakter kontrol (C0, DEL, C1) diganti `?`. Detail tidak lewat `Redactor`, karena probe ini tidak pernah mengirim atau menerima secret di fitur ini (fitur 39 meninjau ulang saat auth masuk).

**Kontrak dengan loop probe spec 0010**: runner mengembalikan hasil berbentuk sama dengan runner `tcp`, `http`, dan `exec` (sukses, atau gagal dengan `ReasonCode` dan `ObservationDetail`). `stackly_core::supervisor::probe::native::run(&NativeProbeSpec, attempt_timeout)` memilih runner lewat `match` exhaustive atas `NativeProbe`. Kalau build spec 0010 memakai bentuk hasil lain, runner native mengikutinya dan kode di tabel atas tetap menjadi reason observasi.

### Ukuran RISK-07

| No | Perubahan | Jenis | Tempat | Dipakai provider lain |
| --- | --- | --- | --- | --- |
| 1 | Jenis probe `native`: field manifest, validasi param, posisi interpolasi baru, `ProbeDef::Native`, `ProbeSpec::Native`, JSON Schema, dispatch runner | generik, direncanakan spec 0009 | `crates/core/src/provider/` (`raw.rs`, `validate.rs`, `manifest.rs`, `plan.rs`, `probe/`), `crates/core/src/supervisor/probe/native.rs` | ya, setiap probe native berikutnya |
| 2 | `nats-readiness`: descriptor, state machine protokol, runner | khusus NATS, satu perubahan core yang diizinkan scope | `crates/core/src/provider/probe/native.rs` (varian dan descriptor), `crates/core/src/supervisor/probe/nats.rs` (atau folder `nats/`) | tidak |
| 3 | Fuzz target `nats_readiness_protocol` dan harness nya | khusus NATS, test dan tooling | `fuzz/fuzz_targets/`, harness di `crates/core/src/supervisor/fuzzing.rs`, baris di `TARGETS` `xtask/src/fuzz.rs` | tidak |

Tidak ada perubahan di daemon, CLI, protokol IPC, storage, platform, domain, atau Swift. Data yang berubah: paket `providers/nats`, `providers/catalog.toml`, `providers/catalog-revisions.json`, `schemas/provider/` (dibangkitkan), golden, fixture, dan evidence. Sebelum fitur ini, kode yang sudah tahu NATS adalah format template `natsConf` (spec 0009) dan resep spike xtask (spec 0003); keduanya tidak disentuh. Pembanding: PostgreSQL (spec 0019) butuh empat tambahan core generik dan format detail probe `exec`, tanpa kode khusus PostgreSQL di luar nama format.

Allowlist AC-14 untuk file sumber non test yang memuat kata `nats` dan disentuh build ini: `crates/core/src/provider/probe/native.rs`, `crates/core/src/supervisor/probe/native.rs`, `crates/core/src/supervisor/probe/nats.rs` (atau isi `crates/core/src/supervisor/probe/nats/`), `crates/core/src/supervisor/fuzzing.rs`, `crates/core/src/supervisor/probe/mod.rs` (deklarasi `mod`), dan `xtask/src/fuzz.rs`.

### Entri catalog

Di commit AC-12, `providers/catalog.toml` menghapus `[[planned]]` `official/nats` dan menambah:

```toml
[[provider]]
id = "official/nats"
mark = "Na"
tile = "green"

[[provider.platform_note]]
os = "linux"
status = "planned"
note = "Linux releases exist upstream; Stackly supports macOS first."

[[provider.platform_note]]
os = "windows"
status = "planned"
note = "Windows releases exist upstream; per version verification is pending."
```

`revision` naik satu dari revision terbesar di ledger saat commit, `published_at` adalah waktu UTC commit itu, lalu `cargo xtask catalog check --update`. Kalau commit PostgreSQL (fitur 19) masuk lebih dulu atau bersamaan, commit yang datang belakangan mengambil revision berikutnya setelah rebase.

### Evidence kompatibilitas

```json
{
  "schema": "stackly.compatibility-evidence/v1",
  "providerRevision": { "id": "official/nats", "revision": "1.0.0", "manifestSha256": "<hex>" },
  "checks": ["adoption", "honest_readiness", "identity_check", "init", "jetstream_mode", "readiness", "start", "stop"],
  "knownLimitations": ["client_auth_not_configured", "jetstream_persistence_not_covered"]
}
```

`checks` urut abjad tanpa duplikat (aturan spec 0018). `knownLimitations` menyebut apa yang belum dibuktikan sampai fitur 39: auth client dan persistence stream setelah restart.

### Data model

Tidak ada tabel, kolom, migrasi, DTO, atau method IPC baru. Untuk instance NATS:

| Data | Tempat | Isi |
| --- | --- | --- |
| provider revision | registry, `provider_revision_dir` | `official/nats@1.0.0#<sha>`, capability `multi_instance`, `persistence`, `metrics`, `cold_backup` |
| config revision | registry (`config_revisions`) | nilai settings (`jetstream` dan tiga batas); `serverName` dihitung saat plan, tidak disimpan |
| endpoint | registry (`instance_endpoints`) | `client` dan `monitoring` (tcp, loopback, `Fixed` atau `Auto`, `bound`) |
| config | folder config instance | `nats.conf`, ditulis ulang sebelum setiap spawn |
| store JetStream | `<data instance>/jetstream/` | milik NATS |
| observasi readiness dan health | memori daemon | reason dan detail dari probe |

### API surface

| Fungsi atau artefak | Tempat | Input utama | Output utama | Pemanggil | Error utama |
| --- | --- | --- | --- | --- | --- |
| `NativeProbeKind::{ALL, descriptor, from_name}` | `core::provider::probe::native` | nama | descriptor | validator, test | tidak ada |
| cabang `native` di `validate::probe` | `core::provider::validate` | `RawProbe` | `ProbeDef::Native` | `ValidatedPackage::from_files` | issue di tabel *Jenis probe native* |
| `ProbeSpec::Native` di `run_plan` dan `reload_plan` | `core::provider::plan` | `PlanContext` | `NativeProbeSpec` | spec 0010 | `UnrepresentableValue`, `EndpointUnresolved` |
| `supervisor::probe::native::run` | `core::supervisor::probe` | `&NativeProbeSpec`, `attempt_timeout` | hasil probe | loop readiness dan health spec 0010 | hasil gagal di tabel |
| `nats::protocol::Exchange` | `core::supervisor::probe::nats` | byte dari server, harapan | byte untuk ditulis, hasil | runner, fuzz, test | hasil gagal |
| `supervisor::fuzzing::nats_readiness_protocol(&[u8])` | core, `#[doc(hidden)]` | byte | tidak ada | fuzz target, regresi | tidak ada |
| paket `providers/nats` | repo | file paket | `ValidatedPackage` | catalog, create, test | `manifest.invalid` |
| test native | `apps/daemon/tests/nats.rs` | runtime dari cache | evidence `native`, `compatibility.json` | `cargo test -p stacklyd --test nats -- --ignored` | `BLOCKED` tanpa artefak |
| test journey CLI | `apps/cli/tests/nats_journey.rs` | `SpawnedDaemon`, runtime dari cache | evidence `native` | `cargo test -p stackly-cli --test nats_journey -- --ignored` | `BLOCKED` tanpa artefak atau fitur 20 |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| validasi | descriptor, nama, kind param | `NativeProbeKind::descriptor` (konstanta di core) |
| plan | host dan port target | endpoint yang dirujuk `probe.endpoint` (`client`): port `Fixed`, atau `bound` yang dialokasikan spec 0010 sebelum `run_plan` |
| plan | `server_name` harapan | param `serverName` = `${settings.serverName}` = field managed `${instance.name}` saat plan dibangun |
| plan | JetStream harapan | param `jetstream` = `${settings.jetstream}` dari config revision aktif |
| plan | timeout percobaan | `readiness.attemptTimeoutMilliseconds` (default 2000), atau milik blok `health` |
| render | `server_name`, `listen`, `http`, `max_payload`, `store_dir`, batas JetStream | template yang ada (spec 0009), tidak berubah |
| probe | byte CONNECT | literal di *Algoritma satu percobaan*; versi dari `env!("CARGO_PKG_VERSION")` `stackly-core` |
| probe | inbox | 16 byte `getrandom` per percobaan |
| probe | reason dan detail | hasil state machine; nilai dari server disanitasi |
| observasi | state, reason, detail, `observed_at` | hasil probe dan `Clock::now()` (spec 0010) |
| create (test) | settings, endpoint | `instance.create` minimal spec 0010: `settings: {}` atau `{ "jetstream": false }`, kedua endpoint `Auto` |
| install (test) | file artefak | `stackly_testkit::runtime_cache::cached_artifact(sha256)` (spec 0018), sha256 dari `versions.json` |
| catalog | `revision`, `published_at` | ledger saat commit ditambah satu; waktu UTC commit |
| evidence | `providerRevision`, `digest`, `version` | `ValidatedPackage::key()` dan record artefak |
| AC-14 | daftar file per commit | `git show --stat` setiap commit build fitur ini, dicatat di `verify.md` |

### Edge cases dan kegagalan

| Kasus | Yang terjadi | Penanganan |
| --- | --- | --- |
| Port `Fixed` dipakai NATS milik pengguna (misalnya 4222) | spec 0010 | `supervisor.endpoint_in_use` sebelum spawn; endpoint `Auto` menghindarinya |
| Server lain mengambil port setelah instance keluar | probe membaca `server_name` lain | `identity_mismatch`, readiness `not_ready`; proses instance sendiri diurus spec 0010 |
| Proses berhenti merespons (tertahan, macet) | connect TCP tetap berhasil lewat backlog kernel | `timeout`, `not_ready` setelah 2 kegagalan, tanpa restart (AC-9) |
| JetStream lambat pulih setelah crash (store besar) | `$JS.API.INFO` error 503 atau tidak menjawab | `jetstream_unavailable` atau `timeout` sampai pulih; deadline `recoverySeconds` habis membuat operasi gagal sementara proses tetap jalan (spec 0010) |
| JetStream dimatikan NATS saat berjalan (misalnya disk penuh) | API JetStream menjawab error | `jetstream_unavailable`, readiness `not_ready` dengan detail |
| Pengguna mematikan JetStream lewat settings | restart (`apply: restart`) | probe mengharapkan mode mati; data di `<data>/jetstream` tidak disentuh |
| Path data berisi `"`, `\`, atau `$` | escaper `natsConf` menolak | `UnrepresentableValue` saat plan (spec 0009, tidak berubah) |
| Instance diganti nama saat berjalan, lalu daemon restart dan mengadopsi proses | plan probe baru memakai nama baru, proses masih memakai nama lama | `identity_mismatch` sampai restart; dicatat sebagai Follow-up untuk fitur 29 dan 30 |
| NATS masuk lame duck (`SIGUSR2` dari luar) | listener ditutup, lalu proses keluar | `connect_failed` atau `lame_duck`, lalu restart policy spec 0010 untuk exit yang tidak diminta |
| Server menutup koneksi di tengah exchange | baca menghasilkan EOF | `protocol_error` dengan detail `connection closed while waiting for <x>` |
| Klien lain berlangganan `_INBOX.>` | melihat balasan account info | tidak sensitif; inbox acak per percobaan mencegah tabrakan |
| Artefak tidak ada di cache | test native | `BLOCKED` dengan alasan dan owner di AC-12 |

### Asumsi pada spec lain

- **Assumed (spec 0009)**: tambahan additive di *Usulan additive untuk spec lain* diterima apa adanya; sampai update spec 0009 diterapkan lead, spec ini sumber kebenaran untuk jenis `native`.
- **Assumed (spec 0010)**: modul `core::supervisor::probe` dengan runner `tcp`, `http`, dan `exec` (langkah 6 spec 0010), core dengan tokio fitur `net` dan `io-util`, hasil probe yang membawa `ReasonCode` dan `ObservationDetail`, readiness rutin dengan debounce, `instance.create` minimal yang menerima `settings` dan endpoint, adopsi lewat lock di fd 3, dan `DaemonConfig` yang interval probenya bisa dipendekkan di test. Kalau `probe` dibangun sebagai satu file, langkah 3 mengubahnya menjadi folder.
- **Assumed (spec 0018)**: `runtime::fixture::install_cached_artifact` (milestone 2), `runtime_cache::cached_artifact`, `cargo xtask runtime compat-record` (milestone 3), bentuk `compatibility.json`, dan baris `runtime.versionCheck` NATS dari langkah 1 spec 0018.
- **Assumed (spec 0019)**: `supervisor::fixture::pin_package(platform, layout, dir)` dari langkah 6 spec 0019; kalau fitur ini dibangun lebih dulu, langkah 6 di sini membuatnya dengan signature itu dan spec 0019 memakainya.
- **Assumed (spec 0017)**: `providers/catalog.toml` dan ledger dari milestone 1 spec 0017, dengan NATS sebagai `[[planned]]`.
- **Assumed (spec 0020, ditulis paralel oleh sesi `arc-f20-spec`)**: form create dirender generik dari model field settings dan `resources.endpoints` paket: field `managed` tampil terkunci dengan nilai turunan, boolean sebagai toggle, `visibility: advanced` di bagian advanced, integer dengan `minimum` dan `maximum`, setiap endpoint tcp dengan Fixed atau Auto; `stackly instance create` menerima settings dan endpoint dengan sintaks fitur 20; dan DTO form punya fixture di `protocols/fixtures/`. Kalau spec 0020 yang di merge berbeda, AC-13 mengikuti spec 0020 lewat update spec ini sebelum milestone 3.

### Jawaban follow-up dari spec lain

- **Spec 0009** ("Fitur 23: tambah jenis probe `native`, registry `NativeProbeDescriptor`, dan `nats-readiness` (cek JetStream dan identitas `server_name`) sesuai kontrak di *Probe*, lalu ganti probe `http` di `providers/nats`"): dijawab *Jenis probe native*, *Algoritma satu percobaan*, dan *Paket NATS*. Dua penyesuaian kontrak ditulis sebagai usulan additive: `params` menerima token interpolasi (harapan per instance tidak bisa literal), dan runner dipilih lewat `match` exhaustive atas enum tertutup, bukan trait (lihat rationale Keputusan 3).
- **Spec 0017** (*Aturan masuk catalog*): dijawab AC-12 dan *Entri catalog*; perpindahan NATS ada di commit yang sama dengan bukti lifecycle ujung ke ujung.
- **Spec 0010** ("Bukti dengan provider nyata milik fitur 19, 23, 26, dan 30"): AC-8, AC-9, AC-11 adalah bukti native untuk startup transaction, readiness dan readiness rutin dengan debounce, stop tanpa `SIGKILL`, render sebelum spawn, dan adopsi lewat lock di fd 3 dengan binary Go (berbeda runtime dari PostgreSQL).
- **Spec 0018** (satu pipeline install untuk semua provider): test native memasang NATS dari cache lewat `install_cached_artifact`, pipeline yang sama dengan provider lain; `nats_installs_from_the_network` tetap milik langkah 16a spec 0018 dan tidak diulang di sini; `runtime.versionCheck` NATS milik langkah 1 spec 0018.
- **Spec 0019** ("Kontrak probe native tetap milik fitur 23"; pola paket, golden, evidence): kontrak itu ditulis di spec ini. Pola paket, golden, test native lewat daemon, dan `compatibility.json` mengikuti spec 0019. NATS butuh probe native karena artefak `nats-server` tidak membawa CLI client apa pun untuk probe `exec`, dan `/healthz` tidak bisa memeriksa identitas.

### Usulan additive untuk spec lain

Ditulis di sini karena spec ini tidak boleh mengubah spec lain; lead menjadikannya item `architect (update)`.

- **Spec 0009** (setelah sesi `f9-closeout` selesai): AC-4 mendapat `native` dengan nama dari registry; *Probe* mendapat bentuk `type: native` dan aturan di *Jenis probe native*, dengan kalimat "runner lewat trait milik spec 0010" diganti "runner dipilih lewat `match` exhaustive atas `NativeProbe` di `core::supervisor::probe::native`"; matriks posisi interpolasi mendapat baris "nilai `params` probe native"; aturan `apply: reload` mencakup `params`; *Execution plan* mendapat `ProbeSpec::Native(NativeProbeSpec)`; tabel *Sumber timeout dan exit code* mendapat baris `ProbeSpec::Native` (percobaan: `attemptTimeoutMilliseconds` blok yang memakainya); *Fuzz target* mendapat `nats_readiness_protocol` (milik spec ini); *Paket contoh NATS* dan keputusan kecil "Probe readiness NATS memakai `/healthz`" ditandai diganti spec 0023; skenario "Himpunan tertutup" memakai probe `native` dengan nama yang tidak ada di registry; baris Follow-up fitur 23 dicentang.
- **Spec 0010**: *Probe, readiness, dan health* mencatat `ProbeSpec::Native` dan dispatch ke `supervisor::probe::native::run`, hasil probe gagal yang membawa `ReasonCode` dan `ObservationDetail`, dan kode reason generik `connect_failed`, `timeout`, dan `protocol_error` yang juga dipakai runner `tcp` dan `http` kalau build spec 0010 belum memilih kode lain; baris "Bukti dengan provider nyata" menyebut AC-8, AC-9, dan AC-11 spec ini.
- **Spec 0020** (tidak mengubah keputusan): form generik juga diuji dengan fixture paket NATS asli di AC-13 spec ini.

### Key invariants

- Readiness `ready` untuk instance NATS hanya dari percobaan yang melihat `server_name` instance itu, mode JetStream yang diminta, PONG, dan (kalau diminta) balasan API JetStream tanpa error.
- Probe hanya terhubung ke endpoint loopback milik instance, tidak pernah mengirim credential, dan hanya berlangganan inbox acak nya sendiri dengan `echo` mati.
- Setiap byte dari server dibaca dengan batas (`NATS_LINE_MAX`, `NATS_REPLY_MAX`, `NATS_READ_MAX`) dan satu deadline per percobaan; tidak ada koneksi yang bertahan melewati percobaan.
- Paket NATS tetap data saja; satu satunya kode khusus NATS di jalur produk adalah descriptor dan runner `nats-readiness`.
- Registry native tertutup: nama yang lolos validasi selalu punya runner, dijamin compiler.

### Security model

- Ancaman yang dijaga: proses lain di port instance yang tampil ready (dijawab cek identitas), port terbuka tanpa server yang melayani (PING dan PONG), JetStream yang tidak sesuai permintaan (cek mode dan API), byte jahat dari server di port itu (batas baca, parser yang di fuzz, sanitasi detail), dan monitoring yang terbuka ke LAN (host endpoint loopback dijaga plan dan dibuktikan `lsof` di AC-8).
- Residual risk yang dinyatakan: instance NATS fitur ini tanpa auth, jadi proses lokal mana pun (termasuk milik user lain di Mac ini) bisa terhubung ke port client dan monitoring di loopback, seperti default upstream `nats-server`. Credential milik fitur 39 (REQ-036), yang juga memperluas probe secara additive. Cek identitas lewat `server_name` membuktikan server NATS bernama instance itu, bukan proses itu sendiri; identitas proses tetap tugas spec 0010.
- Tidak ada `unsafe` baru, tidak ada dependency baru.

### Configuration required

Tidak ada variabel environment, credential, atau port baru untuk daemon dan CLI. Test native butuh artefak `nats-2.15.1-macos-aarch64` (sha256 `ed0fe39222ec2e317c4d0e1a9cdc16c63ec523d94f9355d24a902a4db2655ffd`) di cache runtime xtask; per 2026-10-20 file itu sudah ada di Mac ini. Tidak ada test fitur ini yang mengunduh dari jaringan.

### Critical test scenarios

- Validasi: setiap baris tabel *Jenis probe native* ditolak dengan kode dan path nya; manifest NATS baru lolos; probe `native` di `health.probe` diterima dengan aturan yang sama. Memverifikasi **AC-1**, **AC-2**.
- Plan: golden `run_plan` NATS memuat `ProbeSpec::Native` dengan `serverName` `golden` dan `jetstream` `true` di context golden spec 0009; context dengan nama yang membuat param kosong atau terlalu panjang (lewat paket fixture yang memetakan param ke field teks) menghasilkan `UnrepresentableValue` dengan nama param. Memverifikasi **AC-3**, **AC-4**.
- Protokol: tabel transcript server tiruan untuk setiap baris tabel hasil gagal, ditambah sukses kedua mode, PING dari server, INFO di tengah, dan pemotongan detail; transcript juga dimainkan langsung ke state machine dengan potongan byte acak. Memverifikasi **AC-5**, **AC-7**.
- Loop: readiness `not_ready` lalu `ready`, debounce 2 kegagalan, health `degraded` lalu `unhealthy`, dengan waktu dijeda. Memverifikasi **AC-6**.
- Happy path native: install dari cache, create, start sampai `ready`, `lsof`, `/varz`, log, stop, start lagi. Memverifikasi **AC-8**.
- Readiness jujur: `SIGSTOP` dan `SIGCONT` dengan connect TCP yang tetap berhasil. Memverifikasi **AC-9**.
- Server asing: empat server asing dan instance dengan JetStream mati. Memverifikasi **AC-10**.
- Adopsi: `SIGKILL` daemon saat ready. Memverifikasi **AC-11**.
- Evidence dan catalog: cache kosong `BLOCKED`; run lengkap menulis `compatibility.json` yang lolos `compat-record`; `catalog check` lolos setelah perpindahan. Memverifikasi **AC-12**.
- Journey CLI dan GUI: test CLI, fixture form dan test Swift, lalu pemeriksaan GUI manual dengan keyboard dan VoiceOver. Memverifikasi **AC-13**.
- RISK-07: daftar file per commit dibandingkan dengan allowlist. Memverifikasi **AC-14**.

## Build plan

Urutan mengikuti pendekatan Journey proyek: jalur "instance NATS dari paket sampai ready lalu stop, dengan readiness yang jujur" dibuat lengkap dulu. Milestone 1 membangun semua yang bisa dibuktikan tanpa server NATS asli, milestone 2 membuktikan jalur itu pada NATS 2.15.1 asli lewat daemon dan membuka entri catalog, dan milestone 3 membuktikan journey yang sama dari CLI dan GUI di atas fitur 20 dan 21.

**Milestone 1: Probe native dan `nats-readiness` di fixture**

0. Spike lokal (tanpa daemon dan tanpa jaringan): ekstrak arsip NATS 2.15.1 dari cache runtime xtask ke folder sementara, jalankan `nats-server` dengan `server_name`, dengan dan tanpa `-js`, dan dengan `--user` serta `--pass`, lalu rekam baris INFO, balasan PONG, balasan `$JS.API.INFO` (kedua mode), `-ERR` untuk CONNECT tanpa credential, dan `/varz` ke `crates/core/tests/fixtures/nats-protocol/`. Kalau field yang dipakai spec ini (`server_name`, `jetstream`, `auth_required`, `tls_required`, `ldm`, `type` dan `error` balasan API, `jetstream.config` di `/varz`) berbeda, build berhenti dan kembali ke `/architect`. Spike juga mencatat apakah INFO pertama setelah start sudah memuat `jetstream: true`; kalau belum, readiness tetap benar (percobaan berikutnya mengulang), tetapi detail sementara `jetstream_disabled` dicatat di Consequences lewat update spec ini. Memenuhi **AC-5** dan **AC-7** (transcript asli), **AC-8** (nama field).
1. Engine: field `name` dan `params` di `RawProbe`, modul `provider::probe::native` (`NativeProbeKind`, descriptor, kind param), cabang `native` di `validate::probe` dengan semua aturan *Jenis probe native* (termasuk posisi interpolasi baru dan aturan `apply: reload`), `ProbeDef::Native`, `ProbeSpec::Native` dan `NativeProbe` di plan beserta `Serialize`, JSON Schema lewat `cargo xtask codegen`, dan skenario "Himpunan tertutup" spec 0009 yang memakai nama yang tidak ada di registry. Memenuhi **AC-1**, **AC-2**, **AC-3**.
2. Paket: blok `readiness` di `providers/nats/provider.yaml`, `README.md`, golden `run_plan` NATS (`STACKLY_UPDATE_GOLDEN=1`, diff hanya di probe), `cargo xtask provider check`, dan `cargo xtask fuzz seed`. Memenuhi **AC-4**.
3. Runner: `supervisor::probe::nats` dengan state machine `protocol::Exchange` tanpa IO, pembungkus tokio dengan satu deadline, konstanta batas, sanitasi detail, `supervisor::probe::native::run` dengan `match` exhaustive, dan test server tiruan berskrip untuk setiap hasil. Memenuhi **AC-5**.
4. Loop: `ProbeSpec::Native` di readiness, readiness rutin, dan health spec 0010, dengan test `FakeProcessSupervision` dan waktu dijeda. Memenuhi **AC-6**.
5. Fuzz: harness `supervisor::fuzzing::nats_readiness_protocol`, target `fuzz/fuzz_targets/nats_readiness_protocol.rs`, baris `TARGETS` dan sumber seed di `xtask/src/fuzz.rs`, transcript di `crates/core/tests/fixtures/nats-protocol/`, replay regresi, lalu run 10 menit di Mac ini. Memenuhi **AC-7**.

**Milestone 2: NATS 2.15.1 asli lewat daemon, evidence, dan catalog**

6. Harness test native `apps/daemon/tests/nats.rs`: runtime lewat `install_cached_artifact` dengan jalur `BLOCKED`, `pin_package` (dibuat di sini dengan signature spec 0019 kalau belum ada), server in process dengan `LogSink` perekam dan interval probe yang bisa dipendekkan, helper server asing dengan guard yang me reap prosesnya, lalu test happy path (`lsof`, `/varz`, log, stop, start lagi). Memenuhi **AC-8**.
7. Test readiness jujur (`SIGSTOP`, `SIGCONT`), server asing (identitas, kedua mode, auth), instance dengan JetStream mati, dan adopsi setelah `SIGKILL` daemon. Memenuhi **AC-9**, **AC-10**, **AC-11**.
8. Guard evidence `native`, `compatibility.json`, `cargo xtask runtime compat-record`, dan di commit yang sama perpindahan entri catalog dengan `revision` dan `published_at` baru serta `cargo xtask catalog check --update`. Memenuhi **AC-12**.

**Milestone 3: Journey yang sama dari CLI dan GUI**

9. Test journey CLI `apps/cli/tests/nats_journey.rs` lewat `SpawnedDaemon` dengan runtime dari cache: create default dan dengan JetStream mati, start, `get --json`, stop. Memenuhi **AC-13** (bagian CLI).
10. Fixture form create dari paket NATS asli (DTO fitur 20) yang dibangkitkan test `stacklyd` dengan cek drift, test kontrak Rust dan Swift nya, dan test Swift model form generik terhadap fixture itu. Memenuhi **AC-13** (bagian form).
11. Pemeriksaan RISK-07: daftar file setiap commit build fitur ini dicatat di `verify.md` dan dibandingkan dengan allowlist dan tabel *Ukuran RISK-07*; langkah GUI manual AC-13 disiapkan di `verify.md` untuk `/check verify`. Memenuhi **AC-13** (bagian GUI), **AC-14**.

**Prasyarat**: langkah 0 hanya butuh artefak NATS di cache runtime xtask (sudah ada di Mac ini). Langkah 1 dan 2 butuh build fitur 9 (sudah selesai). Langkah 3 butuh langkah 6 spec 0010 (runner probe dan tokio `net` di core) dan langkah 1 di sini; langkah 4 butuh loop readiness spec 0010 (milestone "Start to ready and safe stop on the fake provider"). Milestone 2 butuh build fitur 10 sampai milestone "Daemon, IPC, and end to end proof", fitur 11 selesai, milestone 2 spec 0018 (`install_cached_artifact`), milestone 1 spec 0017 (`providers/catalog.toml`), dan, untuk langkah 8, milestone 3 spec 0018 (`compat-record`). Milestone 3 butuh build fitur 20 (form create dan `stackly instance create`) dan fitur 21 (Summary dengan readiness).

**Sesi paralel yang aman**: setelah langkah 1 masuk, langkah 2 (paket dan golden) dan langkah 3 (runner) menyentuh file berbeda dan bisa menjadi dua sesi; langkah 5 bisa menyusul langkah 3 di sesi sendiri sementara langkah 4 berjalan. Langkah 6 sampai 8 sebaiknya satu sesi karena berbagi file test. Di milestone 3, langkah 9 dan 10 bisa dua sesi.

## Consequences

**Positive**:

- Ready untuk NATS berarti server milik instance itu menjawab protokol client dengan mode JetStream yang diminta, bukan port terbuka atau HTTP 200 dari proses lain (PRD 22.2).
- RISK-07 terjawab dengan angka: satu perubahan core khusus NATS dan satu perluasan generik yang sudah direncanakan, tanpa perubahan daemon, CLI, protokol, atau Swift.
- Jenis probe `native` sekarang ada beserta pola registry, param, dan fuzzing, jadi provider berikutnya yang butuh cek protokol (misalnya ClickHouse atau Valkey) menambah satu varian, bukan kerangka baru.
- Supervisor mendapat bukti provider nyata kedua dengan runtime Go: startup, readiness jujur, stop, dan adopsi.

**Negative / tradeoffs**:

- Ada kode protokol NATS kecil buatan sendiri di core yang harus dirawat dan di fuzz; perubahan protokol upstream (misalnya field INFO yang berganti nama) bisa membuat readiness gagal sampai runner diperbarui.
- Setiap instance NATS menerima satu koneksi pendek setiap 10 detik selama hidup, terlihat sebagai client `stackly-readiness` di monitoring.
- Instance NATS fitur ini tanpa auth di loopback sampai fitur 39; catalog dan README menyebutnya.
- Cek identitas memakai `server_name`, jadi server NATS asing yang kebetulan bernama sama dengan instance di port yang sama tidak terbedakan oleh probe.
- Mengganti nama instance yang sedang berjalan lalu me restart daemon membuat readiness `identity_mismatch` sampai instance di restart.
- Milestone 3 menunggu fitur 20 dan 21, jadi fitur ini tidak bisa `done` sebelum keduanya.

**Neutral**:

- Kode reason `connect_failed`, `timeout`, dan `protocol_error` diusulkan sebagai kode generik untuk semua runner probe.
- Field settings NATS tetap dalam byte; satuan yang lebih ramah (MiB) adalah Follow-up fitur 39 atau 29.
- `providers/nats` menjadi provider nyata pertama yang memakai probe native dan endpoint ber purpose `monitoring`.

## Follow-up

- [ ] `/architect` update spec 0009 (setelah sesi `f9-closeout` selesai): terapkan *Usulan additive untuk spec lain* bagian spec 0009, lalu centang baris Follow-up fitur 23 di spec itu.
- [ ] `/architect` update spec 0010: terapkan usulan bagian spec 0010 (dispatch native, bentuk hasil probe, kode reason generik, bukti provider nyata).
- [ ] Fitur 39: auth client NATS (credential lewat secret dan template `sensitive`), perluasan `nats-readiness` untuk auth tanpa membaca Keychain setiap 10 detik (misalnya user readiness berhak minimum), dua instance terisolasi, persistence stream setelah restart, dan bentrok port monitoring (AC-036); hapus `knownLimitations` yang sudah terbukti.
- [ ] Fitur 21: connection string NATS (`nats://<host>:<port>` dari endpoint `client`, tanpa credential sampai fitur 39) dan URL monitoring, menjawab juga Follow-up spec 0009 tentang `purpose` endpoint.
- [ ] Fitur 29 dan 30: rename instance saat berjalan mengubah nilai `${instance.name}` yang dipakai field managed dan probe; tolak rename saat berjalan untuk paket yang field managed nya memakai nama, atau bekukan nilai managed saat start untuk dipakai adopsi.
- [ ] Fitur 39 atau 29: satuan MiB untuk batas JetStream dan `max_payload` di form, setelah suffix ukuran `nats.conf` dibuktikan pada binary asli.
- [ ] Fitur 36: kebijakan kompatibilitas v1alpha1 mencatat jenis `native`, kind param, dan cara menambah probe native lewat review.
- [ ] `/sync` setelah build: catat jenis probe native, `cargo test -p stacklyd --test nats -- --ignored`, dan fuzz target baru di root `AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, dan keputusan kecil ada di [rationale.md](rationale.md).
