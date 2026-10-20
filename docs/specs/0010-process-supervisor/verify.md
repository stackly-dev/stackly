# Verify: Process supervisor · spec 0010 · updated 2026-10-20

_Langkah diturunkan dari acceptance criteria spec 0010. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah; Milestone 3 dan 4 ditambah saat dibangun. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Test yang menjalankan `stackly-fake-server` membangunnya sendiri lewat `cargo build` pertama kali (beberapa detik). Semua test memakai `TestRoot`, `allocate_port`, dan kredensial fixture; tidak ada yang menyentuh folder Application Support asli._

_Status awal: tidak ada langkah di bawah yang dicentang; `/check verify` mencentangnya. Satu baris Milestone 1 `BLOCKED` sejak awal: Boot setelah reboot, karena butuh reboot Mac ini dan pemiliknya pengguna (lihat *Hasil spike* di spec)._

## Milestone 1: Process port, registry ops, and macOS adapter

Langkah 1, 2, 2a, 3, dan 4 di Build plan (sudah di `main`): spike, port `ProcessSupervision` beserta tipe domainnya, op registry supervisor, adapter macOS, dan `FakeProcessSupervision`.

### UI / manual

- [x] Buka `evidence/manual/2026-10-18-spec-0010-spike-*.json` → lima belas record spike (spawn tertahan, `getsid`, boot ID, flock yang diwariskan, `kqueue`, `PROC_PIDTBSDINFO` untuk proses sendiri dan milik user lain, `proc_pidpath`, anggota group, sinyal dan reap, spool `O_RDONLY` yang ditolak dan spool `O_RDWR` dengan satuan halaman, cek fd lewat device dan inode) masing masing dengan hasil yang sama dengan *Hasil spike* di spec → AC-1, AC-2, AC-9, AC-12 (bukti kelayakan)
- [x] Buka `crates/platform-macos/src/process/` → `unsafe` hanya di crate ini dan tiap blok punya `// SAFETY:`; token `SIGKILL` hanya di `kill.rs`; deklarasi lokal di `sys.rs` punya `const` assert ukuran dan offset; tidak ada `std::process::Command` atau `tokio::process` di `crates/core` dan `apps/` → AC-1, AC-5, AC-15
- [ ] **BLOCKED** (pemilik: pengguna, butuh reboot Mac ini) Baris Boot setelah reboot: `kern.bootsessionuuid` berbeda setelah reboot dan observasi dari boot lama menjadi `Gone(boot_changed)`. Spike menyiapkan record nya; tidak bisa dijalankan sampai Mac ini di reboot → AC-9, AC-11 (bagian boot)

### Commands

- [x] `cargo test -p stackly-platform --lib process` → 9 unit test lulus: permintaan spawn utama harus berperan `Main` dengan stdin null dan output spool, permintaan pembantu menolak peran `Main`, output spool, dan lock untuk peran yang tidak boleh, NUL di argv atau env ditolak dengan posisi, `SpoolWatermark` hanya membebaskan satuan penuh, pesan `ProcessError` huruf kecil → AC-1, AC-5, AC-6
- [x] `cargo test -p stackly-domain process` → test `process` domain lulus (`Pid`, `ProcessStart` dengan mikrodetik, `BootId`, `IdentityEvidence` dengan nilai `NotChecked`, format teks kolom observasi) → AC-2, AC-10
- [x] `cargo test -p stackly-platform-macos --test process` → 16 integration test lulus dengan dummy server asli: proses utama tertahan dengan argv, env, dan cwd persis dari plan, dengan folder `Test Root Ünï cödé` dan metakarakter shell tanpa file yang dibuat tafsiran shell, dan hanya fd 0 sampai 3; env tertutup dan output ke spool; child yang dibuang tidak pernah berjalan dan lock nya bebas; child tertahan yang mati sendiri dibuang dengan statusnya tanpa sinyal; binary tanpa tanda tangan yang dibunuh OS tetap di spawn dan dibuang; fallback `kinfo_proc` sama dengan `PROC_PIDTBSDINFO`; sinyal hanya sampai ke identitas yang dituju; proses user lain (PID 1) punya fakta dari `kinfo_proc` dan tidak pernah disinyal; pembantu membaca stdin dan ekor outputnya ditangkap; group pembantu yang mengabaikan `SIGTERM` berhenti hanya lewat force; proses yang bukan child diawasi tanpa di reap; lock instance eksklusif, file biasa, dan bebas kalau tidak ada; pembaca spool hanya membebaskan halaman penuh; boot ID dari `kern.bootsessionuuid`; permintaan buruk ditolak sebelum apa pun berjalan; `MacosPlatform` melaporkan `process_supervision` supported → AC-1, AC-2, AC-5, AC-6, AC-15, AC-19
- [x] `cargo test -p stackly-storage --test registry supervision` → 5 test lulus: observasi proses disimpan dalam teks kolom spec 0005, config revision mengisi kolom instance tanpa menaikkan revision, nilai config yang bukan objek JSON ditolak, baris observasi yang rusak dibaca `Corrupt`, status config yang tidak dikenal dibaca `Corrupt` → AC-2, AC-9, AC-11 (prasyarat registry)
- [x] `cargo test -p stackly-testkit --test fake_registry` → 10 test lulus, termasuk konformitas `registry_conformance` untuk op supervisor di `FakeRegistry` → AC-2, AC-9, AC-11 (prasyarat registry)
- [x] `cargo test -p stackly-testkit --test fake_process` → 17 test lulus: PID buatan dan fakta yang bisa diubah, sinyal ditolak untuk identitas yang berbeda, lock dan spool nyata di `TestRoot`, satuan punch yang bisa disetel, catatan setiap panggilan, dan kesalahan yang bisa diinjeksi per method → AC-6, AC-7 (prasyarat test core)
- [x] `cargo check --locked --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → exit 0 → AC-19
- [x] `cargo xtask check-deps` → lulus: tidak ada `std::process::Command` atau `tokio::process` di kode non test `stackly-core` dan `stacklyd`, token `SIGKILL` hanya di `stackly-platform-macos/src/process/kill.rs` → AC-5, AC-15

### Acceptance-criteria coverage

- AC-1 · lengkap di adapter: flag spawn, argv dan env persis, fd 0 sampai 3, path Unicode dan metakarakter; bukti ujung ke ujung lewat daemon menunggu Milestone 4
- AC-2 · bagian adapter dan registry: spawn tertahan, `discard`, observasi dalam satu batch; urutan startup transaction yang memakainya ada di Milestone 2
- AC-5 · bagian adapter: `signal` memeriksa ulang identitas dan tidak ada `SIGKILL` ke proses yang berjalan; alur stop di Milestone 2
- AC-6 · bagian adapter: lock instance eksklusif dan diwariskan; penolakan `supervisor.data_in_use` di Milestone 2
- AC-15 · bagian adapter: group pembantu TERM lalu KILL; aturan di sisi core di Milestone 2
- AC-19 · lengkap: capability supported di macOS, `NotSupported` di `UnsupportedProcessSupervision`, cek Windows
- AC-9, AC-11 · hanya prasyarat (registry, boot ID); baris Boot setelah reboot `BLOCKED`; adopsi dan rekonsiliasi menunggu Milestone 3
- AC-3, AC-4, AC-7, AC-8, AC-10, AC-12 sampai AC-14, AC-16 sampai AC-18, AC-20 sampai AC-23 · belum, milik Milestone 2 sampai 4

## Milestone 2: Start to ready and safe stop on the fake provider

Langkah 5 sampai 8 di Build plan: logika murni (`classify`, restart policy, `SupervisorConfig`, `PortProbe`, aturan commit), runner probe, executor (`execute_init`, startup transaction, stop, alokasi port), readiness dan health rutin, `statusSeq`, dan `execute_reload`. Test core memakai `FakePlatform` (`FakeProcessSupervision`, `FakeRegistry`, `FakeSecretStore`), paket `providers/fake` yang di pin, dan `stackly-fake-server` asli sebagai penjawab probe.

### UI / manual

- [x] Buka `crates/core/src/supervisor/startup.rs` → tahap berurutan `validate`, `lock`, `verify`, `endpoints`, `journal`, `render`, `spawn`, `identity`, `readiness`, `publish`, `complete`; dry run `run_plan` dan `ensure_not_in_argv` ada di `verify` sebelum `journal`; `spawn` dan sesudahnya `irreversible()`; setelah `spawn_main` tidak ada titik cancel sebelum `resume` kecuali yang mem `discard` child dulu; interaksi Keychain hanya untuk percobaan pertama → AC-2, AC-3, AC-18
- [x] Buka `crates/core/src/supervisor/stop.rs` → tidak ada token `SIGKILL`; `stop_process` hanya memanggil `signal` dengan `SignalTarget` (identitas diperiksa ulang) atau pembantu `StopCommand`; `IdentityMismatch` menjadi `identity_lost` dan `process.signal_refused`; timeout meninggalkan lifecycle `stopping` dan proses tetap diawasi → AC-5
- [x] Buka `crates/core/src/supervisor/init.rs` → `InstanceLock` dilepas sebelum error dikembalikan (nilai Arc dilepas saat `init_locked` kembali), `Stdin::Text` lewat `StdinSource::Bytes(SecretValue::new(text))` dan kegagalannya menjadi `init_step_failed`, dan semua segmen secret di materialize sebelum efek samping pertama → AC-13
- [x] Buka `crates/core/src/supervisor/readiness.rs` → probe readiness setiap `interval` sampai berhasil tanpa deadline sendiri, lalu setiap `readiness_refresh_interval` dengan debounce `readiness_failure_threshold`; `holds_file` dipanggil sekali saat readiness pertama; tidak ada panggilan ke `signal`, `spawn`, atau `commit_instance` dari probe → AC-4

### Commands

- [x] `cargo test -p stackly-core --lib supervisor::classify` → 17 test lulus: tabel `classify` lengkap, `holds_file` tidak dipanggil bila enam pemeriksaan lain gagal, error di input mana pun menjadi `Unproven` → AC-10
- [x] `cargo test -p stackly-core --lib supervisor::restart` → 13 test lulus: jeda 1, 2, 4, 8, dan 16 detik dengan jitter 0,5 sampai 1,0, crash keenam dalam jendela tidak dijalankan ulang, jendela bergeser, reset setelah 60 detik `ready` → AC-7
- [x] `cargo test -p stackly-core --lib supervisor::commit` → 8 test lulus: satu `UpdateInstance` per mutasi dengan `expected` berurutan, retry aktor (baca ulang, `Busy`, `NoSpace` menjadi degraded) → AC-22
- [x] `cargo test -p stackly-core --lib supervisor::ports` → 10 test lulus: `PortProbe::try_bind` dengan `SO_REUSEADDR` seperti server, listener di alamat persis dihitung dipakai dan listener wildcard dihitung bebas, `Denied` dan `Unavailable` → AC-17
- [x] `cargo test -p stackly-core --lib supervisor::probe` → 27 test lulus dan `cargo test -p stackly-core --lib supervisor::http` → 7 test lulus: runner `tcp`, `http`, dan `exec` dengan `ProbeOutcome` (`probe_ok`, `connect_failed`, `timeout`, `protocol_error`, `unexpected_status`, `check_failed`), detail tersamar lalu dipotong 256 karakter, aturan pembantu TERM lalu KILL setelah `HELPER_TERM_GRACE` → AC-4, AC-15
- [x] `cargo test -p stackly-core --test supervisor_probe` → 2 test lulus terhadap dummy server asli: `/ready` 503 lalu 200, status di luar `expect_status` → AC-4
- [x] `cargo test -p stackly-core --lib supervisor::error` → 4 test lulus: setiap kode ada sekali dan berawalan `supervisor.` atau `process.`, kunci `details` sesuai tabel *Kode error*, pesan huruf kecil tanpa titik → AC-3, AC-5, AC-8, AC-17
- [x] `cargo test -p stackly-core --lib supervisor::status` → 2 test lulus: perubahan `observed_at` saja bukan perubahan yang terlihat, state atau reason baru adalah perubahan → AC-4
- [x] `cargo test -p stackly-core --test supervisor_start init::` → 5 test lulus: `execute_init` menjalankan plan fake dengan `--token-stdin` dan secret di stdin, lock di fd 3, `instance-manifest.json` ditulis ulang dengan `data.initializedAt`, config ter render, lifecycle `stopped`; langkah yang keluar dengan kode lain menjadi `init_step_failed` dengan exit code dan lock sudah bebas begitu error kembali; langkah yang melewati timeout menerima TERM lalu KILL dan `init_step_timeout`; init yang melewati `initSeconds` menjadi `init_timeout`; secret hilang menjadi `secret.needs_secret` tanpa efek samping dan tanpa perubahan lifecycle → AC-13, AC-15, AC-18
- [x] `cargo test -p stackly-core --test supervisor_start start::` → 20 test lulus: start sampai ready dengan sebelas tahap berurutan, lifecycle `starting` lalu `running`, `bound` final hanya setelah ready, observasi tercatat sebelum proses berjalan, argv dan env persis plan, lock di fd 3, spool; `readyDelayMs` terlihat `running` dengan readiness `not_ready`; sebelas start menghasilkan satu spawn; lock dipegang proses lain dan observasi terakhir yang masih hidup menolak start tanpa spawn (`supervisor.data_in_use`); port `Fixed` dipakai listener luar (`supervisor.endpoint_in_use`, listener tetap hidup), `Denied` (`supervisor.endpoint_unavailable` sebelum `journal`), port `Auto` dipakai ulang atau diganti; paket berubah, marker beda, folder 0755, runtime tidak `installed`, executable keluar dari root runtime, dan secret hilang masing masing gagal dengan kodenya sebelum spawn; crash loop enam spawn lalu `supervisor.retry_budget_exhausted` dengan `crash_loop` dan tanpa dua proses bersamaan; port direbut setelah exit gagal tanpa respawn; deadline readiness habis (`supervisor.readiness_timeout`) tetapi proses dan probe jalan terus sampai `ready`; cancel dengan proses hidup menjadi stop graceful lalu `cancelled`, cancel di jeda backoff menjadi `stopped` tanpa spawn kedua; commit identitas yang gagal membuang child tertahan lewat `discard` tanpa pernah `resume`; spawn yang ditolak OS; hanya percobaan pertama yang boleh dialog Keychain → AC-2, AC-3, AC-6, AC-7, AC-8, AC-17, AC-18
- [x] `cargo test -p stackly-core --test supervisor_start stop::` → 5 test lulus: stop hanya `Term`, menunggu exit, menghapus material secret, tidak ada `force` ke apa pun; timeout (`supervisor.stop_timeout`) membiarkan lifecycle `stopping` dan stop kedua mengirim ulang, lalu exit sendiri menghasilkan `stopped` lewat handler yang sama; identitas yang diubah menghasilkan `process.signal_refused`, tanpa sinyal, instance `unknown` yang menolak start dan stop; anggota group tersisa dilaporkan dan tidak disinyal; stop pada `stopped` dengan desired `running` hanya mengubah desired → AC-5
- [x] `cargo test -p stackly-core --test supervisor_start status::` → 5 test lulus: dua probe gagal menjadi `not_ready` tanpa sinyal atau spawn dan pulih saat server kembali, `statusSeq` mulai 1 tanpa celah dan tanpa pengulangan, nilai lama menjadi `unknown` reason `stale` setelah jam maju 31 detik, health `healthy` lalu `degraded` lalu `unhealthy` pada interval sendiri, instance baru `unknown` reason `daemon_restarted` → AC-4
- [x] `cargo test -p stackly-core --test supervisor_start reload::` → 3 test lulus: `execute_reload` merender ulang dari config revision baru, mengirim `SIGHUP` ke proses terverifikasi, PID tidak berubah dan tidak ada spawn baru (dummy server asli menerima `SIGHUP` lalu `GET /config` memberi greeting baru); paket tanpa capability reload menghasilkan `supervisor.capability_unsupported` tanpa sinyal; reload pada instance yang tidak berjalan gagal → AC-14
- [x] `cargo test -p stackly-core` → semua lulus; `cargo clippy -p stackly-core --all-targets --locked -- -D warnings` dan `cargo fmt --all --check` bersih → AC-19
- [x] `cargo check --locked --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → exit 0 (probe ke socket Unix di core berada di balik `cfg(unix)`) → AC-19

### Acceptance-criteria coverage

- AC-2 · lengkap di sisi core: identitas ditangkap saat tertahan, lifecycle `spawned`, port `Auto`, dan observasi dalam satu batch sebelum `SIGCONT`, `discard` bila gagal; kasus child yang mati sendiri saat tertahan menunggu pemasangan `classify_early_exit` (spec 0018 langkah 8, K7)
- AC-3 · tahap, urutan, dan lifecycle di core; batas 500 ms (NFR-006) menunggu Milestone 4 karena butuh daemon
- AC-4 · readiness dan health terpisah, probe ulang, debounce, `stale`, `statusSeq`; pemetaan ke DTO `instance.get`, `instance.list`, dan event menunggu langkah 12 dan 13
- AC-5 · lengkap di core: strategi provider saja, identitas diperiksa ulang, tanpa `SIGKILL`, timeout di `stopping`, handler exit tunggal, material secret dihapus, sisa group dilaporkan; stop dengan perintah (`StopPlan::Command`) dan timeout perintah belum punya test sendiri
- AC-6 · lock instance dan penjaga observasi terakhir, dan sebelas start satu spawn di core; sepuluh start bersamaan dari dua client menunggu Milestone 4
- AC-7 · logika murni (langkah 5) dan jalur retry di dalam operasi start; timer jeda tanpa operasi, operasi sistem, dan reset setelah sehat menunggu langkah 9; jalur crash di luar operasi sementara berakhir `failed` (`// provisional: F10 step 9` di `exit.rs`)
- AC-8 · sukses, `supervisor.retry_budget_exhausted`, `supervisor.readiness_timeout`, dan cancel di titik proses hidup dan jeda; cancel sebelum `journal` dijamin kode (tidak ada efek samping sebelum `journal`) tetapi belum punya test timing sendiri
- AC-10 · hanya logika `classify` (langkah 5); penerapan di rekonsiliasi menunggu Milestone 3
- AC-13 · bagian core dengan `FakeProcessSupervision`: stdin, lock, file secret, timeout, `init_timeout`, `initializedAt`; `token.sha256` yang ditulis fake sama dengan SHA-256 fixture butuh proses nyata dan dibuktikan di Milestone 4
- AC-14 · reload di core dengan dummy server asli; bukti dengan provider nyata milik fitur 26 dan 19
- AC-15 · aturan pembantu di core (probe `exec` dan langkah init); perintah stop dan reload memakai fungsi yang sama
- AC-17 · `Fixed` dipakai (`endpoint_in_use`), `Denied` (`endpoint_unavailable`), `Auto` dipakai ulang atau diganti, port direbut setelah exit tanpa budget, dan konflik dengan instance lain lewat `endpoints_conflict`; `Denied` sesudah exit dini jatuh ke restart policy
- AC-18 · setiap kegagalan `verify` punya kode dan needs secret menjadi blocker tanpa perubahan lifecycle; interaksi Keychain `Never` untuk respawn; `check_launchable` (quarantine) menunggu langkah 8 spec 0018
- AC-19 · sama seperti Milestone 1, kini dengan seluruh `stackly-core` supervisor
- AC-22 · bagian logika aturan commit (langkah 5) dan jalur `CommitMode::Actor`; test commit tertunda dengan `FakeRegistry` yang `NoSpace` menunggu langkah 11
- AC-1, AC-9, AC-11, AC-12, AC-16, AC-20, AC-21, AC-23 · belum, milik Milestone 3 dan 4

## Milestone 3: Crash, spool, and reconciliation

### UI / manual

- [ ] Langkah 10 tidak punya permukaan UI atau langkah manual: spool hanya terlihat lewat `LogSink` dan `Supervisor::failure_excerpt`, bukan lewat CLI atau app (pemasangan `LogSink` daemon dan `lastFailure.excerpt` di `stacklyd` menyusul di langkah IPC) → AC-12

### Commands

- [ ] `cargo test -p stackly-core --test supervisor_spool` → 16 test lulus di atas spool nyata di `TestRoot` dengan pembaca spool fake: akhiran baris `\r\n` dibuang dan byte bukan UTF-8 menjadi U+FFFD; baris yang terpotong antar pembacaan ditahan sampai newline datang (chunk 5 byte); `read_at` batch memakai `Clock` daemon; nilai secret yang melintasi batas potongan 16 KiB pada baris 40 KiB tersamar sebelum dipotong dan tidak ada potongan yang memuat sebagian secret; baris di atas `LOG_LINE_HARD_MAX` keluar sebagai potongan `partial` dengan sisanya `continued`; sink `Busy` menahan batch tanpa membebaskan byte lalu batch yang sama terkirim sekali; `release_below` memakai ujung baris terakhir yang diterima (baris yang ditahan tidak pernah dibebaskan) dibulatkan ke satuan punch; simulasi 121 tick kali 1000 baris (lebih dari 6 MB) berakhir dengan `first_data_offset()` sama dengan offset yang dibebaskan di setiap tick; backlog di atas batas dibuang dari yang tertua dengan `truncated` sehingga byte yang dibuang ditambah byte yang terkirim sama dengan ukuran file; `resume` dari offset di atas 0 membuang byte sampai newline pertama dan batch pertama `resumed`, sedangkan dari offset 0 baris pertama tetap ada; `finish` mengirim baris tanpa newline sebagai `partial`; kegagalan `release_below` tidak menghentikan baris; `SinkSet` dengan `TailSink` → AC-12
- [ ] `cargo test -p stackly-core --test supervisor_start spool` → 4 test lulus dengan supervisor penuh di atas `FakeProcessSupervision`: baris stdout dan stderr yang ditulis proses yang berjalan sampai ke sink dan `TailSink` dengan secret yang dikenal tersamar (`****`) dan `failure_excerpt` mengurutkan stderr dulu; exit membaca sisa spool termasuk baris tanpa newline sebagai `partial` walau pembaca sedang tidur 60 detik; sink `Busy` tidak menahan exit dan lifecycle tetap `failed`; spool lama dibaca ke sink (batch `resumed`) sebelum run berikutnya dan tail run sebelumnya dilupakan → AC-12
- [ ] `cargo test -p stackly-core --lib supervisor::` → unit test `TailSink` (hanya `TAIL_LINES` terbaru, cuplikan stderr dulu lalu stdout dalam batas baris, baris dipotong 256 karakter, potongan `continued` tidak disimpan, `forget`), `SinkSet` (satu sink `Busy` membuat set `Busy`, ulangan batch yang sama melewati sink yang sudah menerima), dan pemotongan pada batas karakter UTF-8 → AC-12
- [ ] `cargo clippy -p stackly-core --all-targets --locked -- -D warnings` dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → keduanya lulus; `cargo xtask check-deps` → lulus

### Acceptance-criteria coverage

- AC-12 · bagian core (langkah 10): pemotongan baris, redaksi baris utuh sebelum dipotong, `LogSink` yang `Busy`, `release_below` ke ujung baris yang diterima, backlog dengan penanda truncation, resume dengan penanda `resumed`, `TailSink` untuk `lastFailure.excerpt`. Pembacaan oleh actor setiap `SPOOL_DRAIN_INTERVAL`, pembacaan terakhir saat exit, dan pembacaan spool lama sebelum respawn sudah terpasang (`supervisor/drain.rs`, `exit.rs`, `startup.rs`). Yang menunggu: sampel `st_blocks` di bawah 2 MiB selama 60 detik dengan `logLinesPerSecond` 2000 (butuh adapter macOS yang benar benar punch hole dan dummy server, jalan bersama test daemon), dan `resume` saat adopsi (langkah 11, `Opening::Resumed`)
- AC-9, AC-21, AC-22 · bagian spool dan redaksi log bergantung pada titik panggil actor dan rekonsiliasi; belum

## Milestone 4: Daemon, IPC, and end to end proof

Bagian ini ditambah per langkah. Sejauh ini: langkah 15 bagian aturan `check-deps` (sesi `dev-f10-m4-s15`). Pengukuran NFR-003 dari langkah 15 (AC-20) menunggu langkah 13 dan 14, jadi belum ada barisnya di sini.

### Langkah 15, aturan `check-deps`

#### UI / manual

- [ ] Buka `xtask/src/check_deps/source_scan.rs` → aturan *process* hanya berlaku untuk `stackly-core`, `stacklyd`, dan `stackly-platform-macos` (di crate terakhir hanya modul `process` dan `codesign` yang boleh), aturan `SIGKILL` berlaku untuk setiap crate dan hanya `stackly-platform-macos` modul `process::kill` yang diizinkan (`SIGKILL_ALLOWED`); `codesign` adalah satu satunya pengecualian bernama (K9); aturan `ChildRole::RuntimeCheck` (K6) sengaja belum ditambahkan karena variannya belum ada di kode dan `codesign` (K9) mungkin menyebutnya, jadi daftar tempat yang diizinkan belum bisa dipastikan → AC-15
- [ ] Buka `xtask/src/check_deps.rs` → aturan `fixture` mengikuti rumusan *Aturan fitur `fixture`* spec 0014: dependency `stackly-core` yang menyalakan `fixture` hanya lewat `[dev-dependencies]` (juga per target, juga lewat nama lain), tidak ada entri `[features]` yang memuat `stackly-core/fixture` atau `stackly-core?/fixture`, `stackly-core` tidak memuat `fixture` di `default` atau fitur lain, dan `[workspace.dependencies]` tidak menyalakannya → spec 0014 AC-3

#### Commands

- [ ] `cargo xtask check-deps` → lulus di workspace ini: `check-deps: 13 crates follow the dependency, unsafe, and source rules`; fitur `fixture` belum ada di `stackly-core` (langkah 13), dan aturannya sudah menegakkan semua larangan begitu fitur itu muncul → AC-15
- [ ] `cargo test -p xtask source_scan` → test baru lulus: cakupan `Rules::for_crate` untuk aturan process dan `SIGKILL`; core dan daemon tidak menyebut path spawn (`std::process::Command`, `tokio::process`, `std::os::unix::process`, `use std::process as p` dan glob nya, `posix_spawn`, `execve`, `fork`, `vfork` sebagai identifier, juga lewat `use ... as`), sambil `exit`, `ExitCode`, `id`, doc comment, string, dan kode test lolos; adapter macOS hanya spawn di `process` dan `codesign`; `SIGKILL` (juga lewat `use`, `assert_eq!`) hanya lolos di `process::kill` → AC-15
- [ ] `cargo test -p xtask check_deps` → test baru lulus: `only_a_dev_dependency_may_turn_on_the_core_fixture_feature` (normal, build, dan per target ditolak; dev lolos), crate fixture `a_fixture_workspace_that_turns_on_core_fixture_outside_a_dev_dependency_is_rejected` (dependency normal, build, per target, dan `package = "stackly-core"` ditolak; entri `[features]` biasa, opsional `?`, dan lewat nama lain ditolak; `default` dan `extra` di core ditolak; dev dependency, juga per target, lolos), crate fixture `a_fixture_workspace_table_that_turns_on_core_fixture_is_rejected_even_when_inherited` (dua kunci `[workspace.dependencies]` ditolak dan anggota yang mewarisi sebagai dependency normal ditolak, entri bersih lolos), dan crate fixture `a_fixture_workspace_that_breaks_the_process_and_sigkill_rules_fails_with_file_and_line` (pelanggaran dengan file dan baris di core, daemon, dan adapter macOS) → AC-15, spec 0014 AC-3
- [ ] `cargo clippy -p xtask --all-targets --locked -- -D warnings` dan `cargo fmt --all --check` → keduanya lulus

#### Acceptance-criteria coverage

- AC-15 · bagian aturan: `SIGKILL` hanya di `process::kill` dan tidak ada process API di core dan daemon ditegakkan `check-deps` (sebelum langkah ini hanya diklaim di Milestone 1, kode aturannya baru ada di sini); TERM lalu KILL pada group pembantu dibuktikan Milestone 1 dan 2
- AC-20 · belum: pengukuran NFR-003 (10 instance fake idle selama 5 menit) menunggu langkah 13 dan 14
- Spec 0014 AC-3 · bagian aturan `check-deps` lengkap untuk keempat butir rumusan; bagian `cargo tree` dan build rilis menunggu spec 0014
