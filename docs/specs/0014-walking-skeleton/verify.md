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

- [x] `cargo build -p stackly-fake-server -p stacklyd --example skeleton_seed`, lalu `R="$(mktemp -d /tmp/stackly-seed.XXXXXX)"` dan `target/debug/examples/skeleton_seed --root "$R"` → empat baris `root`, `runtime`, `provider`, `next`; `next` adalah satu perintah `STACKLY_ROOT=<R> stackly instance create demo --runtime <uuid> --provider testing/fake --revision 0.2.0 --wait` → AC-1
- [x] Ulangi perintah yang sama → output identik (runtime ID sama) dan `find "$R/data/runtimes" "$R/data/providers"` tidak berubah → AC-1
- [x] `target/debug/examples/skeleton_seed` tanpa argumen → pesan clap dan exit 2; `--root relatif` dan `--root "$R/tidak-ada"` → satu baris `error: ...` dan exit 1 → AC-2

### Commands

- [x] `cargo test -p stackly-core --test fixture_seed` → root kosong (satu commit, tiga record), ulang tanpa tulisan, runtime saja, provider saja, folder tanpa baris (`Leftover` runtime dan provider), kedua `Conflict` → AC-1, AC-2
- [x] `cargo test -p stacklyd --test instance_create_secrets` → create tanpa `secrets[]` membuat reference `local:instance/<id>/<purpose>` berstatus `present` dengan asal `generated` (32 karakter, tidak ada di log daemon), create dengan `secrets[]` eksplisit memakai nilai yang dikirim → AC-1
- [x] `cargo test -p stacklyd --test skeleton_seed` → seed lewat binary example (JSON, teks, idempoten), perintah `createCommand` yang dicetak dijalankan dengan `stackly` dan `succeeded`, tanpa `--root` exit 2, root relatif atau tidak ada, daemon yang hidup, binary fake yang tidak ada (pesan `cargo build -p stackly-fake-server`), folder tanpa baris registry, dan binary lain untuk versi yang sama; setelah setiap penolakan folder runtime dan paket serta jumlah baris sama → AC-1, AC-2
- [x] `cargo tree -p stacklyd -p stackly-cli --edges features,no-dev -i stackly-core` → graph hanya memuat fitur `default`, tanpa `fixture` → AC-3
- [x] `cargo xtask check-deps` → 13 crate lulus dengan example dan dependency dev `fixture` di tempatnya → AC-3
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core`, dan `cargo xtask evidence check` → lulus → AC-1 sampai AC-3

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

- [x] Jalankan `stackly instance create --help`, `start --help`, `stop --help`, `get --help` → semua flag AC-5 muncul (`--runtime`, `--provider`, `--revision`, `--display-name`, `--port`, `--set`, `--set-json`, `--wait`, `--timeout`, `--idempotency-key`); `--secret` belum ada (Milestone 5) → AC-5, AC-8
- [x] Baca `apps/cli/tests/golden/instance_get.txt` → urutan label `Name`, `Display name`, `ID`, `Provider`, `Manifest`, `Software`, `Runtime`, `Version`, `Ownership`, `Initialized`, `Lifecycle`, `Desired`, `Readiness`, `Health`, `Process`, `Restarts`, `Last failure`, `Blocker`, `Logs`, `Revision`, lalu tabel `ENDPOINT`, `TRANSPORT`, `ADDRESS`, `PORT MODE`, `BOUND` persis seperti *CLI* spec 0022 → AC-7
- [x] Baca `instance_get_failed.txt` → `Leftover processes` muncul, `Last failure` memuat kode dan `exit code`, dan `Failure output:` memuat baris `excerpt` berindentasi dua spasi → AC-7

### Commands

- [x] `cargo test -p stackly-cli --bin stackly args::` → parser `IdempotencyKeyArg` (1 sampai 128 byte ASCII tercetak tanpa spasi), `InstanceIdArg` (36 karakter, huruf besar atau kecil), `PortArg` (0 sampai 65535 atau `auto`), `SettingArg`, dan `SettingJsonArg` lulus → AC-5, AC-6, AC-8
- [x] `cargo test -p stackly-cli --bin stackly wait::` → `Submission::finished` dan `wait_hint` lulus → AC-8
- [x] `cargo test -p stackly-cli --test instance create::` → satu `instance.create` tanpa panggilan lain sebelumnya dengan params sesuai tabel flag (port berurutan dengan duplikat, `--set` terakhir menang, `--set-json` menimpa `--set`), port 0 tetap dikirim, setiap kasus `cli.usage` tanpa koneksi (`connections() == 0`), key buatan CLI berupa UUID v7 huruf kecil yang sama di request dan di `result.idempotencyKey`, replay, `--wait` sukses dengan satu `instance.get`, `needs_recovery` dengan instance `failed` di `result` dan exit 7, dan `instance.get` yang gagal setelah wait → `instance` null dengan error operasi → AC-5, AC-6, AC-8
- [x] `cargo test -p stackly-cli --test instance action::` → `start` dan `stop` mengirim `instanceId` huruf kecil (`overrideLeases: false` untuk stop), `alreadyInState` tanpa operasi keluar 0 dengan `operation` null dan satu `instance.get` (juga saat `--wait` dan saat `instance.get` gagal), teks tanpa `--wait` dengan hint `wait with: stackly operation inspect <id> --wait`, `--wait` sukses satu baris `<name> <lifecycle>, readiness <state>`, operasi `failed` yang tetap membawa instance, `cli.wait_timeout` tanpa `instance.get`, error sinkron (`resource.not_found` exit 10, `conflict` exit 3), `ipc.call_timeout` dengan `details.idempotencyKey` dan hint `retry safely with: --idempotency-key <key>` (test ini menunggu 10 detik), dan UUID atau key yang salah exit 2 tanpa koneksi → AC-6, AC-8
- [x] `cargo test -p stackly-cli --test instance get::` → golden `instance_get.txt` dan `.json`, `instance_get_failed.txt`, `instance_get_adopted.txt` (`, adopted, not adoptable` dan `, next attempt`), `instance_get_locked.txt` (blocker `needsSecret`), teks daemon yang di escape (tanpa ESC mentah di stdout), `resource.not_found` exit 10, dan id yang salah exit 2 → AC-7
- [x] `cargo test -p stackly-cli --test instance exit_codes::` → baris matriks exit code `alreadyInState` 0, `needs_recovery` di bawah `--wait` 7, `resource.not_found` 10, `instance.get` gagal setelah menunggu (exit tetap milik operasi), dan `cli.usage` 2 → AC-6, AC-7
- [x] `cargo test -p stackly-cli` → seluruh suite CLI hijau (tidak ada test lama yang berubah selain `tests/common/golden.rs`) → AC-5 sampai AC-8
- [x] `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-cli --test instance` lalu `git status --short` → tidak ada file golden yang berubah (golden tidak bergantung pada waktu atau mesin) → AC-7
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, dan `cargo xtask evidence check` → lulus → AC-5 sampai AC-8

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

- [x] `cargo build -p stackly-fake-server -p stacklyd`, lalu `R="$(mktemp -d /tmp/stackly-m3.XXXXXX)"`, `target/debug/examples/skeleton_seed --root "$R"`, dan `STACKLY_ROOT="$R" target/debug/stacklyd` di terminal lain; jalankan `next` dari seed, lalu `instance start <id> --wait`, `instance get <id>`, `ps -axww -o pid=,args= | grep "$R" | grep "stackly-fake-server serve"` → satu baris; `kill -9` daemon, jalankan lagi, ulangi `get` dan `ps` → PID dan `startedAt` sama, `adopted: true`, tetap satu proses → AC-9, AC-11
- [x] Buat instance dengan `--set-json initDelayMs=15000`, `kill -9` daemon begitu `ps` memperlihatkan `stackly-fake-server init`, jalankan lagi → operasi `needs_recovery`, satu instance `failed`, pembantu init selesai sendiri setelah 15 detik → AC-12

### Commands

- [x] `cargo test -p stackly-fake-server` → `--delay-ms` menunda sampai setelah token dibaca dan sebelum folder data dibuat; nilai yang bukan angka exit 2 → AC-4
- [x] `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test provider_packages` lalu `cargo test -p stackly-core --test provider_packages --test provider_rules --test supervisor_start` → golden `fake/init.json` memuat `--delay-ms=0`, revision `0.2.0`, argv init di test supervisor ikut → AC-4
- [x] `cargo xtask provider check providers/fake` → `ok testing/fake 0.2.0 <sha256>` → AC-4
- [x] `cargo test -p stackly-testkit` → `ProcessTable`, `CliRunner`, `ReapGuard`, `SpawnedDaemon::kill` → AC-9, AC-11, AC-12 (prasyarat)
- [x] `cargo xtask evidence run --kind fixture -- cargo test -p stacklyd --test skeleton --locked --no-fail-fast` → lima test: happy path (create, start, get, stop; satu proses `serve` selama jalan dan nol sesudah stop; file keychain ada), init gagal lalu abandon (`supervisor.init_failed`, `needs_recovery`, `operation.abandoned`, `supervisor.not_initialized`, tidak ada proses), keychain file tanpa dialog, restart `SIGKILL` dan `SIGTERM` (PID, `startedAt`, `adopted`, satu instance, satu proses, isi data sama, start berikutnya `alreadyInState`), create terputus (replay key yang sama, `instance.name_taken`, `dataInUse` bila pembantu terlihat hidup sebelum dan sesudah `get`, abandon, pembantu selesai sendiri) → AC-9 sampai AC-13
- [x] `ps -axww -o pid=,args= | grep stackly-test-` setelah suite → tidak ada proses `stackly-fake-server` dari run ini → AC-9, AC-11, AC-12
- [x] `cargo xtask evidence check`, `cargo xtask check-deps`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check` → lulus → AC-4, AC-9 sampai AC-13

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

Langkah build 9. Test Swift `SkeletonEndToEndTests` memakai binary seed dan `stacklyd` asli, dengan koneksi `StacklyClient` kedua (noninteractive) yang membuat perubahan. Langkah build 10 menambah record manual `NOT_RUN` untuk AC-16 (CLI yang mengubah, GUI yang menampilkan); reviewer menjalankannya dengan tangan dan mengisi hasilnya.

Keputusan kecil yang diambil saat membangun, karena spec diam:

- Setelah `stacklyd` mulai, panggilan pertama bisa dijawab `daemon.starting` (retryable) selama supervisor merekonsiliasi. Rig test mengirim ulang dengan key yang sama sampai 20 detik; model GUI tidak diubah.
- `SkeletonRig` memakai `LiveDaemon` yang sudah ada (root dibuat di sana, lalu seed dijalankan pada root itu sebelum `stacklyd` mulai), jadi helper daemon baru tidak dibuat. `ProcessRunner` menunggu exit lewat handler terminasi dan membunuh proses yang melewati batas waktu.
- Teardown selalu menghentikan setiap instance yang dibuat rig, membunuh proses yang tersisa, lalu menghentikan daemon, juga saat test gagal.
- Test tidak mengunci revision paket atau delay init; keduanya dibaca dari output seed.

### UI / manual

- [ ] Buka `apps/macos/Packages/StacklyUI/Tests/StacklyUITests/Support/ProcessRunner.swift` → tidak ada `waitUntilExit()`; exit datang lewat `terminationHandler`, dan watchdog mengirim SIGKILL setelah batas waktu → AC-14, AC-15
- [ ] Setelah `swift test` selesai, jalankan `ps -axww -o pid=,args= | grep stk-ui-` → tidak ada `stackly-fake-server` atau `stacklyd` dengan root `/private/tmp/stk-ui-*` yang tersisa → AC-14, AC-15
- [ ] Buka `evidence/manual/2026-10-20-walking-skeleton-through-the-cli-and-gui.json` → `covers` AC-001, AC-013, AC-029, `kind` `fixture`, `method` `manual`, `status` `NOT_RUN`, dan 15 `steps` bernomor (bangun, seed, `stacklyd`, app, create, start, tutup jendela, ⌘Q, Ctrl+C daemon, `kill -9` daemon, init gagal, create terputus, stop, bersih, isi record) → AC-16
- [ ] Jalankan langkah record itu di Mac ini pada root sementara (`export R="$(mktemp -d /tmp/stk-skel.XXXXXX)"`; seed `target/debug/examples/skeleton_seed --root "$R"`; `STACKLY_ROOT="$R" target/debug/stacklyd`; app `STACKLY_ROOT="$R" target/xcode/Build/Products/Debug/Stackly.app/Contents/MacOS/Stackly`; CLI `STACKLY_ROOT="$R" target/debug/stackly instance ...`) → baris `demo` muncul, berubah ke Running dan siap, dan Stopped tanpa klik atau refresh; PID sama setelah tutup jendela, ⌘Q, Ctrl+C dan `kill -9` daemon (`adopted`); tepat satu proses `serve` dan satu baris `demo`; `slow` dan `broken` Failed; tidak ada proses fake setelah stop → AC-16
- [ ] Isi `steps`, `actual`, `status`, `reviewer`, dan `finishedAt` di record, lalu `cargo xtask evidence check` → lulus; `status` `PASS` hanya kalau setiap hasil di atas terlihat sendiri oleh reviewer → AC-16

### Commands

- [ ] `cargo build --locked -p stacklyd -p stackly-fake-server && cargo build --locked -p stacklyd --example skeleton_seed` → binary `target/debug/stacklyd`, `target/debug/stackly-fake-server`, dan `target/debug/examples/skeleton_seed` ada (atau set `STACKLY_DAEMON_BIN` dan `STACKLY_SEED_BIN`) → AC-14, AC-15
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter SkeletonEndToEndTests` → 2 test lulus. `theInstanceStoreFollowsChangesAnotherClientMakes`: `demo` muncul di `InstanceStore` setelah create lewat koneksi kedua, melewati `running` dengan readiness `not_ready` sebelum `ready` (`readyDelayMs` 1500), berakhir `stopped` tanpa proses, dan himpunan ID di `operation.list` sama persis dengan operasi koneksi kedua → AC-14. `quittingTheGUIKeepsThreeInstancesRunningForTheNextModel`: `alpha`, `beta`, `gamma` ready, `willTerminate()`, lalu lewat koneksi kedua ketiganya tetap `running` dengan PID yang sama, `GET /health` menjawab 200, `operation.list` tidak berubah, dan model baru dari `ShellBootstrap.live` memegang ketiganya dengan PID yang sama → AC-15
- [ ] Hapus `target/debug/examples/skeleton_seed` sementara lalu jalankan filter yang sama → kedua test berhenti sebagai `BLOCKED` dengan petunjuk perintah build, bukan gagal; bangun lagi sesudahnya → AC-14, AC-15
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --skip "NativeTests\."` → seluruh suite `StacklyUI` hijau, termasuk aturan sumber yang tidak memindai `Tests/` → AC-14, AC-15
- [ ] `cargo xtask evidence run --kind fixture -- sh -c 'cargo build --locked -p stacklyd -p stackly-fake-server && cargo build --locked -p stacklyd --example skeleton_seed && swift test --package-path apps/macos/Packages/StacklyUI --skip "NativeTests\."'` lalu `cargo xtask evidence check` → record `fixture` untuk kedua test dengan `covers` AC-001 dan AC-029 (AC-14) serta AC-001 (AC-15) lulus → AC-14, AC-15
- [ ] `xcrun swift-format lint --strict` pada ketiga file baru, lalu `cargo xtask build-app` → lulus → AC-14, AC-15
- [ ] `cargo xtask evidence check` → record manual AC-16 diterima dengan status `NOT_RUN` atau final → AC-16

### Acceptance-criteria coverage

- AC-14 · `theInstanceStoreFollowsChangesAnotherClientMakes`
- AC-15 · `quittingTheGUIKeepsThreeInstancesRunningForTheNextModel`
- AC-16 · record manual `2026-10-20-walking-skeleton-through-the-cli-and-gui.json` (langkah build 10), `NOT_RUN` sampai reviewer menjalankannya dan mengisi hasilnya; langkah otomatisnya (seed, `stacklyd`, `instance create`, `start`, `get`, `stop`, Ctrl+C dan `kill -9` daemon, init gagal, create terputus) sudah dicoba pada commit ini terhadap binary asli, hanya bagian yang butuh mata reviewer (jendela Stackly, notice, Dock, ⌘Q) yang belum
- AC-1 sampai AC-13 · milik Milestone 1 sampai 3
- AC-17 sampai AC-19 · belum, milik Milestone 5

## Milestone 5: Secrets end to end

Langkah build 11 sampai 13. `--secret` di `instance create` diuji terhadap daemon skrip; sentinel dan keychain terkunci berjalan dengan `stacklyd`, `stackly`, dan `stackly-fake-server` asli pada `TestRoot` (keychain file root itu, tanpa login Keychain); langkah 13 ada di `SkeletonEndToEndTests`.

Keputusan kecil yang diambil saat membangun, karena spec diam atau menyebut dua hal:

- `--secret` memakai `secret_input::field_source` apa adanya sebagai `value_parser`, jadi `expect(dead_code)` di `field_source` dan `SecretSource::Bind` dicabut. Sumber dibaca di `run` sebelum panggilan pertama ke daemon, jadi sumber yang ditolak (`cli.usage`, `cli.input_required`) tidak pernah menghubungi daemon.
- Field yang muncul lebih dari sekali memakai kemunculan terakhir (urutan field mengikuti kemunculan pertama), sama dengan `--set`. `stdin` dihitung setelah penggabungan itu; lebih dari satu field dengan `stdin` ditolak `cli.usage` dengan `kind` `secret_stdin_twice` sebelum stdin dibaca.
- Tidak ada file golden baru untuk `--secret`: teks dan `--json` create tidak berubah karena flag ini, jadi `instance_create.*` tetap golden nya. Yang diuji adalah params yang dikirim dan bahwa tidak ada output yang memuat nilai.
- Pemindaian sentinel membaca `stacklyd.log` (level debug), semua file di folder log daemon, file `.stderr` daemon, registry beserta `-wal` dan `-shm`, semua file di runtime root kecuali folder `secrets/`, dan `instance-manifest.json` tiap instance. `fake.json` dan folder data tidak dipindai (template menulis token ke config); sebagai gantinya test memeriksa `token.sha256` sama dengan SHA-256 nilai tanpa newline.
- Test keychain terkunci di `skeleton.rs` berhenti `BLOCKED` kalau sesi tidak punya GUI login (`launchctl managername` bukan `Aqua`), sama dengan test sentinel spec 0011. Test Swift tidak punya penjaga itu; ia hanya menjalankan `security lock-keychain` di root test.
- Di test Swift, daemon distart ulang lewat `SkeletonRig.restartDaemon()` (SIGTERM, lalu start pada root yang sama) dan koneksi kedua test diganti; model GUI tidak diubah dan menyambung ulang sendiri.

### UI / manual

- [x] `target/debug/stackly instance create --help` → `--secret <FIELD=SOURCE>` muncul dengan ketujuh sumber (`generate`, `prompt`, `stdin`, `file:PATH`, `env:NAME`, `copy:REF`, `bind:REF`) → AC-17
- [x] Dengan root development dan `stacklyd` hidup: `printf '%s\n' "$(uuidgen)" | STACKLY_ROOT="$R" target/debug/stackly instance create s1 --runtime <id> --provider testing/fake --revision 0.2.0 --secret authToken=stdin --wait`, lalu `grep -r` nilai itu di `$R` dan folder runtime root (kecuali `fake.json` dan `secrets/`) → tidak ditemukan → AC-17, AC-18
- [ ] Hentikan instance, `security lock-keychain "$R/data/keychain/stackly.keychain-db"`, lalu `instance start <id> --wait` tanpa TTY → gagal `secret.needs_secret` tanpa dialog, `instance get <id>` menunjukkan `Blocker` `needs secret`, dan baris di app (STACKLY_ROOT=$R) tetap Stopped; hentikan dan jalankan lagi `stacklyd`, ulangi start → berhasil → AC-19

### Commands

- [x] `cargo test -p stackly-cli --test instance secrets::` → enam test: setiap sumber (`generate`, `stdin` tanpa newline, `file:`, `env:`, `copy:`, `bind:`) menjadi `secrets[]` dalam urutan kemunculan pertama dengan kemunculan terakhir menang dan nilai tidak ada di stdout atau stderr; create tanpa `--secret` mengirim `secrets: []`; teks tanpa `--json` juga bersih; `--secret` salah bentuk, `stdin` dua kali, variabel tidak ada, stdin kosong, dan file tidak ada keluar 2 sebelum `instance.create`; `prompt` tanpa terminal keluar 2 `cli.input_required` segera; `--help` memuat flag → AC-17
- [x] `cargo test -p stackly-cli` → seluruh suite CLI hijau (tidak ada test lama yang berubah) → AC-17
- [x] `cargo build --locked -p stacklyd -p stackly-cli -p stackly-fake-server && cargo build --locked -p stacklyd --example skeleton_seed && cargo test -p stacklyd --test skeleton` → tujuh test, dua di antaranya baru: `the_fixture_credential_reaches_the_init_helper_and_leaves_no_trace_anywhere_else` (create dengan `--secret authToken=stdin`, `token.sha256` sama dengan SHA-256 nilai, lalu start dan stop; prefix kredensial tidak ada di semua file yang dipindai maupun di transkrip seluruh CLI) dan `a_locked_file_keychain_blocks_the_start_with_needs_secret_until_the_daemon_restarts` (start gagal `secret.needs_secret`, lifecycle `stopped`, `get` `blocker.type` `needsSecret`; setelah daemon distart ulang start berhasil dan `blocker` null) → AC-18, AC-19
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter SkeletonEndToEndTests` (binary dibangun lebih dulu seperti Milestone 4) → tiga test lulus; yang baru, `aLockedKeychainShowsTheNeedsSecretBlockerInTheStore`, mengunci keychain file root, start lewat koneksi kedua noninteractive ditolak `secret.needs_secret`, item `InstanceStore` berhenti `stopped` dengan blocker `needsSecret`, dan setelah daemon distart ulang start berhasil dan blocker hilang di store → AC-19
- [x] `cargo xtask evidence run --kind fixture -- cargo test -p stacklyd --test skeleton --locked --no-fail-fast` dan `cargo xtask evidence check` → record `fixture` untuk kedua test baru (`covers` AC-016, REQ-016) lulus → AC-18, AC-19
- [x] `ps -axww -o pid=,args= | grep stackly-fake-server` setelah semua suite → tidak ada proses dari root test yang tersisa → AC-17 sampai AC-19
- [x] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo xtask check-deps`, `xcrun swift-format lint --strict` pada file Swift yang berubah → lulus → AC-17 sampai AC-19

### Acceptance-criteria coverage

- AC-17 · `instance secrets::` (semua sumber, aturan baca, tidak ada nilai di output) dan `--help`
- AC-18 · `skeleton::the_fixture_credential_reaches_the_init_helper_and_leaves_no_trace_anywhere_else` (evidence `AC-016`, `REQ-016`)
- AC-19 · `skeleton::a_locked_file_keychain_blocks_the_start_with_needs_secret_until_the_daemon_restarts` dan `SkeletonEndToEndTests.aLockedKeychainShowsTheNeedsSecretBlockerInTheStore`
- AC-16 · belum, milik langkah build 10 (record manual)
- AC-1 sampai AC-15 · milik Milestone 1 sampai 4
