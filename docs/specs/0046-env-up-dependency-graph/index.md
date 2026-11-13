# 0046. Env up dan dependency graph

**Date**: 2026-11-13
**Status**: In Progress

## Summary

Spec ini menetapkan Up: satu perintah yang membuat sebuah environment berjalan. Up memasang runtime yang belum ada, membuat instance dedicated, meminjam instance yang ditunjuk `instanceRef` tanpa mengubah apa pun miliknya, mencatat lease (tanda bahwa environment ini butuh instance itu), lalu menyalakan service menurut urutan dependency dan menunggu tiap service benar benar ready. Up adalah satu operasi pengatur yang memulai operasi anak yang sudah ada (install, create, start), jadi setiap aturan yang berlaku untuk instance biasa tetap berlaku. Kalau satu service gagal, service yang sudah ready dibiarkan jalan, service yang bergantung padanya tidak dinyalakan, dan environment ditandai partially ready dengan alasan per service. Reference secret yang belum diisi menghentikan Up sebelum ada perubahan apa pun. `stackly env up`, `stackly env status`, tombol Up…, dan banner partially ready memakai balasan daemon yang sama. Down dan pelepasan lease dikerjakan spec 0047 lewat titik sambung di sini.

## Requirements

**User stories**:

- Sebagai developer yang baru clone proyek, saya ingin satu perintah yang memasang, membuat, dan menyalakan semua service proyek dalam urutan yang benar, supaya saya tidak menyalakannya satu per satu.
- Sebagai pengguna yang sudah punya instance bersama (misalnya satu Valkey untuk beberapa proyek), saya ingin environment meminjamnya tanpa mengubah config, auth, port, atau autostart instance itu.
- Sebagai pengguna yang Up nya gagal di tengah, saya ingin tahu service mana yang gagal, di tahap apa, dan apa yang harus saya lakukan, tanpa kehilangan service yang sudah jalan.
- Sebagai pengguna yang lupa mengisi password, saya ingin Up berhenti dan menunjuk reference yang kosong, bukan diam diam memakai password kosong.
- Sebagai penulis script, saya ingin `stackly env up --wait` keluar bukan 0 kalau ada service yang tidak ready, dan `stackly env status` menampilkan port yang benar benar dipakai.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-021):

- **AC-1** (pintu masuk Up, REQ-021): `env.up { source, expectedPlanDigest, locked, idempotencyKey }` menjalankan urutan *Precheck Up*: replay, baca manifest, hitung ulang plan lewat `environment::plan` spec 0044 (dengan `PlanInputs` spec 0045 dan `locked` apa adanya), lalu menolak tanpa membuat operasi dan tanpa mutasi apa pun kalau manifest tidak valid (`env.manifest_invalid`), plan tidak `applicable` (`env.plan_blocked` dengan daftar blocker nya), `planDigest` berbeda dari `expectedPlanDigest` (`env.plan_changed`), atau ada reference secret yang belum terisi (AC-2). Up tidak pernah memanggil `runtime::resolve` sendiri: runtime, provider, endpoint, dan urutan diambil dari `PlanItem`. Test penolakan membandingkan pohon file test root dan generation registry sebelum dan sesudah, seperti spec 0044 AC-6.
- **AC-2** (needs secret, PRD bagian 13.1): kalau plan memuat item `create` dengan secret ber `status` `missing` atau `unregistered`, `env.up` ditolak `secret.needs_secret` dengan satu `items` ber `kind` `reference` per reference (bentuk `NeedsSecretItem` yang sudah ada: `reference`, `reason` `missing`, dan `generateAllowed` dari aturan yang sama dengan `resolve_for_start`), urut teks reference, sebelum runtime dipasang, instance dibuat, lease dicatat, atau environment didaftarkan. Hal yang sama berlaku untuk member atau `instanceRef` yang instance nya punya blocker `needsSecret`: plan spec 0044 melaporkannya sebagai `env.instance_blocked`, dan `env.up` mengubahnya menjadi `secret.needs_secret` dengan reference dan alasan dari blocker itu (*Precheck Up*). Status `unknown` (store tidak bisa ditanya tanpa dialog) tidak menolak; kalau saat create atau start reference itu ternyata tidak bisa dibaca, hanya service itu yang berhenti dengan `phase` `needs_secret` dan daftar reference nya (AC-10). Tidak ada jalur yang mengisi secret dengan nilai kosong, nilai default, atau nilai yang dibangkitkan untuk reference yang ditulis di `secretRefs`. Test menaruh reference `missing` dan memeriksa nol baris baru di `runtimes`, `instances`, `environment_instances`, dan `instance_demands`.
- **AC-3** (operasi pengatur): Up berjalan sebagai satu operasi `environment_up` dengan resource `Environment(id)`, lock tambahan `LockKey::EnvironmentName(name)`, `BusyPolicy::Reject`, `expected_revision` dari `EnvPlan.environment.revision` untuk environment terdaftar, dan kelas slot pengatur (`OperationKind::is_coordinator()` benar untuk `environment_up`). Operasi pengatur tidak memakai satu pun dari 8 slot berjalan spec 0007; ia punya batas sendiri `MAX_RUNNING_COORDINATORS` 4, dan yang kelima menunggu `queued`. Pekerjaannya hanya membaca manifest, menulis registry lewat `commit_guarded`, memulai operasi anak, dan menunggu; ia tidak pernah memanggil `ProcessSupervision`, downloader, atau menulis file. Up kedua untuk environment yang sama selama yang pertama belum selesai ditolak `resource.busy` dengan `operationId` yang pertama. Test menjalankan tiga Up bersamaan dengan `max_running` 2 dan memeriksa semuanya selesai (tidak ada deadlock).
- **AC-4** (langkah `accept`: daftar, snapshot, member, lease): langkah pertama operasi, dalam satu `commit_guarded`: mendaftarkan environment lewat `environment::register` spec 0044 kalau belum terdaftar; menulis snapshot (`Environment::set_manifest`), `manifest_sha256`, `lock_sha256` (dari plan spec 0045, null kalau plan tidak memakai lock), dan `last_operation_id`; menambah member `shared` untuk setiap service `instanceRef` yang belum jadi member; menyisipkan satu lease environment `active` untuk setiap member yang alias nya ada di manifest dan belum punya lease `active` dari environment ini (member `detached` tidak diberi lease baru dan lease lamanya tidak disentuh); menulis `dependency_conditions` setiap member yang alias nya ada di manifest; dan menulis `last_up` berstatus `running`. Guard nya membaca environment dan instance di dalam transaksi itu dan memeriksa bahwa setiap instance milik service di manifest masih ada, bukan `retained`, dan provider nya cocok; kalau tidak, operasi gagal `env.plan_changed` tanpa menulis apa pun. Kegagalan `environment::register` (`env.name_taken`, `env.limit_reached`) menggagalkan operasi dengan kode itu. Lease dicatat sebelum service mana pun dinyalakan.
- **AC-5** (`instanceRef`, VAL-014, PRD bagian 13.2): Up untuk service ber `instanceRef` menambah satu member `shared` dan satu lease, tidak pernah membuat instance, dan tidak pernah mengubah instance itu: config revision aktif, isi `config/`, endpoint dan port, secret dan pemakaiannya, `startup`, pin provider, dan runtime nya identik sebelum dan sesudah Up. Satu satunya hal yang boleh berubah adalah desired dan lifecycle nya lewat operasi `start_instance` biasa saat ia sedang berhenti. Up kedua tidak membuat lease kedua. Test membandingkan baris instance (selain desired, lifecycle, dan revision yang naik karena start), config revision, dan pohon `config/` sebelum dan sesudah.
- **AC-6** (install): untuk setiap artefak berbeda di item `create` yang `runtime.install` nya benar, operasi memulai satu operasi anak `install_runtime` dengan selector `sha256:<artifactSha256>` lewat jalur yang sama dengan `runtime.install` (gate trust spec 0037 dan `install_precheck` ikut berjalan), paling banyak `UP_INSTALL_CONCURRENCY` 2 sekaligus. Dua service dengan artefak yang sama menghasilkan satu install. Install yang gagal membuat setiap service yang butuh artefak itu berakhir `failed` di tahap `install`; service lain lanjut.
- **AC-7** (create dengan ikatan, BR-023): untuk setiap item `create` yang runtime nya tersedia, berurutan menurut manifest, operasi memulai satu operasi anak `create_instance` dengan draft dari `environment::plan::draft_for` (fungsi yang sama dengan plan spec 0044: nama efektif, endpoint, settings, `secretRefs` sebagai `Bind`, secret lain `Generate`, `startup` `manual`) ditambah `EnvironmentBinding`. Batch `register` create menulis baris instance, member `dedicated`, `dependency_conditions`, dan lease environment dalam satu transaksi, jadi tidak pernah ada instance dedicated tanpa member atau member tanpa lease. Create yang gagal, dibatalkan, atau terputus dikompensasi oleh operasi create itu sendiri (pohon instance, reference secret turunan, baris instance, member, dan lease nya), dan tidak menyentuh instance, member, lease, atau runtime lain. Ikatan ikut disimpan di `input` journal create dan dibaca lagi oleh `CreateWorkFactory::rebuild`, jadi create yang dibangun ulang sesudah restart daemon tetap membawa ikatannya. `instance.create` dari klien tidak bisa mengirim ikatan.
- **AC-8** (graph, readiness, dan timeout, PRD AC-021, PRD bagian 13.2): sesudah install dan create, service dinyalakan menurut *Aturan graph*: sebuah service baru boleh mulai kalau setiap dependency nya ber `phase` `ready` dan instance dependency itu, dibaca saat itu juga, masih `running` dengan readiness `ready`. Service yang instance nya berhenti dinyalakan lewat operasi anak `start_instance` dengan pemicu `environment`, yang selesai `succeeded` hanya saat readiness `ready` (spec 0010 AC-8). Dependent yang dependency nya gagal, dilewati, atau dibatalkan berakhir `skipped` dengan `env.dependency_not_ready` (`reason` `failed`) tanpa pernah di spawn; dependent yang menunggu lebih lama dari `timeoutSeconds` edge itu (default `DEFAULT_DEPENDENCY_TIMEOUT_SECONDS` 600) berakhir `skipped` dengan `reason` `timeout`. Tidak ada dependent yang dimulai selagi dependency nya tidak ready. Paling banyak `UP_START_CONCURRENCY` 4 start berjalan sekaligus, dan seri diputus urutan `order` plan.
- **AC-9** (instance yang sudah ada dan sehat, PRD AC-021): service yang instance nya sudah `running` dan `ready` langsung `ready` tanpa operasi anak. Up tidak pernah menghentikan, me restart, me reload, merender ulang config, mengganti port, mengganti runtime, atau menghapus instance yang sudah ada, baik saat sukses, gagal, maupun batal, dan tidak pernah mengubah instance supaya cocok dengan manifest (spec 0044 AC-9). Test menjalankan Up yang gagal di service lain dan memeriksa PID, `restart_total`, dan config revision instance yang sehat tidak berubah.
- **AC-10** (partial failure dan recovery, REQ-021, PRD AC-021): Up yang semua service nya `ready` berakhir `succeeded`. Selain itu ia berakhir `failed` dengan error `env.up_incomplete` (`ready`, `total`, dan satu `items` ber `kind` `service` per service yang tidak ready: `service`, `phase`, `stage`, `code`, `dependency`, `operationId`), `stage` operasi berisi tahap service pertama yang tidak ready menurut `order` (`install`, `create`, `start`, atau `wait`), dan hint "fix the services listed, then bring the environment up again; services that are ready keep running". Service yang sudah ready tetap jalan, member dan lease yang sudah dicatat tetap ada, dan Up berikutnya hanya mengerjakan sisanya. Laporan per service yang sama disimpan di `environments.last_up` lewat commit penutup (*Operasi `environment_up`*), yang ditulis sebelum operasi berakhir di setiap jalur selain abandon saat daemon berhenti.
- **AC-11** (batal): `operation.cancel` pada Up berhenti memulai operasi anak baru, membatalkan operasi anak yang masih berjalan lewat `OperationEngine::cancel`, menunggu semuanya selesai, menandai service yang belum selesai `cancelled`, menulis `last_up` lewat commit penutup, lalu berakhir `cancelled`. Pembatalan anak memakai pemicu yang sama dengan pemicu cancel induknya. Service yang sudah ready tetap jalan dan lease tetap ada. Create yang dibatalkan dikompensasi create itu sendiri (AC-7); start yang dibatalkan mengikuti aturan cancel spec 0010 AC-8.
- **AC-12** (kompensasi hanya milik sendiri, BR-023): operasi `environment_up` sendiri tidak punya catatan item dibuat dan tidak pernah menjalankan kompensasi; satu satunya penghapusan yang bisa terjadi selama Up adalah kompensasi sebuah operasi anak `create_instance` atas resource yang ia buat sendiri, atau `install_runtime` atas folder scratch nya. Test menanam instance dan runtime milik pengguna dengan nama mirip, menggagalkan create service lain, dan memeriksa keduanya utuh byte demi byte.
- **AC-13** (bentuk dan umur lease, PRD bagian 13.2): lease environment adalah baris `instance_demands` ber `source_kind` `environment`, `source_id` id environment, tanpa `session_id`, paling banyak satu yang `active` per pasangan instance dan environment. Lease tidak dilepas oleh klien yang putus, CLI yang ditutup, Up yang gagal atau batal, restart daemon, rotasi sesi, logout, atau instance yang berhenti atau crash; hanya Down (spec 0047) dan kompensasi create yang melepas atau menghapusnya. Start dengan pemicu `environment` menulis desired `running` tanpa lease `manual`, juga di jalur tanpa operasi `start_precheck` (instance `running` dengan desired `stopped`). Test memutus klien di tengah Up, me restart daemon, dan menjalankan rotasi sesi `new`, lalu memeriksa lease masih `active` dan desired instance tidak berubah.
- **AC-14** (serentak, PRD AC-073 bagian Up): dua Up dari dua environment yang meminjam instance yang sama menghasilkan tepat dua lease `active` dan satu proses; start kedua menemukan instance sudah jalan. Dua Up yang membuat instance dedicated bernama sama menghasilkan tepat satu instance; yang kalah gagal di create dengan `instance.name_taken` atau `resource.busy`. Up selalu mencatat lease lebih dulu, baru membaca keadaan instance, dan instance yang sedang `stopping` saat giliran service itu tiba ditunggu sampai berhenti lalu dinyalakan lagi lewat `start_instance`. Bagian Down dari AC-073 milik spec 0047.
- **AC-15** (daemon berhenti di tengah Up): `environment_up` tidak resumable dengan kebijakan `CloseInterrupted`. Sesudah daemon start lagi, operasi itu ditutup `failed` alasan `interrupted` tanpa kompensasi; operasi anak yang sedang berjalan mengikuti kebijakan jenisnya sendiri, sedangkan operasi anak `create_instance` dan `start_instance` yang masih `queued` dan induknya ditutup `interrupted` ikut ditutup `failed` alasan `interrupted` oleh `recover` (install yang antre boleh lanjut); member, lease, dan instance yang sudah ada tetap. Up yang masih `queued` saat daemon berhenti ditutup dengan cara yang sama dan tidak punya `lastUp`, karena `accept` belum berjalan. `env.get` menampilkan `lastUp.state` `interrupted` kalau `last_up` masih `running` tetapi operasinya tidak lagi aktif. Menjalankan Up lagi melanjutkan dari keadaan yang ada.
- **AC-16** (baca status, tanpa event baru): `env.get` dan `env.list` mendapat tambahan *DTO*: `EnvironmentSummary.activeOperation`, `EnvironmentDetail.lastUp`, `ServiceInstance.endpoints` (endpoint instance dengan port yang benar benar di bind), dan alasan baru di `ServiceNotReady` menurut *Alasan belum ready*. Selama Up berjalan, `lastUp` diisi dari `UpBoard` di memori daemon; sesudahnya dari kolom `last_up`. Tidak ada `EventKind` baru: klien memuat ulang `env.get` pada event operasi induk (state, stage, dan progress nya) dan pada `operation.state_changed` setiap operasi yang `parent` nya operasi induk itu, yang terbit di setiap pergantian tahap sebuah service.
- **AC-17** (`stackly env up`, REQ-028): `stackly env up [--file <path> | <environment> | <path>] [--locked] [AsyncArgs]` memanggil `env.plan` lalu `env.up` dengan `planDigest` dari plan itu. Manifest tidak valid keluar 2 (`cli.env_invalid`), plan terblokir keluar 3 (`cli.env_plan_blocked`), needs secret keluar 6 dengan satu baris per reference dan hint `stackly secret set <reference>`. Tanpa `--wait` ia mencetak operasi yang dimulai dan keluar 0. Dengan `--wait` ia menulis satu baris progress per perubahan `phase` ke stderr, lalu tabel service dengan endpoint yang benar benar dipakai (port `auto` tampil sebagai angka yang di bind), dan keluar 0 hanya kalau operasi `succeeded`; `env.up_incomplete` keluar 7. `--json` satu baris envelope dengan `result` `{ operation, idempotencyKey, environment }`. Argumen posisi yang memuat `/`, diawali `.` atau `~`, atau berakhiran `.yaml` atau `.yml` adalah path: folder berarti `<folder>/stackly.yaml`.
- **AC-18** (`stackly env status`, REQ-028): `stackly env status [--file <path> | <environment> | <path>]` memanggil `env.get` dengan `checkManifest` true dan mencetak nama, status, hitungan ready, tabel `SERVICE INSTANCE VERSION OWNERSHIP STATUS ENDPOINTS` dengan port yang benar benar di bind, lalu satu baris per service yang belum ready beserta alasannya. Ia keluar 0 selama environment ditemukan, apa pun statusnya; environment yang tidak dikenal keluar 10 (`cli.environment_not_found`). `--json` berisi `EnvironmentDetail`, sama persis dengan data halaman Environments.
- **AC-19** (tombol Up… dan Bring Up): di halaman detail environment, tombol Up… menggantikan Plan… di toolbar dan membuka sheet plan spec 0044 dengan tombol Cancel dan Bring Up. Bring Up aktif hanya saat plan `applicable`; saat tidak aktif alasannya tampil. Menekan Bring Up mengirim `env.up` dengan `planDigest` sheet itu; `env.plan_changed` memuat ulang plan dan menampilkan "The plan changed. Review it again." tanpa menjalankan apa pun. Item menu "Create with Up…" menggantikan Show Plan… untuk service yang belum dibuat. Catatan kaki sheet menjadi "Services start in dependency order and wait for readiness. If one fails, the rest stay up and the environment is marked partially ready."
- **AC-20** (progress dan banner partially ready): selama Up berjalan, halaman detail menampilkan `StateBanner` informative "Bringing up… {done} of {total}" dengan Cancel (hanya dari `OperationSummary.cancel`), dan kolom Status tiap service menampilkan `phase` nya menurut *Copy phase*. Sesudah Up, banner partially ready spec 0044 menampilkan satu `PartialItem` per service yang belum ready dengan alasan dari *Alasan belum ready* (misalnya "Waiting for postgres, which failed", "Needs secret local:erp/postgres/admin", "Start failed: readiness timed out") dan aksi yang sesuai: Set Secret…, Show Operation, atau membuka instance. Banner itu selalu menyebut service nya dengan teks, tidak hanya warna.
- **AC-21** (needs secret di app): `secret.needs_secret` dari `env.up` membuka `SecretEntrySheet` spec 0011 dengan reference dari `items`; sesudah semua terisi, sheet plan dimuat ulang dan Bring Up bisa ditekan lagi. Baris "Needs secret: {reference}" di sheet plan mendapat aksi Set… yang membuka sheet yang sama. Service yang berhenti `needs_secret` di tengah Up menampilkan aksi Set Secret… di banner.
- **AC-22** (permukaan lain dan state, NFR-012): Activity menampilkan operasi pengatur dengan judul "Bring up environment {name}", label "Partial" saat error nya `env.up_incomplete` dengan `ready` di atas 0, dan operasi anak nya bersarang di bawahnya lewat `OperationSummary.parent`; baris environment di sidebar, halaman daftar, dan Overview menampilkan "Bringing up…" selama `activeOperation` terisi; `OnboardingCopy.commandExamples` mendapat `stackly env up ~/Code/erp` dan baris "Environments as files" memakai kalimat "A stackly.yaml per project. Plan, then bring everything up in order."; setiap aksi baru memakai `.mutationGated()` dan punya state loading, gagal, dan offline dari `ViewState`.
- **AC-23** (kontrak): `ProtocolVersion::CURRENT` naik satu minor untuk seluruh kontrak fitur ini (nomor ditetapkan lead saat build); method `env.up` (`recovery refused`), tambahan DTO di modul `env` dan `operation`, kode baru area `env`, dan fixture `protocols/fixtures/env/` masuk dalam satu commit dengan `cargo xtask codegen`; skenario fixture `environment-bringing-up` dan `environment-up-incomplete` spec 0068 ada; `cargo xtask check-deps` dan `cargo check --target aarch64-pc-windows-msvc` untuk domain, platform, protocol, dan core tetap lulus.
- **AC-24** (tambahan engine): engine operasi mendapat tiga hal, tanpa mengubah perilaku jenis operasi lain: kelas slot pengatur yang diturunkan dari `OperationKind::is_coordinator()` dan dicatat di entri antrean, sehingga cek slot dan pelepasan slot memakai penghitung kelasnya (AC-3); `Origin::Child` yang mencatat `parent` di journal dan di `OperationSummary.parent`, memakai klien dan interaktivitas induknya, tanpa idempotency key, dan ditolak `RestartPending` selama gerbang quiesce aktif; dan `OperationEngine::wait_final(id)` yang dibangunkan dari perubahan operasi (tanpa polling) dan selesai saat operasi mencapai state akhir atau `needs_recovery`. `recover` menutup anak `create_instance` dan `start_instance` yang antre dan induknya ditutup `interrupted` (AC-15). `operation.get_stats` melaporkan jumlah pengatur berjalan dan batasnya. Test engine yang ada tetap lulus tanpa perubahan harapan.
- **AC-25** (bukti dan JOURNEY-04 bagian Up): graph diuji dengan provider `fake` saja (rantai, berlian, dependency gagal, timeout, batal), tanpa dependency karangan antar database. Satu test native `apps/daemon/tests/env_up_native.rs` menjalankan Up manifest contoh ERP dari cache runtime xtask tanpa unduhan sampai enam service `ready`, lalu Up kedua yang tidak mengerjakan apa pun; tanpa artefak di cache ia dicatat `BLOCKED` dengan `owner: user`. Lulus di fixture bukan evidence native.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan K1 sampai K18 di [rationale.md](rationale.md): Up adalah operasi pengatur yang memulai operasi anak `install_runtime`, `create_instance`, dan `start_instance` yang sudah ada dan tidak memakai slot berjalan selagi menunggu; lease dicatat bersama member sebelum graph berjalan; kegagalan sebagian tidak pernah membatalkan service yang sudah ready; dan reference secret yang belum terisi menolak Up sebelum mutasi.

Up tidak punya logika install, create, atau start sendiri. Ia hanya memutuskan apa yang dimulai, kapan, dan apa artinya kalau gagal.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

## Feature design

### Trace PRD

| ID PRD | Dicakup oleh |
| --- | --- |
| REQ-021 | AC-1, AC-4, AC-6, AC-7, AC-8, AC-10, AC-12 |
| REQ-028 | AC-17, AC-18 |
| BR-023 | AC-7, AC-11, AC-12 |
| VAL-014 | AC-5 (bagian struktur dan cek plan milik spec 0044 AC-8) |
| AC-021 | AC-8, AC-9, AC-10 |
| AC-073 (bersama fitur 47) | AC-13, AC-14 (bagian Down milik spec 0047) |
| PRD bagian 13.1 | AC-1 (`--locked` diteruskan ke plan), AC-2 (secret tidak pernah diganti nilai kosong), AC-17 |
| PRD bagian 13.2 | AC-5, AC-8, AC-13, AC-25 |

### Batas dengan spec lain

| Hal | Pemilik |
| --- | --- |
| Schema manifest, validate, plan, `planDigest`, import, remove, halaman Environments, sheet plan | spec 0044, dipakai ulang |
| `stackly.lock`, `.stackly.local.yaml`, arti `locked`, blocker lock, field lock di `EnvPlan` | spec 0045 (*Kontrak `--locked`*) |
| Operasi `environment_up`, operasi anak, graph, timeout, pengambilan lease, status needs secret, `env up`, `env status`, tombol Up…, alasan di banner | spec ini |
| Operasi `environment_down`, pelepasan lease, override stop, Stop Services, `agent_off`, ganti endpoint pada instance yang dipinjam, konfirmasi Down, baris Lease | spec 0047 |
| Install runtime, create instance, start dan readiness | spec 0018, 0020, 0010, 0021 (dipanggil sebagai operasi anak) |
| Engine operasi | spec 0007 (ditambah kelas pengatur, `Origin::Child`, `wait_final`; lihat *Update spec lain*) |
| Demand dan rotasi sesi | spec 0031 (aturannya tidak berubah; lihat *Lease*) |

### Istilah

- **Operasi pengatur** (coordinator): operasi yang hanya menulis registry, memulai operasi lain, dan menunggu. `environment_up` adalah yang pertama.
- **Operasi anak**: operasi biasa (`install_runtime`, `create_instance`, `start_instance`) yang dimulai operasi pengatur dan membawa `parent`.
- **Phase**: keadaan satu service di dalam satu Up (*Phase service*).
- **Edge**: satu entri `dependsOn`, dari dependent ke dependency.

### Kontrak `--locked` (disepakati dengan spec 0045)

1. Up tidak pernah me resolve sendiri di mode apa pun. Ia menghitung ulang plan lewat `environment::plan` dan hanya memakai `PlanItem`: install menurut `runtime.artifactSha256` dan `artifactId` (apa pun `PlanRuntime.source` nya, `lock` atau `catalog`), create dengan provider dan endpoint dari plan (override port lokal sudah ada di `PlanItem.endpoints`).
2. `locked` adalah masukan plan: `PlanInputs { manifest, lock, overrides, locked }`. `EnvUpParams.locked` diteruskan apa adanya. Arti `locked`, cek lock dan override, dan setiap kode blocker nya (`env.lock_*`, `env.local_*`, `env.override_location_unknown`) milik spec 0045 dan muncul di `EnvPlan.blockers`, sehingga Up menolak dengan `env.plan_blocked` tanpa mutasi. Spec ini tidak mendefinisikan kode lock apa pun.
3. Lock dipakai juga tanpa `--locked` (aturan plan spec 0045): service yang entri lock nya cocok memakai artefak yang dikunci. Up tidak perlu tahu bedanya. GUI selalu mengirim `locked` false; `--locked` hanya ada di CLI.
4. `planDigest` memuat apa pun yang spec 0045 tambahkan (`locked`, digest lock, digest override lokal, dan per item sumber runtime, revision provider, dan lokasi storage). Up hanya membandingkan digest.
5. Up tidak pernah menulis `stackly.lock` atau `.stackly.local.yaml`.
6. Langkah `accept` menulis `environments.lock_sha256` dari `EnvPlan.lock.sha256` (null kalau tidak ada lock yang terbaca dan valid), lewat argumen `lock_sha256` yang spec 0045 tambahkan ke `Environment::set_manifest` dan `environment::register`.
7. Dua field item `create` dari spec 0045 diteruskan Up ke create: `PlanItem.providerRevision` (create anak menolak kalau key entri catalog di memori berbeda dari field ini) dan `PlanItem.storage.locationId` (dikirim sebagai `CreateDraft.data_location`; kosong berarti lokasi default). Keduanya diisi `environment::plan::draft_for`, jadi Up tidak punya logika sendiri untuk itu.

8. Daemon lama mengabaikan field `locked` yang tidak dikenalnya. Karena itu `stackly env up --locked` memeriksa `EnvPlan.lock.locked` pada plan yang baru diambilnya, dan kalau nilainya bukan true ia berhenti dengan `cli.env_locked_unsupported` (kode CLI milik spec 0045, kategori `unsupported`, exit 5) sebelum memanggil `env.up`.

Nama field di atas adalah nama final dari spec 0045 (dikonfirmasi `arc-f45-spec`, 2026-11-13). Kalau spec 0045 yang di merge memakai nama lain, nama spec 0045 yang berlaku; perilaku delapan butir ini tidak berubah.

### Precheck Up

Handler `env.up` (`apps/daemon/src/ipc/env/up.rs`), berurutan:

1. Validasi params (`EnvUpParams::validate`: aturan path spec 0044, `expectedPlanDigest` 64 hex huruf kecil).
2. `ipc::operation::replay_operation`: permintaan ulang dengan key yang sama membalas balasan pertama tanpa langkah berikutnya.
3. Di `spawn_blocking`: baca manifest (`EnvSource` file, atau `manifest_location` environment terdaftar) dan hitung plan dengan `PlanInputs` lengkap. File yang tidak bisa dibaca: `env.manifest_unreadable`.
4. Di gerbang `begin` (sebuah `Precheck` murni atas plan itu), nilai pertama yang cocok menang:

| Kondisi | Error |
| --- | --- |
| `validation.valid` salah | `env.manifest_invalid` |
| ada blocker selain `env.instance_blocked` ber `reason` `needs_secret` | `env.plan_blocked` |
| plan `applicable` dan `planDigest` bukan `expectedPlanDigest` | `env.plan_changed` |
| ada blocker `env.instance_blocked` ber `reason` `needs_secret`, atau secret `missing` atau `unregistered` di item `create` | `secret.needs_secret` (AC-2), dengan semua reference dari kedua sumber |

Untuk blocker instance, reference dan alasannya diambil dari `InstanceBlocker::NeedsSecret` instance itu di supervisor. Plan yang tidak `applicable` tidak punya `planDigest`, jadi baris ketiga dilewati untuknya; itu aman karena baris keempat selalu menolak.

5. ID environment: `EnvPlan.environment.id` kalau terdaftar; selain itu `EnvironmentId::generate()` dengan `creates_resource: true`.
6. `start_operation` dengan `BeginRequest` AC-3. `input` journal: `{ source, environmentId, name, manifestSha256, planDigest, locked }` (tanpa isi manifest, jauh di bawah 16 KiB). `environmentId` dan `name` ada supaya replay bisa membangun balasan yang sama.

Balasannya `EnvUpReply` begitu operasi diterima; klien menunggu lewat event operasi.

### Operasi `environment_up`

`OperationKind::EnvironmentUp`: `resumable` false, `interrupt_policy` `CloseInterrupted`, `is_coordinator()` benar, tanpa catatan item dibuat, tanpa `WorkFactory` (operasi yang masih antre saat daemon berhenti ditutup `interrupted`). Journal nya dua langkah, keduanya `irreversible`:

| Langkah | Stage | Isi |
| --- | --- | --- |
| `accept` | `accept` | baca ulang manifest, cocokkan `manifestSha256` (beda: gagal `env.manifest_changed`), hitung ulang plan (tidak `applicable`: gagal `env.plan_blocked`), lalu commit AC-4. Digest tidak dibandingkan lagi di sini: perubahan lifecycle sesudah precheck ditangani *Aturan graph*, dan perubahan instance yang diikat ditangkap guard |
| `run` | `install`, lalu `create`, lalu `start` (lewat `set_stage`) | AC-6, AC-7, lalu *Aturan graph* |

Tahap install, create, dan start sengaja satu langkah journal: sesudah cancel, engine tidak mengizinkan operasi `CloseInterrupted` membuka langkah baru (`begin_step` mengembalikan `Cancelled`), sedangkan Up masih harus menutup anak dan mencatat laporannya.

**Commit penutup.** Sebelum mengembalikan hasil apa pun dari langkah `run` (sukses, `env.up_incomplete`, atau `Cancelled`), pekerjaan memanggil `commit_guarded` sekali untuk menulis `last_up` dengan state akhir dan laporan per service; `commit_guarded` tidak memeriksa sinyal berhenti, jadi ia juga berjalan sesudah cancel. Kalau `accept` gagal, tidak ada `last_up` yang ditulis. Kalau daemon berhenti (abandon), commit penutup tidak berjalan dan `last_up` tetap `running`, yang ditampilkan `interrupted` (AC-15).

**Tulisan environment selalu segar.** `WriteOp::UpdateEnvironment` menulis ulang environment beserta semua member nya. Karena itu setiap tulisan environment selama Up, oleh induk (`accept`, commit penutup) maupun oleh anak create (`register`, kompensasi), adalah `commit_guarded` yang membaca environment di dalam guard lewat `GuardView::environments()` dan memakai revision yang dibacanya sebagai `expected`. Tidak ada yang menulis dari `Environment` yang dibaca sebelumnya, supaya member yang ditambah anak tidak terhapus.

`set_progress` memakai `Progress::Counted { done, total }` dengan `total` jumlah service dan `done` jumlah service yang sudah berakhir (ready atau tidak).

**`UpBoard`.** `stackly_core::environment::up::UpBoard` adalah tabel progress di memori (`Arc`, satu untuk seluruh daemon, dibuat di `DaemonCore`): peta dari `EnvironmentId` ke laporan Up yang sedang berjalan (`operationId`, `startedAt`, phase tiap service). Pekerjaan Up menulisnya di setiap pergantian phase dan menghapus entrinya sesudah commit penutup; `env.get` membacanya. Ia tidak pernah ditulis ke disk.

**Operasi anak.** Core tidak mengenal handler IPC, jadi operasi pengatur memulai anak lewat port baru di `stackly-core`:

```rust
// crates/core/src/environment/up/children.rs
pub trait UpChildren: Send + Sync + 'static {
    /// Starts `install_runtime` for one artifact, as `runtime.install` does.
    fn install(&self, parent: &ChildOrigin, provider: &ProviderName, artifact: Sha256Digest)
        -> BoxFuture<'_, Result<ChildStart<RuntimeId>, FailureError>>;
    /// Starts `create_instance` for one service, bound to the environment.
    fn create(&self, parent: &ChildOrigin, draft: CreateDraft, binding: EnvironmentBinding)
        -> BoxFuture<'_, Result<ChildStart<InstanceId>, FailureError>>;
    /// Starts `start_instance` with the environment trigger.
    fn start(&self, parent: &ChildOrigin, instance: InstanceId)
        -> BoxFuture<'_, Result<ChildStart<InstanceId>, FailureError>>;
}

pub enum ChildStart<T> {
    /// The operation runs; wait for it with `OperationEngine::wait_final`.
    Started { operation: OperationId, subject: T },
    /// Nothing to do: the runtime is installed or the instance already runs.
    AlreadyDone(T),
}
```

`ChildOrigin` dipakai juga oleh port `DownChildren` [spec 0047](../0047-env-down-shared-instance-lease/index.md), jadi tipenya tinggal di `crates/core/src/environment/children.rs` (`environment::children`), bukan di `environment::up::children` *(diselaraskan 2026-11-13 dengan spec 0047)*.

`stacklyd` mengimplementasikannya (`apps/daemon/src/ipc/env/children.rs`) dengan memanggil fungsi yang sama dengan handler `runtime.install`, `instance.create`, dan `instance.start`, yang dipecah menjadi bagian "params ke permintaan internal" dan bagian "permintaan internal ke `begin`". Dengan begitu gate trust, `gather` dan `evaluate` create, `install_precheck`, dan cek data spec 0025 berjalan persis seperti untuk permintaan klien. `BusyPolicy` anak sama dengan handler nya (`Queue` untuk install dan start, `Reject` untuk create). Kalimat ini berlaku untuk anak Up saja: anak `stop_instance` Down memakai `Queue` dan precheck sendiri, padahal stop klien `Reject` (spec 0047, *Operasi `environment_down`*); `BusyPolicy` dan precheck sebuah anak dipilih port yang memulainya. Penolakan saat memulai anak (`Err(FailureError)`: error precheck, atau `BeginError` yang dipetakan lewat `ipc::operation::begin_error` yang sudah ada, misalnya `resource.busy`, `operation.queue_full`, `operation.recovery_required`, `daemon.restart_pending`) dan operasi anak yang berakhir `failed`, `cancelled`, atau `needs_recovery` sama sama menjadi `failed` untuk service itu, dengan kode error anak; `needs_recovery` memakai kode `operation.recovery_required`. Dua pengecualian:

- Penolakan `daemon.restart_pending` (gerbang quiesce): driver berhenti memulai anak baru, menunggu anak yang berjalan, dan menandai service yang belum berakhir `cancelled`. Up berakhir `failed` dengan `env.up_incomplete`, seperti Up yang terputus.
- Anak `start_instance` yang masih `queued` lebih lama dari `CHILD_QUEUE_TIMEOUT_SECONDS` 120 (instance nya dikunci operasi lain yang lama) dibatalkan induknya, dan service itu `failed` di tahap `start` dengan kode `resource.busy`. Dengan begitu setiap service pasti berakhir.

Hasil tanpa operasi dipetakan ke `ChildStart::AlreadyDone`: `InstallOutcome::AlreadyInstalled` untuk install, dan `BeginOutcome::NoOp` untuk start (instance sudah `starting`, `running`, atau `recovering`).

**Sumber interaksi secret.** `ChildOrigin` membawa `SecretCaller` dari permintaan `env.up` (klien dan `interactive`). Create memakainya untuk `put` `Bind` (spec 0011 AC-23), dan start memakainya untuk percobaan pertama saja (`StartTrigger::Environment(SecretCaller)`). Di setiap cabang yang membedakan pemicu, `Environment` berperilaku seperti `User` (tidak dilewati saat sesi berakhir, tidak dilewati saat desired bukan `running`, menulis desired `running`, dihitung ke budget seperti start pengguna), dengan satu beda: ia tidak menulis lease `manual`, baik di `manual_lease_ops` maupun di jalur tanpa operasi `start_precheck` (`no_op_with_desired`), yang menerima pemicu sebagai parameter.

### Phase service

| `phase` | Arti | Akhir |
| --- | --- | --- |
| `pending` | belum dikerjakan | |
| `installing` | menunggu install runtime nya | |
| `creating` | operasi create berjalan | |
| `waiting` | menunggu dependency (`waitingOn` berisi alias yang belum ready) | |
| `starting` | operasi start berjalan, atau menunggu instance yang sudah jalan menjadi ready | |
| `ready` | instance `running` dan readiness `ready` | ya |
| `failed` | tahap `install`, `create`, atau `start` gagal (`stage`, `code`, `operationId`) | ya |
| `needs_secret` | create atau start berhenti dengan `secret.needs_secret` (`references`) | ya |
| `skipped` | dependency tidak ready (`dependency`, `reason` `failed`, `timeout`, atau `not_ready`) | ya |
| `cancelled` | Up dibatalkan sebelum service ini selesai | ya |

`UpPhase` adalah enum terbuka di wire. `needs_secret` adalah bentuk khusus `failed`: kode error anak `secret.needs_secret`, dengan `references` disalin dari `items` error itu.

### Aturan graph

Fungsi murni `stackly_core::environment::up::graph` (tanpa IO, diuji tanpa fake): masukannya `order` dan edge dari plan dengan `timeoutSeconds` efektif; keadaannya phase setiap service; keluarannya daftar service yang boleh dimulai sekarang dan service yang harus ditutup. Driver async memanggilnya setiap kali ada peristiwa (anak selesai, timer edge habis, status instance berubah, cancel).

1. Service masuk tahap start sesudah langkah `create` selesai untuk semua service. Service yang `failed` atau `needs_secret` di install atau create sudah berakhir.
2. Service tanpa dependency, atau yang semua dependency nya `ready`, boleh mulai. Tepat sebelum mulai, instance setiap dependency dibaca lagi dari supervisor; kalau ada yang bukan `running` dan `ready`, service ini `skipped` (`reason` `not_ready`), dan phase dependency itu tetap apa adanya di laporan.
3. Saat sebuah service mulai, aksinya diturunkan dari lifecycle instance saat itu (bukan dari plan), memakai tabel *Lifecycle ke aksi* spec 0044:
   - `running` dan `ready`: `ready`.
   - `running` belum ready, `starting`, `initializing`, `recovering`, `stopping`: tunggu status instance sampai `running` dan `ready`, sampai lifecycle keluar dari lima nilai itu (lalu turunkan aksi lagi), atau sampai `SERVICE_SETTLE_TIMEOUT_SECONDS` 120 lewat sejak service ini mulai (`failed`, tahap `start`, kode `env.service_not_ready`). `stopping` ditunggu, bukan langsung di start, karena `start_instance` menolak instance yang sedang `stopping` (`supervisor.instance_stopping`).
   - `stopped`, `failed`: operasi anak `start_instance`. Hasil `succeeded` berarti `ready` sesudah status instance dibaca sekali lagi (`running` dan `ready`); selain itu `failed` atau `needs_secret` dengan kode error anak.
   - `uninitialized`: operasi anak `start_instance` juga, yang gagal dengan kode supervisor nya (`supervisor.not_initialized`). Up tidak pernah menginisialisasi instance yang sudah ada. Sejak update [spec 0044](../0044-manifest-environment-validate-plan/index.md) tanggal 2026-11-13, plan sudah menolak instance seperti ini sebagai blocker `env.instance_blocked` ber `reason` `not_initialized`, jadi cabang ini hanya tercapai kalau keadaan instance berubah sesudah precheck.
   - `unknown`, `retained`, atau instance ber `blocker` yang menolak start: `failed`, tahap `start`, kode `env.instance_blocked`.

   Lifecycle dibaca dari baris instance di registry dan readiness dari `Supervisor::status(id)` yang sudah ada, di polling setiap `UP_POLL_INTERVAL` 250 milidetik selama sebuah service menunggu; tidak ada langganan event baru. Readiness `unknown` atau `stale` berarti "belum", tidak pernah "gagal".
4. Timer edge: untuk edge dari dependent D ke dependency P, timer mulai saat P masuk `starting` (atau saat tahap start dimulai kalau P sudah `ready`), selama `timeoutSeconds` edge itu, default `DEFAULT_DEPENDENCY_TIMEOUT_SECONDS` 600. Waktu install, create, dan waktu P menunggu dependency nya sendiri tidak dihitung. Timer yang habis selagi P belum berakhir membuat D `skipped` (`reason` `timeout`); P sendiri tetap dilanjutkan dan hasilnya dicatat apa adanya.
5. P yang berakhir tidak `ready` membuat setiap D yang masih `waiting` langsung `skipped` (`reason` `failed`), dan itu merambat ke dependent D.
6. Paling banyak `UP_START_CONCURRENCY` start berjalan sekaligus; yang boleh mulai dipilih menurut `order`.
7. Tahap start selesai saat setiap service berakhir.

Cycle sudah ditolak validate (spec 0044 AC-2), jadi graph selalu selesai. Konstanta ada di `crates/core/src/environment/up/limits.rs`.

### Lease

**Bentuk.** Satu baris `instance_demands` (spec 0005) per pasangan instance dan environment yang sedang butuh: `id` `LeaseId` baru (UUID v7), `instance_id`, `source_kind` `environment`, `source_id` id environment, `session_id` null, `state` `active`, `acquired_at` waktu commit. Di domain: `Lease::new(id, instance, LeaseHolder::Environment(environment), None, now)`. Tidak ada kolom baru untuk lease.

**Identitas pemegang.** `EnvironmentId` baris `environments`. Bukan nama, bukan path manifest, bukan koneksi klien, bukan sesi login, dan bukan ID operasi Up. Relink manifest (spec 0044 AC-11) tidak mengubah lease.

**Kapan dicatat.** Selalu bersama keanggotaan, dalam transaksi yang sama:

- member yang alias nya ada di manifest dan service `instanceRef`: di commit langkah `accept` (AC-4);
- instance dedicated baru: di batch `register` operasi anak create (AC-7).

Jadi sesudah tahap `create`, setiap member yang alias nya ada di manifest punya tepat satu lease `active`, sebelum service pertama dinyalakan. Service yang create nya gagal tidak punya instance, member, maupun lease. Member `detached` tidak pernah diberi lease baru oleh Up; ia memegang lease hanya kalau lease itu sudah ada dari Up sebelumnya dan belum dilepas Down.

**Yang tidak dilakukan lease.** Mencatat lease tidak mengubah desired, lifecycle, atau `startup` instance. Desired `running` ditulis operasi `start_instance` (pemicu `environment`), tanpa lease `manual`. Service yang dilewati atau gagal start tetap punya lease dengan desired apa adanya.

**Rotasi sesi dan login (jawaban Follow-up spec 0031).** Aturan `plan_session_rotation` tidak berubah: lease environment tidak pernah dilepas atau di suppress oleh rotasi, dan instance `manual` yang punya lease environment `active` tidak diberi desired `stopped`. Akibatnya, sesudah logout dan login lagi, instance environment yang desired nya `running` dinyalakan antrean start spec 0031 AC-7, urut nama dengan `RECONCILE_START_CONCURRENCY`, bukan urut dependency. Urutan dependency hanya dijamin oleh Up. Override pada `instance.stop_all` dan `agent_off` milik spec 0047.

**Model data** (migrasi dengan nomor bebas berikutnya, ditetapkan lead saat build; jenis kolom memakai nama jenis spec 0005):

- `environments.last_up` `json` objek, nullable: laporan Up terakhir, `{ operationId, state, startedAt, finishedAt?, services: [{ alias, phase, stage?, code?, dependency?, reason?, operationId?, references? }] }`. `state` salah satu `running`, `succeeded`, `failed`, `cancelled`. Paling banyak 32 service; `references` paling banyak 8 per service, sisanya dipotong.
- `operation_journal.parent_id` `uuid` nullable, tanpa FK: operasi induk sebuah operasi anak.
- `environment_instances.dependency_conditions` (kolom yang sudah ada, default `'[]'`) mulai diisi: array `{ service, condition, timeoutSeconds }`, yaitu edge yang dipakai Up terakhir untuk member itu dengan `timeoutSeconds` efektif.
- `environments.last_operation_id` (kolom yang sudah ada) mulai diisi dengan ID operasi `environment_up` terakhir.
- Kolom yang menyimpan kode set terbuka (`last_up` tidak, karena JSON) didaftarkan sesuai aturan spec 0034 AC-8 kalau migrasi menambahnya.

**Domain** (`stackly-domain`, usulan untuk spec 0004 dan 0005):

- `EnvironmentMember` mendapat `dependency_conditions: Vec<MemberDependency>`; `MemberDependency { service: MemberAlias, condition: DependencyCondition, timeout_seconds: u32 }`.
- `Environment` mendapat `last_operation: Option<OperationId>` dan `last_up: Option<UpReport>`; mutator `bind_member(member)` (menolak alias atau instance ganda dengan invariant yang ada), `set_member_conditions(alias, conditions)`, `begin_up(operation, started_at)`, dan `finish_up(report)`, masing masing menaikkan revision 1.
- `UpReport`, `ServiceReport`, dan `UpPhase` (nilai *Phase service*) sebagai tipe domain; `UpPhase` memakai `code_enum!`.
- `OperationKind::EnvironmentUp`: `resumable()` salah, `interrupt_policy()` `CloseInterrupted`, `is_coordinator()` benar (metode baru, salah untuk jenis lain). `EnvironmentDown` tidak diubah di sini.
- `GuardView::environments()` (spec 0044) dipakai guard `accept` dan guard `register` create.
- `WriteOp::DeleteInstanceCreatedBy` ikut menghapus baris `environment_instances` instance itu lebih dulu (FK nya `restrict`) dan menaikkan revision environment nya, di dalam SQL `delete_instance_created_by` sendiri, sehingga kompensator dan kompensasi otomatis sesudah restart tidak perlu tahu ikatannya; baris `instance_demands` instance itu ikut terhapus lewat `ON DELETE CASCADE` yang sudah ada.

**Core** (`crates/core/src/environment/up/`): `mod.rs` (pekerjaan operasi), `precheck.rs`, `accept.rs` (guard murni), `graph.rs` (aturan murni), `driver.rs`, `children.rs` (port), `board.rs` (`UpBoard`), `report.rs`, `limits.rs`. `crates/core/src/instance/create/` mendapat `EnvironmentBinding { environment: EnvironmentId, alias: MemberAlias, conditions: Vec<MemberDependency> }` sebagai masukan opsional `CreateInstanceWork`, dipakai hanya di langkah `register`, dan ikut di encode ke serta di decode dari `input` journal create (`decode_input`). `crates/core/src/supervisor/startup.rs` mendapat `StartTrigger::Environment(SecretCaller)` dan `StartRequest::environment(instance, caller)`; `manual_lease_ops` tetap hanya menulis lease untuk `StartTrigger::User`.

### Alasan belum ready

`EnvironmentService.notReady` (spec 0044) dihitung `env.get` dari keadaan instance sekarang dan dari laporan `lastUp`. Aturan pertama yang cocok menang:

| # | Kondisi | `reason` | Tambahan |
| --- | --- | --- | --- |
| 1 | Up sedang berjalan dan service belum berakhir | `bringing_up` | `phase`, `waitingOn` |
| 2 | instance ada dengan blocker `needsSecret` | `needs_secret` | `references` dari blocker |
| 3 | instance ada, lifecycle bukan `stopped` atau `uninitialized` | nilai spec 0044 (`starting`, `stopping`, `failed`, `not_ready`, `unavailable`) | `detail` spec 0044 |
| 4 | `lastUp` punya entri service ini ber `phase` `needs_secret` | `needs_secret` | `references` |
| 5 | `lastUp` punya entri ber `phase` `skipped` | `dependency_not_ready` | `dependency`, `dependencyReason` |
| 6 | `lastUp` punya entri ber `phase` `failed` | `up_failed` | `stage`, `code`, `operationId` |
| 7 | lainnya | `stopped` atau `not_created` (spec 0044) | |

Aturan 4 sampai 6 hanya berlaku kalau `lastUp.state` bukan `running`. Jadi keadaan instance yang hidup selalu menang atas laporan lama, dan laporan Up hanya menjelaskan service yang sekarang diam.

### API surface

Satu method baru. Lewat handshake dan peer user yang sama (spec 0006).

| Method | Jenis | Recovery | Input | Output | Error utama |
| --- | --- | --- | --- | --- | --- |
| `env.up` | operasi `environment_up` | refused | `EnvUpParams { source: EnvSource, expectedPlanDigest, locked, idempotencyKey }` | `EnvUpReply { environmentId, name, operation: OperationAccepted }` | `env.manifest_unreadable`, `env.manifest_invalid`, `env.plan_blocked`, `env.plan_changed`, `secret.needs_secret`, `resource.busy`, `resource.revision_mismatch`, `catalog.unavailable`; sebagai kegagalan operasi: `env.manifest_changed`, `env.plan_blocked`, `env.plan_changed`, `env.name_taken`, `env.limit_reached`, `env.up_incomplete` |

**DTO** (camelCase; enum terbuka berakhir `unrecognized`; field baru di tipe yang sudah ada memakai `#[serde(default)]`):

- `EnvUpParams`: `locked` default false. `Validate`: aturan path `EnvSource`, digest 64 hex, key spec 0006.
- `EnvironmentSummary` mendapat `activeOperation?: EnvOperationRef { id, kind: OperationKind }`: operasi `environment_up` atau `environment_down` (spec 0047) yang belum berakhir untuk environment itu, dari `OperationEngine::list` dengan `active_only` dan `resource` environment itu (satu panggilan per environment di `env.get`; untuk `env.list`, satu panggilan `active_only` yang disaring per resource).
- `EnvironmentDetail` mendapat `lastUp?: EnvUpReport { operationId, state: UpState, startedAt, finishedAt?, services: [ServiceUpState] }`; `UpState` `running`, `succeeded`, `failed`, `cancelled`, `interrupted`; `ServiceUpState { alias, phase: UpPhase, stage?: UpStage, code?, dependency?, dependencyReason?: DependencyFailure, waitingOn: [alias], operationId?, references: [NeedsSecretItem] }`; `UpStage` `install`, `create`, `start`, `wait`; `DependencyFailure` `failed`, `timeout`, `not_ready`. `references` memakai `NeedsSecretItem` modul `secret` yang sudah ada (`reference`, `reason`, `generateAllowed`).
- `ServiceInstance` mendapat `endpoints: [EndpointStatus]` (tipe modul `instance` yang sudah ada, dengan `port` yang benar benar di bind dan `bound`).
- `ServiceNotReady` mendapat `phase?`, `waitingOn`, `references`, `dependency?`, `dependencyReason?`, `stage?`, `code?`, `operationId?`; `NotReadyReason` mendapat `bringing_up`, `needs_secret`, `dependency_not_ready`, `up_failed`.
- `OperationSummary` mendapat `parent?: Uuid`. `OperationStats` mendapat `coordinators: u32` dan `maxCoordinators: u32` (jenis yang sama dengan `running` dan `maxRunning`); `running` tetap hanya menghitung operasi yang memakai slot.

**Event**: tidak ada `EventKind` baru. Klien memuat ulang `env.get` halaman yang terbuka (dengan `checkManifest` false) dan `env.list` pada `operation.state_changed` dan `operation.progress` operasi ber resource `environment`, dan pada `operation.state_changed` operasi yang `parent` nya sama dengan `activeOperation.id` environment itu, dengan jeda 300 ms yang sama dengan spec 0044; event instance tetap seperti spec 0044. `stackly env up --wait` memakai aturan yang sama untuk baris progress nya.

**Anggaran frame**: `EnvUpReport` paling besar 32 service kali sekitar 1 KiB; error `env.up_incomplete` paling banyak 32 `items`.

### Kode error di wire

Area `env` (spec 0044). Semua `retryable` false kecuali disebut. Pesan lowercase tanpa titik.

| Kode | Kategori | Kapan | Kunci `details` | `items` |
| --- | --- | --- | --- | --- |
| `env.plan_blocked` | `conflict` | Up untuk plan yang tidak `applicable` | `count`, `omitted` | `blocker`: `service`, `code`, `message`; paling banyak 32 |
| `env.plan_changed` | `conflict` | plan sekarang berbeda dari yang ditinjau, atau instance yang diikat berubah sebelum `accept` | | |
| `env.up_incomplete` | `failed` | Up selesai dengan service yang tidak ready | `ready`, `total`, `count`, `omitted` | `service`: `service`, `phase`, `stage`, `code`, `dependency`, `operationId`; paling banyak 32 |
| `env.dependency_not_ready` | `failed` | kode service `skipped` | `service`, `dependency`, `reason` (`failed`, `timeout`, `not_ready`), `timeoutSeconds` | |
| `env.service_not_ready` | `timeout` | instance yang sudah jalan tidak menjadi ready dalam `SERVICE_SETTLE_TIMEOUT_SECONDS` | `service`, `instance`, `timeoutSeconds` | |

`env.manifest_invalid`, `env.manifest_changed`, `env.manifest_unreadable`, dan `env.instance_blocked` dipakai ulang dari spec 0044. `env.dependency_not_ready`, `env.service_not_ready`, dan `env.instance_blocked` hanya muncul sebagai `code` di dalam laporan dan `items`, tidak sebagai error balasan. `secret.needs_secret`, `resource.busy`, dan `resource.revision_mismatch` adalah kode yang sudah ada.

Kode lokal CLI: tidak ada yang baru. `cli.env_invalid`, `cli.env_plan_blocked`, dan `cli.environment_not_found` dipakai ulang dari spec 0044.

### CLI

Di grup `stackly env` (`apps/cli/src/commands/env/`), memakai `AsyncArgs` spec 0008.

**Target.** `up` dan `status` menerima tepat satu dari: `--file <path>`; argumen posisi berupa path, yaitu yang memuat `/`, diawali `.` atau `~`, atau berakhiran `.yaml` atau `.yml` (folder berarti `<folder>/stackly.yaml`, file dipakai apa adanya); atau argumen posisi lain (nama atau UUID environment, aturan `<environment>` spec 0044; nama environment tidak pernah memuat `/` atau `.`). Tanpa ketiganya: `./stackly.yaml`. Path dibuat absolut dengan helper spec 0044. Untuk path, `status` memanggil `env.plan` untuk file itu dan memakai `EnvPlan.environment.id` saat `registration` `registered`; manifest yang tidak valid keluar 2 (`cli.env_invalid`) dengan saran memakai nama environment.

| Perintah | Perilaku | `result` `--json` |
| --- | --- | --- |
| `stackly env up [target] [--locked] [--wait] [--timeout <durasi>] [--idempotency-key <key>]` | `env.plan` (dengan `locked`), cetak ringkasan plan spec 0044 ke stderr, lalu `env.up`. Tanpa `--wait`: "Started bringing up <name> (operation <id>)". Dengan `--wait`: progress ke stderr (`install <software> <version>`, `create <instance>`, `start <instance>`, `ready <instance> <host>:<port>`, `skip <service>: waiting for <dependency>`), lalu tabel `SERVICE INSTANCE STATUS ENDPOINTS` ke stdout dan ringkasan "<ready> of <total> ready." Tanpa konfirmasi. | `{ operation, idempotencyKey, environment: EnvironmentDetail \| null }` (`environment` terisi dengan `--wait`) |
| `stackly env status [target]` | `env.get` dengan `checkManifest` true. Baris kepala `<name>  <Status>  <ready> of <total> ready  <file>`, tabel AC-18, lalu `not ready:` dengan satu baris `<service>: <alasan>` per service, dan `manifest: <state>` kalau bukan `in_sync`. Environment belum terdaftar untuk file itu: "Not imported. Run stackly env up or stackly env import." dan exit 10. | `EnvironmentDetail` |

Exit code hanya dari kategori (spec 0008): plan tidak valid 2, terblokir 3, `--locked` terhadap daemon yang tidak mengenalnya 5 (`cli.env_locked_unsupported`, spec 0045), `env.plan_changed` 3, `resource.busy` 3, `secret.needs_secret` 6, `env.up_incomplete` 7, dibatalkan 9. `--wait` yang kehabisan `--timeout` keluar 8 (`cli.wait_timeout`) tanpa membatalkan Up. `ENDPOINTS` menampilkan `<name> <host>:<port>` dipisah koma, dengan port dari `ServiceInstance.endpoints`; endpoint yang belum di bind tampil sebagai `<name> -`.

### Layar

Semua komponen dari `Components/`, `State/`, `Status/`; copy bahasa Inggris; spec menang atas mockup untuk perilaku.

**Toolbar halaman detail**: Validate, lalu kelompok kanan dengan tempat Down (spec 0047) dan tombol accent Up…. Plan… tidak lagi ada; sheet yang sama dibuka Up…. Up… nonaktif dengan alasan saat `activeOperation` terisi ("Already bringing up"), atau "Bringing down" saat `activeOperation.kind` `environment_down` (spec 0047), atau saat `manifestState` `missing`, `unreadable`, `invalid`, atau `name_changed` (alasan dari banner manifest).

**Sheet plan** (tambahan atas spec 0044): tombol Cancel dan Bring Up (accent, tombol default). Bring Up nonaktif saat plan tidak `applicable`, dengan alasan "Resolve the blockers above first." Baris "Needs secret: {reference}" mendapat tombol Set…. Saat `needsSecrets` di atas 0 untuk reference `missing` atau `unregistered`, Bring Up tetap aktif; menekannya menghasilkan `secret.needs_secret`, yang membuka `SecretEntrySheet` (AC-21). Dari alur Import… tombolnya tetap Cancel dan Import (spec 0044); Up dilakukan dari halaman detail sesudah import.

**Selama Up** (`activeOperation` terisi dengan `kind` `environment_up`; banner Down milik spec 0047): `StateBanner` informative di atas halaman, judul "Bringing up… {done} of {total}", aksi Cancel hanya kalau `OperationSummary.cancel` mengizinkan, dan Show in Activity. Kolom Status tabel Services menampilkan *Copy phase* untuk service yang belum berakhir; service yang sudah berakhir menampilkan status instance seperti biasa.

**Copy phase** (`StatusAppearance.upPhase(phase:)`, ditambahkan ke `apps/macos/design.md`):

| `phase` | Teks | Nada | Simbol |
| --- | --- | --- | --- |
| `pending` | "Queued" | neutral | `clock` |
| `installing` | "Installing…" | informative | `arrow.down.circle` |
| `creating` | "Creating…" | informative | `plus.circle` |
| `waiting` | "Waiting for {alias}" (alias pertama di `waitingOn`, "+{n}" kalau lebih) | neutral | `hourglass` |
| `starting` | "Starting…" | informative | `play.circle` |
| `unrecognized` | "Working…" | neutral | `ellipsis.circle` |

**Banner partially ready** (tambahan atas spec 0044): alasan tiap `PartialItem` dari `notReady.reason`:

| `reason` | Teks | Aksi |
| --- | --- | --- |
| `needs_secret` | "Needs secret {reference}" (reference pertama, "+{n}" kalau lebih) | Set Secret… (`SecretEntrySheet`) |
| `dependency_not_ready` | "Waiting for {dependency}, which failed", "Waiting for {dependency} timed out", atau "Waiting for {dependency}, which is not ready" | membuka service dependency itu |
| `up_failed` | "{Stage} failed: {pesan kode}" (`Install`, `Create`, `Start`) | Show Operation (Activity, operasi `operationId`) |
| nilai spec 0044 | teks spec 0044 | aksi spec 0044 |

Banner yang sama dipakai saat `readyCount` 0 dan `lastUp.state` `failed` atau `interrupted`, dengan judul "Not ready" dan nada caution, supaya Up yang gagal total tetap menunjukkan alasannya. `lastUp.state` `interrupted` menambah baris "The last Up was interrupted. Bring it up again to continue."

**Menu baris service**: untuk service yang belum dibuat, item "Create with Up…" (membuka sheet plan) menggantikan Show Plan….

**Activity dan Overview** ("Activity" di spec ini adalah halaman Operations [spec 0035](../0035-operations-activity-audit/index.md) yang dibuka lewat `openOperation`, bukan tab Activity per instance): `OperationKind.environment_up` berjudul "Bring up environment {name}" (nama dari `EnvironmentStore` lewat resource operasi; "Bring up environment" kalau tidak dikenal). Operasi dengan `parent` ditampilkan bersarang di bawah induknya dan tidak dihitung sendiri di ringkasan Overview. Label status "Partial" (nada caution) dipakai saat `failure.error.code` `env.up_incomplete` dan `details.ready` di atas 0, dengan catatan "{ready} of {total} services ready".

**Sidebar, daftar, Overview Environments**: saat `activeOperation` terisi, teks status diganti "Bringing up…" (informative, `arrow.triangle.2.circlepath`), atau "Bringing down…" saat `kind` nya `environment_down` (spec 0047); hitungan `ready/total` tetap.

**Onboarding**: `OnboardingCopy.commandExamples` menjadi `stackly instance list`, `stackly env up ~/Code/erp`, `stackly doctor`; detail baris "Environments as files" menjadi "A stackly.yaml per project. Plan, then bring everything up in order."

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| `env.up` | daftar kerja (aksi, instance, runtime, endpoint, urutan) | `EnvPlan` yang dihitung ulang daemon; tidak pernah dari klien |
| `env.up` | `expected_revision` | `EnvPlan.environment.revision` |
| `env.up` | ID environment baru | `EnvironmentId::generate()` di handler |
| needs secret | daftar reference | `PlanItem.secrets` ber `status` `missing` atau `unregistered` |
| `accept` | snapshot, `manifest_sha256` | hasil validate file yang dibaca ulang di langkah itu |
| `accept` | `lock_sha256` | `EnvPlan.lock.sha256` (spec 0045) |
| create | `providerRevision`, `data_location` | `PlanItem.providerRevision` dan `PlanItem.storage.locationId` (spec 0045), lewat `draft_for` |
| `accept` | instance untuk member `shared` | `PlanItem.instance.id` item ber `instanceRef` |
| `accept` | `dependency_conditions` | `ServiceDependency` snapshot, `timeoutSeconds` kosong diganti `DEFAULT_DEPENDENCY_TIMEOUT_SECONDS` |
| `accept`, create | `LeaseId` | `LeaseId::generate()`; `acquired_at` dari clock platform |
| install | artefak | `PlanItem.runtime.artifactSha256` dan `provider` |
| create | `runtime_id` | hasil operasi install (`result.resources`), atau `PlanItem.runtime.runtimeId` |
| create | draft | `environment::plan::draft_for(service, plan item)` |
| start | aksi | lifecycle dan readiness instance dari supervisor saat giliran service tiba |
| start | interaksi Keychain | `SecretCaller` permintaan `env.up` |
| graph | `timeoutSeconds` | edge snapshot, default konstanta |
| commit penutup | laporan per service | phase akhir di driver, `code` dari `FailureError` anak |
| `env.get` | `lastUp` | `UpBoard` selama operasi aktif; kolom `last_up` sesudahnya; `interrupted` kalau kolom `running` tanpa operasi aktif |
| `env.get` | `ServiceInstance.endpoints` | `EndpointStatus` instance dari supervisor |
| `env.get`, `env.list` | `activeOperation` | `OperationEngine::list` dengan `active_only` dan `resource` environment |
| needs secret | reference member dan `instanceRef` | `InstanceBlocker::NeedsSecret` instance yang plan nya `env.instance_blocked` ber `reason` `needs_secret` |
| needs secret | `generateAllowed` | aturan yang sama dengan `resolve_for_start` (`crates/core/src/secret/service/resolve.rs`) |
| `env.up` (replay) | `environmentId`, `name` di balasan | `input` journal operasi |
| start | lifecycle dan readiness | baris instance di registry dan `Supervisor::status(id)` |
| CLI | port `auto` yang dipakai | `ServiceInstance.endpoints[].port` |
| GUI | "{done} of {total}" | `OperationSummary.progress` |

### Key invariants

- Up tidak pernah menghentikan, me restart, mengonfigurasi ulang, atau menghapus instance yang sudah ada.
- Setiap member yang alias nya ada di manifest punya tepat satu lease `active` dari environment nya sejak ia diikat atau sejak `accept` Up berikutnya, sampai Down; member dan lease selalu ditulis dalam satu transaksi, dan tidak ada lease environment tanpa member.
- Operasi pengatur tidak memegang slot berjalan dan tidak memegang lock yang dibutuhkan anaknya (anak mengunci instance, nama instance, runtime, dan artefak; induk mengunci environment dan nama environment).
- Tidak ada service yang `ready` selagi dependency nya tidak `ready` pada saat ia dimulai.
- Reference yang ditulis di `secretRefs` tidak pernah diganti nilai kosong atau nilai yang dibangkitkan.
- Satu satunya penghapusan selama Up adalah kompensasi operasi anak atas resource yang ia buat sendiri.
- Plan yang dijalankan adalah plan yang dihitung daemon dan digest nya sama dengan yang ditinjau.

### Security model

- Peer adalah user yang sama (spec 0006); `instanceRef` selalu di registry user itu (VAL-014).
- Klien hanya mengirim sumber manifest dan digest plan. Draft, runtime, dan endpoint selalu dihitung daemon dari file, jadi klien tidak bisa menyelipkan settings atau binary lewat `env.up`.
- Trust: provider `pending` atau `revoked` sudah menjadi blocker plan, dan operasi anak install serta create tetap lewat gate trust yang sama, jadi approval yang dicabut di antara plan dan Up tetap menolak.
- Secret: nilai tidak pernah ada di params, journal, `last_up`, laporan, event, atau log; yang ada hanya teks reference. Pesan dan detail error anak sudah lewat `Redactor` sebelum disalin ke laporan. Dialog Keychain hanya muncul untuk permintaan dari klien interaktif, pada percobaan pertama.
- Audit: operasi pengatur menulis baris auditnya lewat engine dengan catatan `set_audit_note` berisi nama environment, `manifestSha256`, lokasi dengan `~`, jumlah `ready` dan `total`, dan alias yang tidak ready. Setiap operasi anak menulis baris auditnya sendiri seperti biasa.

### Configuration required

Tidak ada variabel environment atau credential baru.

### Critical test scenarios

- Penolakan sebelum mutasi: manifest tidak valid, plan terblokir, digest berbeda, reference `missing`; pohon file dan generation registry tidak berubah, nol operasi; verifies **AC-1**, **AC-2**.
- Tiga Up bersamaan dengan `max_running` 2, dan Up kelima dengan empat pengatur berjalan; verifies **AC-3**, **AC-24**.
- `accept`: environment baru dan terdaftar; member `shared` dan lease tertulis dalam satu commit; instance yang dihapus di antara plan dan `accept` menggagalkan dengan `env.plan_changed` tanpa tulisan; verifies **AC-4**.
- `instanceRef` ke instance berhenti dan berjalan: satu lease, nol instance baru, config revision dan pohon `config/` identik, Up kedua tanpa lease kedua; verifies **AC-5**.
- Dua service satu artefak menghasilkan satu install; install gagal menutup keduanya di tahap `install`; verifies **AC-6**.
- Create gagal di `init` (provider `fake`): pohon, baris instance, member, dan lease service itu hilang; instance dan runtime bernama mirip milik pengguna utuh; verifies **AC-7**, **AC-12**.
- Graph dengan provider `fake`: rantai tiga, berlian, dependency yang readiness nya gagal (dependent `skipped`, tidak pernah di spawn), `timeoutSeconds` 1 dengan `readyDelayMs` panjang (dependent `skipped` `timeout`, dependency akhirnya `ready`), dan dependency yang crash tepat sebelum dependent mulai; verifies **AC-8**.
- Instance sehat di environment yang Up nya gagal: PID, `restart_total`, dan config revision tetap; verifies **AC-9**.
- Up gagal sebagian: state `failed`, `env.up_incomplete` dengan `items`, `stage`, `last_up`, lalu Up kedua hanya mengerjakan sisanya; verifies **AC-10**.
- Cancel di tiap tahap (install, create, start, menunggu dependency), dengan `last_up` tertulis `cancelled` setiap kali; verifies **AC-11**.
- Klien putus, restart daemon, dan rotasi sesi `new` di tengah dan sesudah Up; lease tetap `active`, tanpa lease `manual` dari start environment (juga untuk instance `running` ber desired `stopped`); create anak yang antre saat daemon berhenti tidak meninggalkan instance tanpa member; verifies **AC-13**, **AC-15**.
- Member ber blocker `needsSecret`: `env.up` membalas `secret.needs_secret` dengan reference nya, bukan `env.plan_blocked`; member `detached` yang `retained` tidak menggagalkan `accept` dan tidak diberi lease; verifies **AC-2**, **AC-4**.
- Gerbang quiesce aktif di tengah Up: tidak ada anak baru, sisa service `cancelled`; start anak yang antre lebih dari `CHILD_QUEUE_TIMEOUT_SECONDS` dibatalkan dengan `resource.busy`; verifies **AC-8**, **AC-24**.
- Dua environment meminjam satu instance serentak; dua manifest membuat nama instance yang sama serentak; verifies **AC-14**.
- `env.get` selama dan sesudah Up, setiap baris *Alasan belum ready*, `endpoints` dengan port `auto` yang di bind; verifies **AC-16**.
- CLI: golden manusia dan `--json` untuk `up` (sukses, needs secret, terblokir, incomplete) dan `status`; binary test membandingkan `result` dengan balasan daemon; argumen posisi folder; verifies **AC-17**, **AC-18**.
- Snapshot GUI terang, gelap, dan sempit: sheet plan dengan Bring Up aktif dan nonaktif, halaman selama Up, banner dengan tiap alasan, Activity dengan anak bersarang dan label Partial; verifies **AC-19** sampai **AC-22**.
- Kontrak: fixture terbaca Rust dan Swift, `cargo xtask codegen --check`, klien lama membaca `NotReadyReason` baru sebagai `unrecognized`; verifies **AC-23**.
- Native: Up manifest contoh ERP dari cache xtask; verifies **AC-25**.

### Titik sambung untuk spec 0047

**Bentuk lease dan pemegangnya**

- Lease environment: baris `instance_demands` ber `source_kind` `environment`, `source_id` `EnvironmentId`, `session_id` null, `state` `active`; di domain `LeaseHolder::Environment(id)`. Paling banyak satu `active` per pasangan instance dan environment (unique index spec 0005). Pemegangnya identitas environment, tidak pernah klien, sesi, atau operasi.
- Sesudah Up melewati langkah `accept` dan tahap `create`, setiap member yang alias nya ada di snapshot punya tepat satu lease `active`, dan tidak ada lease environment tanpa member. Member `detached` memegang lease hanya kalau lease itu sudah ada dari Up sebelumnya; Up tidak pernah memberinya lease baru. Jadi Down sebaiknya melepas semua lease `active` ber `source_id` environment itu (bukan berjalan menurut snapshot), supaya member `detached` ikut terlepas.
- Up tidak pernah melepas lease. Yang melepas hanya Down. Kompensasi create menghapus baris lease instance yang ia hapus.

**Yang perlu Down baca**

- `Registry::leases(instance, LeaseFilter::Unreleased)` untuk setiap member, lalu `evaluate_stop(StopRequest::EnvironmentDown(id))` spec 0004 untuk memutuskan stop.
- `environment_instances.ownership` dan `dependency_conditions`: edge yang dipakai Up terakhir, untuk urutan berhenti terbalik. Member tanpa edge (misalnya `detached`) tidak punya urutan.
- `environments.last_operation_id` dan `last_up`: Down boleh menulis `last_operation_id`; kalau Down mengosongkan atau mengganti `last_up`, itu keputusan spec 0047. Diputuskan spec 0047 (AC-4, K8): commit `release` Down mengosongkan `last_up` dan menulis `last_operation_id`. Alasan `dependency_not_ready` dan `up_failed` hanya ditampilkan untuk service yang sekarang diam, jadi laporan lama tidak mengganggu sesudah Down.
- Start environment tidak menulis lease `manual` (`StartTrigger::Environment`). Jadi instance yang hanya dinyalakan Up tidak punya demand lain, dan `evaluate_stop` boleh menghentikannya; instance yang juga kamu start sendiri punya lease `manual` dan tetap jalan.

**Serialisasi**

- Up memegang `LockKey::Environment(id)` dan `LockKey::EnvironmentName(name)` selama berjalan dan menolak Up kedua dengan `BusyPolicy::Reject`. Down untuk environment yang sama memakai kunci yang sama; apakah ia antre atau membatalkan Up yang berjalan diputuskan spec 0047. Diputuskan spec 0047 (AC-3, K2): Down antre dengan `BusyPolicy::Queue` dan meminta cancel setiap Up yang belum berakhir pada environment itu; Up yang datang selagi Down berjalan atau antre ditolak `resource.busy` oleh `BusyPolicy::Reject` Up sendiri.
- Up mencatat lease lebih dulu (commit), baru membaca lifecycle instance, dan instance `stopping` dinyalakan lagi lewat `start_instance` yang antre di lock instance. Supaya AC-073 terpenuhi dari kedua sisi, Down perlu memutuskan stop dan melepas lease nya dalam satu `commit_guarded` yang membaca lease di dalam guard, lalu menghentikan instance lewat operasi anak `stop_instance` yang mengunci instance itu.
- Down bisa memakai mesin yang sama: `OperationKind::is_coordinator()`, `Origin::Child`, `wait_final`, `UpBoard` atau padanannya, dan port sejenis `UpChildren`. `recover` hanya menutup anak antre berjenis create dan start; kalau Down memulai anak `stop_instance`, aturan untuk jenis itu diputuskan spec 0047.

**UI dan kontrak**

- `EnvironmentSummary.activeOperation` sudah membawa `kind`, jadi Down cukup mengisi `environment_down`. `UpState` dan `UpPhase` enum terbuka.
- Tempat tombol Down di toolbar ada di kiri Up…. Up… nonaktif selama operasi environment apa pun berjalan.
- `env.remove` (spec 0044) tetap menolak selama ada lease `active`, jadi environment yang pernah Up harus Down dulu.

### Jawaban Follow-up spec lain

| Asal | Pertanyaan | Jawaban |
| --- | --- | --- |
| Spec 0007 L61, L842 | operasi anak untuk environment; tidak menunggu sambil memegang slot | operasi pengatur dengan kelas slot sendiri, `Origin::Child`, dan `wait_final` (AC-3, AC-24) |
| Spec 0007 L441 | `environment_up` resumable | diubah menjadi tidak resumable, `CloseInterrupted` (AC-15, K4 di rationale); baris `environment_down` milik spec 0047 |
| Spec 0004 L151 | `EnvironmentName` sebagai input create environment | environment dibuat lewat `environment::register` spec 0044, juga dari langkah `accept` Up untuk manifest yang belum terdaftar (AC-4) |
| Spec 0005 L265 | `environment_instances.dependency_conditions` | edge efektif Up terakhir per member (*Model data*) |
| Spec 0016 L458 | `stackly env up ~/Code/erp` di `commandExamples` | AC-22; argumen posisi folder di AC-17 |
| Spec 0031 L71, L90, L100 | sumber lease `environment`, tanpa sesi, lintas sesi | *Lease*, AC-13 |
| Spec 0031 L470 | aturan lease environment di rotasi sesi | tidak berubah; akibatnya dijelaskan di *Lease*. Override `stop_all` dan `agent_off` milik spec 0047 |
| Spec 0022 L176 | label "Bring up environment" | AC-22, *Layar* |
| Spec 0044, titik sambung | `expectedPlanDigest`, penulisan member dan snapshot, default `timeoutSeconds`, nasib `detached`, enum terbuka, tombol Up… | AC-1, AC-4, AC-8 (600 detik), member `detached` tidak disentuh dan tetap memegang lease (K10), `UpPhase` dan `NotReadyReason` baru, AC-19 |
| Spec 0011 L204, L372 | `Bind` untuk env up, `secret.needs_secret` di env up | create anak memakai `Bind` dengan field (AC-7); AC-2 |
| Spec 0012 L428 | `PartialNote` untuk readiness environment | banner memakai `PartialItem` dengan alasan Up (AC-20) |

## Build plan

Urutan mengikuti pendekatan Journey: jalur "tinjau plan, Bring Up, lihat progress, baca hasil dan alasannya" dibangun lengkap dari engine sampai setiap keadaan gagalnya, lalu CLI, lalu GUI. Protokol naik satu minor di langkah 5 (nomor bebas berikutnya, ditetapkan lead saat build; begitu juga nomor migrasi). Semua langkah butuh spec 0044 milestone 1 sudah di `main`.

**Milestone 1: fondasi operasi anak** (needs spec 0044 milestone 1; bisa berjalan paralel dengan milestone CLI dan GUI spec 0044)

1. Engine: kelas slot pengatur (`OperationKind::is_coordinator()`, kelas di entri antrean, `MAX_RUNNING_COORDINATORS`, stats), `Origin::Child` dengan gerbang quiesce, kolom dan field `parent`, `OperationEngine::wait_final` yang dibangunkan perubahan operasi, penutupan anak create dan start yang antre di `recover`, kebijakan `EnvironmentUp` (tidak resumable, `CloseInterrupted`) dan baris tabel resume nya. Memenuhi **AC-3**, **AC-24**, **AC-15**.
2. Domain dan storage: migrasi `environments.last_up` dan `operation_journal.parent_id`; `MemberDependency`, `UpReport`, `UpPhase`, mutator `Environment`; baca dan tulis `dependency_conditions`, `last_operation_id`, `last_up`; perluasan `DeleteInstanceCreatedBy`; fake registry testkit dan `registry_conformance`. Memenuhi **AC-4**, **AC-7**, **AC-13**.
3. Create dan start: `EnvironmentBinding` di `CreateInstanceWork`, di `input` journal create dan `CreateWorkFactory::rebuild`, dan di batch `register` nya; kompensasi yang ikut menghapus member dan lease; `StartTrigger::Environment`, `StartRequest::environment`, dan pemicu sebagai parameter `start_precheck`; golden create dan start yang ada tidak berubah. Memenuhi **AC-7**, **AC-12**, **AC-13**.

**Milestone 2: Up di daemon** (after langkah 1 sampai 3)

4. Core `environment::up`: `precheck`, guard `accept`, `graph` murni dengan unit test (rantai, berlian, gagal, timeout, batal), `driver` dengan commit penutup, port `UpChildren` dengan fake di testkit, `UpBoard`, `report`, `limits`, dan fixture manifest graph ber provider `fake` di `crates/core/tests/fixtures/environments/`. Memenuhi **AC-1**, **AC-2**, **AC-4**, **AC-6**, **AC-8**, **AC-9**, **AC-10**, **AC-11**.
5. Protokol: method `env.up`, `EnvUpParams`, `EnvUpReply`, tambahan DTO `env` dan `operation`, lima kode area `env` dengan hint, fixture `protocols/fixtures/env/`, kenaikan minor, `cargo xtask codegen`, `cargo xtask fuzz seed`. Memenuhi **AC-23**, **AC-16**.
6. `stacklyd`: handler `env.up`, pemecahan handler install, create, dan start menjadi bagian yang bisa dipanggil `UpChildren`, adapter `ipc/env/children.rs`, `UpBoard` di `DaemonCore`, tambahan `env.get` dan `env.list` (`activeOperation`, `lastUp`, `endpoints`, *Alasan belum ready*), `set_audit_note`, dan test integrasi `apps/daemon/tests/env_up.rs` dengan provider `fake` (penolakan tanpa mutasi, `instanceRef`, gagal sebagian, cancel, serentak, klien putus, restart daemon, rotasi sesi). Memenuhi **AC-1** sampai **AC-16**.

**Milestone 3: `stackly env up` dan `status`** (after langkah 5 untuk golden; binary test needs langkah 6; bisa paralel dengan milestone 4)

7. CLI: resolver target dengan aturan path, `env up` dengan progress dan tabel akhir, `env status`, golden dan binary test. Memenuhi **AC-17**, **AC-18**.

**Milestone 4: Up di app** (after langkah 5; needs spec 0044 milestone 3 untuk halaman dan sheet; evidence manual needs langkah 6)

8. Sheet dan toolbar: Up…, Bring Up, `env.plan_changed`, Set… dan `SecretEntrySheet`, "Create with Up…". Memenuhi **AC-19**, **AC-21**.
9. Progress dan alasan: banner "Bringing up…", `StatusAppearance.upPhase` dan tabelnya di `design.md`, alasan banner partially ready dengan aksinya, "Bringing up…" di sidebar, daftar, dan Overview, skenario fixture `environment-bringing-up` dan `environment-up-incomplete`. Memenuhi **AC-20**, **AC-22**, **AC-23**.
10. Activity dan onboarding: judul operasi, anak bersarang lewat `parent`, label Partial, `commandExamples`, kalimat "Environments as files"; snapshot terang, gelap, dan sempit untuk semua layar baru. Memenuhi **AC-22**.
11. Evidence dan manual: test native `env_up_native.rs` dari cache xtask (BLOCKED tanpa artefak), record evidence fixture untuk AC-8, AC-10, dan AC-14; langkah manual (GUI nyata dan Keychain login, hanya dijalankan kamu) ditulis di `verify.md` bagian `### UI / manual`: Bring Up manifest contoh ERP dari sheet plan, lihat progress dan port, kosongkan satu reference lalu isi lewat `SecretEntrySheet`, gagalkan satu service lalu baca banner dan Bring Up lagi, tutup app di tengah Up lalu buka lagi. Memenuhi **AC-25**, **AC-20**, **AC-21**.

**Sesi paralel yang aman**: sesudah langkah 5 masuk, CLI (langkah 7, `apps/cli/`) dan GUI (langkah 8 sampai 10, `apps/macos/`) tidak beririsan file dengan daemon (langkah 6, `apps/daemon/`). `crates/protocol/`, `protocols/`, hasil codegen, dan migrasi hanya disentuh langkah 1, 2, dan 5, oleh satu sesi, dan tidak bersamaan dengan langkah 1 sampai 4 spec 0044 atau langkah spec 0045 yang menyentuh file yang sama.

## Consequences

**Positive**:

- Up tidak membawa aturan install, create, atau start kedua. Trust, cek port, cek data, readiness, dan kompensasi instance yang dibuat lewat environment sama persis dengan yang dibuat lewat tombol biasa.
- Kegagalan satu service tidak bisa merusak service lain: setiap operasi anak hanya membersihkan miliknya sendiri, dan induk tidak punya apa pun untuk dibersihkan.
- Lock instance hanya dipegang selama operasi anak instance itu berjalan, jadi kamu tetap bisa bekerja dengan instance lain selagi runtime besar diunduh.
- Tidak ada deadlock di batas 8 operasi: induk tidak memegang slot dan tidak memegang lock anaknya.
- Up boleh diulang kapan saja. Mengulang adalah satu satunya langkah recovery yang perlu diingat.

**Negative / tradeoffs**:

- Engine yang sudah stabil mendapat tiga konsep baru (kelas pengatur, `Origin::Child`, `wait_final`) dan satu aturan `recover` baru, dan handler install, create, dan start harus dipecah supaya bisa dipanggil dari dalam daemon. Review langkah 1 dan 6 perlu teliti.
- `planDigest` spec 0044 tidak lagi memuat aksi turunan lifecycle instance yang sudah ada (update spec 0044 tanggal 2026-11-13, dari usulan spec ini), jadi instance yang berhenti atau menyala di antara plan dan Bring Up tidak menghasilkan `env.plan_changed`. Perubahan lain pada plan tetap menghasilkannya, dan kamu meninjau plan lagi.
- Progress per service di layar bergantung pada event operasi anak, jadi phase `waiting` baru terlihat pada muat ulang berikutnya, bukan seketika.
- Satu Up menghasilkan banyak baris operasi dan audit (satu induk ditambah sampai tiga per service). Activity harus menyarangkannya supaya tetap terbaca.
- Up yang terputus restart daemon tidak dilanjutkan sendiri. Kamu perlu menjalankannya lagi; halaman menunjukkannya sebagai `interrupted`.
- Sesudah login ulang, instance environment menyala lagi lewat antrean start tanpa urutan dependency. Service yang benar benar butuh urutan perlu Up lagi atau tahan terhadap dependency yang belum siap.
- Lease dan member tetap ada sesudah Up yang gagal, jadi `env.remove` ditolak sampai Down (spec 0047) dijalankan, juga untuk environment yang tidak pernah berhasil Up penuh.
- `create_instance` mendapat masukan ikatan dan kompensasinya menghapus member serta lease. Itu menyentuh jalur create yang stabil; golden nya menjaga perilaku tanpa ikatan.
- Default timeout dependency 600 detik bisa terasa lama untuk dependency yang macet; manifest bisa menulis `timeoutSeconds` yang lebih pendek.

**Neutral**:

- Up yang tidak lengkap berakhir `failed`, bukan `succeeded` dengan catatan; label "Partial" di Activity adalah tampilan untuk kode `env.up_incomplete`.
- `env.up` selalu membuat operasi, juga saat semua service sudah ready (ia tetap mencatat snapshot dan lease).
- Member `detached` tidak disentuh Up dan tetap memegang lease sampai Down.
- Baris `environment_down` di tabel resume spec 0007 tidak diubah di sini.

### Update spec lain

Usulan untuk lead (architect spec ini tidak mengubah spec lain). Semuanya diterapkan 2026-11-13 lewat `architect (update)` (spec 0007 dan dua usulan perilaku spec 0044 lebih dulu, sisanya di sesi gabungan `arc-update-all`), kecuali tabel di `apps/macos/design.md`, yang ditulis langkah build 9:

- **Spec 0007**: kelas slot pengatur (`OperationKind::is_coordinator()`) dan `MAX_RUNNING_COORDINATORS` di AC-5 dan *Batas*; `Origin::Child` dan `parent`; `wait_final` di API engine; `recover` menutup anak create dan start yang antre dan induknya `interrupted`; baris tabel resume `environment_up` menjadi "tidak, kebijakan `CloseInterrupted`"; centang Follow-up L842.
- **Spec 0004 dan 0005**: `MemberDependency`, `UpReport`, `UpPhase`, mutator `Environment`, kolom `last_up` dan `parent_id`, isi `dependency_conditions`, perluasan `DeleteInstanceCreatedBy`.
- **Spec 0010 dan 0031**: `StartTrigger::Environment` (seperti `User` di setiap cabang, tanpa lease `manual`, juga di `start_precheck`); baris sumber `environment` di *Model demand* spec 0031 menunjuk spec ini; centang Follow-up L470 untuk bagian rotasi.
- **Spec 0020**: `EnvironmentBinding` sebagai masukan internal `CreateInstanceWork`, isinya di `input` journal, dan isi tambahan batch `register` serta kompensasinya.
- **Spec 0018 dan 0021**: handler `runtime.install` dan `instance.start` dipecah menjadi bagian yang juga dipanggil `UpChildren`, tanpa perubahan perilaku.
- **Spec 0044** (dua usulan perilaku, perlu architect spec 0044): (a) keluarkan `action` dan `steps` instance yang sudah ada dari masukan `planDigest`, karena keduanya turunan lifecycle dan Up menurunkannya lagi saat giliran service tiba; tanpa itu, instance yang berhenti atau menyala di antara plan dan Bring Up menghasilkan `env.plan_changed` yang tidak perlu; (b) jadikan member atau `instanceRef` yang belum initialized (`uninitialized`, atau `stopped` dan `failed` tanpa data) blocker `env.instance_blocked`, karena `start_instance` selalu menolaknya dengan `supervisor.not_initialized`. Sampai (b) diterapkan, Up melaporkan service itu `failed` di tahap `start` dengan kode supervisor nya.
- **Spec 0044** (tambahan kontrak dan layar): Up… menggantikan Plan…, "Create with Up…" menggantikan Show Plan…, catatan kaki sheet, field baru `EnvironmentSummary`, `EnvironmentDetail`, `ServiceInstance`, `ServiceNotReady`, nilai baru `NotReadyReason`; resolver target ber path juga dipakai `stackly env plan`.
- **Spec 0008, 0016, 0022, 0035**: perintah `env up` dan `env status` di tabel perintah; `commandExamples`; judul operasi dan sarang `parent` di Activity; label Partial.
- **`apps/macos/design.md`**: tabel `StatusAppearance.upPhase`.

## Follow-up

- [ ] Lead: tetapkan nomor minor protokol dan nomor migrasi saat build, dan terapkan *Update spec lain* lewat `architect (update)`.
- [x] Spec 0044: dua usulan perilaku di *Update spec lain* (masukan `planDigest` tanpa aksi turunan lifecycle; instance belum initialized sebagai blocker plan). Diterapkan di spec 0044 (2026-11-13), *Aksi plan*.
- [x] Spec 0045: sesudah di merge, cocokkan nama field di *Kontrak `--locked`* (`PlanInputs.locked`, `EnvPlan.lock.sha256`, `PlanItem.providerRevision`, `PlanItem.storage`, `CreateDraft.data_location`) dengan teks final spec 0045. Dicocokkan dengan spec 0045 di `main` (2026-11-13): nama nama itu sama, dan butir spec 0045 untuk spec ini (`locked` diteruskan ke plan, cek `lock.locked` di `stackly env up --locked`, `lock_sha256` dari `EnvPlan.lock.sha256`, `storage.locationId` ke `CreateDraft.data_location`, cek `providerRevision` di create anak) sudah ada di *Kontrak `--locked`* butir 2, 6, 7, dan 8.
- [x] Spec 0047: `environment_down` di atas *Titik sambung untuk spec 0047*; baris tabel resume `environment_down`; nasib `last_up` sesudah Down; override stop pada instance ber lease. Dijawab spec 0047 (2026-11-13): pengatur tidak resumable, `last_up` dikosongkan commit `release`, override di AC-13 sampai AC-17; diselaraskan di spec ini pada tanggal yang sama (`ChildOrigin`, `BusyPolicy` anak, `activeOperation.kind`, *Titik sambung*).
- [ ] Antrean start saat login tidak mengikuti urutan dependency environment. Kalau pengguna membutuhkannya, itu fitur baru (Up latar belakang saat login), bukan bagian fitur ini.
- [ ] Hapus instance yang masih member environment ditahan FK `restrict` spec 0005; pesan dan alurnya milik fitur hapus instance.
- [ ] `/sync` setelah build: catat di root `AGENTS.md` bahwa Up adalah operasi pengatur yang memulai operasi anak dan tidak pernah memanggil `ProcessSupervision`, bahwa lease environment ditulis bersama member, dan perintah `stackly env up` dan `status`.

## Rationale

Alasan dan opsi yang ditimbang: lihat [rationale.md](rationale.md).
