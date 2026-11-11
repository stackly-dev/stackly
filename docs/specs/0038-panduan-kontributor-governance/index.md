# 0038. Panduan kontributor dan governance

**Date**: 2026-11-11
**Status**: In Progress

## Summary

Spec ini menetapkan dokumen dan aturan supaya orang di luar tim bisa menyumbang provider ke Stackly. Lisensi source tetap `MIT OR Apache-2.0` (OQ-013 ditutup), kontribusi masuk dengan lisensi yang sama tanpa CLA, dan lisensi itu tidak berlaku untuk binary pihak ketiga yang Stackly kelola. Laporan keamanan masuk lewat fitur private vulnerability reporting GitHub (kanal privat bawaan GitHub) dan diproses sebagai GitHub Security Advisory. Semua dokumen publik berbahasa Inggris, dan dua perintah baru `cargo xtask docs check` dan `cargo xtask docs walkthrough` menjaga supaya panduan tidak diam diam basi, karena repo ini tidak punya CI.

## Requirements

**User stories**:

- Sebagai kontributor provider, saya ingin satu panduan yang bisa saya ikuti dari clone bersih sampai validator dan harness menghasilkan report, supaya saya tahu paket saya layak dikirim sebagai PR tanpa menyentuh Rust core atau SwiftUI.
- Sebagai kontributor, saya ingin tahu lisensi apa yang berlaku untuk kontribusi saya dan untuk binary yang provider saya jalankan, supaya saya tidak mengirim sesuatu yang tidak boleh saya kirim.
- Sebagai peneliti keamanan, saya ingin kanal privat dan proses yang tertulis, supaya saya bisa melapor tanpa membuka detail di issue publik.
- Sebagai maintainer, saya ingin template issue dan PR yang memisahkan bug core, bug provider, incompatibility upstream, dan soal konfigurasi, supaya setiap laporan sampai ke pemiliknya.
- Sebagai maintainer di repo tanpa CI, saya ingin satu perintah yang memeriksa link, nama perintah, dan cakupan schema di dokumen publik, supaya panduan tidak basi tanpa ada yang tahu.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit):

- **AC-1** (OQ-013): Lisensi tercatat di empat tempat yang saling cocok: `LICENSE-MIT` dan `LICENSE-APACHE` di root (tidak berubah), `license = "MIT OR Apache-2.0"` di `[workspace.package]` yang diwarisi setiap crate, bagian `License` di `README.md`, dan bagian `Licensing` di `CONTRIBUTING.md` yang memuat klausa kontribusi di *Lisensi* kata demi kata serta kalimat bahwa lisensi Stackly tidak mencakup binary pihak ketiga. `repository` di `[workspace.package]` bernilai `https://github.com/stackly-dev/stackly`. `cargo deny check licenses` lulus.
- **AC-2** (REQ-034): `CONTRIBUTING.md` di root memuat setiap bagian di *`CONTRIBUTING.md`*, dalam bahasa Inggris. Setiap perintah `cargo xtask ...` dan `stackly ...` di dalam blok kode `bash` dokumen publik adalah perintah yang benar benar ada (AC-9 memeriksanya).
- **AC-3** (PRD 39): `CODE_OF_CONDUCT.md` di root berisi teks Contributor Covenant versi 2.1 tanpa perubahan selain baris kontak, yang diisi input I1, dan tidak memuat teks `[INSERT`. `verify.md` mencatat URL sumber teks dan SHA-256 file yang diambil sebelum baris kontak diisi.
- **AC-4** (PRD 39): `SECURITY.md` di root memuat setiap bagian di *`SECURITY.md` dan proses advisory*: kanal privat, larangan membuka issue publik untuk kerentanan, isi laporan yang diminta, cakupan dan batas kepercayaan, versi yang didukung, target waktu, dan langkah proses advisory. `.github/ISSUE_TEMPLATE/config.yml` memuat contact link ke `https://github.com/stackly-dev/stackly/security/advisories/new` dan `blank_issues_enabled: false`.
- **AC-5** (PRD 39, langkah manual, owner user): private vulnerability reporting aktif di repo `stackly-dev/stackly`, dibuktikan `gh api repos/stackly-dev/stackly/private-vulnerability-reporting` membalas `{"enabled":true}`, dan halaman Security repo menampilkan `SECURITY.md` sebagai security policy. Selama pemilik repo belum mengaktifkannya, AC ini `BLOCKED` dengan `owner: user`, dan kanal belum boleh disebut tersedia.
- **AC-6** (PRD 39): `GOVERNANCE.md` di root memuat setiap bagian di *`GOVERNANCE.md`*: tabel peran dan pemegangnya saat ini, cara keputusan diambil dan dicatat, arti `official` dan `reviewed`, kewajiban provider owner, tabel empat kategori issue dan penanggung jawabnya, serta cara aturan ini berubah.
- **AC-7** (REQ-034): `.github/ISSUE_TEMPLATE/` memuat tiga issue form (`bug.yml`, `provider-proposal.yml`, `configuration-help.yml`) dan `config.yml`, dan `.github/PULL_REQUEST_TEMPLATE.md` memuat bagian di *Template issue dan PR*. Setiap form lolos pemeriksaan struktur AC-9, dan label yang dipakai form hanya label di tabel *Label triage*.
- **AC-8** (AC-034, REQ-034, JOURNEY-07; needs feature 36): `cargo xtask docs walkthrough` membuat clone baru dari `HEAD`, menjalankan baris demi baris blok perintah bertanda `docs-walkthrough` di `docs/providers/README.md`, dan keluar 0 hanya bila setiap perintah keluar 0, tepat satu report harness `target/provider-test/<runId>/report.json` per perintah `cargo xtask provider test` ada di clone itu dengan `schema` `stackly.provider-conformance/v1` dan verdict lulus, dan `git status --porcelain` clone itu hanya berisi file baru di bawah folder paket hasil scaffold (tidak ada file core, Swift, atau paket lain yang berubah). Blok itu memuat scaffold (`cargo xtask provider new`), validator (`cargo xtask provider check`), dan harness (`cargo xtask provider test`) spec 0036, pada paket contoh `providers/example/` dan pada paket hasil scaffold, dan tidak ada langkah lain yang dibutuhkan selain prasyarat yang ditulis di halaman yang sama.
- **AC-9**: `cargo xtask docs check` memeriksa kumpulan dokumen publik (*Kumpulan dokumen publik*) menurut aturan di *`cargo xtask docs`* dan menulis satu baris `path:line: pesan` per masalah, keluar 1 bila ada masalah dan 0 bila bersih. Pemeriksaannya: (a) setiap link relatif dan anchor menunjuk file dan heading yang ada; (b) tidak ada placeholder; (c) setiap perintah `cargo xtask` dan `stackly` di blok `bash` ada di tabel perintah statis, tanpa pernah menjalankan perintah itu; (d) cakupan schema AC-10; (e) struktur issue form; (f) setiap paket di `providers/` selain namespace `testing` punya `LICENSE-NOTICES.md` yang cocok dengan inventaris; (g) tidak ada HTML inline selain `<a>` dan setiap blok kode berpagar punya bahasa; (h) kalimat izin AC-12. Test xtask memberi satu fixture rusak per pemeriksaan dan memastikan pesannya, dan `docs check` bersih pada `README.md` serta `providers/*/README.md` yang ada hari ini.
- **AC-10** (REQ-034; needs feature 36): `docs/providers/manifest-reference.md`, `settings-reference.md`, dan `versions-reference.md` menyebut dalam backtick setiap nama property dan setiap nilai enum string dari `schemas/provider/provider.v1alpha1.schema.json`, `settings.v1alpha1.schema.json`, dan `versions.v1.schema.json`. Property atau nilai yang ditambahkan ke schema tanpa masuk ke halaman rujukannya membuat `docs check` gagal dengan nama itu.
- **AC-11** (REQ-034, PRD 39; needs feature 36): `docs/providers/` memuat `README.md`, `authoring.md`, `testing.md`, `compatibility.md`, dan `review.md` dengan isi di *Dokumen provider*. `compatibility.md` menyatakan kebijakan kompatibilitas, deprecation, dan migrasi `stackly/v1alpha1` sesuai keputusan spec 0036, termasuk bahwa manifest dengan versi schema yang tidak didukung ditolak tanpa menjalankan lifecycle.
- **AC-12** (JOURNEY-07; needs feature 37): `docs/providers/trying-locally.md` menjelaskan alur import lokal dan approval spec 0037 dengan nama perintah dan istilah trust dari spec itu (isi di *Dokumen provider*), memuat kalimat izin spec 0037 kata demi kata dari `protocols/fixtures/provider/approval-statement.txt` (pemeriksaan (h) `docs check`), menyatakan dengan jujur bahwa paket hasil scaffold (`testing/<name>`) tidak bisa diimpor, dan menyatakan bahwa maintainer tidak pernah mengimpor paket dari PR ke Stackly aslinya sebelum review selesai.
- **AC-13** (PRD 39): `providers/nats/`, `providers/postgresql/`, dan `providers/traefik/` masing masing punya `LICENSE-NOTICES.md` berformat *`LICENSE-NOTICES.md` paket provider*, dengan lisensi runtime yang sama dengan `license` di `runtimes/inventory.toml` dan pernyataan asal binary (diunduh dari upstream atau dibangun maintainer) yang sama dengan `origin` entri itu, diperiksa pemeriksaan (f). Digest paket dan `providers/catalog-revisions.json` tidak berubah, karena file itu tidak pernah dibaca loader (spec 0009).
- **AC-14**: Semua dokumen publik berbahasa Inggris dan mengikuti `.markdownlint.jsonc`; bagian yang ditegakkan mesin adalah pemeriksaan (g), sisanya dijaga review. `README.md` tidak lagi menyebut status "early scaffold", dan memuat bagian `Contributing`, `Security`, dan `License` yang menunjuk file di root.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md): file community health di root dan `.github/`, panduan provider tulisan tangan di `docs/providers/`, dan penjaga mekanis di xtask.

Stackly tetap berlisensi `MIT OR Apache-2.0` dengan kontribusi inbound sama dengan outbound, memakai private vulnerability reporting GitHub sebagai kanal keamanan, Contributor Covenant 2.1 sebagai Code of Conduct, model governance yang dipimpin maintainer, dan `cargo xtask docs` sebagai pengganti CI untuk dokumen.

OQ-013 diputuskan lewat delegasi pemilik produk (log koordinasi scope 06, 2026-11-11: setiap keputusan mengambil opsi yang direkomendasikan) dan dilaporkan lead ke pemilik produk; ia bisa membatalkannya sebelum langkah 1 dibangun.

Ringkasan keputusan (alasan dan runner up di rationale):

| ID | Keputusan | Pilihan |
| --- | --- | --- |
| K1 | Lisensi source (OQ-013) | `MIT OR Apache-2.0` untuk semua yang ditulis proyek di repo ini; tidak berlaku untuk binary pihak ketiga |
| K2 | Lisensi kontribusi | Inbound sama dengan outbound lewat klausa di `CONTRIBUTING.md`; tanpa CLA, tanpa DCO |
| K3 | Kanal security | Private vulnerability reporting GitHub; email hanya cadangan opsional |
| K4 | Proses advisory | GitHub Security Advisory, target waktu yang jujur untuk satu maintainer |
| K5 | Code of Conduct | Contributor Covenant 2.1, teks asli |
| K6 | Governance | Dipimpin maintainer, peran ditulis terbuka, tanpa `CODEOWNERS` sampai ada maintainer kedua |
| K7 | Letak dokumen | Root untuk empat file utama, `.github/` untuk template, `docs/providers/` untuk panduan provider |
| K8 | Schema docs | Halaman rujukan tulisan tangan, dijaga pemeriksaan cakupan terhadap JSON Schema |
| K9 | Template issue | Issue form YAML: satu form bug dengan pilihan area, proposal provider, bantuan konfigurasi |
| K10 | Template PR | Satu template dengan daftar cek lokal, karena tidak ada CI |
| K11 | Bukti AC-034 | `cargo xtask docs walkthrough` di clone baru, dicatat sebagai record manual spec 0002 |
| K12 | Rujukan CLI (Follow-up spec 0008) | Fitur 38 hanya menulis yang dipakai kontributor; rujukan CLI umum dan JSON Schema envelope tetap di fitur 66 |
| K13 | Provider template | Satu sumber: paket contoh `providers/example/` spec 0036, yang disalin `cargo xtask provider new`; spec ini hanya menetapkan `LICENSE-NOTICES.md` untuk paket yang masuk catalog |

## Feature design

### Batas dengan spec lain

| Hal | Pemilik | Yang spec ini lakukan |
| --- | --- | --- |
| Perintah scaffold, validator, harness, format report, paket contoh `providers/example/` dan `README.md` nya, fake server, kebijakan kompatibilitas schema | spec 0036 | Mendokumentasikannya apa adanya dalam bahasa Inggris; tidak menambah atau mengganti perintah. Spec 0036 tidak menulis dokumen publik lain |
| Import lokal, preview, approval, revocation, dialog izin, istilah trust | spec 0037 | Merujuknya di `trying-locally.md` dan `review.md` |
| Lisensi dan asal tiap artefak runtime, gerbang lisensi sebelum unggahan | spec 0003 (`runtimes/inventory.toml`) | Menunjuk ke sana; `LICENSE-NOTICES.md` tidak menyalin data per versi |
| Layout paket provider, file yang dibaca loader | spec 0009 | Menetapkan isi dua file opsional yang tidak dibaca (`README.md`, `LICENSE-NOTICES.md`) |
| Kontrak output CLI (envelope, exit code, variabel environment) | spec 0008 | Hanya mengutip exit code perintah provider dan variabel untuk dev root |
| Runbook, getting started, troubleshooting, rujukan CLI umum | fitur 66 | Tidak ditulis di sini |
| Evidence record | spec 0002 | Bukti AC-8 dicatat sebagai record manual |
| Notice lisensi untuk rilis, SBOM | fitur 60 | Tidak ditulis di sini |

### Peta dokumen

Semua file di tabel ini berbahasa Inggris. Kolom Butuh menyebut fitur yang harus sudah ada di `main` sebelum file itu bisa ditulis benar.

| File | Isi | Butuh |
| --- | --- | --- |
| `CONTRIBUTING.md` | Pintu masuk kontributor | bagian provider: fitur 36 |
| `CODE_OF_CONDUCT.md` | Contributor Covenant 2.1 | I1 |
| `SECURITY.md` | Kebijakan dan proses pelaporan kerentanan | tidak ada |
| `GOVERNANCE.md` | Peran, keputusan, arti official dan reviewed, triage | I2 |
| `.github/ISSUE_TEMPLATE/bug.yml`, `provider-proposal.yml`, `configuration-help.yml`, `config.yml` | Issue form | tidak ada |
| `.github/PULL_REQUEST_TEMPLATE.md` | Template PR | tidak ada |
| `README.md` (ubah) | Status, bagian Contributing, Security, License | tidak ada |
| `docs/providers/README.md` | Indeks dan quick start (blok walkthrough) | fitur 36 |
| `docs/providers/authoring.md` | Menulis paket provider langkah demi langkah | fitur 36 |
| `docs/providers/manifest-reference.md`, `settings-reference.md`, `versions-reference.md` | Rujukan field | fitur 36 |
| `docs/providers/testing.md` | Validator, harness, fake server, report, bukti native | fitur 36 |
| `docs/providers/compatibility.md` | Kebijakan kompatibilitas dan deprecation schema | fitur 36 |
| `docs/providers/review.md` | Daftar periksa review provider | fitur 36 |
| `docs/providers/trying-locally.md` | Import lokal dan trust | fitur 37 |
| `providers/{nats,postgresql,traefik}/LICENSE-NOTICES.md` | Notice lisensi paket | tidak ada |

**Kumpulan dokumen publik** (yang diperiksa `docs check`): `README.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `GOVERNANCE.md`, semua file di `.github/`, `docs/providers/**/*.md`, `providers/*/README.md`, dan `providers/*/LICENSE-NOTICES.md`. `CODE_OF_CONDUCT.md` hanya ikut pemeriksaan (a) dan `[INSERT`, karena teksnya tidak boleh diubah. `docs/specs/`, `docs/scope/`, `docs/reviews/`, `docs/manual.md`, dan `AGENTS.md` bukan dokumen publik dalam arti ini dan tetap berbahasa seperti sekarang.

### Input dari pemilik repo

Nilai ini hanya pemilik repo yang tahu. Developer menerimanya lewat brief; tanpa nilai itu, baris yang bergantung padanya `BLOCKED` dengan `owner: user`, dan tidak pernah diisi tebakan.

| ID | Nilai | Dipakai di | Bila belum ada |
| --- | --- | --- | --- |
| I1 | Alamat kontak untuk laporan Code of Conduct (alamat email yang boleh dipublikasikan) | `CODE_OF_CONDUCT.md` | Langkah 3 `BLOCKED`; file tidak dibuat dengan placeholder |
| I2 | Handle GitHub yang ditulis sebagai pemegang peran | `GOVERNANCE.md` | Default: akun pemilik organisasi `stackly-dev` yang sudah terlihat publik di riwayat commit, dikonfirmasi lead |
| I3 | Alamat email cadangan untuk laporan keamanan (opsional) | `SECURITY.md` | Bagian cadangan tidak ditulis; kanal GitHub tetap satu satunya |

Langkah yang hanya bisa dilakukan pemilik repo di GitHub (ditulis di `verify.md` bagian `### UI / manual` saat build, owner user):

1. Aktifkan private vulnerability reporting: Settings, Code security, Private vulnerability reporting, Enable. Cek: `gh api repos/stackly-dev/stackly/private-vulnerability-reporting` membalas `{"enabled":true}`. Per 2026-11-11 balasannya `{"enabled":false}` dan repo berstatus publik.
2. Buat label triage (tabel *Label triage*) dengan `gh label create <nama> --repo stackly-dev/stackly --description <teks> --color <hex>`. Form tetap berfungsi tanpa label, tetapi label tidak terpasang otomatis.
3. Setelah file di `main` dan ter-push: buka tab Insights, Community Standards, dan pastikan Code of conduct, Contributing, License, Security policy, Issue templates, dan Pull request template bertanda ada.
4. Push sendiri adalah keputusan pemilik repo; tidak ada sesi yang melakukannya.

### Lisensi

- Semua yang ditulis proyek di repo ini (crate Rust, app Swift, paket provider di `providers/`, schema, fixture, dan dokumentasi) berlisensi `MIT OR Apache-2.0`. Tidak ada header lisensi per file; dua file lisensi di root dan field `license` di manifest crate adalah sumbernya.
- Klausa kontribusi, ditulis kata demi kata di `CONTRIBUTING.md` bagian `Licensing` dan di `README.md` bagian `License`:

  ```text
  Unless you explicitly state otherwise, any contribution intentionally submitted
  for inclusion in Stackly by you, as defined in the Apache-2.0 license, shall be
  dual licensed as above, without any additional terms or conditions.
  ```

- `CONTRIBUTING.md` juga menulis: kontributor hanya mengirim karya yang berhak ia kirim; materi yang disalin dari upstream (misalnya contoh config) harus disebut di `LICENSE-NOTICES.md` paket dengan lisensi dan sumbernya; dan kontribusi yang dibuat dengan bantuan alat AI tetap tanggung jawab pengirimnya.
- Lisensi Stackly tidak menentukan hak atas software yang dikelolanya. Lisensi setiap runtime, per versi, dan apakah binary diunduh dari upstream atau dibangun maintainer, dicatat di `runtimes/inventory.toml` (`license`, `license_url`, `origin`; spec 0003). Artefak buatan maintainer membawa folder `LICENSES/`. Provider baru yang runtime nya berlisensi di luar daftar `allow` `deny.toml` tetap boleh diusulkan (daftar itu hanya untuk dependency yang di-link), tetapi butuh persetujuan lisensi release owner yang dicatat lewat gerbang spec 0003 sebelum masuk catalog.
- Spec ini bukan pendapat hukum. Peninjauan pihak berwenang atas distribusi binary tetap terbuka di spec 0003 dan fitur 60.

### `CONTRIBUTING.md`

Bagian wajib, berurutan:

1. **Ways to contribute**: laporan bug, proposal provider, paket provider, perbaikan docs, perubahan core. Menyebut bahwa provider sederhana tidak butuh Rust atau Swift baru, dan bahwa perubahan core atau primitive eksekusi butuh review tambahan dan biasanya spec lebih dulu.
2. **Before you start**: Code of Conduct, cara bertanya (issue form), dan bahwa kerentanan tidak dilaporkan lewat issue (tunjuk `SECURITY.md`).
3. **Set up**: prasyarat dari `README.md` (Mac Apple Silicon macOS 27, rustup, Xcode 27 hanya untuk app), `git config core.hooksPath .githooks`, dan catatan bahwa kontributor provider cukup dengan toolchain Rust.
4. **Build and check locally**: repo ini tidak punya CI, jadi kontributor menjalankan cek sendiri dan menempel hasilnya di PR. Daftar perintah diambil dari bagian Commands `AGENTS.md`: `cargo fmt --all --check`, clippy, `cargo test -p <crate>`, `cargo deny check`, `cargo xtask check-deps`, `cargo xtask codegen --check`, `cargo xtask provider check`, `cargo xtask docs check`. Perubahan paket provider menambah `cargo xtask provider test <dir>` (sejak fitur 36) dan, bila `providers/catalog.toml` atau paket yang terdaftar di sana berubah, `cargo xtask catalog check`. Perubahan Swift menambah `swift test` dan `cargo xtask build-app`.
5. **Run from source safely**: selalu pakai dev root, `STACKLY_ROOT=$(mktemp -d)`, dan jangan pernah menguji terhadap folder Application Support asli; `STACKLY_LOG` untuk filter log.
6. **Contributing a provider**: ringkasan lima langkah dan link ke `docs/providers/README.md`. Tidak mengulang isi panduan.
7. **Pull requests**: satu perubahan per PR, branch dari `main`, commit `type(scope): subject` dalam bahasa Inggris, isi template PR, maintainer menjalankan ulang cek di mesinnya sebelum merge, dan PR dari fork tidak pernah mendapat kredensial signing atau rilis.
8. **Where decisions live**: `docs/specs/` dan `docs/scope/` ditulis dalam bahasa Indonesia; kontributor boleh meminta ringkasan Inggris lewat issue; kode, komentar, pesan error, dan dokumen publik berbahasa Inggris. Aturan lengkap ada di `AGENTS.md`, dan `CONTRIBUTING.md` menunjuk ke sana alih alih menyalinnya.
9. **Licensing**: isi *Lisensi*.

### `CODE_OF_CONDUCT.md`

Teks resmi Contributor Covenant 2.1 dalam bahasa Inggris, tidak diubah selain satu baris kontak (I1). Developer mengambil teks dari sumber resminya; bila sesi build tidak boleh mengunduh, pemilik repo menambahkannya lewat menu Add file, Code of conduct di GitHub atau menyerahkan filenya, dan langkah itu `BLOCKED` sampai saat itu. Teks tidak pernah ditulis ulang dari ingatan.

### `SECURITY.md` dan proses advisory

Bagian wajib:

1. **Reporting a vulnerability**: pakai tombol Report a vulnerability di tab Security repo (`https://github.com/stackly-dev/stackly/security/advisories/new`). Jangan membuka issue, PR, atau diskusi publik. Bila I3 ada, satu kalimat cadangan dengan alamat itu.
2. **What to include**: versi Stackly (`stackly --version`) dan macOS, provider dan versi runtime, langkah reproduksi, dampak, dan apakah sudah diketahui pihak lain. Diagnostic bundle (spec 0034) sudah disensor, tetapi pelapor diminta memeriksanya sebelum melampirkan dan tidak pernah mengirim password.
3. **Scope**: di dalam cakupan: `stacklyd`, `stackly`, app macOS, protokol IPC, paket provider di repo ini, catalog bawaan, dan pengemasan artefak runtime buatan maintainer. Di luar cakupan: kerentanan di software upstream itu sendiri (laporkan ke upstream, dan beri tahu kami supaya versi yang diperbaiki bisa dipin atau versi lama dicabut), dan serangan yang butuh proses yang sudah berjalan sebagai user macOS yang sama atau root. Batas kepercayaan Stackly adalah satu akun user OS (spec 0006); laporan di luar batas itu tetap diterima lewat kanal privat dan diperlakukan sebagai hardening.
4. **Supported versions**: hanya `main` sampai rilis pertama; sesudah itu, sebelum 1.0, rilis terakhir dan `main`.
5. **What to expect**: konfirmasi diterima dalam 7 hari, penilaian awal dalam 14 hari, dan target perbaikan atau publikasi paling lama 90 hari sejak laporan, dengan kabar bila butuh lebih lama. Ditulis sebagai target, bukan jaminan, karena proyek ini dipegang satu maintainer.
6. **Our process**: (1) security contact mengonfirmasi dan mereproduksi di test root; (2) membuat draft GitHub Security Advisory dan mengundang pelapor; (3) perbaikan dikerjakan di fork privat sementara milik advisory itu, dengan cek lokal yang sama dengan PR biasa; (4) untuk masalah di paket provider, revision baru diterbitkan dan revision yang terdampak ditarik dari catalog bawaan (langkah 11 menambahkan satu kalimat tentang alur revocation spec 0037 setelah fitur itu ada); untuk artefak runtime, build baru dan entri inventaris baru; (5) rilis perbaikan, lalu advisory dipublikasikan, dengan CVE diminta lewat GitHub bila versi yang sudah dirilis terdampak; (6) pelapor diberi kredit kecuali menolak; (7) `CHANGELOG.md` mendapat entri `Security`. Detail sensitif tidak dipublikasikan sebelum langkah 5.

### `GOVERNANCE.md`

Bagian wajib:

1. **Roles**: tabel peran, tanggung jawab, dan pemegang saat ini.

   | Peran | Tanggung jawab | Pemegang saat ini |
   | --- | --- | --- |
   | Core maintainer | Domain, core, daemon, CLI, protokol; keputusan akhir | I2 |
   | Platform maintainer (macOS) | Adapter macOS, app SwiftUI, signing | I2 |
   | Provider owner | Satu paket provider: manifest, versi, bukti kompatibilitas, issue provider itu | I2 untuk setiap provider `official/` |
   | Security contact | Menerima laporan privat, menjalankan proses advisory | I2 |
   | Release owner | Rilis, gerbang lisensi runtime, unggahan artefak | I2 |

   Dokumen menyatakan terus terang bahwa saat ini satu orang memegang semua peran.
2. **How decisions are made**: core maintainer memutuskan; keputusan yang menahan beban dicatat sebagai spec di `docs/specs/`; siapa pun boleh mengusulkan lewat issue; perbedaan pendapat diselesaikan di issue itu, dan maintainer menulis alasannya.
3. **Official and reviewed**: `official` berarti paket tinggal di `providers/` repo ini dengan namespace `official/`, punya provider owner, dan masuk catalog bawaan. `reviewed` berarti revision itu melewati daftar periksa `docs/providers/review.md` pada saat merge. Keduanya bukan jaminan bebas risiko selamanya, dan review berlaku per revision. `reviewed` adalah kata proses di dokumen ini, bukan label di app. Langkah 11 menyamakan bagian ini dengan istilah spec 0037: trust `official`, `development`, dan `local` ("Local, not verified"), approval `pending`, `approved`, dan `revoked`, dengan kata kerja import, approve, revoke, remove. Dokumen publik tidak memakai "untrusted", "trusted publisher", "verified publisher", "sandboxed", atau "safe to run", dan tidak pernah menyebut daftar revocation "up to date" atau "latest".
4. **Provider owners**: tabel provider, owner, dan status; kewajiban owner (menjawab issue provider, memperbarui versi dan bukti, menandai incompatibility upstream); cara menjadi owner (PR provider yang diterima plus kesediaan memegangnya); dan apa yang terjadi bila owner tidak aktif (provider ditandai tanpa owner, lalu boleh ditarik dari catalog bawaan tanpa menghapus data pengguna; langkah 11 menambahkan rujukan ke alur revocation spec 0037).
5. **Issue triage**: tabel empat kategori.

   | Kategori | Label | Penanggung jawab | Hasil yang wajar |
   | --- | --- | --- | --- |
   | Bug core | `core` | Core atau platform maintainer | Perbaikan di repo ini |
   | Bug provider | `provider` | Provider owner | Revision paket baru |
   | Incompatibility upstream | `upstream` | Provider owner | Versi ditandai, link ke issue upstream, tidak diperbaiki di Stackly |
   | Soal konfigurasi | `configuration` | Siapa pun; maintainer menutup | Jawaban dengan link docs; docs diperbaiki bila berulang |

6. **Compatibility and deprecation**: link ke `docs/providers/compatibility.md` (schema provider) dan penjelasan singkat bahwa protokol IPC punya versi dan digest sendiri (spec 0006).
7. **Review without CI**: maintainer menjalankan cek di mesinnya; publikasi catalog dan signing hanya dari konteks rilis tepercaya, tidak pernah dari skrip PR.
8. **Changing this document**: lewat PR yang disetujui core maintainer.

Tidak ada file `CODEOWNERS` sampai ada pemegang peran kedua; saat itu file ditambahkan dan tabel peran diperbarui di PR yang sama.

### Template issue dan PR

Issue form (sintaks issue form GitHub, YAML). Setiap form punya `name`, `description`, `labels`, dan `body`; setiap elemen `body` selain `markdown` punya `id` unik.

| File | Untuk | Field wajib | Label |
| --- | --- | --- | --- |
| `bug.yml` | Sesuatu tidak bekerja | Area (dropdown: `Core (app, daemon, CLI)`, `A provider package`, `An upstream runtime version`, `Not sure`), versi Stackly, versi macOS, provider dan versi runtime (opsional), langkah, hasil yang diharapkan dan yang terjadi, keluaran `stackly doctor` (opsional, dengan peringatan jangan menempel secret) | `needs-triage` |
| `provider-proposal.yml` | Usul provider baru | Nama software, link upstream, lisensi upstream, apakah upstream menerbitkan binary macOS arm64, apakah pengusul mau menjadi provider owner | `needs-triage`, `provider` |
| `configuration-help.yml` | Bantuan konfigurasi | Apa yang dicoba, provider, potongan config tanpa secret | `needs-triage`, `configuration` |
| `config.yml` | | `blank_issues_enabled: false`; contact link `Report a security vulnerability` ke URL advisory | |

**Label triage** (dibuat pemilik repo, langkah manual 2): `needs-triage`, `core`, `provider`, `upstream`, `configuration`. Maintainer mengganti `needs-triage` dengan salah satu dari empat label kategori. Tidak ada label `security`, karena kerentanan tidak pernah masuk issue.

`.github/PULL_REQUEST_TEMPLATE.md`, bagian wajib:

1. `## What and why`, dengan link issue.
2. `## Kind of change`: kotak centang `Provider package`, `Core or daemon`, `CLI`, `macOS app`, `Docs`.
3. `## Checks I ran locally`: kotak centang untuk setiap perintah di `CONTRIBUTING.md` bagian 4 dengan kalimat "this repository has no CI", dan tempat menempel keluaran.
4. `## Provider checklist` (hanya untuk PR provider): `cargo xtask provider check` bersih, `report.json` dari `cargo xtask provider test` dilampirkan, `metadata.revision` dinaikkan untuk setiap perubahan pada file yang masuk digest paket (spec 0009; `README.md` dan `LICENSE-NOTICES.md` tidak termasuk), `cargo xtask catalog check` bersih bila paket terdaftar di catalog, entri `runtimes/inventory.toml` memakai URL dan digest asli, `LICENSE-NOTICES.md` diperbarui, tidak ada perubahan Rust core atau Swift.
5. `## Licensing`: kotak centang "I have the right to submit this and agree to license it under MIT OR Apache-2.0".

### Dokumen provider (`docs/providers/`)

| File | Isi wajib |
| --- | --- |
| `README.md` | Untuk siapa halaman ini; prasyarat (toolchain di `rust-toolchain.toml` lewat rustup dan Xcode Command Line Tools, tanpa unduhan runtime); **quick start** berupa satu blok `bash` yang didahului komentar `<!-- docs-walkthrough -->` dan berisi persis empat perintah: `cargo xtask provider check providers/example`, `cargo xtask provider test providers/example`, `cargo xtask provider new mytool`, `cargo xtask provider test providers/mytool`; satu kalimat tentang letak report; daftar halaman lain; alur kontribusi PRD 39 dalam tujuh langkah (scaffold, validasi schema, test fixture lokal, bukti kompatibilitas native, review source, lisensi, dan keamanan, review maintainer, merge revision yang tidak berubah lagi) |
| `authoring.md` | Layout paket (spec 0009); `cargo xtask provider new <name>` menyalin `providers/example/` menjadi paket `testing/<name>` yang langsung valid; mengisi `provider.yaml`, settings schema, template, dan `tests/scenarios.yaml` (satu satunya file `tests/` yang dibaca; tanpa file itu harness menjalankan satu skenario `default`); lalu bagian **Moving to the official catalog** dengan langkah spec 0036: entri `runtimes/inventory.toml` dengan URL dan digest asli (`cargo xtask runtime pin`, alur spec 0003), ganti `metadata.id` dan `software`, `cargo xtask codegen`, `cargo xtask provider freeze`, test native per line, baris `[[provider]]` di `providers/catalog.toml`, `LICENSE-NOTICES.md`, dan kenaikan `metadata.revision` untuk setiap perubahan sesudahnya pada file yang masuk digest paket |
| `manifest-reference.md`, `settings-reference.md`, `versions-reference.md` | Setiap property dan nilai enum dari schema terkait, dengan arti, wajib atau tidak, batas, dan contoh pendek yang dipotong dari paket nyata di `providers/`; link ke `schemas/provider/*.json` untuk editor |
| `testing.md` | `cargo xtask provider check` (tanpa daemon) dan `stackly provider validate <dir>` (butuh daemon berjalan; hasilnya sama), arti finding `error` dan `warning` serta `loadable` dan `publishable`; `cargo xtask provider test <dir>` dengan flag nya, perilaku fake server yang bisa diatur skenario (delay ready, crash, spam log), report `target/provider-test/<runId>/report.json` (`stackly.provider-conformance/v1`, path dicetak di baris terakhir stderr), exit code 0 lulus, 1 gagal, 2 `BLOCKED`, `cargo xtask provider test --cleanup`, jaminan bahwa harness memakai test root sementara dan daemon nya sendiri; lalu beda bukti fixture dan bukti native (spec 0002): PR kontributor luar cukup membawa report harness, dan maintainer yang menjalankan bukti native |
| `compatibility.md` | Kebijakan spec 0036: perubahan additive, apa yang dianggap breaking, deprecation, migrasi, dan penolakan versi schema yang tidak didukung |
| `review.md` | Daftar periksa yang dipakai maintainer: validator dan `cargo xtask provider check`; report harness; bukti native di Mac maintainer dengan tiga perintah spec 0036 (bukti artefak spec 0003, `provider_conformance` native, dan test launchd hanya bila provider mau bertahan melewati restart daemon terencana); corpus kompatibilitas diperbarui lewat `cargo xtask provider freeze`; sumber dan lisensi (entri inventaris dengan digest asli, tanpa URL karangan, `LICENSE-NOTICES.md`); keamanan (executable hanya dari runtime, tidak ada secret di argv, bind default loopback, tidak ada skrip bebas); revision tidak berubah setelah merge; handover restart daemon hanya lewat test native (aturan `HANDOVER_PROVEN`); dan peringatan bahwa menjalankan test, harness, atau `docs walkthrough` pada branch PR berarti menjalankan kode PR itu, jadi diff dibaca lebih dulu |
| `trying-locally.md` | Alur spec 0037 setelah validator dan harness: `stackly provider validate <dir>`, `stackly provider import <dir> --dry-run` (preview tanpa menulis), `stackly provider import <dir>`, `stackly provider inspect <provider>`, `stackly provider approve <provider>`, lalu `stackly runtime install` dan `stackly instance create` seperti provider resmi; `revoke` dan `remove`; padanan GUI (Catalog, Import Provider). Yang wajib ditulis jujur: import hanya menerima namespace selain `official`, `testing`, dan `stackly`, jadi paket hasil scaffold tidak bisa diimpor dan hanya untuk harness; paket yang diimpor butuh namespace sendiri dan artefak dengan URL `https` dan `sha256` asli; paket untuk namespace `official` dicoba lewat catalog development (`STACKLY_CATALOG_DIR`, spec 0017); paket harus lolos validator tanpa error; setiap perubahan isi paket butuh import dan approve ulang; provider impor tidak pernah berlabel Tested. Kalimat izin dikutip utuh sebagai blockquote. Larangan mengimpor paket PR yang belum direview ke Stackly asli |

Contoh di halaman rujukan selalu potongan dari paket nyata, dengan nama file sumbernya disebut, supaya tidak ada manifest karangan.

### `LICENSE-NOTICES.md` paket provider

File opsional menurut loader (spec 0009), wajib menurut spec ini untuk setiap paket di `providers/` selain namespace `testing`. Tiga heading wajib, berurutan:

1. `## Package`: file paket ini berlisensi `MIT OR Apache-2.0`.
2. `## Runtime`: nama software, lisensi upstream (SPDX) per lini bila berbeda, link lisensi, dan satu kalimat asal binary: `Downloaded from the upstream release` atau `Built by Stackly maintainers from upstream source`. Diakhiri kalimat bahwa data per versi ada di `runtimes/inventory.toml`.
3. `## Third party material in this package`: daftar file paket yang disalin atau diturunkan dari upstream dengan lisensi dan sumbernya, atau `None.`.

Paket contoh `providers/example/` (`testing/example`) dan paket hasil scaffold tidak membawa file ini; `authoring.md` meminta penulis menambahkannya saat paket pindah ke namespace `official`.

### `README.md`

Kalimat status "early scaffold" diganti satu kalimat status yang benar saat build (apa yang sudah bisa dijalankan, tanpa klaim rilis). Bagian baru `Contributing` (link `CONTRIBUTING.md` dan `docs/providers/README.md`) dan `Security` (link `SECURITY.md`), dan bagian `License` mendapat klausa kontribusi. Baris tabel Layout untuk `docs/` menyebut `docs/providers/`.

### `cargo xtask docs`

Modul baru `xtask/src/docs/`. Tidak menyentuh crate lain; `xtask` menambah dependency `serde_yaml_ng` (sudah ada di `[workspace.dependencies]`) untuk membaca issue form dan `provider.yaml`. Dua baris baru masuk tabel `USAGE` di `xtask/src/main.rs`.

`cargo xtask docs check` membaca kumpulan dokumen publik dari root repo, tidak menulis apa pun, tidak membuka jaringan, dan tidak pernah menjalankan perintah yang ia temukan di dokumen.

Aturan umum pembacaan Markdown: blok kode berpagar dikenali dari pagar tiga backtick; inline code span (teks di antara backtick) dan autolink `<https://...>` dilewati oleh pemeriksaan (b) dan (g). Untuk file `.yml` di `.github/`, hanya (a) dan (b) yang berjalan, atas setiap nilai string, ditambah (e). `.github/PULL_REQUEST_TEMPLATE.md` diperlakukan seperti file Markdown lain.

- **(a) Link.** Link inline dan link gaya rujukan (`[teks][id]` dengan definisi `[id]: tujuan`) diperiksa. Tujuan relatif dihitung dari folder file itu; tujuan boleh file atau folder yang ada. Bagian `#anchor` harus cocok dengan slug sebuah heading di file tujuan (atau file itu sendiri bila tujuannya hanya `#anchor`). Slug mengikuti aturan GitHub: huruf kecil; buang setiap karakter selain huruf, angka, spasi, `-`, dan `_`; spasi menjadi `-`; heading kembar mendapat akhiran `-1`, `-2`, dan seterusnya. Tujuan `http`, `https`, dan `mailto` tidak diperiksa.
- **(b) Placeholder.** Kata utuh `TODO`, `TBD`, dan `FIXME` (peka huruf besar kecil), serta teks `[INSERT` dan `example.com`, di luar blok kode dan code span.
- **(c) Perintah.** Sumber kebenaran adalah dua tabel statis, bukan `--help`:
  - Tabel xtask: dari konstanta `USAGE`, setiap baris yang diawali tepat dua spasi lalu huruf kecil memberi satu jalur tugas, yaitu kata kata berhuruf kecil dan `-` di awal baris sebelum token pertama yang diawali `-`, `<`, atau `[` (contoh: `codegen`, `provider check`, `runtime spike`).
  - Tabel `stackly`: satu pemanggilan `cargo run -q --locked -p stackly-cli -- manpage --json` (perintah tersembunyi spec 0015 yang hanya membaca pohon clap); setiap `pages[].file` di `result` tanpa akhiran `.1` adalah satu jalur dengan kata dipisah `-` (contoh `stackly-daemon-status`).
  - Dari setiap blok berpagar `bash`: gabungkan baris yang berakhir `\` dengan baris berikutnya; buang dari ` #` sampai akhir baris; pecah di `&&`, `;`, dan `|`; di setiap potongan, buang awalan `NAMA=nilai` (nilai boleh berbentuk `$(...)`); lalu kenali tiga bentuk: `cargo xtask <kata>...`, `stackly <kata>...`, dan `cargo run ... -p stackly-cli -- <kata>...`. Potongan lain dilewati.
  - Kata kandidat adalah kata kata sebelum token pertama yang diawali `-`, `<`, `[`, `$`, atau yang berisi `/` atau `.`. Untuk xtask, awalan terpanjang kata kandidat harus sama dengan satu jalur tugas; bila tidak ada, masalahnya `unknown xtask task`. Untuk `stackly`, awalan terpanjang yang digabung dengan `-` harus sama dengan satu jalur halaman; bila jalur yang cocok masih punya anak (ada jalur lain berawalan jalur itu plus `-`) dan masih ada kata kandidat tersisa, masalahnya `unknown stackly command`.
- **(d) Cakupan schema.** Untuk tiap pasangan schema dan halaman rujukan di AC-10, telusuri hanya posisi schema: mulai dari root, masuk ke setiap nilai di `properties`, `$defs`, `items`, `additionalProperties` (bila object), dan setiap elemen `anyOf`, `oneOf`, `allOf`. Di setiap posisi schema, kumpulkan kunci `properties` sebagai nama property, dan nilai string di `enum` serta `const` sebagai nilai enum. Field bernama `properties`, `enum`, atau `const` di dalam `properties` settings schema dihitung sebagai nama property biasa. Setiap nama dan nilai yang terkumpul harus muncul paling sedikit sekali sebagai code span utuh di halaman rujukannya; satu penyebutan mencakup nama yang sama di beberapa `$defs`. Pemeriksaan ini berjalan, dan ketiga halaman rujukan wajib ada, sejak `docs/providers/README.md` ada; sebelum itu dilewati.
- **(e) Issue form.** Setiap `.github/ISSUE_TEMPLATE/*.yml` selain `config.yml`: YAML valid; `name`, `description`, dan `body` (array tidak kosong) ada; setiap elemen `body` punya `type` salah satu dari `markdown`, `input`, `textarea`, `dropdown`, `checkboxes`; elemen `markdown` punya `attributes.value`; elemen lain punya `id` unik per form dan `attributes.label`; `dropdown` dan `checkboxes` punya `attributes.options` yang tidak kosong; `labels` hanya dari tabel *Label triage*. `config.yml` punya `blank_issues_enabled: false` dan satu `contact_links` yang `url` nya URL advisory.
- **(f) `LICENSE-NOTICES.md`.** Untuk setiap folder `providers/<nama>/`, baca `provider.yaml` dengan `serde_yaml_ng` dan ambil `metadata.id` dan `metadata.software`; file yang tidak terbaca atau tanpa field itu adalah satu masalah. Bila `metadata.id` tidak diawali `testing/`: `LICENSE-NOTICES.md` harus ada dengan tiga heading wajib berurutan; di bawah `## Runtime`, setiap nilai `license` tingkat artefak dari entri `runtimes/inventory.toml` yang `provider` nya sama dengan `metadata.software` harus muncul sebagai code span; dan kalimat asal harus cocok dengan `origin` entri itu (`upstream` berarti `Downloaded from the upstream release`, `maintainer` berarti `Built by Stackly maintainers from upstream source`; kedua kalimat bila entri bercampur).
- **(g) Lint.** Tidak ada tag HTML selain `<a>` dan komentar HTML di luar blok kode dan code span; setiap blok berpagar punya bahasa. Ini adalah bagian `.markdownlint.jsonc` yang ditegakkan mesin; sisanya (tabel ringkas, satu H1) dijaga review, karena repo tidak memasang markdownlint.
- **(h) Kalimat izin.** Bila `docs/providers/trying-locally.md` ada, isi `protocols/fixtures/provider/approval-statement.txt` (spasi dan baris baru dinormalkan menjadi satu spasi, tanda blockquote dibuang) harus muncul utuh di halaman itu; file fixture yang tidak ada saat halaman itu ada adalah satu masalah.
- Keluaran: `ok docs <jumlah file>` atau satu baris `path:line: pesan` per masalah, terurut path lalu baris. Exit 0 bersih, 1 ada masalah, 2 bila `providers/`, `schemas/provider/`, `runtimes/inventory.toml`, atau tabel `stackly` tidak bisa dibaca.

`cargo xtask docs walkthrough [--keep]`:

- Menolak berjalan (exit 2) bila ruang bebas di folder temp di bawah 8 GiB, dengan pesan yang menyebut angkanya.
- Membuat folder temp lewat `tempfile`, menjalankan `git clone --local --no-hardlinks --no-checkout <root repo> <temp>/stackly`, lalu `git checkout --detach <sha HEAD>` di clone, dan mencetak sha itu. Perubahan yang belum di-commit tidak ikut, dan perintah mencetak peringatan bila tree kotor.
- Membuat `<temp>/root` dengan mode 0700.
- Membaca `docs/providers/README.md` dari clone itu, mengambil blok `bash` pertama setelah komentar `<!-- docs-walkthrough -->`, dan menjalankan setiap baris bukan komentar dengan `/bin/sh -c` di root clone, berurutan, masing masing di process group sendiri, berhenti di kegagalan pertama.
- Environment: `HOME`, `USER`, `TMPDIR`, `LANG`, serta `CARGO_HOME` dan `RUSTUP_HOME` bila diset, diwarisi; `PATH` diawali `<temp>/stackly/target/debug` supaya `stackly` tanpa path tidak pernah menunjuk salinan terpasang milik user; `STACKLY_ROOT=<temp>/root`; `CARGO_TARGET_DIR=<temp>/stackly/target`; `CARGO_PROFILE_DEV_DEBUG=0`; `CARGO_INCREMENTAL=0`; variabel `STACKLY_*` lain tidak diteruskan. Cargo boleh mengunduh crate bila cache nya kosong; selain itu tidak ada jaringan.
- Setelah semua baris keluar 0: jumlah file `target/provider-test/*/report.json` di clone sama dengan jumlah baris `cargo xtask provider test` di blok, setiap file itu JSON dengan `schema` `stackly.provider-conformance/v1` dan verdict lulus (nama field verdict dibaca dari spec 0036 saat build), dan `git status --porcelain` clone hanya berisi entri `??` di bawah `providers/<name>/` untuk setiap `<name>` dari baris `cargo xtask provider new <name>` di blok. Entri lain apa pun, termasuk `Cargo.lock` yang berubah, adalah kegagalan.
- Keluaran: satu baris per perintah (`run`, lalu `ok` atau `fail <exit code>`), lalu `ok walkthrough <sha> <jumlah report>`. Exit 0 lulus, 1 gagal, 2 prasyarat tidak ada.
- Saat Ctrl+C (handler `ctrlc` yang sudah dipakai xtask) atau kegagalan: SIGINT diteruskan ke process group baris yang sedang berjalan, perintah menunggu grup itu selesai (harness spec 0036 membersihkan root dan prosesnya sendiri), lalu folder temp dihapus kecuali `--keep`. Perintah tidak pernah memanggil `cargo xtask provider test --cleanup` sendiri, karena perintah itu juga mengenai run harness lain milik user yang sama; bila ada sisa, pesan akhir menyarankan menjalankannya dengan tangan. Perintah hanya menghapus folder yang ia buat sendiri.
- Bukti: `cargo xtask evidence new --covers AC-034 --covers REQ-034 --kind fixture --title "Provider contributor walkthrough from a clean clone" --expected "docs walkthrough exits 0 and the harness report exists"`, lalu reviewer mengisi hasil run pada commit itu (spec 0002, *Record manual*).

### Kontrak dari spec 0036 dan 0037

Nilai ini milik spec lain. Developer membacanya dari spec itu di `main` saat build, bukan dari tabel ini; tabel ini hanya mendaftar apa yang panduan butuhkan.

| Nilai | Pemilik | Nilai per 2026-11-11 | Dipakai di |
| --- | --- | --- | --- |
| Scaffold | 0036 | `cargo xtask provider new <name> [--out <dir>]`, menyalin `providers/example/` | `README.md` provider, `authoring.md`, blok walkthrough |
| Validator di repo | 0036 | `cargo xtask provider check [--json] [<dir>...]`, tanpa daemon | `testing.md`, blok walkthrough, template PR |
| Validator lewat daemon | 0036 | `stackly provider validate <dir> [--json]`, exit 0 bila `publishable`, selain itu exit 2 (`cli.provider_invalid`) | `testing.md` |
| Harness | 0036 | `cargo xtask provider test <dir> [--line <line>] [--scenario <name>]... [--report <file>] [--keep]`; exit 0, 1, 2; `stackly provider test` tidak dibuat | `testing.md`, blok walkthrough, template PR |
| Report | 0036 | `target/provider-test/<runId>/report.json`, schema `stackly.provider-conformance/v1`, path di baris terakhir stderr | `testing.md`, `docs walkthrough` |
| Paket contoh | 0036 | `providers/example/` (`testing/example`), berjalan di `stackly-fake-server`, tanpa unduhan | blok walkthrough |
| Isi `tests/` paket | 0036 | hanya `tests/scenarios.yaml` (`stackly.provider-scenarios/v1`) | `authoring.md` |
| Corpus kompatibilitas | 0036 | `cargo xtask provider freeze <dir>` | `authoring.md`, `review.md`, `compatibility.md` |
| Kebijakan kompatibilitas | 0036 | additive tetap di `stackly/v1alpha1`; perubahan yang merusak butuh `apiVersion` baru; versi yang pernah dirilis tetap termuat selama ada salinan pinned; versi tak dikenal ditolak dengan `unsupported_api_version` | `compatibility.md` |
| Import dan approval | 0037 | `stackly provider import <dir> [--dry-run] [--approve] [--yes]`, `stackly provider approve <provider> [--yes]`, `stackly provider revoke <provider> [--yes]`, `stackly provider remove <provider> [--yes]`, `stackly provider list`, `stackly provider inspect <provider>`; semuanya butuh daemon; `<provider>` adalah ID lengkap | `trying-locally.md` |
| Padanan GUI | 0037 | Catalog, tombol "Import Provider…", sheet "Import Provider", "Approve…", "Revoke Approval…", "Remove…" | `trying-locally.md` |
| Batas import | 0037 | folder paket saja; namespace selain `official`, `testing`, `stackly`; harness tidak butuh import atau approval | `trying-locally.md`, `authoring.md` |
| Istilah | 0037 | trust `official`, `development`, `local`; approval `pending`, `approved`, `revoked`; frasa terlarang di *`GOVERNANCE.md`* bagian 3 | `trying-locally.md`, `GOVERNANCE.md` bagian 3, `review.md`, `SECURITY.md` |
| Kalimat izin | 0037 | isi `protocols/fixtures/provider/approval-statement.txt` | `trying-locally.md`, pemeriksaan (h) |
| Revocation | 0037 | dua sumber (pengguna dan daftar `revoked` catalog bawaan); hanya menutup install dan instance baru; data, runtime, dan instance lama tidak dihapus | `SECURITY.md` proses langkah 4, `GOVERNANCE.md` bagian 4 |

### Data model sketch

Tidak ada tabel, migrasi, atau DTO baru. Protokol IPC tidak berubah.

### API surface

| Perintah | Input | Output | Exit code |
| --- | --- | --- | --- |
| `cargo xtask docs check` | root repo | `ok docs <n>` atau baris `path:line: pesan` | 0, 1, 2 |
| `cargo xtask docs walkthrough [--keep]` | `HEAD` repo | baris per perintah, lalu `ok walkthrough <commit> <report>` | 0, 1, 2 |

### Value sourcing

| Dokumen atau aksi | Nilai | Sumber |
| --- | --- | --- |
| Semua dokumen | URL repo | `git remote`: `https://github.com/stackly-dev/stackly` |
| `README.md`, `CONTRIBUTING.md` | Nama lisensi dan klausa kontribusi | K1, K2, teks di *Lisensi* |
| `CONTRIBUTING.md` | Prasyarat dan perintah build, cek, test | `README.md` dan bagian Commands `AGENTS.md` pada commit build |
| `CONTRIBUTING.md` | Variabel dev root | `STACKLY_ROOT`, `STACKLY_LOG` (spec 0006) |
| `CODE_OF_CONDUCT.md` | Teks | Contributor Covenant 2.1 dari sumber resminya |
| `CODE_OF_CONDUCT.md` | Kontak | I1 |
| `SECURITY.md` | URL kanal | `https://github.com/stackly-dev/stackly/security/advisories/new` |
| `SECURITY.md` | Batas kepercayaan | spec 0006 (satu akun user OS), spec 0011 (secret hanya di Keychain) |
| `SECURITY.md` | Target waktu | K4: 7, 14, dan 90 hari |
| `SECURITY.md` | Alur pencabutan revision provider | spec 0037 |
| `GOVERNANCE.md` | Pemegang peran | I2 |
| `GOVERNANCE.md` | Daftar provider | folder `providers/` selain namespace `testing` |
| Issue form | Label | tabel *Label triage* |
| Halaman rujukan | Nama property dan nilai enum | `schemas/provider/*.json` hasil `cargo xtask codegen` |
| Halaman rujukan | Arti dan batas field | spec 0009 dan komentar doc tipe manifest di `stackly-core` |
| Halaman rujukan | Contoh | potongan dari `providers/nats`, `providers/postgresql`, `providers/traefik` |
| `testing.md`, `authoring.md`, blok walkthrough | Perintah, exit code, report, paket contoh | spec 0036 (tabel kontrak) |
| `compatibility.md` | Kebijakan | spec 0036 |
| `trying-locally.md` | Perintah, batas import, istilah | spec 0037 (tabel kontrak) |
| `trying-locally.md` | Kalimat izin | `protocols/fixtures/provider/approval-statement.txt` (spec 0037) |
| `LICENSE-NOTICES.md` | Lisensi runtime dan asal binary | `license`, `license_url`, `origin` di `runtimes/inventory.toml` |
| `docs walkthrough` | Path dan isi report | `target/provider-test/*/report.json` di clone (spec 0036) |
| `docs walkthrough` | Commit yang diuji | `git rev-parse HEAD` di clone |
| Record bukti AC-8 | Context run | `cargo xtask evidence new` (spec 0002) |

### Key invariants

- Dokumen publik tidak pernah memuat perintah, URL, digest, alamat kontak, atau nama pemegang peran karangan. Nilai yang belum ada berarti baris itu belum ditulis dan langkahnya `BLOCKED`.
- Dokumen publik tidak menjanjikan hal yang belum ada: tidak ada klaim CI, rilis bertanda tangan, registry komunitas, atau bahwa `official` berarti aman.
- Panduan provider tidak mengulang kontrak spec 0036 dan 0037 dengan kata sendiri di luar yang perlu untuk dijalankan; bila berbeda, spec itu yang menang dan dokumen diperbaiki.
- `docs check` dan `docs walkthrough` tidak pernah menulis ke root repo atau ke Application Support asli. `docs check` tidak menjalankan perintah dari dokumen dan tidak membuka jaringan; `docs walkthrough` hanya membuka jaringan lewat cargo saat cache crate kosong.
- Menambah `README.md` atau `LICENSE-NOTICES.md` ke paket tidak mengubah digest paket (spec 0009).
- Kerentanan tidak pernah dibahas di issue, PR, atau commit publik sebelum advisory terbit.

### Security model

- Laporan kerentanan hanya terlihat oleh pelapor dan admin repo (model akses GitHub Security Advisory). Tidak ada rahasia yang disimpan di repo untuk proses ini.
- Repo tidak punya CI, jadi tidak ada secret yang bisa bocor ke PR dari fork. Maintainer memperlakukan paket provider dari PR sebagai input tidak tepercaya: dibaca dulu, dijalankan hanya lewat harness di test root, dan tidak diimpor ke Stackly asli sebelum review (AC-12).
- Issue form mengingatkan pelapor agar tidak menempel secret, dan menunjuk diagnostic bundle yang sudah disensor.
- `docs walkthrough` menjalankan perintah dari `docs/providers/README.md` pada commit yang di-checkout, dengan `STACKLY_ROOT` di folder temp. Pada branch PR itu berarti menjalankan isi PR, sama seperti `cargo test`; `review.md` meminta diff dibaca lebih dulu.

### Configuration required

Tidak ada variabel environment baru. Pengaturan GitHub yang dibutuhkan ada di *Input dari pemilik repo*.

### Critical test scenarios

- Happy path: `cargo xtask docs check` di repo keluar 0 dan mencetak `ok docs <n>`, verifies **AC-9**, **AC-2**, **AC-7**, **AC-14**.
- Happy path: `cargo xtask docs walkthrough` keluar 0 dengan report harness dan clone yang bersih, verifies **AC-8**.
- Tidak ada positif palsu: code span `<data>` dan `<runId>`, komentar `# ...` di akhir baris perintah, `STACKLY_ROOT=$(mktemp -d) cargo run -q -p stacklyd -- --print-roots`, dan `cargo run -q -p stackly-cli -- daemon status --json` tidak menghasilkan masalah, verifies **AC-9**.
- Failure case: fixture dengan link ke file yang tidak ada, anchor yang salah, placeholder `TBD`, perintah `stackly provider nope` dan `cargo xtask nope`, property schema yang tidak disebut, issue form tanpa `body`, label di luar tabel, paket tanpa `LICENSE-NOTICES.md`, tag `<br>`, dan blok kode tanpa bahasa masing masing menghasilkan satu baris masalah yang menyebut path, baris, dan penyebabnya, verifies **AC-9**, **AC-10**.
- Failure case: report harness dengan verdict gagal, atau perubahan di luar `providers/` di clone, membuat `docs walkthrough` keluar 1, verifies **AC-8**.
- Failure case: blok walkthrough yang satu barisnya gagal membuat `docs walkthrough` berhenti di baris itu dengan exit 1 dan folder temp terhapus, verifies **AC-8**.
- Failure case: ruang bebas di bawah batas membuat `docs walkthrough` keluar 2 tanpa clone, verifies **AC-8**.
- Lisensi: `cargo metadata` menunjukkan `MIT OR Apache-2.0` untuk setiap anggota workspace dan `cargo deny check licenses` lulus, verifies **AC-1**.
- Failure case: `trying-locally.md` yang kalimat izinnya berbeda satu kata dari fixture menghasilkan satu masalah, verifies **AC-12**.
- Isi: review terhadap daftar bagian wajib tiap dokumen, verifies **AC-3**, **AC-4**, **AC-6**, **AC-11**, **AC-12**, **AC-13**.
- Manual (owner user): balasan `gh api` untuk private vulnerability reporting dan tab Community Standards, verifies **AC-5**.

## Build plan

Pendekatan build proyek adalah Journey. Untuk fitur ini artinya dua lintasan pembaca dibangun utuh satu per satu: lebih dulu lintasan siapa pun yang datang ke repo (lisensi, aturan, keamanan, template), lalu lintasan kontributor provider dari clone sampai PR.

Bagian A, tidak bergantung pada fitur 36 atau 37:

1. Catatan lisensi: `repository` di `[workspace.package]`, bagian `License` di `README.md` dengan klausa kontribusi, dan `cargo deny check licenses`, satisfies **AC-1**.
2. `cargo xtask docs check` dengan pemeriksaan (a), (b), (c), (e), (f), (g) dan test fixture rusaknya; pemeriksaan (d) ditulis tetapi hanya berjalan untuk halaman rujukan yang ada, satisfies **AC-9**.
3. `CODE_OF_CONDUCT.md` (butuh I1 dan teks resmi), satisfies **AC-3**.
4. `SECURITY.md` dan `.github/ISSUE_TEMPLATE/config.yml`, satisfies **AC-4**.
5. `GOVERNANCE.md` (butuh I2), satisfies **AC-6**.
6. Tiga issue form dan `.github/PULL_REQUEST_TEMPLATE.md`, satisfies **AC-7**.
7. `CONTRIBUTING.md` (bagian 6 berisi link yang halamannya datang di langkah 9; sampai saat itu bagian 6 menunjuk `providers/` dan spec 0009), `LICENSE-NOTICES.md` untuk tiga paket resmi, dan pembaruan `README.md`, satisfies **AC-2**, **AC-13**, **AC-14**.

Bagian B, needs feature 36:

8. Tiga halaman rujukan schema dan pemeriksaan (d) menjadi wajib, satisfies **AC-10**.
9. `docs/providers/README.md` dengan blok walkthrough, `authoring.md`, `testing.md`, `compatibility.md`, `review.md`, dan bagian 6 `CONTRIBUTING.md` menunjuk ke sana, satisfies **AC-11**, **AC-2**.
10. `cargo xtask docs walkthrough`, test nya, satu run penuh, dan record manual, satisfies **AC-8**.

Bagian C, needs feature 37:

11. `docs/providers/trying-locally.md` dan pemeriksaan (h); istilah trust di `GOVERNANCE.md` bagian 3 serta `review.md` disamakan dengan spec 0037; dan kalimat tentang alur revocation spec 0037 ditambahkan ke `SECURITY.md` (proses, langkah 4) dan `GOVERNANCE.md` bagian 4, satisfies **AC-12**, **AC-4**, **AC-6**.

Langkah manual pemilik repo (private vulnerability reporting, label, Community Standards) ditulis ke `verify.md` saat langkah 4 dan 6 dibangun, satisfies **AC-5**.

**Sesi paralel**: bagian A hanya menyentuh file baru di root, `.github/`, `providers/*/LICENSE-NOTICES.md`, `README.md`, satu baris `Cargo.toml`, dan modul baru `xtask/src/docs/` plus satu cabang di `xtask/src/main.rs` dan satu dependency di `xtask/Cargo.toml`. Ia bisa dibangun bersamaan dengan build fitur 36, dengan satu titik temu: bila fitur 36 juga menambah subperintah xtask atau mengubah `xtask/Cargo.toml`, kedua sesi menyentuh `xtask/src/main.rs` dan `Cargo.lock`, jadi lead menentukan urutan merge dan sesi kedua menjalankan `git merge main` lebih dulu. Bagian B menunggu fitur 36 ada di `main`, bagian C menunggu fitur 37. Di dalam bagian A, langkah 3 sampai 7 hanya menulis Markdown dan YAML dan bisa dikerjakan satu sesi tanpa build Rust selain `docs check`.

## Consequences

**Positive**:

- OQ-013 tertutup dengan catatan di repo, dan kontributor tahu lisensi kontribusinya sebelum menulis.
- Ada kanal keamanan privat dan proses tertulis tanpa infrastruktur baru.
- Panduan dijaga mesin: link, nama perintah, dan cakupan schema gagal keras saat basi, dan AC-034 punya bukti yang bisa diulang.
- Lima provider resmi berikutnya memakai panduan dan daftar periksa yang sama dengan kontributor.

**Negative / tradeoffs**:

- Halaman rujukan ditulis tangan, jadi setiap penambahan field schema butuh suntingan docs di PR yang sama; pemeriksaan cakupan hanya menjamin nama disebut, bukan penjelasannya benar.
- `docs check` membangun `stackly-cli` sekali untuk membaca daftar perintahnya (`manpage --json`), sehingga lebih lambat dari pemeriksaan teks murni; daftar tugas xtask bergantung pada bentuk teks `USAGE`.
- `docs walkthrough` membangun dari nol di clone baru, butuh beberapa GiB dan beberapa menit, sehingga dijalankan saat verifikasi dan sebelum rilis, bukan di setiap commit.
- Kanal keamanan bergantung pada GitHub dan pada satu orang; target waktu ditulis longgar supaya jujur.
- Tanpa DCO atau CLA, asal usul kontribusi hanya dijaga klausa dan kotak centang di template PR.
- Spec dan scope tetap berbahasa Indonesia, jadi kontributor yang ingin memahami keputusan desain harus meminta ringkasan.
- Tiga nilai dan tiga langkah GitHub bergantung pada pemilik repo; sampai ada, AC-3 dan AC-5 `BLOCKED`.

**Neutral**:

- `xtask` mendapat dependency `serde_yaml_ng` dan subperintah `docs`; `AGENTS.md` perlu mencatat keduanya lewat `/sync`.
- Paket resmi mendapat `LICENSE-NOTICES.md`; provider baru wajib membawanya.
- `README.md` berubah, dan `Cargo.toml` akhirnya menunjuk repo `stackly-dev/stackly`.

## Follow-up

- [ ] Spec 0001 (usulan update, pemiliknya): tandai Follow-up "OQ-013: konfirmasi lisensi `MIT OR Apache-2.0`" sudah diputuskan spec 0038 (K1), tandai baris Consequences tentang file governance sudah dijawab spec 0038, catat `repository` sudah menunjuk `stackly-dev/stackly` setelah langkah 1, dan tambahkan modul `docs` serta dependency `serde_yaml_ng` ke baris `xtask` di *Peta crate*.
- [ ] Spec 0008 (usulan update, pemiliknya): Follow-up "Fitur 38 atau 66" dipecah. Fitur 38 hanya menulis exit code perintah provider dan variabel dev root di panduan kontributor. Rujukan CLI umum (envelope, tabel exit code, semua variabel environment) dan pertimbangan JSON Schema envelope di `schemas/cli/` menjadi milik fitur 66; rekomendasi spec ini: schema envelope hanya dibuat bila ada konsumen yang memvalidasinya.
- [ ] Spec 0009 (usulan update, pemiliknya): catat bahwa `LICENSE-NOTICES.md` wajib untuk paket di `providers/` selain namespace `testing` menurut spec 0038, tetap tidak dibaca loader, dan bahwa schema docs untuk `stackly/v1alpha1` tinggal di `docs/providers/`.
- [ ] Spec 0036 (usulan, pemiliknya): `providers/example/README.md` menunjuk `docs/providers/README.md`; path `target/provider-test/<runId>/report.json`, schema `stackly.provider-conformance/v1`, dan empat perintah di blok walkthrough dianggap stabil, dan perubahan padanya ikut mengubah `docs/providers/` di commit yang sama (`docs check` dan `docs walkthrough` menangkapnya). Kontrak ini diterima dari `arc-f36-spec` pada 2026-11-11 sebelum spec 0036 di-accept; developer membaca ulang spec 0036 di `main` saat build.
- [ ] Spec 0037 (usulan, pemiliknya): `protocols/fixtures/provider/approval-statement.txt` adalah sumber kalimat izin yang juga dibaca `docs check` (h), jadi path itu dianggap stabil; pencabutan revision karena advisory memakai daftar `revoked` catalog bawaan spec itu. Kontrak ini diterima dari `arc-f37-spec` pada 2026-11-11 sebelum spec 0037 di-accept; developer membaca ulang spec 0037 di `main` saat build.
- [ ] Fitur 66: `GOVERNANCE.md` bagian Issue triage adalah catatan support ownership; dokumentasi pengguna menunjuk ke sana alih alih menulis tabel kedua.
- [ ] Fitur 60: notice lisensi rilis dan SBOM; periksa apakah `LICENSE-NOTICES.md` paket perlu ikut bundel app.
- [ ] Pemilik repo: I1, konfirmasi I2, I3 opsional, dan tiga langkah GitHub di *Input dari pemilik repo*.
- [ ] Pemilik repo: peninjauan hukum atas distribusi binary pihak ketiga tetap terbuka (PRD 39); spec ini bukan pendapat hukum.
- [ ] `/sync` setelah build: catat `cargo xtask docs check` dan `cargo xtask docs walkthrough` di Commands `AGENTS.md`, aturan dokumen publik berbahasa Inggris, dan kewajiban `LICENSE-NOTICES.md`.
- [ ] Saat ada pemegang peran kedua: tambah `.github/CODEOWNERS` dan perbarui tabel peran.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, dan alasannya ada di [rationale.md](rationale.md).
