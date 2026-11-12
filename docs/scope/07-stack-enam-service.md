# Scope 07 · Stack enam service

[Kembali ke index](index.md)

Lima provider wajib dibangun penuh: NATS dilanjutkan dari fitur 23, lalu Valkey, Traefik, MySQL, dan ClickHouse. Manifest dan template dibuat lewat SDK dari fase 06; probe native atau adapter bawaan yang dibutuhkan provider ditambahkan ke core lewat review, dan jumlah perubahan core per provider dicatat sebagai ukuran RISK-07. Tiap provider menjalani journey yang sama dengan PostgreSQL: catalog, install, create, start, readiness, connection, dan dua instance yang terisolasi. Fase ini selesai kalau setiap provider lulus bagian lifecycle, readiness, persistence, dan endpoint dari matrix minimum di PRD bagian 32.2 tanpa label supported palsu; bagian backup dan restore menyusul di fase 09.

### 39. Provider NATS & JetStream · needs a decision

Lanjutan dari fitur 23: JetStream on/off, `store_dir` per instance, monitoring di loopback, credential, server name unik, batas resource, dan dua instance.

**Done when:** dua instance NATS dengan JetStream dan `store_dir` berbeda hanya mengakses stream miliknya setelah restart; readiness mencakup konektivitas dan mode JetStream yang diminta; monitoring ada di endpoint milik instance itu dan tidak terbuka ke LAN; port monitoring yang bentrok menunjuk endpoint yang tepat tanpa menghentikan proses luar.

**Trace:** REQ-036, AC-010, AC-036, AC-077 (sebagian), PRD bagian 22.2

- [ ] Design it (spec): `/architect provider NATS & JetStream`

### 40. Provider Valkey · needs a decision

Config, bind dan auth, PING, direktori persistent, kebijakan RDB/AOF, log, dan stop, dengan penjelasan jujur soal mode volatile versus durable.

**Done when:** dengan persistence aktif, fixture pulih setelah shutdown normal; mode volatile menampilkan peringatan bahwa data bisa hilang dan tidak menjanjikan restore; Valkey tidak pernah diganti Redis atau container tanpa sepengetahuan pengguna.

**Trace:** REQ-038, AC-038, AC-077 (sebagian), PRD bagian 22.4

- [ ] Design it (spec): `/architect provider Valkey`

### 41. Provider Traefik & routing lokal · in-progress

Entrypoint loopback, routing lewat file provider dengan namespace per instance, health endpoint terpisah, validasi, dan apply yang aman. HTTPS hanya aktif dengan certificate yang diberikan pengguna.

**Done when:** mengubah route di instance A hanya mengubah request di A sementara B tetap jalan; edit route memakai halaman Configuration dari fitur 29 (form typed bila schema mendukung, selain itu advanced view) dengan dampak reload atau restart ditampilkan; config yang tidak valid tidak mengganti last known good; dashboard insecure mati secara default; tidak ada perubahan root CA, DNS, atau `/etc/hosts`.

**Trace:** REQ-039, AC-039, PRD bagian 22.5

**Pointer:** spec [0041](../specs/0041-provider-traefik-routing-lokal/index.md)

- [x] Design it (spec): `/architect provider Traefik & routing lokal`
- [ ] Build it: `/develop provider Traefik & routing lokal`
  - [ ] Paket resmi: blok `connection`, entri `openWith` `routers`, remedy `routes_not_loaded`, `tests/scenarios.yaml` (dua skenario), README, lalu golden, fixture form, seed fuzz, dan test fixture render tanpa `tls` dan dashboard mati (spec 0041 AC-1, AC-6, AC-7 bagian statis, AC-13; build step 1 sampai 3; needs fitur 9, 21, 34, 36, 70, sudah done; menyentuh empat file test core bersama)
  - [ ] Routing terbukti pada Traefik 3.7.15 asli: `apps/daemon/tests/traefik/routing.rs`, `surface.rs`, `watch.rs`, dan run konformitas dengan skenario paket; `BLOCKED` tanpa artefak di cache xtask (spec 0041 AC-2 sampai AC-8, AC-11 bagian konformitas, AC-13; build step 4 sampai 7; after milestone 1; needs fitur 29, sudah done)
  - [ ] Evidence, catalog, dan handover: `traefik_compatibility` dengan `compat-record` dan `codegen`, `official/traefik` ke `[[provider]]` dengan `catalog check --update` dan `provider freeze`, test launchd Traefik lalu baris `HANDOVER_PROVEN`; run launchd setelah persetujuan kamu, selain itu `BLOCKED` (spec 0041 AC-10, AC-11, AC-12, AC-14, AC-7; build step 8 sampai 10; after milestone 2; needs fitur 17, 18, 30, sudah done; file bersama satu sesi pada satu waktu, nomor revision catalog dari lead)
  - [ ] Journey dari CLI dan GUI: `apps/daemon/tests/traefik/cli.rs` dan langkah manual di `verify.md` (spec 0041 AC-9, AC-13, AC-14; build step 11 dan 12; after milestone 3; langkah GUI manual)
- [ ] Verify it: `/check verify provider Traefik & routing lokal`
- [ ] Test it: `/test provider Traefik & routing lokal`

### 42. Provider MySQL · needs a decision

Path data, config, temp, dan log yang terisolasi, bind dan auth eksplisit, readiness lewat query terautentikasi berhak minimum, serta shutdown terkontrol sesuai versi MySQL yang dipilih.

**Done when:** dua instance MySQL dengan data dan port berbeda hanya membaca sentinel miliknya setelah restart; runtime tidak bisa dipakai pada data yang tidak kompatibel karena preflight kompatibilitas menolaknya; data yang sudah ada selalu ditolak sampai alur adoption tersedia di fitur 57.

**Trace:** REQ-055 (tanpa bagian adoption, yang menyusul di fitur 57), AC-081 (tanpa bagian restore, yang menyusul di fase 09), PRD bagian 22.6

- [ ] Design it (spec): `/architect provider MySQL`

### 43. Provider ClickHouse · needs a decision

Binary native yang dipilih, path storage, temp, log, user files, dan config per instance, port HTTP dan native TCP, auth, serta readiness lewat query. Cluster, replikasi, dan Keeper tidak diaktifkan.

**Done when:** dua instance dengan tabel bernama sama berisi data berbeda tidak tercampur setelah restart; seluruh listener aktif cocok dengan registry; jalur Windows lewat WSL tidak pernah diberi label native.

**Trace:** REQ-037, AC-037, AC-074, PRD bagian 22.3

- [ ] Design it (spec): `/architect provider ClickHouse`
