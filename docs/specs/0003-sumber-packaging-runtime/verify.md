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
