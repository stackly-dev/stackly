# 0009. Provider engine dan manifest schema

**Date**: 2026-10-18
**Status**: Proposed

## Summary

Spec ini mengunci kontrak provider Stackly: schema manifest deklaratif `stackly/v1alpha1`, bentuk paket provider di `providers/<nama>/`, loader yang hanya membaca file, validasi ketat yang menolak apa pun yang tidak dikenal, interpolasi `${...}` yang terkontrol (bukan shell), renderer template kecil buatan sendiri dengan escape per format, dan execution plan (rencana eksekusi berupa data murni) yang nanti dijalankan supervisor di spec 0010. Tujuannya dua: tool baru yang cukup dengan primitive (langkah aman bawaan engine) yang ada tidak perlu patch core atau GUI (RISK-07), dan manifest tidak pernah bisa menjadi jalan untuk menjalankan kode sembarangan (RISK-08). Fitur ini juga membawa paket contoh NATS, Traefik dengan daftar routes, dan fake provider dengan dummy server untuk test, generator `versions.json` dari inventory spec 0003, JSON Schema untuk editor, dan fuzz target untuk parser manifest dan template.

## Requirements

**User stories**:

- Sebagai kontributor provider, saya ingin menulis manifest, settings schema, dan template saja, lalu tahu dari validasi (bukan saat start) apa yang salah, supaya tool baru tidak butuh patch Rust core atau SwiftUI.
- Sebagai developer supervisor (fitur 10) dan walking skeleton (fitur 14), saya ingin satu tipe plan yang sudah tervalidasi dan satu fake provider yang bisa delay ready, crash, dan spam log, supaya saya bisa menjalankan proses tanpa menafsirkan manifest sendiri.
- Sebagai pengguna, saya ingin katalog provider dibaca tanpa menjalankan kode apa pun, dan nilai settings saya tidak pernah diteruskan lewat shell, supaya membuka catalog atau mengisi form tidak bisa menjalankan perintah tersembunyi.
- Sebagai developer form create dan edit config (fitur 20 dan 29), saya ingin model field bertipe dengan kelas VAL-012, supaya form dirender dari schema tanpa kode khusus per provider.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-031):

- **AC-1**: `read_package(fs, dir)` membaca paket hanya lewat port `Filesystem` (`symlink_metadata`, `read_dir`, `read_file_limited`) menjadi `PackageFiles` di memori, dan `ValidatedPackage::from_files(files, name_check)` memvalidasinya tanpa IO. Setelah ketiga paket contoh disalin ke `TestRoot` lalu dibaca lewat `FakePlatform`, `mutations()` tidak bertambah selama `read_package`, dan tidak ada parameter di kedua API itu yang bisa menjalankan proses atau membaca secret. Isi folder mengikuti *Paket provider*: file wajib yang tidak ada (`missing_file`), entri yang tidak diizinkan, symlink di mana pun, file yang bukan file biasa, file di atas batas ukuran, lebih dari 32 template, file yang bukan UTF-8 atau berawalan BOM, dan template yang tidak dirujuk masing masing ditolak; `.DS_Store` dilewati, dan `tests/`, `README.md`, serta `LICENSE-NOTICES.md` tidak pernah dibaca (REQ-029, PRD bagian 21.4).
- **AC-2**: Dokumen didecode dua tahap: `apiVersion` dan `kind` lebih dulu. `apiVersion` selain `stackly/v1alpha1` menghasilkan `unsupported_api_version` tanpa mendecode sisa dokumen, dan `kind` selain `Provider` menghasilkan `unknown_kind`. Field yang tidak dikenal di mana pun di `provider.yaml`, settings schema, atau `versions.json` (termasuk di `permissions`) ditolak dengan `unknown_field` yang menyebut file dan path field; tag YAML, merge key, dan key ganda juga ditolak. Decode struktural menghasilkan paling banyak satu issue per file (serde berhenti di error pertama); aturan semantik setelahnya mengumpulkan semua masalah sampai 100, lalu berhenti dengan `too_many_issues`. File YAML 256 KiB berisi alias bersarang (billion laughs) ditolak dalam 2 detik (VAL-018).
- **AC-3**: `metadata.id` berbentuk `<namespace>/<nama>` (`ProviderName`), `metadata.revision` adalah `RevisionLabel`, dan nilai kosong atau yang memuat `..` atau `/` di luar pemisah namespace ditolak. Dengan `NameCheck::FolderName`, bagian nama dari `id` harus sama dengan nama folder paket (`directory_name_mismatch`); salinan pinned di `<data>/providers/` memakai `NameCheck::Skip` karena identitasnya dari path layout. `category: testing` dipakai jika dan hanya jika namespace nya `testing`. `metadata.software` sama dengan `software` di `versions.json` (VAL-002).
- **AC-4**: Primitive di `lifecycle.initialize`, strategi stop, sinyal, aksi reload, jenis probe, dan format template adalah himpunan tertutup. Nilai yang tidak dikenal ditolak saat validasi dengan lokasinya (`unknown_primitive`, `unknown_probe`, `unknown_template_format`, `invalid_value`), tidak pernah ditunda sampai start. Jenis probe di fitur ini hanya `tcp`, `http`, dan `exec`; `type: native` ditolak sebagai `unknown_probe` sampai fitur 23 menambahkannya secara additive (REQ-029, RISK-08).
- **AC-5**: `capabilities` dipetakan ke `Capability` domain. Capability yang tidak bisa dipenuhi ditolak dengan `capability_unsatisfiable`: `reload` tanpa `lifecycle.reload` (dan `lifecycle.reload` tanpa `reload` ditolak dengan `reload_without_capability`), field settings `apply: reload` tanpa capability `reload`, `metrics` tanpa endpoint ber purpose `monitoring`, backup `cold` tanpa `persistence` atau tanpa `backup.cold.paths`, path backup di luar `permissions.managedFilesystem`, serta `tls`, backup `online`, dan backup `logical` yang belum punya primitive di v1alpha1. `ValidatedPackage::provider_revision()` mengembalikan `ProviderRevision` domain dengan `CapabilitySet` dan `LineCompatibility` dari manifest.
- **AC-6**: Nama endpoint mengikuti aturan *Nama dan charset*, paling banyak 16 endpoint, dengan transport `tcp`, `udp`, atau `unix` dan purpose `client`, `monitoring`, atau `admin`. `defaultHost` selain `127.0.0.1` atau `::1` ditolak dengan `endpoint_not_loopback`. Teks literal di `socketName` hanya boleh memakai charset `SocketName` (`[A-Za-z0-9._-]`), hasilnya bukan `.` atau `..`, dan panjangnya dihitung dengan setiap token port bernilai 5 byte lalu ditolak dengan `socket_name_too_long` di atas 15 byte (`SOCKET_NAME_MAX` spec 0004). Rujukan ke endpoint yang tidak ada dari probe, template, token, atau `socketName` ditolak (`unknown_endpoint`), begitu juga probe `tcp` atau `http` ke endpoint `udp`.
- **AC-7**: Interpolasi mengikuti *Interpolasi terkontrol* dan matriks posisinya: token yang tidak dikenal ditolak (`unknown_token`); token di posisi yang tidak menerima tipenya ditolak (`token_not_allowed_here`); field executable hanya menerima tepat satu token executable, dan path literal seperti `/bin/sh` ditolak (`literal_executable`); token `secret.*` hanya di `stdin`, `materializeSecret`, nilai `env` command `lifecycle.start` dan langkah `runExecutable` yang berisi tepat satu token secret tanpa teks lain, dan template config yang `sensitive` (`secret_not_allowed_here`); entri `args` yang diawali token settings teks tanpa `enum`, atau token settings integer yang boleh negatif, ditolak (`argument_injection_risk`); setiap token folder di posisi mana pun wajib tercantum di `permissions.managedFilesystem` (`path_not_permitted`); sufiks path hanya berisi komponen literal dengan charset komponen, komponen `..` ditolak, dan hasil path tetap di bawah folder token itu (`path_escapes_scope`); key env mengikuti aturannya dan tidak bisa menimpa `PATH`, `HOME`, atau `TMPDIR`; `$${` menghasilkan `${` literal (REQ-031, VAL-019).
- **AC-8**: Settings schema memakai dialek terbatas di *Settings schema*: keyword di luar daftar ditolak, `additionalProperties: false` wajib di root dan di item array, `title` wajib, `x-stackly.apply` wajib di setiap field root yang tidak `managed`, batas wajib ada (`maxLength` kecuali ada `enum`, `minimum` dan `maximum` untuk integer, `maxItems` paling banyak 256), paling banyak 64 field root dan 16 field item, `pattern` paling panjang 512 karakter dan selalu dicocokkan penuh (dibungkus `\A(?:…)\z`, dikompilasi crate `regex` dengan batas ukuran), `default` harus lolos batas field nya sendiri, nilai `enum` tidak diawali `-`, field `managed` tidak boleh di `required` atau punya `default`, dan field secret wajib `required` tanpa `default`, `enum`, atau `pattern`. Field `apply: reload` hanya boleh dirujuk dari template milik config file yang ada di `lifecycle.reload.render`. Model field memuat keenam kelas VAL-012. `SettingsSchema::validate` menolak key yang tidak dikenal (`unknown_setting`), field `managed` yang diisi pengguna, tipe yang salah, integer yang bukan literal bulat, nilai di luar batas, pattern yang tidak cocok, teks yang memuat karakter kontrol, dan key item yang ganda, lalu mengisi default; setelahnya setiap field bukan secret punya nilai (VAL-012, VAL-019).
- **AC-9**: Template mengikuti *Template config*. Cek statis saat validasi menolak variabel yang tidak dikenal, variabel loop yang menimpa namespace, output yang bukan skalar, `#each` pada yang bukan array, `#if` pada yang bukan boolean atau array, dan `secret.*` di template yang tidak `sensitive`, dengan nama template, baris, dan kolom. Setiap output melewati escaper format `yaml`, `json`, atau `natsConf`, tanpa mode raw. `natsConf` menolak nilai teks yang memuat `"`, `\`, `$`, atau karakter kontrol dengan `unrepresentable_value` yang menyebut field, tidak pernah nilainya. Baris yang hanya berisi tag blok dihapus utuh. Render diputus begitu output melewati 1 MiB, dan blok bersarang lebih dari 4 ditolak. Output `yaml` dan `json` diparse ulang (render contoh dengan nilai default saat validasi, render nyata saat plan) dan hasil yang bukan dokumen valid ditolak dengan `rendered_output_invalid`. Hasil render ketiga paket contoh sama byte demi byte dengan file golden.
- **AC-10**: Blok `timeouts` mengikuti tabel batas dan default di *Timeout dan reload*; nilai di luar batas ditolak dengan `timeout_out_of_range`. Setiap timeout langkah dan probe punya sumber di tabel *Sumber timeout dan exit code*: `timeoutSeconds` langkah `runExecutable` tidak boleh melebihi `initSeconds`, dan `attemptTimeoutMilliseconds` tidak boleh melebihi deadline fasenya. `lifecycle.reload` hanya merujuk `configFiles` yang ada (`unknown_config_file`), aksinya `signal` (`hup` saja), `run`, atau `none`, dan `verify` bernilai `readiness` atau `none`. Interval probe, timeout per percobaan, dan `failureThreshold` health mengikuti batasnya.
- **AC-11**: `init_plan`, `run_plan`, dan `reload_plan` adalah fungsi murni (tanpa IO) dari `ValidatedPackage` dan `PlanContext`. Setiap entri `args` menjadi tepat satu elemen argv tanpa perubahan, termasuk spasi, Unicode, `;`, `$(...)`, backtick, kutip, `|`, `&`, dan `>` (AC-031, AC-058). Program selalu `ExecutablePath` di dalam root runtime, tidak pernah shell. Env child adalah basis (`PATH`, `HOME`, `TMPDIR`) ditambah env manifest, tanpa warisan dari daemon. Semua path absolut, normal, dan di bawah folder instance yang diizinkan. Secret hanya muncul sebagai `PlanSecret { field, reference }`, dan `Debug` serta `Serialize` plan menampilkan `<secret:field>`. Error plan hanya yang ada di daftar `PlanError`: `reload_plan` untuk paket tanpa capability `reload` mengembalikan `Unsupported` (BR-016), endpoint `tcp` atau `udp` tanpa port konkret menghasilkan `EndpointUnresolved`, host endpoint yang bukan loopback menghasilkan `EndpointNotLoopback`, dan entri argv di atas 4096 byte menghasilkan `ArgumentTooLong`. `RenderedContent::materialize` menghasilkan byte final dengan escaper dan batas yang sama.
- **AC-12**: `manifest_sha256` adalah SHA-256 atas daftar baris `<sha256>  <path>` yang diurutkan, untuk `provider.yaml`, `schemas/settings.schema.json`, dan setiap template yang dirujuk. Mengubah satu byte template mengubah digest; mengubah `versions.json`, `README.md`, atau isi `tests/` tidak. Byte yang di hash adalah byte yang sama dengan yang diparse.
- **AC-13**: `cargo xtask codegen` menulis `providers/<software>/versions.json` dari `runtimes/inventory.toml` untuk setiap paket yang software nya punya entri inventory, dengan bentuk output di *Generator*, dan `cargo xtask codegen --check` gagal kalau file itu drift. Loader menolak `versions.json` yang melanggar aturan di *`versions.json`* (`versions_rule`, `fixture_outside_testing`), artefak untuk target di luar `compatibility.platforms`, serta `runtime.executable` atau path `additionalExecutables` yang tidak ada di `executables` setiap artefak `available` dan `fixture` (`executable_not_in_artifact`). Penanda `// provisional: F9` di `xtask/src/runtime/inventory.rs` dihapus.
- **AC-14**: Paket `providers/nats`, `providers/traefik` (dengan `settings.routes` berbentuk daftar yang dirender ke file provider dinamis), dan `providers/fake` lolos `cargo xtask provider check` di Mac ini (exit 0, satu baris `ok <id> <revision> <manifest_sha256>` per paket) dan lolos integration test core. Paket fixture `testing/fake-command` di `crates/core/tests/fixtures/packages/` lolos validasi dan memakai primitive yang tidak dipakai paket fake (`materializeSecret`, probe `tcp` dan `exec`, stop `command`, reload `run`). Salinan rusak dari tiap paket (primitive yang tidak dikenal) membuat perintah itu exit 1 dengan satu baris per masalah (REQ-029, RISK-07, bagian manifest dari AC-029).
- **AC-15**: `stackly-fake-server` (crate `crates/fake-server`; dependency `signal-hook`, `serde_json`, dan `sha2`; dev dependency `stackly-testkit`) menyediakan `version`, `init`, `serve`, dan `shutdown` dengan perilaku di *Fake provider dan dummy server*: delay ready, crash dengan exit code tertentu, spam log, abaikan SIGTERM, reload config saat SIGHUP, dan menulis argv yang diterima ke folder data. `stackly_testkit::fake_server::binary()` membangun binary itu sekali per proses test dan mengembalikan path nya (PRD bagian 32.3).
- **AC-16**: Empat fuzz target (`provider_manifest`, `provider_template`, `provider_interpolation`, `provider_settings_schema`) dengan corpus awal dari ketiga paket contoh masing masing berjalan 10 menit di Mac ini tanpa crash, hang, atau kehabisan memori (NFR-009, RISK-08).
- **AC-17**: `cargo xtask codegen` menulis `schemas/provider/provider.v1alpha1.schema.json`, `schemas/provider/settings.v1alpha1.schema.json`, dan `schemas/provider/versions.v1.schema.json` dari tipe Rust lewat schemars, setiap object di dalamnya `additionalProperties: false`, field nama dan versi membawa pattern yang sama dengan aturan domain, dan `codegen --check` gagal kalau ketiganya drift.
- **AC-18**: `preflight(recorded, marker, runtime_line)` mengembalikan `Compatible` hanya kalau line data (dari registry, atau dari `instance-manifest.json` kalau registry tidak punya catatan, dan dari file penanda data bila manifest mendeklarasikan `compatibility.dataMarker`) cocok satu sama lain dan boleh dijalankan runtime line itu menurut `LineCompatibility`. Selain itu hasilnya `Incompatible { data, runtime }`, `MarkerMismatch { recorded, marker }`, `MarkerMissing`, `MarkerUnreadable`, atau `Unknown` (BR-009).
- **AC-19**: Setiap varian `PackageError`, `SettingsError`, dan `PlanError` punya kode, kategori, `details`, dan `ItemSpec` persis seperti tabel *Kode error di wire* di katalog `stackly-protocol::codes::manifest` dan `codes::provider`, dengan test pemetaan per varian di `stacklyd`; `ResourceRef` provider memakai bentuk teks di tabel itu dan bolak balik lewat parser nya (spec 0006 AC-8).

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md): manifest `stackly/v1alpha1` yang ketat di semua bagian, divalidasi dengan decode serde bertipe plus fungsi aturan tanpa validator JSON Schema generik, renderer template tanpa logika buatan sendiri dengan escape per format, probe generik `tcp`, `http`, dan `exec` dengan registry probe native tertutup di core yang dibangun fitur provider pertama yang butuh (fitur 23), plan per aksi sebagai data murni di `stackly-core::provider::plan`, `versions.json` yang dibangkitkan dari inventory, dan fake provider di `providers/fake` dengan dummy server di `crates/fake-server`.

Engine provider hanya membaca, memvalidasi, dan menyusun rencana. Ia tidak pernah menjalankan proses, menulis file, atau menyelesaikan secret; semua efek samping terjadi di executor spec 0010 dari plan yang sudah tervalidasi.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya deserialisasi ketat dengan `deny_unknown_fields`, error yang dikumpulkan sebagai daftar issue, dan larangan `unwrap()` di luar test.

## Feature design

### Peta modul dan dependency

Semua aturan arah dependency spec 0001 dan 0004 tetap berlaku. Engine adalah modul di `stackly-core`, sesuai spec 0001 (modul diekstrak menjadi crate saat batasnya terbukti).

| Crate | Dependency langsung yang baru | Isi baru |
| --- | --- | --- |
| `stackly-core` | `serde` (derive), `serde_yaml_ng`, `serde_path_to_error`, `regex`, `sha2`; `schemars` opsional di balik fitur `json-schema` (mati secara default) | modul `provider` dengan submodul `package`, `raw` (tipe decode mentah), `issue`, `names`, `interpolate`, `settings`, `template`, `probe`, `validate`, `digest`, `versions`, `plan`, `preflight`, `secret` (sementara) |
| `stackly-fake-server` (`crates/fake-server`, bin `stackly-fake-server`, `publish = false`) | `signal-hook`, `serde_json`, `sha2`; dev: `stackly-testkit` | dummy server untuk fake provider |
| `stackly-testkit` | tidak ada | modul `fake_server` (membangun dan menemukan binary dummy) |
| `xtask` | `stackly-core` dengan fitur `json-schema`; `stackly-platform-macos` di bawah `cfg(target_os = "macos")` | `provider check` (memakai `MacosFilesystem`, macOS saja), generator `versions.json` dan JSON Schema di `codegen` |
| fuzz crate (lokasi dan toolchain dari spec 0006) | `stackly-core` | empat target di *Fuzz target* |

Aturan baru di `cargo xtask check-deps`: `stackly-fake-server` tidak punya dependency normal ke crate Stackly mana pun (hanya `stackly-testkit` sebagai dev dependency), dan tidak ada crate yang bergantung padanya. `[workspace.dependencies]` mendapat `serde_yaml_ng`, `serde_path_to_error`, `regex`, dan `signal-hook`.

Selama spec 0011 belum dibangun, `core::provider::secret::SecretRef` adalah newtype teks opaque dengan `FromStr` (1 sampai 256 byte ASCII tercetak), dan `core::provider::secret::ResolvedSecrets` adalah peta field ke byte dengan konstruktor publik untuk test; keduanya ditandai `// provisional: F11` dan diganti tipe milik spec 0011 (yang untuk test memakai `ResolvedSecrets::for_tests` di balik fitur `test-support`).

### Paket provider

```text
providers/<nama>/                 # nama = bagian nama dari metadata.id
├── provider.yaml                 # wajib, ≤ 256 KiB
├── versions.json                 # wajib, ≤ 1 MiB; dibangkitkan codegen kecuali paket testing/
├── schemas/
│   └── settings.schema.json      # wajib, ≤ 128 KiB, satu satunya isi schemas/
├── templates/
│   └── *.tmpl                    # nama ^[a-z0-9][a-z0-9._-]{0,63}\.tmpl$, ≤ 256 KiB, paling banyak 32, tanpa subfolder
├── tests/                        # tidak dibaca loader (fixture harness fitur 36)
├── README.md                     # opsional, tidak dibaca
└── LICENSE-NOTICES.md            # opsional, tidak dibaca
```

- `read_package(fs, dir) -> Result<PackageFiles, PackageError>`: folder paket sendiri harus folder, bukan symlink. Loader berjalan dengan `read_dir` tanpa mengikuti symlink; symlink di mana pun ditolak (`symlink_not_allowed`), begitu juga FIFO, device, atau socket (`not_a_regular_file`). Entri lain di root, di `schemas/`, atau di `templates/` ditolak (`unexpected_entry`), kecuali `.DS_Store` yang dilewati. File wajib yang tidak ada menghasilkan `missing_file`. Total byte yang dibaca paling banyak 4 MiB. Semua file harus UTF-8 tanpa BOM (`not_utf8`).
- `PackageFiles` adalah peta path relatif ke byte untuk `provider.yaml`, `versions.json`, `schemas/settings.schema.json`, dan setiap file di `templates/`, ditambah nama folder. `ValidatedPackage::from_files(files, name_check)` memvalidasi tanpa IO; fuzz target dan test memanggilnya langsung. `NameCheck::FolderName` dipakai untuk paket di repo dan di catalog bawaan; `NameCheck::Skip` untuk salinan pinned di `<data>/providers/<namespace>/<name>/<revision>/<manifest-sha256>/`, yang identitasnya dicek pemanggil lewat digest.
- `ValidatedPackage::files()` mengembalikan `PackageFiles` yang sama, untuk disalin saat pin (fitur 17 dan 20) dan dibaca ulang dengan hasil digest yang sama.
- Template yang tidak dirujuk `configFiles` ditolak (`unreferenced_template`), supaya digest tetap sama dengan perilaku.
- Nama file tetap, jadi manifest tidak punya field `versionCatalog` atau `settingsSchema` seperti contoh PRD 21.2.

### Nama dan charset

| Jenis | Aturan |
| --- | --- |
| Segmen token (interpolasi dan template) | `^[a-zA-Z][a-zA-Z0-9]{0,63}$` |
| Nama endpoint, key settings, field item, nama `configFiles`, alias executable, variabel loop | `^[a-z][a-zA-Z0-9]{0,31}$`; endpoint juga harus valid sebagai `EndpointName` domain |
| Variabel loop | tidak boleh sama dengan `instance`, `runtime`, `endpoint`, `settings`, `secret`, `provider`, atau `loop` |
| Komponen path literal (sufiks `destination`, `logs.files`, `cwd`, nama file `materializeSecret`, `dataMarker.file`, path di `runtime.executable` dan `additionalExecutables`) | `^[A-Za-z0-9._-]{1,64}$`, bukan `.` atau `..` |
| Teks literal di `socketName` | charset `SocketName` (`[A-Za-z0-9._-]`) |

### Manifest `stackly/v1alpha1`

Decode memakai tipe mentah di `core::provider::raw` dengan `deny_unknown_fields` di setiap object, kunci YAML camelCase, dan `serde_path_to_error` untuk path field. Field nama, versi, enum, dan tag disimpan sebagai string atau angka mentah, lalu diubah ke tipe domain (`ProviderName`, `RevisionLabel`, `SoftwareName`, `RuntimeLine`, `EndpointName`, `Port`, `Os`, `Arch`) dan enum tertutup di pass semantik, sehingga nilai yang salah menghasilkan kode spesifik dengan path nya. Tipe mentah membawa atribut schemars dengan pattern yang sama dengan aturan domain.

| Field | Tipe | Wajib | Aturan |
| --- | --- | --- | --- |
| `apiVersion` | string | ya | persis `stackly/v1alpha1`, dicek sebelum field lain |
| `kind` | string | ya | persis `Provider` |
| `metadata.id` | `ProviderName` | ya | VAL-002; nama = nama folder dengan `NameCheck::FolderName` |
| `metadata.software` | `SoftwareName` | ya | sama dengan `versions.json` |
| `metadata.displayName` | string | ya | 1 sampai 64 karakter, tanpa karakter kontrol |
| `metadata.revision` | `RevisionLabel` | ya | VAL-002 |
| `metadata.category` | enum | ya | `database`, `messaging`, `cache`, `proxy`, `analytics`, `testing` |
| `metadata.description` | string | tidak | paling banyak 512 karakter, tanpa karakter kontrol |
| `compatibility.platforms` | daftar `{ os, arch }` | ya | minimal 1, unik, kode domain (`macos`, `aarch64`) |
| `compatibility.lineCompatibility` | peta `RuntimeLine` ke daftar `RuntimeLine` | tidak | line runtime tambahan yang boleh menjalankan data line itu; line yang sama selalu boleh; kosong berarti BR-009 ketat |
| `compatibility.dataMarker` | `{ file, pattern }` | tidak | `file` satu komponen di folder data (misalnya `PG_VERSION`); `pattern` dengan aturan pattern settings dan tepat satu capture group yang menghasilkan `RuntimeLine` |
| `runtime.executable` | path relatif | ya | komponen literal, tanpa `..`; ada di `executables` artefak |
| `runtime.executionMode` | enum | ya | `foreground` saja |
| `capabilities` | object | tidak | `reload`, `metrics`, `tls`, `multiInstance`, `persistence` (boolean, default false), `backup` (daftar `cold`, `online`, `logical`, default kosong) |
| `resources.endpoints` | peta nama ke Endpoint | ya | 1 sampai 16 |
| `configFiles` | peta nama ke ConfigFile | tidak | paling banyak 16 |
| `lifecycle.initialize` | daftar langkah | tidak | paling banyak 16 |
| `lifecycle.start` | Command | ya | |
| `lifecycle.stop` | Stop | ya | |
| `lifecycle.reload` | Reload | jika dan hanya jika `capabilities.reload` | |
| `readiness` | ReadinessBlock | ya | |
| `health` | HealthBlock | tidak | default memakai probe readiness |
| `timeouts` | Timeouts | tidak | semua punya default |
| `logs.files` | daftar string | tidak | paling banyak 8, `${instance.logs}` ditambah komponen literal; `logs` wajib di `managedFilesystem` |
| `backup.cold.paths` | daftar `data`, `config` | jika dan hanya jika backup `cold` | subset `permissions.managedFilesystem` |
| `permissions.managedFilesystem` | daftar `data`, `config`, `logs`, `temp`, `run` | ya | folder instance yang boleh dipakai provider ini (ditulis engine atau dirujuk token) |
| `permissions.executeRuntimeBinary` | boolean | ya | wajib `true` di v1alpha1, sebagai pernyataan eksplisit |
| `permissions.additionalExecutables` | peta alias ke path relatif | tidak | paling banyak 16, path ada di `executables` artefak |

Bentuk bagian:

- **Endpoint**: `transport` (`tcp`, `udp`, `unix`), `purpose` (`client`, `monitoring`, `admin`). `tcp` dan `udp` wajib `defaultHost` (loopback) dan `defaultPort` (1 sampai 65535, saran untuk form; pilihan Fixed atau Auto milik fitur 20). `unix` wajib `socketName` (teks literal dan token `${endpoint.<tcp>.port}`).
- **ConfigFile**: `template` (path `templates/<file>.tmpl`), `format` (`yaml`, `json`, `natsConf`), `destination` (lihat matriks posisi), `sensitive` (boolean, default false; file berisi secret).
- **Command**: `executable` (tepat satu token executable), `args` (daftar string, paling banyak 128), `cwd` (default `${instance.data}`), `env` (peta, paling banyak 32 key `^[A-Z][A-Z0-9_]{0,63}$`, bukan `PATH`, `HOME`, `TMPDIR`, dan tidak diawali `DYLD_` atau `LD_`; nilai boleh berupa tepat satu token `${secret.<field>}` tanpa teks lain, yaitu mekanisme `env` spec 0011, tetapi hanya di command `lifecycle.start` dan langkah `runExecutable` initialize, karena hanya kedua aksi itu yang menyelesaikan secret; probe `exec`, stop `command`, dan reload `run` ditolak).
- **Langkah initialize** (`primitive` plus parameter):
  - `createManagedDirectories`: tanpa parameter; memastikan setiap folder di `permissions.managedFilesystem` ada.
  - `renderConfig`: `file` (nama di `configFiles`, `unknown_config_file` kalau tidak ada).
  - `materializeSecret`: `secret` (nama field secret, `unknown_secret` kalau tidak ada), `destination` (`${instance.run}/secrets/` ditambah satu komponen literal, folder secret milik spec 0011 di runtime root); file 0600 berisi byte secret persis, dihapus executor di akhir aksi init, berhasil atau gagal. `${instance.temp}` tidak diterima, karena berada di root data yang ikut backup dan Time Machine (spec 0011).
  - `runExecutable`: Command ditambah `stdin` (`{ secret: <nama> }`, opsional; byte secret persis tanpa newline tambahan, lalu stdin ditutup), `timeoutSeconds` (opsional, paling banyak `initSeconds`), `successExitCodes` (default `[0]`, 1 sampai 8 nilai 0 sampai 255).
- **Stop**: `strategy: gracefulProcess` dengan `signal` `term` (default), `int`, atau `quit`; atau `strategy: command` dengan `command` (Command) dan `successExitCodes` (default `[0]`), lalu menunggu proses utama keluar. Tidak ada strategi kill.
- **Reload**: `render` (daftar nama `configFiles`, paling banyak 16, boleh kosong), `action` (`signal` dengan `signal: hup`, `run` dengan `command` dan `successExitCodes` default `[0]`, atau `none` untuk provider yang mengawasi file), `verify` (`readiness` default, atau `none`).
- **Probe**: `type: tcp` dengan `endpoint` (`tcp` atau `unix`); `type: http` dengan `endpoint` (`tcp`), `path` (literal, diawali `/`, paling banyak 256 byte ASCII tercetak tanpa spasi), `expectStatus` (`{ min, max }` di 100 sampai 599, default 200 sampai 299); `type: exec` dengan `command` (Command, tanpa stdin) dan `successExitCodes` (default `[0]`).
- **ReadinessBlock**: `probe`, `intervalMilliseconds` (100 sampai 10000, default 500; jeda setelah satu percobaan selesai sebelum percobaan berikutnya), `attemptTimeoutMilliseconds` (100 sampai 30000, default 2000; tidak boleh melebihi deadline fase yang memakainya).
- **HealthBlock**: `probe` (opsional), `intervalSeconds` (1 sampai 300, default 10), `attemptTimeoutMilliseconds` (100 sampai 30000, default 2000), `failureThreshold` (1 sampai 10, default 3). Kegagalan berturut turut di bawah ambang berarti `degraded`, mencapai ambang berarti `unhealthy`, sukses berarti `healthy`.

Contoh (paket NATS di fitur ini, disingkat):

```yaml
apiVersion: stackly/v1alpha1
kind: Provider
metadata:
  id: official/nats
  software: nats
  displayName: NATS Server
  revision: "1.0.0"
  category: messaging
compatibility:
  platforms:
    - { os: macos, arch: aarch64 }
runtime:
  executable: nats-server
  executionMode: foreground
capabilities:
  multiInstance: true
  persistence: true
  metrics: true
  backup: [cold]
resources:
  endpoints:
    client: { transport: tcp, purpose: client, defaultHost: "127.0.0.1", defaultPort: 4222 }
    monitoring: { transport: tcp, purpose: monitoring, defaultHost: "127.0.0.1", defaultPort: 8222 }
configFiles:
  natsConf:
    template: templates/nats.conf.tmpl
    format: natsConf
    destination: "${instance.config}/nats.conf"
lifecycle:
  initialize:
    - { primitive: createManagedDirectories }
    - { primitive: renderConfig, file: natsConf }
  start:
    executable: "${runtime.executable}"
    args: ["--config", "${instance.config}/nats.conf"]
    cwd: "${instance.data}"
  stop: { strategy: gracefulProcess, signal: term }
readiness:
  probe: { type: http, endpoint: monitoring, path: /healthz }
timeouts: { readinessSeconds: 30, stopSeconds: 30 }
backup:
  cold: { paths: [data] }
permissions:
  managedFilesystem: [data, config, logs, temp]
  executeRuntimeBinary: true
```

### Interpolasi terkontrol

Grammar: `${` path `}`, dengan path berupa segmen token yang dipisah titik. Tidak ada fungsi, default, ekspresi, atau token bersarang. `$${` menghasilkan `${` literal; `$` lain tetap literal. Token yang tidak tertutup ditolak. Satu string boleh memuat beberapa token dan teks literal; hasilnya tetap satu string.

| Token | Tipe | Nilai dan bentuk teks |
| --- | --- | --- |
| `instance.id`, `instance.name`, `instance.shortKey` | teks | `InstanceId` lowercase, `InstanceName`, `ShortKey` |
| `instance.data`, `instance.config`, `instance.logs`, `instance.temp`, `instance.run` | folder (jenisnya sesuai nama) | `InstancePaths` dan `instance_run_dir(short key)` dari spec 0004 |
| `runtime.root` | folder runtime | root runtime canonical |
| `runtime.executable` | executable | root runtime ditambah `runtime.executable` |
| `runtime.executables.<alias>` | executable | root runtime ditambah path alias di `additionalExecutables` |
| `runtime.version`, `runtime.line` | teks | `ExactVersion`, `RuntimeLine` |
| `endpoint.<n>.host` | teks | IP; IPv4 desimal bertitik, IPv6 bentuk ringkas huruf kecil tanpa kurung |
| `endpoint.<n>.port` | integer | port konkret (tcp, udp), desimal |
| `endpoint.<n>.address` | teks | `host:port`, IPv6 sebagai `[::1]:port` |
| `endpoint.<n>.socket`, `endpoint.<n>.socketDir` | file dan folder di bawah `run` | folder run ditambah `socketName` (unix); `run` wajib di `managedFilesystem` |
| `settings.<field>` | teks, integer, atau boolean sesuai field (field array hanya lewat `#each` di template) | nilai tervalidasi atau nilai `managed` turunan; integer desimal, boolean `true` atau `false` |
| `secret.<field>` | secret | `PlanSecret` field secret; `settings.<field>` untuk field secret ditolak |
| `provider.id`, `provider.revision` | teks | manifest |

Matriks posisi (ya berarti tipe itu diterima di posisi itu):

| Posisi | Teks dan integer | Boolean | Folder | Root runtime | Executable | Secret | Aturan tambahan |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `executable` di Command | tidak | tidak | tidak | tidak | tepat satu, tanpa teks lain | tidak | `literal_executable` untuk selain itu |
| entri `args` | ya | ya | ya | ya | ya | tidak | aturan `argument_injection_risk`; satu entri satu elemen argv |
| nilai `env` | ya | ya | ya | ya | ya | hanya sebagai seluruh nilai, satu token, dan hanya di `lifecycle.start` serta `runExecutable` | key mengikuti aturan Command |
| `cwd` | tidak | tidak | satu token di awal | tidak | tidak | tidak | sufiks hanya komponen literal |
| `configFiles.*.destination` | tidak | tidak | satu token di awal | tidak | tidak | tidak | lalu satu atau dua komponen literal; folder perantara dibuat plan |
| `materializeSecret.destination` | tidak | tidak | `instance.run` di awal, diikuti komponen `secrets` | tidak | tidak | tidak | lalu satu komponen literal |
| `logs.files` | tidak | tidak | `instance.logs` di awal | tidak | tidak | tidak | lalu komponen literal |
| `socketName` | hanya `endpoint.<tcp>.port` | tidak | tidak | tidak | tidak | tidak | literal charset `SocketName` |
| `http.path` | tidak | tidak | tidak | tidak | tidak | tidak | literal saja |
| `stdin` | tidak | tidak | tidak | tidak | tidak | `{ secret: <nama> }` | |
| ekspresi `managed` di settings schema | `instance.*`, `runtime.version`, `runtime.line`, `endpoint.*`, `provider.*` | tidak | ya | tidak | tidak | tidak | tidak boleh `settings.*` atau `secret.*` (mencegah siklus) |
| `{{ }}` di template | ya | ya | ya (sebagai teks) | ya (sebagai teks) | tidak | hanya di config `sensitive` | |

- Setiap token folder di posisi mana pun (termasuk `args`, `env`, dan template) wajib tercantum di `permissions.managedFilesystem`.
- `argument_injection_risk`: entri `args` yang diawali token `settings` teks tanpa `enum`, atau token `settings` integer dengan `minimum` negatif, ditolak, supaya nilai pengguna tidak bisa menjadi flag baru. Field `managed` tidak terkena aturan ini karena nilainya tidak datang dari pengguna.
- Path dinormalisasi secara leksikal: komponen `.` dibuang, komponen `..` ditolak, garis miring ganda digabung. Containment dicek terhadap folder yang namanya disebut token, jadi `${instance.config}/../data/x` ditolak walau hasilnya masih di dalam instance.

### Settings schema

`schemas/settings.schema.json` adalah dialek terbatas JSON Schema 2020-12 yang diparse serde menjadi model field bertipe (`SettingsSchema`). Model ini yang dirender form fitur 20 dan 29; pattern dievaluasi daemon, bukan klien Swift, dan form menampilkan error per field dari daemon.

- Root: `$schema` (persis `https://json-schema.org/draft/2020-12/schema`), `title`, `description`, `type: object`, `additionalProperties: false`, `properties` (paling banyak 64 field), `required`. Keyword lain ditolak (`settings_schema_keyword`).
- Field root: `type` (`string`, `integer`, `boolean`, `array`), `title` (wajib, paling banyak 64 karakter), `description` (paling banyak 512), `default`, `enum` (string saja, 1 sampai 64 nilai unik, masing masing paling banyak 128 karakter, tidak diawali `-`), `minLength`, `maxLength` (dalam karakter Unicode; wajib kecuali ada `enum`, paling banyak 4096), `pattern`, `minimum` dan `maximum` (integer, keduanya wajib, di antara minus 2^53 dan 2^53), `items`, `minItems` (opsional, default 0), `maxItems` (array, wajib, paling banyak 256), `x-stackly`.
- Item array: `type: object`, `additionalProperties: false`, `properties` berisi paling banyak 16 field skalar (keyword sama dengan field root kecuali `items`, `minItems`, `maxItems`, dan `x-stackly`), `required`. Setiap field item wajib `required` atau punya `default`. Tidak ada object bersarang lain.
- `x-stackly` di field root: `apply` (`reload`, `restart`, `immutableAfterInit`; wajib kecuali `managed`), `visibility` (`basic` default, `advanced`), `secret` (boolean, hanya `type: string`), `managed` (string interpolasi menurut matriks posisi; field dihitung Stackly dan tampil terkunci), `key` (array saja: nama satu field item yang wajib unik antar item).
- Setiap field root bukan secret dan bukan `managed` wajib `required` atau punya `default`. Field `managed` tidak boleh di `required` dan tidak boleh punya `default`. Field secret wajib `required` dan hanya boleh punya `title`, `description`, `minLength`, `maxLength`, dan `x-stackly`. Dua field secret yang purpose turunannya sama menurut algoritma spec 0011 (misalnya `fooBar` dan `fooBAr` sama sama menjadi `foo-bar`) ditolak dengan `secret_purpose_collision`, karena reference daemon `local:instance/<id>/<purpose>` harus unik per instance.
- `default` harus lolos batas field nya sendiri (tipe, `enum`, panjang, pattern, rentang, jumlah item).
- `pattern` paling panjang 512 karakter dan selalu dicocokkan penuh: engine membungkusnya menjadi `\A(?:<pattern>)\z` lalu mengompilasinya dengan `regex::RegexBuilder` dengan `size_limit` 64 KiB (`regex` berjalan dalam waktu linear). `^` dan `$` di dalam pattern tidak dibutuhkan.
- Field `apply: reload` hanya boleh dirujuk dari template milik config file yang tercantum di `lifecycle.reload.render`; rujukan dari `args`, `env`, atau template lain ditolak (`settings_schema_rule`), karena perubahannya tidak akan pernah sampai ke proses lewat reload.
- Kelas VAL-012: managed (`managed`), advanced (`visibility: advanced`), secret (`secret: true`), immutable after init (`apply: immutableAfterInit`), reloadable (`apply: reload`), restart required (`apply: restart`).

`SettingsSchema::validate(&serde_json::Value) -> Result<SettingsValues, SettingsError>`: nilai adalah object; key yang tidak dikenal ditolak (`unknown_setting`, VAL-019); field `managed` yang diisi ditolak; tipe, batas, `enum`, dan `pattern` dicek; integer harus literal JSON bulat (bukan `5.0` atau `1e3`); teks tidak boleh memuat karakter kontrol (U+0000 sampai U+001F, U+007F sampai U+009F); key item yang ditandai `key` harus unik; default diisi. Nilai field secret adalah teks reference yang diparse dengan parser `SecretRef`. Ukuran dokumen nilai paling banyak 256 KiB. `SettingsValues::to_json()` menghasilkan bentuk yang disimpan registry; secret tetap reference. Semua error memakai kode `settings_value` dengan path field, kecuali key asing (`unknown_setting`).

### Template config

Grammar (tanpa ekspresi, tanpa partial, tanpa helper):

```text
{{ path }}                         nilai skalar, selalu di escape sesuai format
{{#each path as item}} ... {{/each}}   path harus array settings; item.<field> di dalamnya
{{#if path}} ... {{else}} ... {{/if}}  path harus boolean, atau array (benar kalau tidak kosong)
{{! komentar }}
\{{                                menghasilkan {{ literal
```

- Path memakai namespace yang sama dengan interpolasi (`instance`, `runtime`, `endpoint`, `settings`, `secret`, `provider`) ditambah variabel loop dan `loop.first` serta `loop.last` di dalam `#each`. Tipe yang boleh dikeluarkan mengikuti baris template di matriks posisi.
- Cek statis berjalan saat validasi, terhadap tipe context dan settings schema, untuk setiap `configFiles` yang memakai template itu (format bisa berbeda per config file).
- Baris yang hanya berisi spasi dan satu tag blok (`#each`, `/each`, `#if`, `else`, `/if`, komentar) dihapus bersama newline nya.
- Batas: blok bersarang paling banyak 4; render diputus begitu output melewati 1 MiB; jumlah item loop dibatasi `maxItems` field nya.
- Output format `yaml` dan `json` diparse ulang dengan `serde_yaml_ng` dan `serde_json`: saat validasi dengan render contoh (nilai default, array kosong dan satu item contoh dari default item, secret diganti teks `x`), dan saat plan dengan nilai nyata. Gagal parse menghasilkan `rendered_output_invalid`. Ini menangkap tag yang ditempel ke teks lain (misalnya `prefix-{{ x }}`) yang membuat scalar berkutip tidak valid.
- Escaper per format, dipakai untuk setiap `{{ }}`:

| Format | Teks | Integer | Boolean |
| --- | --- | --- | --- |
| `yaml` | scalar YAML berkutip ganda; `\` dan `"` di escape, karakter kontrol C0, DEL, C1, U+2028, U+2029, dan U+FEFF sebagai `\uXXXX` | desimal | `true`, `false` |
| `json` | string JSON (serde_json) | desimal | `true`, `false` |
| `natsConf` | berkutip ganda; nilai yang memuat `"`, `\`, `$`, atau karakter kontrol ditolak dengan `unrepresentable_value` | desimal | `true`, `false` |

Format `postgresqlConf`, `valkeyConf`, `mysqlCnf`, dan `xml` ditambah fitur provider 19, 40, 42, dan 43 lewat review, masing masing dengan test terhadap binary aslinya.

Nilai yang ditolak escaper bisa bergantung data (misalnya path home dengan `"`), jadi bisa muncul saat plan dibangun, bukan saat validasi; error nya `UnrepresentableValue` dengan nama field atau token, tanpa nilainya. Secret di template sensitif menjadi segmen `Secret { secret: PlanSecret, format }` di plan, dan `RenderedContent::materialize` memakai escaper yang sama saat executor menulis nilainya.

### Probe

- v1alpha1 di fitur ini membekukan tiga probe generik: `tcp` (connect berhasil dalam timeout percobaan), `http` (GET HTTP/1.1 ke endpoint loopback, tanpa mengikuti redirect, body dibaca paling banyak 64 KiB, sukses kalau status di rentang `expectStatus`), dan `exec` (menjalankan executable runtime dengan argv dan env plan, tanpa stdin, sukses kalau exit code ada di `successExitCodes`).
- Probe native adalah kontrak yang dibangun fitur provider pertama yang membutuhkannya (fitur 23 untuk `nats-readiness`), sebagai perubahan additive: jenis `type: native` dengan `name`, `endpoint`, dan `params`; registry tertutup `NativeProbeDescriptor { name, endpoint_transports, params }` di `stackly-core::provider::probe::native`, dengan `params` berupa field skalar bertipe yang divalidasi seperti item settings; dan runner lewat trait milik spec 0010. Setiap probe native baru ditinjau dan dihitung sebagai ukuran RISK-07. Nama yang tidak ada di registry ditolak saat validasi, tidak pernah saat start.
- Manifest NATS di fitur ini memakai probe `http` ke `/healthz` di endpoint monitoring; fitur 23 menggantinya dengan `nats-readiness`.

### Timeout dan reload

| Field `timeouts` | Default | Batas | Arti |
| --- | --- | --- | --- |
| `initSeconds` | 120 | 1 sampai 1800 | seluruh aksi initialize |
| `readinessSeconds` | 60 | 1 sampai 600 | dari spawn sampai ready saat start |
| `recoverySeconds` | sama dengan `readinessSeconds` | 1 sampai 1800 | dari respawn setelah crash sampai ready (misalnya replay WAL) |
| `stopSeconds` | 30 | 1 sampai 600 | dari aksi stop sampai proses keluar |
| `reloadSeconds` | 30 | 1 sampai 300 | aksi reload sampai verifikasi; hanya boleh ada kalau capability `reload` |

**Sumber timeout dan exit code** (diisi plan builder):

| Posisi di plan | `timeout` | `success_exit_codes` |
| --- | --- | --- |
| `InitStep::Run` | `timeoutSeconds` langkah itu, default `initSeconds` | `successExitCodes`, default `[0]` |
| `InitPlan.deadline` | `initSeconds` | |
| `RunPlan.readiness` | percobaan: `attemptTimeoutMilliseconds`; deadline: `readinessSeconds` | |
| `RunPlan.recovery_deadline` | `recoverySeconds` | |
| `RunPlan.health` | percobaan: `attemptTimeoutMilliseconds` health | |
| `ProbeSpec::Exec` | `attemptTimeoutMilliseconds` blok yang memakainya | `successExitCodes` probe, default `[0]` |
| `StopPlan::Signal` | `stopSeconds` | |
| `StopPlan::Command` | `stopSeconds` untuk perintah dan untuk menunggu proses keluar (satu anggaran bersama) | `successExitCodes` stop, default `[0]` |
| `ReloadPlan.deadline` dan `ReloadAction::Run` | `reloadSeconds` (satu anggaran bersama dengan verifikasi) | `successExitCodes` reload, default `[0]` |
| `ReloadPlan.verify` | interval dan percobaan dari `readiness`; deadline: `reloadSeconds` | |

Apa yang terjadi saat setiap batas lewat, serta backoff, jitter, dan retry budget, adalah keputusan spec 0010. Manifest tidak bisa mengatur restart policy.

### Capability dan aturan pemenuhan

| Key manifest | `Capability` domain | Syarat supaya bisa dipenuhi |
| --- | --- | --- |
| `reload` | `reload` | `lifecycle.reload` ada |
| `metrics` | `metrics` | ada endpoint purpose `monitoring` |
| `multiInstance` | `multi_instance` (baru, Follow-up 0004) | tidak ada |
| `persistence` | `persistence` (baru, Follow-up 0004) | tidak ada |
| `backup: [cold]` | `cold_backup` | `persistence: true` dan `backup.cold.paths` |
| `backup: [online]`, `backup: [logical]` | `online_backup`, `logical_backup` | belum bisa di v1alpha1, selalu ditolak |
| `tls` | `tls` | belum bisa di v1alpha1, selalu ditolak |

`install` dari contoh PRD 21.2 tidak ada: setiap provider memasang runtime dari `versions.json`, dan runtime external milik fitur 56.

### `versions.json`

Schema `stackly.provider-versions/v1`, JSON camelCase, ketat. Nilai enum memakai kode snake_case yang sama dengan inventory.

```json
{
  "schema": "stackly.provider-versions/v1",
  "software": "nats",
  "artifacts": [
    {
      "id": "nats-2.15.1-macos-aarch64",
      "line": "2.15",
      "version": "2.15.1",
      "os": "macos",
      "arch": "aarch64",
      "status": "available",
      "minOs": "<dari inventory>",
      "format": "tar_gz",
      "urls": ["<dari inventory>"],
      "sha256": "<dari inventory>",
      "size": 0,
      "archiveRoot": "nats-server-v2.15.1-darwin-arm64",
      "signingPolicy": "keep_or_adhoc",
      "license": "Apache-2.0",
      "licenseUrl": "<dari inventory>",
      "executables": ["nats-server"]
    }
  ]
}
```

- `status`: `available` wajib `minOs`, `format`, `urls`, `sha256`, `size`, `signingPolicy`, `license`, `licenseUrl`, dan `executables`; `archiveRoot` opsional. `blocked` wajib `blocked: { reason, owner, since }`, boleh membawa `license` dan `licenseUrl`, dan tidak boleh membawa field artefak lain. `fixture` hanya untuk namespace `testing`, wajib `executables`, dan tidak boleh membawa `urls`, `sha256`, `size`, `format`, `minOs`, atau `signingPolicy`.
- Aturan lain (`versions_rule`): `id` unik; pasangan (`version`, `os`, `arch`) unik; `version` diawali `<line>.`; `line` dan `version` valid sebagai `RuntimeLine` dan `ExactVersion`; `executables` berisi path dengan komponen literal.
- Catalog tidak pernah membaca `runtimes/inventory.toml`. Inventory tetap file tooling spec 0003.

**Generator** (`cargo xtask codegen`):

- Untuk setiap folder di `providers/`, generator membaca `provider.yaml` saja, decode ke `serde_yaml_ng::Value`, lalu decode ketat bagian `metadata` untuk mendapat `software`. Kalau inventory punya entri dengan `provider` sama dengan `software`, generator menulis `versions.json` dari entri itu dengan urutan inventory; kalau tidak (misalnya fake), file itu ditulis tangan dan tidak disentuh.
- Pemetaan: `provider` menjadi `software` di root; `id`, `line`, `version`, `os`, `arch`, `status`, `format`, `urls`, `sha256`, `size`, `license`, `executables` disalin; `min_os` menjadi `minOs`, `archive_root` menjadi `archiveRoot`, `signing_policy` menjadi `signingPolicy`, `license_url` menjadi `licenseUrl`; `blocked_reason`, `blocked_owner`, dan `blocked_since` (tanggal TOML ditulis `YYYY-MM-DD`) menjadi blok `blocked`. Field khusus tooling (dependency, checksum dan signature upstream, resep, attestation, origin, distribution, build, sumber maintainer) tidak ikut.
- Output: `serde_json` pretty dengan indent 2 spasi, urutan field mengikuti urutan field struct, field opsional yang kosong tidak ditulis, dan satu newline di akhir file. `--check` membandingkan byte.

### Digest paket

`manifest_sha256` = SHA-256 (crate `sha2`) atas teks UTF-8 berisi satu baris `<sha256 file huruf kecil>  <path relatif>\n` per file, diurutkan berdasarkan byte path, untuk `provider.yaml`, `schemas/settings.schema.json`, dan setiap template yang dirujuk `configFiles` (sekali per file). Setiap file dibaca sekali; byte yang sama dipakai untuk hash dan parse. `ProviderRevisionKey` = (`metadata.id`, `metadata.revision`, `manifest_sha256`).

### Preflight kompatibilitas data

- `ValidatedPackage::data_marker_path(&InstancePaths) -> Option<PathBuf>` memberi path file penanda kalau manifest mendeklarasikan `compatibility.dataMarker`. Pemanggil membacanya lewat `read_file_limited` dengan batas 4 KiB dan memberi hasilnya sebagai `MarkerRead`: `NotDeclared`, `Missing` (file tidak ada), `Unreadable` (bukan file biasa, di atas 4 KiB, bukan UTF-8, atau error baca), atau `Contents(String)`.
- `ValidatedPackage::preflight(recorded: Option<RuntimeLine>, marker: MarkerRead, runtime: &RuntimeLine) -> PreflightOutcome`. `recorded` berasal dari registry; `instance-manifest.json` hanya dipakai kalau registry tidak punya catatan (adoption, fitur 57).
- Isi penanda dibuang spasi dan newline di akhirnya, lalu dicocokkan penuh dengan `pattern`; capture group harus valid sebagai `RuntimeLine`.
- Urutan aturan: `Unreadable` berarti `MarkerUnreadable`; penanda dideklarasikan tetapi `Missing` berarti `MarkerMissing`; isi yang tidak cocok pattern, atau line penanda yang berbeda dari `recorded`, berarti `MarkerMismatch { recorded, marker }`; tanpa `recorded` dan tanpa line penanda berarti `Unknown`; line data (dari `recorded`, atau dari penanda kalau `recorded` kosong) yang tidak boleh dijalankan `runtime` menurut `LineCompatibility` berarti `Incompatible { data, runtime }`; selain itu `Compatible`.
- Pemanggil: start (spec 0010, sebelum spawn), rebind (fitur 58), adoption (fitur 57), dan restore (fitur 51). Fake provider mendeklarasikan `FAKE_VERSION` supaya jalur ini teruji; PostgreSQL nanti memakai `PG_VERSION`.

### Execution plan

Plan dibangun tepat sebelum setiap aksi, dari paket yang sudah tervalidasi dan context saat itu. Plan tidak pernah disimpan lalu dibaca ulang; tipe plan hanya `Serialize` (untuk tampilan, golden test, dan snapshot journal), tidak `Deserialize`.

```rust
pub struct PlanContext {
    pub instance_id: InstanceId,
    pub instance_name: InstanceName,
    pub paths: InstancePaths,                 // spec 0004, termasuk run dari instance_run_dir
    pub runtime: RuntimeContext,              // root canonical, ExactVersion, RuntimeLine
    pub endpoints: BTreeMap<EndpointName, ResolvedEndpoint>, // Ip { host, port: Option<Port> } atau Unix
    pub settings: SettingsValues,             // hasil SettingsSchema::validate
}

pub struct PlanSecret { pub field: SettingName, pub reference: SecretRef }

pub struct InitPlan { pub steps: Vec<InitStep>, pub deadline: Duration, pub cleanup: Vec<PathBuf> }
pub enum InitStep {
    EnsureDirectories(Vec<PathBuf>),          // parent selalu sebelum anak
    WriteFile(FileWrite),
    MaterializeSecret { secret: PlanSecret, path: PathBuf },
    Run(RunSpec),
}
pub struct FileWrite { pub path: PathBuf, pub content: RenderedContent, pub sensitive: bool }
pub struct RenderedContent { pub segments: Vec<Segment> }
pub enum Segment { Text(String), Secret { secret: PlanSecret, format: TemplateFormat } }
pub struct ExecutablePath { pub runtime_root: PathBuf, pub relative: PathBuf }
pub struct Command { pub program: ExecutablePath, pub args: Vec<String>, pub cwd: PathBuf, pub env: Vec<(String, EnvValue)> }
pub enum EnvValue { Text(String), Secret(PlanSecret) }
pub struct RunSpec { pub command: Command, pub stdin: Stdin, pub timeout: Duration, pub success_exit_codes: Vec<u8> }
pub enum Stdin { Null, Secret(PlanSecret) }

pub struct RunPlan {
    pub spawn: Command,                       // stdin null, stdout dan stderr ditangkap
    pub readiness: ProbePlan,                 // deadline = readinessSeconds
    pub recovery_deadline: Duration,          // recoverySeconds
    pub health: HealthPlan,
    pub stop: StopPlan,
    pub log_files: Vec<PathBuf>,
    pub endpoints: Vec<(EndpointName, ResolvedEndpoint)>,
}
pub struct ProbePlan { pub probe: ProbeSpec, pub interval: Duration, pub attempt_timeout: Duration, pub deadline: Duration }
pub struct HealthPlan { pub probe: ProbeSpec, pub interval: Duration, pub attempt_timeout: Duration, pub failure_threshold: NonZeroU8 }
pub enum ProbeSpec {
    Tcp { target: ProbeTarget },
    Http { host: IpAddr, port: Port, path: String, expect_status: RangeInclusive<u16> },
    Exec(RunSpec),                            // stdin selalu Null
}
pub enum ProbeTarget { Ip { host: IpAddr, port: Port }, Unix { path: PathBuf } }
pub enum StopPlan { Signal { signal: StopSignal, timeout: Duration }, Command { run: RunSpec, timeout: Duration } }
pub enum StopSignal { Term, Int, Quit }
pub struct ReloadPlan { pub steps: Vec<InitStep>, pub action: ReloadAction, pub verify: Option<ProbePlan>, pub deadline: Duration }
pub enum ReloadAction { Hangup, Run(RunSpec), None }

impl RenderedContent {
    /// Menyelesaikan segmen secret dari `ResolvedSecrets` (spec 0011), memakai escaper format dan batas 1 MiB yang sama dengan renderer.
    pub fn materialize(&self, secrets: &ResolvedSecrets) -> Result<Vec<u8>, PlanError>;
}
```

- `ValidatedPackage::init_plan(&PlanContext) -> Result<InitPlan, PlanError>`, `run_plan(&PlanContext) -> Result<RunPlan, PlanError>`, dan `reload_plan(&PlanContext) -> Result<ReloadPlan, PlanError>`. `ReloadPlan.steps` hanya berisi `EnsureDirectories` dan `WriteFile`. Timeout dan exit code diisi sesuai tabel *Sumber timeout dan exit code*.
- `PlanError` (`#[non_exhaustive]`), daftar lengkap: `Unsupported { capability }` (BR-016), `EndpointUnresolved { endpoint }`, `EndpointNotLoopback { endpoint }`, `UnrepresentableValue { field }`, `RenderedOutputInvalid { config_file }`, `OutputTooLarge { config_file }`, `ArgumentTooLong { position }`, `PathNotUtf8 { position }`, `PathEscapesScope { position }`, `Settings(SettingsError)` (nilai tidak lagi cocok dengan schema), dan `SecretUnavailable { field }` (hanya dari `materialize`, kalau `ResolvedSecrets` tidak memuat field itu; itu bug pemanggil, karena `resolve_for_start` spec 0011 sudah mengembalikan `NeedsSecret` sebelumnya). Pesan tidak pernah memuat nilai settings teks atau secret.
- `EnsureDirectories` berarti: untuk setiap path (parent selalu lebih dulu), buat dengan mode 0700 kalau belum ada, dan terima yang sudah ada hanya kalau folder, bukan symlink, milik user, dan privat (aturan yang sama dengan `prepare_layout` spec 0004). Plan builder memasukkan folder perantara config (misalnya `config/dynamic/`) sebelum `WriteFile` nya.
- Env child: basis `PATH=/usr/bin:/bin:/usr/sbin:/sbin` (konstanta POSIX di core; fase Windows memindahkannya ke platform), `HOME` dan `TMPDIR` sama dengan `${instance.temp}`, lalu env manifest; diurutkan berdasarkan key. Tidak ada yang diwarisi dari daemon.
- `cwd` default `${instance.data}`. `args` dan path bertipe `String` karena manifest dan context selalu UTF-8; context dengan path yang bukan UTF-8 menghasilkan `PathNotUtf8`. Entri argv lebih dari 4096 byte setelah render menghasilkan `ArgumentTooLong`.
- Host endpoint di `PlanContext` yang bukan loopback ditolak dengan `EndpointNotLoopback`, karena bind LAN ada di Deferred.
- `Debug` dan `Serialize` untuk `PlanSecret` menampilkan `<secret:field>`; `SecretRef` tidak pernah ditulis.
- Semua nilai secret satu aksi diambil dari satu `ResolvedSecrets` hasil `SecretService::resolve_for_start` (spec 0011) sebelum efek samping pertama aksi itu. `materialize`, file `MaterializeSecret`, stdin, dan `EnvValue::Secret` membaca nilai hanya lewat fungsi `secret::delivery` spec 0011 (`render_secret_placeholder`, `SecretFile::create`, `stdin_payload`, `env_pair`), jadi `expose()` tetap di modul yang diizinkan. Mekanisme `hash` spec 0011 belum punya bentuk di manifest v1alpha1; fitur provider pertama yang butuh (ClickHouse untuk `sha256_hex`, fitur 41 untuk bcrypt) menambahkannya sebagai filter template di config `sensitive`, secara additive.

### Fake provider dan dummy server

`crates/fake-server` (bin `stackly-fake-server`). Semua flag bernilai memakai bentuk `--nama=nilai`; flag tanpa nilai hanya `--token-stdin`. Nilai `0` untuk delay ready berarti langsung ready, dan untuk crash serta spam log berarti mati. File argv berisi array JSON string dari argumen yang diterima tanpa argv[0].

| Perintah | Perilaku | Exit |
| --- | --- | --- |
| `version` | mencetak `stackly-fake-server <versi crate>` | 0 |
| `init --data=<dir> [--token-stdin] [--fail=<bool>]` | menulis argv ke `<dir>/argv-init.json`; menolak `<dir>` yang berisi apa pun selain file argv itu; dengan `--token-stdin` membaca stdin sampai EOF dan menulis SHA-256 hex dari byte persisnya (bukan nilainya) ke `<dir>/token.sha256`; menulis `<dir>/FAKE_VERSION` berisi `<major>.<minor>` versi crate dan newline | 0, 3 untuk folder tidak kosong, 4 untuk `--fail=true` |
| `serve --config=<file> --data=<dir> --port=<p> [--host=<ip>] [--label=<teks>] [--ready-delay-ms=<n>] [--crash-after-ms=<n>] [--crash-exit-code=<c>] [--log-lines-per-second=<n>] [--ignore-term=<bool>]` | menulis argv ke `<dir>/argv-serve.json`; config adalah JSON `{ greeting, instanceName, authToken }`; HTTP/1.1 di host (default 127.0.0.1) dan port: `GET /ready` 503 sampai delay lewat lalu 200, `GET /health` 200, `GET /config` 200 dengan body JSON `{"greeting": <teks>}`, `POST /shutdown` 200 lalu keluar 0, path lain 404; crash dengan exit code setelah delay; menulis baris log sekitar 100 byte ke stdout dan stderr bergantian; SIGTERM dan SIGINT keluar 0 dengan rapi kecuali `--ignore-term=true` (SIGTERM diabaikan, SIGINT tetap keluar); SIGHUP membaca ulang config dan mencatat `reloaded config` | 0, 2 untuk argumen salah, kode crash |
| `shutdown --port=<p> [--host=<ip>]` | mengirim `POST /shutdown` ke server itu | 0 kalau server menjawab 200, 1 selain itu |

`providers/fake` (ID `testing/fake`, software `fake`, category `testing`): endpoint `http` (tcp, client, 127.0.0.1, port saran 18080); config `fakeConfig` (format `json`, `sensitive: true`, berisi `greeting`, `instanceName`, dan `authToken` dari secret); initialize `createManagedDirectories`, `renderConfig`, lalu `runExecutable` init dengan `--token-stdin` dan `stdin: { secret: authToken }`; start `serve` dengan setiap perilaku diambil dari settings; stop `gracefulProcess` `term`; reload `render: [fakeConfig]`, `action: signal`, `signal: hup`; readiness `http` ke `/ready`; health `http` ke `/health`; `dataMarker` `FAKE_VERSION`; capability `reload`, `multiInstance`, `persistence`, dan backup `cold`; `managedFilesystem` semua lima folder. Settings: `greeting` (teks, reload), `label` (teks maks 256 tanpa pattern, restart, dipakai sebagai `--label=${settings.label}`), `readyDelayMs`, `crashAfterMs`, `crashExitCode` (1 sampai 255, default 70), `logLinesPerSecond` (integer, restart, advanced), `ignoreTerm`, `failInit` (boolean), `authToken` (secret, `immutableAfterInit`). `versions.json` tulisan tangan dengan satu artefak `fixture` yang versinya sama dengan versi crate `stackly-fake-server` (test memastikannya), line `<major>.<minor>`, dan `executables: ["stackly-fake-server"]`.

Paket fixture `testing/fake-command` di `crates/core/tests/fixtures/packages/fake-command/` memakai binary yang sama untuk primitive sisanya: `materializeSecret` ke `${instance.run}/secrets/token`, probe readiness `tcp`, probe health `exec` (`stackly-fake-server version`), stop `command` (`shutdown --port=${endpoint.http.port}`), dan reload `run`.

`stackly_testkit::fake_server::binary() -> PathBuf` menjalankan `cargo build -p stackly-fake-server --locked` sekali per proses test (lewat `OnceLock`, memakai variabel `CARGO` dan target dir workspace; testkit tidak termasuk crate yang dilarang membaca environment) lalu mengembalikan path binary. Cara binary itu dipasang sebagai runtime di test root adalah keputusan fitur 10 dan 14.

### Paket contoh NATS dan Traefik

- **NATS** (`providers/nats`): seperti contoh di atas. Settings: `serverName` (`managed: "${instance.name}"`), `jetstream` (boolean, default true, restart), `jetstreamMaxMemoryBytes` dan `jetstreamMaxFileBytes` (integer, restart, advanced), `maxPayloadBytes` (integer, restart, advanced). Template `nats.conf.tmpl` (format `natsConf`) menulis `server_name`, `listen`, `http`, `max_payload`, dan blok `jetstream { store_dir, max_memory_store, max_file_store }` di dalam `{{#if settings.jetstream}}`.
- **Traefik** (`providers/traefik`): endpoint `web` (tcp, client, 127.0.0.1, saran 8000) dan `ping` (tcp, monitoring, 127.0.0.1, saran 8082, listener loopback khusus untuk health sesuai PRD 22.5). Config `staticConfig` (yaml, `${instance.config}/traefik.yaml`: entry point `web` dan `ping`, `ping.entryPoint: ping`, `api.dashboard: false`, `api.insecure: false`, `providers.file.directory` dari field managed `dynamicDir = "${instance.config}/dynamic"`, `providers.file.watch: true`, `global.checkNewVersion: false`, `global.sendAnonymousUsage: false`) dan `routes` (yaml, `${instance.config}/dynamic/routes.yaml`). Start `--configFile=${instance.config}/traefik.yaml`. Readiness `http` ke `/ping` di endpoint `ping`. Capability `reload` dengan `render: [routes]`, `action: none`, `verify: readiness`. Settings: `logLevel` (enum, restart), `dynamicDir` (managed), dan `routes` (array of object `{ name, rule, backendUrl }`, `maxItems` 64, `key: name`, `apply: reload`, default kosong; `name` berpola `[a-z][a-z0-9-]{0,62}`, `rule` teks maks 1024, `backendUrl` berpola `http://(127\.0\.0\.1|\[::1\]|localhost):[0-9]{1,5}(/[^\s]*)?`). Template routes menulis `http.routers` dan `http.services` dengan `{{#each settings.routes as route}}` di dalam `{{#if settings.routes}}`.
- `versions.json` keduanya dibangkitkan dari inventory. Menjalankan binary NATS dan Traefik lewat plan adalah bagian fitur 23, 39, dan 41.

### Context golden

Golden test di `crates/core/tests/golden/provider/<paket>/` memakai satu `PlanContext` tetap: owned root data `/Volumes/Test Root/Ünï cödé/data`, owned root logs `/Volumes/Test Root/Ünï cödé/logs`, owned root `runtime` (tempat folder run) `/private/tmp/skt-golden/stackly-0000000a`, `InstanceId` `0192f000-0000-7000-8000-00000000000a`, nama `golden`, folder artefak runtime `/Volumes/Test Root/Ünï cödé/data/runtimes/<software>/<versi>/macos-aarch64/<64 nol>`, port endpoint pertama 14000 lalu naik satu per endpoint dalam urutan nama, settings bernilai default kecuali `routes` Traefik berisi tiga route contoh dan `label` fake berisi teks dengan metakarakter shell, dan reference secret `local:instance/0192f000-0000-7000-8000-00000000000a/auth-token` (bentuk reference daemon spec 0011). Segmen secret di golden ditulis sebagai `<secret:field>`. `STACKLY_UPDATE_GOLDEN=1` menulis ulang golden.

### Fuzz target

Toolchain, lokasi crate fuzz, dan cara run dicatat mengikuti spec 0006. Logika harness tinggal di `stackly-core` modul `#[doc(hidden)] pub mod provider::fuzzing`, yang memanggil `ValidatedPackage::from_files` atau parser modulnya langsung tanpa filesystem, sehingga target di `fuzz/fuzz_targets/` dan test regresi stable memanggil fungsi yang sama. Setiap input yang pernah crash disalin ke `crates/core/tests/fuzz_regressions/<target>/` dan diputar ulang oleh `crates/core/tests/fuzz_regressions.rs` di toolchain stable; seed dibuat ulang `cargo xtask fuzz seed` dari paket contoh.

| Target | Input | Yang diuji |
| --- | --- | --- |
| `provider_manifest` | byte sebagai `provider.yaml`, dengan settings schema, template, dan `versions.json` fake di `PackageFiles` | decode dua tahap dan semua aturan validasi |
| `provider_template` | byte sebagai template | parse, cek statis terhadap context fake, render dengan nilai tetap ke ketiga format, parse ulang |
| `provider_interpolation` | byte sebagai string field | parse token dan resolve di setiap jenis posisi |
| `provider_settings_schema` | byte sebagai settings schema | parse dialek, lalu validasi beberapa dokumen nilai tetap |

Corpus awal diambil dari ketiga paket contoh dan paket `fake-command`. Setiap target berjalan 10 menit di Mac ini; hasilnya dicatat di langkah `/check verify`.

### Kode error di wire

Area `manifest` dan `provider` milik spec ini (spec 0006). `stacklyd` memetakan error berikut di modul `map`; semua `retryable` false. Daftar `Issue` dikirim di `ErrorBody.items` (spec 0006 `ErrorItem`) ber `kind` `issue` dengan field `file`, `location`, `code`, dan `message`, paling banyak 100 item seperti batas `PackageError`, dan `message` issue tidak pernah memuat nilai settings teks atau secret.

| Sumber | Kode | Kategori | Kunci `details` | `items` |
| --- | --- | --- | --- | --- |
| `PackageError` | `manifest.invalid` | `invalid_input` | `package`, `issueCount`, `omitted` | `issue` |
| `SettingsError`, `PlanError::Settings` | `provider.invalid_settings` | `invalid_input` | `issueCount`, `omitted` | `issue` |
| `PlanError::Unsupported` | `provider.unsupported_capability` | `unsupported` | `capability` (plus `rule` BR-016) | |
| `PlanError::EndpointNotLoopback` | `provider.endpoint_not_loopback` | `invalid_input` | `endpoint` | |
| `PlanError::UnrepresentableValue` | `provider.unrepresentable_value` | `invalid_input` | `field` | |
| `PlanError::ArgumentTooLong`, `PathNotUtf8`, `PathEscapesScope` | `provider.plan_invalid` | `invalid_input` | `position`, `reason` | |
| `PlanError::RenderedOutputInvalid`, `OutputTooLarge` | `provider.render_failed` | `failed` | `configFile`, `reason` | |
| `PlanError::EndpointUnresolved`, `SecretUnavailable` | `internal.unexpected` (spec 0006) | `internal` | | |

`ResourceRef` untuk provider revision (spec 0006, `ResourceKind` `provider`) memakai `id` berupa teks `<namespace>/<name>@<revision>#<manifest_sha256>`, misalnya `official/nats@2.12.1-1#<64 hex>`. Teks ini tidak ambigu karena `ProviderName` dan `RevisionLabel` tidak pernah memuat `@` atau `#`.

### API surface

Semua fungsi sync dan tanpa IO kecuali `read_package` (yang hanya membaca lewat `&dyn Filesystem`). Pemanggilnya selalu daemon atau tooling; CLI tidak pernah memanggil core (spec 0001).

| Fungsi atau perintah | Tempat | Input utama | Output utama | Pemanggil | Error utama |
| --- | --- | --- | --- | --- | --- |
| `read_package` | `core::provider::package` | `&dyn Filesystem`, path folder paket | `PackageFiles` | catalog (fitur 17), import (fitur 37), xtask, test | `PackageError { package, issues }` |
| `ValidatedPackage::from_files` | `core::provider::validate` | `PackageFiles`, `NameCheck` | `ValidatedPackage` | sama dengan di atas, fuzz | `PackageError { package, issues }` |
| `ValidatedPackage::{key, software, manifest, endpoints, settings_schema, provider_revision, files}` | core | tidak ada | identitas, model, `ProviderRevision`, `PackageFiles` | fitur 5, 17, 20, 37 | tidak ada |
| `SettingsSchema::validate` | `core::provider::settings` | `serde_json::Value` | `SettingsValues` | create (fitur 20), edit config (fitur 29) | `SettingsError { issues }` |
| `ValidatedPackage::{init_plan, run_plan, reload_plan}` | `core::provider::plan` | `&PlanContext` | `InitPlan`, `RunPlan`, `ReloadPlan` | fitur 20, spec 0010, fitur 26 | `PlanError` |
| `RenderedContent::materialize` | `core::provider::plan` | `&ResolvedSecrets` (spec 0011) | byte final | executor spec 0010 | `PlanError` |
| `ValidatedPackage::{data_marker_path, preflight}` | `core::provider::preflight` | `InstancePaths`; line tercatat, `MarkerRead`, line runtime | path; `PreflightOutcome` | spec 0010, fitur 51, 57, 58 | tidak ada |
| `cargo xtask provider check [<dir>...]` | xtask (macOS) | folder paket (default setiap folder di `providers/`) | `ok <id> <revision> <manifest_sha256>` per paket valid, atau `<folder>: <file> <lokasi>: <kode>: <pesan>` per masalah | manual di Mac ini | exit 1 kalau ada masalah |
| `cargo xtask codegen [--check]` | xtask | inventory, `providers/`, tipe Rust | `versions.json`, `schemas/provider/*.json` | manual | drift saat `--check` |
| `stackly-fake-server` | `crates/fake-server` | lihat tabel fake | proses dummy | test fitur 9, 10, 14, 20, 26 | exit code di tabel |
| `fake_server::binary` | testkit | tidak ada | path binary | test | panic dengan pesan build gagal |

`Issue { file, location, code, message }`: `file` adalah `provider.yaml`, `versions.json`, `settings.schema.json`, atau `templates/<nama>`; `location` adalah path field (misalnya `lifecycle.start.args[1]`) atau baris dan kolom; `code` adalah enum `IssueCode` (`#[non_exhaustive]`) dengan `as_code()` snake_case stabil; `message` lowercase tanpa titik di akhir dan tidak pernah memuat nilai secret. Kode yang dipakai: `unsupported_api_version`, `unknown_kind`, `unknown_field`, `missing_field`, `invalid_value`, `duplicate_key`, `yaml_syntax`, `json_syntax`, `missing_file`, `unknown_primitive`, `unknown_probe`, `unknown_template_format`, `unknown_config_file`, `unknown_secret`, `unknown_setting`, `unknown_token`, `token_not_allowed_here`, `secret_not_allowed_here`, `literal_executable`, `argument_injection_risk`, `path_escapes_scope`, `path_not_permitted`, `invalid_name`, `unknown_endpoint`, `endpoint_not_loopback`, `socket_name_too_long`, `capability_unsatisfiable`, `reload_without_capability`, `timeout_out_of_range`, `unknown_executable`, `executable_not_in_artifact`, `template_syntax`, `template_type`, `unrepresentable_value`, `rendered_output_invalid`, `output_too_large`, `unreferenced_template`, `unexpected_entry`, `symlink_not_allowed`, `not_a_regular_file`, `file_too_large`, `not_utf8`, `settings_schema_keyword`, `settings_schema_rule`, `settings_value`, `versions_rule`, `fixture_outside_testing`, `directory_name_mismatch`, `too_many_issues`.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| `read_package` | byte paket | `Filesystem::read_file_limited` pada folder yang diberikan pemanggil (folder catalog bawaan dari fitur 17, argumen xtask, atau salinan di `TestRoot`) |
| `from_files` | `manifest_sha256` | SHA-256 atas daftar baris di *Digest paket* |
| `from_files` | lokasi issue | path dari `serde_path_to_error`, posisi error `serde_yaml_ng` atau `serde_json` (baris dan kolom), atau path field dari validator |
| `provider_revision` | `CapabilitySet`, `LineCompatibility` | `capabilities` dan `compatibility.lineCompatibility` |
| plan | `instance.id`, `instance.name`, `instance.shortKey` | aggregate `Instance` (spec 0004); short key diturunkan dari ID |
| plan | `provider.id`, `provider.revision` | `metadata` paket yang tervalidasi |
| plan | `paths` | `DirectoryLayout::instance_paths` dan `instance_run_dir` (spec 0004), disusun pemanggil |
| plan | `runtime.root` | `DirectoryLayout::runtime_dir` untuk runtime managed, atau root `InstallMode::External` (spec 0004), dibuat canonical oleh pemanggil |
| plan | `runtime.version`, `runtime.line` | record runtime (fitur 5 dan 18) |
| plan | host dan port endpoint | endpoint instance (spec 0004): port `Fixed`, atau port yang dialokasikan spec 0010 dan fitur 20 sebelum memanggil plan |
| plan | path socket | folder run instance ditambah `socketName` yang dirender |
| plan | nilai settings | registry (spec 0005), hasil `SettingsSchema::validate` atas input fitur 20 atau 29 |
| plan | nilai field `managed` | interpolasi ekspresi `managed` dengan context saat plan dibangun |
| plan | `PlanSecret` | nama field secret di schema dan reference di settings (format spec 0011) |
| plan | `PATH`, `HOME`, `TMPDIR` | konstanta di core dan `${instance.temp}` |
| plan | timeout, interval, ambang, exit code | tabel *Sumber timeout dan exit code*; default konstanta di `core::provider::timeouts` |
| `materialize`, `EnvValue::Secret`, stdin, `MaterializeSecret` | byte secret | `ResolvedSecrets` dari `SecretService::resolve_for_start` (spec 0011), dibaca lewat `secret::delivery` |
| `preflight` | line tercatat | `data.initializedLine` di registry; `instance-manifest.json` (spec 0004) kalau registry tidak punya catatan |
| `preflight` | isi penanda | pemanggil membaca `data_marker_path` lewat `read_file_limited` 4 KiB |
| `versions.json` | semua field | `runtimes/inventory.toml` lewat generator, atau tulisan tangan untuk paket `testing/` |
| fake server | perilaku | settings fake yang masuk ke argv lewat interpolasi |
| golden test | context | *Context golden* |

### Asumsi pada spec lain

- **Assumed (spec 0006)**, diselaraskan 2026-10-18: error spec ini memakai kode di tabel *Kode error di wire* (area `manifest` dan `provider` milik spec ini, nama `<area>.<alasan>`), dengan daftar issue lewat `ErrorBody.items` ber `kind` `issue`. Toolchain nightly untuk `cargo-fuzz`, lokasi crate fuzz, modul harness `fuzzing`, replay di stable, dan cara mencatat run juga dari 0006; fitur 9 hanya menambah empat target.
- **Assumed (spec 0011)**, diselaraskan 2026-10-18: `SecretRef` adalah tipe reference opaque di `stackly-domain::secret` dengan parser dan tanpa nilai secret; tipe sementara di fitur ini diganti tipe itu. Nilai secret satu aksi datang dari `ResolvedSecrets` hasil `SecretService::resolve_for_start` (worker Keychain, `NeedsSecret`, dan interaksi milik 0011), dan dibaca hanya lewat fungsi `secret::delivery`. File secret ada di `<runtime>/i/<short-key>/secrets/`. Alur input secret memeriksa `minLength` dan `maxLength` field secret, dan `generate` menjepit panjangnya ke batas itu. Aturan redaction mencakup config file `sensitive`: tidak masuk diff histori config, diagnostics, atau export.
- **Assumed (spec 0005)**: registry menyimpan `ProviderRevision` (key, software, capability, line compatibility) dan nilai settings per instance sebagai JSON dari `SettingsValues::to_json`, dengan field secret sebagai teks reference saja.
- **Assumed (spec 0010)**: executor menjalankan `InitPlan`, `RunPlan`, dan `ReloadPlan` apa adanya:
  - memanggil `ensure_instance_run_dir` (spec 0004) sebelum setiap aksi yang memakai folder `run`;
  - menjalankan `EnsureDirectories` dengan semantik di *Execution plan*;
  - memeriksa ulang setiap path tulis dan hapus dengan `check_owned_path` (canonical) tepat sebelum IO;
  - memanggil `resolve_for_start` (spec 0011) sekali per aksi, lalu menyelesaikan dan meng escape semua segmen secret aksi itu lewat `materialize(&ResolvedSecrets)` sebelum efek samping pertama, dan menulis `WriteFile` lewat `write_file_atomic` (0600);
  - membuat file `MaterializeSecret` dan menghapusnya di akhir aksi lewat operasi hapus yang ditambahkan ke port `Filesystem`;
  - memverifikasi `ExecutablePath` (canonical, di dalam root runtime, file biasa, executable) tepat sebelum exec;
  - spawn dengan argv, env, dan cwd persis dari plan tanpa shell, dan menulis stdin secret lalu menutupnya;
  - menjalankan probe `tcp`, `http`, dan `exec`;
  - memanggil `preflight` sebelum start;
  - mengalokasikan port Auto sebelum memanggil `run_plan`;
  - memutuskan apa yang terjadi saat setiap timeout lewat, termasuk nasib proses probe `exec` dan langkah `Run` yang melewati batas.
- **Assumed (spec 0004)**: value type domain punya konstruktor yang memvalidasi (`FromStr` atau `TryFrom<String>`) dengan `ValueError`, `InstancePaths` memuat `data`, `config`, `temp`, `logs`, dan `run`, `CapabilitySet` bisa dibangun dari daftar `Capability`, dan `prepare_layout` punya aturan privasi folder yang bisa dipakai ulang.
- **Assumed (spec 0003)**: field inventory sesuai tabel spec 0003 (`provider`, `line`, `version`, `os`, `arch`, `status`, `blocked_*`, `format`, `urls`, `sha256`, `size`, `archive_root`, `signing_policy`, `license`, `license_url`, `executables`, `min_os`).

### Key invariants

- Membaca paket tidak pernah menulis file, menjalankan proses, menyentuh jaringan, atau menyelesaikan secret.
- Setiap field manifest dikenal, dan setiap rujukan (endpoint, config file, template, alias executable, field settings, field secret) terselesaikan saat validasi. Paket yang tervalidasi selalu menghasilkan plan untuk context yang valid, kecuali error yang bergantung data dan tercantum di daftar `PlanError`.
- Plan tidak memuat nilai secret dan tidak pernah memuat shell; program selalu executable runtime yang dideklarasikan; satu entri `args` selalu satu elemen argv.
- Setiap path di plan berada di bawah folder instance yang tercantum di `permissions.managedFilesystem`, dan setiap executable di bawah root runtime.
- Template tidak pernah mengeluarkan nilai tanpa escaper format, dan output `yaml` serta `json` selalu dokumen yang bisa diparse.
- Settings tidak bisa mengubah path, executable, port, permission, atau trust (VAL-018, VAL-019); port adalah endpoint, bukan settings.
- `manifest_sha256` mengikuti perilaku (manifest, settings schema, template), bukan katalog versi atau dokumentasi.
- Plan tidak pernah didecode dari data yang disimpan; ia selalu dibangun ulang.

### Security model

- Ancaman utama adalah manifest yang jahat atau ceroboh (RISK-08). Pertahanannya: primitive tertutup, tanpa shell dan tanpa hook, executable hanya dari runtime yang terverifikasi dan dideklarasikan (dan diaudit spike spec 0003 lewat `executables` inventory), path hanya di folder instance yang dideklarasikan, secret hanya di stdin, file 0600 sementara, dan config sensitif, serta env tertutup.
- Permission engine membatasi tindakan engine, bukan sandbox untuk child: executable native tetap berjalan dengan hak user (PRD 21.4). Peringatan ke pengguna dan trust approval milik fitur 37.
- Nilai settings yang jahat dihadang escaper per format, parse ulang output, aturan `argument_injection_risk`, larangan karakter kontrol, batas schema, dan absennya tipe path atau port di settings.
- File paket yang jahat dihadang penolakan symlink, charset nama dan komponen path (termasuk `socketName`), batas ukuran dan jumlah, UTF-8 ketat, batas alias dan rekursi `serde_yaml_ng` (dibuktikan test billion laughs), pattern yang selalu dicocokkan penuh dengan `regex` berwaktu linear dan batas ukuran, batas nesting dan output template, serta fuzzing.
- Probe `http` hanya ke endpoint loopback milik instance, tanpa redirect.
- Namespace `testing/` (fake provider) tidak pernah masuk catalog bawaan app; fitur 15 dan 17 menegakkannya saat bundling dan memuat catalog.
- Tidak ada `unsafe` baru. Dummy server memakai API aman `signal-hook`.

### Configuration required

Tidak ada variabel environment baru untuk daemon atau CLI. Kode test membaca `STACKLY_UPDATE_GOLDEN=1` untuk menulis ulang golden, dan `stackly-testkit` membaca `CARGO` untuk membangun dummy server.

### Critical test scenarios

- Happy path: ketiga paket contoh disalin ke `TestRoot` dan dibaca dengan `FakePlatform` tanpa menambah `mutations()`, `cargo xtask provider check` mencetak tiga baris `ok`, paket `fake-command` lolos, dan plan init, run, serta reload fake sama dengan golden. Memverifikasi **AC-1**, **AC-11**, **AC-14**.
- Tanpa shell: plan run fake dengan `label` berisi `` $(touch pwned); rm -rf ~ `id` "q" 'q' | & > ü 空 `` di context golden dijalankan test lewat `std::process::Command` (program, args, `env_clear` plus env plan, cwd di folder test); `argv-serve.json` sama persis dengan plan dan file `pwned` tidak ada. Memverifikasi **AC-11** (AC-031, AC-058, REQ-031).
- Himpunan tertutup: manifest dengan primitive `runShell`, probe `script`, probe `native`, format `ini`, sinyal `kill`, dan strategi stop `kill` masing masing ditolak saat validasi dengan kode dan lokasinya. Memverifikasi **AC-4**.
- Field keamanan asing: `permissions.network`, `permissions.sandbox: false`, dan `lifecycle.start.shell: true` ditolak dengan `unknown_field` dan path nya; `apiVersion: stackly/v1beta1` ditolak sebelum field lain dibaca; file alias bersarang 256 KiB ditolak dalam 2 detik. Memverifikasi **AC-2** (VAL-018).
- Capability: `reload: true` tanpa `lifecycle.reload`, `lifecycle.reload` tanpa capability, field `apply: reload` tanpa capability, `metrics` tanpa endpoint monitoring, `tls: true`, backup `online`, dan backup `cold` tanpa `persistence` masing masing ditolak. Memverifikasi **AC-5**.
- Interpolasi: `executable: /bin/sh`, `executable: "${runtime.executable} -c"`, `${secret.authToken}` di `args`, di nilai `env` bersama teks lain (`--pass=${secret.authToken}`), dan di template tidak sensitif, `materializeSecret` ke `${instance.temp}/token` atau langsung ke `${instance.run}/token`, `args: ["${settings.label}"]`, `cwd: "${instance.data}/../config"`, `cwd: "${instance.data}/${settings.label}"`, `destination: "${instance.logs}/x"` saat `logs` tidak diizinkan, `--data=${instance.temp}` saat `temp` tidak diizinkan, `${unknown.x}`, `managed: "${settings.greeting}"`, dan env `DYLD_INSERT_LIBRARIES` masing masing ditolak; env `FAKE_TOKEN: "${secret.authToken}"` diterima di `lifecycle.start` dan menjadi `EnvValue::Secret`, tetapi ditolak di probe `exec` dan stop `command`; `$${HOME}` menghasilkan `${HOME}` literal. Memverifikasi **AC-7**.
- Settings: keyword `$ref`, `oneOf`, dan `format` ditolak; field tanpa `maxLength`, enum `-x`, secret dengan `default`, default yang melanggar `maxLength`, field managed di `required`, 65 field root, dan object bersarang ditolak; pattern `a|b` hanya cocok `a` atau `b` penuh; field `apply: reload` yang dipakai di `args` ditolak; nilai dengan key asing, field managed yang diisi, route dengan `name` ganda, integer `5.0`, teks dengan newline, dan integer di luar batas ditolak dengan path field nya. Memverifikasi **AC-8** (VAL-012, VAL-019).
- Template: `{{ settings.routes }}`, `{{#each settings.greeting}}`, `{{#each settings.routes as settings}}`, `{{ secret.authToken }}` di template tidak sensitif, tag tidak seimbang, nesting 5, dan `prefix-{{ endpoint.web.address }}` di template yaml (output tidak valid) ditolak dengan baris dan kolom; context dengan home berisi `"` membuat plan NATS gagal dengan `UnrepresentableValue` yang menyebut token, tanpa nilainya. Memverifikasi **AC-9**.
- Endpoint: `defaultHost: 0.0.0.0`, socket `.s.PGSQL.${endpoint.client.port}x` (16 byte), socket `../x`, nama endpoint `client-tls`, probe ke endpoint yang tidak ada, dan probe `tcp` ke endpoint `udp` ditolak; context dengan host `0.0.0.0` menghasilkan `EndpointNotLoopback`. Memverifikasi **AC-6**, **AC-11**.
- Paket kotor: symlink di `templates/`, FIFO sebagai `provider.yaml`, `provider.yaml` 256 KiB plus 1 byte, `versions.json` yang hilang, file `notes.txt` di root, template yang tidak dirujuk, file berawalan BOM, dan folder `providers/nats2` dengan ID `official/nats` masing masing ditolak; `.DS_Store` dilewati; folder pinned dengan `NameCheck::Skip` diterima. Memverifikasi **AC-1**, **AC-3**.
- VAL-002: ID `official/../x`, `Official/Nats`, `nats` tanpa namespace, revision kosong, dan revision `1..2` ditolak. Memverifikasi **AC-3**.
- Timeout: `stopSeconds: 0`, `readinessSeconds: 601`, `reloadSeconds` tanpa capability reload, langkah run dengan timeout di atas `initSeconds`, dan `attemptTimeoutMilliseconds` di atas `readinessSeconds` ditolak; plan fake memuat timeout dan exit code sesuai tabel sumber. Memverifikasi **AC-10**.
- Digest: satu byte template berubah mengubah digest, sedangkan perubahan `versions.json` dan `README.md` tidak. Memverifikasi **AC-12**.
- Katalog versi: entri inventory NATS yang diubah membuat `codegen --check` gagal; `fixture` di namespace `official`, `available` tanpa `sha256`, versi yang tidak diawali line nya, ID ganda, artefak `linux` yang tidak ada di `platforms`, dan `runtime.executable` yang tidak ada di `executables` ditolak. Memverifikasi **AC-13**.
- Plan khusus: `reload_plan` NATS mengembalikan `Unsupported { capability: reload }`; endpoint Auto tanpa port menghasilkan `EndpointUnresolved`; argv 4097 byte menghasilkan `ArgumentTooLong`; `materialize` dengan `ResolvedSecrets` fixture yang berisi secret dengan `$` untuk config `natsConf` sensitif menghasilkan `UnrepresentableValue` tanpa nilainya, dan `ResolvedSecrets` tanpa field itu menghasilkan `SecretUnavailable`. Memverifikasi **AC-11**.
- Preflight: line tercatat `0.1` dengan penanda `0.1\n` dan runtime `0.1` cocok; penanda hilang, penanda 5 KiB, penanda `0.2` dengan tercatat `0.1`, tanpa catatan dan tanpa penanda, dan runtime `0.2` tanpa `lineCompatibility` masing masing menghasilkan hasil yang benar. Memverifikasi **AC-18**.
- Dummy server: integration test di `crates/fake-server/tests/` menjalankan binary nya sendiri dengan `TestRoot` dan `allocate_port`, dan membuktikan delay ready, crash dengan kode, spam log, `--ignore-term`, reload lewat SIGHUP, `shutdown`, init yang menolak folder tidak kosong, dan `token.sha256` dari stdin sampai EOF. Memverifikasi **AC-15**.
- Fuzz: keempat target berjalan 10 menit masing masing tanpa temuan. Memverifikasi **AC-16**.
- Schema editor: perubahan tipe mentah manifest tanpa `codegen` membuat `codegen --check` gagal. Memverifikasi **AC-17**.

Test yang membuktikan ID PRD (REQ-029, REQ-031, VAL-002, VAL-012, VAL-018, VAL-019, AC-031, AC-058) memasang guard evidence `fixture` spec 0002. AC-029 hanya terbukti sebagian di sini (bagian manifest); install, create, start, health, dan form dibuktikan fitur 14, 20, dan 36.

## Build plan

Urutan mengikuti pendekatan Journey proyek: satu jalur dibuat lengkap lebih dulu, yaitu "paket fake dibaca, divalidasi, lalu menghasilkan plan yang benar, termasuk semua jalur gagalnya", karena itulah yang dibutuhkan fitur 10 dan 14. NATS, Traefik, katalog versi, dan JSON Schema menyusul di atas jalur itu, lalu fuzz dan pengerasan.

**Milestone 1: Fake provider dari paket sampai plan**

0. Domain: setelah update spec 0004 di Follow-up masuk, tambah `Capability::MultiInstance` dan `Capability::Persistence` dengan kodenya, konstruktor `CapabilitySet` dari daftar, dan konstruktor `LineCompatibility` dari peta (kalau model domain fitur 4 belum digabung, perubahan ini masuk lewat cabang itu). Memenuhi **AC-5**.
1. Workspace dependency (`serde_yaml_ng`, `serde_path_to_error`, `regex`, `signal-hook`) dan crate `crates/fake-server` dengan `version`, `init`, `serve`, `shutdown`, semua perilaku, dan integration test nya; aturan `check-deps` untuk crate itu; `stackly_testkit::fake_server::binary()`. Memenuhi **AC-15**.
2. `core::provider` dasar: tipe mentah v1alpha1 yang ketat, decode dua tahap dengan `serde_path_to_error`, model `Issue` dan `IssueCode` dengan batas 100, aturan nama dan charset, `read_package` lewat port `Filesystem`, `PackageFiles`, `from_files` dengan `NameCheck`, digest, dan `SecretRef` sementara bertanda `// provisional: F11`. Memenuhi **AC-1**, **AC-2**, **AC-3**, **AC-12**.
3. Interpolasi: parser, tipe token, bentuk teks, dan matriks posisi. Memenuhi **AC-7**.
4. Settings schema: dialek, model field VAL-012, aturan default dan pattern, dan `SettingsSchema::validate`. Memenuhi **AC-8**.
5. Renderer template: parser, cek statis, aturan baris blok, batas yang memutus render, escaper `yaml`, `json`, serta `natsConf`, dan parse ulang output. Memenuhi **AC-9**.
6. Validasi semantik: primitive, probe generik, endpoint, capability, timeout dan sumbernya, reload, permission, aturan `apply: reload`, serta silang `versions.json` dengan executable dan platform. Memenuhi **AC-4**, **AC-5**, **AC-6**, **AC-8**, **AC-10**, **AC-13** (bagian loader).
7. Plan builder dan preflight: `PlanContext`, `InitPlan`, `RunPlan`, `ReloadPlan`, `PlanSecret`, `materialize`, env basis, daftar `PlanError`, `Serialize` dengan secret tersamar, `data_marker_path`, dan `preflight`. Memenuhi **AC-11**, **AC-18**.
8. Paket `providers/fake` dengan `versions.json` fixture, paket `fake-command` di fixture test core, integration test yang memuat keduanya dan membandingkan plan dengan golden, dan test tanpa shell yang menjalankan dummy server dari plan. Memenuhi **AC-11**, **AC-14** (fake), **AC-15**.

**Milestone 2: NATS, Traefik, katalog versi, dan JSON Schema**

9. Generator `versions.json` di `cargo xtask codegen` dan `--check`; hapus penanda `// provisional: F9` di `xtask/src/runtime/inventory.rs`. Memenuhi **AC-13**.
10. Paket `providers/nats` dan golden render serta plan nya. Memenuhi **AC-14**.
11. Paket `providers/traefik` dengan `settings.routes`, golden untuk nol, satu, dan tiga route, dan plan reload. Memenuhi **AC-14**.
12. JSON Schema lewat fitur `json-schema` dan schemars ke `schemas/provider/`, dengan pattern nama dan versi, plus `--check`. Memenuhi **AC-17**.
13. `cargo xtask provider check` (memakai `MacosFilesystem`) dan test nya dengan salinan rusak tiap paket. Memenuhi **AC-14**.

**Milestone 3: Fuzz dan pengerasan**

14. Empat fuzz target dengan harness di `provider::fuzzing`, seed dari paket contoh, replay regresi di stable, dijalankan 10 menit masing masing di Mac ini. Memenuhi **AC-16**.
15. Suite adversarial dari *Critical test scenarios* yang belum tercakup (termasuk test billion laughs), dengan guard evidence untuk ID PRD. Kalau `serde_yaml_ng` tidak menolak alias bersarang dalam batas waktu, build berhenti dan keputusan YAML kembali ke `/architect`. Memenuhi **AC-1** sampai **AC-10**, **AC-18**.
16. Ganti penanda `// provisional: F9` di `xtask/src/runtime/spike/mod.rs` dan `spike/providers.rs` menjadi `// provisional: F36` dengan catatan bahwa harness fitur 36 akan membangun langkah spike dari paket provider. Memenuhi **AC-13** (kebersihan penanda).
17. Katalog `codes::manifest` dan `codes::provider` di `stackly-protocol`, `ItemSpec` ber `kind` `issue`, bentuk teks `ResourceRef` provider, dan pemetaan di `stacklyd::map` dengan test per varian (butuh milestone 1 spec 0006). Memenuhi **AC-19**.

**Sesi paralel yang aman**: langkah 1 (crate dummy server) tidak menyentuh file lain, jadi bisa jalan di sesinya sendiri sejak awal. Setelah langkah 2 masuk, langkah 3 dan 4 bisa dipecah ke dua sesi kalau masing masing hanya menyentuh file modulnya sendiri dan satu baris deklarasi `mod`; langkah 5 menunggu keduanya karena memakai tipe token dan model settings. Langkah 9 (generator di xtask) juga bisa paralel sejak langkah 2 masuk. Setelah milestone 1 selesai, langkah 10 dan 11 (folder `providers/` dan golden masing masing) serta langkah 14 (crate fuzz) bisa jalan paralel. Langkah 0, 6, 7, 8, 12, 13, 15, dan 16 sebaiknya di satu sesi karena menyentuh file validasi dan test yang sama.

## Consequences

**Positive**:

- Tool baru yang cukup dengan primitive yang ada hanya butuh folder di `providers/`; perubahan core per provider (probe native, format template) terlihat dan bisa dihitung untuk RISK-07.
- Tidak ada jalan dari manifest ke shell atau kode sembarangan, dan semua kesalahan ketahuan saat validasi dengan lokasi yang jelas (RISK-08, REQ-031).
- Spec 0010 menerima plan yang sudah lengkap dan tervalidasi, jadi supervisor tidak perlu tahu format manifest.
- Satu sumber data artefak (inventory) untuk tooling dan catalog, dengan drift yang tertangkap `codegen --check`.
- Form fitur 20 dan 29 bisa dirender dari model field bertipe tanpa kode khusus per provider.

**Negative / tradeoffs**:

- Renderer template dan dialek settings adalah kode sendiri yang harus dirawat dan di fuzz terus; kontributor tidak bisa memakai fitur Jinja atau JSON Schema yang lebih kaya.
- Aturan ketat (field asing ditolak, `args` tidak boleh diawali settings teks bebas, tag template harus menjadi scalar utuh, settings tanpa object bersarang) membuat sebagian manifest lebih panjang, dan setiap perluasan butuh review serta perubahan schema lewat kebijakan fitur 36.
- Format config `postgresqlConf`, `valkeyConf`, `mysqlCnf`, dan `xml`, serta jenis probe `native`, belum ada; fitur 19, 23, 40, 42, dan 43 masing masing menambah satu perubahan ke core.
- Decode struktural hanya melaporkan satu error per file karena serde berhenti di error pertama; kontributor bisa perlu beberapa putaran untuk kesalahan struktur.
- Path context yang memuat karakter yang tidak bisa direpresentasikan format (misalnya `"` di path home untuk `natsConf`) membuat plan gagal; pesan error menyebutnya, tetapi instance itu tidak bisa jalan sampai path berubah.
- `fake_server::binary()` memanggil `cargo build` dari dalam test, sehingga run test pertama sedikit lebih lama.
- `REQ-029` menyebut dependency antar runtime di manifest; v1alpha1 belum memuatnya karena belum ada provider yang membutuhkannya, dan penambahannya nanti adalah perubahan additive.

**Neutral**:

- `serde_yaml_ng`, `serde_path_to_error`, `regex`, `sha2`, dan `serde` masuk ke `stackly-core`; schemars hanya lewat fitur `json-schema` yang dipakai xtask.
- `providers/` dan `schemas/provider/` dibuat di fitur ini, lebih awal dari yang ditulis susunan repo spec 0001.
- Penanda spike spec 0003 berpindah ke fitur 36, jadi dua tempat yang tahu cara menjalankan provider (spike xtask dan paket provider) masih ada sampai harness itu dibangun.

## Follow-up

- [x] `/architect` update spec 0004: tambah `Capability::MultiInstance` (`multi_instance`) dan `Capability::Persistence` (`persistence`); pastikan `CapabilitySet` bisa dibangun dari daftar dan `LineCompatibility` punya konstruktor dari peta line ke himpunan line; tambah operasi hapus file (misalnya `remove_file` yang tidak mengikuti symlink) ke port `Filesystem` untuk file secret sementara spec 0010. Ini juga menutup Follow-up fitur 9 di spec 0004 (batas socket 15 byte, pemetaan capability, isi `LineCompatibility`). Diterapkan di spec 0004 (2026-10-18): capability sebagai titik perluasan fitur 9, `CapabilitySet` dari daftar sudah ada lewat `FromIterator`, dan operasi hapus berbentuk `remove_file` plus `remove_empty_dir` tanpa rekursif.
- [x] `/architect` update spec 0001: tandai baris "Validator JSON Schema dan template engine manifest" di *Batas keputusan* sudah diputuskan spec 0009 (tanpa validator generik, renderer sendiri); di *Peta crate*, core mendapat `serde`, `serde_yaml_ng`, `serde_path_to_error`, `regex`, `sha2`, dan `schemars` opsional, tambah crate bin `stackly-fake-server` (`crates/fake-server`; `signal-hook`, `serde_json`, `sha2`; dev `stackly-testkit`), dan xtask mendapat `stackly-core` serta `stackly-platform-macos` di bawah `cfg(target_os = "macos")`; di *Aturan arah dependency*, `stackly-fake-server` tanpa dependency normal ke crate Stackly dan tanpa crate yang bergantung padanya; di *Susunan repo*, `providers/` dibuat fitur 9 dan `schemas/provider/` berisi schema provider. Diterapkan di spec 0001 (2026-10-18).
- [x] Update spec 0003 (pemiliknya): follow up "Fitur 9 dan 18: putuskan cara catalog membaca inventory" sudah diputuskan untuk bagian fitur 9 (catalog membaca `versions.json` hasil generator, inventory tetap tooling); penanda spike pindah ke fitur 36. Fitur 18 masih memutuskan cara install memakai `versions.json`. Diterapkan di spec 0003 (2026-10-18).
- [ ] Fitur 18: kalau install butuh field di luar `stackly.provider-versions/v1`, tambahkan lewat `/architect` update spec ini sebagai perubahan additive.
- [ ] Fitur 17 dan 15: jangan pernah memuat atau membundel paket namespace `testing/` di catalog app.
- [ ] Fitur 20 dan 21: putuskan apakah `Endpoint` domain perlu membawa `purpose` untuk connection string, atau dibaca dari paket.
- [ ] Fitur 36: kebijakan kompatibilitas v1alpha1 mencakup penambahan primitive, jenis probe, probe native, dan format template secara additive, dan harness menggantikan langkah spike per provider.
- [ ] Fitur 23: tambah jenis probe `native`, registry `NativeProbeDescriptor`, dan `nats-readiness` (cek JetStream dan identitas `server_name`) sesuai kontrak di *Probe*, lalu ganti probe `http` di `providers/nats`.
- [ ] `/sync` setelah build: catat layout paket provider, `cargo xtask provider check`, dan generator `versions.json` di root `AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, keputusan kecil, dan hasil cross check ada di [rationale.md](rationale.md).
