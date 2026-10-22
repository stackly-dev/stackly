# Verify: PostgreSQL 17 & 18 berdampingan · spec 0025 · updated 2026-10-22

_Langkah diturunkan dari acceptance criteria spec 0025. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Bagian Milestone 2 ditulis oleh `dev-f25-m2`; bagian Milestone 1 ditulis `dev-f25-m1` dan digabung lead saat merge._

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
