# Verify: Sumber & packaging runtime · spec 0003 · updated 2026-10-19

_Langkah ini diturunkan dari acceptance criteria spec 0003. `/check verify` menjalankannya; `/test` mengunci yang tahan lama._

## Milestone 1: inventaris dan pemeriksaan

### UI / manual

Tidak ada layar; milestone ini hanya tooling `cargo xtask`.

### Commands

- [x] `cargo xtask runtime check` di repo → exit 0, cetak `0 artifact(s) and 0 evidence run(s) follow the rules` → AC-2 · verify 2026-10-18: sekarang `8 artifact(s) and 7 evidence run(s)`, exit 0; jumlahnya bertambah di milestone berikutnya
- [x] `cargo test -p xtask runtime::` → semua test aturan 1 sampai 7 lulus (satu atau lebih per aturan) → AC-2
- [x] Tambahkan sementara satu entri dengan URL `http://` ke `runtimes/inventory.toml`, jalankan `cargo xtask runtime check` → cetak `<id>: urls must all be https:// URLs, found ... (rule 3)` lalu exit 1; kembalikan file → AC-2
- [x] `cargo xtask runtime pin --url https://github.com/nats-io/nats-server/releases/download/v2.15.1/nats-server-v2.15.1-darwin-arm64.tar.gz --executable nats-server --sha256-url https://github.com/nats-io/nats-server/releases/download/v2.15.1/SHA256SUMS` → cetak `urls`, `sha256` yang sama dengan baris `darwin-arm64.tar.gz` di SHA256SUMS, `size`, dan `min_os = "13.0"`; file `~/Library/Caches/dev.stackly.xtask/runtime/<sha256>` ada → AC-1, value sourcing `pin`
- [x] `pin` yang sama dengan `--sha256-url` milik rilis lain (misalnya v2.15.0) → gagal dengan `the checksum file has no line for nats-server-v2.15.1-darwin-arm64.tar.gz`, tanpa ekstraksi → AC-1, AC-3
- [x] `pin` dengan dua `--url` yang menghasilkan byte berbeda → gagal dengan `the candidate URLs give different digests` → AC-1
- [x] `pin --url http://...` → ditolak sebelum unduhan dengan `url must be an https:// URL` → AC-3
- [x] Tambahkan sementara entri NATS hasil `pin` di atas, jalankan `cargo xtask runtime verify` → `nats-2.15.1-macos-aarch64: OK`; ganti satu huruf `sha256` → `FAIL digest mismatch: ...`; ubah `size` → `FAIL size mismatch: ...`; exit 1 untuk keduanya; kembalikan file → AC-1, AC-3
- [x] Ubah satu byte file cache NATS, lalu jalankan `verify` lagi → peringatan `no longer matches its digest; deleting it`, file diunduh ulang, hasil tetap `OK` → AC-3 (cek ulang cache)
- [ ] Di Linux, `pin` tidak mencetak `min_os` dan memberi catatan `min_os needs vtool`; cache berada di `$XDG_CACHE_HOME/stackly-xtask/runtime/` atau `~/.cache/stackly-xtask/runtime/` → value sourcing cache. Tanpa host Linux langkah ini dicatat `BLOCKED`; path cache Linux tetap dicek di Mac ini oleh unit test `the_cache_folder_follows_the_platform_convention` · verify 2026-10-18: BLOCKED, tidak ada host Linux; unit test `the_cache_folder_follows_the_platform_convention` lulus · verify 2026-10-19: tetap BLOCKED, owner @ojiepermana. Engine Docker Linux aarch64 lokal tidak dipakai atas pilihan kamu (tanpa unduhan toolchain dan crate); kamu menerima unit test `the_cache_folder_follows_the_platform_convention` (lulus lagi 2026-10-19) sebagai cukup untuk path cache. Catatan `pin: min_os needs vtool; run pin on macOS to fill it` dicetak `read_min_os` di `pin.rs` di luar macOS, tanpa run Linux
- [x] Entri dengan `upstream_signature` berjenis `gpg` (pertama kali di MySQL, milestone 2): tanpa `gpg` di PATH hasilnya `SKIP gpg`; dengan `gpg`, signature dicek dengan kunci di `runtimes/keys/` dan fingerprint entri → AC-1 · verify 2026-10-18: tanpa `gpg` terbukti (`SKIP gpg` untuk MySQL 8.4 dan 9.7); jalur dengan `gpg` BLOCKED karena `gpg` tidak terpasang di Mac ini · verify 2026-10-19: GnuPG 2.5.24 dipasang lewat `brew install gnupg` dengan persetujuan kamu; `cargo xtask runtime verify --id mysql-8.4.11-macos-aarch64` dan `--id mysql-9.7.2-macos-aarch64` → masing masing mengunduh `.asc` lalu `OK`, exit 0 (kunci `runtimes/keys/mysql-release-engineering.asc`, fingerprint `BCA43417C3B485DD128EC6D4B7B3B788A8D3785C`). Fingerprint MySQL 8.4 diganti sementara dengan 40 nol (tidak di-commit) → `FAIL gpg signature does not verify with key 0000000000000000000000000000000000000000`, exit 1; file dikembalikan

### Acceptance-criteria coverage

- AC-1 (inventaris, `pin`, `verify` menghasilkan nilai yang sama): langkah pin, verify, dan gpg. Delapan entri itu sendiri baru datang di milestone 2 dan 3.
- AC-2 (`runtime check`, pesan `<id>: <aturan>`): langkah check dan unit test. Aturan 8 dan 9 baru datang di build step 7 dan 12.
- AC-3 (digest dan ukuran dicek sebelum ekstraksi, cache rusak dihapus): langkah sha256 file lain, http, verify mismatch, dan cache rusak. Bagian worker dan hasil spike `FAIL` datang di milestone 2.

## Milestone 2: spike LaunchAgent untuk artefak upstream · 2026-10-17

### UI / manual

- [x] Tidak ada layar. Saat run pertama tiap digest, perhatikan layar Mac: probe quarantine bisa memunculkan dialog Gatekeeper (NATS dan Traefik tercatat `timeout`, MySQL `ran`). Tutup dialognya; tidak ada setelan Gatekeeper yang diubah → AC-8, AC-9 · verify 2026-10-18: layar tidak diamati sesi ini; hasil quarantine tiap digest tercatat di evidence, dan tidak ada setelan yang diubah

### Commands

- [x] `cargo xtask runtime check` → `5 artifact(s) and 2 evidence run(s) follow the rules`, exit 0 → AC-1, AC-2 · verify 2026-10-18: sekarang `8 artifact(s) and 7 evidence run(s)`, exit 0
- [x] `cargo xtask runtime verify` → NATS dan Traefik `OK`; MySQL 8.4 dan 9.7 `SKIP gpg` kalau `gpg` tidak ada di PATH, dan `OK` kalau ada (kunci `runtimes/keys/mysql-release-engineering.asc`, fingerprint `BCA43417C3B485DD128EC6D4B7B3B788A8D3785C`) → AC-1
- [x] `cargo xtask runtime spike --id nats-2.15.1-macos-aarch64 --result <file>` → exit 0; di hasil: `launch.domain` `gui/<uid>`, `parentIsLaunchd` true, `controllingTty` false, `observedVersion` `2.15.1`, `listeners` hanya dua port 127.0.0.1 yang dialokasikan, `stop.signal` null, `closureProbes` `jetstream` PASS → AC-4, AC-5
- [x] Spike MySQL 8.4 → `signature[].kind` `developer_id` (Oracle, team `VB5E2TV963`), `adhocSignedAtInstall` false, `listeners` kosong, probe `dependencies` PASS dengan 9 library; `libraries.warnings` memuat dua `LC_RPATH` absolut di binary debug (peringatan, bukan `FAIL`) → AC-5, AC-6, AC-7 (closure), AC-8
- [x] Ganti satu huruf `sha256` NATS di inventaris (jangan di-commit), jalankan spike → exit 1, `failure.step` `download`, alasan diawali `digest mismatch`, file cache asli tetap ada, tidak ada folder `stackly-runtime-spike-*` tersisa; kembalikan file → AC-3
- [x] Jalankan spike di background, `kill -9` driver setelah sekitar 6 detik (worker masih jalan), lalu `cargo xtask runtime spike --cleanup` → `2 process(es) stopped, agent removed, 1 folder(s) deleted`, exit 0; `launchctl print gui/$(id -u)/dev.stackly.xtask.runtime-spike` tidak menemukan service; `nats-server` milik Homebrew (kalau ada) tetap jalan → AC-4 · verify 2026-10-18: dengan MySQL 9.7 tercatat `1 process(es) stopped, agent removed, 1 folder(s) deleted`; worker ikut berhenti saat agent dicopot, dan sesudahnya tidak ada proses, agent, atau folder spike yang tersisa
- [x] `--cleanup` atau `--prune-cache` saat spike lain berjalan → menunggu lock dan tidak mematikan proses spike; lewat 20 menit keluar dengan `spike in progress` → AC-4 · verify 2026-10-18: `--cleanup` menunggu 11 detik sampai spike MySQL 8.4 selesai (PASS); batas 20 menit dibuktikan unit test `a_held_lock_makes_a_bounded_wait_fail_with_spike_in_progress`
- [x] Shell atau proses lain yang hanya menyebut kata `stackly-runtime-spike-` di command line-nya tidak pernah dimatikan oleh `--cleanup` (pola mencocokkan path root lengkap dan `/tmp/sk-<8 hex>`) → AC-4
- [ ] Jalankan spike lewat SSH tanpa sesi login GUI → exit 2 dengan `no gui login session`, dan test native tercatat `BLOCKED` → AC-4, AC-10 · verify 2026-10-18: BLOCKED, kamu sedang login GUI sehingga `gui/<uid>` selalu ada; butuh logout dari sesi GUI untuk mengujinya · verify 2026-10-19: tetap BLOCKED, owner @ojiepermana. Ini langkah manual berat (logout dari sesi GUI, lalu SSH dari mesin lain), jadi sesuai kebijakan kamu tidak dijalankan; kamu menerima `Verify it` dengan baris ini terbuka
- [x] Di tree bersih: `cargo xtask evidence run --kind native -- cargo test -p xtask --test native_runtime -- --ignored` → NATS, Traefik, MySQL 8.4, MySQL 9.7 `PASS`, ClickHouse `BLOCKED` (alasan dari inventaris); spike berjalan satu per satu walau test paralel → AC-4, AC-10, AC-13 · verify 2026-10-18: run `01a14ebc-ed84-7260-bcfb-68c50e863955`, kedelapan test PASS (ClickHouse PASS sejak milestone 2b)
- [x] `cargo xtask runtime record <report>` → folder baru `evidence/runtime/<runId>/` dengan record `PASS` dan `FAIL` saja, `spike-result.json` per record, dan `output.log` yang disanitasi; jalankan lagi → `already exists` → AC-10
- [x] `runtime record` pada report dari tree kotor → `the run is dirty: ...`; pada report fixture → `the run is a fixture run, not native evidence`; pada report commit lain → `the run is from commit ..., not HEAD ...` → AC-10
- [x] `grep -rE "$(whoami)|$(hostname -s)|/var/folders" evidence/runtime/` → tidak ada hasil → privasi evidence
- [x] Spike kedua untuk digest yang hasil quarantine-nya sudah di-commit → `quarantine.outcome` `skipped` dengan detail folder evidence itu → AC-9
- [x] Driver dan `cargo xtask runtime spike --prune-cache` tidak pernah menghapus `spike.lock` atau file cache yang digest-nya ada di inventaris → AC-4

### Value sourcing

- [x] Port: listener di hasil spike sama dengan port yang dialokasikan dan berbeda di tiap run → port dari `ports.rs`
- [x] Folder socket MySQL `/tmp/sk-<8 hex>` berizin 0700 selama spike dan hilang sesudahnya → folder socket
- [x] Path artefak di hasil memuat `$ROOT/Stackly Spike Ü/` → root ekstraksi dari `TMPDIR`
- [x] `minOsMeasured` sama dengan `min_os` inventaris (NATS 13.0, Traefik 12.0, MySQL 14.0) → `vtool`
- [x] Signature: binary Go `linker_signed`, MySQL `developer_id` → `codesign -dv`
- [ ] Hasil spike lewat batas: worker berhenti sendiri setelah 12 menit dan driver mencatat `FAIL` setelah 15 menit di luar unduhan → batas waktu · verify 2026-10-18: BLOCKED, butuh provider yang menggantung lebih dari 12 menit; konstanta `WORKER_TIMEOUT` 12 menit dan `DRIVER_TIMEOUT` 15 menit ada di `spike/mod.rs` · verify 2026-10-19: tetap BLOCKED, owner @ojiepermana. Worker menolak jalan kalau parent-nya bukan launchd, dan kedua batas adalah konstanta tanpa seam untuk test, jadi tidak bisa dipendekkan. Kamu menerima konstanta itu bersama unit test `a_command_past_its_time_is_killed_with_its_group` (lulus 2026-10-19) sebagai cukup: driver menulis `WORKER_TIMEOUT` ke `plan.worker_timeout_secs` yang dipakai watchdog worker (`timed out after 12 minutes`), dan loop polling driver gagal dengan `the worker did not finish within 15 minutes` setelah `DRIVER_TIMEOUT`

### Acceptance-criteria coverage

- AC-1: lima entri baru (empat `available`, ClickHouse `blocked`), digest dari `pin` dan cocok dengan `verify`. PostgreSQL 17, 18, dan Valkey datang di milestone 3.
- AC-2: aturan 8 aktif di `runtime check`; aturan 9 di build step 12.
- AC-3, AC-4, AC-6, AC-8, AC-9, AC-10: langkah spike, digest salah, kill dan cleanup, tanpa GUI, record, privasi, quarantine.
- AC-5: NATS, Traefik, MySQL 8.4 dan 9.7 `PASS`; PostgreSQL di milestone 3.
- AC-7: bagian closure dicek untuk setiap artefak; probe ICU, pgcrypto, lz4, dan zstd di milestone 3.
- AC-13: ClickHouse `blocked` setelah aturan fallback (26.8 LTS dan 26.9 stable memakai aset self extracting yang sama).
- AC-12: belum di milestone ini.
- AC-11, AC-14, AC-15: dihapus 2026-10-17 (CI ditiadakan, pengembangan fokus di Mac ini dulu).

## Milestone 2b: format `zip` dan ClickHouse · 2026-10-18

### UI / manual

- [x] Tidak ada layar. Run pertama untuk digest ClickHouse menjalankan probe quarantine; dialog Gatekeeper bisa muncul (spike lokal 26.8.30.8 tercatat `ran`, exit 0, tanpa dialog). Kalau muncul, tutup tanpa mengubah setelan → AC-8, AC-9 · verify 2026-10-18: layar tidak diamati sesi ini; quarantine ClickHouse tercatat `ran`

### Commands

- [x] `cargo test -p xtask runtime::` → semua lulus, termasuk `each_format_accepts_only_its_own_kind_of_file`, test zip di `spike::archive` (kasus 1 sampai 5 dan 1b, serta `an_executable_behind_a_link_is_refused_and_its_target_keeps_its_mode`), `a_download_of_another_format_fails_with_the_spike_reason`, `a_zip_download_pins_format_zip_on_every_os`, `min_os_is_read_from_an_executable_inside_a_zip`, dan tiga test aturan 3 untuk `zip` dan `archive_root` → AC-2, AC-16
- [x] `cargo xtask runtime pin --url https://github.com/ClickHouse/ClickHouse/releases/download/v26.8.30.8-lts/clickhouse-macos-aarch64.zip --executable clickhouse` → baris pertama `format = "zip"`, lalu `urls`, `sha256 = "097b41ad97111832cc5ddddd0b514e80307df24f50b5893e9d2035a0c4f47402"`, `size = 216009716`, dan `min_os = "11.0"`, sama dengan entri inventaris → AC-1, AC-16
- [x] `pin --url https://raw.githubusercontent.com/ClickHouse/ClickHouse/v26.8.30.8-lts/LICENSE` → `urls`, `sha256`, dan `size` saja, tanpa `format` dan `min_os`, dengan catatan `pin: the download is not a Mach-O file, a .tar.gz, or a .zip; format and min_os left out` → AC-16
- [x] `/usr/bin/tar -tvf ~/Library/Caches/dev.stackly.xtask/runtime/097b41ad97111832cc5ddddd0b514e80307df24f50b5893e9d2035a0c4f47402` → satu anggota `clickhouse`, file biasa bermode `-rw-rw-r--` (0664), 892587392 byte → build step 16.2
- [x] `cargo xtask runtime verify --id clickhouse-26.8.30.8-macos-aarch64` → `OK` → AC-1
- [x] Ganti sementara `format = "zip"` ClickHouse menjadi `tar_gz` (jangan di-commit): `runtime verify --id clickhouse-26.8.30.8-macos-aarch64` → `FAIL format tar_gz does not match the file (zip)`, exit 1; spike → exit 1, `failure.step` `extract` dengan alasan yang sama, tanpa folder hasil ekstraksi; kembalikan file → AC-16
- [x] `cargo xtask runtime check` → semua artefak dan run evidence lulus aturan, exit 0 → AC-1, AC-2
- [x] Di tree bersih: `cargo xtask evidence run --kind native -- cargo test -p xtask --test native_runtime -- --ignored` → NATS, Traefik, MySQL 8.4, MySQL 9.7, dan ClickHouse `PASS` (tarball milestone 2 tetap lulus dengan cek format baru) → AC-5, AC-10, AC-13
- [x] `spike-result.json` record ClickHouse di folder evidence → `signature[].kind` `developer_id`, `teamId` `ZNDB5FJ8ZW`, `adhocSignedAtInstall` false, `archive.files` 1, detail langkah `extract` `set mode 0755 on clickhouse (was 0644)`, `observedVersion` `26.8.30.8`, `listeners` hanya dua port 127.0.0.1 yang dialokasikan, `stop.exitCode` 0 dalam 60 detik tanpa SIGKILL, probe `dependencies` PASS, `quarantine.outcome` selain `skipped` → AC-5, AC-6, AC-7 (closure), AC-8, AC-9, AC-13, AC-16
- [x] Ekstrak zip ke folder sementara dengan `tar -xf ... --no-mac-metadata`: `codesign -dv --verbose=2` → Authority `Developer ID Application: ClickHouse Inc. (ZNDB5FJ8ZW)`, flag `runtime`; `codesign --verify --strict` lulus sebelum dan sesudah `chmod 0755`; jangan jalankan binary-nya → AC-8, keputusan 2026-10-18
- [x] Spike kedua untuk digest ClickHouse setelah evidence di-commit → `quarantine.outcome` `skipped` dengan folder evidence itu → AC-9
- [x] `grep -rE "$(whoami)|$(hostname -s)|/var/folders" evidence/runtime/` → tidak ada hasil → privasi evidence

### Value sourcing

- [x] `format` entri ClickHouse sama dengan jenis file dari magic number unduhan (`zip`), dicetak `pin` → `format` dari `pin`
- [x] Detail `extract` memuat mode lama di disk setelah `tar` (0644 di bawah umask 022 launchd, walau zip menyimpan 0664) dan mode baru 0755 → mode executable artefak `zip`
- [x] Baris `[[artifact.dependency]]` ClickHouse adalah tujuh baris `system` dari `libraries.static` spike lokal: AppKit, CoreFoundation, Security, SystemConfiguration, `libSystem.B.dylib`, `libiconv.2.dylib`, dan `libobjc.A.dylib` → dependency dari spike lokal
- [x] `executables = ["clickhouse"]` tanpa `archive_root`, karena zip tidak punya folder puncak → `archive_root`, `executables`
- [x] `minOsMeasured` 11.0 sama dengan `min_os` inventaris → `vtool`

### Acceptance-criteria coverage

- AC-1: entri ClickHouse `available` dari `pin`, dan `verify` OK.
- AC-2: aturan 3 menerima `zip` dan membatasi `archive_root`; aturan 8 lulus setelah evidence ClickHouse di-commit.
- AC-3: gerbang digest tetap sebelum cek format dan ekstraksi.
- AC-5, AC-6, AC-8, AC-9, AC-10: record native ClickHouse dan isi `spike-result.json`.
- AC-7: bagian closure untuk ClickHouse; probe PostgreSQL di milestone 3.
- AC-13: ClickHouse 26.8 LTS dari `.zip` (lini LTS terbaru dengan aset macOS aarch64; 26.9 stable sebagai fallback tidak diperlukan).
- AC-16: cek format, ekstraksi zip, `pin`, aturan `archive_root`, dan mode 0755 untuk executable zip.
- AC-12: milestone 3.

## Milestone 3, langkah 17: dukungan xtask untuk artefak maintainer · 2026-10-18

Langkah dengan artefak nyata baru bisa dijalankan setelah sesi B membangun `out/<id>.tar.gz` dan `.sha256`-nya (langkah 9 dan 10); sampai saat itu yang bisa dicek hanya unit test dan perintah tanpa artefak.

### UI / manual

- [x] Tidak ada layar. Spike lokal dengan `--no-quarantine` tidak menjalankan probe quarantine, jadi tidak ada dialog Gatekeeper → AC-9 · verify 2026-10-18: ketiga spike lokal r2 mencatat `quarantine.outcome` `skipped` dengan detail `local run`, jadi probe quarantine tidak jalan

### Commands

- [x] `cargo test -p xtask runtime::` → semua lulus, termasuk `rule_5_needs_a_recipe_that_names_a_repository_commit_and_relative_path`, `rule_5_refuses_an_entry_that_still_carries_attestation_as_invalid_toml`, `an_entry_that_still_carries_attestation_fails_as_an_invalid_inventory`, test `pin_file_*` dan `import_*`, test `the_driver_*`, `a_skipped_quarantine_names_the_local_run_or_where_it_was_recorded`, semua test `spike::closure`, dan test recipe PostgreSQL dan Valkey di `spike::providers` → AC-2, AC-5, AC-9, AC-12, AC-18, AC-19
- [x] Tambahkan sementara `attestation = "https://example.org/a"` ke satu entri (jangan di-commit): `cargo xtask runtime check` → gagal dengan `runtimes/inventory.toml is not a valid inventory` dan ``unknown field `attestation` ``; kembalikan file → AC-2
- [x] `cargo xtask runtime pin --file ~/Development/ojiepermana/stackly-runtimes/out/valkey-9.1.2-r2-macos-aarch64.tar.gz --url https://github.com/stackly-dev/stackly/releases/download/runtime/valkey-9.1.2-r2-macos-aarch64/valkey-9.1.2-r2-macos-aarch64.tar.gz --executable bin/valkey-server` dengan jaringan mati → `format = "tar_gz"`, `urls` persis `--url`, `sha256` sama dengan isi `.sha256`, `size`, dan `min_os = "27.0"`; file ada di cache dengan nama digest-nya dan file asli tetap di `out/` → AC-1, AC-19
- [x] Ulangi dengan `.sha256` dipindah sementara, dengan digest di dalamnya diubah, dengan nama file lain di dalamnya, dan dengan `--url` yang nama file terakhirnya berbeda → masing masing gagal, dan `ls ~/Library/Caches/dev.stackly.xtask/runtime` sebelum dan sesudah sama → AC-19
- [x] `cargo xtask runtime pin --file <tar.gz> --url <https> --sha256-url <https>` → ditolak dengan `--file cannot go with --sha256-url` sebelum apa pun dibaca → AC-19
- [x] `cargo xtask runtime spike --id <id maintainer> --result <file> --no-quarantine` untuk entri lokal di langkah 12.2 → `quarantine.outcome` `skipped` dengan detail `local run`; `closureProbes` memuat `dependencies` lalu `manifest`, keduanya `PASS` → AC-9, AC-18
- [x] Ubah sementara `recipe` entri itu (jangan di-commit) dan ulangi spike lokal → `FAIL` di langkah `closure` dengan `recipe is "...", the entry says "..."` di detail probe `manifest`, dan probe `dependencies` tetap tercatat; kembalikan file → AC-18
- [x] Spike lokal Valkey → `observedVersion` `9.1.2` dari baris `valkey_version` (bukan `redis_version`), `listeners` hanya satu port 127.0.0.1 yang dialokasikan, stop lewat `valkey-cli shutdown nosave` dalam 60 detik → AC-5
- [x] Spike lokal PostgreSQL 18 lalu 17 → probe `icu`, `pgcrypto`, `lz4`, dan `zstd` `PASS`, `observedVersion` `18.6` dan `17.11`, `listeners` kosong (hanya socket di `/tmp/sk-*`), stop karena SIGINT dengan `stop.exitCode` 0 dalam 60 detik → AC-5, AC-7
- [x] `cargo test -p xtask --test native_runtime -- --list --ignored` → delapan test, termasuk `postgresql_17_runs_from_managed_artifact`, `postgresql_18_runs_from_managed_artifact`, dan `valkey_9_1_runs_from_managed_artifact` → AC-10
- [x] Wrapper native sebelum entri maintainer ada → ketiga test baru `BLOCKED` dengan `no inventory entry for postgresql 17`, `no inventory entry for postgresql 18`, dan `no inventory entry for valkey 9.1`, owner `@ojiepermana`; record itu tidak disalin `runtime record` → AC-10
- [x] `cargo xtask evidence check` → semua file Rust lulus aturan makro evidence → AC-10
- [x] `cargo xtask runtime check` → exit 0 (aturan 9 belum aktif) → AC-2 · verify 2026-10-18: exit 0, dan aturan 9 sekarang aktif

### Value sourcing

- [x] `sha256`, `size`, `format`, dan `min_os` dari `pin --file` sama dengan `shasum -a 256`, `stat -f %z`, magic number gzip, dan `vtool -show-build` pada `bin/valkey-server` di dalam tarball → pin `--file`
- [x] `urls` dari `pin --file` persis nilai `--url` (`.../releases/download/runtime/<id>/<id>.tar.gz`) → `urls` pin `--file`
- [x] Probe `manifest` membaca `STACKLY-ARTIFACT.json` dan `LICENSES/` di root artefak, lalu jumlah symlink, setuid, dan file khusus dari laporan archive: detail `PASS` spike lokal menyebut jumlah teks lisensi (Valkey 1 ditambah komponen vendored, PostgreSQL 5), dan unit test `a_missing_license_text_is_named` serta `an_archive_with_a_link_setuid_or_special_file_fails` membuktikan sisi gagalnya → probe `manifest`
- [x] `build`, `source_url`, `source_sha256`, dan `recipe` entri sama dengan `build`, `source.url`, `source.sha256`, dan `recipe` di `STACKLY-ARTIFACT.json` → entri maintainer
- [x] `executables` entri PostgreSQL `bin/postgres`, `bin/initdb`, `bin/psql`, `bin/pg_dump`, dan Valkey `bin/valkey-server`, `bin/valkey-cli` → entri maintainer, `executables`
- [x] `logTail` dan detail spike PostgreSQL memakai user database `stackly`, tidak pernah nama user OS → user database PostgreSQL
- [x] `quarantine.detail` `local run` hanya pada run dengan `--no-quarantine`; spike tanpa flag tetap mencari hasil quarantine di `evidence/runtime/` → flag quarantine, `quarantine.outcome`

### Acceptance-criteria coverage

- AC-2: aturan 5 baru (pola `recipe`, field `attestation` ditolak sebagai TOML tidak valid).
- AC-5, AC-7: recipe `Postgres` dan `Valkey`; `PASS` nyata menunggu artefak di langkah 12.
- AC-9: `--no-quarantine` mencatat `skipped` dengan detail `local run`; test native tidak memakainya.
- AC-10: tiga test native baru, `BLOCKED` sampai entrinya ada.
- AC-12, AC-18: probe `manifest` dan cek isi archive di langkah `closure`.
- AC-19: `pin --file` tanpa jaringan; urutan terbit dicek di langkah 12.

## Milestone 3, langkah 9 dan 10: resep maintainer di `stackly-dev/runtimes` · 2026-10-19

Langkah ini dijalankan di checkout lokal `~/Development/ojiepermana/stackly-runtimes` (commit `8d22ef98bab9f485d26578ceed2ed5b575f2a187`, sama dengan `origin/main`), bukan di repo ini. Log build tetap lokal di `out/` dan tidak dikutip utuh di sini, karena memuat path home.

### UI / manual

- [x] Tidak ada layar; langkah ini hanya `tests/check.sh`, `build.sh`, dan log build di repo `stackly-runtimes` → AC-17

### Commands

- [x] `bash tests/check.sh` di checkout `stackly-runtimes` → `75 passed, 0 failed`, exit 0: preflight `/usr/local` palsu, scan log `/opt/homebrew`, scan path home, audit Mach-O fixture, dan scan notice dengan kasus lulus dan gagal di Critical test scenarios; tree tetap bersih sesudahnya → AC-17, AC-12 · verify 2026-10-19: `75 passed, 0 failed`, exit 0, `git status --porcelain` kosong
- [x] Buat sementara satu file untracked di checkout itu (jangan di-commit), jalankan `./build.sh valkey 9.1.2 3` → mencetak baris `git status --porcelain` lalu `error: stackly-runtimes has uncommitted changes`, exit 1, sebelum unduhan apa pun: `out/sources/` tetap tujuh file, tidak ada `out/*-r3-*`, dan tidak ada root build baru di `/private/tmp/stackly-build/`; hapus file itu, lalu `git status --porcelain` kosong lagi → AC-17 · verify 2026-10-19: sesuai, dengan persetujuan kamu untuk mengotori tree sebentar; file `verify-dirty-probe.txt` dihapus dan tree bersih lagi
- [x] Blok `==> environment` di awal `out/<id>.log` ketiga build r2 (`valkey-9.1.2-r2`, `postgresql-18.6-r2`, `postgresql-17.11-r2`) → keluaran `/usr/bin/env` utuh (hanya `_=` yang dibuang `build.sh`) berisi tepat 13 variabel tabel Environment build: `DEVELOPER_DIR`, `HOME` (`<build>/home`), `LANG` (`C`), `LDFLAGS` (`-Wl,-headerpad_max_install_names`), `MACOSX_DEPLOYMENT_TARGET` (`27.0`), `PATH` (`<build>/venv/bin:/usr/bin:/bin:/usr/sbin:/sbin`), `PKG_CONFIG_LIBDIR` (`<build>/deps/lib/pkgconfig:<build>/sdk-pkgconfig`), `SDKROOT`, `STACKLY_BUILD_ARGS`, `STACKLY_BUILD_ROOT`, `STACKLY_ORIGINAL_HOME`, `STACKLY_RECIPE` (`stackly-dev/runtimes@8d22ef98bab9f485d26578ceed2ed5b575f2a187:recipes/<provider>.sh`), dan `TMPDIR` (`<build>/tmp`); tidak ada `DYLD_*`, `CPATH`, `LIBRARY_PATH`, `PKG_CONFIG_PATH`, `/opt/homebrew`, atau `/usr/local` → AC-17 · verify 2026-10-19: sesuai untuk ketiganya
- [x] Header langkah `==>` di ketiga log r2 → urutan alur `build.sh`: environment, venv tool, toolchain, dependency OpenSSL 3.5.9, ICU 78.3, lz4 1.10.0, dan zstd 1.5.7 (PostgreSQL saja), source, scan notice, build, scan log konfigurasi untuk path Homebrew, folder paket, relokasi dan tanda tangan, audit, manifest, scan path home, pack, cek versi dari salinan paket, lalu `results` dengan `<id>.tar.gz: OK` → AC-17, AC-12 · verify 2026-10-19: sesuai untuk ketiganya
- [x] Cek versi di log r2 → dari salinan di `<build>/check Stackly Ü/`: `bin/postgres`, `bin/initdb`, `bin/psql`, dan `bin/pg_dump` mencetak `18.6` dan `17.11`, dengan `pg_config --sharedir` dan `--pkglibdir` di bawah folder salinan itu; `bin/valkey-server` mencetak `v=9.1.2` dan `bin/valkey-cli` `9.1.2` → AC-17 (cek versi), build step 9 dan 10 · verify 2026-10-19: sesuai
- [x] `shasum -a 256` setiap file di `out/sources/` → sama dengan nama filenya, dan setiap nama itu adalah sha256 yang dipin di `recipes/` → AC-17 (source dicocokkan dengan sha256 yang dipin) · verify 2026-10-19: tujuh file, ketujuhnya cocok

### Acceptance-criteria coverage

- AC-17: `tests/check.sh` (fungsi cek dengan folder sementara), tree kotor ditolak sebelum unduhan, environment build di log, alur langkah, cek versi dari path berisi spasi dan `Ü`, dan source yang cocok dengan sha256 yang dipin. Sisi gagal untuk `/opt/homebrew` di log, path home di paket, dan audit dibuktikan fixture `tests/check.sh`, bukan build nyata yang gagal.
- AC-12: scan notice dan `LICENSES/` dicek di sisi build lewat `tests/check.sh`, dan di artefak nyata di langkah 12.
- AC-7 (closure di sisi build): dependency PostgreSQL dibangun dari source di log; probe closure di langkah 12.

## Milestone 3, langkah 12: PostgreSQL 17 dan 18 serta Valkey terbit dan terbukti · 2026-10-18

Artefak yang terbit: `valkey-9.1.2-r2-macos-aarch64`, `postgresql-18.6-r2-macos-aarch64`, dan `postgresql-17.11-r2-macos-aarch64`, dari commit resep `stackly-dev/runtimes@8d22ef98bab9f485d26578ceed2ed5b575f2a187`. Build r1 tidak pernah diunggah (gerbang lisensi Valkey menolaknya, lihat Notice pihak ketiga di spec).

### UI / manual

- [x] Follow-up spec 0003: kedua checkbox gerbang lisensi (Valkey, PostgreSQL) dicentang dengan tanggal 2026-10-18, `@ojiepermana`, id dan sha256 artefak r2 → langkah 12.3, AC-12
- [x] Halaman release `https://github.com/stackly-dev/stackly/releases/tag/runtime/<id>` untuk ketiga id: judul sama dengan id, berstatus prerelease dan bukan release "Latest" (amandemen spec 2026-10-19), dua aset (`<id>.tar.gz` dan `<id>.tar.gz.sha256`), catatan berisi source, `recipe`, toolchain, dependency atau komponen vendored, dan lisensi, tanpa path lokal → API surface, AC-19 · verify 2026-10-18: GAGAL untuk satu hal: GitHub memberi label `Latest` pada release Valkey (release pertama repo itu) walau dibuat dengan `--latest=false`; judul, aset, dan catatan ketiga release sesuai · verify 2026-10-19: spec 0003 diamandemen lewat `/architect` (release runtime selalu prerelease), lalu `gh release edit runtime/<id> --repo stackly-dev/stackly --prerelease` untuk ketiga id dengan persetujuan kamu, exit 0. Sesudahnya `gh release list` → ketiganya `Pre-release`; halaman release ketiganya memuat label `Pre-release` tanpa `Latest`, dan halaman Releases tanpa `Latest`; `gh api repos/stackly-dev/stackly/releases/latest` → 404; tag, judul, dan aset (nama, ukuran, digest) sama persis dengan sebelum edit; URL unduhan `<id>.tar.gz.sha256` tetap 200
- [x] Tidak ada layar lain. Run native pertama untuk digest baru menjalankan probe quarantine dan dialog Gatekeeper bisa muncul; kalau muncul, tutup tanpa mengubah setelan → AC-9 · verify 2026-10-18: layar tidak diamati sesi ini; quarantine tercatat `timeout` (Valkey) dan `killed` (PostgreSQL)

### Commands

- [x] `git -C ~/Development/ojiepermana/stackly-runtimes branch -r --contains 8d22ef98bab9f485d26578ceed2ed5b575f2a187` → `origin/main` → langkah 12.3a
- [x] `cargo xtask runtime pin --file ~/Development/ojiepermana/stackly-runtimes/out/valkey-9.1.2-r2-macos-aarch64.tar.gz --url https://github.com/stackly-dev/stackly/releases/download/runtime/valkey-9.1.2-r2-macos-aarch64/valkey-9.1.2-r2-macos-aarch64.tar.gz --executable bin/valkey-server` dengan jaringan mati → `format = "tar_gz"`, `urls` persis `--url`, `sha256 = "fef1d702145d599856286b3a6dc6a7d6a102d15f094dc34ead868743f3956370"`, `size = 4581661`, `min_os = "27.0"`, sama dengan entri → AC-19
- [x] Sama untuk PostgreSQL dengan `--executable bin/postgres` → 18.6: `sha256 = "88bb7533e639e852e5258c293aa42f0abf40aed42d18f0f714cb47a9354b6590"`, `size = 28311215`; 17.11: `sha256 = "04b1eff117441c0b6f1cbefa42ded6cf5726d656e54881674defea04b515d6d3"`, `size = 27984415`; keduanya `min_os = "27.0"` → AC-19
- [x] `cargo xtask runtime verify --id valkey-9.1.2-r2-macos-aarch64`, lalu `--id postgresql-18.6-r2-macos-aarch64` dan `--id postgresql-17.11-r2-macos-aarch64` → masing masing mengunduh dari URL release dan mencetak `OK` → AC-1
- [x] `gh release view runtime/<id> --repo stackly-dev/stackly --json assets --jq '.assets[]|{name,size}'` untuk ketiga id → `<id>.tar.gz` dengan ukuran sama dengan `size` entri, dan `<id>.tar.gz.sha256` → AC-1, AC-19
- [x] `gh api repos/stackly-dev/runtimes/commits/8d22ef98bab9f485d26578ceed2ed5b575f2a187 --jq .sha` → sha itu, jadi rujukan `recipe` di artefak bisa dibuka publik → AC-18, langkah 12.3a
- [x] `cargo test -p xtask runtime::` → semua lulus, termasuk `rule_9_accepts_exactly_one_entry_per_required_line`, `rule_9_names_a_required_line_without_an_entry`, `rule_9_takes_any_clickhouse_line_but_only_one`, `rule_9_counts_a_blocked_entry_and_ignores_lines_it_does_not_require`, dan `an_inventory_without_the_required_lines_fails_rule_9` → AC-2
- [x] `cargo xtask runtime check` → `runtime check: 8 artifact(s) and 7 evidence run(s) follow the rules`, exit 0, dengan aturan 9 aktif → AC-1, AC-2
- [x] Hapus sementara entri Valkey dari `runtimes/inventory.toml` (jangan di-commit): `cargo xtask runtime check` → `inventory: the required line valkey 9.1 has no entry (rule 9)`, exit 1; kembalikan file → AC-2
- [x] Di tree bersih: `cargo xtask evidence run --kind native -- cargo test -p xtask --test native_runtime -- --ignored` → delapan test `PASS` (NATS, Traefik, ClickHouse, MySQL 8.4 dan 9.7, PostgreSQL 17 dan 18, Valkey), dan `quarantine.outcome` `skipped` untuk ketiga digest maintainer dengan folder evidence yang sudah memuat hasilnya → AC-5, AC-9, AC-10
- [x] `spike-result.json` di `evidence/runtime/01a14eae-ccfa-72c5-9740-dd77ca4e4a55/` (Valkey) → `observedVersion` `9.1.2`, `listeners` hanya satu port 127.0.0.1, `libraries.static` hanya `/usr/lib/libSystem.B.dylib`, probe `dependencies` dan `manifest` PASS (detail `7 license texts`), `quarantine.outcome` `timeout`, `stop.exitCode` 0 → AC-5, AC-9, AC-18
- [x] `spike-result.json` di `evidence/runtime/01a14eb1-694f-7400-b05e-71b9a321b96b/` (PostgreSQL 18) dan `evidence/runtime/01a14eb2-ecac-77c2-9c09-d03c0f2d367d/` (PostgreSQL 17) → `observedVersion` `18.6` dan `17.11`, `listeners` kosong, probe `dependencies` (`10 non system libraries, as listed`), `manifest` (`5 license texts`), `icu` (`datlocprovider` `i`), `pgcrypto` (16), `lz4`, dan `zstd` PASS, `quarantine.outcome` `killed`, `stop.exitCode` 0 → AC-5, AC-7, AC-9, AC-18
- [x] `tar -tzf ~/Library/Caches/dev.stackly.xtask/runtime/<sha256> | grep LICENSES/` → Valkey tujuh file (`valkey`, `lua`, `libvalkey`, `linenoise`, `hdr_histogram`, `fpconv`, `fast_float`); PostgreSQL lima file (`postgresql`, `openssl`, `icu`, `lz4`, `zstd`) → AC-12
- [x] `tar -xOzf <cache file> <id>/LICENSES/<provider>.txt | grep -c "^Third party notices"` → 1 untuk ketiganya; daftar path di bawahnya: Valkey 41 file di `src/`, PostgreSQL 83 file di `src/` dan `contrib/` → AC-12, Notice pihak ketiga
- [x] `archive` di ketiga `spike-result.json` → `symlinksInternal` 0, `setuid` dan `specialFiles` kosong → AC-12
- [x] `grep -rE "$(whoami)|$(hostname -s)|/var/folders" evidence/runtime/` → tidak ada hasil → privasi evidence

### Value sourcing

- [x] `urls` setiap entri maintainer persis nilai `--url` `pin --file`; `sha256`, `size`, dan `min_os` dari keluaran `pin --file` → pin `--file`
- [x] `build` (2), `source_url`, `source_sha256`, dan `recipe` entri sama dengan `STACKLY-ARTIFACT.json` di artefak → entri maintainer
- [x] `executables`: PostgreSQL `bin/postgres`, `bin/initdb`, `bin/psql`, `bin/pg_dump`; Valkey `bin/valkey-server`, `bin/valkey-cli` → entri maintainer
- [x] Baris dependency PostgreSQL dari `libraries.static` spike lokal pertama: lima `system` (`libSystem.B`, `libc++.1`, `libedit.3`, `libxml2.2`, `libz.1`), tujuh `bundled` dengan `version`, `license`, dan `source_url` dari `dependencies[]` manifest (OpenSSL 3.5.9, ICU 78.3, lz4 1.10.0, zstd 1.5.7), dan tiga `own` (`libecpg.6`, `libpgtypes.3`, `libpq.5`); `libecpg_compat.3.dylib` tidak punya baris karena tidak dimuat Mach-O mana pun → `[[artifact.dependency]]`
- [x] `license_url`: PostgreSQL `COPYRIGHT` di tag `REL_18_6` dan `REL_17_11` mirror `postgres/postgres`; Valkey `COPYING` di tag `9.1.2` `valkey-io/valkey` → `license`, `license_url`
- [x] Release: tag `runtime/<id>`, judul `<id>`, catatan dari `STACKLY-ARTIFACT.json` → release

### Acceptance-criteria coverage

- AC-1: ketiga entri maintainer `available`, dan `runtime verify` OK dari URL release.
- AC-2: aturan 9 aktif dengan unit test-nya; aturan 8 lulus untuk ketiga digest.
- AC-5, AC-10: record native PASS untuk PostgreSQL 17, 18, dan Valkey 9.1 dari LaunchAgent.
- AC-7: probe ICU, pgcrypto, lz4, dan zstd dari artefak PostgreSQL yang terbit.
- AC-9: probe quarantine tercatat sekali per digest (`timeout` untuk Valkey, `killed` untuk PostgreSQL).
- AC-12: `LICENSES/` dengan notice pihak ketiga, archive hanya file biasa dan folder.
- AC-18: probe `manifest` PASS, dan commit `recipe` ada di repo publik.
- AC-19: urutan `pin --file`, spike lokal, gerbang lisensi, commit resep di remote, persetujuan unggahan, unggah, `verify`, commit entri, lalu evidence.
- AC-17: sisi build ada di repo `stackly-dev/runtimes`; diverifikasi di bagian Milestone 3, langkah 9 dan 10 di atas.
- AC-19 (prerelease): ketiga release runtime berstatus prerelease sejak 2026-10-19, jadi tidak satu pun berlabel Latest.
