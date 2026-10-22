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

## Milestone 3: PostgreSQL 17 and 18 side by side on this Mac

Langkah Build plan 6 sampai 8. Native: butuh `postgresql-17.11-r2-macos-aarch64` dan `postgresql-18.6-r2-macos-aarch64` di cache runtime xtask (`~/Library/Caches/dev.stackly.xtask/runtime/`); tanpa unduhan dan tanpa jaringan (daemon test memakai `FakeFetcher` kosong). Bagian ini ditulis oleh `dev-f25-m3`; langkahnya belum dicentang, itu tugas `/check verify`.

### UI / manual

- [x] Tidak ada langkah manual di milestone ini. JOURNEY-03 di aplikasi (AC-10) menunggu milestone 4.

### Commands

- [x] `cargo test -p stacklyd --test postgresql` (tanpa `--ignored`) → kompilasi bersih; 38 test PostgreSQL native tetap `ignored`
- [x] `cargo test -p stacklyd --test postgresql --locked -- --ignored --exact side_by_side::postgresql_18_beside_17_journey` → lulus (sekitar 35 detik): dua PostgreSQL 17 dari spec 0024 (`pg17-dev` running, `pg17-test` di stop), snapshot (`instance.list` sebagai himpunan ID; `instance.get`: `runtimeId`, `provider`, `lifecycle`, `desired`, readiness, setiap endpoint dengan `port`, `portMode`, `bound`, PID dan waktu start, `restart`; digest SHA-256 dan mode `instances/<id>/` dan `logs/instances/<id>/` instance yang berhenti) sama persis sebelum dan sesudah `runtime.install` PostgreSQL 18 dari entri cache (disalin ke `downloads_dir()`, tanpa jaringan) → AC-1 (native). `pg18-dev` dibuat di runtime 18 lewat `instance.create` dan di start; ketiganya `ready`, `instance.list` menampilkan `runtimeVersion` `17.11`, `17.11`, dan `18.6` dengan `runtimeId` kedua PostgreSQL 17 sama dan `runtimeId` `pg18-dev` berbeda, `SHOW server_version` lewat `psql` runtime masing masing `17.11`, `17.11`, `18.6`, tiga `system_identifier` berbeda, `PG_VERSION` dan `data.initializedLine` `17`, `17`, `18`, tiga port berbeda, dan semua path `InstancePaths` `pg18-dev` berbeda dari kedua PostgreSQL 17 → AC-2. Tiga sentinel berbeda (nama instance ditambah UUID v7), `assert_isolated` lolos untuk ketiganya; satu per satu stop (dua lainnya tetap `ready` dan tetap membaca sentinel sendiri) lalu start, `assert_isolated` lolos lagi; sesudah tiga siklus `runtimeId` dan `provider` setiap instance sama dan `SHOW server_version` masih cocok → AC-3
- [x] `cargo test -p stacklyd --test postgresql --locked -- --ignored --exact side_by_side::another_major_is_refused_at_start_and_by_the_rebind_check` → lulus (sekitar 40 detik): `pg17-test` dan `pg18-dev` di stop, daemon in process berhenti (postmaster `pg17-dev` hidup terus); `rebind::check` dari registry yang dibuka test memberi `supervisor.data_incompatible` `outcome` `incompatible` dengan `data` `17` `runtime` `18` untuk `pg17-test` dan `data` `18` `runtime` `17` untuk `pg18-dev`, hint `incompatible` lewat `codes::supervisor::hint`, dan `Current` untuk `pg17-test` ke runtime nya sendiri; baris `Instance` dan `Runtime` (nilai, `revision`, dan generasi registry), `instance-manifest.json` ketiga instance, dan digest pohon kedua instance yang berhenti sama persis sesudahnya → AC-7. Registry ditulis test (`Site::rebind_instance`, daemon berhenti) sehingga `pg17-test` terikat ke runtime 18 dan `pg18-dev` ke runtime 17, folder tidak berubah; daemon jalan lagi, `pg17-dev` teradopsi dengan PID dan waktu start yang sama dan membaca sentinel nya; `instance.start` kedua instance berakhir `failed` di langkah `verify` (tanpa langkah `journal`, `render`, atau `spawn`) dengan `supervisor.data_incompatible`, `outcome` `incompatible`, `data` dan `runtime` yang benar, dan `error.hint` sama dengan hint `incompatible`; keduanya tetap `stopped` tanpa `process`, tanpa `postmaster.pid`, `serving(data)` kosong, dan tanpa proses `postgres` yang menyebut folder datanya; manifest dan digest pohon sama. Daemon berhenti, `runtimeId` dikembalikan, daemon jalan, `pg17-test` dan `pg18-dev` start sampai `ready` dan `assert_isolated` lolos untuk ketiganya → AC-4
- [x] `HOME=<folder kosong> target/debug/deps/postgresql-<hash> --ignored --exact side_by_side::postgresql_18_beside_17_journey side_by_side::another_major_is_refused_at_start_and_by_the_rebind_check` (binary dari `cargo test --no-run`) → kedua test selesai dalam 0,00 detik dan mencatat `BLOCKED` dengan alasan `artifact postgresql-17.11-r2-macos-aarch64 is not in the runtime cache; artifact postgresql-18.6-r2-macos-aarch64 is not in the runtime cache` dan owner `@ojiepermana`, tanpa unduhan → AC-9
- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test postgresql --locked -- --ignored --exact side_by_side::<nama test>` untuk masing masing test → `1 native records (PASS 1, FAIL 0, BLOCKED 0)`, `0 problems`; subject `postgresql` untuk `17.11` (digest `artifact_sha256(PG17)`) dan `18.6` (digest `artifact_sha256(PG18)`), `source` `managed`; `expected` test kedua menyebut bahwa bukti AC-043 hanya bagian (pemeriksaan core pada data asli; aksi pengguna milik fitur 58), dan keduanya menyebut JOURNEY-03 → AC-9. Record di `target/evidence/` tidak di commit.
- [x] `cargo test -p stacklyd --test postgresql --locked -- --ignored --exact mismatch::postgresql_17_data_is_refused_by_a_runtime_18_and_a_missing_or_changed_marker_is_refused same_runtime::two_postgresql_17_instances_from_one_runtime_keep_their_sentinels` → 2 test lulus: harness yang berubah (`in_process` kini memanggil `in_process_on`, folder paket yang di pin dibuat privat, `create_params` memanggil `create_params_on`) tidak mengubah perilaku test PostgreSQL lama
- [x] `cargo fmt --all --check`, `cargo clippy -p stacklyd --all-targets --locked -- -D warnings`, `cargo xtask check-deps`, dan `cargo xtask evidence check` → semuanya bersih

### Acceptance-criteria coverage

- AC-1 · bagian native selesai di sini (bagian fixture di milestone 2).
- AC-2, AC-3, AC-4, AC-7 · selesai di sini. Bagian AC-043 tentang aksi pengguna (method, CLI, GUI, stop terkontrol, arahan ke backup dan restore) milik fitur 58; bagian backup dan restore AC-035 milik fitur 51 dan 52.
- AC-9 · kedua test native memasang guard `native` dengan `covers` dan subject kedua runtime; jalur `BLOCKED` untuk artefak yang hilang diperiksa (nama setiap artefak yang hilang). Pembersihan `cargo xtask runtime spike --cleanup` tidak dijalankan oleh `dev-f25-m3` (menunggu izin lead karena sesi lain bisa menjalankan test native); sebagai gantinya `ps` tidak menemukan proses yang menyebut folder test root sesudah run.
- AC-10 · menunggu milestone 4.

### Catatan implementasi

- Harness tidak memakai `CatalogOverride::Location` seperti tertulis di spec: `providers/` hanya menyimpan sumber catalog (`catalog.toml`), sedangkan daemon membaca `catalog.json` hasil `cargo xtask catalog build` dan memberi `catalog.unavailable` (`missing`). `Site::in_process_offline` memakai `CatalogOverride::Preloaded(Catalog::for_tests(..))` dengan paket `providers/postgresql` asli, pola `native_runtime_install` (spec 0018 AC-20); gerbang catalog `runtime.install` tetap dilewati dengan entri `official/postgresql` yang sama.
- Dengan catalog termuat, `instance.create` menyematkan ulang paket dan menolak folder yang tidak privat; `Site::open_package` kini mengubah folder `providers/` yang di pin fixture menjadi 0700.
- `Site::rebind_instance` (spec 0019) dipakai sebagai `Site::bind_runtime`: perilakunya sama (registry ditulis dengan daemon berhenti, `revision` naik 1, tanpa menyentuh file instance).
- Entri cache disalin dengan `stackly_testkit::RuntimeCache::copy_to` (digest dan ukuran diperiksa) dan helper baru `artifact_size`; `Site::open` dan test PostgreSQL lain tetap memakai `cached_artifact` seperti sebelumnya.

## Milestone 4: JOURNEY-03 in the app, manual

Langkah Build plan 9. Native dan manual: butuh `postgresql-17.11-r2-macos-aarch64` (sha256 `04b1eff1...`) dan `postgresql-18.6-r2-macos-aarch64` (sha256 `88bb7533...`) di cache xtask (`~/Library/Caches/dev.stackly.xtask/runtime/`); tanpa unduhan dan tanpa jaringan. Bagian ini ditulis oleh `dev-f25-m4`; langkah GUI dijalankan user sendiri di Mac ini dan belum dicentang, itu tugas `/check verify` setelah user mengirim hasilnya. Persiapan root dev (catalog dari `cargo xtask catalog build`, `STACKLY_CATALOG_DIR`, dan salinan artefak ke `<root>/cache/downloads/`) sudah diuji kering lewat CLI: `stackly runtime install official/postgresql@17` dan `@18` selesai dari cache dalam sekitar 9 detik tiap runtime, tanpa jaringan.

### UI / manual

Semua langkah ada di [docs/manual.md](../../manual.md) bagian T; jangan salin ulang ke sini.

- [ ] `docs/manual.md` bagian T, langkah 7 dan 9: tombol Get di Catalog, PostgreSQL, memasang 17.11 lalu 18.6 dari cache root dev, dan Runtimes menampilkan kedua runtime → AC-10
- [ ] Bagian T, langkah 8, 9.2, 9.3, dan 11: `pg17-dev` Ready sebelum, selama, dan sesudah install PostgreSQL 18 (PID dan waktu start sama di `stackly instance get`) → AC-10, AC-1 (sisi aplikasi)
- [ ] Bagian T, langkah 10: sheet New Instance menawarkan `PostgreSQL 17.11` dan `PostgreSQL 18.6` → AC-10
- [ ] Bagian T, langkah 11 dan 12: `pg18-dev` dibuat dari 18.6 dan Ready; kolom Version halaman Instances menampilkan `17.11` dan `18.6` berdampingan, dengan port berbeda → AC-10, AC-2 (sisi aplikasi)
- [ ] Bagian T, langkah 13 sampai 15: tab Summary `pg18-dev` menampilkan endpoint dan connection nya sendiri; connection yang disalin terhubung lewat `psql` runtime 18 dengan `SELECT version()` berisi `18.6`, dan hal yang sama untuk `pg17-dev` berisi `17.11` → AC-10
- [ ] Bagian T, langkah 16 dan 17: dua postmaster berjalan dari folder runtime berbeda, lalu pembersihan tuntas (tanpa proses atau folder sisa) → AC-10
- [ ] Empat tangkapan layar (Runtimes, New Instance dengan daftar Runtime, Instances dengan kolom Version, Summary `pg18-dev`) disalin ke `evidence/manual/files/01a163bd-1573-774e-8367-c3db3d283798/` → AC-10

### Commands

- [ ] `cargo xtask evidence check` → bersih, dan record `evidence/manual/2026-10-22-journey-03-in-the-app-postgresql-18-beside-17-on.json` (`kind` `native`, `covers` REQ-035 dan REQ-043, JOURNEY-03 di `expected`, owner `@ojiepermana`) terisi: `steps`, `actual`, `status` `PASS` (atau `BLOCKED` dengan alasan dan owner bila artefak tidak ada di cache), `reviewer`, dan `finishedAt` → AC-10, AC-9
- [ ] `ps -axww -o pid=,args= | grep -E "stk-pg1718" | grep -v grep` → kosong sesudah pembersihan bagian T

### Acceptance-criteria coverage

- AC-10 · satu record manual `native` berstatus `NOT_RUN` menunggu user menjalankan bagian T; tanpa artefak di cache, status akhirnya `BLOCKED`.
- AC-1 dan AC-2 · bukti native sudah di milestone 3; langkah di atas hanya menambah sisi aplikasi (PID `pg17-dev` tidak berubah dan kolom Version).
- AC-1 sampai AC-9 lainnya selesai di milestone 1 sampai 3.
