# 0025. PostgreSQL 17 dan 18 berdampingan

**Date**: 2026-10-22
**Status**: In Progress

## Summary

Spec ini menutup JOURNEY-03: kamu memasang runtime PostgreSQL 18 di Mac yang sudah menjalankan dua instance PostgreSQL 17, membuat instance 18 baru di sampingnya, lalu membandingkan versinya, tanpa satu byte pun data instance lama berubah. Hampir semua bagiannya sudah ada di fitur 18 sampai 24, jadi fitur ini terutama membuktikan gabungannya pada PostgreSQL asli: tiga cluster dengan sentinel (nilai penanda unik) berbeda yang tidak pernah saling terbaca, juga setelah tiap instance dihentikan lalu dijalankan lagi. Kode barunya kecil: satu fungsi core untuk memeriksa kecocokan data dan runtime yang dipakai start dan pemeriksaan rebind, pemeriksaan rebind yang hanya membaca (tanpa operasi dan tanpa method IPC, yang tetap milik fitur 58), dan hint (petunjuk statis di error) yang mengatakan bahwa data mayor lain butuh migrasi. Tidak ada perubahan kontrak IPC.

## Requirements

**User stories**:

- Sebagai developer yang sudah punya `pg17-dev` dan `pg17-test`, saya ingin memasang PostgreSQL 18 dan membuat `pg18-dev` di sampingnya, supaya saya bisa mencoba versi baru tanpa mengganggu database yang sedang saya pakai.
- Sebagai developer, saya ingin yakin bahwa memasang runtime baru tidak pernah memindahkan, mengubah, atau me restart instance lama, supaya install bukan operasi yang berisiko bagi data saya.
- Sebagai developer, saya ingin Stackly menolak menjalankan binary mayor lain pada data saya dengan pesan yang bilang data itu butuh migrasi, supaya saya tidak merusak data dengan sekadar mengganti binary.
- Sebagai developer fitur 58, saya ingin pemeriksaan rebind yang sudah teruji pada data PostgreSQL asli, supaya operasi rebind tinggal membungkusnya dengan stop, kebijakan backup, dan verifikasi start.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-035):

- **AC-1** (REQ-043 kalimat pertama, JOURNEY-03, BR-001): Install runtime (`runtime.install`, operasi `install_runtime` spec 0018) saat sudah ada instance dari software yang sama tidak mengubah instance itu. Untuk setiap instance yang ada, field registry yang terlihat di `instance.get` (`runtimeId`, `provider`, `revision`, `lifecycle`, `desired`, setiap endpoint dengan `port`, `portMode`, dan `bound`) sama sebelum dan sesudah install, dan himpunan instance di `instance.list` tidak berubah. Untuk instance yang `stopped`, digest seluruh pohon `instances/<id>/` dan `logs/instances/<id>/` (SHA-256 dan mode setiap file biasa, dengan path relatif) sama byte demi byte. Instance yang `running` tetap `running` dan `ready` sebelum dan sesudah install, dengan PID dan waktu start proses yang sama dan jumlah restart yang sama. Dibuktikan di fixture (instance fake, install lewat `FakeFetcher`) dan di native (dua PostgreSQL 17 dari spec 0024, `pg17-dev` `running` dan `pg17-test` di stop lebih dulu, lalu PostgreSQL 18 dipasang lewat `runtime.install` dari entri cache yang sudah terverifikasi, tanpa jaringan).
- **AC-2** (REQ-035, AC-035 bagian init, JOURNEY-03): Setelah AC-1, `pg18-dev` dibuat lewat `instance.create` biasa (spec 0020) dari runtime 18 yang baru dipasang, dengan password fixture dan port `Fixed` dari `allocate_port` (pola `harness::create_instance`), lalu di start. Ketiga instance `ready` pada saat yang sama. `instance.list` menampilkan `runtimeVersion` `17.11`, `17.11`, dan `18.6` (field ini hari ini diisi dari versi data saat init, jadi bukti versi server yang benar benar berjalan adalah `SHOW server_version` di bawah), dengan `runtimeId` kedua PostgreSQL 17 sama dan `runtimeId` `pg18-dev` berbeda. `SHOW server_version` lewat endpoint setiap instance (psql milik runtime instance itu sendiri) mengembalikan versi yang mayornya sama dengan line runtime nya (`17.x` untuk kedua PostgreSQL 17, `18.x` untuk `pg18-dev`), jadi endpoint baru memang mengarah ke cluster baru, dan `SentinelCluster::system_identifier` ketiganya berbeda. Isi `PG_VERSION` di folder data ketiganya `17`, `17`, dan `18`, dan `data.initializedLine` di `instance-manifest.json` sama dengan itu. Port ketiganya berbeda, dan setiap path di `InstancePaths` (termasuk folder run tempat socket) milik `pg18-dev` berbeda dari milik kedua PostgreSQL 17.
- **AC-3** (REQ-035, AC-035 bagian write dan restart, PRD AC-009 bagian satu mayor 18 yang melengkapi bagian dua PostgreSQL 17 milik spec 0024): Ketiga cluster mendapat sentinel berbeda lewat `SentinelCluster::write` (helper spec 0024 di `apps/daemon/tests/postgresql/sentinel.rs`), dan `SentinelCluster::assert_isolated` lolos untuk ketiganya. Lalu satu per satu, setiap instance di stop (`instance.stop`) sampai `stopped`, kedua instance lain tetap `ready` dan tetap membaca sentinel miliknya sendiri, instance itu di start lagi (`instance.start`) sampai `ready`, dan `assert_isolated` lolos lagi untuk ketiganya. Setelah ketiga siklus, `runtimeId` dan `provider` setiap instance sama dengan sebelum siklus (BR-001), dan `SHOW server_version` setiap instance masih cocok dengan line runtime nya sendiri. Restart sebagai satu operasi milik fitur 26 dan tidak dipakai di sini.
- **AC-4** (BR-009, AC-035 bagian penolakan, Done when "binary PG18 pada data PG17"): Dengan daemon berhenti, penulisan registry khusus test (`Site::bind_runtime`, *Penulisan registry khusus test*) mengikat `pg17-test` ke runtime 18 dan `pg18-dev` ke runtime 17. Setelah daemon jalan lagi, `instance.start` untuk masing masing berakhir dengan operasi `failed` (bukan `needs_recovery`) di tahap `verify`, sebelum `start_begin` dan sebelum spawn, dengan `supervisor.data_incompatible`, `outcome` `incompatible`, `data` `17` dan `runtime` `18` (untuk `pg17-test`) atau `data` `18` dan `runtime` `17` (untuk `pg18-dev`), dan `ErrorBody` kegagalannya (`failure.error` di operasi) membawa hint `incompatible` dari *Hint migrasi*. Kedua instance tetap `stopped` tanpa `process`, folder data nya tanpa `postmaster.pid`, `support::serving(data)` kosong, dan digest pohon `instances/<id>/` keduanya sama dengan sebelum penulisan registry. `pg17-dev`, yang postmaster nya tetap hidup selama daemon berhenti, diadopsi lagi dengan PID dan waktu start yang sama (`wait_instance` sampai `process` teradopsi dan `ready`, karena adopsi punya jeda not ready) lalu membaca sentinel nya. Setelah penulisan registry khusus test mengembalikan `runtimeId` asli dan daemon jalan lagi, kedua instance start sampai `ready` dan `assert_isolated` lolos untuk ketiganya. Bukti dasar penolakan dua arah dan penanda yang hilang atau berubah tetap milik spec 0019 AC-11; AC ini menambah hint, data yang utuh, tetangga yang tidak terganggu, dan pemulihan.
- **AC-5** (alasan butuh migrasi): `codes::supervisor::hint(code, details)` di `stackly-protocol` mengembalikan teks persis di *Hint migrasi*: teks `incompatible` untuk `supervisor.data_incompatible` dengan detail `outcome` `incompatible`, teks penanda untuk `outcome` `marker_missing`, `marker_mismatch`, dan `marker_unreadable`, dan `None` untuk `outcome` `unknown`, `outcome` yang tidak dikenal, atau kode lain. Kedua teks tidak memuat nilai apa pun (line, versi, atau path ada di message dan details), berbahasa Inggris tanpa `{`, dan paling panjang 200 byte (aturan *Hint* spec 0006, dicek test katalog area). `stacklyd` memasang hint ini pada `ErrorBody` kegagalan operasi berkode `supervisor.data_incompatible` lewat fallback hint `FailureError` spec 0006: fixture daemon membuktikan hint `marker_mismatch` (FAKE_VERSION diubah) dan hint `incompatible` (instance fake diikat ke runtime fake line lain). `cargo xtask codegen --check` bersih tanpa kenaikan `ProtocolVersion::CURRENT`, karena hint bukan bagian dari digest kontrak.
- **AC-6** (REQ-043, AC-043 bagian penolakan, BR-009): `stackly_core::instance::rebind::check(registry, platform, layout, id, target)` hanya membaca dan menjawab apakah data instance `id` boleh dijalankan runtime `target` menurut preflight spec 0009, dengan urutan penolakan di *Pemeriksaan rebind*. Untuk instance dengan data line `17` (registry dan `PG_VERSION` berisi `17\n`) dan runtime target ber line `18` dari paket `providers/postgresql` asli, hasilnya `supervisor.data_incompatible` dengan `outcome` `incompatible`, `data` `17`, dan `runtime` `18`, dan `codes::supervisor::hint` atas kode dan details error itu memberi teks `incompatible` (alasan butuh migrasi); arah sebaliknya sama. Runtime target dengan line yang sama (`17` ke runtime `17` lain) menghasilkan `Compatible`; runtime yang sama dengan runtime instance menghasilkan `Current`; paket dengan `lineCompatibility` yang mengizinkan pasangan itu menghasilkan `Compatible`. `PG_VERSION` yang hilang, berubah menjadi `16`, atau tidak terbaca menghasilkan `outcome` `marker_missing`, `marker_mismatch`, atau `marker_unreadable`. Setiap jalur, termasuk yang berhasil, tidak menulis apa pun: `FakeRegistry` tanpa mutasi, `FakeFilesystem` tanpa mutasi, tidak ada operasi yang dibuat, dan `FakeProcessSupervision` tidak mencatat spawn.
- **AC-7** (AC-043, REQ-043): Pada data PostgreSQL asli dari AC-2 dan AC-3, dengan `pg17-test` dan `pg18-dev` `stopped`, daemon berhenti, dan registry `TestRoot` dibuka test (pola `Site::open_package` di harness), `check` untuk `pg17-test` ke runtime 18 dan untuk `pg18-dev` ke runtime 17 menghasilkan `supervisor.data_incompatible` `outcome` `incompatible` dengan line yang benar dan hint `incompatible` lewat `codes::supervisor::hint`, dan `check` untuk `pg17-test` ke runtime nya sendiri menghasilkan `Current`. Sesudahnya `Instance` dan `Runtime` semua baris registry sama (`PartialEq`, termasuk `revision`) dan generasi baris tidak berubah, `instance-manifest.json` ketiga instance sama byte demi byte, dan digest pohon `instances/<id>/` `pg17-test` dan `pg18-dev` (yang berhenti) sama byte demi byte; `pg17-dev` yang tetap berjalan dibuktikan lewat proses yang sama setelah adopsi. Setelah daemon jalan lagi ketiga instance `ready` dengan `assert_isolated` lolos (konfigurasi sumber dan data tetap utuh). Bagian AC-043 tentang aksi pengguna (method, CLI, GUI, stop terkontrol, dan arahan ke backup dan restore) milik fitur 58.
- **AC-8** (satu sumber kebenaran): Start (`verify` di `core::supervisor::startup`) dan `rebind::check` sama sama memanggil `stackly_core::instance::data_compat::check_data_compatibility`, satu fungsi yang membaca penanda data dan memanggil `ValidatedPackage::preflight`. Test tabel memberi input yang sama ke kedua jalur untuk keempat hasil yang bisa terjadi dengan line tercatat (`incompatible`, `marker_mismatch`, `marker_missing`, `marker_unreadable`) dan mendapat kode, `outcome`, dan details yang sama; `unknown` hanya muncul tanpa line tercatat (adoption fitur 57) dan tidak bisa dicapai lewat fungsi ini. Test start yang sudah ada di `crates/core/tests/supervisor_start/` (termasuk `a_start_checks_the_package_the_folder_the_marker_and_the_runtime_first`) tetap lulus tanpa mengubah harapannya, dan kasus start baru untuk `incompatible`, `marker_missing`, dan `marker_unreadable` ditambah di sana.
- **AC-9** (evidence, spec 0002): Setiap test native fitur ini memasang guard evidence `native` dengan `covers` ID PRD yang dibuktikannya (tabel di *Critical test scenarios*; hanya ID berpola `AC`, `REQ`, `NFR`, `VAL`, atau `BR` yang diterima `stackly-evidence`, jadi JOURNEY-03 disebut di teks `expected`, bukan di `covers`) dan subject untuk kedua runtime (`{ provider: postgresql, version, digest, source: managed }`, `version` dari `SHOW server_version`, `digest` dari `artifact_sha256` artefak). Teks `expected` test yang membuktikan AC-043 menyebut bahwa buktinya bagian (pemeriksaan core pada data asli; aksi pengguna milik fitur 58). Artefak yang tidak ada di cache runtime xtask membuat test `BLOCKED` dengan alasan `artifact <id> is not in the runtime cache` (nama setiap artefak yang hilang) dan owner `@ojiepermana`, tanpa unduhan. Test fixture menulis evidence `fixture`, dan lulus di fixture bukan evidence native.
- **AC-10** (JOURNEY-03 di aplikasi, manual): Di Mac ini, dengan root dev terpisah: pasang PostgreSQL 17 lewat tombol Get, buat dan jalankan `pg17-dev`, lalu pasang PostgreSQL 18 lewat Get saat `pg17-dev` berjalan; `pg17-dev` tetap Ready di halaman Instances selama install. Sheet New Instance menawarkan runtime 17.11 dan 18.6, `pg18-dev` dibuat dari 18.6 dan di start, kolom Version halaman Instances menampilkan `17.11` dan `18.6` berdampingan, tab Summary `pg18-dev` menampilkan endpoint dan connection nya sendiri, dan connection yang disalin terhubung lewat `psql` runtime 18 dengan `SELECT version()` berisi `18.6`. Hasilnya dicatat sebagai evidence manual `native` dengan `covers` REQ-035 dan REQ-043 dan JOURNEY-03 di `expected`; tanpa artefak di cache atau tanpa persetujuan unduhan, `BLOCKED`.

Status test semua AC mulai dari `NOT_RUN`. Lulus di fixture bukan evidence native, dan artefak, sesi GUI, atau persetujuan unduhan yang tidak tersedia dicatat `BLOCKED`.

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md): AC-043 dibuktikan lewat pemeriksaan rebind di core yang hanya membaca, berbagi satu fungsi kecocokan data dengan start, dan diuji pada data PostgreSQL asli, tanpa operasi, method IPC, atau CLI (semuanya tetap milik fitur 58); alasan butuh migrasi menjadi hint statis pada kode `supervisor.data_incompatible` yang sudah ada, tanpa kode baru dan tanpa perubahan kontrak; bukti tiga cluster dibangun di atas skenario dua PostgreSQL 17 dan `SentinelCluster` milik spec 0024, dengan runtime 18 dipasang lewat `runtime.install` sungguhan setelah kedua instance 17 ada; dan "install tidak menyentuh instance" dibuktikan di fixture dan native dengan snapshot registry, digest pohon, dan identitas proses.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Skill async relevan hanya untuk test daemon yang menunggu operasi dan readiness; kode core di sini sync dan dipanggil lewat `spawn_blocking` oleh pemanggilnya.

## Feature design

### Batas fitur

- **Di dalam**: fungsi `check_data_compatibility` yang dipakai bersama, `instance::rebind::check`, hint `supervisor.data_incompatible`, test fixture dan native yang membuktikan JOURNEY-03 dan penolakan mayor, dan cek manual JOURNEY-03 di aplikasi.
- **Di luar**: operasi `rebind_runtime`, method IPC, CLI, dan GUI rebind, stop terkontrol sebelum rebind, kebijakan backup, dan verifikasi start sesudah rebind (fitur 58); migrasi data mayor dan `pg_upgrade` (Deferred, PRD 22.1); backup dan restore logical, termasuk bagian AC-035 untuk keduanya (fitur 51 dan 52); restart sebagai satu operasi (fitur 26); duplicate config, termasuk duplicate lintas mayor yang tidak mungkin karena spec 0024 mengunci duplicate ke runtime sumbernya; skenario dua PostgreSQL 17 dari runtime yang sama (spec 0024); penolakan start dua arah dan penanda hilang atau berubah sebagai bukti dasar (spec 0019 AC-11); install dari cache dan evidence kompatibilitas per artefak (spec 0018 milestone 5, spec 0019 langkah 11).

### Alur JOURNEY-03

```text
pg17-dev (running, ready)   pg17-test (stopped)        runtime postgresql 17.11
        │                          │
        ▼                          ▼
runtime.install postgresql@18 (dari cache) ──► runtime postgresql 18.6 baru
        │   pg17-dev: PID sama, tetap ready; pg17-test: digest pohon sama
        ▼
instance.create pg18-dev --runtime 18.6 ──► instance.start ──► ready
        │
        ▼
bandingkan: runtimeVersion 17.11 / 17.11 / 18.6, SHOW server_version per endpoint
        │
        ▼
sentinel berbeda di tiga cluster ──► stop lalu start satu per satu ──► tetap terisolasi
```

### Kecocokan data (dipakai start dan rebind)

Modul baru `stackly_core::instance::data_compat`:

```rust
/// What `preflight` said when it did not answer `Compatible`, as the wire
/// carries it in `supervisor.data_incompatible`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DataIncompatible {
    pub outcome: DataOutcome,
    pub data: Option<RuntimeLine>,   // hanya untuk Incompatible, seperti hari ini
    pub runtime: RuntimeLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DataOutcome { Incompatible, MarkerMismatch, MarkerMissing, MarkerUnreadable, Unknown }
// as_code(): "incompatible", "marker_mismatch", "marker_missing", "marker_unreadable", "unknown"

/// Reads the data marker of `paths` (when the package declares one) and
/// asks `package.preflight` whether data of `recorded` may run on `runtime`.
pub fn check_data_compatibility(
    fs: &dyn Filesystem,
    package: &ValidatedPackage,
    paths: &InstancePaths,
    recorded: &RuntimeLine,
    runtime: &RuntimeLine,
) -> Result<(), DataIncompatible>;
```

- Isinya persis bagian penanda di `verify` hari ini: `data_marker_path`, `read_file_limited` dengan `DATA_MARKER_MAX_LEN`, `MarkerRead::from_read`, lalu `preflight(Some(recorded), marker, runtime)`. `verify` memanggil fungsi ini lewat `From<DataIncompatible> for SupervisorError` yang menghasilkan `SupervisorError::DataIncompatible` dengan teks `outcome`, `data`, dan `runtime` yang sama dengan hari ini, jadi pesan, details, dan test spec 0010 dan 0019 tidak berubah.
- `DataOutcome::Unknown` tetap ada supaya pemetaan dari `PreflightOutcome` lengkap, tetapi fungsi ini selalu memberi line tercatat, jadi `Unknown` tidak bisa terjadi lewat fungsi ini; pemanggil tanpa line tercatat (adoption fitur 57) memutuskan sendiri jalurnya.
- Tipe `fs` mengikuti yang dipakai `verify` sekarang (`deps.platform.fs()`); kalau bentuknya lain, ikuti bentuk itu, bukan bentuk di atas.

### Pemeriksaan rebind

Modul baru `stackly_core::instance::rebind`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RebindVerdict {
    /// The runtime is the one the instance runs on now.
    Current,
    /// The data may run on the runtime.
    Compatible { runtime: RuntimeId, line: RuntimeLine },
}

/// Answers whether instance `id` could move to runtime `target`, reading the
/// registry, the pinned package, and the data marker, and writing nothing.
/// Feature 58 calls it as the precheck of its operation and again after the
/// controlled stop, right before it mutates anything.
pub fn check(
    registry: &dyn Registry,
    platform: &Platform,
    layout: &PreparedLayout,
    id: InstanceId,
    target: RuntimeId,
) -> Result<RebindVerdict, RebindCheckError>;
```

Urutan pemeriksaan, penolakan pertama yang menang:

| No | Pemeriksaan | Hasil kalau gagal | Kode dan details |
| --- | --- | --- | --- |
| 1 | instance ada di registry | `RebindCheckError::Supervisor(NotFound)` | `resource.not_found` (sama dengan start) |
| 2 | lifecycle bukan `retained` | `Supervisor(InstanceRetained)` | `supervisor.instance_retained` |
| 3 | data sudah diinisialisasi (`data_initialized_at` terisi) | `Supervisor(NotInitialized)` | `supervisor.not_initialized` |
| 4 | runtime target ada di registry | `RuntimeNotFound` | `instance.runtime_not_found`, `runtimeId` |
| 5 | runtime target sama dengan runtime instance | jawaban `Current`, berhenti di sini | |
| 6 | software runtime target sama dengan software instance | `RuntimeMismatch` | `instance.runtime_mismatch`, `runtimeSoftware`, `providerSoftware` (software instance) |
| 7 | runtime target dan dependency nya `installed` | `Supervisor(RuntimeUnavailable)` | `supervisor.runtime_unavailable`, `runtimeId`, `state` |
| 8 | paket pinned instance ada, valid, dan digest nya cocok | `Supervisor(PackageUnavailable)` | `supervisor.package_unavailable`, `provider`, `reason` |
| 9 | folder instance privat dan milik user | `Supervisor(DataUnavailable)` | `supervisor.data_unavailable`, `path`, `reason` |
| 10 | `check_data_compatibility(fs, package, paths, instance.data_line(), target.line())` | `Supervisor(DataIncompatible)` | `supervisor.data_incompatible`, `outcome`, `data`, `runtime`, plus hint di daemon |
| 11 | semua lolos | jawaban `Compatible { runtime, line }` | |

- `RebindCheckError` memakai thiserror, `#[non_exhaustive]`, dan mengimplementasikan trait `OperationError` (`code()`, `retryable()` false, `details()`) seperti `SupervisorError`, supaya fitur 58 bisa memakainya langsung sebagai kegagalan operasi dan supaya hint bisa dipilih dari kode dan details nya. Varian `Supervisor(SupervisorError)` meneruskan kode dan details error jalur start apa adanya, dan `RuntimeNotFound` serta `RuntimeMismatch` memakai kode `instance.runtime_not_found` dan `instance.runtime_mismatch` sebagai string literal lewat `ErrorCodeName::from_static`, cara `CreateError::code` di `core::instance::create` (core tidak bergantung pada `stackly-protocol`). Kedua kode sudah ada di katalog area `instance`; tidak ada kode baru.
- Pemeriksaan 7, 8, dan 9 memakai ulang pembantu `supervisor::prepare` (`require_installed`, `load_package`, `check_instance_folder`). Deklarasi `mod prepare;` di `core::supervisor` menjadi `pub(crate) mod prepare;` dan ketiga fungsi itu `pub(crate)`. Paket yang dipakai adalah paket instance (bukan paket lain untuk runtime target), karena `LineCompatibility` dan `dataMarker` milik provider revision instance.
- Lifecycle `running`, `stopped`, `failed`, atau `unknown` tidak ditolak: pemeriksaan hanya membaca `PG_VERSION`, yang tidak berubah selama server berjalan, dan stop terkontrol adalah langkah fitur 58. Pemeriksaan tidak menyentuh folder runtime target; validasi executable terjadi saat start (spec 0010) di verifikasi start fitur 58.
- Tidak ada lock, operasi, event, atau tulis. Jawabannya snapshot saat itu, jadi fitur 58 sebaiknya memanggilnya lagi di dalam operasinya setelah stop dan sebelum `Instance::rebind` (keputusan akhirnya milik spec fitur 58).
- `Instance::rebind` di domain (spec 0004) tetap penjaga terakhir saat mutasi: untuk paket yang sama, line yang lolos langkah 10 juga lolos `LineCompatibility::allows` di domain, karena keduanya membaca peta yang sama dari manifest.

### Hint migrasi

Fungsi baru `codes::supervisor::hint(code: &str, details: &[ErrorDetail]) -> Option<&'static str>` di blok `@functions` area `supervisor` (pola `catalog::hint` dan `storage::hint`). Teksnya persis:

| `outcome` | Hint |
| --- | --- |
| `incompatible` | `This data belongs to another major version, so this runtime never runs it. Moving it needs a migration: create an instance on this runtime and move the data with a dump and restore.` (181 byte) |
| `marker_missing`, `marker_mismatch`, `marker_unreadable` | `The version marker in the data folder is missing, unreadable, or not what Stackly recorded, so Stackly can’t tell which runtime may run this data and won’t run it until the marker matches.` (192 byte, apostrof lengkung seperti hint area lain) |
| `unknown`, nilai lain | tidak ada hint |

- Test katalog area `supervisor` memanggil `hint` untuk setiap nilai `outcome` di atas dan memeriksa panjang paling banyak 200 byte, bahasa Inggris, dan tanpa `{` (aturan *Hint* spec 0006).
- `stacklyd` memasang hint ini lewat fallback hint `FailureError` spec 0006 (*Hint*): untuk kegagalan operasi yang hint tersimpannya kosong, pemetaan memanggil fungsi `hint` area pemilik kode. Fallback itu dibangun pemilik area pertama yang butuh, langkah 11 spec 0018; kalau belum ada di `main` saat langkah 4, langkah 4 yang membangunnya dengan bentuk spec 0006 di `apps/daemon/src/ipc/operation/dto.rs` (`failure_body`) dan menambah `supervisor` ke pemilihnya. Start tidak pernah menolak `supervisor.data_incompatible` di precheck sinkron (precheck start hanya memeriksa lifecycle), jadi jalur `catalog_body` (yang sudah berakhir di `with_secret_hint`) tidak diubah di fitur ini; fitur 58 memasang hint yang sama kalau precheck rebind nya menolak sinkron. CLI menampilkannya lewat jalur hint yang sudah ada (spec 0008), dan GUI lewat penyajian error yang sudah ada.
- Hint tidak masuk `protocols/error-codes.json` dan tidak ikut digest kontrak, jadi tidak ada kenaikan protocol. `ErrorBody.hint` sudah ada di wire.
- Teks `incompatible` sengaja menyebut "dump and restore", bukan fitur backup Stackly, karena backup dan restore baru datang di fase 09; fitur 51 atau 58 boleh mengubah teksnya menjadi arahan ke backup dan restore Stackly (Follow-up).

### Lingkungan test native

Test native tinggal di modul baru `apps/daemon/tests/postgresql/side_by_side.rs` di binary test `apps/daemon/tests/postgresql.rs` dan memakai harness `postgresql/harness.rs` (milestone 2 spec 0019). Tambahan harness yang dibangun langkah 6:

- **Daemon tanpa jaringan dengan catalog repo**: varian `Site::in_process` yang juga menerima platform, dipanggil dengan `site.platform.clone().with_fetcher(Arc::new(FakeFetcher::new()))` tanpa respons yang didaftarkan, sehingga setiap upaya unduhan gagal dan test ikut gagal, dan dengan `DaemonConfig.catalog = Some(CatalogOverride::Location { location: Some(CatalogLocation::Development(<repo>/providers kanonik)), options })` supaya gerbang catalog `runtime.install` melihat `official/postgresql` sebagai `[[provider]]` (langkah 9 spec 0019).
- **Entri cache**: file dari `harness::cached_artifact(artifact_sha256(PG18))` disalin ke `site.prepared.layout().downloads_dir()/<sha256>` dengan mode 0600 sebelum `runtime.install`, ditambah file pendamping kalau langkah `download` spec 0018 saat itu mensyaratkannya; kalau langkah 16 spec 0018 sudah punya helper untuk ini, pakai helper itu. Langkah `download` lalu memakai entri terverifikasi apa adanya (spec 0018 AC-6).
- **ID dan folder runtime 18**: operasi install ditunggu sampai `succeeded`, ID runtime diambil dari `result.resources[0].id` operasi (atau `RuntimeSummary.id` kalau balasannya `alreadyInstalled`), dan folder nya dari `runtime.get` (`RuntimeDetail.path`).
- **Create di runtime lain**: varian `create_params` dan `create_with` yang menerima runtime ID; `pg18-dev` memakai runtime 18.
- **`Psql` runtime lain**: konstruktor `Psql` untuk folder runtime yang diberikan (misalnya `Site::psql_in(runtime_dir, id, port)`), karena `Site::psql` selalu memakai runtime milik `Site` (17).
- **Digest pohon**: helper yang mengembalikan peta path relatif ke (SHA-256, mode) untuk setiap file biasa di sebuah folder, dengan folder yang tidak ada dihitung sebagai peta kosong (`logs/instances/<id>/` bisa belum ada).
- **`three_postgresql_clusters(ev)`**: memeriksa kedua artefak di cache lebih dulu (pola `install.rs`, `BLOCKED` dengan nama artefak yang hilang), lalu `Site::open(ev, PG17)`, daemon in process, `two_postgresql_17_instances` (spec 0024), `pg17-test` di stop, install 18, create dan start `pg18-dev`, dan mengembalikan struct berisi `Site`, `InProcess`, `Api`, `TwoPostgres17`, `Made` dan `SentinelCluster` `pg18-dev`, ID dan folder runtime 18, serta snapshot sebelum dan sesudah install untuk AC-1. Setiap kali daemon dijalankan lagi, test membuat `Api` baru.
- **Adopsi lintas restart daemon in process**: setelah setiap restart, test menunggu `pg17-dev` teradopsi dengan PID dan waktu start yang sama. Kalau adopsi postmaster yang masih menjadi child proses test terbukti tidak andal saat build, test langkah 7 boleh memakai `BinaryDaemon` (`Site::binary`, `stop`) untuk daemonnya, tanpa mengubah AC.

### Penulisan registry khusus test

`Site::bind_runtime(&self, id: Uuid, runtime_id: Uuid)` di harness, dipakai hanya saat tidak ada daemon yang memegang root:

- Membuka registry dengan pola `Site::open_package` (`OpenOptions::new().open(..)` sampai `RegistryOpen::Ready`), membaca `Instance` lewat `registry.instance(id)`, membangun salinannya dengan `runtime_id` lain lewat jalur restore domain (karena `Instance::rebind` menolak line lain, memang itu yang diuji), lalu menulisnya lewat `WriteOp` pembaruan instance yang sudah ada dengan generasi yang dibaca; `revision` naik 1 seperti setiap mutasi registry.
- Tidak menyentuh `instance-manifest.json` atau file apa pun di folder instance, dan menutup registry sebelum kembali, supaya daemon berikutnya membukanya sendiri.
- Kalau langkah 10 spec 0019 sudah menambah helper dengan perilaku ini, pakai helper itu dan jangan membuat yang kedua.

### API surface

Tidak ada method IPC, DTO, kode error, atau perintah CLI baru. Yang dipakai:

| Pemakai | Method atau fungsi | Pemilik | Dipakai untuk |
| --- | --- | --- | --- |
| test native, GUI | `runtime.install`, `runtime.list` | spec 0018 | memasang runtime 18 di samping 17 |
| test native, GUI | `instance.create` | spec 0020 | membuat `pg18-dev` |
| test native, GUI | `instance.start`, `instance.stop`, `instance.get`, `instance.list` | spec 0010 | lifecycle dan `runtimeVersion` |
| test native | `two_postgresql_17_instances`, `SentinelCluster` | spec 0024 | dua PostgreSQL 17 dan sentinel |
| test, fitur 58 | `stackly_core::instance::rebind::check` | spec ini | pemeriksaan rebind yang hanya membaca |
| start, rebind | `stackly_core::instance::data_compat::check_data_compatibility` | spec ini | satu jalur preflight data |
| `stacklyd` | `codes::supervisor::hint` | spec ini (area milik spec 0010) | alasan butuh migrasi |

### Value sourcing

| Aksi | Nilai yang dihasilkan atau ditampilkan | Sumber |
| --- | --- | --- |
| `rebind::check` | data line yang dicatat | `Instance::data_line()` dari registry (kolom data line spec 0005), diisi create dari line runtime saat init |
| `rebind::check` | line runtime target | `Runtime::line()` baris runtime target di registry, ditulis install spec 0018 dari artefak catalog |
| `rebind::check` | isi penanda | `read_file_limited(package.data_marker_path(paths), DATA_MARKER_MAX_LEN)` di folder data instance |
| `rebind::check` | aturan line yang boleh | `compatibility.lineCompatibility` manifest paket pinned instance lewat `ValidatedPackage::preflight` (PostgreSQL tidak punya, jadi hanya line sama) |
| `rebind::check` | paket | folder provider revision pinned dari `ProviderRevisionKey` instance (loader `supervisor::prepare`, spec 0010) |
| `rebind::check` | path instance | `DirectoryLayout::instance_paths(id)` (spec 0004) |
| hint | teks | konstanta di `codes::supervisor::hint`, dipilih dari kode dan detail `outcome` |
| AC-1 | snapshot instance | `instance.get` sebelum dan sesudah install; digest pohon dari helper digest per file di `apps/daemon/tests/postgresql/harness.rs` (pola `data_digest` di `apps/daemon/tests/supervisor/restart.rs`, ditambah mode file) |
| AC-1 | identitas proses | `InstanceSummary.process` (PID dan waktu start) dan `restart` |
| AC-1, AC-2 | runtime 18 | `runtime.install` dengan selector `18`; ID dari `result.resources[0].id` operasi yang `succeeded`; artefak dari entri cache di `downloads_dir()/<sha256>` (*Lingkungan test native*) |
| AC-2 | folder runtime 18 dan `psql` nya | `runtime.get` (`RuntimeDetail.path`), lalu `Psql` harness yang menunjuk `bin/psql` folder itu |
| AC-2 | versi yang dibandingkan | `InstanceSummary.runtime_version` dari `instance.list`, dan `SHOW server_version` lewat `Psql` milik runtime instance itu |
| AC-2 | port dan password `pg18-dev` | `Made` dari pola `harness::create_instance`: port `Fixed` dari `allocate_port`, password `FixtureCredential`, dengan `runtimeId` runtime 18 (harness diberi varian yang menerima runtime ID) |
| AC-2, AC-3 | cluster PostgreSQL 17 | `TwoPostgres17 { dev, test, runtime_id }` dari `two_postgresql_17_instances(site, api)` (spec 0024, `apps/daemon/tests/postgresql/same_runtime.rs`); `pg17-test` adalah duplicate config dari `pg17-dev` |
| AC-2, AC-3 | `SentinelCluster` `pg18-dev` | `SentinelCluster::new(name, id, psql, password)` (spec 0024) dengan `Psql` runtime 18 dan `Made.password` |
| AC-2 | isi `PG_VERSION` dan `data.initializedLine` | file di folder data dan `instance-manifest.json` ketiga instance (`Site::instance_paths`) |
| AC-3 | nilai sentinel | teks unik per instance yang dibuat test (misalnya nama instance ditambah UUID v7), ditulis ke `stackly_sentinel` |
| AC-4 | pengikatan ke runtime lain | `Site::bind_runtime` (*Penulisan registry khusus test*), saat daemon berhenti |
| AC-4 | tidak ada proses `postgres` | `support::serving(&data)` kosong dan tidak ada `postmaster.pid` di folder data instance |
| AC-7 | registry untuk `check` | `stackly_storage::OpenOptions::new().open(&platform, prepared.layout(), daemon_info().version)` yang memberi `RegistryOpen::Ready`, dibuka test saat daemon in process berhenti (pola `Site::open_package`), dengan `site.platform` dan `site.prepared` |
| AC-7 | baris registry sebelum dan sesudah | `registry.instance(id)` dan `registry.runtime(id)` (`Versioned` dengan nilai dan generasi), dibandingkan dengan `PartialEq` |
| AC-9 | subject evidence | `version` dari `SHOW server_version`, `digest` dari `artifact_sha256(PG17)` lewat `Site::subject` dan dari `artifact_sha256(PG18)` lewat panggilan subject evidence yang sama secara langsung |
| AC-10 | runtime di root dev | tombol Get spec 0018 dari cache root dev yang diisi dari cache spike xtask, atau unduhan jaringan setelah persetujuan kamu |

### Key invariants

- Install runtime hanya menulis folder `runtimes/`, cache, dan baris runtime serta artefak di registry; tidak pernah menulis `instances/`, `logs/instances/`, baris instance, atau mengirim sinyal ke proses instance (PRD 24.3: install menambah instalasi berdampingan, tidak memindahkan instance).
- Instance hanya berpindah runtime lewat `Instance::rebind` (BR-001); fitur ini tidak menambah jalur lain dan `rebind::check` tidak pernah menulis.
- Data dengan line yang tidak diizinkan `LineCompatibility` tidak pernah dijalankan runtime target, baik lewat start maupun lewat rebind (BR-009), dan keduanya menolak lewat satu fungsi yang sama dengan kode dan details yang sama.
- Penolakan `incompatible` selalu membawa alasan butuh migrasi (hint), tidak pernah saran mengganti binary.
- Test tidak pernah memakai port 5432 atau folder Application Support asli; port dari `allocate_port`, root dari `TestRoot`, dan proses dibersihkan guard (aturan `AGENTS.md`).

### Security model

Tidak ada permukaan baru. `rebind::check` berjalan di dalam daemon (dan di test), membaca registry, paket, dan satu file penanda maksimal 4 KiB di folder privat milik user, dan tidak menerima input dari klien di fitur ini. Hint statis tidak memuat nilai, jadi tidak membocorkan path atau versi lewat teks yang mungkin disalin pengguna. Password dalam test native adalah credential fixture (`FixtureCredential`) di `FakeSecretStore` harness, dikirim ke psql lewat helper `Psql` harness (env child, tidak pernah di argv atau log).

### Configuration required

Tidak ada env var atau credential baru. Test native butuh artefak `postgresql-17.11-r2-macos-aarch64` dan `postgresql-18.6-r2-macos-aarch64` di cache runtime xtask (`cargo xtask runtime spike`, spec 0003); tanpa itu `BLOCKED`.

### Edge cases

- **Instance 17 sedang start atau stop saat install 18 berjalan**: install tidak mengambil lock instance, jadi operasi instance berjalan seperti biasa; AC-1 hanya menjamin install tidak mengubahnya.
- **Install 18 gagal atau dibatalkan**: kompensasi spec 0018 hanya menyentuh scratch dan `runtimes/`; instance lama tetap sama (bagian dari AC-1 fixture, lewat `FakeFetcher` yang gagal di tahap Verify digest).
- **Runtime target `installing`, `removing`, atau `failed`**: `rebind::check` menolak di pemeriksaan 7.
- **Dua runtime dengan line sama** (misalnya 17.11 dan patch 17 berikutnya): `rebind::check` menjawab `Compatible`; di fitur ini hanya dibuktikan di fixture karena catalog hanya punya satu artefak per line.
- **Registry dan penanda tidak sepakat** (registry 17, `PG_VERSION` 18): `marker_mismatch`, dengan hint penanda, bukan `incompatible`.
- **Penanda tidak bisa dibaca karena izin**: `marker_unreadable`; folder instance yang bukan milik user sudah ditolak lebih dulu di pemeriksaan 9.
- **Daemon mati di tengah siklus stop dan start AC-3**: di luar fitur ini (reconciliation fitur 30); test memakai guard yang membersihkan proses.
- **Batas shared memory System V**: tiga cluster jauh di bawah batas sekitar 32 instance PostgreSQL di Mac ini (spec 0019); dua test native yang berjalan paralel memegang enam cluster.

### Critical test scenarios

| Test | Jenis | Membuktikan | `covers` evidence |
| --- | --- | --- | --- |
| `data_compatibility_is_the_same_for_start_and_rebind` (core) | fixture | **AC-8** | BR-009 |
| `rebind_check_refuses_another_postgresql_major` dan kasus tabel `rebind::check` (core, paket `providers/postgresql` asli, `FakeRegistry`, `FakeFilesystem`) | fixture | **AC-6** | REQ-043, AC-043, BR-009 |
| `supervisor_hint_names_the_migration` (protocol) | fixture | **AC-5** | BR-009 |
| `data_incompatible_failure_carries_the_hint` (daemon in process, dua kasus: instance fake dengan `FAKE_VERSION` diubah menjadi `0.2`, jadi `marker_mismatch`; dan instance fake yang diikat lewat penulisan registry khusus test ke runtime fake kedua ber line `0.2`, jadi `incompatible`) | fixture | **AC-5** | BR-009 |
| `runtime_install_leaves_existing_instances_untouched` (daemon in process, satu instance fake `running` dan satu `stopped`, install sukses dan install yang gagal di Verify digest lewat `FakeFetcher`) | fixture | **AC-1** | REQ-043, BR-001 |
| `postgresql_18_beside_17_journey` (`apps/daemon/tests/postgresql/side_by_side.rs`) | native | **AC-1**, **AC-2**, **AC-3** | REQ-035, AC-035, AC-009, REQ-043, BR-001 (JOURNEY-03 di `expected`) |
| `another_major_is_refused_at_start_and_by_the_rebind_check` (`apps/daemon/tests/postgresql/side_by_side.rs`) | native | **AC-4**, **AC-7** | BR-009, AC-035, AC-043, REQ-043 (`expected` menyebut bukti AC-043 bagian) |
| JOURNEY-03 di aplikasi | manual native | **AC-10** | REQ-035, REQ-043 (JOURNEY-03 di `expected`) |

- **Happy path**: dua PostgreSQL 17 (penyiapan spec 0024), install 18 dari cache, `pg18-dev` dibuat dan ready, versi dibandingkan, sentinel tetap terisolasi setelah stop dan start per instance. Memverifikasi **AC-1**, **AC-2**, **AC-3**.
- **Failure case**: binary mayor salah pada data yang ada, dua arah, ditolak sebelum spawn dengan hint migrasi; data, tetangga, dan registry tetap utuh, lalu pulih. Memverifikasi **AC-4**, **AC-7**.
- **Batas**: `rebind::check` tidak pernah menulis, dan menolak dengan kode yang sama dengan start. Memverifikasi **AC-6**, **AC-8**.
- Kedua test native didaftarkan `#[path = "postgresql/side_by_side.rs"] mod side_by_side;` di `apps/daemon/tests/postgresql.rs`, bertanda `#[ignore = "native: needs both managed PostgreSQL artifacts in the runtime cache"]` seperti test native lain di file itu, dijalankan dengan `cargo test -p stacklyd --test postgresql side_by_side -- --ignored`, dan memakai `three_postgresql_clusters` (*Lingkungan test native*).
- Runtime fake kedua untuk kasus `incompatible` fixture dipasang lewat varian fixture `supervisor::fixture::install_fake_runtime` yang menerima versi dan line (misalnya `0.2.0`, line `0.2`) alih alih artefak pertama paket, dengan salinan `stackly-fake-server` yang sama; penanda `FAKE_VERSION` instance tetap `0.1`, jadi preflight menjawab `Incompatible` (data `0.1`, runtime `0.2`).

### Usulan update untuk spec lain

Spec ini tidak mengedit spec lain. Usulan berikut diterapkan lead lewat `/architect` update, sebelum langkah build yang menyebutnya:

| Spec | Bagian | Usulan | Dibutuhkan sebelum |
| --- | --- | --- | --- |
| 0010 | *Kode error* (area `supervisor`) | `supervisor.data_incompatible` punya hint statis `codes::supervisor::hint` per `outcome` (teks di *Hint migrasi* spec 0025), dipasang `stacklyd` lewat fallback hint `FailureError` spec 0006 untuk kegagalan operasi; `verify` memanggil `stackly_core::instance::data_compat::check_data_compatibility` (perilaku tidak berubah) | langkah 3 dan 4 |
| 0009 | *Preflight kompatibilitas data*, daftar pemanggil | pemanggil membaca penanda lewat `check_data_compatibility` (spec 0025); rebind (fitur 58) memanggilnya lewat `stackly_core::instance::rebind::check` | langkah 2 |
| 0019 | *Preflight data*, pemanggil; AC-11 | daftar pemanggil menyebut `rebind::check` spec 0025 untuk fitur 58; penolakan AC-11 kini membawa hint `incompatible` atau hint penanda (test AC-11 boleh memeriksanya, tidak wajib) | langkah 4 |
| 0004 | tabel *Invariant BR-001 sampai BR-025* | kolom fitur pemilik BR-001 ditambah fitur 25 (bukti native runtime ID persis setelah install mayor lain), BR-009 ditambah fitur 25 (bukti native start dan pemeriksaan rebind) | langkah 6 |

## Build plan

Urutan mengikuti pendekatan Journey proyek: jalur JOURNEY-03 ("pasang mayor kedua, buat instance baru di sampingnya, bandingkan, dan data lama tetap utuh") dibuat lengkap termasuk jalur gagalnya. Milestone 1 membangun semua kode baru, yang bisa dibuktikan tanpa artefak dan tanpa menunggu fitur lain; milestone 2 membuktikan install tidak menyentuh instance di fixture begitu `runtime.install` ada di daemon; milestone 3 membuktikan seluruh journey dan penolakan pada PostgreSQL asli di atas skenario spec 0024; milestone 4 menjalankan journey yang sama di aplikasi.

**Milestone 1: Kecocokan data bersama, pemeriksaan rebind, dan hint migrasi, dibuktikan di fixture**

1. `stackly_core::instance::data_compat` (`DataIncompatible`, `DataOutcome`, `check_data_compatibility`), `From<DataIncompatible> for SupervisorError`, `verify` di `core::supervisor::startup` memakainya, test tabel yang membandingkan jalur start dengan pemanggilan langsung untuk keempat hasil yang bisa terjadi, dan kasus start baru (`incompatible`, `marker_missing`, `marker_unreadable`) di `crates/core/tests/supervisor_start/` di samping kasus `marker_mismatch` yang sudah ada. Memenuhi **AC-8**.
2. `stackly_core::instance::rebind` (`check`, `RebindVerdict`, `RebindCheckError` dengan implementasi `OperationError`), `pub(crate) mod prepare` dengan pembantunya `pub(crate)`, dan test core dengan `FakeRegistry`, `FakeFilesystem`, `FakeProcessSupervision`, paket `providers/postgresql` asli (`PG_VERSION` `17\n` dan `18\n`), dan paket fake yang dibangun di memori dengan `lineCompatibility` tambahan untuk jalur `Compatible`, mencakup setiap baris tabel *Pemeriksaan rebind* dan tanpa mutasi. Memenuhi **AC-6** (pemeriksaan hint nya setelah langkah 3).
3. `codes::supervisor::hint` di `stackly-protocol` dengan unit test teks persis, panjang paling banyak 200 byte, tanpa `{`, dan `None` untuk `unknown`, nilai asing, dan kode lain, lalu `cargo xtask codegen --check` bersih tanpa kenaikan protocol. Memenuhi **AC-5** (bagian protocol).
4. Fallback hint `FailureError` spec 0006 untuk area `supervisor` di `stacklyd` (memakai fallback langkah 11 spec 0018 kalau sudah di `main`, selain itu membangunnya), varian fixture `install_fake_runtime` yang menerima versi dan line, dan test daemon in process `data_incompatible_failure_carries_the_hint` dengan kedua kasus (`marker_mismatch` dan `incompatible`), masing masing dengan hint nya di `failure.error.hint`. Memenuhi **AC-5**.

**Milestone 2: Install tidak menyentuh instance lama, dibuktikan di fixture**

5. Test daemon in process `runtime_install_leaves_existing_instances_untouched`: dua instance fake dari `install_fake` (satu `running` dan `ready`, satu `stopped`), snapshot `instance.get`, digest pohon, dan identitas proses, lalu `runtime.install` artefak software `fake` dengan line lain (misalnya `0.2.0`, tar.gz berisi salinan `stackly-fake-server` seperti happy path core spec 0018) yang dilayani `FakeFetcher`, memakai catalog dan setup integration test daemon langkah 11 spec 0018 (ditambah entri `CatalogBuilder` itu kalau catalog fixture nya belum punya artefak `fake` yang bisa dipasang), sekali sukses dan sekali gagal di tahap Verify digest, dan snapshot dibandingkan setelah masing masing. Memenuhi **AC-1** (bagian fixture).

**Milestone 3: PostgreSQL 17 dan 18 berdampingan di Mac ini** (butuh kedua artefak PostgreSQL di cache runtime xtask, selain itu `BLOCKED`; tanpa unduhan)

6. Tambahan harness di `apps/daemon/tests/postgresql/harness.rs` (*Lingkungan test native*: daemon in process dengan platform berfetcher kosong dan catalog repo, entri cache di `downloads_dir()`, create di runtime lain, `Psql` runtime lain, digest pohon) dan `Site::bind_runtime` (*Penulisan registry khusus test*). Lalu modul `side_by_side.rs` dengan `three_postgresql_clusters` dan test native `postgresql_18_beside_17_journey`: snapshot di sekitar `runtime.install` 18 dari cache dengan `pg17-dev` berjalan dan `pg17-test` berhenti, create dan start `pg18-dev`, perbandingan versi, sentinel tiga cluster, lalu stop dan start per instance. Memenuhi **AC-1** (bagian native), **AC-2**, **AC-3**, **AC-9**.
7. Test native `another_major_is_refused_at_start_and_by_the_rebind_check` di modul yang sama: `pg17-test` dan `pg18-dev` di stop, daemon in process berhenti (`InProcess::shutdown`; postmaster `pg17-dev` tetap hidup), snapshot, `rebind::check` dua arah dan `Current` lewat registry yang dibuka test, bandingkan snapshot; `Site::bind_runtime` mengikat keduanya ke mayor lain; daemon jalan, `pg17-dev` teradopsi dengan PID dan waktu start yang sama, start kedua instance lain ditolak dengan hint dan tetap `stopped`; daemon berhenti, `runtimeId` dikembalikan, daemon jalan, ketiganya ready dan terisolasi. Memenuhi **AC-4**, **AC-7**, **AC-9**.
8. Jalur `BLOCKED` kedua test (artefak hilang dari cache, dengan nama setiap artefak yang hilang) dan evidence `native` dengan subject kedua runtime (`Site::subject` untuk 17, panggilan subject langsung dengan `artifact_sha256(PG18)` untuk 18) dan `expected` yang menyebut JOURNEY-03 dan sifat bagian bukti AC-043; jalankan `cargo xtask runtime spike --cleanup` sesudah run native untuk memastikan tidak ada proses `postgres` sisa. Memenuhi **AC-9**.

**Milestone 4: JOURNEY-03 di aplikasi** (manual; unduhan lewat tombol Get hanya setelah persetujuan kamu, selain itu cache root dev diisi dari cache spike xtask)

9. Cek manual AC-10 di root dev terpisah (`STACKLY_ROOT`), dengan catatan evidence manual `native` beserta tangkapan layar halaman Runtimes, sheet New Instance, halaman Instances, dan tab Summary `pg18-dev`. Memenuhi **AC-10**.

**Prasyarat**: milestone 1 hanya butuh yang sudah di `main` (jalur start dan kode area `supervisor` spec 0010, paket `providers/postgresql` dari langkah 4 spec 0019); usulan update spec 0010 dan 0009 diterapkan lead sebelum langkah 2 sampai 4. Milestone 2 butuh langkah 11 milestone 3 spec 0018 (`runtime.install` di daemon dengan `FakeFetcher`). Milestone 3 butuh milestone 1 dan 2, milestone 4 spec 0024 (langkah 8 dan 9: `sentinel::SentinelCluster` dan `two_postgresql_17_instances`), langkah 9 spec 0019 (entri catalog `[[provider]]` PostgreSQL, yang dibutuhkan gerbang catalog `runtime.install`; spec 0024 milestone 4 sudah menunggunya juga), dan milestone 3 spec 0018 (`runtime.install`, `runtime.get`, dan kontrak runtime di daemon). Harness PostgreSQL native dan suite PostgreSQL 18 (milestone 2 spec 0019) sudah di `main`. Kalau langkah 16 spec 0018 sudah di `main` saat langkah 6 dimulai, salin entri cache memakai helper nya; kalau belum, langkah 6 menyalin file dari `harness::cached_artifact` sendiri. Milestone 4 butuh milestone 3, milestone 4 spec 0018 (halaman Runtimes dan Get), milestone 3 spec 0020 (sheet New Instance), milestone 3 spec 0021 (tab Summary), dan milestone 3 spec 0022 (halaman Instances).

**Sesi paralel yang aman**: langkah 1 dan 3 menyentuh file berbeda (core supervisor dan instance, protocol) dan bisa dua sesi; langkah 2 menunggu langkah 1, dan langkah 4 menunggu langkah 3. Langkah 1 dan 2 mengubah `crates/core/src/supervisor/startup.rs`, `prepare.rs`, dan `mod.rs`, jadi tidak berjalan bersamaan dengan sesi lain yang mengubah file supervisor core (misalnya build fitur 20 atau 21); langkah 4 mengubah `apps/daemon/src/ipc/operation/dto.rs` dan tidak berjalan bersamaan dengan langkah 11 spec 0018. Milestone 2 menyentuh file test daemon sendiri dan bisa berjalan bersamaan dengan milestone 1. Langkah 6 sampai 8 sebaiknya satu sesi karena menyentuh `apps/daemon/tests/postgresql.rs` dan `postgresql/harness.rs` yang juga diubah milestone 3 spec 0019 dan milestone 4 spec 0024; jangan jalankan bersamaan dengan sesi lain yang mengubah kedua file itu.

## Consequences

**Positive**:

- JOURNEY-03 terbukti pada PostgreSQL asli: mayor kedua dipasang dan dipakai berdampingan tanpa menyentuh data, proses, atau registry instance lama.
- Start dan rebind tidak bisa berbeda pendapat tentang kecocokan data, karena keduanya memanggil satu fungsi; fitur 58 mendapat precheck yang sudah teruji pada data asli.
- Pengguna yang mencoba mayor salah mendapat alasan yang bisa ditindaklanjuti (butuh migrasi), bukan sekadar "tidak kompatibel".
- Tidak ada perubahan kontrak IPC, jadi Swift, fixture wire, dan versi protocol tidak tersentuh.

**Negative / tradeoffs**:

- Bagian AC-043 tentang aksi pengguna (perintah rebind di CLI dan GUI) belum ada sampai fitur 58; di fase ini pengguna tidak bisa "mencoba rebind", jadi bukti AC-043 berhenti di lapisan core dan data asli.
- `rebind::check` adalah kode publik yang belum punya pemanggil produksi sampai fitur 58; bentuknya bisa berubah saat fitur 58 dirancang (misalnya kalau rebind butuh daftar runtime kandidat), dan perubahan itu lewat update spec ini.
- Test native berat: dua test, masing masing tiga cluster PostgreSQL, satu install dari cache, dan beberapa siklus stop dan start; waktunya puluhan detik dan butuh kedua artefak di cache.
- Test AC-4 dan AC-7 menulis registry dari luar daemon (dengan daemon berhenti); itu jalur khusus test yang harus tetap dijaga agar tidak pernah dipakai kode produksi.
- Teks hint `incompatible` menyebut "dump and restore" generik; sampai fase 09, Stackly sendiri belum menyediakan alat untuk itu.

**Neutral**:

- Binary test `apps/daemon/tests/postgresql.rs` menjadi rumah bersama test native spec 0019, 0024, dan 0025, masing masing di modul `postgresql/<nama>.rs` sendiri dengan harness yang sama.
- PRD AC-009 ditutup oleh dua spec: bagian dua PostgreSQL 17 di spec 0024 dan bagian satu mayor 18 di spec ini.

## Follow-up

- [x] `/architect` update spec 0010: hint `supervisor.data_incompatible` per `outcome` dan pemakaian `check_data_compatibility` di `verify` (tabel *Usulan update untuk spec lain*), sebelum langkah 3 dan 4; diterapkan di spec 0010 (2026-10-22).
- [x] `/architect` update spec 0009: daftar pemanggil preflight menyebut `check_data_compatibility` dan `rebind::check` spec 0025, sebelum langkah 2; diterapkan di spec 0009 (2026-10-22).
- [x] `/architect` update spec 0019: pemanggil preflight menyebut `rebind::check` spec 0025, dan AC-11 boleh memeriksa hint, sebelum langkah 4; diterapkan di spec 0019 (2026-10-22).
- [x] `/architect` update spec 0004: fitur 25 di kolom fitur pemilik BR-001 dan BR-009, sebelum langkah 6; diterapkan di spec 0004 (2026-10-22).
- [ ] Fitur 58: operasi `rebind_runtime` memanggil `stackly_core::instance::rebind::check` sebagai precheck dan sekali lagi setelah stop terkontrol sebelum `Instance::rebind`, ditambah method IPC, CLI, GUI, kebijakan backup sebelum rebind, verifikasi start, dan pesan penolakan yang mengarahkan ke backup dan restore (fitur 50 dan 51), termasuk keputusan apakah teks hint `incompatible` diganti.
- [ ] Fitur 51 dan 52: bagian AC-035 untuk backup dan restore pada matriks PostgreSQL 17 dan 18 serta pasangan versi sama, di atas `three_postgresql_clusters`.
- [ ] Fitur 26: setelah restart menjadi satu operasi, ulangi siklus AC-3 dengan `instance.restart` di test yang sama.
- [ ] `/sync` setelah build: catat perintah test native fitur ini dan fungsi `three_postgresql_clusters` di root `AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, keputusan kecil, pertukaran dengan sesi lain, dan cross check ada di [rationale.md](rationale.md).
