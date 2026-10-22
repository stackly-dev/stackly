# Verify: PostgreSQL 17 & 18 berdampingan · spec 0025 · updated 2026-10-22

_Langkah diturunkan dari acceptance criteria spec 0025. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Bagian Milestone 2 ditulis oleh `dev-f25-m2`; bagian Milestone 1 ditulis `dev-f25-m1` dan digabung lead saat merge._

## Milestone 1: Shared data check, rebind check, and migration hint on fixtures

Langkah Build plan 1 sampai 4. Tidak ada UI di milestone ini dan semuanya berjalan di fixture (fake runtime, `FakeRegistry`, `FakePlatform`, paket `providers/postgresql` asli, dan daemon in process), jadi tidak ada bukti native.

### UI / manual

- [x] Tidak ada langkah manual. Penolakan mayor lain di aplikasi dan CLI menunggu milestone 4 (JOURNEY-03 di aplikasi) dan fitur 58.

### Commands

- [x] `cargo test -p stackly-core --test supervisor_start data_compat` → 3 test lulus: `a_start_refuses_data_its_runtime_may_not_run_before_any_spawn` (kasus `incompatible`, `marker_mismatch`, `marker_missing`, dan `marker_unreadable` gagal di `verify` dengan `supervisor.data_incompatible`, `outcome` yang benar, `data` hanya untuk `incompatible`, instance tetap `stopped`, tanpa spawn), `an_incompatible_start_names_both_lines` (data `1.0`, runtime `2.0`), dan `data_compatibility_is_the_same_for_start_and_rebind` (kode dan details start, panggilan langsung `check_data_compatibility`, dan `rebind::check` sama untuk keempat hasil) → AC-8
- [x] `cargo test -p stackly-core --test supervisor_start` → seluruh test start lama lulus tanpa perubahan harapan, termasuk `a_start_checks_the_package_the_folder_the_marker_and_the_runtime_first` → AC-8
- [x] `cargo test -p stackly-core --test instance_rebind` → 6 test lulus pada paket `providers/postgresql` asli di `FakePlatform` dan `FakeRegistry`: `rebind_check_refuses_another_postgresql_major` (17 ke 18 dan 18 ke 17 `incompatible` dengan line yang benar, runtime sendiri `Current`, runtime 17 lain `Compatible`), `a_package_that_allows_the_pair_gives_compatible` (`lineCompatibility` satu arah), `a_missing_changed_or_unreadable_marker_is_refused_with_its_outcome` (`PG_VERSION` hilang, `16`, dan folder), `the_refusals_come_in_the_order_of_the_spec` (sembilan pemeriksaan pertama, satu per satu), `a_running_instance_is_checked_like_a_stopped_one`, dan `rebind_and_start_share_the_answer_of_the_data_check`; setiap jalur menyatakan snapshot registry (nilai dan generation), mutasi file system, dan spawn tidak berubah → AC-6 (tanpa tulis, urutan, dan kode), AC-8
- [x] `cargo test -p stackly-protocol supervisor_hint` → `supervisor_hint_names_the_migration` (teks persis, paling banyak 200 byte, tanpa `{`) dan `supervisor_hint_stays_silent_without_a_known_outcome` (`unknown`, nilai asing, tanpa `outcome`, kode lain, dan kode asing memberi `None`) lulus → AC-5 (bagian protocol)
- [x] `cargo test -p stacklyd --test supervisor data_compat` → `data_incompatible_failure_carries_the_hint` lulus di daemon in process: instance fake dengan `FAKE_VERSION` diubah menjadi `0.2` gagal di tahap `verify` dengan `outcome` `marker_mismatch` dan hint penanda di `failure.error.hint`; instance fake yang diikat (daemon berhenti, `Site::bind_runtime`) ke runtime fake kedua ber line `0.2` gagal dengan `outcome` `incompatible`, `data` `0.1`, `runtime` `0.2`, dan hint migrasi; kedua instance tetap `stopped` tanpa `process` dan tanpa proses yang melayani folder datanya → AC-5
- [x] `cargo xtask codegen --check` → bersih, tanpa kenaikan `ProtocolVersion::CURRENT` dan tanpa perubahan `protocols/` → AC-5
- [x] `cargo fmt --all --check`, `cargo clippy -p stackly-core -p stackly-protocol -p stacklyd --all-targets --locked -- -D warnings`, `cargo xtask check-deps`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-protocol -p stackly-core` → semuanya bersih

### Acceptance-criteria coverage

- AC-5 · hint di `stackly-protocol` dan di `ErrorBody` kegagalan operasi (dua kasus fixture) ada di sini. Hint untuk penolakan `rebind::check` memakai fungsi yang sama dan tidak punya pemakai produksi sampai fitur 58.
- AC-6 · semua baris tabel *Pemeriksaan rebind* diuji di fixture dengan paket PostgreSQL asli dan data `PG_VERSION` tiruan; tahap data PostgreSQL asli menunggu milestone 3 (AC-7). Pemeriksaan teks hint atas error `rebind::check` terjadi lewat `codes::supervisor::hint` yang sama di test protocol dan daemon, karena `stackly-core` tidak bergantung pada `stackly-protocol`.
- AC-8 · selesai di milestone ini.
- AC-1, AC-2, AC-3, AC-4, AC-7, AC-9 menunggu milestone 2 dan 3; AC-10 menunggu milestone 4.

## Milestone 2: Installing a runtime leaves existing instances alone, on fixtures

### UI / manual

- [x] Buka `apps/daemon/tests/runtime/instances_untouched.rs` → `CheckFaking` hanya mengalihkan helper `ChildRole::RuntimeCheck` ke proses fake; proses instance (`spawn_main`) tetap proses sungguhan dari `stackly-fake-server`, jadi instance yang `running` benar benar punya PID dan waktu start → AC-1
- [x] Buka `package()` di file yang sama → artefak `fake-0.2.0-macos-aarch64` (line `0.2`, versi `0.2.0`, `tar_gz`) berisi salinan `stackly-fake-server` dibangun dari `providers/fake`; runtime `0.1.0` milik instance tetap dari `seed_fake`, jadi install menambah line lain dari software yang sama → AC-1

### Commands

- [x] `cargo test -p stacklyd --test runtime instances_untouched` → 1 test lulus (`runtime_install_leaves_existing_instances_untouched`, sekitar 5 detik): dua instance fake dibuat, keduanya di start lalu satu di stop; snapshot sebelum diambil (`instance.list` sebagai himpunan ID; `instance.get`: `runtimeId`, `provider` beserta `revision`, `lifecycle`, `desired`, readiness, setiap endpoint dengan `port`, `portMode`, `bound`, `process` PID dan waktu start, dan `restart`; digest SHA-256 dan mode setiap file biasa di `instances/<id>/` dan `logs/instances/<id>/` instance yang berhenti, diambil ulang sampai dua pembacaan berjarak 200 ms sama); `runtime.install` line `0.2` yang dilayani `FakeFetcher` dengan satu byte diubah gagal di tahap Verify digest dengan `runtime.digest_mismatch` dan `runtime.list` tetap satu runtime; snapshot sama persis; lalu install dengan byte yang benar sukses (`runtime.list` dua runtime, runtime lama tidak berubah) dan snapshot sama persis lagi; instance yang berjalan tetap `running` dan `ready` dengan PID, waktu start, dan jumlah restart yang sama → AC-1
- [x] `cargo test -p stacklyd --test runtime` → 14 test lulus, test lama tidak terpengaruh (test baru memakai site sendiri, `support.rs` tidak diubah) → AC-1
- [x] `cargo clippy -p stacklyd --all-targets --locked -- -D warnings` dan `cargo fmt --all --check` → bersih
- [x] `cargo xtask check-deps` → "13 crates follow the dependency, unsafe, and source rules"

### Acceptance-criteria coverage

- AC-1 · bagian fixture: install sukses dan install yang gagal di Verify digest, instance `running` dan `stopped`; bagian native (dua PostgreSQL 17, PostgreSQL 18 dari cache) menunggu Milestone 3 (`postgresql_18_beside_17_journey`)
- AC-2 sampai AC-9 · bukan milik Milestone 2
- Catatan: folder `logs/instances/<id>/` instance fake kosong (dummy server tidak menulis file log di sana), jadi digest log hanya menjaga agar tidak ada file baru muncul; isi pohon `instances/<id>/` (manifest, `instance.lock`, `data/*`) yang dibandingkan byte demi byte.
