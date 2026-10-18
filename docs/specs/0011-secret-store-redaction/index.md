# 0011. Secret store dan redaction

**Date**: 2026-10-18
**Status**: Proposed

## Summary

Spec ini menetapkan cara Stackly menyimpan dan memakai secret. Nilai secret hanya tinggal di Keychain milik user (login Keychain untuk root default, Keychain file sendiri untuk root development dan test), disentuh hanya oleh `stacklyd` lewat satu thread Keychain, sedangkan registry dan manifest hanya menyimpan reference berbentuk `local:<path>`. Ada satu use case untuk mengisi, membangkitkan, menyalin, atau mengganti nilai di balik reference, yang dipanggil lewat permintaan IPC `secret.set`, sheet GUI, input aman CLI (TTY, stdin, file, atau nama env var, tidak pernah argumen), dan juga oleh create instance, env up, dan restore. Redaction tiga lapis (tipe yang menyamarkan dirinya sendiri, pencocokan nilai yang dikenal, dan pola) berlaku untuk log, error, CLI, GUI, diagnostics, dan export; reveal dan copy hanya lewat satu aksi eksplisit yang dicatat, dan Keychain yang terkunci tidak pernah berujung auth kosong, melainkan blocker needs secret yang menyebut reference dan alasannya.

## Requirements

**User stories**:

- Sebagai developer, saya ingin password tiap instance dibangkitkan dan disimpan di Keychain saat create, supaya saya tidak pernah mengetik atau melihatnya kecuali saya memintanya.
- Sebagai developer yang memakai script, saya ingin mengisi secret tanpa menaruhnya di argumen command, supaya nilainya tidak terbaca dari daftar proses atau history shell.
- Sebagai pengguna yang memulihkan backup atau pindah Mac, saya ingin satu alur untuk mengisi nilai yang benar di balik reference yang hilang, dari GUI maupun CLI.
- Sebagai pengguna yang membagikan log atau bundle diagnosis, saya ingin secret tidak pernah ikut tertulis, supaya saya tidak perlu menyunting file dulu.
- Sebagai pengguna yang Keychain nya terkunci saat login, saya ingin instance tertahan dengan alasan yang jelas, bukan berjalan tanpa password.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-016):

- **AC-1**: `SecretRef::parse` menerima `local:` diikuti 2 sampai 8 segmen yang dipisah `/`, tiap segmen 1 sampai 64 byte dengan pola `[a-z0-9][a-z0-9._-]*` dan bukan `.` atau `..`, dengan total teks paling banyak 255 byte. Teks lain ditolak dengan `SecretRefError` yang menyebut aturannya; skema lain (termasuk `keychain:`) ditolak dengan alasan `unsupported scheme`. Teks yang diterima bolak balik tanpa perubahan. Segmen pertama `instance` hanya dibuat daemon, dan segmen pertama `stackly` dicadangkan, jadi `secret.set` untuk reference yang belum terdaftar dengan segmen pertama itu ditolak (REQ-016, BR-018).
- **AC-2**: `MacosSecretStore` menyimpan setiap nilai sebagai generic password dengan service `stackly-secret` dan account berupa path reference tanpa `local:`, di satu keychain yang dipatok eksplisit pada setiap operasi: login Keychain untuk root default, atau Keychain file `R/data/keychain/stackly.keychain-db` untuk `STACKLY_ROOT=R`. Ia membaca, mengganti secara atomik, menghapus, dan mengecek keberadaan item lewat atribut saja tanpa membaca nilai. Integration test memakai Keychain file di bawah `TestRoot`; keychain file tidak pernah masuk search list user, dan search list user sebelum dan sesudah test sama (REQ-016).
- **AC-3**: Kode `OSStatus` dipetakan oleh fungsi murni `map_status(code, keychain_locked)` dengan test tabel. Dengan interaksi `Never`, Keychain yang terkunci menghasilkan `SecretStoreError::Locked`, item yang ACL nya tidak mempercayai proses ini menghasilkan `AccessDenied { by_user: false }`, dan tidak ada dialog yang muncul. Dengan interaksi `AllowPrompt`, dialog yang ditolak atau dibatalkan pengguna menghasilkan `AccessDenied { by_user: true }`. `NotFound` pada keychain yang terkunci dilaporkan `Locked`, bukan `missing`. Kode lain menjadi `Backend { code }`, dan tidak ada pesan error yang memuat nilai secret.
- **AC-4**: Permintaan dari klien interaktif (GUI, atau CLI dengan stdin dan stderr TTY tanpa mode noninteractive) memakai interaksi `AllowPrompt`; pemicu latar daemon (autostart, reconcile, restart policy) dan klien noninteractive memakai `Never`. Interaksi Security framework mati sebagai default proses dan hanya dinyalakan selama satu panggilan `AllowPrompt`. Semua panggilan ke store lewat satu `SecretWorker` yang menjalankannya berurutan di satu thread, dengan batas tunggu dari `WorkerConfig` (default 180 detik untuk `AllowPrompt` dan 10 detik untuk `Never`). Job yang batasnya lewat sebelum mulai tidak pernah dijalankan dan gagal `Busy`; `write` atau `delete` yang sudah mulai lalu melewati batas gagal `OutcomeUnknown`, dan daemon menjalankan probe setelah panggilan itu selesai.
- **AC-5**: `SecretService::put(target, SecretInput, mode, expected_revision)` adalah satu satunya use case yang mengisi nilai, dipakai `secret.set`, GUI, CLI, create instance, env up, dan restore. `SecretInput` bernilai `value`, `generate`, `copyFrom`, atau `bind`, dan mode bernilai `create`, `replace`, atau `upsert`. Urutannya: cek registry dan `expectedRevision` (konflik berhenti tanpa menyentuh Keychain), siapkan nilai, probe Keychain dan cek mode terhadap hasilnya, tulis Keychain, lalu satu transaksi registry yang memperbarui baris (status `present`, `value_origin`, `value_updated_at`, revision naik tepat 1; baris baru lahir dengan revision 1) dan menulis baris audit. `create` pada item yang sudah ada di Keychain gagal `secret.conflict`, dan `replace` pada item yang belum ada gagal `secret.not_found`. Respons, event `secret.changed`, baris audit, dan log tidak memuat nilai.
- **AC-6**: `generate` menghasilkan 32 karakter dari alfabet `[A-Za-z0-9]` lewat CSPRNG OS; dua ribu nilai bangkitan berbeda semua dan hanya memakai alfabet itu. Create instance membuat reference `local:instance/<instance-id>/<purpose>` per secret field, dengan purpose dari algoritma di *Format secret reference*, dan memakai `generate` kecuali pemanggil secara eksplisit memberi `value`, `copyFrom`, atau `bind`. Daemon tidak pernah memilih reference yang sudah ada untuk resource baru atas inisiatifnya sendiri.
- **AC-7**: Nilai secret harus UTF-8, 1 sampai 65536 byte, tanpa byte NUL; nilai yang melanggar ditolak `secret.invalid_value` sebelum Keychain disentuh. Nilai di bawah 8 byte diterima, dan respons membawa peringatan `shortValueNotScrubbed`.
- **AC-8**: `secret.list` dan `secret.status` mengembalikan reference, pemilik, purpose, status (`present`, `missing`, atau `unknown`), `usedBy`, `valueOrigin`, waktu, revision, dan keadaan store (`available`, `locked`, `unsupported`, atau `unknown`), tanpa nilai. Keadaan store diambil dari cache dan tidak pernah menunggu Keychain. Saat daemon start dan setiap kali `secret.status` diminta, daemon mengecek atribut item lewat worker (interaksi `Never`) lalu memperbarui cache status dan `platform_store_key` tanpa menaikkan revision; item yang dihapus lewat Keychain Access terbaca `missing`, dan item yang tidak bisa dicek terbaca `unknown`.
- **AC-9**: Nilai secret hanya keluar dari daemon lewat respons `secret.reveal`, dengan `purpose` `show` atau `copy`. Setiap reveal, berhasil atau gagal, menulis satu baris audit berisi actor, jenis klien, reference, purpose, dan hasil (`succeeded` atau `failed:<kode error>`), tanpa nilai. Kalau baris audit gagal ditulis, reveal gagal dan nilainya tidak dikembalikan (AC-016).
- **AC-10**: `secret.delete` menghapus baris `secret_references` lebih dulu dalam transaksi yang gagal kalau reference masih punya pemakaian (`secret.conflict` dengan daftar `usedBy`), lalu menghapus item Keychain. Kalau penghapusan Keychain gagal (misalnya terkunci), baris dipulihkan persis seperti semula dengan revision naik 1 dan error dikembalikan. Setiap delete, berhasil atau gagal, menulis baris audit `secret.delete` tanpa nilai.
- **AC-11**: `SecretService::resolve_for_start` selalu membaca nilai dari store (cache tidak dipercaya) dan mengembalikan semua nilai yang dibutuhkan sebuah instance, atau `NeedsSecret` yang mendaftar setiap reference bermasalah beserta alasannya (`missing`, `store_locked`, `access_denied`, `store_busy`, atau `store_unsupported`) dan `generateAllowed`, diurutkan menurut teks reference. Setelah pengguna menolak satu dialog, reference sisanya langsung `access_denied` tanpa dialog baru. Error `Backend` mengalahkan needs secret. `NeedsSecret` dipetakan ke kode error `needs_secret` dan ke blocker `needsSecret` pada instance, yang hilang saat resolve berikutnya berhasil atau saat `secret.changed` membuat semua reference nya `present`. Tidak ada jalur kode yang mengganti nilai yang hilang dengan string kosong, nilai default, auth `trust`, atau password hardcode (AC-062, VAL-018).
- **AC-12**: Pengiriman secret ke provider mengikuti urutan stdin, hash di config, file sekali pakai, env var, lalu config plaintext, dan tidak pernah lewat argv. File secret dibuat dengan mode 0600 di `<runtime>/i/<short-key>/secrets/<nama>` (folder 0700), ditulis atomik, dihapus guard `SecretFile` saat drop (file sekali pakai) atau setelah stop bersih (config plaintext), dan disapu untuk instance yang tidak berjalan. `ensure_not_in_argv` menolak argumen yang sama persis dengan nilai mana pun (termasuk bentuk `--opsi=<nilai>`) atau memuat nilai sepanjang 16 byte atau lebih, dengan kode `secret.delivery_rejected` (BR-018, OQ-008).
- **AC-13**: `Redactor` di core mengganti dengan `****` setiap nilai yang dikenal (nilai 16 byte atau lebih di mana saja, nilai 8 sampai 15 byte hanya sebagai token utuh), password di userinfo URI, dan nilai setelah kunci sensitif, menurut grammar di *Redaction*. Setiap nilai dikenalkan ke `Redactor` begitu ada di memori daemon, sebelum ditulis ke mana pun. Setiap baris log `stacklyd` lewat writer yang memanggil `Redactor` setelah baris diformat, jadi pesan, field, dan error ikut tersaring (AC-016).
- **AC-14**: DTO protokol tidak punya field nilai secret selain `SecretText` di `SecretInput` dan di respons `secret.reveal`. `SecretText` diserialize sebagai string biasa di wire, tetapi `Debug` nya mencetak `SecretText(****)` dan isinya dihapus dari memori saat drop; `SecretValue` di `stackly-platform` sama. Pesan dan detail envelope error, progress operasi, dan event lewat `Redactor` sebelum dikirim, dan daemon tidak pernah mencatat isi frame IPC (AC-016).
- **AC-15**: `cargo xtask check-deps` memindai token stream setiap file (termasuk isi makro) dan gagal dengan pesan `<crate> <file>:<baris>: secret expose outside the allowlist` kalau ada pemanggilan method bernama `expose` di luar modul yang diizinkan (lihat *Penjaga di kode*). Ia juga gagal kalau `security-framework` menjadi dependency langsung selain di `stackly-platform-macos`, atau `rpassword` selain di `stackly-cli`. Test memakai crate fixture yang melanggar, termasuk pemanggilan di dalam `tracing::info!`.
- **AC-16**: Suite sentinel mengisi fixture credential lewat `secret.set` (dan, di milestone 2, lewat `stackly secret set --stdin`), memakainya lewat `resolve_for_start`, lalu menjalankan list, status, reveal, delete, dan satu error yang sengaja dipicu, dengan daemon diarahkan ke `TestRoot` lewat `STACKLY_ROOT`. Prefix fixture credential tidak ditemukan di file log daemon, byte file SQLite registry (termasuk WAL), rekaman frame IPC selain permintaan yang membawa `SecretInput` dan respons `secret.reveal`, dan output default maupun `--json` CLI selain `stackly secret reveal`. Setiap test menulis evidence record `fixture` dengan `covers` AC-016, REQ-016, dan BR-018 (AC-016).
- **AC-17**: `stackly secret set <ref>` membaca nilai dari `/dev/tty` dengan echo mati saat interaktif (dua kali, harus sama, paling banyak tiga percobaan), atau dari tepat satu sumber `--stdin`, `--from-file <path>`, `--from-env <NAMA>`, `--generate`, atau `--copy-from <ref>`. `--stdin` dan `--from-file` membuang tepat satu `\n` atau `\r\n` di akhir; `--from-env` memakai nilainya apa adanya. Tidak ada flag yang menerima nilai secret. Tanpa TTY dan tanpa salah satu sumber itu, command langsung gagal dengan exit code invalid input tanpa menunggu input (BR-018, REQ-028, AC-028).
- **AC-18**: `StacklyTestSupport` punya `FixtureCredential` Swift dengan prefix yang sama dengan `FIXTURE_CREDENTIAL_PREFIX`, `description` dan `debugDescription` `FixtureCredential(****)`, dan `assertAbsent(in:what:)`. `StacklyClient` punya method `listSecrets`, `secretStatus`, `setSecret`, `revealSecret`, dan `deleteSecret`; `revealSecret` mengembalikan `RevealedSecret` buatan tangan yang `description`, `debugDescription`, dan mirror nya tersamar. Test kontrak Swift membaca fixture secret di `protocols/fixtures/` yang memakai nilai contoh tetap `not-a-real-secret`.
- **AC-19**: `StacklyUI` punya `SecretField`, `SecretEntrySheet`, dan `RevealableSecretText`. Nilai yang ditampilkan tersembunyi lagi setelah 30 detik, saat view hilang, atau saat app tidak aktif. Copy menulis pasteboard khusus host ini (`currentHostOnly`) dengan tipe `org.nspasteboard.ConcealedType` dan `org.nspasteboard.TransientType`, lalu mengosongkannya setelah 90 detik atau saat app ditutup, kalau `changeCount` belum berubah. Replace yang efeknya `stored_only` butuh konfirmasi eksplisit. Semua komponen bisa dipakai penuh dengan keyboard, punya label VoiceOver, mengikuti Dynamic Type, memenuhi kontras WCAG AA, dan nilai yang diketik tidak pernah masuk `UserDefaults`, log publik, atau state yang dipersist (AC-016).

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md): login Keychain lewat crate `security-framework` yang hanya disentuh `stacklyd`, reference `local:<path>`, satu use case `put` dengan sumber `value`, `generate`, `copyFrom`, dan `bind`, dialog Keychain hanya untuk aksi pengguna, redaction tiga lapis, reveal dengan satu klik eksplisit yang dicatat, dan config plaintext hanya sebagai jalan terakhir dengan aturan ketat.

Keychain adalah satu satunya tempat nilai secret, daemon adalah satu satunya pembacanya, dan setiap jalur keluar (log, IPC, CLI, GUI, diagnostics, export) lewat satu `Redactor`, sehingga kebocoran harus melewati tiga penjaga sekaligus.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Skill async relevan untuk `SecretWorker` dan pemanggilan store dari kode tokio lewat `spawn_blocking`. Untuk sisi Swift, skill di [apps/macos/AGENTS.md](../../../apps/macos/AGENTS.md) berlaku seperti biasa.

## Feature design

### Batas fitur

- **Di dalam**: port `SecretStore` dan adapter Keychain macOS, format dan validasi reference, use case isi, bangkitkan, salin, ikat, dan ganti, status dan probe, reveal dan copy, delete, `NeedsSecret` dan blocker instance, aturan pengiriman secret ke provider (OQ-008), `Redactor` dan penerapannya di log daemon, IPC, dan CLI, input aman CLI, komponen GUI untuk mengisi dan menampilkan secret, sisi Swift fixture credential, dan suite sentinel.
- **Di luar**: rotasi credential di dalam provider (misalnya `ALTER ROLE`), adapter Windows, data protection keychain (dibahas ulang di fitur 15), secret manager eksternal (1Password dan sejenisnya, skema reference lain dicadangkan), sinkronisasi iCloud, berbagi secret antar user, UI audit (fitur 35), bundle diagnosis (fitur 34), export environment (fitur 45), pipeline log instance (fitur 27), dan primitive hash bcrypt (fitur 41). Fitur fitur itu menerapkan aturan spec ini lewat `Redactor` dan `SecretService::put`.

### Peta modul dan dependency

Aturan arah dependency spec 0001 dan 0004 tetap berlaku. Spec ini menambah:

| Crate | Dependency langsung yang baru | Modul baru |
| --- | --- | --- |
| `stackly-domain` | tidak ada | `secret` (`SecretRef`, `SecretRefError`, `SecretOwner`, `SecretPurpose`, `SecretStatus`, `SecretOrigin`, `NeedsSecretReason`, `InstanceBlocker`) |
| `stackly-platform` | `zeroize` | `secret` (trait `SecretStore`, `SecretValue`, `Interaction`, `WriteMode`, `SecretPresence`, `StoreState`, `SecretBackend`, `SecretStoreError`, `UnsupportedSecretStore`) |
| `stackly-protocol` | `zeroize` (fitur `serde`) | `secret` (`SecretText`, `SecretInput`, dan DTO di *Permukaan API*) |
| `stackly-core` | `getrandom`, `regex`, `sha2` | `secret::{service, worker, generate, redact, delivery}` |
| `stackly-platform-macos` | `security-framework` (plus `security-framework-sys` hanya kalau status kunci keychain tidak punya wrapper aman) | `secret` (`MacosSecretStore`, `KeychainTarget`, `map_status`) |
| `stackly-testkit` | tidak ada | `fake_secret` (`FakeSecretStore`) |
| `stacklyd` | tidak ada | `ipc::secret`, `log::redacting_writer` |
| `stackly-cli` | `rpassword` | `secret_input`, `commands::secret` |
| `xtask` | tidak ada | aturan baru di `check_deps` |
| `StacklyTestSupport` (Swift) | tidak ada | `FixtureCredential` |
| `StacklyClient` (Swift) | tidak ada | method secret dan `RevealedSecret` |
| `StacklyUI` (Swift) | tidak ada | `SecretField`, `SecretEntrySheet`, `RevealableSecretText`, `ConcealedPasteboard` |

Semua versi crate masuk `[workspace.dependencies]` sebagai rentang caret dan dikunci `Cargo.lock`; versi persisnya dipilih saat build, setelah `cargo deny check` lulus. Crate pihak ketiga baru berlisensi MIT atau Apache-2.0. Kode Keychain memakai wrapper aman; satu blok `unsafe` dengan `// SAFETY:` di `stackly-platform-macos` hanya diizinkan untuk membaca status kunci keychain kalau `security-framework` tidak menyediakannya.

### Format secret reference

```text
reference := "local:" segment ("/" segment){1,7}
segment   := [a-z0-9] [a-z0-9._-]{0,63}      (bukan "." dan bukan "..")
panjang   := paling banyak 255 byte
```

- `local` berarti "secret store milik user ini di Mac ini". Nilainya diisi lokal oleh setiap orang dan tidak pernah ikut dibagikan, jadi `stackly.yaml` yang sama berlaku di Mac lain dan nanti di Windows.
- Reference buatan daemon untuk instance: `local:instance/<instance-id>/<purpose>`, dengan `instance-id` UUID huruf kecil.
- Purpose diturunkan dari nama secret field (yang menurut spec 0009 cocok dengan `[a-z][A-Za-z0-9]*` dan unik per manifest): sisipkan `-` sebelum setiap huruf besar yang mengikuti huruf kecil atau angka, lalu kecilkan semua huruf. Contoh: `adminPassword` menjadi `admin-password`, `tlsKeyPEM` menjadi `tls-key-pem`, `token2Fa` menjadi `token2-fa`. Hasil lebih dari 64 byte ditolak saat create.
- Reference di `stackly.yaml` ditulis pengguna, misalnya `local:erp/postgres/admin` seperti contoh PRD bagian 13.1.
- Segmen pertama `instance` hanya dibuat daemon. Segmen pertama `stackly` dicadangkan untuk secret internal nanti. Skema selain `local` dicadangkan untuk penyedia lain dan ditolak sekarang.
- Reference bukan secret. Ia boleh muncul di log, GUI, CLI, diagnostics, dan audit.

### Model data (Assumed: spec 0005)

Skema fisik milik spec 0005. Spec ini membutuhkan dua tabel logis berikut.

`secret_references`:

| Field | Tipe | Wajib | Catatan |
| --- | --- | --- | --- |
| `id` | UUID v7 | PK | dibuat daemon |
| `reference` | text | ya, unik | teks `local:<path>` yang sudah lolos `SecretRef::parse` |
| `platform_store_key` | text | ya | hasil `SecretStore::store_key`, misalnya `macos-keychain:login/stackly-secret/instance/<id>/admin-password`; diperbarui setiap `put` dan probe |
| `owner_kind` | `instance`, `environment`, atau `user` | ya | ditetapkan saat baris dibuat |
| `owner_id` | UUID | null hanya kalau `owner_kind` `user` | tanpa foreign key karena polimorfik, dijaga check constraint |
| `purpose` | text | ya | purpose field, key `secretRefs` yang diturunkan dengan algoritma yang sama, atau `user` |
| `value_origin` | `generated`, `entered`, atau `copied` | null sampai nilai pertama tersimpan | |
| `status` | `present`, `missing`, atau `unknown` | ya | cache observasi Keychain terakhir |
| `status_observed_at` | timestamp UTC | null | |
| `value_updated_at` | timestamp UTC | null | terakhir diisi lewat Stackly |
| `revision` | integer | ya | mulai 1, naik hanya oleh `put` dan pemulihan setelah delete gagal; observasi tidak menaikkannya |
| `created_at`, `updated_at` | timestamp UTC | ya | |

`secret_reference_uses`:

| Field | Tipe | Catatan |
| --- | --- | --- |
| `reference_id` | FK ke `secret_references.id` | `ON DELETE RESTRICT` |
| `resource_kind` | `instance` atau `environment` | |
| `resource_id` | UUID | |
| `field` | text | nama secret field di manifest provider atau key `secretRefs` di environment |
| PK | (`reference_id`, `resource_kind`, `resource_id`, `field`) | ditulis dalam transaksi yang sama dengan config revision atau environment yang memakainya |

Relasi: satu reference punya banyak pemakaian (1:N), satu instance atau environment memakai banyak reference (1:N lewat `secret_reference_uses`), dan satu instance atau environment memiliki banyak reference (1:N lewat `owner_id`). Tidak ada kolom nilai di mana pun, dan `config_revisions.values` hanya memuat teks reference untuk field secret. Pemakaian sebuah instance selalu mencerminkan config revision aktifnya: saat revision baru aktif, baris pemakaian lama instance itu diganti dalam transaksi yang sama.

Aturan pemilik: pemilik ditetapkan saat baris pertama kali dibuat dan tidak pernah berubah, kecuali satu kasus. Env up yang memakai reference yang sudah ada, dan `bind`, hanya menambah baris pemakaian. Saat pemilik instance atau environment dihapus dan reference nya masih dipakai resource lain, pemiliknya menjadi `user` (dicatat di log dan audit); kalau tidak dipakai lagi, reference dihapus lewat use case delete.

### Port `SecretStore`

Trait baru di `stackly-platform`, sync, object safe, dan `Send + Sync`, seperti port lain di spec 0004:

```rust
pub trait SecretStore: Send + Sync {
    fn backend(&self) -> SecretBackend;
    fn state(&self) -> StoreState;
    fn store_key(&self, reference: &SecretRef) -> String;
    fn probe(&self, reference: &SecretRef) -> Result<SecretPresence, SecretStoreError>;
    fn read(&self, reference: &SecretRef, interaction: Interaction) -> Result<SecretValue, SecretStoreError>;
    fn write(&self, reference: &SecretRef, value: &SecretValue, mode: WriteMode, interaction: Interaction) -> Result<(), SecretStoreError>;
    fn delete(&self, reference: &SecretRef, interaction: Interaction) -> Result<(), SecretStoreError>;
}
```

- `SecretValue`: membungkus `zeroize::Zeroizing<String>`. `SecretValue::new(String)` menolak nilai kosong, lebih dari `MAX_SECRET_LEN` (65536 byte), atau berisi NUL dengan `SecretValueError`. `expose() -> &str` satu satunya cara membaca isinya. `Debug` mencetak `SecretValue(****)`. Tidak ada `Display`, `Clone`, `PartialEq`, `Serialize`, atau `Deserialize`. Konstanta `SCRUB_MIN_LEN` bernilai 8 dan `SCRUB_ANYWHERE_LEN` bernilai 16.
- `state()` mengembalikan cache keadaan terakhir yang diamati dan tidak pernah memanggil Keychain. `StoreState`: `Available`, `Locked`, `Unsupported`, atau `Unknown` (belum ada observasi).
- `store_key` deterministik dan tanpa IO.
- `probe` tidak pernah memicu dialog dan berjalan dengan interaksi `Never`.
- `Interaction`: `AllowPrompt` atau `Never`. `WriteMode`: `Create`, `Replace`, atau `Upsert`. `SecretPresence`: `Present` atau `Missing`. `SecretBackend`: `MacosKeychain`, `Fake`, atau `Unsupported`, dengan kode snake_case stabil.
- `SecretStoreError` (`#[non_exhaustive]`): `NotFound`, `AlreadyExists`, `Locked`, `AccessDenied { by_user }`, `Busy`, `OutcomeUnknown`, `NotSupported { os }`, dan `Backend { code }`.
- `Platform` mendapat port keenam `secrets() -> &dyn SecretStore`. `UnsupportedPlatform` memakai `UnsupportedSecretStore`, yang menjawab `NotSupported` untuk setiap method fallible dan `Unsupported` untuk `state()`. `MacosPlatform` melaporkan capability `secret_store` supported setelah milestone 1.

### Adapter Keychain macOS

- `MacosSecretStore::new(target)` dengan `KeychainTarget::Login` atau `KeychainTarget::File { path, password_file }`. `stacklyd` memilih `Login` untuk root default dan `File` di `R/data/keychain/` untuk `STACKLY_ROOT=R`; integration test memakai `File` di bawah `TestRoot`. Jalur produksi root default hanya memakai fungsi `SecItem`.
- Target `File` membuka keychain di `path`, atau membuatnya kalau belum ada, dengan password acak 32 karakter yang ditulis ke `password_file` (0600, di folder 0700) lewat `write_file_atomic`; keychain itu disetel tanpa kunci otomatis dan tanpa kunci saat sleep, dibuka dengan password itu, dan tidak pernah ditambahkan ke search list user. API keychain file itu deprecated di macOS dan hanya dipakai untuk root override dan test.
- Setiap operasi dipatok ke satu keychain: `kSecUseKeychain` saat menambah dan `kSecMatchSearchList` berisi keychain itu saja saat mencari, mengganti, dan menghapus. Untuk target `Login`, keychain itu adalah login Keychain user.
- Item: class generic password, service `stackly-secret`, account path reference tanpa `local:`, nilai berupa byte UTF-8 apa adanya, label `Stackly <reference>` dan description `Stackly secret` supaya terbaca di Keychain Access, tanpa synchronizable, dan dengan ACL bawaan (proses pembuat, yaitu `stacklyd`, dipercaya). Data protection keychain tidak dipakai.
- `store_key`: `macos-keychain:login/stackly-secret/<account>` atau `macos-keychain:file:<path relatif terhadap root data>/stackly-secret/<account>`.
- `probe` meminta atribut saja (`kSecReturnAttributes`, tanpa data).
- `write` dengan `Create` memakai `SecItemAdd`, dengan `Replace` memakai `SecItemUpdate`, dan dengan `Upsert` mencoba add lalu update saat item sudah ada. Pembaruan nilai atomik di sisi Keychain.
- Interaksi: konstruktor mematikan interaksi pengguna Security framework untuk seluruh proses dan membiarkannya mati. Panggilan `AllowPrompt` menyalakannya selama satu panggilan lalu mematikannya lagi. Adapter memegang mutex sendiri di setiap method, jadi saklar global aman walau adapter dipanggil dari mana pun; `SecretWorker` menambahkan batas tunggu di atasnya.
- Spike di langkah 3 Build plan mencoba `kSecUseAuthenticationUI` dengan nilai fail per query. Kalau macOS 27 menghormatinya untuk dialog buka kunci dan dialog ACL di keychain file, `Never` memakai atribut itu dan saklar global hanya dipakai untuk menyalakan interaksi. Hasilnya dicatat di bagian Cross check rationale lewat `/architect` update.
- Pemetaan `OSStatus` lewat `map_status(code, keychain_locked)`, dengan `keychain_locked` dibaca dari status kunci keychain target hanya saat kode itu ambigu:

| Kode | Hasil |
| --- | --- |
| `errSecItemNotFound` | `Locked` kalau keychain terkunci, selain itu `NotFound` |
| `errSecDuplicateItem` | `AlreadyExists` |
| `errSecInteractionNotAllowed` | `Locked` kalau keychain terkunci, selain itu `AccessDenied { by_user: false }` |
| `errSecAuthFailed` | `Locked` kalau keychain terkunci, selain itu `AccessDenied { by_user: true }` |
| `errSecUserCanceled` | `AccessDenied { by_user: true }` |
| lain | `Backend { code }` |

- Tabel itu dikonfirmasi spike di langkah 3: integration test merekam kode nyata untuk keychain terkunci dan ACL asing dengan `Never`, dan record manual merekam kode untuk Deny dan Cancel dengan `AllowPrompt`. Kalau kode nyata berbeda, tabel diperbarui lewat `/architect` update spec ini sebelum milestone 1 selesai.
- Setiap hasil memperbarui cache `state()`: sukses menjadi `Available`, `Locked` menjadi `Locked`.

### Thread Keychain dan batas tunggu

- `SecretWorker::spawn(store, WorkerConfig { prompt_wait, background_wait })` di core membungkus `Arc<dyn SecretStore>` apa pun dan mengembalikan `Arc<dyn SecretStore>` yang menjalankan setiap panggilan fallible di satu thread std miliknya, berurutan, lewat channel. `state()`, `backend()`, dan `store_key()` dijawab langsung tanpa antre. Default `WorkerConfig` adalah 180 detik dan 10 detik; test memakai durasi pendek.
- Setiap job membawa batas waktu yang dihitung dengan jam monotonik sejak masuk antrean: `prompt_wait` untuk `AllowPrompt`, `background_wait` untuk `Never` dan `probe`.
- Job yang batasnya sudah lewat saat giliran tiba tidak dijalankan; pemanggil sudah menerima `Busy`.
- Job yang sedang berjalan saat batasnya lewat tidak bisa dibatalkan (dialog Keychain menahan thread sampai pengguna menjawab). Pemanggil menerima `Busy` untuk `read` dan `probe`, atau `OutcomeUnknown` untuk `write` dan `delete`. Setelah job itu selesai, worker mencatat satu baris info berisi reference dan jenis hasil tanpa nilai, dan untuk `write` atau `delete` memanggil `SecretService::probe_one` supaya cache status benar.
- `stacklyd` membuat satu worker saat start dan memasangnya di `Platform`. Use case core dipanggil dari tokio lewat `spawn_blocking`, sesuai pola spec 0004.

### Use case isi atau ganti nilai

`SecretService::put(target, input, mode, expected_revision, interaction)`:

- `target` adalah `PutTarget::Existing(SecretRef)` (untuk `secret.set` dan pengisian setelah needs secret) atau `PutTarget::New { reference, owner, purpose }` (untuk create instance, env up, dan restore, yang membuat baris dan pemakaian dalam transaksi operasinya sendiri).
- `input` adalah `SecretInput`: `Value(SecretValue)`, `Generate`, `CopyFrom(SecretRef)`, atau `Bind(SecretRef)`. `Bind` hanya sah untuk resource baru (create instance, env up): ia tidak menulis nilai, hanya menambah baris pemakaian ke reference yang sudah ada, dan reference itu harus terdaftar.

Urutan:

1. Parse reference (AC-1). `secret.set` untuk reference yang belum terdaftar membuat baris `owner_kind` `user`, kecuali segmen pertamanya `instance` (ditolak `secret.not_found`, karena reference instance hanya ada kalau daemon yang membuatnya) atau `stackly` (ditolak `secret.invalid_reference`, karena dicadangkan). Ambil lock per reference di memori daemon sampai langkah 7 selesai.
2. Cek registry: `expectedRevision` yang tidak cocok gagal `secret.conflict` di sini, tanpa menyentuh Keychain.
3. Siapkan nilai: `Value` lewat `SecretValue::new`; `Generate` lewat `generate_password()`; `CopyFrom` dengan membaca reference sumber dari store memakai interaksi permintaan ini (reference sumber yang tidak terdaftar atau tidak punya nilai gagal `secret.not_found`). Langsung `Redactor::learn` untuk nilai itu. Nilai tidak pernah keluar ke klien.
4. Probe Keychain dan cek mode terhadap hasilnya: `Create` pada item yang ada gagal `secret.conflict` dengan saran memakai replace; `Replace` pada item yang tidak ada gagal `secret.not_found`. Untuk `Replace` atau `Upsert` pada item yang ada, baca nilai lama (interaksi permintaan ini), simpan di memori, dan `Redactor::learn` nilai itu.
5. Kalau efek salah satu pemakaian adalah `stored_only` dan nilai lama ada, permintaan harus membawa `confirmStoredOnly: true`; tanpa itu gagal `secret.confirmation_required` dengan daftar efeknya, tanpa menulis apa pun.
6. Tulis ke store dengan `WriteMode` yang sesuai.
7. Dalam satu transaksi registry: perbarui atau buat baris (status `present`, `status_observed_at`, `value_origin` `entered`, `generated`, atau `copied`, `value_updated_at`, `platform_store_key`, revision naik 1 atau 1 untuk baris baru) dan tulis baris audit `secret.set`. Kalau transaksi gagal, tulis balik nilai lama ke store (atau hapus item yang baru dibuat), lalu kembalikan error registry; kalau penulisan balik juga gagal, kembalikan `secret_store.outcome_unknown`.
8. Lepas lock, kirim event `secret.changed`, dan kembalikan ringkasan, peringatan, dan `effects`.

- `effects` dihitung per pemakaian dari klasifikasi field di manifest provider (Assumed: spec 0009) dan `data.initializedAt` instance: field `immutable_after_init` pada instance yang datanya sudah diinisialisasi menghasilkan `stored_only` (nilai di dalam database tidak berubah), field `reloadable` menghasilkan `next_reload`, dan selain itu `next_start`. `put` tidak pernah melakukan rotasi di dalam provider.
- Kegagalan apa pun setelah langkah 1 juga menulis baris audit `secret.set` dengan hasil `failed:<kode error>`, dalam transaksi tersendiri.
- Create instance memanggil `put` untuk setiap secret field dengan `PutTarget::New` dan `Generate`, kecuali pemanggil mengirim `SecretInput` lain untuk field itu. Env up memanggil `put` dengan `Bind` (atau mendaftarkan reference baru milik environment tanpa nilai) untuk setiap `secretRefs`, lalu berhenti dengan `needs_secret` kalau ada yang belum terisi. Restore as new membuat reference baru untuk instance hasil restore dan menawarkan `CopyFrom` reference instance sumber atau `Value`.

### Status dan probe

- Saat start, daemon memanggil `probe_all`: setiap reference terdaftar dicek dengan `probe` lewat worker, lalu cache status dan `platform_store_key` diperbarui tanpa menaikkan revision. Log hanya mencatat jumlah per status.
- `probe_one` memegang lock per reference yang sama dengan `put` dan `delete`, jadi observasi lama tidak pernah menimpa hasil `put` yang lebih baru.
- `secret.status` selalu probe ulang satu reference. `secret.list` membaca cache.
- Status `unknown` berarti probe gagal (keychain terkunci, store tidak didukung, worker sibuk, atau error backend).
- Kalau `platform_store_key` yang tersimpan berbeda dengan `store_key` yang dihitung (misalnya registry disalin ke root lain), probe memakai kunci yang dihitung, status menjadi `missing` bila item tidak ada di sana, dan daemon mencatat satu peringatan berisi kedua kunci. Kunci lama tidak pernah dibaca diam diam; kamu mengisi ulang lewat `secret.set`.
- `daemon.status` memuat `secretStore { backend, state, target }`, dengan `target` bernilai `login` atau `file`.

### Reveal dan copy

- `secret.reveal { reference, purpose }` membaca nilai dengan interaksi dari context klien, memanggil `Redactor::learn`, menulis baris audit `secret.reveal`, lalu mengembalikan `SecretText`. Kalau baris audit gagal ditulis, respons adalah error registry tanpa nilai.
- CLI: `stackly secret reveal <ref>` mencetak nilai ke stdout diikuti newline, atau `{"reference": ..., "value": ...}` dengan `--json`. Command lain tidak pernah mencetak nilai.
- Swift: `StacklyClient.revealSecret` mengembalikan `RevealedSecret { reference, value }`, tipe buatan tangan (bukan DTO bangkitan) yang `description`, `debugDescription`, dan `customMirror` nya mencetak `RevealedSecret(<reference>, ****)`.
- GUI: `RevealableSecretText` menampilkan teks yang sudah disamarkan daemon (misalnya URI dengan `****`) beserta tombol Show Password dan Copy. Show memanggil `revealSecret(purpose: .show)` dan menampilkan nilai sampai salah satu dari ini terjadi lebih dulu: 30 detik berlalu, pengguna menekan Hide, view hilang, atau app tidak aktif. Copy memanggil `revealSecret(purpose: .copy)`, menyusun teks lengkap lewat closure pemanggil (misalnya URI dengan password asli), lalu menulis pasteboard lewat `ConcealedPasteboard`.
- `ConcealedPasteboard` memanggil `prepareForNewContents(with: .currentHostOnly)` supaya tidak ikut Universal Clipboard, menulis string beserta tipe `org.nspasteboard.ConcealedType` dan `org.nspasteboard.TransientType`, menyimpan `changeCount`, lalu mengosongkan pasteboard setelah 90 detik atau saat app ditutup kalau `changeCount` masih sama.

### Hapus

`secret.delete { reference, expectedRevision? }`:

1. Ambil lock per reference dan cek `expectedRevision`.
2. Satu transaksi registry: hapus baris (foreign key `RESTRICT` membuatnya gagal kalau ada pemakaian, termasuk pemakaian yang baru saja ditambahkan resource lain; daemon menjawab `secret.conflict` dengan `usedBy`).
3. Hapus item Keychain dengan interaksi permintaan ini. Item yang memang sudah tidak ada dianggap berhasil.
4. Kalau langkah 3 gagal, buat ulang baris dengan isi yang sama dan revision naik 1, lalu kembalikan error store. Crash di antara langkah 2 dan 3 meninggalkan item Keychain yatim tanpa baris, yang ditangani inventaris fitur 57.
5. Tulis baris audit `secret.delete` (berhasil atau gagal) dan event `secret.changed` dengan status `deleted`.

Purge instance (fitur 54) memanggil use case yang sama untuk reference milik instance itu setelah pemakaiannya hilang, mengikuti aturan pemilik di *Model data*.

### Needs secret

- `resolve_for_start(resource, interaction)` membaca reference dari pemakaian aktif resource itu dan selalu memanggil `read` lewat worker, tanpa mempercayai cache. Interaksinya dari pemicu: `AllowPrompt` untuk aksi pengguna dari klien interaktif, `Never` untuk autostart, reconcile, restart policy, dan klien noninteractive. Operasi yang dimulai aksi pengguna membawa interaksi permintaan asalnya sampai selesai, walau klien sudah putus (Assumed: spec 0007).
- Reference dibaca urut teks. Setelah satu `AccessDenied { by_user: true }`, reference sisanya tidak dibaca dan langsung tercatat `access_denied`, supaya pengguna tidak dihujani dialog. `NotFound` memperbarui cache menjadi `missing`.
- Hasilnya `ResolvedSecrets` (peta field ke `SecretValue`, tanpa konstruktor publik lain dan tanpa `Default`) atau `NeedsSecret { references }` yang selalu mendaftar semua reference bermasalah, urut teks reference. Kalau ada error `Backend`, hasilnya error backend itu, bukan needs secret.
- Pemetaan alasan: `NotFound` menjadi `missing`, `Locked` menjadi `store_locked`, `AccessDenied` menjadi `access_denied`, `Busy` dan `OutcomeUnknown` menjadi `store_busy`, dan `NotSupported` menjadi `store_unsupported`.
- `generateAllowed` bernilai false kalau salah satu pemakaian reference itu adalah field `immutable_after_init` pada instance yang datanya sudah diinisialisasi (`data.initializedAt` terisi, termasuk instance hasil restore as new, yang datanya berasal dari backup); nilai bangkitan di sana tidak akan cocok dengan credential di dalam data. Selain itu true.
- `stacklyd` memetakan `NeedsSecret` ke kode error `needs_secret` dengan detail `references: [{ reference, reason, generateAllowed }]` dan recovery hint "isi lewat `stackly secret set <ref>` atau buka kunci Keychain, lalu ulangi".
- Instance mendapat observasi `InstanceBlocker::NeedsSecret { references }`, yang muncul di DTO instance sebagai field `blocker` (`{type: needsSecret, content: {references}}`) dan diumumkan lewat event perubahan instance (Assumed: spec 0006). Blocker tidak mengubah lifecycle apa pun (`stopped`, `failed`, atau `recovering`), desired state, atau revision. Ia disimpan di memori daemon bersama observasi lain, hilang saat `resolve_for_start` berikutnya berhasil atau saat `secret.changed` membuat semua reference nya `present`, dan dihitung ulang setelah daemon restart oleh percobaan start berikutnya.
- Hilangnya blocker tidak memulai instance. Percobaan ulang datang dari pengguna (misalnya tombol di `SecretEntrySheet`) atau dari kebijakan autostart fitur 31.

### Pengiriman secret ke provider (OQ-008)

Urutan mekanisme. Manifest provider memilih yang tertinggi yang didukung tool itu (Assumed: spec 0009):

| Urutan | Mekanisme | Aturan |
| --- | --- | --- |
| 1 | `stdin` | payload dari `stdin_payload(&SecretValue)` ditulis sekali ke stdin proses lalu pipe ditutup |
| 2 | `hash` | config menyimpan hash dari `hash(HashAlgo, &SecretValue)`, bukan nilainya. Algoritma di fitur ini: `sha256_hex` (ClickHouse). bcrypt untuk htpasswd Traefik ditambahkan fitur 41 |
| 3 | `file_once` | `SecretFile::create(fs, dir, name, &SecretValue)` menulis file 0600 di `<runtime>/i/<short-key>/secrets/<name>`; guard nya menghapus file saat drop, jadi file hilang setelah langkah yang memakainya selesai, berhasil atau gagal (misalnya `initdb --pwfile`) |
| 4 | `env` | `env_pair(name, &SecretValue)` untuk environment eksplisit milik child; tidak pernah diwarisi dari environment daemon |
| 5 | `config_plaintext` | file include terpisah 0600 di `<runtime>/i/<short-key>/secrets/<name>`, dirender ulang dari Keychain setiap start dan reload lewat `render_secret_placeholder`, dihapus setelah stop bersih, disapu saat daemon start untuk instance yang tidak berjalan. Kalau tool tidak bisa include file terpisah, seluruh config hasil render diperlakukan sebagai secret material dengan aturan yang sama |
| tidak pernah | argv | manifest yang menaruh secret di argv ditolak saat validasi, dan `ensure_not_in_argv` menjadi penjaga terakhir sebelum spawn |

- `name` untuk file adalah purpose field (`admin-password`) atau nama file include dari manifest, yang harus cocok dengan pola segmen reference.
- Folder `secrets/` ada di runtime root (folder temp privat milik user), bukan di root data, supaya tidak ikut Time Machine, backup registry, cold backup, `config-history`, tampilan config, atau diagnostics. Runtime root memang hanya berisi file yang bisa dibuat ulang (spec 0004), dan file ini bisa dibuat ulang dari Keychain.
- Modul `secret::delivery` di core memegang semua fungsi di atas, plus `remove_secret_material(fs, dir)`, `sweep_secret_material(fs, layout, running)`, dan `ensure_not_in_argv(args, &ResolvedSecrets)`. `ensure_not_in_argv` menolak argumen yang sama persis dengan nilai mana pun, argumen berbentuk `<apa pun>=<nilai>`, dan argumen yang memuat nilai sepanjang `SCRUB_ANYWHERE_LEN` atau lebih, dengan error `SecretInArgv` yang dipetakan ke kode `secret.delivery_rejected` tanpa menyebut nilainya.
- Hash bukan nilai secret dan boleh tinggal di config biasa. Nilai lemah yang diketik pengguna bisa ditebak offline dari hash; ini residual yang dinyatakan.
- Residual risk yang dinyatakan: selama instance berjalan, proses lain milik user yang sama bisa membaca file config plaintext dan environment child. Penghapusan file di APFS tidak menghapus isinya secara fisik, dan macOS bisa membersihkan file temp lama selama instance berjalan lama, jadi reload selalu merender ulang lebih dulu. Hal ini dicatat di dokumentasi pengguna (fitur 66) dan di review keamanan (fitur 63).

### Redaction

Tiga lapis, semuanya wajib di setiap jalur keluar:

1. **Tipe**: nilai hanya hidup di `SecretValue` (proses Rust), `SecretText` (wire), dan `RevealedSecret` (Swift). Ketiganya menyamarkan representasi debug, dan dua yang pertama menghapus isinya saat drop.
2. **Nilai yang dikenal**: `Redactor` menyimpan salinan setiap nilai sepanjang `SCRUB_MIN_LEN` atau lebih yang dibaca, diisi, disalin, atau dibangkitkan selama run daemon ini, termasuk nilai lama sebelum replace, di penyimpanan yang dihapus dari memori saat drop. Nilai `SCRUB_ANYWHERE_LEN` atau lebih diganti di mana pun muncul. Nilai 8 sampai 15 byte hanya diganti kalau muncul sebagai token utuh, yaitu tidak diapit huruf atau angka di kiri dan kanannya (jadi `postgres` di dalam `postgresql` tidak disentuh, tetapi `user:postgres@` tersamar). Nilai terpanjang dicocokkan lebih dulu.
3. **Pola**, dengan grammar normatif berikut:
   - **Kunci sensitif** (huruf besar kecil tidak dibedakan): `password`, `passwd`, `pwd`, `secret`, `token`, `api_key`, `apikey`, `access_key`, `private_key`, `authorization`, `requirepass`, `masterauth`, `pgpassword`, `mysql_pwd`, `rediscli_auth`. Kunci cocok kalau didahului awal baris atau karakter selain huruf dan angka (termasuk `_`, jadi `POSTGRES_PASSWORD` cocok lewat `password`), boleh diapit tanda kutip, lalu diikuti spasi opsional, `=` atau `:`, dan spasi opsional. Kunci yang hanya diikuti spasi tidak cocok, supaya baris seperti `password authentication failed` tetap utuh.
   - **Nilai setelah kunci**: kalau diawali `"` atau `'`, nilai berakhir di tanda kutip yang sama berikutnya dan isinya diganti `****` dengan tanda kutip tetap ada. Kalau tidak, nilai berakhir sebelum spasi, `&`, `,`, `;`, `)`, `}`, `]`, tanda kutip, atau akhir baris. Untuk kunci `authorization`, nilai adalah sisa baris, jadi `Authorization: Bearer x` dan `Authorization: Basic x` tersamar seluruhnya.
   - **Userinfo URI**: untuk `<scheme>://<userinfo>@<host>`, dengan scheme `[A-Za-z][A-Za-z0-9+.-]*`, password adalah semua karakter di antara `:` pertama setelah `://` dan `@` terakhir sebelum `/`, `?`, `#`, spasi, atau akhir baris. Hasilnya `<scheme>://<user>:****@<host>`. Userinfo tanpa `:` tidak diubah.
- Penanda samaran selalu `****`, panjangnya tetap, di semua permukaan teks. GUI menampilkan teks samaran dari daemon apa adanya; field input memakai bullet sistem dari `SecureField`.
- `Redactor` adalah fungsi murni di core: `redact(&str) -> Cow<str>` dan `learn(&SecretValue)`. Ia dibagi sebagai `Arc<Redactor>` di daemon.
- Setelah daemon start dan reconcile, `SecretService::learn_running(resources)` membaca nilai reference yang dipakai instance yang sedang berjalan atau diadopsi (interaksi `Never`) hanya untuk `Redactor::learn`, supaya log provider yang diadopsi tetap tersaring sebelum ada start baru. Kegagalan baca dicatat sebagai jumlah per alasan dan tidak memblokir apa pun.
- Penerapan yang dibangun di fitur ini: writer log `stacklyd` (setiap baris setelah diformat), pesan dan detail envelope error, progress, dan event sebelum dikirim. Penerapan berikutnya milik fitur pemiliknya: log instance sebelum disimpan (fitur 27), bundle diagnosis (fitur 34), `safe_details` audit (fitur 35), dan export environment (fitur 45).
- Daemon dan CLI tidak pernah mencatat isi frame atau body permintaan; log IPC hanya method, request ID, dan ukuran.
- Bentuk yang di encode, dipotong, atau disamarkan sebagian tidak tertangkap lapis 2, dan nilai di bawah 8 byte hanya dilindungi lapis 1 dan 3. Ini residual yang dinyatakan, sama seperti batas pindaian di spec 0002.

### Penjaga di kode

`cargo xtask check-deps` memindai token stream setiap file source (lewat `syn` dan `proc-macro2`, termasuk isi semua makro) dan menolak setiap pemanggilan method bernama `expose` (pola token `. expose (`) di luar modul berikut, di luar modul `#[cfg(test)]` dan folder `tests/`. Tipe lain tidak boleh memberi nama `expose` pada method nya.

| Crate | Modul yang diizinkan | Alasan |
| --- | --- | --- |
| `stackly-platform` | `secret` | validasi di konstruktor |
| `stackly-protocol` | `secret` | konversi milik tipe itu sendiri |
| `stackly-platform-macos` | `secret` | menulis dan membaca Keychain |
| `stackly-testkit` | `fake_secret` | menyimpan nilai di fake |
| `stackly-core` | `secret::redact`, `secret::delivery` | mencocokkan nilai dan mengirimnya ke provider |
| `stacklyd` | `ipc::secret` | memetakan `SecretText` dan `SecretValue` |
| `stackly-cli` | `secret_input`, `commands::secret` | membaca input dan mencetak hasil reveal |

Aturan dependency: `security-framework` (dan `security-framework-sys`) hanya boleh menjadi dependency langsung `stackly-platform-macos`, dan `rpassword` hanya `stackly-cli`.

### Input aman CLI

Kontrak ini dipakai `stackly secret set` dan setiap command lain yang butuh nilai secret (create instance, restore), dan dibangun di atas CLI fitur 8:

| Sumber | Flag | Aturan |
| --- | --- | --- |
| TTY | tanpa flag, kalau stdin dan stderr adalah TTY dan mode noninteractive tidak aktif | prompt di stderr, dibaca dari `/dev/tty` dengan echo mati lewat `rpassword`, ditanya dua kali untuk nilai baru; kalau berbeda, diulang paling banyak tiga kali lalu gagal invalid input |
| stdin | `--stdin` | membaca sampai EOF, paling banyak 65537 byte (lebih dari itu ditolak), lalu membuang tepat satu `\n` atau `\r\n` di akhir |
| file | `--from-file <path>` | file biasa, tidak mengikuti symlink, paling banyak 65537 byte, newline di akhir dibuang sama seperti stdin; izin file yang terbaca group atau other menghasilkan peringatan di stderr |
| env | `--from-env <NAMA>` | nilai variabel apa adanya; variabel yang tidak ada atau kosong ditolak |
| bangkitkan | `--generate` | daemon yang membangkitkan |
| salin | `--copy-from <ref>` | daemon yang menyalin |

- Tepat satu sumber per nilai. Dua sumber sekaligus ditolak. Tanpa TTY dan tanpa flag sumber, command gagal invalid input tanpa menunggu.
- CLI interaktif kalau stdin dan stderr adalah TTY dan mode noninteractive fitur 8 tidak aktif; hanya CLI interaktif yang mengirim permintaan dengan interaksi `AllowPrompt`.
- Mode tulis: default `create`, `--replace`, atau `--upsert`. `--expected-revision <n>` opsional. `--confirm-stored-only` mengisi `confirmStoredOnly`; tanpa flag itu, CLI interaktif menampilkan daftar efek `stored_only` dan meminta konfirmasi `y` di TTY, sedangkan CLI noninteractive gagal `secret.confirmation_required`.
- Tidak ada flag yang menerima nilai secret, dan nilai yang terbaca langsung dibungkus `SecretText`.

### GUI

- `SecretField`: input untuk satu secret field dengan pilihan Generate (default, hanya kalau diizinkan), Enter (`SecureField`, dengan tombol tampilkan sementara saat mengetik), dan Copy from (memilih reference lain). Dipakai sheet New Instance (fitur 20).
- `SecretEntrySheet`: daftar reference yang dibutuhkan beserta status dan alasannya (dari detail `needs_secret` atau preflight restore), satu `SecretField` per reference dengan pilihan Generate hanya kalau `generateAllowed` true, dan tombol simpan per baris yang memanggil `setSecret` dengan mode `create`. Kalau jawabannya `secret.conflict` karena nilai sudah ada, baris menawarkan Replace; kalau jawabannya `secret.confirmation_required`, sheet menampilkan dialog konfirmasi yang menjelaskan bahwa password di dalam database tidak berubah, lalu mengirim ulang dengan `confirmStoredOnly`. Setelah semua baris terisi, tombol utama mengulang aksi asal (start, env up, restore). Peringatan `shortValueNotScrubbed` tampil di baris yang bersangkutan.
- `RevealableSecretText` dan `ConcealedPasteboard` seperti di *Reveal dan copy*. Tab Summary (fitur 21) memakainya untuk connection string.
- Nilai di Swift hanya di `@State` lokal view, `RevealedSecret`, atau variabel sementara, dikosongkan setelah terkirim, dibatalkan, atau disembunyikan, tidak pernah masuk `@AppStorage`, `UserDefaults`, model `@Observable` yang hidup lama, atau log `os.Logger` tanpa privacy `.private`.
- Aksesibilitas: label VoiceOver `Password, hidden` untuk nilai tersamar beserta nama reference, tombol Show, Hide, dan Copy bisa dicapai dengan Tab dan diaktifkan dengan Space, Dynamic Type, kontras WCAG AA, dan token dari spec 0012.

### Sisi Swift fixture credential

`FixtureCredential` di `StacklyTestSupport` (target dari spec 0002): `init()` membuat `stackly-fixture-credential-<32 hex huruf kecil>` dari UUID acak, `value`, `description` dan `debugDescription` `FixtureCredential(****)`, dan `assertAbsent(in: String, what: String)` yang gagal dengan pesan `fixture credential found in <what>` tanpa mencetak nilainya. Prefixnya satu konstanta yang dicek sama dengan nilai Rust lewat satu fixture di `protocols/fixtures/`.

### Asumsi lintas spec

- **Assumed (spec 0005)**: tabel `secret_references` dan `secret_reference_uses` seperti di *Model data*, dengan revision per resource untuk compare and swap dan foreign key `RESTRICT`; pemakaian ditulis dalam transaksi yang sama dengan config revision atau environment; port registry bisa dipanggil dari core dan bisa menulis baris `audit_events` (actor, action, resource, operation ID, result, `safe_details`) dalam transaksi yang sama dengan perubahan lain; dan `config_revisions.values` menyimpan teks reference untuk field secret.
- **Assumed (spec 0006)**: envelope error dengan kode stabil, kategori, detail, retryability, dan recovery hint, termasuk kode `needs_secret`, `secret.invalid_reference`, `secret.invalid_value`, `secret.not_found`, `secret.conflict`, `secret.confirmation_required`, `secret.delivery_rejected`, `secret_store.locked`, `secret_store.access_denied`, `secret_store.busy`, `secret_store.outcome_unknown`, `secret_store.unsupported`, dan `secret_store.backend`, dengan nama akhir mengikuti skema penamaan spec 0006 dan kategori yang menurunkan exit code; context per permintaan yang memuat identitas peer, jenis klien (`gui` atau `cli`), dan `interactive`; pemetaan typeshare `serialized_as = "String"` untuk `SecretText`; DTO instance yang bisa membawa field `blocker` dan event perubahan instance; event dengan cursor untuk `secret.changed`; sink log daemon yang menerima writer pembungkus; frame kontrol 1 MiB (nilai 64 KiB muat); IPC tidak pernah mencatat isi frame; dan klien memverifikasi pemilik endpoint sebelum mengirim nilai secret (AC-070).
- **Assumed (spec 0007)**: operasi yang memanggil `resolve_for_start` berhenti dengan `needs_secret` sebelum efek samping apa pun, mencoba lagi berarti operasi baru, operasi menyimpan context pemicunya (aksi pengguna dari klien interaktif atau pemicu latar) supaya interaksi Keychain tetap benar setelah klien putus, dan kompensasi operasi menghapus reference yang dibuat operasi itu.
- **Assumed (spec 0008)**: kerangka command, mode noninteractive, `--json`, dan exit code dari kategori error; spec 0008 memakai kontrak *Input aman CLI* di atas apa adanya.
- **Assumed (spec 0009)**: manifest menandai secret field beserta namanya (pola `[a-z][A-Za-z0-9]*`, unik per manifest) dan klasifikasinya (VAL-012: `immutable_after_init`, `reloadable`, `restart_required`), menyatakan mekanisme pengiriman per field dari daftar di *Pengiriman secret ke provider*, menolak secret di argv dan mekanisme yang tidak dikenal saat validasi, dan template engine hanya menyelesaikan placeholder secret lewat `render_secret_placeholder` dengan `ResolvedSecrets`.
- **Assumed (spec 0010)**: startup transaction memanggil `resolve_for_start` sebelum merender config dan `ensure_not_in_argv` sebelum spawn, memegang guard `SecretFile` selama langkah yang memakainya, memanggil `remove_secret_material` setelah stop bersih, dan reconcile memanggil `sweep_secret_material` lalu `learn_running`.
- **Assumed (spec 0012)**: token untuk teks tersamar, state field input, dan focus ring.

### Permukaan API

Nama method mengikuti skema spec 0006 (Assumed). Semua permintaan IPC butuh peer yang sudah diverifikasi sebagai user yang sama (spec 0006).

| Permukaan | Bentuk | Input utama | Output utama | Error utama |
| --- | --- | --- | --- | --- |
| `secret.list` | request | filter opsional `ownerKind`, `status` | `items: [SecretRefSummary]` urut reference, `store: SecretStoreStatus` | tidak ada selain envelope umum |
| `secret.status` | request | `reference: string` | `SecretRefSummary` dari probe baru, `store` | `secret.invalid_reference`, `secret.not_found` |
| `secret.set` | request | `reference: string`, `input: SecretInput` (tanpa `bind`), `mode: create, replace, upsert`, `expectedRevision?: u64`, `confirmStoredOnly?: bool` | `summary: SecretRefSummary`, `warnings: [shortValueNotScrubbed]`, `effects: [{resourceKind, resourceId, field, effect}]` | `secret.invalid_reference`, `secret.invalid_value`, `secret.conflict`, `secret.not_found`, `secret.confirmation_required`, `secret_store.*` |
| `secret.reveal` | request | `reference`, `purpose: show, copy` | `reference`, `value: SecretText` | `secret.not_found`, `secret_store.*`, error registry kalau audit gagal |
| `secret.delete` | request | `reference`, `expectedRevision?` | `reference` | `secret.conflict` dengan `usedBy`, `secret.not_found`, `secret_store.*` |
| `secret.changed` | event | | `reference`, `status` (`present`, `missing`, `unknown`, atau `deleted`), `revision` | |
| `SecretInput` | DTO bersama | `{type: value, content: SecretText}`, `{type: generate}`, `{type: copyFrom, content: reference}`, atau `{type: bind, content: reference}` | | dipakai juga di permintaan create instance, env up, dan restore milik fitur 20, 46, dan 51 |
| `daemon.status` | field baru | | `secretStore: {backend, state, target}` | |
| `blocker` di DTO instance | field baru | | `{type: needsSecret, content: {references: [{reference, reason, generateAllowed}]}}` atau null | |
| `needs_secret` | kode error di start, env up, restore | | `details.references: [{reference, reason, generateAllowed}]` | |
| `stackly secret list` dan `status <ref>` | CLI | `--json` | tabel atau JSON tanpa nilai | exit code dari kategori |
| `stackly secret set <ref>` | CLI | sumber dan mode di *Input aman CLI* | ringkasan, peringatan, efek | exit code dari kategori |
| `stackly secret reveal <ref>` | CLI | `--json` | nilai di stdout | exit code dari kategori |
| `stackly secret delete <ref>` | CLI | `--expected-revision` | reference terhapus | exit code dari kategori |
| `SecretService::put` | fungsi core | `PutTarget`, `SecretInput`, mode, revision, interaksi | ringkasan, peringatan, efek | error di baris `secret.set` |
| `SecretService::resolve_for_start` | fungsi core | resource dan interaksi | `ResolvedSecrets` | `NeedsSecret`, error backend |

`SecretRefSummary`: `reference`, `ownerKind`, `ownerId?`, `purpose`, `status`, `statusObservedAt?`, `valueOrigin?`, `valueUpdatedAt?`, `usedBy: [{resourceKind, resourceId, field}]`, `revision`. `SecretStoreStatus`: `backend`, `state`, `target`.

### Value sourcing

| Aksi | Nilai yang dihasilkan atau ditampilkan | Sumber |
| --- | --- | --- |
| `put` | nilai baru | `SecretInput::Value` dari input aman klien, `generate_password()` (CSPRNG OS lewat `getrandom`), atau `read` reference sumber dari store |
| `put` | nilai lama untuk penulisan balik | `read` sebelum replace, di memori sampai transaksi selesai |
| `put` | interaksi | `interactive` di context permintaan (spec 0006), atau context pemicu yang disimpan operasi (spec 0007) |
| `put` | keychain target | root aktif saat daemon start: `Login` untuk root default, `File` di `R/data/keychain/` untuk `STACKLY_ROOT` |
| `put` | service dan account | konstanta `stackly-secret` dan path reference |
| `put` | `platform_store_key` | `SecretStore::store_key` |
| `put` | `owner_kind`, `owner_id`, `purpose` | baris registry; `PutTarget::New` dari pemanggil (instance atau environment dan purpose dari nama field); `secret.set` untuk reference baru mendapat `user`, null, `user` |
| `put` | `value_origin` | jenis `SecretInput` |
| `put` | `value_updated_at`, `status_observed_at` | port `Clock` |
| `put` | `revision` | kolom registry ditambah 1, atau 1 untuk baris baru |
| `put` | `effects` dan kebutuhan konfirmasi | `secret_reference_uses.field`, klasifikasi field di manifest provider (spec 0009), dan `data.initializedAt` instance |
| `put` | peringatan nilai pendek | panjang nilai dibanding `SCRUB_MIN_LEN` |
| create instance | reference baru | `local:instance/<instance-id>/<purpose>`, purpose dari algoritma di *Format secret reference* |
| `secret.list`, `secret.status` | status, waktu, revision | kolom `secret_references`; `secret.status` ditambah hasil `probe` baru |
| `secret.list`, `secret.status` | `usedBy` | `secret_reference_uses` |
| `secret.list`, `secret.status`, `daemon.status` | keadaan store | cache `SecretStore::state()` |
| `secret.reveal` | nilai | `SecretStore::read` |
| `secret.reveal`, audit | actor | identitas peer dan jenis klien dari context permintaan (spec 0006) |
| audit | `result` | `succeeded`, atau `failed:` ditambah kode error dari envelope |
| `secret.delete` | izin hapus | foreign key `RESTRICT` pada `secret_reference_uses` |
| `resolve_for_start` | daftar reference yang dibutuhkan | `secret_reference_uses` untuk resource itu, dari config revision aktif |
| `resolve_for_start` | alasan needs secret | error dari `read` |
| `resolve_for_start` | `generateAllowed` | klasifikasi field (spec 0009) dan `data.initializedAt` instance untuk setiap pemakaian |
| blocker instance | isi blocker | `NeedsSecret` terakhir, di memori daemon |
| `daemon.status` | `secretStore.target` | `KeychainTarget` yang dipilih saat start |
| file secret | nama file | purpose field atau nama file include dari manifest (spec 0009) |
| GUI reveal | batas tampil 30 detik, batas clipboard 90 detik | konstanta di `StacklyUI` |
| worker | 180 detik dan 10 detik | default `WorkerConfig` di core |
| redaction | penanda `****`, 8 byte, 16 byte | konstanta `MASK`, `SCRUB_MIN_LEN`, `SCRUB_ANYWHERE_LEN` |

### Key invariants

- Nilai secret hanya ada di Keychain, di memori proses selama dipakai, di permintaan yang membawa `SecretInput`, di respons `secret.reveal`, dan di file `secrets/` milik instance yang sedang berjalan atau sedang menjalankan satu langkah. Tidak pernah di registry, manifest instance, `stackly.yaml`, lockfile, log, audit, event, atau argv.
- Hanya `stacklyd` yang membaca dan menulis Keychain untuk secret Stackly. GUI dan CLI tidak pernah menautkan Security framework untuk itu.
- Test tidak pernah menyentuh login Keychain; root override dan `TestRoot` selalu memakai keychain file sendiri.
- Interaksi Security framework mati kecuali selama satu panggilan `AllowPrompt`, dan pemicu latar tidak pernah memunculkan dialog.
- Semua panggilan fallible ke store lewat satu `SecretWorker`, dan adapter memegang mutex sendiri.
- Nilai yang hilang atau tidak terbaca selalu menghasilkan `NeedsSecret` atau error, tidak pernah nilai pengganti.
- Daemon tidak pernah memilih reference yang sudah ada untuk resource baru; berbagi reference hanya lewat `bind` yang eksplisit.
- Reference yang masih dipakai tidak bisa dihapus.
- `put`, `delete`, dan `probe_one` untuk reference yang sama tidak pernah berjalan bersilang (lock per reference di `SecretService`).
- Setiap nilai dikenalkan ke `Redactor` sebelum ditulis ke mana pun, dan setiap baris log daemon, envelope error, progress, dan event lewat `Redactor`.
- Keychain ditulis sebelum registry saat mengisi, dan registry dihapus sebelum Keychain saat menghapus. Transaksi registry yang gagal setelah tulisan Keychain dibalik dengan penulisan balik. Ketidakcocokan karena crash diperbaiki probe dan inventaris fitur 57, tidak pernah dengan membaca kunci lama diam diam.

### Security model

- Batasnya satu akun user OS (BR-012). ACL item Keychain mempercayai signature `stacklyd`, jadi proses lain milik user yang sama memicu dialog izin macOS sebelum bisa membaca nilai langsung dari Keychain. ACL itu hanya melindungi akses langsung ke Keychain, bukan akses lewat daemon.
- Klien mana pun yang lolos verifikasi peer (user yang sama) boleh memanggil `secret.reveal`. Ini disengaja (script butuh nilai untuk otomasi), dan setiap reveal tercatat di audit dengan gagal tertutup. Akibatnya malware dengan hak user yang sama bisa meminta nilai lewat socket; ini residual risk yang sama dengan BR-012 dan dicatat.
- Klien memverifikasi pemilik endpoint sebelum mengirim nilai (spec 0006, AC-070).
- Signing ad hoc di development: setiap build ulang `stacklyd` mengubah code signature, jadi aksi pengguna pertama yang membaca item lama di login Keychain memunculkan dialog izin (klik Always Allow), dan pemicu latar menjadi `access_denied` sampai itu terjadi. Root override dan test memakai keychain file yang dibuat proses itu sendiri. Fitur 15 membuktikan item tetap terbaca setelah identity development nyata dipasang.
- Keychain file root override dibuka dengan password di file 0600 di sebelahnya. Itu hanya pantas untuk root development dan test, sesuai batas `STACKLY_ROOT` di spec 0004.
- Residual risk yang dinyatakan (OQ-008): file config plaintext dan environment child selama instance berjalan; hash di config bisa ditebak offline untuk nilai lemah; penghapusan file di APFS tidak fisik; string Swift dan buffer frame JSON tidak bisa dihapus dari memori; bentuk nilai yang di encode tidak tertangkap redaction; nilai di bawah 8 byte tidak dicocokkan di log; API keychain file yang deprecated di jalur root override dan test bisa hilang di versi macOS berikutnya.

### Configuration required

Tidak ada variabel environment atau credential baru. `STACKLY_ROOT` (spec 0004) membuat daemon memakai keychain file `R/data/keychain/stackly.keychain-db`, jadi root development dan test tidak pernah melihat atau menulis login Keychain.

### Menjalankan test di Mac ini

- Integration test Keychain berjenis `fixture` (spec 0002) dan dijalankan lewat wrapper: `cargo xtask evidence run --kind fixture -- cargo test -p stackly-platform-macos --test secret_store --locked`. Test ini butuh sesi login GUI yang aktif, karena Security framework butuh sesi user; tanpa sesi itu test tercatat `BLOCKED`.
- Kode `OSStatus` untuk Deny dan Cancel, serta perilaku dialog setelah build ulang ad hoc (dialog muncul untuk aksi pengguna, needs secret tanpa dialog untuk pemicu latar), dibuktikan sekali dengan tangan di Mac ini memakai root default dan dicatat lewat `cargo xtask evidence new` sebagai record manual di `evidence/manual/`.

### Critical test scenarios

- Reference: tabel kasus valid dan tidak valid (huruf besar, segmen kosong, `..`, 1 segmen, 9 segmen, 256 byte, skema `keychain:`, `local:instance/...` dan `local:stackly/...` dari `secret.set`), uji bolak balik, dan tabel turunan purpose. Memverifikasi **AC-1**, **AC-6**.
- Keychain nyata: di keychain file `TestRoot`, create, probe, read, replace, upsert, dan delete; item di keychain file tidak terlihat dari login Keychain; search list user sama sebelum dan sesudah; `store_key` sesuai target. Memverifikasi **AC-2**.
- Pemetaan status: test tabel `map_status` untuk setiap baris tabel; keychain file terkunci lalu read dan probe dengan `Never` menghasilkan `Locked`; item yang dibuat `/usr/bin/security add-generic-password -T /usr/bin/true` lalu dibaca dengan `Never` menghasilkan `AccessDenied { by_user: false }`; keduanya selesai tanpa dialog dalam kurang dari satu detik, dan kode nyatanya direkam. Memverifikasi **AC-3**, **AC-8**.
- Worker: dengan `WorkerConfig` pendek, `FakeSecretStore::hold_next` menahan satu panggilan; job `Never` di belakangnya gagal `Busy` dan tidak pernah dijalankan (penghitung fake tidak naik), `write` yang ditahan melewati batas gagal `OutcomeUnknown` lalu diikuti probe, dan `state()` tetap menjawab seketika. Memverifikasi **AC-4**, **AC-8**.
- Use case `put`: dengan `FakeSecretStore` dan registry test, keempat input kali ketiga mode; `expectedRevision` salah tanpa satu pun panggilan store; `create` saat registry `missing` tetapi item ada menghasilkan `secret.conflict`; transaksi registry gagal setelah replace menulis balik nilai lama; replace dengan efek `stored_only` tanpa konfirmasi tidak menulis apa pun; `bind` menambah pemakaian tanpa menulis nilai; `copyFrom` tidak pernah mengembalikan nilai ke pemanggil; revision baris baru 1 dan probe tidak menaikkannya. Memverifikasi **AC-5**, **AC-8**.
- Generate: dua ribu nilai unik, panjang 32, alfabet benar; create instance dengan dua secret field menghasilkan dua reference baru `local:instance/<id>/...` dan tidak pernah memakai reference lain tanpa `bind`. Memverifikasi **AC-6**.
- Batas nilai: kosong, 65537 byte, berisi NUL, dan UTF-8 tidak valid (lewat CLI `--from-file`) ditolak sebelum store disentuh; nilai 7 byte diterima dengan peringatan. Memverifikasi **AC-7**, **AC-17**.
- Reveal dan audit: reveal berhasil, reveal saat terkunci, dan reveal yang dibatalkan masing masing menulis satu baris audit dengan `result` yang benar tanpa nilai; registry yang dibuat gagal menulis audit membuat reveal gagal tanpa nilai. Memverifikasi **AC-9**.
- Delete: reference yang dipakai ditolak dengan `usedBy`; Keychain terkunci memulihkan baris dengan revision naik 1; item yang sudah hilang dari Keychain tetap membuat baris terhapus; pemakaian yang ditambahkan bersamaan membuat delete gagal tanpa menyentuh Keychain. Memverifikasi **AC-10**.
- Needs secret: `FakeSecretStore` terkunci dan satu reference hilang menghasilkan `NeedsSecret` dengan dua entri urut teks dan alasan yang benar; setelah satu `AccessDenied { by_user: true }` tidak ada `read` berikutnya; satu error backend mengalahkan semuanya; cache `missing` tidak membuat `read` dilewati; blocker instance terisi tanpa mengubah lifecycle atau revision, lalu hilang setelah `secret.changed`. Test ini menulis evidence untuk AC-062. Memverifikasi **AC-11**.
- Pengiriman: di `FakePlatform`, `SecretFile` 0600 di folder 0700 terhapus saat guard drop setelah langkah berhasil dan setelah panic; sweep menghapus `secrets/` instance yang tidak berjalan dan membiarkan yang berjalan; `ensure_not_in_argv` menolak argumen sama persis dengan nilai 4 byte, `--password=<nilai>`, dan argumen yang memuat nilai 32 byte, tetapi menerima path yang memuat nilai 8 byte `postgres`; `hash(Sha256Hex)` cocok dengan vektor test. Memverifikasi **AC-12**.
- Redactor: tabel grammar (nilai dikenal 32 byte di tengah kata, nilai 8 byte sebagai token dan di dalam kata, nilai lama setelah replace, URI dengan `@` di password, URI tanpa password, `POSTGRES_PASSWORD=x`, `"password": "x"`, `token: 'x'`, `Authorization: Bearer x`, dan `password authentication failed` yang tidak berubah); lalu test daemon sengaja mencatat nilai mentah di setiap level dan file log tidak memuat prefix fixture credential. Memverifikasi **AC-13**.
- DTO dan frame: `format!("{:?}")` untuk setiap DTO secret tidak memuat nilai; rekaman frame selama skenario sentinel hanya memuat nilai di permintaan dengan `SecretInput` dan respons `secret.reveal`; error yang sengaja memuat nilai sampai ke klien dalam bentuk tersamar. Memverifikasi **AC-14**.
- Penjaga: crate fixture yang memanggil `expose()` di modul lain, termasuk di dalam `tracing::info!`, dan yang menambah `security-framework` di `stackly-core`, membuat `check-deps` gagal dengan file dan baris; `stackly-testkit` `fake_secret` lolos. Memverifikasi **AC-15**.
- Sentinel ujung ke ujung lewat daemon dengan `STACKLY_ROOT` ke `TestRoot` (milestone 1) dan lewat CLI (milestone 2), dengan evidence `fixture`. Memverifikasi **AC-16**.
- CLI: tanpa TTY dan tanpa flag gagal seketika; dua flag sumber ditolak; `--stdin` dengan `\r\n` di akhir; `--from-env` variabel kosong ditolak; `--password` ditolak clap; tiga konfirmasi TTY yang berbeda gagal; output default dan `--json` tidak memuat nilai. Memverifikasi **AC-17**.
- Swift: `FixtureCredential` dan prefix yang sama dengan Rust; `RevealedSecret` tidak mencetak nilai lewat `print`, `dump`, atau interpolasi; fixture kontrak secret didecode dengan nilai `not-a-real-secret`. Memverifikasi **AC-18**.
- GUI: dengan jam yang bisa diatur, nilai tersembunyi lagi setelah 30 detik dan saat app tidak aktif; pasteboard dikosongkan setelah 90 detik dan saat app ditutup kalau `changeCount` sama, dan dibiarkan kalau berubah; replace `stored_only` menampilkan konfirmasi; label VoiceOver dan navigasi Tab dicek di preview dan test. Memverifikasi **AC-19**.
- Izin: klien dari user lain ditolak sebelum permintaan secret diproses (dibuktikan oleh test spec 0006, dipakai ulang di sini). Memverifikasi **AC-9**, **AC-14**.

## Build plan

Urutan mengikuti pendekatan Journey: satu jalur dibuat lengkap dulu, yaitu "isi secret lewat daemon, simpan di Keychain, pakai untuk start, dan buktikan tidak bocor ke mana pun", baru kemudian permukaan CLI dan GUI di atasnya. Milestone 1 bisa mulai setelah fitur 4, 5, dan 6 dibangun. Milestone 2 butuh fitur 8 dan 13 (dan token fitur 12).

**Milestone 1: Keychain, reference, dan redaction lewat daemon**

1. Domain dan platform dasar: modul `secret` di domain (`SecretRef` dengan parse dan turunan purpose, `SecretOwner`, `SecretPurpose`, `SecretStatus`, `SecretOrigin`, `NeedsSecretReason`, observasi `InstanceBlocker` di `Instance`); tambahan spec 0004 yang dibutuhkan fitur ini (`Filesystem::remove_file`, `Filesystem::remove_empty_dir`, `DirectoryLayout::instance_secrets_dir`, dengan pemeriksaan di `fs_conformance`). Memenuhi **AC-1**, **AC-6**, **AC-11**, **AC-12**.
2. Platform dan protocol: `SecretValue`, trait `SecretStore` beserta tipenya, `UnsupportedSecretStore`, port keenam di `Platform`, capability, `SecretText`, dan `SecretInput`; `FakeSecretStore` di testkit (peta di memori, `lock`, `deny`, `unsupported`, `hold_next`, `fail_next`, penghitung per method). Memenuhi **AC-3**, **AC-4**, **AC-7**, **AC-14**.
3. `stackly-platform-macos`: spike kode `OSStatus` dan `kSecUseAuthenticationUI` dulu, lalu `MacosSecretStore` lewat `security-framework` dengan `KeychainTarget::Login` dan `File`, patokan keychain eksplisit, saklar interaksi default mati, mutex, `map_status` dengan test tabel, dan integration test `crates/platform-macos/tests/secret_store.rs`. Memenuhi **AC-2**, **AC-3**.
4. Core: `SecretWorker` dengan `WorkerConfig`, `generate_password`, `Redactor` dengan grammar normatif, `secret::delivery`, dan `SecretService` (`put`, list, status, reveal, delete, `probe_all`, `probe_one`, `resolve_for_start`, `learn_running`) di atas port registry spec 0005, dengan test memakai `FakeSecretStore` dan `FakePlatform`. Memenuhi **AC-4**, **AC-5**, **AC-6**, **AC-8**, **AC-9**, **AC-10**, **AC-11**, **AC-12**, **AC-13**.
5. `stacklyd` dan protocol: DTO secret dengan fixture di `protocols/fixtures/` dan `cargo xtask codegen`, handler `ipc::secret`, pilihan `KeychainTarget` dari root aktif, writer log yang lewat `Redactor`, `Redactor` di envelope, progress, dan event, `probe_all` saat start, `secretStore` di `daemon.status`, field `blocker` di DTO instance, dan baris audit. Memenuhi **AC-5**, **AC-8**, **AC-9**, **AC-11**, **AC-13**, **AC-14**.
6. `cargo xtask check-deps`: pindaian token stream untuk `expose`, allowlist, dan aturan letak `security-framework` serta `rpassword`, dengan crate fixture yang melanggar. Memenuhi **AC-15**.
7. Suite sentinel tingkat daemon dengan evidence `fixture`, plus record manual untuk kode Deny dan Cancel serta perilaku dialog setelah build ulang ad hoc. Memenuhi **AC-3**, **AC-11**, **AC-13**, **AC-14**, **AC-16**.

Sesi paralel: setelah langkah 1 dan 2 masuk, langkah 3 (adapter Keychain) dan langkah 4 (core) bisa dikerjakan dua sesi terpisah karena hanya bertemu di trait `SecretStore`. Langkah 6 tidak bergantung pada yang lain dan bisa menjadi sesi ketiga. Langkah 5 dan 7 menunggu ketiganya.

**Milestone 2: Alur secret di CLI dan GUI**

8. CLI: modul `secret_input` (TTY lewat `rpassword`, stdin, file, env, generate, copy from) dan command `stackly secret list`, `status`, `set`, `reveal`, dan `delete` dengan `--json`; test CLI dan bagian CLI dari suite sentinel. Memenuhi **AC-16**, **AC-17**.
9. Swift client dan test support: `FixtureCredential` di `StacklyTestSupport`, method secret dan `RevealedSecret` di `StacklyClient`, dan test kontrak Swift. Memenuhi **AC-18**.
10. `StacklyUI`: `SecretField`, `SecretEntrySheet`, `RevealableSecretText`, dan `ConcealedPasteboard` dengan preview, test timer dan pasteboard, serta cek aksesibilitas. Memenuhi **AC-19**.
11. Bukti GUI: alur isi lewat `SecretEntrySheet` dan reveal lewat `RevealableSecretText` dengan fixture credential, dicatat sebagai record manual sampai helper XCUITest (spec 0002 Follow-up) tersedia. Memenuhi **AC-16**, **AC-19**.

Sesi paralel: langkah 8 (CLI, Rust) dan langkah 9 sampai 10 (Swift) bisa dikerjakan dua sesi terpisah karena tidak menyentuh file yang sama. Langkah 11 menunggu langkah 10.

## Consequences

**Positive**:

- Satu tempat untuk nilai (Keychain), satu pembaca (daemon), satu use case pengisi (`put`), dan satu penyaring (`Redactor`), jadi fitur 20, 21, 27, 34, 35, 45, 46, dan 51 cukup memanggil yang sudah ada.
- Pemicu latar tidak pernah tertahan dialog, dan Keychain yang terkunci terlihat sebagai alasan yang bisa ditindaklanjuti, bukan instance yang diam diam tanpa auth.
- `stackly.yaml` dan registry tetap bisa dibagikan atau dibackup tanpa membawa password.
- Test dan root development tidak pernah menyentuh login Keychain pengguna, dan kebocoran baru tertangkap oleh tiga lapis plus pindaian evidence spec 0002 dan aturan `check-deps`.

**Negative / tradeoffs**:

- Selama signing masih ad hoc, kamu akan melihat dialog izin Keychain setelah setiap build ulang `stacklyd` yang memakai root default, dan instance yang dinyalakan pemicu latar tertahan `access_denied` sampai kamu mengizinkan lewat aksi pengguna.
- Satu thread Keychain berarti satu dialog yang dibiarkan terbuka menahan semua akses secret lain sampai batas tunggu, dan tulisan yang melewati batas berakhir `outcome_unknown` yang harus dicek ulang.
- Replace membaca nilai lama lebih dulu, jadi replace setelah build ulang ad hoc bisa memunculkan dua dialog (baca lalu tulis).
- Config plaintext tetap ada untuk Valkey dan NATS selama instance berjalan, dan file itu bisa hilang kalau macOS membersihkan folder temp saat instance berjalan lama; reload harus merender ulang lebih dulu.
- Pencocokan pola bisa salah samar (misalnya nilai yang kebetulan mengikuti `token:`), dan nilai pendek hanya dicocokkan sebagai token utuh atau tidak sama sekali.
- Replace tidak merotasi password di dalam database, jadi pengguna harus melewati konfirmasi `stored_only`.
- Mockup memakai teks `keychain:<path>`; tampilan akhir memakai `local:<path>` dengan label store "Login Keychain".
- Keychain file untuk root override dan test bergantung pada API yang sudah deprecated, dan mungkin butuh satu blok `unsafe` untuk status kunci.

**Neutral**:

- Dependency baru: `zeroize` (platform, protocol), `getrandom`, `regex`, dan `sha2` (core), `security-framework` (`stackly-platform-macos`), dan `rpassword` (`stackly-cli`).
- `Platform::new` mendapat argumen keenam, dan `PlatformCapability::SecretStore` menjadi supported di macOS.
- `Filesystem` mendapat operasi hapus file dan folder kosong, dan `DirectoryLayout` mendapat folder `secrets/` per instance di runtime root.
- Domain mendapat observasi blocker di samping readiness dan health.

## Follow-up

Perubahan untuk spec 0004 (dicatat di sini, tidak ditulis langsung karena sesi paralel; diterapkan lewat `/architect` update spec 0004):

- [ ] *Port platform*: tambah trait `SecretStore` beserta tipenya, port keenam `secrets` di `Platform` (konstruktor berubah), `UnsupportedSecretStore` di `UnsupportedPlatform`, `FakeSecretStore` di `FakePlatform`, dan capability `secret_store` supported di `MacosPlatform` setelah milestone 1.
- [ ] *Port platform*: tambah `Filesystem::remove_file(path)` (menghapus file biasa atau symlink itu sendiri tanpa mengikutinya, tidak pernah folder) dan `Filesystem::remove_empty_dir(path)`, keduanya hanya di bawah owned root, plus pemeriksaan di `fs_conformance`. Key invariant "tidak ada operasi yang menghapus file" diperbarui.
- [ ] *Model domain*: tambah modul `secret` (`SecretRef`, `SecretOwner`, `SecretPurpose`, `SecretStatus`, `SecretOrigin`, `NeedsSecretReason`) dan observasi `InstanceBlocker::NeedsSecret` di `Instance` yang tidak mengubah lifecycle, desired state, atau revision.
- [ ] *Layout direktori*: tambah `instance_secrets_dir(ShortKey)` (`<runtime>/i/<short-key>/secrets/`) dan folder `keychain/` di root data untuk root override.
- [ ] *Invariant BR*: baris BR-018 menunjuk spec 0011 (AC-1, AC-12, AC-17).
- [ ] *Peta modul*: `stackly-platform` mendapat `zeroize`, `stackly-platform-macos` mendapat `security-framework`.

Perubahan untuk spec 0001 (lewat `/architect` update spec 0001 pada pass rekonsiliasi):

- [ ] *Batas keputusan*: baris "Crate untuk Keychain" sudah diputuskan, `security-framework` (spec 0011).
- [ ] *Peta crate*: `stackly-protocol` boleh bergantung pada `zeroize`; `stackly-core` mendapat `getrandom`, `regex`, dan `sha2`; `stackly-cli` mendapat `rpassword`; `stackly-platform-macos` mendapat `security-framework`.
- [ ] *Aturan arah dependency*: `security-framework` hanya di `stackly-platform-macos` dan `rpassword` hanya di `stackly-cli` (ditegakkan `check-deps`, AC-15).

Untuk fitur dan spec lain:

- [ ] Spec 0002: tandai Follow-up "Fitur 11: sisi Swift dari `FixtureCredential`" sudah dicakup spec ini (AC-18) setelah milestone 2 dibangun.
- [ ] Fitur 10: startup transaction memanggil `resolve_for_start` sebelum render config dan `ensure_not_in_argv` sebelum spawn, memegang guard `SecretFile`; stop bersih memanggil `remove_secret_material`; reconcile memanggil `sweep_secret_material` lalu `learn_running`.
- [ ] Fitur 14: walking skeleton memberi fake provider satu secret field, lalu membuktikan `FakeSecretStore` terkunci menghasilkan blocker needs secret di CLI dan GUI, dan fixture credential tidak muncul di mana pun ujung ke ujung.
- [ ] Fitur 15: buktikan item yang dibuat saat signing ad hoc tetap terbaca setelah identity development nyata (sekali Always Allow per item, atau tulis ulang lewat aksi pengguna), lalu putuskan apakah pindah ke data protection keychain.
- [ ] Fitur 20, 46, dan 51: permintaan create instance, env up, dan restore membawa `SecretInput` per secret field dan memanggil `SecretService::put` dengan `PutTarget::New`; secret dibuat sebelum `initdb`, kegagalan menyimpan menghentikan operasi dengan `needs_secret`, dan kompensasi operasi menghapus reference yang dibuat operasi itu.
- [ ] Fitur 20: `SecretField` di sheet New Instance dengan `generate` sebagai default; preflight "Secret stored in Keychain" membaca hasil `put`.
- [ ] Fitur 21: connection URI tersamar secara default dan `RevealableSecretText` untuk Show Password dan Copy.
- [ ] Fitur 24: duplicate memakai `generate` atau `copyFrom` ke reference baru, tidak pernah `bind` diam diam.
- [ ] Fitur 26 dan 29: reload merender ulang file `secrets/` lebih dulu; tampilan config, diff, dan `config-history` tidak pernah membaca folder `secrets/`.
- [ ] Fitur 27: setiap baris log instance lewat `Redactor` sebelum disimpan, dengan test dummy server yang mencetak fixture credential.
- [ ] Fitur 31: autostart memakai interaksi `Never`, Keychain yang terkunci menghasilkan blocker needs secret, dan kebijakan kapan mencoba lagi setelah blocker hilang.
- [ ] Fitur 34, 35, dan 45: bundle diagnosis, `safe_details` audit, dan export environment lewat `Redactor`, masing masing dengan test sentinel.
- [ ] Fitur 41: primitive hash bcrypt untuk htpasswd Traefik di `secret::delivery`.
- [ ] Fitur 44 dan 45: `stackly.yaml` hanya menerima reference `local:` dan menolak literal secret (VAL-013); lockfile dan export tidak pernah memuat nilai.
- [ ] Fitur 48, 49, dan 51: backup registry membawa reference saja dan menandai yang hilang; cold backup mencatat `secret material` dikecualikan; restore as new memakai `SecretEntrySheet` dengan `copyFrom`.
- [ ] Fitur 54: purge menghapus reference milik instance beserta item Keychain nya lewat use case delete, mengikuti aturan pemilik.
- [ ] Fitur 57 dan 62: inventaris item Keychain yatim (service `stackly-secret` tanpa baris registry, termasuk sisa delete yang terputus) lewat atribut saja; uninstall menjelaskan bahwa secret tetap di Keychain.
- [ ] Fitur 63: review keamanan mencakup `Redactor`, `secret::delivery`, dan allowlist; fuzz target untuk `SecretRef::parse` dan `Redactor`.
- [ ] Fitur 66: dokumentasi pengguna menyatakan residual risk OQ-008 dan perilaku dialog Keychain.
- [ ] Fitur 70: membuka instance di aplikasi klien memakai aturan pengiriman yang sama dan tidak pernah lewat URL atau argv.
- [ ] `/scope`: catat di [ui-macos.md](../../scope/ui-macos.md) selisih mockup `keychain:<path>` lawan `local:<path>` di tab Summary.
- [ ] `/sync` setelah build: catat aturan secret (allowlist `expose()`, `Redactor` di setiap jalur keluar, DTO tanpa nilai secret, test Keychain hanya di keychain file) di root `AGENTS.md`, dan baris `Declined:` untuk Agent Skills `security-framework`, `zeroize`, dan `rpassword` yang tidak dicari atas pilihan kamu.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, keputusan kecil, dan hasil cross check ada di [rationale.md](rationale.md).
