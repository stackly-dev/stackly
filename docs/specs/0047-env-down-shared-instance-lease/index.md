# 0047. Env down, shared instance, dan lease

**Date**: 2026-11-13
**Status**: Proposed

## Summary

Spec ini menetapkan Down: satu perintah yang melepas lease sebuah environment (tanda bahwa environment itu butuh sebuah instance) lalu menghentikan hanya instance yang tidak dibutuhkan siapa pun lagi. Instance yang masih dipakai environment lain, yang kamu nyalakan sendiri, atau yang menyala saat login tetap jalan, dan konfirmasi Down menyebutnya satu per satu sebelum apa pun terjadi. Down tidak pernah menghapus data, runtime, instance, atau keanggotaan. Spec ini juga menutup celah di sisi lain: stop manual, Stop All Services, Turn Off, dan ganti port pada instance yang sedang dipegang lease environment selalu ditolak dulu atau meminta override, dengan nama environment yang terdampak. Down adalah operasi pengatur seperti Up (spec 0046), jadi stop tiap instance tetap lewat jalur stop yang sudah ada.

## Requirements

**User stories**:

- Sebagai developer yang selesai bekerja di satu proyek, saya ingin satu perintah yang mematikan service proyek itu tanpa mematikan instance yang masih dipakai proyek lain.
- Sebagai pengguna yang menyalakan sebuah instance sendiri sebelum Up, saya ingin instance itu tetap jalan sesudah Down, dan saya ingin tahu itu sebelum menekan tombol.
- Sebagai pengguna yang menekan Stop pada instance yang dipegang environment, saya ingin diberi tahu environment mana yang terganggu, lalu memilih sendiri.
- Sebagai pengguna menu bar, saya ingin Stop All Services menunjukkan environment mana yang ikut terdampak.
- Sebagai penulis script, saya ingin `stackly env down --wait` keluar bukan 0 kalau ada instance yang seharusnya berhenti tetapi tidak berhenti, dan saya ingin yakin perintah itu tidak pernah menghapus data.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-022):

- **AC-1** (preview Down, baca saja): `env.preview_down { id }` membalas `EnvDownPreview` dari fungsi murni `stackly_core::environment::down::select` (*Pilihan Down*): `stop` (instance yang akan dihentikan), `keep` (instance yang tetap jalan beserta alasannya: lease environment lain dengan namanya, lease `manual`, lease `startup`), `idle` (sudah berhenti), `blocked` (tidak bisa dihentikan, dengan kode), `leaseCount`, dan `cancelsUp`. Ia bukan operasi dan tidak menulis apa pun. `env.preview_down`, precheck `env.down`, dan pemeriksaan ulang per instance memakai fungsi itu saja; tidak ada pemanggil yang memilih dengan aturan sendiri.
- **AC-2** (pintu masuk Down, REQ-022): `env.down { id, expectedStopInstanceIds, idempotencyKey }` menjalankan *Precheck Down*: replay, cek environment ada (`env.not_found`), hitung `select`, lalu menolak `env.down_changed` tanpa operasi dan tanpa mutasi kalau himpunan `stop` saat ini berbeda dari `expectedStopInstanceIds`. Down tidak pernah membaca file manifest, lockfile, catalog, atau jaringan, jadi ia bekerja juga saat `manifestState` `missing` atau `invalid` dan saat offline. Environment tanpa lease dan tanpa instance yang jalan tetap diterima dan selesai `succeeded` tanpa menghentikan apa pun.
- **AC-3** (operasi pengatur dan Up yang sedang berjalan): Down berjalan sebagai satu operasi `environment_down` dengan resource `Environment(id)`, lock tambahan `LockKey::EnvironmentName(name)`, kelas slot pengatur spec 0046 (`OperationKind::is_coordinator()` benar), dan `BusyPolicy::Queue`. Tepat sesudah `begin`, handler meminta `operation.cancel` untuk setiap operasi `environment_up` yang belum berakhir pada environment itu, dengan klien permintaan Down sebagai pemicu cancel; Down lalu menunggu lock sampai Up itu selesai menutup anaknya dan menulis `last_up`. Operasi lain pada environment itu (import, remove, Down lain) ditunggu, tidak dibatalkan. Up yang datang selagi Down berjalan atau antre ditolak `resource.busy` oleh aturan Up sendiri. Down tidak pernah memanggil `ProcessSupervision`, downloader, atau menulis file.
- **AC-4** (langkah `release`, PRD AC-022, PRD bagian 13.2): langkah pertama operasi, dalam satu `commit_guarded`: melepas (`released`) setiap lease `active` ber `source_kind` `environment` dan `source_id` environment itu, termasuk milik member `detached`; mengosongkan `last_up`; dan menulis `last_operation_id`. Sesudah commit itu environment tidak memegang lease `active` satu pun, apa pun yang terjadi sesudahnya. Lease environment lain, lease `manual`, lease `startup`, baris `environment_instances`, dan baris `environments` tidak disentuh. Guard nya membaca environment di dalam transaksi; kalau environment sudah tidak ada, operasi gagal `env.not_found` tanpa menulis.
- **AC-5** (hanya menghentikan yang tidak dibutuhkan, BR-014, REQ-022): untuk setiap instance di himpunan `stop` yang dicatat di journal, operasi memulai satu operasi anak `stop_instance` dengan pemicu `environment_down`. Di bawah lock instance itu, commit yang menulis desired `stopped` adalah commit ber guard yang membaca lease instance di dalam transaksi dan menjalankan `evaluate_stop(StopRequest::EnvironmentDown(id))` (*Stop dengan guard lease*): kalau masih ada lease `active` dari pemegang mana pun, tidak ada yang ditulis, respawn yang sedang dijadwalkan tidak dibatalkan, dan operasi anak itu selesai `succeeded` dengan hasil `kept` yang eksplisit; kalau tidak ada, instance berhenti lewat alur *Stop* spec 0010 (stop normal, tidak pernah `SIGKILL`). Instance yang tidak ada di himpunan `stop` tidak pernah dihentikan Down, walau lease nya ikut dilepas. Test PRD AC-022: dua environment meminjam satu instance, Down environment pertama, lalu lease environment kedua masih `active`, PID instance sama, dan readiness nya `ready`.
- **AC-6** (urutan berhenti): instance berhenti dalam urutan dependency terbalik menurut `environment_instances.dependency_conditions`: sebuah instance baru mulai dihentikan sesudah setiap member di himpunan `stop` yang bergantung padanya berakhir (apa pun hasilnya). Member tanpa edge (misalnya `detached`) tidak menunggu siapa pun. Paling banyak `DOWN_STOP_CONCURRENCY` 4 stop berjalan sekaligus, dan seri diputus alias lalu ID. Dependent yang tetap jalan (`kept`) tidak menahan dependency nya.
- **AC-7** (tidak pernah menghapus atau mengubah, BR-006, VAL-014): Down tidak pernah menghapus atau mengubah folder data, config, log, runtime, artefak cache, secret dan nilai Keychain, baris instance selain desired dan lifecycle lewat stop, baris member, baris environment selain `last_up`, `last_operation_id`, dan revision, `startup`, port, atau config revision. Instance yang dipinjam lewat `instanceRef` identik sebelum dan sesudah Down selain lease environment itu. Test membandingkan pohon file test root (di luar log dan `run/`), jumlah baris `instances`, `runtimes`, `environment_instances`, dan `environments`, serta config revision tiap instance sebelum dan sesudah Down, termasuk Down yang gagal dan yang dibatalkan.
- **AC-8** (hasil, sapuan akhir, dan gagal sebagian): setiap instance di himpunan `stop` berakhir `stopped`, `already_stopped`, `kept`, `failed` (dengan kode stop, misalnya `supervisor.stop_timeout`), atau `cancelled`. Sesudah semua operasi anak berakhir, Down menjalankan `select` sekali lagi atas keadaan segar (*Sapuan akhir*): instance yang kini masuk `stop` tetapi tidak ada di himpunan yang dikonfirmasi tidak dihentikan dan dilaporkan `left_running` dengan kode `env.down_changed`. Down berakhir `succeeded` kalau tidak ada yang `failed`, `cancelled`, atau `left_running`; `result.resources` berisi instance `stopped`, dan `result.partial` satu item ber outcome `skipped` dan kode `env.instance_in_use` per instance `kept`. Selain itu ia berakhir `failed` dengan error `env.down_incomplete` (`stopped`, `total`, dan satu `items` ber `kind` `instance` per instance yang tidak berhenti: `instanceId`, `name`, `outcome`, `code`, `operationId`) dan hint "bring the environment down again, or stop the instances listed; nothing was deleted". Lease tetap terlepas di setiap hasil. Down berikutnya hanya mengerjakan sisanya.
- **AC-9** (batal): `operation.cancel` pada Down yang masih antre mengakhirinya `cancelled` tanpa melepas lease apa pun; Up yang sudah dibatalkan Down itu tetap batal. Sesudah langkah `release`, cancel berhenti memulai operasi anak baru, menunggu stop yang sedang berjalan sampai selesai (stop yang berjalan tidak dibatalkan), menandai sisanya `cancelled`, melewati sapuan akhir, lalu berakhir `cancelled`. Lease yang sudah dilepas tetap terlepas.
- **AC-10** (daemon berhenti di tengah Down): `environment_down` tidak resumable dengan kebijakan `CloseInterrupted`. Sesudah daemon start lagi operasi itu ditutup `failed` alasan `interrupted`; operasi anak `stop_instance` yang masih `queued` dan induknya ditutup `interrupted` ikut ditutup `failed` alasan `interrupted` oleh `recover`, jadi tidak ada stop yang menyala sendiri belakangan. Lease sudah terlepas, jadi instance yang belum sempat dihentikan masih punya desired `running` tanpa lease: di sesi login yang sama daemon yang start lagi mengadopsi atau menyalakannya lagi seperti instance lain, dan di sesi login baru rotasi spec 0031 memberinya desired `stopped`. Down yang dijalankan lagi menemukan member yang masih hidup tanpa lease lewat `select` dan menghentikannya.
- **AC-11** (serentak, PRD AC-073 bagian Down): Up environment A dan Down environment B yang bersamaan pada instance bersama X selalu berakhir dengan tepat satu lease `active` milik A, lease B `released`, dan tepat satu instance X. Kalau commit lease A lebih dulu dari guard stop, X tidak pernah berhenti (PID sama). Kalau guard stop lebih dulu, Up A membaca `stopping`, menunggu, lalu menyalakan X lagi lewat `start_instance` (spec 0046 AC-14), sehingga X `running` dan `ready` saat Up A selesai; kalau stop itu tidak selesai dalam batas tunggu spec 0046 (`SERVICE_SETTLE_TIMEOUT_SECONDS`), Up A melaporkan service itu `failed` dengan `env.service_not_ready`, lease A tetap `active`, dan Up A berikutnya menyalakannya. Dua Down dari dua environment pada X menghasilkan paling banyak satu stop, dan kalau keduanya saling melihat lease yang lain saat preview sehingga tidak ada yang menghentikan X, paling sedikit satu dari keduanya berakhir `env.down_incomplete` dengan X `left_running` (AC-8), tidak pernah `succeeded` diam diam. Up dan Down pada environment yang sama tidak pernah berjalan bersamaan (AC-3). Test menjalankan pasangan itu 50 kali dengan urutan acak dan memeriksa invariant di atas setiap kali, tanpa baris instance, member, atau lease ganda.
- **AC-12** (baca lease, PRD bagian 13.2): `EnvironmentDetail` mendapat `lease: EnvLeaseInfo { activeCount, heldSince? }` dari lease `active` environment itu di registry, dan `InstanceEnvironment` (spec 0044) mendapat `leaseActive: bool`. Keduanya dibaca dari registry, tidak dari koneksi klien: menutup CLI atau GUI, memutus socket di tengah Down, atau me restart daemon tidak mengubahnya, dan Down yang kliennya putus tetap berjalan sampai selesai. Test memutus klien tepat sesudah `env.down` diterima dan memeriksa operasi selesai dan lease `released`.
- **AC-13** (stop manual, BR-015): `instance.stop` tanpa `overrideLeases` pada instance yang hidup (`starting`, `running`, `recovering`, `stopping`) dan punya lease environment `active` ditolak di precheck dengan `supervisor.stop_requires_override` tanpa operasi, dengan satu `items` ber `kind` `environment` per environment pemegang (`environmentId`, `name`, `alias`, `ownership`). Dengan `overrideLeases` true instance berhenti, lease environment tetap `active`, desired menjadi `stopped`, dan catatan audit stop memuat nama environment yang di override. Pemeriksaan yang sama diulang di guard commit yang menulis desired `stopped` (mekanisme AC-5): lease yang dicatat Up sesudah precheck tetapi sebelum commit itu membuat operasi `stop_instance` berakhir `failed` dengan kode dan `items` yang sama, tanpa menghentikan apa pun. Stop pada instance yang sudah diam (`stopped`, `failed`) tetap seperti sekarang: ia hanya menulis desired `stopped` dan tidak meminta override, karena tidak ada yang terganggu. Instance yang di override tetap berhenti sesudah logout dan login lagi (aturan rotasi spec 0031 tidak berubah), dan menyala lagi lewat Up atau start manual.
- **AC-14** (yang tidak butuh override): `instance.restart`, `instance.reload`, `instance.config.apply`, `instance.rename`, dan `instance.force_stop` pada instance ber lease environment tidak ditolak dan tidak menerima field override, karena instance nya kembali jalan dengan identitas dan port yang sama (atau, untuk force stop, stop nya sudah diputuskan lebih dulu). Lease tidak berubah oleh operasi itu.
- **AC-15** (ganti endpoint): `instance.change_endpoints` mendapat `overrideLeases` (default false). Tanpa nya, permintaan yang benar benar mengubah port pada instance ber lease environment `active` ditolak `instance.endpoints_require_override` tanpa operasi, dengan `items` environment seperti AC-13. Permintaan `dryRun` tidak ditolak dan membalas `EndpointValidation.leases` berisi environment pemegang, supaya dampaknya tampil sebelum Apply. Dengan `overrideLeases` true perubahan berjalan seperti spec 0029, lease tetap `active`, dan catatan audit memuat nama environment. Pemeriksaan lease diulang di dalam `commit_guarded` operasi itu.
- **AC-16** (`instance.stop_all` dan preview nya): `instance.preview_stop_all` dan `instance.stop_all` mendapat `overrideLeases` (default false). Dengan false, aturan 6 *Pilihan stop all* spec 0031 tetap: instance ber lease environment masuk `skipped` dengan `supervisor.stop_requires_override`. Dengan true, instance itu masuk `stop`, dan langkahnya memakai stop manual dengan override (AC-13). Langkah stop operasi `stop_all_instances` memakai guard yang sama dengan nilai override itu, jadi dengan false lease yang muncul sesudah pilihan membuat instance itu `skipped` dengan `supervisor.stop_requires_override`, bukan berhenti. `StopAllItem` mendapat `environments: [InstanceEnvironment]` (environment yang lease nya `active`), `StopAllSkip` mendapat field yang sama, dan `StopAllPreview` mendapat `affectedEnvironments`. Aturan `expectedInstanceIds` spec 0032 tidak berubah: daemon menolak `instance.stop_all_changed` kecuali ia akan menghentikan persis himpunan yang ditampilkan.
- **AC-17** (`agent_off`): `daemon.shutdown { mode: agent_off }` mendapat `overrideLeases` (default false) yang diteruskan ke `stop_all::select` dan ke langkah stop nya. Dengan true, instance ber lease environment ikut dihentikan sehingga daemon bisa keluar; lease tetap `active`. Dengan false perilaku spec 0031 tetap (instance itu `skipped` dan daemon tidak keluar).
- **AC-18** (`stackly env down`, REQ-028): `stackly env down [--file <path> | <environment> | <path>] [AsyncArgs]` me resolve target dengan aturan spec 0046, memanggil `env.preview_down`, mencetak apa yang akan berhenti dan apa yang tetap jalan ke stderr, lalu `env.down` dengan ID dari preview itu, tanpa konfirmasi. Tanpa `--wait` ia mencetak operasi yang dimulai dan keluar 0. Dengan `--wait` ia mencetak satu baris per instance yang berakhir, lalu ringkasan, dan keluar 0 hanya kalau operasi `succeeded`; `env.down_incomplete` keluar 7. `env.down_changed` diulang sekali dengan preview baru, lalu keluar 3. Environment yang tidak dikenal keluar 10 (`cli.environment_not_found`). `--json` satu baris envelope dengan `result` `{ operation, idempotencyKey, preview, environment }`. `stackly env status` mendapat baris `lease:`.
- **AC-19** (override di CLI, REQ-028): `stackly instance stop <target> --override-leases`, `stackly instance stop --all --override-leases`, `stackly instance endpoint set … --override-leases`, dan `stackly daemon uninstall --override-leases` mengirim `overrideLeases` true. Tanpa flag itu, penolakan AC-13 dan AC-15 keluar 3 dengan satu baris per environment dan hint yang menyebut flag itu serta `stackly env down <environment>`. Sesudah stop dengan override, CLI mencetak environment yang kini tidak lengkap.
- **AC-20** (tombol Down dan konfirmasi): toolbar halaman detail environment mendapat tombol Down di kiri Up…. Menekannya memanggil `env.preview_down` lalu membuka konfirmasi *Copy Down*: judul "Bring down “{name}”?", kalimat mockup, daftar instance yang akan berhenti, daftar instance yang tetap jalan dengan alasannya ("used by {environment}", "started by you", "starts at login"), daftar yang tidak bisa dihentikan, dan "Data is kept." Tombolnya Cancel dan Bring Down (bukan destructive). Bring Down mengirim `env.down` dengan ID dari preview itu; `env.down_changed` memuat ulang preview dan menampilkan "What Down will stop changed. Review it again." tanpa menjalankan apa pun. Down nonaktif dengan alasan saat offline dan saat operasi `environment_down` sudah berjalan. Kalau preview tidak punya `stop`, `leaseCount` 0, dan `cancelsUp` salah, tidak ada konfirmasi dan tidak ada `env.down`: app menampilkan "Nothing to bring down."
- **AC-21** (progress, hasil, dan Activity, NFR-012): selama Down berjalan halaman detail menampilkan `StateBanner` informative "Bringing down… {done} of {total}" dengan Cancel (hanya dari `OperationSummary.cancel`) dan Show in Activity; sidebar, halaman daftar, dan Overview menampilkan "Bringing down…". Sesudah selesai, kartu umpan balik spec 0067 menampilkan hasil menurut *Copy Down*, selalu dengan teks, tidak hanya warna. Activity memberi judul "Bring down environment {name}", menyarangkan operasi anak lewat `parent`, dan memakai label "Partial" saat error nya `env.down_incomplete` dengan `stopped` di atas 0. Setiap aksi baru memakai `.mutationGated()` dan punya state loading, gagal, dan offline dari `ViewState`.
- **AC-22** (baris Lease dan kata lease): seksi Details halaman environment mendapat baris "Lease" di bawah baris spec 0045, dengan teks *Copy Lease* dari `EnvironmentDetail.lease`. Baris Ownership di inspector instance menampilkan "Dedicated · lease" atau "Shared · lease" saat ada entri `environments` ber `leaseActive` true, dan tanpa kata itu saat tidak ada. `stackly env status` mencetak hal yang sama sebagai `lease: held on <n> instances since <waktu>` atau `lease: none`.
- **AC-23** (stop dan ganti port pada instance ber lease di app): Stop dari permukaan mana pun (halaman instance, daftar, menu baris service, menu bar) yang ditolak `supervisor.stop_requires_override` membuka konfirmasi destructive "Stop “{name}”?" dengan nama environment dari `items` error itu dan kalimat *Copy override*; menekan Stop mengirim ulang dengan `overrideLeases` true. Sheet port spec 0029 menampilkan `StateBanner` caution dari `EndpointValidation.leases`, dan tombolnya menjadi "Change Anyway" yang mengirim `overrideLeases` true.
- **AC-24** (Stop All Services dan Turn Off di app): popover menu bar memanggil `instance.preview_stop_all` dengan `overrideLeases` true. Setiap baris instance ber lease menampilkan "Used by {environment}", dan kaki preview menyebut environment yang terdampak menurut *Copy Stop All*; konfirmasi mengirim `instance.stop_all` dengan `overrideLeases` true dan `expectedInstanceIds`. Konfirmasi Turn Off memakai preview yang sama, menyebut environment yang terdampak, tidak lagi nonaktif hanya karena lease environment, dan mengirim `agent_off` dengan `overrideLeases` true.
- **AC-25** (kontrak): `ProtocolVersion::CURRENT` naik satu minor untuk seluruh kontrak fitur ini (nomor ditetapkan lead saat build); method `env.preview_down` dan `env.down` (keduanya `recovery refused`), DTO baru dan tambahan di modul `env`, `instance`, `stop_all`, dan `daemon`, kode baru, dan fixture `protocols/fixtures/env/` masuk dalam satu commit dengan `cargo xtask codegen`; skenario fixture `environment-bringing-down` spec 0068 ada. Tidak ada `EventKind` baru. `OperationKind::EnvironmentDown` tidak resumable, `CloseInterrupted`, dan pengatur. `cargo xtask check-deps` dan `cargo check --target aarch64-pc-windows-msvc` untuk domain, platform, protocol, dan core tetap lulus.
- **AC-26** (bukti dan JOURNEY-04 bagian Down): semua aturan diuji dengan provider `fake`. Satu test native `apps/daemon/tests/env_down_native.rs` menjalankan Up manifest contoh ERP dari cache runtime xtask tanpa unduhan, lalu Down, dan memeriksa enam instance `stopped`, enam folder data masih ada dengan isi yang sama jumlah file nya, dan Up kedua kembali `ready` dengan data yang sama; tanpa artefak di cache ia dicatat `BLOCKED` dengan `owner: user`. Lulus di fixture bukan evidence native.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan K1 sampai K16 di [rationale.md](rationale.md): Down adalah operasi pengatur yang melepas semua lease environment dalam satu commit, lalu menghentikan tiap instance lewat operasi anak `stop_instance` yang memutuskan ulang di bawah lock instance dengan `evaluate_stop(EnvironmentDown)`; Down membatalkan Up yang sedang berjalan pada environment yang sama; dan setiap aksi yang mengganggu instance ber lease (stop, Stop All, Turn Off, ganti port) ditolak dulu lalu bisa di override dengan nama environment yang terdampak, tanpa pernah melepas lease.

Down tidak punya logika stop sendiri. Ia hanya memutuskan lease mana yang dilepas, instance mana yang dicoba dihentikan, dalam urutan apa, dan apa artinya kalau gagal.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

## Feature design

### Trace PRD

| ID PRD | Dicakup oleh |
| --- | --- |
| REQ-022 | AC-2, AC-4, AC-5, AC-6, AC-7, AC-8 |
| REQ-028 | AC-18, AC-19 |
| BR-006 | AC-7, AC-26 |
| BR-014 | AC-5, AC-11 |
| BR-015 | AC-13, AC-15, AC-16, AC-17, AC-23, AC-24 |
| VAL-014 | AC-7 (instance yang dipinjam tidak diubah Down); bagian struktur dan Up milik spec 0044 AC-8 dan spec 0046 AC-5 |
| AC-022 | AC-4, AC-5 |
| AC-073 (bersama fitur 46) | AC-11 (bagian Up milik spec 0046 AC-13 dan AC-14) |
| PRD bagian 13.2 | AC-4, AC-12, AC-13 |

### Batas dengan spec lain

| Hal | Pemilik |
| --- | --- |
| Schema manifest, plan, import, remove (`env.lease_active`), halaman Environments, `InstanceSummary.environments` | spec 0044 (dipakai; satu field baru, lihat *Update spec lain*) |
| Lockfile, override lokal | spec 0045 (tidak dipakai Down) |
| Bentuk lease, pengambilan lease, operasi `environment_up`, kelas pengatur, `Origin::Child`, `wait_final`, `ChildOrigin`, `last_up`, tombol Up… | spec 0046 (dipakai) |
| Operasi `environment_down`, pelepasan lease, preview dan konfirmasi Down, override pada stop, `stop_all`, `agent_off`, dan ganti endpoint, baris Lease, kata "· lease" | spec ini |
| `Lease`, `evaluate_stop`, `StopDecision` | spec 0004 (dipakai tanpa perubahan) |
| Alur stop, restart, reload, force stop | spec 0010, 0026 (dipanggil sebagai operasi anak; pemeriksaan lease pindah ke guard) |
| Ganti endpoint | spec 0029 (ditambah `overrideLeases` dan `leases`) |
| Rotasi sesi, `stop_all::select`, operasi `stop_all_instances`, `agent_off` | spec 0031 (aturan 6 mendapat parameter) |
| Popover menu bar, `expectedInstanceIds` | spec 0032 (ditambah dampak lease) |
| Engine operasi | spec 0007 (`EnvironmentDown` menjadi pengatur yang tidak resumable; `recover` menutup anak stop yang antre) |

### Istilah

- **Lease environment**: baris `instance_demands` ber `source_kind` `environment` (spec 0046, *Lease*). Pemegangnya `EnvironmentId`.
- **Demand lain**: lease `active` apa pun pada instance yang sama selain milik environment yang sedang Down: lease environment lain, lease `manual` (kamu menyalakannya sendiri), atau lease `startup` (menyala saat login).
- **Override**: pilihan sadar pengguna untuk tetap menjalankan aksi yang mengganggu environment. Override tidak pernah melepas lease; lease tetap `active` dan environment itu tampil tidak lengkap sampai Up lagi atau Down.
- **Himpunan stop**: ID instance di `EnvDownPreview.stop`, yang dikirim balik klien sebagai `expectedStopInstanceIds`.

### Pilihan Down

Fungsi murni `stackly_core::environment::down::select(view: &DownView) -> DownSelection { stop, keep, idle, blocked }` (tanpa IO, diuji tanpa fake). `DownView` diisi `down::read_view(registry, environment)`: member environment itu (termasuk `detached`) ditambah setiap instance yang masih punya lease `active` dari environment itu, lalu untuk tiap instance: nama, lifecycle, desired, retained, lease yang belum `released`, dan operasi yang belum final beserta key nya. Pembacaan memakai pola `stop_all::read_view` (baca ulang sampai generation sama, paling banyak `READ_TRIES` kali).

Aturan per instance, aturan pertama yang cocok menang:

| # | Kondisi | Hasil |
| --- | --- | --- |
| 1 | retained | tidak dipilih |
| 2 | `evaluate_stop(EnvironmentDown(id))` menghasilkan `KeepRunning` (ada demand lain yang `active`) | `keep`, dengan sumber setiap lease di `remaining` |
| 3 | lifecycle `unknown` | `blocked`, `supervisor.identity_unproven` |
| 4 | lifecycle bukan `starting`, `running`, `recovering`, atau `stopping`, desired bukan `running`, dan tanpa operasi `start_instance` atau `restart_instance` terbuka | `idle` |
| 5 | key nya dipegang operasi `needs_recovery` | `blocked`, `operation.recovery_required` |
| 6 | lainnya | `stop` |

Setiap daftar urut nama, lalu ID. Aturan 2 berdiri di atas aturan lain supaya instance yang masih dibutuhkan tidak pernah dilaporkan `blocked`. Aturan 6 mencakup instance `stopped` atau `failed` yang desired nya masih `running` (misalnya menunggu Keychain atau sesudah akhir sesi): stop nya hanya menulis desired `stopped`, supaya antrean start saat login tidak menyalakannya lagi. Lease `suppressed` bukan demand, sama seperti di `evaluate_stop`.

`keep` membawa alasan dari lease `remaining`: `environment` dengan `InstanceEnvironment` environment pemegang, `manual`, dan `startup`. Lease `manual` ditulis start dan restart pengguna (spec 0031), tidak pernah oleh start environment (spec 0046, `StartTrigger::Environment`), jadi instance yang hanya dinyalakan Up tidak punya demand lain.

### Precheck Down

Handler `env.down` (`apps/daemon/src/ipc/env/down.rs`), berurutan:

1. Validasi params (`EnvDownParams::validate`: `expectedStopInstanceIds` paling banyak `DOWN_EXPECTED_IDS_MAX` 256, tanpa ganda; key spec 0006).
2. `ipc::operation::replay_operation`: permintaan ulang dengan key yang sama membalas balasan pertama tanpa langkah berikutnya.
3. Di `spawn_blocking`: baca environment (`env.not_found` kalau tidak ada) dan `down::read_view`.
4. Di gerbang `begin` (sebuah `Precheck` murni atas view itu): himpunan ID `select(view).stop` harus sama dengan himpunan `expectedStopInstanceIds`; kalau berbeda, `env.down_changed`.
5. `start_operation` dengan `BeginRequest` AC-3: `kind` `EnvironmentDown`, `creates_resource` false, tanpa `expected_revision`, `input` journal `{ environmentId, name, stop: [uuid] }`.
6. Sesudah `begin` berhasil: `OperationEngine::list` dengan `active_only`, resource environment itu, dan jenis `environment_up`; untuk tiap hasil, `OperationEngine::cancel`. Kegagalan cancel (misalnya Up sudah menutup) diabaikan; Down tetap antre di belakangnya.

Balasannya `EnvDownReply` begitu operasi diterima; klien menunggu lewat event operasi.

### Operasi `environment_down`

`OperationKind::EnvironmentDown`: `resumable` false, `interrupt_policy` `CloseInterrupted`, `is_coordinator()` benar, tanpa catatan item dibuat, tanpa kompensasi, tanpa `WorkFactory`. Journal nya dua langkah, keduanya `irreversible`:

| Langkah | Stage | Isi |
| --- | --- | --- |
| `release` | `release` | commit AC-4 |
| `stop` | `stop` | AC-5 dan AC-6 untuk himpunan `stop` di `input` |

**Commit `release`.** Satu `commit_guarded`. Guard nya membaca environment lewat `GuardView::environments()` dan lease nya lewat `GuardView::leases_of_environment(id, LeaseFilter::Unreleased)` (baru), lalu menghasilkan satu `WriteOp::UpdateLease` (`Lease::release(now)`) per lease `active` dan satu `WriteOp::UpdateEnvironment` dari `Environment::begin_down(operation)` (baru: mengosongkan `last_up`, menulis `last_operation`, menaikkan revision 1) dengan revision yang dibacanya sebagai `expected`. Aturan *Tulisan environment selalu segar* spec 0046 berlaku.

**Langkah `stop`.** Driver async memakai fungsi murni `down::order` (*Urutan*) dan port anak:

```rust
// crates/core/src/environment/down/children.rs
pub trait DownChildren: Send + Sync + 'static {
    /// Starts `stop_instance` for one instance on behalf of an environment
    /// that goes down.
    fn stop(&self, parent: &ChildOrigin, instance: InstanceId, environment: EnvironmentId)
        -> BoxFuture<'_, Result<DownChild, FailureError>>;
}

pub enum DownChild {
    /// The operation runs; wait for it with `OperationEngine::wait_final`.
    Started { operation: OperationId },
    /// Another lease holds the instance; nothing was written.
    Kept,
    /// Nothing runs and the desired state is already `stopped`.
    AlreadyStopped,
}
```

`ChildOrigin` adalah tipe spec 0046 (`environment::up::children`), dipindah ke `environment::children` supaya dipakai keduanya. `stacklyd` mengimplementasikan port itu (`apps/daemon/src/ipc/env/children.rs`) dengan memanggil bagian "permintaan internal ke `begin`" handler `instance.stop`, dengan beda berikut dari stop klien: `StopRequest::environment_down(instance, environment)`, `BusyPolicy::Queue` (stop klien `Reject`), `Origin::Child`, dan precheck sendiri (*Precheck stop anak*). Tepat sebelum memulai anak, driver menjalankan `select` atas view satu instance itu: `keep` langsung `kept`, `idle` langsung `already_stopped`, `blocked` langsung `failed` dengan kodenya, instance yang sudah dihapus atau retained dibuang dari hasil; hanya `stop` yang memulai anak.

**Precheck stop anak** (tidak pernah menulis; beda dari `stop_precheck` klien, yang jalur `no_op_with_desired` nya tetap dipakai stop klien saja):

| Keadaan instance | Hasil |
| --- | --- |
| tidak ada, atau retained | penolakan dengan kode yang ada; driver membuangnya dari hasil |
| `unknown`, `initializing` | penolakan dengan kode `stop_precheck` yang ada (`failed`) |
| `evaluate_stop(EnvironmentDown)` atas lease yang dibaca menghasilkan `KeepRunning` | `DownChild::Kept`, tanpa operasi |
| `uninitialized`, atau `stopped` atau `failed` dengan desired `stopped` | `DownChild::AlreadyStopped`, tanpa operasi |
| `stopped` atau `failed` dengan desired `running` | `Proceed`: operasi berjalan, dan cabang "tidak ada proses" pekerjaan stop menulis desired `stopped` lewat commit ber guard |
| `starting`, `running`, `recovering`, `stopping` | `Proceed` |

Pemetaan hasil anak:

| Kejadian | Hasil instance |
| --- | --- |
| `DownChild::Kept` | `kept` |
| `DownChild::AlreadyStopped` | `already_stopped` |
| anak `succeeded` dan `result.partial` nya memuat kode `env.instance_in_use` (guard menemukan demand lain) | `kept` |
| anak `succeeded` selain itu | `stopped` |
| anak `failed`, `cancelled`, atau `needs_recovery`, atau penolakan saat memulai | `failed` dengan kode error anak (`needs_recovery` memakai `operation.recovery_required`) |
| anak masih `queued` lebih lama dari `CHILD_QUEUE_TIMEOUT_SECONDS` 120 (spec 0046) | dibatalkan induk; hasil sementara `busy`, diputuskan *Sapuan akhir* |
| penolakan `daemon.restart_pending` (gerbang quiesce) | driver berhenti memulai anak baru; sisanya `cancelled`, Down berakhir `failed` dengan `env.down_incomplete` |

Pekerjaan stop yang guard nya memutuskan `KeepRunning` kembali `Ok` dengan satu item `partial` (`item` nama instance, `resource` instance itu, `outcome` `skipped`, `code` `env.instance_in_use`) dan catatan audit `kept: true`, supaya stop yang tidak terjadi tidak tercatat sebagai stop.

Down tidak membatalkan operasi start yang terbuka pada instance (beda dari Stop All): anak stop antre di belakangnya, dan start pengguna yang selesai meninggalkan lease `manual`, sehingga guard stop memutuskan `kept`. Itu hasil yang benar, karena kamu baru saja meminta instance itu menyala.

**Sapuan akhir.** Sesudah setiap anak berakhir, dan hanya kalau Down tidak dibatalkan dan gerbang quiesce tidak menutupnya, driver membaca `down::read_view` sekali lagi dan menjalankan `select`:

- instance berhasil sementara `busy`: `kept` kalau sapuan menaruhnya di `keep`, `already_stopped` kalau di `idle`, selain itu `failed` dengan `resource.busy`;
- instance yang sapuan taruh di `stop` dan tidak ada di himpunan yang dikonfirmasi: `left_running` dengan kode `env.down_changed`. Down tidak menghentikannya. Ini menangkap dua Down serentak yang saling melihat lease yang lain, dan instance yang sempat dinyalakan anak Up di antara precheck dan pembatalan Up itu;
- instance yang sudah `failed` tidak dilaporkan dua kali.

`set_progress` memakai `Progress::Counted { done, total }` dengan `total` jumlah instance di himpunan `stop`.

### Stop dengan guard lease

Pemeriksaan lease mengikuti permintaan, bukan `StopIntent`. `StopIntent::User` (`crates/domain/src/restart.rs`) juga dipakai separuh stop restart (`restart_op.rs`), stop sesudah start yang dibatalkan (`startup.rs`, `session_end.rs`), dan langkah Stop All, dan ketiganya tidak boleh berubah perilaku (AC-14). Karena itu `Supervisor::journal_stop` dan `stop_direct` mendapat satu parameter baru:

```rust
// crates/core/src/supervisor/stop.rs
pub(crate) enum LeaseCheck {
    /// No lease is read: restart, a cancelled start, reconcile, planned
    /// restart, session end.
    None,
    /// A person stops the instance (BR-015).
    Manual { session: Option<SessionId>, override_leases: bool },
    /// An environment goes down (BR-014).
    EnvironmentDown(EnvironmentId),
}
```

| Pemanggil | `LeaseCheck` | `StopIntent` |
| --- | --- | --- |
| `run_stop` untuk `StopRequest::new` (stop klien) | `Manual { session, override_leases }` dari permintaan | `User` |
| `run_stop` untuk `StopRequest::environment_down` (baru) | `EnvironmentDown(environment)` | `User` (menulis desired `stopped`) |
| langkah `stop_all_instances` (`stop_all_step`) | `Manual { session, override_leases }` dari `StopAllOptions` | `User` |
| restart, start yang dibatalkan, rekonsiliasi, restart terencana, akhir sesi | `None` | tidak berubah |

`StopIntent::checks_leases()` dihapus; `StopIntent` tidak mendapat varian baru. Pemicu stop Down adalah `environment_down`, dicatat di tempat yang sama dengan `session_end` spec 0031.

Dengan `LeaseCheck` selain `None`, commit `journal_stop` yang menulis desired `stopped` (commit `stopping` untuk proses hidup, commit `exited` tanpa proses, dan commit desired saja di cabang "tidak ada proses" `run_stop`) berjalan lewat helper baru `persist::commit_stop_decision`. Guard nya membaca `GuardView::leases(instance, LeaseFilter::Unreleased)` di dalam transaksi, menjalankan `evaluate_stop`, dan hasilnya final (tidak dibangun ulang berulang seperti `commit_exit`):

| `LeaseCheck` | Hasil `evaluate_stop` di guard | Hasil commit | Akibat |
| --- | --- | --- | --- |
| `Manual` | `RequiresOverride` | `StopCommit::RequiresOverride(holders)` | tidak ada tulisan; operasi gagal `supervisor.stop_requires_override` dengan `items`; langkah Stop All menjadi `skipped` dengan kode itu |
| `Manual` | `Stop` | `StopCommit::Committed` | `release` dan `suppress` seperti `manual_stop_ops` (dihitung di guard dari lease yang dibacanya), desired `stopped`, lanjut alur stop |
| `EnvironmentDown` | `KeepRunning` | `StopCommit::Kept` | tidak ada tulisan; pekerjaan kembali `Ok` dengan hasil `kept` |
| `EnvironmentDown` | `Stop` | `StopCommit::Committed` | lease di `release` (kalau masih ada) dilepas, desired `stopped`, lanjut alur stop |

`holders` diisi guard dari lease environment `active`, `GuardView::environments()`, dan member nya, sebagai `Vec<EnvironmentHolder { environment, name, alias, ownership }>`; varian `SupervisorError::StopRequiresOverride` membawa daftar itu, dan precheck handler mengisinya dengan cara yang sama.

Commit `exited` tanpa proses yang juga butuh guard riwayat force stop (`commit_exit`, spec 0026) menjalankan kedua pemeriksaan di satu guard: keputusan lease lebih dulu dan final; pembacaan niat force tetap membangun ulang seperti sekarang.

**Respawn.** `run_stop` hari ini memanggil `actor.cancel_respawn` sebelum `journal_stop`. Instance `recovering` yang guard nya memutuskan `Kept` atau `RequiresOverride` tidak boleh kehilangan respawn nya. Aturannya: keputusan lease dibaca sekali sebelum `cancel_respawn` (seperti pemeriksaan `manual_stop_ops` sekarang), dan hasil `KeepRunning` atau `RequiresOverride` di titik itu kembali tanpa menyentuh actor. Kalau guard commit kemudian memutuskan lain dari bacaan awal (lease muncul di antara keduanya), pekerjaan menjadwalkan ulang respawn instance itu lewat jalur yang sama dengan rekonsiliasi instance `recovering` sebelum kembali. Test menutup kasus ini dengan instance `recovering` dan lease yang disisipkan di antara dua bacaan.

Dengan guard ini, urutan apa pun antara "Up mencatat lease" dan "stop menulis `stopping`" menghasilkan keadaan yang konsisten (AC-11, AC-13), sedangkan restart, cancel start, dan anak Up yang dibatalkan Down tetap bisa menghentikan prosesnya sendiri tanpa cek lease.

### Urutan

Fungsi murni `down::order(members, stop_set, ended) -> Vec<InstanceId>`: sebuah instance P boleh mulai kalau setiap member D di himpunan `stop` yang `dependency_conditions` nya memuat alias P sudah berakhir. Edge ke alias yang tidak ada lagi, dan edge dari member yang tidak di himpunan `stop`, diabaikan. Yang boleh mulai dipilih menurut alias lalu ID, paling banyak `DOWN_STOP_CONCURRENCY` sekaligus. Kalau tidak ada yang boleh mulai dan tidak ada yang berjalan tetapi masih ada sisa (data edge rusak), sisanya dimulai menurut alias; cycle sudah ditolak validate spec 0044. Dependent yang gagal berhenti tidak menahan dependency nya: Down tetap mencoba semuanya dan melaporkan tiap hasil. Konstanta ada di `crates/core/src/environment/down/limits.rs`.

### Override pada aksi lain

| Aksi | Tanpa override | Dengan override | Lease |
| --- | --- | --- | --- |
| `instance.stop` pada instance yang hidup | `supervisor.stop_requires_override` + `items` | berhenti, desired `stopped` | tetap `active` |
| `instance.stop` pada instance yang sudah diam | tidak ditolak; hanya desired `stopped` | sama | tetap `active` |
| `instance.stop_all`, `instance.preview_stop_all` | instance ber lease `skipped` | masuk `stop` | tetap `active` |
| `daemon.shutdown` `agent_off` | instance ber lease `skipped`, daemon tetap jalan | ikut dihentikan | tetap `active` |
| `instance.change_endpoints` (port berubah) | `instance.endpoints_require_override` + `items` | berjalan seperti spec 0029 | tetap `active` |
| `instance.restart`, `reload`, `config.apply`, `rename`, `force_stop` | tidak ditolak | tidak ada field | tidak berubah |
| Akhir sesi, restart daemon terencana, rekonsiliasi | tidak memeriksa lease (spec 0030, 0031) | | tidak berubah |
| `instance.remove` pada member | ditahan FK `restrict` spec 0005 | milik fitur hapus instance | |

`stop_all::select` mendapat `StopAllOptions { override_leases: bool }`; aturan 6 hanya berlaku saat `override_leases` false. Langkah stop operasi `stop_all_instances` (`stop_all_step`) hari ini membangun `manual_stop_ops` dengan override selalu true lalu memanggil `stop_direct`; ia kini meneruskan `LeaseCheck::Manual` dengan nilai `override_leases` operasi itu ke `stop_direct`, dan `input` journal nya mencatat `overrideLeases`. `agent_off` meneruskan nilai dari `ShutdownParams.overrideLeases` ke kedua pemanggilan `select` nya (awal dan akhir).

`instance.change_endpoints`: pemeriksaan lease berjalan di precheck handler dan diulang di guard `commit_guarded` operasi `change_endpoints`. "Benar benar mengubah port" berarti hasil `ChangePlan` memuat paling sedikit satu endpoint yang `port_request` nya berbeda dari yang aktif; permintaan yang sama dengan keadaan sekarang dijawab `unchanged` seperti sebelumnya, tanpa cek lease.

**Sesudah override.** Instance berhenti dengan desired `stopped` dan lease masih `active`. Environment nya tampil `partially_ready` atau `down` dengan alasan `stopped` (spec 0044). Rotasi sesi spec 0031 tidak menyentuh desired instance ber lease environment, jadi ia tetap berhenti sesudah login lagi. Up berikutnya menyalakannya lewat `start_instance` pemicu `environment`; start manual menyalakannya dengan lease `manual`. Down melepas lease nya seperti biasa.

### Model data

Tidak ada tabel atau kolom baru, dan tidak ada migrasi.

- `instance_demands` (spec 0005, bentuk lease di spec 0046): Down menulis `state` `released` dan `released_at` lewat `WriteOp::UpdateLease` yang sudah ada. Baris `released` dipangkas aturan spec 0031 AC-14 dan dihapus `WriteOp::DeleteEnvironment` (spec 0044).
- `environments.last_up` dikosongkan dan `environments.last_operation_id` ditulis oleh commit `release`.
- `environment_instances` tidak pernah ditulis Down. `dependency_conditions` hanya dibaca.

**Domain** (`stackly-domain`, usulan untuk spec 0004 dan 0005):

- `OperationKind::EnvironmentDown`: `resumable()` salah, `interrupt_policy()` `CloseInterrupted`, `is_coordinator()` benar.
- `Environment::begin_down(operation: OperationId)`: `last_up` menjadi `None`, `last_operation` terisi, revision naik 1.
- `Registry::leases_of_environment(environment, filter) -> Versioned<Vec<Lease>>` dan `GuardView::leases_of_environment(environment, filter)`: lease ber `source_kind` `environment` dan `source_id` itu, urut `acquired_at` lalu ID. Fake registry testkit dan `registry_conformance` ikut.
- `evaluate_stop`, `StopRequest`, `StopDecision`, dan `Lease` tidak berubah.

**Core**: `crates/core/src/environment/down/` berisi `mod.rs` (pekerjaan operasi), `select.rs` (`DownView`, `read_view`, `select`), `order.rs`, `driver.rs`, `children.rs` (port), `limits.rs`. `crates/core/src/environment/children.rs` memegang `ChildOrigin`. `crates/core/src/supervisor/stop.rs`, `daemon_restart.rs` (`stop_direct`), dan `persist.rs` mendapat `LeaseCheck` dan `commit_stop_decision`. `crates/core/src/instance/stop_all.rs` dan `crates/core/src/supervisor/stop_all.rs` mendapat `StopAllOptions`. `crates/core/src/session.rs`: `manual_stop_ops` dipanggil dari dalam guard.

### API surface

Dua method baru, lewat handshake dan peer user yang sama (spec 0006).

| Method | Jenis | Recovery | Input | Output | Error utama |
| --- | --- | --- | --- | --- | --- |
| `env.preview_down` | baca, bukan operasi | refused | `EnvPreviewDownParams { id }` | `EnvDownPreview` | `env.not_found` |
| `env.down` | operasi `environment_down` | refused | `EnvDownParams { id, expectedStopInstanceIds, idempotencyKey }` | `EnvDownReply { environmentId, name, operation: OperationAccepted }` | `env.not_found`, `env.down_changed`, `operation.queue_full`, `operation.recovery_required`; sebagai kegagalan operasi: `env.not_found`, `env.down_incomplete` |

Method yang diperluas (field baru memakai `#[serde(default)]`):

| Method | Tambahan |
| --- | --- |
| `instance.stop` | error `supervisor.stop_requires_override` kini membawa `name`, `alias`, `ownership` di tiap `items` |
| `instance.change_endpoints` | params `overrideLeases: bool`; `EndpointValidation.leases: [InstanceEnvironment]`; error `instance.endpoints_require_override` |
| `instance.preview_stop_all` | params `StopAllPreviewParams { overrideLeases: bool }`. Method ini hari ini varian tanpa params (`Call::InstancePreviewStopAll`); ia menjadi varian ber params yang menerima params yang tidak ada, `null`, atau objek kosong sebagai nilai default, jadi permintaan klien lama tetap sah dan ini tetap kenaikan minor. Fixture memuat permintaan tanpa params |
| `instance.stop_all` | params `overrideLeases: bool`, ikut sidik jari idempotency |
| `daemon.shutdown` | `ShutdownParams.overrideLeases: bool`, hanya dibaca untuk `agent_off` |

**DTO** (camelCase; enum terbuka berakhir `unrecognized`; hitungan memakai `Count`):

- `EnvDownPreview { environment: EnvironmentRef, stop: [DownInstance], keep: [DownKeep], idle: [DownInstance], blocked: [DownBlocked], leaseCount: Count, cancelsUp: bool }`. `leaseCount` jumlah lease `active` environment itu. `cancelsUp` benar saat ada operasi `environment_up` yang belum berakhir untuk environment itu.
- `DownInstance { instanceId, name, alias?, ownership?: EnvOwnership, lifecycle: InstanceLifecycle }`. `alias` dan `ownership` dari baris member; keduanya kosong untuk instance ber lease tanpa baris member.
- `DownKeep { instance: DownInstance, demand: [DemandSourceKind], environments: [InstanceEnvironment] }`. `DemandSourceKind` dipakai ulang dari modul `stop_all`; `environments` hanya environment lain yang lease nya `active`.
- `DownBlocked { instance: DownInstance, code }`.
- `EnvDownParams`: `expectedStopInstanceIds: [Uuid]` wajib (boleh kosong).
- `EnvironmentDetail` mendapat `lease?: EnvLeaseInfo { activeCount: Count, heldSince?: Timestamp }`; `heldSince` adalah `acquired_at` terkecil di antara lease `active`, kosong saat `activeCount` 0. Field itu kosong hanya dari daemon lama.
- `InstanceEnvironment` (spec 0044) mendapat `leaseActive: bool`.
- `StopAllItem` dan `StopAllSkip` mendapat `environments: [InstanceEnvironment]`; `StopAllPreview` mendapat `affectedEnvironments: [AffectedEnvironment { environmentId, name, instanceCount: Count }]`, yaitu environment yang punya lease `active` pada paling sedikit satu instance di `items`, urut nama.
- `EnvironmentSummary.activeOperation.kind` (spec 0046) kini bisa `environment_down`.

**Event**: tidak ada `EventKind` baru. Klien memakai aturan muat ulang spec 0046 (event operasi ber resource `environment` dan event operasi anak). Tambahan: klien memuat ulang `instance.list` saat `operation.state_changed` operasi ber resource `environment` berganti stage atau berakhir, supaya `leaseActive` segar, karena pencatatan dan pelepasan lease tidak menerbitkan event instance. Cache keanggotaan daemon (spec 0044) ikut menyimpan `leaseActive` dan disegarkan oleh setiap commit yang menulis `instance_demands` ber sumber environment atau `environment_instances`: commit `accept` Up, batch `register` create dan kompensasinya, commit `release`, hapus instance, dan `env.remove`.

**Anggaran frame**: `EnvDownPreview` paling besar sekitar 64 instance kali 0,5 KiB; error `env.down_incomplete` paling banyak 32 `items`.

### Kode error di wire

Semua `retryable` false. Pesan lowercase tanpa titik.

| Kode | Kategori | Kapan | Kunci `details` | `items` |
| --- | --- | --- | --- | --- |
| `env.down_changed` | `conflict` | himpunan `stop` sekarang berbeda dari `expectedStopInstanceIds`; juga `code` instance `left_running` di sapuan akhir | | |
| `env.down_incomplete` | `failed` | Down selesai dengan instance yang tidak berhenti | `stopped`, `total`, `count`, `omitted` | `instance`: `instanceId`, `name`, `outcome` (`failed`, `cancelled`, `left_running`), `code`, `operationId`; paling banyak 32 |
| `env.instance_in_use` | `conflict` | hanya sebagai `code` item `partial`: instance tetap jalan karena demand lain muncul sesudah preview | | |
| `instance.endpoints_require_override` | `conflict` | ganti port pada instance ber lease environment tanpa override | `count`, `omitted` | `environment`: `environmentId`, `name`, `alias`, `ownership`; paling banyak 100 |
| `supervisor.stop_requires_override` (sudah ada) | `conflict` | stop tanpa override | `omitted` | `environment`: `environmentId` (sudah ada), ditambah `name`, `alias`, `ownership`; batas 100 yang ada tidak berubah |

Hint: `env.down_changed` "review what will stop, then bring it down again"; `env.down_incomplete` seperti AC-8; `instance.endpoints_require_override` "change the ports anyway with the override, or bring the environment down first"; `supervisor.stop_requires_override` "stop it anyway with the override, or bring the environment down".

Kode lokal CLI: tidak ada yang baru. `cli.environment_not_found` dipakai ulang dari spec 0044.

### CLI

Di grup `stackly env` (`apps/cli/src/commands/env/down.rs`), memakai `AsyncArgs` spec 0008 dan resolver target spec 0046 (`--file`, path, nama, UUID, atau `./stackly.yaml`).

| Perintah | Perilaku | `result` `--json` |
| --- | --- | --- |
| `stackly env down [target] [--wait] [--timeout <durasi>] [--idempotency-key <key>]` | `env.preview_down`, lalu ke stderr: `stop <instance>` per `stop`, `keep <instance>: <alasan>` per `keep`, `skip <instance>: <kalimat kode>` per `blocked`, dan `cancels the running up` saat `cancelsUp`; lalu `env.down`. Tanpa `--wait`: "Started bringing down <name> (operation <id>)". Dengan `--wait`: `stopped <instance>`, `kept <instance>`, `failed <instance>: <kode>`, atau `left running <instance>` per instance yang berakhir, lalu "Down. <s> stopped, <k> kept running." Preview tanpa `stop`, dengan `leaseCount` 0 dan `cancelsUp` salah, tetap mengirim `env.down` (supaya script punya satu perilaku) dan mencetak "Nothing to stop." Tanpa konfirmasi, karena tidak ada yang dihapus | `{ operation, idempotencyKey, preview: EnvDownPreview, environment: EnvironmentDetail \| null }` (`environment` terisi dengan `--wait`) |
| `stackly env status [target]` (spec 0046) | mendapat baris `lease: held on <n> instances since <RFC 3339>` atau `lease: none` di bawah baris kepala | `EnvironmentDetail` (dengan `lease`) |
| `stackly instance stop <target> [--override-leases]` | flag mengirim `overrideLeases` true; sesudah sukses mencetak "<name> stopped. <environment> is no longer complete." per environment ber `leaseActive` | tidak berubah |
| `stackly instance stop --all [--override-leases]` | preview dan stop all dengan `overrideLeases` dari flag; konfirmasi yang ada mendapat baris "affects <environment> (<n> instances)" | tidak berubah |
| `stackly instance endpoint set … [--override-leases]` | flag mengirim `overrideLeases` true | tidak berubah |
| `stackly daemon uninstall [--override-leases]` | preview dan `agent_off` dengan `overrideLeases` dari flag | tidak berubah |

Alasan `keep`: `environment` "used by <name>" (nama dipisah koma), `manual` "started by you", `startup` "starts at login"; beberapa alasan digabung dengan "; ". Exit code hanya dari kategori (spec 0008): `env.down_changed` 3 (sesudah satu ulangan otomatis), `env.down_incomplete` 7, dibatalkan 9, `--wait` yang kehabisan `--timeout` 8 (`cli.wait_timeout`) tanpa membatalkan Down. Penolakan override keluar 3 dengan satu baris `<name> is used by <environment> (<alias>)` per item dan hint yang menyebut `--override-leases` dan `stackly env down <environment>`.

### Layar

Semua komponen dari `Components/`, `State/`, `Status/`, `Confirmation/`; copy bahasa Inggris; spec menang atas mockup untuk perilaku.

**Toolbar halaman detail**: Validate, lalu kelompok kanan Down dan Up… (accent). Down nonaktif dengan alasan: offline (alasan `.mutationGated()`) atau `activeOperation.kind` `environment_down` ("Already bringing down"). Down tetap aktif selama Up berjalan. Klien tidak menebak apakah ada yang perlu dihentikan; itu jawaban preview. Up… nonaktif selama `activeOperation` terisi (spec 0046), dengan alasan "Bringing down" untuk jenis ini.

**Copy Down** (`EnvironmentDownModel`, `ConfirmationRequest` ber role `standard`; baris pesan dipisah baris baru; daftar nama paling banyak 6 lalu "and {n} more"):

| Tempat | Kondisi | Teks |
| --- | --- | --- |
| judul | | "Bring down “{name}”?" |
| pesan, baris 1 | | "Releases the lease and stops only instances nothing else needs. Shared and manually started instances keep running. Data is kept." |
| pesan | `stop` tidak kosong | "Stops: {nama, nama}." |
| pesan | `stop` kosong | "Nothing needs to stop." |
| pesan | `keep` tidak kosong | "Keeps running: {nama} ({alasan}), …" dengan alasan "used by {environment}", "started by you", "starts at login", digabung " and " |
| pesan | `blocked` tidak kosong | "Can’t stop: {nama} ({kalimat kode}), …" dengan `supervisor.identity_unproven` "Stackly can’t confirm this process" dan `operation.recovery_required` "an operation for it needs your attention" |
| pesan | `cancelsUp` | "This also cancels the Up in progress." |
| tombol | | Cancel, Bring Down |
| preview kosong | `stop` kosong, `leaseCount` 0, `cancelsUp` salah | tanpa konfirmasi; umpan balik "Nothing to bring down." |
| `env.down_changed` | | "What Down will stop changed. Review it again." lalu konfirmasi dibuka lagi dengan preview baru |
| banner selama Down | | "Bringing down… {done} of {total}" dengan Cancel dan Show in Activity |
| umpan balik | `succeeded`, `stopped` > 0, tanpa `kept` | "{name} is down. {s} instances stopped. Data is kept." |
| umpan balik | `succeeded` dengan instance yang tetap jalan | "{name} is down. {s} stopped, {k} kept running." |
| umpan balik | `succeeded`, tidak ada yang berhenti | "{name} released its lease. Nothing needed to stop." |
| umpan balik | `env.down_incomplete` | "{name} is not fully down. {instance} didn’t stop." atau "{f} instances didn’t stop." dengan Show Operation; kalau semua item `left_running`: "{name} released its lease, but {instance} is still running. Bring it down again." dengan Bring Down… |
| umpan balik | `cancelled`, `lease.activeCount` 0 sesudah muat ulang | "Bringing down {name} was cancelled. Its lease is released." |
| umpan balik | `cancelled`, `lease.activeCount` di atas 0 | "Bringing down {name} was cancelled before anything changed." |

Angka `{s}` dari `result.resources`, `{k}` dari jumlah `keep` preview ditambah item `partial` ber kode `env.instance_in_use`, `{f}` dari `details.count`.

**Copy Lease** (baris "Lease" di seksi Details, di bawah baris Lock dan Local override spec 0045):

| Kondisi | Teks |
| --- | --- |
| `activeCount` 1 | "Lease held on 1 instance since {waktu}. Down stops only instances with no other lease or manual start." |
| `activeCount` n > 1 | "Lease held on {n} instances since {waktu}. Down stops only instances with no other lease or manual start." |
| `activeCount` 0 | "No lease. Up takes one on each service’s instance." |
| `lease` kosong (daemon lama) | baris disembunyikan |

`{waktu}` memakai format waktu yang sama dengan baris Activity (jam saja untuk hari ini, tanggal dan jam selain itu).

**Inspector instance**: baris Ownership menampilkan "Dedicated · lease" atau "Shared · lease" saat ada entri `environments` ber `leaseActive` true, selain itu teks spec 0044.

**Copy override** (konfirmasi sesudah `supervisor.stop_requires_override`, role `destructive`, dari `items` error):

| Kondisi | Judul | Pesan | Tombol |
| --- | --- | --- | --- |
| satu environment | "Stop “{name}”?" | "It’s leased by {environment}. Stopping it leaves the environment partially ready. Data is kept." | Cancel, Stop |
| lebih dari satu | "Stop “{name}”?" | "It’s leased by {environment, environment}. Stopping it leaves those environments partially ready. Data is kept." | Cancel, Stop |

Satu fungsi di model aksi instance menangani ini untuk semua permukaan Stop, jadi tidak ada permukaan yang mengirim `overrideLeases` true tanpa konfirmasi itu.

**Sheet port** (spec 0029): saat `EndpointValidation.leases` tidak kosong, `StateBanner` caution "{environment} uses this instance. Changing its ports can break that environment until you update what connects to it." (nama digabung " and "), dan tombol Apply menjadi "Change Anyway".

**Copy Stop All** (tambahan atas spec 0032; popover memanggil preview dengan `overrideLeases` true):

| Tempat | Kondisi | Teks |
| --- | --- | --- |
| baris item, sumber `environment` | `environments` tidak kosong | "Used by {environment}" (nama digabung ", ") menggantikan "Used by an environment" |
| kaki | `affectedEnvironments` satu | "{environment} will no longer be complete. Its lease stays." |
| kaki | `affectedEnvironments` n > 1 | "{n} environments will no longer be complete: {nama, nama}. Their leases stay." |

**Turn Off** (spec 0031): konfirmasi memakai preview dengan `overrideLeases` true; instance ber lease tampil di daftar yang akan berhenti dengan "Used by {environment}", ditambah kalimat "Environments keep their leases. Bring them up again after you turn Stackly back on." Turn Off tetap nonaktif untuk alasan `skipped` lain.

**Activity dan Overview**: `OperationKind.environment_down` berjudul "Bring down environment {name}" ("Bring down environment" kalau tidak dikenal). Operasi anak bersarang lewat `parent` (spec 0046). Label "Partial" (nada caution) saat `failure.error.code` `env.down_incomplete` dan `details.stopped` di atas 0, dengan catatan "{stopped} of {total} instances stopped".

**Sidebar, daftar, Overview Environments**: saat `activeOperation.kind` `environment_down`, teks status "Bringing down…" (informative, `arrow.triangle.2.circlepath`).

**Remove** (spec 0044): pesan `env.lease_active` "Bring it down first." mendapat aksi Bring Down… yang membuka konfirmasi di atas. Remove yang dicoba lagi sesudah Down memakai revision dari `env.get` yang dimuat ulang, karena Down menaikkan revision environment.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| `env.preview_down`, `env.down` | daftar instance | member `environment_instances` environment itu ditambah instance ber lease `active` dari environment itu |
| `select` | lease per instance | `Registry::leases(instance, LeaseFilter::Unreleased)` |
| `select` | lifecycle, desired, retained | baris `instances` |
| `select` | operasi terbuka dan `needs_recovery` | journal operasi, seperti `stop_all::read_view` |
| preview | alasan `keep` | pemegang tiap lease di `StopDecision::KeepRunning.remaining` |
| preview | nama environment lain | baris `environments` ber `id` `source_id` lease; alias dan ownership dari `environment_instances` |
| preview | `leaseCount` | `Registry::leases_of_environment(id)` yang `active` |
| preview | `cancelsUp` | `OperationEngine::list` dengan `active_only`, resource environment, jenis `environment_up` |
| `env.down` | himpunan stop yang dijalankan | `select` di precheck, harus sama dengan `expectedStopInstanceIds`; disimpan di `input` journal |
| `env.down` (replay) | `environmentId`, `name` di balasan | `input` journal operasi |
| `release` | lease yang dilepas | `GuardView::leases_of_environment` di dalam guard |
| `release` | `released_at` | clock platform saat commit |
| `stop` | keputusan akhir per instance | `evaluate_stop(EnvironmentDown(id))` atas lease yang dibaca guard commit stop |
| `stop` | urutan | `dependency_conditions` tiap member, alias ke instance lewat `environment_instances` |
| `stop` | hasil `stopped` atau `kept` | `DownChild`, atau `result.partial` operasi anak (kode `env.instance_in_use`) |
| sapuan akhir | `left_running`, nasib `busy` | `select` atas `down::read_view` yang dibaca sesudah semua anak berakhir |
| umpan balik `cancelled` | lease masih dipegang atau tidak | `EnvironmentDetail.lease.activeCount` sesudah muat ulang |
| `env.get` | `lease.activeCount`, `heldSince` | `leases_of_environment` yang `active`; `acquired_at` terkecil |
| `instance.list`, event instance | `leaseActive` | cache keanggotaan daemon (spec 0044) yang ikut menyimpan lease `active` per pasangan instance dan environment |
| stop, ganti endpoint | `items` environment | lease environment `active` instance itu, digabung baris `environments` dan `environment_instances` |
| preview stop all | `environments`, `affectedEnvironments` | lease `active` di `StopAllView`, digabung baris `environments` |
| GUI | "{done} of {total}" | `OperationSummary.progress` |
| GUI | `{s}`, `{k}`, `{f}` | `OperationSummary.result` dan `failure.error.details`, serta preview yang dikonfirmasi |
| GUI | `{waktu}` | `EnvironmentDetail.lease.heldSince` |

### Key invariants

- Down tidak pernah menghapus atau mengubah data, config, runtime, secret, instance, member, atau environment selain `last_up`, `last_operation_id`, dan revision.
- Sesudah commit `release`, environment tidak memegang lease `active`. Hanya Down, kompensasi create (spec 0046), dan hapus instance yang melepas atau menghapus lease environment.
- Instance tidak pernah dihentikan Down selagi punya lease `active` dari pemegang lain, dan keputusan itu diambil di dalam transaksi yang menulis desired `stopped`.
- Down tidak pernah berakhir `succeeded` selagi ada member yang hidup tanpa demand; sapuan akhir melaporkannya.
- Restart, start yang dibatalkan, rekonsiliasi, restart terencana, dan akhir sesi tidak pernah membaca lease.
- Down tidak pernah menghentikan instance di luar himpunan yang ditampilkan dan dikirim balik klien.
- Override tidak pernah melepas lease. Tidak ada jalur yang menghentikan instance ber lease environment atas permintaan pengguna tanpa `overrideLeases` true, kecuali Down milik environment pemegang terakhir.
- Preview, precheck, dan pemeriksaan ulang memilih lewat `down::select` saja; Stop All lewat `stop_all::select` saja.
- Operasi pengatur tidak memegang slot berjalan dan tidak memegang lock yang dibutuhkan anaknya.

### Security model

- Peer adalah user yang sama (spec 0006); semua instance dan environment ada di registry user itu.
- Klien hanya mengirim ID environment dan himpunan ID yang ia tampilkan. Daemon menghitung sendiri pilihan dan menolak kalau berbeda, jadi klien tidak bisa membuat Down menghentikan instance di luar environment itu: ID yang bukan hasil `select` selalu membuat `env.down_changed`.
- `overrideLeases` adalah pilihan eksplisit per permintaan. GUI hanya mengirimnya sesudah konfirmasi yang menyebut environment; CLI hanya lewat flag.
- Secret: Down tidak membaca atau menulis secret. Tidak ada nilai di params, journal, hasil, event, atau log.
- Audit: operasi `environment_down` menulis baris audit lewat engine dengan `set_audit_note` berisi nama environment, jumlah lease yang dilepas, dan nama instance `stopped`, `kept`, dan `failed`. Stop, Stop All, `agent_off`, dan ganti endpoint dengan override menambah `overriddenEnvironments` (nama) ke catatan audit operasinya. Semua nilai lewat `Redactor`.

### Configuration required

Tidak ada variabel environment atau credential baru.

### Critical test scenarios

- `select`: setiap baris tabel *Pilihan Down*, termasuk lease `suppressed`, instance `stopped` ber desired `running`, start pengguna yang antre, member `detached`, dan urutan nama; verifies **AC-1**.
- `env.down` dengan himpunan yang berbeda, environment tidak dikenal, manifest hilang, dan environment tanpa lease; pohon file dan generation registry tidak berubah pada penolakan; verifies **AC-2**.
- Down selagi Up berjalan di tiap tahap (install, create, start, menunggu dependency): Up berakhir `cancelled` dengan `last_up` tertulis lalu dikosongkan Down, tidak ada deadlock dengan `max_running` 2; Up selagi Down berjalan ditolak `resource.busy`; verifies **AC-3**.
- Commit `release`: semua lease environment `released` termasuk milik member `detached`, lease lain utuh, `last_up` kosong; environment yang dihapus di antara precheck dan langkah itu menggagalkan dengan `env.not_found`; verifies **AC-4**.
- Dua environment satu instance, Down yang pertama: lease kedua `active`, PID sama, `ready`; lalu Down yang kedua menghentikannya; instance dengan lease `manual` dan dengan lease `startup` tetap jalan; verifies **AC-5**.
- Rantai tiga dan berlian dengan provider `fake`: urutan `stopping` terbalik dari urutan start; dependent yang `kept` tidak menahan; dependent yang stop nya timeout tidak menahan; verifies **AC-6**.
- Pohon data, baris registry, dan config revision sama sebelum dan sesudah Down yang sukses, gagal, dan batal; instance `instanceRef` identik; verifies **AC-7**.
- Stop yang timeout di satu instance: `env.down_incomplete` dengan `items`, lease tetap `released`, Down kedua hanya mengerjakan sisanya; demand yang muncul sesudah preview menghasilkan `kept` dan item `partial`; instance yang dinyalakan di antara precheck dan langkah `stop` dilaporkan `left_running`; anak stop yang antre lebih dari batas di belakang start pengguna berakhir `kept` sesudah sapuan; verifies **AC-8**.
- Cancel saat antre, sesudah `release`, dan di tengah stop; verifies **AC-9**.
- Daemon berhenti sesudah `release` dan di tengah langkah `stop`; anak stop yang antre ditutup; Down lagi menyelesaikan; verifies **AC-10**.
- Up A dan Down B serentak pada instance bersama, 50 kali; dua Down serentak (satu stop, atau `left_running` di paling sedikit satu Down, lalu Down ulang menghentikannya); instance `recovering` dengan lease yang disisipkan di antara bacaan awal dan guard tetap mendapat respawn; verifies **AC-5**, **AC-11**.
- Klien putus sesudah `env.down` diterima; `env.get` membaca `lease` sebelum dan sesudah; `leaseActive` di `instance.list` dan di summary event; verifies **AC-12**.
- Stop tanpa override ditolak dengan `items`; dengan override berhenti dan lease tetap; lease yang dicatat di antara precheck dan commit tetap menolak; rotasi sesi `new` sesudah override tidak menyalakan instance; verifies **AC-13**.
- Restart, reload, apply config, rename, dan force stop pada instance ber lease berjalan tanpa field override; start yang dibatalkan pada instance ber lease tetap menghentikan prosesnya; verifies **AC-14**.
- Ganti port tanpa override ditolak, `dryRun` membalas `leases`, permintaan `unchanged` tidak ditolak, dengan override berjalan; verifies **AC-15**.
- Tabel `stop_all::select` dengan `override_leases` true dan false; `expectedInstanceIds` tetap dijaga; `agent_off` dengan dan tanpa override; verifies **AC-16**, **AC-17**.
- CLI: golden manusia dan `--json` untuk `env down` (sukses, tetap jalan, incomplete, changed), `env status` dengan baris lease, dan tiap flag `--override-leases`; binary test membandingkan `result` dengan balasan daemon; verifies **AC-18**, **AC-19**.
- Snapshot GUI terang, gelap, dan sempit: konfirmasi Down dengan ketiga daftar, banner Bringing down, tiap umpan balik, baris Lease tiga keadaan, inspector dengan "· lease", konfirmasi override, sheet port dengan banner, popover Stop All dan Turn Off dengan environment terdampak; verifies **AC-20** sampai **AC-24**.
- Kontrak: fixture terbaca Rust dan Swift, `cargo xtask codegen --check`, klien lama mengabaikan field baru; verifies **AC-25**.
- Native: Up lalu Down lalu Up manifest contoh ERP dari cache xtask; verifies **AC-26**.

### Jawaban Follow-up spec lain

| Asal | Pertanyaan | Jawaban |
| --- | --- | --- |
| Spec 0004 L320, L321 | BR-014 `evaluate_stop(EnvironmentDown)`, BR-015 `evaluate_stop(Manual)` di fitur 47 | dipakai tanpa perubahan, di dalam guard commit stop (AC-5, AC-13) |
| Spec 0029 L84, L437 | ganti port pada instance ber lease aktif | ditolak `instance.endpoints_require_override`, bisa di override dengan dampak tampil dari `dryRun` (AC-15, AC-23) |
| Spec 0031 L71, L90, L100 | lease environment dan override stop milik fitur 46 dan 47 | Down melepas (AC-4); override di AC-13, AC-16, AC-17 |
| Spec 0031 L470 | aturan rotasi sesi dan override di `stop_all` dan `agent_off` | rotasi tidak berubah (spec 0046, dan *Sesudah override* di sini); `overrideLeases` di `stop_all`, preview, dan `agent_off` |
| Spec 0031 AC-10, *Pilihan stop all* aturan 6 | "aturan override milik fitur 47" | aturan 6 berlaku hanya tanpa `overrideLeases` (AC-16) |
| Spec 0032 L96, L490 | dampak lease dan override di preview Stop All | `environments`, `affectedEnvironments`, dan `overrideLeases` (AC-16, AC-24) |
| Spec 0026 L79, L407, L420 (diserahkan spec 0044) | restart pada instance yang dipinjam butuh override atau tidak | tidak butuh; alasannya di K11 rationale (AC-14) |
| Spec 0022 L176 | label "Bring down environment" | AC-21, *Layar* |
| Spec 0007 *Tabel resume* (L458) dan Follow-up L870 | baris `environment_down`, apakah ia pengatur, dan aturan `recover` untuk anak `stop_instance` yang antre | tidak resumable dengan `CloseInterrupted`; pengatur (`is_coordinator()` benar); `recover` menutup anak `stop_instance` yang `queued` dan induknya ditutup `interrupted`, sama dengan aturan anak create dan start (AC-3, AC-10, AC-25, K15 rationale). Down yang antre di belakang Up belum berjalan dan belum memegang slot pengatur, jadi aturan "pengatur tidak menunggu pengatur lain" tetap utuh |
| Spec 0044, titik sambung | lease di `instance_demands`, `env.remove` menolak, `InstanceSummary.environments`, tombol Down, baris Lease, "· lease" | AC-4, aksi Bring Down… di pesan remove, `leaseActive`, AC-20, AC-22 |
| Spec 0046, titik sambung | lepas semua lease ber `source_id`, urutan terbalik dari `dependency_conditions`, nasib `last_up`, antre atau batalkan Up, guard lease, anak `stop_instance` dan `recover` | AC-4, AC-6, `last_up` dikosongkan (K8), Down membatalkan Up (K2), AC-5, AC-10 |

## Build plan

Urutan mengikuti pendekatan Journey: jalur "tinjau apa yang berhenti dan apa yang tetap jalan, Bring Down, lihat hasilnya" dibangun lengkap dari stop ber guard sampai setiap keadaan gagalnya, lalu jalur override, lalu CLI, lalu GUI. Protokol naik satu minor di langkah 4 (nomor bebas berikutnya, ditetapkan lead saat build). Tidak ada migrasi. Semua langkah butuh spec 0046 milestone 1 dan 2 (engine pengatur, `Origin::Child`, `wait_final`, handler yang dipecah, lease dari Up) dan spec 0044 milestone 1 sudah di `main`.

**Milestone 1: Down di daemon** (needs spec 0046 milestone 1 dan 2)

1. Domain dan storage: kebijakan `OperationKind::EnvironmentDown` (tidak resumable, `CloseInterrupted`, pengatur) dan baris tabel resume nya, `Environment::begin_down`, `Registry::leases_of_environment` dan `GuardView::leases_of_environment`, fake registry testkit dan `registry_conformance`; `recover` menutup anak `stop_instance` yang antre dan induknya `interrupted`. Memenuhi **AC-4**, **AC-10**, **AC-25**.
2. Stop ber guard: `LeaseCheck` di `journal_stop` dan `stop_direct`, `persist::commit_stop_decision`, `StopRequest::environment_down`, pemicu `environment_down`, hasil `kept` yang eksplisit, aturan respawn, `EnvironmentHolder` di `SupervisorError::StopRequiresOverride` dan `items` nya, catatan audit `overriddenEnvironments`; test stop, restart, cancel start, dan Stop All yang ada tetap lulus tanpa perubahan harapan. Memenuhi **AC-5**, **AC-13**.
3. Core `environment::down`: `select` dan `read_view` dengan test tabel, `order` dengan unit test (rantai, berlian, `kept`, gagal), `driver` dengan sapuan akhir, port `DownChildren` dengan fake di testkit, pemindahan `ChildOrigin` ke `environment::children`, `limits`. Memenuhi **AC-1**, **AC-5**, **AC-6**, **AC-8**, **AC-9**.
4. Protokol: `env.preview_down`, `env.down`, DTO baru, `EnvLeaseInfo`, `leaseActive`, tambahan `stop_all`, `daemon`, dan `instance`, kode baru dengan hint, fixture `protocols/fixtures/env/`, kenaikan minor, `cargo xtask codegen`, `cargo xtask fuzz seed`. Memenuhi **AC-25**, **AC-12**.
5. `stacklyd`: handler `env.preview_down` dan `env.down` (precheck, cancel Up), adapter `DownChildren` di `ipc/env/children.rs` dengan *Precheck stop anak*, `lease` di `env.get`, `leaseActive` di cache keanggotaan, `set_audit_note`, dan test integrasi `apps/daemon/tests/env_down.rs` dengan provider `fake` (AC-022, serentak 50 kali, Down selagi Up, cancel, restart daemon, klien putus, tidak ada yang terhapus). Memenuhi **AC-1** sampai **AC-12**.

**Milestone 2: override** (after langkah 2 dan 4; bisa paralel dengan langkah 5 kalau file `apps/daemon/src/ipc/env/` tidak disentuh)

6. `stop_all::select` dengan `StopAllOptions`, `overrideLeases` di `instance.stop_all`, `instance.preview_stop_all`, dan `agent_off`, `environments` dan `affectedEnvironments` di preview; `overrideLeases` dan `leases` di `instance.change_endpoints` dengan cek ulang di guard; test tabel dan test socket `stop_all`, `agent_off`, dan `change_endpoints`. Memenuhi **AC-14**, **AC-15**, **AC-16**, **AC-17**.

**Milestone 3: CLI** (after langkah 4 untuk golden; binary test needs langkah 5 dan 6; bisa paralel dengan milestone 4)

7. `stackly env down`, baris `lease:` di `env status`, flag `--override-leases` di `instance stop`, `stop --all`, `endpoint set`, dan `daemon uninstall`, hint penolakan, completion, man page, golden dan binary test. Memenuhi **AC-18**, **AC-19**, **AC-22** (bagian CLI).

**Milestone 4: app** (after langkah 4; needs spec 0044 milestone 3 dan spec 0046 milestone 4 untuk halaman, toolbar, dan sarang Activity; evidence manual needs langkah 5 dan 6)

8. Down di halaman environment: tombol Down, `EnvironmentDownModel`, konfirmasi *Copy Down*, banner "Bringing down…", umpan balik, "Bringing down…" di sidebar, daftar, dan Overview, judul dan label Partial di Activity, aksi Bring Down… di pesan remove, skenario fixture `environment-bringing-down`. Memenuhi **AC-20**, **AC-21**, **AC-25**.
9. Lease dan override: baris Lease, "· lease" di inspector, konfirmasi override untuk semua permukaan Stop, banner dan "Change Anyway" di sheet port, dampak environment di popover Stop All dan konfirmasi Turn Off; snapshot terang, gelap, dan sempit untuk semua layar baru. Memenuhi **AC-22**, **AC-23**, **AC-24**.
10. Evidence dan manual: test native `env_down_native.rs` dari cache xtask (BLOCKED tanpa artefak), record evidence fixture untuk AC-5, AC-7, dan AC-11; langkah manual (GUI nyata, hanya dijalankan kamu) ditulis di `verify.md` bagian `### UI / manual`: Up dua environment yang berbagi satu instance, Down yang pertama dan baca konfirmasinya, tutup app di tengah Down lalu buka lagi, Stop instance ber lease dan baca dialognya, Stop All Services dari menu bar, ganti port instance ber lease. Memenuhi **AC-26**, **AC-20**, **AC-23**, **AC-24**.

**Sesi paralel yang aman**: sesudah langkah 4 masuk, CLI (langkah 7, `apps/cli/`) dan GUI (langkah 8 sampai 10, `apps/macos/`) tidak beririsan file dengan daemon. `crates/protocol/`, `protocols/`, dan hasil codegen hanya disentuh langkah 4, oleh satu sesi, dan tidak bersamaan dengan langkah protokol spec 0044, 0045, atau 0046. Langkah 2 dan 6 menyentuh `crates/core/src/supervisor/stop.rs` dan `crates/core/src/instance/stop_all.rs`, jadi keduanya tidak berjalan bersamaan dengan langkah 3 spec 0046 (yang menyentuh `supervisor/startup.rs` dan handler stop yang dipecah).

## Consequences

**Positive**:

- Down tidak membawa aturan stop kedua. Stop normal, timeout, `last_exit`, dan audit instance yang dihentikan Down sama persis dengan yang dihentikan tombol Stop.
- Keputusan "masih dibutuhkan atau tidak" diambil di satu tempat (`evaluate_stop`) dan di dalam transaksi yang menulis desired, jadi tidak ada urutan Up dan Down yang membuat instance berhenti selagi dibutuhkan lalu dibiarkan mati.
- Kamu selalu melihat apa yang berhenti dan apa yang tetap jalan sebelum Down, dan Down tidak pernah menghentikan lebih dari yang kamu lihat.
- Lease dilepas dalam satu commit, jadi sesudah Down berakhir `env.remove` tidak lagi ditolak lease, juga kalau stop nya gagal.
- Down boleh diulang kapan saja; mengulang adalah satu satunya langkah recovery.
- Stop pengguna ikut mendapat pemeriksaan lease di dalam transaksi, yang menutup celah lama antara precheck dan commit.

**Negative / tradeoffs**:

- Down hanya melihat lease, bukan dependency. Service yang kamu nyalakan sendiri tetap jalan walau dependency nya di environment yang sama ikut berhenti; konfirmasi menyebut keduanya, tetapi tidak memperingatkan hubungan itu.
- Lease dilepas sebelum stop. Kalau stop gagal atau daemon berhenti di tengah, ada instance yang masih jalan tanpa lease sampai kamu menjalankan Down lagi atau menghentikannya sendiri; daemon yang start lagi di sesi login yang sama bahkan menyalakannya lagi, karena desired nya masih `running`.
- Down tidak pernah menghentikan instance di luar himpunan yang dikonfirmasi. Dua Down serentak pada environment yang berbagi instance, atau instance yang sempat menyala di antara preview dan stop, karena itu berakhir `env.down_incomplete` dengan `left_running`, dan kamu perlu menjalankan Down sekali lagi.
- Down membatalkan Up yang sedang berjalan tanpa bertanya lagi selain satu kalimat di konfirmasi. Create yang sedang berjalan ikut dikompensasi (spec 0046 AC-11).
- Jalur stop yang stabil berubah: `journal_stop` dan `stop_direct` mendapat `LeaseCheck`, commit desired menjadi commit ber guard untuk stop klien, langkah Stop All, dan Down, dan urutan terhadap `cancel_respawn` harus dijaga. Review langkah 2 perlu teliti, dan test stop, restart, cancel start, dan Stop All yang ada harus lulus tanpa perubahan harapan.
- Override tidak melepas lease, jadi environment tampil tidak lengkap sampai Up lagi atau Down, dan `env.remove` tetap menolak. Itu disengaja, tetapi bisa mengejutkan sesudah Stop All.
- Environment lain yang mengambil lease selagi dialog override terbuka ikut di override tanpa tampil di dialog itu. Akibatnya tetap terlihat (status environment dan catatan audit), tetapi dialog tidak memuat ulang.
- Restart, reload, apply config, dan rename tidak dijaga lease. Rename instance yang dipinjam lewat `instanceRef` membuat plan environment peminjam menunjuk nama lama (lihat Follow-up).
- `stackly instance stop --all` dan `stackly daemon uninstall` tanpa flag tetap melewati instance ber lease, sedangkan app meng override sesudah konfirmasi. Dua bawaan itu berbeda dengan sengaja: CLI tanpa terminal tidak punya tempat untuk menampilkan dampak.
- Satu Down menghasilkan satu baris operasi dan audit per instance yang dihentikan, ditambah induknya.

**Neutral**:

- Tidak ada tabel, kolom, migrasi, atau `EventKind` baru.
- Down yang tidak lengkap berakhir `failed`, sama seperti Up; label "Partial" di Activity adalah tampilan untuk kode `env.down_incomplete`.
- `env.down` selalu membuat operasi, juga saat tidak ada yang perlu dihentikan (ia tetap melepas lease dan mengosongkan `last_up`).
- Member dan snapshot tetap ada sesudah Down, jadi halaman environment tetap menampilkan service dan instance nya dengan status `down`.
- Baris lease `released` menumpuk sampai dipangkas rotasi sesi atau `env.remove`; jumlahnya paling banyak 32 per siklus Up dan Down.

### Update spec lain

Usulan untuk lead (architect spec ini tidak mengubah spec lain):

- **Spec 0007**: baris tabel resume `environment_down` menjadi "tidak, kebijakan `CloseInterrupted`, pengatur"; `recover` juga menutup anak `stop_instance` yang antre dan induknya `interrupted`.
- **Spec 0004 dan 0005**: `Environment::begin_down`, `Registry::leases_of_environment`, `GuardView::leases_of_environment`; baris BR-014 dan BR-015 menunjuk AC-5 dan AC-13 spec ini.
- **Spec 0010, 0026, dan 0030**: `LeaseCheck` di `journal_stop` dan `stop_direct` menggantikan `StopIntent::checks_leases()`, pemicu stop `environment_down`, dan pemeriksaan lease stop pengguna di dalam guard commit desired; spec 0026 *Consequences* "restart tidak meminta override" dikukuhkan (AC-14), centang Follow-up L420.
- **Spec 0029**: `overrideLeases` dan `EndpointValidation.leases` di `instance.change_endpoints`, kode `instance.endpoints_require_override`, banner dan tombol di sheet port; centang Follow-up L437.
- **Spec 0031**: `StopAllOptions`, aturan 6 bersyarat, `overrideLeases` di `instance.stop_all`, `instance.preview_stop_all` (kini punya params), dan `agent_off`; konfirmasi Turn Off; baris `environment` di *Model demand* menunjuk spec 0046 dan spec ini; centang Follow-up L470.
- **Spec 0032**: `environments` di `StopAllItem` dan `StopAllSkip`, `affectedEnvironments`, baris *Copy Stop All* baru, popover memakai `overrideLeases` true; centang Follow-up L490.
- **Spec 0044**: `InstanceEnvironment.leaseActive`; `EnvironmentDetail.lease`; baris Lease dan "· lease"; aksi Bring Down… di pesan `env.lease_active`.
- **Spec 0046**: `ChildOrigin` pindah ke `environment::children`; `activeOperation.kind` `environment_down`; `last_up` dikosongkan Down; centang Follow-up baris spec 0047.
- **Spec 0008, 0022, 0035**: perintah `env down` dan flag `--override-leases` di tabel perintah; judul operasi; catatan audit `overriddenEnvironments`.
- **`apps/macos/design.md`**: teks status "Bringing down…".

## Follow-up

- [ ] Lead: tetapkan nomor minor protokol saat build, dan terapkan *Update spec lain* lewat `architect (update)`.
- [ ] Spec 0044: rename instance yang dipinjam lewat `instanceRef` membuat plan environment peminjam melaporkan `env.instance_ref_not_found` walau member nya masih terikat lewat ID. Putuskan di sana apakah plan mengikuti member, atau rename meminta override seperti ganti port.
- [ ] Fitur hapus instance: pesan dan alur untuk instance yang masih member environment (FK `restrict` spec 0005), termasuk tawaran Bring Down.
- [ ] Peringatan dependency di konfirmasi Down ("{service} keeps running but depends on {dependency}, which stops") belum ada. Tambahkan kalau pengguna sering tersandung.
- [ ] Down latar belakang saat `agent_off` (melepas lease, bukan meng override) bukan bagian fitur ini; pertimbangkan lewat `/scope` kalau dibutuhkan.
- [ ] `/sync` setelah build: catat di root `AGENTS.md` bahwa Down adalah operasi pengatur yang hanya melepas lease dan memulai anak `stop_instance`, bahwa `down::select` adalah satu satunya aturan pilihan Down, bahwa klien mengirim `expectedStopInstanceIds` dari `env.preview_down`, bahwa override tidak pernah melepas lease, dan perintah `stackly env down`.

## Rationale

Alasan dan opsi yang ditimbang: lihat [rationale.md](rationale.md).
