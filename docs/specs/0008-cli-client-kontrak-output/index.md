# 0008. CLI client dan kontrak output

**Date**: 2026-10-18
**Status**: Proposed

## Summary

Spec ini menetapkan kerangka CLI `stackly` dan kontrak yang dipegang script: exit code yang hanya diturunkan dari kategori error (delapan nilai PRD bagian 12, ditambah 10 untuk `not_found`, 1 untuk `internal`, dan 130 untuk Ctrl+C), output `--json` berupa satu baris envelope stabil berversi di stdout, progress yang selalu di stderr, mode noninteractive yang tidak pernah menunggu input, dan `--wait` yang menunggu operasi async tanpa pernah membatalkannya. Hanya `--json` dan exit code yang stabil; teks untuk manusia boleh berubah. Di atas kerangka itu dibangun `stackly daemon status` versi akhir serta `stackly operation list`, `inspect`, dan `cancel`, semuanya lewat daemon. Command group lain (instance di fitur 14, secret di fitur 11, dan seterusnya) menempel ke kerangka yang sama tanpa mengubah aturan output atau exit code.

## Requirements

**User stories**:

- Sebagai penulis script dan CI, saya ingin exit code yang stabil dan satu baris JSON yang bisa diparse tanpa ANSI atau progress, supaya automation saya tidak rusak saat tampilan untuk manusia berubah.
- Sebagai pengguna di Terminal, saya ingin tabel yang rapi, warna status yang membantu, waktu lokal, dan progress yang terlihat saat menunggu, supaya saya tahu apa yang sedang terjadi.
- Sebagai pengguna yang menjalankan perintah async, saya ingin `--wait` yang menunggu sampai selesai tanpa membatalkan operasi kalau saya berhenti menunggu, supaya Ctrl+C atau batas waktu tidak merusak pekerjaan yang sedang berjalan.
- Sebagai developer fitur 14 dan seterusnya, saya ingin satu pola untuk menambah command group, supaya perintah baru otomatis mengikuti kontrak output, exit code, dan mode noninteractive.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-028):

- **AC-1**: Exit code `stackly` hanya diturunkan dari `ErrorBody.category` lewat satu fungsi `exit_code`, tanpa pernah membaca `message` atau `code`: sukses 0, `internal` 1, `invalid_input` 2, `conflict` 3, `unauthorized` 4, `unsupported` 5, `unavailable` 6, `failed` 7, `timeout` 8, `cancelled` 9, `not_found` 10. Satu satunya pengecualian adalah `cli.interrupted` (Ctrl+C saat menunggu), yang keluar 130 lewat `exit_override` di `CliFailure`. Argumen yang ditolak clap keluar 2, sedangkan `--help` dan `--version` keluar 0. CLI menerima kode apa pun dari daemon, termasuk yang tidak ada di katalognya sendiri, dan tetap memetakan exit dari kategorinya. Untuk setiap kategori, daemon skrip yang menjawab dengan kode berkategori itu (kode sintetis seperti `test.failed` untuk kategori yang belum punya kode di katalog) menghasilkan exit sesuai tabel; dua kode dengan kategori sama tetapi pesan berbeda menghasilkan exit yang sama, dan pesan yang sama dengan kategori berbeda menghasilkan exit yang berbeda (REQ-028, AC-028, PRD bagian 12).
- **AC-2**: Dengan `--json`, perintah non streaming menulis tepat satu baris ke stdout: objek JSON compact diakhiri `\n` dengan field berurutan `schemaVersion` (1), `type` (`result`), `command`, `result`, `error`, `requestId`, dan `warnings`, lalu tidak ada byte lain. Exit 0 kalau dan hanya kalau `error` null. `error` adalah `ErrorBody` dari daemon seperti yang didecode client (field yang dikenal versi protokol client), tanpa satu field pun diubah atau ditambah CLI, atau dibuat CLI dengan area `cli`; `requestId` membawa ID request daemon yang menghasilkan error itu (null untuk error sisi CLI). Baris itu tidak memuat byte ESC (0x1B) atau carriage return walau `--color always` aktif dan stderr adalah TTY yang sedang menampilkan progress. Argumen yang ditolak clap dengan `--json` di argv (dicari sebelum `--`) juga menghasilkan envelope dengan `cli.usage` dan `command` kosong. `--help` dan `--version` bukan perintah: keduanya selalu menulis teks ke stdout dengan exit 0, juga saat `--json` ada. Golden file per perintah menjaga bentuknya (REQ-028, AC-028).
- **AC-3**: Tanpa `--json`, hasil ditulis ke stdout, sedangkan error dan warning ke stderr. Tabel memakai kolom rata kiri tanpa border dengan header huruf besar, lebar kolom dihitung dengan `unicode-width`, dan teks tidak pernah dipotong. Warna hanya muncul kalau stream nya TTY dengan `--color auto` (default), selalu dengan `--color always`, tidak pernah dengan `--color never`; `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, dan `TERM=dumb` dihormati untuk mode auto. State selalu tertulis sebagai teks (nilai wire seperti `needs_recovery`), dan warnanya dari lima tone spec 0012 hanya sebagai tambahan. Waktu tertulis di zona waktu sistem (`2026-10-18 10:04:05 +07:00`), dan tabel memakai kolom umur (`3m ago`). Karakter kontrol C0 dan C1, DEL, dan karakter override bidi di teks yang berasal dari daemon atau argumen ditulis sebagai escape `\u{..}`.
- **AC-4**: Progress hanya ditulis ke stderr, langsung ke handle stderr (bukan lewat `anstream`). Kalau stderr TTY dan `TERM` bukan `dumb`, progress adalah satu baris paling lebar 60 kolom yang ditimpa dengan `\r` plus kode hapus baris, dan dihapus sebelum tulisan lain apa pun ke stderr (warning, baris `-v`, error) maupun sebelum output akhir; kalau tidak, satu baris polos per perubahan state atau tahap, tanpa `\r` atau ANSI. `--quiet` mematikan progress, warning teks, dan hint, tetapi tidak mematikan error. stdout tidak pernah menerima progress dalam mode apa pun.
- **AC-5**: CLI interaktif kalau dan hanya kalau stdin dan stderr adalah TTY, `--no-input` tidak dipasang, dan `STACKLY_NO_INPUT` tidak aktif (`1` atau `true` tanpa peduli huruf besar menyalakan; tidak di set, kosong, `0`, atau `false` mematikan; nilai lain gagal `cli.invalid_environment`, exit 2). Dalam mode noninteractive, setiap prompt gagal `cli.input_required` (exit 2) tanpa membaca stdin. Konfirmasi lewat komponen `Confirm`, yang menerima flag persetujuan dari perintah pemanggil (default `--yes` lewat `ConfirmArgs`): hanya perintah yang butuh konfirmasi punya flag itu; mode interaktif menulis pertanyaan, akibat, dan `[y/N]` ke stderr lalu membaca satu baris dari stdin (yang pasti TTY dalam mode interaktif), dan hanya `y` atau `yes` (tanpa peduli huruf besar) yang menyetujui, sedangkan Enter, EOF, atau jawaban lain gagal `cli.confirmation_declined` (exit 9); mode noninteractive tanpa flag persetujuan gagal `cli.confirmation_required` (exit 2) sebelum apa pun dikirim ke daemon, kecuali perintah memilih membiarkan daemon menjawab dengan kode konfirmasinya sendiri (misalnya `secret.confirmation_required` di spec 0011) (REQ-028).
- **AC-6**: `stackly daemon status` membaca `STACKLY_ROOT` sekali, memverifikasi endpoint, melakukan handshake sebagai client `cli`, lalu menampilkan status kesehatan, versi dan protokol, pid, uid, waktu start (lokal), `serverInstanceId`, subsystem yang tidak `ok`, keempat root, path socket, dan apakah override aktif. Dengan `--json`, `result` adalah `{ "info": DaemonInfo, "health": DaemonHealth | null, "client": { "protocol", "contractDigest", "contractDrift" } }`. Kalau `STACKLY_ROOT` aktif, warning `cli.root_override` yang menyebut path nya ditulis ke stderr (teks) atau ke `warnings` (JSON); kontrak yang berbeda menambah warning `cli.contract_drift`. Status kesehatan apa pun tetap exit 0. Kalau `get_info` berhasil tetapi `get_health` gagal, `result` berisi info dengan `health` null, `error` terisi, dan exit mengikuti kategori error itu. Daemon yang tidak jalan keluar 6 dengan pesan yang menyebut path socket, endpoint yang tidak dipercaya keluar 4, dan major yang berbeda keluar 5. CLI tidak pernah menjalankan `stacklyd`, membuat folder, atau menulis file (REQ-028, BR-011).
- **AC-7**: `stackly operation list` memanggil `operation.list` dengan default 50 operasi terbaru dari semua state; `--active` hanya yang belum berstate akhir (termasuk `needs_recovery`), `--resource <kind>:<uuid>` (atau `manager`, dengan kind dari filter spec 0007) membatasi ke satu resource, `--limit` 1 sampai 500, dan `--before <cursor>` melanjutkan dari `next` sebelumnya. `result` JSON nya `{ "items": [OperationDto], "next": string | null }`. `stackly operation inspect <id>` memanggil `operation.get` dan `stackly operation cancel <id>` memanggil `operation.cancel`; `<id>` wajib UUID lengkap (selain itu exit 2 dari clap). Operasi yang tidak ada keluar 10. Cancel yang diterima daemon keluar 0 dan menampilkan state hasilnya (`cancel_requested` atau `cancelled`); cancel pada operasi yang sudah berstate akhir keluar sesuai kategori error spec 0007. Semua perintah ini hanya bicara ke daemon dan tidak pernah membaca registry (REQ-028, REQ-054, BR-011).
- **AC-8**: `stackly operation inspect <id> --wait` dan `stackly operation cancel <id> --wait` menunggu sampai state akhir (`succeeded`, `failed`, `cancelled`) atau `needs_recovery`. Untuk inspect: `succeeded` exit 0, state lain exit dari kategori `ErrorBody` operasi itu (atau kode cadangan `cli.operation_failed`, `cli.operation_cancelled`, `cli.operation_needs_recovery` kalau DTO tidak membawanya). Untuk cancel: `cancelled` exit 0, `succeeded` atau `failed` gagal `cli.cancel_not_applied` (exit 3) dengan state akhir tertulis apa adanya, dan `needs_recovery` exit dari kategori error operasinya. `--timeout <durasi>` hanya berlaku dengan `--wait`; kalau lewat, CLI gagal `cli.wait_timeout` (exit 8) dengan `operationId`. Ctrl+C saat menunggu gagal `cli.interrupted` (exit 130) dan mencetak operation ID beserta cara memeriksanya. Batas waktu dan Ctrl+C berlaku untuk seluruh penantian, termasuk connect, pengulangan `daemon.starting`, `operation.get` pertama, dan sambung ulang, sehingga `--timeout 1s` selesai paling lama sekitar 1 detik setelah batasnya; sebelum DTO pertama diterima, `details.state` bernilai `unknown`. Batas waktu maupun Ctrl+C tidak pernah mengirim `operation.cancel` dan operasinya tetap berjalan. Progress diambil dari field state, tahap, dan progress operasi (REQ-028, REQ-054, AC-054 bagian CLI).
- **AC-9**: Connect plus handshake dibatasi 5 detik. Setiap request punya batas yang dipilih perintah pemanggil: `Control` 10 detik (default) atau `Interactive` 200 detik untuk panggilan yang bisa menunggu dialog Keychain (interaksi `AllowPrompt` spec 0011); yang lewat gagal `cli.request_timeout` (exit 8). Jawaban `daemon.starting` diulang otomatis dengan backoff selama paling lama 10 detik, lalu exit 6 kalau daemon masih starting; kode retryable lain tidak pernah diulang otomatis di luar `--wait`. Saat `--wait`, hanya koneksi yang benar benar putus (akhir `EventStream`, `ipc.connection_closed`, `ipc.endpoint_missing`, `ipc.connection_refused`) yang memicu sambung ulang; `cli.request_timeout` dan `daemon.starting` pada `operation.get` diulang di dalam anggaran yang sama; error lain mengakhiri penantian dengan error itu. Sambung ulang memakai backoff selama paling lama 30 detik sejak putus, lalu melanjutkan menunggu (termasuk ke daemon baru dengan `serverInstanceId` berbeda); kalau tidak tersambung lagi, CLI gagal `cli.wait_disconnected` (exit 6) dengan `operationId` (REQ-028, NFR-006).
- **AC-10**: Hook panic milik CLI dipasang paling awal dan tidak mencetak isi pesan panic; panic di thread utama (lewat `catch_unwind`) maupun di task tokio (lewat `JoinError`) menjadi `cli.panic` (exit 1), ditulis sebagai envelope kalau mode `--json` sudah diketahui, dan dengan `-v` lokasi panic (file dan baris) ditulis ke stderr. stdout yang tertutup (broken pipe, misalnya `| head -1`) menghentikan penulisan tanpa pesan; kegagalan tulis stdout yang lain menulis satu baris `cli.output_failed` ke stderr. Dalam kedua kasus exit tetap mengikuti hasil perintah, supaya mutasi yang sudah berhasil tidak dilaporkan gagal.
- **AC-11**: `cargo xtask check-deps` gagal, dengan pesan `stackly-cli <file>:<baris>: the cli never spawns processes (spec 0008, BR-011)`, kalau kode non test di `apps/cli` memakai item `std::process` selain `exit`, `ExitCode`, dan `id` (termasuk lewat alias modul seperti `use std::process as p` atau glob `use std::process::*`), menyebut `tokio::process` atau `std::os::unix::process`, atau kalau `stackly-cli` punya dependency normal langsung `libc`, `nix`, `duct`, `subprocess`, `xshell`, `open`, atau `opener` (dependency dev tidak dihitung). Ia juga gagal kalau kode non test `apps/cli` memakai makro `print!`, `println!`, `eprint!`, atau `eprintln!` (panic saat broken pipe), dan kalau kode non test di luar `apps/cli` dan `crates/protocol` menyebut `codes::cli`. Aturan lama (tanpa `stackly-core` dan `stackly-storage`) tetap berlaku. Test memakai crate fixture yang melanggar setiap aturan, termasuk `use std::process::Command;`, `std::process::Command::new` lengkap, dan alias modul (BR-011).
- **AC-12**: Command group ditambah lewat satu modul di `apps/cli/src/commands/`, satu varian di enum `Command`, dan tipe hasil yang mengimplementasikan `Serialize` dan `Render`, tanpa mengubah modul `output`, `exit`, `session`, `wait`, `confirm`, atau `progress`. Group `daemon` dan `operation` di fitur ini dibangun persis dengan pola itu.
- **AC-13**: `-v` menulis satu baris ke stderr saat request daemon dikirim (method dan `requestId` yang dibuat CLI sebelum mengirim) dan satu baris saat selesai (hasil `ok`, kode error, atau `timeout`, dan durasi dalam milidetik), tanpa params, isi reply, atau nilai secret; `-v` dan `-q` bekerja terpisah. `stackly --help` memuat flag global, variabel environment (`STACKLY_ROOT`, `STACKLY_NO_INPUT`, `NO_COLOR`, `CLICOLOR_FORCE`), dan tabel exit code; setiap perintah punya deskripsi dan paling tidak satu contoh. `stackly --version` mencetak `stackly <versi> (protocol <major>.<minor>)`.
- **AC-14**: Semua kode `cli.*` ada di katalog `stackly-protocol` (`codes::cli`) dengan kategori, `retryable` false, dan kunci `details` sesuai tabel *Kode error area cli*, ikut di `protocols/error-codes.json`, dan lulus test katalog spec 0006. Daemon tidak pernah mengirim kode area `cli`, dijaga pindaian `check-deps` di AC-11.

## Decision

**Chosen option**: pilihan yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md): exit code dari kategori dengan 10 untuk `not_found`, 1 untuk `internal`, dan 130 untuk Ctrl+C; envelope JSON satu baris berversi di stdout; progress di stderr (baris hidup di TTY, baris per tahap di luar TTY); warna otomatis lewat `anstream` dengan `--color`; mode noninteractive dari TTY, `--no-input`, atau `STACKLY_NO_INPUT`; `--yes` per perintah; perintah async kembali dengan operation ID; `--wait` tanpa batas default dengan `--timeout` opsional, sambung ulang 30 detik, dan Ctrl+C yang tidak membatalkan; serta test lewat `stacklyd` sungguhan, daemon skrip, dan pty.

Kerangka CLI adalah satu lapisan output dan satu peta exit code yang dipakai semua command group, sehingga perintah baru hanya menulis logika panggilan daemon dan tampilan hasilnya.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya: tidak ada `unwrap()` di luar test walau contoh skill memakainya, dan setiap channel atau antrean di CLI berbatas.

## Feature design

### Batas dengan spec lain

| Bagian | Pemilik |
| --- | --- |
| Envelope `ErrorBody`, sepuluh kategori, nama kode `area.alasan`, handshake, verifikasi endpoint, `Client` dan `EventStream`, resep snapshot plus cursor | spec 0006 |
| Exit code per kategori, envelope output `--json`, output manusia, progress, mode noninteractive, `Confirm`, `--wait`, kode area `cli`, kerangka command group | spec ini |
| Method, DTO, event, dan kode area `operation`; arti idempotency key; state dan aturan cancel | spec 0007 |
| Kontrak input secret aman dan modul `secret_input`, perintah `stackly secret` | spec 0011 (langkah 8), di atas kerangka ini |
| `STACKLY_ROOT`, `MacosPlatform::new`, `daemon_socket()` | spec 0004 |
| Lima tone status dan aturan konfirmasi destruktif | spec 0012 (CLI mengikuti artinya) |
| `stackly instance create`, `start`, `stop` sementara | fitur 14, di atas kerangka ini |
| `stackly daemon install`, `start`, `stop`, `restart`, completion shell, man page | fitur 15 |

### Peta crate dan modul

Semua aturan arah dependency spec 0001, 0004, dan 0006 tetap berlaku. `stackly-cli` tetap bin saja.

| Crate | Dependency langsung yang baru | Isi baru |
| --- | --- | --- |
| `stackly-protocol` | tidak ada | `codes::cli` (katalog kode area `cli`), area `cli` di `AREAS` |
| `stackly-ipc` | tidak ada | `Client::call_detailed(Call, CallOptions { request_id: Option<String>, idempotency_key: Option<String> }) -> Result<Reply, ErrorBody>`, tambahan API tanpa perubahan wire: pemanggil boleh memberi `requestId` sendiri (formatnya dicek sama dengan aturan spec 0006) supaya ID nya sudah diketahui sebelum dan walau tanpa balasan; `Client::call` lama memanggilnya dengan opsi kosong |
| `stackly-cli` | `anstream`, `anstyle`, `unicode-width`, `jiff`, `uuid`, `serde`; fitur clap `string` (teks `--version` yang dirakit saat runtime); fitur tokio tambahan `time`, `sync`, `signal`; dev: `stackly-testkit`, `stackly-evidence` | modul di bawah |
| `xtask` | tidak ada | aturan `check-deps` AC-11 |

Root `Cargo.toml` menambah `anstream = "1.0"`, `anstyle = "1.0"`, dan `unicode-width = "0.2"` ke `[workspace.dependencies]`. `anstream` dan `anstyle` sudah ada di `Cargo.lock` lewat clap.

Modul `apps/cli/src`:

| Modul | Isi |
| --- | --- |
| `main.rs` | tipis: memasang hook panic milik CLI, memanggil `app::run()` di dalam `std::panic::catch_unwind`, lalu `ExitCode::from(code)` |
| `app.rs` | `Cli` (root clap), `GlobalArgs`, enum `Command`, pindaian argv mentah untuk `--json` dan `--color` sebelum clap, peta `clap::error::ErrorKind` ke `details.kind`, dispatch |
| `panic.rs` | hook panic yang hanya menyimpan lokasi ke `OnceLock`, dan mode output yang disimpan setelah parse supaya envelope `cli.panic` bisa ditulis |
| `env.rs` | membaca `STACKLY_ROOT` (lewat `MacosPlatform::new`) dan `STACKLY_NO_INPUT` sekali |
| `context.rs` | `Context`: mode output, interaktif, `Session` lazy, kumpulan warning |
| `exit.rs` | `exit_code(ErrorCategory) -> u8` (match exhaustive) dan `EXIT_INTERRUPTED = 130` |
| `failure.rs` | `CliFailure { body: ErrorBody, request_id: Option<String>, exit_override: Option<u8> }`, konstruktor per kode `cli.*`; hanya konstruktor `interrupted` yang mengisi `exit_override` |
| `output/mod.rs` | `OutputMode` (`Human` atau `Json`), `CommandOutput<T>`, `emit` |
| `output/envelope.rs` | `Envelope<'a, T: Serialize>` (field `result: Option<&'a T>` supaya urutan field hasil tetap urutan struct), `StreamItem<'a, T>`, `Warning`, `CLI_SCHEMA_VERSION = 1`, writer satu baris |
| `output/stderr.rs` | `StderrSink`: satu pemilik stderr yang dipakai progress, warning, baris `-v`, hint, dan error; membersihkan baris progress hidup sebelum tulisan lain |
| `output/human.rs` | trait `Render`, `HumanWriter` dengan `kv`, `table`, `state`, `time`, `age` |
| `output/escape.rs` | `escape_display(&str) -> Cow<str>` |
| `output/style.rs` | `Tone` (lima tone spec 0012) ke `anstyle::Style`, pemetaan state ke tone |
| `output/time.rs` | `format_local(ts, tz)`, `format_age(ts, now)`, fungsi murni |
| `progress.rs` | `Progress` dengan mode `Live`, `Lines`, `Off` |
| `confirm.rs` | `Confirm`, `ConfirmArgs { yes }`, `ConfirmRole` (`Standard`, `Destructive`) |
| `session.rs` | `Session`: connect lazy, `RequestTimeout` (`Control`, `Interactive`), request ID buatan CLI, ulang `daemon.starting`, baris `-v` |
| `wait.rs` | `Waiter`, `WaitArgs { wait, timeout }`, `WaitOutcome` |
| `args.rs` | parser nilai: `OperationIdArg` (UUID lengkap), `ResourceFilter`, `CursorArg`, durasi (jiff friendly), `IdempotencyKeyArg` |
| `commands/daemon.rs` | `daemon status` |
| `commands/operation.rs` | `operation list`, `inspect`, `cancel` |

### Struktur perintah dan flag global

Bentuk perintah `stackly <group> <verb> [argumen]` seperti PRD bagian 12. Flag global (clap `global = true`, boleh di posisi mana pun):

| Flag | Arti |
| --- | --- |
| `--json` | output mesin, satu baris envelope di stdout |
| `--color <auto\|always\|never>` | default `auto` |
| `--no-input` | mode noninteractive |
| `-q`, `--quiet` | tanpa progress dan tanpa warning teks; error tetap ditulis |
| `-v`, `--verbose` | satu baris per request daemon di stderr |

Flag milik perintah, didefinisikan sekali sebagai struct yang di `#[command(flatten)]`:

- `WaitArgs`: `--wait`, `--timeout <durasi>` (clap `requires = "wait"`). Durasi berupa bilangan bulat detik (`90`) atau format friendly jiff dengan satuan `s`, `m`, dan `h` (`30s`, `5m`, `1h30m`), dari 1 detik sampai 168 jam; di luar itu ditolak clap (exit 2). Deadline dihitung dengan `checked_add`, jadi tidak pernah panic.
- `AsyncArgs` (fitur 14 ke atas): `WaitArgs` ditambah `--idempotency-key <key>` (1 sampai 128 byte ASCII tercetak, aturan spec 0006).
- `ConfirmArgs`: `--yes`, `-y`. Hanya di perintah yang memanggil `Confirm` dengan flag default, jadi clap menolak `--yes` di perintah lain. Konfirmasi khusus (misalnya `--confirm-stored-only` spec 0011) memakai flag milik perintah itu.

Urutan saat start:

1. Hook panic dipasang.
2. Pindaian argv mentah sampai `--` pertama: argumen yang persis `--json` menandai mode JSON untuk error clap, dan `--color <v>` atau `--color=<v>` mengatur `ColorChoice` clap sebelum parse, supaya help dan error clap ikut `--color`.
3. Parse clap. `--help`, `--version`, dan `stackly help <perintah>` menulis teks ke stdout dan keluar 0, juga dengan `--json`. `stackly` tanpa argumen menulis help ke stderr dan keluar 2 (`arg_required_else_help`). Error clap: tanpa `--json`, clap menulis teksnya sendiri ke stderr dan keluar 2; dengan `--json`, envelope `cli.usage` dengan `command` kosong, `message` dari baris pertama teks polos error clap tanpa awalan `error: ` dan tanpa titik di akhir (dipotong 200 karakter oleh `ErrorBody::new`), dan `details.kind` dari peta eksplisit `ErrorKind` ke snake_case (`invalid_value`, `unknown_argument`, `missing_required_argument`, `invalid_subcommand`, `missing_subcommand`, `argument_conflict`, `value_validation`, `too_many_values`, `too_few_values`, `wrong_number_of_values`, `no_equals`, `invalid_utf8`), dengan `other` untuk jenis lain karena enum clap `non_exhaustive`.
4. Environment (`env.rs`): `STACKLY_ROOT` lalu `STACKLY_NO_INPUT`. Help dan version tidak pernah gagal karena environment, karena sudah selesai di langkah 3.
5. Deteksi terminal, lalu perintah dijalankan.

`-q` dan `-v` bekerja terpisah: `-q` mematikan progress, warning teks, dan hint; `-v` menambah baris request.

### Peta exit code

| Exit | Arti | Sumber |
| --- | --- | --- |
| 0 | sukses | `error` null |
| 1 | bug atau invariant Stackly yang rusak | kategori `internal` (termasuk `cli.panic`) |
| 2 | input atau argumen salah | kategori `invalid_input`, juga error clap |
| 3 | conflict, state berubah atau resource sedang dipakai | `conflict` |
| 4 | identitas, peer, atau trust ditolak | `unauthorized` |
| 5 | versi, method, atau capability tidak didukung | `unsupported` |
| 6 | daemon atau dependency tidak tersedia | `unavailable` |
| 7 | pekerjaan dijalankan dan gagal | `failed` |
| 8 | batas waktu lewat; hasil bisa belum diketahui | `timeout` |
| 9 | dibatalkan | `cancelled` |
| 10 | resource tidak ada | `not_found` |
| 130 | pengguna menekan Ctrl+C saat CLI menunggu; operasi tetap berjalan | hanya `cli.interrupted` |

`ErrorCategory` adalah enum tertutup (spec 0006), jadi `exit_code` memakai `match` exhaustive tanpa cabang `_`; kategori baru hanya mungkin lewat protokol major baru dan compiler memaksa tabel ini diperbarui. Exit akhir adalah `failure.exit_override` kalau terisi (hanya `cli.interrupted`), selain itu `exit_code(failure.body.category)`. Ctrl+C di luar `--wait` tidak ditangani: proses mati oleh SIGINT, shell melaporkan 130, dan tidak ada envelope yang ditulis.

### Kode error area `cli`

Semua di `stackly-protocol::codes::cli`, `retryable` false, tidak pernah dikirim lewat wire. Pesan bahasa Inggris, huruf kecil di awal, tanpa titik di akhir (spec 0006).

| Kode | Kategori | Kapan | Kunci `details` |
| --- | --- | --- | --- |
| `cli.usage` | `invalid_input` | clap menolak argv (envelope hanya dengan `--json`; tanpa itu clap menulis teksnya sendiri) | `kind` |
| `cli.invalid_environment` | `invalid_input` | `STACKLY_ROOT` ditolak resolver, atau `STACKLY_NO_INPUT` bernilai tidak dikenal | `variable`, `reason` |
| `cli.platform_unavailable` | `unavailable` | `MacosPlatform::new` gagal karena alasan lain (home atau folder temp tidak terbaca, path socket melewati anggaran) | `reason` |
| `cli.input_required` | `invalid_input` | prompt dibutuhkan dalam mode noninteractive | `input` |
| `cli.confirmation_required` | `invalid_input` | konfirmasi dibutuhkan dalam mode noninteractive tanpa `--yes` | `flag` |
| `cli.confirmation_declined` | `cancelled` | pengguna tidak menjawab `y` atau `yes` | |
| `cli.request_timeout` | `timeout` | connect plus handshake lewat 5 detik atau request lewat batas `RequestTimeout` nya; request bisa saja sudah berefek | `method`, `seconds`, `idempotencyKey` (kalau request membawanya) |
| `cli.wait_timeout` | `timeout` | `--timeout` lewat sebelum state akhir | `state`, `timeoutSeconds` |
| `cli.wait_disconnected` | `unavailable` | koneksi tidak kembali dalam 30 detik saat menunggu | `state` |
| `cli.interrupted` | `cancelled` | Ctrl+C saat menunggu; exit 130 | `state` |
| `cli.cancel_not_applied` | `conflict` | `operation cancel --wait` berakhir `succeeded` atau `failed` | `state` |
| `cli.operation_failed` | `failed` | cadangan: operasi `failed` tanpa `error` di DTO | |
| `cli.operation_cancelled` | `cancelled` | cadangan: operasi `cancelled` tanpa `error` di DTO | |
| `cli.operation_needs_recovery` | `failed` | cadangan: operasi `needs_recovery` tanpa `error` di DTO | |
| `cli.panic` | `internal` | panic di CLI tertangkap | |

Kode yang berhubungan dengan operasi selalu mengisi `operationId` dan `resource` (`operation`, ID nya). Menambah kode ini mengubah digest kontrak, jadi commit nya menaikkan minor `ProtocolVersion::CURRENT` sesuai aturan spec 0006.

Kode warning (bukan error, tidak di katalog error, daftar tetap di `output/envelope.rs`): `cli.root_override` (`STACKLY_ROOT` aktif, pesan menyebut path nya), `cli.contract_drift` (major dan minor sama, digest berbeda), dan `cli.output_failed` (tulis stdout gagal selain broken pipe; hanya ditulis ke stderr karena stdout nya sendiri yang gagal, dan exit tetap mengikuti hasil perintah).

### Kontrak `--json`

Satu baris per record di stdout, JSON compact (tanpa spasi atau indentasi) diakhiri `\n`, dengan urutan field tetap sesuai deklarasi struct. Perintah non streaming menulis tepat satu record `result`:

```json
{"schemaVersion":1,"type":"result","command":"operation inspect","result":{"id":"0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e","state":"failed","...":"..."},"error":{"code":"instance.start_failed","category":"failed","...":"..."},"requestId":null,"warnings":[]}
```

| Field | Tipe | Isi |
| --- | --- | --- |
| `schemaVersion` | integer | `CLI_SCHEMA_VERSION`, sekarang 1 |
| `type` | string | `result` untuk record akhir, `item` untuk record stream |
| `command` | string | path perintah dipisah spasi, misalnya `daemon status`, `operation inspect`; string kosong untuk `cli.usage` dan untuk panic sebelum perintah terpilih |
| `result` | objek atau null | tipe hasil perintah; boleh terisi bersama `error` hanya untuk hasil parsial yang didokumentasikan perintah itu |
| `error` | `ErrorBody` atau null | null kalau dan hanya kalau exit 0; dari daemon tanpa diubah CLI (hint tambahan CLI hanya muncul di teks manusia) |
| `requestId` | string atau null | `requestId` request daemon yang menghasilkan `error`; null untuk error sisi CLI atau sukses |
| `warnings` | `Warning[]` | `{ code, message }`, boleh kosong |

Perintah streaming (pertama di fitur 27, misalnya `instance logs --follow`) menulis nol atau lebih record `{"schemaVersion":1,"type":"item","command":"...","item":{...}}` lalu satu record `result` sebagai penutup. Pembaca membedakan lewat `type`. Fitur ini tidak punya perintah streaming.

Hasil per perintah di fitur ini:

| Perintah | `result` | Parsial |
| --- | --- | --- |
| `daemon status` | `{ info: DaemonInfo, health: DaemonHealth \| null, client: { protocol, contractDigest, contractDrift } }` | `health` null dengan `error` kalau `get_health` gagal |
| `operation list` | `{ items: OperationDto[], next: string \| null }` | tidak |
| `operation inspect` | `OperationDto` | dengan `--wait`: DTO terakhir bersama `error` kalau tidak sukses |
| `operation cancel` | `OperationDto` setelah cancel | dengan `--wait`: DTO terakhir bersama `error` kalau tidak sukses |
| perintah async (fitur 14 ke atas) | `{ operation: OperationDto, idempotencyKey: string, ... }`; fitur pemilik boleh menambah key saudara (misalnya `instance`) | dengan `--wait`: sama seperti inspect |

Aturan stabilitas:

- Hanya `--json` dan exit code yang menjadi kontrak. Teks untuk manusia boleh berubah kapan saja dan tidak boleh diparse script.
- Tambahan yang aman: field baru di envelope atau di hasil, kode warning baru, kode error baru, nilai baru di enum terbuka. Pembaca wajib mengabaikan field yang tidak dikenal dan memperlakukan nilai enum asing sebagai `unknown`.
- Menghapus atau mengganti nama field, mengubah tipe atau arti, atau mengubah peta exit code adalah perubahan breaking: `CLI_SCHEMA_VERSION` naik, dan spec ini, golden file, test, dan contoh diperbarui di commit yang sama. Hasil yang menyisipkan DTO protokol ikut aturan versi protokol; protokol major yang mengubah DTO yang disisipkan juga menaikkan `CLI_SCHEMA_VERSION` di commit yang sama.
- Dalam mode `--json`, warning hanya masuk `warnings`, tidak ditulis ke stderr. Progress tetap di stderr.
- `ErrorBody` dari daemon didecode ke tipe protokol client, jadi field yang ditambah daemon dengan minor lebih baru tidak ikut di envelope; pembaca yang butuh field itu memakai client dengan protokol yang sama.

### Output manusia

- stdout untuk hasil; stderr untuk error, warning, hint, footer halaman, dan progress.
- `HumanWriter` membungkus `anstream::AutoStream` per stream dengan `ColorChoice` dari `--color`; mode auto memakai deteksi TTY milik `anstream` dan menghormati `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, dan `TERM=dumb`.
- `kv(label, value)`: label rata kiri dengan lebar label terpanjang di blok itu ditambah dua spasi.
- `table(headers, rows)`: header huruf besar, kolom dipisah dua spasi, lebar per kolom dari `unicode-width`, tanpa border, tanpa memotong. Sel tanpa nilai ditulis `-`. Daftar kosong menulis satu kalimat (misalnya `No operations.`) ke stdout.
- Nilai kosong di `kv` (misalnya `Started` yang null) juga ditulis `-`. Durasi ditulis dengan dua satuan terbesar tanpa spasi: `12s`, `3m04s`, `1h02m`.
- Semua teks yang berasal dari daemon atau argumen lewat `escape_display`: C0 (U+0000 sampai U+001F, termasuk tab dan newline di dalam nilai), DEL (U+007F), C1 (U+0080 sampai U+009F), dan override bidi (U+202A sampai U+202E, U+2066 sampai U+2069) ditulis sebagai `\u{1b}` dan seterusnya. Label dan teks tetap milik CLI tidak di escape.
- State ditulis dengan nilai wire nya (`cancel_requested`, `needs_recovery`, `unknown`), diberi warna dari tone:

| Tone (spec 0012) | Warna terminal (`anstyle::AnsiColor`) | State |
| --- | --- | --- |
| `positive` | `Green` | health `ready`, subsystem `ok` |
| `caution` | `Yellow` | `cancel_requested`, `succeeded` dengan `partial`, health `starting`, `degraded`, `stopping`, subsystem `degraded`, nilai `unknown` apa pun |
| `critical` | `Red` | `failed`, `needs_recovery`, subsystem `unavailable` |
| `informative` | `BrightBlue` (biru biasa sulit dibaca di tema gelap) | `running` |
| `neutral` | tanpa warna | `queued`, `succeeded`, `cancelled`, subsystem `not_built` |

- Error di stderr: baris `error: <message>` (`error:` merah tebal kalau berwarna), lalu baris berindentasi `code: <code>`, `hint: <hint>` bila ada, satu baris per `fields[]` (`<path>: <message>`), dan `request: <requestId>` hanya dengan `-v`. Hint tambahan milik CLI (misalnya `start it with stacklyd`) ditulis sebagai baris `hint:` kedua di teks ini saja, tidak pernah dimasukkan ke `ErrorBody`. Warning: `warning: <message>` (kuning tebal), kecuali dengan `--quiet`.
- Waktu absolut: `format_local(ts, TimeZone::system())` menghasilkan `YYYY-MM-DD HH:MM:SS ±HH:MM`. Umur: `format_age(ts, now)` menghasilkan `now` (di bawah 1 detik atau di masa depan), lalu `<n>s ago`, `<n>m ago`, `<n>h ago`, `<n>d ago` dengan satuan terbesar dan dibulatkan ke bawah. `TZ` dihormati lewat jiff.

### Progress

- `Progress::new(stderr_is_tty, term_is_dumb, quiet)` memilih `Off` (`--quiet`), `Live` (stderr TTY dan `TERM` bukan `dumb`), atau `Lines` (selain itu).
- Progress menulis langsung ke handle stderr lewat `StderrSink`, bukan lewat `anstream`, karena `anstream` membuang semua escape termasuk kode hapus baris saat warna mati. Kode kursor yang dipakai hanya `\r` dan `ESC [ K`.
- Isi satu update: state, tahap (kalau ada), progress `done/total` kalau terhitung (tanpa persentase palsu untuk progress yang tidak terukur), dan lama menunggu.
- `Live`: satu baris `<state> <stage> <done>/<total> (<lama>)`, di escape lalu dipotong ke 60 kolom tampilan (ukuran terminal tidak dibaca, karena itu butuh `ioctl`), ditulis ulang dengan `\r` plus `ESC [ K`. Tanpa spinner. Sebelum `StderrSink` menulis apa pun yang lain (warning, baris `-v`, hint, error) dan sebelum output akhir, baris hidup dibersihkan; setelah tulisan lain, baris digambar ulang pada update berikutnya.
- `Lines`: satu baris `<lama> <state> <stage>` setiap kali state atau tahap berubah, tidak setiap tick progress.
- Sebelum menunggu, kecuali `--quiet`: `waiting for operation <id> (Ctrl+C stops waiting; the operation keeps running)`.

### Mode interaktif dan konfirmasi

- `interactive = stdin.is_terminal() && stderr.is_terminal() && !no_input_flag && !no_input_env` (`std::io::IsTerminal`). Dihitung sekali di awal dan disimpan di `Context`.
- `Context::prompt_allowed()` dipakai setiap kode yang ingin membaca input; kalau false, kode itu gagal `cli.input_required`. Spec 0011 langkah 8 memakainya untuk sumber TTY secret dan untuk menentukan `AllowPrompt`.
- `Confirm::ask(cx, ConfirmRequest { role, question, consequence }, Approval { given: bool, flag: &'static str }, OnNoninteractive)`: `Approval` datang dari flag perintah (default `ConfirmArgs`, `flag` `--yes`; spec 0011 memberi `--confirm-stored-only`). Persetujuan yang diberikan langsung lolos. Noninteractive tanpa persetujuan: dengan `OnNoninteractive::Fail` gagal `cli.confirmation_required` (`details.flag` dari `Approval.flag`), dengan `OnNoninteractive::LetDaemonDecide` lanjut tanpa persetujuan supaya daemon menjawab dengan kode konfirmasinya sendiri. Interaktif menulis `question`, `consequence`, dan `[y/N] ` ke stderr, membaca satu baris dari stdin, dan hanya `y` atau `yes` yang lolos. Role `Destructive` menulis pertanyaan dengan tone `critical`. Default selalu tidak, jadi Enter tidak pernah menjalankan aksi (sama dengan aturan Return di spec 0012).
- Fitur ini belum punya perintah yang memanggil `Confirm`; `operation cancel` tidak butuh konfirmasi (spec 0012: cancel operasi tanpa konfirmasi). Fitur 54 boleh menambah konfirmasi yang lebih berat (mengetik nama) di atas `Confirm`.

### Koneksi ke daemon

- `Session` dibuat lazy saat request pertama: `MacosPlatform` dari `env.rs`, `daemon_socket()`, `Client::connect` dengan `ClientInfo { kind: cli, version: CARGO_PKG_VERSION, pid }` dan batas 5 detik untuk connect plus handshake. Hasil handshake disimpan untuk blok `client` dan pemeriksaan drift.
- `Session::call(call, CallSpec { timeout: RequestTimeout, idempotency_key: Option<String> })` membuat `requestId` (UUID v7 lowercase) lebih dulu, menulis baris `-v` pertama, lalu memanggil `Client::call_detailed` dengan ID itu di bawah `tokio::time::timeout`. `RequestTimeout::Control` 10 detik untuk semua panggilan kecuali yang bisa menunggu dialog Keychain; `RequestTimeout::Interactive` 200 detik (180 detik antrean `SecretWorker` spec 0011 ditambah margin) dipilih perintah yang mengirim interaksi `AllowPrompt`. Lewat dari batas: `cli.request_timeout` dengan `requestId` yang sama di `CliFailure`, sehingga baris log daemon tetap bisa dicocokkan.
- Jawaban `daemon.starting` diulang dengan backoff 100 ms, 200 ms, 400 ms, 800 ms, lalu 1 detik, sampai 10 detik sejak jawaban starting pertama; setelah itu error `daemon.starting` terakhir dikembalikan (exit 6). Kode lain, termasuk yang `retryable`, tidak diulang.
- `ipc.endpoint_missing` dan `ipc.connection_refused` di teks manusia ditulis `daemon is not running (no socket at <path>)` dengan hint CLI `start it with stacklyd` sampai fitur 15 menyediakan `stackly daemon start`; `ErrorBody` di `--json` tetap apa adanya. CLI tidak pernah menjalankan daemon.
- `request.unknown_method` dari daemon lama (kategori `unsupported`, exit 5) mendapat hint CLI `update stacklyd to a version that supports <method>`, juga hanya di teks manusia.
- `ConnectPolicy` memakai uid dari `UserIdentity::current_user()` untuk pemilik path dan peer (spec 0006).
- Baris `-v`: `-> <method> <requestId>` sebelum mengirim, lalu `<- <ok|code|timeout> <ms> ms`.

### `stackly daemon status`

Menggantikan versi sementara spec 0006 (`// provisional: F8`). Urutan: `env.rs`, `Session`, `daemon.get_info`, `daemon.get_health`. Baris teks, satu fakta per baris, label lewat `kv`:

| Label | Isi |
| --- | --- |
| `Status` | `HealthStatus` dengan tone |
| `Version` | `<version> (protocol <major>.<minor>)` |
| `PID`, `UID` | dari `DaemonInfo`; `UID` dilewati kalau null |
| `Started` | waktu lokal dan umur dalam kurung |
| `Server ID` | `serverInstanceId` |
| `Subsystems` | hanya yang bukan `ok`: `<name>: <status>` dan `detail` dalam kurung, satu per baris |
| `Data`, `Logs`, `Cache`, `Runtime` | keempat root |
| `Socket` | `socketPath` |
| `Override` | `yes (STACKLY_ROOT)` atau `no` |

Warning `cli.root_override` dan `cli.contract_drift` seperti AC-6.

### `stackly operation list`, `inspect`, `cancel`

- `list`: `OperationListParams { activeOnly, resource, limit, before }` dari flag. `--resource` berformat `<kind>:<uuid>` atau `manager`, dengan kind yang diterima filter spec 0007 (saat spec ini ditulis `runtime`, `instance`, dan `environment`, sesuai resource operasi di spec 0004). Tabel: `ID` (UUID lengkap), `KIND`, `STATE`, `STAGE`, `PROGRESS` (`done/total` atau `-`), `RESOURCE` (`<kind>:<uuid>` atau `manager`), `AGE` (dari `createdAt`). Kalau `next` terisi, footer hint di stderr yang mengulang flag asli: `more operations: stackly operation list [--active] [--resource <r>] [--limit <n>] --before <next>`; tidak ditulis dengan `--json` (sudah ada di `result.next`) atau `--quiet`.
- `inspect`: `kv` untuk setiap field skalar `OperationDto` sesuai urutan DTO spec 0007, minimal `ID`, `Kind`, `Resource`, `State`, `Stage`, `Progress`, `Created`, `Started`, `Updated`, `Revision`, lalu `Error` (`<code>: <message>`) dan `Hint` kalau ada. Daftar bertingkat (misalnya langkah journal kalau spec 0007 menyediakannya) ditulis sebagai tabel berindentasi. Field yang tidak dikenal CLI hanya muncul di `--json`.
- `cancel`: memanggil `operation.cancel`, lalu menulis `cancel requested for operation <id> (state <state>)`; kalau DTO menyatakan langkah saat ini tidak bisa dibatalkan, ditambah satu baris yang menjelaskannya apa adanya (REQ-054). Tanpa konfirmasi.
- `--wait` di `inspect` dan `cancel` lewat `Waiter`.

### `--wait`

`Waiter::wait(cx, operation_id, &WaitArgs, mode)` dengan `mode` `Observe` (inspect dan perintah async) atau `Cancel`:

1. Daftarkan SIGINT dengan `tokio::signal::unix::signal(SignalKind::interrupt())` (terdaftar saat dipanggil, bukan saat pertama di poll), hitung `deadline` dari `--timeout` (kalau ada), lalu tulis baris `waiting for operation ...`. Urutan ini membuat SIGINT setelah baris itu selalu tertangkap.
2. Seluruh langkah 3 sampai 6 berjalan di dalam satu `tokio::select!` (dengan `biased`, cabang SIGINT dan `deadline` lebih dulu) melawan SIGINT dan `deadline`, termasuk connect, pengulangan `daemon.starting`, `operation.get` pertama, dan sambung ulang. Jadi batas waktu dan Ctrl+C tidak pernah tertahan oleh langkah lain.
3. Tersambung (lewat `Session`), lalu `events.subscribe { since: null, kinds: ["operation."], resource: { kind: operation, id } }` lebih dulu, baru `operation.get`, sesuai resep spec 0006.
4. Update progress dari DTO. Kalau state sudah akhir atau `needs_recovery`, selesai.
5. Loop atas event dari stream (event operasi itu, `events.gap`, `SkippedEvent`, atau `daemon.shutting_down`) dan tick setiap 5 detik. Event dan tick hanya menandai "perlu refresh"; paling banyak satu `operation.get` berjalan sekaligus, dengan jeda minimal 250 ms antara dua get, sehingga banjir event progress digabung menjadi satu get. DTO hanya dipakai kalau `revision` nya lebih besar dari yang dipegang. Snapshot adalah kebenaran, event hanya pemicu.
6. Penanganan error di langkah 3 sampai 5: koneksi putus (akhir `EventStream`, `ipc.connection_closed`, `ipc.endpoint_missing`, `ipc.connection_refused`) memicu sambung ulang dengan backoff 250 ms berlipat sampai 2 detik, paling lama 30 detik sejak putus, lalu kembali ke langkah 3; `cli.request_timeout` dan `daemon.starting` pada get diulang di dalam anggaran 30 detik yang sama tanpa memutus koneksi; error lain (misalnya `not_found` karena operasi sudah dipangkas retensi) mengakhiri penantian dengan error itu. Lewat 30 detik tanpa tersambung atau tanpa get yang berhasil: `cli.wait_disconnected`.
7. `deadline` lewat: `cli.wait_timeout`. SIGINT pertama: `cli.interrupted`. Keduanya membawa DTO terakhir sebagai `result` (`details.state` `unknown` kalau belum ada DTO), tidak pernah mengirim `operation.cancel`, dan langsung menulis output tanpa panggilan daemon lagi. SIGINT kedua selama penulisan itu keluar 130 seketika. Pesan teks Ctrl+C: `stopped waiting; operation <id> is still <state>` dengan hint `check it with: stackly operation inspect <id>`.

Pemetaan hasil:

| State akhir | `Observe` | `Cancel` |
| --- | --- | --- |
| `succeeded` | exit 0 | `cli.cancel_not_applied`, exit 3 |
| `failed` | `error` operasi (atau `cli.operation_failed`) | `cli.cancel_not_applied`, exit 3 |
| `cancelled` | `error` operasi (atau `cli.operation_cancelled`) | exit 0 |
| `needs_recovery` | `error` operasi (atau `cli.operation_needs_recovery`) | sama dengan `Observe` |

### Perintah async dan menambah command group

Pola untuk fitur 14 dan seterusnya:

1. Modul baru `apps/cli/src/commands/<group>.rs` dengan enum clap `Subcommand` untuk verb nya.
2. Satu varian di enum `Command` di `app.rs` yang mendelegasikan ke modul itu.
3. Setiap verb mengimplementasikan `RunCommand { type Output: Serialize + Render; async fn run(self, cx: &mut Context) -> CommandOutput<Self::Output>; }`. `CommandOutput<T> { result: Option<T>, failure: Option<CliFailure>, warnings: Vec<Warning> }` menentukan envelope dan exit code; perintah tidak pernah memanggil `std::process::exit` atau menulis ke stdout sendiri.
4. Perintah async memakai `#[command(flatten)] AsyncArgs` dan `cx.submit(call, &async_args, extract_operation_id)`, yang mengirim call dengan idempotency key (dari `--idempotency-key` atau UUID v7 baru), lalu tanpa `--wait` mengembalikan `{ operation, idempotencyKey }` dari reply (atau dari `operation.get` kalau reply hanya berisi ID), dan dengan `--wait` memanggil `Waiter` mode `Observe`. Teks tanpa `--wait`: `operation <id> <state>` dan hint `wait with: stackly operation inspect <id> --wait`. Kalau request async gagal `cli.request_timeout`, `details.idempotencyKey` terisi dan teks manusia menambah hint `retry safely with: --idempotency-key <key>`, karena request itu mungkin sudah berefek.
5. Perintah yang butuh konfirmasi memakai `ConfirmArgs` dan `Confirm`; perintah yang butuh secret memakai `secret_input` milik spec 0011.
6. Golden teks dan JSON untuk setiap verb di `apps/cli/tests/golden/`, dan kasus exit code di matriks daemon skrip.

`AsyncArgs` dan `Context::submit` ditetapkan di sini dan dibangun oleh perintah pertama yang memakainya (fitur 14), beserta test nya. Perintah sementara diberi `// provisional: F<n>` milik fitur penggantinya.

### Data model sketch

CLI tidak menyimpan data apa pun: tidak ada file config, cache, atau state di disk. Model nya adalah tipe output di `apps/cli`:

| Tipe | Field | Catatan |
| --- | --- | --- |
| `Envelope<'a, T>` | `schemaVersion: u32`, `type: "result"`, `command: &'a str`, `result: Option<&'a T>`, `error: Option<&'a ErrorBody>`, `requestId: Option<&'a str>`, `warnings: &'a [Warning]` | serde camelCase, urutan field tetap; `T` diserialisasi langsung (bukan lewat `serde_json::Value`, yang mengurutkan key) |
| `StreamItem<'a, T>` | `schemaVersion`, `type: "item"`, `command`, `item: &'a T` | dipakai pertama di fitur 27 |
| `Warning` | `code: String`, `message: String` | kode dari daftar warning tetap |
| `CliFailure` | `body: ErrorBody`, `request_id: Option<String>`, `exit_override: Option<u8>` | satu satunya bentuk error di CLI; `exit_override` hanya untuk `cli.interrupted` |
| `CommandOutput<T>` | `result: Option<T>`, `failure: Option<CliFailure>`, `warnings: Vec<Warning>` | `failure` None berarti exit 0 |
| `DaemonStatus` | `info: DaemonInfo`, `health: Option<DaemonHealth>`, `client: ClientContract { protocol, contractDigest, contractDrift }` | DTO dari spec 0006 |
| `OperationList` | `items: Vec<OperationDto>`, `next: Option<String>` | DTO dari spec 0007 |

### Asumsi ke spec lain

- **Assumed (spec 0007)**: method `operation.list(OperationListParams { activeOnly: bool, resource: Option<OperationResourceFilter>, limit: u16 (1 sampai 500), before: Option<String> }) -> OperationPage { items: Vec<OperationDto>, next: Option<String> }` dengan `next` berupa string cursor opaque yang diterima kembali apa adanya; `operation.get(OperationIdParams { operationId }) -> OperationDto`; `operation.cancel(OperationIdParams) -> OperationDto` (state setelah permintaan), dengan error berkategori `conflict` untuk operasi yang sudah berstate akhir; operasi yang tidak ada memakai kode berkategori `not_found` dengan `resource` operasi. `OperationDto` membawa `id`, `kind`, `resource`, `state` (nilai wire snake_case `queued`, `running`, `succeeded`, `failed`, `cancel_requested`, `cancelled`, `needs_recovery`), `stage`, `progress` (tidak terukur atau `{ done, total }`), `partial` untuk `succeeded`, `error: Option<ErrorBody>` (dengan `operationId`) yang terisi untuk `failed`, `cancelled`, dan `needs_recovery` (kategori `cancelled` untuk yang dibatalkan), penanda bahwa langkah saat ini tidak bisa dibatalkan, `createdAt`, `startedAt`, `updatedAt`, dan `revision`. Setiap perubahan state, tahap, atau progress mempublikasikan event `operation.*` lewat `publish_resource` dengan resource `operation`. Method mutasi menerima `Request.idempotencyKey`, dan key yang sama dengan params yang sama menghasilkan operasi yang sama. Reply mutasi async berisi paling tidak `operationId`. Untuk integration test CLI terhadap `stacklyd` sungguhan, spec 0007 menyediakan cara membuat operasi yang bisa dikendalikan (misalnya kind khusus test di belakang fitur cargo); kalau tidak ada, test daemon sungguhan fitur ini terbatas pada list kosong dan kasus `not_found`, dan matriks `--wait` penuh berjalan di daemon skrip.
- **Assumed (spec 0006)**: `Client` di `stackly-ipc` boleh mendapat tambahan `call_detailed(Call, CallOptions { request_id, idempotency_key })` tanpa mengubah wire; `Client::call` boleh berjalan selagi `EventStream` dari koneksi yang sama masih hidup (balasan dicocokkan lewat `requestId`); kalau ternyata tidak, `Waiter` memakai dua koneksi, satu untuk event dan satu untuk `operation.get`. `ResourceKind::Operation` bisa dipakai di filter `events.subscribe`; koneksi yang putus terlihat sebagai akhir `EventStream` atau `ipc.connection_closed`. Output dan exit code sementara `stackly daemon status` (langkah 20 spec 0006) diganti spec ini.
- **Assumed (spec 0011)**: langkah 8 spec 0011 membangun `secret_input` dan perintah `stackly secret` di atas `Context::prompt_allowed()`, `--no-input`, `STACKLY_NO_INPUT`, `Confirm` (role `Standard`, `Approval` dari `--confirm-stored-only`, dan `OnNoninteractive::LetDaemonDecide` supaya daemon menjawab `secret.confirmation_required`), `RequestTimeout::Interactive` untuk panggilan dengan `AllowPrompt`, dan envelope ini; `stackly secret reveal --json` menaruh `{ reference, value }` di `result`. Field wire yang membawa `interactive` ke daemon belum ada di `Request` spec 0006; pemiliknya spec 0006 dan 0011, dan CLI mengisinya dari aturan interaktif spec ini begitu field itu ada.
- **Assumed (spec 0004)**: `stackly` membaca `STACKLY_ROOT` sekali di `main` lewat `MacosPlatform::new` dengan aturan yang sama dengan daemon; `STACKLY_NO_INPUT` juga hanya dibaca bin. `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, `TERM`, dan `TZ` dibaca library pihak ketiga (`anstream`, jiff) di proses bin.
- **Assumed (spec 0012)**: arti lima tone dipakai juga untuk warna terminal, tanpa dependency ke kode Swift.
- **Assumed (spec 0005)**: tampilan waktu lokal untuk CLI (Follow-up spec 0005 untuk fitur 8) dipenuhi lewat `format_local` dan `format_age`; registry tetap UTC.

### API surface

CLI adalah permukaannya. Semua perintah bisa juga keluar 2 (clap atau `cli.invalid_environment`), 4 (`ipc.endpoint_untrusted`), 5 (`protocol.major_mismatch`), 6 (daemon tidak jalan atau `daemon.starting`), 8 (`cli.request_timeout`), dan 1 (`cli.panic`, `internal.unexpected`).

| Perintah | Method daemon | Input | `result` | Error dan exit utama |
| --- | --- | --- | --- | --- |
| `stackly daemon status` | `daemon.handshake`, `daemon.get_info`, `daemon.get_health` | flag global | `DaemonStatus` | status apa pun 0; parsial dengan kategori error `get_health` |
| `stackly operation list` | `operation.list` | `--active`, `--resource <kind>:<uuid>\|manager`, `--limit 1..500` (default 50), `--before <cursor>` | `OperationList` | 2 untuk flag salah |
| `stackly operation inspect <id>` | `operation.get`; dengan `--wait` juga `events.subscribe` | UUID lengkap, `WaitArgs` | `OperationDto` | 10 tidak ada; dengan `--wait` dari state akhir, 8 `cli.wait_timeout`, 130, 6 `cli.wait_disconnected` |
| `stackly operation cancel <id>` | `operation.cancel`; dengan `--wait` juga `events.subscribe`, `operation.get` | UUID lengkap, `WaitArgs` | `OperationDto` | 10 tidak ada; 3 sudah berakhir; dengan `--wait` 3 `cli.cancel_not_applied` |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| semua | exit code | `exit_code(ErrorBody.category)`; 130 hanya untuk `cli.interrupted` |
| semua | mode output | flag `--json` |
| semua | pilihan warna | `--color`, lalu `anstream` (TTY, `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, `TERM`) |
| semua | interaktif | `IsTerminal` stdin dan stderr, `--no-input`, `STACKLY_NO_INPUT` |
| semua | mode progress | `IsTerminal` stderr dan `--quiet` |
| semua | path socket | `MacosPlatform::new(STACKLY_ROOT)` lalu `daemon_socket()` (spec 0004) |
| semua | `ClientInfo` | konstanta `cli`, `CARGO_PKG_VERSION`, `std::process::id()` |
| semua | batas connect dan request | konstanta `CONNECT_TIMEOUT` 5 detik, `RequestTimeout::Control` 10 detik, dan `RequestTimeout::Interactive` 200 detik di `session.rs`; perintah memilih `RequestTimeout` per panggilan |
| semua | jadwal ulang starting | konstanta backoff 100 ms sampai 1 detik, total 10 detik |
| semua | `requestId` | `Uuid::now_v7()` lowercase dibuat `Session` sebelum mengirim, diteruskan lewat `CallOptions.request_id` |
| semua | uid untuk `ConnectPolicy` | `UserIdentity::current_user()` (spec 0004 dan 0006) |
| semua | urutan start | hook panic, pindaian argv (`--json`, `--color`), clap, environment, terminal |
| envelope | `schemaVersion` | `CLI_SCHEMA_VERSION` |
| envelope | `command` | path subcommand clap yang terpilih, dipisah spasi; kosong untuk `cli.usage` dan panic sebelum perintah terpilih |
| envelope | `error` | `ErrorBody` dari daemon tanpa diubah, atau `CliFailure` dari katalog `codes::cli` |
| envelope | `requestId` | `requestId` buatan `Session` untuk request yang gagal |
| envelope | `warnings` | warning yang dikumpulkan `Context` |
| error clap | `cli.usage` dan `details.kind` | peta eksplisit `clap::error::ErrorKind` ke snake_case dengan cadangan `other`; pesan dari baris pertama teks polos error clap tanpa awalan `error: ` dan titik akhir |
| exit | nilai akhir | `CliFailure.exit_override`, atau `exit_code(body.category)` |
| panic | lokasi untuk `-v` | `PanicHookInfo::location()` yang disimpan hook di `OnceLock` |
| teks | waktu lokal | `jiff::tz::TimeZone::system()` (menghormati `TZ`) |
| teks | umur | `jiff::Timestamp::now()` saat render dikurangi waktu DTO |
| teks | tone state dan warna | tabel tone di *Output manusia* (`anstyle::AnsiColor`) |
| teks | lebar kolom | `unicode-width` atas teks setelah escape |
| teks | sel atau nilai kosong | `-` |
| teks | durasi | dua satuan terbesar (`12s`, `3m04s`, `1h02m`) |
| teks | hint tambahan CLI | teks tetap per kode (`ipc.endpoint_missing`, `ipc.connection_refused`, `request.unknown_method`, `cli.request_timeout` dengan idempotency key) |
| progress | lebar baris hidup | konstanta 60 kolom tampilan |
| progress | mode | `IsTerminal` stderr, `TERM` (`dumb` berarti `Lines`), `--quiet` |
| `daemon status` | info, health | `daemon.get_info`, `daemon.get_health` |
| `daemon status` | blok `client` | `ProtocolVersion::CURRENT`, `CONTRACT_DIGEST`, `contractDrift` dari `HandshakeAccepted` |
| `daemon status` | warning override | `DaemonInfo.roots.rootOverride` dan path data root |
| `operation list` | params | flag, default `limit` 50 |
| `operation list` | footer | `OperationPage.next` dan flag asli perintah |
| `operation list` | kind yang diterima `--resource` | filter `operation.list` spec 0007 |
| `operation inspect`, `cancel` | `operationId` | argumen posisi, diparse `uuid::Uuid` |
| `--wait` | `deadline` | `--timeout` (bilangan detik atau jiff friendly `s`, `m`, `h`) ditambah waktu mulai menunggu, lewat `checked_add` |
| `--wait` | state, tahap, progress, revision | `operation.get` (spec 0007) |
| `--wait` | `details.state` sebelum DTO pertama | konstanta `unknown` |
| `--wait` | pemicu refresh | event `operation.*`, gap, skipped, shutting down, dan tick 5 detik, digabung dengan jeda minimal 250 ms |
| `--wait` | jadwal sambung ulang | backoff 250 ms sampai 2 detik, total 30 detik sejak putus |
| `--wait` | `error` akhir | `OperationDto.error`, atau kode cadangan `cli.operation_*`, atau `cli.cancel_not_applied` |
| perintah async | idempotency key | `--idempotency-key`, atau `Uuid::now_v7()`; dikembalikan di `result.idempotencyKey` |
| konfirmasi | jawaban | satu baris dari stdin (TTY dalam mode interaktif) |
| konfirmasi | flag persetujuan | `Approval` dari perintah pemanggil, default `--yes` |
| `-v` | baris request | method dari `Call`, `requestId` buatan `Session`, `Instant` sebelum dan sesudah |

### Key invariants

- Exit code tidak pernah bergantung pada teks pesan atau kode; hanya kategori, kecuali 130 untuk `cli.interrupted`.
- Dengan `--json`, stdout berisi record JSON saja; progress, warning teks, prompt, dan hint tidak pernah masuk stdout.
- `error` null kalau dan hanya kalau exit 0, dan `ErrorBody` dari daemon tidak pernah diubah atau ditambah CLI.
- CLI tidak pernah menjalankan proses, menulis file, membuat folder, membaca registry, atau menyalakan daemon; semua efek terjadi di daemon.
- Mode noninteractive tidak pernah membaca stdin untuk prompt atau konfirmasi, dan tidak pernah menunggu input.
- Batas waktu, Ctrl+C, dan koneksi putus saat menunggu tidak pernah membatalkan operasi.
- Teks dari daemon tidak pernah sampai ke terminal dengan karakter kontrol mentah.
- Tidak ada flag yang menerima nilai secret, dan `-v` tidak pernah mencatat params atau isi reply.

### Security model

- Batas kepercayaan satu akun user OS (BR-012). CLI berjalan sebagai user itu dan tidak punya hak lebih.
- Endpoint diverifikasi (path, pemilik, peer) sebelum satu byte dikirim, lewat `stackly-ipc` (spec 0006, AC-070).
- CLI tidak pernah menjalankan proses (BR-011), dijaga `check-deps` AC-11.
- Secret tidak pernah lewat argv (spec 0011); nilai yang di echo clap di pesan error hanya berasal dari argumen biasa.
- Teks daemon di escape sebelum ke terminal, sehingga path, pesan provider, atau nama yang dibuat orang lain tidak bisa menyuntik escape terminal atau membalik teks dengan bidi.
- Pesan error dan hint dari daemon sudah lewat redaksi spec 0011; CLI tidak menambah nilai input ke pesan buatannya sendiri.
- Panic tidak pernah mencetak isi pesannya, karena bisa memuat data.

### Configuration required

- `STACKLY_ROOT` (sudah ada, spec 0004): dibaca `stackly` sekali saat start, aturan sama dengan daemon.
- `STACKLY_NO_INPUT` (baru, opsional): `1` atau `true` menyalakan mode noninteractive; tidak di set, kosong, `0`, atau `false` mematikan; nilai lain ditolak.
- `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, `TERM`, `TZ`: konvensi standar yang dihormati lewat `anstream` dan jiff, tanpa kode baca sendiri.

### Critical test scenarios

Test integrasi menjalankan binary `stackly` (lewat `cargo_bin` testkit) dengan `STACKLY_ROOT` ke `TestRoot`. Daemon skrip (`apps/cli/tests/support/scripted_daemon.rs`, di atas `stackly_ipc::Listener` dan DTO protokol, bind di `daemon_socket()` root test) menjawab handshake lalu mengikuti skrip per method: reply, error, jeda, putus koneksi, event. Perilaku TTY diuji lewat helper pty yang menjalankan `/usr/bin/script -q /dev/null /bin/sh -c '<binary> <argumen> >"$OUT"; echo $? >"$STATUS"'` dengan stdin `Stdio::null()` atau pty sesuai kasus: stderr (dan stdin bila perlu) adalah pty nyata, stdout ditulis ke file di root test sehingga konversi `\n` ke `\r\n` milik pty tidak mengotori pemeriksaan stdout, dan exit code dibaca dari file, tidak bergantung pada exit `script`. Daemon skrip menerima kode apa pun, termasuk kode sintetis seperti `test.failed`, karena CLI tidak memvalidasi kode terhadap katalognya. Golden di `apps/cli/tests/golden/`, ditulis ulang dengan `STACKLY_UPDATE_GOLDEN=1`, dengan waktu dan umur dinormalkan di harness (format waktu diuji di unit test dengan `now` dan zona waktu tetap).

- Matriks exit code: untuk setiap `ErrorCategory`, daemon skrip menjawab `daemon.get_info` dengan kode berkategori itu, dan `stackly daemon status` (teks dan `--json`) keluar sesuai tabel; dua kode sekategori dengan pesan beda keluar sama; pesan sama dengan kategori beda keluar beda; `--help` dan `--version` 0 (juga dengan `--json`, berupa teks), argumen salah 2. Memverifikasi **AC-1**.
- JSON bersih: setiap perintah dengan `--json` di bawah helper pty (stderr TTY), `--color always`, dan progress aktif menghasilkan file stdout satu baris tanpa byte 0x1B atau `\r` yang lolos parse dengan field berurutan; `error` sama dengan yang dikirim daemon skrip, tanpa hint tambahan CLI, dan `requestId` cocok dengan yang dicatat daemon skrip; argumen salah dengan `--json` (dan `--json` setelah `--` yang tidak dihitung) menghasilkan envelope `cli.usage` dengan `command` kosong dan `details.kind` yang benar. Memverifikasi **AC-2**.
- Teks: golden tabel dan `kv`; nama Unicode lebar ganda tetap rata; `--color never`, `NO_COLOR=1`, dan stdout bukan TTY tanpa ANSI; `--color always` dan `CLICOLOR_FORCE=1` dengan ANSI; nilai berisi `\x1b[31m`, `\u{202e}`, dan newline ditulis sebagai escape; unit test `format_local` dan `format_age` dengan zona `Asia/Jakarta` dan `UTC`. Memverifikasi **AC-3**.
- Progress: `operation inspect --wait` di helper pty menulis baris `\r` plus `ESC [ K` di stderr (juga dengan `--color never` dan `NO_COLOR=1`) dan file stdout tetap hanya hasil; `TERM=dumb` dan stderr bukan TTY menulis satu baris per perubahan tahap; baris `-v` dan warning tidak tercampur dengan sisa baris hidup; `--quiet` tidak menulis progress. Memverifikasi **AC-4**.
- Noninteractive: `STACKLY_NO_INPUT` dengan nilai `1`, `TRUE`, `0`, kosong, dan `yes` (ditolak); unit test `Confirm` dengan input yang disuntikkan untuk `y`, `YES`, Enter, EOF, `n`, flag persetujuan default dan khusus, `OnNoninteractive::Fail` dan `LetDaemonDecide`; `stackly operation cancel <id> --yes` ditolak clap (exit 2); prompt dalam mode noninteractive gagal seketika walau stdin terbuka. Memverifikasi **AC-5**.
- Status: terhadap `stacklyd` sungguhan (`SpawnedDaemon`), teks dan `--json` dengan warning `cli.root_override`; daemon skrip untuk digest berbeda (warning drift), `get_health` gagal (parsial), dan major berbeda (exit 5); daemon mati exit 6 dengan path socket; endpoint palsu exit 4; daftar isi root test sama sebelum dan sesudah. Memverifikasi **AC-6**.
- Operasi: daemon skrip untuk list dengan `next`, list kosong, flag `--resource` dan `--limit` sampai ke params, inspect, cancel yang diterima, cancel pada operasi berakhir (3), dan ID tidak ada (10); ID bukan UUID ditolak clap (2); terhadap `stacklyd` sungguhan, list kosong dan inspect ID acak keluar 10, ditambah jalur penuh kalau spec 0007 menyediakan operasi test. Memverifikasi **AC-7**.
- Wait: daemon skrip memutar state `queued`, `running`, lalu masing masing akhir untuk `inspect --wait` dan `cancel --wait` dengan tabel pemetaan; `--timeout 1s` keluar 8 dalam sekitar 1 detik, juga saat daemon skrip menahan handshake atau menjawab `daemon.starting` terus, dan log skrip tidak memuat `operation.cancel`; SIGINT lewat `/bin/kill -INT <pid>` setelah baris `waiting for operation` muncul di stderr keluar 130 tanpa `operation.cancel`, juga saat connect masih tertahan; DTO tanpa `error` memakai kode cadangan; seratus event progress dalam satu detik menghasilkan paling banyak lima `operation.get`. Memverifikasi **AC-8**.
- Ketahanan: daemon skrip yang diam lebih dari 10 detik memberi `cli.request_timeout` (8) dengan `requestId` yang sama dengan baris `-v`; `daemon.starting` tiga kali lalu sukses memberi exit 0, dan terus starting memberi exit 6 setelah sekitar 10 detik; `request.too_many_in_flight` tidak diulang; saat `--wait`, `operation.get` yang diam sekali diulang tanpa sambung ulang, daemon skrip yang menutup koneksi lalu kembali dengan `serverInstanceId` baru membuat wait selesai normal, dan daemon yang tidak kembali memberi `cli.wait_disconnected` (6); unit test `Session` memastikan `RequestTimeout::Interactive` memakai 200 detik dengan jam tokio yang dijeda. Memverifikasi **AC-9**.
- Robust: unit test bahwa panic yang disuntikkan (di thread utama dan di task tokio) menjadi exit 1 tanpa isi pesan, dengan envelope `cli.panic` dalam mode `--json`; `stackly operation list | head -c 1` terhadap daftar panjang tidak menulis error dan exit 0; stdout ke file di folder tanpa izin tulis tidak mengubah exit perintah dan menulis `cli.output_failed` ke stderr. Memverifikasi **AC-10**.
- BR-011: crate fixture dengan `use std::process::Command;`, `std::process::Command::new`, `use std::process as p;` lalu `p::Command::new`, `use std::process::*;`, `tokio::process::Command`, `println!`, dependency `libc` dan `duct`, serta `codes::cli` di crate selain CLI dan protokol membuat `check-deps` gagal dengan file dan baris; `std::process::exit`, `std::process::ExitCode`, `clap::Command`, dan dependency dev `libc` lolos. Memverifikasi **AC-11**, **AC-14**.
- Pola group: review di `/check verify` bahwa `daemon` dan `operation` hanya menambah file di `commands/` dan satu varian `Command`. Memverifikasi **AC-12**.
- Bantuan dan verbose: `--help` memuat tabel exit code dan variabel environment; setiap perintah punya contoh; `--version` sesuai format; `-v` menulis baris kirim sebelum balasan dan baris selesai sesudahnya, tidak memuat penanda params (`"params-sentinel"`) yang dikirim test, dan tetap jalan bersama `-q`. Memverifikasi **AC-13**.
- Katalog: test katalog spec 0006 lulus dengan `codes::cli`; `cargo xtask codegen --check` lulus setelah minor naik. Memverifikasi **AC-14**.

Test yang membuktikan REQ-028, AC-028 (bagian output dan exit code), dan BR-011 memakai guard evidence spec 0002 dengan label `fixture`. Bukti AC-028 penuh (operasi yang setara dari GUI dan CLI) menunggu create instance di fitur 20.

## Build plan

Urutan mengikuti pendekatan Journey. Jalur yang dibangun lengkap adalah "script atau pengguna menjalankan `stackly` dan selalu mendapat kontrak yang sama", dimulai dari perintah yang sudah punya daemon nyata (`daemon status`) dengan semua state gagal nya, lalu jalur operasi dan `--wait`. Setiap test yang membuktikan ID PRD memakai guard evidence spec 0002.

Sesi paralel: di Milestone 1, langkah 2 (`check-deps`, hanya `xtask/` dan crate fixture nya) bisa berjalan sebagai sesi terpisah bersamaan dengan langkah lain. Langkah 1 kecil dan dikerjakan lebih dulu di sesi utama karena langkah 3 sampai 8 memakai `codes::cli`. Milestone 2 butuh Milestone 1 dan langkahnya berurutan karena menyentuh file yang sama.

**Milestone 1: Kerangka CLI dan `stackly daemon status`** (`crates/protocol`, `crates/ipc`, `apps/cli`, `xtask`; butuh milestone 1, 2, dan 4 spec 0006 sudah dibangun)

1. `codes::cli` dengan semua kode di tabel, area `cli` di `AREAS`, test katalog, `cargo xtask codegen` (`error-codes.json`, digest, minor `CURRENT` naik, `contract-versions.json`). Memenuhi **AC-14**, **AC-1**.
2. Aturan `check-deps` AC-11: pindaian `std::process` (hanya `exit`, `ExitCode`, `id`; alias modul dan glob ditolak), `tokio::process`, `std::os::unix::process`, dan makro `print!`, `println!`, `eprint!`, `eprintln!` di `apps/cli` non test; daftar dependency normal yang dilarang untuk `stackly-cli`; larangan menyebut `codes::cli` di luar `apps/cli` dan `crates/protocol`; crate fixture yang melanggar setiap aturan. Memenuhi **AC-11**, **AC-14**.
3. Kerangka `apps/cli`: dependency baru (termasuk fitur clap `string`), `panic.rs` dengan hook sendiri, `app.rs` dengan pindaian argv, flag global, enum `Command`, dan peta `ErrorKind`, `env.rs`, `exit.rs`, `failure.rs` dengan `exit_override`, `main.rs` dengan `catch_unwind` dan `ExitCode`, envelope `cli.usage` untuk error clap dengan `--json`, `--version`, dan help dengan variabel environment dan tabel exit code. Memenuhi **AC-1**, **AC-2**, **AC-5**, **AC-10**, **AC-13**.
4. Lapisan output: `Envelope<T>`, `StreamItem<T>`, `Warning`, writer satu baris dengan broken pipe dan warning `cli.output_failed`, `StderrSink`, `HumanWriter` (`kv`, `table`, state bertone, `-` untuk kosong), `escape_display`, `style`, `time` (waktu lokal, umur, durasi), penulisan error, hint CLI, dan warning, `--quiet`; unit test. Memenuhi **AC-2**, **AC-3**, **AC-10**.
5. `Progress` (`Live` lewat `StderrSink` dengan potongan 60 kolom, `Lines`, `Off`) dan `Confirm` (`Approval`, `OnNoninteractive`, baca stdin) dengan input dan terminal yang bisa disuntikkan, `Context::prompt_allowed`, `ConfirmArgs`; unit test. Memenuhi **AC-4**, **AC-5**.
6. `Client::call_detailed` dengan `CallOptions` di `stackly-ipc` (tambahan API) dan `Session`: connect lazy 5 detik, `RequestTimeout` per panggilan, `requestId` buatan CLI, ulang `daemon.starting`, baris `-v`, hint CLI untuk daemon tidak jalan dan method tidak dikenal. Memenuhi **AC-9**, **AC-13**.
7. `stackly daemon status` versi akhir: menggantikan versi sementara spec 0006, hasil parsial, warning `cli.root_override` dan `cli.contract_drift`. Memenuhi **AC-6**, **AC-12**.
8. Dukungan test dan test integrasi: `scripted_daemon.rs` (kode error sintetis, pencatat `requestId` dan method), helper menjalankan binary, helper pty `/usr/bin/script` dengan stdout ke file dan exit code dari file, harness golden; matriks exit code, JSON bersih, teks, status terhadap `stacklyd` sungguhan, ketahanan request, evidence REQ-028, AC-028, dan BR-011. Memenuhi **AC-1** sampai **AC-6**, **AC-9**, **AC-10**, **AC-13**.

**Milestone 2: `stackly operation` dan `--wait`** (`apps/cli`; butuh Milestone 1 dan DTO serta method `operation.*` spec 0007 di `stackly-protocol`; bagian test terhadap `stacklyd` sungguhan butuh engine spec 0007 di daemon)

9. `args.rs` (`OperationIdArg`, `ResourceFilter`, `CursorArg`, durasi), `Render` untuk `OperationDto` dan `OperationList`, pemetaan tone state operasi. Memenuhi **AC-3**, **AC-7**.
10. `operation list`, `inspect`, dan `cancel` tanpa `--wait`. Memenuhi **AC-7**, **AC-12**.
11. `Waiter` dan `WaitArgs`: SIGINT terdaftar sebelum baris waiting, satu `select!` dengan `deadline` dan SIGINT yang membungkus seluruh penantian, subscribe lalu get, satu get berjalan dengan jeda minimal 250 ms, tick 5 detik, klasifikasi error (putus, diulang, akhir), sambung ulang 30 detik, progress, pemetaan `Observe` dan `Cancel`, kode cadangan, dan `state` `unknown`. Memenuhi **AC-4**, **AC-8**, **AC-9**.
12. Test integrasi: matriks `--wait` di daemon skrip (semua state akhir, timeout, SIGINT, putus lalu kembali, putus terus, starting), golden operasi, terhadap `stacklyd` sungguhan (list kosong, `not_found`, dan jalur penuh lewat operasi test spec 0007 kalau ada), evidence. Memenuhi **AC-7**, **AC-8**, **AC-9**, **AC-12**.

## Consequences

**Positive**:

- Script punya satu tempat parse (satu baris di stdout) untuk sukses dan gagal, dan exit code yang tidak berubah saat pesan atau tampilan diperbaiki.
- Fitur 11, 14, 15, 20, dan seterusnya menambah perintah tanpa memutuskan ulang output, warna, prompt, konfirmasi, atau exit code.
- Ctrl+C, batas waktu, dan restart daemon saat menunggu tidak pernah membatalkan atau merusak operasi, dan selalu meninggalkan operation ID untuk diperiksa.
- `not_found` yang terpisah membuat script `inspect || create` bisa membedakan "belum ada" dari input salah, tanpa membaca JSON.
- Escape karakter kontrol menutup terminal injection dari path atau pesan provider, dan pindaian `check-deps` membuat BR-011 dijaga compiler, bukan hanya review.
- Golden JSON dan matriks exit code membuat setiap perubahan kontrak terlihat di diff.

**Negative / tradeoffs**:

- Envelope membuat path jq lebih panjang (`.result.id`, bukan `.id`) dibanding output DTO mentah.
- Hasil yang menyisipkan DTO protokol membuat CLI ikut aturan versi protokol: perubahan breaking di DTO juga breaking untuk script, dan `CLI_SCHEMA_VERSION` harus naik bersama.
- Exit 10 dan 130 berada di luar tabel PRD bagian 12; dokumentasi CLI harus menjelaskannya, dan 130 adalah satu satunya exit yang tidak diturunkan dari kategori.
- `--wait` tanpa batas default bisa menunggu lama kalau operasi macet di daemon; script di CI sebaiknya selalu memberi `--timeout`.
- Kode `cli.*` di katalog protokol menaikkan minor protokol walau tidak pernah lewat wire, dan ikut bentrok `CURRENT` dengan cabang paralel lain saat merge.
- Daemon skrip adalah kode test yang meniru daemon; ia bisa menyimpang dari daemon sungguhan, jadi jalur utama tetap diuji terhadap `stacklyd`.
- Test pty bergantung pada `/usr/bin/script` macOS, dan jeda waktu nyata (timeout 1 detik, starting 10 detik, putus 30 detik) membuat sebagian test integrasi lambat.
- Teks manusia berbahasa Inggris dan tidak dilokalisasi; hanya waktu yang mengikuti zona waktu sistem.
- Tabel tanpa pemotongan bisa membungkus baris di terminal sempit karena UUID lengkap.
- Baris progress hidup dipotong tetap di 60 kolom karena lebar terminal tidak dibaca (butuh `ioctl`); terminal yang lebih sempit dari itu bisa menyisakan potongan baris.
- Pemindaian `check-deps` berbasis nama, jadi crate pihak ketiga yang menjalankan proses secara tidak langsung hanya tertangkap lewat daftar dependency yang dilarang dan review.
- `ErrorBody` di `--json` hanya memuat field yang dikenal versi protokol client, jadi field tambahan dari daemon dengan minor lebih baru tidak terlihat sampai CLI diperbarui.

**Neutral**:

- Dependency baru di `stackly-cli`: `anstream` dan `anstyle` (sudah ada di lock lewat clap), `unicode-width`, `jiff`, `uuid`, `serde`, fitur tokio `time`, `sync`, `signal`.
- `stackly-ipc` mendapat `Client::call_detailed` tanpa perubahan wire.
- `AsyncArgs` dan `Context::submit` baru dibangun di fitur 14, saat perintah async pertama ada.
- Completion shell dan man page tetap milik fitur 15.

## Follow-up

- [ ] `/architect` update spec 0001 (lewat sesi rekonsiliasi): tandai baris "Kontrak output CLI dan peta exit code" di *Batas keputusan* sudah diputuskan di spec 0008; *Peta crate*: `stackly-cli` mendapat `anstream`, `anstyle`, `unicode-width`, `jiff`, `uuid`, `serde`, dan fitur tokio `time`, `sync`, `signal`, serta `[workspace.dependencies]` mendapat `anstream`, `anstyle`, dan `unicode-width`; *Aturan arah dependency*: `stackly-cli` tidak boleh bergantung pada `libc` atau `nix`, dan kode non test `apps/cli` tidak boleh menyebut path spawn proses (`check-deps`, AC-11); baris Error di tabel stack: tabel exit code lengkap ada di spec 0008 (`not_found` 10, `internal` 1, Ctrl+C 130).
- [ ] `/architect` update spec 0004 (lewat sesi rekonsiliasi): Follow-up "Fitur 8: `stackly` membaca `STACKLY_ROOT` sekali" dipenuhi spec 0006 dan 0008, centang setelah Milestone 1 dibangun; konsekuensi "Fitur 8 harus menampilkan root aktif" dipenuhi warning `cli.root_override`; catat `STACKLY_NO_INPUT` sebagai variabel kedua yang dibaca bin `stackly`, dan bahwa `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, `TERM`, dan `TZ` dibaca library pihak ketiga di proses bin.
- [ ] Spec 0006 (pemiliknya): catat `Client::call_detailed` dengan `CallOptions { request_id, idempotency_key }` di *Transport `stackly-ipc`*, pastikan `Client::call` boleh berjalan selagi `EventStream` hidup, pertimbangkan mengecualikan area `cli` dari digest kontrak (kodenya tidak pernah lewat wire, jadi tidak perlu menaikkan minor protokol), dan centang Follow-up "Spec 0008 menetapkan exit code `not_found` dan `internal`" setelah Milestone 1 dibangun.
- [ ] Spec 0007: pastikan Assumed spec ini (method, DTO, event, idempotency, operasi test) cocok dengan spec 0007; kalau berbeda, update spec ini lewat `/architect`.
- [ ] Spec 0011: langkah 8 memakai `Context::prompt_allowed`, `Confirm` dengan `Approval` dari `--confirm-stored-only` dan `OnNoninteractive::LetDaemonDecide`, `RequestTimeout::Interactive` untuk panggilan dengan `AllowPrompt`, dan envelope; `stackly secret reveal --json` menaruh `{ reference, value }` di `result` envelope. Field wire `interactive` yang diasumsikan spec 0011 belum ada di `Request` spec 0006 dan perlu diputuskan pemiliknya.
- [ ] Fitur 14: bangun `AsyncArgs` dan `Context::submit` bersama perintah `instance create`, `start`, dan `stop` sementara, mengikuti *Perintah async dan menambah command group*.
- [ ] Fitur 15: allowlist satu modul di aturan `check-deps` AC-11 kalau `stackly daemon start` perlu memanggil `launchctl`; completion shell (`clap_complete`) dan man page (`clap_mangen`); ganti hint `start it with stacklyd`.
- [ ] Fitur 20: bukti AC-028 penuh (create instance dari GUI dan `--json` CLI dengan validasi dan error yang sama).
- [ ] Fitur 27: perintah streaming pertama memakai record `type: item`.
- [ ] Fitur 54: konfirmasi purge yang lebih berat di atas `Confirm`, sesuai scope "purge di mode noninteractive tetap butuh konfirmasi eksplisit".
- [ ] Fitur 38 atau 66: dokumentasi referensi CLI (envelope, exit code, variabel environment), dan pertimbangkan JSON Schema envelope di `schemas/cli/`.
- [ ] Fitur 64: ukur waktu start `stackly` dan latensi `daemon status` di mesin baseline.
- [ ] `/sync` setelah build: catat di root `AGENTS.md` tabel exit code, envelope `--json`, `STACKLY_NO_INPUT`, aturan `check-deps` CLI, dan pola command group; tambah `anstream`, `anstyle`, dan `unicode-width` ke baris `Declined:` untuk Agent Skills (ditolak pengguna 2026-10-18).

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, dan keputusan kecil yang diambil saat menulis spec ada di [rationale.md](rationale.md).
