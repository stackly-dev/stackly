# 0001. Stack dan arsitektur Stackly: catatan keputusan

Dokumen ini adalah catatan alasan untuk [index.md](index.md). `/develop` tidak perlu membacanya saat build.

## Context

Repo ini masih greenfield: isinya baru `prd.md`, scope di `docs/scope/`, dan mockup UI. PRD sudah mengunci Rust untuk core, daemon, dan CLI, SwiftUI untuk macOS, SQLite untuk registry, Unix domain socket untuk IPC, serta macOS 27 di Apple Silicon. Yang masih terbuka: toolchain dan versinya, pembagian crate, encoding IPC dan tooling pembangkit kontrak (OQ-005), library yang dipakai lintas fitur, dan tooling build serta test. Tanpa keputusan ini, fitur Foundation 4 sampai 11 dan jalur SwiftUI (12, 13, 67 sampai 69) masing masing akan menebak strukturnya sendiri, dan walking skeleton (fitur 14) tidak punya tempat berpijak.

Gaya yang membentuk keputusan:

- **Portabilitas yang dibuktikan, bukan dijanjikan.** NFR-010 dan AC-050 mewajibkan domain dan protocol bebas dari framework GUI dan API OS, dengan target check dan test fake platform. REQ-050 meminta core, DTO, dan schema siap Windows, walau Windows baru dikerjakan nanti.
- **Daemon adalah pusat.** REQ-001 dan PRD 15.1 menyatakan GUI dan CLI hanya client, dan hanya daemon yang menulis registry serta menjalankan proses. Tidak ada listener TCP (PRD 18.1).
- **Satu kontrak untuk tiga bahasa.** PRD 18.3 mewajibkan DTO Swift (dan C# nanti) berasal dari schema atau fixture yang sama dengan Rust, dengan test untuk enum, field opsional, timestamp, kode error, lebar integer, dan Unicode.
- **Anggaran sumber daya daemon.** NFR-003 menargetkan daemon idle di bawah 150 MiB RSS dan 1% satu CPU. NFR-004 mewajibkan semua antrean dan buffer berbatas, dan NFR-011 mewajibkan correlation ID di setiap operasi dan error.
- **Reproducible.** PRD 16 meminta toolchain dan dependency dipin setelah audit kompatibilitas, dan melarang membuat banyak crate kosong hanya untuk meniru pohon usulan.
- **Keamanan.** NFR-009 melarang temuan critical atau high yang belum diselesaikan di IPC, eksekusi provider, dan operasi destruktif. Permukaan `unsafe` dan dependency ikut menentukan seberapa besar area yang harus diaudit.
- **Realitas mesin uji.** App dengan target macOS 27 hanya bisa dijalankan di macOS 27. PRD 27 melarang test runtime nyata menyentuh data pribadi developer.
- **Tim dan proses.** Pemilik proyek perorangan dibantu agent, dengan rigor default Beta dan pendekatan Journey. Biaya operasional setiap tool tambahan terasa langsung.

## Options considered

Yang dibandingkan adalah stack utuh, bukan teknologi satu per satu. Pilihan per lapisan ada di tabel setelah bagian ini.

### Option 1: Workspace berlapis dengan crate di batas, IPC JSON, dan typeshare

Crate Rust terpisah hanya di batas yang perlu dijaga compiler (domain, platform, protocol, core, adapter, testkit, dua bin). Modul lain menjadi modul di dalam core. IPC memakai JSON dengan prefix panjang. Swift dibangkitkan dari tipe Rust oleh typeshare, dan JSON Schema oleh schemars, dengan fixture bersama sebagai penjaga.

**Pros**:
- Arah dependency domain ke adapter dijaga compiler, sehingga NFR-010 terbukti.
- Satu sumber kontrak. Perubahan protokol tanpa regenerasi Swift langsung gagal di `cargo xtask codegen --check`.
- Sedikit tool tambahan: tidak perlu Node, protoc, atau framework dependency injection.

**Cons**:
- Delapan crate plus xtask sejak scaffold. Ada lebih banyak `Cargo.toml` untuk dirawat dibanding satu crate.
- Bergantung pada typeshare, yang rilis terakhirnya Januari 2026, dan bentuk enum dibatasi oleh kemampuannya.

### Option 2: Satu crate core besar, IPC JSON, dan Swift ditulis tangan

Domain, application, engine, dan adapter dalam satu crate dengan modul. Bin daemon dan CLI tipis di atasnya. DTO Swift ditulis manual, dan kecocokan dijaga oleh fixture.

**Pros**:
- Paling cepat dimulai, dengan satu tempat untuk semua kode Rust.
- Tanpa generator, sehingga tidak ada risiko tool pembangkit yang mandek.

**Cons**:
- Larangan domain menyentuh API OS hanya dijaga lewat review. Satu `use libc` di domain lolos tanpa disadari.
- Setiap perubahan protokol ditulis dua kali, dan field yang terlewat hanya tertangkap kalau fixture mencakupnya.
- CLI ikut mengompilasi seluruh core, padahal CLI tidak boleh menjalankan logika lifecycle.

### Option 3: Pohon PRD penuh dengan Protobuf

Sekitar 17 crate persis seperti PRD 16, dengan Protobuf (prost, `swift-protobuf`, buf) sebagai encoding dan pembangkit kontrak untuk Rust, Swift, dan C#.

**Pros**:
- Batas modul paling tegas sejak hari pertama.
- Codegen resmi dan matang untuk ketiga bahasa, dengan aturan evolusi schema yang ketat.

**Cons**:
- Banyak crate tipis yang belum punya isi, bertentangan dengan catatan PRD 16, dan memperlambat build.
- Toolchain buf atau protoc ditambah plugin di setiap mesin. `oneof` kurang nyaman dipakai, debug pesan biner lebih sulit, dan PRD 18.1 menyebut Protobuf tidak perlu hanya demi IPC lokal.

## Rationale

Option 1 dipilih karena dua gaya paling berat di Context, portabilitas yang dibuktikan (NFR-010) dan satu kontrak untuk tiga bahasa (PRD 18.3), sama sama butuh penjagaan mekanis, bukan disiplin manusia. Crate di batas membuat compiler menolak domain yang mengimpor API OS, dan `cargo check` lintas target bisa dijalankan per crate. Option 2 lebih murah di hari pertama, tapi biayanya naik setiap kali ada fitur baru menyentuh domain atau protokol. Option 3 membayar biaya struktur dan toolchain sebelum ada kebutuhannya (basis: aturan dependency ke dalam pada arsitektur ports and adapters; PRD 15.2 dan 16).

JSON tetap dipakai, bukan Protobuf, karena IPC ini lokal dengan frame maksimal 1 MiB, sehingga ukuran dan kecepatan tidak menentukan. Yang menentukan adalah kemudahan debug dan dukungan bawaan di Swift (`Codable`) dan C# (`System.Text.Json`) (basis: PRD 18.1). Codegen dari tipe Rust dipilih supaya ada tepat satu tempat yang diubah. Risiko typeshare diterima secara sadar karena keluarannya disimpan di git dan fixture adalah kontrak yang sebenarnya. Kalau typeshare mandek, Swift bisa ditulis tangan tanpa mengubah wire format (basis: satu sumber kebenaran untuk kontrak, dijaga contract test di dua sisi).

Untuk library lintas fitur, setiap pilihan jatuh ke opsi yang membosankan dan matang: tokio, rusqlite, clap, tracing, thiserror. Dua pengecualian yang lebih baru diambil dengan sadar. jiff dipilih karena CLI harus menampilkan waktu lokal dari tzdb sistem dengan benar. serde_yaml_ng dipilih karena serde_yml terkena advisory soundness, dan serde_yaml sudah diarsipkan (basis: RUSTSEC-2025-0068).

Kamu memilih `cargo test` daripada `cargo-nextest`. Itu bisa berjalan, dengan syarat setiap integration test membuat test root dan port sendiri, dan child process selalu dibereskan oleh guard. Risikonya, test yang berbagi proses bisa saling mengganggu lewat state global atau child process yang tertinggal. Kalau gejala itu muncul, `cargo-nextest` tetap rekomendasi saya sebagai langkah berikutnya, karena perpindahannya tidak mengubah kode test.

### Pilihan per lapisan

| Lapisan | Pilihan | Runner up | Alasan singkat |
| --- | --- | --- | --- |
| Pembagian crate | Crate di batas saja | Satu crate core | Compiler menjaga NFR-010 tanpa crate kosong (basis: PRD 16) |
| Letak trait platform | Crate `stackly-platform` netral | Di domain | Domain tetap murni, adapter Windows nanti cukup bergantung pada crate kecil |
| Crate saat scaffold | Rantai dependency, tanpa storage | Minimal | Cukup untuk membuktikan arah dependency. Storage menunggu skema fitur 5 |
| Nama daemon | `stacklyd` | `stackly-daemon` | Konvensi Unix, pendek di `ps` dan Activity Monitor |
| Lisensi | MIT OR Apache-2.0 | Apache-2.0 | Konvensi ekosistem Rust, sama dengan default PRD (OQ-013) |
| Toolchain Rust | 1.99.1 dipin persis, edition 2024 | 1.98 | Reproducible. Naik versi lewat PR yang lulus semua cek di Mac ini (basis: PRD 16) |
| Toolchain Swift | Xcode 27, mode Swift 6 strict | Mode Swift 5 | Data race antara socket dan UI tertangkap saat compile, sebelum kode bertambah |
| Async runtime | tokio | smol | Socket, child process, sinyal, timer, dan channel berbatas dalam satu ekosistem (NFR-004) |
| Encoding IPC | JSON | Protobuf | Mudah diperiksa saat debug dan dibaca native di Swift dan C# (basis: PRD 18.1) |
| Framing | u32 big endian dengan batas | NDJSON | Batas dicek sebelum alokasi, sederhana di tiga bahasa, mudah diuji dengan fuzzing (basis: framing dengan prefix panjang) |
| DTO Swift | typeshare dan fixture bersama | Swift tulisan tangan | Satu sumber kontrak, drift tertangkap oleh `cargo xtask codegen --check` |
| JSON Schema | schemars dari tipe Rust | Ditulis tangan | Sumber yang sama dengan serde dan typeshare, dipakai C# dan validasi editor |
| Registry | rusqlite `bundled`, satu thread penulis | sqlx | Versi SQLite terkunci, API sync cocok dengan pola satu penulis |
| YAML | serde_yaml_ng | serde_norway | Fork serde_yaml yang dirawat dan disebut advisory RustSec |
| Waktu | jiff | chrono | Zona waktu lokal dari tzdb sistem, `Timestamp` RFC 3339 |
| ID | UUID v7 | ULID | Standar, berurutan waktu, dan ada tipe bawaannya di Swift dan C# (basis: RFC 9562) |
| CLI | clap v4 derive | argh | Subcommand bertingkat, completion, value enum |
| Error | thiserror bertipe, anyhow hanya di xtask dan test | anyhow | Exit code harus berasal dari kategori, bukan teks (fitur 8) |
| Logging | tracing JSON dengan span | log | Span membawa request ID dan operation ID (NFR-011) |
| `unsafe` | Forbid kecuali `platform-macos` | Forbid di semua crate | Area audit keamanan sempit dan jelas (NFR-009) |
| Proyek Xcode | `.xcodeproj` dengan synchronized folders | XcodeGen | Tanpa tool tambahan, konflik pbxproj jarang |
| Modul Swift | App, StacklyClient, StacklyUI | Satu target app | Client dan UI bisa diuji dan dipratinjau tanpa app (fitur 68) |
| Task runner | cargo xtask | just | Tanpa install tambahan, jalan juga di Windows |
| Test Rust | `cargo test` | `cargo-nextest` | Pilihanmu. Lihat catatan di atas |
| Test Swift | Swift Testing, plus XCUITest untuk UI | XCTest | Test berparameter cocok untuk fixture kontrak dan state mockup |
| Lint dan format | rustfmt dan clippy, `swift-format` | SwiftLint | Semuanya ikut toolchain, tanpa install tambahan |
| Audit dependency | `cargo-deny` | `cargo-deny` dan `cargo-about` | Lisensi, advisory, dan sumber dalam satu gate. Notice rilis milik fitur 60 |
| Bundle ID sementara | `com.ojiepermana.stackly` | `io.github.ojiepermana.stackly` | Namespace pemilik repo, diganti sebelum rilis publik bila perlu (OQ-001) |

Keputusan kecil yang saya ambil sendiri (RECOMMEND), dengan runner up:

- **Profil release:** `panic = "unwind"`, `lto = "thin"`, `debug = "line-tables-only"`, dan `split-debuginfo = "packed"`, sehingga dSYM disimpan untuk diagnosis. Dengan unwind, tokio mengisolasi panic per task dan panic hook mencatatnya, sementara `abort` mematikan seluruh daemon karena satu bug. Runner up: `abort` demi binary lebih kecil.
- **Penjaga arah dependency:** `cargo xtask check-deps` membaca `cargo metadata` dan menerapkan aturan di index.md. Runner up: `cargo-deny` `bans` per crate, yang kurang ekspresif untuk aturan per anggota workspace.
- **Cek portabilitas:** `cargo check` untuk target `aarch64-pc-windows-msvc` pada crate netral, dijalankan di Mac ini. `check` tidak melakukan link, jadi tidak butuh MSVC. Runner up: mesin Windows sungguhan, yang baru perlu di fase Windows.
- **Pusat versi dependency:** `[workspace.package]`, `[workspace.dependencies]`, dan `[workspace.lints]` di root, dan anggota memakai `workspace = true`. Runner up: versi per crate, yang mudah membuat versi ganda.
- **Mekanisme larangan `unsafe`:** `deny` di workspace, ditambah `#![allow(unsafe_code)]` hanya di platform-macos dan pemindaian oleh `check-deps`. Efeknya sama dengan `forbid` kecuali satu crate, karena `forbid` tidak bisa dibuka per crate. Runner up: tabel `[lints]` terpisah di platform-macos, yang mudah menyimpang dari tabel workspace.
- **App Sandbox mati di target app:** app perlu terhubung ke socket daemon dan membundel helper, dan distribusinya langsung, bukan lewat Mac App Store. Runner up: sandbox menyala dengan entitlement pengecualian, yang menambah kerumitan tanpa manfaat untuk distribusi langsung.
- **testkit tanpa core:** fake platform dan test root hanya butuh domain dan platform. Kalau testkit bergantung pada core, unit test di core melihat dua salinan tipe yang sama.
- **Protocol tidak bergantung pada domain:** DTO adalah kontrak publik yang stabil, sedangkan model domain bebas berubah. Pemetaan antara keduanya terjadi di daemon. Dengan begitu CLI bisa memakai protocol tanpa domain. Runner up: DTO memakai tipe domain langsung, yang lebih sedikit kode tapi membocorkan perubahan internal ke wire.
- **GUI tidak menautkan Rust:** tanpa FFI (misalnya uniffi). GUI hanya bicara lewat IPC, sesuai REQ-001. Runner up: FFI untuk berbagi tipe, yang ditolak karena menggoda GUI melewati daemon.

### Cross check 16 Oktober 2026

Satu pass review read only oleh model lain menemukan enam celah yang akan membuat scaffold menebak (asal proyek Xcode, manifest SwiftPM, nilai workspace Cargo, mekanisme typeshare, aturan `check-deps` yang harus berlaku untuk dependency langsung, dan isi scaffold yang provisional) serta perbaikan teknis (siklus testkit dan core, huruf UUID di Swift, mekanisme lint `unsafe`, dependency macOS di bin, target toolchain, dan profil release). Kedua belas perbaikan diterima engineer dan sudah masuk ke index.md.

### Bukti: pengecekan landscape 16 Oktober 2026

Dijalankan sekali lewat subagent researcher (5 pencarian, 8 halaman). Ringkasannya disimpan di `docs/.agent-cache/research/stack-arsitektur.md`.

- Rust stable terbaru 1.99.1. Edition 2024 stabil sejak 1.85.
- Crate typeshare 1.0.5 terbit 2 Januari 2026. Status perawatannya tidak terkonfirmasi, karena halaman rilis GitHub tidak terjangkau.
- serde_yml tidak aman dan tidak dirawat (RUSTSEC-2025-0068). Advisory itu menyebut serde_yaml_ng dan serde_norway sebagai fork yang dirawat.
- Xcode 27 (Swift 6.4, SDK macOS 27, butuh macOS 26.6 atau lebih baru) hanya terkonfirmasi dari sumber pihak ketiga.
- jiff 0.2.38 (6 Oktober 2026), belum 1.0.

### Perubahan 2026-10-17

CI ditiadakan atas keputusan pemilik proyek, dan pengembangan fokus di Mac ini dulu (macOS 27, Apple Silicon). Yang berubah: baris CI di tabel stack dan di Pilihan per lapisan dihapus, begitu juga Mac self hosted dan runner `macos-26`. Cek portabilitas kini hanya `cargo check --target aarch64-pc-windows-msvc` di Mac ini, tanpa cek native di Linux. `cargo xtask check-deps` dan `cargo xtask codegen --check` dijalankan manual, dan naik versi toolchain cukup lulus semua cek di Mac ini.

### Perubahan 2026-10-18

Rekonsiliasi dengan spec yang diterima sesudah scaffold. Keputusan di spec ini tidak berubah; yang diperbarui adalah peta dan aturan yang diturunkan spec lain:

- *Peta crate*: crate baru `stackly-storage` (spec 0005), `stackly-ipc` dan `stacklyd` sebagai library plus bin (spec 0006), `stackly-fake-server` (spec 0009), `stackly-evidence` (spec 0002, sebelumnya hanya disebut di aturan), dan dependency baru tiap crate dari spec 0004, 0005, 0006, 0009, dan 0011. `stackly-storage` tidak lagi boleh bergantung pada core, karena port registry diputuskan ada di `stackly-domain` (spec 0005, dikonfirmasi engineer).
- *Aturan arah dependency*: aturan `check-deps` dari spec 0004, 0005, 0006, 0009, dan 0011 dicatat di satu tempat; pengecualian `unsafe` untuk `stackly-storage` dihapus karena storage tidak memakainya.
- *Proposed stack*: baris registry (WAL, `synchronous=FULL`, `fullfsync`, satu thread), manifest provider, secret, transport IPC, fuzzing, dan sink log.
- *Susunan repo*, *Alur kontrak*, dan *Konfigurasi workspace* mengikuti spec 0006 dan 0009, termasuk fallback enum terbuka `unrecognized` yang diputuskan saat pengecekan silang spec 0006 dengan state `unknown` di spec 0004.
- *Batas keputusan* mendapat kolom yang menunjuk spec yang sudah memutuskan setiap baris.

## References

**Project sources**:
- `prd.md` bagian 14 (NFR-003, NFR-004, NFR-009, NFR-010, NFR-011), 15, 15.1, 15.2, 16, 18.1, 18.3, 25, 27, dan 38 (OQ-001, OQ-002, OQ-005, OQ-013)
- `docs/scope/01-foundation.md`, fitur 1 dan batas fitur 2 sampai 14
- `docs/scope/ui-macos.md`, fitur 12, 13, 67, 68
- `docs/scope/index.md`, pendekatan Journey dan workflow Beta
- `docs/.agent-cache/research/stack-arsitektur.md`, pengecekan landscape

**Practices & standards**:
- Ports and adapters, dengan dependency yang mengarah ke dalam
- Satu sumber kebenaran untuk kontrak, dijaga contract test di dua sisi
- Framing dengan prefix panjang dan batas yang dicek sebelum alokasi
- Toolchain dipin untuk build yang reproducible
- Meminimalkan permukaan `unsafe` untuk mempersempit audit keamanan
- UUID versi 7 (RFC 9562)

**Links** (diverifikasi saat pengecekan landscape):
- Rust releases: https://releases.rs/
- typeshare di crates.io: https://crates.io/api/v1/crates/typeshare
- jiff di crates.io: https://crates.io/api/v1/crates/jiff
- RUSTSEC-2025-0068 (serde_yml): https://api.osv.dev/v1/vulns/RUSTSEC-2025-0068
