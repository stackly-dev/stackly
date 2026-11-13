# Scope 08 · Environment proyek

[Kembali ke index](index.md)

Journey PRD JOURNEY-04 (contoh environment ERP): import `stackly.yaml`, review trust provider, plan, lock versi exact, up dengan progress dan readiness, lalu down tanpa menghapus data. Fase ini datang setelah stack enam service karena environment ERP memakai keenamnya. Fase ini selesai kalau tim bisa berbagi satu file environment tanpa ikut membagikan data, path pribadi, atau password, dan seluruh jalur itu bekerja dari GUI dan CLI.

### 44. Manifest environment: validate & plan · in-progress

Schema `stackly.yaml` (versi, namespace, selector runtime, endpoint, settings, secret reference, dependency, ownership) dan plan yang menampilkan diff serta dampak tanpa efek samping. Termasuk halaman Environments untuk import, edit, validasi, dan preview plan.

**Done when:** plan untuk manifest baru menampilkan install, create, dan start tanpa membuat file data, proses, perubahan trust, atau runtime; cycle, reference yang tidak ada, secret literal, dan konflik endpoint fixed ditolak sebelum ada mutasi; `stackly env validate` dan `plan` setara dengan GUI; halaman Environments bisa dipakai dengan interaksi desktop; halaman Environments dan sheet plan mengikuti mockup UI: status environment, tabel service dengan selector, instance, versi, ownership, dan status, plan dengan kata kerja Create, Start, dan Keep, serta daftar environment di sidebar dengan hitungan ready; menu aksi per service di tabel service memakai `OpenInMenu` dari [spec 0070](../specs/0070-buka-instance-aplikasi-klien/index.md) (fitur 70), ditambah Copy Connection URL, Show Instance, Show Logs, dan Start atau Stop seperti mockup.

**Trace:** REQ-020, REQ-028, VAL-013, VAL-014, PAGE-10, AC-020, AC-080 (bagian plan), NFR-012, JOURNEY-04

**Pointer:** spec [0044](../specs/0044-manifest-environment-validate-plan/index.md) · code in `crates/core/src/environment/`, `crates/protocol/src/env.rs`, `apps/daemon/src/ipc/env/`, `apps/daemon/src/membership.rs`, `apps/cli/src/commands/env/`, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Environments/`

- [x] Design it (spec): `/architect manifest environment: validate & plan`
- [ ] Build it: `/develop manifest environment: validate & plan`
  - [x] Manifest, validate, dan plan di daemon: migrasi `environments.services` dan unique index nama serta lokasi, `DeclaredService` dan dua `OperationKind`, loader dan aturan validasi `stackly.yaml` dengan JSON Schema hasil codegen, plan di atas `gather` dan `evaluate` create (`planned_runtime`), kenaikan minor protokol dengan modul `env`, enam method, area kode `env`, `InstanceOwnership` baru, lalu handler `env.validate`, `env.plan`, `env.list`, `env.get`, operasi `import_environment` dan `remove_environment`, cache keanggotaan, dan test tanpa efek samping (spec 0044 AC-1 sampai AC-15, AC-24; build steps 1 to 5; needs scope 01 sampai 07 di `main`)
  - [x] `stackly env validate`, `plan`, `import`, `list`, `remove`: grup `env`, resolve target, kode `cli.env_invalid`, `cli.env_plan_blocked`, `cli.environment_not_found`, kolom `ENVIRONMENT` di `instance list`, golden dan binary test (spec 0044 AC-16, AC-17, AC-15; build step 6; after build step 4 untuk golden; binary test needs build step 5; bisa paralel dengan milestone berikutnya)
  - [x] Environments di app: `EnvironmentStore`, baris anak sidebar, halaman daftar dan detail dengan tabel Services dan menu baris (`OpenInMenu`), banner manifest dan partially ready, sheet plan, Import…, Validate, Reload, Remove, seksi Environments di Overview, kolom dan baris Environment di Instances, baris onboarding, fixture `environment-partially-ready`, snapshot, lalu evidence dan langkah manual yang butuh mata dan tangan kamu (spec 0044 AC-18 sampai AC-25; build steps 7 to 11; after build step 4; evidence manual needs build step 5)
- [ ] Verify it: `/check verify manifest environment: validate & plan`
- [ ] Test it: `/test manifest environment: validate & plan`

### 45. Lockfile & override lokal · in-progress

Pisahkan manifest yang dibagikan, `stackly.lock` yang menyimpan versi provider, runtime, dan digest per platform, dan `.stackly.local.yaml` untuk override lokal. `stackly env up --locked` tidak pernah melakukan resolusi ulang.

**Done when:** lockfile tidak memuat password, home directory, atau PID; `stackly env lock` boleh memakai metadata online tapi tidak menjalankan provider; mesin tanpa artefak yang kompatibel gagal dengan jelas tanpa substitusi; override port dan storage tidak mengubah manifest milik tim; `stackly env export` tidak membawa secret.

**Trace:** REQ-023, REQ-028, BR-018, AC-023, AC-080, PRD bagian 13.1 dan 13.3

**Pointer:** spec [0045](../specs/0045-lockfile-override-lokal/index.md)

- [x] Design it (spec): `/architect lockfile & override lokal`
- [ ] Build it: `/develop lockfile & override lokal`
  - [ ] Lock, override, dan export di daemon: `environment::lock` (loader, emitter, state, entri yang dipertahankan) dengan JSON Schema, plan dengan lock dan `locked` (*Runtime dari lock*, blocker, `planDigest`), `environment::local` dan `CreateDraft.data_location`, `lock_sha256` di `Environment`, `environment::export` lewat `Redactor`, kenaikan minor protokol dengan `env.lock`, `env.export`, dan kode `env.*` baru, lalu handler, test tanpa efek samping, test sentinel, dan skenario root A ke root B (spec 0045 AC-1 sampai AC-15, AC-20, AC-21; build steps 1 to 6; needs spec 0044 build steps 1 to 5)
  - [ ] `stackly env lock`, `plan --locked`, `export`: flag `--update` dan `--service`, kolom `SOURCE` dan baris `lock:` serta `local:`, kode `cli.env_lock_blocked` dan `cli.env_locked_unsupported`, golden dan binary test (spec 0045 AC-6, AC-16, AC-17; build step 7; after build step 5 untuk golden; binary test needs build step 6; bisa paralel dengan milestone berikutnya)
  - [ ] Lock dan override di app: baris Lock dan Local override, Lock Versions, Update Lock…, Copy Environment Variables, tambahan sheet plan, snapshot, lalu evidence dan langkah manual yang butuh mata dan tangan kamu (spec 0045 AC-18, AC-19, AC-21; build steps 8 to 9; after build step 5 dan milestone app spec 0044; evidence manual needs build step 6; override storage sampai instance needs Up spec 0046)
- [ ] Verify it: `/check verify lockfile & override lokal`
- [ ] Test it: `/test lockfile & override lokal`

### 46. Env up & dependency graph · in-progress

Up memasang yang dibutuhkan, membuat instance dedicated atau meminjam instance yang ada lewat `instanceRef` (tanpa mengubah config, auth, port, atau autostart miliknya), mengambil lease, lalu menjalankan dependency graph berdasarkan readiness dan timeout. Partial failure menghasilkan state yang jelas, dan kompensasi hanya menyentuh resource milik operasi itu.

**Done when:** dependency yang tidak ready membuat dependent tidak ditandai sukses; instance yang sudah ada dan sehat tidak dihapus; up dengan `instanceRef` menambah lease tanpa membuat instance baru; secret reference yang belum diisi membuat up berhenti dengan status needs secret yang menunjuk reference itu, tidak pernah diganti password kosong; operasi menunjukkan tahap yang gagal dan langkah recovery; `stackly env up` dan `stackly env status` menampilkan port auto yang sebenarnya dipakai; graph diuji dengan provider fixture tanpa dependency palsu antar database; banner partially ready di halaman Environments menunjuk service yang belum ready beserta alasannya, seperti mockup UI.

**Trace:** REQ-021, REQ-028, BR-023, VAL-014, AC-021, AC-073 (bersama fitur 47), PRD bagian 13.1 dan 13.2

**Pointer:** spec [0046](../specs/0046-env-up-dependency-graph/index.md) · code in `crates/core/src/operation/`, `crates/core/src/instance/create/binding.rs`, `crates/domain/src/up.rs`

- [x] Design it (spec): `/architect env up & dependency graph`
- [ ] Build it: `/develop env up & dependency graph`
  - [x] Fondasi operasi anak: kelas slot pengatur dari `OperationKind::is_coordinator()` dengan `MAX_RUNNING_COORDINATORS`, `Origin::Child` dan `parent`, `OperationEngine::wait_final`, `recover` yang menutup anak antre, kebijakan `environment_up` (tidak resumable, `CloseInterrupted`); migrasi `environments.last_up` dan `operation_journal.parent_id`, `MemberDependency`, `UpReport`, mutator `Environment`, perluasan `DeleteInstanceCreatedBy`; `EnvironmentBinding` di create (batch `register`, `input` journal, kompensasi) dan `StartTrigger::Environment` (spec 0046 AC-3, AC-4, AC-7, AC-12, AC-13, AC-15, AC-24; build steps 1 to 3; needs fitur 44 milestone 1 di `main`, dan update spec 0007 diterapkan)
  - [ ] Up di daemon: core `environment::up` (precheck, guard `accept`, graph murni, driver dengan commit penutup, port `UpChildren`, `UpBoard`), kenaikan minor protokol dengan `env.up`, tambahan DTO `env` dan `operation`, lima kode area `env`, fixture; handler `env.up`, pemecahan handler install, create, dan start, adapter anak, tambahan `env.get` dan `env.list`, test integrasi dengan provider `fake` (spec 0046 AC-1 sampai AC-16, AC-23; build steps 4 to 6; after milestone pertama; needs fitur 45 build untuk `PlanInputs.locked` dan field lock di plan)
  - [ ] `stackly env up` dan `status`: resolver target ber path, progress dan tabel akhir dengan port yang benar benar dipakai, `--locked`, golden dan binary test (spec 0046 AC-17, AC-18; build step 7; after build step 5 untuk golden; binary test needs build step 6; bisa paralel dengan milestone berikutnya)
  - [ ] Up di app: tombol Up… dan Bring Up di sheet plan, `SecretEntrySheet` untuk needs secret, "Create with Up…", banner "Bringing up…" dan phase per service, alasan di banner partially ready, Activity dengan anak bersarang dan label Partial, onboarding, fixture `environment-bringing-up` dan `environment-up-incomplete`, snapshot, lalu test native ERP dari cache xtask, evidence, dan langkah manual yang butuh mata dan tangan kamu (spec 0046 AC-19 sampai AC-22, AC-25; build steps 8 to 11; after build step 5; needs fitur 44 milestone app; evidence manual needs build step 6)
- [ ] Verify it: `/check verify env up & dependency graph`
- [ ] Test it: `/test env up & dependency graph`

### 47. Env down, shared instance & lease · in-progress

Down melepas lease environment dan hanya menghentikan instance yang tidak dibutuhkan lagi, memakai aturan evaluasi demand dari fitur 31. Stop manual atau Stop Services yang mengganggu environment lain ditolak atau meminta override dengan dampak yang jelas.

**Done when:** saat environment A down, lease B tetap ada dan instance tetap ready; up A dan down B yang bersamaan menghasilkan lease konsisten tanpa create ganda; lease disimpan per identitas environment, bukan per koneksi client, jadi menutup CLI atau GUI tidak melepas lease; Stop Services dari menu bar menampilkan lease environment yang terdampak; ganti port atau endpoint pada instance yang dipinjam environment dengan lease aktif ditolak atau meminta override dengan dampak yang jelas; `stackly env down` tidak pernah menghapus data atau runtime; konfirmasi Down menjelaskan instance mana yang tetap jalan (shared atau dimulai manual), dan baris Lease di halaman Environments mengikuti mockup UI.

**Trace:** REQ-022, REQ-028, BR-006, BR-014, BR-015, VAL-014, AC-022, AC-073 (bersama fitur 46), PRD bagian 13.2

**Pointer:** spec [0047](../specs/0047-env-down-shared-instance-lease/index.md) · code in `crates/core/src/supervisor/stop.rs`, `crates/core/src/supervisor/persist.rs`

- [x] Design it (spec): `/architect env down, shared instance & lease`
- [ ] Build it: `/develop env down, shared instance & lease`
  - [ ] Down di daemon: kebijakan `EnvironmentDown` (pengatur, tidak resumable), `Environment::begin_down`, `leases_of_environment`, stop ber guard (`LeaseCheck`, `commit_stop_decision`, hasil `kept`, aturan respawn), core `environment::down` (`select`, `order`, driver dengan sapuan akhir, port `DownChildren`), kenaikan minor protokol dengan `env.preview_down`, `env.down`, `EnvLeaseInfo`, `leaseActive`, dan kode baru, lalu handler, cancel Up, dan test `env_down.rs` (PRD AC-022, serentak, tidak ada yang terhapus) (spec 0047 AC-1 sampai AC-12, AC-25; build steps 1 to 5; needs spec 0046 milestone 1 dan 2 dan spec 0044 milestone 1; langkah 2 after langkah 3 spec 0046)
  - [ ] Override pada aksi lain: `StopAllOptions` dan `overrideLeases` di `instance.stop_all`, `instance.preview_stop_all`, dan `agent_off`, `environments` dan `affectedEnvironments` di preview, `overrideLeases` dan `leases` di `instance.change_endpoints` (spec 0047 AC-14 sampai AC-17; build step 6; after build steps 2 dan 4)
  - [ ] `stackly env down` dan flag override: `env down` dengan preview dan `--wait`, baris `lease:` di `env status`, `--override-leases` di `instance stop`, `stop --all`, `endpoint set`, dan `daemon uninstall`, golden dan binary test (spec 0047 AC-18, AC-19, AC-22 bagian CLI; build step 7; after build step 4 untuk golden; binary test needs build steps 5 dan 6; bisa paralel dengan milestone berikutnya)
  - [ ] Down, lease, dan override di app: tombol Down dan konfirmasi, banner dan umpan balik, Activity, baris Lease dan "· lease", konfirmasi override untuk semua permukaan Stop, sheet port, popover Stop All dan Turn Off, snapshot, lalu evidence native dan langkah manual yang butuh mata dan tangan kamu (spec 0047 AC-20 sampai AC-24, AC-26; build steps 8 to 10; after build step 4; needs milestone app spec 0044 dan spec 0046; evidence manual needs build steps 5 dan 6)
- [ ] Verify it: `/check verify env down, shared instance & lease`
- [ ] Test it: `/test env down, shared instance & lease`
