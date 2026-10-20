# Review, claude/chk-f10-review-c3f69b (fitur 10 Process supervisor, spec 0010), 2026-10-20

**Reviewed by**: Claude Fable 5.1, lima subagent paralel, satu per irisan (author on Claude Opus 5.5 dan Claude Sonnet 5.5; Fable satu satunya family yang tidak ikut menulis). Konsolidasi dan verifikasi temuan Major oleh Claude Opus 5.5 (sesi `chk-f10-review`) terhadap kode di worktree.
**Scope**: 61 file, rentang `4311e5c^..8fe3a27` dibatasi ke path fitur 10 (worktree di `main` `a0537aa`, yang sudah memuat rentang itu). Daftar file dari `git diff --name-only 4311e5c^..8fe3a27 --` path di brief, ditambah `crates/domain/src/supervisor.rs` (tipe domain supervisor dari `4311e5c`, tidak tercakup pola `crates/domain/src/process*`). `crates/protocol/src/codes.rs` hanya bagian kode `supervisor`, `process`, dan `instance`; `xtask/src/check_deps*` hanya aturan process, `SIGKILL`, dan `fixture`.
**Verdict**: Changes requested (4 Major, tanpa Blocker)
**Status setelah fix** (`036a9b8..fa6d6a1`): Changes requested. Major 2, 3, dan 4 tertutup; Major 1 masih terbuka untuk satu urutan kejadian yang belum ditangani; dua Minor dan tiga Nit baru dari diff fix. Rincian di *Konfirmasi fix* di bawah.
**Status setelah fix putaran 2** (`d34e762..b2c7fae`): Approve with nits. Keempat Major tertutup, tanpa Blocker atau Major yang terbuka; dua Minor dari putaran 1 tertutup; satu Nit baru dan satu Nit putaran 1 (flag `degraded` tunggal) tetap terbuka. Rincian di *Konfirmasi fix putaran 2*.

## Konfirmasi fix putaran 2 (d34e762..b2c7fae)

**Dikonfirmasi oleh**: Claude Opus 5.5 (sesi `chk-f10-review`), inline. Commit fix `a2f3710` ditulis Claude Sonnet 5.5, jadi reviewer beda family. Worktree di `git merge main` (memuat `b2c7fae`); diff dibaca dengan `git diff d34e762..b2c7fae` (9 file kode, 4 file test). Tidak ada build atau test di sesi ini; gate lead di `main` (fmt, clippy, check-deps, cek Windows, core 16 target, `stacklyd` 19 target, evidence, tanpa proses yatim) dipakai sebagai bukti test baru lulus.

| Butir putaran 1 | Hasil | Bukti |
| --- | --- | --- |
| Major 1, urutan yang tersisa | tertutup | `stop.rs:161`, test `a_stop_whose_process_exit_was_handled_before_its_journal_still_ends_stopped` |
| Minor, loop baca `respawn` dan `pending` | tertutup | `respawn.rs:109`, `pending.rs:100`, test `reconcile::a_registry_read_that_fails_for_a_pending_run_is_tried_again_and_the_respawn_goes_on` |
| Minor, task exit dan recheck memegang `Supervisor` kuat | tertutup | `exit.rs:38`, `recheck.rs:31`, test core `a_shut_down_supervisor_lets_go_of_the_registry_though_it_adopted_a_live_process` dan test daemon yang kini menjalankan `Supervision::reconcile` |
| Nit, crash fantom di riwayat restart | tertutup, dengan sisa kecil | `startup.rs:1288` (`forget_last_crash`), test `a_crash_taken_back_uses_no_budget`; lihat Nit baru |
| Nit, `Next::Settled` di start menjadi `internal.unexpected` | tertutup | `startup.rs:702`, kini `supervisor.instance_stopping` (sudah di katalog) |
| Celah test, cancel untuk Major 3 | tertutup | test `a_cancel_after_the_render_and_before_any_process_runs_removes_the_secret_material` |
| Nit, flag `degraded` tunggal | masih terbuka | tidak dikerjakan; developer mengusulkan pekerjaan terpisah. Tidak menahan merge |

**Major 1**: `signal_and_wait` kini mengembalikan `StopEnd::Stopped` bila slot proses sudah kosong (`stop.rs:161`), dan `stop_process` lalu menjalankan `settle_stopping`, yang menulis `Exited` untuk `stopping`. Saya cek kedua pemanggil `stop_process` (`run_stop` di `stop.rs:423` dan `cancel_cleanup` di `startup.rs:1312`) hanya sampai di sana setelah melihat proses: `run_stop` lebih dulu menolak `has_process == false` untuk `starting`, `running`, dan `stopping`, dan `cancel_cleanup` hanya memanggilnya bila `has_process`. Slot kosong di titik itu berarti handler exit sudah selesai (slot baru dikosongkan `finish_process`, sesudah commit exit), jadi `settle_stopping` membaca aggregate yang final. Respawn tidak bisa mengisi slot di antaranya, karena operasi stop memegang lock instance dan respawn memakai `BusyPolicy::Reject`; jeda respawn yang dijadwalkan sesudah `cancel_respawn` membaca `stopped` dan berhenti. Test baru menahan stop di tahap `journal` (`FaultyRegistry::hold_stage`), membiarkan handler exit selesai penuh (`recovering`, slot kosong), lalu melepasnya, persis urutan yang saya laporkan; developer menyebut test ini gagal di kode putaran 1. Hasilnya operasi `succeeded` dan instance `stopped` dengan desired `stopped`.

**Minor loop baca**: kedua tempat kini memakai `read_instance_actor`, jadi jadwal (`Busy` dengan jeda berlipat, `NoSpace`, `Unavailable`, dan `Io` tiap `REGISTRY_RETRY_INTERVAL`), flag `degraded`, dan aturan menyerah sama dengan commit actor. Error permanen kini menghentikan respawn atau run tertunda dengan satu `warn`, tidak lagi mengulang tiap detik. Test baru untuk `pending::run_one` mengakui sendiri bahwa ia cakupan, bukan regresi; itu cukup untuk Minor.

**Minor handle `Supervisor`**: task exit (`exit.rs:38`) dan recheck (`recheck.rs:31`) kini memegang `WeakSupervisor` dan baru `upgrade` saat exit tiba atau saat bangun, jadi setelah `shutdown()` dan `close()`, handle kuat terakhir ada di tangan daemon. Test core memeriksa jumlah handle registry kembali seperti semula setelah supervisor kedua mengadopsi proses hidup lalu dimatikan; test daemon kini menjalankan rekonsiliasi (task recheck) sebelum `close()`.

### Temuan baru putaran 2

- ⚪ `crates/core/src/supervisor/startup.rs:1267`, di cabang `Exhausted`, `restart_status` diisi `exhausted` sebelum hasil commit diketahui. Bila commit menghasilkan `stopped` (stop mendarat tepat di crash keenam), `forget_crash` mengembalikan budget, tetapi status restart instance tetap terbaca `exhausted` pada instance `stopped` sampai status itu dihitung ulang. Isi status hanya untuk hasil `Crashed`, atau hitung ulang setelah `forget_crash`.

Tidak ada temuan Blocker, Major, atau Minor baru di diff putaran 2. Tindak lanjut di luar kode yang tetap berlaku: spec 0010 *Asumsi lintas spec* perlu diperbarui lewat `/architect` agar menyebut keputusan exit di dalam commit actor dengan CAS revision, dan stop yang menemukan prosesnya sudah keluar menutup `stopping` sendiri, menggantikan kalimat tentang `LockTable::holder` dan `try_lock_internal`.

## Konfirmasi fix (036a9b8..fa6d6a1)

**Dikonfirmasi oleh**: Claude Opus 5.5 (sesi `chk-f10-review`), inline. Ketiga commit fix (`6b90aaf`, `905bb9a`, `b9ccf0f`) ditulis Claude Sonnet 5.5, jadi reviewer beda family dari penulis fix. Worktree di `git merge main` sampai `fa6d6a1`; diff dibaca dengan `git diff 036a9b8..fa6d6a1` (13 file kode dan test, ditambah log koordinasi). Tidak ada build atau test yang dijalankan di sesi ini; gate lead di `main` (fmt, clippy, check-deps, cek Windows, test core dan `stacklyd`, evidence) dipakai sebagai bukti bahwa test baru lulus.

| Major | Hasil | Ringkasan |
| --- | --- | --- |
| 1, stop di tengah commit crash | **masih terbuka** (satu urutan) | urutan yang diuji sudah benar; urutan sebaliknya masih meninggalkan `stopping` |
| 2, bacaan registry yang gagal | tertutup | Minor baru: loop baca di `respawn` dan `pending` tidak pernah menyerah |
| 3, secret material setelah start gagal | tertutup | test memakai paket yang merender ke `${instance.secrets}` |
| 4, siklus `Arc` di daemon | tertutup | Minor baru: task exit dan recheck masih memegang `Supervisor` kuat setelah `shutdown()` |

### Major 1, masih terbuka untuk satu urutan, `crates/core/src/supervisor/stop.rs:384`, `crates/core/src/supervisor/stop.rs:158`

**Yang sudah benar**: event exit kini dipilih di dalam closure `build` dari aggregate yang baru dibaca (`persist.rs` `apply_exit`): `stopping` menjadi `Exited`, lifecycle berjalan dengan desired `stopped` menjadi `StopBegin` lalu `Exited`, selain itu crash. Konflik revision membuat commit actor membaca ulang dan memutuskan lagi, jadi stop yang menulis `stopping` di antara keputusan dan commit actor dihormati. `apply_policy` mengembalikan `Next::Settled` (tanpa respawn) bila commit menghasilkan `stopped`, `fail_lifecycle` menangani `stopping`, dan `stop_process` memanggil `settle_stopping` setelah `done` sebagai jaring pengaman. Test `actor_commit::a_stop_that_lands_in_the_middle_of_a_crash_commit_ends_stopped_not_stopping` menahan commit crash, membiarkan stop menulis `stopping`, lalu melepasnya; hasilnya `stopped` dan operasi `succeeded`.

**Problem**: `run_stop` membaca `has_process` sekali di tahap `lock` (`stop.rs:384`), lalu menulis langkah journal dan `journal_stop` (`:404`), baru kemudian `stop_process` membaca slot proses di `signal_and_wait`. Bila slot sudah kosong di titik itu, ia mengembalikan `IdentityUnproven { pid: None }` (`:158`), dan `settle_stopping` tidak jalan karena hanya dipanggil untuk `Ok(StopEnd::Stopped)` (`:120`). Urutan yang memicunya: proses keluar sendiri; stop pengguna membaca `has_process == true`; handler exit menyelesaikan commit crash (`recovering`, `will_retry`) dan `finish_process` (slot kosong, `done` menyala) selama stop masih menulis langkah journal; `journal_stop` membaca `recovering` dan menulis `stopping` (`no_process` false karena `has_process` true); `signal_and_wait` menemukan slot kosong dan operasi stop gagal `supervisor.identity_unproven`. Jeda respawn yang dijadwalkan handler exit lalu membaca `stopping` dan berhenti. Hasilnya sama dengan Major semula: `stopping`, desired `stopped`, tanpa proses dan tanpa operasi; `start` menjawab `supervisor.instance_stopping` dan `stop` menjawab `supervisor.identity_unproven` sampai daemon restart. Varian yang lebih ringan: bila stop menulis `stopping` lebih dulu dan handler exit menyelesaikan commit `Exited` plus `finish_process` sebelum `signal_and_wait` membaca slot, lifecycle benar `stopped` tetapi operasi stop tetap gagal dengan `supervisor.identity_unproven`. Jendelanya beberapa milidetik (beberapa tulisan journal), sama besar dengan jendela semula. Ditemukan dengan membaca kode; test yang ada tidak menempuh urutan ini karena pintunya menahan commit crash sebelum `finish_process`.
**Suggested fix**: Di `stop_process`, bila `run_stop` melihat proses tetapi slot sudah kosong (exit nya sudah tiba), anggap proses sudah berhenti: kembalikan `StopEnd::Stopped` dan jalankan `settle_stopping`, yang menulis `Exited` untuk `stopping`. Alternatif: `run_stop` membaca ulang slot sesudah `journal_stop` dan memakai jalur `no_process` bila sudah kosong. Tambah test yang membiarkan handler exit selesai penuh (commit dan `finish_process`) di antara pembacaan `has_process` dan tahap `stop`.

**Penyimpangan dari teks spec**: developer sengaja tidak memakai `LockTable::holder` atau `try_lock_internal`. Menurut saya ini bisa diterima: operasi stop memegang `LockKey::Instance` selama menunggu handler exit, jadi `try_lock_internal` di handler exit justru akan gagal di kasus yang paling penting, sedangkan keputusan di dalam commit ditambah CAS revision memberi jaminan yang spec inginkan ("event diterapkan lagi kalau masih sah", AC-22). Celah sisa di atas bukan akibat tidak adanya lock, tetapi karena jalur stop memakai bacaan slot yang basi. Spec 0010 perlu diperbarui lewat `/architect`: kalimat di *Asumsi lintas spec* ("Actor memakai `LockTable::holder` ... dan `try_lock_internal` untuk menulis lifecycle setelah crash") diganti aturan bahwa commit actor memutuskan event dari aggregate yang dibacanya, dengan CAS revision, dan bahwa stop yang menemukan prosesnya sudah keluar menutup `stopping` sendiri.

### Major 2, tertutup, `crates/core/src/supervisor/persist.rs:91`, `crates/core/src/supervisor/exit.rs:115`, `crates/core/src/supervisor/respawn.rs:122`, `crates/core/src/supervisor/pending.rs:105`

`commit_instance` mode `Actor` kini mengalirkan bacaan ulang yang gagal lewat `ActorCommitRetry::next` dan `wait_out` (degraded, tunggu, baca lagi; menyerah hanya untuk error yang tidak bisa disembuhkan), `read_instance_actor` membungkus bacaan actor dengan aturan yang sama, `settle` tidak lagi menurunkan event dari bacaan yang gagal (memori `stopping` juga tidak lagi dipakai), dan `after_unrequested_exit` memakai bacaan actor di mode `Actor`. Akhir jeda respawn dan `pending::run_one` mengulang bacaan yang gagal alih alih membuang respawn. Dua test baru (`a_registry_read_that_fails_when_a_process_exits_is_tried_again_and_the_crash_respawns`, `a_registry_read_that_fails_at_the_end_of_a_pause_does_not_drop_the_respawn`) menempuh dua dari tiga jalur; `pending::run_one` belum punya test sendiri (dicatat di bawah).

### Major 3, tertutup, `crates/core/src/supervisor/startup.rs:787`, `crates/core/src/supervisor/startup.rs:1289`

`fail()` memanggil `remove_material` sebelum menulis lifecycle, dan `cancel_cleanup` memanggilnya bila tidak ada proses. Saya cek setiap pemanggil `fail()` berada sesudah tahap `lock` (baris 916 sampai 1072, lock diambil di `:351`), jadi lock instance dipegang dan tidak ada proses lain milik instance itu yang bisa kehilangan file secret nya. Test `start::a_start_that_fails_to_spawn_removes_the_secret_material_it_rendered` memakai paket yang merender ke `${instance.secrets}` dan mengharapkan `spawn_failed`, kode yang hanya muncul setelah render berhasil menulis, jadi test nya tidak lolos secara kosong. Jalur cancel belum punya test.

### Major 4, tertutup, `apps/daemon/src/supervisor.rs:120`, `crates/core/src/supervisor/deps.rs:167`

`BusSupervisorEvents` kini memegang `WeakSupervisor`, `Supervision.events` menjadi `Mutex<Option<..>>` yang dikosongkan di `close()`, jadi siklus kuat `Shared` ke events ke `Supervisor` hilang. Test `closing_the_supervision_lets_go_of_the_registry_the_supervisor_held` membuktikannya untuk supervisor tanpa actor. Tanpa siklus, handle registry yang tersisa paling lambat lepas saat `runtime.shutdown_timeout` di `apps/daemon/src/startup.rs:388` membuang task, sebelum `release(lock)` di `:390`, jadi registry tertutup sebelum lock daemon dilepas. Sisa celahnya lebih kecil dan dicatat sebagai Minor baru di bawah.

### Temuan baru dari diff fix

#### 🟡 Task exit dan recheck masih memegang `Supervisor` kuat setelah `shutdown()`, `crates/core/src/supervisor/exit.rs:29`, `crates/core/src/supervisor/recheck.rs:28`
**Problem**: `spawn_exit_task` memindahkan `sup: Supervisor` ke task yang menunggu `exit.wait_for(Option::is_some)`, yang baru selesai saat proses keluar atau listener watcher dibuang; `Supervisor::shutdown` (lewat `stop_background`) menghentikan probe, drain, dan jeda respawn, tetapi tidak task ini. Proses instance sengaja dibiarkan hidup saat daemon berhenti, jadi setiap instance yang berjalan menyisakan satu task dengan handle kuat. Task recheck (`recheck.rs:28`, dimulai setiap rekonsiliasi) juga memegang klon kuat dan baru memeriksa `is_shutdown` setelah tidur `UNKNOWN_RECHECK_INTERVAL` (30 detik).
**Why it matters**: Di run daemon biasa (selalu ada rekonsiliasi, biasanya ada instance berjalan), `core.take_registry()` di `apps/daemon/src/server.rs:466` masih bukan handle terakhir, berlawanan dengan komentarnya ("Dropping the last handle joins the registry thread"); penutupan pindah ke `runtime.shutdown_timeout`. Untuk `stacklyd` itu masih sebelum lock dilepas, jadi data dan urutan stop aman. Untuk server di dalam proses (`stacklyd::server::start` di test), registry lama tetap terbuka selama proses fake masih hidup di runtime yang sama. Test baru tidak menempuh rekonsiliasi atau actor, jadi tidak melihat ini.
**Suggested fix**: Beri task exit dan recheck `WeakSupervisor` (upgrade saat exit tiba atau saat bangun; bila gagal, selesai), atau `select!` pada notifikasi shutdown. Perluas test daemon dengan rekonsiliasi dan satu proses fake yang diadopsi.

#### 🟡 Loop baca di akhir jeda respawn dan di `pending::run_one` tidak pernah menyerah, `crates/core/src/supervisor/respawn.rs:133`, `crates/core/src/supervisor/pending.rs:105`
**Problem**: Kedua loop mengulang setiap `SupervisorError::Registry(_)` tanpa batas, setiap `restart_base_delay` (1 detik), dengan satu log `warn` per putaran, termasuk error yang `ActorCommitRetry` anggap tidak bisa disembuhkan (`GiveUp`, misalnya baris `Corrupt`). Jalur lain di fix ini (`read_instance_actor`, `commit_instance`) memakai `ActorCommitRetry` dengan `REGISTRY_RETRY_INTERVAL` 5 detik dan menyerah untuk error permanen. `run_one` juga memegang satu dari empat permit rekonsiliasi selama loop.
**Why it matters**: Satu baris instance yang rusak membuat log berisi peringatan setiap detik selamanya dan menahan satu permit start rekonsiliasi; selama gangguan registry, jadwal coba ulang tidak sama dengan AC-22.
**Suggested fix**: Pakai `read_instance_actor` (atau `ActorCommitRetry` dan `wait_out`) di kedua tempat, sehingga jadwal, flag `degraded`, dan aturan menyerah sama dengan commit actor. Tambah test untuk `pending::run_one`.

#### Nit baru
- ⚪ `crates/core/src/supervisor/startup.rs:1225`, `apply_policy` mencatat crash di riwayat restart (`record_crash`) sebelum commit memutuskan; bila commit menghasilkan `stopped` (`Next::Settled`), riwayat tetap memuat crash yang sebenarnya stop. Start pengguna dari `stopped` tidak menghapus riwayat (hanya dari `failed`, `startup.rs:235`), jadi crash loop berikutnya dalam 5 menit mendapat satu percobaan lebih sedikit.
- ⚪ `crates/core/src/supervisor/startup.rs:701`, `Next::Settled` di dalam operasi start menjadi `SupervisorError::internal("the instance was stopped while its process exited")`, kode `internal.unexpected`; nyaris tidak terjangkau (stop pengguna ditolak selama start), tetapi kode supervisor yang bernama akan lebih jelas.
- ⚪ `apps/daemon/src/supervisor.rs:310`, flag `degraded` subsystem adalah satu nilai, jadi loop yang pulih menghapus `degraded` milik actor lain yang masih gagal. Sudah ada sebelum fix (`degrade`), tetapi loop baru di `respawn` dan `pending` menambah pengubahnya.

#### Celah test dari fix
- urutan Major 1 yang tersisa (handler exit selesai penuh di antara `has_process` dan tahap `stop`);
- `pending::run_one` dengan bacaan yang gagal;
- `cancel_cleanup` yang menghapus secret material saat tidak ada proses;
- stop daemon dengan rekonsiliasi dan proses yang berjalan, untuk handle registry.

Irisan dan penulisnya:

| Irisan | File | Penulis di rentang |
| --- | --- | --- |
| 1, adapter macOS dan port | `crates/platform/src/process.rs`, `crates/platform-macos/src/process.rs`, `process/{boot,inspect,kill,lock,spawn,spool,stdin,sys,watch}.rs` (11) | Opus |
| 2, runtime core | `crates/core/src/supervisor/{actor,commit,restart,respawn,exit,stop,drain,lock,pending,reload,recheck,status,steps,deps,config,mod,error,codes,readiness}.rs` (19) | campuran |
| 3, startup dan rekonsiliasi | `crates/core/src/supervisor/{startup,reconcile,adopt,init,classify,prepare,persist,evidence,helper,secrets,fixture,ports}.rs` (12) | campuran |
| 4, spool, probe, domain, storage, fake | `supervisor/{spool,sink,http}.rs`, `supervisor/probe/{mod,exec,http,tcp}.rs`, `crates/domain/src/{process,supervisor}.rs`, `crates/storage/src/registry/supervision.rs`, `crates/testkit/src/fake_process.rs` (11) | campuran |
| 5, daemon, protokol, check-deps | `apps/daemon/src/supervisor.rs`, `apps/daemon/src/ipc/instance/{create,dto,mod}.rs`, `crates/protocol/src/instance.rs`, `crates/protocol/src/codes.rs`, `xtask/src/check_deps.rs`, `xtask/src/check_deps/source_scan.rs` (8) | Sonnet, merge `5eace53` Opus |

Catatan proses: disk sempat penuh (`ENOSPC`) saat subagent menulis file irisan, sehingga tiga irisan mengembalikan temuannya lewat pesan dan sesi ini menyalinnya. Tidak ada `cargo build` atau `cargo test` yang dijalankan; review membaca kode dan diff saja. Keempat Major dibaca ulang langsung di kode oleh sesi ini (lokasi dan alur di bawah). Minor dan Nit berasal dari reviewer irisan; beberapa dicek ulang (ditandai "dicek ulang"), sisanya dicatat apa adanya.

## Summary

Fitur 10 menambah port `ProcessSupervision`, adapter macOS dengan `posix_spawn`, kqueue, dan identitas majemuk, supervisor di `stackly-core` (actor, transaksi startup, stop, restart policy, rekonsiliasi, adopsi, spool dan sink, probe), registry observasi proses, fake di `stackly-testkit`, kontrak protokol 1.8 untuk method `instance.*`, wiring daemon, dan aturan `check-deps`. Permukaan yang paling berbahaya kuat: setiap blok `unsafe` punya `// SAFETY:` yang benar, layout `sys.rs` cocok dengan SDK arm64 dan dikunci assert, flag spawn tepat AC-1, `SIGKILL` hanya di `kill.rs`, setiap sinyal didahului pemeriksaan ulang identitas, `Unproven` tidak pernah menghasilkan sinyal (dibuktikan oleh test sapuan tabel `plan_for`), anak tertahan selalu di `discard` sebelum `resume`, redaksi baris utuh sebelum dipotong, dan shutdown daemon tidak menyinyal apa pun. Masalah utama ada di jalur yang jarang: commit actor memutuskan event lifecycle dari bacaan registry yang bisa basi atau gagal, sehingga balapan stop dengan crash atau satu bacaan registry yang gagal meninggalkan instance di state yang hanya sembuh lewat restart daemon; file secret hasil render tidak dihapus saat start gagal sebelum proses berjalan; dan siklus `Arc` kuat di daemon membuat `Supervision::close()` tidak pernah melepas handle registry. Tidak ada temuan yang membuat sinyal salah sasaran, kehilangan data, atau secret masuk argv.

## Major

### 🟠 Stop yang mendarat di tengah commit crash membuat instance macet di `stopping`, `crates/core/src/supervisor/exit.rs:112`, `crates/core/src/supervisor/persist.rs:88`, `crates/core/src/supervisor/stop.rs:159`

**Problem**: Saat proses keluar sendiri, `handle_exit` membaca `state.stopping` di awal, lalu `settle` membaca registry sekali (`exit.rs:119`) dan memilih event dari bacaan itu. Untuk `running` dengan desired `running`, ia memanggil `after_unrequested_exit` (`startup.rs:1151`), yang membaca lagi, mencoba bind port, lalu `apply_policy` memanggil `commit_instance(.., None, CommitMode::Actor, ..)`. `commit_instance` membaca ulang aggregate (`persist.rs:90`) dan menjalankan `build`, dan `Crashed` pada `stopping` tidak ada di tabel lifecycle (`crates/domain/src/instance.rs:134` sampai `:149`), jadi `build(..)?` mengembalikan `Internal` dan commit ditinggalkan (`Next::Final`, hanya log "not restarted"). Urutan yang memicunya: proses keluar sendiri saat `running`; `settle` membaca `running`; operasi stop pengguna mulai (tidak ada operasi yang memegang lock instance), `run_stop` melihat `state.process` masih `Some` (slot baru diambil `finish_process` setelah commit, `exit.rs:98`), `journal_stop` menulis `stopping`, `stop_process` menyinyal PID yang sudah mati dan `NotFound` dianggap "exit sedang datang" (`stop.rs:159`), lalu menunggu `done`. Commit crash membaca `stopping` dan gagal; `finish_process` menyalakan `done`; operasi stop selesai `succeeded`. Tidak ada pihak yang menulis `Exited`. Saya cek: `LockTable::holder` dan `try_lock_internal`, yang spec *Asumsi lintas spec* sebut sebagai cara actor menulis lifecycle setelah crash, tidak dipakai di mana pun di `crates/core/src/supervisor/`.
**Why it matters**: Registry berakhir `stopping`, desired `stopped`, tanpa proses dan tanpa operasi. `instance.start` menjawab `supervisor.instance_stopping` (`startup.rs:219`) dan `instance.stop` menjawab `supervisor.identity_unproven` (`stop.rs:357` sampai `:365`), dua kode yang menyesatkan, sampai daemon restart dan rekonsiliasi mengubah `stopping` plus `Gone` menjadi `stopped`. Jendelanya milidetik, tetapi client otomatis atau stop sistem dari rekonsiliasi bisa mengenainya, dan hasilnya lengket.
**Suggested fix**: Putuskan event di dalam closure `build` dari aggregate yang baru dibaca (`stopping` menjadi `Exited`; `running` atau `recovering` dengan desired `running` menjadi keputusan restart; desired `stopped` menjadi `StopBegin` lalu `Exited`), sesuai AC-22 "event diterapkan lagi kalau masih sah". Pakai `try_lock_internal` atau `holder` di handler exit seperti spec bilang, dan buat `run_stop` menganggap slot yang exit nya sudah tiba sebagai `no_process`. Tambah test core yang membalapkan stop dengan crash di batas `settle`.

### 🟠 Bacaan registry yang gagal di jalur actor dianggap jawaban, bukan dicoba lagi (AC-22), `crates/core/src/supervisor/exit.rs:119`, `crates/core/src/supervisor/persist.rs:90`, `crates/core/src/supervisor/respawn.rs:106`, `crates/core/src/supervisor/pending.rs:98`

**Problem**: Tiga tempat melipat `Err` dari `read_instance` menjadi keputusan state:
1. `settle` (`exit.rs:119` sampai `:150`): bila bacaan gagal dan flag memori `stopping` mati, kedua cabang `is_ok_and` bernilai false, jadi exit jatuh ke `Crashed { will_retry: false, reason: process_exited }`. Bila bacaan ulang di commit berhasil, crash yang semestinya di restart tersimpan sebagai `failed` terminal tanpa respawn.
2. `commit_instance` mode `Actor` dipanggil dengan `base: None`, dan bacaan ulang tiap putaran memakai `read_instance(deps, id).await?` (`persist.rs:90`), jadi bacaan yang gagal keluar dari loop seketika, tanpa `Wait`, `Degraded`, atau status `degraded`. Jadwal coba ulang AC-22 hanya berlaku untuk tulisan yang gagal. Bila exit tidak pernah tercatat, registry tetap `running` tanpa proses: stop menjawab `identity_unproven`, start tidak melakukan apa pun (`startup.rs:202` sampai `:218`).
3. Akhir jeda respawn (`respawn.rs:106` sampai `:115`): `wanted` bernilai false untuk `Err`, `clear_pause` menghapus handle cancel dan `nextAttemptAt`, lalu task selesai. Instance tetap `recovering`, desired `running`, tanpa proses, tanpa timer, selamanya; recheck 30 detik hanya mengurus `unknown` dan `dataInUse` (`recheck.rs:47` sampai `:66`). Bentuk yang sama di `pending.rs:98` sampai `:100` (run rekonsiliasi dibuang).
**Why it matters**: AC-22 menyebut `Unavailable` sebagai keadaan yang membuat subsystem `degraded` sambil dicoba lagi tiap 5 detik; registry yang tidak tersedia biasanya menggagalkan bacaan juga. Gangguan registry sesaat pada daemon yang berjalan tanpa diawasi berubah menjadi lifecycle permanen yang salah (`failed`), exit yang tidak tercatat, atau instance `recovering` yang tidak pernah dijalankan lagi, bertentangan dengan AC-7 dan invariant restart policy, tanpa petunjuk di status. Test AC-22 hanya menolak commit, tidak menolak bacaan.
**Suggested fix**: Jangan pernah menurunkan event dari bacaan yang gagal; biarkan closure `build` yang memutuskan (sama dengan perbaikan Major pertama). Di mode `Actor`, alirkan bacaan ulang yang gagal lewat `ActorCommitRetry::next` alih alih `?`. Di `respawn` dan `pending::run_one`, perlakukan `Err` seperti `Begun::Later` (tidur `restart_base_delay`, ulangi, jeda tetap aktif) dengan log `warn`. Tambah kegagalan bacaan ke test AC-22 dan ke test jeda respawn.

### 🟠 File secret hasil render tertinggal setelah start gagal sebelum proses berjalan, `crates/core/src/supervisor/startup.rs:901`, `crates/core/src/supervisor/startup.rs:772`, `crates/core/src/supervisor/startup.rs:1255`

**Problem**: `run_attempt` menjalankan `run_render` (`startup.rs:901`, isi di `steps.rs:111`), yang untuk template `sensitive` bertujuan `${instance.secrets}` (mekanisme 5 `config_plaintext`, `crates/core/src/secret/delivery/mod.rs:13` sampai `:17`) menulis config berisi secret ke folder `secrets/` instance. Setiap kegagalan sesudahnya dan sebelum `resume` (`command_parts` di `:907`, `fresh_spools` di `:916`, `spawn_main` di `:937` sampai `:944`, `boot_id` di `:958`, commit identitas, `watch_exit`, `resume`) kembali lewat `fail(..)` (`:772`), yang hanya menulis lifecycle; cancel di `enter(SPAWN)` atau `enter(IDENTITY)` kembali lewat `cancel_cleanup` (`:1255`). Tidak ada yang memanggil `remove_material`; fungsi itu hanya dipanggil dari `on_exit` (`:1130`) dan `exit::handle_exit`, keduanya butuh proses yang keluar. Render yang gagal di tengah (file kedua gagal) juga meninggalkan file pertama. Spec 0010 *Stop*: "`remove_secret_material` juga dipanggil setiap kali proses utama keluar tanpa respawn (`crash_loop`, `start_failed`, `endpoint_in_use`)", dan `start_failed` persis yang jalur ini catat.
**Why it matters**: Setelah `instance.start` gagal dengan `executable_invalid`, `spawn_failed`, atau `identity_capture_failed`, atau di cancel di antara `render` dan `spawn`, config secret plaintext tetap di disk (0600 di runtime root 0700) sampai sapuan saat daemon start berikutnya atau sampai proses berikutnya keluar. Saat ini laten: saya cek tidak ada paket di `providers/` yang merender ke `${instance.secrets}` (fixture `fake-command` memakai `materializeSecret`, mekanisme 3), jadi tidak ada test yang melihatnya. Provider nyata pertama yang memakai mekanisme 5 (fitur 19, 23) akan kena.
**Suggested fix**: Panggil `remove_material` dari `fail()` setelah `run_render` pernah jalan (atau di setiap `return Ok(fail(..))` sesudahnya) dan dari `cancel_cleanup` bila `has_process` false; jangan di jalur readiness timeout, tempat proses hidup dengan sengaja. Tambah test core dengan paket `config_plaintext` dan spawn yang gagal, lalu pastikan `<run>/secrets/` kosong.

### 🟠 `Supervision::close()` tidak bisa melepas registry karena siklus `Arc` kuat `Supervisor` dan `BusSupervisorEvents`, `apps/daemon/src/supervisor.rs:101`, `apps/daemon/src/supervisor.rs:229`, `apps/daemon/src/supervisor.rs:245`

**Problem**: `Supervision::build` membuat `BusSupervisorEvents` yang memegang `supervisor: Arc<OnceLock<Supervisor>>` (`:101` sampai `:106`), memasangnya sebagai `deps.events` supervisor (`:117`), lalu mengisi `OnceLock` itu dengan `supervisor.clone()` (`:120`). `Supervisor` adalah `Arc<Shared>` dan `Shared.deps.events` adalah objek event itu (`crates/core/src/supervisor/deps.rs:93`, `:134`), jadi `Shared` ke events ke `OnceLock<Supervisor>` ke `Shared` adalah siklus kuat. `Supervisor::shutdown` (`deps.rs:207`) hanya menyalakan flag dan menghentikan task latar; tidak melepas apa pun. `Supervision.events` juga `OnceLock` yang tidak pernah dikosongkan, dan `close()` (`:229` sampai `:231`) hanya menulis ulang `slot`. `Shared.deps` memegang klon `SqliteRegistry` (`server.rs:720`), `SecretService`, dan engine operasi.
**Why it matters**: `server.rs:448` sampai `:452` dan `:466` sampai `:469` mengandalkan bahwa setelah `close()` handle di `take_registry()` adalah yang terakhir ("Dropping the last handle joins the registry thread, which checkpoints and closes the file"), dan `server.rs:700` sampai `:704` sengaja memakai `Weak` supaya secret worker tidak menahan registry melewati stop. Dengan siklus ini, stop bersih `stacklyd` melewatkan join thread registry, checkpoint WAL, dan penutupan file; thread mati saat proses keluar (data commit tetap aman karena WAL). Setiap server di dalam proses (`stacklyd::server::start` di test) membocorkan satu supervisor, actor nya, secret service, dan handle SQLite per run. Fitur 30 (restart dan serah terima daemon) bergantung pada penutupan yang bersih.
**Suggested fix**: Beri `BusSupervisorEvents` handle lemah (`Weak<Shared>` lewat API core, atau kirim `InstanceStatus` ke `instance_changed` supaya objek event tidak butuh supervisor), dan buat `close()` melepas slot sekaligus handle events (`Mutex<Option<Arc<..>>>` alih alih `OnceLock`). Tambah test stop yang memastikan jumlah handle registry tinggal satu sebelum `take_registry()`.

## Minor

### Irisan 1, adapter macOS dan port

#### 🟡 Thread watcher yang mati karena error `kevent` meninggalkan kqueue tanpa pembaca, diam diam, `crates/platform-macos/src/process/watch.rs:535` (dicek ulang)
**Problem**: `kevent` yang gagal selain `EINTR` membuat `Shared::run` kembali (`:535` sampai `:541`), tetapi `Watcher::shared` tetap diberikan ke setiap `start()` (`:73` sampai `:77`), jadi `spawn_main`, `spawn_helper`, dan `watch_exit` terus mendaftarkan `NOTE_EXIT` ke kqueue yang tidak dibaca siapa pun. Tidak ada log dan tidak ada error.
**Why it matters**: Satu `ENOMEM` atau `EBADF` dan, sampai daemon berhenti, tidak ada exit proses utama yang teramati (instance tetap `running`, tanpa respawn, secret material tidak dihapus) dan setiap langkah init, probe, dan stop command baru selesai lewat timeout. Daemon tampak sehat.
**Suggested fix**: Tandai `Shared` mati (`AtomicBool`) saat `run` kembali; `start`, `watch`, dan `register_*` menjawab `ProcessError::Io` selama tanda itu menyala (atau mulai watcher baru dan daftarkan ulang PID yang dilacak), supaya daemon bisa menandai subsystem `supervisor` `degraded`.

#### 🟡 Waktu start proses yang diadopsi tidak dibaca ulang setelah watch exit terdaftar, `crates/platform-macos/src/process/watch.rs:320`, `crates/core/src/supervisor/adopt.rs:90`
**Problem**: Baris Exit di spec *Adapter macOS* meminta waktu start proses non child dibaca lagi setelah registrasi kqueue, untuk menutup jendela antara `gather_evidence` dan watch. Port `watch_exit` tidak membawa identitas, dan `adopt_process` langsung lanjut ke probe.
**Why it matters**: Bila proses yang terbukti keluar dan PID nya dipakai proses lain sebelum registrasi, watcher melacak orang asing dan instance tetap `running` (readiness `not_ready`) sampai orang asing itu keluar. Tidak ada sinyal salah karena `checked_signal` membaca ulang fakta; butuh putaran PID, jadi jarang. Baris spec yang eksplisit tanpa implementasi.
**Suggested fix**: Di `adopt_process`, setelah `watch_exit` berhasil, `inspect` sekali lagi dan bandingkan `start` dan `session`; bila beda, anggap hilang atau `identityUnproven`.

#### 🟡 Daftar descriptor dan anggota group bisa terpotong diam diam, `crates/platform-macos/src/process/inspect.rs:488`, `crates/platform-macos/src/process/inspect.rs:289`
**Problem**: `list_fds` dan `group_members` menanyakan ukuran, mengalokasikan ukuran itu plus 16 entri, lalu menerima apa pun yang terisi. Bila proses membuka lebih dari 16 descriptor baru di antara dua panggilan, ekornya hilang dan hasilnya dianggap lengkap.
**Why it matters**: `holds_file` berjalan saat ready pertama, adopsi, dan recheck, tepat saat server database membuka banyak file. Daftar yang melewatkan file lock menjawab `HoldsFile::No`, yang menjadi `adoptable: false` atau, setelah daemon restart, `unknown` dengan `identityUnproven` yang harus diselesaikan pengguna.
**Suggested fix**: Ulangi selama buffer terisi penuh, dengan buffer yang membesar; sama untuk `proc_listpids`.

### Irisan 2, runtime core

#### 🟡 Exit tanpa diminta saat desired `stopped` dicatat sebagai kegagalan, `crates/core/src/supervisor/exit.rs:124`
**Problem**: Tanpa stop berjalan dan dengan desired `stopped` (rekonsiliasi mengadopsi proses `running` yang stop nya terputus, stop sistem yang diantrekan belum menyinyal), exit sendiri menulis `Crashed { will_retry: false }` lalu `failed` dengan `last_failure`, padahal spec mendefinisikan crash sebagai exit saat desired `running`.
**Why it matters**: Banner kegagalan fitur 33 akan muncul untuk instance yang justru melakukan yang pengguna minta.
**Suggested fix**: Terapkan `StopBegin` lalu `Exited`.

#### 🟡 Reload dengan identitas yang tidak cocok membiarkan instance `running`, `crates/core/src/supervisor/reload.rs:124`
**Problem**: `Hangup` memetakan `IdentityMismatch` hanya ke `SignalRefused`, sedangkan jalur stop (`stop.rs:160` sampai `:163`) juga menulis `IdentityLost` dan menutup slot. Setelah reload ditolak, actor terus mem probe PID yang ia tahu bukan proses tercatat. Baru terjangkau dari test core sampai fitur 26.
**Suggested fix**: Pakai `identity_lost` yang sama.

#### 🟡 `ReloadWork` tidak mencatat langkah dan mengabaikan cancel, `crates/core/src/supervisor/reload.rs:192`
**Problem**: Hanya `set_stage(VERIFY)`; tidak ada `begin_step`, `check_cancel`, atau `ctx.stopped()` di loop verify, padahal spec *Asumsi lintas spec* menjadikan tahap reload langkah `StepSpec`. Reload yang di cancel berjalan sampai selesai dan journal tidak punya langkah untuk `recover`.
**Suggested fix**: Pakai kerangka langkah stop dan `select!` pada `ctx.stopped()`.

#### 🟡 Detail error dua kode katalog tidak terisi, `crates/core/src/supervisor/error.rs:358`, `crates/core/src/supervisor/error.rs:453`, `apps/daemon/src/ipc/instance/mod.rs:221`, `apps/daemon/src/ipc/instance/mod.rs:256`
**Problem**: `supervisor.identity_unproven` dari operasi (`stop.rs:305`, `:364`, `startup.rs:222`) dan dari precheck daemon dibuat dengan `pid: None` dan tanpa `items` berjenis `check`, padahal tabel *Kode error* dan katalog (`codes.rs`, `IDENTITY_UNPROVEN` dengan `items { kind "check", max 7 }`) menjanjikan `pid` dan satu item per pemeriksaan identitas; bukti itu sudah ada di `instance.blocker()`. `StopRequiresOverride` tidak membawa item `environment` dengan `environmentId` walau `run_stop` memegang lease nya (`stop.rs:339` sampai `:349`); laten sampai fitur 44.
**Suggested fix**: Bangun error dari `blocker()` di precheck dan di operasi, dan beri `SupervisorError` implementasi `items` untuk dua kode yang mendeklarasikan item.

#### 🟡 Celah yang diketahui: tanpa `WorkFactory`, operasi antrean ditutup `failed` saat daemon berhenti (`verify.md` langkah 13)
**Penilaian**: Minor. Operasi antrean belum berjalan, jadi tidak ada efek samping yang hilang dan registry tetap konsisten; `CloseInterrupted` sudah menutup start dan stop yang berjalan sebagai `failed interrupted`, jadi hasil yang sama untuk yang mengantre jujur, dan rekonsiliasi menurunkan ulang apa yang harus jalan. Ongkosnya: kegagalan yang menyesatkan untuk operasi yang tidak pernah mulai dan idempotency key yang terpakai. Antrean jarang (`Queue` hanya untuk start dan reload pengguna dan start sistem rekonsiliasi). Perbaiki saat spec 0007 punya factory supaya `recover` bisa mengantre ulang; bukan syarat merge.

#### 🟡 `wait_final` mem poll engine tiap 50 ms selama start berjalan, `crates/core/src/supervisor/pending.rs:201`
**Problem**: Setiap run rekonsiliasi memegang permit nya dan memanggil `engine.get` 20 kali per detik sampai final; deadline readiness bisa menit, sampai empat loop bersamaan. NFR-003 hanya mengukur instance diam.
**Suggested fix**: Sinyal selesai dari engine, atau jeda poll yang lebih panjang.

### Irisan 3, startup dan rekonsiliasi

#### 🟡 Tahap `validate` tidak mengulang BR-008 dan `expectedRevision` di dalam operasi, `crates/core/src/supervisor/startup.rs:197`
**Problem**: Spec *Startup transaction* meminta `validate` mengulang pemeriksaan "data sudah diinisialisasi (BR-008)" di dalam operasi. Kode hanya memeriksa lifecycle; instance `failed` yang init nya gagal lolos di cabang `Failed`. Hari ini precheck daemon (`apps/daemon/src/ipc/instance/mod.rs:228`) menolaknya, dan start latar rekonsiliasi hanya menyentuh instance yang sudah diinisialisasi.
**Why it matters**: Pemanggil `start_work` berikutnya (create fitur 20, restart fitur 26) yang melewatkan precheck bisa menjalankan server di folder data kosong.
**Suggested fix**: Kembalikan `NotInitialized` bila `data_initialized_at()` kosong.

#### 🟡 Instance yang tidak berjalan mendapat blocker `identityUnproven`, padahal spec hanya mengizinkan `dataInUse` saat lock dipegang, `crates/core/src/supervisor/reconcile.rs:211`, `crates/core/src/supervisor/reconcile.rs:444`
**Problem**: Baris tabel untuk `stopped`, `failed`, dan `uninitialized` berbunyi "tidak berubah, blocker `dataInUse` kalau lock dipegang", tetapi `plan_for` memasang `BlockerPlan::Unproven` untuk setiap `Unproven`, jadi error bukti (misalnya `inspect` menjawab `Io` untuk PID bekas) pada observasi basi instance `stopped` menghasilkan `identityUnproven` dengan lock bebas. `recheck_one` hanya membersihkan `dataInUse` (`recheck.rs:58`), jadi blocker bertahan sampai start berikutnya berhasil.
**Suggested fix**: Untuk lifecycle yang tidak berjalan, petakan `Unproven` ke `DataInUse` hanya bila lock `Held`, selain itu tanpa blocker.

#### 🟡 Recheck yang mengadopsi proses dengan desired `stopped` tidak pernah menjalankan stop, `crates/core/src/supervisor/recheck.rs:51`, `crates/core/src/supervisor/reconcile.rs:204` (dicek ulang)
**Problem**: `plan_for(Unknown, Stopped, Proven)` mengembalikan `adopting(Then::Stop)`. `reconcile()` mengantrekannya, tetapi `recheck_one` membuang `settled.then`. Skenario: stop terputus crash daemon (`running`, desired `stopped`), rekonsiliasi mendapat `Unproven` lalu `unknown`, 30 detik kemudian recheck membuktikan dan mengadopsi sebagai `running` dengan desired `stopped`, dan tidak ada yang menghentikannya.
**Suggested fix**: Di `recheck_one`, setelah adopsi berhasil dengan `Then::Stop`, mulai `stop_instance` sistem lewat helper `begin` di `pending.rs`.

#### 🟡 Panggilan filesystem sinkron berjalan di thread runtime tokio, `crates/core/src/supervisor/startup.rs:907`, `crates/core/src/supervisor/init.rs:247`, `crates/core/src/supervisor/steps.rs:284`
**Problem**: `command_parts` (lewat `verify_executable`: `canonicalize` dan `symlink_metadata`) dipanggil langsung dari `run_attempt` dan `run_command`, dan `materialize_secret` menulis file secret langsung di `run_init_steps`, sementara sisa irisan membungkus setiap panggilan platform dengan `blocking`. Panggilan `socket` dan `bind` di `select_ports` dan `after_unrequested_exit` juga inline, tetapi hanya mikrodetik.
**Why it matters**: Volume yang macet (runtime root di folder temp yang bisa dibersihkan macOS) bisa menahan worker yang juga melayani actor, probe, dan balasan IPC.
**Suggested fix**: Pindahkan `command_parts` dan `materialize_secret` ke `blocking`, seperti `verify`, `run_render`, dan `fresh_spools`.

#### 🟡 Kegagalan tahap verify pada respawn dicatat `config_render_failed`, `crates/core/src/supervisor/startup.rs:736`
**Problem**: `error_failure_code` memetakan semua varian selain lima ke `CONFIG_RENDER_FAILED`, jadi runtime yang dihapus, paket yang hilang, atau port yang diambil instance lain berakhir `crashed { will_retry: false, reason: config_render_failed }`.
**Why it matters**: `last_failure` dan banner fitur 33 menunjuk ke config yang salah. Daftar `FailureCode` spec belum punya kode yang lebih tepat, jadi ini juga celah spec.
**Suggested fix**: Tambah kode kegagalan untuk keluarga tahap verify lewat `/architect`, atau minimal petakan varian endpoint ke `ENDPOINT_IN_USE` dan log error aslinya.

#### 🟡 `seed_fake` bisa menulis lalu tetap mengembalikan error, `crates/core/src/supervisor/fixture.rs:333`
**Problem**: Doc berkata fungsi ini tidak menulis apa pun saat mengembalikan error, tetapi `install_fake_runtime` menulis folder runtime (`:338`) sebelum `pin` (`:347`) bisa gagal; run berikutnya kena `Leftover`. Hanya root pengembangan (fitur `fixture`).
**Suggested fix**: Jalankan `pin` lebih dulu dan hapus folder yang ditulis saat error berikutnya, atau perlunak doc.

### Irisan 4, spool, probe, domain, storage, fake

#### 🟡 Probe exec melewati `ensure_not_in_argv` dan `verify_executable`, `crates/core/src/supervisor/probe/exec.rs:31` (dicek ulang)
**Problem**: Probe membangun `HelperCommand` langsung dari plan (`program.absolute()`, `args.clone()`), sedangkan setiap spawn helper lain lewat `steps::command_parts` (`verify_executable` lalu `ensure_not_in_argv`) atau memanggil `ensure_not_in_argv` sendiri (`init.rs:138`). Validasi manifest spec 0009 sudah menolak token secret di argumen probe, jadi yang lolos hanya nilai secret yang pengguna ketik ke setting biasa dan di interpolasi template.
**Why it matters**: Nilai seperti itu muncul di `ps` tiap 10 detik selama instance hidup, dan pemeriksaan executable kanonik di dalam runtime root dilewati untuk helper yang paling sering jalan. Security model: "Secret tidak pernah di argv".
**Suggested fix**: Beri `ProbeContext` secret yang sudah di resolve dan jalankan `ensure_not_in_argv` atas argumen probe; alirkan program lewat `verify_executable`.

#### 🟡 Batas backlog tidak ditegakkan selama sink `Busy`, `crates/core/src/supervisor/spool.rs:216`
**Problem**: `pump` kembali `Busy` dari `flush_pending` sebelum `enforce_backlog`, jadi selama sink terus `Busy`, byte yang belum terbaca di spool tumbuh tanpa batas, padahal spec *Spool log* menyebut sink tertahan sebagai pemicu `SPOOL_BACKLOG_MAX`. Laten sampai fitur 27 menambah sink yang bisa `Busy` (`TailSink` tidak pernah `Busy`).
**Suggested fix**: Jalankan `enforce_backlog` sebelum menawarkan batch tertunda.

#### 🟡 Status coba ulang `SinkSet` dikunci per stream, bukan per batch, dan hidup lebih lama dari drain, `crates/core/src/supervisor/sink.rs:119`
**Problem**: `delivered` mengingat sink mana yang menerima "batch" untuk `(instance, stream)` dan baru dipakai oleh `accept` berikutnya untuk key itu, batch apa pun itu. Drain yang dibuang dengan batch tertunda (`drain.rs:90` sampai `:97`) membuat batch pertama drain berikutnya melewati sink yang sudah menerima batch lama. Hari ini tertutupi karena `drain_old_spools` memanggil `tail.forget`; laten untuk sink ketiga.
**Suggested fix**: Ikat status coba ulang ke batch (nomor urut di `LogBatch`) atau hapus key saat drain stream itu dibuang.

#### 🟡 Observasi terbaru dipilih menurut id baris, tetapi retensi memangkas menurut `observed_at`, `crates/storage/src/registry/supervision.rs:93`
**Problem**: `latest_observation` memakai `ORDER BY id DESC`, sedangkan retensi (`prune.rs:131` sampai `:137`) menyimpan 20 baris terbaru menurut `observed_at DESC, id DESC`, dan `observed_at` dari jam dinding. Setelah jam mundur cukup jauh, baris spawn terbaru bisa terpangkas, `latest_observation` menunjuk PID lama yang mati, dan instance menjadi `unknown` dengan `dataInUse` yang tidak pernah selesai lewat recheck. Aman (tanpa sinyal), tetapi butuh stop manual; kemungkinannya belum dikonfirmasi.
**Suggested fix**: Pangkas menurut `id DESC`, atau samakan kunci pembaca dan pemangkas.

#### 🟡 Resume membuang satu baris utuh bila byte sebelum offset awal adalah newline, `crates/core/src/supervisor/spool.rs:145`
**Problem**: `resume` memasang `Skip::ToNewline` setiap kali `start > 0`; untuk proses diam yang diadopsi, byte terakhir hampir selalu `\n`, jadi baris utuh pertama setelah adopsi hilang. Kode sesuai teks spec; aturannya yang perlu diubah.
**Suggested fix**: Lewat `/architect`: baca byte di `start - 1` dan lewati hanya bila bukan `\n`.

### Irisan 5, daemon, protokol, check-deps

#### 🟡 `reconcile` yang gagal membuat `shutdown()` tidak bisa lagi menjangkau supervisor, `apps/daemon/src/supervisor.rs:135`
**Problem**: Saat `Err`, `unavailable("reconciliation failed")` mengganti `Slot::Reconciling(supervisor)` dengan `Slot::Unavailable`, dan `shutdown()` hanya menjangkau `Reconciling` atau `Ready`. Actor yang sudah dibuat sebelum kegagalan (adopsi, probe, penanganan exit) terus jalan melewati stop daemon, bertentangan dengan AC-21. Apakah `Supervisor::reconcile` bisa gagal setelah membuat actor belum dikonfirmasi.
**Suggested fix**: Matikan supervisor sebelum menandai slot tidak tersedia, atau simpan handle di `Slot::Unavailable`.

#### 🟡 Setiap alasan tidak tersedia dijawab `registry.unavailable`, `apps/daemon/src/supervisor.rs:169`
**Problem**: `Slot::Unavailable(detail)` selalu dijawab `registry::UNAVAILABLE` dengan pesan tetap, apa pun `detail` nya ("reconciliation failed", engine tidak pulih, "the daemon stopped"). Client yang memanggil `instance.list` setelah rekonsiliasi gagal diberi tahu registry mati, sementara `daemon.get_health` berkata `registry` `ok`.
**Suggested fix**: Bawa `detail` sebagai detail `reason`, atau petakan kasus berhenti ke `daemon.shutting_down`.

#### 🟡 Field secret bisa ditanam di `settings` sebagai reference dan melewati `secrets`, tabel uses, dan default `generate`, `apps/daemon/src/ipc/instance/create.rs:413`
**Problem**: `check_field` menerima nilai apa pun yang ter parse sebagai `SecretRef` untuk field secret (`crates/core/src/provider/settings.rs:659` sampai `:665`). Di `settings_of`, field secret yang ada di `params.settings` dilewati loop generate (`:462`) dan tidak pernah dilihat loop `secrets`, jadi tidak ada di `plan.secrets` maupun batch `ReplaceSecretUses` (`:732` sampai `:746`), sedangkan `resolve_for_start` hanya membaca `secret_uses_by(instance)`. Contoh: `settings: { "password": "local:instance/<id lain>/password" }` tanpa entri `secrets`; create berhasil dengan config revision yang menyebut secret tanpa baris uses, mungkin milik instance lain, sehingga `secret.delete` atas reference itu tidak melihat pemakai ini. Doc `SettingsDocument` (`crates/protocol/src/instance.rs:491` sampai `:495`) berjanji secret hanya lewat `secrets`, tetapi tidak ditegakkan. Peer tetap user yang sama, jadi bukan batas hak akses.
**Suggested fix**: Tolak field secret skema yang muncul di `params.settings` dengan `instance.settings_invalid` di `settings.<field>`.

#### 🟡 Peringatan `PutOutcome` dibuang saat create, `apps/daemon/src/ipc/instance/create.rs:666`
**Problem**: Hanya cabang `Err` dari `service.put` yang dibaca; `warnings` di `Ok(PutOutcome)` hilang. Spec 0011 AC-7 berkata nilai di bawah 8 byte diterima dengan peringatan `short_value_not_scrubbed`; balasan create tidak punya tempat untuk itu dan tidak ada log.
**Suggested fix**: Minimal `tracing::warn!` dengan nama field (tanpa nilai); idealnya peringatan masuk ringkasan operasi.

#### 🟡 Paket pinned yang tidak valid atau digest nya beda dilaporkan `instance.provider_not_found`, `apps/daemon/src/ipc/instance/create.rs:300`
**Problem**: Kegagalan baca, `from_files`, digest folder yang beda dari digest manifest, dan nama atau revision yang tidak cocok semuanya dilipat ke `not_found()`, padahal `supervisor.package_unavailable` didefinisikan untuk "paket pinned hilang, tidak valid, atau digest beda". Pengguna yang mengedit `provider.yaml` di folder digest diarahkan ke tempat yang salah.
**Suggested fix**: Kembalikan `supervisor.package_unavailable` dengan `reason` (`invalid`, `digest_mismatch`) untuk folder yang ada, dan simpan `provider_not_found` untuk folder atau baris registry yang tidak ada.

#### 🟡 Celah yang diketahui: `lastFailure.at`, `exitCode`, dan `signal` selalu null, `apps/daemon/src/ipc/instance/dto.rs:57`
**Penilaian**: Minor. Doc DTO berkata `at` "null after a daemon restart" (`crates/protocol/src/instance.rs:238`), jadi client yang membaca null di run yang sama menyimpulkan restart yang tidak terjadi, dan tampilan crash loop fitur 33 tidak bisa menunjukkan exit code, padahal core sudah punya `last_exit_code` dan `last_signal` (terlihat di `SupervisorError::RetryBudgetExhausted`). Tidak ada nilai salah yang dikirim.
**Suggested fix**: Bawa status exit dan waktu terakhir di `InstanceStatus` lalu petakan di sini; sampai itu, sebut di doc DTO bahwa daemon ini belum mengisinya.

#### 🟡 `SPAWN_NAMES` tidak memuat `system` dan `popen`, `xtask/src/check_deps/source_scan.rs:89`
**Problem**: Keduanya menjalankan program lewat shell dan tidak ada di daftar. Core dan daemon tidak bisa memakainya (`unsafe` dilarang di luar `stackly-platform-macos` dan core tidak bergantung pada `libc`), tetapi modul adapter macOS di luar `process` dan `codesign` bisa memanggil `libc::system` dan lolos aturan yang pesannya berkata hanya `process` dan `codesign` yang menjalankan program.
**Suggested fix**: Tambah `system` dan `popen` ke `SPAWN_NAMES` dan perluas test `the_macos_adapter_spawns_only_in_process_and_codesign`.

## Nits

Irisan 1:
- ⚪ `crates/platform-macos/src/process/watch.rs:762`, hasil `F_GETFL` yang `-1` tidak diperiksa sebelum `F_SETFL(.. | O_NONBLOCK)`.
- ⚪ `crates/platform-macos/src/process/stdin.rs:35`, thread penulis stdin tidak punya batas waktu, padahal baris Spawn spec menyebut batas waktu langkah; dengan helper yang menyerahkan fd 0 ke turunan yang menjadi daemon, thread dan secret hidup lebih lama dari langkahnya.
- ⚪ `crates/platform-macos/src/process/inspect.rs:311`, satu anggota yang `proc_pidinfo` nya gagal sesaat menggagalkan seluruh `group_members`, dan core lalu melaporkan tanpa sisa (`exit.rs:177` sampai `:180`).
- ⚪ `crates/platform-macos/src/process/watch.rs:325`, `watch` kedua untuk PID yang sama mengganti listener pertama diam diam; dokumentasikan di port (`crates/platform/src/process.rs:924`) atau tolak.
- ⚪ `crates/platform-macos/src/process/spawn.rs:31`, `Launch` men derive `Debug` dengan argv dan env lengkap; `{launch:?}` di masa depan akan menaruh env plan ke log daemon.
- ⚪ `crates/platform-macos/src/process/lock.rs:73`, `.mode(0o600)` hanya berlaku saat dibuat; `instance.lock` lama dengan mode lebih lebar diterima tanpa peringatan.
- ⚪ `crates/platform/src/process.rs:78`, `:128`, `ChildRole` dan `RequestPart` publik, akan bertambah, tanpa `#[non_exhaustive]`.

Irisan 2:
- ⚪ `crates/core/src/supervisor/exit.rs:95`, `startup.rs:1131`, anggota group yang tersisa masuk status tetapi tidak pernah di log (spec *Stop* langkah 5: status dan log).
- ⚪ `crates/core/src/supervisor/respawn.rs:125`, di loop `Later`, `show_pause` tidak diperbarui, jadi `nextAttemptAt` menunjukkan waktu lampau.
- ⚪ `crates/core/src/supervisor/actor.rs:242`, `published: placeholder(now)` langsung ditimpa; `placeholder` mati.
- ⚪ `crates/core/src/supervisor/stop.rs:36`, `reload.rs:26`, `use stackly_domain::..` sesudah grup `crate::`.
- ⚪ `crates/core/src/supervisor/recheck.rs:47`, tiap 30 detik setiap actor membaca registry sebelum memeriksa state `unknown` atau `dataInUse` di memori.
- ⚪ `crates/core/src/supervisor/stop.rs:364`, "actor tidak mengenal proses" dilaporkan sebagai `supervisor.identity_unproven`, kode yang disiapkan untuk `unknown`.

Irisan 3:
- ⚪ `crates/core/src/supervisor/startup.rs:369`, `unreachable!` di future operasi non test; `WorkError::Engine` akan menggagalkan operasi alih alih task.
- ⚪ `crates/core/src/supervisor/startup.rs:1103`, penanda `// provisional: spec 0018`, AGENTS.md meminta `// provisional: F18`; komentar `check_launchable` di `:914` tanpa penanda.
- ⚪ `crates/core/src/supervisor/startup.rs:528`, `:565`, port `Fixed` yang bentrok dengan endpoint lain milik instance yang sama dilaporkan `supervisor.auto_port_unavailable`.
- ⚪ `crates/core/src/supervisor/startup.rs:349`, respawn juga menjalankan `guard_last_observation`, padahal spec bilang respawn hanya mengulang `lock_instance`; tidak berbahaya.
- ⚪ `crates/core/src/supervisor/startup.rs:1050`, `resume` yang gagal meninggalkan child tercatat yang tidak di `discard`; hanya terjadi saat child sudah mati, tetapi `discard` eksplisit membuat invariant terlihat.
- ⚪ `crates/core/src/supervisor/startup.rs:243`, putaran ulang `RevisionMismatch` memanggil `secrets::resolve` lagi dengan caller pengguna, jadi store yang selalu meminta izin bisa menampilkan dialog kedua.
- ⚪ `crates/core/src/supervisor/startup.rs:579`, spec menyebut `resolve_port` untuk `Auto`; kode menulis aturan yang sama inline.
- ⚪ `crates/core/src/supervisor/classify.rs:31`, `:58`, `:80`, `ports.rs:22`, `startup.rs:56`, enum publik `GoneReason`, `UnprovenBlocker`, `Classification`, `BindOutcome`, `StartTrigger` tanpa `#[non_exhaustive]`.
- ⚪ `crates/core/src/supervisor/fixture.rs:123`, `:157`, `std::fs` langsung alih alih port `fs()` (hanya pengembangan).

Irisan 4:
- ⚪ `crates/core/src/supervisor/spool.rs:428`, peringatan "freeing the spool failed" membawa stream tetapi tidak membawa instance.
- ⚪ `crates/core/src/supervisor/spool.rs:269`, `:404`, batch tertunda di klon setiap kali ditawarkan ulang selama sink `Busy`.
- ⚪ `crates/core/src/supervisor/sink.rs:119`, entri `delivered` untuk instance yang dihapus tidak pernah dibuang.
- ⚪ `crates/storage/src/registry/supervision.rs:127`, `boot_id` dan `session_id` nullable di skema tetapi pembaca mewajibkan keduanya; baris NULL dibaca `Corrupt` (aman, menjadi `Unproven`).
- ⚪ `crates/testkit/src/fake_process.rs:212`, `released` dikunci `FileIdentity` dan tidak dipangkas saat file spool dihapus (hanya test).

Irisan 5:
- ⚪ `xtask/src/check_deps/source_scan.rs:382`, `in_modules` adalah pencocokan awalan atas path modul plus nama `mod` inline, jadi `mod kill { .. }` inline di `spawn.rs` atau `#[path]` yang dialihkan memenuhi `process::kill`; angka `9` polos juga tidak tertangkap, wajar untuk aturan token.
- ⚪ `xtask/src/check_deps/source_scan.rs:361`, identifier apa pun bernama `fork` atau `execv` di core atau daemon (field, variabel) dilaporkan; risiko rendah, layak satu baris di doc aturan.
- ⚪ `crates/protocol/src/instance.rs:257`, `InstanceRestart.attempts`, `budget`, dan `window_seconds` memakai `u32` polos sementara `DaemonReconciling` memakai `Count`; masih dalam rentang aman, tetapi konvensi newtype tidak seragam.
- ⚪ `apps/daemon/src/ipc/instance/create.rs:562`, `NameTaken::code` menulis ulang `"instance.name_taken"` alih alih `instance::NAME_TAKEN.code`.
- ⚪ `apps/daemon/src/ipc/instance/create.rs:221`, `supervisor.runtime_unavailable` adalah error sinkron create yang tidak ada di kolom "Error sinkron" spec (celah tabel spec; kodenya masuk akal).
- ⚪ `apps/daemon/src/ipc/instance/create.rs:72`, pemeriksaan `provider.revision` mendahului pemeriksaan idempotency key, jadi request tanpa key dan tanpa revision mendapat `request.invalid_params`, bukan `operation.idempotency_key_required`.
- ⚪ Celah yang diketahui, `apps/daemon/src/ipc/instance/create.rs:398`: endpoint `unix` mendapat `<nama>.sock` yang dipotong ke 15 byte. Reviewer mengecek `check_socket_name` (`crates/domain/src/value.rs:136`): setiap potongan nama endpoint yang sah plus `.sock` tetap sah, jadi create tidak bisa gagal karenanya, nilainya hanya pengisi kolom, dan belum ada paket dengan endpoint `unix`.
- ⚪ Celah yang diketahui, `apps/daemon/src/ipc/instance/dto.rs:261`, `:272`: `socketPath` null, ditunda eksplisit ke langkah 9 spec 0020.
- ⚪ Celah yang diketahui, `apps/daemon/src/ipc/instance/create.rs:164`: belum ada `instance.name_invalid`; nama yang salah menjadi `request.invalid_params` di `name` dengan kategori `invalid_input` yang sama, jadi exit code CLI tidak berubah, dan K4 mengizinkan penundaan.

## Penilaian celah yang diketahui

| Celah | Tingkat | Alasan singkat |
| --- | --- | --- |
| `lastFailure.at`, `exitCode`, `signal` null | Minor | doc DTO membuat null terbaca "daemon restart"; data sudah ada di core |
| tanpa `WorkFactory`, antrean ditutup `failed` | Minor | tidak ada efek samping hilang, konsisten dengan `CloseInterrupted`; key terpakai |
| `socketPath` null | Nit | ditunda eksplisit ke spec 0020 langkah 9 |
| nama socket pengisi `<nama>.sock` | Nit | selalu sah, tidak terlihat di wire, belum ada endpoint `unix` |
| belum ada `instance.name_invalid` | Nit | kategori dan exit code sama, K4 mengizinkan |
| `test_starting_method` dipertahankan | bukan temuan | `#[doc(hidden)]`, default `None`, `DefaultHandlers` memakai default, `connection.rs:648` hanya merutekan `test.start_operation` bila handler mengembalikan `Some`; client biasa tidak bisa menjangkaunya |

## Di luar lingkup (untuk lead, tidak dihitung)

- Paket `providers/fake` merender `{{ secret.authToken }}` sebagai plaintext ke `${instance.config}/fake.json` dengan `sensitive: true` (`providers/fake/provider.yaml:29` sampai `:32`, `templates/fake.json.tmpl`). Spec 0011 baris mekanisme 5 berkata, kalau tool tidak bisa include file terpisah, seluruh config hasil render diperlakukan sebagai secret material dengan aturan yang sama (dihapus setelah stop bersih, disapu saat daemon start), tetapi `remove_secret_material` dan `sweep_secret_material` hanya menyentuh `instance_secrets_dir` (`crates/core/src/secret/delivery/material.rs:155`), dan `interpolate.rs:622` sampai `:624` mengizinkan template `sensitive` ke folder instance mana pun. Jadi token fake tinggal permanen di folder config instance. Pemiliknya spec 0009 dan 0011 (fitur 9 dan 11) atau paket fake spec 0014, bukan fitur 10; layak dicek di review fitur itu.

## Strengths

- Port membuat invariant menjadi tipe: hanya `SuspendedChild` by value yang bisa di `discard`, hanya group `HelperHandle` yang bisa dihentikan, `ProviderSignal` tidak punya `Kill`, dan setiap sinyal lain lewat `SignalTarget` yang adapter periksa ulang di bawah mutex slot PID. Dengan `kill.rs` sebagai satu satunya file yang menyebut `SIGKILL` (ditegakkan `check-deps`, termasuk isi macro dan `use .. as`), AC-5 dan AC-15 berlaku by construction.
- `sys.rs` mendeklarasikan setiap simbol utuh dengan `#[repr(C)]` dan assert ukuran serta offset yang cocok dengan SDK arm64 (reviewer menelusuri per field, total `kinfo_proc` 648); flag spawn `0x448c` di assert saat kompilasi; setiap fd sumber di duplikasi ke 10 ke atas dengan `F_DUPFD_CLOEXEC` dan `OwnedFd` hidup melewati `posix_spawn`.
- Watcher kqueue menandai setiap registrasi dengan token per slot di `udata`, jadi event exit basi dari PID yang dipakai ulang tidak menyentuh slot baru; ia hanya me reap anak sendiri per PID, tidak pernah `waitpid(-1)`.
- `classify` adalah fungsi murni total, `gather_evidence` menegakkan urutan pengumpulan bukti spec di satu tempat, dan test sapuan `no_row_of_the_table_signals_or_adopts_what_is_not_proven` membuat "Unproven tidak pernah menyinyal" menjadi properti yang diperiksa, bukan konvensi. Setiap transisi tabel rekonsiliasi diuji terhadap `InstanceLifecycle::apply`.
- Jendela anak tertahan tertutup di setiap jalur: setiap kegagalan antara `spawn_main` dan `resume` lewat `discard`, identitas ter commit sebelum `SIGCONT`, dan `watch_exit` terdaftar sebelum `resume`. Lock instance dilepas by construction.
- Redaksi lalu potong ditegakkan di tepat dua titik (`SpoolDrain::emit`, `ProbeOutcome::failure`), keduanya punya test dengan secret yang melintasi potongan; baris parsial ditahan melintasi batas chunk.
- `restart.rs` murni dan pasti (jitter dalam sepersejuta, `checked_pow` dan `checked_mul`, jendela di jam monotonic) dengan test yang mengunci angka spec; IO actor selalu di `spawn_blocking`, mutex actor tidak pernah melintasi `.await`, dan setiap `select!` menunggu receiver `watch` yang aman dibatalkan.
- K12 berlaku ujung ke ujung (`replay_operation` sebelum `plan_create`, `SecretText` di fingerprint sebagai penanda tetap), create sementara sesuai paragraf *Create sementara* (lock nama dengan `Reject`, `validate`, `secrets`, `layout` reversible, `register` titik pertama yang tidak bisa dibalik, `PutTarget::New` dengan `classify_create_failure`, `JournalPayload::empty()`), dan `ipc::instance` tidak pernah memanggil `.expose()`. Default `generate` hanya untuk field yang `SecretFieldRule::from_setting` kenali, membawa batas panjang field, dan nilai yang dibuat tidak pernah kembali ke balasan.
- Protokol 1.8 mematuhi aturan wire (open enum berakhir `Unrecognized`, `#[serde(default)]` di field opsional, `Validate` di setiap params, tanpa `BTreeMap`, kelima method `recovery refused`), dan area kode cocok dengan tabel *Kode error* baris demi baris. Fitur `fixture` di gate tiga lapis dan aturan `check-deps` nya mencakup setiap rute manifest.

## Test coverage

Configured (`cargo test`). `verify.md` mencatat cakupan luas: 16 test integrasi adapter dengan dummy server nyata (set fd AC-1, `discard` anak hidup dan yang mati sendiri, PID 1 sebagai proses asing, TERM lalu paksa pada group, lock dan `probe_lock`, punch hole), tabel `classify` (17) dan `plan_for` (17), suite rekonsiliasi (23), startup (20), spool (16), probe dan HTTP (34 unit plus 2 nyata), registry supervision, fake (17), dan test daemon `supervisor` dengan evidence. Tidak ada temuan yang bertentangan dengan langkah yang `verify.md` tandai PASS.

Celah test yang terkait temuan di atas (tidak dihitung sebagai temuan sendiri):
- balapan stop pengguna dengan crash di batas `settle` (Major pertama);
- bacaan registry yang gagal saat exit, di akhir jeda respawn, dan di bacaan ulang commit actor (Major kedua; test AC-22 hanya menolak commit);
- secret material setelah start gagal sebelum spawn, dengan paket `config_plaintext` (Major ketiga);
- jumlah handle registry setelah stop daemon (Major keempat);
- cabang desired `stopped` di `settle`, reload dengan identitas tidak cocok, cancel `ReloadWork`, recheck yang mengadopsi lalu harus stop;
- lock masih `Held` saat percobaan respawn dan cabang error `guard_last_observation` (reviewer tidak bisa grep test karena disk penuh; belum dikonfirmasi);
- kematian thread watcher, pemotongan daftar fd, `watch_exit` kedua untuk PID yang sama, backlog saat sink `Busy`, status coba ulang `SinkSet` melintasi drain yang dibuang, nilai secret di argv probe exec, dan reference secret yang ditanam di `settings`.
