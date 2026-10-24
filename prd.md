# PRD — Stackly: Native Development Environment Manager

Rust Core · Rust Daemon & CLI · SwiftUI untuk macOS · WinUI 3 untuk Windows

| Metadata | Nilai |
| --- | --- |
| Nama final produk | **Stackly** |
| Slug | `stackly` |
| PRD ID | `PRD-{NNN}`; ditentukan setelah memeriksa repository |
| Versi dokumen | `1.0.0` |
| Tanggal | 15 Oktober 2026 |
| Status | **DRAFT / PROPOSED SCOPE — siap untuk review dan perencanaan implementasi** |
| Peran dokumen | Kebutuhan awal; agent pelaksana mengikuti scope dan specs yang berlaku |
| Target pertama | Aplikasi macOS untuk penggunaan development sehari-hari secara internal |
| Target berikutnya | Publikasi stabil dan ekosistem komunitas; aplikasi Windows native dengan WinUI 3 |
| Minimum OS | macOS 27; Windows 11 versi 26H2 |
| Bahasa core | Rust |
| GUI pertama | SwiftUI, tanpa Tauri, Electron, atau webview sebagai UI utama |
| Model eksekusi | Native process, per-user, daemon terpisah dari GUI |
| Penyimpanan metadata | SQLite lokal; bukan database layanan yang sedang dikelola |
| Provider awal wajib | PostgreSQL, MySQL, NATS Server + JetStream, ClickHouse, Valkey, Traefik |
| Status implementasi | **Belum diaudit atau diimplementasikan melalui dokumen ini** |
| Status pengujian | **NOT_RUN**; verifikasi dokumentasi bukan bukti bahwa integrasi sudah lulus pengujian |
| Sumber persyaratan | Keputusan dalam percakapan pengguna; asumsi tambahan diberi label usulan atau OQ |

> **Definisi production dalam PRD ini:** Stackly siap digunakan secara andal untuk mengelola lingkungan development lokal. Ini **bukan** sertifikasi bahwa seluruh database yang dijalankan menjadi platform production bisnis, highly available, atau memiliki SLA server produksi.
>
> **Batas isolasi:** setiap instance mempunyai data, konfigurasi, endpoint, identitas proses, dan lifecycle terpisah. Ini adalah **isolasi operasional/logis**, bukan sandbox keamanan seperti VM. Instance native dengan user OS yang sama tetap berbagi kernel, CPU, memori, disk, dan hak akses user tersebut.
>
> **Batas dukungan:** semua fitur di dokumen ini adalah target implementasi. Label `SUPPORTED` hanya boleh diberikan setelah kombinasi provider, versi, OS, dan arsitektur lulus pengujian. Dukungan macOS tidak otomatis berarti dukungan Windows native.

## Peta dokumen

| Bagian | Isi |
| --- | --- |
| [Petunjuk penggunaan PRD](#petunjuk-prd) | Penyimpanan, tiga lapisan, status, dan aturan untuk coding agent |
| [Lapisan 1 — Product Requirements / WHAT](#layer-1) | Visi, scope, provider, konsep domain, use case, fitur, aturan, UI, CLI, dan environment |
| [Lapisan 2 — Project Integration / PROJECT FIT](#layer-2) | Arsitektur Rust, monorepo, model data, IPC, supervisor, keamanan, integrasi native, dan distribusi |
| [Lapisan 3 — Verification](#layer-3) | Acceptance criteria, traceability, pengujian, milestone, timeline, dan production gates |
| [Lampiran keputusan dan risiko](#lampiran-keputusan) | Risiko, open questions, governance komunitas, dan handoff agent |
| [Referensi](#referensi) | Sumber primer yang diperiksa dan batas penggunaannya |

<a id="petunjuk-prd"></a>

## Petunjuk penggunaan PRD

### Struktur tiga lapisan

Dokumen tunggal `prd.md` ini merekam kebutuhan produk awal sebagai bahan untuk menyusun scope dan specs, dengan tiga lapisan:

1. **WHAT:** tujuan, perilaku produk, cakupan, aturan bisnis, pengguna, serta kebutuhan pengalaman pengguna.
2. **PROJECT FIT:** batas arsitektur dan cara mengintegrasikan kebutuhan dengan repository, protokol, platform, serta komponen yang tersedia.
3. **VERIFICATION:** acceptance criteria yang observable dan testable, bukti pengujian, kriteria rilis, serta Definition of Done.

Saat menyusun atau merekonsiliasi scope/specs, keputusan produk yang telah disepakati tidak boleh diganti diam-diam dengan pilihan teknologi lain. Detail implementasi yang diberi label **usulan** dapat disesuaikan melalui Architecture Decision Record (ADR), selama requirement dan keamanan tetap dipenuhi.

### Prioritas untuk pekerjaan agent berikutnya

PRD ini adalah input kebutuhan awal, bukan sumber instruksi untuk pekerjaan implementasi setelah scope dan specs tersedia. Agent pelaksana wajib mengikuti scope task yang aktif, specs yang relevan dan telah disetujui, serta `AGENTS.md`; agent tidak boleh mengambil atau menyimpulkan kebutuhan, keputusan teknis, maupun acceptance criteria langsung dari PRD ini. PRD ini hanya dirujuk ketika task secara eksplisit meminta penyusunan atau rekonsiliasi scope/specs. Jika informasi yang diperlukan belum ada atau dokumen tersebut bertentangan, ikuti workflow untuk memperbarui scope/specs atau minta keputusan; jangan diam-diam menjadikan PRD ini fallback.

### Penyimpanan di repository

Lokasi target:

```text
docs/plan/prd/{NNN}-stackly/prd.md
```

Agent wajib memeriksa direktori PRD existing. Jika PRD produk ini sudah ada, gunakan nomor dan folder tersebut. Jika belum ada, pilih nomor valid tertinggi ditambah satu dengan tiga digit; mulai `001` hanya jika belum ada nomor. Jangan menggunakan jumlah folder sebagai nomor berikutnya. Folder menggunakan lowercase kebab-case. Jika nomor melewati `999`, catat konflik aturan dan jangan memotong nomor.

Untuk memudahkan kolaborasi, dokumen boleh dipecah tanpa kehilangan ID atau traceability:

```text
docs/plan/prd/{NNN}-stackly/
├── README.md
├── prd.md                           # Dokumen konsolidasi / entry referensi
├── 01-product-requirements.md        # WHAT
├── 02-project-integration.md         # PROJECT FIT
├── 03-acceptance-criteria.md         # VERIFICATION
├── implementation-plan.md
├── traceability.md
├── compatibility-matrix.md
├── release-checklist.md
└── references.md
```

Tetapkan satu sumber kanonis ketika melakukan pemecahan agar versi konsolidasi dan dokumen terpisah tidak berbeda. File yang diserahkan ini belum disimpan ke repository dan belum mendapatkan nomor repository.

### Aturan untuk menyusun scope/spec

Bagian ini hanya berlaku jika task secara eksplisit meminta penyusunan atau rekonsiliasi scope/specs dari PRD ini; bukan instruksi bagi agent pelaksana. Untuk task tersebut, baca `AGENTS.md`, aturan workspace, scope/specs existing, dokumentasi arsitektur, dan kode yang relevan. Terapkan **REUSE > EXTEND > CREATE**. Untuk repository kosong, tulis keputusan foundation secara eksplisit; jangan menganggap modul, CI, signing identity, registry, atau aplikasi telah tersedia.

Saat menurunkan kebutuhan ke scope/specs, pertahankan ID `REQ`, `BR`, `VAL`, `NFR`, `AC`, dan `OQ` secara stabil. Status pengujian awal adalah `NOT_RUN`; hasil mock bukan bukti kompatibilitas runtime nyata. Catat `BLOCKED` jika hardware, binary, izin, signing credential, atau akses pengujian belum tersedia.

Dokumen ini **bukan** instruksi untuk memodifikasi Herd, menyalin kode proprietary, atau menggunakan REA. Produk dikembangkan sebagai implementasi independen dari kebutuhan dan dokumentasi publik.

---

<a id="layer-1"></a>

# LAPISAN 1 — PRODUCT REQUIREMENTS / WHAT

## 1. Ringkasan produk

Stackly adalah aplikasi open-source untuk menginstal, membuat instance, menjalankan, mengonfigurasi, dan memantau developer tools native pada satu mesin. Core, CLI, dan daemon ditulis dalam Rust. GUI macOS menggunakan SwiftUI; Windows ditambahkan melalui WinUI 3 yang menggunakan kontrak daemon yang sama.

Nilai utama produk bukan jumlah tool yang di-hardcode, melainkan **kontrak provider yang dapat diperluas**, **multi-version**, **multi-instance**, **perlindungan data**, dan **pengalaman penggunaan native**.

Contoh hasil yang harus dapat dicapai:

```text
Satu Mac
├── PostgreSQL 17 — pg17-dev   — 127.0.0.1:5432 — data A
├── PostgreSQL 17 — pg17-test  — 127.0.0.1:5433 — data B
├── PostgreSQL 18 — pg18-dev   — 127.0.0.1:5434 — data C
├── MySQL                  — 127.0.0.1:3306 — data D
├── NATS + JetStream          — 127.0.0.1:4222 — storage E
├── ClickHouse               — HTTP :8123 / native TCP :9000 — data F
├── Valkey                   — 127.0.0.1:6379 — data G
└── Traefik                  — HTTP :8080 / HTTPS :8443 bila dikonfigurasi
```

PostgreSQL memang mendukung beberapa instance pada satu host dengan area data dan port komunikasi berbeda. Stackly mengotomasi pengelolaan kondisi tersebut; bukan menambahkan kemampuan yang tidak dimiliki server PostgreSQL. [S01]

## 2. Masalah yang diselesaikan

Developer membutuhkan beberapa versi database atau tool untuk proyek berbeda, tetapi instalasi manual mudah menyebabkan konflik port, salah binary, campuran konfigurasi, dan upgrade data yang tidak aman. Menjalankan beberapa service dengan Homebrew juga tidak dengan sendirinya menyediakan registry instance, environment, GUI terpadu, dan kontrak provider lintas OS.

Tim membutuhkan file environment yang dapat dibagikan tanpa ikut membagikan data database, path pribadi, atau password. Kontributor komunitas membutuhkan cara menambahkan tool tanpa memahami seluruh Rust core atau menambahkan halaman GUI khusus untuk setiap layanan.

### Tujuan produk

| ID | Tujuan | Indikator keberhasilan |
| --- | --- | --- |
| GOAL-01 | Multi-version dan multi-instance yang dapat diandalkan | PostgreSQL 17 dan 18 serta dua instance versi identik berjalan bersamaan tanpa data tercampur |
| GOAL-02 | Pengelolaan native yang mudah | Pengguna dapat install, create, start, stop, inspect, dan recover melalui GUI maupun CLI |
| GOAL-03 | Provider extensible | Provider deklaratif tambahan dapat digunakan tanpa perubahan core untuk kemampuan yang sudah tersedia |
| GOAL-04 | Data pengguna terlindungi | Operasi stop, update manager, remove runtime, dan uninstall tidak diam-diam menghapus data |
| GOAL-05 | Arsitektur lintas platform | Domain dan kontrak tidak bergantung pada SwiftUI, WinUI, atau command macOS |
| GOAL-06 | Siap kolaborasi | SDK, template, validation tool, test harness, dokumentasi, dan proses review tersedia |

### Pengguna utama

| Persona | Kebutuhan |
| --- | --- |
| Developer individu | Menjalankan stack lokal, mengganti versi secara terkontrol, melihat log, menyalin connection string |
| Developer dalam tim | Memakai definisi environment yang sama dengan data dan secrets lokal masing-masing |
| Maintainer internal | Menentukan versi teruji, membagikan provider, mendiagnosis kegagalan, mengelola pembaruan |
| Kontributor provider | Menambahkan atau memperbarui tool melalui manifest, template, dan test |
| Maintainer platform | Memelihara core, security boundaries, compatibility matrix, packaging, dan release gates |

## 3. Prinsip dan keputusan produk

| Prinsip | Konsekuensi |
| --- | --- |
| Native-first, tidak mewajibkan Docker | Runtime dijalankan sebagai executable OS; backend WSL/container tidak disamarkan sebagai native |
| Daemon-centric | GUI dan CLI adalah client; tidak menjadi pemilik lifecycle database |
| Runtime berbeda dari instance | Satu instalasi versi konkret dapat dipakai banyak instance dengan storage berbeda |
| Provider-first | UI dan operasi standar digerakkan schema/capability, bukan switch berdasarkan nama tool |
| Local-first | Tidak memerlukan akun cloud; start/stop runtime yang sudah terinstal tetap bekerja offline |
| Secure defaults | Loopback, least privilege, secrets tidak masuk log, operasi destruktif eksplisit |
| Reproducible | Exact version, artifact digest, provider revision, platform, dan dependency dicatat |
| Community-friendly | Kontribusi deklaratif menjadi jalur utama; plugin executable tidak wajib untuk provider sederhana |
| Native GUI | SwiftUI macOS dahulu; WinUI 3 Windows kemudian; bukan migrasi core ke bahasa GUI |
| Evidence-first | Tidak memberi label supported/healthy/production-ready tanpa bukti yang sesuai |

## 4. Scope dan target platform

### 4.1 Baseline rilis stabil macOS

Cakupan wajib meliputi Rust core, daemon, CLI, SwiftUI, registry SQLite, provider deklaratif, enam provider awal, multi-version, multi-instance, port/data/config isolation, log, health, metrik dasar, autostart setelah login, project environment, trust policy, backup/recovery dasar, installer bertanda tangan, dan dokumentasi operasional.

**Minimum OS macOS adalah macOS 27. Apple Silicon / ARM64 adalah target validasi utama.** Dukungan Intel/x86_64 disiapkan di model runtime, tetapi tidak boleh diiklankan supported tanpa hardware/runner dan artefak provider yang diuji.

Service Management Apple menyediakan `SMAppService` untuk helper, login item, LaunchAgent, dan LaunchDaemon pada macOS 13 atau lebih baru. Rilis Stackly tetap harus menguji versi macOS yang dipilih dan penolakan izin background oleh pengguna. [S02]

### 4.2 Setelah baseline macOS

Minimum OS Windows adalah **Windows 11 versi 26H2**. WinUI 3 menggunakan C# + XAML, berkomunikasi dengan Rust daemon. WinUI 3 merupakan framework UI native dalam Windows App SDK; pilihan versi SDK stabil dan arsitektur yang didukung harus dikunci saat fase Windows. [S03]

Porting Windows mencakup platform adapter, IPC transport, process ownership, startup, filesystem, secrets, signing, installer, dan runtime provider; **bukan hanya mengganti UI**. Executable Rust dibangun ulang untuk target Windows. Source core dan kontrak yang digunakan bersama tidak berarti memakai binary macOS pada Windows.

Linux disiapkan melalui abstraksi platform, tetapi GUI dan distribusi Linux bukan syarat rilis macOS.

### 4.3 Di luar baseline

Cloud account, remote server management, kolaborasi real-time, sinkronisasi data antar-device, HA database, distributed cluster manager, Kubernetes, container/WSL backend, root daemon, privileged network helper, pengubahan DNS sistem, penginstalan root certificate otomatis, migrasi mayor database otomatis, runtime bahasa lengkap, marketplace berbayar, dan eksekusi arbitrary shell dari provider tidak masuk baseline.

Public community registry dapat berkembang setelah internal stable. Namun, format provider, dokumentasi kontribusi, local provider loading, validasi, dan test harness **harus tersedia sejak baseline** agar extensibility tidak hanya menjadi janji roadmap.

## 5. Dukungan provider

### 5.1 Arti status

| Status | Arti |
| --- | --- |
| `PLANNED` | Masuk roadmap, belum diimplementasikan |
| `UNVERIFIED` | Integrasi/artefak belum melalui seluruh pemeriksaan yang diperlukan |
| `EXPERIMENTAL` | Dapat dicoba dengan peringatan; tidak termasuk jaminan rilis stabil |
| `SUPPORTED` | Kombinasi versi, OS, arsitektur, dan capability telah lulus matrix pengujian |
| `UNSUPPORTED` | Kombinasi diketahui tidak tersedia atau tidak didukung oleh produk |
| `BLOCKED` | Ada prasyarat yang belum terpenuhi dengan alasan yang tercatat |

Status upstream dan status integrasi Stackly adalah dua hal berbeda. Pada tanggal dokumen ini, **seluruh integrasi Stackly masih PLANNED / NOT_RUN**.

### 5.2 Provider awal wajib

| Provider | Target versi awal | Kemampuan minimum | macOS | Catatan Windows |
| --- | --- | --- | --- | --- |
| PostgreSQL | Mayor 17 dan 18; patch konkret dikunci saat implementasi | Init cluster, multi-version/instance, auth, query readiness, log, persistence, logical backup/restore | Target wajib; artefak dan dependency wajib diuji | Kandidat native; lifecycle Windows didokumentasikan upstream, tetap perlu pengujian [S04] |
| MySQL | Rilis stabil konkret dipilih dan dikunci sebelum beta | Server terisolasi, multi-instance, auth, query readiness, log, persistence, backup/restore | Target wajib; artefak dan dependency wajib diuji [S22] | Kandidat native; paket, lifecycle, dan arsitektur tetap perlu pengujian [S22] |
| NATS Server | Rilis 2.x yang dipilih dan diverifikasi | Core messaging, JetStream opsional, storage per-instance, client/monitoring ports, health | Target wajib; instalasi macOS didokumentasikan [S05] | Rilis Windows tersedia menurut dokumentasi NATS; arsitektur/per-versi tetap diverifikasi [S05] |
| ClickHouse | Rilis stabil konkret yang kompatibel; tidak hardcode `latest` | Single-node server, HTTP/native TCP, config, path, health/query, persistence | Dokumentasi resmi mencakup native macOS; target wajib setelah artefak diuji [S06] | Jalur resmi yang diperiksa menggunakan WSL; **jangan klaim Windows native** [S06] |
| Valkey | Rilis stabil konkret dipilih dan dikunci sebelum beta | Standalone, port, auth, PING, persistence policy, log, data per-instance | Dokumentasi resmi mencakup macOS/Homebrew; paket konkret tetap diuji [S07] | Upstream tidak mendukung Windows native; jangan mengganti dengan WSL secara diam-diam [S07] |
| Traefik | Rilis 3.x konkret yang diverifikasi | Native process, entrypoints, file-based routes, health, config reload sesuai kemampuan | Target wajib; artefak per-arch diuji | Kandidat setelah artefak dan lifecycle/file-watching Windows diverifikasi |

Minimal matriks versi untuk rilis: PostgreSQL mempunyai dua mayor yang diuji, sedangkan lima provider lainnya masing-masing mempunyai minimal satu versi konkret teruji dan dua instance dari versi tersebut. Kemampuan menjalankan versi lain ditandai supported hanya setelah diuji; jangan menjanjikan semua versi historis.

### 5.3 Kandidat perluasan katalog

| Kelompok | Kandidat | Batas komitmen |
| --- | --- | --- |
| Database | MariaDB, MongoDB | Ditambahkan setelah availability, packaging, lisensi, dan data lifecycle ditinjau |
| Messaging | RabbitMQ, Kafka, Redpanda | Dependency seperti Erlang/JVM dan kebutuhan OS/backend diperiksa; bukan janji native macOS |
| Cache | Redis, Memcached | Provider terpisah dari Valkey; kompatibilitas protokol tidak menjanjikan pertukaran tanpa verifikasi |
| Proxy | Caddy, Nginx, HAProxy | Routing, TLS, privilege, dan port tambahan sesuai provider |
| Object storage | MinIO atau implementasi kompatibel yang dipilih | Status distribusi dan izin redistribusi ditinjau sebelum masuk katalog |
| Search | Meilisearch, Typesense, OpenSearch | Runtime dependency dan resource minimum dinyatakan |
| Observability | OpenTelemetry Collector, Prometheus, Grafana | Endpoint dan storage tambahan diisolasi |
| Development service | Mailpit, Temporal, LocalStack | Backend native/container tidak disamakan; hanya didukung jika kontrak tersedia |
| Runtime / CLI | Bun, PHP, Node.js, Python, kubectl, SQLite tools | Tool tanpa server tidak dipaksa mempunyai instance, port, atau tombol Start |

Daftar ini adalah backlog, bukan bagian yang harus selesai agar baseline macOS diterima.

## 6. Domain produk dan definisi isolasi

| Konsep | Definisi |
| --- | --- |
| Software | Identitas tool, misalnya PostgreSQL |
| Provider | Paket metadata dan aturan pengelolaan tool, termasuk schema, capabilities, artefak, dan lifecycle |
| Provider revision | Versi immutable manifest/template yang digunakan untuk menjalankan instance |
| Runtime | Instalasi exact version untuk OS/arch/dependency tertentu; binary dapat digunakan ulang |
| Instance | Konfigurasi, data, endpoint, dan lifecycle independen yang merujuk runtime konkret |
| Environment | Definisi kebutuhan beberapa instance untuk satu proyek, termasuk dependency dan ownership |
| Artifact | Berkas distribusi dengan sumber, digest, signature/provenance bila tersedia |
| Operation | Pekerjaan asinkron yang mempunyai ID, status, progress, hasil, dan error |
| Lease | Catatan bahwa environment atau pengguna sedang memerlukan instance berjalan |
| Capability | Kemampuan provider yang benar-benar tersedia, misalnya reload atau backup |

Relasi utama:

```text
Software → Provider revisions → Runtime installations → Instances
                                                    ↖ Environment references
```

Runtime tidak mengandung data bisnis instance. Satu runtime PostgreSQL 17 yang sama dapat dipakai `pg17-dev` dan `pg17-test`; keduanya tidak memakai direktori data yang sama. PostgreSQL mendefinisikan database cluster sebagai kumpulan database dalam area data suatu instance. [S01], [S08]

### Apa yang wajib dipisahkan

| Resource | Kebijakan |
| --- | --- |
| Data/WAL/JetStream/RDB/AOF | Direktori per-instance; tidak berbagi writable data path |
| Konfigurasi | File effective config, include, dan revision per-instance |
| Log | Stream dan direktori terpisah; akses dan retention terdefinisi |
| Endpoint | Seluruh kombinasi address, port, transport dan Unix socket yang aktif dicatat |
| Proses | Process tree, start identity, runtime identity, dan instance ownership terpisah |
| Secrets | Reference per-instance; tidak reuse password secara diam-diam |
| Temp/PID/socket | Lokasi privat dan unik per-instance |
| Runtime binary | Boleh dibagi secara read-only secara operasional; tidak menjamin isolasi keamanan dari user pemilik yang sama |
| CPU/RAM/disk | Tetap resource host bersama; limit hanya ditampilkan enforced jika benar-benar dapat diterapkan |

Tidak ada klaim bahwa instance A mustahil mengakses file instance B jika binary A berbahaya dan dijalankan sebagai user yang sama. Native provider adalah kode tepercaya yang harus ditinjau; permission manifest bukan sandbox OS untuk executable tersebut.

## 7. Penggunaan multi-version dan multi-instance

### 7.1 Melalui GUI

Pengguna membuka **Catalog → PostgreSQL**, memilih versi mayor 17 dan patch terverifikasi, lalu memasang runtime. Setelah itu **Create Instance** meminta nama, endpoint, data location, auth, dan autostart. Runtime yang sama dipilih lagi untuk membuat instance kedua dengan port berbeda. Langkah yang sama untuk mayor 18 menghasilkan instance ketiga.

| Instance | Runtime | Port TCP | Path logis |
| --- | --- | --- | --- |
| `pg17-dev` | Exact runtime hasil resolusi mayor 17 | `5432` | `instances/<id-a>/data` |
| `pg17-test` | **Runtime ID identik** dengan `pg17-dev` | `5433` | `instances/<id-b>/data` |
| `pg18-dev` | Exact runtime hasil resolusi mayor 18 | `5434` | `instances/<id-c>/data` |

Ketiganya dapat berjalan bersamaan. Stop, delete, backup, config change, atau restart pada satu instance tidak boleh diterapkan ke instance lain.

Nama adalah label yang bisa diubah. Storage path menggunakan ID stabil; rename instance tidak otomatis memindahkan seluruh data.

### 7.2 Melalui CLI

Seluruh command berikut adalah **kontrak CLI yang diusulkan untuk Stackly**, bukan command produk yang sudah tersedia. `--output id` mengeluarkan satu ID runtime pada stdout; progress menuju stderr. `@17` dan `@18` adalah selector saat install, bukan runtime mutable.

```bash
# Simpan ID runtime konkret, termasuk exact version dan identitas artefaknya.
PG17_ID="$(stackly runtime install official/postgresql@17 --output id)"
PG18_ID="$(stackly runtime install official/postgresql@18 --output id)"

# Dua instance memakai runtime identik, dengan data/konfigurasi/port terpisah.
stackly instance create pg17-dev  --runtime "$PG17_ID" --port database=5432
stackly instance create pg17-test --runtime "$PG17_ID" --port database=5433

# Mayor lain berjalan berdampingan.
stackly instance create pg18-dev --runtime "$PG18_ID" --port database=5434

stackly instance start pg17-dev
stackly instance start pg17-test
stackly instance start pg18-dev

stackly instance list
stackly instance inspect pg17-dev --json
stackly instance logs pg17-dev --follow
stackly instance connection pg17-dev --format uri

stackly instance stop pg17-test
```

Output connection URI menyembunyikan password secara default. Reveal/copy secret memerlukan tindakan eksplisit. Di noninteractive mode, secret disampaikan melalui mekanisme input yang aman; bukan diwajibkan menjadi argumen CLI yang dapat terbaca dari daftar proses.

`stackly runtime remove <runtime-id>` harus ditolak jika runtime masih direferensikan instance, meskipun instance tersebut sedang stopped. `stackly instance remove <id>` default mempertahankan data; purge adalah command terpisah dengan konfirmasi.

### 7.3 Banyak port dalam satu instance

| Provider | Instance A | Instance B | Tambahan yang harus diperiksa |
| --- | --- | --- | --- |
| NATS | Client `4222`, monitoring `8222` | Client `4223`, monitoring `8223` | Cluster, leaf, gateway, WebSocket/MQTT bila diaktifkan; default fitur tersebut off |
| MySQL | TCP `3306` | TCP `3307` | Unix socket/TLS bila diaktifkan; port contoh, bukan jaminan ketersediaan |
| ClickHouse | HTTP `8123`, native TCP `9000` | HTTP `8124`, native TCP `9001` | Interserver, HTTPS/TLS, metrics atau endpoint lain bila diaktifkan |
| Traefik | HTTP `8080`, HTTPS `8443`, health `18080` | HTTP `8081`, HTTPS `8444`, health `18081` | Dashboard/API, TCP/UDP entrypoints, HTTP/3 bila diaktifkan |
| Valkey | TCP `6379` | TCP `6380` | Unix socket/TLS bila diaktifkan; cluster bukan baseline |

Angka adalah contoh konfigurasi Stackly, bukan jaminan port host sedang kosong. Monitoring NATS merupakan endpoint terpisah dari koneksi client dan harus dikonfigurasi tersendiri. [S09]

---

## 8. Kebutuhan fungsional wajib

Semua `REQ` di bagian ini berlaku untuk baseline macOS, kecuali pekerjaan Windows aktual yang secara eksplisit ditunda. Kata **wajib** adalah release requirement; bukan klaim implementasi selesai. Setiap ID mempunyai acceptance criterion utama dengan nomor yang sama di Lapisan 3.

### Platform & lifecycle

| ID | Kebutuhan | Perilaku yang wajib dipenuhi |
| --- | --- | --- |
| REQ-001 | **Core independen** | Core tidak mengimpor framework GUI. GUI dan CLI mengakses daemon yang sama; aplikasi tetap dapat digunakan headless. Penutupan window maupun Quit GUI tidak dianggap perintah stop service. |

### Catalog & runtime

| ID | Kebutuhan | Perilaku yang wajib dipenuhi |
| --- | --- | --- |
| REQ-002 | **Catalog berbasis capability** | Catalog menampilkan publisher, provider revision, kategori, OS/arch, versi tersedia, trust, status kompatibilitas, dependency, kebutuhan disk, dan capability. Unsupported tidak ditampilkan seolah install berhasil. |
| REQ-003 | **Resolusi dan pin versi** | Selector seperti @17, @18, dan @stable diselesaikan menjadi exact version dan artifact digest. Instance menyimpan runtime ID immutable; katalog yang berubah tidak mengganti runtime instance. |
| REQ-004 | **Instalasi managed yang atomik** | Unduh ke staging, validasi origin/digest dan signature sesuai kebijakan, ekstrak aman, validasi binary/dependency, lalu commit instalasi. Retry tidak membuat duplikasi atau runtime setengah terpasang. |
| REQ-005 | **OS, arsitektur, dan dependency runtime** | Runtime menyimpan target OS, arsitektur, minimum OS, build revision, dan dependency closure. Gunakan absolute executable path. Jangan bergantung pada PATH shell interaktif atau symlink versi global yang berubah. |
| REQ-006 | **Runtime external/Homebrew** | Pengguna dapat mendaftarkan instalasi existing yang diverifikasi sebagai external. Ownership dan batas reproduksibilitas terlihat. Stackly tidak menjalankan brew upgrade/cleanup, mengganti symlink, atau mengambil alih data/service existing tanpa tindakan terpisah. |
| REQ-007 | **Penghapusan dan garbage collection runtime** | Penghapusan runtime managed ditolak selama ada referensi instance, dependency, operasi, atau recovery snapshot yang masih diperlukan. GC hanya menghapus artefak cache/runtime tidak terpakai dalam scope milik manager. |

### Instance & supervision

| ID | Kebutuhan | Perilaku yang wajib dipenuhi |
| --- | --- | --- |
| REQ-008 | **Create instance terkontrol** | Create menghasilkan ID stabil, nama, provider revision, runtime ID, credential reference, config revision, endpoint, direktori, dan startup policy. Inisialisasi hanya pada target baru yang tervalidasi dan tidak menimpa data existing. |
| REQ-009 | **Multi-version dan multi-instance terisolasi** | Versi berbeda serta versi identik dapat berjalan bersama dengan writable data, config, log, temp, socket, identity, dan lifecycle terpisah. Runtime yang sama boleh digunakan ulang; data directory tidak boleh dibagi. |
| REQ-010 | **Seluruh endpoint dan konflik resource** | Validasi address, transport, port, wildcard IPv4/IPv6, Unix socket dan endpoint tambahan provider. Fixed port tidak diganti diam-diam. Auto-port harus dipersistenkan dan ditampilkan setelah bind/readiness berhasil. |
| REQ-011 | **Lifecycle konsisten dan aman** | Start, stop, restart, reload, dan status mengikuti capability. Stop menggunakan graceful provider-specific strategy dengan timeout; forced kill bukan default dan tidak boleh menyasar proses tanpa ownership yang terverifikasi. |
| REQ-012 | **Readiness berbeda dari process state** | Pisahkan process running, readiness, health, dan desired state. Ready memerlukan probe provider yang sesuai; TCP terbuka saja tidak cukup membuktikan database dan autentikasi siap. |
| REQ-013 | **Recovery dan reconciliation** | Daemon merekonsiliasi proses, runtime, endpoint, storage, operasi, dan desired state setelah restart. Crash-loop dibatasi backoff/retry budget. Proses surviving hanya diadopsi setelah identitas diverifikasi. |
| REQ-014 | **Autostart per-user** | Autostart daemon dan autostart instance adalah setting berbeda. Baseline menjalankan background agent dalam sesi user. Izin background yang dicabut terlihat di UI; login bukan boot sebelum user login. |

### Configuration & diagnostics

| ID | Kebutuhan | Perilaku yang wajib dipenuhi |
| --- | --- | --- |
| REQ-015 | **Config revision dan drift** | Form typed dan advanced config memakai schema provider. Tampilkan diff, validasi, revision, restart/reload impact, serta drift akibat edit luar. Managed field penting tidak boleh ditimpa oleh override tanpa validasi. |
| REQ-016 | **Secrets dan akses lokal** | Simpan secret di platform secret store dan hanya reference di SQLite/manifest. Masking berlaku untuk GUI, CLI, logs, diagnostics, dan environment export. Kebutuhan file config plaintext oleh tool dinyatakan sebagai residual risk dengan permission ketat. |
| REQ-017 | **Log streaming dan retention** | Log per-instance mendukung follow, pagination/cursor, filter dasar, timestamps, pemisahan stdout/stderr bila tersedia, batas ukuran, dan rotasi. Subscriber lambat tidak boleh menahan pipe proses atau membuat memori tidak terbatas. |
| REQ-018 | **Monitoring dasar** | Tampilkan status, readiness, PID tree, uptime, restart count, last failure, CPU/memory dan disk usage terjadwal. Bedakan metrik proses utama dan agregat child. Unsupported metric ditandai unavailable. |
| REQ-019 | **Doctor dan paket diagnosis** | Doctor memeriksa izin, runtime/dependency missing, endpoint konflik, data lock, space, registry, background agent, protocol mismatch, dan config drift. Diagnosis bukan izin otomatis menghapus lock atau mengubah data. |

### Environment

| ID | Kebutuhan | Perilaku yang wajib dipenuhi |
| --- | --- | --- |
| REQ-020 | **Environment plan dan validasi** | File deklaratif mempunyai schema version, namespace, runtime selector, endpoint, settings, secret references, dependencies, dan ownership. Plan menampilkan diff serta dampak tanpa download, spawn, trust grant, atau file mutation. |
| REQ-021 | **Environment up dan partial failure** | Up menginstal yang diperlukan, membuat dedicated instance, memperoleh lease, lalu menjalankan dependency graph berdasarkan readiness. Siklus ditolak. Partial failure menghasilkan state yang jelas dan kompensasi hanya pada resource milik operasi tersebut. |
| REQ-022 | **Environment down dan shared instance** | Down melepas lease environment dan menghentikan hanya instance yang tidak lagi dibutuhkan. Shared reference, manual start, dan ownership diperhitungkan. Down tidak menghapus data atau runtime. |
| REQ-023 | **Portabilitas dan lockfile** | Pisahkan manifest yang dibagikan, resolved lockfile, dan override lokal. Lockfile menyimpan exact version/provider/artifact per platform tanpa path pribadi dan secrets. Mesin tanpa artefak kompatibel gagal jelas, bukan memakai versi lain diam-diam. |

### Native GUI

| ID | Kebutuhan | Perilaku yang wajib dipenuhi |
| --- | --- | --- |
| REQ-024 | **Shell SwiftUI dan state global** | GUI menyediakan navigasi, daemon connection status, operation center, loading/empty/error/stale/unsupported states, serta reconnect. Seluruh business mutation melalui API daemon. |
| REQ-025 | **Runtime, instance, dan catalog UI** | Sediakan install wizard, create/duplicate-config instance, runtime list, instance list/detail, version/endpoint/auth summary, lifecycle actions, dan dependency warning. Duplicate-config membuat storage baru dan bukan clone data. |
| REQ-026 | **Config, logs, environment dan recovery UI** | GUI menyediakan config diff/validation, logs, monitoring, environment preview/up/down, backup/restore progress, dan diagnostics. Tombol tidak boleh tersedia seolah capability unsupported sudah bekerja. |
Notifikasi opsional dan permission-aware. |

### CLI & provider

| ID | Kebutuhan | Perilaku yang wajib dipenuhi |
| --- | --- | --- |
| REQ-028 | **CLI parity dan automation** | Seluruh operasi administrasi baseline tersedia di CLI dengan help, JSON output, exit code stabil, noninteractive mode, progress ke stderr, serta secret input aman. CLI menggunakan daemon dan tidak menjadi supervisor kedua. |
| REQ-029 | **Manifest provider deklaratif** | Manifest mendeskripsikan identity, versions/artifacts, platforms, settings schema, capabilities, resources, template, startup/shutdown/readiness dan dependencies. Tool baru dengan primitive existing tidak perlu patch core/GUI. |
| REQ-030 | **Versioned Provider SDK** | Schema dan SDK mempunyai version compatibility, deprecation policy, validator, template scaffold, test harness, serta schema migration policy. Unknown field keamanan dan capability yang tidak dapat dipenuhi ditolak. |
| REQ-031 | **Eksekusi provider dibatasi** | Gunakan primitive host yang diizinkan, executable terverifikasi, argv array, cwd/env eksplisit, dan batas file scope. Tidak ada shell interpolation atau arbitrary plugin script default. Complex hook/WASM merupakan perluasan terpisah. |
| REQ-032 | **Trust dan supply chain** | Official, reviewed-community, dan local/unverified adalah provenance labels, bukan bukti sandbox. Approval terikat publisher/revision/capability/digest; perubahan material meminta review. Revocation memblokir instalasi baru tanpa menghapus data yang telah ada. |
| REQ-033 | **Sumber catalog dan provider lokal** | Baseline mendukung bundled/official catalog serta local directory/package yang diimpor eksplisit. Private registry dan public discovery dapat ditambahkan melalui kontrak source. Browsing catalog tidak menjalankan kode provider. |
| REQ-034 | **Kontribusi komunitas sejak baseline** | Sediakan CONTRIBUTING, provider template, schema docs, compatibility fixture, security policy, issue template, dan provider test command. Submission dapat melalui PR; public registry hosting bukan prasyarat baseline. |

### Provider awal

| ID | Kebutuhan | Perilaku yang wajib dipenuhi |
| --- | --- | --- |
| REQ-035 | **PostgreSQL 17 dan 18** | Provider menginisialisasi cluster dengan tool versi cocok, auth lokal aman, seluruh endpoint terpisah, query readiness, shutdown terkontrol, log, persistence, logical backup/restore. Upgrade mayor in-place otomatis dilarang. [S01], [S04], [S10] |
| REQ-036 | **NATS dan JetStream** | Provider mendukung client endpoint, monitoring loopback, credential configuration, JetStream enable/disable, unique server identity, store_dir dan resource limits. Monitoring tidak boleh terbuka ke LAN secara default. [S09], [S11] |
| REQ-037 | **ClickHouse single-node** | Provider mengelola native binary terpilih, path storage/temp/log/user-files/config, HTTP/native ports, auth, query readiness dan persistence. Endpoint lain dimatikan atau didaftarkan. Windows WSL tidak diberi label native. [S06] |
| REQ-038 | **Valkey standalone** | Provider mengelola config, bind/auth, PING, persistent directory, RDB/AOF policy, log dan stop. UI menjelaskan mode volatile versus durable dan batas durability yang dipilih; tidak mengasumsikan semua key selalu persisten. [S21] |
| REQ-039 | **Traefik dan routing lokal** | Provider mendukung entrypoint loopback, file-based routing config, health endpoint, per-instance route namespace, validation dan safe apply. HTTPS membutuhkan certificate material yang disediakan; root CA/DNS sistem tidak dipasang otomatis. [S12], [S13] |
| REQ-055 | **MySQL standalone** | Provider mengelola data/config/temp/log path terisolasi, bind/auth, readiness query, shutdown terkontrol, persistence dan backup/restore. Runtime tidak boleh dibind ke data yang tidak kompatibel; adopsi data existing memerlukan preflight dan persetujuan eksplisit. [S22], [S23] |

### Data safety & release

| ID | Kebutuhan | Perilaku yang wajib dipenuhi |
| --- | --- | --- |
| REQ-040 | **Backup metadata manager** | Sediakan backup konsisten registry/config metadata dengan versi schema, checksum, dan secret references. Backup metadata tidak diklaim mencakup seluruh database provider atau isi Keychain. Restore metadata wajib reconcile dengan file/proses nyata. |
| REQ-041 | **Backup data per-provider** | Sediakan native backup bila capability tersedia dan fallback cold backup hanya setelah stop bersih untuk layout lokal yang telah divalidasi. Backup mempunyai consistency mode, scope, runtime identity, file digests, size dan status verifikasi; bukan zip live data generik. |
| REQ-042 | **Restore aman dan pengujian recovery** | Restore default membuat instance baru dengan endpoint baru. Overwrite existing membutuhkan konfirmasi, backup/preflight, compatible runtime dan disk cukup. Backup belum dianggap recoverable sampai restore verification berhasil. |
| REQ-043 | **Update runtime berbeda dari migrasi data** | Install new runtime tidak mengubah instance. Rebind hanya diizinkan untuk pasangan kompatibel yang telah diverifikasi. Major migration memakai alur eksplisit/manual yang tervalidasi; tidak downgrade data dengan sekadar mengganti binary. [S10] |
| REQ-044 | **Update aplikasi dan kompatibilitas** | Update GUI/CLI/daemon memeriksa protocol dan registry schema compatibility, menghormati active operation serta restart window. Tidak auto-upgrade tool database. Rollback aplikasi dipisahkan dari rollback data provider. |
| REQ-045 | **Audit lokal** | Catat actor lokal/client, operation ID, instance, timestamp, action, config revision dan hasil tanpa secret. Audit lokal milik user bukan catatan tamper-proof atau bukti forensic yang tak dapat diubah. |
| REQ-046 | **Offline dan privasi** | Start/stop/status/config/log/runtime terinstal bekerja tanpa internet. Install baru memerlukan cached verified artifact atau koneksi. Telemetry dan upload diagnostics tidak aktif tanpa consent; tidak wajib cloud account. |

### IPC & integrity

| ID | Kebutuhan | Perilaku yang wajib dipenuhi |
| --- | --- | --- |
| REQ-047 | **IPC lokal berversi dan terotorisasi** | Sediakan handshake/version/capability, request ID, structured errors, operation polling/events dan local-user authorization. Tidak membuka TCP admin listener pada baseline. Transport macOS dan Windows boleh berbeda; payload contract sama. |
| REQ-048 | **Concurrency dan idempotency** | Daemon tunggal mengoordinasikan mutasi dengan lock per-resource, state revision, idempotency key dan operation journal. SQLite tidak dipakai sebagai mutex yang ditahan sepanjang download atau process startup. |

### Data safety & release — packaging

| ID | Kebutuhan | Perilaku yang wajib dipenuhi |
| --- | --- | --- |
| REQ-049 | **Packaging dan distribusi macOS** | Distribusi .app dengan helper/CLI, signing/notarization sesuai kebijakan, installer/uninstaller, upgrade path dan troubleshooting. Pengguna final tidak perlu Rust/Xcode untuk core manager; dependency provider harus terlihat dan terpasang melalui sumber yang disetujui. [S14] |

### Platform & lifecycle — Windows readiness

| ID | Kebutuhan | Perilaku yang wajib dipenuhi |
| --- | --- | --- |
| REQ-050 | **Windows-ready, bukan Windows-done** | Core, DTO, provider schema dan storage logical model bebas asumsi macOS. Platform-specific implementation dipisah. Windows GUI/adapter/provider readiness merupakan milestone tersendiri, bukan syarat fitur WinUI selesai pada rilis macOS. |

### Data safety & release — quality and recovery

| ID | Kebutuhan | Perilaku yang wajib dipenuhi |
| --- | --- | --- |
| REQ-051 | **Quality gates dan bukti pengujian** | Semua requirement mempunyai AC, test/evidence dan status. Provider supported wajib lulus integration matrix nyata. Release ditahan bila mandatory security, data safety, clean-install atau recovery test gagal/blocked. |
| REQ-052 | **Uninstall, remove dan purge** | Remove instance mempertahankan data sebagai retained record; purge adalah aksi berbeda dengan path/size/impact preview dan explicit confirmation. Uninstall default mempertahankan data serta menjelaskan cara recovery; cache/runtime tidak ikut dihapus tanpa scope jelas. |
| REQ-053 | **Adoption dan orphan data** | Directory existing/orphan dapat dideteksi dan diinventarisasi read-only. Adoption melalui preflight version/config/process ownership dan tindakan eksplisit; tidak menjalankan initdb, reset, upgrade atau take-over otomatis. |

### IPC & integrity — operation progress

| ID | Kebutuhan | Perilaku yang wajib dipenuhi |
| --- | --- | --- |
| REQ-054 | **Progress, cancellation dan recovery operation** | Install/create/backup/restore/update mempunyai stage, progress bila terukur, cancellability dan recovery action. Cancel pada langkah irreversible tidak berarti rollback berhasil; tampilkan continuing-safe-shutdown atau needs-recovery. |

## 9. Aturan bisnis dan invariants

| ID | Aturan |
| --- | --- |
| BR-001 | Instance selalu menunjuk exact runtime ID dan provider revision; selector tidak dievaluasi ulang diam-diam saat start. |
| BR-002 | Dua instance tidak boleh mempunyai writable data root yang sama, bertumpuk parent/child, atau resolve ke target yang sama melalui symlink. |
| BR-003 | Mengganti nama instance tidak mengganti ID, runtime, credential reference, atau storage path. |
| BR-004 | Port dicatat sebagai address + transport + number; TCP dan UDP dibedakan. Listener wildcard dan overlap IPv4/IPv6 harus diperiksa. |
| BR-005 | Port fixed yang konflik menghasilkan error. Hanya permintaan auto-port yang boleh menghasilkan port pilihan manager. |
| BR-006 | Delete/remove tidak identik dengan purge. Down/stop tidak pernah berarti hapus data. |
| BR-007 | Runtime yang masih diperlukan tidak boleh dihapus, termasuk yang dipakai instance stopped atau dependency runtime lain. |
| BR-008 | Create hanya menginisialisasi direktori baru yang dimiliki operasi; existing data menggunakan alur adoption/restore terpisah. |
| BR-009 | Tidak boleh memulai binary mayor PostgreSQL berbeda pada cluster existing tanpa migrasi yang sah. [S10] |
| BR-010 | Status ready memerlukan probe yang relevan dan fresh; PID saja bukan bukti readiness. |
| BR-011 | Daemon menjadi satu-satunya penulis registry dan pengendali lifecycle. GUI/CLI tidak melakukan fallback spawn langsung. |
| BR-012 | Akun user OS adalah scope baseline. Produk tidak mengklaim mencegah malware lain dengan hak user yang sama. |
| BR-013 | Update manager, update provider, install runtime baru, dan migrasi data merupakan empat operasi terpisah. |
| BR-014 | Shared instance tidak dihentikan oleh satu environment jika masih ada demand/lease lain. |
| BR-015 | Stop individual yang mengganggu environment lain harus ditolak atau meminta override dengan dampak yang jelas; tidak diam-diam melanggar lease. |
| BR-016 | Tidak semua provider mendukung reload, backup online, TLS, metrics atau multi-version historis; unsupported harus konsisten pada GUI, CLI, dan daemon. |
| BR-017 | Provider revision baru tidak boleh mengubah konfigurasi instance lama sampai pengguna memilih apply/update dengan diff. |
| BR-018 | Secret tidak masuk manifest yang dibagikan, lockfile, audit default, atau runtime command arguments bila ada mekanisme input yang lebih aman. |
| BR-019 | Runtime external tidak dianggap milik Stackly. Registrasi external binary tidak mengambil alih service/data yang sudah ada. |
| BR-020 | Native binary terverifikasi tetap merupakan kode yang berjalan dengan hak user; checksum bukan bukti binary bebas malware. |
| BR-021 | Restore default menggunakan instance baru; cross-OS/cross-arch restore tidak diasumsikan compatible hanya karena nama provider sama. |
| BR-022 | Fitur backup ditandai recoverable hanya setelah restore drill; backup registry bukan backup database provider. |
| BR-023 | Kegagalan/pembatalan operation tidak boleh menyebabkan penghapusan resource yang dibuat oleh operasi lain atau user. |
| BR-024 | Autostart adalah kebijakan saat login/startup agent, bukan alasan membatalkan stop eksplisit pengguna selama sesi yang sama. |
| BR-025 | `SUPPORTED` terikat matrix yang diuji. Registry yang lebih baru tidak menjamin semua versinya supported pada semua mesin. |

## 10. Validasi input dan resource

| ID | Validasi minimum |
| --- | --- |
| VAL-001 | Nama instance: usulan 1–63 karakter lowercase kebab-case, dimulai huruf; display name opsional dapat Unicode. ID tetap terpisah. |
| VAL-002 | Nama provider namespaced, misalnya `official/postgresql`; revision/version tidak kosong dan tidak mengandung path traversal. |
| VAL-003 | Runtime selector di-resolve menurut scheme provider. Jangan memaksakan SemVer pada semua software yang memakai format versi berbeda. |
| VAL-004 | Port 1–65535; baseline default memakai port nonprivileged. Port otomatis merupakan mode eksplisit, bukan angka 0 yang bocor sebagai connection endpoint. |
| VAL-005 | Bind loopback secara default. Non-loopback membutuhkan advanced opt-in, dampak keamanan, serta auth yang sesuai; tidak mengubah firewall otomatis. |
| VAL-006 | Semua listener tambahan, Unix socket, named resource dan file PID diperiksa sebelum start dan setelah startup. |
| VAL-007 | Canonical path tidak boleh keluar dari owned root, overlap data root lain, atau masuk lokasi system; symlink/alias diperiksa pada operasi destruktif. |
| VAL-008 | Data directory harus sesuai owner/permission, filesystem supported, available space, runtime version, dan status lock. |
| VAL-009 | External path hanya diadopsi eksplisit; path network/shared-sync untuk live database tidak dianggap supported tanpa uji khusus. |
| VAL-010 | Download memiliki size cap, timeout, retry budget, HTTPS origin policy dan digest; signature diverifikasi bila diwajibkan metadata. |
| VAL-011 | Archive extraction menolak traversal, absolute path, symlink escape, special device file, dan decompression bomb; tidak mempertahankan setuid. |
| VAL-012 | Schema config mengklasifikasi field managed, advanced, secret, immutable-after-init, reloadable dan restart-required. |
| VAL-013 | Environment menolak dependency cycle, unresolved reference, secret literal yang dilarang, dan fixed endpoint konflik sebelum mutation. |
| VAL-014 | `instanceRef` harus ada, sesuai provider/capability dan memiliki access scope user yang sama; tidak otomatis direkonfigurasi oleh environment peminjam. |
| VAL-015 | Operasi destruktif memeriksa expected revision, active operation, environment references, ownership dan path lagi saat commit. |
| VAL-016 | Backup/restore memvalidasi manifest, checksum, compatibility, free space, secret handling dan scope external path sebelum mengganti data. |
| VAL-017 | IPC membatasi message size, nesting, unknown method, concurrent operation dan subscription; client disconnect bukan implicit cancel. |
| VAL-018 | Trust metadata dan provider permission tidak boleh berubah melalui field arbitrary config atau environment variable tak dikenal. |
| VAL-019 | Unknown settings yang berisiko mengubah path/port/proses ditolak, bukan diteruskan tanpa pemeriksaan. |
| VAL-020 | Setiap error mempunyai code stabil, safe message, field/resource context, retryability dan recovery hint bila tersedia. |

## 11. Halaman GUI dan alur pengguna

### 11.1 Peta navigasi

```text
Stackly
├── Overview
├── Instances
│   └── Instance Detail
│       ├── Summary & Connection
│       ├── Configuration
│       ├── Logs
│       ├── Metrics & Health
│       ├── Backup & Restore
│       └── Activity
├── Catalog
│   └── Provider Detail
├── Runtimes
├── Environments
│   └── Environment Detail & Plan
├── Operations
├── Diagnostics
└── Settings
```

### 11.2 Spesifikasi per halaman

| ID | Halaman | Informasi dan aksi wajib |
| --- | --- | --- |
| PAGE-01 | Onboarding | Status daemon/background permission, lokasi storage, model isolasi, catalog trust, pilihan CLI install, dan tanpa kewajiban akun cloud |
| PAGE-02 | Overview | Ringkasan ready/stopped/failed, active operations, capacity warning, quick navigation; tidak hanya menghitung PID |
| PAGE-03 | Instances | Search/filter/sort; nama, provider, exact version, endpoints, lifecycle, readiness, ownership, environment reference |
| PAGE-04 | Create Instance | Pilih runtime, nama, endpoints, auth, persistence, settings, storage, autostart; preflight dan error per-field |
| PAGE-05 | Instance Summary | ID, version/digest, data size/path, endpoints, connection string masked, start/stop/restart, dependency impact |
| PAGE-06 | Configuration | Typed form, advanced view, diff, validation, history minimum last-known-good, drift warning, apply/reload/restart |
| PAGE-07 | Logs & Metrics | Cursor-based follow, source/filter, pause/resume, truncation banner, CPU/RAM/process-tree scope, health timestamp |
| PAGE-08 | Provider Catalog | Search category, trust/compatibility labels, supported version matrix, source/license metadata, install/import provider |
| PAGE-09 | Runtimes | Installed versions, source managed/external, dependency and instance references, verified status, install/remove/doctor |
| PAGE-10 | Environments | Import/edit/validate/plan/up/down, dependency graph sebagai daftar/tree, lease information, partially-ready diagnosis |
| PAGE-11 | Backup & Restore | Backup scope/mode, size/checksum, progress, consistency, verified restore date; restore-as-new dan explicit overwrite |
| PAGE-12 | Operations & Activity | Stage/progress/cancelability/error, retry/resume bila aman, audit detail tanpa secret |
| PAGE-13 | Diagnostics | Preflight/doctor, missing binaries, permission/port/config issues, preview sanitized diagnostic bundle |
| PAGE-14 | Settings | Background startup, autostart defaults, catalog sources/trust, data/cache/log paths, retention, update channel, privacy |

Semua halaman data mendukung state loading, empty, error, stale/disconnected, unsupported dan partial. Errors per-instance tidak membuat seluruh aplikasi blank. Tombol tetap divalidasi daemon meskipun UI sudah menonaktifkannya.

Menu bar status dasar menampilkan ringkasan service dan aksi membuka aplikasi. **Quit App** menutup GUI saja. **Stop Services** merupakan aksi berbeda dengan impact preview. Shortcut serta istilah mengikuti idiom macOS dan tidak harus ditiru secara identik pada WinUI.

### 11.3 User journeys wajib

**JOURNEY-01 — Instalasi pertama:** signed app → onboarding → background permission → catalog → runtime install → create instance → readiness → copy connection.

**JOURNEY-02 — Versi sama, port berbeda:** pilih runtime existing → create second instance → auto/fixed endpoint validation → data baru → start keduanya → verifikasi data berbeda.

**JOURNEY-03 — Mayor berbeda:** install runtime baru → create fresh instance → buka endpoint baru → bandingkan versi. Tidak pernah mengubah data directory instance lama sebagai bagian install.

**JOURNEY-04 — Environment ERP:** import manifest → review provider trust → plan → resolve exact lock → up → progress/readiness → down tanpa menghapus data.

**JOURNEY-05 — Kegagalan runtime:** start error → safe message → doctor/log → perbaikan config/dependency → retry → operation record; tidak otomatis reset data.

**JOURNEY-06 — Recovery:** backup terverifikasi → restore-as-new → secret configuration → readiness/data checks → pengguna memilih connection baru; instance sumber tetap ada.

**JOURNEY-07 — Menambah provider:** scaffold → manifest/template → local validation → explicit trust → contract/integration tests → PR untuk review. Provider sederhana tidak membutuhkan Rust atau Swift code baru.

## 12. CLI yang direncanakan

| Kelompok | Command baseline |
| --- | --- |
| Daemon | `stackly daemon status`, `stackly daemon install`, `stackly daemon start`, `stackly daemon stop`, `stackly daemon restart` |
| Catalog/provider | `stackly provider list`, `stackly provider inspect`, `stackly provider import`, `stackly provider validate`, `stackly provider test` |
| Runtime | `stackly runtime list`, `stackly runtime install`, `stackly runtime register`, `stackly runtime inspect`, `stackly runtime remove` |
| Instance | `stackly instance create`, `list`, `inspect`, `start`, `stop`, `restart`, `reload`, `connection`, `remove`, `purge` |
| Config/log | `stackly instance config get`, `config validate`, `config apply`, `logs`, `metrics` |
| Environment | `stackly env validate`, `plan`, `lock`, `up`, `down`, `status`, `export` |
| Recovery | `stackly backup create`, `list`, `verify`, `stackly restore`, `stackly instance adopt` |
| Operasional | `stackly operation list`, `inspect`, `cancel`, `stackly doctor`, `stackly diagnostics export` |

Nama command merupakan rancangan kontrak; perubahan sebelum stabil harus memperbarui schema, docs, test, dan contoh sekaligus. Setelah rilis stabil, perubahan breaking memerlukan major protocol/CLI policy.

Usulan exit code: `0` sukses; `2` invalid input; `3` resource conflict; `4` unauthorized/trust denied; `5` incompatible/unsupported; `6` daemon unavailable; `7` operation failed; `8` timeout; `9` cancelled. Detail error machine-readable tetap menjadi sumber utama. Command async mengembalikan operation ID; `--wait` menunggu hasil final. `--json` tidak mengandung ANSI escape atau progress bercampur.

## 13. Project environment deklaratif

### 13.1 Contoh `stackly.yaml`

Format berikut adalah **schema produk yang diusulkan**, bukan format tool pihak ketiga. Namespace `stackly/v1alpha1` adalah identitas schema internal yang diusulkan; keputusan namespace final tetap dicatat di OQ-001 dan bukan klaim kepemilikan domain internet.

```yaml
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: erp-development

services:
  postgres:
    provider: official/postgresql
    version: "18"
    ownership: dedicated
    endpoints:
      database:
        host: "127.0.0.1"
        port: 5432
        transport: tcp
    secretRefs:
      adminPassword: "local:erp/postgres/admin"
    settings:
      initialDatabase: erp
    storage:
      mode: managed
      persistent: true

  mysql:
    provider: official/mysql
    version: stable
    ownership: dedicated
    endpoints:
      database:
        host: "127.0.0.1"
        port: 3306
        transport: tcp
    secretRefs:
      rootPassword: "local:erp/mysql/root"
    settings:
      initialDatabase: erp
    storage:
      mode: managed
      persistent: true

  nats:
    provider: official/nats
    version: "2"
    ownership: dedicated
    endpoints:
      client: {host: "127.0.0.1", port: 4222, transport: tcp}
      monitoring: {host: "127.0.0.1", port: 8222, transport: tcp}
    secretRefs:
      clientToken: "local:erp/nats/token"
    settings:
      jetstream: true
    storage:
      mode: managed
      persistent: true

  clickhouse:
    provider: official/clickhouse
    version: stable
    ownership: dedicated
    endpoints:
      http: {host: "127.0.0.1", port: 8123, transport: tcp}
      native: {host: "127.0.0.1", port: 9000, transport: tcp}
    secretRefs:
      adminPassword: "local:erp/clickhouse/admin"
    storage:
      mode: managed
      persistent: true

  valkey:
    provider: official/valkey
    version: stable
    ownership: dedicated
    endpoints:
      client: {host: "127.0.0.1", port: 6379, transport: tcp}
    secretRefs:
      defaultPassword: "local:erp/valkey/password"
    settings:
      persistence: aof
    storage:
      mode: managed
      persistent: true

  traefik:
    provider: official/traefik
    version: "3"
    ownership: dedicated
    endpoints:
      http: {host: "127.0.0.1", port: 8080, transport: tcp}
      health: {host: "127.0.0.1", port: 18080, transport: tcp}
    settings:
      routes:
        - name: erp-api
          rule: "PathPrefix(`/api`)"
          upstream: "http://127.0.0.1:3000"
```

Keenam service tidak memiliki dependency wajib satu sama lain pada contoh tersebut; jangan menambahkan dependency palsu sekadar membuat grafik. Backend ERP pada port `3000` adalah aplikasi pengguna yang dijalankan terpisah, bukan runtime aplikasi yang otomatis dibuat oleh contoh ini. HTTPS baru diaktifkan setelah certificate/key diberikan melalui alur yang didukung.

```bash
stackly env validate --file stackly.yaml
stackly env plan --file stackly.yaml
# Lock memilih versi konkret dari catalog yang dipercaya; secrets tetap reference.
stackly env lock --file stackly.yaml
stackly env up --file stackly.yaml --locked --wait
stackly env status --file stackly.yaml
stackly env down --file stackly.yaml
```

`env lock` dapat memerlukan metadata online, tetapi tidak menjalankan provider. `env up --locked` wajib menolak resolusi ulang diam-diam. Secret reference harus diisi secara lokal sebelum startup; jangan mengganti missing secret dengan password kosong.

### 13.2 Shared instance dan leases

Environment dapat menggunakan `instanceRef` alih-alih membuat dedicated instance. Alur shared reference hanya meminta instance yang ada; tidak menulis ulang runtime, config, auth, port atau autostart instance tersebut.

Lease disimpan secara persisten berdasarkan identitas environment, bukan koneksi CLI sementara. Client yang ditutup tidak melepaskan lease. `env down` melepaskan lease environment terkait. Start manual dapat membuat manual demand; stop manual memerlukan evaluasi terhadap seluruh demand yang masih aktif.

Autostart menimbulkan startup intent saat login sesuai policy. Stop eksplisit tidak segera dibatalkan oleh reconciler; status suppression untuk sesi terkait dicatat. Pada login berikutnya, startup policy dapat berlaku kembali dan perilaku tersebut harus dijelaskan di Settings.

Dependency graph mendukung kondisi `ready` dan timeout, bukan sekadar urutan spawn. Unit/contract test menggunakan provider fixture untuk menguji graph, cycle, failure, dan dependency recovery tanpa mengarang dependency teknis antar-database.

### 13.3 Lockfile dan override lokal

```text
project/
├── stackly.yaml                  # Dibagikan ke tim
├── stackly.lock                  # Versi provider/runtime dan digest yang terpilih
└── .stackly.local.yaml            # Override lokal; masuk .gitignore
```

Lockfile tidak memuat password, home directory, atau PID. Artefak dicatat per platform; jika Windows belum memiliki artefak yang compatible, itu merupakan blocker yang terlihat. Override port dan storage bersifat lokal dan tidak diam-diam mengubah manifest milik tim. Port aktual auto-allocated harus dapat diinspeksi melalui GUI/CLI serta dihasilkan ke env output tanpa mengekspor secret secara default.

## 14. Kebutuhan non-fungsional

Angka berikut adalah **target engineering awal**, bukan hasil benchmark. Validasi dengan hardware baseline yang dicatat sebelum beta; perubahan target memerlukan ADR dan persetujuan product owner.

| ID | Area | Target dan cara mengukur |
| --- | --- | --- |
| NFR-001 | Respons API lokal | p95 operasi metadata read ≤250 ms untuk 100 instance terdaftar, pada mesin baseline, di luar network/disk-heavy operation |
| NFR-002 | Respons GUI | Snapshot awal setelah koneksi daemon ≤2 detik untuk 100 instance; operasi berat tidak memblokir UI thread |
| NFR-003 | Overhead manager | Usulan daemon idle ≤150 MiB RSS dan rata-rata ≤1% satu logical CPU pada 10 instance idle; tidak menghitung memori binary provider |
| NFR-004 | Resource limit | Semua task queue/subscription/log buffer dibatasi; batas efektif dan metrik dropped/truncated terlihat |
| NFR-005 | Konsistensi | Operasi concurrency/idempotency tidak menghasilkan runtime/instance ganda atau active config setengah jadi |
| NFR-006 | Startup/readiness | Acknowledgement operasi lokal ≤500 ms; deadline readiness provider configurable, default usulan 60 detik tanpa janji semua server selesai pada durasi itu |
| NFR-007 | Reconciliation | Inventory proses dan state tersedia ≤10 detik setelah daemon ready untuk 10 instance uji; provider recovery dapat lebih lama dan tetap visible |
| NFR-008 | Data safety | Tidak ada known data-loss defect, overwrite tak berizin, atau cleanup keluar scope; durable fixture diuji dengan pengaturan upstream yang dicatat |
| NFR-009 | Security | Tidak ada unresolved critical/high finding pada distribusi, IPC, provider execution atau destructive operations sebelum stable |
| NFR-010 | Portabilitas | Domain/protocol tidak mengimpor framework GUI atau API OS; target check dan fake-platform tests berjalan di CI |
| NFR-011 | Observability | Semua operation/error memiliki correlation ID; metrics/log dipisah dari registry growth yang tak terkendali |
| NFR-012 | Aksesibilitas | Critical journeys lulus interaksi desktop review; kontras/status/error tidak mengandalkan warna saja |
| NFR-013 | Test quality | Target line coverage ≥80% pada core kritis sebagai sinyal, bukan pengganti AC/fault injection/recovery tests |
| NFR-014 | Offline | Local lifecycle bekerja tanpa akun/cloud; catalog stale ditampilkan dengan waktu pembaruan terakhir |
| NFR-015 | Distribusi | Clean-install, reinstall, upgrade, uninstall-retain-data dan approval-denied diuji pada setiap OS/arch yang diiklankan |
| NFR-016 | Privasi | Tidak mengirim telemetry, daftar proyek, path atau log secara default; diagnostics upload hanya setelah preview/consent |

Usulan baseline pengujian performa: Mac Apple Silicon, RAM 16 GB, storage APFS lokal, OS dalam matrix supported, maksimal 10 instance idle/light-load untuk overhead manager. Hasil provider throughput bukan KPI Stackly dan tidak boleh diklaim meningkat hanya karena core menggunakan Rust.

---

<a id="layer-2"></a>

# LAPISAN 2 — PROJECT INTEGRATION / PROJECT FIT

## 15. Arsitektur sistem

```text
                   macOS                               Windows (fase berikutnya)
             SwiftUI native UI                         WinUI 3 / C# / XAML
                    │                                           │
             Swift IPC client                           C# IPC client
                    │                                           │
                    ├──────── Rust CLI / local client ───────────┤
                    │        Versioned protocol contract         │
                    ▼                                           ▼
             Unix domain socket                            Named pipe
                    │                                           │
                    └────────────── Rust Daemon ─────────────────┘
                                         │
                                  Application services
                                         │
                         Domain: runtime / instance / environment
                                         │
             ┌───────────────────────────┼──────────────────────────┐
             │                           │                          │
       Provider engine           Operations & supervisor     Storage / security
             │                           │                          │
       Provider packages           Platform adapters          SQLite / secrets
             │                           │
             └────────────── Native executable processes ──────────┘
```

**Core** adalah library business/domain logic. **Daemon** adalah executable yang menjalankan application services. **Supervisor** adalah komponen pengelola proses di dalam atau di bawah daemon, bukan nama lain untuk GUI. **Platform adapter** mengisolasi OS-specific behavior. Provider tidak mempunyai hak untuk mengubah registry atau menjalankan proses di luar jalur host yang disetujui.

### 15.1 Batas modul

| Komponen | Tanggung jawab | Tidak boleh |
| --- | --- | --- |
| Domain/core | Invariants, state transition, policies, model dan interfaces | Menjalankan `launchctl`, mengakses UI, atau membaca raw manifest langsung untuk side effects |
| Application services | Use case, transaction boundary, operation orchestration | Mengasumsikan GUI selalu online |
| Runtime manager | Resolve, fetch, verify, stage, install, pin, refs, remove | Menentukan data directory database lewat default global |
| Instance manager | Create, configure, ownership, endpoint plan, retention | Menginisialisasi data existing tanpa alur khusus |
| Supervisor | Process ownership, graceful lifecycle, reconciliation, restart policy | Global kill by process name atau PID-only identity |
| Provider engine | Interpret schema dan safe primitives; produce validated execution plan | Unrestricted shell/plugin hook atau mengambil alih lifecycle daemon |
| Environment manager | Graph, leases, dedicated/shared, plan/up/down | Mematikan shared instance tanpa mengecek demand |
| Storage | Registry, migrations, operation journal, audit | Menjadi data store PostgreSQL/NATS/ClickHouse milik pengguna |
| Platform adapter | Process facilities, filesystem paths, user identity, secrets, IPC, startup | Membocorkan OS detail ke public product schema |
| GUI/CLI | Presentasi, input, permission prompts, API client | Menulis SQLite atau menjalankan binary provider langsung |

### 15.2 Dependency direction

Domain tidak bergantung pada platform atau provider konkret. Application services menggunakan trait/interfaces. Adapter mengimplementasikan interface domain. Daemon merangkai dependency secara eksplisit; tidak memerlukan framework dependency injection berat. Semua mutasi penting harus dapat diuji memakai fake platform serta fixture provider.

Tidak diperlukan MySQL, NATS, ClickHouse, Valkey, HTTP server, maupun OpenTelemetry Collector agar **Stackly sendiri** dapat berjalan. Tool tersebut adalah service yang dikelola, bukan dependency wajib core manager. Structured logs lokal cukup untuk foundation; integrasi observability eksternal adalah opsional.

## 16. Struktur pengembangan monorepo

Struktur berikut adalah usulan modul/logical workspace. Jangan membuat banyak crate kosong hanya untuk meniru tree; beberapa modul boleh berada dalam satu crate pada foundation, lalu diekstrak setelah boundary terbukti.

```text
stackly/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── README.md
├── LICENSE
├── CONTRIBUTING.md
├── CODE_OF_CONDUCT.md
├── SECURITY.md
├── GOVERNANCE.md
├── crates/
│   ├── domain/                       # Model, invariants, policies
│   ├── application/                  # Use cases dan orchestration
│   ├── runtime/                      # Sources, artifacts, installation
│   ├── instance/                     # Instance configuration/data policy
│   ├── supervisor/                   # Lifecycle, identity, recovery
│   ├── environment/                  # Graph, leases, lockfile
│   ├── provider-sdk/                 # Schema, types, safe primitives
│   ├── provider-engine/              # Loader, validation, execution plan
│   ├── provider-registry/            # Catalog dan trust metadata
│   ├── protocol/                     # Requests, responses, events, errors
│   ├── ipc/                          # Framing dan client/server transport
│   ├── storage/                      # SQLite, migrations, audit/journal
│   ├── security/                     # Trust, secret refs, redaction
│   ├── diagnostics/                  # Logs, metrics, doctor
│   ├── platform/                     # Platform traits/facade
│   ├── platform-macos/               # macOS-specific implementation
│   └── platform-windows/             # Dibangun pada fase Windows
├── apps/
│   ├── daemon/                       # Rust executable
│   ├── cli/                          # Rust executable
│   ├── macos/                        # SwiftUI + native integration
│   └── windows/                      # WinUI 3, fase berikutnya
├── clients/
│   ├── swift/                        # DTO/client dan contract fixtures
│   └── csharp/                       # DTO/client, fase Windows
├── protocols/
│   ├── schemas/
│   ├── fixtures/
│   └── compatibility/
├── schemas/
│   ├── provider.schema.json
│   ├── environment.schema.json
│   └── lockfile.schema.json
├── providers/
│   ├── postgresql/
│   ├── mysql/
│   ├── nats/
│   ├── clickhouse/
│   ├── valkey/
│   └── traefik/
├── tests/
│   ├── unit/
│   ├── contract/
│   ├── integration/
│   ├── e2e/
│   ├── security/
│   ├── recovery/
│   └── fixtures/
├── tools/
│   ├── provider-template/
│   ├── packaging/
│   ├── release/
│   └── scripts/
├── docs/
│   ├── plan/prd/{NNN}-stackly/
│   ├── architecture/decisions/
│   ├── getting-started/
│   ├── providers/
│   ├── security/
│   ├── development/
│   ├── release/
│   └── troubleshooting/
└── .github/
    ├── workflows/
    ├── ISSUE_TEMPLATE/
    └── PULL_REQUEST_TEMPLATE.md
```

Rust toolchain dan dependencies dipin setelah audit compatibility; PRD tidak mengarang nomor versi terbaru. Swift/Xcode dan Windows App SDK juga dikunci pada versi teruji di build matrix. Dependency baru harus disertai alasan, lisensi, biaya maintenance, dan pertimbangan reuse.

## 17. Model data registry

Schema fisik boleh disesuaikan, tetapi logical entities dan constraints berikut wajib ada. Semua waktu disimpan dalam UTC dan ditampilkan sesuai locale pengguna.

| Entity | Field inti | Constraints/perilaku |
| --- | --- | --- |
| `provider_sources` | id, kind, location, trust_root, last_sync, status | Source tidak otomatis trusted hanya karena HTTPS |
| `provider_revisions` | provider_id, revision, schema_version, manifest_digest, publisher, capabilities, approval | Revision immutable; unique provider + revision + digest |
| `artifacts` | id, provider_revision, version, os, arch, minimum_os, source, digest, signature_info, dependency_manifest | Satu artefak konkret; tidak menyimpan selector sebagai exact version |
| `runtimes` | id, artifact_id, install_mode, resolved_path, state, installed_at, last_verified | `managed` atau `external`; path aktif tidak dianggap immutable bila external |
| `runtime_dependencies` | runtime_id, dependency_runtime_id, constraint | Menahan GC dependency yang masih dipakai |
| `instances` | id, name, provider_revision, runtime_id, desired_state, config_revision, startup_policy, ownership, retained_at | Name unique per-user scope; data path stable berbasis ID |
| `instance_paths` | instance_id, kind, logical_root, resolved_path, owner, managed | Data, temp, config, log, socket; larangan writable overlap |
| `instance_endpoints` | instance_id, name, address, port, transport, enabled, requested_mode | Bind conflicts memerlukan validasi OS selain constraint SQL |
| `config_revisions` | id, instance_id, values, rendered_digest, previous_id, status, created_at | Secret disimpan sebagai reference; perubahan CAS/revision |
| `secret_references` | id, platform_store_key, scope, purpose, status | Tidak menyimpan nilai password/token |
| `environments` | id, name, manifest_location, manifest_digest, lock_digest, last_operation | Identitas tidak hanya bergantung pada working directory sementara |
| `environment_instances` | environment_id, alias, instance_id, ownership, dependency_conditions | Shared ref tidak menjadi owner data |
| `instance_demands` | instance_id, source_kind, source_id, state, session_policy | Lease environment/manual/startup dibedakan |
| `operations` | id, type, resource_id, idempotency_key, stage, status, progress, error_code, recovery_hint | Operation journal dan compensation scope |
| `process_observations` | instance_id, pid, start_identity, executable_identity, boot/session_id, observed_at | Bukan sumber kebenaran tunggal; tidak mengotorisasi kill dengan PID saja |
| `backups` | id, instance_id, method, scope, runtime_identity, path, checksum_manifest, consistency, verified_at | Pisahkan created dengan restore_verified |
| `audit_events` | id, actor, action, resource, operation_id, result, safe_details, timestamp | Retention terkontrol; bukan tamper-proof claim |
| `settings` | key, typed_value, revision | Policy global dan user preferences yang bukan state tampilan sementara |

Foreign key dan transaction enforcement wajib aktif. SQLite berada di filesystem lokal supported. Migrations berurutan, diuji dari versi sebelumnya, dan memiliki backup serta forward/rollback policy. Jangan menyalin file SQLite aktif secara sembarang; gunakan mekanisme backup konsisten yang sesuai. [S15]

State UI seperti posisi window dan selected tab boleh disimpan client; tidak menggandakan desired state layanan.

## 18. Kontrak IPC dan client native

### 18.1 Transport dan authorization

**Usulan baseline:** versioned JSON messages dengan framing length-prefixed melalui Unix domain socket pada macOS; Windows menggunakan named pipe dengan payload schema yang sama. Pilihan Protobuf boleh menggantikan encoding melalui ADR sebelum protocol stabil, tetapi tidak diperlukan hanya untuk mendapatkan IPC lokal.

Unix socket ditempatkan dalam runtime directory privat dengan path pendek dan permission pemilik saja. Server memverifikasi peer user identity melalui platform API. File permission dan pemeriksaan peer harus diuji, bukan hanya mengandalkan nama socket yang sulit ditebak. Client juga memeriksa identitas endpoint agar tidak mengirim secret ke server lokal palsu.

Pada Windows, named pipe memakai security descriptor terbatas pada SID yang berhak, bukan default broad access; local-only policy serta first-instance/identity behavior diuji. Microsoft mendokumentasikan bahwa akses pipe mengikuti security descriptor dan token client. [S17]

Scope kepercayaan baseline adalah **local user**, bukan jaminan bahwa semua proses dalam user itu jinak. Tidak ada shared global password atau token yang hardcode di aplikasi. Remote IPC dan HTTP admin port tidak dibuka.

### 18.2 Handshake dan message contract

Handshake menyertakan protocol major/minor, application version, supported capabilities, client type, server instance identity, schema revision dan event cursor. Major incompatible ditolak. Tambahan minor harus additive; client lama dapat mengabaikan field nonsecurity yang tidak dikenal sesuai schema policy.

Contoh payload konseptual; UUID dan nilai adalah ilustrasi, bukan respons aplikasi aktual:

```json
{
  "protocol": {"major": 1, "minor": 0},
  "requestId": "request-001",
  "idempotencyKey": "start-instance-a-001",
  "method": "instance.start",
  "params": {
    "instanceId": "instance-a",
    "expectedRevision": 7
  }
}
```

```json
{
  "requestId": "request-001",
  "result": {
    "operationId": "operation-001",
    "status": "queued"
  }
}
```

Core API minimum:

| Area | Operasi kontrak |
| --- | --- |
| Daemon | Handshake, GetInfo, GetHealth, GetCapabilities |
| Provider | List, Inspect, Validate, Import, Approve, Test |
| Runtime | Resolve, Install, RegisterExternal, List, Inspect, Verify, Remove |
| Instance | Create, Inspect, List, Start, Stop, Restart, Reload, UpdateConfig, Remove, Purge, Adopt |
| Config | GetSchema, Validate, PreviewDiff, ApplyRevision, GetHistory |
| Environment | Validate, Plan, ResolveLock, Up, Down, Inspect, Export |
| Recovery | CreateBackup, ListBackups, VerifyBackup, RestoreAsNew, PreviewOverwrite |
| Observability | SubscribeEvents, TailLogs, ReadLogPage, GetMetrics, GetAudit |
| Operations | GetOperation, ListOperations, CancelOperation, ResumeWhenSupported |
| Diagnostics | RunDoctor, PreviewDiagnosticBundle, ExportDiagnosticBundle |

Semua async operations mempunyai state `queued/running/succeeded/failed/cancel_requested/cancelled/needs_recovery` dan tidak menggunakan boolean success sebelum pekerjaan selesai. Client disconnect tidak membatalkan pekerjaan. Progress indeterminate digunakan jika ukuran kerja belum diketahui; jangan membuat persentase palsu.

### 18.3 Event dan backpressure

Event mempunyai monotonic sequence untuk stream daemon, timestamp, resource revision, type dan safe payload. Jenis event minimum: `instance.state_changed`, `instance.health_changed`, `operation.progress`, `runtime.installed`, `config.drift_detected`, `provider.trust_changed`, dan `daemon.reconciling`.

Snapshot + cursor menjadi pola reconnect. Event/log delivery **tidak diklaim exactly-once**; client melakukan dedup dan menangani gap dengan re-fetch snapshot. Usulan frame control maksimal 1 MiB, log batch maksimal 64 KiB, dan subscription queue berbatas; angka dapat dikunci melalui ADR/performance test.

Swift DTO/client dan C# DTO/client berasal dari schema atau fixture yang sama. CI menguji serialisasi enum, optional field, timestamps, error code, integer width, Unicode, path abstraction dan forward compatibility.

## 19. State machine dan process supervision

### 19.1 Pisahkan tiga state domain

| State domain | Nilai contoh |
| --- | --- |
| Runtime installation | `absent`, `resolving`, `downloading`, `verifying`, `installing`, `installed`, `failed`, `missing`, `drifted` |
| Instance lifecycle | `uninitialized`, `initializing`, `stopped`, `starting`, `running`, `stopping`, `failed`, `recovering`, `unknown`, `retained` |
| Readiness/health | Readiness: `unknown/not_ready/ready`; health: `unknown/healthy/degraded/unhealthy`; timestamp dan reason wajib |

Desired state disimpan terpisah dari actual lifecycle. `Running → Ready` bukan rangkaian state process yang menghapus informasi bahwa proses masih hidup. `Unhealthy` tidak selalu berarti proses harus dihentikan atau direstart.

### 19.2 Startup transaction

```text
Validate request / expected revision
→ acquire instance operation lock
→ verify runtime / dependency / trust / data ownership
→ validate all endpoints and configuration
→ persist starting intent / operation journal
→ render config atomically if required
→ spawn verified executable with explicit argv/cwd/env
→ capture process identity and tree ownership
→ run readiness probe with deadline
→ publish running + readiness state and connection endpoints
→ complete operation and release lock
```

Port preflight tidak menjamin port tetap bebas sampai executable melakukan bind. Bind failure tetap harus ditangani. Port manager tidak boleh membunuh proses luar untuk membuat port tersedia.

### 19.3 Stop dan restart

Provider menghasilkan shutdown strategy terkontrol. PostgreSQL dapat memakai tool kontrol yang cocok dengan cluster dan mode shutdown yang telah ditetapkan. Dokumentasi `pg_ctl` membedakan smart, fast, dan immediate; mode immediate memerlukan crash recovery pada start berikutnya. [S04]

Jangan menerapkan `SIGKILL` sebagai stop normal. Setelah timeout, tampilkan proses masih stopping serta opsi force dengan risiko yang jelas. Database yang gagal stop bersih tidak boleh dijadikan dasar cold backup berlabel clean.

Restart menangani stop → verify stopped → verify endpoint/data lock → start. Stop/start request bersamaan diserialisasi. Setelah manual stop, reconciler menghormati intent pengguna dan tidak menciptakan crash loop.

### 19.4 Pemilik restart dan hubungan dengan OS

**Baseline:** OS service manager menjaga daemon Stackly; Rust supervisor mengelola instance. Jangan mengaktifkan dua supervisor yang sama-sama mencoba restart instance yang sama. Homebrew services tidak dipakai untuk mengelola instance yang telah menjadi milik Stackly.

Spawn menggunakan mode foreground jika tool mendukung, process group/tree yang dapat ditelusuri, absolute executable path, working directory dan environment yang disanitasi. Jangan membiarkan library Rust yang dilepas karena client disconnect ikut menghentikan service.

Menutup GUI tidak menghentikan daemon. **Restart/crash daemon tidak dijanjikan zero downtime**: platform dapat menghentikan child process, atau proses mungkin tetap hidup. Reconciler harus menangani kedua keadaan tanpa memulai server kedua pada data yang sama. Restart daemon yang direncanakan melakukan stop aman terhadap instance terdampak bila belum ada handover supervisor yang terbukti, lalu memulihkan desired state; UI memperlihatkan disruption window.

Untuk proses surviving, verifikasi PID, start time/identity, executable/runtime identity, user, boot/session identity, dan data/config association sebelum adoption atau stop. Jika identitas tidak dapat dibuktikan, gunakan `unknown` dan diagnosis; jangan memaksakan kill.

Usulan restart policy: exponential backoff dengan jitter, maksimum 5 percobaan per 5 menit, reset setelah periode sehat yang dikonfigurasi. Setelah budget habis, hentikan retry dan tampilkan last failure. Provider dapat membutuhkan timeout berbeda; jangan menyamakan recovery PostgreSQL dan startup proxy ringan.

### 19.5 Sleep, logout, reboot, dan multi-user

Tidur/wake bukan bukti service crash. Health yang gagal sementara harus memiliki freshness/debounce policy. Setelah wake, ulangi pemeriksaan endpoint tanpa mengganti port fixed. Logout mengakhiri user-session lifecycle; boot tanpa login bukan janji baseline.

Pada mesin multi-user, namespace registry/socket/data terpisah per-user, tetapi TCP port host tetap dapat konflik antar-user. Permission direktori tidak menghilangkan konflik port host. Test harus mencakup background permission yang dicabut dan dua profil user.

## 20. Penyimpanan filesystem dan binary

### 20.1 Layout macOS usulan

```text
~/Library/Application Support/Stackly/
├── registry/stackly.sqlite
├── providers/<namespace>/<provider>/<revision>/
├── runtimes/<provider>/<version>/<os-arch>/<artifact-digest>/
├── instances/<instance-id>/
│   ├── data/
│   ├── config/
│   ├── config-history/
│   ├── temp/
│   └── instance-manifest.json
├── environments/<environment-id>/
├── operations/
├── backups/
└── retained/

~/Library/Logs/Stackly/<instance-id>/
~/Library/Caches/Stackly/downloads/
<private-user-runtime-directory>/stackly/daemon.sock
```

Layout adalah usulan, bukan hasil inspeksi Herd. Gunakan platform-standard directory resolver, jangan hardcode username atau `/opt/homebrew`. Socket path pendek ditentukan platform adapter agar tidak melewati limit sistem.

File PID dan socket adalah ephemeral state; registry/config/data bukan cache. Runtime install staged pada volume yang mendukung atomic rename ke destination. Shared dependency harus mempunyai reference counting dan dependency identity, tidak sekadar menyalin satu executable yang kebetulan berjalan di mesin developer.

### 20.2 Managed versus external

| Mode | Janji Stackly | Batas |
| --- | --- | --- |
| Managed | Exact artifact/version dan dependency terverifikasi; install/update/remove melalui manager | Hanya versi yang paketnya tersedia dan lulus test |
| External | Verifikasi lokasi/version/digest, own-instance config/data, drift detection | Binary dapat berubah/dihapus tool lain; tidak dapat menjamin reproducible install |
| Build-from-source | Dapat menjadi pipeline maintainer untuk menerbitkan paket teruji | Bukan kewajiban end-user menginstal toolchain untuk baseline supported |

Homebrew mempunyai versioned keg dan `opt` symlink ke versi aktif; command upgrade/cleanup juga dapat mengubah atau membersihkan instalasi. Karena itu, reference ke symlink global saja tidak cukup untuk pinning runtime. Fitur pin Homebrew bukan pengganti lockfile dan dependency control Stackly. [S16]

Packaging runtime macOS, terutama PostgreSQL beserta `lib`, `share`, extensions, OpenSSL/ICU bila dibutuhkan, merupakan feasibility gate awal. **Jangan menganggap semua software tersedia sebagai satu binary standalone yang mudah dipindahkan.** Jika managed artifact untuk provider wajib belum tersedia, tandai blocker; jangan mengklaim clean-install tanpa dependency ketika sebenarnya mengandalkan Homebrew developer.

## 21. Provider SDK dan manifest

### 21.1 Paket provider

```text
providers/nats/
├── provider.yaml
├── versions.json
├── schemas/settings.schema.json
├── templates/nats.conf.tmpl
├── tests/fixtures/
├── tests/scenarios.yaml
├── README.md
└── LICENSE-NOTICES.md
```

`versions.json` menunjuk artefak yang benar-benar ada, dengan digest asli. Metadata schema tidak boleh berisi fabricated URL/checksum agar terlihat lengkap. Registry dapat di-host terpisah tetapi pinned provider revision tetap tersimpan lokal.

### 21.2 Manifest ilustratif

Berikut adalah proposal schema yang valid sebagai contoh struktural. Ia belum bisa diinstal karena version catalog dan template riil harus disediakan serta diverifikasi.

```yaml
apiVersion: stackly/v1alpha1
kind: Provider
metadata:
  id: official/nats
  displayName: NATS Server
  revision: "1.0.0"
  category: messaging

compatibility:
  coreProtocolMajor: 1
  platforms:
    - {os: macos, arch: arm64}

runtime:
  executable: bin/nats-server
  versionCatalog: versions.json
  executionMode: foreground

capabilities:
  install: true
  multiInstance: true
  persistence: true
  reload: false
  metrics: true
  backupMethods: [cold]

settingsSchema: schemas/settings.schema.json

resources:
  paths: [data, config, logs, temp]
  endpoints:
    client: {transport: tcp, defaultHost: "127.0.0.1", defaultPort: 4222}
    monitoring: {transport: tcp, defaultHost: "127.0.0.1", defaultPort: 8222}

lifecycle:
  initialize:
    - {primitive: createManagedDirectories}
    - primitive: renderTemplate
      source: templates/nats.conf.tmpl
      destination: "${instance.config}/nats.conf"
  start:
    executable: "${runtime.executable}"
    args: ["--config", "${instance.config}/nats.conf"]
    cwd: "${instance.data}"
  stop:
    strategy: gracefulProcess
    timeoutSeconds: 30

health:
  type: providerProbe
  probe: nats-readiness
  endpoint: client

permissions:
  managedFilesystem: [data, config, logs, temp]
  executeRuntimeBinary: true
  additionalExecutables: []
```

`${...}` adalah interpolation terkontrol oleh engine, **bukan shell expansion**. Daemon harus menyelesaikan token dari context yang tervalidasi. Placeholder tidak dapat memanggil command, membaca sembarang environment variable, atau menyisipkan subexpression executable.

Nilai `reload: false` pada contoh mengilustrasikan bahwa provider dapat memulai dengan capability terbatas; menambahnya kemudian membutuhkan pengujian. Probe `nats-readiness` adalah primitive yang harus benar-benar diimplementasikan; bukan API NATS baru yang diasumsikan tersedia.

### 21.3 Primitive baseline

Engine menyediakan primitive untuk create owned directory, render typed template, resolve secret reference, verify artifact, spawn approved executable dengan argv, graceful stop, probe TCP/HTTP/provider-native, collect log, serta cold backup approved paths setelah quiesce.

PostgreSQL initialization membutuhkan `initdb`; tool tersebut harus didaftarkan sebagai executable tambahan yang berada dalam runtime terverifikasi. Secret input tidak digabungkan ke shell string. Operasi database spesifik yang kompleks memakai reviewed built-in adapter sesuai kontrak, bukan unrestricted lifecycle hook.

Provider deklaratif tidak dapat memenuhi semua tool secara otomatis. Jika kebutuhan tidak bisa diekspresikan dengan primitive existing, contributor mengajukan perluasan SDK/core yang ditinjau. WASM/component plugin dan custom hooks ditunda sampai permission model serta ABI stabil; tidak dijadikan dependency baseline.

### 21.4 Trust dan review

Trust approval terikat kepada package identity, revision, digest, source/publisher dan capability set. Catalog yang dibaca tidak menjalankan kode. Provider local/unverified memerlukan opt-in dan warning bahwa native executable berjalan dengan hak user.

Signature/hash melindungi integritas sesuai trust root, tetapi tidak membuktikan publisher baik. Revocation metadata mempunyai freshness policy: offline execution runtime yang sudah dipasang tetap dapat berjalan sesuai kebijakan lokal; installer tidak mengklaim revocation list terkini jika offline.

Permission engine membatasi tindakan **host engine**, tetapi bukan hard sandbox bagi child executable. Ini harus terlihat pada developer docs dan user confirmation, bukan hanya dalam security appendix.

## 22. Integrasi khusus enam provider

### 22.1 PostgreSQL

Inisialisasi cluster terpisah menggunakan `initdb` versi runtime yang dipilih. Credential bootstrap, locale/encoding, auth policy dan initial database eksplisit; hindari default `trust` tanpa sengaja. Port, listen address, Unix socket, data/log/temp directory, serta file konfigurasi dikelola per-instance. [S08]

Readiness minimal memeriksa server accepting connections dan authenticated query menggunakan role probe berhak minimum. Jangan menggunakan superuser default untuk setiap health check jika role terbatas cukup. Tool admin menggunakan runtime yang cocok; GUI tidak memilih `psql` global dari PATH.

Major upgrade 17 → 18 **bukan** rebind runtime biasa. Produk baseline dapat mengarahkan pengguna ke backup/restore dan panduan migrasi yang sesuai; otomasi `pg_upgrade` penuh adalah scope terpisah dengan preflight extensions, locale, data compatibility, disk dan rollback. [S10]

### 22.2 NATS

Setiap instance memiliki server name unik, client endpoint, monitoring loopback, config dan JetStream `store_dir` sendiri. Konfigurasi JetStream juga menetapkan resource policy yang terlihat pengguna. Core messaging dan JetStream bukan hal yang sama; persistence hanya diklaim untuk mode/storage yang benar-benar diaktifkan. [S11]

Monitoring NATS jangan dianggap aman untuk diekspos ke LAN hanya karena berguna bagi UI. Manager dapat membaca endpoint monitoring lokal, tetapi credential/URL sensitif disanitasi. Health readiness mencakup konektivitas dan mode JetStream yang diminta, bukan sekadar HTTP 200 dari proses yang salah. [S09]

### 22.3 ClickHouse

Paket native macOS yang dipilih diverifikasi executable, dependency, signature policy dan parameter server-nya. ClickHouse CLI resmi juga telah menyediakan pengelolaan versi/local server; hal itu menjadi referensi upstream atau kemungkinan source adapter, **bukan supervisor kedua yang berjalan bersamaan dengan Stackly**. [S06]

Konfigurasi wajib mengarahkan data, temp, logs, user files, user/access configuration dan path relevan lain ke scope instance. Provider mengaudit seluruh listener aktif; port HTTP/native TCP saja bukan seluruh kemungkinan endpoint. Baseline single-node tidak mengaktifkan cluster/replication/Keeper secara diam-diam.

Readiness mencakup query dan identitas endpoint yang benar. Backup online ClickHouse memiliki mekanisme upstream tersendiri; dukungan itu tidak boleh diasumsikan identik dengan backup file generik. [S20]

### 22.4 Valkey

Mode volatile harus disebut sebagai pilihan cache tanpa jaminan persistence. Mode persistent menyimpan config RDB/AOF dan seluruh file terkait pada path instance. Ketika AOF menggunakan beberapa file atau manifest pada versi target, backup tidak boleh hanya mengambil satu filename lama.

Valkey menyediakan RDB, AOF, gabungan keduanya, atau persistence nonaktif; durability bergantung pada kebijakan yang dipilih. Test recovery harus mencatat konfigurasi tersebut dan membedakan shutdown normal, crash process, serta kehilangan daya host. [S21]

Jangan mengganti Valkey dengan Redis, Memurai, atau image container secara diam-diam karena platform tidak tersedia. Redis adalah provider/backend terpisah dengan capability, kompatibilitas, dan ketentuan distribusinya sendiri.

### 22.5 Traefik

Bedakan startup configuration dan routing configuration. Startup mengatur entrypoint/provider; file provider menyuplai route dinamis. Baseline Stackly menggunakan file provider untuk mengelola routing tanpa dependency Docker. [S12], [S13]

Route mempunyai ID/name yang dinamespace per-instance/environment. File routing ditulis atomik dan file-watching behavior harus diuji. Jika suatu field memerlukan restart, UI menampilkan dampaknya. Health endpoint memakai listener loopback khusus agar tidak konflik dengan contoh HTTP `8080`.

Dashboard/API insecure tidak diaktifkan secara default. HTTPS hanya aktif dengan certificate/key yang valid dan cara penyimpanan yang aman. Tidak ada janji wildcard domain lokal, port 80/443, trusted certificate otomatis, atau perubahan `/etc/hosts` tanpa fitur terpisah yang secara eksplisit disetujui.

### 22.6 MySQL

Setiap instance menggunakan data, config, temporary, dan log path sendiri; bind loopback dan autentikasi dikonfigurasi eksplisit. Readiness menjalankan query terautentikasi dengan hak minimum. Initialization dan shutdown menggunakan tool/lifecycle yang sesuai versi MySQL terpilih. [S22]

Backup/restore mengikuti metode MySQL yang tervalidasi untuk scope data yang dipilih; copy filesystem hanya boleh dilakukan setelah shutdown bersih dan validasi layout. Uji restore-as-new wajib memverifikasi data sentinel dan kompatibilitas runtime. Major upgrade tidak dilakukan dengan mengganti binary pada data directory yang sama tanpa preflight dan prosedur migrasi teruji. [S23]

## 23. Keamanan dan model ancaman

| Ancaman | Kontrol wajib | Residual risk yang harus dinyatakan |
| --- | --- | --- |
| Manifest/provider berbahaya | Explicit trust, signed/digested metadata, safe primitives, review/test | Executable native tetap mempunyai hak user setelah dijalankan |
| Supply-chain tampering | HTTPS, expected digest, signature/provenance policy, staged verify | Trust root/publisher yang disusupi dapat tetap menerbitkan binary berbahaya |
| IPC dari user lain | Private endpoint, peer identity, pipe ACL, no remote admin listener | Tidak melindungi dari admin/root atau malicious process dengan identitas user yang sama |
| Shell injection | Argv array, no shell interpolation, allowed executable/cwd/env | Executable sendiri dapat menafsirkan opsi berbahaya; schema harus membatasi parameter |
| Path traversal/purge escape | Canonical ownership validation, safe extraction, symlink race handling | User pemilik tetap dapat memodifikasi filesystem di luar manager |
| PID reuse/proses salah | Compound process identity, tree association, no kill-by-name | Jika identitas tidak dapat diverifikasi, recovery menjadi manual/blocker |
| Database terekspos | Loopback defaults, auth, monitoring private, explicit non-loopback opt-in | Port lokal tetap dapat diakses proses lokal yang punya kredensial/akses |
| Secret leak | Keychain/secure store, restricted temp files, redaction, no default reveal | Tool yang mensyaratkan secret plaintext di config meninggalkan residual local-user exposure |
| Disk/log exhaustion | Rotation, bounded queue, free-space preflight, low-space alert | Provider dapat menulis data bisnis lebih cepat dari perkiraan manager |
| Update buruk | Verify, compatibility gate, safe restart, registry backup, canary | Rollback executable tidak otomatis membalikkan migrasi data |

Security review wajib mencakup archive parser, template engine, native process execution, purge/adoption, IPC framing, credentials, permissions, update/revocation dan diagnostic export. Fuzzing diprioritaskan untuk manifest/parser/framing/path handling.

Keychain dipakai untuk secrets macOS; detail access policy daemon/GUI dituangkan dalam ADR. Database metadata hanya menyimpan opaque reference. Windows memakai platform secret adapter yang dipilih dan diuji pada scope user yang sama; jangan menganggap format secret macOS dapat disalin langsung.

App Sandbox bukan syarat baseline direct distribution dan tidak diklaim aktif melindungi provider. Hardened Runtime/signing/notarization aplikasi juga **bukan** pengganti sandbox instance. Jangan menginstruksikan pengguna menonaktifkan Gatekeeper, SIP, atau menghapus quarantine secara global untuk membuat provider bekerja. [S14]

## 24. Backup, restore, upgrade dan recovery

### 24.1 Cakupan backup baseline

| Objek/provider | Minimum baseline | Di luar janji baseline |
| --- | --- | --- |
| Registry Stackly | Snapshot konsisten SQLite + nonsecret config + references | Backup otomatis seluruh Keychain atau database provider |
| PostgreSQL | Logical backup/restore untuk scope yang dipilih, termasuk penjelasan roles/global objects; cold backup validated layout sebagai opsi | PITR/WAL archive, online physical backup automation, migrasi mayor satu klik |
| NATS JetStream | Cold backup verified local instance storage setelah stop bersih; restore dengan runtime compatible | Distributed cluster snapshot orchestration dan zero-downtime migration |
| ClickHouse | Cold backup local managed layout setelah stop bersih; native backup dapat ditambah sebagai capability teruji | Seluruh external object-storage data atau cluster replication state otomatis |
| MySQL | Logical dump/restore untuk scope yang dipilih; cold backup hanya setelah stop bersih dan layout tervalidasi | PITR, binary-log recovery, replication, atau hot physical backup tanpa capability dan test khusus |
| Valkey persistent | Backup metode provider yang tervalidasi; minimum cold backup seluruh persistence set yang relevan | Jaminan mempertahankan key dari volatile mode atau semua writes saat power failure |
| Traefik | Export/backup config dan route; certificate secret handling eksplisit | Backup lengkap external certificate authority atau backend application data |

Metode native PostgreSQL mencakup beberapa pendekatan yang tidak dapat disamakan; setiap backup harus menyebut scope dan consistency. [S19] SQLite Online Backup API mendukung snapshot konsisten database aktif. [S15]

**Cold backup bukan menyalin file saat service masih hidup.** Dapatkan exclusive maintenance lock, evaluasi shared leases, stop bersih, verifikasi tidak ada proses/penulisan aktif, enumerasi semua path milik instance, copy ke staging, checksum, write manifest, commit, kemudian restore desired state yang disetujui. Jika ada tablespace/path eksternal yang belum didukung, tolak dengan alasan; jangan mengklaim backup lengkap.

Baseline backup manual disimpan lokal pada destination yang dipilih dengan permission privat. Tidak ada auto-delete backup pengguna; retention otomatis, incremental chain, S3 dan scheduled backup adalah perluasan yang memerlukan PRD/capability terpisah. Setiap backup menyatakan apakah secret material dikecualikan atau membutuhkan perlindungan khusus. Raw database backup dapat berisi data sensitif dan authentication material; jangan menandainya sanitized.

### 24.2 Restore workflow

```text
Select backup → inspect scope/compatibility/checksum
→ choose restore-as-new (default)
→ select supported runtime and new endpoints
→ preflight space/permissions/secret requirements
→ restore into owned staging data
→ validate data structure/provider preflight
→ commit new instance atomically where feasible
→ start + readiness + fixture/query verification
→ record restore verification evidence
```

Restore over existing data mempunyai separate preview dan explicit confirmation, stop safety, backup-before-overwrite policy dan journal. Jangan memindahkan data source setelah operasi baru dimulai lalu berharap undo selalu berhasil.

### 24.3 Update dan rollback

| Operasi | Aturan |
| --- | --- |
| Update GUI/CLI | Tidak mengubah database provider; handshake compatibility diperiksa |
| Update daemon | Koordinasi operasi aktif dan restart disruption; backup registry sebelum schema mutation |
| Update provider revision | Diff capability/template/permission dan explicit apply; instance lama tetap pin |
| Install runtime baru | Menambah instalasi berdampingan, tidak memindahkan instance |
| Rebind compatible runtime | Controlled stop, compatibility check, backup policy, start verification |
| Major database upgrade | Migrasi terpisah; tidak diasumsikan reversible dengan binary lama |
| Rollback manager | Hanya bila schema/protocol kompatibel atau registry recovery tervalidasi |

## 25. Integrasi macOS dan distribusi

Rilis pertama berupa aplikasi native `.app` dengan helper daemon dan CLI. Signed DMG dapat menjadi format distribusi awal; PKG dipertimbangkan untuk kebutuhan install helper/path yang membutuhkan alur installer. Tidak ada kewajiban Mac App Store.

SwiftUI mengelola consent background melalui mekanisme OS. Helper didaftarkan dengan pendekatan Service Management yang sesuai untuk deployment target; status `requiresApproval/disabled/notRegistered` harus menjadi state produk yang terlihat. `SMAppService` mendukung registration/control helper dalam konteks ini. [S02]

Distribution pipeline wajib menghasilkan signed app/helper sesuai identity yang sah, notarization dan stapling bila berlaku, verified artifact/checksum, release notes, daftar OS/arch supported dan third-party notices. Semua langkah diuji pada profil nondeveloper. Apple mendokumentasikan notarization sebagai bagian distribusi aplikasi macOS di luar store. [S14]

Downloaded runtimes tidak otomatis dicakup oleh signature/notarization aplikasi manager. Setiap sumber runtime perlu verification policy, kompatibilitas OS, serta perilaku Gatekeeper yang diuji. Jangan mengklaim signing manager membuat semua binary provider dipercaya sistem.

CLI installation tidak mengubah PATH global tanpa consent. Pengguna dapat memakai path eksplisit atau user-writable symlink yang dikelola. GUI uninstall dan retained-data cleanup dijelaskan terpisah; menghapus app bundle tidak boleh menghapus database pengguna.

Update check boleh otomatis hanya untuk metadata setelah preference/consent yang dinyatakan; instalasi update memerlukan workflow aman. Auto-apply update dapat ditunda sampai rollback dan operasi aktif telah terbukti aman. Ketersediaan signing/notarization credential adalah release blocker, bukan alasan mengarang bukti release berhasil.

## 26. Strategi porting Windows

Windows menggunakan Rust core/protocol yang sama dan GUI WinUI 3 native. Build/QA dilakukan dengan toolchain Windows dan runner yang sesuai; keberhasilan compile Rust lintas target saja tidak membuktikan installer, GUI, atau provider Windows bekerja. [S03]

| Area | Implementasi Windows yang perlu dibuat |
| --- | --- |
| GUI | C# + XAML, native navigation/commands, client state dan accessibility |
| IPC | Named pipe, user SID authorization, local-only policy, endpoint identity [S17] |
| Process | Process-tree ownership, shutdown semantics, handle inheritance, Job Objects bila tepat [S18] |
| Startup | Default usulan background process per-user; Windows Service/SCM merupakan mode sistem opsional yang butuh desain izin, bukan kewajiban semua tool |
| Paths | User data/cache/log dirs, path length, Unicode, file locks, rename semantics |
| Secrets | Platform secure store adapter; identity/scope konsisten |
| Runtime | Paket Windows per-arch, DLL/dependency, source/trust/compatibility matrix |
| Packaging | Installer bertanda tangan, dependency WinUI/Windows App SDK, update/uninstall behavior |

Tidak semua executable dapat langsung didaftarkan sebagai Windows Service. Supervisor dapat mengelola proses biasa; mode service memerlukan host/adapter yang cocok. Job Objects menyediakan mekanisme pengelompokan proses pada Windows, tetapi kebijakan kill-on-close tidak boleh diaktifkan tanpa mempertimbangkan lifecycle daemon dan data safety. [S18]

PostgreSQL, MySQL, dan NATS menjadi kandidat awal validasi Windows. ClickHouse tidak diberi label native Windows karena dokumentasi yang diperiksa menggunakan WSL. Valkey upstream tidak mendukung Windows native; jangan memakai WSL atau mengganti provider diam-diam. Ketidaktersediaan provider harus tampil sebagai capability gap. [S04], [S05], [S06], [S07], [S22]

## 27. CI/CD dan engineering workflow

Branch/PR pipeline minimal menjalankan format, lint, typecheck/build, unit tests, contract/schema validation, dependency/license audit, security static checks, dan fixture provider tests. Merge ke protected branch membutuhkan review serta check wajib lulus.

Provider integration jobs menggunakan version matrix dan machine yang sesuai; mereka tidak menyentuh database pribadi developer. Gunakan ephemeral test root, random/allocated test ports, fixture credentials, cleanup ownership, dan artefak log yang disanitasi.

Nightly/release candidate menambahkan fault injection, daemon restart, clean-install, backup/restore, prolonged log stream, filesystem edge cases, dan macOS background approval tests. Signing hanya dilakukan dari trusted release context; PR dari fork tidak mendapat signing secret atau registry publishing credential.

Release artifact mempunyai source commit, toolchain, dependency lock, provider catalog revision, checksums, SBOM/third-party inventory sesuai kebijakan, hasil test dan release checklist. Signed metadata dan published binary harus cocok. Tidak ada klaim reproducible build bit-for-bit sampai pipeline itu benar-benar dibuktikan.

---

<a id="layer-3"></a>

# LAPISAN 3 — ACCEPTANCE CRITERIA & VERIFICATION

## 28. Standar bukti dan status

Setiap AC berikut berstatus awal **NOT_RUN**. Status yang diperbolehkan: `NOT_RUN`, `PASS`, `FAIL`, `BLOCKED`, atau `DEFERRED` untuk scope yang memang di luar baseline. Mandatory AC tidak boleh dijadikan DEFERRED hanya untuk meloloskan rilis tanpa scope change yang disetujui.

Evidence record wajib memuat AC ID, source commit, provider/runtime exact version, OS/arch, test command atau langkah manual, expected/actual result, timestamp, evidence path, dan reviewer bila manual. Fixture/mock report dibedakan dari native runtime integration report.

`PASS` berarti hasil aktual sesuai kriteria; bukan sekadar kode test telah ditulis. **BLOCKED pada mandatory security, data safety, packaging atau compatibility berarti belum production-ready.**

## 29. Acceptance criteria utama

Nomor AC-001 sampai AC-054 berpasangan langsung dengan REQ bernomor sama. AC-081 memverifikasi REQ-055; AC tambahan lintas fitur mempertahankan ID yang sudah ada. Setiap skenario menggunakan pola **Given / When / Then**.

### Platform & lifecycle (AC-001)

| ID | Skenario dan hasil yang harus terlihat | Trace |
| --- | --- | --- |
| AC-001 | Given tiga instance ready; when GUI ditutup dan dibuka kembali; then PID/identitas instance tetap konsisten, query tetap berhasil, dan UI mengambil ulang state dari daemon. | REQ-001 |

### Catalog & runtime (AC-002–007)

| ID | Skenario dan hasil yang harus terlihat | Trace |
| --- | --- | --- |
| AC-002 | Given provider untuk platform lain; when catalog dibuka; then provider berlabel unsupported dengan alasan dan aksi install ditolak oleh daemon, bukan hanya disembunyikan UI. | REQ-002 |
| AC-003 | Given instance memakai runtime konkret; when catalog mendapat versi baru; then runtime ID, digest, dan versi instance lama tetap sama sampai ada operasi update eksplisit. | REQ-003 |
| AC-004 | Given unduhan putus setelah sebagian byte; when install diulang; then hanya artefak utuh terverifikasi yang menjadi installed dan tidak ada binary parsial yang dijalankan. | REQ-004 |
| AC-005 | Given ARM64 dan artefak x86_64 tanpa dukungan yang dinyatakan; when install/start diminta; then operasi ditolak dengan diagnosis yang benar, bukan menjalankan emulasi diam-diam. | REQ-005 |
| AC-006 | Given runtime external terdaftar; when binary diubah atau dihapus di luar Stackly; then drift/missing terdeteksi dan Stackly tidak mengganti ke binary lain atau menghapus instalasi tersebut. | REQ-006 |
| AC-007 | Given runtime dipakai instance stopped; when remove/GC dijalankan; then runtime ditahan dengan daftar referensi dan data instance tidak berubah. | REQ-007 |

### Instance & supervision (AC-008–014)

| ID | Skenario dan hasil yang harus terlihat | Trace |
| --- | --- | --- |
| AC-008 | Given data directory berisi data; when create mencoba menggunakannya tanpa alur adoption; then operasi ditolak sebelum initdb atau inisialisasi provider berjalan. | REQ-008 |
| AC-009 | Given dua instance PostgreSQL mayor 17 dengan runtime ID sama serta satu mayor 18; when ketiganya menerima data sentinel berbeda; then pembacaan dan restart per-instance tidak menampilkan atau mengubah sentinel instance lain. | REQ-009 |
| AC-010 | Given monitoring port NATS sudah digunakan proses luar; when instance start; then kegagalan menunjuk monitoring endpoint yang konflik dan proses luar tidak dihentikan. | REQ-010 |
| AC-011 | Given koneksi PostgreSQL aktif; when stop normal dipilih; then strategi shutdown provider dijalankan, hasil dilaporkan, dan tidak ada kill generik terhadap seluruh proses bernama postgres. | REQ-011 |
| AC-012 | Given proses hidup tetapi query readiness gagal; when status diminta; then process=running dan readiness=not_ready ditampilkan, bukan ready. | REQ-012 |
| AC-013 | Given daemon crash dan restart; when reconciliation selesai; then tidak ada instance ganda, tidak ada penghapusan data, dan proses yang identitasnya meragukan berstatus unknown/blocker tanpa dibunuh. | REQ-013 |
| AC-014 | Given autostart aktif; when user login kembali dan izin OS tersedia; then daemon serta instance yang dipilih ready; jika izin dicabut, UI menunjukkan alasan dan tidak mengubah setting OS diam-diam. | REQ-014 |

### Configuration & diagnostics (AC-015–019)

| ID | Skenario dan hasil yang harus terlihat | Trace |
| --- | --- | --- |
| AC-015 | Given edit config tidak valid; when apply; then active config dan instance sehat tetap dipertahankan, error menunjuk field/line yang relevan, dan tidak ada restart parsial. | REQ-015 |
| AC-016 | Given credential sentinel; when inspect/export/log/diagnostic dilakukan; then sentinel tidak muncul dalam hasil default atau database metadata dan reveal hanya terjadi setelah aksi eksplisit. | REQ-016 |
| AC-017 | Given provider menghasilkan log besar dan client lambat; when streaming berlangsung; then service tetap responsif, buffer dibatasi, rotasi berjalan, dan kehilangan segmen ditandai gap/truncation. | REQ-017 |
| AC-018 | Given provider multiprocess; when metrics ditampilkan; then label menjelaskan cakupan agregasi dan metrik tidak tersedia tidak ditampilkan sebagai nol yang menyesatkan. | REQ-018 |
| AC-019 | Given stale PID record yang menunjuk proses lain; when doctor berjalan; then masalah dilaporkan beserta bukti, tanpa menghapus file data atau menghentikan proses tersebut. | REQ-019 |

### Environment (AC-020–023)

| ID | Skenario dan hasil yang harus terlihat | Trace |
| --- | --- | --- |
| AC-020 | Given manifest baru; when env plan; then daftar install/create/start terlihat tetapi tidak ada file data, proses, perubahan trust, atau runtime yang dibuat. | REQ-020 |
| AC-021 | Given dependency tidak ready; when env up; then dependent tidak dijalankan sebagai sukses, instance existing yang sehat tidak dihapus, dan operation menunjukkan tahap gagal serta langkah recovery. | REQ-021 |
| AC-022 | Given dua environment memakai shared instance; when environment A down; then lease B tetap ada dan instance tetap ready; data tidak dihapus. | REQ-022 |
| AC-023 | Given environment diekspor dan diimpor ke user lain; when resolve dilakukan; then tidak ada path home/secret lama dan artefak unsupported dilaporkan tanpa auto-substitution. | REQ-023 |

### Native GUI (AC-024–027)

| ID | Skenario dan hasil yang harus terlihat | Trace |
| --- | --- | --- |
| AC-024 | Given daemon unreachable; when GUI dibuka; then UI tetap dapat menampilkan status koneksi dan retry/diagnostics tanpa menyatakan instance stopped atau menjalankan binary langsung. | REQ-024 |
| AC-025 | Given duplicate instance config; when wizard diselesaikan; then ID, data path, secret policy, dan port baru tervalidasi; data lama tidak disalin tanpa fitur backup/restore. | REQ-025 |
| AC-026 | Given provider tidak memiliki hot reload; when config berubah; then UI meminta controlled restart dan tidak menampilkan keberhasilan reload palsu. | REQ-026 |
| AC-027 | Given navigasi interaksi desktop; when pengguna membuat, menghentikan, dan memeriksa instance; then alur dapat diselesaikan tanpa informasi yang hanya tersedia lewat warna atau hover. | REQ-027 |

### CLI & provider (AC-028–034)

| ID | Skenario dan hasil yang harus terlihat | Trace |
| --- | --- | --- |
| AC-028 | Given instance dapat dibuat dari GUI; when operasi setara dilakukan CLI --json; then validation/error semantics sama dan output dapat diparse tanpa progress bercampur di stdout. | REQ-028 |
| AC-029 | Given provider fake-server baru dengan manifest dan template valid; when dimuat; then install/create/start/health/form UI bekerja tanpa perubahan Rust core atau SwiftUI. | REQ-029 |
| AC-030 | Given manifest dengan versi schema tidak didukung; when validate/load; then error menjelaskan versi yang didukung dan tidak mengeksekusi lifecycle. | REQ-030 |
| AC-031 | Given field command berisi shell metacharacter dan payload; when provider dirender/dijalankan; then tidak terjadi shell execution dan command yang melanggar schema ditolak. | REQ-031 |
| AC-032 | Given provider revision baru meminta executable/sumber/izin berbeda; when update; then trust lama tidak otomatis mencakup perubahan tersebut dan install menunggu approval/policy. | REQ-032 |
| AC-033 | Given local provider belum dipercaya; when file diimpor untuk preview; then metadata dapat diperiksa tetapi executable dan init hook tidak berjalan. | REQ-033 |
| AC-034 | Given contributor mengikuti panduan dari repository bersih; when contoh provider dijalankan; then validator dan test harness dapat menghasilkan report tanpa modifikasi core. | REQ-034 |

### Provider awal (AC-035–039, AC-081)

| ID | Skenario dan hasil yang harus terlihat | Trace |
| --- | --- | --- |
| AC-035 | Given matrix PG17/PG18 dan same-version pair; when init/write/restart/backup/restore dijalankan; then seluruh skenario lulus dan startup binary mayor yang salah terhadap data existing ditolak. | REQ-035 |
| AC-036 | Given dua NATS dengan JetStream dan store_dir berbeda; when publish/consume persistent fixture dan restart; then masing-masing hanya mengakses stream sendiri dan monitoring berada pada endpoint masing-masing. | REQ-036 |
| AC-037 | Given dua ClickHouse instances; when tabel bernama sama diisi data berbeda dan masing-masing direstart; then data/ports tidak tercampur dan seluruh listener aktif cocok dengan resource registry. | REQ-037 |
| AC-038 | Given Valkey persistence aktif dengan fixture tersimpan; when shutdown normal dan start; then fixture pulih; ketika mode volatile dipilih, UI menunjukkan data dapat hilang dan tidak menjanjikan restore. | REQ-038 |
| AC-039 | Given dua Traefik instances dan backend uji; when route A diubah; then hanya request pada instance A yang berubah, B tetap berjalan, dan config invalid tidak mengganti last-known-good config. | REQ-039 |
| AC-081 | Given dua instance MySQL dengan runtime kompatibel, data directory, dan port berbeda; when data sentinel ditulis, kedua instance direstart, dan restore-as-new diuji; then query hanya membaca data milik instance yang sesuai dan runtime tidak dapat direbind ke data yang tidak kompatibel. | REQ-055 |

### Data safety & release (AC-040–046)

| ID | Skenario dan hasil yang harus terlihat | Trace |
| --- | --- | --- |
| AC-040 | Given registry backup saat aplikasi aktif; when dipulihkan pada profil uji; then metadata konsisten, missing secret ditandai, dan tidak ada instance otomatis di-init pada path data existing. | REQ-040 |
| AC-041 | Given service masih menulis; when cold backup diminta; then quiesce/stop dan verifikasi proses selesai dilakukan sebelum copy; external path yang tak didukung membuat operasi ditolak, bukan backup parsial berlabel complete. | REQ-041 |
| AC-042 | Given backup valid; when restore-as-new; then instance sumber tidak berubah, instance baru lulus query/data fixture, dan checksum/compatibility failure tidak menimpa target. | REQ-042 |
| AC-043 | Given PG17 data dan runtime PG18; when pengguna mencoba rebind biasa; then operasi ditolak dengan kebutuhan migration; konfigurasi sumber dan data tetap utuh. | REQ-043 |
| AC-044 | Given update manager baru; when diterapkan; then runtime IDs/data provider tidak berubah, operasi aktif ditangani aman, dan schema incompatible mencegah rollback buta. | REQ-044 |
| AC-045 | Given create/start/change-port/purge; when operasi selesai atau gagal; then audit mempunyai correlation ID dan result tanpa credential plaintext. | REQ-045 |
| AC-046 | Given network dimatikan; when instance installed dijalankan dari GUI/CLI; then fungsi lokal bekerja; artefak tidak tersedia menghasilkan offline/cache-missing yang benar. | REQ-046 |

### IPC & integrity (AC-047–048)

| ID | Skenario dan hasil yang harus terlihat | Trace |
| --- | --- | --- |
| AC-047 | Given client UID tidak berhak atau protocol major berbeda; when koneksi/mutation dicoba; then ditolak sebelum side effect dan error aman dikembalikan. | REQ-047 |
| AC-048 | Given GUI dan CLI mengirim start/create bersamaan dengan idempotency key sama; when diproses; then hanya satu side effect dibuat dan kedua client menerima operation/result konsisten. | REQ-048 |

### Data safety & release (AC-049 packaging)

| ID | Skenario dan hasil yang harus terlihat | Trace |
| --- | --- | --- |
| AC-049 | Given profil macOS bersih pada matrix supported; when signed distribution diinstal; then GUI/daemon/CLI dan provider wajib dapat digunakan tanpa developer toolchain serta tanpa menonaktifkan Gatekeeper. | REQ-049 |

### Platform & lifecycle (AC-050 cross-platform core)

| ID | Skenario dan hasil yang harus terlihat | Trace |
| --- | --- | --- |
| AC-050 | Given test platform fake dan cross-target core check; when kontrak diuji; then tidak ada dependency GUI/macOS di domain dan not-supported dikembalikan untuk capability Windows yang belum dibuat. | REQ-050 |

### Data safety & release (AC-051–053)

| ID | Skenario dan hasil yang harus terlihat | Trace |
| --- | --- | --- |
| AC-051 | Given candidate release; when satu mandatory restore test BLOCKED; then release report menyatakan belum stable-ready dan tidak mengubah status test menjadi PASS. | REQ-051 |
| AC-052 | Given pengguna memilih remove tanpa purge; when selesai; then data masih ada dan tercatat retained; purge di luar owned root ditolak sekalipun ada symlink. | REQ-052 |
| AC-053 | Given orphan PG data ditemukan; when scan dilakukan; then hanya metadata ditemukan; adoption tidak berjalan sampai kompatibilitas/ownership diverifikasi dan pengguna menyetujui. | REQ-053 |

### IPC & integrity (AC-054 cancellation)

| ID | Skenario dan hasil yang harus terlihat | Trace |
| --- | --- | --- |
| AC-054 | Given cancel saat restore staging; when diterima; then target asli tidak berubah; bila commit sudah dimulai, status menjelaskan tindakan recovery dan tidak melaporkan sukses palsu. | REQ-054 |

## 30. Acceptance criteria lintas fitur dan adversarial

| ID | Kasus | Given / When / Then | Trace |
| --- | --- | --- | --- |
| AC-055 | Endpoint race | Given port lolos preflight lalu direbut proses lain; when child bind gagal; then operasi gagal dengan endpoint conflict, tidak mengubah fixed port diam-diam, dan tidak menghentikan proses luar. | REQ-010, VAL-004, BR-005 |
| AC-056 | IPv4/IPv6 dan wildcard | Given listener wildcard/dual-stack yang overlap dengan endpoint rencana; when validate/start; then overlap terdeteksi atau bind failure ditangani jelas; tuple port tidak diperlakukan hanya sebagai integer global. | REQ-010, BR-004, VAL-006 |
| AC-057 | Symlink dan path escape | Given purge target atau archive mengandung symlink/traversal menuju file di luar scope; when purge/extract; then operasi ditolak dan fixture file luar tetap identik, termasuk race yang dapat direproduksi. | REQ-004, REQ-052, VAL-007, VAL-011 |
| AC-058 | Path dengan spasi dan Unicode | Given home/working/data path mengandung spasi serta Unicode; when init/start/log/backup/restore; then path diproses sebagai argumen utuh, tidak memicu shell parsing atau salah target. | REQ-005, REQ-031, VAL-007 |
| AC-059 | Checksum mismatch | Given artifact digest tidak cocok metadata trusted; when install; then binary tidak pernah dijalankan, runtime tidak berstatus installed, dan file staged masuk cleanup aman atau quarantine lokal. | REQ-004, REQ-032, VAL-010 |
| AC-060 | Artifact sangat besar atau archive bomb | Given metadata size melampaui batas atau rasio ekstraksi melebihi policy; when unduh/extract; then operasi berhenti terkontrol, disk/memori tidak tumbuh tanpa batas, dan registry tetap konsisten. | REQ-004, VAL-011, NFR-004 |
| AC-061 | Isolasi yang dijelaskan dengan benar | Given onboarding, docs provider, dan permission dialog; when direview; then semuanya menjelaskan logical/process isolation dan tidak menyatakan native provider disandbox atau mustahil membaca file user yang sama. | REQ-009, REQ-032, BR-012, BR-020 |

| ID | Kasus | Given / When / Then | Trace |
| --- | --- | --- | --- |
| AC-062 | Secret store terkunci | Given Keychain tidak dapat diakses saat autostart; when service membutuhkan secret; then status blocked/needs-secret terlihat dan tidak fallback ke auth kosong, trust, atau password hardcode. | REQ-014, REQ-016, VAL-018 |
| AC-063 | Cold backup tidak boleh melompati stop | Given instance gagal berhenti bersih; when backup cold dijalankan; then copy tidak dipublikasikan sebagai consistent backup dan pengguna menerima alasan serta pilihan aman. | REQ-011, REQ-041, BR-022 |
| AC-064 | Disk full saat backup/restore | Given free space habis di staging; when backup/restore gagal; then instance sumber tetap utuh, target belum diklaim ready, operation needs-recovery/failed akurat dan cleanup tidak menghapus sumber. | REQ-041, REQ-042, REQ-054, NFR-008 |
| AC-065 | Migrasi registry gagal | Given schema migration manager gagal di tengah jalan; when daemon restart; then aplikasi masuk safe recovery state, tidak init ulang registry/data diam-diam, dan backup/recovery evidence tersedia. | REQ-040, REQ-044, NFR-005 |
| AC-066 | Reinstall dengan retained data | Given app dihapus dengan data dipertahankan; when versi compatible dipasang kembali; then retained registry/data dapat direkonsiliasi atau diadopsi tanpa initdb dan nama instance tidak memicu overwrite. | REQ-049, REQ-052, REQ-053 |
| AC-067 | PID reused | Given PID dalam registry kini milik executable lain; when stop/reconcile/doctor; then proses tersebut tidak dibunuh dan instance lama dilaporkan unknown/missing dengan evidence identitas. | REQ-011, REQ-013, REQ-019 |
| AC-068 | Restart budget | Given provider terus crash pada startup; when retry budget habis; then restart berhenti, tidak ada busy loop atau child process ganda, dan GUI/CLI menampilkan backoff serta last error. | REQ-013, NFR-003, NFR-007 |

| ID | Kasus | Given / When / Then | Trace |
| --- | --- | --- | --- |
| AC-069 | Autostart dan stop manual | Given autostart instance aktif; when pengguna stop selama sesi login yang sama; then reconciler tidak menyalakannya kembali segera; pada login berikutnya policy startup berlaku sesuai setting yang ditampilkan. | REQ-014, REQ-022, BR-024 |
| AC-070 | IPC endpoint palsu dan remote access | Given endpoint palsu/wrong owner atau koneksi dari scope tidak berhak; when client mencoba mengirim request; then verifikasi identity/authorization gagal sebelum credential atau mutation dikirim; tidak ada TCP admin listener baseline. | REQ-016, REQ-047, VAL-017 |
| AC-071 | Protocol/event compatibility | Given client minor lebih lama, optional field baru, dan event cursor kadaluarsa; when reconnect; then compatible payload tetap diproses dan gap memicu snapshot refresh; major incompatible ditolak. | REQ-047, REQ-050 |
| AC-072 | Provider revision dan revocation | Given revision installed lama serta revision baru revoked/berubah permission; when sync catalog; then data/runtime existing tidak dihapus, install baru diblokir sesuai policy, dan status trust dapat ditelusuri. | REQ-003, REQ-032, BR-017 |
| AC-073 | Environment concurrent ownership | Given env A up dan env B down bersamaan pada shared instance; when operasi diserialisasi; then hasil lease konsisten, instance tidak dihentikan saat masih diperlukan, dan tidak ada create duplicate. | REQ-021, REQ-022, REQ-048 |
| AC-074 | No fake Windows parity | Given catalog/UI Windows pada build berikutnya; when provider hanya mempunyai macOS atau WSL artifact; then native support tidak ditandai true dan tidak ada silent Docker/WSL install/substitution. Pada baseline macOS, schema/fixture policy ini diuji tanpa mengklaim GUI Windows selesai. | REQ-002, REQ-037, REQ-050 |
| AC-075 | Notarization/signing failure | Given signed distribution pipeline gagal verifikasi/notarization; when release gate dievaluasi; then artefak tidak dipublikasikan sebagai stable supported dan workaround mematikan proteksi OS tidak dimasukkan ke installer. | REQ-049, REQ-051, NFR-009 |

| ID | Kasus | Given / When / Then | Trace |
| --- | --- | --- | --- |
| AC-076 | Performance dan memory bound | Given hardware baseline tercatat dan 100 registered/10 running idle instances; when benchmark metadata/UI/daemon/log dilakukan; then NFR yang ditetapkan diukur, hasil disimpan, dan outlier/failure tidak disembunyikan. | NFR-001, NFR-002, NFR-003, NFR-004, NFR-006, NFR-007 |
| AC-077 | Durability policy bukan janji universal | Given Valkey volatile, Valkey persistent dan NATS JetStream fixtures; when normal restart/crash/power-loss simulation yang tersedia diuji; then hasil dibedakan berdasarkan policy/durability, tidak menjanjikan semua unflushed writes selalu bertahan. | REQ-036, REQ-038, REQ-041, NFR-008 |
| AC-078 | Third-party package compliance | Given enam provider wajib akan dirilis; when provenance/license/dependency/signing inventory direview; then setiap artefak mempunyai sumber dan keputusan distribusi yang tercatat; paket dengan izin/sumber tak jelas tetap BLOCKED. | REQ-004, REQ-032, REQ-049 |
| AC-079 | Dua user pada satu Mac | Given dua user dengan registry/socket terpisah; when user B mencoba IPC user A atau memakai port yang sama; then IPC terlarang ditolak dan port conflict dilaporkan tanpa mengambil alih data/proses user A. | REQ-010, REQ-047, BR-012 |
| AC-080 | Environment non-destructive plan dan export | Given manifest berisi local overrides dan secret references; when plan/export dijalankan; then plan tidak menjalankan side effect, export tidak memuat secrets/home path/PID, dan lockfile mempertahankan exact artifacts. | REQ-020, REQ-023, REQ-046 |

## 31. Traceability awal

Tabel ini bukan status implementasi. Agent wajib menambahkan path kode, schema/API, UI, test nyata, command dan evidence setelah pekerjaan dilakukan. Semua baris awal **NOT_RUN**.

| Requirement | Komponen utama | Acceptance criterion utama | Status |
| --- | --- | --- | --- |
| REQ-001 | domain, daemon, clients | AC-001 | NOT_RUN |
| REQ-002 | provider-engine, catalog, GUI | AC-002 | NOT_RUN |
| REQ-003 | runtime, lockfile | AC-003 | NOT_RUN |
| REQ-004 | runtime, operations, security | AC-004 | NOT_RUN |
| REQ-005 | runtime, platform | AC-005 | NOT_RUN |
| REQ-006 | runtime, external-adapter | AC-006 | NOT_RUN |
| REQ-007 | runtime, storage | AC-007 | NOT_RUN |
| REQ-008 | instance, storage, operations | AC-008 | NOT_RUN |
| REQ-009 | instance, runtime, supervisor | AC-009 | NOT_RUN |
| REQ-010 | port-manager, provider-engine | AC-010 | NOT_RUN |
| REQ-011 | supervisor, platform | AC-011 | NOT_RUN |
| REQ-012 | health, provider-engine | AC-012 | NOT_RUN |
| REQ-013 | supervisor, operations, platform | AC-013 | NOT_RUN |
| REQ-014 | platform-macos, preferences | AC-014 | NOT_RUN |
| REQ-015 | config, provider-engine, GUI | AC-015 | NOT_RUN |
| REQ-016 | security, platform-secrets | AC-016 | NOT_RUN |
| REQ-017 | logging, IPC, GUI | AC-017 | NOT_RUN |
| REQ-018 | metrics, platform, GUI | AC-018 | NOT_RUN |
| REQ-019 | diagnostics, CLI, GUI | AC-019 | NOT_RUN |
| REQ-020 | environment, validation | AC-020 | NOT_RUN |
| REQ-021 | environment, operations | AC-021 | NOT_RUN |
| REQ-022 | environment, leases, supervisor | AC-022 | NOT_RUN |
| REQ-023 | environment, lockfile | AC-023 | NOT_RUN |
| REQ-024 | apps/macos, client-swift | AC-024 | NOT_RUN |
| REQ-025 | apps/macos, schemas | AC-025 | NOT_RUN |
| REQ-026 | apps/macos | AC-026 | NOT_RUN |
| REQ-027 | **Interaksi desktop** | Copy endpoint, menu bar status dasar, dan konfirmasi destructive action. Notifikasi opsional dan permission-aware. |
| REQ-028 | apps/cli, IPC | AC-028 | NOT_RUN |
| REQ-029 | provider-engine, schemas | AC-029 | NOT_RUN |
| REQ-030 | provider-sdk, schema-tools | AC-030 | NOT_RUN |
| REQ-031 | provider-engine, security | AC-031 | NOT_RUN |
| REQ-032 | security, provider-registry | AC-032 | NOT_RUN |
| REQ-033 | provider-registry, catalog | AC-033 | NOT_RUN |
| REQ-034 | docs, provider-sdk, CI | AC-034 | NOT_RUN |
| REQ-035 | providers/postgresql | AC-035 | NOT_RUN |
| REQ-036 | providers/nats | AC-036 | NOT_RUN |
| REQ-037 | providers/clickhouse | AC-037 | NOT_RUN |
| REQ-038 | providers/valkey | AC-038 | NOT_RUN |
| REQ-039 | providers/traefik, routing | AC-039 | NOT_RUN |
| REQ-040 | storage, backup | AC-040 | NOT_RUN |
| REQ-041 | backup, provider-engine | AC-041 | NOT_RUN |
| REQ-042 | backup, instance, operations | AC-042 | NOT_RUN |
| REQ-043 | runtime, migration-policy | AC-043 | NOT_RUN |
| REQ-044 | updater, storage, packaging | AC-044 | NOT_RUN |
| REQ-045 | audit, storage | AC-045 | NOT_RUN |
| REQ-046 | core, catalog, preferences | AC-046 | NOT_RUN |
| REQ-047 | protocol, IPC, platform | AC-047 | NOT_RUN |
| REQ-048 | operations, storage | AC-048 | NOT_RUN |
| REQ-049 | packaging, platform-macos | AC-049 | NOT_RUN |
| REQ-050 | platform, protocol, domain | AC-050 | NOT_RUN |
| REQ-051 | tests, CI, release | AC-051 | NOT_RUN |
| REQ-052 | instance, packaging, storage | AC-052 | NOT_RUN |
| REQ-053 | instance, diagnostics | AC-053 | NOT_RUN |
| REQ-054 | operations, IPC, GUI | AC-054 | NOT_RUN |
| REQ-055 | providers/mysql | AC-081 | NOT_RUN |

Additional AC pada bagian sebelumnya melengkapi negative cases, BR/VAL, dan NFR. Kebutuhan Windows aktual diverifikasi ulang pada release gate Windows; keberhasilan fixture baseline tidak mengubahnya menjadi implemented.

## 32. Strategi pengujian dan compatibility matrix

### 32.1 Lapisan test

| Jenis test | Cakupan | Kapan |
| --- | --- | --- |
| Unit | Invariants, version selectors, endpoint overlap, path policy, lifecycle, leases, redaction, config validation | Setiap PR |
| Contract | Provider schema, IPC fixtures, Swift/Rust DTO, compatibility/deprecation, fake provider | Setiap PR |
| Integration | SQLite transactions, actual runtime install/create/start/stop, credential paths, log/health, shared environments | PR terkait dan nightly |
| E2E GUI/CLI | User journeys, parity, loading/error/reconnect, accessibility, destructive confirmation | Sebelum beta dan candidate release |
| Security/fuzz | Archive/path/parser/IPC abuse, unsafe argv, trust changes, local authorization | CI selektif dan security review |
| Fault injection | Crash daemon/provider, PID reuse, port race, disk full, download corruption, cancelled operation | Nightly/candidate |
| Recovery | Backup/restore-as-new, retained data, schema migration failure, provider version mismatch | Mandatory release gate |
| Distribution | Fresh user, signed app/helper, notarization, background approval, PATH/CLI, upgrade/uninstall | Tiap OS/arch yang diiklankan |
| Performance/soak | Metadata latency, UI snapshot, CPU/RSS, log backpressure, repeated lifecycle | Beta/candidate pada hardware tercatat |

Test harus membedakan **process crash**, **OS restart**, dan **power loss simulation**. Jangan mengklaim ketahanan terhadap kehilangan daya hanya karena `kill` dan restart normal berhasil. Jangan menonaktifkan durability setting untuk mempercepat demo yang kemudian dipakai sebagai bukti produksi.

### 32.2 Matrix minimum baseline

| Dimensi | Minimum |
| --- | --- |
| macOS/arch | macOS 27 (minimum) pada Apple Silicon dan satu OS terbaru yang dipilih untuk rilis; Intel hanya jika didukung secara eksplisit |
| Windows/arch | Windows 11 versi 26H2 (minimum); setiap arsitektur yang diiklankan harus masuk matrix pengujian |
| PostgreSQL | Satu patch terpilih di mayor 17 dan satu di mayor 18; dua instance mayor 17 memakai runtime ID sama |
| MySQL | Satu versi konkret; dua instance dengan data directory terpisah; isolation, readiness, shutdown, backup/restore-as-new |
| NATS | Satu versi konkret; dua instance; JetStream on dan off; persistence fixture ketika on |
| ClickHouse | Satu versi konkret; dua instance; HTTP + native TCP serta audit listener lain |
| Valkey | Satu versi konkret; dua instance; mode persistent dan volatile dengan expected result berbeda |
| Traefik | Satu versi konkret; dua instance; routing update, invalid config, HTTP, health; HTTPS ketika capability diuji |
| Source | Managed install end-to-end; external register/drift/missing; corrupted artifact |
| Filesystem | Path space/Unicode, symlink negative tests, missing path, full disk simulation; local filesystem baseline |
| Lifecycle | Start/stop/restart, Quit GUI, restart daemon, login/logout, wake, crash-loop, permission denied |
| Recovery | Metadata snapshot + setiap provider persistent yang diklaim supported dapat direstore sesuai scope backup |

Compatibility record wajib mencatat source URL, artifact digest, provider revision, dependency identity, OS build/arch, feature subset, test results, known limitations dan tanggal. Versi yang tidak ada pada matrix tetap `UNVERIFIED`, meskipun nomor versinya tampak dekat.

### 32.3 Test fixture wajib

Fixture minimum meliputi dummy server TCP/HTTP yang dapat delay-ready/crash/log-spam; fake provider declarative; tiga cluster PostgreSQL dengan sentinel berbeda; dua server MySQL dengan data directory dan sentinel berbeda; NATS stream/message per-instance; tabel ClickHouse bernama sama dengan isi berbeda; Valkey key/value dan persistence policy; backend HTTP uji Traefik; malformed archives; secrets sentinel; protocol mismatch; operation race; serta registry versi sebelumnya.

Cleanup test wajib memeriksa test-run ownership dan tidak melakukan recursive delete pada path yang dipasok tanpa validasi. Integration tests tidak boleh memakai database bisnis pengguna sebagai fixture.

## 33. Rencana pengembangan dan timeline

### 33.1 Asumsi estimasi

**Baseline perencanaan: sekitar 24 minggu, dengan rentang awal 24–32 minggu sebelum stabil internal**, tergantung feasibility packaging, security fixes, dan hasil pilot. Ini adalah estimasi proyek, bukan tanggal penyelesaian yang dijamin.

Asumsi tim: satu engineer Rust/backend berpengalaman, satu engineer SwiftUI yang bekerja paralel setelah kontrak cukup stabil, QA paruh waktu, review security/release berkala, serta akses Mac/runner, signing account, dan artefak provider. Coding agent merupakan alat bantu, bukan pengganti review keamanan, pengujian native, atau kepemilikan keputusan.

`T0` adalah tanggal kickoff yang belum ditetapkan. Durasi kalender harus dihitung ulang setelah compatibility spike. Bila dikerjakan satu orang/paruh waktu, jangan menggunakan angka ini tanpa revisi kapasitas. Scope katalog tambahan tidak ikut dimasukkan ke estimasi baseline.

### 33.2 Milestone sampai stabil macOS

| Milestone | Minggu relatif | Fokus | Exit gate dan bukti |
| --- | --- | --- | --- |
| M0 — Discovery & feasibility | 1–2 | Audit repository, ADR, threat model, runtime sources, signing/background prototype, matrix awal enam provider | Satu executable tiap provider berjalan pada Mac uji; strategi managed packaging PG, MySQL, Valkey/dependency dan distribusi jelas; blocker tercatat |
| M1 — Core, daemon, protocol | 3–5 | Workspace, domain, storage/migrations, operations, IPC, CLI, fake platform/provider | GUI/client fixture dan CLI dapat mengelola dummy service; auth/contract/reconciliation dasar lulus |
| M2 — Runtime & instances | 6–8 | Verified installer, multi-version, data/port/config isolation, shutdown, provider engine | PG17 + PG18 + dua instance runtime identik lulus isolation/persistence tests dari CLI |
| M3 — Enam provider wajib | 9–12 | MySQL, NATS, ClickHouse, Valkey, Traefik, PG hardening; provider capability/tests | Semua versi terpilih pada matrix lulus lifecycle/readiness/persistence/endpoint tests; tidak ada false supported |
| M4 — Native SwiftUI | 9–16, paralel M3/M5 | App shell, catalog/runtime/instance, config/log/metrics, operations, accessibility, menu bar | Critical user journeys dan CLI/GUI parity lulus; close GUI tidak menghentikan service |
| M5 — Environment & recovery | 13–17 | Manifest/lockfile, graph/leases, shared ownership, metadata/data backup, restore-as-new | ERP environment six-service bekerja; partial failure/shared-down/restore drill lulus |
| M6 — Hardening & release candidate | 18–21 | Security/fuzz, crash/disk/race tests, packaging/signing, update/migrations, docs, provider SDK | Mandatory RC gates lulus; tidak ada critical/high security atau data-loss blocker |
| M7 — Internal pilot & stable | 22–24 | Dogfooding, support runbooks, UAT, bug fixes, clean-install/reinstall/upgrade verification | Minimal pilot yang disetujui lulus; requirement/evidence matrix dan sign-off lengkap |
| Contingency | Tambahan 0–8 | Perbaikan packaging, platform, dependency, regression dan pilot issues | Perpanjangan berbasis blocker nyata; bukan pemotongan requirement keamanan/data |

Keputusan arsitektur provider tidak ditunda sampai kelima service selesai. NATS/Traefik digunakan lebih awal untuk membuktikan engine tidak hanya cocok bagi PostgreSQL. GUI boleh dimulai dari contract fixtures agar implementasi dapat paralel, tetapi fixture success tidak menggantikan native integration test.

### 33.3 Setelah macOS stabil

| Tahap | Fokus | Syarat mulai |
| --- | --- | --- |
| Community beta | Publish SDK/docs, source contribution, issue triage, reviewed catalog workflow | Provider contract cukup stabil dan local-provider tests tersedia |
| Public registry | Discovery, publishing, trust roots, revocation, compatibility reports | Governance/security ownership dan distribusi runtime disepakati |
| Windows foundation | IPC named pipe, process/secret/path/startup adapters, runner dan packaging spike | Core/protocol stabil serta matrix native Windows jelas |
| WinUI 3 beta | C# client, native UI, lifecycle/recovery parity untuk provider yang tersedia | Windows daemon dan set provider minimum lulus test nyata |
| Windows stable | Distribution/security/data recovery gates Windows | Tidak hanya UI selesai; seluruh matrix supported lulus |
| Linux/advanced plugins | Platform distribution, CLI/headless, WASM/custom capabilities | Ditetapkan sebagai scope terpisah setelah kebutuhan terbukti |

Usulan effort awal untuk **Windows beta** setelah foundation macOS: 12–20 minggu dengan tim yang sesuai, **tidak menjanjikan kesetaraan semua provider**. Estimasi Windows stable ditetapkan setelah feasibility/compatibility gate Windows, bukan dipatok sekarang pada bulan tertentu.

## 34. Release gates dan sign-off

| Gate | Syarat wajib | Pemilik sign-off |
| --- | --- | --- |
| G0 — Feasibility | Sumber binary/dependency, artifact identity, OS matrix, background behavior, dan distribution policy enam provider jelas | Tech lead + release owner |
| G1 — Functional | Seluruh REQ baseline dan AC utama lulus; GUI/CLI parity serta provider extensibility terbukti | Product owner + engineering |
| G2 — Isolation/data safety | Multi-version/same-version, no path overlap, safe purge, external ownership, shared leases, concurrency lulus | Core maintainer + QA |
| G3 — Recovery | Backup metadata/data, restore-as-new, restart/crash, migration rollback/forward, retained-data recovery lulus | QA + data safety reviewer |
| G4 — Security | IPC/trust/execution/archive/path/secrets/update review; no critical/high unresolved findings | Security reviewer |
| G5 — Distribution | Signed/notarized release, clean-install, permission denial, update/uninstall, supported matrix dan notices lengkap | Release owner |
| G6 — Operasional | Runbooks, diagnostic export, retention, known limitations, support ownership, incident process tersedia | Maintainer internal |
| G7 — Pilot | Usulan minimal 3 pengguna internal selama 10 hari kerja; issue triage, lifecycle/recovery feedback tercatat | Product owner + pilot owner |

Pilot size/durasi adalah usulan yang dapat disesuaikan sebelum M7, bukan data penggunaan aktual. Gate wajib tidak otomatis dianggap lulus karena timeline berakhir. Semua mandatory blockers harus diselesaikan atau scope diubah secara eksplisit dengan dampak dan persetujuan tertulis.

## 35. Rollout, operasional, dan runbooks

### 35.1 Jalur rilis

```text
Developer build
→ Internal alpha (fixture + native provider tests)
→ Internal beta (data development yang dapat dipulihkan)
→ Release candidate (frozen matrix dan signed distribution)
→ Internal stable
→ Public beta/stable setelah governance siap
```

Rilis pertama tidak langsung dipakai untuk satu-satunya salinan data penting. Pengguna pilot menjalankan backup teruji dan mencatat versi sebelumnya. Kanal alpha/beta/stable tidak mengubah data provider secara otomatis.

### 35.2 Default operasional usulan

| Area | Default usulan | Catatan |
| --- | --- | --- |
| Log rotation | 5 file × 20 MiB per stream/instance | Disesuaikan melalui setting; truncation terlihat |
| Audit retention | 30 hari atau batas ukuran yang disetujui | Aksi penting dapat diekspor; bukan immutable audit |
| Metrics | In-memory bounded history; sampling default 5 detik | Disk usage scan lebih jarang/on-demand |
| Restart retry | Maks. 5 percobaan dalam 5 menit | Backoff+jitter; provider-specific recovery timeout |
| Readiness | Default deadline 60 detik | Configurable; timeout bukan bukti proses telah mati |
| Backup retention | Tidak ada auto-delete default | Retention terjadwal/incremental di luar baseline |
| Artifact cache | GC hanya cache tidak direferensikan | Tidak menghapus runtime/dependency aktif |
| Low disk | Warning + preflight berdasarkan ukuran operasi | Jangan mematikan database sembarang hanya karena threshold |

Nilai final ditetapkan setelah benchmark dan pilot. Tidak ada background telemetry yang diperlukan untuk menjalankan defaults tersebut.

### 35.3 Runbook minimum

| Kondisi | Langkah aman yang harus didokumentasikan |
| --- | --- |
| Daemon tidak terhubung | Periksa background permission, user scope, endpoint ownership, protocol, helper signature; tidak otomatis delete registry |
| Port conflict | Identifikasi endpoint dan proses yang menggunakan; pilih port lain secara eksplisit atau kelola proses luar sendiri |
| Runtime missing/drifted | Verify source/digest, install ulang artefak yang tepat; jangan auto-rebind ke `latest` |
| Database start gagal | Inspect logs, version/data compatibility, disk/permission/lock; jangan reset data atau hapus PID tanpa bukti |
| Disk penuh | Hentikan download/backup baru, bersihkan cache/log yang aman, tentukan data growth; jangan purge data otomatis |
| Crash loop | Stop retry, tampilkan last failure, periksa config/runtime/resource, lakukan recovery terkontrol |
| Backup gagal | Pertahankan sumber, tandai backup incomplete, cleanup staging milik operation, jangan klaim recoverable |
| Restore gagal | Pertahankan sumber/backup, inspect compatibility, discard staging hanya setelah ownership verified |
| Update gagal | Periksa app/protocol/registry compatibility; rollback hanya menurut runbook yang sudah diuji |
| Provider revoked | Stop instalasi baru, tampilkan informasi risiko; jangan menghapus data atau menghentikan service tanpa kebijakan eksplisit |
| Uninstall/reinstall | Stop terkontrol, unregister background helper, retain data/metadata, reinstall compatible dan reconcile |

## 36. Definition of Done

MacOS baseline dinyatakan selesai hanya bila pengguna dapat memasang signed distribution pada mesin supported, menginstal runtime tanpa toolchain developer untuk paket yang diiklankan managed, membuat beberapa versi dan beberapa instance dari versi identik, mengelola enam provider melalui GUI/CLI, dan menjalankan environment tanpa membutuhkan cloud account atau Docker.

Seluruh data/config/endpoints/identitas instance terpisah; menutup GUI tidak menghentikan service. Restart daemon, logout/login, dan restart OS mempunyai perilaku yang diuji serta batas disruption yang terdokumentasi. Backup yang diklaim recoverable telah melalui restore drill. Unsupported capability dan platform gap ditampilkan dengan jujur.

Provider deklaratif contoh dapat ditambahkan tanpa perubahan core/GUI untuk kemampuan yang sudah ada. SDK, schema, docs, contributor workflow, trust/security policy, CI, release checklist, compatibility matrix, operational runbooks dan third-party inventory tersedia.

Semua mandatory AC berstatus PASS dengan evidence nyata, tidak ada unresolved critical/high security atau data-loss issue, clean-install/update/uninstall lulus, dan sign-off release gates lengkap. PRD, ADR, implementation plan dan traceability sesuai source commit yang dirilis. Windows tetap berstatus roadmap sampai milestone dan pengujiannya sendiri diselesaikan.

---

<a id="lampiran-keputusan"></a>

# LAMPIRAN — KEPUTUSAN, RISIKO, DAN HANDOFF

## 37. Risiko utama dan mitigasi

| ID | Risiko | Dampak | Mitigasi / trigger |
| --- | --- | --- | --- |
| RISK-01 | Native macOS runtime tidak relocatable atau bergantung Homebrew global | Multi-version/reproducibility dan clean-install gagal | Prototype packaging pada M0; dependency closure dan managed artifact pipeline |
| RISK-02 | Trust provider disalahartikan sebagai sandbox | Pengguna menjalankan executable berbahaya dengan hak user | Explicit native-code warning, review, source/digest control; tidak mengklaim security isolation |
| RISK-03 | Dua supervisor atau race start | Dua proses menyasar data yang sama | Satu mutation owner, journal/locks, ownership identity, reconciliation |
| RISK-04 | Data version incompatible | Data corrupt/tidak dapat dibuka | Pinning, preflight, major-upgrade block, verified backup/restore |
| RISK-05 | Platform parity diasumsikan | Windows scope membesar atau provider tidak tersedia | Capability matrix dan native/WSL distinction sejak awal |
| RISK-06 | Disk/log/backup tidak dibatasi | Host kehabisan ruang, database error | Retention/buffer caps, space preflight, non-destructive cleanup |
| RISK-07 | Provider schema terlalu sempit | Setiap tool tetap membutuhkan patch core | Uji PostgreSQL, NATS, Traefik dan fixture provider sejak foundation |
| RISK-08 | Provider schema terlalu powerful | Arbitrary code execution melalui manifest | Safe primitive set, no default shell/hooks, extensibility melalui review |
| RISK-09 | GUI dan daemon protocol drift | State salah atau operasi gagal setelah update | Versioned protocol, shared fixtures, handshake dan release compatibility |
| RISK-10 | Packaging/signing terlambat ditemukan | Rilis tidak dapat dipakai di mesin bersih | Spike pada M0 dan signed clean-install sebelum beta |
| RISK-11 | Scope katalog tak terkendali | Baseline tidak selesai | Enam provider wajib; tambahan tetap backlog dengan owner/prioritas |
| RISK-12 | Lisensi/sumber artefak tidak jelas | Distribusi tidak dapat disetujui | Inventory per versi, source/redistribution review sebelum publishing |
| RISK-13 | Backup ada tetapi tidak bisa direstore | Kehilangan kemampuan recovery | Mandatory restore drill, compatible runtime inventory, missing-secret warning |
| RISK-14 | Status berasal dari PID/cache lama | Wrong kill atau false ready | Compound identity, fresh probe, unknown state dan safe fallback |

## 38. Open questions dan default untuk melanjutkan

OQ bukan alasan menghentikan seluruh pekerjaan. Gunakan default usulan untuk prototype yang aman, simpan keputusan/evidence, dan selesaikan blocker sebelum gate terkait.

| ID | Keputusan yang diperlukan | Default sementara | Gate / owner |
| --- | --- | --- | --- |
| OQ-001 | CLI, bundle ID, namespace, license project, pemeriksaan legal merek | Nama final produk: Stackly; identifier teknis, lisensi, dan pemeriksaan legal masih perlu dituntaskan | Sebelum public release / product owner |
| OQ-002 | Dukungan Intel macOS dan arsitektur Windows | Minimum OS: macOS 27 dan Windows 11 versi 26H2; Apple Silicon/ARM64 wajib untuk macOS; Intel hanya setelah matrix lulus; arsitektur Windows dikunci sebelum fase Windows | Gate platform terkait / platform lead |
| OQ-003 | Sumber paket managed enam provider dan dependency closure | Sumber resmi/terpercaya atau maintainer-built dengan provenance; sumber dan ketentuan distribusi tiap provider diverifikasi; external hanya mode tambahan | G0 / runtime maintainer |
| OQ-004 | Exact versions dan artifact matrix enam provider | PG17/18 patch terpilih; lima provider lain satu stable concrete version masing-masing | Sebelum beta / provider owners |
| OQ-005 | Encoding IPC final dan generation tooling | JSON schema + length-prefix + Unix socket; Windows named pipe | M1 / tech lead |
| OQ-006 | Mechanism helper registration dan bundling final | Per-user background helper, Service Management sesuai deployment target | M0/M1 / macOS lead |
| OQ-007 | Policy restart daemon dan child handover | Controlled disruption pada restart; reconcile surviving processes; tidak menjanjikan zero downtime | M1/M2 / supervisor owner |
| OQ-008 | Keychain access policy dan secret material di native config | Store reference; private file hanya bila perlu; no default export/reveal | G4 / security owner |
| OQ-009 | Provider trust root, signing, revocation freshness | Bundled/official + explicit local import; no arbitrary script | Sebelum public catalog / security owner |
| OQ-010 | Final backup scope per-provider dan perlindungan backup sensitif | PostgreSQL logical; validated cold backup untuk provider persistent lain; restore-as-new | G3 / data safety owner |
| OQ-011 | Hardware baseline dan angka NFR | Mac ARM64 16 GB, 10 idle/light instances, targets bagian 14 | Sebelum beta / QA |
| OQ-012 | Signing account, distribution identity dan registry hosting | Direct signed distribution; hosting dapat ditunda untuk internal | G5 / release owner |
| OQ-013 | Lisensi open-source core dan kontribusi | MIT/Apache-2.0 dapat ditinjau; keputusan bukan otomatis berlaku pada binary pihak ketiga | Sebelum publikasi source/artefak / owner |
| OQ-014 | Model Windows per-user versus system service | Per-user dulu; SCM mode terpisah bila dibutuhkan | Windows feasibility / Windows lead |
| OQ-015 | Windows native provider set dan WSL policy | Tidak memasang WSL/container otomatis; unsupported terlihat | Windows G0 / provider owners |
| OQ-016 | T0, kapasitas tim dan pilot participants | Timeline relatif 24–32 minggu; minimal pilot usulan bagian 34 | Kickoff / product owner |
| OQ-017 | Kebutuhan LAN binding/privileged ports/TLS trust | Loopback dan high ports; cert diberikan eksplisit; privileged helper di luar baseline | Scope change / security + product owner |
| OQ-018 | Dukungan custom data volume/external storage | Local managed filesystem dulu; external adoption dengan preflight | Sebelum capability diaktifkan / storage owner |

## 39. Open source dan governance komunitas

Core, daemon, CLI, GUI, official providers, protocol dan dokumentasi dikelola dalam monorepo awal. Registry/community catalog dapat dipisahkan setelah kontrak stabil; jangan membuat layanan cloud wajib hanya agar contributor bisa menguji provider.

Minimal governance: core maintainer, platform maintainer, provider owner, security contact, release owner, CODEOWNERS bila dipakai, issue/PR templates, Code of Conduct, contribution guide dan kebijakan deprecation. Istilah official/reviewed berarti review sesuai proses yang tercatat, bukan bebas risiko selamanya.

Alur provider contribution:

```text
Scaffold → schema validation → local fixture tests
→ native compatibility evidence → source/license/security review
→ maintainer review → merge immutable revision
→ publish catalog metadata → feedback dan maintenance
```

Contributor tidak harus menulis Rust untuk primitive existing. Perubahan core atau primitive execution memerlukan review tambahan. Registry publication dan signing hanya dilakukan trusted pipeline, bukan script dari PR contributor yang dijalankan dengan release credentials.

Pilih lisensi source core secara sadar, catat lisensi dependensi dan artefak provider per-versi, serta dokumentasikan apakah binary diunduh dari upstream atau didistribusikan ulang. Lisensi Stackly tidak otomatis menentukan hak atas seluruh software yang dapat dikelolanya. Keputusan legal/distribusi perlu peninjauan pihak yang berwenang; PRD bukan pendapat hukum.

Security disclosure harus mempunyai kanal privat dan proses advisory sebelum detail sensitif dipublikasikan. Respons community issue membedakan bug core, bug provider, incompatibility upstream, dan configuration issue sehingga tanggung jawab jelas.

## 40. Aturan handoff untuk agent berikutnya

PRD ini hanya menjadi bahan awal bagi agent atau pemilik produk yang menyusun scope dan specs. Setelah scope dan specs tersedia, agent implementasi mengikuti dokumen-dokumen tersebut dan `AGENTS.md`, bukan instruksi atau rincian requirement dalam PRD ini. Jika task belum memiliki scope/spec yang diperlukan, agent harus menggunakan workflow scope/spec untuk membuat atau merekonsiliasinya sebelum implementasi; jangan mengambil detail dari PRD ini sebagai pengganti.

---

<a id="referensi"></a>

# Referensi primer dan batas verifikasi

Dokumentasi berikut diperiksa untuk penyusunan PRD pada **15 Oktober 2026**. Referensi mendukung kemampuan upstream tertentu; **bukan bukti bahwa integrasi Stackly, paket runtime, permission, atau installer telah diuji pada perangkat nyata**. Dokumen yang menggunakan jalur `latest` dapat berubah; saat implementasi, pin referensi dan artifact version yang benar-benar dipakai.

| ID | Sumber primer | Digunakan untuk |
| --- | --- | --- |
| S01 | [PostgreSQL 18 — postgres][S01] | Beberapa server pada host dengan data area dan communication ports berbeda |
| S02 | [Apple — SMAppService][S02] | Background helper, LoginItems, LaunchAgents/LaunchDaemons dan kebutuhan OS |
| S03 | [Microsoft — WinUI 3][S03] | Native Windows UI, C#/C++ dan Windows App SDK |
| S04 | [PostgreSQL 18 — pg_ctl][S04] | Lifecycle, shutdown modes, readiness wait dan opsi Windows service |
| S05 | [NATS — Getting Started][S05] | Jalur instalasi macOS dan Windows, server/CLI distinction |
| S06 | [ClickHouse — Install with the ClickHouse CLI][S06] | Native macOS/Linux, local version/server management, Windows melalui WSL |
| S07 | [Valkey — Installation][S07] | Jalur macOS/Homebrew dan keterangan dukungan Windows/WSL |
| S08 | [PostgreSQL 18 — Creating a Database Cluster][S08] | Cluster/data directory dan inisialisasi |
| S09 | [NATS — Monitoring endpoints][S09] | Port monitoring terpisah dan endpoint observability |
| S10 | [PostgreSQL 18 — pg_upgrade][S10] | Major upgrade, compatibility dan batas rollback data |
| S11 | [NATS — Single server][S11] | Client port, JetStream dan store directory |
| S12 | [Traefik — Configuration Introduction][S12] | Startup versus routing configuration |
| S13 | [Traefik — File provider][S13] | Routing dinamis dari YAML/TOML/file directory |
| S14 | [Apple — Notarizing macOS software before distribution][S14] | Signing/notarization dalam workflow distribusi macOS |
| S15 | [SQLite — Backup API][S15] | Backup/snapshot konsisten metadata registry |
| S16 | [Homebrew — Manpage][S16] | Keg, opt symlink, pinning, upgrade/cleanup semantics |
| S17 | [Microsoft — Named Pipe Security and Access Rights][S17] | Security descriptor dan akses client pada IPC Windows |
| S18 | [Microsoft — Job Objects][S18] | Pengelompokan dan pengelolaan proses Windows |
| S19 | [PostgreSQL 18 — Backup and Restore][S19] | Perbedaan metode backup database |
| S20 | [ClickHouse — Backup and restore overview][S20] | Mekanisme backup/restore upstream yang perlu adapter tersendiri |
| S21 | [Valkey — Persistence][S21] | RDB/AOF, volatile mode dan durability trade-offs |
| S22 | [MySQL 8.4 Reference Manual — Installing MySQL][S22] | Referensi instalasi dan konfigurasi MySQL; belum diverifikasi pada host target |
| S23 | [MySQL 8.4 Reference Manual — Backup and Recovery][S23] | Referensi backup/restore MySQL; belum diverifikasi pada host target |

[S01]: https://www.postgresql.org/docs/18/app-postgres.html
[S02]: https://developer.apple.com/documentation/servicemanagement/smappservice
[S03]: https://learn.microsoft.com/en-us/windows/apps/winui/winui3/
[S04]: https://www.postgresql.org/docs/18/app-pg-ctl.html
[S05]: https://docs.nats.io/concepts/getting-started/
[S06]: https://clickhouse.com/docs/get-started/setup/self-managed/clickhousectl
[S07]: https://valkey.io/topics/installation/
[S08]: https://www.postgresql.org/docs/18/creating-cluster.html
[S09]: https://docs.nats.io/learn/monitoring/monitoring-endpoints
[S10]: https://www.postgresql.org/docs/18/pgupgrade.html
[S11]: https://docs.nats.io/learn/topologies/single-server
[S12]: https://doc.traefik.io/traefik/getting-started/configuration-overview/
[S13]: https://doc.traefik.io/traefik/reference/install-configuration/providers/others/file/
[S14]: https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
[S15]: https://sqlite.org/backup.html
[S16]: https://docs.brew.sh/Manpage
[S17]: https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights
[S18]: https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects
[S19]: https://www.postgresql.org/docs/18/backup.html
[S20]: https://clickhouse.com/docs/concepts/features/backup-restore/overview
[S21]: https://valkey.io/topics/persistence/
[S22]: https://dev.mysql.com/doc/refman/8.4/en/installing.html
[S23]: https://dev.mysql.com/doc/refman/8.4/en/backup-and-recovery.html

---

**Akhir PRD.** Dokumen ini merekam kebutuhan produk awal sebagai bahan penyusunan scope dan specs; bukan sumber instruksi implementasi bagi agent berikutnya. Agent implementasi mengikuti scope dan specs yang berlaku. Dokumen ini tidak menyatakan bahwa source code, runtime packages, provider integration, signing, native compatibility, atau pengujian sudah selesai.
