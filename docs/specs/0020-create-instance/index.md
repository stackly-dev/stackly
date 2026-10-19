# 0020. Create instance

**Date**: 2026-10-20
**Status**: Proposed

## Summary

Spec ini menetapkan cara pengguna membuat instance baru dari GUI dan CLI: memilih runtime yang sudah terpasang, memberi nama, memilih port fixed atau automatic per endpoint, mengisi settings dan secret, lalu melihat preflight (pemeriksaan sebelum create) sebelum menekan Create. Form dibangun daemon dari settings schema dan resources manifest provider, jadi GUI tidak punya kode khusus PostgreSQL, NATS, atau fake provider. Preflight dan create memakai satu fungsi evaluasi yang sama di daemon, sehingga GUI, `stackly instance create --dry-run`, dan `stackly instance create --json` selalu memberi kode error dan field error yang sama. Create mem pin paket provider dari catalog (menyalinnya ke folder data), menyimpan secret di Keychain sebelum `initdb`, membuat folder instance yang selalu baru, mendaftarkan instance, lalu menginisialisasi data; semua langkah sebelum inisialisasi bisa dibatalkan dengan bersih, dan data yang sudah ada tidak pernah ditimpa.

## Requirements

**User stories**:

- Sebagai pengguna yang baru memasang PostgreSQL 18, saya ingin membuat instance dengan nama, port, dan password dalam satu sheet yang menunjukkan apa yang akan terjadi sebelum saya menekan Create, supaya saya tidak menemukan konflik port atau folder setelah semuanya setengah jadi.
- Sebagai pengguna, saya ingin port fixed yang bentrok ditolak dengan port bebas yang ditawarkan, bukan diganti diam diam, supaya connection string yang saya tulis di aplikasi saya tetap benar.
- Sebagai pengguna keyboard atau VoiceOver, saya ingin menyelesaikan seluruh alur create tanpa mouse dan tanpa informasi yang hanya ada lewat warna, supaya saya bisa memakai Stackly sama baiknya.
- Sebagai penulis script, saya ingin `stackly instance create` dengan `--json`, `--dry-run`, dan flag yang setara dengan sheet, plus perintah yang bisa saya salin dari GUI, supaya hasil dan error nya sama dengan GUI.
- Sebagai penulis provider (fitur 23, 40 sampai 43), saya ingin form create yang muncul dari settings schema dan resources manifest saja, supaya provider baru tidak butuh perubahan SwiftUI atau Rust.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-008):

- **AC-1** (form): `instance.get_create_form { runtimeId, provider? }` (`recovery refused`, hanya membaca) membalas `CreateForm` seperti *Method IPC*. Provider dipilih menurut *Resolusi provider dan revision*: tanpa `provider`, entri catalog berjenis provider yang software paketnya sama dengan software runtime; nol entri menghasilkan `instance.provider_not_found`, lebih dari satu `instance.provider_ambiguous`, entri planned atau invalid `instance.provider_unavailable`, catalog yang tidak dimuat `catalog.unavailable`, runtime yang tidak ada `instance.runtime_not_found`, runtime yang bukan `installed` `supervisor.runtime_unavailable`, dan `provider` yang software nya berbeda `instance.runtime_mismatch`. Isinya: provider (nama, revision catalog yang dimuat, `manifestSha256`, nama tampil, kategori, sumber), `RuntimeSummary` spec 0018, `suggestedName` menurut *Nama*, `dataPath`, setiap endpoint manifest (urut purpose `client`, `admin`, `monitoring`, lalu nama), setiap field settings schema dalam urutan deklarasi `properties` dengan keenam kelas VAL-012, dan `capabilities` (`CapabilityStatus` spec 0017). Dengan `FakePlatform` dan `FakeRegistry`, method ini tidak menambah `mutations()`, commit registry, tulisan Keychain, atau spawn.
- **AC-2** (preflight): `instance.preflight` (`recovery refused`, hanya membaca, tanpa idempotency key) menerima draft create tanpa nilai secret (params nya tidak punya field nilai, hanya jenis sumber) dan membalas `CreatePreflight { ready, provider, checks, fields, command }`. Baris `runtime`, `name`, satu `endpoint` per endpoint IP, `data`, `disk`, dan satu `secret` per secret field selalu ada dalam urutan itu; baris `provider` dan `settings` hanya muncul saat statusnya bukan `ok`. `ready` bernilai true jika dan hanya jika tidak ada baris `error`. Method ini tidak pernah menulis registry, disk, atau Keychain, tidak pernah spawn, dan tidak pernah memicu dialog Keychain (dibuktikan dengan penghitung fake). Preflight dan precheck `instance.create` memanggil fungsi yang sama, `instance::create::evaluate`: untuk setiap baris tabel test evaluasi, kode error baris `error` pertama preflight sama dengan kode error sinkron `instance.create`, dan `fields` keduanya sama persis.
- **AC-3** (nama, VAL-001): nama yang tidak cocok dengan `InstanceName` spec 0004 (1 sampai 63 karakter, huruf kecil, angka, dan tanda hubung tunggal, diawali huruf) menghasilkan baris `name` error dan `instance.name_invalid` dengan field `name`; nama yang dipakai instance yang tidak retained menghasilkan `instance.name_taken` (detail `existing`); nama milik instance retained boleh dipakai lagi; `displayName` yang dikirim tetapi bukan `DisplayName` valid (1 sampai 128 karakter tanpa karakter kontrol) menghasilkan `instance.name_invalid` dengan field `displayName`. Test ujung ke ujung create memasang guard evidence `covers = ["VAL-001"]` (Follow-up spec 0004).
- **AC-4** (endpoint dan port, BR-004, BR-005, VAL-004 sampai VAL-006, AC-056): endpoint selalu memakai `defaultHost` manifest (loopback, VAL-005); permintaan port untuk endpoint yang tidak ada, untuk endpoint `unix`, ganda, atau di luar 1 sampai 65535 menghasilkan `instance.endpoint_invalid`; endpoint IP tanpa permintaan berarti `Auto` (spec 0010). Untuk port `Fixed` p di alamat A, evaluasi menerapkan *Pemeriksaan port* secara berurutan: konflik `endpoints_conflict` dengan port efektif atau port `Fixed` instance Stackly lain yang tidak retained, termasuk yang `stopped`, menghasilkan `supervisor.endpoint_conflict` (detail `endpoint`, `port`, `otherInstance`); konflik di antara endpoint draft sendiri menghasilkan `instance.endpoint_invalid`; bind percobaan di A:p yang gagal karena dipakai menghasilkan `supervisor.endpoint_in_use` (detail `endpoint`, `address`, `port`); bind wildcard (`0.0.0.0:p` untuk A IPv4, `[::]:p` dengan `IPV6_V6ONLY` untuk A IPv6) atau bind loopback keluarga lain (`[::1]:p` atau `127.0.0.1:p`) yang gagal menghasilkan status `caution` dengan keterangan, bukan error. Dengan listener test di `127.0.0.1:p`, `0.0.0.0:p`, `[::]:p` dual stack, `[::1]:p`, dan UDP di port yang sama, hasil untuk endpoint TCP `127.0.0.1:p` di Mac ini sama dengan tabel *Hasil yang diharapkan* di *Pemeriksaan port*, dan tuple (alamat, transport, port) tidak pernah diperlakukan sebagai satu angka global: TCP dan UDP pada port sama tidak saling konflik. Endpoint `Auto` tidak di probe dan berketerangan bahwa port dipilih saat instance start.
- **AC-5** (saran port, BR-005): untuk port `Fixed` yang konflik atau dipakai, baris endpoint membawa `suggestedPort`: port pertama di atas p (paling banyak `PORT_SUGGESTION_ATTEMPTS` kandidat, di rentang 1024 sampai 65535) yang lolos semua langkah *Pemeriksaan port* tanpa error dan tanpa caution, termasuk terhadap endpoint lain di draft yang sama; tanpa kandidat, field itu kosong. Saran tidak pernah dipakai tanpa aksi pengguna: GUI hanya mengganti nilai lewat tombol "Use {n}" yang mengisi mode `Fixed` dengan angka itu, CLI hanya menulisnya di pesan, dan daemon tidak pernah mengganti port `Fixed` di create atau start.
- **AC-6** (data, storage, disk, VAL-007, VAL-008, BR-002): baris `data` gagal dengan `instance.storage_unsupported` (detail `fsType`, `root`) kalau root data bukan volume lokal ber filesystem `apfs` atau `hfs` menurut `Filesystem::volume_facts`, dan dengan `supervisor.data_unavailable` kalau `instances/` atau `retained/` bukan folder privat milik user. Baris `disk` gagal dengan `instance.insufficient_space` (detail `needed`, `available`) kalau `available_space(instances/)` di bawah `CREATE_MIN_FREE_BYTES`. Di operasi, folder instance selalu diturunkan dari ID baru lewat `DirectoryLayout` dan diperiksa `check_owned_path` sebelum setiap tulisan; setelah `create_instance_layout`, `check_data_root_overlap` terhadap folder data setiap ID yang dikenal (aktif dan retained) yang menemukan tumpukan menghasilkan `instance.data_overlap` (detail `path`, `otherInstance`, rule BR-002) sebelum init.
- **AC-7** (secret): setiap secret field mendapat tepat satu sumber: `generate` (default kalau tidak dikirim), `value`, `copyFrom`, atau `bind`. Field yang tidak ada di schema, bukan secret, atau dikirim dua kali menghasilkan `instance.settings_invalid` dengan path `secrets[<i>].field`; secret field yang ikut di `settings` menghasilkan `instance.settings_invalid` dengan path `settings.<field>`. Di langkah `secrets`, setiap sumber selain `bind` memanggil `SecretService::put` dengan `PutTarget::New { reference: local:instance/<id>/<purpose>, owner, purpose }`, mode `create`, dan interaksi dari pemicu operasi, setelah `record_secret_intent`, sebelum folder dan sebelum `initdb`. Kegagalan store (`locked`, `access_denied`, `busy`, `unsupported`) menghentikan operasi dengan `secret.needs_secret` (item per reference), nilai di luar batas atau berkarakter kontrol untuk field `printableOnly` dengan `secret.invalid_value`, dan sumber `copyFrom` atau `bind` yang tidak terdaftar dengan `secret.not_found`; semuanya sebelum folder dibuat, dan kompensasi menghapus tepat reference yang dibuat operasi itu. Nilai secret tidak pernah ada di journal, sidik jari request, log, event, preflight, atau perintah setara.
- **AC-8** (AC-008, BR-008): create tidak pernah menginisialisasi folder yang sudah ada atau tidak kosong. Folder instance yang sudah ada untuk ID itu (dipaksa test lewat sumber ID tiruan, dengan `instances/<id>/data/PG_VERSION` yatim) dan file yang muncul di folder data antara `create_instance_layout` dan pemeriksaan isi (lewat kait test) sama sama menghasilkan `instance.data_not_empty` (detail `path`) sebelum langkah `init` dimulai: `FakeProcessSupervision` mencatat nol spawn, file yatim dan file sisipan tetap identik, catatan pohon instance ditandai `not_owned` dengan detail `data_not_empty` sehingga kompensasi tidak menghapus folder itu, dan reference secret operasi itu dikompensasi.
- **AC-9** (settings dan plan): sebelum evaluasi, daemon mengisi setiap secret field di settings dengan reference yang akan dipakainya (reference turunan ID untuk `generate`, `value`, dan `copyFrom`, atau reference yang di `bind`), lalu memanggil `SettingsSchema::validate`. Setiap `SettingsError` menjadi satu `FieldError` dengan path `settings.<field>` atau `settings.<field>[<i>].<item>` dan kode tetap spec 0006, dan baris `settings` berstatus error dengan jumlah field bermasalah. Dry run `init_plan` dan `run_plan` dengan ID placeholder, path turunannya, dan port `Fixed` atau `defaultPort` untuk `Auto` yang gagal menghasilkan `instance.settings_invalid` atau `instance.endpoint_invalid` dengan path field yang disebut `PlanError`. Field `managed` yang diisi dan key asing ditolak dengan kode spec 0009 yang dipetakan ke `FieldError`.
- **AC-10** (operasi create): `instance.create` mewajibkan idempotency key, mencari key lebih dulu (replay mengembalikan reply aslinya), lalu menjalankan precheck dengan `evaluate`: hasil dengan baris error ditolak sinkron dengan kode baris error pertama dan semua `fields`, tanpa membuat operasi. Selain itu daemon membuat `InstanceId` baru yang short key nya tidak ada di `known_instance_ids`, lalu `begin` dengan `creates_resource: true`, lock tambahan `LockKey::InstanceName(name)`, dan `BusyPolicy::Queue`. Langkah operasi persis seperti *Operasi `create_instance`*; langkah `register` meng commit `InsertProviderRevision` (kalau belum ada), `InsertInstance`, `InsertConfigRevision`, dan `ReplaceSecretUses` dalam satu batch lewat `ctx.commit` dengan `expected_generation` dari baca yang dipakai untuk memeriksa nama dan konflik endpoint, membaca dan memeriksa ulang lalu mencoba lagi paling banyak lima kali saat `GenerationConflict` atau `DuplicateResource` untuk provider revision. Sepuluh create bersamaan dengan nama sama menghasilkan tepat satu instance, dan dua create bersamaan dengan port `Fixed` sama menghasilkan tepat satu instance dengan port itu; yang kalah gagal sebelum titik tak bisa dibalik dengan `instance.name_taken` atau `supervisor.endpoint_conflict`, tanpa sisa folder, reference, atau baris registry.
- **AC-11** (gagal, cancel, dan terputus): titik tak bisa dibalik operasi create adalah awal langkah `init`. Kegagalan atau cancel sebelum itu menjalankan kompensasi spec 0007 urut terbalik (baris registry lewat kompensator `instance_registration`, pohon instance, lalu reference secret) sehingga tidak ada baris instance, folder instance, reference secret, atau pemakaian secret milik operasi itu yang tersisa, sedangkan folder paket yang di pin tetap ada (BR-023). Kegagalan `init` menutup operasi `failed` dengan `supervisor.init_failed` dan meninggalkan instance `failed` untuk di purge (fitur 54), bukan `needs_recovery`; daemon yang mati saat `init` ditutup `failed` alasan `interrupted` oleh `recover` dan instance nya menjadi `failed interrupted` oleh rekonsiliasi spec 0010; daemon yang mati sebelum `init` dikompensasi otomatis. Semua lewat kebijakan baru `InterruptPolicy::CompensateThenClose` (*Usulan update spec lain*).
- **AC-12** (hasil, REQ-008, BR-001): instance yang berhasil dibuat punya ID stabil buatan daemon, nama, provider revision key exact, runtime ID exact, reference secret per secret field, satu config revision `active` dengan settings tervalidasi (secret sebagai teks reference), endpoint dengan host `defaultHost` dan permintaan port sesuai draft, tujuh baris path dari ID, `startup` `manual`, lifecycle `stopped` setelah `init_succeeded`, dan `instance-manifest.json` yang `data.initializedAt` nya sama dengan `data_initialized_at` registry. Endpoint `unix` yang `socketName` nya merujuk port endpoint `Auto` disimpan dengan penanda `auto` di tempat port sampai spawn pertama (*Socket Unix dengan token port*), dan `socketPath` di `InstanceSummary` bernilai null selama endpoint rujukannya belum punya port efektif.
- **AC-13** (K-1, pin): `catalog::pin_revision` memenuhi kriteria K-1 spec 0017 apa adanya: folder 0700 dan file 0600 lewat `write_file_atomic`, `check_owned_path` sebelum IO, baca ulang dengan `NameCheck::Skip` dan digest dicek, file yang dicakup `manifest_sha256` dibandingkan byte demi byte (sama dilewati, beda menghasilkan `catalog.pin_failed` reason `existing_differs` tanpa menimpa), `versions.json` tidak dibandingkan, panggilan kedua tidak menambah `mutations()`, symlink atau entri asing menghasilkan `catalog.pin_failed` tanpa menimpa atau menghapus, delapan thread yang mem pin revision sama semuanya berhasil dengan isi sama, hasilnya `ProviderRevisionRecord` dengan `source_id`, pin tidak pernah menghapus file, dan kompensasi tidak pernah menghapus folder pinned. Create adalah satu satunya pemanggilnya di fase 02.
- **AC-14** (K-2, sumber): setelah registry `Ready` dan tidak dalam recovery mode, daemon memastikan tepat satu baris `provider_sources` untuk catalog yang dimuat dengan kolom di tabel K-2 spec 0017; restart daemon memakai baris yang sama; `last_sync_at` tetap NULL; kegagalan menulis menambah bagian `degraded` `catalog source not recorded: <kode>` ke subsystem `catalog`, dicoba lagi sekali tepat sebelum pin pertama, dan kalau tetap gagal pin ditolak `catalog.pin_failed` reason `source_unrecorded` tanpa satu file pun ditulis. Daemon tanpa catalog tidak membuat baris.
- **AC-15** (revision dan capability, BR-016): tanpa `provider.revision`, create memakai revision entri catalog yang dimuat saat precheck dan mem pin nya. Dengan `provider.revision` yang sama dengan revision catalog, jalurnya sama. Dengan revision lain, create memakai provider revision yang sudah pinned di registry untuk nama dan revision itu (tepat satu digest dan folder nya cocok, jalur spec 0010), `instance.provider_ambiguous` untuk lebih dari satu, dan `instance.provider_not_found` kalau tidak ada. Paket tanpa capability `multi_instance` saat sudah ada instance tidak retained dari provider yang sama menghasilkan `instance.capability_unsupported` (detail `capability`, `provider`, `existing`).
- **AC-16** (auto port dan race, REQ-010, AC-055): instance dengan endpoint `Auto` yang dibuat lalu di start menyimpan port di `bound` sejak spawn dan menampilkannya `bound: true` setelah readiness (spec 0010); test ujung ke ujung fitur ini membuktikannya lewat `instance.get`. Port `Fixed` yang lolos preflight lalu direbut proses luar sebelum start membuat `instance.start` gagal `supervisor.endpoint_in_use`, port tersimpan tetap sama, dan proses luar tetap hidup.
- **AC-17** (kontrak): method `instance.get_create_form` dan `instance.preflight` (keduanya `recovery refused`), field opsional `provider.revision` di params `instance.create`, DTO di *Method IPC*, newtype `SignedSafeInteger`, enum terbuka baru (masing masing diakhiri `Unrecognized`), dan kode baru di *Kode error* ada di `stackly-protocol` dengan fixture `protocols/fixtures/` untuk setiap varian `Call` dan `Reply` baru, setiap kode, dan nilai asing setiap enum terbuka, terbaca di Rust dan Swift, dengan kenaikan minor `ProtocolVersion::CURRENT` di commit yang sama dan `cargo xtask codegen --check` bersih. Fixture `protocols/fixtures/instance/create-form-<fake|postgresql|nats>.json` dibangkitkan dari paket nyata dan dijaga test Rust.
- **AC-18** (CLI): `stackly instance create <name> --runtime <id|software@versi>` dengan flag di *CLI* membangun params dari `instance.get_create_form`, mengirim `instance.preflight` untuk `--dry-run` (exit 0 kalau `ready`, selain itu exit dari kategori error baris error pertama) dan `instance.create` untuk selain itu, mengikuti kontrak output, exit code, progress, dan `--wait` spec 0008. `--set` dikonversi menurut jenis field; nilai yang tidak bisa dikonversi dikirim sebagai teks sehingga daemon yang menolaknya. Sumber secret hanya lewat `--secret <field>=<sumber>` dan tidak pernah berupa nilai di argv. `--start` menunggu create selesai lalu mengirim `instance.start`. Perintah setara yang dibalas preflight, setelah dipecah seperti shell, diparse kembali oleh CLI menjadi draft yang sama untuk setiap fixture `protocols/fixtures/instance/commands.json`.
- **AC-19** (AC-028): untuk matriks input buruk di *Critical test scenarios* (nama, port, settings, secret, runtime, provider), params yang sama yang dikirim lewat `stackly-ipc` `Client` (jalur yang dipakai GUI) dan lewat `stackly instance create --json` menghasilkan `ErrorBody` dengan `code`, `category`, `details`, dan `fields` yang sama, dan stdout CLI tepat satu baris envelope tanpa progress.
- **AC-20** (sheet New Instance, PAGE-04): sheet mengikuti mockup dan *Sheet New Instance*: header, filter kategori, picker runtime, nama, satu baris port per endpoint IP dengan Fixed atau Automatic, field settings dari schema, toggle "Start after creating", daftar preflight, perintah setara dengan tombol Copy, Cancel, dan Create yang hanya aktif saat preflight terakhir `ready` untuk nilai form saat ini dan daemon tersambung. Sheet dibuka dari toolbar dan empty state Instances, tombol "New Instance…" di panel detail Catalog (nonaktif dengan alasan saat belum ada runtime terpasang untuk provider itu), dan perintah New Instance… (⌘N). Setelah Create, sheet menampilkan progress tahap operasi; sukses menutup sheet, mengirim `instance.start` kalau toggle aktif, dan memilih instance baru di Instances; gagal mengembalikan form dengan error di baris yang bersangkutan.
- **AC-21** (form dari schema, VAL-012, AC-029 bagian form UI): `SchemaFormModel` dan view nya merender setiap `FormField` hanya dari DTO: teks, integer, boolean, pilihan, dan daftar item; field secret lewat `SecretField` spec 0011 dengan Generate sebagai default; field `immutableAfterInit` diberi tanda "Set once"; field advanced di grup Advanced yang tertutup kecuali ada error di dalamnya; field managed terkunci dengan "Set by Stackly"; dan jenis yang tidak dikenal sebagai baris read only. Test Swift membangun form dari ketiga fixture `create-form-*.json` (fake, PostgreSQL, NATS) dan mendapat satu baris per field dengan kontrol yang benar; untuk NATS test itu memeriksa `serverName` sebagai field managed yang terkunci, `jetstream` sebagai toggle, tiga integer advanced (`jetstreamMaxMemoryBytes`, `jetstreamMaxFileBytes`, `maxPayloadBytes`) di grup Advanced dengan batasnya, dan dua baris port (`client`, `monitoring`) dengan Fixed atau Automatic (dipakai ulang AC-13 spec 0023); dan `cargo xtask check-deps` menolak nama atau ID provider di kode `CreateInstance/` Swift dan `instance::create` Rust.
- **AC-22** (AC-027, NFR-012): seluruh alur create bisa diselesaikan dengan keyboard (urutan fokus di *Aksesibilitas*, Return untuk Create saat aktif, Escape untuk Cancel) dan VoiceOver (label, nilai, error, dan status setiap baris preflight terbaca sebagai teks), status preflight dibedakan oleh bentuk simbol dan teks, bukan hanya warna, tidak ada informasi yang hanya muncul saat hover, teks mengikuti Dynamic Type tanpa terpotong di lebar sheet minimum, dan semua pasangan warna lolos WCAG AA di light, dark, dan Increase Contrast; dibuktikan evidence manual keyboard dan VoiceOver di Mac ini.
- **AC-23** (fuzz, VAL-007): fuzz target `instance_path` (harness `stackly_core::instance::fuzzing::owned_path`) memanggil `check_owned_path` dan `check_data_root_overlap` dengan path acak di atas pohon fixture yang memuat symlink ke luar root, loop symlink, nama Unicode, dan folder dalam, dan memeriksa invariant di *Fuzz target*; ia berjalan 10 menit di Mac ini tanpa crash dengan evidence `covers = ["VAL-007"]`, dan seed nya dijalankan ulang sebagai test regresi di `stackly-core`.
- **AC-24** (kinerja): dengan 10 instance terdaftar di Mac ini, `instance.create` dijawab dalam 500 ms (NFR-006) dan `instance.preflight` dalam 300 ms pada persentil 95 untuk paket dengan dua endpoint IP tanpa pencarian saran.
- **AC-25** (PostgreSQL nyata): di Mac ini, PostgreSQL 18 dan 17 masing masing dibuat lewat CLI dan lewat GUI dengan password bangkitan dan port fixed, preflight menunjukkan konflik nyata saat port dipakai listener test, instance hasilnya bisa di start sampai `ready` (spec 0019), dan alur GUI dijalankan penuh dengan keyboard dan VoiceOver. Tanpa artefak di cache runtime xtask hasilnya `BLOCKED` dengan alasan dan owner.
- **AC-26** (NFR-009): review fresh model atas jalur create (params IPC, path, port, secret, kompensasi) tidak meninggalkan temuan critical atau high yang belum diselesaikan, dan test keamanan di *Critical test scenarios* (nama berbahaya, path traversal di nama, secret di argv dan log, symlink di folder providers dan instances) lulus.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md): dua method baca baru (`instance.get_create_form` dan `instance.preflight`) di atas satu evaluator core yang juga menjadi precheck `instance.create`; form bertipe dari settings schema dalam urutan deklarasi; revision catalog saat create yang di pin pada saat itu; port fixed yang konflik ditolak dengan saran yang hanya dipakai lewat aksi pengguna, dengan probe tepat di alamat endpoint sebagai error dan probe wildcard serta loopback keluarga lain sebagai caution; titik tak bisa dibalik di awal `init` dengan kompensator pendaftaran registry, sehingga semua langkah sebelumnya bisa dibatalkan; kegagalan init yang ditutup `failed` lewat `InterruptPolicy::CompensateThenClose`; "Start after creating" sebagai rangkaian di client; Start at login ditunda ke fitur 31; perintah setara dibangun satu fungsi di `stackly-protocol`; sheet yang tetap terbuka selama operasi; dan penanda `auto` untuk socket Unix yang merujuk port automatic.

Daemon adalah satu satunya yang memvalidasi dan memutuskan; GUI dan CLI hanya mengirim draft dan menampilkan hasil evaluasi yang sama.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya: tidak ada `unwrap()` di luar test, kontrol native SwiftUI menggantikan tiruan HTML di mockup, dan validasi pattern tetap di daemon, bukan di Swift.

## Feature design

### Trace PRD

| ID PRD | Dipenuhi oleh |
| --- | --- |
| REQ-008 (create terkontrol) | AC-10, AC-11, AC-12, AC-8 |
| REQ-010 (endpoint dan konflik resource) | AC-4, AC-5, AC-12, AC-16 |
| REQ-029 (bagian form) | AC-1, AC-21 |
| PAGE-04 (Create Instance) | AC-20, AC-21, AC-22; autostart (Start at login) dibangun fitur 31 |
| BR-002 (data root tidak bertumpuk) | AC-6 |
| BR-004 (alamat, transport, port, wildcard) | AC-4 |
| BR-005 (fixed konflik error, auto hanya pilihan manager) | AC-4, AC-5, AC-16 |
| BR-008 (hanya folder baru milik operasi) | AC-8, AC-11 |
| VAL-001 | AC-3 |
| VAL-004, VAL-005, VAL-006 | AC-4, AC-5 |
| VAL-007 | AC-6, AC-23 |
| VAL-008 | AC-6, AC-15 (runtime), spec 0010 (lock saat init) |
| VAL-012 | AC-1, AC-21 |
| AC-008 | AC-8 |
| AC-027 | AC-22, AC-25 |
| AC-029 (bagian form UI) | AC-21 |
| AC-055 | AC-16 |
| AC-056 | AC-4 |
| NFR-009 | AC-26 |
| NFR-012 | AC-22 |
| AC-028 (bukti penuh, Follow-up spec 0008) | AC-18, AC-19 |

### Batas dengan spec lain

| Bagian | Pemilik |
| --- | --- |
| `InstanceName`, `Endpoint`, `endpoints_conflict`, `resolve_port`, `create_instance_layout`, `check_owned_path`, `check_data_root_overlap`, manifest instance | spec 0004 (validasi endpoint di `Instance::new` dan `volume_facts` ditambah lewat *Usulan update spec lain*) |
| `WriteOp`, baca registry, `provider_sources` | spec 0005 (tambahan di *Usulan update spec lain*) |
| Engine operasi, kompensasi, `claim_resource`, `record_secret_intent`, `ctx.commit` | spec 0007 (`CompensateThenClose` dan detail `data_not_empty` lewat usulan) |
| Settings schema, `SettingsSchema::validate`, plan, `PlanError` | spec 0009 (urutan deklarasi field lewat usulan) |
| `execute_init`, `PortProbe`, start, `InstanceSummary`, method instance minimal, area `instance` dan `supervisor` | spec 0010 (create diperluas secara additive di sini) |
| `SecretService::put`, `SecretField`, reference, kode `secret.*` | spec 0011 |
| Catalog read model, `CapabilityStatus`, kontrak K-1 dan K-2 | spec 0017 (dibangun di sini) |
| `RuntimeSummary`, `RuntimeStore`, `available_space` | spec 0018 |
| Paket PostgreSQL, `printableOnly`, `pin_package` | spec 0019 |
| Paket NATS dan probe readiness NATS | fitur 23 |
| Halaman Instances dan Overview, empty state Instances, tata letak toolbar, `ShellNavigator` | fitur 22 (fitur ini memberi aksi `NewInstanceAction` lewat environment dan memanggil `ShellNavigator.show`; sebelum fitur 22 dibangun, aksi dipasang di daftar sementara spec 0013) |
| Summary, connection string, socket path untuk connection | fitur 21 |
| Start at login, `startup` `at_login` | fitur 31 |
| Purge instance yang `failed` | fitur 54 |
| Komponen dasar (checklist preflight, sheet dengan footer, progress bertahap) | fitur 67 (dipakai kalau sudah ada; kalau belum, fitur ini membangun versi di `CreateInstance/` dan fitur 67 mengadopsinya) |
| Peta perintah dan shortcut | fitur 69 (⌘N didaftarkan di peta itu, atau `CommandGroup` sementara dengan `// provisional: F69`) |

### Alur create

1. Pengguna memilih runtime di sheet (dari `RuntimeStore`) atau CLI menerima `--runtime`.
2. Client memanggil `instance.get_create_form { runtimeId }` dan mendapat provider, endpoint, field, dan nama saran. GUI mengisi draft awal: nama saran, setiap endpoint IP `Fixed` dengan `defaultPort`, nilai default setiap field, dan setiap secret field `generate`.
3. Setiap perubahan draft (GUI, jeda `PREFLIGHT_DEBOUNCE`) atau `--dry-run` (CLI) memanggil `instance.preflight`, yang menjalankan `gather` lalu `evaluate` dan membalas baris preflight, field error, saran port, dan perintah setara.
4. Create mengirim `instance.create` dengan draft dan `SecretInput` lengkap. Precheck menjalankan `gather` dan `evaluate` yang sama; kalau ada error, ditolak sinkron. Kalau tidak, operasi `create_instance` dimulai.
5. Operasi menjalankan `validate`, `pin`, `secrets`, `layout`, `register`, dan `init`, lalu selesai `succeeded`.
6. Kalau "Start after creating" aktif (GUI) atau `--start` (CLI), client mengirim `instance.start` setelah create `succeeded`.

### Model data

Tidak ada tabel atau kolom baru dan tidak ada migrasi. Yang ditulis create:

| Data | Tempat | Isi | Ditulis |
| --- | --- | --- | --- |
| folder paket pinned | `providers/<namespace>/<nama>/<revision>/<manifest_sha256>/` | salinan file paket catalog | langkah `pin` (K-1); tidak pernah dihapus create |
| `provider_sources` | registry | satu baris per catalog yang dimuat (K-2) | start daemon, `InsertProviderSource` (usulan spec 0005) |
| `provider_revisions` | registry | `ProviderRevisionRecord` dengan `source_id` | batch `register`, hanya kalau belum ada |
| `secret_references` dan audit | registry | baris reference baru, status `present` | `SecretService::put` di langkah `secrets` (transaksi nya sendiri) |
| `instances`, `instance_endpoints`, `instance_paths` | registry | aggregate `Instance::new` | batch `register` |
| `config_revisions` | registry | config revision awal `active` | batch `register` (`InsertConfigRevision`) |
| `secret_reference_uses` | registry | peta field ke reference | batch `register` (`ReplaceSecretUses`) |
| folder instance | `instances/<id>/` dan `logs/instances/<id>/` | manifest, `data/`, `config/`, `config-history/`, `temp/` | langkah `layout` (`create_instance_layout`) |
| isi folder data | `instances/<id>/data/` | hasil init provider | langkah `init` (`execute_init` spec 0010) |
| journal operasi | registry | input draft tanpa nilai secret, langkah, catatan dibuat | engine spec 0007 |

`Instance::new` dipanggil dengan: `id` dari handler, `name`, `display_name`, `provider_revision` dari `ValidatedPackage::key()`, `software` paket, `runtime_id`, `data_line` dan `data_version` dari record runtime, `endpoints` dari *Endpoint di aggregate*, `startup: StartupPolicy::Manual`, `created_at` dari `Clock` di langkah `register`, dan `created_by` operasi create. Lifecycle awalnya `uninitialized`.

### Endpoint di aggregate

- Endpoint `tcp` dan `udp` manifest menjadi `Endpoint::Ip { name, transport, address: defaultHost, request, bound: None }` dengan `request` dari draft (`Fixed(port)` atau `Auto`; tanpa permintaan berarti `Auto`).
- Endpoint `unix` menjadi `Endpoint::Unix { name, socket }`, dengan `socket` hasil render `socketName` manifest memakai port efektif endpoint yang dirujuknya. Lihat *Socket Unix dengan token port* untuk rujukan ke endpoint `Auto`.
- `Instance::new` menolak nama endpoint ganda, `bound` yang terisi, socket Unix ganda, dan endpoint IP draft yang saling konflik (`endpoints_conflict` di antara port `Fixed` instance itu sendiri, rule BR-004) (*Usulan update spec lain*, spec 0004).

### Socket Unix dengan token port

`socketName` manifest boleh memuat `${endpoint.<tcp>.port}` (spec 0009), misalnya `.s.PGSQL.${endpoint.client.port}` milik PostgreSQL. Plan selalu merender socket dari paket pinned dan port saat itu, jadi nilai di registry hanya untuk tampilan dan untuk aturan socket ganda.

- Port rujukan `Fixed`: nama dirender dengan port itu saat create.
- Port rujukan `Auto`: token port diganti teks `auto` (misalnya `.s.PGSQL.auto`, charset `SocketName` dan selalu muat karena spec 0009 menghitung token port 5 byte). Batch `spawned` spec 0010 yang mengisi `bound` juga merender ulang nama setiap endpoint Unix yang `socketName` paket pinned nya merujuk endpoint itu, lewat setter domain baru `Instance::set_socket_name(endpoint, SocketName)` (*Usulan update spec lain*, spec 0004 dan 0010).
- `InstanceSummary.endpoints[].socketPath` (spec 0010) bernilai null selama endpoint rujukannya tidak punya port efektif, sehingga penanda `auto` tidak pernah tampil sebagai path.

### Resolusi provider dan revision

| Masukan | Hasil |
| --- | --- |
| `provider.revision` kosong, catalog `Loaded`, entri provider dengan nama itu | paket entri catalog, sumber `bundled_catalog` atau `development_catalog`, di pin di langkah `pin` |
| `provider.revision` kosong, catalog `NotConfigured` atau `Unavailable` | `catalog.unavailable` |
| nama tidak ada di catalog (dan revision kosong) | `instance.provider_not_found` |
| entri `Planned` atau `Invalid` | `instance.provider_unavailable` (detail `state` `planned` atau `invalid`) |
| `provider.revision` sama dengan revision entri catalog | sama dengan baris pertama |
| `provider.revision` lain, registry punya tepat satu provider revision untuk nama dan revision itu, folder pinned nya terbaca dengan `NameCheck::Skip` dan digest nya sama | paket pinned, sumber `pinned`, tanpa pin |
| lebih dari satu digest untuk nama dan revision itu | `instance.provider_ambiguous` |
| tidak ada | `instance.provider_not_found` |

`instance.get_create_form` selalu memakai baris pertama (form untuk instance baru selalu dari catalog). Pemilihan entri dari runtime: entri provider yang `ValidatedPackage::software()` sama dengan software runtime; di fase 02 hanya namespace `official`, jadi paling banyak satu. Fitur 37 yang nanti memakai `provider` untuk memilih di antara beberapa.

### Method IPC

Semua method lewat handshake dan peer user yang sama (spec 0006). Port di wire bertipe `u16` seperti spec 0010.

| Method | Recovery | Input | Output | Error utama |
| --- | --- | --- | --- | --- |
| `instance.get_create_form` | refused | `GetCreateFormParams { runtimeId: Uuid, provider: Option<String> }` | `CreateForm` | `instance.runtime_not_found`, `supervisor.runtime_unavailable`, `instance.provider_not_found`, `instance.provider_ambiguous`, `instance.provider_unavailable`, `instance.runtime_mismatch`, `catalog.unavailable` |
| `instance.preflight` | refused | `PreflightCreateParams` | `CreatePreflight` | `request.invalid_params` (hanya batas ukuran), `catalog.unavailable` dan `instance.provider_*` tidak pernah sebagai error method melainkan sebagai baris `provider` |
| `instance.create` (diperluas) | refused (spec 0010) | params spec 0010, dengan `provider.revision` menjadi opsional | `InstanceActionAccepted` (operasi `create_instance`) | sinkron: kode baris error pertama `evaluate` dengan `fields`; operasi: lihat *Operasi `create_instance`* |

DTO baru di modul `instance` `stackly-protocol`:

```rust
pub struct GetCreateFormParams { pub runtime_id: Uuid, #[serde(default)] pub provider: Option<String> }
pub struct CreateForm {
    pub provider: CreateProvider,
    pub runtime: RuntimeSummary,                    // spec 0018
    pub suggested_name: Option<String>,
    pub data_path: String,                          // "instances/<new instance id>/data", relatif ke root data
    pub endpoints: Vec<CreateEndpoint>,
    pub fields: Vec<FormField>,
    pub capabilities: Vec<CapabilityStatus>,         // spec 0017, urutan capability spec 0017
}
pub struct CreateProvider { pub name: String, pub revision: String, pub manifest_sha256: String,
                            pub display_name: String, pub category: ProviderCategory, pub source: CreateProviderSource }
pub enum CreateProviderSource { BundledCatalog, DevelopmentCatalog, Pinned, Unrecognized }
pub struct CreateEndpoint { pub name: String, pub transport: EndpointTransport, pub purpose: EndpointPurpose,
                            pub host: Option<String>, pub default_port: Option<u16>, pub socket_name: Option<String> }
pub enum EndpointTransport { Tcp, Udp, Unix, Unrecognized }
pub enum EndpointPurpose { Client, Admin, Monitoring, Unrecognized }
pub struct FormField {
    pub key: String, pub title: String, #[serde(default)] pub description: Option<String>,
    pub kind: FieldKind, pub required: bool, pub secret: bool, pub printable_only: bool, pub managed: bool,
    pub visibility: FieldVisibility, #[serde(default)] pub apply: Option<FieldApply>,
    #[serde(default)] pub default: Option<SettingValue>,
    #[serde(default)] pub min_length: Option<Count>, #[serde(default)] pub max_length: Option<Count>,
    #[serde(default)] pub minimum: Option<SignedSafeInteger>, #[serde(default)] pub maximum: Option<SignedSafeInteger>,
    pub choices: Vec<String>,
    #[serde(default)] pub min_items: Option<Count>, #[serde(default)] pub max_items: Option<Count>,
    #[serde(default)] pub item_key: Option<String>, pub items: Vec<FormItemField>,
}
pub struct FormItemField { pub key: String, pub title: String, #[serde(default)] pub description: Option<String>,
                           pub kind: FieldKind, pub required: bool, #[serde(default)] pub default: Option<ScalarSetting>,
                           #[serde(default)] pub min_length: Option<Count>, #[serde(default)] pub max_length: Option<Count>,
                           #[serde(default)] pub minimum: Option<SignedSafeInteger>, #[serde(default)] pub maximum: Option<SignedSafeInteger>,
                           pub choices: Vec<String> }
pub enum FieldKind { Text, Integer, Boolean, Choice, List, Unrecognized }
pub enum FieldVisibility { Basic, Advanced, Unrecognized }
pub enum FieldApply { Reload, Restart, ImmutableAfterInit, Unrecognized }
pub enum SettingValue { Text(String), Integer(SignedSafeInteger), Boolean(bool), List(Vec<SettingItem>) }   // tag type, content
pub enum ScalarSetting { Text(String), Integer(SignedSafeInteger), Boolean(bool) }                          // tag type, content
pub struct SettingItem { pub entries: Vec<SettingEntry> }
pub struct SettingEntry { pub key: String, pub value: ScalarSetting }

pub struct PreflightCreateParams {
    pub name: String, #[serde(default)] pub display_name: Option<String>,
    pub provider: ProviderSelection, pub runtime_id: Uuid,
    pub endpoints: Vec<EndpointRequest>,             // bentuk spec 0010: { name, port: { type: fixed, content: u16 } | { type: auto } }
    pub settings: SettingsDocument,                  // object JSON settings yang sama dengan instance.create (spec 0010)
    pub secrets: Vec<SecretPlan>,
    #[serde(default)] pub start: bool,               // hanya untuk menambah --start di perintah setara
}
pub struct ProviderSelection { pub name: String, #[serde(default)] pub revision: Option<String> }
pub struct SecretPlan { pub field: String, pub source: SecretSource }
pub enum SecretSource { Generate, Value, CopyFrom(String), Bind(String) }   // tag type, content; tanpa nilai

pub struct CreatePreflight { pub ready: bool, #[serde(default)] pub provider: Option<ResolvedProvider>,
                             pub checks: Vec<PreflightCheck>, pub fields: Vec<FieldError>, pub command: String }
pub struct ResolvedProvider { pub name: String, pub revision: String, pub manifest_sha256: String }
pub struct PreflightCheck {
    pub id: String,                                  // provider, runtime, name, endpoint.<n>, settings, data, disk, secret.<field>
    pub kind: PreflightCheckKind, pub status: PreflightStatus,
    pub title: String, pub detail: String,           // bahasa Inggris, paling banyak 200 karakter, lewat Redactor
    #[serde(default)] pub subject: Option<String>,   // nama endpoint atau key field
    #[serde(default)] pub suggested_port: Option<u16>,
    #[serde(default)] pub error: Option<ErrorBody>,  // hanya untuk status error; kode sama dengan error sinkron create
}
pub enum PreflightCheckKind { Provider, Runtime, Name, Endpoint, Settings, Data, Disk, Secret, Unrecognized }
pub enum PreflightStatus { Ok, Caution, Error, Unrecognized }
```

- Params `instance.create` tetap bentuk spec 0010 (`name`, `displayName?`, `provider: { name, revision? }`, `runtimeId`, `endpoints`, `settings`, `secrets: [{ field, input: SecretInput }]`); hanya `provider.revision` yang menjadi opsional (minor). Kalau spec 0010 langkah 12 belum memberi nama tipe untuk object settings, namanya `SettingsDocument`.
- `SignedSafeInteger` adalah newtype baru di `wire.rs` untuk `i64` di antara minus dan plus `MAX_SAFE_INTEGER`, dipetakan `typeshare.toml` ke `Int64` Swift, sama polanya dengan `Count`.
- `Validate` untuk `PreflightCreateParams` dan params `instance.create` hanya memeriksa batas ukuran (nama paling banyak 256 byte, nama tampil 512 byte, endpoint 16, secret 64, key field 64 byte, reference 255 byte, provider 127 byte, revision 64 byte). Format nama, port, dan settings diperiksa `evaluate`, supaya preflight tetap membalas semua baris saat satu field salah.
- Kalau paket tidak bisa ditentukan (baris `provider` error), baris `endpoint`, `settings`, dan `secret` tidak ada karena jumlahnya bergantung pada paket, dan baris `runtime` hanya memeriksa keberadaan dan state runtime. Tidak ada status "dilewati"; baris yang tidak bisa dievaluasi tidak dikirim.

### Evaluator create

Modul baru `stackly-core::instance::create`:

```rust
pub struct CreateDraft { pub name: String, pub display_name: Option<String>, pub provider: ProviderChoice,
                         pub runtime_id: RuntimeId, pub endpoints: Vec<(EndpointName, PortRequest)>,
                         pub settings: serde_json::Value, pub secrets: Vec<(String, SecretSourceKind)>, pub start: bool }
pub struct CreateFacts { /* catalog, provider revisions, runtime, instances, known ids, port states, volume, space, secret store */ }
pub fn gather(deps: &CreateDeps, draft: &CreateDraft) -> Result<CreateFacts, GatherError>;   // satu satunya IO
pub fn evaluate(draft: &CreateDraft, facts: &CreateFacts) -> Evaluation;                      // fungsi murni
pub struct Evaluation { pub checks: Vec<Check>, pub field_issues: Vec<FieldIssue>, pub resolved: Option<ResolvedCreate> }
pub fn equivalent_draft_for_command(draft: &CreateDraft, facts: &CreateFacts) -> CommandDraft;
```

- `gather` hanya membaca: catalog di memori, `Registry` (`provider_revision`, `runtime`, `runtimes`, `instances(include_retained: false)`, `known_instance_ids`, reference secret untuk sumber `copyFrom` dan `bind`), paket pinned lewat `read_package` kalau jalur `pinned`, `symlink_metadata` untuk executable runtime dan folder `instances/` serta `retained/`, `Filesystem::volume_facts`, `available_space`, `PortProbe` (bind percobaan yang langsung ditutup), `SecretStore::state` dan `SecretStore::probe` (interaksi `Never`). Tidak ada tulisan, spawn, atau dialog.
- `evaluate` murni dan diuji dengan tabel kasus tanpa fake. Urutan baris dan aturan setiap baris ada di tabel di bawah; field issue diurutkan menurut path.
- `ResolvedCreate` (paket, runtime, endpoint domain, settings tervalidasi, rencana secret, nama exact) hanya ada kalau tidak ada error; operasi memakainya, preflight membuangnya.

| Baris | Aturan (berhenti di gagal pertama dalam baris) | Status dan kode |
| --- | --- | --- |
| `provider` | *Resolusi provider dan revision*; `multi_instance` (AC-15) | error dengan kode tabel itu atau `instance.capability_unsupported`; selain itu `ok` dan baris disembunyikan |
| `runtime` | record ada; software sama dengan paket; state `installed`; `runtime.executable` dan setiap `additionalExecutables` adalah file biasa yang bisa dieksekusi di dalam root runtime; install mode | `instance.runtime_not_found`, `instance.runtime_mismatch`, `supervisor.runtime_unavailable`, `instance.runtime_incompatible`; external menjadi `caution`; selain itu `ok` |
| `name` | parse `InstanceName`; tidak dipakai instance tidak retained | `instance.name_invalid`, `instance.name_taken` |
| `endpoint.<n>` | *Pemeriksaan port* untuk setiap endpoint IP (urut seperti form) | `instance.endpoint_invalid`, `supervisor.endpoint_conflict`, `supervisor.endpoint_in_use`, atau `caution` |
| `settings` | AC-7 (bagian settings), `SettingsSchema::validate`, dry run plan (AC-9) | `instance.settings_invalid` atau `instance.endpoint_invalid`; baris disembunyikan saat `ok` |
| `data` | `instances/` dan `retained/` folder privat milik user; `volume_facts` lokal dengan `apfs` atau `hfs` | `supervisor.data_unavailable`, `instance.storage_unsupported` |
| `disk` | `available_space(instances/)` minimal `CREATE_MIN_FREE_BYTES` | `instance.insufficient_space` |
| `secret.<field>` | sumber AC-7; backend store bukan `unsupported`; `copyFrom` atau `bind` terdaftar (dan `copyFrom` `present`) | `secret.store_unsupported`, `secret.invalid_reference`, `secret.not_found`; store `locked` atau `bind` tanpa nilai menjadi `caution` |

Error sinkron `instance.create` adalah `ErrorBody` dari baris error pertama menurut urutan di atas, ditambah `fields` berisi semua field issue. Pesan field issue dibuat evaluator dari batas schema dan tidak pernah memuat nilai settings teks atau secret.

### Pemeriksaan port

Untuk endpoint IP dengan port `Fixed` p di alamat A (`defaultHost`) dan transport T, berurutan:

| Langkah | Pemeriksaan | Gagal |
| --- | --- | --- |
| 1 | p di 1 sampai 65535 dan endpoint ada di manifest | `instance.endpoint_invalid` (error) |
| 2 | tidak konflik dengan endpoint lain di draft (`endpoints_conflict`) | `instance.endpoint_invalid` (error) |
| 3 | tidak konflik dengan port efektif atau port `Fixed` setiap instance tidak retained di registry (`endpoints_conflict`, BR-004) | `supervisor.endpoint_conflict` (error) |
| 4 | `PortProbe` bind T di A:p tanpa `SO_REUSEADDR` dan `SO_REUSEPORT` | `supervisor.endpoint_in_use` (error) |
| 5 | bind T di wildcard keluarga yang sama (`0.0.0.0:p` atau `[::]:p` dengan `IPV6_V6ONLY` 1) | `caution`: port dipakai di alamat lain Mac ini |
| 6 | kalau A loopback, bind T di loopback keluarga lain (`[::1]:p` atau `127.0.0.1:p`) | `caution`: client yang memakai `localhost` bisa sampai ke server lain |

- Probe yang gagal karena alasan lain (misalnya `EACCES`) menjadi `caution` "couldn’t check this port", tidak pernah `ok`.

*Hasil yang diharapkan* untuk endpoint TCP `127.0.0.1:p` (semantik bind BSD: bind alamat spesifik tanpa `SO_REUSEADDR` gagal saat socket lain memegang wildcard di port itu, termasuk wildcard IPv6 dual stack):

| Listener lain | Hasil |
| --- | --- |
| TCP `127.0.0.1:p` | error `supervisor.endpoint_in_use` (langkah 4) |
| TCP `0.0.0.0:p` | error `supervisor.endpoint_in_use` (langkah 4) |
| TCP `[::]:p` dual stack | error `supervisor.endpoint_in_use` (langkah 4) |
| TCP `[::1]:p` | `caution` loopback keluarga lain (langkah 6) |
| TCP di alamat LAN Mac ini, port p | `caution` alamat lain (langkah 5) |
| UDP `127.0.0.1:p` | `ok` |

Kalau test native milestone 4 menunjukkan perilaku macOS yang lain dari tabel ini, test itu gagal dan keputusan probe kembali ke `/architect` update spec ini; implementasi tidak menyesuaikan tabel sendiri.
- Langkah 4 sampai 6 hanya jalan kalau langkah 1 sampai 3 lolos. Pencarian saran (AC-5) memakai langkah 2 sampai 6 untuk setiap kandidat.
- Endpoint `Auto` hanya menjalankan langkah 1 dan berstatus `ok` dengan keterangan bahwa port dipilih saat start; alokasi dan probe nya milik start (spec 0010).
- Precheck create dan langkah `validate` menjalankan langkah yang sama; langkah `register` hanya mengulang langkah 2 dan 3 atas data registry yang dibaca bersama generation nya.

### Nama

- `suggestedName`: `<software>-<line>` dengan titik di line diganti tanda hubung (misalnya `postgresql-18`, `mysql-8-4`); kalau dipakai instance tidak retained, tambahkan `-2` sampai `-99`; bagian software dipotong supaya hasil paling panjang 63 karakter. Kalau semuanya dipakai, kosong.
- Validasi memakai `InstanceName::parse`; nama tidak pernah menjadi path atau argumen shell, karena path diturunkan dari ID.

### Operasi `create_instance`

Handler `instance.create`: key wajib, pencarian key (replay), precheck (`gather` lalu `evaluate`; error berarti `Reject`), lalu `InstanceId::generate()` diulang sampai short key nya tidak ada di `known_instance_ids`, lalu `begin(BeginRequest { kind: create_instance, resource: Instance(id), creates_resource: true, extra_locks: [InstanceName(name)], busy: Queue, expected_revision: None, origin: Request(..), input, work, precheck })`. `input` adalah `JournalPayload` berisi draft tanpa nilai secret (sumber `value` dicatat sebagai `value` saja); kalau hasilnya melewati `INPUT_MAX`, settings diganti `{ "settingsOmitted": true, "settingsSha256": "<hex>" }`.

| Langkah | `repeatable` | `reversible` | Isi | Gagal |
| --- | --- | --- | --- | --- |
| `validate` | ya | ya | `gather` dan `evaluate` ulang di bawah lock | kode baris error pertama |
| `pin` | ya | ya | untuk sumber catalog: `ensure_source` kalau sumber belum tercatat (K-2), lalu `pin_revision` (K-1); sumber `pinned` dilewati | `catalog.pin_failed` |
| `secrets` | tidak | ya | per secret field selain `bind`: `record_secret_intent(ref)`, `SecretService::put(PutTarget::New, input, Create, None, interaksi pemicu)`, `confirm_created` | `secret.needs_secret`, `secret.invalid_value`, `secret.conflict`, `secret.not_found`, `secret.store_*` lain |
| `layout` | tidak | ya | `claim_resource`, `InstanceManifest::new`, `create_instance_layout(platform, prepared, manifest, known)`, `check_data_root_overlap`, pemeriksaan folder data (folder, bukan symlink, milik user, privat, nol entri), lalu `confirm_created`; `AlreadyExists` atau isi tidak kosong berarti `not_created(data_not_empty)`, `ShortKeyTaken` berarti `not_created(short_key_taken)` | `instance.data_not_empty`, `instance.data_overlap`, `instance.short_key_taken`, `supervisor.data_unavailable` |
| `register` | tidak | ya | `record_custom_intent(instance_registration, <id>)`, lalu paling banyak lima kali: baca registry dan generation, periksa nama dan langkah 2 dan 3 *Pemeriksaan port*, `ctx.commit(FinalBatch { expected_generation, ops: [InsertProviderRevision?, InsertInstance, InsertConfigRevision, ReplaceSecretUses] })`; `GenerationConflict` dan `DuplicateResource` provider revision diulang; sukses berarti `confirm_created` | `instance.name_taken`, `supervisor.endpoint_conflict`, `registry.generation_conflict` setelah lima kali, kode registry lain |
| `init` | tidak | tidak (titik tak bisa dibalik) | `execute_init` spec 0010 dengan interaksi pemicu: lock instance, `init_begin`, langkah `InitPlan`, `init_succeeded`, manifest dengan `data.initializedAt` | `supervisor.init_failed`, `supervisor.data_in_use`, `secret.*` dari `resolve_for_start` |

- `finalize` tidak menulis apa pun selain yang ditambahkan engine; `result.resources` berisi `Instance(id)`.
- Config revision awal: `ConfigRevisionId::generate()`, `values` dari `SettingsValues::to_json()` (secret sebagai teks reference), status `active`, `created_by_operation` operasi ini, `created_at` waktu `Clock` di `register`, tanpa `rendered_sha256` dan `previous` (spec 0010).
- `ReplaceSecretUses { user: Instance(id), uses }` memetakan setiap secret field ke reference hasil `put` atau reference yang di `bind`. `bind` tidak memanggil `put`, karena pemakaiannya ditulis di batch ini (klarifikasi atas spec 0011).
- Kompensator kustom `instance_registration` (didaftarkan `stacklyd`): baca instance; tidak ada berarti `Compensated`; lifecycle `uninitialized` dan `created_by` operasi ini berarti commit `WriteOp::DeleteUninitializedInstance { id, expected, operation }` (usulan spec 0005) lalu `Compensated`; selain itu `NotOwned` dengan reason `initialized` atau `created_by_other_operation`. Karena catatannya lebih baru dari catatan reference secret, ia dikompensasi lebih dulu, sehingga pemakaian secret sudah hilang sebelum reference dihapus (`ON DELETE RESTRICT`).
- Cancel sebelum `init` mengompensasi lalu `cancelled`; cancel saat `init` dilanjutkan sampai selesai (spec 0007).
- `WorkFactory` create membangun ulang operasi antri yang belum mulai setelah daemon restart dari `input`; input dengan sumber `value` atau `settingsOmitted` menghasilkan `NotRebuildable`, sehingga operasi itu ditutup `failed interrupted`.
- `InterruptPolicy::CompensateThenClose`: sebelum titik tak bisa dibalik, kegagalan, cancel, dan terputus dikompensasi seperti `Compensate`; sesudahnya, kegagalan ditutup `failed` dengan kode `WorkError` dan terputus ditutup `failed` alasan `interrupted` tanpa kompensasi, tidak pernah `needs_recovery`. `OperationKind::CreateInstance` memakai kebijakan ini.

### Pin dan sumber catalog

- `catalog::pin` dan `catalog::source` di `stackly-core` membangun K-1 dan K-2 spec 0017 persis dengan signature di sana (`pin_revision`, `PinnedRevision`, `PinError`, `ensure_source`).
- `stacklyd` memanggil `ensure_source` di slot start setelah registry `Ready` dan catalog dimuat, menyimpan `Option<ProviderSourceId>` dan error terakhir di state daemon, dan menambah bagian `degraded` subsystem `catalog` seperti K-2. Langkah `pin` membaca state itu dan memanggil `ensure_source` sekali lagi kalau kosong.
- Kode `catalog.pin_failed` ditambah ke `codes::catalog` dengan kategori, `retryable`, details, pesan, dan hint dari K-1.

### Kode error

Area `instance` milik spec 0010 dan area `catalog` milik spec 0017; fitur ini menambah kode berikut lewat aturan "fitur yang pertama kali memakainya". Semua `retryable` false. Pesan bahasa Inggris, huruf kecil di awal, tanpa titik, tanpa nilai settings teks atau secret.

| Kode | Kategori | Kapan | Kunci `details` |
| --- | --- | --- | --- |
| `instance.name_invalid` | `invalid_input` | nama tidak cocok VAL-001; `fields[name]` | |
| `instance.provider_unavailable` | `unsupported` | entri catalog planned atau invalid | `provider`, `state` |
| `instance.capability_unsupported` | `unsupported` | `multi_instance` tidak ada dan instance provider itu sudah ada (BR-016) | `capability`, `provider`, `existing` |
| `instance.runtime_incompatible` | `conflict` | executable yang dibutuhkan paket tidak ada di runtime | `runtimeId`, `executable` |
| `instance.storage_unsupported` | `unsupported` | root data bukan volume lokal `apfs` atau `hfs` | `fsType`, `root` |
| `instance.insufficient_space` | `unavailable` | ruang bebas di bawah batas | `needed`, `available` |
| `instance.data_not_empty` | `conflict` | folder instance sudah ada atau folder data tidak kosong (BR-008) | `path` |
| `instance.data_overlap` | `conflict` | folder data bertumpuk dengan instance lain (BR-002, `rule`) | `path`, `otherInstance` |
| `instance.short_key_taken` | `conflict` | balapan short key (spec 0007) | `shortKey` |
| `catalog.pin_failed` | `failed` | K-1 | `provider`, `revision`, `reason` |

Kode yang dipakai ulang: `instance.name_taken`, `instance.provider_not_found`, `instance.provider_ambiguous`, `instance.runtime_not_found`, `instance.runtime_mismatch`, `instance.settings_invalid`, `instance.endpoint_invalid` (spec 0010), `supervisor.endpoint_conflict`, `supervisor.endpoint_in_use`, `supervisor.runtime_unavailable`, `supervisor.data_unavailable`, `supervisor.data_in_use`, `supervisor.init_failed` (spec 0010), `secret.*` (spec 0011), `catalog.unavailable` (spec 0017), dan `registry.generation_conflict` (spec 0005). Path di `details` selalu relatif terhadap root data, tidak pernah path home penuh.

### CLI

```text
stackly instance create <name> --runtime <runtime-id | software@versi>
    [--provider <namespace/nama>] [--revision <label>] [--display-name <teks>]
    [--port <endpoint>=<port|auto>]... [--set <field>=<nilai>]... [--set-json <field>=<json>]...
    [--settings-file <path.json>] [--secret <field>=<sumber>]... [--start] [--dry-run]
    [--wait] [--timeout <durasi>] [--idempotency-key <key>] [--json]
```

- `--runtime software@versi` dicari CLI lewat `runtime.list`: tepat satu runtime `installed` dengan software dan versi itu; nol atau lebih dari satu gagal `cli.runtime_not_found` atau `cli.runtime_ambiguous` (exit 10 dan 3, kode area `cli` baru). UUID diteruskan apa adanya.
- CLI selalu memanggil `instance.get_create_form` lebih dulu untuk provider dan jenis field. `--provider` dan `--revision` mengisi `provider`; tanpa `--revision`, revision dikosongkan supaya daemon memakai revision catalog.
- `--set` dikonversi menurut `FieldKind`: `integer` lewat parse `i64`, `boolean` dari `true` atau `false`, `text` dan `choice` apa adanya; nilai yang gagal dikonversi dikirim sebagai teks. `--set-json` untuk field `list`. `--settings-file` dibaca lebih dulu, lalu flag menimpa key yang sama. Key yang tidak ada di form tetap dikirim, supaya daemon menolaknya dengan `unknown_setting`.
- `--secret <field>=<sumber>` dengan sumber `generate` (default untuk field yang tidak disebut), `prompt` (TTY dengan echo mati, dua kali, aturan spec 0011 AC-17), `stdin` (paling banyak satu field), `file:<path>`, `env:<NAMA>`, `copy:<ref>`, atau `bind:<ref>`. Tanpa TTY, `prompt` langsung gagal exit 2. Nilai dibaca lewat modul `secret_input` spec 0011.
- Tanpa `--port`, endpoint IP berarti `auto` (spec 0010). Perintah setara selalu menulis `--port` untuk setiap endpoint IP.
- `--dry-run` hanya memanggil `instance.preflight`. `--json` memberi `result` `{ "preflight": CreatePreflight }`; teks manusia satu baris per pemeriksaan (`ok`, `warn`, atau `fail`, judul, keterangan) lalu field error.
- Tanpa `--dry-run`: `--json` memberi `result` `{ operation, idempotencyKey, instance, start }` (pola perintah async spec 0008), dengan `instance` berisi `InstanceSummary` setelah `--wait` sukses dan `start` berisi `{ operation, idempotencyKey }` kalau `--start`. `--start` menunggu operasi create selesai walau tanpa `--wait`; dengan `--wait`, operasi start juga ditunggu. Progress tahap di stderr: Checking, Pinning provider package, Storing secrets in Keychain, Creating folders, Registering, Initializing.
- Exit code mengikuti kategori spec 0008. Create yang ditolak sinkron menulis `error` dengan `fields`, dan teks manusia menulis setiap field error di baris `<path>: <pesan>`.

**Perintah setara**: `stackly_protocol::instance::command::equivalent_command(&CommandDraft) -> String`, fungsi murni yang dipanggil daemon untuk `CreatePreflight.command` dan diuji CLI dengan round trip. Urutan: `stackly instance create <name> --runtime <label>`, `--display-name` kalau ada, `--port` untuk setiap endpoint IP dalam urutan form, `--set` atau `--set-json` hanya untuk field yang nilainya berbeda dari default (urutan form), `--secret <field>=<generate|prompt|copy:ref|bind:ref>` untuk setiap secret field (sumber `value` ditulis `prompt`), lalu `--start` kalau diminta. `<label>` adalah `software@versi` kalau tepat satu runtime `installed` cocok, selain itu UUID runtime. Kutip tunggal POSIX dipakai untuk nilai yang memuat karakter di luar `[A-Za-z0-9._/:@%+=,-]`. Perintah ini tidak pernah memuat nilai secret.

### Sheet New Instance (GUI)

Folder baru `StacklyUI/CreateInstance/`: `CreateInstanceModel`, `SchemaFormModel`, `NewInstanceSheet`, `EndpointPortRow`, baris field, `PreflightSection`, `EquivalentCommandRow`, dan `CreateProgressSection`. `StacklyClient` mendapat `getCreateForm`, `preflight`, dan params create yang diperluas dari tipe hasil generate.

- **Model**: `CreateInstanceModel` (`@Observable`, main actor) dengan state `choosingRuntime`, `loadingForm`, `formFailed(FailureContent)`, `editing`, `submitting`, `creating(operationId)`, dan `finished`. Masukannya `RuntimeStore` (spec 0018), `CatalogStore` (spec 0017), `MutationPath` dan `OperationCenter` (spec 0013), dan protokol `CreateInstanceService` (`getCreateForm`, `preflight`) supaya test memakai tiruan. Setiap perubahan draft menaikkan penghitung generasi, membatalkan preflight yang berjalan, dan memanggil preflight setelah `PREFLIGHT_DEBOUNCE`; balasan untuk generasi lama dibuang.
- **Header**: tile dan mark provider dari `CatalogStore`, judul "New Instance", subjudul "Its own data directory, port and secret. Nothing is shared with other instances."
- **Filter kategori**: picker bersegmen All, Databases, Messaging, Analytics, Cache, Proxy (kategori spec 0017; `testing` hanya terlihat di All). Mengganti kategori memilih runtime pertama di kategori itu, atau mengosongkan picker dengan teks "No installed runtimes in this category."
- **Runtime**: `Picker` berisi runtime `installed` dengan label "{nama tampil} {versi}" ditambah " (external)" untuk install external, urut nama tampil lalu `installedAt` terbaru. Tanpa runtime terpasang sama sekali, isi sheet menjadi `StatePlaceholder` "No Runtimes" dengan pesan "Install a runtime from the catalog to create instances." dan tombol Browse Catalog (menutup sheet dan membuka Catalog).
- **Nama**: `TextField` berlabel "Name", terisi `suggestedName` dan terpilih saat sheet dibuka; error dari `fields[name]` tampil di bawahnya.
- **Port**: satu baris per endpoint IP, label "Port · {nama endpoint}", picker bersegmen Fixed dan Automatic, lalu `TextField` angka untuk Fixed atau teks "Assigned when the instance starts" untuk Automatic. Error baris endpoint tampil di bawahnya dengan kalimat "Fixed ports are never changed silently." dan tombol "Use {n}" kalau ada `suggestedPort`; caution tampil dengan simbol segitiga dan keterangannya. Endpoint Unix tidak tampil sebagai baris.
- **Settings**: `SchemaForm` (*Form dari settings schema*).
- **Toggle**: "Start after creating" dengan subjudul "Initialise the data directory, start, and wait for readiness.", default aktif. Toggle "Start at login" tidak tampil sampai fitur 31.
- **Preflight**: judul "Preflight" lalu satu baris per `PreflightCheck` (simbol, `title`, `detail`). Selama preflight pertama belum kembali, baris menampilkan `DelayedProgress`; preflight yang gagal (koneksi atau error method) menampilkan "Couldn’t run preflight." dengan Retry, dan Create tetap nonaktif.
- **Perintah setara**: teks monospace "$ {command}" yang bisa dipilih, dan tombol Copy yang menulis pasteboard biasa lalu mengumumkan "Command copied."
- **Footer**: Cancel (Escape) dan Create (tombol default, Return), Create aktif hanya saat `MutationAvailability` `.available`, preflight terakhir `ready` untuk generasi draft saat ini, dan semua `SecretField` berisi nilai yang lolos batas panjang dan `printableOnly` di sisi client (pemeriksaan client hanya untuk kenyamanan; daemon tetap memvalidasi).
- **Create**: `MutationPath.perform(.instanceCreate(params))` dengan key baru. `rejected` dengan error sinkron menampilkan `StateBanner` critical di atas form berisi pesan daemon dan menjalankan preflight lagi; `notSent` dan `outcomeUnknown` memakai copy `MutationFeedback` spec 0013, dan percobaan ulang setelah `outcomeUnknown` memakai key yang sama. Operasi yang diterima mengubah sheet ke mode progress: form read only, `CreateProgressSection` menampilkan tahap operasi dari `OperationCenter.updates(for:)` (Checking, Pinning provider package, Storing secrets in Keychain, Creating folders, Registering, Initializing), dan baris preflight secret berubah menjadi "Stored in Keychain" setelah tahap `secrets` selesai. Cancel di mode ini memanggil `operation.cancel` setelah konfirmasi spec 0012 ("Stop creating this instance?"), dan nonaktif setelah tahap `init` mulai.
- **Selesai**: `succeeded` menutup sheet, mengirim `instance.start` lewat `MutationPath` kalau toggle aktif, lalu memanggil `ShellNavigator.show(.instances(filter: .all, select: id))` spec 0022 dengan instance ID dari `result.resources` (sebelum fitur 22 dibangun: pindah ke bagian Instances dengan daftar sementara spec 0013). `failed` atau `cancelled` mengembalikan mode edit dengan banner berisi error operasi, baris preflight yang terkait tahap gagal ditandai error, dan preflight dijalankan lagi.
- **Titik masuk**: `AppShell` memasang nilai environment `newInstanceAction: NewInstanceAction?` (spec 0022); selama nilainya ada, fitur 22 merender tombol toolbar "New Instance" di Overview dan Instances serta "New Instance…" sebagai aksi kedua empty state Instances (placeholder dan copy nya milik fitur 22); sebelum fitur 22 dibangun, fitur ini memasang tombol toolbar dan aksi yang sama di daftar sementara spec 0013. Titik masuk lain: tombol "New Instance…" di panel detail Catalog untuk entri provider (nonaktif dengan alasan "Install a version first." kalau `RuntimeStore` tidak punya runtime `installed` untuk software itu, dan membuka sheet dengan kategori dan runtime terpilih: runtime `installed` software itu dengan `installedAt` terbaru), dan perintah menu File > New Instance… (⌘N). Semua titik masuk nonaktif saat `MutationAvailability` `.blocked`, lewat `.mutationGated()` spec 0012.

### Form dari settings schema

`SchemaFormModel` mengubah `[FormField]` dan nilai draft menjadi baris tanpa cabang per provider:

| `FormField` | Kontrol | Nilai yang dikirim |
| --- | --- | --- |
| `secret` | `SecretField` spec 0011 (Generate default, Enter, Copy from) | `SecretInput` di `instance.create`; `SecretSource` di preflight |
| `managed` | baris read only dengan simbol gembok dan "Set by Stackly" | tidak dikirim |
| `text` | `TextField`, dengan batas panjang di hint | string |
| `integer` | `TextField` angka | integer kalau bisa di parse `Int64`, selain itu string (daemon yang menolak) |
| `boolean` | `Toggle` | boolean |
| `choice` | `Picker` gaya menu dengan `choices` | string |
| `list` | tabel item dengan Add dan Remove, satu kolom per `FormItemField`, batas `minItems` dan `maxItems` | array object |
| `unrecognized` | baris read only "{title}: This setting needs a newer version of Stackly." | tidak dikirim (daemon memakai default atau menolak) |

- Field tampil dalam urutan `fields`. Field `visibility: advanced` dikumpulkan di `DisclosureGroup("Advanced")` yang tertutup secara default dan terbuka sendiri kalau ada error di dalamnya.
- Field `apply: immutableAfterInit` mendapat `ToneBadge` netral "Set once" dan teks bantuan "Can’t be changed after the instance is created."; kelas `reload` dan `restart` tidak ditampilkan di create (milik fitur 29).
- `title` menjadi label, `description` menjadi teks bantuan, dan `required` tanpa default ditandai "Required" dalam teks, bukan hanya simbol.
- Field error dari `fields` dicocokkan lewat path dan tampil di bawah kontrolnya; error yang tidak cocok dengan field mana pun tampil di baris preflight `settings`.

### Aksesibilitas

- Urutan fokus: filter kategori, runtime, nama, baris port (picker mode lalu angka lalu tombol Use), field settings dalam urutan form, Advanced, toggle, baris preflight (masing masing satu elemen), tombol Copy, Cancel, Create.
- Label VoiceOver: field "{title}, {jenis kontrol}, required" plus "set once at creation" untuk `immutableAfterInit`; error dibacakan sebagai nilai tambahan; baris preflight "{title}, {Passed | Warning | Failed}: {detail}" (status `unrecognized` dibaca "Unknown status"); tombol saran "Use port {n}". Perubahan status preflight dan hasil create diumumkan lewat `announce(_:)` spec 0012.
- Simbol status: lingkaran centang (`ok`), segitiga (`caution`), lingkaran silang (`error`), dan tanda tanya (`unrecognized`), dengan warna tone spec 0012 sebagai tambahan, bukan satu satunya pembeda; Differentiate Without Color tidak mengubah apa pun karena bentuk sudah berbeda.
- Sheet memakai `Form` `.grouped` di dalam `ScrollView`, lebar minimum 560 pt, tinggi mengikuti isi sampai batas layar, sehingga teks besar menggulir tanpa terpotong.

### Copy preflight

Ditulis daemon (`title`, `detail`), sama di GUI dan CLI. Ukuran memakai satuan desimal seperti spec 0017 (misalnya "41 GB").

| Baris | `title` | `detail` |
| --- | --- | --- |
| `provider` (gagal) | "{displayName} revision {revision}" atau "Provider {nama}" | pesan error |
| `runtime` | "Runtime {displayName} {versi}" | `ok` "Verified {12 hex pertama digest}"; external "External runtime · not reproducible"; error pesan error |
| `name` | "Name available" | `ok` nama; error "Use 1 to 63 lowercase letters, digits, or hyphens, starting with a letter."; nama tampil salah "Use up to 128 characters without control characters." atau "An instance named {nama} already exists." |
| `endpoint` fixed | "Endpoint {host}:{port}" | `ok` "Free on IPv4 and IPv6 loopback"; error "In use by {instance}" atau "In use by another process"; caution "Also in use on {alamat}; clients that connect to localhost may reach that server" atau "In use on another address of this Mac" atau "Couldn’t check this port" |
| `endpoint` auto | "Endpoint {host} · {nama endpoint}" | "Port assigned when the instance starts" |
| `settings` (gagal) | "Settings" | "{n} settings need attention" |
| `data` | "Empty data directory" | `ok` "instances/<new instance id>/data · created new for this instance"; error pesan error |
| `disk` | "Disk space" | "{bebas} free · needs at least 256 MB" |
| `secret` | "{title field} stored in Keychain" | `generate` "Generated on create, never logged"; `value` "Entered value, never logged"; `copyFrom` "Copied from {ref}"; `bind` "Uses {ref}"; caution locked "Keychain is locked; macOS asks you to unlock it when you create"; caution `bind` tanpa nilai "No value stored yet; start will ask for it" |

### Konstanta

| Konstanta | Nilai | Tempat |
| --- | --- | --- |
| `CREATE_MIN_FREE_BYTES` | 256 MiB (268 435 456 byte), ditampilkan "256 MB" sesuai pembulatan desimal | `instance::create` |
| `PORT_SUGGESTION_ATTEMPTS` | 64 | `instance::create` |
| `PORT_SUGGESTION_MIN` | 1024 | `instance::create` |
| `REGISTER_RETRY_LIMIT` | 5 (aturan spec 0005) | `instance::create` |
| `NAME_SUGGESTION_MAX_SUFFIX` | 99 | `instance::create` |
| `PREFLIGHT_DEBOUNCE` | 300 ms | `CreateInstanceModel` |
| `PREFLIGHT_PLACEHOLDER_ID` | UUID v7 tetap `00000000-0000-7000-8000-000000000000` untuk dry run plan di preflight | `instance::create` |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| form | provider dan revision | entri catalog yang software paketnya sama dengan software runtime (`Catalog::entries`), `ValidatedPackage::key()` |
| form | `source` | `CatalogMeta.location` (`Bundled` atau `Development`) |
| form | runtime | `Registry::runtime(runtimeId)` dipetakan ke `RuntimeSummary` seperti spec 0018 |
| form | `suggestedName` | software paket, line runtime, nama instance tidak retained (*Nama*) |
| form | endpoint | `resources.endpoints` paket (transport, purpose, `defaultHost`, `defaultPort`, `socketName`) |
| form | field, kelas, batas, default | `SettingsSchema::fields()` dalam urutan deklarasi (usulan spec 0009) |
| form | `printableOnly` | `x-stackly.printableOnly` field (spec 0019) |
| form | `capabilities` | `CapabilitySet` paket dalam urutan capability spec 0017 |
| form | `dataPath` | teks tetap `instances/<new instance id>/data` |
| preflight, create | provider terpilih | *Resolusi provider dan revision* |
| preflight, create | runtime cocok | record runtime, `symlink_metadata` executable di `runtime_dir(..)` |
| preflight, create | nama dipakai | `Registry::instance_by_name` |
| preflight, create | konflik port Stackly | `Registry::instances(include_retained: false)` dan `endpoints_conflict` |
| preflight, create | port dipakai OS | `PortProbe` (bind percobaan) di alamat langkah 4 sampai 6 |
| preflight | `suggestedPort` | kandidat p+1 dan seterusnya yang lolos *Pemeriksaan port* |
| preflight, create | pemilik port Stackly di pesan | nama instance yang konflik dari registry; proses luar tidak pernah dinamai |
| preflight, create | filesystem | `Filesystem::volume_facts(root data)` (usulan spec 0004) |
| preflight, create | ruang bebas | `Filesystem::available_space(instances/)` (spec 0018) |
| preflight, create | keadaan store | `SecretStore::state()` dan `backend()` |
| preflight, create | reference sumber | registry `secret_references` dan `SecretStore::probe` (interaksi `Never`) |
| preflight | `command` | `equivalent_command` dari draft dan label runtime |
| create | `InstanceId` | `InstanceId::generate()` oleh handler, diulang sampai short key bebas |
| create | reference secret | `local:instance/<id>/<purpose>`, purpose dari nama field (spec 0011) |
| create | nilai secret | `SecretInput` di params (`value`), `generate_password` (`generate`), store (`copyFrom`) |
| create | interaksi Keychain | `OperationTrigger::interaction()` dari handshake (spec 0007, 0011) |
| create | folder instance | `DirectoryLayout::instance_paths(id)` dan `instance_log_dir(id)` |
| create | ID yang dikenal untuk layout dan overlap | `Registry::known_instance_ids` |
| create | paket pinned | `pin_revision` (K-1) |
| create | `source_id` | `ensure_source` (K-2) |
| create | settings tersimpan | `SettingsSchema::validate` atas settings draft yang secret field nya diisi reference |
| create | host endpoint | `defaultHost` manifest |
| create | permintaan port | draft (`Fixed` atau `Auto`; kosong berarti `Auto`) |
| create | nama socket Unix | render `socketName` dengan port `Fixed`, atau penanda `auto` |
| create | `startup` | konstanta `manual` |
| create | `data_line`, `data_version` | record runtime |
| create | `created_at` | `Clock::now()` di langkah `register` |
| create | `data.initializedAt` | `Clock` saat `init_succeeded` (spec 0010) |
| create | `expected_generation` | `Versioned.generation` dari baca registry di `register` |
| sheet | daftar runtime dan kategori | `RuntimeStore` (state `installed`) dan `CatalogStore` (kategori per software) |
| sheet | tahap progress | `OperationSummary.stage` dari `OperationCenter.updates(for:)` |
| sheet | instance yang dipilih setelah sukses | `OperationSummary.result.resources` (instance ID) |

### Key invariants

- Create tidak pernah menginisialisasi folder yang tidak dibuat operasi itu, dan tidak pernah menulis ke folder data yang tidak kosong (BR-008).
- Port `Fixed` tidak pernah diganti oleh daemon; hanya permintaan `Auto` yang menghasilkan port pilihan Stackly (BR-005).
- Preflight dan create memakai `evaluate` yang sama; tidak ada aturan validasi yang hanya ada di GUI atau hanya di CLI.
- Sebelum `init`, setiap efek create bisa dibalik dan dibalik dengan tepat (reference secret, folder instance, baris registry); folder paket pinned dan baris provider revision tidak pernah dihapus create.
- Nilai secret hanya ada di params `instance.create`, di memori daemon selama `put`, dan di Keychain.
- Instance yang terdaftar selalu menyimpan provider revision dan runtime exact, bukan selector (BR-001).
- Semua path yang ditulis create diturunkan dari ID dan layout dan diperiksa `check_owned_path` sebelum IO.

### Security model

- Semua method lewat socket privat milik user yang sama (spec 0006); tidak ada method baru yang bisa dipanggil tanpa handshake.
- Nama instance dibatasi VAL-001 dan tidak pernah dipakai sebagai path atau argumen shell; path dari ID UUID v7 buatan daemon.
- Settings divalidasi schema, termasuk aturan `argument_injection_risk` spec 0009; argv init diperiksa `ensure_not_in_argv` (spec 0011) sebelum spawn.
- Endpoint hanya loopback (`defaultHost`); create tidak pernah mengubah firewall atau bind ke alamat lain (VAL-005).
- Secret: tidak ada nilai di preflight, journal, sidik jari request (`SecretText` disamarkan), log, event, perintah setara, atau pesan error; reference bukan secret dan boleh tampil.
- Pin dan layout menolak symlink dan entri asing tanpa menimpa atau menghapus (K-1, spec 0004).
- Ukuran params dibatasi `Validate` dan `CONTROL_FRAME_MAX`; settings paling banyak 256 KiB (spec 0009); preflight hanya membaca, jadi pemanggilan berulang tidak mengubah state.
- Log daemon untuk create memuat ID operasi, ID instance, nama, provider, runtime, dan tahap, tidak pernah nilai settings atau secret; preflight hanya dicatat di level debug dengan jumlah baris per status.

### Configuration required

Tidak ada variabel environment, setting, atau credential baru. Catalog memakai `STACKLY_CATALOG_DIR` spec 0017 untuk development dan test.

### Fuzz target

`fuzz/fuzz_targets/instance_path.rs` (aturan nama spec 0006) memanggil `stackly_core::instance::fuzzing::owned_path(platform, prepared, fixtures, input)`. Target menyiapkan sekali, di folder sementara, `MacosPlatform::with_roots` dengan pohon fixture: symlink dari dalam root ke luar root, symlink ke root lain, loop symlink, symlink folder, nama Unicode dan berspasi, dan folder sedalam 64 tingkat. Input dipakai sebagai byte path (absolut atau relatif terhadap root data). Invariant:

- Tidak pernah panic.
- Path dengan komponen `.` atau `..`, atau path relatif, selalu `NotNormalized`.
- `Ok(owned)` berarti `owned.canonical` berada di dalam salah satu root canonical dan tidak sama dengan root itu.
- Path yang melewati symlink yang keluar dari root tidak pernah `Ok` dengan canonical di luar root.
- `check_data_root_overlap(candidate, fixtures data)` mengembalikan `DataRootOverlap` jika dan hanya jika identitas file candidate atau ancestor nya sama dengan salah satu folder data fixture atau sebaliknya.

Seed ada di `fuzz/seeds/instance_path/` dan dijalankan test `instance_path_seeds_hold_the_invariants` di `stackly-core` dengan `FakePlatform` (filesystem nyata di `TestRoot`).

### Critical test scenarios

- Happy path fake: `Catalog::for_tests` dengan `testing/fake`, `install_fake` runtime saja, `instance.get_create_form`, `instance.preflight` `ready`, `instance.create` dengan secret `authToken` bangkitan, operasi `succeeded`, instance `stopped`, manifest `initializedAt` sama dengan registry, folder pinned ada, baris `provider_sources` satu. Memverifikasi **AC-1**, **AC-2**, **AC-10**, **AC-12**, **AC-13**, **AC-14**.
- Tabel evaluasi (unit test murni `evaluate`): setiap baris tabel *Evaluator create* dan setiap langkah *Pemeriksaan port*, termasuk kombinasi nama salah dan port dipakai sekaligus, dengan kode baris error pertama dan `fields` yang diharapkan. Memverifikasi **AC-2**, **AC-3**, **AC-4**, **AC-6**, **AC-7**, **AC-9**, **AC-15**.
- Listener nyata (integration test `stackly-core` dengan `PortProbe` asli): `127.0.0.1:p` (error), `0.0.0.0:p` (error di langkah 4 atau caution di langkah 5 menurut perilaku macOS yang dicatat), `[::]:p` dual stack, `[::1]:p` (caution), UDP di port sama dengan endpoint TCP (ok). Memverifikasi **AC-4**, **AC-5**.
- Saran port: port `Fixed` milik instance `stopped` menghasilkan `supervisor.endpoint_conflict` dengan saran, saran tidak pernah dipakai create, dan setelah port diganti ke saran preflight `ready`. Memverifikasi **AC-5**.
- Data tidak kosong: ID dipaksa ke folder yatim, dan file disisipkan setelah layout. Memverifikasi **AC-8**.
- Secret: Keychain terkunci (`FakeSecretStore::lock`) menghasilkan `secret.needs_secret` sebelum folder dibuat; `printableOnly` dengan nilai ber `\n` menghasilkan `secret.invalid_value`; `bind` ke reference yang ada menulis pemakaian tanpa `put`; prefix fixture credential tidak ada di journal, log, event, dan preflight. Memverifikasi **AC-7**.
- Balapan: sepuluh create bersamaan dengan nama sama, dua create dengan port fixed sama, dan `GenerationConflict` yang disuntik empat kali lalu berhasil. Memverifikasi **AC-10**.
- Gagal dan terputus: kegagalan disuntik di setiap langkah sebelum `init` (registry, folder, reference bersih; folder pinned tetap), `failInit` fake (operasi `failed`, instance `failed`), daemon dimatikan saat `secrets`, `layout`, `register`, dan `init`. Memverifikasi **AC-11**.
- Pin: setiap kriteria K-1 dan K-2 sebagai test terpisah di `stackly-core` dan `stacklyd`. Memverifikasi **AC-13**, **AC-14**.
- Auto port dan race: create `Auto` lalu start sampai `ready` (`bound` dan `socketPath`), dan create `Fixed` lalu port direbut listener test sebelum start. Memverifikasi **AC-12**, **AC-16**.
- Kontrak: fixture setiap DTO, nilai asing setiap enum terbuka, dan fixture `create-form-*.json` yang dibangkitkan ulang sama dengan di repo. Memverifikasi **AC-17**.
- CLI: golden teks dan JSON untuk `--dry-run`, create, `--start`, `--wait`; konversi `--set`; setiap sumber `--secret`; `prompt` tanpa TTY exit 2; round trip `commands.json`. Memverifikasi **AC-18**.
- Paritas AC-028: matriks input buruk (nama `Bad_Name`, nama 64 karakter, port 0, port dipakai, port milik instance lain, setting integer `"abc"`, key asing, secret field di settings, secret field tidak dikenal, runtime tidak ada, runtime software lain, provider planned) lewat `Client` dan lewat CLI `--json`. Memverifikasi **AC-19**.
- GUI: test Swift `CreateInstanceModel` dengan service tiruan (debounce, generasi, Create aktif, progress, gagal, sukses dengan dan tanpa start), `SchemaFormModel` dari tiga fixture, dan evidence manual keyboard dan VoiceOver. Memverifikasi **AC-20**, **AC-21**, **AC-22**.
- Fuzz: 10 menit di Mac ini dan test seed. Memverifikasi **AC-23**.
- Kinerja: pengukuran di Mac ini dengan 10 instance fake terdaftar. Memverifikasi **AC-24**.
- PostgreSQL nyata: create 18 dan 17 lewat CLI dan GUI, konflik nyata, start sampai `ready`. Memverifikasi **AC-25**.
- Keamanan: nama `../x`, `a b`, `a;rm`, dan Unicode ditolak VAL-001; symlink di `providers/official/postgresql/` ditolak pin; symlink `instances/<id>` ditolak layout; grep nilai secret dan settings teks di log daemon kosong. Memverifikasi **AC-26**.

### Jawaban untuk follow up spec lain

| Spec dan baris Follow-up | Jawaban di spec ini |
| --- | --- |
| 0017: ambil *Kontrak untuk fitur 20* (K-1, K-2, update 0004 dan 0005), panggil `pin_revision`, putuskan revision, tombol New Instance di panel detail | AC-13, AC-14, *Pin dan sumber catalog*; revision catalog saat create (AC-15); tombol di *Sheet New Instance*; usulan 0004 dan 0005 di *Usulan update spec lain* |
| 0010: perluas `instance.create` secara additive (preflight, form dari schema, autostart) | dua method baca baru, `provider.revision` opsional, precheck, langkah baru (*Operasi `create_instance`*); autostart: Start after creating di client, Start at login ditunda ke fitur 31 |
| 0011: `SecretInput` per secret field, `put` dengan `PutTarget::New` sebelum `initdb`, gagal simpan `secret.needs_secret`, kompensasi reference | AC-7, langkah `secrets`, AC-11 |
| 0011: `SecretField` di sheet dengan `generate` default; preflight membaca hasil `put` | *Form dari settings schema*; baris secret berubah setelah tahap `secrets` (*Sheet New Instance*) |
| 0005: `known_instance_ids` sebelum layout, `InsertInstance` di batch operasi, `endpoints_conflict`, `expected_generation`, ulang lima kali | langkah `layout` dan `register`, AC-10 |
| 0004: tulis ulang manifest dengan `data.initializedAt` setelah `init_succeeded` | dilakukan `execute_init` spec 0010 (AC-13 spec 0010); spec ini membuktikannya ujung ke ujung (AC-12) |
| 0004: guard evidence `VAL-001` di test ujung ke ujung | AC-3 |
| 0004: BR-002, BR-004, BR-005, BR-008, `create_instance_layout`, `resolve_port`, validasi endpoint `Instance::new` | AC-4 sampai AC-8, *Endpoint di aggregate*; `resolve_port` tetap dipanggil start (spec 0010) karena create tidak memilih port `Auto` |
| 0008: bukti AC-028 penuh | AC-18, AC-19 |
| 0009: `Endpoint` domain membawa `purpose` atau dibaca dari paket | dibaca dari paket pinned (immutable); domain dan registry tidak berubah; fitur 21 memakai cara yang sama untuk connection string |
| 0012: DTO capability lewat spec 0013 dan fitur 20 | `CapabilityStatus` spec 0017 dipakai di `CreateForm`; untuk aksi pada instance yang sudah ada, fitur 21 atau 22 menambah capability dari provider revision instance ke `InstanceSummary` saat membutuhkannya |
| 0019: preflight folder data kosong, `superuserPassword` dan `printableOnly`, `pin_package` | AC-6, AC-8; `printableOnly` di `FormField` dan pemeriksaan client; `pin_package` tetap fixture test, create memakai `pin_revision` |
| 0018: create menerima `RuntimeId`; satu satunya pemanggil `pin_revision` | AC-1, AC-13 |

### Usulan update spec lain

Ditulis di sini karena spec ini tidak boleh mengubah spec lain; lead menjadikannya item `architect (update)` sebelum milestone yang membutuhkannya.

- **Spec 0004** (additive): ID `ProviderSourceId` (UUID v7) dan `ProviderSourceRecord { id, kind, location, trust_root, last_sync_at, status, revision, created_at, updated_at }` di modul `registry` domain (dari K-2 spec 0017); `Filesystem::volume_facts(path) -> VolumeFacts { fs_type: String, local: bool }` dengan adapter macOS lewat `statfs` (`f_fstypename`, `MNT_LOCAL`), `FakeFilesystem` yang bisa diatur (default `apfs`, lokal), dan pemeriksaan di `fs_conformance`; validasi endpoint di `Instance::new` (nama ganda, `bound` terisi, socket ganda, konflik endpoint draft sendiri dengan rule BR-004) beserta varian `InvariantDetail`; setter `Instance::set_socket_name(endpoint, SocketName)` yang menaikkan revision; varian `InterruptPolicy::CompensateThenClose` dan `OperationKind::CreateInstance` memakainya; konstanta `ReasonCode::DATA_NOT_EMPTY`.
- **Spec 0005** (additive, tanpa migrasi): `WriteOp::InsertProviderSource(ProviderSourceRecord)` dengan `DuplicateResource` untuk ID atau (`kind`, `location`); baca `provider_source_by_location(kind, location) -> Versioned<Option<ProviderSourceRecord>>`; `ProviderRevisionRecord.source_id: Option<ProviderSourceId>` dengan `MissingReference`; `ResourceRef::ProviderSource`; `WriteOp::DeleteUninitializedInstance { id, expected, operation }` yang menghapus baris instance (endpoint, path, dan config revision ikut lewat cascade) dan semua `secret_reference_uses` instance itu dalam satu op, hanya kalau lifecycle `uninitialized` dan `created_by_operation` sama, selain itu error baru `RegistryError::NotDeletable { resource, reason }`; semuanya di `FakeRegistry`, `registry_conformance`, dan `SqliteRegistry`.
- **Spec 0007** (additive): `InterruptPolicy::CompensateThenClose` (perilaku di *Operasi `create_instance`*) di tabel resume untuk `create_instance`, baris "pekerjaan gagal setelah titik tak bisa dibalik" di tabel transisi engine mengecualikan kebijakan ini seperti `CloseInterrupted`, dan `not_created` menerima detail `data_not_empty` setelah pembuatan yang berhasil tetapi isinya tidak milik operasi.
- **Spec 0009** (additive): `SettingsSchema` menyimpan urutan deklarasi `properties` (root dan item) dan `fields()` mengembalikan field dalam urutan itu; decode memakai visitor map yang mempertahankan urutan dokumen tanpa fitur `preserve_order` serde_json.
- **Spec 0010** (additive): `instance.create` dengan `provider.revision` opsional, precheck penuh lewat `instance::create::evaluate`, kode sinkron baru, dan langkah operasi `validate`, `pin`, `secrets`, `layout`, `register`, `init` dengan `init` sebagai titik tak bisa dibalik (menggantikan daftar tahap create minimal); format nama yang tidak valid menjadi `instance.name_invalid` dari evaluator, bukan `request.invalid_params`; batch `spawned` merender ulang socket Unix yang merujuk endpoint `Auto` (*Socket Unix dengan token port*) dan `socketPath` null selama endpoint rujukannya tanpa port efektif.
- **Spec 0011** (klarifikasi): create tidak memanggil `put` untuk sumber `bind`; pemakaiannya ditulis `ReplaceSecretUses` di batch pendaftaran. Kegagalan store saat `put` di create dipetakan ke `secret.needs_secret`.
- **Spec 0008** (tanpa update): area `cli` mendapat `cli.runtime_not_found` dan `cli.runtime_ambiguous` lewat aturan pemakai pertama; `CLI_SCHEMA_VERSION` tetap karena hasil baru hanya menambah key.
- **Spec 0006** (tanpa update): kode baru masuk area `instance` dan `catalog` pemiliknya lewat aturan pemakai pertama.

### Selisih mockup yang disengaja

Untuk dicatat lead di `ui-macos.md`:

- Nama mengikuti VAL-001 (1 sampai 63 karakter), bukan pola mockup 3 sampai 31 karakter.
- Port diterima 1 sampai 65535 (VAL-004); saran hanya mulai 1024.
- Sheet dibuka dengan `defaultPort` manifest dan menolak konflik dengan saran, bukan mengisi port bebas berikutnya seperti mockup saat dibuka dari Catalog.
- Automatic menampilkan "Assigned when the instance starts", bukan angka port yang ditebak.
- Toggle Start at login tidak tampil sampai fitur 31.
- Sheet tetap terbuka dengan progress selama operasi create, bukan langsung tertutup.
- Baris preflight `provider` dan `settings` muncul saat gagal; baris secret per secret field dengan judul field.
- Tombol Copy menyalin perintah lengkap termasuk `--port` dan `--secret`, bukan versi pendek mockup.

## Build plan

Urutan mengikuti pendekatan Journey proyek. Jalur yang dibuat lengkap adalah "pengguna membuat instance dengan preflight lalu instance itu bisa di start", lebih dulu di core di atas fake dengan semua jalur gagalnya, lalu lewat daemon dan CLI, lalu di GUI, lalu dibuktikan dengan PostgreSQL nyata di Mac ini.

**Milestone 1: Evaluasi, pin, dan operasi create di core** (needs `/architect` update spec 0004, 0005, 0007, 0009, dan 0010 dari *Usulan update spec lain*; fitur 5 dan 7 milestone 3 sudah ada; fitur 9 build; fitur 10 milestone 2 (`PortProbe`, `execute_init`); fitur 11 langkah 4 (`SecretService::put`); fitur 17 milestone 1 (`core::catalog`); fitur 18 langkah 2 (`available_space`))

1. Domain dan platform: `ProviderSourceId` dan `ProviderSourceRecord`, validasi endpoint di `Instance::new`, `Instance::set_socket_name`, `InterruptPolicy::CompensateThenClose`, `ReasonCode::DATA_NOT_EMPTY`, dan `Filesystem::volume_facts` di `MacosFilesystem`, `FakeFilesystem`, dan `fs_conformance`. Memenuhi **AC-6**, **AC-11**, **AC-12**.
2. Registry: `InsertProviderSource`, `provider_source_by_location`, `ProviderRevisionRecord.source_id`, `ResourceRef::ProviderSource`, dan `DeleteUninitializedInstance` dengan `NotDeletable`, di `FakeRegistry`, `registry_conformance`, dan `SqliteRegistry`. Memenuhi **AC-11**, **AC-14**.
3. Engine: perilaku `CompensateThenClose` di runner, `recover`, dan tabel hint, plus detail `data_not_empty` di `not_created`, dengan test `ScriptedWork`. Memenuhi **AC-11**.
4. `SettingsSchema` dengan urutan deklarasi. Memenuhi **AC-1**, **AC-21**.
5. `core::catalog::pin` dan `core::catalog::source` (K-1, K-2) dengan semua test kriteria. Memenuhi **AC-13**, **AC-14**.
6. `core::instance::create`: `CreateDraft`, `gather`, `evaluate` dengan tabel test, *Pemeriksaan port* dan saran, nama saran, pemetaan `SettingsError` dan `PlanError` ke field issue, dan `instance::fuzzing::owned_path` dengan test seed. Memenuhi **AC-2** sampai **AC-7**, **AC-9**, **AC-15**, **AC-23** (harness).
7. `CreateInstanceWork`, factory, kompensator `instance_registration`, sumber ID yang bisa disuntik di fitur `test-support`, dan kait test setelah layout, dengan test core memakai `FakePlatform`, `FakeRegistry`, `FakeSecretStore`, dan `FakeProcessSupervision`. Memenuhi **AC-7**, **AC-8**, **AC-10**, **AC-11**, **AC-12**.

Sesi paralel: setelah langkah 1 masuk, langkah 2 (registry), 3 (engine), 4 (settings), dan 5 (pin, setelah langkah 2) bisa menjadi sesi terpisah karena menyentuh crate dan modul berbeda; langkah 2 tidak berjalan bersamaan dengan sesi lain yang menulis `WriteOp`. Langkah 6 dan 7 satu sesi setelah semuanya.

**Milestone 2: Kontrak, daemon, dan CLI** (after milestone 1; needs fitur 6, fitur 8 build (kerangka CLI), fitur 10 milestone 4 (method instance, `InstanceSummary`), fitur 11 langkah 8 (`secret_input`), dan fitur 14 (`stackly instance create` dasar) di `01-foundation.md`; fitur 17 langkah 6 (catalog di daemon) dan fitur 18 milestone 3 (`runtime.list`, `RuntimeSummary`))

8. Protokol: `SignedSafeInteger`, DTO, enum terbuka, dua method di tabel `methods!` (`recovery refused`), `provider.revision` opsional, kode baru, `instance::command::equivalent_command`, fixture termasuk `create-form-*.json` dan `commands.json`, kenaikan minor, `cargo xtask codegen`, dan test kontrak Swift. Memenuhi **AC-17**.
9. `stacklyd`: `ensure_source` di slot start dan bagian `degraded` subsystem `catalog`, handler `instance.get_create_form`, `instance.preflight`, dan precheck `instance.create`, pendaftaran kompensator, pemetaan DTO dan error, dan integration test daemon untuk setiap skenario yang butuh daemon (termasuk auto port, race dengan start, dan skenario keamanan). Memenuhi **AC-1**, **AC-2**, **AC-10** sampai **AC-14**, **AC-16**, **AC-24**, **AC-26** (test; review fresh model di kotak Review it).
10. CLI: `stackly instance create` lengkap dengan konversi `--set`, sumber `--secret`, `--dry-run`, `--start`, golden, test round trip perintah setara, dan matriks paritas AC-028. Memenuhi **AC-18**, **AC-19**.
11. Aturan `check-deps` tanpa identitas provider di kode create Rust dan Swift, dengan fixture yang melanggar. Memenuhi **AC-21**.

Sesi paralel: setelah langkah 8 masuk, langkah 9 dan 10 bisa menjadi dua sesi (daemon dan CLI menyentuh folder berbeda; matriks paritas di langkah 10 menunggu langkah 9 di merge), dan langkah 11 sesi kecil terpisah.

**Milestone 3: Sheet New Instance** (after langkah 8; needs fitur 12 dan fitur 13 milestone 4 (`ui-macos.md`), fitur 11 langkah 10 (`SecretField`), fitur 17 milestone 3 (layar Catalog dan `CatalogStore`), dan fitur 18 milestone 4 (`RuntimeStore`); memakai komponen fitur 67 dan peta perintah fitur 69 kalau sudah ada)

12. `StacklyClient`: method `getCreateForm`, `preflight`, params create yang diperluas, dan test fixture. Memenuhi **AC-17**.
13. `CreateInstanceModel`, `SchemaFormModel`, dan `CreateInstanceService` dengan test Swift dari fixture dan service tiruan. Memenuhi **AC-20**, **AC-21**.
14. `NewInstanceSheet` dan semua barisnya, mode progress, titik masuk (toolbar dan empty state Instances, panel detail Catalog, ⌘N), preview, dan evidence manual keyboard serta VoiceOver dengan daemon development. Memenuhi **AC-20**, **AC-21**, **AC-22**.

Sesi paralel: milestone 3 bisa berjalan bersamaan dengan langkah 9 sampai 11 karena hanya bergantung pada fixture langkah 8.

**Milestone 4: PostgreSQL nyata dan bukti di Mac ini** (after milestone 2 dan 3; needs fitur 19 milestone 2 (PostgreSQL lewat daemon) dan fitur 18 milestone 2 (`install_cached_artifact`); artefak PostgreSQL di cache runtime xtask, selain itu `BLOCKED`)

15. Test native lewat daemon: create PostgreSQL 18 dan 17 dengan password bangkitan, preflight dengan listener nyata di `127.0.0.1`, `0.0.0.0`, `[::]`, dan `[::1]` (perilaku macOS dicatat di tabel *Pemeriksaan port*), race port sebelum start, lalu start sampai `ready`. Memenuhi **AC-4**, **AC-16**, **AC-25**.
16. Fuzz `instance_path` 10 menit dengan evidence, pengukuran kinerja AC-24, dan evidence manual GUI untuk PostgreSQL (keyboard dan VoiceOver). Memenuhi **AC-22**, **AC-23**, **AC-24**, **AC-25**.

Sesi paralel: langkah 16 bagian fuzz bisa berjalan sebagai sesi sendiri begitu langkah 6 masuk.

## Consequences

**Positive**:

- Satu evaluator membuat GUI, `--dry-run`, dan create memberi jawaban yang sama, jadi AC-028 terbukti dari struktur kode, bukan dari kebetulan.
- Provider baru (NATS di fitur 23, lalu Valkey, Traefik, MySQL, ClickHouse) mendapat form, preflight, dan CLI tanpa kode khusus.
- Create yang gagal sebelum inisialisasi tidak meninggalkan apa pun selain folder paket pinned yang memang boleh dipakai ulang; tidak ada operasi `needs_recovery` untuk kegagalan init yang wajar.
- Konflik port terlihat sebelum Create, termasuk listener wildcard dan `localhost` yang bisa salah sasaran, dan port fixed tidak pernah berubah diam diam.
- Paket provider yang dipakai instance tetap ada di folder data walau catalog di app berubah, jadi start offline tetap bisa.

**Negative / tradeoffs**:

- Enam spec lain butuh update additive sebelum milestone 1, dan lead harus menjadwalkannya.
- Preflight membuka socket percobaan setiap kali draft berubah; jeda 300 ms dan pembatalan menjaga jumlahnya, tetapi tetap ada IO kecil per ketikan.
- Probe port hanya bisa menamai pemilik port kalau pemiliknya instance Stackly; proses luar ditulis "another process".
- Penanda `auto` di nama socket Unix menambah satu aturan di batch `spawned` spec 0010 dan satu setter domain.
- "Start after creating" bergantung pada client yang tetap hidup: app yang ditutup di antara create dan start meninggalkan instance `stopped` (terlihat jujur, tetapi butuh start manual).
- Batas ruang 256 MiB adalah angka tetap tanpa data per provider; provider yang butuh lebih besar baru tertangkap saat init.
- Instance `failed` karena init gagal tidak bisa dihapus dari GUI atau CLI sampai fitur 54 (purge) dibangun.

**Neutral**:

- Dua method baru, satu field opsional, satu newtype wire, sepuluh kode wire baru, dua kode `cli`, dan kenaikan minor protokol.
- Modul core baru `instance::create`, `catalog::pin`, dan `catalog::source`; folder Swift `CreateInstance/`; fuzz target `instance_path`.
- Kompensator kustom pertama di luar engine (`instance_registration`).
- Aturan `check-deps` baru yang ikut memindai kode Swift.

## Follow-up

- [ ] Lead: jadwalkan `/architect` update spec 0004, 0005, 0007, 0009, 0010, dan 0011 dari *Usulan update spec lain* sebelum milestone 1 (spec 0004 dan 0005 juga membawa usulan K-1 dan K-2 dari spec 0017).
- [ ] Lead: catat *Selisih mockup yang disengaja* di `ui-macos.md` lewat `/scope`.
- [ ] Fitur 14: pakai nama flag di *CLI* (`--runtime`, `--port`, `--set`, `--secret`) untuk `stackly instance create` dasar, supaya fitur ini hanya menambah flag.
- [ ] Fitur 21: baca purpose endpoint dan template socket dari paket pinned untuk connection string; tampilkan socket path hanya saat `socketPath` tidak null.
- [ ] Fitur 22: render tombol toolbar dan aksi empty state dari `newInstanceAction`, sediakan `ShellNavigator.show(.instances(filter: .all, select:))`, dan tambah capability provider revision ke `InstanceSummary` kalau layar instance membutuhkannya (disepakati dengan arc-f22-spec).
- [ ] Fitur 23: tidak ada perubahan create; AC-13 spec 0023 memakai ulang fixture `create-form-nats.json` dan test render Swift AC-21, dan NATS pindah ke `[[provider]]` di milestone 2 fitur 23 sebelum create GUI untuk NATS (disepakati dengan arc-f23-spec).
- [ ] Fitur 31: tambah `startup` ke params `instance.create` (minor), toggle "Start at login" di sheet, dan perintah setara `--start-at-login`.
- [ ] Fitur 32: path data yang bisa diganti untuk instance baru melewati baris `data` dan pemeriksaan `volume_facts` yang sama.
- [ ] Fitur 54: purge untuk instance `failed` hasil init gagal, memakai lock instance spec 0010.
- [ ] Fitur 67: adopsi `PreflightSection`, footer sheet, dan `CreateProgressSection` kalau fitur ini membangunnya lebih dulu.
- [ ] Fitur 69: daftarkan New Instance… (⌘N) di peta perintah dan hapus `CommandGroup` sementara.
- [ ] `/sync` setelah build: catat `instance.get_create_form`, `instance.preflight`, fuzz target `instance_path`, aturan `check-deps` baru, dan folder `CreateInstance/` di `AGENTS.md` root dan `apps/macos/AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, dan hasil cross check ada di [rationale.md](rationale.md).
