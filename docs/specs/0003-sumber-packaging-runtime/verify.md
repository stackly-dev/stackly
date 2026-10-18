# Verify: Sumber & packaging runtime · spec 0003 · updated 2026-10-18

_Langkah ini diturunkan dari acceptance criteria spec 0003. `/check verify` menjalankannya; `/test` mengunci yang tahan lama._

## Milestone 1: inventaris dan pemeriksaan

### UI / manual

Tidak ada layar; milestone ini hanya tooling `cargo xtask`.

### Commands

- [ ] `cargo xtask runtime check` di repo → exit 0, cetak `0 artifact(s) and 0 evidence run(s) follow the rules` → AC-2
- [ ] `cargo test -p xtask runtime::` → semua test aturan 1 sampai 7 lulus (satu atau lebih per aturan) → AC-2
- [ ] Tambahkan sementara satu entri dengan URL `http://` ke `runtimes/inventory.toml`, jalankan `cargo xtask runtime check` → cetak `<id>: urls must all be https:// URLs, found ... (rule 3)` lalu exit 1; kembalikan file → AC-2
- [ ] `cargo xtask runtime pin --url https://github.com/nats-io/nats-server/releases/download/v2.15.1/nats-server-v2.15.1-darwin-arm64.tar.gz --executable nats-server --sha256-url https://github.com/nats-io/nats-server/releases/download/v2.15.1/SHA256SUMS` → cetak `urls`, `sha256` yang sama dengan baris `darwin-arm64.tar.gz` di SHA256SUMS, `size`, dan `min_os = "13.0"`; file `~/Library/Caches/dev.stackly.xtask/runtime/<sha256>` ada → AC-1, value sourcing `pin`
- [ ] `pin` yang sama dengan `--sha256-url` milik rilis lain (misalnya v2.15.0) → gagal dengan `the checksum file has no line for nats-server-v2.15.1-darwin-arm64.tar.gz`, tanpa ekstraksi → AC-1, AC-3
- [ ] `pin` dengan dua `--url` yang menghasilkan byte berbeda → gagal dengan `the candidate URLs give different digests` → AC-1
- [ ] `pin --url http://...` → ditolak sebelum unduhan dengan `url must be an https:// URL` → AC-3
- [ ] Tambahkan sementara entri NATS hasil `pin` di atas, jalankan `cargo xtask runtime verify` → `nats-2.15.1-macos-aarch64: OK`; ganti satu huruf `sha256` → `FAIL digest mismatch: ...`; ubah `size` → `FAIL size mismatch: ...`; exit 1 untuk keduanya; kembalikan file → AC-1, AC-3
- [ ] Ubah satu byte file cache NATS, lalu jalankan `verify` lagi → peringatan `no longer matches its digest; deleting it`, file diunduh ulang, hasil tetap `OK` → AC-3 (cek ulang cache)
- [ ] Di Linux, `pin` tidak mencetak `min_os` dan memberi catatan `min_os needs vtool`; cache berada di `$XDG_CACHE_HOME/stackly-xtask/runtime/` atau `~/.cache/stackly-xtask/runtime/` → value sourcing cache. Tanpa host Linux langkah ini dicatat `BLOCKED`; path cache Linux tetap dicek di Mac ini oleh unit test `the_cache_folder_follows_the_platform_convention`
- [ ] Entri dengan `upstream_signature` berjenis `gpg` (pertama kali di MySQL, milestone 2): tanpa `gpg` di PATH hasilnya `SKIP gpg`; dengan `gpg`, signature dicek dengan kunci di `runtimes/keys/` dan fingerprint entri → AC-1

### Acceptance-criteria coverage

- AC-1 (inventaris, `pin`, `verify` menghasilkan nilai yang sama): langkah pin, verify, dan gpg. Delapan entri itu sendiri baru datang di milestone 2 dan 3.
- AC-2 (`runtime check`, pesan `<id>: <aturan>`): langkah check dan unit test. Aturan 8 dan 9 baru datang di build step 7 dan 12.
- AC-3 (digest dan ukuran dicek sebelum ekstraksi, cache rusak dihapus): langkah sha256 file lain, http, verify mismatch, dan cache rusak. Bagian worker dan hasil spike `FAIL` datang di milestone 2.

## Milestone 2: spike LaunchAgent untuk artefak upstream · 2026-10-17

### UI / manual

- [ ] Tidak ada layar. Saat run pertama tiap digest, perhatikan layar Mac: probe quarantine bisa memunculkan dialog Gatekeeper (NATS dan Traefik tercatat `timeout`, MySQL `ran`). Tutup dialognya; tidak ada setelan Gatekeeper yang diubah → AC-8, AC-9

### Commands

- [ ] `cargo xtask runtime check` → `5 artifact(s) and 2 evidence run(s) follow the rules`, exit 0 → AC-1, AC-2
- [ ] `cargo xtask runtime verify` → NATS dan Traefik `OK`; MySQL 8.4 dan 9.7 `SKIP gpg` kalau `gpg` tidak ada di PATH, dan `OK` kalau ada (kunci `runtimes/keys/mysql-release-engineering.asc`, fingerprint `BCA43417C3B485DD128EC6D4B7B3B788A8D3785C`) → AC-1
- [ ] `cargo xtask runtime spike --id nats-2.15.1-macos-aarch64 --result <file>` → exit 0; di hasil: `launch.domain` `gui/<uid>`, `parentIsLaunchd` true, `controllingTty` false, `observedVersion` `2.15.1`, `listeners` hanya dua port 127.0.0.1 yang dialokasikan, `stop.signal` null, `closureProbes` `jetstream` PASS → AC-4, AC-5
- [ ] Spike MySQL 8.4 → `signature[].kind` `developer_id` (Oracle, team `VB5E2TV963`), `adhocSignedAtInstall` false, `listeners` kosong, probe `dependencies` PASS dengan 9 library; `libraries.warnings` memuat dua `LC_RPATH` absolut di binary debug (peringatan, bukan `FAIL`) → AC-5, AC-6, AC-7 (closure), AC-8
- [ ] Ganti satu huruf `sha256` NATS di inventaris (jangan di-commit), jalankan spike → exit 1, `failure.step` `download`, alasan diawali `digest mismatch`, file cache asli tetap ada, tidak ada folder `stackly-runtime-spike-*` tersisa; kembalikan file → AC-3
- [ ] Jalankan spike di background, `kill -9` driver setelah sekitar 6 detik (worker masih jalan), lalu `cargo xtask runtime spike --cleanup` → `2 process(es) stopped, agent removed, 1 folder(s) deleted`, exit 0; `launchctl print gui/$(id -u)/dev.stackly.xtask.runtime-spike` tidak menemukan service; `nats-server` milik Homebrew (kalau ada) tetap jalan → AC-4
- [ ] `--cleanup` atau `--prune-cache` saat spike lain berjalan → menunggu lock dan tidak mematikan proses spike; lewat 20 menit keluar dengan `spike in progress` → AC-4
- [ ] Shell atau proses lain yang hanya menyebut kata `stackly-runtime-spike-` di command line-nya tidak pernah dimatikan oleh `--cleanup` (pola mencocokkan path root lengkap dan `/tmp/sk-<8 hex>`) → AC-4
- [ ] Jalankan spike lewat SSH tanpa sesi login GUI → exit 2 dengan `no gui login session`, dan test native tercatat `BLOCKED` → AC-4, AC-10
- [ ] Di tree bersih: `cargo xtask evidence run --kind native -- cargo test -p xtask --test native_runtime -- --ignored` → NATS, Traefik, MySQL 8.4, MySQL 9.7 `PASS`, ClickHouse `BLOCKED` (alasan dari inventaris); spike berjalan satu per satu walau test paralel → AC-4, AC-10, AC-13
- [ ] `cargo xtask runtime record <report>` → folder baru `evidence/runtime/<runId>/` dengan record `PASS` dan `FAIL` saja, `spike-result.json` per record, dan `output.log` yang disanitasi; jalankan lagi → `already exists` → AC-10
- [ ] `runtime record` pada report dari tree kotor → `the run is dirty: ...`; pada report fixture → `the run is a fixture run, not native evidence`; pada report commit lain → `the run is from commit ..., not HEAD ...` → AC-10
- [ ] `grep -rE "$(whoami)|$(hostname -s)|/var/folders" evidence/runtime/` → tidak ada hasil → privasi evidence
- [ ] Spike kedua untuk digest yang hasil quarantine-nya sudah di-commit → `quarantine.outcome` `skipped` dengan detail folder evidence itu → AC-9
- [ ] Driver dan `cargo xtask runtime spike --prune-cache` tidak pernah menghapus `spike.lock` atau file cache yang digest-nya ada di inventaris → AC-4

### Value sourcing

- [ ] Port: listener di hasil spike sama dengan port yang dialokasikan dan berbeda di tiap run → port dari `ports.rs`
- [ ] Folder socket MySQL `/tmp/sk-<8 hex>` berizin 0700 selama spike dan hilang sesudahnya → folder socket
- [ ] Path artefak di hasil memuat `$ROOT/Stackly Spike Ü/` → root ekstraksi dari `TMPDIR`
- [ ] `minOsMeasured` sama dengan `min_os` inventaris (NATS 13.0, Traefik 12.0, MySQL 14.0) → `vtool`
- [ ] Signature: binary Go `linker_signed`, MySQL `developer_id` → `codesign -dv`
- [ ] Hasil spike lewat batas: worker berhenti sendiri setelah 12 menit dan driver mencatat `FAIL` setelah 15 menit di luar unduhan → batas waktu

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

- [ ] Tidak ada layar. Run pertama untuk digest ClickHouse menjalankan probe quarantine; dialog Gatekeeper bisa muncul (spike lokal 26.8.30.8 tercatat `ran`, exit 0, tanpa dialog). Kalau muncul, tutup tanpa mengubah setelan → AC-8, AC-9

### Commands

- [ ] `cargo test -p xtask runtime::` → semua lulus, termasuk `each_format_accepts_only_its_own_kind_of_file`, test zip di `spike::archive` (kasus 1 sampai 5 dan 1b, serta `an_executable_behind_a_link_is_refused_and_its_target_keeps_its_mode`), `a_download_of_another_format_fails_with_the_spike_reason`, `a_zip_download_pins_format_zip_on_every_os`, `min_os_is_read_from_an_executable_inside_a_zip`, dan tiga test aturan 3 untuk `zip` dan `archive_root` → AC-2, AC-16
- [ ] `cargo xtask runtime pin --url https://github.com/ClickHouse/ClickHouse/releases/download/v26.8.30.8-lts/clickhouse-macos-aarch64.zip --executable clickhouse` → baris pertama `format = "zip"`, lalu `urls`, `sha256 = "097b41ad97111832cc5ddddd0b514e80307df24f50b5893e9d2035a0c4f47402"`, `size = 216009716`, dan `min_os = "11.0"`, sama dengan entri inventaris → AC-1, AC-16
- [ ] `pin --url https://raw.githubusercontent.com/ClickHouse/ClickHouse/v26.8.30.8-lts/LICENSE` → `urls`, `sha256`, dan `size` saja, tanpa `format` dan `min_os`, dengan catatan `pin: the download is not a Mach-O file, a .tar.gz, or a .zip; format and min_os left out` → AC-16
- [ ] `/usr/bin/tar -tvf ~/Library/Caches/dev.stackly.xtask/runtime/097b41ad97111832cc5ddddd0b514e80307df24f50b5893e9d2035a0c4f47402` → satu anggota `clickhouse`, file biasa bermode `-rw-rw-r--` (0664), 892587392 byte → build step 16.2
- [ ] `cargo xtask runtime verify --id clickhouse-26.8.30.8-macos-aarch64` → `OK` → AC-1
- [ ] Ganti sementara `format = "zip"` ClickHouse menjadi `tar_gz` (jangan di-commit): `runtime verify --id clickhouse-26.8.30.8-macos-aarch64` → `FAIL format tar_gz does not match the file (zip)`, exit 1; spike → exit 1, `failure.step` `extract` dengan alasan yang sama, tanpa folder hasil ekstraksi; kembalikan file → AC-16
- [ ] `cargo xtask runtime check` → semua artefak dan run evidence lulus aturan, exit 0 → AC-1, AC-2
- [ ] Di tree bersih: `cargo xtask evidence run --kind native -- cargo test -p xtask --test native_runtime -- --ignored` → NATS, Traefik, MySQL 8.4, MySQL 9.7, dan ClickHouse `PASS` (tarball milestone 2 tetap lulus dengan cek format baru) → AC-5, AC-10, AC-13
- [ ] `spike-result.json` record ClickHouse di folder evidence → `signature[].kind` `developer_id`, `teamId` `ZNDB5FJ8ZW`, `adhocSignedAtInstall` false, `archive.files` 1, detail langkah `extract` `set mode 0755 on clickhouse (was 0644)`, `observedVersion` `26.8.30.8`, `listeners` hanya dua port 127.0.0.1 yang dialokasikan, `stop.exitCode` 0 dalam 60 detik tanpa SIGKILL, probe `dependencies` PASS, `quarantine.outcome` selain `skipped` → AC-5, AC-6, AC-7 (closure), AC-8, AC-9, AC-13, AC-16
- [ ] Ekstrak zip ke folder sementara dengan `tar -xf ... --no-mac-metadata`: `codesign -dv --verbose=2` → Authority `Developer ID Application: ClickHouse Inc. (ZNDB5FJ8ZW)`, flag `runtime`; `codesign --verify --strict` lulus sebelum dan sesudah `chmod 0755`; jangan jalankan binary-nya → AC-8, keputusan 2026-10-18
- [ ] Spike kedua untuk digest ClickHouse setelah evidence di-commit → `quarantine.outcome` `skipped` dengan folder evidence itu → AC-9
- [ ] `grep -rE "$(whoami)|$(hostname -s)|/var/folders" evidence/runtime/` → tidak ada hasil → privasi evidence

### Value sourcing

- [ ] `format` entri ClickHouse sama dengan jenis file dari magic number unduhan (`zip`), dicetak `pin` → `format` dari `pin`
- [ ] Detail `extract` memuat mode lama di disk setelah `tar` (0644 di bawah umask 022 launchd, walau zip menyimpan 0664) dan mode baru 0755 → mode executable artefak `zip`
- [ ] Baris `[[artifact.dependency]]` ClickHouse adalah tujuh baris `system` dari `libraries.static` spike lokal: AppKit, CoreFoundation, Security, SystemConfiguration, `libSystem.B.dylib`, `libiconv.2.dylib`, dan `libobjc.A.dylib` → dependency dari spike lokal
- [ ] `executables = ["clickhouse"]` tanpa `archive_root`, karena zip tidak punya folder puncak → `archive_root`, `executables`
- [ ] `minOsMeasured` 11.0 sama dengan `min_os` inventaris → `vtool`

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

- [ ] Tidak ada layar. Spike lokal dengan `--no-quarantine` tidak menjalankan probe quarantine, jadi tidak ada dialog Gatekeeper → AC-9

### Commands

- [ ] `cargo test -p xtask runtime::` → semua lulus, termasuk `rule_5_needs_a_recipe_that_names_a_repository_commit_and_relative_path`, `rule_5_refuses_an_entry_that_still_carries_attestation_as_invalid_toml`, `an_entry_that_still_carries_attestation_fails_as_an_invalid_inventory`, test `pin_file_*` dan `import_*`, test `the_driver_*`, `a_skipped_quarantine_names_the_local_run_or_where_it_was_recorded`, semua test `spike::closure`, dan test recipe PostgreSQL dan Valkey di `spike::providers` → AC-2, AC-5, AC-9, AC-12, AC-18, AC-19
- [ ] Tambahkan sementara `attestation = "https://example.org/a"` ke satu entri (jangan di-commit): `cargo xtask runtime check` → gagal dengan `runtimes/inventory.toml is not a valid inventory` dan ``unknown field `attestation` ``; kembalikan file → AC-2
- [ ] `cargo xtask runtime pin --file ~/Development/ojiepermana/stackly-runtimes/out/valkey-9.1.2-r1-macos-aarch64.tar.gz --url https://github.com/ojiepermana/stackly/releases/download/runtime/valkey-9.1.2-r1-macos-aarch64/valkey-9.1.2-r1-macos-aarch64.tar.gz --executable bin/valkey-server` dengan jaringan mati → `format = "tar_gz"`, `urls` persis `--url`, `sha256` sama dengan isi `.sha256`, `size`, dan `min_os = "27.0"`; file ada di cache dengan nama digest-nya dan file asli tetap di `out/` → AC-1, AC-19
- [ ] Ulangi dengan `.sha256` dipindah sementara, dengan digest di dalamnya diubah, dengan nama file lain di dalamnya, dan dengan `--url` yang nama file terakhirnya berbeda → masing masing gagal, dan `ls ~/Library/Caches/dev.stackly.xtask/runtime` sebelum dan sesudah sama → AC-19
- [ ] `cargo xtask runtime pin --file <tar.gz> --url <https> --sha256-url <https>` → ditolak dengan `--file cannot go with --sha256-url` sebelum apa pun dibaca → AC-19
- [ ] `cargo xtask runtime spike --id <id maintainer> --result <file> --no-quarantine` untuk entri lokal di langkah 12.2 → `quarantine.outcome` `skipped` dengan detail `local run`; `closureProbes` memuat `dependencies` lalu `manifest`, keduanya `PASS` → AC-9, AC-18
- [ ] Ubah sementara `recipe` entri itu (jangan di-commit) dan ulangi spike lokal → `FAIL` di langkah `closure` dengan `recipe is "...", the entry says "..."` di detail probe `manifest`, dan probe `dependencies` tetap tercatat; kembalikan file → AC-18
- [ ] Spike lokal Valkey → `observedVersion` `9.1.2` dari baris `valkey_version` (bukan `redis_version`), `listeners` hanya satu port 127.0.0.1 yang dialokasikan, stop lewat `valkey-cli shutdown nosave` dalam 60 detik → AC-5
- [ ] Spike lokal PostgreSQL 18 lalu 17 → probe `icu`, `pgcrypto`, `lz4`, dan `zstd` `PASS`, `observedVersion` `18.6` dan `17.11`, `listeners` kosong (hanya socket di `/tmp/sk-*`), stop karena SIGINT dengan `stop.exitCode` 0 dalam 60 detik → AC-5, AC-7
- [ ] `cargo test -p xtask --test native_runtime -- --list --ignored` → delapan test, termasuk `postgresql_17_runs_from_managed_artifact`, `postgresql_18_runs_from_managed_artifact`, dan `valkey_9_1_runs_from_managed_artifact` → AC-10
- [ ] Wrapper native sebelum entri maintainer ada → ketiga test baru `BLOCKED` dengan `no inventory entry for postgresql 17`, `no inventory entry for postgresql 18`, dan `no inventory entry for valkey 9.1`, owner `@ojiepermana`; record itu tidak disalin `runtime record` → AC-10
- [ ] `cargo xtask evidence check` → semua file Rust lulus aturan makro evidence → AC-10
- [ ] `cargo xtask runtime check` → exit 0 (aturan 9 belum aktif) → AC-2

### Value sourcing

- [ ] `sha256`, `size`, `format`, dan `min_os` dari `pin --file` sama dengan `shasum -a 256`, `stat -f %z`, magic number gzip, dan `vtool -show-build` pada `bin/valkey-server` di dalam tarball → pin `--file`
- [ ] `urls` dari `pin --file` persis nilai `--url` (`.../releases/download/runtime/<id>/<id>.tar.gz`) → `urls` pin `--file`
- [ ] Probe `manifest` membaca `STACKLY-ARTIFACT.json` dan `LICENSES/` di root artefak, lalu jumlah symlink, setuid, dan file khusus dari laporan archive: detail `PASS` spike lokal menyebut jumlah teks lisensi (Valkey 1 ditambah komponen vendored, PostgreSQL 5), dan unit test `a_missing_license_text_is_named` serta `an_archive_with_a_link_setuid_or_special_file_fails` membuktikan sisi gagalnya → probe `manifest`
- [ ] `build`, `source_url`, `source_sha256`, dan `recipe` entri sama dengan `build`, `source.url`, `source.sha256`, dan `recipe` di `STACKLY-ARTIFACT.json` → entri maintainer
- [ ] `executables` entri PostgreSQL `bin/postgres`, `bin/initdb`, `bin/psql`, `bin/pg_dump`, dan Valkey `bin/valkey-server`, `bin/valkey-cli` → entri maintainer, `executables`
- [ ] `logTail` dan detail spike PostgreSQL memakai user database `stackly`, tidak pernah nama user OS → user database PostgreSQL
- [ ] `quarantine.detail` `local run` hanya pada run dengan `--no-quarantine`; spike tanpa flag tetap mencari hasil quarantine di `evidence/runtime/` → flag quarantine, `quarantine.outcome`

### Acceptance-criteria coverage

- AC-2: aturan 5 baru (pola `recipe`, field `attestation` ditolak sebagai TOML tidak valid).
- AC-5, AC-7: recipe `Postgres` dan `Valkey`; `PASS` nyata menunggu artefak di langkah 12.
- AC-9: `--no-quarantine` mencatat `skipped` dengan detail `local run`; test native tidak memakainya.
- AC-10: tiga test native baru, `BLOCKED` sampai entrinya ada.
- AC-12, AC-18: probe `manifest` dan cek isi archive di langkah `closure`.
- AC-19: `pin --file` tanpa jaringan; urutan terbit dicek di langkah 12.
