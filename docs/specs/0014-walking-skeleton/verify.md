# Verify: Walking skeleton · spec 0014 · updated 2026-10-20

_Langkah diturunkan dari acceptance criteria spec 0014. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27._

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
- [x] `cargo test -p stackly-cli --test instance action::` → `start` dan `stop` mengirim `instanceId` huruf kecil (`overrideLeases: false` untuk stop), `alreadyInState` keluar 0 dengan `operation` null dan satu `instance.get` (juga saat `--wait` dan saat `instance.get` gagal), teks tanpa `--wait` dengan hint `wait with: stackly operation inspect <id> --wait`, `--wait` sukses satu baris `<name> <lifecycle>, readiness <state>`, operasi `failed` yang tetap membawa instance, `cli.wait_timeout` tanpa `instance.get`, error sinkron (`resource.not_found` exit 10, `conflict` exit 3), `ipc.call_timeout` dengan `details.idempotencyKey` dan hint `retry safely with: --idempotency-key <key>` (test ini menunggu 10 detik), dan UUID atau key yang salah exit 2 tanpa koneksi → AC-6, AC-8
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
