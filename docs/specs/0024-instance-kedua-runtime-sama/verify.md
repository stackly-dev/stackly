# Verify: Instance kedua dari runtime yang sama · spec 0024 · updated 2026-10-22

_Langkah diturunkan dari acceptance criteria spec 0024. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah._

## Milestone 1: Duplicate draft in core and the fake sentinel

Langkah Build plan 1 dan 2. Tidak ada UI di milestone ini, jadi bagian manual hanya memeriksa fake server dengan tangan.

### UI / manual

- [x] Jalankan `cargo build -p stackly-fake-server`, buat folder data dan config uji, jalankan `target/debug/stackly-fake-server serve --config <config.json> --data <folder> --port <port>`, lalu `curl -i -X PUT --data 'alpha' http://127.0.0.1:<port>/sentinel` → 204 tanpa body, `<folder>/SENTINEL` berisi `alpha` → AC-8 (alat bukti)
- [x] `curl -i http://127.0.0.1:<port>/sentinel` → 200 dengan isi `alpha`; hentikan server, jalankan lagi di folder yang sama, `GET /sentinel` masih `alpha`; di folder data lain atau sebelum PUT pertama → 404 → AC-7, AC-8
- [x] `curl -i -X PUT --data '' http://127.0.0.1:<port>/sentinel` dan PUT body 257 byte → 400, isi sentinel tidak berubah → AC-8

### Commands

- [x] `cargo test -p stackly-fake-server` → 18 unit test dan 36 test integrasi `dummy_server` lulus, termasuk `sentinel_is_written_and_read_back_from_the_data_folder`, `sentinel_accepts_one_to_two_hundred_fifty_six_bytes` (1 dan 256 byte diterima; kosong, 257 byte, dan bukan UTF 8 ditolak 400 tanpa mengubah isi lama), `sentinel_routes_answer_only_their_methods`, `two_servers_keep_their_own_sentinels`, dan `sentinel_survives_a_restart_on_the_same_data_folder`; tidak ada file sementara tersisa di folder data → AC-7, AC-8
- [x] `cargo test -p stackly-core --test instance_duplicate` → 22 test lulus. `draft::*` membuktikan tabel `build_duplicate` dengan paket fake, PostgreSQL, dan NATS: settings disaring (secret, managed `serverName`, key asing keluar; `immutableAfterInit` dan nilai default tetap), tanpa config aktif settings `{}`, setiap endpoint IP `auto` (sumber dibuat dengan port `Fixed` 5432, endpoint `unix` PostgreSQL tidak ikut), setiap secret `generate` dengan `secret_sources` dari pemakaian, runtime ID dan provider revision sumber, `PackageSource::Pinned`, `displayName` kosong dan `start` false, nama form dan draft sama, instance retained tidak menahan nama, semua kandidat terpakai memberi nama kosong → AC-2, AC-3, AC-4, AC-5
- [x] Test yang sama, `gather::*` membuktikan `gather_duplicate` di `FakePlatform` dan `FakeRegistry`: instance tidak ada, retained (dicek sebelum paket dibaca), paket hilang, tidak valid, dan digest beda dengan tiga teks `reason` `load_package`, catalog dengan label revision sama dan digest lain, catalog yang sama atau `NotConfigured` yang tetap berhasil, tanpa `multi_instance`, runtime bukan `installed`, `values` yang bukan JSON object menjadi `Corrupt` `config_revisions.values`, urutan pemeriksaan, sumber tanpa folder instance, log, dan run yang berhasil dengan generation registry, mutasi file system, panggilan Keychain, dan spawn proses tidak berubah → AC-1, AC-7 (bagian core)
- [x] `cargo test -p stackly-core instance::duplicate` → 7 unit test `duplicate_name` lulus: `pg17-dev`, `pg17-dev-copy`, `pg17-dev-copy-7`, sumber 63 karakter dipotong menjadi 58 lalu `-copy`, tanda hubung di ujung potongan dibuang, akhiran yang bukan angka 2 sampai 99 tetap bagian dasar, dan semua kandidat terpakai → AC-3
- [x] `cargo test -p stackly-core --test create_evaluate` → test form spec 0020 lulus tanpa perubahan setelah `build_form` memanggil `build_form_from_package` → AC-1 (bentuk form dipakai ulang)
- [x] `cargo clippy -p stackly-core -p stackly-fake-server --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-core` → semuanya bersih

### Acceptance-criteria coverage

- AC-1 (urutan pemeriksaan dan tanpa jejak) · `gather::*`. Dua bagian menunggu milestone 2: kode error di wire dan batas 300 ms pada persentil 95 dengan 10 instance (daemon). Cabang `RuntimeNotFound` tidak bisa dicapai lewat `FakeRegistry` karena registry menolak instance yang runtime-nya tidak ada (`MissingReference`); cabang itu dijaga oleh tipe dan baru bisa diuji bila registry rusak, yang tidak ada alatnya di testkit.
- AC-2, AC-3, AC-4 (draft) · `draft::*` dan unit test `duplicate_name`. Port efektif setelah start dan penolakan `supervisor.endpoint_conflict` untuk `Fixed` yang sama menunggu milestone 2.
- AC-5 (kebijakan secret) · draft selalu `generate` dan `secret_sources` terisi di sini; reference baru, `copyFrom`, dan nilai berbeda menunggu milestone 2 (daemon).
- AC-6 menunggu milestone 2; AC-7 · rute `/sentinel` (alat) dan `gather_duplicate` tanpa sentuhan folder sumber di sini, sisanya (mode 000, daftar pohon, 404 di duplicate) menunggu milestone 2; AC-8 · rute `/sentinel` (alat) di sini, skenario tiga instance menunggu milestone 2.
- AC-9, AC-10, AC-11, AC-12, AC-13, AC-14 menunggu milestone 2 sampai 4.

## Milestone 2: Contract, daemon, CLI, and fixture proof

Langkah Build plan 3, 4, dan 5. Tidak ada UI di milestone ini; bagian manual memakai daemon dan CLI nyata dengan runtime fake.

### UI / manual

- [x] `cargo build -p stackly-fake-server && cargo run -q -p stacklyd --example skeleton_seed -- --root <R>`, jalankan `STACKLY_ROOT=<R> cargo run -q -p stacklyd`, lalu `stackly instance create fake-a --runtime <runtimeId> --provider testing/fake --revision 0.3.0 --wait` (ID runtime dicetak seed) → exit 0, instance `fake-a` ada → AC-6
- [x] `stackly instance duplicate fake-a fake-b --dry-run` → stderr `Duplicating settings of fake-a; no data is copied.`, baris preflight seperti `create --dry-run`, `ready to create`, dan `command: stackly instance create fake-b --runtime … --provider testing/fake --port http=auto --secret authToken=generate`; tidak ada instance baru di `stackly instance list` → AC-4, AC-11
- [x] `stackly instance duplicate fake-a fake-b --wait --json` → satu baris envelope dengan `result.source` `{ id, name: "fake-a" }`, `result.instance.name` `fake-b`, `runtimeId` sama dengan `fake-a` → AC-6, AC-11
- [x] `stackly instance start <id fake-a> --wait`, `stackly instance start <id fake-b> --wait`, ambil port dari `stackly instance inspect <nama> --json`; `curl -X PUT --data alpha http://127.0.0.1:<port a>/sentinel` → 204, `curl -i http://127.0.0.1:<port b>/sentinel` → 404, PUT `bravo` ke port b lalu GET keduanya → `alpha` dan `bravo`; port keduanya berbeda → AC-4, AC-7, AC-8
- [x] `stackly instance duplicate fake-a fake-c --runtime x` → exit 2 `cli.usage`, tidak menghubungi daemon (juga `--provider` dan `--revision`); `stackly instance duplicate gamma` → exit 10 `cli.instance_not_found` → AC-11
- [x] `stackly instance duplicate fake-a fake-c --secret authToken=copy --wait`, lalu bandingkan `instances/<id>/data/token.sha256` kedua instance → sama; tanpa `--secret` → beda → AC-5

### Commands

- [x] `cargo test -p stackly-protocol` → semua lulus, termasuk `duplicate_fixtures`: `every_duplicate_form_fixture_roundtrips_in_the_wire_form`, `a_duplicate_form_pins_the_package_asks_auto_ports_and_generates_every_secret`, `the_fixtures_name_the_duplicate_after_the_names_already_taken`, `the_request_frame_decodes_to_the_new_method`, `the_response_frame_carries_the_postgresql_fixture_and_foreign_values_stay_decodable`, dan test tabel `message.rs` (method baru `recovery refused`) → AC-10
- [x] `cargo test -p stacklyd --test duplicate_form_fixtures` → `the_duplicate_form_of_each_real_package_is_its_fixture` lulus: form dan draft yang dibangun daemon dari `providers/fake` dan `providers/postgresql` sama dengan `protocols/fixtures/instance/duplicate-form-{fake,postgresql}.json` (tulis ulang dengan `STACKLY_UPDATE_GOLDEN=1`) → AC-1, AC-10
- [x] `cargo xtask codegen --check` → bersih; `ProtocolVersion::CURRENT` 1.13 dan digest di `protocols/contract-versions.json`; `cargo xtask fuzz seed` tidak menambah file lagi → AC-10
- [x] `swift test --package-path clients/swift` → 196 test lulus, termasuk `DuplicateContractTests` (empat test: form fake dan PostgreSQL, nama, frame, nilai asing `.unrecognized`) dan hitungan 33 method di `IpcFixtureTests` → AC-10
- [x] `cargo test -p stacklyd --test supervisor same_runtime` → 8 test lulus (butuh `cargo build -p stackly-fake-server -p stacklyd`):
  - `the_duplicate_form_copies_the_settings_and_asks_for_new_identity_inputs` (form sumber: pinned, runtime dan revision sumber, `fake-a-copy`, port `auto`, secret `generate` dengan `sourceReference`, settings tanpa token, tanpa operasi dan folder baru, tetap berhasil saat folder instance, log, dan run sumber dihapus) → AC-1, AC-2, AC-3, AC-4, AC-5
  - `a_duplicate_form_is_refused_with_the_code_of_the_first_failing_check` (`resource.not_found` untuk UUID tak dikenal dan UUID nil, `supervisor.instance_retained` tanpa detail dan lebih dulu dari paket, `supervisor.package_unavailable` dengan `provider` dan `reason` "the pinned package cannot be read: …") dan `a_duplicate_form_of_a_source_on_a_failed_runtime_is_refused` (`supervisor.runtime_unavailable` dengan `runtimeId` dan `state` `failed`) → AC-1
  - `the_duplicate_form_answers_within_300_ms_at_the_95th_percentile_with_ten_instances` (40 panggilan, 10 instance) → AC-1
  - `three_instances_of_one_runtime_keep_their_own_sentinels_processes_and_folders` (create biasa, duplicate, dan `crashAfterMs` dari runtime yang sama; runtime ID dan provider revision sama; sembilan path per instance ada dan tidak ada yang sama atau bertumpuk; sentinel sendiri-sendiri lewat stop dan start bergantian; PID dan readiness yang lain tidak berubah selama crash loop dan stop; folder runtime sama persis) → AC-6, AC-8
  - `a_duplicate_never_reads_lists_copies_or_writes_the_tree_of_its_source` (isi folder data sumber mode 000, form, create, start, dan stop duplicate berhasil, `GET /sentinel` duplicate 404, pohon dan log sumber sama persis sesudahnya) → AC-7
  - `a_duplicate_gets_a_new_secret_unless_the_user_copies_the_one_of_the_source` (reference baru milik instance baru, `token.sha256` beda untuk draft apa adanya dan sama untuk `copyFrom`, record dan pemakaian reference sumber tidak berubah) → AC-5, AC-6
  - `a_duplicate_never_fixes_a_port_and_a_fixed_port_of_the_source_is_refused` (draft `auto` walau sumber `Fixed`; `Fixed` yang sama dengan sumber `stopped` ditolak `supervisor.endpoint_conflict` di baris preflight dan di create, dengan `otherInstance` sumber; dua instance `auto` mendapat port berbeda dan port yang sama dipakai lagi sesudah stop dan start) → AC-4
- [x] `cargo test -p stacklyd --lib --test supervisor` → seluruh test binary `supervisor` (43) dan test lib lulus; helper `put_sentinel` dan `get_sentinel` di `support.rs` → AC-8
- [x] `cargo test -p stackly-cli --test instance duplicate` → 11 test lulus:
  - `duplicate::*` (scripted daemon): flag di atas draft (`<name>`, `--port`, `--settings-file` yang hanya mengganti key nya, `--set` bertipe, `--secret superuserPassword=copy` menjadi `copyFrom` reference sumber), tanpa `<name>` memakai nama draft dan secret `generate`, resolusi nama lewat `instance.list` dan `cli.instance_not_found` (exit 10), `copy` untuk field tanpa `sourceReference` exit 2 tanpa `instance.create`, `--runtime`/`--provider`/`--revision` ditolak clap tanpa menghubungi daemon, `--dry-run` hanya `instance.preflight` (golden `instance_duplicate_dry_run.txt` dan `.json`), preflight yang diblokir exit 3, `--start --wait` dengan key SHA-256 `<key>:start` dan key `source` di JSON (golden `instance_duplicate.json`), form yang ditolak exit 3 tanpa create → AC-11
  - `duplicate_real::a_bad_duplicate_gets_the_same_error_through_the_client_and_through_the_cli` (stacklyd nyata: `Bad_Name`, nama sumber dipakai, port sama dengan sumber, integer `"abc"`, key asing, `bind` ke reference yang tidak ada; `code`, `category`, `details`, `fields` sama, satu baris envelope, exit code per kategori) → AC-11, AC-028
  - `duplicate_real::the_fake_journey_creates_duplicates_starts_both_and_reads_a_sentinel_of_each` (JOURNEY-02 dengan runtime fake: create, duplicate, start keduanya, `GET /sentinel` berbeda, 404 di duplicate sebelum PUT pertama) → AC-4, AC-7, AC-8, AC-11
- [x] `cargo test -p stackly-cli` → seluruh test CLI lulus (create dan golden lama tidak berubah setelah pembantu `latest_sources`, `read_secrets`, `secret_plans`, `start_after` dijadikan fungsi bersama) → AC-11
- [x] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo xtask check-deps`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-protocol` → semuanya bersih

### Acceptance-criteria coverage

- AC-1 (form duplicate) · daemon: bentuk balasan, kode `resource.not_found`, `supervisor.instance_retained`, `supervisor.package_unavailable`, `supervisor.runtime_unavailable`, tanpa operasi dan folder baru, tanpa folder sumber, dan p95 di bawah 300 ms dengan 10 instance. `instance.provider_ambiguous` dan `instance.capability_unsupported` dipetakan di `map::duplicate::duplicate_error` dan diuji di core (milestone 1) tetapi belum lewat wire: keduanya butuh catalog development dengan digest lain atau paket pinned tanpa `multi_instance`, yang tidak ada di harness `supervisor`. Detail `existing` pada `instance.capability_unsupported` adalah nama sumber (spec 0024), sedangkan create spec 0020 mengisinya dengan ID.
- AC-2, AC-3 · daemon (settings dan nama di balasan) di atas `draft::*` milestone 1.
- AC-4 (port) · draft `auto`, `Fixed` yang sama ditolak dengan `otherInstance`, port berbeda dan dipakai ulang di daemon; CLI `--port`.
- AC-5 (secret) · reference baru, nilai berbeda atau sama untuk `copyFrom`, reference sumber tidak berubah (daemon); `--secret <field>=copy` dan jalur `bind` (CLI). Bahwa journal create dari draft tidak memuat `bind` dijaga oleh bentuk draft (semua `generate`) dan test kontrak; pemeriksaan prefix kredensial fixture di log dan event belum ada di sini.
- AC-6 (create, identitas baru) · runtime ID, provider revision, sembilan path (`InstancePaths`: root, manifest, lock, data, config, config_history, temp, logs, run) dan perbedaan tiap instance di daemon. `socketPath` berbeda tidak diuji karena runtime fake tidak punya endpoint unix; itu dibuktikan di milestone 4 dengan PostgreSQL.
- AC-7 (data tidak disalin) · fake: mode 000, pohon dan log sumber tidak berubah, 404 di duplicate (daemon dan CLI). Bagian PostgreSQL (`stackly_sentinel`, `system_identifier`) menunggu milestone 4.
- AC-8 (dua instance dari runtime yang sama, fixture) · tiga instance, sentinel, stop dan start bergantian, crash loop, folder runtime tetap (daemon); journey CLI dua instance.
- AC-10 (kontrak) · lengkap: DTO, method `recovery refused`, fixture, test penjaga Rust, test Swift, minor 1.13, `codegen --check`, seed fuzz. Kasus fixture tampilan spec 0068: folder `Fixtures/` ada di `main`, tetapi spec 0068 tidak mendaftarkan kasus duplicate di *Kasus yang menunggu DTO*, dan `apps/macos/` bukan milik milestone ini, jadi tidak ada yang ditambah.
- AC-11 (CLI) · lengkap (lihat daftar test di atas).
- AC-9, AC-12, AC-13, AC-14 menunggu milestone 3 dan 4.

## Milestone 3: Duplicate in the GUI

Langkah Build plan 6 dan 7. Semua bagian otomatis jalan di fixture dan service tiruan; bagian manual menunggu kamu (`docs/manual.md` bagian U). Centang oleh `/check verify`, bukan oleh developer.

### UI / manual

- [ ] `docs/manual.md` bagian U bagian 1 (fake provider): ⌘2, panah ke baris, ⌘D → sheet New Instance dengan subjudul duplicate, baris Runtime read only "{nama tampil} {versi} · same as “{sumber}”" tanpa filter kategori dan picker, Name `{sumber}-copy` terpilih, port Automatic, settings sumber, secret Generate dengan Copy from yang lebih dulu menawarkan "{judul field} of “{sumber}”"; Return membuat dan kartu umpan balik berbunyi "“{nama}” created from “{sumber}”. Starting it now." → AC-12, AC-14
- [ ] Dari detail: Tab ke More, Space, panah ke Duplicate… (tepat sesudah Copy Connection String, tanpa shortcut), Return, Escape; menu Instance memuat Duplicate… ⌘D di kelompok sesudah Copy Connection String, nonaktif tanpa subjudul tanpa target dan saat daemon mati, dan ⌘D tidak berbuat apa pun selama sheet terbuka → AC-13, AC-14
- [ ] `PUT /sentinel` di `demo`, lalu `GET /sentinel` di `demo-copy` → 404 → AC-7 (lewat GUI)
- [ ] VoiceOver membaca baris Runtime "Runtime, {nama tampil} {versi}, same as {sumber}", subjudul, pilihan Copy from, dan setiap baris preflight; sheet 560 pt tanpa teks terpotong di light, dark, dan Increase Contrast; seluruh alur hanya dengan keyboard → AC-14 (bagian fake), NFR-012
- [ ] `docs/manual.md` bagian U bagian 2 (PostgreSQL 17): instance kedua lewat New Instance (port dari "Use {n}") dan lewat Duplicate…, ketiganya Ready dari satu runtime, `psql` memperlihatkan data dan `system_identifier` berbeda → AC-14 (JOURNEY-02 di GUI), AC-027; `BLOCKED` dengan alasan `artifact <id> is not in the runtime cache` bila artefak PostgreSQL 17 tidak ada di cache xtask

### Commands

- [x] `swift test --package-path clients/swift --filter CreateClientTests` → `getDuplicateForm` mengirim `instance.get_duplicate_form` dengan ID sumber, membaca `DuplicateForm` dari frame fixture, menolak balasan jenis lain, dan meneruskan error daemon → AC-12
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter CreateInstanceDuplicateTests` → 23 test model mode duplicate atas `duplicate-form-fake.json`, `duplicate-form-postgresql.json`, dan service tiruan: loading dan editing dari draft, settings dan port Automatic, `provider.revision` sumber di preflight dan create, baris Runtime dan subjudul, Copy from (hanya reference sumber selama `listSecrets` belum kembali atau gagal, daftar create di belakangnya tanpa duplikat, `copyFromReference` tetap, reference `missing` tetap di depan, label sumber menang, field tanpa `sourceReference` memakai daftar create), `formFailed` dengan Retry, balasan lama dibuang, sukses dengan umpan balik "created from", start lanjutan gagal → AC-12
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "DuplicateEntryPointTests|CommandMapTests|InstanceDetailCommandsTests|FixtureCreateDuplicateTests"` → aksi Duplicate… (`mutates` true, tanpa konfirmasi, nonaktif saat gate `.blocked`), item More urut sesudah Copy Connection String dan tanpa Duplicate… di jendela tanpa sheet, penerbit Instances hanya dengan target, baris `duplicateInstance` built di kelompok 4 dengan ⌘D, presenter membuka mode duplicate dan menjaga create yang berjalan, dan fixture `FixtureCreateDuplicate` → AC-12, AC-13
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter NewInstanceSnapshotTests` → empat snapshot mode duplicate (`new-instance-duplicate`, `-copy-from`, `-loading`, `-failed`) dalam enam varian, dan semua snapshot New Instance lama tidak berubah (`new-instance-ready` light pernah goyah sekali di beban tinggi; ulangi sendirian sekali, jangan direkam ulang) → AC-12, AC-14 (kontras dan lebar 560 pt)
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` (butuh `cargo build -p stacklyd -p stackly-fake-server` dan contoh `skeleton_seed` lebih dulu) → seluruh suite lulus
- [x] `swift test --package-path clients/swift` → seluruh suite lulus
- [x] `cargo xtask build-app` → `BUILD SUCCEEDED`
- [x] `xcrun swift-format lint --strict` untuk file Swift yang berubah, dan `cargo xtask evidence check` → bersih (dua record manual spec 0024 `NOT_RUN` menunggu bagian U)

### Acceptance-criteria coverage

- AC-12 · `getDuplicateForm`, `CreateMode`, mode duplicate `CreateInstanceModel`, sheet (baris Runtime, subjudul, Copy from), tes model, dan snapshot; form gagal dimuat dengan Retry hanya dibuktikan test model dan snapshot `new-instance-duplicate-failed` (daemon nyata tidak punya cara andal untuk memicunya dari GUI)
- AC-13 · item More, penerbit halaman Instances, baris `duplicateInstance` built dengan ⌘D, dan `DuplicateInstanceAction`; kunci ⌘D selama sheet terbuka dijaga `StacklyCommands` spec 0069 dan dicek manual
- AC-14 · bagian fake menunggu bagian U bagian 1; JOURNEY-02 dengan PostgreSQL 17 menunggu bagian U bagian 2 (record `NOT_RUN` `evidence/manual/2026-10-22-spec-0024-manual-check-duplicate-in-the-app-with.json` dan `...-journey-02-in-the-app-a-s.json`)
- AC-1 sampai AC-11 · milestone 1 dan 2; AC-7 lewat GUI ikut langkah manual ketiga; AC-9 menunggu milestone 4

## Milestone 4: PostgreSQL 17 on this Mac

_Langkah 8 (helper sentinel) dan bagian native langkah 9. Run native memakai artefak `postgresql-17.11-r2-macos-aarch64` dari cache runtime xtask, tanpa unduhan; tanpa artefak itu hasilnya `BLOCKED` dengan alasan `artifact postgresql-17.11-r2-macos-aarch64 is not in the runtime cache` dan owner `@ojiepermana`. Evidence manual JOURNEY-02 lewat GUI menunggu milestone 3._

### UI / manual

- [ ] JOURNEY-02 lewat GUI dengan PostgreSQL 17 di Mac ini: buat `pg17-dev`, Duplicate menjadi `pg17-test` dengan Copy from terisi, start keduanya, baca sentinel masing masing → menunggu milestone 3 (GUI) → AC-14

### Commands

- [x] `cargo test -p stacklyd --test postgresql -- --ignored the_sentinel_helper` → 1 test lulus (native, PostgreSQL 17): `sentinel::the_sentinel_helper_writes_reads_and_names_one_cluster`. Cluster baru membaca daftar kosong (tabel `stackly_sentinel` belum ada), satu baris miliknya sendiri sesudah `write`, nilai baru sesudah `write` kedua, dan `system_identifier` numerik yang tidak berubah → AC-9 (alat bukti)
- [x] `cargo test -p stacklyd --test postgresql -- --ignored --exact same_runtime::two_postgresql_17_instances_from_one_runtime_keep_their_sentinels` → 1 test lulus (native, `two_postgresql_17_instances` sebagai penyiapan bersama). `pg17-dev` (create biasa, port `Fixed`) dan `pg17-test` (`instance.get_duplicate_form` lalu `instance.create` atas draft dengan `copyFrom` reference password sumber) memakai runtime ID yang sama, di start bersamaan sampai `ready`, dengan port, folder data, folder run (soket `.s.PGSQL.<port>`), dan PID yang berbeda. Setiap cluster membaca hanya sentinel miliknya lewat TCP, `system_identifier` keduanya berbeda, dan sesudah tiap instance dihentikan lalu di start lagi bergantian (port tetap sama) baca silang tetap bersih. Evidence `native` dengan `covers = ["REQ-009", "AC-009", "AC-025"]`; JOURNEY-02 ada di `expected`, dan `actual` mencatat bahwa bagian mayor 18 AC-009 dibuktikan spec 0025 → AC-6 (soket dan folder berbeda), AC-9
- [x] `cargo test -p stacklyd --test postgresql -- --ignored --exact same_runtime::duplicate_postgresql_instance_starts_with_empty_data` → 1 test lulus (native). Sumber `stopped` yang menyimpan sentinel diduplikasi: duplicate tidak punya tabel `stackly_sentinel`, `system_identifier` berbeda dari sumber, daftar (path relatif, ukuran, waktu ubah) folder data sumber sama persis sesudah form, create, dan start serta stop duplicate, folder data duplicate tidak memuat nilai sentinel sumber, dan sumber yang di start lagi masih memegang sentinel dan identifier nya. Evidence `native` dengan `covers = ["REQ-009", "AC-025"]` → AC-7 (bagian PostgreSQL)
- [x] `cargo test -p stacklyd --test postgresql` → ketiga test native di atas `ignored` di run biasa, test PostgreSQL lain tidak berubah
- [x] `cargo xtask runtime spike --cleanup` setelah run native → 0 proses `postgres` sisa dari test
- [x] `cargo fmt --all --check`, `cargo clippy -p stacklyd --all-targets --locked -- -D warnings`, `cargo xtask check-deps`, `cargo xtask evidence check` → bersih

### Acceptance-criteria coverage

- AC-6 (soket dan folder berbeda) · terbukti native: dua folder run dengan soket `.s.PGSQL.<port>` sendiri dan dua folder data berbeda; milestone 2 sudah membuktikan sembilan path dengan runtime fake.
- AC-7 (data tidak disalin, bagian PostgreSQL) · terbukti native lewat `duplicate_postgresql_instance_starts_with_empty_data`. Mode 000 di dalam folder data sumber hanya diuji dengan runtime fake (milestone 2); test PostgreSQL ini tidak mengubah mode file sumber.
- AC-9 (PostgreSQL 17 nyata) · terbukti native, tanpa langkah CLI sungguhan: test memakai method yang sama dengan `stackly instance duplicate pg17-dev pg17-test --secret superuserPassword=copy` (`instance.get_duplicate_form` lalu `instance.create` atas draft). Bagian mayor 18 AC-009 milik spec 0025.
- AC-14 (GUI) · menunggu milestone 3.
