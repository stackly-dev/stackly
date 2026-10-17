# 0004. Domain core dan abstraksi platform

**Date**: 2026-10-17
**Status**: Proposed

## Summary

Spec ini mengisi pusat arsitektur Stackly: model domain (software, provider revision, runtime, instance, environment, operation, lease), tiga state machine yang terpisah (state runtime, lifecycle instance, readiness dan health) ditambah state machine operasi, dan BR-001 sampai BR-025 sebagai invariant (aturan yang harus selalu benar) yang dicek kode bila bisa dicek hari ini. Di sampingnya ada port platform (trait yang diimplementasikan adapter OS) untuk layout direktori per user, filesystem, jam, dan identitas user, dengan satu fake platform di `stackly-testkit` supaya semua use case bisa diuji tanpa folder asli. Layout dibagi dua: owned root panjang di `~/Library` untuk data, log, dan cache, plus runtime root pendek di folder temp privat milik user untuk socket, supaya path socket Unix tidak melewati batas 104 byte macOS. Setiap folder instance membawa `instance-manifest.json` yang cukup untuk inventarisasi tanpa registry, dan capability Windows yang belum dibuat selalu menjawab not supported.

## Requirements

**User stories**:

- Sebagai developer fitur 5 sampai 20, saya ingin satu model domain dengan state machine dan invariant yang sudah teruji, supaya registry, IPC, supervisor, dan provider tidak masing masing menafsirkan ulang aturan PRD.
- Sebagai penulis integration test, saya ingin mengarahkan daemon dan CLI ke root alternatif, supaya test tidak pernah menyentuh folder Application Support asli dan bisa berjalan paralel.
- Sebagai pengguna yang registry nya rusak atau hilang, saya ingin setiap folder instance tetap menjelaskan dirinya sendiri, supaya datanya bisa diinventarisasi dan nanti diadopsi ulang (fitur 57) tanpa tebakan.
- Sebagai maintainer fase Windows, saya ingin capability Windows yang belum dibuat menjawab not supported secara jujur, supaya port berikutnya cukup mengganti adapter tanpa menyentuh core.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-050):

- **AC-1**: `stackly-domain` memuat semua entitas dan value type di *Model domain* dengan dependency langsung hanya `serde`, `thiserror`, `uuid`, dan `jiff`. `cargo xtask check-deps` lulus, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-core` lulus (REQ-001, REQ-050, NFR-010).
- **AC-2**: State runtime, lifecycle instance, serta readiness dan health adalah tipe terpisah yang disimpan di field terpisah. Untuk state runtime dan lifecycle instance, satu test per machine menjalankan setiap pasangan (state, event), termasuk setiap nilai payload event: pasangan yang ada di tabel menghasilkan state yang tertulis, pasangan lain mengembalikan `TransitionError` yang menyebut machine, state, dan event, dan state tidak berubah. Setiap state bolak balik lewat kode snake_case stabilnya tanpa kehilangan nilai (PRD bagian 19.1, REQ-012).
- **AC-3**: Readiness `ready` hanya diterima saat lifecycle `running`, dan health selain `unknown` juga hanya diterima saat `running`. Setiap event yang membawa instance keluar dari `running` membuat readiness dan health `unknown` dengan reason `not_running`; masuk ke `running` membuat readiness `not_ready` dan health `unknown`, keduanya dengan reason `awaiting_probe`. Observasi dengan waktu lebih lama dari yang tersimpan diabaikan. `effective(now, max_age)` pada readiness dan health mengembalikan `unknown` dengan reason `stale` untuk nilai selain `unknown` yang lebih tua dari `max_age`. Perubahan health tidak pernah mengubah lifecycle atau desired state (BR-010, REQ-012).
- **AC-4**: Operation memakai tujuh state PRD bagian 18.2 sesuai tabel, diuji menyeluruh per pasangan (state, event) seperti AC-2. `succeeded`, `failed`, dan `cancelled` tidak menerima event apa pun; cancel pada operasi `queued` yang belum pernah mulai langsung `cancelled`, sedangkan pada operasi `queued` yang sudah pernah mulai menjadi `cancel_requested`; operasi `cancel_requested` yang langkah irreversible nya selesai menjadi `succeeded`, bukan `cancelled`; operasi yang terputus dan tidak bisa dilanjutkan menjadi `needs_recovery`. Progress yang melebihi total atau diubah di luar `running` dan `cancel_requested` menghasilkan `ProgressError` (REQ-054).
- **AC-5**: Setiap BR yang ditandai `F4` di tabel *Invariant BR-001 sampai BR-025* punya fungsi domain atau core dan minimal satu test yang lulus dan menulis evidence record `fixture` (spec 0002) dengan `covers` berisi ID BR itu. Kalau aturan itu menolak lewat error, error tersebut mengembalikan ID aturan lewat `rule()`. Setiap BR yang ditandai dipetakan menyebut fitur pemiliknya di tabel itu (BR-001 sampai BR-025).
- **AC-6**: Resolver macOS menurunkan root data, log, dan cache dari home directory milik uid yang sedang berjalan menurut database user (`getpwuid_r`, bukan variabel `HOME`) ditambah `Library/Application Support/Stackly`, `Library/Logs/Stackly`, dan `Library/Caches/Stackly`, serta runtime root dari `confstr(_CS_DARWIN_USER_TEMP_DIR)` ditambah `stackly`. Semua path lain berasal dari fungsi `DirectoryLayout`. `cargo xtask check-deps` gagal kalau crate library membaca variabel environment, memanggil `home_dir` atau `temp_dir`, atau memuat literal string `/Users/`, `/opt/homebrew`, `/usr/local`, `/tmp`, atau `/var/folders`.
- **AC-7**: `RootSet::under_override(R, base)` menghasilkan `R/data`, `R/logs`, `R/cache`, dan runtime root `base/stackly-<8 hex>`. Adapter macOS memvalidasi root override (tidak kosong, absolut, ada, folder dan bukan symlink, dimiliki user yang sedang berjalan, tanpa izin tulis untuk group dan other, tidak sama dengan atau berada di dalam root data default) dan gagal dengan pesan yang menyebut path dan aturannya. `stacklyd` membaca `STACKLY_ROOT` sekali saat start, mencatat keempat root di satu baris log, dan keluar dengan kode 1 serta log error kalau override ditolak; integration test di `apps/daemon/tests/` membuktikan keduanya. `TestRoot` di testkit memakai fungsi `under_override` yang sama.
- **AC-8**: `RootSet::new` menolak runtime root yang lebih panjang dari 76 byte dengan `RuntimeRootTooLong`. Untuk setiap root set yang diterima, path socket daemon dan path `instance_run_dir(key)` ditambah nama socket 15 byte tidak pernah lebih dari 103 byte. Root default dan root override di Mac uji diterima.
- **AC-9**: `prepare_layout` membuat owned root dan subfolder standarnya yang belum ada dengan mode 0700, dan run kedua tidak mengubah apa pun. Root atau subfolder standar yang berupa symlink, bukan folder, dimiliki user lain, atau bisa diakses group atau other ditolak dengan error yang menyebut path dan alasannya, begitu juga root yang bentuk canonical nya sama atau bertumpuk dengan root lain. Tidak ada yang dibuat di luar owned root (fake filesystem mencatat nol pelanggaran).
- **AC-10**: `create_instance_layout` membuat `instances/<id>/`, menulis `instance-manifest.json` secara atomik sebelum subfolder lain (dengan `data.initializedAt` null), lalu membuat `data/`, `config/`, `config-history/`, `temp/`, dan `logs/instances/<id>/`. Ia menolak instance yang foldernya sudah ada di `instances/` atau `retained/` (BR-008), dan menolak short key yang sudah dipakai instance yang diketahui dengan `ShortKeyTaken`. Kalau satu langkah gagal, ia mengembalikan daftar persis path yang dibuatnya dan tidak menghapus apa pun selain file sementara miliknya sendiri (BR-023).
- **AC-11**: `inventory_instances` mendaftar setiap folder di `instances/` lalu di `retained/`, masing masing urut nama, hanya dari isi folder, dengan data dari manifest. Ia mengklasifikasikan manifest yang hilang, bukan file biasa, tidak terbaca, berschema lain, ID yang tidak cocok dengan nama folder, lokasi yang tidak cocok dengan penanda retained, symlink, dan entri asing (termasuk nama folder yang bukan ID), dan melewati `.DS_Store`. Ia tidak pernah mengikuti symlink, tidak pernah membuka file yang bukan file biasa, dan tidak memanggil satu pun operasi filesystem yang mengubah isi disk. Manifest lebih dari 64 KiB tercatat tidak terbaca (REQ-053).
- **AC-12**: `FakePlatform` di `stackly-testkit` memberi filesystem nyata yang dikurung di root milik test (path yang parent canonical nya di luar root menghasilkan `OutsideTestRoots` dan dihitung), fault injection per operasi, log mutasi, jam yang bisa diatur, identitas user yang bisa diganti, dan capability yang bisa ditimpa. Satu suite `fs_conformance` di testkit lulus untuk `FakeFilesystem` dan `MacosFilesystem`. Test core untuk AC-9 sampai AC-11 dijalankan manual di Mac ini dengan perintah fixture di spec 0002, bagian *Menjalankan test di Mac ini* (NFR-010).
- **AC-13**: `UnsupportedPlatform` untuk os `windows` menjawab `NotSupported { capability, os: windows }` untuk `Directories`, `Filesystem`, `UserIdentity`, `ProcessSupervision`, `SecretStore`, `IpcEndpoint`, `BackgroundAgent`, dan `ProcessMetrics`, dan setiap method dari tiga trait pertama mengembalikan error itu; `Clock` tetap bekerja. Test-nya jalan di Mac ini, dan crate-nya lulus `cargo check --target aarch64-pc-windows-msvc` (AC-050).
- **AC-14**: `native_support(artifact_targets, host)` bernilai true hanya kalau ada artefak dengan os dan arch yang persis sama dengan host. Untuk host Windows, provider yang hanya punya artefak macOS, atau hanya artefak `linux` (termasuk build untuk WSL), menghasilkan false, dan domain tidak punya jalur substitusi apa pun (AC-074).
- **AC-15**: `MacosPlatform` melaporkan `Directories`, `Filesystem`, `Clock`, dan `UserIdentity` sebagai supported, dan lima capability lain sebagai not supported dengan reason yang menyebut fitur pemiliknya. Integration test di `crates/platform-macos/tests/` menjalankan `prepare_layout`, `create_instance_layout`, dan `inventory_instances` lewat `MacosPlatform::with_roots(test_root.roots())`, dan menguji validasi override di filesystem asli (REQ-001).
- **AC-16**: Instance baru lahir `uninitialized` dengan desired `stopped`. `start_begin` ditolak dengan `InvariantError` BR-008 `DataNotInitialized` selama `data_initialized_at` kosong, dan `init_succeeded` yang mengisinya. `rebind` hanya diterima saat lifecycle `stopped` dan untuk runtime dengan software yang sama. Observasi readiness dan health tidak menaikkan `revision`, sedangkan setiap mutasi lain menaikkannya tepat 1.

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md): domain lengkap dengan state machine berbasis tabel, port platform sync yang dibundel sebagai `Arc<dyn Trait>`, layout dua lapis (owned root standar macOS plus runtime root pendek di temp privat user), root alternatif lewat `STACKLY_ROOT`, dan fake platform di atas filesystem nyata yang dikurung.

Domain menjadi satu satunya tempat aturan PRD ditulis sebagai kode, platform hanya memberi fakta OS lewat trait kecil, dan core merangkai keduanya dalam use case sync yang bisa diuji dengan fake yang sama di Linux dan macOS.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya newtype dengan validasi di konstruktor, dan error dengan `rule()` yang menyebut ID BR.

## Feature design

### Peta modul dan dependency

Semua aturan arah dependency spec 0001 tetap berlaku. Spec ini menambah:

| Crate | Dependency langsung yang baru | Modul baru |
| --- | --- | --- |
| `stackly-domain` | `serde` (derive), `jiff` (fitur `serde`); `uuid` mendapat fitur `serde`; dev: `stackly-evidence`, `serde_json` | `id`, `value`, `target`, `endpoint`, `capability`, `provider`, `runtime`, `instance`, `observation`, `environment`, `lease`, `operation`, `manifest`, `rule`, `error` |
| `stackly-platform` | `stackly-domain`, `jiff`; dev: `stackly-evidence` | `layout`, `fs`, `clock`, `identity`, `info`, `capability`, `unsupported`, `platform`, `error` |
| `stackly-core` | `stackly-domain`, `stackly-platform`, `serde_json`, `thiserror`; dev: `stackly-testkit` | `layout`, `instance_dir`, `inventory`, `ownership`, `error` |
| `stackly-platform-macos` | `stackly-domain`, `libc`; dev: `stackly-core`, `stackly-testkit` | `dirs`, `fs`, `identity`, `info`, `platform` |
| `stackly-testkit` | `stackly-domain` | `fake_fs`, `fake_platform`, `conformance`; `TestRoot` mendapat runtime folder pendek |
| `stacklyd` | dev: `stackly-testkit`, `serde_json` | `main` membaca `STACKLY_ROOT` dan memakai `MacosPlatform` |
| `xtask` | `syn` (fitur `full`, `visit`), kalau langkah 2 spec 0002 belum menambahkannya | pindaian source di `check_deps` |

Aturan baru di `cargo xtask check-deps`:

- `stackly-platform` tidak punya tokio atau libc sebagai dependency langsung, supaya ia tetap berisi trait dan logika path yang portabel.
- Pindaian source lewat `syn` untuk `stackly-domain`, `stackly-platform`, `stackly-core`, `stackly-protocol`, dan `stackly-platform-macos` (di luar modul `#[cfg(test)]` dan folder `tests/`): tidak boleh ada pemanggilan `env::var`, `env::var_os`, `env::vars`, `env::vars_os`, `env::home_dir`, atau `env::temp_dir`, dan tidak boleh ada literal string yang memuat `/Users/`, `/opt/homebrew`, `/usr/local`, `/tmp`, atau `/var/folders`. Pesan gagalnya `<crate> <file>:<baris>: <aturan>`.

`stackly-protocol` tidak berubah. Pemetaan tipe domain ke DTO tetap terjadi di `stacklyd` (fitur 6).

### Model domain

Semua tipe ada di `stackly-domain`. Field privat dengan getter; konstruktor memvalidasi, jadi nilai yang sudah ada selalu valid. Setiap enum state punya `as_code()` dan `FromStr` dengan kode snake_case yang stabil untuk registry dan DTO. Waktu memakai `jiff::Timestamp` (UTC).

**ID** (satu makro `uuid_id!` menghasilkan newtype yang sama polanya dengan `InstanceId` hari ini; error bersama `InvalidId` menggantikan `InvalidInstanceId`):

| Tipe | Bentuk | Catatan |
| --- | --- | --- |
| `InstanceId`, `RuntimeId`, `EnvironmentId`, `OperationId`, `LeaseId`, `BackupId` | UUID v7, teks lowercase dengan tanda hubung, parse tidak peka huruf besar kecil | hanya daemon yang memanggil `generate()`; serde lewat bentuk teks dan menolak versi selain 7. `BackupId` dipakai di sini hanya sebagai referensi snapshot recovery (fitur 49 memakainya penuh) |
| `ShortKey` | 8 hex huruf kecil | `InstanceId::short_key()` mengambil 8 karakter terakhir bentuk teks (bagian acak UUID v7) |

**Value type**:

| Tipe | Aturan | Sumber nilai |
| --- | --- | --- |
| `InstanceName` | 1 sampai 63 karakter, `^[a-z]([a-z0-9]*(-[a-z0-9]+)*)?$` (VAL-001) | input create atau rename |
| `DisplayName` | 1 sampai 128 karakter Unicode, tanpa karakter kontrol | input opsional |
| `SoftwareName` | pola sama dengan `InstanceName`, misalnya `postgresql` | provider revision |
| `ProviderName` | `<namespace>/<name>`, tiap bagian berpola `InstanceName`, misalnya `official/postgresql` (VAL-002) | manifest provider (fitur 9) |
| `RevisionLabel` | 1 sampai 64 karakter, `^[0-9A-Za-z][0-9A-Za-z.+_-]*$`, tanpa `..` (VAL-002) | manifest provider |
| `ExactVersion` | maksimal 64 karakter, `^[0-9]+(\.[0-9]+)+([+-][0-9A-Za-z.]+)?$` (pola spec 0003); hanya kesamaan, tanpa urutan | artefak runtime |
| `RuntimeLine` | `^[0-9]+(\.[0-9]+)?$`, misalnya `17`, `8.4`, `26.3`; `contains(version)` benar kalau versi diawali `<line>.` | artefak runtime |
| `Os`, `Arch`, `Target` | `macos`, `windows`, `linux`; `aarch64`, `x86_64`; `Target` ditampilkan `macos-aarch64`. Tidak ada varian WSL: artefak untuk WSL adalah `linux` | artefak, host |
| `Sha256Digest` | 64 hex, disimpan huruf kecil | artefak, manifest provider, diff yang disetujui |
| `Port` | 1 sampai 65535 (`NonZeroU16`) | input atau port yang benar benar di bind |
| `SocketName` | 1 sampai 15 byte, `[A-Za-z0-9._-]`, bukan `.` atau `..` | provider (fitur 9 memvalidasi panjangnya) |
| `EndpointName` | pola `InstanceName`, maksimal 32 karakter | provider |
| `IdempotencyKey` | 1 sampai 128 byte ASCII tercetak (0x21 sampai 0x7e) | client (fitur 6 dan 7) |
| `SessionId` | 1 sampai 64 byte ASCII tercetak, opaque | platform saat login (fitur 31) |
| `ReasonCode`, `FailureCode` | `^[a-z][a-z0-9_]{0,63}$` | kode tetap di kode sumber |
| `StageName` | `^[a-z][a-z0-9_]{0,31}$` | engine operasi (fitur 7) |
| `ResourceRevision` | `u64`, mulai 1, naik tepat 1 setiap mutasi aggregate selain observasi readiness dan health | aggregate itu sendiri |

**Entitas**:

| Entitas | Kunci | Field | Relasi |
| --- | --- | --- | --- |
| Software | `SoftwareName` (value) | nama | 1:N ProviderRevision, 1:N Runtime |
| ProviderRevision | `ProviderRevisionKey` (`provider: ProviderName`, `revision: RevisionLabel`, `manifest_sha256: Sha256Digest`), immutable | `software`, `capabilities: CapabilitySet`, `line_compatibility: LineCompatibility` | N:1 Software, 1:N Instance |
| Runtime | `RuntimeId` | `software`, `version: ExactVersion`, `line: RuntimeLine`, `target`, `install: InstallMode`, `state: RuntimeState`, `installed_at` dan `last_verified_at` (opsional), `depends_on: BTreeSet<RuntimeId>` | 1:N Instance, N:M Runtime |
| Instance | `InstanceId` | `name`, `display_name` (opsional), `provider_revision: ProviderRevisionKey`, `software`, `runtime_id`, `data_line: RuntimeLine`, `data_initialized_at` (opsional), `endpoints: Vec<Endpoint>`, `desired: DesiredState`, `startup: StartupPolicy`, `lifecycle: InstanceLifecycle`, `last_failure: Option<FailureCode>`, `readiness: Readiness`, `health: Health`, `revision`, `created_at`, `created_by: OperationId`, `retained_at` (opsional) | N:1 Runtime, N:1 ProviderRevision, 1:N Lease, 1:1 InstanceManifest |
| Endpoint (value) | (instance, `EndpointName`) | `Ip { name, transport: tcp atau udp, address: IpAddr, request: PortRequest, bound: Option<Port> }` atau `Unix { name, socket: SocketName }` | N:1 Instance |
| Environment | `EnvironmentId` | `name` (pola `InstanceName`), `manifest_sha256` (opsional), `members: Vec<EnvironmentMember { alias, instance_id, ownership: dedicated atau shared }>` | N:M Instance lewat member, 1:N Lease |
| Lease | `LeaseId` | `instance_id`, `holder: environment(EnvironmentId), manual, atau startup`, `session: Option<SessionId>`, `state: active, suppressed { at }, atau released { at }`, `acquired_at` | N:1 Instance, N:1 Environment |
| Operation | `OperationId` | `kind: OperationKind`, `resource: runtime(RuntimeId), instance(InstanceId), environment(EnvironmentId), atau manager`, `idempotency_key` (opsional), `state: OperationState`, `started_at` (opsional, diisi saat pertama kali `start`), `stage` (opsional), `progress: indeterminate atau counted { done, total }`, `failure: Option<OperationFailure { code: FailureCode, retryable }>`, `created_at`, `updated_at` | N:1 resource |
| InstanceManifest | `instance_id` | lihat *Metadata folder instance* | cermin Instance di disk |

Tipe pendukung:

- `InstallMode`: `Managed { artifact_sha256 }` (path diturunkan layout) atau `External { root: PathBuf, observed_sha256: Option<Sha256Digest> }` (path milik pengguna, BR-019).
- `DesiredState`: `running`, `stopped`. `StartupPolicy`: `manual`, `at_login`.
- `PortRequest`: `Fixed(Port)` atau `Auto`. Port efektif sebuah endpoint IP adalah `bound` kalau ada, selain itu port `Fixed`; endpoint `Auto` yang belum di bind tidak punya port efektif. `bound` hanya diisi setelah bind dan readiness berhasil (REQ-010, fitur 10).
- `Capability` (provider): `reload`, `online_backup`, `cold_backup`, `logical_backup`, `tls`, `metrics` (`#[non_exhaustive]`). `CapabilitySet::require(cap)` mengembalikan `InvariantError` BR-016 `Unsupported { capability }`.
- `LineCompatibility`: peta `data line → runtime line yang boleh`, dari manifest provider (fitur 9). Kosong berarti hanya line yang sama (BR-009).
- `RuntimeReferences { instances: Vec<InstanceId>, dependent_runtimes: Vec<RuntimeId>, active_operations: Vec<OperationId>, recovery_snapshots: Vec<BackupId> }`, diisi pemanggil dari registry; `instances` mencakup instance di semua lifecycle, termasuk `stopped` dan `retained`. `RemovalBlocker` adalah satu varian per jenis referensi itu, dengan ID-nya. `RemovalPlan`: `DeleteManagedFiles` atau `UnregisterOnly`.
- `AcceptedRevisionChange::new(from: ProviderRevisionKey, to: ProviderRevisionKey, accepted_diff_sha256: Sha256Digest)`: digest dari diff config yang ditampilkan dan disetujui pengguna (dihitung fitur 59). `Instance::apply_provider_revision` menolak kalau `from` bukan revision saat ini atau `to.provider` berbeda.
- `Instance::record_data_migration(operation: &Operation, line, version)`: hanya diterima kalau `operation.kind` adalah `migrate_data`, state-nya `running`, dan resource-nya instance ini.
- `OperationKind` (`#[non_exhaustive]`): `install_runtime`, `register_external_runtime`, `remove_runtime`, `create_instance`, `start_instance`, `stop_instance`, `restart_instance`, `reload_instance`, `rename_instance`, `remove_instance`, `purge_instance`, `rebind_runtime`, `update_provider_revision`, `backup_instance`, `restore_instance`, `environment_up`, `environment_down`, `update_manager` (resource `manager`), `migrate_data`. `deletes_data()` hanya benar untuk `purge_instance` (BR-006).
- Aturan Environment: alias unik per environment, satu instance paling banyak sekali per environment, dan `owned_instances()` hanya mengembalikan member `dedicated` (member `shared` tidak pernah menjadi pemilik data).

**Error**:

- `InvalidId` (`Malformed`, `WrongVersion(usize)`) dan `ValueError { kind: ValueKind, reason: &'static str }` untuk value type.
- `TransitionError { machine: Machine, from: &'static str, event: &'static str }` dengan `Machine` bernilai `runtime`, `instance_lifecycle`, atau `operation`. Pesannya misalnya `instance lifecycle cannot apply start_begin in state running`.
- `InvariantError { rule: Option<Rule>, detail: InvariantDetail }`. `Rule` berisi `Br001` sampai `Br025` dengan `id()` yang mengembalikan `"BR-001"` dan seterusnya. Detail yang bukan pelanggaran BR (misalnya `HealthWithoutProcess` atau `RebindRequiresStopped`) memakai `rule: None`. Semua error domain punya `rule() -> Option<Rule>`.
- `ProgressError`: `OutOfRange { done, total }` atau `NotAllowedInState { state }`.
- Semua enum error `#[non_exhaustive]`, pesan lowercase tanpa titik di akhir.

### State machine

Setiap machine adalah `enum` state, `enum` event yang membawa datanya sendiri, dan satu `apply(event) -> Result<State, TransitionError>`. Pasangan yang tidak ada di tabel adalah error, bukan no op; use case yang ingin idempoten (misalnya stop pada instance yang sudah `stopped`) memeriksa state sendiri sebelum memanggil `apply`. Event dengan payload: `InitFailed { reason: FailureCode }`, `StartFailed { reason }`, `Crashed { will_retry: bool, reason }`, `Interrupted { resumable: bool }`, `Fail(OperationFailure)`, `Resolve(Outcome)` dengan `Outcome` bernilai `succeeded`, `failed`, atau `cancelled`.

**State runtime** (`RuntimeState`, kode PRD bagian 19.1):

| Dari | Event | Ke |
| --- | --- | --- |
| `absent` | `resolve` | `resolving` |
| `resolving` | `resolved` (install managed) | `downloading` |
| `resolving` | `resolved` (install external) | `verifying` |
| `downloading` | `downloaded` | `verifying` |
| `verifying` | `verified` | `installing` |
| `installing` | `committed` | `installed` |
| `resolving`, `downloading`, `verifying`, `installing` | `failed` | `failed` |
| `failed` | `retry` | `resolving` |
| `installed` | `observed_missing` | `missing` |
| `installed` | `observed_drift` | `drifted` |
| `missing`, `drifted` | `reverified` | `installed` |
| `missing`, `drifted` | `retry` | `resolving` |
| `failed`, `installed`, `missing`, `drifted` | `removed` | `absent` |

- Runtime hanya dibuat setelah selector diselesaikan menjadi identitas exact (software, versi, line, target, install mode), jadi aggregate lahir di `resolving`. `absent` adalah state setelah `removed`: record masih ada sampai registry membuangnya, dan `resolve` memasang ulang identitas yang sama.
- `Runtime::apply(event)` membaca managed atau external dari `self.install`, jadi event tidak membawa install mode.
- `Runtime::remove(refs)` memanggil `plan_runtime_removal` (BR-007, BR-019) sebelum menerapkan `removed`. Untuk runtime external, `installing` berarti mencatat registrasi; tidak ada file yang disalin.

**Lifecycle instance** (`InstanceLifecycle`):

| Dari | Event | Ke |
| --- | --- | --- |
| `uninitialized` | `init_begin` | `initializing` |
| `initializing` | `init_succeeded` | `stopped` |
| `initializing` | `init_failed` | `failed` |
| `stopped`, `failed` | `start_begin` | `starting` |
| `starting`, `recovering` | `spawned` | `running` |
| `starting` | `start_failed` | `failed` |
| `running`, `recovering` | `crashed` (`will_retry` true) | `recovering` |
| `running`, `recovering` | `crashed` (`will_retry` false) | `failed` |
| `starting`, `running`, `recovering` | `stop_begin` | `stopping` |
| `stopping` | `exited` | `stopped` |
| `starting`, `running`, `stopping`, `recovering` | `identity_lost` | `unknown` |
| `unknown` | `adopted` | `running` |
| `unknown` | `confirmed_stopped` | `stopped` |
| `uninitialized`, `stopped`, `failed` | `retain` | `retained` |

- Tabel di atas adalah machine murni. Aturan aggregate `Instance::apply_lifecycle(event, at)` di atasnya: `start_begin` ditolak dengan `InvariantError` BR-008 `DataNotInitialized` selama `data_initialized_at` kosong (instance yang init-nya gagal tidak pernah di start; jalannya purge atau buat ulang); `init_succeeded` mengisi `data_initialized_at`; `init_failed`, `start_failed`, dan `crashed` mengisi `last_failure` dengan reason dari event; `spawned` dan `adopted` mengosongkannya.
- `retained` adalah state akhir; purge menghapus record, bukan state baru (fitur 54).
- `initializing` yang terputus karena daemon mati diselesaikan reconciliation dengan `init_failed { reason: interrupted }`, tidak pernah dengan init ulang (BR-008).
- Stop yang melewati timeout tetap `stopping`; force stop adalah aksi eksplisit pengguna (fitur 26) yang berakhir dengan `exited`. `unknown` tidak menerima `stop_begin`, karena proses tanpa identitas terbukti tidak boleh disinyal (REQ-011).
- `DesiredState` disimpan terpisah dan tidak pernah diubah oleh `apply`.

**Readiness dan health** (`Readiness`, `Health`; masing masing menyimpan state, `observed_at`, `reason: ReasonCode`, dan `detail` opsional maksimal 256 karakter tanpa secret):

| Aturan | Readiness (`unknown`, `not_ready`, `ready`) | Health (`unknown`, `healthy`, `degraded`, `unhealthy`) |
| --- | --- | --- |
| Nilai awal saat instance dibuat | `unknown`, reason `never_observed` | `unknown`, reason `never_observed` |
| Observasi baru | state mana pun ke state mana pun | state mana pun ke state mana pun |
| Observasi dengan `at` lebih lama dari `observed_at` | diabaikan, hasil `Ignored::Stale` | diabaikan, hasil `Ignored::Stale` |
| Syarat lifecycle | `ready` hanya saat `running`, selain itu `InvariantError` BR-010 `ReadyWithoutProcess` | selain `unknown` hanya saat `running`, selain itu `InvariantError` `HealthWithoutProcess` (tanpa BR); tidak pernah mengubah lifecycle atau desired state |
| Lifecycle masuk `running` (`spawned`, `adopted`) | `not_ready`, reason `awaiting_probe` | `unknown`, reason `awaiting_probe` |
| Lifecycle keluar dari `running` | `unknown`, reason `not_running` | `unknown`, reason `not_running` |
| Dibaca dengan `effective(now, max_age)` | nilai selain `unknown` yang lebih tua dari `max_age` dibaca `unknown` reason `stale` | sama (PRD bagian 19.5, kesegaran) |

Nilai `effective` dihitung saat dibaca dan tidak disimpan. `Instance::apply_lifecycle` yang menegakkan kaitan lifecycle dengan readiness dan health. Observasi tidak menaikkan `revision`.

**State operation** (`OperationState`, PRD bagian 18.2; mesinnya di domain, engine journal, lock, dan idempotency di fitur 7):

| Dari | Event | Ke |
| --- | --- | --- |
| `queued` | `start` | `running` |
| `queued` | `fail` | `failed` |
| `queued` (`started_at` kosong) | `request_cancel` | `cancelled` |
| `queued` (`started_at` terisi) | `request_cancel` | `cancel_requested` |
| `running` | `succeed` | `succeeded` |
| `running` | `fail` | `failed` |
| `running` | `request_cancel` | `cancel_requested` |
| `running` | `interrupted` (`resumable` true) | `queued` |
| `running` | `interrupted` (`resumable` false) | `needs_recovery` |
| `cancel_requested` | `cancelled` | `cancelled` |
| `cancel_requested` | `succeed` | `succeeded` |
| `cancel_requested` | `fail` | `failed` |
| `cancel_requested` | `interrupted` (nilai apa pun) | `needs_recovery` |
| `running`, `cancel_requested` | `compensation_failed` | `needs_recovery` |
| `needs_recovery` | `resume` | `queued` |
| `needs_recovery` | `resolve` (succeeded, failed, atau cancelled) | state yang disebut |

`succeeded`, `failed`, dan `cancelled` adalah state akhir. `start` mengisi `started_at` kalau masih kosong. `resume` hanya dipakai engine untuk operasi yang menyatakan bisa dilanjutkan (fitur 7). Progress hanya boleh diubah saat `running` atau `cancel_requested`, dan `done ≤ total`; pelanggaran menghasilkan `ProgressError`.

### Aturan lease dan demand

- `Lease::new` menolak holder `environment` dengan session, dan holder `startup` tanpa session. Holder `manual` boleh dengan atau tanpa session.
- `release(at)`: `active` atau `suppressed` menjadi `released`. `suppress(at)`: hanya lease `startup` yang `active`.
- `evaluate_stop(instance, leases, request) -> StopDecision`:
  - `EnvironmentDown(env)`: lease `active` milik env itu dilepas. Kalau masih ada lease `active` lain dengan holder apa pun (environment lain, manual, atau startup), hasilnya `KeepRunning { release, remaining }` (BR-014); kalau tidak, `Stop { release, suppress: [], overridden: [] }`.
  - `Manual { session, override_leases }`: kalau ada lease `active` berholder environment dan `override_leases` false, hasilnya `RequiresOverride { blocking }` (BR-015). Selain itu `Stop { release: semua lease manual yang active di instance ini (session mana pun), suppress: lease startup yang active milik session itu, overridden: lease environment yang active }`. Lease environment yang di override tetap `active`; desired state `stopped` yang mencegah reconciler menyalakannya lagi.
- `should_autostart(policy, leases, session)` benar hanya kalau policy `at_login` dan ada lease `startup` `active` untuk session itu. Lease yang `suppressed` membuatnya false sampai session berikutnya membuat lease startup baru (BR-024, fitur 31).

### Invariant BR-001 sampai BR-025

`F4` berarti spec ini membangun fungsi dan test-nya sekarang. `Dipetakan` berarti aturannya belum punya input nyata di fitur ini; tipe domain yang membuatnya bisa dinyatakan disebut, dan fitur pemiliknya yang membangun pemeriksaannya.

| BR | Status | Tempat dan fungsi | Test (evidence `fixture`, `covers` = ID BR) | Pemilik alur penuh |
| --- | --- | --- | --- | --- |
| BR-001 | F4 | domain: `Instance` hanya menyimpan `RuntimeId` dan `ProviderRevisionKey`, tidak ada tipe selector di instance; runtime hanya berubah lewat `rebind` | `instance_keeps_its_exact_runtime_and_revision_until_rebind` | fitur 20, 21 |
| BR-002 | F4 | core: `check_data_root_overlap` membandingkan identitas file (device, inode) di sepanjang ancestor, jadi alias symlink, beda huruf di APFS, dan beda normalisasi Unicode tetap terdeteksi | `data_roots_that_are_equal_nested_or_aliased_are_rejected` (alias symlink di semua OS, beda huruf hanya di macOS) | fitur 20, 57 |
| BR-003 | F4 | domain: `Instance::rename` hanya mengubah `name`; path diturunkan dari ID | `renaming_keeps_id_runtime_and_paths` | fitur 29 |
| BR-004 | F4 | domain: `Endpoint`, `endpoints_conflict`: konflik kalau transport sama, port efektif sama, dan alamat bertumpuk (sama, `0.0.0.0` dengan IPv4 apa pun, `::` dengan alamat apa pun, `::ffff:a.b.c.d` sama dengan `a.b.c.d`); TCP dan UDP tidak pernah konflik satu sama lain; endpoint Unix tidak pernah konflik dengan endpoint IP, dan nama socket unik per instance | `endpoint_conflicts_follow_address_transport_and_port` | fitur 20, 10 (cek bind OS, VAL-006) |
| BR-005 | F4 | domain: `resolve_port(endpoint, candidate: Option<Port>, others: &[Endpoint])`: `Fixed` yang konflik dengan port efektif endpoint lain menghasilkan `FixedPortConflict` tanpa pengganti; `Auto` memakai kandidat dari pemanggil, atau `AutoPortUnavailable` kalau kandidat kosong atau konflik | `fixed_port_conflict_is_an_error_and_never_substituted` | fitur 20 |
| BR-006 | F4 | domain: `OperationKind::deletes_data` hanya untuk purge; `retain` tidak menyentuh data | `only_purge_deletes_data` | fitur 54, 47 |
| BR-007 | F4 | domain: `plan_runtime_removal(runtime, refs: &RuntimeReferences)` menolak dengan `RemovalBlocked { blockers }` selama ada instance (termasuk `stopped` dan `retained`), runtime yang bergantung, operasi aktif, atau snapshot recovery | `runtime_in_use_cannot_be_removed` | fitur 55 |
| BR-008 | F4 | core: `create_instance_layout` hanya membuat folder baru (create tanpa replace) dan menolak folder yang sudah ada; domain: `start_begin` ditolak sebelum `init_succeeded` | `create_refuses_an_existing_instance_folder`, `start_is_refused_until_data_is_initialized` | fitur 20, 57 |
| BR-009 | F4 | domain: `Instance::rebind(runtime_id, software, line, compat)` hanya saat `stopped`, untuk software yang sama, dan menolak line berbeda kecuali `LineCompatibility` mengizinkan | `runtime_of_another_line_is_rejected_on_existing_data` | fitur 58, 19 |
| BR-010 | F4 | domain: aturan readiness di atas dan `Readiness::effective` | `running_process_alone_is_not_ready`, `stale_ready_reads_as_unknown` | fitur 21, 28 |
| BR-011 | Dipetakan | sudah dijaga `check-deps` (CLI tidak bergantung pada core atau storage) | tidak ada test baru | fitur 6 |
| BR-012 | Dipetakan | kebijakan, ditulis di Security model | tidak ada | fitur 63, 66 |
| BR-013 | F4 | domain: empat operasi punya varian `OperationKind` sendiri, dan `data_line` hanya berubah lewat `Instance::record_data_migration` yang mewajibkan operasi `migrate_data` | `only_data_migration_changes_the_data_line` | fitur 18, 59, 61 |
| BR-014 | F4 | domain: `evaluate_stop(EnvironmentDown)` | `environment_down_keeps_a_shared_instance_with_other_leases` | fitur 47 |
| BR-015 | F4 | domain: `evaluate_stop(Manual)` | `manual_stop_needs_override_when_an_environment_holds_a_lease` | fitur 26, 47 |
| BR-016 | F4 | domain: `CapabilitySet::require` | `unsupported_capability_is_reported_not_attempted` | fitur 6, 8, 13 (konsistensi tampilan), 9 (pemetaan manifest) |
| BR-017 | F4 | domain: `ProviderRevision` immutable; revision instance hanya berubah lewat `apply_provider_revision(AcceptedRevisionChange)` | `provider_revision_changes_only_with_an_accepted_diff` | fitur 59 |
| BR-018 | Dipetakan | domain tidak menyimpan nilai secret, dan manifest instance tidak punya field secret (Key invariants) | tidak ada test di fitur ini | fitur 11 |
| BR-019 | F4 | domain: `plan_runtime_removal` untuk runtime external hanya menghasilkan `UnregisterOnly`, tidak pernah menghapus file | `external_runtime_removal_never_deletes_files` | fitur 55, 56 |
| BR-020 | Dipetakan | kebijakan, ditulis di Security model | tidak ada | fitur 37, 66 |
| BR-021 | Dipetakan | `Target` dan `native_support` dipakai untuk cek kompatibilitas restore | tidak ada | fitur 51 |
| BR-022 | Dipetakan | tidak ada tipe di fitur ini | tidak ada | fitur 49, 51 |
| BR-023 | F4 (sebagian) | core: `create_instance_layout` tidak pernah menghapus; error parsial membawa daftar persis path yang dibuat; manifest mencatat `createdByOperation` | `failed_create_reports_only_its_own_paths_and_deletes_nothing` | fitur 7 (kompensasi) |
| BR-024 | F4 | domain: `Lease::suppress`, `should_autostart` | `explicit_stop_suppresses_autostart_for_the_session` | fitur 31 |
| BR-025 | Dipetakan | tidak ada tipe di fitur ini | tidak ada | fitur 18 (compatibility record), 65 |

Test lain yang menulis evidence (semuanya `fixture`, termasuk test di filesystem asli, sesuai spec 0002):

| Test | `covers` | Crate |
| --- | --- | --- |
| `runtime_and_lifecycle_tables_are_exhaustive`, `readiness_and_health_follow_the_lifecycle` | `REQ-012` | `stackly-domain` |
| `operation_table_is_exhaustive` | `REQ-054` | `stackly-domain` |
| `inventory_reads_folders_without_the_registry` | `REQ-053` | `stackly-core` |
| `windows_capabilities_report_not_supported` | `AC-050`, `REQ-050` | `stackly-platform` |
| `windows_native_support_needs_a_windows_artifact` | `AC-074` | `stackly-domain` |
| `core_use_cases_run_on_the_fake_platform` | `NFR-010` | `stackly-core` |

### Port platform

Semua trait di `stackly-platform` sync, object safe, dan `Send + Sync`. Fitur 6, 10, 11, 15, dan 28 menambah trait mereka sendiri lewat update spec ini.

```rust
pub trait DirectoryResolver: Send + Sync {
    fn roots(&self) -> Result<RootSet, PlatformError>;
}

pub trait Filesystem: Send + Sync {
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, PlatformError>;
    fn symlink_metadata(&self, path: &Path) -> Result<Option<FileInfo>, PlatformError>;
    fn create_dir_private(&self, path: &Path) -> Result<(), PlatformError>;
    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntryInfo>, PlatformError>;
    fn read_file_limited(&self, path: &Path, max_len: u64) -> Result<Vec<u8>, PlatformError>;
    fn write_file_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), PlatformError>;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> jiff::Timestamp;
}

pub trait UserIdentity: Send + Sync {
    fn current_user(&self) -> Result<UserId, PlatformError>;
}

pub trait PlatformInfo: Send + Sync {
    fn target(&self) -> Target;
    fn capabilities(&self) -> PlatformCapabilities;
}
```

- `symlink_metadata` tidak mengikuti symlink dan mengembalikan `None` kalau path tidak ada. `FileInfo { kind: file, dir, symlink, atau other; len; identity: FileIdentity { device, inode }; owner: Option<UserId>; mode: Option<u32> }`.
- `create_dir_private` membuat satu tingkat folder dengan mode 0700 dan gagal dengan `AlreadyExists` kalau path sudah ada, termasuk symlink.
- `read_dir` mengembalikan nama dan `FileInfo` setiap entri tanpa mengikuti symlink.
- `read_file_limited` membuka tanpa mengikuti symlink di komponen terakhir (`O_NOFOLLOW`), gagal dengan `NotARegularFile` kalau yang terbuka bukan file biasa (dicek lewat `fstat`, jadi FIFO tidak pernah dibaca), dan gagal dengan `FileTooLarge` di atas `max_len`.
- `write_file_atomic` menulis ke `.<nama file>.tmp-<pid>-<counter>` di folder yang sama dengan mode 0600, `sync_all` (di macOS std memakai `F_FULLFSYNC`), rename ke tujuan, lalu `sync_all` folder. Kalau satu langkah gagal, ia menghapus file sementaranya sendiri sebelum mengembalikan error; file itu tidak pernah dihitung sebagai path yang dibuat.
- `UserId`: `Unix(u32)` atau `Windows(String)`, `#[non_exhaustive]`.
- `PlatformCapability`: `directories`, `filesystem`, `clock`, `user_identity`, `process_supervision` (fitur 10), `secret_store` (fitur 11), `ipc_endpoint` (fitur 6), `background_agent` (fitur 15), `process_metrics` (fitur 28). Namanya berbeda dari `Capability` provider di domain. `PlatformCapabilities::status(cap)` mengembalikan `Supported` atau `NotSupported { reason: &'static str }`.
- `SystemClock` (memakai `jiff::Timestamp::now()`) dan `host_target()` (dari `std::env::consts::{OS, ARCH}`) ada di `stackly-platform` karena hanya memakai std yang portabel.
- `Platform` adalah struct `#[non_exhaustive]` yang memegang kelima trait sebagai `Arc<dyn Trait>`, dibuat dengan `Platform::new(directories, fs, clock, identity, info)` dan dibaca lewat getter. `stacklyd` merangkainya sekali; use case async di fitur 6 dan 7 memanggil use case core lewat `spawn_blocking`.

`PlatformError` (`#[non_exhaustive]`): `NotSupported { capability, os }`, `HomeDirUnavailable`, `TempDirUnavailable`, `InvalidRoots { reason }`, `RuntimeRootTooLong { path, len, max }`, `InvalidRootOverride { path, reason }`, `NotFound { path }`, `AlreadyExists { path }`, `PermissionDenied { path }`, `NoSpace { path }`, `NotARegularFile { path }`, `FileTooLarge { path, max }`, `Io { op, path, kind, message }`, dan `OutsideTestRoots { path }` (hanya dihasilkan fake di testkit).

### Layout direktori per user

`RootSet` memegang empat owned root: `data`, `logs`, `cache`, dan `runtime`. `RootSet::new` menolak path relatif, root yang sama atau bertumpuk satu sama lain secara leksikal, dan runtime root lebih dari `RUNTIME_ROOT_MAX` byte. `prepare_layout` mengulang cek tumpuk itu pada bentuk canonical. `DirectoryLayout::new(roots)` menurunkan semua path lain tanpa IO; komponen path berasal dari value type domain yang sudah tervalidasi, jadi fungsinya tidak bisa gagal.

| Sumber root | `data` | `logs` | `cache` | `runtime` |
| --- | --- | --- | --- | --- |
| macOS default | `<home>/Library/Application Support/Stackly` | `<home>/Library/Logs/Stackly` | `<home>/Library/Caches/Stackly` | `<darwin user temp>/stackly` |
| `STACKLY_ROOT=R` (macOS) | `R/data` | `R/logs` | `R/cache` | `<darwin user temp>/stackly-<fnv8(R)>` |
| `TestRoot` | `<root>/data` | `<root>/logs` | `<root>/cache` | `<run folder pendek>/stackly-<fnv8(root)>` |

`<home>` adalah `pw_dir` dari `getpwuid_r(getuid())`, `<darwin user temp>` adalah hasil `confstr(_CS_DARWIN_USER_TEMP_DIR)` (folder 0700 yang dibuat OS untuk user itu, sekitar 49 byte), dan `fnv8(R)` adalah 8 hex huruf kecil dari 32 bit bawah hash FNV-1a 64 bit atas byte path canonical `R`. Baris kedua dan ketiga dihasilkan fungsi yang sama, `RootSet::under_override(root, runtime_base)`.

```text
<data>/
├── stacklyd.lock                      # lock satu daemon per user (fitur 6)
├── registry/                          # stackly.sqlite (fitur 5)
├── providers/<namespace>/<name>/<revision>/<manifest-sha256>/
├── runtimes/<software>/<version>/<os>-<arch>/<artifact-sha256>/
├── instances/<instance-id>/
│   ├── instance-manifest.json
│   ├── data/
│   ├── config/
│   ├── config-history/
│   └── temp/
├── environments/<environment-id>/
├── operations/
├── backups/
└── retained/<instance-id>/            # pohon yang sama dengan instances/<instance-id>/
<logs>/
├── daemon/
└── instances/<instance-id>/
<cache>/
└── downloads/
<runtime>/                             # hanya berisi file yang bisa dibuat ulang
├── stacklyd.sock                      # fitur 6
└── i/<short-key>/                     # socket dan file PID per instance (fitur 10)
```

Lock daemon sengaja ada di root data, bukan di runtime root: OS bisa membersihkan runtime root saat daemon masih hidup, dan lock yang hilang akan membiarkan daemon kedua menyala.

Fungsi `DirectoryLayout`: `owned_roots()`, `daemon_lock()`, `registry_dir()`, `registry_db()`, `providers_dir()`, `provider_revision_dir(&ProviderRevisionKey)`, `runtimes_dir()`, `runtime_dir(&SoftwareName, &ExactVersion, Target, &Sha256Digest)`, `instances_dir()`, `instance_paths(InstanceId) -> InstancePaths`, `retained_dir()`, `retained_instance_paths(InstanceId) -> InstancePaths`, `environments_dir()`, `environment_dir(EnvironmentId)`, `operations_dir()`, `backups_dir()`, `daemon_log_dir()`, `instance_log_dir(InstanceId)`, `downloads_dir()`, `daemon_socket()`, `instance_runs_dir()`, `instance_run_dir(ShortKey)`. `InstancePaths` memuat `root`, `manifest`, `data`, `config`, `config_history`, `temp`, `logs`, dan `run`.

Anggaran path socket (konstanta di `layout`):

| Konstanta | Nilai | Asal |
| --- | --- | --- |
| `SOCKET_PATH_MAX` | 103 byte | `sun_path` macOS 104 byte termasuk NUL; Linux 108, jadi angka macOS yang mengikat |
| `SOCKET_NAME_MAX` | 15 byte | nama socket provider terpanjang hari ini `.s.PGSQL.65535` (14 byte); fitur 9 menolak nama yang lebih panjang |
| `RUNTIME_ROOT_MAX` | 76 byte | `103 − len("/i/") − 8 − len("/") − 15`; socket daemon (`/stacklyd.sock`, 14 byte) muat dengan sisa |

Path socket memakai bentuk runtime root apa adanya (`/var/folders/...`, bukan `/private/var/folders/...`) supaya tetap pendek; cek containment memakai bentuk canonical yang disimpan `PreparedLayout`. Contoh di Mac uji: runtime root default 56 byte, runtime root override 65 byte, `TestRoot` sekitar 40 byte. Proses yang berjalan di App Sandbox mendapat temp dir container yang jauh lebih panjang dan akan ditolak, karena itu app tetap tanpa sandbox (spec 0001).

### Root alternatif

- Hanya bin yang membaca environment: `stacklyd` (spec ini) dan `stackly` (fitur 8) membaca `STACKLY_ROOT` sekali di `main` sebagai `OsString`, lalu memanggil `MacosPlatform::new(root_override)`. Variabel yang tidak ada berarti root default; variabel yang ada tetapi kosong ditolak (`InvalidRootOverride` reason `empty`), supaya salah setel tidak diam diam kembali ke data asli. Library tidak pernah membaca environment (dijaga `check-deps`). Nama variabel ada sebagai konstanta `ROOT_OVERRIDE_ENV` di `stackly-platform`.
- `validate_root_override(path, fs, identity, default_data_root)` (portabel, di `stackly-platform`) menerima root kalau: absolut; ada; folder dan bukan symlink pada komponen terakhir; dimiliki user yang sedang berjalan; `mode & 0o022 == 0`; dan path canonical-nya tidak sama dengan atau berada di dalam root data default (dibandingkan lewat identitas file kalau root default ada). Hasilnya path canonical. Setiap penolakan memakai `InvalidRootOverride { path, reason }` dengan reason seperti `empty`, `not absolute`, `does not exist`, `is a symlink`, `owned by another user`, `writable by group or others`, atau `inside the default data root`.
- Dengan override, semua root (termasuk log dan cache) pindah. Tidak ada mode campuran.
- `MacosPlatform::with_roots(RootSet)` merangkai adapter macOS dengan root set yang diberikan pemanggil, tanpa resolver default. Dipakai integration test supaya tidak ada folder yang tertinggal di temp dir user asli.
- `stacklyd` mencatat `data_root`, `logs_root`, `cache_root`, `runtime_root`, dan `root_override` (true atau false) di baris log start, lalu keluar 0 seperti sekarang. Kalau resolver atau validasi override gagal, ia mencatat error dengan path dan alasannya lalu keluar 1. Pemanggilan `prepare_layout` saat start adalah bagian fitur 6.

### Metadata folder instance

`instance-manifest.json` di root setiap folder instance, JSON UTF-8 camelCase, schema `stackly.instance-manifest/v1`. Tipe `InstanceManifest` ada di domain dengan derive serde; encode dan decode JSON terjadi di core. Registry tetap sumber kebenaran; manifest adalah cermin supaya folder bisa dikenali tanpa registry (inventaris, adoption di fitur 57, pemulihan di fitur 48 dan 62).

| Field | Tipe | Wajib | Sumber | Catatan |
| --- | --- | --- | --- | --- |
| `schema` | string | ya | konstanta | `stackly.instance-manifest/v1` |
| `instanceId` | UUID v7 | ya | ID instance (dibuat daemon) | harus sama dengan nama folder |
| `name` | `InstanceName` | ya | input create, lalu rename | |
| `displayName` | string atau null | tidak | input | |
| `software` | `SoftwareName` | ya | provider revision | |
| `provider` | `{ name, revision, manifestSha256 }` | ya | `ProviderRevisionKey` | |
| `runtime` | `{ id, version, line, os, arch, installMode, artifactSha256 }` | ya | record runtime | `artifactSha256` null untuk external |
| `data` | `{ initializedLine, initializedVersion, initializedAt }` | ya | line dan versi runtime saat create; `initializedAt` dari `Clock` saat `init_succeeded` | `initializedAt` null sampai init berhasil; line dan versi hanya berubah lewat `migrate_data` (BR-009, BR-013) |
| `createdAt` | RFC 3339 UTC | ya | `Clock` saat create | |
| `createdByOperation` | UUID v7 | ya | operasi create | dasar kompensasi fitur 7 (BR-023) |
| `updatedAt` | RFC 3339 UTC | ya | `Clock` setiap tulis | |
| `stacklyVersion` | string | ya | `stackly_core::daemon_info().version` saat tulis | |
| `retained` | `{ retainedAt, byOperation }` atau null | tidak | operasi remove (fitur 54) | wajib ada di `retained/`, wajib null di `instances/` |

- Tidak ada secret, endpoint, desired state, config, atau path di manifest. Path diturunkan dari layout dan ID, jadi pindah ke `retained/` tidak mengubah isi path. Short key juga diturunkan dari ID.
- Ditulis lewat `write_file_atomic`, `to_vec_pretty` plus newline di akhir, maksimal 64 KiB.
- Ditulis ulang saat create dan setelah `init_succeeded` (fitur 20), rename (fitur 29), rebind (fitur 58), apply provider revision (fitur 59), dan retain (fitur 54).
- Pembaca mendecode field `schema` saja lebih dulu; schema lain menghasilkan `UnsupportedSchema` tanpa mencoba decode sisanya. Field opsional baru boleh ditambahkan tetap di `v1`, dan pembaca mengabaikan field yang tidak dikenal. Perubahan lain menaikkan versi ke `v2`.

### Fake platform

`stackly-testkit` (dependency dev saja, sesuai spec 0001):

- `TestRoot` tetap membuat folder di temp dir sistem, dan sekarang juga membuat runtime folder pendek dengan prefix `skt-` lewat `tempfile` di `/tmp` (canonical `/private/tmp` di macOS) pada Unix, mode 0700, yang ikut dihapus saat drop tanpa mengikuti symlink. Prefix `skt-` dipilih supaya tidak pernah tersentuh pembersihan spike spec 0003, yang mematikan proses dan menghapus folder `/tmp/sk-*`. `TestRoot::roots()` memanggil `RootSet::under_override(path, run_dir)`, dan `TestRoot::resolver()` mengembalikan resolver yang memberi root set itu. `FakeDirectoryResolver` lama dan `app_support_dir()` dihapus.
- `FakeFilesystem`: `std::fs` sungguhan, tetapi untuk setiap path argumen, parent canonical ditambah nama komponen terakhir apa adanya harus berada di bawah salah satu root milik test itu; hasil `canonicalize` juga dicek setelah dipanggil. Kalau tidak, operasi gagal dengan `OutsideTestRoots { path }` dan penghitung `violations()` naik. Test yang butuh symlink "ke luar" menaruh targetnya di folder lain di dalam test root. `fail_next(op, kind)` dan `fail_when(op, path_suffix, kind)` membuat operasi tertentu gagal dengan `NoSpace`, `PermissionDenied`, atau `Io`. `mutations()` menghitung panggilan `create_dir_private` dan `write_file_atomic`.
- `FakeClock`: mulai `2026-01-01T00:00:00Z`, dengan `set(ts)` dan `advance(duration)`.
- `FakeIdentity`: default uid pemilik folder `TestRoot` (dibaca dari metadata, tanpa libc); `set_current(UserId)` mensimulasikan user lain.
- `FakePlatformInfo`: target `macos-aarch64`, capability seperti `MacosPlatform`, dan `with_capability(cap, status)` untuk menimpa.
- `FakePlatform::new(&TestRoot)` merangkai semuanya dan memberi `platform() -> Platform` serta handle `clock()`, `fs()`, dan `identity()`.
- `fs_conformance(fs: &dyn Filesystem, root: &TestRoot)` menjalankan satu set pemeriksaan perilaku yang sama untuk implementasi `Filesystem` apa pun: mode 0700 dan 0600, `AlreadyExists` untuk symlink, `symlink_metadata` tanpa follow, `NotARegularFile` untuk FIFO, batas `read_file_limited`, dan tidak ada file sementara yang tertinggal setelah tulis gagal. Testkit menjalankannya untuk `FakeFilesystem`, dan `stackly-platform-macos` untuk `MacosFilesystem`.
- Test `validate_root_override` ada di testkit (dengan `FakeIdentity` untuk kasus user lain) dan di integration test platform-macos (filesystem asli). Test di dalam `stackly-platform` sendiri tidak memakai testkit, supaya crate itu tidak terkompilasi dua kali.

### Windows dan capability

- `UnsupportedPlatform::new(target)` di `stackly-platform` mengembalikan `Platform` yang `DirectoryResolver`, `Filesystem`, dan `UserIdentity`-nya selalu `NotSupported { capability, os }`, dengan `SystemClock` dan info yang melaporkan target serta capability: `clock` supported, delapan lainnya not supported dengan reason `windows adapter not built`. `stackly-platform-windows` tetap baru dibuat di fase Windows (spec 0001).
- `MacosPlatform` di fitur ini: `directories`, `filesystem`, `clock`, dan `user_identity` supported; `process_supervision`, `secret_store`, `ipc_endpoint`, `background_agent`, dan `process_metrics` not supported dengan reason `not built yet (feature 10)` dan seterusnya. Setiap fitur itu mengubah statusnya sendiri saat dibangun.
- `native_support(artifact_targets: &[Target], host: Target) -> bool` di domain hanya mencocokkan os dan arch persis (AC-074).

### API surface

Semua use case core adalah fungsi sync yang menerima `&Platform`. Pemanggilnya selalu daemon (spec 0001: hanya daemon yang menulis dan membuat path); fake di testkit memanggilnya di test.

| Fungsi | Tempat | Input utama | Output utama | Pemanggil | Error utama |
| --- | --- | --- | --- | --- | --- |
| `DirectoryResolver::roots` | platform (trait) | tidak ada | `RootSet` | adapter | `HomeDirUnavailable`, `TempDirUnavailable`, `RuntimeRootTooLong`, `NotSupported` |
| `RootSet::new`, `RootSet::under_override` | platform | path absolut | `RootSet` | adapter, testkit | `InvalidRoots`, `RuntimeRootTooLong` |
| `DirectoryLayout::*` | platform | `RootSet`, ID dan value type domain | `PathBuf`, `InstancePaths` | core, daemon | tidak ada (murni) |
| `validate_root_override` | platform | path, `&dyn Filesystem`, `&dyn UserIdentity`, root data default | path canonical | adapter saat start | `InvalidRootOverride` |
| `MacosPlatform::new` | platform-macos | `Option<OsString>` dari `STACKLY_ROOT` | `Platform` | `stacklyd`, `stackly` | error resolver dan validasi override |
| `MacosPlatform::with_roots` | platform-macos | `RootSet` | `Platform` | integration test | tidak ada |
| `UnsupportedPlatform::new` | platform | `Target` | `Platform` | bin di OS tanpa adapter, test | tidak ada |
| `fs_conformance` | testkit | `&dyn Filesystem`, `&TestRoot` | panic dengan nama pemeriksaan yang gagal | test testkit dan platform-macos | tidak ada |
| `prepare_layout` | core | `&Platform` | `PreparedLayout` (layout plus root canonical) | daemon saat start (fitur 6) | `RootNotPrivate { path, reason }`, `RootsOverlap { a, b }`, `Platform(..)` |
| `ensure_instance_run_dir` | core | `&Platform`, `&PreparedLayout`, `ShortKey` | path folder run | supervisor sebelum start (fitur 10) | `RootNotPrivate`, `Platform(..)` |
| `create_instance_layout` | core | `&Platform`, `&PreparedLayout`, `&InstanceManifest`, `known: &[InstanceId]` | `InstancePaths` | create instance (fitur 20) | `AlreadyExists`, `ShortKeyTaken { key }`, `Partial { created, source }` |
| `write_instance_manifest`, `read_instance_manifest` | core | `&InstancePaths`, manifest | `()`, `InstanceManifest` | init, rename, rebind, retain, inventaris | `Manifest(ManifestError)`, `Platform(..)` |
| `inventory_instances` | core | `&Platform`, `&PreparedLayout` | `Inventory { entries: Vec<InventoryEntry> }` | daemon, adoption (fitur 57) | `Platform(..)` hanya kalau folder `instances/` atau `retained/` sendiri tidak terbaca |
| `check_owned_path` | core | `&Platform`, `&PreparedLayout`, path | `OwnedPath { root, canonical }` | operasi destruktif (fitur 7, 54, 18) | `OutsideOwnedRoots { path }`, `IsOwnedRoot { path }`, `NotNormalized { path }`, `Platform(..)` |
| `check_data_root_overlap` | core | `&Platform`, kandidat, data root yang ada | `()` | create, adoption | `Invariant(BR-002 DataRootOverlap)` |
| `apply` tiap state machine, `Instance::{new, apply_lifecycle, observe_readiness, observe_health, rename, rebind, record_data_migration, apply_provider_revision, set_desired}`, `plan_runtime_removal`, `evaluate_stop`, `should_autostart`, `endpoints_conflict`, `resolve_port`, `CapabilitySet::require`, `native_support` | domain | tipe domain | state baru, keputusan, atau error | core dan fitur berikutnya | `TransitionError`, `InvariantError`, `ProgressError`, `ValueError` |

`InventoryEntry`: `Found { location: active atau retained, manifest }`, `MissingManifest { dir }`, `UnreadableManifest { dir, reason }` (termasuk `not a regular file` dan `too large`), `UnsupportedSchema { dir, schema }`, `IdMismatch { dir, manifest_id }` (nama folder adalah ID yang valid tetapi berbeda), `LocationMismatch { dir }`, `Symlink { path }`, `UnexpectedEntry { name, kind }` (termasuk folder yang namanya bukan ID lowercase dan file biasa). Urutannya semua entri `instances/` urut nama, lalu semua entri `retained/` urut nama. `.DS_Store` dilewati tanpa entri.

`prepare_layout` membuat root yang belum ada (parent-nya harus sudah ada; folder `~/Library` tidak pernah dibuat), memeriksa setiap root yang ada (folder, bukan symlink, dimiliki user yang sedang berjalan, `mode & 0o077 == 0`), mengulang cek tumpuk root pada bentuk canonical, lalu membuat subfolder standar yang belum ada dan memeriksa yang sudah ada dengan aturan yang sama: `registry`, `providers`, `runtimes`, `instances`, `environments`, `operations`, `backups`, `retained` di data; `daemon` dan `instances` di logs; `downloads` di cache; dan `i` di runtime. Root atau subfolder yang terlalu terbuka ditolak, tidak di `chmod`.

`ensure_instance_run_dir` membuat ulang runtime root dan `i/` dengan mode 0700 kalau OS sudah menghapusnya (keduanya diperiksa dengan aturan yang sama kalau masih ada), lalu memastikan `i/<key>/`.

`check_owned_path` menolak path relatif dan path dengan komponen `.` atau `..` (`NotNormalized`) sebelum IO apa pun, lalu membuat canonical ancestor terdekat yang ada dan menyambung sisa komponen yang belum ada. Path harus berada di dalam salah satu root canonical dan tidak boleh sama dengan root itu sendiri (`IsOwnedRoot`).

`create_instance_layout` berjalan berurutan: cek `known` untuk short key yang sama, cek `instances/<id>` dan `retained/<id>` belum ada, `create_dir_private` untuk root instance, tulis manifest, lalu `data`, `config`, `config-history`, `temp`, dan `logs/instances/<id>`. Folder run di runtime root tidak dibuat di sini, karena runtime root bisa dikosongkan OS saat reboot; supervisor memanggil `ensure_instance_run_dir` sebelum setiap start.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| resolver macOS | home directory | `pw_dir` dari `getpwuid_r(getuid())`, bukan `HOME` |
| resolver macOS | temp dir user | `confstr(_CS_DARWIN_USER_TEMP_DIR)` |
| resolver macOS | root override | `STACKLY_ROOT` sebagai `OsString`, dibaca sekali oleh `main` `stacklyd` (dan `stackly` di fitur 8); kosong ditolak |
| `under_override` | `fnv8` | FNV-1a 64 bit (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`) atas byte path canonical, 32 bit bawah sebagai 8 hex huruf kecil |
| `prepare_layout`, `validate_root_override` | user saat ini | `UserIdentity::current_user` (macOS `getuid`) |
| `prepare_layout`, `validate_root_override` | pemilik, mode, jenis file, identitas | `Filesystem::symlink_metadata` (macOS `lstat`) |
| `write_file_atomic` | nama file sementara | `std::process::id()` ditambah counter atomik per proses |
| `create_instance_layout` | ID instance | dibuat use case create (fitur 20) dengan `InstanceId::generate()`, dibuat ulang saat `ShortKeyTaken` |
| `create_instance_layout` | `known` | ID dari registry (fitur 5); di test fitur ini dari `inventory_instances` |
| manifest | `createdAt`, `updatedAt`, `retainedAt`, `data.initializedAt` | `Clock::now` |
| manifest | `createdByOperation`, `byOperation` | ID operasi yang sedang berjalan (fitur 7); di test `OperationId::generate()` |
| manifest | `stacklyVersion` | `stackly_core::daemon_info().version` |
| manifest | `data.initializedLine`, `data.initializedVersion` | `line` dan `version` runtime saat create |
| `Instance` | `data_initialized_at` | argumen `at` dari `apply_lifecycle(init_succeeded, at)`, yang berasal dari `Clock::now` |
| `Instance` | `last_failure` | reason di payload `init_failed`, `start_failed`, atau `crashed` |
| `Operation` | `started_at` | argumen `at` saat event `start` pertama |
| `Instance::rebind` | `LineCompatibility` | manifest provider (fitur 9); kosong sampai saat itu |
| `Instance::apply_provider_revision` | `accepted_diff_sha256` | digest diff yang disetujui pengguna (fitur 59) |
| `plan_runtime_removal` | `RuntimeReferences` | registry (fitur 5 dan 55) |
| `effective` readiness dan health | `now` | `Clock::now` |
| `effective` readiness dan health | `max_age` | parameter; default `READINESS_MAX_AGE = 30 s` di core, `// provisional: F28` (tiga kali interval probe 10 detik) |
| `evaluate_stop`, `should_autostart` | `session` | parameter; sumber platform diputuskan fitur 31 |
| `resolve_port` | kandidat auto port | parameter dari pencari port platform (fitur 20 dan 10) |
| capability | reason not supported | konstanta di adapter |
| `ShortKey` | nilai | 8 karakter terakhir bentuk teks `InstanceId` |
| anggaran socket | 103, 15, 76 | konstanta di `layout` (lihat tabel anggaran) |
| `inventory_instances` | semua field entri | isi disk saja |

### Key invariants

Selain tabel BR:

- Domain tidak pernah memanggil IO, membaca environment, atau membuat waktu sendiri; waktu selalu parameter. Satu satunya pengecualian adalah `generate()` ID, yang membaca jam sistem lewat crate `uuid` untuk UUID v7.
- ID dan path hanya dibuat daemon; semua path berasal dari `DirectoryLayout`.
- Tidak ada operasi di fitur ini yang menghapus file atau folder, kecuali `write_file_atomic` yang membuang file sementaranya sendiri saat gagal.
- Tidak ada manifest atau tipe domain yang memuat nilai secret.
- Symlink tidak pernah diikuti oleh inventaris, file yang bukan file biasa tidak pernah dibaca, dan containment selalu memakai path canonical atau identitas file.
- Setiap owned root dan subfolder standarnya bermode 0700 dan dimiliki user yang sedang berjalan; yang tidak memenuhi itu ditolak, tidak diperbaiki diam diam.
- Runtime root hanya berisi file yang bisa dibuat ulang. Socket daemon dan socket instance selalu di bawahnya, dan path nya muat dalam 103 byte. Lock daemon ada di root data.
- Library crate tidak membaca environment dan tidak memuat path hardcode (dijaga `check-deps`).
- Kode state stabil; mengganti kode adalah perubahan kontrak yang butuh migrasi registry.

### Security model

- Batasnya satu akun user OS (BR-012). Stackly tidak mengklaim mencegah proses lain dengan hak user yang sama membaca atau mengubah file, dan checksum bukan bukti binary bebas malware (BR-020).
- Owned root 0700 milik user, dan penolakan root atau subfolder yang dimiliki user lain, berupa symlink, atau bisa diakses group dan other mencegah user lokal lain menyiapkan folder Stackly lebih dulu (squatting) atau membelokkan tulisan lewat symlink.
- Runtime root ada di temp dir privat yang dibuat OS per user, bukan `/tmp` bersama, jadi socket mewarisi privasi folder itu. Daemon tetap memverifikasi peer di fitur 6.
- `STACKLY_ROOT` hanya untuk test dan development, tidak pernah ditampilkan di GUI, tidak boleh kosong, dan tidak bisa menunjuk ke dalam root data default, sehingga test atau environment yang salah setel tidak menulis ke data asli pengguna.
- File manifest 0600, tanpa secret. Pembacaan manifest tidak mengikuti symlink dan tidak membuka FIFO atau device.
- `unsafe` baru hanya di `stackly-platform-macos`: `getuid`, `getpwuid_r`, dan `confstr`, masing masing dengan komentar `// SAFETY:`.

### Configuration required

- `STACKLY_ROOT` (opsional): path absolut folder privat yang sudah ada. Mengarahkan semua owned root ke `R/data`, `R/logs`, `R/cache`, dan runtime root `stackly-<fnv8>`. Untuk integration test dan development saja. Kalau di set, nilainya tidak boleh kosong.

### Critical test scenarios

- Happy path layout: `prepare_layout` di `FakePlatform` membuat keempat root dan subfolder 0700, run kedua tidak menambah mutasi, dan `violations()` nol. Memverifikasi **AC-9**, **AC-12**.
- Root dan subfolder asing: root data milik uid lain (lewat `FakeIdentity::set_current`), root berupa symlink ke folder lain di dalam test root, root 0755, dan `instances/` yang berupa symlink masing masing ditolak dengan alasan yang benar, dan folder tujuan symlink tidak tersentuh. Memverifikasi **AC-9**.
- Instance ujung ke ujung: create dua instance, tandai satu `init_succeeded` (manifest ditulis ulang dengan `initializedAt`), retain satu (rename manual folder di test ke `retained/` dengan penanda di manifest), lalu `inventory_instances` mengembalikan keduanya di urutan dan lokasi yang benar, dan `mutations()` tidak bertambah selama inventaris. Memverifikasi **AC-10**, **AC-11**.
- Inventaris kotor: folder tanpa manifest, manifest JSON rusak, manifest 64 KiB plus 1 byte, manifest berupa FIFO, manifest berupa symlink, schema `v2` dengan field yang tidak dikenal di v1, nama folder ID yang beda dengan manifest, nama folder `not-an-id`, folder di `retained/` tanpa penanda, symlink ke folder instance lain, `.DS_Store`, dan file `notes.txt` masing masing terklasifikasi atau dilewati sesuai aturan, tanpa perubahan di disk. Memverifikasi **AC-11**.
- Create gagal di tengah: `fail_when(create_dir_private, "config", NoSpace)` membuat create mengembalikan `Partial` dengan tepat root instance, manifest, dan `data`, tanpa penghapusan, dan folder instance lain tetap utuh. Memverifikasi **AC-10**, **AC-5** (BR-023).
- Create pada folder yang ada: folder `instances/<id>/` atau `retained/<id>/` yang sudah ada ditolak sebelum apa pun ditulis; short key yang sama dengan instance di `known` menghasilkan `ShortKeyTaken`. Memverifikasi **AC-10**, **AC-5** (BR-008).
- Tulis atomik gagal: rename yang dibuat gagal meninggalkan tujuan lama utuh dan tidak ada file `.tmp-` di folder itu, untuk fake dan filesystem asli. Memverifikasi **AC-10**, **AC-12**.
- Anggaran socket: root set dengan runtime root 76 byte diterima dan 77 byte ditolak; untuk 1000 `InstanceId` acak, path `instance_run_dir` ditambah 15 byte tidak lebih dari 103 byte. Memverifikasi **AC-8**.
- Override: `validate_root_override` menolak nilai kosong, path relatif, path yang tidak ada, symlink, folder milik user lain, folder 0777, dan subfolder dari root data default; `under_override` untuk dua root berbeda menghasilkan runtime root berbeda; FNV-1a `"a"` sama dengan `af63dc4c8601ec8c`. Integration test `stacklyd` dengan `STACKLY_ROOT` ke `TestRoot` mencetak keempat root dan keluar 0, dan dengan `STACKLY_ROOT` kosong keluar 1. Memverifikasi **AC-7**.
- Tabel state: test menyeluruh untuk machine runtime, lifecycle, dan operasi per pasangan dan per nilai payload, termasuk `unknown` yang menolak `stop_begin`, `retained` yang menolak semua event, dan cancel pada `queued` sebelum dan sesudah pernah mulai. Memverifikasi **AC-2**, **AC-4**.
- Readiness dan health: `spawned` tanpa probe menghasilkan `not_ready`; probe sukses lalu jam maju 31 detik dibaca `unknown` reason `stale`; health `healthy` saat `stopped` ditolak; observasi lama diabaikan; `crashed` mereset keduanya ke `unknown`. Memverifikasi **AC-3**.
- Aggregate instance: instance yang init-nya gagal tidak bisa di start; rebind saat `running` atau ke software lain ditolak; seratus observasi tidak mengubah `revision`, dan satu rename menaikkannya tepat 1. Memverifikasi **AC-16**.
- Invariant BR: satu test per BR berstatus `F4` di tabel, masing masing dengan guard evidence. Memverifikasi **AC-5**.
- Windows: `UnsupportedPlatform::new(windows-aarch64)` menjawab not supported untuk delapan capability dan method ketiga trait, dan `native_support` untuk provider macOS saja dan linux saja bernilai false. Memverifikasi **AC-13**, **AC-14**.
- macOS nyata: integration test `crates/platform-macos/tests/layout.rs` membuat `MacosPlatform::with_roots(test_root.roots())`, menjalankan `fs_conformance`, prepare, create, dan inventaris, dan memastikan root default tidak disentuh (`symlink_metadata` root default sebelum dan sesudah sama). Memverifikasi **AC-6**, **AC-12**, **AC-15**.
- Penjaga path hardcode: crate fixture di test `check-deps` yang memanggil `std::env::var` atau memuat `"/opt/homebrew"` membuat `check-deps` gagal dengan pesan file dan baris. Memverifikasi **AC-6**.
- Portabilitas: `cargo check --target aarch64-pc-windows-msvc` untuk domain, platform, protocol, dan core dijalankan manual di Mac ini seperti sekarang. Memverifikasi **AC-1**.

## Build plan

Urutan mengikuti pendekatan Journey: satu jalur dibuat lengkap dulu, yaitu "arahkan Stackly ke satu root, siapkan folder, buat folder instance, lalu inventarisasi tanpa registry", lalu model domain dan invariant diisi di atasnya. Milestone 1 sampai 3 tidak butuh crate `stackly-evidence`; milestone 4 butuh langkah 1 spec 0002 sudah dibangun, karena di situ guard evidence dipasang di semua test yang membuktikan ID PRD.

**Milestone 1: Port platform dan layout dari ujung ke ujung**

1. Domain dasar yang dibutuhkan port: makro `uuid_id!` dengan `InstanceId` dan `EnvironmentId`, `InvalidId`, `ShortKey`, serta `Os`, `Arch`, dan `Target`. Memenuhi **AC-1**, **AC-8**.
2. `stackly-platform`: trait, `FileInfo`, `UserId`, `PlatformCapability`, `PlatformCapabilities`, `PlatformError`, `SystemClock`, `host_target`, dan `Platform`; hapus `app_support_dir()` dan penanda `// provisional: F4`. Memenuhi **AC-1**.
3. `RootSet` (validasi, konstanta anggaran, `under_override` dengan FNV-1a) dan `DirectoryLayout` untuk root, lock daemon, instance, retained, environment, operasi, backup, log, unduhan, dan socket, plus `validate_root_override`. Memenuhi **AC-7**, **AC-8**.
4. `UnsupportedPlatform` dan test-nya. Memenuhi **AC-13**.
5. `stackly-testkit`: runtime folder `skt-` di `TestRoot`, `roots()`, `FakeFilesystem`, `FakeClock`, `FakeIdentity`, `FakePlatformInfo`, `FakePlatform`, dan `fs_conformance`; ganti `FakeDirectoryResolver`; test `validate_root_override` dengan fake. Memenuhi **AC-7**, **AC-12**.
6. `stackly-platform-macos`: `MacosPlatform::new` dan `with_roots` dengan `getpwuid_r`, `confstr`, filesystem, identitas, info, dan capability; `stacklyd` membaca `STACKLY_ROOT` (kosong ditolak), mencatat keempat root, dan keluar 1 kalau gagal, dengan integration test di `apps/daemon/tests/`. Memenuhi **AC-6**, **AC-7**, **AC-15**.
7. Core: `prepare_layout`, `ensure_instance_run_dir`, dan `check_owned_path` dengan test di `FakePlatform`. Memenuhi **AC-9**, **AC-12**.

**Milestone 2: Folder instance, manifest, dan inventaris**

8. Value type untuk manifest: nama, `ExactVersion`, `RuntimeLine`, `Sha256Digest`, `ProviderRevisionKey`, `RuntimeId`, `OperationId`, dan `InstanceManifest` dengan contoh JSON di `crates/domain/tests/fixtures/` (satu `v1` lengkap, satu dengan field tambahan yang tidak dikenal, satu `v2`); tambah `runtime_dir` dan `provider_revision_dir` ke layout. Memenuhi **AC-1**, **AC-11**.
9. Core: baca dan tulis manifest (decode `schema` lebih dulu), `create_instance_layout`, dan `inventory_instances`. Memenuhi **AC-10**, **AC-11**.
10. Core: `check_data_root_overlap` lewat identitas file, plus test alias symlink dan beda huruf di macOS. Memenuhi **AC-5** (BR-002).
11. Integration test `crates/platform-macos/tests/layout.rs` lewat `with_roots(test_root.roots())`, termasuk `fs_conformance` untuk `MacosFilesystem` dan test override di filesystem asli. Memenuhi **AC-12**, **AC-15**.

**Milestone 3: Model domain dan state machine**

12. `RuntimeState` dan aggregate `Runtime`, `InstallMode`, `RuntimeReferences`, `BackupId`, serta `plan_runtime_removal`. Memenuhi **AC-2**.
13. `InstanceLifecycle`, `Readiness`, `Health`, `Endpoint`, `Port`, `PortRequest`, `resolve_port`, dan aggregate `Instance` (guard init, rename, rebind, migrasi data, apply provider revision, desired state, revision). Memenuhi **AC-2**, **AC-3**, **AC-16**.
14. `OperationState`, `OperationKind`, `Operation`, `ProgressError`, `ProviderRevision`, `CapabilitySet`, `LineCompatibility`, `Environment`, `Lease`, `evaluate_stop`, `should_autostart`, dan `native_support`. Memenuhi **AC-2**, **AC-4**, **AC-14**.

**Milestone 4: Invariant dan penegakan**

15. Guard evidence di semua test yang membuktikan ID PRD (tabel BR dan tabel test lain), satu test per BR berstatus `F4` yang belum punya test, serta test AC-050, AC-074, dan NFR-010. Memenuhi **AC-5**, **AC-12**, **AC-13**, **AC-14**.
16. `cargo xtask check-deps`: aturan tanpa tokio dan libc untuk `stackly-platform`, pindaian `syn` untuk pembacaan environment dan literal path (tambahkan `syn` ke xtask kalau langkah 2 spec 0002 belum melakukannya), plus test dengan crate fixture yang melanggar. Memenuhi **AC-1**, **AC-6**.

## Consequences

**Positive**:

- Fitur 5 sampai 11 membangun di atas satu model dan satu set kode state, jadi registry, DTO, supervisor, dan engine operasi tidak masing masing menafsirkan PRD bagian 19.1 dan 18.2.
- Setiap use case yang menyentuh disk bisa diuji di Linux dan macOS dengan fake yang memakai filesystem asli, dan path yang bocor keluar test root langsung membuat test gagal.
- Path socket PostgreSQL dan MySQL muat di batas macOS sejak awal, bukan ditemukan saat fitur 19.
- Folder instance tetap bisa dikenali walau registry hilang, yang menjadi dasar inventaris orphan (fitur 57) dan reinstall tanpa `initdb` (fitur 62).
- Instance yang init-nya gagal tidak pernah di start, jadi cluster setengah jadi tidak pernah dijalankan.
- Windows mendapat jawaban not supported yang jujur dan teruji, tanpa adapter palsu.

**Negative / tradeoffs**:

- Runtime root di temp dir user bisa dikosongkan OS (saat boot, dan file lama secara berkala). Daemon (fitur 6) dan supervisor (fitur 10) wajib memeriksa ulang socket dan membuat ulang folder run; kalau tidak, CLI kehilangan socket setelah daemon lama berjalan.
- Anggaran 76 byte untuk runtime root berarti setup temp dir yang aneh ditolak saat start, bukan diakali, dan proses yang berjalan di App Sandbox tidak bisa memakai layout ini. Kegagalannya jelas, tetapi tetap kegagalan.
- Short key dari ID berarti create harus membuat ulang ID saat bentrok, sebelum apa pun disimpan. Peluangnya kecil (32 bit acak), tetapi jalurnya harus ada dan teruji.
- Manifest menduplikasi sebagian data registry, jadi keduanya bisa berbeda. Registry menang, dan reconciliation (fitur 30) harus membandingkan keduanya.
- `Arc<dyn Trait>` dan IO sync berarti setiap pemanggilan dari kode async lewat `spawn_blocking`. Itu tanggung jawab fitur 6 dan 7.
- `STACKLY_ROOT` yang tertinggal di shell pengguna mengarahkan CLI ke root lain. Fitur 8 harus menampilkan root aktif di `stackly daemon status`.
- Dua implementasi `Filesystem` (fake dan macOS) bisa menyimpang; suite `fs_conformance` mengurangi risikonya, tetapi hanya untuk perilaku yang diperiksanya.
- Domain dimodelkan sebelum fitur 5 sampai 11 memakainya, jadi sebagian field bisa berubah. Penambahan lewat update spec ini, bukan langsung di kode.
- Layout log menyimpang sedikit dari usulan PRD bagian 20.1 (`logs/instances/<id>/` dan `logs/daemon/`, bukan `logs/<id>/`).
- Fake filesystem di disk asli lebih lambat dari fake di memori, dan test kepemilikan hanya jalan di Unix.

**Neutral**:

- Dependency baru: `jiff` dan `serde` di domain, `jiff` dan `stackly-domain` di platform, `serde_json` dan `thiserror` di core, `libc` di platform-macos (ditambah dev dependency `stackly-core` dan `stackly-testkit`), `uuid` dengan fitur `serde` di workspace, dan `syn` di xtask bila belum ada.
- `InvalidInstanceId` berganti nama menjadi `InvalidId`, dan field log `app_support_dir` di `stacklyd` diganti empat field root. Langkah verifikasi spec 0001 yang memeriksa field itu adalah catatan sejarah.
- `cargo xtask check-deps` kini juga memindai source dengan `syn`.

## Follow-up

- [ ] Fitur 5: petakan tipe dan kode state di sini ke tabel registry, simpan `ResourceRevision` untuk compare and swap, dan sediakan daftar `known` untuk `create_instance_layout` serta `RuntimeReferences` untuk `plan_runtime_removal`.
- [ ] Fitur 6: panggil `prepare_layout` saat daemon start, ambil lock di `daemon_lock()` (root data, bukan runtime root), bind socket di `daemon_socket()`, periksa ulang socket secara berkala dan bind ulang kalau hilang, dan tampilkan root aktif serta status override di `stackly daemon status`.
- [ ] Fitur 8: `stackly` membaca `STACKLY_ROOT` sekali saat start dengan aturan yang sama dengan daemon.
- [ ] Fitur 9: tolak nama socket lebih dari 15 byte, petakan capability manifest ke `Capability`, dan isi `LineCompatibility`.
- [ ] Fitur 10: panggil `ensure_instance_run_dir` sebelum setiap start, dan ubah capability `process_supervision` menjadi supported.
- [ ] Fitur 20: tulis ulang manifest dengan `data.initializedAt` setelah `init_succeeded`.
- [ ] Fitur 11, 15, 28, dan 31: tambahkan trait masing masing lewat `/architect` update spec ini dan ubah capability-nya menjadi supported. Fitur 28 juga mengganti `READINESS_MAX_AGE`, dan fitur 31 menentukan sumber `SessionId`.
- [ ] Fitur 13: GUI menghitung path socket daemon dengan algoritma yang sama (`confstr`, `STACKLY_ROOT`, FNV-1a), dijaga fixture bersama berisi contoh input dan path hasilnya di `protocols/fixtures/`.
- [ ] Fitur 54: tambahkan `rename_no_replace` ke `Filesystem` untuk memindahkan folder ke `retained/`.
- [ ] Fitur 6 (test native): pastikan di macOS 27 kapan isi `_CS_DARWIN_USER_TEMP_DIR` dibersihkan, dan catat hasilnya di spec ini.
- [ ] `/architect` update spec 0001: tandai baris Fitur 4 di *Batas keputusan* sudah diputuskan, dan perbarui *Peta crate* (platform mendapat `jiff`; core mendapat `serde_json` dan `thiserror`; platform-macos mendapat dev dependency `stackly-core` dan `stackly-testkit`).
- [ ] `/sync` setelah build: catat `STACKLY_ROOT`, layout direktori, dan aturan `check-deps` yang baru di root `AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, keputusan kecil, dan hasil cross check ada di [rationale.md](rationale.md).
