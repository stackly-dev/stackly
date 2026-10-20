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
- **AC-2**: Registry probe native di `stackly_core::provider::probe::native` adalah enum tertutup `NativeProbeKind` dengan tepat satu varian, `NatsReadiness` (nama `nats-readiness`, transport endpoint `tcp`, param `serverName` teks wajib 1 sampai 255 byte dan `jetstream` boolean wajib). `NativeProbe` dan fungsi `native::build` yang mengubah param tervalidasi menjadi `NativeProbe` tinggal di modul yang sama, sehingga `validate.rs`, `manifest.rs`, dan `plan.rs` hanya memakai tipe generik dan tidak pernah menyebut NATS. Descriptor, `build`, dan runner setiap varian dipilih lewat `match` yang exhaustive, jadi varian tanpa salah satunya gagal dikompilasi; unit test memastikan nama unik dan cocok `^[a-z][a-z0-9-]{0,31}$`.
- **AC-3**: `run_plan` membangun `ProbeSpec::Native(NativeProbeSpec { target, probe })` dengan `target` `ProbeTarget::Ip` dari endpoint yang dirujuk dan `probe` `NativeProbe::NatsReadiness { server_name, jetstream }` dari param yang sudah diinterpolasi. Param teks yang setelah render kosong, di atas batas descriptor, atau memuat karakter kontrol menghasilkan `PlanError::UnrepresentableValue { field }` dengan `field` `readiness.probe.params.<nama>` (atau `health.probe...`), tanpa nilainya. `Serialize` plan menulis `{"type":"native","content":{"target":...,"probe":{"name":"nats-readiness","params":{"serverName":...,"jetstream":...}}}}`, dan `ProbePlan` native memakai `attemptTimeoutMilliseconds` blok yang memakainya.
- **AC-4**: Paket `providers/nats` memakai probe readiness dan blok `connection` di *Paket NATS* dan README yang diperbarui; selain itu isi paket tidak berubah oleh fitur ini (template, settings schema, endpoint, stop, timeout, dan `metadata.revision` `1.0.0` tetap; baris `runtime.versionCheck` milik langkah 1 spec 0018 dipertahankan apa adanya). `cargo xtask provider check` mencetak `ok official/nats 1.0.0 <manifest_sha256 baru>`, render golden `nats.conf` tidak berubah, golden `run_plan` NATS hanya berubah di `readiness.probe` dan `health.probe`, dan seed fuzz dibangkitkan ulang lewat `cargo xtask fuzz seed`.
- **AC-5**: Satu percobaan `nats-readiness` mengikuti *Algoritma satu percobaan* persis. State machine `provider::probe::nats::Exchange` (tanpa IO) terbukti untuk setiap hasil lewat transcript, termasuk transcript asli dari langkah 0: sukses dengan JetStream aktif dan tidak aktif; `protocol_error` untuk baris pertama yang bukan `INFO`, INFO yang bukan object, field INFO dengan tipe salah (misalnya `"jetstream":"x"` atau `server_name` berupa angka), byte setelah object INFO, baris di atas 16 KiB, `-ERR`, `MSG` dengan subject atau sid lain, payload di atas 64 KiB, total baca di atas 128 KiB, serta koneksi yang ditutup atau di reset di tengah exchange; `identity_mismatch`; `tls_required`; `auth_required`; `lame_duck`; `jetstream_disabled`; `jetstream_unexpected`; dan `jetstream_unavailable` untuk balasan ber `error` atau ber `type` lain. Field boolean INFO yang tidak ada dibaca `false`. `PING` dari server dijawab `PONG`, dan `INFO` serta `+OK` di tengah exchange diabaikan. Byte yang dikirim probe sama persis dengan *Algoritma satu percobaan* (CONNECT tanpa credential apa pun). Driver async lewat `tokio::io::duplex` di bawah waktu dijeda membuktikan `connect_failed`, dan `timeout` saat connect, INFO, PONG, atau balasan `$JS.API.INFO` tidak datang, dengan percobaan berakhir tepat di deadline dan teks detail per fase di *Hasil gagal*; satu test smoke dengan `tokio::net::TcpListener` asli membuktikan sukses lewat socket. Setiap detail kegagalan paling panjang 256 karakter, nilai dari server lebih dari 64 karakter dipotong menjadi 61 karakter ditambah `...`, dan karakter kontrol diganti `?`.
- **AC-6**: Supervisor menjalankan `ProbeSpec::Native` di jalur readiness, readiness rutin, dan health yang sama dengan probe lain (interval, deadline, debounce `READINESS_FAILURE_THRESHOLD`, dan ambang health spec 0010). `ProbeOutcome::Ok` menjadi observasi `ready` (atau `healthy`) dengan reason `probe_ok`; `ProbeOutcome::Failed { reason, detail }` menjadi `not_ready` dengan reason dan detail yang sama, dan untuk health `degraded` setelah 1 kegagalan lalu `unhealthy` setelah 3 (ambang default). Dibuktikan dengan `FakeProcessSupervision` dan driver di atas `tokio::io::duplex` di bawah waktu tokio yang dijeda.
- **AC-7**: Fuzz target `nats_readiness_protocol` memberi byte acak ke `Exchange` (tanpa IO) dengan encoding input di *Fuzz target*, untuk kedua mode JetStream, dan berjalan 10 menit di Mac ini tanpa crash, hang, atau kehabisan memori; seed berasal dari transcript di `crates/core/tests/fixtures/nats-protocol/`, dan input yang pernah crash diputar ulang di stable lewat `crates/core/tests/fuzz_regressions.rs`. Setelah perubahan validator di langkah 1, fuzz target `provider_manifest` dan `provider_interpolation` dijalankan ulang 10 menit masing masing (NFR-009).
- **AC-8**: Test `nats_lifecycle_in_process` lewat daemon in process (`stacklyd::server::start`) dengan root data `RootSet::under_override(<TestRoot>/Test Root Ünï cödé, <TestRoot>.run_dir())` (jadi `store_dir` di `nats.conf` memuat spasi dan Unicode, dan prefix `stackly-test-` tetap ada) dan runtime NATS 2.15.1 dari cache runtime xtask: `install_cached_artifact`, `pin_package` paket NATS, `instance.create` (create sementara spec 0010) dengan `provider.revision` `1.0.0` dari record `pin_package`, settings `{}`, dan kedua endpoint `Auto`, dan init sukses. Test berlangganan `instance.health_changed` sebelum `instance.start`, lalu membuktikan lewat urutan `statusSeq` bahwa readiness `not_ready` datang sebelum `ready` lewat `nats-readiness`, dan `instance.get` menampilkan lifecycle, desired, readiness, health, dan proses secara terpisah. `lsof` hanya menunjukkan dua listener proses itu, di host dan port endpoint `client` dan `monitoring` (loopback). `GET /varz` di endpoint `monitoring` (HTTP biasa dari test) melaporkan `server_name` sama dengan nama instance, `jetstream.config.store_dir` di bawah folder data instance, serta `max_memory` dan `max_storage` sama dengan settings default; folder `<data instance>/jetstream` ada. Baris log `Server is ready` sampai ke `LogSink` perekam yang dipasang lewat `DaemonConfig.extra_log_sinks` (spec 0010, K5), dan selama 60 detik setelah ready spool tidak bertambah satu baris per percobaan probe. `instance.stop` membuat PID itu hilang (`kill(pid, 0)` gagal `ESRCH`) dalam 10 detik, instance `stopped` dengan `lastFailure` null dan `leftoverProcesses` kosong, lalu start berikutnya `ready` lagi. Tidak ada jalur kode daemon yang berbeda untuk NATS (AC-029 bagian provider nyata, REQ-036 dasar).
- **AC-9**: Readiness jujur, di test `nats_compatibility` (lewat `SpawnedDaemon` dengan interval default): setelah `ready`, test mengirim `SIGSTOP` ke `nats-server`. Connect TCP ke endpoint `client` tetap berhasil (dibuktikan test), tetapi dalam 40 detik readiness menjadi `not_ready` dengan reason `timeout` setelah 2 kegagalan berturut turut, dan health tidak lagi `healthy`; lifecycle tetap `running` dengan PID yang sama (tidak ada restart atau stop). Setelah `SIGCONT`, readiness kembali `ready`. Guard test selalu mengirim `SIGCONT`, lalu stop instance, dan baru `SIGKILL` sebagai jalan terakhir (REQ-012, Done when "bukan TCP terbuka saja").
- **AC-10**: Identitas dan mode terhadap server asing, di test `nats_compatibility`: test menjalankan `nats-server` dari runtime terpasang yang sama, selalu dengan `-a 127.0.0.1`, `-p <port dari allocate_port>`, dan tanpa monitoring, lalu memanggil runner langsung. `-n foreign` dengan harapan `golden` menghasilkan `identity_mismatch`; `-n golden` tanpa `-js` dengan harapan JetStream aktif menghasilkan `jetstream_disabled`; `-n golden -js -sd <TestRoot>/foreign-js/store` dengan harapan tidak aktif menghasilkan `jetstream_unexpected`; `-n golden --user u --pass p` menghasilkan `auth_required`; nama dan mode yang cocok menghasilkan sukses. Setiap server asing dijaga guard yang me reap prosesnya. Lewat daemon, instance dengan settings `{ "jetstream": false }` menjadi `ready`, `GET /varz` tidak memuat `jetstream.config`, dan folder `<data instance>/jetstream` tidak dibuat (PRD 22.2 "mode JetStream yang diminta").
- **AC-11**: Di test `nats_compatibility`, `SpawnedDaemon` yang dibunuh `SIGKILL` saat instance NATS `ready` lalu dijalankan lagi di root yang sama mengadopsi `nats-server` yang sama (PID dan waktu start sama, tanpa spawn baru), `lsof -d 3` pada proses itu menunjukkan `instance.lock`, dan readiness kembali `ready` lewat `nats-readiness` ke port yang tersimpan (bukti provider nyata spec 0010 AC-9 dengan binary Go; REQ-013).
- **AC-12**: Setiap test native fitur ini memasang guard evidence `native` spec 0002 dengan `covers` `REQ-029`, `REQ-036`, dan `AC-029`, ditambah `REQ-012` untuk bagian AC-9 dan `REQ-013` untuk bagian AC-11. Test `nats_compatibility` menambah setiap check ke daftar hanya setelah assertion nya lulus, lalu menulis evidence `native` dengan `covers` `BR-025`, subject `{ provider: nats, version, digest, source: managed }` dengan `version` dari field `version` `GET /varz` server yang berjalan dan `digest` dari `ArtifactRecord.sha256`, dan `compatibility.json` di `artifact_dir()` berbentuk *Evidence kompatibilitas*. Evidence itu harus dihasilkan dari `provider.yaml` final (termasuk `runtime.versionCheck` dari langkah 1 spec 0018); perubahan paket sesudahnya mewajibkan run ulang. Urutannya: commit paket dan test, jalankan test native dari `HEAD` yang bersih, `cargo xtask runtime compat-record`, lalu satu commit berisi `evidence/compatibility/`, perpindahan entri NATS di `providers/catalog.toml` dari `[[planned]]` ke `[[provider]]` sesuai *Entri catalog*, dan `providers/catalog-revisions.json` hasil `cargo xtask catalog check --update` dengan `revision` dan `published_at` baru (aturan masuk catalog spec 0017: commit itu yang membawa bukti lifecycle). Tanpa artefak di cache, setiap test native `BLOCKED` dengan alasan `artifact nats-2.15.1-macos-aarch64 is not in the runtime cache` dan owner `@ojiepermana`, tanpa unduhan.
- **AC-13**: Journey yang sama dari CLI dan GUI, setelah fitur 20 dan 21 dibangun: `stackly instance create` untuk `official/nats` dengan nilai default dan dengan JetStream dimatikan (sintaks fitur 20), `stackly instance start`, `stackly instance inspect <nama> --json` dengan readiness `ready` di `result.instance` (bentuk hasil `{ instance, detail, dataUsage }` spec 0021; `get` hanya alias opsional), dan `stackly instance stop` lulus terhadap NATS asli lewat `SpawnedDaemon`, dengan catalog uji yang memuat `official/nats` sebagai provider. Fixture `protocols/fixtures/instance/create-form-nats.json` milik spec 0020 (jawaban `instance.get_create_form` yang dibangkitkan dari paket NATS asli) lolos test kontrak Rust dan Swift, dan test Swift render nya membuktikan model form generik merender `serverName` sebagai field managed yang terkunci, JetStream sebagai toggle, ketiga field integer advanced dengan batasnya, serta endpoint `client` dan `monitoring` dengan pilihan Fixed atau Automatic. Sheet New Instance di GUI membuat dan menjalankan instance NATS sampai Summary menampilkan readiness `ready` terpisah dari status proses, dan alur itu bisa diselesaikan dengan keyboard dan VoiceOver (AC-029 bagian form UI untuk provider nyata).
- **AC-14**: Ukuran RISK-07 tercatat dan benar. `verify.md` mencatat commit dasar (commit tepat sebelum commit build pertama fitur ini) dan daftar commit build fitur ini. Untuk setiap commit itu, baris yang ditambahkan ke file sumber non test (`git diff -U0 <commit>^ <commit>` lalu baris berawalan `+` yang memuat `nats` tanpa peduli huruf besar) hanya ada di file allowlist *Ukuran RISK-07*; file sumber non test di `apps/`, `clients/`, `crates/domain/`, `crates/platform/`, `crates/platform-macos/`, `crates/protocol/`, `crates/storage/`, dan `crates/ipc/` tidak disentuh kecuali seam test bersama di baris 4 tabel, masing masing dicatat dengan file dan alasannya; dan `ProtocolVersion::CURRENT` tidak berubah. `git diff --numstat` per baris 1, 2, dan 4 tabel dicatat, dan selisih dengan tabel kembali ke `/architect` lewat update spec ini.

Status test semua AC mulai dari `NOT_RUN`. Lulus di fixture bukan evidence native, dan artefak, sesi GUI, atau fitur prasyarat yang tidak tersedia dicatat `BLOCKED`.

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md), ditambah semua perbaikan cross check yang disetujui lead atas nama user (2026-10-20): `nats-readiness` berbicara protokol client NATS ke endpoint `client` (INFO untuk identitas dan mode, CONNECT lalu PING dan PONG, dan `$JS.API.INFO` saat JetStream diminta); nilai harapan per instance masuk lewat `params` probe yang menerima token interpolasi dan diselesaikan di plan; registry native adalah enum tertutup dengan `match` exhaustive, bukan trait; logika protokol adalah state machine tanpa IO buatan sendiri di `provider::probe::nats` dengan driver tokio generik di `supervisor::probe::native`, bukan crate client NATS; fitur ini tidak mengonfigurasi auth (milik fitur 39); RISK-07 diukur lewat tabel perubahan, `git diff --numstat`, dan pemeriksaan baris yang ditambah per commit; dan form GUI dibuktikan dengan fixture spec 0020 dari paket asli ditambah pemeriksaan manual.

Paket NATS tetap data saja. Satu satunya kode Rust yang tahu NATS adalah descriptor, state machine, dan satu cabang dispatch `nats-readiness`, sesuai kalimat scope "probe `nats-readiness` sebagai satu perubahan core yang ditinjau"; jenis `native` sendiri adalah perluasan generik yang sudah direncanakan spec 0009.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Skill async relevan untuk driver tokio (satu deadline untuk seluruh percobaan, tanpa task latar) dan test dengan waktu tokio yang dijeda.

## Feature design

### Batas fitur

- **Di dalam**: jenis probe `native` di manifest, validasi, plan, dan JSON Schema; registry `NativeProbeKind` dengan `nats-readiness`; state machine protokol, driver, dan dispatch; `ProbeSpec::Native` di loop probe supervisor; perubahan probe di paket `providers/nats` dan README nya; golden; fuzz target; test native lewat daemon dengan NATS 2.15.1 dari cache; evidence kompatibilitas; entri catalog; test journey CLI; dan assertion tambahan di test render fixture form spec 0020.
- **Di luar**: credential dan auth NATS, dua instance yang terisolasi, persistence stream setelah restart, dan bentrok port monitoring (fitur 39, AC-036); connection string NATS dan tombol copy (fitur 21); form, preflight, dan fixture form create (fitur 20); reload (fitur 26 mencatat NATS sebagai provider tanpa reload); metrics dari endpoint monitoring (fitur 28); backup JetStream (fitur 53); TLS, cluster, leafnode, gateway, websocket, dan MQTT; unduhan runtime dari jaringan (`nats_installs_from_the_network` tetap langkah 16a spec 0018); dan baris `runtime.versionCheck` NATS (langkah 1 spec 0018).

### Jenis probe native

Bentuk manifest (tambahan additive pada *Probe* spec 0009, lihat *Usulan update spec 0009*):

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
// stackly_core::provider::probe::native (tanpa IO; satu satunya modul engine yang menyebut NATS)
#[non_exhaustive]
pub enum NativeProbeKind { NatsReadiness }

pub struct NativeProbeDescriptor {
    pub name: &'static str,                          // "nats-readiness"
    pub endpoint_transports: &'static [EndpointTransport],
    pub params: &'static [NativeParamDescriptor],
}
pub struct NativeParamDescriptor { pub name: &'static str, pub kind: NativeParamKind }
pub enum NativeParamKind { Text { max_bytes: usize }, Boolean }
pub enum NativeParamValue { Text(String), Boolean(bool) }   // sesudah interpolasi

#[non_exhaustive]
pub enum NativeProbe { NatsReadiness { server_name: String, jetstream: bool } }

impl NativeProbeKind {
    pub const ALL: &'static [NativeProbeKind] = &[NativeProbeKind::NatsReadiness];
    pub fn descriptor(self) -> &'static NativeProbeDescriptor;   // match exhaustive
    pub fn from_name(name: &str) -> Option<NativeProbeKind>;
}
/// Mengubah param yang sudah divalidasi dan diinterpolasi menjadi probe bertipe (match exhaustive).
pub fn build(kind: NativeProbeKind, params: &[(&str, NativeParamValue)]) -> NativeProbe;

// stackly_core::provider::plan (tambahan generik)
pub enum ProbeSpec { Tcp { .. }, Http { .. }, Exec(RunSpec), Native(NativeProbeSpec) }
pub struct NativeProbeSpec { pub target: ProbeTarget, pub probe: NativeProbe }
```

- `ProbeDef::Native { kind, endpoint, params }` di `manifest.rs` menyimpan nilai param yang sudah divalidasi sebagai literal atau ekspresi interpolasi; plan builder menginterpolasi, mengecek batas teks, lalu memanggil `native::build`.
- `NativeProbe` di `Serialize` dengan tag `name` (nilai `nats-readiness`) dan konten `params` ber key camelCase sama dengan manifest.
- Endpoint `unix` untuk probe native belum diterima (descriptor `nats-readiness` hanya `tcp`); `ProbeTarget::Unix` tetap ada untuk probe berikutnya.

### Paket NATS

Perubahan fitur ini di `providers/nats/provider.yaml` adalah blok `readiness` di bawah, dan blok `connection` dari spec 0021 (entri `uri` `nats://${endpoint.client.address}` berjudul "Client URL" dan entri `monitoring` `http://${endpoint.monitoring.address}` berjudul "Monitoring URL", tanpa credential sampai fitur 39), yang ditulis di commit yang sama supaya `manifest_sha256` hanya berubah sekali sebelum evidence BR-025:

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
- `timeouts`, endpoint `client` (4222) dan `monitoring` (8222), template `nats.conf.tmpl`, settings schema, stop `gracefulProcess` `term`, capability, dan `permissions` tidak berubah. `metadata.revision` tetap `1.0.0`; `manifest_sha256` berubah karena isi manifest berubah. Baris `runtime.versionCheck` ditambah langkah 1 spec 0018 dan tidak disentuh di sini.
- `serverName` adalah field `managed` (`${instance.name}`), jadi nilai yang dirender ke `server_name` dan nilai yang diharapkan probe datang dari ekspresi yang sama di plan yang sama.
- `README.md` diganti: readiness lewat `nats-readiness` (identitas, mode JetStream, PING, dan API JetStream), client bernama `stackly-readiness` yang terhubung setiap 10 detik, instance tanpa auth di loopback sampai fitur 39, monitoring hanya di loopback, dan store JetStream di `<data>/jetstream`.

### Algoritma satu percobaan

`provider::probe::nats::Exchange` adalah state machine tanpa IO: ia menerima byte dari server dan mengembalikan byte untuk ditulis, atau hasil akhir (`Ok` atau `NativeProbeFailure { reason: ReasonCode, detail: ObservationDetail }`). Driver generik `supervisor::probe::native` memegang IO: ia memanggil fungsi connect (yang bisa diganti di test) untuk mendapat stream `AsyncRead + AsyncWrite`, lalu memindahkan byte antara stream dan `Exchange`. Satu deadline, `now + attempt_timeout`, berlaku untuk seluruh percobaan; setiap connect, baca, dan tulis memakai sisa deadline itu, dan tidak ada task latar.

1. Connect TCP ke `target` (selalu loopback, dijaga plan spec 0009). Error: `connect_failed`. Deadline lewat: `timeout`.
2. Baca satu baris yang berakhir `\r\n`. Baris harus diawali `INFO` (nama operasi tanpa peduli huruf besar), satu atau lebih spasi atau tab, lalu tepat satu object JSON tanpa byte lain sesudahnya. Field yang dibaca: `server_name` (string), `server_id` (string), `version` (string), `jetstream`, `auth_required`, `tls_required`, `ldm` (boolean); field lain diabaikan, field boolean yang tidak ada dibaca `false`, dan `server_name` yang tidak ada dilaporkan sebagai `identity_mismatch` di langkah 3. Nilai yang bukan object, field yang dibaca tetapi tipenya salah, atau byte setelah object: `protocol_error`.
3. Pemeriksaan berurutan, hasil gagal pertama yang menang: `server_name` tidak ada atau berbeda dengan `server_name` harapan (perbandingan byte persis) `identity_mismatch`; `tls_required` `tls_required`; `auth_required` `auth_required`; `ldm` `lame_duck`; `jetstream` `false` padahal harapan `true` `jetstream_disabled`; `jetstream` `true` padahal harapan `false` `jetstream_unexpected`.
4. Tulis tepat `CONNECT {"verbose":false,"pedantic":false,"tls_required":false,"name":"stackly-readiness","lang":"rust","version":"<versi crate stackly-core>","protocol":1,"echo":false,"headers":false,"no_responders":false}\r\nPING\r\n` (urutan field persis ini, tanpa spasi di JSON).
5. Baca baris sampai `PONG`. Selama menunggu (juga di langkah 7): `PING` dijawab `PONG\r\n`, `INFO` dan `+OK` diabaikan, `-ERR <teks>` menjadi `protocol_error` dengan teks itu, dan baris lain menjadi `protocol_error`.
6. Harapan JetStream `false`: sukses, tutup koneksi.
7. Harapan `true`: buat inbox `_INBOX.stackly.<32 hex>` dari 16 byte `getrandom`, tulis `SUB <inbox> 1\r\nPUB $JS.API.INFO <inbox> 0\r\n\r\n`, lalu baca sampai baris `MSG <inbox> 1 <n>` (argumen dipisah spasi atau tab; subject atau sid lain `protocol_error`; `n` di atas `NATS_REPLY_MAX` `protocol_error`), baca `n` byte payload dan `\r\n`, lalu parse object JSON. Object dengan field `error` menjadi `jetstream_unavailable` dengan `code` dan `description` nya; `type` selain `io.nats.jetstream.api.v1.account_info_response` juga `jetstream_unavailable`; selain itu sukses. Tutup koneksi.
8. EOF, `ECONNRESET`, atau `EPIPE` di langkah mana pun setelah connect: `protocol_error` (`connection closed while waiting for <x>` atau `connection reset while waiting for <x>`). Deadline lewat: `timeout`. Target `ProbeTarget::Unix` (yang sudah ditolak validasi) dijawab `protocol_error` `nats-readiness supports tcp endpoints only` tanpa connect.

Batas (konstanta di `provider::probe::nats`, bukan `SupervisorConfig`, karena ini batas protokol): `NATS_LINE_MAX` 16 KiB per baris, `NATS_REPLY_MAX` 64 KiB per payload, `NATS_READ_MAX` 128 KiB total baca per percobaan (lewat: `protocol_error`), `NATS_DETAIL_VALUE_MAX` 64 karakter untuk setiap nilai dari server di detail.

**Hasil gagal** (kode `ReasonCode` snake_case; generik supaya probe native berikutnya bisa memakai ulang yang cocok):

| Kode | Arti | Detail |
| --- | --- | --- |
| `connect_failed` | connect TCP ditolak atau error | `connect to <host:port> failed: <jenis error io>` |
| `timeout` | deadline percobaan lewat, per fase | `connect to <host:port> timed out after <n> ms`, `no INFO from the server within <n> ms`, `no PONG from the server within <n> ms`, `no reply to $JS.API.INFO within <n> ms` |
| `protocol_error` | balasan yang tidak sesuai protokol, termasuk `-ERR`, EOF, dan reset | `expected INFO, got "<cuplikan>"`, `server error: <teks -ERR>`, `connection closed while waiting for PONG` |
| `identity_mismatch` | server lain di port itu | `server_name is "<nilai>", expected "<harapan>"` atau `server_name is missing, expected "<harapan>"` |
| `tls_required` | server menuntut TLS, yang tidak pernah dikonfigurasi paket ini | `the server requires TLS` |
| `auth_required` | server menuntut auth, yang tidak pernah dikonfigurasi paket ini | `the server requires authentication` |
| `lame_duck` | server sedang lame duck (menuju berhenti) | `the server is in lame duck mode` |
| `jetstream_disabled` | JetStream mati padahal diminta | `JetStream is off, the instance asks for it` |
| `jetstream_unexpected` | JetStream aktif padahal tidak diminta | `JetStream is on, the instance asks for it off` |
| `jetstream_unavailable` | API JetStream menjawab error atau jenis balasan lain | `$JS.API.INFO error <code>: <description>`, `$JS.API.INFO replied with type "<type>"` |

Detail ditulis dalam bahasa Inggris, paling panjang 256 karakter (`ObservationDetail` spec 0004); setiap nilai dari server (nama, teks `-ERR`, `description`, `type`, cuplikan baris) yang lebih dari 64 karakter dipotong menjadi 61 karakter ditambah `...`, dan karakter kontrol (C0, DEL, C1) diganti `?`. Runner spec 0010 menyamarkan setiap detail lewat `Redactor` sebelum menjadi `ObservationDetail`, termasuk detail probe native (K3); probe ini sendiri tidak pernah mengirim atau menerima secret di fitur ini (fitur 39 meninjau ulang saat auth masuk). `lame_duck` hanya bisa muncul pada server yang mengirim `ldm` di INFO; NATS yang masuk lame duck menutup listener nya, jadi untuk server asli hasil yang terlihat adalah `connect_failed`, dan kode ini hanya diuji di fixture.

### Hasil probe dan loop supervisor

- `supervisor::probe` (modul folder, langkah 6 spec 0010, K3) memakai `ProbeOutcome::{Ok, Failed { reason: ReasonCode, detail: Option<ObservationDetail> }}`. Kode kegagalan spec 0010 adalah `connect_failed`, `timeout`, `protocol_error`, `unexpected_status` (hanya `http`), dan `check_failed` (hanya `exec`); probe native memakai `connect_failed`, `timeout`, dan `protocol_error` dengan arti yang sama, ditambah kode khusus di tabel *Hasil gagal*, dan tidak pernah memakai `unexpected_status` atau `check_failed`.
- `ProbeOutcome::Ok` menjadi observasi dengan reason `probe_ok` (kode yang sudah dipakai test domain `observation.rs`).
- Arm `ProbeSpec::Native` di match `supervisor::probe` ditambah langkah 1 (K3) dan memanggil `supervisor::probe::native::run(&NativeProbeSpec, attempt_timeout)`. Sampai langkah 5, `run` mengembalikan `ProbeOutcome::Failed` dengan reason `protocol_error` dan detail `native probe driver is not built yet` (`// provisional: F23 langkah 5`); belum ada paket yang memakai probe native di instance yang berjalan sebelum milestone 2. Mulai langkah 5, `run` memilih state machine lewat `match` exhaustive atas `NativeProbe` (satu cabang `NatsReadiness`), lalu memanggil driver generik, dan reason di tabel *Hasil gagal* menjadi reason observasi lewat runner spec 0010 (yang juga menyamarkan detailnya).

### Test native

| Test | Daemon | Membuktikan | Seam yang dipakai |
| --- | --- | --- | --- |
| `nats_lifecycle_in_process` (`apps/daemon/tests/nats.rs`) | in process, `stacklyd::server::start` | AC-8 | `install_cached_artifact` (spec 0018 langkah 9), `supervisor::fixture::pin_package` (fitur `fixture` core, spec 0010 langkah 13, K5), dan `LogSink` perekam lewat `DaemonConfig.extra_log_sinks` (spec 0010 langkah 13, K5) |
| `nats_compatibility` (`apps/daemon/tests/nats.rs`) | `SpawnedDaemon`, interval default | AC-9, AC-10, AC-11, evidence AC-12 | runtime dipasang `install_cached_artifact` dan paket dipin `pin_package` (K5) di root yang sama sebelum `SpawnedDaemon` start (registry ditutup dulu, karena hanya satu writer); runner dipanggil langsung untuk server asing |
| `nats_journey` (`apps/cli/tests/nats_journey.rs`) | `SpawnedDaemon` | AC-13 bagian CLI | runtime dari cache seperti di atas, dan catalog uji berisi `official/nats` sebagai provider yang dibangun `stackly_core::catalog::testing::CatalogBuilder` (fitur `test-support` core, spec 0017) di `TestRoot` dan diberikan ke daemon lewat `STACKLY_CATALOG_DIR` |

- Root data test daemon: `RootSet::under_override(<TestRoot>/Test Root Ünï cödé, <TestRoot>.run_dir())`, fungsi yang juga dipakai `STACKLY_ROOT`; untuk `SpawnedDaemon`, `STACKLY_ROOT` menunjuk folder Unicode yang sama.
- `nats_compatibility` berjalan berurutan dan menambah check hanya setelah assertion nya lulus: `init` dan `start` (create dan start sampai `ready`), `readiness` (urutan `statusSeq`), `honest_readiness` (AC-9), `identity_check` dan `jetstream_mode` (AC-10, termasuk instance dengan JetStream mati), `adoption` (AC-11), lalu `stop`. Kalau satu check gagal, test gagal dan `compatibility.json` tidak ditulis.
- Setiap `nats-server` yang dijalankan test sendiri (server asing) dan setiap instance yang di `SIGSTOP` dijaga guard yang me reap prosesnya, mengikuti aturan test AGENTS.md; `cargo xtask runtime spike --cleanup` tetap menemukan sisa proses lewat argv yang memuat `/stackly-test-`.
- Ketiga test bertanda `#[ignore]` dan dijalankan dengan `--ignored` seperti test native spec 0019; tanpa artefak di cache mereka `BLOCKED` (AC-12).

### Ukuran RISK-07

| No | Perubahan | Jenis | Tempat | Dipakai provider lain |
| --- | --- | --- | --- | --- |
| 1 | Jenis probe `native`: field manifest, validasi param, posisi interpolasi baru, `ProbeDef::Native`, `ProbeSpec::Native`, JSON Schema, arm `ProbeSpec::Native` di `supervisor::probe` (K3), driver generik, dan dispatch (`ProbeOutcome` milik spec 0010) | generik, direncanakan spec 0009 | `crates/core/src/provider/` (`raw.rs`, `validate.rs`, `manifest.rs`, `plan.rs`, `probe/mod.rs`, `probe/native.rs` bagian generik), `crates/core/src/supervisor/probe/` | ya, setiap probe native berikutnya |
| 2 | `nats-readiness`: varian, descriptor, `build`, state machine `Exchange`, cabang dispatch | khusus NATS, satu perubahan core yang diizinkan scope | `crates/core/src/provider/probe/native.rs` (bagian varian), `crates/core/src/provider/probe/nats.rs`, `crates/core/src/supervisor/probe/native.rs` (satu cabang `match`) | tidak |
| 3 | Fuzz target `nats_readiness_protocol` dan harness nya | khusus NATS, test dan tooling | `fuzz/fuzz_targets/`, harness di `crates/core/src/provider/fuzzing.rs`, baris `TARGETS` di `xtask/src/fuzz.rs` | tidak |
| 4 | Seam test bersama yang dipakai, dibangun spec 0010 langkah 13 (K5), bukan di sini: `pin_package` dan `DaemonConfig.extra_log_sinks` | generik, test | `crates/core/src/supervisor/fixture.rs` di balik fitur `fixture`; `stacklyd::server::DaemonConfig` | ya |

Tidak ada perubahan produk di daemon, CLI, protokol IPC, storage, platform, domain, atau Swift. Data yang berubah: paket `providers/nats`, `providers/catalog.toml`, `providers/catalog-revisions.json`, `schemas/provider/` (dibangkitkan), golden, fixture, dan evidence. Sebelum fitur ini, kode yang sudah tahu NATS adalah format template `natsConf` (spec 0009, termasuk satu pesan di `validate.rs`) dan resep spike xtask (spec 0003); keduanya tidak ditambah. Pembanding: PostgreSQL (spec 0019) butuh empat tambahan core generik dan format detail probe `exec`, tanpa kode khusus PostgreSQL di luar nama format.

**Allowlist AC-14** (file sumber non test yang boleh menerima baris baru yang memuat `nats`): `crates/core/src/provider/probe/native.rs`, `crates/core/src/provider/probe/nats.rs`, `crates/core/src/provider/probe/mod.rs` (deklarasi `mod`), `crates/core/src/provider/fuzzing.rs`, `crates/core/src/supervisor/probe/native.rs`, dan `xtask/src/fuzz.rs`.

### Entri catalog

Di commit evidence AC-12, `providers/catalog.toml` menghapus `[[planned]]` `official/nats` dan menambah:

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

`revision` naik satu dari revision terbesar di ledger `providers/catalog-revisions.json` saat commit, `published_at` adalah waktu UTC commit itu, lalu `cargo xtask catalog check --update`. Kalau commit PostgreSQL (fitur 19) masuk lebih dulu atau bersamaan, commit yang datang belakangan mengambil revision berikutnya setelah rebase.

### Evidence kompatibilitas

```json
{
  "schema": "stackly.compatibility-evidence/v1",
  "providerRevision": { "id": "official/nats", "revision": "1.0.0", "manifestSha256": "<hex>" },
  "checks": ["adoption", "honest_readiness", "identity_check", "init", "jetstream_mode", "readiness", "start", "stop"],
  "knownLimitations": ["client_auth_not_configured", "jetstream_persistence_not_covered", "multi_instance_not_covered"]
}
```

`checks` urut abjad tanpa duplikat (aturan spec 0018). `knownLimitations` menyebut apa yang belum dibuktikan sampai fitur 39: auth client, persistence stream setelah restart, dan dua instance yang terisolasi, walau paket mendeklarasikan capability `persistence` dan `multiInstance`. `manifestSha256` adalah digest `provider.yaml` final yang masuk catalog.

### Fuzz target

`provider::fuzzing::nats_readiness_protocol(data: &[u8])`: kalau `data` kosong, selesai. Bit 0 byte pertama memilih harapan JetStream; nilai byte pertama digeser satu bit ditambah 1 (1 sampai 128) adalah ukuran potongan; sisa byte diberikan ke `Exchange` sebagai byte dari server dalam potongan sebesar itu, dengan harapan `server_name` tetap `golden`. Harness berhenti saat `Exchange` selesai. `max_len` target 262144 byte, supaya batas `NATS_READ_MAX` 128 KiB bisa tercapai. Seed: transcript di `crates/core/tests/fixtures/nats-protocol/` (dari langkah 0 dan dari test transcript), ditulis ulang `cargo xtask fuzz seed`.

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
| `NativeProbeKind::{ALL, descriptor, from_name}`, `native::build` | `core::provider::probe::native` | nama; param tervalidasi | descriptor; `NativeProbe` | validator, plan builder, test | tidak ada |
| cabang `native` di `validate::probe` | `core::provider::validate` | `RawProbe` | `ProbeDef::Native` | `ValidatedPackage::from_files` | issue di tabel *Jenis probe native* |
| `ProbeSpec::Native` di `run_plan` dan `reload_plan` | `core::provider::plan` | `PlanContext` | `NativeProbeSpec` | spec 0010 | `UnrepresentableValue`, `EndpointUnresolved` |
| `nats::Exchange` | `core::provider::probe::nats` | byte dari server, harapan | byte untuk ditulis, hasil | driver, fuzz, test | `NativeProbeFailure` |
| `supervisor::probe::native::run` dan driver | `core::supervisor::probe` | `&NativeProbeSpec`, `attempt_timeout`, fungsi connect | `ProbeOutcome` | loop readiness dan health spec 0010 | hasil gagal di tabel |
| `provider::fuzzing::nats_readiness_protocol(&[u8])` | core, `#[doc(hidden)]` | byte | tidak ada | fuzz target, regresi | tidak ada |
| paket `providers/nats` | repo | file paket | `ValidatedPackage` | catalog, create, test | `manifest.invalid` |
| test native | `apps/daemon/tests/nats.rs` | runtime dari cache | evidence `native`, `compatibility.json` | `cargo test -p stacklyd --test nats -- --ignored` | `BLOCKED` tanpa artefak |
| test journey CLI | `apps/cli/tests/nats_journey.rs` | `SpawnedDaemon`, runtime dari cache, catalog uji | evidence `native` | `cargo test -p stackly-cli --test nats_journey -- --ignored` | `BLOCKED` tanpa artefak atau fitur 20 |

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
| probe | reason dan detail | hasil `Exchange` atau driver; nilai dari server disanitasi |
| observasi | state, reason (`probe_ok` saat sukses), detail, `observed_at` | `ProbeOutcome` dan `Clock::now()` (spec 0010) |
| create (test) | provider, runtime, settings, endpoint | `instance.create` create sementara spec 0010 (K1): `provider: { name: "official/nats", revision: "1.0.0" }` dari `ProviderRevisionRecord` hasil `pin_package` (revision wajib selama create sementara berlaku), `runtimeId` dari `Runtime` hasil `install_cached_artifact`, `settings: {}` atau `{ "jetstream": false }`, dan kedua endpoint default `auto` |
| install (test) | file artefak | `stackly_testkit::runtime_cache::cached_artifact(sha256)` (spec 0018), sha256 dari `versions.json` |
| test | root data Unicode | `RootSet::under_override(<TestRoot>/Test Root Ünï cödé, <TestRoot>.run_dir())` |
| test | urutan readiness | `statusSeq` di `instance.health_changed` |
| test | port server asing | `stackly_testkit::allocate_port` |
| catalog | `revision`, `published_at` | revision terbesar di `providers/catalog-revisions.json` ditambah satu; waktu UTC commit |
| evidence | `version` subject | field `version` `GET /varz` server yang berjalan (versi dari binary yang berjalan, spec 0002) |
| evidence | `digest` subject | `ArtifactRecord.sha256` |
| evidence | `providerRevision` | `ValidatedPackage::key()` paket final |
| AC-14 | commit dasar, daftar commit, baris yang ditambah, numstat | `git log` dan `git diff -U0 <commit>^ <commit>` per commit build, dicatat di `verify.md` |

### Edge cases dan kegagalan

| Kasus | Yang terjadi | Penanganan |
| --- | --- | --- |
| Port `Fixed` dipakai NATS milik pengguna (misalnya 4222) | spec 0010 (K2): bind percobaan memakai `SO_REUSEADDR` | listener di alamat yang sama persis memberi `supervisor.endpoint_in_use` sebelum spawn; NATS pengguna yang listen di wildcard (`0.0.0.0:4222`, default upstream) tidak menghalangi start (hanya caution preflight fitur 20), dan cek identitas probe memastikan yang menjawab di loopback adalah server instance; endpoint `Auto` menghindari semuanya |
| Server lain mengambil port setelah instance keluar | probe membaca `server_name` lain | `identity_mismatch`, readiness `not_ready`; proses instance sendiri diurus spec 0010 |
| Proses berhenti merespons (tertahan, macet) | connect TCP tetap berhasil lewat backlog kernel | `timeout`, `not_ready` setelah 2 kegagalan, tanpa restart (AC-9) |
| JetStream lambat pulih setelah crash (store besar) | `$JS.API.INFO` error 503 atau tidak menjawab | `jetstream_unavailable` atau `timeout` sampai pulih; deadline `recoverySeconds` habis membuat operasi gagal sementara proses tetap jalan (spec 0010) |
| JetStream dimatikan NATS saat berjalan (misalnya disk penuh), kalau langkah 0 membuktikan perilaku itu ada | API JetStream menjawab error | `jetstream_unavailable`, readiness `not_ready` dengan detail |
| Pengguna mematikan JetStream lewat settings | restart (`apply: restart`) | probe mengharapkan mode mati; data di `<data>/jetstream` tidak disentuh |
| Path data berisi `"`, `\`, atau `$` | escaper `natsConf` menolak | `UnrepresentableValue` saat plan (spec 0009, tidak berubah) |
| Instance diganti nama saat berjalan, lalu daemon restart dan mengadopsi proses | plan probe baru memakai nama baru, proses masih memakai nama lama | `identity_mismatch` sampai restart; dicatat sebagai Follow-up untuk fitur 29 dan 30 |
| NATS masuk lame duck (`SIGUSR2` dari luar) | listener ditutup, lalu proses keluar | `connect_failed`, lalu restart policy spec 0010 untuk exit yang tidak diminta |
| Server menutup atau me reset koneksi di tengah exchange | baca menghasilkan EOF atau error | `protocol_error` dengan detail `connection closed while waiting for <x>` atau `connection reset while waiting for <x>` |
| Klien lain berlangganan `_INBOX.>` | melihat balasan account info | tidak sensitif; inbox acak per percobaan mencegah tabrakan |
| Artefak tidak ada di cache | test native | `BLOCKED` dengan alasan dan owner di AC-12 |
| Paket berubah setelah evidence direkam | `manifest_sha256` evidence basi | run ulang `nats_compatibility` dan `compat-record` sebelum commit catalog (AC-12) |

### Asumsi pada spec lain

Dicek ulang terhadap `main` setelah perbaikan konsistensi spec 0015, 0017, 0018, dan 0019 (2026-10-20).

- **Assumed (spec 0009)**: jenis `native`, registry, param bertoken, dan `ProbeSpec::Native` sudah tertulis di spec 0009 (*Perluasan additive fase 02*, 2026-10-20); kontrak di spec ini dan di spec 0009 sama.
- **Assumed (spec 0010)**, diselaraskan dengan keputusan K foundation 2026-10-20: modul folder `core::supervisor::probe` dengan runner `tcp`, `http`, dan `exec`, `ProbeOutcome`, reason `probe_ok`, `connect_failed`, `timeout`, `protocol_error`, `unexpected_status`, dan `check_failed`, redaksi setiap detail lewat `Redactor`, dan dispatch native ke `supervisor::probe::native::run` (langkah 6 spec 0010, K3); core dengan tokio fitur `net` dan `io-util`; readiness rutin dengan debounce; create sementara (K1) yang menolak `provider.revision` kosong selama berlaku; `pin_package` di fitur `fixture` core dan `DaemonConfig.extra_log_sinks` (langkah 13 spec 0010, K5); bind percobaan port dengan `SO_REUSEADDR` (K2); adopsi lewat lock di fd 3; dan `instance.health_changed` dengan `statusSeq`.
- **Assumed (spec 0018)**: `stackly_core::runtime::fixture::install_cached_artifact(platform, layout: &PreparedLayout, registry, package: &ValidatedPackage, artifact_id, cache_file)` (langkah 9, milestone 2) yang meng commit record sendiri dan menjalankan exec check kalau paket mendeklarasikan `runtime.versionCheck`; `stackly_testkit::runtime_cache::cached_artifact(sha256)`; `cargo xtask runtime compat-record` yang hanya menyalin record BR-025 dari `HEAD` yang bersih (langkah 13, milestone 3); bentuk `compatibility.json`; dan baris `runtime.versionCheck` NATS dari langkah 1 (milestone 1), dicocokkan per baris setelah trim.
- **Assumed (spec 0019)**: aturan evidence spec 0019 (versi dari binary yang berjalan, evidence dari `provider.yaml` final) diikuti. `pin_package` dan `LogSink` perekam sekarang milik spec 0010 langkah 13 (K5), bukan spec 0019.
- **Assumed (spec 0017)**: `providers/catalog.toml` (TOML snake_case, `published_at`) dan ledger `providers/catalog-revisions.json` dari milestone 1 spec 0017, dengan NATS sebagai `[[planned]]`; `stackly_core::catalog::testing::CatalogBuilder` di fitur `test-support` untuk catalog uji.
- **Assumed (spec 0020, dikonfirmasi sesi `arc-f20-spec` 2026-10-20 dari draf nya)**: daemon membangun form create dari paket lewat `instance.get_create_form { runtimeId }`: satu baris port Fixed atau Automatic per endpoint tcp atau udp (NATS mendapat dua, `client` dan `monitoring`), dan field dari settings schema dalam urutan deklarasi dengan kelas VAL-012 dan kind text, integer, boolean, choice, dan list (NATS hanya memakai text, integer, dan boolean); preflight memeriksa setiap endpoint IP, menjalankan `SettingsSchema::validate`, dan dry run `init_plan` serta `run_plan` (jadi `UnrepresentableValue` param native muncul sebelum create); instance baru memakai revision catalog yang dimuat, sehingga NATS baru bisa dibuat dari GUI setelah menjadi `[[provider]]` (milestone 2 di sini), selain itu `instance.provider_unavailable`; spec 0020 membangkitkan `protocols/fixtures/instance/create-form-nats.json` dari `providers/nats` dan test Swift merendernya; `stackly instance create` menerima settings dan endpoint dengan sintaks fitur 20. Kalau spec 0020 yang di merge berbeda, AC-13 mengikuti spec 0020 lewat update spec ini sebelum milestone 3.

### Jawaban follow-up dari spec lain

- **Spec 0009** ("Fitur 23: tambah jenis probe `native`, registry `NativeProbeDescriptor`, dan `nats-readiness` (cek JetStream dan identitas `server_name`) sesuai kontrak di *Probe*, lalu ganti probe `http` di `providers/nats`"): dijawab *Jenis probe native*, *Algoritma satu percobaan*, dan *Paket NATS*. Dua penyesuaian kontrak (param bertoken dan `match` exhaustive menggantikan trait) ada di *Usulan update spec 0009*.
- **Spec 0017** (*Aturan masuk catalog*): dijawab AC-12 dan *Entri catalog*; perpindahan NATS ada di commit yang membawa evidence lifecycle ujung ke ujung, bersama ledger.
- **Spec 0010** ("Bukti dengan provider nyata milik fitur 19, 23, 26, dan 30"): AC-8, AC-9, dan AC-11 adalah bukti native untuk startup transaction, readiness dan readiness rutin dengan debounce, stop tanpa `SIGKILL`, render sebelum spawn, dan adopsi lewat lock di fd 3 dengan binary Go (berbeda runtime dari PostgreSQL).
- **Spec 0018** (satu pipeline install untuk semua provider): test native memasang NATS dari cache lewat `install_cached_artifact`, pipeline yang sama dengan provider lain, termasuk exec check `runtime.versionCheck`; `nats_installs_from_the_network` tetap milik langkah 16a spec 0018 dan tidak diulang; `runtime.versionCheck` NATS milik langkah 1 spec 0018 dan menjadi prasyarat langkah evidence di sini.
- **Spec 0019** ("Kontrak probe native tetap milik fitur 23"; pola paket, golden, evidence): kontrak itu ditulis di spec ini. Pola paket, golden, test native lewat daemon, `compatibility.json`, versi evidence dari binary yang berjalan, dan urutan versionCheck sebelum evidence mengikuti spec 0019. NATS butuh probe native karena artefak `nats-server` tidak membawa CLI client apa pun untuk probe `exec`, dan `/healthz` tidak bisa memeriksa identitas.

### Usulan update spec 0009

Diterapkan di spec 0009 pada 2026-10-20 (sesi `upd-0009`).

Satu usulan yang berdiri sendiri, untuk digabung lead dengan usulan spec 0017, 0018, dan 0019 menjadi satu item `architect (update)` spec 0009 setelah fitur 9 ditutup. Semuanya additive; tidak ada perilaku paket yang sudah ada yang berubah.

1. **AC-4 dan himpunan tertutup**: jenis probe menjadi `tcp`, `http`, `exec`, dan `native`; `native` hanya dengan nama dari registry, dan nama lain ditolak `unknown_probe` saat validasi.
2. ***Probe***: bentuk `type: native` dengan `name`, `endpoint`, dan `params`, aturan validasi, dan kind param (`text { max_bytes }`, `boolean`) persis seperti *Jenis probe native* di spec ini. Registry di `core::provider::probe::native`: `NativeProbeKind`, `NativeProbeDescriptor`, `NativeParamDescriptor`, `NativeParamKind`, `NativeParamValue`, `NativeProbe`, dan `build`. Kalimat "runner lewat trait milik spec 0010" diganti "runner dipilih lewat `match` exhaustive atas `NativeProbe` di `core::supervisor::probe::native`", karena compiler lalu menjamin setiap nama yang lolos validasi punya runner.
3. ***Peta modul***: submodul baru `probe` (`native`, `nats`) di `core::provider`. `probe::nats` memuat state machine protokol `nats-readiness` yang tetap tanpa IO, jadi invariant "engine provider tidak pernah menjalankan proses, menulis file, atau menyentuh jaringan" tetap berlaku; IO nya di `core::supervisor::probe`.
4. ***Interpolasi terkontrol***: baris baru "nilai `params` probe native" di matriks posisi, sama dengan tabel *Jenis probe native*: param teks menerima teks literal ditambah token teks, integer, atau boolean (dirender sebagai teks, seperti entri `args`); param boolean menerima tepat satu token boolean tanpa teks lain; folder, root runtime, dan executable tidak (`token_not_allowed_here`), secret tidak (`secret_not_allowed_here`).
5. ***Settings schema***: aturan field `apply: reload` mencakup `params` probe (`settings_schema_rule`).
6. ***Execution plan***: `ProbeSpec::Native(NativeProbeSpec { target: ProbeTarget, probe: NativeProbe })`, bentuk `Serialize` di AC-3 spec ini, dan `PlanError::UnrepresentableValue { field }` untuk param teks yang kosong, terlalu panjang, atau memuat karakter kontrol setelah render.
7. ***Sumber timeout dan exit code***: baris `ProbeSpec::Native`: percobaan `attemptTimeoutMilliseconds` blok yang memakainya, tanpa exit code.
8. ***Fuzz target***: `nats_readiness_protocol` (milik spec 0023, harness di `provider::fuzzing`).
9. ***Paket contoh NATS***: readiness memakai `nats-readiness`; keputusan kecil "Probe readiness NATS memakai `/healthz`" ditandai diganti spec 0023.
10. ***Critical test scenarios*** "Himpunan tertutup": probe `native` dengan nama yang tidak ada di registry, bukan `type: native` secara umum.
11. ***Follow-up***: baris "Fitur 23: tambah jenis probe `native` ..." dicentang dengan rujukan ke spec 0023.

### Usulan update spec 0010

Diterapkan di spec 0010 pada 2026-10-20 (keputusan K3 dan K5 lead foundation, sesi `upd-0010`): `ProbeOutcome`, reason `probe_ok`, kode generik (ditambah `unexpected_status` dan `check_failed`), redaksi setiap detail termasuk probe native, dispatch native, dan baris bukti provider nyata.

Untuk item `architect (update)` spec 0010 berikutnya; tidak mengubah keputusan spec 0010.

1. ***Probe, readiness, dan health***: hasil runner berbentuk `ProbeOutcome::{Ok, Failed { reason: ReasonCode, detail: Option<ObservationDetail> }}`; sukses menjadi reason `probe_ok`; kode kegagalan generik `connect_failed`, `timeout`, dan `protocol_error` untuk runner `tcp` dan `http` kalau build spec 0010 belum memilih kode lain.
2. Dispatch `ProbeSpec::Native` ke `supervisor::probe::native::run`, dengan driver generik di atas stream `AsyncRead + AsyncWrite` dari fungsi connect yang bisa diganti di test (supaya test timing memakai `tokio::io::duplex` di bawah waktu dijeda), satu deadline per percobaan, dan tanpa task latar.
3. Baris "Bukti dengan provider nyata" menyebut AC-8, AC-9, dan AC-11 spec 0023.

### Key invariants

- Readiness `ready` untuk instance NATS hanya dari percobaan yang melihat `server_name` instance itu, mode JetStream yang diminta, PONG, dan (kalau diminta) balasan API JetStream tanpa error.
- Probe hanya terhubung ke endpoint loopback milik instance, tidak pernah mengirim credential, dan hanya berlangganan inbox acak nya sendiri dengan `echo` mati.
- Setiap byte dari server dibaca dengan batas (`NATS_LINE_MAX`, `NATS_REPLY_MAX`, `NATS_READ_MAX`) dan satu deadline per percobaan; tidak ada koneksi yang bertahan melewati percobaan.
- Paket NATS tetap data saja; satu satunya kode khusus NATS di jalur produk adalah varian, descriptor, state machine, dan satu cabang dispatch `nats-readiness`.
- Registry native tertutup: nama yang lolos validasi selalu punya descriptor, `build`, dan runner, dijamin compiler.
- Evidence BR-025 NATS selalu berasal dari `provider.yaml` yang sama dengan yang masuk catalog.

### Security model

- Ancaman yang dijaga: proses lain di port instance yang tampil ready (dijawab cek identitas), port terbuka tanpa server yang melayani (PING dan PONG), JetStream yang tidak sesuai permintaan (cek mode dan API), byte jahat dari server di port itu (batas baca, state machine yang di fuzz, sanitasi detail), dan monitoring yang terbuka ke LAN (host endpoint loopback dijaga plan dan dibuktikan `lsof` di AC-8).
- Residual risk yang dinyatakan: instance NATS fitur ini tanpa auth, jadi proses lokal mana pun (termasuk milik user lain di Mac ini) bisa terhubung ke port client dan monitoring di loopback, seperti default upstream `nats-server`. Credential milik fitur 39 (REQ-036), yang juga memperluas probe secara additive. Cek identitas lewat `server_name` membuktikan server NATS bernama instance itu, bukan proses itu sendiri; identitas proses tetap tugas spec 0010.
- Tidak ada `unsafe` baru, tidak ada dependency baru.

### Configuration required

Tidak ada variabel environment, credential, atau port baru untuk daemon dan CLI. Test native butuh artefak `nats-2.15.1-macos-aarch64` (sha256 `ed0fe39222ec2e317c4d0e1a9cdc16c63ec523d94f9355d24a902a4db2655ffd`) di cache runtime xtask; per 2026-10-20 file itu sudah ada di Mac ini. Tidak ada test fitur ini yang mengunduh dari jaringan.

### Critical test scenarios

- Validasi: setiap baris tabel *Jenis probe native* ditolak dengan kode dan path nya; manifest NATS baru lolos; probe `native` di `health.probe` diterima dengan aturan yang sama; `validate.rs` dan `plan.rs` tidak menambah baris yang menyebut NATS. Memverifikasi **AC-1**, **AC-2**.
- Plan: golden `run_plan` NATS memuat `ProbeSpec::Native` dengan `serverName` `golden` dan `jetstream` `true` di context golden spec 0009; paket fixture yang memetakan param ke field teks menghasilkan `UnrepresentableValue` dengan nama param untuk nilai kosong atau terlalu panjang. Memverifikasi **AC-3**, **AC-4**.
- State machine: tabel transcript untuk setiap baris *Hasil gagal* dan setiap kasus `protocol_error` di AC-5, sukses kedua mode, PING dari server, INFO di tengah, transcript asli langkah 0, dan pemotongan detail; transcript juga dimainkan dengan potongan byte acak. Memverifikasi **AC-5**, **AC-7**.
- Driver dan loop: `tokio::io::duplex` di bawah waktu dijeda untuk connect gagal, timeout per fase, dan urutan readiness, debounce, `degraded` lalu `unhealthy`; satu smoke test socket asli. Memverifikasi **AC-5**, **AC-6**.
- Happy path native in process: install dari cache, create, start, urutan `statusSeq`, `lsof`, `/varz`, log, stop tanpa sisa, start lagi. Memverifikasi **AC-8**.
- `nats_compatibility`: `SIGSTOP` dan `SIGCONT`, server asing dan instance dengan JetStream mati, adopsi setelah `SIGKILL` daemon, lalu evidence. Memverifikasi **AC-9**, **AC-10**, **AC-11**, **AC-12**.
- Evidence dan catalog: cache kosong `BLOCKED`; run dari `HEAD` bersih menulis `compatibility.json` yang lolos `compat-record`; `catalog check` lolos setelah perpindahan. Memverifikasi **AC-12**.
- Journey CLI dan GUI: test CLI, fixture form spec 0020 dengan assertion tambahan, lalu pemeriksaan GUI manual dengan keyboard dan VoiceOver. Memverifikasi **AC-13**.
- RISK-07: baris yang ditambah per commit dibandingkan dengan allowlist, dan numstat dibandingkan dengan tabel. Memverifikasi **AC-14**.

## Build plan

Urutan mengikuti pendekatan Journey proyek: jalur "instance NATS dari paket sampai ready lalu stop, dengan readiness yang jujur" dibuat lengkap dulu. Milestone 1 membangun semua yang bisa dibuktikan tanpa server NATS asli di daemon, milestone 2 membuktikan jalur itu pada NATS 2.15.1 asli lewat daemon dan membuka entri catalog, dan milestone 3 membuktikan journey yang sama dari CLI dan GUI di atas fitur 20 dan 21.

**Milestone 1: Probe native dan `nats-readiness` di fixture**

0. Spike lokal (tanpa daemon dan tanpa jaringan): ekstrak arsip NATS 2.15.1 dari cache runtime xtask ke folder sementara, jalankan `nats-server` dengan `server_name`, dengan dan tanpa `-js`, dan dengan `--user` serta `--pass`, lalu rekam baris INFO, balasan PONG, balasan `$JS.API.INFO` (kedua mode), `-ERR` untuk CONNECT tanpa credential, dan `/varz` ke `crates/core/tests/fixtures/nats-protocol/`. Catat juga tiga fakta di `verify.md` (dan lewat update spec ini di rationale): apakah INFO pertama setelah start sudah memuat `jetstream: true` sebelum JetStream siap, apakah listener client baru terbuka setelah JetStream aktif, dan apakah JetStream bisa dimatikan NATS saat berjalan. Ketiga fakta itu tidak membuang langkah `$JS.API.INFO` (keputusan lead 2026-10-20). Kalau field yang dipakai spec ini (`server_name`, `version`, `jetstream`, `auth_required`, `tls_required`, `ldm`, `type` dan `error` balasan API, `jetstream.config` dan `version` di `/varz`) berbeda, build berhenti dan kembali ke `/architect`. Memenuhi **AC-5** dan **AC-7** (transcript asli), **AC-8** dan **AC-12** (nama field).
1. Engine: field `name` dan `params` di `RawProbe`, modul `provider::probe::native` (`NativeProbeKind`, descriptor, kind dan nilai param, `NativeProbe`, `build`), cabang `native` di `validate::probe` dengan semua aturan *Jenis probe native* (termasuk posisi interpolasi baru dan aturan `apply: reload`), `ProbeDef::Native`, `ProbeSpec::Native` dan `NativeProbeSpec` di plan beserta `Serialize`, JSON Schema lewat `cargo xtask codegen`, skenario "Himpunan tertutup" spec 0009 yang memakai nama yang tidak ada di registry, dan arm `ProbeSpec::Native` di `supervisor::probe` yang memanggil `native::run` sementara (K3, lihat *Hasil probe dan loop supervisor*). Memenuhi **AC-1**, **AC-2**, **AC-3**.
2. Paket: blok `readiness` dan blok `connection` (spec 0021; hanya langkah ini yang menulis blok NATS, keputusan lead D1) di `providers/nats/provider.yaml` dalam satu commit, `README.md`, golden `run_plan` NATS (`STACKLY_UPDATE_GOLDEN=1`, diff hanya di probe), `cargo xtask provider check`, dan `cargo xtask fuzz seed`. Memenuhi **AC-4**.
3. State machine: `provider::probe::nats::Exchange`, `NativeProbeFailure`, konstanta batas, aturan parse INFO, sanitasi dan pemotongan detail, dan test transcript untuk setiap hasil (termasuk transcript asli langkah 0). Memenuhi **AC-5** (bagian state machine).
4. Fuzz: harness `provider::fuzzing::nats_readiness_protocol` dengan encoding di *Fuzz target*, target `fuzz/fuzz_targets/nats_readiness_protocol.rs`, baris `TARGETS` dan sumber seed di `xtask/src/fuzz.rs`, replay regresi, run 10 menit di Mac ini, lalu run ulang `provider_manifest` dan `provider_interpolation` 10 menit masing masing setelah langkah 1 masuk. Memenuhi **AC-7**.
5. Driver: driver generik di `supervisor::probe::native` (menggantikan `run` sementara langkah 1) di atas `AsyncRead + AsyncWrite` dari fungsi connect, satu deadline, dispatch `match` exhaustive, teks timeout per fase, test `tokio::io::duplex` di bawah waktu dijeda, dan satu smoke test `TcpListener` asli. Memenuhi **AC-5** (bagian IO dan timing).
6. Loop: `ProbeSpec::Native` di readiness, readiness rutin, dan health spec 0010, reason `probe_ok`, dengan test `FakeProcessSupervision` dan driver di atas duplex di bawah waktu dijeda. Memenuhi **AC-6**.

**Milestone 2: NATS 2.15.1 asli lewat daemon, evidence, dan catalog**

7. Harness `apps/daemon/tests/nats.rs`: root data Unicode lewat `RootSet::under_override`, runtime lewat `install_cached_artifact` dengan jalur `BLOCKED`, `pin_package` dan `LogSink` perekam lewat `DaemonConfig.extra_log_sinks` (spec 0010 langkah 13, K5), test create dengan `provider.revision` dari `pin_package` (K1), helper server asing dengan guard yang me reap prosesnya, lalu test `nats_lifecycle_in_process`. Memenuhi **AC-8**.
8. Test `nats_compatibility` lewat `SpawnedDaemon`: check berurutan yang ditambah setelah assertion lulus, readiness jujur dengan guard `SIGCONT`, server asing dengan argv di AC-10, instance dengan JetStream mati, adopsi setelah `SIGKILL` daemon, dan penulisan `compatibility.json`. Memenuhi **AC-9**, **AC-10**, **AC-11**, **AC-12** (bagian test).
9. Evidence dan catalog, dengan urutan di AC-12: commit paket final dan test, run native dari `HEAD` bersih, `cargo xtask runtime compat-record`, lalu satu commit berisi `evidence/compatibility/`, entri catalog, dan ledger dari `cargo xtask catalog check --update`. Memenuhi **AC-12**.

**Milestone 3: Journey yang sama dari CLI dan GUI**

10. Test `nats_journey` di `apps/cli/tests/nats_journey.rs` lewat `SpawnedDaemon` dengan runtime dari cache dan catalog uji dari `CatalogBuilder`: create default dan dengan JetStream mati, start, `inspect <nama> --json` (readiness dari `result.instance`), stop. Memenuhi **AC-13** (bagian CLI).
11. Pastikan fixture `create-form-nats.json` dan test render Swift milik spec 0020 masih lolos setelah perubahan paket fitur ini, dan tambah assertion yang belum ada di test itu (field managed terkunci, toggle, tiga integer advanced dengan batas, dua baris endpoint) tanpa kode form baru. Memenuhi **AC-13** (bagian form).
12. Pemeriksaan RISK-07: commit dasar dan daftar commit build, baris yang ditambah per commit terhadap allowlist, `git diff --numstat` per baris tabel, dan seam baris 4 dicatat di `verify.md`; langkah GUI manual AC-13 disiapkan di `verify.md` untuk `/check verify`. Memenuhi **AC-13** (bagian GUI), **AC-14**.

**Prasyarat**:

- Langkah 0 hanya butuh artefak NATS di cache runtime xtask (sudah ada di Mac ini).
- Langkah 1 dan 2 butuh build fitur 9 dan update spec 0009 (keduanya sudah selesai). Langkah 1 juga butuh langkah 6 spec 0010 (modul folder `supervisor::probe` dan `ProbeOutcome`), karena ia menambah arm `ProbeSpec::Native` di match itu (K3). Langkah 2 menunggu langkah 1.
- Langkah 3 dan 4 tidak butuh update spec 0009 atau spec 0010: keduanya file baru tanpa IO. Bagian run ulang fuzz di langkah 4 menunggu langkah 1.
- Langkah 5 butuh langkah 1 dan langkah 3 (langkah 6 spec 0010, runner probe dan tokio `net` di core, sudah prasyarat langkah 1). Langkah 6 butuh loop readiness dan health dari milestone fitur 10 yang sama.
- Milestone 2 butuh build fitur 10 sampai milestone "Daemon, IPC, and end to end proof" (termasuk langkah 13: create sementara, `pin_package`, dan `DaemonConfig.extra_log_sinks`, K1 dan K5), fitur 11 selesai, milestone 2 spec 0018 (`install_cached_artifact`, langkah 9), dan milestone 1 spec 0017 (`providers/catalog.toml`, ledger, `CatalogBuilder`). Langkah 9 juga butuh langkah 1 spec 0018 (`runtime.versionCheck` NATS sudah di paket sebelum evidence) dan langkah 13 spec 0018 (`compat-record`, milestone 3).
- Langkah 2 butuh langkah 1 spec 0021 (loader blok `connection`), dan langkah 9 butuh blok `connection` sudah di paket, supaya evidence dibuat dari `provider.yaml` final (spec 0021).
- Milestone 3 butuh milestone 2 di sini (NATS sebagai `[[provider]]`), build fitur 20 (form create, `instance.get_create_form`, fixture `create-form-nats.json`, dan `stackly instance create`), dan build fitur 21 (Summary dengan readiness).

**Sesi paralel yang aman**: setelah langkah 0, langkah 3 (state machine) bisa langsung jalan, dan langkah 1 (engine) begitu langkah 6 spec 0010 ada; keduanya bisa dua sesi; keduanya hanya berbagi satu baris deklarasi `mod` di `provider/probe/mod.rs`, yang dibuat sesi yang masuk lebih dulu. Langkah 4 menyusul langkah 3 di sesi nya sendiri, dan langkah 2 menyusul langkah 1. Langkah 5 dan 6 satu sesi setelah prasyaratnya. Langkah 7 sampai 9 satu sesi karena berbagi file test. Di milestone 3, langkah 10 dan 11 bisa dua sesi.

## Consequences

**Positive**:

- Ready untuk NATS berarti server milik instance itu menjawab protokol client dengan mode JetStream yang diminta, bukan port terbuka atau HTTP 200 dari proses lain (PRD 22.2).
- RISK-07 terjawab dengan angka: satu perubahan core khusus NATS dan satu perluasan generik yang sudah direncanakan, tanpa perubahan produk di daemon, CLI, protokol, atau Swift, diukur per baris yang ditambah.
- Jenis probe `native` sekarang ada beserta pola registry, param, state machine tanpa IO, driver generik, dan fuzzing, jadi provider berikutnya yang butuh cek protokol (misalnya ClickHouse atau Valkey) menambah satu varian dan satu state machine, bukan kerangka baru.
- Supervisor mendapat bukti provider nyata kedua dengan runtime Go: startup, readiness jujur, stop, dan adopsi.

**Negative / tradeoffs**:

- Ada kode protokol NATS kecil buatan sendiri di core yang harus dirawat dan di fuzz; perubahan protokol upstream (misalnya field INFO yang berganti nama) bisa membuat readiness gagal sampai state machine diperbarui.
- Setiap instance NATS menerima satu koneksi pendek setiap 10 detik selama hidup, terlihat sebagai client `stackly-readiness` di monitoring.
- Instance NATS fitur ini tanpa auth di loopback sampai fitur 39; README dan `knownLimitations` menyebutnya, dan catalog menampilkan capability `persistence` dan `multiInstance` sebelum fitur 39 membuktikannya ujung ke ujung.
- Cek identitas memakai `server_name`, jadi server NATS asing yang kebetulan bernama sama dengan instance di port yang sama tidak terbedakan oleh probe.
- Mengganti nama instance yang sedang berjalan lalu me restart daemon membuat readiness `identity_mismatch` sampai instance di restart.
- Test `nats_compatibility` berurutan dan butuh sampai sekitar satu menit untuk readiness jujur dengan interval default.
- Milestone 3 menunggu fitur 20 dan 21, jadi fitur ini tidak bisa `done` sebelum keduanya.

**Neutral**:

- Kode reason `connect_failed`, `timeout`, dan `protocol_error` diusulkan sebagai kode generik untuk semua runner probe, dan `probe_ok` untuk sukses.
- Modul `core::provider` mendapat state machine protokol tanpa IO; IO tetap di supervisor.
- Field settings NATS tetap dalam byte; satuan yang lebih ramah (MiB) adalah Follow-up fitur 39 atau 29.
- `providers/nats` menjadi provider nyata pertama yang memakai probe native dan endpoint ber purpose `monitoring`.

## Follow-up

- [x] `architect (update)` spec 0009 (setelah fitur 9 ditutup, digabung lead dengan usulan spec 0017, 0018, dan 0019): terapkan *Usulan update spec 0009*, lalu centang baris Follow-up fitur 23 di spec itu. Diterapkan di spec 0009 (2026-10-20), termasuk Follow-up fitur 23 yang dicentang.
- [x] `architect (update)` spec 0010: terapkan *Usulan update spec 0010* (`ProbeOutcome`, `probe_ok`, kode generik, dispatch native, bukti provider nyata). Diterapkan di spec 0010 (2026-10-20, K3 dan K5).
- [ ] Fitur 39: auth client NATS (credential lewat secret dan template `sensitive`), perluasan `nats-readiness` untuk auth tanpa membaca Keychain setiap 10 detik (misalnya user readiness berhak minimum), dua instance terisolasi, persistence stream setelah restart, dan bentrok port monitoring (AC-036); hapus `knownLimitations` yang sudah terbukti.
- [ ] Fitur 21: connection string NATS (`nats://<host>:<port>` dari endpoint `client`, tanpa credential sampai fitur 39) dan URL monitoring, menjawab juga Follow-up spec 0009 tentang `purpose` endpoint.
- [ ] Fitur 29 dan 30: rename instance saat berjalan mengubah nilai `${instance.name}` yang dipakai field managed dan probe; tolak rename saat berjalan untuk paket yang field managed nya memakai nama, atau bekukan nilai managed saat start untuk dipakai adopsi.
- [ ] Fitur 39 atau 29: satuan MiB untuk batas JetStream dan `max_payload` di form, setelah suffix ukuran `nats.conf` dibuktikan pada binary asli.
- [ ] Fitur 36: kebijakan kompatibilitas v1alpha1 mencatat jenis `native`, kind param, dan cara menambah probe native lewat review.
- [ ] `/architect` update spec ini setelah langkah 0: catat tiga fakta startup dan JetStream NATS 2.15.1 di rationale.
- [ ] `/sync` setelah build: catat jenis probe native, `cargo test -p stacklyd --test nats -- --ignored`, dan fuzz target baru di root `AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, keputusan kecil, dan hasil cross check ada di [rationale.md](rationale.md).
