# Verify: Sumber & packaging runtime · spec 0003 · updated 2026-10-17

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
- [ ] Di Linux (atau job `linux`), `pin` tidak mencetak `min_os` dan memberi catatan `min_os needs vtool`; cache berada di `$XDG_CACHE_HOME/stackly-xtask/runtime/` atau `~/.cache/stackly-xtask/runtime/` → value sourcing cache
- [ ] Entri dengan `upstream_signature` berjenis `gpg` (pertama kali di MySQL, milestone 2): tanpa `gpg` di PATH hasilnya `SKIP gpg`; dengan `gpg`, signature dicek dengan kunci di `runtimes/keys/` dan fingerprint entri → AC-1
- [ ] Di pull request, langkah `Runtime inventory and evidence rules (spec 0003)` di job `linux` hijau → AC-2

### Acceptance-criteria coverage

- AC-1 (inventaris, `pin`, `verify` menghasilkan nilai yang sama): langkah pin, verify, dan gpg. Delapan entri itu sendiri baru datang di milestone 2 dan 3.
- AC-2 (`runtime check`, pesan `<id>: <aturan>`, job `linux`): langkah check, unit test, dan CI. Aturan 8 dan 9 baru datang di build step 7 dan 12.
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
- [ ] Driver dan `cargo xtask runtime spike --prune-cache` tidak pernah menghapus `spike.lock` atau file cache yang digest-nya ada di inventaris → AC-4, AC-15

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
- AC-11, AC-12, AC-14, AC-15: belum di milestone ini.
