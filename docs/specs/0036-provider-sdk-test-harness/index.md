# 0036. Provider SDK dan test harness

**Date**: 2026-11-11
**Status**: In Progress

## Summary

Spec ini memberi kontributor provider satu jalur yang sama dengan provider resmi: aturan tertulis tentang cara schema `stackly/v1alpha1` boleh berubah, satu validator, satu paket contoh yang juga menjadi template scaffold (kerangka awal), dan satu harness konformitas (rangkaian uji yang menjalankan paket lewat daemon sungguhan di folder sementara). Validator hanya membaca file dan tidak pernah menjalankan apa pun, jadi manifest dengan versi schema yang tidak didukung ditolak dengan penjelasan sebelum ada lifecycle yang berjalan. Harness berjalan dari checkout repo lewat `cargo xtask provider test`, memakai daemon di dalam proses test, menulis report JSON, dan menghapus foldernya sendiri hanya setelah memeriksa bahwa folder itu memang milik run tersebut. Dengan ini lima provider berikutnya (fitur 39 sampai 43) dibangun dengan menambah folder paket, bukan dengan menambah kode Rust core atau SwiftUI.

## Requirements

**User stories**:

- Sebagai kontributor provider, saya ingin membuat paket baru dari template yang langsung valid, lalu tahu dari satu perintah apakah paket saya bisa dipasang, dibuat, dijalankan, dan sehat, supaya saya tidak perlu membaca kode core untuk mulai.
- Sebagai maintainer, saya ingin aturan tertulis tentang perubahan schema dan satu kumpulan paket lama yang dibekukan, supaya penambahan primitive atau probe tidak pernah mematahkan instance yang sudah di pin ke revision lama.
- Sebagai pengguna dan sebagai developer import (fitur 37), saya ingin memeriksa sebuah folder paket tanpa menjalankan isinya, supaya paket yang salah atau dibuat untuk versi schema lain ketahuan sebelum ada executable yang berjalan.
- Sebagai pemilik scope, saya ingin bukti bahwa provider fake server yang baru bekerja dari install sampai form tanpa perubahan Rust core atau SwiftUI (AC-029 akhir), dan bahwa harness menghasilkan report dari checkout bersih (AC-034).

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-030):

- **AC-1** (versi schema, AC-030, REQ-030): `stackly_core::provider::check::SUPPORTED_API_VERSIONS` berisi tepat satu entri, `stackly/v1alpha1` berstatus `current`. Paket yang `apiVersion` nya bukan salah satu nama di tabel itu menghasilkan tepat satu finding `unsupported_api_version` di `provider.yaml` lokasi `apiVersion`, dengan pesan `apiVersion "<nilai>" is not supported; this Stackly supports: <daftar nama dipisah koma>`, tanpa mendecode sisa dokumen (`<nilai>` adalah teks scalar YAML itu, atau `<not a string>` kalau bukan scalar teks; `apiVersion` yang tidak ada tetap `missing_field` seperti sekarang). Hal ini berlaku di setiap jalur muat: `check_package`, `provider.validate`, `cargo xtask provider check`, muat catalog (entri menjadi `invalid`, spec 0017), dan harness (verdict `fail` di langkah `validate`, tanpa root dibuat). Di setiap jalur itu `FakePlatform::mutations()` tidak bertambah dan tidak ada proses yang di spawn.
- **AC-2** (validator core, REQ-030): `check_package(fs, dir, name_check)` membaca paket hanya lewat port `Filesystem` dan mengembalikan `PackageCheck` yang `findings` nya memuat: setiap issue `PackageError` sebagai `error` ber origin `package`; setiap issue `ValidatedPackage::authoring_findings()` sebagai `error` ber origin `authoring` (hari ini persis isi `init_port_issues`, spec 0009 AC-21); dan setiap masalah `tests/scenarios.yaml` sebagai `error` ber origin `scenarios`. `loadable()` benar kalau tidak ada finding origin `package`; `publishable()` benar kalau tidak ada finding `error` sama sekali. `ValidatedPackage::from_files` tidak berubah: fixture `fake-command` (yang punya satu issue `init_port_issues`) tetap `loadable` tetapi tidak `publishable`. Fungsi ini tidak punya parameter yang bisa menjalankan proses, menulis file, atau membaca secret.
- **AC-3** (`tests/scenarios.yaml`): file itu opsional dan mengikuti *File skenario*: decode ketat (field asing, key ganda, tag lokal, dan merge key ditolak), `schema` persis `stackly.provider-scenarios/v1`, paling banyak 16 skenario, nama mengikuti aturan `InstanceName` domain dengan panjang paling banyak 24 karakter (huruf kecil, angka, dan `-` tunggal di tengah) dan unik, `expect` salah satu dari `ready`, `start_fails`, atau `create_fails`, dan `settings` tiap skenario lolos `SettingsSchema::validate` paket itu. Key `settings` yang merujuk field secret ditolak dengan `secret_not_allowed_here`. Ukuran file di atas 64 KiB, symlink, atau file yang bukan UTF-8 ditolak. Paket tanpa file itu punya satu skenario tersirat bernama `default` dengan settings kosong dan `expect: ready`. Mengubah file itu tidak mengubah `manifest_sha256` (spec 0009 AC-12), dan `read_package` tetap tidak pernah membacanya. Fuzz target `provider_scenarios` (corpus awal dari `providers/example` dan `providers/fake`) berjalan 10 menit di Mac ini tanpa crash, hang, atau kehabisan memori.
- **AC-4** (`cargo xtask provider check`): perintah ini memanggil `check_package` dengan `NameCheck::FolderName` dan mencetak, per paket, satu baris `ok <id> <revision> <manifest_sha256>` kalau `publishable`, atau satu baris `<folder>: <file> <lokasi>: <kode>: <pesan>` per finding `error` (format yang ada tidak berubah), dan satu baris berawalan `warning: ` per finding `warning`. Exit 1 kalau ada paket yang tidak `publishable`. Tanpa argumen folder, perintah ini juga gagal dengan `<folder>: not frozen: run cargo xtask provider freeze providers/<folder>` untuk setiap paket yang tercantum sebagai `[[provider]]` di `providers/catalog.toml` dan belum punya entri corpus dengan digest yang sama (AC-6).
- **AC-5** (`provider.validate` dan CLI): method `provider.validate` (`recovery allowed`, bukan operation, tidak menulis audit) menerima `ProviderValidateParams { path }` dan membalas `ProviderValidation` di *Kontrak wire*. Path relatif, kosong, di atas 4096 byte, atau memuat NUL ditolak `request.invalid_params`. Paket yang rusak, folder yang tidak ada, atau folder yang tidak bisa dibaca tetap balasan sukses dengan `loadable: false` dan finding nya; daftar finding dipotong di 100 dengan `omitted` terisi. `stackly provider validate <dir> [--json]` mengubah `<dir>` menjadi path absolut, mencetak baris yang sama dengan AC-4, keluar 0 kalau `publishable`, dan selain itu memberi `cli.provider_invalid` (kategori `invalid_input`, exit 2) dengan `result` tetap terisi. Memanggil method ini tidak mengubah registry, catalog, atau file apa pun, dibuktikan dengan generation registry dan isi root yang sama sebelum dan sesudah.
- **AC-6** (corpus kompatibilitas, REQ-030): `crates/core/tests/fixtures/compat/<nama>/<revision>/` menyimpan `package/` (salinan beku setiap revision paket `official/` yang pernah tercantum di catalog sejak fitur ini) dan `frozen.json` di sampingnya, di luar folder paket supaya `read_package` tidak menolaknya. Test core memuat setiap entri dengan `NameCheck::Skip` dan membuktikan: paket termuat, `manifest_sha256` sama dengan `frozen.json`, `init_plan` dan `run_plan` terbangun untuk *Context beku*, dan argv start, nama key env start, serta SHA-256 setiap config yang dirender sama dengan `frozen.json`. `cargo xtask provider freeze <dir>` menulis entri baru dan menolak menimpa entri yang ada dengan digest berbeda. Saat fitur ini ditutup, corpus berisi revision `official/nats`, `official/postgresql`, dan setiap paket lain yang saat itu tercantum sebagai `[[provider]]`.
- **AC-7** (paket contoh dan scaffold): `providers/example/` (ID `testing/example`, software `fake`, category `testing`) lolos `cargo xtask provider check` dan memenuhi *Paket contoh*. `cargo xtask provider new <name> [--out <dir>]` menyalin paket itu ke `<dir>/<name>/` (default `providers/<name>/`), mengganti identitas sesuai *Scaffold*, menolak nama yang bukan bagian nama `ProviderName` yang valid, menolak folder tujuan yang sudah ada tanpa menulis apa pun, lalu menjalankan `check_package` pada hasilnya dan mencetak baris `ok`. Hasil scaffold berbeda dari paket contoh hanya pada baris yang tercantum di *Scaffold*.
- **AC-8** (form dari schema, AC-029 bagian form): `apps/daemon/tests/create_form_fixtures.rs` membangun `protocols/fixtures/instance/create-form-<folder>.json` untuk setiap folder di `providers/`, bukan dari daftar nama di kode, dan gagal kalau file fixture sebuah folder tidak ada atau drift. Fakta runtime tiap fixture dibaca dari paket: nama tampil dari `metadata.displayName`, versi dan line dari artefak pertama `versions.json` untuk target host. Fixture `fake`, `nats`, dan `postgresql` yang ada ditulis ulang sekali di langkah ini, dan test Swift yang membaca nilai lama ikut disesuaikan. Satu test `StacklyUI` membaca setiap file `create-form-*.json` di folder fixture itu, membangun `SchemaFormModel` (spec 0020 AC-21), dan gagal kalau ada field yang jatuh ke jenis yang tidak dikenal. Menambah paket baru menambah satu file fixture dan tidak menambah kode Swift atau Rust.
- **AC-9** (harness, AC-029 akhir, REQ-029): `cargo xtask provider test providers/example` berakhir exit 0 dengan report ber `verdict: pass`. Harness membuat root sementara, menjalankan `stacklyd` di dalam proses lewat `stacklyd::server::start`, dan sesudah persiapan root hanya memakai method kontrak lewat socket untuk langkah di *Langkah skenario*, dalam urutan ini: `catalog`, `runtime`, `form`, `create`, `start`, `health`, `connection`, `process`, `logs`, `second_instance` (kalau `multiInstance`), `reload` (kalau capability `reload`), `restart`, `handover`, `stop`, `leftovers`, dan `start_again`. Setiap langkah yang tidak berlaku tercatat `skipped` dengan alasan. Paket `providers/fake` juga lulus. Harness menolak paket dengan namespace selain `official` dan `testing`, dan paket yang tidak `publishable`, dengan verdict `fail` di langkah `validate` tanpa membuat root.
- **AC-10** (kontrak proses, spec 0010): langkah `process` mengambil dua sampel proses utama di dalam langkah itu sendiri, dengan jarak 2 detik, dan gagal dengan reason di *Kontrak proses* kalau: `instance.get` melaporkan `adoptable` selain `true` atau fd 3 proses itu bukan `instance.lock` instance (`lock_not_held`); file executable proses berbeda dari program runtime (root runtime dari `runtime.get` digabung `runtime.executable`) atau berbeda antarsampel (`re_exec`); PID atau `startedAt` berbeda antarsampel, atau proses bukan pemimpin process group nya (`not_foreground`). Langkah `leftovers` gagal kalau sesudah stop masih ada proses yang memegang `instance.lock`, masih ada proses di process group proses utama, atau port endpoint masih menerima koneksi. Paket fixture `testing/fake-rogue` dengan `closeLockFd` gagal di `process` dengan `lock_not_held`, dengan `reExec` gagal di `process` dengan `re_exec`, dan dengan `background` gagal di `start` (proses utama keluar sebelum ready) sementara `leftovers` mencatat `lock_still_held` dan `process_left` untuk proses anak yang tertinggal. Di ketiganya harness tetap mengakhiri proses yang tertinggal. Perbandingan `not_foreground` dibuktikan unit test atas dua sampel buatan.
- **AC-11** (fault fake server, PRD bagian 32.3): dengan paket `providers/fake`, skenario `readyDelayMs: 1500` lulus dan durasi langkah `start` paling sedikit 1500 ms; skenario `readyDelayMs: 60000` dan `crashAfterMs: 300` (crash sebelum ready, seperti test crash yang ada) dengan `expect: start_fails` lulus, dan skenario yang sama dengan `expect: ready` gagal di langkah `start` dengan kode error operasi tercatat di report; skenario `logLinesPerSecond: 2000` lulus, dan langkah `logs` mengulang `instance.logs_read` dengan `limit: 1000` sampai satu halaman berisi 1000 baris, tanpa melewati batas waktu langkah. Skenario fault ini diberikan test lewat `RunRequest.scenarios`, bukan lewat `tests/scenarios.yaml` paket fake.
- **AC-12** (root dan pembersihan, PRD bagian 27 dan 32.3): setiap folder root harness membawa penanda `.stackly-test-run` (`stackly.test-run/v1`) berisi `runId`, `uid`, dan `pid`. `remove_checked` menghapus sebuah folder hanya kalau semua aturan di *Root dan kepemilikan* terpenuhi; folder tanpa penanda, penanda dengan `runId` atau `uid` lain, symlink, atau folder di luar folder sementara tidak dihapus, dan langkah `cleanup` tercatat `fail` dengan reason. Path yang diberikan pengguna (`<dir>` paket, `--report`, `--out`) tidak pernah dihapus rekursif. `--keep` membiarkan root dan mencetak path nya. `remove_checked` juga menolak selama masih ada proses yang argumennya memuat path root itu. `cargo xtask provider test --cleanup` hanya menyentuh root berpenanda milik uid ini yang `pid` nya sudah mati: ia mengakhiri proses yang tertinggal di root itu lebih dulu, lalu menghapusnya. Setelah run yang lulus maupun gagal, tidak ada proses provider dan tidak ada folder root yang tersisa.
- **AC-13** (report, AC-034): setiap run menulis `report.json` ber schema `stackly.provider-conformance/v1` (*Report*) secara atomik ke `<target dir>/provider-test/<runId>/` atau ke `--report <file>`, dan mencetak `provider test report: <path absolut>` sebagai baris terakhir stderr. Report tidak memuat path home (ditulis `~`), nilai secret, atau nilai settings teks; ekor log yang ikut hanya teks yang sudah melewati redactor daemon. Exit 0 untuk `pass`, 1 untuk `fail`, 2 untuk `blocked`. Dari checkout tanpa folder `target/` dan tanpa file untracked, `cargo xtask provider check providers/example` lalu `cargo xtask provider test providers/example` menghasilkan baris `ok` dan report `pass` tanpa unduhan runtime dan tanpa perubahan file yang dilacak git. Satu satunya akses jaringan adalah cargo yang mengambil crate di `Cargo.lock` saat build pertama; harness dan daemon di dalamnya tidak membuka koneksi selain ke loopback. Run yang dihentikan Ctrl+C menulis report dengan `interrupted: true`, verdict `fail`, dan exit 130.
- **AC-14** (provider baru tanpa perubahan core, AC-029 akhir, AC-034): test `xtask` menjalankan `provider new` ke folder di `TestRoot` (di luar tree repo), lalu `provider check` dan `provider test` pada hasilnya; report nya `pass`, dan `git status --porcelain` repo sama sebelum dan sesudah.
- **AC-15** (konformitas native): `apps/daemon/tests/provider_conformance.rs` punya satu test native per line runtime paket `official/` yang ada (`nats` 2.15, `postgresql` 17, `postgresql` 18, `traefik` 3.7), masing masing `#[ignore = "native: ..."]` dengan `covers = ["AC-029", "REQ-029"]`, subject berisi versi teramati dan digest artefak, dan `actual` berformat `<provider> <versi> ready in <ms> ms, stopped in <ms> ms`. Artefak yang tidak ada di cache runtime xtask membuat test `BLOCKED` dengan alasan dan owner, tidak pernah unduhan. Langkah `handover` hanya melaporkan `proven` atau `unproven` dari `daemon.restart_preview`; hasil harness tidak pernah menambah `HANDOVER_PROVEN`.
- **AC-16** (spike dari paket, spec 0003): untuk provider inventory yang punya paket di `providers/` (`nats`, `postgresql`, `traefik`), worker spike membangun init, start, ready, versi, dan stop dari paket itu lewat `PackageRecipe` (*Spike dari paket*), dan struct recipe ketiganya dihapus dari `xtask/src/runtime/spike/providers.rs`. Hasil spike tetap `stackly.runtime-spike-result/v1` dengan field yang sama. Penanda `// provisional: F36` hilang dari `spike/mod.rs` dan `spike/providers.rs`; recipe yang tersisa membawa `// provisional: F40` (Valkey), `// provisional: F42` (MySQL), dan `// provisional: F43` (ClickHouse). `xtask` memakai `stackly-core` dengan fitur `test-support` untuk `ResolvedSecrets::for_tests`, dan `xtask/src/runtime/spike/package_recipe.rs` masuk `EXPOSE_ALLOWED`, karena ia menulis nilai secret fixture buatannya sendiri ke stdin, env, dan file langkah init; nilai itu selalu berawalan credential fixture spec 0002. Test unit worker membuktikan `PackageRecipe` paket fake menjalankan init, start, ready, dan stop terhadap `stackly-fake-server`.

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md): kebijakan additive tanpa field versi baru di manifest dan dijaga corpus paket beku; satu validator di core dengan dua pintu (xtask tanpa daemon, dan method read only `provider.validate` untuk CLI serta fitur 37); paket contoh yang valid sebagai template scaffold; harness konformitas sebagai modul test `stacklyd` yang menjalankan daemon di dalam proses dan dibungkus `cargo xtask provider test`; dan langkah spike yang dibangun dari paket provider.

`stackly provider validate` berjalan di daemon. `stackly provider test` tidak dibuat: harness membutuhkan fitur `fixture` core, secret store test, dan binary hasil build checkout, jadi ia hanya ada sebagai `cargo xtask provider test`. CLI tetap tidak pernah menjalankan binary provider, dan setiap proses provider di harness di spawn lewat port `ProcessSupervision` milik daemon.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang.

## Feature design

### Trace PRD

| ID | Dipenuhi oleh |
| --- | --- |
| REQ-030 | AC-1 (versi), AC-2 dan AC-5 (validator), AC-6 (kompatibilitas dan migrasi), AC-7 (scaffold), AC-9 (harness), *Kebijakan kompatibilitas* |
| AC-029 (akhir) | AC-8 (form), AC-9, AC-14, AC-15 |
| AC-030 | AC-1 |
| AC-034 | AC-13, AC-14 (bagian dokumen panduan milik spec 0038) |
| PRD bagian 21 | *Kebijakan kompatibilitas*, *File skenario*, *Paket contoh* |
| PRD bagian 27 | AC-12 (root sementara, port dari daemon, credential fixture, kepemilikan cleanup, log tersanitasi) |
| PRD bagian 32.3 | AC-10, AC-11, AC-12 |

### Batas dengan spec lain

- Spec 0009 tetap pemilik schema manifest, loader, validasi, dan plan. Spec ini tidak mengubah `read_package` atau `from_files`, dan tidak menambah `IssueCode`. Yang berubah di area spec 0009 hanya teks pesan `unsupported_api_version` (AC-1).
- Spec 0037 memakai `check_package` (dengan `NameCheck::Skip`) dan bentuk `ValidationFinding` untuk preview import. Apakah import menolak atau memperingatkan finding `authoring` adalah keputusan spec 0037 (Follow-up spec 0009), bukan spec ini; spec 0037 memutuskan import menolak finding `error` ber origin `package` atau `authoring`, membuang finding `scenarios`, dan meneruskan finding `warning` ke preview.
- Spec 0038 memiliki `docs/providers/` (panduan penulis, referensi schema, dan halaman kebijakan berbahasa Inggris yang merangkum *Kebijakan kompatibilitas*). Spec ini hanya menulis `providers/example/README.md`.
- Spec 0003 tetap pemilik inventory, artefak, dan langkah spike di luar lifecycle provider. Spec 0018 tetap pemilik `runtime.install`; harness tidak menguji unduhan.
- Spec 0030 tetap pemilik `HANDOVER_PROVEN`.

### Kebijakan kompatibilitas

Ini keputusan normatif untuk `stackly/v1alpha1`. Setiap perubahan schema lewat `/architect` update spec 0009 (pemiliknya) dan mencatat barisnya di tabel *Perluasan additive* spec itu.

**Versi dan status.** `apiVersion` adalah satu satunya penanda versi kontrak manifest. Tabel `SUPPORTED_API_VERSIONS` di core memuat setiap versi yang bisa dimuat build itu beserta statusnya:

| Status | Arti |
| --- | --- |
| `current` | versi untuk paket baru |
| `deprecated` | masih termuat dan berjalan; `check_package` menambah satu finding `warning` per paket, dan `cargo xtask provider check` tanpa argumen menolak paket di `providers/` yang memakainya |

Tidak ada field versi kedua di manifest (tidak ada `schemaRevision` atau versi engine minimum). Paket yang memakai penambahan yang lebih baru dari build yang membacanya ditolak oleh validasi ketat yang ada (`unknown_field`, `unknown_primitive`, `unknown_probe`, `unknown_template_format`), yang pesannya sudah menyebut nilai yang dikenal.

**Tiga kelas perubahan.**

| Kelas | Contoh | Aturan |
| --- | --- | --- |
| Additive | primitive baru, jenis probe baru, probe native baru, kind param native baru, format template baru, token interpolasi baru, field manifest opsional baru yang default nya sama dengan perilaku lama, nilai baru di enum tertutup, capability baru, kode issue baru | tetap di `stackly/v1alpha1`; setiap paket yang valid sebelumnya tetap valid, digest nya sama, dan plan nya sama (dijaga AC-6) |
| Pengetatan untuk penulis | aturan baru yang membuat paket lama tidak lagi layak terbit, misalnya `init_port_issues` | tidak pernah masuk `read_package` atau `from_files`; ditulis sebagai fungsi laporan terpisah dan dikumpulkan `ValidatedPackage::authoring_findings()`, supaya salinan pinned di `<data>/providers/` tetap termuat dan instance nya tetap bisa start |
| Merusak | menghapus atau mengganti arti field, primitive, token, atau default; mengubah hasil render atau argv paket yang ada; mengetatkan `from_files` | butuh `apiVersion` baru |

**Syarat perubahan additive.** Satu perubahan additive selesai kalau: (1) spec 0009 diperbarui lewat `/architect` dan barisnya ada di tabel *Perluasan additive*; (2) nilai yang tidak dikenal tetap ditolak saat validasi, tidak pernah saat start; (3) test corpus AC-6 tetap lulus tanpa mengubah satu pun `frozen.json`; (4) `cargo xtask codegen` menulis ulang schema editor di `schemas/provider/`; (5) test jumlah kode di `issue.rs` diperbarui kalau ada kode baru; (6) corpus fuzz target yang terkait mendapat contoh baru; dan (7) paket contoh atau fixture memakai penambahan itu paling sedikit sekali. Penambahan kode issue yang sampai ke wire tetap mengikuti aturan bump `ProtocolVersion` spec 0006.

**Probe native** (jawaban Follow-up spec 0023). Jenis `native` adalah registry tertutup `NativeProbeKind` di `stackly_core::provider::probe::native`; manifest tidak pernah bisa menambah nama. Kind param yang ada: `text { max_bytes }`, `boolean`, dan `configFile { max_bytes }`. Probe native baru atau kind param baru adalah perubahan additive dengan syarat tambahan, diperiksa saat review:

1. Kebutuhannya tidak bisa dipenuhi probe `tcp`, `http`, atau `exec`; alasan nya ditulis di spec provider itu.
2. Descriptor (`name`, transport endpoint, param) dan `build` ditambah lewat `match` exhaustive, jadi setiap nama yang lolos validasi punya runner.
3. State machine protokol nya tanpa IO di `core::provider::probe`, dengan batas byte yang dibaca dan batas waktu per percobaan; IO nya di `core::supervisor::probe::native`.
4. Probe hanya menghubungi endpoint loopback instance itu, tidak pernah menerima token `secret.*`, dan param path hanya lewat kind `configFile`.
5. Parser protokol nya punya fuzz target.
6. Ada test native dengan runtime sungguhan.
7. Probe itu dihitung sebagai ukuran RISK-07 di catatan spec provider nya.

**Deprecation.** Sebuah field, primitive, token, atau format ditandai deprecated lewat update spec 0009 yang menyebut penggantinya. Sejak itu `authoring_findings()` melaporkannya sebagai `warning` dengan pesan yang menyebut pengganti, dan paket di `providers/` tidak boleh memakainya (penjaga: `cargo xtask provider check` tanpa argumen memperlakukan `warning` di paket repo sebagai kegagalan). Item deprecated tetap bekerja di `v1alpha1` selamanya; ia hanya hilang di `apiVersion` baru. Saat spec ini ditulis tidak ada item deprecated, jadi kode `IssueCode` untuknya ditambah oleh deprecation pertama, bukan sekarang.

**Migrasi ke `apiVersion` baru.** `apiVersion` baru membutuhkan spec sendiri yang memuat: decoder untuk versi baru di samping versi lama; tabel `SUPPORTED_API_VERSIONS` dengan versi lama berstatus `deprecated`; dan konversi murni dari model versi lama ke model engine, tanpa mengubah byte paket atau `manifest_sha256` nya. Paket tidak pernah ditulis ulang di tempat. Versi yang pernah keluar di rilis stabil tetap bisa dimuat selama masih mungkin ada salinan pinned yang memakainya; membuang decoder nya butuh spec sendiri dengan migrasi registry yang memindahkan setiap instance ke revision baru lebih dulu. Corpus AC-6 ikut berlaku untuk setiap versi.

### Validator

Modul baru `stackly_core::provider::check`, tanpa IO selain lewat `&dyn Filesystem`:

```rust
pub struct ApiVersionSupport { pub name: &'static str, pub status: ApiVersionStatus }
pub enum ApiVersionStatus { Current, Deprecated }
pub const SUPPORTED_API_VERSIONS: &[ApiVersionSupport];

pub enum Severity { Error, Warning }
pub enum FindingOrigin { Package, Authoring, Scenarios }
pub struct Finding { pub severity: Severity, pub origin: FindingOrigin, pub issue: Issue }

#[non_exhaustive]
pub struct PackageCheck {
    pub folder: String,
    pub api_version: Option<String>,
    pub package: Option<ValidatedPackage>,
    pub scenarios: Vec<Scenario>,
    pub findings: Vec<Finding>,
}
impl PackageCheck { pub fn loadable(&self) -> bool; pub fn publishable(&self) -> bool; }

pub fn check_package(fs: &dyn Filesystem, dir: &Path, name_check: NameCheck) -> PackageCheck;
```

Urutan `check_package`: (1) `read_package`; gagal berarti finding `package` dan selesai. (2) `apiVersion` dibaca dari `provider.yaml` untuk `api_version` (nilai string mentah, atau `None` kalau tidak ada). (3) `ValidatedPackage::from_files`; gagal berarti finding `package` dan selesai. (4) `authoring_findings()`. (5) `read_scenarios` lalu `Scenarios::validate` terhadap settings schema paket. Urutan finding: `package`, `authoring`, lalu `scenarios`, masing masing dalam urutan sumbernya. `folder` adalah nama folder terakhir dari `dir`.

`ValidatedPackage::authoring_findings() -> Vec<Finding>` memanggil `init_port_issues()` dan mengembalikan setiap issue nya sebagai `Error` ber origin `Authoring`. `init_port_issues` tetap ada dan tidak berubah.

Pesan `unsupported_api_version` dibangun dari `SUPPORTED_API_VERSIONS`, dan `validate.rs` memakai tabel itu sebagai ganti konstanta `API_VERSION`.

### File skenario

`tests/scenarios.yaml` adalah satu satunya file di `tests/` yang dibaca, dan hanya oleh `check_package`, harness, dan spike. File lain di `tests/` tidak pernah dibaca.

```yaml
schema: stackly.provider-scenarios/v1
scenarios:
  - name: default
    settings: { greeting: "hello" }
  - name: slow-ready
    settings: { readyDelayMs: 1500 }
  - name: crash-on-start
    settings: { readyDelayMs: 60000, crashAfterMs: 300 }
    expect: start_fails
```

| Field | Tipe | Wajib | Aturan |
| --- | --- | --- | --- |
| `schema` | string | ya | persis `stackly.provider-scenarios/v1`, selain itu `invalid_value` |
| `scenarios` | daftar | ya | 1 sampai 16, selain itu `invalid_value` |
| `scenarios[].name` | string | ya | lolos `InstanceName` domain dan paling banyak 24 karakter (`invalid_name`), unik (`duplicate_key`) |
| `scenarios[].settings` | object | tidak | default kosong; divalidasi `SettingsSchema::validate` setelah key secret diperiksa; issue settings dipakai apa adanya dengan lokasi `scenarios[<i>].settings.<key>`; key yang merupakan field secret `secret_not_allowed_here` |
| `scenarios[].expect` | string | tidak | `ready` (default), `start_fails`, atau `create_fails`, selain itu `invalid_value`. `create_fails` terpenuhi oleh `instance.preflight` atau `instance.create` yang ditolak, atau oleh operasi create yang berakhir `failed` |

Modul `stackly_core::provider::scenarios`: `read_scenarios(fs, dir) -> Result<Option<String>, Issue>` membaca `tests/scenarios.yaml` dengan `symlink_metadata` dan `read_file_limited` (64 KiB; symlink `symlink_not_allowed`, bukan file biasa `not_a_regular_file`, bukan UTF-8 `not_utf8`; `tests/` yang berupa symlink juga ditolak), dan `Scenarios::parse(text) -> Result<Vec<Scenario>, Vec<Issue>>` memakai aturan YAML yang sama dengan manifest (`unknown_field`, `duplicate_key`, `yaml_syntax`, `missing_field`). `Scenario { name, settings: serde_json::Value, expect: Expect }`. Semua issue memakai `file` `tests/scenarios.yaml`. Skenario tersirat untuk paket tanpa file itu dibuat `check_package`, bukan pemanggilnya.

Secret tidak pernah ditulis di file ini: harness meminta daemon membuat nilainya (`SecretSource::Generate`).

### Kontrak wire

Area `provider` (spec 0006). Bump minor `ProtocolVersion`, nomor bebas berikutnya, ditetapkan lead saat build.

| Method | Recovery | Input | Output | Error |
| --- | --- | --- | --- | --- |
| `provider.validate` | allowed | `ProviderValidateParams { path: String }` | `ProviderValidation` | `request.invalid_params` |

`ProviderValidateParams::validate`: `path` 1 sampai 4096 byte, diawali `/`, tanpa NUL (`invalid_format`).

`ProviderValidation`:

| Field | Tipe | Sumber |
| --- | --- | --- |
| `folder` | string | `PackageCheck.folder` |
| `loadable`, `publishable` | boolean | `PackageCheck::loadable()`, `publishable()` |
| `apiVersion` | string, opsional | `PackageCheck.api_version` |
| `supportedApiVersions` | daftar `{ name, status }` | `SUPPORTED_API_VERSIONS`; `status` enum terbuka `current`, `deprecated` |
| `identity` | object, opsional | ada kalau `package` ada: `provider`, `revision`, `manifestSha256` dari `ValidatedPackage::key()`; `software` dan `displayName` dari manifest |
| `findings` | daftar `ValidationFinding` | paling banyak 100 pertama |
| `findingCount` | `Count` | jumlah semua finding |
| `omitted` | `Count` | `findingCount` dikurangi panjang `findings` |

`ValidationFinding { severity, origin, file, location, code, message, reason? }`: `severity` enum terbuka `error`, `warning`; `origin` enum terbuka `package`, `authoring`, `scenarios`; lima field sisanya dari `Issue` spec 0009 (`code` lewat `as_code()`). Semua enum terbuka berakhir `Unrecognized` (spec 0006). Fixture kontrak di `protocols/fixtures/provider/`: satu balasan `publishable`, satu `unsupported_api_version`, satu dengan finding `authoring`.

Handler di `stacklyd` (`ipc::provider`) memanggil `check_package(platform.filesystem, path, NameCheck::FolderName)` di `spawn_blocking`, lalu memetakan hasilnya. Ia tidak menyentuh registry, catalog, operation engine, atau audit.

### CLI

`stackly provider validate <dir> [--json]`. `<dir>` relatif digabung dengan folder kerja saat ini lalu dinormalkan secara leksikal (tanpa mengikuti symlink) sebelum dikirim. Teks manusia: baris AC-4 lalu satu baris ringkasan `<n> error(s), <n> warning(s)`. Mode `--json` memakai envelope spec 0008 dengan `result` `ProviderValidation`. Kode `cli.provider_invalid` baru di katalog CLI, kategori `invalid_input`.

### Corpus kompatibilitas

```text
crates/core/tests/fixtures/compat/
└── <nama>/<revision>/
    ├── package/        # provider.yaml, versions.json, schemas/, templates/, tests/scenarios.yaml: salinan byte demi byte
    └── frozen.json     # di luar package/, karena read_package menolak entri yang tidak dikenal
```

`frozen.json` (`stackly.provider-frozen/v1`): `provider`, `revision`, `manifestSha256`, `startArgs` (daftar argv start tanpa program), `startEnvKeys` (terurut), dan `rendered` (peta nama config file ke SHA-256 byte hasil render). Nilainya dihitung satu fungsi core di balik fitur `test-support`, `stackly_core::provider::testing::frozen_facts(&ValidatedPackage, &[Scenario]) -> Result<FrozenFacts, PlanError>`, yang dipakai `cargo xtask provider freeze` dan test `provider_compat` supaya keduanya tidak bisa berbeda.

**Context beku.** `frozen_facts` membangun satu `PlanContext` tetap: ID instance `0192f3a6-0000-7000-8000-0000000000f0`, nama `frozen`, path dari `DirectoryLayout::instance_paths` untuk root tetap `/frozen/data`, `/frozen/cache`, `/frozen/logs`, dan `/frozen/run`; root runtime `/frozen/runtime`; setiap endpoint tcp dan udp memakai `defaultPort` nya; settings adalah `settings` skenario `default` paket di atas default schema; dan setiap field secret bernilai byte `frozen-<field>` lewat `ResolvedSecrets::for_tests`. `rendered` adalah SHA-256 byte `RenderedContent::materialize` dengan secret itu. *Context golden* spec 0009 tidak dipakai, karena ia helper privat test `provider_packages` dan settings nya khusus per paket.

- `cargo xtask provider freeze <dir>`: memanggil `check_package`; menolak paket yang tidak `publishable` atau yang namespace nya bukan `official`; menulis entri; kalau entri `<nama>/<revision>` sudah ada dengan `manifestSha256` yang sama ia mencetak `already frozen` dan keluar 0; kalau digest nya berbeda ia keluar 1 dengan pesan bahwa revision harus naik.
- Entri tidak pernah dihapus atau diubah. `STACKLY_UPDATE_GOLDEN` tidak berlaku untuk corpus.
- Test `crates/core/tests/provider_compat.rs` berjalan untuk setiap entri (AC-6) dan memasang guard evidence `fixture` dengan `covers = ["REQ-030"]`.
- Paket `testing/` tidak dibekukan, karena tidak pernah di pin di root pengguna.

### Paket contoh

`providers/example/`, ID `testing/example`, software `fake`, category `testing`, revision `0.1.0`. Ia sengaja kecil supaya terbaca dalam sekali lihat:

- satu endpoint `http` (tcp, client, `127.0.0.1`, port saran 18090);
- satu config file `serverConfig` (format `json`, `sensitive: true`, destination `${instance.secrets}/server.json`) berisi `greeting`, `instanceName`, dan `authToken`, karena `stackly-fake-server serve` membaca ketiganya;
- initialize `createManagedDirectories` lalu `runExecutable` `init --data=${instance.data} --token-stdin` dengan `stdin: { secret: authToken }`;
- start `serve --config=${instance.secrets}/server.json --data=${instance.data} --port=${endpoint.http.port} --ready-delay-ms=${settings.readyDelayMs}`;
- stop `gracefulProcess` `term`; reload `render: [serverConfig]`, `action: signal`, `signal: hup`;
- readiness `http` `/ready`; health `http` `/health`; `dataMarker` `FAKE_VERSION`;
- capability `reload`, `multiInstance`, `persistence`; `managedFilesystem` `data`, `config`, `logs`, `temp`, `run`;
- settings: `greeting` (teks, `apply: reload`, default `hello`), `readyDelayMs` (integer 0 sampai 60000, default 0, `apply: restart`, `visibility: advanced`), `authToken` (secret, `immutableAfterInit`);
- satu entri `connection` `http://${endpoint.http.address}/config`;
- `versions.json` tulisan tangan dengan satu artefak `fixture` yang sama dengan milik `providers/fake`; test yang menjaga versi artefak fake sama dengan versi crate `stackly-fake-server` juga berlaku untuk paket ini;
- `tests/scenarios.yaml` dengan dua skenario: `default`, dan `slow-ready` (`readyDelayMs: 1500`);
- `README.md` berbahasa Inggris: apa isi tiap file, lima perintah (`new`, `check`, `test`, `freeze`, `codegen`), dan langkah menjadi paket `official/` (*Dari contoh ke paket resmi*), dengan link ke `docs/providers/` milik spec 0038.

Aturan "hanya `providers/fake/versions.json` yang tulisan tangan" di AGENTS.md menjadi "`versions.json` paket ber namespace `testing` tulisan tangan" (Follow-up untuk `/sync`). `cargo xtask codegen` tidak berubah, karena software `fake` tidak punya entri inventory.

### Scaffold

`cargo xtask provider new <name> [--out <dir>]`:

1. `<name>` harus lolos aturan bagian nama `ProviderName` dan bukan `example`; selain itu exit 1 dengan pesan aturan nya.
2. Tujuan `<dir>/<name>/` (default `<repo>/providers/<name>/`). Kalau sudah ada, exit 1 tanpa menulis apa pun.
3. Salin setiap file yang `read_package` baca dari `providers/example/`, ditambah `tests/scenarios.yaml` dan `README.md`, ke folder sementara di samping tujuan, dengan tiga penggantian teks saja: baris `id: testing/example` menjadi `id: testing/<name>`; baris `displayName: Example` menjadi `displayName: <Name>` (`<name>` dengan huruf pertama kapital); dan baris judul `# Example provider` di README menjadi `# <Name> provider`. Setiap penggantian wajib cocok tepat satu kali; kalau tidak, exit 1 (penjaga supaya paket contoh dan scaffold tidak lepas).
4. Jalankan `check_package` pada folder sementara; kalau `publishable`, ganti nama folder itu ke tujuan (rename), lalu cetak baris `ok` dan langkah berikutnya (`check`, `test`, README, dan untuk paket di `providers/`: `STACKLY_UPDATE_GOLDEN=1 cargo test -p stacklyd --test create_form_fixtures`, karena AC-8 menuntut satu fixture form per folder). Kalau tidak, hapus folder sementara yang baru ia buat itu dan exit 1.

Hasilnya paket `testing/<name>` yang langsung lulus `provider check` dan `provider test`.

**Dari contoh ke paket resmi** (isi README, dan urutan yang fitur 39 sampai 43 ikuti): tambah entri di `runtimes/inventory.toml` dan bukti spike (spec 0003); ganti `metadata.id` ke `official/<name>`, `software`, `category`, `runtime.executable`, lalu hapus `versions.json` tulisan tangan dan jalankan `cargo xtask codegen`; tulis `tests/scenarios.yaml` untuk mode yang didukung; `cargo xtask provider freeze`; tambah satu test native per line di `apps/daemon/tests/provider_conformance.rs`; lalu pindahkan provider dari `[[planned]]` ke `[[provider]]` di `providers/catalog.toml` (spec 0017).

### Harness

**Susunan.** Harness adalah kode test milik `stacklyd`, karena hanya di sana daemon bisa dijalankan di dalam proses (`stacklyd::server::start`) dengan fitur `fixture` dan `test-support` core, tanpa melanggar aturan "tidak ada crate yang bergantung pada library `stacklyd`".

| Bagian | Tempat | Isi |
| --- | --- | --- |
| modul bersama | `apps/daemon/tests/conformance/` (`mod.rs`, `site.rs`, `steps.rs`, `process.rs`, `report.rs`) | `run(RunRequest) -> Report`: root, daemon di dalam proses, client socket, langkah, kontrak proses, report |
| pintu umum | `apps/daemon/examples/provider_harness.rs` | binary yang memuat modul bersama lewat `#[path = "../tests/conformance/mod.rs"]`; argumen `--package <dir absolut> [--line <line>] [--scenario <name>]... --report <file> [--keep]` atau `--cleanup` |
| test bernama | `apps/daemon/tests/provider_conformance.rs` | test fixture (contoh, fake, fault, rogue) dan test native per line (AC-15), masing masing dengan guard evidence |
| paket rogue | `apps/daemon/tests/conformance/packages/fake-rogue/` | `testing/fake-rogue`, salinan paket contoh dengan settings `closeLockFd`, `reExec`, `background` (boolean, default false) yang diteruskan sebagai flag fake server |
| pembungkus | `xtask/src/provider.rs` | `provider test`: menjalankan `cargo build -p stackly-fake-server --locked`, lalu `cargo run -q --locked -p stacklyd --example provider_harness -- ...`, meneruskan exit code |

Sumber `xtask check-deps` hanya memindai target lib dan bin, jadi modul di `tests/` dan `examples/` boleh menjalankan `lsof` dan `ps`. Karena example dan test memakai bagian modul yang berbeda, deklarasi `mod conformance;` di keduanya membawa `#[allow(dead_code, reason = "shared by the provider_harness example and the provider_conformance test")]`. `RunRequest { package_dir, line, scenarios: Option<Vec<Scenario>>, only: Vec<String>, report_path, keep }`: `scenarios` yang terisi menggantikan skenario dari paket, dan hanya test yang mengisinya. Test di `provider_conformance.rs` berjalan satu per satu lewat satu mutex statis, karena masing masing menjalankan daemon dan mengukur waktu.

**Persiapan root** (satu satunya bagian yang tidak lewat socket):

1. `check_package(MacosFilesystem, dir, NameCheck::FolderName)`. Tidak `publishable`, atau namespace bukan `official` atau `testing`, berarti verdict `fail` di langkah `validate` dan selesai tanpa root.
2. Pilih artefak dari `ValidatedPackage::versions()` untuk target host: dengan `--line`, artefak line itu; tanpa `--line`, satu satunya artefak ber status `fixture` atau `available`; kalau lebih dari satu, verdict `blocked` dengan daftar line yang ada. Artefak `blocked` atau `planned` berarti verdict `blocked`.
3. Buat `TestRoot` berpenanda (*Root dan kepemilikan*), dengan nama folder data yang memuat spasi dan karakter non ASCII seperti harness native yang ada, dan satu `stackly_testkit::ReapGuard` untuk root itu.
4. Pasang runtime, mengikuti urutan `Site::open` di `apps/daemon/tests/nats/harness.rs` yang dipindah ke `conformance/site.rs`: siapkan layout (`PreparedLayout`), buka registry, pasang runtime, commit `WriteBatch` nya, lalu tutup registry. Artefak `fixture` (wajib software `fake`, selain itu `blocked`) lewat `stackly_core::supervisor::fixture::install_fake_runtime` dengan `stackly_testkit::fake_server::binary()`; artefak `available` lewat `stackly_testkit::runtime_cache` (dicari dengan `sha256` dan `size`) dan `stackly_core::runtime::fixture::install_cached_artifact`. Artefak yang tidak ada di cache berarti verdict `blocked` dengan reason `artifact <id> is not in the runtime cache` dan owner `runtime maintainer`. Tidak ada unduhan.
5. Ambil `DaemonLock`, bangun `DaemonDeps` (`LogSink`, `StartFacts`) seperti `Site::open`, lalu jalankan daemon lewat `stacklyd::server::start` dengan `DaemonConfig` yang dipendekkan dan `CatalogOverride::Preloaded(Catalog::for_tests(&[CatalogEntry::Provider(ProviderEntry { package, tile, notes })]))`. `tile` memakai nilai tetap yang sama dengan `apps/daemon/tests/postgresql/harness.rs`, dan `notes` kosong. Platform nya `MacosPlatform::with_roots(..).with_secrets(..)` dengan `stackly_testkit::FakeSecretStore`, jadi Keychain tidak pernah disentuh.

Jenis run mengikuti artefak: `fixture` untuk artefak `fixture`, `native` untuk `available`.

**Langkah skenario.** Skenario yang dijalankan: semua dari `PackageCheck.scenarios`, atau hanya yang disebut `--scenario`. Nama instance `c-<skenario>-<n>`. Setiap langkah punya batas waktu sendiri dan seluruh skenario dibatasi 15 menit.

| Langkah | Method | Lulus kalau | Batas waktu |
| --- | --- | --- | --- |
| `validate` | (persiapan) | paket `publishable`, namespace diterima | |
| `catalog` | `catalog.get_provider` | provider ada dengan revision dan `manifestSha256` paket | 10 s |
| `runtime` | `runtime.list`, `runtime.get` | runtime software dan versi artefak berstatus terpasang; ID nya dipakai langkah berikutnya | 10 s |
| `form` | `instance.get_create_form` dengan ID runtime itu | balasan terdecode, jumlah field sama dengan `SettingsSchema::fields`, tidak ada jenis field `Unrecognized` | 10 s |
| `create` | `instance.preflight`, `instance.create`, `operation.get` | operasi `succeeded`, lifecycle `stopped`, `initialized: true`; settings dari skenario di atas default form, setiap field secret `SecretSource::Generate`, setiap endpoint tcp dan udp mode port otomatis | `initSeconds` + 30 s |
| `start` | `instance.start`, `operation.get`, `instance.get` | operasi `succeeded`, lifecycle `running`, readiness `ready` | `readinessSeconds` + 10 s |
| `health` | `instance.get` (polling 500 ms) | health `healthy` | `intervalSeconds` × (`failureThreshold` + 1) + `attemptTimeoutMilliseconds`, paling lama 120 s |
| `connection` | `instance.get_detail` | jumlah `connections` sama dengan jumlah entri `connection` manifest, setiap entri ber `state` tersedia, dan `masked` nya URI absolut; `skipped` kalau manifest tanpa blok `connection` | 10 s |
| `process` | `instance.get` ditambah `lsof` dan `ps` | *Kontrak proses* | 30 s |
| `logs` | `instance.logs_read` dengan `limit: 1000` | satu halaman terbaca tanpa error; jumlah barisnya dicatat di `detail` | 30 s |
| `second_instance` | `create`, `start`, `health` untuk `c-<skenario>-2` | lulus seperti di atas, PID dan port berbeda dari instance pertama, instance pertama tetap `ready`, lalu instance kedua dihentikan dengan `instance.stop` sebelum langkah ini selesai; `skipped` kalau `multiInstance` false | sama dengan langkah asalnya |
| `reload` | `instance.reload`, `operation.get` | operasi `succeeded`, readiness kembali `ready`, PID sama; `skipped` tanpa capability `reload` | `reloadSeconds` + 10 s |
| `restart` | `instance.restart`, `operation.get` | operasi `succeeded`, `ready`, PID berbeda | `stopSeconds` + `readinessSeconds` + 10 s |
| `handover` | `daemon.restart_preview` | selalu lulus; mencatat `proven` kalau reason instance `handover_proven`, selain itu `unproven` | 10 s |
| `stop` | `instance.stop`, `operation.get` | operasi `succeeded`, lifecycle `stopped`, tanpa force stop | `stopSeconds` + 10 s |
| `leftovers` | `lsof`, `pgrep`, connect | *Kontrak proses* bagian sesudah stop | 15 s |
| `start_again` | `start`, `health`, `stop`, lalu pemeriksaan `leftovers` pada instance pertama | lulus; membuktikan instance yang sudah berhenti bisa start lagi di data yang sama (pemeriksaan kompatibilitas data spec 0025 lulus) | sama dengan langkah asalnya |

`expect: create_fails`: langkah `create` lulus kalau `instance.preflight` atau `instance.create` ditolak, atau operasi berakhir `failed`; lalu langkah sisanya `skipped`. `expect: start_fails`: `create` harus lulus, `start` lulus kalau operasi berakhir `failed` dan `instance.get` menunjukkan `lastFailure` terisi; lalu `leftovers` dijalankan dan sisanya `skipped`. Operasi yang berhasil padahal diharapkan gagal adalah `fail`. Skenario berhenti di langkah pertama yang `fail`; harness lalu menghentikan setiap instance yang masih berjalan lewat `instance.stop` (dan `instance.force_stop` kalau stop itu sendiri gagal), menjalankan `leftovers`, dan lanjut ke skenario berikutnya. Kode dan pesan error operasi yang gagal disalin ke `failure` di report.

**Kontrak proses** (jawaban Follow-up spec 0010). Kedua sampel diambil di dalam langkah `process`, berjarak 2 detik, sebelum `restart` mengganti PID. File dicocokkan lewat nomor device dan inode (field `D` dan `i` dari `lsof -F`, dibandingkan dengan `symlink_metadata` file itu), bukan lewat teks path, karena `lsof` menulis ulang byte non ASCII di path. Setiap pencarian pemegang file mengecualikan PID harness sendiri, karena daemon berjalan di proses yang sama. Sumber nilai: PID dan `startedAt` dari `instance.get` `process`; path `instance.lock` dari layout `stackly_platform` untuk root dan ID instance itu; program dari `runtime.get` (root runtime) digabung `runtime.executable` manifest, dibuat canonical.

| Pemeriksaan | Cara | Reason saat gagal |
| --- | --- | --- |
| fd 3 tetap lock instance | `process.adoptable == true`, dan `lsof -a -p <pid> -d 3 -F Di` menunjuk device dan inode `instance.lock` itu, di kedua sampel | `lock_not_held` |
| tidak re exec | entri `txt` pertama `lsof -a -p <pid> -d txt -F Di` menunjuk device dan inode program canonical, di kedua sampel | `re_exec` |
| tetap foreground | PID dan `startedAt` sama di kedua sampel, dan `ps -o pgid= -p <pid>` sama dengan PID (proses di spawn dengan `POSIX_SPAWN_SETSID`) | `not_foreground` |
| tidak ada sisa sesudah stop | `lsof -t -- <instance.lock>` tanpa PID selain harness; `pgrep -g <pgid>` kosong (`pgid` dari sampel, atau PID yang dicatat `lastFailure` kalau start gagal); connect ke setiap port endpoint tcp ditolak | `lock_still_held`, `process_left`, `port_still_open` |

Proses yang tertinggal dicatat di report (`pid`, `command` tanpa argumen) lalu diakhiri lewat `ReapGuard` root itu (SIGTERM, lalu SIGKILL sesudah 5 detik, untuk proses yang argumennya memuat path root). Ini kode test yang membersihkan sisa run nya sendiri, bukan stop normal daemon, jadi aturan `SIGKILL_ALLOWED` tidak tersentuh.

**Root dan kepemilikan.** `stackly_testkit` mendapat:

- `TestRoot::new_marked(run_id: &str) -> io::Result<TestRoot>`: seperti `new()`, lalu menulis `.stackly-test-run` di kedua foldernya (`$TMPDIR/stackly-test-*` dan `/tmp/skt-*`) berisi `{ "schema": "stackly.test-run/v1", "runId", "uid", "pid", "createdAt" }`, dan mematikan penghapusan otomatis `TempDir` untuk root itu.
- `TestRoot::remove_checked(self) -> Result<(), CleanupRefused>`: untuk tiap folder, hapus hanya kalau semuanya benar: parent canonical nya adalah folder sementara canonical (`std::env::temp_dir()` untuk yang pertama, `/private/tmp` untuk yang kedua); namanya berawalan `stackly-test-` atau `skt-`; `symlink_metadata` menunjukkan folder, bukan symlink; pemilik folder adalah uid proses ini; penanda terbaca dengan `runId` dan `uid` yang sama dengan run ini; dan tidak ada proses yang argumennya memuat path folder itu. Kalau satu saja salah, folder itu dibiarkan dan `CleanupRefused { path, reason }` dikembalikan.
- `sweep_marked() -> Vec<PathBuf>` untuk `--cleanup`: aturan yang sama kecuali `runId` bebas, ditambah syarat `pid` di penanda tidak lagi hidup (`ps -p <pid>` gagal). Proses yang masih berjalan di root seperti itu diakhiri lebih dulu dengan cara `ReapGuard`.

`TestRoot::new()` dan `Drop` nya tidak berubah, jadi test lain tidak terpengaruh. uid dibaca dari metadata folder yang baru dibuat proses ini (`std::os::unix::fs::MetadataExt`), tanpa `unsafe`.

Urutan akhir run: (1) hentikan setiap instance yang masih berjalan lewat `instance.preview_stop_all` lalu `instance.stop_all` dengan `expectedInstanceIds` dari preview itu, dan `instance.force_stop` untuk yang gagal berhenti, karena shutdown daemon membiarkan proses instance tetap berjalan; (2) shutdown `stacklyd::server`; (3) `ReapGuard` mengakhiri proses yang masih memuat path root, dan setiap proses seperti itu dicatat di `cleanup.processesLeft`; (4) tulis report; (5) `remove_checked`, kecuali `--keep`. Dengan `--keep` folder data di `$TMPDIR` dibiarkan; folder runtime pendek di `/private/tmp` tetap dibersihkan `ReapGuard`.

Sinyal: harness memasang satu penanda `signal-hook` untuk SIGINT dan SIGTERM yang diperiksa di antara langkah dan di setiap polling; sinyal pertama memicu urutan akhir run di atas dengan `interrupted: true`, sinyal kedua keluar saat itu juga dan meninggalkan root untuk `--cleanup`. Harness memakai `stacklyd::server::start`, bukan `main`, jadi handler sinyal daemon tidak terpasang. Pembungkus `xtask` mengabaikan SIGINT selama child berjalan dan meneruskan exit code nya.

**Report** `stackly.provider-conformance/v1`:

| Field | Isi | Sumber |
| --- | --- | --- |
| `schema`, `runId` | konstanta; UUID v7 | harness |
| `startedAt`, `finishedAt` | RFC 3339 UTC | jam harness (jiff) |
| `kind` | `fixture` atau `native` | status artefak terpilih |
| `host` | `{ os, osVersion, arch }` | `stackly_evidence` run context yang sama dengan record evidence |
| `sourceCommit`, `dirty` | SHA `HEAD` atau null; boolean | `git rev-parse HEAD` dan `git status --porcelain` di repo; null dan `true` kalau bukan repo git |
| `package` | `{ folder, provider, revision, manifestSha256, apiVersion }` | `PackageCheck` |
| `runtime` | `{ software, version, sha256, source }` atau null | artefak terpilih; `version` yang dilaporkan `runtime.get`; `source` `fixture` atau `managed` |
| `validation` | `{ loadable, publishable, findings }` | `PackageCheck`, bentuk `ValidationFinding` |
| `scenarios[]` | `{ name, expect, outcome, steps[], failure? }` | `outcome` `pass` atau `fail`; `steps[]` `{ name, status, durationMs, detail? }`; `failure` `{ step, reason?, code?, message? }` |
| `handover` | `proven`, `unproven`, atau null | langkah `handover` skenario pertama yang mencapainya |
| `cleanup` | `{ rootRemoved, kept, processesLeft[], refused? }` | urutan akhir run |
| `verdict` | `pass`, `fail`, `blocked` | `blocked` kalau persiapan `blocked`; `fail` kalau ada skenario `fail`, `validate` gagal, atau `cleanup` gagal; selain itu `pass` |
| `blocked` | `{ reason, owner }` atau null | persiapan |
| `interrupted` | boolean | penanda sinyal harness |

Setiap string yang memuat path melewati pengganti `~` untuk home dan `$TMPDIR` untuk folder sementara canonical. `detail` langkah hanya berisi angka, kode, dan nama; tidak pernah nilai settings teks. Report ditulis ke file sementara di folder yang sama lalu di rename.

**Test bernama** di `provider_conformance.rs`:

| Test | Kind | Covers |
| --- | --- | --- |
| `the_example_package_passes_conformance` | fixture | AC-029, AC-034, REQ-029 |
| `the_fake_package_passes_conformance` | fixture | AC-029 |
| `a_slow_ready_scenario_passes_within_the_readiness_limit` | fixture | AC-029 |
| `a_crashing_scenario_fails_at_start_unless_the_scenario_expects_it` | fixture | AC-029 |
| `a_log_spamming_scenario_still_completes` | fixture | AC-029 |
| `a_server_that_closes_fd_3_fails_the_process_contract`, `a_server_that_re_execs_fails_the_process_contract`, `a_server_that_goes_to_the_background_fails_start_and_leaves_nothing_behind` | fixture | REQ-013 |
| `an_unsupported_api_version_ends_before_any_lifecycle` | fixture | AC-030, REQ-030 |
| `a_root_without_this_runs_marker_is_never_removed` | tanpa guard | tidak ada ID PRD; menjaga AC-12 |
| `nats_2_15_passes_conformance`, `postgresql_17_...`, `postgresql_18_...`, `traefik_3_7_...` | native | AC-029, REQ-029 |

Setiap test native memuat `evidence!` dengan `Kind::Native` di badan fungsinya sendiri (aturan pindaian spec 0002), memanggil `conformance::run`, lalu mengisi subject dan `pass_with` atau `blocked` dari `Report`. Modul harness sendiri tidak pernah menulis record evidence.

### Fake server

`stackly-fake-server serve` mendapat tiga flag (default `false`), tanpa `unsafe` dan tanpa dependency baru:

| Flag | Perilaku |
| --- | --- |
| `--close-lock-fd=<bool>` | sebelum listen, mengganti dirinya lewat `exec` dengan `/bin/sh -c 'exec 3>&-; exec "$0" "$@"'` yang menjalankan ulang binary yang sama dengan argumen yang sama ditambah `--lock-fd-closed=true` (flag internal yang mencegah pengulangan) |
| `--re-exec=<bool>` | menyalin binary nya ke `<data>/fake-copy`, lalu `exec` salinan itu dengan argumen yang sama ditambah `--re-executed=true` |
| `--background=<bool>` | men spawn dirinya sendiri dengan argumen yang sama ditambah `--backgrounded=true`, lalu proses awal keluar 0 sebelum listen; proses anak mewarisi fd 3 dan process group yang sama |

Ketiganya hanya dipakai paket `testing/fake-rogue`. `providers/fake` dan `providers/example` tidak berubah karenanya, dan versi crate tidak naik.

### Spike dari paket

Jawaban Follow-up spec 0003. `xtask/src/runtime/spike/package_recipe.rs` menambah `PackageRecipe`, implementasi trait `Recipe` yang ada, dibangun dari paket di `providers/` yang `metadata.software` nya sama dengan `provider` entri inventory. Dua tanda tangan trait berubah supaya bisa diisi dari data: `recipe(repo: &Path, provider: &str) -> Result<Box<dyn Recipe>>` (memilih `PackageRecipe` kalau paket itu ada, selain itu recipe lama; paket yang tidak `publishable` membuat spike `FAIL` di langkah `recipe` dengan alasan `package <folder> is not publishable`), dan `version_args(&self) -> Vec<String>`.

| Bagian `Recipe` | Sumber |
| --- | --- |
| context | `PlanContext` dengan path dari `DirectoryLayout::instance_paths`: root data, cache, dan logs di bawah `run_dir`, dan root runtime di folder socket pendek `/tmp/sk-<8 hex>` yang sudah dibuat worker, supaya batas panjang socket terpenuhi; ID instance UUID v7 buatan worker dan nama `spike`; endpoint tcp dan udp dengan port dari `xtask/src/runtime/ports.rs`; settings skenario `default` paket (dari `check_package`) di atas default schema; setiap field secret lewat `ResolvedSecrets::for_tests` dengan nilai ber awalan credential fixture spec 0002 |
| `init` | `init_plan`: `EnsureDirectories` membuat folder; `WriteFile` menulis `RenderedContent::materialize`; `MaterializeSecret` menulis file 0600; `Run` lewat `ctx.run_logged` dengan stdin dan `successExitCodes` plan; lalu hapus `InitPlan.cleanup`. `match` atas `InitStep` exhaustive |
| `start` | `run_plan`: tulis setiap config di `RunPlan.render`, lalu `Command` dari program, args, env, dan cwd plan |
| `ready` | satu percobaan `stackly_core::supervisor::probe::run` atas probe readiness plan, dengan adapter `stackly_platform_macos` dan `TcpConnect`, diulang sampai batas 120 detik spec 0003 |
| `version`, `normalize`, `version_args` | `runtime.versionCheck` paket: `args`, lalu capture group `pattern` |
| `stop` | plan stop: sinyal `gracefulProcess`, atau command stop dengan `successExitCodes` nya |
| `probes` | probe closure artefak tetap di xtask, dipindah ke `spike/closure_probes.rs` dan dipilih dari nama provider: empat probe PostgreSQL AC-7 spec 0003 dan probe JetStream NATS. Ini pengetahuan tentang artefak, bukan lifecycle provider. Mereka menerima `SpikeFacts { endpoints, settings, secrets }` dari context di atas: probe PostgreSQL memakai socket dan port endpoint, nama user dari settings, dan password fixture lewat env `PGPASSWORD` |
| `uses_socket_dir`, `log_files` | ada endpoint `unix` di paket; `logs.files` manifest |

`xtask` mendapat dependency `tokio` (fitur `rt`, `net`, `time`; runtime satu thread untuk memanggil `probe::run` dengan platform `stackly_platform_macos`, `Redactor` yang memuat secret fixture itu, dan `SupervisorConfig::default()`), dan `stackly-core` nya menyalakan fitur `test-support` di samping `json-schema`. Hasil spike, urutan langkah worker, audit, dan batas waktu spec 0003 tidak berubah. Paket yang tidak punya `runtime.versionCheck` membuat spike `FAIL` di langkah `recipe` dengan alasan `package has no versionCheck`.

### Handover

Jawaban Follow-up spec 0030. Lulus harness tidak pernah membuat provider masuk `HANDOVER_PROVEN`. Provider baru yang mau bertahan melewati restart daemon terencana menambah test native launchd untuk setiap line di `apps/daemon/tests/native_launchd.rs` (restart terencana dan `SIGKILL` daemon, spec 0030 AC-8), dan baru sesudah record nya `PASS` menambah entri `HANDOVER_PROVEN`. Sampai itu terjadi report harness mencatat `handover: unproven`, dan instance provider itu memakai jalur `stop_restore`. Provider tanpa entri tetap sah masuk catalog.

### API surface

| Fungsi atau perintah | Tempat | Input utama | Output utama | Error utama |
| --- | --- | --- | --- | --- |
| `check_package` | `core::provider::check` | `&dyn Filesystem`, folder, `NameCheck` | `PackageCheck` | tidak ada (semua jadi finding) |
| `ValidatedPackage::authoring_findings` | `core::provider::check` | tidak ada | `Vec<Finding>` | tidak ada |
| `read_scenarios`, `Scenarios::parse`, `Scenarios::validate` | `core::provider::scenarios` | folder; teks; settings schema | teks; `Vec<Scenario>` | `Issue` |
| `provider.validate` | `stacklyd` `ipc::provider` | `ProviderValidateParams` | `ProviderValidation` | `request.invalid_params` |
| `stackly provider validate <dir> [--json]` | `apps/cli` | path | baris AC-4 atau envelope | exit 2 `cli.provider_invalid`, exit 6 daemon tidak terjangkau |
| `cargo xtask provider check [<dir>...]` | xtask | folder paket | baris AC-4 | exit 1 |
| `frozen_facts` | `core::provider::testing` (fitur `test-support`) | paket, skenario | `FrozenFacts` | `PlanError` |
| `cargo xtask provider new <name> [--out <dir>]` | xtask | nama | folder paket baru, baris `ok` | exit 1 |
| `cargo xtask provider freeze <dir>` | xtask | folder paket `official/` | entri corpus | exit 1 |
| `cargo xtask provider test <dir> [--line] [--scenario]... [--report] [--keep]`, `--cleanup` | xtask, example `provider_harness` | folder paket | `report.json`, ringkasan teks | exit 1 `fail`, exit 2 `blocked` |
| `TestRoot::new_marked`, `remove_checked`, `sweep_marked` | `stackly-testkit` | `runId` | root berpenanda | `CleanupRefused` |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| validate | daftar versi yang didukung di pesan dan DTO | `SUPPORTED_API_VERSIONS` |
| validate | `folder` | nama folder terakhir path input |
| validate | `identity` | `ValidatedPackage::key()` dan manifest |
| validate (CLI) | path absolut | argumen digabung folder kerja CLI |
| freeze | `startArgs`, `startEnvKeys`, `rendered` | `frozen_facts` dengan *Context beku* |
| form fixture | nama tampil, versi, line runtime | `metadata.displayName`; artefak pertama `versions.json` untuk target host |
| scaffold | `<Name>` | `<name>` dengan huruf pertama kapital |
| harness | `runId` | UUID v7 yang dibuat harness |
| harness | artefak | `ValidatedPackage::versions()`, target host, `--line` |
| harness | settings create | default `instance.get_create_form`, ditimpa `settings` skenario |
| harness | secret | daemon (`SecretSource::Generate`); harness tidak pernah melihat nilainya |
| harness | port | daemon (mode otomatis spec 0020) |
| harness | batas waktu langkah | blok `timeouts`, `readiness`, dan `health` manifest (dengan default spec 0009) |
| harness | PID, `startedAt`, `adoptable` | `instance.get` `process` |
| harness | file `instance.lock`, program | layout `stackly_platform` untuk root dan ID instance; root runtime dari `runtime.get` digabung `runtime.executable`; keduanya dibandingkan lewat device dan inode |
| harness | `pgid` untuk `leftovers` | `ps -o pgid=` di sampel pertama; PID dari `lastFailure` kalau start gagal, dan kalau tidak ada, hanya pemeriksaan lock dan port |
| harness | `handover` | `daemon.restart_preview` |
| harness | `sourceCommit`, `dirty` | git di folder repo (`CARGO_MANIFEST_DIR` naik dua tingkat) |
| harness | `host` | context run `stackly-evidence` |
| harness | folder report default | `<target directory dari cargo metadata>/provider-test/<runId>/` |
| test native | subject | versi dari `runtime.get`, digest artefak dari `versions.json` |
| spike | settings | skenario `default` paket |

### Key invariants

- `check_package`, `provider.validate`, `provider check`, `provider new`, dan `provider freeze` tidak pernah menjalankan proses provider, menyelesaikan secret, atau menyentuh jaringan.
- `read_package` dan `from_files` tidak berubah karena fitur ini; pengetatan baru hanya lewat `authoring_findings()`.
- Entri corpus tidak pernah diubah atau dihapus, dan setiap perubahan engine harus lulus test corpus tanpa mengubah `frozen.json`.
- Harness hanya berjalan di root berpenanda yang ia buat sendiri, tidak pernah di root Application Support, dan tidak pernah memakai Keychain login.
- Harness menghapus folder hanya lewat `remove_checked`, dan tidak pernah menghapus path yang diberikan pengguna.
- Sesudah persiapan root, harness hanya memakai method kontrak; ia tidak memanggil core untuk create, start, atau stop.
- Setiap proses provider di harness di spawn daemon lewat `ProcessSupervision`. CLI tidak pernah menjalankan binary provider.
- Lulus harness bukan bukti runtime native kalau artefaknya `fixture`, dan tidak pernah menambah `HANDOVER_PROVEN`.
- Report dan output perintah tidak pernah memuat nilai secret atau nilai settings teks.

### Security model

- `provider.validate` membuat daemon membaca folder yang disebut client. Client sudah terbukti user yang sama (spec 0006), pembacaan dibatasi `read_package` (4 MiB, tanpa symlink, hanya file yang dikenal) ditambah satu file skenario 64 KiB, dan balasan hanya memuat finding serta identitas, tidak pernah isi file. Pesan issue tidak memuat nilai settings teks atau secret (spec 0009).
- Validasi tidak memberi trust. Paket yang `publishable` belum boleh dijalankan di root pengguna; itu keputusan import dan approval fitur 37.
- Harness menjalankan executable native dengan hak user yang menjalankannya. Untuk paket `official/` executable itu berasal dari artefak yang digest nya dipin inventory; untuk paket `testing/` hanya `stackly-fake-server`. Harness menolak namespace lain, jadi paket pihak ketiga tidak pernah dijalankan lewat jalur ini.
- `tests/scenarios.yaml` hanya membawa nilai settings bukan secret, yang divalidasi schema paket; ia tidak bisa menyebut executable, path, atau perintah.
- Penghapusan folder mengikuti *Root dan kepemilikan*, jadi path yang dipasok atau symlink tidak bisa mengarahkan harness menghapus folder lain.

### Configuration required

Tidak ada variabel environment, setting, credential, atau migrasi registry baru. Dependency baru: `tokio` untuk `xtask` (sudah ada di `[workspace.dependencies]`), fitur `test-support` `stackly-core` untuk `xtask`, dan `signal-hook` sebagai dev dependency `stacklyd` (sudah ada di workspace). Satu entri baru di `EXPOSE_ALLOWED` (AC-16). Protokol naik satu minor untuk `provider.validate`; nomor ditetapkan lead saat build.

### Critical test scenarios

- Versi tidak didukung: paket dengan `apiVersion: stackly/v2` di `check_package`, `provider.validate`, `provider check`, catalog, dan harness; satu finding dengan pesan yang menyebut `stackly/v1alpha1`, tanpa mutasi dan tanpa spawn. Memverifikasi **AC-1**.
- Validator: `fake-command` `loadable` tetapi tidak `publishable`; paket rusak memberi finding `package`; `from_files` atas salinan pinned lama tetap berhasil. Memverifikasi **AC-2**.
- Skenario: field asing, nama ganda, key secret, settings di luar batas, file 65 KiB, symlink; dan digest paket tidak berubah saat file itu berubah. Memverifikasi **AC-3**.
- `provider check`: format baris lama tetap, baris `warning`, dan paket `[[provider]]` yang belum dibekukan. Memverifikasi **AC-4**.
- `provider.validate`: fixture kontrak Rust dan Swift, path relatif ditolak, folder tidak ada menjadi `loadable: false`, 150 finding dipotong ke 100, registry tidak berubah; CLI exit 0 dan 2. Memverifikasi **AC-5**.
- Corpus: setiap entri termuat dengan digest dan hasil yang sama; `freeze` menolak digest berbeda di revision yang sama. Memverifikasi **AC-6**.
- Scaffold: nama tidak valid, folder sudah ada, hasil valid, dan hanya tiga baris yang berbeda. Memverifikasi **AC-7**.
- Form: fixture untuk setiap folder `providers/`, dan test Swift generik. Memverifikasi **AC-8**.
- Harness lulus untuk paket contoh dan fake, dengan langkah `skipped` yang benar untuk capability yang tidak ada. Memverifikasi **AC-9**.
- Rogue: tiga pelanggaran kontrak proses masing masing gagal dengan reason nya, dan tidak ada proses tersisa. Memverifikasi **AC-10**.
- Fault: ready lambat, crash dengan dan tanpa `expect`, spam log. Memverifikasi **AC-11**.
- Cleanup: root tanpa penanda, penanda `runId` lain, symlink ke folder lain, `--keep`, `--cleanup` dengan pid hidup dan mati. Memverifikasi **AC-12**.
- Report: schema, sanitasi path, exit code, baris terakhir stderr. Memverifikasi **AC-13**.
- Provider baru dari scaffold di luar tree repo lulus tanpa perubahan repo. Memverifikasi **AC-14**.
- Native per line, dan `BLOCKED` tanpa artefak di cache. Memverifikasi **AC-15**.
- `PackageRecipe` terhadap fake server; recipe lama hilang untuk tiga provider. Memverifikasi **AC-16**.

Test yang membuktikan ID PRD memasang guard evidence spec 0002. Record fixture yang lulus bukan bukti runtime native.

## Build plan

Urutan mengikuti pendekatan Journey: jalur kontributor dibangun berurutan dari memeriksa paket, membuat paket, menguji paket, sampai bukti native.

1. `core::provider::check` dan `core::provider::scenarios`: `SUPPORTED_API_VERSIONS`, pesan `unsupported_api_version`, `Finding`, `PackageCheck`, `check_package`, `authoring_findings`, parser dan validasi skenario, fuzz target `provider_scenarios`, dengan unit test. Memenuhi **AC-1** (bagian core), **AC-2**, **AC-3**.
2. `cargo xtask provider check` memakai `check_package` dan baris `warning`. Memenuhi **AC-4** (tanpa pemeriksaan corpus), **AC-1** (bagian xtask).
3. DTO dan method `provider.validate` di `stackly-protocol` (bump minor, `cargo xtask codegen`, fixture kontrak, `cargo xtask fuzz seed`), handler `ipc::provider` di `stacklyd`, test Swift fixture di `clients/swift`. Memenuhi **AC-5** (bagian daemon), **AC-1** (bagian method dan catalog).
4. `stackly provider validate` di `apps/cli` dengan `cli.provider_invalid`. Memenuhi **AC-5** (bagian CLI).
5. Corpus: `core::provider::testing::frozen_facts`, `cargo xtask provider freeze`, entri awal untuk setiap paket `[[provider]]`, test `provider_compat`, dan pemeriksaan `not frozen` di `provider check`. Memenuhi **AC-6**, **AC-4**.
6. Paket `providers/example/` dengan `tests/scenarios.yaml`, README, golden plan dan render nya di test `provider_packages`. Memenuhi **AC-7** (bagian paket).
7. `cargo xtask provider new`. Memenuhi **AC-7**.
8. `create_form_fixtures` untuk setiap folder `providers/` dengan fakta runtime dari paket (fixture yang ada ditulis ulang), dan test `StacklyUI` generik atas `create-form-*.json`. Memenuhi **AC-8**.
9. `stackly-testkit`: `TestRoot::new_marked`, `remove_checked`, `sweep_marked`. Memenuhi **AC-12** (bagian testkit).
10. Modul `apps/daemon/tests/conformance/`: persiapan root, daemon di dalam proses, langkah `catalog` sampai `stop` dan `start_again`, report. Test `the_example_package_passes_conformance` dan `the_fake_package_passes_conformance`. Memenuhi **AC-9**, **AC-13** (bagian report).
11. Kontrak proses dan `leftovers`; flag rogue di `stackly-fake-server`; paket `testing/fake-rogue`; tiga test rogue. Memenuhi **AC-10**.
12. Skenario fault dan test nya; test versi tidak didukung dan penanda root. Memenuhi **AC-11**, **AC-1** (bagian harness), **AC-12**.
13. Example `provider_harness` dan `cargo xtask provider test` termasuk `--cleanup` dan `--keep`; test `xtask` scaffold ke harness. Memenuhi **AC-13**, **AC-14**, **AC-12**.
14. Test native per line di `provider_conformance.rs`. Menjalankannya butuh artefak di cache xtask dan persetujuan kamu untuk run native; tanpa itu record nya `BLOCKED`. Memenuhi **AC-15**.
15. `PackageRecipe`, perubahan tanda tangan `recipe` dan `version_args`, `closure_probes.rs`, fitur `test-support` dan `tokio` di `xtask`, entri `EXPOSE_ALLOWED`, penghapusan recipe NATS, PostgreSQL, dan Traefik, dan penanda. Run spike native (`cargo test -p xtask --test native_runtime -- --ignored`) butuh sesi login GUI dan persetujuan kamu; ditulis sebagai langkah manual. Memenuhi **AC-16**.

**Sesi paralel.** Sesudah langkah 1 masuk, tiga jalur bisa dikerjakan sesi terpisah tanpa file beririsan: (a) langkah 3 dan 4 (`crates/protocol/`, `protocols/`, `clients/swift/`, `apps/daemon/src/ipc/`, `apps/cli/`); (b) langkah 6 dan 8 (`providers/example/`, `apps/daemon/tests/create_form_fixtures.rs`, `protocols/fixtures/instance/`, `StacklyUI`), dengan catatan (a) dan (b) sama sama menyentuh `protocols/fixtures/` tetapi di subfolder berbeda; (c) langkah 9 (`crates/testkit/`) dan langkah 15 (`xtask/src/runtime/spike/`), yang hanya butuh langkah 1. Langkah 2, 5, 7, dan 13 semuanya menyentuh `xtask/src/provider.rs`, jadi dikerjakan satu sesi berurutan. Langkah 10 sampai 12 dan 14 menyentuh `apps/daemon/tests/conformance/` dan `provider_conformance.rs`, jadi satu sesi berurutan sesudah langkah 6 dan 9.

## Consequences

**Positive**:

- Satu fungsi `check_package` dipakai xtask, daemon, CLI, harness, spike, dan import fitur 37, jadi tidak ada dua validator yang bisa berbeda pendapat.
- Kebijakan additive punya penjaga yang bisa dijalankan: corpus beku gagal begitu engine mengubah hasil paket lama.
- Kontributor mulai dari paket yang langsung lulus, dan lima provider berikutnya menambah folder paket, satu file fixture form, dan beberapa baris test native, bukan kode core atau SwiftUI.
- Tiga Follow-up lama tertutup: kontrak proses spec 0010 diperiksa untuk setiap provider, pengetahuan lifecycle di spike berpindah ke paket, dan aturan probe native tertulis.

**Negative / tradeoffs**:

- `stackly provider test` di tabel CLI PRD dan di deskripsi fitur 36 tidak dibuat; perintah test provider hanya ada untuk yang punya checkout repo.
- Penulis paket pihak ketiga tanpa checkout hanya mendapat validator, bukan harness, sampai ada keputusan lain.
- Harness memasang runtime lewat fungsi fixture, jadi ia tidak menguji unduhan dan installer `runtime.install`; itu tetap dibuktikan test spec 0018.
- Tanpa field versi kedua, paket yang lebih baru dari build pembacanya ditolak field demi field, bukan dengan satu kalimat "butuh Stackly yang lebih baru".
- Modul harness dimuat example lewat `#[path]` ke folder `tests/`, pola yang belum ada di repo ini.
- `PackageRecipe` adalah pelaksana plan kedua di luar supervisor. Ia kecil dan `match` nya exhaustive, tetapi primitive init baru tetap harus ditambahkan di dua tempat.
- `xtask` menyalakan fitur `test-support` core dan mendapat satu entri `EXPOSE_ALLOWED`, supaya spike bisa memberi secret fixture ke init paket. Ini melebarkan daftar penjaga secret, walau hanya untuk tooling dan hanya untuk nilai fixture.
- Fixture form `fake`, `nats`, dan `postgresql` ditulis ulang sekali, karena fakta runtime nya kini dibaca dari paket.
- Recipe MySQL, ClickHouse, dan Valkey tetap ada sampai fitur 40, 42, dan 43.
- Corpus bertambah satu folder per revision paket resmi dan tidak pernah menyusut.

**Neutral**:

- `cargo xtask provider check` tanpa argumen kini juga menuntut paket resmi dibekukan, jadi setiap kenaikan revision paket resmi membawa `cargo xtask provider freeze` di commit yang sama.
- `providers/` mendapat folder kedua ber namespace `testing` (`example`), yang seperti `fake` tidak pernah masuk catalog app.
- Test native per provider yang sudah ada (`apps/daemon/tests/nats/`, `postgresql/`, `traefik/`) tidak dihapus; test konformitas berjalan di sampingnya.

## Follow-up

- [ ] Lewat lead, untuk `docs/scope/06-menambah-provider.md`: deskripsi fitur 36 menyebut `stackly provider test`; spec ini memutuskan harness sebagai `cargo xtask provider test`. Usul: ganti nama di deskripsi, atau catat `stackly provider test` di Deferred.
- [x] Update spec 0003 (pemiliknya): tabel *Langkah spike per provider* menjadi "dari paket provider" untuk NATS, PostgreSQL, dan Traefik; versi teramati dibaca lewat `runtime.versionCheck` paket; baris Follow-up "Fitur 36" dicentang; penanda sisa menjadi F40, F42, F43. Diterapkan di spec 0003 (2026-11-11): AC-5 dan *Spike dari paket provider*. Benturan probe `icu` AC-7 spec 0003 dengan `initdb` paket PostgreSQL (`--locale-provider=builtin`) ditutup lead 2026-11-11: probe `icu` membuat database percobaan ber provider ICU (spec 0003 AC-7 dan butir *Probe `icu`* di *Spike dari paket provider*), jadi langkah 15 memindahkan ketiga provider. Developer langkah 15 membaca bagian spec 0003 itu untuk probe, superuser `postgres`, dan `env` plan; untuk probe closure teks spec 0003 yang mengikat, termasuk nama user `postgres` yang tetap (baris *Spike dari paket* di sini yang menulis "nama user dari settings" dibetulkan lewat `/sync`).
- [ ] Update spec 0009 (pemiliknya): baris Follow-up "Fitur 36" dicentang dengan rujukan ke *Kebijakan kompatibilitas* spec ini; komentar `tests/` di *Paket provider* menyebut `tests/scenarios.yaml`; `Issue.file` boleh bernilai `tests/scenarios.yaml`; pesan `unsupported_api_version` yang baru.
- [ ] Update spec 0010, 0023, 0030, dan 0014 (pemiliknya): centang baris Follow-up "Fitur 36" masing masing dengan rujukan ke *Kontrak proses*, *Probe native*, *Handover*, dan AC-9 serta AC-14 spec ini.
- [ ] Update spec 0006 dan 0008 (pemiliknya): method `provider.validate` (`recovery allowed`) dan perintah `stackly provider validate` dengan `cli.provider_invalid`.
- [ ] Spec 0037: pakai `check_package` dan bentuk `ValidationFinding` (sudah diputuskan di sana, termasuk bahwa import menolak finding `authoring`); yang masih terbuka: apakah paket lokal ber namespace lain boleh diuji harness sesudah approval.
- [ ] Spec 0038: `docs/providers/` merangkum *Kebijakan kompatibilitas* dan alur lima perintah; `providers/example/README.md` menautkannya.
- [ ] Fitur 39 sampai 43: setiap provider menambah paket lewat `cargo xtask provider new`, satu test native per line di `provider_conformance.rs`, dan `cargo xtask provider freeze`; fitur 40, 42, dan 43 menghapus recipe spike nya. Masing masing memutuskan apakah test BR-025 (`compatibility.json`, spec 0018) nya adalah test konformitas.
- [ ] `/sync` setelah build: catat di root `AGENTS.md` perintah `provider new`, `provider test`, `provider freeze`, aturan corpus, aturan `versions.json` tulisan tangan untuk namespace `testing`, bahwa `tests/scenarios.yaml` kini dibaca validator, harness, dan spike (loader tetap tidak membacanya), dan bahwa pengetatan penulis hanya lewat `authoring_findings()`.
- [ ] Pemilik `stackly-testkit` (opsional): pertimbangkan penanda kepemilikan untuk setiap `TestRoot`, bukan hanya root harness.

## Rationale

Konteks, opsi yang ditimbang, dan alasan tiap keputusan ada di [rationale.md](rationale.md).
