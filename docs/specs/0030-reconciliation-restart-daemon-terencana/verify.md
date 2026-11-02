# Verify: Rekonsiliasi dan restart daemon terencana · spec 0030 · updated 2026-11-02

_Langkah diturunkan dari acceptance criteria spec 0030. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah._

## Milestone 1: Contract and data

### UI / manual

- [x] Buka `crates/protocol/src/restart.rs` → `RestartPreview`, `RestartPlanView`, dan enum terbuka `PlanDecision`, `PlanReason`, `PlanState`, `PlanCloseReason`, `PlanOutcome` masing masing berakhir di `Unrecognized` → AC-18
- [x] Buka `protocols/contract-versions.json` → versi 2.0 dan 2.1 tercatat; setiap perubahan schema berikutnya di fitur ini memakai 2.2 dan tidak memakai `--amend` → AC-18
- [x] Buka `crates/storage/migrations/0009_daemon_restart_plan.sql` → tabel `daemon_restart_plan` dan `daemon_restart_plan_instance`, `instance_id` tanpa foreign key, unique index parsial `daemon_restart_plan_open` untuk state terbuka → AC-19

### Commands

- [x] `cargo xtask codegen --check` → "20 generated files are up to date" → AC-18
- [x] `cargo test -p stackly-protocol` → semua lulus, termasuk `ipc_fixtures` (fixture `daemon.restart_preview`, `daemon.shutdown` dengan `expectedPlanDigest`, event `daemon.restart_plan`, `daemon.reconciling` dengan `findings`, tiga error `daemon.restart_*`) dan tiga test fixture katalog, create, dan duplicate yang kini memakai digest paket nyata → AC-18
- [x] `swift test --package-path clients/swift` → 225 test lulus, termasuk fixture round trip untuk 43 method → AC-18
- [x] `cargo test -p stackly-domain` → state machine rencana (`hand_over`, `abort`, `take_over`, `record_outcome`, `expired` di boot baru), keputusan `plan_decision` menurut lifecycle, `StopIntent`, transisi `data_missing` dari `stopped` dan `failed`, `OperationKind::DaemonRestart` → AC-4, AC-9, AC-13
- [x] `cargo test -p stackly-storage --test open a_version_8` → migrasi 9 membuat dua tabel kosong; rencana kedua yang terbuka ditolak `UNIQUE`, `finished_at` yang tidak cocok dengan state ditolak `CHECK`, baris instance ikut terhapus bersama rencananya → AC-19
- [x] `cargo test -p stackly-storage --test registry` dan `cargo test -p stackly-testkit --test fake_registry` → suite konformasi `restart_plans`: satu rencana terbuka, update, dan prune menyisakan 20 rencana terminal tanpa menyentuh rencana terbuka → AC-19
- [x] `cargo test -p stacklyd --test server` → 84 test lulus; handshake memakai `ProtocolVersion::CURRENT.major` → AC-18
- [x] `cargo test -p stackly-cli --bin stackly there_is_one_constructor_per_cli_code_and_each_follows_the_catalog` → konstruktor `restart_aborted` dan `restart_restore_failed` mengikuti katalog → AC-15 (bagian kode)
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings` dan `cargo fmt --all --check` → bersih
- [x] `cargo xtask check-deps` → lulus; `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → lulus

### Acceptance-criteria coverage

- AC-18 · seluruh kontrak: method, field, event, `findings`, kode, fixture, tipe Swift, bump mayor 2.0 (tambahan F70 jadi 2.1)
- AC-19 · migrasi 9, unique index parsial, retensi 20 rencana terminal di prune, suite konformasi
- AC-4 · hanya bagian domain (transisi `data_missing`, failure code); pemeriksaan folder data menunggu Milestone 2
- AC-9, AC-13 · hanya bagian domain (state machine rencana); alur daemon menunggu Milestone 3
- AC-1 sampai AC-3, AC-5 sampai AC-8, AC-10 sampai AC-12, AC-14 sampai AC-17 · belum, milik milestone lain

### Sisa kegagalan di luar milestone ini

`stackly-core catalog::status::tests::an_available_host_artifact_takes_its_support_from_its_records_and_notes_fill_empty_columns` dan `stackly-cli --test provider inspect::inspect_writes_the_identity_capabilities_matrix_and_artifacts` (F70 M3), `stacklyd ipc::operation::dto::tests::every_domain_code_has_a_known_wire_value` (F29 M2).
