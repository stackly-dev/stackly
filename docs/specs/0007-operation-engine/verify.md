# Verify: Operation engine · spec 0007 · updated 2026-10-19

_Langkah diturunkan dari acceptance criteria spec 0007, satu bagian per milestone (Milestone 1 sampai 4). `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27. Probe yang mengubah file sementara selalu dikembalikan sesudahnya (`git status` bersih lagi)._

_Hasil `/check verify` 2026-10-19 (cabang `claude/f7-daemon-closeout-7fb5d1` di atas main `6b3da2c`, commit `0b5fe19`): semua langkah Milestone 1 sampai 4 dijalankan dan lulus. Evidence fixture dari dua run `cargo xtask evidence run`: 9 record, semuanya PASS, 0 masalah (6 dari engine dan `remove_tree`, 3 dari daemon). `stacklyd` sungguhan dijalankan dua kali dengan `STACKLY_ROOT` sementara: sekali normal, sekali dengan registry dalam recovery mode. Satu bagian AC-3 (operasi latar tidak membawa key) belum punya test; langkahnya dibuktikan lewat probe sementara dan diserahkan ke `/test`._

## Milestone 1: Journal di registry

### UI / manual

- [x] Buka `crates/storage/migrations/0002_operation_journal.sql` → `UPDATE operations SET idempotency_key = NULL` untuk key ganda yang lebih lama, `CREATE UNIQUE INDEX operations_idempotency_key ... WHERE idempotency_key IS NOT NULL`, empat tabel `operation_journal`, `operation_steps`, `operation_created`, `operation_locks` dengan `STRICT` dan `ON DELETE CASCADE`, index `operation_locks_key`, dan backfill satu baris journal per operasi dengan `CASE` resumable (`install_runtime`, `environment_up`, `environment_down`, `backup_instance`) → AC-17
- [x] Buka `crates/storage/src/registry/journal.rs` → upsert langkah dan catatan ditulis `ON CONFLICT (operation_id, seq) DO UPDATE`, tidak ada `INSERT OR REPLACE` di `crates/storage/src` → AC-17

### Commands

- [x] `cargo test --locked -p stackly-domain --lib` → 115 test lulus, termasuk tipe `operation::journal` (`JournalPayload`, `StepSeq`, `CreatedSeq`, `FailureError`, `OperationJournal`, `JournalStep`, `CreatedRecord`), `LockKey`, `RecoveryHint`, `Operation::touch`, `Outcome::Failed`, `OperationKind::resumable()` dan `interrupt_policy()` → AC-2, AC-10, AC-11, AC-17
- [x] `cargo test --locked -p stackly-testkit` → semua target lulus, termasuk `fake_registry_passes_the_conformance_suite` dan `the_conformance_suite_passes_on_a_registry_that_already_holds_data` (journal, unique key dengan `IdempotencyKeyTaken`, upsert, cascade prune) → AC-2, AC-17
- [x] `cargo test --locked -p stackly-storage` → 27 unit test, 39 test `open`, 48 test `registry` lulus, termasuk `journal::migrating_v1_keeps_the_newest_key_and_backfills_a_journal_per_operation`, `journal::the_resumable_kinds_of_migration_2_match_the_domain`, `journal::the_v2_fixture_journal_rows_read_back_through_the_registry`, `fixture::frozen_fixtures_are_unchanged`, `prune::prune_deletes_operations_in_batches_with_their_journal_rows`, `registry::prune::tests::operations_prune_500_per_transaction_and_other_history_5000`, dan `conformance::the_sqlite_registry_passes_the_registry_conformance_suite` → AC-17

### Acceptance-criteria coverage

- AC-17 · lengkap: migrasi, fixture beku `registry-v2.sql`, migrasi maju dari v1 dengan key ganda, conformance di `FakeRegistry` dan `SqliteRegistry`, cascade prune 500 per transaksi, operasi `needs_recovery` tidak pernah di prune
- AC-2, AC-10, AC-11 · bagian data (key unik, `resumable` dibekukan per operasi, `recovery_hint`); perilakunya di Milestone 3

## Milestone 2: Primitive hapus aman

### Commands

- [x] `cargo test --locked -p stackly-testkit` (bagian filesystem) → `fake_filesystem_passes_the_conformance_suite`, `fake_remove_tree_stops_at_another_device`, `remove_tree_outside_the_test_roots_fails_and_removes_nothing`, dan `a_remove_tree_fault_fires_before_anything_is_removed` lulus → AC-9
- [x] `cargo test --locked -p stackly-platform-macos` → 3 unit test, 12 test `layout`, 3 test `remove_tree`, 5 test `secret_store` lulus; `macos_filesystem_passes_the_conformance_suite` di 12 test itu → AC-9
- [x] `cargo test --locked -p stackly-core --lib ownership` → 16 test `ownership::tests` lulus: path di luar root, root itu sendiri, path relatif atau bertitik ditolak sebelum IO, symlink di jalan menuju target tidak menghapus apa pun, symlink yang ditanam di pohon milik operasi terhapus sebagai dirinya → AC-8, AC-9
- [x] `cargo xtask evidence run --kind fixture --out <scratch>/ev-engine -- cargo test --locked -p stackly-core --test operation_engine -p stackly-platform-macos --test remove_tree` → 6 record, semua PASS, 0 masalah; untuk milestone ini: `macos_remove_tree_stops_at_another_device` (VAL-007, disk image `hdiutil` ter attach, tidak `BLOCKED` di run ini) dan `remove_tree_never_follows_a_folder_swapped_for_a_symlink` (VAL-007, "809 remove_tree calls against 2726 swap cycles in 2000 ms ... all 6 sentinel entries identical") → AC-9

### Acceptance-criteria coverage

- AC-9 · lengkap: conformance di `FakeFilesystem` dan `MacosFilesystem`, `CrossesDevice` lewat `hdiutil`, kedalaman, dan test balapan 2 detik dengan evidence VAL-007
- AC-8 · bagian `remove_owned_tree`; kompensasi penuh di Milestone 3

## Milestone 3: Engine dari ujung ke ujung di atas fake

### Commands

- [x] `cargo test --locked -p stackly-core --test operation_engine` → 43 test lulus di enam modul (`flow`, `cancel`, `compensation`, `failures`, `restart`, `close_interrupted`) → AC-1 sampai AC-8, AC-10 sampai AC-16
- [x] `cargo test --locked -p stackly-core --lib` → 131 test lulus, termasuk `operation::lock::tests` (key diambil sekaligus atau tidak sama sekali, lock internal gagal seketika, commit yang melepas key tidak pernah terlihat dengan key masih dipegang) dan `operation::work::tests` → AC-4, AC-11
- [x] `cargo test --locked -p stackly-core --test operation_engine flow::` → 10 test lulus: urutan event dan revision, 16 begin satu key (satu `Created`, 15 `Replayed`, satu efek, key bebas lagi setelah prune), antrian FIFO dan paralel, `BusyPolicy::Reject` menjadi `Busy` dengan pemegangnya, batas 8 berjalan dan 64 antri, penunggu yang ditutup saat pemegangnya `needs_recovery`, revisi basi saat antri, langkah tertahan tanpa transaksi registry, progress paling banyak dua commit per detik, task request yang dibatalkan, dan finalize yang diulang setelah generation conflict → AC-1, AC-2, AC-4, AC-5, AC-6, AC-12, AC-13, AC-15
- [x] `cargo test --locked -p stackly-core --test operation_engine cancel::` dan `compensation::` → 10 test lulus: setiap disposisi cancel, operasi hasil resume yang sudah melewati titik tak bisa dibalik tetap berjalan, skenario restore palsu, symlink keluar terhapus sebagai link, manifest operasi lain `not_owned`, folder run bersama dibiarkan, item yang sudah ada tidak pernah dikompensasi, kompensasi gagal lalu abandon, kompensator kustom tanpa registrasi, dan `compensator_conformance` → AC-7, AC-8
- [x] `cargo test --locked -p stackly-core --test operation_engine restart::` dan `close_interrupted::` → 14 test lulus: resume setelah restart dengan `attempts` bertambah, operasi yang tidak bisa dilanjutkan dikompensasi atau `needs_recovery`, crash di tengah kompensasi, operasi antri dibangun ulang atau ditutup, pembersih scratch, resume dan abandon dengan aturan sisa, shutdown dalam grace (termasuk `cancel_requested`), `CloseInterrupted` dan `precheck` `NoOp` → AC-10, AC-11, AC-14
- [x] `cargo test --locked -p stackly-core --test operation_engine failures::` → 6 test lulus: panic, catatan `intended` saat sukses, commit `start` gagal lalu diulang, tulisan journal gagal, kegagalan disimpan lewat `Redactor`, input di atas batas ditolak tanpa baris → AC-16
- [x] Evidence run `ev-engine` di atas → untuk milestone ini: REQ-054 dan AC-054 (`cancel::a_cancelled_fake_restore_keeps_the_target_or_reports_its_point_of_no_return`), BR-023 dan VAL-007 (`compensation::a_failed_create_removes_its_own_tree_and_a_planted_symlink_only_as_a_link`), VAL-015 (`flow::a_resource_that_changed_while_queued_fails_the_operation_before_any_effect`), REQ-048, AC-048, dan NFR-005 (`flow::sixteen_begins_with_one_key_create_one_operation_and_run_its_work_once`), semuanya PASS → AC-2, AC-7, AC-8, AC-15
- [x] Probe sementara (ditambah ke `flow.rs`, dijalankan, lalu dikembalikan): operasi dengan `Origin::Background` tersimpan dengan `idempotency_key` kosong, `method` kosong, `fingerprint` kosong, dan `trigger.origin` `background` → AC-3 (bagian latar; test permanennya milik `/test`)

### Acceptance-criteria coverage

- AC-1, AC-2, AC-4 sampai AC-8, AC-10 sampai AC-16 · lengkap di atas fake; bagian SQLite, IPC, dan daemon di Milestone 4
- AC-3 · bagian engine: key dan sidik jari lewat registry, operasi latar tanpa key (probe)

## Milestone 4: Kontrak wire dan daemon

### UI / manual

- [x] Jalankan `STACKLY_ROOT=<scratch>/root STACKLY_LOG=debug ./target/debug/stacklyd`, lalu probe socket (handshake protokol 1.3, lalu setiap method operasi) → `daemon.get_health` `ready` dengan subsystem `operations` `ok`; `daemon.get_capabilities` memuat enam method `operation.*` dan event `operation.state_changed` serta `operation.progress`; `operation.get_stats` `{ running: 0, queued: 0, needsRecovery: 0, maxRunning: 8, maxQueued: 64, ... rejectedQueueFull: 0 }`; `operation.list` dengan `limit` 5 memberi halaman kosong; `limit` 0 menjadi `request.invalid_params` (field `limit`, `out_of_range`); `operation.get`, `cancel`, `resume`, dan `abandon` untuk ID yang tidak ada menjadi `resource.not_found`; log berisi `operation recovery finished`, dan SIGTERM memberi `operation engine stopped` lalu `stacklyd stopped` dengan exit 0 → AC-5, AC-11, AC-14, AC-20
- [x] Ubah header registry ke versi skema 999 di root yang sama, jalankan `stacklyd` lagi dan probe → `daemon.get_health` `degraded` dengan `operations` `unavailable` detail `registry in recovery`; keenam method operasi menjadi `registry.recovery_mode` (`reason` `schema_newer_than_app`, `reportPath`); log tidak memuat `operation recovery finished` (recover tidak berjalan) → AC-5 (spec 0007 *Start daemon dan pemulihan*, recovery mode)

### Commands

- [x] `cargo test --locked -p stackly-protocol` → 120 unit test, 28 test `ipc_fixtures`, dan target lain lulus, termasuk DTO dan `Validate` `ListOperationsParams`, `OperationCursor`, `fingerprint` (`two_values_that_differ_only_in_a_secret_have_the_same_fingerprint`), dan tabel katalog area `operation` → AC-3, AC-18, AC-20
- [x] `swift test --package-path clients/swift --filter "OperationContractTests|IpcFixtureTests"` → 12 test di 2 suite lulus, termasuk 69 kasus fixture → AC-18
- [x] `cargo xtask codegen --check` → "19 generated files are up to date"; `ProtocolVersion::CURRENT` 1.3 → AC-18
- [x] `cargo test --locked -p stacklyd --lib dto` → 7 test lulus: setiap kode domain punya nilai wire yang dikenal, setiap state punya state wire, client yang tidak dikenal dibiarkan kosong, error tersimpan mengambil kategori dari katalog dengan `operationId`, kode di luar katalog menjadi `internal.unexpected`, detail di luar entri katalog dibuang, manager tanpa resource → AC-16, AC-18
- [x] `cargo test --locked -p stacklyd --test operations` → 21 test lulus di atas `SqliteRegistry` nyata dan client IPC nyata: event dan revision urut, replay dan penolakan key, key yang salah format, client yang pergi dan task request yang dibatalkan setelah `begin`, cancel sebelum dan sesudah titik tak bisa dibalik, resume (dengan subsystem `degraded` lalu `ok`), abandon, panic dengan error katalog, kelengkapan katalog, paging dan filter `operation.list`, potongan halaman 900 KiB, baris log request dengan `operation_id`, recovery mode, `daemon.starting` selama slot start, restart di tengah operasi, stop yang memarkir, sinyal kedua → AC-1, AC-3, AC-5, AC-6, AC-10, AC-11, AC-13, AC-14, AC-16, AC-19, AC-20
- [x] `cargo test --locked -p stacklyd` → semua target lulus (94 unit, 21 `operations`, 40 `server`, 9 `served`, dan lainnya), termasuk `lifecycle::health_reports_starting_while_the_start_slot_is_held_then_ready` yang kini melihat `operations` `ok` → AC-5
- [x] `cargo xtask evidence run --kind fixture --out <scratch>/ev-daemon -- cargo test --locked -p stacklyd --test operations` → 3 record, semua PASS, 0 masalah: NFR-006 "begin p50 22002 us, p99 38852 us; over the socket p50 20840 us, p99 43269 us; added p50 0 us, p99 4417 us"; REQ-048 "50 commits and 50 reads in 2351 ms of a 5 s held step: slowest commit 6630 us, slowest read 5831 us"; NFR-011 pindaian sentinel file registry dan log daemon → AC-1, AC-3, AC-12, AC-19

### Acceptance-criteria coverage

- AC-1 · NFR-006 di `SqliteRegistry`: p99 begin 38,9 ms (batas 500 ms), socket menambah 4,4 ms di p99 (batas 50 ms)
- AC-3 · lewat socket: tanpa key menjadi `operation.idempotency_key_required` tanpa baris, sidik jari lain menjadi `operation.idempotency_key_reused` dengan `operationId`, dan sentinel tidak ada di registry maupun log; method pemula lewat socket memakai method khusus test (`test.start_operation`, `Handlers::test_starting_method`, provisional F10) karena belum ada method kontrak yang memulai operasi
- AC-5 · `operation.get_stats` dan subsystem `operations` (`ok`, `degraded` dengan jumlah, `unavailable` di recovery mode)
- AC-6, AC-10, AC-11, AC-13, AC-14, AC-16 · lewat wire dan restart sungguhan, di atas bukti fake Milestone 3
- AC-12 · di `SqliteRegistry` dengan evidence REQ-048
- AC-18 · fixture, Swift, codegen, minor 1.3 (dibangun di kontrak 1.3)
- AC-19 · baris request method pemula dan method yang menyebut operasi membawa `operation_id`; transisi state di level info dengan `operation_id` dan `kind`; baris di task operasi membawa span `operation_id` dan `kind`; params dan secret tidak pernah tercatat
- AC-20 · paging, cursor, `activeOnly`, `resource`, potongan 900 KiB, `resource.not_found`

## Ringkasan AC

- AC-1 sampai AC-20 · semua terpenuhi dengan bukti di atas
- Bagian AC-3 "operasi latar tidak membawa key" dibuktikan lewat probe sementara saat verify, lalu dikunci `/test` di `flow::a_background_operation_records_no_key_method_or_fingerprint_and_a_request_records_all_three`
- Kompensator `secret_ref` tetap provisional sampai spec 0011 langkah 5 (bukan acceptance criterion spec ini; tercatat di `apps/daemon/src/operations.rs`)
