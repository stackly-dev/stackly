# Scope 07 · Stack enam service

[Kembali ke index](index.md)

Lima provider wajib dibangun penuh: NATS dilanjutkan dari fitur 23, lalu Valkey, Traefik, MySQL, dan ClickHouse. Manifest dan template dibuat lewat SDK dari fase 06; probe native atau adapter bawaan yang dibutuhkan provider ditambahkan ke core lewat review, dan jumlah perubahan core per provider dicatat sebagai ukuran RISK-07. Tiap provider menjalani journey yang sama dengan PostgreSQL: catalog, install, create, start, readiness, connection, dan dua instance yang terisolasi. Fase ini selesai kalau setiap provider lulus bagian lifecycle, readiness, persistence, dan endpoint dari matrix minimum di PRD bagian 32.2 tanpa label supported palsu; bagian backup dan restore menyusul di fase 09.

### 39. Provider NATS & JetStream · in-progress

Lanjutan dari fitur 23: JetStream on/off, `store_dir` per instance, monitoring di loopback, credential, server name unik, batas resource, dan dua instance.

**Done when:** dua instance NATS dengan JetStream dan `store_dir` berbeda hanya mengakses stream miliknya setelah restart; readiness mencakup konektivitas dan mode JetStream yang diminta; monitoring ada di endpoint milik instance itu dan tidak terbuka ke LAN; port monitoring yang bentrok menunjuk endpoint yang tepat tanpa menghentikan proses luar.

**Trace:** REQ-036, AC-010, AC-036, AC-077 (sebagian), PRD bagian 22.2

**Pointer:** spec [0039](../specs/0039-provider-nats-jetstream/index.md)

- [x] Design it (spec): `/architect provider NATS & JetStream`
- [ ] Build it: `/develop provider NATS & JetStream`
  - [ ] Paket `1.1.0` dan escaper, di fixture: escaper `natsConf` (`"` dan `\` di escape, `$` literal) di `crates/core/src/provider/template.rs`, lalu paket `providers/nats` revision `1.1.0` (password client lewat secret `clientPassword`, akun `APP` dan `STACKLY_READINESS` dengan `no_auth_user`, `nats.conf` sensitif di `secrets/`, batas MiB dan KiB, `tests/scenarios.yaml`, README), golden, fixture form dan catalog, revision catalog, entri corpus `compat/nats/1.1.0/`, dan test Swift yang mengikuti fixture (spec 0039 AC-1, AC-2 bagian fixture, AC-9 schema, AC-10 file skenario, AC-11 fixture dan Swift, AC-12 catalog dan corpus; build step 1 sampai 3; step 1 boleh masuk `main` sendiri; step 3 after step 2, boleh sesi Swift terpisah; `crates/core/tests/provider_packages.rs` dipakai bersama fitur 41 milestone 1)
  - [ ] NATS 2.15.1 asli lewat daemon: harness dan client uji NATS di `apps/daemon/tests/nats/`, `nats_auth_in_process` (auth, batas izin tanpa credential, percobaan suntik balasan, kebocoran password, readiness tanpa bacaan secret), check baru `client_auth`, `isolation`, `persistence`, dan `monitoring_conflict` di `nats_compatibility`, konformitas dua skenario, dan `nats_journey` CLI (spec 0039 AC-2 bagian native, AC-3 sampai AC-11; build step 4 sampai 7; after milestone 1; needs artefak `nats-2.15.1-macos-aarch64` di cache xtask, selain itu `BLOCKED`)
  - [ ] Evidence, catalog, dan ukuran RISK-07: run native dari `HEAD` bersih, `cargo xtask runtime compat-record`, satu commit evidence dengan `versions.json`, ledger catalog, dan golden status dukungan, lalu catatan RISK-07 dan langkah manual di `verify.md` (spec 0039 AC-12, AC-13, AC-14, AC-11 bagian GUI; build step 8 dan 9; after milestone 2; step 2 sampai 8 masuk `main` bersama dalam satu giliran catalog, karena paket `1.1.0` tanpa evidence menurunkan status NATS ke `experimental`; run launchd AC-13 dan pemeriksaan GUI AC-11 adalah langkah manual)
- [ ] Verify it: `/check verify provider NATS & JetStream`
- [ ] Test it: `/test provider NATS & JetStream`

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

### 43. Provider ClickHouse · in-progress

Binary native yang dipilih, path storage, temp, log, user files, dan config per instance, port HTTP dan native TCP, auth, serta readiness lewat query. Cluster, replikasi, dan Keeper tidak diaktifkan.

**Done when:** dua instance dengan tabel bernama sama berisi data berbeda tidak tercampur setelah restart; seluruh listener aktif cocok dengan registry; jalur Windows lewat WSL tidak pernah diberi label native.

**Trace:** REQ-037, AC-037, AC-074, PRD bagian 22.3

**Pointer:** spec [0043](../specs/0043-provider-clickhouse/index.md)

- [x] Design it (spec): `/architect provider ClickHouse`
- [ ] Build it: `/develop provider ClickHouse`
  - [ ] Core dan paket tanpa binary: format config `xml`, filter template `sha256Hex` (dilewati kalau fitur 40 sudah memasukkannya), paket `providers/clickhouse` dengan `versions.json` dari codegen, golden plan, render, connection, dan open action, test key yang dilarang dan test platform AC-074, lalu fixture form dan entri `index.json` (spec 0043 AC-1 sampai AC-3, AC-4 bagian template, AC-6 bagian render, AC-11 bagian render, AC-13, AC-16 bagian fixture; build step 1 sampai 5; needs update spec 0009 *Perluasan additive* untuk `xml` dan `sha256Hex`; needs fitur 36, sudah done; step 2 after filter fitur 40 kalau fitur 40 lebih dulu)
  - [ ] ClickHouse 26.8.30.8 asli lewat daemon: harness `apps/daemon/tests/clickhouse.rs`, lifecycle dan kontrak proses, isolasi dua instance dengan tabel sama nama, audit listener, auth dan dua user terkelola, reload lewat `SYSTEM RELOAD CONFIG` dengan rotasi password, kegagalan port dan folder data, lalu konformitas dan install dari cache (spec 0043 AC-4 sampai AC-12, AC-14; build step 6 sampai 9; after milestone 1; needs artefak ClickHouse di cache xtask, tanpa itu `BLOCKED`; step 6 sampai 8 bisa dibagi sesudah harness step 6 masuk)
  - [ ] Catalog, corpus, spike, dan ukuran RISK-07: hapus recipe spike dan penanda `F43`, record BR-025 lewat `clickhouse_compatibility` dan `compat-record`, `official/clickhouse` pindah ke `[[provider]]` dengan catatan platform dan revision catalog baru, `cargo xtask provider freeze`, dan catatan ukuran RISK-07 serta tiga langkah manual di `verify.md` (spec 0043 AC-13 bagian catalog, AC-15 sampai AC-17; build step 10 sampai 13; step 11 dan 12 after milestone 2 lulus; step 10 bisa paralel dengan milestone 2; run spike native, fuzz 10 menit, dan sesi GUI adalah langkah manual)
- [ ] Verify it: `/check verify provider ClickHouse`
- [ ] Test it: `/test provider ClickHouse`
