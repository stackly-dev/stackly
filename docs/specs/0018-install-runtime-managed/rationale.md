# 0018. Install runtime managed: rationale

[Kembali ke spec](index.md)

## Context

Fitur 18 adalah titik pertama Stackly mengambil byte dari internet dan menjalankannya di Mac pengguna. Fitur 3 sudah membuktikan, lewat spike xtask di Mac ini, bahwa delapan artefak (PostgreSQL 17 dan 18, MySQL 8.4 dan 9.7, NATS, ClickHouse, Valkey, Traefik) jalan dari folder managed tanpa Homebrew, dan digest serta ukurannya dipin di `runtimes/inventory.toml`. Spike itu boleh memakai tool developer (`otool`, `lipo`, `/usr/bin/tar`) karena hanya jalan di Mac maintainer. Daemon di Mac pengguna tidak boleh: AC-049 menuntut provider wajib bisa dipakai tanpa developer toolchain, dan `otool` serta `lipo` di macOS dasar hanya shim yang meminta Command Line Tools.

Gaya kerja daemon sudah dikunci spec lain. Setiap mutasi adalah operasi spec 0007 dengan journal, lock per resource, dan aturan "efek filesystem di path milik operasi dulu, registry terakhir"; `install_runtime` sudah tercatat resumable dengan catatan "download, verifikasi, dan ekstrak di scratch; commit lewat rename atomik lalu `finalize`". Kompensasi hanya boleh menghapus yang dimiliki operasi karena konstruksi (spec 0007 AC-8), dan `remove_tree` berbasis file descriptor adalah satu satunya primitive hapus. Core harus portabel (cek lintas target Windows), port platform sinkron dan object safe, dan `unsafe` hanya di `stackly-platform-macos`. Spec 0001 menyerahkan pilihan HTTP client dan ekstraksi archive ke fitur ini.

Kekuatan lain yang membentuk desain:

- **Ancaman archive dan unduhan** (VAL-010, VAL-011, AC-057, AC-059, AC-060, PRD bagian 23): traversal, symlink escape, archive bomb, file device, setuid, redirect ke HTTP, dan unduhan yang terpotong. Digest yang dipin menutup sebagian besar risiko, tetapi PRD meminta pertahanan berlapis dan fuzzing untuk parser archive (NFR-009, bagian 32.1).
- **Follow up spec 0003**: extractor harus menolak `..`, penulisan lewat symlink, dan sebaiknya path absolut (bsdtar hanya membuang `/`), mengabaikan AppleDouble di kedua format (bsdtar masih memulihkan xattr dari `._<nama>` di `tar_gz`), meniru aturan mode 0755 `zip`, dan menjalankan ulang kasus AC-16 terhadap pembaca zip sendiri.
- **Data spike**: MySQL membawa 34 symlink internal, sebagian dengan `..` (misalnya `bin/libprotobuf.24.4.0.dylib -> ../lib/libprotobuf.24.4.0.dylib` dan rantai `lib/libfido2.dylib -> libfido2.1.dylib -> libfido2.1.15.0.dylib`), jadi "tolak semua symlink" tidak mungkin. Probe quarantine spike menunjukkan binary ad hoc dan linker signed yang diberi quarantine mati karena SIGKILL (PostgreSQL, Traefik) atau menggantung di balik dialog Gatekeeper sampai batas 30 detik (NATS, Valkey), sementara binary Developer ID tetap jalan (MySQL, ClickHouse). Itu inti Done when "ditolak OS saat spawn menghasilkan error terdiagnosis, bukan hang".
- **Offline dan koneksi buruk** (REQ-046, AC-004): ClickHouse 216 MB dan MySQL 168 MB; mengulang unduhan dari nol setiap kali koneksi putus tidak bisa diterima, dan pesan offline harus jujur.
- **Compatibility record** (BR-025, PRD bagian 32.2): `SUPPORTED` terikat matrix yang diuji, versi tanpa record tetap `UNVERIFIED`, dan record harus memuat provider revision. Evidence yang ada hari ini (spike) menguji artefak, bukan integrasi provider.
- **Batas dengan spec paralel**: spec 0017 memiliki model baca catalog, gerbang install, dan `SupportStatus`; spec 0019 menulis manifest PostgreSQL; spec 0009 masih dipegang sesi penutupan fitur 9, jadi kebutuhan field baru ditulis di sini sebagai usulan additive.

## Options considered

### Keputusan 1: bentuk operasi install

**Option 1 (dipilih): satu operasi resumable dengan lima langkah, baris registry hanya di `finalize`.** Langkah `download`, `verify_digest`, `extract`, `validate_binary` repeatable dan reversible; `commit` repeatable dan menjadi titik tak bisa dibalik.

- Pro: cocok persis dengan tabel resume spec 0007 dan lima tahap mockup; registry tidak pernah melihat runtime setengah jadi (REQ-004); cancel dan kompensasi gratis dari engine.
- Kontra: state runtime `downloading` dan seterusnya tidak pernah disimpan; tampilan "sedang dipasang" harus dibaca dari operasi, bukan dari baris runtime.

**Option 2: sisipkan baris runtime `resolving` di awal lalu `UpdateRuntime` per tahap.**

- Pro: state machine runtime terlihat langsung di registry dan `runtime.list`.
- Kontra: melanggar aturan "registry terakhir"; butuh kompensator kustom untuk baris itu; crash di tengah meninggalkan baris `downloading` yang harus dibersihkan doctor; unique index per artefak menahan retry.

**Option 3: dua operasi (download ke cache, lalu install dari cache).**

- Pro: unduhan bisa dipakai ulang lintas install tanpa journal bersama.
- Kontra: dua operasi untuk satu aksi pengguna, dua jejak di Operations, dan cancel serta progress yang terpecah; mockup menampilkan satu operasi lima tahap.

### Keputusan 2: HTTP client

**Option 1 (dipilih): `ureq` 3 sinkron dengan rustls dan `rustls-platform-verifier`, di port `Fetcher` dengan adapter di `stackly-platform-macos`.**

- Pro: API sinkron cocok dengan pola port spec 0004 dan `spawn_blocking`; dependency kecil; verifikasi sertifikat lewat trust store dan setelan trust macOS (root perusahaan dan MDM ikut); dekompresi otomatis bisa dimatikan; mudah diuji dengan server loopback.
- Kontra: tidak membaca setelan proxy sistem; rustls dengan ring membawa kode C dan lisensi ISC; cancel di tengah baca bergantung pada timeout baca dan flag stop di antara chunk.

**Option 2: `reqwest` async dengan rustls.**

- Pro: ekosistem terbesar, async alami di tokio, membaca proxy sistem macOS lewat `system-configuration`.
- Kontra: graph dependency jauh lebih besar (hyper, h2, tower); port async keluar dari pola port sinkron semua spec; cancel lebih mudah tetapi pola lain di daemon tetap sinkron.

**Option 3: `NSURLSession` lewat `objc2` di adapter macOS.**

- Pro: proxy, PAC, dan trust sistem persis seperti aplikasi macOS; tanpa crate TLS.
- Kontra: permukaan `unsafe` besar (delegate, run loop, callback lintas thread); lebih sulit diuji tanpa jaringan; adapter Windows nanti harus ditulis ulang dari nol.

**Option 4: `/usr/bin/curl` sebagai proses dari adapter.**

- Pro: tanpa dependency Rust baru; resume `-C -` sudah ada.
- Kontra: progress dan error dari teks dan exit code; proxy hanya dari environment; satu proses lagi yang harus diawasi; perilaku TLS mengikuti build curl sistem.

### Keputusan 3: ekstraksi archive

**Option 1 (dipilih): extractor Rust sendiri di `stackly-core` di atas `tar`, `flate2`, dan `zip` yang hanya membaca entri, menulis lewat port `TreeWriter` berbasis `openat` dengan `O_NOFOLLOW` dan `O_EXCL`.**

- Pro: aturan yang sama untuk ketiga format; path absolut ditolak, bukan dibuang; AppleDouble diabaikan di kedua format dan xattr tidak pernah ditulis; bisa difuzz (NFR-009); berjalan di Mac tanpa tool developer; batas byte dan entri diterapkan sebelum menulis.
- Kontra: kode keamanan milik sendiri yang harus direview dan dirawat; perilaku bisa berbeda dari bsdtar pada archive tidak lazim, jadi `pin` dan test native menjadi gerbangnya.

**Option 2: `/usr/bin/tar` seperti spike.**

- Pro: perilaku sama persis dengan evidence spike; tanpa crate baru.
- Kontra: tidak bisa difuzz dari Rust; path absolut hanya dibuang; `._<nama>` di `tar_gz` masih bisa memulihkan xattr (termasuk quarantine milik pembuat archive); batas archive bomb tidak bisa diterapkan di tengah stream; error hanya dari teks stderr.

**Option 3: `ditto` atau `unzip`.**

- Pro: tool sistem.
- Kontra: spec 0003 sudah mencatat keduanya diam diam menulis ulang anggota `..`, dan keduanya tetap tidak bisa difuzz atau dibatasi.

### Keputusan 4: pembacaan Mach-O

**Option 1 (dipilih): crate `object` di core, hanya header dan load command.**

- Pro: portabel, tanpa proses, bisa difuzz (`runtime_macho`), dipakai luas (backtrace std); cukup untuk slice, `LC_BUILD_VERSION`, dylib, dan rpath.
- Kontra: satu dependency lagi di core; aturan penyelesaian `@rpath` harus ditulis sendiri.

**Option 2: `otool` dan `lipo` seperti spike.** Kontra yang menentukan: keduanya shim yang butuh Command Line Tools di Mac bersih (AC-049).

**Option 3: parser tulisan tangan.** Pro: tanpa dependency. Kontra: kode parser biner baru tanpa riwayat pemakaian, untuk format dengan banyak varian (universal 32 dan 64 bit, arm64e).

### Keputusan 5: pemeriksaan signature

**Option 1 (dipilih): `/usr/bin/codesign --verify --strict` dan `-dv --verbose=2` dengan parser yang sama dengan spike, lewat port `CodeSigning`.**

- Pro: perilaku sama dengan evidence yang membuat artefak `available`; `codesign` bagian macOS dasar; tanda tangan ad hoc tetap butuh `codesign` karena tidak ada API publik untuk itu.
- Kontra: satu proses per file Mach-O; parser teks output manusia.

**Option 2: Security.framework (`SecStaticCodeCheckValidity`).**

- Pro: tanpa proses, hasil terstruktur.
- Kontra: flag yang setara `--strict` harus dibuktikan sendiri; jalur kode berbeda dari evidence; tanda tangan ad hoc tetap lewat `codesign`, jadi dua mekanisme.

### Keputusan 6: letak unduhan dan resume

**Option 1 (dipilih): `.part` per sha256 di root `cache`, dengan sidecar validator.**

- Pro: install baru dengan key lain (setelah gagal atau cancel) melanjutkan byte yang sama (AC-004); entri cache terverifikasi memungkinkan install offline (REQ-046); scratch tetap kecil.
- Kontra: file di luar scratch tidak dikompensasi, jadi `.part` yang ditinggalkan butuh pembersihan fitur 55; dua install artefak yang sama harus diserialkan lewat lock.

**Option 2: unduh ke scratch operasi.**

- Pro: semua efek milik operasi, terhapus otomatis.
- Kontra: unduhan yang gagal hilang bersama scratch, sehingga retry mulai dari nol; tidak ada install offline dari cache.

### Keputusan 7: commit

**Option 1 (dipilih): rename tanpa replace dari staging ke `runtime_dir`, marker di root runtime, dan pengambilalihan folder yatim.**

- Pro: satu rename atomik di volume yang sama; resume setelah crash tahu apakah rename sudah terjadi (marker); folder sisa operasi yang di abandon tidak menjadi konflik permanen dan tidak pernah dihapus langsung di `runtimes/`.
- Kontra: satu file tambahan (`.stackly-runtime.json`) di setiap runtime; aturan "yatim" bergantung pada baca registry dan journal operasi lain.

**Option 2: tulis registry dulu dengan state `installing`, lalu rename.**

- Pro: tidak ada folder tanpa baris.
- Kontra: baris yang menunjuk folder yang belum ada; butuh kompensator kustom; melanggar aturan registry terakhir spec 0007.

### Keputusan 8: exec check

**Option 1 (dipilih): field manifest opsional `runtime.versionCheck`, dijalankan sekali dari staging lewat `spawn_helper`.**

- Pro: membuktikan binary benar benar bisa dijalankan OS di Mac itu (tahap Validate binary) dan memberi tempat untuk diagnosis SIGKILL, hang, dan `EBADARCH`; cara membaca versi adalah pengetahuan provider, jadi tempatnya di manifest.
- Kontra: menjalankan kode artefak sekali saat install; butuh update spec 0009; provider tanpa field itu hanya mendapat validasi statis.

**Option 2: validasi statis saja, diagnosis di start instance.**

- Pro: tidak menjalankan apa pun saat install.
- Kontra: masalah OS baru terlihat saat create atau start, jauh dari aksi install; Done when fitur 18 tidak terpenuhi di fitur ini sendiri.

**Option 3: perintah versi per artefak di `versions.json`.** Kontra: perintahnya sama untuk semua versi satu provider, jadi duplikasi per artefak dan bukan data catalog.

### Keputusan 9: compatibility record

**Option 1 (dipilih): record dibangkitkan dari evidence native yang di commit ke repo, dibawa catalog, dan disalin ke registry saat install; scope `runtime` memberi `experimental`, scope `provider` memberi `supported`.**

- Pro: setiap klaim bisa ditelusuri ke run evidence; record ikut aplikasi yang ditandatangani, tanpa layanan online; status jujur tentang apa yang sudah diuji (artefak saja lawan integrasi provider).
- Kontra: semua runtime tampil `experimental` sampai test provider menulis record scope `provider`; generator dan perintah salin baru di xtask.

**Option 2: record dari evidence spike dihitung `supported`.**

- Pro: status hijau segera untuk delapan artefak.
- Kontra: spike tidak memakai provider revision atau manifest; PRD bagian 32.2 meminta provider revision dan BR-025 mengikat `SUPPORTED` ke matrix yang diuji, jadi klaim itu berlebihan.

**Option 3: tidak ada record sampai fitur 65.** Kontra: Done when fitur 18 meminta record untuk versi yang dipasang.

### Keputusan 10: arti `@stable`

**Option 1 (dipilih): penanda eksplisit `stable: true` pada satu artefak per target.**

- Pro: maintainer bisa menahan line baru yang punya masalah; tidak perlu urutan versi (VAL-003).
- Kontra: field baru di inventory dan `versions.json`; lupa mengisinya membuat `@stable` gagal `no_stable_artifact`.

**Option 2: line tertinggi secara numerik.** Pro: tanpa field. Kontra: "stable" tidak selalu line terbaru, dan aturan itu memaksakan urutan versi yang PRD minta dihindari.

### Keputusan kecil

- **Format `binary`**: tetap didukung. Biayanya satu cabang di extractor, dan menolaknya membuat entri catalog `available` yang tidak bisa dipasang. Runner up: tolak dengan `runtime.format_mismatch` sampai ada entri pertama.
- **Mode akhir read only**: file 0444 atau 0555 dan folder 0755. Mencegah proses provider menulis ke binary nya sendiri, sementara hapus fitur 55 tetap bisa lewat `unlinkat`. Runner up: mode dari archive apa adanya (perilaku spike).
- **Antrian lawan tolak untuk install artefak yang sama**: `BusyPolicy::Queue`, karena install kedua cukup selesai `alreadyInstalled` setelah yang pertama. Runner up: `Reject` dengan `resource.busy`.
- **Batas unpacked sebagai batas atas**: `unpackedSize` dan `entryCount` hanya batas atas, supaya perubahan kecil aturan lewati di scanner tidak membuat artefak lama gagal dipasang.
- **Provider revision di `artifacts`**: tetap NULL, karena runtime managed dipakai semua revision; kesepakatan dengan spec 0017.

## Rationale

Bentuk operasi mengikuti spec 0007 apa adanya: aturan registry terakhir dan kompensasi hanya atas milik sendiri membuat Option 1 satu satunya bentuk yang tidak butuh kompensator kustom, dan tabel resume sudah menulis polanya. Lima langkah sengaja sama dengan mockup supaya GUI, CLI, dan Operations memakai satu bahasa.

Pilihan HTTP client ditentukan port sinkron dan ukuran graph dependency. `ureq` dengan verifier platform memberi verifikasi sertifikat yang sama dengan aplikasi macOS lain tanpa membawa stack async kedua, dan mematikan dekompresi otomatis itu penting: `Content-Encoding: gzip` pada `.tar.gz` bisa membuat byte yang di hash bukan byte yang dipin. Kekurangan proxy dicatat jujur sebagai Follow-up, karena PRD tidak menyebut proxy dan tidak ada pengguna proxy di pilot awal yang diketahui.

Extractor dan parser Mach-O sendiri adalah konsekuensi langsung AC-049 dan NFR-009: tool developer tidak boleh dipakai di Mac pengguna, dan parser archive harus difuzz. Aturan symlink dibuat dari data MySQL nyata, bukan dari aturan umum: target relatif yang diselesaikan secara leksikal, komponen perantara yang harus folder asli, dan pembuatan symlink paling akhir membuat resolusi leksikal sama dengan resolusi fisik, sehingga symlink dengan `..` yang aman tetap diterima tanpa membuka jalan escape.

Signature tetap lewat `codesign` karena alasan sebaliknya: di sana kesetaraan dengan evidence spike lebih berharga daripada kecepatan, dan `codesign` ada di setiap Mac. Exec check di manifest menutup Done when yang paling sulit: data spike menunjukkan penolakan OS datang sebagai SIGKILL atau hang, dan hanya menjalankan binary sekali dengan batas waktu yang bisa membedakan keduanya sebelum pengguna membuat instance.

Compatibility record mengikuti BR-025 secara harfiah. Evidence spike membuktikan artefak berjalan, jadi itu `experimental`; `supported` menunggu test yang memakai provider revision. Record ikut catalog yang ditandatangani dan disalin ke registry supaya runtime terpasang tetap punya catatan ujinya walau catalog berganti (AC-003).

## Bukti tambahan

Daftar isi delapan artefak inventory di cache spike Mac ini (`~/Library/Caches/dev.stackly.xtask/runtime/<sha256>`, dibaca dengan `tar -tv` dan `zipinfo` tanpa ekstraksi, 2026-10-19):

| Artefak | Format | Anggota | Symlink | Hard link | AppleDouble |
| --- | --- | --- | --- | --- | --- |
| PostgreSQL 17.11 r2 | `tar_gz` | 1 989 | 0 | 0 | 0 |
| PostgreSQL 18.6 r2 | `tar_gz` | 2 031 | 0 | 0 | 0 |
| MySQL 8.4.11 | `tar_gz` | 375 | 34 | 0 | 0 |
| MySQL 9.7.2 | `tar_gz` | 388 | 34 | 0 | 0 |
| NATS 2.15.1 | `tar_gz` | 3 | 0 | 0 | 0 |
| Traefik 3.7.15 | `tar_gz` | 3 | 0 | 0 | 0 |
| Valkey 9.1.2 r2 | `tar_gz` | 16 | 0 | 0 | 0 |
| ClickHouse 26.8.30.8 | `zip` (dibuat di Unix, deflate) | 1 | 0 | 0 | 0 |

Penolakan hard link dan AppleDouble tidak memblokir artefak mana pun hari ini. Symlink MySQL berbentuk `bin/<lib> -> ../lib/<lib>`, `lib/<lib> -> <lib versi>` (termasuk rantai dua tingkat), dan `lib/plugin/<lib> -> ../../lib/<lib>`; semuanya lolos aturan symlink spec ini. Rasio ukuran terekstrak terhadap unduhan paling besar 4,13 (ClickHouse), dasar cadangan `5 × size` untuk preflight disk.

## Cross check

Cross check `Another model` (Fable 5.1, baca saja) pada 2026-10-19 menemukan 14 gap kelengkapan keputusan dan 7 masalah soundness. Tiga gap pertama (nama reason gerbang, `catalog.provider_not_found`, dan `support_status` atas `VersionArtifact` dengan `SupportReason`) diselaraskan dengan spec 0017 di `main`, karena spec itu pemiliknya. Sisanya diputuskan lead atas nama user dengan aturan "opsi yang direkomendasikan diambil" (log koordinasi scope 02) dan diterapkan:

| Gap | Keputusan |
| --- | --- |
| Hasil operasi di wire | tanpa payload baru: runtime dari `result.resources[0]`, `alreadyInstalled` saat ID itu berbeda dari resource operasi |
| Kode saat semua URL gagal | offline, lalu untrusted, lalu too large, lalu failed |
| Entri planned atau invalid | dicek sebelum resolusi dan dikirim ke gerbang |
| Preflight disk tanpa `unpackedSize` | `5 × size + 64 MiB` |
| Record saat artefak hilang dari catalog | record host dibekukan di input journal saat `begin` |
| Kegagalan yang belum punya kode | ENOSPC menjadi `insufficient_space`, IO lain `runtime.io_failed`, sinyal selain SIGKILL `binary_check_failed` |
| "{n} installed" di kartu Catalog | dibangun fitur ini, join di client |
| Tipe sha256 di wire | `String` dengan `Validate` |
| Pemetaan `checks` | snake case langkah dan probe PASS dari `spike-result.json` |
| Kriteria `--cleanup` | argv memuat `/stackly-test-` |
| Batas memori AC-8 | puncak alokasi di bawah `max_entries × 2 KiB + 1 MiB`, cadangan `max_entries` 50 000 |
| File cache diganti di antara langkah | langkah verify tetap gerbang, dan extract menghitung ulang digest stream nya |
| Aturan panjang dan resume | `Content-Length` dan `Content-Range` per jenis respons, `Range` dikirim ulang di redirect, 416 mulai dari 0 |
| Folder yatim dengan operasi yang dipangkas | yatim kalau operasi marker berakhir atau sudah tidak ada |

Perbaikan non keputusan yang ikut diterapkan: folder exec check dibuat 0700 sebelum spawn, aturan zip (komentar EOCD, data descriptor, `S_IFLNK` hanya dari host Unix), nama `max_total_file_bytes`, letak modul `fuzzing`, dan tabel bukti di atas. Atas catatan lead, perubahan spec 0009 dikumpulkan di satu bagian *Usulan update spec 0009*, dan langkah native yang butuh jaringan ditandai "setelah persetujuan kamu".

## Pemeriksaan konsistensi lintas spec (2026-10-20)

Pemeriksaan lintas spec 0015, 0017, 0018, dan 0019 oleh lead menemukan 22 temuan milik spec ini (5 major), masing masing dicek ulang reviewer kedua. Semua diterapkan dengan perbaikan yang direkomendasikan (aturan user), dengan tiga catatan:

- Temuan yang muncul dua kali dari dua batas diterapkan sekali: `HostFacts` di `stackly-platform` (tiga temuan), selector `sha256:` yang dibandingkan dengan artefak gerbang (dua temuan), copy `SupportReason` baru (dua temuan), dan aturan pencocokan `versionCheck` (dua temuan).
- Dua temuan `versionCheck` mengusulkan aturan berbeda (cocok penuh baris pertama lawan cari per baris). Yang dipakai adalah cari per baris tanpa `\A…\z`, karena cocok dengan output beberapa baris (`traefik version`) dan dengan preseden spike (`v([0-9.]+)`), dan pattern PostgreSQL spec 0019 yang membawa `^` dan `$` sendiri tetap cocok; unit test output harfiah `postgres (PostgreSQL) 18.6\n` dari temuan lain tetap ditambahkan.
- Keputusan lead yang menyentuh spec lain: bentuk hint `codes::<area>::hint` lewat `ErrorBody::with_hint` menjadi usulan spec 0006; `CatalogArtifact.sha256` ditambah spec 0017 sendiri dan hanya dirujuk di sini; `ProviderSummary.software` dan `InstallingRuntime.provider` tetap di usulan spec 0017 spec ini; langkah catalog di `cargo xtask build-app` milik spec 0017 milestone 4 (spec ini tidak menyebutnya); test native per lini dibatasi ke provider yang paketnya ada (NATS, Traefik, PostgreSQL setelah spec 0019), dan MySQL, ClickHouse, serta Valkey tercatat `BLOCKED` sampai fitur 40 sampai 43.

Perubahan kecil yang ikut: `runtime.selector_invalid` dihapus karena bentuk selector kini dicek `Validate` (`request.invalid_params`), `ListRuntimesParams` mendapat `Validate`, replay idempotency key dilayani dari input journal sebelum catalog dibaca, `DaemonConfig.fetcher` diganti `Platform::with_fetcher`, `compat-record` hanya menyalin record BR-025, `install_cached_artifact` mendapat signature dan perilaku commit, DTO runtime diberi tipe (`HostTarget`, `DependencyInfo`, `SignatureInfo`, `CompatibilityRecordView`, enum terbuka), usulan spec 0010 mendapat kode kegagalan start, dan usulan spec 0002 mendapat `evidence/compatibility/`.

## Penyelarasan 2026-10-20 (keputusan K foundation)

Penyelarasan dengan keputusan lead foundation K1 sampai K18 dan update spec foundation di `main` (0001, 0002, 0003, 0004, 0005, 0006, 0009, 0010, 0011, 0012), dari daftar lead `docs/scope/coordination/02-instalasi-pertama.penyelarasan-k.md` bagian *0018* dan *Cross-cutting*. Setiap butir dicek ulang terhadap teks spec foundation di `main` sebelum ditulis. AC dan nomor langkah Build plan tidak berubah; status tetap `Proposed`.

| Butir | Perubahan di spec ini | Dasar |
| --- | --- | --- |
| K6 | exec check memakai `ChildRole::RuntimeCheck` yang langsung di kill saat timeout, hanya `core::runtime::validate` yang memakainya, fallback `Probe` dihapus | spec 0010 *Port*, *Aturan pembantu*, AC-15 |
| K7 | `classify_early_exit` hanya untuk `Some(Signaled(9))` child daemon yang tidak dihentikan daemon; status dari `discard` di tahap `identity`; langkah 8 memasang ke `core::supervisor` kalau langkah 7 spec 0010 sudah ada, dan memuat konstanta `FailureCode` beserta test pin nya; test native binary bertanda tangan rusak lewat `spawn_main` di langkah 16 | spec 0010 *Startup transaction*, langkah 7, Follow-up |
| K9 | `MacosCodeSigning` memakai helper spawn dan kill `pub(crate)` modul `process`, me reap hanya PID nya sendiri, tanpa token `SIGKILL` di luar `process/kill.rs` | spec 0010 *Adapter macOS*, spec 0004 *Key invariants* |
| K11 | migrasi `0005_runtime_install.sql` dan fixture `registry-v5.sql` di atas `0004_operation_failure.sql` langkah 16 fitur 7; langkah 6 menunggu langkah itu; catatan bahwa setelah K11 `FailureError` tidak menyimpan hint | keputusan K11, spec 0005 *Tambahan fitur 18* |
| K12 | handler memakai `OperationEngine::replay(&RequestOrigin)` lewat `replay_operation`, bukan pencarian `operation_by_idempotency_key`; langkah 11 membangunnya kalau fitur 7 belum | keputusan K12 |
| K18 | milestone 4 menunggu langkah 16 spec 0013 (yang menunggu milestone 2 fitur 10); `RuntimeStore` tidak menerima observasi dengan revision sama, jadi aturan observasi generik spec 0013 bukan milik fitur ini | keputusan K18 |
| `OsVersion`, `ArtifactFormat` | dibangun milestone 1 langkah 1 spec 0017; milestone 1 dan langkah 6 spec ini hanya memakainya | spec 0004, 0009 (tipe harus ada sebelum spec 0017 milestone 1 langkah 1) |
| `pin`, `verify`, dan `scan` | bagian `pin` dan `verify` dengan `scan`, pengisian `unpacked_size` dan `entry_count`, dan aturan 11 pindah dari akhir langkah 1 ke akhir langkah 3, tempat `archive::scan` dibangun | urutan langkah; spec 0003 aturan 11 masih menyebut langkah 1 (dilaporkan ke lead) |
| Teks besar dan umpan balik | "Dynamic Type" diganti cek lebar sempit 640 pt spec 0012; toast diganti umpan balik opak atau presentasi sistem, `// provisional: F67` | spec 0012 *Tipografi*, *Material* |
| `with_roots` dan `host_facts` | `MacosPlatform::new` dan `with_roots` membaca host facts sekali dan gagal `HostFactsUnavailable`; langkah 8 memperbarui pemanggil `with_roots` | spec 0004 *Port platform* |
| `available_space` | provided method di atas `volume_space` spec 0022; langkah 2 membangun `volume_space` kalau belum ada | spec 0004 *Port platform* |
| Exec check | `CHILD_PATH` di `core::provider::plan`, accessor `ValidatedPackage::version_check`, paket fake memakai subcommand `version` | spec 0009 |
| Usulan untuk spec lain | dicatat bahwa usulan untuk spec 0001 sampai 0010 sudah diterapkan dan spec pemilik yang mengikat; follow up yang sudah diterapkan dicentang | spec 0001, 0002, 0003, 0004, 0005, 0006, 0009, 0010, 0019 |

Tidak berubah karena sudah sesuai: K4 (kenaikan minor protokol mengambil nomor bebas berikutnya saat merge), dan butir K lain yang tidak menyentuh fitur ini (K1 sampai K3, K5, K8, K10, K13 sampai K17).

## Perbaikan pasca penyelarasan (2026-10-20)

Delapan temuan minor dari reviewer lintas dokumen (`docs/scope/coordination/post-alignment/post-0018.md`), dengan keputusan lead D3 dan D5 (`post-decisions.md`). Temuan yang berulang dari dua batas diterapkan sekali.

- Hint `runtime.install_conflict` mengikuti aturan hint engine spec 0007: `resume`, karena `install_runtime` resumable dan langkah `commit` repeatable; resume gagal lagi sampai folder dibereskan atau operasi di abandon.
- Paket PostgreSQL dibuat langkah 4 spec 0019, dan `runtime.versionCheck` nya di langkah 7 (D5); AC-20, test native, dan milestone 5 menyebut keduanya.
- Override catalog test bernama `CatalogOverride` (milik spec 0017, varian `Location` dan `Preloaded`, D3), bukan `CatalogConfig`.
- `ProviderSummary.software` dibangun langkah 5 spec 0017 dan hanya dipakai di sini sebagai kunci join; atribusi langkah di usulan spec 0017 dirinci (gerbang dan `support_status` langkah 9 dan 11, `SupportReason` wire langkah 10, `RuntimeGetAction` langkah 15).
- AC-1 tidak lagi bertentangan dengan input journal: artefak hasil resolusi adalah sumber kebenaran, dan selector hanya dicatat sebagai asal tanpa pernah diresolusi ulang.

## Update 2026-10-21 dari spec 0067 dan 0068

- **Komponen.** Halaman Runtimes dirakit dari `ProviderTile`, `StagedProgress`, `ActionMenuItems`, `.stacklyInspector`, `InspectorHeader`, dan `FeedbackCenter` spec 0067, tanpa `ActionFeedbackBanner` atau `StateBanner` sementara. Aturan tampil persen (bilangan bulat, dibulatkan ke bawah, paling banyak 99 sebelum `succeeded`) datang dari spec 0067; rumus persen keseluruhan tetap. Progress mini 76 pt di Catalog dicatat sebagai pengecualian karena `StagedProgress` tidak punya ukuran mini. Langkah 1 sampai 4 yang sudah terbangun tidak diubah.
- **Cross check (2026-10-21).** Cross check `Another model` (Sonnet 5.5) atas update ini dan perbaikannya diterima lead atas delegasi user (2026-10-21): `StagedProgress` baris Installing dengan `accessibilityLabel` dan `announcesStages: true`; alur Get dengan field action, `rejected` dan `failed` di `StateBanner` panel, dan hasil akhir lewat `RuntimeStore.recordStarted(operationId:)` dan `finishedInstalls` ke `AppShell` (D8); suite `RuntimesSnapshotTests`, `runtime.list` di daftar `.afterSnapshot`, dan kasus fixture runtime di langkah 10 (D14); pengecualian progress mini (D18).
- **Kolom halaman 384 pt (2026-10-21).** Temuan langkah 14 spec 0067: di jendela 900 pt dengan sidebar dan inspector terbuka, kolom halaman hanya 384 pt (424 pt dengan inspector 240 pt). User menerimanya, jadi halaman Runtimes harus bekerja di lebar itu. Kolom Source dan Digest disembunyikan di bawah 560 pt karena nilainya ada di inspector; ambang 560 pt adalah rekomendasi update ini. Teks AC-17 (cek di 640 pt) belum diubah dan diajukan ke lead.
