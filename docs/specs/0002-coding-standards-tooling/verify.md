# Verify: Coding standards & tooling · spec 0002 · updated 2026-10-18

_Langkah diturunkan dari Done when fitur 2 dan keputusan di spec 0002. Spec ini tidak punya ID `AC-N` sendiri, jadi tiap langkah menunjuk ID Trace dari scope (`REQ-051`, `OQ-011`, PRD bagian 27, 28, 31, 32.1, 32.3) atau bagian spec yang dibuktikannya. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27. Probe yang menambah file atau mengubah file sementara selalu dikembalikan sesudahnya (`git status` bersih lagi)._

## UI / manual

- [x] Baca root `AGENTS.md` → bagian Stack, Commands, Rules, dan Tooling cocok dengan repo (Rust 1.99.1, Swift 6, Xcode 27, cargo xtask), dan Rules mewajibkan ID PRD tetap stabil di spec dan test, status awal `NOT_RUN`, serta `TestRoot`, `allocate_port`, dan fixture credential untuk integration test → PRD bagian 31

## Commands

### Tooling bersih di Mac ini

- [x] `cargo fmt --all --check` → exit 0 → PRD bagian 31
- [x] `cargo build --workspace --locked` → exit 0 → PRD bagian 31
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings` → exit 0, tanpa warning → PRD bagian 31
- [x] `cargo test --workspace --locked --no-fail-fast` → semua biner test lulus; `xtask/tests/native_runtime.rs` melaporkan `0 passed; 5 ignored` → PRD bagian 31, 32.1
- [x] `swift test --package-path clients/swift` → 42 test `StacklyTestSupportTests` dan 2 test `StacklyClientTests` lulus; suite `SharedExamplesTests` jalan tanpa syarat (skip provisional F2 sudah dihapus) → REQ-051
- [x] `git ls-files -- '*.swift' ':(exclude)**/Generated/**' | xargs xcrun swift-format lint --strict` → exit 0 untuk 18 file. Catatan: `swift-format lint -r clients/swift` tanpa pengecualian gagal di `Generated/StacklyProtocol.swift`, yang memang dilewati hook dan `.swift-format` → PRD bagian 31
- [x] `cargo xtask codegen --check` → "6 generated files are up to date", termasuk empat schema di `schemas/evidence/` → PRD bagian 31 (validasi schema)
- [x] Ubah satu doc comment `EvidenceRecord` di `crates/evidence/src/record.rs`, jalankan `cargo xtask codegen --check` → gagal dan menyebut `schemas/evidence/record.schema.json` serta `report.schema.json`; kembalikan → Evidence record `stackly.evidence/v1`
- [x] `cargo deny check` → "advisories ok, bans ok, licenses ok, sources ok" → PRD bagian 31 (lisensi dan security static checks)
- [x] `cargo xtask check-deps` → "10 crates follow the dependency and unsafe rules" → Crate `stackly-evidence`
- [x] Tambahkan sementara `stackly-evidence.workspace = true` ke `[dependencies]` `crates/core/Cargo.toml`, jalankan `cargo xtask check-deps` → gagal, "stackly-evidence is a dev dependency everywhere except xtask and stackly-testkit"; kembalikan → Crate `stackly-evidence`
- [x] Tambahkan sementara `stackly-domain.workspace = true` ke `[dependencies]` `crates/evidence/Cargo.toml`, jalankan `cargo xtask check-deps` → gagal, "stackly-evidence depends on no Stackly crate"; kembalikan. `stackly-domain` sudah memakai `stackly-evidence` sebagai dependency dev dan lolos → Crate `stackly-evidence`
- [x] `cargo check --locked --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → exit 0 → AGENTS.md Rules
- [x] `cargo xtask build-app` → `** BUILD SUCCEEDED **`, `target/xcode/Build/Products/Debug/Stackly.app` → PRD bagian 31
- [x] Stage file Rust yang salah format dan memakai `unwrap()`, plus file Swift yang salah format, lalu jalankan `sh .githooks/pre-commit` → exit 1: `cargo fmt --check` menunjukkan diff, clippy per crate gagal, `swift-format lint --strict` gagal; unstage dan hapus → Tooling

### Wrapper `cargo xtask evidence run`

- [x] `cargo xtask evidence run --kind fixture -- cargo test --workspace --locked --no-fail-fast` → exit 0, 1 record fixture `PASS` untuk `REQ-051` (`guard::tests::guard_writes_one_record_per_test_with_its_final_status`), 0 masalah; `context` berisi `sourceCommit`, `sourceDirty: false`, host Mac13,1, toolchain `1.99.1` / `27.0 (27A266a)` / `6.4`, dan `baseline: { matches: true, osBuildMatches: true }`; `test.file` relatif terhadap root workspace → REQ-051, PRD bagian 28
- [x] `cargo xtask evidence run --kind fixture -- swift test --package-path clients/swift --skip 'NativeTests\.'` → exit 0, 3 record Swift fixture `PASS` untuk `REQ-051`, `package` null, `target` `StacklyTestSupportTests`, `file` relatif → REQ-051, PRD bagian 28
- [x] `cargo xtask evidence run --kind fixture --out <folder> -- sh -c 'echo ok'` → exit 0, folder berisi `run.json`, `records/`, `artifacts/`, `output.log`, `report.json`; baris terakhir stderr `evidence report: <path absolut>` → Perintah xtask
- [x] Perintah `sh -c 'exit 7'` → wrapper keluar 7 → Sinyal dan exit code
- [x] `STACKLY_EVIDENCE_DIR=/tmp cargo xtask evidence run ...` → ditolak, "evidence runs never nest", folder run tidak dibuat → Perintah xtask
- [x] `--kind banana` dan tanpa `--kind` → keduanya ditolak dengan pesan jelas, exit 1 → Perintah xtask
- [x] Perintah mencetak `$STACKLY_EVIDENCE_DIR` dan `$STACKLY_EVIDENCE_KIND` → keduanya diisi wrapper (path absolut folder run dan kind run) → Variabel environment
- [x] Kirim SIGINT ke wrapper selama perintah `sleep` jalan → perintah menerima SIGTERM, `report.json` tetap ditulis dengan `interrupted: true`, wrapper keluar dengan exit code perintah (143) → Sinyal dan exit code
- [x] Perintah `sh -c 'kill -9 $$'` → `commandSignal: 9`, `commandExitCode: null`, wrapper keluar 137 → Sinyal dan exit code
- [x] Perintah yang mencetak prefix fixture credential → exit 1, satu masalah `credential_leak` tingkat run "fixture credential prefix found in output.log", nilainya tidak dicetak → PRD bagian 27, 32.3
- [x] Record mentah yang ditanam lewat skrip di run fixture → `invalid_record` (JSON rusak, `covers` bukan ID PRD), `kind_mismatch` (record native di run fixture), `duplicate_test` (identitas sama dua kali), `left_not_run` (`NOT_RUN` dengan exit 0), `credential_leak` dengan `recordId` (prefix di dalam record); file `*.tmp` tidak ikut dikumpulkan → Report run
- [x] Record mentah native yang ditanam di run native (skrip, tanpa test dan tanpa spike) → `invalid_record` untuk `PASS` native tanpa subject dan dengan `actual` default, `BLOCKED` tanpa `owner`, dan subject dengan versi selector, digest salah, serta `source: fixture` → Aturan subject, Status

### Guard Rust `evidence!` (file test sementara di `crates/testkit/tests/`, lewat `stackly_testkit::evidence`)

- [x] Run fixture berisi 13 test probe → `pass()` jadi `PASS` "all assertions passed"; `pass_with("observed 42")` jadi `PASS` dengan teks itu; `panic!` jadi `FAIL` "test panicked, see output.log"; tanpa akhir dan `Err` lewat `?` jadi `FAIL` "test ended without pass() or blocked()"; `blocked(reason, owner)` jadi `BLOCKED` dengan keduanya; panic dari drop variabel lain jadi `FAIL`; guard kedua dan dua hasil di satu guard langsung panic (satu record `FAIL`); covers `AC-1` panic tanpa record; `artifact_dir()` memberi `evidencePath` `artifacts/<id>/` dan prefix di file itu jadi `credential_leak` dengan `recordId` → Rust: pola yang benar, Status
- [x] Test probe yang memanggil `std::process::abort()` di biner terpisah → record tetap `NOT_RUN`, `finishedAt` null, wrapper keluar dengan exit code `cargo test` → Status (hasil yang tidak teramati)
- [x] `cargo test` biasa tanpa wrapper → tidak ada record ditulis, tetapi covers `AC-1` tetap panic; `Debug` `FixtureCredential` mencetak `FixtureCredential(****)`; `assert_absent` panic dengan "fixture credential found in probe log" tanpa nilainya → Variabel environment, Fixture credential
- [x] `STACKLY_EVIDENCE_DIR` relatif, folder yang tidak ada, dan `STACKLY_EVIDENCE_KIND` kosong → test langsung gagal dengan pesan yang menyebut variabelnya → Variabel environment
- [x] Test fixture di run native (`--kind native`, hanya test probe) → panic "this test is fixture but the run is native ... #[ignore = \"native: ...\"]" → Rust: pola yang benar
- [x] Test probe yang sama dijalankan dua kali dalam satu run → `duplicate_test` → Identitas test

### Trait Swift `.evidence` (file test sementara di `StacklyTestSupportTests`)

- [x] Run fixture dengan `--filter ZzProbe` → `PASS` "all expectations passed"; `Evidence.actual` memberi `actual` itu; `#expect` gagal dan error yang dilempar jadi `FAIL` dengan teks issue; known issue tetap `PASS`; test berparameter dengan satu case gagal jadi satu record `FAIL`; `Evidence.blocked` jadi `BLOCKED` dan dilaporkan cancelled; `Test.cancel` langsung membuat record tetap `NOT_RUN`; covers `AC-1` mencatat issue tanpa record → Swift: pola yang benar, Status
- [x] Test `kind: .native` di run fixture → dilaporkan skipped dengan alasan "this native test runs only in a native run", tanpa record (perilaku ini dicatat di spec 0002 pada 2026-10-18) → Swift: pola yang benar
- [x] Run native hanya untuk test probe → test fixture mencatat issue dan record `FAIL` plus `kind_mismatch`; test native tanpa subject jadi `invalid_record` → Swift: pola yang benar

### `cargo xtask evidence check`, `evidence new`, dan baseline

- [x] `cargo xtask evidence check` → "0 manual record(s), baseline valid, 98 Rust and 18 Swift file(s) follow the rules" → Aturan pindaian
- [x] File Rust sementara → gagal untuk `#[ignore]` tanpa alasan, `#[ignore = "slow"]`, `#[cfg_attr(unix, ignore)]`, fungsi `#[ignore]` tanpa `evidence!` native, dan `Kind::Native` tanpa `#[ignore]`; fungsi `#[ignore = "native: ..."]` dengan guard native lolos. Pesan menyebut file dan fungsi → Penanda test native
- [x] File Swift sementara → gagal untuk `@Test` tanpa `kind: .native` di folder `*NativeTests` (termasuk atribut yang terpecah beberapa baris), `kind: .native` di luar folder itu, dan `Test.cancel(` di file yang memakai `.evidence(`; atribut native multi baris dan komentar `// kind: .native` lolos → Penanda test native
- [x] `cargo xtask evidence new --covers AC-047 REQ-051 --kind fixture --title "  --Client refuses: another protocol MAJOR!! (über) --" --expected ... --subject postgresql@18.1` → `evidence/manual/2026-10-18-client-refuses-another-protocol-major-ber.json`, `NOT_RUN`, `method: manual`, `evidencePath` file itu sendiri, subject `managed` dengan digest null, `context` dengan `runId`, `runKind`, `command` null; judul yang sama lagi menghasilkan akhiran `-2`; `evidence check` lolos untuk keduanya; file dihapus sesudahnya → Record manual
- [x] Ubah record manual jadi `PASS` tanpa `reviewer`, beri record kedua `id` yang sama dan `evidencePath` lain → `evidence check` gagal dengan tiga pesan (reviewer, id ganda, evidencePath) → Record manual
- [x] `evidence new` tanpa `--kind`, dengan `--covers AC-47`, dan dengan `--subject postgresql@18` → ditolak dengan pesan jelas → Record manual
- [x] `evidence/baseline-host.json` ada dan valid: Mac13,1, Apple M1 Max, 34359738368 byte, 8P dan 2E, macOS 27.0 (26A428), `virtualized: false`, APFS internal SSD di `/System/Volumes/Data` → OQ-011
- [x] `cargo xtask evidence baseline` di Mac ini → hanya `recordedAt` yang berubah; file dikembalikan → OQ-011
- [x] Ubah `virtualized` jadi true → `evidence check` gagal "the baseline host is a real Mac, not a VM"; kembalikan → Baseline benchmark

### Pemisahan run fixture dan run native

- [x] `cargo test --workspace --locked --tests -- --ignored --list` → hanya lima test `native_runtime` (ClickHouse, MySQL 8.4, MySQL 9.7, NATS, Traefik), yang dilewati run fixture → Penanda test native, PRD bagian 32.1
- [x] `cargo xtask runtime check` → "5 artifact(s) and 3 evidence run(s) follow the rules"; ketiga report di `evidence/runtime/` berlabel `native`, dibuat dari `cargo test -p xtask --test native_runtime -- --ignored`, dengan record `PASS` dan subject versi exact → PRD bagian 28, 32.1
- [x] Setelah seluruh suite test, `~/Library/Application Support/Stackly` tidak ada → integration test tidak menyentuh folder Application Support pengguna → PRD bagian 32.1
- [ ] Run native penuh sesuai "Menjalankan test di Mac ini" (`runtime spike --cleanup`, run native Rust, run native Swift, `--cleanup` lagi) → tidak dijalankan di verify 2026-10-18 atas pilihan pemilik proyek, karena sesi fitur 3 step 17 sedang berjalan dan `--cleanup` bisa mematikan spike miliknya. Bukti run native nyata yang sudah ada: tiga run di `evidence/runtime/` → PRD bagian 32.1

## Acceptance-criteria coverage

- REQ-051 (setiap requirement punya test, evidence, dan status): record `PASS` Rust dan Swift lewat wrapper, plus probe guard dan trait untuk setiap status
- OQ-011 (mesin baseline): `evidence/baseline-host.json` valid, `context.baseline.matches` true, dan `evidence baseline` menulis ulang nilai yang sama
- PRD bagian 27 dan 32.3 (artefak disanitasi, fixture credential): pindaian `credential_leak` di `output.log`, `artifacts/`, dan record, serta `FixtureCredential` yang tidak membocorkan nilainya
- PRD bagian 28 (isi evidence record): field record, `context`, `subjects`, dan record manual lewat `evidence new`
- PRD bagian 31 (tooling bersih): fmt, clippy, build, test, swift-format, codegen, deny, check-deps, build-app, dan pre-commit hook
- PRD bagian 32.1 (test root ephemeral, pemisahan fixture dan native): `TestRoot` dan `allocate_port` di integration test, `--ignored --list`, dan run native di `evidence/runtime/`; run native penuh di sesi ini belum dijalankan (lihat langkah yang tidak dicentang)
