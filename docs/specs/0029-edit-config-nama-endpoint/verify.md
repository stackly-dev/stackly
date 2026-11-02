# Verify: Edit config, nama, dan endpoint · spec 0029 · updated 2026-11-02

_Langkah diturunkan dari acceptance criteria spec 0029. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Milestone 1 hanya membuktikan domain, registry, dan core pada fake (paket `providers/fake` 0.4.0); bukti lewat socket dan CLI ada di Milestone 2, provider nyata di Milestone 4._

## Milestone 1: Domain, registry, and core

### UI / manual

- [x] Buka `crates/storage/migrations/0008_config_revision_history.sql` → kolom `number`, `endpoints`, `ready_at`, `origin`, `source_number` di `config_revisions` dengan backfill (`number` 1, `endpoints` dari `instance_endpoints`, `origin` `created`, `ready_at` null) → AC-7, AC-12
- [x] Buka `crates/core/src/supervisor/change.rs` → `apply_config` dan `change_endpoints` punya langkah `validate`, `lock`, `commit`, `apply`; `rename_instance` punya `validate`, `lock`, `commit`; guard commit ada di `change_guard.rs` dan membaca instance serta instance lain di dalam transaksi yang sama → AC-9, AC-13, AC-14
- [x] Buka `providers/fake/templates/fake.conf.tmpl` dan `provider.yaml` → revision `0.4.0`, `fakeConf` ke `${instance.config}/fake.conf` (tidak sensitif) dan ada di `lifecycle.reload.render`; `fake.json` tetap sensitif → AC-10

### Commands

- [x] `cargo test -p stackly-domain` → semua lulus, termasuk `apply_allowed`, `rename_allowed`, `Instance::set_port_requests`, `ConfigRevisionRecord::following`, dan himpunan kind `CloseInterrupted` yang memuat `apply_config` dan `change_endpoints` (bukan `rename_instance` dan `resolve_config_drift`) → AC-6, AC-7, AC-12, AC-13
- [x] `cargo test -p stackly-testkit` → `registry_conformance` lulus untuk `FakeRegistry`: `ActivateConfigRevision` (nomor naik, revision lama `superseded`, `Integrity` untuk endpoint yang tidak sama), `MarkConfigRevisionReady` idempoten, `PruneConfigRevisions` tidak pernah menghapus revision aktif atau last known good → AC-7, AC-12
- [x] `cargo test -p stackly-storage --lib` → adapter SQLite lulus conformance yang sama; `cargo test -p stackly-storage --test open` memeriksa migrasi dari registry versi sebelumnya (test milik `dev-f30-m1`) → AC-7, AC-12
- [x] `cargo test -p stackly-core --test config_evaluate` → evaluator pada fake, PostgreSQL, NATS, dan Traefik: `reload` lawan `restart`, `secretReadOnly`, `immutableAfterInit`, `managed`, daftar `routes` dibandingkan lewat `name`, dampak menurut lifecycle, capability, dan port, pratinjau hanya file tidak sensitif dengan baris berubah → AC-1, AC-2, AC-15, AC-16
- [x] `cargo test -p stackly-core --test supervisor_start -- config::` → 32 test lulus pada fake dan engine operasi (lihat daftar di bawah) → AC-3 sampai AC-14, AC-20
- [x] `cargo test -p stackly-core --test provider_packages` → golden `plan` dan `reload` fake 0.4.0 cocok; `STACKLY_UPDATE_GOLDEN=1` tidak mengubah apa pun → AC-10
- [x] `cargo clippy -p stackly-domain -p stackly-core -p stackly-testkit -p stackly-storage --all-targets --locked -- -D warnings` → bersih (tes integrasi `crates/storage/tests/open/migrate.rs` punya satu peringatan `type_complexity` milik `dev-f30-m1`)
- [x] `cargo xtask check-deps` → lulus
- [x] `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → lulus

Rincian `config::` di `crates/core/tests/supervisor_start/config.rs`:

- Drift: salinan per file tanpa file sensitif; file diubah di luar Stackly memblokir start (`config.drift_detected`, file tidak ditimpa, blocker `ConfigDrift`) sampai `resolve_config_drift`; file hilang terdeteksi `missing`; instance yang belum pernah merender tidak drift; pemeriksaan saat daemon start memasang blocker; respawn yang bertemu drift berhenti `failed` tanpa spawn; apply dan reload bertemu drift sebelum menulis apa pun → AC-10, AC-11
- Last known good: ready pertama menandai `ready_at`; proses adopsi tidak menandai revision baru; apply yang gagal ready meninggalkan revision baru tanpa `ready_at` dan rollback menulis revision baru berisi settings revision baik, lalu ready menandainya; rollback tanpa last known good lain `config.no_last_known_good` → AC-7, AC-8
- Apply: reload (PID sama, sinyal HUP, tanpa spawn baru), restart (PID baru, langkah `validate lock commit apply journal stop`), simpan saja untuk instance berhenti, lifecycle yang ditolak (`starting`, `stopping`, `unknown`), settings tidak valid (tanpa revision), field secret dan set once ditolak, base revision usang dan dua apply antre (`config.revision_conflict` dengan `activeRevision`), `unchanged`, restore dari histori dan revision yang sudah tidak ada, histori 20 revision dengan folder salinan ikut dihapus dan nomor tidak dipakai ulang → AC-3 sampai AC-6, AC-8, AC-9, AC-12
- Rename dan endpoint: rename saat running (ID, PID, revision config sama), `instance.name_taken`, nama yang sama, `initializing` ditolak; port fixed bebas disimpan untuk start berikutnya, port milik instance lain ditolak dengan saran port bebas, port yang dipegang proses luar ditolak tetapi `bound_port` sendiri lolos probe, ganti port di instance running me-restart, nama endpoint tidak dikenal; ringkasan audit memuat nama setting dan port tanpa nilai → AC-13, AC-14, AC-20

### Acceptance-criteria coverage

- AC-1, AC-2, AC-15, AC-16 · evaluator murni dan pratinjau di milestone ini (`config_evaluate`); method `instance.config.get` dan `validate`, DTO, dan form menunggu Milestone 2 dan 3
- AC-3 sampai AC-9, AC-12 sampai AC-14 · di tingkat core pada fake; bukti lewat socket menunggu Milestone 2, PostgreSQL dan NATS nyata menunggu Milestone 4
- AC-10, AC-11 · salinan histori, drift, blocker, `resolve_config_drift` pada fake; `postgresql.conf` nyata menunggu Milestone 4
- AC-20 · `ChangePlan::audit` dan input journal dibangun di core; pengisian journal oleh handler menunggu Milestone 2 langkah 9
- AC-17 sampai AC-19, AC-21 · belum, milik Milestone 2 sampai 4

## Milestone 2: Contract, daemon, CLI, and fixture proof

_Bukti lewat socket memakai fake (`providers/fake` 0.4.0) dan dummy server; PostgreSQL, NATS, dan Traefik nyata menunggu Milestone 4. Nomor method dan protokol: minor 2.3._

### UI / manual

- [x] Buka `crates/protocol/src/instance/config.rs` → enam params, `InstanceConfigView`, `ConfigValidation`, `ConfigApplyAccepted`, `ChangeEndpointsReply` (`applied` atau `dryRun`), `EndpointValidation`; `ConfigRevisionNumber` di `wire.rs` dipetakan ke `UInt64` di `typeshare.toml` → AC-1, AC-2, AC-14, AC-17
- [x] Buka `protocols/error-codes.json` → area `config` dengan `config.revision_conflict` (detail `baseRevision`, `activeRevision`), `config.drift_detected` (`files`), `config.no_last_known_good`; kode port `supervisor.endpoint_*` punya detail `suggestedPort` → AC-9, AC-11, AC-14
- [x] Jalankan `stackly instance config get <nama>` pada instance fake → tabel setting dengan kolom Applies (`reload`, `restart`, `set once`, `secret, read only`) dan tabel histori dengan tanda `active` dan `last known good` → AC-1, AC-17
- [x] Ubah `fake.conf` di folder `config/` instance lalu `stackly instance config get <nama>` → baris "changed outside Stackly"; `stackly instance start <nama>` ditolak `config.drift_detected`; `stackly instance config keep-revision <nama> --yes` menulis ulang file → AC-10, AC-11, AC-17

### Commands

- [x] `cargo test -p stackly-protocol` → semua lulus, termasuk `message::tests` (enam method baru, 24 method `instance.*`), `ipc_fixtures` (30 fixture config di `protocols/fixtures/ipc/index.json`, tiga reject berlabel `rust`), dan `config_fixtures` → AC-1, AC-2, AC-9, AC-10, AC-14, AC-17
- [x] `cargo xtask codegen --check` dan `cargo xtask fuzz seed` → bersih; digest 2.3 tercatat di `protocols/contract-versions.json` → kontrak
- [x] `swift test --package-path clients/swift` → lulus (225 test; `everyCallVariantHasARoundTripFixture` menghitung 50 method)
- [x] `cargo test -p stacklyd --lib` → lulus, termasuk `ipc::operation::dto::tests::every_domain_code_has_a_known_wire_value` untuk `apply_config`, `change_endpoints`, `rename_instance`, `resolve_config_drift`, dan `map::config::tests` → AC-20
- [x] `cargo test -p stacklyd --test supervisor -- config::` → 12 test (1 ignored, lihat catatan) lulus, `--test-threads=1` → AC-3, AC-6, AC-8 sampai AC-14, AC-20, AC-21
- [x] `cargo test -p stackly-cli --test instance -- config` → 9 test lulus (scripted daemon, goldens, dan matriks parity lawan `stacklyd` nyata) → AC-17
- [x] `cargo clippy -p stackly-protocol -p stackly-core -p stacklyd -p stackly-cli --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → bersih

Rincian `config::` di `apps/daemon/tests/supervisor/config.rs`:

- Get: revision 1 `created`, field terkunci (`failInit`, `initDelayMs`, `authToken`), kelas apply per field, secret hanya sebagai reference, last known good setelah ready pertama, salinan `fake.conf` → AC-1, AC-7, AC-15
- Validate dan apply tidak valid: diff dan dampak (`reload`, `restart`), `secret_read_only`, `immutable_after_init`, nilai di luar rentang; apply tidak valid, base usang (`config.revision_conflict` dengan `activeRevision`), dan perubahan yang sama (`unchanged`) tidak menambah operasi dan tidak mengubah PID → AC-2, AC-3, AC-9
- Reload (PID sama, revision 2 jadi last known good), restart (PID baru), instance berhenti hanya menyimpan (`next_start`), roll back (`rollback` tanpa last known good lain `config.no_last_known_good`) dan restore → AC-4 sampai AC-6, AC-8
- Drift: file diubah memasang blocker `configDrift` dan diff di `get`; start, apply, dan change endpoints ditolak `config.drift_detected` tanpa operasi dan tanpa mengubah file; `keep_revision` menulis ulang, file hilang terdeteksi `missing`, `keep_revision` tanpa drift `alreadyInState`; daemon yang start di atas file yang diubah memasang blocker → AC-10, AC-11
- Histori: 25 apply menyisakan 20 baris, revision aktif 26, last known good tetap, nomor berikutnya 27 → AC-12
- Rename: PID, ID, dan revision config sama; `instance.name_taken`, `instance.name_invalid`, nama yang sama tanpa operasi → AC-13
- Endpoint: port milik instance lain `supervisor.endpoint_conflict` dengan `suggestedPort`, dry run, endpoint yang tidak dikenal, port bebas me-restart instance running pada port baru dengan origin `endpoints`, `Auto` pada instance berhenti → AC-14
- Audit: input journal `apply_config` dan `change_endpoints` memuat revision lama dan baru, nama setting, dan port, tanpa nilai setting dan tanpa token → AC-20
- Offline: get, validate, apply, keep revision, rename, dan change endpoints tanpa akses katalog, tanpa panggilan `Fetcher`, tanpa `CodeSigning::assess` → AC-21

### Acceptance-criteria coverage

- AC-1, AC-2 · `get` dan `validate` lewat socket pada fake; PostgreSQL, NATS, dan Traefik nyata (fixture `config-view-postgresql` dan `config-view-traefik` dari paket pinned) menunggu Milestone 3 dan 4
- AC-3, AC-6, AC-8 sampai AC-14 · lewat socket pada fake; bukti pada provider nyata menunggu Milestone 4. `starting`, `stopping`, `unknown` ditolak di tingkat core (Milestone 1) dan tidak diulang lewat socket
- AC-4, AC-5 · reload dan restart pada fake; PostgreSQL 18 dan NATS nyata menunggu Milestone 4
- AC-7 · last known good lewat socket (ready pertama, reload, start berikutnya); kasus adopsi di Milestone 1
- AC-17 · seluruh verb CLI ada dan diuji (flag ke params, `--dry-run`, konfirmasi, matriks parity); completion dan man page dibuat dari definisi clap, tidak ada berkas terpisah
- AC-20 · input journal dari handler; hasil dan correlation ID dari journal operasi yang sama
- AC-21 · bagian fixture lewat harness offline; run dengan jaringan mati sungguhan milik Milestone 4
- AC-15, AC-16, AC-18, AC-19 · menunggu Milestone 3 (GUI)

### Catatan

- `a_running_instance_can_change_a_fixed_port_to_automatic` di-`#[ignore]`: mengubah port Fixed menjadi Auto pada instance yang sedang jalan gagal di langkah `stop` restart dengan `supervisor.config_invalid` ("endpoint http has no concrete address"), karena `Instance::set_port_requests` mengosongkan `bound_port` dan rencana stop memerlukan alamat konkret. Perbaikannya di `crates/core/src/supervisor/` (milik `dev-f30-m3`); hilangkan `#[ignore]` setelah diperbaiki → AC-14
- Method `instance.config.apply` dan `instance.config.keep_revision` memakai nama `instance.config_apply` dan `instance.config_keep_revision` di rekaman idempotency, karena `MethodName` domain hanya menerima satu titik; nama di kawat tidak berubah
- `start`, `restart`, dan `reload` menolak `config.drift_detected` dari blocker yang tersimpan di memori (diisi `instance.config.get`, setiap render, dan pemeriksaan saat daemon start); file yang diubah sesudah pemeriksaan terakhir ditangkap gerbang drift pada render, yang menggagalkan operasi sebelum menulis file
