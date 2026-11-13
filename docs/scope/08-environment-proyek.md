# Scope 08 · Environment proyek

[Kembali ke index](index.md)

Journey PRD JOURNEY-04 (contoh environment ERP): import `stackly.yaml`, review trust provider, plan, lock versi exact, up dengan progress dan readiness, lalu down tanpa menghapus data. Fase ini datang setelah stack enam service karena environment ERP memakai keenamnya. Fase ini selesai kalau tim bisa berbagi satu file environment tanpa ikut membagikan data, path pribadi, atau password, dan seluruh jalur itu bekerja dari GUI dan CLI.

### 44. Manifest environment: validate & plan · in-progress

Schema `stackly.yaml` (versi, namespace, selector runtime, endpoint, settings, secret reference, dependency, ownership) dan plan yang menampilkan diff serta dampak tanpa efek samping. Termasuk halaman Environments untuk import, edit, validasi, dan preview plan.

**Done when:** plan untuk manifest baru menampilkan install, create, dan start tanpa membuat file data, proses, perubahan trust, atau runtime; cycle, reference yang tidak ada, secret literal, dan konflik endpoint fixed ditolak sebelum ada mutasi; `stackly env validate` dan `plan` setara dengan GUI; halaman Environments bisa dipakai dengan interaksi desktop; halaman Environments dan sheet plan mengikuti mockup UI: status environment, tabel service dengan selector, instance, versi, ownership, dan status, plan dengan kata kerja Create, Start, dan Keep, serta daftar environment di sidebar dengan hitungan ready; menu aksi per service di tabel service memakai `OpenInMenu` dari [spec 0070](../specs/0070-buka-instance-aplikasi-klien/index.md) (fitur 70), ditambah Copy Connection URL, Show Instance, Show Logs, dan Start atau Stop seperti mockup.

**Trace:** REQ-020, REQ-028, VAL-013, VAL-014, PAGE-10, AC-020, AC-080 (bagian plan), NFR-012, JOURNEY-04

**Pointer:** spec [0044](../specs/0044-manifest-environment-validate-plan/index.md)

- [x] Design it (spec): `/architect manifest environment: validate & plan`
- [ ] Build it: `/develop manifest environment: validate & plan`
  - [ ] Manifest, validate, dan plan di daemon: migrasi `environments.services` dan unique index nama serta lokasi, `DeclaredService` dan dua `OperationKind`, loader dan aturan validasi `stackly.yaml` dengan JSON Schema hasil codegen, plan di atas `gather` dan `evaluate` create (`planned_runtime`), kenaikan minor protokol dengan modul `env`, enam method, area kode `env`, `InstanceOwnership` baru, lalu handler `env.validate`, `env.plan`, `env.list`, `env.get`, operasi `import_environment` dan `remove_environment`, cache keanggotaan, dan test tanpa efek samping (spec 0044 AC-1 sampai AC-15, AC-24; build steps 1 to 5; needs scope 01 sampai 07 di `main`)
  - [ ] `stackly env validate`, `plan`, `import`, `list`, `remove`: grup `env`, resolve target, kode `cli.env_invalid`, `cli.env_plan_blocked`, `cli.environment_not_found`, kolom `ENVIRONMENT` di `instance list`, golden dan binary test (spec 0044 AC-16, AC-17, AC-15; build step 6; after build step 4 untuk golden; binary test needs build step 5; bisa paralel dengan milestone berikutnya)
  - [ ] Environments di app: `EnvironmentStore`, baris anak sidebar, halaman daftar dan detail dengan tabel Services dan menu baris (`OpenInMenu`), banner manifest dan partially ready, sheet plan, Import…, Validate, Reload, Remove, seksi Environments di Overview, kolom dan baris Environment di Instances, baris onboarding, fixture `environment-partially-ready`, snapshot, lalu evidence dan langkah manual yang butuh mata dan tangan kamu (spec 0044 AC-18 sampai AC-25; build steps 7 to 11; after build step 4; evidence manual needs build step 5)
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

### 46. Env up & dependency graph · needs a decision

Up memasang yang dibutuhkan, membuat instance dedicated atau meminjam instance yang ada lewat `instanceRef` (tanpa mengubah config, auth, port, atau autostart miliknya), mengambil lease, lalu menjalankan dependency graph berdasarkan readiness dan timeout. Partial failure menghasilkan state yang jelas, dan kompensasi hanya menyentuh resource milik operasi itu.

**Done when:** dependency yang tidak ready membuat dependent tidak ditandai sukses; instance yang sudah ada dan sehat tidak dihapus; up dengan `instanceRef` menambah lease tanpa membuat instance baru; secret reference yang belum diisi membuat up berhenti dengan status needs secret yang menunjuk reference itu, tidak pernah diganti password kosong; operasi menunjukkan tahap yang gagal dan langkah recovery; `stackly env up` dan `stackly env status` menampilkan port auto yang sebenarnya dipakai; graph diuji dengan provider fixture tanpa dependency palsu antar database; banner partially ready di halaman Environments menunjuk service yang belum ready beserta alasannya, seperti mockup UI.

**Trace:** REQ-021, REQ-028, BR-023, VAL-014, AC-021, AC-073 (bersama fitur 47), PRD bagian 13.1 dan 13.2

- [ ] Design it (spec): `/architect env up & dependency graph`

### 47. Env down, shared instance & lease · needs a decision

Down melepas lease environment dan hanya menghentikan instance yang tidak dibutuhkan lagi, memakai aturan evaluasi demand dari fitur 31. Stop manual atau Stop Services yang mengganggu environment lain ditolak atau meminta override dengan dampak yang jelas.

**Done when:** saat environment A down, lease B tetap ada dan instance tetap ready; up A dan down B yang bersamaan menghasilkan lease konsisten tanpa create ganda; lease disimpan per identitas environment, bukan per koneksi client, jadi menutup CLI atau GUI tidak melepas lease; Stop Services dari menu bar menampilkan lease environment yang terdampak; ganti port atau endpoint pada instance yang dipinjam environment dengan lease aktif ditolak atau meminta override dengan dampak yang jelas; `stackly env down` tidak pernah menghapus data atau runtime; konfirmasi Down menjelaskan instance mana yang tetap jalan (shared atau dimulai manual), dan baris Lease di halaman Environments mengikuti mockup UI.

**Trace:** REQ-022, REQ-028, BR-006, BR-014, BR-015, VAL-014, AC-022, AC-073 (bersama fitur 46), PRD bagian 13.2

- [ ] Design it (spec): `/architect env down, shared instance & lease`
