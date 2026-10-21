# 0020. Create instance

**Date**: 2026-10-20
**Status**: In Progress

## Summary

Spec ini menetapkan cara pengguna membuat instance baru dari GUI dan CLI: memilih runtime yang sudah terpasang, memberi nama, memilih port fixed atau automatic per endpoint, mengisi settings dan secret, lalu melihat preflight (pemeriksaan sebelum create) sebelum menekan Create. Form dibangun daemon dari settings schema dan resources manifest provider, jadi GUI tidak punya kode khusus PostgreSQL, NATS, atau fake provider. Preflight dan create memakai satu fungsi evaluasi yang sama di daemon, sehingga GUI, `stackly instance create --dry-run`, dan `stackly instance create --json` selalu memberi kode error dan field error yang sama. Create mem pin paket provider dari catalog (menyalinnya ke folder data), menyimpan secret di Keychain, membuat folder instance yang selalu baru, mendaftarkan instance, lalu menginisialisasi data; selama operasi belum selesai, setiap langkahnya bisa dibatalkan dengan bersih, termasuk inisialisasi yang gagal, dan data yang tidak dibuat operasi itu tidak pernah disentuh.

## Requirements

**User stories**:

- Sebagai pengguna yang baru memasang PostgreSQL 18, saya ingin membuat instance dengan nama, port, dan password dalam satu sheet yang menunjukkan apa yang akan terjadi sebelum saya menekan Create, supaya saya tidak menemukan konflik port atau folder setelah semuanya setengah jadi.
- Sebagai pengguna, saya ingin port fixed yang bentrok ditolak dengan port bebas yang ditawarkan, bukan diganti diam diam, supaya connection string yang saya tulis di aplikasi saya tetap benar.
- Sebagai pengguna yang create nya gagal, saya ingin tidak ada sisa setengah jadi yang memakan nama dan port, supaya saya bisa langsung mencoba lagi.
- Sebagai pengguna keyboard atau VoiceOver, saya ingin menyelesaikan seluruh alur create tanpa mouse dan tanpa informasi yang hanya ada lewat warna, supaya saya bisa memakai Stackly sama baiknya.
- Sebagai penulis script, saya ingin `stackly instance create` dengan `--json`, `--dry-run`, dan flag yang setara dengan sheet, plus perintah yang bisa saya salin dari GUI, supaya hasil dan error nya sama dengan GUI.
- Sebagai penulis provider (fitur 23, 40 sampai 43), saya ingin form create yang muncul dari settings schema dan resources manifest saja, supaya provider baru tidak butuh perubahan SwiftUI atau Rust.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-008):

- **AC-1** (form): `instance.get_create_form { runtimeId, provider? }` (`recovery refused`, hanya membaca) membalas `CreateForm` seperti *Method IPC*. Provider dipilih menurut *Resolusi provider dan revision*: tanpa `provider`, entri catalog berjenis provider yang software paketnya sama dengan software runtime; nol entri menghasilkan `instance.provider_not_found`, lebih dari satu `instance.provider_ambiguous`, entri planned atau invalid `instance.provider_unavailable`, catalog yang tidak dimuat `catalog.unavailable`, runtime yang tidak ada `instance.runtime_not_found`, runtime yang bukan `installed` `supervisor.runtime_unavailable`, dan `provider` yang software nya berbeda `instance.runtime_mismatch`. Isinya: provider (nama, revision catalog yang dimuat, `manifestSha256`, nama tampil, kategori, sumber), `RuntimeSummary` spec 0018, `suggestedName` menurut *Nama*, `dataPath`, setiap endpoint manifest (urut purpose `client`, `admin`, `monitoring`, lalu nama), setiap field settings schema dalam urutan deklarasi `properties` dengan keenam kelas VAL-012, dan `capabilities` (`CapabilityStatus` spec 0017). Dengan `FakePlatform` dan `FakeRegistry`, method ini tidak menambah `mutations()`, commit registry, tulisan Keychain, atau spawn.
- **AC-2** (preflight): `instance.preflight` (`recovery refused`, hanya membaca, tanpa idempotency key) menerima draft create tanpa nilai secret (params nya hanya membawa jenis sumber secret) dan membalas `CreatePreflight { ready, provider, checks, fields, command }`. Baris `runtime`, `name`, `data`, dan `disk` selalu ada; satu baris `endpoint` per endpoint IP manifest dan satu baris `secret` per secret field ada kalau paket bisa ditentukan; satu baris `endpoint` tambahan untuk setiap permintaan port yang menyebut endpoint tidak dikenal, endpoint `unix`, atau endpoint yang sama dua kali; baris `provider` dan `settings` hanya ada saat statusnya bukan `ok`. `ready` bernilai true jika dan hanya jika tidak ada baris `error`. Method ini tidak pernah menulis registry, disk, atau Keychain, tidak pernah spawn, dan tidak pernah memicu dialog Keychain (dibuktikan dengan penghitung fake). Preflight dan precheck `instance.create` memanggil fungsi yang sama, `instance::create::evaluate`, atas `CreateFacts` yang dikumpulkan `gather` yang sama: untuk setiap baris tabel test evaluasi, kode error baris `error` pertama preflight sama dengan kode error sinkron `instance.create`; kalau kode itu membawa `fields` (mendaftarkan `omitted` dan tanpa `items`, lihat *Evaluator create*), `fields` error create sama persis dengan `fields` preflight, dan untuk kode lain, termasuk saat `secret.needs_secret` di depan, error create tanpa `fields` (D7). Satu satunya pengecualian adalah bentuk nilai secret sumber `value` (panjang dan `printableOnly`), yang hanya bisa diperiksa precheck create karena preflight tidak pernah menerima nilai; nilai kosong, ber NUL, atau lebih dari 65536 byte sudah ditolak lebih dulu oleh `Validate` params `instance.create` (spec 0011 AC-7).
- **AC-3** (nama, VAL-001): nama yang tidak cocok dengan `InstanceName` spec 0004 (1 sampai 63 karakter, huruf kecil, angka, dan tanda hubung tunggal, diawali huruf) menghasilkan baris `name` error dan `instance.name_invalid` dengan field `name`; nama yang dipakai instance yang tidak retained menghasilkan `instance.name_taken` (detail `existing`); nama milik instance retained boleh dipakai lagi; `displayName` yang dikirim tetapi bukan `DisplayName` valid (1 sampai 128 karakter tanpa karakter kontrol) menghasilkan `instance.name_invalid` dengan field `displayName`. Test ujung ke ujung create memasang guard evidence `covers = ["VAL-001"]` (Follow-up spec 0004).
- **AC-4** (endpoint dan port, BR-004, BR-005, VAL-004 sampai VAL-006, AC-056): endpoint selalu memakai `defaultHost` manifest (loopback, VAL-005); permintaan port untuk endpoint yang tidak ada, untuk endpoint `unix`, ganda, atau di luar 1 sampai 65535 menghasilkan `instance.endpoint_invalid`; endpoint IP tanpa permintaan berarti `Auto` (spec 0010). Untuk port `Fixed` p di alamat A, evaluasi menerapkan *Pemeriksaan port* berurutan: konflik `endpoints_conflict` dengan port efektif atau port `Fixed` instance Stackly lain yang tidak retained, termasuk yang `stopped`, menghasilkan `supervisor.endpoint_conflict` (detail `endpoint`, `port`, `otherInstance`); konflik di antara endpoint draft sendiri menghasilkan `instance.endpoint_invalid`; bind percobaan di A:p dengan `SO_REUSEADDR` (seperti server) yang gagal karena dipakai menghasilkan `supervisor.endpoint_in_use` (detail `endpoint`, `address`, `port`), dan yang ditolak OS (`Denied`) atau alamatnya tidak tersedia (`Unavailable`) menghasilkan `supervisor.endpoint_unavailable` (detail `endpoint`, `address`, `port`, `reason`), sama dengan tahap `endpoints` start spec 0010 (K2); bind wildcard (`0.0.0.0:p` untuk A IPv4, `[::]:p` dengan `IPV6_V6ONLY` untuk A IPv6) atau bind loopback keluarga lain (`[::1]:p` atau `127.0.0.1:p`) tanpa `SO_REUSEADDR` yang gagal menghasilkan status `caution` dengan keterangan, bukan error. Dengan listener test di `127.0.0.1:p`, `0.0.0.0:p`, `[::]:p` dual stack, `[::1]:p`, sisa socket `TIME_WAIT` di `127.0.0.1:p`, dan UDP di port yang sama, hasil untuk endpoint TCP `127.0.0.1:p` di Mac ini sama dengan tabel *Hasil yang diharapkan*, dan tuple (alamat, transport, port) tidak pernah diperlakukan sebagai satu angka global. Endpoint `Auto` tidak di probe dan berketerangan bahwa port dipilih saat instance start.
- **AC-5** (saran port, BR-005): untuk port `Fixed` yang konflik atau dipakai, baris endpoint membawa `suggestedPort`, yaitu port pertama dari urutan `max(p+1, 1024)` sampai 65535 (paling banyak `PORT_SUGGESTION_ATTEMPTS` kandidat) yang lolos langkah 2 sampai 6 *Pemeriksaan port* tanpa error dan tanpa caution, termasuk terhadap endpoint lain di draft yang sama; tanpa kandidat, field itu kosong. Saran dihitung `gather`. Saran tidak pernah dipakai tanpa aksi pengguna: GUI hanya mengganti nilai lewat tombol "Use {n}" yang mengisi mode `Fixed` dengan angka itu, CLI hanya menulisnya di pesan, dan daemon tidak pernah mengganti port `Fixed` di create atau start.
- **AC-6** (data, storage, disk, VAL-007, VAL-008, BR-002): baris `data` gagal dengan `instance.storage_unsupported` (detail `fsType`, `root`) kalau root data bukan volume lokal ber filesystem `apfs` atau `hfs` menurut `Filesystem::volume_facts`, dan dengan `supervisor.data_unavailable` kalau `instances/` atau `retained/` bukan folder privat milik user. Baris `disk` gagal dengan `instance.insufficient_space` (detail `needed`, `available`) kalau `available_space(instances/)` di bawah `CREATE_MIN_FREE_BYTES`. Di operasi, folder instance selalu diturunkan dari ID baru lewat `DirectoryLayout` dan diperiksa `check_owned_path` sebelum setiap tulisan; setelah `create_instance_layout`, `check_data_root_overlap` terhadap folder data setiap ID yang dikenal (aktif dan retained) yang menemukan tumpukan menghasilkan `instance.data_overlap` (detail `path`, `otherInstance`, rule BR-002) sebelum init. Pemeriksaan ini sengaja berlapis: folder baru tidak mungkin alias, kecuali ada symlink yang ditanam sebelumnya.
- **AC-7** (secret): setiap secret field mendapat tepat satu sumber: `generate` (default kalau tidak dikirim), `value`, `copyFrom`, atau `bind`. Field yang tidak ada di schema, bukan secret, atau dikirim dua kali menghasilkan `instance.settings_invalid` dengan path `secrets[<i>].field`; secret field yang ikut di `settings` menghasilkan `instance.settings_invalid` dengan path `settings.<field>`. Backend store `unsupported` selalu menjadi `secret.store_unsupported`. Sumber `copyFrom` atau `bind` dengan reference yang tidak terdaftar menghasilkan `secret.not_found`, dan yang terdaftar tetapi tanpa nilai menghasilkan `secret.needs_secret` (reason `missing`), keduanya sebagai baris error. Nilai sumber `value` yang kosong, memuat NUL, atau lebih dari 65536 byte ditolak `Validate` sebagai `request.invalid_params` di `secrets[<i>].input.content` (spec 0011 AC-7). Precheck create menolak nilai sumber `value` yang di luar `minLength` dan `maxLength`, atau memuat karakter kontrol untuk field `printableOnly`, dengan `secret.invalid_value` (`reason` `length` atau `control_characters`) secara sinkron, lewat aturan `SecretFieldRule::check` yang sama dengan `put`. Di langkah `secrets`, setiap secret field memanggil `SecretService::put` dengan `PutTarget::New { reference: local:instance/<id>/<purpose>, owner, purpose, field: Some(FieldContext) }` (`FieldContext.rule` dari `SecretFieldRule::from_setting`), mode `create`, dan interaksi dari pemicu operasi; sumber selain `bind` didahului `record_secret_intent`, sedangkan `put` dengan `Bind` hanya membaca nilai reference itu sekali dan memeriksanya terhadap aturan field tanpa menulis apa pun (spec 0011 AC-23). Kegagalan `put` dipetakan `secret::error::classify_create_failure` (spec 0011): `locked`, `access_denied`, atau `busy` pada reference sumber `copyFrom` atau `bind` menjadi `secret.needs_secret` dengan satu item untuk reference itu, sumber yang hilang dari store menjadi item `missing`, kegagalan menulis reference target menjadi `secret.store_locked`, `secret.store_access_denied`, atau `secret.store_busy`, dan nilai yang melanggar aturan field menjadi `secret.invalid_value`; create berhenti di `put` pertama yang gagal tanpa dialog baru, dan semuanya terjadi sebelum folder dibuat; kompensasi menghapus tepat reference yang dibuat operasi itu. Nilai secret tidak pernah ada di journal, sidik jari request, log, event, preflight, atau perintah setara.
- **AC-8** (AC-008, BR-008): create tidak pernah menginisialisasi folder yang sudah ada atau tidak kosong. Folder instance yang sudah ada untuk ID itu (dipaksa test lewat sumber ID tiruan, dengan `instances/<id>/data/PG_VERSION` yatim) dan file yang muncul di folder data antara `create_instance_layout` dan pemeriksaan isi (lewat kait test) sama sama menghasilkan `instance.data_not_empty` (detail `path`) sebelum langkah `init` dimulai: `FakeProcessSupervision` mencatat nol spawn, file yatim dan file sisipan tetap identik, catatan pohon instance ditandai `not_owned` (detail `already_exists` untuk folder yang sudah ada, `data_not_empty` untuk isi yang muncul setelah pembuatan) sehingga kompensasi tidak menghapus folder itu, dan reference secret operasi itu dikompensasi.
- **AC-9** (settings dan plan): sebelum evaluasi, daemon mengisi setiap secret field di settings dengan reference yang akan dipakainya (reference turunan ID untuk `generate`, `value`, dan `copyFrom`, atau reference yang di `bind`), lalu memanggil `SettingsSchema::validate`. Setiap issue menjadi satu `FieldError` dengan path dan kode menurut tabel *Field error*, dan baris `settings` berstatus error dengan jumlah field bermasalah. Dry run `init_plan` dan `run_plan` dengan ID placeholder, path turunannya, dan port `Fixed` atau `defaultPort` untuk `Auto` yang gagal menghasilkan `instance.settings_invalid` atau `instance.endpoint_invalid` dengan path field yang disebut `PlanError`, atau path `settings` dengan kode `invalid_value` untuk varian `PlanError` yang tidak menyebut field. Field `managed` yang diisi dan key asing ditolak.
- **AC-10** (operasi create): `instance.create` mewajibkan idempotency key. Handler menjalankan `gather` di `spawn_blocking` sebelum `begin`, lalu `begin` mencari key lebih dulu (replay mengembalikan reply aslinya dan membuang hasil `gather`), lalu precheck menjalankan `evaluate` murni atas hasil itu: hasil dengan baris error ditolak sinkron dengan kode baris error pertama, ditambah semua field issue sebagai `fields` hanya kalau kode itu membawa `fields` (D7), tanpa membuat operasi. Selain itu daemon membuat `InstanceId` baru yang short key nya tidak ada di `known_instance_ids`, lalu `begin` dengan `creates_resource: true`, lock tambahan `LockKey::InstanceName(name)`, dan `BusyPolicy::Reject`. Langkah operasi persis seperti *Operasi `create_instance`*; langkah `register` membaca registry beserta generation nya, memeriksa ulang setiap aturan yang bersumber dari registry (nama, `multi_instance`, runtime ada dan `installed` dengan software sama, langkah 2 dan 3 *Pemeriksaan port*), lalu meng commit `InsertProviderRevision` (kalau belum ada), `InsertInstance`, `InsertConfigRevision`, dan `ReplaceSecretUses` dalam satu batch lewat `ctx.commit` dengan `expected_generation` dari baca itu, dan mengulang paling banyak lima kali saat `GenerationConflict` atau `DuplicateResource` untuk provider revision. Sepuluh create bersamaan dengan nama sama menghasilkan tepat satu instance; yang lain langsung ditolak `resource.busy` (dengan `operationId` pemenang) atau `instance.name_taken` kalau pemenangnya sudah selesai. Dua create bersamaan dengan nama berbeda dan port `Fixed` sama menghasilkan tepat satu instance dengan port itu, dan yang kalah gagal `supervisor.endpoint_conflict`. Yang kalah tidak meninggalkan folder, reference, atau baris registry.
- **AC-11** (gagal, cancel, dan terputus, BR-023): operasi create tidak punya titik tak bisa dibalik; semua langkahnya `reversible`. Kegagalan langkah mana pun, termasuk `init` (`supervisor.init_failed` dengan cuplikan, atau kegagalan sebelum `init_begin` seperti `secret.needs_secret` dari `resolve_for_start` atau `supervisor.data_in_use`), menjalankan kompensasi spec 0007 urut terbalik: baris registry lewat kompensator `instance_registration` (`WriteOp::DeleteInstanceCreatedBy`, lalu event `instance.removed`), pohon instance, lalu reference secret, sehingga tidak ada baris instance, folder instance, reference secret, atau pemakaian secret milik operasi itu yang tersisa, sedangkan folder paket yang di pin tetap ada; operasi berakhir `failed` dengan error langkah itu. Cancel saat `init` menunggu `init` selesai lalu mengompensasi dan berakhir `cancelled`. Daemon yang mati di langkah mana pun dikompensasi otomatis oleh `recover` dan dispatcher spec 0007 setelah daemon siap. Kompensator `instance_registration` dan `instance_tree` sama sama memeriksa `instance.lock` sebelum menghapus (file lock ada, lalu `probe_lock`, K10): lock yang dipegang proses lain (pembantu init yang tertinggal setelah daemon mati) membuat item `compensation_failed` dengan detail `instance_locked`, dan probe yang error membuat detail `lock_probe_failed`, keduanya tanpa menghapus apa pun. Baris registry, pohon instance, dan reference secret yang masih dipakai baris itu (`ON DELETE RESTRICT`) tetap ada, operasi menjadi `needs_recovery` dengan hint `abandon`, dan instance nya tetap terlihat `failed interrupted` lewat rekonsiliasi spec 0010 sampai abandon setelah lock bebas. Init yang gagal biasa tidak memicu penolakan ini, karena `execute_init` melepas `InstanceLock` daemon sebelum mengembalikan error (langkah 7 spec 0010, K10).
- **AC-12** (hasil, REQ-008, BR-001): instance yang berhasil dibuat punya ID stabil buatan daemon, nama, provider revision key exact, runtime ID exact, reference secret per secret field, satu config revision `active` dengan settings tervalidasi (secret sebagai teks reference), endpoint dengan host `defaultHost` dan permintaan port sesuai draft, tujuh baris path dari ID, `startup` `manual`, lifecycle `stopped` setelah `init_succeeded`, `created_at` registry sama dengan `createdAt` manifest (satu pembacaan `Clock` di langkah `layout`), dan `instance-manifest.json` yang `data.initializedAt` nya sama dengan `data_initialized_at` registry. Endpoint `unix` disimpan dengan nama socket hasil render `socketName` memakai port `Fixed` yang dirujuknya, atau dengan penanda `auto` di tempat port kalau endpoint rujukannya `Auto`, dan nama itu tidak pernah ditulis ulang. `InstanceSummary.endpoints[].socketPath` dihitung saat dibaca dari template `socketName` paket pinned dan port efektif endpoint rujukannya, dan bernilai null selama port efektif itu belum ada.
- **AC-13** (K-1, pin): `catalog::pin_revision` memenuhi kriteria K-1 spec 0017 apa adanya: folder 0700 dan file 0600 lewat `write_file_atomic`, `check_owned_path` sebelum IO, baca ulang dengan `NameCheck::Skip` dan digest dicek, file yang dicakup `manifest_sha256` dibandingkan byte demi byte (sama dilewati, beda menghasilkan `catalog.pin_failed` reason `existing_differs` tanpa menimpa), `versions.json` tidak dibandingkan, panggilan kedua tidak menambah `mutations()`, symlink atau entri asing menghasilkan `catalog.pin_failed` tanpa menimpa atau menghapus, delapan thread yang mem pin revision sama semuanya berhasil dengan isi sama, hasilnya `ProviderRevisionRecord` dengan `source_id`, pin tidak pernah menghapus file, dan kompensasi tidak pernah menghapus folder pinned. Create adalah satu satunya pemanggilnya di fase 02.
- **AC-14** (K-2, sumber): setelah registry `Ready` dan tidak dalam recovery mode, daemon memastikan tepat satu baris `provider_sources` untuk catalog yang dimuat dengan kolom di tabel K-2 spec 0017; restart daemon memakai baris yang sama; `last_sync_at` tetap NULL; kegagalan menulis menambah bagian `degraded` `catalog source not recorded: <kode>` ke subsystem `catalog`, dicoba lagi sekali tepat sebelum pin pertama, dan kalau tetap gagal pin ditolak `catalog.pin_failed` reason `source_unrecorded` tanpa satu file pun ditulis. Daemon tanpa catalog tidak membuat baris.
- **AC-15** (revision dan capability, BR-016): tanpa `provider.revision`, create memakai revision entri catalog yang dimuat saat `gather` dan mem pin nya. Dengan `provider.revision` yang sama dengan revision catalog, jalurnya sama. Dengan revision lain, create memakai provider revision yang sudah pinned di registry untuk nama dan revision itu (tepat satu digest dan folder nya cocok, jalur spec 0010), `instance.provider_ambiguous` untuk lebih dari satu, dan `instance.provider_not_found` kalau tidak ada. Paket tanpa capability `multi_instance` saat sudah ada instance tidak retained dari provider yang sama menghasilkan `instance.capability_unsupported` (detail `capability`, `provider`, `existing`), juga saat dua create seperti itu berjalan bersamaan (pemeriksaan ulang di `register`).
- **AC-16** (auto port dan race, REQ-010, AC-055): instance dengan endpoint `Auto` yang dibuat lalu di start menyimpan port di `bound` sejak spawn (aturan spec 0010) dan menampilkannya `bound: true` setelah readiness; test ujung ke ujung fitur ini membuktikannya lewat `instance.get`. Port `Fixed` yang lolos preflight lalu direbut proses luar di alamat endpoint yang persis sama sebelum start membuat `instance.start` gagal `supervisor.endpoint_in_use`, port tersimpan tetap sama, dan proses luar tetap hidup (listener wildcard di port itu tidak menggagalkan start, K2).
- **AC-17** (kontrak): method `instance.get_create_form` dan `instance.preflight` (keduanya `recovery refused`), DTO baru di *Method IPC*, newtype `SignedSafeInteger`, enum terbuka baru (masing masing diakhiri `Unrecognized`), dan kode baru di *Kode error* (pesan dan hint statis lewat `codes::<area>::hint`) ada di `stackly-protocol` dengan fixture `protocols/fixtures/` untuk setiap varian `Call` dan `Reply` baru, setiap kode, dan nilai asing setiap enum terbuka, terbaca di Rust dan Swift, dengan kenaikan minor `ProtocolVersion::CURRENT` (minor `CURRENT` di main ditambah 1 saat merge, K4) di commit yang sama dan `cargo xtask codegen --check` bersih. Field opsional `provider.revision` di params `instance.create` serta DTO dan fixture event `instance.removed` sudah dikirim langkah 12 spec 0010 (K4); fitur ini tidak mengubah bentuknya, mencabut penolakan `provider.revision` kosong milik create sementara, dan menjadi yang pertama mengirim event itu. Fixture `protocols/fixtures/instance/create-form-<fake|postgresql|nats>.json` dibangkitkan dari paket nyata dan dijaga test Rust.
- **AC-18** (CLI): `stackly instance create <name> --runtime <id|software@versi>` dengan flag di *CLI* membangun params dari `instance.get_create_form`, mengirim `instance.preflight` untuk `--dry-run` (exit 0 kalau `ready`, selain itu exit dari kategori error baris error pertama) dan `instance.create` untuk selain itu, mengikuti kontrak output, exit code, progress, dan `--wait` spec 0008. `--set` dikonversi menurut jenis field; nilai yang tidak bisa dikonversi dikirim sebagai teks sehingga daemon yang menolaknya. Port yang bukan angka 0 sampai 65535, `--set-json` yang bukan JSON, dan `--settings-file` yang tidak terbaca atau bukan object JSON gagal `cli.usage` (exit 2) tanpa memanggil daemon. Sumber secret hanya lewat `--secret <field>=<sumber>` dan tidak pernah berupa nilai di argv. `--start` menunggu create selesai lalu mengirim `instance.start` dengan idempotency key turunan. Perintah setara yang dibalas preflight, setelah dipecah seperti shell, diparse kembali oleh CLI menjadi draft yang sama dengan draft asal setelah keduanya diisi default form, untuk setiap fixture `protocols/fixtures/instance/commands.json`.
- **AC-19** (AC-028): untuk matriks input buruk di *Critical test scenarios* (nama, port, settings, secret, runtime, provider), params yang sama yang dikirim lewat `stackly-ipc` `Client` (jalur yang dipakai GUI) dan lewat `stackly instance create --json` menghasilkan `ErrorBody` dengan `code`, `category`, `details`, dan `fields` yang sama, dan stdout CLI tepat satu baris envelope tanpa progress.
- **AC-20** (sheet New Instance, PAGE-04): sheet mengikuti mockup dan *Sheet New Instance*: header, filter kategori, picker runtime, nama, satu baris port per endpoint IP dengan Fixed atau Automatic, field settings dari schema, toggle "Start after creating", daftar preflight, perintah setara dengan tombol Copy, Cancel, dan Create yang hanya aktif saat preflight terakhir `ready` untuk nilai form saat ini dan daemon tersambung. Sheet dibuka dari aksi `newInstanceAction` (toolbar dan empty state Overview dan Instances milik fitur 22), tombol "New Instance…" di panel detail Catalog (nonaktif dengan alasan saat belum ada runtime terpasang untuk provider itu), dan perintah New Instance… (⌘N), yang nonaktif saat jendela utama bukan key window atau onboarding tampil (spec 0069 AC-4, AC-9). Setelah Create, sheet menampilkan progress tahap operasi; sukses menutup sheet, mengirim `instance.start` kalau toggle aktif, dan memilih instance baru lewat `ShellNavigator`; gagal mengembalikan form dengan error di baris yang bersangkutan. Start yang gagal setelah sheet tertutup terlihat sebagai operasi `start_instance` di Operation center dan di `lastFailure` instance.
- **AC-21** (form dari schema, VAL-012, AC-029 bagian form UI): `SchemaFormModel` dan view nya merender setiap `FormField` hanya dari DTO: teks, integer, boolean, pilihan, dan daftar item; field secret lewat `SecretField` spec 0011 dengan Generate sebagai default; field `immutableAfterInit` diberi tanda "Set once"; field advanced di grup Advanced yang tertutup kecuali ada error di dalamnya; field managed terkunci dengan "Set by Stackly"; dan jenis yang tidak dikenal sebagai baris read only. Test Swift membangun form dari ketiga fixture `create-form-*.json` (fake, PostgreSQL, NATS) dan mendapat satu baris per field dengan kontrol yang benar; untuk NATS test itu memeriksa `serverName` sebagai field managed yang terkunci, `jetstream` sebagai toggle, tiga integer advanced (`jetstreamMaxMemoryBytes`, `jetstreamMaxFileBytes`, `maxPayloadBytes`) di grup Advanced dengan batasnya, dan dua baris port (`client`, `monitoring`) dengan Fixed atau Automatic (dipakai ulang AC-13 spec 0023). `cargo xtask check-deps` menolak nama atau ID provider di kode `CreateInstance/` Swift dan `instance::create` Rust.
- **AC-22** (AC-027, NFR-012): seluruh alur create bisa diselesaikan dengan keyboard (urutan fokus di *Aksesibilitas*, Return untuk Create saat aktif, Escape untuk Cancel) dan VoiceOver (label, nilai, error, dan status setiap baris preflight terbaca sebagai teks), status preflight dibedakan oleh bentuk simbol dan teks, bukan hanya warna, tidak ada informasi yang hanya muncul saat hover, teks memakai text style bawaan dan tidak terpotong di lebar sheet minimum 560 pt, yang menjadi cek teks besar di macOS 27 (spec 0012, Tipografi), dan semua pasangan warna lolos WCAG AA di light, dark, dan Increase Contrast; dibuktikan evidence manual keyboard dan VoiceOver di Mac ini.
- **AC-23** (fuzz, VAL-007): fuzz target `instance_path` (harness `stackly_core::instance::fuzzing::owned_path`) memanggil `check_owned_path` dan `check_data_root_overlap` dengan path acak di atas pohon fixture yang memuat symlink ke luar root, loop symlink, nama Unicode, dan folder dalam, dan memeriksa invariant di *Fuzz target*; ia berjalan 10 menit di Mac ini tanpa crash dengan evidence `covers = ["VAL-007"]`, dan seed nya dijalankan ulang sebagai test regresi di `stackly-core`.
- **AC-24** (kinerja): dengan 10 instance terdaftar di Mac ini, `instance.create` dijawab dalam 500 ms (NFR-006) dan `instance.preflight` dalam 300 ms pada persentil 95 untuk paket dengan dua endpoint IP tanpa pencarian saran, juga saat dialog Keychain dari aksi lain sedang terbuka.
- **AC-25** (PostgreSQL nyata): di Mac ini, PostgreSQL 18 dan 17 masing masing dibuat lewat CLI dan lewat GUI dengan password bangkitan dan port fixed, preflight menunjukkan konflik nyata saat port dipakai listener test, instance hasilnya bisa di start sampai `ready` (spec 0019), dan alur GUI dijalankan penuh dengan keyboard dan VoiceOver. Tanpa artefak di cache runtime xtask hasilnya `BLOCKED` dengan alasan dan owner.
- **AC-26** (NFR-009): review fresh model atas jalur create (params IPC, path, port, secret, kompensasi) tidak meninggalkan temuan critical atau high yang belum diselesaikan, dan test keamanan di *Critical test scenarios* (nama berbahaya, path traversal di nama, secret di argv dan log, symlink di folder providers dan instances) lulus.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md), ditambah semua perbaikan cross check yang disetujui lead atas nama user (2026-10-20): dua method baca baru (`instance.get_create_form` dan `instance.preflight`) di atas satu evaluator core murni yang juga menjadi precheck `instance.create`, dengan IO dikumpulkan `gather` di luar gerbang `begin`; form bertipe dari settings schema dalam urutan deklarasi; revision catalog saat create yang di pin pada saat itu; port fixed yang konflik ditolak dengan saran yang hanya dipakai lewat aksi pengguna, dengan bind tepat di alamat endpoint memakai `SO_REUSEADDR` sebagai error dan bind wildcard serta loopback keluarga lain sebagai caution; create yang seluruhnya bisa dibalik, termasuk init yang gagal, dengan kompensator pendaftaran registry dan event `instance.removed`; `BusyPolicy::Reject` untuk create bernama sama; "Start after creating" sebagai rangkaian di client dengan key turunan; Start at login ditunda ke fitur 31; perintah setara dibangun satu fungsi di `stackly-protocol`; sheet yang tetap terbuka selama operasi; dan socket path yang dihitung saat dibaca dari template paket pinned.

Daemon adalah satu satunya yang memvalidasi dan memutuskan; GUI dan CLI hanya mengirim draft dan menampilkan hasil evaluasi yang sama.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya: tidak ada `unwrap()` di luar test, kontrol native SwiftUI menggantikan tiruan HTML di mockup, dan validasi pattern tetap di daemon, bukan di Swift.

## Feature design

### Trace PRD

| ID PRD | Dipenuhi oleh |
| --- | --- |
| REQ-008 (create terkontrol) | AC-8, AC-10, AC-11, AC-12 |
| REQ-010 (endpoint dan konflik resource) | AC-4, AC-5, AC-12, AC-16 |
| REQ-029 (bagian form) | AC-1, AC-21 |
| PAGE-04 (Create Instance) | AC-20, AC-21, AC-22; autostart (Start at login) dibangun fitur 31 |
| BR-002 (data root tidak bertumpuk) | AC-6 |
| BR-004 (alamat, transport, port, wildcard) | AC-4 |
| BR-005 (fixed konflik error, auto hanya pilihan manager) | AC-4, AC-5, AC-16 |
| BR-008 (hanya folder baru milik operasi) | AC-8, AC-11 |
| BR-023 (kegagalan tidak menghapus milik operasi lain) | AC-8, AC-11 |
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
| `InstanceName`, `Endpoint`, `endpoints_conflict`, `resolve_port`, `create_instance_layout`, `check_owned_path`, `check_data_root_overlap`, manifest instance | spec 0004 (validasi endpoint di `Instance::new` dan `volume_facts` sudah ditambah untuk fitur 20 pada 2026-10-20) |
| `WriteOp`, baca registry, `provider_sources`, `ProviderSourceRecord` | spec 0005 (*Tambahan fitur 20*, sudah masuk pada 2026-10-20) |
| Engine operasi, kompensasi, `claim_resource`, `record_secret_intent`, `ctx.commit`, `failure_items`, kompensator `instance_tree` | spec 0007 (baris resume `create_instance`, detail `data_not_empty`, dan aturan `instance_tree` K10 lewat *Usulan update spec lain*, diterapkan di spec 0007 pada 2026-10-20, keputusan 29 dan 30; `failure_items` dari langkah 16 fitur 7, K11) |
| Settings schema, `SettingsSchema::validate`, urutan deklarasi, `SettingsIssueKind`, `x-stackly.printableOnly`, plan, `PlanError` | spec 0009 (tambahan fase 02 sudah masuk pada 2026-10-20) |
| `execute_init`, `PortProbe::try_bind` (langkah 5 spec 0010), start, `InstanceSummary`, event instance termasuk DTO `instance.removed` (langkah 12 spec 0010), method instance, create sementara (`// provisional: F20 until F20 step 9`), area `instance` dan `supervisor` | spec 0010 (langkah 9 spec ini mengganti create sementara dengan *Target fitur 20* spec 0010) |
| `SecretService::put`, `FieldContext`, `SecretFieldRule`, `classify_create_failure`, `SecretInput::field_errors`, `SecretField`, reference, kode `secret.*` | spec 0011 (tambahan fase 02, K13 sampai K15, sudah masuk pada 2026-10-20) |
| Catalog read model, `CapabilityStatus`, `ProviderSummary.software`, kontrak K-1 dan K-2 | spec 0017 (K-1 dan K-2 dibangun di sini) |
| `RuntimeSummary`, `RuntimeStore`, `runtime.list`, `available_space` | spec 0018 |
| Paket PostgreSQL | spec 0019 (`printableOnly` di model settings spec 0009; `pin_package` di fitur `fixture` core spec 0010, K5) |
| Paket NATS dan probe readiness NATS | spec 0023 |
| Halaman Instances dan Overview, empty state Instances, tata letak toolbar, `ShellNavigator` | fitur 22 (fitur ini memberi aksi `NewInstanceAction` lewat environment dan memanggil `ShellNavigator.show`; sebelum fitur 22 dibangun, aksi dipasang di `ProvisionalInstanceList` spec 0013, K17) |
| Summary, connection string, socket path untuk connection | fitur 21 |
| Start at login, `startup` `at_login` | fitur 31 |
| Purge instance yang tertinggal dari create terputus | fitur 54 (abandon operasi spec 0007 sudah membersihkannya lebih dulu) |
| Komponen dasar (checklist, sheet dengan footer, progress bertahap, tile, umpan balik) | fitur 67 ([spec 0067](../0067-komponen-dasar-stackly/index.md)); `PreflightSection` dan `CreateProgressSection` adalah pembungkus layar di `CreateInstance/` di atas `CheckList` dan `StagedProgress` (diperbarui 2026-10-21) |
| Peta perintah dan shortcut | fitur 69 ([spec 0069](../0069-menu-aplikasi-perintah-shortcut/index.md)): baris `newInstance` di `CommandMap` menjadi built, dan `AppShell` menerbitkan action dari `newInstanceAction` (spec 0069 *Penerbit*); `CommandGroup` sementara hanya menurut aturan langkah 14 (diperbarui 2026-10-21) |

### Alur create

1. Pengguna memilih runtime di sheet (dari `RuntimeStore`) atau CLI menerima `--runtime`.
2. Client memanggil `instance.get_create_form { runtimeId }` dan mendapat provider, endpoint, field, dan nama saran. GUI mengisi draft awal: nama saran, setiap endpoint IP `Fixed` dengan `defaultPort`, nilai default setiap field, dan setiap secret field `generate`.
3. Setiap perubahan draft (GUI, jeda `PREFLIGHT_DEBOUNCE`) atau `--dry-run` (CLI) memanggil `instance.preflight`, yang menjalankan `gather` lalu `evaluate` dan membalas baris preflight, field error, saran port, dan perintah setara.
4. Create mengirim `instance.create` dengan draft dan `SecretInput` lengkap. Handler menjalankan `gather`, lalu `begin` dengan precheck `evaluate`; kalau ada error, ditolak sinkron. Kalau tidak, operasi `create_instance` dimulai.
5. Operasi menjalankan `validate`, `pin`, `secrets`, `layout`, `register`, dan `init`, lalu selesai `succeeded`; kegagalan atau cancel di mana pun mengompensasi semuanya.
6. Kalau "Start after creating" aktif (GUI) atau `--start` (CLI), client mengirim `instance.start` dengan key turunan setelah create `succeeded`.

### Model data

Tidak ada tabel atau kolom baru dan tidak ada migrasi. Yang ditulis create:

| Data | Tempat | Isi | Ditulis | Dihapus kompensasi |
| --- | --- | --- | --- | --- |
| folder paket pinned | `providers/<namespace>/<nama>/<revision>/<manifest_sha256>/` | salinan file paket catalog | langkah `pin` (K-1) | tidak pernah |
| `provider_sources` | registry | satu baris per catalog yang dimuat (K-2) | start daemon, `InsertProviderSource` (spec 0005) | tidak pernah |
| `provider_revisions` | registry | `ProviderRevisionRecord` dengan `source_id` | batch `register`, hanya kalau belum ada | tidak pernah (immutable, idempoten, dan dipakai bersama; pengecualian sengaja atas aturan kompensator `ctx.commit` spec 0007, butir 4 blok spec 0007 di *Usulan update spec lain*) |
| `secret_references` dan audit | registry | baris reference baru, status `present` | `SecretService::put` di langkah `secrets` (transaksi nya sendiri); sumber `bind` tidak membuat baris | ya, kompensator `secret_ref` |
| `instances`, `instance_endpoints`, `instance_paths` | registry | aggregate `Instance::new` | batch `register` | ya, `instance_registration` |
| `config_revisions` | registry | config revision awal `active` | batch `register` (`InsertConfigRevision`) | ya, ikut cascade |
| `secret_reference_uses` | registry | peta field ke reference | batch `register` (`ReplaceSecretUses`) | ya, `DeleteInstanceCreatedBy` menghapusnya eksplisit |
| folder instance | `instances/<id>/` dan `logs/instances/<id>/` | manifest, `data/`, `config/`, `config-history/`, `temp/` | langkah `layout` (`create_instance_layout`) | ya, `instance_tree` (kecuali `not_owned`) |
| isi folder data | `instances/<id>/data/` | hasil init provider | langkah `init` (`execute_init` spec 0010) | ya, bersama pohon instance |
| journal operasi | registry | input draft tanpa nilai secret, langkah, catatan dibuat | engine spec 0007 | tidak (riwayat) |

`Instance::new` dipanggil dengan: `id` dari handler, `name`, `display_name`, `provider_revision` dari `ValidatedPackage::key()`, `software` paket, `runtime_id`, `data_line` dan `data_version` dari record runtime, `endpoints` dari *Endpoint di aggregate*, `startup: StartupPolicy::Manual`, `created_at` dari pembacaan `Clock` yang sama dengan `createdAt` manifest di langkah `layout`, dan `created_by` operasi create. Lifecycle awalnya `uninitialized`.

### Endpoint di aggregate

- Endpoint `tcp` dan `udp` manifest menjadi endpoint IP (`Endpoint::Ip` dengan `IpEndpoint` yang dibangun lewat konstruktor domain spec 0004) dengan nama, transport, alamat `defaultHost`, permintaan dari draft (`Fixed(port)` atau `Auto`; tanpa permintaan berarti `Auto`), dan tanpa `bound`.
- Endpoint `unix` menjadi endpoint Unix dengan nama dan `SocketName` hasil render *Socket Unix dengan token port*.
- `Instance::new` mengembalikan `Result<Instance, InvariantError>` dan menolak nama endpoint ganda (`DuplicateEndpoint`, rule None), `bound` yang terisi (`BoundOnNew`, rule None), nama socket Unix ganda (`DuplicateSocket`, BR-004), dan endpoint IP draft yang saling konflik (`endpoints_conflict` di antara port `Fixed` instance itu sendiri, `EndpointSelfConflict`, BR-004) (spec 0004, *Model domain*).

### Socket Unix dengan token port

`socketName` manifest boleh memuat `${endpoint.<tcp>.port}` (spec 0009), misalnya `.s.PGSQL.${endpoint.client.port}` milik PostgreSQL. Plan selalu merender socket dari paket pinned dan port saat itu, jadi nilai di registry hanya pengisi kolom wajib dan dasar aturan socket ganda.

- Port rujukan `Fixed`: nama dirender dengan port itu saat create.
- Port rujukan `Auto`: token port diganti teks `auto` (misalnya `.s.PGSQL.auto`, charset `SocketName` dan selalu muat karena spec 0009 menghitung token port 5 byte).
- Nama itu tidak pernah ditulis ulang oleh create atau start.
- `InstanceSummary.endpoints[].socketPath` (spec 0010) dihitung `stacklyd` saat membangun summary: folder run ditambah `socketName` paket pinned yang dirender dengan port efektif endpoint rujukannya, atau null kalau endpoint rujukan belum punya port efektif. Template dibaca dari paket pinned dan di cache di memori daemon per provider revision key. Endpoint Unix tanpa token port memakai nama tersimpan apa adanya.

### Resolusi provider dan revision

| Masukan | Hasil |
| --- | --- |
| `provider.revision` kosong, catalog `Loaded`, entri provider dengan nama itu | paket entri catalog, sumber `bundled_catalog` atau `development_catalog`, di pin di langkah `pin` |
| `provider.revision` kosong, catalog `NotConfigured` atau `Unavailable` | `catalog.unavailable` |
| nama tidak ada di catalog (dan revision kosong) | `instance.provider_not_found` |
| entri `Planned` atau `Invalid` | `instance.provider_unavailable` (detail `state` `planned` atau `invalid`) |
| `provider.revision` sama dengan revision entri catalog | sama dengan baris pertama |
| `provider.revision` lain: `gather` mendaftar `providers/<namespace>/<nama>/<revision>/` dengan `Filesystem::read_dir` dan menemukan tepat satu folder digest, `Registry::provider_revision(key)` untuk kunci itu ada, dan `read_package` dengan `NameCheck::Skip` membaca folder itu dengan digest yang sama | paket pinned, sumber `pinned`, tanpa pin |
| lebih dari satu folder digest di bawah `<revision>/` | `instance.provider_ambiguous` |
| tidak ada folder digest, atau provider revision nya tidak ada di registry | `instance.provider_not_found` |

`instance.get_create_form` selalu memakai baris pertama (form untuk instance baru selalu dari catalog). Pemilihan entri dari runtime: entri provider yang `ValidatedPackage::software()` sama dengan software runtime; di fase 02 hanya namespace `official`, jadi paling banyak satu. Fitur 37 yang nanti memakai `provider` untuk memilih di antara beberapa.

### Method IPC

Semua method lewat handshake dan peer user yang sama (spec 0006). Port di wire bertipe `u16` seperti spec 0010.

| Method atau event | Recovery | Input | Output | Error utama |
| --- | --- | --- | --- | --- |
| `instance.get_create_form` | refused | `GetCreateFormParams { runtimeId: Uuid, provider: Option<String> }` | `CreateForm` | `instance.runtime_not_found`, `supervisor.runtime_unavailable`, `instance.provider_not_found`, `instance.provider_ambiguous`, `instance.provider_unavailable`, `instance.runtime_mismatch`, `catalog.unavailable` |
| `instance.preflight` | refused | `PreflightCreateParams` | `CreatePreflight` | hanya `request.invalid_params` (batas ukuran), `daemon.starting`, dan `registry.recovery_mode`; masalah provider dan catalog menjadi baris `provider`, bukan error method |
| `instance.create` (handler pengganti create sementara) | refused (spec 0010) | params spec 0010, bentuk final sejak langkah 12 spec 0010 (`provider.revision` opsional; kosong berarti revision catalog) | `InstanceActionAccepted` (operasi `create_instance`) | sinkron: `request.invalid_params` dari `Validate`, kode baris error pertama `evaluate` dengan `fields`, atau `resource.busy` dari lock nama; operasi: lihat *Operasi `create_instance`* |
| event `instance.removed` | | | `InstanceRemoved { instanceId: Uuid }` (DTO dan fixture dari langkah 12 spec 0010) lewat `publish_resource(Instance(id), revision terakhir + 1)` | dikirim pertama kali oleh fitur ini, setelah kompensasi menghapus baris instance; fitur 54 memakainya ulang untuk purge |

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
                            #[serde(default)] pub host: Option<String>, #[serde(default)] pub default_port: Option<u16>,
                            #[serde(default)] pub socket_name: Option<String> }
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
pub struct InstanceRemoved { pub instance_id: Uuid }   // sudah ada sejak langkah 12 spec 0010, tidak dibuat ulang
```

- Params `instance.create` tetap bentuk spec 0010 (`name`, `displayName?`, `provider: { name, revision? }`, `runtimeId`, `endpoints`, `settings`, `secrets: [{ field, input: SecretInput }]`), yang sudah final sejak langkah 12 spec 0010, termasuk `provider.revision` opsional; fitur ini tidak mengubah params create, dan handler nya tidak lagi menolak `provider.revision` kosong. Kalau spec 0010 langkah 12 belum memberi nama tipe untuk object settings, namanya `SettingsDocument`.
- `SignedSafeInteger` adalah newtype baru di `wire.rs` untuk `i64` di antara minus dan plus `MAX_SAFE_INTEGER`, dipetakan `typeshare.toml` ke `Int64` Swift, sama polanya dengan `Count`.
- `Validate` untuk `PreflightCreateParams` dan params `instance.create` memeriksa batas ukuran (nama paling banyak 256 byte, nama tampil 512 byte, endpoint 16, secret 64, key field 64 byte, reference 255 byte, provider 127 byte, revision 64 byte). `Validate` params `instance.create` juga memanggil `SecretInput::field_errors(&format!("secrets[{i}].input"))` untuk setiap entri `secrets` (dibangun langkah 12 spec 0010, spec 0011 AC-7), sehingga nilai kosong, NUL, dan lebih dari 65536 byte menjadi `request.invalid_params` di `secrets[<i>].input.content`. Format nama, port, dan settings diperiksa `evaluate`, supaya preflight tetap membalas semua baris saat satu field salah.
- Kalau paket tidak bisa ditentukan (baris `provider` error), baris `endpoint` manifest, `settings`, dan `secret` tidak ada karena jumlahnya bergantung pada paket, dan baris `runtime` hanya memeriksa keberadaan dan state runtime. Tidak ada status "dilewati".
- Path `FieldError` yang dipakai: `name`, `displayName`, `runtimeId`, `provider.name`, `provider.revision`, `endpoints[<i>].name`, `endpoints[<i>].port`, `settings`, `settings.<field>`, `settings.<field>[<j>].<item>`, `secrets[<i>].field`, dan `secrets[<i>].input.content` (hanya di create: `request.invalid_params` dari `Validate`, dan field issue `secret.invalid_value` dari precheck). Indeks mengikuti urutan array di params.

### Evaluator create

Modul baru `stackly-core::instance::create`:

```rust
pub struct CreateDraft { pub name: String, pub display_name: Option<String>, pub provider: ProviderChoice,
                         pub runtime_id: RuntimeId, pub endpoints: Vec<(String, PortRequestDraft)>,
                         pub settings: serde_json::Value, pub secrets: Vec<(String, SecretSourceKind)>,
                         pub secret_values: Vec<(String, SecretValueShape)>,  // hanya create: jumlah karakter dan ada karakter kontrol
                         pub start: bool }
pub struct CreateDeps<'a> { pub registry: &'a dyn Registry, pub catalog: &'a CatalogState, pub source: Option<ProviderSourceId>,
                            pub platform: &'a Platform, pub prepared: &'a PreparedLayout, pub probe: &'a dyn PortProbe }
pub struct CreateFacts { /* catalog entry atau paket pinned, runtime dan semua runtime installed, instance tidak retained,
                            ID yang dikenal, hasil PortProbe per endpoint dan saran, volume, ruang, keadaan store, reference sumber */ }
pub fn gather(deps: &CreateDeps<'_>, draft: &CreateDraft) -> Result<CreateFacts, GatherError>;  // satu satunya IO
pub fn evaluate(draft: &CreateDraft, facts: &CreateFacts) -> Evaluation;                         // fungsi murni
pub fn build_form(entry: &ProviderEntry, runtime: &Runtime, facts_names: &[InstanceName], meta: &CatalogMeta) -> CreateFormModel;
pub struct Evaluation { pub checks: Vec<Check>, pub field_issues: Vec<FieldIssue>, pub resolved: Option<ResolvedCreate>,
                        pub command: CommandSpec }
```

- `gather` hanya membaca: catalog di memori, `Registry` (`provider_revision`, `runtime`, `runtimes`, `instances(include_retained: false)`, `known_instance_ids`, reference secret untuk sumber `copyFrom` dan `bind`), untuk jalur `pinned`, `Filesystem::read_dir` folder `providers/<namespace>/<nama>/<revision>/` lalu `read_package` dengan `NameCheck::Skip` atas satu satunya folder digest (*Resolusi provider dan revision*), `symlink_metadata` untuk executable runtime dan folder `instances/` serta `retained/`, `Filesystem::volume_facts`, `available_space`, `PortProbe::try_bind` (langkah 4 sampai 6 *Pemeriksaan port*, lalu pencarian saran hanya untuk endpoint `Fixed` yang gagal langkah 3 atau 4), `SecretStore::state`, `backend`, dan `probe` (interaksi `Never`; `Busy` menjadi caution). Tidak ada tulisan, spawn, atau dialog. Handler `instance.create` dan `instance.preflight` menjalankannya di `spawn_blocking`, di luar gerbang `begin` engine.
- `evaluate` murni dan diuji dengan tabel kasus tanpa fake. Urutan baris dan aturan setiap baris ada di tabel di bawah; field issue diurutkan menurut path.
- `SecretValueShape { chars: u64, has_control: bool }` dibuat `secret::redact::value_shape(&SecretValue)`, fungsi kecil di samping `char_count` dan `has_control_character` (spec 0011) yang hanya mengembalikan dua nilai itu, jadi tidak ada tempat `expose()` baru. Handler mengubah `SecretText` menjadi `SecretValue` di `ipc::secret` `stacklyd` (spec 0011 AC-7), dan `instance::create` tidak pernah melihat nilai secret. `evaluate` memeriksa bentuk itu dengan `SecretFieldRule::check(chars, has_control)`, aturan yang sama dengan `put` (K13, K15).
- `ResolvedCreate` (paket, runtime, endpoint domain, settings tervalidasi, rencana secret, nama exact) hanya ada kalau tidak ada error; operasi memakainya, preflight membuangnya.
- `CommandSpec { name, runtime_label, provider: Option<String>, revision: Option<String>, display_name, ports, settings: Vec<(key, nilai)> (hanya yang beda dari default), secrets: Vec<(field, jenis sumber dan reference)>, start }` adalah tipe core: `runtime_label` adalah `software@versi` kalau tepat satu runtime `installed` cocok di `CreateFacts`, selain itu UUID; `provider` dan `revision` hanya terisi kalau draft mengirimnya; tanpa paket yang bisa ditentukan, `settings` kosong. DTO wire `CommandDraft` dengan field yang sama tinggal di `stackly_protocol::instance::command` di samping `equivalent_command`, dan `stacklyd` memetakan `CommandSpec` ke `CommandDraft` (langkah 9), karena `stackly-core` tidak bergantung pada `stackly-protocol` (spec 0001) dan CLI butuh tipe wire itu untuk round trip (D8).
- `build_form` membangun `CreateFormModel`, tipe core (provider dan sumbernya, record runtime, nama saran, endpoint dengan purpose, field settings bertipe dari `SettingsSchema::fields()`, dan `CapabilitySet`), dari entri catalog, runtime, nama instance tidak retained (untuk `suggestedName`), dan `CatalogMeta`. `stacklyd` memetakannya ke DTO `CreateForm`, termasuk `RuntimeSummary` (spec 0018) dan `CapabilityStatus` (spec 0017), di langkah 9 (D8).

| Baris | Aturan (berhenti di gagal pertama dalam baris) | Status dan kode |
| --- | --- | --- |
| `provider` | *Resolusi provider dan revision*; `multi_instance` (AC-15) | error dengan kode tabel itu atau `instance.capability_unsupported`; selain itu `ok` dan baris disembunyikan |
| `runtime` | record ada; software sama dengan paket; state `installed`; `runtime.executable` dan setiap `additionalExecutables` adalah file biasa yang bisa dieksekusi di dalam root runtime; install mode | `instance.runtime_not_found`, `instance.runtime_mismatch`, `supervisor.runtime_unavailable`, `instance.runtime_incompatible`; external menjadi `caution`; selain itu `ok` |
| `name` | parse `InstanceName`; tidak dipakai instance tidak retained; `DisplayName` kalau ada | `instance.name_invalid`, `instance.name_taken` |
| `endpoint.<n>` | *Pemeriksaan port* untuk setiap endpoint IP manifest (urut seperti form), lalu satu baris per permintaan tidak valid | `instance.endpoint_invalid`, `supervisor.endpoint_conflict`, `supervisor.endpoint_in_use`, `supervisor.endpoint_unavailable`, atau `caution` |
| `settings` | AC-7 (bagian settings), `SettingsSchema::validate`, dry run plan (AC-9) | `instance.settings_invalid` atau `instance.endpoint_invalid`; baris disembunyikan saat `ok` |
| `data` | `instances/` dan `retained/` folder privat milik user; `volume_facts` lokal dengan `apfs` atau `hfs` | `supervisor.data_unavailable`, `instance.storage_unsupported` |
| `disk` | `available_space(instances/)` minimal `CREATE_MIN_FREE_BYTES` | `instance.insufficient_space` |
| `secret.<field>` | sumber AC-7; backend store bukan `unsupported`; `copyFrom` atau `bind` terdaftar dan punya nilai; bentuk nilai `value` (hanya create, `SecretFieldRule::check`) | `secret.store_unsupported`, `secret.invalid_reference`, `secret.not_found`, `secret.needs_secret`, `secret.invalid_value`; store `locked` atau probe `Busy` menjadi `caution` |

Error sinkron `instance.create` adalah `ErrorBody` dari baris error pertama menurut urutan di atas. `fields` berisi semua field issue hanya kalau kode itu mendaftarkan `omitted` dan tidak mengizinkan `items` (aturan spec 0006, D7): `instance.name_invalid`, `instance.settings_invalid`, `instance.endpoint_invalid`, `supervisor.endpoint_in_use`, `supervisor.endpoint_conflict`, `supervisor.endpoint_unavailable`, dan `secret.invalid_value` (kunci `omitted` di tabel spec 0010 dan 0011 ditambah pemiliknya, D7). Untuk kode lain, termasuk `secret.needs_secret` yang membawa `items`, `fields` tidak dikirim; field issue nya tetap ada di `CreatePreflight.fields`, yang dipakai sheet setelah menjalankan preflight lagi.

### Field error

Spec 0009 memberi setiap issue settings jenis yang tetap (`SettingsIssueKind`, spec 0009); evaluator memetakannya ke `FieldCode` dari set tetap spec 0006. `message` setiap `FieldError` adalah teks tetap kode itu (spec 0006), tidak pernah memuat nilai input atau batas schema. Teks yang menyebut batas disusun GUI dari `code` dan batas `FormField` (kolom *Teks GUI*), dan untuk baris nama, endpoint, dan secret ada di `detail` baris preflight (*Copy preflight*).

| Jenis issue | Kode `FieldError` | Teks GUI di bawah kontrol |
| --- | --- | --- |
| key asing (`unknown`) | `unknown_value` | "Unknown setting." |
| tipe salah (`type`) | `invalid_value` | "Use a {text, whole number, true or false, list}." |
| field managed diisi (`managed_filled`) | `invalid_value` | "Set by Stackly; leave it out." |
| karakter kontrol (`control_characters`) | `invalid_value` | "Remove control characters." |
| key item ganda (`duplicate_key`) | `invalid_value` | "Each {itemKey} must be unique." |
| pattern tidak cocok (`pattern`) | `invalid_format` | "Doesn’t match the required format." |
| terlalu pendek (`too_short`) | `too_short` | "Use at least {minLength} characters." |
| terlalu panjang (`too_long`) | `too_long` | "Use at most {maxLength} characters." |
| di luar rentang (`out_of_range`) | `out_of_range` | "Use a whole number from {minimum} to {maximum}." |
| bukan pilihan (`enum`) | `unknown_value` | "Choose one of the listed values." |
| item terlalu banyak (`too_many_items`) | `too_many` | "Use at most {maxItems} items." |
| item terlalu sedikit (`too_few_items`) | `too_short` | "Use at least {minItems} items." |
| wajib kosong (`required`) | `required` | "Required." |
| `PlanError` tanpa field | `invalid_value` di path `settings` | "These settings can’t be turned into a valid configuration." |

Field issue di luar settings memakai `FieldCode` tetap berikut:

| Path | Kondisi | Kode `FieldError` |
| --- | --- | --- |
| `name` | bukan `InstanceName` valid | `invalid_format` |
| `name` | dipakai instance tidak retained | `invalid_value` |
| `displayName` | bukan `DisplayName` valid | `invalid_format` |
| `runtimeId` | runtime tidak ada | `unknown_value` |
| `runtimeId` | runtime bukan `installed`, software nya berbeda, atau executable nya tidak lengkap | `invalid_value` |
| `provider.name` | provider tidak ada | `unknown_value` |
| `provider.name` | ambigu, planned, invalid, atau tanpa `multi_instance` saat instance lain dari provider itu ada | `invalid_value` |
| `provider.revision` | revision lain yang tidak ada | `unknown_value` |
| `endpoints[<i>].name` | endpoint tidak ada di manifest | `unknown_value` |
| `endpoints[<i>].name` | endpoint `unix`, atau endpoint yang sama disebut dua kali | `invalid_value` |
| `endpoints[<i>].port` | di luar 1 sampai 65535 | `out_of_range` |
| `endpoints[<i>].port` | konflik di draft, konflik dengan instance lain, dipakai, atau tidak bisa di bind (langkah 2 sampai 4 *Pemeriksaan port*) | `invalid_value` |
| `secrets[<i>].field` | tidak ada di schema atau bukan secret field | `unknown_value` |
| `secrets[<i>].field` | dikirim dua kali | `invalid_value` |
| `settings.<field>` | secret field ikut di `settings` | `invalid_value` |
| `secrets[<i>].input.content` | nilai `value` melanggar aturan field (hanya create) | `invalid_value` |

`catalog.unavailable`, baris `data` dan `disk`, dan masalah reference sumber secret (`secret.not_found`, `secret.needs_secret`, `secret.invalid_reference`, `secret.store_*`) tidak membuat field issue; semuanya hanya baris preflight dan kode error.

### Pemeriksaan port

Untuk endpoint IP dengan port `Fixed` p di alamat A (`defaultHost`) dan transport T, berurutan:

| Langkah | Pemeriksaan | Gagal |
| --- | --- | --- |
| 1 | p di 1 sampai 65535 dan endpoint ada di manifest | `instance.endpoint_invalid` (error) |
| 2 | tidak konflik dengan endpoint lain di draft (`endpoints_conflict`) | `instance.endpoint_invalid` (error) |
| 3 | tidak konflik dengan port efektif atau port `Fixed` setiap instance tidak retained di registry (`endpoints_conflict`, BR-004) | `supervisor.endpoint_conflict` (error) |
| 4 | `PortProbe::try_bind(T, A:p, { reuse_addr: true, v6only: A IPv6 })` | `InUse`: `supervisor.endpoint_in_use` (error); `Denied` atau `Unavailable`: `supervisor.endpoint_unavailable` (error, K2) |
| 5 | `try_bind(T, wildcard keluarga yang sama:p, { reuse_addr: false, v6only: true untuk IPv6 })` | `InUse`: `caution`, port dipakai di alamat lain Mac ini |
| 6 | kalau A loopback, `try_bind(T, loopback keluarga lain:p, { reuse_addr: false, v6only: true untuk IPv6 })` | `InUse`: `caution`, client yang memakai `localhost` bisa sampai ke server lain |

- `PortProbe::try_bind(transport, addr, BindOptions { reuse_addr, v6only }) -> BindOutcome` dengan `BindOutcome` `Free`, `InUse`, `Denied`, atau `Unavailable` adalah port spec 0010 yang dibangun langkah 5 spec 0010 (K2); langkah 6 spec ini memakainya ulang tanpa implementasi kedua. Socket dibuat lewat `socket2`, opsi dipasang, `bind` saja tanpa `listen`, lalu langsung ditutup. `std::net` tidak dipakai karena ia memasang `SO_REUSEADDR` dan memanggil `listen` sendiri.
- `Denied` atau `Unavailable` di langkah 4 menjadi error `supervisor.endpoint_unavailable` (detail `endpoint`, `address`, `port`, `reason` `denied` atau `unavailable`), sama dengan tahap `endpoints` start (spec 0010, K2), karena server juga tidak akan bisa bind di alamat itu. Di langkah 5 dan 6 keduanya menjadi `caution` "couldn’t check this port", tidak pernah `ok`.
- Langkah 4 sampai 6 hanya jalan kalau langkah 1 sampai 3 lolos. Pencarian saran (AC-5) memakai langkah 2 sampai 6 untuk setiap kandidat.
- Endpoint `Auto` hanya menjalankan langkah 1 dan berstatus `ok` dengan keterangan bahwa port dipilih saat start; alokasi dan probe nya milik start (spec 0010).
- Langkah `validate` menjalankan pemeriksaan yang sama; langkah `register` hanya mengulang langkah 2 dan 3 atas data registry yang dibaca bersama generation nya.

*Hasil yang diharapkan* untuk endpoint TCP `127.0.0.1:p` (semantik bind BSD: bind alamat spesifik dengan `SO_REUSEADDR` berhasil di samping wildcard milik socket lain dan di samping sisa `TIME_WAIT`, tetapi gagal saat socket lain memegang alamat yang sama; bind wildcard tanpa `SO_REUSEADDR` gagal saat alamat apa pun di port itu dipegang):

| Listener lain | Hasil |
| --- | --- |
| TCP `127.0.0.1:p` (`LISTEN`) | error `supervisor.endpoint_in_use` (langkah 4) |
| TCP `0.0.0.0:p` | `caution` alamat lain (langkah 5) |
| TCP `[::]:p` dual stack | `caution` alamat lain (langkah 5) |
| TCP `[::1]:p` | `caution` loopback keluarga lain (langkah 6) |
| TCP di alamat LAN Mac ini, port p | `caution` alamat lain (langkah 5) |
| sisa `TIME_WAIT` di `127.0.0.1:p` setelah server berhenti | `ok` |
| UDP `127.0.0.1:p` | `ok` |

Kalau test native milestone 4 menunjukkan perilaku macOS yang lain dari tabel ini, test itu gagal dan keputusan probe kembali ke `/architect` update spec ini; implementasi tidak menyesuaikan tabel sendiri.

### Nama

- `suggestedName`: `<software>-<line>` dengan titik di line diganti tanda hubung (misalnya `postgresql-18`, `mysql-8-4`); kalau dipakai instance tidak retained, tambahkan `-2` sampai `-99`; bagian software dipotong supaya hasil paling panjang 63 karakter. Kalau semuanya dipakai, kosong.
- Validasi memakai `InstanceName::parse`; nama tidak pernah menjadi path atau argumen shell, karena path diturunkan dari ID.

### Operasi `create_instance`

Handler `instance.create`: key wajib, `gather` di `spawn_blocking`, `InstanceId::generate()` diulang sampai short key nya tidak ada di `known_instance_ids`, lalu `begin(BeginRequest { kind: create_instance, resource: Instance(id), creates_resource: true, extra_locks: [InstanceName(name)] kalau nama valid, busy: Reject, expected_revision: None, origin: Request(..), input, work, precheck })`. Engine mencari key dulu (replay membuang hasil `gather`), lalu precheck menjalankan `evaluate` atas hasil `gather` itu; error berarti `Reject`. `OperationError` spec 0007 tidak punya saluran `fields`, jadi precheck juga menyimpan `Evaluation` nya di slot yang dipegang handler (misalnya `Arc<OnceLock<Evaluation>>`); saat `begin` mengembalikan `Rejected`, `stacklyd` memasang field issue dari slot itu lewat `with_fields` menurut aturan *Error sinkron* di *Evaluator create*, tanpa downcast. `input` adalah `JournalPayload` berisi draft tanpa nilai secret (sumber `value` dicatat sebagai `value` saja); kalau hasilnya melewati `INPUT_MAX`, settings diganti `{ "settingsOmitted": true, "settingsSha256": "<hex>" }`. Handler ini menggantikan handler create sementara spec 0010 (`// provisional: F20 until F20 step 9`) di langkah 9; penanda itu dan penolakan `provider.revision` kosong hilang di commit yang sama.

Semua langkah `reversible: true`, jadi operasi ini tidak punya titik tak bisa dibalik dan kebijakan interrupt nya tetap `Compensate`.

| Langkah | `repeatable` | Isi | Gagal |
| --- | --- | --- | --- |
| `validate` | ya | `gather` dan `evaluate` ulang di bawah lock | kode baris error pertama |
| `pin` | ya | untuk sumber catalog: `ensure_source` kalau sumber belum tercatat (K-2), lalu `pin_revision` (K-1); sumber `pinned` dilewati | `catalog.pin_failed` |
| `secrets` | tidak | per secret field, berurutan: untuk sumber selain `bind`, `record_secret_intent(ref)`; lalu `SecretService::put(PutTarget::New { .., field: Some(FieldContext) }, input, Create, None, interaksi pemicu)` (untuk `Bind`, `put` hanya membaca sekali dan memeriksa aturan field, spec 0011 AC-23); untuk sumber selain `bind`, `confirm_created`. Kegagalan `put` dipetakan `classify_create_failure` | `secret.needs_secret` (item hanya untuk reference sumber), `secret.invalid_value`, `secret.conflict`, `secret.not_found`, `secret.store_locked`, `secret.store_access_denied`, `secret.store_busy`, `secret.store_*` lain |
| `layout` | tidak | baca `Clock` sekali (`created_at`), `claim_resource`, `InstanceManifest::new`, `create_instance_layout(platform, prepared, manifest, known)`, `check_data_root_overlap`, pemeriksaan folder data (folder, bukan symlink, milik user, privat, nol entri), lalu `confirm_created`; `AlreadyExists` berarti `not_created(already_exists)`, isi tidak kosong setelah pembuatan berarti `not_created(data_not_empty)`, `ShortKeyTaken` berarti `not_created(short_key_taken)` | `instance.data_not_empty`, `instance.data_overlap`, `instance.short_key_taken`, `supervisor.data_unavailable` |
| `register` | tidak | `record_custom_intent(instance_registration, <id>)`, lalu paling banyak lima kali: baca registry dan generation, periksa ulang semua aturan yang bersumber dari registry (AC-10), `ctx.commit(FinalBatch { expected_generation, ops: [InsertProviderRevision?, InsertInstance, InsertConfigRevision, ReplaceSecretUses] })`; `GenerationConflict` dan `DuplicateResource` provider revision diulang; sukses berarti `confirm_created` | `instance.name_taken`, `instance.capability_unsupported`, `supervisor.runtime_unavailable`, `supervisor.endpoint_conflict`, `registry.generation_conflict` setelah lima kali, kode registry lain |
| `init` | tidak | `execute_init` spec 0010 dengan interaksi pemicu: lock instance, `resolve_for_start`, `init_begin`, langkah `InitPlan`, `init_succeeded`, manifest dengan `data.initializedAt` | `supervisor.init_failed` (dengan cuplikan), `supervisor.data_in_use`, `secret.*` dari `resolve_for_start`, `secret.delivery_rejected` |

- `finalize` tidak menulis apa pun selain yang ditambahkan engine; `result.resources` berisi `Instance(id)`.
- Config revision awal: `ConfigRevisionId::generate()`, `values` dari `SettingsValues::to_json()` (secret sebagai teks reference), status `active`, `created_by_operation` operasi ini, `created_at` sama dengan `created_at` instance, tanpa `rendered_sha256` dan `previous` (spec 0010).
- `ReplaceSecretUses { user: Instance(id), uses }` memetakan setiap secret field ke reference hasil `put` atau reference yang di `bind`. `put` dengan `Bind` tidak menulis reference atau pemakaian; pemakaiannya ditulis di batch ini (spec 0011).
- Kompensator kustom `instance_registration` (didaftarkan `stacklyd`, karena ia juga mengirim event): baca instance; tidak ada berarti `Compensated`; `created_by` bukan operasi ini berarti `NotOwned` reason `created_by_other_operation`; selain itu, periksa `instance.lock` lebih dulu dengan urutan yang sama dengan `instance_tree` (`symlink_metadata` file lock; tidak ada berarti bebas; ada berarti `probe_lock`, dengan `Held` atau state lain selain `Free` menjadi item `compensation_failed` detail `instance_locked` dan error probe menjadi `lock_probe_failed`, tanpa menghapus apa pun), lalu commit `WriteOp::DeleteInstanceCreatedBy { id, expected, operation }` (spec 0005), lalu `instance.removed`, lalu `Compensated`. Kepemilikan cukup dibuktikan `created_by`, karena kompensasi hanya berjalan selama operasi pembuatnya memegang lock key instance itu, sehingga tidak ada start yang bisa terjadi di antaranya. Probe lock dibutuhkan karena kompensasi berjalan urut terbalik: tanpa probe, baris registry sudah hilang saat `instance_tree` menolak, dan instance yang masih dipakai pembantu init tidak terlihat lagi sampai abandon (K1, K10). Karena catatannya lebih baru dari catatan reference secret, ia dikompensasi lebih dulu, sehingga pemakaian secret sudah hilang sebelum reference dihapus (`ON DELETE RESTRICT`).
- Kompensator `instance_tree` spec 0007 mengikuti urutan K10 (usulan spec 0007, dibangun langkah 3): pemeriksaan manifest (manifest operasi lain berarti `not_owned`, manifest yang tidak terbaca berarti `compensation_failed` detail `manifest_unreadable`), `symlink_metadata(InstancePaths.lock)` (tidak ada berarti bebas, probe dilewati), `probe_lock` (`Free` lanjut; `Held` atau state lain menjadi `compensation_failed` detail `instance_locked`; error probe menjadi `compensation_failed` detail `lock_probe_failed`; keduanya tanpa menghapus apa pun), lalu `remove_owned_tree`. Penolakan itu hanya mungkin saat pembantu init masih berjalan setelah daemon mati, karena `execute_init` melepas `InstanceLock` daemon sebelum mengembalikan error (langkah 7 spec 0010, K10).
- Cancel di langkah mana pun menjalankan kompensasi; cancel saat `init` berlaku setelah `execute_init` kembali, karena langkah `Run` init tidak dibatalkan di tengah.
- `WorkFactory` create membangun ulang operasi antri yang belum mulai setelah daemon restart dari `input`; input dengan sumber `value` atau `settingsOmitted` menghasilkan `NotRebuildable`, sehingga operasi itu ditutup `failed interrupted`.

### Pin dan sumber catalog

- `catalog::pin` dan `catalog::source` di `stackly-core` membangun K-1 dan K-2 spec 0017 persis dengan signature di sana (`pin_revision`, `PinnedRevision`, `PinError`, `ensure_source`).
- `stacklyd` memanggil `ensure_source` di slot start setelah registry `Ready` dan catalog dimuat, menyimpan `Option<ProviderSourceId>` dan error terakhir di state daemon, dan menambah bagian `degraded` subsystem `catalog` seperti K-2. Langkah `pin` membaca state itu dan memanggil `ensure_source` sekali lagi kalau kosong.
- Kode `catalog.pin_failed` ditambah ke `codes::catalog` dengan kategori, `retryable`, details, dan pesan dari K-1, dan hint statis "run stackly doctor" lewat `codes::catalog::hint`.

### Kode error

Area `instance` milik spec 0010, area `catalog` milik spec 0017, area `registry` milik spec 0005, dan area `cli` milik spec 0008; fitur ini menambah kode berikut lewat aturan "fitur yang pertama kali memakainya". Sembilan kode `instance` di tabel ini sudah tercatat di tabel kode spec 0010 dengan tanda *(fitur 20, spec 0020)* dan dibangun langkah 8 beserta pesan dan hint statis nya; langkah 12 spec 0010 boleh menambah `instance.name_invalid` lebih dulu (K4). `registry.not_deletable` dibangun langkah 2. Semua `retryable` false. Pesan bahasa Inggris, huruf kecil di awal, tanpa titik, tanpa nilai settings teks atau secret. Hint selalu teks statis dari `codes::<area>::hint(code, details)` yang dipasang `stacklyd` lewat `ErrorBody::with_hint`.

| Kode | Kategori | Kapan | Kunci `details` | Pesan | Hint statis |
| --- | --- | --- | --- | --- | --- |
| `instance.name_invalid` | `invalid_input` | nama atau nama tampil tidak valid; `fields` | `omitted` | `the instance name is not valid` | "use lowercase letters, digits, and single hyphens, starting with a letter" |
| `instance.provider_unavailable` | `unsupported` | entri catalog planned atau invalid | `provider`, `state` | `{provider} can't be used to create instances` | per `state`: `planned` "this provider is not in this release"; `invalid` "reinstall Stackly, this provider package failed validation" |
| `instance.capability_unsupported` | `unsupported` | `multi_instance` tidak ada dan instance provider itu sudah ada (BR-016) | `capability`, `provider`, `existing` | `{provider} runs one instance at a time` | "remove the existing instance first" |
| `instance.runtime_incompatible` | `conflict` | executable yang dibutuhkan paket tidak ada di runtime | `runtimeId`, `executable` | `the runtime lacks a program the provider needs` | "reinstall the runtime with stackly runtime install" |
| `instance.storage_unsupported` | `unsupported` | root data bukan volume lokal `apfs` atau `hfs` | `fsType`, `root` | `the data folder is on an unsupported file system` | "keep the Stackly data folder on a local APFS volume" |
| `instance.insufficient_space` | `unavailable` | ruang bebas di bawah batas | `needed`, `available` | `not enough free disk space to create an instance` | "free up disk space, then try again" |
| `instance.data_not_empty` | `conflict` | folder instance sudah ada atau folder data tidak kosong (BR-008) | `path` | `the new instance folder is not empty` | "run stackly doctor; Stackly never initializes a folder that already holds data" |
| `instance.data_overlap` | `conflict` | folder data bertumpuk dengan instance lain (BR-002, `rule`) | `path`, `otherInstance` | `the new data folder overlaps another instance` | "run stackly doctor" |
| `instance.short_key_taken` | `conflict` | balapan short key (spec 0007) | `shortKey` | `the new instance ID collided with another instance` | "try again, a new ID is generated" |
| `catalog.pin_failed` | `failed` | K-1 | `provider`, `revision`, `reason` | `the provider package could not be pinned` | "run stackly doctor" |
| `registry.not_deletable` | `conflict` | `DeleteInstanceCreatedBy` ditolak (spec 0005) | `reason` (`created_by_other_operation` atau `active`); instance nya di `ErrorBody.resource` (`ResourceKind` `instance`) | `the instance can't be removed by this operation` | |
| `cli.runtime_not_found` | `not_found` | `--runtime software@versi` tidak cocok | `selector` | `no installed runtime matches {selector}` | "run stackly runtime list" |
| `cli.runtime_ambiguous` | `conflict` | lebih dari satu runtime cocok | `selector`, `candidates` | `more than one installed runtime matches {selector}` | "pass the runtime ID instead" |

Kode yang dipakai ulang: `instance.name_taken`, `instance.provider_not_found`, `instance.provider_ambiguous`, `instance.runtime_not_found`, `instance.runtime_mismatch`, `instance.settings_invalid`, `instance.endpoint_invalid` (spec 0010), `supervisor.endpoint_conflict`, `supervisor.endpoint_in_use`, `supervisor.endpoint_unavailable` (K2), `supervisor.runtime_unavailable`, `supervisor.data_unavailable`, `supervisor.data_in_use`, `supervisor.init_failed` (spec 0010), `secret.*` (spec 0011), `catalog.unavailable` (spec 0017), `registry.generation_conflict` (spec 0005), dan `resource.busy` (spec 0006 dan 0007). Path di `details` selalu relatif terhadap root data, tidak pernah path home penuh. Kode yang bisa membawa `fields` mendaftarkan `omitted` (aturan spec 0006): `instance.name_invalid` di tabel ini, serta `instance.settings_invalid`, `instance.endpoint_invalid`, `supervisor.endpoint_in_use`, `supervisor.endpoint_conflict`, dan `supervisor.endpoint_unavailable` di tabel spec 0010 dan `secret.invalid_value` di tabel spec 0011, yang ditambah pemiliknya (D7).

### CLI

```text
stackly instance create <name> --runtime <runtime-id | software@versi>
    [--provider <namespace/nama>] [--revision <label>] [--display-name <teks>]
    [--port <endpoint>=<port|auto>]... [--set <field>=<nilai>]... [--set-json <field>=<json>]...
    [--settings-file <path.json>] [--secret <field>=<sumber>]... [--start] [--dry-run]
    [--wait] [--timeout <durasi>] [--idempotency-key <key>] [--json]
```

- `--runtime software@x` dicari CLI lewat `runtime.list` di antara runtime `installed` dengan software itu: lebih dulu yang `version` nya sama persis dengan x, kalau tidak ada yang `line` nya sama dengan x. Tepat satu cocok dipakai; nol cocok gagal `cli.runtime_not_found` (exit 10); lebih dari satu (misalnya managed dan external dengan versi sama, atau dua versi di satu line) gagal `cli.runtime_ambiguous` (exit 3) dengan ID kandidat. UUID diteruskan apa adanya.
- CLI selalu memanggil `instance.get_create_form` lebih dulu untuk provider dan jenis field. `--provider` dan `--revision` mengisi `provider`; tanpa `--revision`, revision dikosongkan supaya daemon memakai revision catalog.
- `--set` dikonversi menurut `FieldKind`: `integer` lewat parse `i64`, `boolean` dari `true` atau `false`, `text` dan `choice` apa adanya; nilai yang gagal dikonversi dikirim sebagai teks. `--set-json` untuk field `list`. `--settings-file` dibaca lebih dulu, lalu flag menimpa key yang sama. Key yang tidak ada di form tetap dikirim, supaya daemon menolaknya dengan kode `unknown_value`.
- Validasi sisi CLI hanya tiga, karena tipe wire tidak bisa membawa nilainya: angka port di `--port` di luar 0 sampai 65535 atau bukan angka, `--set-json` yang bukan JSON, dan `--settings-file` yang tidak terbaca atau bukan object JSON; ketiganya `cli.usage` (exit 2) tanpa memanggil daemon. Port 0 tetap dikirim supaya daemon yang menolaknya.
- `--secret <field>=<sumber>` dengan sumber `generate` (default untuk field yang tidak disebut), `prompt` (TTY dengan echo mati, dua kali, aturan spec 0011 AC-17), `stdin` (paling banyak satu field), `file:<path>`, `env:<NAMA>`, `copy:<ref>`, atau `bind:<ref>`. Tanpa TTY, `prompt` langsung gagal exit 2. Nilai dibaca lewat modul `secret_input` spec 0011.
- Tanpa `--port`, endpoint IP berarti `auto` (spec 0010). Perintah setara selalu menulis `--port` untuk setiap endpoint IP.
- `--dry-run` hanya memanggil `instance.preflight`. `--json` memberi `result` `{ "preflight": CreatePreflight }`; teks manusia satu baris per pemeriksaan (`ok`, `warn`, atau `fail`, judul, keterangan) lalu field error.
- Tanpa `--dry-run`: `--json` memberi `result` `{ operation, idempotencyKey, instance, start }` (pola perintah async spec 0008). `instance` berisi `InstanceSummary` setelah create `succeeded` dan ditunggu (selalu terisi dengan `--start`, terisi dengan `--wait`, null selain itu). `start` berisi `{ operation, idempotencyKey }` kalau `--start`. `--start` menunggu operasi create selesai walau tanpa `--wait`; dengan `--wait`, operasi start juga ditunggu. Progress tahap di stderr: Checking, Pinning provider package, Storing secrets in Keychain, Creating folders, Registering, Initializing.
- Key start: hex huruf kecil SHA-256 dari `<key create>:start`, sehingga mengulang perintah yang sama dengan `--idempotency-key` yang sama tidak membuat start kedua. GUI memakai turunan yang sama.
- Exit code mengikuti kategori spec 0008. Create yang ditolak sinkron menulis `error` dengan `fields` kalau kodenya membawa `fields` (D7), dan teks manusia menulis setiap field error di baris `<path>: <pesan>`.

**Perintah setara**: `stackly_protocol::instance::command::equivalent_command(&CommandDraft) -> String`, fungsi murni di samping DTO `CommandDraft` di modul yang sama, yang dipanggil `stacklyd` untuk `CreatePreflight.command` setelah memetakan `CommandSpec` core (D8), dan diuji CLI dengan round trip. Urutan: `stackly instance create <name> --runtime <runtime_label>`, `--provider` dan `--revision` kalau terisi, `--display-name` kalau ada, `--port` untuk setiap endpoint IP dalam urutan form, `--set` atau `--set-json` hanya untuk field yang nilainya berbeda dari default (urutan form), `--secret <field>=<generate|prompt|copy:ref|bind:ref>` untuk setiap secret field (sumber `value` ditulis `prompt`), lalu `--start` kalau diminta. Kutip tunggal POSIX dipakai untuk nilai yang memuat karakter di luar `[A-Za-z0-9._/:@%+=,-]`. Perintah ini tidak pernah memuat nilai secret. Round trip membandingkan kedua draft setelah default form diisi ke keduanya.

### Sheet New Instance (GUI)

Folder baru `StacklyUI/CreateInstance/`: `CreateInstanceModel`, `SchemaFormModel`, `NewInstanceSheet`, `EndpointPortRow`, baris field, `PreflightSection`, `EquivalentCommandRow`, dan `CreateProgressSection`. *Komponen (2026-10-21, spec 0067):* `NewInstanceSheet` memakai `StacklySheet` dengan `minWidth: 560`, dan `Form` di dalamnya memberi `.scrollContentBackground(.hidden)`; `PreflightSection` memetakan setiap `PreflightCheck` ke `CheckItem` (`ok` ke `passed`, `caution` ke `warning`, `error` ke `failed`, `unrecognized` ke `unrecognized`; `id` dari `PreflightCheck.id`, `title` dan `detail` dari daemon, `action` nil karena tombol "Use {n}" tetap di baris Port) di dalam `CardSection("Preflight")` dengan `CheckList` `.adaptive`; sebelum balasan preflight pertama dan saat preflight gagal dijalankan, kartu yang sama menampilkan `DelayedProgress` atau "Couldn’t run preflight." dengan Retry di luar `CheckList` (*Preflight* di bawah); `CreateProgressSection` memakai `StagedProgress(value, layout: .stages, announcesStages: true)` dengan enam `StageDefinition` create (`validate` "Checking", `pin` "Pinning provider package", `secrets` "Storing secrets in Keychain", `layout` "Creating folders", `register` "Registering", `init` "Initializing"). `StacklyClient` mendapat `getCreateForm`, `preflight`, params create yang diperluas, dan event `instance.removed` dari tipe hasil generate.

- **Model**: `CreateInstanceModel` (`@Observable`, main actor) dengan state `choosingRuntime`, `loadingForm`, `formFailed(FailureContent)`, `editing`, `submitting`, `creating(operationId)`, dan `finished`. Masukannya `RuntimeStore` (spec 0018), `CatalogStore` (spec 0017), `MutationPath` dan `OperationCenter` (spec 0013), dan protokol `CreateInstanceService` (`getCreateForm`, `preflight`) supaya test memakai tiruan. Setiap perubahan draft menaikkan penghitung generasi, membatalkan preflight yang berjalan, dan memanggil preflight setelah `PREFLIGHT_DEBOUNCE`; balasan untuk generasi lama dibuang.
- **Header**: `tile` `StacklySheet` (`TileSpec` dari `mark`, `tile`, dan `displayName` entri `CatalogStore` runtime terpilih), judul "New Instance", subjudul "Its own data directory, port and secret. Nothing is shared with other instances."
- **Filter kategori**: picker bersegmen All, Databases, Messaging, Analytics, Cache, Proxy (kategori spec 0017; `testing` hanya terlihat di All). Kategori setiap runtime diambil dari entri `CatalogStore` yang `ProviderSummary.software` nya sama dengan `RuntimeSummary.software`. Mengganti kategori memilih runtime pertama di kategori itu, atau mengosongkan picker dengan teks "No installed runtimes in this category."
- **Runtime**: `Picker` berisi runtime `installed` dengan label "{nama tampil} {versi}" ditambah " (external)" untuk install external, urut nama tampil lalu `installedAt` terbaru. Tanpa runtime terpasang sama sekali, isi sheet menjadi `StatePlaceholder` "No Runtimes" dengan pesan "Install a runtime from the catalog to create instances." dan tombol Browse Catalog (menutup sheet dan membuka Catalog).
- **Nama**: `TextField` berlabel "Name", terisi `suggestedName` dan terpilih saat sheet dibuka; error dari `fields[name]` tampil di bawahnya.
- **Port**: satu baris per endpoint IP, label "Port · {nama endpoint}", picker bersegmen Fixed dan Automatic, lalu `TextField` angka untuk Fixed atau teks "Assigned when the instance starts" untuk Automatic. Teks yang bukan angka 0 sampai 65535 tidak dikirim ke preflight; baris menampilkan "Enter a port from 1 to 65535." dan Create nonaktif (satu satunya aturan sisi client, karena tipe wire `u16`). Error baris endpoint tampil di bawahnya dengan kalimat "Fixed ports are never changed silently." dan tombol "Use {n}" kalau ada `suggestedPort`; caution tampil dengan simbol segitiga dan keterangannya. Endpoint Unix tidak tampil sebagai baris.
- **Settings**: `SchemaForm` (*Form dari settings schema*).
- **Toggle**: "Start after creating" dengan subjudul "Initialise the data directory, start, and wait for readiness.", default aktif. Toggle "Start at login" tidak tampil sampai fitur 31.
- **Preflight**: judul "Preflight" lalu satu baris per `PreflightCheck` (simbol, `title`, `detail`). Selama preflight pertama belum kembali, baris menampilkan `DelayedProgress`; preflight yang gagal (koneksi atau error method) menampilkan "Couldn’t run preflight." dengan Retry, dan Create tetap nonaktif.
- **Perintah setara**: `ConsoleBlock(command:)` spec 0067 (teks "$ {command}" yang bisa dipilih dan tombol Copy yang menyalin perintah tanpa "$ " lewat `PlainPasteboard`), dengan pengumuman "Command copied." (diperbarui 2026-10-21). Baris tampil setelah balasan preflight pertama dan memakai `command` dari balasan terakhir yang diterima; saat preflight gagal dijalankan, perintah terakhir tetap tampil.
- **Footer**: Cancel (Escape) dan Create (tombol default, Return), Create aktif hanya saat `MutationAvailability` `.available`, preflight terakhir `ready` untuk generasi draft saat ini, dan semua `SecretField` berisi nilai yang lolos batas panjang dan `printableOnly` di sisi client (kenyamanan saja; precheck create tetap memeriksanya, AC-7). *Komponen (2026-10-21, spec 0067):* Create adalah `primary` `StacklySheet`, `StacklyAction(id: "create", title: "Create", role: .standard(confirmation: nil), mutates: true)`, `isBusy` true selama `loadingForm`, `submitting`, dan `creating`, dan `disabledReason` dari kondisi pertama yang berlaku di urutan ini: tidak ada runtime terpasang (isi sheet placeholder "No Runtimes") "Install a runtime first."; `formFailed` "Couldn’t load the form."; belum ada runtime terpilih "Choose a runtime."; ada error sisi client (port bukan angka 1 sampai 65535, secret di luar batas) atau preflight terakhir untuk generasi ini punya baris `error` "Fix the errors above."; preflight gagal dijalankan "Run preflight again to continue."; preflight generasi ini belum kembali "Checking…". Gate `.blocked` tidak ditulis di `disabledReason`, karena footer `StacklySheet` sudah menampilkan alasan gate.
- **Create**: `MutationPath.perform(.instanceCreate(params))` dengan key baru. `rejected` dengan error sinkron menampilkan `StateBanner` critical di atas form berisi pesan daemon dan menjalankan preflight lagi; `resource.busy` dari lock nama memakai pesan yang sama; `notSent` dan `outcomeUnknown` memakai copy `MutationFeedback` spec 0013 sebagai `StateBanner` caution di atas form, tanpa aksi Show Operations di dalam sheet (2026-10-21), dan percobaan ulang setelah `outcomeUnknown` memakai key yang sama. Operasi yang diterima mengubah sheet ke mode progress: form read only, `CreateProgressSection` menampilkan tahap operasi dari `OperationCenter.updates(for:)` (Checking, Pinning provider package, Storing secrets in Keychain, Creating folders, Registering, Initializing), dan baris preflight secret berubah menjadi "Stored in Keychain" setelah tahap `secrets` selesai. Cancel di mode ini memanggil `operation.cancel` setelah konfirmasi spec 0012 ("Stop creating this instance?", dengan teks "Everything created so far is removed. Initialization in progress finishes first."). *Komponen (2026-10-21, spec 0067):* di mode ini `cancel` `StacklySheet` adalah `StacklyAction` berjudul "Cancel…" (judul aksi yang dikonfirmasi diakhiri "…") dengan `ActionRole.standard(confirmation: ActionConfirmation(title: "Stop creating this instance?", message: "Everything created so far is removed. Initialization in progress finishes first.", actionTitle: "Stop"))`; konfirmasinya tampil di atas sheet lewat `.actionHost()` milik `StacklySheet`. `Cancel…` ber `mutates` true karena mengirim `operation.cancel`; selama gate `.blocked` di mode `creating`, `cancel` diganti `StacklyAction(id: "close", title: "Close", mutates: false)` yang hanya menutup sheet, sehingga sheet tidak pernah terkunci, dan operasi tetap berjalan dan terlihat di Operations.
- **Selesai**: `succeeded` menutup sheet, mengirim `instance.start` lewat `MutationPath` dengan key turunan (*CLI*) kalau toggle aktif, lalu memanggil `ShellNavigator.show(.instances(filter: .all, select: id))` spec 0022 dengan instance ID dari `result.resources` (sebelum fitur 22 dibangun: pindah ke bagian Instances dengan daftar sementara spec 0013). Setelah sheet tertutup, `AppShell` (yang mempresentasikan sheet lewat `newInstanceAction`) mengirim hasil ke `FeedbackCenter` spec 0067, dengan `{name}` dari nama di draft: `.success("“{name}” created.")` tanpa start, atau `.started("“{name}” created. Starting it now.")` saat toggle aktif dan `instance.start` diterima; kalau `instance.start` lanjutan `rejected`, `notSent`, atau `outcomeUnknown`, `Feedback(tone: .caution, message: "“{name}” created, but it didn’t start.", detail: Feedback.outcome(...)?.message)`, supaya kabar create tidak hilang (diperbarui 2026-10-21). Start yang gagal setelah itu terlihat sebagai operasi `start_instance` di `OperationCenter` dan di `lastFailure` instance yang terpilih. `failed` atau `cancelled` mengembalikan mode edit dengan banner berisi error operasi (atau "Creation was cancelled. Nothing was kept."), baris preflight yang terkait tahap gagal ditandai error, dan preflight dijalankan lagi.
- **Titik masuk**: `AppShell` memasang nilai environment `newInstanceAction: NewInstanceAction?` (*Titik sambung New Instance* spec 0022); langkah 14 mendefinisikan struct dan kunci environment itu kalau belum ada konsumen yang lebih dulu. Selama nilainya ada, fitur 22 merender tombol toolbar "New Instance" di Overview dan Instances serta "New Instance…" sebagai aksi kedua empty state Instances (placeholder dan copy nya milik fitur 22), sebagai `ActionButton` atas `StacklyAction` `newInstance` yang sama dengan ⌘N (spec 0022, 2026-10-21). Sebelum fitur 22 dibangun, langkah 14 (setelah langkah 16 spec 0013 di merge) menambah tombol toolbar "New Instance" dan aksi kedua "New Instance…" di empty state `ProvisionalInstanceList` spec 0013, bertanda `// provisional: F22`, hanya saat `newInstanceAction` tidak nil dan dengan `.mutationGated()` (AC-17 spec 0013 yang ditulis ulang, K17); pesan empty state "Create one with stackly instance create." tetap. Kalau milestone 3 fitur 22 di merge lebih dulu, langkah 14 hanya menyambung halaman fitur 22 dan tidak menyentuh daftar sementara. Titik masuk lain: tombol "New Instance…" di panel detail Catalog untuk entri provider (nonaktif dengan alasan "Install a version first." kalau `RuntimeStore` tidak punya runtime `installed` untuk software itu, dan membuka sheet dengan kategori dan runtime terpilih: runtime `installed` software itu dengan `installedAt` terbaru), dan perintah menu File > New Instance… (⌘N), yang juga nonaktif saat jendela utama bukan key window atau onboarding tampil (spec 0069 AC-4, AC-9; ditambah 2026-10-21). Semua titik masuk nonaktif saat `MutationAvailability` `.blocked`, lewat `.mutationGated()` spec 0012.
- **Daftar instance**: `InstanceStore` spec 0013 menerapkan `instance.removed` sebagai penghapusan baris (delete pada revision event), sehingga instance yang sempat muncul saat `register` hilang lagi setelah kompensasi. Case reduce itu ditambah langkah 12, kecuali langkah 16 spec 0013 sudah memetakannya karena DTO langkah 12 spec 0010 sudah ada saat itu (K16).

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
- Field error dari `fields` dicocokkan lewat path dan tampil di bawah kontrolnya; error yang tidak cocok dengan field mana pun tampil di baris preflight `settings`. Teks nya disusun GUI dari `code` dan batas `FormField` menurut kolom *Teks GUI* tabel *Field error*, karena `message` di wire adalah teks tetap per kode (spec 0006).

### Aksesibilitas

- Urutan fokus: filter kategori, runtime, nama, baris port (picker mode lalu angka lalu tombol Use), field settings dalam urutan form, Advanced, toggle, baris preflight (masing masing satu elemen), tombol Copy, Cancel, Create.
- Label VoiceOver: field "{title}, {jenis kontrol}, required" plus "set once at creation" untuk `immutableAfterInit`; error dibacakan sebagai nilai tambahan; baris preflight "{title}, {Passed | Warning | Failed}: {detail}" (status `unrecognized` dibaca "Unrecognized status") dari `CheckList` spec 0067; tombol saran "Use port {n}". Perubahan status preflight diumumkan lewat `announce(_:)` spec 0012, sekali per balasan preflight untuk generasi draft saat ini dan hanya saat ringkasannya berubah dari yang terakhir diumumkan: "Preflight passed.", "Preflight passed with {n} warnings.", atau "{n} preflight checks failed.". Hasil create diumumkan lewat `FeedbackCenter` (diperbarui 2026-10-21).
- Status preflight memakai tabel *Status checklist* spec 0067 lewat pemetaan `PreflightSection` di atas: `ok` ke `passed` (tone `positive`, `checkmark.circle.fill`), `caution` ke `warning` (`caution`, `exclamationmark.triangle.fill`), `error` ke `failed` (`critical`, `xmark.circle.fill`), dan `unrecognized` ke `unrecognized` (`caution`, `questionmark.diamond.fill`). Tabel itu sudah ada di design.md dari spec 0067, jadi langkah 14 tidak menambah tabel status preflight (diperbarui 2026-10-21). Bentuk simbol berbeda per status, dan warna tone hanya tambahan, bukan satu satunya pembeda; Differentiate Without Color tidak mengubah apa pun.
- Sheet memakai `Form` `.grouped` di dalam isi `StacklySheet`, yang bergulir sendiri (tinggi isi dibatasi tinggi layar yang terlihat dikurangi 160 pt, spec 0067), lebar minimum 560 pt, sehingga teks besar menggulir tanpa terpotong (diperbarui 2026-10-21).

### Copy preflight

Ditulis daemon (`title`, `detail`), sama di GUI dan CLI. Ukuran memakai satuan desimal seperti spec 0017 (misalnya "41 GB").

| Baris | `title` | `detail` |
| --- | --- | --- |
| `provider` (gagal) | "{displayName} revision {revision}" atau "Provider {nama}" | pesan error |
| `runtime` | "Runtime {displayName} {versi}" | `ok` "Verified {12 hex pertama digest}"; external "External runtime · not reproducible"; error pesan error |
| `name` | "Name available" | `ok` nama; error "Use 1 to 63 lowercase letters, digits, or hyphens, starting with a letter." atau "An instance named {nama} already exists."; nama tampil salah "Use up to 128 characters without control characters." |
| `endpoint` fixed | "Endpoint {host}:{port}" | `ok` "Free on IPv4 and IPv6 loopback"; error "In use by {instance}", "In use by another process", atau "macOS won’t let Stackly use this port" (`supervisor.endpoint_unavailable`); caution "Also in use on {alamat}; clients that connect to localhost may reach that server" atau "In use on another address of this Mac" atau "Couldn’t check this port" |
| `endpoint` auto | "Endpoint {host} · {nama endpoint}" | "Port assigned when the instance starts" |
| `endpoint` permintaan tidak valid | "Endpoint {nama yang diminta}" | "This provider has no endpoint by that name." atau "Sockets don’t take a port." atau "This endpoint is listed twice." |
| `settings` (gagal) | "Settings" | "{n} settings need attention" |
| `data` | "Empty data directory" | `ok` "instances/<new instance id>/data · created new for this instance"; error pesan error |
| `disk` | "Disk space" | "{bebas} free · needs at least 256 MB" |
| `secret` | "{title field} stored in Keychain" | `generate` "Generated on create, never logged"; `value` "Entered value, never logged"; `copyFrom` "Copied from {ref}"; `bind` "Uses {ref}"; error `bind` atau `copyFrom` tanpa nilai "{ref} has no stored value yet"; caution locked "Keychain is locked; macOS asks you to unlock it when you create"; caution probe sibuk "Keychain is busy; checked again when you create" |

### Konstanta

| Konstanta | Nilai | Tempat |
| --- | --- | --- |
| `CREATE_MIN_FREE_BYTES` | 256 000 000 byte, ditampilkan "256 MB" | `instance::create` |
| `PORT_SUGGESTION_ATTEMPTS` | 64 | `instance::create` |
| `PORT_SUGGESTION_MIN` | 1024 | `instance::create` |
| `REGISTER_RETRY_LIMIT` | 5 (aturan spec 0005) | `instance::create` |
| `NAME_SUGGESTION_MAX_SUFFIX` | 99 | `instance::create` |
| `PREFLIGHT_DEBOUNCE` | 300 ms | `CreateInstanceModel` |
| `PREFLIGHT_PLACEHOLDER_ID` | UUID v7 tetap `00000000-0000-7000-8000-000000000000` untuk dry run plan di `evaluate` | `instance::create` |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| form | provider dan revision | entri catalog yang software paketnya sama dengan software runtime (`Catalog::entries`), `ValidatedPackage::key()` |
| form | `source` | `CatalogMeta.location` (`Bundled` atau `Development`) |
| form | runtime | `Registry::runtime(runtimeId)` dipetakan ke `RuntimeSummary` seperti spec 0018 |
| form | `suggestedName` | software paket, line runtime, nama instance tidak retained (*Nama*) |
| form | endpoint | `resources.endpoints` paket (transport, purpose, `defaultHost`, `defaultPort`, `socketName`) |
| form | field, kelas, batas, default | `SettingsSchema::fields()` dalam urutan deklarasi (spec 0009) |
| form | `printableOnly` | `x-stackly.printableOnly` field (spec 0009, ditambah untuk spec 0019) |
| form | `capabilities` | `CapabilitySet` paket dalam urutan capability spec 0017 |
| form | `dataPath` | teks tetap `instances/<new instance id>/data` |
| preflight, create | provider terpilih | *Resolusi provider dan revision* |
| preflight, create | runtime cocok | record runtime, `symlink_metadata` executable di `runtime_dir(..)` |
| preflight, create | nama dipakai | `Registry::instance_by_name` |
| preflight, create | konflik port Stackly | `Registry::instances(include_retained: false)` dan `endpoints_conflict` |
| preflight, create | port dipakai OS | `PortProbe::try_bind` (langkah 5 spec 0010) di alamat langkah 4 sampai 6 |
| preflight | `suggestedPort` | pencarian kandidat di `gather` (AC-5) |
| preflight, create | pemilik port Stackly di pesan | nama instance yang konflik dari registry; proses luar tidak pernah dinamai |
| preflight, create | filesystem | `Filesystem::volume_facts(root data)` (spec 0004) |
| preflight, create | ruang bebas | `Filesystem::available_space(instances/)` (spec 0018) |
| preflight, create | keadaan store | `SecretStore::state()` dan `backend()` |
| preflight, create | reference sumber | registry `secret_references` dan `SecretStore::probe` (interaksi `Never`) |
| create | bentuk nilai `value` | `secret::redact::value_shape` atas `SecretValue` hasil konversi `ipc::secret`, diperiksa `SecretFieldRule::check` di precheck |
| preflight | `command` | `equivalent_command` atas `CommandDraft` hasil pemetaan `CommandSpec` oleh `stacklyd`, label runtime dari runtime `installed` di `CreateFacts` |
| create | `InstanceId` | `InstanceId::generate()` oleh handler, diulang sampai short key bebas |
| create | reference secret | `local:instance/<id>/<purpose>`, purpose dari nama field (spec 0011) |
| create | nilai secret | `SecretInput` di params (`value`), `generate_password` (`generate`), store (`copyFrom`) |
| create | aturan field secret | `FieldContext.rule` dari `SecretFieldRule::from_setting` atas secret field schema (`minLength`, `maxLength`, `printableOnly`), lewat `PutTarget::New.field` (spec 0011) |
| create | kode kegagalan `put` | `secret::error::classify_create_failure` atas `SecretError::Store { reference, error }` dan reference target (spec 0011) |
| create | interaksi Keychain | `OperationTrigger::interaction()` dari handshake (spec 0007, 0011) |
| create | folder instance | `DirectoryLayout::instance_paths(id)` dan `instance_log_dir(id)` |
| create | ID yang dikenal untuk layout dan overlap | `Registry::known_instance_ids` |
| create | paket pinned | `pin_revision` (K-1) |
| create | `source_id` | `ensure_source` (K-2) |
| create | settings tersimpan | `SettingsSchema::validate` atas settings draft yang secret field nya diisi reference |
| create | host endpoint | `defaultHost` manifest |
| create | permintaan port | draft (`Fixed` atau `Auto`; kosong berarti `Auto`) |
| create | nama socket Unix tersimpan | render `socketName` dengan port `Fixed`, atau penanda `auto` |
| create | `startup` | konstanta `manual` |
| create | `data_line`, `data_version` | record runtime |
| create | `created_at` dan `createdAt` manifest | satu pembacaan `Clock::now()` di langkah `layout` |
| create | `data.initializedAt` | `Clock` saat `init_succeeded` (spec 0010) |
| create | `expected_generation` | `Versioned.generation` dari baca registry di `register` |
| start setelah create | idempotency key start | SHA-256 hex dari `<key create>:start` |
| summary | `socketPath` | template `socketName` paket pinned, port efektif endpoint rujukan, folder run |
| event `instance.removed` | revision | revision terakhir instance yang dihapus ditambah 1 |
| sheet | daftar runtime | `RuntimeStore` (state `installed`) |
| sheet | kategori runtime | `CatalogStore`, join `ProviderSummary.software` dengan `RuntimeSummary.software` |
| sheet | tahap progress | `OperationSummary.stage` dari `OperationCenter.updates(for:)` |
| sheet | instance yang dipilih setelah sukses | `OperationSummary.result.resources` (instance ID) |

### Key invariants

- Create tidak pernah menginisialisasi folder yang tidak dibuat operasi itu, dan tidak pernah menulis ke folder data yang tidak kosong (BR-008).
- Port `Fixed` tidak pernah diganti oleh daemon; hanya permintaan `Auto` yang menghasilkan port pilihan Stackly (BR-005).
- Preflight dan create memakai `gather` dan `evaluate` yang sama; selain angka port di luar `u16`, tidak ada aturan validasi yang hanya ada di GUI atau hanya di CLI.
- Selama operasi create belum selesai, setiap efeknya bisa dibalik dan dibalik dengan tepat (reference secret, folder instance, baris registry); folder paket pinned dan baris provider revision tidak pernah dihapus create, dan kompensasi tidak pernah menghapus apa pun yang bukan buatan operasi itu (BR-023).
- Nilai secret hanya ada di params `instance.create`, di memori daemon selama `put` atau pemeriksaan `bind`, dan di Keychain.
- Instance yang terdaftar selalu menyimpan provider revision dan runtime exact, bukan selector (BR-001).
- Semua path yang ditulis create diturunkan dari ID dan layout dan diperiksa `check_owned_path` sebelum IO.

### Security model

- Semua method lewat socket privat milik user yang sama (spec 0006); tidak ada method baru yang bisa dipanggil tanpa handshake.
- Nama instance dibatasi VAL-001 dan tidak pernah dipakai sebagai path atau argumen shell; path dari ID UUID v7 buatan daemon.
- Settings divalidasi schema, termasuk aturan `argument_injection_risk` spec 0009; argv init diperiksa `ensure_not_in_argv` (spec 0011) sebelum spawn.
- Endpoint hanya loopback (`defaultHost`); create tidak pernah mengubah firewall atau bind ke alamat lain (VAL-005).
- Secret: tidak ada nilai di preflight, journal, sidik jari request (`SecretText` disamarkan), log, event, perintah setara, atau pesan error; reference bukan secret dan boleh tampil.
- Pin dan layout menolak symlink dan entri asing tanpa menimpa atau menghapus (K-1, spec 0004); kompensasi baris registry dan pohon instance menolak menghapus selama lock instance dipegang (K10).
- Ukuran params dibatasi `Validate` dan `CONTROL_FRAME_MAX`; settings paling banyak 256 KiB (spec 0009); preflight hanya membaca, jadi pemanggilan berulang tidak mengubah state, dan `gather` berjalan di luar gerbang `begin` sehingga preflight yang lambat tidak menahan operasi lain.
- Log daemon untuk create memuat ID operasi, ID instance, nama, provider, runtime, dan tahap, tidak pernah nilai settings atau secret; preflight hanya dicatat di level debug dengan jumlah baris per status.

### Configuration required

Tidak ada variabel environment, setting, atau credential baru. Catalog memakai `STACKLY_CATALOG_DIR` spec 0017 untuk development dan test. Tidak ada dependency baru: `socket2` di `stackly-core` untuk `PortProbe::try_bind` sudah dipasang langkah 5 spec 0010 (spec 0001, K2).

### Fuzz target

`fuzz/fuzz_targets/instance_path.rs` (aturan nama spec 0006) memanggil `stackly_core::instance::fuzzing::owned_path(platform, prepared, fixtures, input)`. Target menyiapkan sekali, di folder sementara, `MacosPlatform::with_roots` dengan pohon fixture: symlink dari dalam root ke luar root, symlink ke root lain, loop symlink, symlink folder, nama Unicode dan berspasi, dan folder sedalam 64 tingkat. Input dipakai sebagai byte path (absolut atau relatif terhadap root data). Invariant:

- Tidak pernah panic.
- Path dengan komponen `.` atau `..`, atau path relatif, selalu `NotNormalized`.
- `Ok(owned)` berarti `owned.canonical` berada di dalam salah satu root canonical dan tidak sama dengan root itu.
- Path yang melewati symlink yang keluar dari root tidak pernah `Ok` dengan canonical di luar root.
- `check_data_root_overlap(candidate, fixtures data)` mengembalikan `DataRootOverlap` jika dan hanya jika identitas file candidate atau ancestor nya sama dengan salah satu folder data fixture atau sebaliknya.

Seed ada di `fuzz/seeds/instance_path/` dan dijalankan test `instance_path_seeds_hold_the_invariants` di `stackly-core` dengan `FakePlatform` (filesystem nyata di `TestRoot`).

### Critical test scenarios

- Happy path fake: `Catalog::for_tests` dengan `testing/fake`, `install_fake` runtime saja, `instance.get_create_form`, `instance.preflight` `ready`, `instance.create` dengan secret `authToken` bangkitan, operasi `succeeded`, instance `stopped`, manifest `initializedAt` dan `createdAt` sama dengan registry, folder pinned ada, baris `provider_sources` satu. Memverifikasi **AC-1**, **AC-2**, **AC-10**, **AC-12**, **AC-13**, **AC-14**.
- Tabel evaluasi (unit test murni `evaluate`): setiap baris tabel *Evaluator create*, setiap langkah *Pemeriksaan port* (termasuk `Denied` dan `Unavailable` di langkah 4 sampai 6), setiap baris *Field error*, permintaan endpoint tidak valid, dan kombinasi nama salah dan port dipakai sekaligus, dengan kode baris error pertama dan `fields` yang diharapkan, termasuk error create tanpa `fields` untuk kode yang tidak mendaftarkan `omitted` dan saat `secret.needs_secret` di depan (D7). Memverifikasi **AC-2**, **AC-3**, **AC-4**, **AC-6**, **AC-7**, **AC-9**, **AC-15**.
- Listener nyata (integration test `stackly-core` dengan `PortProbe` asli): setiap baris tabel *Hasil yang diharapkan*, termasuk sisa `TIME_WAIT` dan UDP. Memverifikasi **AC-4**, **AC-5**.
- Saran port: port `Fixed` milik instance `stopped` menghasilkan `supervisor.endpoint_conflict` dengan saran, saran tidak pernah dipakai create, dan setelah port diganti ke saran preflight `ready`; p 65535 dan p di bawah 1024 memberi kandidat yang benar. Memverifikasi **AC-5**.
- Data tidak kosong: ID dipaksa ke folder yatim (`already_exists`), dan file disisipkan setelah layout (`data_not_empty`). Memverifikasi **AC-8**.
- Secret: Keychain terkunci (`FakeSecretStore::lock`) saat `put` sumber `generate` menghasilkan `secret.store_locked`, dan saat `put` sumber `copyFrom` menghasilkan `secret.needs_secret` dengan satu item `store_locked` untuk reference sumber, keduanya sebelum folder dibuat dan tanpa dialog kedua; nilai `value` kosong atau ber NUL ditolak `request.invalid_params` di `secrets[<i>].input.content` sebelum precheck; `printableOnly` dengan nilai ber `\n` ditolak sinkron di precheck dengan `reason` `control_characters`; `bind` ke reference yang ada memanggil `put(Bind)` yang hanya membaca sekali, lalu pemakaian ditulis batch `register` tanpa reference baru; `bind` ke reference tanpa nilai adalah baris error; prefix fixture credential tidak ada di journal, log, event, dan preflight. Memverifikasi **AC-7**.
- Balapan: sepuluh create bersamaan dengan nama sama (satu instance, sisanya `resource.busy` atau `instance.name_taken`), dua create dengan port fixed sama, dua create provider tanpa `multi_instance`, dan `GenerationConflict` yang disuntik empat kali lalu berhasil. Memverifikasi **AC-10**, **AC-15**.
- Gagal, cancel, dan terputus: kegagalan disuntik di setiap langkah termasuk `init` (`failInit` fake) dan sebelum `init_begin` (store terkunci saat `resolve_for_start`): registry, folder, dan reference bersih, folder pinned tetap, event `instance.removed` terkirim kalau baris sempat terdaftar; cancel saat `init` berakhir `cancelled` tanpa sisa; daemon dimatikan saat `secrets`, `layout`, `register`, dan `init` lalu dikompensasi setelah start berikutnya; pembantu init yang masih memegang lock membuat operasi `needs_recovery` dengan baris registry tetap ada (`instance_registration` dan `instance_tree` menolak dengan `instance_locked`) dan abandon membersihkannya setelah lock bebas; probe yang error menjadi `lock_probe_failed`, file lock yang tidak ada melewati probe, dan init yang gagal biasa dikompensasi penuh tanpa `needs_recovery` karena lock daemon sudah dilepas. Memverifikasi **AC-11**.
- Pin: setiap kriteria K-1 dan K-2 sebagai test terpisah di `stackly-core` dan `stacklyd`. Memverifikasi **AC-13**, **AC-14**.
- Penggantian create sementara: setelah langkah 9 tidak ada lagi `// provisional: F20 until F20 step 9` di kode, `provider.revision` kosong memakai revision catalog, dan test langkah 13 dan 14 spec 0010, walking skeleton fitur 14, dan skenario init gagal spec 0019 AC-5 di `apps/daemon/tests/postgresql.rs` yang mengharapkan `needs_recovery` lalu abandon setelah init gagal diperbarui menjadi operasi `failed` yang dikompensasi penuh; test spec 0019 itu membuat `initdb` gagal lewat kait test setelah pemeriksaan isi folder data atau lewat jalur init fake, bukan file yang ditanam sebelum init, karena pemeriksaan isi di langkah `layout` menolak file itu lebih dulu dengan `instance.data_not_empty` (D9). Memverifikasi **AC-10**, **AC-11**, **AC-15**.
- Auto port dan race: create `Auto` lalu start sampai `ready` (`bound` dan `socketPath` untuk PostgreSQL fixture), dan create `Fixed` lalu port direbut listener test di alamat endpoint yang sama sebelum start. Memverifikasi **AC-12**, **AC-16**.
- Kontrak: fixture setiap DTO baru (DTO dan fixture `instance.removed` dari langkah 12 spec 0010 dipakai ulang), nilai asing setiap enum terbuka, hint statis setiap kode baru, dan fixture `create-form-*.json` yang dibangkitkan ulang sama dengan di repo. Memverifikasi **AC-17**.
- CLI: golden teks dan JSON untuk `--dry-run`, create, `--start`, `--wait`; konversi `--set`; tiga aturan `cli.usage`; resolusi `--runtime software@x` (versi, line, ambigu); setiap sumber `--secret`; `prompt` tanpa TTY exit 2; key start turunan; round trip `commands.json`. Memverifikasi **AC-18**.
- Paritas AC-028: matriks input buruk (nama `Bad_Name`, nama 64 karakter, port 0, port dipakai, port milik instance lain, setting integer `"abc"`, key asing, secret field di settings, secret field tidak dikenal, runtime tidak ada, runtime software lain, provider planned) lewat `Client` dan lewat CLI `--json`. Memverifikasi **AC-19**.
- GUI: test Swift `CreateInstanceModel` dengan service tiruan (debounce, generasi, Create aktif dan `disabledReason` untuk setiap kondisi di *Footer* dalam urutannya, port bukan angka, progress, gagal, cancel, sukses dengan dan tanpa start beserta `Feedback` yang dikirim ke `FeedbackCenter` tiruan, start lanjutan yang gagal, Close menggantikan `Cancel…` saat gate blocked di mode progress, dan pengumuman preflight hanya saat ringkasan berubah), `SchemaFormModel` dari tiga fixture dengan assertion NATS, `InstanceStore` menghapus baris pada `instance.removed`, dan evidence manual keyboard dan VoiceOver. Memverifikasi **AC-20**, **AC-21**, **AC-22**.
- Fuzz: 10 menit di Mac ini dan test seed. Memverifikasi **AC-23**.
- Kinerja: pengukuran di Mac ini dengan 10 instance fake terdaftar, juga saat dialog Keychain lain terbuka. Memverifikasi **AC-24**.
- PostgreSQL nyata: create 18 dan 17 lewat CLI dan GUI, konflik nyata, start sampai `ready`. Memverifikasi **AC-25**.
- Keamanan: nama `../x`, `a b`, `a;rm`, dan Unicode ditolak VAL-001; symlink di `providers/official/postgresql/` ditolak pin; symlink `instances/<id>` ditolak layout; grep nilai secret dan settings teks di log daemon kosong. Memverifikasi **AC-26**.

### Jawaban untuk follow up spec lain

| Spec dan baris Follow-up | Jawaban di spec ini |
| --- | --- |
| 0017: ambil *Kontrak untuk fitur 20* (K-1, K-2, update 0004 dan 0005), panggil `pin_revision`, putuskan revision, tombol New Instance di panel detail | AC-13, AC-14, *Pin dan sumber catalog*; revision catalog saat create (AC-15); tombol di *Sheet New Instance*; usulan 0004 dan 0005 di *Usulan update spec lain* |
| 0010: perluas `instance.create` secara additive (preflight, form dari schema, autostart) | dua method baca baru, precheck, langkah baru (*Operasi `create_instance`*), dan langkah 9 yang mengganti create sementara spec 0010; `provider.revision` opsional sudah dikirim langkah 12 spec 0010; autostart: Start after creating di client, Start at login ditunda ke fitur 31 |
| 0011: `SecretInput` per secret field, `put` dengan `PutTarget::New` sebelum `initdb`, gagal simpan `secret.needs_secret`, kompensasi reference | AC-7 (`classify_create_failure`), langkah `secrets`, AC-11 |
| 0011: `SecretField` di sheet dengan `generate` default; preflight membaca hasil `put` | *Form dari settings schema*; baris secret berubah setelah tahap `secrets` (*Sheet New Instance*) |
| 0005: `known_instance_ids` sebelum layout, `InsertInstance` di batch operasi, `endpoints_conflict`, `expected_generation`, ulang lima kali | langkah `layout` dan `register`, AC-10 |
| 0004: tulis ulang manifest dengan `data.initializedAt` setelah `init_succeeded` | dilakukan `execute_init` spec 0010 (AC-13 spec 0010); spec ini membuktikannya ujung ke ujung (AC-12) |
| 0004: guard evidence `VAL-001` di test ujung ke ujung | AC-3 |
| 0004: BR-002, BR-004, BR-005, BR-008, `create_instance_layout`, `resolve_port`, validasi endpoint `Instance::new` | AC-4 sampai AC-8, *Endpoint di aggregate*; `resolve_port` tetap dipanggil start (spec 0010) karena create tidak memilih port `Auto` |
| 0008: bukti AC-028 penuh | AC-18, AC-19 |
| 0009: `Endpoint` domain membawa `purpose` atau dibaca dari paket | dibaca dari paket pinned (immutable); domain dan registry tidak berubah; fitur 21 memakai cara yang sama untuk connection string |
| 0012: DTO capability lewat spec 0013 dan fitur 20 | `CapabilityStatus` spec 0017 dipakai di `CreateForm`; untuk aksi pada instance yang sudah ada, fitur 21 atau 22 menambah capability dari provider revision instance ke `InstanceSummary` saat membutuhkannya |
| 0019: preflight folder data kosong, `superuserPassword` dan `printableOnly`, `pin_package` | AC-6, AC-8; `printableOnly` di `FormField`, precheck, dan `PutTarget::New.field`; `pin_package` tetap fixture test, create memakai `pin_revision` |
| 0018: create menerima `RuntimeId`; satu satunya pemanggil `pin_revision` | AC-1, AC-13 |

### Usulan update spec lain

Ditulis di sini karena spec ini tidak boleh mengubah spec lain; lead membawa daftar ini ke program foundation supaya perubahan masuk sebelum atau di dalam build spec pemiliknya. Satu blok per spec target, dan setiap blok berdiri sendiri. Teks lama dikutip persis dari spec target di `main` (2026-10-20). "Ganti" berarti teks baru menggantikan teks lama di tempat yang sama; "Sisipkan sesudah" berarti teks lama tetap dan teks baru ditambahkan tepat sesudahnya.

#### Spec 0001 (stack dan arsitektur)

Diterapkan di spec 0001 pada 2026-10-20 (sesi `upd-0001-0012`), persis seperti teks baru di bawah.

1. *Peta crate*, baris `stackly-core`. Ganti:

   ```text
   `getrandom` (spec 0011);
   ```

   dengan:

   ```text
   `getrandom` (spec 0011); `socket2` untuk bind percobaan `PortProbe` tanpa `listen` dan dengan opsi socket yang dipilih pemanggil (spec 0020);
   ```

#### Spec 0004 (domain core dan abstraksi platform)

Diterapkan di spec 0004 pada 2026-10-20 (sesi `upd-0004-0005`). Teks baru butir 2 dan 3 di bawah sudah disamakan dengan teks yang diterapkan: `DuplicateSocket` masuk BR-004 (bukan rule None), dan `Instance::new` mengembalikan `Result<Instance, InvariantError>`.

1. *Model domain*, tabel ID. Sisipkan sesudah:

   ```text
   | `ConfigRevisionId`, `SecretReferenceId`, `AuditEventId` | UUID v7, makro yang sama | ID baris `config_revisions`, `secret_references`, dan `audit_events` *(ditambah fitur 10 dan 11, spec 0005)* |
   ```

   baris:

   ```text
   | `ProviderSourceId` | UUID v7, makro yang sama | ID baris `provider_sources` *(ditambah fitur 20, spec 0020 dan K-2 spec 0017)* |
   ```

2. *Model domain*, tipe pendukung (butir "Belum dibangun di fitur 4"). Ganti:

   ```text
   validasi endpoint di `Instance::new` dan setter `display_name`, `startup`, serta port yang di bind (fitur 20 dan 10);
   ```

   dengan:

   ```text
   setter `display_name` dan `startup` (fitur 29 dan 31) serta port yang di bind (fitur 10); *(ditambah fitur 20, spec 0020:* `Instance::new` *mengembalikan* `Result<Instance, InvariantError>` *dan menolak nama endpoint ganda (detail* `DuplicateEndpoint`*, rule None), nama socket Unix ganda (*`DuplicateSocket`*, BR-004, karena dua endpoint Unix dengan nama socket sama konflik menurut baris BR-004), `bound` yang terisi (*`BoundOnNew`*, rule None; endpoint yang sudah di bind dibangun lewat* `Instance::restore` *atau* `set_bound_port`*), dan dua endpoint IP instance itu sendiri yang konflik menurut* `endpoints_conflict` *atas port* `Fixed` *(*`EndpointSelfConflict`*, BR-004));*
   ```

3. *Model domain*, butir error "Pemetaan detail ke aturan". Ganti:

   ```text
   `FixedPortConflict` dan `AutoPortUnavailable` ke BR-005;
   ```

   dengan:

   ```text
   `FixedPortConflict` dan `AutoPortUnavailable` ke BR-005; `EndpointSelfConflict` dan `DuplicateSocket` ke BR-004 (spec 0020);
   ```

4. *Port platform*, trait `Filesystem`. Sisipkan sesudah:

   ```text
       fn remove_tree(&self, root: &Path, relative: &Path) -> Result<RemoveReport, PlatformError>;
   ```

   baris:

   ```text
       // ditambah fitur 20 (spec 0020):
       fn volume_facts(&self, path: &Path) -> Result<VolumeFacts, PlatformError>;
   ```

   dan sesudah butir `remove_tree` di bawah blok kode, butir baru:

   ```text
   - `volume_facts(path)` *(ditambah fitur 20, spec 0020)* mengembalikan `VolumeFacts { fs_type: String, local: bool }` untuk volume yang memuat `path`. Adapter macOS memakai `statfs` (`f_fstypename` dan flag `MNT_LOCAL`); `FakeFilesystem` bisa diatur dan default nya `apfs` lokal; `fs_conformance` memeriksa bahwa root test terbaca sebagai volume lokal.
   ```

#### Spec 0005 (data model registry)

Diterapkan di spec 0005 pada 2026-10-20 (sesi `upd-0004-0005`). Teks baru butir 3, 5, dan 6 di bawah sudah disamakan dengan teks yang diterapkan: tipe `ProviderSourceKind` dan `ProviderSourceStatus`, builder `with_source_id`, enum `NotDeletableReason`, dan kunci `details` `registry.not_deletable` yang hanya `reason` (instance nya di `ErrorBody.resource`).

1. *Port registry*, trait `Registry`. Sisipkan sesudah:

   ```text
       fn known_instance_ids(&self) -> Result<Versioned<Vec<InstanceId>>, RegistryError>;
   ```

   baris:

   ```text
       fn provider_source_by_location(&self, kind: &str, location: &str) -> Result<Versioned<Option<ProviderSourceRecord>>, RegistryError>;  // spec 0020
   ```

2. *Port registry*, tabel `WriteOp`. Sisipkan sesudah:

   ```text
   | `UpdateOperation { operation, expected }` | baris operasi | |
   ```

   baris:

   ```text
   | `InsertProviderSource(ProviderSourceRecord)` | satu baris `provider_sources` | revision 1 (selain itu `InvalidRevision`); `DuplicateResource` (`ResourceRef::ProviderSource`) kalau ID atau (`kind`, `location`) sudah ada; satu entri `Committed.revisions` (spec 0020) |
   | `DeleteInstanceCreatedBy { id, expected, operation }` | menghapus baris `instances` (endpoint, path, dan config revision ikut cascade) dan semua `secret_reference_uses` instance itu | instance ada (`NotFound`); revision (`RevisionConflict`); `created_by_operation` sama dengan `operation` dan lifecycle bukan `starting`, `running`, `stopping`, `recovering`, atau `unknown`, selain itu `NotDeletable { resource, reason }` dengan reason `created_by_other_operation` atau `active`; tanpa entri `Committed.revisions` (spec 0020) |
   ```

3. *Port registry*, record. Ganti:

   ```text
   `ProviderRevisionRecord { revision: ProviderRevision, schema_version: Option<String>, publisher: Option<String>, approval: Option<String> }`
   ```

   dengan:

   ```text
   `ProviderRevisionRecord { revision: ProviderRevision, schema_version: Option<String>, publisher: Option<String>, approval: Option<String>, source_id: Option<ProviderSourceId> }` (`source_id` dicek `MissingReference`, spec 0020), `ProviderSourceRecord { id: ProviderSourceId, kind: ProviderSourceKind, location: String, trust_root: Option<String>, last_sync_at: Option<Timestamp>, status: ProviderSourceStatus, revision: ResourceRevision, created_at: Timestamp, updated_at: Timestamp }` (spec 0020; `ProviderSourceKind` dan `ProviderSourceStatus` adalah kode set terbuka seperti `ConfigRevisionStatus`, dengan nilai awal dari K-2 spec 0017: kind `bundled` dan `development`, status `active`; fitur 37 menambah `local` dan status lain),
   ```

   dan di kalimat sesudahnya ganti `` (`source_id`, `approved_at`, `` dengan `` (`approved_at`, ``; `source_id` masuk lewat method builder additive `with_source_id`, karena `ProviderRevisionRecord::new` tidak berubah arity.

4. *Port registry*, `ResourceRef`. Ganti:

   ```text
   `ProviderRevision(ProviderRevisionKey)`, `Artifact(Sha256Digest)`.
   ```

   dengan:

   ```text
   `ProviderRevision(ProviderRevisionKey)`, `Artifact(Sha256Digest)`, `ProviderSource(ProviderSourceId)` (spec 0020).
   ```

5. *Port registry*, `RegistryError`. Ganti:

   ```text
   `NoSpace`, `Busy`, `Unavailable`, `Io { detail }`.
   ```

   dengan:

   ```text
   `NoSpace`, `Busy`, `Unavailable`, `Io { detail }`, `NotDeletable { resource: ResourceRef, reason: NotDeletableReason }` (spec 0020; `NotDeletableReason`: `CreatedByOtherOperation` atau `Active`, kode `created_by_other_operation` dan `active`).
   ```

6. *Kode error di wire*. Sisipkan sesudah:

   ```text
   | `NameTaken` | `registry.name_taken` | `conflict` | `name`, `existing` |
   ```

   baris:

   ```text
   | `NotDeletable` (fitur 20) | `registry.not_deletable` | `conflict` | `reason` (`created_by_other_operation` atau `active`); instance nya dikirim di `ErrorBody.resource` (`ResourceKind` `instance`), sesuai aturan di atas, bukan sebagai kunci `details` |
   ```

#### Spec 0007 (operation engine)

Diterapkan di spec 0007 pada 2026-10-20 (sesi `arc-f7-update`, keputusan 29 dan 30 spec 0007) seperti teks baru di bawah, dengan satu beda: kalimat AC-8 di butir 3 menjadi **AC-22** baru di spec 0007, karena AC-8 sudah dicentang di verify fitur 7; kodenya tetap dibangun langkah 3 milestone 1. Baris `BusyPolicy` spec 0007 (Queue untuk start dan reload pengguna, Reject untuk create) dari Follow-up spec 0010 (K1) ikut diterapkan di update yang sama. Butir 4 (pengecualian `ctx.commit` untuk `InsertProviderRevision`) ditambah perbaikan pasca penyelarasan 2026-10-20.

1. *Tabel resume per jenis operasi*. Ganti baris:

   ```text
   | `create_instance`, `restore_instance`, `migrate_data`, `purge_instance`, `remove_instance`, `remove_runtime`, `register_external_runtime`, `rename_instance`, `rebind_runtime`, `update_provider_revision`, `update_manager` | tidak | terputus sebelum titik tak bisa dibalik berarti kompensasi otomatis; sesudahnya `needs_recovery` (fitur 20, 51, 54, 55, 56, 58, 59, 61) |
   ```

   dengan dua baris:

   ```text
   | `create_instance` | tidak | sejak langkah 9 spec 0020 semua langkahnya `reversible`, jadi gagal, cancel, dan terputus selalu dikompensasi dan berakhir `failed` atau `cancelled`, dan `needs_recovery` hanya kalau kompensasi gagal; create sementara spec 0010 sebelum itu punya titik tak bisa dibalik di `register`, jadi terputus sesudahnya berarti `needs_recovery` (K1) |
   | `restore_instance`, `migrate_data`, `purge_instance`, `remove_instance`, `remove_runtime`, `register_external_runtime`, `rename_instance`, `rebind_runtime`, `update_provider_revision`, `update_manager` | tidak | terputus sebelum titik tak bisa dibalik berarti kompensasi otomatis; sesudahnya `needs_recovery` (fitur 51, 54, 55, 56, 58, 59, 61) |
   ```

2. *API engine*, aturan "Catatan dibuat". Ganti:

   ```text
   `confirm_created` menandai `created`; `not_created(item, reason)` menandai `not_owned` dengan `detail` `already_exists` atau `short_key_taken` saat pembuatan gagal karena sudah ada.
   ```

   dengan:

   ```text
   `confirm_created` menandai `created`; `not_created(item, reason)` menandai `not_owned` dengan `detail` `already_exists` atau `short_key_taken` saat pembuatan gagal karena sudah ada, atau `data_not_empty` saat pembuatan berhasil tetapi isinya tidak terbukti milik operasi (spec 0020). `not_created` dipanggil untuk catatan yang masih `intended`, sebelum `confirm_created`.
   ```

3. *Kompensasi dan primitive hapus*, kompensator `instance_tree`. Ganti:

   ```text
   Kalau `instance-manifest.json` terbaca dan `createdByOperation` bukan operasi ini, seluruh item `not_owned` (alasan `created_by_other_operation`). Folder run dilewati kalau `known_instance_ids` memuat instance lain dengan short key yang sama. Setiap path lewat `remove_owned_tree`.
   ```

   dengan:

   ```text
   Urutannya tetap (K10, spec 0020): (1) pemeriksaan manifest: `instance-manifest.json` milik operasi lain membuat seluruh item `not_owned` (alasan `created_by_other_operation`), dan manifest yang tidak terbaca membuat item `compensation_failed` dengan detail `manifest_unreadable`; (2) `Filesystem::symlink_metadata(InstancePaths.lock)`: tidak ada berarti bebas, dan probe dilewati; (3) kalau ada, `processes().probe_lock(lock)` (spec 0010): `Free` lanjut, `Held` atau state lain yang bukan `Free` membuat item `compensation_failed` dengan detail `instance_locked`, dan `ProcessError` apa pun (`NotSupported`, `LockFileInvalid`, `Io`) membuat item `compensation_failed` dengan detail `lock_probe_failed`, keduanya tanpa menghapus apa pun; (4) folder run dilewati kalau `known_instance_ids` memuat instance lain dengan short key yang sama, dan setiap path lewat `remove_owned_tree`.
   ```

   dan di AC-8 tambahkan satu kalimat: kompensasi `instance_tree` tidak menghapus apa pun saat lock instance `Held` atau probe nya error, dan melewati probe saat file lock tidak ada, dengan test core untuk ketiganya di `FakePlatform` (K10).

4. *API engine*, butir *Commit registry terakhir*. Ganti:

   ```text
   `ctx.commit` di tengah operasi hanya untuk keadaan yang memang harus terlihat sebelum selesai, dan apa pun yang ia sisipkan wajib punya kompensator kustom.
   ```

   dengan:

   ```text
   `ctx.commit` di tengah operasi hanya untuk keadaan yang memang harus terlihat sebelum selesai, dan apa pun yang ia sisipkan wajib punya kompensator kustom, kecuali baris bersama yang immutable dan idempoten (`InsertProviderRevision`), yang sengaja tidak dikompensasi (spec 0020).
   ```

#### Spec 0009 (provider engine dan manifest schema)

Diterapkan di spec 0009 pada 2026-10-20 (sesi `upd-0009`), persis seperti teks baru di bawah.

1. *Settings schema*, paragraf pembuka. Ganti:

   ```text
   `schemas/settings.schema.json` adalah dialek terbatas JSON Schema 2020-12 yang diparse serde menjadi model field bertipe (`SettingsSchema`). Model ini yang dirender form fitur 20 dan 29;
   ```

   dengan:

   ```text
   `schemas/settings.schema.json` adalah dialek terbatas JSON Schema 2020-12 yang diparse serde menjadi model field bertipe (`SettingsSchema`). Model ini menyimpan urutan deklarasi `properties` di root dan di item array (decode lewat visitor map yang mengikuti urutan dokumen, tanpa fitur `preserve_order` serde_json), dan `fields()` serta field item dikembalikan dalam urutan itu (spec 0020). Setiap issue `SettingsError` membawa `SettingsIssueKind` tetap (`unknown`, `type`, `managed_filled`, `control_characters`, `duplicate_key`, `pattern`, `too_short`, `too_long`, `out_of_range`, `enum`, `too_many_items`, `too_few_items`, `required`) di samping kode issue yang sudah ada (spec 0020). Model ini yang dirender form fitur 20 dan 29;
   ```

#### Spec 0010 (process supervisor)

Diterapkan di spec 0010 pada 2026-10-20 (sesi `upd-0010`, merge `989e816`), dengan keputusan K yang mengubah teksnya; teks spec 0010 yang berlaku. Butir 3 menjadi paragraf *Create sementara* (K1, `// provisional: F20 until F20 step 9`, normatif untuk langkah 13 fitur 10, dengan `register` sebagai langkah pertama yang `reversible: false`) yang diikuti paragraf *Target fitur 20* berisi teks baru butir 3, dan kolom error butir 2 memuat daftar keduanya. Butir 4 ditambah aturan `Denied` dan `Unavailable` dengan `supervisor.endpoint_unavailable` (K2). Butir 6 menyebut bahwa DTO `instance.removed` ikut langkah 12 tetapi tidak dikirim sebelum fitur 20 (K4). Butir 7 menandai sembilan kode dengan *(fitur 20, spec 0020)* dan memperluas baris `instance.endpoint_invalid` (K4).

1. *Permukaan IPC*, baris `instance.create`, kolom input (di dalam code span yang sama). Ganti:

   ```text
   provider: { name, revision }
   ```

   dengan:

   ```text
   provider: { name, revision? }
   ```

   Arti revision yang kosong ada di paragraf create pengganti (butir 3).

2. *Permukaan IPC*, baris `instance.create`, kolom error sinkron. Ganti:

   ```text
   `instance.name_taken`, `instance.provider_not_found`, `instance.provider_ambiguous`, `instance.runtime_not_found`, `instance.runtime_mismatch`, `instance.settings_invalid` (`fields[]`), `instance.endpoint_invalid`
   ```

   dengan:

   ```text
   kode baris error pertama `instance::create::evaluate` dengan semua `fields[]` (daftar kode di *Kode error* spec 0020, termasuk `instance.name_invalid` untuk nama yang tidak valid), atau `resource.busy` dari lock nama
   ```

3. *Permukaan IPC*, paragraf yang diawali `Create minimal: validasi di method` dan diakhiri `Fitur 20 menambah preflight, form, dan opsi lain secara additive.`. Ganti seluruh paragraf dengan:

   ```text
   Create: `provider.revision` yang kosong berarti revision catalog yang dimuat, yang di pin langkah `pin`. Handler menjalankan `instance::create::gather` di `spawn_blocking`, lalu `begin` dengan precheck `evaluate` (spec 0020), `creates_resource: true`, lock tambahan `InstanceName(name)`, dan `BusyPolicy::Reject`. Operasi `create_instance` menjalankan langkah `validate`, `pin`, `secrets`, `layout`, `register`, dan `init` persis seperti *Operasi `create_instance`* spec 0020; semua langkah `reversible`, jadi kegagalan di mana pun, termasuk `init`, dikompensasi penuh (baris registry lewat kompensator `instance_registration` dan event `instance.removed`, pohon instance, reference secret) dan operasi berakhir `failed` dengan kode langkah itu.
   ```

4. *Startup transaction*, baris `endpoints`. Ganti:

   ```text
   (bind percobaan di alamat endpoint, `tcp` dengan listener dan `udp` dengan socket UDP, IPv4 atau IPv6 sesuai alamat, tanpa `SO_REUSEADDR`)
   ```

   dengan:

   ```text
   (`PortProbe::try_bind(transport, alamat endpoint, BindOptions { reuse_addr: true, v6only: alamat IPv6 })`: socket `socket2` dengan opsi itu, `bind` tanpa `listen`, langsung ditutup, hasil `Free`, `InUse`, `Denied`, atau `Unavailable`; `SO_REUSEADDR` dipasang seperti server supaya sisa `TIME_WAIT` dan listener wildcard milik proses lain tidak dihitung dipakai, spec 0020)
   ```

5. *Status dan event*, `InstanceSummary` baris `endpoints`. Ganti:

   ```text
   `socketPath` = folder run ditambah `socketName`
   ```

   dengan:

   ```text
   `socketPath` = folder run ditambah `socketName` paket pinned yang dirender dengan port efektif endpoint rujukannya, dihitung saat dibaca dan di cache per provider revision key, atau null kalau endpoint rujukan belum punya port efektif (nama socket tersimpan hanya pengisi kolom; untuk endpoint rujukan `Auto` ia memuat penanda `auto`, spec 0020)
   ```

6. *Status dan event*. Sisipkan sebelum butir:

   ```text
   - `daemon.reconciling { phase: started | finished, instances, adopted, unknown, restarting }` lewat `publish_daemon`.
   ```

   butir:

   ```text
   - `instance.removed { instanceId }` lewat `publish_resource(Instance(id), revision terakhir + 1)` setelah baris instance dihapus (kompensasi create spec 0020; purge fitur 54 memakainya ulang); client membuang instance itu dari daftar.
   ```

7. *Kode error*. Sisipkan sesudah:

   ```text
   | `instance.endpoint_invalid` | `invalid_input` | endpoint tidak ada di manifest, port di luar 1 sampai 65535, atau permintaan untuk endpoint `unix` | `endpoint` |
   ```

   baris `instance.name_invalid`, `instance.provider_unavailable`, `instance.capability_unsupported`, `instance.runtime_incompatible`, `instance.storage_unsupported`, `instance.insufficient_space`, `instance.data_not_empty`, `instance.data_overlap`, dan `instance.short_key_taken` dengan kategori, kapan, dan kunci `details` persis seperti tabel *Kode error* spec 0020.

8. *Asumsi lintas spec*, butir Spec 0007. Ganti:

   ```text
   `BusyPolicy::Queue` untuk start, create, dan reload pengguna
   ```

   dengan:

   ```text
   `BusyPolicy::Queue` untuk start dan reload pengguna, `BusyPolicy::Reject` untuk create (lock nama, spec 0020)
   ```

#### Spec 0011 (secret store dan redaction)

Diterapkan di spec 0011 pada 2026-10-20 (sesi `upd-0011`, `4827440`), dengan keputusan K13 sampai K15 yang mengubah teksnya; teks spec 0011 yang berlaku. `PutTarget::New.field` bertipe `Option<FieldContext>` dengan `FieldContext { name, rule: SecretFieldRule }` dari `SecretFieldRule::from_setting`, bukan `SecretFieldRules`. `Bind` lewat `put` dengan field yang hanya membaca dan memeriksa (AC-23). Kegagalan `put` dipetakan `classify_create_failure`: item `needs_secret` hanya untuk reference sumber, dan tulisan target yang gagal menjadi `secret.store_*`. `Validate` setiap params yang membawa `SecretInput` memanggil `SecretInput::field_errors` (AC-7).

1. *Use case isi atau ganti nilai*, butir `target`. Ganti:

   ```text
   `PutTarget::New { reference, owner, purpose }` (untuk create instance, env up, dan restore:
   ```

   dengan:

   ```text
   `PutTarget::New { reference, owner, purpose, field: Option<SecretFieldRules> }` dengan `SecretFieldRules { min_length, max_length, printable_only }` dari schema secret field (untuk create instance, env up, dan restore; `field` terisi untuk create dan restore, spec 0020:
   ```

2. *Use case isi atau ganti nilai*, butir create instance. Ganti:

   ```text
   - Create instance memanggil `put` untuk setiap secret field dengan `PutTarget::New` dan `Generate`, kecuali pemanggil mengirim `SecretInput` lain untuk field itu.
   ```

   dengan:

   ```text
   - Create instance memanggil `put` untuk setiap secret field dengan `PutTarget::New` dan `Generate`, kecuali pemanggil mengirim `SecretInput` lain untuk field itu. Untuk `Bind`, create tidak memanggil `put`: ia membaca nilai reference itu sekali untuk memeriksa `SecretFieldRules`, dan pemakaiannya ditulis `ReplaceSecretUses` di batch pendaftaran. Kegagalan store saat `put` di create (`Locked`, `AccessDenied`, `Busy`) dipetakan ke `secret.needs_secret` (spec 0020).
   ```

Tidak ada update untuk spec 0006 (kode baru masuk area pemiliknya lewat aturan pemakai pertama, dan hint memakai `codes::<area>::hint` yang sudah ada) dan spec 0008 (kode `cli` baru lewat aturan yang sama; `CLI_SCHEMA_VERSION` tetap karena hasil baru hanya menambah key).

### Selisih mockup yang disengaja

Untuk dicatat lead di `ui-macos.md`:

- Nama mengikuti VAL-001 (1 sampai 63 karakter), bukan pola mockup 3 sampai 31 karakter.
- Port diterima 1 sampai 65535 (VAL-004); saran hanya mulai 1024.
- Sheet dibuka dengan `defaultPort` manifest dan menolak konflik dengan saran, bukan mengisi port bebas berikutnya seperti mockup saat dibuka dari Catalog.
- Automatic menampilkan "Assigned when the instance starts", bukan angka port yang ditebak.
- Toggle Start at login tidak tampil sampai fitur 31.
- Sheet tetap terbuka dengan progress selama operasi create, bukan langsung tertutup, dan Cancel di mode itu membatalkan create.
- Baris preflight `provider` dan `settings` muncul saat gagal; baris secret per secret field dengan judul field; listener wildcard dan `::1` tampil sebagai peringatan, bukan error.
- Tombol Copy menyalin perintah lengkap termasuk `--port` dan `--secret`, bukan versi pendek mockup.

## Build plan

Urutan mengikuti pendekatan Journey proyek. Jalur yang dibuat lengkap adalah "pengguna membuat instance dengan preflight lalu instance itu bisa di start", lebih dulu di core di atas fake dengan semua jalur gagalnya, lalu lewat daemon dan CLI, lalu di GUI, lalu dibuktikan dengan PostgreSQL nyata di Mac ini.

**Milestone 1: Evaluasi, pin, dan operasi create di core** (update spec 0007 dari *Usulan update spec lain* dengan aturan `instance_tree` K10 sudah diterapkan di spec 0007 pada 2026-10-20, keputusan 29 dan 30; update spec 0001, 0004, 0005, 0009, 0010, dan 0011 sudah masuk pada 2026-10-20; fitur 5 dan 7 milestone 3 sudah ada; langkah 16 fitur 7 (migrasi `0004_operation_failure.sql` dan `failure_items`, K11) sebelum langkah 2 dan sebelum langkah 7; fitur 9 build; fitur 10 langkah 2a dan milestone 2 (`PortProbe::try_bind` di langkah 5, dan `execute_init` di langkah 7 yang melepas `InstanceLock` sebelum mengembalikan error); fitur 11 langkah 4 dan 4a (`FieldContext`, `put(Bind)`, `SecretError::Store`, `classify_create_failure`) sebelum langkah 3; fitur 17 milestone 1 (`core::catalog`); fitur 18 langkah 2 (`available_space`))

1. Domain dan platform: `ProviderSourceId`, validasi endpoint di `Instance::new` dengan detail `InvariantDetail` baru, dan `Filesystem::volume_facts` di `MacosFilesystem`, `FakeFilesystem`, dan `fs_conformance`. `Instance::new` yang fallible adalah preferensi spec 0010, bukan syarat: kalau langkah 13 spec 0010 sudah dibangun dengan `Instance::new` lama, langkah ini juga memperbaiki satu titik panggilnya di `stacklyd`; kalau langkah ini ditarik lebih dulu dari langkah 13 spec 0010, ia berjalan setelah langkah 2a spec 0010 di merge, karena keduanya menyentuh `crates/storage/tests/registry/common.rs` (K1). Memenuhi **AC-6**, **AC-12**.
2. Registry: `ProviderSourceRecord`, `InsertProviderSource`, `provider_source_by_location`, `ProviderRevisionRecord.source_id`, `ResourceRef::ProviderSource`, dan `DeleteInstanceCreatedBy` dengan `NotDeletable` dan kode `registry.not_deletable`, di `FakeRegistry`, `registry_conformance`, dan `SqliteRegistry`. Langkah ini berjalan setelah langkah 2a spec 0010 dan langkah 16 fitur 7 (migrasi 0004), dan satu per satu dengan langkah 6 spec 0018 (migrasi 0005) dalam urutan merge (spec 0005, K11). Memenuhi **AC-11**, **AC-14**.
3. Engine: detail `data_not_empty` dan `not_created` untuk catatan `intended`, dan aturan `instance_tree` K10 (manifest yang tidak terbaca menjadi `manifest_unreadable`, keberadaan file lock lewat `symlink_metadata`, `probe_lock`, `instance_locked`, dan `lock_probe_failed`), dengan test `ScriptedWork` dan `FakeProcessSupervision` untuk lock `Held`, error probe, dan file lock yang tidak ada. Karena keberadaan file lock diperiksa lebih dulu, langkah ini tidak menunggu langkah 3 spec 0010. Memenuhi **AC-8**, **AC-11**.
4. `SettingsSchema` dengan urutan deklarasi dan `SettingsIssueKind`. Memenuhi **AC-1**, **AC-9**, **AC-21**.
5. `core::catalog::pin` dan `core::catalog::source` (K-1, K-2) dengan semua test kriteria. Memenuhi **AC-13**, **AC-14**.
6. `core::instance::create`: `CreateDraft`, `CreateDeps`, `PortProbe::try_bind` dari langkah 5 spec 0010 (dipakai ulang, K2), `secret::redact::value_shape`, `gather` dengan saran port, `evaluate` dengan tabel test, `build_form` dengan `CreateFormModel`, `CommandSpec`, pemetaan *Field error* dan `PlanError`, nama saran, dan `instance::fuzzing::owned_path` dengan test seed. Memenuhi **AC-2** sampai **AC-7**, **AC-9**, **AC-15**, **AC-23** (harness).
7. `CreateInstanceWork`, factory, kompensator `instance_registration` dengan probe lock (dan kait event yang dipasang `stacklyd`), pemetaan kegagalan `put` lewat `classify_create_failure`, `StageName::PIN` beserta pembaruan test `startup_transaction_stages_follow_the_table_order` (`crates/domain/src/supervisor.rs`) di commit yang sama (K1), sumber ID yang bisa disuntik di fitur `test-support`, dan dua kait test: setelah layout dan sebelum pemeriksaan isi (AC-8), serta setelah pemeriksaan isi dan sebelum `init` (test init gagal spec 0019, D9), dengan test core memakai `FakePlatform`, `FakeRegistry`, `FakeSecretStore`, dan `FakeProcessSupervision`. Memenuhi **AC-7**, **AC-8**, **AC-10**, **AC-11**, **AC-12**.

Sesi paralel: setelah langkah 1 masuk, langkah 2 (registry), 3 (engine), 4 (settings), dan 5 (pin, setelah langkah 2) bisa menjadi sesi terpisah karena menyentuh crate dan modul berbeda; langkah 2 tidak berjalan bersamaan dengan sesi lain yang menulis `WriteOp` atau migrasi registry. Langkah 6 dan 7 satu sesi setelah semuanya.

**Milestone 2: Kontrak, daemon, dan CLI** (after milestone 1; needs fitur 6, fitur 8 build (kerangka CLI), fitur 10 milestone 4 (method instance, `InstanceSummary`, kontrak langkah 12, dan create sementara langkah 13 yang diganti langkah 9), fitur 11 langkah 8 (`secret_input`), dan fitur 14 (`stackly instance create` dasar) di `01-foundation.md`; fitur 17 langkah 6 (catalog di daemon), fitur 18 milestone 3 (`runtime.list`, `RuntimeSummary`), dan fitur 19 milestone 1 (paket PostgreSQL) untuk fixture PostgreSQL)

8. Protokol: `SignedSafeInteger`, DTO, enum terbuka, dua method di tabel `methods!` (`recovery refused`), sembilan kode `instance` dari tabel spec 0010 dan `catalog.pin_failed` dengan hint statis (`provider.revision` opsional dan DTO `instance.removed` sudah ada dari langkah 12 spec 0010), DTO `CommandDraft` dan `equivalent_command` di `instance::command` (D8), fixture termasuk `create-form-fake.json`, `create-form-nats.json` (dari paket NATS yang sudah ada), `create-form-postgresql.json`, dan `commands.json`, kasus fixture tampilan untuk preflight dengan konflik port (*Kasus yang menunggu DTO* spec 0068; di commit ini kalau `Fixtures/` spec 0068 sudah di `main`, selain itu milestone 1 spec 0068 yang menambahkannya, spec 0068 *Key invariants*; ditambah 2026-10-21), kenaikan minor (`CURRENT` di main ditambah 1 saat merge, K4), `cargo xtask codegen`, dan test kontrak Swift. Memenuhi **AC-17**.
9. `stacklyd`: `ensure_source` di slot start dan bagian `degraded` subsystem `catalog`, handler `instance.get_create_form`, `instance.preflight`, dan `instance.create` (gather di `spawn_blocking`, precheck, `BusyPolicy::Reject`) yang mengganti handler create sementara spec 0010 dan menghapus penanda `// provisional: F20 until F20 step 9` serta penolakan `provider.revision` kosong, pembaruan test langkah 13 dan 14 spec 0010, walking skeleton fitur 14, dan skenario init gagal spec 0019 AC-5 di `apps/daemon/tests/postgresql.rs` yang mengharapkan `needs_recovery` setelah init gagal, sehingga semuanya mengharapkan operasi `failed` yang dikompensasi penuh (D9, lihat *Critical test scenarios*), pemetaan `CreateFormModel` ke `CreateForm`, `CommandSpec` ke `CommandDraft`, serta `Check` dan `FieldIssue` ke `PreflightCheck` dan `FieldError` (D8), pemasangan `fields` dari slot `Evaluation` saat precheck menolak (D7), konversi `SecretText` ke `SecretValue` di `ipc::secret`, pendaftaran kompensator dengan event, `socketPath` saat dibaca di pemetaan `InstanceSummary`, pemetaan DTO dan error, dan integration test daemon untuk setiap skenario yang butuh daemon (termasuk auto port, race dengan start, dan skenario keamanan); test daemon yang memakai PostgreSQL sebelum langkah 9 spec 0019 memakai catalog `CatalogBuilder` yang memuat `official/postgresql` (D6). Memenuhi **AC-1**, **AC-2**, **AC-10** sampai **AC-14**, **AC-16**, **AC-24**, **AC-26** (test; review fresh model di kotak Review it).
10. CLI: `stackly instance create` lengkap dengan konversi `--set`, tiga aturan `cli.usage`, resolusi `--runtime`, sumber `--secret`, `--dry-run`, `--start` dengan key turunan, golden, test round trip perintah setara, dan matriks paritas AC-028. Memenuhi **AC-18**, **AC-19**.
11. Aturan `check-deps` tanpa identitas provider di kode create Rust dan Swift, dengan fixture yang melanggar. Memenuhi **AC-21**.

Sesi paralel: setelah langkah 8 masuk, langkah 9 dan 10 bisa menjadi dua sesi (daemon dan CLI menyentuh folder berbeda; matriks paritas di langkah 10 menunggu langkah 9 di merge), dan langkah 11 sesi kecil terpisah.

**Milestone 3: Sheet New Instance** (after langkah 8; needs fitur 12 dan fitur 13 milestone 4 (`ui-macos.md`), termasuk langkah 16 fitur 13 (`InstanceStore`, `ProvisionalInstanceList`) yang menurut K18 menunggu milestone 2 fitur 10, dan update spec 0013 untuk K16 dan K17; fitur 11 langkah 10 (`SecretField`), fitur 17 milestone 3 (layar Catalog dan `CatalogStore`), dan fitur 18 milestone 4 (`RuntimeStore`); needs fitur 67 milestone 1 dan 2 di `ui-macos.md`, atau membangun komponen itu menurut spec 0067; memakai `newInstanceAction` dan `ShellNavigator` fitur 22, dan peta perintah fitur 69 kalau sudah ada)

12. `StacklyClient`: method `getCreateForm`, `preflight`, dan test fixture; case reduce `instance.removed` di `InstanceStore` kalau langkah 16 spec 0013 belum memetakannya (K16). Memenuhi **AC-17**, **AC-20**.
13. `CreateInstanceModel`, `SchemaFormModel`, dan `CreateInstanceService` dengan test Swift dari fixture (termasuk assertion NATS) dan service tiruan. Memenuhi **AC-20**, **AC-21**.
14. `NewInstanceSheet` dan semua barisnya di atas `StacklySheet`, `CheckList`, `StagedProgress`, dan `ConsoleBlock`, mode progress dan cancel, umpan balik hasil create lewat `FeedbackCenter`, titik masuk (`NewInstanceAction` dan kunci environment nya kalau belum ada, tombol di `ProvisionalInstanceList` dengan `// provisional: F22` atau di halaman fitur 22, panel detail Catalog, ⌘N), preview, dan evidence manual keyboard serta VoiceOver dengan daemon development. *Tambahan dari spec 0067, 0068, dan 0069 (2026-10-21):* tabel status preflight tidak ditambah ke design.md (tabel *Status checklist* spec 0067 berlaku). ⌘N: kalau langkah ini dibangun sebelum milestone 2 spec 0069, `AppShell` juga memasang `.focusedSceneValue(\.newInstanceAction, …)` (kunci `FocusedValues.newInstanceAction` didefinisikan langkah ini) dan `CommandGroup(replacing: .newItem)` bertanda `// provisional: F69` membaca `@FocusedValue(\.newInstanceAction)` lalu memanggilnya; kalau sesudahnya, tanpa `CommandGroup`, cukup baris `newInstance` di `CommandMap` menjadi built dan `AppShell` menerbitkan action nya. Suite `NewInstanceSnapshotTests` di atas harness spec 0068 untuk setiap state sheet (pilih runtime, form, preflight per status, progress, gagal) kalau milestone 1 spec 0068 sudah di merge, selain itu fitur 68 yang menulisnya (spec 0068 *Key invariants*). Memenuhi **AC-20**, **AC-21**, **AC-22**.

Sesi paralel: milestone 3 bisa berjalan bersamaan dengan langkah 9 sampai 11 karena hanya bergantung pada fixture langkah 8. Langkah 14 tidak berjalan bersamaan dengan langkah 6 dan 7 spec 0022 atau langkah 2 milestone 1 spec 0016, karena semuanya mengubah `AppShell.swift` dan `StacklyApp.swift`.

**Milestone 4: PostgreSQL nyata dan bukti di Mac ini** (after milestone 2 dan 3; needs fitur 19 milestone 2 (PostgreSQL lewat daemon) dan langkah 9 milestone 3 fitur 19 (PostgreSQL sebagai `[[provider]]` di `providers/catalog.toml`, D6), dan fitur 18 milestone 2 (`install_cached_artifact`); artefak PostgreSQL di cache runtime xtask, selain itu `BLOCKED`)

15. Test native lewat daemon: create PostgreSQL 18 dan 17 dengan password bangkitan, preflight dengan listener nyata di `127.0.0.1`, `0.0.0.0`, `[::]`, `[::1]`, sisa `TIME_WAIT`, dan UDP (hasil dicocokkan dengan tabel *Hasil yang diharapkan*), race port sebelum start, lalu start sampai `ready` dengan `socketPath` benar. Memenuhi **AC-4**, **AC-12**, **AC-16**, **AC-25**.
16. Fuzz `instance_path` 10 menit dengan evidence, pengukuran kinerja AC-24, dan evidence manual GUI untuk PostgreSQL (keyboard dan VoiceOver). Memenuhi **AC-22**, **AC-23**, **AC-24**, **AC-25**.

Sesi paralel: langkah 16 bagian fuzz bisa berjalan sebagai sesi sendiri begitu langkah 6 masuk.

## Consequences

**Positive**:

- Satu evaluator membuat GUI, `--dry-run`, dan create memberi jawaban yang sama, jadi AC-028 terbukti dari struktur kode, bukan dari kebetulan.
- Provider baru (NATS di fitur 23, lalu Valkey, Traefik, MySQL, ClickHouse) mendapat form, preflight, dan CLI tanpa kode khusus.
- Create yang gagal, dibatalkan, atau terputus tidak meninggalkan apa pun selain folder paket pinned yang memang boleh dipakai ulang, jadi pengguna bisa langsung mencoba lagi dengan nama dan port yang sama, tanpa menunggu purge fitur 54.
- Konflik port terlihat sebelum Create, termasuk listener wildcard dan `localhost` yang bisa salah sasaran, tanpa salah alarm dari sisa `TIME_WAIT`, dan port fixed tidak pernah berubah diam diam.
- Paket provider yang dipakai instance tetap ada di folder data walau catalog di app berubah, jadi start offline tetap bisa.

**Negative / tradeoffs**:

- Update fase 02 untuk spec 0001, 0004, 0005, 0009, 0010, dan 0011 sudah masuk, dan update spec 0007 (tabel resume, `data_not_empty`, aturan `instance_tree` K10) diterapkan di spec 0007 pada 2026-10-20, keputusan 29 dan 30; langkah 9 harus mengganti create sementara spec 0010 beserta test yang mengharapkan `needs_recovery` setelah init gagal.
- Preflight membuka socket percobaan setiap kali draft berubah; jeda 300 ms dan pembatalan menjaga jumlahnya, tetapi tetap ada IO kecil per ketikan.
- Probe port hanya bisa menamai pemilik port kalau pemiliknya instance Stackly; proses luar ditulis "another process".
- Instance bisa terlihat sebentar di daftar selama create berjalan lalu hilang lagi lewat `instance.removed` kalau create gagal; client yang tidak menangani event itu menampilkan baris basi sampai snapshot berikutnya.
- Cancel saat init menunggu init selesai, karena langkah `Run` init tidak dibatalkan di tengah.
- "Start after creating" bergantung pada client yang tetap hidup: app yang ditutup di antara create dan start meninggalkan instance `stopped` (terlihat jujur, tetapi butuh start manual).
- Batas ruang 256 MB adalah angka tetap tanpa data per provider; provider yang butuh lebih besar baru tertangkap saat init, lalu dikompensasi.
- Create yang terputus saat pembantu init masih memegang lock butuh abandon manual setelah pembantu itu selesai, dan selama itu baris instance tetap terlihat `failed interrupted`.

**Neutral**:

- Dua method baru, satu newtype wire, sembilan kode `instance` (sudah terdaftar di tabel spec 0010), `catalog.pin_failed`, `registry.not_deletable`, dua kode `cli`, dan kenaikan minor protokol; field opsional `provider.revision` dan DTO `instance.removed` sudah dikirim langkah 12 spec 0010, dan fitur ini yang pertama mengirim event itu.
- Modul core baru `instance::create`, `catalog::pin`, dan `catalog::source`; folder Swift `CreateInstance/`; fuzz target `instance_path`; tanpa dependency baru (`socket2` dari langkah 5 spec 0010).
- Kompensator kustom pertama di luar engine (`instance_registration`).
- Aturan `check-deps` baru yang ikut memindai kode Swift.

## Follow-up

- [x] Lead: bawa *Usulan update spec lain* (spec 0001, 0004, 0005, 0007, 0009, 0010, 0011) ke program foundation sebagai item `architect (update)` sebelum milestone 1 (spec 0004 dan 0005 juga membawa usulan K-1 dan K-2 dari spec 0017). Spec 0001, 0004, 0005, 0009, 0010, dan 0011 sudah diterapkan pada 2026-10-20 (dengan keputusan K, lihat penanda di setiap blok); spec 0007 diterapkan di spec 0007 pada 2026-10-20, keputusan 29 dan 30, termasuk aturan `instance_tree` K10 di butir 3.
- [ ] Lead: update spec 0013 untuk K16 dan K17 (penyaringan retained, aturan `statusSeq`, dan AC-17 dengan tombol yang hanya dirender saat `newInstanceAction` tidak nil) sebelum langkah 16 fitur 13, karena langkah 12 dan 14 spec ini bergantung padanya.
- [ ] Lead: catat *Selisih mockup yang disengaja* di `ui-macos.md` lewat `/scope`.
- [ ] Fitur 14: pakai nama flag di *CLI* (`--runtime`, `--port`, `--set`, `--secret`) untuk `stackly instance create` dasar, supaya fitur ini hanya menambah flag.
- [ ] Fitur 21: baca purpose endpoint dan template socket dari paket pinned untuk connection string; tampilkan socket path hanya saat `socketPath` tidak null.
- [ ] Spec 0022 (lewat lead): tambahkan langkah 14 milestone 3 spec 0020 ke daftar sesi yang tidak berjalan bersamaan dengan langkah 6 spec 0022, karena keduanya mengubah `AppShell.swift`.
- [ ] Fitur 22: render tombol toolbar dan aksi empty state dari `newInstanceAction`, sediakan `ShellNavigator.show(.instances(filter: .all, select:))`, terapkan `instance.removed` di daftar dan Needs Attention, dan tambah capability provider revision ke `InstanceSummary` kalau layar instance membutuhkannya (disepakati dengan arc-f22-spec).
- [ ] Fitur 23: tidak ada perubahan create; AC-13 spec 0023 memakai ulang fixture `create-form-nats.json` dan test render Swift AC-21, fixture dibangkitkan ulang setelah perubahan paket NATS fitur 23, dan NATS pindah ke `[[provider]]` di milestone 2 fitur 23 sebelum create GUI untuk NATS (disepakati dengan arc-f23-spec).
- [ ] Fitur 31: tambah `startup` ke params `instance.create` (minor), toggle "Start at login" di sheet, dan perintah setara `--start-at-login`.
- [ ] Fitur 32: path data yang bisa diganti untuk instance baru melewati baris `data` dan pemeriksaan `volume_facts` yang sama.
- [ ] Fitur 54: purge memakai event `instance.removed` dan lock instance spec 0010.
- [x] Fitur 67: adopsi `PreflightSection`, footer sheet, dan `CreateProgressSection` kalau fitur ini membangunnya lebih dulu. Diterapkan (2026-10-21): spec fase 02 tidak membuat versi sementara; `PreflightSection` dan `CreateProgressSection` adalah pembungkus layar di atas `CheckList` dan `StagedProgress`, dan footer milik `StacklySheet` spec 0067.
- [ ] Fitur 69: daftarkan New Instance… (⌘N) di peta perintah dan hapus `CommandGroup` sementara. Diputuskan spec 0069 AC-9 dan AC-16 dan aturan dua jalur di langkah 14 (2026-10-21); tetap terbuka sampai baris `newInstance` built.
- [ ] `/sync` setelah build: catat `instance.get_create_form`, `instance.preflight`, event `instance.removed`, fuzz target `instance_path`, aturan `check-deps` baru, dan folder `CreateInstance/` di `AGENTS.md` root dan `apps/macos/AGENTS.md`.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, kesepakatan antarsesi, dan hasil cross check ada di [rationale.md](rationale.md).
