# 0002. Evidence test, pemisahan test native, dan baseline benchmark

**Date**: 2026-10-17
**Status**: Accepted

## Summary

Spec ini menutup bagian Done when fitur 2 yang belum diputuskan spec mana pun. Setiap test yang membuktikan ID PRD (AC, REQ, NFR, VAL, atau BR) menulis satu evidence record (catatan bukti) berformat JSON, berlabel `fixture` atau `native`, yang lahir sebagai `NOT_RUN` lalu diganti dengan hasil aslinya. Test native (yang butuh binary provider asli atau Mac macOS 27) ditandai `#[ignore = "native: ..."]` di Rust dan target `*NativeTests` di Swift, lalu hanya jalan di run native (`cargo xtask evidence run --kind native`), yang kamu jalankan dengan tangan di Mac ini. Mac yang sama menjadi mesin baseline benchmark dengan sidik jari (fingerprint, data pengenal hardware) di `evidence/baseline-host.json`, dan fixture credential bersama masuk sekarang lewat crate kecil baru `stackly-evidence`, yang juga diekspor ulang oleh `stackly-testkit`.

## Decision

**Chosen option**: Option 1 di keputusan 1, 2, dan 4, dan Option 2 di keputusan 3: evidence dideklarasikan di dalam test dan dikumpulkan wrapper xtask, `#[ignore = "native: ..."]` khusus untuk test native, Mac ini (Mac pengembangan) sebagai baseline, dan fixture credential tersedia di testkit sekarang.

Test menulis bukti tentang dirinya sendiri lewat satu helper per bahasa, `cargo xtask evidence run` menjadi satu satunya pintu yang menjalankan test sebagai bukti, dan run fixture serta run native tidak pernah tercampur.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang.

## Standard definition

### Istilah

| Istilah | Arti |
| --- | --- |
| PRD ID | ID dengan pola `^(AC\|REQ\|NFR\|VAL\|BR)-[0-9]{3}$`, misalnya `AC-011` atau `NFR-005` |
| `fixture` | Test yang memakai fake, mock, dummy server, fake provider, atau fixture kontrak. Test di filesystem dan proses nyata dengan dummy server, juga test Keychain, tetap `fixture` |
| `native` | Test yang butuh binary provider asli (PostgreSQL, MySQL, NATS, ClickHouse, Valkey, Traefik), atau sesuatu yang hanya ada di Mac macOS 27 (helper yang ditandatangani, persetujuan SMAppService, API yang baru ada di macOS 27). Hanya run native yang menjalankannya |
| `automated`, `manual` | Nilai `method`: hasil dari test yang dijalankan, atau dari reviewer yang mengerjakan langkahnya dengan tangan |
| wrapper | `cargo xtask evidence run`, perintah yang menyiapkan run, menjalankan perintah test, lalu mengumpulkan record |
| run | Satu kali wrapper menjalankan satu perintah test. Satu run punya tepat satu `kind` |
| evidence record | Satu catatan bukti untuk satu fungsi test dalam satu run, atau untuk satu review manual |

### Test mana yang menulis record

- Done when fitur 2 menulis "tiap test yang membuktikan ID PRD menghasilkan evidence record", karena PRD bagian 28 mengikat evidence ke AC.
- Test yang membuktikan minimal satu PRD ID menulis tepat satu record per fungsi test per run. Satu PRD ID boleh dibuktikan banyak test, dan satu test boleh membuktikan banyak ID.
- Test unit biasa tanpa PRD ID (seperti test `allocate_port` dan `TestRoot` hari ini) tidak menulis record.
- Setiap test native wajib menulis record berlabel `native`, karena bukti native adalah yang dicari laporan rilis (fitur 65).

### Crate `stackly-evidence`

Crate daun (leaf crate, tanpa dependency ke crate Stackly lain) baru di `crates/evidence`, karena `stackly-domain` dan `stackly-protocol` tidak boleh bergantung pada crate Stackly mana pun, termasuk `stackly-testkit`.

- Isinya: tipe `EvidenceRecord`, `RunContext`, `RunReport`, dan `BaselineHost`; fungsi aturan validasi; guard `Evidence` dan makro `evidence!`; serta `FixtureCredential`.
- Dependency: `serde`, `serde_json`, `schemars`, `uuid` (fitur `v7` dan `serde`), `jiff` 0.2 (fitur `serde`), dan `tempfile`, semuanya lewat `[workspace.dependencies]`. Fitur `serde` di `uuid` ditambahkan di workspace. Untuk schema, aktifkan fitur `uuid1` dan `jiff02` di `schemars`; kalau nama fitur itu tidak ada di versi yang terkunci, pakai `#[schemars(with = "String")]` di field waktu dan ID.
- `stackly-testkit` bergantung padanya sebagai dependency normal dan mengekspornya ulang (`pub use stackly_evidence as evidence;`), jadi crate yang sudah memakai testkit cukup menulis `stackly_testkit::evidence`.
- Aturan baru di `cargo xtask check-deps`:
  - `stackly-evidence` tidak bergantung pada crate Stackly mana pun, dan tidak punya tokio atau libc sebagai dependency langsung.
  - `stackly-evidence` hanya boleh menjadi dependency dev, kecuali di `xtask` dan `stackly-testkit` (dependency normal).
  - `stackly-domain` dan `stackly-protocol` tetap tidak bergantung pada crate Stackly mana pun, kecuali `stackly-evidence` sebagai dependency dev.
  - Aturan testkit di spec 0001 tidak berubah: tetap hanya dependency dev, dan tidak bergantung pada `stackly-core`.

### Evidence record `stackly.evidence/v1`

JSON UTF-8 dengan nama field camelCase, sama seperti wire IPC. Tipe Rust di `stackly-evidence` adalah sumber kebenarannya. `cargo xtask codegen` menulis JSON Schema lewat schemars ke `schemas/evidence/`, dan `cargo xtask codegen --check` gagal kalau file itu tertinggal. Contoh di `schemas/evidence/examples/` adalah kontrak bersama yang dibaca test Rust dan test Swift, sama seperti `protocols/fixtures/`. Field opsional baru boleh ditambahkan tetap di `v1`; perubahan lain (mengganti arti, menghapus, atau mewajibkan field) menaikkan versi ke `v2`. Satu pengecualian tercatat: `context.ci` dan `runnerName` di baseline dihapus pada 2026-10-17 dan tetap di `v1` (pengecualian dicatat 2026-10-18, alasannya di rationale, Perubahan 2026-10-18).

Kolom Sumber adalah value sourcing: dari mana setiap nilai berasal, supaya build tidak perlu menebak.

| Field | Tipe | Wajib | Sumber | Catatan |
| --- | --- | --- | --- | --- |
| `schema` | string | ya | konstanta | `"stackly.evidence/v1"` |
| `id` | UUID v7 | ya | dibuat guard atau trait saat test mulai, atau `evidence new` untuk manual | juga nama file record mentah |
| `covers` | array PRD ID | ya, minimal 1 | argumen `covers` di test, atau `--covers` di `evidence new` | pola PRD ID, duplikat ditolak |
| `kind` | `fixture` atau `native` | ya | argumen `kind` di test, atau `--kind` | wajib sama dengan `kind` run |
| `method` | `automated` atau `manual` | ya | guard dan trait menulis `automated`, `evidence new` menulis `manual` | |
| `test` | object | ya | lihat Identitas test | `{ language, package, target, name, file, line }` |
| `status` | enum | ya | lihat Status | |
| `expected` | string | ya | argumen `expected` (satu kalimat perilaku), atau `--expected` | |
| `actual` | string atau null | `PASS` dan `FAIL` | Rust: `"all assertions passed"` dari `pass()`, teks dari `pass_with(actual)`, `"test panicked, see output.log"` saat panic, `"test ended without pass() or blocked()"` saat drop tanpa akhir. Swift: `"all expectations passed"`, teks dari `Evidence.actual(_:)`, atau deskripsi issue gagal pertama. Manual: reviewer | `PASS` native wajib memakai nilai yang diamati (`pass_with` atau `Evidence.actual`), bukan nilai default |
| `reason` | string atau null | `BLOCKED`, `DEFERRED` | argumen `blocked`, atau manual | |
| `owner` | string atau null | `BLOCKED` | argumen `blocked`, atau manual | siapa yang bisa membuka blokir |
| `subjects` | array | `PASS` native: minimal 1 | `subject(...)` di test, atau `--subject` | `{ provider, version, digest, source }`. Lihat aturan subject di bawah |
| `steps` | array string atau null | manual | reviewer | automated selalu null, karena perintahnya ada di `context.command` |
| `reviewer` | string atau null | manual dengan status final | reviewer | nama atau handle GitHub |
| `startedAt` | RFC 3339 UTC | ya | jam saat guard atau trait dibuat (jiff `Timestamp::now()`, `Date()` di Swift), atau saat `evidence new` | |
| `finishedAt` | RFC 3339 UTC atau null | status final | jam saat hasil ditulis | |
| `evidencePath` | string | ya | automated: `output.log`, atau `artifacts/<id>/` kalau test memanggil `artifact_dir()`, relatif ke folder run. Manual: path relatif repo, yaitu file record itu sendiri, atau `evidence/manual/files/<id>/` kalau reviewer menyimpan file bukti | `evidence check` memastikan path manual ada |
| `context` | object | di `report.json` dan record manual | wrapper, atau `evidence new` | tidak ada di file record mentah |

Aturan subject:

- `version` harus exact, yaitu cocok dengan `^[0-9]+(\.[0-9]+)+([+-][0-9A-Za-z.]+)?$`. Selector seperti `17`, `@17`, atau `^17` ditolak.
- `digest` boleh null. Kalau terisi, nilainya SHA-256 berupa 64 karakter hex huruf kecil tanpa awalan, sama dengan `sha256` di inventaris spec 0003. `source` bernilai `managed`, `external`, atau `fixture`, dan record `native` tidak pernah memakai `fixture`.
- `PASS` native wajib punya minimal satu subject. `FAIL` dan `BLOCKED` native mencantumkannya kalau runtime sudah diketahui.
- Panggil `subject(...)` segera setelah runtime ditemukan, dengan versi yang dibaca dari binary yang berjalan (output `--version` atau query ke server), tidak pernah dari literal di test. Satu pengecualian tertulis ada di spec 0003: test runtime yang `FAIL` sebelum versinya terbaca memakai versi dari inventaris, supaya record itu tetap terhubung ke digest artefaknya.

### Identitas test

Identitas sebuah test adalah `(language, package, target, name)`. Identitas yang sama dua kali dalam satu run adalah masalah `duplicate_test`.

| Field | Rust | Swift | Manual |
| --- | --- | --- | --- |
| `language` | `rust` | `swift` | `manual` |
| `package` | `env!("CARGO_PKG_NAME")` | null (nama modul sudah unik dalam satu build) | null |
| `target` | `env!("CARGO_CRATE_NAME")` (nama target test) | `test.sourceLocation.moduleName` | null |
| `name` | nama thread libtest, yaitu path test di dalam target (`tests::stop_never_sends_sigkill`). Kalau thread tidak bernama atau bernama `main`, pakai `module_path!()` ditambah `:` dan nomor baris | `test.id` tanpa awalan modul dan tanpa akhiran lokasi file, misalnya `InstanceListTests/clientRefusesAnotherProtocolMajor()` | `--title` |
| `file` | `file!()` | `test.sourceLocation.filePath` | null |
| `line` | `line!()` dari pemanggilan makro | `test.sourceLocation.line` | null |

Wrapper mengubah setiap `file` absolut menjadi path relatif terhadap root workspace (dari `cargo metadata`) saat menulis `report.json`. `stackly-evidence` punya satu test penjaga yang memastikan nama hasil turunan sama dengan path test-nya sendiri, jadi kalau libtest berhenti menamai thread, test itu langsung merah.

### Context run

`RunContext` dihitung wrapper sekali di awal run, ditulis ke `run.json`, lalu disalin ke setiap record di `report.json`. Record manual mengisinya lewat `evidence new`. `sourceCommit`, `sourceDirty` (boolean), `host.os`, dan `host.arch` wajib ada, dan run langsung gagal kalau salah satunya tidak bisa dibaca. Probe lain dijalankan satu perintah per nilai; kalau gagal, nilainya null dan wrapper mencetak satu baris peringatan.

| Field | Sumber | Catatan |
| --- | --- | --- |
| `sourceCommit` | `git rev-parse HEAD` | commit yang benar benar diuji |
| `sourceDirty` | `git -C <root workspace> status --porcelain --untracked-files=normal` tidak kosong, diambil sekali di awal run. Hasilnya sama dengan `git status --porcelain` dengan config default; opsi itu ditulis eksplisit supaya `status.showUntrackedFiles` di config lokal atau global tidak diam diam mematikan hitungan file untracked | file untracked yang tidak di-gitignore ikut dihitung, karena cargo ikut mengompilasi file test, `build.rs`, atau `src/bin` baru yang belum di `git add`. Catatan pribadi di folder repo masuk `.git/info/exclude`. Run dirty tidak pernah menjadi bukti rilis |
| `runId` | UUID v7 dari wrapper | null untuk manual |
| `runKind` | `--kind` wrapper | null untuk manual |
| `command` | argv perintah yang dijalankan wrapper | null untuk manual |
| `host.os`, `host.arch` | `std::env::consts::OS` dan `ARCH` | wajib |
| `host.osVersion`, `host.osBuild` | macOS: `sw_vers -productVersion` dan `sw_vers -buildVersion`. Linux: `uname -r` dan null | |
| `host.hardwareModel`, `host.chip`, `host.memoryBytes` | `sysctl -n hw.model`, `sysctl -n machdep.cpu.brand_string`, `sysctl -n hw.memsize` | null di luar macOS |
| `host.performanceCores`, `host.efficiencyCores` | `sysctl -n hw.perflevel0.physicalcpu` dan `hw.perflevel1.physicalcpu` | null kalau key tidak ada (misalnya di VM) |
| `host.virtualized` | `sysctl -n kern.hv_vmm_present` bernilai 1 | null di luar macOS |
| `baseline` | `host` dibandingkan dengan `evidence/baseline-host.json` | `{ matches, osBuildMatches }`, null kalau file baseline belum ada |
| `toolchain.rustc` | token kedua `rustc --version`, misalnya `1.99.1` | |
| `toolchain.xcode` | dua baris `xcodebuild -version` digabung, misalnya `27.0 (27A266a)` | null tanpa Xcode |
| `toolchain.swift` | angka versi dari `swift --version`, misalnya `6.4` | null tanpa Swift |

Pemetaan ke PRD bagian 28: AC ID ada di `covers`, source commit di `context.sourceCommit`, versi exact provider atau runtime di `subjects[].version`, OS dan arch di `context.host`, test command di `context.command` ditambah `test`, langkah manual di `steps`, expected dan actual di `expected` dan `actual`, timestamp di `startedAt` dan `finishedAt`, evidence path di `evidencePath`, reviewer di `reviewer`, dan label fixture atau native di `kind`.

### Status

Nilainya `NOT_RUN`, `PASS`, `FAIL`, `BLOCKED`, dan `DEFERRED` (PRD bagian 28).

| Perubahan | Kapan |
| --- | --- |
| baru → `NOT_RUN` | guard atau trait dibuat saat test mulai, atau `evidence new` membuat record manual |
| `NOT_RUN` → `PASS` | Rust: `pass()` atau `pass_with()` sudah dipanggil dan thread tidak sedang panic saat guard di drop. Swift: body selesai tanpa issue dengan `isFailure` dan tanpa cancel |
| `NOT_RUN` → `FAIL` | Rust: guard di drop saat thread panic (termasuk panic dari drop variabel lain), atau tanpa `pass()` maupun `blocked()` (misalnya test mengembalikan `Err` lebih awal). Swift: ada issue dengan `isFailure`; error yang dilempar body juga tercatat sebagai issue seperti itu |
| `NOT_RUN` → `BLOCKED` | test memanggil `blocked(reason, owner)` karena prasyarat tidak ada (binary, hardware, signing). Di Swift, `Evidence.blocked` menandai `BLOCKED` lebih dulu, lalu memanggil `Test.cancel(_:)`, jadi test dilaporkan cancelled, bukan passed |
| tetap `NOT_RUN` | proses test mati sebelum hasilnya ditulis (abort, SIGKILL, timeout), atau test Swift di cancel tanpa `Evidence.blocked`. Hasil yang tidak teramati tidak pernah dibaca sebagai `PASS` |
| manual `NOT_RUN` → status final | reviewer mengisi hasilnya. `DEFERRED` hanya boleh di record manual, dengan `reason`, untuk scope yang memang di luar baseline |

Aturan:

- Record dengan status final tidak pernah diubah. Menjalankan ulang berarti run baru dengan record baru, dan review ulang berarti file manual baru. Memberi label ulang dilarang (PRD bagian 28).
- `BLOCKED` wajib punya `reason` dan `owner`, dan tidak menggagalkan run.
- Test dengan `#[should_panic]` tidak memakai guard, karena guard akan mencatat `FAIL`. Buktikan perilakunya lewat `Result` atau `std::panic::catch_unwind`.

### Folder dan file

```text
target/evidence/<runId>/            # STACKLY_EVIDENCE_DIR, tidak masuk git
├── run.json                        # RunContext, ditulis wrapper sebelum perintah jalan
├── records/<id>.json               # record mentah per fungsi test, ditulis guard atau trait, tanpa context
├── artifacts/<id>/                 # log dan file bukti yang sengaja disimpan test
├── output.log                      # salinan stdout dan stderr perintah
└── report.json                     # RunReport, stackly.evidence-report/v1
evidence/                           # masuk git
├── baseline-host.json              # BaselineHost, stackly.evidence-baseline/v1
├── manual/
│   ├── <yyyy-mm-dd>-<slug>.json    # record manual lengkap, dengan context
│   └── files/<id>/                 # screenshot atau file bukti manual, opsional
├── runtime/<runId>/                # bukti G0 runtime, ditulis `cargo xtask runtime record` (spec 0003)
│   ├── report.json                 # RunReport yang difilter ke record PASS dan FAIL dari target test native_runtime
│   ├── output.log                  # salinan output.log run itu yang sudah disanitasi (lihat spec 0003)
│   └── artifacts/<id>/spike-result.json
└── compatibility/<runId>/          # compatibility record, ditulis `cargo xtask runtime compat-record` (spec 0018, ditambah fase 02)
    ├── report.json                 # RunReport yang difilter ke record PASS dan FAIL yang covers nya memuat BR-025
    ├── output.log                  # salinan output.log run itu yang sudah disanitasi
    └── artifacts/<id>/compatibility.json   # stackly.compatibility-evidence/v1
schemas/evidence/                   # hasil codegen, plus contoh bersama yang ditulis tangan
├── record.schema.json
├── run-context.schema.json
├── report.schema.json
├── baseline-host.schema.json
└── examples/*.json
```

Setiap file ditulis atomik (tulis ke `<nama>.tmp` di folder yang sama, lalu rename), jadi pembaca tidak pernah melihat file setengah jadi. Wrapper mengabaikan file `*.tmp` di `records/`.

`evidence/runtime/` dan `evidence/compatibility/` adalah satu satunya evidence otomatis di git, ditulis hanya oleh `runtime record` (`runtime/`) dan `runtime compat-record` (`compatibility/`), divalidasi `runtime check` (spec 0003; folder kedua ditambah fase 02, 2026-10-20, spec 0018). `evidence/runtime/` berisi bukti G0 untuk spec 0003, dan `evidence/compatibility/` berisi compatibility record per provider (BR-025); keduanya bukan bukti rilis. `cargo xtask evidence check` tidak membaca kedua folder ini.

### Report run `stackly.evidence-report/v1`

| Field | Isi |
| --- | --- |
| `schema` | `"stackly.evidence-report/v1"` |
| `runId`, `kind` | sama dengan `context.runId` dan `context.runKind` |
| `wrapperVersion` | `CARGO_PKG_VERSION` milik `xtask` |
| `context` | `RunContext` run ini |
| `startedAt`, `finishedAt` | jam wrapper mulai dan selesai |
| `commandExitCode` | exit code perintah, null kalau perintah mati karena sinyal |
| `commandSignal` | nomor sinyal yang mematikan perintah, atau null |
| `interrupted` | true kalau wrapper menerima SIGINT atau SIGTERM |
| `summary` | `{ total, byStatus, byKind }`, jumlah record per status dan per kind |
| `problems` | array `{ code, recordId, message }`, `recordId` null untuk masalah tingkat run |
| `records` | semua record lengkap, masing masing dengan `context` tertanam, jadi fitur 65 bisa membaca satu record tanpa report-nya |

Kode masalah (`code`) yang menggagalkan run:

| Kode | Arti |
| --- | --- |
| `invalid_record` | record mentah tidak bisa didecode, atau melanggar aturan (pola ID, subject native, format `digest`, `actual` native, `reason` dan `owner` untuk `BLOCKED`). `message` menyebut aturannya |
| `kind_mismatch` | `kind` record berbeda dengan `kind` run |
| `duplicate_test` | identitas test yang sama muncul dua kali |
| `left_not_run` | record masih `NOT_RUN` padahal perintah selesai dengan exit 0 |
| `credential_leak` | prefix fixture credential ditemukan di `output.log`, `artifacts/`, atau record, atau salah satu file itu tidak bisa dipindai |

### Validasi

Validasi memakai decode serde bertipe ditambah fungsi aturan di `stackly-evidence` (`EvidenceRecord::validate`, `BaselineHost::validate`), bukan validator JSON Schema. Field yang tidak dikenal diterima, karena `v1` boleh bertambah, dan field yang sudah dihapus dari `v1` (`context.ci` dan `runnerName` di file lama) diabaikan saat didecode. File schema adalah hasil terbitan untuk pembaca lain (fitur 65, editor, C# nanti). Test di `stackly-evidence` memastikan setiap file di `schemas/evidence/examples/` bisa didecode dan lolos (atau gagal, untuk contoh yang sengaja salah) aturan yang sama.

### Variabel environment

- `STACKLY_EVIDENCE_DIR`: path absolut folder run. Hanya wrapper yang mengisinya. Kalau tidak diisi, guard dan trait tidak menulis apa pun, jadi `cargo test` dan `swift test` biasa tetap seperti sekarang, tetapi argumennya tetap divalidasi (pola ID, `kind`), supaya kesalahan terlihat di laptop. Kalau diisi tetapi relatif, foldernya tidak ada, atau `STACKLY_EVIDENCE_KIND` kosong, test langsung gagal dengan pesan yang jelas (fail fast).
- `STACKLY_EVIDENCE_KIND`: `fixture` atau `native`, diisi wrapper.
- Test tidak pernah mengisi variabel environment (di edition 2024 `set_var` itu `unsafe` dan berlaku global).

### Rust: pola yang benar

```rust
use stackly_testkit::evidence::{evidence, Kind, Source};

#[test]
fn stop_never_sends_sigkill() {
    let ev = evidence!(
        covers = ["AC-011"],
        kind = Kind::Fixture,
        expected = "a normal stop ends the dummy server with SIGTERM only",
    );
    // arrange, act, assert as usual
    ev.pass();
}

#[test]
#[ignore = "native: needs a managed PostgreSQL 18 runtime"]
fn postgres_18_reaches_ready_from_a_managed_runtime() {
    let ev = evidence!(
        covers = ["AC-009"],
        kind = Kind::Native,
        expected = "PostgreSQL 18 reaches ready from the managed runtime",
    );
    // `managed_runtime` is illustrative; feature 18 owns the real lookup.
    let Some(runtime) = managed_runtime("postgresql", "18") else {
        ev.blocked("no managed PostgreSQL 18 artifact on this Mac", "feature 3 owner");
        return;
    };
    ev.subject("postgresql", &runtime.reported_version(), Some(runtime.digest()), Source::Managed);
    // start, probe readiness, assert
    ev.pass_with(&format!("ready after {:?} on port {}", runtime.ready_after(), runtime.port()));
}
```

- `evidence!(covers, kind, expected)` membuat guard `Evidence` dan langsung menulis record `NOT_RUN`. Guard selalu menjadi pernyataan pertama di test, supaya ia di drop paling akhir dan melihat panic dari drop variabel lain.
- `pass()`, `pass_with(actual)`, dan `blocked(reason, owner)` mengambil `&self` dan hanya mencatat hasil yang dimaksud. `Drop` yang menulis record final: `PASS` atau `BLOCKED` sesuai yang dicatat kalau thread tidak sedang panic, selain itu `FAIL`. Memanggil dua di antaranya pada guard yang sama langsung panic.
- `subject(provider, version, digest, source)` menambah satu subject. `artifact_dir()` mengembalikan folder `artifacts/<id>/` (dibuat saat dipanggil). Tanpa `STACKLY_EVIDENCE_DIR`, ia mengembalikan folder sementara yang dihapus saat guard di drop, jadi test tidak perlu bercabang.
- `Evidence` tidak `Send` dan tidak `Sync`, jadi `Drop` selalu jalan di thread test tempat `std::thread::panicking()` bermakna. `Drop` tidak pernah panic; kegagalan menulis file dicetak ke stderr dan membuat record tetap `NOT_RUN`.
- Satu guard per test: guard kedua di thread yang sama langsung panic. Slot thread dibebaskan saat guard di drop.
- `kind` guard yang berbeda dengan `STACKLY_EVIDENCE_KIND` langsung panic, misalnya test fixture yang ikut diberi `#[ignore]` lalu jalan di run native. Pesannya menyebut aturan `#[ignore = "native: ..."]`.
- Test untuk guard itu sendiri memakai `Evidence::with_dir(...)`, konstruktor yang menerima folder secara eksplisit, bukan lewat environment, dan tidak memakai slot thread.
- Test yang mengembalikan `Result` memanggil `pass()` sebelum `Ok(())`. Return lebih awal lewat `?` tidak memanggilnya, jadi tercatat `FAIL`, sama dengan yang dilaporkan libtest.

### Swift: pola yang benar

`StacklyTestSupport` adalah target library baru di package `StacklyClient` (`clients/swift`), yang hanya dipakai target test. Ia menulis format JSON yang sama dengan `Codable` tulisan tangan, dan test-nya mendecode `schemas/evidence/examples/` lewat path relatif dari `#filePath`, seperti fixture kontrak.

Sejak fitur 13 (spec 0013), `StacklyTestSupport` adalah product package `clients/swift`, sehingga test `StacklyUI` juga memakai trait `.evidence`. Integration test Swift yang butuh `stacklyd` membaca path binary dari variabel `STACKLY_DAEMON_BIN`; kalau variabel itu tidak di set atau binary nya tidak ada, test memanggil `Evidence.blocked(reason:owner:)` dengan reason yang menyebut variabel itu dan owner fitur pemilik test, jadi test tercatat `BLOCKED`, bukan `FAIL`. Test tanpa evidence memakai pola yang sama lewat `Test.cancel`.

```swift
import StacklyTestSupport
import Testing

struct InstanceListTests {
    @Test(.evidence(covers: ["AC-047"], expected: "the client refuses a daemon with another protocol major"))
    func clientRefusesAnotherProtocolMajor() async throws {
        // #expect and #require as usual
    }
}

// Only in a test target whose name ends with NativeTests, for example StacklyClientNativeTests.
struct PostgresNativeTests {
    @Test(.evidence(covers: ["AC-019"], kind: .native, expected: "the client sees PostgreSQL 18 ready"))
    func clientSeesPostgres18Ready() async throws {
        // `managedRuntime` is illustrative; feature 18 owns the real lookup.
        guard let runtime = try await managedRuntime("postgresql", "18") else {
            try Evidence.blocked(reason: "no managed PostgreSQL 18 artifact", owner: "feature 3 owner")
        }
        Evidence.subject(provider: "postgresql", version: runtime.reportedVersion, digest: runtime.digest, source: .managed)
        // drive the client, assert
        Evidence.actual("ready on port \(runtime.port)")
    }
}
```

Cara kerja trait (dibuktikan lewat compile spike di Xcode 27, lihat rationale):

- `.evidence(covers:kind:expected:)` memenuhi `TestTrait` dan `TestScoping`. `scopeProvider(for:testCase:)` mengembalikan dirinya hanya saat `testCase` bernilai nil, jadi satu scope dan satu record mencakup seluruh fungsi, termasuk test berparameter (`FAIL` kalau satu case gagal). Default `kind` adalah `.fixture`.
- Di `provideScope`, trait menulis `NOT_RUN`, lalu menjalankan body lewat `IssueHandlingTrait.compactMapIssues { ... }.provideScope(...)` untuk melihat setiap issue. Hanya issue dengan `isFailure` yang dihitung, jadi known issue tidak menggagalkan record.
- Setelah body: `BLOCKED` yang sudah ditandai menang; selain itu ada issue gagal berarti `FAIL`; selain itu `Task.isCancelled` bernilai true berarti record dibiarkan `NOT_RUN`; selain itu `PASS`.
- Record yang sedang berjalan disimpan di `@TaskLocal`, jadi `Evidence.subject`, `Evidence.actual`, dan `Evidence.blocked` bisa menemukannya. Dipanggil di luar test dengan trait ini, ketiganya mencatat issue. State yang berubah ada di `final class` yang memegang `Mutex` dari modul `Synchronization`, karena `Mutex` tidak bisa disalin ke dalam closure yang escaping.
- `Evidence.blocked(reason:owner:)` bertipe `throws -> Never`: ia menandai `BLOCKED`, lalu memanggil `Test.cancel(_:)`. Karena itu ia sah sebagai isi `else` pada `guard`.
- `kind: .native` di luar run native (tanpa `STACKLY_EVIDENCE_KIND=native`) memanggil `Test.cancel` di `prepare(for:)`, sebelum menulis apa pun, jadi test native tidak pernah jalan di laptop atau run fixture tanpa sengaja. Swift Testing melaporkan test itu skipped beserta alasannya, dan tidak ada record. `Test.cancel` di dalam `provideScope` sebelum body jalan tidak dipakai, karena test itu hilang dari output `swift test` (tidak cancelled, tidak skipped, tidak dihitung). `kind: .fixture` di dalam run native mencatat issue, jadi record-nya `FAIL`.
- Test dengan trait ini tidak memanggil `Test.cancel` langsung; pakai `Evidence.blocked`.

### Record manual

- `cargo xtask evidence new --covers <ID>... --kind <fixture|native> --title <judul> --expected <kalimat> [--subject <provider>@<versi>]...` menulis `evidence/manual/<yyyy-mm-dd>-<slug>.json` berstatus `NOT_RUN`, dengan `method: manual`, `evidencePath` menunjuk file itu sendiri, dan `context` dari checkout saat itu (commit, dirty, host, toolchain).
- `--kind` wajib, tanpa default. `--subject` boleh diulang, dengan `source: managed` dan `digest` null; ubah file-nya untuk nilai lain.
- Tanggal memakai UTC. Slug dibuat dari judul: huruf kecil, setiap deret karakter selain huruf ASCII dan angka menjadi satu `-`, `-` di awal dan akhir dibuang, paling panjang 48 karakter. Kalau nama file sudah ada, tambahkan `-2`, `-3`, dan seterusnya.
- `evidence check` menolak dua record manual dengan `id` yang sama (review ulang berarti record baru dari `evidence new`), dan menerima `evidencePath` hanya kalau nilainya file record itu sendiri atau `evidence/manual/files/<id>/` dengan `<id>` milik record itu.
- Reviewer mengerjakan langkahnya pada commit itu, lalu mengisi `steps`, `actual`, `status`, `reviewer`, dan `finishedAt`, plus file bukti opsional di `evidence/manual/files/<id>/` (dan mengubah `evidencePath` ke folder itu).
- `kind: native` untuk review yang memakai runtime provider asli di Mac macOS 27, selain itu `fixture`.
- Record itu masuk git di commit berikutnya. `sourceCommit` tetap commit yang direview. Untuk bukti rilis, commit itu harus ada di `main` atau di tag release candidate (fitur 65 yang memeriksanya).

### Penanda test native

Rust:

- `#[ignore]` hanya untuk test native, ditulis `#[ignore = "native: <apa yang dibutuhkan>"]`. `#[cfg_attr(..., ignore)]` dilarang.
- Setiap fungsi `#[ignore]` memakai `evidence!` dengan `Kind::Native`, dan `Kind::Native` hanya muncul di fungsi `#[ignore]`.
- Test native selalu fungsi `#[test]` biasa, bukan hasil makro lain (`proptest!`, `rstest`), supaya cek statis bisa melihatnya.
- Run fixture menjalankan `cargo test` biasa, yang melewati test ignored. Run native menjalankan `cargo test --tests -- --ignored`, yang hanya menjalankan test ignored. `--tests` wajib di sana, karena tanpa itu `--ignored` juga menjalankan contoh kode doc yang ditandai ` ```ignore ` dan gagal (dicek di Mac ini). `--include-ignored` tidak dipakai di kedua run itu.
- Disarankan (bukan aturan) menaruh test native di `tests/native_<topik>.rs`, supaya kamu bisa memilihnya di laptop dengan `cargo test --test 'native_*' -- --ignored`.

Swift:

- Test native hanya ada di target test yang namanya berakhiran `NativeTests` (misalnya `StacklyClientNativeTests`), dan setiap `@Test` di sana memakai `.evidence(kind: .native)`. `kind: .native` tidak boleh muncul di target lain.
- Run fixture menjalankan `swift test ... --skip 'NativeTests\.'`, dan run native menjalankan `swift test ... --filter 'NativeTests\.'`. ID test Swift diawali nama target, keduanya sudah dicek di Mac ini, dan filter yang tidak cocok dengan apa pun hanya memberi warning dengan exit 0, jadi run native bisa memakai perintah ini sebelum ada test native Swift pertama.

### Menjalankan test di Mac ini

Semua perintah dijalankan dengan tangan di Mac ini (macOS 27, Apple Silicon), dari root repo.

| Langkah | Perintah |
| --- | --- |
| run fixture Rust | `cargo xtask evidence run --kind fixture -- cargo test --workspace --locked --no-fail-fast` |
| run fixture Swift | `cargo xtask evidence run --kind fixture -- swift test --package-path clients/swift --skip 'NativeTests\.'` |
| run native Rust | `cargo xtask evidence run --kind native -- cargo test --workspace --locked --no-fail-fast --tests -- --ignored` |
| run native Swift | `cargo xtask evidence run --kind native -- swift test --package-path clients/swift --filter 'NativeTests\.'` |
| cek | `cargo xtask evidence check`, lalu `cargo xtask runtime check` (spec 0003) |

- Setiap `cargo test` dan `swift test` yang menjadi bukti berjalan lewat wrapper, termasuk package Swift lain (`StacklyUI`) saat mulai punya test. `cargo xtask codegen --check` dan `cargo deny check` tidak menulis evidence, karena keduanya cek tooling tanpa PRD ID.
- `--no-fail-fast` dipakai di setiap `cargo test`, supaya biner test berikutnya tetap jalan dan menulis record setelah satu biner gagal.
- Run native butuh sesi login GUI yang aktif di Mac ini, karena spike runtime spec 0003 memakai domain launchd `gui/<uid>`. Tanpa sesi itu, test runtime tercatat `BLOCKED`, bukan `FAIL`.
- Urutan run native: `cargo xtask runtime spike --cleanup`; kalau pembersihan itu gagal, kedua run native tidak dijalankan. Lalu run native Rust, lalu run native Swift walau run Rust gagal (satu record `FAIL` sudah membuat exit code bukan 0). Terakhir `cargo xtask runtime spike --cleanup` lagi, juga saat run sebelumnya gagal atau dibatalkan.
- Pembersihan kedua menangkap proses yang lepas saat run native dibatalkan, karena worker LaunchAgent bukan child dari perintah test. `--cleanup` keluar dengan kode 0 walau domain GUI tidak ada, dan tidak pernah menyentuh cache unduhan. Cache spike (`~/Library/Caches/dev.stackly.xtask/runtime/`) menetap di antara run.
- *Ditambah fase 02 (2026-10-20, spec 0015 dan 0018):* `--cleanup` juga (a) mencabut semua job `dev.stackly.test.agent.*` di `gui/<uid>` (agent test LaunchAgent spec 0015), tanpa lock spike, dan melewatinya kalau domain GUI tidak ada; dan (b) mengirim TERM lalu KILL ke proses milik user ini yang argv nya memuat `/stackly-test-` (proses sisa test native spec 0018). Karena (b) juga mengenai run test yang sedang berjalan, `--cleanup` hanya dijalankan di titik urutan run native di atas, tidak di tengah run. Semantik lock dan urutan resminya ada di spec 0003 (*Mekanisme LaunchAgent dan pembersihan*). Run native Rust di atas ikut menjalankan test LaunchAgent spec 0015 (`apps/daemon/tests/native_launchd.rs`), karena semua test ignored berjalan di sana; spec 0015 menganggap run native yang kamu mulai sebagai persetujuan untuk test itu.
- *Ditambah fase 02 (2026-10-20, spec 0018):* test native yang mengunduh dari jaringan (`nats_installs_from_the_network`) hanya jalan dengan `STACKLY_NATIVE_NETWORK=1`; tanpa variabel itu test tersebut `BLOCKED` dengan alasan `network run not approved`. Variabel itu diset hanya setelah persetujuan kamu untuk run itu.
- Perintah `--cleanup` dibangun di milestone 2 spec 0003, bukan di build spec ini.
- Cabang Linux yang masih ada (probe `uname -r` di context run, juga path cache Linux dan perintah `runtime` yang jalan di OS apa pun di spec 0003) tetap dipertahankan, tetapi tidak ada lagi yang menjalankannya di Linux; yang menguji kode itu hanya unit test di Mac ini. `cargo deny check` hanya memeriksa graph dependency untuk `aarch64-apple-darwin` dan `aarch64-pc-windows-msvc`.

### Perintah xtask

Argumen diparse dengan tangan seperti task `xtask` lain, tanpa clap.

| Perintah | Yang dikerjakan |
| --- | --- |
| `cargo xtask evidence run --kind <fixture\|native> [--out <folder>] -- <perintah>` | menolak jalan kalau `STACKLY_EVIDENCE_DIR` sudah terisi (tidak ada run bersarang); membuat folder run (default `<target directory dari cargo metadata>/evidence/<runId>/`); menulis `run.json`; menjalankan perintah dengan kedua variabel environment, sambil menyalin stdout dan stderr ke terminal dan `output.log`; mengumpulkan `records/`; melengkapi `context` dan path file; memvalidasi; memindai fixture credential; menulis `report.json` lalu mencetak `evidence report: <path absolut report.json>` sebagai baris terakhir stderr |
| `cargo xtask evidence check` | memvalidasi `evidence/manual/*.json` dan `evidence/baseline-host.json`; memindai source Rust dan Swift dengan aturan di bawah; tidak membaca `evidence/runtime/` dan `evidence/compatibility/`, yang keduanya divalidasi `cargo xtask runtime check` (spec 0003, spec 0018) |
| `cargo xtask evidence new ...` | membuat record manual `NOT_RUN` (lihat Record manual) |
| `cargo xtask evidence baseline` | menulis `evidence/baseline-host.json` dari Mac tempat perintah dijalankan (lihat Baseline benchmark) |
| `cargo xtask codegen` | juga menulis empat schema evidence |

Sinyal dan exit code `evidence run`:

- Wrapper memasang handler SIGINT dan SIGTERM (crate `ctrlc` dengan fitur `termination`). Saat sinyal datang, ia meneruskannya ke perintah sebagai SIGTERM lewat `kill -TERM <pid>` (tanpa `unsafe`), menunggu perintah selesai, lalu tetap menulis `report.json` dengan `interrupted: true`. Di luar Unix, langkah meneruskan sinyal dilewati.
- Exit code: kalau perintah gagal, exit code perintah (atau 128 ditambah nomor sinyal kalau perintah mati karena sinyal); selain itu 1 kalau `problems` tidak kosong; selain itu 0.

Aturan pindaian `evidence check`:

- Source Rust di `crates/`, `apps/`, dan `xtask/` (melewati `target/`) diparse dengan `syn` (fitur `full` dan `visit`, memakai major `syn` yang sudah ada di `Cargo.lock` supaya tidak menambah duplikat di `deny.toml`). Gagal kalau: `#[ignore]` tanpa alasan berawalan `native: `; ada `cfg_attr` yang berisi `ignore`; fungsi `#[ignore]` tanpa `evidence!` berisi `Kind::Native`; atau `evidence!` berisi `Kind::Native` di fungsi tanpa `#[ignore]`.
- Pesan pelanggaran Rust menyebut file dan nama fungsi, bukan nomor baris, karena `syn` tanpa fitur `span-locations` di `proc-macro2` tidak memberi nomor baris.
- Source Swift di `clients/` dan `apps/macos/` dipindai sebagai teks, dengan atribut `@Test(...)` dibaca sampai kurung penutupnya walau terpecah beberapa baris. Baris komentar `//` (termasuk `///`) dilewati. Gagal kalau: `@Test` di folder target `*NativeTests` tanpa `kind: .native`; `kind: .native` di luar folder seperti itu; atau file yang memakai `.evidence(` memanggil `Test.cancel(` langsung.
- Test yang dibuat makro lain tidak terlihat oleh pindaian ini; karena itu test native wajib berupa fungsi `#[test]` biasa.

### Baseline benchmark

- Mesinnya adalah Mac ini, yaitu Mac pengembangan (macOS 27, Apple Silicon). Benchmark fitur 64 jalan di sana.
- Kamu menjalankan `cargo xtask evidence baseline` dengan tangan di Mac ini, lalu commit `evidence/baseline-host.json`.
- File `stackly.evidence-baseline/v1` berisi `recordedAt`, `host` (field yang sama dengan `context.host`), dan `storage` (`{ volume, filesystem, internal, solidState }`, dari `diskutil info -plist` untuk volume yang memuat `std::env::temp_dir()`, tempat test root dan data benchmark ditulis, dengan key `MountPoint`, `FilesystemType`, `Internal`, dan `SolidState`).
- Perintah menolak jalan di luar macOS dan di VM (`kern.hv_vmm_present` bernilai 1).
- Wrapper mengisi `context.baseline.matches` bernilai true kalau `hardwareModel`, `chip`, `memoryBytes`, `performanceCores`, `efficiencyCores`, dan `virtualized` semuanya terisi dan sama, dan `osBuildMatches` terpisah. Nilainya null selama file belum ada.
- PRD mengusulkan Mac Apple Silicon 16 GB (OQ-011). Kalau Mac ini berbeda, file mencatat nilai aslinya dan fitur 64 menyebutnya di samping setiap angka. Mengganti mesin baseline berarti update spec ini, file baseline baru, dan benchmark diulang tanpa membandingkan dengan angka lama.
- Sampai baseline dicatat di Mac ini, file belum ada dan bagian "mesin benchmark baseline tercatat" di Done when tetap terbuka.
- Spec ini hanya memutuskan mesin dan pencatatannya. Angka NFR, skenario, dan penyimpanan hasil benchmark milik fitur 64.

### Fixture credential

- `stackly_evidence::FixtureCredential::new()` (juga lewat `stackly_testkit::evidence`) membuat nilai `stackly-fixture-credential-<32 hex huruf kecil>`, dengan bagian acak dari `Uuid::now_v7().simple()`, jadi berbeda di setiap pemanggilan. Prefixnya konstanta `FIXTURE_CREDENTIAL_PREFIX`.
- `value()` mengembalikan nilainya untuk diteruskan ke kode yang diuji. `Debug` mencetak `FixtureCredential(****)`, supaya pesan assertion tidak membocorkannya.
- `assert_absent(haystack, what)` panic dengan pesan `fixture credential found in <what>` tanpa mencetak nilainya.
- Wrapper memindai `output.log`, semua file di `artifacts/`, dan semua record untuk prefix itu secara streaming (PRD bagian 27, artefak log yang disanitasi). Ini penjagaan terbaik yang bisa dilakukan, bukan jaminan: bentuk yang di encode, dipotong, atau disamarkan sebagian tidak tertangkap. Test root dari `TestRoot` ada di luar folder run dan tidak dipindai, jadi test boleh menyimpan credential di sana dengan sengaja.
- Pindaian itu fail closed: file atau folder yang tidak bisa dibaca juga menjadi masalah `credential_leak` (`cannot scan <path> ...`), karena tidak adanya prefix tidak bisa dibuktikan. Symlink, socket, dan pipe tidak pernah diikuti atau dibuka, jadi tidak ada file di luar folder run yang ikut dipindai dan FIFO tidak bisa membuat wrapper menggantung. File `records/*.tmp` ikut dipindai walau wrapper mengabaikannya saat mengumpulkan record.
- Tidak pernah berisi secret asli, dan test tidak pernah menulis nilainya sebagai literal. Fitur 11 memakai ulang tipe ini untuk test redaction (secrets sentinel di PRD bagian 32.3) dan menambah sisi Swift saat test Swift pertama membutuhkannya.

### Invariant

- Hanya wrapper yang mengisi variabel environment evidence.
- Record final tidak pernah diubah; run atau review baru menulis record baru. Menyalin ke `evidence/runtime/` atau `evidence/compatibility/` hanya boleh membuang record utuh dan menghitung ulang `summary`; isi record yang disalin tetap sama di setiap field yang dikenal; field yang tidak dikenal tidak ikut tersalin.
- Record fixture tidak pernah dihitung sebagai bukti native.
- Hasil yang tidak teramati tetap `NOT_RUN`, tidak pernah `PASS`.
- Evidence tidak pernah berisi credential asli, dan run gagal kalau prefix fixture credential muncul di outputnya.

**Replaces**:

- Test native yang terlihat lulus karena diam diam `return` saat binary tidak ada. Sekarang test itu memanggil `blocked()`.
- `#[ignore]` untuk alasan selain native, misalnya test lambat atau flaky. Test lambat tetap test biasa; test flaky diperbaiki atau dihapus.
- Hasil native yang dilaporkan dari run fixture, atau record yang diberi label ulang.
- Bukti untuk PRD ID yang hanya berupa centang di `verify.md`. Centang itu tetap checklist `/check verify`, tetapi bukti manual untuk PRD ID tinggal di `evidence/manual/`.

**Enforcement**:

- Saat test jalan: guard dan trait menolak ID yang salah, `kind` yang tidak cocok, dan guard ganda (test gagal).
- `cargo xtask evidence run` di setiap run yang menjalankan test: aturan record, `kind`, duplikat, `NOT_RUN` yang tertinggal, dan pindaian credential.
- `cargo xtask evidence check`: aturan `#[ignore]`, test native wajib punya evidence, aturan target `NativeTests`, dan record manual valid.
- `cargo xtask check-deps`: aturan dependency `stackly-evidence`.
- `cargo xtask codegen --check`: schema evidence tidak tertinggal dari tipe Rust.
- Review: versi subject dibaca dari binary yang berjalan, `#[should_panic]` tanpa guard, dan record manual yang sudah final tidak diedit.

**Rollout**: berlaku untuk kode baru mulai sekarang. Hari ini belum ada test yang membuktikan PRD ID (7 test Rust dan 2 test Swift adalah test scaffold dan tooling), jadi tidak ada yang perlu dimigrasi. Urutan build mengikuti pendekatan Journey: satu jalur dibuat lengkap dulu, lalu jalur berikutnya.

1. Jalur Rust dari ujung ke ujung: crate `stackly-evidence` (tipe, aturan, guard, `FixtureCredential`) dan ekspor ulangnya di testkit, aturan baru di `check-deps`, schema lewat `codegen`, `cargo xtask evidence run` dengan `report.json`. Record pertama berasal dari test guard itu sendiri, yang membuktikan REQ-051 (setiap requirement punya test, evidence, dan status).
2. Aturan dan bukti manual: `cargo xtask evidence check` (pindaian Rust dan Swift, record manual), `cargo xtask evidence new`, dan contoh di `schemas/evidence/examples/`.
3. Jalur Swift: target `StacklyTestSupport` dengan trait `.evidence`, test-nya membaca contoh bersama dan membuktikan REQ-051.
4. Kebocoran: pindaian prefix credential di wrapper.
5. Baseline: `cargo xtask evidence baseline`, lalu `evidence/baseline-host.json` dicatat di Mac ini.

**Exceptions**:

- `cargo xtask codegen --check` dan `cargo deny check` tidak menulis evidence.
- Test tanpa PRD ID tidak menulis record.
- UI test XCUITest (XCTest) belum punya helper. Fitur pertama yang menambah XCUITest adalah fitur 68 (spec 0013 memakai Swift Testing dan record manual untuk shell); fitur itu menambah padanan XCTest lewat `/architect` update spec ini. Sampai saat itu, review GUI memakai record manual.
- Fuzz target dan benchmark baru menulis record setelah fitur 63 dan 64 memutuskan field tambahannya (sebagai field opsional `v1`, atau `v2`).

## Consequences

**Positive**:

- Fitur 65 membaca satu format dari lima tempat (`report.json` setiap run, `evidence/manual/`, `evidence/baseline-host.json`, `evidence/runtime/` dari spec 0003, yang hanya berlaku sebagai bukti G0, dan `evidence/compatibility/` dari spec 0018, bukti kompatibilitas BR-025 dan bukan bukti rilis, ditambah fase 02 2026-10-20) dan bisa membedakan fixture dari native tanpa menebak.
- Crash, abort, timeout, atau test yang di cancel tidak pernah terbaca sebagai `PASS`.
- Test native tidak pernah jalan tanpa sengaja di laptop.
- Bocornya fixture credential ke log atau artifact membuat run gagal.
- Test di crate daun (`stackly-domain`, `stackly-protocol`) bisa menulis evidence tanpa melanggar arah dependency.

**Negative / tradeoffs**:

- Setiap test AC mendapat guard di baris pertama dan wajib memanggil `pass()`. Lupa memanggilnya membuat test tercatat `FAIL`. Kesalahannya jelas, bukan diam diam, tetapi tetap mengganggu.
- Nama test bergantung pada libtest yang menamai thread sesuai path test. Itu perilaku implementasi, bukan jaminan yang terdokumentasi; test penjaga membuat perubahannya langsung terlihat, dan fallback-nya path modul ditambah nomor baris.
- `#[ignore]` tidak bisa dipakai untuk keperluan lain.
- Mac ini juga mesin kerja harian dengan beban latar, jadi angka benchmark-nya berisik.
- Workspace bertambah satu crate (`stackly-evidence`) dan `xtask` bertambah `syn` serta `ctrlc`.
- `Codable` Swift tulisan tangan menduplikasi tipe Rust, dan keduanya hanya dijaga sama oleh contoh bersama.
- Trait Swift bergantung pada `TestScoping`, `IssueHandlingTrait`, dan `Test.cancel` (ada di Swift 6.4 dan Xcode 27), jadi `StacklyTestSupport` tidak bisa dibangun dengan Xcode yang lebih lama. Test berparameter hanya punya satu record per fungsi, karena `Test.Case` tidak punya ID publik.
- Pindaian credential hanya menangkap prefix yang utuh.
- `sourceDirty` ikut menghitung file untracked yang tidak di-gitignore, jadi catatan pribadi di folder repo membuat run dirty sampai dimasukkan ke `.git/info/exclude`.
- Baseline belum tercatat sampai `cargo xtask evidence baseline` dijalankan di Mac ini.

**Neutral**:

- Folder baru `evidence/`, `schemas/evidence/`, dan `crates/evidence/`, serta target baru `StacklyTestSupport`. Spec 0003 menambah `evidence/runtime/` dengan format report yang sama, dan spec 0018 menambah `evidence/compatibility/` (fase 02).
- `cargo-nextest` tetap tidak dipakai. Laporan JUnit-nya bisa menambah data untuk test tanpa PRD ID nanti, tanpa mengubah format record.

## Follow-up

- [x] Jalankan `cargo xtask evidence baseline` di Mac ini, lalu commit `evidence/baseline-host.json`. Dicatat 2026-10-17 (Mac13,1, Apple M1 Max, 32 GB, macOS 27.0 26A428).
- [ ] Kalau memori Mac ini bukan 16 GB, catat bedanya bersama OQ-011 saat fitur 64 menetapkan angka NFR.
- [x] `/scope`: ubah "tiap test menghasilkan evidence record" di Done when fitur 2 menjadi "tiap test yang membuktikan ID PRD menghasilkan evidence record".
- [ ] Fitur 65: tandai `NOT_RUN` untuk AC yang tidak punya record sama sekali; terima bukti rilis hanya dari commit yang ada di `main` atau tag release candidate; simpan evidence rilis; dan cek bahwa record manual final tidak diedit, dibandingkan dengan riwayat git.
- [x] Test native pertama yang menjalankan provider asli (fitur 3): hentikan proses sisa run sebelumnya sebelum run native. Dipegang spec 0003 AC-4 lewat `cargo xtask runtime spike --cleanup` (milestone 2 spec itu), yang dijalankan sebelum dan sesudah run native (lihat Menjalankan test di Mac ini).
- [ ] `--cleanup` hanya membersihkan proses dan LaunchAgent milik spike runtime. Fitur pertama yang menambah test native yang menjalankan provider di luar spike (misalnya fitur 18 lewat daemon dan supervisor) menambah pembersihan untuk proses sisanya.
- [ ] Helper evidence untuk XCTest saat XCUITest pertama datang (fitur 68), lewat `/architect` update spec ini.
- [ ] Fitur 11: sisi Swift dari `FixtureCredential`, dan test redaction yang memakai prefix yang sama.
- [ ] Fitur 63 dan 64: field tambahan untuk hasil fuzz dan angka benchmark; fitur 64 juga menenangkan Mac (tanpa build lain) sebelum mengukur.
- [ ] ID risiko PRD di evidence (dicatat 2026-10-20 dari test fitur 10, keputusan lead): pola PRD ID spec ini (`^(AC|REQ|NFR|VAL|BR)-[0-9]{3}$`, yaitu `PRD_PREFIXES` di `crates/evidence/src/rules.rs` dan `prdPrefixes` di `clients/swift/Sources/StacklyTestSupport/Rules.swift`) tidak menerima ID risiko, yang di PRD berbentuk `RISK-NN` dua digit (`RISK-01` sampai `RISK-14`). Jadi guard yang diminta spec 0010 membuktikan RISK-03 dan RISK-14 (*Menjalankan test di Mac ini* spec 0010) menulis ID itu di awal teks `expected` beserta judul pendeknya, misalnya `RISK-03 (two servers on one data folder): ...`, dan `covers` nya memuat AC atau REQ terdekat yang ikut dibuktikan, karena setiap record butuh paling sedikit satu PRD ID yang valid. Sampai update spec ini memutuskan lain, itu aturannya untuk ID risiko; kode tidak berubah. Kalau laporan rilis fitur 65 perlu menghitung bukti per risiko, update berikutnya menambah `RISK` dengan pola dua digit di Rust dan Swift beserta test aturannya dalam satu perubahan, lalu memindahkan ID itu dari `expected` ke `covers`.
- [ ] Setelah build, `/sync` mencatat perintah `cargo xtask evidence ...`, aturan `#[ignore]` khusus native, pola guard, dan crate `stackly-evidence` di root `AGENTS.md`, serta `StacklyTestSupport` di `clients/swift/AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan, alasan, dan hasil pengecekan lokal ada di [rationale.md](rationale.md).
