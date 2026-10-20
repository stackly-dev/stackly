# Verify: Walking skeleton · spec 0014 · updated 2026-10-20

_Langkah diturunkan dari acceptance criteria spec 0014. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27._

## Milestone 1: Development root seed

Langkah build 1 dan 2. Seed berjalan sendiri; belum ada daemon asli yang menjalankan instance dari root nya (itu Milestone 3).

Keputusan kecil yang diambil saat membangun, karena spec diam atau menyebut dua hal:

- `install_fake_runtime` sudah publik dari langkah 13 spec 0010, jadi `seed_fake` memakainya dan tidak membuat fungsi baru. Pemindahan helper commit langkah 14 (`seed_root` di `apps/daemon/tests/supervisor/support.rs`) ke `seed_fake` belum dilakukan, karena folder itu di luar file yang boleh diubah sesi ini; hasilnya ada dua helper yang setara sampai sesi lain memindahkannya.
- Registry hanya mencari provider revision lewat kunci lengkap (nama, revision, digest), jadi `Conflict` untuk "nama dan revision sama, digest berbeda" dideteksi dari folder pinned: ada folder digest lain di bawah folder revision yang sama. Runtime dibandingkan lewat `runtimes()` (software `fake`, versi sama) dengan SHA-256 binary.
- Penolakan root (`validate_root_override`, lock yang dipegang `stacklyd`, registry recovery) terjadi sebelum `seed_fake`, dan `Leftover` serta `Conflict` terjadi sebelum tulisan apa pun; `prepare_layout` dan pembukaan registry tetap boleh membuat folder layout dan registry kosong di root yang lolos pemeriksaan.
- `--json` mencetak `root` sebagai path canonical dari `--root`.
- `createCommand` memakai `demo` dan tidak memuat `--secret`. Untuk itu `instance.create` di daemon memakai `generate` bagi setiap secret field paket yang tidak ada di `secrets[]` (spec 0011 AC-6, dikoreksi di `apps/daemon/src/ipc/instance/create.rs`; dicakup `apps/daemon/tests/instance_create_secrets.rs`), dan test AC-1 menjalankan perintah yang dicetak dengan binary `stackly` apa adanya.

### UI / manual

- [ ] `cargo build -p stackly-fake-server -p stacklyd --example skeleton_seed`, lalu `R="$(mktemp -d /tmp/stackly-seed.XXXXXX)"` dan `target/debug/examples/skeleton_seed --root "$R"` → empat baris `root`, `runtime`, `provider`, `next`; `next` adalah satu perintah `STACKLY_ROOT=<R> stackly instance create demo --runtime <uuid> --provider testing/fake --revision 0.1.0 --wait` → AC-1
- [ ] Ulangi perintah yang sama → output identik (runtime ID sama) dan `find "$R/data/runtimes" "$R/data/providers"` tidak berubah → AC-1
- [ ] `target/debug/examples/skeleton_seed` tanpa argumen → pesan clap dan exit 2; `--root relatif` dan `--root "$R/tidak-ada"` → satu baris `error: ...` dan exit 1 → AC-2

### Commands

- [ ] `cargo test -p stackly-core --test fixture_seed` → root kosong (satu commit, tiga record), ulang tanpa tulisan, runtime saja, provider saja, folder tanpa baris (`Leftover` runtime dan provider), kedua `Conflict` → AC-1, AC-2
- [ ] `cargo test -p stacklyd --test instance_create_secrets` → create tanpa `secrets[]` membuat reference `local:instance/<id>/<purpose>` berstatus `present` dengan asal `generated` (32 karakter, tidak ada di log daemon), create dengan `secrets[]` eksplisit memakai nilai yang dikirim → AC-1
- [ ] `cargo test -p stacklyd --test skeleton_seed` → seed lewat binary example (JSON, teks, idempoten), perintah `createCommand` yang dicetak dijalankan dengan `stackly` dan `succeeded`, tanpa `--root` exit 2, root relatif atau tidak ada, daemon yang hidup, binary fake yang tidak ada (pesan `cargo build -p stackly-fake-server`), folder tanpa baris registry, dan binary lain untuk versi yang sama; setelah setiap penolakan folder runtime dan paket serta jumlah baris sama → AC-1, AC-2
- [ ] `cargo tree -p stacklyd -p stackly-cli --edges features,no-dev -i stackly-core` → graph hanya memuat fitur `default`, tanpa `fixture` → AC-3
- [ ] `cargo xtask check-deps` → 13 crate lulus dengan example dan dependency dev `fixture` di tempatnya → AC-3
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core`, dan `cargo xtask evidence check` → lulus → AC-1 sampai AC-3

### Acceptance-criteria coverage

- AC-1 · `fixture_seed` (record dan idempotensi), `skeleton_seed` (output, ulang, daemon asli menerima create)
- AC-2 · `fixture_seed` (`Leftover`, `Conflict`), `skeleton_seed` (parser, root, lock, binary, leftover, conflict)
- AC-3 · `cargo tree` dan `check-deps`; penolakan setiap crate fixture yang melanggar sudah diuji langkah 15 spec 0010 (`b08d4f5`)
- AC-4 · belum, milik Milestone 3
- AC-5 sampai AC-8 · milik Milestone 2
- AC-9 sampai AC-19 · belum, milik Milestone 3, 4, dan 5

## Milestone 2: CLI instance commands

Langkah build 3 dan 4. Semua langkah di bagian ini berjalan terhadap daemon skrip (`apps/cli/tests/common/scripted.rs`); `stacklyd` asli menunggu Milestone 3.

Keputusan kecil yang diambil saat membangun, karena spec diam atau menyebut dua hal:

- `instance.get` setelah `--wait` hanya dipanggil kalau operasi berhenti bergerak (state akhir atau `needs_recovery`). Wait yang berakhir `cli.wait_timeout`, `cli.interrupted`, atau `cli.wait_disconnected` membiarkan `instance` null, supaya Ctrl+C tidak memicu panggilan baru dan operasi yang masih jalan tidak diberi snapshot yang mungkin basi.
- `alreadyInState` dengan `instance.get` yang gagal tetap keluar 0 dengan `instance` null; teksnya `already running` (start) atau `already stopped` (stop) dari verb, bukan dari lifecycle.
- Isi baris `Blocker` di teks `get`: `needs secret: <reference> (<reason>), ...`, `data in use` atau `identity unproven`, masing masing dengan `, pid <pid>` kalau ada. Kolom `ADDRESS` tabel endpoint: `<host>:<port>` (IPv6 dalam kurung siku, `auto` kalau port belum ada), path socket untuk endpoint `unix`, `socket (pending)` kalau path nya null, sama dengan kolom `ENDPOINTS` di `list` spec 0022. Blok `Failure output:` hanya muncul kalau `lastFailure.excerpt` tidak kosong. Nilai `Restarts` memakai `ceil(windowSeconds / 60)` menit.
- Jawaban `alreadyInState: true` yang tetap membawa operasi (`no_op` yang sukses, bentuk nyata dari `stacklyd`) diperlakukan sama dengan `alreadyInState` tanpa operasi: tidak ditunggu (juga dengan `--wait`), satu `instance.get`, teks `already <lifecycle>`. `operation` di `--json` tetap diisi operasi itu, tidak dinullkan seperti AC-6 untuk jawaban tanpa operasi, supaya ID operasinya tidak hilang. Dicakup test `already_in_state_with_a_succeeded_no_op_operation_waits_for_nothing`.
- `--port` ganda tetap dikirim berurutan (tabel *Pemetaan flag create*); aturan "kemunculan terakhir menang" berlaku untuk `--set` dan `--set-json`.
- Replay (`replayed: true`) hanya terlihat sebagai hint di stderr: `the idempotency key matched an earlier request, so this is its operation`. `result` tetap tiga key persis seperti AC-5.
- Harness golden menormalkan `checked <umur>` selain `(<umur>)` di `tests/common/golden.rs`, karena baris `Readiness` dan `Health` memuat umur.

### UI / manual

- [ ] Jalankan `stackly instance create --help`, `start --help`, `stop --help`, `get --help` → semua flag AC-5 muncul (`--runtime`, `--provider`, `--revision`, `--display-name`, `--port`, `--set`, `--set-json`, `--wait`, `--timeout`, `--idempotency-key`); `--secret` belum ada (Milestone 5) → AC-5, AC-8
- [ ] Baca `apps/cli/tests/golden/instance_get.txt` → urutan label `Name`, `Display name`, `ID`, `Provider`, `Manifest`, `Software`, `Runtime`, `Version`, `Ownership`, `Initialized`, `Lifecycle`, `Desired`, `Readiness`, `Health`, `Process`, `Restarts`, `Last failure`, `Blocker`, `Logs`, `Revision`, lalu tabel `ENDPOINT`, `TRANSPORT`, `ADDRESS`, `PORT MODE`, `BOUND` persis seperti *CLI* spec 0022 → AC-7
- [ ] Baca `instance_get_failed.txt` → `Leftover processes` muncul, `Last failure` memuat kode dan `exit code`, dan `Failure output:` memuat baris `excerpt` berindentasi dua spasi → AC-7

### Commands

- [ ] `cargo test -p stackly-cli --bin stackly args::` → parser `IdempotencyKeyArg` (1 sampai 128 byte ASCII tercetak tanpa spasi), `InstanceIdArg` (36 karakter, huruf besar atau kecil), `PortArg` (0 sampai 65535 atau `auto`), `SettingArg`, dan `SettingJsonArg` lulus → AC-5, AC-6, AC-8
- [ ] `cargo test -p stackly-cli --bin stackly wait::` → `Submission::finished` dan `wait_hint` lulus → AC-8
- [ ] `cargo test -p stackly-cli --test instance create::` → satu `instance.create` tanpa panggilan lain sebelumnya dengan params sesuai tabel flag (port berurutan dengan duplikat, `--set` terakhir menang, `--set-json` menimpa `--set`), port 0 tetap dikirim, setiap kasus `cli.usage` tanpa koneksi (`connections() == 0`), key buatan CLI berupa UUID v7 huruf kecil yang sama di request dan di `result.idempotencyKey`, replay, `--wait` sukses dengan satu `instance.get`, `needs_recovery` dengan instance `failed` di `result` dan exit 7, dan `instance.get` yang gagal setelah wait → `instance` null dengan error operasi → AC-5, AC-6, AC-8
- [ ] `cargo test -p stackly-cli --test instance action::` → `start` dan `stop` mengirim `instanceId` huruf kecil (`overrideLeases: false` untuk stop), `alreadyInState` keluar 0 dengan `operation` null dan satu `instance.get` (juga saat `--wait` dan saat `instance.get` gagal), teks tanpa `--wait` dengan hint `wait with: stackly operation inspect <id> --wait`, `--wait` sukses satu baris `<name> <lifecycle>, readiness <state>`, operasi `failed` yang tetap membawa instance, `cli.wait_timeout` tanpa `instance.get`, error sinkron (`resource.not_found` exit 10, `conflict` exit 3), `ipc.call_timeout` dengan `details.idempotencyKey` dan hint `retry safely with: --idempotency-key <key>` (test ini menunggu 10 detik), dan UUID atau key yang salah exit 2 tanpa koneksi → AC-6, AC-8
- [ ] `cargo test -p stackly-cli --test instance get::` → golden `instance_get.txt` dan `.json`, `instance_get_failed.txt`, `instance_get_adopted.txt` (`, adopted, not adoptable` dan `, next attempt`), `instance_get_locked.txt` (blocker `needsSecret`), teks daemon yang di escape (tanpa ESC mentah di stdout), `resource.not_found` exit 10, dan id yang salah exit 2 → AC-7
- [ ] `cargo test -p stackly-cli --test instance exit_codes::` → baris matriks exit code `alreadyInState` 0, `needs_recovery` di bawah `--wait` 7, `resource.not_found` 10, `instance.get` gagal setelah menunggu (exit tetap milik operasi), dan `cli.usage` 2 → AC-6, AC-7
- [ ] `cargo test -p stackly-cli` → seluruh suite CLI hijau (tidak ada test lama yang berubah selain `tests/common/golden.rs`) → AC-5 sampai AC-8
- [ ] `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-cli --test instance` lalu `git status --short` → tidak ada file golden yang berubah (golden tidak bergantung pada waktu atau mesin) → AC-7
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, dan `cargo xtask evidence check` → lulus → AC-5 sampai AC-8

### Acceptance-criteria coverage

- AC-5 · `create::` (pemetaan flag, `cli.usage`, `--wait`) dan golden `instance_create.*`
- AC-6 · `action::` (start, stop, `alreadyInState`, aturan `instance`) dan golden `instance_start.*`, `instance_stop.*`; `exit_codes::`
- AC-7 · `get::` dan golden `instance_get*.txt`, `instance_get.json`; `Failure output:` dan blok `kv`
- AC-8 · `args::`, `wait::`, `create::` (key buatan, replay), `action::` (`ipc.call_timeout` dengan hint)
- AC-1 sampai AC-4 · milik Milestone 1 dan 3
- AC-9 sampai AC-19 · belum, milik Milestone 3, 4, dan 5 (daemon asli, restart, GUI, `--secret`)

## Milestone 3: Through the real daemon and restart

Langkah build 5 sampai 8. Semua test di bagian ini menjalankan `stacklyd`, `stackly`, dan `stackly-fake-server` asli pada `TestRoot` (`STACKLY_ROOT`), dengan keychain file root itu. Suite `skeleton` memakai `/bin/ps` dan menunggu delay init 15 detik, jadi ia lebih lambat dari test lain dan hanya jalan di macOS.

Keputusan kecil yang diambil saat membangun, karena spec diam atau menyebut dua hal:

- `ProcessTable`, `CliRunner`, dan `ReapGuard` ada di `stackly-testkit` dengan `serde_json` sebagai dependency baru testkit (untuk `CliOutput::json`), jadi `Cargo.lock` berubah satu baris. Guard `ReapGuard` menghapus runtime root hanya kalau nama foldernya `stackly-*` dan ia di luar `TestRoot`.
- Runner menambah `--json` sendiri (kecuali sudah ada) dan `--timeout 60s` untuk perintah dengan `--wait` tanpa `--timeout`. Proses CLI yang belum selesai setelah 90 detik dibunuh dan test gagal.
- `Rig` di `skeleton.rs` menjatuhkan daemon lebih dulu, lalu guard yang membunuh sisa proses di bawah root, lalu `TestRoot`. Instance dibuat dengan `--port http=auto` supaya test paralel tidak berebut port 18080.
- Folder data instance dihitung dari `layout.instances_dir()/<id>/data`, bukan dari inventarisasi `instance-manifest.json`; keduanya menunjuk folder yang sama dan jalur itu sudah diuji spec 0004.
- Daemon baru dianggap selesai merekonsiliasi saat `stackly daemon status` melaporkan subsystem `supervisor` `ok`.
- Abandon create yang gagal init meninggalkan satu `leftovers` `secret_ref` dengan `reference_in_use` (`compensation_failed`) dan menghapus pohon instance. Itu perilaku `stacklyd` sekarang (create sementara, K1); test tidak memeriksa leftover itu.
- Revision paket fake naik ke `0.2.0` (langkah 5), jadi perintah `createCommand` yang dicetak seed memuat `--revision 0.2.0` dan langkah manual Milestone 1 yang menyebut `0.1.0` sudah usang.

### UI / manual

- [ ] `cargo build -p stackly-fake-server -p stacklyd`, lalu `R="$(mktemp -d /tmp/stackly-m3.XXXXXX)"`, `target/debug/examples/skeleton_seed --root "$R"`, dan `STACKLY_ROOT="$R" target/debug/stacklyd` di terminal lain; jalankan `next` dari seed, lalu `instance start <id> --wait`, `instance get <id>`, `ps -axww -o pid=,args= | grep "$R" | grep "stackly-fake-server serve"` → satu baris; `kill -9` daemon, jalankan lagi, ulangi `get` dan `ps` → PID dan `startedAt` sama, `adopted: true`, tetap satu proses → AC-9, AC-11
- [ ] Buat instance dengan `--set-json initDelayMs=15000`, `kill -9` daemon begitu `ps` memperlihatkan `stackly-fake-server init`, jalankan lagi → operasi `needs_recovery`, satu instance `failed`, pembantu init selesai sendiri setelah 15 detik → AC-12

### Commands

- [ ] `cargo test -p stackly-fake-server` → `--delay-ms` menunda sampai setelah token dibaca dan sebelum folder data dibuat; nilai yang bukan angka exit 2 → AC-4
- [ ] `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test provider_packages` lalu `cargo test -p stackly-core --test provider_packages --test provider_rules --test supervisor_start` → golden `fake/init.json` memuat `--delay-ms=0`, revision `0.2.0`, argv init di test supervisor ikut → AC-4
- [ ] `cargo xtask provider check providers/fake` → `ok testing/fake 0.2.0 <sha256>` → AC-4
- [ ] `cargo test -p stackly-testkit` → `ProcessTable`, `CliRunner`, `ReapGuard`, `SpawnedDaemon::kill` → AC-9, AC-11, AC-12 (prasyarat)
- [ ] `cargo xtask evidence run --kind fixture -- cargo test -p stacklyd --test skeleton --locked --no-fail-fast` → lima test: happy path (create, start, get, stop; satu proses `serve` selama jalan dan nol sesudah stop; file keychain ada), init gagal lalu abandon (`supervisor.init_failed`, `needs_recovery`, `operation.abandoned`, `supervisor.not_initialized`, tidak ada proses), keychain file tanpa dialog, restart `SIGKILL` dan `SIGTERM` (PID, `startedAt`, `adopted`, satu instance, satu proses, isi data sama, start berikutnya `alreadyInState`), create terputus (replay key yang sama, `instance.name_taken`, `dataInUse` bila pembantu terlihat hidup sebelum dan sesudah `get`, abandon, pembantu selesai sendiri) → AC-9 sampai AC-13
- [ ] `ps -axww -o pid=,args= | grep stackly-test-` setelah suite → tidak ada proses `stackly-fake-server` dari run ini → AC-9, AC-11, AC-12
- [ ] `cargo xtask evidence check`, `cargo xtask check-deps`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check` → lulus → AC-4, AC-9 sampai AC-13

### Acceptance-criteria coverage

- AC-4 · test `dummy_server` (`init_delay_*`), golden `fake/init.json`, `provider_packages`, `provider_rules`, `supervisor_start::init`, `cargo xtask provider check providers/fake`
- AC-9 · `skeleton::an_instance_is_created_started_read_and_stopped_through_the_real_daemon` (evidence `AC-029`)
- AC-10 · `skeleton::a_failed_init_needs_recovery_and_abandoning_it_leaves_no_instance_to_start`
- AC-11 · `skeleton::a_running_instance_survives_sigkill_and_sigterm_of_the_daemon_with_the_same_process` (evidence `AC-013`)
- AC-12 · `skeleton::a_create_cut_off_by_sigkill_ends_needing_recovery_with_one_instance_and_no_stray_process` (evidence `AC-013`)
- AC-13 · `skeleton::secrets_go_to_the_file_keychain_of_the_root_without_any_prompt` dan setiap test lain di `skeleton.rs` (semua jalan noninteractive di keychain file)
- AC-1 sampai AC-3 · milik Milestone 1 · AC-5 sampai AC-8 · milik Milestone 2
- AC-14 sampai AC-16 · belum, milik Milestone 4 · AC-17 sampai AC-19 · belum, milik Milestone 5

## Milestone 4: GUI and AC-001

Langkah build 9. Test Swift `SkeletonEndToEndTests` memakai binary seed dan `stacklyd` asli, dengan koneksi `StacklyClient` kedua (noninteractive) yang membuat perubahan. Langkah build 10 (record manual dengan CLI yang mengubah, AC-16) menunggu Milestone 2 dan 3 di main dan belum dikerjakan.

Keputusan kecil yang diambil saat membangun, karena spec diam:

- Setelah `stacklyd` mulai, panggilan pertama bisa dijawab `daemon.starting` (retryable) selama supervisor merekonsiliasi. Rig test mengirim ulang dengan key yang sama sampai 20 detik; model GUI tidak diubah.
- `SkeletonRig` memakai `LiveDaemon` yang sudah ada (root dibuat di sana, lalu seed dijalankan pada root itu sebelum `stacklyd` mulai), jadi helper daemon baru tidak dibuat. `ProcessRunner` menunggu exit lewat handler terminasi dan membunuh proses yang melewati batas waktu.
- Teardown selalu menghentikan setiap instance yang dibuat rig, membunuh proses yang tersisa, lalu menghentikan daemon, juga saat test gagal.
- Test tidak mengunci revision paket atau delay init; keduanya dibaca dari output seed.

### UI / manual

- [ ] Buka `apps/macos/Packages/StacklyUI/Tests/StacklyUITests/Support/ProcessRunner.swift` → tidak ada `waitUntilExit()`; exit datang lewat `terminationHandler`, dan watchdog mengirim SIGKILL setelah batas waktu → AC-14, AC-15
- [ ] Setelah `swift test` selesai, jalankan `ps -axww -o pid=,args= | grep stk-ui-` → tidak ada `stackly-fake-server` atau `stacklyd` dengan root `/private/tmp/stk-ui-*` yang tersisa → AC-14, AC-15

### Commands

- [ ] `cargo build --locked -p stacklyd -p stackly-fake-server && cargo build --locked -p stacklyd --example skeleton_seed` → binary `target/debug/stacklyd`, `target/debug/stackly-fake-server`, dan `target/debug/examples/skeleton_seed` ada (atau set `STACKLY_DAEMON_BIN` dan `STACKLY_SEED_BIN`) → AC-14, AC-15
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter SkeletonEndToEndTests` → 2 test lulus. `theInstanceStoreFollowsChangesAnotherClientMakes`: `demo` muncul di `InstanceStore` setelah create lewat koneksi kedua, melewati `running` dengan readiness `not_ready` sebelum `ready` (`readyDelayMs` 1500), berakhir `stopped` tanpa proses, dan himpunan ID di `operation.list` sama persis dengan operasi koneksi kedua → AC-14. `quittingTheGUIKeepsThreeInstancesRunningForTheNextModel`: `alpha`, `beta`, `gamma` ready, `willTerminate()`, lalu lewat koneksi kedua ketiganya tetap `running` dengan PID yang sama, `GET /health` menjawab 200, `operation.list` tidak berubah, dan model baru dari `ShellBootstrap.live` memegang ketiganya dengan PID yang sama → AC-15
- [ ] Hapus `target/debug/examples/skeleton_seed` sementara lalu jalankan filter yang sama → kedua test berhenti sebagai `BLOCKED` dengan petunjuk perintah build, bukan gagal; bangun lagi sesudahnya → AC-14, AC-15
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --skip "NativeTests\."` → seluruh suite `StacklyUI` hijau, termasuk aturan sumber yang tidak memindai `Tests/` → AC-14, AC-15
- [ ] `cargo xtask evidence run --kind fixture -- sh -c 'cargo build --locked -p stacklyd -p stackly-fake-server && cargo build --locked -p stacklyd --example skeleton_seed && swift test --package-path apps/macos/Packages/StacklyUI --skip "NativeTests\."'` lalu `cargo xtask evidence check` → record `fixture` untuk kedua test dengan `covers` AC-001 dan AC-029 (AC-14) serta AC-001 (AC-15) lulus → AC-14, AC-15
- [ ] `xcrun swift-format lint --strict` pada ketiga file baru, lalu `cargo xtask build-app` → lulus → AC-14, AC-15

### Acceptance-criteria coverage

- AC-14 · `theInstanceStoreFollowsChangesAnotherClientMakes`
- AC-15 · `quittingTheGUIKeepsThreeInstancesRunningForTheNextModel`
- AC-16 · belum, milik langkah build 10 (record manual, menunggu Milestone 2 dan 3 di main)
- AC-1 sampai AC-13 · milik Milestone 1 sampai 3
- AC-17 sampai AC-19 · belum, milik Milestone 5
