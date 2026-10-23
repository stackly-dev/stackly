# 0024. Instance kedua dari runtime yang sama

**Date**: 2026-10-22
**Status**: In Progress

## Summary

Spec ini menetapkan cara kamu membuat instance kedua yang memakai runtime yang sama persis dengan instance yang sudah ada, misalnya `pg17-test` di samping `pg17-dev` dari satu PostgreSQL 17 terpasang. Ada dua jalan: create biasa dari spec 0020 (memilih runtime yang sama), atau Duplicate… yang mengisi form create dari config instance sumber. Duplicate hanya menyalin pengaturan, tidak pernah data: daemon membangun draft create dari config revision sumber lewat satu method baca baru, lalu create berjalan lewat preflight dan operasi create spec 0020 yang sama, sehingga ID, folder data, port, dan secret selalu baru dan tervalidasi. Port draft selalu Automatic dan secret selalu dibangkitkan baru, kecuali kamu sendiri memilih port Fixed atau Copy from; bukti isolasinya adalah sentinel (nilai penanda) yang tidak pernah terbaca dari instance lain, di fake provider dan di PostgreSQL 17 nyata.

## Requirements

**User stories**:

- Sebagai pengembang yang sudah punya `pg17-dev`, saya ingin membuat `pg17-test` dari runtime PostgreSQL 17 yang sama tanpa memasang runtime lagi, supaya dua database jalan bersamaan dengan data dan port masing masing.
- Sebagai pengguna, saya ingin Duplicate… yang mengisi form dengan pengaturan instance yang sudah saya setel, supaya saya tidak mengulang isian, tetapi tanpa pernah menyalin data atau memakai password yang sama diam diam.
- Sebagai pengguna, saya ingin melihat preflight yang sama dengan create sebelum menekan Create, supaya port dan folder baru sudah terbukti aman sebelum apa pun dibuat.
- Sebagai penulis script, saya ingin `stackly instance duplicate pg17-dev pg17-test` dengan flag dan output yang sama dengan `stackly instance create`, supaya script dan GUI memberi hasil yang sama.
- Sebagai pengguna keyboard atau VoiceOver, saya ingin Duplicate… lewat ⌘D atau menu More dan menyelesaikan sheet tanpa mouse.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-025):

- **AC-1** (form duplicate): `instance.get_duplicate_form { instanceId }` (`recovery refused`, hanya membaca, tanpa idempotency key) membalas `DuplicateForm { source, form, draft, secrets }` seperti *Method IPC*. `form` dibangun dari paket pinned provider revision instance sumber (`form.provider.source` `pinned`, `revision` dan `manifestSha256` sama dengan provider revision key sumber) dan record runtime sumber; `draft.runtimeId` sama dengan runtime ID sumber dan `draft.provider` berisi nama dan revision sumber. Pemeriksaan berurutan dengan kode: instance tidak ada `resource.not_found`; instance retained `supervisor.instance_retained`; paket pinned hilang, tidak valid, atau digest nya beda `supervisor.package_unavailable` (detail `provider` teks provider revision key, `reason` salah satu dari tiga teks `load_package` spec 0010: "the pinned package cannot be read: …", "the pinned package is invalid: …", "the pinned package has another digest than the instance records"); catalog yang dimuat punya entri provider dengan nama dan label revision yang sama tetapi `manifestSha256` lain `instance.provider_ambiguous` (detail `provider`, `revision`), karena create akan memakai paket catalog itu dan instance baru mendapat provider revision key lain; paket tanpa capability `multi_instance` `instance.capability_unsupported` (detail `capability` `multi_instance`, `provider`, `existing` nama sumber); runtime tidak ada `instance.runtime_not_found` (detail `runtimeId`); runtime bukan `installed` `supervisor.runtime_unavailable` (detail `runtimeId`, `state`). Dengan `FakePlatform` dan `FakeRegistry`, method ini tidak menambah `mutations()`, commit registry, tulisan Keychain, spawn, atau bind percobaan, dan tetap berhasil walaupun folder instance sumber (`instances/<id>/`, folder log, dan folder run) tidak ada sama sekali. Dengan 10 instance terdaftar, balasannya datang dalam 300 ms pada persentil 95 di Mac ini.
- **AC-2** (settings yang disalin): `draft.settings` adalah `values` config revision `active` sumber yang dibatasi ke field settings schema paket pinned yang bukan secret dan bukan managed, apa adanya (termasuk field `immutableAfterInit` dan nilai yang sama dengan default); key di luar schema, field secret, dan field managed tidak ikut. Tanpa config revision aktif, `draft.settings` adalah object kosong; `values` yang bukan object JSON adalah registry rusak (`RegistryError::Corrupt { table: "config_revisions" }`, kode area `registry`). `draft.displayName` selalu kosong dan `draft.start` selalu false. Test tabel `build_duplicate` membuktikan setiap aturan dengan paket fake, PostgreSQL, dan NATS (field managed `serverName` tidak ikut).
- **AC-3** (nama): `draft.name` dan `form.suggestedName` mengikuti *Nama duplicate*: `<dasar>-copy`, lalu `<dasar>-copy-2` sampai `<dasar>-copy-99` selama nama itu dipakai instance yang tidak retained; `<dasar>` adalah nama sumber tanpa akhiran `-copy` atau `-copy-<n>`, dipotong supaya hasil paling panjang 63 karakter tanpa tanda hubung di ujung. Kalau semua kandidat dipakai, `form.suggestedName` null dan `draft.name` string kosong. Test tabel memuat `pg17-dev`, `pg17-dev-copy`, `pg17-dev-copy-7`, nama sumber 63 karakter, dan kasus semua kandidat dipakai.
- **AC-4** (port, BR-005): setiap endpoint IP manifest di `draft.endpoints` meminta `auto`, dalam urutan form, apa pun permintaan port sumbernya; daemon tidak pernah mengisi port `Fixed` di draft duplicate, sehingga hanya permintaan auto yang menghasilkan port pilihan Stackly. Port `Fixed` hanya datang dari pengguna (Fixed di sheet atau `--port`) dan melewati *Pemeriksaan port* spec 0020 apa adanya: port `Fixed` yang sama dengan port sumber ditolak `supervisor.endpoint_conflict` dengan `otherInstance` sumber, juga saat sumber `stopped`. Setelah instance duplicate di start, port efektifnya berbeda dari port efektif setiap instance tidak retained lain (spec 0010 AC-17), tersimpan di `bound`, dan dipakai ulang di start berikutnya selama masih bebas.
- **AC-5** (kebijakan secret, Follow-up spec 0011): setiap secret field di `draft.secrets` bersumber `generate`, dan `secrets` balasan mendaftar `{ field, sourceReference }` dengan reference yang dipakai sumber untuk field itu (dari `secret_uses_by`, null kalau tidak ada). Draft, GUI, dan CLI tanpa flag `--secret` tidak pernah mengirim `copyFrom` atau `bind`: `copyFrom` hanya terkirim setelah aksi pengguna (pilihan Copy from di `SecretField`, atau `--secret <field>=copy` atau `copy:<ref>`), dan `bind` hanya lewat `--secret <field>=bind:<ref>` yang diketik pengguna. Duplicate dengan draft apa adanya membuat reference baru `local:instance/<id baru>/<purpose>` yang nilainya berbeda dari nilai sumber; dengan `copyFrom` reference sumber, reference baru berisi nilai yang sama, reference sumber tidak berubah (revision dan daftar pemakaian sama), dan instance baru tidak memakai reference sumber.
- **AC-6** (create, identitas baru, REQ-009): instance kedua dari runtime yang sama, lewat create biasa atau dari draft duplicate, melewati `instance.preflight` dan `instance.create` spec 0020 tanpa perubahan kode atau kontrak create, dengan kode error dan `fields` yang sama untuk params yang sama. Hasilnya: ID baru, runtime ID sama persis dengan instance pertama, provider revision key sama persis dengan sumber (untuk duplicate), tujuh path `DirectoryLayout::instance_paths(id)` yang semuanya turunan ID baru, ada di disk, dan tidak ada yang sama atau bertumpuk dengan milik instance lain (test menurunkannya dari root test seperti `Site::instance_paths`, karena wire tidak membawa daftar path), folder run dan `socketPath` di `instance.get` berbeda, config revision baru milik operasi create itu, `startup` `manual`, dan pemakaian secret hanya ke reference baru atau ke reference yang pengguna `bind` secara eksplisit.
- **AC-7** (data lama tidak pernah disalin, AC-025, BR-008): duplicate tidak pernah membaca isi, mendaftar, menyalin, atau menulis apa pun di pohon instance sumber; satu satunya sentuhan ke pohon itu adalah pemeriksaan identitas folder data setiap instance yang dikenal oleh `check_data_root_overlap` di create (spec 0020 AC-6), yang hanya membaca metadata folder itu sendiri. Dengan sumber `stopped`, daftar (path relatif, ukuran, sha256, waktu ubah) pohon `instances/<id sumber>/` dan folder log sumber sama persis sebelum `instance.get_duplicate_form` dan sesudah instance duplicate dibuat, di start, lalu di stop; duplicate tetap berhasil saat setiap file dan subfolder di dalam folder data sumber dibuat mode 000 selama form, create, dan start duplicate (folder data itu sendiri tetap 0700, supaya pemeriksaan tumpukan create tetap berjalan dan tidak dilewati). Folder data duplicate hanya berisi hasil init provider: sentinel yang ditulis ke sumber sebelum duplicate tidak terbaca dari duplicate (`GET /sentinel` fake menjawab 404), dan di PostgreSQL tabel `stackly_sentinel` tidak ada di duplicate dan `system_identifier` kedua cluster berbeda (AC-9).
- **AC-8** (dua instance dari runtime yang sama, fixture, REQ-009, bagian AC-009): dengan fake provider, dua instance dari runtime ID yang sama (satu create biasa, satu duplicate) dan instance ketiga dari runtime yang sama dengan `crashAfterMs` berjalan bersamaan. Masing masing menyimpan sentinel berbeda lewat `PUT /sentinel` dan hanya membaca miliknya lewat `GET /sentinel`; stop instance pertama tidak mengubah PID, lifecycle, dan readiness instance kedua, dan setelah instance pertama di start lagi setiap instance tetap membaca sentinel nya sendiri; hal yang sama berlaku saat instance kedua dihentikan lalu di start; crash loop instance ketiga tidak mengubah PID dan readiness kedua instance lain; dan daftar isi folder runtime (path relatif, ukuran, sha256) sama persis sebelum start pertama dan sesudah semua instance berhenti.
- **AC-9** (PostgreSQL 17 nyata, bagian AC-009, JOURNEY-02): di Mac ini, lewat daemon nyata, `pg17-dev` dibuat dengan create biasa dan port `Fixed`, dan `pg17-test` lewat `instance.get_duplicate_form` lalu `instance.create` atas draft itu dengan `copyFrom` reference password sumber (jalur yang sama dengan `stackly instance duplicate pg17-dev pg17-test --secret superuserPassword=copy`), keduanya dari runtime ID PostgreSQL 17 yang sama, lalu keduanya di start sampai `ready` bersamaan dengan port, folder data, dan socket yang berbeda. Lewat *Helper sentinel*, setiap cluster menulis sentinel berbeda dan hanya membaca miliknya, `system_identifier` keduanya berbeda, dan setelah tiap instance dihentikan lalu di start lagi secara bergantian (bukan restart, fitur 26) baca silang tetap bersih. Test menulis evidence `native` dengan `covers = ["REQ-009", "AC-009", "AC-025"]` (hanya ID berpola `AC`, `REQ`, `NFR`, `VAL`, atau `BR` yang diterima `stackly-evidence`, jadi JOURNEY-02 disebut di teks `expected`, bukan di `covers`) dan mencatat di `steps` bahwa bagian mayor 18 AC-009 dibuktikan spec 0025. Tanpa artefak PostgreSQL 17 di cache runtime xtask hasilnya `BLOCKED` dengan alasan `artifact <id> is not in the runtime cache` dan owner `@ojiepermana`.
- **AC-10** (kontrak): method `instance.get_duplicate_form` (`recovery refused`) dan DTO di *Method IPC* ada di `stackly-protocol` dengan fixture `protocols/fixtures/` untuk varian `Call` dan `Reply` baru, plus `protocols/fixtures/instance/duplicate-form-fake.json` dan `duplicate-form-postgresql.json` yang dibangkitkan dari paket nyata dan skenario *Skenario fixture duplicate* lalu dijaga test Rust, terbaca di Rust dan Swift, dengan kenaikan minor `ProtocolVersion::CURRENT` (minor `CURRENT` di main ditambah 1 saat merge) di commit yang sama dan `cargo xtask codegen --check` bersih. Tidak ada kode error baru, dan params serta reply `instance.create` dan `instance.preflight` tidak berubah.
- **AC-11** (CLI): `stackly instance duplicate <source> [<name>]` dengan flag di *CLI* menerima nama atau UUID sumber dengan resolusi yang sama dengan `stackly instance inspect` (spec 0022, `cli.instance_not_found`), membangun draft dari `instance.get_duplicate_form`, menerapkan flag di atasnya, lalu mengirim `instance.preflight` untuk `--dry-run` atau `instance.create` untuk selain itu, dengan output, exit code, progress, `--start`, `--wait`, dan key start turunan sama dengan `stackly instance create` (spec 0020) ditambah key `source` di JSON. Flag `--runtime`, `--provider`, dan `--revision` tidak ada, jadi clap menolaknya (`cli.usage`, exit 2). Untuk matriks input buruk di *Critical test scenarios*, `--json` dan `stackly-ipc` `Client` dengan params create yang sama menghasilkan `ErrorBody` dengan `code`, `category`, `details`, dan `fields` yang sama.
- **AC-12** (sheet mode duplicate, PAGE-04): Duplicate… membuka `NewInstanceSheet` dalam mode duplicate: `DelayedProgress` selama `instance.get_duplicate_form`, lalu form yang terisi draft (nama terpilih, setiap port Automatic, settings sumber, setiap secret Generate dengan Copy from yang lebih dulu memilih reference sumber), baris Runtime read only "{nama tampil} {versi} · same as “{sumber}”" sebagai ganti filter kategori dan picker runtime, dan subjudul di *Copy*. Preflight, perintah setara, footer, progress, cancel, dan penanganan gagal sama dengan spec 0020. Form yang gagal dimuat menampilkan pesan daemon dengan Retry, dan Create nonaktif dengan alasan "Couldn’t load the form.". Sukses menutup sheet, mengirim start kalau toggle aktif, memilih instance baru lewat `ShellNavigator`, dan mengirim umpan balik di *Copy* ke `FeedbackCenter`. Test Swift `CreateInstanceModel` dengan service tiruan dan fixture `duplicate-form-*.json` membuktikan setiap state.
- **AC-13** (titik masuk, spec 0069 AC-11): menu More di toolbar halaman detail berisi "Duplicate…" tepat sesudah "Copy Connection String", dan menu Instance berisi Duplicate… ⌘D di kelompok 4; keduanya memakai `StacklyAction` duplicate yang sama, diterbitkan halaman Instances untuk target instance yang detailnya terbuka, selain itu baris terpilih tabel, dengan `mutates` true sehingga nonaktif saat gate mutasi `.blocked`. Tanpa target, item menu Instance nonaktif tanpa subjudul. Item More tidak memasang shortcut, baris `duplicateInstance` di `CommandMap` menjadi built, dan ⌘D tidak melakukan apa pun selama sheet atau dialog terbuka (spec 0069).
- **AC-14** (AC-027, NFR-012, JOURNEY-02 di GUI): alur duplicate bisa diselesaikan penuh dengan keyboard (⌘2, panah ke baris, ⌘D, ketik nama, Return; atau dari detail: Tab ke More, Space, panah ke Duplicate…, Return) dan VoiceOver (baris Runtime terbaca "Runtime, {nama tampil} {versi}, same as {sumber}", subjudul terbaca, dan setiap baris preflight seperti spec 0020), di lebar sheet 560 pt tanpa teks terpotong dan dengan kontras WCAG AA di light, dark, dan Increase Contrast. Buktinya evidence manual di Mac ini yang sekaligus menjalankan JOURNEY-02 dengan PostgreSQL 17: instance kedua lewat New Instance (port dari tombol "Use {n}") dan lewat Duplicate…, keduanya di start, lalu data berbeda dicek dengan `psql` milik runtime itu.

Status test semua AC mulai dari `NOT_RUN`. Lulus di fixture bukan evidence native, dan artefak atau sesi GUI yang tidak tersedia dicatat `BLOCKED`.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md): satu method baca `instance.get_duplicate_form` yang membangun draft create dari config revision aktif sumber, lalu create biasa spec 0020 tanpa perubahan; draft memakai provider revision dan runtime sumber yang persis sama, setiap port Automatic, setiap secret Generate, nama `<sumber>-copy`, dan tanpa catatan asal di registry; CLI `stackly instance duplicate`; GUI memakai sheet New Instance dalam mode duplicate dari menu More dan ⌘D; dan bukti isolasi lewat sentinel, di fake provider dengan rute `/sentinel` baru dan di PostgreSQL 17 nyata dengan helper yang dipakai ulang spec 0025.

Duplicate adalah cara lain mengisi form create, bukan operasi baru. Satu satunya pintu masuk yang membuat sesuatu tetap `instance.create`, jadi semua jaminan spec 0020 (folder selalu baru, port fixed tidak pernah diganti, kompensasi penuh) berlaku tanpa kode kedua.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang.

## Feature design

### Trace PRD

| ID PRD | Dipenuhi oleh |
| --- | --- |
| REQ-009 (instance terisolasi, runtime boleh dipakai ulang, data tidak dibagi) | AC-6, AC-7, AC-8, AC-9 |
| REQ-025 (bagian duplicate config) | AC-1 sampai AC-7, AC-10 sampai AC-14 |
| AC-009 (bagian dua instance mayor 17 dengan runtime ID sama) | AC-8 (fixture), AC-9 (native); bagian satu instance mayor 18 dan matriks tiga cluster milik spec 0025 |
| AC-025 | AC-1 (ID dan runtime), AC-4 (port), AC-5 (secret), AC-6 (ID dan path data), AC-7 (data tidak disalin) |
| JOURNEY-02 | AC-9 (CLI dan native), AC-14 (GUI) |
| BR-005 | AC-4 |
| BR-002, BR-008 | AC-6, AC-7 (lewat create spec 0020) |
| PAGE-04 (Create Instance, mode duplicate) | AC-12 |
| AC-027, NFR-012 | AC-14 |
| AC-028 (paritas CLI dan GUI) | AC-11 |

### Batas dengan spec lain

| Bagian | Pemilik |
| --- | --- |
| `instance.get_create_form`, `instance.preflight`, `instance.create`, `gather`, `evaluate`, *Pemeriksaan port*, kompensasi, `CreateInstanceModel`, `NewInstanceSheet`, perintah setara | spec 0020; spec ini memakainya tanpa mengubah kontraknya, dan hanya menambah `build_form_from_package` serta mode duplicate (*Usulan update untuk spec lain*) |
| Start, stop, alokasi port `Auto`, `bound`, `PortProbe` | spec 0010 |
| `SecretService::put`, `copyFrom`, `bind`, `SecretField`, reference `local:instance/<id>/<purpose>` | spec 0011; spec ini memakai `copyFromOptions` yang sudah ada dan menambah `copyFromLabels` di `SecretField` (*Usulan update untuk spec lain*) |
| Halaman detail, menu More, `InstanceTab` | spec 0021; Duplicate adalah item More, bukan tab |
| Halaman Instances, `InstanceActionModel`, resolusi nama CLI (`resolve.rs`, `cli.instance_not_found`) | spec 0022 |
| `CommandMap`, menu Instance, `.publishesCommands` | spec 0069 |
| Paket PostgreSQL, test binary native `apps/daemon/tests/postgresql.rs` dengan harness `apps/daemon/tests/postgresql/harness.rs` (`Site`, `Psql`, `create_instance`) | spec 0019 |
| Mayor 18 di samping dua mayor 17, matriks tiga cluster, penolakan rebind | spec 0025 (memakai ulang *Helper sentinel* dan `two_postgresql_17_instances`) |
| Restart sebagai satu operasi | fitur 26 |
| Remove dan purge, termasuk reference secret yang di `bind` bersama | fitur 54 |
| Riwayat asal instance di tab Activity | fitur 35 (Follow-up) |

### Alur

**Create biasa.** Kamu memilih runtime yang sama di sheet New Instance atau `stackly instance create --runtime postgresql@17`. Spec 0020 berlaku apa adanya: nama saran `postgresql-17` (atau `postgresql-17-2` kalau nama itu sudah dipakai), port `defaultPort` yang sudah dipakai instance pertama ditolak `supervisor.endpoint_conflict` dengan tombol "Use {n}" (GUI), dan tanpa `--port` endpoint menjadi `auto` (CLI). Spec ini tidak mengubah alur itu; ia membuktikannya (AC-6, AC-8, AC-9, AC-14).

**Duplicate.**

1. Kamu memilih Duplicate… di menu More halaman detail atau ⌘D (menu Instance) dengan baris terpilih, atau menjalankan `stackly instance duplicate <sumber>`.
2. Client memanggil `instance.get_duplicate_form { instanceId }`. Daemon membaca registry dan paket pinned sumber (`gather_duplicate`), lalu `build_duplicate` membangun form dan draft tanpa IO.
3. GUI mengisi sheet dari draft; CLI menerapkan flag di atas draft. Setiap perubahan (GUI) atau `--dry-run` (CLI) memanggil `instance.preflight` spec 0020.
4. Create mengirim `instance.create` spec 0020 dengan draft itu dan `SecretInput` per secret field. Operasi `create_instance` berjalan apa adanya.
5. Kalau "Start after creating" aktif atau `--start`, client mengirim `instance.start` dengan key turunan (spec 0020).

### Model data

Tidak ada tabel, kolom, migrasi, atau op registry baru. `instance.get_duplicate_form` hanya membaca; semua yang ditulis duplicate ditulis create spec 0020 (*Model data* spec itu). Registry tidak mencatat asal duplicate (keputusan 9 di rationale): instance hasil duplicate tidak bisa dibedakan dari instance hasil create biasa, kecuali lewat journal operasi create yang menyimpan draft tanpa nilai secret (termasuk reference `copyFrom`).

Yang dibaca `gather_duplicate`:

| Data | Sumber | Dipakai untuk |
| --- | --- | --- |
| instance sumber | `Registry::instance(id)` | ada atau tidak, retained, nama, `provider_revision`, `runtime_id`, endpoint |
| paket pinned | `supervisor::prepare::load_package` (spec 0010: `provider_revision_dir(key)`, `NameCheck::Skip`, digest dicocokkan dengan key instance) | form, schema, endpoint, capability |
| catalog yang dimuat | `CatalogState` daemon (spec 0017) | penolakan label revision sama dengan digest lain |
| runtime | `Registry::runtime(runtime_id)` | form dan pemeriksaan state |
| config aktif | `Registry::active_config_revision(id)` | `draft.settings` |
| pemakaian secret | `Registry::secret_uses_by(SecretUser::Instance(id))` | `secrets[].sourceReference` |
| nama terpakai | `Registry::instances(include_retained: false)` | nama duplicate |

`gather_duplicate` tidak membaca apa pun di bawah `instances/<id sumber>/`, folder log, atau folder run sumber (AC-1). Create sesudahnya hanya menyentuh metadata folder data sumber lewat `check_data_root_overlap` spec 0020 (AC-7).

### Method IPC

Semua method lewat handshake dan peer user yang sama (spec 0006).

| Method | Recovery | Input | Output | Error |
| --- | --- | --- | --- | --- |
| `instance.get_duplicate_form` | refused | `GetDuplicateFormParams { instanceId: Uuid }` | `DuplicateForm` | `resource.not_found` (`ErrorBody.resource` instance), `supervisor.instance_retained`, `supervisor.package_unavailable`, `instance.provider_ambiguous`, `instance.capability_unsupported`, `instance.runtime_not_found`, `supervisor.runtime_unavailable`, kode area `registry`, `daemon.starting`, `registry.recovery_mode` |
| `instance.preflight`, `instance.create` | spec 0020 | tidak berubah | tidak berubah | tidak berubah |

DTO baru di modul `instance` `stackly-protocol`:

```rust
pub struct GetDuplicateFormParams { pub instance_id: Uuid }
pub struct DuplicateForm {
    pub source: DuplicateSource,
    pub form: CreateForm,                 // spec 0020; provider.source pinned, suggestedName = nama duplicate
    pub draft: PreflightCreateParams,     // spec 0020; start false, displayName kosong
    pub secrets: Vec<DuplicateSecret>,    // satu per secret field, urutan form
}
pub struct DuplicateSource { pub instance_id: Uuid, pub name: String }
pub struct DuplicateSecret { pub field: String, #[serde(default)] pub source_reference: Option<String> }
```

- `Validate` untuk `GetDuplicateFormParams` tidak punya aturan tambahan (hanya UUID), tetapi tetap diimplementasikan (aturan spec 0006).
- `draft.secrets` berisi `SecretPlan { field, source: Generate }` untuk setiap secret field, jadi draft bisa langsung dikirim ke `instance.preflight`. `instance.create` butuh `SecretInput`, yang client bangun dari pilihan pengguna per field (Generate, Enter, Copy from), sama seperti create.
- `form.dataPath` tetap teks `instances/<new instance id>/data` (spec 0020).
- Handler berjalan di `spawn_blocking` seperti `instance.get_create_form`, karena `gather_duplicate` membaca registry dan file paket.

**Skenario fixture duplicate** (masukan tetap untuk `duplicate-form-*.json`, dibangun dengan `FakeRegistry` dan paket nyata lewat mekanisme yang sama dengan `create-form-*.json` spec 0020; UUID adalah konstanta UUID v7 di test):

| Fixture | Sumber | Runtime | Config aktif | Pemakaian secret | Instance lain |
| --- | --- | --- | --- | --- | --- |
| `duplicate-form-postgresql.json` | `pg17-dev`, `official/postgresql` revision paket di `providers/postgresql`, endpoint `client` `Fixed` 5432 | PostgreSQL 17 managed `installed`, versi dari `harness::PG17` | `maxConnections` 200, `logStatement` `ddl`, field lain default | `superuserPassword` ke reference `local:instance/<id pg17-dev>/<purpose>` (purpose menurut spec 0011) | tidak ada, jadi nama `pg17-dev-copy` |
| `duplicate-form-fake.json` | `fake-a`, `testing/fake`, endpoint `http` `Fixed` 18080 | runtime fake `installed` | `greeting` `hello from fake-a`, `crashAfterMs` 0, field lain default | `authToken` ke reference instance itu | `fake-a-copy` dipakai, jadi nama `fake-a-copy-2` |

Kedua fixture menunjukkan semua port `auto`, secret `generate` dengan `sourceReference`, dan settings tanpa field secret; test Swift AC-12 membaca nilai settings dan Copy from dari fixture ini.

### Draft duplicate

Modul baru `stackly_core::instance::duplicate`, di samping `instance::create`:

```rust
pub struct DuplicateDeps<'a> { pub registry: &'a dyn Registry, pub catalog: &'a CatalogState, pub platform: &'a Platform,
                               pub prepared: &'a PreparedLayout }
pub struct DuplicateFacts { pub source: Instance, pub package: ValidatedPackage, pub runtime: Runtime,
                            pub values: Option<serde_json::Value>, pub uses: Vec<SecretUse>, pub taken: Vec<InstanceName> }
pub fn gather_duplicate(deps: &DuplicateDeps<'_>, source: InstanceId) -> Result<DuplicateFacts, DuplicateError>; // satu satunya IO
pub fn build_duplicate(facts: &DuplicateFacts) -> DuplicateModel;                                                 // murni
pub struct DuplicateModel { pub source: (InstanceId, InstanceName), pub form: CreateFormModel,
                            pub draft: CreateDraft, pub secret_sources: Vec<(SecretFieldName, Option<SecretRef>)> }
#[non_exhaustive]
pub enum DuplicateError { NotFound, Retained, PackageUnavailable { provider: String, reason: String },
                          ProviderAmbiguous { provider: ProviderName, revision: RevisionLabel },
                          CapabilityUnsupported { provider: ProviderName, existing: InstanceName },
                          RuntimeNotFound { runtime_id: RuntimeId }, RuntimeUnavailable { runtime_id: RuntimeId, state: RuntimeState }, Registry(RegistryError) }
pub fn duplicate_name(source: &InstanceName, taken: &[InstanceName]) -> Option<InstanceName>;
```

- `gather_duplicate` menjalankan pemeriksaan AC-1 berurutan dan berhenti di yang pertama gagal. Paket pinned dibaca lewat `supervisor::prepare::load_package(platform, layout, instance)` yang dipakai tahap `verify` start spec 0010 (langkah 2 menjadikannya `pub(crate)`), lalu `SupervisorError::PackageUnavailable { provider, reason }` nya diteruskan apa adanya, supaya teks `reason` sama dengan start. Sesudah itu `gather_duplicate` mencari di `CatalogState` yang dimuat entri provider dengan nama dan label revision sumber: kalau ada dan `manifest_sha256` nya beda dengan key sumber, hasilnya `ProviderAmbiguous` (catalog yang tidak dimuat atau tanpa entri itu bukan masalah, karena create lalu memakai jalur pinned spec 0020).
- `build_form_from_package(package, source: PackageSource, runtime, suggested_name) -> CreateFormModel` di `instance::create::form` adalah isi `build_form` yang sekarang, dan `build_form` spec 0020 memanggilnya dengan sumber catalog dan `suggested_name` nya sendiri. Duplicate memanggilnya dengan `PackageSource::Pinned` dan nama duplicate. Tidak ada form kedua.
- `CreateDraft` core spec 0020 dipakai ulang sebagai draft; `stacklyd` memetakannya ke `PreflightCreateParams` (pemetaan baru, kebalikan pemetaan params ke draft yang sudah ada).
- `DuplicateError` dipetakan `stacklyd` ke kode di *Method IPC*: `NotFound` ke `resource.not_found`, `Retained` ke `supervisor.instance_retained`, `PackageUnavailable` ke `supervisor.package_unavailable` (detail `provider`, `reason`), `ProviderAmbiguous` ke `instance.provider_ambiguous` (detail `provider`, `revision`), `CapabilityUnsupported` ke `instance.capability_unsupported` (detail `capability` `multi_instance`, `provider`, `existing`), `RuntimeNotFound` ke `instance.runtime_not_found` (detail `runtimeId`), `RuntimeUnavailable` ke `supervisor.runtime_unavailable` (detail `runtimeId`, `state`), dan `Registry` ke kode area `registry`. `supervisor.instance_retained` tidak punya detail.

| Bagian draft | Isi | Sumber |
| --- | --- | --- |
| `name` | *Nama duplicate*, atau string kosong | nama sumber, `taken` |
| `displayName` | kosong | tetap (nama tampil adalah identitas, bukan config) |
| `provider` | `{ name, revision }` sumber | `Instance.provider_revision` (nama dan revision key) |
| `runtimeId` | runtime ID sumber | `Instance.runtime_id` |
| `endpoints` | setiap endpoint IP manifest `{ name, port: auto }`, urutan form | paket pinned (endpoint `tcp` dan `udp`); endpoint `unix` tidak pernah ada di draft |
| `settings` | `values` aktif, hanya field schema yang bukan secret dan bukan managed | `active_config_revision`, `SettingsSchema::fields()` paket pinned |
| `secrets` | `generate` per secret field, urutan form | `SettingsSchema::fields()` |
| `start` | false | tetap; toggle GUI dan `--start` CLI yang menentukan |

`secret_sources` berisi satu entri per secret field: reference dari `uses` yang `field` nya sama, atau `None`.

### Nama duplicate

- Dasar: nama sumber. Kalau nama sumber berakhir `-copy` atau `-copy-<n>` dengan n 2 sampai 99, akhiran itu dibuang dulu, jadi duplicate dari `pg17-dev-copy` menyarankan `pg17-dev-copy-2`, bukan `pg17-dev-copy-copy`.
- Kandidat berurutan: `<dasar>-copy`, `<dasar>-copy-2`, sampai `<dasar>-copy-99`. Yang pertama yang tidak dipakai instance tidak retained dan lolos `InstanceName::parse` dipakai.
- Kalau kandidat lebih dari 63 karakter, dasar dipotong dari kanan sampai muat, lalu tanda hubung di ujung dasar dibuang.
- Tanpa kandidat yang lolos, nama kosong; GUI menampilkan field nama kosong dengan error preflight `name`, dan CLI tanpa `<name>` mengirim nama kosong sehingga daemon menolaknya `instance.name_invalid`.

| Sumber | Nama terpakai | Saran |
| --- | --- | --- |
| `pg17-dev` | tidak ada | `pg17-dev-copy` |
| `pg17-dev` | `pg17-dev-copy` | `pg17-dev-copy-2` |
| `pg17-dev-copy` | `pg17-dev-copy` | `pg17-dev-copy-2` |
| `pg17-dev-copy-7` | `pg17-dev-copy-7` | `pg17-dev-copy` |
| `a` diikuti 62 huruf `b` | tidak ada | dasar dipotong menjadi 58 karakter, lalu `-copy` |
| `pg17-dev` | `pg17-dev-copy` sampai `pg17-dev-copy-99` | kosong |

### Kode error

Tidak ada kode baru. Kode yang dipakai ulang: `resource.not_found` (spec 0006), `supervisor.instance_retained`, `supervisor.package_unavailable`, `supervisor.runtime_unavailable` (spec 0010), `instance.provider_ambiguous`, `instance.capability_unsupported`, `instance.runtime_not_found` (spec 0010 dan 0020), `cli.instance_not_found` (spec 0022), kode area `registry` (spec 0005), dan semua kode create spec 0020 untuk preflight dan create.

### CLI

```text
stackly instance duplicate <source> [<name>]
    [--display-name <teks>] [--port <endpoint>=<port|auto>]... [--set <field>=<nilai>]...
    [--set-json <field>=<json>]... [--settings-file <path.json>] [--secret <field>=<sumber>]...
    [--start] [--dry-run] [--wait] [--timeout <durasi>] [--idempotency-key <key>] [--json]
```

- `<source>` diresolusi seperti `stackly instance inspect` (spec 0022, `resolve.rs`): bentuk UUID langsung, selain itu nama persis lewat `instance.list`; nama yang tidak ada gagal `cli.instance_not_found` (exit 10). Lalu CLI memanggil `instance.get_duplicate_form`.
- Draft dari daemon menjadi dasar. `<name>` menggantikan `draft.name`. `--port` menggantikan permintaan endpoint itu. `--settings-file` menggantikan key yang sama di `draft.settings` (key lain dari sumber tetap), lalu `--set` dan `--set-json` menggantikan key nya, dengan konversi dan tiga aturan `cli.usage` spec 0020.
- `--secret <field>=<sumber>` menerima semua sumber create spec 0020 (`generate`, `prompt`, `stdin`, `file:<path>`, `env:<NAMA>`, `copy:<ref>`, `bind:<ref>`) ditambah bentuk pendek `copy`, yang berarti `copyFrom` reference yang dipakai sumber untuk field itu (`secrets[].sourceReference`); `copy` untuk field tanpa `sourceReference` gagal `cli.usage` (exit 2) tanpa mengirim create. Field yang tidak disebut tetap `generate`.
- Tidak ada `--runtime`, `--provider`, dan `--revision`: runtime dan provider revision selalu milik sumber. Clap menolak flag itu dengan `cli.usage` (exit 2).
- Selebihnya sama dengan `stackly instance create` (spec 0020): `--dry-run` hanya memanggil `instance.preflight`, progress tahap di stderr, `--start` menunggu create lalu mengirim start dengan key SHA-256 dari `<key create>:start`, `--wait`, `--timeout`, dan exit code dari kategori spec 0008.
- `--json` sama dengan create ditambah key `source` `{ "id": "<uuid>", "name": "<nama>" }`: `{ "preflight": CreatePreflight, "source": … }` untuk `--dry-run`, dan `{ operation, idempotencyKey, instance, start, source }` untuk selain itu. Teks manusia diawali satu baris `Duplicating settings of <sumber>; no data is copied.` di stderr; baris itu juga dicetak dengan `--json`, karena stderr tidak pernah menyentuh envelope stdout (spec 0008).
- Perintah setara di `CreatePreflight.command` tetap `stackly instance create …` lengkap (fungsi `equivalent_command` spec 0020 tanpa perubahan), dengan `--provider` dan `--revision` sumber, `--port <endpoint>=auto`, dan `--secret` per field. Perintah itu menghasilkan instance yang sama walaupun sumber nanti berubah atau dihapus.

### Sheet mode duplicate (GUI)

`CreateInstanceModel` mendapat `mode: CreateMode` dengan `.create(preselect: UUID?)` (perilaku spec 0020; runtime yang dipilih lebih dulu, misalnya dari panel detail Catalog) dan `.duplicate(DuplicateSourceRef { id, name })`. `CreateInstanceService` mendapat `getDuplicateForm(instanceId:)`. Semua yang tidak disebut di sini sama dengan *Sheet New Instance* spec 0020.

- **Mulai**: mode duplicate langsung ke state `loadingForm`, tanpa `choosingRuntime`, dengan tugas muat bertoken spec 0020 (butir Copy from *Sheet New Instance*, Keputusan 19) yang memanggil `getDuplicateForm` dan `listSecrets` bersamaan. Sukses masuk `editing` begitu form kembali, tanpa menunggu daftar, dengan nilai dari `draft` (bukan default form); gagal masuk `formFailed` dengan `FailureContent` berisi pesan daemon (misalnya "{provider} runs one instance at a time") dan tombol Retry yang memulai tugas muat baru untuk kedua panggilan itu. `listSecrets` yang gagal tidak pernah membuat `formFailed` (spec 0020).
- **Header**: judul tetap "New Instance" (mockup); subjudul diganti teks duplicate di *Copy*.
- **Runtime**: filter kategori dan picker runtime tidak tampil. Gantinya satu `LabeledContent("Runtime")` read only dengan nilai "{nama tampil} {versi} · same as “{sumber}”" (ditambah " (external)" untuk install external): nama tampil dari `form.provider.displayName`, versi dan install mode dari `form.runtime`, dan sumber dari `source.name`. Tile header diambil dari entri `CatalogStore` yang software nya sama dengan `form.runtime.software` (seperti create), atau tile umum kalau catalog tidak memuatnya.
- **Nama, port, settings**: sama dengan create, terisi dari draft. Setiap baris port mulai dari Automatic; memilih Fixed mengisi angka `defaultPort` endpoint itu seperti create, lalu preflight menolak konflik dengan saran "Use {n}".
- **Secret** (update 2026-10-22, spec 0020 Keputusan 19): setiap `SecretField` mulai dari Generate. Daftar Copy from dihitung per field oleh `CreateInstanceModel.copyFromOptions(for field: String) -> [String]` (key field seperti `secrets[].field`), yang diteruskan `SchemaFormModel` ke `SecretField` field itu; di mode create fungsi ini mengembalikan `copyFromOptions` spec 0020 untuk setiap field. Di mode duplicate, untuk field yang `sourceReference` nya ada, hasilnya reference sumber di urutan pertama, lalu `copyFromOptions` mode create tanpa reference itu (tanpa duplikat). Reference sumber selalu ada di depan tanpa melihat status nya di `secret.list`, karena ia datang dari `secrets[].sourceReference`, bukan dari daftar; reference yang nilainya hilang ditolak daemon di baris preflight `secret.<field>` (spec 0020). Selama `listSecrets` belum kembali, gagal, atau tidak menyisakan reference, daftar field itu hanya reference sumber, jadi Copy from tetap tampil; saat daftar kembali, ia hanya bertambah di belakang reference sumber, sehingga pilihan pengguna tidak berubah. Model mengisi `SecretDraft.copyFromReference` dengan reference sumber sambil tetap memilih Generate, sehingga Copy from langsung menunjuk reference sumber; pengguna tetap harus memilih Copy from sendiri. `copyFromLabels` mode duplicate adalah `copyFromLabels` mode create (label "{purpose} of “{nama}”" spec 0020) digabung dengan label sumber "{judul field} of “{sumber}”" untuk setiap `sourceReference`; untuk reference yang sama, label sumber menang. Sebelum daftar kembali, peta itu hanya berisi label sumber, dan satu peta yang sama diteruskan ke setiap `SecretField`. Field tanpa `sourceReference` memakai perilaku create (daftar mode create apa adanya, Copy from tersembunyi selama daftar kosong).
- **Footer**: urutan `disabledReason` spec 0020, tanpa "Install a runtime first." dan "Choose a runtime." (keduanya tidak mungkin di mode ini).
- **Selesai**: sama dengan create (`ShellNavigator.show(.instances(filter: .all, select: id))`, start kalau toggle aktif), dengan teks umpan balik duplicate di *Copy*.
- **Presentasi**: `AppShell` mempresentasikan satu sheet untuk kedua mode lewat item `CreateSheetRequest` (`.create(preselect:)` atau `.duplicate(DuplicateSourceRef)`), dan memasang nilai environment `duplicateInstanceAction: DuplicateInstanceAction?` (struct `@MainActor` dengan `callAsFunction(_ source: DuplicateSourceRef)`) di samping `newInstanceAction` spec 0020.

### Titik masuk

- **Menu More halaman detail** (spec 0021): `ActionMenu` mendapat `StacklyAction(id: "duplicate", title: "Duplicate…", role: .standard(confirmation: nil), mutates: true)` tepat sesudah "Copy Connection String"; `perform` memanggil `duplicateInstanceAction` dengan instance yang terbuka. Item ini hanya dirender saat `duplicateInstanceAction` tidak nil, tidak memasang shortcut, dan gate nya lewat `availability(gate:)`. Fitur 54 nanti menambah pemisah dan "Remove Instance…" di bawahnya (urutan mockup).
- **Menu Instance** (spec 0069): baris `duplicateInstance` di `CommandMap` menjadi built, `CommandItem(.duplicateInstance)` ditambah di kelompok 4 builder menu Instance, dan tabel perintah di `design.md` diperbarui (aturan *Menambah perintah* butir 4).
- **Penerbit**: halaman Instances menerbitkan `duplicateInstance` untuk target (detail yang terbuka, selain itu baris terpilih) dengan action yang sama dengan item More, hanya saat `duplicateInstanceAction` tidak nil. Tanpa target, tidak ada action, sehingga item menu nonaktif tanpa subjudul.
- Tidak ada tombol Duplicate di toolbar tabel, inspector, Overview, atau menu konteks: menu More detail adalah jalan di jendela (aturan *Menambah perintah* butir 1), dan ⌘D jalan cepat dari tabel.
- Duplicate tidak bergantung pada lifecycle sumber: instance `stopped`, `running`, `failed`, atau `uninitialized` sama sama bisa diduplikasi, karena hanya registry dan paket yang dibaca. Instance retained tidak tampil di halaman Instances, dan daemon menolaknya `supervisor.instance_retained`.

### Aksesibilitas

- Urutan fokus mode duplicate: nama, baris port, field settings, Advanced, toggle, baris preflight, Copy, Cancel, Create. Baris Runtime read only bukan kontrol, tetapi terbaca VoiceOver saat menjelajah: "Runtime, {nama tampil} {versi}, same as {sumber}".
- Subjudul header terbaca sebagai bagian header sheet. Pilihan Copy from untuk reference sumber terbaca dengan label nya, "{judul field} of {sumber}".
- Item More "Duplicate…" dan item menu Instance memakai judul yang sama; tooltip tidak ada karena item menu tidak punya tooltip (spec 0069 butir 5).
- Semua aturan teks besar, kontras, dan status preflight spec 0020 dan 0067 berlaku tanpa tambahan.

### Copy

| Tempat | Teks |
| --- | --- |
| Subjudul sheet mode duplicate | "Settings copied from “{sumber}”. Its own data directory, ports and secrets; no data is copied." |
| Baris Runtime | "{nama tampil} {versi} · same as “{sumber}”" |
| Label Copy from untuk reference sumber (`copyFromLabels`; menang atas label mode create untuk reference yang sama) | "{judul field} of “{sumber}”", misalnya "Superuser password of “pg17-dev”" |
| Label Copy from untuk reference lain | label mode create spec 0020: "{purpose} of “{nama}”" untuk reference milik instance di `InstanceStore`, selain itu teks reference |
| Item More dan menu Instance | "Duplicate…" |
| Umpan balik sukses | "“{nama}” created from “{sumber}”." |
| Umpan balik sukses dengan start | "“{nama}” created from “{sumber}”. Starting it now." |
| Start lanjutan gagal | "“{nama}” created, but it didn’t start." (spec 0020) |
| Form gagal dimuat | judul "Couldn’t load the form.", isi pesan daemon, tombol Retry |
| Baris pertama CLI (stderr) | "Duplicating settings of {sumber}; no data is copied." |

### Fake server: rute sentinel

`stackly-fake-server` `serve` mendapat dua rute untuk bukti fixture:

- `http::Request` mendapat `body: Vec<u8>`: `read_request` menyimpan body sampai `MAX_BODY_BYTES` yang sudah ada alih alih membuangnya.
- `PUT /sentinel` dengan body teks UTF 8 1 sampai 256 byte menulis `<data>/SENTINEL` secara atomik (file sementara di folder yang sama lalu rename) dan menjawab 204; body kosong, lebih dari 256 byte, atau bukan UTF 8 dijawab 400.
- `GET /sentinel` menjawab 200 dengan isi file itu, atau 404 kalau file tidak ada.
- `<data>` adalah `--data` proses itu sendiri, jadi jawabannya membuktikan setiap proses membaca folder data instance nya sendiri. Test `crates/fake-server/tests/dummy_server.rs` mencakup tulis, baca, 404, dan 400. Sisi test daemon memakai `put_sentinel(port, value)` dan `get_sentinel(port) -> Option<String>` baru di `apps/daemon/tests/supervisor/support.rs`, di samping helper HTTP yang sudah ada di sana.

### Helper sentinel

Untuk test native fitur ini dan spec 0025 (disepakati dengan arc-f25-spec, 2026-10-22). Semuanya tinggal di test binary `postgresql` milik spec 0019 dan memakai harness nya (`Site`, `Psql`, `Via`, `Made`, `create_instance`, `SUPERUSER`), karena binary itu sudah memasang runtime dari cache xtask dan mem pin paket:

- Modul `apps/daemon/tests/postgresql/sentinel.rs`, dideklarasikan di `apps/daemon/tests/postgresql.rs` sebagai `#[path = "postgresql/sentinel.rs"] mod sentinel;` seperti modul lain di binary itu (nama `support` sudah dipakai `supervisor/support.rs`).
- `pub struct SentinelCluster { pub name: String, pub id: Uuid, pub psql: Psql, pub password: String }`, dibuat lewat `SentinelCluster::new(name, id, psql, password)`. `psql` adalah `Psql` yang menunjuk instance itu dengan `psql` dari runtime instance itu sendiri (untuk satu runtime, `Site::psql(id, port)`), jadi helper tidak pernah mengasumsikan mayor 17 dan tidak pernah membaca cluster dengan `psql` mayor lain. `password` adalah password superuser yang test pegang (`Made.password`, atau password sumber untuk duplicate dengan `copyFrom`).
- `write(&self, value: &str)`: `CREATE TABLE IF NOT EXISTS stackly_sentinel (id text primary key, value text not null)`, lalu upsert baris `id = name`.
- `read(&self) -> Vec<(String, String)>`: semua baris `stackly_sentinel` urut `id`; tabel yang tidak ada dibaca sebagai daftar kosong (`to_regclass('stackly_sentinel') IS NULL`).
- `system_identifier(&self) -> String`: `SELECT system_identifier FROM pg_control_system()`.
- `assert_isolated(expected: &[(&SentinelCluster, &str)])`: setiap cluster membaca tepat satu baris, miliknya sendiri, dengan nilai yang diharapkan.
- Koneksi selalu `Psql::run(Via::Tcp, SUPERUSER, Some(password), "postgres", sql)`, yaitu TCP `127.0.0.1:<port>` instance itu dengan password hanya di env `PGPASSWORD` child `psql` (harness spec 0019: `env_clear`, `PGCONNECT_TIMEOUT=5`, `--no-psqlrc`), sehingga setiap baca juga membuktikan port itu milik cluster itu. Kegagalan memanggil `panic!` dengan stderr `psql`, seperti `Psql::scalar`; nilai sentinel dan password tidak pernah dicetak.
- Modul `apps/daemon/tests/postgresql/same_runtime.rs` memegang penyiapan bersama `pub async fn two_postgresql_17_instances(site: &Site, api: &Api) -> TwoPostgres17`: dengan `Site` yang dibuka untuk `PG17`, ia membuat `pg17-dev` lewat `create_instance` (port `Fixed` bebas), membuat `pg17-test` lewat `Api::duplicate_form` lalu `Api::create_from_draft` dengan `copyFrom` reference password `pg17-dev` (dari `secrets[].sourceReference`), men start keduanya sampai `ready`, menulis sentinel berbeda, lalu mengembalikan `TwoPostgres17 { dev: SentinelCluster, test: SentinelCluster, runtime_id: Uuid }`. Kedua test native fitur ini memanggilnya, dan spec 0025 memakainya sebagai titik awal matriks tiga cluster.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| form duplicate | instance sumber, nama, retained | `Registry::instance(id)` |
| form duplicate | paket dan form | `supervisor::prepare::load_package` atas `Instance.provider_revision` (digest dicocokkan dengan key yang dicatat instance) |
| form duplicate | penolakan digest catalog | entri `CatalogState` dengan nama dan label revision sumber, dibandingkan `manifest_sha256` nya |
| form duplicate | `form.provider.source` | konstanta `pinned` |
| form duplicate | runtime dan state nya | `Registry::runtime(Instance.runtime_id)` dipetakan ke `RuntimeSummary` (spec 0018) |
| form duplicate | capability `multi_instance` | `CapabilitySet` paket pinned |
| form duplicate | `suggestedName`, `draft.name` | `duplicate_name` atas nama sumber dan `Registry::instances(include_retained: false)` |
| form duplicate | `draft.provider` | `Instance.provider_revision` (nama dan revision) |
| form duplicate | `draft.runtimeId` | `Instance.runtime_id` |
| form duplicate | `draft.endpoints` | endpoint `tcp` dan `udp` paket pinned, semuanya `auto` |
| form duplicate | `draft.settings` | `Registry::active_config_revision(id).values`, disaring `SettingsSchema::fields()` (bukan secret, bukan managed) |
| form duplicate | `draft.secrets` | konstanta `generate` per secret field schema |
| form duplicate | `secrets[].sourceReference` | `Registry::secret_uses_by(SecretUser::Instance(id))`, dicocokkan per `field` |
| form duplicate | `source` | ID dan nama instance sumber |
| sheet | baris Runtime | `form.runtime` (nama tampil, versi, install mode) dan `source.name` |
| sheet | Copy from terisi | `secrets[].sourceReference` lewat `copyFromOptions(for:)`, `SecretDraft.copyFromReference`, dan `copyFromLabels`; judul field dari `FormField.title` |
| sheet | `copyFromOptions(for:)` setiap secret field | `secrets[].sourceReference` field itu di urutan pertama, lalu `copyFromOptions` mode create spec 0020 (`secret.list` lewat `CreateInstanceService.listSecrets`, dipanggil dalam tugas muat yang sama dengan `getDuplicateForm`, Keputusan 19) tanpa reference itu |
| sheet | `copyFromLabels` | `copyFromLabels` mode create spec 0020 digabung label sumber untuk setiap `secrets[].sourceReference` (judul dari `FormField.title`, sumber dari `source.name`); label sumber menang |
| CLI | sumber | `resolve.rs` spec 0022 (UUID atau nama lewat `instance.list`) |
| CLI | `--secret <field>=copy` | `secrets[].sourceReference` field itu |
| preflight, create | semua nilai | spec 0020 *Value sourcing* (tidak berubah) |
| start duplicate | port `Auto` | spec 0010 (`bound` tersimpan kalau bebas, kalau tidak bind `:0` lalu `resolve_port`) |
| umpan balik | `{nama}`, `{sumber}` | nama di draft saat Create, `source.name` |
| test | sentinel fake | `PUT /sentinel` dan `GET /sentinel` di folder `--data` proses |
| test | sentinel PostgreSQL | tabel `stackly_sentinel` lewat `psql` runtime instance itu di TCP port instance itu, password yang dipegang test (`Made.password`, sama untuk duplicate dengan `copyFrom`) |
| test | identitas cluster | `pg_control_system().system_identifier` |

### Key invariants

- Duplicate tidak pernah membaca isi, mendaftar, menyalin, atau menulis pohon instance sumber: form duplicate hanya membaca registry dan paket pinned, dan create hanya membaca metadata folder data sumber untuk `check_data_root_overlap` (AC-7).
- Satu satunya jalan yang membuat instance tetap `instance.create`; duplicate tidak punya operasi, kompensasi, atau validasi sendiri.
- Daemon tidak pernah memilih port `Fixed` untuk instance baru: draft duplicate selalu `auto`, dan port `Fixed` hanya dari pengguna (BR-005).
- Daemon tidak pernah memilih reference yang sudah ada untuk instance baru atas inisiatifnya sendiri: draft duplicate selalu `generate`, dan `copyFrom` atau `bind` hanya dari aksi pengguna (spec 0011 AC-6).
- Instance duplicate memakai runtime ID dan provider revision key sumber yang persis sama (BR-001), dan settings nya selalu divalidasi ulang oleh preflight dan create, tidak dipercaya karena berasal dari sumber.
- Dua instance dari runtime yang sama tidak pernah berbagi folder data, config, log, temp, run, socket, atau reference secret, kecuali reference yang pengguna `bind` secara eksplisit.

### Security model

- Method baru lewat socket privat milik user yang sama (spec 0006), hanya membaca, dan `recovery refused`.
- `DuplicateForm` memuat reference secret sumber (bukan nilai); reference bukan secret dan sudah tampil di halaman detail (spec 0021). Tidak ada nilai secret di form, draft, journal, log, atau perintah setara.
- `copyFrom` membaca nilai sumber di daemon dengan interaksi pemicu operasi create (spec 0011), tidak pernah lewat client.
- Settings sumber divalidasi ulang schema, termasuk aturan `argument_injection_risk` spec 0009, karena config sumber bisa saja dibuat oleh revision paket dengan aturan lain.
- Log daemon untuk method ini memuat ID sumber dan hasil (kode error atau jumlah field), tidak pernah nilai settings.
- Helper sentinel hanya ada di test; password hanya di env child `psql` dan tidak pernah di argv atau output test.

### Configuration required

Tidak ada variabel environment, setting, credential, atau dependency baru. Test native memakai cache runtime xtask yang sama dengan spec 0019; `anyhow` sudah boleh di test.

### Critical test scenarios

- Tabel `build_duplicate` (unit test murni): settings dari paket fake, PostgreSQL, dan NATS (secret, managed `serverName`, key asing, `immutableAfterInit`, nilai default, tanpa config aktif), endpoint semua `auto` termasuk sumber `Fixed`, secret `generate` dengan `secret_sources` dari pemakaian, dan setiap baris tabel *Nama duplicate*. Memverifikasi **AC-2**, **AC-3**, **AC-4**, **AC-5**.
- `gather_duplicate` dengan `FakePlatform` dan `FakeRegistry`: setiap kode AC-1 dalam urutannya (instance tidak ada, retained, paket hilang, digest beda, paket tidak valid, catalog dengan label revision sama dan digest lain, catalog tanpa entri itu yang tetap berhasil, tanpa `multi_instance` lewat paket fixture, runtime tidak ada, runtime `failed`, `values` yang bukan object), sumber tanpa folder apa pun di filesystem berhasil, dan penghitung `mutations()`, commit, Keychain, spawn, serta `PortProbe` tiruan tetap nol. Memverifikasi **AC-1**, **AC-7**.
- Daemon, fixture dua instance (modul baru `apps/daemon/tests/supervisor/same_runtime.rs` di test binary `supervisor` spec 0010, dengan `Api` nya ditambah `duplicate_form`, runtime fake lewat `install_fake`): create biasa `fake-a`, duplicate `fake-b`, create `fake-c` dengan `crashAfterMs`, sentinel lewat `PUT` dan `GET /sentinel`, stop dan start bergantian, PID dan readiness yang lain tetap, tujuh path dan `socketPath` berbeda, daftar isi folder runtime tetap. Memverifikasi **AC-6**, **AC-8**.
- Daemon, data tidak disalin: sentinel di `fake-a`, `fake-a` di stop, daftar pohon dan log `fake-a` dicatat, setiap file dan subfolder di dalam `instances/<id fake-a>/data` dibuat mode 000 (folder data sendiri tetap 0700), form, create, dan start duplicate berhasil, `GET /sentinel` duplicate 404, mode dipulihkan, daftar pohon `fake-a` sama persis. Memverifikasi **AC-7**.
- Daemon, secret: duplicate dengan draft apa adanya membuat reference baru dengan nilai berbeda (dibandingkan lewat `secret.reveal` dua reference); dengan `copyFrom` reference sumber nilainya sama, revision dan pemakaian reference sumber tetap; tidak ada `bind` di journal create dari draft; prefix fixture credential tidak ada di form, journal, log, dan event. Memverifikasi **AC-5**.
- Daemon, port: draft semua `auto`; duplicate dengan `--port` sama dengan port `Fixed` sumber yang `stopped` ditolak `supervisor.endpoint_conflict` dengan `otherInstance` sumber; dua instance `auto` dari runtime yang sama mendapat port efektif berbeda dan port yang sama dipakai lagi setelah stop dan start. Memverifikasi **AC-4**. *(diselaraskan dengan build, 2026-10-22: skenario ini memakai paket fake, yang init nya tidak memakai port. Draft duplicate dari sumber yang paket pinned nya butuh port saat `initialize` ditolak preflight sampai pengguna memilih port Fixed (*Consequences*); bukti nya test spec 0020 `the_nats_1_0_0_shape_needs_a_fixed_port_for_each_endpoint_its_config_names` di `crates/core/tests/create_evaluate/init_port.rs`, jadi spec ini tidak menambah test untuk itu.)*
- Daemon, error dan kinerja: sumber tidak ada, retained, paket pinned dihapus, runtime ditandai `failed`; balasan form dalam 300 ms pada persentil 95 dengan 10 instance. Memverifikasi **AC-1**.
- Kontrak: fixture varian baru, `duplicate-form-fake.json` dan `duplicate-form-postgresql.json` yang dibangkitkan ulang sama dengan di repo, decode Swift, kenaikan minor, `codegen --check`. Memverifikasi **AC-10**.
- CLI: golden teks dan JSON untuk `duplicate` dengan dan tanpa `<name>`, `--dry-run`, `--start`, `--wait`, `--port`, `--set` di atas settings sumber, `--settings-file` yang hanya mengganti key nya, `--secret field=copy` (ada dan tidak ada `sourceReference`), `--runtime` ditolak exit 2, sumber tidak dikenal exit 10; matriks paritas (nama `Bad_Name`, nama sumber yang dipakai, port sama dengan sumber, setting integer `"abc"`, key asing, `bind` ke reference yang tidak ada) lewat `Client` dan `--json`; journey JOURNEY-02 di fake (create, duplicate, start keduanya, `GET /sentinel` berbeda). Memverifikasi **AC-11**.
- GUI: test Swift `CreateInstanceModel` mode duplicate dengan service tiruan (loading, editing dari draft, nilai settings sumber, port Automatic, Copy from terisi tetapi Generate terpilih, Copy from mode duplicate (hanya reference sumber selama `listSecrets` belum kembali dan saat gagal, daftar create digabung di belakangnya tanpa duplikat, `copyFromReference` tetap setelah daftar kembali, reference sumber yang `missing` di `secret.list` tetap di depan, label sumber menang atas label create, field tanpa `sourceReference` memakai daftar create), `formFailed` dengan Retry, sukses dengan umpan balik duplicate, start lanjutan gagal), render `SchemaFormModel` dari `duplicate-form-postgresql.json`, penerbit Instances (target detail mengalahkan pilihan tabel, tanpa target tidak ada action, gate blocked), item More urut sesudah Copy Connection String, test peta `CommandMap` dengan baris built, dan suite snapshot mode duplicate. Memverifikasi **AC-12**, **AC-13**.
- Native PostgreSQL 17 (modul `apps/daemon/tests/postgresql/same_runtime.rs` di test binary `postgresql`, `--ignored`): `two_postgresql_17_instances_from_one_runtime_keep_their_sentinels` dan `duplicate_postgresql_instance_starts_with_empty_data`. Memverifikasi **AC-7**, **AC-9**.
- Manual GUI di Mac ini: keyboard, VoiceOver, 560 pt, light, dark, Increase Contrast, dan JOURNEY-02 dengan PostgreSQL 17. Memverifikasi **AC-14**.

### Jawaban untuk follow up spec lain

| Spec dan baris Follow-up | Jawaban di spec ini |
| --- | --- |
| 0011: "Fitur 24: duplicate memakai `generate` atau `copyFrom` ke reference baru, tidak pernah `bind` diam diam." | AC-5: draft selalu `generate`; `copyFrom` reference sumber ke reference baru hanya lewat aksi pengguna (Copy from, `--secret <field>=copy`); `bind` hanya lewat `--secret <field>=bind:<ref>` yang diketik pengguna; *Key invariants* |
| 0021: "Fitur 24 …: tambah tab dan tombol masing masing ke halaman detail lewat `InstanceTab` dan toolbar spec ini." | Fitur 24 tidak menambah tab; ia menambah item "Duplicate…" di menu More toolbar sesudah Copy Connection String (*Titik masuk*, AC-13) |
| 0069: "Fitur 24 …: ubah baris `duplicateInstance` … menjadi built, tambah `CommandItem`, dan terbitkan action dari halaman Instances" | *Titik masuk*, AC-13; Duplicate… ⌘D di kelompok 4 menu Instance, tanpa konfirmasi karena hanya membuka sheet |
| `ui-macos.md` peta layar: Detail instance · Summary, "menu More: Duplicate 24" | item More "Duplicate…" (AC-13) membuka sheet New Instance mode duplicate (AC-12) |

### Usulan update untuk spec lain

Ditulis di sini karena spec ini tidak boleh mengubah spec lain; lead membawanya ke pemilik spec. "Sisipkan sesudah" berarti teks lama tetap dan teks baru ditambahkan tepat sesudahnya.

#### Spec 0020 (create instance)

Diterapkan di spec 0020 pada 2026-10-22, apa adanya (*Jawaban untuk follow up spec lain* spec 0020 dan Keputusan 19, `49f0f4f`).

1. *Batas dengan spec lain*, sisipkan sesudah baris `| Peta perintah dan shortcut | … |` baris:

   ```text
   | Duplicate config: `instance.get_duplicate_form`, `build_form_from_package`, mode duplicate `CreateInstanceModel` dan `NewInstanceSheet` (`CreateMode`, `CreateSheetRequest`, `duplicateInstanceAction`), `stackly instance duplicate` | fitur 24 ([spec 0024](../0024-instance-kedua-runtime-sama/index.md)); kontrak, evaluator, dan operasi create spec ini tidak berubah |
   ```

2. *Evaluator create*, butir `build_form`, sisipkan sesudah kalimat terakhir butir itu: "*(fitur 24, spec 0024:* `build_form` *memanggil* `build_form_from_package(package, source, runtime, suggested_name)`*, yang juga dipakai form duplicate dengan sumber* `Pinned`*.)*"

#### Spec 0011 (secret store dan redaction)

Diterapkan di spec 0011 pada 2026-10-22 (`975b349`), persis seperti teks di bawah.

1. *Komponen UI*, butir `SecretField`, sisipkan sesudah "Dipakai sheet New Instance (fitur 20), yang menawarkan Generate, Enter, dan Copy from tanpa Bind.": "*(fitur 24, spec 0024:* `SecretField` *menerima* `copyFromLabels: [String: String]` *(default kosong) di samping* `copyFromOptions`*; picker Copy from menampilkan label itu untuk reference yang punya label, dan teks reference untuk yang lain. Pemanggil yang mengisi* `SecretDraft.copyFromReference` *lebih dulu membuat Copy from langsung menunjuk reference itu, sementara pilihan awal tetap Generate.)*"

#### Spec 0021 (start, readiness, dan connection)

Diterapkan di spec 0021 pada 2026-10-22 (`70e580e`, merge `ca18158`): menu More satu bagian, "Duplicate…" sesudah Copy Connection String tanpa pemisah.

1. *Halaman detail*, butir toolbar yang diawali "Toolbar: tombol aksi utama", sisipkan sesudah kalimat yang berakhir "nonaktif dengan alasan "No connection string available." atau alasan entri).": "*(fitur 24, spec 0024: item More kedua adalah "Duplicate…", sesudah Copy Connection String.)*"

Tidak ada update untuk spec 0010 (kode yang dipakai ulang sudah ada), 0022 (halaman Instances hanya menerbitkan action baru lewat aturan spec 0069), dan 0069 (baris `duplicateInstance` sudah ada di peta; fitur ini mengikuti aturan *Menambah perintah*).

### Selisih mockup yang disengaja

Untuk dicatat lead di `ui-macos.md`:

- Mockup membuka sheet create yang sama dengan runtime yang bisa diganti; spec ini mengunci runtime sumber dan menampilkannya sebagai baris read only tanpa filter kategori.
- Subjudul sheet di mode duplicate menyebut sumber dan bahwa data tidak disalin.
- Mockup mengisi angka port dalam mode auto; sheet menampilkan Automatic dengan "Assigned when the instance starts" (spec 0020).
- Password tidak ikut tersalin: secret mulai dari Generate, dan Copy from hanya terisi lebih dulu.
- Umpan balik menyebut sumber ("created from “{sumber}”").

## Build plan

Urutan mengikuti pendekatan Journey proyek. Jalur yang dibuat lengkap adalah "pengguna punya satu instance, membuat instance kedua dari runtime yang sama lewat create biasa atau Duplicate…, lalu keduanya berjalan dengan data sendiri", lebih dulu di core dan fixture, lalu lewat daemon dan CLI, lalu di GUI, lalu dibuktikan dengan PostgreSQL 17 nyata.

**Milestone 1: Draft duplicate di core dan sentinel fake** (needs fitur 20 milestone 1 (`instance::create` core: `build_form`, `CreateDraft`, `gather`, `evaluate`) di `02-instalasi-pertama.md`)

1. Fake server: rute `PUT /sentinel` dan `GET /sentinel` di `crates/fake-server/src/serve.rs` dengan test di `crates/fake-server/tests/dummy_server.rs`. Memenuhi **AC-7**, **AC-8** (alat bukti).
2. Core: `build_form_from_package` di `instance::create::form` dengan `build_form` yang memanggilnya (test form spec 0020 tetap lulus tanpa perubahan), modul `instance::duplicate` (`DuplicateDeps`, `DuplicateFacts`, `gather_duplicate` dengan `load_package` spec 0010 yang dijadikan `pub(crate)` dan pemeriksaan digest catalog, `build_duplicate`, `duplicate_name`, `DuplicateError`), tabel test murni, dan test `gather_duplicate` di `FakePlatform` dan `FakeRegistry`. Memenuhi **AC-1** sampai **AC-5**, **AC-7** (bagian core).

Sesi paralel: langkah 1 dan 2 bisa menjadi dua sesi (crate berbeda). Langkah 2 tidak berjalan bersamaan dengan sesi fitur 20 yang masih mengubah `crates/core/src/instance/create/form.rs`.

**Milestone 2: Kontrak, daemon, CLI, dan bukti fixture** (after milestone 1; needs fitur 20 milestone 2 (kontrak, handler, dan `stackly instance create`) dan fitur 22 milestone 2 (`resolve.rs`, `cli.instance_not_found`) di `02-instalasi-pertama.md`)

3. Protokol: `GetDuplicateFormParams`, `DuplicateForm`, `DuplicateSource`, `DuplicateSecret`, method di tabel `methods!` (`recovery refused`), fixture varian baru dan `duplicate-form-fake.json` serta `duplicate-form-postgresql.json` dari paket nyata dengan test penjaga, kasus fixture tampilan untuk mode duplicate (*Kasus yang menunggu DTO* spec 0068, di commit yang sama kalau `Fixtures/` spec 0068 sudah di `main`), kenaikan minor (`CURRENT` di main ditambah 1 saat merge), `cargo xtask codegen`, dan test kontrak Swift. Memenuhi **AC-10**.
4. `stacklyd`: handler `instance.get_duplicate_form` di `spawn_blocking`, pemetaan `DuplicateModel` ke DTO (termasuk `CreateDraft` ke `PreflightCreateParams`) dan `DuplicateError` ke kode, `Api::duplicate_form`, `Api::create_from_draft(draft: PreflightCreateParams, secrets: Vec<SecretFieldInput>)` (mengubah draft menjadi `CreateInstanceParams`), `put_sentinel`, dan `get_sentinel` di `apps/daemon/tests/supervisor/support.rs`, dan integration test di modul baru `apps/daemon/tests/supervisor/same_runtime.rs` untuk dua instance, data tidak disalin, secret, port, error, dan kinerja. Memenuhi **AC-1**, **AC-4** sampai **AC-8**.
5. CLI: `stackly instance duplicate` dengan resolusi sumber, penerapan flag di atas draft, `--secret <field>=copy`, baris stderr, key `source` di JSON, golden, matriks paritas, dan journey JOURNEY-02 di fake. Memenuhi **AC-11**.

Sesi paralel: setelah langkah 3 masuk, langkah 4 dan 5 bisa menjadi dua sesi (daemon dan CLI di folder berbeda; matriks paritas dan journey di langkah 5 menunggu langkah 4 di merge).

**Milestone 3: Duplicate di GUI** (after langkah 3; needs fitur 20 milestone 3 (sheet New Instance), fitur 21 milestone 3 (halaman detail dan menu More), dan fitur 22 milestone 3 (halaman Instances dan penerbitnya) di `02-instalasi-pertama.md`; fitur 69 milestone 3 (menu Instance dan penerbit halaman Instances) dan fitur 67 di `ui-macos.md`; fitur 11 milestone "Swift client and GUI secret flows" (langkah 9 sampai 11, `SecretField`) di `01-foundation.md`)

6. `StacklyClient` `getDuplicateForm` dan test fixture; `CreateMode`, `CreateInstanceService.getDuplicateForm`, dan state mode duplicate di `CreateInstanceModel` (tugas muat yang memanggil `getDuplicateForm` bersama `listSecrets` spec 0020, `copyFromOptions(for:)`, dan peta `copyFromLabels` gabungan), dengan test Swift dari `duplicate-form-*.json` dan service tiruan. Memenuhi **AC-12**.
7. Sheet mode duplicate (baris Runtime, subjudul, `SchemaFormModel` yang meneruskan `copyFromOptions(for:)` dan `copyFromLabels` gabungan ke setiap `SecretField` (parameter `copyFromLabels` dibangun langkah 14 spec 0020), pengisian `copyFromReference`, umpan balik), `CreateSheetRequest` dan `duplicateInstanceAction` di `AppShell`, item More di halaman detail, penerbit `duplicateInstance` di halaman Instances, baris `duplicateInstance` built dengan `CommandItem` di menu Instance, tabel perintah `design.md`, preview, suite snapshot mode duplicate kalau harness spec 0068 sudah di merge, dan evidence manual keyboard serta VoiceOver dengan daemon development dan fake provider. Memenuhi **AC-12**, **AC-13**, **AC-14** (bagian fake). *(diselaraskan dengan build, 2026-10-22: parameter additive `SecretField.copyFromLabels` di `Secrets/SecretField.swift` dibangun milestone 3 spec 0020 (`dev-f20-m3`), jadi milestone 3 spec ini memakainya tanpa menambahkannya lagi.)*

Sesi paralel: milestone 3 bisa berjalan bersamaan dengan langkah 4 dan 5 karena hanya butuh fixture langkah 3. Langkah 7 mengubah `AppShell.swift`, jadi tidak berjalan bersamaan dengan sesi lain yang mengubah file shell (langkah 14 spec 0020, langkah 6 dan 7 spec 0022, langkah 2 milestone 1 spec 0016, atau langkah spec 0069 yang menyentuh `StacklyCommands`).

**Milestone 4: PostgreSQL 17 nyata di Mac ini** (after milestone 2, dan milestone 3 untuk evidence GUI; needs fitur 19 milestone 3 (PostgreSQL 17 lewat daemon dan di catalog; harness test binary `postgresql` dari milestone 2 sudah di `main`) di `02-instalasi-pertama.md`; artefak PostgreSQL 17 di cache runtime xtask, selain itu `BLOCKED`)

8. Helper sentinel: `apps/daemon/tests/postgresql/sentinel.rs` dengan deklarasi `#[path]` nya di `apps/daemon/tests/postgresql.rs` (`SentinelCluster` dengan `new`, `write`, `read`, `system_identifier`, `assert_isolated`), dengan satu test kecil yang memakainya pada satu cluster. Memenuhi **AC-9** (alat bukti).
9. Test native di modul `apps/daemon/tests/postgresql/same_runtime.rs`: `two_postgresql_17_instances(site, api)` sebagai penyiapan bersama, `two_postgresql_17_instances_from_one_runtime_keep_their_sentinels`, dan `duplicate_postgresql_instance_starts_with_empty_data`, dengan evidence `native`; lalu evidence manual JOURNEY-02 GUI dengan PostgreSQL 17 (butuh milestone 3). Run native dan sesi GUI ini berjalan di Mac ini atas permintaan pengguna. Memenuhi **AC-7**, **AC-9**, **AC-14**.

Sesi paralel: langkah 8 bisa dimulai begitu milestone 2 masuk. Milestone tiga cluster spec 0025 berjalan after langkah 9.

## Consequences

**Positive**:

- Duplicate tidak punya jalur create kedua: semua validasi, kompensasi, dan bukti spec 0020 berlaku otomatis, dan bug create diperbaiki sekali.
- Data lama tidak mungkin tersalin karena tidak ada kode yang membaca isi pohon instance sumber; jaminan ini datang dari bentuk kode, dan test dengan folder data sumber mode 000 menjaganya.
- Port dan password instance hasil duplicate selalu baru tanpa keputusan tersembunyi, sesuai BR-005 dan aturan reference spec 0011.
- Helper sentinel dan penyiapan dua PostgreSQL 17 dipakai ulang spec 0025, jadi bukti AC-009 tidak ditulis dua kali.
- Rute sentinel di fake server bisa dipakai fitur isolasi berikutnya (backup, restore as new) tanpa provider nyata.

**Negative / tradeoffs**:

- Instance hasil duplicate dengan port Automatic mendapat port efemeral yang tinggi saat start pertama; pengguna yang ingin port yang mudah diingat harus memilih Fixed sendiri dan menekan "Use {n}" kalau bentrok.
- Password tidak ikut tersalin secara default; pengguna yang ingin aplikasinya cukup mengganti port harus memilih Copy from per secret field (atau `--secret <field>=copy`).
- Registry tidak mencatat asal duplicate; tab Activity (fitur 35) hanya bisa menunjukkannya dari journal create kalau nanti dibutuhkan.
- Runtime terkunci ke runtime sumber: duplicate ke mayor lain tidak didukung dan harus lewat New Instance.
- Kalau runtime sumber rusak atau dihapus, duplicate gagal sampai runtime diperbaiki, walaupun runtime lain dengan versi sama terpasang.
- *(diselaraskan dengan build, 2026-10-22)* Sejak F20 milestone 5 (`7cd4d1a9`, AC-9 spec 0020 yang disetujui pengguna Scope 02), sumber duplicate yang paket pinned nya butuh port saat `initialize` (hari ini hanya salinan NATS `1.0.0`, yang merender `nats.conf` saat init) membuat draft yang semua port nya `auto` mendapat preflight `ready: false`: baris `settings` error `instance.endpoint_invalid` dan field `endpoints[<i>].port` `invalid_value`, sampai pengguna memilih port Fixed untuk setiap endpoint yang dibutuhkan init (untuk NATS `1.0.0` itu `client` dan `monitoring`; penolakan pindah ke endpoint berikutnya selama masih ada yang `auto`). Sebelumnya preflight `ready` lalu create gagal di init dengan `supervisor.config_invalid`. NATS `1.0.1` lulus dengan semua port `auto`. AC-4 dan AC-6 tetap benar apa adanya: draft tetap meminta `auto`, dan duplicate memakai preflight dan create spec 0020 tanpa jalur sendiri. Aturan dan teks nya ada di AC-9, *Copy preflight*, dan *Consequences* [spec 0020](../0020-create-instance/index.md).
- Untuk revision yang labelnya sama dengan catalog, create memakai paket catalog (spec 0020), jadi duplicate harus menolak lebih dulu kalau catalog development memuat isi lain dengan label yang sama (`instance.provider_ambiguous`); pengguna harus menaikkan revision paket atau memakai New Instance. Catalog bawaan tidak bisa sampai ke keadaan ini karena ledger revision nya.
- Dua atau tiga cluster PostgreSQL bersamaan bisa mengenai batas IPC kernel macOS (semaphore atau shared memory System V) dengan setelan bawaan; test native langkah 9 yang gagal karena itu adalah temuan yang kembali ke `/architect`, bukan alasan mengubah test.

**Neutral**:

- Satu method baca baru, empat DTO, kenaikan minor protokol, tanpa kode error baru dan tanpa migrasi.
- Modul core baru `instance::duplicate` dan fungsi `build_form_from_package`; mode baru di sheet create; perintah CLI baru; dua rute fake server; modul test `sentinel` dan `same_runtime` di test binary `postgresql` dan `same_runtime` di test binary `supervisor`.
- `CreateMode` dan `CreateSheetRequest` mengubah cara `AppShell` mempresentasikan sheet New Instance.

## Follow-up

- [x] Lead: bawa *Usulan update untuk spec lain* untuk spec 0021 ke pemiliknya sebelum milestone 3; blok spec 0020 dan 0011 sudah diterapkan (2026-10-22). Blok spec 0021 diterapkan lead Scope 02 di `70e580e` (merge `ca18158`, 2026-10-22).
- [x] Lead: catat *Selisih mockup yang disengaja* di `ui-macos.md` lewat `/scope`. Dicatat lead `ui-macos` di `2b1e4f3` (2026-10-22).
- [x] Lead: spec 0020 tidak menyebut dari mana `copyFromOptions` `SecretField` di mode create diisi (misalnya `secret.list`); mode duplicate spec ini hanya menambahkan reference sumber di depan daftar itu. Putuskan lewat `/architect` update spec 0020 sebelum milestone 3 spec 0020; dijawab spec 0020 Keputusan 19 (`49f0f4f`, 2026-10-22).
- [x] Spec 0025: pakai *Helper sentinel* dan `two_postgresql_17_instances` (disepakati dengan arc-f25-spec, 2026-10-22); milestone tiga cluster after langkah 9. Diterapkan di spec 0025 (`0afcd9b`, milestone 3 langkah 6).
- [ ] Fitur 35: kalau tab Activity perlu menampilkan "created from", ambil dari journal create (reference `copyFrom` dan nama di draft) atau putuskan field asal di create lewat `/architect`.
- [ ] Fitur 54: remove instance yang reference secret nya di `bind` oleh instance lain mengikuti aturan `in_use` spec 0011.
- [ ] Fitur 26: restart sebagai satu operasi memakai ulang skenario sentinel langkah 9 untuk membuktikan isolasi setelah restart.
- [ ] `/sync` setelah build: catat `instance.get_duplicate_form`, `stackly instance duplicate`, rute `/sentinel` fake server, helper `apps/daemon/tests/postgresql/sentinel.rs`, dan mode duplicate sheet di `AGENTS.md` root dan `apps/macos/AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, kesepakatan antarsesi, dan hasil cross check ada di [rationale.md](rationale.md).
