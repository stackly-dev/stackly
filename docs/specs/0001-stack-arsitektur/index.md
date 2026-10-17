# 0001. Stack dan arsitektur Stackly

**Date**: 2026-10-16
**Status**: Accepted

## Summary

Spec ini mengunci bagian stack Stackly yang belum dikunci PRD: versi toolchain, pembagian crate Rust, encoding IPC (OQ-005), dan library serta tooling yang dipakai banyak fitur. Batas crate dipilih supaya compiler sendiri menolak kode domain yang menyentuh API macOS, sehingga core tetap siap untuk Windows. Kontrak IPC ditulis sekali di Rust, lalu tipe Swift dan JSON Schema dibangkitkan (dibuat otomatis) darinya. Scaffold (`/develop stack & arsitektur`) membuat workspace Rust dan proyek SwiftUI kosong dari peta di bawah, tanpa crate yang belum punya isi.

## Decision

**Chosen option**: Option 1: Workspace berlapis dengan crate di batas, IPC JSON, dan typeshare

Stackly dibangun sebagai satu daemon Rust per user dengan dua client (CLI Rust dan app SwiftUI) yang hanya bicara lewat IPC JSON berprefix panjang di Unix socket. Kodenya dibagi ke crate yang batasnya dijaga compiler, dan kontraknya dibangkitkan dari tipe Rust.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya edition 2024, `cargo test`, dan kebijakan `unsafe`.

## Proposed stack

Sudah dikunci PRD dan tidak dibahas ulang: Rust untuk core, daemon, dan CLI; SwiftUI untuk GUI; SQLite untuk registry; Unix domain socket privat tanpa listener TCP; macOS 27.

| Lapisan | Pilihan | Alasan |
| --- | --- | --- |
| Target platform | macOS 27, Apple Silicon (`aarch64-apple-darwin`) saja, tanpa universal binary | Dikunci PRD (OQ-002). Intel menunggu matrix lulus |
| Pola arsitektur | Satu daemon per user (monolith berlapis) dengan CLI dan GUI sebagai client IPC. GUI tidak menautkan Rust lewat FFI | REQ-001: hanya daemon yang menulis registry dan menjalankan proses |
| Toolchain Rust | Rust 1.99.1 dipin persis di `rust-toolchain.toml` (komponen rustfmt dan clippy), edition 2024 di semua crate, `rust-version = "1.99.1"` | Build reproducible. Naik versi lewat PR khusus yang lulus semua cek di Mac ini |
| Toolchain Swift | Xcode 27 (Swift 6.4, SDK macOS 27), mode bahasa Swift 6 dengan strict concurrency penuh | Data race antara client socket dan UI tertangkap saat compile |
| Async runtime | tokio, runtime multi thread | Socket, child process, sinyal, timer, dan channel berbatas (NFR-004) dalam satu ekosistem |
| Encoding IPC (OQ-005) | JSON `UTF-8` lewat serde_json, nama field camelCase | Mudah diperiksa saat debug, dan dibaca native oleh Swift dan C# nanti |
| Framing IPC | Panjang `u32` big endian (4 byte) lalu payload. Batas 1 MiB untuk frame kontrol dan 64 KiB untuk batch log, dicek sebelum alokasi | Sederhana di tiga bahasa dan mudah diuji dengan fuzzing. Angka batas dari PRD 18.3 |
| Kontrak Swift (OQ-005) | typeshare membangkitkan `Codable` Swift dari tipe di `stackly-protocol`. Hasilnya disimpan di git dan dicek ulang dengan `cargo xtask codegen --check`. Fixture JSON bersama diuji di Rust dan Swift | Satu sumber kontrak. Fixture tetap menjadi kontrak kalau generator diganti |
| JSON Schema | schemars dari tipe Rust yang sama, ditulis oleh `cargo xtask codegen` | Dipakai C# nanti, validasi editor manifest, dan dokumentasi |
| Registry | rusqlite dengan fitur `bundled`, satu thread penulis yang menerima perintah lewat channel dari tokio | Versi SQLite terkunci, cocok dengan pola satu penulis |
| YAML | serde_yaml_ng | Fork serde_yaml yang dirawat. serde_yml dilarang (RUSTSEC-2025-0068) |
| Waktu | jiff: `Timestamp` UTC di registry dan wire (RFC 3339), zona waktu lokal dari tzdb sistem untuk tampilan CLI | Benar untuk zona waktu, tanpa database zona yang dibundel |
| ID resource | UUID v7 (crate `uuid`, fitur `v7` dan `serde`). Hanya daemon yang membuat ID dan path. Rust selalu menulis lowercase dalam format standar, dan semua pembaca tidak peka huruf besar kecil (`UUID` di Swift menulis huruf besar saat encode) | Berurutan waktu, ada tipe bawaannya di Swift dan C# |
| Parser CLI | clap v4 derive | Subcommand bertingkat, value enum, completion |
| Error | thiserror di setiap crate library, kategori kode stabil di `stackly-protocol`. anyhow hanya di `xtask` dan test | Exit code CLI berasal dari kategori, bukan teks pesan |
| Logging | tracing dan `tracing-subscriber` dengan output JSON. Span membawa request ID dan operation ID | Correlation ID otomatis di setiap baris (NFR-011) |
| Kebijakan `unsafe` | Dilarang di semua crate kecuali `stackly-platform-macos` (dan `stackly-storage` bila terbukti perlu). Mekanismenya ada di Konfigurasi workspace. Setiap blok `unsafe` wajib komentar `// SAFETY:` | Area audit keamanan sempit (NFR-009) |
| Proyek Xcode | `apps/macos/Stackly.xcodeproj` dibuat sekali dari template app macOS di Xcode 27, lalu disimpan di git dengan synchronized folder groups. Setelannya ada di bagian Proyek Xcode dan package Swift | Tanpa tool tambahan, konflik pbxproj jarang |
| Modul Swift | Target app `Stackly`, package lokal `StacklyClient` (`clients/swift`) dan `StacklyUI` (`apps/macos/Packages/StacklyUI`) | Client dan UI bisa diuji dan dipratinjau tanpa app |
| Test Rust | `cargo test` | Pilihan engineer. Lihat aturan isolasi di Consequences |
| Test Swift | Swift Testing untuk unit test, XCUITest untuk UI test | Test berparameter cocok untuk fixture kontrak |
| Format dan lint | rustfmt dan clippy, `swift-format` (`.swift-format`) | Semuanya ikut toolchain |
| Audit dependency | `cargo-deny` (lisensi, advisory RustSec, ban, sumber) | Satu gate untuk PRD 27 |
| Task runner | `cargo xtask` (crate `xtask/`, alias di `.cargo/config.toml`) | Tanpa install tambahan, jalan juga di Windows |
| Lisensi | `MIT OR Apache-2.0` (file `LICENSE-MIT` dan `LICENSE-APACHE`) | Konvensi Rust, default PRD (OQ-013) |
| Nama dan identifier | CLI `stackly`, daemon `stacklyd`, bundle ID sementara `com.ojiepermana.stackly` | OQ-001 tetap terbuka sampai sebelum rilis publik |

### Peta crate

Semua crate library memakai prefix `stackly-`. Kolom terakhir menunjukkan kapan crate dibuat. Crate yang dibuat saat scaffold diberi satu isi nyata yang kecil, bukan dibiarkan kosong.

| Crate (path) | Isi | Boleh bergantung pada | Dibuat |
| --- | --- | --- | --- |
| `stackly-domain` (`crates/domain`) | Model, invariant, state machine, policy | Crate pihak ketiga netral OS saja (serde, thiserror, uuid, jiff). Tidak ada crate Stackly lain, tokio, atau libc sebagai dependency langsung | Scaffold, dengan satu tipe `InstanceId` (UUID v7) |
| `stackly-platform` (`crates/platform`) | Trait platform netral OS (proses, filesystem, resolver direktori, identitas user, secret, endpoint IPC, jam) dan tipe pendukungnya | `stackly-domain` | Scaffold, dengan satu trait resolver `app_support_dir()` yang ditandai `// provisional: F4` |
| `stackly-protocol` (`crates/protocol`) | DTO request, response, dan event, envelope error, kategori kode, codec framing sebagai fungsi byte murni tanpa IO | Crate pihak ketiga netral OS saja (serde, serde_json, schemars, typeshare, uuid, jiff, thiserror). Tidak bergantung pada `stackly-domain` | Scaffold, dengan satu tipe `ProtocolVersion` |
| `stackly-core` (`crates/core`) | Application services dan engine sebagai modul (runtime, instance, supervisor, provider engine, environment, operation, diagnostics), diekstrak menjadi crate saat batasnya terbukti | `stackly-domain`, `stackly-platform`, tokio, tracing | Scaffold, dengan satu use case info daemon yang mengembalikan versi crate (`CARGO_PKG_VERSION`) |
| `stackly-platform-macos` (`crates/platform-macos`) | Implementasi trait platform untuk macOS | `stackly-platform`, `stackly-domain`, crate khusus macOS | Scaffold, dengan resolver dari `std::env::home_dir()` ditambah `Library/Application Support/Stackly`, tanpa `unsafe` (implementasi final di fitur 4) |
| `stackly-storage` (`crates/storage`) | Adapter SQLite, migrasi, journal, audit | `stackly-domain`, `stackly-platform`, `stackly-core` bila port registry didefinisikan di sana, rusqlite | Fitur 5 |
| `stackly-testkit` (`crates/testkit`) | Fake platform, builder fixture, test root ephemeral | `stackly-domain`, `stackly-platform` saja. Tidak boleh bergantung pada `stackly-core`, supaya unit test di core tidak melihat dua salinan tipe yang sama | Scaffold, dengan fake resolver direktori yang menunjuk ke test root |
| `stacklyd` (`apps/daemon`) | Bin daemon: merangkai dependency secara eksplisit, server IPC | Semua crate library kecuali testkit. `stackly-platform-macos` dipasang di bawah `[target.'cfg(target_os = "macos")'.dependencies]` | Scaffold, dengan `--version` yang mencetak versi dari use case info daemon |
| `stackly-cli`, bin `stackly` (`apps/cli`) | Client IPC dan presentasi | `stackly-protocol`, `stackly-platform`, clap, tokio, dan `stackly-platform-macos` di bawah `cfg(target_os = "macos")` | Scaffold, dengan `--version` |
| `xtask` (`xtask/`) | `codegen`, `codegen --check`, `check-deps`, `build-app` | Bebas, tidak pernah dikirim ke pengguna | Scaffold |

`stackly-platform-windows` dan client C# baru dibuat di fase Windows.

### Aturan arah dependency

Aturan ini dijaga oleh `cargo xtask check-deps`, yang membaca `cargo metadata` dan gagal kalau ada pelanggaran. Perintah ini dijalankan manual di Mac ini. Aturan hanya berlaku untuk **dependency langsung**, dibedakan menurut jenisnya (normal, dev, build). Dependency transitif tidak dicek, karena misalnya `uuid` v7 sah menarik `getrandom` lalu `libc`. Portabilitas dibuktikan oleh cek lintas target, bukan oleh daftar crate terlarang.

- `stackly-domain`, `stackly-platform`, `stackly-protocol`, dan `stackly-core` tidak pernah bergantung langsung pada `stackly-platform-macos`, `stackly-storage`, atau crate yang hanya ada di macOS.
- `stackly-domain` dan `stackly-protocol` tidak bergantung pada crate Stackly mana pun, dan tidak punya tokio atau libc sebagai dependency langsung.
- `stackly-core` tidak bergantung pada `stackly-protocol` atau `stackly-storage`. Pemetaan dari domain ke DTO terjadi di `stacklyd`.
- `stackly-cli` tidak pernah bergantung pada `stackly-core` atau `stackly-storage`, karena CLI tidak menjalankan binary provider dan tidak menulis registry.
- `stackly-testkit` hanya boleh muncul sebagai dependency berjenis dev, dan tidak bergantung pada `stackly-core`.
- [Spec 0002](../0002-coding-standards-tooling/index.md) menambah crate daun `stackly-evidence` (`crates/evidence`) dengan aturannya sendiri: tanpa crate Stackly lain, hanya dependency dev kecuali di `xtask` dan `stackly-testkit`, dan boleh menjadi dependency dev `stackly-domain` serta `stackly-protocol`.
- `#![allow(unsafe_code)]` hanya boleh ada di `stackly-platform-macos` (dan `stackly-storage` bila nanti terbukti perlu). `check-deps` memindai source crate lain dan gagal kalau menemukannya.
- Cek portabilitas (NFR-010, AC-050): di Mac ini, jalankan `cargo check --target aarch64-pc-windows-msvc` untuk `stackly-domain`, `stackly-platform`, `stackly-protocol`, dan `stackly-core`. `check` tidak melakukan link, jadi tidak butuh MSVC.

### Susunan repo

```text
stackly/
├── Cargo.toml              # workspace: resolver, package, dependencies, lints terpusat
├── Cargo.lock              # disimpan di git
├── rust-toolchain.toml     # 1.99.1, rustfmt, clippy, dua target
├── .cargo/config.toml      # alias xtask
├── typeshare.toml          # konfigurasi generator Swift
├── .gitignore              # target/, DerivedData, xcuserdata, .DS_Store
├── LICENSE-MIT
├── LICENSE-APACHE
├── README.md
├── crates/
│   ├── domain/
│   ├── platform/
│   ├── protocol/
│   ├── core/
│   ├── platform-macos/
│   ├── testkit/
│   └── storage/            # dibuat di fitur 5
├── apps/
│   ├── daemon/             # bin stacklyd
│   ├── cli/                # bin stackly
│   └── macos/
│       ├── Stackly.xcodeproj
│       ├── Stackly/        # target app (synchronized folder)
│       └── Packages/StacklyUI/
├── clients/swift/          # package StacklyClient
│   └── Sources/StacklyClient/Generated/StacklyProtocol.swift
├── protocols/
│   ├── schemas/            # JSON Schema protokol, hasil codegen
│   └── fixtures/           # fixture JSON bersama Rust dan Swift
├── schemas/                # schema provider, environment, lockfile (fitur 9, 44)
├── providers/              # paket provider (fitur 19 dan seterusnya)
├── xtask/
└── docs/
```

Integration test Rust berada di folder `tests/` milik masing masing crate. Spec di `docs/specs/` menggantikan folder ADR `docs/architecture/decisions/` yang diusulkan PRD.

### Alur kontrak

- **Sumber kebenaran:** tipe di `stackly-protocol` dengan derive `Serialize`, `Deserialize`, `JsonSchema`, dan atribut `#[typeshare]`.
- **Bentuk serde:** `rename_all = "camelCase"`. Enum yang membawa data memakai bentuk adjacently tagged (`tag = "type"`, `content = "content"`), karena itulah bentuk yang didukung typeshare untuk Swift. Enum tanpa data ditulis sebagai string.
- **Codegen:** `cargo xtask codegen` menjalankan `typeshare` untuk menulis satu file `clients/swift/Sources/StacklyClient/Generated/StacklyProtocol.swift`, lalu schemars untuk menulis schema ke `protocols/schemas/`. `typeshare.toml` di root mengatur decorator Swift `Sendable` dan `Equatable` (Swift 6 menolak DTO yang tidak `Sendable` saat berpindah dari client ke UI di `MainActor`).
- **Pin `typeshare-cli`:** `typeshare-cli` adalah binary terpisah, jadi xtask menyimpan versinya sebagai konstanta, mencocokkannya dengan output `typeshare --version`, dan gagal dengan petunjuk `cargo install --locked typeshare-cli --version <versi>` kalau tidak cocok. Versi persisnya ditetapkan saat scaffold.
- **Cek drift:** typeshare tidak punya mode check. `cargo xtask codegen --check` membangkitkan ke direktori sementara, membandingkannya dengan file yang disimpan di git, dan gagal kalau berbeda.
- **Fixture:** satu file JSON per kasus di `protocols/fixtures/`. Test Rust membaca file itu dan memeriksa serialize serta deserialize. Test Swift Testing membaca file yang sama lewat path relatif dari `#filePath`, lalu membandingkan **nilai hasil decode**, bukan teks JSON mentah (contohnya, Swift menulis UUID dengan huruf besar). Cara ini bergantung pada path source yang asli, jadi build tidak boleh memakai remap path (`-file-prefix-map`).
- **Bukti di scaffold:** satu tipe kecil (`ProtocolVersion` dengan `major` dan `minor` bertipe `u32`, fixture `{"major": 0, "minor": 1}`) melewati seluruh alur: codegen, fixture, test Rust, dan test Swift. Pesan protokol yang sebenarnya didesain di fitur 6.

### Konfigurasi workspace

- `[workspace]` menulis `resolver = "3"` secara eksplisit. Workspace virtual tidak mewarisi resolver dari edition anggotanya.
- `[workspace.package]` memuat `version = "0.1.0"`, edition 2024, `rust-version = "1.99.1"`, `license = "MIT OR Apache-2.0"`, `repository = "https://github.com/ojiepermana/stackly"`, dan `publish = false`. Semua anggota mewarisinya dengan `workspace = true`, sehingga `--version` di kedua bin bersumber dari satu angka.
- `[workspace.dependencies]` memuat semua versi crate pihak ketiga sebagai rentang caret (misalnya `"1.47"`), dan versi persisnya dikunci oleh `Cargo.lock` yang disimpan di git. Anggota tidak menulis versi sendiri. Scaffold hanya menambahkan crate yang benar benar dipakai isi scaffold. Contohnya, rusqlite masuk di fitur 5, serde_yaml_ng di fitur 9, dan clap hanya dipakai kedua bin.
- `[workspace.lints]` memuat `rust.unsafe_code = "deny"` dan `clippy.undocumented_unsafe_blocks = "deny"`, dan semua anggota mewarisinya (`lints.workspace = true`). `stackly-platform-macos` menambahkan `#![allow(unsafe_code)]` di `lib.rs`, dan `check-deps` menolak `allow` itu di crate lain. Mekanisme ini dipakai karena `forbid` tidak bisa dibuka per crate, dan crate yang menulis `[lints]` sendiri tidak bisa sekaligus mewarisi tabel workspace.
- `[profile.release]` memakai `panic = "unwind"` (tokio mengisolasi panic per task, panic hook mencatatnya), `lto = "thin"`, `debug = "line-tables-only"`, dan `split-debuginfo = "packed"` tanpa `strip`, sehingga dSYM berisi informasi baris yang cukup untuk diagnosis crash. dSYM disimpan sebagai artefak dan tidak ikut ke app.
- `rust-toolchain.toml` memasang target `aarch64-apple-darwin` dan `aarch64-pc-windows-msvc`. Target Windows hanya berisi std (kecil), supaya kamu bisa menjalankan cek portabilitas secara lokal.
- `.gitignore` mengabaikan `target/`, `DerivedData/`, `xcuserdata/`, `*.xcuserstate`, dan `.DS_Store`.
- Saat scaffold, log daemon dan CLI hanya ke stderr. Lokasi file log daemon mengikuti resolver direktori di fitur 4, dan sink file serta rotasinya dipasang saat daemon dibangun di fitur 6.

### Proyek Xcode dan package Swift

- **Asal proyek:** `Stackly.xcodeproj` dibuat sekali dari template app macOS di Xcode 27, lalu disimpan di git. Setelah itu, file baru masuk lewat synchronized folder tanpa mengubah pbxproj.
- **Setelan target app yang menyimpang dari template:**
  - `ARCHS = arm64`, supaya build Release tidak menjadi universal binary.
  - `MACOSX_DEPLOYMENT_TARGET = 27.0` dan `PRODUCT_BUNDLE_IDENTIFIER = com.ojiepermana.stackly`.
  - Signing ad hoc: `CODE_SIGN_IDENTITY = "-"`, tanpa `DEVELOPMENT_TEAM`. Identity development nyata dipasang di fitur 15.
  - **App Sandbox mati** (`ENABLE_APP_SANDBOX = NO`, entitlement sandbox dihapus), karena app harus terhubung ke socket daemon di direktori runtime privat dan membundel helper. Distribusi tidak lewat Mac App Store (PRD 25).
  - Hardened runtime dibiarkan seperti default template. Keputusannya, bersama notarization, ada di fitur 15 dan 60.
  - `SWIFT_VERSION = 6` (strict concurrency penuh) dan default isolation `MainActor` untuk target app.
- **Manifest package:** `swift-tools-version` sama dengan versi Swift dari Xcode yang dipin. Platform ditulis dalam bentuk string, `.macOS("27.0")`, supaya tidak bergantung pada nama konstanta versi.
  - `StacklyClient` tidak memakai default isolation `MainActor`, karena IO socket berjalan di luar main actor. DTO hasil generator bersifat `Sendable`.
  - `StacklyUI` memakai `.defaultIsolation(MainActor.self)` dan berisi satu `RootView` placeholder dengan `#Preview`, karena SwiftPM menolak target tanpa source. App menampilkan `RootView` di satu `WindowGroup`. Shell sebenarnya dibangun di fitur 13.
- **Perintah standar:** `swift test --package-path clients/swift` untuk test kontrak Swift, dan `cargo xtask build-app` (yang memanggil `xcodebuild` untuk scheme `Stackly`) untuk build app.

### Batas keputusan

Hal berikut sengaja tidak diputuskan di sini. Masing masing dimiliki fitur yang membutuhkannya.

| Keputusan | Pemilik |
| --- | --- |
| Format dan lint yang dipasang (`rustfmt.toml`, `.swift-format`, `deny.toml`), plus pencatatan konvensi ke `AGENTS.md`. Batasannya: tanpa remap path | Fitur 2 |
| Desain trait platform, resolver direktori, dan cara mengarahkan root alternatif untuk test | Fitur 4 |
| Skema registry, mekanisme migrasi, dan letak port registry | Fitur 5 |
| Bentuk pesan, handshake, envelope error, pemetaan tipe asing di typeshare (`Uuid`, `Timestamp`), integer 64 bit, enum yang tidak dikenal, adapter IO untuk framing, sink log daemon | Fitur 6 |
| Journal operasi, lock, dan idempotency | Fitur 7 |
| Kontrak output CLI dan peta exit code | Fitur 8 |
| Validator JSON Schema dan template engine manifest | Fitur 9 |
| API proses macOS dan identitas proses | Fitur 10 |
| Crate untuk Keychain | Fitur 11 |
| Transport IPC di Swift dan shell aplikasi | Fitur 13 |
| Memasukkan `stacklyd` dan `stackly` ke `.app`, signing, SMAppService, bundle ID helper | Fitur 15 |
| HTTP client untuk download dan ekstraksi archive | Fitur 18 |
| Library snapshot test Swift | Fitur 68 |

## Consequences

**Positive**:
- Compiler menolak domain, platform, protocol, dan core yang menyentuh API macOS, dan cek lintas target membuktikannya (NFR-010).
- Satu kontrak untuk Rust dan Swift. Perubahan protokol tanpa regenerasi langsung gagal di `cargo xtask codegen --check`.
- Sedikit tool di luar toolchain: hanya `typeshare-cli` dan `cargo-deny`. Tidak perlu Node, protoc, atau XcodeGen.
- Permukaan `unsafe` terkumpul di satu crate, sehingga review keamanan tahu persis harus melihat ke mana.

**Negative / tradeoffs**:
- **typeshare:** rilis terakhirnya Januari 2026. Bentuk enum terikat pada adjacently tagged, dan integer 64 bit butuh penanganan eksplisit. Kalau typeshare mandek, Swift bisa ditulis tangan tanpa mengubah wire, karena fixture tetap menjadi kontrak.
- **`cargo test` tanpa nextest:** test dalam satu biner berbagi proses. Setiap integration test wajib membuat test root dan port sendiri, child process wajib dibereskan oleh guard, dan test tidak boleh memakai state global. Pindah ke `cargo-nextest` kalau muncul gejala saling ganggu.
- **jiff masih 0.2:** satu kali upgrade breaking saat 1.0 keluar.
- **Pin persis:** setiap sekitar enam minggu ada PR naik versi Rust, dan Xcode juga harus dinaikkan dengan sengaja.
- **Delapan crate dan xtask sejak scaffold:** lebih banyak `Cargo.toml` dan sedikit lebih lama untuk build bersih dibanding satu crate.
- **rusqlite `bundled`:** mengompilasi kode C SQLite, sehingga build bersih lebih lama dan butuh compiler C (sudah ada di Xcode Command Line Tools).
- **Data Xcode 27** (Swift 6.4, butuh macOS 26.6 atau lebih baru) baru terkonfirmasi dari sumber pihak ketiga.
- **App Sandbox mati:** app berjalan dengan seluruh izin user, bukan di dalam kontainer sandbox. Ini sesuai distribusi langsung (PRD 25), tapi artinya batas keamanan GUI bergantung pada kode kita sendiri dan pada daemon yang memverifikasi peer.

**Neutral**:
- File governance (CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, GOVERNANCE) datang bersama fitur 38. Scaffold hanya membuat README dan dua file lisensi.
- Mac Intel tidak didukung sampai OQ-002 dibuka lagi, karena tidak ada universal binary.
- `docs/.agent-cache/` berisi hasil pengecekan landscape dan penemuan tool, dan boleh dibuang dari git kalau kamu tidak mau menyimpannya.

## Follow-up

- [ ] Saat scaffold, verifikasi Xcode 27 dan versi Swift di dalamnya (diduga 6.4), lalu pin versi persisnya di `swift-tools-version`. Cek juga nama setelan build untuk default isolation di Xcode 27.
- [ ] Saat scaffold, pin versi `typeshare-cli` dan cek aktivitas repo typeshare. Kalau mandek, ganti ke Swift tulisan tangan lewat `/architect` update (wire tidak berubah).
- [ ] OQ-001: tetapkan bundle ID final sebelum rilis publik (sementara `com.ojiepermana.stackly`).
- [ ] OQ-013: konfirmasi lisensi `MIT OR Apache-2.0` dengan product owner sebelum source dipublikasikan.
- [ ] Root `AGENTS.md` belum ada. `/audit` (fitur 2) perlu mencatat stack ini di sana.
- [ ] Konvensi `rust-best-practices` dan `rust-async-patterns` belum ada di root `AGENTS.md` `## Agent skills`. Keduanya berlaku untuk semua crate Rust, jadi tempatnya di root.
- [ ] Konvensi `swiftui-expert-skill`, `swift-concurrency`, dan `swift-testing-pro` belum tercatat. Tempatnya di `AGENTS.md` bersarang untuk kode Swift (`apps/macos/AGENTS.md`, dan `clients/swift/AGENTS.md` yang menunjuk ke sana), bukan di root.
- [ ] `AGENTS.md` perlu baris `MCP servers:` (XcodeBuildMCP, GitHub MCP server, SQLite MCP, yang kamu sambungkan sendiri lewat `claude mcp add` setelah memverifikasi repo masing masing, dengan SQLite hanya diarahkan ke test root) dan baris `Declined:` (`rust-engineer`, `github-actions` dari callstackincubator, `github-actions-templates`).
- [ ] Pertimbangkan `cargo-nextest` di fitur 2 atau 64 kalau integration test mulai saling ganggu.
- [ ] Upgrade jiff saat versi 1.0 rilis.

## Rationale

Alasan, opsi yang dibandingkan, dan bukti pengecekan landscape ada di [rationale.md](rationale.md).
