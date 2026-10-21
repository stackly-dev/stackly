# 0018. Install runtime managed

**Date**: 2026-10-19
**Status**: In Progress

## Summary

Spec ini menetapkan cara daemon memasang runtime managed: selector seperti `@17` atau `@stable` diselesaikan menjadi satu artefak persis dari catalog bawaan, artefak diunduh lewat HTTPS ke cache yang bisa dilanjutkan, dicocokkan dengan digest SHA-256 dan ukuran yang dipin, diekstrak oleh extractor Rust milik Stackly sendiri (bukan `/usr/bin/tar`) ke folder staging, diperiksa sebagai binary macOS (arsitektur arm64, versi macOS minimum, library yang dimuat, signature, quarantine, lalu sekali dijalankan untuk membaca versinya), baru dipindahkan ke folder runtime dengan satu rename atomik dan dicatat di registry dalam satu commit. Semua itu satu operasi `install_runtime` di operation engine spec 0007, dengan lima tahap yang sama dengan mockup (Download, Verify digest, Extract, Validate binary, Commit), sehingga cancel, resume setelah daemon restart, dan retry tidak pernah meninggalkan runtime setengah terpasang. Setiap versi yang dipasang membawa compatibility record PRD bagian 32.2 yang dibangkitkan dari evidence native di repo; versi tanpa record tetap `unverified`. Fitur ini juga membangun `stackly runtime install`, `list`, dan `inspect`, halaman Runtimes, tombol Get beserta progress di panel Catalog, dan fuzz target untuk ekstraksi archive dan pembacaan Mach-O.

## Requirements

**User stories**:

- Sebagai developer, saya ingin `stackly runtime install official/postgresql@17` atau tombol Get di Catalog memasang satu versi persis yang terverifikasi, supaya instance saya selalu jalan dari binary yang sama dengan yang diuji Stackly.
- Sebagai developer dengan koneksi yang putus putus, saya ingin unduhan yang terputus dilanjutkan dan install yang gagal bisa diulang tanpa sisa, supaya saya tidak pernah menjalankan binary setengah jadi.
- Sebagai developer tanpa internet, saya ingin pesan yang jujur bahwa saya offline dan artefaknya belum ada di cache, dan install tetap jalan kalau artefaknya sudah di cache.
- Sebagai runtime maintainer, saya ingin daemon memeriksa archive dan binary dengan aturan yang sama dengan spike spec 0003, supaya evidence yang membuat artefak `available` juga berlaku untuk hasil install di Mac pengguna.
- Sebagai release owner, saya ingin setiap versi terpasang punya compatibility record yang bisa ditelusuri ke evidence, supaya klaim supported hanya muncul untuk kombinasi yang benar benar diuji (BR-025).

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit):

- **AC-1** (REQ-003, VAL-003): `runtime.install` dan `runtime.resolve` menerima `provider` (`ProviderName`) dan `selector`. Selector `stable` memilih satu artefak `available` dengan `stable: true` untuk target host; selector `sha256:<64 hex>` memilih artefak dengan digest itu; selector lain dicocokkan lebih dulu dengan `version` persis, lalu dengan `line`. Pencocokan tidak pernah mengurutkan versi (tanpa SemVer). Selector yang cocok dengan sebuah `version` dan sebuah `line` sekaligus, atau dengan lebih dari satu artefak, gagal `runtime.selector_ambiguous` dengan daftar versinya; selector yang tidak cocok gagal `runtime.selector_unresolved`; teks yang tidak sah ditolak `InstallRuntimeParams::validate` dengan `request.invalid_params` spec 0006. Selector `sha256:` tidak pernah memasang artefak dengan digest lain: digest target lain gagal `catalog.not_installable`, walau versi yang sama punya artefak untuk host. Hasilnya `ResolvedArtifact` (versi persis, line, target, sha256, ukuran) ada di reply, dan journal operasi menyimpan artefak hasil resolusi sebagai sumber kebenaran; selector hanya dicatat sebagai asal dan tidak pernah diresolusi ulang.
- **AC-2** (REQ-003, AC-003): Runtime yang terpasang tidak pernah berubah karena catalog berubah: setelah daemon dimulai ulang dengan catalog yang line 17 nya kini menunjuk versi lain, runtime lama tetap dengan ID, versi, digest, dan isi folder yang sama (dibandingkan file per file), dan `@17` kini memasang runtime kedua dengan ID baru. Selector tidak pernah disimpan di runtime.
- **AC-3** (REQ-004, AC-004): Unduhan yang putus setelah sebagian byte dilanjutkan dari file `.part` di cache dengan `Range` dan `If-Range`; server yang menjawab 200 untuk permintaan `Range` membuat unduhan mulai lagi dari byte 0. Daemon yang dihentikan di tengah tahap Download melanjutkan operasi yang sama setelah start (operasi resumable), dan install baru dengan idempotency key lain untuk artefak yang sama melanjutkan file `.part` yang sama. Hanya file yang ukuran dan digestnya cocok yang diganti nama menjadi entri cache `<sha256>`, dan tidak ada byte yang diekstrak atau dijalankan sebelum itu. Selama operasi belum commit, registry tidak punya baris runtime atau artefak untuk artefak itu.
- **AC-4** (AC-059, VAL-010, PRD bagian 27): Artefak yang digest, ukuran, atau magic number nya tidak cocok dengan catalog tidak pernah diekstrak atau dijalankan: operasi gagal di tahap Verify digest dengan `runtime.digest_mismatch`, `runtime.size_mismatch`, atau `runtime.format_mismatch`, tanpa baris runtime atau artefak, dan file yang salah dihapus dari cache. Kalau file yang salah berasal dari unduhan yang dilanjutkan, daemon membuang `.part` dan mengunduh sekali lagi dari awal di tahap yang sama sebelum gagal. Entri cache yang sudah ada selalu dihitung ulang digestnya sebelum dipakai.
- **AC-5** (VAL-010): Kebijakan unduhan: setiap URL dan setiap redirect (paling banyak 5) harus `https`, redirect ke `http` gagal `runtime.download_untrusted`; sertifikat diperiksa terhadap trust store macOS; `Accept-Encoding: identity` dan tidak ada dekompresi otomatis; `Content-Length` yang tidak sama dengan sisa ukuran yang diharapkan, atau byte yang melebihi `size`, menghentikan unduhan (`runtime.download_too_large`); batas waktu connect 15 detik dan idle 30 detik; setiap URL dicoba paling banyak 3 kali dengan jeda 1, 4, dan 16 detik untuk error sementara, lalu URL kandidat berikutnya. Disk yang tidak cukup untuk sisa unduhan gagal `runtime.insufficient_space` sebelum unduhan dimulai.
- **AC-6** (REQ-046 cache kosong, AC-046 bagian install): Tanpa jaringan dan tanpa entri cache yang terverifikasi, operasi gagal di tahap Download dengan `runtime.download_offline` (kategori `unavailable`, `failure.retryable` true) setelah percobaan connect habis, bukan menggantung, dan GUI serta CLI menampilkan bahwa kamu offline dan artefaknya belum ada di cache. Dengan entri cache yang terverifikasi, install berhasil tanpa jaringan.
- **AC-7** (VAL-011, AC-057): Extractor menolak, dengan `runtime.archive_rejected` dan `reason` tetap, anggota archive dengan path absolut, komponen `..`, `.` atau kosong, nama yang bukan UTF-8 atau memuat NUL atau `\`, anggota yang tertulis lewat symlink, symlink dengan target absolut atau target yang keluar dari root, hard link, file device, FIFO, socket, entri sparse, entri ganda (juga yang hanya beda huruf besar kecil di APFS), dan nama yang dicadangkan. Tidak ada file yang tertulis di luar folder staging: fixture file sentinel di luar folder itu tetap identik, termasuk saat symlink folder ke luar ditanam sebelum anggota yang menulis lewatnya. Metadata AppleDouble (`__MACOSX/` dan anggota yang komponen terakhirnya diawali `._`) dilewati di kedua format, extractor tidak pernah menulis xattr, dan bit setuid, setgid, serta sticky dibuang. Symlink internal yang aman (misalnya bentuk MySQL `bin/libprotobuf.24.4.0.dylib -> ../lib/libprotobuf.24.4.0.dylib`) dibuat setelah semua file dan folder.
- **AC-8** (AC-060, NFR-004): Ekstraksi berhenti terkontrol dengan `runtime.archive_rejected` begitu jumlah byte file biasa melewati `unpackedSize` atau jumlah anggota melewati `entryCount` artefak (atau batas cadangan saat field itu belum ada), sebelum menulis byte berikutnya; puncak alokasi extractor tetap di bawah `max_entries × 2 KiB + 1 MiB` untuk archive mana pun (diukur dengan pengalokasi penghitung di test bomb), jadi memori tidak tumbuh mengikuti ukuran archive; zip dengan data sebelum header pertama, data setelah komentar EOCD, entri yang datanya bertumpuk, entri terenkripsi, atau metode kompresi selain stored dan deflate ditolak. Folder data yang tidak punya ruang untuk `unpackedSize` (atau `5 × size` saat field itu belum ada) ditambah cadangan 64 MiB gagal `runtime.insufficient_space` sebelum ekstraksi, dan ENOSPC di tengah langkah mana pun juga menjadi `runtime.insufficient_space`. Registry tetap konsisten (tanpa baris runtime atau artefak) di semua jalur ini.
- **AC-9** (follow up spec 0003): Format diambil dari `format` artefak lalu dicocokkan dengan magic number (`tar_gz` dengan gzip `1f 8b`, `zip` dengan `50 4b 03 04`, `binary` dengan Mach-O tipis atau universal), dan yang tidak cocok gagal `runtime.format_mismatch` sebelum ekstraksi. Kasus AC-16 spec 0003 (`..`, symlink, path absolut, `__MACOSX/`) dijalankan ulang terhadap pembaca zip fitur ini, ditambah nama di central directory yang berbeda dengan header lokalnya. `archiveRoot` diterapkan sama untuk `tar_gz` dan `zip`. Path `executables` artefak `zip` dan `binary` mendapat bit eksekusi; untuk `tar_gz` bit eksekusi diambil dari archive.
- **AC-10** (REQ-005, AC-005): Validasi binary: setiap path di `executables` adalah file biasa (bukan symlink) di dalam root, berformat Mach-O dengan slice arm64; artefak yang executable nya hanya punya slice x86_64 gagal `runtime.architecture_mismatch` dengan diagnosis bahwa artefak itu bukan untuk Apple Silicon dan Stackly tidak menjalankannya lewat Rosetta. `minos` `LC_BUILD_VERSION` setiap executable dan `minOs` artefak tidak boleh di atas versi macOS host (`runtime.os_too_old`). Setiap load command non sistem di setiap file Mach-O di pohon itu berakhir di file di dalam root, dan gabungan library `bundled` dan `own` yang ditemukan sama persis dengan `dependencies` artefak (`runtime.dependency_outside_runtime`, `runtime.dependency_mismatch`). Permintaan untuk target yang tidak punya artefak ditolak daemon lewat gerbang catalog spec 0017 (`catalog.not_installable` alasan `no_build_for_host` atau `no_build_for_arch` dengan `availableTargets`) sebelum operasi dibuat, dan artefak yang `minOs` nya di atas macOS host ditolak gerbang yang sama dengan alasan baru `requires_newer_os`.
- **AC-11** (spec 0003 signing policy, PRD bagian 23): `keep_or_adhoc`: setiap file Mach-O di staging diperiksa dengan `codesign --verify --strict`; kalau satu saja gagal, semua file Mach-O di staging ditandatangani ulang ad hoc dan `signature_info` mencatat `adhocSignedAtInstall: true`. `adhoc_pipeline`: semua harus lolos, selain itu gagal `runtime.signature_invalid`. File di cache tidak pernah diubah. Tidak ada langkah yang menghapus quarantine, mengubah Gatekeeper atau setelan sistem, atau memakai `sudo`.
- **AC-12** (Done when fitur 18: binary yang ditolak OS terdiagnosis): Sebelum commit, daemon menjalankan `runtime.versionCheck` manifest provider sekali dari staging lewat port `ProcessSupervision` dengan environment yang dikosongkan, dan versi yang terbaca harus sama dengan `version` artefak (`runtime.version_mismatch`). Executable yang membawa `com.apple.quarantine` gagal `runtime.quarantined` tanpa dijalankan; proses yang mati karena SIGKILL yang tidak dikirim daemon gagal `runtime.binary_rejected_by_os`; proses yang tidak selesai dalam batas waktunya dihentikan bersama process group nya lalu gagal `runtime.binary_check_timeout` dengan petunjuk bahwa macOS mungkin sedang menampilkan dialog keamanan; `EBADARCH` saat spawn gagal `runtime.architecture_mismatch`. Tidak satu pun jalur itu menggantung lebih lama dari batas waktu ditambah 5 detik.
- **AC-13** (REQ-004): Commit: pohon staging yang sudah lolos validasi dipindahkan ke `runtime_dir(software, version, target, sha256)` dengan rename yang menolak tujuan yang sudah ada, lalu satu batch registry menulis `InsertArtifact` (dengan signature, dependency, dan compatibility record) dan `InsertRuntime` berstate `installed`. Daemon yang mati di antara rename dan batch melanjutkan operasi itu setelah start dan menulis batchnya tanpa rename kedua. Folder runtime yang tertinggal dari operasi lain yang sudah berakhir tanpa commit diambil alih (dipindah ke scratch lalu dibuang), bukan menjadi konflik permanen. Dua install artefak yang sama berjalan berurutan lewat lock `artifact:<sha256>` dan menghasilkan satu runtime; install untuk artefak yang sudah `installed` menjawab `alreadyInstalled` dengan runtime itu tanpa membuat operasi.
- **AC-14** (REQ-004): Cancel sebelum tahap Commit menghapus scratch (termasuk pohon staging), menutup operasi `cancelled`, dan membiarkan file `.part` di cache; cancel di tahap Commit mendapat disposisi `continuing_past_point_of_no_return` dan operasi berakhir `succeeded`. Install yang gagal sebelum Commit dikompensasi otomatis dan tidak meninggalkan apa pun di `runtimes/`.
- **AC-15** (BR-025, PRD bagian 32.2): `cargo xtask codegen` menulis compatibility record ke `versions.json` dari evidence native di `evidence/runtime/` (scope `runtime`) dan `evidence/compatibility/` (scope `provider`), dengan field source URL (dari artefak), digest, provider revision (null untuk scope `runtime`), identitas dependency (dari artefak), versi dan build macOS, arsitektur, `checks`, hasil, `knownLimitations`, `testedAt`, dan rujukan evidence. Install menyalin record artefak itu ke registry. `support_status` memberi `supported` hanya untuk record scope `provider` yang lulus untuk provider revision, mayor macOS, dan arsitektur host yang sama; `experimental` untuk record scope `runtime` yang lulus di host itu; `unverified` kalau tidak ada record untuk host itu; `unsupported` untuk target atau macOS minimum yang tidak cocok; `blocked` untuk artefak `blocked`.
- **AC-16** (REQ-028): `stackly runtime install <provider>@<selector>` (dengan `--wait`, `--timeout`, `--output id`, `--dry-run`, `--expect-sha256`, `--idempotency-key`), `stackly runtime list`, dan `stackly runtime inspect <id>` setara dengan halaman Runtimes dan tombol Get: field yang sama, kode error dan exit code dari kategori spec 0008, progress tahap di stderr, dan `--json` satu baris envelope. `--output id` menunggu hasil akhir lalu mencetak ID runtime saja (juga untuk `alreadyInstalled`).
- **AC-17** (PAGE-09): Halaman Runtimes menggantikan `UnbuiltPage(.runtimes)` dan mengikuti mockup UI: subjudul "{n} installed", tombol Browse Catalog, bagian Installing untuk install yang berjalan (tanda provider, judul, tahap dan persen, progress bar), tabel Runtime, Source, Digest, dan Used By dengan tombol hapus yang terkunci, catatan kaki mockup, state loading, empty, failed, unsupported, dan stale, plus inspector yang memuat isi `stackly runtime inspect`. Di Catalog (spec 0017), kolom aksi matrix versi menampilkan Get, progress install, atau Installed, dan kartu provider menampilkan "{n} installed". Semuanya bisa dipakai dengan keyboard dan VoiceOver, lolos cek teks besar spec 0012 (preview dan snapshot di lebar sempit 640 pt, karena text style macOS 27 tidak ikut `dynamicTypeSize`), lolos kontras WCAG AA, dan tabel status runtime ditambahkan ke `apps/macos/design.md`.
- **AC-18** (spec 0006, spec 0007): DTO, method `runtime.install` (`recovery refused`, idempotency key wajib), `runtime.resolve` (`recovery allowed`), `runtime.list` dan `runtime.get` (`recovery refused`), event `runtime.state_changed`, dan katalog kode area `runtime` punya fixture di `protocols/fixtures/ipc/` (termasuk nilai asing untuk enum terbuka), test kontrak Swift mendecode semuanya, `cargo xtask codegen --check` bersih, dan minor protokol naik.
- **AC-19** (NFR-009, VAL-011): Fuzz target `runtime_archive` (byte sebagai `tar_gz`, `zip`, atau `binary` lewat extractor ke pohon di memori) dan `runtime_macho` (byte sebagai file Mach-O lewat parser load command) masing masing berjalan 10 menit di Mac ini tanpa crash, memakai nightly dan cargo-fuzz yang dipin spec 0006, dengan seed dari `cargo xtask fuzz seed` dan input regresi yang diputar ulang `cargo test -p stackly-core`; hasil kedua run tercatat sebagai evidence.
- **AC-20** (AC-049 bagian install, PRD bagian 27, follow up spec 0002): Test native memasang dari cache spike xtask lewat daemon ke `TestRoot` sampai `installed` (tanpa jaringan) setiap lini `available` yang paket provider nya ada (NATS, Traefik, dan PostgreSQL 17 dan 18 setelah spec 0019 langkah 4 (paket) dan langkah 7 (`runtime.versionCheck`); MySQL, ClickHouse, dan Valkey tercatat `BLOCKED` `no provider package (features 40 to 43)` sampai paketnya ada), dan, setelah persetujuan kamu, NATS 2.15.1 dari jaringan dari awal sampai `installed`, masing masing dengan evidence `native`; tidak ada file terpasang yang membawa `com.apple.quarantine`; daemon hanya menjalankan `/usr/bin/codesign` dan executable artefak (tanpa `otool`, `lipo`, `xcrun`, atau tool developer lain); dan `cargo xtask runtime spike --cleanup` juga mematikan proses sisa test native milik user ini yang argv nya memuat `/stackly-test-` (yang terlihat `pgrep -f`).
- **AC-21** (follow up spec 0003 dan 0009): Inventory dan `versions.json` mendapat field additive `stable`, `unpackedSize`, `entryCount`, `build`, `dependencies`, dan `compatibility`, dan manifest mendapat `runtime.versionCheck` opsional, lewat update spec 0003 dan 0009 yang diusulkan di *Usulan additive untuk spec lain*. `cargo xtask runtime pin` mengisi `unpacked_size` dan `entry_count` dengan scanner extractor fitur ini, `runtime check` menegakkan aturan barunya, dan pipeline install tetap berjalan dengan perilaku cadangan yang tertulis saat field opsional tidak ada.

## Decision

**Chosen option**: Option 1 di setiap keputusan di [rationale.md](rationale.md): pipeline install sebagai satu operasi resumable spec 0007 dengan lima langkah yang sama dengan mockup; HTTP client `ureq` 3 (sinkron, rustls, trust store macOS lewat `rustls-platform-verifier`, tanpa dekompresi otomatis) di belakang port platform baru `Fetcher`; cache unduhan di `<cache>/downloads/` dengan file `.part` yang bisa dilanjutkan; extractor Rust sendiri di `stackly-core` di atas crate `tar`, `flate2`, dan `zip` (hanya membaca entri, tidak pernah memanggil fungsi unpack library) yang menulis lewat port `TreeWriter` berbasis file descriptor; parser Mach-O di core lewat crate `object`; signature dan quarantine lewat port `CodeSigning` yang memakai `/usr/bin/codesign` dengan flag yang sama dengan spike; exec check dari field manifest `runtime.versionCheck`; commit lewat rename tanpa replace ke `runtime_dir` lalu satu batch registry; compatibility record yang dibangkitkan dari evidence native di repo dan disalin ke registry saat install.

Runner up per keputusan ada di rationale. Format `binary` tetap didukung (biayanya kecil dan inventory menerimanya), dan `stable` adalah penanda eksplisit per artefak, bukan aturan "line tertinggi".

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `signing-entitlements` (`openai/plugins`, `.claude/skills/signing-entitlements/`) · `swiftui-expert-skill` (`avdlee/swiftui-agent-skill`, `.claude/skills/swiftui-expert-skill/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang.

## Feature design

### Batas fitur

| Bagian | Pemilik |
| --- | --- |
| Resolusi selector, unduhan, cache, verifikasi, ekstraksi aman, validasi binary, commit, compatibility record (format, generator, penyimpanan, `support_status`), `runtime.install`, `resolve`, `list`, `get`, CLI `runtime install`, `list`, `inspect`, halaman Runtimes, tombol Get dan progress di kolom aksi Catalog, fuzz target `runtime_archive` dan `runtime_macho` | spec ini |
| Model baca catalog (`Catalog`, `CatalogEntry`), gerbang `catalog::installability`, kode `catalog.not_installable`, `SupportStatus` di domain dan label serta tone nya, halaman Catalog selain kolom aksi | spec 0017 |
| Manifest dan `versionCheck` PostgreSQL, readiness, init | spec 0019 |
| Spawn helper, identitas proses, start instance | spec 0010 (fitur ini memakai `spawn_helper`) |
| Hapus runtime dan pembersihan cache | fitur 55 |
| Runtime external (Register External) | fitur 56 |
| Pemeriksaan ulang runtime terpasang (`missing`, `drifted`) dan doctor | fitur 34 dan 55 |
| Proxy sistem untuk unduhan | belum dimiliki (Follow-up) |

Fitur ini tidak pernah pin provider revision. Runtime managed dipakai bersama oleh semua revision provider, jadi `artifacts.provider_revision_pk` tetap NULL; satu satunya pemanggil `pin_revision` di fase 02 adalah create instance (fitur 20), sesuai kesepakatan dengan spec 0017.

### Peta modul dan dependency

| Crate | Dependency langsung baru | Isi baru |
| --- | --- | --- |
| `stackly-domain` | tidak ada | `runtime::selector` (`RuntimeSelector`), value type `TeamId`, `Runtime::committed`, record registry di *Data model*; `OsVersion` dan enum `ArtifactFormat` (`tar_gz`, `zip`, `binary`) adalah titik perluasan fitur 18 di spec 0004 yang dibangun lebih dulu di milestone 1 langkah 1 spec 0017, dan fitur ini hanya memakainya |
| `stackly-platform` | tidak ada | port `Fetcher` (modul `fetch`), port `CodeSigning` (modul `codesign`), tambahan `Filesystem` (`open_read`, `open_append`, `rename_no_replace`, `available_space`, `open_tree_writer`), `PlatformInfo::host_facts` dengan `HostFacts` (modul `info`), `StopFlag` (modul `fetch`), value type `HttpsUrl`, `EntryPath`, `FileMode` |
| `stackly-platform-macos` | `ureq` 3 (`default-features = false`, fitur `rustls` dan `platform-verifier`) | `MacosFetcher`, `MacosCodeSigning` (memanggil `/usr/bin/codesign`, membaca xattr lewat `getxattr` dengan `XATTR_NOFOLLOW`), `MacosTreeWriter` (`openat`, `mkdirat`, `symlinkat`, `fchmodat` dengan `O_NOFOLLOW` dan `O_EXCL`), `renamex_np(RENAME_EXCL)`, `volume_space` lewat `statfs` (spec 0022; `available_space` adalah provided method di atasnya, spec 0004), `host_facts` lewat `sysctlbyname` `kern.osproductversion` dan `kern.osversion` yang dibaca sekali saat `MacosPlatform::new` atau `with_roots` membuat platform (keduanya gagal `HostFactsUnavailable`, jadi `with_roots` menjadi `Result<Platform, PlatformError>`, spec 0004) |
| `stackly-testkit` | tidak ada | `FakeFetcher`, `FakeCodeSigning`, `FakeTreeWriter` lewat `FakeFilesystem`, `fs_conformance` untuk method baru, `runtime_cache::cached_artifact(sha256)`, builder archive fixture (`tar_gz`, `zip` stored dan deflate, Mach-O tipis dan universal) |
| `stackly-core` | `tar`, `flate2` (backend Rust murni), `zip` (`default-features = false`, hanya `deflate`), `object` (`default-features = false`, fitur `read_core`, `macho`, `std`) | modul `runtime`: `resolve`, `download`, `archive`, `macho`, `closure`, `validate`, `commit`, `compatibility`, `install` (pekerjaan dan factory `install_runtime`), `launch` (dipakai juga spec 0010), `fixture` (di balik fitur `fixture`, isi di *API surface*), `fuzzing` (`#[doc(hidden)] pub mod`) |
| `stacklyd` | tidak ada | handler `ipc::runtime`, pemetaan DTO, registrasi factory `install_runtime`, event `runtime.state_changed`; test in process memberi fetcher lewat `Platform::with_fetcher` di `DaemonDeps`, bukan field config |
| `stackly-protocol` | tidak ada | modul `runtime` (DTO), area kode `runtime` |
| `stackly-cli` | tidak ada | `commands/runtime.rs` |
| `xtask` | tidak ada | field baru di codegen `versions.json`, `runtime pin` mengukur `unpacked_size` dan `entry_count`, aturan `runtime check` baru, `runtime compat-record`, perluasan `runtime spike --cleanup`, dua fuzz target dan seed nya, aturan `check-deps` baru |
| `StacklyClient`, `StacklyUI` | tidak ada | tipe hasil generate, `RuntimeStore`, `RuntimesPage`, `RuntimeInspector`, `InstallingRow` (view layar di atas `ProviderTile` `rowSize` dan `StagedProgress` `.compact` spec 0067), `RuntimeGetAction` |

Aturan `check-deps` baru: `ureq` hanya dependency langsung `stackly-platform-macos`; `tar`, `flate2`, `zip`, dan `object` hanya dependency langsung `stackly-core` (dan `xtask`, `fuzz/`); di kode non test `stackly-platform-macos`, literal teks yang diawali `/usr/bin/`, `/usr/sbin/`, `/bin/`, atau `/Library/Developer/` hanya boleh `/usr/bin/codesign`, dan hanya di modul `codesign`. Aturan ini melengkapi daftar `MACHINE_PATHS` yang sudah ada. Lisensi baru yang diharapkan masuk graph adalah `ISC` (ring, rustls, untrusted); kalau `cargo deny check` menemukannya, `ISC` ditambah ke `allow` di `deny.toml` dengan komentar asalnya. Lisensi lain yang muncul dicek saat build dan dihentikan kalau bukan lisensi permisif.

### Alur install

Satu operasi `install_runtime` (resumable, kebijakan `Compensate`), resource `runtime` dengan `RuntimeId` baru, `creates_resource: true`, lock tambahan `LockKey::Artifact(sha256)`, `BusyPolicy::Queue`.

Sebelum operasi dibuat (handler `runtime.install`, berurutan): validasi params (`InstallRuntimeParams::validate`); tanyakan replay ke engine lewat `OperationEngine::replay(&RequestOrigin) -> Result<Option<BeginOutcome>, BeginError>` (K12, spec 0007; read only, menjalankan cek pertama `begin` saja tanpa gerbang, precheck, atau tulisan, dengan error `KeyReused`, `NotReady`, `ShuttingDown`, dan `Registry`) lewat helper `stacklyd` `replay_operation`: kalau ada operasi dengan key itu, handler melewati catalog sama sekali dan membalas `InstallRuntimeReply` dengan `resolved` dibangun dari `artifact` di input journal operasi itu dan `outcome` `started` berisi `OperationAccepted` yang di replay (key yang sama dengan sidik jari lain tetap `operation.idempotency_key_reused`); kalau tidak ada, cari entri catalog (`Catalog::entry`; tidak ada berarti `catalog.provider_not_found` spec 0017; entri `Planned` atau `Invalid` lihat *Resolusi selector*); resolusi selector atas `ProviderEntry::artifacts()` menjadi versi; gerbang `catalog::installability(entry, version, host)` spec 0017 (versi yang tidak ada berarti `catalog.version_not_found`, yang tidak bisa terjadi setelah resolusi); untuk selector `sha256:`, cek bahwa artefak gerbang sama dengan artefak yang dipilih (*Resolusi selector* langkah 4); pakai artefak `Installable` dari gerbang; cek `expectedSha256` (berbeda berarti `runtime.selector_changed`); lalu `begin` dengan `precheck` yang membaca registry: runtime managed untuk sha256 itu dengan state `installed` menjawab `NoOp` `alreadyInstalled`, state lain menjawab `Reject` `runtime.already_registered`.

| Langkah (`StageName`) | Label UI | `repeatable` | `reversible` | Isi | Progress |
| --- | --- | --- | --- | --- | --- |
| `download` | Download | ya | ya | lihat *Download dan cache*: entri cache terverifikasi dipakai apa adanya; selain itu unduh atau lanjutkan `.part`, lalu rename ke `<sha256>` | `counted` byte |
| `verify_digest` | Verify digest | ya | ya | hitung ulang SHA-256 dan ukuran entri cache, cocokkan magic number dengan `format` | `counted` byte |
| `extract` | Extract | ya | ya | hapus `stage/` di scratch kalau ada, cek ruang disk, ekstrak ke `stage/tree/` sambil menghitung SHA-256 stream yang dibaca; digest yang berbeda di akhir membuang `stage/` dan gagal `runtime.digest_mismatch` | `counted` byte file biasa (total `unpackedSize`, atau `indeterminate`) |
| `validate_binary` | Validate binary | ya | ya | validasi statis Mach-O dan closure, kebijakan signing, mode akhir, quarantine, exec check, tulis marker | `counted` file Mach-O diperiksa, ditambah 1 untuk exec check |
| `commit` | Commit | ya | tidak (titik tak bisa dibalik) | ambil alih folder yatim kalau ada, buat folder induk, `rename_no_replace(stage/tree, runtime_dir)`, lalu kembalikan `finalize` | `indeterminate` |

- Langkah memanggil `ctx.set_stage` dengan nama langkah itu. `check_cancel` dipanggil di antara setiap chunk unduhan, setiap anggota archive, dan setiap file Mach-O; penantian panjang (backoff, exec check) memakai `ctx.stopped()` di `tokio::select!`. Pekerjaan sinkron (port, hashing, ekstraksi) berjalan di `spawn_blocking` dengan `stackly_platform::fetch::StopFlag` (newtype di atas `Arc<AtomicBool>`, juga dipakai extractor) yang diset task async saat `stopped()` selesai.
- `finalize` membangun `Runtime::committed(NewRuntime { id, software, version, line, target, install: Managed { artifact_sha256 }, depends_on: {} }, verified_at, committed_at)` dengan waktu dari `Clock`, `ArtifactRecord` lengkap (*Data model*), lalu batch `InsertArtifact` dan `InsertRuntime`. `finalize` membaca registry lebih dulu: kalau runtime untuk sha256 itu sudah ada (balapan yang lolos lock, seharusnya tidak terjadi), hasilnya `runtime.install_conflict` dan langkah commit gagal setelah titik tak bisa dibalik, sehingga operasi `needs_recovery`.
- Hasil operasi memakai `OperationOutcome` spec 0007 tanpa payload baru: `result.resources` (di wire `OperationSummary.result.resources`) berisi tepat satu `ResourceRef` ber kind `runtime`, yaitu runtime yang baru dibuat. Operasi yang antri di belakang install lain untuk artefak yang sama memeriksa registry lagi di awal langkah `download`; kalau runtime nya sudah ada, pekerjaan selesai tanpa langkah lain dan `result.resources` berisi runtime yang sudah ada itu. Client menurunkan `alreadyInstalled` sebagai `result.resources[0].id` yang berbeda dari `operation.resource.id` (ID baru yang tidak pernah dibuat); CLI dan GUI memakai aturan yang sama.
- Setelah batch `succeeded`, engine menghapus scratch (spec 0007). Entri cache tetap ada.
- Input journal (`JournalPayload`, di bawah `INPUT_MAX` 16 KiB): `{ provider, selector, host: { target, osVersion }, artifact: <entri versions.json tanpa compatibility>, compatibility: <record artefak itu yang `os` dan `arch` nya sama dengan host, paling banyak 8>, packageRevision: { id, revision, manifestSha256 }, versionCheck }`. `finalize` menulis record dari input ini, tidak pernah membaca ulang catalog, jadi artefak yang hilang dari catalog karena update aplikasi tetap membawa record nya. Test memastikan entri inventory terbesar (MySQL 9.7, sekitar 5 KiB) muat. `WorkFactory::rebuild` membangun ulang pekerjaan dari input ini saja, jadi catalog yang berubah karena update aplikasi tidak mengubah operasi yang sedang berjalan.
- Checkpoint per langkah: `download` `{ cacheFile: "<sha256>", source }`, `extract` `{ files, bytes, symlinks, skippedMetadata }`, `validate_binary` `{ adhocSigned, machOFiles }`.

### Resolusi selector

- `RuntimeSelector` (domain) diparse dari teks 1 sampai 80 byte dengan charset `[A-Za-z0-9.+:_-]`: `stable`; `sha256:<64 hex huruf kecil>`; selain itu teks itu sendiri sebagai `Version` (kalau lolos `ExactVersion`) dan `Line` (kalau lolos `RuntimeLine`). Aturan bentuk ini dicek `InstallRuntimeParams::validate` (spec 0006): panjang di luar batas `too_long`, charset atau bentuk yang tidak lolos keempatnya `invalid_format`, keduanya di path `selector`, dan `provider` yang bukan `ProviderName` `invalid_format` di path `provider`, semuanya `request.invalid_params`. `@` di CLI bukan bagian selector wire.
- Sebelum resolusi, jenis entri dicek: untuk entri `Planned` atau `Invalid`, handler mengambil reason dari jenis entri (`provider_planned` atau `package_invalid`, kode yang sama dengan hasil gerbang untuk entri itu) tanpa memanggil gerbang dan tanpa mengarang versi, lalu memetakannya ke `catalog.not_installable` dengan `details.version` berisi teks selector mentah dan `availableTargets` kosong. Hanya `ProviderEntry` yang diselesaikan.
- `runtime::resolve(entry: &ProviderEntry, selector, host) -> Result<Resolution, ResolveError>` di core:
  1. `stable`: artefak dengan `stable: true` dan target host; tidak ada berarti `selector_unresolved` dengan `reason` `no_stable_artifact`.
  2. `sha256:`: artefak dengan digest itu di target mana pun; versinya diteruskan ke langkah 4 bersama artefak terpilih.
  3. teks: kumpulkan artefak yang `version` nya sama persis, dan artefak yang `line` nya sama. Kalau keduanya tidak kosong, atau salah satu memuat lebih dari satu versi berbeda, hasilnya `selector_ambiguous` dengan `versions`. Kosong keduanya berarti `selector_unresolved` dengan `knownLines` (daftar line artefak entri itu).
  4. Versi hasil langkah 1 sampai 3 diberikan ke gerbang catalog, dan artefak `Installable` dari gerbang yang dipakai. Untuk selector `sha256:`, setelah gerbang mengembalikan `Installable { artifact }`, `artifact.sha256` dibandingkan dengan digest yang dipilih; kalau berbeda (digest itu milik target lain sementara versi yang sama punya artefak host), hasilnya `catalog.not_installable` dengan reason `no_build_for_arch` kalau OS artefak terpilih sama dengan OS host atau `no_build_for_host` kalau tidak, `target` berisi target host, dan `availableTargets` dari gerbang. Artefak host versi itu tidak pernah dipakai sebagai pengganti.
- Artefak `blocked` dan `fixture` tetap ikut pencocokan (supaya pesan gerbang catalog yang menjelaskan), dan gerbang yang menolaknya.
- `runtime.resolve` menjalankan langkah yang sama tanpa registry dan tanpa operasi, sehingga `recovery allowed`; `--dry-run` CLI dan pratinjau GUI memakainya.

### Download dan cache

Layout cache (`DirectoryLayout::downloads_dir()`, root `cache`):

| File | Isi | Ditulis |
| --- | --- | --- |
| `<sha256>` | artefak utuh yang ukuran dan digestnya cocok, mode 0600 | rename dari `<sha256>.part` setelah cek |
| `<sha256>.part` | byte yang sudah diterima | `open_append`, `sync` setiap 8 MiB dan di akhir |
| `<sha256>.part.json` | `{ "schema": "stackly.download-partial/v1", "url", "validator", "bytes", "updatedAt" }` | `write_file_atomic` setelah setiap `sync` |
| `<sha256>.json` | `{ "schema": "stackly.download-source/v1", "url", "fetchedAt" }`, asal byte untuk `artifacts.source` | `write_file_atomic` sebelum rename |

Urutan langkah `download`:

1. Registry dibaca untuk dedupe operasi antri (lihat *Alur install*).
2. `<sha256>` ada dan file biasa: langkah selesai tanpa jaringan (verifikasinya di langkah berikutnya). Ini jalur offline AC-6.
3. Cek ruang: `available_space(downloads_dir)` minimal `size − bytes .part + 64 MiB`, selain itu `runtime.insufficient_space` dengan `needed`, `available`, dan `root: cache`.
4. Untuk setiap URL di `urls` berurutan, paling banyak 3 percobaan per URL dengan jeda 1, 4, dan 16 detik yang bisa dibatalkan:
   - `.part` dan sidecar nya dibaca; panjang file dipotong ke `bytes` sidecar (byte setelah `sync` terakhir dibuang); tanpa sidecar yang cocok dengan URL itu, `.part` dimulai dari 0.
   - Prefix `.part` di hash dulu, lalu `Fetcher::fetch` dengan `resume { offset, validator }` kalau offset lebih dari 0.
   - Respons `Resumed` melanjutkan hash; respons `Full` memotong `.part` ke 0 dan memulai hash dari awal.
   - Setiap chunk: `check_cancel`, tulis, hash, progress. Byte melewati `size` berarti `TooLarge`.
5. Klasifikasi error per percobaan: `Interrupted`, `IdleTimeout`, `ConnectTimeout`, dan HTTP 408, 429 (dengan `Retry-After` paling lama 60 detik), atau 5xx diulang pada URL yang sama; `Unreachable`, `Tls`, `InsecureRedirect`, `TooManyRedirects`, `LengthMismatch`, `TooLarge`, dan 4xx lain langsung pindah ke URL berikutnya.
6. Setelah byte terakhir: ukuran dan digest dibandingkan. Cocok: `sync`, tulis `<sha256>.json`, `rename_no_replace(<sha256>.part, <sha256>)` (tujuan yang sudah ada karena operasi lain berarti `.part` dibuang dan entri yang ada dipakai), hapus sidecar. Tidak cocok dan unduhan ini dilanjutkan dari offset lebih dari 0: hapus `.part` dan sidecar, ulangi sekali dari 0 di URL yang sama. Tidak cocok lagi atau tidak dilanjutkan: hapus `.part` dan sidecar, gagal `runtime.digest_mismatch` atau `runtime.size_mismatch`.
7. Semua URL habis: kode dipilih dengan urutan ini, yang pertama cocok menang: semua percobaan `Unreachable` atau `ConnectTimeout` berarti `runtime.download_offline`; ada `Tls`, `InsecureRedirect`, atau `TooManyRedirects` berarti `runtime.download_untrusted`; ada `TooLarge` atau `LengthMismatch` berarti `runtime.download_too_large`; selain itu `runtime.download_failed` dengan error terakhir. `.part` dibiarkan untuk percobaan berikutnya.

Langkah `verify_digest` membuka `<sha256>` lewat `open_read`, menghitung SHA-256 dan ukuran sambil membaca, dan mengklasifikasi 8 byte pertama (`gzip`, `zip`, `mach-o`, `unknown`). Digest atau ukuran salah: entri cache dihapus lewat `remove_owned_tree` lalu gagal (AC-4). Magic number salah: gagal `runtime.format_mismatch` dengan `format` dan `kind`, dan entri cache dibiarkan (byte nya sesuai digest; yang salah catalog).

Port `Fetcher` (modul `fetch` di `stackly-platform`, `Platform::with_fetcher`, capability `network_fetch`):

```rust
pub trait Fetcher: Send + Sync {
    fn fetch(&self, request: &FetchRequest<'_>, sink: &mut dyn FetchSink) -> Result<FetchSummary, FetchError>;
}
pub struct FetchRequest<'a> {
    pub url: &'a HttpsUrl,
    pub resume: Option<Resume<'a>>,        // Range: bytes=<offset>-, If-Range: <validator>
    pub size: u64,                         // ukuran artefak dari catalog; aturan panjang di bawah
    pub stop: &'a StopFlag,
}
pub struct Resume<'a> { pub offset: u64, pub validator: &'a str }
pub trait FetchSink {
    fn start(&mut self, response: &FetchResponse) -> Result<(), SinkError>;
    fn write(&mut self, chunk: &[u8]) -> Result<(), SinkError>;
}
pub struct FetchResponse { pub kind: ResponseKind /* Full | Resumed { offset } */, pub content_length: Option<u64>, pub validator: Option<String>, pub host: String }
pub struct FetchSummary { pub received: u64, pub host: String }
pub enum FetchError { // #[non_exhaustive]
    Unreachable { host: String, detail: String }, ConnectTimeout { host: String }, IdleTimeout { received: u64 },
    Tls { host: String, detail: String }, InsecureRedirect { host: String }, TooManyRedirects,
    Http { status: u16, retry_after: Option<Duration> }, LengthMismatch { expected: u64, announced: u64 },
    TooLarge, RangeNotSatisfiable, Interrupted { received: u64 }, Stopped, Sink(SinkError), NotSupported { capability, os },
}
```

- Aturan panjang dan resume: respons `Full` (200) wajib `Content-Length` sama dengan `size` kalau header itu ada; respons `Resumed` (206) wajib `Content-Range: bytes <offset>-<size − 1>/<size>` dan `Content-Length` sama dengan `size − offset`; yang lain berarti `LengthMismatch`. `Range` dan `If-Range` dikirim ulang di setiap hop redirect. HTTP 416 untuk permintaan resume berarti `RangeNotSatisfiable`: core membuang `.part` dan sidecar lalu mengulang dari 0 di URL yang sama, dihitung sebagai satu percobaan.
- `HttpsUrl`: teks 1 sampai 2048 byte, diawali `https://`, host tidak kosong, tanpa userinfo, spasi, atau karakter kontrol.
- `MacosFetcher`: satu `ureq::Agent` dengan TLS rustls dan verifier platform (trust store dan setelan trust macOS, termasuk root yang dipasang MDM), redirect dimatikan di agent dan diikuti sendiri (paling banyak 5, setiap `Location` harus `https`), `Accept-Encoding: identity`, `User-Agent: Stackly/<versi> (macOS; arm64)`, tanpa cookie, tanpa kredensial, tanpa proxy (Follow-up), timeout connect 15 detik, timeout baca per chunk 30 detik, buffer 64 KiB. `Unreachable` untuk DNS gagal, `ENETUNREACH`, `EHOSTUNREACH`, `ENETDOWN`, dan `ECONNREFUSED` dari setiap alamat host. `ETag` kuat dipakai sebagai validator, selain itu `Last-Modified`; validator lemah (`W/`) tidak dipakai, sehingga unduhan itu tidak bisa dilanjutkan.
- `FakeFetcher` (testkit): peta URL ke byte dengan skrip per URL (putus setelah N byte, abaikan `Range`, status tertentu, lambat, validator berganti, error tertentu) dan pencatat permintaan. Integration test `MacosFetcher` memakai server HTTPS loopback dengan sertifikat buatan test untuk jalur penolakan TLS, dan server HTTP loopback lewat fitur crate `loopback-http` yang hanya diaktifkan dependency dev untuk perilaku `Range`, redirect, dan batas.

### Ekstraksi aman

Extractor ada di `stackly_core::runtime::archive`, membaca archive sebagai stream dari file yang sudah diverifikasi, dan menulis lewat `TreeWriter`. Ia tidak pernah memanggil `unpack`, `extract`, atau fungsi lain dari crate archive yang menulis ke disk, dan tidak pernah menyentuh xattr.

```rust
pub fn extract(format: ArtifactFormat, input: &mut dyn ReadSeekFile, spec: &ExtractSpec, sink: &mut dyn TreeWriter, stop: &StopFlag) -> Result<ExtractReport, ArchiveError>;
pub fn scan(format: ArtifactFormat, input: &mut dyn ReadSeekFile, spec: &ExtractSpec) -> Result<ExtractReport, ArchiveError>; // dipakai xtask pin, menulis ke pohon di memori yang membuang isi
pub struct ExtractSpec { pub archive_root: Option<String>, pub executables: Vec<EntryPath>, pub limits: ExtractLimits }
pub struct ExtractLimits { pub max_total_file_bytes: u64, pub max_entries: u64 }
pub struct ExtractReport { pub files: u64, pub dirs: u64, pub symlinks: u64, pub file_bytes: u64, pub skipped_metadata: u64, pub stripped_special_bits: u64, pub entries: Vec<EntryRecord> }
```

`entries` (path, jenis, mode akhir, ukuran, target symlink) disimpan di memori untuk resolusi closure; jumlahnya dibatasi `max_entries`.

**Batas** (`runtime::archive::limits`):

| Batas | Nilai |
| --- | --- |
| `max_total_file_bytes` (jumlah byte semua file biasa) | `unpackedSize` artefak; tanpa field itu `min(32 × size, 16 GiB)` |
| `max_entries` | `entryCount` artefak; tanpa field itu 50 000 (artefak terbesar hari ini sekitar 2 000 anggota) |
| ruang disk sebelum ekstraksi (root data) | `unpackedSize + 64 MiB`; tanpa field itu `5 × size + 64 MiB` (rasio terbesar yang terukur 4,13, ClickHouse) |
| memori puncak extractor | di bawah `max_entries × 2 KiB + 1 MiB` (catatan `entries` dibatasi panjang path dan target, ditambah buffer) |
| panjang path, komponen, kedalaman | 1024 byte, 255 byte, 32 tingkat |
| target symlink | 1024 byte, rantai paling panjang 8 |
| header PAX atau nama panjang GNU | 64 KiB per anggota |
| buffer salin | 64 KiB |

Batas `unpackedSize` dan `entryCount` adalah batas atas, bukan kesamaan: angka dari `runtime pin` diukur dengan `scan` yang sama, dan nilai sebenarnya yang lebih kecil diterima.

**Normalisasi nama anggota** (sama untuk ketiga format): byte nama harus UTF-8 tanpa NUL dan tanpa `\`; satu awalan `./` dibuang; path absolut (`absolute_path`), komponen `..` (`path_traversal`), komponen `.` atau kosong selain garis miring akhir untuk folder (`name_invalid`), dan batas panjang (`name_too_long`) ditolak. `archiveRoot`: setiap anggota yang tidak dilewati harus `<archiveRoot>` atau di bawahnya (`unexpected_top_level`), dan awalan itu dibuang sebelum ditulis; tanpa `archiveRoot`, anggota ditulis langsung di root staging. Nama `.stackly-runtime.json` di root staging dicadangkan (`reserved_name`). Anggota yang komponen pertamanya `__MACOSX` atau komponen terakhirnya diawali `._` dilewati dan dihitung di `skipped_metadata`.

**Jenis anggota**:

| Jenis | Perlakuan |
| --- | --- |
| folder | `create_dir`, mode 0700 selama ekstraksi |
| file biasa (tar `0`, `\0`, `7`; zip tanpa bit `S_IFLNK`) | `create_file` (0600, `O_EXCL`), isi disalin dengan batas, ukuran sebenarnya harus sama dengan ukuran header (`size_mismatch`) |
| symlink (tar `2`; zip dengan `S_IFLNK` di atribut eksternal) | dicatat, dibuat di akhir (aturan di bawah) |
| hard link, device karakter atau blok, FIFO, socket, sparse GNU, jenis lain | ditolak: `hard_link`, `special_file`, `unsupported_entry` |
| header PAX global dan ekstensi | dibaca crate `tar` untuk path dan ukuran; atribut xattr (`SCHILY.xattr.*`, `LIBARCHIVE.xattr.*`) diabaikan |

**Aturan symlink**: target relatif, tidak kosong, tanpa NUL, paling banyak 1024 byte. Target diselesaikan secara leksikal dari folder induk symlink (dalam koordinat root staging) dan tidak boleh keluar dari root (`symlink_escape`). Setiap komponen di antara path hasil resolusi harus folder yang dibuat archive (bukan symlink), dan ujungnya harus anggota archive (file, folder, atau symlink lain yang juga lolos aturan ini, paling dalam 8 tingkat, tanpa siklus): `symlink_dangling`, `symlink_chain`. Anggota yang path nya melewati symlink yang tercatat ditolak `entry_through_symlink`. Semua symlink dibuat dengan `create_symlink` setelah file dan folder terakhir, sehingga tidak ada tulisan yang bisa lewat symlink; `MacosTreeWriter` juga membuka setiap komponen dengan `O_NOFOLLOW`, jadi symlink yang tertanam di luar urutan ini menghasilkan `entry_through_symlink` dari OS. Path `executables` tidak boleh symlink (`executable_not_regular`).

**Format**:

- `tar_gz`: `flate2::read::GzDecoder` satu member, lalu `tar::Archive::entries`. Setelah blok akhir tar, sisa stream gzip harus nol dan tidak ada byte setelah member gzip (`trailing_data`).
- `zip`: `zip::ZipArchive` dari central directory. Ditolak: offset archive lebih dari 0 (`prefix_data`), entri terenkripsi (`encrypted`), metode selain stored dan deflate (`unsupported_compression`), rentang data entri yang bertumpuk (`overlapping_entries`), nama di header lokal yang tidak sama dengan nama di central directory (`name_mismatch`), dan byte setelah record end of central directory beserta komentarnya (`trailing_data`; komentar EOCD sendiri diterima). Entri dengan data descriptor (bit 3, ukuran 0 di header lokal) diterima, dengan ukuran dari central directory. Bit `S_IFLNK` dan bit mode lain di atribut eksternal hanya dibaca kalau host "version made by" adalah Unix (3); selain itu entri adalah file biasa 0644 (path `executables` tetap mendapat bit eksekusi). Ukuran sebenarnya setelah dekompresi dihitung sendiri dan harus sama dengan ukuran di central directory.
- `binary`: file cache ditulis ke `<executables[0]>` (satu komponen), tanpa `archiveRoot`.

**Mode akhir** (diterapkan di langkah `validate_binary` sesudah penandatanganan, lewat `set_mode` tanpa mengikuti symlink): folder 0755; file biasa 0555 kalau archive memberinya bit eksekusi atau path nya ada di `executables` artefak `zip` dan `binary`, selain itu 0444. Bit setuid, setgid, dan sticky tidak pernah ditulis dan dihitung di `stripped_special_bits`. Runtime terpasang jadi read only bagi proses provider, sedangkan folder tetap bisa dihapus fitur 55 lewat `remove_owned_tree`.

Port `TreeWriter` (`Filesystem::open_tree_writer(root)`, `root` harus folder kosong 0700 di scratch):

```rust
pub trait TreeWriter: Send {
    fn create_dir(&mut self, path: &EntryPath) -> Result<(), PlatformError>;
    fn create_file(&mut self, path: &EntryPath) -> Result<Box<dyn TreeFile + '_>, PlatformError>;
    fn create_symlink(&mut self, path: &EntryPath, target: &str) -> Result<(), PlatformError>;
    fn set_mode(&mut self, path: &EntryPath, mode: FileMode) -> Result<(), PlatformError>;
    fn sync(&mut self) -> Result<(), PlatformError>;
}
pub trait TreeFile: std::io::Write { fn finish(self: Box<Self>) -> Result<u64, PlatformError>; }
```

`EntryPath` adalah path relatif normal (1 sampai 32 komponen, aturan di atas). `FileMode` hanya menerima nilai sampai 0o777. `MacosTreeWriter` memegang fd root (`O_DIRECTORY | O_NOFOLLOW`), turun per komponen dengan `openat(O_DIRECTORY | O_NOFOLLOW)`, membuat dengan `mkdirat`, `openat(O_CREAT | O_EXCL | O_NOFOLLOW)`, dan `symlinkat`, dan menolak entri yang sudah ada (`AlreadyExists`, yang menjadi `duplicate_entry`; APFS yang tidak peka huruf besar kecil membuat `Foo` dan `foo` bertabrakan di sini). `TreeFile::finish` memanggil `fsync`; `sync` memanggil `fsync` pada setiap folder yang dibuat lalu satu `F_FULLFSYNC`. `FakeTreeWriter` mengikuti semantik yang sama dengan `std::fs` di dalam `TestRoot` dan tidak tahan race.

### Validasi binary

Urutan di langkah `validate_binary`, semuanya pada `stage/tree/`:

1. **Executable**: setiap path `executables` ada di `entries` sebagai file biasa (`runtime.executable_missing`, `executable_not_regular`).
2. **Mach-O**: setiap file biasa dibaca 8 byte pertamanya lewat `open_read`; file Mach-O tipis (`MH_MAGIC_64`, `MH_CIGAM_64`) dan universal (`FAT_MAGIC`, `FAT_MAGIC_64`) diparse `runtime::macho` dengan `object` dari header dan load command saja (paling banyak 4 MiB per slice, `object::ReadCache` di atas `open_read`). Per file: daftar arsitektur slice, slice arm64 (atau arm64e) yang dipakai untuk aturan berikut, `LC_BUILD_VERSION` platform macOS atau `LC_VERSION_MIN_MACOSX`, `LC_ID_DYLIB`, `LC_LOAD_DYLIB`, `LC_LOAD_WEAK_DYLIB`, `LC_REEXPORT_DYLIB`, `LC_LAZY_LOAD_DYLIB`, `LC_LOAD_UPWARD_DYLIB`, dan `LC_RPATH`.
3. **Arsitektur dan macOS minimum**: executable tanpa slice arm64 berarti `runtime.architecture_mismatch` dengan `path` dan `arches` (misalnya `x86_64`). File Mach-O lain tanpa slice arm64 dicatat di `warnings` (tidak dimuat oleh proses arm64). `minos` setiap executable dan `minOs` artefak dibandingkan dengan `HostFacts.os_version` secara numerik per komponen (`runtime.os_too_old`).
4. **Closure** (`runtime::closure`, aturan AC-6 dan AC-7 spec 0003 dipindah ke daemon): `@executable_path` adalah folder setiap executable artefak, `@loader_path` folder file pemuat, `@rpath` dicoba berurutan dari `LC_RPATH` file itu dan executable nya. Resolusi berjalan di atas `entries` dengan aturan symlink yang sama, tanpa membaca filesystem. Referensi yang berakhir di file di dalam root lolos; referensi di bawah `/usr/lib/` atau `/System/Library/` lolos tanpa cek keberadaan (shared cache dyld); selain itu `runtime.dependency_outside_runtime` dengan `path` dan `library`. `LC_RPATH` absolut di luar sistem gagal untuk artefak maintainer (penanda: `build` ada) dan menjadi `warnings` untuk artefak upstream, kecuali ada referensi yang hanya bisa diselesaikan lewat rpath itu. Weak dylib di dalam root yang tidak ada menjadi `warnings`. Gabungan nama file library yang ditemukan di dalam root (setelah symlink diselesaikan) sama persis dengan nama dependency `bundled` dan `own`; selain itu `runtime.dependency_mismatch` dengan `missing` dan `unexpected`. Tanpa field `dependencies`, perbandingan himpunan dilewati dan dicatat di `warnings`.
5. **Signing** (`CodeSigning`): `assess` untuk setiap file Mach-O. `keep_or_adhoc` dengan satu saja `strict_valid` false: `sign_adhoc` untuk semua file Mach-O di staging (tanpa `--preserve-metadata`), lalu `assess` lagi dan semua harus lolos (`runtime.signature_invalid`). `adhoc_pipeline` dengan satu saja yang gagal: `runtime.signature_invalid`. `signature_info` dirakit dari hasil executable.
6. **Mode akhir** (lihat *Ekstraksi aman*), lalu `TreeWriter::sync`.
7. **Quarantine**: `quarantine(path)` untuk setiap executable; ada berarti `runtime.quarantined`. Extractor tidak pernah menulis xattr, jadi jalur ini menjaga terhadap file yang diganti di luar Stackly dan juga dipakai spec 0010 sebelum spawn (`runtime::launch`).
8. **Exec check** (kalau `runtime.versionCheck` ada di manifest paket catalog yang sama): `ProcessSupervision::spawn_helper` dengan `program` path canonical `runtime.executable` di staging, `args` dari manifest, `env` `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, `HOME` di `scratch/check/home/`, `TMPDIR` di `scratch/check/tmp/`, `LANG=en_US.UTF-8`, `cwd` `scratch/check/` (ketiga folder dibuat dengan `create_dir_private` sebelum spawn, mode 0700), stdin `Null`, output `Capture`, role `ChildRole::RuntimeCheck` (spec 0010, K6: tanpa lock; hanya `core::runtime::validate` yang memakai role ini; tidak ada fallback ke `ChildRole::Probe`). `PATH` adalah konstanta publik `CHILD_PATH` di `core::provider::plan` (spec 0009), bukan salinan baru, dan args, pattern, serta timeout dibaca lewat `ValidatedPackage::version_check` (spec 0009). Batas waktu `timeoutSeconds` (default 15, paling banyak 60); saat lewat, helper langsung menerima `terminate_helper_group(force: true)` tanpa `SIGTERM` dan tanpa `HELPER_TERM_GRACE` (aturan pembantu spec 0010). Pencocokan: `pattern` dikompilasi dengan batas pattern settings spec 0009 (512 karakter, `size_limit` 64 KiB) tetapi tidak dibungkus `\A…\z`; exec check mencari (find) pattern di setiap baris stdout lalu setiap baris stderr, setelah `\n` dan `\r` di akhir baris dibuang, dan baris pertama yang cocok menang (pattern yang membawa `^` dan `$` sendiri, seperti pattern PostgreSQL spec 0019, tetap cocok per baris). Hasil: keluar 0 dan capture group baris yang cocok sama dengan `version` lolos; pattern tidak cocok atau versi berbeda `runtime.version_mismatch` (`expected`, `observed`); keluar bukan 0 `runtime.binary_check_failed` (`exitCode`, ekor stderr yang sudah diredaksi); `Signaled(9)` tanpa terminate dari daemon `runtime.binary_rejected_by_os` (`signal`); sinyal lain (misalnya SIGABRT dari dyld atau SIGTRAP) `runtime.binary_check_failed` dengan `signal`; melewati batas waktu `runtime.binary_check_timeout`; `SpawnFailed` dengan errno `EBADARCH` `runtime.architecture_mismatch`, `EACCES` atau `ENOEXEC` `runtime.binary_check_failed`. Tanpa `versionCheck`, langkah ini dilewati dan `warnings` mencatat `version check not declared`.
9. **Marker**: `.stackly-runtime.json` di root staging, `{ "schema": "stackly.runtime-marker/v1", "operationId", "sha256", "software", "version", "target", "validatedAt" }`, mode 0444, lalu `sync`.

`runtime::launch` (dipakai fitur ini dan spec 0010): `check_launchable(platform, executable) -> Result<(), LaunchRefusal>` memeriksa quarantine, dan `classify_early_exit(status, terminated_by_daemon) -> Option<LaunchRefusal>` memberi `BinaryRejectedByOs` hanya untuk status `Some(Signaled(9))` dari child daemon ini yang tidak dihentikan daemon sendiri (K7); status `None` (proses yang diadopsi) dan `SIGKILL` dari `discard` atau terminate daemon tidak pernah diklasifikasi. Spec 0010 memakai keduanya di tahap `spawn`, `identity` (status child tertahan yang sudah mati dari `discard`), dan exit dini (spec 0010 *Startup transaction*); pemasangannya ke `core::supervisor` dikerjakan yang di merge belakangan di antara langkah 8 spec ini dan langkah 7 spec 0010.

Port `CodeSigning` (modul `codesign`, `Platform::with_code_signing`, capability `code_signing`):

```rust
pub trait CodeSigning: Send + Sync {
    fn assess(&self, path: &Path) -> Result<CodeAssessment, PlatformError>;
    fn sign_adhoc(&self, paths: &[PathBuf]) -> Result<(), PlatformError>;
    fn quarantine(&self, path: &Path) -> Result<Option<QuarantineInfo>, PlatformError>;
}
pub struct CodeAssessment { pub signature: SignatureKind, pub strict_valid: bool, pub detail: Option<String> }
pub enum SignatureKind { DeveloperId { team: TeamId, authority: String }, Adhoc, LinkerSigned, OtherSigned { authority: String }, Unsigned } // #[non_exhaustive]
pub struct QuarantineInfo { pub agent: Option<String> } // nama agen dari nilai xattr, tanpa UUID
```

`MacosCodeSigning` menjalankan `/usr/bin/codesign --verify --strict <path>` dan `/usr/bin/codesign -dv --verbose=2 <path>` dengan aturan baca yang sama dengan `xtask/src/runtime/spike/signing.rs` (kode itu dipindah ke adapter dan xtask memakai ulang parser nya), `sign_adhoc` menjalankan `/usr/bin/codesign --force --sign - <paths>` per paling banyak 64 path, dengan environment kosong, batas waktu 120 detik per pemanggilan, dan output dibatasi 64 KiB. Spawn ini di luar `ProcessSupervision` karena tidak pernah menjalankan kode artefak (pengecualian bernama spec 0004 *Key invariants* dan spec 0010), tetapi memakai helper spawn dan kill `pub(crate)` dari modul `process` crate itu (K9, spec 0010 *Adapter macOS*), jadi hanya ada satu implementasi `posix_spawn`: program tetap `/usr/bin/codesign`, stdin `/dev/null`, environment kosong, hanya me reap PID nya sendiri (tanpa `waitpid(-1)` dan tanpa handler `SIGCHLD`), dan saat timeout dihentikan lewat fungsi `pub(crate)` di `process/kill.rs`, sehingga token `SIGKILL` tetap hanya di file itu. `/usr/bin/codesign` bagian dari macOS dasar, jadi tidak butuh Command Line Tools (AC-049). `FakeCodeSigning` bisa disetel per path.

### Commit atomik, marker, dan folder yatim

Langkah `commit` (repeatable, tidak reversible):

1. `target = layout.runtime_dir(software, version, target, sha256)`.
2. `target` ada:
   - marker nya memuat `operationId` operasi ini: rename sudah terjadi sebelum daemon mati, lompat ke langkah 4.
   - marker nya (dibaca `read_file_limited` dengan batas 4 KiB) memuat `sha256` yang sama, registry tidak punya runtime untuk sha256 itu, dan operasi di marker berstate akhir di registry atau sudah tidak ada di registry (dipangkas retensi spec 0005): folder itu yatim (sisa operasi yang di abandon atau gagal setelah rename). Ia dipindah dengan `rename_no_replace` ke `scratch/orphan/` (dibuang bersama scratch saat operasi selesai) dan langkah lanjut.
   - selain itu (marker hilang, rusak, dengan sha256 lain, milik operasi yang belum berakhir, atau runtime sudah ada): `runtime.install_conflict` dengan `path`, dan operasi `needs_recovery` dengan hint `resume` dari aturan hint spec 0007 (langkah `commit` repeatable); resume gagal lagi dengan kode yang sama sampai folder itu dibereskan (doctor fitur 34) atau operasi di abandon.
3. Folder induk `runtimes/<software>/<version>/<os>-<arch>/` dibuat satu per satu dengan `create_dir_private` (yang sudah ada diterima kalau folder biasa milik user), lalu `rename_no_replace(stage/tree, target)` dan `sync` folder induk.
4. Kembalikan `finalize`.

Tidak ada penghapusan di `runtimes/` oleh fitur ini. `rename_no_replace(from, to)` (tambahan `Filesystem`): pemanggil core menjalankan `check_owned_path` pada kedua path dan memastikan root nya sama sebelum memanggil port; port sendiri tidak memeriksa kepemilikan (aturan spec 0004). Adapter memakai `renamex_np(RENAME_EXCL)` di macOS, lalu `fsync` kedua folder induk; tujuan yang ada berarti `AlreadyExists`, volume berbeda berarti `CrossesDevice`. Scratch dan `runtimes/` sama sama di root data, jadi rename selalu di satu volume.

### Compatibility record

**Bentuk** (field `compatibility` di artefak `versions.json`, juga DTO dan isi tabel registry):

| Field | Isi | Sumber |
| --- | --- | --- |
| `scope` | `runtime` atau `provider` | jenis evidence |
| `result` | `pass` atau `fail` | status record evidence terbaru untuk kunci itu |
| `providerRevision` | `{ id, revision, manifestSha256 }` atau null untuk `runtime` | `compatibility.json` evidence scope `provider` |
| `os`, `osVersion`, `osBuild`, `arch` | `macos`, `27.0`, `26A428`, `aarch64` | `context.host` report evidence |
| `checks` | daftar kode snake_case yang diuji, urut, tanpa duplikat | `runtime`: setiap `steps[].name` `spike-result.json` yang berstatus PASS, diubah ke snake case (huruf kecil, spasi menjadi `_`, dan `launch` menjadi `launch_agent`; misalnya `static_audit`, `ready`, `stop`), ditambah setiap `closureProbes[].name` yang PASS (misalnya `dependencies`, `icu`, `pgcrypto`, `lz4`, `zstd`, `manifest`, `jetstream`); `provider`: `checks` di `compatibility.json` |
| `knownLimitations` | daftar kode | `runtime`: selalu `provider_integration_not_covered`; `provider`: dari `compatibility.json` |
| `summary` | teks `actual` record, paling banyak 256 karakter | record evidence |
| `testedAt` | waktu UTC | `finishedAt` record |
| `evidence` | `runtime/<runId>/<recordId>` atau `compatibility/<runId>/<recordId>` | path folder evidence |

Source URL (`urls[0]`), digest (`sha256`), dan identitas dependency (`dependencies`) adalah field artefak tempat record itu berada, jadi record tidak menyalinnya; DTO `CompatibilityRecordView` menggabungkannya untuk tampilan. Kunci satu record: (sha256, `scope`, `providerRevision`, mayor `osVersion`, `arch`); generator memilih record evidence terbaru per kunci menurut `finishedAt`, apa pun statusnya, lalu mengurutkan menurut `scope`, `testedAt` turun.

**Produsen**:

- Scope `runtime`: report di `evidence/runtime/*/report.json` (spec 0003) dengan subject `digest` sama dengan sha256 artefak, plus `artifacts/<recordId>/spike-result.json` untuk `checks`.
- Scope `provider`: report di `evidence/compatibility/<runId>/` yang disalin `cargo xtask runtime compat-record <report.json>` dengan aturan penolakan report yang sama dengan `runtime record` spec 0003 (dirty, fixture, `problems`, `interrupted`, commit bukan `HEAD`, folder tujuan ada). `compat-record` menyalin hanya record `PASS` dan `FAIL` yang `covers` nya memuat `BR-025`; record lain dilewati seperti record `BLOCKED` dan `NOT_RUN`, jadi run native lengkap sebuah provider (banyak test tanpa BR-025) tetap bisa disalin. Report ditolak kalau salah satu record BR-025 itu bukan `kind: native`, tidak punya subject managed dengan digest, atau tidak punya `artifacts/<recordId>/compatibility.json` berschema `stackly.compatibility-evidence/v1` (`providerRevision`, `checks`, `knownLimitations`); `no record to keep` berlaku kalau tidak ada record BR-025. Test native fitur provider (mulai spec 0019) menulis file itu lewat `artifact_dir()`. `runtime check` menambah aturan 12: setiap report di `evidence/compatibility/` lolos aturan itu.

**`support_status`** (`stackly_core::runtime::compatibility`, dipanggil spec 0017 untuk sel OS host dan fitur ini untuk runtime terpasang):

```rust
// HostFacts adalah tipe stackly_platform::info (usulan spec 0004); core hanya mengimpornya.
pub fn support_status<'a>(artifact: &'a VersionArtifact, revision: Option<&ProviderRevisionKey>, host: &HostFacts) -> SupportVerdict<'a>;
pub struct SupportVerdict<'a> { pub status: SupportStatus, pub reason: SupportReason, pub record: Option<&'a CompatibilityRecord> }
```

Urutan aturan (reason di kurung, kode `SupportReason` spec 0017 dengan tiga kode baru dari fitur ini): artefak `blocked` berarti `blocked` (`artifact_blocked`); artefak `fixture` berarti `unverified` (`fixture_only`); target bukan target host berarti `unsupported` (`no_build_for_arch`, atau `no_build_for_host` untuk OS lain); `minOs` di atas `HostFacts.os_version` berarti `unsupported` (`requires_newer_os`, baru); record `provider` dengan `result: pass`, `providerRevision` sama dengan `revision`, mayor OS dan arsitektur sama dengan host berarti `supported` (`provider_tested`, baru); record `runtime` dengan `result: pass` untuk host itu berarti `experimental` (`runtime_tested`, baru); selain itu `unverified` (`untested`). `record` adalah record yang menentukan status, kosong untuk aturan tanpa record. Spec 0017 tetap memanggil fungsi ini hanya untuk sel OS host. Untuk runtime terpasang, `revision` adalah revision paket catalog yang memuat sha256 itu, dan record diambil dari catalog kalau artefaknya masih ada di sana, selain itu dari salinan registry.

### Usulan additive untuk spec lain

Bagian ini memegang desain yang dibangun fitur ini di area spec lain; kode nya dibangun di Build plan spec ini. Sejak 2026-10-20 usulan untuk spec 0001, 0002, 0003, 0004, 0005, 0006, 0009, dan 0010 sudah diterapkan di spec pemiliknya (beserta keputusan K foundation), dan usulan spec 0017 diterapkan sebagian; kalau teks di bawah berbeda dengan spec pemilik, spec pemilik yang mengikat. Teks ini tetap sebagai catatan desain fitur ini.

**Spec 0009**: semua perubahan nya ada di *Usulan update spec 0009* di bawah, sebagai satu usulan yang berdiri sendiri.

**Spec 0003, inventory dan xtask runtime**: field `stable` (bool), `unpacked_size`, dan `entry_count` (wajib untuk `available` setelah aturan diaktifkan) per artefak; aturan `runtime check` 10 (paling banyak satu `stable` per provider, os, dan arch, hanya pada `available`), 11 (`available` wajib `unpacked_size` dan `entry_count`), dan 12 (report `evidence/compatibility/`). `runtime pin` mencetak `unpacked_size` dan `entry_count` dari `stackly_core::runtime::archive::scan` atas unduhan atau `--file`, di semua OS. `runtime verify` membandingkan keduanya dengan hasil scan. Perintah baru `runtime compat-record`. `runtime spike --cleanup` juga mengirim TERM lalu KILL ke proses milik user ini yang argv nya memuat `/stackly-test-` (proses sisa test native fitur ini), tetap tanpa menyentuh cache. Nilai `stable` diisi runtime maintainer saat update inventory; rekomendasi: line terbaru yang punya evidence native (PostgreSQL 18, MySQL 9.7) dan satu satunya line untuk provider lain.

**Spec 0004, port dan domain**:

- `Filesystem`: `open_read(path) -> Box<dyn ReadSeekFile>` (`O_NOFOLLOW`, file biasa, memberi `len`), `open_append(path) -> Box<dyn AppendFile>` (buat 0600 atau buka yang ada, `O_NOFOLLOW`; `AppendFile: Write` dengan `len`, `truncate`, `sync`), `rename_no_replace(from, to)`, `available_space(path) -> u64`, dan `open_tree_writer(root) -> Box<dyn TreeWriter>`, semuanya dengan semantik di atas, dibangun di `MacosFilesystem`, `FakeFilesystem`, dan `fs_conformance`.
- Port baru `Fetcher` dan `CodeSigning` lewat `with_fetcher` dan `with_code_signing`, capability `network_fetch` dan `code_signing`, default Unsupported.
- `PlatformInfo::host_facts() -> HostFacts` dengan `pub struct HostFacts { pub target: Target, pub os_version: OsVersion, pub os_build: Option<String> }` di modul `info` `stackly-platform` (semua field tipe domain), dan `StopFlag` (newtype `Arc<AtomicBool>` dengan `stop()` dan `is_stopped()`) di modul `fetch`; `OsVersion` (1 sampai 3 komponen angka, urutan numerik), `TeamId` (10 karakter `[A-Z0-9]`), dan enum `ArtifactFormat` (`tar_gz`, `zip`, `binary`) di domain. Core (`runtime::compatibility`, `runtime::archive`) dan gerbang spec 0017 hanya mengimpornya.
- `Runtime::committed(new: NewRuntime, verified_at, committed_at) -> Result<Runtime, TransitionError>`: menjalankan `resolved`, `downloaded`, `verified`, dan `committed` di state machine dari `resolving`, lalu mengembalikan runtime `installed` di revision 1, karena insert registry mewajibkan revision 1 dan satu commit adalah satu mutasi.
- `RuntimeSelector` di domain (modul `runtime::selector`).
- Follow up spec 0004 "Fitur 54: tambahkan `rename_no_replace` ke `Filesystem`" dicentang: method itu dibangun fitur ini, dan fitur 54 hanya memakainya.

**Spec 0005, Tambahan fitur 18**: lihat *Data model*.

**Spec 0017** (menjawab dua follow up fitur 18 di spec 0017; gerbang dan `support_status` dibangun langkah 9 dan 11 fitur ini, nilai wire `SupportReason` di langkah 10, dan `RuntimeGetAction` di langkah 15): `catalog::installability` menerima `&stackly_platform::HostFacts` (bukan `Target`; tipe dari usulan spec 0004, modul `info`) dan mendapat `NotInstallableReason::RequiresNewerOs` (kode `requires_newer_os`) untuk artefak `available` target host yang `minOs` nya di atas `HostFacts.os_version`; aturan build catalog yang menolak `minOs` di atas 27.0 (`APP_MINIMUM_MACOS`) dihapus; `SupportReason` mendapat `requires_newer_os`, `provider_tested`, dan `runtime_tested`; fungsi pengganti `host_cell_status` (`// provisional: F18`) diganti `support_status`; `VersionArtifact` `Available` membawa field additive spec 0009 di atas; dan kolom aksi matrix versi memasang `RuntimeGetAction`, yang mencocokkan baris dengan runtime dan operasi lewat `CatalogArtifact.sha256` (field yang ditambah spec 0017 sendiri). `ProviderSummary.software: Option<String>` (dibangun langkah 5 spec 0017, null untuk entri planned dan invalid) dipakai sebagai kunci join, sehingga kartu bisa dihitung tanpa detail. Kunci join: `ProviderSummary.software == RuntimeSummary.software` untuk "{n} installed", dan `InstallingRuntime.provider == ProviderSummary.provider` untuk tanda provider di baris Installing. Baris *Copy* baru: reason `requires_newer_os` "Needs macOS {minOs} or later. This Mac runs {hostOs}.", `provider_tested` "Tested with this provider on macOS {osVersion}.", dan `runtime_tested` "Runtime tested on macOS {osVersion}; provider integration not yet tested.". `catalog.not_installable` dengan reason `requires_newer_os` membawa kunci details opsional `minOs` dan `hostOs`; nilai yang berubah ada di `message` ("{provider} {version} needs macOS {minOs} or later; this Mac runs macOS {hostOs}") dan details itu, sedangkan hint nya statis, sesuai bentuk `codes::<area>::hint` yang mengembalikan `&'static str`: "Update macOS or choose an older version.". Nilai wire `SupportReason` baru beserta fixture nya masuk di build step 10 bersama satu kenaikan minor protokol fitur ini (`codegen --amend` selama versi itu belum di `main`). Test daemon fitur ini memakai `Catalog::for_tests` (fitur `test-support` core) lewat `CatalogOverride::Preloaded` (tipe `CatalogOverride` milik spec 0017, dengan varian `Location` dan `Preloaded`), termasuk paket `testing/`.

**Spec 0010** (diterapkan di spec 0010 pada 2026-10-20 sebagai K6 dan K7; teks spec 0010 yang mengikat): varian `ChildRole::RuntimeCheck` (tanpa lock, output `Capture`, stdin `Null`, timeout langsung `terminate_helper_group(force: true)`), dan tahap `spawn` start instance memanggil `runtime::launch::check_launchable` sebelum `spawn_main` serta `classify_early_exit` untuk proses yang keluar sebelum readiness, supaya binary yang ditolak OS saat start memberi diagnosis yang sama. `LaunchRefusal::Quarantined` di tahap `spawn` menjadi `start_failed { quarantined }` dengan kode wire `runtime.quarantined`; `classify_early_exit` yang memberi `BinaryRejectedByOs` menjadi `crashed { will_retry: false, reason: binary_rejected_by_os }` dan operasi gagal `runtime.binary_rejected_by_os` tanpa memakai budget restart policy; `quarantined` dan `binary_rejected_by_os` ditambah ke daftar `FailureCode` spec 0010 bersama test pin nya dalam satu commit. Fallback ke `ChildRole::Probe` ditolak (K6).

**Spec 0001**: baris *HTTP client untuk download dan ekstraksi archive* di tabel batas keputusan diisi spec ini; peta crate menambah dependency di *Peta modul dan dependency*; aturan `check-deps` di atas.

**Spec 0002**: follow up `--cleanup` dijawab di usulan spec 0003 di atas. *Folder dan file* mendapat `evidence/compatibility/<runId>/` (`report.json`, `output.log`, `artifacts/<id>/compatibility.json` berschema `stackly.compatibility-evidence/v1`); kalimat "`evidence/runtime/` adalah satu satunya evidence otomatis yang masuk git" menjadi "`evidence/runtime/` dan `evidence/compatibility/` adalah satu satunya evidence otomatis di git, ditulis `runtime record` dan `runtime compat-record`, divalidasi `runtime check`"; daftar sumber fitur 65 di *Consequences* ditambah folder itu sebagai bukti kompatibilitas, bukan bukti rilis.

**Spec 0006** (keputusan lead untuk semua area): hint error memakai satu bentuk, `codes::<area>::hint(code, details) -> Option<&'static str>`, yang dipasang `stacklyd` lewat `ErrorBody::with_hint`, juga untuk `FailureError` operasi yang hint tersimpannya kosong. Area `runtime` memakai bentuk itu untuk hint di *Kode error*.

### Usulan update spec 0009

Usulan additive untuk `architect (update)` spec 0009 setelah fitur 9 ditutup. Ia berdiri sendiri dan bisa digabung lead dengan usulan spec 0017 (`ValidatedPackage::versions()`, `VersionArtifact`, `ArtifactStatus`) menjadi satu item update. Tidak ada field yang ada diubah artinya, jadi `stackly.provider-versions/v1` dan `stackly/v1alpha1` tidak berganti nama schema.

**1. Field artefak baru di `versions.json`** (semua opsional; decode ketat yang sama, jadi field asing tetap `unknown_field`):

| Field | Tipe | Aturan baru di `versions_rule` |
| --- | --- | --- |
| `stable` | bool | hanya `true` atau tidak ada; hanya pada `available`; paling banyak satu per (`os`, `arch`) di satu file |
| `build` | int ≥ 1 | hanya pada `available` dan `blocked` (artefak maintainer) |
| `unpackedSize`, `entryCount` | `Count` | hanya pada `available`; keduanya ada atau keduanya tidak ada; `unpackedSize` 0 hanya kalau `entryCount` 0 |
| `dependencies` | daftar `{ name, kind, installName, version?, license?, sourceUrl? }` | hanya pada `available`; `kind` `bundled`, `own`, atau `system`; `name` unik; `bundled` wajib `version`, `license` (SPDX), `sourceUrl` (`https`); `installName` 1 sampai 1024 byte |
| `compatibility` | daftar record (bentuk di bawah), paling banyak 32 | hanya pada `available`; kunci record unik |

Bentuk satu record `compatibility` (JSON camelCase): `scope` (`runtime` atau `provider`), `result` (`pass` atau `fail`), `providerRevision` (`{ id, revision, manifestSha256 }`, wajib tepat saat `scope` `provider`, selain itu null), `os`, `osVersion` (angka bertitik, 1 sampai 16 byte), `osBuild` (opsional, `[0-9A-Za-z]`, 1 sampai 32 byte), `arch`, `checks` dan `knownLimitations` (daftar kode snake case, masing masing paling banyak 64), `summary` (opsional, paling banyak 256 karakter), `testedAt` (UTC RFC 3339), dan `evidence` (path relatif di bawah `evidence/`, misalnya `runtime/<runId>/<recordId>`). Kunci record: (`scope`, `providerRevision`, mayor `osVersion`, `arch`).

Contoh potongan satu artefak setelah update:

```json
{
  "id": "postgresql-18.6-r2-macos-aarch64",
  "build": 2,
  "stable": true,
  "unpackedSize": 0,
  "entryCount": 0,
  "dependencies": [
    { "name": "libssl.3.dylib", "kind": "bundled", "installName": "@rpath/libssl.3.dylib", "version": "3.5.9", "license": "Apache-2.0", "sourceUrl": "<dari inventory>" }
  ],
  "compatibility": [
    { "scope": "runtime", "result": "pass", "providerRevision": null, "os": "macos", "osVersion": "27.0", "osBuild": "26A428", "arch": "aarch64",
      "checks": ["closure", "dependencies", "icu", "launch_agent", "lz4", "manifest", "pgcrypto", "ready", "stop", "zstd"],
      "knownLimitations": ["provider_integration_not_covered"], "summary": "postgresql 18.6 ready in 493 ms, stopped in 184 ms",
      "testedAt": "2026-10-18T11:07:19.973Z", "evidence": "runtime/01a14eb1-694f-7400-b05e-71b9a321b96b/01a14eb1-6c78-7311-bd09-2b75053ea60d" }
  ]
}
```

(Angka `unpackedSize` dan `entryCount` di contoh hanya penanda, nilainya selalu dari `runtime pin`; daftar `checks` di contoh dipersingkat.)

**2. Tipe core**: field di atas masuk varian `ArtifactStatus::Available` (tipe yang diusulkan spec 0017) sebagai `stable: bool`, `build: Option<u32>`, `unpacked: Option<UnpackedSize { bytes, entries }>`, `dependencies: Option<Vec<ArtifactDependency>>` (`None` saat field tidak ada, supaya catalog bisa membedakan "tidak ada data" dari "tanpa dependency"), dan `compatibility: Vec<CompatibilityRecord>`; `build` juga di varian `Blocked`.

**3. Generator `cargo xtask codegen`**: menyalin `stable`, `build`, `unpacked_size` dan `entry_count` (menjadi `unpackedSize` dan `entryCount`), dan `[[artifact.dependency]]` (menjadi `dependencies`, `install_name` menjadi `installName`, `source_url` menjadi `sourceUrl`) dari inventory, dan menulis `compatibility` dari evidence: scope `runtime` dari `evidence/runtime/*/report.json` beserta `artifacts/<recordId>/spike-result.json` milik record yang subject `digest` nya sama dengan `sha256` artefak, dan scope `provider` dari `evidence/compatibility/*/report.json` beserta `compatibility.json`. Per kunci record dipilih record evidence terbaru menurut `finishedAt` apa pun statusnya, lalu urut `scope`, kemudian `testedAt` turun. Aturan `checks` dan `knownLimitations` ada di *Compatibility record*. Urutan field output mengikuti tabel di atas setelah field yang sudah ada, field opsional yang kosong tidak ditulis, dan `--check` tetap membandingkan byte.

**4. Field manifest `runtime.versionCheck`** (opsional, `stackly/v1alpha1`): `{ args, pattern, timeoutSeconds? }`. `args` paling banyak 8 string literal (tanpa token interpolasi, `token_not_allowed_here` kalau ada); `pattern` memakai batas pattern settings spec 0009 (512 karakter, `size_limit` 64 KiB) tetapi tidak dibungkus `\A…\z`, karena dicari per baris (aturan di *Validasi binary* langkah 8), dan punya tepat satu capture group (`invalid_value` kalau tidak); `timeoutSeconds` 1 sampai 60, default 15 (`timeout_out_of_range`). Field ini bukan bagian plan eksekusi, tetapi ikut digest paket karena berada di `provider.yaml`. Paket `fake`, `nats`, dan `traefik` mendapat field ini di build step 1 fitur ini (paket fake memakai `args: ["version"]` terhadap subcommand `version` dummy server yang sudah ada, spec 0009); PostgreSQL di spec 0019. JSON Schema editor dibangkitkan ulang.

**5. Fixture dan test**: paket fixture loader untuk setiap aturan baru (dua `stable` untuk satu target, `stable` pada `blocked`, `unpackedSize` tanpa `entryCount`, `bundled` tanpa lisensi, record `provider` tanpa `providerRevision`, record ganda per kunci, `versionCheck` dengan token atau tanpa capture group), dan golden generator untuk satu artefak dengan semua field baru.

### Data model

Tambahan fitur 18 untuk registry spec 0005, dibangun di build step 6 dengan aturan migrasi, fixture beku, `FakeRegistry`, `registry_conformance`, dan `SqliteRegistry` spec itu.

Migrasi `0005_runtime_install.sql` dengan fixture beku `registry-v5.sql` (K11: langkah 16 fitur 7 memegang `0004_operation_failure.sql` dan `registry-v4.sql`, jadi migrasi ini dibuat di atas v4; kalau urutan merge berubah, yang di merge belakangan menomori ulang dan membangkitkan ulang fixture nya):

- `artifacts` mendapat kolom `catalog_id` (`TEXT`, 1 sampai 128, `[a-z0-9.-]`), `build` (`INTEGER`, `>= 1`), `format` (`TEXT`, set tertutup `tar_gz`, `zip`, `binary`), dan `size_bytes` (`INTEGER`, `>= 1`), semuanya nullable supaya baris lama dan baris fixture tetap sah. Kolom yang sudah ada mulai diisi: `minimum_os` dari `minOs`, `source` dari `<sha256>.json` cache (NULL kalau tidak ada), `signature_info` dan `dependency_manifest` (object JSON di bawah). `provider_revision_pk` tetap NULL.
- Tabel baru `compatibility_records`:

| Kolom | Jenis | Aturan |
| --- | --- | --- |
| `artifact_sha256`* | `sha256` | FK `artifacts(sha256)` cascade |
| `position`* | `INTEGER` | `>= 0`; PK `(artifact_sha256, position)` |
| `scope`* | `TEXT` | set tertutup `runtime`, `provider` |
| `provider` | `TEXT` | `ProviderName`; terisi tepat saat `scope = 'provider'`, bersama `provider_revision` dan `manifest_sha256` |
| `provider_revision` | `TEXT` | `RevisionLabel` |
| `manifest_sha256` | `sha256` | |
| `os`* | `TEXT` | set tertutup seperti `artifacts` |
| `os_version`* | `TEXT` | 1 sampai 16, hanya `[0-9.]` |
| `os_build` | `TEXT` | 1 sampai 32, hanya `[0-9A-Za-z]` |
| `arch`* | `TEXT` | set tertutup seperti `artifacts` |
| `result`* | `TEXT` | set tertutup `pass`, `fail` |
| `checks`*, `known_limitations`* | `json` array | kode `code` |
| `summary` | `TEXT` | 1 sampai 256 karakter |
| `tested_at`* | `ts` | |
| `evidence`* | `TEXT` | 1 sampai 256, `GLOB '[a-z]*/*'`, tanpa `..` |

- `ArtifactRecord` mendapat field `catalog_id: Option<String>`, `build: Option<u32>`, `format: Option<ArtifactFormat>`, `size: Option<u64>`, `signature_info: Option<SignatureInfoJson>`, `dependency_manifest: Option<DependencyManifestJson>`, dan `compatibility: Vec<CompatibilityRecordRow>`. `SignatureInfoJson` dan `DependencyManifestJson` adalah value type teks JSON object kanonik di modul `registry` domain, dengan pola `ConfigValuesJson` (core membuat dan membaca, domain hanya memeriksa bentuk luar). `InsertArtifact` menulis baris `artifacts` dan semua baris `compatibility_records` dalam op yang sama; `Registry::artifact(sha256)` membaca keduanya. Baris artefak dan record nya immutable sampai fitur 55 menghapus artefak.

Isi JSON:

- `signature_info`: `{ "policy": "keep_or_adhoc" | "adhoc_pipeline", "adhocSignedAtInstall": bool, "machOFiles": n, "executables": [{ "path", "kind": "developer_id" | "adhoc" | "linker_signed" | "other" | "unsigned", "teamId": string | null }] }`.
- `dependency_manifest`: `{ "dependencies": [<bentuk versions.json>], "warnings": [string], "skippedMetadata": n, "strippedSpecialBits": n }`.

`runtimes` tidak berubah: baris ditulis sekali dengan state `installed`, `installed_at` waktu commit, `last_verified_at` waktu langkah `validate_binary` selesai, dan `install_mode` `managed`. Path runtime diturunkan `DirectoryLayout::runtime_dir`, tidak disimpan.

| Entitas | Kunci | Relasi |
| --- | --- | --- |
| `artifacts` | `sha256` unik | 1:N `compatibility_records`, 1:1 `runtimes` managed (unique index parsial spec 0005) |
| `compatibility_records` | (`artifact_sha256`, `position`) | N:1 `artifacts` |
| `runtimes` | `id` | N:1 `artifacts` lewat `artifact_sha256` |
| Entri cache (`<cache>/downloads/<sha256>`) | sha256 | bukan registry; cache yang bisa dibuat ulang |

### State transitions

- Runtime: tidak ada baris sebelum commit. `Runtime::committed` menjalankan `resolving → downloading → verifying → installing → installed` di memori saat `finalize`, jadi registry hanya pernah melihat `installed` dari fitur ini. State `failed`, `missing`, dan `drifted` tidak ditulis fitur ini (milik fitur 34, 55, dan 56); kegagalan install hidup di operasi.
- Operasi `install_runtime`: state machine spec 0004 dan 0007. Gagal sebelum `commit` berarti kompensasi otomatis lalu `failed`; gagal di `commit` atau `finalize` berarti `needs_recovery` dengan hint `resume` (langkah `commit` repeatable); daemon yang mati di langkah mana pun membuat operasi `queued` lagi saat start dan dilanjutkan dari langkah yang terputus.
- Entri cache: tidak ada → `.part` (unduhan berjalan atau terputus) → `<sha256>` (terverifikasi). `.part` yang digestnya salah dihapus; `<sha256>` yang digestnya salah saat dipakai dihapus.

### API surface

IPC (`stackly-protocol::runtime`; semua butuh handshake spec 0006):

| Method | Params | Reply | Recovery | Error utama |
| --- | --- | --- | --- | --- |
| `runtime.install` (memulai operasi, idempotency key wajib) | `InstallRuntimeParams { provider: String, selector: String, expectedSha256: Option<String> }` (64 hex huruf kecil, dicek `Validate`, sama dengan `contractDigest`) | `InstallRuntimeReply { resolved: ResolvedArtifact, outcome: InstallOutcome }`; `InstallOutcome` `{ type: "started", content: OperationAccepted }` atau `{ type: "alreadyInstalled", content: RuntimeSummary }` | refused | `runtime.selector_*`, `catalog.not_installable`, `catalog.provider_not_found`, `catalog.unavailable`, `runtime.already_registered`, kode `operation.*` dari `begin` |
| `runtime.resolve` | sama dengan install | `ResolvedArtifact` | allowed | `runtime.selector_*`, `catalog.not_installable`, `catalog.provider_not_found`, `catalog.unavailable` |
| `runtime.list` | `ListRuntimesParams { limit: Option<Count> (1 sampai 200, default 100; di luar itu `request.invalid_params` `out_of_range` lewat `Validate`), after: Option<Uuid> }` | `RuntimeListPage { items: Vec<RuntimeSummary>, next: Option<Uuid>, installing: Vec<InstallingRuntime> }` (`installing` hanya di halaman pertama) | refused | |
| `runtime.get` | `GetRuntimeParams { runtimeId: Uuid }` | `RuntimeDetail` | refused | `resource.not_found` (`ResourceKind::Runtime`) |

DTO (camelCase, aturan wire spec 0006). Setiap `target` bertipe `HostTarget { os: TargetOs, arch: TargetArch }` spec 0017. Enum terbuka (diakhiri `Unrecognized`, Swift `.unrecognized`, dengan fixture nilai asing): `ArtifactFormat`, `InstallModeKind`, `RuntimeState`, `CompatibilityScope` (`runtime`, `provider`), `CompatibilityResult` (`pass`, `fail`), `SigningPolicy` (`keep_or_adhoc`, `adhoc_pipeline`), dan `SignatureKindDto` (`developer_id`, `adhoc`, `linker_signed`, `other`, `unsigned`); `SupportStatus`, `SupportReason`, dan `DependencyKind` dipakai ulang dari spec 0017.

- `ResolvedArtifact { provider, software, artifactId, version, line, target, sha256, size: Count, unpackedSize: Option<Count>, format: ArtifactFormat (terbuka), minOs, stable: bool, support: SupportStatus, supportReason: SupportReason }` (kedua enum terbuka milik spec 0017).
- `RuntimeSummary { id, revision, software, displayName, version, line, target, installMode: InstallModeKind (terbuka: managed, external), state: RuntimeState (terbuka), artifactSha256: Option<String>, installedAt, lastVerifiedAt, support: SupportStatus, supportReason: SupportReason, usedBy: Vec<RuntimeUser { instanceId, name, retained: bool }>, dependentRuntimes: Vec<Uuid> }`. `displayName` dari catalog untuk software itu, cadangan nama software.
- `RuntimeDetail { summary, path: String (folder runtime absolut), artifact: Option<ArtifactInfo>, executables: Vec<String>, compatibility: Vec<CompatibilityRecordView> }`; `ArtifactInfo { catalogId, sha256, size, unpackedSize, format, minimumOs, build, source, signature: SignatureInfo, dependencies: Vec<DependencyInfo>, warnings: Vec<String> }`; `DependencyInfo { name, kind: DependencyKind, installName, version: Option<String>, license: Option<String>, sourceUrl: Option<String> }`; `SignatureInfo { policy: SigningPolicy, adhocSignedAtInstall: bool, machOFiles: Count, executables: Vec<ExecutableSignature { path, kind: SignatureKindDto, teamId: Option<String> }> }`; `CompatibilityRecordView { scope: CompatibilityScope, result: CompatibilityResult, providerRevision: Option<ProviderRevisionRef { id, revision, manifestSha256 }>, os: TargetOs, osVersion, osBuild: Option<String>, arch: TargetArch, checks: Vec<String>, knownLimitations: Vec<String>, summary: Option<String>, testedAt: Timestamp, evidence, sourceUrl: Option<String>, sha256 }` (field tabel *Compatibility record* ditambah URL sumber dan digest artefak nya).
- `InstallingRuntime { operationId, runtimeId, provider, software, displayName, version, target, sha256 }` dari input journal operasi `install_runtime` yang belum berakhir (`provider` dari field `provider` input itu); stage dan progress dibaca client dari operasi itu.
- Event `runtime.state_changed` adalah varian unit `EventKind` lewat `publish_resource(runtime, id, revision)` setelah batch install (dan untuk fitur lain yang mengubah runtime), tanpa payload. `RuntimeStore.reduce` menjawabnya `.refetch(id, revision)` (spec 0013), yang mengambil ulang lewat `runtime.get`.
- `ProtocolVersion::CURRENT` naik minor di commit yang menambah kontrak ini.

Helper test di core (`stackly_core::runtime::fixture`, fitur crate `fixture` yang hanya diaktifkan dependency dev, pola spec 0010):

```rust
pub fn install_cached_artifact(platform: &Platform, layout: &PreparedLayout, registry: &dyn Registry,
    package: &ValidatedPackage, artifact_id: &str, cache_file: &Path) -> Result<(Runtime, ArtifactRecord), InstallError>;
```

Ia menyalin `cache_file` ke `downloads_dir()/<sha256>`, lalu menjalankan langkah `download` (cache hit), `verify_digest`, `extract`, `validate_binary` (termasuk exec check kalau paket mendeklarasikan `runtime.versionCheck`), `commit`, dan batch `finalize` secara langsung, tanpa operation engine dan tanpa jaringan, di scratch sementara di bawah `operations_dir()`. Ia menulis `InsertArtifact` dan `InsertRuntime` ke `registry` sendiri, jadi record yang dikembalikan sudah di commit dan test tidak boleh meng commit nya lagi (berbeda dengan `install_fake` spec 0010). Artefak yang tidak ada di paket, atau file yang digestnya berbeda, gagal dengan error pipeline yang sama. Pemakainya spec 0019 (test native PostgreSQL) dan test core fitur ini.

CLI (`apps/cli/src/commands/runtime.rs`, pola command group spec 0008):

| Perintah | Panggilan | Output manusia | `--json` `result` |
| --- | --- | --- | --- |
| `stackly runtime install <provider>@<selector> [--expect-sha256 <hex>] [--dry-run] [--output id] [AsyncArgs]` | `runtime.install`, atau `runtime.resolve` dengan `--dry-run` | baris progress per tahap di stderr; akhir `Installed PostgreSQL 18.6 (runtime <id>)`, `Already installed: PostgreSQL 18.6 (runtime <id>)`, atau tanpa `--wait` `Started installing PostgreSQL 18.6 (operation <id>)` | `{ operation, idempotencyKey, resolved, runtimeId, alreadyInstalled }`; `--dry-run`: `ResolvedArtifact` |
| `stackly runtime list` | `runtime.list` semua halaman | tabel `ID`, `SOFTWARE`, `VERSION`, `TARGET`, `SOURCE`, `STATE`, `SUPPORT`, `DIGEST` (12 hex pertama), `USED BY` | `{ items, installing }` |
| `stackly runtime inspect <runtime-id>` | `runtime.get` | `kv` field ringkasan, lalu tabel dependency, signature, dan compatibility record | `RuntimeDetail` |

- `<provider>@<selector>` wajib memuat `@`; tanpa selector clap menolak dengan petunjuk `use <provider>@stable or <provider>@<line>`. Provider harus bentuk lengkap `<namespace>/<nama>`.
- `--output id` mengimplikasikan `--wait`, menulis tepat satu baris `<runtime-id>\n` ke stdout saat sukses atau `alreadyInstalled`, dan tidak bisa digabung dengan `--json` atau `--dry-run` (clap, exit 2).
- Exit code dari kategori spec 0008: `request.invalid_params` 2 (selector atau provider yang bentuknya salah), `selector_unresolved` 10, `catalog.not_installable` 5, `download_offline` 6, kegagalan operasi dari kategorinya.

### Kode error

Area `runtime` di `stackly-protocol::codes`, owner `spec 0018`. Kode operasi dikirim sebagai `failure.error` (spec 0007); kolom retryable di bawah adalah `failure.retryable` operasi, sedangkan `ErrorBody.retryable` mengikuti katalog (false untuk semua).

| Kode | Kategori | Saat | Kunci `details` | `failure.retryable` |
| --- | --- | --- | --- | --- |
| `runtime.selector_unresolved` | `not_found` | tidak cocok | `provider`, `selector`, `reason`, `knownLines` | |
| `runtime.selector_ambiguous` | `invalid_input` | cocok ganda | `selector`, `versions` | |
| `runtime.selector_changed` | `conflict` | `expectedSha256` berbeda | `expectedSha256`, `resolvedSha256` | |
| `runtime.already_registered` | `conflict` | runtime untuk artefak itu ada dengan state selain `installed` | `runtimeId`, `state` | |
| `runtime.download_offline` | `unavailable` | tanpa jaringan dan tanpa cache | `artifact`, `hosts` | true |
| `runtime.download_failed` | `failed` | HTTP atau koneksi | `artifact`, `host`, `status`, `reason` | true |
| `runtime.download_untrusted` | `failed` | TLS atau redirect `http` | `artifact`, `host`, `reason` | false |
| `runtime.download_too_large` | `failed` | panjang tidak cocok atau melebihi | `artifact`, `expected`, `received` | false |
| `runtime.digest_mismatch` | `failed` | SHA-256 | `artifact`, `expected`, `actual` | true |
| `runtime.size_mismatch` | `failed` | ukuran | `artifact`, `expected`, `actual` | true |
| `runtime.format_mismatch` | `failed` | magic number | `artifact`, `format`, `kind` | false |
| `runtime.insufficient_space` | `failed` | preflight ruang, atau ENOSPC di langkah mana pun | `root`, `needed`, `available` | true |
| `runtime.io_failed` | `failed` | error IO lain dari port (`PlatformError::Io` dan sejenisnya) | `op`, `root` | true |
| `runtime.archive_rejected` | `failed` | aturan ekstraksi | `artifact`, `reason`, `entry` (paling banyak 256 karakter) | false |
| `runtime.architecture_mismatch` | `unsupported` | slice arm64 tidak ada, atau `EBADARCH` | `path`, `arches` | false |
| `runtime.os_too_old` | `unsupported` | `minos` di atas host | `path`, `required`, `host` | false |
| `runtime.executable_missing` | `failed` | path `executables` | `path`, `reason` | false |
| `runtime.dependency_outside_runtime` | `failed` | closure | `path`, `library` | false |
| `runtime.dependency_mismatch` | `failed` | himpunan library | `missing`, `unexpected` | false |
| `runtime.signature_invalid` | `failed` | signing policy | `path`, `policy`, `detail` | false |
| `runtime.quarantined` | `failed` | xattr quarantine | `path` | false |
| `runtime.binary_rejected_by_os` | `failed` | SIGKILL saat dijalankan | `path`, `signal` | false |
| `runtime.binary_check_timeout` | `timeout` | exec check | `path`, `seconds` | true |
| `runtime.binary_check_failed` | `failed` | keluar bukan 0, mati karena sinyal selain SIGKILL, atau spawn gagal | `path`, `exitCode`, `signal`, `errno` | false |
| `runtime.version_mismatch` | `failed` | versi terbaca | `expected`, `observed` | false |
| `runtime.install_conflict` | `conflict` | folder tujuan bukan milik operasi ini | `path` | false |

Hint (paling banyak 200 karakter) datang dari `codes::runtime::hint(code, details)` (bentuk spec 0006 usulan lead di *Usulan additive untuk spec lain*), dipasang `stacklyd` lewat `ErrorBody::with_hint`, juga untuk `FailureError` operasi yang hint tersimpannya kosong (setelah langkah 16 fitur 7, K11, `FailureError` tidak lagi menyimpan hint, jadi hint operasi selalu dari fungsi ini); Setiap hint statis (tanpa placeholder), karena fungsi itu mengembalikan `&'static str`; nilai yang berubah (versi, path, host, ukuran) ada di `message` dan `details`. Teksnya misalnya `download_offline`: "Connect to the internet and try again. Stackly keeps any partial download."; `quarantined` dan `binary_rejected_by_os`: "Stackly never removes quarantine. Remove this runtime and install it again from Stackly."; `binary_check_timeout`: "macOS may be showing a security prompt. Check for a dialog, then try again."; `architecture_mismatch`: "This artifact is not built for Apple Silicon, and Stackly does not run it under Rosetta.".

### GUI

**Halaman Runtimes** (`StacklyUI/Runtimes/`, menggantikan `UnbuiltPage(.runtimes)` `// provisional: F18`):

- `RuntimeStore` adalah `ResourceStore<RuntimeSummary>` spec 0013 (`kinds: ["runtime."]`, `fetchPage` `runtime.list`, `fetchOne` `runtime.get`), ditambah daftar `installing` dari halaman pertama yang diperbarui lewat `OperationCenter.updates(for:)`; operasi `install_runtime` baru yang belum dikenal memicu `refresh()`.
- Toolbar `.pageChrome`: judul Runtimes, subjudul `"{n} installed"` (jumlah item store), tombol Browse Catalog yang memilih `SidebarSection.catalog`. Register External milik fitur 56 dan belum dirender.
- Bagian Installing (hanya kalau ada): per operasi tanda provider `ProviderTile(TileSpec(mark:tile:name:), size: ProviderTile.rowSize)` spec 0067 dari `ProviderSummary` spec 0017 yang `provider` nya sama dengan `InstallingRuntime.provider` (`name` dari `displayName`; tanpa `ProviderSummary` itu, `TileSpec(mark: nil, tile: nil, name: displayName)` memberi mark default dari nama di tile abu abu), judul `Install <displayName> <version>`, lalu `StagedProgress(value, layout: .compact, accessibilityLabel: "Install <displayName> <version>", announcesStages: true)` yang menampilkan teks `<label tahap> · <persen>%` dan bar nya. Nilainya dari `StagedProgressValue.from(operation:stages:)` dengan lima `StageDefinition` dari tabel *Alur install* (`download` "Download", `verify_digest` "Verify digest", `extract` "Extract", `validate_binary` "Validate binary", `commit` "Commit"). Persen keseluruhan tetap `(index tahap + pecahan tahap) / 5`, dengan pecahan dari progress `counted` tahap itu dan 0 untuk `indeterminate`; aturan tampil persen dari spec 0067: bilangan bulat, dibulatkan ke bawah, paling banyak 99 sebelum `succeeded`. Menu konteks Cancel Install memakai `ActionMenuItems` dengan `StacklyAction` yang memanggil `operation.cancel`, dengan `disabledReason` "Can’t be cancelled during Commit" saat tahap `commit` (diperbarui 2026-10-21).
- Tabel `Table` native dengan kolom Runtime (tanda, `<displayName> <version>`, baris kedua `arm64 · verified` untuk runtime `installed` managed atau label state), Source (Managed), Digest (`sha256:<8 hex>…<4 hex>` monospace, menu konteks Copy Digest), Used By (nama instance dipisah koma, retained diberi `(retained)`, atau Unused), dan tombol hapus yang terkunci: nonaktif lewat `.disabled(reason:)` spec 0012 dengan "Used by {n} instance(s)" atau, selama fitur 55 belum ada, "Removing runtimes arrives in a later build". Catatan kaki mockup: "Runtimes in use can’t be removed, even by stopped instances. External runtimes are read-only and never upgraded by Stackly."
- Inspector (`.stacklyInspector` dengan `InspectorToggle` di toolbar dan `InspectorHeader` di atas isi, spec 0067; ditampilkan dengan seleksi baris, tombol toolbar Show Inspector): isi `RuntimeDetail` dengan urutan ID (copy), Software, Version, Line, Target, Source, State, Support (label dan tone spec 0017) beserta record penentunya ("Tested on macOS 27.0 (26A428), arm64, 18 Oct 2026: launch_agent, readiness, stop"), Digest lengkap, Download size, Unpacked size, Path, Signature, Installed, Verified, Used By, Executables, Dependencies (nama, jenis, versi, lisensi), dan Warnings. Inspector adalah selisih dari mockup yang disengaja demi paritas `stackly runtime inspect`, dicatat untuk pembaruan mockup.
- State: loading (`StatePlaceholder` progress), empty ("No Runtimes", "Install a runtime from the catalog to create instances.", tombol Browse Catalog), failed (pesan daemon dan kode, Retry), unsupported (daemon tanpa `runtime.list`), stale (banner shell spec 0013, data lama tetap tampil), partial (halaman berikutnya gagal: baris yang ada tetap tampil dengan `StateBanner` caution).

**Kartu provider Catalog**: status "{n} installed" (tone `positive`, spec 0017 *Tone dan label*) dihitung di client dari `RuntimeStore`: jumlah runtime `installed` dengan `RuntimeSummary.software == ProviderSummary.software` (field tambahan di usulan **Spec 0017**), dan tidak tampil kalau 0 atau kalau `software` kartu null.

**Kolom aksi Catalog** (`RuntimeGetAction`, ditaruh spec 0017 di kolom aksi matrix versi untuk baris target host):

- `Installed` (teks tone `positive`) kalau `RuntimeStore` memuat runtime `installed` yang `artifactSha256` nya sama dengan `CatalogArtifact.sha256` baris itu (field yang ditambah spec 0017).
- Progress mini 76 pt dengan persen kalau ada `InstallingRuntime` dengan `sha256` sama dengan `CatalogArtifact.sha256` baris itu. Ini view layar, pengecualian yang dicatat dari aturan satu pembangun spec 0067 karena `StagedProgress` tidak punya ukuran mini; persen nya tetap dari `StagedProgressValue.from(operation:stages:)` dengan lima tahap install (2026-10-21).
- Tombol Get selain itu: `StacklyAction(id: "runtimeGet", title: "Get", mutates: true)` yang memanggil `MutationPath.perform(.runtimeInstall(provider, selector: version, expectedSha256: CatalogArtifact.sha256))`; nonaktif dengan alasan dari `MutationAvailability` saat tidak tersambung, atau dari gerbang catalog. Cancel Install di menu konteks baris Installing adalah `StacklyAction(id: "cancelInstall", title: "Cancel Install", mutates: true)` tanpa konfirmasi (aturan spec 0012 untuk cancel operasi). Umpan balik setelah aksi lewat `FeedbackCenter` spec 0067, tanpa banner sementara dan tanpa penanda `// provisional: F67` (diperbarui 2026-10-21), dengan `{name}` dan `{version}` dari `InstallingRuntime.displayName` dan `version` (sebelum baris itu ada, dari `ProviderSummary.displayName` dan versi baris matrix): sukses memulai memakai `FeedbackCenter.show(.started("Installing {name} {version}. Staged, verified, then committed."))`; `notSent` dan `outcomeUnknown` lewat `Feedback.outcome`; `rejected` dan operasi `failed` tetap menampilkan `StateBanner` critical di panel detail dengan pesan, hint, dan kode, serta tautan ke Operations, bukan kartu. Hasil akhir tetap tampil walau pengguna sudah meninggalkan Catalog: `RuntimeStore` (hidup di `DaemonModel`) mencatat operation ID yang diterima dari `RuntimeGetAction` lewat `recordStarted(operationId:)`, memancarkan hasil akhirnya lewat `finishedInstalls` dari `OperationCenter.updates(for:)`, dan `AppShell` (pemegang `FeedbackCenter` di environment) menampilkan `.success("{name} {version} installed and verified.")` untuk `succeeded` dan `.neutral("Install of {name} {version} was cancelled. Nothing was kept.")` untuk `cancelled`, dengan pengumuman VoiceOver dari `FeedbackCenter`.

**Status runtime** (ditambahkan ke `apps/macos/design.md`, tone menurut kolom Arti spec 0012):

| State | Label | Tone |
| --- | --- | --- |
| `installed` | Installed | `positive` |
| `resolving`, `downloading`, `verifying`, `installing` | Resolving…, Downloading…, Verifying…, Installing… | `informative` |
| `failed` | Failed | `critical` |
| `missing` | Missing | `critical` |
| `drifted` | Drifted | `caution` |
| `absent` | Removed | `neutral` |
| `unrecognized` | Unrecognized State | `caution` |

Tahap install memakai label kolom kedua tabel *Alur install*. Aksesibilitas: setiap baris tabel satu elemen VoiceOver "<name> <version>, managed, installed, verified, used by <names>"; baris Installing membacakan persen sebagai nilai elemen `StagedProgress` (teks progress spec 0067) dan mengumumkan perubahan tahap paling sering sekali per tahap lewat `announcesStages: true`; semua aksi punya padanan keyboard (seleksi tabel, Return membuka inspector, menu konteks lewat keyboard); teks memakai text style spec 0012, dan cek teks besar adalah cek lebar sempit 640 pt spec 0012 (text style macOS 27 tidak ikut `dynamicTypeSize`). `RuntimeStore` tidak menerima event observasi dengan revision yang sama (`runtime.state_changed` selalu membawa revision baru), jadi aturan observasi generik spec 0013 tidak dibangun di sini (K18).

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| resolve | entri provider | `Catalog::entry(provider)` spec 0017, dari `CatalogState::Loaded` |
| resolve | target host, versi macOS | `PlatformInfo::host_facts()` |
| resolve | artefak terpilih | aturan *Resolusi selector* atas `ProviderEntry::artifacts()` (`VersionArtifact` dari `ValidatedPackage::versions()` spec 0009 dan 0017) |
| resolve | `support` | `support_status(artifact, revision paket, host)` |
| install | `RuntimeId` baru | `RuntimeId::generate()` di handler |
| install | URL, ukuran, digest, format, `archiveRoot`, `executables`, `dependencies`, `unpackedSize`, `entryCount`, `signingPolicy`, `minOs`, `build` | artefak `versions.json` (dibekukan di input journal) |
| install | args dan pattern exec check | `runtime.versionCheck` manifest paket catalog (dibekukan di input journal) |
| install | path cache, scratch, runtime | `DirectoryLayout::downloads_dir`, `operations_dir()/<id>`, `runtime_dir` |
| install | `artifacts.source` | `<sha256>.json` di cache, ditulis saat unduhan selesai |
| install | `signature_info` | hasil `CodeSigning::assess` dan apakah `sign_adhoc` dipanggil |
| install | `dependency_manifest` | `dependencies` artefak ditambah `warnings` dan hitungan dari validasi |
| install | compatibility record | record host artefak itu yang dibekukan di input journal saat `begin` (dari catalog), ditulis apa adanya di `finalize` |
| install | `installed_at`, `last_verified_at` | `Clock::now()` saat `finalize` dan saat `validate_binary` selesai |
| install | persen di UI | stage dan progress `OperationSummary` spec 0007, rumus di *GUI* |
| list | `usedBy` | `instances(include_retained: true)` dikelompokkan per `runtime_id`, satu baca |
| list | `dependentRuntimes` | `depends_on` runtime lain dari `runtimes()` |
| list | `installing` | operasi aktif `install_runtime` dari engine beserta input journal nya |
| list, get | `displayName` | catalog spec 0017 untuk software itu; cadangan nama software |
| GUI | "{n} installed" di kartu | join `RuntimeSummary.software == ProviderSummary.software` di client |
| GUI | tanda provider di baris Installing | `ProviderSummary` yang `provider` nya sama dengan `InstallingRuntime.provider` |
| GUI | Installed dan progress di kolom aksi | join `artifactSha256` dan `InstallingRuntime.sha256` dengan `CatalogArtifact.sha256` spec 0017 |
| get | `path` | `runtime_dir(...)` dalam bentuk resolved |
| get | `compatibility` | catalog kalau sha256 masih ada di sana, selain itu `compatibility_records` registry |
| record (xtask) | host, waktu, hasil, ringkasan | `context.host`, `finishedAt`, `status`, `actual` record evidence |
| record (xtask) | `checks` | `steps` dan `closureProbes` `spike-result.json`, atau `compatibility.json` |
| pin (xtask) | `unpacked_size`, `entry_count` | `archive::scan` atas byte unduhan |

### Key invariants

- Tidak ada byte artefak yang diekstrak, ditandatangani, atau dijalankan sebelum ukuran, SHA-256, dan magic number file yang dibaca cocok dengan catalog; entri cache selalu dihitung ulang sebelum dipakai, dan langkah `extract` menghitung ulang SHA-256 stream yang ia baca sendiri, sehingga file yang diganti di antara langkah atau saat daemon restart tidak pernah sampai ke commit.
- Registry tidak pernah punya baris runtime atau artefak untuk install yang belum commit, dan setiap baris runtime managed `installed` dari fitur ini menunjuk folder yang sudah lolos validasi lengkap.
- Satu runtime managed per sha256 (unique index spec 0005), satu install berjalan per sha256 (lock `artifact:<sha256>`).
- Extractor tidak pernah menulis di luar folder staging, tidak pernah menulis lewat symlink, tidak pernah menulis xattr, dan tidak pernah memberi bit setuid, setgid, atau sticky.
- Fitur ini tidak pernah menghapus apa pun di `runtimes/`; folder yatim dipindah ke scratch, dan scratch dihapus lewat `remove_owned_tree`. Entri cache yang salah dihapus lewat `remove_owned_tree` pada root `cache`.
- Tidak ada langkah yang menghapus quarantine, mengubah Gatekeeper atau setelan sistem, memakai `sudo`, atau menjalankan tool developer; satu satunya tool sistem yang dijalankan adalah `/usr/bin/codesign`.
- Selector tidak pernah disimpan di runtime atau di input journal sebagai sumber kebenaran; input journal menyimpan artefak hasil resolusi.
- Unduhan hanya lewat HTTPS dengan verifikasi sertifikat; tidak ada jalan pintas ke HTTP di build produksi (fitur `loopback-http` hanya untuk dependency dev).
- Runtime terpasang tidak berubah karena catalog berubah; hanya fitur 55 (hapus) dan 34 serta 55 (observasi `missing` dan `drifted`) yang mengubah baris runtime managed.
- File runtime terpasang read only (0444 atau 0555) dan folder 0755.

### Security model

- **Akar kepercayaan**: digest di catalog bawaan aplikasi (spec 0017), yang berasal dari inventory yang direview (spec 0003) dan ikut ditandatangani bersama aplikasi. HTTPS menjaga privasi dan ketersediaan, bukan integritas; integritasnya adalah SHA-256 yang dipin. Tidak ada field catalog yang mewajibkan signature unduhan terpisah di v1 (GPG MySQL dicek saat `pin`), sehingga syarat VAL-010 "signature bila diwajibkan metadata" terpenuhi dengan menolak setiap catalog yang membawa field tak dikenal (decode ketat spec 0009).
- **Pertahanan berlapis**: aturan ekstraksi, batas, dan validasi binary tetap berlaku walau digest cocok, supaya catalog yang salah review atau upstream yang bermasalah tidak bisa menulis di luar folder runtime atau menjalankan binary yang ditolak OS.
- **Hak proses**: daemon berjalan sebagai user; exec check menjalankan kode artefak dengan hak user dan environment yang dikosongkan, satu kali per install, sama dengan residual risk PRD bagian 23. Exec check tidak pernah diberi secret atau port.
- **Residual**: proses lain milik user yang sama bisa mengubah cache, scratch, atau folder runtime (PRD bagian 23 tidak melindungi dari itu); digest dihitung ulang setiap kali cache dipakai, dan drift folder terpasang ditangkap fitur 34 dan 55.
- **Privasi**: log dan detail error memuat host URL dan ID artefak, tidak pernah path home penuh di `message` wire (path runtime ada di `RuntimeDetail.path` untuk pemiliknya sendiri). `User-Agent` tidak memuat nama user, host, atau ID mesin. Tidak ada telemetry.
- **Otorisasi**: semua method mengikuti handshake dan peer check spec 0006 (uid yang sama).

### Configuration required

- Tidak ada variabel environment atau credential baru untuk daemon, CLI, atau app. Root mengikuti `STACKLY_ROOT` spec 0004.
- Test saja: `STACKLY_NATIVE_NETWORK=1` mengizinkan test native yang mengunduh dari jaringan (`nats_installs_from_the_network`); tanpa variabel itu test tersebut `BLOCKED` dengan alasan `network run not approved`. Diset hanya setelah persetujuan kamu untuk run itu.
- Jaringan ke host URL artefak (GitHub Releases, `cdn.mysql.com`) untuk install pertama.
- Test native: sesi login GUI tidak dibutuhkan (daemon in process), tetapi jaringan dibutuhkan untuk test unduhan NATS, dan cache spike xtask (`~/Library/Caches/dev.stackly.xtask/runtime/<sha256>`) untuk test per lini; artefak yang tidak ada di cache membuat test lini itu `BLOCKED`.

### Menjalankan test di Mac ini

- Fixture: `cargo xtask evidence run --kind fixture -- cargo test --workspace --locked --no-fail-fast` mencakup unit dan integration test fitur ini.
- Native dari cache (tanpa jaringan, kedelapan artefak inventory sudah ada di cache spike Mac ini): `cargo xtask runtime spike --cleanup`, lalu `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test native_runtime_install -- --ignored`, lalu `--cleanup` lagi (urutan spec 0002).
- Native dengan jaringan, setelah persetujuan kamu: perintah yang sama dengan `STACKLY_NATIVE_NETWORK=1`, yang menambah `nats_installs_from_the_network`. Artefak yang hilang dari cache tidak pernah diunduh test per lini; test lini itu `BLOCKED` dan cache diisi ulang lewat `cargo xtask runtime spike --id <id>`, yang juga butuh persetujuan kamu.
- Fuzz: `cargo xtask fuzz run runtime_archive --time 600` dan `cargo xtask fuzz run runtime_macho --time 600`, hasilnya record manual `cargo xtask evidence new --covers NFR-009 VAL-011 --kind native ...`.

### Critical test scenarios

- Happy path di core: `install_runtime` dengan `FakePlatform`, `FakeFetcher` yang menyajikan tar.gz fixture berisi salinan `stackly-fake-server` (exec check nyata lewat `FakeProcessSupervision` yang menjalankan skrip versi), `FakeCodeSigning` lolos, lalu runtime `installed`, artefak dengan signature dan dependency, record compatibility, folder di `runtime_dir`, scratch terhapus, entri cache tetap. Memverifikasi **AC-1**, **AC-13**, **AC-15**.
- Resolusi: tabel selector (`stable`, line, versi, `sha256:`, ambigu versi lawan line, dua artefak satu line di paket fixture, tidak ada, teks tidak sah lewat `InstallRuntimeParams::validate` dengan `request.invalid_params`, `blocked`, target lain lewat gerbang catalog dengan `availableTargets`, `sha256:` artefak linux yang versinya juga punya artefak `macos-aarch64` (gagal `catalog.not_installable` `no_build_for_host`, artefak host tidak pernah dipasang), dan entri planned serta invalid dengan `details.version` berisi selector mentah). Replay: request dengan key yang sama setelah daemon restart dengan catalog lain membalas `resolved` dari input journal tanpa membaca catalog. Memverifikasi **AC-1**, **AC-10**.
- Catalog berubah: daemon in process dengan catalog A (line 17 = 17.11), install `@17`, restart dengan catalog B (line 17 = 17.12), runtime lama identik per file, `@17` memasang runtime kedua. Memverifikasi **AC-2**.
- Putus dan lanjut: `FakeFetcher` putus di 40%, operasi diparkir lewat shutdown daemon, start lagi, unduhan lanjut dengan `Range` dari offset terakhir yang di `sync` dan selesai `installed`; varian server mengabaikan `Range` (mulai dari 0), validator berganti (mulai dari 0), dan install baru dengan key lain memakai `.part` yang sama. Memverifikasi **AC-3**.
- Digest salah: byte fixture diubah satu byte; varian unduhan baru (gagal tanpa ekstraksi, `.part` terhapus), varian dilanjutkan (satu unduhan ulang lalu sukses kalau byte kedua benar), varian entri cache yang dirusak (dihapus lalu diunduh ulang). Tidak ada panggilan `TreeWriter` atau spawn di jalur gagal. Memverifikasi **AC-4**.
- Kebijakan unduhan: integration test `MacosFetcher` dengan server loopback: redirect ke `http` ditolak, enam redirect ditolak, sertifikat buatan sendiri ditolak `Tls`, `Content-Encoding: gzip` tidak didekompresi, `Content-Length` salah, byte lebih dari `size`, server diam 31 detik (`IdleTimeout`), 200 untuk permintaan `Range` dengan `Content-Length` sama dengan `size` (mulai dari 0), 206 dengan `Content-Range` offset salah (`LengthMismatch`), 416 (mulai dari 0), `Range` yang dikirim ulang setelah redirect, urutan kode akhir saat semua URL gagal dengan campuran error, dan jeda retry. Memverifikasi **AC-5**.
- Offline: `FakeFetcher` yang menjawab `Unreachable` untuk semua host tanpa cache memberi `download_offline` dalam waktu percobaan; dengan entri cache, install sukses tanpa satu pun panggilan fetch. Memverifikasi **AC-6**.
- Archive jahat (tar.gz dan zip, dengan `MacosTreeWriter` dan `FakeTreeWriter`): setiap `reason` di *Ekstraksi aman*, folder sentinel di luar staging dibandingkan sebelum dan sesudah, symlink folder yang ditanam lebih dulu lalu anggota lewat symlink itu, `Foo` dan `foo`, AppleDouble di kedua format tanpa xattr baru selain `com.apple.provenance`, setuid dibuang, fixture bentuk symlink MySQL (`..` dan rantai) diterima. Memverifikasi **AC-7**, **AC-9**.
- Bomb: archive deflate dengan rasio besar melewati `unpackedSize` berhenti sebelum menulis byte berikutnya dengan ukuran file staging tidak melebihi batas ditambah satu buffer, entri melebihi `entryCount`, zip dengan data di depan, byte setelah komentar EOCD, entri bertumpuk, entri terenkripsi, metode bzip2, entri dengan data descriptor (diterima), `S_IFLNK` di zip buatan non Unix (menjadi file biasa), ruang disk palsu yang kecil lewat `FakeFilesystem`, dan ENOSPC yang disuntikkan di tengah ekstraksi (`insufficient_space`); puncak alokasi diukur dengan pengalokasi penghitung di test itu. Registry tanpa baris di semua kasus. Memverifikasi **AC-8**.
- Validasi: Mach-O fixture x86_64 saja (`architecture_mismatch` dengan teks Rosetta), universal x86_64 plus arm64 diterima, `minos` 99.0, referensi `/opt/homebrew/lib/libfoo.dylib`, `@rpath` yang hanya bisa diselesaikan lewat rpath absolut, library tak terdaftar dan library yang hilang dari daftar, executable symlink. Memverifikasi **AC-10**.
- Signing: `keep_or_adhoc` dengan satu file gagal memanggil `sign_adhoc` untuk semua file Mach-O di staging dan tidak menyentuh cache; `adhoc_pipeline` gagal; integration test `MacosCodeSigning` pada binary hasil `cc` di test (ad hoc, linker signed, disalin lalu dirusak satu byte). Memverifikasi **AC-11**.
- Exec check: unit test pencocokan dengan output harfiah `postgres (PostgreSQL) 18.6\n` dan pattern spec 0019, output `traefik version` beberapa baris, dan baris dengan `\r\n`; lalu skrip yang mencetak versi lain, keluar 3, `kill -9 $$` sendiri, `kill -ABRT $$` (`binary_check_failed` dengan `signal`), tidur melewati batas waktu (process group dihentikan, durasi di bawah batas ditambah 5 detik), dan file ber xattr quarantine yang tidak pernah di spawn. Memverifikasi **AC-12**.
- Commit dan crash: daemon dihentikan setelah rename sebelum batch (lewat titik injeksi test di `ScriptedWork` gaya spec 0007), start lagi, batch ditulis tanpa rename kedua; folder yatim dari operasi yang di abandon diambil alih; folder dengan marker operasi aktif menjadi `install_conflict`; dua install paralel artefak yang sama menghasilkan satu runtime dan satu `alreadyInstalled`; precheck `alreadyInstalled` tanpa operasi. Memverifikasi **AC-13**.
- Cancel: di setiap tahap sebelum Commit (scratch hilang, `.part` tetap, `cancelled`) dan di Commit (`continuing_past_point_of_no_return`, `succeeded`). Memverifikasi **AC-14**.
- Compatibility: generator xtask atas report fixture (scope `runtime` dari `spike-result.json`, scope `provider` dari `compatibility.json`, record terbaru menang, FAIL tetap ditulis), `compat-record` dengan setiap penolakan, report run lengkap provider yang memuat banyak record tanpa `BR-025` (hanya record BR-025 yang disalin), report tanpa record BR-025 (`no record to keep`), dan tabel `support_status`. Memverifikasi **AC-15**.
- CLI: golden teks dan JSON untuk `install` (started, `alreadyInstalled`, `--dry-run`, `--output id`), `list`, `inspect`, dan matriks exit code terhadap daemon skrip spec 0008. Memverifikasi **AC-16**.
- GUI: preview dan snapshot light, dark, lebar sempit 640 pt (cek teks besar spec 0012), Increase Contrast untuk halaman Runtimes (loading, empty, failed, unsupported, stale, partial, dengan Installing), inspector, dan tiga state `RuntimeGetAction`; test unit `StacklyUI` untuk rumus persen, label tahap, label dan tone status, dan teks VoiceOver; cek manual keyboard dan VoiceOver di `/check verify`. Memverifikasi **AC-17**.
- Kontrak: fixture setiap DTO, reply, event, dan kode di `protocols/fixtures/ipc/`, test kontrak Swift, dan `codegen --check`. Memverifikasi **AC-18**.
- Fuzz: dua target 10 menit, seed dari fixture archive dan Mach-O, regresi diputar ulang stable. Memverifikasi **AC-19**.
- Native: satu test per lini dari cache spike xtask tanpa jaringan, hanya untuk lini yang paket provider nya ada saat milestone 5 dibangun: `nats_2_15_installs_from_the_cache` dan `traefik_3_7_installs_from_the_cache` (paket sudah ada), serta `postgresql_17_installs_from_the_cache` dan `postgresql_18_installs_from_the_cache` (setelah spec 0019 langkah 4 (paket) dan langkah 7 (`runtime.versionCheck`, yang dibutuhkan exec check)). `mysql_8_4_installs_from_the_cache`, `mysql_9_7_installs_from_the_cache`, `clickhouse_installs_from_the_cache`, dan `valkey_9_1_installs_from_the_cache` ditulis sekarang tetapi tercatat `BLOCKED` dengan alasan `no provider package (features 40 to 43)` dan owner `@ojiepermana` sampai fitur 40 sampai 43 menambah paketnya. Kedelapan file artefak sudah ada di cache Mac ini per 2026-10-19. Lalu `nats_installs_from_the_network` yang hanya dijalankan setelah persetujuan kamu, semuanya lewat daemon in process ke `TestRoot`, evidence `native` dengan `covers = ["REQ-004", "REQ-005", "AC-049"]` dan subject versi teramati, sha256, `source: managed`; setiap file terpasang tanpa quarantine; pencatat spawn adapter hanya melihat `/usr/bin/codesign` dan executable artefak; `--cleanup` mematikan helper sisa dari run yang dibatalkan. Memverifikasi **AC-20**, **AC-6**, **AC-12**.
- Data catalog: `runtime check` aturan 10 sampai 12, `pin` mencetak `unpacked_size` dan `entry_count` yang sama dengan `scan`, `versions_rule` untuk field baru, dan install yang tetap jalan saat field opsional tidak ada. Memverifikasi **AC-21**.

## Build plan

Urutan mengikuti pendekatan Journey: jalur "pilih versi, pasang dengan aman, lihat hasilnya" dibangun lengkap termasuk semua jalur gagal sebelum layar lain menumpuk. Data catalog dan extractor duluan karena semua langkah lain membacanya; pipeline core lalu daemon dan CLI menutup jalur dari terminal; GUI dan bukti native menutup jalur dari aplikasi.

**Milestone 1: Data catalog, extractor aman, dan fuzz**

1. Data catalog: field inventory (`stable`, `build`, `unpacked_size`, `entry_count`, dependency) dan aturan `runtime check` 10, generator `versions.json` untuk `stable`, `build`, `unpackedSize`, `entryCount`, dan `dependencies`, decode dan `versions_rule` di `core::provider::versions` (field baru di `ArtifactStatus::Available` spec 0017), field manifest `runtime.versionCheck` beserta paket `fake`, `nats`, dan `traefik`, lalu isi `stable` dan dependency di inventory dan `cargo xtask codegen`. `pin` dan `verify` dengan `scan`, pengisian `unpacked_size` dan `entry_count`, dan aturan 11 tidak di sini, karena `archive::scan` baru ada di langkah 3. Butuh `/architect` update spec 0009 (setelah fitur 9 ditutup, *Usulan update spec 0009*) dan update spec 0003 yang diterapkan lead. Memenuhi **AC-21**, **AC-1** (bagian `stable`).
2. Tambahan `Filesystem` dan `TreeWriter`: `open_read`, `open_append`, `rename_no_replace`, `available_space` (provided method di atas `volume_space` spec 0022; kalau `volume_space` belum ada, langkah ini membangunnya dengan bentuk spec 0004, termasuk adapter `statfs`, fake, dan conformance), `open_tree_writer`, `EntryPath`, `FileMode`, di `MacosFilesystem`, `FakeFilesystem`, dan `fs_conformance`, dengan integration test race symlink di `crates/platform-macos/tests/`. Memenuhi **AC-7**, **AC-8**, **AC-13** (prasyarat).
3. Extractor `runtime::archive` (tiga format, normalisasi, symlink, batas, AppleDouble, mode, `scan`) dan builder archive fixture di testkit; lalu, setelah `scan` ada, `runtime pin` dan `verify` memakainya, `unpacked_size` dan `entry_count` diisi untuk semua entri `available` dari cache spike, aturan `runtime check` 11 diaktifkan, dan `cargo xtask codegen` dijalankan lagi. Memenuhi **AC-7**, **AC-8**, **AC-9**, **AC-21** (bagian `unpackedSize`).
4. Parser `runtime::macho` dan `runtime::closure` atas `entries`, dengan fixture Mach-O di testkit. Memenuhi **AC-10**.
5. Fuzz target `runtime_archive` dan `runtime_macho` di `fuzz/`, harness di modul `#[doc(hidden)] pub mod fuzzing` `stackly_core::runtime` (pola spec 0006, tanpa fitur crate), seed di `cargo xtask fuzz seed`, regresi di `crates/core/tests/fuzz_regressions.rs`, refresh `fuzz/Cargo.lock`. Memenuhi **AC-19**.

Prasyarat milestone ini: tipe domain `OsVersion` dan `ArtifactFormat` serta `ValidatedPackage::versions()` dan `ArtifactStatus` dari milestone 1 langkah 1 spec 0017 (tipe spec 0004 dan 0009); langkah 1 sampai 5 hanya memakainya. Sesi paralel: langkah 1 (xtask dan paket) dan langkah 2 (platform) tidak menyentuh file yang sama dan bisa jalan bersamaan. Langkah 3 dan 4 menunggu langkah 2 untuk tipe `EntryPath`, lalu bisa dua sesi; bagian `pin`, `verify`, dan aturan 11 di akhir langkah 3 juga menunggu langkah 1. Langkah 5 menunggu langkah 3 dan 4.

**Milestone 2: Pipeline install di core**

6. Registry: *Data model* (migrasi `0005_runtime_install.sql` dan fixture beku `registry-v5.sql` di atas v4, `ArtifactRecord` baru, `compatibility_records`, value type JSON) di domain, `FakeRegistry`, `registry_conformance`, dan `SqliteRegistry`, plus `Runtime::committed`, `RuntimeSelector`, dan `TeamId`. `OsVersion` dan `ArtifactFormat` dipakai dari milestone 1 langkah 1 spec 0017, tidak dibangun di sini. Butuh langkah 16 fitur 7 (`0004_operation_failure.sql`, K11) sudah di merge, dan tidak pernah berjalan bersamaan dengan sesi migrasi lain. Memenuhi **AC-13**, **AC-15** (prasyarat).
7. Port `Fetcher` dan `MacosFetcher` (ureq), `FakeFetcher`, dan `runtime::download` (cache, `.part`, resume, retry, klasifikasi). Memenuhi **AC-3**, **AC-4**, **AC-5**, **AC-6**.
8. Port `CodeSigning`, `MacosCodeSigning` (pindahan parser spike, memakai helper spawn dan kill `pub(crate)` modul `process`, K9), `FakeCodeSigning`, `PlatformInfo::host_facts` dengan `HostFacts` dan `StopFlag` di `stackly-platform` (`MacosPlatform::new` dan `with_roots` menjadi fallible dengan `HostFactsUnavailable`, dan langkah ini memperbarui semua pemanggil `with_roots`), `ChildRole::RuntimeCheck` kalau belum masuk bersama langkah 3 spec 0010 (K6), konstanta `FailureCode` `quarantined` dan `binary_rejected_by_os` dengan test pin nya dalam satu commit (K7), dan `runtime::validate` serta `runtime::launch` (signing, mode, quarantine, exec check lewat `spawn_helper`). Kalau langkah 7 spec 0010 sudah di merge, langkah ini juga memasang `check_launchable` dan `classify_early_exit` ke tahap `spawn`, `identity`, dan exit dini `core::supervisor`; kalau belum, langkah 7 spec 0010 yang memasangnya (K7). Butuh langkah 3 spec 0010 (adapter macOS dengan helper `pub(crate)`). Memenuhi **AC-10**, **AC-11**, **AC-12**.
9. `runtime::resolve`, `runtime::compatibility` (`support_status` dan `SupportVerdict` di atas `stackly_platform::HostFacts`) beserta perubahan catalog spec 0017 di *Usulan additive untuk spec lain* (gerbang dengan `&HostFacts` dan `requires_newer_os`, `SupportReason` baru, stand in diganti, aturan `APP_MINIMUM_MACOS` dihapus), `runtime::commit`, pekerjaan dan factory `install_runtime`, dan `runtime::fixture::install_cached_artifact` di balik fitur `fixture`. Memenuhi **AC-1**, **AC-2**, **AC-13**, **AC-14**, **AC-15**.

Sesi paralel: langkah 6, 7, dan 8 bisa tiga sesi (registry, jaringan, dan signing tidak berbagi file). Langkah 6 menyentuh file registry yang sama dengan tambahan registry fitur lain, jadi tidak pernah berjalan bersamaan dengan sesi lain yang menulis `WriteOp`. Langkah 9 menunggu ketiganya dan modul `core::catalog` fitur 17 (milestone 1 spec 0017, langkah 1 sampai 4); langkah 11 juga butuh langkah 6 spec 0017 untuk `CatalogOverride::Preloaded`.

**Milestone 3: Daemon, kontrak, dan CLI**

10. Kontrak: modul `stackly-protocol::runtime`, method di tabel `methods!`, event, area kode `runtime` beserta hint, fixture, `cargo xtask codegen`, test kontrak Swift, dan minor protokol. *Tambahan dari spec 0068 (2026-10-21):* kasus fixture tampilan runtime managed (baris runtime di *Kasus yang menunggu DTO* spec 0068; runtime external tetap milik fitur 56) di commit ini kalau `Fixtures/` spec 0068 sudah di `main`, selain itu milestone 1 spec 0068 yang menambahkannya. Memenuhi **AC-18**.
11. `stacklyd`: handler `ipc::runtime`, gerbang catalog, precheck, registrasi factory, `installing` dari engine, event, `OperationEngine::replay` dan helper `stacklyd` `replay_operation` beserta test paritas core terhadap `begin` (hit, miss, sidik jari lain, NoOp tercatat, `NotReady`) kalau belum dibangun fitur 7 (K12, paling lambat di langkah ini), fetcher test lewat `Platform::with_fetcher` di `DaemonDeps` (`MacosPlatform::with_roots(..)?.with_fetcher(Arc::new(FakeFetcher))` atau `FakePlatform`), dan integration test daemon (termasuk `runtime_install_refuses_an_artifact_for_another_target` terhadap fixture catalog spec 0017 `official/linuxonly` dan `official/intelonly`, yang membuktikan PRD AC-002 untuk milestone 4 spec 0017 dan AC-005 untuk fitur ini tanpa operasi yang dibuat). Memenuhi **AC-1**, **AC-2**, **AC-3**, **AC-6**, **AC-10**, **AC-13**, **AC-14**.
12. CLI `runtime install`, `list`, dan `inspect` dengan golden. Memenuhi **AC-16**.
13. Compatibility record di xtask: generator `compatibility` dari `evidence/runtime/` dan `evidence/compatibility/`, `runtime compat-record`, dan aturan `runtime check` 12. Memenuhi **AC-15**.

Sesi paralel: langkah 10 dulu; lalu langkah 11, 12 (setelah 10), dan 13 bisa tiga sesi.

**Milestone 4: Halaman Runtimes dan tombol Get**

Prasyarat: milestone 3, fitur 12, langkah 16 spec 0013 (`InstanceStore` dan `ResourceStore` final, yang menunggu milestone 2 fitur 10, K18), dan halaman Catalog spec 0017 milestone 3. *Ditambah 2026-10-21:* needs fitur 67 milestone 1, 2, dan 3 (`ui-macos.md`), atau membangun komponen itu menurut spec 0067 (aturan *Siapa membangun versi pertama*).


14. `StacklyClient`: method runtime dan tipe hasil generate; `RuntimeStore` dan integrasinya di `DaemonModel`. Memenuhi **AC-17**, **AC-18**.
15. `StacklyUI`: `RuntimesPage`, `InstallingRow`, `RuntimeInspector`, `RuntimeGetAction` di kolom aksi Catalog, tabel status runtime di `apps/macos/design.md`, preview, snapshot, dan test unit. *Tambahan dari spec 0067 (2026-10-21):* halaman dirakit dari `ProviderTile`, `StagedProgress`, `ActionMenuItems`, `.stacklyInspector`, `InspectorHeader`, dan `FeedbackCenter`, tanpa banner umpan balik sementara; `RuntimeStore.recordStarted(operationId:)` dan `finishedInstalls` dengan pemasangan di `AppShell`. *Tambahan dari spec 0068 (2026-10-21):* suite `RuntimesSnapshotTests` di atas harness spec 0068 untuk setiap state halaman dan tiga state `RuntimeGetAction`, dan `runtime.list` ditambah ke daftar call `list` yang dijawab `LinkHandle.fixture` sebelum `.afterSnapshot`, kalau milestone 1 spec 0068 sudah di merge, selain itu fitur 68 yang menulisnya (spec 0068 *Key invariants*). Memenuhi **AC-17**.

**Milestone 5: Bukti native** (dari cache tanpa jaringan; satu run jaringan hanya setelah persetujuan kamu; test PostgreSQL butuh spec 0019 langkah 4 (paket) dan langkah 7 (`runtime.versionCheck`))

16. Test native `apps/daemon/tests/native_runtime_install.rs`: test per lini dari cache spike xtask (tanpa jaringan, bisa jalan kapan saja) untuk NATS dan Traefik, untuk PostgreSQL 17 dan 18 setelah spec 0019 langkah 4 (paket) dan langkah 7 (`runtime.versionCheck`), dan test MySQL, ClickHouse, dan Valkey yang tercatat `BLOCKED` `no provider package (features 40 to 43)`, `stackly_testkit::runtime_cache`, cek quarantine, pencatat spawn, test native binary dengan tanda tangan rusak yang dijalankan lewat `spawn_main` (bukan hanya `spawn_helper`) dan diklasifikasi `binary_rejected_by_os` (K7, spec 0010), dan perluasan `runtime spike --cleanup`. Memenuhi **AC-20**, **AC-6**, **AC-12**.
16a. Setelah persetujuan kamu: run `nats_installs_from_the_network`, yang mengunduh NATS 2.15.1 (sekitar 6,8 MB) dari GitHub Releases ke `TestRoot`. Test ini `BLOCKED` dengan alasan `network run not approved` sampai variabel `STACKLY_NATIVE_NETWORK=1` diset pada run native itu. Memenuhi **AC-20**, **AC-5**.
17. Run fuzz 10 menit per target dan record manual. Memenuhi **AC-19**.

Sesi paralel: langkah 17 tidak bergantung pada milestone 2 sampai 4 dan bisa menjadi sesi sendiri begitu milestone 1 selesai. Langkah 16 menunggu milestone 3 dan berjalan dari cache; langkah 16a menunggu langkah 16 dan persetujuan kamu.

## Consequences

**Positive**:

- Satu pipeline untuk semua provider: NATS, PostgreSQL, dan provider berikutnya memakai jalur yang sama, dan fitur 19, 20, 23, 24, dan 44 tinggal menerima `RuntimeId`.
- Extractor dan parser Mach-O milik sendiri bisa difuzz dan berperilaku sama di semua Mac, tanpa bergantung pada perilaku bsdtar atau tool developer yang tidak ada di Mac bersih.
- Aturan spike spec 0003 (digest, closure, signing, quarantine) kini berlaku di Mac pengguna, jadi evidence G0 berlaku untuk hasil install.
- Install yang terputus, dibatalkan, atau gagal tidak pernah meninggalkan runtime setengah jadi, dan unduhan besar tidak diulang dari awal.
- Klaim supported hanya muncul dari evidence nyata, dan status `experimental` jujur tentang apa yang belum diuji.

**Negative / tradeoffs**:

- Banyak usulan ke spec lain (0001, 0003, 0004, 0005, 0009, 0010) harus diterapkan lead sebelum milestone 1 dan 2 bisa dibangun; urutan itu menahan fitur ini lebih lama dari fitur lain di fase 02.
- TLS lewat rustls dan ring menambah dependency C (ring) dan lisensi ISC ke graph, dan unduhan tidak memakai proxy sistem macOS sampai ada keputusan lanjutan; pengguna di balik proxy korporat belum bisa install.
- Extractor sendiri berarti kode keamanan sendiri yang harus direview (NFR-009) dan dirawat; perilakunya bisa berbeda dari bsdtar pada archive aneh, dan perbedaan itu baru ketahuan saat `pin` atau test native.
- `codesign` dijalankan sebagai proses per file Mach-O (PostgreSQL punya puluhan), menambah beberapa detik pada install.
- Semua runtime dari evidence spike hari ini tampil `experimental`, bukan `supported`, sampai test provider (mulai spec 0019) menulis record scope `provider`.
- Entri cache tidak dibersihkan fitur ini; cache bisa membesar (ClickHouse 216 MB) sampai pembersihan fitur 55 ada.
- Inspector di halaman Runtimes adalah selisih dari mockup yang harus dicatat untuk pembaruan mockup.
- Exec check menjalankan kode artefak sekali saat install; binary yang punya efek samping saat `--version` (tidak ada di baseline) akan menjalankannya.

**Neutral**:

- Folder baru `evidence/compatibility/`, tabel `compatibility_records`, empat kolom baru di `artifacts`, dua port platform baru, dan lima method baru di `Filesystem`.
- `xtask/src/runtime/spike/signing.rs` pindah ke adapter dan dipakai ulang xtask.
- Format `binary` tetap didukung walau belum ada entri yang memakainya.
- `ProtocolVersion` naik minor sekali untuk area `runtime`.

## Follow-up

- [x] `architect (update)` spec 0009 setelah fitur 9 ditutup: *Usulan update spec 0009* (field additive `versions.json`, tipe core, generator, dan `runtime.versionCheck`), digabung lead dengan usulan `ValidatedPackage::versions()` spec 0017; lalu centang follow up spec 0009 "Fitur 18: kalau install butuh field di luar `stackly.provider-versions/v1`". Diterapkan di spec 0009 (2026-10-20).
- [x] `architect (update)` spec 0003: field inventory, aturan `runtime check` 10 sampai 12, `pin` dan `verify` dengan `scan`, `runtime compat-record`, dan perluasan `--cleanup`; lalu centang empat follow up fitur 18 di spec 0003 (cara install memakai `versions.json`, `quarantine.outcome` dan symlink archive, extractor aman, AppleDouble `tar_gz`). Diterapkan di spec 0003 (2026-10-20); centang empat follow up fitur 18 di spec 0003 tetap lewat `/sync` lead. Catatan: aturan 11 spec 0003 menyebut aktif di milestone 1 langkah 1, padahal `scan` baru ada di langkah 3 (lihat Build plan).
- [x] `architect (update)` spec 0004: tambahan `Filesystem`, port `Fetcher` dan `CodeSigning`, `host_facts` dengan `HostFacts` (modul `info`) dan `StopFlag` (modul `fetch`) di `stackly-platform`, `OsVersion`, `TeamId`, `ArtifactFormat`, `RuntimeSelector`, dan `Runtime::committed` sebagai baris titik perluasan fitur 18; BR-025 dipetakan ke spec ini; centang follow up "Fitur 54: tambahkan `rename_no_replace` ke `Filesystem`" (dibangun fitur 18, fitur 54 hanya memakainya). Diterapkan di spec 0004 (2026-10-20), termasuk `with_roots` yang fallible dan helper codesign (K9).
- [x] `architect (update)` spec 0005: baris *Tambahan fitur 18* di tabel titik perluasan, menunjuk *Data model* spec ini. Diterapkan di spec 0005 (2026-10-20); nomor migrasi 0005 dan fixture v5 dari K11.
- [x] `architect (update)` spec 0010: `ChildRole::RuntimeCheck`, serta `check_launchable` dan `classify_early_exit` di tahap `spawn` start instance. Diterapkan di spec 0010 (2026-10-20, K6 dan K7).
- [x] `architect (update)` spec 0001: baris HTTP client di tabel batas keputusan, dependency crate baru di peta crate, dan aturan `check-deps` baru. Diterapkan di spec 0001 (2026-10-20).
- [ ] Spec 0002: centang follow up `--cleanup` lewat `/sync` setelah milestone 5 (dijawab di usulan spec 0003), dan `architect (update)` untuk `evidence/compatibility/` di *Folder dan file*, kalimat evidence otomatis di git, dan sumber fitur 65 (usulan **Spec 0002** di *Usulan additive untuk spec lain*). Bagian `evidence/compatibility/` dan perluasan `--cleanup` sudah diterapkan di spec 0002 (2026-10-20); yang tersisa hanya centang follow up `--cleanup` lewat `/sync` setelah milestone 5.
- [x] `architect (update)` spec 0006 (antrian lead): bentuk hint `codes::<area>::hint(code, details)` dengan `ErrorBody::with_hint`, juga untuk `FailureError` operasi tanpa hint (usulan **Spec 0006**). Diterapkan di spec 0006 (2026-10-20, commit `0e52572`).
- [ ] Spec 0007: catatan tabel resume `install_runtime` dijawab di *Alur install*; centang lewat `/sync`.
- [ ] Spec 0012 dan 0013: tabel status runtime masuk `design.md` dan `UnbuiltPage(.runtimes)` diganti di milestone 4; centang lewat `/sync`.
- [ ] Spec 0017: dua follow up fitur 18 di spec itu (stand in `// provisional: F18`, `installedSize`, `dependencies`, Get, progress, "{n} installed"; dan cek `minOs` saat berjalan lewat `HostFacts`) dijawab di *Usulan additive untuk spec lain*; centang lewat `/sync` setelah milestone 2 dan 4. `architect (update)` spec 0017 untuk `ProviderSummary.software`, baris *Copy* tiga reason baru, dan details serta hint `requires_newer_os` (usulan **Spec 0017**).
- [x] Spec 0019: deklarasikan `runtime.versionCheck` PostgreSQL dan tulis `compatibility.json` di test native nya. Diterapkan di spec 0019 (2026-10-20).
- [ ] Fitur 55: pembersihan cache dan hapus runtime wajib memegang lock `artifact:<sha256>` sebelum menghapus entri cache atau folder runtime, supaya tidak pernah menghapus file yang sedang dipakai install.
- [ ] Keputusan lanjutan (`/architect`): dukungan proxy sistem macOS untuk unduhan (misalnya lewat `SCDynamicStoreCopyProxies`), kalau pengguna di balik proxy korporat menjadi kebutuhan.
- [ ] Mockup UI: catat inspector halaman Runtimes dan menu konteks Cancel Install sebagai selisih yang disengaja.
- [ ] `/sync` setelah build: catat port `Fetcher` dan `CodeSigning`, dependency `ureq`, `tar`, `flate2`, `zip`, dan `object`, folder `evidence/compatibility/`, `cargo xtask runtime compat-record`, dan fuzz target baru di `AGENTS.md`.

## Rationale

Alasan, opsi yang dipertimbangkan, dan runner up per keputusan: lihat [rationale.md](rationale.md).
