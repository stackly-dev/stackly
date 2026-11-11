# Langkah manual: `06-menambah-provider`

Langkah yang butuh mata, tangan, atau akun user. Tidak ada sesi yang menjalankannya. Kirim hasilnya ke lead (nomor langkah, `PASS` atau `FAIL`); lead meneruskannya ke checker yang mencentang `verify.md`.

## Fitur 38: panduan kontributor dan governance (spec 0038)

Dari laporan `arc-f38-spec` (2026-11-11). Semua langkah ini hanya bisa dilakukan pemilik repo. Tidak ada sesi yang mengubah pengaturan GitHub atau melakukan push selain push `main` oleh lead di penutupan.

### Nilai yang dibutuhkan sebelum build milestone 1

1. I1 (wajib untuk AC-3): alamat email yang boleh dipublikasikan untuk laporan Code of Conduct. Tanpa nilai ini `CODE_OF_CONDUCT.md` tidak dibuat dan AC-3 `BLOCKED`.
2. I2: konfirmasi handle GitHub yang ditulis sebagai pemegang semua peran di `GOVERNANCE.md` (default: akun pemilik organisasi `stackly-dev`).
3. I3 (opsional): alamat email cadangan untuk laporan keamanan. Bila tidak diberi, kanal GitHub menjadi satu satunya.
4. Konfirmasi OQ-013: lisensi tetap `MIT OR Apache-2.0`, kontribusi masuk dengan lisensi yang sama tanpa CLA dan tanpa DCO. Diputuskan lewat delegasi; batalkan sebelum langkah 1 spec 0038 dibangun bila tidak setuju. Peninjauan hukum atas distribusi binary pihak ketiga tetap terbuka.
5. Bila sesi build tidak boleh mengunduh: tambahkan `CODE_OF_CONDUCT.md` lewat GitHub (Add file, Create new file, ketik `CODE_OF_CONDUCT.md`, pilih template Contributor Covenant 2.1), atau serahkan file teks resminya.

### Setelah file ada di `main` dan sudah di-push

6. AC-5: aktifkan private vulnerability reporting. Buka `https://github.com/stackly-dev/stackly/settings/security_analysis`, bagian Private vulnerability reporting, tekan Enable.
   Cek: `gh api repos/stackly-dev/stackly/private-vulnerability-reporting` harus membalas `{"enabled":true}` (per 2026-11-11 balasannya `{"enabled":false}`).
   Cek: tab Security repo menampilkan Security policy dari `SECURITY.md` dan tombol Report a vulnerability.
7. Buat lima label triage:
   `gh label create needs-triage --repo stackly-dev/stackly --description "Not yet sorted by a maintainer" --color FBCA04`
   `gh label create core --repo stackly-dev/stackly --description "Bug in the app, daemon, CLI, or protocol" --color D73A4A`
   `gh label create provider --repo stackly-dev/stackly --description "Bug in or proposal for a provider package" --color 0E8A16`
   `gh label create upstream --repo stackly-dev/stackly --description "Incompatibility in an upstream runtime version" --color 5319E7`
   `gh label create configuration --repo stackly-dev/stackly --description "Configuration question, not a defect" --color C5DEF5`
8. Buka Insights, Community Standards, dan pastikan Code of conduct, Contributing, License, Security policy, Issue templates, dan Pull request template bertanda ada.
9. Buka New issue dan pastikan tiga form tampil, issue kosong tidak ditawarkan, dan ada link "Report a security vulnerability".

Hasil: tulis `PASS` atau `FAIL` per nomor. AC-5 tetap `BLOCKED` (`owner: user`) sampai nomor 6 `PASS`.

## Fitur 37: import provider lokal dan trust (spec 0037)

Dari laporan `arc-f37-spec` (2026-11-11). Teks lengkap ditulis `/develop` di `verify.md` spec 0037 bagian `### UI / manual` saat langkah 13; langkah ini baru bisa dijalankan setelah build fitur 37 selesai. Pakai root sementara (`STACKLY_ROOT`), tidak pernah Application Support asli, kecuali langkah 1 yang butuh agent terpasang.

1. Import dari folder terlindung: dengan bundle dari `cargo xtask build-app --install` dan agent menyala, jalankan `stackly provider import ~/Downloads/<paket>` dan ulangi dari GUI. Harapan: berhasil, atau macOS menolak dan Stackly menampilkan `provider.import_unreadable` reason `permission_denied` dengan hint Files and Folders; tidak ada crash, tidak ada permintaan mematikan proteksi.
2. Quarantine nyata: unduh paket contoh sebagai `.zip` lewat browser, buka dengan Finder, impor foldernya. Harapan: preview menampilkan catatan "This folder was downloaded from the internet…", import berhasil, `xattr -l` pada folder asal tetap menunjukkan `com.apple.quarantine`, salinan di `provider-imports/` tanpa atribut itu.
3. Alur GUI lengkap: Import Provider… → sheet preview → Import → banner "Not approved yet" → Approve… (Cancel default, kalimat izin) → install runtime dan create instance → Revoke Approval… → create baru ditolak, instance lama tetap start → Remove…. Cek juga Settings › Sources › Imported providers dan langkah Catalog Trust onboarding.
4. Jaringan mati: ulangi approve, revoke, start instance lama, dan `stackly provider list`. Harapan: semuanya sama, tidak ada teks "latest", "up to date", atau klaim revocation terkini; `runtime.install` baru gagal dengan `runtime.download_offline` biasa.

## Fitur 36: provider SDK dan test harness (spec 0036)

Dari laporan `arc-f36-spec` (2026-11-11). Langkah 2 dan 3 butuh persetujuan user (unduhan artefak bila cache kosong, sesi login GUI).

1. Fuzz parser skenario (spec 0036 AC-3, sesudah build step 1). Prasyarat: `rustup toolchain install nightly-2026-10-17 --profile minimal` dan `cargo install --locked cargo-fuzz --version 0.13.2`. Jalankan `cargo xtask fuzz run provider_scenarios --time 600`. Lulus kalau selesai tanpa crash, hang, atau kehabisan memori. Catat hasilnya di `verify.md` spec 0036.
2. Konformitas native (spec 0036 AC-15, sesudah build step 14). Prasyarat: sesi login GUI aktif, artefak NATS 2.15, PostgreSQL 17 dan 18, dan Traefik 3.7 ada di cache xtask (`~/Library/Caches/dev.stackly.xtask/runtime/`; kalau belum, `cargo xtask runtime verify` mengunduhnya dan itu butuh persetujuan kamu). Jalankan `cargo xtask runtime spike --cleanup`, lalu `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test provider_conformance -- --ignored --test-threads 1`, lalu `cargo xtask runtime spike --cleanup` lagi. Lulus kalau keempat record `PASS`; artefak yang tidak ada di cache memberi `BLOCKED` dengan alasan dan owner, bukan gagal.
3. Spike dari paket (spec 0036 AC-16, sesudah build step 15). Prasyarat: sesi login GUI aktif dan tree bersih. Jalankan `cargo xtask runtime spike --cleanup`, lalu `cargo xtask evidence run --kind native -- cargo test -p xtask --test native_runtime -- --ignored`, lalu `cargo xtask runtime spike --cleanup`. Lulus kalau record NATS, PostgreSQL 17 dan 18, dan Traefik `PASS` dengan langkah yang kini dibangun dari paket, dan record MySQL, ClickHouse, dan Valkey tetap seperti sebelumnya. Jangan jalankan `cargo xtask runtime record` kecuali kamu memang mau mengganti bukti G0 yang tersimpan. Probe `icu` PostgreSQL 17 dan 18 kini memakai database percobaan `stackly_icu_probe` (spec 0003 AC-7, diubah 2026-11-11); bukti probe baru `NOT_RUN` sampai run ini `PASS` dan direkam lewat `cargo xtask runtime record`.
4. Checkout bersih (spec 0036 AC-13). Di clone atau worktree baru tanpa `target/`: `cargo xtask provider check providers/example`, lalu `cargo xtask provider test providers/example`. Lulus kalau baris `ok` tercetak, report `verdict: pass`, baris terakhir stderr `provider test report: <path>`, dan `git status --porcelain` kosong sesudahnya. Build pertama mengambil crate dari jaringan; tidak ada unduhan runtime.
