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
