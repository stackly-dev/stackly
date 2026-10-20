# 0002. Rationale: evidence test, pemisahan test native, dan baseline benchmark

[Kembali ke spec](index.md)

## Context

Fitur 2 sudah memasang format, lint, deny, pre commit hook, serta `TestRoot` dan `allocate_port` di `stackly-testkit`. Done when-nya masih menyisakan empat hal yang belum diputuskan spec mana pun: evidence record yang dihasilkan setiap test, pemisahan run fixture dari run runtime nyata, mesin benchmark baseline, dan fixture credential. Workspace ini ada di monorepo tunggal, dan keputusannya berlaku untuk seluruh repo (Rust lewat `cargo test`, Swift lewat `swift test`), bukan satu crate.

PRD bagian 28 menetapkan isi evidence record: AC ID, source commit, versi exact provider atau runtime, OS dan arch, test command atau langkah manual, expected dan actual, timestamp, evidence path, dan reviewer bila manual. Status awal setiap AC adalah `NOT_RUN`, dan laporan fixture harus dibedakan dari laporan integrasi runtime nyata. Fitur 65 (quality gates dan release report) mengagregasi "format laporan dari fitur 2", jadi format ini menjadi kontrak yang dibaca belakangan. Kesalahan di sini akan muncul sebagai laporan rilis yang tidak bisa membedakan bukti asli dari bukti fixture, atau yang menganggap test yang crash sebagai lulus.

Batasan teknisnya: spec 0001 memilih `cargo test` tanpa nextest, dan output JSON libtest masih butuh flag unstable. Integration test berbagi proses dan dilarang memakai state global; di edition 2024, `std::env::set_var` bersifat `unsafe`. `cargo xtask check-deps` melarang `stackly-domain` dan `stackly-protocol` bergantung pada crate Stackly mana pun, termasuk sebagai dependency dev, padahal invariant domain (BR) dan kontrak protokol adalah bukti yang paling awal ditulis. Swift memakai Swift Testing untuk unit test dan XCUITest untuk UI test. Belum ada satu pun test native, dan belum ada test yang membuktikan PRD ID.

PRD bagian 27 meminta integration test memakai test root ephemeral, port yang dialokasikan, fixture credential, dan artefak log yang disanitasi. PRD mengusulkan baseline performa berupa Mac Apple Silicon 16 GB (OQ-011, sebelum beta). Fitur 11 (secret store) memiliki sentinel credential untuk test redaction, tetapi fitur 5 dan 6 datang lebih dulu dan sudah menyentuh secret reference dan pengiriman credential lewat IPC.

## Options considered

### Keputusan 1: cara test menghasilkan evidence record

#### Option 1: Dideklarasikan di dalam test, dikumpulkan wrapper xtask

Test Rust membuat guard `evidence!` dari crate test kecil (akhirnya `stackly-evidence`, diekspor ulang oleh `stackly-testkit`), test Swift memakai trait `.evidence`. Keduanya menyebut PRD ID yang dibuktikan dan menulis satu file JSON per fungsi test. `cargo xtask evidence run` menyiapkan run, menjalankan perintah test, lalu mengumpulkan file itu menjadi `report.json`.

**Pros**:

- Jalan di Rust stable tanpa tool baru.
- Hubungan test ke AC ada tepat di samping assertion, jadi ikut berubah saat test berubah.
- Satu format untuk Rust dan Swift.

**Cons**:

- Setiap test AC mendapat baris tambahan dan wajib diakhiri `pass()`.
- Nama test di Rust bergantung pada nama thread libtest.

#### Option 2: Mengonversi laporan runner

cargo-nextest menulis JUnit untuk setiap test, `swift test --xunit-output` menulis xUnit, dan xtask memetakan nama test ke AC lewat file pemetaan.

**Pros**:

- Setiap test tercatat tanpa mengubah kode test.
- Hasil pass atau fail datang langsung dari runner.

**Cons**:

- Menambah nextest, yang tidak dipilih spec 0001.
- Pemetaan ke AC tinggal jauh dari test dan mudah basi saat test diganti nama.
- Field seperti subject provider dan alasan `BLOCKED` tetap butuh jalur lain dari dalam test.

#### Option 3: Keduanya

Laporan runner untuk setiap test, plus deklarasi di dalam test untuk hubungan ke AC.

**Pros**:

- Paling lengkap: semua test tercatat, dan test AC membawa detail.

**Cons**:

- Dua mekanisme yang harus dijaga tetap sejalan.
- Paling banyak yang dibangun untuk fitur yang hari ini belum punya satu pun test AC.

Pilihan turunan yang dijawab di keputusan ini:

- **Test mana**: hanya test yang membuktikan PRD ID (dipilih), atau setiap test. Setiap test memberi fitur 65 banyak record tanpa ID yang harus disaring.
- **Penyimpanan**: satu file JSON per test di `target/evidence/<runId>/` (dipilih), satu file JSON Lines per run (butuh lock karena test paralel dan biner terpisah menulis bersamaan), atau commit ke git (setiap run mengubah file, dan bukti satu commit selalu masuk di commit lain).
- **Record manual**: JSON di `evidence/manual/` yang dicommit (dipilih), tabel Markdown di `docs/evidence/` (gampang ditulis, rapuh diparse), atau di `verify.md` (checklist, bukan record dengan commit, host, dan reviewer).
- **Arti `NOT_RUN`**: ditulis saat test mulai lalu diganti di akhir (dipilih), atau hanya dihitung belakangan oleh fitur 65 untuk ID tanpa record (test yang crash tidak meninggalkan jejak).

### Keputusan 2: penanda dan pemilihan test native

#### Option 1: `#[ignore = "native: ..."]` khusus native

`#[ignore]` dipakai hanya untuk test native, alasannya wajib diawali `native: `, run fixture melewatinya dengan sendirinya, dan run native menjalankan `-- --ignored`.

**Pros**:

- Fitur bawaan libtest, tanpa rebuild dan tanpa flag.
- Alasan ignore tampil di output test.
- Test native tidak pernah jalan tanpa sengaja di laptop.

**Cons**:

- `#[ignore]` tidak bisa dipakai untuk keperluan lain.
- Aturan prefix butuh cek statis tambahan di xtask.

#### Option 2: `--cfg stackly_native`

`#[cfg_attr(not(stackly_native), ignore)]`, dan run native mengisi `RUSTFLAGS`.

**Pros**:

- `#[ignore]` tetap bebas untuk keperluan lain.

**Cons**:

- `RUSTFLAGS` memaksa rebuild penuh dan cache terpisah.
- Butuh entri `check-cfg` di lints workspace.

#### Option 3: Cargo feature `native`

Setiap crate punya feature `native`, dan target test native memakai `required-features`.

**Pros**:

- Eksplisit di `Cargo.toml`.

**Cons**:

- Feature menyebar ke setiap crate dan menyatu (feature unification) di build workspace, jadi mudah ikut aktif tanpa sengaja.

#### Option 4: Penamaan file saja

Test native ada di `tests/native_*.rs` dan dipilih dengan `--test 'native*'`.

**Pros**:

- Mudah dilihat di struktur folder.

**Cons**:

- Run fixture harus mengecualikannya lewat nama, dan Rust tidak bisa melewati test saat runtime, jadi pengecualian yang terlewat langsung menjalankannya.

Pilihan turunan:

- **Garis native**: runtime provider asli atau fasilitas yang hanya ada di macOS 27 (dipilih); setiap fasilitas OS nyata; atau hanya binary provider asli.

### Keputusan 3: mesin benchmark baseline

#### Option 1

_Dihapus 2026-10-17: CI ditiadakan, pengembangan fokus di Mac ini dulu._

#### Option 2: Mac yang dipakai sesi ini

Mac Studio `Mac13,1`, Apple M1 Max, 32 GB, macOS 27.0.

**Pros**:

- Tersedia sekarang.

**Cons**:

- Mesin kerja harian dengan beban latar, jadi angkanya berisik.
- Memorinya dua kali usulan PRD.

#### Option 3: Mac terpisah 16 GB

**Pros**:

- Paling dekat dengan usulan PRD dan Mac minimum pengguna.

**Cons**:

- Mesinnya harus diadakan sebelum beta, dan jadi mesin kedua yang dirawat.

Pilihan turunan untuk tempat pencatatan: `evidence/baseline-host.json` yang dibandingkan otomatis dengan host setiap run (dipilih), atau hanya tabel di spec (tidak bisa dicek tool).

### Keputusan 4: fixture credential bersama

#### Option 1: Masuk `stackly-testkit` sekarang (diwujudkan di `stackly-evidence`, diekspor ulang oleh testkit)

**Pros**:

- Fitur 5 dan 6 sudah bisa memakainya sebelum fitur 11 ada.
- Wrapper bisa memindai kebocoran prefix sejak hari pertama.

**Cons**:

- Kode yang belum dipakai test PRD mana pun masuk lebih dulu, walau kecil.

#### Option 2: Menunggu fitur 11

**Pros**:

- Sentinel didesain bersama redaction, di tempat yang memilikinya.

**Cons**:

- Fitur 5 dan 6 harus menunggu atau membuat versinya sendiri, dan pindaian kebocoran tertunda.

## Rationale

**Keputusan 1.** Kekuatan yang paling menentukan adalah fitur 65: ia harus tahu test mana membuktikan AC mana, pada commit dan host apa, dengan versi runtime apa, dan apakah hasilnya fixture atau native. Laporan runner (Option 2) hanya menjawab "lulus atau gagal"; sisanya tetap harus datang dari dalam test, jadi Option 2 dan 3 membangun dua mekanisme untuk menjawab satu pertanyaan. Option 1 menaruh semua jawaban di satu tempat, jalan di Rust stable tanpa nextest (sesuai spec 0001), dan membuat hubungan ke AC berubah bersama test-nya. Menulis `NOT_RUN` saat test mulai adalah cara paling sederhana untuk menepati PRD bagian 28 dengan jujur: test yang crash atau kena timeout meninggalkan record yang jelas belum punya hasil, bukan kekosongan yang bisa disalahartikan. Satu file per test menghindari lock antar thread dan antar biner, dan evidence otomatis tidak masuk git karena PRD meminta bukti dijalankan ulang per commit, bukan dibawa dari commit lain. Record manual masuk git karena wrapper tidak bisa mengulang hasil kerja manusia.

**Keputusan 2.** Rust tidak bisa melewati test saat runtime, jadi setiap mekanisme yang memilih test native lewat nama atau feature punya mode gagal yang sama: test native jalan diam diam di tempat yang salah. `#[ignore]` membalik default itu, karena test native tidak jalan kecuali diminta. Biayanya, `#[ignore]` hanya boleh berarti native, dan itu justru menutup kebiasaan buruk (mengabaikan test flaky). Garis native ditarik di runtime provider asli dan fasilitas khusus macOS 27, sedangkan test filesystem, proses, dan Keychain tetap fixture.

**Keputusan 3 dan 4.** Mac ini dipilih karena pengembangan fokus di Mac ini dulu dan ia tersedia sekarang. Bebannya sebagai mesin kerja harian membuat angkanya berisik, jadi fitur 64 menenangkan Mac sebelum mengukur. Sidik jari di file JSON membuat setiap laporan bisa menyatakan sendiri apakah ia jalan di baseline, jadi angka dari laptop tidak tercampur dengan angka baseline. Fixture credential masuk sekarang karena Done when fitur 2 sendiri menyebutnya, fitur 5 dan 6 membutuhkannya lebih dulu dari fitur 11, dan pindaian prefix di wrapper adalah cara paling murah untuk menepati "artefak log yang disanitasi" sejak hari pertama.

### Keputusan kecil yang diambil di spec ini

| Hal | Pilihan | Alasan | Runner up |
| --- | --- | --- | --- |
| Rumah tipe record | crate daun baru `stackly-evidence`, diekspor ulang oleh testkit, dan menjadi dependency normal `xtask` | crate daun boleh menjadi dependency dev `stackly-domain` dan `stackly-protocol` tanpa membawa crate Stackly lain, jadi test BR dan kontrak bisa menulis evidence | modul di testkit dengan pengecualian untuk `xtask`. Pilihan ini ada di draf pertama, lalu dibuang setelah cross check menunjukkan crate daun tidak bisa memakai testkit |
| Bentuk record | satu tipe; file mentah tanpa `context`, wrapper melengkapinya di `report.json`, dan setiap record di report membawa `context` sendiri | context dihitung sekali per run, dan fitur 65 bisa membaca satu record tanpa report-nya | setiap test membaca context dari environment, lebih banyak variabel dan lebih lambat |
| Validasi | decode serde bertipe ditambah fungsi aturan, field tak dikenal diterima | spec 0001 menyerahkan validator JSON Schema ke fitur 9, dan aturan seperti "subject native wajib" lebih jelas sebagai kode | crate validator JSON Schema di `xtask` |
| Penulis di Swift | `Codable` tulisan tangan di target `StacklyTestSupport` dalam package `StacklyClient`, dijaga contoh bersama | typeshare hanya untuk kontrak IPC, dan satu target lebih ringan dari satu package | package lokal terpisah `clients/swift-testkit` |
| Nama test Rust | nama thread libtest, lalu `module_path!()` dan baris, dijaga test penjaga | tanpa argumen tambahan, sudah dicek di 1.99.1, dan perubahan libtest langsung terlihat | argumen `name` wajib di makro |
| Akhir test Rust | `pass(&self)` hanya mencatat, `Drop` menulis status final, guard di baris pertama | `pass(self)` yang langsung menulis akan mencatat `PASS` walau drop variabel lain sesudahnya panic; test yang mengembalikan `Err` tidak panic, jadi "lulus kalau tidak panic" juga salah | `pass(self)` yang langsung menulis, dengan aturan "pernyataan terakhir" |
| Native di Swift | target `*NativeTests` plus trait yang membatalkan test di luar run native | ID test Swift diawali nama target, jadi `--filter` dan `--skip` cukup, dan trait mencegah jalan tanpa sengaja | tag Swift Testing, yang belum bisa dipilih langsung dari `swift test` |
| Test berparameter di Swift | satu record per fungsi, lewat `scopeProvider` yang hanya aktif saat `testCase` nil | `Test.Case` tidak punya ID publik, jadi record per case tidak punya nama yang stabil | satu record per case dengan nomor urut, yang berubah antar run |
| Test Swift yang di cancel | `Evidence.blocked` menandai `BLOCKED` sebelum `Test.cancel`; cancel tanpa itu dibiarkan `NOT_RUN` | spike menunjukkan cancel tidak terlihat sebagai issue maupun error, hanya lewat `Task.isCancelled` | membaca event stream `swift test` di wrapper, yang menambah satu format lagi untuk dijaga |
| `sourceDirty` (ditinjau ulang setelah spec 0003) | `git status --porcelain` biasa (ditulis dengan `--untracked-files=normal` supaya config tidak mengubahnya), wajib boolean | cross check kedua menunjukkan `--untracked-files=no` membuat file test baru yang belum di `git add` ikut dikompilasi tanpa membuat run dirty, sehingga bukti G0 bisa berasal dari kode yang tidak ada di commit. File yang di-gitignore sudah tidak dihitung, dan catatan pribadi cukup masuk `.git/info/exclude` | `--untracked-files=no` (sempat dipilih lalu dibatalkan), atau menghitung file untracked hanya di folder build, yang lebih rumit dijaga |
| Sinyal di wrapper | `ctrlc` mencatat sinyal, wrapper meneruskannya lewat `kill -TERM`, lalu tetap menulis report | `unsafe` dilarang di `xtask`, dan report yang hilang saat perintah dibatalkan membuat run sulit dibaca | tanpa handler, dan run tanpa `report.json` dianggap tidak lengkap |
| Cek `#[ignore]` | `syn` di `cargo xtask evidence check` | atribut bisa terpecah beberapa baris, dan regex mudah salah | regex per baris |
| `BLOCKED` di run | dilaporkan, bukan run gagal | `BLOCKED` adalah fakta yang dilaporkan, dan fitur 65 yang memutuskan dampaknya pada rilis | run gagal pada `BLOCKED` |
| Nilai acak credential | `Uuid::now_v7().simple()` | `uuid` dengan fitur `v7` sudah ada di workspace | crate `getrandom` langsung |
| Kecocokan baseline | model, chip, memori, dan jumlah core harus sama; build OS dicatat terpisah | build OS berubah lebih sering dari hardware, dan bedanya perlu terlihat tanpa membatalkan kecocokan | build OS ikut wajib sama |

### Hasil pengecekan lokal

Dicek di Mac sesi ini (Mac Studio `Mac13,1`, Apple M1 Max, 34359738368 byte memori, macOS 27.0 build 26A428, Xcode 27 dengan Swift 6.4) pada 2026-10-17.

- **Nama thread libtest**: crate percobaan mencetak `thread=Some("plain_one")` dan `thread=Some("inner::nested")`, sama saja dengan atau tanpa `--test-threads=1`, baik dengan cargo 1.97.0 maupun dengan toolchain yang dipin repo (`rustup run 1.99.1`, cargo 1.99.1).
- **`--ignored` dan contoh doc**: dengan 1.99.1, `cargo test -- --ignored` ikut menjalankan contoh doc ` ```ignore ` dan gagal (`src/lib.rs - add_one (line 5) --- FAILED`), sedangkan `cargo test --tests -- --ignored` tidak.
- **Alasan ignore**: output test menampilkan `test: native_one, ignore_message: native: needs PostgreSQL`.
- **Glob target test**: `cargo test --test 'native*' -- --ignored` menjalankan tepat satu test native.
- **Swift Testing di Xcode 27**: interface `Testing.swiftmodule` memuat `protocol TestScoping`, `IssueHandlingTrait` dengan `compactMapIssues` dan `filterIssues`, `Test.cancel(_:)`, `Issue.isFailure`, dan `ConditionTrait.enabled(if:)`.
- **ID test Swift**: `swift test --package-path clients/swift list` mencetak `StacklyClientTests.ProtocolVersionFixtureTests/encodedValueDecodesLikeTheFixture()` dan `StacklyClientTests.ProtocolVersionFixtureTests/fixtureDecodesToCurrentVersion()`.
- **Filter tanpa hasil**: `swift test --package-path clients/swift --filter 'NativeTests\.'` mencetak `warning: No matching test cases were run` dan exit 0.
- **Environment ke test Swift**: package percobaan dengan `#expect(ProcessInfo.processInfo.environment["STACKLY_PROBE"] == "yes")` lulus saat dijalankan dengan `STACKLY_PROBE=yes swift test`.
- **Aturan crate daun di `check-deps`**: `xtask/src/check_deps.rs` menyimpan `LEAVES = ["stackly-domain", "stackly-protocol"]` dan menolak dependency apa pun ke crate Stackly untuk keduanya, dan menolak `stackly-testkit` dengan jenis selain dev untuk semua paket.
- **Spike trait Swift** (package percobaan, Swift 6.4): trait `TestTrait` dan `TestScoping` yang menjalankan body lewat `IssueHandlingTrait.compactMapIssues { ... }.provideScope(...)` berhasil dikompilasi. Hasilnya: `#expect` yang gagal tercatat `FAIL`; error yang dilempar body tercatat sebagai issue gagal (`Caught error`), bukan error yang keluar dari `provideScope`; `withKnownIssue` tetap `PASS` karena `isFailure` bernilai false; `Test.cancel` tidak terlihat sebagai issue maupun error, tetapi `Task.isCancelled` bernilai true sesudahnya; dan `scopeProvider` yang hanya aktif saat `testCase` nil memberi satu scope untuk test berparameter dengan dua case. `Mutex` harus dibungkus `final class` supaya bisa dipakai di closure yang escaping. `Test.Case` hanya punya `isParameterized` sebagai anggota publik.
- **`--skip` dan `--filter`**: di package percobaan, `swift test --skip 'NativeTests\.'` menjalankan semua test kecuali target `SpikeNativeTests`, dan `--filter 'NativeTests\.'` hanya menjalankan target itu. ID test berbentuk `SpikeTests.SpikeSuite/cancels()/T.swift:9:6`.

### Cross check

Draf pertama spec ini dibaca ulang oleh model lain (Sonnet 5.5) dengan akses baca saja. Ia menemukan celah keputusan yang harus diisi builder sendiri, dan semua perbaikan yang direkomendasikan diterapkan atas persetujuan engineer: crate daun `stackly-evidence`, `pass(&self)` dengan `Drop` yang menulis, bentuk `report.json` dan `run.json`, validasi lewat serde, aturan subject native, sumber identitas test, `evidencePath` dan `actual` yang wajib, rincian baseline (`virtualized`, volume penyimpanan), kebijakan probe yang gagal, aturan `evidence new`, sinyal di wrapper, cakupan `evidence check`, serta `--tests --no-fail-fast`.

Update spec ini setelah spec 0003 juga dibaca ulang model lain (Sonnet 5.5, baca saja). Temuan yang diterapkan atas persetujuan engineer: `sourceDirty` kembali ke `git status --porcelain` biasa dan wajib boolean, format `digest`, baris path report di wrapper, sesi login GUI untuk run native, urutan langkah, pembersihan kedua, `evidence/runtime/` sebagai pengecualian yang divalidasi `runtime check`, dan aturan penyalinan record.

### Perubahan 2026-10-17

CI ditiadakan atas keputusan pemilik proyek, dan pengembangan fokus di Mac ini dulu. Semua run fixture, run native, cek, dan pencatatan baseline dijalankan dengan tangan di Mac ini.

- Keputusan 3: Option 1 (Mac self hosted khusus CI) dihapus, dan mesin baseline menjadi Mac ini (Option 2).
- Keputusan 2: pilihan turunan trigger job native dihapus, begitu juga alasan runner hosted untuk garis native; garisnya sendiri tetap.
- Keputusan kecil: baris job native, commit di pull request, jadwal nightly, dan retention artifact dihapus; `BLOCKED` tidak lagi menjadi `::warning::`, dan `--runner-name` hilang dari baseline.

### Perubahan 2026-10-18

Saat kode disesuaikan dengan spec tanpa CI, muncul satu benturan: aturan versi di spec ini menulis bahwa menghapus field menaikkan versi ke `v2`, sedangkan `context.ci` (di `RunContext`) dan `runnerName` (di `BaselineHost`) sudah hilang dari spec dengan versi tetap `v1`. Pemilik proyek memilih menghapus kedua field itu dan tetap di `v1` (`stackly.evidence/v1`, `stackly.evidence-report/v1`, dan `stackly.evidence-baseline/v1`). Ini satu pengecualian tercatat dari aturan versi, bukan aturan baru; penghapusan field berikutnya tetap menaikkan versi.

Alasannya, tidak ada pembaca atau data yang terdampak:

- `ci` opsional dengan nilai default, dan bernilai null di setiap record dan report yang sudah di commit di `evidence/runtime/`.
- Decoder menerima field yang tidak dikenal, jadi report lama yang masih memuat `"ci": null` tetap bisa didecode dan lolos `runtime check` tanpa diedit.
- `evidence/baseline-host.json` belum pernah ditulis, jadi belum ada file baseline yang memuat `runnerName`.
- Fitur 65, pembaca format ini di luar `xtask`, belum dibangun.

Opsi yang ditolak: naik ke `v2` (report `v1` yang sudah di commit tidak boleh diedit, jadi validasi dan aturan 7 `runtime check` di spec 0003 harus menerima dua versi sekaligus), dan mempertahankan `ci` sebagai field usang yang selalu null di `v1` (schema tetap membawa bentuk khusus CI tanpa ada yang mengisinya).

Masih dari penghapusan CI: target `x86_64-unknown-linux-gnu` dibuang dari `[graph] targets` di `deny.toml`, karena tidak ada lagi yang dibangun atau dijalankan di Linux. Yang tersisa `aarch64-apple-darwin` dan cek portabilitas `aarch64-pc-windows-msvc`.

Catatan ini dibaca ulang model lain (Sonnet 5.5, baca saja). Tiga temuannya diterapkan atas persetujuan engineer: kalimat di Validasi tentang field yang sudah dihapus, satu baris tentang cabang Linux yang tersisa dan target `cargo deny`, serta "field yang dikenal" pada aturan penyalinan record (di invariant spec ini dan di skenario test `runtime record` spec 0003), karena `runtime record` mendecode lalu menulis ulang report, sehingga field yang tidak dikenal tidak ikut tersalin.

### Perubahan 2026-10-18: pilihan build yang dicatat saat verify

Tiga sesi build fitur 2 mengambil beberapa pilihan kecil yang tidak disebut spec ini. Saat `/check verify`, pilihan itu ditunjukkan ke pemilik proyek, yang memilih mencatat semuanya apa adanya. Kode tidak berubah; spec hanya menyusul perilaku yang sudah dibangun dan sudah dibuktikan di Mac ini.

- Pindaian fixture credential fail closed. File yang tidak bisa dibaca menjadi `credential_leak` "cannot scan", karena tidak adanya prefix tidak bisa dibuktikan. Opsi yang ditolak: hanya peringatan, yang membuat run lolos walau sebagian output tidak pernah diperiksa.
- Pindaian tidak mengikuti symlink, socket, atau pipe, supaya tidak keluar dari folder run dan tidak menggantung di FIFO. File `records/*.tmp` ikut dipindai, karena record yang setengah ditulis tetap bisa memuat prefix.
- `evidence check` memangkas `-` di kedua ujung slug, menolak dua record manual dengan `id` yang sama (aturan "review ulang berarti file manual baru" jadi bisa dicek), dan hanya menerima `evidencePath` manual yang menunjuk file record itu sendiri atau `evidence/manual/files/<id>/`.
- Pesan pindaian Rust menyebut file dan fungsi, bukan nomor baris. Nomor baris butuh fitur `span-locations` di `proc-macro2`; nama fungsi sudah cukup untuk menemukan test-nya. Opsi yang ditolak: menambah fitur itu hanya untuk pesan error.
- Gate native Swift memanggil `Test.cancel` di `prepare(for:)`, sehingga Swift Testing melaporkan test itu skipped dengan alasannya. Memanggilnya di dalam `provideScope` sebelum body jalan membuat test hilang dari output `swift test`, jadi kata "cancelled" hanya berlaku untuk `Evidence.blocked` di dalam body.
- Pindaian Swift melewati baris komentar `//`, supaya contoh `.evidence(` dan `kind: .native` di doc comment `StacklyTestSupport` tidak dianggap kode.

## Perubahan 2026-10-18 (rekonsiliasi dengan spec 0013)

XCUITest pertama kini milik fitur 68, karena spec 0013 membuktikan shell dengan Swift Testing dan record manual. `StacklyTestSupport` menjadi product package `clients/swift` supaya test `StacklyUI` bisa memakai `.evidence`, dan integration test Swift yang butuh `stacklyd` memakai `STACKLY_DAEMON_BIN` dengan pola `BLOCKED` saat binary tidak ada. Tidak ada keputusan spec ini yang berubah.


## Perubahan 2026-10-20 (ditambah fase 02)

Dari inventaris `docs/scope/coordination/02-instalasi-pertama.usulan-fondasi.md` bagian *Spec 0002*; semua resolusi yang direkomendasikan diambil (keputusan lead scope 02 atas nama user). Keputusan spec ini tidak berubah.

- Folder `evidence/compatibility/<runId>/` (spec 0018) masuk tabel *Folder dan file*; kalimat evidence otomatis di git, baris `evidence check`, invariant salinan record, dan sumber fitur 65 di Consequences menyebut kedua folder.
- *Menjalankan test di Mac ini* mencatat dua langkah `--cleanup` baru: pencabutan agent test `dev.stackly.test.agent.*` tanpa lock (spec 0015) dan TERM lalu KILL untuk proses `/stackly-test-` (spec 0018). Spec ini hanya menjelaskan apa yang jalan dan kapan relatif terhadap run native; semantik lock dan urutannya ditunjuk ke spec 0003, supaya kedua spec tidak bergeser satu sama lain.
- Gate test `STACKLY_NATIVE_NETWORK=1` (spec 0018) dicatat di sebelah urutan run native.
- Yang tidak diubah di sini: Follow-up `--cleanup` (proses sisa di luar spike) dibiarkan terbuka sampai `/sync` lead setelah milestone 5 langkah 16 spec 0018 dan milestone 2 langkah 8 spec 0015 mendarat; penambahan `cargo xtask catalog check` ke pre commit hook (spec 0017) adalah item `/sync` untuk `AGENTS.md`, karena spec ini tidak mendeskripsikan hook.

Perubahan ini dibaca model lain (Sonnet 5.5, baca saja) sebelum commit. Semua butir inventaris dinilai sudah diterapkan dengan benar; perbaikan kata yang diusulkannya ikut diterapkan (sumber fitur 65 menjadi lima tempat, dan siapa yang menulis kedua folder evidence).

## Perubahan 2026-10-20 (ID risiko PRD, penyelarasan setelah build)

Update `/architect` sesi `arc-foundation-update`, dari log koordinasi `docs/scope/coordination/01-foundation.log.md` bagian Keputusan (catatan `evidence!` hanya menerima AC, REQ, NFR, VAL, dan BR). Satu baris Follow-up baru mencatat bahwa ID risiko `RISK-NN` tidak lolos pola PRD ID, sehingga guard spec 0010 untuk RISK-03 dan RISK-14 menulisnya di awal `expected`, dan bahwa itu aturan sementara sampai laporan rilis butuh hitungan per risiko. Keputusan spec ini tidak berubah, dan kode `stackly-evidence` serta `StacklyTestSupport` tidak disentuh. Runner up yang tidak diambil sekarang: langsung menambah `RISK` dua digit ke pola, karena itu perubahan kode di dua bahasa tanpa pemakai yang membutuhkannya hari ini.
