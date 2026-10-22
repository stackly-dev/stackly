# Verify: Install runtime managed · spec 0018 · updated 2026-10-21

_Langkah diturunkan dari acceptance criteria spec 0018. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Bagian Milestone 1 dan 3 ditulis sesi milestone itu._

## Milestone 1: Catalog data, safe extractor, and fuzz

### UI / manual

- [x] Buka `providers/fake/provider.yaml`, `providers/nats/provider.yaml`, dan `providers/traefik/provider.yaml` → masing masing punya blok `runtime.versionCheck` dengan `args` literal (`["version"]`, `["--version"]`, `["version"]`), `pattern` dengan tepat satu capture group, dan `timeoutSeconds: 10`; PostgreSQL membawa blok nya sendiri dari spec 0019 → AC-12, AC-21
- [x] Buka `runtimes/inventory.toml` → setiap entri `available` punya `unpacked_size` dan `entry_count` berdampingan (misalnya PostgreSQL 18.6 80668813 dan 2030, MySQL 9.7.2 713257358 dan 376), diukur dari cache spike tanpa jaringan → AC-21
- [x] Buka `providers/nats/versions.json` → artefak `available` membawa `stable`, `unpackedSize`, `entryCount`, `dependencies`, dan `compatibility` (satu record scope `runtime`), dan artefak `blocked` tidak membawa satu pun field fase 02 → AC-21
- [x] Buka `fuzz/seeds/runtime_archive/` dan `fuzz/seeds/runtime_macho/` → 54 dan 9 seed, byte pertama seed archive memilih format dan pengupasan folder; isinya hasil `cargo xtask fuzz seed`, tidak ditulis tangan → AC-19

### Commands

- [x] `cargo xtask runtime check` → aturan 1 sampai 12 lulus atas inventory dan seluruh evidence (aturan 11 aktif, aturan 12 membaca `evidence/compatibility/`) → AC-21
- [x] `cargo xtask codegen --check` dan `cargo xtask provider check` dan `cargo xtask catalog check` → file generate tidak berbeda dan ketiga paket serta catalog lulus tanpa ledger baru → AC-21
- [x] `cargo test -p stackly-core --lib provider::versions` → field fase 02 (`stable`, `build`, `unpackedSize`, `entryCount`, `dependencies`, `compatibility`) di `versions.json` lolos dan setiap aturan yang dilanggar menunjuk lokasinya → AC-21
- [x] `cargo test -p stackly-core --test provider_packages` → paket fake, nats, traefik, dan PostgreSQL membaca versinya dari keluaran harfiah binary lewat `runtime.versionCheck`, dan golden rencana tidak berubah → AC-12, AC-21
- [x] `cargo test -p stackly-core --lib runtime::archive` → tiga format, normalisasi nama, symlink, batas, AppleDouble, mode, dan satu sampel untuk setiap `RejectReason` → AC-7, AC-8, AC-9
- [x] `cargo test -p stackly-core --lib runtime::macho` dan `cargo test -p stackly-core --lib runtime::closure` → Mach-O tipis dan universal, minos, library, rpath, closure atas entri → AC-10
- [x] `cargo test -p xtask --bin xtask runtime::` → `pin` mencetak `unpacked_size` dan `entry_count` dari `scan` (zip dengan satu folder puncak, zip tanpa folder, binary, tar.gz sungguhan), `verify` menolak angka yang berbeda atau hilang, dan aturan 11 menolak salah satu saja, nol dengan entri, dan negatif → AC-21
- [x] `cargo test -p stackly-core --test fuzz_regressions` → input regresi dan seed `runtime_archive` dan `runtime_macho` lolos harness mereka, juga dengan satu byte dibalik di sebaran posisi → AC-19
- [x] `cargo xtask fuzz build runtime_archive` lalu `cargo xtask fuzz build runtime_macho` lalu `cargo xtask fuzz run runtime_archive --time 40` dan `cargo xtask fuzz run runtime_macho --time 40` → keduanya build di nightly yang dipin dan berjalan tanpa crash; run 10 menit dan record evidence menunggu Milestone 5 → AC-19
- [x] `cargo test --workspace --locked --no-fail-fast` → semuanya lulus → AC-21

### Acceptance-criteria coverage

- AC-7 sampai AC-9 · extractor dan sampel nya (langkah 3 awal); tes race symlink di adapter macOS dari langkah 2
- AC-10 · parser Mach-O dan closure (langkah 4); keputusan arsitektur dan `minos` di pipeline ada di Milestone 2
- AC-12 · pembacaan versi dari keluaran harfiah setiap paket (langkah 1); exec check nya sendiri ada di Milestone 2
- AC-19 · target dan harness dibangun dan di-smoke 40 detik tanpa crash; dua run 10 menit dan record evidence manual menunggu Milestone 5 langkah 17
- AC-21 · field inventory dan `versions.json`, aturan `runtime check` 10 sampai 12, `pin` dan `verify` dengan `scan`; `runtime verify` terhadap jaringan (membandingkan angka dengan unduhan baru) tidak dijalankan di sini karena butuh jaringan, angkanya diukur dari cache spike
- AC-1 (bagian `stable`) · `stable` di inventory dan `versions.json`

## Milestone 2: Install pipeline in core

### UI / manual

- [x] Buka `crates/storage/migrations/0005_runtime_install.sql` → empat `ADD COLUMN` di `artifacts` (`catalog_id`, `build`, `format`, `size_bytes`, semuanya nullable) dan tabel `compatibility_records` dengan CHECK `provider`, `provider_revision`, dan `manifest_sha256` terisi tepat saat `scope = 'provider'` → AC-13, AC-15
- [x] Buka `crates/storage/tests/fixtures/registry-v5.sql` → baris `artifacts` yang membawa `catalog_id`, `build`, `format`, `size_bytes`, dan dua baris `compatibility_records` (satu per scope); file itu beku dan hash nya ada di `FIXTURE_SHA256` → AC-13, AC-15
- [x] Buka `crates/platform-macos/src/codesign.rs` → satu satunya program yang dijalankan adalah `/usr/bin/codesign` (literal itu hanya ada di modul ini), lewat `process::spawn::launch` dan `process::kill::signal_group`, stdin `/dev/null`, environment kosong, hanya me reap PID nya sendiri → AC-11, AC-20
- [x] Buka `crates/core/src/runtime/validate.rs` → langkah exec check memakai `ChildRole::RuntimeCheck`, `CHILD_PATH`, `HOME` dan `TMPDIR` di `scratch/check/`, dan saat batas waktu lewat memanggil `terminate_helper_group(force: true)` tanpa `SIGTERM` → AC-12
- [x] Buka `crates/core/src/runtime/install.rs` → lima tahap `download`, `verify_digest`, `extract`, `validate_binary`, `commit` (langkah `commit` `irreversible`), input journal tanpa selector sebagai sumber kebenaran, dan `finalize` yang menulis `InsertArtifact` lalu `InsertRuntime` dalam satu batch → AC-1, AC-13, AC-14

### Commands

- [x] `cargo test -p stackly-domain` → `Runtime::committed` (installed di revision 1 dengan `installed_at` dan `last_verified_at`, runtime external ditolak), `RuntimeSelector` (`stable`, `sha256:` huruf kecil 64 hex, versi dan line, teks tidak sah), `TeamId`, `CompatibilityRecordRow`, dan test pin `FailureCode` `quarantined` dan `binary_rejected_by_os` lulus → AC-1, AC-12, AC-13, AC-15
- [x] `cargo test -p stackly-storage` → fixture v1 sampai v5 tidak berubah, sidik jari skema versi 5 cocok, migrasi v4 ke v5 menjaga semua baris, conformance `SqliteRegistry` memuat `artifact_install_details` → AC-13, AC-15
- [x] `cargo test -p stackly-testkit` → conformance `FakeRegistry` memuat skenario yang sama dengan `SqliteRegistry` → AC-13, AC-15
- [x] `cargo test -p stackly-platform` → `HttpsUrl` menolak `http`, userinfo, host kosong, spasi, dan panjang lebih dari 2048 → AC-5
- [x] `cargo test -p stackly-platform-macos --test fetch` → server loopback: `Range` dan `If-Range` dikirim lagi di setiap hop, 200 untuk permintaan `Range` mulai dari byte 0, 206 dengan `Content-Range` salah menjadi `LengthMismatch`, 416, redirect ke `http` ditolak, enam redirect ditolak dan lima diikuti, `Content-Encoding: gzip` tidak didekompresi, `Content-Length` salah, byte lebih dari `size`, server diam menjadi `IdleTimeout`, port tertutup menjadi `Unreachable`, validator lemah tidak dipakai → AC-3, AC-5
- [x] `cargo test -p stackly-platform-macos --test fetch_tls` → sertifikat buatan test ditolak `Tls` oleh trust store macOS → AC-5
- [x] `cargo test -p stackly-platform-macos --test codesign` → binary sistem `OtherSigned`, salinan tanpa signature menjadi ad hoc lalu valid, satu byte berubah gagal verifikasi tanpa path di `detail`, binary hasil `cc` `LinkerSigned` (dilewati bila tidak ada kompiler), quarantine dibaca dari xattr dan tidak lewat symlink → AC-11
- [x] `cargo test -p stackly-core --test runtime_download` → cache entry dipakai tanpa jaringan, resume dengan `Range` dari byte terakhir yang di `sync`, 200 untuk `Range` mulai dari 0, validator berganti, tanpa validator, digest salah (`.part` dan entri dihapus), unduhan dilanjutkan yang salah diulang sekali, offline, retry 5xx dan 429, 4xx pindah URL, urutan kode akhir, ruang disk, stop di tengah dan di jeda retry → AC-3, AC-4, AC-5, AC-6
- [x] `cargo test -p stackly-core --test runtime_validate` → arsitektur x86_64 saja (diagnosis Rosetta), universal diterima, `minos` terlalu baru, library di luar runtime, himpunan dependency, kebijakan signing `keep_or_adhoc` dan `adhoc_pipeline`, quarantine tanpa spawn, exec check (versi cocok, berbeda, tidak cocok pattern, keluar bukan 0, `SIGKILL`, sinyal lain, batas waktu dengan group dibunuh sekali tanpa `SIGTERM`, `EBADARCH`, `EACCES`), mode akhir dan marker → AC-10, AC-11, AC-12
- [x] `cargo test -p stackly-core --test supervisor_start launch::` → executable ber quarantine ditolak `runtime.quarantined` sebelum spawn, `SIGKILL` sebelum ready menjadi `runtime.binary_rejected_by_os` tanpa budget restart, sinyal lain tetap crash biasa, child yang mati `SIGKILL` saat tertahan terklasifikasi, discard oleh daemon tidak → AC-12
- [x] `cargo test -p stackly-core --test runtime_resolve` → tabel selector (`stable`, versi, line, ambigu, digest, digest target lain tanpa pengganti, `blocked`, `minOs`) dan aturan `support_status` berurutan → AC-1, AC-10, AC-15
- [x] `cargo test -p stackly-core --test runtime_commit` → rename bersih, rename yang sudah terjadi, folder yatim (operasi hilang atau gagal) dipindah ke scratch, konflik tanpa marker atau dengan artefak lain atau operasi yang masih berjalan atau runtime terdaftar → AC-13
- [x] `cargo test -p stackly-core --test runtime_install` → operasi lima tahap lewat engine: sukses satu runtime `installed` dengan artefak, signature, dependency, dan cache tetap; digest salah; offline lalu dari cache; versi berbeda; ad hoc signing; dua install artefak sama menjadi satu runtime; cancel saat unduhan menyisakan `.part`; shutdown lalu start lagi melanjutkan dari `.part`; catalog bergeser membuat runtime kedua dan yang lama tidak berubah → AC-1, AC-2, AC-3, AC-4, AC-6, AC-13, AC-14
- [x] `cargo test -p stackly-core --test runtime_fixture_install` → `install_cached_artifact` memasang tanpa engine dan jaringan, menulis registry sendiri, dan gagal dengan error pipeline untuk artefak atau digest yang salah → AC-13
- [x] `cargo test -p stackly-core --lib catalog` → gerbang dengan `&HostFacts` dan `requires_newer_os`, status sel dari `support_status` → AC-10, AC-15
- [x] `cargo xtask check-deps`, `cargo deny check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → semuanya lulus → AC-5

### Acceptance-criteria coverage

- AC-1 · resolusi (`stable`, digest, versi, line, ambigu, tidak cocok, target lain lewat gerbang) dan journal yang membekukan artefak hasil resolusi (selector hanya asal); handler `runtime.resolve` dan reply menunggu Milestone 3
- AC-2 · runtime yang terpasang tidak berubah saat line bergeser, `@line` memasang runtime kedua (`a_catalog_that_moves_a_line...`); versi daemon in process dengan restart menunggu Milestone 3
- AC-3 · resume `.part` dengan `Range` dan `If-Range` (`FakeFetcher` dan server loopback), operasi yang dilanjutkan setelah shutdown; install baru dengan key lain memakai `.part` yang sama lewat test download
- AC-4 · digest, ukuran, dan magic number salah tidak pernah diekstrak; `.part` dibuang dan satu unduhan ulang; entri cache dihitung ulang sebelum dipakai
- AC-5 · kebijakan unduhan di `MacosFetcher` (https saja, 5 redirect, identity, panjang, idle, retry, urutan kode akhir, ruang disk); batas waktu connect 15 detik dan idle 30 detik adalah konstanta yang diuji dengan nilai pendek
- AC-6 · `runtime.download_offline` tanpa cache dan install dari cache tanpa jaringan
- AC-10 · arsitektur, `minos`, closure, dependency, dan `requires_newer_os` di gerbang; keluaran gerbang untuk target lain lewat `availableTargets` ditampilkan lewat daemon di Milestone 3
- AC-11 · kebijakan signing `keep_or_adhoc` dan `adhoc_pipeline`, cache tidak pernah diubah, tanpa langkah yang menghapus quarantine
- AC-12 · exec check, quarantine, `SIGKILL`, batas waktu, `EBADARCH`, dan klasifikasi di supervisor; test native binary bertanda tangan rusak lewat `spawn_main` menunggu Milestone 5
- AC-13 · commit (rename menolak tujuan yang ada, rename yang sudah terjadi, folder yatim, konflik), satu batch `InsertArtifact` dan `InsertRuntime`, lock `artifact:<sha256>`; crash di antara rename dan batch diuji di tingkat `commit_tree`, bukan di tingkat operasi
- AC-14 · cancel sebelum commit menghapus scratch dan menyisakan `.part`, kegagalan sebelum commit dikompensasi engine; disposisi di tahap `commit` memakai langkah `irreversible` dan diuji engine di spec 0007
- AC-15 · `support_status` dan penyalinan record ke registry saat install (kolom dan baris `compatibility_records`); generator xtask dan `compat-record` menunggu Milestone 3 langkah 13
- AC-7 sampai AC-9, AC-19, AC-21 · Milestone 1; AC-16 sampai AC-18 · Milestone 3 dan 4; AC-20 · Milestone 5

## Milestone 3: Daemon, contract, and CLI

### UI / manual

- [x] Buka `crates/protocol/src/runtime.rs` → empat method (`runtime.install`, `resolve`, `list`, `get`) dengan params yang divalidasi (`provider` ProviderName, `selector` 1 sampai 80 byte, `expectedSha256` 64 hex huruf kecil, `limit` 1 sampai 200), semua enum terbuka berakhir `Unrecognized`, dan `InstallOutcome` bertag `type` dan `content` dengan `started` dan `alreadyInstalled` → AC-18
- [x] Buka `crates/protocol/src/codes.rs` bagian `runtime` → 26 kode dengan kategori dan kunci `details` seperti tabel *Kode error*, semuanya `retryable false`, dan fungsi `hint` yang memberi satu kalimat statis di bawah 200 karakter untuk setiap kode; `catalog.not_installable` mendapat hint `requires_newer_os` → AC-1, AC-10
- [x] Buka `apps/daemon/src/ipc/runtime.rs` → `install` menanyakan replay ke engine lebih dulu (tanpa membaca catalog), baru resolusi, `expectedSha256`, lalu `begin` dengan lock `artifact:<sha256>`, `BusyPolicy::Queue`, dan precheck yang menjawab `alreadyInstalled` tanpa operasi atau menolak `runtime.already_registered`; `operations.rs` mendaftarkan `InstallRuntimeFactory` → AC-1, AC-2, AC-3, AC-13
- [x] Buka `apps/daemon/src/ipc/operation.rs` (`announce_runtime`) → `runtime.state_changed` diterbitkan hanya untuk runtime yang dibuat operasi `install_runtime` yang `succeeded`, dengan revision 1 → AC-18
- [x] Buka `protocols/fixtures/runtime/` dan `protocols/fixtures/ipc/*runtime*` → daftar, halaman terakhir, detail, artefak terpilih, balasan `started` dan `alreadyInstalled`, event, dua error, dan balasan dengan nilai asing; semuanya dibaca Rust dan Swift → AC-18
- [x] Buka `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Fixtures/FixtureRuntimeCases.swift` → data kasus runtime managed dari DTO hasil generate, `FixtureClock`, dan `FixtureID` (kind 3, nomor kasus instance), tanpa `Date.now` atau `ContractInfo`, dan belum terdaftar sebagai skenario karena daftar nama skenario dipaku test spec 0068 → AC-18
- [x] Jalankan `stackly runtime install official/postgresql@18 --dry-run` terhadap daemon dengan catalog uji → teks `Provider`, `Artifact`, `Digest`, dan seterusnya; `--output id` bersama `--json` atau `--dry-run` keluar dengan kode 2 → AC-16

### Commands

- [x] `cargo test -p stackly-protocol` → kontrak (tabel `methods!`, kode dan hint, batas, fixture runtime dan ipc, nilai asing, versi 1.11) lulus → AC-18
- [x] `cargo xtask codegen --check` dan `cargo xtask fuzz seed` → file generate dan seed tidak berbeda (`protocols/contract-versions.json` mencatat 1.11) → AC-18
- [x] `cargo test -p stacklyd --test runtime` → resolve (stable, line, versi, digest), penolakan (`selector_unresolved`, `selector_changed`, provider tidak ada dengan saran, entri `Planned`, target Linux dan Intel dari fixture catalog tanpa operasi), install lima tahap sampai satu runtime `installed` yang bisa dibaca dan di list, event `runtime.state_changed`, `alreadyInstalled`, replay key, digest salah dengan hint, `already_registered`, paging, install yang belum berakhir di halaman pertama saja, restart di tengah unduhan, dan cancel yang menyisakan `.part` → AC-1, AC-2, AC-3, AC-10, AC-13, AC-14, AC-18
- [x] `cargo test -p stacklyd --lib map::runtime` dan `cargo test -p stacklyd --lib map::catalog` → setiap kode `runtime.*` yang dilempar core ada di katalog dengan hint, pemetaan error resolusi, dan `requires_newer_os` di `catalog.not_installable` → AC-1, AC-10
- [x] `cargo test -p stackly-cli --test runtime` → `install` (resolve lalu install dengan `expectedSha256`, `--wait`, `alreadyInstalled`, `--dry-run`, `--output id`, argumen salah, baris exit code), `list` (semua halaman, kosong), dan `inspect` dengan golden teks dan JSON → AC-16
- [x] `swift test --package-path clients/swift` → `RuntimeContractTests` dan fixture `ipc/index.json` (30 method) lulus → AC-18
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` → seluruh suite lulus, tidak ada referensi snapshot lama yang bergeser, dan `FixtureRuntimeTests` lulus → AC-18
- [x] `cargo test --workspace --locked --no-fail-fast`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo xtask check-deps`, `cargo xtask runtime check`, `cargo xtask provider check`, `cargo xtask catalog check` → semuanya lulus → AC-15

### Acceptance-criteria coverage

- AC-1 · resolusi lewat daemon (`runtime.resolve` dan `install`), hint kode runtime di jawaban dan di kegagalan operasi tersimpan; support selalu `unverified` karena catalog belum membaca record compatibility
- AC-2 · daemon yang memakai artefak dari journal saat restart (test restart mengganti catalog dengan yang kosong); `@line` kedua dan catalog yang bergeser tetap diuji di tingkat core (Milestone 2)
- AC-3 · restart di tengah unduhan dilanjutkan dari journal lewat factory; `Range` dan `If-Range` diuji di core dan di server loopback (Milestone 2)
- AC-6 · jalur offline dan cache diuji di core (Milestone 2); di daemon hanya jalur kode `download_offline` dan hint nya
- AC-10 · fixture catalog `official/linuxonly` dan `official/intelonly` ditolak dengan alasan dan tanpa operasi (`runtime_install_refuses_an_artifact_for_another_target`)
- AC-13 · satu runtime dari dua install artefak sama (`alreadyInstalled` tanpa operasi), runtime di state lain ditolak, event setelah commit
- AC-14 · cancel saat unduhan berakhir `cancelled` dan menyisakan `.part`; cancel di tahap Commit diuji di core
- AC-15 · generator `compatibility`, `runtime compat-record`, dan aturan 12 sudah dilaporkan terpisah (langkah 13); pemakaian record oleh `support_status` di daemon menunggu catalog membaca record
- AC-16 · `install`, `list`, dan `inspect` dengan golden; `--timeout` dan `--idempotency-key` datang dari `AsyncArgs` milik spec 0008
- AC-18 · kontrak 1.11, fixture, test Rust dan Swift; kasus fixture tampilan berupa data tanpa skenario sampai halaman Runtimes (Milestone 4)
- AC-4, AC-5, AC-7 sampai AC-9, AC-11, AC-12 · Milestone 1 dan 2; AC-17 · Milestone 4; AC-19 · Milestone 5; AC-20 · Milestone 5

## Milestone 5: Native proof

_Dari cache spike xtask tanpa jaringan. Run jaringan `nats_installs_from_the_network` (langkah 16a) disetujui user lewat lead pada 2026-10-22 dan lulus; tanpa `STACKLY_NATIVE_NETWORK=1` test itu tetap `BLOCKED` `network run not approved`. Fuzz dan test native memakai ruang disk; hapus `fuzz/target` setelah run fuzz._

### UI / manual

- [x] Buka `apps/daemon/tests/native_runtime_install.rs` → satu test per lini dari cache: `nats_2_15_installs_from_the_cache`, `traefik_3_7_installs_from_the_cache`, `postgresql_17_installs_from_the_cache`, `postgresql_18_installs_from_the_cache`; empat test `mysql_8_4_`, `mysql_9_7_`, `clickhouse_`, dan `valkey_9_1_installs_from_the_cache` selalu berakhir `BLOCKED` `no provider package (features 40 to 43)` dengan owner `@ojiepermana`; `nats_installs_from_the_network` berakhir `BLOCKED` `network run not approved` tanpa `STACKLY_NATIVE_NETWORK=1` → AC-20
- [x] Buka `apps/daemon/tests/native_runtime_install/recorders.rs` → pencatat ada di port, bukan di adapter (adapter macOS tidak punya hook spawn): `RecordingProcesses` mencatat program, argumen, peran, dan environment setiap `spawn_main` dan `spawn_helper`; `RecordingCodeSigning` mencatat `assess`, `sign_adhoc`, dan `quarantine`; `NoNetwork` menolak dan mencatat setiap permintaan unduh. `MacosCodeSigning` hanya menjalankan `/usr/bin/codesign` (konstanta di `crates/platform-macos/src/codesign.rs`) → AC-20
- [x] Buka `crates/testkit/src/runtime_cache.rs` → `RuntimeCache` membuka cache xtask (`~/Library/Caches/dev.stackly.xtask/runtime`), menganggap hanya file biasa dengan ukuran dan digest yang cocok sebagai entri, dan menyalin ke `downloads_dir()` daemon sebagai `<sha256>` → AC-20
- [x] Buka `xtask/src/runtime/spike/cleanup.rs` → `sweep_test_processes` (TERM, tunggu 5 detik, KILL) untuk proses milik user ini yang argv nya memuat `/stackly-test-`; hanya `runtime spike --cleanup` yang memanggilnya (di dalam lock spike), driver spike tidak → AC-20
- [x] Buka `evidence/manual/*runtime-archive*` dan `evidence/manual/*runtime-macho*` → dua record manual run fuzz 600 detik dengan statistik libFuzzer, commit sumber, dan tanpa crash, hang, atau out of memory (kind `fixture`, seperti record fuzz sebelumnya, karena `evidence check` menolak record native PASS tanpa subject) → AC-19

### Commands

- [x] `cargo xtask runtime spike --cleanup`, lalu `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test native_runtime_install --locked -- --ignored`, lalu `--cleanup` lagi → 12 record native: 7 PASS (empat lini dari cache, offline, tanda tangan rusak, quarantine), 5 BLOCKED (empat tanpa paket, satu jaringan tanpa `STACKLY_NATIVE_NETWORK=1`), 0 masalah → AC-20, AC-6, AC-12
- [x] `cargo test -p stacklyd --test native_runtime_install --locked` → test sumber `no_daemon_code_names_a_developer_tool_to_run` lulus tanpa native: tidak ada baris bukan komentar di `stackly-core`, `stackly-platform-macos`, dan `stacklyd` yang menyebut `otool`, `lipo`, `xcrun`, `xcodebuild`, atau `clang` sebagai program → AC-20
- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test native_runtime_install --locked -- --ignored a_binary_with_a_damaged_signature` → `nats-server` dari artefak cache keluar 0 lewat `spawn_main`; salinan dengan satu byte instruksi pertama diubah dimatikan `SIGKILL` oleh OS dan `classify_early_exit` menyebutnya `binary_rejected_by_os` → AC-12
- [x] (disetujui, lulus) `STACKLY_NATIVE_NETWORK=1 cargo xtask evidence run --kind native -- cargo test -p stacklyd --test native_runtime_install --locked -- --ignored nats_installs_from_the_network` → NATS 2.15.1 (sekitar 6,8 MB, sha256 `ed0fe392…5ffd`, source managed) diunduh ke `TestRoot` sampai `installed` dalam 4,4 detik, 1 exec check, tanpa quarantine; record manual `evidence/manual/2026-10-22-network-install-of-nats-2-15-1-to-a-testroot-thr.json` → AC-20, AC-5
- [x] `cargo xtask fuzz run runtime_archive --time 600` dan `cargo xtask fuzz run runtime_macho --time 600`, satu per satu → exit 0 dengan `ran for 600 s without a crash`, `fuzz/artifacts/` kosong → AC-19
- [x] `cargo test -p stackly-testkit --lib runtime_cache` dan `cargo test -p xtask -- the_test_sweep` → `RuntimeCache` dan sweep proses test lulus → AC-20
- [x] `cargo xtask evidence check` (52 record manual, baseline valid, tanpa pelanggaran; 39 pelanggaran lama milik spec 0019 sudah hilang sejak perbaikan evidence F19), `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps` → lulus

### Acceptance-criteria coverage

- AC-20 · empat lini yang paketnya ada dipasang dari cache lewat daemon in process ke `TestRoot` tanpa satu pun permintaan unduh, dengan exec check `runtime.versionCheck` yang dijalankan di staging (peran `RuntimeCheck`, empat variabel environment tetap), tanpa `com.apple.quarantine` di pohon terpasang (`xattr -lr`), dan setiap pertanyaan ke port code signing menyangkut file di root test; empat lini tanpa paket tercatat `BLOCKED`; `--cleanup` mematikan proses sisa `/stackly-test-`; run jaringan menunggu persetujuan
- AC-6 · offline tanpa entri cache: `runtime.download_offline` dengan `failure.retryable` true dalam waktu percobaan (test `an_install_without_a_network_and_without_a_cache_entry_is_offline_not_a_hang`); dengan entri cache, install jalan dengan nol permintaan unduh (test lini)
- AC-12 · quarantine pada executable ditolak `check_launchable` tanpa spawn; binary dengan tanda tangan rusak dimatikan OS dan diklasifikasi `binary_rejected_by_os` lewat `spawn_main`. Yang dibuktikan di sini adalah klasifikasi pada adapter nyata; pemasangan ke `core::supervisor` sudah diuji dengan fake di `crates/core/tests/supervisor_start/launch.rs`
- AC-19 · dua run fuzz 600 detik, record manual di `evidence/manual/`
- AC-5 (bagian jaringan) · langkah 16a lulus: unduhan https nyata lewat `MacosFetcher` sampai `installed`; kebijakan redirect, TLS, dan batas ada di integration test `MacosFetcher` (Milestone 2)

### Hasil verify checker (2026-10-22, sesi `chk-f18-m5`, main `dbb5fc7`)

**Putusan: PASS** untuk AC-20, AC-6, AC-12, AC-19, dan AC-5 (bagian jaringan). Semua langkah dijalankan ulang kecuali dua yang lead kecualikan.

- Dijalankan ulang di worktree bersih: `runtime spike --cleanup` sebelum dan sesudah (0 proses, 0 agen, tanpa proses `/stackly-test-`); `evidence run --kind native` tanpa `STACKLY_NATIVE_NETWORK` → 12 record (7 PASS, 5 BLOCKED, 0 masalah), 12 test lulus dalam 17,21 detik, run id `01a161c1-a2eb-70a3-b4ec-cae2749822b4`; `no_daemon_code_names_a_developer_tool_to_run`; `runtime_cache` (3 test); `the_test_sweep` (1 test); `cargo fmt --all --check`; `cargo xtask check-deps`; `cargo xtask evidence check`; `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- Record native: PASS NATS 2.15.1 (2 pertanyaan codesign, 1 exec check, 0 fetch), Traefik 3.7.15, PostgreSQL 17.11 (132 pertanyaan codesign) dan 18.6 (134), semuanya `source: managed`, tanpa `com.apple.quarantine`; PASS offline (`runtime.download_offline` setelah 140 ms, retryable, tanpa runtime); PASS quarantine (ditolak, agent `Safari`, tidak ada spawn); PASS tanda tangan rusak (kontrol keluar 0, salinan rusak `Signaled(9)`, `binary_rejected_by_os`). BLOCKED: empat lini `no provider package (features 40 to 43)` (owner `@ojiepermana`) dan `nats_installs_from_the_network` `network run not approved`.
- Tidak dijalankan ulang (keputusan lead): langkah 16a diperiksa dari `evidence/manual/2026-10-22-network-install-of-nats-2-15-1-to-a-testroot-thr.json` (subject nats 2.15.1, sha256 `ed0fe392…5ffd`, `source: managed`, PASS); run fuzz 600 detik diperiksa dari dua record manual `runtime-archive` (882320 run, 1468 exec/s) dan `runtime-macho` (1867869 run, 3107 exec/s), commit sumber `27e4135`, tanpa crash, hang, atau OOM; `fuzz/artifacts/runtime_archive` dan `runtime_macho` kosong.
- Penyimpangan developer, dinilai terhadap AC: (1) pencatat spawn di port memenuhi AC-20 karena adapter macOS memang tidak punya hook spawn dan `MacosCodeSigning` hanya menjalankan `/usr/bin/codesign` (konstanta); catatan: "hanya codesign dan executable artefak" dibuktikan lewat peran, environment, dan path program yang dicatat di port serta test sumber, bukan lewat pengamatan exec; (2) `sha2` di `crates/testkit` adalah dependency workspace yang dipakai `RuntimeCache` untuk memeriksa digest, `check-deps` lulus; (3) record fuzz ber-kind `fixture` diterima, `evidence check` menolak PASS native tanpa subject dan AC-19 meminta hasil tercatat, bukan kind native.
- Dipindah, bukan FAIL: bagian a langkah 17 (`--cleanup` mencabut agent `dev.stackly.test.agent.*`) dipindah ke milestone 2 F15 atas keputusan lead (F15 yang pertama membuat LaunchAgent sementara). `--cleanup` saat ini mematikan proses `/stackly-test-` (AC-20) dan agent spike (`agent not loaded` pada kedua run).
- Catatan kecil: record jaringan punya `finishedAt` (03:12:00) lebih awal dari `startedAt` (03:13:58), dan `sourceDirty: true` di ketiga record manual; tidak mengubah putusan.
