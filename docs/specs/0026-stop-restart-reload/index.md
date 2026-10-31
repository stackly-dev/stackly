# 0026. Stop, restart, dan reload

**Date**: 2026-11-01
**Status**: Accepted

## Summary

Spec ini melengkapi lifecycle harian sebuah instance: stop yang jujur, restart sebagai satu operasi, reload yang hanya ada kalau provider mendukungnya, dan force stop sebagai jalan keluar yang disengaja. Force stop hanya bisa dipakai saat instance masih `stopping` setelah stop normal gagal selesai, dan ia mengirim `SIGKILL` hanya ke proses utama yang identitasnya baru saja diperiksa ulang, tidak pernah ke proses yang tidak terbukti milik instance itu. Restart adalah satu operasi daemon (stop, tunggu proses keluar dan `instance.lock` bebas, lalu start) yang antre di belakang operasi lain, sehingga stop dan start yang bersamaan selalu berurutan. Setiap akhir proses dicatat di registry sebagai `clean`, `forced`, atau `crashed`, dan app tahu dari daftar instance apakah provider bisa reload, supaya tombol dan menu Reload nonaktif dengan alasan yang terlihat untuk provider seperti NATS.

## Requirements

**User stories**:

- Sebagai developer, saya ingin menghentikan `pg17-dev` dan melihat hasil stop yang jujur, termasuk saat stop belum selesai, supaya saya tahu data saya aman.
- Sebagai developer, saya ingin me restart instance dengan satu aksi dari GUI atau `stackly instance restart`, dan yakin tidak ada dua proses yang berjalan di data yang sama.
- Sebagai developer, saya ingin me reload config PostgreSQL tanpa memutus koneksi, dan melihat dengan jelas bahwa NATS tidak mendukung reload, bukan klaim reload palsu.
- Sebagai developer, saya ingin bisa memaksa berhenti instance yang macet saat stopping, dengan risiko yang dijelaskan, tanpa Stackly pernah membunuh proses yang bukan miliknya.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-011):

- **AC-1** (stop normal, REQ-011, AC-011): `instance.stop` tetap menjalankan hanya strategi provider seperti spec 0010 *Stop* (sinyal ke proses utama yang identitasnya diperiksa ulang, atau perintah stop), tanpa `SIGKILL` dan tanpa kill berdasarkan nama. Hasilnya dilaporkan: operasi `succeeded` dengan `lastExit` `clean`, atau `failed` dengan `supervisor.stop_timeout` atau `supervisor.stop_command_failed` sementara lifecycle tetap `stopping`. Dibuktikan pada fake provider (fixture) dan PostgreSQL 18 nyata dengan koneksi aktif (native).
- **AC-2** (force hanya saat stopping): method baru `instance.force_stop` hanya menerima instance berlifecycle `stopping` tanpa operasi aktif. `running`, `starting`, dan `recovering` ditolak `supervisor.force_requires_stopping` tanpa operasi; `stopped` dan `failed` dijawab `alreadyInState` tanpa operasi; `unknown` ditolak `supervisor.identity_unproven` tanpa sinyal; `uninitialized`, `initializing`, dan retained ditolak dengan kode spec 0010 *Stop* langkah 1 yang sama; operasi lain yang memegang instance membuat `resource.busy`. Instance `stopping` yang slot prosesnya ternyata kosong ditutup lewat `settle_stopping` spec 0010 tanpa sinyal, dan operasi `succeeded`.
- **AC-3** (sasaran force, REQ-011, AC-067): operasi `force_stop_instance` memeriksa ulang identitas majemuk proses utama tepat sebelum mengirim `SIGKILL` lewat port baru `ProcessSupervision::force_kill`. Identitas yang tidak cocok (misalnya PID yang kini milik executable lain) tidak disinyal: instance menjadi `unknown` dan operasi gagal `process.signal_refused`. `SIGKILL` hanya ke proses utama, tidak ke process group; anggota group yang tersisa dilaporkan sebagai `leftoverProcesses` seperti stop normal, dan ditampilkan di umpan balik force.
- **AC-4** (akhir force): journal force menulis desired `stopped` sebelum `SIGKILL`. Setelah `SIGKILL`, handler exit tunggal spec 0010 menutup instance menjadi `stopped` dengan `lastExit` `forced`, lalu operasi `succeeded`. Proses yang belum keluar dalam `FORCE_EXIT_WAIT` membuat operasi gagal `supervisor.force_timeout`, lifecycle tetap `stopping`, proses tetap diawasi, dan exit yang datang kemudian tetap tercatat `forced`. Daemon yang mati setelah langkah `kill` mulai juga tidak menghilangkan catatan itu: rekonsiliasi membacanya dari journal operasi (*Handler exit: `last_exit`*).
- **AC-5** (catatan akhir proses): setiap kali proses utama keluar, commit yang menulis `exited` juga menulis `instances.last_exit` menurut tabel *Handler exit: `last_exit`*: `forced` kalau ada operasi `force_stop_instance` untuk instance itu yang langkah `kill` nya sudah mulai dan yang masih terbuka atau ditutup `interrupted`; `crashed` untuk child daemon yang keluar karena sinyal yang tidak dikirim daemon saat `stopping`, dan untuk setiap exit saat `starting`, `running`, atau `recovering`; selain itu `clean` untuk exit saat `stopping`. `exited` tanpa proses (jeda backoff, startup yang dibatalkan daemon sebelum proses berjalan) tidak mengubah `last_exit`. `InstanceSummary.lastExit` membawa nilainya (null untuk instance yang belum pernah berjalan atau baris sebelum migrasi), dan revision instance naik seperti setiap commit `exited`.
- **AC-6** (restart sebagai operasi): `instance.restart` pada instance `running` (siap atau belum) atau `recovering` menjalankan satu operasi `restart_instance`: journal stop menulis desired `stopped` dan `stop_begin` (seperti stop spec 0010), stop dengan strategi provider, tunggu proses keluar, tunggu `instance.lock` bebas sampai `RESTART_LOCK_WAIT`, lalu startup transaction spec 0010 yang journal nya menulis desired `running`, dengan pemeriksaan data spec 0025. Hasilnya instance `running` dengan PID baru dan desired `running`. Journal stop restart juga memanggil `state.restart.clear()` dan menghitung ulang `restart_status`, apa pun lifecycle sebelumnya, jadi jendela restart policy mulai dari nol. `recovering` dengan timer backoff saja membatalkan timer itu lalu langsung start; `recovering` dengan operasi respawn sistem yang sedang berjalan antre (AC-9) lalu memvalidasi ulang dari state segar.
- **AC-7** (restart ditolak): restart pada `starting`, `stopped`, `failed`, atau `uninitialized` ditolak `supervisor.instance_not_running` tanpa operasi; `stopping` ditolak `supervisor.instance_stopping`; `unknown` ditolak `supervisor.identity_unproven`; `initializing` dan retained dengan kode spec 0010. Penolakan yang baru terlihat setelah antre (misalnya respawn yang berakhir `failed`) menggagalkan operasi restart dengan kode yang sama. Restart tidak menerima opsi force dan tidak memeriksa lease environment.
- **AC-8** (restart gagal di separuh stop): stop yang timeout atau perintah stop yang gagal di dalam restart membuat operasi gagal dengan kode stop yang sama, lifecycle tetap `stopping`, desired tetap `stopped` (sudah ditulis journal stop), dan banner stopping tampil. Detail error menyebut bahwa instance akan tetap berhenti dan Start adalah langkah berikutnya. Proses yang keluar kemudian membuat instance `stopped` tanpa start otomatis. Lock yang masih dipegang setelah `RESTART_LOCK_WAIT` membuat operasi gagal `supervisor.data_in_use` dengan detail `leftoverProcesses`, memasang blocker `InstanceBlocker::DataInUse` (dihapus recheck spec 0010 saat lock bebas), dan tidak ada proses kedua di data yang sama.
- **AC-9** (serialisasi): restart memakai `BusyPolicy::Queue` (seperti start dan reload pengguna) dan memeriksa state lagi dari data segar setelah antrean. Stop pengguna selama operasi restart berjalan mendapat `resource.busy` (sama dengan stop selama start, spec 0010 AC-7); kamu membatalkan restart lebih dulu, dan cancel di separuh start menjalankan stop biasa di dalam pekerjaan. `force_stop` memakai `BusyPolicy::Reject`. Test yang mengirim start, stop, dan restart bersamaan dari dua client tidak pernah menghasilkan dua proses utama atau dua pemegang `instance.lock`.
- **AC-10** (reload, BR-016, AC-026): `instance.reload` pada instance `running` (readiness apa pun) dari provider dengan capability `reload` menjalankan operasi `reload_instance` di atas `execute_reload` spec 0010: PID tidak berubah dan readiness diverifikasi. Provider tanpa capability itu ditolak `supervisor.capability_unsupported` sebelum operasi dibuat; `stopping` ditolak `supervisor.instance_stopping`, `unknown` ditolak `supervisor.identity_unproven`, dan lifecycle lain yang tidak `running` ditolak `supervisor.instance_not_running`, semuanya tanpa operasi. Untuk instance yang belum ready, reload terverifikasi saat aksinya selesai dan PID tidak berubah; instance yang ready harus ready lagi dalam anggaran plan (`plan.deadline` dari `reloadSeconds`). Dibuktikan pada PostgreSQL 18 nyata (PID sama, ready) dan NATS nyata (ditolak, tidak ada operasi).
- **AC-11** (capability di daftar instance): `InstanceSummary.capabilities.reload` berisi capability `reload` dari baris `provider_revisions` yang dipin instance, bukan dari catalog dan tanpa membaca paket. GUI, CLI, dan daemon memakai sumber yang sama, jadi alasan unsupported konsisten (BR-016).
- **AC-12** (offline, REQ-046, AC-046): stop, restart, reload, dan force stop untuk instance fake dengan runtime dan paket pinned yang sudah ada berhasil dengan nol akses `CatalogState`, nol panggilan `Fetcher`, dan nol panggilan `CodeSigning::assess`, dengan harness offline spec 0021 AC-10. Bukti native dengan jaringan mati untuk PostgreSQL `BLOCKED` sampai disetujui user.
- **AC-13** (CLI, REQ-028, AC-028): `stackly instance restart <target>` dan `stackly instance reload <target>` menerima `--wait`, `--timeout`, dan `--idempotency-key` seperti start dan stop, dengan target lewat `TargetArg`. `stackly instance stop <target> --force` memanggil `instance.force_stop`; di terminal ia meminta konfirmasi destruktif (jawaban default tidak), tanpa terminal atau dengan `--json` (juga di terminal) ia butuh `--yes`, selain itu `cli.confirmation_required` (exit 2) sebelum apa pun dikirim. `--force` menghormati `--wait`, dan `alreadyInState` keluar dengan exit 0. Kode error dan exit code mengikuti kategori di `stackly-protocol`, dan `--json` setara dengan GUI.
- **AC-14** (toolbar detail, mockup): toolbar detail instance menampilkan tombol aksi utama Start atau Stop yang sudah ada, lalu tombol ikon Restart dan Reload seperti mockup. Restart aktif pada `running` dan `recovering`; Reload aktif pada `running` dengan capability `reload`. Masing masing nonaktif dengan alasan dari *Alasan nonaktif*, dan tooltip memakai teks alasan yang sama.
- **AC-15** (menu Instance, spec 0069): menu Instance mendapat item Restart, Reload, dan Force Stop… untuk target yang sama dengan Start dan Stop (detail terbuka, selain itu baris terpilih), dengan alasan nonaktif sebagai subtitle, jadi alasannya terlihat tanpa hover (AC-027, NFR-012). Item tanpa shortcut keyboard (kebijakan interaksi 2026-10-24).
- **AC-16** (banner stopping dan konfirmasi force): instance `stopping` tanpa operasi aktif menampilkan banner caution "Still stopping." di Summary dengan tombol Stop Again dan Force Stop…. Force Stop… (dari banner atau menu) membuka konfirmasi destruktif native dengan Cancel sebagai default dan teks risiko di *Copy*; Force Stop baru mengirim `instance.force_stop` setelah dikonfirmasi.
- **AC-17** (catatan akhir di Details): kartu Details menampilkan baris "Last exit" dengan "Forced" atau "Crashed" saat `lastExit` bernilai itu, dan baris tidak tampil untuk `clean` atau null.
- **AC-18** (umpan balik, AC-027): restart, reload, dan force stop yang selesai atau gagal menampilkan kartu umpan balik spec 0067 dengan teks di *Copy*, dan selama operasinya berjalan tombol yang bersangkutan memakai label sibuk "Restarting…", "Reloading…", atau "Force stopping…" dengan aturan label sibuk spec 0022 (subtitle menu tetap "In progress" dari spec 0069). Alur stop, restart, dan inspect bisa diselesaikan dengan pointer tanpa informasi yang hanya tersedia lewat warna atau hover.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md), dipilih user pada 2026-11-01: force hanya dari `stopping` setelah stop normal, sebagai method sendiri `instance.force_stop` dan operasi `force_stop_instance`, `SIGKILL` hanya ke proses utama setelah pemeriksaan ulang identitas lewat port baru `force_kill`, penolakan untuk `unknown`; kolom registry `instances.last_exit` (`clean`, `forced`, `crashed`); restart sebagai satu operasi daemon dari `running` atau `recovering`, antre di belakang operasi lain, gagal dengan desired `stopped` kalau separuh stop gagal, menunggu `instance.lock` sebentar, tanpa force dan tanpa cek lease; stop selama restart `resource.busy`; reload yang ditolak di precheck untuk provider tanpa capability, aktif pada `running` dengan readiness apa pun; capability `reload` di `InstanceSummary`; banner "Still stopping." dengan Force Stop… dan item menu; `stop --force` di CLI dengan konfirmasi atau `--yes`; dan teks risiko netral provider dengan catatan data; ditambah semua perbaikan dari dua cross check (Sonnet G1 sampai G25, Fable 5.1 F1 sampai F11 dan H1 sampai H15) yang disetujui user pada 2026-11-01, termasuk niat force yang dibaca dari journal operasi alih alih kolom baru.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

## Rationale

Alasan dan opsi yang ditimbang: lihat [rationale.md](rationale.md).

## Feature design

### Trace PRD

| ID PRD | Kriteria spec ini |
|---|---|
| REQ-011 (lifecycle aman) | AC-1 sampai AC-10 |
| REQ-028 (CLI parity) | AC-13 |
| REQ-046 (offline) | AC-12 |
| BR-016 (unsupported konsisten) | AC-10, AC-11, AC-14, AC-15 |
| AC-011 | AC-1 |
| AC-026 | AC-10, AC-14 (alasan nonaktif menyarankan restart) |
| AC-027, NFR-012 | AC-15, AC-16, AC-18 |
| AC-046 | AC-12 |
| AC-067 | AC-3 |

### Batas dengan spec lain

| Hal | Pemilik |
|---|---|
| Strategi stop, handler exit, identitas majemuk, `execute_reload`, startup transaction | spec 0010 (dipakai ulang; spec ini mengubah aturan `SIGKILL` di *Usulan update untuk spec lain*) |
| Operation engine, `BusyPolicy`, `CloseInterrupted` | spec 0007 |
| Pemeriksaan data saat start | spec 0025 (`check_data_compatibility`) |
| Label sibuk dan aksi utama Start atau Stop | spec 0022 (`InstanceAction.resolve`) |
| Menu Instance dan peta perintah | spec 0069 |
| Kartu umpan balik, banner, konfirmasi | spec 0067, spec 0012 |
| Edit config dan dampak reload atau restart, Apply and Reload | fitur 29 |
| Jumlah restart sejak dibuat | fitur 28 |
| Backup yang membaca `last_exit` | fitur 49 |
| Lease environment yang lebih ketat untuk restart | fitur 44 sampai 47 |
| Restart daemon terencana | fitur 30 |

### Model data

Satu migrasi registry, `crates/storage/migrations/0006_instance_last_exit.sql`:

| Tabel | Kolom | Tipe | Null | Nilai | Ditulis oleh |
|---|---|---|---|---|---|
| `instances` | `last_exit` | `TEXT` | ya | `CHECK (last_exit IN ('clean', 'forced', 'crashed'))` | handler exit tunggal, di commit yang sama dengan `exited` |

- Null berarti instance belum pernah punya proses yang keluar, atau baris dibuat sebelum migrasi. Migrasi tidak mengisi baris lama.
- Domain: `stackly_domain::ExitKind { Clean, Forced, Crashed }` sebagai field `Option<ExitKind>` di aggregate instance. `LifecycleEvent::Exited` membawa `Option<ExitKind>` (null untuk `exited` tanpa proses, misalnya jeda backoff, yang tidak mengubah `last_exit`), dan state machine tetap pemilik satu satunya transisi itu.
- Tidak ada tabel baru dan tidak ada kolom niat force: niat force dibaca dari journal operasi (*Handler exit: `last_exit`*), jadi tidak ada commit tambahan yang menaikkan revision.
- Capability dibaca dari kolom `provider_revisions.capabilities` (array JSON) yang sudah ada. Loader registry memuatnya bersama aggregate `Instance` (join lewat `provider_revision_pk`) sebagai `capabilities` read only, sehingga `dto::summary` (`apps/daemon/src/ipc/instance/dto.rs`) tetap membangun summary dari aggregate saja, untuk `instance.list`, `instance.get`, dan setiap event `instance.state_changed`.

DTO (`stackly-protocol`, minor bump `ProtocolVersion::CURRENT` dari 1.15 ke 1.16):

| Tipe | Perubahan |
|---|---|
| `InstanceSummary` | `capabilities: InstanceCapabilities` (`#[serde(default)]`, default semua false) dan `lastExit: Option<InstanceLastExit>` (`#[serde(default)]`) |
| `InstanceCapabilities` | `reload: bool`; struct `#[non_exhaustive]` supaya capability lain (backup, metrics) bisa menyusul |
| `InstanceLastExit` | enum terbuka `clean`, `forced`, `crashed`, `Unrecognized` (daemon tidak pernah mengirim `Unrecognized`) |
| `OperationKind` | varian baru `ForceStopInstance` (`force_stop_instance`), juga di `stackly_domain::OperationKind` dengan `InterruptPolicy::CloseInterrupted` |
| `RestartInstanceParams` | `instanceId`, `expectedRevision` (opsional) |
| `ReloadInstanceParams` | `instanceId`, `expectedRevision` (opsional) |
| `ForceStopInstanceParams` | `instanceId`, `expectedRevision` (opsional) |

Ketiga params mengimplementasikan `Validate`, dan fixture kontrak baru masuk `protocols/fixtures/`.

### State transitions

Transisi lifecycle tidak bertambah; spec ini hanya menyusun yang sudah ada.

```
force_stop:  stopping (desired stopped) ──force_kill──▶ (proses keluar) ──exited{forced}──▶ stopped
restart:     running|recovering ──stop_begin{desired stopped}──▶ stopping ──exited{clean}──▶ stopped
             ──(lock bebas)──▶ start_begin{desired running} ──▶ starting ──▶ running
restart gagal di stop:  stopping ──(nanti exited{clean})──▶ stopped (desired tetap stopped)
reload:      running ──▶ running (PID sama)
```

Daemon mati di tengah restart: di separuh stop (desired sudah `stopped`) rekonsiliasi spec 0010 mengirim ulang stop atau menutup `stopping`, jadi instance berakhir berhenti; di separuh start (desired sudah `running`) rekonsiliasi menjalankan start sistem seperti untuk start yang terputus.

### Method IPC

Tiga baris baru di tabel `methods!` (`crates/protocol/src/message.rs`), semuanya `recovery refused`:

| Method | Params | Balasan | Operasi | Busy | Error penting |
|---|---|---|---|---|---|
| `instance.restart` | `RestartInstanceParams` | `InstanceActionAccepted` | `restart_instance` | `Queue` | `supervisor.instance_not_running`, `supervisor.instance_stopping`, `supervisor.identity_unproven`, `resource.revision_mismatch`; dari operasi: `supervisor.stop_timeout`, `supervisor.stop_command_failed`, `supervisor.data_in_use`, kode start spec 0010 |
| `instance.reload` | `ReloadInstanceParams` | `InstanceActionAccepted` | `reload_instance` | `Queue` | `supervisor.capability_unsupported`, `supervisor.instance_not_running`, `supervisor.instance_stopping`, `supervisor.identity_unproven`, `resource.revision_mismatch`; dari operasi: `supervisor.reload_failed`, `process.signal_refused` |
| `instance.force_stop` | `ForceStopInstanceParams` | `InstanceActionAccepted` (`alreadyInState` untuk `stopped` dan `failed`) | `force_stop_instance` | `Reject` | `supervisor.force_requires_stopping`, `supervisor.identity_unproven`, `resource.busy`; dari operasi: `process.signal_refused`, `supervisor.force_timeout` |

- Setiap method memanggil `ipc::operation::replay_operation` lebih dulu, lalu precheck, lalu `ipc::operation::start_operation`, seperti aturan precheck di `AGENTS.md`. Precheck yang menolak tidak membuat operasi.
- Precheck membaca state dari registry; pekerjaan operasi membaca lagi di dalam lock (state bisa berubah selama antre) dan menolak dengan kode yang sama kalau state tidak lagi cocok.
- `expectedRevision` hanya diperiksa di precheck. Validasi ulang setelah antre memakai lifecycle, bukan revision, karena commit readiness dan health menaikkan revision selama antre.
- Detail error `supervisor.data_in_use` dari restart memakai key `leftoverProcesses` (daftar PID, tipe yang sama dengan status spec 0010).
- `instance.stop` tidak berubah.

Kode error baru (`crates/protocol/src/codes.rs`):

| Kode | Kategori | Retryable | Kapan |
|---|---|---|---|
| `supervisor.instance_not_running` | Conflict | false | restart atau reload pada instance yang tidak `running` (reload) atau tidak `running` atau `recovering` (restart) |
| `supervisor.force_requires_stopping` | Conflict | false | force pada `running`, `starting`, atau `recovering`; detail menyarankan stop lebih dulu |
| `supervisor.force_timeout` | Timeout | false | proses belum keluar `FORCE_EXIT_WAIT` setelah `SIGKILL` |

### Supervisor: force stop

Modul baru `crates/core/src/supervisor/force.rs`, dengan `Supervisor::force_stop_work(ForceStopRequest)`. Langkah (`StepSpec`):

1. `validate`: baca instance; lifecycle harus `stopping`, kalau tidak tolak dengan kode AC-2.
2. `lock`: `LockKey::Instance` dengan `BusyPolicy::Reject`. Slot proses kosong: baca lagi; `stopped` atau `failed` berarti handler exit sudah menutup instance, dan operasi `succeeded` tanpa sinyal; masih `stopping` berarti exit sedang atau sudah ditangani tanpa penutupan, jadi `settle_stopping` spec 0010 menulis `exited{clean}` dan operasi `succeeded` tanpa sinyal.
3. `journal`: satu commit CAS (compare and swap, ditulis hanya kalau revision belum berubah) yang menulis desired `stopped` dan hanya berlaku saat lifecycle masih `stopping`; lifecycle lain berarti proses sudah keluar, dan operasi `succeeded` tanpa sinyal. Setelah langkah ini kamu tidak bisa membatalkan dengan kompensasi.
4. `kill` (`reversible: false`): engine menandai langkah ini `running` di journal operasi sebelum pekerjaan langkah dijalankan (spec 0007), lalu `force_kill(SignalTarget::new(identity))`. Hasil:
   - `Ok`: lanjut ke langkah 5.
   - `IdentityMismatch`: tidak ada sinyal, `identity_lost` (`unknown`), operasi gagal `process.signal_refused`.
   - `ESRCH` (proses keluar di antara pemeriksaan dan kill): diperlakukan sebagai exit; lanjut ke langkah 5, dan handler exit menentukan `ExitKind` (tetap `forced`, karena langkah `kill` sudah mulai).
   - Error lain (`EPERM`, `NotSupported`): tidak ada sinyal terkirim, operasi gagal `process.signal_refused`, dan langkah ditutup `failed` di journal supaya tidak dihitung sebagai niat force.
5. `await_exit`: tunggu handler exit sampai `FORCE_EXIT_WAIT`. Waktu habis: `supervisor.force_timeout`, dan langkah `kill` tetap tercatat mulai, jadi exit yang datang kemudian tetap `forced`.

Port baru di `stackly-platform` (`ProcessSupervision`):

- `fn force_kill(&self, target: &SignalTarget) -> Result<(), ProcessError>`: pemeriksaan ulang identitas yang sama dengan `signal`, lalu `SIGKILL` ke PID itu saja. Implementasi macOS di `stackly-platform-macos/src/process/kill.rs`, satu satunya modul di `SIGKILL_ALLOWED`. `ESRCH` dikembalikan sebagai `ProcessError::Gone`. `UnsupportedProcessSupervision` mengembalikan `NotSupported`; `FakeProcessSupervision` mencatat panggilan dan identitas yang diperiksa, dan test bisa menyuntikkan `Gone` dan `IdentityMismatch`.
- Aturan baru `cargo xtask check-deps`: `force_kill` hanya dipanggil dari `stackly-core` modul `supervisor::force` (daftar `FORCE_KILL_ALLOWED` di `xtask/src/check_deps/source_scan.rs`), dengan crate fixture yang melanggar.

### Supervisor: restart

Modul baru `crates/core/src/supervisor/restart_op.rs` (nama `restart.rs` sudah dipakai restart policy), dengan `Supervisor::restart_work(RestartRequest { instance, caller })`. Langkah:

1. `validate`: `running` atau `recovering`; selain itu kode AC-7.
2. `lock`: `LockKey::Instance` dengan `BusyPolicy::Queue`; setelah lock, baca lagi dan validasi ulang dengan aturan AC-7. Sub kasus `recovering` (dari state segar):
   - timer backoff saja, tanpa operasi respawn: lanjut ke langkah 3;
   - ada operasi respawn sistem: restart menunggu di antrean sampai operasi itu selesai, lalu validasi ulang memutuskan (`running`: restart biasa; `failed` karena `crash_loop`: operasi gagal `supervisor.instance_not_running`).
   `recovering` dengan proses hidup tidak ada di luar tahap `identity` operasi respawn, karena state machine memetakan `(Recovering, Spawned)` ke `running`.
3. `journal`: satu commit yang menulis input journal `trigger: user_restart`, desired `stopped`, `state.restart.clear()` dan `restart_status` baru, serta: `running` mendapat `stop_begin`; `recovering` dengan timer saja: timer dibatalkan di commit yang sama, lalu `exited` tanpa `ExitKind` (`last_exit` tidak berubah). Crash yang ditangani sebelum commit ini membuat validasi ulang melihat `recovering`, yang memang benar.
4. `stop`: `stop_process` spec 0010 yang sama dengan anggaran `StopPlan` provider. Gagal: operasi gagal dengan kode stop (AC-8); desired sudah `stopped`, jadi tidak ada commit tambahan.
5. `await_exit`: handler exit menulis `exited{clean}` (atau `crashed`, lihat *Handler exit*).
6. `lock_wait`: coba `lock_instance` setiap `RESTART_LOCK_POLL` sampai `RESTART_LOCK_WAIT`. Masih `Held`: `group_members` dicatat sebagai `leftoverProcesses` (status dan detail error), blocker `InstanceBlocker::DataInUse` dipasang, dan operasi gagal `supervisor.data_in_use`.
7. `start`: startup transaction spec 0010 mulai dari tahap `verify` di bawah operasi yang sama; journal start menulis desired `running` dan `start_begin` (spec 0010 *Startup transaction*), dengan interaksi Keychain dari caller (seperti start pengguna) dan `check_data_compatibility` spec 0025. Tahap `spawn` dan sesudahnya `reversible: false`. Kegagalan di separuh ini mengikuti aturan start pengguna spec 0010 apa adanya, dengan kode, lifecycle, dan desired yang sama seperti start pengguna yang gagal di tahap yang sama: kegagalan sebelum journal start (`verify`, `endpoints`) tidak mengubah apa pun, jadi desired tetap `stopped` dari journal stop restart; crash sebelum ready di respawn di bawah operasi yang sama sampai retry budget (*Restart policy*); `endpoint_in_use` dan `binary_rejected_by_os` gagal tanpa budget.

Cancel: sebelum langkah 3 tanpa perubahan; selama `stop`, `await_exit`, atau `lock_wait` operasi `cancelled` dan instance berhenti (desired sudah `stopped`, jadi Stackly tidak menyalakannya lagi); selama `start` aturan cancel startup transaction spec 0010, termasuk membatalkan jeda backoff di dalam operasi ("jeda di dalam operasi", spec 0010 *Restart policy*).

### Supervisor: reload

`reload_work` yang sudah ada dipakai dengan dua perubahan kecil: `execute_reload` memakai `supervisor.instance_not_running`, `supervisor.instance_stopping`, atau `supervisor.identity_unproven` untuk lifecycle selain `running` (menggantikan `ReloadFailed` dengan reason "not running"), dan langkah verify menerima instance yang tidak ready sebelum reload sebagai terverifikasi kalau aksi selesai dan identitas proses tetap sama. Capability `reload` di `provider_revisions.capabilities` selalu berarti ada `ReloadPlan` (spec 0009 AC-5); kalau tetap `PlanError::Unsupported`, hasilnya `supervisor.capability_unsupported`. Anggarannya `plan.deadline` dari `reloadSeconds` manifest. Secret di resolve lewat `ReloadRequest.caller`. Handler `instance.reload` di `stacklyd` melakukan precheck capability dan lifecycle sebelum operasi dibuat.

### Handler exit: `last_exit`

Niat force tidak disimpan di memori atau kolom, tetapi dibaca dari journal operasi: `force_intent(instance)` di `OperationEngine` mengembalikan true kalau ada operasi `force_stop_instance` untuk instance itu yang langkah `kill` nya berstatus `running` atau `succeeded`, dan operasinya masih terbuka atau ditutup `interrupted` oleh `recover`. Pembacaan ini berjalan di thread registry di dalam commit yang menulis `exited`, jadi tidak ada race dengan journal force. Operasi force yang langkah `kill` nya `failed` tidak dihitung.

Setiap jalur yang menulis `exited` memakai tabel ini (baris pertama yang cocok menang):

| Jalur | Kondisi | `ExitKind` |
|---|---|---|
| semua jalur dengan proses | `force_intent` true | `forced` |
| handler exit, child daemon | lifecycle `stopping`, status `Signaled(_)` yang tidak dikirim daemon | `crashed` |
| handler exit (child atau adopsi) | lifecycle `stopping` | `clean` |
| handler exit | lifecycle `starting`, `running`, atau `recovering` (termasuk crash yang di respawn) | `crashed` |
| `settle_stopping` (stop atau force menemukan slot kosong saat `stopping`) | | `clean` |
| `settle_gone_process` (tahap `lock` stop, `stop.rs`) | lifecycle yang dibaca `stopping` | `clean` |
| rekonsiliasi atau recheck `unknown`, proses `Gone` | lifecycle tersimpan `stopping` | `clean` |
| rekonsiliasi atau recheck `unknown`, proses `Gone` | lifecycle tersimpan `running`, `starting`, atau `recovering` | `crashed` |
| `journal_stop` tanpa proses (jeda backoff), journal restart dari `recovering` dengan timer saja | | tidak berubah |
| startup yang dibatalkan daemon sebelum proses berjalan (`SuspendedChild::discard`, kegagalan sebelum `resume`) | | tidak berubah |
| startup yang dihentikan daemon setelah `resume` (deadline readiness, cancel) | lewat alur *Stop*, lifecycle `stopping` | `clean` |

`LifecycleEvent::Exited` membawa `Option<ExitKind>`; `None` hanya untuk baris "tidak berubah". Status `Signaled(_)` hanya dipakai untuk child daemon ini (proses adopsi selalu berstatus `None`, spec 0010).

### Alasan nonaktif

Tombol toolbar dan item menu memakai satu fungsi murni `InstanceLifecycleActions.resolve(summary, busy, online)` di `StacklyUI`, di samping `InstanceAction.resolve` spec 0022, supaya toolbar, menu, dan test fixture setuju. Urutan pemeriksaan (alasan pertama yang cocok menang):

| Aksi | Urutan alasan |
|---|---|
| Restart | daemon offline (alasan shell yang sama dengan aksi lain) · sibuk · `unknown`: "Stackly can’t confirm this instance’s process." · bukan `running` atau `recovering`: "Restart needs a running instance." |
| Reload | daemon offline · sibuk · `capabilities.reload` false: "{Provider} doesn’t support reload. Restart to apply configuration changes." · `unknown`: "Stackly can’t confirm this instance’s process." · bukan `running`: "Reload needs a running instance." |
| Force Stop… | daemon offline · sibuk · `unknown`: "Stackly can’t confirm this instance’s process." · bukan `stopping`: "Force stop is available while an instance is still stopping." |

"Sibuk" di tombol toolbar memakai label sibuk aksi yang berjalan (spec 0022 dan *Copy*); di menu Instance subtitle sibuk tetap "In progress" dari `CommandItemState.resolve` spec 0069.

`{Provider}` adalah `software` dengan nama tampil dari peta nama tampil provider yang sudah dipakai daftar instance (spec 0022).

### CLI

- `apps/cli/src/commands/instance/action.rs` mendapat `RestartArgs` dan `ReloadArgs` (target dan `AsyncArgs`), dengan `Verb::Restart` dan `Verb::Reload` untuk teks kesimpulan.
- `StopArgs` mendapat `--force` dan `ConfirmArgs` (`--yes`, hanya berarti bersama `--force`; `--yes` tanpa `--force` adalah usage error). `--force` memakai `ConfirmRole::Destructive` dan `OnNoninteractive::Fail`. Pertanyaan: `force stop <name>?`; consequence: teks risiko CLI di *Copy*.
- Kesimpulan teks (stdout, progress di stderr): tanpa `--wait`, id operasi yang diterima seperti start dan stop; dengan `--wait`, `restarted <name> (pid <pid>)` dengan PID dari `instance.get` setelah operasi `succeeded` (hasil operasi tidak membawa PID, dan protokol tidak diubah untuk itu), `reloaded <name>`, `force stopped <name>` diikuti `leftover processes: <pid>, …` kalau ada; untuk `alreadyInState` force `<name> is already stopped` dengan exit 0.
- `--force --json` selalu memakai jalur noninteractive, juga di terminal: tanpa `--yes` gagal `cli.confirmation_required` (kategori `InvalidInput`, exit 2). `--force` menghormati `--wait` dan `--timeout`.
- Completion dan man page dibangun ulang.

### GUI

- Toolbar detail (`InstanceDetail`): urutan mockup, tombol aksi utama, lalu tombol ikon Restart (`arrow.clockwise`) dan Reload (`bolt`), dengan `help` berisi judul atau alasan nonaktif. Menu More tetap di ujung.
- Banner stopping: `StateBanner` caution di atas kartu Summary saat `lifecycle == stopping` dan tidak ada operasi aktif untuk instance itu (dari store operasi yang sama dengan label sibuk spec 0022). Teksnya netral, karena `stopping` tanpa operasi juga bisa datang dari rekonsiliasi atau stop yang terputus. Tombol: Stop Again (memanggil `instance.stop`, yang mengirim ulang strategi dengan anggaran baru) dan Force Stop…. Selama ada operasi aktif banner tidak tampil, dan label sibuk berlaku.
- Umpan balik force yang sukses menyebut PID yang tersisa kalau `leftoverProcesses` tidak kosong. Restart yang gagal `supervisor.data_in_use` menampilkan blocker `DataInUse` spec 0022 di Summary.
- Tombol ikon sibuk: selama restart atau reload berjalan, tombol ikon itu menampilkan `ProgressView` kecil di tempat ikon, nonaktif, dengan `help` berisi label sibuk.
- Konfirmasi Force Stop: presentasi native spec 0012 dan 0067, destruktif, Cancel default.
- Menu Instance: baris `restartInstance` dan `reloadInstance` di peta perintah spec 0069 menjadi built, ditambah baris baru `forceStopInstance` (judul "Force Stop…", tanpa shortcut), diterbitkan halaman Instances untuk target yang sama.
- Details: baris "Last exit" sesudah Restart attempts.
- `ResourceStore` dan `InstanceStore` membaca field DTO baru lewat tipe hasil typeshare; `capabilities` dan `lastExit` ikut fixture tampilan spec 0068 (state: reload didukung, tidak didukung, stopping dengan banner, last exit forced dan crashed).

### Copy

| Tempat | Teks |
|---|---|
| Tombol Restart (aktif) | help "Restart" |
| Tombol Reload (aktif) | help "Reload configuration" |
| Banner stopping, judul | "Still stopping." |
| Banner stopping, isi | "Stopping hasn’t finished. You can wait, stop again, or force stop." |
| Label sibuk | "Restarting…" · "Reloading…" · "Force stopping…" |
| Konfirmasi, judul | "Force stop “{name}”?" |
| Konfirmasi, isi (GUI dan CLI sama) | "Force stop sends SIGKILL to the main process. Unsaved work is lost and the next start may run crash recovery. Child processes may keep running and hold the data until they exit. The last stop is marked forced, so no backup will call this data clean." |
| Umpan balik force dengan sisa | "Force stopped “{name}”. Still running: PID {pids}." |
| Detail error restart gagal di stop | "The restart stopped halfway. “{name}” stays stopped; use Start when it has finished stopping." |
| Konfirmasi, tombol | "Force Stop" (destruktif), "Cancel" |
| Umpan balik sukses | "Restarted “{name}”." · "Reloaded “{name}”." · "Force stopped “{name}”." |
| Umpan balik gagal | "Couldn’t restart “{name}”." · "Couldn’t reload “{name}”." · "Couldn’t force stop “{name}”." dengan pesan dari kode error |
| Details, "Last exit" | "Forced" · "Crashed" |

### Konstanta

| Nama | Nilai | Tempat |
|---|---|---|
| `FORCE_EXIT_WAIT` | 10 s | `core::supervisor::config` (`SupervisorConfig`, bisa dipersingkat di test) |
| `RESTART_LOCK_WAIT` | 10 s | `core::supervisor::config` |
| `RESTART_LOCK_POLL` | 200 ms | `core::supervisor::config` |

### Value sourcing

| Aksi | Nilai | Sumber |
|---|---|---|
| precheck force, restart, reload | lifecycle, desired | `instances.lifecycle`, `instances.desired` |
| precheck reload | capability `reload` | `provider_revisions.capabilities` lewat `instances.provider_revision_pk` |
| precheck force | ada operasi aktif | `LockTable` engine spec 0007 (`BusyPolicy::Reject`) |
| force | identitas proses utama | slot proses actor (`identity` dari spawn atau adopsi, spec 0010) |
| handler exit, rekonsiliasi | niat force | `OperationEngine::force_intent(instance)` dari journal operasi (langkah `kill` `running` atau `succeeded`) |
| handler exit | `ExitKind` | tabel *Handler exit: `last_exit`* dari niat force, jalur exit, lifecycle yang dibaca, dan status exit child |
| restart | anggaran stop | `StopPlan.timeout` dari paket pinned (spec 0009) |
| restart | batas tunggu lock | `RESTART_LOCK_WAIT` |
| restart | interaksi Keychain | `OperationTrigger` caller (seperti start pengguna) |
| restart | reset jendela restart policy | `state.restart.clear()` dan `restart_status` baru di journal stop restart |
| restart | PID baru untuk kesimpulan CLI | `instance.get` setelah operasi `succeeded` (`--wait` saja) |
| reload | interaksi Keychain | `ReloadRequest.caller` dari `OperationTrigger` (seperti start pengguna) |
| reload | anggaran | `plan.deadline` dari `reloadSeconds` paket pinned |
| `InstanceSummary` | `capabilities.reload` | `capabilities` di aggregate `Instance`, dimuat loader registry dari `provider_revisions` |
| `InstanceSummary` | `lastExit` | `instances.last_exit` |
| detail error restart | `leftoverProcesses` | `group_members(pid)` spec 0010 |
| Summary setelah `data_in_use` | blocker | `InstanceBlocker::DataInUse` |
| alasan nonaktif | nama provider | `software` dan peta nama tampil spec 0022 |
| alasan nonaktif | sibuk | store operasi app (spec 0022) |
| alasan nonaktif | offline | state koneksi shell (spec 0013) |
| banner stopping | tampil atau tidak | `lifecycle == stopping` dan tidak ada operasi aktif untuk instance |
| konfirmasi | `{name}` | `InstanceSummary.name` |
| `leftoverProcesses` | PID tersisa | `group_members(pid)` spec 0010 |

### Key invariants

- `SIGKILL` ke proses utama yang sudah berjalan hanya lewat `force_kill`, hanya dari `supervisor::force`, hanya saat `stopping`, dan selalu setelah pemeriksaan ulang identitas. Stop normal dan restart tidak pernah mengirim `SIGKILL` ke proses utama; satu satunya `SIGKILL` lain tetap pengecualian spec 0010 AC-15 untuk group proses pembantu (perintah stop, probe, init) yang melewati timeout nya, dan untuk child tertahan yang belum pernah berjalan.
- Restart menulis desired `stopped` sebelum sinyal stop pertama dan desired `running` hanya di journal start, jadi desired selalu cocok dengan separuh restart yang sedang berjalan.
- Tidak pernah ada dua proses utama, atau dua pemegang `instance.lock`, untuk satu instance: restart start hanya setelah lock bebas.
- Precheck yang menolak tidak membuat operasi; pekerjaan yang menemukan state berubah setelah antre menolak dengan kode yang sama.
- `last_exit` hanya ditulis di commit yang menulis `exited`, menurut tabel *Handler exit: `last_exit`*; niat force hanya dibaca dari journal operasi.
- Restart dan reload tidak menyentuh catalog, jaringan, atau cek trust online.
- Capability yang tampil di GUI dan CLI berasal dari provider revision yang dipin, bukan catalog.

### Security model

- Semua method hanya lewat socket Unix privat daemon milik user (spec 0006); tidak ada peran tambahan.
- Force tidak menerima PID, nama proses, atau sinyal dari client; target selalu slot proses milik daemon.
- Restart me resolve secret dengan aturan start spec 0011; secret tidak pernah lewat argv atau log.
- Teks risiko tidak memuat path data atau secret.

### Configuration required

Tidak ada env var, secret, atau credential baru.

### Critical test scenarios

- Happy path stop: fake dan PostgreSQL 18 dengan koneksi aktif berhenti lewat strategi provider, `lastExit` `clean`, tidak ada `SIGKILL` di catatan `FakeProcessSupervision`. Memverifikasi **AC-1**, **AC-5**.
- Force: fake `--ignore-term=true` dengan provider stop `term` dan `stopSeconds` pendek, stop gagal `supervisor.stop_timeout`, banner tampil, force mengirim satu `force_kill` ke PID utama, instance `stopped` dengan `lastExit` `forced`. Memverifikasi **AC-2**, **AC-4**, **AC-16**.
- Force ke PID yang dipakai ulang: identitas fake diubah test sebelum `kill`, tidak ada sinyal, `unknown`, `process.signal_refused`. Memverifikasi **AC-3** (AC-067).
- Force di luar `stopping`: `running` ditolak tanpa operasi, `unknown` ditolak tanpa sinyal, `stopped` `alreadyInState`. Memverifikasi **AC-2**.
- Restart happy path: PID baru, desired `running`, jendela restart policy direset (juga dari `running` dengan riwayat crash). Memverifikasi **AC-6**.
- Restart dari `recovering`: timer backoff saja dibatalkan lalu start; restart yang antre di belakang respawn sistem yang berakhir `crash_loop` gagal `supervisor.instance_not_running`. Memverifikasi **AC-6**, **AC-7**.
- Daemon mati di tengah force: daemon di `SIGKILL` (test kill daemon, bukan instance) setelah langkah `kill` mulai, lalu dijalankan lagi; rekonsiliasi menutup instance dengan `lastExit` `forced`. Daemon mati setelah journal force tetapi sebelum langkah `kill` mulai: rekonsiliasi mengirim stop normal dan exit tercatat `clean`. Memverifikasi **AC-4**, **AC-5**.
- Daemon mati di tengah restart: di separuh stop instance berakhir `stopped`; di separuh start rekonsiliasi menjalankan start sistem. Memverifikasi **AC-6**, **AC-8**.
- Force dan restart pada proses adopsi: daemon di restart saat fake `running`, proses diadopsi, lalu force (setelah stop timeout) dan restart berjalan, dengan restart mendapat `instance.lock` baru untuk spawn baru. Memverifikasi **AC-3**, **AC-4**, **AC-6**.
- Force dengan `ESRCH`: fake menyuntikkan `Gone`, operasi `succeeded`, `lastExit` `forced`. Memverifikasi **AC-4**.
- Crash saat stopping: child fake keluar karena `SIGSEGV` saat stop, `lastExit` `crashed`. Memverifikasi **AC-5**.
- Restart dengan lock tertahan: helper fixture memegang `instance.lock` setelah proses utama keluar, `supervisor.data_in_use` setelah `RESTART_LOCK_WAIT`, tidak ada spawn kedua. Memverifikasi **AC-8**.
- Restart dengan stop timeout: desired `stopped`, lifecycle `stopping`, exit kemudian berakhir `stopped` tanpa start. Memverifikasi **AC-8**.
- Serialisasi: dua client mengirim start, restart, dan stop bersamaan berulang kali (waktu tokio dijeda, dengan port lock dan proses fake yang deterministik supaya pemanggilan `blocking()` tidak membuat test flaky), tidak pernah dua proses atau dua pemegang lock; stop selama restart `resource.busy`. Memverifikasi **AC-9**.
- Reload: PostgreSQL 18 nyata tetap PID sama dan ready; NATS nyata `supervisor.capability_unsupported` dan `operation.list` tidak bertambah. Memverifikasi **AC-10**, **AC-11**.
- Offline: harness spec 0021 AC-10 untuk keempat aksi. Memverifikasi **AC-12**.
- CLI: `stop --force` tanpa terminal dan tanpa `--yes` gagal `cli.confirmation_required` tanpa request ke daemon; `restart --json` dan `reload --json` sama dengan fixture kontrak. Memverifikasi **AC-13**.
- GUI: test `InstanceLifecycleActions.resolve` untuk setiap baris *Alasan nonaktif*; snapshot toolbar, banner, konfirmasi, dan baris Last exit. Memverifikasi **AC-14** sampai **AC-18**.
- Migrasi: registry v5 dimigrasi ke v6 dengan `last_exit` null, CHECK menolak nilai lain. Memverifikasi **AC-5**.

### Usulan update untuk spec lain

- Spec 0010 AC-15 dan *Key invariants*: kalimat "`SIGKILL` hanya bisa dikirim lewat `HelperHandle` atau `SuspendedChild::discard`, tidak pernah ke proses utama yang sudah berjalan" ditambah pengecualian bernama: `force_kill` dari `supervisor::force` saat `stopping` setelah pemeriksaan ulang identitas (spec 0026). *Stop* langkah 4 "force milik fitur 26" menunjuk spec ini. Follow up fitur 26 di spec 0010 dicentang.
- Spec 0010 *Stop* langkah 5: handler exit menulis `ExitKind`.
- Spec 0005: kolom `instances.last_exit` dan migrasi 0006.
- Spec 0069: baris `restartInstance` dan `reloadInstance` menjadi built, ⌥⌘R di baris `restartInstance` dihapus (kebijakan interaksi 2026-10-24), dan baris baru `forceStopInstance` tanpa shortcut.
- Spec 0007: query baca `force_intent(instance)` di engine, yang berjalan di thread registry di dalam commit `exited`.
- Spec 0021 AC-12: Restart dan Reload tampil di toolbar.

### Selisih mockup yang disengaja

- Reload aktif pada `running` dengan readiness apa pun; mockup hanya saat ready. Scope dan spec menang (alasan: config yang membuat instance tidak ready justru perlu reload).
- Restart juga aktif pada `recovering`.
- Tooltip unsupported menambah saran restart (AC-026).
- Banner "Still stopping." dan Force Stop… tidak ada di mockup; asalnya PRD bagian 19.3.
- Toast mockup "Reload signal sent" menjadi kartu umpan balik setelah reload terverifikasi, bukan setelah sinyal terkirim.

## Build plan

Pendekatan Journey: journey stop, restart, dan reload diselesaikan lengkap (state, error, CLI, GUI, bukti) sebelum fitur 27. Urutannya dari dalam ke luar supaya setiap milestone bisa dites sendiri.

**Milestone 1: Domain, registry, platform, dan core** (needs fitur 10 dan 7 di `01-foundation.md`, fitur 25 di `03-multi-versi-dan-instance.md`)

1. `ExitKind` dan field `last_exit` di aggregate instance, `LifecycleEvent::Exited` membawa `ExitKind`, `OperationKind::ForceStopInstance` dengan `CloseInterrupted`; unit test domain. Memenuhi **AC-5**, **AC-2**.
2. Migrasi `0006_instance_last_exit.sql`, mapping storage, fixture registry v5, dan test migrasi. Memenuhi **AC-5**.
3. Port `force_kill` di `stackly-platform`, implementasi di `stackly-platform-macos/src/process/kill.rs`, fake di testkit, dan aturan `FORCE_KILL_ALLOWED` di `cargo xtask check-deps` dengan crate fixture yang melanggar. Memenuhi **AC-3**.
4. `OperationEngine::force_intent` (spec 0007) dan tabel `ExitKind` di setiap jalur yang menulis `exited`: handler exit, `settle_stopping`, `settle_gone_process`, rekonsiliasi, dan recheck `unknown`. Memenuhi **AC-4**, **AC-5**.
5. `supervisor::force` dan `force_stop_work`, dengan konstanta di `SupervisorConfig`; test core dengan fake. Memenuhi **AC-2**, **AC-3**, **AC-4**.
6. `supervisor::restart_op` dan `restart_work` (desired dan reset restart policy di journal stop, sub kasus `recovering`, tunggu exit, tunggu lock dengan blocker `DataInUse`, start, cancel); test core dengan fake. Memenuhi **AC-6**, **AC-7**, **AC-8**.
7. `execute_reload` memakai kode lifecycle AC-10 dan aturan verify untuk instance yang belum ready. Memenuhi **AC-10**.

**Milestone 2: Kontrak, daemon, CLI, dan bukti fixture** (after milestone 1)

8. DTO dan params baru, tiga baris `methods!`, kode error baru, `ProtocolVersion` 1.16, fixture kontrak, `cargo xtask codegen`, dan `cargo xtask fuzz seed`. Memenuhi **AC-11**, **AC-13**.
9. Handler `instance.restart`, `instance.reload`, `instance.force_stop` di `stacklyd` (replay, precheck, start operation) dan mapping `capabilities` serta `lastExit` di `InstanceSummary`. Memenuhi **AC-2**, **AC-6**, **AC-7**, **AC-10**, **AC-11**.
10. Test daemon lewat socket: force, restart, reload, serialisasi bersamaan, dan harness offline spec 0021. Memenuhi **AC-1**, **AC-2**, **AC-4**, **AC-8**, **AC-9**, **AC-12**.
11. CLI `restart`, `reload`, `stop --force` dengan konfirmasi, completion, man page, dan test CLI. Memenuhi **AC-13**.

**Milestone 3: GUI** (after milestone 2; needs fitur 21 dan 22 di `02-instalasi-pertama.md`, fitur 67, 68, dan 69 di `ui-macos.md`)

12. `InstanceLifecycleActions.resolve` dan test Swift untuk setiap alasan. Memenuhi **AC-14**, **AC-15**.
13. Tombol Restart dan Reload di toolbar detail, banner stopping, konfirmasi Force Stop, kartu umpan balik, dan baris Last exit. Memenuhi **AC-14**, **AC-16**, **AC-17**, **AC-18**.
14. Item menu Instance Restart, Reload, dan Force Stop… di peta perintah. Memenuhi **AC-15**.
15. Fixture tampilan spec 0068 untuk state baru dan snapshot review. Memenuhi **AC-14** sampai **AC-18**.

**Milestone 4: Provider nyata di Mac ini** (after milestone 2, dan milestone 3 untuk bukti GUI; needs fitur 19 dan 23 di `02-instalasi-pertama.md`; artefak PostgreSQL 18 dan NATS di cache runtime xtask, selain itu `BLOCKED`)

16. Native test PostgreSQL 18: stop dengan koneksi aktif tanpa `SIGKILL` dan `lastExit` `clean`, restart dengan PID baru dan data sentinel utuh, reload dengan PID sama dan ready. Memenuhi **AC-1**, **AC-6**, **AC-10**.
17. Native test NATS: reload ditolak `supervisor.capability_unsupported` tanpa operasi, restart berhasil. Memenuhi **AC-10**, **AC-11**.
18. Run native dengan jaringan mati (PostgreSQL lewat GUI dan CLI) setelah persetujuan user; `BLOCKED` sampai disetujui. Memenuhi **AC-12**.

## Consequences

**Positive**:

- Stop, restart, dan reload jujur dan sama di GUI, CLI, dan daemon; NATS tidak pernah mengklaim reload.
- `SIGKILL` ke proses utama tetap sempit: satu port, satu modul pemanggil, satu lifecycle, ditegakkan `check-deps`.
- Fitur 49 bisa menolak label backup clean dari `last_exit` tanpa mencari histori operasi.
- Restart di daemon berarti CLI dan GUI tidak perlu merangkai stop lalu start sendiri, dan daemon mati di tengah restart dipulihkan dari desired state.

**Negative / tradeoffs**:

- Satu migrasi registry dan minor bump protokol; client lama tetap bisa bicara (major sama) tetapi tidak melihat field baru.
- Force tidak menyentuh anggota group yang tersisa; child yang bandel tetap harus kamu bereskan sendiri, dan restart bisa gagal `supervisor.data_in_use` karenanya.
- `clean` berarti "keluar saat stopping tanpa sinyal yang tidak dikirim daemon", bukan bukti bahwa provider menulis checkpoint. Untuk proses adopsi status exit tidak diketahui, jadi proses adopsi yang dibunuh pihak lain saat stopping tetap tercatat `clean`.
- Daemon yang mati setelah journal force tetapi sebelum langkah `kill` mulai membuat force tidak terjadi; rekonsiliasi mengirim stop normal. Itu benar (tidak ada `SIGKILL`), tetapi kamu perlu menekan Force Stop… lagi.
- Handler exit membaca journal operasi di commit `exited`; biayanya satu query engine per exit.
- Stop selama restart ditolak, jadi kamu butuh dua langkah (cancel, lalu stop) saat ingin membatalkan restart.
- Restart dengan lease environment tidak meminta override; fitur 44 mungkin perlu mengetatkannya.

**Neutral**:

- `restart_op.rs` berdampingan dengan `restart.rs` (restart policy); dokumentasi modul membedakan keduanya.
- Konstanta tunggu baru bisa disetel lewat `SupervisorConfig` di test.
- Shortcut ⌥⌘R di spec 0069 tidak dipasang karena kebijakan interaksi 2026-10-24.

## Follow-up

- [ ] Terapkan *Usulan update untuk spec lain* ke spec 0010, 0005, 0069, dan 0021 saat milestone yang bersangkutan dibangun.
- [ ] Fitur 49: backup cold menolak label clean saat `last_exit` bukan `clean`.
- [ ] Fitur 29: Apply and Reload atau Apply and Restart memakai `instance.reload` dan `instance.restart` dari spec ini.
- [ ] Fitur 44: tinjau apakah restart pada instance yang dipinjam environment butuh override.
- [ ] `/sync` setelah build: catat `force_kill`, `FORCE_KILL_ALLOWED`, `last_exit`, dan tiga method baru di root `AGENTS.md`.
