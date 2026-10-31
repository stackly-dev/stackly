# Scope: Stackly

Stackly adalah aplikasi open source untuk memasang, membuat instance, menjalankan, mengatur, dan memantau developer tools native di satu Mac, dengan core, daemon, dan CLI berbasis Rust serta GUI SwiftUI. Penggunanya developer dan tim internal yang butuh banyak versi dan banyak instance database secara lokal, tanpa Docker dan tanpa akun cloud.

**Build approach:** Journey (satu user journey dibangun lengkap, termasuk semua state, error, dan polish, sebelum journey berikutnya dimulai).

**Workflow:** Beta (setelah `/develop`: `/check verify`, lalu `/test`). Tingkat rigor default proyek. Fitur bertag `· GA` juga menjalani `/check review` dengan model baru, lalu `/document`. `/architect` disarankan sebagai langkah pertama untuk fitur yang punya keputusan nyata, tapi boleh kamu lewati kalau cara membangunnya sudah jelas.

**Cakupan:** baseline rilis stabil macOS (macOS 27, Apple Silicon). Windows, Linux, public registry, dan katalog tambahan ada di [Deferred](#deferred).

**Sumber:** `prd.md` versi 1.0.0 (15 Oktober 2026). ID `REQ`, `BR`, `VAL`, `NFR`, `AC`, `PAGE`, `JOURNEY`, dan `OQ` dari PRD dipertahankan di baris **Trace** tiap fitur supaya traceability tetap utuh; sebagian `RISK` dan `GOAL` juga dicantumkan di fitur yang paling langsung menanganinya.

**Desain UI:** mockup Claude Design di `docs/mockups/Stackly.dc.html` (16 Oktober 2026), bahasa visual native macOS. Di seluruh scope ini disebut **mockup UI**. Aturan fidelity, selisih yang sudah diketahui, dan peta layar ke fitur pemiliknya ada di [ui-macos.md](ui-macos.md).

_Ini rekomendasi supaya build kamu tetap tertib, bukan kewajiban. Lewati apa pun yang tidak cocok: kalau kamu sudah tahu cara membangun sebuah fitur, langsung pakai `/develop` dan lewati `/architect`. Kamu yang menentukan kapan sebuah fitur `done`._

## Urutan file

Bangun dari atas ke bawah. Nomor file adalah urutan development yang disarankan, dan nomor fitur berlanjut lintas file. `ui-macos.md` tidak bernomor karena jalur paralel: fiturnya dikerjakan berdampingan dengan Foundation (lihat [Catatan urutan](#catatan-urutan)).

| File | Fase | Fitur | Status |
| --- | --- | --- | --- |
| [01-foundation.md](01-foundation.md) | Foundation dan Skeleton | 1 sampai 11, 14 | 12 done |
| [ui-macos.md](ui-macos.md) | UI & desain macOS (jalur paralel SwiftUI) | 12, 13, 67 sampai 69 | 5 done |
| [02-instalasi-pertama.md](02-instalasi-pertama.md) | Instalasi pertama (JOURNEY-01, PostgreSQL, plus NATS dasar) | 15 sampai 23 | 7 in-progress, 2 done |
| [03-multi-versi-dan-instance.md](03-multi-versi-dan-instance.md) | Multi versi & instance (JOURNEY-02, JOURNEY-03) | 24 sampai 25 | 2 done |
| [04-kerja-harian.md](04-kerja-harian.md) | Kerja harian | 26 sampai 32, 70 | 8 planned |
| [05-saat-instance-gagal.md](05-saat-instance-gagal.md) | Saat instance gagal (JOURNEY-05) | 33 sampai 35 | 3 planned |
| [06-menambah-provider.md](06-menambah-provider.md) | Menambah provider (JOURNEY-07) | 36 sampai 38 | 3 planned |
| [07-stack-enam-service.md](07-stack-enam-service.md) | Stack enam service | 39 sampai 43 | 5 planned |
| [08-environment-proyek.md](08-environment-proyek.md) | Environment proyek (JOURNEY-04) | 44 sampai 47 | 4 planned |
| [09-backup-dan-restore.md](09-backup-dan-restore.md) | Backup & restore (JOURNEY-06) | 48 sampai 53 | 6 planned |
| [10-merawat-runtime-dan-data.md](10-merawat-runtime-dan-data.md) | Merawat runtime & data | 54 sampai 59 | 6 planned |
| [11-distribusi-aplikasi.md](11-distribusi-aplikasi.md) | Distribusi aplikasi | 60 sampai 62 | 3 planned |
| [12-siap-rilis-stabil.md](12-siap-rilis-stabil.md) | Siap rilis stabil | 63 sampai 66 | 4 planned |

## At a glance

| # | Feature | Phase | Status |
| --- | --- | --- | --- |
| 1 | Stack & arsitektur | Foundation | done |
| 2 | Coding standards & tooling | Foundation | done |
| 3 | Sumber & packaging runtime | Foundation | done |
| 4 | Domain core & platform abstraction | Foundation | done |
| 5 | Data model registry | Foundation | done |
| 6 | Protokol IPC & daemon | Foundation | done |
| 7 | Operation engine | Foundation | done |
| 8 | CLI client & kontrak output | Foundation | done |
| 9 | Provider engine & manifest schema | Foundation | done |
| 10 | Process supervisor | Foundation | done |
| 11 | Secret store & redaction | Foundation | done |
| 12 | Design system & UI foundation | Foundation | done |
| 13 | App shell SwiftUI & Swift client | Foundation | done |
| 14 | Walking skeleton | Skeleton | done |
| 15 | Background agent & CLI install | Instalasi pertama | in-progress |
| 16 | Onboarding | Instalasi pertama | in-progress |
| 17 | Catalog & provider detail | Instalasi pertama | done |
| 18 | Install runtime managed | Instalasi pertama | in-progress |
| 19 | Provider PostgreSQL | Instalasi pertama | done |
| 20 | Create instance | Instalasi pertama | in-progress |
| 21 | Start, readiness & connection | Instalasi pertama | in-progress |
| 22 | Overview & daftar instance | Instalasi pertama | in-progress |
| 23 | Provider NATS lewat manifest | Instalasi pertama | in-progress |
| 24 | Instance kedua dari runtime yang sama | Multi versi & instance | done |
| 25 | PostgreSQL 17 & 18 berdampingan | Multi versi & instance | done |
| 26 | Stop, restart & reload | Kerja harian | planned |
| 27 | Log per instance | Kerja harian | planned |
| 28 | Metrics & health | Kerja harian | planned |
| 29 | Edit config, nama & endpoint | Kerja harian | planned |
| 30 | Reconciliation & restart daemon terencana | Kerja harian | planned |
| 31 | Demand, autostart & login | Kerja harian | planned |
| 32 | Menu bar & Settings | Kerja harian | planned |
| 33 | Crash loop & error start yang aman | Saat instance gagal | planned |
| 34 | Doctor & diagnostic bundle | Saat instance gagal | planned |
| 35 | Operations, activity & audit | Saat instance gagal | planned |
| 36 | Provider SDK & test harness | Menambah provider | planned |
| 37 | Import provider lokal & trust | Menambah provider | planned |
| 38 | Panduan kontributor & governance | Menambah provider | planned |
| 39 | Provider NATS & JetStream | Stack enam service | planned |
| 40 | Provider Valkey | Stack enam service | planned |
| 41 | Provider Traefik & routing lokal | Stack enam service | planned |
| 42 | Provider MySQL | Stack enam service | planned |
| 43 | Provider ClickHouse | Stack enam service | planned |
| 44 | Manifest environment: validate & plan | Environment proyek | planned |
| 45 | Lockfile & override lokal | Environment proyek | planned |
| 46 | Env up & dependency graph | Environment proyek | planned |
| 47 | Env down, shared instance & lease | Environment proyek | planned |
| 48 | Backup metadata registry | Backup & restore | planned |
| 49 | Cold backup & kontrak backup | Backup & restore | planned |
| 50 | Backup logical PostgreSQL | Backup & restore | planned |
| 51 | Restore as new & verifikasi | Backup & restore | planned |
| 52 | Halaman Backup & Restore | Backup & restore | planned |
| 53 | Backup MySQL, NATS, ClickHouse, Valkey & Traefik | Backup & restore | planned |
| 54 | Remove, retain & purge | Merawat runtime & data | planned |
| 55 | Hapus runtime & garbage collection | Merawat runtime & data | planned |
| 56 | Runtime external (Homebrew) | Merawat runtime & data | planned |
| 57 | Scan data orphan & adoption | Merawat runtime & data | planned |
| 58 | Rebind runtime kompatibel | Merawat runtime & data | planned |
| 59 | Update provider revision | Merawat runtime & data | planned |
| 60 | Packaging, signing & notarization | Distribusi aplikasi | planned |
| 61 | Update aplikasi & kompatibilitas | Distribusi aplikasi | planned |
| 62 | Uninstall & reinstall dengan data tetap | Distribusi aplikasi | planned |
| 63 | Security review & fuzzing | Siap rilis stabil | planned |
| 64 | Fault injection, recovery & soak | Siap rilis stabil | planned |
| 65 | Quality gates & release report | Siap rilis stabil | planned |
| 66 | Runbook & dokumentasi pengguna | Siap rilis stabil | planned |
| 67 | Komponen dasar Stackly | Foundation | done |
| 68 | Fixture tampilan & cek visual | Foundation | done |
| 69 | Menu aplikasi, perintah & shortcut keyboard | Foundation | done |
| 70 | Buka instance di aplikasi klien | Kerja harian | planned |

## Catatan urutan

- **Mulai risiko besar lebih awal.** Fitur 3 (sumber runtime) bisa kamu kerjakan bersamaan dengan fitur 1 dan 2, karena hasilnya menentukan fitur 18.
- **Helper dan signing dibuktikan di fase 02.** Bundling, signing development, dan registrasi helper dibuktikan di fitur 15; fitur 60 hanya menambah notarization, installer, pipeline rilis, dan third party notices. Minta signing account (OQ-012) ke release owner sejak awal, karena PRD menjadikannya release blocker (RISK-10).
- **Engine dibuktikan pada provider kedua sejak awal.** NATS dasar lewat manifest saja (fitur 23) dijalankan sebelum fase 04, dan schema Traefik dengan routes berbentuk daftar sudah divalidasi di fitur 9 lalu dipakai sebagai uji form di fitur 29. Ini menjawab RISK-07 sesuai PRD bagian 33.2.
- **Jalur SwiftUI paralel.** Fitur 12, 13, 67, 68, dan 69 di [ui-macos.md](ui-macos.md) bisa dikerjakan engineer SwiftUI segera setelah kontrak IPC (fitur 6) stabil, memakai contract fixtures, dengan urutan 12, 13, lalu 67 sampai 69. Fitur 13 tetap dibutuhkan walking skeleton (fitur 14), dan 67 sampai 69 sebaiknya selesai sebelum layar pertama di fase 02 supaya semua layar dirakit dari komponen, fixture, dan peta perintah yang sama. Setelah itu setiap journey membawa layarnya sendiri sesuai peta layar di file itu, jadi GUI dan CLI tetap setara per fase.
- **Mockup UI mengikuti scope, bukan sebaliknya.** Mockup mengunci tampilan dan state; kalau perilakunya berbeda dengan scope atau spec, scope dan spec yang menang. Hanya fitur 70 yang lahir dari mockup dan belum ada di PRD.
- **Jalur kontributor sebelum lima provider.** Fase 06 sengaja datang sebelum fase 07, supaya NATS, Valkey, Traefik, MySQL, dan ClickHouse dibangun lewat SDK dan harness yang sama dengan kontributor (JOURNEY-07, REQ-029, gate G1).
- **Enam provider sebelum environment.** Environment ERP di fase 08 memakai keenam provider, jadi fase 07 harus selesai lebih dulu.
- **Checkpoint performa ringan di akhir fase 04.** Ukur NFR-001, NFR-003, dan NFR-006 sekali dengan fixture 100 instance terdaftar dan 10 berjalan, supaya masalah ketahuan sebelum IPC, registry, dan GUI membeku; angka final tetap diukur di fitur 64.
- **Fuzzing sejak awal.** Tiap fitur 6, 9, 18, 20, dan 54 membawa fuzz target di Done when dan menjalankannya di Mac ini; fitur 63 memperluas dan menutupnya.
- **Pilot bukan fitur.** Pilot internal (gate G7) dijalankan setelah fitur 66, dengan backup teruji, dan hasilnya dicatat lewat `/scope` berikutnya. Peserta dan kapasitasnya (OQ-016) diputuskan product owner saat kickoff.

## Deferred

Di luar baseline macOS, dicatat supaya rencana tetap jujur.

- **Community beta**: publikasi SDK dan docs, kontribusi source, alur review catalog · needs a decision
- **Public registry**: discovery, publishing, trust roots, sinkronisasi catalog dan revocation list online, laporan kompatibilitas · needs a decision · GA
- **Windows foundation**: IPC named pipe, adapter proses, secret, path, dan startup, serta packaging spike (OQ-014, OQ-015) · needs a decision
- **WinUI 3 beta**: client C#, UI native, parity lifecycle dan recovery untuk provider yang tersedia · needs a decision
- **Windows stable**: gate distribusi, keamanan, dan recovery untuk Windows · needs a decision · GA
- **Linux dan plugin lanjutan**: distribusi Linux, mode headless, WASM atau custom hooks · needs a decision
- **Dukungan Intel macOS**: setelah hardware dan matrix lulus (OQ-002)
- **Katalog tambahan**: MariaDB, MongoDB, RabbitMQ, Kafka, Redpanda, Redis, Memcached, Caddy, Nginx, HAProxy, MinIO, Meilisearch, Typesense, OpenSearch, OpenTelemetry Collector, Prometheus, Grafana, Mailpit, Temporal, LocalStack, dan runtime bahasa (PRD bagian 5.3) · needs a decision
- **Backup lanjutan**: PITR, WAL archive, backup terjadwal atau incremental, retention otomatis, S3 · needs a decision
- **Upgrade mayor otomatis**: otomasi `pg_upgrade` penuh dengan preflight dan rollback · needs a decision · GA
- **Jaringan lanjutan**: bind LAN, port privileged, TLS trust otomatis, perubahan DNS atau `/etc/hosts` (OQ-017) · needs a decision · GA
- **Storage eksternal**: custom data volume, external storage, dan relokasi data instance yang sudah ada (OQ-018) · needs a decision
- **Extension PostgreSQL lanjutan**: pgvector, PostGIS, PL/Perl, dan PL/Python di artefak PostgreSQL managed, masing masing dengan lisensi dan dependency closure sendiri (from spec 0003) · needs a decision

## Legend

**Kotak keputusan.** Tiap fitur bertag needs a decision punya tepat satu kotak yang labelnya berakhir dengan `(spec)`; fitur tanpa tag itu langsung punya kotak `/develop` (atau `/audit`). Bunyinya bisa berbeda (`Design it (spec)` biasanya, `Decide the stack (spec)` untuk Stack & arsitektur), jadi skill lain mencarinya lewat akhiran `(spec)`, bukan label persis. Kotak lain adalah kotak eksekusi, dan `/architect` tidak pernah mencentangnya.

**Siklus fitur:**

| State | Diatur oleh | Yang terlihat di fitur |
| --- | --- | --- |
| `planned` · needs a decision | `/scope` | satu kotak: `Design it (spec): /architect <feature>` |
| `in-progress` (sudah didesain) | `/architect` saat spec ditangkap | `Design it` dicentang, spec ditautkan, `Build it: /develop <feature>` dengan 2 sampai 5 milestone, lalu kotak penutup sesuai tier (`Verify it` mulai Alpha, `Test it` mulai Beta, `Review it` dan `Document it` untuk GA) |
| `in-progress` (sedang dibangun) | `/develop` | milestone dicentang satu per satu, pointer kode terisi |
| `in-progress` (sudah diverifikasi) | `/check verify` | `Build it` dan milestone dicentang, `Verify it` dicentang |
| `done` | kamu, saat kamu memutuskan; `/sync` merekonsiliasi | kotak yang dijalankan dicentang, yang dilewati ditandai skipped; tahap terakhir tier (Beta: setelah `/test`) adalah titik yang disarankan |

- **Langkah berikutnya** adalah kotak pertama yang belum dicentang, selalu berupa command atau milestone.
- **needs a decision**: jalankan `/architect` dulu; tanpa tag ini langsung `/develop` (atau `/audit` untuk standards & tooling). Tag ini hilang setelah spec ditangkap.
- **Tugas build yang rinci** tinggal di `## Build plan` milik spec, bukan di sini. Scope hanya membawa ringkasan milestone.
- **Status**: `planned`, `in-progress`, `done`, plus `existing` (sudah ada sebelum workflow) dan `dropped` (dikeluarkan dari scope, tetap disimpan sebagai riwayat).
- **Tag tier** di samping judul (misalnya `· GA`) menaikkan rigor fitur itu di atas default Beta. Tanpa tag berarti ikut default.
- **Workflow** (baris header) adalah tahap setelah `/develop`: **Prototype** tidak ada; **Alpha** `/check verify`; **Beta** `/check verify` lalu `/test`; **GA** ditambah `/check review` dengan model baru lalu `/document`. Fitur yang dibangun di atas keputusan yang belum diratifikasi (spec `Assumed`) tetap ditandai, tapi tidak menghalangi `done`.
- **Trace**: ID dari `prd.md` yang dicakup fitur itu. `/architect` menurunkan requirement dan acceptance criteria lengkap ke spec dari ID ini, supaya PRD tidak perlu dibaca langsung saat implementasi.
- **Pointer** (`spec <n> · code in <path>`): link spec ditambahkan `/architect`, path kode ditambahkan `/develop`.
