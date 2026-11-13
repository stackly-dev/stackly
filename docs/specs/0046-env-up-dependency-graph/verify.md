# Verify: Env up & dependency graph · spec 0046 · updated 2026-11-13

_Langkah diturunkan dari acceptance criteria spec 0046. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27, dengan `export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`._

## Milestone 1: fondasi operasi anak

Langkah Build plan 1 sampai 3: engine operasi pengatur dan anak, domain dan storage (migrasi 15), lalu create dan start untuk Up. Protokol tidak berubah di milestone ini (`OperationSummary.parent` dan `coordinators` di `OperationStats` masuk di langkah 5).

### UI / manual

- [x] Buka `crates/domain/src/operation.rs` → `OperationKind::is_coordinator()` benar hanya untuk `environment_up`; `environment_up` tidak `resumable()` dan `interrupt_policy()` nya `CloseInterrupted`; `environment_down` tidak berubah (resumable, `Compensate`, milik spec 0047) → AC-3, AC-15
- [x] Buka `crates/core/src/operation/limits.rs` → `MAX_RUNNING_COORDINATORS` 4 dan `EngineLimits.max_coordinators`; buka `crates/core/src/operation/scheduler.rs` → `QueueEntry.coordinator` diturunkan dari jenis operasi, dispatcher memeriksa `coordinators < max_coordinators` untuk pengatur dan `running < max_running` untuk operasi lain, dan `finish_slot` mengurangi penghitung kelasnya sendiri → AC-3, AC-24
- [x] Buka `crates/storage/migrations/0015_environment_up.sql` → dua `ALTER TABLE` saja: `environments.last_up` (JSON objek, nullable) dan `operation_journal.parent_id` (UUID v7, nullable, tanpa FK) → AC-24, AC-10
- [x] Buka `crates/core/src/instance/create/binding.rs` → guard `register` membaca environment di dalam transaksinya, menulis `UpdateEnvironment` dengan revision yang baru dibacanya dan `InsertLease` environment `active` sesudah `InsertInstanceAt`; `instance.create` dari klien tidak pernah membuat `EnvironmentBinding` (`grep -rn with_binding apps/` kosong) → AC-7, AC-13
- [x] Buka `crates/core/src/supervisor/startup.rs` → `StartTrigger::Environment` diperlakukan seperti `User` di setiap cabang (caller percobaan pertama, tidak dilewati saat sesi berakhir, menulis desired `running`), dan `writes_manual_lease()` benar hanya untuk `User` → AC-13

### Commands

- [x] `cargo test -p stackly-domain --lib operation::tests` → `only_environment_up_is_a_coordinator`, daftar resumable tanpa `environment_up`, dan daftar `CloseInterrupted` dengan `environment_up` lulus → AC-3, AC-15
- [x] `cargo test -p stackly-core --test operation_engine coordinator` → 7 test lulus: tiga pengatur dengan `max_running` 2 masing masing memulai satu anak dan menunggunya lewat `wait_final` tanpa deadlock; pengatur kelima `queued` selagi empat berjalan dan `running` 0 sementara operasi biasa tetap jalan; anak mencatat `parent`, pemicu induk, tanpa method, fingerprint, dan idempotency key, dan ditolak `RestartPending` selama quiesce (operasi background tidak); `wait_final` selesai di `succeeded`, `failed`, dan `needs_recovery`, `None` untuk operasi yang tidak ada, dan `ShuttingDown` saat shutdown; `recover` menutup pengatur yang terputus beserta anak `create_instance` dan `start_instance` yang antre (`failed`, alasan `interrupted`, `operation.interrupted`) dan membangun ulang anak `install_runtime`; pengatur yang masih antre ditutup bersama anak antrenya → AC-3, AC-15, AC-24
- [x] `cargo test -p stackly-core --test operation_engine` → semua test engine lama tetap lulus (89 sebelum milestone ini); satu test restart memakai `import_environment` sebagai jenis tanpa factory menggantikan `environment_up`, yang kini pengatur dan langsung berjalan di slotnya sendiri → AC-24
- [x] `cargo test -p stackly-storage --test open environments` → fixture v15 membaca `lastUp` (state `failed`, service `db` `failed` tahap `start` dengan `supervisor.readiness_timeout`), `last_operation_id`, `updated_at`, dan `parent` journal anak; `last_up` yang bukan objek JSON ditolak skema → AC-10, AC-24
- [x] `cargo test -p stackly-storage --test open` → 78 test lulus, termasuk fingerprint migrasi 15, fixture beku v15 (`STACKLY_WRITE_REGISTRY_FIXTURE`), dan kolom baru di model data → AC-24
- [x] `cargo test -p stackly-storage --test registry conformance` dan `cargo test -p stackly-testkit` → `registry_conformance` lulus pada fake dan SQLite: `journal_parent_round_trips`, `environment_up_report_round_trips` (laporan Up, operasi terakhir, `dependency_conditions`, dan stempel tulis), dan `delete_created_member` (`DeleteInstanceCreatedBy` menghapus baris member dan lease instance itu, revision environment naik satu, member dan lease lain tetap) → AC-4, AC-7, AC-12, AC-13
- [x] `cargo test -p stackly-core --test create_operation bound` → 4 test lulus: create terikat menulis instance, member `dedicated` dengan kondisinya, dan satu lease environment yang sudah ada saat init berjalan, desired tetap `stopped`; create terikat yang init nya gagal menghapus hanya instance, member, dan lease miliknya, sedangkan instance pengguna bernama mirip, member `shared` dan lease nya, dan runtime utuh; environment yang hilang gagal `env.not_found` tanpa folder; input journal tanpa ikatan sama persis dengan `encode_input`, dan create yang dibangun ulang dari input terikat tetap menulis member dan lease → AC-7, AC-12, AC-13
- [x] `cargo test -p stackly-core --test supervisor_start leases` → start ber pemicu `environment` menulis desired `running` tanpa lease apa pun, juga pada instance `running` yang desired nya `stopped`; test lease `manual` lama tetap lulus → AC-13
- [x] `cargo test -p stackly-core --test create_operation` dan `cargo test -p stacklyd` → golden dan test create serta start yang ada tetap lulus tanpa perubahan harapan → AC-7, AC-13
- [x] `cargo test -p stacklyd --test environment updated_at` → `updatedAt` summary sama dengan stempel baris `environments`, tidak bergeser oleh import yang tidak mengubah apa pun (di `env.import`, `env.list`, dan `env.get`), dan bergeser oleh import yang mengubah baris (perbaikan spec 0044 AC-14)
- [x] `cargo xtask check-deps`, `cargo xtask evidence check`, dan `cargo xtask codegen --check` → lulus tanpa perubahan file hasil generate
- [x] `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → exit 0

### Acceptance-criteria coverage

- AC-3 · bagian engine: kelas slot pengatur, batas 4, tiga pengatur tanpa deadlock; `BusyPolicy::Reject`, `LockKey::EnvironmentName`, dan `expected_revision` operasi Up dipasang handler di Milestone 2 (langkah 4 dan 6)
- AC-4 · bagian domain dan storage: `bind_member`, `set_member_conditions`, `begin_up`, `finish_up`, baca dan tulis `last_operation_id`, `last_up`, `dependency_conditions`; guard `accept` dan commit nya di langkah 4. `lock_sha256` di commit `accept` menunggu field `Environment.lock_sha256` dari spec 0045
- AC-7 · ikatan create, input journal dan `rebuild`, batch `register`, kompensasi; `environment::plan::draft_for` dan pemanggilan dari Up di langkah 4 dan 6
- AC-12 · kompensasi create hanya atas milik sendiri (fixture); Up sendiri tanpa kompensasi menunggu langkah 4
- AC-13 · bentuk lease, lease di batch `register`, start `environment` tanpa lease `manual` (juga cabang `no_op_with_desired` di `start_precheck` daemon, yang menerima pemicu); test klien putus, restart daemon, dan rotasi sesi di langkah 6
- AC-15 · bagian engine: `environment_up` `CloseInterrupted`, anak antre ditutup `recover`, install antre dibangun ulang; `lastUp.state` `interrupted` di `env.get` di langkah 6
- AC-24 · kelas slot, `Origin::Child` dan quiesce, `parent` di journal, `wait_final`, aturan `recover`, `EngineStats.coordinators` dan `max_coordinators`; `OperationSummary.parent` dan `operation.get_stats` di wire di langkah 5
- AC-10 · hanya kolom `last_up` dan bentuk `UpReport`; isi laporannya di langkah 4
- AC-1, AC-2, AC-5, AC-6, AC-8, AC-9, AC-11, AC-14, AC-16 sampai AC-23, AC-25 · belum, milik Milestone 2 sampai 4
