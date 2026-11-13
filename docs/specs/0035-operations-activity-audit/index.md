# 0035. Operations, activity, dan audit lokal

**Date**: 2026-11-10
**Status**: Accepted

## Summary

Spec ini membuat jejak setiap perubahan bisa kamu lihat dan percayai. Mesin operasi (spec 0007) menulis satu baris audit di transaksi yang sama setiap kali operasi selesai, gagal, dibatalkan, atau perlu pemulihan, plus satu baris saat kamu meminta cancel, resume, atau abandon. Barisnya memuat siapa (user atau daemon, lewat CLI atau app), operation ID sebagai correlation ID (penanda yang menyambungkan baris audit, log, dan error dari satu kejadian), instance, config revision, dan hasilnya, tanpa nilai secret. Di atasnya dibangun halaman Operations (daftar operasi dengan tahap, progress, Cancel yang mati di langkah yang tidak bisa dibatalkan dengan aman, Retry, Resume, Abandon), tab Activity per instance, perintah `stackly audit list`, dan setting lama simpan audit (7, 30, atau 90 hari).

## Requirements

**User stories**:

- Sebagai pengguna yang start instance lalu gagal, saya ingin melihat operasi itu, tahap tempat ia berhenti, kode error, dan tombol Retry, supaya saya bisa mencoba lagi tanpa menebak.
- Sebagai pengguna yang membatalkan operasi, saya ingin tahu sebelum menekan Cancel apakah langkah sekarang masih bisa dibatalkan dengan aman, dan sesudahnya apakah operasi berhenti bersih, terus sampai akhir yang aman, atau perlu pemulihan.
- Sebagai pengguna yang kembali ke instance setelah beberapa hari, saya ingin tab Activity yang menyebut siapa melakukan apa dan kapan (create, start, ganti port, lihat password), tanpa pernah menampilkan password.
- Sebagai pengguna CLI, saya ingin `stackly audit list` dengan JSON, supaya saya bisa mengekspor dan mencari jejak itu sendiri.
- Sebagai pengguna, saya ingin memilih berapa lama audit disimpan.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit):

- **AC-1** (REQ-045): Setiap commit engine yang membawa operasi ke `succeeded`, `failed`, atau `cancelled` memuat tepat satu baris `audit_events` dengan action `operation.finished` di batch yang sama, dan setiap commit yang membawanya ke `needs_recovery` memuat tepat satu baris `operation.needs_recovery`. Ini berlaku di semua jalur di *Baris audit operasi* (finalize, gagal sebelum mulai, cancel sebelum mulai, kompensasi selesai, jawaban `NoOp` yang menulis batch, pemulihan saat start, shutdown, abandon). Baris memuat `actor` (`user` untuk origin `user`, `daemon` untuk `background`), `client` dari `trigger.client`, `operation_id` sama dengan ID operasi, `operation_kind`, `resource` operasi (kosong untuk manager), `instance_id` bila resource nya instance, `config_revision` dari catatan audit pekerjaan bila ada, `result` (`succeeded`, `failed`, `cancelled`, atau `needs_recovery`), dan `safe_details` di *Kunci `safe_details`*. Commit yang gagal tidak meninggalkan baris audit, dan commit ulang engine menulisnya bersama state akhir. Test core memeriksa setiap jalur di tabel itu dengan `ScriptedWork` di `FakeRegistry`, dan di `SqliteRegistry` lewat daemon serta conformance. *Diperbarui 2026-11-10 (koordinasi scope 05):* `SqliteRegistry` diuji lewat daemon dan `registry_conformance`, karena `cargo xtask check-deps` melarang `stackly-core` memakai `stackly-storage`.
- **AC-2** (AC-045): Lewat socket dengan provider fake di `TestRoot`: `instance.create`, `instance.start`, dan `instance.change_endpoints` (ganti port), masing masing sukses dan gagal (start gagal di dalam operasi karena port dipegang listener test, `supervisor.endpoint_in_use`; create gagal di langkah `init` lewat setting fake `failInit`), lalu `audit.list { instanceId }` membalas baris `operation.finished` yang `operationId` nya sama dengan `operationId` di `OperationAccepted` dan di `failure.error` operasi itu, `result` sesuai state akhir, `errorCode` sama dengan `failure.error.code` untuk yang gagal, `configRevision` 1 untuk create, nomor revision aktif yang dirender untuk start, dan revision baru plus `oldRevision` dan `ports` (`main 5432 → 5433`) untuk ganti port. Instance dibuat dengan `FixtureCredential` sebagai password: pindaian byte file registry (termasuk WAL), balasan `audit.list` mentah, dan log daemon tidak memuat nilai sentinel. Assertion `configRevision` dan detail berlaku untuk operasi yang ditutup di run daemon yang sama; operasi yang ditutup `recover` setelah restart punya baris tanpa catatan (K5). Purge (fitur 54) tercakup aturan AC-1 dan catatan audit dari spec pemiliknya; bagian AC-045 untuk purge tetap `NOT_RUN` sampai fitur 54.
- **AC-3** (Follow-up spec 0007): `operation.cancel` yang meng commit perubahan (disposisi `cancelled_before_start`, `requested`, atau `continuing_past_point_of_no_return` yang pertama) menulis satu baris `operation.cancel` berhasil dengan `disposition`, di batch yang sama dengan commit itu. `operation.resume` yang berhasil menulis `operation.resume`, dan `operation.abandon` yang berhasil menulis `operation.abandon` dengan `leftovers` (jumlah), di samping baris `operation.finished` penutupnya. Actor dan client baris itu milik pengirim permintaan (dari handshake koneksi nya), bukan milik pemicu operasi: operasi yang dimulai GUI lalu dibatalkan CLI punya baris `operation.cancel` dengan client `cli`. Jawaban yang tidak mengubah apa pun (`already_final`, `recovery_required`, cancel kedua, `operation.not_resumable`, `operation.not_abandonable`) tidak menulis baris audit.
- **AC-4** (Follow-up spec 0011): Setiap nilai `safe_details` yang ditulis engine dan catatan audit pekerjaan lewat `Redactor` daemon sebelum dipotong ke 512 byte di batas karakter, kunci lebih dari 20 dibuang dari belakang dengan `omitted` berisi jumlahnya, dan baris audit yang dibangun tidak pernah membuat commit state gagal. Test sentinel: pekerjaan yang gagal dengan pesan berisi nilai `FixtureCredential`, dan catatan audit yang detail nya berisi nilai itu, menghasilkan baris tanpa nilai tersebut; nilai 600 byte yang memuat sentinel melintasi byte ke 512 tetap tersensor (redaksi dulu, potong kemudian).
- **AC-5** (REQ-045): `audit.list { instanceId?, operationId?, limit?, before? }` (`recovery refused`, tanpa idempotency key) membalas `AuditPage { items, next }` urut `(occurredAt, id)` turun, `limit` 1 sampai 200 (default 50, di luar itu `request.invalid_params`), filter `instanceId` dan `operationId` bisa digabung (AND), cursor `before` opaque yang ditulis daemon (tidak valid: `request.invalid_params` dengan field `before`), `before` eksklusif (baris cursor itu sendiri tidak ikut), daemon membaca `limit + 1` baris dan mengisi `next` hanya bila baris tambahan itu ada, sehingga halaman terakhir membawa `next` null, dan halaman berhenti sebelum JSON nya melewati 900 KiB dengan `next` menunjuk baris terakhir yang masuk. Instance yang sudah dihapus tetap bisa dicari dengan UUID nya. Baris secret milik instance (spec 0011) ikut muncul dengan filter `instanceId`.
- **AC-6** (REQ-045, PRD 35.2): `DaemonSettingsView.auditRetention { days, maxRows }` menampilkan setting `audit_retention_days` (7, 30, atau 90; default 30) dan batas baris 100.000 dari konstanta. `settings.update` dengan `auditRetentionDays` lain ditolak `request.invalid_params`. Setiap prune memakai `RetentionPolicy` yang `audit_max_age` nya dari setting itu (umur operasi, lease, dan observasi tidak berubah), dan commit yang mengubah setting itu memanggil `Notify` milik loop prune (`DaemonCore::wake_prune`, setelah commit berhasil) sehingga baris yang lebih tua dari batas baru hilang dalam 10 detik tanpa restart daemon. `stackly settings set audit-retention-days <7|30|90>` dan baris "Keep activity for" di tab Storage Settings memakai setting yang sama.
- **AC-7**: Migrasi `NNNN_audit_operation_fields.sql` (nomor bebas berikutnya, ditetapkan lead saat build; sementara `0012`) menambah kolom `audit_events.instance_id`, `operation_kind`, `config_revision`, dan `client` dengan CHECK di *Model data*, index parsial `audit_events_instance`, dan kolom `daemon_settings.audit_retention_days`, lewat `ALTER TABLE ... ADD COLUMN` tanpa membangun ulang tabel. Migrasi maju dari fixture beku versi sebelumnya mempertahankan semua baris (baris audit lama mendapat NULL, setting mendapat 30), trigger `audit_events_append_only` tetap menolak `UPDATE`, fixture beku versi baru ditambahkan, dan `registry_conformance` mencakup filter `AuditQuery` dan kolom baru di `FakeRegistry` dan `SqliteRegistry`.
- **AC-8** (REQ-054, PAGE-12): `OperationSummary.cancel` (opsional) bernilai menurut tabel *Ketersediaan cancel*: `available`, `past_point_of_no_return`, `requested`, `continuing`, atau `none`, dihitung satu fungsi murni domain dari state, `pointOfNoReturnAt`, dan `interrupt_policy` jenisnya, dan dikirim di setiap `OperationSummary` (reply dan event). Operasi `start_instance` yang sudah lewat `spawn` bernilai `available` (jenis `CloseInterrupted` selalu menerima cancel), sedangkan `install_runtime` di tahap `commit` bernilai `past_point_of_no_return`. Test domain menelusuri setiap `OperationKind` dengan dan tanpa titik tak bisa dibalik untuk setiap state. Klien yang tidak menerima field ini (daemon lama) memakai aturan cadangan di *Ketersediaan cancel*.
- **AC-9** (AC-054, REQ-054): Lewat socket memakai method khusus test `Handlers::test_starting_method` dengan pekerjaan berlangkah staging lalu commit yang tidak bisa dibalik (pola fixture restore palsu spec 0007 AC-7): cancel saat staging membuat operasi `cancelled`, file sentinel target identik, dan folder staging hilang; `cancel` di summary bernilai `past_point_of_no_return` sejak langkah commit mulai; cancel setelah itu mendapat disposisi `continuing_past_point_of_no_return`, `cancel` menjadi `continuing`, dan operasi berakhir `succeeded` dengan `cancelRequestedAt` terisi, atau `needs_recovery` bila langkah commit dibuat gagal, tidak pernah `cancelled`. Halaman Operations menulis "Finishing safely" untuk `continuing` dan "Needs Recovery" untuk `needs_recovery`, dan `stackly operation cancel` menulis kalimat disposisi di *CLI*, tidak pernah melaporkan sukses palsu.
- **AC-10** (REQ-054, PAGE-12, JOURNEY-05): Resume hanya tampil (GUI) dan hanya disarankan (hint CLI) untuk operasi `needs_recovery` yang `recovery.actions` nya memuat `resume`; Abandon untuk yang memuat `abandon`, selalu lewat dialog konfirmasi yang menyebut bahwa item yang tidak bisa dibalik dibiarkan, dan selalu dengan `expectedRevision` dari summary yang ditampilkan (`resource.revision_mismatch` memuat ulang operasi dan menampilkan kartu umpan balik). Retry tampil untuk operasi `failed` berjenis `start_instance`, `stop_instance`, `restart_instance`, dan `reload_instance` bila instancenya masih ada di `InstanceStore`, operasi itu yang terbaru di antara keempat jenis itu untuk instance tersebut di daftar yang termuat, dan `InstanceLifecycleActions` (spec 0026) mengizinkan aksi itu untuk status instance sekarang. Retry mengirim `instance.start`, `instance.stop` (stop biasa, bukan force), `instance.restart`, atau `instance.reload` dengan params bawaan yang sama dengan tombol toolbar instance, sebagai operasi baru dengan idempotency key baru (sama dengan Retry banner spec 0033); jenis lain tidak punya Retry di halaman ini.
- **AC-11** (PAGE-12): Halaman Operations menggantikan `UnbuiltPage(.operations)` dan menampilkan operasi aktif dari `OperationCenter` plus riwayat dari `operation.list` (50 per halaman, Show More dengan `before`), urut `createdAt` turun, diperbarui event tanpa memuat ulang halaman. Setiap baris memuat judul `OperationTitle`, baris catatan menurut *Baris catatan*, waktu mulai dan delapan karakter pertama ID, `ToneBadge(StatusAppearance.operation(...))`, Cancel untuk `cancel` `available` (aktif) atau `past_point_of_no_return` (nonaktif, dengan catatan "can’t be cancelled"), Retry menurut AC-10, dan `StagedProgress` untuk operasi berjalan yang jenisnya punya definisi tahap (selain itu label tahap dengan progress indeterminate). Placeholder: tanpa operasi "No Operations", gagal dimuat "Couldn’t Load Operations" dengan Retry (Follow-up spec 0022), daemon lama (`OperationCenter.store.unsupported`) "Operations Unavailable", dan saat offline data terakhir tetap tampil dengan semua tombol mutasi nonaktif (aturan shell spec 0013).
- **AC-12** (PAGE-12): Memilih baris membuka inspector detail yang membaca `operation.get`: jenis, resource, actor dan client, waktu dibuat, mulai, dan diperbarui, ID lengkap dengan Copy, langkah (nama, state, percobaan, waktu, kode error), item yang dibuat dan sisanya, `failure` (kode, pesan, hint, `items`), `recovery` (cause, hint, aksi), hasil `partial`, dan tombol Cancel, Retry, Resume, Abandon menurut AC-8 dan AC-10. Operasi yang tidak dikenal lagi (`resource.not_found`, sudah di prune) menampilkan "This operation is no longer in the history." tanpa error lain.
- **AC-13** (JOURNEY-05): `ShellNavigator.openOperation(_:)` membuka halaman Operations dengan operasi itu terpilih dan inspector terbuka, juga untuk operasi yang tidak ada di halaman riwayat yang termuat (dibaca lewat `operation.get` dan disisipkan di urutannya). Baris seksi Operations di Overview, item O1, baris Activity yang membawa operation ID, banner spec 0033, dan temuan doctor spec 0034 memakai fungsi ini. Isi O1 `// provisional: F35` diganti teks di *Navigasi* yang menunjuk halaman Operations, bukan perintah CLI.
- **AC-14** (PAGE-12, Follow-up spec 0067): `OperationStages.definitions(for:)` memberi definisi tahap untuk `start_instance`, `stop_instance`, `restart_instance`, `force_stop_instance`, `apply_config`, `change_endpoints`, dan `resolve_config_drift` menurut *Definisi tahap*, dengan `StageDefinition.also` (kunci tambahan yang dihitung sebagai tahap itu, masing masing dengan judulnya sendiri), sehingga `backoff` (spec 0033) tampil "Waiting to restart" di tahap Launch tanpa bar mundur. Test Swift per jenis memeriksa setiap kunci tahap yang dimasuki pekerjaannya (daftar di *Definisi tahap*) memetakan ke indeks yang benar, dan kunci asing memakai label cadangan.
- **AC-15** (REQ-045, PAGE-12): Tab Activity (`InstanceTab.activity`, sesudah Metrics) menampilkan baris `audit.list { instanceId }` di `Table` native tiga kolom (Time, By, Activity) menurut *Teks Activity*, 50 per halaman dengan Show More, memuat ulang halaman pertama saat tab tampil, saat event `operation.state_changed` apa pun untuk operasi berresource instance ini datang (dikumpulkan 500 ms; event terbit setelah commit, spec 0007), dan setiap `ShellSync.completedRounds` naik. Baris yang membawa operation ID membuka operasi itu (AC-13). Placeholder: kosong "No Activity Yet", gagal "Couldn’t Load Activity" dengan Retry, daemon lama (`request.unknown_method`, pola `ResourceStore` spec 0013) "Activity Unavailable". Tidak ada nilai secret yang bisa tampil karena daemon tidak pernah mengirimnya.
- **AC-16** (REQ-045): `stackly audit list [--instance <name|uuid>] [--operation <uuid>] [--limit <n>] [--before <cursor>] [--json]` memakai `TargetArg` untuk `--instance` (nama yang tidak dikenal keluar 10 dengan `cli.instance_not_found`, UUID instance yang sudah dihapus diteruskan apa adanya), menulis tabel TIME, BY, ACTION, RESULT, OPERATION, DETAILS dan footer `next` seperti `stackly operation list`, dan `--json` menulis envelope dengan `AuditPage`. `stackly operation inspect` dan `list` menampilkan ketersediaan cancel, dan `stackly operation cancel` menulis kalimat disposisi di *CLI* (kalimat lama "the operation stops after it" yang keliru diganti).
- **AC-17**: Kontrak: `CancelAvailability`, `OperationSummary.cancel`, `audit.list` dan DTO nya, `DaemonSettingsView.auditRetention`, `SettingsPatch.auditRetentionDays`, dan `CreateInstanceParams.duplicateOf` punya fixture di `protocols/fixtures/` (termasuk nilai asing untuk setiap enum terbuka baru), test kontrak Swift mendecode semuanya, `cargo xtask codegen --check` bersih, `audit.list` tercatat `recovery refused` di tabel `methods!`, `cargo xtask fuzz seed` dijalankan, dan minor protokol naik satu (nomor ditetapkan lead saat build; sementara 3.4). Tidak ada `EventKind` baru.
- **AC-18** (Follow-up spec 0024): `instance.create` menerima `duplicateOf: Option<Uuid>` (sheet Duplicate spec 0024 mengirimnya). Bila instance itu ada saat `begin`, catatan audit create memuat `duplicateOf` (UUID) dan `duplicateOfName` (nama saat itu); bila tidak ada, field diabaikan tanpa error. Tab Activity menulis "Created from {duplicateOfName}" untuk baris itu, dan "Created" untuk create biasa.
- **AC-19** (PAGE-12): Fixture tampilan spec 0068 untuk halaman Operations (berjalan dengan tahap, Cancel nonaktif di commit, cancel `continuing`, gagal dengan Retry, `needs_recovery` dengan Resume dan Abandon, kosong, gagal dimuat, unsupported, offline, inspector terbuka) dan tab Activity (berisi baris dari setiap action, kosong, gagal, unsupported), masing masing light dan dark, tercatat di snapshot test dan bisa dibandingkan lewat `cargo xtask snapshot review`.
- **AC-20** (PAGE-12, PAGE-14): Tab Storage Settings mendapat grup "Activity" dengan picker native "Keep activity for" (7 days, 30 days, 90 days) dan keterangan "Up to {maxRows} entries. Older entries are removed every day and right after you change this.", memakai `settings.update` dengan `expectedRevision` dan perilaku konflik spec 0032. Memilih nilai yang lebih pendek dari sekarang membuka dialog konfirmasi native (Cancel sebagai default): "Remove activity older than {n} days?", "Older entries are removed now and can’t be brought back.", tombol "Remove".

## Decision

**Chosen option**: rekomendasi di setiap keputusan di [rationale.md](rationale.md) (keputusan K1 sampai K16): baris audit operasi ditulis engine di batch commit state yang sama, empat kolom baru di `audit_events` untuk instance, jenis operasi, config revision, dan client, ketersediaan cancel dihitung daemon dan dikirim di `OperationSummary`, satu method baca `audit.list` tanpa event baru, setting retensi audit 7, 30, atau 90 hari di `daemon_settings`, dan halaman Operations serta tab Activity di atas store yang sudah ada.

Engine operasi menjadi satu satunya penulis baris audit operasi, sehingga tidak ada operasi yang bisa berakhir tanpa jejak dan tidak ada fitur yang harus ingat menulisnya sendiri; GUI dan CLI hanya membaca.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swiftui-expert-skill` dan `swift-concurrency` (lihat `apps/macos/AGENTS.md`) · `swift-testing-pro`

Kalau isi skill bertentangan dengan spec ini, spec yang menang.

## Feature design

### Trace PRD

| ID PRD | AC spec ini |
| --- | --- |
| REQ-045 | AC-1, AC-3, AC-4, AC-5, AC-6, AC-7, AC-15, AC-16 |
| AC-045 | AC-2 (create, start, ganti port), AC-1 (purge lewat aturan umum, `NOT_RUN` sampai fitur 54) |
| REQ-054 | AC-8, AC-9, AC-10, AC-11 |
| AC-054 | AC-9 (dengan pekerjaan fixture; restore nyata milik fitur 51) |
| PAGE-12 | AC-8, AC-10 sampai AC-15, AC-19, AC-20 |
| JOURNEY-05 | AC-10 (Retry dan Resume), AC-13 (dari banner dan doctor ke operasi), AC-15 ("operation record") |

### Batas dengan spec lain

- **Spec 0007** (engine): spec ini menambah baris audit di batch engine, `OperationTrigger` sebagai pengirim untuk `cancel`, `resume`, dan `abandon`, catatan audit pekerjaan, dan ketersediaan cancel. State machine, disposisi, aturan hint, dan aksi pemulihan tidak berubah.
- **Spec 0033** (`arc-f33-spec`): banner gagal, Retry dari banner, backoff, dan crash loop milik spec itu. Spec ini memberi `openOperation`, Retry yang sama di halaman Operations, dan tahap `backoff` di definisi `start_instance`.
- **Spec 0034** (`arc-f34-spec`): doctor dan bundle bukan operasi dan tidak diaudit; doctor menampilkan operasi `needs_recovery` dan membuka halaman ini lewat `openOperation`. Audit tidak ikut bundle; jalur export audit adalah `stackly audit list --json`.
- **Fitur 51, 54** (restore, remove, purge): pekerjaannya memanggil `set_audit_note` dengan detail di *Catatan audit pekerjaan*; baris auditnya otomatis lewat AC-1.
- Interaction policy `AGENTS.md`: tanpa dukungan screen reader kustom, navigasi keyboard kustom, shortcut baru, atau identifier UI test. Kontrol native, menu, dan pengeditan teks native dipertahankan.

### Model data

Migrasi `crates/storage/migrations/NNNN_audit_operation_fields.sql` (nomor bebas berikutnya saat build; sementara `0012`). Jenis kolom bersama spec 0005 (`uuid`, `code`).

**`audit_events`** (tabel spec 0005, kolom lama tidak berubah), kolom baru:

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `instance_id` | `uuid` | tanpa FK, supaya riwayat tetap ada setelah instance dihapus |
| `operation_kind` | `code` | set terbuka kode `OperationKind`; terisi tepat untuk action `operation.*` |
| `config_revision` | `INTEGER` | `>= 1`; nomor config revision instance (bukan ID baris) |
| `client` | `code` | set terbuka (`cli`, `gui`, dan seterusnya), dari handshake |

CHECK kolom (ditulis aman untuk NULL, seperti migrasi 0004): `config_revision IS NULL OR (config_revision >= 1 AND instance_id IS NOT NULL)`. Index `CREATE INDEX audit_events_instance ON audit_events (instance_id, occurred_at, id) WHERE instance_id IS NOT NULL`. Index `operation_id` yang sudah ada dipakai filter operasi. Daemon lama menolak registry dengan versi schema yang lebih baru lewat recovery `schema_newer_than_app` spec 0005, jadi downgrade tidak membaca kolom ini.

**`daemon_settings`** kolom baru `audit_retention_days INTEGER NOT NULL DEFAULT 30 CHECK (audit_retention_days IN (7, 30, 90))`.

**Domain** (`stackly-domain`, Follow-up spec 0005):

- `AuditEventRecord` mendapat `instance: Option<InstanceId>`, `operation_kind: Option<OperationKind>`, `config_revision: Option<ConfigRevisionNumber>`, dan `client: Option<ClientCode>`, semuanya kosong lewat `AuditEventRecord::new`.
- `AuditResult` mendapat `Cancelled` (`cancelled`) dan `NeedsRecovery` (`needs_recovery`).
- `AuditResource` mendapat `Instance(InstanceId)` (`instance`), `Runtime(RuntimeId)` (`runtime`), dan `Environment(EnvironmentId)` (`environment`); resource manager tidak ditulis (`resource_id` wajib UUID).
- `AuditAction` konstanta `OPERATION_FINISHED`, `OPERATION_NEEDS_RECOVERY`, `OPERATION_CANCEL`, `OPERATION_RESUME`, `OPERATION_ABANDON`.
- `AuditQuery` mendapat `instance: Option<InstanceId>` dan `operation: Option<OperationId>`; batas `limit` tetap 1 sampai 500 di port (IPC membatasi 200).
- `DaemonSettings` mendapat `audit_retention: AuditRetentionDays` (enum `Days7`, `Days30`, `Days90`), `SettingsPatch` mendapat `audit_retention`, dan `RetentionPolicy::with_audit_days(days)` mengganti `audit_max_age` dari `DEFAULT`. Konstanta `AUDIT_MAX_ROWS` tetap 100.000.
- `CancelAvailability` (`Available`, `PastPointOfNoReturn`, `Requested`, `Continuing`, `None`) dan fungsi murni `Operation::cancel_availability(&self, point_of_no_return: bool) -> CancelAvailability` yang membaca `kind().interrupt_policy()`.

Rekor secret spec 0011: `SecretService::audit_row` mengisi `client` dari `caller.client` dan `instance` dari pemilik reference bila pemiliknya instance (teks reference `local:instance/<id>/...` dan kolom `owner` sama), tanpa mengubah `safe_details` yang sudah diuji spec 0011.

### Baris audit operasi

Satu fungsi core `operation::audit::row(view, event, origin, note, redactor, now) -> AuditEventRecord` membangun setiap baris, dan setiap jalur di bawah memakainya lewat satu helper yang menambah `WriteOp::InsertAuditEvent` ke batch commit state. Tidak ada jalur lain yang menulis state akhir atau `needs_recovery`.

| Jalur (spec 0007) | Action | Result | Origin baris |
| --- | --- | --- | --- |
| finalize sukses, termasuk sesudah cancel yang `continuing` | `operation.finished` | `succeeded` | trigger operasi |
| gagal sebelum titik tak bisa dibalik, kompensasi selesai (`step_failed`, `panicked`) | `operation.finished` | `failed` | trigger operasi |
| `fail` dari `queued` (`precondition_failed`, `recovery_required`, `interrupted` saat rebuild gagal) | `operation.finished` | `failed` | trigger operasi |
| jenis `CloseInterrupted` yang gagal setelah titik itu, atau ditutup `interrupted` oleh `recover` | `operation.finished` | `failed` | trigger operasi |
| cancel sebelum mulai (`cancelled_before_start`) | `operation.finished` dan `operation.cancel` | `cancelled` dan `succeeded` | trigger operasi; pengirim cancel |
| kompensasi setelah cancel selesai | `operation.finished` | `cancelled` | trigger operasi |
| cancel `requested` atau `continuing_past_point_of_no_return` pertama | `operation.cancel` | `succeeded` | pengirim cancel |
| masuk `needs_recovery` (kompensasi gagal, gagal setelah titik itu, terputus saat start) | `operation.needs_recovery` | `needs_recovery` | trigger operasi |
| kompensasi otomatis `auto_compensate` selesai | `operation.finished` | `failed` atau `cancelled` | trigger operasi |
| `operation.resume` | `operation.resume` | `succeeded` | pengirim resume |
| `operation.abandon` | `operation.abandon` dan `operation.finished` | `succeeded` dan `failed` | pengirim abandon; trigger operasi |
| shutdown (`daemon_stopping`) | `operation.finished` | `failed` | trigger operasi |
| `NoOp` dengan batch (baris operasi `succeeded` tanpa langkah) | `operation.finished` | `succeeded` | trigger operasi |

Cancel pada operasi `running` meng commit `cancel_requested` lewat tulisan `mark_cancel_requested` yang sudah ada di `scheduler::cancel`; baris `operation.cancel` ikut di tulisan itu. Baris dalam satu batch memakai satu `now` engine, dengan `occurred_at` naik 1 nanodetik per baris menurut urutan kolom Action di tabel (baris aksi pengirim dulu, lalu `operation.finished`), sehingga urutan `(occurredAt, id)` selalu sama. Resource `create_instance` adalah instance ID yang dialokasikan sebelum `begin` (spec 0020, `instanceId` di input journal), jadi create yang gagal tetap punya `instance_id`. Operasi yang menyentuh instance lewat resource manager (`stop_all_instances`) tidak muncul di Activity instance itu; ia ada di halaman Operations. Operasi resumable yang diparkir `queued` saat shutdown tidak menulis baris (belum berakhir). Jawaban `NoOp` tanpa batch tidak membuat operasi dan tidak diaudit.

API engine yang berubah: `cancel(id, by: OperationTrigger)`, `resume(id, expected, by: OperationTrigger)`, dan `abandon(id, expected, by: OperationTrigger)`. `stacklyd` membangun `OperationTrigger` dari handshake koneksi dengan fungsi yang sama dengan `RequestOrigin`. Test dan pemanggil internal memakai `OperationTrigger::background()`.

### Catatan audit pekerjaan

`OperationContext::set_audit_note(AuditNote)` (core) menyimpan catatan di memori operasi; panggilan berikutnya menggantinya. `AuditNote { config_revision: Option<ConfigRevisionNumber>, details: Vec<(DetailKey, String)> }`, paling banyak 12 entri. Catatan tidak masuk journal, jadi operasi yang ditutup `recover` setelah daemon restart punya baris tanpa catatan (correlation ID dan hasil tetap ada). Pekerjaan memanggilnya segera setelah nilainya diketahui, supaya baris kegagalan pun membawanya. `config_revision` hanya diisi dengan revision yang sudah ada di registry: start, restart, reload, dan drift memakai revision aktif yang dibaca (drift tidak menulis revision baru), sedangkan apply dan ganti port mengisinya setelah langkah `commit` (sebelum itu catatan hanya membawa `oldRevision`).

| Jenis | Kapan | `config_revision` | Detail |
| --- | --- | --- | --- |
| `create_instance` | setelah validasi draft | 1 | `provider` (nama), `runtimeId`, `duplicateOf` dan `duplicateOfName` (AC-18) |
| `start_instance` | setelah revision aktif dibaca, sebelum `render` | nomor revision aktif | `ports` (`<endpoint> <port>`, dipisah koma) setelah port dipilih |
| `restart_instance`, `reload_instance` | setelah revision aktif dibaca | nomor revision aktif | tidak ada |
| `apply_config`, `change_endpoints` | dari `ChangePlan` spec 0029: sebelum `commit` hanya `oldRevision`, setelah `commit` catatan penuh | `newRevision` | `oldRevision`, `settings` (nama, dipisah koma), `ports` (`<endpoint> <lama> → <baru>`, `auto` untuk port otomatis), `impact` |
| `resolve_config_drift` | setelah revision aktif dibaca, sebelum `render` | nomor revision aktif (file dikembalikan ke revision itu) | tidak ada |
| `rename_instance` | dari plan | tidak ada | `oldName`, `newName` |
| jenis lain | tidak ada catatan sampai pemiliknya menambahkannya | | |

Satu fungsi `ChangePlan::audit_note()` menggantikan pemakaian JSON `audit()` untuk catatan ini; input journal spec 0029 AC-20 tidak berubah. *Diperbarui 2026-11-10 (koordinasi scope 05):* baris drift dipisah dari `ChangePlan`, sesuai `supervisor/drift.rs`.

### Kunci `safe_details`

Engine menulis kunci ini lebih dulu, lalu detail catatan pekerjaan (kunci yang sama dari catatan dibuang):

| Kunci | Action | Isi |
| --- | --- | --- |
| `state` | `operation.finished`, `operation.needs_recovery` | state akhir operasi |
| `reason` | `operation.finished` gagal | `failure.reason` (`step_failed`, `interrupted`, `abandoned`, dan seterusnya) |
| `errorCode` | gagal, `needs_recovery` | `failure.error.code` atau `recovery.error.code` |
| `message` | gagal, `needs_recovery` | pesan error (maksimal 200 karakter, sudah diredaksi engine) |
| `stage` | gagal, dibatalkan, `needs_recovery` | stage terakhir |
| `cancelRequested` | `operation.finished` sukses | `true` bila cancel diminta setelah titik tak bisa dibalik |
| `cause`, `hint` | `operation.needs_recovery` | `recovery_cause`, `recovery_hint` |
| `disposition` | `operation.cancel` | kode `CancelDisposition` |
| `leftovers` | `operation.abandon` | jumlah item sisa |
| `omitted` | semua | jumlah kunci yang dibuang karena batas 20 |

Batas 20 kunci termasuk `omitted`, jadi paling banyak 19 kunci isi bila ada yang dibuang. Setiap nilai lewat `Redact::redact`, lalu dipotong ke 512 byte di batas karakter (`message` sudah paling banyak 200 karakter dari `FailureError`). `SafeDetails::new` yang tetap gagal (bug) diganti `SafeDetails` kosong dengan baris log error, dan commit state tetap berjalan.

### Ketersediaan cancel

| Kondisi (urut) | `cancel` |
| --- | --- |
| state `succeeded`, `failed`, `cancelled`, `needs_recovery` | `none` |
| state `cancel_requested`, titik tak bisa dibalik sudah lewat, jenis bukan `CloseInterrupted` | `continuing` |
| state `cancel_requested` | `requested` |
| state `queued` atau `running`, titik itu sudah lewat, jenis bukan `CloseInterrupted` | `past_point_of_no_return` |
| state `queued` atau `running` | `available` |

Aturan cadangan klien bila `cancel` kosong (daemon lama): `queued` atau `running` berarti `available`, `cancelRequested` berarti `requested`, selain itu `none`. Cadangan ini sengaja tidak membaca `pointOfNoReturnAt`, karena klien tidak tahu kebijakan jenis; cancel yang dikirim selalu aman, dan engine menjawab dengan disposisi yang benar, yang ditampilkan sebagai umpan balik. Nilai `unrecognized` dari daemon yang lebih baru diperlakukan sebagai `none`.

### Method IPC

Semua method butuh handshake di koneksi uid yang sama (spec 0006).

| Method | Input | Output | Error utama |
| --- | --- | --- | --- |
| `audit.list` (`recovery refused`) | `ListAuditParams { instanceId: Option<Uuid>, operationId: Option<Uuid>, limit: Option<u32>, before: Option<String> }` | `AuditPage { items: Vec<AuditEventInfo>, next: Option<String> }` | `request.invalid_params` (`limit`, `before`) |
| `settings.update` (spec 0032) | `SettingsPatch` ditambah `auditRetentionDays: Option<u32>` | `DaemonSettingsView` ditambah `auditRetention` | `request.invalid_params` bila bukan 7, 30, atau 90 |
| `instance.create` (spec 0020) | params ditambah `duplicateOf: Option<Uuid>` | tidak berubah | tidak berubah |

DTO baru di `stackly-protocol::audit` (camelCase):

- `AuditEventInfo { id: Uuid, occurredAt: Timestamp, actor: AuditActorKind, client: Option<ClientKind>, action: String, operationKind: Option<OperationKind>, operationId: Option<Uuid>, resource: Option<ResourceRef>, instanceId: Option<Uuid>, configRevision: Option<Count>, result: AuditResultKind, details: Vec<AuditDetail> }` dengan `AuditDetail { key: String, value: String }` urut kunci.
- `AuditActorKind` terbuka (`user`, `daemon`, `unrecognized`); `AuditResultKind` terbuka (`succeeded`, `failed`, `cancelled`, `needs_recovery`, `unrecognized`).
- `CancelAvailability` terbuka (`available`, `past_point_of_no_return`, `requested`, `continuing`, `none`, `unrecognized`); `OperationSummary.cancel: Option<CancelAvailability>` dengan `#[serde(default)]`.
- `AuditRetention { days: u32, maxRows: Count }`.
- Cursor `before` dan `next`: `<occurred_at 30 karakter>_<uuid>`, paling banyak 80 karakter, sama dengan cursor operasi.

Handler `audit.list` membaca registry di `spawn_blocking` (baca halaman kecil, sehingga antrian tulis satu thread spec 0005 tidak tertahan lama), memetakan record ke DTO di `stacklyd::ipc::audit`, dan berhenti mengisi sebelum 900 KiB. Kenaikan minor protokol satu kali untuk seluruh spec ini.

### CLI

| Perintah | Perilaku |
| --- | --- |
| `stackly audit list` | AC-16; DETAILS berisi `errorCode`, `configRevision`, lalu detail lain `key=value`. DETAILS dipotong ke sisa lebar `COLUMNS` (paling sedikit 24 kolom) hanya bila stdout adalah terminal dan env `COLUMNS` berisi bilangan 1 sampai 10000; selain itu tidak dipotong, karena spec 0008 memutuskan CLI tidak membaca lebar terminal lewat `ioctl` (pengecualian ini dicatat di spec 0008) |
| `stackly operation list` | kolom baru CANCEL (`yes`, `no`, `requested`, `finishing`, kosong) |
| `stackly operation inspect` | baris `Cancel` dengan kalimat tabel di bawah; tanpa baris bila cancel tidak mengubah apa pun |
| `stackly operation cancel` | kalimat disposisi di bawah |
| `stackly settings set audit-retention-days <7\|30\|90>` | AC-6; `settings get` menulis `audit retention: 30 days, up to 100000 entries` |

| Disposisi atau ketersediaan | Kalimat |
| --- | --- |
| `cancelled_before_start` | "it had not started yet, so nothing of it ran" |
| `requested` | "it stops at its next safe point and undoes what it did"; bila `pointOfNoReturnAt` operasi terisi (hanya jenis `CloseInterrupted` yang masih menerima cancel setelah titik itu): "it stops safely" |
| `continuing_past_point_of_no_return` | "it is past a step that can't be undone, so it continues to a safe end; check it with stackly operation inspect {id}" |
| `already_final` | hanya "operation {id} already ended (state {state})" |
| `recovery_required` | "operation {id} needs recovery (state {state})", lalu kalimat dari `recovery.actions` saja (AC-10): `resume` dan `abandon` "resume or abandon it instead", hanya `resume` "resume it instead", hanya `abandon` "abandon it instead", tanpa keduanya atau tanpa `recovery` "it offers no recovery action now" |
| `available` (inspect) | "possible now, " lalu kalimat `requested` di atas |
| `past_point_of_no_return` (inspect) | "not possible now, the current step can't be undone safely" |
| `requested` (inspect) | "requested, " lalu kalimat `requested` di atas |
| `continuing` (inspect) | "requested past a step that can't be undone, so it continues to a safe end" |

*Diperbarui 2026-11-10 (koordinasi scope 05):* baris `already_final`, kalimat `recovery_required` (termasuk "it offers no recovery action now", copy developer), baris inspect selain `past_point_of_no_return`, dan syarat `CloseInterrupted` lewat `pointOfNoReturnAt` diselaraskan dengan `apps/cli/src/commands/operation.rs`. Tiga disposisi pertama didahului baris "cancel requested for operation {id} (state {state})". Daemon lama tanpa `cancel`: `queued` dan `running` dianggap `available`, `cancel_requested` dianggap `requested`.

### Halaman Operations

`StacklyUI/Operations/OperationsPage.swift` menggantikan `UnbuiltPage(.operations)` di `AppShell`. Model `OperationHistory` (`@MainActor @Observable`) memegang halaman `operation.list` tanpa `activeOnly`, menerapkan setiap perubahan dari `OperationCenter` (aktif dan `recent`) menurut `id` dan `revision` yang lebih tinggi, dan memuat ulang halaman pertama pada `.resnapshot` `ShellSync`. Operasi yang belum ada di daftar disisipkan hanya bila `(createdAt, id)` nya tidak lebih tua dari baris tertua yang termuat (atau daftar sudah lengkap tanpa `next`); yang lebih tua hanya menunggu Show More. Halaman berikutnya yang memuat ID yang sudah ada memakai revision yang lebih tinggi.

- **Daftar**: `List` native dengan pilihan tunggal; inspector `.inspector` untuk detail (pola halaman Instances). Toolbar: judul "Operations" dan subjudul `OperationCenter.subtitle` (spec 0013).
- **Baris**: judul `OperationTitle.title` (install yang sudah selesai: nama dan versi dari `RuntimeStore` lewat `resource`), baris catatan, "{jam mulai} · {8 karakter ID}", `ToneBadge`, lalu tombol. Operasi berjalan dengan definisi tahap: `StagedProgress(value, layout: .stages)` (*Diperbarui 2026-11-10 (koordinasi scope 05):* tata letak `.stages` spec 0067; `.standard` tidak ada); tanpa definisi: label tahap dan `ProgressView()` indeterminate.
- **Status**: `StatusAppearance.operation(state:partial:continuing:)` dengan parameter baru `continuing` (default false) yang memberi "Finishing…" tone `caution` untuk `cancelRequested` yang `cancel` nya `continuing` (Follow-up spec 0012).
- **Footer**: "Stackly stages and checks each change before a step that can’t be undone. From that step on, Cancel is off and the operation finishes on its own." (adaptasi copy mockup; angka 80% mockup hanya contoh).
- **Konfirmasi**: Cancel tanpa dialog (bisa dibalik atau berhenti aman); Abandon dengan dialog destruktif native, Cancel sebagai tombol default: judul "Abandon “{judul}”?", isi "Stackly undoes what it can and leaves the rest as it is. You can’t resume it afterwards.", tombol "Abandon".
- **Umpan balik**: kartu umpan balik spec 0067 untuk hasil cancel (`continuing`: "It’s past a step that can’t be undone, so it will finish on its own."), resume, abandon (jumlah sisa), dan error.

**Baris catatan**:

| Keadaan | Teks |
| --- | --- |
| `queued` dengan `waitingOn` | "Waiting for another operation" |
| `queued` | "Queued" |
| `running`, `counted` | "{persen}% · {label tahap}" lalu " · can’t be cancelled" bila `past_point_of_no_return` |
| `running` | "{label tahap}" lalu " · can’t be cancelled" bila `past_point_of_no_return` |
| `cancel_requested` | `requested`: "Cancelling · {label tahap}"; `continuing`: "Finishing safely · {label tahap}" |
| `needs_recovery` | menurut `recovery.hint`: `resume` "It can continue. Resume or abandon it."; `abandon` "It can’t continue. Abandon it to clean up."; `manual` "It needs manual cleanup. Open it for details."; `auto_compensate` "Stackly is undoing it."; asing "Open it for details." |
| `failed` | `failure.error.message` (warna teks kritis spec 0012), lalu " · {kode}" |
| `cancelled` | "Cancelled" lalu " · undone" bila tidak ada `partial` gagal |
| `succeeded` | "Done", atau "Done after cancel request" bila `cancelRequestedAt` terisi, atau "{n} of {m} done" bila `partial` |

### Definisi tahap

Kunci tahap adalah konstanta `StageName` di `crates/domain/src/supervisor.rs`; developer memeriksa urutan terhadap file pekerjaan yang disebut sebelum menulis test.

| Jenis (file pekerjaan) | Tahap (`key`: judul, `also`) |
| --- | --- |
| `start_instance` (`supervisor/startup.rs`) | `validate`: Check, also `journal`, `lock`, `verify`, `endpoints` · `render`: Render config · `spawn`: Launch, also `identity`, `backoff` ("Waiting to restart") · `readiness`: Readiness · `publish`: Finish, also `complete` |
| `stop_instance` (`supervisor/stop.rs`) | `validate`: Check, also `lock` · `journal`: Prepare · `stop`: Stop · `complete`: Finish |
| `restart_instance` (`supervisor/restart_op.rs`) | `validate`: Check, also `lock`, `journal` · `stop`: Stop, also `await_exit`, `lock_wait` · `start`: Start, also `render`, `spawn`, `identity`, `backoff` · `readiness`: Readiness · `publish`: Finish, also `complete` |
| `force_stop_instance` (`supervisor/force.rs`) | `validate`: Check, also `lock`, `journal` · `kill`: Kill · `await_exit`: Wait for exit · `complete`: Finish |
| `apply_config`, `change_endpoints` (`supervisor/change.rs`) | `validate`: Check, also `lock` · `commit`: Save revision · `apply`: Apply · `verify`: Verify |
| `resolve_config_drift` (`supervisor/drift.rs`) | `validate`: Check · `render`: Rewrite files · `verify`: Verify |

`StageDefinition` mendapat `also: [StageAlias]` dengan `StageAlias { key, title }` (default kosong); `StagedProgressValue.from` mencari kunci di `key` lalu di `also`, memakai indeks definisi itu, dan teks tahap memakai judul alias bila ada. Jenis lain tetap tanpa definisi (pengecualian spec 0022 untuk jenis milik fitur 50 dan 52, serta rename, startup, settings, stop all, dan restart daemon yang hanya dua tahap).

### Navigasi

- `ShellNavigator.openOperation(_ id: UUID)` menyimpan `OperationRequest { id: UUID(), operation }` (baru setiap panggilan) dan mengganti `section` ke `.operations`; berpindah ke section lain membuang request itu. `ShellDestination.operations` tetap tanpa associated value.
- Overview: setiap baris seksi Operations memanggil `openOperation(operation.id)`; "Show All in Operations" tetap `show(.operations)`. Item O1 memanggil `openOperation`, dan isinya menjadi teks `needs_recovery` di *Baris catatan* (tanpa perintah CLI). O2 tidak berubah.
- Baris Activity dengan operation ID (klik ganda lewat `primaryAction`, atau menu konteks "Show in Operations") dan temuan doctor memanggil `openOperation`.

### Teks Activity

| Kolom | Isi |
| --- | --- |
| Time | hari ini: jam singkat locale; selain itu tanggal singkat tanpa tahun (tahun lain: dengan tahun); tooltip native (`.help`) tanggal dan jam lengkap |
| By | `client` `gui`: "App"; `cli`: "CLI"; actor `daemon`: "Stackly"; actor `user` lain: "Client" |
| Activity | dari tabel di bawah; hasil gagal ditambah " · {errorCode}". Baris `failed` dan `cancelled` tidak memakai templat sukses yang butuh detail catatan: "{Kata kerja} failed" atau "{Kata kerja} cancelled", dan `reason` `abandoned` menjadi "{Kata kerja} abandoned". Templat sukses yang detailnya kosong memakai teks cadangan di kurung |

| Action dan jenis | Teks |
| --- | --- |
| `operation.finished` `create_instance` | "Created" atau "Created from {duplicateOfName}"; gagal "Create failed" |
| `operation.finished` `start_instance`, `stop_instance`, `restart_instance`, `reload_instance`, `force_stop_instance` | "Started", "Stopped", "Restarted", "Reloaded", "Force stopped"; gagal "{Start} failed"; dibatalkan "{Start} cancelled" |
| `operation.finished` `change_endpoints` | "Ports changed: {ports}" ("Ports changed") |
| `operation.finished` `apply_config` | "Configuration revision {configRevision} applied" ("Configuration applied") |
| `operation.finished` `resolve_config_drift` | "Configuration files rewritten from revision {configRevision}" ("Configuration files rewritten") |
| `operation.finished` `rename_instance` | "Renamed from {oldName} to {newName}" ("Renamed") |
| `operation.finished` jenis lain | `OperationTitle` jenis itu tanpa nama instance, lalu hasil |
| `operation.needs_recovery` | "{judul} needs recovery" |
| `operation.cancel`, `operation.resume`, `operation.abandon` | "Cancel requested", "Resumed", "Abandoned, {leftovers} items left" |
| `secret.reveal` | "Viewed {field} (show)" atau "Copied {field}" dari bagian terakhir reference |
| `secret.set`, `secret.delete` | "Changed {field}", "Deleted {field}" |
| action asing | action apa adanya |

Di bawah tabel dan Show More, tab menulis baris kaki `.caption` `textSecondary` "Double-click an entry to open its operation." (`ActivityCopy.footer`). *Diperbarui 2026-11-10 (koordinasi scope 05):* baris kaki ini copy developer F35 dan tidak ada di mockup; dicatat agar spec, kode, dan snapshot sama.

### Baris Settings

Tab Storage Settings (spec 0032) mendapat grup "Activity" di bawah Logs: `Picker` "Keep activity for" dengan 7 days, 30 days, 90 days, dan keterangan AC-20. Saat offline, atau saat `DaemonSettingsView` tidak membawa `auditRetention` (daemon lama), baris tidak tampil, sama dengan aturan baris setting milik fitur yang belum dibangun di spec 0032.

### Fixture

Skenario spec 0068 baru di `Fixtures/`: `FixtureOperations` (satu operasi per keadaan di AC-19, dengan judul dan waktu tetap, instance dan runtime dari data mockup) dan `FixtureActivity` (satu baris per action di *Teks Activity*). Snapshot test di `SnapshotTests/OperationsPageSnapshotTests.swift` dan `InstanceActivitySnapshotTests.swift`.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| baris audit operasi | `id`, `occurred_at` | `AuditEventId::generate()` dan `Clock::now` engine, di batch yang sama |
| baris audit operasi | `actor`, `client` | `OperationTrigger` journal (`origin`, `client`); untuk cancel, resume, abandon: `OperationTrigger` dari handshake pengirim |
| baris audit operasi | `operation_id` (correlation ID) | ID operasi; sama dengan `ErrorBody.operationId` dan field log `operation_id` (spec 0007 AC-19) |
| baris audit operasi | `operation_kind`, `resource`, `instance_id` | `Operation::kind()`, `Operation::resource()`; `instance_id` bila resource instance |
| baris audit operasi | `config_revision` | `AuditNote.config_revision` dari pekerjaan (*Catatan audit pekerjaan*) |
| baris audit operasi | `result` | state commit (*Baris audit operasi*) |
| baris audit operasi | `safe_details` | *Kunci `safe_details`*: journal (`failure`, `recovery`, stage, `cancel_requested_at`), disposisi cancel, laporan abandon, dan `AuditNote.details` |
| baris secret | `instance_id`, `client` | pemilik reference spec 0011; `SecretCaller.client` |
| `create_instance` | `duplicateOf`, `duplicateOfName` | `CreateInstanceParams.duplicateOf` lalu `Registry::instance` saat validasi |
| `start_instance` | `config_revision`, `ports` | revision aktif yang dibaca pekerjaan start; port yang dipilih langkah `endpoints` |
| ganti port dan apply | `config_revision`, `oldRevision`, `settings`, `ports` | `ChangePlan` spec 0029 |
| `OperationSummary.cancel` | ketersediaan | `Operation::cancel_availability` dari state, `point_of_no_return_at` journal, dan `interrupt_policy` |
| `audit.list` | halaman | `Registry::audit_events(AuditQuery)` |
| `DaemonSettingsView.auditRetention` | `days`, `maxRows` | kolom `daemon_settings.audit_retention_days`; konstanta `AUDIT_MAX_ROWS` |
| prune | `audit_max_age` | setting itu saat prune berjalan |
| halaman Operations | daftar | `operation.list` (riwayat) dan `OperationCenter` (aktif dan event) |
| halaman Operations | judul | `OperationTitle` (spec 0022) dengan `InstanceStore` dan `RuntimeStore` |
| halaman Operations | tahap | `OperationStages.definitions(for:)` dan `OperationSummary.stage` |
| halaman Operations | Cancel aktif atau mati | `OperationSummary.cancel` (aturan cadangan bila kosong) |
| halaman Operations | Retry | `kind`, `state`, `resource`, dan `InstanceStore` (AC-10) |
| halaman Operations | Resume, Abandon | `recovery.actions`; `expectedRevision` dari `revision` summary yang tampil |
| inspector | langkah, item, failure, recovery | `operation.get` (`OperationDetail` spec 0007) |
| tab Activity | baris | `audit.list { instanceId }` |
| tab Activity | Time | `occurredAt` dengan locale dan zona waktu sistem |
| Settings | picker | `DaemonSettingsView.auditRetention.days`; `revision` untuk `expectedRevision` |

### Key invariants

- Tidak ada commit yang membawa operasi ke state akhir atau `needs_recovery` tanpa baris audit di batch yang sama, dan tidak ada baris audit operasi tanpa commit itu.
- Baris audit tidak pernah memuat nilai secret: semua nilai lewat `Redactor`, dan catatan pekerjaan hanya berisi nama, nomor, port, dan reference.
- Baris audit append only (trigger spec 0005) dan hanya hilang lewat prune; ini bukan catatan tamper proof (REQ-045).
- Gagal membangun baris audit tidak pernah menahan state operasi.
- `cancel` dihitung di satu fungsi domain; GUI dan CLI tidak punya tabel kebijakan jenis sendiri.
- Cancel di GUI tidak pernah aktif saat `cancel` bernilai `past_point_of_no_return`, `requested`, `continuing`, atau `none`.

### Security model

Semua method hanya untuk koneksi uid yang sama (spec 0006). `audit.list` hanya membaca dan tidak pernah membawa nilai secret, karena registry tidak menyimpannya (spec 0011). Audit lokal ini milik user: user yang sama bisa menghapus file registry, jadi audit bukan bukti forensik, dan spec ini tidak mengklaim sebaliknya. Client kind berasal dari handshake yang dinyatakan client sendiri, jadi "App" atau "CLI" adalah klaim client, bukan identitas terverifikasi.

### Configuration required

Tidak ada env var atau credential baru. Satu setting daemon baru di registry (`audit_retention_days`).

### Menjalankan test di Mac ini

Test core dan storage memakai `TestRoot`; test daemon memakai `stacklyd::server::start` dengan `DaemonConfig` yang `prune_interval` nya dipendekkan, provider fake (`cargo build -p stackly-fake-server` dan `skeleton_seed`), dan `allocate_port`. Swift: `swift test --package-path apps/macos/Packages/StacklyUI` setelah daemon dan fake dibangun, selain itu `BLOCKED`.

### Critical test scenarios

- Setiap jalur di *Baris audit operasi* menulis tepat satu baris yang benar di batch commit, di fake dan SQLite; commit gagal `NoSpace` tidak meninggalkan baris dan commit ulang menulisnya sekali, memverifikasi **AC-1**.
- Create, start, dan ganti port sukses dan gagal lewat socket dengan `FixtureCredential`, lalu `audit.list` dan pindaian byte, memverifikasi **AC-2**, **AC-4**, **AC-5**.
- Operasi dimulai koneksi `gui` lalu dibatalkan koneksi `cli`; cancel kedua dan resume yang ditolak tanpa baris, memverifikasi **AC-3**.
- Prune dengan 7, 30, dan 90 hari di `FakeClock`, dan `settings.update` yang membangunkan loop prune, memverifikasi **AC-6**.
- Migrasi dari fixture beku versi sebelumnya dan conformance filter, memverifikasi **AC-7**.
- Tabel ketersediaan cancel sebagai test domain per baris, plus summary `start_instance` setelah `spawn` dan `install_runtime` di `commit`, memverifikasi **AC-8**.
- Pekerjaan staging lalu commit lewat `test_starting_method`: cancel saat staging, cancel setelah commit mulai, dan commit yang gagal, memverifikasi **AC-9**.
- Model Swift: Resume, Abandon, dan Retry per kombinasi state, jenis, dan aksi; `revision_mismatch` saat abandon, memverifikasi **AC-10**.
- `OperationHistory` dengan klien palsu: gabungan halaman dan event, Show More, resnapshot, offline, error, unsupported, memverifikasi **AC-11**, **AC-12**, **AC-13**.
- Pemetaan tahap per jenis, termasuk `backoff` dan kunci asing, memverifikasi **AC-14**.
- Model Activity: pemetaan teks per action, refresh pada event dan `completedRounds`, memverifikasi **AC-15**, **AC-18**.
- Output CLI manusia dan JSON, exit code nama yang tidak dikenal, memverifikasi **AC-16**.
- Fixture kontrak Rust dan Swift, nilai asing, memverifikasi **AC-17**.
- Snapshot light dan dark, memverifikasi **AC-19**, **AC-20**.

### Usulan update untuk spec lain

Dicatat di *Follow-up*; lead menerapkannya lewat `/sync` atau `/architect` update. Tidak ada ID AC atau keputusan spec lain yang berubah: semuanya tambahan.

### Tambahan fitur 44 sampai 47

*Ditambah fitur 44, 45, 46, dan 47 ([spec 0044](../0044-manifest-environment-validate-plan/index.md), [spec 0045](../0045-lockfile-override-lokal/index.md), [spec 0046](../0046-env-up-dependency-graph/index.md), [spec 0047](../0047-env-down-shared-instance-lease/index.md), 2026-11-13).* Aturan engine, bentuk baris audit, dan halaman spec ini dipakai apa adanya; di sini hanya yang ditambah pemilik jenis operasi baru. AC spec ini tidak berubah. Spec 0046 dan 0047 menyebut halaman Operations spec ini "Activity" (misalnya aksi "Show in Activity"); yang dimaksud adalah halaman Operations lewat `openOperation`, bukan tab Activity per instance.

**Catatan audit pekerjaan** (tambahan untuk tabel di *Catatan audit pekerjaan*; semuanya lewat `set_audit_note`, dan setiap nilai lewat `Redactor`):

| Jenis | Detail | Sumber |
| --- | --- | --- |
| `import_environment`, `remove_environment` | nama environment, `manifestSha256`, lokasi manifest dengan `~` | spec 0044 *Security model* |
| `environment_up` | nama environment, `manifestSha256`, lokasi dengan `~`, jumlah `ready` dan `total`, alias yang tidak ready | spec 0046 *Security model* |
| `environment_down` | nama environment, jumlah lease yang dilepas, nama instance `stopped`, `kept`, dan `failed` | spec 0047 *Security model* |
| `stop_instance`, `stop_all_instances`, `change_endpoints`, dan stop dari `agent_off`, saat dijalankan dengan `overrideLeases` true | tambahan `overriddenEnvironments` (nama environment yang di override) | spec 0047 AC-13, AC-15, *Security model* |
| `stop_instance` anak Down yang guard nya memutuskan instance tetap jalan | `kept: true`, supaya stop yang tidak terjadi tidak tercatat sebagai stop | spec 0047 *Operasi `environment_down`* |

Operasi pengatur menulis satu baris audit sendiri, dan setiap operasi anak (`install_runtime`, `create_instance`, `start_instance`, `stop_instance`) menulis barisnya sendiri seperti biasa (spec 0046). Resource operasi environment adalah `AuditResource::Environment`, jadi barisnya tidak ber `instance_id`: seperti `stop_all_instances`, ia tidak muncul di tab Activity sebuah instance, sedangkan baris operasi anaknya muncul di instance masing masing.

**Yang tidak diaudit**: `env.validate`, `env.plan`, dan `env.preview_down` baca saja dan bukan operasi (spec 0044 AC-4 dan AC-6, spec 0047 AC-1). `env.lock` menulis satu file tanpa operasi dan tanpa audit, dan `env.export` hanya membalas teks (spec 0045).

**Halaman Operations**:

| Hal | Aturan | Sumber |
| --- | --- | --- |
| Judul | `OperationTitle` (spec 0022) memberi "Bring up environment {name}" dan "Bring down environment {name}", tanpa nama kalau environment tidak dikenal | spec 0046 AC-22, spec 0047 AC-21 |
| Operasi anak | operasi dengan `OperationSummary.parent` ditampilkan bersarang di bawah induknya, dan tidak dihitung sendiri di ringkasan Overview | spec 0046 AC-22 dan *Activity dan Overview* |
| Label "Partial" (nada caution) untuk Up | saat `failure.error.code` `env.up_incomplete` dan `details.ready` di atas 0, dengan catatan "{ready} of {total} services ready" | spec 0046 AC-22 |
| Label "Partial" (nada caution) untuk Down | saat `failure.error.code` `env.down_incomplete` dan `details.stopped` di atas 0, dengan catatan "{stopped} of {total} instances stopped" | spec 0047 AC-21 |
| Cancel | hanya dari `OperationSummary.cancel`, seperti jenis lain; klien tidak punya tabel jenis sendiri | spec 0046 dan spec 0047, banner selama Up dan Down |

Operasi itu sendiri tetap berstate `failed`; "Partial" hanya tampilan untuk dua kode itu. Spec 0046 dan 0047 tidak menambah baris di *Definisi tahap*, jadi `environment_up` dan `environment_down` memakai aturan jenis tanpa definisi tahap.

Belum ditetapkan sumber: judul `OperationTitle` untuk `import_environment` dan `remove_environment` (dicatat di Follow-up spec 0022).

## Build plan

Pendekatan Journey: milestone 1 menyelesaikan jalur "jejak operasi dari CLI" lengkap (daemon menulis, CLI membaca, retensi berlaku), milestone 2 jalur "pantau dan pulihkan operasi di app", milestone 3 jalur "riwayat instance di app", milestone 4 bukti. Prasyarat: fitur 1 sampai 32 dan 67 sampai 70 di `main`. Nomor protokol dan migrasi ditetapkan lead saat build (sementara 3.4 dan `0012`).

**Milestone 1: audit operasi sampai CLI**

1. Domain dan storage: field `AuditEventRecord`, `AuditResult`, `AuditResource`, `AuditAction`, `AuditQuery`, `AuditRetentionDays` di `DaemonSettings` dan `SettingsPatch`, `RetentionPolicy::with_audit_days`, `CancelAvailability` dan `cancel_availability`, migrasi `NNNN_audit_operation_fields.sql` dengan fixture beku, `FakeRegistry`, `SqliteRegistry`, dan `registry_conformance`, satisfies **AC-6**, **AC-7**, **AC-8**.
2. Core engine: `operation::audit::row` dan helper batch di setiap jalur tabel, `OperationTrigger` di `cancel`, `resume`, `abandon`, `set_audit_note`, redaksi dan pemotongan, `audit_row` secret yang mengisi `instance` dan `client`, test core per jalur dengan `ScriptedWork` dan sentinel, satisfies **AC-1**, **AC-3**, **AC-4**.
3. Catatan audit pekerjaan: create (dengan `duplicateOf` di draft), start, restart, reload, `ChangePlan::audit_note` untuk apply, ganti port, dan drift, rename; test core per jenis, satisfies **AC-2**, **AC-18**.
4. Protocol dan daemon: DTO dan method di *Method IPC*, `OperationSummary.cancel`, minor protokol, `cargo xtask codegen`, fixture, `cargo xtask fuzz seed`, handler `ipc::audit`, `settings.update` dan view, prune dari setting dengan wake, integration test `apps/daemon/tests/audit.rs` (AC-2, AC-5, AC-6) dan `apps/daemon/tests/operations/cancel_point.rs` (AC-9), satisfies **AC-2**, **AC-5**, **AC-6**, **AC-8**, **AC-9**, **AC-17**.
5. CLI: `stackly audit list`, kolom dan kalimat cancel di `stackly operation`, key `audit-retention-days`, test output dan exit code, satisfies **AC-6**, **AC-9**, **AC-16**.

**Milestone 2: halaman Operations**

6. `StacklyClient`: method `auditList`, tipe hasil codegen, test kontrak Swift; `StageDefinition.also`, definisi tahap di `OperationStages`, `StatusAppearance.operation(state:partial:continuing:)`, test pemetaan, satisfies **AC-14**, **AC-17**.
7. `StacklyUI/Operations/`: `OperationHistory`, `OperationsPage`, baris, inspector, aksi Cancel, Retry, Resume, Abandon dengan konfirmasi dan umpan balik, placeholder, `ShellNavigator.openOperation`, pengganti `UnbuiltPage(.operations)`, perubahan Overview (baris dan O1), test model dengan klien palsu, satisfies **AC-9**, **AC-10**, **AC-11**, **AC-12**, **AC-13**.

**Milestone 3: tab Activity dan setting**

8. `InstanceTab.activity`, `InstanceActivityModel` dan view di `InstanceDetail/Activity/`, teks per action, refresh, navigasi ke operasi; sheet Duplicate mengirim `duplicateOf`; grup Activity di tab Storage Settings; test model, satisfies **AC-15**, **AC-18**, **AC-20**.

**Milestone 4: tampilan dan bukti**

9. Fixture `FixtureOperations` dan `FixtureActivity`, snapshot light dan dark, satisfies **AC-19**.
10. *(Langkah manual, butuh mata dan tangan kamu; lead memindahkannya ke `docs/manual.md`)* Sesi GUI di Mac ini dengan daemon asli dan provider fake di root sementara: install runtime lalu lihat Cancel mati di tahap Commit, start yang gagal karena port dipakai lalu Retry dari halaman Operations, buka operasi dari Overview dan dari tab Activity, ganti port lalu bandingkan tab Activity dengan `stackly audit list --instance`, ubah "Keep activity for", light dan dark, satisfies **AC-11**, **AC-13**, **AC-15**, **AC-20**.

**Sesi paralel.** Langkah 1 sampai 4 berurutan (kontrak dulu). Setelah langkah 4, langkah 5 (CLI, `apps/cli/`) dan milestone 2 (`clients/swift/` dan `StacklyUI/Operations/`, `Components/Progress/`, `Overview/`, `Shell/`) tidak berbagi file dan bisa berjalan sebagai dua sesi. Langkah 8 menyentuh `InstanceDetail/`, `CreateInstance/` atau `Duplicate/`, dan `Settings/`; ia butuh `openOperation` dari langkah 7, jadi mulai setelah bagian navigator langkah 7 di merge, lalu bisa berjalan paralel dengan sisa langkah 7. Langkah 3 bisa dibagi per file pekerjaan setelah langkah 2.

## Consequences

**Positive**:

- Tidak ada operasi yang berakhir tanpa jejak, dan fitur baru mendapat audit tanpa menulis apa pun selain catatan opsional.
- Correlation ID sama di audit, error, log, dan halaman Operations, jadi satu ID cukup untuk menelusuri kegagalan.
- Cancel yang tidak aman tidak bisa ditekan di GUI, dan CLI menjelaskan apa yang terjadi kalau tetap dikirim.
- Audit bisa diekspor lewat `stackly audit list --json` tanpa format export baru.

**Negative / tradeoffs**:

- Setiap commit akhir operasi menulis satu sampai dua baris tambahan; murah, tetapi menambah ukuran registry sampai batas 100.000 baris, yang membuang baris tertua lebih dulu tanpa melihat umur. Operasi latar ikut diaudit; respawn crash loop dibatasi retry budget spec 0033 (5 kali dalam 5 menit), jadi tidak bisa membanjiri batas itu.
- Catatan audit pekerjaan hanya di memori: operasi yang ditutup setelah daemon restart kehilangan detail fitur (hasil dan correlation ID tetap ada).
- Tab Activity tidak menerima event untuk aksi secret dari client lain; baris itu muncul saat tab dimuat ulang (event baru berarti bump mayor).
- `audit.list` antre di thread registry yang sama dengan tulisan; halaman dibatasi 200 baris dan 900 KiB untuk menahan dampaknya.
- Retensi operasi tetap 30 hari, jadi riwayat halaman Operations bisa lebih pendek dari tab Activity bila audit disetel 90 hari.

**Neutral**:

- Satu migrasi, satu minor protokol, tanpa `EventKind` baru.
- `AuditResult` dan `AuditResource` bertambah varian; registry lama terbaca karena kolom `result` sudah set terbuka di SQL.

## Follow-up

- [x] Spec 0005 (`/sync`): catat kolom audit baru, index `audit_events_instance`, `AuditQuery.instance` dan `operation`, kolom `daemon_settings.audit_retention_days`, dan `RetentionPolicy::with_audit_days` di tabel *Tambahan*; baris 320, 515, dan 820 dijawab di sini. Diterapkan di spec 0005 (2026-11-10): baris *Tambahan fitur 35* dan catatan `result`.
- [x] Spec 0007 (`/sync`): baris Follow-up "Fitur 35: baris audit untuk cancel, abandon, dan jenis operasi destruktif" dijawab di *Baris audit operasi* (semua jenis, bukan hanya yang destruktif); `cancel`, `resume`, dan `abandon` menerima `OperationTrigger` pengirim; catat `set_audit_note` dan `cancel_availability` di *API engine*. Diterapkan di spec 0007 (2026-11-10): Follow-up dicentang, `by: OperationTrigger` di *API engine*, `set_audit_note` di `OperationContext`, dan `cancel_availability` di *Cancel*.
- [x] Spec 0011 (`/sync`): baris Follow-up 612 bagian `safe_details` audit dijawab AC-4; `audit_row` mengisi kolom `instance_id` dan `client`. Diterapkan di spec 0011 (2026-11-10): catatan `audit_row` (`client`, `instance_id`) dan bagian fitur 35 di Follow-up 612.
- [x] Spec 0013 (`/sync`): Follow-up 733 dijawab; halaman Operations di atas `OperationCenter` plus `OperationHistory`. Diterapkan di spec 0013 (2026-11-10).
- [x] Spec 0022 (`/sync`): Follow-up 592 dijawab (AC-11, AC-13); teks O1 diganti *Baris catatan*, dan baris seksi Operations memanggil `openOperation`. Diterapkan di spec 0022 (2026-11-10).
- [x] Spec 0024 (`/sync`): Follow-up 476 dijawab AC-18 (field `duplicateOf` di `instance.create`). Diterapkan di spec 0024 (2026-11-10).
- [ ] Spec 0012 (`/sync`): Follow-up 435 dijawab; tambahan parameter `continuing` di `StatusAppearance.operation` dan barisnya di tabel status operasi `design.md`. *(2026-11-10) Spec 0012 diselaraskan (baris "Finishing…" dan parameter `continuing`). Tetap terbuka untuk baris yang sama di tabel status operasi `apps/macos/design.md`, yang di luar `docs/specs/`.*
- [x] Spec 0067 (`/sync`): Follow-up 468 dijawab *Definisi tahap*; `StageDefinition.also`. Diterapkan di spec 0067 (2026-11-10): `StageDefinition.also` dan `StageAlias` di sketsa API.
- [x] Spec 0032 (`/sync`): grup Activity di tab Storage, `auditRetention` di `DaemonSettingsView`, dan key CLI `audit-retention-days`; AC-21 spec itu (retensi log read only) tidak berubah. Diterapkan di spec 0032 (2026-11-10).
- [x] Spec 0008 (`/sync`): perintah `stackly audit list` dan perubahan output `stackly operation`. Diterapkan di spec 0008 (2026-11-10), termasuk pengecualian `COLUMNS` untuk DETAILS.
- [x] Spec 0020 dan 0029 (`/sync`): `duplicateOf` di params create; `ChangePlan::audit_note` di samping input journal AC-20. Diterapkan di spec 0020 dan 0029 (2026-11-10).
- [ ] Fitur 51 dan 54: catatan audit untuk restore, remove, dan purge (paling sedikit `config_revision` dan apakah data dihapus), lalu jalankan bagian purge AC-045.
- [x] Fitur 34: tidak ada audit untuk doctor dan bundle (disepakati dengan `arc-f34-spec`); bila audit perlu ikut bundle, pakai `audit.list`. Fitur 34 di `main` tanpa audit untuk doctor dan bundle (2026-11-10); bagian bersyarat "bila audit perlu ikut bundle" dilacak Follow-up spec 0034.

## Rationale

Alasan, opsi, dan keputusan K1 sampai K16 ada di [rationale.md](rationale.md).
