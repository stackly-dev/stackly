# Verify: Lockfile & override lokal · spec 0045 · updated 2026-11-13

_Langkah diturunkan dari acceptance criteria spec 0045. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27, dengan `export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`._

## Milestone 1: Lock, override, dan export di daemon

### UI / manual

- Tidak ada langkah manual di milestone ini; langkah GUI nyata ada di Milestone 3 (langkah 9).

### Commands

- [ ] `cargo test -p stackly-core --test environment lock::` → 7 test lulus: lock ERP enam entri sama dengan golden `crates/core/tests/fixtures/environments/erp-development.lock`, jalan kedua `kept` dengan byte sama, tanpa `local:`, port, settings, path root, atau URL; selector tanpa artefak memberi blocker tanpa lock; service `instanceRef` tidak masuk lock; `refresh` `none` mempertahankan versi saat `stable` naik, `all` menaikkan semua, `services` satu, alias tak dikenal `ServiceNotFound`; Mac Intel menambah artefaknya tanpa ganti versi dan entri yang hilang `removed`; lock `apiVersion` asing tidak pernah ditimpa, lock rusak hanya diganti dengan `all`; loader menolak field asing, target ganda, tag YAML, dan alias ke anchor yang tidak ada; satu kasus per `LockStaleReason` dan port, settings, komentar, urutan tetap `in_sync` → AC-1, AC-2, AC-3, AC-4
- [ ] `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test environment lock::the_erp` lalu `git status --short` → golden tidak berubah (emitter tetap) → AC-1
- [ ] `cargo test -p stackly-core --test environment plan_lock::` → 5 test lulus: entri cocok memakai versi terkunci saat catalog `stable` pindah (`source` `lock`), service baru `catalog` dan `stale` `not_locked`; `locked` tanpa lock `env.lock_missing`, lock rusak `env.lock_invalid` di kedua mode, setiap alias basi satu `env.lock_stale`, tidak ada item `catalog`; Mac ber target ketiga `env.lock_no_artifact`, digest asing `not_in_catalog`, versi `version_mismatch`, target `target_mismatch`, ID `id_mismatch`, revision provider `env.lock_provider_mismatch`, macOS lebih lama `catalog.not_installable`, semuanya di kedua mode dan tanpa runtime pengganti; member dengan runtime lain dari lock `keep` dengan `differences` `lock` dan tetap applicable dengan `locked`; `planDigest` berubah karena `locked`, byte lock, dan revision provider → AC-5, AC-6, AC-7, AC-8, AC-12
- [ ] `cargo test -p stackly-core --test environment plan_local::` → 5 test lulus: satu file override per kode *Aturan override* menjadi satu blocker `env.local_invalid`, `kind` asing memakai pesan "kind must be EnvironmentLocal", alias tak dikenal hanya warning dan `ignored`, file tak terbaca `env.local_unreadable`; `env.validate` sama dengan dan tanpa file override; override port melepas bentrok dengan instance lain, `applied`, byte `stackly.yaml` dan `stackly.lock` serta `manifestSha256` tidak berubah; member membandingkan port terhadap override (`compared`, storage `not_applied_existing`); override storage ke `<p>` dan `<p>/Stackly` mengisi `PlanItem.storage`, path tak dikenal `env.override_location_unknown`, folder lokasi yang hilang menjadi error baris `storage_location` → AC-9, AC-10, AC-11
- [ ] `cargo test -p stackly-core --test create_operation a_draft_data_location` → create dengan `CreateDraft.data_location` menaruh lima path data di lokasi itu, log di lokasi default, dan default Settings tidak berubah → AC-11
- [ ] `cargo test -p stackly-core --test environment export::` → 2 test lulus: tiga variabel per endpoint IP dalam urutan snapshot dan paket, dotenv dan shell tepat, IPv6 dalam kurung siku, `mayChange` untuk `auto` yang tidak running, `skipped` `unix_endpoint`, `port_not_assigned`, `not_created`, `retained`, `package_unavailable`, `name_collision`; nilai yang dikenal `Redactor` masuk `skipped` `redacted` dan tidak ada di `text` → AC-14, AC-15
- [ ] `cargo test -p stackly-storage --test registry the_lock_digest` → `lock_sha256` lewat `set_manifest` tersimpan, terbaca, dan kembali NULL → AC-12
- [ ] `cargo test -p stackly-protocol --test env_fixtures --test ipc_fixtures` → fixture `protocols/fixtures/env/` (plan, detail, `lock-written`, `lock-blocked`, `export-dotenv`) dan request/response `env.lock`, `env.export`, `env.plan` `locked` roundtrip; enum baru membaca nilai asing sebagai `unrecognized`; plan dari daemon tanpa lock terbaca `lock.locked` false dan `source` `catalog`; setiap kode `env.*` punya hint statis → AC-6, AC-20
- [ ] `cargo test -p stacklyd --test environment env_lock plan_and_get a_lock_moved export_prints` → 5 test lulus: `env.lock` ERP menulis hanya `stackly.lock` (pohon file, generation, operasi, dan audit sama), tanpa reference, port, path home, atau PID, jalan kedua `written` false, alias tak dikenal `env.service_not_found`; selector tanpa artefak memberi blocker tanpa file, folder di path lock `env.lock_unwritable` `not_a_file`; `env.plan` `locked` tanpa lock `env.lock_missing`, dengan lock dan override `in_sync`, `source` `lock`, override `applied`; `env.get` `checkManifest` membawa `lock` dan `local`, tanpa itu keduanya kosong, `lockSha256` tetap kosong sebelum Up; export member running tiga variabel di dotenv dan shell (juga dari sumber file), tanpa path, nilai yang dikenal `Redactor` tidak tercetak, file yang belum terdaftar `env.not_registered`; root B dengan `stackly.yaml` dan `stackly.lock` saja memilih artefak yang sama dengan root A tanpa path, port override, atau status reference A, dan lock tanpa artefak target ini `env.lock_no_artifact` tanpa runtime → AC-2, AC-13, AC-14, AC-15, AC-21
- [ ] `cargo xtask codegen --check` → semua file hasil generate up to date, termasuk `schemas/environment/lock.v1alpha1.schema.json` dan `local.v1alpha1.schema.json`, dan `protocols/contract-versions.json` mencatat 4.3 → AC-20
- [ ] `cargo test -p stackly-protocol --lib version` → `ProtocolVersion::CURRENT` 4.3 → AC-20
- [ ] `cargo xtask check-deps` → lulus → AC-20
- [ ] `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → exit 0 → AC-20
- [ ] `swift test --package-path clients/swift` dan `swift build --package-path apps/macos/Packages/StacklyUI --build-tests` → lulus dengan tipe hasil generate 4.3 → AC-20

### Acceptance-criteria coverage

- AC-1, AC-2, AC-3, AC-4 · core `environment::lock` dan `env.lock` di daemon
- AC-5, AC-6, AC-7, AC-8, AC-12 · plan dengan lock; bagian CLI AC-6 (`cli.env_locked_unsupported`) menunggu Milestone 2; penulisan `environments.lock_sha256` oleh Up milik spec 0046 (milestone ini menyediakan `set_manifest` dan kolomnya)
- AC-9, AC-10, AC-11 · `environment::local`, plan, dan `CreateDraft.data_location`; penerusan `PlanItem.storage` ke create oleh Up milik spec 0046
- AC-13 · `env.get` di daemon; tampilan GUI menunggu Milestone 3
- AC-14, AC-15 · core `environment::export` dan `env.export`; stdout CLI menunggu Milestone 2
- AC-20 · protokol 4.3, method, kode, fixture, schema
- AC-21 · daemon root A ke root B; bagian "catalog tanpa target B" dibuktikan dengan lock yang hanya punya target lain (daemon) dan dengan catalog fixture dua target (core AC-7)
- AC-16, AC-17 · Milestone 2; AC-18, AC-19 · Milestone 3
