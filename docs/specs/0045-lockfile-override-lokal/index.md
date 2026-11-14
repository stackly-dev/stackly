# 0045. Lockfile dan override lokal

**Date**: 2026-11-13
**Status**: Accepted

## Summary

Spec ini menambah dua file di samping `stackly.yaml`. `stackly.lock` mencatat pilihan tepat untuk setiap service (revision provider, versi runtime, dan digest artefak per platform) supaya seluruh tim menjalankan binary yang sama; `.stackly.local.yaml` berisi penyesuaian milik satu Mac saja (port dan lokasi data) yang tidak pernah mengubah file tim. `stackly env lock` menulis lock dari catalog yang dipercaya tanpa menjalankan provider, dan plan (juga Up, lewat plan) memakai lock itu: dengan `--locked` tidak ada versi yang dipilih ulang, dan Mac yang tidak punya artefak yang dikunci berhenti dengan pesan jelas, bukan memakai versi lain. `stackly env export` mencetak host dan port aktual setiap service sebagai variabel environment, tanpa secret, path home, atau PID.

## Requirements

**User stories**:

- Sebagai anggota tim, saya ingin `stackly.lock` yang bisa saya commit, supaya rekan saya dan CI mendapat versi dan binary yang persis sama dengan milik saya.
- Sebagai penulis script CI, saya ingin `stackly env up --locked` gagal saat lock tidak cocok dengan manifest atau dengan mesin itu, bukan diam diam memilih versi lain.
- Sebagai developer yang port 5432 nya sudah terpakai, saya ingin mengganti port dan lokasi data di file lokal saya tanpa menyentuh `stackly.yaml` milik tim.
- Sebagai developer aplikasi, saya ingin mengambil host dan port aktual setiap service (termasuk port `auto`) sebagai variabel environment, tanpa password ikut tercetak.
- Sebagai pengguna GUI, saya ingin melihat apakah environment saya terkunci dan override lokal apa yang berlaku.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-023):

- **AC-1** (isi lock, REQ-023, BR-018, PRD 13.3): `stackly.lock` adalah dokumen YAML `apiVersion: stackly/v1alpha1`, `kind: EnvironmentLock` sesuai *Schema `stackly.lock`*: satu entri per service dedicated berisi `provider`, `providerRevision`, `providerManifestSha256`, `selector`, `version`, `line`, dan `artifacts` (satu per target: `target`, `id`, `sha256`). File itu tidak punya field untuk secret, reference secret, path, port, settings, nama host, PID, waktu, atau URL unduhan. Byte nya ditentukan sepenuhnya oleh isinya (service urut alias, artefak urut target, emitter tetap), jadi dua Mac dengan manifest dan catalog yang sama menghasilkan file yang sama byte demi byte. Test menulis lock dari root yang menyimpan nilai secret sentinel dan override lokal ber path home, lalu mencari sentinel, path home, dan PID daemon di file itu.
- **AC-2** (`env.lock` memilih versi, PRD 13.1): `env.lock { source, refresh }` membaca dan memvalidasi manifest seperti `env.validate`, lalu untuk setiap service dedicated memilih artefak lewat gate trust (`require_entry_trust`) dan `runtime::resolve` terhadap catalog di memori, dan mencatat setiap artefak `available` versi itu untuk semua target yang catalog punya. Service `instanceRef` tidak pernah masuk lock. Method ini tidak mengunduh, tidak memanggil `ProcessSupervision`, tidak membuka koneksi jaringan, tidak menulis registry (termasuk operasi dan audit), tidak menyentuh Keychain, dan tidak mengubah trust; satu satunya file yang ia tulis adalah file lock, lewat `Filesystem::write_file_atomic`. Kalau ada service yang tidak bisa dipilih (provider `pending` atau `revoked`, selector tidak ter resolve, artefak tidak installable), balasan membawa `blockers` dengan kode yang sama dengan plan spec 0044 dan file tidak ditulis sama sekali.
- **AC-3** (lock yang ada dipertahankan): dengan `refresh` `none`, entri yang masih berlaku menurut *Entri yang dipertahankan* tidak berubah versinya; hanya service baru atau yang entrinya basi yang dipilih ulang, dan entri service yang sudah tidak ada dibuang. `refresh` `all` memilih ulang semuanya; `refresh` `services` memilih ulang alias yang disebut (alias yang tidak ada di manifest, atau yang service nya `instanceRef`, ditolak `env.service_not_found`). Lock yang ada tetapi `apiVersion` atau `kind` nya tidak dikenal (misalnya ditulis Stackly yang lebih baru) tidak pernah ditimpa: balasan membawa blocker `env.lock_invalid`. Lock yang rusak dengan cara lain hanya diganti dengan `refresh` `all`; dengan `none` atau `services` ia juga menjadi blocker `env.lock_invalid`. Kalau byte hasilnya sama dengan file yang ada, file tidak ditulis ulang dan balasan membawa `written` false. Setiap service di balasan membawa `change` (`added`, `kept`, `updated`, `removed`) dan versi sebelumnya kalau berubah.
- **AC-4** (state lock): `environment::lock::state(manifest, lock)` adalah fungsi murni yang mengembalikan `absent`, `unreadable`, `invalid`, `stale`, atau `in_sync` menurut *State lock*, dengan satu `LockStaleItem` per alias yang tidak cocok (`not_locked`, `provider_changed`, `selector_changed`, `selector_unsatisfied`, `not_in_manifest`). Selector dibandingkan sesudah di parse ke `RuntimeSelector` (lewat teks `Display` nya, dengan `version` yang tidak ditulis berarti `stable`), bukan sebagai teks YAML mentah. Mengubah port, settings, `secretRefs`, `dependsOn`, komentar, atau urutan service di manifest tidak pernah membuat lock basi.
- **AC-5** (plan memakai lock, REQ-023): `env.plan` membaca lock di folder manifest. Untuk item `create` yang entri lock nya cocok, runtime diambil dari artefak yang dikunci untuk target Mac ini lewat jalur digest `runtime::resolve` (`RuntimeSelector::Sha256`), `PlanRuntime.source` bernilai `lock`, dan selector manifest tidak pernah di resolve. Dengan `locked` false, service tanpa entri atau dengan entri basi di resolve dari catalog seperti spec 0044, `PlanRuntime.source` bernilai `catalog`, dan service itu tercatat di `EnvPlan.lock.stale`. Tanpa file lock, plan sama persis dengan plan spec 0044 dan `lock.state` `absent`.
- **AC-6** (`--locked`, PRD 13.1, AC-023): dengan `locked` true, plan tidak pernah memanggil `runtime::resolve` dengan selector manifest. Lock yang tidak ada (`env.lock_missing`), tidak terbaca (`env.lock_unreadable`), tidak valid (`env.lock_invalid`), atau basi untuk alias mana pun (satu `env.lock_stale` per alias) menjadi blocker, `applicable` false, dan `planDigest` tidak dikirim. Karena Up spec 0046 hanya menjalankan isi plan, `stackly env up --locked` berhenti sebelum operasi dibuat. Daemon yang belum mengenal `locked` mengabaikan field itu, jadi klien yang mengirim `locked` true wajib memeriksa `EnvPlan.lock.locked` di balasan dan menolak dengan `cli.env_locked_unsupported` kalau nilainya bukan true; tanpa cek itu daemon lama akan me resolve ulang diam diam.
- **AC-7** (tanpa substitusi, REQ-023, AC-023): entri lock yang cocok dengan manifest dipenuhi persis atau menjadi blocker, di kedua mode: tidak ada artefak yang dikunci untuk target Mac ini (`env.lock_no_artifact`, dengan `target` dan `lockedTargets`), digest yang dikunci tidak dikenal catalog atau versi dan targetnya di catalog berbeda dari isi lock (`env.lock_artifact_unknown`), revision provider di catalog berbeda dari yang dikunci (`env.lock_provider_mismatch`), atau gate menolak artefak itu (`catalog.not_installable` dengan alasannya). Plan tidak pernah mengganti artefak yang dikunci dengan artefak lain, versi lain, atau artefak target lain. Test memakai catalog fixture dengan dua target dan memeriksa bahwa Mac ber target ketiga mendapat blocker, bukan item `create`.
- **AC-8** (member yang sudah ada): lock hanya menentukan runtime item `create`. Service yang sudah punya member tetap `keep` atau `start` menurut spec 0044; kalau runtime member itu `managed` dan `artifact_sha256` nya berbeda dari artefak yang dikunci untuk target ini, item membawa `differences` dengan `field` `lock`. Perbedaan itu tidak pernah menjadi blocker, juga dengan `locked` true, dan Stackly tidak pernah mengganti runtime instance supaya cocok dengan lock.
- **AC-9** (file override lokal, PRD 13.3): `.stackly.local.yaml` adalah dokumen `apiVersion: stackly/v1alpha1`, `kind: EnvironmentLocal` sesuai *Schema `.stackly.local.yaml`*, dibaca dengan loader ketat yang sama dengan manifest (paling besar 64 KiB). Ia hanya menerima `services.<alias>.endpoints.<nama>.port` dan `services.<alias>.storage.dataLocation`. Setiap baris tabel *Aturan override* diterapkan dan semua finding dikembalikan di `EnvPlan.local.findings`; finding ber severity `error` menjadi satu blocker `env.local_invalid`, dan file yang ada tetapi tidak bisa dibaca menjadi blocker `env.local_unreadable`, di kedua mode. Alias yang tidak ada di manifest hanya `warning` (`override_unknown_service`) dan override nya diabaikan. File override yang punya finding `error` tidak diterapkan sama sekali, juga bagiannya yang benar: plan memakai permintaan port dan lokasi dari manifest, dan setiap entri `local.overrides` ber state `ignored`, tidak pernah `applied` *(ditambah 2026-11-13)*. `env.validate` tidak membaca file ini, jadi hasil validate tetap sama di setiap Mac.
- **AC-10** (override port): override port mengganti permintaan port endpoint itu di draft create sebelum `gather` dan `evaluate`, jadi cek bentrok, `PlanItem.endpoints`, dan `planDigest` memakai port efektif. Dua endpoint fixed service `create` yang sesudah override memakai alamat dan port yang sama ditolak `override_endpoint_conflict`, dengan aturan tumpang tindih `endpoint_conflict` spec 0044 dan host dari `defaultHost` paket; bentrok dengan instance terdaftar tetap `supervisor.endpoint_conflict` dari `evaluate`. File manifest, `manifestSha256`, snapshot `environments.services`, dan file lock tidak pernah berubah karena override; test membandingkan byte `stackly.yaml` dan `stackly.lock` serta `manifestSha256` sebelum dan sesudah plan dengan override. Untuk service yang sudah punya member, override tidak diterapkan ke instance; `differences` `endpoint.<nama>.port` dihitung terhadap port efektif (override menang atas manifest).
- **AC-11** (override storage): `storage.dataLocation` harus menunjuk lokasi data yang sudah terdaftar di `storage_locations` (spec 0032) menurut *Aturan override*; kalau tidak, plan membawa blocker `env.override_location_unknown`. Item `create` nya membawa `PlanItem.storage { locationId, path }`, dan create menerima lokasi itu lewat `CreateDraft.data_location`: `gather_locations` memakai lokasi itu sebagai lokasi data (dan memeriksa foldernya seperti lokasi default), lalu commit lewat `WriteOp::InsertInstanceAt` yang sudah ada. Lokasi log dan lokasi default tidak berubah. Lokasi yang foldernya tidak terjangkau menjadi blocker lewat baris check storage create. Override storage tidak pernah diterapkan ke service yang sudah punya member (state `not_applied_existing`) dan tidak pernah memindah data.
- **AC-12** (plan dan registry): `EnvPlan` membawa `lock: PlanLock` (selalu ada) dan `local: PlanLocal` (kalau file nya ada); `PlanItem` item `create` membawa `providerRevision`. `planDigest` ikut menghitung `locked`, `lock.sha256`, `local.sha256`, dan per item `runtime.source`, `providerRevision`, dan `storage.locationId`, jadi plan yang ditinjau dengan satu mode, lock, atau override tidak bisa dijalankan dengan yang lain. Untuk manifest yang tidak valid, `lock.state` `in_sync` hanya berarti file lock terbaca dan valid (basi tidak dihitung tanpa manifest), `lock.stale` kosong, dan klien tidak menampilkan state lock (*State lock*) *(ditambah 2026-11-13)*. `environments.lock_sha256` ditulis lewat `Environment::set_manifest` oleh Up spec 0046 dengan nilai `EnvPlan.lock.sha256` (NULL kalau plan tidak punya lock yang valid); import spec 0044 meneruskan nilai yang tersimpan tanpa mengubahnya. `env.lock` tidak pernah menulis kolom itu.
- **AC-13** (`env.get`): dengan `checkManifest` true dan `manifestState` `in_sync` atau `changed`, `EnvironmentDetail` membawa `lock: EnvLockInfo` (file, `state`, target yang dikunci, jumlah service) dan `local: EnvLocalInfo` (file, `state`, daftar override). `local.state` tidak dikirim kalau file override tidak ada; `local` sendiri tetap dikirim dengan `overrides` kosong dan `problems` 0 *(ditambah 2026-11-13)*. Dengan `checkManifest` false keduanya tidak dikirim dan tidak ada file yang dibaca. `EnvironmentSummary.lockSha256` berisi kolom registry apa adanya.
- **AC-14** (`env.export`, PRD 13.3): `env.export { source, format }` membalas satu variabel `HOST`, `PORT`, dan `ADDRESS` per endpoint IP setiap service yang punya instance (member, atau instance `instanceRef` yang bernama sama untuk service shared tanpa member), dengan port efektif instance itu, termasuk port `auto` yang sudah dialokasikan, menurut *Variabel export*. Endpoint yang belum punya port, endpoint unix, dan service yang belum dibuat masuk `skipped` dengan alasannya dan tidak pernah dicetak sebagai nilai kosong. `text` berformat `dotenv` atau `shell`. Method ini hanya membaca registry dan paket pinned: tidak ada operasi, audit, file, Keychain, atau proses.
- **AC-15** (export tanpa secret, BR-018, AC-080, spec 0011): balasan `env.export` tidak pernah memuat nilai secret, reference secret, connection string, path (termasuk path socket dan path home), atau PID, dan tidak ada opsi untuk menyertakannya. Urutannya: `export::collect` menyusun nilai, memeriksa karakternya, lalu melewatkan setiap `value` ke `Redactor`; variabel yang nilainya diubah `Redactor` tidak dicetak dan masuk `skipped` dengan alasan `redacted`, lalu `text` disusun hanya dari variabel yang lolos. Test menyimpan secret sentinel untuk setiap service, menjalankan export di kedua format, dan mencari sentinel, path home, dan PID setiap proses instance di balasan, di stdout CLI, dan di log daemon; satu test unit memberi `Redactor` nilai yang sama dengan alamat sebuah endpoint dan memeriksa bahwa variabel itu masuk `skipped` dan tidak ada di `text`.
- **AC-16** (CLI lock, REQ-028): `stackly env lock [--file <path> | <environment>] [--update] [--service <alias>]...` memanggil `env.lock`. Sukses mencetak satu baris per service ke stderr dan "Locked <n> services in <file>." atau "<file> is up to date."; exit 0. Manifest tidak valid keluar 2 dengan `cli.env_invalid`; blocker keluar 3 dengan `cli.env_lock_blocked`; alias `--service` yang tidak dikenal keluar 10. `result` `--json` selalu `EnvLockResult`, dan stdout bersih dari progress.
- **AC-17** (CLI plan dan export, REQ-028): `stackly env plan --locked` mengirim `locked` true dan menjalankan cek AC-6 (`cli.env_locked_unsupported`, exit 5); keluaran plan mendapat baris `lock:` dan `local:` serta kolom `SOURCE` menurut *CLI*. `stackly env export [--file <path> | <environment>] [--format dotenv|shell]` mencetak tepat `EnvExport.text` ke stdout dan daftar `skipped` ke stderr, exit 0; environment yang belum terdaftar keluar 10. Hasilnya sama persis dengan teks yang disalin GUI untuk environment yang sama.
- **AC-18** (baris Lock dan Local override): seksi Details halaman environment mendapat baris "Lock" dan "Local override" dengan teks menurut *Layar*, dan menu More mendapat Lock Versions, Update Lock…, dan Copy Environment Variables. Lock Versions dan Update Lock… memakai `.mutationGated()`; Update Lock… meminta konfirmasi.
- **AC-19** (sheet plan dan state, NFR-012): sheet plan menandai item yang versinya dari lock, menampilkan setiap override yang berlaku, dan menampilkan lock yang basi sebagai catatan caution; blocker lock dan override tampil lewat banner blocker spec 0044 dengan hint kodenya. Setiap status berupa teks dan simbol, tidak hanya warna, dan setiap aksi punya state gagal dan offline dari `ViewState`.
- **AC-20** (kontrak): `ProtocolVersion::CURRENT` naik satu minor untuk seluruh kontrak fitur ini; method `env.lock` dan `env.export` (`recovery refused`), field baru, kode `env.*` baru, dan fixture di `protocols/fixtures/env/` masuk dalam satu commit dengan `cargo xtask codegen`; `cargo xtask codegen` menulis `schemas/environment/lock.v1alpha1.schema.json` dan `schemas/environment/local.v1alpha1.schema.json` dari tipe raw di `stackly-core`; `cargo xtask check-deps` dan `cargo check --target aarch64-pc-windows-msvc` untuk domain, platform, protocol, dan core tetap lulus.
- **AC-21** (pindah ke user lain, AC-023, AC-080): root A menyimpan secret untuk setiap `secretRefs`, punya override port dan storage, dan menulis lock. Root B (home lain, registry kosong) menerima hanya `stackly.yaml` dan `stackly.lock`, lalu menjalankan `env.plan` dengan `locked` true: plan memilih artefak yang sama dengan A, tidak memuat path home A, nilai secret A, atau override A, dan setiap reference tampil `unregistered`. Dengan catalog fixture yang tidak punya artefak untuk target B, plan yang sama membawa `env.lock_no_artifact` dan nol item yang memakai artefak lain.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan K1 sampai K21 di [rationale.md](rationale.md): lock YAML yang deterministik dan hanya berisi pilihan (bukan sumber trust), dipakai plan di kedua mode dan wajib cocok dengan `--locked`; override lokal terbatas pada port dan lokasi data; export berupa variabel endpoint yang dibangun daemon.

Lock adalah catatan pilihan, catalog tetap satu satunya sumber artefak yang dipercaya, dan semua aturannya hidup di plan supaya Up, GUI, dan CLI tidak punya logika lock sendiri.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

## Feature design

### Trace PRD

| ID PRD | Dicakup oleh |
| --- | --- |
| REQ-023 | AC-1, AC-2, AC-5, AC-6, AC-7, AC-9, AC-21 |
| REQ-028 | AC-16, AC-17 |
| BR-018 | AC-1, AC-15 |
| AC-023 | AC-6, AC-7, AC-21 ("diekspor" di sini berarti `stackly.yaml` dan `stackly.lock` yang dibagikan; lihat K1) |
| AC-080 (bagian export dan lockfile) | AC-1, AC-15, AC-21 (bagian plan milik spec 0044 AC-6) |
| PRD 13.1 (`env lock`, `--locked`) | AC-2, AC-6 |
| PRD 13.3 | AC-1, AC-7, AC-9, AC-10, AC-11, AC-14 |

### Batas dengan spec lain

| Hal | Pemilik |
| --- | --- |
| Schema `stackly.yaml`, validate, bentuk dasar plan, import, remove, halaman Environments | spec 0044 |
| `env.up`, `EnvUpParams.locked` (diteruskan apa adanya ke plan), pembuatan member, penulisan `lock_sha256` saat Up, `stackly env up --locked`, `stackly env status` | spec 0046 |
| Lokasi storage dan aturannya | spec 0032, dipakai ulang |
| Resolve runtime dan gate install | spec 0017 dan 0018, dipakai ulang |
| Trust provider lokal | spec 0037, dipakai ulang |
| `Redactor` | spec 0011, dipakai ulang |

### Nama file

Ketiga file ada di folder yang sama. Dengan `<stem>` nama file manifest tanpa ekstensi terakhirnya (`.yaml` atau `.yml`):

| File | Nama | Untuk `stackly.yaml` |
| --- | --- | --- |
| Lock | `<stem>.lock` | `stackly.lock` |
| Override lokal | `.<stem>.local.yaml` | `.stackly.local.yaml` |

Manifest tanpa ekstensi `.yaml` atau `.yml` memakai seluruh nama filenya sebagai `<stem>`. Kedua file dibaca dengan `Filesystem::read_file_limited` (bukan symlink, file biasa): lock paling besar 256 KiB, override 64 KiB.

### Schema `stackly.lock`

```yaml
# Written by `stackly env lock`. Commit this file. Do not edit it by hand.
apiVersion: "stackly/v1alpha1"
kind: "EnvironmentLock"
services:
  postgres:
    provider: "official/postgresql"
    providerRevision: "3"
    providerManifestSha256: "<64 hex>"
    selector: "18"
    version: "18.6"
    line: "18"
    artifacts:
      - target: "macos-aarch64"
        id: "postgresql-18.6-macos-aarch64"
        sha256: "<64 hex>"
```

| Field | Aturan |
| --- | --- |
| `apiVersion`, `kind` | tepat `stackly/v1alpha1` dan `EnvironmentLock` |
| `services` | map 0 sampai 32 entri; key `MemberAlias`; ditulis urut byte alias |
| `provider` | `ProviderName` |
| `providerRevision`, `providerManifestSha256` | `revision` dan `manifest_sha256` dari `ProviderRevisionKey` entri catalog saat dikunci |
| `selector` | teks selector manifest saat dikunci (`stable` kalau manifest tidak menulis `version`) |
| `version`, `line` | `ExactVersion` dan `RuntimeLine` artefak yang dipilih |
| `artifacts[]` | 1 sampai 16 entri, satu per target, urut teks target; `target` teks `Target` (`<os>-<arch>`), `id` ID artefak catalog, `sha256` digest unduhannya |

Loader: loader ketat spec 0044 (field tak dikenal, key ganda, nilai bertag, merge key, dan `apiVersion` atau `kind` lain ditolak; anchor dan alias YAML diterima dalam anggaran expand, menurut *YAML yang diterima* [spec 0044](../0044-manifest-environment-validate-plan/index.md)) *(diselaraskan 2026-11-13 dengan spec 0044: sebelumnya anchor dan alias ditolak)*. Target ganda dalam satu service ditolak. Baris komentar pertama boleh ada atau tidak.

Emitter: `environment::lock::emit` menulis byte sendiri (bukan `serde_yaml_ng::to_string`): baris komentar di atas, indent dua spasi, setiap nilai string dalam kutip ganda, akhir baris `\n`, satu `\n` di akhir file. Golden test menjaga bentuknya.

### State lock

`environment::lock::state(manifest, lock_file)` (murni, tanpa catalog), nilai pertama yang cocok menang:

1. `absent`: file tidak ada.
2. `unreadable`: `read_file_limited` gagal.
3. `invalid`: loader menolak dokumennya.
4. `stale`: ada alias yang tidak cocok menurut tabel di bawah.
5. `in_sync`.

*(Ditambah 2026-11-13, K23.)* Fungsi ini butuh manifest yang valid untuk butir 4. Untuk manifest yang tidak valid, `env.plan` berhenti di butir 3: lock yang terbaca dan valid dilaporkan `in_sync` dengan `stale` kosong, yang di sini hanya berarti "terbaca dan valid". `env.get` tidak mengirim `lock` sama sekali untuk `manifestState` `invalid`. Klien yang menerima `EnvPlan.validation.valid` false tidak menampilkan state lock, banner lock basi, atau baris `lock:`; mereka hanya menampilkan finding manifest. Konsumen `--json` membaca `lock.state` hanya saat `validation.valid` true. *(Diselaraskan dengan build (2026-11-14): K23 dibangun seperti tertulis. Daemon tetap mengirim `lock.state` `in_sync` untuk manifest tidak valid di `env.plan`; CLI tidak mencetak baris `lock:` dan `local:`, dan sheet plan tidak menampilkan keterangan lock atau override, saat `validation.valid` false. Keduanya dikunci golden `env_plan_invalid_stderr.txt` dan snapshot `environment-plan-invalid-locked`.)*

| `LockStaleReason` | Kapan |
| --- | --- |
| `not_locked` | service dedicated tanpa entri lock |
| `provider_changed` | `provider` entri berbeda dari `provider` service |
| `selector_changed` | `selector` entri berbeda dari selector service (tanpa `version` berarti `stable`) |
| `selector_unsatisfied` | selector teks tidak sama dengan `version` maupun `line` entri; atau selector `sha256:` yang digest nya tidak ada di `artifacts` entri. Selector `stable` selalu terpenuhi |
| `not_in_manifest` | entri lock yang alias nya tidak ada di manifest, atau service nya kini `instanceRef` |

Entri yang lolos semua baris disebut **cocok**.

### Entri yang dipertahankan

`env.lock` dengan `refresh` `none` mempertahankan entri yang cocok kalau salah satu berlaku: artefak yang dikunci untuk target Mac ini masih lolos *Runtime dari lock* langkah 3 sampai 5; atau entri belum punya artefak untuk target Mac ini dan catalog punya artefak `available` ber versi yang dikunci untuk target ini yang lolos gate install (artefak itu lalu ditambahkan, versinya tetap). Entri yang dipertahankan:

- `version`, `line`, dan setiap artefak yang sudah tercatat tidak berubah.
- `providerRevision` dan `providerManifestSha256` diganti dengan key entri catalog saat ini.
- Target yang belum tercatat dan punya artefak `available` versi itu di catalog ditambahkan.

Entri lain dipilih ulang: gate trust, `runtime::resolve(entry, selector, host, trust)`, lalu `artifacts` berisi satu artefak per target untuk versi itu: untuk target Mac ini artefak hasil resolve; untuk target lain artefak pertama target itu dalam urutan catalog (aturan gate spec 0017), dan hanya dicatat kalau statusnya `available`. Aturan yang sama dipakai saat menambah target ke entri yang dipertahankan.

Pembanding `change` adalah lock yang ada kalau terbaca dan valid; lock rusak yang diganti lewat `refresh` `all` dianggap kosong, jadi semua entrinya `added`. `change`: `added` (alias baru), `kept` (versi sama, termasuk kalau hanya revision provider atau daftar target yang berubah), `updated` (versi berubah, dengan `previousVersion`), `removed` (entri dibuang).

### Runtime dari lock

Untuk item `create` yang entri lock nya cocok, di kedua mode:

1. Jalankan gate trust persis seperti baris `create` spec 0044 (`require_entry_trust`): provider `pending` atau `revoked` menjadi blocker `provider.approval_required` atau `provider.revoked`, dan langkah berikutnya dilewati.
2. Bandingkan `ProviderRevisionKey` entri catalog dengan `providerRevision` dan `providerManifestSha256` entri lock. Berbeda: blocker `env.lock_provider_mismatch`.
3. Cari artefak entri lock yang `target` nya sama dengan `HostFacts.target`. Tidak ada: blocker `env.lock_no_artifact`.
4. Cari digest itu di `entry.artifacts()`. Tidak ada: `env.lock_artifact_unknown` dengan `reason` `not_in_catalog`. Ada tetapi `version`, `target`, atau `id` nya berbeda dari isi lock: `env.lock_artifact_unknown` dengan `reason` `version_mismatch`, `target_mismatch`, atau `id_mismatch` (yang pertama cocok dalam urutan itu).
5. Panggil `runtime::resolve(entry, RuntimeSelector::Sha256(digest), host, trust)`. Aturan jalur digest spec 0018 sudah menolak artefak host sebagai pengganti. `NotInstallable` menjadi `catalog.not_installable` dengan alasannya (kode yang sama dengan spec 0044); `Unresolved` (tidak mungkin sesudah langkah 4) dipetakan ke `not_in_catalog`.
6. Lolos: `PlanRuntime` diisi dari artefak catalog itu (`version`, `artifactId`, `artifactSha256`, `downloadSize`), `selector` tetap teks manifest, `source` `lock`, dan `install` serta `runtimeId` dihitung seperti spec 0044 (baris runtime `managed` ber `artifact_sha256` itu dengan state `installed`).

Lock tidak pernah dipakai untuk mengunduh: URL, ukuran, dan aturan signing selalu dari catalog. Runtime terpasang yang digest nya sama tidak menggantikan langkah 4 dan 5; lihat K5.

**Mode.** Dengan `locked` false, service ber alasan `not_locked`, `provider_changed`, `selector_changed`, atau `selector_unsatisfied` di resolve dari catalog seperti spec 0044 (`source` `catalog`); `not_in_manifest` tidak memengaruhi item apa pun. Semuanya tercatat di `PlanLock.stale`. Dengan `locked` true, setiap `LockStaleItem` menjadi blocker `env.lock_stale`, state `absent` menjadi `env.lock_missing`, dan tidak ada resolve dari catalog. State `unreadable` dan `invalid` selalu blocker (`env.lock_unreadable`, `env.lock_invalid`), di kedua mode: lock yang rusak tidak pernah diabaikan diam diam.

**Member.** Untuk service ber member, baris *Aturan `differences`* spec 0044 mendapat satu `field` baru:

| `field` | Dihitung berbeda kalau |
| --- | --- |
| `lock` | entri lock cocok, punya artefak untuk target ini, runtime member `managed`, dan `artifact_sha256` runtime itu berbeda dari digest yang dikunci. `manifest` berisi versi yang dikunci, `instance` versi runtime member |

### Schema `.stackly.local.yaml`

```yaml
apiVersion: stackly/v1alpha1
kind: EnvironmentLocal
services:
  postgres:
    endpoints:
      client: {port: 5434}
    storage:
      dataLocation: /Volumes/Fast/Stackly
```

| Field | Aturan |
| --- | --- |
| `apiVersion`, `kind` | tepat `stackly/v1alpha1` dan `EnvironmentLocal` |
| `services` | map 0 sampai 32 entri; key `MemberAlias` |
| `endpoints.<nama>.port` | bilangan 1 sampai 65535 (fixed) atau string `auto`; `<nama>` harus endpoint IP paket provider service itu. Hanya `port` yang boleh ditulis |
| `storage.dataLocation` | path absolut, paling panjang 1024 byte, tanpa NUL, tanpa segmen `.` atau `..` (aturan `import_path_code`) |

### Aturan override

Finding memakai `EnvFinding` spec 0044 dengan template pesan tetap per kode. Urutan: bentuk dokumen dulu (kalau gagal, berhenti), lalu per service.

| Kode | Severity | Kapan |
| --- | --- | --- |
| `yaml_syntax`, `unknown_field`, `missing_field`, `invalid_value`, `duplicate_key`, `yaml_alias`, `unsupported_api_version`, `unsupported_kind` | error | bentuk dokumen, sama dengan manifest. `yaml_alias` berarti nilai atau key bertag, merge key, atau alias yang mengembangkan dokumen melewati anggaran expand; anchor dan alias biasa bukan finding (*YAML yang diterima* spec 0044) |
| `override_unknown_service` | warning | alias tidak ada di manifest; override nya diabaikan |
| `override_not_allowed` | error | service nya `instanceRef` (environment tidak pernah mengonfigurasi instance yang dipinjam) |
| `unknown_endpoint`, `endpoint_not_ip` | error | nama endpoint bukan endpoint IP paket |
| `override_endpoint_conflict` | error | sesudah override, dua endpoint fixed service `create` di manifest ini memakai alamat dan port yang sama; menunjuk keduanya |
| `storage_location_invalid` | error | `dataLocation` bukan path yang lolos aturan di atas |

**Penerapan** (di `environment::plan`, sesudah manifest valid dan sebelum evaluasi create):

- Port: untuk item `create`, permintaan port endpoint itu di draft diganti (`fixed` atau `auto`), juga untuk endpoint yang tidak ditulis manifest (yang tanpa override berarti `auto`). Untuk service ber member, draft tidak ada; override hanya mengganti nilai pembanding `differences` `endpoint.<nama>.port`.
- Storage: `dataLocation` di canonicalize seluruhnya lewat `Filesystem::canonicalize` (path nya harus ada), lalu dicocokkan dengan `path` baris `storage_locations` ber kind `data`; baik folder yang kamu pilih di Settings (`<p>`) maupun subfolder nya (`<p>/Stackly`, bentuk yang disimpan dan dicetak `stackly storage locations`) diterima. Lokasi bawaan tidak punya path tersimpan dan tidak bisa disebut di sini (ia hanya dipakai sebagai default). Cocok: `PlanItem.storage` terisi dan `gather_locations` memakai lokasi itu, jadi baris check storage memeriksa foldernya. Tidak ada, atau tidak cocok dengan baris mana pun: blocker `env.override_location_unknown` (hint: tambahkan lokasi itu di Settings, Storage). Untuk service ber member: tidak diterapkan.

`PlanOverride.state`: `applied` (dipakai draft create), `compared` (port untuk service ber member, hanya pembanding), `not_applied_existing` (storage untuk service ber member), `ignored` (alias tidak dikenal, atau file override nya punya finding `error`).

**File override dengan finding `error`** *(ditambah 2026-11-13, K22)*. Begitu file override punya satu finding ber severity `error`, tidak ada override yang diterapkan, juga yang benar: draft create dan nilai pembanding `differences` memakai manifest, `PlanItem.storage` kosong, `local.state` `invalid`, dan `local.sha256` tetap terisi kalau file terbaca. `local.overrides` tetap mendaftar apa yang file minta sejauh bentuk dokumennya terbaca, dengan setiap entri ber state `ignored`; kalau bentuk dokumennya sendiri gagal, daftar itu kosong. `applied`, `compared`, dan `not_applied_existing` hanya muncul saat `local.state` `present`. Aturan yang sama berlaku untuk `EnvLocalInfo.overrides` di `env.get`. File yang hanya punya finding `warning` (`override_unknown_service`) tetap `present`: override alias itu `ignored`, yang lain diterapkan. *(Diselaraskan dengan build (2026-11-14): K22 dibangun seperti tertulis di `environment::plan` dan di pembangun `EnvLocalInfo`; file override dengan satu error memberi `local.state` `invalid`, `sha256` terisi, setiap entri `ignored`, port plan dari manifest, dan blocker `env.local_invalid`. Di CLI entri itu tercetak `<service>.<field>: <value> (ignored)`.)*

### Variabel export

Untuk setiap service snapshot dalam urutan snapshot, lalu setiap endpoint instance nya dalam urutan paket pinned:

| Variabel | Nilai |
| --- | --- |
| `STACKLY_<ALIAS>_<ENDPOINT>_HOST` | host efektif endpoint |
| `STACKLY_<ALIAS>_<ENDPOINT>_PORT` | port efektif endpoint |
| `STACKLY_<ALIAS>_<ENDPOINT>_ADDRESS` | `host:port`, IPv6 dalam kurung siku |

`<ALIAS>` dan `<ENDPOINT>`: huruf dijadikan kapital, setiap karakter selain `A-Z` dan `0-9` menjadi `_`. Nama yang sudah dipakai variabel sebelumnya dilewati dengan `name_collision`.

`ExportSkipReason`: `not_created` (service belum punya instance), `port_not_assigned` (endpoint `auto` yang belum pernah dialokasikan), `unix_endpoint`, `package_unavailable` (paket pinned tidak terbaca), `retained` (instance `retained`), `name_collision`, `redacted` (AC-15).

`ExportVariable.mayChange` benar kalau endpoint itu `auto` dan lifecycle instance bukan `running` (aturan `portMayChange` spec 0021).

Format `text`: `dotenv` menulis `NAME=value` per baris; `shell` menulis `export NAME='value'` per baris. Tanpa komentar, tanpa baris kosong, `\n` di akhir. Nilai hanya berisi angka, huruf, titik, titik dua, dan kurung siku, jadi tidak butuh escaping; `export::collect` menolak nilai dengan karakter lain lewat error internal (kategori `internal`) supaya nilai asing tidak pernah lolos ke shell. Host dicetak apa adanya dari endpoint instance; hari ini setiap paket resmi memakai host loopback, tetapi export tidak menyaring host yang bukan loopback.

### Model data

Tidak ada migrasi: kolom `environments.lock_sha256` sudah ada sejak spec 0005.

**Domain** (`stackly-domain`):

- `Environment` dan `StoredEnvironment` mendapat `lock_sha256: Option<Sha256Digest>`; `Environment::set_manifest(location, sha256, declared, lock_sha256)` (mutator spec 0044) menerima nilainya. `read::environments` membacanya; `WriteOp::InsertEnvironment` dan `UpdateEnvironment` menulisnya.
- Tidak ada `WriteOp` baru untuk create: `WriteOp::InsertInstanceAt { instance, data, logs }` sudah menerima lokasi data dan menolak `MissingReference` serta `WrongLocationKind`.

**Core**:

- `crates/core/src/environment/lock.rs`: tipe raw (serde dan `schemars`), `load`, `emit`, `state`, `resolve_entries` (isi `env.lock`), dan pemetaan *Runtime dari lock*.
- `crates/core/src/environment/local.rs`: tipe raw, `load`, `validate`, `apply`.
- `crates/core/src/environment/export.rs`: `collect` dan `render`.
- `environment::plan::PlanInputs { manifest, lock: Option<LockInput>, overrides: Option<LocalOverrides>, locked: bool }`. `LockInput` membawa hasil `load` (dokumen atau kegagalannya) dan digest byte file; `LocalOverrides` begitu juga.
- `CreateDraft.data_location: Option<StorageLocationId>` (`None` berarti default). `gather_locations` memakai `draft.data_location` kalau terisi, selain itu `settings.data_location`, dan memeriksa foldernya dengan cara yang sama; lokasi log selalu `settings.logs_location`. Karena `Evaluation.locations` membawa pasangan itu, folder root, data, config, config history, dan temp instance dibuat di lokasi data yang dipilih, dan commit `InsertInstanceAt` tidak berubah. `instance.create` dan `instance.preflight` di wire tidak mendapat field ini dan selalu mengirim `None`; golden mereka tidak berubah.

Tidak ada kode macOS; lulus `cargo check` Windows.

### API surface

Semua method lewat handshake dan peer user yang sama (spec 0006). Path memakai aturan `import_path_code`.

| Method | Jenis | Recovery | Input | Output | Error utama |
| --- | --- | --- | --- | --- | --- |
| `env.plan` (spec 0044) | baca | refused | `EnvPlanParams { source, locked }` | `EnvPlan` dengan field baru | seperti spec 0044 |
| `env.lock` | menulis satu file, bukan operasi | refused | `EnvLockParams { source: EnvSource, refresh: LockRefresh }` | `EnvLockResult` | `env.manifest_unreadable`, `env.not_found`, `env.service_not_found`, `env.lock_unwritable`, `catalog.unavailable` |
| `env.export` | baca | refused | `EnvExportParams { source: EnvSource, format: ExportFormat }` | `EnvExport` | `env.manifest_unreadable`, `env.manifest_invalid`, `env.not_found`, `env.not_registered` |
| `env.get` (spec 0044) | baca | refused | tidak berubah | `EnvironmentDetail` dengan `lock` dan `local` | seperti spec 0044 |

`env.lock` mengikuti pola `diagnostics.export_bundle`: daemon menulis file di tempat yang ditentukan klien secara tidak langsung (folder manifest), tanpa operasi dan tanpa audit, karena ia tidak menulis registry. Dua `env.lock` bersamaan untuk manifest yang sama aman: penulisan atomik, dan penulis terakhir menang (byte nya sama kalau `refresh` dan catalog nya sama). File ditulis dengan mode 0600 bawaan `write_file_atomic`; git hanya mencatat bit executable, jadi mode itu tidak ikut ke repo.

Masukan `env.lock`:

- Sumber `registered` yang `manifest_location` nya kosong ditolak `env.manifest_unreadable` dengan `reason` `not_found`.
- `env.lock` tidak melihat pendaftaran: file yang kini mendeklarasikan nama lain, atau nama yang terdaftar di lokasi lain, tetap dikunci. Yang dikunci adalah file itu.
- Manifest tidak valid: balasan sukses dengan `validation.valid` false, tanpa membaca lock dan tanpa menulis.
- Folder yang tidak bisa ditulis: `env.lock_unwritable` dengan `permission_denied` atau `io`; file lama utuh.
- Catalog tidak tersedia: `catalog.unavailable`.

`env.export` dengan sumber `file`: daemon membaca manifest hanya untuk `metadata.name` (file tidak terbaca: `env.manifest_unreadable`; bentuk dokumen gagal: `env.manifest_invalid`; finding lain diabaikan dan catalog tidak dibutuhkan), lalu mencocokkan nama dan lokasinya ke registry seperti langkah 1 *Aksi plan* spec 0044; `new` atau `name_taken` ditolak `env.not_registered`. Dengan sumber `registered`, file manifest tidak dibaca sama sekali, jadi export tetap jalan saat manifest hilang.

**DTO** (camelCase; enum terbuka berakhir `unrecognized`; hitungan memakai `Count`; field baru pada tipe yang sudah ada memakai `#[serde(default)]`):

- `EnvPlanParams.locked: bool` (default false).
- `EnvPlan.lock: PlanLock { file, state: LockState, sha256?, locked, targets: [String], stale: [LockStaleItem { service, reason: LockStaleReason }] }`; `LockState` `absent`, `in_sync`, `stale`, `invalid`, `unreadable`. `sha256` terisi kalau file terbaca dan valid (`in_sync` atau `stale`). `targets` gabungan target semua entri, urut teks. Untuk `absent`, `unreadable`, dan `invalid`, `sha256` kosong dan `targets` serta `stale` array kosong. Untuk manifest yang tidak valid, `lock.state` tetap dihitung sampai `invalid` saja (tanpa cek basi), jadi lock yang terbaca dan valid tampil `in_sync` dengan `stale` kosong (*State lock*), dan `local` dikirim hanya dengan finding bentuk dokumen.
- `EnvPlan.local?: PlanLocal { file, state: LocalState, sha256?, overrides: [PlanOverride { service, field, value, state: OverrideState }], findings: [EnvFinding] }`; `LocalState` `present`, `invalid`, `unreadable`; `OverrideState` `applied`, `compared`, `not_applied_existing`, `ignored`. `field` berupa `endpoints.<nama>.port` atau `storage.dataLocation`; `value` teks (path dengan home sebagai `~`).
- `PlanItem.providerRevision?: PlanProviderRevision { revision, manifestSha256 }` (item `create`: key entri catalog yang dipakai); `PlanItem.storage?: PlanStorage { locationId, path }`; `PlanRuntime.source: RuntimeSource` `lock`, `catalog`; `PlanDifference.field` mendapat nilai `lock`.
- `LockRefresh`: `{type: "none"}`, `{type: "all"}`, atau `{type: "services", content: [alias]}` (1 sampai 32 alias).
- `EnvLockResult { validation: EnvValidation, lockFile, written, changed, sha256?, targets: [String], services: [LockedService { alias, provider, selector, version?, artifactId?, change: LockChange, previousVersion? }], blockers: [PlanBlocker] }`; `LockChange` `added`, `kept`, `updated`, `removed`. `changed` benar kalau byte baru berbeda dari file yang ada; `written` benar hanya kalau file benar benar ditulis. Dengan manifest tidak valid atau `blockers` terisi, `written` dan `changed` false dan `sha256` kosong.
- `EnvExportParams.format: ExportFormat` `dotenv`, `shell`.
- `EnvExport { environment: EnvironmentRef, format, text, variables: [ExportVariable { name, value, service, endpoint, kind: ExportValueKind, mayChange }], skipped: [ExportSkipped { service, endpoint?, reason: ExportSkipReason }] }`; `ExportValueKind` `host`, `port`, `address`.
- `EnvironmentDetail.lock?: EnvLockInfo { file, state: LockState, targets: [String], services: Count }` dan `local?: EnvLocalInfo { file, state?: LocalState, overrides: [PlanOverride], problems: Count }`; `state` tidak dikirim kalau file override tidak ada (`overrides` kosong, `problems` 0), jadi `LocalState` tidak butuh nilai "tidak ada" *(diselaraskan 2026-11-13 dengan kontrak 4.3 yang sudah dibangun)*. *(Diselaraskan dengan build (2026-11-14): K24 cocok dengan kode, `EnvLocalInfo.state` adalah `Option<LocalState>` di `crates/protocol/src/env.rs`, dan app menulis baris Local override "None" untuk `state` yang kosong.)* Di `env.get`, `OverrideState` hanya `applied`, `compared`, `not_applied_existing`, atau `ignored` menurut ada tidaknya member, tanpa evaluasi create.
- `EnvironmentSummary.lockSha256?`.

**`planDigest`**: masukan spec 0044 ditambah `locked`, `lock.sha256`, `local.sha256`, dan per item `runtime.source`, `providerRevision`, dan `storage.locationId`.

**Anggaran frame**: `EnvLockResult` paling besar 32 service kali sekitar 400 byte ditambah `EnvValidation`; `EnvExport` paling besar 32 service kali 8 endpoint kali 3 variabel, sekitar 100 KiB. Keduanya jauh di bawah 1 MiB.

### Kode error di wire

Area `env` (spec 0044). Semua `retryable` false. Pesan lowercase tanpa titik.

| Kode | Kategori | Kapan | Kunci `details` |
| --- | --- | --- | --- |
| `env.lock_missing` | `conflict` | blocker plan: `locked` true tanpa file lock | `file` |
| `env.lock_unreadable` | `invalid_input` | blocker plan: file lock tidak bisa dibaca | `reason` (nilai `env.manifest_unreadable`) |
| `env.lock_invalid` | `invalid_input` | blocker plan dan blocker `env.lock`: loader menolak lock | `code`, `location` |
| `env.lock_stale` | `conflict` | blocker plan (`locked` true): satu alias tidak cocok | `service`, `reason` |
| `env.lock_provider_mismatch` | `conflict` | blocker plan: revision provider berbeda dari lock | `service`, `provider`, `lockedRevision`, `availableRevision` |
| `env.lock_no_artifact` | `unsupported` | blocker plan: lock tidak punya artefak untuk target ini | `service`, `target`, `lockedTargets` |
| `env.lock_artifact_unknown` | `unsupported` | blocker plan: digest yang dikunci tidak ada di catalog, atau versi, target, atau ID nya berbeda | `service`, `artifactId`, `sha256`, `reason` |
| `env.lock_unwritable` | `failed` | `env.lock` tidak bisa menulis file | `reason`: `not_a_file`, `permission_denied`, `io` |
| `env.local_unreadable` | `invalid_input` | blocker plan: file override tidak bisa dibaca | `reason` |
| `env.local_invalid` | `invalid_input` | blocker plan: file override punya finding `error` | `errors` |
| `env.override_location_unknown` | `not_found` | blocker plan: `dataLocation` bukan lokasi data terdaftar | `service`, `path` (dengan `~`) |
| `env.service_not_found` | `not_found` | `refresh` menyebut alias yang tidak ada atau yang `instanceRef` | `service` |
| `env.not_registered` | `not_found` | export dari file yang environment nya belum terdaftar di lokasi itu | `name` |

Hint: `env.lock_missing`, `env.lock_stale`, dan `env.lock_provider_mismatch` menyarankan `stackly env lock`; `env.lock_no_artifact` dan `env.lock_artifact_unknown` menyebut target Mac ini dan menyarankan memperbarui Stackly atau mengunci ulang di Mac ini; `env.override_location_unknown` menunjuk Settings, Storage. *(Diselaraskan dengan build (2026-11-14): hint kedua kode artefak menyebut target dengan namanya, "the lock has no artifact this Mac (macos-aarch64) can use; update Stackly, or lock again on this Mac" (`apps/daemon/src/ipc/env/dto.rs`), dan blocker `env.lock` untuk selector yang tidak bisa di resolve menyebut providernya, "official/valkey has no version matching 8", sama dengan `env.plan` untuk kasus yang sama. Hint dari daemon menyebut perintah CLI (`stackly env lock`) juga saat tampil di banner app, yang aksinya bernama Lock Versions.)*

Kode lokal CLI: `cli.env_lock_blocked` (`conflict`, exit 3, details `blockers`) dan `cli.env_locked_unsupported` (`unsupported`, exit 5; daemon tidak mengenal `locked`).

### CLI

Semua di `apps/cli/src/commands/env/`. `--file` default `./stackly.yaml`; `<environment>` di resolve seperti spec 0044.

| Perintah | Perilaku | `result` `--json` |
| --- | --- | --- |
| `stackly env lock [--file <path> \| <environment>] [--update] [--service <alias>]...` | `env.lock`. Tanpa `--update`: `refresh` `none`; `--update` saja: `all`; `--update` dengan satu atau lebih `--service`: `services`. `--service` tanpa `--update` ditolak clap (exit 2). Satu baris per service ke stderr: `<change> <alias> <provider> <version>` (`updated` menulis `<lama> -> <baru>`). Lalu "Locked <n> services in <file>." atau "<file> is up to date." Blocker: daftar `blocked:` lalu `cli.env_lock_blocked` | `EnvLockResult` |
| `stackly env plan … [--locked]` | mengirim `locked`; dengan `--locked`, balasan tanpa `lock.locked` true ditolak `cli.env_locked_unsupported`. Tabel mendapat kolom `SOURCE` (`lock`, `catalog`, atau `-`). Sesudah tabel, ke stderr: `lock: <file> (<state>[, <targets>])` dan satu baris per alias basi; `local: <file>` dan satu baris `<service>.<field>: <value> (<state>)` per override, lalu finding nya. Untuk manifest yang tidak valid baris `lock:` dan `local:` tidak dicetak; hanya finding manifest *(ditambah 2026-11-13)* | `EnvPlan` |
| `stackly env export [--file <path> \| <environment>] [--format dotenv\|shell]` | `env.export`, default `dotenv`. Stdout: tepat `text`. Stderr: `skipped: <service>[.<endpoint>]: <reason>` per entri. Exit 0 juga saat semua dilewati | `EnvExport` |

**Untuk developer** *(ditambah 2026-11-13)*. Tiga hal, tanpa perubahan wire (protokol tetap 4.3, tanpa codegen):

1. `EnvLocalInfo.state` sudah opsional di kontrak 4.3; tidak ada perubahan kode. Milestone 3 membaca `state` yang kosong sebagai baris "None".
2. Di `environment::plan` (dan pembangun `EnvLocalInfo` di `apps/daemon/src/ipc/env/`), saat file override punya finding `error`: jangan terapkan satu override pun, set `local.state` `invalid`, dan tandai setiap entri `overrides` `ignored`. Hari ini entri yang benar masih dilaporkan `applied` walau `PlanItem.endpoints` sudah memakai manifest (temuan verify Milestone 1). Tambahkan assertion itu ke `plan_local::` dan ke test daemon `plan_and_get_read_the_lock_and_the_overrides_next_to_the_manifest`.
3. `lock.state` untuk manifest tidak valid tidak berubah di daemon. Milestone 2 (CLI) tidak mencetak baris `lock:` dan `local:` saat `validation.valid` false, dan Milestone 3 (sheet) tidak menampilkan keterangan lock atau override untuk manifest seperti itu; keduanya dikunci golden dan snapshot.

### Layar

Semua komponen dari `Components/` dan `State/`; copy bahasa Inggris.

**Seksi Details** (di bawah baris Schema spec 0044):

| Baris | State | Teks |
| --- | --- | --- |
| Lock | `in_sync` | "{file} · {targets dipisah koma}" |
| Lock | `stale` | "{file} · out of date" |
| Lock | `absent` | "Not locked" |
| Lock | `invalid`, `unreadable` | "{file} · can’t be read" |
| Local override | tanpa file atau tanpa override | "None" |
| Local override | ada override | "{service}.{field}: {value}", lalu " and {n} more" kalau lebih dari satu |
| Local override | `invalid`, `unreadable` | "{file} · {n} problems" atau "{file} · can’t be read" |

Kedua baris memakai teks monospace seperti mockup. Selama `lock` dan `local` belum dikirim (`checkManifest` false, atau manifest `missing`, `unreadable`, `invalid`), keduanya menampilkan "—".

**Menu More** (sesudah Reload Manifest): Lock Versions (`env.lock`, `refresh` `none`; feedback "Locked {n} services." atau "The lock is up to date."), Update Lock… (`ConfirmationRequest` "Update the lock?" / "Stackly picks versions again from the catalog and rewrites {file}." / "Update", lalu `refresh` `all`), Copy Environment Variables (`env.export` `dotenv` ke pasteboard biasa; feedback "Copied {n} variables." atau, kalau `variables` kosong, "Nothing to copy yet. Bring the environment up first."). Blocker `env.lock` tampil sebagai daftar di `StateBanner` critical dengan hint kodenya.

**Sheet plan** (tambahan pada sheet spec 0044):

- Baris `create` yang `runtime.source` nya `lock` mendapat akhiran " · locked".
- Di bawah baris: "Local override: port {port}" atau "Local override: data in {path}" untuk override `applied`; "Differs from lock: {versi dikunci}" untuk `differences` `lock`.
- `StateBanner` caution saat `lock.state` `stale` dan plan tidak `locked`: "The lock is out of date for {services}. Those versions come from the catalog."
- Finding `warning` file override tampil sebagai item caution; finding `error` sudah menjadi blocker.
- Untuk manifest yang tidak valid, sheet hanya menampilkan Problems spec 0044: tidak ada akhiran " · locked", banner lock basi, atau baris Local override *(ditambah 2026-11-13)*.
- Override ber state `ignored` tidak pernah tampil sebagai "Local override: …" di bawah baris; untuk file override yang `invalid`, yang tampil adalah blocker `env.local_invalid` dan finding nya.

Sesudah `env.lock` berhasil dan sesudah file di folder manifest berubah (aturan muat ulang `checkManifest` true spec 0044), halaman memanggil ulang `env.get`.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| lock | nama file lock dan override | *Nama file*, dari path manifest |
| lock | `providerRevision`, `providerManifestSha256` | `entry.package.key()` entri catalog di memori |
| lock | `version`, `line`, `artifacts` | `runtime::resolve` atas selector, lalu `entry.artifacts()` ber versi sama dan status `Available` |
| lock | `change`, `previousVersion` | perbandingan dengan lock yang ada (kalau terbaca dan valid) |
| lock | `sha256` | SHA-256 byte hasil `emit` |
| plan | target Mac ini | `HostFacts.target` |
| plan | runtime item ber lock | *Runtime dari lock* |
| plan | `lock.sha256`, `local.sha256` | SHA-256 byte file yang dibaca |
| plan | `providerRevision` item | key entri catalog yang dipakai item itu |
| plan | `storage.locationId` | baris `storage_locations` kind `data` yang `path` nya sama dengan `dataLocation` |
| plan | pembanding `differences` `lock` | `artifact_sha256` baris runtime member |
| up (spec 0046) | `environments.lock_sha256` | `EnvPlan.lock.sha256` |
| get | `lock`, `local` | file di folder `manifest_location`, hanya saat `checkManifest` true |
| export | host dan port | endpoint efektif instance di registry (data yang sama dengan `instance.get_detail`) |
| export | urutan dan nama endpoint | paket pinned instance |
| export | instance service shared tanpa member | instance bernama `instanceRef` (aturan *Status environment* spec 0044) |
| GUI | "{file}" | nama file dari `lock.file` atau `local.file` |

### Key invariants

- Hanya `env.lock` yang menulis file lock. Plan, Up, export, import, dan `env.get` tidak pernah menulis `stackly.lock`, `.stackly.local.yaml`, atau `stackly.yaml`.
- Lock tidak pernah menjadi sumber artefak: setiap artefak yang dipasang berasal dari catalog yang dipercaya dan lolos gate install.
- Entri lock yang cocok dipenuhi persis atau menjadi blocker; tidak ada substitusi di mode mana pun.
- Dengan `locked` true tidak ada resolve dari selector manifest.
- Override lokal tidak pernah mengubah manifest, `manifestSha256`, snapshot, lock, atau instance yang sudah ada.
- Lock, export, dan balasan plan tidak pernah memuat nilai secret; lock dan export juga tidak memuat reference secret, path, atau PID.
- `env.validate` tidak bergantung pada lock atau override.

### Security model

- Peer adalah user yang sama (spec 0006). Daemon menulis hanya ke `<folder manifest>/<stem>.lock`; nama itu diturunkan daemon, bukan dikirim klien. Kalau path itu sudah ada dan bukan file biasa (symlink, folder), `env.lock` menolak dengan `env.lock_unwritable` `not_a_file`.
- Lock dari orang lain tidak bisa membawa binary: ia tidak punya URL, dan digest yang tidak dikenal catalog ditolak. Ia juga tidak bisa membawa trust: provider `pending` atau `revoked` tetap blocker.
- Isi lock dan override tidak pernah ditulis ke log. Pesan finding dan blocker dibangun dari template tetap; path di `details` memakai `home_mask`.
- Export hanya memuat host dan port yang memang sudah terlihat di tab Summary instance; nilainya tetap lewat `Redactor`, dan variabel yang tersentuh `Redactor` tidak dicetak.

### Configuration required

Tidak ada variabel environment atau credential baru.

### Critical test scenarios

- Lock manifest contoh ERP di root kosong: enam entri, golden byte, dua kali jalan menghasilkan byte sama dan `written` false di kali kedua; sentinel secret, path home, dan PID tidak ada di file; verifies **AC-1**, **AC-2**, **AC-3**.
- `env.lock` terhadap test root: pohon file hanya bertambah satu file lock, generation registry sama, fake downloader, supervision, dan secret nol panggilan; provider lokal `pending` dan selector tanpa artefak memberi blocker dan nol file; verifies **AC-2**.
- Catalog fixture naik versi: `refresh` `none` mempertahankan versi lama, `all` menaikkan semua, `services` menaikkan satu; revision provider berubah tanpa perubahan versi memberi `kept`; host ber target yang belum ada di entri menambah artefaknya tanpa mengubah versi; lock ber `apiVersion` asing tidak ditimpa, dan lock rusak hanya diganti dengan `all`; verifies **AC-3**.
- Satu kasus per `LockStaleReason`, dan manifest yang hanya berubah port, settings, komentar, dan urutan tetap `in_sync`; verifies **AC-4**.
- Plan dengan lock cocok saat catalog `stable` sudah pindah versi: item memakai versi yang dikunci, `source` `lock`; service baru tanpa entri memakai catalog dan tercatat `stale`; verifies **AC-5**.
- `locked` true: tanpa lock, lock rusak, dan setiap alasan basi memberi blocker dan tidak ada item ber `source` `catalog`; CLI terhadap fixture balasan tanpa `lock` keluar 5 dengan `cli.env_locked_unsupported`; verifies **AC-6**.
- Catalog fixture dua target, host target ketiga; digest tidak dikenal; versi di catalog berbeda dari lock; revision provider berbeda; artefak `blocked`: semua blocker di kedua mode dan tidak ada item yang memakai artefak lain; verifies **AC-7**.
- Member dengan runtime lain dari lock: `keep` dengan `differences` `lock`, `applicable` tetap true dengan `locked` true; verifies **AC-8**.
- Satu file override per kode *Aturan override*; alias tidak dikenal hanya warning; `env.validate` sama dengan dan tanpa file override; verifies **AC-9**.
- Override port menyelesaikan bentrok dengan instance lain dan override yang membuat bentrok baru; byte manifest dan lock serta `manifestSha256` tidak berubah; member membandingkan terhadap port override; verifies **AC-10**.
- Override storage ke lokasi terdaftar, ditulis sebagai `<p>` dan sebagai `<p>/Stackly` (create dengan `data_location` membuat folder dan `instance_paths` di lokasi itu, log tetap di default), ke path tak dikenal dan ke lokasi yang foldernya hilang (blocker); member tidak tersentuh; verifies **AC-11**.
- `planDigest` berubah saat `locked`, byte lock, byte override, atau revision provider berubah; `lock_sha256` tersimpan lewat `set_manifest`; verifies **AC-12**.
- `env.get` untuk setiap `LockState` dan `LocalState`, dan `checkManifest` false tidak membaca file; verifies **AC-13**.
- File override dengan satu error dan satu override yang benar: blocker `env.local_invalid`, port dari manifest, kedua entri `ignored`; file tanpa override: `local.state` tidak dikirim; manifest tidak valid dengan lock yang valid: `lock.state` `in_sync`, `stale` kosong, dan teks CLI tanpa baris `lock:`; verifies **AC-9**, **AC-12**, **AC-13** *(ditambah 2026-11-13)*.
- Export: member running dengan port `auto`, member berhenti (`mayChange`), endpoint unix, service belum dibuat, service shared tanpa member, tabrakan nama; kedua format golden; verifies **AC-14**.
- Export dengan secret sentinel: tidak ada di balasan, stdout, atau log; variabel yang nilainya dikenal `Redactor` masuk `skipped` `redacted`; verifies **AC-15**.
- CLI: golden manusia dan `--json` untuk lock (baru, up to date, update, blocked), plan `--locked` (applicable dan blocked), export; exit code; verifies **AC-16**, **AC-17**.
- Snapshot GUI terang, gelap, dan sempit: setiap teks baris Lock dan Local override, sheet plan dengan item terkunci, override, dan catatan basi; verifies **AC-18**, **AC-19**.
- Kontrak: fixture terbaca Rust dan Swift, `cargo xtask codegen --check`, klien lama membaca `PlanItem` baru; verifies **AC-20**.
- Root A ke root B dengan home lain, lalu catalog tanpa target B; verifies **AC-21**.

### Jawaban Follow-up spec lain

| Asal | Pertanyaan | Jawaban |
| --- | --- | --- |
| Spec 0011 L613 | export environment lewat `Redactor` dengan test sentinel | AC-15 |
| Spec 0011 L615 | lockfile dan export tidak pernah memuat nilai secret | AC-1 (lock tidak punya field secret), AC-15 (export) |
| Spec 0001 L145 | JSON Schema lockfile di `schemas/` | `schemas/environment/lock.v1alpha1.schema.json` dan `local.v1alpha1.schema.json` hasil codegen (AC-20) |
| Spec 0005 L263 | kolom `environments.lock_sha256` | digest lock yang dipakai Up terakhir, ditulis lewat `set_manifest` (AC-12), dibaca `EnvironmentSummary.lockSha256` (AC-13) |
| Spec 0044, *Titik sambung* | `PlanInputs.lock` dan `overrides`, baris Lock dan Local override, schema lock, `warning`, grup CLI | *Model data*, AC-5 sampai AC-13, AC-16 sampai AC-20 |

## Build plan

Urutan mengikuti pendekatan Journey: jalur "mengunci versi, lalu plan memakai lock itu" dibangun lengkap dulu dengan setiap state gagalnya, lalu jalur override lokal, lalu export, lalu CLI dan GUI. Protokol naik satu minor di langkah 5 (nomor bebas berikutnya, ditetapkan lead saat build).

**Milestone 1: Lock, override, dan export di daemon** (needs spec 0044 milestone 1, langkah 1 sampai 5)

1. Core `environment::lock`: tipe raw, `load`, `emit` dengan golden, `state`, `resolve_entries` dengan aturan *Entri yang dipertahankan*, fixture lock di `crates/core/tests/fixtures/environments/`, JSON Schema lock lewat `cargo xtask codegen`. Memenuhi **AC-1**, **AC-2**, **AC-3**, **AC-4**.
2. Plan dengan lock: `PlanInputs.lock` dan `locked`, *Runtime dari lock*, `PlanRuntime.source`, `providerRevision`, `differences` `lock`, blocker dan `PlanLock`, masukan baru `planDigest`. Memenuhi **AC-5**, **AC-6**, **AC-7**, **AC-8**, **AC-12**.
3. Core `environment::local` dan create: tipe raw, `load`, `validate`, `apply`, JSON Schema override; `CreateDraft.data_location` dan `gather_locations` yang memakainya (commit tetap `InsertInstanceAt`); `PlanLocal`, `PlanItem.storage`. Memenuhi **AC-9**, **AC-10**, **AC-11**.
4. Domain dan storage: `lock_sha256` di `Environment`, `set_manifest`, `read::environments`, dan kedua `WriteOp`; core `environment::export` (`collect`, `render`, `Redactor`). Memenuhi **AC-12**, **AC-14**, **AC-15**.
5. Protokol: `env.lock`, `env.export` (`recovery refused`), semua field dan enum baru, kode `env.*` dengan hint, fixture `protocols/fixtures/env/`, kenaikan minor `ProtocolVersion::CURRENT`, `cargo xtask codegen`, `cargo xtask fuzz seed`. Memenuhi **AC-20**.
6. `stacklyd`: handler `env.lock` dan `env.export`, pembacaan lock dan override di `env.plan` dan `env.get`, pemetaan DTO dan error, test tanpa efek samping (pohon file, generation, fake), test sentinel lock dan export, skenario root A ke root B. Memenuhi **AC-2**, **AC-13**, **AC-14**, **AC-15**, **AC-21**.

**Milestone 2: `stackly env lock`, `plan --locked`, `export`** (after langkah 5 untuk golden; binary test needs langkah 6; bisa paralel dengan milestone 3)

7. CLI: `env lock`, `--locked` dan keluaran baru `env plan`, `env export`, kode `cli.env_lock_blocked` dan `cli.env_locked_unsupported`, golden dan binary test terhadap daemon dengan provider `fake`. Memenuhi **AC-6**, **AC-16**, **AC-17**.

**Milestone 3: Lock dan override di app** (after langkah 5 dan spec 0044 milestone 3; evidence manual needs langkah 6)

8. GUI: baris Lock dan Local override, tiga item menu More dengan konfirmasi dan feedback, tambahan sheet plan, snapshot terang, gelap, dan sempit. Memenuhi **AC-18**, **AC-19**.
9. Evidence dan manual: record evidence fixture untuk AC-1, AC-7, AC-15, dan AC-21; langkah manual (GUI nyata, hanya dijalankan kamu) ditulis di `verify.md` bagian `### UI / manual`: jalankan Lock Versions pada manifest contoh ERP dan lihat baris Lock berubah, tulis `.stackly.local.yaml` dengan satu port lalu buka sheet plan, dan salin variabel environment. Memenuhi **AC-18**, **AC-19**, **AC-21**.

**Sesi paralel yang aman**: sesudah langkah 5 masuk, CLI (`apps/cli/`) dan GUI (`apps/macos/`) tidak beririsan file dengan daemon (`apps/daemon/`). `crates/protocol/`, `protocols/`, `schemas/`, dan hasil codegen hanya disentuh langkah 1, 3, dan 5, oleh satu sesi, dan tidak bersamaan dengan langkah spec 0044 atau 0046 yang menyentuh folder yang sama. Override storage baru berjalan sampai instance sesudah Up spec 0046 meneruskan `PlanItem.storage.locationId` ke create; sebelum itu ia diuji di plan dan di create lewat test.

## Consequences

**Positive**:

- Tim dan CI mendapat binary yang sama, dan penyebab perbedaan selalu terlihat: lock basi, revision provider lain, atau tidak ada artefak untuk platform itu.
- Lock tidak menambah permukaan trust: file dari orang lain tidak bisa menunjuk unduhan.
- Semua aturan lock ada di plan, jadi Up, GUI, dan CLI otomatis konsisten dan `planDigest` mengikat semuanya.
- Port yang bentrok di satu Mac selesai tanpa commit ke `stackly.yaml`.
- Tidak ada migrasi dan tidak ada `OperationKind` baru.

**Negative / tradeoffs**:

- Pembaruan Stackly yang membawa revision provider baru membuat lock lama menjadi blocker sampai seseorang menjalankan `stackly env lock` dan meng commit hasilnya; rekan dengan Stackly lebih lama lalu harus memperbarui app nya.
- Lock tidak menggerakkan instance yang sudah ada. Member dengan versi lain hanya dilaporkan, jadi "terkunci" berarti "yang dibuat sejak sekarang", bukan "yang sedang berjalan".
- Tanpa `--locked`, lock yang basi untuk satu service berarti service itu memakai catalog. Itu terlihat di plan, tetapi tetap bisa terlewat oleh yang tidak membaca plan.
- Override storage hanya bisa menunjuk lokasi data yang sudah terdaftar, dan hari ini lokasi baru hanya bisa didaftarkan dengan menjadikannya default di Settings.
- Export tidak memberi connection string siap pakai; aplikasi menyusun URL nya sendiri dari host dan port dan mengambil password lewat jalurnya sendiri.
- `env.lock` menulis file di repo kamu tanpa operasi dan tanpa audit; jejaknya adalah git.
- `CreateDraft.data_location` menyentuh `gather_locations` di jalur create yang stabil; golden preflight menjaga perilaku default.
- File lock ditulis dengan mode 0600 (bawaan `write_file_atomic`), bukan mode file sebelumnya.
- Klien baru terhadap daemon lama harus memeriksa `lock.locked` sendiri, karena protokol minor tidak menolak field yang tidak dikenal.

**Neutral**:

- Hari ini catalog hanya punya artefak `macos-aarch64`, jadi setiap entri lock punya satu artefak; aturan per target sudah diuji dengan catalog fixture.
- `env lock` tidak memakai jaringan. Kalau kelak ada pembaruan catalog lewat jaringan, lock memakai catalog apa pun yang sedang dimuat.
- Dua JSON Schema baru di `schemas/environment/` untuk editor.

### Update spec lain

Usulan untuk lead (architect spec ini tidak mengubah spec lain):

- **Spec 0044**: `EnvPlanParams.locked`; field baru `EnvPlan`, `PlanItem`, `PlanRuntime`, `EnvironmentDetail`, `EnvironmentSummary`; `PlanDifference.field` `lock`; `set_manifest` dengan `lock_sha256`; masukan baru `planDigest`; kolom `SOURCE` dan baris `lock:` serta `local:` di `stackly env plan`; baris Details dan item menu More.
- **Spec 0046**: `EnvUpParams.locked` diteruskan ke plan; `stackly env up --locked` menjalankan cek `lock.locked` yang sama dengan AC-6 pada plan yang ia tinjau; Up menulis `lock_sha256` dari `EnvPlan.lock.sha256`; Up meneruskan `PlanItem.storage.locationId` ke `CreateDraft.data_location` dan memeriksa `providerRevision` (sudah disepakati dengan `arc-f46-spec`).
- **Spec 0004 dan 0005**: `Environment.lock_sha256` dibaca dan ditulis lewat `InsertEnvironment` dan `UpdateEnvironment`.
- **Spec 0020 dan 0032**: `CreateDraft.data_location` dan `gather_locations`.
- **Root `AGENTS.md`** (lewat `/sync`): `env.lock` menulis satu file tanpa operasi karena tidak menyentuh registry, seperti `diagnostics.export_bundle`.
- **Spec 0008**: `stackly env lock`, `export`, `plan --locked`, dan `cli.env_lock_blocked`.
- **Spec 0011, 0001, 0005**: centang baris Follow-up sesuai *Jawaban Follow-up spec lain*.

## Follow-up

- [ ] Lead: tetapkan nomor minor protokol saat build dan terapkan *Update spec lain* lewat `architect (update)`.
- [x] Spec 0046: teruskan `locked`, cek `lock.locked` di CLI `up --locked`, tulis `lock_sha256`, teruskan `storage.locationId`, dan cek `providerRevision` di precheck create. Diterapkan di spec 0046 *Kontrak `--locked`* butir 2, 6, 7, dan 8 (2026-11-13) dan dibangun fitur 46 (2026-11-14): `EnvUpParams.locked` (`crates/protocol/src/env.rs`), cek `plan.lock.locked` dan `cli.env_locked_unsupported` di `apps/cli/src/commands/env/up.rs`, `lock_sha256` di `crates/core/src/environment/up/accept.rs`, dan `data_location` serta revision provider di `draft_for` (`crates/core/src/environment/plan.rs`).
- [ ] Spec 0032: cara mendaftarkan lokasi data tanpa menjadikannya default, supaya override storage bisa dipakai tanpa mengubah default.
- [ ] `/document`: panduan tim (commit `stackly.yaml` dan `stackly.lock`, masukkan `.stackly.local.yaml` ke `.gitignore`, pakai `--locked` di CI).
- [ ] Sesudah ada permintaan nyata: connection string di export (tersamar atau dengan opt in eksplisit), dan format export berupa bundle definisi untuk dibagikan di luar repo (runner up K1).
- [x] `/sync` setelah build: catat di root `AGENTS.md` bahwa hanya `env.lock` yang menulis lock, bahwa lock bukan sumber artefak, dan perintah `stackly env lock` dan `export`. *Dicentang `/sync` akhir scope 08 (2026-11-14).*

## Rationale

Alasan dan opsi yang ditimbang: lihat [rationale.md](rationale.md).
