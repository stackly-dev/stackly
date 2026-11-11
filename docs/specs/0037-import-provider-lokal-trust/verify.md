# Verify: Import provider lokal dan trust · spec 0037 · updated 2026-11-11

_Langkah diturunkan dari acceptance criteria spec 0037. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27, dengan `export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`. Probe yang mengubah file sementara selalu dikembalikan sesudahnya (`git status` bersih lagi)._

## Milestone 1: Local package into the catalog

_Status 2026-11-11: langkah Build plan 1 sampai 5 dibangun (`dev-f37-m1`). Penyimpangan urutan yang disetujui lead: empat `OperationKind` operasi provider masuk di langkah 4 bersama nilai wire nya (bukan langkah 1), dan literal `ProviderEntry`/`InvalidEntry` di `apps/daemon` diperbaiki di langkah 3. `provider.approve` dan `provider.revoke` sudah ada di kontrak 4.0 tetapi handler nya menjawab `request.unknown_method` sampai milestone 2 (langkah 6). Revocation catalog (`revocations`, `revocation` preview) tetap kosong sampai langkah 8._

### UI / manual

Langkah ini butuh izin user dan tidak dijalankan sesi build. Record evidence `native` tetap `NOT_RUN` sampai dijalankan (spec 0037 *Critical test scenarios*).

- [ ] Folder terlindung: salin `crates/core/tests/fixtures/packages/acme-demo` ke `~/Downloads/acme-demo`, jalankan daemon sebagai LaunchAgent (bukan `cargo run`), lalu `stackly provider import ~/Downloads/acme-demo --dry-run` → kalau macOS menolak, balasan `provider.import_unreadable` dengan `reason` `permission_denied` dan hint yang menyebut Privacy & Security > Files and Folders; tidak ada dialog izin yang muncul dari sesi build → AC-1
- [ ] Quarantine nyata: unduh arsip paket `acme-demo` lewat Safari (atau browser lain) supaya `com.apple.quarantine` terpasang, ekstrak, lalu preview dan import → `quarantined` true dengan nama agen; `xattr -l <folder asal>/provider.yaml` sebelum dan sesudah sama; `xattr -l` salinan di `<data>/provider-imports/<packageSha256>/provider.yaml` tidak memuat `com.apple.quarantine` → AC-13
- [ ] (milestone 4) Alur GUI lengkap: Catalog › Import Provider… → panel folder → sheet Import Provider → entri `Local, not verified` dengan banner approval → AC-15
- [ ] (milestone 2) Jaringan mati: matikan Wi-Fi dan kabel, lalu preview, import, list, dan detail entri lokal → perilaku sama dengan saat online, tidak ada permintaan jaringan → AC-8

### Commands

- [x] `cargo test -p stackly-domain` → semua lulus, termasuk `registry::import::tests` (`approval_state`: tanpa keputusan `pending`, keputusan atas byte tersimpan menjadi state nya, keputusan atas byte lain tetap `pending`; kode `TrustDecision` dan `ApprovalState` bolak balik) dan `operation::lock::tests` (`provider:acme/demo` bolak balik, `provider:acme` dan `provider:Acme/demo` ditolak) → AC-18, AC-3
- [x] `cargo test -p stackly-platform --lib layout::tests::an_imported_package_lives_under_provider_imports_by_its_digest` → `provider_import_dir` = `<data>/provider-imports/<digest>`, bukan folder standar → AC-3
- [x] `cargo test -p stackly-testkit` → lulus, termasuk `registry_conformance` atas `FakeRegistry` dengan skenario `imports` (insert, baca, list, duplikat, `MissingReference` sumber, decision ditolak di `PutProviderImport`, digest paket unik, `DecideProviderImport` menaikkan revision, replace menjaga keputusan dan `created_at`, `RevisionConflict`, delete, `NotFound`, `SetProviderRevisionApproval` nol atau satu baris, `RevokeProviderApprovals` hanya provider itu, `provider_has_approved_revision`) → AC-18, AC-3
- [x] `cargo test -p stackly-storage --test registry conformance` → skenario conformance yang sama lulus di SQLite → AC-18
- [x] `cargo test -p stackly-storage --test open imports` → 3 test lulus: registry versi 12 bermigrasi ke 13 dengan semua baris tetap dan `provider_imports` kosong; fixture beku v13 terbaca lewat registry (urut provider, `approved` untuk byte tersimpan, `pending` untuk keputusan atas byte lain, `quarantined` null, `provider_revisions.approval` `approved` dengan `approved_at`); CHECK tabel menolak keputusan setengah, paket di atas 4 MiB, `origin_path` kosong, `quarantined` 2, `decided_permissions` bukan object, digest paket ganda, sumber yang tidak ada, dan ID tidak valid → AC-18
- [x] `cargo test -p stackly-storage --test open health` → `every_open_set_column_of_the_schema_is_coded_or_named_free` lulus dengan `provider_imports.decision` dan `provider_revisions.approval` di `CODED_COLUMNS`; `every_coded_column_reports_a_foreign_code_that_fails_the_normal_read` membuat kode asing di kedua kolom itu menjadi `Corrupt` di tabel dan kolomnya → AC-18
- [x] `cargo test -p stackly-storage` → semua lulus, termasuk `fixture::every_builtin_schema_version_has_a_frozen_fixture` (v13 dengan SHA-256 `9af09e89…c6a0`) dan `migration::tests::fingerprints_match_migrations` (versi 13) → AC-18
- [x] `cargo test -p stackly-core --test catalog_local` → 11 test lulus: `acme-demo` bisa diimpor dengan digest, ukuran, dan `Permissions` lengkap (capability dalam urutan spec 0017, folder urut kode, `runtime` dengan `start` dan `version_check`, `demoCli` dengan `probe` dan `open_with`, secret `password`, endpoint `client`, unduhan dengan host unik urut, `openWith` terminal); JSON `stackly.provider-permissions/v1` bolak balik dan kode asing ditolak; setiap perubahan byte mengubah `packageSha256`, `versions.json` di luar `manifestSha256`, dan `changes` `revision`/`downloads`/`download_hosts` untuk `acme-demo-next`, `content` untuk `acme-demo-template`, `downloads` untuk paket yang hanya beda `versions.json`; namespace `official`, `stackly`, `testing`, kategori `testing`, URL yang bukan `HttpsUrl`, dan artefak `fixture` menjadi `importIssues`, dan opsi test menerimanya; 33 artefak menjadi `catalog_limit` reason `artifacts`; finding `authoring` (port di init) dan `unsupported_api_version` menolak, finding `scenarios` dibuang; batas 32 provider dan 16 MiB dengan replace menghitung selisih; aturan path (absolut, UTF-8, 1024 byte, tanpa `.` atau `..`); folder symlink tidak tervalidasi; store menulis 0700 dan 0600, membaca balik dengan digest paket, tidak menulis dua kali, membandingkan `versions.json`, dan melaporkan `existing_differs`, `digest_mismatch`, `no_space` → AC-1, AC-2, AC-3, AC-6, AC-13
- [x] `cargo test -p stackly-core --test catalog_pin` → 11 test lulus tanpa perubahan: pin tetap memakai `StoreMode::Pinned` (salinan pertama `versions.json` menang, tanpa kompensasi) → AC-3
- [x] `cargo test -p stackly-core --test catalog_trust` → 8 test lulus: import termuat dari salinan store sebagai entri lokal `pending` (tile `Ac` gray, `blocked` reason `approval_required`, gate `approval_required`); approve membuka gate dan revoke menutupnya sebagai `trust_revoked` by `user` dengan `since` waktu keputusan; `require_entry_trust` membaca ulang registry (snapshot `approved` tetapi registry `revoked` ditolak; baris dengan byte lain atau yang hilang menjadi `approval_required`); `guard_trust` di dalam `commit_guarded` hanya mempercayai byte yang sama; `pinned_trust` dan `guard_pinned_trust` hanya dicabut oleh kolom `approval` nya; salinan hilang menjadi entri `invalid` dengan finding `package` dan salinan yang diubah menjadi `invalid` dengan satu issue `import_digest_mismatch`, keduanya tetap lokal dan entri lain tetap termuat; compatibility record di paket lokal diabaikan (`unverified` reason `untested`, sedangkan paket yang sama sebagai entri catalog `experimental`); `with_local` menjaga entri catalog dan mengganti entri lokal → AC-4, AC-7, AC-10, AC-11
- [x] `cargo test -p stackly-core --lib catalog::` → lulus, termasuk `refusal_codes_are_the_support_reason_codes` dengan `approval_required` dan `trust_revoked` → AC-4
- [x] `cargo test -p stackly-core` → semua lulus → AC-1, AC-2, AC-3, AC-10
- [x] `cargo clippy -p stackly-domain -p stackly-storage -p stackly-testkit -p stackly-platform -p stackly-core --all-targets --locked -- -D warnings` → bersih
- [x] `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → exit 0 → arsitektur
- [x] `cargo xtask check-deps` → lulus
- [x] `cargo test -p stackly-protocol` → lulus: `ProtocolVersion::CURRENT` 4.0, kelima method `provider.*` bertanda `recovery refused`, `Validate` params (path absolut tanpa `.`/`..` sampai 1024 byte, ID ber namespace, digest 64 hex), enum terbuka baru dengan `unrecognized`, kode `provider.*` dengan hint statis, fixture `protocols/fixtures/provider/` (preview pending, invalid, pengganti, nilai asing; detail lokal pending, approved, revoked) dan `ipc/` (setiap request, reply, `catalog.changed`, satu frame error per kode baru), kalimat izin satu baris tanpa frasa terlarang → AC-18, AC-11
- [x] `swift test --package-path clients/swift` → lulus, termasuk `ProviderImportContractTests` dan jumlah method 67 serta event `catalog.changed` di `ContractNames` → AC-18
- [x] `cargo xtask codegen --check` → "20 generated files are up to date"; `protocols/contract-versions.json` mencatat 4.0 → AC-18
- [x] `cargo xtask fuzz seed` → seed `ipc_request_decode` dan `ipc_server_frame_decode` mencakup fixture baru → AC-18
- [x] `cargo test -p stacklyd --test provider_import` → 7 test lulus: preview `acme-demo` tanpa menulis (generation registry, audit, dan pohon root tetap), dengan `quarantined` false, izin, dan digest; paket ber namespace `official` `importable` false reason `package_invalid`; folder hilang, symlink, file, dan path relatif ditolak (`provider.import_unreadable` reason `not_found`, `symlink`, `not_a_directory`; `request.invalid_params`); import menulis salinan 0700/0600, baris, dan sumber `local`, entri `local` `pending` `blocked` reason `approval_required`, `localCount` 1, satu baris audit `import_provider` dengan `provider` dan `packageSha256`, satu event `catalog.changed` `imported`; import kedua byte sama tanpa operasi, audit, atau event; byte yang berubah sesudah preview `provider.import_changed` tanpa baris atau folder; import pengganti menghapus salinan lama; restart memuat entri lokal lagi; remove ID catalog `provider.import_not_found`, remove entri lokal menghapus baris dan salinan, remove kedua `provider.import_not_found`; atribut `com.apple.quarantine` nyata di test root dilaporkan `quarantined` true agen `Safari`, tetap di folder asal, dan tidak ada di salinan store; list 64 provider, 64 planned, dan 32 entri lokal di bawah `CONTROL_FRAME_MAX` → AC-1, AC-2, AC-3, AC-9, AC-10, AC-13, AC-17
- [x] `cargo test -p stacklyd --lib map::` → lulus, termasuk `map::trust` (setiap kode core punya nilai wire, setiap error punya hint) dan fixture catalog yang ditulis ulang dengan `STACKLY_UPDATE_GOLDEN=1 cargo test -p stacklyd --lib map::catalog` → AC-18
- [x] `cargo test -p stacklyd` dan `cargo test -p stackly-cli` → semua lulus (flaky beban yang dikenal diulang sendirian)

### Hasil `/check verify` 2026-11-11 (Milestone 1, commit `20e93db3`)

**Verdict: FAIL pada empat butir** (temuan 1 sampai 4). Ke 22 langkah Commands dijalankan dengan `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0` dan lulus; angka nya sama dengan gerbang lead. Yang diuji adalah `main` `20e93db3`, yang kodenya sama dengan merge langkah 1 sampai 5 (`24f1c51f`); milestone 2 belum ada di sana. Langkah UI / manual tidak dijalankan (`NOT_RUN`, owner user).

| Perintah | Hasil |
| --- | --- |
| `cargo test -p stackly-domain` / `stackly-testkit` / `stackly-storage` / `stackly-protocol` | 338 / 159 / 161 / 403 lulus |
| `cargo test -p stackly-storage --test registry conformance`, `--test open imports`, `--test open health` | 3 / 3 / 7 lulus |
| `cargo test -p stackly-core --test catalog_local` / `catalog_pin` / `catalog_trust` / `--lib catalog::` / seluruh `stackly-core` | 11 / 11 / 8 / 51 / 1576 lulus |
| `cargo test -p stacklyd --test provider_import` / `--lib map::` / seluruh `stacklyd` / `stackly-cli` | 7 / 84 / 686 (107 `ignored`) / 591 lulus |
| `swift test --package-path clients/swift` | 280 test di 41 suite lulus, termasuk `ProviderImportContractTests` |
| clippy lima crate, `cargo check --target aarch64-pc-windows-msvc`, `cargo xtask check-deps`, `codegen --check` (20 file), `fuzz seed` lalu `git status fuzz/seeds` | bersih |

**Probe terhadap `stacklyd` sungguhan** (binary hasil build, `STACKLY_ROOT=$(mktemp -d)` berisi runtime fake dari `skeleton_seed`, `STACKLY_CATALOG_DIR` hasil `cargo xtask catalog build`, method dipanggil lewat klien IPC kecil di scratchpad dan CLI; paket probe ditaruh di bawah folder home supaya penyamaran path teruji; kedua daemon dihentikan dan kedua root dihapus sesudahnya; Application Support asli tidak disentuh):

- **Preview** `acme-demo`: identitas, publisher `acme`, tiga digest, 3312 byte, 4 file, izin lengkap, `verdict.importable: true`, `originPath` `~/Worktrees/...`. Hash semua file root dan runtime root, serta jumlah baris `audit_events`, `operations`, dan `provider_imports`, sama sebelum dan sesudah. Path relatif dan path ber `..` → `request.invalid_params`; folder hilang, symlink, dan file → `provider.import_unreadable` reason `not_found`, `symlink`, `not_a_directory`.
- **Namespace cadangan**: `official/demo` dan `stackly/demo` → preview `importable: false` reason `package_invalid` dengan `namespace_not_allowed`, import `manifest.invalid`; `testing/demo` ditolak validator (`invalid_value`).
- **Import**: digest salah → `provider.import_changed` tanpa baris dan tanpa folder. Digest benar → entri `local`, `pending`, `blocked`, `approval_required`; folder store 0700 dan file 0600 tanpa `com.apple.quarantine`; satu baris `provider_imports` dengan `origin_path` ber `~`; satu baris sumber `local` `imports`; satu operasi `import_provider` dan satu baris audit dengan `provider`, `providerRevision`, `packageSha256`, `manifestSha256`; folder asal sama byte, atribut, dan waktu ubahnya. Import kedua byte sama: tanpa operasi dan tanpa audit baru. Satu byte diubah sesudah preview → `provider.import_changed` dengan `expected` dan `actual`, tanpa perubahan.
- **Batas**: 32 entri lokal diimpor; yang ke 33 → preview `import_limit`, import `provider.import_limit` (`limit: count`, `max: 32`). Mengganti entri yang ada di batas itu tetap boleh, dan folder store lama terhapus. Batas 16 MiB hanya dibuktikan test core.
- **Gate install**: `runtime.resolve` dan `runtime.install` untuk `acme/demo` `pending` → `catalog.not_installable` reason `approval_required`, tanpa operasi, tanpa unduhan, tanpa folder runtime baru.
- **Remove**: ID catalog → `provider.import_not_found` reason `not_local`; ID asing → `not_imported`; entri lokal → baris dan folder store hilang, satu baris audit `remove_provider_import`; hash folder `instances`, `runtimes`, dan `providers` (pinned) sama sebelum dan sesudah; instance yang ada tetap bisa `start` dan `stop`; remove kedua `not_imported`; kunci idempotency yang diulang mendapat balasan pertamanya.
- **Gagal tulis store lewat daemon**: `provider-imports` dibuat 0500 → `provider.store_failed` reason `io`, operasi `failed`, tanpa baris dan tanpa folder sisa.
- **Delapan import bersamaan** atas byte yang sama → delapan balasan sukses, satu baris, satu folder.
- **Restart**: entri lokal termuat lagi. Salinan store yang diubah → `invalid` dengan `import_digest_mismatch`; salinan yang dihapus → `invalid` dengan `missing_file`; keduanya tetap `local`, entri lain tetap, kesehatan `catalog` `degraded` `2 catalog providers failed validation`.
- **Recovery mode** (versi schema file registry dinaikkan, lalu dikembalikan): kesehatan `catalog` `degraded` `local providers not loaded: registry recovery`, list tanpa entri lokal, `provider.preview_import` dan `provider.remove` ditolak `registry.recovery_mode`.
- **Path home dan secret**: nol kemunculan path home mentah di log daemon, dump registry, dan journal operasi untuk semua operasi yang sukses. Paket tidak membawa nilai secret; `permissions.secrets` hanya nama field. Pengecualian: temuan 2.

**Delapan hal yang diminta lead:**

1. Preview tidak menjalankan dan tidak menulis apa pun: terbukti (hash root sama).
2. Import menyalin ke store beralamat isi sebagai `pending`, dan byte yang berubah ditolak: terbukti.
3. Namespace `official`, `testing`, `stackly` ditolak; batas 32 terbukti lewat daemon, 16 MiB lewat test core.
4. Paket `pending` lewat jalur yang sudah ada: **install tertutup, create terbuka** (temuan 1). Titik gate yang baru datang di langkah 7: `select()` di `ipc/runtime.rs` (`provider.approval_required`, menggantikan `catalog.not_installable`), handler form sesudah `select_provider`, `resolve_package` cabang entri dan cabang `find_pinned`, `gather_duplicate`, serta `guard_trust` di commit `create_instance` dan `install_runtime`.
5. Remove tidak menghapus runtime atau data instance: terbukti.
6. Path home mentah: tidak ada untuk operasi yang sukses; ada di pesan `provider.store_failed` (temuan 2).
7. `provider_imports.decision` dan `provider_revisions.approval` terdaftar di `CODED_COLUMNS`: terbukti (`--test open health`).
8. Celah yang developer catat: delapan import bersamaan adalah celah bukti, kini tertutup probe (dengan temuan rendah 6). `NoSpace` lewat daemon adalah celah bukti; jalur gagal dan kompensasinya kini terbukti dengan reason `io`, reason `no_space` sendiri tetap hanya di test core. Kesehatan catalog saat recovery adalah celah bukti atas teks AC-10, kini tertutup probe. Aturan snapshot generation lebih tua adalah celah bukti atas teks AC-10 yang **masih terbuka**: kodenya ada (`replace_catalog` menolak generation lebih kecil), tetapi tidak ada test dan tidak bisa diprobe dari luar.

**Verdict per AC (bagian milestone 1):**

| AC | Verdict |
| --- | --- |
| AC-1 | terpenuhi |
| AC-2 | terpenuhi |
| AC-3 | terpenuhi, kecuali perbaikan salinan store yang diubah (temuan 3) dan replay kunci (temuan 4) |
| AC-9 | terpenuhi untuk bagian milestone 1 |
| AC-10 | terpenuhi; aturan generation lebih tua belum punya bukti |
| AC-13 | terpenuhi untuk preview dan import (atribut nyata di test root); dua langkah manual `NOT_RUN` |
| AC-17 | terpenuhi untuk import dan remove yang sukses; tidak untuk operasi yang gagal di store (temuan 2) |
| AC-18 | terpenuhi |

**Temuan yang perlu fix:**

1. **Tinggi. Paket lokal `pending` bisa membuat dan menjalankan instance.** Dengan runtime terpasang yang `software` nya sama (probe: paket `acme/fakeish` ber `software: fake` dan runtime fake), `instance.get_create_form` membalas form (juga tanpa `provider`, dan dengan `provider.source: "development_catalog"`), `instance.preflight` membalas `ready: true`, `instance.create` berhasil (langkah init paket berjalan), dan `instance.start` menghasilkan proses `running`, `ready`, `healthy`. Baris `provider_revisions` nya ber `publisher` `Stackly`, `source_id` sumber catalog, dan `approval` NULL, jadi sesudah remove pun create dari revision pinned itu tetap berhasil. Penyebab: `resolve_package` (`crates/core/src/instance/create/facts.rs`) dan `select_provider` (`form.rs`) mengambil entri dari snapshot gabungan tanpa cek trust. Ini melanggar *Key invariants* ("tidak ada ... init, atau start yang berasal dari entri lokal kecuali `approved`") dan AC-11 (label). Gate nya memang milik langkah 7, tetapi `provider.import` sudah dilayani di `main`, jadi keadaan `main` sekarang tidak aman untuk dirilis. Langkah 7 juga perlu menangani baris pinned yang sempat dibuat lewat celah ini.
2. **Sedang. Pesan `provider.store_failed` memuat path absolut data root**, jadi path home mentah masuk ke wire dan ke baris audit (`safe_details.message`) saat data root ada di bawah home (probe dengan root di bawah home: `... create folder failed for /Users/<user>/...`). AC-17 meminta setiap nilai lewat `Redactor` dengan home sebagai `~`, dan tabel *Kode error di wire* meminta pesan tetap `the provider package could not be stored`.
3. **Sedang. Import ulang tidak bisa memperbaiki salinan store yang diubah.** Untuk entri `invalid` ber `import_digest_mismatch`, import ulang byte yang sama gagal `provider.store_failed` reason `existing_differs`. AC-3 dan *Critical test scenarios* meminta import itu menulis ulang salinan dengan keputusan utuh. Salinan yang hilang bisa diperbaiki; yang diubah hanya bisa lewat remove lalu import, yang membuang keputusan.
4. **Sedang. `provider.import` tidak memanggil `replay_operation` sebelum pra baca folder.** Kunci idempotency yang diulang sesudah folder asal dipindah menjawab `provider.import_unreadable`, bukan balasan pertamanya (*API surface*; aturan `replay_operation` di `AGENTS.md`). Dengan folder masih ada, replay benar; `provider.remove` juga benar.

**Temuan rendah:**

5. Error yang datang lewat mesin operasi tidak membawa hint statis: `provider.import_not_found`, `provider.import_limit`, dan `provider.store_failed` tiba dengan `hint` kosong, sedangkan tabel *Kode error di wire* memberi hint untuk ketiganya. Error yang dibuat handler (`provider.import_unreadable`, `provider.import_changed`) membawa hint.
6. Guard import selalu menulis. Delapan import bersamaan berakhir dengan satu baris, tetapi `revision` baris itu 8, dengan delapan operasi dan delapan baris audit. *API surface* berkata guard tidak menulis apa pun bila baris sudah memuat byte itu.
7. Aturan "snapshot ber generation lebih tua tidak menimpa" (AC-10) belum punya test.
8. Preview paket yang tidak tervalidasi tidak membawa identitas (semua field identitas null), seperti yang developer catat.

**Tafsiran developer yang dinilai:** diterima: empat `OperationKind` dan dua `SupportReason` di langkah 4; `PutProviderImport { record, expected }` yang tidak pernah mengubah keputusan; `idempotencyKey` di envelope; `provider.approve` dan `provider.revoke` menjawab `request.unknown_method` sampai milestone 2; `origin_path` disimpan dengan `~`; `catalog_id_issue` mengabaikan entri lokal; `revocations` kosong sampai langkah 8; identitas preview null untuk paket yang tidak tervalidasi (temuan rendah 8); kategori `testing` dijaga validator; operasi provider memegang `manager:self`. Diterima dengan catatan: paket tidak valid di `provider.import` memberi `manifest.invalid` dari pra baca daemon, dan pekerjaan operasi memang membaca dan memvalidasi ulang folder (bukan hanya digest), tetapi pra baca itu mendahului replay (temuan 4).

### Cek ulang 2026-11-11 (commit `7a62eb10`, memuat `56287e5c`)

**Verdict akhir Milestone 1: PASS.** Keempat temuan tertutup, dibuktikan probe `stacklyd` nyata (root sementara dengan runtime fake, catalog hasil `cargo xtask catalog build`, CLI dan klien IPC kecil; daemon dihentikan dan root dihapus sesudahnya) dan `cargo test -p stacklyd --test provider_import --test provider_gate --test provider_trust` (7, 8, dan 4 lulus).

- **Temuan 1 (tinggi): tertutup.** Paket `acme/fakeish` `pending` ber `software` sama dengan runtime fake yang terpasang: form dengan `provider` dan form dengan `revision` → `provider.approval_required`; form tanpa `provider` → `instance.provider_ambiguous` (`candidates: acme/fakeish`); `instance.preflight` → `ready: false`, check `provider` `provider.approval_required`; `instance.create` dengan dan tanpa `--revision` → `provider.approval_required`; `runtime.resolve` dan `runtime.install` → `provider.approval_required`. Sesudahnya nol instance, nol baris `provider_revisions` untuk `acme`, tidak ada folder pinned `acme`, nol proses. Label: `trust: local`, `publisher: acme`, `blocked` reason `approval_required`; sesudah approve, pin tercatat `publisher` `acme`, `approval` `approved`, `source_id` sumber `local`. Pin asal lokal tanpa approval (dibuat dengan mengosongkan `approval` baris pin lalu remove import): create dan form dari revision pinned itu serta duplicate instance nya → `provider.approval_required`; instance yang sudah ada tetap bisa start dan stop.
- **Temuan 2: tertutup.** Dengan data root di bawah home dan `provider-imports` 0500: `provider.store_failed` ber pesan persis `the provider package could not be stored`, `reason: io`, hint `run stackly doctor`; `safe_details` baris audit tanpa path; dump registry tanpa path home mentah; tanpa baris dan tanpa folder sisa. Catatan rendah: satu baris `WARN` log daemon masih memuat path absolut folder store, sama seperti baris start daemon yang memang mencatat data root.
- **Temuan 3: tertutup.** Salinan store yang diubah → entri `invalid` (`import_digest_mismatch`), keputusan `approved` tetap; import ulang byte yang sama menulis ulang salinan (isinya sama dengan folder asal), entri kembali `available`, keputusan tetap `approved`, kesehatan `catalog` `ok`.
- **Temuan 4: tertutup.** Kunci `provider.import` yang diulang sesudah folder asal dipindah mendapat balasan pertamanya; kunci baru dengan folder hilang tetap `provider.import_unreadable`.
- Temuan rendah yang masih terbuka (dicatat developer): guard import selalu menulis; test aturan generation `replace_catalog`; identitas preview untuk paket yang tidak tervalidasi. Hint statis kini terlihat pada `provider.store_failed` dan `provider.not_approved`; `provider.import_not_found` dan `provider.import_limit` tidak diprobe ulang.

### Acceptance-criteria coverage

- AC-1 · lengkap: `provider.preview_import` hanya membaca lewat `Filesystem` dan `CodeSigning::quarantine`, `check_package` ditambah aturan import, `ImportPreview` lengkap (identitas, publisher, tiga digest, ukuran, izin, finding, `importIssues`, `verdict`, `existing`, `changes`), tanpa tulis dan tanpa audit; paket tidak valid tetap preview, folder tak terbaca `provider.import_unreadable`. Pembacaan ketat `metadata` untuk paket yang tidak tervalidasi belum ada (field identitas null)
- AC-2 · lengkap: aturan import, batas 32 provider dan 16 MiB (preview `import_limit`, import `provider.import_limit`), aturan path di `Validate` dan core
- AC-3 · lengkap: operasi `import_provider`, `provider.import_changed`, store 0700/0600 dengan baca ulang digest, baris dan sumber `local` lewat `commit_guarded` yang memeriksa ulang batas, jalur tanpa operasi untuk byte sama yang salinannya valid, folder asal tidak disentuh. Delapan import bersamaan dan `NoSpace` lewat daemon belum diuji di daemon (store `NoSpace` diuji di core)
- AC-9 · bagian milestone 1: `remove_provider_import` menghapus baris, mencabut `provider_revisions` yang `approved`, dan menghapus salinan yang tidak dirujuk; instance dan folder pinned milik milestone 2 sampai 3 untuk test lintas create
- AC-10 · lengkap: snapshot `RwLock` dengan generation, `refresh_local` saat start tanpa event dan sesudah operasi dengan `catalog.changed`, entri `invalid` lokal, recovery mode dan refresh gagal di kesehatan `catalog`, anggaran frame. Test kesehatan recovery dan snapshot generation lebih tua di daemon belum ditulis
- AC-13 · lengkap untuk preview dan import (atribut nyata di test root); langkah manual quarantine dari unduhan browser tetap di atas
- AC-17 · bagian import dan remove: baris audit di commit yang sama lewat mesin operasi dengan `provider`, `providerRevision`, `packageSha256`, `manifestSha256`; `changes` di audit import dan approve milik milestone 2
- AC-18 · lengkap: kenaikan major 4.0, method `recovery refused`, DTO, enum terbuka, kode, event, fixture Rust dan Swift, seed fuzz, migrasi 13, `CODED_COLUMNS`, `ProviderSourceKind::Local`
- AC-4 · bagian core (milestone 2 menyambungkan gate di handler); AC-11 · `trusted_compatibility` di status dan kalimat izin di fixture; AC-5 sampai AC-8, AC-12, AC-14 sampai AC-16, AC-19 · milik milestone 2 sampai 4

## Milestone 2: Approval, revocation, and the gate

_Status 2026-11-11: langkah Build plan 6 sampai 8 dibangun (`dev-f37-m2`). Langkah 6 di commit sendiri; langkah 7 dan 8 di satu commit karena keduanya mengubah `catalog::trust`, `map::trust`, dan `provider_gate.rs` yang sama. Kontrak naik minor ke 4.1: `CreateProviderSource` mendapat `local_import`. Temuan 1 verify milestone 1 (paket `pending` bisa create dan start) ditutup gate langkah 7; revision yang sempat dipin lewat celah itu (`approval` null, sumber catalog, namespace selain `official`/`testing`) kini `ApprovalRequired` di `pinned_trust`, jadi create dari pin itu dan duplicate instance nya ditolak `provider.approval_required`._

_Fix sesudah verify milestone 2 (`dev-f37-fix`, keputusan lead): (1) baris audit `import_provider` kini memuat `changes` terhadap keputusan terakhir atas byte lain, dihitung sama dengan approve (`decision_changes` di `crates/core/src/catalog/import.rs`), dan kosong pada import pertama; (2) `instance.provider_ambiguous` dari pemilihan menurut `software` (`details.candidates` tidak kosong) kini berpesan `no single approved provider runs this software; choose one of the candidates`, kodenya tetap; (3) baris `WARN` log daemon saat store gagal hanya memuat `provider` dan `reason`, tanpa path. Langkah cek nya:_

- [x] `cargo test -p stacklyd --test provider_gate -- the_audit_row a_pick` → 2 test lulus: import pertama tanpa `changes`, import revision `0.4.1` dengan host unduhan lain sesudah approve memuat `changes` dengan `revision` dan `download_hosts`; form tanpa `provider` dengan satu entri lokal `pending` memberi pesan di atas dengan `candidates` `acme/fake` → AC-17, AC-19

### UI / manual

- [ ] Jaringan mati: matikan Wi-Fi dan kabel, lalu preview, import, approve, revoke, list, dan detail entri lokal, serta `runtime.resolve` entri yang dicabut catalog → perilaku sama dengan saat online, tidak ada permintaan jaringan → AC-8

### Commands

- [x] `cargo test -p stacklyd --test provider_trust` → 4 test lulus: approve dengan digest lain `provider.import_changed` ber hint; approve byte yang ditampilkan membuka gate (`approved`, `available`, versi `installable`), satu event `approved`, satu baris audit `approve_provider`, approve ulang tanpa operasi; revoke menutup gate dan keputusan mengikuti byte nya lintas import; `settings.update` dengan kunci trust `request.invalid_params`; daemon dengan `STACKLY_TRUST=1` dan `STACKLY_APPROVE_ALL=1` tetap mengimpor `pending` → AC-5, AC-6, AC-7, AC-12, AC-17
- [x] `cargo test -p stacklyd --test provider_gate` → 8 test lulus: entri `pending` ditolak `provider.approval_required` (detail `provider`, hint) di `runtime.install`, `runtime.resolve`, form tanpa dan dengan revision, dan `instance.create`, tanpa instance atau baris `provider_revisions`, dan form tanpa provider memilih entri catalog; entri `approved` membuat form ber sumber `local_import`, create sukses dan pin menulis `publisher` `acme`, `approval` `approved`, `approved_at` waktu keputusan, dan `source_id` baris import; sesudah revoke kelima titik dan form duplicate menolak `provider.revoked` by `user`, dan instance yang ada tetap bisa start; revision yang dicabut catalog muncul di `ListProvidersReply.revocations` dan `ImportPreview.revocation`, approve nya ditolak `provider.revoked` by `catalog` dengan `catalogRevision` dan `since`, dan semua titik gate menolak; import yang diulang dengan kunci yang sama sesudah folder asal dipindah membalas detail, bukan `provider.import_unreadable` (temuan 4 verify milestone 1: `provider.import`, `provider.approve`, dan `provider.revoke` memanggil `replay_operation` sebelum membaca folder atau entri); nama tampil runtime terpasang diambil dari entri catalog, bukan entri lokal ber `software` sama → AC-4, AC-7, AC-8, AC-11, AC-19
- [x] `cargo test -p stacklyd --test provider_gate a_pending_package` → probe temuan 1 milestone 1: catalog tanpa entri `fake`, paket `acme/fake` `pending`; form dengan `provider` `provider.approval_required`, tanpa `provider` `instance.provider_ambiguous` dengan `candidates` `acme/fake`; preflight dengan dan tanpa revision tidak `ready` dengan `provider.approval_required`; create dan create duplicate dari instance yang ada ditolak `provider.approval_required`; tidak ada instance baru, baris `provider_revisions`, atau folder pinned `acme` → AC-4, AC-11
- [x] `cargo test -p stackly-core --test catalog_trust` → `a_pinned_revision_is_revoked_by_its_approval_column_and_a_local_one_needs_an_approval`: revision `acme` tanpa approval `ApprovalRequired`, `official` tanpa approval dipercaya → AC-4
- [x] `cargo test -p stacklyd --test provider_gate a_store` → temuan 2 milestone 1: dengan `provider-imports` 0500, import menjawab `provider.store_failed` dengan pesan persis `the provider package could not be stored` dan detail `reason`; tidak ada nilai `safe_details` baris audit `import_provider` yang memuat path test root → AC-17
- [x] `cargo test -p stacklyd --test provider_gate an_import_of` → temuan 3 milestone 1: salinan store yang diubah sesudah approve menjadi entri `invalid` sesudah restart; import ulang byte yang sama menulis ulang file itu, entri tidak lagi `invalid`, dan keputusan `approved` tetap → AC-3, AC-10
- [x] `cargo test -p stackly-core --test catalog_local` → `the_store_repairs_a_changed_copy_and_reports_a_full_disk_and_a_wrong_digest`: mode `Import` menulis ulang file yang berbeda; mode `Pinned` tetap `existing_differs` (`catalog_pin`) → AC-3
- [x] `cargo test -p stackly-core --test runtime_install local` → 2 test lulus: input dari entri lokal membuang compatibility record dan membawa `localPackageSha256` lewat journal (input lama tanpa field tetap terbaca); install dari entri lokal tanpa approval gagal `provider.approval_required` sebelum exec pertama, tanpa spawn `RuntimeCheck`, runtime, atau folder → AC-4, AC-11
- [x] `cargo test -p stackly-core --lib catalog::load` → lulus, termasuk tiga test `revoked`: daftar termuat dan urut, `Catalog::revocation` mencakup semua digest tanpa `manifestSha256` dan hanya digest yang disebut, `catalog_revocation` by `catalog` dengan `since` tengah malam UTC dan revision catalog, snapshot lokal menjaga daftar, index tanpa revocation tidak menulis `revoked` dan digest nya tetap; entri yang cocok dengan provider di catalog yang sama menggagalkan build dan membuat loader `index_invalid`; aturan urut, unik, ID, revision, digest, reason, batas 64, dan `since` di masa depan saat build → AC-8
- [x] `cargo test -p xtask catalog::source` → `[[revoked]]` terbaca dengan tanggal, waktu atau field asing ditolak → AC-8
- [x] `cargo xtask catalog check` → `ok catalog revision 11 7f4ebd23…` (digest catalog bawaan tidak berubah) → AC-8
- [x] `cargo test -p stackly-core --test catalog_trust` dan `--test runtime_resolve` → lulus dengan `installability`, `entry_status`, dan `resolve` yang menerima verdict → AC-4
- [x] `cargo test -p stackly-protocol` → lulus, termasuk `a_form_from_a_local_entry_names_the_local_import_source` dan `current_version_is_four_point_one` → AC-19, AC-18
- [x] `swift test --package-path clients/swift` → lulus dengan versi 4.1 → AC-18
- [x] `cargo xtask codegen --check` → "20 generated files are up to date"; `protocols/contract-versions.json` mencatat 4.1
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo xtask check-deps`, `cargo xtask evidence check`, dan cek target Windows → bersih
- [x] `cargo test -p stacklyd --test provider_gate a_snapshot` → `replace_catalog` menolak snapshot dari generation lebih tua, menjaga yang lebih baru, dan menerima generation sama → AC-10 (ditambahkan sesudah verify, commit `690889d2`; gerbang lead 9 test lulus)

### Hasil `/check verify` 2026-11-11 (Milestone 2, commit `7a62eb10`)

**Verdict: PASS, dengan satu klausa AC-17 yang belum diterapkan** (temuan 1 di bawah; kecil, butuh fix atau keputusan). Ke 16 langkah Commands dijalankan dengan `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0` dan lulus. Langkah jaringan mati tidak dijalankan (`NOT_RUN`, owner user).

| Perintah | Hasil |
| --- | --- |
| `cargo test -p stacklyd --test provider_trust` / `provider_gate` / `provider_import` | 4 / 8 / 7 lulus |
| `cargo test -p stackly-core --test catalog_trust` / `catalog_local` / `catalog_pin` / `runtime_install` / `runtime_resolve`, `--lib catalog::` | 8 / 12 / 11 / 12 / 10 / 54 lulus |
| `cargo test -p xtask catalog::source`, `cargo xtask catalog check` | 3 lulus; `ok catalog revision 11 7f4ebd23…` |
| `cargo test -p stackly-protocol`, `swift test --package-path clients/swift` | lulus (protokol 4.1); 280 test |
| `cargo xtask codegen --check`, `check-deps`, `evidence check`, cek target Windows, `cargo clippy --workspace --all-targets --locked -- -D warnings` | bersih (clippy workspace gagal sekali karena disk penuh, lalu lulus sesudah ruang dibebaskan) |

**Probe terhadap `stacklyd` sungguhan** (root sementara dengan runtime fake; paket `acme/fakeish` ber `software: fake`; semua lewat CLI `stackly provider ...` dan klien IPC kecil; daemon dihentikan dan root dihapus sesudahnya):

- **Approval mengikat `packageSha256` (AC-5, AC-6):** approve dengan digest lain → `provider.import_changed`; approve digest yang ditampilkan → `approved`, `available`, versi `installable`, satu operasi dan satu baris audit; approve ulang dan kunci yang diulang → tanpa operasi baru. Satu byte template diubah lalu diimpor → `pending`, `changes: [content]`, gate tertutup lagi, approve dengan digest lama → `provider.import_changed`. Revision `0.2.0` dengan port, capability, dan unduhan lain → `pending` dengan `changes` `revision`, `downloads`, `download_hosts`, `capabilities`, `endpoints`. Mengimpor ulang byte yang keputusannya tersimpan → kembali `approved`.
- **Revoke (AC-7):** `provider.revoke` → `blocked` reason `trust_revoked`, `revocation` by `user` dengan `since`; baris `provider_revisions` provider itu `revoked`; hash folder `instances`, `runtimes`, `providers` (pinned), dan `provider-imports` serta baris `runtimes`, `instances`, `artifacts` sama sebelum dan sesudah. Sesudahnya `provider.revoked` by `user` di `runtime.resolve`, `runtime.install`, form (dengan `provider`, dengan revision pinned), create (entri, pinned `0.1.0`, `0.2.0`), dan duplicate; form tanpa `provider` → `instance.provider_ambiguous`. Instance yang ada tetap bisa start, restart, dan stop. Revoke kedua → `provider.not_approved`; kunci revoke yang diulang → balasan pertamanya. Approve lagi membuka gate entri, sedangkan pin revision lama tetap `revoked`.
- **Remove:** pin `approved` menjadi `revoked`; create dari revision pinned dan duplicate → `provider.revoked`; instance yang ada tetap start; folder data, runtime, dan pinned sama.
- **Revocation catalog (AC-8):** catalog percobaan revision 12 dengan satu `[[revoked]]` untuk `acme/fakeish 0.1.0` (dibangun lewat `catalog check --update` dan `catalog build --out` ke scratchpad, lalu `providers/catalog.toml` dan ledger dikembalikan dengan `git checkout`). List memuat `revocations`; preview dan detail memuat `revocation` by `catalog` dengan `since` `2026-11-01T00:00:00.000Z`, `reason`, dan `catalogRevision` 12; approve → `provider.revoked` by `catalog`; resolve, form, form pinned, create, dan duplicate → `provider.revoked` by `catalog`; instance yang ada tetap start; folder data sama. CLI menulis `Revoked by the catalog in this version of Stackly (revision 12) since 2026-11-01` dan `Revoked by this catalog (revision 12, published 2026-11-11)`: revision dan tanggal terbit, tanpa klaim terkini.
- **Race revoke dan create (guard commit):** delapan ronde approve, lalu `instance create` dan `provider.revoke` bersamaan. Di semua ronde create berakhir `provider.revoked`; kedelapan operasi `create_instance` lolos gate handler lalu digagalkan di tahap `register` oleh guard, dan dikompensasi: tidak ada instance, baris, atau folder baru, dan tidak ada operasi yang belum final.
- **Antrian (spec 0007):** lima ronde `provider.approve`, `provider.revoke`, `stackly settings set telemetry`, dan `stackly instance stop --all` bersamaan → semuanya selesai dalam 6 detik, tanpa deadlock, nol operasi belum final, kesehatan daemon `ok`.
- **Audit (AC-17):** baris `approve_provider` dan `revoke_provider` memuat `provider`, `providerRevision`, `packageSha256`, `manifestSha256`; approve atas byte yang berubah memuat `changes`. Tidak ada nilai secret, dan dump registry sesudah semua probe tidak memuat path home mentah.
- **VAL-018 (AC-12):** test `provider_trust` (kunci settings asing, `STACKLY_TRUST=1`, `STACKLY_APPROVE_ALL=1`) lulus; manifest dengan `permissions.trusted: true` → `unknown_field`.
- **Copy (AC-11, PRD AC-061):** teks yang terlihat di probe ("Importing doesn't run anything...", `Trust local, not verified`, `Publisher acme (not verified)`, teks remove dan revoke, hint kode `provider.*`) dan `approval-statement.txt` dibaca ulang: tidak ada klaim sandbox, keamanan binary, publisher terverifikasi, atau kesegaran. Frasa terlarang hanya muncul di daftar test `copy.rs`.

**Verdict per AC (bagian milestone 2):**

| AC | Verdict |
| --- | --- |
| AC-4 | terpenuhi: setiap titik tabel *Gate trust* menolak, guard commit terbukti lewat race |
| AC-5 | terpenuhi |
| AC-6 | terpenuhi |
| AC-7 | terpenuhi; perbandingan byte sebelum dan sesudah dibuktikan probe, belum ada sebagai test |
| AC-8 | terpenuhi kecuali langkah jaringan mati (`NOT_RUN`) |
| AC-11 | terpenuhi untuk bagian milestone 2 |
| AC-12 | terpenuhi |
| AC-17 | terpenuhi untuk approve dan revoke; baris audit import tidak pernah memuat `changes` (temuan 1) |
| AC-19 | terpenuhi: form ber sumber `local_import`, pin dengan `publisher`, `approval`, `approved_at`, dan `source_id` yang benar |

**Tafsiran developer yang dinilai** (semua diterima): langkah 7 dan 8 di satu commit; protokol 4.1 hanya menambah `CreateProviderSource::local_import`, dan enum itu terbuka (`Unrecognized`), jadi kenaikan minor benar; hanya finding `error` yang membuat salinan store `invalid`, sejalan dengan keputusan `loadable()` spec 0036; tiga butir rendah milestone 1 belum dikerjakan dan tetap terbuka.

**Temuan:**

1. **Kecil, klausa AC-17. Baris audit `import_provider` tidak pernah memuat `changes`.** `run_import` memanggil `note(&record, &[])` (`crates/core/src/catalog/import.rs`). AC-17 meminta `changes` untuk import dan approve; import revision `0.2.0` di atas keputusan `approved` punya lima kode perubahan di preview, tetapi baris auditnya tanpa `changes`. Approve sudah benar. Perlu fix, atau keputusan bahwa `changes` cukup di approve.
2. Rendah. Pesan `instance.provider_ambiguous` untuk form tanpa `provider` berbunyi `more than one package digest matches the provider and revision`, padahal kasusnya satu entri lokal yang belum dipercaya; `details.candidates` nya benar.
3. Rendah. Residual K25 (runtime yang dipasang lewat provider lokal tetap terpasang sesudah revoke) tidak bisa diprobe dengan fixture, karena tidak ada artefak nyata untuk provider lokal; yang terbukti: baris `runtimes` dan folder runtime tidak berubah oleh revoke dan remove.

**Langkah manual (tidak dijalankan, `NOT_RUN`, owner user):** jaringan mati (AC-8); folder terlindung dan quarantine nyata (Milestone 1); alur GUI (Milestone 4).

### Cek ulang fix AC-17 2026-11-11 (commit `dffd465f`, fix `1cd34477`)

**Verdict: temuan 1 tertutup, AC-17 terpenuhi untuk import, approve, revoke, dan remove.** Dua butir rendah yang ikut diperbaiki juga tertutup. Dua kotak di atas (`the_audit_row a_pick` dan `a_snapshot`) dicentang.

- `cargo test -p stacklyd --test provider_gate` → 11 lulus, 0 gagal, termasuk `the_audit_row...`, `a_pick...`, dan `a_snapshot...`.
- **Probe `stacklyd` sungguhan** (root sementara, catalog development, paket `acme/fakeish`; daemon dihentikan dan root dihapus sesudahnya). Baris audit dibaca dari registry test root (`sqlite3 -readonly`) dan dari `stackly audit list --json`:
  - Import pertama → baris `import_provider` dengan `provider`, `providerRevision` `0.1.0`, `packageSha256`, `manifestSha256`, tanpa `changes`.
  - Sesudah approve, satu byte template diubah lalu diimpor → preview `changes: [content]`, baris audit `changes: content`.
  - Revision `0.2.0` dengan host unduhan dan port lain diimpor → preview `changes: [revision, download_hosts, endpoints]`, baris audit `changes: revision,download_hosts,endpoints`, `providerRevision` `0.2.0`. Isi `changes` di audit sama dengan yang ditampilkan preview.
  - Tidak ada baris audit yang memuat path home.
- **Pesan `instance.provider_ambiguous`:** `stackly instance create probe --runtime fake@0.1.0` tanpa `--provider`, dengan satu entri lokal `pending` ber `software` sama → pesan `no single approved provider runs this software; choose one of the candidates`, `details.candidates` `acme/fakeish`, kode tetap, tidak ada instance dibuat.
- **Baris `WARN` store:** dibaca di diff (`tracing::warn!` kini hanya membawa `provider` dan `reason`); tidak diprobe ulang dengan store yang gagal. Test `a_store` tetap lulus, dan log daemon probe ini tidak memuat path home.

Catatan: `changes` dihitung terhadap keputusan terakhir atas byte lain (sama dengan approve). Import di atas entri yang belum pernah diputuskan tidak membawa `changes`; ini sesuai teks fix dan diterima.

### Acceptance-criteria coverage

- AC-4 · lengkap di daemon dan core: `select()` runtime, form sesudah `select_provider` (`form_trust`), `resolve_package` (cabang entri dan `find_pinned`), `gather_duplicate`, guard `create_instance` (`recheck_trust`), dan `install_runtime` (baca ulang sebelum `validate_binary`, sebelum `commit`, dan generation import di batch akhir). Test spawn nol dan `FakeFetcher` tanpa permintaan ada di core untuk install; di daemon penolakan dibuktikan tanpa instance dan tanpa pin
- AC-5, AC-6, AC-12 · lengkap (langkah 6)
- AC-7 · lengkap untuk gate dan instance yang ada tetap start; perbandingan byte folder data dan pinned sebelum dan sesudah revoke belum ditulis sebagai test
- AC-8 · lengkap kecuali langkah jaringan mati di atas
- AC-11 · bagian milestone 2: compatibility record lokal tidak masuk `InstallInput` dan tidak ke matrix, nama tampil dan status runtime mengikuti entri catalog ber `software` sama; copy GUI dan CLI milik milestone 3 dan 4
- AC-17 · lengkap untuk approve dan revoke (langkah 6)
- AC-19 · lengkap: `PackageSource::LocalImport`, `PinMeta` lokal, *Software yang sama* di `select_provider`, `catalog_match`, dan `DisplayNames`

## Milestone 3: `stackly provider import`, `approve`, `revoke`, `remove`

_Status 2026-11-11: langkah Build plan 9 dibangun (`dev-f37-m3`) terhadap kontrak 4.0 di `main` `743301d9`. Binary test `approve` dan `revoke` terhadap `stacklyd` nyata ditambah `dev-f37-m2` sesudah milestone 2 masuk `main` (`56287e5c`, kontrak 4.1); perilaku daemon 4.1 sama dengan yang CLI asumsikan dari fixture, jadi tidak ada golden atau pesan CLI yang berubah._

### UI / manual

Tidak ada langkah yang butuh izin user di milestone ini. Pembacaan ulang copy CLI terhadap PRD AC-061 dilakukan `/check verify`:

- [x] Baca `apps/cli/src/commands/provider/copy.rs` dan golden `apps/cli/tests/golden/provider_import*.txt`, `provider_approve*.txt`, `provider_revoke.txt`, `provider_remove.txt`, `provider_list_local.txt`, `provider_inspect_local_*.txt` → publisher selalu "(not verified)", trust selalu "local, not verified", tidak ada klaim aman, terverifikasi, atau segar; daftar revocation selalu menyebut revision dan tanggal terbit catalog terpasang → AC-11, AC-8

### Commands

- [x] `cargo test -p stackly-cli --bin stackly commands::provider` → lulus, termasuk `copy::tests` (kalimat izin sama persis dengan `approval-statement.txt`; tidak ada frasa "sandboxed", "safe to run", "verified publisher", "trusted publisher", "malware", "up to date", "latest" di copy CLI; label `changes` dan `uses`) dan `local::tests` (issue import dan finding yang dilewati dihitung error) → AC-11, AC-14
- [x] `cargo test -p stackly-cli --bin stackly confirm::` → lulus, termasuk `an_empty_consequence_leaves_no_blank_line` → AC-14
- [x] `cargo test -p stackly-cli --test provider trust::` → 17 test lulus terhadap daemon skrip: `import --dry-run` mengirim path absolut tanpa `.` atau `..` dan menulis preview (publisher "(not verified)", izin, catatan quarantine, catatan kaki import), `--json` = `ImportPreview`; preview tidak valid exit 2 `cli.provider_invalid` dengan baris finding dan issue di stderr dan `result` tetap ada; `import` mengirim `expectedPackageSha256` dari preview dengan idempotency key, tidak bertanya, `--json` = `ProviderDetail`; byte sama menulis "Already imported."; paket tidak valid exit 2 dan batas import exit 3 `provider.import_limit`, keduanya tanpa `provider.import`; `import --approve` tanpa `--yes` (tanpa TTY atau `--json`) exit 2 `cli.confirmation_required` sebelum `provider.import`; `import --approve --yes` merangkai `provider.import` lalu `provider.approve` dengan digest entri dan key sendiri; `approve --yes` mencetak baris identitas, izin, kalimat izin persis, dan catatan kaki ke stderr; `approve`, `revoke`, `remove` tanpa `--yes` exit 2 dengan hanya `catalog.get_provider`; di pty `n` exit 9 tanpa mutasi dan `y` menyetujui; entri yang sudah `approved` tanpa request; entri catalog exit 10 `provider.import_not_found` reason `not_local` sebelum bertanya; `revoke --yes` dan `remove` (pty, juga entri `invalid`) dengan pesan dialog; exit code tiap kode `provider.*` dan `manifest.invalid` dari kategorinya (4, 3, 2, 7, 10); `list` dengan kolom `TRUST`, `needs approval`/`revoked`, ", 3 imported", dan bagian "Revoked by this catalog (revision 1, published 2026-10-20):"; `inspect` entri lokal pending, berubah, approved, revoked, dan invalid → AC-14, AC-11
- [x] `cargo test -p stackly-cli --test provider trust_stacklyd` → terhadap `stacklyd` nyata (butuh `cargo build -p stacklyd`): salinan `acme-demo` di test root di preview, diimpor (`pending`, `local`), diimpor ulang ("Already imported."), tampil di `list` dan `inspect`, lalu di remove; folder paket tidak berubah. Test kedua di root ber runtime fake (`skeleton_seed`): `revoke` entri `acme/fake` yang `pending` exit 3 `provider.not_approved`; `instance create` dari entri `pending` exit 4 `provider.approval_required`; `approve --yes` membuat `approved` dan create sukses; `revoke --yes` membuat `revoked`, create baru exit 4 `provider.revoked` by `user`, dan instance lama tetap `start` dan `stop`; `import --approve --yes` atas `acme-demo` berakhir `approved`; daftar instance hanya berisi instance yang dibuat → AC-14, AC-3, AC-5, AC-7, AC-9
- [x] `cargo test -p stackly-cli --test provider` → semua lulus, termasuk golden `provider_list*.txt` dan `provider_list.json` yang ditulis ulang dengan kolom `TRUST` dan `revocations` → AC-14
- [x] `cargo test -p stackly-cli` → semua lulus
- [x] `cargo clippy -p stackly-cli --all-targets --locked -- -D warnings` → bersih
- [x] `cargo xtask check-deps` dan `cargo xtask evidence check` → lulus

### Hasil `/check verify` 2026-11-11 (Milestone 3, commit `21c3c41c`)

**Verdict: PASS.** AC-14 dan bagian CLI AC-11 terpenuhi; tidak ada temuan yang menggagalkan AC. Delapan langkah Commands dan langkah pembacaan copy dikerjakan dengan `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0` pada `main` `21c3c41c`.

| Perintah | Hasil |
| --- | --- |
| `cargo test -p stackly-cli --bin stackly commands::provider` / `confirm::` | 12 / 6 lulus |
| `cargo test -p stackly-cli --test provider trust::` / `trust_stacklyd` / seluruh `--test provider` | 17 / 2 / 32 lulus |
| `cargo test -p stackly-cli` | 616 lulus, 1 `ignored` |
| `cargo clippy -p stackly-cli --all-targets --locked -- -D warnings`, `cargo xtask check-deps`, `cargo xtask evidence check` | bersih |

**Probe terhadap `stacklyd` sungguhan** (root sementara dengan runtime fake dari `skeleton_seed`, catalog hasil `cargo xtask catalog build`; hanya CLI, mode teks dan `--json`, tanpa terminal dan di pty sungguhan lewat skrip kecil yang menunggu prompt; daemon dihentikan dan root dihapus sesudahnya):

- **Alur penuh lewat CLI saja:** `import` (path relatif `./fakeish/../fakeish` dari folder lain) → `Imported ...`, `Approval needs approval`; `import` lagi → `Already imported.` tanpa operasi baru; `approve --yes` → `Approved`; `instance create` sukses; `revoke --yes` → `Revoked the approval ... Existing instances can still start.`; create baru exit 4 `provider.revoked` by `user`; instance lama tetap start dan stop; `remove --yes` → `RemovedProvider`; `remove` lagi exit 10.
- **stdout lawan stderr:** hasil perintah di stdout; ringkasan preview saat `import`, baris identitas, izin, kalimat izin, catatan kaki, pesan akibat revoke dan remove, dan setiap error di stderr. `import --dry-run` menulis preview ke stdout. Tidak ada kode warna ANSI di keluaran tanpa terminal, dan di pty pun tidak ada.
- **`--json`:** envelope spec 0008 (`schemaVersion` 2, `type`, `command`, `result`, `error`, `requestId`, `warnings`); `result` `ImportPreview` untuk `--dry-run`, `ProviderDetail` untuk `import`, `approve`, `revoke`, `RemovedProvider` untuk `remove`; pada kegagalan `result` null dan `error` berisi kode dan kategori, kecuali `--dry-run` paket tidak valid yang tetap mengisi `result`.
- **Exit code dari kategori:** 0 sukses; 2 `cli.provider_invalid`, `provider.import_unreadable`, `cli.confirmation_required`, dan usage; 3 `provider.not_approved` dan `provider.import_limit`; 4 `provider.revoked`; 9 `cli.confirmation_declined`; 10 `provider.import_not_found` dan `catalog.provider_not_found`.
- **Konfirmasi:** tanpa terminal dan tanpa `--yes`, `approve`, `revoke`, `remove`, dan `import --approve` → exit 2 `cli.confirmation_required` tanpa mutasi (jumlah operasi, audit, dan baris import sama; untuk `import --approve` juga tanpa `provider.import`). Di pty: prompt `[y/N]`; `y` dan `yes` menjalankan; `n`, `no`, dan `maybe` → exit 9 tanpa mutasi; `--json` tanpa `--yes` → `cli.confirmation_required` tanpa prompt. `approve` atas entri yang sudah `approved` → `... is already approved.` tanpa prompt dan tanpa operasi.
- **Masukan yang salah:** folder hilang → exit 2 `provider.import_unreadable`; namespace `official` → preview tetap tampil dengan `Publisher official (not verified)`, exit 2 `cli.provider_invalid` dengan baris `namespace_not_allowed`, tanpa `provider.import`; provider ke 33 → exit 3 `provider.import_limit` tanpa `provider.import`, dan `--dry-run` nya menulis "You've reached the limit for imported providers. Remove one first."; entri catalog (`official/nats`) pada `approve` dan `remove` → exit 10 `provider.import_not_found` reason `not_local`.
- **`list` dan `inspect`:** kolom `TRUST` (`local`, `development`), `STATE` `needs approval`, `revoked`, atau state biasa, baris catalog `..., 32 imported`. `inspect` entri lokal: `Publisher acme (not verified)`, `Trust local, not verified`, `Approval approved on <waktu>`, `Imported <waktu> from ~/...`, `Package sha256:...`, bagian `Permissions:`, `Changes since your decision: revision, endpoints`, dan sel matrix `unverified (untested)`. Dengan catalog percobaan ber `[[revoked]]`: bagian `Revoked by this catalog (revision 12, published 2026-11-11):` di `list`, `revocations` di `list --json`, baris `Revoked by the catalog in this version of Stackly (revision 12) since 2026-11-01` di `inspect`, kalimat "The catalog in this version of Stackly revokes this revision..." di preview, dan `approve` exit 4 `provider.revoked` by `catalog`.
- **Quarantine:** folder dengan atribut `com.apple.quarantine` di `provider.yaml` → baris `Quarantine downloaded by Safari` dan catatan quarantine di preview.
- **CLI tidak membaca paket dan tidak menjalankan apa pun:** di `apps/cli/src/commands/provider/` dan `confirm.rs` tidak ada pembacaan file atau spawn proses di luar modul test; path hanya dinormalkan secara leksikal lalu dikirim ke daemon.

**Pembacaan copy terhadap PRD AC-061 (langkah UI / manual):** `copy.rs`, 18 golden, dan semua teks yang muncul di probe dibaca ulang. Kalimat izin sama persis dengan `protocols/fixtures/provider/approval-statement.txt` (satu baris). Publisher selalu "(not verified)", trust entri lokal selalu "local, not verified". Tidak ada frasa "sandboxed", "safe to run", "verified publisher", "trusted publisher", "malware", "up to date", atau "latest" (nol kemunculan di golden dan di keluaran probe; daftar frasa itu hanya ada di test). Daftar revocation selalu menyebut revision dan tanggal terbit catalog terpasang.

**Tafsiran developer yang dinilai** (semua diterima):

- Preview dengan `import_limit` gagal lokal `provider.import_limit` exit 3 tanpa `provider.import`: diterima. Untuk `--dry-run`, teks spec menyebut `cli.provider_invalid` exit 2 bagi paket yang tidak `importable`; kode `provider.import_limit` lebih tepat karena paketnya sah. Perlu satu kalimat di spec.
- `--json` tanpa `--yes` selalu `cli.confirmation_required`, juga di TTY: persis spec.
- `approve`, `revoke`, `remove` pada entri catalog gagal lokal `provider.import_not_found` reason `not_local`: diterima; kodenya sama dengan yang daemon berikan.
- `approve` pada entri `approved` tidak mengirim `provider.approve`: diterima; daemon pun tidak membuat operasi.
- `import --approve` yang ditolak di konfirmasi menyimpan entri dan exit 9: diterima (terlihat di pty: entri `acme/demo` tetap `needs approval`). Syarat `--yes` atau TTY tetap dicek sebelum `provider.import`.
- `provider list --json` mendapat `revocations`: sesuai kontrak.
- Prompt approve ke stderr; `import --yes` tanpa `--approve` adalah usage error; tidak ada flag `--idempotency-key`: diterima.

**Temuan rendah** (tidak menggagalkan AC):

1. Error `provider.import_limit` yang dibuat CLI sendiri tidak membawa `details` (`limit`, `max`), sedangkan error yang sama dari daemon membawanya.
2. Teks spec bagian *CLI* menulis `import <dir> [--approve] [--yes]` seolah `--yes` boleh sendiri, dan `--dry-run` untuk `import_limit` seolah `cli.provider_invalid`; keduanya berbeda di kode dengan alasan yang masuk akal. Perlu disesuaikan di spec.
3. `remove` atau `approve` untuk ID yang tidak ada di catalog menjawab `catalog.provider_not_found` (exit 10), bukan `provider.import_not_found` reason `not_imported`; kategori dan exit code nya sama.

### Acceptance-criteria coverage

- AC-14 · lengkap di CLI: keempat subcommand memanggil method yang sama dengan GUI dan tidak membaca file paket; `--json` memberi `ImportPreview`, `ProviderDetail`, atau `RemovedProvider`; konfirmasi TTY dengan `cli.confirmation_required` tanpa request mutasi (juga sebelum `provider.import` untuk `import --approve`); `list` dan `inspect` menampilkan trust, approval, dan izin; exit code dari kategori, dibuktikan juga terhadap `stacklyd` nyata untuk `approve`, `revoke`, `import --approve`, dan create sesudah revoke
- AC-11 · bagian CLI: kalimat izin persis dari fixture (test membaca file yang sama), publisher "(not verified)", trust "local, not verified", pindaian frasa terlarang atas copy CLI. Copy GUI milik milestone 4
- AC-8 · bagian tampilan CLI: `list` dan `inspect` menyebut revision dan tanggal catalog terpasang, tanpa klaim kesegaran; isi revocation milik langkah 8
- AC-1 sampai AC-7, AC-9, AC-10, AC-12, AC-13, AC-15 sampai AC-19 · milik milestone 1, 2, dan 4

## Milestone 4: Import and approval in the app

_Status 2026-11-11: langkah Build plan 10 sampai 12 dibangun (`dev-f37-m4`) dari fixture `protocols/fixtures/provider/`. Sesudah milestone 2 masuk `main` (`56287e5c`, protokol 4.1), `LocalProviderEndToEndTests` menjalankan model GUI terhadap `stacklyd` nyata; perilaku daemon sama dengan yang diasumsikan model dari fixture, jadi sumber StacklyUI tidak berubah. Langkah 13 ada di bagian sesudah milestone ini._

Tafsiran yang diambil (teks AC tidak berubah):

- Label folder "Writes to": `config` Configuration, `data` Data, `logs` Logs, `run` Runtime state, `temp` Temporary files (spec hanya menyebut "label folder").
- Umpan balik sesudah approve, revoke, dan remove: "Approved {name}.", "Revoked the approval of {name}.", "Removed {name} from the catalog." (spec hanya menetapkan teks import).
- Import yang ditolak daemon membiarkan sheet dengan preview dan alasan daemon, dengan tombol utama "Preview Again" yang membaca folder lagi.
- Kartu dengan label badge lebih dari 12 karakter ("Needs Approval", "Unrecognized State") menaruh badge di bawah nama, supaya nama tidak terpotong per huruf di kolom sempit.
- Settings › Sources: entri lokal hanya tampil di grup "Imported providers", tidak diulang di grup "Providers". Tanggal impor di baris itu datang dari `catalog.get_provider` (`local.importedAt`), karena `ProviderSummary` tidak membawanya.
- Sheet approve memakai `StacklySheet(cancelIsDefault: true)`: Cancel digambar sebagai tombol default dan Approve sebagai tombol biasa. Tidak ada shortcut keyboard (Interaction policy 2026-10-24).

### UI / manual

Langkah ini butuh izin user dan tidak dijalankan sesi build.

- [ ] Alur GUI lengkap (milestone 2 harus sudah di `main`): jalankan app Debug dengan daemon nyata, Catalog › toolbar "Import Provider…" → panel folder dengan tombol "Choose" → pilih salinan `crates/core/tests/fixtures/packages/acme-demo` → sheet "Import Provider" dengan Publisher "acme (not verified)", Package 12 hex, Folder `~/...`, Permissions, dan catatan kaki "Importing doesn’t run anything…" → Import → sheet tertutup, kartu "Acme Demo" terpilih dengan badge "Needs Approval" dan meta "Database · Local", kartu umpan balik "Imported Acme Demo. Approve it before you install anything." → AC-15
- [ ] Banner "Not approved yet" → Approve… → sheet `Approve “Acme Demo”?` dengan kalimat izin persis seperti `protocols/fixtures/provider/approval-statement.txt`, Cancel digambar sebagai tombol default → Approve → badge kembali ke state biasa, banner hilang, Get di matrix tidak lagi diblok `approval_required` → AC-15, AC-11, AC-5
- [ ] Revoke Approval… → dialog native `Revoke approval for “Acme Demo”?` dengan Cancel default dan tombol merah "Revoke Approval" → banner "Approval revoked" dengan "Approve Again…"; instance yang sudah ada tetap bisa Start → AC-15, AC-7
- [ ] Remove… → dialog `Remove “Acme Demo” from the catalog?` → kartu hilang; folder asal tidak berubah (`ls -la` sebelum dan sesudah sama) → AC-15, AC-9
- [ ] Ubah satu template di folder asal lalu import lagi → sheet menulis "Replaces imported revision …" dan "Differs from the version you decided on: configuration content."; detail menampilkan banner "Changed since your approval" → AC-15, AC-6
- [ ] Settings › Sources: grup "Imported providers" dengan baris "{id} · {approval} · imported {tanggal}" dan "Show in Catalog" yang membuka jendela utama di Catalog dengan entri itu terpilih; "No imported providers." saat kosong; tombol "Import Provider…" membuka sheet di jendela Settings; matikan daemon → tombol nonaktif dengan alasan "Not available while stacklyd is offline." → AC-16
- [ ] Onboarding (reset dengan `defaults delete com.ojiepermana.stackly` pada akun uji, bukan akun utama): langkah Catalog Trust tanpa baris atau toggle baru, subjudul "Providers describe how to install and run each server. Stackly only installs and runs providers from sources you trust." dan kalimat "You can import a provider from a folder later in Catalog. It stays blocked until you approve it." → AC-16
- [x] `/check verify` membaca ulang semua copy `ProviderTrustCopy.swift` terhadap PRD AC-061 dan mencatat hasilnya → AC-11

### Commands

- [x] `xcrun swift-format lint --strict` atas semua file Swift yang diubah milestone ini → bersih
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter ProviderTrustCopyTests` → lulus: kalimat izin sama persis dengan `approval-statement.txt`; pindaian frasa terlarang ("sandboxed", "safe to run", "verified publisher", "trusted publisher", "malware", "up to date", "latest") atas semua copy fitur ini, label CatalogCopy lokal, banner setiap state, dan copy onboarding baru; tabel label `uses` dan `changes`; banner pending tanpa keputusan, berubah sejak keputusan, revoked user, dan revoked catalog dengan revision dan tanggal terbit; approved `neutral`, bukan `positive` → AC-11, AC-15
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter ProviderImportModelTests` → lulus: Cancel di panel folder tanpa sheet dan tanpa request; preview tanpa mutasi; folder tak terbaca; Import mengirim `expectedPackageSha256` yang ditampilkan; penolakan menyimpan preview dan alasan; paket tidak valid dan paket yang sama tidak mengirim apa pun, salinan rusak boleh "Import Again"; catatan pengganti, perubahan, dan quarantine; masalah dipotong di 20 dengan "and N more"; Approve mengirim digest detail yang ditampilkan; `provider.import_changed` menutup sheet dan memuat ulang; penolakan lain tetap di sheet; Revoke dan Remove bertanya dulu dengan copy destruktif; urutan grup izin → AC-15
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter CatalogStoreTests` → lulus, termasuk `catalog.changed` memuat snapshot lagi dan `revocations` serta entri lokal tersimpan → AC-15, AC-8
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "SettingsAvailabilityTests|OnboardingCopyTests|CatalogTrustContentTests|FixtureScenarioTests"` → lulus: `.importProviders` `isBuilt: true` milik feature 37, `.communitySources` tetap tidak dibangun dengan pemilik "Deferred (public registry)", copy onboarding baru, scenario `settings-sources-imported` → AC-16
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "CatalogLocalSnapshotTests|SettingsSnapshotTests|OnboardingSnapshotTests"` → lulus terhadap referensi: `catalog-import-preview`, `catalog-import-invalid`, `catalog-import-replaces`, `catalog-local-pending`, `catalog-local-changed`, `catalog-local-approved`, `catalog-local-revoked`, `catalog-local-catalog-revoked`, `catalog-approve-sheet` (light, dark, narrow), `settings-sources-imported` (light, dark), dan `onboarding-catalog-trust` yang direkam ulang → AC-15, AC-16
- [x] `cargo xtask snapshot review` → `target/snapshot-review/index.html` menampilkan PNG baru dan `onboarding-catalog-trust` lama dan baru untuk ditinjau
- [x] `cargo build -p stacklyd -p stackly-fake-server && cargo build -p stacklyd --example skeleton_seed && swift test --package-path apps/macos/Packages/StacklyUI` → semua lulus (flaky beban yang dikenal diulang sendirian)
- [x] `cargo build -p stacklyd -p stackly-fake-server && cargo build -p stacklyd --example skeleton_seed && swift test --package-path apps/macos/Packages/StacklyUI --filter LocalProviderEndToEndTests` → lulus: terhadap `stacklyd` nyata (root `skeleton_seed`, catalog kosong `stackly-testkit` sebagai `STACKLY_CATALOG_DIR`) model GUI mengimpor salinan `providers/fake` sebagai `acme/fake` dengan `pending` dan badge "Needs Approval"; form dan create ditolak `provider.approval_required`; digest basi menutup sheet approve dengan umpan balik `provider.import_changed`; approve membuka gate (form ber `source` `local_import`, create berhasil); revoke dari panel membuat create baru ditolak `provider.revoked` `by` `user` dengan hint "existing instances still start" dan banner "Approval revoked" ber "Approve Again…", sedangkan instance lama tetap start dan stop; approve dari koneksi lain sampai ke `CatalogStore` lewat `catalog.changed` saja; remove menghilangkan entri → AC-4, AC-5, AC-7, AC-9, AC-15
- [x] `swift test --package-path clients/swift` → lulus
- [x] `cargo xtask build-app` → BUILD SUCCEEDED
- [x] `grep -rn "provisional: F37" apps/macos clients/swift` → tidak ada hasil

### Hasil `/check verify` 2026-11-11 (Milestone 4, commit `78c8ccef`)

**Verdict: PASS untuk bagian otomatis.** AC-11 (bagian GUI), AC-15, AC-16, dan bagian tampilan AC-8 dan AC-13 terpenuhi dari fixture, snapshot, dan test terhadap `stacklyd` nyata; tidak ada temuan yang menggagalkan AC. Tujuh langkah GUI manual tidak dijalankan (`NOT_RUN`, owner user). Ke 12 langkah Commands dijalankan dengan `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0` dan lulus.

| Perintah | Hasil |
| --- | --- |
| `xcrun swift-format lint --strict` atas 60 file Swift yang berubah sejak `e5506e2c` | bersih |
| `swift test --package-path apps/macos/Packages/StacklyUI` (seluruhnya, sesudah build `stacklyd`, fake server, dan `skeleton_seed`) | 2109 test di 208 suite lulus dalam 325 detik; 6 "known issue" semuanya dari self test `SnapshotHelperTests` |
| suite `ProviderTrustCopyTests`, `ProviderImportModelTests`, `CatalogStoreTests`, `SettingsAvailabilityTests`, `OnboardingCopyTests`, `CatalogTrustContentTests`, `CatalogLocalSnapshotTests`, `SettingsSnapshotTests`, `OnboardingSnapshotTests`, `LocalProviderEndToEndTests` | semuanya berjalan dan lulus di run penuh itu (`LocalProviderEndToEndTests` 38 detik terhadap daemon nyata, tidak `BLOCKED`) |
| `swift test --package-path clients/swift` | 281 test lulus |
| `cargo xtask build-app` | `built .../target/app/Debug/Stackly.app` |
| `cargo xtask snapshot review --base e5506e2c` | `target/snapshot-review/index.html` dibuat |
| `grep -rn "provisional: F37" apps/macos clients/swift` | kosong |

**Kode yang dibaca:**

- Sheet approve: `Text(ProviderTrustCopy.approvalStatement)`, dan `ProviderTrustCopyTests` membandingkannya dengan `approval-statement.txt`; `StacklySheet(cancelIsDefault: true)`; `provider.approve` dikirim dengan digest detail yang ditampilkan.
- Revoke dan Remove: `confirmationDialog` dengan role `.destructive`.
- Aksi mutasi memakai `StacklyAction(mutates: true)`, jadi `ActionHost` mematikannya saat daemon offline dengan alasan; tombol toolbar memakai "Not available while stacklyd is offline.".
- File baru tidak menawarkan cancel, resume, atau abandon operasi; "Cancel" di kedua sheet hanya menutup sheet.
- `CatalogStore` berlangganan kind `catalog.` dan memuat snapshot lagi (`CatalogStoreTests`; di test end to end approve dari koneksi lain sampai lewat `catalog.changed` saja).
- `SettingsAvailability`: `.importProviders` `isBuilt: true` milik "feature 37"; `.communitySources` tetap tidak dibangun, pemilik "Deferred (public registry)".

**Pembacaan copy terhadap PRD AC-061:** `ProviderTrustCopy.swift` dan `CatalogCopy.swift` dibaca ulang. Kalimat izin sama persis dengan fixture. Sembilan teks yang juga ada di CLI (`copy.rs`) sama kata demi kata (catatan kaki approve dan import, catatan quarantine, paket sama, salinan rusak, revocation catalog, batas import, pesan revoke, pesan remove). Judul dan pesan banner sama dengan tabel *Banner approval*. Trust "Local, not verified", publisher "(not verified)". Tidak ada frasa "sandboxed", "safe to run", "verified publisher", "trusted publisher", "malware", "up to date", atau "latest" di teks tampil (satu kemunculan "verified publisher" ada di komentar dokumentasi yang menjelaskan larangannya).

**Tinjauan PNG referensi** (dibuka satu per satu):

- `catalog-import-preview`: header, baris Publisher "acme (not verified)", Category, Software, Versions, Package 12 hex, Folder `~/...`, catatan quarantine, grup Permissions dalam urutan spec, Cancel dan Import. Sesuai.
- `catalog-import-invalid`: banner kritis "This package can't be imported" dengan `provider.yaml metadata.id · Namespace official can't be imported.`, Import nonaktif dengan alasan. Sesuai.
- `catalog-import-replaces`: "Replaces imported revision 1.0.0..." dan "Differs from the version you decided on: revision, files it downloads, download hosts.". Sesuai.
- `catalog-approve-sheet`: judul `Approve "Acme Demo"?`, baris identitas, kalimat izin utuh tanpa terpotong, "Approve only if you trust where this provider came from.", Permissions, perubahan, catatan kaki. Sesuai. Tombol default tidak terlihat bedanya di PNG (render di luar layar); itu bagian langkah GUI manual.
- `catalog-local-pending`, `-changed`, `-revoked` (narrow): kartu dengan badge "Needs Approval" di bawah nama atau "Revoked", meta "Database · Local", banner dengan judul, pesan, dan aksi menurut tabel, "Needs your approval." atau "Approval revoked.", sel macOS "Unverified", Get nonaktif. Sesuai; tidak ada teks terpotong.
- `catalog-local-catalog-revoked`: banner "Revoked by the catalog" dengan revision, tanggal terbit, `since`, dan reason. Lihat temuan 1.
- `settings-sources-imported`: "2 revoked revisions in this catalog", grup "Imported providers" dengan "acme/demo · Needs Approval · imported Oct 17, 2026" dan "Show in Catalog"; entri lokal tidak diulang di "Providers". Sesuai.
- `onboarding-catalog-trust`: subjudul dan kalimat tambahan baru, tanpa baris atau toggle baru. Sesuai.

**Tafsiran developer yang dinilai** (semua diterima): label "Writes to"; teks umpan balik approve, revoke, remove; "Preview Again"; badge panjang di bawah nama (terlihat benar di kolom sempit); entri lokal hanya di "Imported providers"; `ShellDestination.provider` dengan `CatalogRequest` sekali pakai (pola yang sama dengan `instance(UUID)`); `StacklySheet(cancelIsDefault:)`; umpan balik gagal yang persisten; helper test yang memuat ulang lewat epoch.

**Temuan rendah** (tidak menggagalkan AC):

1. PNG `catalog-local-catalog-revoked` (tiga varian) menampilkan tombol "Get" aktif. Penyebabnya fixture test: `ProviderFixture.catalogRevokedDetail()` dibuat dari `detail-local-approved` dan hanya mengubah `summary` serta menambah `revocation`, jadi `versions[].install.installable` tetap true. Daemon nyata memberi `installable: false` reason `trust_revoked` (probe CLI Milestone 2 dan 3: `no (trust_revoked)`). Saran: perbaiki fixture dan rekam ulang tiga PNG itu.
2. Snapshot panel detail terpotong di batas layar: baris Publisher, Trust, Source, Package, seksi Permissions, dan tombol "Revoke Approval…" serta "Remove…" tidak terlihat di PNG mana pun. Teks nya dijaga test model, tetapi tidak punya referensi visual.
3. Di `catalog-import-preview` catatan kaki "Importing doesn't run anything..." ada di bawah batas gulir, dan di `settings-sources-imported` tombol "Import Provider…" tidak ikut tertangkap.

### Acceptance-criteria coverage

- AC-11 · bagian GUI lengkap: trust "Local, not verified", publisher "(not verified)", kalimat izin persis dari fixture di sheet approve, pindaian frasa terlarang; bagian CLI milik milestone 3
- AC-15 · lengkap dari fixture: tombol toolbar (nonaktif dengan alasan saat offline, hilang saat `unsupported`), panel folder, sheet Import Provider dengan setiap state (loading, gagal baca, tidak valid, sama, pengganti, gagal import), banner approval per state, baris Publisher, Trust, Source, Package, seksi Permissions, Approve…, Revoke Approval…, Remove…, sheet approve dengan Cancel default, dialog native, kartu umpan balik, snapshot; alur terhadap daemon nyata untuk approve dan revoke menunggu milestone 2
- AC-16 · lengkap: grup "Imported providers", "Show in Catalog", "Import Provider…" di Settings, hitungan revocation, `SettingsAvailability`, copy onboarding
- AC-8 · bagian tampilan: banner "Revoked by the catalog" dengan revision dan tanggal terbit catalog terpasang, hitungan revocation di Settings, tanpa klaim kesegaran; isi revocation dari daemon milik milestone 2
- AC-13 · bagian tampilan: catatan quarantine di sheet import
- AC-1 sampai AC-7, AC-9, AC-10, AC-12, AC-14, AC-17 sampai AC-19 · milik milestone 1 sampai 3

## Langkah 13: evidence and manual steps

_Status 2026-11-11 (`dev-f37-m4`): guard evidence `fixture` ditambahkan pada test daemon fitur ini untuk ID PRD di *Trace PRD* (VAL-018 belum punya guard sama sekali): `provider_import.rs` (AC-033, REQ-033), `provider_trust.rs` (REQ-032, AC-032, AC-072, VAL-018), `provider_gate.rs` (AC-033, AC-072, REQ-032); test Swift membawa REQ-032, REQ-033, AC-033, AC-032, AC-061, AC-072, BR-020; CLI membawa REQ-028. PAGE-08 dan OQ-009 bukan ID yang diterima evidence (awalan hanya AC, REQ, NFR, VAL, BR), jadi PAGE-08 dibuktikan lewat snapshot dan langkah manual 3, dan OQ-009 tetap terbuka (*Security model*). Empat record `native` `NOT_RUN` di `evidence/manual/` menunggu langkah di bawah; owner semuanya user._

### UI / manual

Langkah ini butuh izin user dan tidak dijalankan sesi build. Pakai root sementara (`STACKLY_ROOT`), tidak pernah Application Support asli, kecuali langkah 1 yang butuh agent terpasang. Sesudah setiap langkah, isi record nya (`steps`, `actual`, `status`, `reviewer`, `finishedAt`) dan jalankan `cargo xtask evidence check`.

- [ ] **Import dari folder terlindung** (record `evidence/manual/2026-11-11-spec-0037-import-from-a-protected-folder-by-the.json`, AC-033, REQ-033): pasang bundle dengan `cargo xtask build-app --install`, nyalakan agent, salin `crates/core/tests/fixtures/packages/acme-demo` ke `~/Downloads/acme-demo`, jalankan `stackly provider import ~/Downloads/acme-demo --dry-run` lalu tanpa `--dry-run`, dan ulangi lewat Catalog › "Import Provider…" di GUI → berhasil, atau macOS menolak dan Stackly menampilkan `provider.import_unreadable` reason `permission_denied` dengan hint yang menyebut Privacy & Security > Files and Folders; tidak ada crash dan tidak ada teks yang meminta mematikan proteksi → AC-1, AC-13
- [ ] **Quarantine nyata** (record `evidence/manual/2026-11-11-spec-0037-quarantine-of-a-package-downloaded-by.json`, AC-033, REQ-033): unduh arsip `.zip` paket `acme-demo` lewat browser, buka dengan Finder, lalu preview dan import folder hasil ekstraknya → preview menampilkan `quarantined` true dengan nama agen dan catatan "This folder was downloaded from the internet. Stackly only reads its text files and never changes the folder."; import berhasil; `xattr -l <folder asal>/provider.yaml` sebelum dan sesudah sama dan masih memuat `com.apple.quarantine`; `xattr -l <data>/provider-imports/<packageSha256>/provider.yaml` tidak memuatnya → AC-13
- [ ] **Alur GUI lengkap** (record `evidence/manual/2026-11-11-spec-0037-full-gui-flow-of-import-approval-revok.json`, REQ-032, REQ-033, AC-033, AC-061, AC-072; PAGE-08): app Debug di atas daemon nyata yang memuat milestone 2, root sementara, lalu delapan langkah rinci *Alur GUI* di bagian `### UI / manual` Milestone 4 di atas: Import Provider… → sheet preview → Import → banner "Not approved yet" → Approve… (Cancel default, kalimat izin persis `approval-statement.txt`) → install runtime dan create instance → Revoke Approval… → create baru ditolak dan instance lama tetap start → Remove…; plus Settings › Sources › Imported providers dan langkah Catalog Trust onboarding; plus tinjauan snapshot dengan `cargo xtask snapshot review` → AC-11, AC-15, AC-16
- [ ] **Jaringan mati** (record `evidence/manual/2026-11-11-spec-0037-approval-and-revocation-with-the-netwo.json`, REQ-032, AC-072): matikan Wi-Fi dan kabel, lalu ulangi approve, revoke, start instance lama, preview, import, list, detail entri lokal, dan `stackly provider list` → semuanya sama dengan saat online; tidak ada teks "latest", "up to date", atau klaim daftar revocation terkini; yang tertulis hanya revision dan tanggal terbit catalog terpasang; `runtime.install` baru gagal hanya dengan `runtime.download_offline` biasa → AC-8

### Commands

- [x] `cargo test -p stacklyd --test provider_import --test provider_trust --test provider_gate` → lulus dengan guard evidence `fixture` di sepuluh test (lihat status di atas) → AC-1, AC-3, AC-4, AC-5, AC-6, AC-7, AC-8, AC-9, AC-12
- [x] `cargo xtask evidence check` → lulus, termasuk empat record `NOT_RUN` baru di `evidence/manual/`

### Hasil `/check verify` 2026-11-11 (Langkah 13, commit `78c8ccef`)

**Verdict: PASS untuk bagian otomatis; empat bukti `native` `NOT_RUN`, owner user.**

- `cargo test -p stacklyd --test provider_import --test provider_trust --test provider_gate` → 7, 4, dan 9 lulus; sepuluh guard `evidence!` ada di ketiga file itu, dengan `covers` AC-033, REQ-033, REQ-032, AC-032, AC-072, dan VAL-018.
- `cargo xtask evidence check` → `68 manual record(s), baseline valid`.
- Empat record `evidence/manual/2026-11-11-spec-0037-*.json`: `kind` `native`, `method` `manual`, `status` `NOT_RUN`, `covers` sesuai langkahnya (REQ-033 dan AC-033 untuk folder terlindung dan quarantine; REQ-032, REQ-033, AC-033, AC-061, AC-072 untuk alur GUI; REQ-032 dan AC-072 untuk jaringan mati), `expected` menyebut hasil yang harus diamati. Field `owner` nya null: spec 0002 hanya mewajibkan `owner` untuk `BLOCKED`, dan 15 dari 16 record `NOT_RUN` lain di repo juga begitu. Pemiliknya (user) tertulis di bagian ini.
- ID Trace: PAGE-08 dan OQ-009 memang bukan ID evidence. PAGE-08 dibuktikan snapshot, test model, dan record alur GUI (yang membawa AC-033 dan AC-061); itu memadai sampai langkah GUI manual dijalankan. OQ-009 sengaja tetap terbuka (*Security model*).
- Guard Swift membawa REQ-032, REQ-033, AC-033, AC-032, AC-061, AC-072, BR-020; guard CLI REQ-028.

### Acceptance-criteria coverage

- AC-8, AC-13, AC-15 · bagian otomatis lengkap; bukti `native` empat langkah di atas `NOT_RUN`, owner user
- VAL-018 · kini punya guard `fixture` (`provider_trust.rs`, dua test AC-12)


## Test suite `/test` dan fix 2026-11-11 (`tst-f37-test`, `dev-f37-fix2`)

_Setiap AC-1 sampai AC-19 punya test yang menyebut ID nya; setiap titik tabel *Gate trust* punya test untuk `pending`, `revoked` by user, dan `revoked` by catalog, kecuali exec check `install_runtime` yang dicabut di tengah unduhan (hanya `pending` yang teruji, di `crates/core/tests/runtime_install.rs`)._

### Commands

- [x] `cargo test -p stackly-core --test catalog_local operation` → operasi `import_provider` di `FakePlatform`: refresh tepat sekali sesudah commit; disk penuh → `provider.store_failed` reason `no_space` tanpa baris, folder, atau refresh; disk penuh saat memperbaiki salinan → folder yang sudah ada tetap; guard menolak di batas 32 → salinan yang ia tulis dihapus kompensasi → AC-2, AC-3, AC-10
- [x] `cargo test -p stacklyd --test provider_gate` → perbandingan byte baris `runtimes`, `instances`, `artifacts` dan folder instance, runtime, pinned, serta store sebelum dan sesudah revoke lalu remove, dengan start, restart, stop, dan ubah config sesudah revoke; race create lawan revoke di guard commit; antrian bersama `settings.update` dan stop all; pin asal lokal tanpa approval; `find_pinned` dicabut user dan catalog; replay → AC-4, AC-7, AC-8, AC-9, AC-18
- [x] `cargo test -p stacklyd --test provider_import` → delapan import bersamaan (satu baris, `revision` 1, satu salinan); import ke 33 dan pengganti di batas; hint `provider.import_limit` dan `provider.import_not_found`; kesehatan `catalog` saat recovery; satu `catalog.changed` per operasi sebelum operasi selesai dan tidak ada untuk import yang gagal; 64 revocation dalam satu control frame → AC-2, AC-3, AC-10, AC-14
- [x] `cargo test -p stackly-cli --test provider no_provider_golden` → tidak ada frasa terlarang di golden provider → AC-11

### Fix yang masuk

- `3a0041b5` `fix(core)`: import yang gagal sesudah membuat folder store (disk penuh) kini menghapus folder itu lewat kompensator; folder yang sudah ada sebelumnya tidak pernah diklaim.
- `c716d173` `fix(core)`: guard import yang menemukan byte nya sudah tersimpan tidak menulis apa pun (temuan rendah 6 milestone 1); setiap import itu tetap operasi dengan baris audit dan tetap me refresh catalog.

### Tafsiran replay (keputusan lead 2026-11-11, opsi C)

Request `provider.import`, `provider.approve`, atau `provider.revoke` yang diulang dengan `idempotencyKey` yang sama tidak menjalankan operasi kedua dan menjawab dengan semantik snapshot spec 0007 *Idempotency*: detail provider saat ini (bukan salinan balasan pertama), `provider.import_not_found` bila entri sudah di remove, dan error operasi pertama bila operasi itu gagal. `provider.remove` menjawab `RemovedProvider` yang sama. Dikunci oleh `a_repeated_approve_or_revoke_answers_the_provider_as_it_is_now_and_import_not_found_after_a_remove`, `a_repeated_request_whose_first_operation_failed_answers_its_first_error`, dan `an_approve_and_a_revoke_repeated_with_their_key_run_no_second_operation`. Teks *API surface* diselaraskan architect.


## Verify penuh fitur 37

### Hasil `/check verify` 2026-11-11 (seluruh spec, commit `242b795b`)

**Verdict: PASS.** Kesembilan belas AC terpenuhi untuk semua yang bisa dibangun dan dijalankan sesi, dan keenam kalimat **Done when** terpenuhi. Tidak ada `specced but missing`. Empat bukti `native` tetap `NOT_RUN` dengan owner user (GUI dengan mata, folder terlindung, quarantine dari unduhan browser, jaringan mati); menurut instruksi awal user itu tidak menghalangi PASS. Ada satu temuan rendah (baris audit import yang gagal tidak menyebut provider) dan satu catatan cakupan test.

Milestone 1 sampai 4 dan Langkah 13 sudah PASS dan tidak diulang satu per satu. Yang diperiksa di sini adalah seluruh AC pada `main` sesudah merge terakhir, dengan fokus pada tiga perubahan yang belum pernah diperiksa (`3a0041b5`, `c716d173`, aturan replay opsi C) dan pada celah trust milestone 1.

**Cara.** Tiga probe terhadap `stacklyd` dan `stackly` nyata hasil build `HEAD`, masing masing di `STACKLY_ROOT` sementara yang diisi `skeleton_seed` (runtime fake 0.1.0), dengan catalog development revision 11 di `STACKLY_CATALOG_DIR` (dan revision 12 dengan satu entri `revoked` untuk AC-8). Request dikirim lewat socket dan lewat CLI; registry dibaca dengan `sqlite3` read only. Tidak ada Application Support asli, tidak ada jaringan, tidak ada GUI, tidak ada unduhan. Semua daemon probe dihentikan dan semua root sementara dihapus. Record: `evidence/manual/2026-11-11-spec-0037-full-verify-import-approval-revocation.json` (`fixture`, `PASS`).

| Perintah | Hasil |
| --- | --- |
| `cargo test -p stackly-core --test catalog_local` | 16 lulus, termasuk `a_full_disk_fails_the_import_with_no_space_and_leaves_no_row_folder_or_refresh`, `a_full_disk_during_a_repair_keeps_the_store_folder_that_was_there`, `an_import_the_commit_guard_refuses_at_the_limit_removes_the_folder_it_stored` |
| `cargo test -p stacklyd --test provider_import --test provider_trust --test provider_gate` | 14, 4, dan 19 lulus, termasuk `eight_imports_of_the_same_bytes_at_once_write_the_row_only_once` dan tiga test replay |
| `cargo test -p stackly-cli --test provider` | 34 lulus |
| `cargo test -p stackly-core --test runtime_install` | 12 lulus |
| `cargo test -p stackly-storage`, `-p stackly-domain`, `-p stackly-protocol` | 161, 338, dan 405 lulus |
| `cargo test -p stackly-core` (seluruhnya) | 1587 lulus, 0 gagal, 1 diabaikan (46 binary test) |
| `cargo test -p stacklyd` (seluruhnya) | 718 lulus, 0 gagal, 107 diabaikan (test `native` fitur lain yang butuh artefak runtime atau launchd) |
| `cargo test -p stackly-cli` (seluruhnya) | 618 lulus, 0 gagal, 1 diabaikan |
| `swift test --package-path clients/swift` | 281 test di 41 suite lulus |
| `swift test --package-path apps/macos/Packages/StacklyUI --filter "ProviderTrustCopyTests\|ProviderImportModelTests\|CatalogStoreTests\|SettingsAvailabilityTests\|OnboardingCopyTests\|CatalogTrustContentTests\|CatalogLocalSnapshotTests\|LocalProviderEndToEndTests"` | 109 test di 9 suite lulus, termasuk snapshot `catalog-local-catalog-revoked` yang baru dan `LocalProviderEndToEndTests` terhadap `stacklyd` nyata |
| `cargo clippy -p stackly-core -p stacklyd -p stackly-cli --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps` | bersih |
| `cargo xtask codegen --check`, `cargo xtask catalog check`, `cargo xtask provider check`, `cargo xtask evidence check` | 20 file sama; `ok catalog revision 11 7f4ebd23…`; lima baris `ok` dengan digest yang sama; 70 record manual, baseline valid |
| `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` | lulus |

**Empat hal yang berubah sesudah milestone, pada daemon nyata:**

1. **Import yang gagal karena disk penuh (`3a0041b5`).** Seluruh `STACKLY_ROOT` diletakkan di disk image APFS 96 MB yang dipasang di scratchpad sesi, lalu volume itu diisi sampai penuh, jadi `ENOSPC` nya nyata.
   - Import paket baru (`acme/bigc`, `versions.json` 1 MB) menjawab `provider.store_failed` reason `no_space` dengan pesan tetap `the provider package could not be stored`. Tidak ada folder `<data>/provider-imports/<digest>` yang tersisa, tidak ada baris `provider_imports`, tidak ada `catalog.changed`, dan item store di `operation_created` berakhir `compensated`.
   - Import ulang byte yang sama untuk entri yang salinan store nya sudah diubah satu byte (folder sudah ada) juga menjawab `no_space`. Folder itu tetap ada dengan isi yang sama, baris dan keputusan `approved` nya tidak berubah, dan item store nya `not_owned` (`store_failed`).
   - Baris `WARN` daemon hanya memuat `provider` dan `reason` (sebelumnya hanya dibaca di diff).
   - Sesudah volume dikosongkan, kunci baru memperbaiki salinan itu dengan approval tetap `approved` dan mengimpor `acme/bigc`.
   - Jalur pin tidak berubah: diff `3a0041b5` dan `c716d173` hanya menyentuh `crates/core/src/catalog/import.rs`, dan di root biasa create dari entri `approved` menulis salinan pinned 0700 dan 0600 yang sama byte dengan paketnya.
2. **Guard import yang tidak menulis (`c716d173`).** Delapan `provider.import` atas byte yang sama dikirim serentak di delapan koneksi dengan delapan kunci: delapan balasan `ProviderDetail`, satu baris `provider_imports` ber `revision` 1, satu folder store, delapan operasi `succeeded`, delapan baris audit (masing masing dengan `provider` dan digest), dan delapan event `catalog.changed` `imported` (satu per operasi, seperti bunyi *API surface*).
3. **Replay opsi C.** Semua cocok dengan teks *Request yang diulang*:
   - approve, revoke, dan import yang diulang dengan kunci pertamanya sesudah revoke menjawab detail saat ini (`approval` `revoked`), tanpa operasi, baris audit, atau event kedua;
   - import yang diulang sesudah folder asal dipindah tetap menjawab detail (kunci baru menjawab `provider.import_unreadable`);
   - sesudah remove, ketiganya menjawab `provider.import_not_found` reason `not_imported`, dan remove yang diulang menjawab `RemovedProvider` yang sama;
   - import yang operasi pertamanya gagal (`provider.store_failed` di disk penuh) menjawab error yang sama sesudah disk dikosongkan, tanpa operasi baru;
   - approve yang dijawab tanpa operasi (sudah `approved`) tidak mengikat kuncinya: diulang sesudah revoke ia berjalan sebagai request baru;
   - kunci yang sama dengan params lain ditolak `operation.idempotency_key_reused`.
   - Tidak diprobe: request ulang yang menunggu operasi pertama yang masih berjalan (operasi provider selesai dalam milidetik).
4. **Celah trust tetap tertutup.**
   - `pending`: `runtime.resolve`, `runtime.install`, form dengan provider, dan create menjawab `provider.approval_required`; form tanpa provider menjawab `instance.provider_ambiguous` dengan `candidates` dan pesan baru. Tidak ada operasi, instance, runtime, atau proses anak daemon.
   - `revoked` oleh user dan sesudah remove: semua titik itu, ditambah form dan create dari revision pinned serta form dan create duplicate, menjawab `provider.revoked`.
   - Pin asal lokal tanpa approval (kolom `provider_revisions.approval` dikosongkan di registry root sementara saat daemon mati, meniru root dev sebelum gate ada): form, create, dan duplicate menjawab `provider.approval_required`.
   - Dicabut catalog (revision 12): approve dan semua titik gate menjawab `provider.revoked` by `catalog` dengan `catalogRevision` 12 dan `since` 2026-11-01.
   - Di setiap keadaan itu instance yang sudah ada tetap bisa `start`, `restart`, dan `stop`. Itu memang bunyi spec: gate hanya menutup install dan instance baru.

**Verdict per AC:**

| AC | Vonis | Bukti |
| --- | --- | --- |
| AC-1 preview | terpenuhi | Probe: preview `acme/fakeish` mengisi identitas, digest, izin, `quarantined`, dan `verdict`, tanpa operasi, baris audit, atau folder store; paket ber namespace `official` tetap menghasilkan preview dengan `importable` false; folder symlink dan folder yang tidak ada menjadi `provider.import_unreadable`. Test `catalog_local` dan `provider_import` untuk port dan `FakePlatform`. |
| AC-2 aturan import | terpenuhi | Probe: `namespace_not_allowed`, path relatif dan path ber `..` ditolak `request.invalid_params`. Batas 32 dan 16 MiB, URL `http`, kategori `testing`: test `the_33rd_local_provider_…` dan `catalog_local`. |
| AC-3 import | terpenuhi | Probe: digest lain `provider.import_changed`; store 0700 dan 0600; entri `local` `pending`; byte sama tanpa operasi, event, atau baris audit; entri `invalid` diperbaiki import ulang dengan keputusan utuh; delapan import serentak satu baris `revision` 1; disk penuh tanpa folder sisa; folder asal sama byte, mode, dan atributnya. |
| AC-4 belum disetujui tidak berjalan | terpenuhi | Probe gate `pending` (lihat butir 4): tanpa operasi, instance, runtime, atau proses anak. Entri ber `state` `blocked` dengan `approval_required` atau `trust_revoked`, dan kolom install di inspect menulis `no (trust_revoked)`. Cek ulang di pekerjaan install: `runtime_install` (lihat catatan cakupan). |
| AC-5 approval | terpenuhi | Probe: approve membuka `runtime.resolve` dan create; approve kedua tanpa operasi; digest salah `provider.import_changed`; entri yang dicabut catalog `provider.revoked`; baris pin kembali `approved` hanya untuk kunci yang tersimpan. |
| AC-6 perubahan butuh approval ulang | terpenuhi | Probe: revision 0.2.0 menjadi `pending` dengan `changes` `revision`, `download_hosts`, `endpoints` di preview, detail, dan baris audit; baris pin lama dan instance tidak disentuh; byte pertama kembali `approved`. Kode `executables`, `downloads`, `content`: test core dan cek milestone 2. |
| AC-7 revoke | terpenuhi | Probe: baris `runtimes`, `instances`, `artifacts` dan folder instance, runtime, pinned, serta store sama sebelum dan sesudah; semua gate tertutup termasuk pin; instance lama start, restart, stop; revoke kedua `provider.not_approved` (exit 3); approve lagi berhasil. |
| AC-8 revocation catalog | terpenuhi | Probe catalog revision 12: `provider.revoked` by `catalog` dengan `catalogRevision` dan `since`, approve ditolak, tidak ada yang dihapus, instance lama start; `stackly provider list` menulis `Revoked by this catalog (revision 12, published 2026-11-11)`; tidak ada teks `latest` atau `up to date`. Tidak ada kode jaringan (`check-deps`). Langkah jaringan mati `NOT_RUN`, owner user. |
| AC-9 remove | terpenuhi | Probe: entri hilang, baris pin `approved` menjadi `revoked` di langkah yang sama, folder store hilang, folder lain dan folder asal sama, instance lama start, create dan duplicate dari pin ditolak, approve sesudah remove exit 10. |
| AC-10 read model dan event | terpenuhi | Probe: satu `catalog.changed` per operasi, sebelum operasi selesai, dan tidak ada untuk import yang gagal; restart memuat entri dan keputusannya tanpa operasi atau baris audit; folder store yang dihapus menjadi entri `invalid` `local` (`missing_file`), salinan yang diubah menjadi `import_digest_mismatch`; entri `invalid` ditolak gate (`catalog.not_installable`) dan keduanya diperbaiki import ulang byte yang sama dengan keputusan utuh. Recovery mode, generation, dan anggaran frame: test `provider_import`. |
| AC-11 label jujur | terpenuhi | Probe: `trust` `local`, `Publisher acme (not verified)`, sel host `unverified (untested)`; CLI di pty mencetak kalimat izin persis seperti `approval-statement.txt`; 47 keluaran CLI tanpa frasa terlarang. `ProviderTrustCopyTests` dan golden CLI lulus. |
| AC-12 VAL-018 | terpenuhi | Probe: daemon dengan `STACKLY_TRUST=1` dan `STACKLY_APPROVE_ALL=1` tetap memberi `pending`; `settings.update` dengan kunci `trust` ditolak `request.invalid_params`. `provider_trust` lulus. |
| AC-13 quarantine | terpenuhi | Probe: `provider.yaml` asal diberi `com.apple.quarantine` dengan `xattr -w`; preview melaporkan `quarantined` true dan agen `Safari`; import berhasil; atribut asal tetap; salinan store tanpa atribut itu (hanya `com.apple.provenance` yang dipasang macOS sendiri). Tidak ada teks tentang Gatekeeper atau menghapus quarantine. Unduhan browser nyata dan folder terlindung `NOT_RUN`, owner user. |
| AC-14 CLI | terpenuhi | Probe: `import --dry-run` (0 dan 2), `import --json`, `import --approve` tanpa `--yes` (2, tidak ada yang diimpor), `import --approve --yes`, `approve`, `revoke`, `remove` tanpa `--yes` (2) dan dengan `--yes --json` (DTO yang sama dengan socket), `Already imported.`, exit 3, 4, dan 10 sesuai kode. `cargo test -p stackly-cli --test provider` 34 lulus. |
| AC-15 GUI Catalog | terpenuhi untuk bagian otomatis | `ProviderImportModelTests`, `CatalogStoreTests`, `CatalogLocalSnapshotTests` lulus terhadap referensi. Alur GUI dengan mata `NOT_RUN`, owner user. |
| AC-16 Settings dan onboarding | terpenuhi untuk bagian otomatis | `SettingsAvailabilityTests`, `OnboardingCopyTests`, `CatalogTrustContentTests` lulus. Tinjauan mata `NOT_RUN`, owner user. |
| AC-17 jejak | terpenuhi, satu temuan rendah | Probe: setiap operasi yang berhasil punya satu baris audit dengan `provider`, `providerRevision`, dua digest, dan `changes`; preview tanpa baris; `audit.list` menampilkan keempat jenis operasi; home ditulis `~` di preview, error, detail, dan tidak muncul di audit atau log. Temuan 1 di bawah untuk operasi yang gagal. |
| AC-18 kontrak dan storage | terpenuhi | `codegen --check` bersih pada protokol 4.1; lima method `provider.*` bertanda `recovery refused`; migrasi `0013_provider_imports.sql`; dua kolom di `CODED_COLUMNS`; test protocol, storage, dan `clients/swift` lulus. |
| AC-19 create dan pin | terpenuhi | Probe: create dari entri `approved` menulis `provider_revisions` dengan publisher `acme`, `approval` `approved`, `approved_at` terisi, dan `source_id` baris sumber `local` `imports`; satu satunya entri lokal yang belum `Trusted` menjawab `instance.provider_ambiguous` dengan `candidates`. |

**Done when, kalimat demi kalimat:**

1. *Provider lokal yang belum dipercaya bisa diperiksa, tapi executable dan init tidak berjalan*: terpenuhi. Preview, list, dan inspect menampilkan izin dan digest; setiap jalur install dan create ditolak, dan daemon tidak punya proses anak.
2. *Revision baru yang meminta izin atau executable berbeda menunggu approval ulang*: terpenuhi. Revision 0.2.0 dengan host unduhan dan port lain menjadi `pending` dan gate menutup lagi.
3. *Revocation tidak menghapus runtime atau data, saat offline tidak menghalangi start, dan tidak diklaim terkini*: terpenuhi untuk yang bisa dijalankan. Revoke, remove, dan revocation catalog tidak mengubah satu baris atau folder pun, instance lama tetap start, dan teksnya hanya menyebut revision serta tanggal terbit catalog. Fitur ini tidak punya kode jaringan; langkah dengan jaringan benar benar mati `NOT_RUN`, owner user.
4. *Quarantine ditangani kebijakan fitur 3 tanpa meminta pengguna mematikannya*: terpenuhi. Stackly hanya membaca dan melaporkan atribut itu, dan tidak ada teks yang menyuruh mematikan proteksi.
5. *Dialog izin menyatakan native executable berjalan dengan hak user*: terpenuhi. CLI mencetak kalimat fixture persis; sheet GUI dipegang `ProviderTrustCopyTests` dan snapshot `catalog-approve-sheet`.
6. *`stackly provider import` setara dengan GUI*: terpenuhi. CLI dan model app memanggil method yang sama dan menerima DTO yang sama (`LocalProviderEndToEndTests` terhadap `stacklyd` nyata).

**Temuan, berperingkat:**

1. **Rendah.** Baris audit `import_provider` yang gagal di langkah `read` atau `store` tidak menyebut provider. Di probe disk penuh `safe_details` nya hanya `errorCode`, `message`, `reason`, `stage`, dan `state`. Sebabnya `set_audit_note` baru dipanggil di langkah `commit` (`crates/core/src/catalog/import.rs:613`), padahal provider, revision, dan digest sudah diketahui sesudah langkah `read`, dan dokumen `OperationContext::set_audit_note` meminta note dipasang sedini mungkin supaya baris kegagalan ikut membawanya. Operasi yang gagal tidak mengubah keputusan trust, jadi jejak keputusan AC-17 utuh. Usul: pasang note tanpa `changes` di akhir langkah `read`, lalu ganti di langkah `commit`.
2. **Rendah, cakupan test** (sudah dicatat tester). Cek ulang trust di pekerjaan `install_runtime` hanya diuji dengan baris import yang tidak ada (`provider.approval_required`). Verdict `revoked` dan revoke yang masuk di antara exec check dan commit hanya dibaca di kode (`recheck_local` dan `final_batch` di `crates/core/src/runtime/install.rs`), dan kodenya menangani keduanya.
3. **Info.** Tujuh dari delapan import serentak tidak mengubah data tetapi tetap menerbitkan `catalog.changed`. Itu bunyi *API surface* (refresh tetap dipanggil), jadi client memuat ulang list delapan kali. Tidak perlu tindakan.

**Tetap `NOT_RUN`, owner user** (tidak dijalankan sesi; tidak menghalangi PASS): empat record `native` di `evidence/manual/` yang dibuat Langkah 13 (folder terlindung, quarantine dari unduhan browser, alur GUI lengkap, jaringan mati) dan langkah `### UI / manual` Milestone 1, 2, dan 4 di atas.

### Acceptance-criteria coverage

- AC-1 sampai AC-14, AC-17, AC-18, AC-19 · terpenuhi, dibuktikan test dan probe daemon nyata
- AC-15, AC-16 · bagian otomatis terpenuhi; tinjauan GUI dengan mata `NOT_RUN`, owner user
- AC-8, AC-13 · terpenuhi; bukti `native` jaringan mati, folder terlindung, dan quarantine unduhan browser `NOT_RUN`, owner user
- Done when fitur 37 · keenam kalimat terpenuhi
