# 0029. Edit config, nama, dan endpoint

**Date**: 2026-11-02
**Status**: In Progress

## Summary

Spec ini menetapkan cara kamu mengubah instance yang sudah ada: settings provider lewat halaman Configuration, nama lewat Rename, dan port lewat Endpoints. Setiap perubahan settings atau port menjadi config revision baru bernomor di registry, divalidasi dulu dengan aturan yang sama seperti create, lalu diterapkan dalam satu operasi daemon yang langsung me reload atau me restart instance yang sedang jalan, atau hanya disimpan untuk start berikutnya kalau instance berhenti. Revision yang pernah membuat instance ready menjadi last known good (config terakhir yang terbukti sehat), dan kamu bisa kembali ke sana dengan satu aksi kalau config baru gagal. File config yang diubah di luar Stackly terdeteksi sebagai drift (selisih antara file di disk dan yang terakhir ditulis Stackly), dan Stackly menolak start, reload, atau apply sampai kamu memilih Keep Revision.

## Requirements

**User stories**:

- Sebagai developer, saya ingin mengubah `max_connections` PostgreSQL dari form, melihat diff dan dampaknya (reload atau restart) sebelum Apply, supaya saya tidak me restart database tanpa sengaja.
- Sebagai developer, saya ingin config yang tidak valid ditolak dengan error di field yang salah tanpa menyentuh config aktif dan instance yang sehat.
- Sebagai developer, saya ingin kembali ke config terakhir yang terbukti sehat dengan satu aksi kalau instance gagal start setelah perubahan saya.
- Sebagai developer, saya ingin tahu kalau seseorang mengedit `postgresql.conf` langsung, dan Stackly tidak menimpanya diam diam.
- Sebagai developer, saya ingin mengganti nama instance tanpa memindahkan data, dan mengganti port dengan pemeriksaan konflik yang sama seperti create.
- Sebagai penulis script, saya ingin `stackly instance config get`, `validate`, dan `apply` dengan `--json` yang memberi kode error dan field error yang sama dengan GUI.
- Sebagai penulis provider, saya ingin halaman Configuration muncul dari settings schema saja (termasuk daftar `routes` Traefik), tanpa kode SwiftUI khusus provider.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-015):

- **AC-1** (baca config, REQ-015, PAGE-06): `instance.config.get` mengembalikan, untuk satu instance, field form dari settings schema paket pinned dengan nilai revision aktif, kelas tiap field (`reload`, `restart`, `immutableAfterInit`, `managed`, `secret`, `advanced`), nomor revision aktif, nomor last known good (atau kosong), histori (paling banyak 20 baris), endpoint revision aktif, status drift, dan Effective File (isi setiap file config yang tidak sensitif). Method ini tidak membaca catalog, jaringan, atau Keychain.
- **AC-2** (validasi tanpa efek, AC-015, VAL-012): `instance.config.validate` dengan `baseRevision` dan nilai settings baru mengembalikan field error dengan path `settings.<key>` (dan path item daftar seperti `settings.routes[2].rule`) memakai `FieldCode` wire yang sama dengan create (`create/evaluate.rs::field_code`), ditambah dua varian baru `immutableAfterInit` untuk field `apply: immutableAfterInit` yang berubah dan `secretReadOnly` untuk field secret yang berubah (field yang secret sekaligus set once mendapat `secretReadOnly`). Balasan yang valid memuat diff per setting (nilai lama dan baru), dampak (`none`, `next_start`, `reload`, `restart`), dan pratinjau Effective File dengan baris yang berubah ditandai. Validasi tidak menulis registry, file, atau journal operasi.
- **AC-3** (apply yang tidak valid, AC-015): `instance.config.apply` dengan nilai yang tidak lolos AC-2 ditolak `instance.settings_invalid` dengan field error yang sama, tanpa operasi, tanpa revision baru, tanpa file yang ditulis, dan tanpa stop, reload, atau restart. Revision aktif dan proses yang sehat tidak berubah.
- **AC-4** (apply dengan reload): pada instance `running` (readiness apa pun) dari provider dengan capability `reload`, kalau setiap field yang berubah `apply: reload`, satu operasi `apply_config` menulis revision baru (nomor naik satu, `previous_id` ke revision lama, revision lama `superseded`, revision instance naik satu), lalu menjalankan langkah reload spec 0026 di bawah operasi yang sama: PID tidak berubah dan readiness diverifikasi. Dibuktikan pada PostgreSQL 18 nyata.
- **AC-5** (apply dengan restart, AC-026): pada instance `running` atau `recovering`, kalau ada field `apply: restart` yang berubah, atau provider tidak punya capability `reload`, operasi `apply_config` menulis revision lalu menjalankan langkah restart spec 0026 di bawah operasi yang sama, dengan PID baru. Provider tanpa reload tidak pernah mendapat langkah reload dan tidak pernah menampilkan keberhasilan reload. Dibuktikan pada NATS nyata.
- **AC-6** (apply per lifecycle): `stopped`, `failed`, dan `uninitialized` hanya menyimpan revision (dampak `next_start`); start berikutnya merender dari revision itu. `starting` ditolak `supervisor.instance_not_running` (sama dengan restart spec 0026 AC-7; hint CLI dan teks GUI menyarankan menunggu sampai start selesai) dan `stopping` ditolak `supervisor.instance_stopping`, `unknown` ditolak `supervisor.identity_unproven`, `initializing` dan instance retained dengan kode spec 0010, semuanya tanpa operasi.
- **AC-7** (last known good): `config_revisions.ready_at` diisi pada saat pertama instance mencapai ready di atas revision itu, lewat jalur start apa pun (start pengguna, restart, apply, respawn sistem, reload yang terverifikasi ready, dan ready rutin setelah not ready), tetapi hanya kalau proses yang ready dijalankan atau di reload setelah revision itu aktif (*Last known good*). Proses yang diadopsi setelah daemon mati di antara commit dan restart tidak pernah memberi `ready_at` ke revision baru. Last known good adalah revision instance itu dengan `ready_at` terisi dan nomor tertinggi.
- **AC-8** (config valid yang gagal, roll back): reload yang gagal, restart yang gagal, atau instance yang tidak ready sampai batas readiness setelah apply membuat operasi gagal dengan kode spec 0026 atau spec 0010 yang sama, dan revision baru tetap aktif dengan `ready_at` kosong. Selama revision aktif belum pernah ready dan ada last known good yang lain, GUI menampilkan banner roll back dan CLI menerima `stackly instance config rollback`. Roll back menulis revision baru berisi settings dan endpoint last known good, lalu mengikuti aturan dampak AC-4 sampai AC-6. Stackly tidak pernah roll back sendiri.
- **AC-9** (serialisasi dan konflik): apply, rollback, change endpoints, dan rename memakai `BusyPolicy::Queue`. Apply membawa `baseRevision` (nomor revision yang dilihat client); nomor yang tidak sama dengan revision aktif, di precheck maupun setelah antre, ditolak `config.revision_conflict` dengan nomor aktif di detail. Apply tanpa perubahan apa pun dijawab `unchanged` tanpa operasi dan tanpa revision.
- **AC-10** (deteksi drift, REQ-015): setiap render config (init, start, restart, reload, apply) juga menulis salinan setiap file tidak sensitif yang direndernya ke `config-history/<revision-id>/`, per file: file yang tidak dirender di render itu (misalnya di luar `lifecycle.reload.render` saat reload) tetap memakai salinan lamanya. Drift adalah file di `config/` yang isinya berbeda dari salinan itu, atau hilang. Pemeriksaan berjalan saat `instance.config.get`, tepat sebelum setiap render, dan sekali untuk setiap instance saat daemon start. Drift yang ditemukan memasang `InstanceBlocker::ConfigDrift` dengan daftar file dan waktu pemeriksaan; `instance.config.get` membawa diff per file (isi salinan lawan isi disk).
- **AC-11** (drift memblokir, menunggu pilihan): selama drift terpasang, start, restart, reload, apply, rollback, dan change endpoints ditolak `config.drift_detected` tanpa operasi dan tanpa menulis file. `instance.config.keep_revision` menjalankan operasi `resolve_config_drift` yang menulis ulang file dari salinan revision aktif lalu menghapus blocker. Stackly tidak pernah menimpa file yang drift tanpa aksi itu. Instance yang belum punya salinan (dibuat sebelum fitur ini) tidak pernah dilaporkan drift sampai render pertama menulis salinannya.
- **AC-12** (histori): registry menyimpan 20 revision terbaru per instance; revision aktif dan last known good tidak pernah dihapus, dan folder salinannya ikut dihapus bersama barisnya. Nomor revision tidak pernah dipakai ulang. Restore di History memuat settings dan endpoint baris itu ke form sebagai perubahan tertunda, lalu Apply berjalan seperti biasa.
- **AC-13** (rename, BR-003): `instance.rename` mengganti `name` lewat operasi `rename_instance` pada lifecycle apa pun kecuali `initializing` dan retained, dengan aturan VAL-001 dan keunikan yang dicek ulang di dalam `commit_guarded` (`instance.name_invalid`, `instance.name_taken`). ID, runtime, secret reference, path data, `display_name`, proses, dan config revision tidak berubah. Nama yang sama dengan nama sekarang dijawab `unchanged` tanpa operasi.
- **AC-14** (ganti endpoint, REQ-010, AC-055, AC-056): `instance.change_endpoints` mengubah endpoint IP menjadi `Fixed(port)` atau `Auto` lewat operasi `change_endpoints`, dengan pemeriksaan create spec 0020 (rentang port, konflik di antara endpoint sendiri, konflik dengan instance lain di dalam `commit_guarded`, dan probe bind per tuple dengan wildcard dan IPv4 atau IPv6). Port yang sama dengan `bound_port` instance itu sendiri saat ini melewati probe bind tetapi tetap dicek terhadap instance lain. Port fixed yang konflik tidak pernah diganti otomatis; balasan error membawa saran port bebas. Operasi menulis revision baru (settings sama, endpoint baru) dan `instance_endpoints` dalam satu transaksi, lalu me restart instance `running` atau `recovering`, atau menyimpan untuk start berikutnya. Endpoint Unix dan host tidak bisa diubah.
- **AC-15** (kelas field di form, VAL-012): field `managed` tampil terkunci dengan "Set by Stackly", port tampil sebagai baris terkunci dengan "Change in Endpoints", field `apply: immutableAfterInit` terkunci dengan badge "Set once", dan field secret tampil sebagai reference read only. Setiap field yang bisa diedit menampilkan label dampaknya ("Applies on reload" atau "Requires restart") dan nilai default nya.
- **AC-16** (schema driven, RISK-07): form Configuration memakai `SchemaFormModel` dan `SchemaFormSections` spec 0020 tanpa cabang per provider. Form, apply, dan last known good diuji pada PostgreSQL, NATS, dan Traefik, termasuk `settings.routes` Traefik sebagai daftar dengan `name` unik, tanpa patch SwiftUI khusus provider.
- **AC-17** (CLI, REQ-028, AC-028): `stackly instance config get|validate|apply|rollback|keep-revision`, `stackly instance rename`, dan `stackly instance endpoint set` menerima target lewat `TargetArg`, `--json`, dan kode error serta exit code dari kategori `stackly-protocol`. `config apply` menerima `--set`, `--set-json`, `--unset`, `--from-revision N`, `--expect-revision N`, `--dry-run` (sama dengan `validate`), `--wait`, `--timeout`, dan `--idempotency-key`. `rollback` dan `keep-revision` meminta konfirmasi di terminal dan butuh `--yes` tanpa terminal atau dengan `--json`. Hasilnya setara dengan GUI.
- **AC-18** (tab Configuration, mockup UI): tab Configuration menampilkan segmented Form dan Effective File, "Revision N", tag dampak ("In sync", "Applies on reload", "Requires restart", "Applies at next start"), tombol Revert (membuang perubahan tertunda), tombol Apply and Reload, Apply and Restart, atau Apply menurut dampak, Pending Changes (baris `key = lama` dan `key = baru`), History (nomor, asal, waktu, badge last known good, Restore), banner drift dengan Keep Revision N dan Show Diff, dan banner roll back. Tombol apply nonaktif saat tidak ada perubahan, ada error, daemon offline, atau drift.
- **AC-19** (rename, endpoint, dan Overview di GUI): item Rename… di menu More detail, menu Instance, dan menu konteks baris Instances membuka sheet dengan pemeriksaan nama langsung. Tombol Edit di kartu Endpoints tab Summary membuka sheet port dengan `EndpointPortRow` spec 0020. Overview menampilkan item perhatian drift dari `InstanceBlocker::ConfigDrift` yang membuka tab Configuration. Setiap operasi yang selesai atau gagal menampilkan kartu umpan balik spec 0067, dan selama berjalan tombolnya memakai label sibuk ("Applying…", "Renaming…", "Changing Ports…").
- **AC-20** (audit, AC-045): journal operasi `apply_config`, `change_endpoints`, `rename_instance`, dan `resolve_config_drift` mencatat correlation ID, hasil, nomor revision lama dan baru, nama setting yang berubah, dan port lama dan baru, tanpa nilai secret (field secret hanya sebagai reference).
- **AC-21** (offline, REQ-046): get, validate, apply, rollback, keep revision, rename, dan change endpoints untuk instance dengan paket pinned dan runtime yang sudah ada berhasil dengan nol akses `CatalogState`, nol panggilan `Fetcher`, dan nol panggilan `CodeSigning::assess`, dengan harness offline spec 0021 AC-10.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md), dipilih user pada 2026-11-02: Effective File read only; apply langsung reload atau restart instance yang jalan dan hanya menyimpan untuk yang berhenti, sebagai satu operasi `apply_config` yang memakai ulang langkah spec 0026; roll back ke last known good sebagai aksi eksplisit, tidak otomatis; last known good dari kolom `ready_at`; revision bernomor yang juga menyimpan snapshot endpoint; histori 20 revision dengan Restore dari baris mana pun; drift dicek saat dibaca dan sebelum render (plus sekali saat daemon start), memasang blocker yang menahan start sampai Keep Revision; rename hanya `name` di lifecycle apa pun; ganti endpoint hanya port dan mode Fixed atau Automatic, sebagai operasi `change_endpoints` dengan sheet dari kartu Endpoints; secret read only.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

## Rationale

Alasan dan opsi yang ditimbang: lihat [rationale.md](rationale.md).

## Feature design

### Trace PRD

| ID PRD | Di spec ini |
|---|---|
| REQ-015, PAGE-06 | AC-1, AC-2, AC-10 sampai AC-12, AC-18 |
| REQ-010, AC-055, AC-056 | AC-14 |
| REQ-028, AC-028 | AC-17 |
| VAL-012 | AC-2, AC-15 |
| BR-003 | AC-13 |
| BR-017 | tidak berubah: revision baru hanya dari aksi pengguna; update provider revision milik fitur 59 |
| AC-015 | AC-2, AC-3 |
| AC-026 | AC-5 |
| AC-045 | AC-20 |
| REQ-046 | AC-21 |
| RISK-07 | AC-16 |

### Batas dengan spec lain

| Hal | Pemilik |
|---|---|
| Settings schema, kelas VAL-012, `Resolver`, `run_plan`, `reload_plan`, flag `sensitive` di `FileWrite` | spec 0009 (dipakai ulang) |
| Langkah restart dan reload, kode lifecycle restart dan reload | spec 0026 (`restart_op`, `reload_work`) |
| Startup transaction, render sebelum spawn, `write_config` | spec 0010 (spec ini menambah pemeriksaan drift dan salinan histori) |
| Evaluator create, pemeriksaan port, `SchemaFormModel`, `EndpointPortRow`, kode field error | spec 0020 |
| `commit_guarded` | spec 0020 Keputusan 21, spec 0007 |
| `config_revisions`, `instance_endpoints`, `instance_paths` kind `config_history` | spec 0005 (spec ini melengkapi, sesuai catatan "fitur 29 melengkapi") |
| Lease environment saat ganti port | fitur 47 |
| Update provider revision dengan diff (BR-017) | fitur 59 |
| Rotasi secret | di luar fase ini; field secret read only di spec ini |
| Metrik dan cadence health | fitur 28 |

### Alur

```
Form (client) ──edit──▶ perubahan tertunda ──instance.config.validate──▶ diff, dampak, pratinjau, field error
              ──Apply──▶ instance.config.apply(baseRevision, settings)
daemon: replay ─▶ precheck (lifecycle, drift, baseRevision, validasi) ─▶ operasi apply_config
  langkah: validate ─▶ lock (Queue) ─▶ recheck ─▶ commit (commit_guarded: revision baru + pointer + revision instance)
           ─▶ salah satu: simpan saja (next_start) │ langkah reload spec 0026 │ langkah restart spec 0026
           ─▶ ready pertama: ready_at diisi
```

`instance.change_endpoints` dan rollback mengikuti alur yang sama; rename hanya validate, lock, commit.

### Model data

Satu migrasi registry dengan nomor bebas berikutnya saat build (spec 0028 dan 0031 sama sama mengusulkan `0007`, jadi nomor final ditentukan urutan merge), nama `NNNN_config_revision_history.sql`:

| Tabel | Kolom | Tipe | Null | Aturan |
|---|---|---|---|---|
| `config_revisions` | `number` | `INTEGER` | tidak | mulai 1, unik `(instance_id, number)`, backfill 1 untuk baris yang sudah ada (setiap instance tepat punya satu revision saat migrasi) |
| `config_revisions` | `endpoints` | `TEXT` (json object) | tidak | snapshot `{ "<nama endpoint>": { "type": "fixed", "content": 5433 } \| { "type": "auto" } }` untuk endpoint IP; CHECK `json_valid` dan diawali `{`; backfill dari `instance_endpoints.fixed_port` (null berarti `auto`) |
| `config_revisions` | `ready_at` | `TEXT` (ts UTC) | ya | diisi sekali, saat ready pertama di revision itu; backfill null |
| `config_revisions` | `status` | `code` | tidak | set terbuka yang sudah ada; nilai yang dipakai: `active`, `superseded` |
| `config_revisions` | `origin` | `code` | tidak | set terbuka: `created`, `applied`, `endpoints`, `rollback`, `restored`; backfill `created` |
| `config_revisions` | `source_number` | `INTEGER` | ya | nomor revision sumber untuk `rollback` dan `restored`, selain itu null; tanpa FK karena sumber bisa sudah di prune |
| `config_revisions` | `rendered_sha256` | `sha256` | ya | tidak berubah; spec ini tidak menulis atau membacanya |

- Migrasi tidak membuat baris untuk instance tanpa revision (create yang gagal setelah `InsertInstance`, atau retained); `instance.config.get` pada instance itu ditolak `instance.provider_unavailable` dengan detail `reason: no_config_revision`.
- Tidak ada tabel baru. `instances.config_revision_id` dan `instances.revision` sudah ada. `instance_endpoints` tetap bagian aggregate `Instance`: endpoint berubah lewat aggregate (method domain baru `Instance::set_port_request(name, PortRequest)`, yang juga mengosongkan `bound_port` endpoint itu dan menaikkan revision), tidak pernah lewat tulisan kolom langsung.
- Relasi tidak berubah: instance 1:N `config_revisions` (cascade), revision N:1 `previous_id` (set null), instance 1:1 revision aktif (FK komposit deferred).
- Domain (`stackly-domain`, modul `registry`): `ConfigRevisionRecord` mendapat `number: ConfigRevisionNumber` (newtype `u64`), `endpoints: EndpointRequests` (peta nama ke `PortRequest`), dan `ready_at: Option<Timestamp>`. `ConfigRevisionStatus` mendapat `Superseded`.
- `WriteOp` baru: `ActivateConfigRevision { instance: Instance, expected: ResourceRevision, record }`, yaitu `UpdateInstance` plus baris revision dalam satu op: aggregate yang sudah diubah (`set_port_request` kalau endpoint berubah, selain itu hanya revision naik) ditulis dengan pemeriksaan `expected` yang sama dengan `UpdateInstance`, lalu revision di insert dengan nomor aktif + 1, `previous` = aktif, aktif lama jadi `superseded`, dan pointer pindah. Op menolak `record.endpoints` yang tidak sama dengan permintaan port di aggregate (`Integrity`). `MarkConfigRevisionReady { revision, at }` (hanya kalau `ready_at` masih null; idempoten), dan `PruneConfigRevisions { instance, keep: 20 }` (hapus yang tertua di luar 20 terbaru, tidak pernah aktif atau last known good). Histori selalu diurutkan menurut `number`, bukan dengan mengikuti `previous_id` (yang menjadi null setelah prune). Aturan `InsertConfigRevision` yang menolak revision kedua tetap untuk create.
- `Registry::config_revisions(InstanceId, limit)` dan `Registry::last_known_good(InstanceId)`.
- File: `instances/<id>/config-history/<revision-id>/<path relatif config>` berisi salinan file tidak sensitif dari render terakhir revision itu. Ditulis atomik sebelum file di `config/`, dihapus bersama baris revision saat prune.

### State transitions

Lifecycle instance tidak bertambah. Revision punya state sendiri:

```
(baru) ──ActivateConfigRevision──▶ active ──revision berikutnya aktif──▶ superseded ──prune──▶ (dihapus)
active ──ready pertama──▶ active + ready_at        (last known good = nomor tertinggi dengan ready_at)
```

Dampak apply menurut lifecycle saat pekerjaan berjalan (setelah antre):

| Lifecycle | Field berubah | Langkah setelah commit |
|---|---|---|
| `running` | semua `reload`, provider punya `reload` | reload spec 0026 |
| `running` | ada `restart`, atau provider tanpa `reload`, atau endpoint berubah | restart spec 0026 |
| `recovering` | apa pun | restart spec 0026 (sub kasus `recovering`) |
| `stopped`, `failed`, `uninitialized` | apa pun | tidak ada (`next_start`) |
| `starting`, `stopping`, `unknown`, `initializing`, retained | apa pun | ditolak, kode AC-6 |

Gerbang lifecycle apply adalah fungsi baru `apply_allowed(lifecycle) -> ApplyGate` (`Save`, `Reload`, `Restart`, atau penolakan dengan kode AC-6), bukan `restart_allowed` spec 0026, yang memetakan `stopped` ke penolakan. Rename punya gerbangnya sendiri, `rename_allowed` (semua kecuali `initializing` dan retained).

Interrupt policy: `apply_config` dan `change_endpoints` memakai `InterruptPolicy::CloseInterrupted` (seperti `restart_instance`, karena commit tidak bisa dibatalkan); `rename_instance` dan `resolve_config_drift` memakai `Compensate`. Test domain yang memeriksa himpunan kind `CloseInterrupted` diperbarui.

Daemon mati di tengah apply: sebelum commit tidak ada perubahan; setelah commit revision tetap aktif dan operasi ditutup `interrupted`. Separuh restart dipulihkan rekonsiliasi spec 0010 dan spec 0026 dari desired state. Kalau daemon mati setelah commit tetapi sebelum reload atau restart mulai, proses lama diadopsi dan masih memakai revision sebelumnya: revision baru tidak mendapat `ready_at` dari proses itu (*Last known good*), dan tag dampak menampilkan "Requires restart" sampai proses dijalankan ulang.

### Method IPC

Baris baru di tabel `methods!` (`crates/protocol/src/message.rs`). Method baca `recovery allowed`, method yang mengubah `recovery refused`. Setiap method yang mengubah memanggil `ipc::operation::replay_operation`, lalu precheck, lalu `ipc::operation::start_operation`.

| Method | Params | Balasan | Operasi | Error penting |
|---|---|---|---|---|
| `instance.config.get` | `instanceId` | `InstanceConfigView` | tidak | `resource.not_found`, `instance.provider_unavailable` (paket pinned hilang) |
| `instance.config.validate` | `instanceId`, `baseRevision`, `settings` | `ConfigValidation` | tidak | `config.revision_conflict`, `instance.settings_invalid` (field error) |
| `instance.config.apply` | `instanceId`, `baseRevision`, `settings` atau `fromRevision`, `rollback: bool`, `expectedRevision` opsional, `idempotencyKey` | `ConfigApplyAccepted` (`operationId`, `revision`, `impact`) atau `unchanged` | `apply_config` | `instance.settings_invalid`, `config.revision_conflict`, `config.drift_detected`, `config.no_last_known_good` (rollback tanpa LKG lain), kode lifecycle AC-6; dari operasi: kode reload dan restart spec 0026 |
| `instance.config.keep_revision` | `instanceId`, `idempotencyKey` | `InstanceActionAccepted` (`alreadyInState` tanpa drift) | `resolve_config_drift` | `resource.busy` |
| `instance.rename` | `instanceId`, `name`, `expectedRevision` opsional, `idempotencyKey` | `InstanceActionAccepted` atau `unchanged` | `rename_instance` | `instance.name_invalid`, `instance.name_taken` |
| `instance.change_endpoints` | `instanceId`, `baseRevision`, `endpoints: [{name, port: fixed(u16) \| auto}]`, `dryRun: bool`, `idempotencyKey` | `ConfigApplyAccepted` atau `EndpointValidation` (dry run) | `change_endpoints` | `instance.endpoint_invalid` (field error per endpoint, dengan saran port), kode port spec 0020, `config.revision_conflict`, `config.drift_detected`, kode lifecycle AC-6 |

DTO (`stackly-protocol`, minor bump ke nomor berikutnya yang bebas saat build; kini 1.18 dan spec 0028 mengusulkan 1.19):

| Tipe | Isi |
|---|---|
| `InstanceConfigView` | `revision: ConfigRevisionSummary`, `lastKnownGood: Option<ConfigRevisionNumber>`, `fields: [ConfigField]`, `values` (object JSON, secret sebagai teks reference), `endpoints: [ConfigEndpoint]`, `history: [ConfigRevisionSummary]`, `effectiveFiles: [EffectiveFile]`, `drift: Option<ConfigDrift>`, `reloadSupported: bool` |
| `ConfigField` | `FormField` spec 0020 ditambah `apply: ConfigApplyClass` (`reload`, `restart`, `immutableAfterInit`, `Unrecognized`) dan `locked: bool` |
| `ConfigRevisionSummary` | `number`, `createdAt`, `origin` (`created`, `applied`, `endpoints`, `rollback`, `restored`, `Unrecognized`), `readyAt` opsional, `isActive`, `isLastKnownGood` |
| `EffectiveFile` | `path` (relatif `config/`), `content`, `changedLines: [u32]` (hanya di pratinjau), `preview: bool`, `portNote` opsional ("Automatic ports shown as {port}") |
| `ConfigDrift` | `observedAt`, `files: [DriftFile { path, kind: modified \| missing, diff: [DiffLine] }]` |
| `ConfigValidation` | `fieldErrors`, `changes: [SettingChange { key, old, new }]`, `impact: ConfigImpact` (`none`, `nextStart`, `reload`, `restart`, `Unrecognized`), `preview: [EffectiveFile]` |
| `ConfigApplyAccepted` | `operationId`, `revision: ConfigRevisionNumber`, `impact` |
| `InstanceBlocker` | varian baru `ConfigDrift(ConfigDriftBlocker { files: [String], observedAt })` |
| `OperationKind` | varian baru `ApplyConfig` (`apply_config`), `ChangeEndpoints` (`change_endpoints`), `ResolveConfigDrift` (`resolve_config_drift`), juga di `stackly_domain::OperationKind` |

Setiap params mengimplementasikan `Validate`; `ConfigRevisionNumber` memakai newtype `u64` di `wire.rs`; fixture kontrak baru masuk `protocols/fixtures/` (`config-view-fake.json`, `config-view-postgresql.json`, `config-view-traefik.json`, `config-validation-restart.json`, `config-drift.json`).

Kode error baru (`crates/protocol/src/codes.rs`):

| Kode | Kategori | Retryable | Kapan |
|---|---|---|---|
| `config.revision_conflict` | Conflict | false | `baseRevision` bukan revision aktif; detail `activeRevision` |
| `config.drift_detected` | Conflict | false | blocker `ConfigDrift` terpasang; detail `files` |
| `config.no_last_known_good` | Conflict | false | rollback tanpa last known good yang berbeda dari revision aktif |

`FieldCode` (`crates/protocol/src/json.rs`, kini delapan varian tertutup) mendapat `immutableAfterInit` dan `secretReadOnly`, dengan teks GUI di tabel *Field error* spec 0020 dan fixture kontrak. Di core, `evaluate_change` menghasilkan dua kasus baru yang dipetakan ke varian itu oleh `field_code`; `SettingsIssueKind` sendiri tidak berubah karena keduanya bukan hasil `SettingsSchema::validate`.

### Evaluator config

Modul baru `crates/core/src/instance/config/` dengan fungsi murni yang dipakai `get`, `validate`, dan precheck apply supaya ketiganya selalu sama (pola evaluator create spec 0020):

- `gather_config(instance)`: membaca instance, revision aktif, histori, last known good dari registry, dan paket pinned lewat `load_package`, lalu `prepare(.., Checks::Plan)` untuk pratinjau. Tidak ada catalog. Folder runtime yang hilang tidak menggagalkan `get`: Effective File tanpa salinan dikosongkan dengan catatan "Runtime missing, preview unavailable", dan `validate` tetap memeriksa nilai tanpa pratinjau.
- `evaluate_change(schema, active, proposed) -> Evaluation`: `SettingsSchema::validate`, lalu per field yang berubah: secret jadi `secretReadOnly`, `immutableAfterInit` jadi `immutableAfterInit`, `managed` tetap `ManagedFilled`. Dampak: `none` kalau tidak ada perubahan; selain itu menurut tabel *State transitions*, dihitung dari `x-stackly.apply` field yang berubah, capability `reload` dari `provider_revisions.capabilities`, dan lifecycle.
- `preview(package, values, endpoints, instance)`: render `run_plan` hanya untuk file yang tidak `sensitive`. Port: `bound_port` kalau ada, selain itu `Fixed`, selain itu `defaultPort` manifest dengan `portNote`. Render pratinjau tidak pernah me resolve secret; template yang merujuk secret selalu milik file sensitif (spec 0009), dan file sensitif tidak pernah tampil.
- `diff_settings(active, proposed)`: daftar `SettingChange` berurutan sesuai urutan deklarasi schema. Item daftar dengan `x-stackly.key` dibandingkan lewat key, dan `SettingChange.key` berbentuk `routes[name=api].rule` (item baru atau hilang: `routes[name=api]`), jadi urutan item yang berubah tidak menghasilkan diff palsu. Daftar tanpa key dibandingkan per indeks (`items[2].x`).

### Drift

- `supervisor::drift::check(instance_dir, revision_id) -> DriftReport`: untuk setiap file yang ada di salinan `config-history/<revision aktif>/` (semua file salinan, bukan hanya yang akan dirender), bandingkan byte dengan file di `config/`. Beda: `modified`; tidak ada: `missing`. Tidak ada folder salinan: tidak ada drift.
- Salinan ditulis per file oleh `write_config` (spec 0010 `steps.rs`) untuk setiap `FileWrite` yang tidak `sensitive`, atomik, sebelum file di `config/`; file lain di folder salinan tidak disentuh. Karena itu `run_render` mendapat konteks baru `RenderTarget { instance_dir, revision_id }` dari pemanggilnya.
- Dipanggil di `instance.config.get`; di `startup::run_attempt`, di `execute_init`, dan di `execute_reload`, masing masing tepat sebelum `run_render`; di precheck apply, rollback, dan change endpoints; dan sekali per instance di akhir rekonsiliasi saat daemon start, setelah `BlockerPlan` rekonsiliasi dipasang.
- Respawn sistem juga diperiksa: drift membuat respawn gagal `config.drift_detected` tanpa memakai retry budget, dan instance berakhir `failed` dengan blocker `ConfigDrift` (tidak ada pengguna untuk memilih, jadi Stackly berhenti, bukan menimpa).
- Hasil drift memasang `InstanceBlocker::ConfigDrift` hanya kalau slot blocker kosong; hasil bersih menghapus blocker `ConfigDrift` saja. Slot blocker tetap satu: `NeedsSecret`, `DataInUse`, dan `IdentityUnproven` didahulukan, dan drift dipasang ulang oleh pemeriksaan berikutnya setelah blocker lain hilang. `BlockerPlan` di `reconcile.rs` tidak mendapat cabang baru.
- Render yang menemukan drift gagal `config.drift_detected` sebelum menulis satu file pun.
- `resolve_config_drift`: lock instance (`Reject`; operasi lain yang memegang instance memberi `resource.busy`, dan kamu mencoba lagi), tulis ulang setiap file `modified` atau `missing` dari salinan secara atomik, periksa lagi, hapus blocker. Instance yang sedang jalan tidak di reload; label banner sesudahnya menyebut bahwa proses memakai file baru setelah reload atau restart berikutnya.

### Operasi

`apply_config` (`crates/core/src/instance/config/apply.rs`), langkah (`StepSpec`):

1. `validate`: gather dan `evaluate_change`; tolak dengan kode AC-3, AC-6, AC-9, AC-11.
2. `lock`: `LockKey::Instance`, `BusyPolicy::Queue`; baca lagi dan ulangi langkah 1 dari data segar.
3. `commit`: `commit_guarded` dengan guard murni yang memeriksa revision aktif masih `baseRevision`, lifecycle masih cocok dengan dampak, dan (untuk endpoint) port fixed tidak konflik dengan instance lain; lalu `ActivateConfigRevision` dan `PruneConfigRevisions` dalam satu transaksi. Kompensator: tidak ada setelah langkah ini (revision tetap aktif).
4. `apply`: dampak dihitung ulang dari lifecycle segar setelah commit (state bisa berubah, misalnya instance berhenti sendiri): `stopped`, `failed`, atau `uninitialized` sekarang berarti `next_start`, dan operasi `succeeded` dengan dampak itu di hasilnya, bukan gagal. `next_start` selesai; `reload` menjalankan langkah reload spec 0026 di bawah operasi ini; `restart` menjalankan langkah restart spec 0026 (stop, tunggu exit, tunggu lock, start) di bawah operasi ini. Kegagalan memakai kode langkah itu (AC-8).
5. Secret: params tidak membawa caller; handler membangun `SecretCaller` pengguna dari `OperationTrigger` request, sama dengan restart spec 0026, dan meneruskannya ke `ReloadRequest.caller` dan startup. `NeedsSecret` di sub langkah restart menggagalkan operasi dengan kode spec 0010 dan 0011, memasang blocker `NeedsSecret`, dan revision baru tetap aktif.

### Last known good

- Supervisor menyimpan `applied_revision: ConfigRevisionId` di slot proses (`ActorState`) setiap kali ia merender untuk sebuah spawn atau reload: nilainya revision aktif yang dibaca saat render itu. Proses yang diadopsi (spec 0010) mendapat `applied_revision` kosong.
- Pada transisi ke ready (`readiness::first_ready`, transisi rutin not ready ke ready, dan verify reload yang ready), supervisor menjadwalkan `MarkConfigRevisionReady { revision: applied_revision, at }` lewat `blocking` di luar task probe, hanya kalau `applied_revision` terisi dan sama dengan revision aktif. Readiness di memori tidak menunggu tulisan itu.
- Tulisan yang gagal dicatat di log (`warn`, kode registry) dan dicoba lagi pada transisi ready berikutnya; ia tidak pernah mengubah readiness atau lifecycle.
- Proses adopsi tidak pernah memberi `ready_at`, jadi revision baru yang belum pernah dirender tidak pernah menjadi last known good karena proses lama. Instance adopsi tetap bisa memberi last known good setelah restart berikutnya.

Rollback adalah `apply_config` dengan `rollback: true`: settings dan endpoint dari last known good, `origin` `rollback`. `fromRevision` (Restore di CLI) memuat settings dan endpoint revision itu dengan `origin` `restored`. `change_endpoints` memakai langkah yang sama dengan settings revision aktif dan endpoint baru, `origin` `endpoints`; dampaknya selalu `restart` untuk `running` dan `recovering`. `rename_instance`: validate, lock (`Queue`), commit (`commit_guarded` dengan cek nama), tanpa langkah proses.

### Fake provider

File config fake `0.3.0` hanya `fake.json` yang `sensitive` di `${instance.secrets}`, jadi tidak ada file untuk drift, salinan, atau Effective File. Paket `providers/fake` naik ke revision `0.4.0` dengan template `fake.conf.tmpl` (format `plain`, tidak merujuk secret) yang dirender ke `${instance.config}/fake.conf`, berisi setting fake yang `reload` dan `restart`, dan fake server membacanya saat start dan saat reload. Golden plan dan render, fixture kontrak yang mengikuti digest fake, dan seed fuzz diperbarui (`STACKLY_UPDATE_GOLDEN=1`, `cargo xtask fuzz seed`). Fixture `config-view-fake.json` menggantikan kebutuhan provider nyata untuk test Swift; `config-view-postgresql.json` dan `config-view-traefik.json` dibangun dari paket pinned di test daemon tanpa menjalankan runtime.

### CLI

Di `apps/cli/src/commands/instance/`:

| Perintah | Isi |
|---|---|
| `stackly instance config get <target> [--json] [--file <path>]` | settings dengan kelas dan dampak, revision, last known good, histori, drift; `--file` mencetak satu Effective File |
| `stackly instance config validate <target> [--set k=v] [--set-json k=json] [--unset k] [--from-revision N] [--json]` | diff, dampak, field error; exit 0 kalau valid |
| `stackly instance config apply <target> …` | flag `validate` ditambah `--expect-revision N`, `--dry-run`, `--wait`, `--timeout`, `--idempotency-key`; tanpa `--expect-revision` CLI memakai revision aktif dari `get` |
| `stackly instance config rollback <target> [--yes]` | roll back ke last known good, konfirmasi |
| `stackly instance config keep-revision <target> [--yes]` | menulis ulang file drift, konfirmasi |
| `stackly instance rename <target> <new-name>` | `--json`, `--idempotency-key` |
| `stackly instance endpoint set <target> --port <endpoint>=<port\|auto>… [--dry-run] [--wait]` | sama dengan `--port` di create |

`--unset k` mengembalikan field ke default schema (ditolak untuk field `required` tanpa default). `--from-revision` digabung dengan `--set` berarti nilai revision itu lalu override. Progress ke stderr, hasil ke stdout.

### GUI

Di `apps/macos/Packages/StacklyUI/Sources/StacklyUI/InstanceDetail/Configuration/` (baru):

- `ConfigurationModel` (`@Observable`, `@MainActor`): memuat `instance.config.get`, menyimpan perubahan tertunda, memanggil `validate` dengan debounce 300 ms setelah edit, menyimpan `baseRevision`, dan memuat ulang setelah event `instance.state_changed` yang mengubah revision. `config.revision_conflict` menampilkan banner "This configuration changed. Reload to see revision {N}." dan menyimpan perubahan tertunda sampai kamu memilih.
- `ConfigurationTab`: segmented Form dan Effective File, baris header (Revision N, tag dampak, Revert, tombol apply), banner drift, banner roll back, form dari `SchemaFormSections` dengan baris terkunci tambahan untuk managed, port, set once, dan secret, Pending Changes, dan History.
- Effective File: teks monospace read only, satu bagian per file, baris berubah ditandai saat ada perubahan tertunda, `portNote` di atasnya.
- Show Diff: sheet diff per file memakai view diff spec 0067.
- Konfirmasi native untuk Apply and Restart ("Restarting stops {name} briefly."), Roll Back, dan Keep Revision, dengan Cancel sebagai default untuk Keep Revision karena menimpa edit di luar Stackly.
- `RenameSheet` dan `EndpointsSheet` (di `InstanceDetail/`), item menu Rename… dan Edit Ports… di peta perintah spec 0069 tanpa shortcut, dan item perhatian drift di Overview.

### Copy

| Tempat | Teks |
|---|---|
| Tag dampak | "In sync" · "Applies on reload" · "Requires restart" · "Applies at next start" |
| Catatan field | "{Applies on reload \| Requires restart} · default {value}" |
| Baris port | "Managed by Stackly · change in Endpoints" |
| Field set once | badge "Set once", "Can’t be changed after the instance is created." |
| Field secret | "Stored in Keychain as {reference}." |
| Banner drift | judul "{file} was changed outside Stackly"; isi "{time} · Stackly won’t overwrite it or start {name} until you choose."; tombol Keep Revision {N}, Show Diff |
| Banner roll back | judul "Revision {N} hasn’t reached ready"; isi "Revision {M} is the last known good configuration."; tombol Roll Back to Revision {M} |
| Asal histori | "Created from provider defaults" · "Applied" · "Ports changed" · "Rolled back to revision {M}" · "Restored from revision {M}" |
| Umpan balik | "Revision {N} applied to {name}." · "Revision {N} saved. It applies at next start." · "{old} renamed to {new}." · "Ports changed on {name}." · "Configuration files restored from revision {N}." |
| Pending kosong | "No pending changes. Edit a value to see the diff." |

### Konstanta

| Nama | Nilai | Tempat |
|---|---|---|
| `CONFIG_HISTORY_KEEP` | 20 | `core::instance::config` |
| debounce validate GUI | 300 ms | `ConfigurationModel` |

### Value sourcing

| Aksi | Nilai | Sumber |
|---|---|---|
| get | field, kelas, dampak per field, default | settings schema paket pinned (`load_package`), `x-stackly.apply` |
| get | nilai settings | `config_revisions.config_values` revision aktif |
| get | nomor revision aktif | `config_revisions.number` lewat `instances.config_revision_id` |
| get | last known good | `Registry::last_known_good` (nomor tertinggi dengan `ready_at`) |
| get | histori dan asal | `config_revisions` (20 terbaru, urut `number`); asal dari kolom `origin` dan `source_number` |
| get | endpoint | `config_revisions.endpoints` revision aktif dan `instance_endpoints.bound_port` |
| get | Effective File | salinan `config-history/<revision aktif>/`; tanpa salinan, `preview` dengan nilai aktif |
| get, validate | `reloadSupported` | `provider_revisions.capabilities` lewat `instances.provider_revision_pk` |
| get | drift | `supervisor::drift::check` |
| validate | dampak | `evaluate_change`: field berubah, capability, `instances.lifecycle` |
| validate | port di pratinjau | `bound_port`, lalu `Fixed`, lalu `defaultPort` manifest |
| apply | nomor revision baru | nomor revision aktif + 1 di dalam `commit_guarded` |
| apply | `baseRevision` | input client (dari `get`); CLI tanpa `--expect-revision` memakai `get` sebelum kirim |
| rollback | settings dan endpoint | revision last known good |
| ready_at | revision dan waktu | `applied_revision` di slot proses dan `Clock` saat transisi ready (*Last known good*) |
| apply, restart | secret caller | `OperationTrigger` request, seperti restart spec 0026 |
| apply | dampak akhir | `apply_allowed` dari lifecycle segar setelah commit |
| change endpoints | saran port bebas | `PortProbe` dan pemeriksaan spec 0020 |
| change endpoints | port milik sendiri | `instance_endpoints.bound_port` |
| rename | aturan nama | `InstanceName` (VAL-001) dan indeks unik `instances_active_name` |
| journal (AC-20) | nama setting berubah, port lama dan baru | `diff_settings` dan snapshot endpoint, secret sebagai reference |
| banner drift | `{file}`, `{time}` | `ConfigDrift.files[0].path` (ditambah "and {n} more"), `observedAt` dalam locale pengguna |
| tombol apply | label | `ConfigValidation.impact` |
| Overview drift | instance terdampak | `InstanceSummary.blocker` `ConfigDrift` |

### Key invariants

- Revision aktif selalu revision dengan nomor tertinggi milik instance itu, jadi nomor tidak pernah dipakai ulang walau baris lama dihapus.
- Revision aktif dan last known good tidak pernah dihapus prune.
- Config yang tidak lolos validasi tidak pernah menulis revision, file, atau journal operasi, dan tidak pernah menyentuh proses.
- Satu satunya jalan settings atau port berubah adalah `ActivateConfigRevision` di dalam `commit_guarded`; `instance_endpoints.fixed_port` selalu sama dengan snapshot endpoint revision aktif.
- Port fixed tidak pernah diganti diam diam, termasuk saat start setelah change endpoints.
- Stackly tidak pernah menulis file di `config/` yang drift kecuali lewat `resolve_config_drift`.
- Field secret, `managed`, dan `immutableAfterInit` tidak pernah berubah lewat fitur ini.
- Rename tidak mengubah ID, runtime, secret reference, path, atau config revision.
- `get`, `validate`, pratinjau, dan pemeriksaan drift tidak pernah me resolve secret, membaca catalog, atau memakai jaringan.
- Reload hanya dijalankan untuk provider dengan capability `reload` dan perubahan yang semuanya `apply: reload`.

### Security model

- Semua method hanya lewat socket Unix privat daemon milik user (spec 0006); tidak ada peran tambahan.
- Nilai secret tidak pernah ada di `config_values`, journal, histori, Effective File, atau diff; file sensitif tidak pernah disalin ke `config-history/` dan tidak pernah dikirim ke client.
- Path di `EffectiveFile` dan `DriftFile` relatif terhadap `config/`; client tidak pernah mengirim path.
- Salinan histori ditulis dengan izin yang sama dengan `config/` (0700 folder milik user).
- Host endpoint tidak bisa diubah, jadi fitur ini tidak pernah membuka listener ke LAN.

### Configuration required

Tidak ada env var, secret, atau credential baru.

### Critical test scenarios

- Happy path reload: PostgreSQL 18 nyata, `logMinDurationStatementMs` (`apply: reload`) diubah lewat apply, PID sama, ready, revision 2 aktif dengan `ready_at`, `SHOW log_min_duration_statement` menunjukkan nilai baru. Lalu `maxConnections` (`apply: restart`) diubah: PID baru dan `SHOW max_connections` menunjukkan nilai baru. `superuserPassword` (secret, set once) ditolak `secret_read_only`. Memverifikasi **AC-4**, **AC-5**, **AC-7**, **AC-15**.
- Restart: NATS nyata, perubahan setting `restart`, PID baru, tidak ada langkah reload di journal. Memverifikasi **AC-5**.
- Tidak valid: nilai di luar batas, field set once diubah, field secret diubah; balasan field error, `operation.list` tidak bertambah, revision aktif sama, PID sama. Memverifikasi **AC-2**, **AC-3**.
- Config valid yang gagal: fake provider dengan setting `restart` yang membuat fake gagal ready; operasi gagal, revision 2 aktif tanpa `ready_at`, rollback menulis revision 3 dengan nilai revision 1, ready. Memverifikasi **AC-8**.
- Konflik: dua client apply dengan `baseRevision` yang sama; satu berhasil, satu `config.revision_conflict`; apply tanpa perubahan `unchanged`. Memverifikasi **AC-9**.
- Drift: pada fake provider `0.4.0` (*Fake provider*), test mengubah `fake.conf` di `config/`; `get` melaporkan drift, start ditolak `config.drift_detected` dan file tidak berubah, Keep Revision menulis ulang dan start berhasil; daemon restart dengan drift memasang blocker saat rekonsiliasi; file yang dihapus terdeteksi `missing`. Memverifikasi **AC-10**, **AC-11**.
- Instance lama: registry dari migrasi sebelumnya tanpa salinan histori, tidak ada drift sampai start pertama. Memverifikasi **AC-11**.
- Prune: 25 apply, 20 baris tersisa, revision aktif dan last known good tetap, folder salinan yang dihapus ikut hilang, nomor berikutnya 26. Memverifikasi **AC-12**.
- Rename: saat running, PID dan path sama; nama dipakai instance lain `instance.name_taken`; create dan rename bersamaan dengan nama sama, satu menang. Memverifikasi **AC-13**.
- Endpoint: fixed ke port yang dipegang proses luar ditolak dengan saran; Auto ke Fixed pada port `bound_port` sendiri saat running diterima; port fixed instance lain ditolak di dalam `commit_guarded`; overlap wildcard dan IPv6 spec 0020. Memverifikasi **AC-14**.
- Traefik: form `routes` daftar dengan `name` unik, item ganda `DuplicateKey`, apply reload mengubah route di instance A tanpa menyentuh B. Memverifikasi **AC-16**, **AC-4**.
- Audit: journal apply dan change endpoints memuat nama setting dan port, tidak memuat nilai secret. Memverifikasi **AC-20**.
- Offline: harness spec 0021 AC-10 untuk setiap method. Memverifikasi **AC-21**.
- CLI: `config apply --dry-run --json` sama dengan fixture `config-validation-restart.json`; `rollback` tanpa terminal dan tanpa `--yes` gagal `cli.confirmation_required` tanpa request. Memverifikasi **AC-17**.
- GUI: test `ConfigurationModel` (debounce, conflict, label tombol per dampak), snapshot tab Configuration untuk PostgreSQL, NATS, Traefik, drift, roll back, offline, dan sheet rename dan port. Memverifikasi **AC-15**, **AC-18**, **AC-19**.
- Migrasi: registry versi sebelumnya dimigrasi; setiap revision `number` 1, `endpoints` sama dengan `instance_endpoints`, `ready_at` null. Memverifikasi **AC-7**, **AC-12**.

### Usulan update untuk spec lain

- Spec 0005: kolom `number`, `endpoints`, `ready_at` di `config_revisions`; status `superseded`; `WriteOp::ActivateConfigRevision`, `MarkConfigRevisionReady`, `PruneConfigRevisions`; kalimat "fitur 29 memutuskan apakah mengaktifkan revision berikutnya menaikkan revision instance" dijawab: ya, naik satu.
- Spec 0010 *Startup transaction* dan reload: langkah render memeriksa drift dulu dan menulis salinan histori; commit readiness menulis `MarkConfigRevisionReady`.
- Spec 0026: langkah reload dan restart bisa dijalankan sebagai sub langkah operasi lain (`apply_config`, `change_endpoints`); follow up fitur 29 di spec itu dicentang.
- Spec 0020: `SchemaFormModel` menerima baris terkunci tambahan dan label dampak lewat parameter, tanpa cabang provider.
- Spec 0022: item drift di Overview tidak lagi menunggu fitur 29.
- Spec 0069: baris `renameInstance` dan `editPorts` menjadi built, tanpa shortcut.
- Spec 0021: tab Configuration dan tombol Edit di kartu Endpoints tidak lagi menunggu fitur 29.

### Selisih mockup yang disengaja

- Banner drift punya Show Diff selain Keep Revision; mockup hanya Keep Revision.
- Tag "Applies at next start" dan tombol Apply untuk instance yang berhenti tidak ada di mockup.
- Banner roll back tidak ada di mockup; asalnya PRD PAGE-06 (last known good) dan AC-8.
- Effective File read only dengan catatan port Automatic; mockup tidak menunjukkan apakah bisa diedit.
- Field secret tampil sebagai reference read only.

## Build plan

Pendekatan Journey: journey "ubah instance yang sudah ada" diselesaikan lengkap (state, error, CLI, GUI, bukti) sebelum fitur 30. Urutannya dari dalam ke luar supaya setiap milestone bisa dites sendiri. Butuh fitur 26 (spec 0026) di main, yang sudah done.

**Milestone 1: Domain, registry, dan core**

1. Domain: `ConfigRevisionNumber`, `EndpointRequests`, `ConfigRevisionOrigin`, field baru `ConfigRevisionRecord`, `ConfigRevisionStatus::Superseded`, `Instance::set_port_request`, `OperationKind::ApplyConfig`, `ChangeEndpoints`, `ResolveConfigDrift` dengan interrupt policy di *State transitions* (test himpunan `CloseInterrupted` diperbarui), `InstanceBlocker::ConfigDrift`, `apply_allowed` dan `rename_allowed`; unit test domain. Memenuhi **AC-6**, **AC-7**, **AC-10**, **AC-12**, **AC-13**.
2. Migrasi `NNNN_config_revision_history.sql` (nomor bebas berikutnya), backfill, mapping storage, `ActivateConfigRevision`, `MarkConfigRevisionReady`, `PruneConfigRevisions`, `config_revisions`, `last_known_good`, `FakeRegistry`, `registry_conformance`, fixture registry versi sebelumnya, dan test migrasi. Memenuhi **AC-7**, **AC-12**, **AC-14**.
3. Fake provider `0.4.0` dengan `fake.conf` di `config/` (*Fake provider*), golden, dan seed fuzz. Memenuhi **AC-10**, **AC-16**.
4. Evaluator `crates/core/src/instance/config/` (`gather_config`, `evaluate_change`, `preview`, `diff_settings`) dan pemetaan `field_code` ke dua varian `FieldCode` baru, dengan table test pada fake, PostgreSQL, NATS, dan Traefik. Memenuhi **AC-1**, **AC-2**, **AC-15**, **AC-16**.
5. Salinan histori per file di `write_config` dengan `RenderTarget`, `supervisor::drift` di `run_attempt`, `execute_init`, dan `execute_reload`, respawn yang berhenti karena drift, pemeriksaan saat rekonsiliasi daemon start, blocker, `resolve_config_drift`; test core dengan `FakePlatform`. Memenuhi **AC-10**, **AC-11**.
6. `applied_revision` di slot proses dan `MarkConfigRevisionReady` lewat `blocking` pada setiap transisi ready, dengan test adopsi setelah daemon mati di antara commit dan restart. Memenuhi **AC-7**.
7. Operasi `apply_config` (termasuk rollback dan `fromRevision`, dampak dihitung ulang setelah commit, secret caller), `change_endpoints` dengan pemeriksaan port spec 0020 dan pengecualian `bound_port` sendiri, dan `rename_instance`, memakai sub langkah reload dan restart spec 0026; test core dengan fake, termasuk konflik, antre, dan daemon mati setelah commit. Memenuhi **AC-3** sampai **AC-9**, **AC-13**, **AC-14**, **AC-20**.

**Milestone 2: Kontrak, daemon, CLI, dan bukti fixture** (after milestone 1)

8. DTO, params, kode error, enam baris `methods!`, minor bump `ProtocolVersion`, fixture kontrak, `cargo xtask codegen`, dan `cargo xtask fuzz seed`. Memenuhi **AC-1**, **AC-2**, **AC-17**.
9. Handler `stacklyd` (replay, precheck, start operation) dan mapping blocker `ConfigDrift` di `InstanceSummary`. Memenuhi **AC-1** sampai **AC-14**.
10. Test daemon lewat socket: apply reload dan restart pada fake, invalid tanpa operasi, konflik, drift dengan daemon restart, prune, rename bersamaan dengan create, change endpoints, audit journal, dan harness offline. Memenuhi **AC-3**, **AC-6**, **AC-8** sampai **AC-14**, **AC-20**, **AC-21**.
11. CLI `config get|validate|apply|rollback|keep-revision`, `rename`, `endpoint set`, completion, man page, golden, dan matriks parity. Memenuhi **AC-17**.

**Milestone 3: GUI** (after milestone 2; needs fitur 67, 68, dan 69 di `ui-macos.md`)

12. `StacklyClient`: method dan tipe hasil codegen, test fixture Swift. Memenuhi **AC-1**, **AC-17**.
13. `ConfigurationModel` dan `ConfigurationTab` (Form, Effective File, Pending Changes, History dengan Restore, banner drift dan Show Diff, banner roll back, konfirmasi), parameter baris terkunci dan label dampak di `SchemaFormModel`; test Swift. Memenuhi **AC-15**, **AC-16**, **AC-18**.
14. `RenameSheet`, `EndpointsSheet` dari kartu Endpoints, item menu Rename… dan Edit Ports…, item drift di Overview, kartu umpan balik dan label sibuk. Memenuhi **AC-19**.
15. Fixture tampilan spec 0068 untuk PostgreSQL, NATS, Traefik, drift, roll back, offline, dan kedua sheet, lalu snapshot review. Memenuhi **AC-16**, **AC-18**, **AC-19**.

**Milestone 4: Provider nyata di Mac ini** (after milestone 2, dan milestone 3 untuk bukti GUI; needs fitur 19 dan 23 di `02-instalasi-pertama.md`; artefak di cache runtime xtask, selain itu `BLOCKED`; Traefik `BLOCKED` sampai artefaknya ada)

16. Native PostgreSQL 18: apply reload dan apply restart, last known good, rollback setelah setting yang membuat start gagal, change endpoints fixed dengan restart dan konflik dari proses luar, drift pada `postgresql.conf`. Memenuhi **AC-4**, **AC-7**, **AC-8**, **AC-11**, **AC-14**.
17. Native NATS: apply restart (tanpa reload), last known good. Memenuhi **AC-5**, **AC-7**, **AC-16**.
18. Native Traefik: apply `routes` lewat reload di instance A, B tetap; last known good. Memenuhi **AC-4**, **AC-16**.
19. Run dengan jaringan mati lewat GUI dan CLI setelah persetujuan user; `BLOCKED` sampai disetujui. Memenuhi **AC-21**.

## Consequences

**Positive**:

- Satu jalur untuk semua perubahan settings dan port: validasi sama dengan create, satu operasi, satu journal, dan satu revision bernomor yang bisa dilihat, dibandingkan, dan dipulihkan.
- Last known good adalah fakta (pernah ready), bukan tebakan, jadi roll back selalu ke config yang terbukti jalan.
- Drift tidak pernah hilang diam diam karena render sebelum spawn, dan pemeriksaannya hanya membaca file kecil saat memang dibutuhkan.
- Provider baru, termasuk daftar `routes` Traefik, mendapat halaman Configuration dari schema tanpa kode Swift.

**Negative / tradeoffs**:

- Fake provider naik ke `0.4.0`, jadi golden, fixture, dan seed fuzz yang mengikuti digest fake ikut berubah.
- Satu migrasi registry dan minor bump protokol (termasuk dua varian `FieldCode` baru), yang berebut nomor dengan spec 0028 dan 0031; nomor final ditentukan saat merge.
- Effective File read only: kamu tidak bisa memakai setting yang tidak ada di schema provider; itu harus lewat penambahan field di paket provider.
- Drift memblokir start sampai kamu memilih, jadi edit manual yang disengaja selalu butuh satu klik Keep Revision (yang membuang edit itu) atau penambahan field di schema. Tidak ada Adopt Changes.
- Revision yang dibuat sebelum fitur ini tidak punya `ready_at`, jadi instance lama baru punya last known good setelah ready berikutnya.
- Pratinjau untuk endpoint Automatic yang belum pernah bind memakai `defaultPort`, jadi isinya bisa berbeda dari file yang dirender saat start; catatan port menjelaskannya.
- Satu slot blocker: drift bisa tertutup blocker lain dan baru tampil setelah blocker itu hilang.
- Rotasi secret belum ada; mengganti password tetap di luar Stackly.

**Neutral**:

- Folder `config-history/` yang sudah dibuat create kini terisi; ukurannya kecil (file config teks, paling banyak 20 revision).
- Kode `config.*` baru membuka kategori kode untuk fitur environment (fitur 44 sampai 47).

## Follow-up

- [ ] Terapkan *Usulan update untuk spec lain* ke spec 0005, 0010, 0026, 0020, 0022, 0069, dan 0021 saat milestone yang bersangkutan dibangun.
- [ ] Fitur 47: ganti port pada instance yang dipinjam environment dengan lease aktif ditolak atau butuh override.
- [ ] Fitur 59: update provider revision memakai diff dan revision dari spec ini (BR-017).
- [ ] Rotasi secret sebagai fitur sendiri, kalau dibutuhkan (belum ada di scope).
- [ ] Tentukan nomor migrasi dan minor protokol saat build, setelah spec 0028 dan 0031 merge.
