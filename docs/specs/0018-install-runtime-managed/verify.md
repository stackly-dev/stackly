# Verify: Install runtime managed · spec 0018 · updated 2026-10-21

_Langkah diturunkan dari acceptance criteria spec 0018. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Bagian Milestone 1 ditulis sesi milestone itu._

## Milestone 2: Install pipeline in core

### UI / manual

- [ ] Buka `crates/storage/migrations/0005_runtime_install.sql` → empat `ADD COLUMN` di `artifacts` (`catalog_id`, `build`, `format`, `size_bytes`, semuanya nullable) dan tabel `compatibility_records` dengan CHECK `provider`, `provider_revision`, dan `manifest_sha256` terisi tepat saat `scope = 'provider'` → AC-13, AC-15
- [ ] Buka `crates/storage/tests/fixtures/registry-v5.sql` → baris `artifacts` yang membawa `catalog_id`, `build`, `format`, `size_bytes`, dan dua baris `compatibility_records` (satu per scope); file itu beku dan hash nya ada di `FIXTURE_SHA256` → AC-13, AC-15
- [ ] Buka `crates/platform-macos/src/codesign.rs` → satu satunya program yang dijalankan adalah `/usr/bin/codesign` (literal itu hanya ada di modul ini), lewat `process::spawn::launch` dan `process::kill::signal_group`, stdin `/dev/null`, environment kosong, hanya me reap PID nya sendiri → AC-11, AC-20
- [ ] Buka `crates/core/src/runtime/validate.rs` → langkah exec check memakai `ChildRole::RuntimeCheck`, `CHILD_PATH`, `HOME` dan `TMPDIR` di `scratch/check/`, dan saat batas waktu lewat memanggil `terminate_helper_group(force: true)` tanpa `SIGTERM` → AC-12
- [ ] Buka `crates/core/src/runtime/install.rs` → lima tahap `download`, `verify_digest`, `extract`, `validate_binary`, `commit` (langkah `commit` `irreversible`), input journal tanpa selector sebagai sumber kebenaran, dan `finalize` yang menulis `InsertArtifact` lalu `InsertRuntime` dalam satu batch → AC-1, AC-13, AC-14

### Commands

- [ ] `cargo test -p stackly-domain` → `Runtime::committed` (installed di revision 1 dengan `installed_at` dan `last_verified_at`, runtime external ditolak), `RuntimeSelector` (`stable`, `sha256:` huruf kecil 64 hex, versi dan line, teks tidak sah), `TeamId`, `CompatibilityRecordRow`, dan test pin `FailureCode` `quarantined` dan `binary_rejected_by_os` lulus → AC-1, AC-12, AC-13, AC-15
- [ ] `cargo test -p stackly-storage` → fixture v1 sampai v5 tidak berubah, sidik jari skema versi 5 cocok, migrasi v4 ke v5 menjaga semua baris, conformance `SqliteRegistry` memuat `artifact_install_details` → AC-13, AC-15
- [ ] `cargo test -p stackly-testkit` → conformance `FakeRegistry` memuat skenario yang sama dengan `SqliteRegistry` → AC-13, AC-15
- [ ] `cargo test -p stackly-platform` → `HttpsUrl` menolak `http`, userinfo, host kosong, spasi, dan panjang lebih dari 2048 → AC-5
- [ ] `cargo test -p stackly-platform-macos --test fetch` → server loopback: `Range` dan `If-Range` dikirim lagi di setiap hop, 200 untuk permintaan `Range` mulai dari byte 0, 206 dengan `Content-Range` salah menjadi `LengthMismatch`, 416, redirect ke `http` ditolak, enam redirect ditolak dan lima diikuti, `Content-Encoding: gzip` tidak didekompresi, `Content-Length` salah, byte lebih dari `size`, server diam menjadi `IdleTimeout`, port tertutup menjadi `Unreachable`, validator lemah tidak dipakai → AC-3, AC-5
- [ ] `cargo test -p stackly-platform-macos --test fetch_tls` → sertifikat buatan test ditolak `Tls` oleh trust store macOS → AC-5
- [ ] `cargo test -p stackly-platform-macos --test codesign` → binary sistem `OtherSigned`, salinan tanpa signature menjadi ad hoc lalu valid, satu byte berubah gagal verifikasi tanpa path di `detail`, binary hasil `cc` `LinkerSigned` (dilewati bila tidak ada kompiler), quarantine dibaca dari xattr dan tidak lewat symlink → AC-11
- [ ] `cargo test -p stackly-core --test runtime_download` → cache entry dipakai tanpa jaringan, resume dengan `Range` dari byte terakhir yang di `sync`, 200 untuk `Range` mulai dari 0, validator berganti, tanpa validator, digest salah (`.part` dan entri dihapus), unduhan dilanjutkan yang salah diulang sekali, offline, retry 5xx dan 429, 4xx pindah URL, urutan kode akhir, ruang disk, stop di tengah dan di jeda retry → AC-3, AC-4, AC-5, AC-6
- [ ] `cargo test -p stackly-core --test runtime_validate` → arsitektur x86_64 saja (diagnosis Rosetta), universal diterima, `minos` terlalu baru, library di luar runtime, himpunan dependency, kebijakan signing `keep_or_adhoc` dan `adhoc_pipeline`, quarantine tanpa spawn, exec check (versi cocok, berbeda, tidak cocok pattern, keluar bukan 0, `SIGKILL`, sinyal lain, batas waktu dengan group dibunuh sekali tanpa `SIGTERM`, `EBADARCH`, `EACCES`), mode akhir dan marker → AC-10, AC-11, AC-12
- [ ] `cargo test -p stackly-core --test supervisor_start launch::` → executable ber quarantine ditolak `runtime.quarantined` sebelum spawn, `SIGKILL` sebelum ready menjadi `runtime.binary_rejected_by_os` tanpa budget restart, sinyal lain tetap crash biasa, child yang mati `SIGKILL` saat tertahan terklasifikasi, discard oleh daemon tidak → AC-12
- [ ] `cargo test -p stackly-core --test runtime_resolve` → tabel selector (`stable`, versi, line, ambigu, digest, digest target lain tanpa pengganti, `blocked`, `minOs`) dan aturan `support_status` berurutan → AC-1, AC-10, AC-15
- [ ] `cargo test -p stackly-core --test runtime_commit` → rename bersih, rename yang sudah terjadi, folder yatim (operasi hilang atau gagal) dipindah ke scratch, konflik tanpa marker atau dengan artefak lain atau operasi yang masih berjalan atau runtime terdaftar → AC-13
- [ ] `cargo test -p stackly-core --test runtime_install` → operasi lima tahap lewat engine: sukses satu runtime `installed` dengan artefak, signature, dependency, dan cache tetap; digest salah; offline lalu dari cache; versi berbeda; ad hoc signing; dua install artefak sama menjadi satu runtime; cancel saat unduhan menyisakan `.part`; shutdown lalu start lagi melanjutkan dari `.part`; catalog bergeser membuat runtime kedua dan yang lama tidak berubah → AC-1, AC-2, AC-3, AC-4, AC-6, AC-13, AC-14
- [ ] `cargo test -p stackly-core --test runtime_fixture_install` → `install_cached_artifact` memasang tanpa engine dan jaringan, menulis registry sendiri, dan gagal dengan error pipeline untuk artefak atau digest yang salah → AC-13
- [ ] `cargo test -p stackly-core --lib catalog` → gerbang dengan `&HostFacts` dan `requires_newer_os`, status sel dari `support_status` → AC-10, AC-15
- [ ] `cargo xtask check-deps`, `cargo deny check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → semuanya lulus → AC-5

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
