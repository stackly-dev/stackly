# 0044. Manifest environment: validate dan plan

**Date**: 2026-11-13
**Status**: In Progress

## Summary

Spec ini menetapkan `stackly.yaml`, file yang mendeskripsikan service sebuah proyek (provider, versi, port, settings, reference secret, dependency, dan apakah instance nya milik proyek atau dipinjam). Stackly bisa memeriksa file itu (validate) dan menunjukkan apa yang akan dilakukan Up (plan: pasang runtime, buat instance, nyalakan, atau biarkan) tanpa mengunduh, menjalankan proses, mengubah trust, atau menulis file data. Environment dikenali lewat namanya, yang unik di Mac kamu, dan didaftarkan lewat import yang hanya menulis registry. Halaman Environments, daftar di sidebar, dan `stackly env validate` serta `stackly env plan` memakai method daemon yang sama, jadi hasilnya selalu setara. Lockfile, Up, Down, dan lease dikerjakan spec 0045, 0046, dan 0047 lewat titik sambung yang disebut di sini.

## Requirements

**User stories**:

- Sebagai developer dalam tim, saya ingin satu file `stackly.yaml` yang bisa saya commit, supaya rekan saya mendapat service yang sama tanpa ikut menerima data, path pribadi, atau password saya.
- Sebagai pengguna yang menerima `stackly.yaml` dari orang lain, saya ingin melihat dulu apa yang akan dipasang, dibuat, dan dinyalakan, supaya tidak ada yang berjalan sebelum saya setuju.
- Sebagai penulis manifest, saya ingin kesalahan (cycle, reference yang tidak ada, password yang tertulis langsung, port fixed yang bentrok) ditunjuk di baris yang tepat sebelum ada perubahan apa pun.
- Sebagai pengguna GUI, saya ingin halaman Environments yang menampilkan status tiap environment dan tiap service nya, dan dari sana membuka instance, log, dan aplikasi klien.
- Sebagai penulis script, saya ingin `stackly env validate` dan `stackly env plan` dengan JSON stabil dan exit code yang bisa diandalkan.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-020):

- **AC-1** (schema, REQ-020): `stackly-core::environment::manifest` membaca `stackly.yaml` dengan `apiVersion: stackly/v1alpha1` dan `kind: Environment` sesuai *Schema `stackly.yaml`*: `metadata.name`, lalu 1 sampai 32 `services` yang masing masing membawa provider, selector runtime (`version`), ownership, `instanceRef`, `instanceName`, endpoint, settings, `secretRefs`, `storage`, dan `dependsOn`. File dibaca lewat port `Filesystem::read_file_limited` (paling besar 256 KiB, bukan symlink, file biasa). Field yang tidak dikenal, key ganda, anchor atau alias YAML, dan `apiVersion` atau `kind` lain ditolak dengan finding yang menunjuk baris dan kolom. Manifest contoh ERP `crates/core/tests/fixtures/environments/erp-development.yaml` (contoh PRD bagian 13.1 dengan nama endpoint, secret field, setting, dan line yang benar benar dipakai keenam paket resmi; lihat *Manifest contoh ERP*) lolos tanpa finding terhadap catalog bawaan.
- **AC-2** (aturan validasi, VAL-013): `environment::validate` menerapkan setiap baris tabel *Aturan validasi* dan mengembalikan semua finding, bukan hanya yang pertama: dependency cycle (pesannya menyebut jalurnya, misalnya `api -> db -> api`), `dependsOn` ke alias yang tidak ada atau ke diri sendiri, provider yang tidak ada di catalog, endpoint atau setting yang tidak dikenal paket, dan dua endpoint fixed di manifest yang memakai alamat dan port yang sama. Manifest dengan finding ber severity `error` tidak pernah bisa di plan sebagai `applicable`, diimpor, atau (lewat spec 0046) dijalankan.
- **AC-3** (secret literal, VAL-013, spec 0011): nilai secret tidak punya tempat di manifest. Nilai `secretRefs.<field>` yang bukan reference `local:` sesuai grammar spec 0011 ditolak dengan `secret_literal`; segmen pertama `instance` atau `stackly` ditolak dengan `secret_reference_reserved`; secret field paket yang ditulis di `settings` ditolak dengan `secret_in_settings`. Pesan setiap finding dibangun dari template tetap per kode ditambah nama field, tidak pernah dari teks error parser YAML atau `SettingsSchema`, jadi pesan, `location`, log daemon, audit, registry, dan balasan tidak pernah memuat nilai dari file. Test menaruh nilai sentinel di ketiga tempat itu dan di posisi bertipe salah (angka diganti string, map diganti string), lalu mencarinya di balasan, log, registry, dan test root.
- **AC-4** (`env.validate`): `env.validate { path }` membaca file, menjalankan AC-1 dan AC-2 terhadap catalog di memori, dan membalas `EnvValidation` (`valid`, nama, `manifestSha256`, `services` yang dideklarasikan, paling banyak 100 `findings`, `findingCount`, `omitted`). Manifest yang tidak valid tetap balasan sukses dengan `valid` false. Method ini tidak membaca atau menulis registry, tidak membuat operasi atau baris audit, dan tidak menyentuh Keychain. File yang tidak bisa dibaca menghasilkan `env.manifest_unreadable` dengan `reason`.
- **AC-5** (isi plan, PRD AC-020, REQ-020): `env.plan { source }` membalas `EnvPlan` berisi satu `PlanItem` per service dalam urutan manifest, dengan `action` `create`, `start`, atau `keep` menurut tabel *Aksi plan*; untuk `create`: nama instance, provider dan trust nya, runtime hasil `runtime::resolve` beserta `install` (salah hanya kalau ada baris runtime `managed` ber `artifact_sha256` artefak itu dengan state `installed`), endpoint, dan status tiap reference secret; `order` (urutan start menurut dependency, seri diputus urutan manifest); `blockers`; `applicable`; dan `planDigest`. Setiap item juga membawa `steps` (`install_runtime`, `create_instance`, `start_instance`) supaya install, create, dan start terbaca sebagai langkah terpisah. Untuk manifest baru di root kosong, plan manifest contoh ERP menampilkan enam install, enam create, enam start, dan nol keep.
- **AC-6** (plan tanpa efek samping, PRD AC-020, AC-080 bagian plan, REQ-020): `env.validate` dan `env.plan` tidak mengunduh, tidak memanggil `ProcessSupervision`, tidak memberi atau mengubah approval, tidak menulis registry (termasuk operasi, audit, dan pin provider), tidak menulis Keychain, dan tidak membuat, mengubah, atau menghapus satu file atau folder pun di data root, cache root, log root, maupun folder manifest. Satu satunya sentuhan ke sistem adalah trial bind port yang sudah dipakai `instance.preflight` (soket dibuka lalu langsung ditutup). Test membandingkan pohon file test root (path, ukuran, mtime) dan generation registry sebelum dan sesudah, dan memeriksa nol panggilan pada fake `Downloader`, `ProcessSupervision`, dan `SecretStore::write`.
- **AC-7** (blocker sebelum mutasi, VAL-013): plan menandai `applicable` false dan mengisi `blockers` (satu `ErrorBody` per masalah, dengan alias service nya) saat: port fixed sebuah service `create` bentrok dengan port instance terdaftar mana pun, termasuk member environment ini (`supervisor.endpoint_conflict`), atau sedang dipakai proses lain (`supervisor.endpoint_in_use`); provider nya `pending` atau `revoked` (`provider.approval_required`, `provider.revoked`); selector tidak bisa di resolve atau artefaknya tidak installable (`runtime.selector_unresolved`, `runtime.selector_ambiguous`, `catalog.not_installable`); nama instance sudah dipakai instance lain (`instance.name_taken`); provider tanpa `multi_instance` sudah punya instance (`instance.capability_unsupported`); atau nama environment terdaftar di lokasi lain (`env.name_taken`). Kode nya sama dengan yang dipakai create dan resolve, jadi GUI dan CLI menampilkan pesan dan hint yang sama.
- **AC-8** (`instanceRef`, VAL-014): service dengan `instanceRef` tidak boleh membawa `version`, `instanceName`, `endpoints`, `settings`, `secretRefs`, atau `storage` (finding `instance_ref_conflict`), jadi environment peminjam secara struktur tidak bisa mengubah instance itu. Saat plan, instance bernama itu harus ada di registry user ini (`env.instance_ref_not_found`) dan provider nya harus sama dengan `provider` service (`env.instance_ref_mismatch`). Item nya `start` atau `keep` menurut tabel *Lifecycle ke aksi*, tidak pernah `create`.
- **AC-9** (member yang sudah ada): untuk environment terdaftar, service yang alias nya sudah punya member diberi `keep` atau `start` menurut tabel *Lifecycle ke aksi*, tanpa menjalankan evaluasi create. Kalau manifest berbeda dari instance nya menurut *Aturan `differences`*, item membawa `differences` (`field`, `manifest`, `instance`); perbedaan tidak pernah menjadi blocker dan Stackly tidak pernah mengubah instance supaya cocok. Member yang alias nya tidak lagi ada di manifest masuk `EnvPlan.detached` (alias, instance, ownership) dan tidak disentuh.
- **AC-10** (reference secret di plan): setiap `secretRefs` sebuah item `create` tampil dengan `status` `present`, `missing` (terdaftar tanpa nilai), `unregistered` (tidak ada di registry), atau `unknown` (store tidak bisa ditanya), dibaca tanpa dialog Keychain. Status selain `present` menaikkan `EnvPlan.needsSecrets` dan tidak mengubah `applicable`. Reference `present` juga selalu ditampilkan GUI dan CLI ("Uses secret {reference}"), supaya manifest dari orang lain tidak bisa memakai secret yang sudah kamu simpan tanpa kamu lihat. Secret field paket yang tidak ditulis di `secretRefs` tampil dengan `status` `generated` (dibangkitkan lokal saat create, aturan spec 0011 AC-6). Plan tidak pernah mengusulkan nilai kosong.
- **AC-11** (identitas dan keunikan): identitas environment adalah baris `environments` (`id` UUID v7); `name` unik lewat unique index dan diambil dari `metadata.name`. Manifest dicocokkan ke environment terdaftar lewat nama. Kalau nama itu terdaftar dengan `manifest_location` lain, plan membawa blocker `env.name_taken` dan import ditolak dengan kode yang sama, kecuali import dikirim dengan `relinkEnvironmentId` berisi id environment itu, yang memindahkan `manifest_location` ke file baru tanpa mengubah `id`, member, atau lease. Relink ditolak `env.relink_mismatch` kalau `metadata.name` file berbeda dari nama environment itu. `manifest_location` unik di antara environment: import yang lokasinya sudah dipegang environment lain (file itu kini mendeklarasikan nama lain) mengosongkan lokasi environment lama dalam commit yang sama, sehingga environment lama tampil `missing`. Lokasi disimpan dan dibandingkan sebagai folder induk yang sudah di canonicalize (lewat port `Filesystem`, tanpa mengikuti komponen terakhir) ditambah nama file, supaya `/tmp` dan `/private/tmp` atau beda huruf besar di APFS tidak terbaca sebagai dua lokasi.
- **AC-12** (import): `env.import { path, expectedManifestSha256, relinkEnvironmentId? }` menjalankan operasi `import_environment` yang membaca dan memvalidasi ulang file, menolak `env.manifest_changed` kalau digest nya berbeda dan `env.manifest_invalid` kalau ada finding `error`, lalu lewat `commit_guarded` menulis atau memperbarui baris `environments` (nama, lokasi, `manifest_sha256`, snapshot `services`) tanpa member. Guard menolak nama yang terdaftar di lokasi lain (kecuali relink ke id itu) dan environment ke 65 (`env.limit_reached`). Import tidak memasang runtime, tidak membuat instance, tidak mengambil lease, dan tidak mengubah trust. Permintaan ulang dengan idempotency key yang sama tidak memulai operasi kedua, dan import file yang digest dan lokasinya sama dengan yang tersimpan tidak mengubah baris `environments` (revision tetap); operasi dan baris auditnya tetap tercatat.
- **AC-13** (remove): `env.remove { id, expectedRevision }` menjalankan operasi `remove_environment` yang lewat `commit_guarded` menolak `env.lease_active` selama environment itu memegang lease `active`, lalu menghapus baris demand `released` miliknya, baris pemakaian secret miliknya, dan baris `environments` (member ikut terhapus). Instance, data, runtime, nilai Keychain, dan file manifest tidak disentuh; instance yang tadinya `dedicated` menjadi `manual`.
- **AC-14** (list dan get): `env.list` membalas paling banyak 64 `EnvironmentSummary` (nama, lokasi manifest, `status`, `readyCount`, `serviceCount`) hanya dari registry, tanpa membaca file pengguna. `env.get { id, checkManifest }` membalas `EnvironmentDetail`: summary, satu `EnvironmentService` per service snapshot (alias, selector atau `instanceRef`, ownership, instance beserta versi dan statusnya, atau `not_created`), dan member yang tidak lagi dideklarasikan. Hanya dengan `checkManifest` true daemon membaca file dan mengisi `manifestState` (`in_sync`, `changed`, `missing`, `unreadable`, `invalid`, `name_changed`) serta `problems`, menurut *State manifest*; dengan false keduanya tidak dikirim dan file tidak disentuh. `status` mengikuti *Status environment*.
- **AC-15** (ownership instance, spec 0010 dan 0022): `InstanceSummary.ownership` bernilai `dedicated` untuk member `dedicated`, `shared` untuk instance yang hanya dipinjam, dan `manual` selain itu; field baru `InstanceSummary.environments` mendaftar environment nya (`environmentId`, `name`, `alias`, `ownership`), yang `dedicated` lebih dulu lalu menurut nama. Nilainya sama di `instance.list`, `instance.get`, event `instance.*`, dan summary yang dibangun doctor, karena semuanya membaca satu cache keanggotaan di memori daemon (pola cache `autostart`) yang dimuat saat start dan sesudah setiap commit yang mengubah `environments` atau `environment_instances`. `stackly instance list` mendapat kolom `ENVIRONMENT`.
- **AC-16** (CLI validate dan plan, REQ-028): `stackly env validate [--file <path>]` dan `stackly env plan [--file <path> | <environment>]` memanggil `env.validate` dan `env.plan`, dengan `--file` default `./stackly.yaml`. Validate keluar 0 kalau valid dan 2 dengan `cli.env_invalid` kalau tidak; plan keluar 0 kalau `applicable`, 2 dengan `cli.env_invalid` kalau manifest tidak valid, dan 3 dengan `cli.env_plan_blocked` kalau ada blocker. Di ketiga kasus `result` `--json` tetap terisi (`EnvValidation` atau `EnvPlan`), stdout bersih dari progress, dan finding serta blocker nya sama persis dengan yang ditampilkan GUI untuk file yang sama.
- **AC-17** (CLI import, list, remove, REQ-028): `stackly env import [--file <path>] [--relink]`, `stackly env list`, dan `stackly env remove <environment> [--yes]` setara dengan aksi GUI. `<environment>` adalah nama atau UUID utuh; nama yang tidak dikenal keluar 10 dengan `cli.environment_not_found`. Remove meminta konfirmasi dengan aturan `ConfirmArgs` spec 0008.
- **AC-18** (halaman Environments, PAGE-10): `EnvironmentsPage` menggantikan `UnbuiltPage(.environments)`. Sidebar menampilkan satu baris anak per environment di bawah Environments dengan nama dan hitungan `ready/total`. Halaman daftar menampilkan setiap environment dengan status dan "{n} of {m} ready · {file}". Halaman detail menampilkan nama, file, `StatusLabel` environment, banner partially ready, dan tabel Services dengan kolom Service, Selector, Instance, Version, Ownership, dan Status seperti *Layar*.
- **AC-19** (menu aksi per service): menu baris service memakai `OpenInMenu` spec 0070 untuk instance nya, lalu Copy Connection URL, Show Instance, Show Logs, dan Start, Stop, atau Retry dari aksi instance yang sudah ada. Untuk service yang belum dibuat, satu satunya item adalah Show Plan…. Klik ganda baris membuka instance.
- **AC-20** (sheet plan): sheet "Plan for {name}" menampilkan satu baris per service dengan badge kata kerja Create, Start, atau Keep beserta teksnya (*Layar*), baris Install di bawah Create yang butuh runtime baru, daftar blocker, daftar reference yang butuh secret, dan perbedaan. Sheet hanya membaca: tidak ada tombol yang menjalankan sesuatu sampai spec 0046 menambah Bring Up.
- **AC-21** (import, validate, edit, remove di app): Import… membuka pemilih file, menjalankan `env.plan` untuk file itu, menampilkan sheet plan beserta trust tiap provider, lalu Import mendaftarkannya. Validate menjalankan `env.validate` untuk manifest environment itu dan menampilkan hasil sukses sebagai feedback atau daftar Problems. Edit berarti Open Manifest (editor bawaan macOS untuk file itu) dan Reveal in Finder; Stackly tidak punya editor YAML. Saat `manifestState` `changed`, banner menawarkan Review Plan dan Reload. Remove Environment… meminta konfirmasi yang menyatakan instance dan data tetap ada.
- **AC-22** (state dan interaksi, NFR-012): setiap layar di AC-18 sampai AC-21 punya state loading, kosong, gagal, dan offline dari `ViewState`; aksi yang mengubah data memakai `.mutationGated()` dan menampilkan alasan saat nonaktif; status environment dan service selalu berupa teks dan simbol, tidak hanya warna; semua alur bisa dijalankan dengan pointer dan kontrol native.
- **AC-23** (permukaan lain): Overview menampilkan seksi Environments di kolom kanan di atas Operations saat ada environment; tabel Instances mendapat kolom Environment; inspector instance mendapat baris Environment dan teks Ownership "Dedicated", "Shared", atau "Manual"; langkah Welcome onboarding mendapat baris "Environments as files".
- **AC-24** (kontrak): `ProtocolVersion::CURRENT` naik satu minor untuk seluruh kontrak fitur ini; modul `stackly-protocol::env`, enam method, kode area `env`, dan fixture `protocols/fixtures/env/` masuk dalam satu commit dengan `cargo xtask codegen`; `cargo xtask codegen` menulis `schemas/environment/environment.v1alpha1.schema.json` dari tipe raw di `stackly-core`; skenario fixture `environment-partially-ready` spec 0068 ada; `cargo xtask check-deps` dan `cargo check --target aarch64-pc-windows-msvc` untuk domain, platform, protocol, dan core tetap lulus.
- **AC-25** (JOURNEY-04 bagian fitur ini): dari root kosong, mengimpor manifest contoh ERP lewat GUI maupun CLI menampilkan trust keenam provider, lalu plan dengan enam install, enam create, dan enam start, tanpa membuat proses, runtime, atau file data; langkah lock, up, dan down milik spec 0045 sampai 0047.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan K1 sampai K26 di [rationale.md](rationale.md): manifest YAML ketat ber `apiVersion` `stackly/v1alpha1`, identitas environment lewat nama yang unik dan terikat ke lokasi file, import sebagai operasi yang hanya menulis registry, dan plan yang memakai ulang `gather` dan `evaluate` milik create tanpa pernah memulai operasi.

Validate adalah pemeriksaan file terhadap catalog; plan adalah validate ditambah keadaan Mac ini; keduanya method baca yang dipakai sama oleh GUI dan CLI.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

## Feature design

### Trace PRD

| ID PRD | Dicakup oleh |
| --- | --- |
| REQ-020 | AC-1, AC-2, AC-5, AC-6 |
| REQ-028 | AC-16, AC-17 |
| VAL-013 | AC-2, AC-3, AC-7 |
| VAL-014 | AC-8 |
| PAGE-10 | AC-18 sampai AC-22 (bagian import, edit, validate, plan; up, down, lease, dan diagnosis up milik spec 0046 dan 0047) |
| AC-020 | AC-5, AC-6 |
| AC-080 (bagian plan) | AC-6 (bagian export dan lockfile milik spec 0045) |
| NFR-012 | AC-22 |
| JOURNEY-04 | AC-25 |

### Batas dengan spec lain

| Hal | Pemilik |
| --- | --- |
| `stackly.lock`, `.stackly.local.yaml`, `env lock`, `env export`, `--locked` | spec 0045 |
| Operasi `environment_up`, pembuatan member, lease, eksekusi dependency graph, timeout default, status needs secret, `env up`, `env status`, tombol Bring Up | spec 0046 |
| Operasi `environment_down`, pelepasan lease, override stop dan restart pada instance yang dipinjam, konfirmasi Down, baris Lease | spec 0047 |
| Evaluasi create, port, settings, secret per instance | spec 0020, dipakai ulang |
| Resolve runtime dan install | spec 0018, dipakai ulang (plan hanya memanggil resolve) |
| Trust provider lokal | spec 0037, dipakai ulang (plan hanya membaca verdict) |
| `OpenInMenu` | spec 0070 |

### Istilah

- **Manifest**: file `stackly.yaml`. Nama file bebas; `stackly.yaml` adalah default CLI.
- **Service**: satu entri di `services`, dikenal lewat alias nya (`MemberAlias`).
- **Member**: baris `environment_instances`, yaitu service yang sudah terikat ke sebuah instance. Member hanya dibuat oleh Up (spec 0046).
- **Snapshot**: daftar service yang dideklarasikan manifest saat terakhir diterima (import, atau Up di spec 0046), disimpan di registry supaya daftar dan status tidak perlu membaca file pengguna.

### Schema `stackly.yaml`

```yaml
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: erp-development          # EnvironmentName (pola InstanceName, 1 sampai 63)

services:
  postgres:                      # MemberAlias
    provider: official/postgresql
    version: "18"                # selector runtime, selalu string
    ownership: dedicated         # default dedicated; shared hanya dengan instanceRef
    instanceName: erp-pg         # opsional; default "<metadata.name>-<alias>"
    endpoints:
      client: {host: "127.0.0.1", port: 5432, transport: tcp}
    settings:
      maxConnections: 200
    secretRefs:
      superuserPassword: "local:erp/postgres/admin"
    storage: {mode: managed, persistent: true}
    dependsOn:
      - {service: cache, condition: ready, timeoutSeconds: 60}

  cache:
    provider: official/valkey
    instanceRef: shared-valkey   # meminjam instance yang ada; ownership menjadi shared
```

| Field | Wajib | Aturan |
| --- | --- | --- |
| `apiVersion` | ya | tepat `stackly/v1alpha1`; lainnya `unsupported_api_version` dengan daftar yang didukung |
| `kind` | ya | tepat `Environment` |
| `metadata.name` | ya | `EnvironmentName` |
| `services` | ya | map 1 sampai 32 entri; key nya `MemberAlias`; urutan dokumen dipertahankan |
| `provider` | ya | `ProviderName` (`<namespace>/<name>`); harus ada di catalog (bawaan atau lokal) |
| `version` | tidak, dedicated saja | selector spec 0018 sebagai string YAML: `stable`, line (`"18"`), versi exact (`"18.6"`), atau `sha256:<64hex>`; default `stable`. Angka YAML tanpa kutip ditolak (`invalid_value`, "write the version in quotes") |
| `ownership` | tidak | `dedicated` atau `shared`. Default `shared` kalau `instanceRef` ada, selain itu `dedicated`. `shared` tanpa `instanceRef`, atau `dedicated` dengan `instanceRef`, ditolak (`ownership_conflict`) |
| `instanceRef` | untuk shared | `InstanceName` sebuah instance yang sudah ada. Melarang `version`, `instanceName`, `endpoints`, `settings`, `secretRefs`, `storage` (`instance_ref_conflict`) |
| `instanceName` | tidak, dedicated saja | `InstanceName`. Default `<metadata.name>-<alias>`; kalau default itu lebih dari 63 karakter, finding `instance_name_invalid` meminta `instanceName` ditulis. Dua service dengan nama efektif sama ditolak (`instance_name_duplicate`) |
| `endpoints.<nama>` | tidak | `<nama>` harus endpoint IP milik paket (`unknown_endpoint`; endpoint `unix` `endpoint_not_ip`). `port`: bilangan 1 sampai 65535 (fixed) atau string `auto`; endpoint yang tidak ditulis berarti `auto` (aturan spec 0020 AC-4). `host` dan `transport` opsional dan, kalau ditulis, harus sama dengan `defaultHost` dan transport paket (`endpoint_host_unsupported`, `endpoint_transport_mismatch`) |
| `settings` | tidak | map; divalidasi `SettingsSchema::validate` paket (`unknown_setting`, `settings_value`) |
| `secretRefs.<field>` | tidak | `<field>` harus secret field paket (`unknown_secret`); nilainya reference `local:` (AC-3). Field yang tidak ditulis dibangkitkan lokal saat create |
| `storage` | tidak | hanya `mode: managed` dan `persistent: true` yang diterima di v1alpha1 (nilai lain `unsupported_value`); lokasi lain adalah override lokal spec 0045 |
| `dependsOn[]` | tidak | paling banyak 31 entri; `service` alias lain di manifest; `condition` hanya `ready`; `timeoutSeconds` opsional 1 sampai 3600 (tanpa nilai berarti default Up, spec 0046) |

Manifest tidak punya field untuk path data, autostart, user OS, nilai secret, atau trust. Instance dedicated dibuat dengan `startup` `manual`.

### Manifest contoh ERP

Contoh PRD bagian 13.1 ditulis sebelum paket resmi ada, jadi beberapa namanya tidak cocok dengan paket yang dibangun scope 07. Fixture `erp-development.yaml` memakai nama paket yang sebenarnya, dan dialah "manifest contoh ERP" di setiap AC dan test:

| Service | Provider | `version` | Endpoint | `secretRefs` | `settings` |
| --- | --- | --- | --- | --- | --- |
| `postgres` | `official/postgresql` | `"18"` | `client` 5432 | `superuserPassword` | `maxConnections` |
| `mysql` | `official/mysql` | `stable` | `client` 3306 | `rootPassword` | |
| `nats` | `official/nats` | `"2.15"` | `client` 4222, `monitoring` 8222 | `clientPassword` | `jetstream: true` |
| `clickhouse` | `official/clickhouse` | `stable` | `http` 8123, `native` 9000 | `defaultPassword` | |
| `valkey` | `official/valkey` | `stable` | `client` 6379 | `password` | `persistence` |
| `traefik` | `official/traefik` | `"3.7"` | `web` 8000, `ping` 8082 | | `routes` (satu route ke `http://127.0.0.1:3000`) |

Tidak ada `dependsOn` di antara keenamnya (PRD: jangan mengarang dependency). Graph dependency diuji dengan fixture lain yang memakai provider `fake`. Developer mencocokkan nilai `settings` dengan schema paket saat menulis fixture; kalau sebuah nama di tabel ini ternyata berbeda dari paket di `main`, paket yang menang dan tabel ini diperbarui lewat lead.

### Aturan validasi

Semua aturan mengembalikan `Finding { severity, code, location, service?, message }`. `location` adalah `line:column` untuk masalah sintaks dan path field (`services.postgres.endpoints.database.port`) selain itu. Di v1alpha1 setiap kode di bawah ber severity `error`; `warning` disediakan untuk spec 0045.

| Kode | Kapan |
| --- | --- |
| `yaml_syntax`, `unknown_field`, `missing_field`, `invalid_value`, `duplicate_key`, `yaml_alias` | bentuk dokumen |
| `unsupported_api_version`, `unsupported_kind` | identitas schema |
| `invalid_name`, `too_many_services`, `too_many_dependencies` | nama dan batas |
| `unknown_provider`, `provider_unreadable` | provider tidak ada di catalog, atau paketnya tidak bisa dimuat |
| `ownership_conflict`, `instance_ref_conflict`, `duplicate_instance_ref` | ownership; dua service menunjuk `instanceRef` yang sama |
| `instance_name_invalid`, `instance_name_duplicate` | nama instance dedicated |
| `unknown_endpoint`, `endpoint_not_ip`, `endpoint_host_unsupported`, `endpoint_transport_mismatch`, `endpoint_conflict` | endpoint; `endpoint_conflict` untuk dua endpoint fixed di manifest dengan host dan port sama, menunjuk keduanya |
| `unknown_setting`, `settings_value`, `unsupported_value` | settings dan storage |
| `unknown_secret`, `secret_literal`, `secret_reference_reserved`, `secret_in_settings` | secret (AC-3) |
| `unknown_dependency`, `self_dependency`, `duplicate_dependency`, `dependency_cycle` | dependency |

Urutan: bentuk dokumen dulu (kalau gagal, berhenti di situ), lalu aturan per service terhadap paket provider dari catalog saat ini (revision entri catalog; pin revision milik spec 0045), lalu aturan lintas service. Validasi boleh membaca paket provider yang belum disetujui: membaca bukan menjalankan (spec 0037).

### Aksi plan

Plan = validate, lalu untuk manifest yang valid:

1. Cocokkan nama ke registry: `new`, `registered` (lokasi sama), atau `name_taken` (lokasi lain; blocker). Untuk sumber `registered`, file yang mendeklarasikan nama lain menghasilkan blocker `env.manifest_renamed` dan plan berhenti di situ dengan `items` kosong.
2. Per service, dalam urutan manifest:

| Keadaan | `action` | `steps` | Yang dihitung |
| --- | --- | --- | --- |
| alias sudah punya member | menurut *Lifecycle ke aksi* | `start_instance` atau kosong | `differences` |
| `instanceRef` ada, belum jadi member | menurut *Lifecycle ke aksi* | `start_instance` atau kosong | provider cocok |
| dedicated tanpa member | `create` | `install_runtime` (kalau `runtime.install`), `create_instance`, `start_instance` | trust (`require_entry_trust`), `runtime::resolve`, baris runtime terpasang, lalu `gather` dan `evaluate` create |

3. `order` lewat urutan topologis `dependsOn` dengan pemutus seri urutan manifest; `waitsFor` per item.
4. `applicable` benar kalau manifest valid dan `blockers` kosong.
5. `planDigest`: SHA-256 dari JSON dengan key terurut (encoding yang sama dengan `crates/protocol/src/fingerprint.rs`) atas `manifestSha256`, `registration`, dan per item `alias`, `action`, `steps`, `instance`, `runtime.artifactSha256`, `runtime.install`, dan `endpoints`, tanpa teks check dan tanpa status secret. Hanya ada saat `applicable`.

**Lifecycle ke aksi** (untuk instance yang sudah ada, member maupun `instanceRef`):

| Lifecycle instance | `action` | `NotReadyReason` di `env.get` |
| --- | --- | --- |
| `running` | `keep` | `not_ready` kalau readiness bukan `ready` |
| `starting`, `initializing`, `recovering` | `keep` | `starting` |
| `stopped`, `uninitialized` | `start` | `stopped` |
| `failed` | `start` | `failed` |
| `stopping` | `start` | `stopping` |
| `unknown`, `retained` | blocker `env.instance_blocked` dengan `reason` lifecycle nya; item tetap ada dengan `action` `keep` | `unavailable` |

Instance yang `InstanceSummary.blocker` nya terisi (misalnya drift spec 0029 atau data tidak kompatibel spec 0025) dan aksinya `start` juga menjadi blocker `env.instance_blocked`, dengan `reason` kode blocker itu, karena start nya pasti ditolak.

**Aturan `differences`** (hanya untuk member; `field` salah satu dari nilai di bawah, `manifest` dan `instance` teks tampilan):

| `field` | Dihitung berbeda kalau |
| --- | --- |
| `provider` | nama provider pin instance berbeda dari `provider` service |
| `runtime` | selector berupa line dan line `runtimeVersion` instance berbeda; atau selector versi exact dan `runtimeVersion` berbeda. Selector `stable` dan `sha256:` tidak dibandingkan |
| `endpoint.<nama>.port` | manifest menulis port fixed dan port efektif endpoint instance berbeda. Manifest `auto` tidak pernah berbeda |
| `ownership` | ownership member berbeda dari ownership service (misalnya member `dedicated` tetapi service kini `instanceRef`) |
| `instanceRef` | service shared menunjuk nama yang berbeda dari nama instance member |

Settings dan `secretRefs` tidak dibandingkan di v1alpha1.

**Evaluasi create di dalam plan.** Draft dibuat dari service: `name` nama efektif, `provider` entri catalog, `endpoints` dari manifest, `settings` dari manifest, setiap `secretRefs` sebagai `SecretSourceKind::Bind(reference)` dan secret field lain `Generate`, `start` true, `startup` manual. `CreateDraft` tidak berubah. Untuk runtime yang belum terpasang, `gather` mendapat varian `gather_planned(deps, draft, PlannedRuntime)` yang mengisi `CreateFacts.planned_runtime` (versi, line, dan software dari artefak catalog) dan `runtime_row` memberi status ok dengan detail "will be installed" saat field itu terisi, tanpa cek program di folder runtime; `draft.runtime_id` saat itu berisi ID placeholder (pola `placeholder_instance_id`) yang tidak pernah sampai ke jurnal, karena plan tidak memulai operasi. `instance.preflight` dan `instance.create` tidak pernah mengisi `planned_runtime`, dan golden mereka tidak berubah. Cek disk tetap aturan create per instance; plan tidak menjumlah ukuran unduhan.

Baris check ber status `error` menjadi blocker dengan `ErrorBody` baris itu, dengan satu pengecualian: baris secret untuk reference `Bind` yang `Unregistered` (`secret.not_found`) atau terdaftar tanpa nilai (`secret.needs_secret`) ditulis ulang plan menjadi `caution` dan tidak menjadi blocker; statusnya tampil di `secrets` (AC-10). Pemetaan fakta secret ke `PlanSecretStatus`: `Unregistered` menjadi `unregistered`, `Present` `present`, `Missing` `missing`, `Locked`, `Busy`, atau `Failed` `unknown`. Store `Unsupported` tetap blocker (`secret.store_unsupported`).

Validate dan plan memvalidasi settings lewat satu helper yang dikeluarkan dari `settings_row` create: ia mengisi setiap secret field dengan reference nya (reference dari `secretRefs`, atau reference turunan placeholder untuk field yang dibangkitkan) lalu memanggil `SettingsSchema::validate`, supaya secret field wajib tidak muncul sebagai `settings_value` palsu. Port fixed dua service `create` dalam satu manifest sudah ditolak validate. Port member environment ini tetap dihitung bentrok, karena member tidak pernah diubah (AC-9) dan create baru di port yang sama pasti gagal.

**Runtime.** Plan memanggil `runtime::resolve(entry, selector, host, trust)` yang sama dengan `runtime.resolve`, lalu mencari baris `runtimes` ber `install_mode` `managed` dan `artifact_sha256` sama dengan SHA-256 artefak itu. `install` salah dan `runtimeId` terisi hanya kalau state baris itu `installed`; baris ber state lain (`failed`, `missing`, `drifted`, atau sedang dipasang) dan ketiadaan baris berarti `install` benar, dan Up menyerahkannya ke `runtime.install` dengan aturannya sendiri. Tidak ada aturan "pakai yang terpasang kalau cocok line": versi yang dipilih tanpa lockfile selalu hasil resolve catalog saat itu, dan pin nya urusan spec 0045.

### Model data

**Migrasi** (nomor bebas berikutnya, ditetapkan lead saat build; jenis kolom memakai nama jenis spec 0005):

- `environments.services` `json` array, `NOT NULL DEFAULT '[]'`: snapshot service, tiap elemen `{alias, provider, ownership, selector?, instanceRef?, instanceName?, dependsOn: [{service, condition, timeoutSeconds?}]}`. Tanpa settings, endpoint, dan `secretRefs`.
- Index `environments_name` diganti unique index `environments_name_unique` pada `name`.
- Unique index parsial `environments_manifest_location` pada `manifest_location WHERE manifest_location IS NOT NULL`.
- Kolom lain tidak berubah. `manifest_location` dan `manifest_sha256` mulai diisi oleh import. `lock_sha256` (spec 0045), `last_operation_id`, dan `environment_instances.dependency_conditions` (spec 0046) tidak disentuh fitur ini.

**Domain** (`stackly-domain`, usulan untuk spec 0004 dan 0005):

- `Environment` dan `StoredEnvironment` mendapat `manifest_location` (opsional, `PathBuf` yang dicek absolut seperti kolom `abspath` lain) dan `declared: Vec<DeclaredService>`; `Environment::new` tetap membuat revision 1; mutator `set_manifest(location, sha256, declared)` menaikkan revision 1.
- `DeclaredService { alias: MemberAlias, provider: ProviderName, ownership: Ownership, selector: Option<RuntimeSelector>, instance_ref: Option<InstanceName>, instance_name: Option<InstanceName>, depends_on: Vec<DependencyEdge> }`, `DependencyEdge { service: MemberAlias, condition: DependencyCondition, timeout_seconds: Option<u32> }`, `DependencyCondition::Ready`. Invariant: alias unik; `depends_on` hanya ke alias yang ada.
- `OperationKind::ImportEnvironment` (`import_environment`) dan `RemoveEnvironment` (`remove_environment`), tidak resumable. Precheck import me resolve nama ke `id` environment yang ada, atau membuat `EnvironmentId` baru lewat `IdSource` (`new_environment_id`). Kedua operasi memakai resource `OperationResource::Environment(id)` dan mengambil dua lock sekaligus, `LockKey::EnvironmentName(name)` dan `LockKey::Environment(id)`, supaya import, remove, dan nanti Up (spec 0046) saling mengecualikan.
- `GuardView` mendapat `environments()` (semua baris, dengan nama, lokasi, dan revision) untuk guard import dan remove.
- `read::environments` ikut membaca `manifest_location` dan `services`; `WriteOp::InsertEnvironment` dan `UpdateEnvironment` ikut menulisnya; `WriteOp::DeleteEnvironment { id, expected }` baru, yang dalam satu batch menghapus `instance_demands` `released` ber `source_id` itu, `secret_reference_uses` ber `resource_kind` `environment` dan `resource_id` itu, memindahkan `secret_references` ber `owner_kind` `environment` dan `owner_id` itu ke `user` (satu satunya perpindahan pemilik yang diizinkan trigger spec 0005), lalu menghapus baris `environments`.

**Core** (`crates/core/src/environment/`): `manifest.rs` (tipe raw serde dan `schemars`, loader), `validate.rs`, `plan.rs`, `status.rs`, `register.rs` (guard import dan remove). Tidak ada kode macOS; lulus `cargo check` Windows.

### Status environment

Dihitung daemon dari snapshot dan registry, tanpa file:

- `serviceCount` = jumlah service snapshot. `readyCount` = service snapshot yang instance nya (member, atau `instanceRef` yang bernama sama untuk service shared tanpa member) ber lifecycle `running` dan readiness `ready`.
- `status` (`EnvironmentStatus`, enum terbuka): `ready` kalau `readyCount == serviceCount` dan `serviceCount > 0`; `down` kalau `readyCount == 0`; selain itu `partially_ready`. Spec 0046 boleh menambah nilai (misalnya saat Up berjalan); klien lama membacanya `unrecognized` dan menampilkan "Unknown".
- Alasan per service yang belum ready (`EnvironmentService.notReady`): `not_created` kalau belum ada instance, selain itu menurut tabel *Lifecycle ke aksi*, dengan `detail` dari detail readiness atau `lastFailure` instance yang sudah lewat `Redactor`. Spec 0046 menambah alasan dari operasi Up terakhir.

### State manifest

Dihitung `env.get` dengan `checkManifest` true, nilai pertama yang cocok menang:

1. `missing`: `manifest_location` kosong, atau file tidak ada.
2. `unreadable`: file ada tetapi `read_file_limited` gagal.
3. `invalid`: validate menghasilkan finding `error`; `problems` berisi jumlahnya. Kalau catalog tidak tersedia, validate dilewati dan urutan lanjut ke butir berikutnya dengan `problems` 0.
4. `name_changed`: `metadata.name` file berbeda dari nama environment.
5. `changed`: digest file berbeda dari `manifest_sha256`.
6. `in_sync`.

### API surface

Semua method lewat handshake dan peer user yang sama (spec 0006). Modul DTO baru `stackly-protocol::env`. Setiap params type mengimplementasi `Validate`; path memakai aturan `import_path_code` (absolut, paling panjang 1024 byte, tanpa NUL, tanpa segmen `.` atau `..`).

| Method | Jenis | Recovery | Input | Output | Error utama |
| --- | --- | --- | --- | --- | --- |
| `env.validate` | baca, bukan operasi | refused | `EnvValidateParams { path }` | `EnvValidation` | `env.manifest_unreadable`, `catalog.unavailable` |
| `env.plan` | baca, bukan operasi | refused | `EnvPlanParams { source: EnvSource }` | `EnvPlan` | `env.manifest_unreadable`, `env.not_found`, `catalog.unavailable` |
| `env.list` | baca | refused | tanpa params | `EnvironmentList { items }` | |
| `env.get` | baca | refused | `EnvGetParams { id, checkManifest }` | `EnvironmentDetail` | `env.not_found` |
| `env.import` | operasi `import_environment` | refused | `ImportEnvironmentParams { path, expectedManifestSha256, relinkEnvironmentId? }` | `EnvironmentDetail` | `env.manifest_unreadable`, `env.manifest_invalid`, `env.manifest_changed`, `env.name_taken`, `env.relink_mismatch`, `env.limit_reached` |
| `env.remove` | operasi `remove_environment` | refused | `RemoveEnvironmentParams { id, expectedRevision }` | `RemovedEnvironment { id, name }` | `env.not_found`, `resource.revision_mismatch` (kode yang sudah ada), `env.lease_active` |

`env.import` dan `env.remove` lewat `ipc::operation::replay_operation` lalu `start_operation`, seperti `provider.import`; permintaan ulang dengan key yang sama membalas detail apa adanya saat itu.

**DTO** (camelCase; enum terbuka berakhir `unrecognized`; hitungan memakai `Count`, revision `Revision`):

- `EnvSource`: `{type: "file", content: <path>}` atau `{type: "registered", content: <uuid>}` (daemon membaca `manifest_location` nya).
- `EnvValidation { file, valid, apiVersion?, supportedApiVersions, name?, manifestSha256?, services: [DeclaredServiceDto], findings: [EnvFinding], findingCount, omitted }`.
- `EnvFinding { severity: FindingSeverity, code, location, service?, message }` (`FindingSeverity` dipakai ulang dari modul `provider`).
- `DeclaredServiceDto { alias, provider, ownership: EnvOwnership, selector?, instanceRef?, instanceName?, dependsOn: [ServiceDependency { service, condition: DependencyConditionKind, timeoutSeconds? }] }`; `EnvOwnership` `dedicated`, `shared`.
- `EnvPlan { validation, registration: PlanRegistration, environment?: EnvironmentRef { id, name, revision }, registeredLocation?, applicable, planDigest?, items: [PlanItem], order: [alias], blockers: [PlanBlocker { service?, error: ErrorBody }], detached: [DetachedMember { alias, instance: InstanceRef, ownership }], needsSecrets: Count }`; `PlanRegistration` `new`, `registered`, `name_taken`.
- `PlanItem { alias, ownership, action: PlanAction, steps: [PlanStep], instance: PlanInstance { id?, name }, provider, trust?: PlanTrust (hanya item `create`), runtime?: PlanRuntime { selector, version, artifactId, artifactSha256, install, runtimeId?, downloadSize? }, endpoints: [PlanEndpoint { name, host, port: PortRequest }] (item `create`: setiap endpoint IP paket dalam urutan paket, yang tidak ditulis manifest sebagai `auto`, `host` dari `defaultHost` paket; item lain: endpoint IP instance dengan port efektifnya sebagai `fixed`), secrets: [PlanSecret { field, reference?, status: PlanSecretStatus }], waitsFor: [alias], differences: [PlanDifference { field, manifest, instance }], checks: [PreflightCheck] }`; `PlanAction` `create`, `start`, `keep`; `PlanStep` `install_runtime`, `create_instance`, `start_instance`; `PlanTrust` `trusted`, `approval_required`, `revoked`; `PlanSecretStatus` `present`, `missing`, `unregistered`, `unknown`, `generated`. `PortRequest` dan `PreflightCheck` dipakai ulang dari modul `instance`.
- `EnvironmentSummary { id, name, revision, manifestLocation?, manifestSha256?, status: EnvironmentStatus, readyCount, serviceCount, updatedAt }`.
- `EnvironmentDetail { summary, services: [EnvironmentService], detached: [DetachedMember], manifestState?: ManifestState, problems?: Count }`; `EnvironmentService { alias, provider, ownership, selector?, instanceRef?, instance?: ServiceInstance { id, name, runtimeVersion, lifecycle, readiness, health }, notReady?: ServiceNotReady { reason: NotReadyReason, detail? } }`; `NotReadyReason` `not_created`, `stopped`, `starting`, `stopping`, `failed`, `not_ready`, `unavailable`.
- Tambahan `instance`: `InstanceOwnership` mendapat `dedicated` dan `shared`; `InstanceSummary.environments: Vec<InstanceEnvironment { environmentId, name, alias, ownership: EnvOwnership }>` dengan `#[serde(default)]`.

`manifestLocation` dikirim sebagai path absolut (klien butuh nya untuk membuka file); klien menampilkan home sebagai `~`. Pesan error dan catatan audit memakai `home_mask`.

**Event**: tidak ada `EventKind` baru. Klien memuat ulang `env.list`, dan `env.get` halaman yang sedang terbuka dengan `checkManifest` false, saat `operation.state_changed` terminal membawa resource ber kind `environment`, dan, dengan jeda 300 ms, saat `instance.state_changed`, `instance.health_changed`, atau `instance.removed` tiba, serta saat tersambung ulang. `env.get` dengan `checkManifest` true hanya dipanggil saat halaman detail dibuka, saat app kembali aktif, sesudah Validate, Reload, atau Locate…, dan sesudah operasi environment itu selesai, jadi file pengguna tidak dibaca pada setiap event instance.

**Anggaran frame**: `env.list` 64 kali sekitar 1 KiB; `EnvironmentDetail` dan `EnvPlan` paling besar 32 service kali sekitar 12 KiB (sembilan check per item) ditambah 100 finding, jauh di bawah 1 MiB; test menjaga plan 32 service di bawah 900 KiB.

### Kode error di wire

Area baru `env` (pemilik spec ini; ditambahkan ke `AREAS`). Semua `retryable` false. Pesan lowercase tanpa titik.

| Kode | Kategori | Kapan | Kunci `details` |
| --- | --- | --- | --- |
| `env.manifest_unreadable` | `invalid_input` | file tidak bisa dibaca | `reason`: `not_found`, `not_a_file` (termasuk symlink, karena `read_file_limited` tidak membedakannya), `permission_denied`, `too_large`, `io` |
| `env.manifest_invalid` | `invalid_input` | import manifest dengan finding `error` | `errors` |
| `env.manifest_changed` | `conflict` | digest file berbeda dari `expectedManifestSha256` | |
| `env.name_taken` | `conflict` | nama terdaftar di lokasi lain | `name`, `location` (dengan `~`) |
| `env.limit_reached` | `conflict` | sudah 64 environment | `limit` |
| `env.not_found` | `not_found` | `id` tidak dikenal | |
| `env.lease_active` | `conflict` | remove saat masih ada lease | `count`; `items` ber kind `instance` (`instanceId`, `name`), paling banyak 32 |
| `env.instance_ref_not_found` | `not_found` | blocker plan: `instanceRef` tidak ada | `service`, `instanceRef` |
| `env.instance_ref_mismatch` | `conflict` | blocker plan: provider instance berbeda | `service`, `expected`, `actual` |
| `env.instance_blocked` | `conflict` | blocker plan: instance member atau `instanceRef` tidak bisa dinyalakan (*Aksi plan*) | `service`, `instance`, `reason` |
| `env.manifest_renamed` | `conflict` | blocker plan: sumber `registered` tetapi file nya mendeklarasikan nama lain | `declared`, `registered` |
| `env.relink_mismatch` | `conflict` | relink ke environment yang namanya berbeda dari `metadata.name` file | `declared`, `registered` |

Kode lokal CLI: `cli.env_invalid` (`invalid_input`, exit 2, details `errors`, `warnings`), `cli.env_plan_blocked` (`conflict`, exit 3, details `blockers`), `cli.environment_not_found` (`not_found`, exit 10, details `name`).

### CLI

Grup baru `stackly env` (`apps/cli/src/commands/env/`). Path dibuat absolut dari cwd dan dinormalkan (`.` dan `..` dibuang secara teks) lewat helper bersama yang dikeluarkan dari `apps/cli/src/commands/provider/validate.rs`, supaya `--file ../x/stackly.yaml` tidak ditolak `import_path_code`.

| Perintah | Perilaku | `result` `--json` |
| --- | --- | --- |
| `stackly env validate [--file <path>]` | `env.validate`. Valid: `ok <name> <n> services sha256:<digest>`, exit 0. Tidak valid: satu baris per finding `<file>: <location>: <code>: <message>` ke stderr, lalu `<e> error(s), <w> warning(s)`, `cli.env_invalid` | `EnvValidation` |
| `stackly env plan [--file <path> \| <environment>]` | `env.plan`. Tabel `ACTION SERVICE INSTANCE PROVIDER VERSION ENDPOINTS` dengan satu baris per langkah di `steps` (`install`, `create`, `start`) dan satu baris `keep` untuk item tanpa langkah; lalu `blocked:`, `secrets:` (setiap reference dengan statusnya), `differs:`, dan `not in manifest:` ke stderr; lalu ringkasan "Plan: <a> to install, <b> to create, <c> to start, <d> to keep." (`<c>` menghitung setiap `start_instance`, termasuk milik item `create`) Exit sesuai AC-16 | `EnvPlan` |
| `stackly env import [--file <path>] [--relink]` | `env.plan` untuk mengambil `manifestSha256`, lalu `env.import`; `--relink` mengirim `relinkEnvironmentId` dari `EnvPlan.environment.id` saat `registration` `name_taken`. Tanpa konfirmasi (tidak menjalankan apa pun). Kalau `summary.revision` sesudah import sama dengan `EnvPlan.environment.revision` sebelumnya: "Already imported." exit 0 | `EnvironmentDetail` |
| `stackly env list` | `env.list`. Tabel `NAME STATUS READY FILE`; kosong: "No environments." | `{ items: [EnvironmentSummary] }` |
| `stackly env remove <environment> [--yes]` | konfirmasi "Remove environment <name>? Instances and data are kept. [y/N]", lalu `env.remove` dengan revision dari `env.list` | `RemovedEnvironment` |

`<environment>` di resolve seperti `InstanceTarget`: 36 karakter yang terbaca sebagai UUID adalah id, selain itu nama persis dari `env.list`. `stackly instance list` mendapat kolom `ENVIRONMENT` (nama dipisah koma, atau `-`).

### Layar

Semua komponen dari `Components/`, `State/`, `Status/`; copy bahasa Inggris dari mockup kecuali yang disebut berbeda di sini (spec menang atas mockup).

**Sidebar.** Pilihan sidebar menjadi `SidebarSelection`: `section(SidebarSection)` atau `environment(UUID)`; nilai scene storage untuk seksi tidak berubah, environment disimpan `environment:<uuid>` dan jatuh ke `.environments` kalau id nya tidak ada lagi. Baris Environments mendapat `DisclosureGroup` (terbuka sejak awal) berisi satu baris per environment menurut nama: simbol status dari `StatusAppearance.environment`, nama, dan teks `{ready}/{total}`. Baris Environments sendiri tidak mendapat badge. `ShellDestination` mendapat `environments` dan `environment(UUID)`.

**`StatusAppearance.environment(status:)`**: `ready` "Ready" positive `checkmark.circle.fill`; `partially_ready` "Partially Ready" caution `circle.lefthalf.filled`; `down` "Down" neutral `circle`; `unrecognized` "Unknown" neutral `questionmark.circle`. Tabelnya ditambahkan ke `apps/macos/design.md`.

**Halaman daftar** (`.environments`): judul "Environments", subjudul "{n} projects". Toolbar: Import…. Satu baris per environment: nama, `StatusLabel`, "{ready} of {total} ready · {file}"; klik membuka detail. Kosong: `StatePlaceholder` "No Environments" / "Import a stackly.yaml to review its plan." dengan aksi Import….

**Halaman detail** (`environment(id)`): judul nama, subjudul file dengan `~`.

- Toolbar: Validate, Plan…, dan menu More (Open Manifest, Reveal in Finder, Copy Path, Reload Manifest, Remove Environment…). Tempat Up… dan Down milik spec 0046 dan 0047, di kanan Plan….
- Banner manifest (`StateBanner`, hanya saat bukan `in_sync`): `changed` "The manifest changed on disk." dengan Review Plan dan Reload; `missing` atau `unreadable` "The manifest file can’t be read." dengan Locate… (pemilih file, lalu import dengan `relinkEnvironmentId` environment ini; file yang mendeklarasikan nama lain ditolak dengan pesan `env.relink_mismatch`); `invalid` "The manifest has {n} problems." dengan Show Problems; `name_changed` "This file now declares “{name}”." dengan Import as New.
- Banner partially ready (`StateBanner` caution, saat `0 < readyCount < serviceCount`): judul "{n} of {m} ready", satu `PartialItem` per service belum ready: nama "{instance} ({alias})", alasan dari `notReady` ("stopped", "not created", "failed", atau detail readiness), aksi membuka instance.
- Seksi Services: `Table` native, kolom Service (alias), Selector (`{provider}@{selector}` atau `instanceRef: {name}`), Instance (nama atau "Will be created"), Version (versi runtime atau "—"), Ownership ("Dedicated" atau "Shared ref"), Status (`StatusLabel(.row)` dari `StatusAppearance.instance`, atau "Not created" neutral `circle.dashed`), dan kolom aksi dengan `ActionMenu`. Kolom menyempit lewat `ColumnSet`: Version lalu Selector disembunyikan lebih dulu. Member `detached` tampil di seksi "Not in Manifest" dengan nama instance dan ownership.
- Seksi Details (`CardSection`): Manifest (path, `CopyButton`), Schema `stackly/v1alpha1`. Baris Lock dan Local override ditambahkan spec 0045 di bawahnya, baris Lease oleh spec 0047.
- Seksi Problems (sesudah Validate gagal, atau dari Show Problems): `CheckList` dengan satu item per finding, judul `message`, detail `location`.

**Menu baris service** (AC-19): seksi 1 isi `OpenInMenu` (lewat `OpenInModel.content(summary:detail:…)`; `InstanceDetail` dimuat saat menu dibuka, seperti inspector); seksi 2 Copy Connection URL (aksi salin connection string pertama dari kartu Connection spec 0021, termasuk aturan secret nya); seksi 3 Show Instance, Show Logs (membuka detail instance di tab Logs); seksi 4 aksi lifecycle instance yang sudah ada (Start, Stop, atau Retry). Service belum dibuat: hanya Show Plan….

**Sheet plan** (`StacklySheet`, judul "Plan for {name}", subjudul "Review what Up will do. Nothing runs until you confirm."): satu baris per item dalam `order`:

- `create` (berarti buat lalu nyalakan): badge "+ Create" positive, teks "{alias} → {instance} · {software} {version} · {host}:{port}" (port `auto` ditulis "auto port"); kalau `runtime.install`, baris kedua "Installs {software} {version}".
- `start`: badge "▸ Start" informative, "{alias} → {instance} · wait for readiness".
- `keep`: badge "= Keep" neutral, "{alias} → {instance} · {version}".
- Di bawah baris: provider yang butuh approval ("Needs approval" dengan aksi Review in Catalog, navigasi ke detail provider spec 0037), `differences` ("Differs from manifest: {field}"), dan setiap reference secret: "Uses secret {reference}" untuk `present`, "Needs secret: {reference}" selain itu.
- `StateBanner` critical di atas daftar saat ada blocker, satu item per blocker dengan pesan dan hint kode nya; untuk manifest tidak valid, daftar Problems menggantikan daftar item.
- Catatan kaki: "Services start in dependency order and wait for readiness."
- Tombol: dari halaman detail hanya Done. Dari Import…: Cancel dan Import (nonaktif dengan alasan saat manifest tidak valid atau `name_taken`; blocker lain tidak menghalangi import).

**Import** (`EnvironmentImportModel`, pola `ProviderImportModel`): `FilePicking` baru (`NSOpenPanel`, file `.yaml` dan `.yml`, pesan "Choose a stackly.yaml. You’ll review trust and a plan before anything runs."), lalu `env.plan` untuk path itu, sheet plan, lalu `env.import` dengan `manifestSha256` dari plan. `name_taken` menampilkan "“{name}” is already registered at {location}." dengan Use This File (import dengan `relinkEnvironmentId` dari `EnvPlan.environment.id`). Sesudah sukses, sidebar memilih environment baru.

**Validate**: sukses menampilkan feedback "{name}: schema stackly/v1alpha1 valid · no cycles · secrets referenced."; gagal membuka seksi Problems.

**Remove**: `ConfirmationRequest` destructive "Remove “{name}”?" / "Stackly forgets this environment. Its instances, data, and stackly.yaml are kept." / "Remove". `env.lease_active` menampilkan "Bring it down first." dengan daftar instance.

**Overview**: `SectionCard` "Environments" di kolom kanan di atas Operations, hanya saat ada environment: paling banyak lima baris (nama, `StatusLabel`, "{ready} of {total} ready · {file}"), lalu Show All; klik membuka environment.

**Instances**: kolom "Environment" (nama environment pertama di `environments`, "+{n}" kalau lebih, "—" kalau kosong), bisa diurutkan, disembunyikan lebih dulu saat sempit. Inspector: baris "Environment" (nama, tautan ke environment; "—") dan "Ownership" "Dedicated", "Shared", atau "Manual". Kata "lease" di baris itu milik spec 0047.

**Onboarding**: `OnboardingCopy.Feature` baru "Environments as files" / "A stackly.yaml per project. Review the plan before anything runs." dengan `SectionIcon(.environments)`. Kalimat mockup tentang bring up menyusul di spec 0046.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| validate | `manifestSha256` | SHA-256 byte file yang dibaca |
| validate | paket provider untuk aturan | entri catalog di memori untuk `provider` (revision catalog saat ini) |
| plan | nama instance `create` | `instanceName`, atau `<metadata.name>-<alias>` |
| plan | `runtime.version`, `artifactId`, `artifactSha256`, `downloadSize` | `runtime::resolve` atas `version` (default `stable`) |
| plan | `runtime.install`, `runtimeId` | baris `runtimes` `managed` ber `artifact_sha256` artefak hasil resolve dan state `installed` |
| plan | `trust` | `require_entry_trust` (membaca ulang baris import, spec 0037) |
| plan | status secret | baris `secret_references` untuk reference itu dan presence store tanpa dialog, lewat fakta secret `gather` |
| plan | `differences` | *Aturan `differences`* |
| plan | `registration`, `registeredLocation` | baris `environments` ber `name` sama; lokasi dibanding sesudah folder induknya di canonicalize |
| list, get | `readyCount`, `status`, `notReady` | snapshot `services`, `environment_instances`, dan status instance di supervisor |
| get | `manifestState`, `problems` | *State manifest*; hanya saat `checkManifest` true |
| plan | `PlanEndpoint.host` | `defaultHost` endpoint di paket provider |
| plan | `keep`, `start`, blocker instance | tabel *Lifecycle ke aksi* atas lifecycle dan `blocker` instance |
| import | snapshot | `services` hasil validate file yang sama |
| GUI | "{file}" | `manifestLocation` dengan home diganti `~` di klien |
| GUI | software dan versi di sheet | `DisplayNames` catalog (spec 0017) dan `runtime.version` |

### Key invariants

- Validate dan plan tidak pernah memulai operasi, menulis registry, menulis file, mengunduh, spawn, atau mengubah trust (AC-6).
- Nilai secret tidak pernah ada di manifest, snapshot, plan, log, atau audit; yang ada hanya reference `local:`.
- Environment tidak pernah mengonfigurasi instance yang dipinjam, dan tidak pernah mengubah instance yang sudah ada supaya cocok dengan manifest.
- Nama environment unik; satu instance paling banyak punya satu pemilik `dedicated` (index spec 0005).
- Import dan remove hanya menulis registry. Remove tidak pernah menghapus instance, data, runtime, atau nilai Keychain.
- Daftar dan status environment tidak bergantung pada file pengguna; hanya `env.get`, `env.validate`, `env.plan`, dan `env.import` yang membaca file.
- Kode yang membaca paket untuk plan lewat gate trust yang ada (`require_entry_trust`), bukan cek sendiri.

### Security model

- Peer adalah user yang sama (spec 0006); registry per user, jadi `instanceRef` selalu dalam access scope user itu (VAL-014).
- Daemon membaca path yang diberi klien hanya dengan `read_file_limited` (tanpa mengikuti symlink terakhir, hanya file biasa, paling besar 256 KiB) dan tidak pernah mengeksekusinya. Isi manifest tidak pernah ditulis ke log.
- Pesan finding tidak pernah memuat nilai dari file: setiap kode punya template tetap, dan teks error `serde_yaml_ng` serta `SettingsSchema` hanya dipakai untuk memilih kode dan `location`. `Redactor` tidak diandalkan di sini, karena ia hanya mengenal nilai yang pernah dipelajarinya.
- Manifest tidak bisa membawa trust: tidak ada field approval, dan provider `pending` hanya tampil sebagai blocker dengan navigasi ke alur approval spec 0037 (VAL-018).
- Catatan audit `import_environment` dan `remove_environment` memuat nama, `manifestSha256`, dan lokasi dengan `~`, lewat `set_audit_note`.

### Configuration required

Tidak ada variabel environment atau credential baru.

### Critical test scenarios

- Manifest contoh ERP lolos validate dan menghasilkan plan enam install, enam create, dan enam start di root kosong; verifies **AC-1**, **AC-5**, **AC-25**.
- Satu manifest per kode di *Aturan validasi*, termasuk cycle tiga service dengan jalurnya di pesan dan dua port fixed sama; semua finding kembali sekaligus; verifies **AC-2**.
- Sentinel secret di `secretRefs`, di `settings`, dan sebagai reference `local:instance/...`: ditolak, dan sentinel tidak ada di balasan, log, registry, atau test root; verifies **AC-3**.
- Plan dan validate terhadap test root: pohon file dan generation registry sama sebelum dan sesudah, fake downloader, supervision, dan secret write nol panggilan; verifies **AC-4**, **AC-6**.
- Blocker: port fixed bentrok dengan instance lain, provider lokal `pending`, selector tanpa artefak, nama instance terpakai, nama environment di lokasi lain; `applicable` false dan kode sesuai; verifies **AC-7**, **AC-11**.
- `instanceRef`: tidak ada, provider berbeda, field terlarang, instance berjalan (`keep`) dan berhenti (`start`); verifies **AC-8**.
- Environment dengan member (registry test menulis member langsung): setiap baris *Lifecycle ke aksi*, instance ber `blocker`, setiap `field` `differences`, port fixed service baru yang sama dengan port member (blocker), dan `detached`; summary dari event membawa `environments` yang sama dengan `instance.list`; verifies **AC-9**, **AC-14**, **AC-15**.
- Reference `present`, `missing`, `unregistered`, dan field tanpa `secretRefs` (`generated`); `needsSecrets` benar dan `applicable` tetap true; verifies **AC-10**.
- Import: baru, sama (revision tetap), berubah, relink (termasuk nama berbeda dan lokasi yang sudah dipegang environment lain), `/tmp` lawan `/private/tmp`, digest berbeda, tidak valid, environment ke 65, key yang sama dua kali; verifies **AC-12**.
- Remove: dengan lease `active` ditolak; tanpa lease menghapus baris dan demand `released`, instance dan data tetap, ownership kembali `manual`; verifies **AC-13**.
- CLI: golden manusia dan `--json` untuk validate valid dan tidak valid, plan applicable, blocked, dan tidak valid, dengan exit code; binary test membandingkan `result` CLI dengan balasan daemon; verifies **AC-16**, **AC-17**.
- Snapshot GUI terang, gelap, dan sempit: daftar kosong dan terisi, detail ready, partially ready, dan down, setiap banner manifest, sheet plan dengan ketiga kata kerja, blocker, dan Problems, Overview dengan seksi Environments; verifies **AC-18** sampai **AC-23**.
- Kontrak: fixture `protocols/fixtures/env/` terbaca Rust dan Swift, `cargo xtask codegen --check`, test anggaran frame, klien lama membaca `ownership` baru sebagai `unrecognized`; verifies **AC-24**.

### Titik sambung untuk spec 0045, 0046, 0047

**Spec 0045 (lockfile dan override)**

- Plan menerima masukan tambahan di satu tempat: `environment::plan::PlanInputs { manifest, lock: Option<..>, overrides: Option<..> }`. Fitur ini selalu mengirim `None`; spec 0045 mengisi keduanya dari `stackly.lock` dan `.stackly.local.yaml` di folder manifest dan menambah field ke `EnvPlan` (minor protokol).
- Selector: `PlanRuntime.selector` adalah teks manifest; `version` dan `artifactId` adalah hasil resolve. Lock menggantikan pemanggilan `runtime::resolve` dengan artefak yang dikunci; revision provider untuk validasi diambil dari entri catalog sampai lock mem pin nya.
- Override port dan storage masuk sebelum evaluasi create (mengganti `endpoints` draft), tidak pernah mengubah `manifestSha256` atau snapshot.
- `environments.lock_sha256` belum diisi. JSON Schema lockfile ditaruh di `schemas/environment/` di samping `environment.v1alpha1.schema.json`.
- UI: baris Lock dan Local override di seksi Details; `EnvFinding.severity` `warning` tersedia; `stackly env lock` dan `export` masuk grup `apps/cli/src/commands/env/`.

**Spec 0046 (up dan graph)**

- `env.up` menerima `source: EnvSource` dan `expectedPlanDigest`; daemon menghitung ulang plan dan menolak kalau digest berbeda. Up untuk manifest yang belum terdaftar memanggil `environment::register` yang sama dengan import di dalam operasinya.
- `PlanItem` adalah daftar kerja: `create` (dengan `runtime.install`), `start`, `keep`; `order` dan `waitsFor` adalah graph nya; `ServiceDependency.timeoutSeconds` kosong berarti default yang diputuskan spec 0046.
- Up yang menulis member juga menulis snapshot (`Environment::set_manifest`) dan `manifest_sha256`; `dependency_conditions` dan `last_operation_id` milik spec 0046.
- `PlanSecretStatus` selain `present` dan `generated` adalah bahan status needs secret; `secretRefs` dikirim ke create sebagai `SecretInput::Bind`.
- `EnvPlan.detached` dan `PlanItem.differences`: aturan fitur ini adalah "tidak disentuh". Kalau Up ingin melepas member `detached`, itu keputusan spec 0046.
- `EnvironmentStatus` dan `NotReadyReason` enum terbuka untuk nilai baru; `EventKind` baru untuk environment berarti kenaikan major dan diputuskan di sana.
- UI: tombol Up… di toolbar menggantikan Plan… (sheet yang sama mendapat Bring Up), item menu "Create with Up…" menggantikan Show Plan…, banner partially ready mendapat alasan dari operasi Up, dan kalimat onboarding.

**Spec 0047 (down dan lease)**

- Lease environment disimpan `instance_demands` ber `source_kind` `environment` dan `source_id` id environment (fondasi spec 0005). `env.remove` fitur ini menolak selama ada lease `active`.
- `InstanceSummary.environments` dan `ownership` tersedia untuk pesan dampak stop, Stop Services, dan ganti endpoint.
- Follow-up spec 0026 (restart pada instance yang dipinjam environment butuh override atau tidak) diserahkan ke spec 0047, karena jawabannya bergantung pada aturan override stop di sana.
- UI: tombol Down di toolbar, baris Lease di seksi Details, kata "· lease" di baris Ownership inspector.

### Jawaban Follow-up spec lain

| Asal | Pertanyaan | Jawaban |
| --- | --- | --- |
| Spec 0001 L145 | JSON Schema environment dan lockfile di `schemas/` | `schemas/environment/environment.v1alpha1.schema.json` hasil codegen (AC-24); lockfile di folder yang sama oleh spec 0045 |
| Spec 0004 L151, L152 | `EnvironmentName`, `MemberAlias` | `metadata.name` dan key `services` (*Schema*); `EnvironmentName` kini juga input import, bukan hanya fitur 46 |
| Spec 0005 L263 | keunikan `environments.name` | unik lewat `environments_name_unique` (AC-11, *Model data*) |
| Spec 0005 L265 | `environment_instances` | dibaca untuk status, plan, dan ownership; ditulis mulai spec 0046 |
| Spec 0010 L373, L554; spec 0022 L306, L427 | `InstanceOwnership` hanya `manual` | nilai `dedicated` dan `shared` dari keanggotaan, plus `InstanceSummary.environments` (AC-15) |
| Spec 0022 L77, L130, L588 | kolom dan baris Environment, seksi Environments Overview | AC-23, *Layar* |
| Spec 0011 L615 | `stackly.yaml` hanya menerima `local:` | AC-3; bagian lockfile dan export tetap terbuka untuk spec 0045 |
| Spec 0012 L428 | `PartialNote` untuk readiness environment | banner partially ready memakai `StateBanner` dengan `PartialItem` (*Layar*); alasan Up menyusul di spec 0046 |
| Spec 0013 L365, L736 | Environments menggantikan `UnbuiltPage(.environments)` | AC-18 |
| Spec 0016 L117, L457 | baris Welcome "Environments as files" | AC-23, dengan kalimat detail yang belum menjanjikan Up |
| Spec 0026 L79, L407, L420 | restart pada instance yang dipinjam | diserahkan ke spec 0047 (*Titik sambung*); fitur ini tidak mengubah `instance.restart` |
| Spec 0068 L194 | fixture "environment partially ready" | skenario `environment-partially-ready` (AC-24), ditambahkan di langkah GUI pertama yang memakai DTO nya |
| Spec 0070 L275 | `OpenInMenu` di Environments | AC-19 |
| Spec 0029 L432 | kategori kode error environment | area `env` (*Kode error di wire*) |

## Build plan

Urutan mengikuti pendekatan Journey: jalur "menerima `stackly.yaml`, memvalidasinya, mengimpornya, dan membaca plan nya" dibangun lengkap dari core sampai setiap state gagalnya, lalu CLI, lalu GUI. Protokol naik satu minor, di langkah 4, dengan seluruh kontrak fitur ini (nomor bebas berikutnya, ditetapkan lead saat build; begitu juga nomor migrasi).

**Milestone 1: Manifest, validate, dan plan di daemon** (needs scope 01 sampai 07 di `main`)

1. Domain dan storage: migrasi `services` dan unique index nama; `DeclaredService`, `DependencyEdge`, field baru `Environment`, `set_manifest`; dua `OperationKind` dan baris tabel resume nya; `read::environments`, `WriteOp` insert, update, dan `DeleteEnvironment`; `CODED_COLUMNS` atau `FREE_CODE_COLUMNS`; `GuardView::environments()`; canonicalize folder induk di port `Filesystem` kalau belum ada; fake registry testkit. Memenuhi **AC-11**, **AC-12**, **AC-13**.
2. Core `environment::manifest` dan `validate`: tipe raw, loader ketat, setiap aturan tabel *Aturan validasi*, template pesan tetap per kode, helper settings bersama yang dikeluarkan dari `settings_row`, fixture manifest di `crates/core/tests/fixtures/environments/` (`erp-development.yaml`, graph dengan provider `fake`, satu file per kode), dan JSON Schema lewat `cargo xtask codegen`. Memenuhi **AC-1**, **AC-2**, **AC-3**.
3. Core `environment::plan` dan `status`: `gather_planned` dan `CreateFacts.planned_runtime` di create (`facts`, `evaluate`) tanpa mengubah golden preflight, penulisan ulang baris secret `Bind`, pencocokan registry, tabel *Lifecycle ke aksi*, `steps`, `differences`, `detached`, urutan topologis, `planDigest`, `PlanInputs`, status environment. Memenuhi **AC-5**, **AC-7**, **AC-8**, **AC-9**, **AC-10**, **AC-14**.
4. Protokol: modul `env`, enam method (`recovery refused`), DTO, `InstanceOwnership` dan `InstanceSummary.environments`, area kode `env` dengan hint, fixture `protocols/fixtures/env/` dan entri `instance/index.json` yang terdampak, kenaikan minor `ProtocolVersion::CURRENT`, `cargo xtask codegen`, `cargo xtask fuzz seed`. Memenuhi **AC-24**, **AC-15**.
5. `stacklyd`: handler `env.validate`, `env.plan`, `env.list`, `env.get`; operasi `import_environment` dan `remove_environment` dengan `commit_guarded`, replay, dan `set_audit_note`; pemetaan DTO dan error per varian; cache keanggotaan di memori dan pengisian `ownership` serta `environments` di setiap pembangun `InstanceSummary` (list, get, event, doctor); test tanpa efek samping (pohon file, generation, fake), test sentinel, dan test anggaran frame. Memenuhi **AC-4**, **AC-6**, **AC-12**, **AC-13**, **AC-14**, **AC-15**.

**Milestone 2: `stackly env`** (after langkah 4 untuk golden dari fixture; binary test needs langkah 5; bisa paralel dengan milestone 3)

6. CLI: grup `env` dengan `validate`, `plan`, `import`, `list`, `remove`, resolve target environment, tiga kode `cli.*`, kolom `ENVIRONMENT` di `instance list`, golden dan binary test terhadap daemon dengan provider `fake`. Memenuhi **AC-16**, **AC-17**, **AC-15**.

**Milestone 3: Environments di app** (after langkah 4; evidence manual needs langkah 5)

7. Data dan navigasi: `EnvironmentStore` terdaftar di `ShellSync` dengan aturan muat ulang *Event*, `SidebarSelection` dan baris anak sidebar, `ShellDestination` baru, `StatusAppearance.environment` dan tabelnya di `design.md`, `FilePicking`, skenario fixture `environment-partially-ready` dan `PageRig`. Memenuhi **AC-18**, **AC-22**, **AC-24**.
8. Halaman daftar dan detail: `EnvironmentsPage`, banner manifest dan partially ready, tabel Services dengan menu baris (`OpenInMenu`, Copy Connection URL, Show Instance, Show Logs, lifecycle), seksi Details dan Problems, Validate. Memenuhi **AC-18**, **AC-19**, **AC-21**, **AC-22**.
9. Sheet plan, alur Import… dengan relink, Reload, Open Manifest, Reveal, dan Remove. Memenuhi **AC-20**, **AC-21**.
10. Permukaan lain: seksi Environments di Overview, kolom dan baris inspector di Instances, baris onboarding; snapshot terang, gelap, dan sempit untuk semua layar. Memenuhi **AC-23**, **AC-22**.
11. Evidence dan manual: record evidence fixture untuk AC-5, AC-6, dan AC-25; langkah manual (GUI nyata, hanya dijalankan kamu) ditulis di `verify.md` bagian `### UI / manual`: impor manifest contoh ERP lewat Import…, baca sheet plan, buka manifest di editor lalu lihat banner "changed", jalankan Validate dan Remove, dan cek sidebar, Overview, serta tabel Instances dengan pointer saja. Memenuhi **AC-25**, **AC-22**.

**Sesi paralel yang aman**: sesudah langkah 4 masuk, CLI (langkah 6, `apps/cli/`) dan GUI (langkah 7 sampai 10, `apps/macos/`) tidak beririsan file dengan daemon (langkah 5, `apps/daemon/`). `crates/protocol/`, `protocols/`, `schemas/`, hasil codegen, dan migrasi hanya disentuh langkah 1, 2, dan 4, oleh satu sesi.

## Consequences

**Positive**:

- Satu sumber kebenaran untuk validasi dan plan: GUI dan CLI tidak bisa berbeda karena keduanya hanya menampilkan balasan daemon.
- Plan memakai evaluasi create yang sama dengan `instance.preflight`, jadi apa yang plan katakan bisa dibuat memang lolos precheck create saat Up.
- Secret tidak bisa bocor lewat manifest karena tidak ada field yang menerima nilai.
- Daftar environment tetap benar dan cepat walau file manifest hilang, dipindah, atau ada di volume lambat.

**Negative / tradeoffs**:

- Snapshot di registry dan file di disk bisa berbeda; halaman harus menjelaskannya (`manifestState`) dan kamu perlu Reload atau Up supaya sama lagi.
- Nama adalah identitas: dua checkout proyek yang sama tidak bisa terdaftar bersamaan tanpa mengganti `metadata.name`, dan mengganti nama di file berarti environment baru.
- `planned_runtime` dan helper settings bersama menyentuh jalur create yang sudah stabil; golden preflight menjaga perilakunya, tetapi review langkah 2 dan 3 perlu teliti.
- Perbedaan manifest dengan instance yang sudah ada hanya dilaporkan. Mengubah port atau versi service yang sudah dibuat tetap lewat alur instance (spec 0029, 0025), bukan lewat manifest.
- Plan melakukan trial bind port, jadi ia tidak sepenuhnya pasif terhadap jaringan loopback, sama seperti preflight.
- Tanpa `EventKind` baru, GUI memuat ulang `env.list` pada event instance; murah untuk 64 environment, tetapi bukan push murni.

**Neutral**:

- Dua `OperationKind` baru dan area kode `env`; method memakai awalan `env.` sedangkan `ResourceKind` dan `LockKey` tetap `environment`.
- `storage` hanya menerima satu nilai di v1alpha1; ia ada supaya contoh PRD valid dan supaya v1beta bisa menambah mode.
- Sampai spec 0046 dibangun, tidak ada member yang dibuat produk; kolom Environment, `keep`, `start`, dan `differences` diuji dengan member yang ditulis registry test.

### Update spec lain

Usulan untuk lead (architect spec ini tidak mengubah spec lain):

- **Spec 0004**: `Environment` mendapat `manifest_location` dan `declared`; tipe `DeclaredService`, `DependencyEdge`, `DependencyCondition`; `EnvironmentName` adalah input import.
- **Spec 0005**: kolom `environments.services`, unique index nama, `WriteOp::DeleteEnvironment` dan isi batch nya.
- **Spec 0007**: `import_environment` dan `remove_environment` di tabel jenis operasi (tidak resumable) dan kunci lock nya.
- **Spec 0008**: grup `stackly env` dan tiga kode `cli.*` di tabel perintah dan `result`.
- **Spec 0010 dan 0022**: `InstanceOwnership` `dedicated` dan `shared`, `InstanceSummary.environments`, kolom dan baris Environment, seksi Environments Overview.
- **Spec 0013**: `SidebarSelection`, baris anak sidebar, `ShellDestination.environments` dan `environment(UUID)`, `EnvironmentsPage`.
- **Spec 0016, 0068, 0070, 0012, 0011, 0001, 0029, 0026**: centang baris Follow-up sesuai tabel *Jawaban Follow-up spec lain* (spec 0026 dengan catatan "diserahkan ke spec 0047").
- **`apps/macos/design.md`**: tabel `StatusAppearance.environment`.

## Follow-up

- [ ] Lead: tetapkan nomor minor protokol dan nomor migrasi saat build, dan terapkan *Update spec lain* lewat `architect (update)`.
- [ ] Spec 0045: isi `PlanInputs.lock` dan `overrides`, baris Lock dan Local override, schema lockfile.
- [ ] Spec 0046: `env.up` dengan `expectedPlanDigest`, penulisan member dan snapshot, default `timeoutSeconds`, nasib member `detached`, tombol Up… dan Bring Up, kalimat onboarding.
- [ ] Spec 0047: tombol Down, baris Lease, jawaban Follow-up spec 0026 tentang restart pada instance yang dipinjam.
- [ ] `/sync` setelah build: catat di root `AGENTS.md` bahwa `env.validate` dan `env.plan` read only dan bukan operasi, bahwa plan memakai `gather` dan `evaluate` create, dan perintah `stackly env`.

## Rationale

Alasan dan opsi yang ditimbang: lihat [rationale.md](rationale.md).
