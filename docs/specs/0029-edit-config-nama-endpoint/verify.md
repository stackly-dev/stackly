# Verify: Edit config, nama, dan endpoint · spec 0029 · updated 2026-11-03

_Langkah diturunkan dari acceptance criteria spec 0029. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Milestone 1 hanya membuktikan domain, registry, dan core pada fake (paket `providers/fake` 0.4.0); bukti lewat socket dan CLI ada di Milestone 2, provider nyata di Milestone 4._

## Milestone 1: Domain, registry, and core

### UI / manual

- [x] Buka `crates/storage/migrations/0008_config_revision_history.sql` → kolom `number`, `endpoints`, `ready_at`, `origin`, `source_number` di `config_revisions` dengan backfill (`number` 1, `endpoints` dari `instance_endpoints`, `origin` `created`, `ready_at` null) → AC-7, AC-12
- [x] Buka `crates/core/src/supervisor/change.rs` → `apply_config` dan `change_endpoints` punya langkah `validate`, `lock`, `commit`, `apply`; `rename_instance` punya `validate`, `lock`, `commit`; guard commit ada di `change_guard.rs` dan membaca instance serta instance lain di dalam transaksi yang sama → AC-9, AC-13, AC-14
- [x] Buka `providers/fake/templates/fake.conf.tmpl` dan `provider.yaml` → revision `0.4.0`, `fakeConf` ke `${instance.config}/fake.conf` (tidak sensitif) dan ada di `lifecycle.reload.render`; `fake.json` tetap sensitif → AC-10

### Commands

- [x] `cargo test -p stackly-domain` → semua lulus, termasuk `apply_allowed`, `rename_allowed`, `Instance::set_port_requests`, `ConfigRevisionRecord::following`, dan himpunan kind `CloseInterrupted` yang memuat `apply_config` dan `change_endpoints` (bukan `rename_instance` dan `resolve_config_drift`) → AC-6, AC-7, AC-12, AC-13
- [x] `cargo test -p stackly-testkit` → `registry_conformance` lulus untuk `FakeRegistry`: `ActivateConfigRevision` (nomor naik, revision lama `superseded`, `Integrity` untuk endpoint yang tidak sama), `MarkConfigRevisionReady` idempoten, `PruneConfigRevisions` tidak pernah menghapus revision aktif atau last known good → AC-7, AC-12
- [x] `cargo test -p stackly-storage --test registry` → adapter SQLite lulus conformance yang sama (`crates/storage/tests/registry/conformance.rs`; baris ini dulu menyebut `--lib`, padahal conformance ada di test `registry`); `cargo test -p stackly-storage --test open` memeriksa migrasi dari registry versi sebelumnya (test milik `dev-f30-m1`) → AC-7, AC-12
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
- [x] `cargo test -p stacklyd --test supervisor -- config::` → 12 test (1 ignored saat itu, lihat catatan) lulus, `--test-threads=1`; kini 14 lulus dan 0 ignored, setelah fix `4bd9c15e` dan test balapan create dan rename ke satu nama (`a65f4e0d`) → AC-3, AC-6, AC-8 sampai AC-14, AC-20, AC-21
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

- `a_running_instance_can_change_a_fixed_port_to_automatic` di-`#[ignore]`: mengubah port Fixed menjadi Auto pada instance yang sedang jalan gagal di langkah `stop` restart dengan `supervisor.config_invalid` ("endpoint http has no concrete address"), karena `Instance::set_port_requests` mengosongkan `bound_port` dan rencana stop memerlukan alamat konkret. Perbaikannya di `crates/core/src/supervisor/` (milik `dev-f30-m3`); hilangkan `#[ignore]` setelah diperbaiki → AC-14. Sudah selesai di `4bd9c15e`: perbaikannya ternyata di domain, `IpEndpoint::set_request` menyimpan port yang dipegang proses sebagai `bound_port` saat permintaan baru `Auto`, dan `#[ignore]` sudah dihapus
- Method `instance.config.apply` dan `instance.config.keep_revision` memakai nama `instance.config_apply` dan `instance.config_keep_revision` di rekaman idempotency, karena `MethodName` domain hanya menerima satu titik; nama di kawat tidak berubah
- `start`, `restart`, dan `reload` menolak `config.drift_detected` dari blocker yang tersimpan di memori (diisi `instance.config.get`, setiap render, dan pemeriksaan saat daemon start); file yang diubah sesudah pemeriksaan terakhir ditangkap gerbang drift pada render, yang menggagalkan operasi sebelum menulis file

## Milestone 3: GUI

_Bukti memakai fixture tampilan (`FixtureConfiguration`) dan service yang dijawab dari data, tanpa daemon dan tanpa jaringan; PostgreSQL, NATS, dan Traefik di sini memakai key dan kelas apply paket aslinya, bukan runtime yang berjalan. Langkah di daemon asli dan jaringan mati milik Milestone 4. Aplikasi memakai komponen asli (`SchemaFormSections`, `EndpointPortRow`, `DiffView`, `StateBanner`, `StacklySheet`) tanpa cabang per provider._

### UI / manual

- [x] Buka `apps/macos/Packages/StacklyUI/Tests/StacklyUITests/SnapshotTests/__Snapshots__/ConfigurationSnapshotTests/config-postgresql.light.png` → "Revision 3" dengan tag "In sync", field secret dan set once terkunci ("Read only", badge "Set once", "Stored in Keychain as …"), field restart dan reload dengan catatan "Applies on reload · default none", baris port terkunci "Managed by Stackly · change in Endpoints", History dengan badge Active dan Last known good → AC-15, AC-18
- [x] Buka `config-postgresql-pending-restart.light.png` → Pending Changes `key = lama` dan `key = baru`, tag "Requires restart", tombol "Apply and Restart…"; `config-postgresql-pending-reload.light.png` → "Applies on reload" dan "Apply and Reload"; `config-postgresql-invalid.light.png` → error di bawah field dan Apply nonaktif dengan alasannya → AC-15, AC-18
- [x] Buka `config-nats.light.png` → NATS tanpa capability reload: setiap field yang bisa diedit berkata "Requires restart", field managed terkunci; `config-traefik.light.png` dan `config-traefik-route-added.light.png` → `routes` sebagai daftar dengan Add dan Remove dari schema saja, route baru muncul sebagai `routes[name=docs]` → AC-16
- [x] Buka `config-drift.light.png` → banner "postgresql.conf was changed outside Stackly" dengan Keep Revision 3… dan Show Diff, Apply nonaktif dengan alasan; `config-roll-back.light.png` → banner "Revision 4 hasn’t reached ready" dengan "Roll Back to Revision 3…"; `config-offline.light.png` → perubahan tertunda tetap, Apply nonaktif → AC-18
- [x] Buka `config-effective-file.light.png` → Effective File read only dengan baris yang berubah ditandai → AC-18
- [x] Buka `rename-sheet-taken.light.png`, `rename-sheet.light.png`, `ports-sheet-conflict.light.png`, dan `ports-sheet.light.png` → pemeriksaan nama langsung, saran port bebas dengan tombol "Use 5435" (tidak pernah dipakai otomatis), "Fixed ports are never changed silently." → AC-14, AC-19
- [x] Buka `instance-detail-configuration.light.png` → picker Summary, Configuration, Logs, Metrics di halaman detail; `overview-config-drift.light.png` → item Needs Attention "orders-db’s configuration changed" → AC-18, AC-19
- [ ] Dengan daemon fake (`cargo build -p stacklyd -p stackly-fake-server` lalu `cargo run -q -p stacklyd --example skeleton_seed -- --root <R>`), buka aplikasi, pilih instance, tab Configuration, ubah satu field, tunggu 300 ms, Apply → kartu "Revision N applied to <nama>." dan History bertambah (tidak dijalankan di sesi ini; native milik Milestone 4)
- [ ] Menu Instance → Rename… dan Edit Ports… (tanpa shortcut), menu More di halaman detail, menu konteks baris, dan tombol Edit di kartu Endpoints membuka sheet yang sama (cek tangan di aplikasi nyata)

### Commands

- [x] `swift test --package-path clients/swift` → lulus (247 test); `InstanceConfigClientTests` memeriksa params dan balasan `get`, `validate`, dan dry run `change_endpoints`, balasan jenis lain sebagai `protocolViolation`, dan `config.revision_conflict` → AC-1, AC-17
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter ConfigurationModelTests` → 18 test: baris terkunci dari schema, debounce 300 ms, label Apply menurut dampak, konfirmasi restart, Restore, banner roll back dan drift, konflik revision, dan refresh → AC-12, AC-15, AC-16, AC-18
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter ConfigurationSheetsTests` → 14 test: nama dicek langsung, Rename dan Change Ports mengirim panggilan yang benar, saran port tidak diambil sendiri, `renameInstance` dan `editPorts` built tanpa shortcut, item drift Overview, dan tab di navigator → AC-13, AC-14, AC-19
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter ConfigurationSnapshotTests` → 17 test snapshot (terang, gelap, sempit) lulus; baca PNG dengan `cargo xtask snapshot review` → AC-15, AC-16, AC-18, AC-19
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` → suite penuh sekali: 1652 test, semua lulus kecuali `CreateInstanceModelTests.aRefusedCreateLeavesAnEnterSecretOffUntilTheTextIsTypedAgain` (flaky yang sudah ada sebelum milestone ini); snapshot halaman detail (`InstanceDetailSnapshotTests`, `InstanceMetricsSnapshotTests`, `InstanceLogsSnapshotTests`) direkam ulang karena picker kini punya segmen Configuration dan kartu Endpoints punya tombol Edit, lalu lulus
- [x] `cargo xtask build-app` → bersih, `Stackly.app` terbangun dan bertanda tangan ad hoc
- [x] `xcrun swift-format lint --strict` per file yang berubah → bersih

### Acceptance-criteria coverage

- AC-1, AC-17 (sisi client Swift) · `InstanceConfigClient` dan test fixture; verb CLI sudah milik Milestone 2
- AC-15, AC-16, AC-18 · tab Configuration pada PostgreSQL, NATS, dan Traefik dari fixture; bukti pada PostgreSQL 18, NATS, dan Traefik nyata menunggu Milestone 4
- AC-19 · Rename, Edit Ports, menu, item Overview, dan label sibuk ("Applying…", "Renaming…", "Changing Ports…"); kartu umpan balik lewat `InstanceActionContext`
- AC-12 (sisi GUI) · Restore memuat settings sebuah revision lama sebagai perubahan tertunda dan Apply mengirim `restore(N)` selama form belum diedit lagi
- AC-21 · tidak berubah; jaringan mati sungguhan milik Milestone 4

### Catatan

- Tombol yang meminta konfirmasi berjudul diakhiri "…" ("Apply and Restart…", "Roll Back to Revision N…", "Keep Revision N…") karena `StacklyAction` menolak aksi berkonfirmasi tanpa tanda itu; spec menulis judul tanpa tanda itu
- Sheet Rename dan Ports dihosting `InstancesPage`; wiring di luar folder fitur: `AppShell` (satu argumen `config:`), `InstancesPage`, `ShellNavigator` (`openInstance(_:tab:)`), `CommandMap` dan `StacklyCommands` (dua baris built), `InstancesCommandActions`, `AttentionRules` (aturan I11), `InstanceFailureCopy`, `EndpointPortRow` (init tanpa `CreateInstanceModel`), `SchemaFormModel` dan `SchemaFormView` (kunci baris dan catatan, additif)
- `RenameModel.submit` memakai nama itu karena aturan sumber spec 0015 AC-14 melarang pemanggilan `rename(`

## Milestone 4: Provider nyata di Mac ini

_Langkah 16, 17, dan 18 sudah dibangun: test native PostgreSQL 18, NATS, dan Traefik lewat daemon in process dan proses asli dari cache runtime xtask (tanpa unduhan). Langkah 19 (jaringan mati) dipindah ke langkah manual milikmu: kamu mengizinkannya (2026-11-02), tetapi mematikan Wi-Fi dan Ethernet mengubah setelan sistem dan memutus semua sesi Claude, termasuk Remote Control, jadi lead tidak menjalankannya. AC-21 dibuktikan oleh harness offline di dalam proses (Milestone 2). Tidak ada kode produksi yang diubah; semua perubahan ada di `apps/daemon/tests/`._

### UI / manual

- [ ] BLOCKED, reason: run dengan jaringan mati sungguhan (GUI dan CLI) dipindah ke langkah manual milik user, owner: @ojiepermana. Cara menjalankannya sendiri: matikan jaringan, jalankan `stackly instance config get`, `config validate`, `config apply`, `config keep-revision`, `rename`, dan `endpoint set` pada instance PostgreSQL 18 yang pinned, lalu buka tab Configuration di app dan terapkan satu perubahan → semuanya berhasil tanpa akses katalog, unduhan, atau pemeriksaan tanda tangan baru → AC-21

### Commands

- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test postgresql --test nats --test traefik -- --ignored --test-threads 1 config::` → 6 test lulus (run `01a19b1c-1ba4-718f-8390-d4ecfdc904a6`, 6 record native PASS, 0 problem): PostgreSQL 18 (4), NATS (1), Traefik (1) → AC-4, AC-5, AC-7, AC-8, AC-11, AC-14, AC-16
- [x] PostgreSQL 18, apply reload dan apply restart (record `01a19b1c-6ca2-754a-9138-9e07ca9fe7b7`): `logStatement` berdampak `reload`, PID dan waktu start postmaster sama, `SHOW log_statement` jadi `all`, revision 2 menjadi last known good; `maxConnections` berdampak `restart`, postmaster baru, `SHOW max_connections` 60, data tetap, revision 3 menjadi last known good, histori 3 baris → AC-4, AC-7
- [x] PostgreSQL 18, rollback setelah setting yang membuat start gagal (record `01a19b1d-0605-721c-8940-b35a41398564`): salinan paket yang melonggarkan batas `maxConnections` di schema, nilai 300000 ditolak postmaster, operasi apply `failed`, revision 2 tanpa `ready_at`, last known good tetap 1; `Rollback` menulis revision 3 (origin `rollback`, nilai revision 1), instance ready lagi dengan datanya, last known good 3 → AC-8, AC-7
- [x] PostgreSQL 18, ganti port Fixed (record `01a19b1c-d957-7591-ab39-84d037f547eb`): port yang dipegang listener luar ditolak `supervisor.endpoint_*` dengan `suggestedPort`, tanpa operasi dan PID sama; port bebas me-restart instance, `SHOW port` dan nama socket mengikuti port baru, port lama bebas lagi, revision 2 origin `endpoints` → AC-14
- [x] PostgreSQL 18, drift `postgresql.conf` (record `01a19b1c-a335-70e5-b602-b09efde7ea1e`): file diedit tangan, `config get` menandai `Modified` dengan diff dan blocker `configDrift`; apply, restart, dan reload ditolak `config.drift_detected` tanpa mengubah file; `keep-revision` menulis ulang salinan, lalu reload berjalan dan postmaster sama → AC-11
- [x] NATS 2.15.1 (record `01a19b1c-5507-744a-bb87-dbc8d73ec8a2`): `reloadSupported` false, reload ditolak, `maxPayloadBytes` berdampak `restart`, `nats-server` baru dan lama sudah mati, `/varz` `max_payload` 2097152, revision 2 last known good; nilai dikembalikan jadi revision 3 dan last known good 3 → AC-5, AC-7, AC-16
- [x] Traefik 3.7.15 (record `01a19b1d-3d13-7048-ab27-bc6c3ce20b17`): dua instance berdampingan; nama route ganda ditolak `fieldErrors` di `settings.routes`; route valid pada instance A berdampak `reload`, PID sama, route menjawab `alpha` dari backend, last known good 2; instance B tetap (PID, revision, file `routes.yaml`, 404 untuk host itu) sampai ia mendapat route sendiri → AC-4, AC-16
- [x] `cargo clippy -p stacklyd --all-targets --locked -- -D warnings` dan `cargo fmt --all --check` → bersih

### Acceptance-criteria coverage

- AC-4 · reload di PostgreSQL 18 dan Traefik pada proses asli, PID sama
- AC-5 · restart di PostgreSQL 18 dan NATS pada proses asli, proses baru melayani data yang sama (NATS tanpa reload)
- AC-7 · last known good pada PostgreSQL 18 (revision 2 dan 3), NATS, dan Traefik; kasus adopsi tetap di Milestone 1
- AC-8 · rollback setelah start yang gagal pada PostgreSQL 18
- AC-11 · drift `postgresql.conf` pada PostgreSQL 18
- AC-14 · ganti port Fixed dengan restart dan konflik dengan proses luar pada PostgreSQL 18
- AC-16 · bagian native: form `routes` Traefik (nama ganda tidak valid) dan setting NATS lewat daemon nyata; tampilan GUI menunggu Milestone 3
- AC-21 · dibuktikan oleh harness offline di dalam proses (Milestone 2); run jaringan mati sungguhan menjadi langkah manual milikmu

### Catatan

- Test rollback memakai salinan paket di folder sementara dengan batas `maximum` `maxConnections` dinaikkan, karena Mac ini (32 GiB) tetap berhasil start dengan `sharedBuffersMb` 16384, nilai terbesar yang schema asli izinkan; PostgreSQL sendiri menolak `max_connections` di atas 262143. Paket di `providers/` tidak diubah.
- Harness Traefik (`apps/daemon/tests/traefik/harness.rs`) adalah harness NATS dengan paket dan artefak yang diganti; pemanggil config bersama untuk ketiga crate test ada di `apps/daemon/tests/supervisor/config_calls.rs`.
