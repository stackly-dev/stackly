# Scope 08 · Environment proyek

[Kembali ke index](index.md)

Journey PRD JOURNEY-04 (contoh environment ERP): import `stackly.yaml`, review trust provider, plan, lock versi exact, up dengan progress dan readiness, lalu down tanpa menghapus data. Fase ini datang setelah stack enam service karena environment ERP memakai keenamnya. Fase ini selesai kalau tim bisa berbagi satu file environment tanpa ikut membagikan data, path pribadi, atau password, dan seluruh jalur itu bekerja dari GUI dan CLI.

### 44. Manifest environment: validate & plan · needs a decision

Schema `stackly.yaml` (versi, namespace, selector runtime, endpoint, settings, secret reference, dependency, ownership) dan plan yang menampilkan diff serta dampak tanpa efek samping. Termasuk halaman Environments untuk import, edit, validasi, dan preview plan.

**Done when:** plan untuk manifest baru menampilkan install, create, dan start tanpa membuat file data, proses, perubahan trust, atau runtime; cycle, reference yang tidak ada, secret literal, dan konflik endpoint fixed ditolak sebelum ada mutasi; `stackly env validate` dan `plan` setara dengan GUI; halaman Environments bisa dipakai dengan interaksi desktop; halaman Environments dan sheet plan mengikuti mockup UI: status environment, tabel service dengan selector, instance, versi, ownership, dan status, plan dengan kata kerja Create, Start, dan Keep, serta daftar environment di sidebar dengan hitungan ready.

**Trace:** REQ-020, REQ-028, VAL-013, VAL-014, PAGE-10, AC-020, AC-080 (bagian plan), NFR-012, JOURNEY-04

- [ ] Design it (spec): `/architect manifest environment: validate & plan`

### 45. Lockfile & override lokal · needs a decision

Pisahkan manifest yang dibagikan, `stackly.lock` yang menyimpan versi provider, runtime, dan digest per platform, dan `.stackly.local.yaml` untuk override lokal. `stackly env up --locked` tidak pernah melakukan resolusi ulang.

**Done when:** lockfile tidak memuat password, home directory, atau PID; `stackly env lock` boleh memakai metadata online tapi tidak menjalankan provider; mesin tanpa artefak yang kompatibel gagal dengan jelas tanpa substitusi; override port dan storage tidak mengubah manifest milik tim; `stackly env export` tidak membawa secret.

**Trace:** REQ-023, REQ-028, BR-018, AC-023, AC-080, PRD bagian 13.1 dan 13.3

- [ ] Design it (spec): `/architect lockfile & override lokal`

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
