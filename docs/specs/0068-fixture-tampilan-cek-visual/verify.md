# Verify: Fixture tampilan dan cek visual · spec 0068 · updated 2026-10-21

_Langkah diturunkan dari acceptance criteria spec 0068. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27._

## Milestone 2: XCTest evidence helper

### UI / manual

- [ ] Buka `clients/swift/Package.swift` → product `StacklyClient`, `StacklyTestSupport`, dan `StacklyXCTestSupport`; target internal `StacklyEvidenceCore` tidak punya product; `StacklyTestSupport` dan `StacklyXCTestSupport` sama sama bergantung pada `StacklyEvidenceCore`; `StacklyClient` tetap tanpa dependency → AC-10
- [ ] Buka `clients/swift/Sources/StacklyEvidenceCore/` → `Record.swift`, `Rules.swift`, `RunPlan.swift`, `RecordFile.swift`, dan `Finish.swift` ada di sana dan tidak lagi di `Sources/StacklyTestSupport/`; semua deklarasi selain `EvidenceKind` dan `EvidenceSource` berakses `package`; `Evidence.Kind` dan `Evidence.Source` di `StacklyTestSupport` adalah typealias dari keduanya, jadi `apps/macos/Packages/StacklyUI` tidak perlu diubah → AC-10
- [ ] Jalankan satu test XCTest hidup dengan `STACKLY_EVIDENCE_DIR=$(mktemp -d)` dan `STACKLY_EVIDENCE_KIND=fixture` (misalnya tambah sementara satu method yang memanggil `try evidence(covers: ["AC-024"], expected: "x")` lalu `XCTAssertEqual(1, 2)`, jalankan `swift test --package-path clients/swift --filter <kelas>`, kembalikan sesudahnya) → `records/<uuid>.json` berstatus `FAIL` dengan `actual` berisi `XCTAssertEqual failed`; method yang memanggil `try XCTEvidence.blocked(reason: "r", owner: "o")` menghasilkan `BLOCKED` dengan `reason` `r` dan `owner` `o`; method dengan `covers: ["bad"]` gagal tanpa record; dengan `STACKLY_EVIDENCE_KIND=native` method yang lulus menjadi `FAIL` dengan `actual` "this test is fixture but the run is native: ..." → AC-10

### Commands

- [ ] `swift test --package-path clients/swift` → semua lulus: test Swift Testing `StacklyTestSupportTests` tetap lulus tanpa perubahan perilaku (hanya `@testable import StacklyEvidenceCore` ditambah di file test nya), dan 28 test XCTest `StacklyXCTestSupportTests` lulus → AC-10
- [ ] `swift test --package-path clients/swift --filter XCTEvidenceMappingTests` → 18 test lulus: satu test per baris pemetaan (PASS dengan `actual` bawaan dan dengan `XCTEvidence.actual`, FAIL dari kegagalan dan dari exception tak terduga dengan `actual` issue pertama, FAIL tanpa issue terbaca, issue yang bukan kegagalan tetap PASS, BLOCKED dengan reason dan owner, BLOCKED menang atas kegagalan, skip tanpa `blocked` tetap `NOT_RUN`, kegagalan menang atas skip), isi record yang tidak berubah, dan nama test (`AndReturnError:`, `WithCompletionHandler:`, kelas bersarang, nama tanpa bentuk `-[Kelas selector]`, modul dari `String(reflecting:)`) → AC-10
- [ ] `swift test --package-path clients/swift --filter XCTEvidenceRunTests` → 5 test lulus lewat XCTest sungguhan: record `PASS` dengan `kind` `fixture`, `method` `automated`, `test.target` `StacklyXCTestSupportTests`, `test.name` `Probe/passes()`; `actual` dari `XCTEvidence.actual`; `BLOCKED` dengan reason dan owner; skip tanpa `blocked` meninggalkan `NOT_RUN` tanpa `finishedAt`; tanpa run tidak ada file yang ditulis → AC-10
- [ ] `swift test --package-path clients/swift --filter XCTEvidenceTrackerTests` → 5 test lulus: hanya issue pertama disimpan, `begin` kedua untuk test yang sama ditolak, `actual` dan `blocked` ditolak saat tidak ada test terlacak, panggilan berikutnya mengganti yang lama, dan `finish` melupakan test → AC-10
- [ ] `cargo test -p xtask evidence::swift_scan` → test aturan baru lulus: UI test yang memblokir lewat helper lulus; `XCTSkip(`, `XCTSkipIf(`, dan `XCTSkipUnless (` langsung di file yang memanggil `evidence(covers:` gagal dengan nomor baris; `kind:` di argumen `evidence` (juga multi baris) gagal; file `*UITests.swift` tanpa panggilan `evidence(covers:` boleh `XCTSkip`; baris komentar dan nama yang lebih panjang (`MyXCTSkip(`) tidak dihitung; trait `.evidence(` Swift Testing bukan panggilan; aturan hanya berlaku di `apps/macos/` pada file yang namanya berakhiran `UITests` (folder `StacklyUITests` tidak ikut) → AC-11
- [ ] `cargo xtask evidence check` → lulus pada pohon saat ini (aturan baru tidak mengubah file yang ada) → AC-11
- [ ] `cargo clippy -p xtask --all-targets --locked -- -D warnings` dan `cargo test -p xtask` → hijau → AC-11
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI` (setelah `cargo build -p stacklyd -p stackly-fake-server` dan `cargo build -p stacklyd --example skeleton_seed`) → semua lulus; pemakai trait `.evidence` tidak terdampak oleh pemindahan target → AC-10
- [ ] `cargo xtask check-deps` → lulus → AC-10, AC-11

### Acceptance-criteria coverage

- AC-10: step "Buka Package.swift", "Buka StacklyEvidenceCore", pemakaian hidup, `swift test` penuh, `XCTEvidenceMappingTests`, `XCTEvidenceRunTests`, `XCTEvidenceTrackerTests`, dan `StacklyUI` · pemakaian dari bundle UI test sungguhan (`StacklyAppUITests`) menunggu milestone 3 (langkah 10 sampai 14)
- AC-11: `cargo test -p xtask evidence::swift_scan` dan `cargo xtask evidence check` · pindaian atas file `*UITests.swift` sungguhan menunggu milestone 3, karena file itu belum ada
