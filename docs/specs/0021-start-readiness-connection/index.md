# 0021. Start, readiness, dan connection

**Date**: 2026-10-20
**Status**: Proposed

## Summary

Spec ini menutup JOURNEY-01: setelah instance dibuat, pengguna menjalankannya, melihat proses dan readiness sebagai dua status terpisah, lalu menyalin connection string. Connection string dideklarasikan provider di manifest (blok `connection` baru), dirender daemon dari paket yang di pin, config aktif, dan port efektif, lalu dikirim ke GUI dan CLI dalam bentuk tersamar; password asli hanya diambil lewat `secret.reveal` saat pengguna menekan Show Password atau Copy, atau memberi `--show-password`. Halaman detail instance dengan tab Summary (endpoint, readiness, connection, secret reference, dependents, dan detail) dan `stackly instance connection` membaca data yang sama dari method baru `instance.get_detail`, ditambah `instance.get_data_usage` untuk ukuran folder data. Start, status, dan connection hanya memakai registry, paket pinned, dan runtime terpasang, jadi semuanya tetap bekerja tanpa jaringan dan tanpa catalog.

## Requirements

**User stories**:

- Sebagai pengguna yang baru membuat PostgreSQL, saya ingin menekan Start, melihat instance berjalan lalu siap, dan menyalin connection string ke aplikasi saya, supaya JOURNEY-01 selesai tanpa membuka terminal.
- Sebagai pengguna, saya ingin proses yang hidup tetapi belum siap tidak pernah tampil sebagai siap, supaya saya tidak menghubungkan aplikasi ke database yang belum menerima koneksi.
- Sebagai pengguna, saya ingin password tidak pernah tampil atau tersalin tanpa saya minta, supaya layar dan clipboard saya tidak membocorkannya.
- Sebagai pengguna tanpa internet, saya ingin instance yang runtime nya sudah terpasang tetap bisa start, menampilkan status, dan memberi connection, supaya kerja lokal saya tidak bergantung pada jaringan.
- Sebagai penulis script, saya ingin `stackly instance connection <instance> --format uri` yang tersamar secara default dan `stackly instance inspect` yang berisi data yang sama dengan tab Summary.
- Sebagai penulis provider, saya ingin connection string dideklarasikan di manifest, supaya provider baru tidak butuh kode Rust atau Swift untuk tombol Copy.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-012):

- **AC-1** (blok `connection` di manifest): manifest `stackly/v1alpha1` menerima blok opsional `connection`, daftar 1 sampai 8 entri `{ name, title, encoding, template }` seperti *Blok connection di manifest*. Loader menolak dengan path field: `name` di luar pola `^[a-z][a-z0-9_]{0,31}$` atau ganda (`connection_name_duplicate`), `title` kosong, lebih dari 64 karakter, atau berkarakter kontrol, `encoding` selain `uri` atau `none`, `template` kosong atau di atas 512 byte, token di luar baris matriks posisi `connection.template` (`token_not_allowed_here`), endpoint atau secret yang tidak ada (`unknown_endpoint`, `unknown_secret`), `settings.<field>` untuk field secret, array, atau managed, dan `endpoint.<n>.host` di entri `encoding: uri` (`token_not_allowed_here`, karena alamat IPv6 butuh kurung siku). Blok ini ikut `manifest_sha256`. Paket `providers/postgresql` dan `providers/nats` membawa blok di *Connection PostgreSQL dan NATS*, dan `cargo xtask provider check` serta golden render lolos.
- **AC-2** (render connection): `instance::connection::render(package, ctx)` mengembalikan satu `RenderedConnection` per entri manifest, dalam urutan manifest. Entri yang semua tokennya bisa diselesaikan berstatus tersedia dengan `parts` (teks yang sudah di encode dan bagian secret yang hanya membawa field, reference, dan encoding) dan `masked` (gabungan teks dengan setiap bagian secret diganti `****`, selalu empat bintang, tanpa melihat panjang nilai). Entri yang merujuk port endpoint `Auto` tanpa port efektif berstatus `port_not_assigned`, yang merujuk socket yang `socketPath` nya null berstatus `socket_not_created`, dan instance retained berstatus `retained`. Dengan `encoding: uri`, nilai token `secret.*`, `settings.*`, `instance.*`, `provider.*`, `runtime.*`, `endpoint.<n>.socket`, dan `endpoint.<n>.socketDir` di percent encode kecuali karakter unreserved RFC 3986 (`A-Z a-z 0-9 - . _ ~`), sedangkan `endpoint.<n>.port` dan `endpoint.<n>.address` (IPv6 dalam kurung siku) disisipkan apa adanya; dengan `encoding: none` tidak ada encode. `port_may_change` bernilai true jika dan hanya jika entri merujuk endpoint `Auto` dan instance tidak sedang `running` atau `recovering`. Render tidak pernah membaca Keychain. Test tabel mencakup IPv4, IPv6, socket di path berspasi dan Unicode, settings teks berkarakter khusus, dan setiap status tidak tersedia.
- **AC-3** (komposisi): fungsi murni `compose(parts, values)` ada dua kali dengan perilaku sama, `stackly_protocol::instance::connection::compose` (Rust, dipakai CLI) dan `ConnectionComposer.compose` (Swift, `StacklyUI`): teks apa adanya, nilai secret di percent encode dengan himpunan unreserved yang sama untuk `encoding: uri` dan apa adanya untuk `none`. Keduanya menghasilkan string yang sama persis untuk setiap kasus fixture `protocols/fixtures/instance/connection-compose.json` (password dengan spasi, `@`, `:`, `/`, `%`, `?`, `#`, `'`, `"`, `\`, emoji, dan huruf beraksen, serta entri dengan dua secret). Nilai secret tidak pernah ada di DTO `InstanceDetail`.
- **AC-4** (`instance.get_detail`): `instance.get_detail { instanceId }` (`recovery refused`, hanya membaca) membalas `InstanceDetail` seperti *Method IPC* dengan setiap field bersumber dari tabel *Value sourcing*: provider dan runtime dari registry dan paket pinned (nama tampil dari `metadata.displayName` paket pinned, tidak pernah dari catalog), path data absolut, `startup`, satu baris secret per pemakaian secret instance (field, judul dari schema paket pinned, reference, status cache spec 0011), dependents dari environment dan lease registry, dan connection dari AC-2. Paket pinned yang tidak terbaca atau digest nya beda tidak menggagalkan method: setiap connection berstatus `package_unavailable` dan judul secret memakai nama field. ID yang tidak ada menghasilkan `resource.not_found`. Method ini tidak menulis apa pun, tidak spawn, dan tidak pernah membaca nilai secret.
- **AC-5** (`instance.get_data_usage`): `instance.get_data_usage { instanceId }` (`recovery refused`) membalas `DataUsage { bytes, files, complete, scannedAt }` untuk folder `data` instance itu dari `Filesystem::tree_usage` (usulan spec 0004): jumlah blok teralokasi (`st_blocks` kali 512) setiap entri, tanpa mengikuti symlink, tanpa menyeberang device, berhenti di `DATA_USAGE_MAX_ENTRIES` entri atau `DATA_USAGE_DEADLINE` dengan `complete: false`. Hasil di cache per instance selama `DATA_USAGE_CACHE_TTL`, dan permintaan bersamaan untuk instance yang sama memakai satu scan. Folder yang hilang, bukan folder privat milik user, atau tidak terbaca menghasilkan `instance.data_usage_unavailable` (detail `reason` `missing`, `not_private`, atau `io`). Test memakai pohon dengan symlink keluar root, hard link, file sparse, folder di volume lain (image disk sementara di macOS), dan 250 000 entri.
- **AC-6** (`stackly instance connection`): `stackly instance connection <instance> [--format <name>] [--all] [--show-password]` menerima nama atau UUID lewat resolver spec 0022, memanggil `instance.get_detail`, dan tanpa `--format` memilih entri pertama. Stdout default berisi tepat string `masked` diikuti newline, tanpa password. `--format` yang tidak ada gagal `cli.connection_not_found` (exit 10, detail `format`, `available`); entri yang tidak tersedia gagal `cli.connection_unavailable` (exit 6, detail `format`, `reason`); provider tanpa blok `connection` gagal `cli.connection_not_found` dengan `available` kosong. `--all` menulis satu baris per entri. `--show-password` memanggil `secret.reveal { reference, purpose: show }` sekali per reference berbeda dengan batas waktu `Interactive` spec 0008, menyusun string lengkap lewat `compose`, dan hanya itu yang mencetak password; `--all` bersama `--show-password` gagal `cli.usage` (exit 2). `--json` memberi `result` `{ "instance": { "id", "name" }, "connection": ConnectionString, "value": string | null }`, atau `{ "instance", "connections": [ConnectionString] }` untuk `--all`, dengan `value` terisi hanya dengan `--show-password`. Setiap reveal tercatat audit `secret.reveal` spec 0011.
- **AC-7** (paritas inspect dan Summary): `stackly instance inspect` (spec 0022) juga memanggil `instance.get_detail` dan `instance.get_data_usage`, menambah baris kv dan tabel di *CLI*, dan dengan `--json` memberi `result` `{ "instance": InstanceSummary, "detail": InstanceDetail, "dataUsage": DataUsage | null }` (usulan spec 0022). Golden CLI dan test view model Swift membaca fixture `protocols/fixtures/instance/detail-postgresql.json`, `detail-nats.json`, dan `detail-unavailable.json` yang sama dan menghasilkan teks yang sama untuk setiap baris di tabel *Format bersama* (versi, digest, provider, ukuran data, uptime, restarts, autostart, endpoint, connection tersamar, dan alasan tidak tersedia).
- **AC-8** (AC-012, REQ-012, BR-010): instance fake yang prosesnya `running` tetapi probe readiness nya gagal (setting `readyDelayMs` di atas durasi test) tampil di header halaman detail sebagai "Running" dan di baris Readiness Summary sebagai "Waiting for probe" atau "Not ready" dengan detail probe, bukan "Ready", dan `stackly instance inspect` menulis `Lifecycle running` dan `Readiness not_ready (<reason>)`; setelah probe berhasil keduanya berubah ke "Ready" tanpa memuat ulang halaman. Test GUI (view model dengan fixture urutan event) dan test CLI terhadap daemon nyata memasang guard evidence `covers = ["AC-012", "REQ-012", "BR-010"]`.
- **AC-9** (start dan stop dari halaman detail): toolbar halaman detail memakai aksi utama `InstanceActionModel` spec 0022 (Start, Stop, Retry, tombol sibuk, atau nonaktif dengan alasan) dengan simbol dan label; Start mengirim `instance.start`, status menjadi "Starting…" lalu `running` dengan readiness "Waiting for probe" lalu "Ready" lewat event spec 0010, dan connection yang tadinya `port_not_assigned` menjadi tersedia setelah detail dimuat ulang. Perintah setara CLI tetap `stackly instance start <instance> --wait` dan `stackly instance stop <instance> --wait` (fitur 14).
- **AC-10** (offline, REQ-046, AC-046): dengan daemon yang catalog nya `NotConfigured`, `Fetcher` tiruan yang menggagalkan test pada panggilan apa pun, dan penghitung akses catalog di `stacklyd`, urutan `instance.start` sampai `ready`, `instance.get`, `instance.get_detail`, `instance.get_data_usage`, `stackly instance connection`, dan `stackly instance inspect` untuk instance fake dengan runtime dan paket pinned yang sudah ada berhasil, dengan nol akses catalog, nol panggilan `Fetcher`, dan nol panggilan `CodeSigning::assess`. Bukti native dengan jaringan Mac ini dimatikan (Wi-Fi dan Ethernet) untuk PostgreSQL lewat GUI dan CLI dicatat sebagai evidence `native` setelah persetujuan user, dan `BLOCKED` sampai disetujui.
- **AC-11** (navigasi halaman detail): `ShellDestination.instance(UUID)` (usulan spec 0022) membuka halaman detail di bagian Instances dengan `NavigationStack`, dan `ShellNavigator.openInstance(_:)` memakainya, menggantikan perilaku `// provisional: F21` spec 0022. Klik ganda atau Return pada baris Instances, tombol Open di inspector Instances, baris instance di Overview, dan setelah create (spec 0020) membuka halaman ini. Tombol kembali sistem kembali ke Instances dengan pilihan baris tetap. Instance yang tidak ada di `InstanceStore` dan tidak ditemukan `instance.get` menampilkan "Instance Not Found" dengan Back to Instances.
- **AC-12** (header dan toolbar): halaman memakai `.pageChrome(title: name, subtitle: "{nama tampil provider} {versi} · {label status}")`; toolbar berisi tombol aksi utama dan menu More dengan "Copy Connection String" (entri pertama; nonaktif dengan alasan kalau tidak ada entri atau entri itu tidak tersedia). Restart, Reload, Duplicate, dan Remove tidak tampil sampai fitur 26, 24, dan 54 membangunnya. Picker tab hanya menampilkan tab yang sudah dibangun dan tersembunyi selama Summary satu satunya tab.
- **AC-13** (tab Summary, PAGE-05): tab Summary mengikuti mockup dan *Tab Summary*: kartu Endpoints (satu baris per endpoint dengan alamat atau keterangan, tombol Copy hanya untuk alamat yang diketahui, lalu baris Readiness terpisah), kartu Connection (setiap entri dengan string tersamar, Show Password dan Copy untuk entri ber secret lewat `RevealableSecretText` spec 0011, Copy saja untuk entri tanpa secret, keterangan alasan untuk entri tidak tersedia, catatan `port_may_change`, lalu satu baris Secret per reference dengan statusnya), kartu Dependents, dan kartu Details (Instance ID, Runtime, Version, Digest, Provider, Data, Data size, Uptime, Restarts, Autostart). Copy endpoint menulis teks alamat ke pasteboard biasa hanya saat tombolnya ditekan; Copy entri ber secret memanggil `revealSecret(purpose: .copy)`, menyusun lewat `ConnectionComposer`, dan menulis lewat `ConcealedPasteboard`; Show Password memanggil `revealSecret(purpose: .show)` dan menyembunyikan lagi menurut aturan spec 0011. Tanpa aksi pengguna, tidak ada reveal dan tidak ada tulisan pasteboard.
- **AC-14** (banner dan state): Summary menampilkan banner masalah dari aturan I1 sampai I4 spec 0022 (failed dengan cuplikan `lastFailure.excerpt` dan Retry, `unknown`, blocker), dan blocker `needsSecret` menawarkan "Enter Secret…" yang membuka `SecretEntrySheet` spec 0011 dengan item blocker itu. Detail yang sedang dimuat menampilkan `DelayedProgress` di baris yang bergantung pada detail sementara baris dari `InstanceSummary` sudah tampil; detail yang gagal dimuat menampilkan `StateBanner` caution "Couldn’t load instance details." dengan Retry tanpa mengosongkan halaman; daemon yang tidak terjangkau mempertahankan data terakhir dengan banner stale shell dan aksi tergate. Detail dimuat ulang saat `revision` summary instance itu berubah (jeda 250 ms) dan saat `secret.changed` menyentuh salah satu reference nya.
- **AC-15** (inspector Instances): inspector spec 0022 mendapat tombol Open di samping aksi utama, baris Data (ukuran dari `instance.get_data_usage` dengan path di baris kedua), dan baris Connection (entri pertama tersamar dengan Show Password dan Copy lewat komponen yang sama), dan placeholder "No Selection" berpesan "Select an instance to see its details. Double-click a row to open it.".
- **AC-16** (AC-027, NFR-012): alur Start sampai Copy connection bisa diselesaikan dengan keyboard (urutan fokus di *Aksesibilitas*) dan VoiceOver (status proses dan readiness dibacakan terpisah, nilai tersamar dibacakan "Password, hidden" beserta reference, setiap tombol Copy berlabel apa yang disalin, dan hasil copy diumumkan); tidak ada informasi yang hanya ada lewat warna atau hover; teks membungkus tanpa terpotong di lebar konten 640 pt dengan Dynamic Type; kontras WCAG AA di light, dark, dan Increase Contrast; dibuktikan evidence manual keyboard dan VoiceOver di Mac ini.
- **AC-17** (kontrak): method `instance.get_detail` dan `instance.get_data_usage` (keduanya `recovery refused`), DTO di *Method IPC*, enum terbuka baru (diakhiri `Unrecognized`), kode `instance.data_usage_unavailable` (hint statis lewat `codes::instance::hint`), dan kode `cli.connection_not_found` serta `cli.connection_unavailable` ada di `stackly-protocol` dengan fixture `protocols/fixtures/` untuk setiap varian `Call` dan `Reply`, setiap kode, dan nilai asing setiap enum terbuka, terbaca di Rust dan Swift, dengan kenaikan minor `ProtocolVersion::CURRENT` di commit yang sama dan `cargo xtask codegen --check` bersih.
- **AC-18** (keamanan, AC-016): suite sentinel memakai fixture credential sebagai password instance fake dan memastikan prefix nya tidak ada di balasan `instance.get_detail`, event, log daemon, output default dan `--json` `stackly instance connection` dan `inspect` tanpa `--show-password`, dan state model `@Observable` yang hidup lama di Swift; nilai hanya muncul di respons `secret.reveal`, di stdout `--show-password`, dan di `ConcealedPasteboard` setelah Copy.
- **AC-19** (kinerja): dengan 10 instance terdaftar di Mac ini, `instance.get_detail` dijawab dalam 100 ms pada persentil 95 saat cache paket sudah hangat dan dalam 300 ms saat dingin, dan `instance.get_data_usage` selesai dalam `DATA_USAGE_DEADLINE` ditambah 100 ms.
- **AC-20** (PostgreSQL dan NATS nyata): di Mac ini, instance PostgreSQL 18 dan NATS yang dibuat lewat fitur 20 di start dari GUI dan dari CLI sampai `ready`; string `uri` PostgreSQL yang disusun dengan password hasil `secret.reveal` berhasil menjalankan `SELECT 1` lewat `psql` milik runtime (TCP), string `socket` juga berhasil, dan string `uri` NATS menerima baris `INFO` server; tab Summary dan `stackly instance connection` menunjukkan string tersamar yang sama. Tanpa artefak di cache runtime xtask hasilnya `BLOCKED` dengan alasan dan owner.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md): connection string dideklarasikan di blok `connection` manifest dan dirender daemon dengan aturan encode per entri; string lengkap disusun client dari bagian tersamar dan nilai `secret.reveal`, dengan satu fixture komposisi untuk Rust dan Swift; data halaman detail dari method baru `instance.get_detail`, dan ukuran folder data dari `instance.get_data_usage` dengan scan terbatas dan cache; baris Restarts memakai jendela restart policy; semua entri connection ditampilkan; `stackly instance connection` memilih entri di client dari detail; `stackly instance inspect --json` dibungkus `{ instance, detail, dataUsage }`; tab yang belum dibangun tidak ditampilkan; dan bukti offline lewat instrumentasi otomatis ditambah run native dengan jaringan mati setelah persetujuan user.

Daemon menghitung semua nilai yang tampil; GUI dan CLI hanya memformat dengan aturan bersama dan meminta nilai secret lewat `secret.reveal` saat pengguna memintanya.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya: tidak ada `unwrap()` di luar test, kontrol native SwiftUI menggantikan tiruan HTML di mockup, dan nilai secret tidak pernah disimpan di model `@Observable` yang hidup lama.

## Feature design

### Trace PRD

| ID PRD | Dipenuhi oleh |
| --- | --- |
| REQ-012 (readiness berbeda dari process state) | AC-8, AC-9, AC-13 |
| REQ-046 (offline) | AC-10 |
| PAGE-05 (Instance Summary) | AC-11 sampai AC-15; restart dan dependency impact penuh di fitur 26 dan 44 sampai 47 |
| BR-010 (ready butuh probe yang relevan dan segar) | AC-8 |
| AC-012 | AC-8 |
| AC-027 | AC-16, AC-20 |
| AC-046 | AC-10 |
| NFR-012 | AC-16 |
| JOURNEY-01 (copy connection) | AC-9, AC-13, AC-20 |
| REQ-016 dan AC-016 (masking, reveal eksplisit) | AC-2, AC-6, AC-13, AC-18 |
| REQ-028 (CLI setara GUI) | AC-6, AC-7 |
| REQ-029 (provider tanpa patch core dan GUI) | AC-1 |

### Batas dengan spec lain

| Bagian | Pemilik |
| --- | --- |
| `instance.start`, `instance.stop`, `instance.get`, `InstanceSummary`, readiness, event instance, `socketPath` dari template paket pinned | spec 0010 (dengan usulan `socketPath` dari spec 0020) |
| Manifest, interpolasi, matriks posisi, `provider check` | spec 0009 (blok `connection` lewat usulan) |
| `Filesystem` | spec 0004 (`tree_usage` lewat usulan) |
| `secret.reveal`, `RevealableSecretText`, `ConcealedPasteboard`, `SecretEntrySheet`, status reference, audit | spec 0011 (dipakai apa adanya) |
| Halaman Instances, inspector, `InstanceActionModel`, `ShellNavigator`, aturan I1 sampai I4, `UptimeText`, `RelativeAge`, resolver CLI, `stackly instance list` dan `inspect` | spec 0022 (destinasi `.instance`, inspector, dan bentuk JSON inspect lewat usulan) |
| Paket PostgreSQL dan NATS | spec 0019 dan 0023 (blok `connection` lewat usulan) |
| `stackly instance start` dan `stop` dengan `--wait` | fitur 14 (spec 0008 dan 0010) |
| Create, `instance.removed` | spec 0020 |
| Tab Configuration, Logs, Metrics, Backups, Activity; Restart dan Reload; Duplicate; Remove | fitur 29, 27, 28, 49, 35; 26; 24; 54 |
| Banner crash loop yang lebih kaya | fitur 33 (spec ini memakai aturan I1 spec 0022 dengan Retry) |
| Jumlah restart sejak dibuat dan latensi probe | fitur 28 |
| Dependents dari environment | fitur 44 sampai 47 (spec ini sudah membaca registry, yang kosong di fase 02) |
| Komponen dasar dan peta perintah | fitur 67 dan 69 |

### Alur

1. Pengguna membuka halaman detail (klik ganda baris, Open di inspector, Overview, atau setelah create).
2. `InstanceDetailModel` memakai `InstanceSummary` dari `InstanceStore` untuk header, status, endpoint, dan readiness, lalu memanggil `instance.get_detail` dan `instance.get_data_usage`.
3. Start lewat `InstanceActionModel` mengirim `instance.start`; event `instance.state_changed` dan `instance.health_changed` spec 0010 memperbarui status dan readiness, dan perubahan `revision` memicu muat ulang detail sehingga connection yang menunggu port menjadi tersedia.
4. Copy pada connection ber secret memanggil `secret.reveal { purpose: copy }`, menyusun string lewat `ConnectionComposer`, dan menulis `ConcealedPasteboard`.
5. CLI: `stackly instance start <instance> --wait`, lalu `stackly instance connection <instance>` (tersamar) atau dengan `--show-password`.

### Blok connection di manifest

Field baru di manifest `stackly/v1alpha1` (usulan spec 0009), opsional:

```yaml
connection:
  - name: uri                      # ^[a-z][a-z0-9_]{0,31}$, unik; entri pertama adalah entri utama
    title: Connection string       # 1 sampai 64 karakter, tanpa karakter kontrol; label di GUI
    encoding: uri                  # uri atau none
    template: "postgresql://postgres:${secret.superuserPassword}@${endpoint.client.address}/postgres"
```

- 1 sampai 8 entri. Paket tanpa blok ini valid; GUI menulis "This provider doesn’t describe a connection string." dan CLI gagal `cli.connection_not_found`.
- `template` 1 sampai 512 byte, memakai grammar interpolasi spec 0009. Token yang diterima di posisi baru `connection.template`: `endpoint.<n>.address`, `endpoint.<n>.port`, `endpoint.<n>.host` (hanya `encoding: none`), `endpoint.<n>.socket`, `endpoint.<n>.socketDir`, `instance.name`, `instance.id`, `settings.<field>` (field skalar bukan secret dan bukan managed), `secret.<field>` (field secret), `provider.id`, `provider.revision`, dan `runtime.version`. Token folder lain dan `runtime.executable` ditolak.
- Aturan encode di AC-2. Teks literal template tidak pernah di encode; penulis paket menulis teks yang sudah sah untuk formatnya.

### Connection PostgreSQL dan NATS

PostgreSQL (`providers/postgresql/provider.yaml`, usulan spec 0019):

```yaml
connection:
  - name: uri
    title: Connection string
    encoding: uri
    template: "postgresql://postgres:${secret.superuserPassword}@${endpoint.client.address}/postgres"
  - name: socket
    title: Unix socket
    encoding: uri
    template: "postgresql://postgres:${secret.superuserPassword}@/postgres?host=${endpoint.socket.socketDir}&port=${endpoint.client.port}"
```

NATS (`providers/nats/provider.yaml`, usulan spec 0023), tanpa credential sampai fitur 39:

```yaml
connection:
  - name: uri
    title: Client URL
    encoding: uri
    template: "nats://${endpoint.client.address}"
  - name: monitoring
    title: Monitoring URL
    encoding: uri
    template: "http://${endpoint.monitoring.address}"
```

- Fake provider (`testing/fake`) mendapat `uri` `http://stackly:${secret.authToken}@${endpoint.http.address}/` untuk test komposisi dan sentinel.
- Menambah blok ini mengubah `manifest_sha256`, jadi paket harus menulisnya sebelum evidence BR-025 nya dibuat (aturan spec 0019 dan 0023) dan catalog menaikkan revision di commit yang sama (aturan spec 0017).

### Render di daemon

Modul baru `stackly-core::instance::connection`:

```rust
pub struct ConnectionContext<'a> { pub instance: &'a Instance, pub paths: &'a InstancePaths, pub run_dir: &'a Path,
                                   pub settings: &'a SettingsValues, pub secret_refs: &'a [(SecretFieldName, SecretRef)],
                                   pub running: bool }
pub fn render(package: &ValidatedPackage, ctx: &ConnectionContext<'_>) -> Vec<RenderedConnection>;
pub struct RenderedConnection { pub name: String, pub title: String, pub state: ConnectionState, pub port_may_change: bool }
pub enum ConnectionState { Available { masked: String, parts: Vec<ConnectionPart> }, Unavailable(ConnectionUnavailable) }
pub enum ConnectionPart { Text(String), Secret { field: SecretFieldName, reference: SecretRef, encoding: ConnectionEncoding } }
pub enum ConnectionUnavailable { PortNotAssigned, SocketNotCreated, PackageUnavailable, Retained }
```

- Port efektif mengikuti spec 0004 (`bound` kalau ada, selain itu port `Fixed`); host dari endpoint aggregate; socket dan folder socket dari folder run instance ditambah `socketName` paket pinned yang dirender dengan port efektif (aturan *Socket Unix dengan token port* spec 0020).
- `settings` dari `Registry::active_config_revision` yang divalidasi ulang dengan `SettingsSchema::validate` paket pinned; kalau gagal, semua entri yang merujuk `settings.*` menjadi `PackageUnavailable`.
- `secret_refs` dari `secret_uses_by(Instance(id))` (spec 0005). Render tidak pernah memanggil `SecretStore`.
- `ConnectionPart::Text` berisi teks yang sudah di encode; dua teks berurutan digabung.

### Method IPC

Semua method lewat handshake dan peer user yang sama (spec 0006).

| Method | Recovery | Input | Output | Error utama |
| --- | --- | --- | --- | --- |
| `instance.get_detail` | refused | `GetInstanceDetailParams { instanceId: Uuid }` | `InstanceDetail` | `resource.not_found` |
| `instance.get_data_usage` | refused | `GetDataUsageParams { instanceId: Uuid }` | `DataUsage` | `resource.not_found`, `instance.data_usage_unavailable` |

DTO baru di modul `instance` `stackly-protocol`:

```rust
pub struct GetInstanceDetailParams { pub instance_id: Uuid }
pub struct InstanceDetail {
    pub instance_id: Uuid,
    pub revision: Revision,                         // revision instance yang dipakai membangun detail ini
    pub provider: DetailProvider,
    pub runtime: DetailRuntime,
    pub data_path: String,                          // path absolut folder data
    pub startup: StartupPolicyKind,
    pub secrets: Vec<DetailSecret>,
    pub dependents: Vec<DetailDependent>,
    pub connections: Vec<ConnectionString>,
}
pub struct DetailProvider { pub name: String, pub revision: String, pub manifest_sha256: String, pub display_name: String }
pub struct DetailRuntime { pub id: Uuid, pub version: String, pub line: String, pub target: String,
                           pub install_mode: InstallModeKind, #[serde(default)] pub artifact_sha256: Option<String> }
pub enum StartupPolicyKind { Manual, AtLogin, Unrecognized }
pub struct DetailSecret { pub field: String, pub title: String, pub reference: String, pub status: SecretRefStatus }  // status: enum spec 0011
pub struct DetailDependent { pub environment_id: Uuid, pub name: String, pub ownership: DependentOwnership, pub lease_active: bool }
pub enum DependentOwnership { Dedicated, Shared, Unrecognized }
pub struct ConnectionString {
    pub name: String, pub title: String, pub available: bool,
    #[serde(default)] pub reason: Option<ConnectionUnavailableReason>,
    #[serde(default)] pub masked: Option<String>,    // terisi jika dan hanya jika available
    pub parts: Vec<ConnectionPart>,                  // kosong kalau tidak available
    pub port_may_change: bool,
}
pub enum ConnectionPart { Text(String), Secret(ConnectionSecretPart) }     // tag type, content
pub struct ConnectionSecretPart { pub field: String, pub reference: String, pub encoding: ConnectionEncoding }
pub enum ConnectionEncoding { Uri, None, Unrecognized }
pub enum ConnectionUnavailableReason { PortNotAssigned, SocketNotCreated, PackageUnavailable, Retained, Unrecognized }
pub struct GetDataUsageParams { pub instance_id: Uuid }
pub struct DataUsage { pub bytes: Count, pub files: Count, pub complete: bool, pub scanned_at: Timestamp }
```

- `SecretRefStatus` adalah enum status reference spec 0011 (`present`, `missing`, `unknown`); `InstallModeKind` dari spec 0018.
- Client yang menerima `ConnectionEncoding::Unrecognized` di bagian secret tidak menawarkan Show Password dan Copy untuk entri itu ("Update Stackly to copy this connection string."), karena tidak tahu cara encode nya.
- `stacklyd` menyimpan cache paket pinned per `ProviderRevisionKey` (LRU 32 entri, isi immutable), dipakai bersama `socketPath` spec 0020.

### Ukuran data

- `Filesystem::tree_usage(root, UsageLimits { max_entries, deadline }) -> Result<TreeUsage { bytes, files, complete }, PlatformError>` (usulan spec 0004): penelusuran iteratif berbasis file descriptor di adapter macOS (`openat` dengan `O_NOFOLLOW`, `fstatat` dengan `AT_SYMLINK_NOFOLLOW`), `bytes` jumlah `st_blocks * 512` setiap entri termasuk folder, hard link dihitung sekali per (device, inode), entri di device lain dilewati, symlink dihitung sebagai dirinya sendiri, dan berhenti dengan `complete: false` di batas entri atau tenggat. `FakeFilesystem` memakai penelusuran portabel yang sama semantiknya.
- `stacklyd` memeriksa folder data lebih dulu (folder, bukan symlink, milik user, privat), menjalankan scan di `spawn_blocking`, menyimpan hasil per instance selama `DATA_USAGE_CACHE_TTL`, dan menggabungkan permintaan bersamaan ke satu scan.
- Ukuran ditampilkan desimal seperti spec 0017 ("1.2 GB"); `complete: false` ditulis "at least 1.2 GB".

### Tab Summary

Folder baru `StacklyUI/InstanceDetail/`: `InstanceDetailPage`, `InstanceDetailModel`, `SummaryTab`, `EndpointsCard`, `ConnectionCard`, `DependentsCard`, `DetailsCard`, `InstanceDetailBanner`, `ConnectionComposer`, `DetailFormat`, dan `InstanceTab`. `StacklyClient` mendapat `getInstanceDetail` dan `getDataUsage` dari tipe hasil generate.

- **Latar dan tata letak**: `surfaceWindow`, `ScrollView`, kartu `.cardSurface()`. Saat lebar konten minimal 760 pt, dua kolom (kiri Endpoints, Connection, Dependents; kanan Details); di bawahnya satu kolom dengan urutan yang sama.
- **Banner**: di atas kartu, aturan I1 sampai I4 spec 0022 dengan copy `InstanceFailureCopy` spec 0022; I1 membawa blok cuplikan `lastFailure.excerpt` dan tombol Retry (aksi utama); blocker `needsSecret` menampilkan "Needs a secret to start" dengan pesan reference yang bermasalah dan tombol "Enter Secret…" yang membuka `SecretEntrySheet`.
- **Endpoints**: judul "Endpoints"; per endpoint DTO, nama di kiri dan nilai monospace di kanan menurut *Format bersama*; tombol Copy (simbol `doc.on.doc`) hanya saat nilai adalah alamat atau path yang diketahui. Baris terakhir "Readiness" dengan teks readiness spec 0022 ("Ready", "Waiting for probe", "Not ready", atau "Unknown", ditambah `detail` dan "checked {RelativeAge}").
- **Connection**: judul "Connection"; per `ConnectionString`: `title` sebagai label kecil, lalu
  - tersedia dengan bagian secret: `RevealableSecretText` (nilai tersamar, Show Password atau Hide Password, Copy) dengan closure komposisi `ConnectionComposer.compose(parts:values:)`;
  - tersedia tanpa bagian secret: teks monospace bisa dipilih dan tombol Copy (pasteboard biasa);
  - tidak tersedia: teks alasan dari *Copy*;
  - `portMayChange`: catatan "The port can change the next time the instance starts.".
  Lalu satu baris "Secret" per `DetailSecret`: reference monospace, dan untuk status `missing` atau `unknown` keterangan "Missing; starting asks for it." atau "Status unknown".
- **Dependents**: judul "Dependents"; "Nothing depends on this instance." kalau kosong, selain itu satu baris per environment "{name} · {Dedicated | Shared}" ditambah " · lease active".
- **Details**: judul "Details"; `LabeledContent` dengan nilai `textSecondary` bisa dipilih, baris dan format di *Format bersama*: Instance ID (dengan Copy), Runtime, Version, Digest, Provider, Data, Data size (dengan tombol refresh `arrow.clockwise` berlabel "Recalculate data size" dan keterangan "scanned {RelativeAge}"), Uptime, Restarts, Autostart.
- `InstanceDetailModel` (`@Observable`, main actor) memegang detail, data usage, state muat, dan error; memuat saat halaman tampil, saat `revision` summary berubah (jeda 250 ms), dan saat `secret.changed` menyentuh reference nya; data usage dimuat saat halaman tampil dan saat tombol refresh ditekan. Uptime diperbarui lewat `TimelineView(.periodic(from:by: 1))` hanya di baris itu.

### Format bersama

Dipakai GUI (`DetailFormat`) dan CLI (`inspect`, `connection`); test paritas membaca fixture yang sama (AC-7).

| Baris | Sumber | Teks |
| --- | --- | --- |
| Endpoint IP | `InstanceSummary.endpoints[]` | "{host}:{port}" (IPv6 "[{host}]:{port}"), atau "{host}, port assigned at start" kalau `port` null |
| Endpoint Unix | sama | `socketPath`, atau "socket, created at start" kalau null |
| Version | `detail.provider.displayName`, `detail.runtime.version`, `detail.runtime.target` | "{displayName} {version} · {arch}" dengan `aarch64` "arm64" dan `x86_64` "x86_64" |
| Runtime | `detail.runtime.id` | UUID |
| Digest | `detail.runtime.artifactSha256`, `installMode` | "sha256:" ditambah 12 hex pertama (GUI: lengkap di `.help` dan label VoiceOver; CLI `inspect`: lengkap); external "External runtime" |
| Provider | `detail.provider` | "{name} {revision}" |
| Data | `detail.dataPath` | GUI: path dengan home disingkat `~` dan dipotong di tengah; CLI: path lengkap |
| Data size | `DataUsage` | "{bytes desimal}", "at least {bytes desimal}" kalau tidak lengkap, "Calculating…" selama dimuat, "Unavailable" kalau error |
| Uptime | `InstanceSummary.process.startedAt` | `UptimeText` spec 0022, atau "—" (CLI `-`) tanpa proses |
| Restarts | `InstanceSummary.restart` | "{attempts} in the last {menit} min" kalau lebih dari 0, "None in the last {menit} min" kalau 0, "Stopped restarting" kalau `exhausted` |
| Autostart | `detail.startup` | `manual` "Off", `at_login` "At login", asing "Unknown" |
| Connection tidak tersedia | `reason` | lihat *Copy* |

### Halaman detail dan navigasi

- `ShellDestination.instance(UUID)` (usulan spec 0022) memilih bagian Instances dan mendorong `InstanceRoute(id)` ke `NavigationStack` halaman Instances; `openInstance(_:)` memanggilnya. Tombol kembali sistem kembali ke tabel dengan pilihan baris tetap.
- Header `.pageChrome(title:subtitle:)`: judul `name`; subjudul nama tampil provider (dari detail, atau `software` sebelum detail termuat), `runtimeVersion`, dan teks `StatusLabel` lifecycle.
- Toolbar: tombol aksi utama `InstanceActionModel` (simbol `play.fill`, `stop.fill`, atau `arrow.clockwise` untuk Retry, ditambah label teks) lalu menu More (`ellipsis.circle`, label "More") dengan "Copy Connection String" (entri pertama, perilaku Copy yang sama, nonaktif dengan alasan "No connection string available." atau alasan entri). Semua item yang mengubah state memakai `.mutationGated()`; Copy tidak.
- `InstanceTab` hanya berisi `.summary` di fitur ini; picker tab tampil saat ada lebih dari satu tab.
- State halaman: instance tidak ada di `InstanceStore` → satu `instance.get`; `resource.not_found` → `StatePlaceholder` "Instance Not Found", "It may have been removed.", tombol Back to Instances; instance yang menjadi retained (event spec 0010) atau dihapus (`instance.removed` spec 0020) saat halaman terbuka menampilkan placeholder yang sama dengan pesan "This instance was removed.".

### CLI

**`stackly instance connection`**:

```text
stackly instance connection <instance> [--format <name>] [--all] [--show-password] [--json]
```

- Resolver nama atau UUID spec 0022 (`resolve.rs`), lalu `instance.get_detail`.
- Teks: tanpa `--all`, stdout tepat `masked` diikuti newline; kalau `portMayChange`, stderr menulis `note: the port can change the next time the instance starts`. `--all`: satu baris per entri `<name>  <masked>` atau `<name>  unavailable (<reason>)`, dengan lebar kolom nama tetap.
- `--show-password`: reveal per reference berbeda (`purpose: show`), lalu stdout tepat string lengkap; reveal yang gagal menghentikan perintah dengan error `secret.*` itu tanpa mencetak apa pun ke stdout.
- Kode area `cli` baru (spec 0008, aturan pemakai pertama): `cli.connection_not_found` (`not_found`, details `format`, `available`, pesan `no connection named <format>` atau `this provider describes no connection strings`, hint statis `list them with stackly instance connection <instance> --all`) dan `cli.connection_unavailable` (`unavailable`, details `format`, `reason`, pesan `the <format> connection is not available yet`, hint statis per `reason`: `port_not_assigned` "start the instance to assign its port", `socket_not_created` "start the instance to create its socket", `package_unavailable` "run stackly doctor", `retained` "the instance was removed").

**`stackly instance inspect`** (perluasan spec 0022): sesudah `instance.get`, perintah memanggil `instance.get_detail` dan `instance.get_data_usage` (kegagalan data usage tidak menggagalkan perintah). Blok `kv` mendapat baris sesudah `Version`: `Digest` (lengkap), `Data` (path lengkap), `Data size`, `Autostart`; sesudah `Logs`: `Dependents` (`none` atau daftar `<name> (<ownership>, lease active)`), dan `Secrets` (`<field>: <reference> (<status>)` per baris). Lalu tabel `CONNECTION`, `VALUE` dengan nilai tersamar atau `unavailable (<reason>)`. `--json`: `{ "instance": InstanceSummary, "detail": InstanceDetail, "dataUsage": DataUsage | null }`.

### Aksesibilitas

- Urutan fokus halaman detail: tombol kembali, aksi utama, More, banner dan tombolnya, kartu Endpoints (setiap tombol Copy), kartu Connection (Show Password, Copy per entri), kartu Details (Copy ID, refresh ukuran data).
- Label VoiceOver: header dibacakan "{name}, {provider} {versi}, Status: {label lifecycle}"; baris Readiness "Readiness: {teks}"; tombol Copy endpoint "Copy {nama endpoint} address"; nilai tersamar "Password, hidden" ditambah reference (aturan spec 0011); tombol Copy connection "Copy {title}"; tombol refresh "Recalculate data size". Hasil copy diumumkan "{title} copied." atau "Address copied.", dan perubahan readiness ke Ready diumumkan "{name} is ready.".
- Status proses dan readiness selalu dua teks terpisah; warna tone hanya tambahan.
- Teks membungkus di lebar konten 640 pt; nilai monospace yang panjang (connection, path) membungkus per karakter tanpa dipotong, dan tetap bisa dipilih.

### Copy

| Konteks | Teks |
| --- | --- |
| Connection `port_not_assigned` | "Start the instance to assign its port." |
| Connection `socket_not_created` | "The socket is created when the instance starts." |
| Connection `package_unavailable` | "Stackly can’t read this provider’s package. Run stackly doctor." |
| Connection `retained` | "This instance was removed." |
| Connection encoding asing | "Update Stackly to copy this connection string." |
| Provider tanpa blok connection | "This provider doesn’t describe a connection string. Use the endpoints above." |
| Port bisa berubah | "The port can change the next time the instance starts." |
| Detail gagal dimuat | "Couldn’t load instance details." |
| Instance tidak ada | "Instance Not Found" / "It may have been removed." |
| Copy berhasil | "{title} copied." / "Address copied." / "ID copied." |
| Reveal gagal | pesan `secret.*` dari daemon |
| Needs secret | "Needs a secret to start" / tombol "Enter Secret…" |

### Konstanta

| Konstanta | Nilai | Tempat |
| --- | --- | --- |
| `CONNECTION_ENTRIES_MAX` | 8 | `core::provider` |
| `CONNECTION_TEMPLATE_MAX` | 512 byte | `core::provider` |
| `DATA_USAGE_MAX_ENTRIES` | 200 000 | `stacklyd` |
| `DATA_USAGE_DEADLINE` | 2 detik | `stacklyd` |
| `DATA_USAGE_CACHE_TTL` | 60 detik | `stacklyd` |
| `PINNED_PACKAGE_CACHE` | 32 entri | `stacklyd` |
| `DETAIL_RELOAD_DEBOUNCE` | 250 ms | `InstanceDetailModel` |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| detail | `provider.name`, `revision`, `manifestSha256` | `Instance.provider_revision` (registry) |
| detail | `provider.displayName` | `metadata.displayName` paket pinned di `provider_revision_dir(key)` (cache paket) |
| detail | `runtime.*` | `Registry::runtime(instance.runtime_id)`; `artifactSha256` dari `InstallMode::Managed` |
| detail | `dataPath` | `DirectoryLayout::instance_paths(id).data` |
| detail | `startup` | `Instance.startup` |
| detail | `secrets[]` | `secret_uses_by(Instance(id))`, status dari cache `secret_references` (spec 0011), judul dari `title` field schema paket pinned (cadangan: nama field) |
| detail | `dependents[]` | `Registry::environments()` yang punya member instance itu (ownership dari member), `lease_active` dari `Registry::leases(id, Unreleased)` berholder environment itu |
| detail | `connections[]` | blok `connection` paket pinned, `render` (AC-2) dengan port efektif, folder run, config revision aktif, dan reference dari `secret_uses_by` |
| detail | `revision` | `Instance.revision` saat dibaca |
| data usage | `bytes`, `files`, `complete` | `Filesystem::tree_usage(instance_paths(id).data, limits)` |
| data usage | `scannedAt` | `Clock::now()` saat scan selesai |
| Summary | status, endpoint, readiness, proses, restart, blocker, `lastFailure` | `InstanceSummary` dari `InstanceStore` (spec 0010, 0013) |
| Summary | uptime | `InstanceSummary.process.startedAt` dan jam lokal (`UptimeText`) |
| Summary | nilai secret untuk Show dan Copy | `secret.reveal { reference, purpose }` (spec 0011) |
| Summary | string lengkap | `ConnectionComposer.compose(parts, values)` |
| Summary | judul provider dan tile | detail (`provider.displayName`); tile dan mark dari `CatalogStore` kalau ada, selain itu tile `gray` |
| CLI connection | entri terpilih | `--format` atau entri pertama `detail.connections` |
| CLI connection | string lengkap | `secret.reveal` dan `stackly_protocol::instance::connection::compose` |
| offline test | akses catalog, fetch, assess | penghitung di `stacklyd` (akses `CatalogState`), `Fetcher` tiruan, `CodeSigning` tiruan |

### Key invariants

- Process state dan readiness selalu tampil sebagai dua nilai terpisah; tidak ada teks yang menyebut instance siap tanpa readiness `ready` dari probe yang segar (BR-010).
- Daemon tidak pernah mengirim nilai secret di `InstanceDetail`, event, atau error; string tersamar memakai empat bintang tetap.
- Tidak ada reveal, tulisan pasteboard, atau cetak password tanpa aksi eksplisit pengguna.
- Start, status, detail, data usage, dan connection tidak pernah membaca catalog, memanggil jaringan, atau menilai signature secara online (REQ-046).
- Connection dirender dari paket pinned yang sama yang dipakai plan, sehingga host, port, dan socket nya selalu sama dengan yang di bind proses.

### Security model

- Semua method lewat socket privat milik user yang sama (spec 0006); `instance.get_detail` mengirim path absolut folder data hanya ke pemiliknya (pola `RuntimeDetail.path` spec 0018).
- Nilai secret hanya keluar lewat `secret.reveal`, yang diaudit per purpose (spec 0011 AC-9); CLI hanya mencetaknya dengan `--show-password`, GUI hanya menampilkannya di `RevealableSecretText` yang tersembunyi lagi setelah 30 detik dan hanya menyalinnya lewat `ConcealedPasteboard` (host saja, transient, dikosongkan setelah 90 detik).
- String tersamar boleh memuat nilai settings teks (misalnya nama database) karena itu config pengguna sendiri; daemon tidak mencatat isi detail ke log.
- Template connection divalidasi saat paket dimuat; token secret dan settings di encode sesuai `encoding`, sehingga nilai pengguna tidak bisa mengubah struktur URI.
- Scan ukuran data tidak mengikuti symlink dan tidak menyeberang device, dan dibatasi jumlah entri serta waktu.

### Configuration required

Tidak ada variabel environment, setting, atau credential baru.

### Critical test scenarios

- Manifest: blok `connection` valid untuk PostgreSQL, NATS, dan fake; setiap aturan penolakan AC-1 dengan path field nya. Memverifikasi **AC-1**.
- Render (unit test murni): IPv4 dan IPv6, port `Fixed`, `Auto` dengan dan tanpa `bound`, `running` dan `stopped` (`port_may_change`), socket dengan folder run berspasi dan Unicode, settings teks dengan `/`, `?`, dan spasi, entri tanpa secret, paket rusak, instance retained. Memverifikasi **AC-2**.
- Komposisi: setiap kasus `connection-compose.json` di Rust dan Swift. Memverifikasi **AC-3**.
- Detail: instance fake dengan secret `authToken`, dua environment fixture (dedicated dengan lease aktif, shared tanpa lease), paket pinned yang dirusak; penghitung `SecretStore` nol. Memverifikasi **AC-4**.
- Data usage: pohon dengan symlink keluar, hard link, file sparse, volume lain, 250 000 entri (`complete: false`), folder hilang dan bukan privat, dua permintaan bersamaan (satu scan), cache 60 detik. Memverifikasi **AC-5**.
- CLI connection: default tersamar, `--format socket`, format tidak ada, entri tidak tersedia, provider tanpa blok, `--all`, `--show-password` dengan audit, `--all --show-password` exit 2, golden teks dan JSON. Memverifikasi **AC-6**.
- Paritas: golden `inspect` dan test Swift dari tiga fixture detail. Memverifikasi **AC-7**.
- AC-012: fake dengan `readyDelayMs` panjang, lalu probe berhasil. Memverifikasi **AC-8**.
- Start dari halaman detail: view model dengan service tiruan dan urutan event (Starting, Running not ready, Ready), connection `port_not_assigned` menjadi tersedia. Memverifikasi **AC-9**.
- Offline: daemon tanpa catalog, `Fetcher` dan `CodeSigning` tiruan yang gagal saat dipanggil, penghitung akses catalog nol. Memverifikasi **AC-10**.
- GUI: navigasi (klik ganda, Return, Open, Overview, setelah create, kembali, tidak ditemukan, dihapus saat terbuka), toolbar dan More, kartu Summary dari fixture, Copy endpoint (pasteboard biasa) dan Copy connection (concealed, setelah reveal), Show Password dan sembunyi otomatis, banner I1 sampai I4 dan needs secret, state detail gagal dan offline, inspector Open, Data, Connection. Memverifikasi **AC-11** sampai **AC-15**.
- Aksesibilitas: evidence manual keyboard dan VoiceOver untuk alur Start sampai Copy. Memverifikasi **AC-16**.
- Kontrak: fixture setiap DTO, kode, dan nilai asing. Memverifikasi **AC-17**.
- Sentinel: prefix fixture credential tidak ada di detail, event, log, output CLI default, dan state Swift. Memverifikasi **AC-18**.
- Kinerja: 10 instance, cache dingin dan hangat. Memverifikasi **AC-19**.
- Native: PostgreSQL 18 dan NATS lewat GUI dan CLI, `psql` dan `INFO`, lalu jaringan mati setelah persetujuan. Memverifikasi **AC-10**, **AC-20**.

### Jawaban untuk follow up spec lain

| Spec dan baris Follow-up | Jawaban di spec ini |
| --- | --- |
| 0009: `Endpoint` domain membawa `purpose` atau dibaca dari paket (fitur 20 dan 21) | dibaca dari paket pinned (keputusan spec 0020 dipertahankan); connection tidak memakai purpose sama sekali, melainkan blok `connection` yang menyebut endpoint nya sendiri |
| 0011: connection URI tersamar secara default dan `RevealableSecretText` untuk Show Password dan Copy | AC-2, AC-13; closure komposisi spec 0011 diisi `ConnectionComposer` |
| 0019: bentuk connection string PostgreSQL dan tempatnya di manifest | blok `connection` (AC-1) dan entri `uri` serta `socket` di *Connection PostgreSQL dan NATS* |
| 0020: purpose endpoint dan template socket dari paket pinned; socket path hanya saat `socketPath` tidak null | render memakai paket pinned dan port efektif; entri socket `socket_not_created` saat path belum ada (AC-2) |
| 0022: ganti `openInstance` dengan halaman detail, Open di inspector, "Double-click a row to open it.", baris Data dan Connection di inspector | AC-11, AC-15, usulan spec 0022 |
| 0023: connection string NATS dari endpoint `client`, tanpa credential, dan URL monitoring | entri `uri` dan `monitoring` di *Connection PostgreSQL dan NATS* |
| 0013: pemilik halaman detail instance | AC-11 sampai AC-14 |
| 0004: BR-001 dan BR-010 | BR-001 tidak berubah (detail membaca runtime dan revision exact); BR-010 di AC-8 |
| 0010: start, readiness, `instance.get`, event | dipakai apa adanya (AC-8, AC-9) |
| 0012: tata letak kartu Summary | `surfaceWindow` dan `.cardSurface()` (*Tab Summary*) |

### Usulan update spec lain

Ditulis di sini karena spec ini tidak boleh mengubah spec lain; lead membawanya ke pemilik masing masing sebelum milestone yang membutuhkannya. Satu blok per spec target, setiap blok berdiri sendiri. Teks lama dikutip persis dari spec target di `main` (2026-10-20). "Ganti" berarti teks baru menggantikan teks lama; "Sisipkan sesudah" berarti teks lama tetap dan teks baru ditambahkan tepat sesudahnya.

#### Spec 0004 (domain core dan abstraksi platform)

1. *Port platform*, trait `Filesystem`. Sisipkan sesudah baris berikut (dan sesudah baris `volume_facts` usulan spec 0020 kalau sudah masuk):

   ```text
       fn remove_tree(&self, root: &Path, relative: &Path) -> Result<RemoveReport, PlatformError>;
   ```

   baris:

   ```text
       // ditambah fitur 21 (spec 0021):
       fn tree_usage(&self, root: &Path, limits: UsageLimits) -> Result<TreeUsage, PlatformError>;
   ```

   dan sesudah butir `remove_tree` di bawah blok kode, butir baru:

   ```text
   - `tree_usage(root, UsageLimits { max_entries, deadline })` *(ditambah fitur 21, spec 0021)* mengembalikan `TreeUsage { bytes, files, complete }`: jumlah `st_blocks * 512` setiap entri di bawah `root` termasuk folder, hard link dihitung sekali per (device, inode), tanpa mengikuti symlink, tanpa menyeberang device, dan `complete: false` saat batas entri atau tenggat tercapai. Adapter macOS berbasis file descriptor (`openat` dengan `O_NOFOLLOW`, `fstatat` dengan `AT_SYMLINK_NOFOLLOW`); `FakeFilesystem` memakai penelusuran portabel dengan semantik yang sama; `fs_conformance` memeriksa symlink keluar, hard link, dan batas entri.
   ```

#### Spec 0009 (provider engine dan manifest schema)

1. *Manifest `stackly/v1alpha1`*, tabel field. Sisipkan sesudah:

   ```text
   | `permissions.additionalExecutables` | peta alias ke path relatif | tidak | paling banyak 16, path ada di `executables` artefak |
   ```

   baris:

   ```text
   | `connection` | daftar `{ name, title, encoding, template }` | tidak | 1 sampai 8 entri; `name` `^[a-z][a-z0-9_]{0,31}$` dan unik (`connection_name_duplicate`), `title` 1 sampai 64 karakter tanpa karakter kontrol, `encoding` `uri` atau `none`, `template` 1 sampai 512 byte di posisi `connection.template`; entri pertama adalah connection utama (spec 0021) |
   ```

2. *Interpolasi terkontrol*, matriks posisi. Sisipkan sesudah:

   ```text
   | `{{ }}` di template | ya | ya | ya (sebagai teks) | ya (sebagai teks) | tidak | hanya di config `sensitive` | |
   ```

   baris:

   ```text
   | `connection.template` | ya (`endpoint.<n>.address`, `port`, `host` hanya untuk `encoding: none`, `instance.name`, `instance.id`, `settings.<field>` skalar bukan secret dan bukan managed, `provider.id`, `provider.revision`, `runtime.version`) | ya | hanya `endpoint.<n>.socket` dan `endpoint.<n>.socketDir` | tidak | tidak | ya, `secret.<field>` | dengan `encoding: uri`, nilai token selain `endpoint.<n>.address` dan `endpoint.<n>.port` di percent encode kecuali karakter unreserved RFC 3986; teks literal tidak di encode (spec 0021) |
   ```

#### Spec 0019 (provider PostgreSQL)

1. *Paket*, manifest. Sisipkan sesudah:

   ```text
       socket: { transport: unix, purpose: client, socketName: ".s.PGSQL.${endpoint.client.port}" }
   ```

   blok tingkat atas:

   ```text
   connection:
     - name: uri
       title: Connection string
       encoding: uri
       template: "postgresql://postgres:${secret.superuserPassword}@${endpoint.client.address}/postgres"
     - name: socket
       title: Unix socket
       encoding: uri
       template: "postgresql://postgres:${secret.superuserPassword}@/postgres?host=${endpoint.socket.socketDir}&port=${endpoint.client.port}"
   ```

   Blok ini masuk sebelum langkah evidence BR-025 spec 0019, karena ikut `manifest_sha256`.

2. *Value sourcing*. Ganti:

   ```text
   | connection (fitur 21) | user, database, password, endpoint | `postgres`, `postgres`, field `superuserPassword`, endpoint `client` atau `socket` (Follow-up) |
   ```

   dengan:

   ```text
   | connection (fitur 21) | connection string | blok `connection` paket ini (entri `uri` lewat endpoint `client`, entri `socket` lewat folder socket), dirender daemon dan disusun client dengan `superuserPassword` (spec 0021) |
   ```

#### Spec 0022 (overview dan daftar instance)

1. *Navigasi*, `ShellDestination`. Ganti:

   ```text
   - `ShellDestination`: `.overview`, `.instances(filter: InstanceFilter = .all, select: UUID? = nil)`, `.catalog`, `.runtimes`, `.operations`, `.diagnostics`.
   ```

   dengan:

   ```text
   - `ShellDestination`: `.overview`, `.instances(filter: InstanceFilter = .all, select: UUID? = nil)`, `.instance(UUID)` (halaman detail fitur 21: bagian Instances dengan `InstanceRoute` di `NavigationStack`, spec 0021), `.catalog`, `.runtimes`, `.operations`, `.diagnostics`.
   ```

2. *Navigasi*, `openInstance`. Ganti:

   ```text
   - `openInstance(_ id:)` sama dengan `.instances(filter: .all, select: id)` (`// provisional: F21`, diganti halaman detail fitur 21).
   ```

   dengan:

   ```text
   - `openInstance(_ id:)` sama dengan `.instance(id)` (spec 0021).
   ```

3. *Halaman Instances*, inspector tanpa pilihan. Ganti:

   ```text
   5. Tanpa pilihan: `StatePlaceholder` ukuran `.section` "No Selection", "Select an instance to see its details.".
   ```

   dengan:

   ```text
   5. Tanpa pilihan: `StatePlaceholder` ukuran `.section` "No Selection", "Select an instance to see its details. Double-click a row to open it." (spec 0021).
   ```

4. *Halaman Instances*, catatan pemilik. Ganti:

   ```text
   - Data path, ukuran data, connection string, dan Open milik fitur 21.
   ```

   dengan:

   ```text
   - Fitur 21 (spec 0021) menambah tombol Open di samping aksi utama, baris Data (ukuran dari `instance.get_data_usage`, path di baris kedua) sesudah Version, dan baris Connection (entri pertama tersamar dengan Show Password dan Copy) sesudah Endpoints.
   ```

5. *Requirements*, AC-11. Ganti:

   ```text
   dengan `--json` memberi `InstanceSummary`.
   ```

   dengan:

   ```text
   dengan `--json` memberi `result` `{ "instance": InstanceSummary }`; fitur 21 menambah key `detail` dan `dataUsage` (spec 0021).
   ```

#### Spec 0023 (provider NATS lewat manifest)

1. *Paket NATS*. Ganti:

   ```text
   Satu satunya perubahan fitur ini di `providers/nats/provider.yaml` adalah blok `readiness`:
   ```

   dengan:

   ```text
   Perubahan fitur ini di `providers/nats/provider.yaml` adalah blok `readiness` di bawah, dan blok `connection` dari spec 0021 (entri `uri` `nats://${endpoint.client.address}` berjudul "Client URL" dan entri `monitoring` `http://${endpoint.monitoring.address}` berjudul "Monitoring URL", keduanya `encoding: uri`, tanpa credential sampai fitur 39), yang ditulis di commit yang sama supaya `manifest_sha256` hanya berubah sekali sebelum evidence BR-025:
   ```

Tidak ada update untuk spec 0006, 0008, 0010, 0011, 0012, dan 0013: method dan kode baru masuk area pemiliknya lewat aturan pemakai pertama, `socketPath` sudah diusulkan spec 0020, dan komponen spec 0011 dipakai apa adanya.

### Selisih mockup yang disengaja

Untuk dicatat lead di `ui-macos.md`:

- Readiness menampilkan state, detail probe, dan umur pemeriksaan, bukan latensi probe ("SELECT 1 · 8 ms"); latensi milik fitur 28.
- Restarts di Details adalah jumlah di jendela restart policy ("{n} in the last 5 min"), bukan jumlah sejak dibuat; jumlah sejak dibuat milik fitur 28.
- Runtime menampilkan UUID runtime, bukan ID buatan mockup (`rt-pg-17.6`); Instance ID menampilkan UUID, bukan `inst_...`.
- Secret reference memakai bentuk spec 0011 (`local:instance/<id>/superuser-password`), bukan `keychain:...`.
- Kartu Connection menampilkan semua entri provider (PostgreSQL: Connection string dan Unix socket; NATS: Client URL dan Monitoring URL), bukan satu string.
- Tab Configuration, Logs, Metrics, Backups, dan Activity serta tombol Restart, Reload, Duplicate, dan Remove tidak tampil sampai fitur pemiliknya dibangun.
- Banner gagal tidak menampilkan "Use Port …" atau Show Logs sampai fitur 29 dan 27; Retry saja.
- Autostart menulis "Off" atau "At login", bukan "With environment".

## Build plan

Urutan mengikuti pendekatan Journey proyek. Jalur yang dibuat lengkap adalah "pengguna menjalankan instance, melihat ready, lalu menyalin connection", lebih dulu di core (manifest, render, ukuran data), lalu lewat daemon dan CLI dengan bukti offline, lalu di GUI, lalu dibuktikan dengan PostgreSQL dan NATS nyata di Mac ini.

**Milestone 1: Connection dan ukuran data di core** (needs `/architect` update spec 0004 dan 0009, dan usulan paket spec 0019 dan 0023; fitur 9 build; fitur 19 milestone 1 (paket PostgreSQL) dan fitur 23 langkah 2 (perubahan paket NATS) untuk blok paket)

1. Blok `connection` di `core::provider` (raw, validasi, posisi matriks, JSON Schema `schemas/provider/`, `provider check`), dengan test penolakan. Memenuhi **AC-1**.
2. Blok `connection` di paket PostgreSQL, NATS, dan fake, dengan golden render dan kenaikan revision catalog sesuai aturan spec 0017. Memenuhi **AC-1**.
3. `core::instance::connection::render` dengan test tabel, dan `stackly_protocol::instance::connection::compose` dengan fixture `connection-compose.json`. Memenuhi **AC-2**, **AC-3**.
4. `Filesystem::tree_usage` di `MacosFilesystem` dan `FakeFilesystem` dengan `fs_conformance` dan test volume lain di macOS. Memenuhi **AC-5**.
5. `core::instance::detail::build` (fungsi murni dari snapshot registry, paket pinned, dan layout) dengan test. Memenuhi **AC-4**.

Sesi paralel: langkah 1 dan 4 bisa menjadi dua sesi (provider dan platform); langkah 2 dan 3 setelah langkah 1; langkah 5 setelah langkah 3.

**Milestone 2: Kontrak, daemon, CLI, dan bukti offline** (after milestone 1; needs fitur 6, fitur 8 build, fitur 10 milestone 4, fitur 11 langkah 5 (`secret.reveal` lewat daemon) dan langkah 8 (CLI secret), dan fitur 14 di `01-foundation.md`; fitur 22 milestone 2 (`resolve.rs`, `inspect`) dan usulan spec 0022 nomor 5; spec 0020 milestone 2 untuk `socketPath`)

6. Protokol: DTO, dua method di tabel `methods!` (`recovery refused`), enum terbuka, kode `instance.data_usage_unavailable` dan kode `cli` baru dengan hint statis, fixture termasuk `detail-*.json`, kenaikan minor, `cargo xtask codegen`, dan test kontrak Swift. Memenuhi **AC-17**.
7. `stacklyd`: handler kedua method, cache paket pinned, cache dan penggabungan scan data usage, penghitung akses catalog untuk test, dan integration test daemon (detail, data usage, offline, AC-012 bagian daemon, sentinel). Memenuhi **AC-4**, **AC-5**, **AC-8**, **AC-10**, **AC-18**, **AC-19**.
8. CLI: `stackly instance connection` dan perluasan `inspect` dengan golden dan test paritas. Memenuhi **AC-6**, **AC-7**, **AC-8**.

Sesi paralel: setelah langkah 6 masuk, langkah 7 dan 8 bisa menjadi dua sesi (daemon dan CLI), dengan test CLI terhadap daemon nyata menunggu langkah 7 di merge.

**Milestone 3: Halaman detail dan tab Summary** (after langkah 6; needs fitur 12 dan fitur 13 milestone 4 (`ui-macos.md`), fitur 11 langkah 10 (`RevealableSecretText`, `ConcealedPasteboard`, `SecretEntrySheet`), fitur 22 milestone 3 (Instances, inspector, `InstanceActionModel`, `ShellNavigator`) dan usulan spec 0022 nomor 1 sampai 4; memakai komponen fitur 67 dan peta perintah fitur 69 kalau sudah ada)

9. `StacklyClient` method baru, `ConnectionComposer` dengan fixture komposisi, `DetailFormat` dengan fixture detail, dan `InstanceDetailModel` dengan service tiruan. Memenuhi **AC-3**, **AC-7**, **AC-9**, **AC-14**.
10. `InstanceDetailPage`, navigasi `.instance(id)`, header, toolbar, kartu Summary, banner, state, dan tambahan inspector, dengan test dan preview. Memenuhi **AC-8**, **AC-11** sampai **AC-15**.
11. Evidence manual keyboard dan VoiceOver untuk alur Start sampai Copy dengan daemon development. Memenuhi **AC-16**.

Sesi paralel: milestone 3 bisa berjalan bersamaan dengan langkah 7 dan 8 karena hanya bergantung pada fixture langkah 6.

**Milestone 4: PostgreSQL dan NATS nyata, dan offline di Mac ini** (after milestone 2 dan 3; needs fitur 19 milestone 2, fitur 23 milestone 2, fitur 18 milestone 2 (`install_cached_artifact`), dan spec 0020 milestone 2 dan 3 (create lewat daemon dan GUI); artefak di cache runtime xtask, selain itu `BLOCKED`)

12. Test native: PostgreSQL 18 dan NATS di start dari CLI dan GUI sampai `ready`, `psql` milik runtime menjalankan `SELECT 1` dengan string `uri` dan `socket` hasil komposisi, dan klien TCP menerima `INFO` NATS lewat string `uri`. Memenuhi **AC-20**.
13. Run offline dengan jaringan Mac ini dimatikan setelah persetujuan user (start, status, Summary, `connection`, `inspect`), dicatat evidence `native`; `BLOCKED` sampai disetujui. Memenuhi **AC-10**.

## Consequences

**Positive**:

- JOURNEY-01 selesai dari GUI dan CLI dengan password yang tidak pernah tampil atau tersalin tanpa diminta.
- Provider berikutnya (Valkey, MySQL, ClickHouse, Traefik) cukup menulis blok `connection` di manifest untuk mendapat tombol Copy dan `stackly instance connection`.
- Connection dirender dari paket pinned dan port efektif yang sama dengan plan, jadi tidak pernah menunjuk host atau port yang salah.
- Semua jalur lokal terbukti tidak menyentuh catalog atau jaringan.

**Negative / tradeoffs**:

- Blok `connection` mengubah `manifest_sha256` paket PostgreSQL dan NATS, jadi evidence BR-025 dan revision catalog harus menunggu atau diulang; lead harus mengurutkan update spec 0009, 0019, dan 0023 sebelum evidence kedua provider.
- Fungsi komposisi ada dua kali (Rust dan Swift); fixture bersama menjaganya tetap sama, tetapi setiap perubahan aturan encode menyentuh dua tempat.
- Scan ukuran data memakan IO; batas entri dan waktu membuat folder besar ditampilkan "at least".
- `stackly instance inspect --json` berubah bentuk menjadi object pembungkus, jadi usulan spec 0022 harus masuk sebelum fitur 22 dibangun supaya tidak menjadi perubahan breaking.
- Bukti offline native butuh jaringan Mac ini dimatikan, langkah manual yang menunggu persetujuan user.
- Restarts sejak dibuat dan latensi probe tidak ada sampai fitur 28.

**Neutral**:

- Dua method baru, satu kode wire, dua kode `cli`, satu field manifest, satu method `Filesystem`, dan kenaikan minor protokol.
- Folder Swift baru `InstanceDetail/`, modul core `instance::connection` dan `instance::detail`.

## Follow-up

- [ ] Lead: bawa *Usulan update spec lain* (spec 0004, 0009, 0019, 0022, 0023) ke pemiliknya sebelum milestone 1 dan 2; urutkan blok `connection` paket sebelum evidence BR-025 fitur 19 dan 23.
- [ ] Lead: catat *Selisih mockup yang disengaja* di `ui-macos.md` lewat `/scope`.
- [ ] Fitur 24, 26, 27, 28, 29, 35, 49, dan 54: tambah tab dan tombol masing masing ke halaman detail lewat `InstanceTab` dan toolbar spec ini.
- [ ] Fitur 28: jumlah restart sejak dibuat dan latensi probe di baris Readiness dan Restarts.
- [ ] Fitur 33: banner crash loop yang lebih kaya di atas banner I1 spec ini.
- [ ] Fitur 39: credential NATS di template `uri` NATS.
- [ ] Fitur 44 sampai 47: isi Dependents dari environment nyata (detail sudah membaca registry).
- [ ] `/sync` setelah build: catat `instance.get_detail`, `instance.get_data_usage`, blok `connection` manifest, `stackly instance connection`, dan folder `InstanceDetail/` di `AGENTS.md` root dan `apps/macos/AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, dan hasil cross check ada di [rationale.md](rationale.md).
