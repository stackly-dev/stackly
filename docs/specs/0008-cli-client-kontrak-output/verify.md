# Verify: CLI client & kontrak output · spec 0008 · updated 2026-10-20

_Langkah diturunkan dari acceptance criteria spec 0008. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah, Milestone 1 dan 2. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27. Probe yang mengubah file sementara selalu dikembalikan sesudahnya (`git status` bersih lagi)._

_Angka test di bawah diambil dari run 2026-10-20 di cabang `claude/dbg-f8-m2-ba9bdb` (di atas main `eea005d`), sesudah ketiga test `stacklyd` yang sebelumnya `#[ignore]` diperbaiki dan lulus. Kotak belum dicentang: `/check verify` yang mencentangnya._

## Milestone 1: CLI framework and stackly daemon status end to end

Langkah build 1 sampai 8. Test integrasi menjalankan binary `stackly` lewat `cargo_bin` dengan `STACKLY_ROOT` ke `TestRoot`; daemon skrip ada di `apps/cli/tests/common/scripted.rs`, helper pty (`/usr/bin/script`) di `apps/cli/tests/common/pty.rs`, dan golden di `apps/cli/tests/golden/`.

### UI / manual

- [ ] Di satu terminal: `export STACKLY_ROOT=$(mktemp -d)` lalu `cargo run -q -p stacklyd`; di terminal lain dengan `STACKLY_ROOT` yang sama: `cargo run -q -p stackly-cli -- daemon status` → satu fakta per baris: status `ready`, pid, uid, versi, protokol `1.6`, server instance, waktu start di zona waktu lokal, keempat root, path socket, dan override aktif; warning `cli.root_override` yang menyebut path nya di stderr; exit 0 → AC-3, AC-6
- [ ] Dengan daemon yang sama: `cargo run -q -p stackly-cli -- daemon status --json` → tepat satu baris di stdout dengan field berurutan `schemaVersion` (1), `type` (`result`), `command` (`daemon status`), `result` (`info`, `health`, `client` berisi `protocol`, `contractDigest`, dan `contractDrift: false`), `error` null, `requestId`, dan `warnings` berisi `cli.root_override`; exit 0 → AC-2, AC-6
- [ ] `cargo run -q -p stackly-cli -- daemon status --json --color always | LC_ALL=C grep -c $'\e'` → `0` (tidak ada byte ESC walau warna dipaksa) → AC-2
- [ ] `cargo run -q -p stackly-cli -- --help` → flag global, variabel environment (`STACKLY_ROOT`, `STACKLY_NO_INPUT`, `NO_COLOR`, `CLICOLOR_FORCE`), dan tabel `Exit codes:` 0 sampai 10 plus 130; `stackly daemon status --help` punya bagian `Examples:`; `stackly --version` → `stackly 0.1.0 (protocol 1.6)`; ketiganya exit 0, juga dengan `--json` → AC-1, AC-2, AC-13
- [ ] `STACKLY_NO_INPUT=yes cargo run -q -p stackly-cli -- daemon status` → `error: the value of STACKLY_NO_INPUT is refused: use 1 or true to turn it on, 0 or false to turn it off` dengan `code: cli.invalid_environment`, exit 2 → AC-5
- [ ] Ctrl+C di terminal daemon, lalu `cargo run -q -p stackly-cli -- daemon status` → `error: daemon is not running (no socket at <path>)`, `code: ipc.endpoint_missing`, `hint: start it with stacklyd`, exit 6; `ls $STACKLY_ROOT` sama dengan sebelum perintah (CLI tidak membuat folder atau file) → AC-6
- [ ] Buka `apps/cli/src/commands/` dan enum `Command` di `apps/cli/src/app.rs` → group `daemon` adalah satu modul `commands/daemon.rs` dan satu varian `Daemon`, hasilnya `Serialize` plus `Render` → AC-12 (bagian `daemon`)

### Commands

- [ ] `cargo test -p stackly-protocol --lib codes::` → 20 test lulus, termasuk `cli_codes_match_the_spec_0008_table` (setiap kode `cli.*` dengan kategori, `retryable` false, dan kunci `details` tabel spec) dan `every_area_is_registered_with_its_owning_spec` (area `cli` milik spec 0008) → AC-14
- [ ] `cargo xtask codegen --check` → semua file generate mutakhir; `protocols/error-codes.json` memuat kode `cli.*` → AC-14
- [ ] `cargo test -p xtask check_deps` → 50 test lulus, termasuk `every_process_spawning_path_in_the_cli_is_reported_with_its_line`, `print_macros_in_the_cli_are_reported_also_inside_other_macros`, `codes_cli_outside_the_cli_and_the_protocol_is_reported`, `the_cli_never_takes_a_process_spawning_crate_as_a_normal_dependency`, dan `a_fixture_workspace_that_breaks_the_cli_rules_fails_with_file_and_line` → AC-11, AC-14
- [ ] `cargo xtask check-deps` → lulus → AC-11
- [ ] Tambah sementara `pub fn probe() { let _ = std::process::Command::new("true"); }` di akhir `apps/cli/src/main.rs`, jalankan `cargo xtask check-deps` → gagal dengan `stackly-cli apps/cli/src/main.rs:<baris>: the cli never spawns processes (spec 0008, BR-011)`; kembalikan dengan `git checkout -- apps/cli/src/main.rs` → AC-11
- [ ] `cargo test -p stackly-cli --bin stackly` → 78 unit test lulus: `panic::` 4 (hook tanpa isi pesan, lokasi hanya dengan `-v`, panic di task menjadi `cli.panic`), `output::` 29 (envelope, escape, style, waktu dan umur, `StderrSink`, `HumanWriter`), `progress::` 5, `confirm::` 5, `env::` 3, `session::` 5 (`RequestTimeout::Interactive` 200 detik pada jam yang dijeda), `app::` 7, `exit::` 2, `failure::` 5, `context::` 2 (prompt noninteractive gagal seketika walau stdin terbuka), dan 11 unit test Milestone 2 → AC-1, AC-2, AC-3, AC-4, AC-5, AC-9, AC-10, AC-13
- [ ] `cargo test -p stackly-cli --test contract` → 24 test lulus terhadap daemon skrip → AC-1, AC-2, AC-3, AC-5, AC-6, AC-9, AC-10, AC-13
  - `exit_codes::` 4: setiap kategori keluar sesuai tabel di teks dan `--json` apa pun kodenya, pesan dan kode tidak pernah mengubah exit, `--help` dan `--version` teks dengan exit 0, argumen yang ditolak exit 2 atau envelope `cli.usage` → AC-1, AC-2
  - `json::` 3: `--json` di terminal dengan `--color always` satu baris bersih membawa error daemon apa adanya, error sisi CLI tanpa `requestId` dan tanpa hint CLI di body, sukses dengan `error` null dan urutan field → AC-2
  - `text::` 3: satu fakta per baris dalam waktu lokal, warna hanya di terminal (auto) atau saat dipaksa, karakter kontrol dan override bidi di escape → AC-3
  - `noninteractive::` 2: `STACKLY_NO_INPUT` menerima `1`, `true`, `0`, kosong dan menolak nilai lain, handshake `interactive` hanya di terminal tanpa `--no-input` → AC-5
  - `status::` 4: drift kontrak dan root override sebagai warning, `get_health` gagal memberi info dengan `health` null dan exit kategorinya, major lain exit 5, recovery mode → AC-6
  - `resilience::` 5: request tanpa jawaban timeout 10 detik dengan `requestId` sendiri, handshake 5 detik, `daemon.starting` diulang sampai siap, terus starting exit 6 setelah sekitar 10 detik, kode retryable lain tidak diulang → AC-9
  - `robust::` 2: pembaca yang pergi mengakhiri output tanpa pesan dengan exit perintah, stdout yang gagal ditulis memberi satu baris warning dan exit tetap → AC-10
  - `verbose::` 1: `-v` menulis satu baris sebelum setiap request dan satu sesudahnya tanpa isi reply → AC-13
- [ ] `cargo test -p stackly-cli --test daemon_status` → 8 test lulus terhadap `stacklyd` sungguhan (`SpawnedDaemon`): fakta teks dan `--json` tanpa membuat apa pun, daemon mati exit 6 dengan path socket, endpoint bukan socket exit 4, daemon degraded tetap exit 0, setelah daemon kedua ditolak status menyebut yang pertama, override yang ditolak exit 2, handshake diam 5 detik dan call diam 10 detik exit 8 → AC-6, AC-9

### Acceptance-criteria coverage

- AC-1 · `exit_code` dari kategori lewat `exit::` dan `failure::`; matriks setiap kategori dan kode sintetis di `contract exit_codes::`; 130 untuk `cli.interrupted` dibuktikan di Milestone 2 (`wait::ctrl_c_stops_waiting_with_exit_130_and_leaves_the_operation_running`)
- AC-2 · envelope satu baris, urutan field, error daemon apa adanya, `cli.usage` untuk argumen yang ditolak, tanpa ESC atau `\r` di bawah pty; golden `daemon_status.json`
- AC-3 · tabel dan `kv` lewat golden, warna per mode, escape, waktu lokal; golden operasi di Milestone 2
- AC-4 · bagian unit (`progress::`); baris hidup di pty dan baris polos di luar TTY dibuktikan di Milestone 2 (`wait::progress_on_a_terminal_is_one_live_line_and_stdout_keeps_only_the_result`, `wait::without_a_terminal_or_with_term_dumb_progress_is_one_plain_line_per_change`)
- AC-5 · `STACKLY_NO_INPUT`, `Confirm` dengan input yang disuntikkan, handshake `interactive`; `cli.confirmation_required` untuk abandon di Milestone 2
- AC-6 · lengkap: `stacklyd` sungguhan dan daemon skrip
- AC-9 · connect 5 detik, `Control` 10 detik, `Interactive` 200 detik (unit), ulang `daemon.starting`; sambung ulang saat `--wait` di Milestone 2
- AC-10 · hook panic dan `catch_unwind` (unit), broken pipe dan stdout gagal (integrasi)
- AC-11 · aturan `check-deps` dengan crate fixture, plus probe di atas
- AC-12 · bagian `daemon`; bagian `operation` di Milestone 2
- AC-13 · `--help`, contoh per perintah, `--version`, baris `-v`
- AC-14 · katalog `codes::cli` dan `error-codes.json`
- REQ-028, AC-028 (bagian output dan exit code), BR-011 · evidence `fixture` dari test di atas; AC-028 penuh (GUI dan CLI setara) menunggu fitur 20
- AC-7, AC-8, AC-15 · milik Milestone 2

## Milestone 2: stackly operation and --wait

Langkah build 9 sampai 12. Test daemon skrip ada di `apps/cli/tests/operation/`. Jalur penuh terhadap `stacklyd` sungguhan ada di `apps/daemon/tests/cli.rs`: server, engine, dan registry jalan di dalam proses pada `TestRoot`, method test `test.start_operation` (`Handlers::test_starting_method`, `provisional: F10`) memulai operasi `ScriptedWork`, dan binary `stackly` yang asli bicara lewat socket.

Catatan stage: `ScriptedWork` tidak pernah memanggil `OperationContext::set_stage` (spec 0007, *`ScriptedWork`* tidak punya modifier stage), jadi operasi di test `stacklyd` sungguhan selalu punya `stage` null dan baris progress nya `<lama> <state>`. Nama langkah journal bukan stage. Tampilan stage dibuktikan di daemon skrip (`wait::without_a_terminal_or_with_term_dumb_progress_is_one_plain_line_per_change` memutar stage `fetch` lalu `unpack`).

### UI / manual

- [ ] Dengan daemon dari Milestone 1 berjalan: `cargo run -q -p stackly-cli -- operation list` → `No operations.`, exit 0; `--json` → `result` `{"items":[],"next":null}` dengan `error` null → AC-7
- [ ] `cargo run -q -p stackly-cli -- operation inspect <UUID v7 acak>` → `error: the operation does not exist` dengan `code: resource.not_found`, exit 10; sama untuk `cancel`, `resume`, dan `abandon --yes` → AC-7
- [ ] `cargo run -q -p stackly-cli -- operation inspect abc` → clap menolak dengan `invalid value 'abc' for '<ID>': expected a whole operation id, ...`, exit 2; `stackly operation cancel <UUID> --yes` → `unexpected argument '--yes' found`, exit 2 (hanya `abandon` yang punya `--yes`) → AC-5, AC-7
- [ ] `cargo run -q -p stackly-cli -- operation abandon <UUID> --json </dev/null` → exit 2, envelope dengan `error.code` `cli.confirmation_required`, `details` `flag` `--yes`, dan `hint` `pass --yes to confirm`; tidak ada request ke daemon → AC-5, AC-15
- [ ] `cargo run -q -p stackly-cli -- operation inspect <UUID> --wait --timeout 1s` → stderr diawali `waiting for operation <UUID> (Ctrl+C stops waiting; the operation keeps running)`, lalu `resource.not_found` exit 10 (error yang bukan putus koneksi mengakhiri penantian dengan error itu) → AC-8, AC-9
- [ ] `git show --stat bf5d98f` → group `operation` adalah satu modul `commands/operation.rs` dan satu varian `Operation` di `Command`; `args.rs` dan `wait.rs` adalah modul baru langkah 9 dan 11; perubahan di `output/human.rs`, `output/stderr.rs`, `session.rs`, dan `confirm.rs` hanya atribut `expect(dead_code)` dan penghapusan helper yang tidak dipakai, tanpa perubahan perilaku → AC-12
- Jalur penuh (operasi berjalan, cancel, resume, abandon) terhadap `stacklyd` belum bisa dicoba dengan tangan: method kontrak pertama yang memulai operasi datang di fitur 10. Jalur itu dibuktikan lewat `cargo test -p stacklyd --test cli` di bawah.

### Commands

- [ ] `cargo test -p stackly-cli --bin stackly -- args:: wait:: commands::operation::` → 11 unit test lulus: parser `OperationIdArg`, `ResourceFilter`, `CursorArg`, dan durasi, pemetaan `Observe` dan `Cancel` ke exit, state yang mengakhiri penantian, klasifikasi error wait (putus, diulang, akhir), anggaran ulang 250 ms berlipat sampai 2 detik dan habis setelah 30 detik, footer `list` yang hanya mengulang flag yang diberikan, dan resource dibaca sebagai `<kind>:<uuid>` atau `manager` → AC-7, AC-8, AC-9
- [ ] `cargo test -p stackly-cli --test operation commands::` → 6 test lulus: list dengan tabel, footer, dan flag sebagai params, flag salah ditolak sebelum mengirim, inspect dengan langkah dan item dibuat (exit 0 untuk operasi gagal), cancel dengan `disposition` dan exit 3 saat tidak berlaku, resume mengirim revision dari `operation.get`, abandon sebagai konfirmasi destructive yang menampilkan `leftovers`; golden `operation_*.txt` dan `operation_*.json` → AC-3, AC-5, AC-7, AC-15
- [ ] `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-cli --test operation commands::` lalu `git status --short apps/cli/tests/golden` → kosong (golden mutakhir) → AC-3
- [ ] `cargo test -p stackly-cli --test operation wait::` → 11 test lulus (sekitar 30 detik, ada jeda waktu nyata): `inspect --wait` memutar `queued`, `running`, lalu setiap state akhir dengan exit nya, `cancel --wait` exit 0 hanya untuk `cancelled`, `--timeout 1s` exit 8 tanpa cancel, Ctrl+C exit 130 dan operasi tetap berjalan, seratus event progress dalam satu detik memberi paling banyak lima get, daemon yang kembali dengan server instance baru, daemon yang tidak kembali exit 6 setelah 30 detik, get diam atau `daemon.starting` diulang di koneksi yang sama, error bukan putus mengakhiri penantian, baris hidup di pty dengan stdout hanya hasil, satu baris polos per perubahan di luar TTY atau dengan `TERM=dumb` → AC-1 (130), AC-4, AC-8, AC-9
- [ ] `cargo test -p stackly-cli --test operation stacklyd::` → 2 test lulus terhadap `stacklyd` sungguhan: list kosong dengan setiap filter tanpa membuat apa pun, setiap verb pada ID tidak dikenal exit 10 dengan `not_found` milik daemon → AC-7
- [ ] `cargo test -p stacklyd --test cli` → 6 test lulus, tanpa `#[ignore]` (`grep -c '#\[ignore' apps/daemon/tests/cli.rs` → `0`) → AC-7, AC-8, AC-15
  - `a_running_operation_is_listed_inspected_and_waited_for_and_stopping_the_wait_never_cancels_it`: list, `--active`, tabel, dan inspect menunjukkan operasi `running` dengan `stage` null dan langkah `prepare` done serta `wait` running; `inspect --wait --timeout 1s` exit 8 dalam 5 detik dengan `cli.wait_timeout`; Ctrl+C sesudah baris ` running` exit 130 dengan `cli.interrupted`, operasi tetap `running`, dan daemon tidak mencatat `operation.cancel`; `inspect --wait` exit 0 saat `succeeded`; cancel sesudahnya exit 3 `cli.cancel_not_applied` dengan `already_final`
  - `cancel_wait_undoes_an_operation_before_its_point_of_no_return_and_exits_0`: `--resource instance:<id>` memfilter, `cancel --wait` exit 0 dengan `requested` lalu `cancelled`, pohon instance dihapus, `inspect --wait` exit 9 `cli.operation_cancelled`
  - `cancel_wait_past_the_point_of_no_return_ends_succeeded_and_exits_3`: cancel di dalam langkah yang tidak bisa dibalik menjawab `continuing_past_point_of_no_return`, penantian menampilkan ` cancel_requested`, lalu exit 3 `cli.cancel_not_applied` dengan state `succeeded` dan `cancelRequestedAt` terisi
  - `a_failed_operation_exits_0_to_inspect_and_by_its_errors_category_to_inspect_wait`: inspect exit 0, `inspect --wait` exit 7 dengan error operasi apa adanya (`provider.render_failed`)
  - `resume_sends_the_revision_it_read_and_waits_for_the_resumed_operation`: `inspect --wait` pada `needs_recovery` exit 1 dari kategori `internal.unexpected`, teks inspect menulis `Recovery ... resume after step_failed` dan `Actions ... resume, abandon`, `resume --wait` exit 0 dengan `succeeded` dan langkah dicoba dua kali, resume kedua exit 3 `operation.not_resumable` (`wrong_state`)
  - `abandon_needs_yes_noninteractive_then_closes_the_operation_with_its_leftovers`: abandon tanpa `--yes` exit 2 tanpa request `operation.abandon` dan revision tetap, resume exit 3, cancel exit 1 dengan `recovery_required`, `abandon --yes` exit 0 dengan `failed` dan `operation.abandoned` beserta `leftovers`, abandon kedua exit 3 `operation.not_abandonable`, `inspect --wait` exit 7
- [ ] `cargo xtask evidence check` → lulus (tidak ada `#[ignore]` selain test native) → spec 0002

### Acceptance-criteria coverage

- AC-3 · render operasi lewat golden `operation_*`, state sebagai teks wire (`needs_recovery`, `step_failed`) dengan tone
- AC-4 · baris hidup 60 kolom dengan `\r` plus `ESC [ K` di pty, baris polos per perubahan state atau stage di luar TTY; stage hanya dari daemon skrip (lihat catatan stage)
- AC-7 · daemon skrip untuk list, filter, cursor, inspect, cancel, dan ID tidak ada; `stacklyd` sungguhan untuk list kosong, `not_found`, dan jalur penuh lewat `test.start_operation`
- AC-8 · matriks wait di daemon skrip dan `stacklyd` sungguhan: setiap state akhir, `needs_recovery`, timeout, Ctrl+C tanpa `operation.cancel`, kode cadangan, `state` `unknown` sebelum DTO pertama
- AC-9 · sambung ulang 30 detik, daemon baru dengan server instance berbeda, get yang diulang di koneksi yang sama
- AC-12 · group `operation` lewat satu modul dan satu varian
- AC-15 · resume dan abandon mengirim `expectedRevision` dari `operation.get` (daemon skrip memeriksanya; `stacklyd` sungguhan menerimanya), abandon destructive dengan `--yes`, `leftovers` di teks dan `--json`
- REQ-028, REQ-054, AC-054 (bagian CLI), AC-028 · evidence `fixture` dari `apps/daemon/tests/cli.rs` dan `apps/cli/tests/operation/`
