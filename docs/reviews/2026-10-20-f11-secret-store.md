# Review, claude/chk-f10-review-c3f69b (fitur 11 Secret store & redaction, spec 0011), 2026-10-20

**Reviewed by**: Claude Fable 5.1, tujuh subagent paralel, satu per irisan (author on Claude Opus 5.5 dan Claude Sonnet 5.5; Fable satu satunya family yang tidak ikut menulis). Konsolidasi dan verifikasi setiap Major oleh Claude Opus 5.5 (sesi `chk-f11-review`) terhadap kode di worktree.
**Scope**: keadaan kode fitur 11 di `main` `1bcc3af`, bukan satu diff. Riwayat dari `4224a35` (commit F11 pertama) sampai `ad5f5cc`. 47 file source utama (11.395 baris) di path yang brief sebut, ditambah bagian secret dari `crates/protocol/src/codes.rs`, `crates/platform/src/{platform,private_dir}.rs`, `apps/daemon/src/{server,startup,connection}.rs`, `apps/cli/src/commands/instance/create.rs`, dan aturan `expose` di `xtask/src/check_deps*`. `crates/domain/src/secret/` di brief sebenarnya `crates/domain/src/secret.rs` dan `crates/domain/src/registry/secret.rs`. Test hanya dibaca untuk menilai cakupan.
**Verdict**: Changes requested (2 Major, tanpa Blocker). 20 Minor dan 42 Nit.
**Status setelah fix** (`28b19d5..b5941d1`): Approve with nits. Kedua Major tertutup, tanpa Blocker atau Major yang terbuka; enam Minor ikut tertutup; satu Minor baru dan dua Nit baru dari diff fix. Terbuka: 15 Minor (14 dari putaran 1 plus 1 baru) dan 44 Nit. Rincian di *Konfirmasi fix* di bawah.

## Konfirmasi fix (28b19d5..b5941d1)

**Dikonfirmasi oleh**: Claude Opus 5.5 (sesi `chk-f11-review`), inline. Ketujuh commit fix (`fc02f90`, `4aeddb1`, `b881ca1`, `b77f284`, `7d914d5`, `ec7d11d`, `7836231`) ditulis Claude Sonnet 5.5, jadi reviewer beda family. Worktree di `git merge main` (`aec766a`); diff dibaca dengan `git diff 28b19d5..b5941d1` (10 file kode, 8 file test, plus record manual step 11). Tidak ada build atau test di sesi ini; gate lead di `main` (fmt, clippy workspace, `codegen --check`, check-deps, cek Windows, core 16 target, protocol 7, CLI 6, `stacklyd` 19, StacklyUI 314, evidence check, tanpa proses yatim) dipakai sebagai bukti test baru lulus.

| Butir putaran 1 | Hasil | Bukti |
| --- | --- | --- |
| Major 1, `secret.needs_secret` dari operasi tanpa `items` dan hint | tertutup | `crates/core/src/secret/error.rs:421` (`items()` untuk `NeedsSecret`, `Conflict(InUse)`, `ConfirmationRequired`), `crates/core/src/supervisor/error.rs:466` (delegasi `Secret`), `apps/daemon/src/ipc/operation/dto.rs:212` dan `apps/daemon/src/ipc/secret.rs:411` (`hint_for` dan `with_secret_hint`, satu sumber hint untuk jalur operasi dan method `secret.*`); `needs_secret_error` dan `delivery_error` dihapus, dan `secret_error` untuk `NeedsSecret` kini lewat `operation_error_body`. `omitted` dihitung engine (`crates/core/src/operation/failure.rs`) atau `ErrorBody::with_items`, tidak lagi di `details`. Test: `apps/daemon/tests/supervisor/actions.rs` (item `reference`, `reason` `missing`, `generateAllowed`, `count` 1, tanpa `omitted`, hint), `apps/daemon/tests/skeleton.rs` (item `store_locked` dan hint dari operasi start sungguhan), `apps/daemon/tests/instance_create_secrets.rs::a_bind_to_a_source_in_a_locked_keychain_fails_the_create_with_the_source_as_an_item` |
| Major 2, `hide()` membatalkan Show dan Copy diam diam | tertutup | `RevealModel.swift:149` (`conceal(because:)`: hanya Show yang sedang menunggu berakhir `.failed` dengan sebabnya), `:119` (`copy()` memakai `generation: nil`, jadi tidak pernah dibatalkan), `inFlight` dan nomor `flight` supaya Show lama yang selesai terlambat tidak mengubah state permintaan yang lebih baru. Saya telusuri: Copy di detik ke 29 lalu timer menyala sekarang mengosongkan teks yang tampil dan tetap berakhir `.copied`; app tidak aktif di tengah Show berakhir `.failed("Hidden because Stackly went to the background. ...")`; Show yang dibatalkan bisa dimulai lagi sementara permintaan lama masih berjalan. Package memakai `defaultIsolation(MainActor.self)`, jadi state baru tidak berbalapan. Test: `aHideWhileShowWaitsExplainsItAndNeverShowsTheValue`, `theAppTurningInactiveWhileShowWaitsExplainsItAndShowsNothing`, `aShowCancelledByTheAppTurningInactiveCanBeStartedAgain`, `theThirtySecondTimerFiringWhileCopyWaitsStillCopies`, `theAppTurningInactiveWhileCopyWaitsStillCopies`. Urutan dialog SecurityAgent masuk record manual step 11 sebagai langkah opsional (masih `NOT_RUN`) |
| Minor, `reason` hilang di `secret.delivery_rejected` dari operasi | tertutup | `crates/core/src/supervisor/error.rs:448`, test `a_secret_in_argv_names_the_rule_it_broke_and_a_secret_error_delegates_its_items` |
| Minor, `secret.conflict` dari `Store { AlreadyExists }` tanpa detail | tertutup | `crates/core/src/secret/error.rs:395` (`reference` dan `reason: already_exists`), hint `use replace` ikut lewat `hint_for` |
| Minor, kunci sensitif berkutip yang di escape formatter JSON | tertutup | `crates/core/src/secret/redact.rs:24` (`\\?["']?` di kedua sisi kunci) dan `:290` (nilai berakhir di `\"` atau `\'`), test `a_sensitive_key_in_a_json_escaped_message_hides_its_value` lewat `serde_json::to_string` yang sama dengan formatter log; baris JSON tetap sah setelah redaksi |
| Minor, redaksi envelope setelah potong 200 karakter | tertutup | `crates/protocol/src/error.rs:144` (`Uncut`, paling banyak 4096 karakter, `#[serde(skip)]`, `#[schemars(skip)]`, `#[typeshare(skip)]`, `Debug` tanpa isi, tidak memengaruhi kesetaraan) dan `:323` (`redacted_with` meredaksi teks utuh lalu memotong). Saya cek tidak ada setter lain untuk `message`, jadi teks utuh tidak bisa basi. Test `a_value_that_straddles_the_cut_is_redacted_before_the_message_is_cut` dan `the_message_before_the_cut_never_travels_and_does_not_change_equality`; wire dan digest kontrak tidak berubah |
| Minor, Ctrl+C di prompt meninggalkan terminal raw | tertutup | `apps/cli/src/secret_input.rs:139` (`mod interrupt`: listener SIGINT di thread sendiri, `guarded` di sekitar `rpassword::read_password`), `:464` (`cli.interrupted`, exit 130, tanpa prompt kedua), `apps/cli/src/wait.rs:176` (`hand_over_to_wait`). Saya cek di source rpassword 7.5.4 (`src/lib.rs:129` sampai `:138`): sesudah `raise` ia mengembalikan `Interrupted`, jadi dengan handler terpasang `Drop` sempat memulihkan termios. Bila listener gagal dipasang, perilakunya sama dengan sebelumnya. Test `ctrl_c_at_the_prompt_is_cli_interrupted_with_exit_130_and_no_retry`, `a_sigint_raised_during_a_guarded_read_does_not_end_the_process`, dan test pty di `apps/cli/tests/instance/secrets.rs` |
| Minor, VoiceOver tidak membaca nilai yang tampil | tertutup (tidak disebut di laporan fix) | `RevealableSecretText.swift:92` (`.accessibilityValue(shown)`), test `aShownValueIsReadByVoiceOverAsTheLabelsValue` |
| Celah yang diketahui, `providers/fake` merender `authToken` ke folder config | dipindah ke fitur 9, kode belum berubah | spec 0009, 0010, 0011, dan 0014 diperbarui (`370b141`, `cbe9751`, `be44e2d`, `886d9cd`) dan fitur 9 dibuka lagi dengan milestone *Sensitive config in the secrets folder* (`5555e2d`). `crates/core/src/provider/interpolate.rs` dan `providers/fake` tidak berubah di rentang ini, jadi celahnya tetap ada sampai milestone itu dibangun; tetap di luar verdict fitur 11 |

Minor putaran 1 yang masih terbuka (14): irisan 1 ketiganya (saklar interaksi saat panic, `CFData`, status `Unknown` saat konstruksi), irisan 2 keduanya (dua slot blocker, `AuditQuery`), irisan 3 dua (thread worker yang mati, penulisan balik `NotFound`), irisan 4 satu (`on_starting` tanpa `Redactor`), irisan 5 satu (`PackageFieldRules`), irisan 6 satu (escape daftar efek `stored_only`), dan irisan 7 empat (penolakan konfirmasi, batas `secretStatus`, fallback `compose`, teks ketikan `SecretField`). Tidak ada yang menahan verdict.

### Penilaian keputusan Copy

Keputusan developer, Copy yang sedang menunggu tetap menulis pasteboard walau Hide, timer 30 detik, atau app tidak aktif datang di tengah jalan, **bisa diterima**. Alasannya:

- Aturan sembunyi AC-19 (30 detik, view hilang, app tidak aktif) mengatur nilai yang **ditampilkan**. Copy tidak menampilkan apa pun, jadi tidak ada yang perlu disembunyikan, dan aturan Copy di spec adalah aturan pasteboard (`currentHostOnly`, `ConcealedType`, `TransientType`, kosong setelah 90 detik atau saat app ditutup bila `changeCount` belum berubah), yang tetap berlaku lewat `ConcealedPasteboard.shared`.
- Copy adalah aksi eksplisit, dan daemon sudah menulis baris audit reveal `succeeded` dengan purpose `copy`. Membuang nilai yang sudah keluar dari daemon justru membuat audit mencatat salinan yang tidak pernah terjadi, kebalikan dari Major 2 putaran 1.
- Urutan dialog SecurityAgent hanya bisa berhasil dengan keputusan ini: dialog membuat app tidak aktif, dan Copy yang dibatalkan karenanya tidak akan pernah selesai saat dialog masih dibutuhkan.

Satu sisa risiko yang layak ditutup ada di temuan baru pertama di bawah: Copy yang terlambat bisa menimpa isi clipboard yang pengguna salin sesudah menekan Copy.

### Temuan baru dari diff fix

#### 🟡 Copy yang selesai terlambat menimpa clipboard yang sudah diganti pengguna, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Secrets/RevealModel.swift:119`, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Secrets/ConcealedPasteboard.swift:82`

**Problem**: `copy()` sekarang tidak pernah dibatalkan, dan `ConcealedPasteboard.copy` menulis tanpa melihat apakah pasteboard berubah sejak Copy ditekan. `changeCount` hanya dicatat sesudah menulis (`ConcealedPasteboard.swift:88`), untuk aturan pengosongan 90 detik.
**Why it matters**: Dengan dialog Keychain (`AllowPrompt`, batas tunggu worker 180 detik), pengguna bisa pindah app dan menyalin hal lain (misalnya URL) sebelum menekan Allow. Nilai secret yang datang kemudian menimpa salinan itu, dan paste berikutnya menaruh password di tempat yang dimaksudkan untuk URL, misalnya jendela chat. Jendelanya sempit (tanpa dialog, jawaban daemon datang dalam milidetik) dan pasteboard tetap `currentHostOnly` serta kosong setelah 90 detik, jadi Minor. Hal yang sama membuat Copy yang selesai setelah view hilang menulis clipboard tanpa umpan balik apa pun.
**Suggested fix**: Pemilik `StacklyUI/Secrets/`: catat `changeCount` pasteboard saat `copy()` dimulai (tambah getter di `ConcealedPasteboard`), dan bila sudah berubah saat nilai datang, jangan menulis dan isi `phase` dengan `.failed("The clipboard changed while waiting. Nothing was copied. Press Copy again.")`. Tambah satu test dengan pasteboard palsu yang `changeCount` nya naik di tengah permintaan.

#### Nit baru

- ⚪ `crates/core/src/secret/error.rs:290`, `item()` mengembalikan `Option` dan `items()` memakai `filter_map`, jadi item yang gagal dibangun hilang diam diam dan `count` bisa tidak cocok dengan `items`. Tidak terjangkau hari ini (`FailureItem::new` hanya gagal di atas 8 field atau nilai yang terlalu panjang), tetapi `expect` yang menyebut invariant (aturan `AGENTS.md`) membuatnya terlihat.
- ⚪ `apps/cli/src/secret_input.rs:139`, setelah prompt dan sebelum wait (misalnya selama permintaan `secret.set`), Ctrl+C mengakhiri command lewat `std::process::exit(130)` dari thread listener, tanpa envelope `cli.interrupted` dan tanpa output `--json`. Exit code nya sama dengan aksi default dan doc comment menyebutnya; cukup dicatat.

Irisan dan penulisnya:

| Irisan | File | Penulis |
| --- | --- | --- |
| 1, port platform, adapter Keychain, fake store | `crates/platform/src/secret.rs`, `crates/platform-macos/src/secret.rs`, `crates/testkit/src/fake_secret.rs`, kabel `with_secrets` | Opus |
| 2, domain dan registry | `crates/domain/src/secret.rs`, `crates/domain/src/registry/secret.rs`, `crates/storage/migrations/0003_secret_references.sql`, `crates/storage/src/registry/secret.rs`, `crates/testkit/src/registry/conformance/secrets.rs` | Opus |
| 3, core worker dan service | `crates/core/src/secret/{mod,worker,error,compensator,resolved,generate,rules}.rs`, `crates/core/src/secret/service/*.rs` | Opus |
| 4, Redactor, delivery, log daemon, penjaga `expose` | `crates/core/src/secret/{redact,delivery/*}.rs`, `apps/daemon/src/log_sink/*`, `apps/daemon/src/map/redact.rs`, jalur keluar envelope, `xtask/src/check_deps/source_scan.rs` | core Opus, daemon Sonnet, xtask campuran |
| 5, protokol dan daemon IPC | `crates/protocol/src/secret.rs`, `codes::secret`, `apps/daemon/src/ipc/secret.rs`, `apps/daemon/src/secrets.rs`, wiring start | protokol campuran, daemon Sonnet |
| 6, CLI input aman | `apps/cli/src/secret_input.rs`, `apps/cli/src/commands/secret.rs`, jalur `--secret` di create | Sonnet |
| 7, Swift client dan GUI | `clients/swift/Sources/StacklyClient/Secrets/*`, `FixtureCredential.swift`, `StacklyUI/Secrets/*`, `Gallery/SecretsGalleryPage.swift` | Sonnet |

Catatan proses: tidak ada `cargo build`, `cargo test`, atau `swift test` yang dijalankan; review membaca kode, spec, dan test saja. Kedua Major dan celah fake provider dibaca ulang langsung di kode oleh sesi ini (lokasi dan alur di bawah). Beberapa Minor juga dicek ulang (ditandai "dicek ulang"); sisanya dari reviewer irisan, dicatat apa adanya. Satu Minor irisan 4 (`SecretFile` dan IO sinkron di `init.rs:247`) sudah tercatat di review F10, jadi dipindah ke *Di luar lingkup* dan tidak dihitung lagi.

## Summary

Fitur 11 menambah reference `local:<path>`, port `SecretStore` dengan `SecretValue` yang menyamarkan diri dan dihapus saat drop, adapter Keychain macOS dengan saklar interaksi global dan status kunci lewat `SecKeychainGetStatus`, `SecretWorker` satu thread dengan batas tunggu, `SecretService` (`put`, list, status, reveal, delete, `resolve_for_start`, `learn_running`), `Redactor` tiga lapis, aturan pengiriman ke provider, migrasi 0003 dan op registry, kontrak protokol 1.7, writer log daemon yang lewat `Redactor`, CLI input aman, dan komponen Swift. Permukaan yang paling berbahaya kuat: tidak ditemukan jalur yang membocorkan nilai ke argv, log, event, frame, registry, `Debug`, atau pesan error, dan tidak ada jalur yang mengganti nilai yang hilang dengan auth kosong. Setiap `expose()` ada di modul yang diizinkan, setiap panggilan service dari daemon lewat `spawn_blocking`, lock per reference tidak pernah melintasi `.await`, reveal gagal tertutup bila audit gagal ditulis, dan Keychain yang terkunci menghasilkan `Locked` atau needs secret tanpa dialog. Dua masalah utama: kegagalan `secret.needs_secret` dari operasi start dan create sampai ke klien tanpa daftar reference dan tanpa hint (pemeta yang benar hanya dipakai lima handler `secret.*` yang tidak pernah menghasilkannya), dan `RevealModel` di GUI membatalkan Show atau Copy yang sedang menunggu daemon secara diam diam saat timer 30 detik atau app tidak aktif menyala. Celah fake provider yang diketahui berisiko Major, tetapi pemiliknya spec 0009 dan 0014, bukan kode fitur 11.

## Major

### 🟠 `secret.needs_secret` dari operasi start dan create sampai ke klien tanpa `items` dan tanpa hint, `crates/core/src/secret/error.rs:287`, `crates/core/src/supervisor/error.rs:446`, `apps/daemon/src/ipc/operation/dto.rs:180`, `apps/daemon/src/ipc/secret.rs:417`

**Problem**: Ada dua pemeta `SecretError` ke wire. `ipc::secret::needs_secret_error` (`apps/daemon/src/ipc/secret.rs:417` sampai `:439`) membangun item `reference` (`reference`, `reason`, `generateAllowed`) dan hint "fill them with stackly secret set, or unlock the keychain, then try again", tetapi hanya dipanggil lewat `secret_error` oleh lima handler `secret.*` (`ipc/secret.rs:164`), dan tidak satu pun dari kelima itu menghasilkan `NeedsSecret`. Dua jalur yang sungguh menghasilkannya lewat engine operasi: `supervisor::secrets::resolve` mengembalikan `SupervisorError::Secret(SecretError::NeedsSecret)` untuk start (`crates/core/src/supervisor/secrets.rs:39` sampai `:45`), dan langkah `secrets` create membungkus `classify_create_failure` dengan cara yang sama (`apps/daemon/src/ipc/instance/create.rs:667` sampai `:671`). Kegagalan operasi dirender `failure_body` lalu `catalog_body` (`apps/daemon/src/ipc/operation/dto.rs:147` sampai `:211`), yang mengambil `error.items()` dan tidak pernah memasang hint ("The hint is not set here; the area that owns a code adds its own"). `impl OperationError for SecretError` (`crates/core/src/secret/error.rs:287` sampai `:370`) hanya mengisi `code`, `retryable`, dan `details`, dan `SupervisorError` hanya mendelegasikan `details` (`crates/core/src/supervisor/error.rs:446`). Saya cek: `grep "fn items"` di `crates/` dan `apps/` hanya menemukan default trait (`crates/core/src/operation/work.rs:292`, `Vec::new()`) dan dua impl test. Jadi envelope yang sampai ke klien membawa `count` dan `omitted`, tanpa `items` dan tanpa hint, kebalikan dari fixture `protocols/fixtures/ipc/frame-error-secret-needs-secret.json`. `needs_secret_error` dan `delivery_error` tidak punya pemanggil produksi; test pemetaan per varian di `ipc/secret.rs` menguji fungsi yang jalur operasi tidak pakai.
**Why it matters**: Skenario: Keychain terkunci, lalu `stackly instance start pg` dari script noninteractive. Operasi berakhir `failed` dengan `secret.needs_secret`, pesan "2 secret references cannot be read", tetapi reference mana, alasannya (`store_locked` lawan `missing`), dan `generateAllowed` tidak ada, dan hint pemulihan juga hilang. `SecretEntrySheet` membangun barisnya dari `items` kode itu (`SecretNeed`), jadi sheet kosong; CLI tidak bisa menyebut reference yang harus diisi. Untuk start, klien masih bisa membaca blocker instance, tetapi untuk `instance.create` dengan `bind` atau `copyFrom` yang sumbernya terkunci atau hilang (AC-23), instance belum ada sehingga tidak ada blocker sama sekali. Ini melanggar AC-11 (pemetaan ke `secret.needs_secret` dengan `items` dan hint), AC-21 (`ItemSpec` persis seperti tabel), dan AC-23 (item untuk reference sumber). Test yang ada (`apps/daemon/tests/supervisor/actions.rs:261`, `apps/daemon/tests/skeleton.rs:747`, `crates/core/tests/supervisor_start/start.rs:408`) hanya memeriksa kode, jadi celah ini lolos. Akar yang sama juga membuang `reason` dari `secret.delivery_rejected` dan detail `secret.conflict` di jalur operasi (dua Minor di bawah).
**Suggested fix**: Pemilik core (`crates/core/src/secret/error.rs`): tambah `fn items(&self) -> Vec<FailureItem>` di `impl OperationError for SecretError` untuk `NeedsSecret` (kind `reference`, field `reference`, `reason`, `generateAllowed`), `Conflict(InUse)` (kind `use`), dan `ConfirmationRequired` (kind `effect`), lalu delegasikan `items` di `impl OperationError for SupervisorError` untuk varian `Secret` (`crates/core/src/supervisor/error.rs`). Pastikan `omitted` dihitung sekali: `details` sekarang menulis `omitted` sendiri, dan `ErrorBody::with_items` juga menghitung item di atas `max`. Pemilik daemon: pasang hint `secret.needs_secret` di jalur `failure_body` (misalnya satu helper di `ipc::secret` yang dipanggil `ipc::operation` untuk kode area `secret`), dan hapus `needs_secret_error` serta `delivery_error` yang mati atau jadikan keduanya satu sumber untuk kedua jalur. Tambah assert `items` dan hint pada envelope operasi sungguhan di `apps/daemon/tests/supervisor/actions.rs` dan `apps/daemon/tests/skeleton.rs`, dan satu test create dengan sumber `bind` yang terkunci di `apps/daemon/tests/instance_create_secrets.rs`.

### 🟠 `hide()` membatalkan Show dan Copy yang sedang menunggu daemon tanpa umpan balik, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Secrets/RevealModel.swift:113`, `RevealModel.swift:141`, `RevealModel.swift:151`, `RevealModel.swift:168`, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Secrets/RevealableSecretText.swift:73`

**Problem**: Satu penghitung `generation` dipakai untuk dua maksud: menyembunyikan teks yang sudah tampil, dan membatalkan aksi yang sedang menunggu daemon. `hide()` (`RevealModel.swift:113` sampai `:119`) menaikkan `generation` dan mengubah `phase` dari `.working` menjadi `.idle`. `gather` lalu pulang `nil` di `:141` atau `:151`, sehingga `show()` (`:95`) dan `copy()` (`:107`) kembali tanpa menyentuh `phase`. Hasilnya `phase == .idle`: `note` menampilkan `EmptyView()` (`RevealableSecretText.swift:111`), tombol aktif lagi, tanpa teks dan tanpa pesan. `hide()` dipanggil oleh tombol Hide, oleh timer 30 detik (`RevealModel.swift:168` sampai `:169`), dan oleh `NSApplication.didResignActiveNotification` (`RevealableSecretText.swift:73` sampai `:77`), dan `copy()` memakai `generation` yang sama walau tidak pernah menampilkan apa pun. Saya cek alurnya: nilai sedang tampil (timer dipasang dengan `mine = g`), pengguna menekan Copy di detik ke 29 (`copy()` mengambil `mine = g`), timer menyala, `hide()` menaikkan `generation` ke `g + 1`, `gather` membuang nilai yang diterima, pasteboard tidak ditulis, dan layar diam.
**Why it matters**: Tiga urutan: (a) Copy saat nilai tampil dan timer 30 detik menyala di tengah permintaan, terkonfirmasi di kode; (b) pengguna pindah app saat daemon masih menjawab Show atau Copy, terkonfirmasi di kode; (c) Show atau Copy pertama pada login Keychain memunculkan dialog SecurityAgent (spec 0011 *Security model*: setiap build ulang ad hoc dan setiap ganti identity), app Stackly menjadi tidak aktif karena dialog itu, `appBecameInactive()` membatalkan aksi, dan nilai yang datang setelah pengguna menekan Allow dibuang. Urutan (c) belum dikonfirmasi di Mac ini. Kalau benar, reveal lewat GUI tidak pernah berhasil selama dialog masih dibutuhkan (Allow sekali selalu memunculkan dialog lagi), daemon tetap menulis baris audit reveal `succeeded` untuk nilai yang tidak pernah dilihat pengguna, dan pengguna tidak diberi tahu kenapa. Nilai tidak bocor, tetapi aksi eksplisit pengguna hilang diam diam, dan AC-19 serta AC-24 meminta hasil yang jelas (tampil, tersalin, atau gagal dengan pesan).
**Suggested fix**: Pemilik `StacklyUI/Secrets/RevealModel.swift`: pisahkan dua maksud itu. Biarkan `hide()`, timer, dan app tidak aktif hanya mengosongkan `shownText` dan menandai bahwa hasil Show yang datang kemudian tidak ditampilkan, lalu isi `phase` dengan `.failed` yang menjelaskan (misalnya "Hidden because Stackly went to the background. Press Show again.") alih alih `.idle`. Biarkan `copy()` selesai tanpa peduli `generation` (ia tidak menampilkan apa pun, dan `ConcealedPasteboard` tetap menjaga 90 detiknya), atau paling tidak laporkan `.failed` saat dibatalkan. Ubah `aHideWhileRevealingDiscardsTheResult` di `RevealModelTests.swift:192` supaya menegaskan pesan itu, tambah test Copy yang timer nya menyala di tengah jalan, dan cek urutan (c) sekali di Mac ini lewat record manual step 11.

## Minor

### Irisan 1, port platform, adapter Keychain, fake store

#### 🟡 Saklar interaksi tidak dipulihkan saat panic di dalam panggilan `AllowPrompt`, `crates/platform-macos/src/secret.rs:236`

**Problem**: `KeychainGuard::run` menyalakan interaksi (`:245`), menjalankan `call()`, lalu `self.disable()` (`:247`) berurutan, tanpa guard RAII. Kalau `call()` panic, interaksi tetap menyala untuk seluruh proses. `acquire()` berikutnya menyembuhkannya, tetapi panggilan berikutnya tidak pernah datang, karena panic itu mematikan thread `SecretWorker` (lihat Minor pertama irisan 3).
**Why it matters**: Invariant "interaksi mati kecuali selama satu panggilan `AllowPrompt`" (AC-4, *Key invariants*) bergantung pada urutan pernyataan, bukan tipe. Harm rendah: setelah worker mati tidak ada panggilan Keychain lagi, jadi tidak ada dialog yang bisa muncul.
**Suggested fix**: `impl Drop for KeychainGuard` yang memanggil `disable()`, atau guard kecil di `run`. Tambah test di `crates/platform-macos/tests/secret_store.rs` yang membaca `SecKeychain::user_interaction_allowed()` sebelum dan sesudah `read` `AllowPrompt` pada item milik proses (tanpa dialog).

#### 🟡 Salinan nilai di `CFData` tidak pernah dinolkan, `crates/platform-macos/src/secret.rs:369`, `crates/platform-macos/src/secret.rs:391`

**Problem**: `add` dan `update` memakai `CFData::from_buffer(value.expose().as_bytes())`, yang menyalin nilai ke buffer Core Foundation yang dibebaskan tanpa dinolkan; `CFData` hasil `SecItemCopyMatching` di jalur baca juga tidak dinolkan.
**Why it matters**: AC-14 menjanjikan nilai di memori dihapus saat drop; salinan ini di luar `SecretValue` dan belum dicatat sebagai residual risk di *Security model*.
**Suggested fix**: Untuk tulis, `CFData::from_arc` (`CFDataCreateWithBytesNoCopy`) dengan buffer `Zeroizing`; untuk baca, tambahkan satu kalimat residual risk di spec 0011 lewat `/architect`.

#### 🟡 Status kunci `Unknown` saat konstruksi diterima tanpa reaksi, `crates/platform-macos/src/secret.rs:322`

**Problem**: `MacosSecretStore::new` menyimpan `observed_state` apa adanya; bila `SecKeychainGetStatus` gagal saat start, konstruktor tetap `Ok` dan setiap panggilan berikutnya gagal `Backend`. Pemicu nyata untuk target `Login` belum dikonfirmasi.
**Why it matters**: Spec meminta daemon berhenti dengan jelas saat store tidak bisa disiapkan, "tidak pernah store yang diam diam terkunci".
**Suggested fix**: Perlakukan status yang gagal dibaca di konstruktor sebagai `KeychainSetupError::Keychain { step, path, code }`, supaya `apps/daemon/src/startup.rs:335` berhenti seperti kasus setup lain.

### Irisan 2, domain dan registry

#### 🟡 Slot `blocker` di aggregate `Instance` tidak dipakai produksi dan aturannya berbeda dari slot supervisor, `crates/domain/src/instance.rs:631`, `crates/domain/src/instance.rs:646`, `crates/core/src/supervisor/actor.rs:148`

**Problem**: `Instance::observe_blocker` dan `clear_blocker` hanya dipanggil test. Produksi memakai `ActorState.blocker` (`supervisor/secrets.rs:44`) dengan prioritas `identityUnproven`, `dataInUse`, lalu `needsSecret`; slot domain mengganti isi menurut `observed_at` saja, apa pun jenisnya.
**Why it matters**: Doc `InstanceBlocker` menyebut slot domain sebagai tempat blocker; fitur berikutnya yang memakainya akan membuat `needsSecret` menimpa `dataInUse`.
**Suggested fix**: Satu pemilik: hapus slot domain (update spec 0004 atau 0010 lewat `/architect`), atau pindahkan aturan prioritas ke `Instance::observe_blocker` dan buat `ActorState` memanggilnya.

#### 🟡 `AuditQuery` dirancang bertambah field tetapi tidak `#[non_exhaustive]`, `crates/domain/src/registry/secret.rs:644`, `crates/domain/src/registry/secret.rs:392`

**Problem**: Doc nya sendiri berkata "Feature 35 adds filters", tetapi struct ini (dan `SecretReferenceFilter`) tanpa `#[non_exhaustive]` dan tanpa konstruktor, jadi pemanggil menulis struct literal.
**Why it matters**: Melanggar aturan `AGENTS.md` untuk struct yang akan tumbuh; fitur 35 harus menyentuh setiap literal di testkit, storage, daemon, dan test.
**Suggested fix**: `#[non_exhaustive]` plus konstruktor (`AuditQuery::newest(limit)`, `before(self, cursor)`), sama untuk `SecretReferenceFilter`.

### Irisan 3, core worker dan service

#### 🟡 Thread worker yang mati membuat setiap panggilan store menjawab `Busy` selamanya tanpa jejak, `crates/core/src/secret/worker.rs:212`, `crates/core/src/secret/worker.rs:296`, `crates/core/src/secret/worker.rs:309`

**Problem**: `serve` memanggil `(job.run)(store)` tanpa `catch_unwind`. Panic di adapter mengakhiri thread `stackly-secret`; sejak itu `submit` menjawab `Busy` di `:212` tanpa log, sedangkan `state()` tetap meneruskan cache `available`.
**Why it matters**: Setelah satu panic, `secret.set` gagal `store_busy`, `resolve_for_start` memberi needs secret `store_busy` untuk semua reference, dan hint menyuruh "ulangi", padahal hanya restart daemon yang menolong. Adapter dan fake tidak punya `unwrap` di jalur produksi, jadi pemicunya jarang.
**Suggested fix**: Bungkus job dengan `catch_unwind`, catat satu baris `error`, kembalikan `OutcomeUnknown` (write, delete) atau `Backend` (lainnya); saat `sender.send` gagal, catat sekali dan biarkan `state()` menjawab `Unknown`. Satu fix dengan Minor pertama irisan 1.

#### 🟡 `secret.conflict` dari `Store { AlreadyExists }` di jalur operasi tidak membawa `reference` dan `reason`, `crates/core/src/secret/error.rs:366`

**Problem**: `details` untuk varian ini jatuh ke cabang penampung yang kosong, padahal katalog mendaftar `reference`, `reason`, dan `omitted`. Jalur IPC `secret.set` benar (`ipc/secret.rs:598`); jalur operasi create (`create.rs:668`) tidak.
**Why it matters**: Create yang tulisan targetnya menjawab `AlreadyExists` (item dibuat di luar daemon di antara probe dan `write`) gagal tanpa `reason`. Akar yang sama dengan Major pertama.
**Suggested fix**: Tambah cabang `Store { reference, error: AlreadyExists }` dengan `reference`, `reason: already_exists`, `omitted: 0`, di fix yang sama dengan Major pertama.

#### 🟡 Penulisan balik yang menemukan item sudah hilang dihitung gagal, `crates/core/src/secret/service/put.rs:348`, `crates/core/src/secret/service/put.rs:352`

**Problem**: Setelah commit registry gagal untuk item baru, `delete` yang menjawab `NotFound` menjadi `WriteBackFailed` (`secret.store_outcome_unknown`), padahal store sudah di keadaan akhir yang diinginkan. `delete_held` (`access.rs:211`) sudah memperlakukan `NotFound` sebagai berhasil.
**Why it matters**: Klien diminta "cek ulang" untuk kegagalan yang sebenarnya hanya registry. Jarang.
**Suggested fix**: Di cabang `None`, anggap `Err(NotFound)` dari `delete` berhasil, sama seperti `delete_held`.

### Irisan 4, Redactor, delivery, log daemon, penjaga `expose`

#### 🟡 Redaction envelope berjalan setelah `ErrorBody::new` memotong pesan di 200 karakter, kebalikan dari urutan spec, `crates/protocol/src/error.rs:138`, `apps/daemon/src/connection.rs:784` (dicek ulang)

**Problem**: Spec *Redaction* dan AC-14: pesan, detail, dan `items` lewat `Redactor` "sebelum `ErrorBody::new` memotong pesan di 200 karakter (supaya potongan nilai tidak lolos)". `ErrorBody::new` memotong di konstruktor (`error.rs:142`), dan `map::redact::error` baru berjalan di `connection.rs:792`. Nilai yang dikenal hanya cocok kalau utuh, jadi nilai yang terpotong di batas 200 lolos sebagai awalan. Jalur engine sudah benar (`crates/core/src/operation/state.rs:493`).
**Why it matters**: Laten: tidak ditemukan handler yang menaruh teks bebas berisi nilai di pesan `ErrorBody::new` hari ini. Handler berikutnya yang memformat rantai error atau kutipan stderr child lebih dari 200 karakter akan membocorkan awalan nilai.
**Suggested fix**: Di `stackly-protocol` (spec 0006): simpan pesan tanpa potongan (atau dengan batas kasar besar, misalnya 4096 byte) dan potong ke `MESSAGE_MAX` di `redacted_with` atau saat serialisasi; tambah test dengan nilai dikenal di sekitar karakter ke 200 di `apps/daemon/src/map/redact.rs`.

#### 🟡 Kunci sensitif berkutip di pesan log tidak tertangkap karena formatter JSON mengubah `"` menjadi `\"`, `crates/core/src/secret/redact.rs:25`, `crates/core/src/secret/redact.rs:288`, `apps/daemon/src/log_sink/redacting.rs:76` (dicek ulang)

**Problem**: Writer log meredaksi baris JSON yang sudah diformat (AC-13), dan `tracing` meng escape setiap `"` di pesan dan field string. `SENSITIVE_KEY` menuntut `["']?` langsung menempel di kunci lalu `[ \t]*[=:]`, jadi `cfg {\"password\": \"hunter2\"}` di dalam `message` tidak cocok (sesudah `password` ada `\`). Cabang nilai berkutip di `key_values` juga mencari kutip penutup tanpa backslash. Field `tracing` bernama `password` tetap tertangkap (kuncinya kunci JSON, tidak di escape), begitu pula bentuk tanpa kutip dan nilai yang dikenal.
**Why it matters**: Lapis 3 adalah satu satunya pelindung nilai di bawah 8 byte dan nilai yang belum dikenal daemon. Satu `warn!(error = %err)` yang Display nya memuat potongan JSON config dengan `"password": "..."` lolos utuh ke `stacklyd.log` dan stderr.
**Suggested fix**: Di `secret::redact`: izinkan `\\?["']` di kedua sisi kunci dan di cabang nilai berkutip. Tambah kasus `"password": "x"` dan `{"authorization": "Bearer x"}` lewat formatter JSON sungguhan di `apps/daemon/tests/server/secret_log.rs`.

#### 🟡 Jalur `on_starting` mengirim error dan ringkasan operasi tanpa `Redactor`, `apps/daemon/src/connection.rs` (fungsi `on_starting`)

**Problem**: `run_request` melewatkan setiap error dan balasan operasi ke `map::redact` (`connection.rs:784` sampai `:792`), tetapi `on_starting` meng encode `ErrorFrame` dan `OperationAccepted` langsung.
**Why it matters**: Hanya dipakai `Handlers::test_starting_method` (`provisional: F10`), jadi tidak ada klien produksi yang mengenainya; tetapi ini satu satunya jalur keluar envelope tanpa seam AC-14, dan polanya mudah tersalin oleh method starting kontrak pertama.
**Suggested fix**: Lewatkan keduanya ke `map::redact::error` dan `map::redact::summary` sebelum encode, atau hapus `on_starting` saat method starting kontrak pertama masuk lewat `dispatch`.

### Irisan 5, protokol dan daemon IPC

#### 🟡 `secret.delivery_rejected` dari operasi start kehilangan detail `reason`, `crates/core/src/supervisor/error.rs:456`, `apps/daemon/src/ipc/secret.rs:445`

**Problem**: Akar yang sama dengan Major pertama. Katalog mendaftar `details ["reason"]`; `ensure_not_in_argv` di startup gagal sebagai `SupervisorError::SecretInArgv`, yang `details` nya kosong, dan `ipc::secret::delivery_error` yang menulis `reason` tidak punya pemanggil.
**Why it matters**: Paket yang salah menaruh secret di argv berhenti tanpa menyebut aturan yang dilanggar (`exact`, `assigns_value`, atau nilai panjang), satu satunya petunjuk aman yang spec izinkan (AC-12, AC-21). Tidak ada nilai yang bocor.
**Suggested fix**: Isi `details` `SecretInArgv` dengan `reason` di `crates/core/src/supervisor/error.rs`, di fix yang sama dengan Major pertama.

#### 🟡 `PackageFieldRules` menelan kegagalan baca paket tanpa log, `apps/daemon/src/secrets.rs:247`

**Problem**: `read_package(..).ok()` dan `from_files(..).ok()` menyimpan `None` di cache tanpa `tracing::warn`, sampai daemon restart.
**Why it matters**: Paket revisi provider yang rusak membuat `secret.set` pada reference instance lolos tanpa aturan field (misalnya 3 karakter untuk `minLength` 12) tanpa satu baris log.
**Suggested fix**: Catat `warn!` dengan `provider`, `revision`, dan error, dan jangan simpan `None` di cache supaya paket yang diperbaiki terbaca tanpa restart.

### Irisan 6, CLI input aman

#### 🟡 Ctrl+C di prompt tersembunyi mematikan proses saat terminal masih raw, `apps/cli/src/secret_input.rs:117`, `apps/cli/src/secret_input.rs:195` (dicek ulang)

**Problem**: rpassword 7.5.4 mematikan `ECHO | ICANON | ECHONL | ISIG` di `/dev/tty` (`src/unix.rs:133`, `:143`), jadi Ctrl+C datang sebagai byte 0x03 dan rpassword memanggil `libc::raise(SIGINT)` sendiri (`src/unix.rs:162`). CLI tidak memasang handler SIGINT sebelum prompt (handler tokio hanya di `wait.rs:172` dan `:467`), jadi proses mati di dalam `raise` sebelum `Drop` rpassword memulihkan termios, dan `prompt_once` tidak pernah memetakannya ke `cli.interrupted`.
**Why it matters**: Di zsh dan bash dengan job control, shell biasanya memulihkan tty; di `sh`, script pembungkus, `script`, atau pty alat lain, pengguna mengetik buta sampai `stty sane`.
**Suggested fix**: Pegang listener SIGINT selama prompt supaya `raise` tidak mematikan proses, petakan `ErrorKind::Interrupted` ke `cli.interrupted` (exit 130), dan pastikan Ctrl+C sesudah prompt tetap mengakhiri command.

#### 🟡 Teks daemon di daftar efek `stored_only` ditulis ke terminal tanpa escape, `apps/cli/src/commands/secret.rs:232`

**Problem**: `ask_stored_only` menulis `resourceKind`, `resourceId`, dan `field` dari `body.items()` apa adanya, padahal stderr error (`output/stderr.rs:126`) dan output manusia (`output/human.rs:39`) selalu memakai `escape_display` (spec 0008 AC-3).
**Why it matters**: Karakter kontrol atau override bidi dari nama field sampai ke terminal mentah tepat sebelum pertanyaan `[y/N]` yang memutuskan penggantian nilai. Harm kecil (daemon milik user yang sama).
**Suggested fix**: Bungkus ketiga nilai dengan `escape_display` dan tambah assert dengan `\u{1b}` di test `an_interactive_stored_only_replace_lists_the_effects_and_asks_with_no_as_the_default`.

### Irisan 7, Swift client dan GUI

#### 🟡 Menolak dialog konfirmasi `stored_only` tidak disampaikan ke model, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Secrets/SecretEntrySheet.swift:132`, `SecretEntryModel.swift:90`

**Problem**: `presentConfirmation()` hanya mengisi `perform`; saat ditolak, `StacklyConfirmationModifier.sync()` (`Confirmation/StacklyConfirmation.swift:44`) hanya mengosongkan `item`. `model.cancelConfirmation(reference:)` tidak pernah dipanggil view, jadi baris tetap `.needsConfirmation` dan `pendingConfirmation` tetap terisi; `effects` yang sudah diparse juga tidak dipakai di pesan dialog.
**Why it matters**: Gagal aman (tombol utama tetap tertahan, Replace lagi memunculkan dialog lagi), tetapi state model tidak mencerminkan layar, dan test `decliningTheConfirmationSendsNothingMore` membuktikan jalur yang tidak pernah ditempuh view.
**Suggested fix**: Panggil `model.cancelConfirmation` saat `confirmation` menjadi `nil` dengan baris `.needsConfirmation`, atau beri `ConfirmationRequest` closure `onCancel`; sebut instance dan field dari `pending.effects` di pesan dialog.

#### 🟡 `secretStatus` memakai batas 10 detik yang sama dengan `background_wait` daemon, `clients/swift/Sources/StacklyClient/Secrets/SecretsClient.swift:43`

**Problem**: `secret.status` selalu probe ulang lewat worker dengan `Never` (`background_wait` 10 detik, `crates/core/src/secret/worker.rs:39`), dan klien menyerah di `ContractLimits.callTimeout` 10 detik.
**Why it matters**: Saat worker tertahan dialog lain, pengguna melihat "stacklyd didn't answer in time" alih alih `unknown` atau `secret.store_busy` yang daemon jawab sesaat kemudian.
**Suggested fix**: Batas di atas `background_wait` (misalnya 15 detik) untuk `secretStatus`, dengan alasannya di doc comment.

#### 🟡 VoiceOver tidak bisa membaca nilai yang sudah ditampilkan, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Secrets/RevealableSecretText.swift:84` (dicek ulang)

**Problem**: `Text(shown)` diberi `.accessibilityLabel("Password, shown")` tanpa `accessibilityValue`, jadi VoiceOver membaca label itu saja dan tidak pernah nilainya.
**Why it matters**: AC-19 meminta komponen bisa dipakai penuh dengan VoiceOver; Show tidak punya hasil yang bisa diakses, padahal pengguna sendiri yang memintanya.
**Suggested fix**: Tambah `.accessibilityValue(shown)` di state tampil, dan satu assert di `SecretViewsAccessibilityTests`.

#### 🟡 Closure `compose` contoh memakai password kosong sebagai fallback, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Gallery/SecretsGalleryPage.swift:82`, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Secrets/SecretPreviews.swift:68`

**Problem**: Keduanya menyusun `postgres://admin:\(values[ref]?.value ?? "")@…`. Tidak terjangkau hari ini (`gather` hanya memanggil `compose` setelah semua nilai ada, dan keduanya hanya DEBUG).
**Why it matters**: Ini satu satunya contoh `compose` di repo dan fitur 21 akan menyalinnya; polanya melanggar aturan "nilai yang hilang tidak pernah menjadi auth kosong" (AC-11, *Key invariants*).
**Suggested fix**: Lempar error saat kunci tidak ada (`RevealModel` sudah membatalkan aksi saat `compose` melempar), dan catat pola itu di doc comment `RevealableSecretText`.

#### 🟡 Teks ketikan yang ditampilkan di `SecretField` tidak pernah tersembunyi lagi sendiri, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Secrets/SecretField.swift:21`, `SecretField.swift:84`

**Problem**: `showsTypedText` hanya kembali `false` saat sumber berganti atau view hilang, tanpa batas waktu dan tanpa reaksi pada app tidak aktif, padahal spec menyebut "tampilkan sementara saat mengetik".
**Why it matters**: Nilai yang diketik tetap terbaca selama sheet terbuka, termasuk saat pindah app; tidak konsisten dengan `RevealableSecretText`.
**Suggested fix**: Sembunyikan lagi pada `didResignActiveNotification` dan setelah `RevealModel.showDuration`, atau ubah kata "sementara" di spec bila perilaku sekarang yang diinginkan.

## Nits

Irisan 1:
- ⚪ `crates/platform-macos/src/secret.rs:621`, `buf.fill(0)` bisa dihapus optimizer, jalur error `copy_bytes` tidak menolkan `buf`, dan `CreateOptions::password` menyimpan `String` biasa; hanya root development dan test.
- ⚪ `crates/platform-macos/src/secret.rs:360`, `Unknown` dihitung tidak terkunci, jadi `errSecAuthFailed` saat status gagal dibaca menjadi `AccessDenied`, bukan `Locked`.
- ⚪ `crates/platform-macos/src/platform.rs:78`, `with_roots` melaporkan capability `SecretStore` Supported tetapi memasang `UnsupportedSecretStore`; sebut di doc bahwa pemanggil memasang store.
- ⚪ `crates/platform-macos/src/secret.rs:462`, `Upsert` dengan `AllowPrompt` bisa memunculkan dua dialog; catat di doc `write`.
- ⚪ `crates/platform-macos/src/secret.rs:448`, hasil cari tanpa `SearchResult::Data` dipetakan `NotFound`; `Backend { code: ERR_DECODE }` lebih jujur.
- ⚪ `crates/platform-macos/src/secret.rs:489`, path relatif `Library/Keychains/login.keychain-db` di pesan error.
- ⚪ `crates/platform/src/secret.rs:96`, `from_utf8` memvalidasi UTF-8 dua kali.
- ⚪ `crates/testkit/src/fake_secret.rs:246`, `deny` pada reference yang tidak tersimpan menjawab `AccessDenied`, Keychain menjawab `NotFound`.

Irisan 2:
- ⚪ `crates/storage/src/registry/secret.rs:348`, `views()` menjalankan satu query `uses_of` per reference (N+1).
- ⚪ `crates/domain/src/secret.rs:148`, `TooLong` dicek sebelum skema, jadi `keychain:` lebih dari 255 byte dijawab panjang, bukan `unsupported scheme`.
- ⚪ `crates/storage/migrations/0003_secret_references.sql:59`, primary key membolehkan dua reference untuk satu `(resource, field)`; index unik `(resource_kind, resource_id, field)` akan menjadikannya invariant database (spec 0005).
- ⚪ `crates/domain/src/registry/secret.rs:549`, `SafeDetails::get` memindai linear; paling banyak 20 entri.

Irisan 3:
- ⚪ `crates/core/src/secret/worker.rs:224`, coba `result.try_recv()` sebelum memutuskan `OutcomeUnknown` pada timeout `Running`.
- ⚪ `crates/core/src/secret/service/put.rs:310`, `store_key` dihitung setelah `write`; hitung sebelum langkah 6.
- ⚪ `crates/core/src/secret/service/access.rs:249`, audit delete yang berhasil hanya upaya; sebut di doc.
- ⚪ `crates/core/src/secret/rules.rs:210`, field tidak dikenal pada data yang sudah diinisialisasi menjadi `StoredOnly`, lebih ketat dari tabel spec; catat di spec.
- ⚪ `crates/core/src/secret/error.rs:80`, `ConflictReason` publik tanpa `#[non_exhaustive]`.
- ⚪ `crates/core/src/secret/service/probe.rs:139`, `Vec::contains` di dalam loop; `BTreeSet` cukup.
- ⚪ `crates/core/src/secret/error.rs:275` dan `service/audit.rs:19`, dua helper `key()` yang sama.
- ⚪ `crates/core/src/secret/service/access.rs:68`, `id.to_string()` per panggilan; parse segmen sebagai `InstanceId`.
- ⚪ `crates/core/src/secret/service/put.rs:507`, `unwrap_or(u64::MAX)` adalah fallback mati.
- ⚪ `crates/core/src/secret/service/audit.rs:29`, audit `failed` untuk `CopyFrom` atau `Bind` tidak menyebut reference yang gagal; satu entri `failedReference` menolong.

Irisan 4:
- ⚪ `apps/daemon/src/log_sink/mod.rs:455`, baris milik writer thread sendiri tidak lewat `Redactor`; isinya aman, sebut di doc modul.
- ⚪ `apps/daemon/src/startup.rs:187`, tanpa panic hook, pesan panic keluar ke stderr tanpa redaksi; aman hari ini, catat sebagai asumsi di *Security model*.
- ⚪ `crates/core/src/secret/redact.rs:296`, `eq_ignore_ascii_case("authorization")` sementara regex `(?i)` Unicode; satu komentar cukup.
- ⚪ `xtask/src/check_deps/source_scan.rs:119`, `stackly-platform secret` dan `stackly-cli secret_input` belum masuk `EXPOSE_ALLOWED` karena belum ada call site; tabel *Penjaga di kode* sebaiknya menyebut aturan "masuk saat ada call site".

Irisan 5:
- ⚪ `crates/protocol/src/secret.rs:56`, `SecretText` menurunkan `Clone`, `PartialEq`, dan `Eq` (perbandingan bukan constant time); satu kalimat doc bahwa itu disengaja.
- ⚪ `crates/protocol/src/secret.rs:137`, skema JSON `SecretText` memakai `writeOnly: true`, padahal tipe yang sama adalah output `RevealSecretReply.value`.
- ⚪ `apps/daemon/src/ipc/secret.rs:136` dan `:224`, `expectedRevision` 0 ditolak dengan tangan di handler; tempatnya di `Validate`.
- ⚪ `apps/daemon/src/secrets.rs:171`, `probe_all` mengembalikan `Option<ProbeReport>` yang dibuang pemanggil satu satunya.
- ⚪ `apps/daemon/src/secrets.rs:209`, aturan revision plus 1 untuk status `deleted` belum disebut spec 0011 *Permukaan API*.

Irisan 6:
- ⚪ `apps/cli/src/secret_input.rs:187`, rpassword 7.5 menulis `\n` sendiri, jadi `stderr.line("")` menambah satu baris kosong; doc comment menggambarkan rpassword lama.
- ⚪ `apps/cli/src/secret_input.rs:181`, "three times" ditulis literal padahal batasnya `PROMPT_ATTEMPTS`.
- ⚪ `apps/cli/src/secret_input.rs:288`, `mode & 0o077` menandai bit apa pun, padahal pesannya "can be read"; periksa `0o044` atau ubah kata.
- ⚪ `apps/cli/src/secret_input.rs:261`, tanpa `O_NOFOLLOW | O_NONBLOCK`; dev dan ino sesudah open menutup celah symlink, tetapi file yang ditukar menjadi FIFO membuat `open` memblokir.
- ⚪ `apps/cli/src/secret_input.rs:216` dengan `app.rs:284`, sampai 8 KiB nilai yang ditolak bisa tinggal di buffer `BufReader` tanpa dinolkan.
- ⚪ `apps/cli/src/secret_input.rs:117`, rpassword mengganti byte non UTF-8 dengan U+FFFD di jalur TTY, sedangkan `--stdin` dan `--from-file` menolaknya; informasi saja.

Irisan 7:
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Secrets/SecretEntrySheet.swift:112`, fallback `SecretDraft()` mulai dari `.generate`, juga untuk baris dengan `generateAllowed` false.
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Secrets/RevealableSecretText.swift:44`, model `@State` diisi sekali per identitas view; doc perlu menyebut `.id(...)` untuk fitur 21.
- ⚪ `clients/swift/Sources/StacklyClient/Secrets/RevealedSecret.swift:13`, penanda `****` ditulis di tiga tempat; satu konstanta publik menjaganya sama.
- ⚪ `apps/macos/Packages/StacklyUI/Tests/StacklyUITests/Secrets/SecretViewsAccessibilityTests.swift:11`, pemindaian API terlarang tidak mencakup `Gallery/SecretsGalleryPage.swift`.
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Secrets/SecretEntrySheet.swift:65`, judul dan kalimat sheet tidak ada di mockup; cocokkan dengan copy fitur 20 atau 21.

## Penilaian celah yang diketahui

| Celah | Tingkat | Alasan singkat |
| --- | --- | --- |
| Konfirmasi GUI untuk replace berefek `stored_only`, `DEFERRED` ke fitur 20 dan 21 | rendah | Jalurnya ada dan gagal aman: `secret.confirmation_required` menjadi `.needsConfirmation(effects:)`, dialog muncul, Replace mengirim ulang dengan `confirmStoredOnly: true`, tidak ada yang ditulis tanpa flag dan tidak ada loop. Sisa masalahnya Minor pertama irisan 7 (penolakan tidak sampai ke model, efek tidak disebut di dialog). Bukti GUI tetap menunggu layar yang punya reference milik instance. |
| Prefix `FixtureCredential` Swift dicocokkan dengan teks `FIXTURE_CREDENTIAL_PREFIX` di `crates/evidence/src/credential.rs` | rendah, bukan temuan | `FixtureCredentialTests.swift:50` naik lima tingkat dari `#filePath`, jadi tidak bergantung pada cwd (hanya pada tidak adanya `-file-prefix-map`, yang sudah dicatat di `clients/swift/AGENTS.md`). Setiap mode gagalnya berisik: file pindah membuat baca melempar, konstanta diganti nama gagal `#require`, nilai Rust yang berubah gagal karena pencocokan memuat kedua tanda kutip. Perbaikan kecil yang bisa dipertimbangkan: ekstrak literal lalu bandingkan sama persis supaya pesan gagal menyebut kedua nilai. |
| `providers/fake` merender `{{ secret.authToken }}` plaintext ke `${instance.config}/fake.json` (`sensitive: true`) | risiko Major, milik spec 0009 dan 0014, bukan kode fitur 11, tidak dihitung di verdict | Saya cek: `crates/core/src/provider/interpolate.rs:622` sampai `:624` menerima `ConfigDestination { sensitive: true }` dengan `PathBase::Dir(_)` apa pun; hanya kebalikannya yang ditolak (`:628`). `providers/fake/provider.yaml:27` sampai `:31` menulis `fake.json` ke folder config, dan start membacanya lewat `--config=${instance.config}/fake.json`. `remove_secret_material` dan `sweep_secret_material` hanya menyentuh `instance_secrets_dir` (`crates/core/src/secret/delivery/material.rs:147`, `:226`), jadi token tetap di root data setelah stop, tidak pernah disapu, dan ikut cold backup (`backup: [cold]`) serta semua pembaca folder config. Kode `delivery` fitur 11 benar untuk folder yang dimilikinya; yang hilang adalah penegakan "config `sensitive` wajib di `${instance.secrets}`" di matriks posisi spec 0009 (yang hari ini hanya bilang `${instance.secrets}` "diterima"), sedangkan spec 0011 mekanisme 5 sudah menyatakan aturannya. Hari ini hanya paket `testing/fake` yang terkena (tidak masuk katalog app), tetapi provider mekanisme 5 berikutnya (Valkey `requirepass`, NATS) akan lolos validasi dan meninggalkan password di root data. Rekomendasi: `/architect` update spec 0009 (config `sensitive` hanya di `${instance.secrets}`, ditolak `TokenNotAllowedHere` di `interpolate.rs`), lalu paket fake memakai `${instance.secrets}/fake.json` dan pengecualian `fake.json` di spec 0014 AC-18 dihapus supaya pindaian sentinel F14 ikut memeriksa folder config. |

## Di luar lingkup (untuk lead, tidak dihitung)

- `crates/core/src/supervisor/reconcile.rs:346` sampai `:352` (fitur 10, pemanggil API fitur 11): `sweep_secret_material` mengembalikan `Ok(report)` dengan `report.failed` berisi folder `secrets/` yang gagal dihapus, tetapi reconcile hanya mencatat `Err` dari seluruh sweep. File plaintext mekanisme 5 milik instance yang tidak berjalan bisa tertinggal tanpa satu baris log. Saran: satu `warn!` per entri `report.failed`.
- `crates/core/src/supervisor/init.rs:247` sampai `:257` (fitur 10): `materialize_secret` dan drop guard `SecretFile` menjalankan IO file di thread runtime tokio. Sudah tercatat sebagai Minor di review F10 (*Panggilan filesystem sinkron berjalan di thread runtime tokio*), jadi tidak dihitung lagi.

## Strengths

- Tidak ada tipe yang bisa membawa nilai ke tempat yang salah: `SecretValue` tanpa `Clone`, `Display`, `PartialEq`, atau serde, dinolkan di setiap jalur error konstruktor, dan `Redactor` menyimpan salinannya sebagai `SecretValue` dengan `Debug` yang hanya mencetak jumlah. Domain dan registry hanya memuat ID, teks reference, store key, kode, dan waktu; `SafeDetails` dibatasi 20 entri dan 512 byte di konstruktor, dan suite sentinel memindai frame, log daemon pada level debug, stderr, serta byte registry beserta WAL.
- `map_status` murni, `const fn`, dan terkunci tabel per baris; setiap panggilan adapter membaca `SecKeychainGetStatus` di bawah guard yang sama sebelum dilepas, jadi `state()` dan `keychain_locked` tidak pernah berbeda pendapat. Setiap operasi terpatok ke satu keychain, search list user tidak disentuh, dan blok `unsafe` tunggal punya `// SAFETY:` yang tepat. `FakeSecretStore` mengikuti hasil spike (terkunci: probe, delete, dan `NotFound` tetap jalan).
- `SecretWorker` membuat "job kadaluarsa tidak pernah berjalan" dan "job terlambat memanggil probe" eksplisit lewat `Stage` yang dibagi, hook nya berjalan di thread sendiri dan dipasang lewat `Weak`, dan setiap panggilan service dari daemon lewat `spawn_blocking` tanpa lock yang melintasi `.await`. Urutan `put` mengikuti AC-5 langkah demi langkah, dengan `learn` sebelum pemeriksaan dan tulisan apa pun, penulisan balik yang teruji, dan reveal yang gagal tertutup.
- `Redactor::redact` menghitung ketiga lapis dari teks yang sama lalu menggabungkan range, aman di batas karakter UTF-8, stabil bila diredaksi dua kali, dan mengembalikan `Cow::Borrowed` untuk teks bersih. `RedactingWriter` meredaksi satu baris utuh saat drop, jadi nilai yang terbelah dua `write` tetap tertangkap, dan satu `Arc<Redactor>` dipakai writer log, engine operasi, bus event, probe runner, spool, dan service.
- Pemindai `expose` membaca method call, path, isi makro, dan argumen atribut; allowlist nya subset tabel spec, dan test `every_allowed_expose_module_of_this_workspace_holds_a_call_site` mencegahnya melebar diam diam. `ensure_not_in_argv` berjalan di preflight, sebelum init, dan tepat sebelum setiap spawn.
- CLI tidak punya flag yang membawa nilai, membaca stdin lewat `take(MAX_READ + 1)` ke buffer berkapasitas pas yang dinolkan di setiap jalur tolak, gagal `cli.input_required` sebelum membaca apa pun saat noninteractive, dan hanya `reveal` yang memanggil `expose()`. Di Swift, `RevealedSecret`, `FixtureCredential`, dan `SecretDraft` menyamarkan setiap bentuk teks dan sengaja tidak `Codable`, `Equatable`, atau `Hashable`; `ConcealedPasteboard.shared` memegang task pengosongan 90 detik yang hidup melewati view.

## Test coverage

Configured (`cargo test`, `swift test`). `verify.md` mencatat cakupan luas yang sudah lulus: conformance registry secret di fake dan SQLite, migrasi 0003 dan fixture beku, worker (`Busy`, `OutcomeUnknown` lalu probe, default 180 dan 10 detik), `put` empat input kali tiga mode, status dan probe, reveal dan delete dengan audit, `resolve_for_start`, `printable_only`, `classify_create_failure` per baris, value sourcing, delivery dan symlink, `map_status` dan keychain file nyata di bawah `TestRoot` (terkunci dan ACL asing), katalog 13 kode, server `secret` 12 test, sentinel daemon dengan perekam frame, log redaksi di lima level, CLI 12 test plus sentinel lewat CLI, dan test Swift untuk penyamaran, timer, pasteboard, dan model. Tidak ada temuan yang bertentangan dengan langkah yang `verify.md` tandai PASS; dua record manual step 7 dan record step 11 masih `NOT_RUN`.

Celah test yang terkait temuan di atas (tidak dihitung sebagai temuan sendiri):
- `items` dan hint pada envelope `secret.needs_secret` dari operasi start dan create sungguhan, termasuk create dengan sumber `bind` yang terkunci, dan `reason` pada `secret.delivery_rejected` dari startup (Major pertama);
- Copy yang dibatalkan timer 30 detik atau app tidak aktif di tengah permintaan, dan reveal pertama dengan dialog Keychain di Mac ini (Major kedua; `aHideWhileRevealingDiscardsTheResult` justru menegaskan `.idle` yang diam);
- saklar interaksi sebelum dan sesudah panggilan `AllowPrompt`, thread worker yang mati, dan `submit` setelah channel putus;
- kunci sensitif berkutip yang lewat formatter JSON sungguhan, dan pesan error lebih dari 200 karakter dengan nilai dikenal di sekitar batas potong;
- `Store { AlreadyExists }` di jalur operasi, penulisan balik yang menjawab `NotFound`, `PackageFieldRules` dengan paket yang tidak terbaca;
- Ctrl+C (byte 0x03) di prompt pty lalu keadaan termios, dan escape teks daemon di daftar efek `stored_only`;
- penolakan dialog konfirmasi yang tersambung ke `cancelConfirmation`, `secretStatus` terhadap batas tunggu worker, `accessibilityValue` state tampil, dan Show atau Hide di `SecretField`;
- cabang `remove_created` tanpa baris registry (AC-23, `OutcomeUnknown` pada tulisan target), satu reference yang dipakai dua field dari instance yang sama di `resolve_for_start`, dan `RestoreSecretReference` dengan `id` yang sudah dipakai;
- pindaian sentinel F14 yang sengaja mengecualikan `fake.json`, sehingga celah fake provider tidak terlihat oleh suite mana pun.
