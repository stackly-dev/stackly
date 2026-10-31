# 0021. Start, readiness, dan connection
> Perubahan cakupan 2026-10-24: dukungan pembaca layar khusus, navigasi dan shortcut keyboard khusus, pengaturan tampilan aksesibilitas, dan identifier UI test dihapus atas permintaan pengguna. Ketentuan serta cek khusus kemampuan tersebut di dokumen ini tidak berlaku; nomor kriteria tetap stabil.


**Date**: 2026-10-20
**Status**: Accepted

## Summary

Spec ini menutup JOURNEY-01: setelah instance dibuat, pengguna menjalankannya, melihat proses dan readiness sebagai dua status terpisah, lalu menyalin connection string. Connection string dideklarasikan provider di manifest (blok `connection` baru berisi template URI), dirender daemon dari paket yang di pin dan port efektif lewat mesin interpolasi yang sama dengan plan, lalu dikirim ke GUI dan CLI dalam bentuk tersamar; password asli hanya diambil lewat `secret.reveal` saat pengguna menekan Show Password atau Copy, atau memberi `--show-password`. Halaman detail instance dengan tab Summary (endpoint, proses, readiness, connection, secret reference, dependents, dan detail) dan `stackly instance connection` membaca data yang sama dari method baru `instance.get_detail`, ditambah `instance.get_data_usage` untuk ukuran folder data yang dihitung hanya saat diminta. Start, status, dan connection hanya memakai registry, paket pinned, dan runtime terpasang, jadi semuanya tetap bekerja tanpa jaringan dan tanpa catalog.

## Requirements

**User stories**:

- Sebagai pengguna yang baru membuat PostgreSQL, saya ingin menekan Start, melihat instance berjalan lalu siap, dan menyalin connection string ke aplikasi saya, supaya JOURNEY-01 selesai tanpa membuka terminal.
- Sebagai pengguna, saya ingin proses yang hidup tetapi belum siap tidak pernah tampil sebagai siap, supaya saya tidak menghubungkan aplikasi ke database yang belum menerima koneksi.
- Sebagai pengguna, saya ingin password tidak pernah tampil atau tersalin tanpa saya minta, supaya layar dan clipboard saya tidak membocorkannya.
- Sebagai pengguna tanpa internet, saya ingin instance yang runtime nya sudah terpasang tetap bisa start, menampilkan status, dan memberi connection, supaya kerja lokal saya tidak bergantung pada jaringan.
- Sebagai penulis script, saya ingin `stackly instance connection <instance> --format uri` yang tersamar secara default dan `stackly instance inspect` yang berisi data yang sama dengan tab Summary.
- Sebagai penulis provider, saya ingin connection string dideklarasikan di manifest, supaya provider baru tidak butuh kode Rust atau Swift untuk tombol Copy.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-012):

- **AC-1** (blok `connection` di manifest): manifest `stackly/v1alpha1` menerima blok opsional `connection`, daftar 1 sampai 8 entri `{ name, title, template }` seperti *Blok connection di manifest*; setiap template adalah URI. Loader menolak dengan path field dan kode issue: `name` di luar pola `^[a-z][a-z0-9_]{0,31}$`, `title` kosong, lebih dari 64 karakter, atau berkarakter kontrol, dan `template` kosong atau di atas 512 byte dengan `connection_invalid`; `name` ganda dengan `connection_name_duplicate`; token di luar baris matriks posisi `connection.template` (termasuk `endpoint.<n>.host` dan `settings.*`) dengan `token_not_allowed_here`; endpoint atau secret yang tidak ada dengan `unknown_endpoint` dan `unknown_secret`; dan template yang hasil render nya dengan nilai sentinel (port 15432, host `127.0.0.1` dan `::1`, folder run `/stackly sentinel/run`, secret `sentinel value`) gagal diparse sebagai URI absolut RFC 3986 dengan `connection_invalid` reason `not_a_uri`. Blok ini ikut `manifest_sha256`. Reason ada di `Issue.reason` (spec 0009). Paket `providers/postgresql` dan `providers/fake` membawa blok di *Connection PostgreSQL, NATS, dan fake* (ditulis langkah 2), paket `providers/nats` membawanya lewat langkah 2 spec 0023 di commit yang sama dengan readiness (keputusan lead D1), dan `cargo xtask provider check` serta golden render ketiga paket lolos.
- **AC-2** (render connection): `instance::connection::render(package, ctx)` memakai `PlanContext` dan resolver interpolasi spec 0009 yang sama dengan plan, lalu mengembalikan satu `RenderedConnection` per entri manifest dalam urutan manifest. Status entri dipilih dengan urutan `retained` (instance retained), lalu `package_unavailable` kalau paket yang diberikan bukan paket pinned instance (`key()` berbeda dari provider revision instance), lalu kegagalan render pertama entri itu: `port_not_assigned` untuk setiap `EndpointUnresolved` (endpoint yang dirujuk belum punya port efektif, atau endpoint manifest yang tidak ada di context) atau `package_unavailable` untuk kegagalan render lain dan field secret tanpa pemakaian tercatat, selain itu tersedia. Paket pinned yang tidak terbaca tidak punya entri untuk dirender, jadi `render` tidak dipanggil untuk nya; AC-4 melaporkan nya lewat `provider.packageReadable` *(diperjelas 2026-10-22)*. Entri tersedia membawa `parts` (teks yang sudah di encode dan bagian secret yang hanya membawa field dan reference) dan `masked` (gabungan teks dengan setiap bagian secret diganti `****`, selalu empat bintang). Nilai setiap token di percent encode sebagai byte UTF 8 dengan hex huruf besar, kecuali karakter unreserved RFC 3986 (`A-Z a-z 0-9 - . _ ~`), kecuali `endpoint.<n>.port` dan `endpoint.<n>.address` (IPv6 dalam kurung siku) yang disisipkan apa adanya; teks literal template tidak pernah di encode. `port_may_change` bernilai true jika dan hanya jika entri merujuk endpoint `Auto` dan lifecycle instance bukan `running`. Render tidak pernah membaca Keychain atau filesystem selain paket pinned di cache.
- **AC-3** (komposisi): fungsi murni `compose(parts, values)` dengan `values` berupa peta reference ke nilai secret ada dua kali dengan perilaku sama, `stackly_protocol::instance::connection::compose` (Rust, dipakai CLI) dan `ConnectionComposer.compose(parts:values:)` (Swift, `StacklyUI`): teks apa adanya, nilai secret di percent encode dengan aturan AC-2, dan reference yang tidak ada di `values` menghasilkan error `MissingValue { reference }`, tidak pernah string kosong. Keduanya menghasilkan string yang sama persis untuk setiap kasus fixture `protocols/fixtures/instance/connection-compose.json` (password dengan spasi, `@`, `:`, `/`, `%`, `?`, `#`, `'`, `"`, `\`, emoji, dan huruf beraksen; entri dengan dua secret; nilai yang hilang), dan bagian teks fixture yang sama (nilai `instance.name` dan folder socket berspasi dan Unicode) dijalankan juga lewat `render` core dan menghasilkan teks yang sama. Nilai secret tidak pernah ada di DTO `InstanceDetail`.
- **AC-4** (`instance.get_detail`): `instance.get_detail { instanceId }` (`recovery refused`, hanya membaca) membalas `InstanceDetail` seperti *Method IPC* dengan setiap field bersumber dari tabel *Value sourcing*: provider dan runtime dari registry dan paket pinned (nama tampil dari `metadata.displayName` paket pinned, null kalau paket tidak terbaca, tidak pernah dari catalog), target runtime sebagai `HostTarget`, path data absolut dari baris path `data` registry yang diselesaikan lewat `RootSet::resolve`, `startup`, satu baris secret per pemakaian secret instance urut field (judul dari schema paket pinned atau nama field, status dari cache spec 0011 atau `unknown` kalau belum ada), dependents dari environment dan lease registry urut nama lalu ID, dan connection dari AC-2. Paket pinned yang tidak terbaca tidak menggagalkan method: `provider.packageReadable` bernilai false (juga saat paket di cache bukan paket pinned instance), `provider.displayName` null, `connections` kosong, dan judul secret memakai nama field; selain itu `packageReadable` true, juga untuk paket tanpa blok `connection` yang `connections` nya kosong. Kartu Connection dan `stackly instance connection` memperlakukan `packageReadable` false sebagai alasan `package_unavailable` (teks *Copy* dan `cli.connection_unavailable`), bukan sebagai provider tanpa blok `connection` *(ditambah 2026-10-22)*. ID yang tidak ada menghasilkan `resource.not_found`. Method ini tidak menulis apa pun, tidak spawn, dan tidak pernah membaca nilai secret. Setiap request memeriksa ulang `provider.yaml` di folder pinned (inode, ukuran, mtime) sebelum memakai cache paket, dan perbedaan memicu baca ulang dengan cek digest.
- **AC-5** (`instance.get_data_usage`): `instance.get_data_usage { instanceId, refresh, cachedOnly }` (`recovery refused`) membalas `DataUsageReply { usage: DataUsage | null }` dengan `DataUsage { bytes, complete, scannedAt }` untuk folder data instance itu dari `Filesystem::tree_usage` (spec 0004): jumlah blok teralokasi (`st_blocks` kali 512) setiap entri, hard link dihitung sekali, tanpa mengikuti symlink, tanpa menyeberang device, dan berhenti dengan `complete: false` saat entri ke `DATA_USAGE_MAX_ENTRIES + 1` terbaca, setelah `DATA_USAGE_DEADLINE`, atau di folder yang lebih dalam dari `TREE_USAGE_MAX_DEPTH` tingkat, yang tidak dibuka dan isinya tidak dihitung *(diubah 2026-10-22, keputusan `tree_usage`)*. Hasil lengkap di cache per instance selama `DATA_USAGE_CACHE_TTL` dan hasil tidak lengkap selama `DATA_USAGE_PARTIAL_TTL`; `refresh: true` melewati cache; `cachedOnly: true` tidak pernah memulai scan dan membalas `usage: null` kalau cache kosong; permintaan bersamaan untuk instance yang sama memakai satu scan; paling banyak `DATA_USAGE_CONCURRENCY` scan berjalan bersamaan untuk semua instance. Folder yang hilang, bukan folder privat milik user, atau tidak terbaca menghasilkan `instance.data_usage_unavailable` (tabel *Kode error*). Test memakai pohon dengan symlink keluar root, hard link, file sparse, folder di volume lain (image disk sementara di macOS), pohon yang lebih dalam dari `TREE_USAGE_MAX_DEPTH`, dan 250 000 entri.
- **AC-6** (`stackly instance connection`): `stackly instance connection <instance> [--format <name> | --all] [--show-password]` menerima nama atau UUID lewat resolver spec 0022, memanggil `instance.get_detail`, dan tanpa `--format` memilih entri pertama. Stdout default berisi tepat string `masked` diikuti newline, tanpa password. `--format` yang tidak ada gagal `cli.connection_not_found` (exit 10); entri yang tidak tersedia gagal `cli.connection_unavailable` (exit 3); provider tanpa blok `connection` gagal `cli.connection_not_found` dengan `available` kosong; paket pinned yang tidak terbaca (`provider.packageReadable` false) gagal `cli.connection_unavailable` dengan `reason` `package_unavailable` (exit 3) di setiap bentuk *(ditambah 2026-10-22)*. `--all` menulis satu baris per entri dan tidak bisa digabung dengan `--format` atau `--show-password` (penolakan clap, exit 2). `--show-password` memanggil `secret.reveal { reference, purpose: show }` sekali per reference berbeda dengan batas waktu `Interactive` spec 0008, menyusun string lengkap lewat `compose`, dan hanya itu yang mencetak password; untuk entri tanpa secret, ia mencetak string yang sama tanpa reveal. `--json` memberi `result` `{ "instance": { "id", "name" }, "connection": ConnectionString, "value": string | null }`, atau `{ "instance", "connections": [ConnectionString] }` untuk `--all`, dengan `value` terisi hanya dengan `--show-password`. Setiap reveal tercatat audit `secret.reveal` spec 0011.
- **AC-7** (paritas inspect dan Summary): `stackly instance inspect` (spec 0022) juga memanggil `instance.get_detail` dan `instance.get_data_usage` dengan `cachedOnly: true` (`--size` memakai `refresh: true`), menambah baris di *CLI*, dan dengan `--json` memberi `result` `{ "instance": InstanceSummary, "detail": InstanceDetail, "dataUsage": DataUsage | null }` (usulan spec 0022). Kalau `detail.revision` berbeda dengan `revision` summary, perintah mengambil ulang keduanya sekali. Golden CLI dan test view model Swift membaca fixture triple (summary, detail, data usage) `protocols/fixtures/instance/detail-postgresql.json`, `detail-nats.json`, `detail-unavailable.json`, dan `detail-package-unreadable.json` (paket pinned tidak terbaca, ditambah 2026-10-22) dengan `TZ=UTC` dan jam tetap, dan menghasilkan teks yang sama untuk setiap baris yang ditambah spec ini di tabel *Format bersama* (Digest, Data, Data size, Autostart, Dependents, Secrets, Connection, dan alasan tidak tersedia); baris yang sudah ada di spec 0022 tetap memakai teks spec 0022 di masing masing permukaan.
- **AC-8** (AC-012, REQ-012, BR-010): instance fake yang prosesnya `running` tetapi probe readiness nya belum berhasil (setting `readyDelayMs` di atas durasi test) tampil di header halaman detail dengan label `StatusLabel` spec 0012 ("Starting…" dengan "Waiting for probe", atau "Not Ready"), dan di kartu Endpoints sebagai dua baris terpisah: Process "Running (PID {pid})" dan Readiness "Waiting for probe" atau "Not ready" dengan detail probe, tidak pernah "Ready"; `stackly instance inspect` menulis `Lifecycle running` dan `Readiness not_ready (<reason>)`. Setelah probe berhasil, header menjadi "Running" dan baris Readiness "Ready" tanpa memuat ulang halaman. Test GUI (view model dengan fixture urutan event) dan test CLI terhadap daemon nyata memasang guard evidence `covers = ["AC-012", "REQ-012", "BR-010"]`.
- **AC-9** (start dan stop dari halaman detail): toolbar halaman detail memakai aksi utama `InstanceActionModel` spec 0022 (Start, Stop, Retry, tombol sibuk, atau nonaktif dengan alasan) dengan simbol dan label; Start mengirim `instance.start`, status berjalan dari "Starting…" sampai "Running" dengan Readiness "Ready" lewat event spec 0010, dan connection yang tadinya `port_not_assigned` menjadi tersedia setelah detail dimuat ulang. Perintah setara CLI tetap `stackly instance start <instance> --wait` dan `stackly instance stop <instance> --wait` (fitur 14).
- **AC-10** (offline, REQ-046, AC-046): test integration menjalankan `stacklyd::server::start` di dalam proses dengan catalog `NotConfigured`, `Fetcher` tiruan dan `CodeSigning` tiruan yang menggagalkan test pada panggilan apa pun, dan penghitung akses `CatalogState` di `stacklyd`, lalu memakai binary `stackly` asli ke socket server itu; urutan `instance.start` sampai `ready`, `instance.get`, `instance.get_detail`, `instance.get_data_usage`, `stackly instance connection`, dan `stackly instance inspect` untuk instance fake dengan runtime dan paket pinned yang sudah ada berhasil, dengan nol akses `CatalogState`, nol panggilan `Fetcher`, dan nol panggilan `CodeSigning::assess`. Bukti native dengan jaringan Mac ini dimatikan (Wi-Fi dan Ethernet) untuk PostgreSQL lewat GUI dan CLI dicatat sebagai evidence `native` setelah persetujuan user, dan `BLOCKED` sampai disetujui.
- **AC-11** (navigasi halaman detail): `ShellDestination.instance(UUID)` (usulan spec 0022) membuka halaman detail lewat `instanceRequest` yang diterapkan `InstancesPage` ke `NavigationStack` nya, juga saat halaman Instances belum tampil, dan `ShellNavigator.openInstance(_:)` memakainya, menggantikan perilaku `// provisional: F21` spec 0022. Klik ganda atau Return pada baris Instances, tombol Open di inspector Instances, dan baris instance di Overview membuka halaman ini. Tombol kembali sistem kembali ke Instances dengan pilihan baris tetap, dan klik ulang Instances di sidebar saat halaman detail terbuka kembali ke tabel. Instance yang tidak ada di `InstanceStore` dan tidak ditemukan `instance.get` menampilkan "Instance Not Found" dengan Back to Instances.
- **AC-12** (header dan toolbar): halaman memakai `.pageChrome(title: name, subtitle: "{nama tampil provider} {versi} · {label status}")`; toolbar berisi tombol aksi utama dan menu More dengan "Copy Connection String" (entri pertama; nonaktif dengan alasan kalau tidak ada entri atau entri itu tidak tersedia). Tooltip tombol aksi utama memakai `CommandMap.helpText(for:title:)` dengan `startInstance` atau `stopInstance` (misalnya "Retry (⌘R)"), menu More memakai help bawaan "More", item More tidak memasang shortcut, dan halaman Instances menerbitkan `back` (⌘[, hanya saat detail terbuka) dan `copyConnectionString` (action yang sama dengan item More) dengan target detail mengalahkan pilihan tabel (spec 0069 AC-11, AC-12). Restart, Reload, Duplicate, dan Remove tidak tampil sampai fitur 26, 24, dan 54 membangunnya. Picker tab hanya menampilkan tab yang sudah dibangun dan tersembunyi selama Summary satu satunya tab.
- **AC-13** (tab Summary, PAGE-05): tab Summary mengikuti mockup dan *Tab Summary*: kartu Endpoints (satu baris per endpoint dengan alamat atau keterangan, tombol Copy hanya untuk alamat yang diketahui, lalu baris Process dan Readiness terpisah), kartu Connection (setiap entri dengan string tersamar, Show Password dan Copy untuk entri ber secret, Copy saja untuk entri tanpa secret, keterangan alasan untuk entri tidak tersedia, catatan `portMayChange`, caution "Not accepting connections yet." selama `running` tanpa readiness `ready`, lalu satu baris Secret per reference dengan statusnya), kartu Dependents, dan kartu Details (Instance ID, Runtime, Version, Digest, Provider, Data, Data size, Uptime, Restart attempts kalau lebih dari 0, Autostart). Copy endpoint menulis teks alamat ke pasteboard biasa hanya saat tombolnya ditekan; Show Password memanggil `secret.reveal { purpose: show }` untuk setiap reference entri dan menampilkan string tersusun sampai aturan sembunyi spec 0011 berlaku; Copy memanggil `secret.reveal { purpose: copy }`, menyusun lewat `ConnectionComposer`, dan menulis lewat `ConcealedPasteboard`. Show Password dan Copy nonaktif dengan alasan saat salah satu reference entri berstatus `missing`. Tanpa aksi pengguna, tidak ada reveal dan tidak ada tulisan pasteboard.
- **AC-14** (banner dan state): Summary menampilkan banner masalah dari aturan I1 sampai I4 spec 0022 (failed dengan cuplikan `lastFailure.excerpt` dan Retry, `unknown`, blocker), dan blocker `needsSecret` menawarkan "Enter Secret…" yang membuka `SecretEntrySheet` spec 0011 dengan item blocker itu. Detail yang sedang dimuat menampilkan `DelayedProgress` di baris yang bergantung pada detail sementara baris dari `InstanceSummary` sudah tampil; detail yang gagal dimuat menampilkan `StateBanner` caution "Couldn’t load instance details." dengan Retry tanpa mengosongkan halaman; daemon yang tidak terjangkau mempertahankan data terakhir dengan banner stale shell dan aksi tergate. Detail dimuat ulang saat `revision` summary instance itu berubah (jeda `DETAIL_RELOAD_DEBOUNCE`) dan saat event `secret.changed` menyebut salah satu reference nya, dan balasan dengan `revision` yang lebih lama dari detail yang dipegang dibuang.
- **AC-15** (inspector Instances): inspector spec 0022 mendapat tombol Open di samping aksi utama, baris Data (ukuran dari `instance.get_data_usage` dengan `cachedOnly: true` atau "—", path di baris kedua), dan baris Connection (entri pertama tersamar dengan Show Password dan Copy lewat komponen yang sama), dan placeholder "No Selection" berpesan "Select an instance to see its details. Double-click a row to open it.". Tombol Open adalah action `openInstance` yang juga diterbitkan halaman Instances ke menu Instance (⌘O), dan Copy di baris Connection adalah action `copyConnectionString` yang sama dengan menu saat detail belum terbuka (spec 0069 AC-11).
- **AC-16** (AC-027, NFR-012): Dihapus dari cakupan pada 2026-10-24.
- **AC-17** (kontrak): method `instance.get_detail` dan `instance.get_data_usage` (keduanya `recovery refused`), DTO di *Method IPC*, enum terbuka baru (diakhiri `Unrecognized`), kode `instance.data_usage_unavailable` (hint statis lewat `codes::instance::hint`), dan kode `cli.connection_not_found` serta `cli.connection_unavailable` ada di `stackly-protocol` dengan fixture `protocols/fixtures/` untuk setiap varian `Call` dan `Reply`, setiap kode, nilai asing setiap enum terbuka (termasuk `ConnectionPartKind` dan `ConnectionState` asing), terbaca di Rust dan Swift, dengan kenaikan minor `ProtocolVersion::CURRENT` di commit yang sama dan `cargo xtask codegen --check` bersih. Test kontrak memeriksa konsistensi `ConnectionString`: `available` jika dan hanya jika `masked` terisi, `parts` tidak kosong, dan `reason` kosong.
- **AC-18** (keamanan, AC-016): suite sentinel memakai fixture credential sebagai password instance fake dan memastikan prefix nya tidak ada di balasan `instance.get_detail`, event, log daemon, output default dan `--json` `stackly instance connection` dan `inspect` tanpa `--show-password`, dan state model `@Observable` yang hidup lama di Swift (`InstanceDetailModel`, `InstanceStore`); nilai hanya muncul di respons `secret.reveal`, di stdout `--show-password`, di `RevealableSecretText` yang sedang ditampilkan, dan di `ConcealedPasteboard` setelah Copy.
- **AC-19** (kinerja): dengan 10 instance terdaftar di Mac ini, `instance.get_detail` dijawab dalam 100 ms pada persentil 95 saat cache paket sudah hangat dan dalam 300 ms saat dingin, dan `instance.get_data_usage` yang memulai scan selesai dalam `DATA_USAGE_DEADLINE` ditambah 100 ms.
- **AC-20** (PostgreSQL dan NATS nyata): di Mac ini, instance PostgreSQL 18 dan NATS yang dibuat lewat fitur 20 di start dari GUI dan dari CLI sampai `ready`; string `uri` PostgreSQL yang disusun dengan password hasil `secret.reveal` berhasil menjalankan `SELECT 1` lewat `psql` milik runtime (TCP), string `socket` juga berhasil, dan string `uri` NATS menerima baris `INFO` server; tab Summary dan `stackly instance connection` menunjukkan string tersamar yang sama. Tanpa artefak di cache runtime xtask hasilnya `BLOCKED` dengan alasan dan owner.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md), ditambah semua perbaikan cross check yang disetujui lead atas nama user (2026-10-20): connection string dideklarasikan di blok `connection` manifest sebagai template URI, divalidasi dengan render sentinel saat paket dimuat, dan dirender daemon lewat `PlanContext` dan resolver interpolasi plan; string lengkap disusun client dari bagian tersamar dan nilai `secret.reveal`, dengan satu fixture komposisi untuk core, CLI, dan Swift; data halaman detail dari method baru `instance.get_detail`, dan ukuran folder data dari `instance.get_data_usage` yang hanya scan saat diminta, dengan cache dan batas scan bersamaan; header memakai label status spec 0012 dan Summary menulis Process dan Readiness sebagai dua baris; semua entri connection ditampilkan; `stackly instance connection` memilih entri di client dari detail; `stackly instance inspect --json` dibungkus `{ instance, detail, dataUsage }`; tab yang belum dibangun tidak ditampilkan; dan bukti offline lewat test di dalam proses ditambah run native dengan jaringan mati setelah persetujuan user.

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
| Manifest, interpolasi, `PlanContext`, matriks posisi, kode issue, `provider check` | spec 0009 (blok `connection` lewat usulan) |
| `Filesystem`, `RootSet::resolve`, `instance_run_dir`, `check_private_dir` | spec 0004 (`tree_usage` lewat usulan yang diterapkan 2026-10-20; aturan penelusuran 2026-10-22) |
| `secret.reveal`, `RevealableSecretText`, `ConcealedPasteboard`, `SecretEntrySheet`, status reference, audit | spec 0011 (`RevealableSecretText` untuk banyak reference lewat usulan) |
| Label status (`StatusLabel`, `StatusAppearance.instance`) | spec 0012 (dipakai apa adanya) |
| Halaman Instances, inspector, `InstanceActionModel`, `ShellNavigator`, aturan I1 sampai I4, `UptimeText`, resolver CLI, `stackly instance list` dan `inspect` | spec 0022 (destinasi `.instance`, `instanceRequest`, `NavigationStack`, inspector, dan bentuk JSON inspect lewat usulan) |
| Paket PostgreSQL dan NATS | spec 0019 dan 0023 (blok `connection` dan urutan evidence lewat usulan) |
| `stackly instance start` dan `stop` dengan `--wait` | fitur 14 (spec 0008 dan 0010) |
| Create, `instance.removed` | spec 0020 |
| Tab Configuration, Logs, Metrics, Backups, Activity; Restart dan Reload; Duplicate; Remove | fitur 29, 27, 28, 49, 35; 26; 24 (item More "Duplicate…" sesudah Copy Connection String, [spec 0024](../0024-instance-kedua-runtime-sama/index.md) AC-13); 54 |
| Banner crash loop yang lebih kaya | fitur 33 (spec ini memakai aturan I1 spec 0022 dengan Retry) |
| Jumlah restart sejak dibuat dan latensi probe | fitur 28 |
| Dependents dari environment, dan pemicu muat ulang detail untuk perubahan config dan environment | fitur 29 dan 44 sampai 47 (spec ini sudah membaca registry, yang kosong di fase 02) |
| Komponen dasar dan peta perintah | fitur 67 dan 69 |

### Alur

1. Pengguna membuka halaman detail (klik ganda baris, Open di inspector, atau Overview).
2. `InstanceDetailModel` memakai `InstanceSummary` dari `InstanceStore` untuk header, endpoint, proses, dan readiness, lalu memanggil `instance.get_detail` dan `instance.get_data_usage`.
3. Start lewat `InstanceActionModel` mengirim `instance.start`; event `instance.state_changed` dan `instance.health_changed` spec 0010 memperbarui status dan readiness, dan perubahan `revision` memicu muat ulang detail sehingga connection yang menunggu port menjadi tersedia.
4. Copy pada connection ber secret memanggil `secret.reveal { purpose: copy }` untuk setiap reference nya, menyusun string lewat `ConnectionComposer`, dan menulis `ConcealedPasteboard`.
5. CLI: `stackly instance start <instance> --wait`, lalu `stackly instance connection <instance>` (tersamar) atau dengan `--show-password`.

### Blok connection di manifest

Field baru di manifest `stackly/v1alpha1` (usulan spec 0009), opsional:

```yaml
connection:
  - name: uri                      # ^[a-z][a-z0-9_]{0,31}$, unik; entri pertama adalah entri utama
    title: Connection string       # 1 sampai 64 karakter, tanpa karakter kontrol; label di GUI
    template: "postgresql://postgres:${secret.superuserPassword}@${endpoint.client.address}/postgres"
```

- 1 sampai 8 entri. Paket tanpa blok ini valid; GUI menulis "This provider doesn’t describe a connection string. Use the endpoints above." dan CLI gagal `cli.connection_not_found`.
- `template` 1 sampai 512 byte dengan grammar interpolasi spec 0009, dan hasil render nya selalu URI. Token yang diterima di posisi baru `connection.template`: `endpoint.<n>.address`, `endpoint.<n>.port`, `endpoint.<n>.socket`, `endpoint.<n>.socketDir`, `instance.name`, `instance.id`, `secret.<field>` (field secret), `provider.id`, `provider.revision`, dan `runtime.version`. `endpoint.<n>.host` dan `settings.*` ditolak.
- Aturan encode di AC-2. Teks literal template tidak pernah di encode; penulis paket menulis teks yang sudah sah untuk URI, dan render sentinel saat paket dimuat menolak template yang hasilnya bukan URI absolut.

### Connection PostgreSQL, NATS, dan fake

PostgreSQL (`providers/postgresql/provider.yaml`, usulan spec 0019):

```yaml
connection:
  - name: uri
    title: Connection string
    template: "postgresql://postgres:${secret.superuserPassword}@${endpoint.client.address}/postgres"
  - name: socket
    title: Unix socket
    template: "postgresql://postgres:${secret.superuserPassword}@/postgres?host=${endpoint.socket.socketDir}&port=${endpoint.client.port}"
```

NATS (`providers/nats/provider.yaml`, usulan spec 0023), tanpa credential sampai fitur 39:

```yaml
connection:
  - name: uri
    title: Client URL
    template: "nats://${endpoint.client.address}"
  - name: monitoring
    title: Monitoring URL
    template: "http://${endpoint.monitoring.address}"
```

- Fake provider (`testing/fake`) mendapat `uri` `http://stackly:${secret.authToken}@${endpoint.http.address}/` untuk test komposisi dan sentinel.
- Menambah blok ini mengubah `manifest_sha256`, jadi paket PostgreSQL dan NATS menulisnya sebelum evidence BR-025 nya dibuat (usulan prasyarat spec 0019 dan 0023) dan catalog menaikkan revision di commit yang sama (aturan spec 0017). Instance yang di pin ke paket lama tetap tanpa connection sampai update provider revision fitur 59.

### Render di daemon

Modul baru `stackly-core::instance::connection`:

```rust
pub fn render(package: &ValidatedPackage, ctx: &PlanContext, instance: &Instance,
              secret_refs: &[(SecretFieldName, SecretRef)]) -> Vec<RenderedConnection>;
pub struct RenderedConnection { pub name: String, pub title: String, pub state: ConnectionState, pub port_may_change: bool }
pub enum ConnectionState { Available { masked: String, parts: Vec<ConnectionPart> }, Unavailable(ConnectionUnavailable) }
pub enum ConnectionPart { Text(String), Secret { field: SecretFieldName, reference: SecretRef } }
pub enum ConnectionUnavailable { Retained, PackageUnavailable, PortNotAssigned }
```

- `PlanContext` (spec 0009) dibangun `stacklyd` dengan cara yang sama dengan dry run plan start: `instance_paths(id)`, folder run dari `instance_run_dir(short_key)` (fungsi murni, tanpa membuat folder), runtime dari record runtime, dan setiap endpoint dengan port efektif spec 0004 (`bound` kalau ada, selain itu port `Fixed`, selain itu tanpa port). Settings tidak dibutuhkan karena template tidak boleh merujuk settings, jadi config revision tidak dibaca.
- Token yang tidak bisa diselesaikan karena endpoint tanpa port efektif membuat entri `PortNotAssigned`. Host, port, dan socket dihitung resolver yang sama dengan plan, jadi selalu sama dengan yang di bind proses.
- `secret_refs` dari `secret_uses_by(Instance(id))` (spec 0005); render tidak pernah memanggil `SecretStore`.
- `ConnectionPart::Text` berisi teks yang sudah di encode; dua teks berurutan digabung.
- Paket pinned yang tidak terbaca (atau yang di cache tetapi `key()` nya bukan provider revision instance) tidak punya entri, jadi `detail::build` tidak memanggil `render` dan mengisi `DetailProvider.package_readable` false (*2026-10-22*). `ConnectionUnavailable::PackageUnavailable` per entri hanya untuk paket yang terbaca: paket yang bukan paket pinned saat `render` dipanggil langsung, atau kegagalan render selain `EndpointUnresolved` (`RenderFailure::of` memetakan setiap `EndpointUnresolved` ke `PortNotAssigned`, termasuk endpoint manifest yang tidak ada di context).

### Method IPC

Semua method lewat handshake dan peer user yang sama (spec 0006).

| Method | Recovery | Input | Output | Error utama |
| --- | --- | --- | --- | --- |
| `instance.get_detail` | refused | `GetInstanceDetailParams { instanceId: Uuid }` | `InstanceDetail` | `resource.not_found` |
| `instance.get_data_usage` | refused | `GetDataUsageParams { instanceId: Uuid, refresh: bool, cachedOnly: bool }` | `DataUsageReply` | `resource.not_found`, `instance.data_usage_unavailable` |

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
    pub secrets: Vec<DetailSecret>,                 // urut field
    pub dependents: Vec<DetailDependent>,           // urut nama, lalu ID
    pub connections: Vec<ConnectionString>,         // urut manifest
}
pub struct DetailProvider { pub name: String, pub revision: String, pub manifest_sha256: String,
                            #[serde(default)] pub display_name: Option<String>,
                            pub package_readable: bool }      // false: paket pinned tidak terbaca; maka display_name null dan connections kosong (2026-10-22)
pub struct DetailRuntime { pub id: Uuid, pub version: String, pub target: HostTarget,         // HostTarget spec 0017
                           pub install_mode: InstallModeKind, #[serde(default)] pub artifact_sha256: Option<String> }
pub enum StartupPolicyKind { Manual, AtLogin, Unrecognized }
pub struct DetailSecret { pub field: String, pub title: String, pub reference: String, pub status: SecretRefStatus }
pub struct DetailDependent { pub environment_id: Uuid, pub name: String, pub ownership: DependentOwnership, pub lease_active: bool }
pub enum DependentOwnership { Dedicated, Shared, Unrecognized }
pub struct ConnectionString {
    pub name: String, pub title: String, pub state: ConnectionState,
    #[serde(default)] pub reason: Option<ConnectionUnavailableReason>,   // terisi jika dan hanya jika unavailable
    #[serde(default)] pub masked: Option<String>,                        // terisi jika dan hanya jika available
    pub parts: Vec<ConnectionPart>,                                      // tidak kosong jika dan hanya jika available
    pub port_may_change: bool,
}
pub enum ConnectionState { Available, Unavailable, Unrecognized }
pub enum ConnectionUnavailableReason { PortNotAssigned, PackageUnavailable, Retained, Unrecognized }
pub struct ConnectionPart { pub kind: ConnectionPartKind, #[serde(default)] pub text: Option<String>,
                            #[serde(default)] pub secret: Option<ConnectionSecretPart> }
pub enum ConnectionPartKind { Text, Secret, Unrecognized }
pub struct ConnectionSecretPart { pub field: String, pub reference: String }
pub struct GetDataUsageParams { pub instance_id: Uuid, #[serde(default)] pub refresh: bool, #[serde(default)] pub cached_only: bool }
pub struct DataUsageReply { #[serde(default)] pub usage: Option<DataUsage> }
pub struct DataUsage { pub bytes: Count, pub complete: bool, pub scanned_at: Timestamp }
```

- `SecretRefStatus` adalah enum status reference terbuka milik spec 0011 (`present`, `missing`, `unknown`, `unrecognized`); kalau build spec 0011 memberi nama lain, nama itu yang dipakai. `InstallModeKind` dari spec 0018 dan `HostTarget` dari spec 0017.
- `Validate` untuk `GetDataUsageParams` menolak `refresh` dan `cachedOnly` yang sama sama true (`request.invalid_params`, field `cachedOnly`, kode `invalid_value`; *Ukuran data*, 2026-10-22).
- Client memperlakukan `ConnectionState::Unrecognized` dan entri yang memuat `ConnectionPartKind::Unrecognized` sebagai tidak tersedia dengan teks "Update Stackly to copy this connection string.".
- `stacklyd` menyimpan cache paket pinned per `ProviderRevisionKey` (LRU `PINNED_PACKAGE_CACHE` entri), dipakai bersama `socketPath` spec 0020, dengan pemeriksaan ulang stat `provider.yaml` setiap request (AC-4).

### Ukuran data

- `Filesystem::tree_usage(root, UsageLimits { max_entries: u64, deadline: Duration }) -> Result<TreeUsage { bytes, complete }, PlatformError>` (spec 0004): penelusuran iteratif berbasis file descriptor di adapter macOS (`openat` dengan `O_NOFOLLOW`, `fstatat` dengan `AT_SYMLINK_NOFOLLOW`), `bytes` jumlah `st_blocks * 512` setiap entri termasuk folder, hard link dihitung sekali per (device, inode), entri di device lain dilewati, symlink dihitung sebagai dirinya sendiri, dan berhenti dengan `complete: false` di batas entri, saat tenggat (diukur dengan jam monotonik sejak panggilan) lewat, atau di folder yang lebih dalam dari `TREE_USAGE_MAX_DEPTH`. `FakeFilesystem` memakai penelusuran portabel dengan aturan yang sama; test tenggat nyata berjalan di adapter macOS, test batas entri, kedalaman, dan tenggat nol di keduanya. Aturan rinci penelusuran (diputuskan 2026-10-22, sama persis dengan *Port platform* spec 0004):
  - **Root.** `root` harus path absolut tanpa komponen `.` atau `..`, tanpa byte NUL, dan tanpa garis miring di akhir (yang membuat `O_NOFOLLOW` mengikuti symlink); selain itu `Io` kind `InvalidInput` dengan op `tree_usage`. `root` dibuka seperti `remove_tree`, tanpa mengikuti symlink di komponen terakhir: symlink gagal `SymlinkInPath`, bukan folder `NotADirectory`, tidak ada `NotFound`. Blok root sendiri masuk `bytes`, tetapi root bukan entri untuk `max_entries`.
  - **Entri.** Setiap nama yang dibaca dari isi sebuah folder, kecuali `.` dan `..`, adalah satu entri untuk `max_entries`, apa pun hasil stat nya (file, folder, symlink, entri di device lain, entri yang sudah hilang, atau hard link kedua). Membaca nama ke `max_entries + 1` menghentikan penelusuran dengan `complete: false`, jadi pohon dengan tepat `max_entries` entri lengkap, dan root kosong lengkap juga dengan `max_entries` 0. Nama dibaca satu per satu dari stream folder, tidak pernah seluruh isi folder sekaligus, jadi memori dan tenggat tetap terjaga untuk satu folder yang sangat besar.
  - **Kedalaman.** `root` adalah tingkat 1, dan paling banyak `TREE_USAGE_MAX_DEPTH` (32) folder terbuka sekaligus, satu file descriptor per tingkat: stream folder (`fdopendir`) membungkus descriptor yang sama yang dipakai `openat` dan `fstatat` (lewat `dirfd`), `.` tidak pernah dibuka ulang, dan setiap folder dibuka dengan flag folder `remove_tree` (`O_DIRECTORY`, `O_NOFOLLOW`, `O_NONBLOCK`, `O_CLOEXEC`). Folder di tingkat 33 di-stat dan bloknya dihitung seperti entri lain, tetapi tidak dibuka; isinya tidak dihitung dan hasilnya `complete: false`.
  - **Hard link.** Hanya entri bukan folder dengan `st_nlink > 1` yang dicatat per `(st_dev, st_ino)`; kemunculan berikutnya tetap entri, tetapi tidak menambah `bytes`.
  - **Tenggat.** Batasnya `Instant::now().checked_add(deadline)` saat panggilan mulai (`None` berarti tanpa tenggat), dicek sekali untuk setiap nama yang dikembalikan stream, sesudah nama itu dihitung dan sebelum stat nya: kalau `Instant::now()` sudah sama dengan atau melewati batas, penelusuran berhenti dengan `complete: false`. Akhir stream tidak pernah dicek, jadi pohon yang selesai ditelusuri selalu lengkap: tenggat nol memberi root kosong `complete: true` dan root berisi `complete: false` dengan `bytes` hanya blok root. Di `FakeFilesystem`, jeda `set_delay` terjadi sebelum tenggat mulai dihitung.
  - **Error di tengah jalan.** `ENOENT` di mana pun di bawah root (stat, open, atau membaca isi folder) berarti entri itu hilang, dan ia dilewati diam diam, jadi `NotFound` hanya berasal dari root. `EINTR` diulang. Folder yang berubah jenis antara stat dan open (open memberi `ELOOP` atau `ENOTDIR`) atau yang setelah dibuka ada di device lain tidak dimasuki, blok dari stat nya tetap dihitung, dan `complete` tidak berubah karenanya. Errno lain menghentikan penelusuran dengan `Err` lewat `PlatformError::from_io` dengan op `tree_usage`, tanpa hasil parsial.
  - **Urutan dan perubahan.** Urutan penelusuran tidak ditentukan, jadi `bytes` hasil yang tidak lengkap bisa berbeda antara adapter; test hasil tidak lengkap hanya memeriksa `complete: false` dan `bytes` yang tidak melebihi hasil lengkap pohon yang sama. Selama folder berubah (database yang jalan), hasilnya perkiraan: `readdir` bisa melewatkan nama atau mengembalikannya dua kali.
  - **Test.** `FakeFilesystem` menghitung panggilan `tree_usage` dan jumlah panggilan bersamaan tertinggi (`tree_usage_calls()`, `tree_usage_peak()`) untuk test penggabungan dan semaphore daemon. Kasus platform (tepat batas dan satu lebih, root kosong dengan batas 0, 32 dan 33 tingkat, tenggat nol, root relatif, symlink, file, atau tidak ada) ada di `fs_conformance`; tenggat nyata diuji di adapter macOS dengan 50 000 file: tenggat 1 ms memberi `complete: false` dalam 100 ms, dan tenggat 60 detik `complete: true`.
- `stacklyd` (langkah 7) menjawab dengan urutan ini *(2026-10-22)*:
  1. Instance tidak ada di registry: `resource.not_found`.
  2. Tanpa `refresh`, entri cache yang masih segar dibalas apa adanya, dengan `scannedAt` yang tersimpan, tanpa menyentuh filesystem. Entri segar jika dan hanya jika `scannedAt <= Clock::now() < scannedAt + ttl` (`DATA_USAGE_CACHE_TTL` untuk hasil lengkap, `DATA_USAGE_PARTIAL_TTL` untuk tidak lengkap), lewat port `Clock` supaya `FakeClock` menggerakkan test; entri kedaluwarsa sama dengan tidak ada.
  3. `cachedOnly: true` tanpa entri segar: `usage: null`.
  4. Scan yang sedang berjalan untuk instance itu diikuti, juga oleh `refresh: true`, dan semua yang mengikutinya menerima hasil atau error yang sama.
  5. Selain itu daemon memulai scan di task yang terlepas dari koneksi (client yang putus tidak membatalkannya): cek folder data lewat `symlink_metadata` dan `check_private_dir` spec 0004 dengan user dari `UserIdentity` (tidak ada: reason `missing`; setiap `PrivateDirViolation`: `not_private`; error `symlink_metadata`: `io`), lalu permit semaphore `DATA_USAGE_CONCURRENCY`, lalu `tree_usage` di `spawn_blocking`. `scannedAt` adalah `Clock::now()` saat scan selesai. Hasil disimpan di cache per `InstanceId` di memori daemon; error tidak pernah disimpan.

  Error `tree_usage` menjadi `instance.data_usage_unavailable`: `NotFound` reason `missing`; `SymlinkInPath`, `NotADirectory` (folder diganti antara cek dan scan), dan `PermissionDenied` (folder atau isinya tanpa izin baca) reason `not_private`; selain itu reason `io`. `refresh` dan `cachedOnly` yang sama sama true ditolak `Validate` dengan `request.invalid_params` (field `cachedOnly`, kode `invalid_value`). Tenggat dihitung sejak `tree_usage` mulai, jadi waktu menunggu permit tidak termasuk; batas AC-19 diukur saat permit tersedia. `DaemonConfig` membawa batas entri, tenggat, kedua TTL, dan jumlah scan bersamaan (default dari *Konstanta*), supaya test bisa memperkecilnya.
- Scan hanya dimulai oleh halaman detail yang terbuka, tombol Recalculate, dan `stackly instance inspect --size`; inspector dan `inspect` tanpa `--size` memakai `cachedOnly: true`.

### Tab Summary

Folder baru `StacklyUI/InstanceDetail/`: `InstanceDetailPage`, `InstanceDetailModel`, `SummaryTab`, `EndpointsCard`, `ConnectionCard`, `DependentsCard`, `DetailsCard`, `InstanceDetailBanner`, `ConnectionComposer`, `DetailFormat`, `ProcessText`, `ReadinessText`, dan `InstanceTab`. `StacklyClient` mendapat `getInstanceDetail` dan `getDataUsage` dari tipe hasil generate.

- **Latar dan tata letak**: `surfaceWindow`, `ScrollView`, setiap kartu sebuah `CardSection` [spec 0067](../0067-komponen-dasar-stackly/index.md) (judul kartu sebagai judul seksi, permukaan `.cardSurface()`), dan baris label dan nilai sebagai `LabeledContent` dengan `.labeledContentStyle(.stackly)` (diperbarui 2026-10-21). Saat lebar konten minimal 760 pt, dua kolom (kiri Endpoints, Connection, Dependents; kanan Details); di bawahnya satu kolom dengan urutan yang sama.
- **Banner**: di atas kartu, aturan I1 sampai I4 spec 0022 dengan copy `InstanceFailureCopy` spec 0022; I1 membawa blok cuplikan `lastFailure.excerpt` dan tombol Retry (aksi utama); blocker `needsSecret` menampilkan "Needs a secret to start" dengan reference yang bermasalah dan tombol "Enter Secret…" yang membuka `SecretEntrySheet`.
- **Endpoints**: judul "Endpoints"; per endpoint DTO, nama di kiri dan nilai monospace di kanan menurut *Format bersama*; tombol Copy hanya saat nilai adalah alamat atau path yang diketahui: `CopyButton(address, what: "{nama endpoint} address", style: .icon, announcement: "Address copied.", key: "endpoint.{nama endpoint}")` spec 0067 di samping `LabeledContent`, di luar nya (`HStack { LabeledContent(...); CopyButton(...) }`). Lalu dua baris terpisah: "Process" dengan `ProcessText` dan "Readiness" dengan `ReadinessText`.
- **Connection**: judul "Connection". Kalau `detail.provider.packageReadable` false, kartu hanya berisi teks *Copy* `package_unavailable` ("Stackly can’t read this provider’s package. Run stackly doctor.") di tempat entri, bukan teks provider tanpa blok connection, lalu baris Secret seperti biasa (*2026-10-22*). Teks itu datang dari satu fungsi murni `DetailFormat.connectionUnavailableText(_ detail: InstanceDetail) -> String?` (teks *Copy* `package_unavailable` saat `packageReadable` false, selain itu nil), yang juga dipakai alasan item More Copy Connection String, baris Connection inspector (AC-15), dan alasan nonaktif action `copyConnectionString` (spec 0069), supaya keempat permukaan tidak pernah menyebut sebab yang berbeda. Selain itu, per `ConnectionString`: `title` sebagai label kecil, lalu
  - tersedia dengan bagian secret: `RevealableSecretText(references:compose:)` (usulan spec 0011) dengan nilai tersamar, Show Password atau Hide Password, dan Copy, memakai `ConnectionComposer.compose`; nonaktif dengan alasan "{reference} has no stored value yet." saat salah satu reference `missing`;
  - tersedia tanpa bagian secret: teks monospace bisa dipilih dan `CopyButton(string, what: title, announcement: "{title} copied.", key: "connection.{name}")` spec 0067 (pasteboard biasa lewat `PlainPasteboard`);
  - tidak tersedia: teks alasan dari *Copy*;
  - `portMayChange`: catatan "The port can change the next time the instance starts.";
  - lifecycle `running` dengan readiness bukan `ready`: caution "Not accepting connections yet.".
  Lalu satu baris "Secret" per `DetailSecret`: reference monospace, dan untuk status `missing` atau `unknown` keterangan "Missing; starting asks for it." atau "Status unknown".
- **Dependents**: judul "Dependents"; "Nothing depends on this instance." kalau kosong, selain itu satu baris per environment "{name} · {Dedicated | Shared}" ditambah " · lease active".
- **Details**: judul "Details"; `LabeledContent` dengan nilai `textSecondary` bisa dipilih, baris dan format di *Format bersama*: Instance ID (dengan `CopyButton(id, what: "instance ID", announcement: "ID copied.", key: "instanceId")` di luar `LabeledContent`), Runtime, Version, Digest, Provider, Data, Data size (dengan tombol `arrow.clockwise` berlabel "Recalculate data size" yang memanggil `refresh: true`, dan keterangan "scanned {RelativeAge}"), Uptime, Restart attempts (hanya kalau lebih dari 0 atau `exhausted`), Autostart.
- `InstanceDetailModel` (`@Observable`, main actor) memegang detail, data usage, state muat, dan error, tanpa nilai secret; memuat detail saat halaman tampil, saat `revision` summary berubah (jeda `DETAIL_RELOAD_DEBOUNCE`), dan saat event `secret.changed` yang diterima lewat `ShellSync.events(kindPrefix: "secret.")` (usulan spec 0013, dibangun langkah 9) menyebut salah satu reference nya, dan membuang balasan yang `revision` nya lebih lama dari detail yang dipegang; data usage dimuat saat halaman tampil dan saat Recalculate. Uptime diperbarui lewat `TimelineView(.periodic(from:by: 1))` hanya di baris itu. Uptime dan "scanned {RelativeAge}" memakai `fixtureNow ?? context.date` (aturan kunci environment spec 0068, 2026-10-21).

### Format bersama

`ProcessText` dan `ReadinessText` adalah fungsi murni atas `InstanceSummary` (usulan spec 0022: inspector memakainya juga). Baris yang ditambah spec ini diuji paritas GUI dan CLI (AC-7).

| Baris | Sumber | Teks |
| --- | --- | --- |
| Endpoint IP | `InstanceSummary.endpoints[]` | "{host}:{port}" (IPv6 "[{host}]:{port}"), atau "{host}, port assigned at start" kalau `port` null |
| Endpoint Unix | sama | `socketPath`, atau "socket, created at start" kalau null |
| Process | `lifecycle`, `process` | `running` "Running (PID {pid})" ditambah ", adopted" kalau `adopted`; `recovering` "Restarting"; `starting` "Starting"; `stopping` "Stopping"; `stopped` "Stopped"; `failed` "Failed"; `unknown` "Unknown"; `initializing` "Initializing"; `uninitialized` "Not initialized" |
| Readiness | `readiness`, `lifecycle` | hanya saat `running` atau `recovering`: `ready` "Ready"; `not_ready` reason `awaiting_probe` "Waiting for probe"; `not_ready` lain "Not ready" ditambah ": {detail}"; `unknown` "Unknown" ditambah " ({reason})"; selain itu "—" (CLI `-`) |
| Version | `detail.provider.displayName` (cadangan `software`), `detail.runtime.version`, `detail.runtime.target` | "{nama} {version} · {arch}" dengan `aarch64` "arm64" dan `x86_64` "x86_64" |
| Runtime | `detail.runtime.id` | UUID |
| Digest | `detail.runtime.artifactSha256`, `installMode` | Dihapus dari cakupan pada 2026-10-24. |
| Provider | `detail.provider` | GUI Details "{name} {revision}"; CLI tetap baris spec 0022 |
| Data | `detail.dataPath` | Dihapus dari cakupan pada 2026-10-24. |
| Data size | `DataUsage` | "{ukuran}", "at least {ukuran}" kalau tidak lengkap, "Calculating…" selama dimuat, "—" kalau `usage` null, "Unavailable" kalau error; ukuran: di bawah 1 000 byte "{n} bytes", selain itu kB, MB, GB, atau TB desimal (1 000) dengan satu angka desimal, misalnya "1.2 GB" |
| Uptime | `InstanceSummary.process.startedAt` | `UptimeText` spec 0022, atau "—" tanpa proses |
| Restart attempts | `InstanceSummary.restart` | "{attempts} in the last {menit} min" kalau lebih dari 0, "Stopped restarting" kalau `exhausted`, selain itu baris tidak tampil |
| Autostart | `detail.startup` | `manual` "Off", `at_login` "At login", asing "Unknown" |
| Dependents | `detail.dependents` | "Nothing depends on this instance." atau "{name} · {Dedicated \| Shared}" ditambah " · lease active"; CLI `none` atau `<name> (<ownership>, lease active)` |
| Secrets | `detail.secrets` | "{reference}" ditambah status; CLI `<field>: <reference> (<status>)` |
| Connection | `ConnectionString` | `masked`, atau teks alasan dari *Copy* (CLI `unavailable (<reason>)`) |
| Connection tanpa entri (2026-10-22) | `detail.provider.packageReadable`, `detail.connections` | paket tidak terbaca: GUI teks *Copy* `package_unavailable` lewat `DetailFormat.connectionUnavailableText`, CLI `inspect` satu baris `Connection  unavailable (package_unavailable)`; paket terbaca tanpa blok: GUI teks *Copy* provider tanpa blok connection, CLI `inspect` tanpa tabel `CONNECTION` |

### Halaman detail dan navigasi

- `ShellDestination.instance(UUID)` (usulan spec 0022) mengisi `ShellNavigator.instanceRequest` dan memilih bagian Instances; `InstancesPage` menerapkannya lewat `.onChange(of: navigator.instanceRequest, initial: true)` ke `NavigationStack` nya (path `[InstanceRoute]`), sehingga permintaan tetap diterapkan saat Instances belum tampil. *(Diubah 2026-10-31, spec 0022 bagian Navigasi butir Umur request.)* `ShellNavigator` menyimpan request itu sampai section berganti dan halaman tidak mengosongkannya, karena SwiftUI bisa membangun halaman Instances dua kali pada kunjungan pertama jendela; halaman menerapkan request yang ada saat muncul dan saat berubah. `openInstance(_:)` memanggil `.instance(id)`. Tombol kembali sistem kembali ke tabel dengan pilihan baris tetap; klik ulang Instances di sidebar mengosongkan path lewat hook `onReselect` (lihat *Perintah halaman* di bawah).
- Header `.pageChrome(title:subtitle:)`: judul `name`; subjudul nama tampil provider (dari detail, atau `software` sebelum detail termuat), `runtimeVersion`, dan label `StatusLabel` spec 0012 (`StatusAppearance.instance(lifecycle:readiness:health:)`).
- Toolbar: tombol aksi utama `InstanceActionModel` (simbol `play.fill`, `stop.fill`, atau `arrow.clockwise` untuk Retry, ditambah label teks) lalu menu More (`ellipsis.circle`, label "More") dengan "Copy Connection String" (entri pertama, perilaku Copy yang sama, nonaktif dengan alasan "No connection string available." atau alasan entri; saat `provider.packageReadable` false, alasan nya teks *Copy* `package_unavailable`, 2026-10-22). *(fitur 24, spec 0024: item More kedua adalah "Duplicate…", sesudah Copy Connection String.)* Menu More dibangun langkah 10 sebagai `ActionMenu` dengan satu bagian (`[[StacklyAction]]` spec 0067) yang di fitur ini hanya berisi Copy Connection String; milestone 3 [spec 0024](../0024-instance-kedua-runtime-sama/index.md) menambah `StacklyAction` "duplicate" tepat sesudahnya di bagian yang sama, tanpa pemisah (dirender hanya saat `duplicateInstanceAction` tidak nil, `mutates` true, tanpa shortcut), dan aksi, sheet, serta penerbit nya dibangun spec 0024, bukan spec ini; fitur 54 menambah bagian kedua untuk Remove *(2026-10-22)*. Item yang mengubah state memakai `.mutationGated()`; Copy tidak.
- *Komponen dan perintah (2026-10-21, spec 0067 dan [spec 0069](../0069-menu-aplikasi-perintah-shortcut/index.md)):* aksi utama adalah `StacklyAction` dari `InstanceActionModel.stacklyAction(for:)` (adapter milik spec 0022 langkah 6) yang dirender `ActionButton(action, showsReason: false)` (wajib untuk tombol toolbar, spec 0067 AC-7), dan alasan nonaktif nya tampil di konten halaman sebagai `DisabledReasonText` di atas kartu Summary, di bawah banner. Menu More adalah `ActionMenu` dengan `StacklyAction` "Copy Connection String" (`mutates` false, `disabledReason` dari copy di atas). Tooltip tombol aksi utama memakai `CommandMap.helpText(for: .startInstance, title: action.title)` atau `.stopInstance` sesuai action nya (misalnya "Retry (⌘R)"); menu More tidak punya `CommandID`, jadi help nya default `ActionMenu` ("More"), dan item More tidak memasang shortcut. Kalimat `.mutationGated()` di atas diwujudkan lewat `availability(gate:)` `StacklyAction`.
- *Perintah halaman (2026-10-21, spec 0069 AC-11 dan AC-12):* halaman Instances menerbitkan `openInstance` (Open di inspector), `back` (⌘[, hanya saat detail terbuka), dan `copyConnectionString` (action yang sama dengan item More dan baris Connection inspector; alasan nonaktif nya saat `provider.packageReadable` false dari `DetailFormat.connectionUnavailableText`, 2026-10-22), dengan target detail yang terbuka mengalahkan pilihan tabel. Aturan klik ulang sidebar diisi lewat hook `onReselect(section)` di `AppShell.select(_:)` (spec 0069 AC-7): untuk Instances hook itu mengosongkan path. Ketukan pada baris sidebar yang sudah terpilih juga memanggil `select(_:)` (misalnya lewat `simultaneousGesture` pada baris, karena `List(selection:)` tidak memanggil setter untuk nilai yang sama), jadi klik ulang dan ⌘2 di detail sama sama kembali ke tabel.
- `InstanceTab` hanya berisi `.summary` di fitur ini; picker tab tampil saat ada lebih dari satu tab.
- State halaman: instance tidak ada di `InstanceStore` → satu `instance.get`; `resource.not_found` → `StatePlaceholder` "Instance Not Found", "It may have been removed.", tombol Back to Instances; instance yang menjadi retained (event spec 0010) atau dihapus (`instance.removed` spec 0020) saat halaman terbuka menampilkan placeholder yang sama dengan pesan "This instance was removed.".

### CLI

**`stackly instance connection`**:

```text
stackly instance connection <instance> [--format <name> | --all] [--show-password] [--json]
```

- Resolver nama atau UUID spec 0022 (`resolve.rs`), lalu `instance.get_detail`.
- Teks: tanpa `--all`, stdout tepat `masked` diikuti newline; kalau `portMayChange`, stderr menulis `note: the port can change the next time the instance starts`. `--all`: satu baris per entri `<name>  <masked>` atau `<name>  unavailable (<reason>)`, dengan lebar kolom nama tetap.
- `--all` memakai `conflicts_with` clap terhadap `--format` dan `--show-password` (exit 2, envelope `cli.usage` spec 0008).
- `detail.provider.packageReadable` false (*2026-10-22*): setiap bentuk (default, `--format`, `--all`, dengan atau tanpa `--show-password`) gagal `cli.connection_unavailable` dengan `reason` `package_unavailable` (exit 3), tanpa reveal dan tanpa nilai connection di stdout; dengan `--json`, stdout hanya envelope error (spec 0008 AC-2), jadi `--all --json` juga memberi envelope itu, bukan `{ instance, connections }`. Kunci `format` berisi nilai `--format` kalau dikirim, dan tidak ada kalau tidak. Karena tidak ada entri, alasan ini juga yang dilaporkan untuk instance `retained` yang paket nya tidak terbaca. Provider tanpa blok `connection` (paket terbaca, `connections` kosong) tetap gagal `cli.connection_not_found`.
- `--show-password`: reveal per reference berbeda (`purpose: show`), lalu stdout tepat string lengkap; reveal yang gagal menghentikan perintah dengan error `secret.*` itu tanpa mencetak apa pun ke stdout; entri tanpa secret mencetak string nya tanpa reveal.
- Kode area `cli` baru (spec 0008, aturan pemakai pertama) di tabel *Kode error*.

**`stackly instance inspect`** (perluasan spec 0022): sesudah `instance.get`, perintah memanggil `instance.get_detail` dan `instance.get_data_usage` (`cachedOnly: true`, atau `refresh: true` dengan flag baru `--size`; kegagalan data usage tidak menggagalkan perintah), dan mengambil ulang keduanya sekali kalau `detail.revision` berbeda dengan `revision` summary. Blok `kv` mendapat baris sesudah `Version`: `Digest` (lengkap), `Data` (path lengkap), `Data size`, `Autostart`; sesudah `Logs`: `Dependents` dan `Secrets` (satu baris per secret). Lalu tabel `CONNECTION`, `VALUE` dengan nilai tersamar atau `unavailable (<reason>)`; saat `detail.provider.packageReadable` false, di posisi tabel itu `inspect` mencetak satu blok `kv` tersendiri (panggilan `HumanWriter::kv` dengan satu baris, jadi label nya diikuti tepat dua spasi): `Connection  unavailable (package_unavailable)`; saat paket terbaca tanpa blok `connection`, tabel itu tidak dicetak (*2026-10-22*). `--json`: `{ "instance": InstanceSummary, "detail": InstanceDetail, "dataUsage": DataUsage | null }`.

### Kode error

Semua `retryable` false. Hint statis lewat `codes::<area>::hint(code, details)` yang dipasang `stacklyd` lewat `ErrorBody::with_hint` (CLI memasangnya sendiri untuk kode `cli`).

| Kode | Kategori | Kapan | Kunci `details` | Pesan | Hint statis |
| --- | --- | --- | --- | --- | --- |
| `instance.data_usage_unavailable` | `unavailable` | folder data hilang, bukan folder privat milik user, atau tidak terbaca (pemetaan di *Ukuran data*) | `reason` (`missing`, `not_private`, `io`) | `the data folder size can't be measured` | per `reason`: `missing` "run stackly doctor; the data folder is missing"; `not_private` "run stackly doctor; the data folder isn't a private folder you own, or part of it can't be read" (2026-10-22); `io` "try again" |
| `cli.connection_not_found` | `not_found` | `--format` tidak ada, atau provider tanpa blok `connection` | `format`, `available` | `no connection named <format>` atau `this provider describes no connection strings` | "list them with stackly instance connection <instance> --all" |
| `cli.connection_unavailable` | `conflict` | entri terpilih tidak tersedia, atau paket pinned tidak terbaca (`provider.packageReadable` false, *2026-10-22*) | `format` (nama entri terpilih: nilai `--format` atau entri pertama; tidak ada hanya saat paket tidak terbaca dan `--format` tidak dikirim), `reason` | `the <format> connection is not available yet`, atau `the provider package of this instance can't be read` untuk `package_unavailable` dari paket yang tidak terbaca | per `reason`: `port_not_assigned` "start the instance to assign its port"; `package_unavailable` "run stackly doctor"; `retained` "the instance was removed"; asing "update Stackly" |

Kode issue manifest `connection_invalid` (reason di `Issue.reason`: `name`, `title`, `template_length`, `not_a_uri`) dan `connection_name_duplicate` ada di katalog `IssueCode` spec 0009 (diterapkan 2026-10-20). Aturan `Issue.reason` (spec 0009, keputusan lead D2 yang sudah pasti lewat spec 0017): field core dibangun spec 0017 milestone 1 langkah 3 (pemakai pertama `catalog_limit`); kunci wire `reason` di item `issue` (`ItemSpec` `issue` `codes::manifest`), fixture, `cargo xtask codegen`, dan kenaikan minor protokol (CURRENT+1 saat merge, K4) milik langkah 1 spec ini. Issue catalog hanya sampai ke wire di DTO `ProviderDetail.issues` spec 0017 sendiri (`CatalogIssue.reason`, di dalam minor fitur 17), tidak pernah sebagai `ErrorBody.items` ber kind `issue`.

### Aksesibilitas

- Urutan fokus halaman detail: tombol kembali, aksi utama, More, banner dan tombolnya, kartu Endpoints (setiap tombol Copy), kartu Connection (Show Password, Copy per entri), kartu Details (Copy ID, Recalculate data size).
Hasil copy diumumkan "{title} copied." atau "Address copied.", dan "{name} is ready." diumumkan sekali setelah readiness `ready` bertahan 2 detik.
- Status proses dan readiness selalu dua teks terpisah di Summary; warna tone hanya tambahan.
- Teks membungkus di lebar konten 640 pt; nilai monospace yang panjang (connection, path) membungkus per karakter tanpa dipotong, dan tetap bisa dipilih.
- *Lebar dengan inspector (2026-10-21, keputusan user atas temuan langkah 14 spec 0067):* di jendela minimum 900 pt dengan sidebar (236 pt) dan inspector (280 pt) terbuka, kolom halaman hanya 384 pt (424 pt dengan inspector 240 pt), dan inspector dibatasi paling lebar 280 pt di jendela di bawah 1000 pt (spec 0067), jadi 384 pt adalah batas bawahnya; jaminan konten minimum 640 pt spec 0013 hanya berlaku saat inspector tertutup. Halaman detail dan tab Summary bekerja di kolom 384 pt saat inspector Instances terbuka: kartu satu kolom, baris label dan nilai dari `.labeledContentStyle(.stackly)` bertumpuk saat sempit, dan tombol Copy tetap di samping nilainya tanpa teks terpotong.

### Copy

| Konteks | Teks |
| --- | --- |
| Connection `port_not_assigned` | "Start the instance to assign its port." |
| Connection `package_unavailable`, juga kartu Connection dan alasan Copy Connection String saat `provider.packageReadable` false | "Stackly can’t read this provider’s package. Run stackly doctor." |
| Connection `retained` | "This instance was removed." |
| Connection state atau bagian asing | "Update Stackly to copy this connection string." |
| Provider tanpa blok connection | "This provider doesn’t describe a connection string. Use the endpoints above." |
| Port bisa berubah | "The port can change the next time the instance starts." |
| Belum siap | "Not accepting connections yet." |
| Secret tanpa nilai | "{reference} has no stored value yet." |
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
| `DATA_USAGE_PARTIAL_TTL` | 10 detik | `stacklyd` |
| `DATA_USAGE_CONCURRENCY` | 2 | `stacklyd` |
| `TREE_USAGE_MAX_DEPTH` | 32 tingkat (root tingkat 1) | `stackly-platform`, di samping `REMOVE_TREE_MAX_DEPTH` (spec 0004) |
| `PINNED_PACKAGE_CACHE` | 32 entri | `stacklyd` |
| `DETAIL_RELOAD_DEBOUNCE` | 250 ms | `InstanceDetailModel` |
| `READY_ANNOUNCE_DELAY` | 2 detik | `InstanceDetailModel` |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| detail | `provider.name`, `revision`, `manifestSha256` | `Instance.provider_revision` (registry) |
| detail | `provider.displayName` | `metadata.displayName` paket pinned di `provider_revision_dir(key)` (cache paket); null kalau tidak terbaca |
| detail | `provider.packageReadable` (2026-10-22) | `DetailSnapshot.package` yang ada dan `key()` nya sama dengan `Instance.provider_revision`, dihitung `detail::build`; false selain itu |
| detail | `runtime.*` | `Registry::runtime(instance.runtime_id)`; `target` sebagai `HostTarget`; `artifactSha256` dari `InstallMode::Managed` |
| detail | `dataPath` | baris `instance_paths` kind `data` di registry, diselesaikan `RootSet::resolve` |
| detail | `startup` | `Instance.startup` |
| detail | `secrets[]` | `secret_uses_by(Instance(id))` urut field, status dari cache `secret_references` (spec 0011, `unknown` tanpa baris), judul dari `title` field schema paket pinned (cadangan: nama field) |
| detail | `dependents[]` | `Registry::environments()` yang punya member instance itu (ownership dari member), `lease_active` dari `Registry::leases(id, Unreleased)` berholder environment itu, urut nama lalu ID |
| detail | `connections[]` | blok `connection` paket pinned, `render` (AC-2) dengan `PlanContext` (layout, `instance_run_dir`, runtime, port efektif) dan reference dari `secret_uses_by` |
| detail | `revision` | `Instance.revision` saat dibaca |
| data usage | `bytes`, `complete` | `Filesystem::tree_usage(dataPath, limits)` |
| data usage | `scannedAt` | `Clock::now()` saat scan selesai |
| Summary | label status header | `StatusAppearance.instance(lifecycle:readiness:health:)` spec 0012 dari `InstanceSummary` |
| Summary | Process, Readiness, endpoint, restart, blocker, `lastFailure` | `InstanceSummary` dari `InstanceStore` (spec 0010, 0013) lewat `ProcessText` dan `ReadinessText` |
| Summary | uptime | `InstanceSummary.process.startedAt` dan jam lokal (`UptimeText`) |
| Summary | nilai secret untuk Show dan Copy | `secret.reveal { reference, purpose }` (spec 0011), satu per reference berbeda |
| Summary | string lengkap | `ConnectionComposer.compose(parts, values)` |
| Summary | tile dan mark | `CatalogStore` kalau termuat, selain itu tile `gray` dengan mark default spec 0017 |
| Summary | "Not accepting connections yet." | `lifecycle` `running` dan readiness bukan `ready` |
| CLI connection | entri terpilih | `--format` atau entri pertama `detail.connections`, hanya saat `detail.provider.packageReadable` true; false langsung menjadi `cli.connection_unavailable` `package_unavailable` |
| GUI | teks Connection tanpa entri karena paket tidak terbaca (2026-10-22) | `DetailFormat.connectionUnavailableText(detail)` |
| CLI connection | string lengkap | `secret.reveal` dan `stackly_protocol::instance::connection::compose` |
| offline test | akses catalog, fetch, assess | penghitung akses `CatalogState` di `stacklyd`, `Fetcher` tiruan, `CodeSigning` tiruan |

### Key invariants

- Process state dan readiness selalu tampil sebagai dua baris terpisah di Summary dan dua field di CLI; tidak ada teks yang menyebut instance siap tanpa readiness `ready` dari probe yang segar (BR-010).
- Daemon tidak pernah mengirim nilai secret di `InstanceDetail`, event, atau error; string tersamar memakai empat bintang tetap.
- Tidak ada reveal, tulisan pasteboard, atau cetak password tanpa aksi eksplisit pengguna, dan komposisi tidak pernah mengganti nilai yang hilang dengan string kosong.
- Start, status, detail, data usage, dan connection tidak pernah membaca `CatalogState` daemon, memanggil jaringan, atau menilai signature (REQ-046).
- Connection dirender dengan `PlanContext` dan resolver yang sama dengan plan, sehingga host, port, dan socket nya selalu sama dengan yang di bind proses.

### Security model

- Semua method lewat socket privat milik user yang sama (spec 0006); `instance.get_detail` mengirim path absolut folder data hanya ke pemiliknya (pola `RuntimeDetail.path` spec 0018).
- Nilai secret hanya keluar lewat `secret.reveal`, yang diaudit per purpose (spec 0011 AC-9); CLI hanya mencetaknya dengan `--show-password`, GUI hanya menampilkannya di `RevealableSecretText` yang tersembunyi lagi setelah 30 detik dan hanya menyalinnya lewat `ConcealedPasteboard` (host saja, transient, dikosongkan setelah 90 detik).
- Template connection divalidasi saat paket dimuat (token, panjang, dan render sentinel sebagai URI); setiap nilai token di percent encode, sehingga nilai pengguna tidak bisa mengubah struktur URI.
- Scan ukuran data tidak mengikuti symlink, tidak menyeberang device, dibatasi entri, waktu, kedalaman (jadi paling banyak `TREE_USAGE_MAX_DEPTH` file descriptor per scan), dan jumlah scan bersamaan, dan hanya dimulai atas permintaan.
- Daemon tidak mencatat isi detail ke log.

### Configuration required

Tidak ada variabel environment, setting, atau credential baru.

### Critical test scenarios

- Manifest: blok `connection` valid untuk PostgreSQL, NATS, dan fake; setiap aturan penolakan AC-1 dengan kode issue dan path nya, termasuk template yang bukan URI. Memverifikasi **AC-1**.
- Render (unit test): IPv4 dan IPv6, port `Fixed`, `Auto` dengan dan tanpa `bound`, `running` dan `stopped` (`port_may_change`), socket dengan folder run berspasi dan Unicode, entri tanpa secret, paket yang bukan paket pinned (`key()` lain), instance retained, dan urutan alasan. Memverifikasi **AC-2**.
- Komposisi: setiap kasus `connection-compose.json` di core, protokol, dan Swift, termasuk nilai yang hilang. Memverifikasi **AC-3**.
- Detail: instance fake dengan secret `authToken`, dua environment fixture (dedicated dengan lease aktif, shared tanpa lease), paket pinned yang dirusak lalu diperbaiki (cache dibaca ulang), penghitung `SecretStore` nol. Paket yang dirusak memberi `packageReadable` false, `displayName` null, `connections` kosong, dan judul secret `authToken`; paket di snapshot dengan `key()` lain memberi hal yang sama; paket tanpa blok `connection` (`package(false)` di `crates/core/tests/instance_detail.rs`, fixture `fake-command` tanpa blok itu) memberi `packageReadable` true dengan `connections` kosong; di test daemon, paket yang dirusak lalu diperbaiki memberi `packageReadable` false lalu true lagi (*2026-10-22*). Memverifikasi **AC-4**.
- Paket tidak terbaca di CLI dan GUI (*2026-10-22*), dengan fixture kontrak `protocols/fixtures/instance/detail-package-unreadable.json`: `stackly instance connection` default, `--format uri`, dan `--all`, masing masing dalam teks dan `--json`, gagal `cli.connection_unavailable` `reason` `package_unavailable` (exit 3, tanpa reveal, `format` hanya pada `--format`, pesan paket tidak terbaca; golden envelope untuk ketiga nya); `inspect` mencetak blok satu baris `Connection  unavailable (package_unavailable)`; test Swift untuk `DetailFormat.connectionUnavailableText` dan untuk kartu Connection, alasan Copy Connection String, baris Connection inspector, dan alasan action `copyConnectionString`. Test kontrak: setiap fixture detail dengan `packageReadable` false punya `displayName` null dan `connections` kosong, dan ketiga fixture triple lain membawa `packageReadable` true. Memverifikasi **AC-4**, **AC-6**, **AC-7**, **AC-13**, **AC-15**.
- Data usage di platform (langkah 4): kasus `fs_conformance` dan tenggat nyata di *Ukuran data* (tepat batas entri dan satu lebih, root kosong dengan batas 0, 32 dan 33 tingkat, tenggat nol, root relatif, symlink, file, atau tidak ada), ditambah symlink keluar, hard link, file sparse, dan volume lain. Memverifikasi **AC-5**.
- Data usage di daemon (langkah 7): 250 000 entri dengan konstanta asli (`complete: false`, TTL 10 detik lewat `FakeClock`), batas yang diperkecil lewat `DaemonConfig`, folder hilang, bukan privat, dan diganti antara cek dan scan (`run_on_next`), `PermissionDenied`, `refresh` (juga saat scan berjalan), `cachedOnly` tanpa cache dan dengan entri kedaluwarsa, `refresh` dan `cachedOnly` bersamaan ditolak, dua permintaan bersamaan (satu scan, `tree_usage_calls()`), client yang putus saat scan berjalan, dan tiga instance bersamaan (paling banyak dua scan, `tree_usage_peak()`, dengan `set_delay` menahan scan). Memverifikasi **AC-5**.
- CLI connection: default tersamar, `--format socket`, format tidak ada, entri tidak tersedia (exit 3), provider tanpa blok, `--all`, konflik clap, `--show-password` dengan audit dan untuk entri tanpa secret, golden teks dan JSON. Memverifikasi **AC-6**.
- Paritas: golden `inspect` dan test Swift dari empat fixture triple (sejak 2026-10-22 termasuk `detail-package-unreadable.json`), termasuk ambil ulang saat revision beda. Memverifikasi **AC-7**.
- AC-012: fake dengan `readyDelayMs` panjang, lalu probe berhasil. Memverifikasi **AC-8**.
- Start dari halaman detail: view model dengan service tiruan dan urutan event, connection `port_not_assigned` menjadi tersedia. Memverifikasi **AC-9**.
- Offline: server di dalam proses tanpa catalog, `Fetcher` dan `CodeSigning` tiruan yang gagal saat dipanggil, binary CLI asli, penghitung akses catalog nol. Memverifikasi **AC-10**.
- GUI: navigasi (klik ganda, Return, Open, Overview, permintaan saat Instances belum tampil, kembali, klik ulang sidebar, tidak ditemukan, dihapus saat terbuka), toolbar dan More, kartu Summary dari fixture, Copy endpoint (pasteboard biasa) dan Copy connection (concealed, setelah reveal semua reference), Show Password dan sembunyi otomatis, secret `missing` menonaktifkan, caution belum siap, banner I1 sampai I4 dan needs secret, state detail gagal, balasan lama dibuang, dan offline, inspector Open, Data, Connection. Memverifikasi **AC-11** sampai **AC-15**.
- Aksesibilitas: evidence manual interaksi desktop untuk alur Start sampai Copy. Memverifikasi **AC-16**.
- Kontrak: fixture setiap DTO, kode, nilai asing, dan konsistensi `ConnectionString`. Memverifikasi **AC-17**.
- Sentinel: prefix fixture credential tidak ada di detail, event, log, output CLI default dan `--json`, dan state model Swift. Memverifikasi **AC-18**.
- Kinerja: 10 instance, cache dingin dan hangat. Memverifikasi **AC-19**.
- Native: PostgreSQL 18 dan NATS lewat GUI dan CLI, `psql` dan `INFO`, lalu jaringan mati setelah persetujuan. Memverifikasi **AC-10**, **AC-20**.

### Jawaban untuk follow up spec lain

| Spec dan baris Follow-up | Jawaban di spec ini |
| --- | --- |
| 0009: `Endpoint` domain membawa `purpose` atau dibaca dari paket (fitur 20 dan 21) | dibaca dari paket pinned (keputusan spec 0020 dipertahankan); connection tidak memakai purpose, melainkan blok `connection` yang menyebut endpoint nya sendiri |
| 0011: connection URI tersamar secara default dan `RevealableSecretText` untuk Show Password dan Copy | AC-2, AC-13; `RevealableSecretText(references:compose:)` lewat usulan spec 0011 |
| 0019: bentuk connection string PostgreSQL dan tempatnya di manifest | blok `connection` (AC-1) dan entri `uri` serta `socket` di *Connection PostgreSQL, NATS, dan fake* |
| 0020: purpose endpoint dan template socket dari paket pinned; socket path hanya saat `socketPath` tidak null | render memakai `PlanContext` dari paket pinned dan port efektif; entri socket `port_not_assigned` selama port rujukannya belum ada (AC-2) |
| 0022: ganti `openInstance` dengan halaman detail, Open di inspector, "Double-click a row to open it.", baris Data dan Connection di inspector | AC-11, AC-15, usulan spec 0022 |
| 0023: connection string NATS dari endpoint `client`, tanpa credential, dan URL monitoring | entri `uri` dan `monitoring` di *Connection PostgreSQL, NATS, dan fake* |
| 0013: pemilik halaman detail instance | AC-11 sampai AC-14 |
| 0004: BR-001 dan BR-010 | BR-001 tidak berubah (detail membaca runtime dan revision exact); BR-010 di AC-8 |
| 0010: start, readiness, `instance.get`, event | dipakai apa adanya (AC-8, AC-9) |
| 0012: tata letak kartu Summary | `surfaceWindow` dan `.cardSurface()` (*Tab Summary*), label status header tanpa perubahan |

### Usulan update spec lain

Ditulis di sini karena spec ini tidak boleh mengubah spec lain; lead membawanya ke pemilik masing masing sebelum milestone yang membutuhkannya. Satu blok per spec target, setiap blok berdiri sendiri. Teks lama dikutip persis dari spec target di `main` (2026-10-20). "Ganti" berarti teks baru menggantikan teks lama; "Sisipkan sesudah" berarti teks lama tetap dan teks baru ditambahkan tepat sesudahnya.

#### Spec 0004 (domain core dan abstraksi platform)

Diterapkan di spec 0004 pada 2026-10-20. Aturan penelusuran rinci (keputusan `tree_usage` 2026-10-22: root, entri, kedalaman, hard link, tenggat, error, urutan) ditulis langsung di butir `tree_usage` spec 0004 oleh update spec ini, bukan lewat blok usulan; teks di bawah adalah bentuk asli 2026-10-20.

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
   - `tree_usage(root, UsageLimits { max_entries: u64, deadline: Duration })` *(ditambah fitur 21, spec 0021)* mengembalikan `TreeUsage { bytes, complete }`: jumlah `st_blocks * 512` setiap entri di bawah `root` termasuk folder, hard link dihitung sekali per (device, inode), tanpa mengikuti symlink, tanpa menyeberang device, dan `complete: false` saat batas entri tercapai atau tenggat (jam monotonik sejak panggilan) lewat. Adapter macOS berbasis file descriptor (`openat` dengan `O_NOFOLLOW`, `fstatat` dengan `AT_SYMLINK_NOFOLLOW`); `FakeFilesystem` memakai penelusuran portabel dengan semantik yang sama; `fs_conformance` memeriksa symlink keluar, hard link, dan batas entri.
   ```

#### Spec 0009 (provider engine dan manifest schema)

Diterapkan di spec 0009 pada 2026-10-20 (sesi `upd-0009`).

1. *Manifest `stackly/v1alpha1`*, tabel field. Sisipkan sesudah:

   ```text
   | `permissions.additionalExecutables` | peta alias ke path relatif | tidak | paling banyak 16, path ada di `executables` artefak |
   ```

   baris:

   ```text
   | `connection` | daftar `{ name, title, template }` | tidak | 1 sampai 8 entri; `name` `^[a-z][a-z0-9_]{0,31}$` dan unik, `title` 1 sampai 64 karakter tanpa karakter kontrol, `template` 1 sampai 512 byte di posisi `connection.template` yang hasil render nya dengan nilai sentinel wajib URI absolut RFC 3986; entri pertama adalah connection utama; kode issue `connection_invalid` (reason di `Issue.reason`: `name`, `title`, `template_length`, `not_a_uri`) dan `connection_name_duplicate`, token salah memakai `token_not_allowed_here`, `unknown_endpoint`, dan `unknown_secret` (spec 0021) |
   ```

2. *Interpolasi terkontrol*, matriks posisi. Sisipkan sesudah:

   ```text
   | `{{ }}` di template | ya | ya | ya (sebagai teks) | ya (sebagai teks) | tidak | hanya di config `sensitive` | |
   ```

   baris:

   ```text
   | `connection.template` | hanya `endpoint.<n>.address`, `endpoint.<n>.port`, `instance.name`, `instance.id`, `provider.id`, `provider.revision`, `runtime.version` | tidak | hanya `endpoint.<n>.socket` dan `endpoint.<n>.socketDir` | tidak | tidak | ya, `secret.<field>` | `endpoint.<n>.host` dan `settings.*` ditolak; nilai token selain `endpoint.<n>.address` dan `endpoint.<n>.port` di percent encode sebagai byte UTF 8 dengan hex huruf besar kecuali karakter unreserved RFC 3986; teks literal tidak di encode (spec 0021) |
   ```

#### Spec 0011 (secret store dan redaction)

Diterapkan di spec 0011 pada 2026-10-20.

1. *Reveal dan copy*, butir GUI. Ganti:

   ```text
   Show memanggil `revealSecret(purpose: .show)` dan menampilkan nilai sampai salah satu dari ini terjadi lebih dulu: 30 detik berlalu, pengguna menekan Hide, view hilang, atau app tidak aktif. Copy memanggil `revealSecret(purpose: .copy)`, menyusun teks lengkap lewat closure pemanggil (misalnya URI dengan password asli), lalu menulis pasteboard lewat `ConcealedPasteboard`.
   ```

   dengan:

   ```text
   Komponen menerima `references: [String]` (satu atau lebih reference berbeda) dan closure `compose: ([String: RevealedSecret]) throws -> String`. Show memanggil `revealSecret(purpose: .show)` untuk setiap reference dan menampilkan teks tersusun sampai salah satu dari ini terjadi lebih dulu: 30 detik berlalu, pengguna menekan Hide, view hilang, atau app tidak aktif. Copy memanggil `revealSecret(purpose: .copy)` untuk setiap reference, menyusun teks lengkap lewat closure pemanggil (misalnya URI dengan password asli), lalu menulis pasteboard lewat `ConcealedPasteboard`. Reveal berlaku semua atau tidak sama sekali: satu reveal yang gagal membatalkan aksi tanpa menampilkan atau menyalin apa pun, dan nilai yang sudah diterima dibuang (spec 0021).
   ```

#### Spec 0019 (provider PostgreSQL)

Diterapkan di spec 0019 pada 2026-10-20 (sesi `upd-0019-0022-0023`).

1. *Paket*, manifest. Sisipkan sesudah:

   ```text
       socket: { transport: unix, purpose: client, socketName: ".s.PGSQL.${endpoint.client.port}" }
   ```

   blok tingkat atas:

   ```text
   connection:
     - name: uri
       title: Connection string
       template: "postgresql://postgres:${secret.superuserPassword}@${endpoint.client.address}/postgres"
     - name: socket
       title: Unix socket
       template: "postgresql://postgres:${secret.superuserPassword}@/postgres?host=${endpoint.socket.socketDir}&port=${endpoint.client.port}"
   ```

2. *Value sourcing*. Ganti:

   ```text
   | connection (fitur 21) | user, database, password, endpoint | `postgres`, `postgres`, field `superuserPassword`, endpoint `client` atau `socket` (Follow-up) |
   ```

   dengan:

   ```text
   | connection (fitur 21) | connection string | blok `connection` paket ini (entri `uri` lewat endpoint `client`, entri `socket` lewat folder socket), dirender daemon dan disusun client dengan `superuserPassword` (spec 0021) |
   ```

3. *Build plan*, prasyarat. Ganti:

   ```text
   Langkah 11 juga butuh langkah 13 spec 0018 (milestone 3: `cargo xtask runtime compat-record` dan aturan `runtime check` 12).
   ```

   dengan:

   ```text
   Langkah 11 juga butuh langkah 13 spec 0018 (milestone 3: `cargo xtask runtime compat-record` dan aturan `runtime check` 12). Blok `connection` spec 0021 masuk paket sebelum evidence: langkah 1 dan 2 spec 0021 (loader dan blok paket) selesai sebelum langkah 11, dan blok itu ikut `provider.yaml` final yang dipakai evidence.
   ```

#### Spec 0022 (overview dan daftar instance)

Diterapkan di spec 0022 pada 2026-10-20 (sesi `upd-0019-0022-0023`). *Diselaraskan 2026-10-31:* teks butir 2 di bawah kini sama dengan *Navigasi* spec 0022 sesudah merge `b705d978` (commit `84f13ae9`): halaman tidak lagi mengosongkan `instancesRequest` dan `instanceRequest`, dan `ShellNavigator` menyimpan keduanya sampai section berganti (butir *Umur request* spec 0022).

1. *Navigasi*, `ShellDestination`. Ganti:

   ```text
   - `ShellDestination`: `.overview`, `.instances(filter: InstanceFilter = .all, select: UUID? = nil)`, `.catalog`, `.runtimes`, `.operations`, `.diagnostics`.
   ```

   dengan:

   ```text
   - `ShellDestination`: `.overview`, `.instances(filter: InstanceFilter = .all, select: UUID? = nil)`, `.instance(UUID)` (halaman detail fitur 21, spec 0021), `.catalog`, `.runtimes`, `.operations`, `.diagnostics`.
   ```

2. *Navigasi*, permintaan halaman detail. Sisipkan sesudah:

   ```text
   - `.instances` mengisi `instancesRequest` (filter dan pilihan). `InstancesPage` membacanya lewat `.onChange(of: navigator.instancesRequest, initial: true)`, jadi permintaan tetap diterapkan saat Instances sudah tampil: filter dipasang, pencarian dikosongkan, dan kalau `select` ada, baris itu dipilih dan inspector ditampilkan. Halaman tidak mengosongkan `instancesRequest` (butir *Umur request* di bawah).
   ```

   butir:

   ```text
   - `.instance(id)` memilih bagian Instances dan mengisi `instanceRequest`. `InstancesPage` memegang `NavigationStack` dengan path `[InstanceRoute]` dan membaca request itu lewat `.onChange(of: navigator.instanceRequest, initial: true)`: path diisi `[InstanceRoute(id)]` dan baris itu dipilih, jadi permintaan berlaku juga saat Instances belum tampil. Halaman tidak mengosongkan `instanceRequest` (butir *Umur request* di bawah). Klik ulang Instances di sidebar saat path tidak kosong mengosongkan path (spec 0021).
   ```

3. *Navigasi*, `openInstance`. Ganti:

   ```text
   - `openInstance(_ id:)` sama dengan `.instances(filter: .all, select: id)` (`// provisional: F21`, diganti halaman detail fitur 21).
   ```

   dengan:

   ```text
   - `openInstance(_ id:)` sama dengan `.instance(id)` (spec 0021).
   ```

4. *Halaman Instances*, inspector tanpa pilihan. Ganti:

   ```text
   5. Tanpa pilihan: `StatePlaceholder` ukuran `.section` "No Selection", "Select an instance to see its details.".
   ```

   dengan:

   ```text
   5. Tanpa pilihan: `StatePlaceholder` ukuran `.section` "No Selection", "Select an instance to see its details. Double-click a row to open it." (spec 0021).
   ```

5. *Halaman Instances*, baris Readiness inspector. Ganti:

   ```text
   | Readiness | "Ready", "Waiting for probe", "Not ready", atau "Unknown", ditambah `detail` dan "checked {RelativeAge}" dari `observedAt` |
   ```

   dengan:

   ```text
   | Readiness | `ReadinessText` spec 0021 ("Ready", "Waiting for probe", "Not ready" ditambah detail, "Unknown" ditambah reason, atau "—" saat tidak running), tanpa umur pemeriksaan karena spec 0010 tidak mengirim pembaruan `observed_at` saja |
   ```

6. *Halaman Instances*, catatan pemilik. Ganti:

   ```text
   - Data path, ukuran data, connection string, dan Open milik fitur 21.
   ```

   dengan:

   ```text
   - Fitur 21 (spec 0021) menambah tombol Open di samping aksi utama, baris Data (ukuran dari `instance.get_data_usage` dengan `cachedOnly: true` atau "—", path di baris kedua) sesudah Version, dan baris Connection (entri pertama tersamar dengan Show Password dan Copy) sesudah Endpoints.
   ```

7. *Requirements*, AC-11. Ganti:

   ```text
   dengan `--json` memberi `InstanceSummary`.
   ```

   dengan:

   ```text
   dengan `--json` memberi `result` `{ "instance": InstanceSummary }`; fitur 21 menambah key `detail` dan `dataUsage` dan flag `--size` (spec 0021).
   ```

#### Spec 0023 (provider NATS lewat manifest)

Diterapkan di spec 0023 pada 2026-10-20 (sesi `upd-0019-0022-0023`).

1. *Paket NATS*. Ganti:

   ```text
   Satu satunya perubahan fitur ini di `providers/nats/provider.yaml` adalah blok `readiness`:
   ```

   dengan:

   ```text
   Perubahan fitur ini di `providers/nats/provider.yaml` adalah blok `readiness` di bawah, dan blok `connection` dari spec 0021 (entri `uri` `nats://${endpoint.client.address}` berjudul "Client URL" dan entri `monitoring` `http://${endpoint.monitoring.address}` berjudul "Monitoring URL", tanpa credential sampai fitur 39), yang ditulis di commit yang sama supaya `manifest_sha256` hanya berubah sekali sebelum evidence BR-025:
   ```

2. *Requirements*, AC-4. Ganti:

   ```text
   - **AC-4**: Paket `providers/nats` memakai probe readiness di *Paket NATS* dan README yang diperbarui; selain itu isi paket tidak berubah oleh fitur ini
   ```

   dengan:

   ```text
   - **AC-4**: Paket `providers/nats` memakai probe readiness dan blok `connection` di *Paket NATS* dan README yang diperbarui; selain itu isi paket tidak berubah oleh fitur ini
   ```

3. *Build plan*, prasyarat. Sisipkan sesudah:

   ```text
   - Milestone 2 butuh build fitur 10 sampai milestone "Daemon, IPC, and end to end proof", fitur 11 selesai, milestone 2 spec 0018 (`install_cached_artifact`, langkah 9), dan milestone 1 spec 0017 (`providers/catalog.toml`, ledger, `CatalogBuilder`). Langkah 9 juga butuh langkah 1 spec 0018 (`runtime.versionCheck` NATS sudah di paket sebelum evidence) dan langkah 13 spec 0018 (`compat-record`, milestone 3).
   ```

   butir:

   ```text
   - Langkah 2 butuh langkah 1 spec 0021 (loader blok `connection`), dan langkah 9 butuh blok `connection` sudah di paket, supaya evidence dibuat dari `provider.yaml` final (spec 0021).
   ```

#### Spec 0013 (app shell SwiftUI dan Swift client)

1. *API surface*, tabel. Sisipkan sesudah:

   ```text
   | `ShellSync.register` | main actor | `ResourceStore` | | |
   ```

   baris:

   ```text
   | `ShellSync.events(kindPrefix:)` *(ditambah fitur 21, spec 0021)* | main actor | awalan jenis event (misalnya `secret.`) | `AsyncStream<Event>` | |
   ```

2. *ShellSync*, butir `.event`. Sesudah kalimat "`.event`: diarahkan ke store yang mendaftarkan awalan jenis itu (misalnya `instance.`), lalu ke `reduce` store." tambahkan:

   ```text
   Pengamat tanpa store (misalnya `InstanceDetailModel` spec 0021 untuk `secret.`) memakai `events(kindPrefix:) -> AsyncStream<Event>` *(ditambah fitur 21, spec 0021, dibangun langkah 9 nya)*, yang menerima setiap event dengan awalan jenis itu.
   ```

Tidak ada update untuk spec 0006, 0008, 0010, dan 0012: method dan kode baru masuk area pemiliknya lewat aturan pemakai pertama, `socketPath` sudah diusulkan spec 0020, dan label status spec 0012 dipakai apa adanya.

### Selisih mockup yang disengaja

Untuk dicatat lead di `ui-macos.md`:

- Readiness menampilkan state dan detail probe sebagai baris sendiri di bawah baris Process, tanpa latensi ("SELECT 1 · 8 ms") dan tanpa umur pemeriksaan; latensi milik fitur 28.
- Header memakai label status spec 0012 ("Starting…" atau "Not Ready" selama probe belum berhasil), bukan "Running".
- Restart attempts di Details adalah jumlah di jendela restart policy dan hanya tampil kalau lebih dari 0, bukan "Restarts" sejak dibuat; jumlah sejak dibuat milik fitur 28.
- Runtime menampilkan UUID runtime, bukan ID buatan mockup (`rt-pg-17.6`); Instance ID menampilkan UUID, bukan `inst_...`.
- Secret reference memakai bentuk spec 0011 (`local:instance/<id>/superuser-password`), bukan `keychain:...`.
- Kartu Connection menampilkan semua entri provider (PostgreSQL: Connection string dan Unix socket; NATS: Client URL dan Monitoring URL), bukan satu string, dan menulis "Not accepting connections yet." selama belum siap.
- Tab Configuration, Logs, Metrics, Backups, dan Activity serta tombol Restart, Reload, Duplicate, dan Remove tidak tampil sampai fitur pemiliknya dibangun.
- Banner gagal tidak menampilkan "Use Port …" atau Show Logs sampai fitur 29 dan 27; Retry saja.
- Autostart menulis "Off" atau "At login", bukan "With environment".

## Build plan

Urutan mengikuti pendekatan Journey proyek. Jalur yang dibuat lengkap adalah "pengguna menjalankan instance, melihat ready, lalu menyalin connection", lebih dulu di core (manifest, render, ukuran data), lalu lewat daemon dan CLI dengan bukti offline, lalu di GUI, lalu dibuktikan dengan PostgreSQL dan NATS nyata di Mac ini.

**Milestone 1: Connection dan ukuran data di core** (update spec 0004, 0009, 0011, 0019, 0022, dan 0023 dari *Usulan update spec lain* sudah ada di `main`; needs fitur 9 build; langkah 2 butuh fitur 19 langkah 4 (paket PostgreSQL, keputusan lead D5) dan, untuk cek golden render NATS, fitur 23 langkah 2 (blok NATS, D1), yang sendiri menunggu langkah 1 spec ini)

1. Blok `connection` di `core::provider` (raw, validasi dengan kode issue, posisi matriks, render sentinel dan parse URI, JSON Schema `schemas/provider/`, `provider check`), dengan test penolakan; `Issue.reason` dari spec 0009 (field core sudah dibangun spec 0017 milestone 1 langkah 3), dan kunci wire `reason` di item `issue` (`ItemSpec` `issue`) dengan fixture, `cargo xtask codegen`, dan kenaikan minor sendiri (D2; issue catalog tidak pernah sampai ke wire sebagai item `issue`). Memenuhi **AC-1**.
2. Blok `connection` di paket PostgreSQL dan fake (blok NATS ditulis langkah 2 spec 0023 di commit yang sama dengan readiness, D1; golden render NATS dicek di sini), dengan golden render dan kenaikan revision catalog sesuai aturan spec 0017. Memenuhi **AC-1**.
3. `core::instance::connection::render` di atas `PlanContext` dengan test tabel, `stackly_protocol::instance::connection::compose`, dan fixture `connection-compose.json` yang dijalankan lewat keduanya. Memenuhi **AC-2**, **AC-3**.
4. `Filesystem::tree_usage` dan `TREE_USAGE_MAX_DEPTH` di `stackly-platform` (dengan impl `UnsupportedPlatform`), `MacosFilesystem` dan `FakeFilesystem` (dengan `tree_usage_calls()` dan `tree_usage_peak()`) dengan aturan penelusuran di *Ukuran data*, `fs_conformance`, dan test tenggat nyata serta volume lain di macOS. Memenuhi **AC-5**.
5. `core::instance::detail::build` (fungsi murni dari snapshot registry, paket pinned, dan layout) dengan test. Memenuhi **AC-4**.

Sesi paralel: langkah 1 dan 4 bisa menjadi dua sesi (provider dan platform); langkah 2 dan 3 setelah langkah 1; langkah 5 setelah langkah 3.

**Milestone 2: Kontrak, daemon, CLI, dan bukti offline** (after milestone 1; needs fitur 6, fitur 8 build, fitur 10 milestone 4, fitur 11 langkah 5 (`secret.reveal` lewat daemon) dan langkah 8 (CLI secret), dan fitur 14 di `01-foundation.md`; fitur 22 milestone 2 (`resolve.rs`, `inspect`) dan usulan spec 0022 nomor 7; spec 0020 milestone 2 untuk `socketPath`)

6. Protokol: DTO, dua method di tabel `methods!` (`recovery refused`), enum terbuka, kode `instance.data_usage_unavailable` dan kode `cli` baru dengan hint statis, fixture termasuk fixture triple `detail-*.json`, dengan triple keempat `detail-package-unreadable.json` (2026-10-22: summary instance PostgreSQL `stopped`, detail dengan `packageReadable` false, `displayName` null, `connections` kosong, dan secret `superuserPassword` dengan judul nama field itu, `dataUsage` null) dan `packageReadable` true di tiga triple lain, kenaikan minor, `cargo xtask codegen`, dan test kontrak Swift dengan konsistensi `ConnectionString`. *Tambahan dari spec 0068 (2026-10-21):* kasus fixture tampilan untuk start yang gagal karena konflik port (*Kasus yang menunggu DTO* spec 0068) di commit ini kalau `Fixtures/` spec 0068 sudah di `main`, selain itu milestone 1 spec 0068 yang menambahkannya (spec 0068 *Key invariants*). Memenuhi **AC-17**.
7. `stacklyd`: field `package_readable` di `stackly_core::instance::detail::DetailProvider` yang diisi `detail::build` (milestone 1 sudah di `main`, jadi langkah ini yang menambahkan nya, dengan test core nya; pemetaan DTO menyalin nilai itu apa adanya, tidak menghitung ulang; 2026-10-22), handler kedua method, cache paket pinned dengan cek stat, cache, penggabungan, dan semaphore scan data usage (urutan jawaban, kesegaran lewat `Clock`, pemetaan error, dan batas di `DaemonConfig` menurut *Ukuran data*), penghitung akses `CatalogState` untuk test, dan integration test di dalam proses (detail, data usage, offline dengan binary CLI asli, AC-012 bagian daemon, sentinel bagian daemon). Memenuhi **AC-4**, **AC-5**, **AC-8**, **AC-10**, **AC-18**, **AC-19**.
8. CLI: `stackly instance connection` dan perluasan `inspect` (`--size`, ambil ulang saat revision beda), termasuk perilaku `packageReadable` false di *CLI* (2026-10-22), dengan golden, test paritas, dan sentinel bagian CLI. Memenuhi **AC-6**, **AC-7**, **AC-8**, **AC-18**.

Sesi paralel: setelah langkah 6 masuk, langkah 7 dan 8 bisa menjadi dua sesi (daemon dan CLI), dengan test CLI terhadap daemon nyata menunggu langkah 7 di merge.

**Milestone 3: Halaman detail dan tab Summary** (after langkah 6; needs fitur 12 dan fitur 13 milestone 4 (`ui-macos.md`), fitur 11 langkah 10 (`RevealableSecretText` dengan usulan spec 0011, `ConcealedPasteboard`, `SecretEntrySheet`), fitur 22 milestone 3 (Instances, inspector, `InstanceActionModel`, `ShellNavigator`) dan usulan spec 0022 nomor 1 sampai 6; needs fitur 67 milestone 1 dan 3 di `ui-macos.md`; memakai peta perintah fitur 69 kalau sudah ada)

9. `StacklyClient` method baru, `ShellSync.events(kindPrefix:)` (usulan spec 0013), `ConnectionComposer` dengan fixture komposisi, `DetailFormat`, `ProcessText`, dan `ReadinessText` dengan fixture detail (kalau langkah 7 spec 0022 di merge lebih dulu, ia sudah membangun `ReadinessText` dengan aturan dan lokasi spec ini, dan langkah ini memakainya ulang), dan `InstanceDetailModel` dengan service tiruan; `DetailFormat.connectionUnavailableText` (2026-10-22). Memenuhi **AC-3**, **AC-7**, **AC-9**, **AC-14**.
10. `InstanceDetailPage`, navigasi `.instance(id)` lewat `instanceRequest`, header, toolbar, kartu Summary (termasuk kartu Connection dan alasan Copy Connection String saat `provider.packageReadable` false, 2026-10-22), banner, state, tambahan inspector (tombol Open adalah action `openInstance` yang dirender `ActionButton`), dan sentinel bagian Swift, dengan test dan preview, termasuk `#Preview` lebar sempit 640 pt sebagai cek teks besar spec 0012, dan `#Preview` serta varian snapshot 384 pt untuk kolom halaman dengan inspector terbuka (2026-10-21). *Tambahan dari spec 0067, 0068, dan 0069 (2026-10-21):* kartu dari `CardSection`, `.labeledContentStyle(.stackly)`, dan `CopyButton`, toolbar dari `ActionButton` dan `ActionMenu` (menu More dengan satu bagian yang berisi Copy Connection String saja; item "Duplicate…" sesudahnya di bagian yang sama dibangun milestone 3 spec 0024, 2026-10-22); hook `onReselect` dan ketukan baris sidebar yang sudah terpilih (kalau `AppShell.select(_:)` dan hook `onReselect` belum ada karena milestone 2 spec 0069 belum di merge, langkah ini membuatnya menurut spec 0069 AC-7, dan langkah 4 spec 0069 memakainya ulang); penerbitan `openInstance`, `back`, dan `copyConnectionString` kalau milestone 2 spec 0069 sudah di merge (selain itu langkah 6 spec 0069 yang menerbitkannya); suite `InstanceDetailSnapshotTests` di atas harness spec 0068 untuk setiap state halaman dan kartu kalau milestone 1 spec 0068 sudah di merge, selain itu fitur 68 yang menulisnya (spec 0068 *Key invariants*). Memenuhi **AC-8**, **AC-11** sampai **AC-15**, **AC-18**.
11. Evidence manual interaksi desktop untuk alur Start sampai Copy dengan daemon development. Memenuhi **AC-16**.

Sesi paralel: milestone 3 bisa berjalan bersamaan dengan langkah 7 dan 8 karena hanya bergantung pada fixture langkah 6.

**Milestone 4: PostgreSQL dan NATS nyata, dan offline di Mac ini** (after milestone 2 dan 3; needs fitur 19 milestone 3 langkah 9 (PostgreSQL sebagai `[[provider]]` di `providers/catalog.toml`, keputusan lead D6; test daemon sebelum itu boleh memakai catalog `CatalogBuilder` yang memuat `official/postgresql`), milestone endpoint `Auto` fitur 23 (milestone 3 di scope, langkah 13 dan 14 spec 0023: start NATS dari CLI atau GUI dengan endpoint default `Auto` baru bekerja dengan paket NATS `1.0.1`; sebelumnya ditulis "fitur 23 milestone 2", diubah 2026-10-22), fitur 18 milestone 2 (`install_cached_artifact`), dan spec 0020 milestone 2 dan 3 (create lewat daemon dan GUI); artefak di cache runtime xtask, selain itu `BLOCKED`)

12. Test native: PostgreSQL 18 dan NATS di start dari CLI dan GUI sampai `ready`, `psql` milik runtime menjalankan `SELECT 1` dengan string `uri` dan `socket` hasil komposisi, dan klien TCP menerima `INFO` NATS lewat string `uri`. Memenuhi **AC-20**.
13. Run offline dengan jaringan Mac ini dimatikan setelah persetujuan user (start, status, Summary, `connection`, `inspect`), dicatat evidence `native`; `BLOCKED` sampai disetujui. Memenuhi **AC-10**.

## Consequences

**Positive**:

- JOURNEY-01 selesai dari GUI dan CLI dengan password yang tidak pernah tampil atau tersalin tanpa diminta.
- Provider berikutnya (Valkey, MySQL, ClickHouse, Traefik) cukup menulis blok `connection` di manifest untuk mendapat tombol Copy dan `stackly instance connection`.
- Connection dirender dengan `PlanContext` dan resolver yang sama dengan plan, jadi tidak pernah menunjuk host, port, atau socket yang salah.
- Semua jalur lokal terbukti tidak menyentuh catalog atau jaringan.

**Negative / tradeoffs**:

- Hint dan teks `package_unavailable` menyuruh menjalankan `stackly doctor`, yang baru dibangun fitur 34; sampai itu, perintah tersebut belum ada. Teks ini sudah ada sebelum update 2026-10-22, tetapi sekarang menjadi jalur utama untuk paket yang tidak terbaca. Start untuk keadaan yang sama gagal `supervisor.package_unavailable` (kategori `failed`, exit 7), sedangkan `stackly instance connection` keluar 3 (`conflict`); kedua nya sengaja berbeda karena yang satu operasi yang gagal dan yang lain entri yang tidak tersedia.
- Detail dimuat ulang hanya saat `revision` berubah atau saat `secret.changed`, jadi kartu Connection tetap "can’t read" sesudah paket diperbaiki sampai halaman dibuka lagi; memperbaiki paket tidak mengubah revision instance (*2026-10-22*).
- Paket pinned yang tidak terbaca membuat kartu Connection hanya menyebut sebab nya; nama entri connection tidak diketahui tanpa manifest, jadi CLI tidak bisa menyebut entri yang diminta selain lewat `--format` yang dikirim pengguna, dan `--all` tidak punya baris per entri (*2026-10-22*).
- Blok `connection` mengubah `manifest_sha256` paket PostgreSQL dan NATS, jadi blok itu harus masuk sebelum evidence BR-025 kedua provider; instance yang di pin ke paket lama tidak punya connection sampai fitur 59.
- Encoder URI ada tiga kali (core, protokol untuk CLI, dan Swift); fixture bersama menjaganya sama, tetapi setiap perubahan aturan encode menyentuh tiga tempat.
- Template connection hanya bisa memakai endpoint, identitas instance, provider, runtime, dan secret; nilai settings (misalnya nama database yang bisa diganti) butuh update spec ini kalau provider membutuhkannya.
- Ukuran data hanya dihitung saat diminta, jadi inspector menampilkan "—" sampai halaman detail atau `--size` pernah menghitungnya.
- Folder data yang lebih dalam dari 32 tingkat hanya mendapat "at least {ukuran}", karena batas kedalaman menjaga jumlah file descriptor daemon; folder data PostgreSQL, NATS, dan provider PRD lain jauh lebih dangkal.
- `stackly instance inspect --json` berubah bentuk menjadi object pembungkus, jadi usulan spec 0022 harus masuk sebelum fitur 22 dibangun supaya tidak menjadi perubahan breaking.
- Bukti offline native butuh jaringan Mac ini dimatikan, langkah manual yang menunggu persetujuan user.
- Restarts sejak dibuat dan latensi probe tidak ada sampai fitur 28, dan detail belum dimuat ulang untuk perubahan config atau environment sampai fitur 29 dan 44 menambah pemicunya.

**Neutral**:

- Dua method baru, satu kode wire, dua kode `cli`, dua kode issue manifest, satu field manifest, satu method `Filesystem`, dan dua kenaikan minor protokol (langkah 1 untuk kunci wire `reason` di item `issue`, dan langkah 6).
- Folder Swift baru `InstanceDetail/`, modul core `instance::connection` dan `instance::detail`.

## Follow-up

- [x] Lead: bawa *Usulan update spec lain* ke pemiliknya: spec 0004, 0009, dan 0011 lewat program foundation, spec 0019, 0022, dan 0023 lewat sesi architect fase 02, sebelum milestone 1 dan 2; blok `connection` paket masuk sebelum evidence BR-025 fitur 19 dan 23. Diterapkan 2026-10-20 (penanda di setiap blok).
- [ ] Lead: bawa usulan spec 0013 (`ShellSync.events(kindPrefix:)`) ke pemilik spec 0013 sebelum langkah 9.
- [ ] Lead: catat *Selisih mockup yang disengaja* di `ui-macos.md` lewat `/scope`.
- [ ] Fitur 26, 27, 28, 29, 35, 49, dan 54: tambah tab dan tombol masing masing ke halaman detail lewat `InstanceTab` dan toolbar spec ini.
- [ ] Fitur 24: item More "Duplicate…" sesudah Copy Connection String, tanpa tab, seperti dirancang spec 0024 (AC-13, *Titik masuk*) dan dibangun milestone 3 nya.
- [ ] Fitur 28: jumlah restart sejak dibuat dan latensi probe di baris Readiness dan Restart attempts.
- [ ] Fitur 29 dan 44 sampai 47: muat ulang detail saat config revision, environment, atau lease berubah (perubahan itu tidak menaikkan `revision` instance), dan isi Dependents dari environment nyata.
- [ ] Fitur 33: banner crash loop yang lebih kaya di atas banner I1 spec ini.
- [ ] Fitur 39: credential NATS di template `uri` NATS.
- [ ] Fitur 59: instance yang di pin ke paket tanpa blok `connection` mendapat connection setelah update provider revision.
- [x] Lead (temuan F21 milestone 4, dicatat 2026-10-22): `stackly instance start` dan `stop` hanya menerima ID penuh (`InstanceArg`, pesan "expected a whole instance id"), sedangkan `connection`, `inspect`, dan `get` menerima nama atau ID lewat resolver `TargetArg` spec 0022. Catatan verify milestone 4 menyebut `get` juga, tetapi di `main` `9df6beee` `get` sudah alias `inspect` dengan `TargetArg` (langkah 5 spec 0022), jadi yang tersisa hanya `start` dan `stop`. Alur journey di spec ini menulis `stackly instance start <instance> --wait` lalu `stackly instance connection <instance>`, jadi pengguna wajar mengetik nama di keduanya. Usulan: pemilik spec 0022 (pemilik resolver) menambah satu langkah kecil yang memakai `TargetArg` di `StartArgs` dan `StopArgs` (`apps/cli/src/commands/instance/action.rs`), dengan golden dan test CLI untuk nama, nama yang tidak dikenal (`cli.instance_not_found`), dan UUID; tanpa perubahan kontrak atau kode error. AC spec ini tidak berubah; AC baru untuk start dan stop dengan nama (draf AC-15) ada di spec 0022 bila langkah 5a dijadwalkan, dan AC-6 spec 0014 tetap. Runner up: langkah susulan fitur 21, karena journey CLI nya ada di spec ini. Dicatat di spec 0022 sebagai Follow-up terbuka "`stackly instance start` dan `stop` dengan nama" dengan langkah 5a di milestone 2 dan draf AC-15 yang menunggu persetujuan user (2026-10-22); pekerjaan build nya dilacak di sana.
- [ ] `/sync` setelah build: catat `instance.get_detail`, `instance.get_data_usage`, blok `connection` manifest, `stackly instance connection`, dan folder `InstanceDetail/` di `AGENTS.md` root dan `apps/macos/AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, dan hasil cross check ada di [rationale.md](rationale.md).
