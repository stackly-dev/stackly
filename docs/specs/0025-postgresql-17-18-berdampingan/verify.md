# Verify: PostgreSQL 17 dan 18 berdampingan · spec 0025 · updated 2026-10-22

_Langkah diturunkan dari acceptance criteria spec 0025. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah._

## Milestone 1: Shared data check, rebind check, and migration hint on fixtures

Langkah Build plan 1 sampai 4. Tidak ada UI di milestone ini dan semuanya berjalan di fixture (fake runtime, `FakeRegistry`, `FakePlatform`, paket `providers/postgresql` asli, dan daemon in process), jadi tidak ada bukti native.

### UI / manual

- [ ] Tidak ada langkah manual. Penolakan mayor lain di aplikasi dan CLI menunggu milestone 4 (JOURNEY-03 di aplikasi) dan fitur 58.

### Commands

- [ ] `cargo test -p stackly-core --test supervisor_start data_compat` → 3 test lulus: `a_start_refuses_data_its_runtime_may_not_run_before_any_spawn` (kasus `incompatible`, `marker_mismatch`, `marker_missing`, dan `marker_unreadable` gagal di `verify` dengan `supervisor.data_incompatible`, `outcome` yang benar, `data` hanya untuk `incompatible`, instance tetap `stopped`, tanpa spawn), `an_incompatible_start_names_both_lines` (data `1.0`, runtime `2.0`), dan `data_compatibility_is_the_same_for_start_and_rebind` (kode dan details start, panggilan langsung `check_data_compatibility`, dan `rebind::check` sama untuk keempat hasil) → AC-8
- [ ] `cargo test -p stackly-core --test supervisor_start` → seluruh test start lama lulus tanpa perubahan harapan, termasuk `a_start_checks_the_package_the_folder_the_marker_and_the_runtime_first` → AC-8
- [ ] `cargo test -p stackly-core --test instance_rebind` → 6 test lulus pada paket `providers/postgresql` asli di `FakePlatform` dan `FakeRegistry`: `rebind_check_refuses_another_postgresql_major` (17 ke 18 dan 18 ke 17 `incompatible` dengan line yang benar, runtime sendiri `Current`, runtime 17 lain `Compatible`), `a_package_that_allows_the_pair_gives_compatible` (`lineCompatibility` satu arah), `a_missing_changed_or_unreadable_marker_is_refused_with_its_outcome` (`PG_VERSION` hilang, `16`, dan folder), `the_refusals_come_in_the_order_of_the_spec` (sembilan pemeriksaan pertama, satu per satu), `a_running_instance_is_checked_like_a_stopped_one`, dan `rebind_and_start_share_the_answer_of_the_data_check`; setiap jalur menyatakan snapshot registry (nilai dan generation), mutasi file system, dan spawn tidak berubah → AC-6 (tanpa tulis, urutan, dan kode), AC-8
- [ ] `cargo test -p stackly-protocol supervisor_hint` → `supervisor_hint_names_the_migration` (teks persis, paling banyak 200 byte, tanpa `{`) dan `supervisor_hint_stays_silent_without_a_known_outcome` (`unknown`, nilai asing, tanpa `outcome`, kode lain, dan kode asing memberi `None`) lulus → AC-5 (bagian protocol)
- [ ] `cargo test -p stacklyd --test supervisor data_compat` → `data_incompatible_failure_carries_the_hint` lulus di daemon in process: instance fake dengan `FAKE_VERSION` diubah menjadi `0.2` gagal di tahap `verify` dengan `outcome` `marker_mismatch` dan hint penanda di `failure.error.hint`; instance fake yang diikat (daemon berhenti, `Site::bind_runtime`) ke runtime fake kedua ber line `0.2` gagal dengan `outcome` `incompatible`, `data` `0.1`, `runtime` `0.2`, dan hint migrasi; kedua instance tetap `stopped` tanpa `process` dan tanpa proses yang melayani folder datanya → AC-5
- [ ] `cargo xtask codegen --check` → bersih, tanpa kenaikan `ProtocolVersion::CURRENT` dan tanpa perubahan `protocols/` → AC-5
- [ ] `cargo fmt --all --check`, `cargo clippy -p stackly-core -p stackly-protocol -p stacklyd --all-targets --locked -- -D warnings`, `cargo xtask check-deps`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-protocol -p stackly-core` → semuanya bersih

### Acceptance-criteria coverage

- AC-5 · hint di `stackly-protocol` dan di `ErrorBody` kegagalan operasi (dua kasus fixture) ada di sini. Hint untuk penolakan `rebind::check` memakai fungsi yang sama dan tidak punya pemakai produksi sampai fitur 58.
- AC-6 · semua baris tabel *Pemeriksaan rebind* diuji di fixture dengan paket PostgreSQL asli dan data `PG_VERSION` tiruan; tahap data PostgreSQL asli menunggu milestone 3 (AC-7). Pemeriksaan teks hint atas error `rebind::check` terjadi lewat `codes::supervisor::hint` yang sama di test protocol dan daemon, karena `stackly-core` tidak bergantung pada `stackly-protocol`.
- AC-8 · selesai di milestone ini.
- AC-1, AC-2, AC-3, AC-4, AC-7, AC-9 menunggu milestone 2 dan 3; AC-10 menunggu milestone 4.
