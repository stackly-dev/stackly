# Verify: Manifest environment, validate dan plan · spec 0044 · updated 2026-11-13

_Langkah diturunkan dari acceptance criteria spec 0044. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27._

## Milestone 1: Manifest, validate, dan plan di daemon

_Bagian ini mencakup Build plan langkah 1 sampai 4 (domain dan storage, core manifest dan validate, core plan dan status, protokol 4.2). Langkah 5 (handler `env.*` di `stacklyd`, cache keanggotaan, test tanpa efek samping lewat socket) menyusul dan menambah langkahnya sendiri di sini._

### UI / manual

- [ ] Buka `schemas/environment/environment.v1alpha1.schema.json` → schema draft 2020-12 berjudul `Environment`, `additionalProperties: false` di setiap objek dengan properti, `services` berupa map ke `Service`, `port` berupa angka 1 sampai 65535 atau `auto`; file diawali komentar apa pun tidak ada (JSON murni) → AC-24
- [ ] Buka `protocols/error-codes.json` → area `env` milik spec 0044 berisi dua belas kode tabel *Kode error di wire* dengan kategori dan `details` persis seperti spec, semuanya `retryable` false; `env.lease_active` membawa `items` kind `instance` (field `instanceId`, `name`, paling banyak 32) dan detail `omitted`; area `cli` mendapat `cli.env_invalid`, `cli.env_plan_blocked`, dan `cli.environment_not_found` → AC-24
- [ ] Buka `crates/core/tests/fixtures/environments/erp-development.yaml` → enam service dengan nama endpoint, secret field, setting, dan line yang dipakai keenam paket resmi (tabel *Manifest contoh ERP*), tanpa `dependsOn` → AC-1

### Commands

- [ ] `cargo test -p stackly-domain --lib environment` → `set_manifest` menaikkan revision 1 dan menolak alias ganda atau `dependsOn` ke alias yang tidak dideklarasikan tanpa mengubah revision; `ManifestLocation` hanya menerima path absolut tanpa `.` atau `..` → AC-11, AC-12
- [ ] `cargo test -p stackly-storage --test open environments` → registry versi 13 bermigrasi ke 14 dengan semua baris tetap dan snapshot kosong; fixture beku v14 membaca snapshot `services` (urutan manifest, `dependsOn` dengan `timeoutSeconds`, service shared dengan `instanceRef`); tabel menolak nama kedua, lokasi manifest kedua, dan `services` yang bukan array JSON → AC-11, AC-12
- [ ] `cargo test -p stackly-storage --test open fixture` → `registry-v14.sql` tercatat di `FIXTURE_SHA256` dan dumper menghasilkannya lagi dari `seed/v14.sql` → AC-11
- [ ] `cargo test -p stackly-storage --test registry` dan `cargo test -p stackly-testkit` → suite conformance (SQLite dan fake) lulus, termasuk skenario `environments`: snapshot dan lokasi terbaca kembali, nama dan lokasi unik (`Integrity` di op 0), lokasi yang dilepas bisa diambil environment lain, guard membaca `GuardView::environments`, dan `DeleteEnvironment` menolak lease `active` dengan `NotDeletable` `active_lease`, membandingkan revision, lalu menghapus demand `released` dan pemakaian secret, memindahkan reference milik environment ke user (revision naik 1), dan menyisakan instance → AC-11, AC-12, AC-13
- [ ] `cargo test -p stackly-core --test environment validate` → manifest contoh ERP lolos tanpa finding; setiap file di `fixtures/environments/invalid/` (satu per kode `FindingCode`, 34 file) menghasilkan kodenya sendiri dan pesannya selalu template kode itu; cycle tiga service bernama `api -> db -> cache -> api` muncul bersama `self_dependency`, `unknown_dependency`, dan `endpoint_conflict` yang menunjuk kedua endpoint; anchor dan alias ditunjuk di `7:8` dan `9:8`; versi tanpa kutip meminta kutip; sentinel di `secretRefs`, `settings`, port, dan posisi bertipe salah tidak pernah muncul di finding atau snapshot → AC-1, AC-2, AC-3, AC-8
- [ ] `cargo test -p stackly-core --test environment plan` → di root kosong plan manifest contoh ERP berisi enam `install_runtime`, enam `create_instance`, enam `start_instance`, nol `keep`, `applicable` true, lima reference `unregistered` (`needsSecrets` 5), dan `planDigest`; urutan start fixture `graph.yaml` `db, cache, api, web`; plan tidak menaikkan generation registry, tidak mengubah satu file pun di test root, dan tidak memanggil proses, tulis secret, atau unduhan → AC-5, AC-6, AC-10, AC-25 (bagian core)
- [ ] `cargo test -p stackly-core --test environment plan::a_fixed_port_of_another_instance_and_a_taken_name_block_the_plan` dan `plan::a_port_in_use_and_an_unresolved_selector_block_the_plan` → blocker `supervisor.endpoint_conflict`, `instance.name_taken`, `supervisor.endpoint_in_use`, dan `runtime.selector_unresolved`, `applicable` false, tanpa digest → AC-7
- [ ] `cargo test -p stackly-core --test environment plan::a_name_registered_at_another_location_is_name_taken` dan `plan::a_registered_source_whose_file_declares_another_name_stops_with_manifest_renamed` → `registration` `name_taken` dengan blocker `env.name_taken`; sumber `registered` dengan nama lain berhenti dengan `env.manifest_renamed` dan `items` kosong → AC-7, AC-11
- [ ] `cargo test -p stackly-core --test environment plan::an_instance_ref_is_kept_or_started_and_never_created` → `env.instance_ref_not_found`, `keep` saat berjalan, `start` saat berhenti, `env.instance_ref_mismatch` untuk provider lain, tidak pernah `create` → AC-8
- [ ] `cargo test -p stackly-core --test environment plan::a_member_follows_the_lifecycle_table_reports_differences_and_detached` → setiap baris *Lifecycle ke aksi* (termasuk `uninitialized` dan `stopped` tanpa data sebagai `env.instance_blocked` `not_initialized`, dan blocker instance pada `start`), `differences` `runtime` dan `endpoint.http.port` yang tidak pernah menjadi blocker, dan member `old` di `detached` → AC-9
- [ ] `cargo test -p stackly-core --test environment plan::the_digest_ignores_member_lifecycle_and_ports_but_follows_what_up_creates` → digest sama saat member pindah `stopped` ke `running` dan saat port efektifnya berubah; berbeda saat endpoint item `create` berubah dan saat member diganti instance lain → AC-5
- [ ] `cargo test -p stackly-core --test environment plan::bound_references_show_present_missing_unregistered_and_generated` → status `present`, `missing`, `unregistered`, `generated`; `needsSecrets` 2, `applicable` tetap true, nilai dari store tidak muncul di plan → AC-10
- [ ] `cargo test -p stackly-core --test environment status` → status `partially_ready` dan `down`, `readyCount` dan `serviceCount`, alasan `stopped` dan `not_created`, member yang tidak dideklarasikan; `manifestState` `missing`, `unreadable`, `in_sync`, `name_changed`, `changed`, `invalid` dengan jumlah problem → AC-14
- [ ] `cargo test -p stackly-core --test create_evaluate --test create_operation` → golden preflight dan create tidak berubah sesudah `planned_runtime` dan `settings_with_references` → AC-5 (Konsekuensi)
- [ ] `cargo test -p stackly-protocol --no-fail-fast` → test katalog (area `env` terdaftar dengan pemiliknya, `items` dengan `omitted`, tabel `cli`), `CONTRACT_LIMITS` (`envFindingsMax`, `envListMax`, `envServicesMax`), tabel method (enam `env.*`, semuanya `recovery refused`), `ipc_fixtures` (fixture roundtrip setiap method baru, dua `reject`, tiga error `env.*`, `decode` untuk status yang tidak dikenal, dan `dto-instance-summary-environment-member.json` dengan `ownership` `dedicated` dan `environments`), dan `env_fixtures` (fixture `protocols/fixtures/env/` roundtrip, enum terbuka membaca nilai baru sebagai `unrecognized`, setiap kode `env` punya hint statis) → AC-15 (bagian protokol), AC-24
- [ ] `swift test --package-path clients/swift` → `EnvironmentContractTests` membaca fixture `env/` dan `frame-response-env-list-unknown-values.json` (`.unrecognized`), `IpcFixtureTests` menghitung 73 method, `ContractTests` protokol 4.2 → AC-24
- [ ] `cargo xtask codegen --check` → semua file hasil generate mutakhir, termasuk `schemas/environment/environment.v1alpha1.schema.json` dan entri 4.2 di `protocols/contract-versions.json` → AC-24
- [ ] `cargo xtask check-deps` dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → lulus → AC-24

### Acceptance-criteria coverage

- AC-1, AC-2, AC-3 · core `environment::manifest` dan `validate` dengan fixture per kode; tampilannya lewat `env.validate` menunggu langkah 5
- AC-5, AC-7, AC-8, AC-9, AC-10 · core `environment::plan`; balasan `env.plan` lewat socket menunggu langkah 5
- AC-11, AC-12, AC-13 · bagian data: kolom, index unik, `WriteOp::DeleteEnvironment`, `GuardView::environments`; operasi `import_environment` dan `remove_environment` (guard, replay, audit) menunggu langkah 5
- AC-14 · core `environment::status`; `env.list` dan `env.get` menunggu langkah 5
- AC-15 · bagian protokol: `InstanceOwnership` `dedicated` dan `shared`, `InstanceSummary.environments`; cache keanggotaan dan pengisian di setiap pembangun summary menunggu langkah 5 (sampai itu daemon mengirim `manual` dan daftar kosong)
- AC-24 · modul `env`, enam method, area `env`, fixture `protocols/fixtures/env/`, protokol 4.2, schema editor; skenario fixture `environment-partially-ready` spec 0068 milik langkah 7
- AC-4, AC-6 (bagian socket) · menunggu langkah 5
- AC-16 sampai AC-23, AC-25 (CLI dan GUI) · milik Milestone 2 dan 3

## Milestone 2: stackly env

Langkah 6 Build plan. Golden dan test di bawah memakai scripted daemon yang menjawab dengan fixture `protocols/fixtures/env/`; binary test terhadap `stacklyd` sungguhan dengan provider `fake` menunggu handler `env.*` langkah 5 masuk `main` (lihat coverage).

### UI / manual

Tidak ada langkah manual. Semua perintah di bawah jalan tanpa terminal interaktif, dengan root test sementara.

### Commands

- [ ] `cargo test -p stackly-cli --test env validate` → manifest valid mencetak `ok erp-development 2 services sha256:<digest>` dan exit 0; manifest tidak valid menulis satu baris `<file>: <location>: <code>: <message>` per finding lalu `2 error(s), 0 warning(s)` ke stderr, stdout kosong, exit 2 dengan `cli.env_invalid` (`errors` 2, `warnings` 0), dan `result` `--json` tetap `EnvValidation`; `--file` default `./stackly.yaml`, path relatif dikirim absolut dan dinormalkan (`./a/../b/./stackly.yaml` menjadi `<cwd>/b/stackly.yaml`) → AC-16
- [ ] `cargo test -p stackly-cli --test env plan` → `plan-applicable.json` mencetak tabel `ACTION SERVICE INSTANCE PROVIDER VERSION ENDPOINTS` dengan baris `install`, `create`, `start` untuk `postgres` dan `keep` untuk `cache`, lalu `Plan: 1 to install, 1 to create, 1 to start, 1 to keep.`; `secrets:`, `differs:`, dan `not in manifest:` ke stderr; exit 0. `plan-blocked.json` exit 3 dengan `cli.env_plan_blocked` (`blockers` 2) dan satu baris `blocked:` per blocker beserta hint nya; manifest tidak valid exit 2 dengan `cli.env_invalid`; `result` `--json` selalu `EnvPlan`, stderr kosong dengan `--json`. Nama atau UUID utuh (huruf besar juga) dikirim sebagai sumber `registered`; nama yang tidak dikenal exit 10 dengan `cli.environment_not_found` sebelum `env.plan` dikirim; `--file` bersama nama ditolak sebagai usage (exit 2) → AC-16, AC-17
- [ ] `cargo test -p stackly-cli --test env manage` → import mengirim `env.plan` lalu `env.import` dengan `expectedManifestSha256` dari plan dan idempotency key baru setiap kali, mencetak `Imported environment erp-development.` dengan baris Name, ID, File, Status, Services, dan `Already imported.` saat revision tidak berubah; `--relink` mengirim `relinkEnvironmentId` hanya saat `registration` `name_taken`; manifest tidak valid tidak pernah dikirim ke `env.import` (exit 2). `env list` mencetak `NAME STATUS READY FILE`, `No environments.` saat kosong, dan `{ items }` dengan `--json`. `env remove` tanpa `--yes` dan tanpa terminal gagal `cli.confirmation_required` (`flag` `--yes`) sebelum `env.remove`; dengan `--yes` mengirim id dan revision dari `env.list` dan mencetak `Removed environment erp-development. Instances and data are kept.`; nama yang tidak dikenal exit 10 → AC-17
- [ ] `cargo test -p stackly-cli --test instance list` → kolom `ENVIRONMENT` sesudah `OWNERSHIP`: nama environment dipisah koma dalam urutan daemon (`erp-development, billing`), `-` untuk instance tanpa environment; golden `instance_list.txt`, `instance_list_failed.txt`, dan `instance_list.json` (field `environments`) diperbarui → AC-15 (bagian CLI)
- [ ] `cargo test -p stackly-cli --bin stackly` → unit test: `path::absolute` dan `home_masked`, `EnvironmentTarget` (UUID utuh dalam huruf apa pun adalah id), hitungan ringkasan plan menghitung setiap `start_instance` termasuk milik item `create`, baris finding `warning: `, dan satu constructor per kode `cli.*` (tiga kode `cli.env*` ikut) → AC-16, AC-17
- [ ] `cargo test -p stackly-cli --no-fail-fast`, `cargo clippy -p stackly-cli --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps` → lulus

### Acceptance-criteria coverage

- AC-16 · `apps/cli/src/commands/env/validate.rs` dan `plan.rs`, golden `env_validate*`, `env_plan*`; kesetaraan finding dan blocker dengan GUI terjamin karena CLI menulis apa yang `env.validate` dan `env.plan` kirim tanpa menghitung ulang
- AC-17 · `import.rs`, `list.rs`, `remove.rs`, `target.rs`; konfirmasi memakai aturan `ConfirmArgs` spec 0008 lewat `confirm_action`
- AC-15 · kolom `ENVIRONMENT` di `apps/cli/src/commands/instance/list.rs`; isi `environments` dari daemon menunggu cache keanggotaan langkah 5
- Binary test `stackly env` terhadap `stacklyd` dengan provider `fake` (validate, plan, import, list, remove, dan kolom `ENVIRONMENT` pada instance yang dibuat lewat environment) · menunggu handler langkah 5 di `main`
