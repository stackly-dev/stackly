# Verify: Panduan kontributor & governance · spec 0038 · updated 2026-11-11

_Langkah diturunkan dari acceptance criteria spec 0038. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27, dengan `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`. Probe yang mengubah file sementara selalu dikembalikan sesudahnya (`git status` bersih lagi)._

## Milestone 1: License record, `cargo xtask docs check`, and the community files

Langkah Build plan 1 sampai 7 (bagian A). Langkah 3 (`CODE_OF_CONDUCT.md`) `BLOCKED`: `reason` butuh I1 (alamat kontak Code of Conduct) dan teks resmi Contributor Covenant 2.1, yang tidak boleh diunduh sesi build dan tidak boleh ditulis dari ingatan; `owner` user. File itu sengaja tidak dibuat.

### UI / manual

- [x] Buka `CONTRIBUTING.md` → sembilan bagian berurutan: `Ways to contribute`, `Before you start`, `Set up`, `Build and check locally`, `Run from source safely`, `Contributing a provider`, `Pull requests`, `Where decisions live`, `Licensing`; semuanya bahasa Inggris; bagian `Licensing` memuat klausa kontribusi kata demi kata dari spec bagian *Lisensi* dan kalimat bahwa lisensi Stackly tidak mencakup software pihak ketiga; bagian 6 menunjuk spec 0009 dan `providers/` (bukan `docs/providers/`, yang datang di langkah 9) → AC-1, AC-2
- [x] Buka `README.md` → tidak ada lagi "early scaffold"; ada bagian `Contributing` (link `CONTRIBUTING.md`), `Security` (link `SECURITY.md`), dan `License` dengan klausa kontribusi kata demi kata → AC-1, AC-14
- [x] Buka `SECURITY.md` → bagian `Reporting a vulnerability` (tombol Report a vulnerability, URL `https://github.com/stackly-dev/stackly/security/advisories/new`, larangan issue, PR, dan diskusi publik, tanpa email cadangan karena I3 tidak ada), `What to include`, `Scope` (di dalam dan di luar cakupan, batas satu akun user macOS), `Supported versions`, `What to expect` (7, 14, 90 hari sebagai target), `Our process` tujuh langkah → AC-4
- [x] Buka `GOVERNANCE.md` → bagian `Roles` (lima peran, pemegang `@ojiepermana` sebagai default I2, kalimat bahwa satu orang memegang semua peran, tanpa `CODEOWNERS`), `How decisions are made`, `Official and reviewed`, `Provider owners` (tiga provider `official/`, kewajiban, cara menjadi owner, owner tidak aktif), `Issue triage` (empat kategori dan labelnya), `Compatibility and deprecation`, `Review without CI`, `Changing this document`; tidak ada "untrusted", "trusted publisher", "verified publisher", "sandboxed", atau "safe to run" → AC-6
- [x] Buka `.github/ISSUE_TEMPLATE/` → `bug.yml` (dropdown area dengan empat pilihan spec, versi Stackly, versi macOS, provider opsional, langkah, harapan, hasil, `stackly doctor` opsional dengan peringatan secret, label `needs-triage`), `provider-proposal.yml` (nama, link upstream, lisensi, binary macOS arm64, kesediaan menjadi owner, label `needs-triage` dan `provider`), `configuration-help.yml` (apa yang dicoba, provider, config tanpa secret, label `needs-triage` dan `configuration`), dan `config.yml` (`blank_issues_enabled: false`, contact link `Report a security vulnerability` ke URL advisory) → AC-4, AC-7
- [x] Buka `.github/PULL_REQUEST_TEMPLATE.md` → bagian `What and why`, `Kind of change` (lima kotak), `Checks I ran locally` (kalimat "This repository has no CI", satu kotak per perintah bagian 4 `CONTRIBUTING.md`, blok untuk keluaran), `Provider checklist`, `Licensing` (kotak hak dan lisensi `MIT OR Apache-2.0`) → AC-7
- [x] Buka `providers/nats/LICENSE-NOTICES.md`, `providers/postgresql/LICENSE-NOTICES.md`, `providers/traefik/LICENSE-NOTICES.md` → tiga heading `## Package`, `## Runtime`, `## Third party material in this package` berurutan; lisensi `Apache-2.0`, `PostgreSQL`, `MIT` sebagai code span; kalimat asal `Downloaded from the upstream release` (nats, traefik) dan `Built by Stackly maintainers from upstream source` (postgresql), sama dengan `origin` di `runtimes/inventory.toml`; kalimat penutup yang menunjuk `runtimes/inventory.toml`; `None.` untuk materi pihak ketiga → AC-13
- [ ] Langkah manual pemilik repo (owner user), tidak dijalankan sesi mana pun: aktifkan private vulnerability reporting di Settings, Code security, Private vulnerability reporting, Enable, lalu `gh api repos/stackly-dev/stackly/private-vulnerability-reporting` → `{"enabled":true}`. Per 2026-11-11 balasannya `{"enabled":false}`, jadi AC-5 `BLOCKED` dan kanal belum boleh disebut tersedia. Sebaiknya aktifkan sebelum `SECURITY.md` di-push → AC-5
- [ ] Langkah manual pemilik repo (owner user): buat lima label triage, satu perintah per label: `gh label create needs-triage --repo stackly-dev/stackly --description "Waiting for a maintainer to sort it" --color ededed`, lalu `core` ("Core bug: app, daemon, CLI, protocol", `d73a4a`), `provider` ("Provider package bug or proposal", `0075ca`), `upstream` ("Incompatibility in an upstream runtime", `fbca04`), `configuration` ("Configuration question", `c5def5`). Form tetap berfungsi tanpa label, tetapi label tidak terpasang otomatis → AC-7
- [ ] Langkah manual pemilik repo (owner user), setelah file di `main` dan ter-push (push adalah keputusan pemilik repo): tab Insights, Community Standards menandai Contributing, License, Security policy, Issue templates, dan Pull request template ada; Code of conduct baru ada setelah langkah 3 → AC-3, AC-4, AC-5, AC-7
- [ ] Langkah 3, owner user: berikan I1, lalu tambahkan `CODE_OF_CONDUCT.md` berisi teks resmi Contributor Covenant 2.1 (lewat Add file, Code of conduct di GitHub, atau serahkan filenya ke sesi berikutnya). Catat URL sumber teks dan SHA-256 file sebelum baris kontak diisi di bagian ini. Setelah itu `CONTRIBUTING.md` bagian `Before you start` bisa menautkan file itu → AC-3

### Commands

- [x] `cargo metadata --format-version 1 --no-deps | python3 -c "import json,sys; print({(p['license'], p['repository']) for p in json.load(sys.stdin)['packages']})"` → satu pasangan saja, `('MIT OR Apache-2.0', 'https://github.com/stackly-dev/stackly')` → AC-1
- [x] `cargo deny check licenses` → `licenses ok` → AC-1
- [x] `cargo xtask docs check` → `ok docs 16`, exit 0 (16 file: `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `GOVERNANCE.md`, empat file di `.github/ISSUE_TEMPLATE/`, `.github/PULL_REQUEST_TEMPLATE.md`, empat `providers/*/README.md`, tiga `LICENSE-NOTICES.md`) → AC-2, AC-7, AC-9, AC-13, AC-14
- [x] `cargo test -p xtask --bin xtask docs::` → 42 test lulus: satu fixture rusak per pemeriksaan dengan pesan persisnya, yaitu (a) link ke file yang tidak ada dan anchor yang salah, (b) `TBD` di luar code, (c) `cargo xtask nope`, `cargo xtask provider nope`, `stackly provider nope`, dan `cargo run -q -p stackly-cli -- nope`, (d) property schema yang tidak disebut (dan dilewati sebelum `docs/providers/README.md` ada), (e) form tanpa `body`, label di luar tabel, id ganda, label dan options kosong, YAML rusak, `config.yml` tanpa link advisory, (f) paket tanpa `LICENSE-NOTICES.md`, heading tidak berurutan, lisensi dan kalimat asal yang salah, manifest tanpa metadata, (g) tag `<br>` dan blok tanpa bahasa, (h) kalimat izin yang beda satu kata; dan tanpa positif palsu untuk `<data>`, `<runId>`, komentar `# ...`, `STACKLY_ROOT=$(mktemp -d) cargo run -q -p stacklyd -- --print-roots`, dan `cargo run -q -p stackly-cli -- daemon status --json` → AC-9, AC-10
- [x] `cargo test -p xtask --test docs_check` → 2 test lulus: binary nyata membaca tabel `stackly` lewat `stackly manpage --json` dan repo ini bersih; tugas `docs nope` ditolak → AC-9, AC-14
- [x] Tambah sementara `TODO` ke `README.md`, jalankan `cargo xtask docs check` → satu baris `README.md:<baris>: placeholder `TODO``, exit 1; kembalikan → AC-9
- [x] Ganti sementara `cargo xtask catalog check` di `CONTRIBUTING.md` dengan `cargo xtask catalog nope` → satu baris `CONTRIBUTING.md:<baris>: unknown xtask task `cargo xtask catalog nope``, exit 1; kembalikan → AC-2, AC-9
- [x] Pindahkan sementara `providers/nats/LICENSE-NOTICES.md` ke luar repo → `providers/nats/LICENSE-NOTICES.md:1: `LICENSE-NOTICES.md` is missing; `official/nats` is not a testing package`, exit 1; kembalikan → AC-9, AC-13
- [x] Ganti sementara nama `runtimes/inventory.toml` → `cargo xtask docs check` mencetak `error: cannot read ...` dan keluar 2; kembalikan → AC-9
- [x] `cargo xtask provider check` → empat baris `ok` dengan digest yang sama seperti di `main` (`official/nats 1.0.1 8e30…`, `official/postgresql 1.0.2 389b…`, `official/traefik 1.0.0 cbb4…`), dan `git diff main -- providers/catalog-revisions.json` kosong; `cargo xtask catalog check` → `ok catalog revision 11` → AC-13
- [x] `cargo xtask help` → baris `docs check` ada di daftar tugas → AC-9
- [x] `cargo clippy -p xtask --all-targets --locked -- -D warnings`, `cargo xtask check-deps`, `cargo fmt --all --check` → lulus

### Hasil `/check verify` 2026-11-11 (Milestone 1, commit `97f2b5cd`)

Verdict: **FAIL**, satu langkah gagal (`GOVERNANCE.md`, AC-6). Enam langkah isi dan dua belas langkah perintah lulus dan dicentang. Empat langkah pemilik repo tetap terbuka (AC-3 dan AC-5 `BLOCKED`, owner user).

- **FAIL, AC-6:** `GOVERNANCE.md` baris 37 menulis `official/traefik` berstatus `In the bundled catalog`, dan baris 26 mendefinisikan `official` sebagai paket yang `ships in the bundled catalog`. Catalog bawaan hanya mendaftar Traefik sebagai `planned`: `cargo xtask catalog build --out <scratch>` mencetak `catalog: revision 11, 2 provider(s), 4 planned`, `catalog.json` punya `providers` = `official/nats`, `official/postgresql` dan `planned` memuat `official/traefik`, dan folder `packages/official/` hanya berisi `nats` dan `postgresql`. Ini melanggar Key invariant (dokumen publik tidak menjanjikan hal yang belum ada) dan bertentangan dengan `README.md` baris 9 (`Traefik has a first package`). Bagian lain `GOVERNANCE.md` sesuai spec: delapan bagian berurutan, lima peran, kalimat satu orang, tanpa `CODEOWNERS`, tabel triage empat kategori, dan tidak ada lima frasa terlarang. Perbaikan milik developer: status Traefik ditulis sebagai planned (paket ada di repo, belum bisa dipasang dari catalog).
- **Probe file rusak (semua dikembalikan, `git status` bersih):** `TODO` di `README.md` → `README.md:75: placeholder `TODO``, exit 1. `cargo xtask catalog nope` di `CONTRIBUTING.md` → `CONTRIBUTING.md:54: unknown xtask task `cargo xtask catalog nope``, exit 1. `providers/nats/LICENSE-NOTICES.md` dipindah → pesan persis di langkahnya, exit 1, dan `cargo xtask provider check` tetap mencetak digest `8e30…` untuk `official/nats`, jadi file itu tidak masuk digest. `runtimes/inventory.toml` diganti nama → `error: cannot read …/runtimes/inventory.toml: No such file or directory (os error 2)`, exit 2.
- **Probe tambahan pada repo nyata (dikembalikan):** link ke file yang tidak ada, anchor salah, `<br>`, blok tanpa bahasa, `stackly provider nope`, `cargo run -q -p stackly-cli -- nope`, `cargo xtask nope`, dan `cargo xtask provider nope` di `README.md` masing masing memberi satu baris, sedangkan `stackly daemon status --json   # fine` dan `STACKLY_ROOT=$(mktemp -d) cargo run -q -p stacklyd -- --print-roots` tidak dilaporkan. Label `security` di `bug.yml` → `label `security` is not a triage label`; `config.yml` tanpa URL advisory → satu baris. Lisensi dan kalimat asal yang salah di notice Traefik → tiga baris, termasuk kalimat asal yang tidak punya entri inventaris.
- **AC-5, `BLOCKED`, owner user:** `gh api repos/stackly-dev/stackly/private-vulnerability-reporting` membalas `{"enabled":false}` pada 2026-11-11, dan `gh label list` belum memuat lima label triage. `SECURITY.md` sudah menyebut tombol Report a vulnerability, jadi sebaiknya fitur itu diaktifkan sebelum file ini di-push.
- **AC-3, `BLOCKED`, owner user:** `CODE_OF_CONDUCT.md` belum ada (butuh I1 dan teks resmi).
- **Tafsiran developer yang dinilai memenuhi teks AC:** Code of Conduct disebut tanpa link di `CONTRIBUTING.md` (link menyusul langkah 3); `USAGE` hanya memuat `docs check` (`docs walkthrough` menyusul langkah 10); `cargo xtask provider test` tidak ada di blok `bash` `CONTRIBUTING.md` (perintahnya belum ada di `main`) dan hanya code span di template PR, sesuai teks spec; link di `.github/` memakai URL absolut (anchor `#build-and-check-locally` dicek tangan, karena pemeriksaan (a) tidak membaca URL `https`); `README.md` menunda link dan baris Layout `docs/providers/`; `GOVERNANCE.md` memakai `@ojiepermana` (satu satunya penulis di riwayat commit) dan menyatakan halaman kompatibilitas belum ditulis; parser `USAGE` berhenti di jarak dua spasi dan memberi jalur yang sama dengan aturan spec; pemeriksaan (f) juga menolak kalimat asal tanpa entri inventaris.
- **Catatan untuk langkah berikutnya:** langkah 9 perlu menambah link `docs/providers/README.md` dan baris Layout di `README.md` serta link `compatibility.md` di `GOVERNANCE.md` (Build plan hanya menyebut bagian 6 `CONTRIBUTING.md`). Di template PR, tiga perintah Swift digabung dalam satu kotak dan `cargo xtask catalog check` ada di `Provider checklist`, bukan di `Checks I ran locally`. `cargo deny check licenses` lulus dengan peringatan `unmatched license allowance` untuk `NCSA`.

### Cek ulang 2026-11-11 (commit `2fa3cc0d`, range `568be828..2fa3cc0d`)

Verdict: **PASS** untuk AC-6 dan untuk Milestone 1 di luar langkah pemilik repo (AC-3 dan AC-5 tetap `BLOCKED`, owner user).

- **AC-6, met:** `GOVERNANCE.md` baris 37 sekarang menulis `official/traefik` sebagai `Package in this repository; listed as planned in the bundled catalog`, dan baris 26 mendefinisikan `official` sebagai `ships in the bundled catalog or is listed there as planned`. Ini cocok dengan `cargo xtask catalog build --out <scratch>` pada commit ini: `catalog: revision 11, 2 provider(s), 4 planned`, `providers` = `official/nats`, `official/postgresql`, dan `planned` memuat `official/traefik`. Delapan bagian tetap berurutan, dan lima frasa terlarang tidak ada. Langkah `GOVERNANCE.md` dicentang.
- **Template PR, AC-7, tetap met:** `Checks I ran locally` sekarang punya dua belas kotak, satu untuk setiap dari dua belas perintah di `CONTRIBUTING.md` bagian `Build and check locally` (termasuk `cargo xtask catalog check`, dua `swift test`, dan `cargo xtask build-app`), kalimat `This repository has no CI`, dan blok `text` untuk keluaran. Catatan rendah di hasil pertama tentang kotak yang digabung sudah selesai.
- **`cargo xtask docs check`** → `ok docs 16`, exit 0. Sejak verify pertama hanya `GOVERNANCE.md` (dua baris) dan `.github/PULL_REQUEST_TEMPLATE.md` yang berubah di antara dokumen publik dan `xtask/`, jadi langkah lain tidak diulang.

### Acceptance-criteria coverage

- AC-1: langkah `cargo metadata`, `cargo deny check licenses`, dan isi `README.md` serta `CONTRIBUTING.md` bagian `Licensing`.
- AC-2: isi `CONTRIBUTING.md` dan `docs check` bersih (setiap perintah `cargo xtask` dan `stackly` di blok `bash` ada). Bagian provider penuh (link ke `docs/providers/README.md` dan `cargo xtask provider test`) menunggu Milestone berikutnya, langkah 9, setelah fitur 36.
- AC-3: `BLOCKED`, owner user (I1 dan teks resmi). `docs check` sudah memeriksa `[INSERT` dan link bila file itu ada, dan tetap hijau selama file itu belum ada.
- AC-4: isi `SECURITY.md` dan `config.yml`. Kalimat revocation spec 0037 di proses langkah 4 menunggu langkah 11 (fitur 37).
- AC-5: `BLOCKED`, langkah manual owner user di atas.
- AC-6: isi `GOVERNANCE.md`, dengan I2 memakai default `@ojiepermana` (akun pemilik organisasi `stackly-dev`), menunggu konfirmasi user. Istilah trust spec 0037 di bagian 3 dan rujukan revocation di bagian 4 menunggu langkah 11.
- AC-7: tiga form, `config.yml`, template PR, pemeriksaan (e) di `docs check`; label di GitHub adalah langkah manual.
- AC-8: menunggu langkah 10 (fitur 36).
- AC-9: `docs check` dengan pemeriksaan (a) sampai (h) dan test fixture rusaknya. Pemeriksaan (d) sudah ditulis dan berjalan begitu `docs/providers/README.md` ada; pemeriksaan (h) berjalan begitu `docs/providers/trying-locally.md` ada.
- AC-10, AC-11: menunggu langkah 8 dan 9 (fitur 36).
- AC-12: menunggu langkah 11 (fitur 37).
- AC-13: tiga `LICENSE-NOTICES.md`, pemeriksaan (f), dan digest paket yang tidak berubah.
- AC-14: `docs check` pemeriksaan (g) dan isi `README.md`. Baris tabel Layout `docs/` yang menyebut `docs/providers/` ditunda ke langkah 9, karena folder itu belum ada.

## Milestone 2: Provider guide and schema reference

Langkah Build plan 8 saja: tiga halaman rujukan schema dan pemeriksaan (d) menjadi wajib. Langkah 9 (`docs/providers/README.md` dengan blok walkthrough, `authoring.md`, `testing.md`, `compatibility.md`, `review.md`, bagian 6 `CONTRIBUTING.md`) menyusul setelah `cargo xtask provider test` ada di `main` (spec 0036 langkah 13), karena pemeriksaan (c) menolak perintah yang belum ada.

Tafsiran: Build plan langkah 8 berkata pemeriksaan (d) "menjadi wajib", sedangkan *`cargo xtask docs`* (d) berkata pemeriksaan berjalan sejak `docs/providers/README.md` ada. Langkah 8 datang sebelum `README.md` itu (langkah 9), jadi (d) sekarang berjalan tanpa syarat dan ketiga halaman wajib ada; kalimat spec tetap benar sesudah langkah 9.

### UI / manual

- [x] Buka `docs/providers/manifest-reference.md` → bahasa Inggris; bagian per blok manifest (`metadata`, `compatibility`, `runtime`, `capabilities`, `resources`, `configFiles`, `lifecycle` dengan command, langkah initialize, `stop`, `reload`, `readiness` dan `health` dengan empat jenis probe, `timeouts`, `logs`, `backup`, `permissions`, `connection`, `openWith`, `remedies`) dan tabel token; setiap field dengan arti, wajib atau tidak, dan batasnya sesuai spec 0009, spec 0034 (`remedies`), dan spec 0070 (`openWith`); `apiVersion` menyebut `SUPPORTED_API_VERSIONS` dan `unsupported_api_version`; link ke `schemas/provider/provider.v1alpha1.schema.json` → AC-10, AC-14
- [x] Buka `docs/providers/settings-reference.md` → root, field, item array, `x-stackly` (`apply`, `visibility`, `secret`, `printableOnly`, `managed`, `key`), dan cara daemon memeriksa nilai, sesuai spec 0009 *Settings schema*; link ke `schemas/provider/settings.v1alpha1.schema.json` → AC-10, AC-14
- [x] Buka `docs/providers/versions-reference.md` → root, artefak dengan tiga status (`available`, `blocked`, `fixture`) dan field yang wajib serta terlarang per status, `dependencies`, dan record `compatibility`, sesuai spec 0009 *`versions.json`* dan `crates/core/src/provider/versions.rs`; kalimat bahwa paket official tidak menulis file ini dengan tangan (`cargo xtask codegen`); link ke `schemas/provider/versions.v1.schema.json` → AC-10, AC-14
- [x] Setiap blok contoh di ketiga halaman menyebut file sumbernya dan cocok (atau berupa potongan yang disingkat) dengan file itu di `providers/` (`postgresql`, `nats`, `traefik`, `example`); tidak ada manifest karangan, dan tidak ada perintah yang belum ada (`cargo xtask provider test`, alur import fitur 37) → AC-10

### Commands

- [x] `cargo xtask docs check` → `ok docs 20`, exit 0 (17 file Milestone 1 ditambah tiga halaman rujukan) → AC-9, AC-10, AC-14
- [x] `cargo test -p xtask --bin xtask docs::schema` → test lulus: fixture bersih tanpa masalah, halaman yang hilang adalah satu masalah walau `docs/providers/README.md` belum ada, property yang tidak disebut dan nilai enum yang tidak disebut masing masing satu baris dengan namanya, dan penelusuran hanya posisi schema → AC-9, AC-10
- [x] `cargo test -p xtask --test docs_check` → 2 test lulus; repo nyata bersih dengan (d) wajib → AC-9, AC-10
- [x] Tambah sementara property `ticket` dengan `enum: ["open", "wontfix"]` ke `$defs.Blocked` di `schemas/provider/versions.v1.schema.json`, jalankan `cargo xtask docs check` → tiga baris `docs/providers/versions-reference.md:1: ...` untuk `open`, `wontfix`, dan `ticket`, exit 1; kembalikan dengan `git checkout -- schemas/provider/versions.v1.schema.json` → AC-10
- [x] Pindahkan sementara `docs/providers/settings-reference.md` ke luar repo → `docs/providers/settings-reference.md:1: reference page is missing; it must cover `schemas/provider/settings.v1alpha1.schema.json`` ditambah dua baris link dari dua halaman lain, exit 1; kembalikan → AC-10
- [x] `cargo xtask codegen --check` → bersih, jadi nama yang diperiksa adalah schema hasil codegen saat ini → AC-10
- [x] `cargo clippy -p xtask --all-targets --locked -- -D warnings`, `cargo test -p xtask`, `cargo xtask check-deps`, `cargo fmt --all --check` → lulus

### Acceptance-criteria coverage

- AC-9: pemeriksaan (d) sekarang wajib; test fixture rusaknya di `xtask/src/docs/schema.rs` (property, nilai enum, halaman hilang). Pemeriksaan lain tetap seperti Milestone 1.
- AC-10: ketiga halaman rujukan dan (d) wajib, dibuktikan `docs check` bersih dan probe schema di atas.
- AC-14: ketiga halaman berbahasa Inggris dan lolos (g); sisanya (tabel ringkas, satu H1) dijaga review di langkah UI / manual.
- AC-2, AC-11: langkah 9, lihat *Langkah 9* di bawah.
- AC-8: menunggu langkah 10. AC-12: menunggu langkah 11 (fitur 37). AC-1, AC-3 sampai AC-7, AC-13: Milestone 1.

### Langkah 9: panduan provider

Ditambahkan sesudah `cargo xtask provider test` masuk `main` (`0cc879af`, spec 0036 langkah 13). Langkah ini ikut Milestone 2, karena kotak SCOPE nya mencakup langkah 8 dan 9.

#### UI / manual

- [x] Buka `docs/providers/README.md` → untuk siapa; prasyarat (Mac Apple Silicon macOS 27, rustup dengan toolchain `rust-toolchain.toml`, Xcode Command Line Tools, tanpa unduhan runtime); komentar `<!-- docs-walkthrough -->` tepat di atas satu blok `bash` berisi persis empat perintah `cargo xtask provider check providers/example`, `cargo xtask provider test providers/example`, `cargo xtask provider new mytool`, `cargo xtask provider test providers/mytool`; satu kalimat letak report (`target/provider-test/<runId>/report.json`, baris terakhir stderr); tabel halaman lain; alur tujuh langkah PRD 39 → AC-8, AC-11
- [x] Buka `docs/providers/authoring.md` → layout paket spec 0009; `cargo xtask provider new <name>` menyalin `providers/example/` menjadi `testing/<name>` yang langsung valid; urutan mengisi `provider.yaml`, settings schema, template, `tests/scenarios.yaml` (satu satunya file `tests/` yang dibaca; tanpa file itu satu skenario `default`); bagian **Moving to the official catalog** dengan entri inventaris (`cargo xtask runtime pin`), ganti `metadata.id` dan `software`, `cargo xtask codegen`, `PROVIDER_WORDS`, fixture form `create_form_fixtures`, test native per line, `[[provider]]` di `providers/catalog.toml`, `cargo xtask provider freeze` (sesudah `[[provider]]`, karena freeze menolak paket di luar catalog), `LICENSE-NOTICES.md`, dan kenaikan `metadata.revision` → AC-11
- [x] Buka `docs/providers/testing.md` → `cargo xtask provider check` tanpa daemon dan `stackly provider validate <dir>` lewat daemon (exit 0 atau 2 `cli.provider_invalid`, hasil sama); `error` dan `warning`; origin `package`, `authoring`, `scenarios`; `loadable` dan `publishable`; `cargo xtask provider test` dengan `--line`, `--scenario`, `--report`, `--keep`; namespace `official` dan `testing` saja; setting fake server (`readyDelayMs`, `crashAfterMs`, `logLinesPerSecond`, …); report `stackly.provider-conformance/v1`; exit 0, 1, 2; `--cleanup`; root sementara dan daemon sendiri; bukti fixture dan native → AC-11
- [x] Buka `docs/providers/compatibility.md` → `current` dan `deprecated` (`deprecated_api_version`), `unsupported_api_version` ditolak tanpa lifecycle, tiga kelas perubahan, corpus beku, deprecation, migrasi `apiVersion` baru, sesuai spec 0036 *Kebijakan kompatibilitas* → AC-11
- [x] Buka `docs/providers/review.md` → peringatan membaca diff sebelum menjalankan apa pun; validator; report harness; bukti native dengan tiga perintah (`native_runtime`, `provider_conformance`, `native_launchd` hanya untuk handover); corpus `cargo xtask provider freeze`; sumber dan lisensi; keamanan (executable dari runtime, tanpa secret di argv, loopback, tanpa skrip); revision tidak berubah sesudah merge; `HANDOVER_PROVEN` hanya lewat test native launchd → AC-11
- [x] `CONTRIBUTING.md` bagian 4 punya blok `cargo xtask provider test providers/<name>`, dan bagian 6 berisi lima langkah ringkas dengan link ke `docs/providers/README.md`, `authoring.md`, `testing.md`, `review.md` → AC-2
- [x] `README.md` bagian `Contributing` menautkan `docs/providers/README.md`, dan baris Layout `docs/` menyebut `docs/providers/`; `GOVERNANCE.md` bagian 3 menautkan `review.md` dan bagian 6 menautkan `compatibility.md`; `providers/example/README.md` menautkan `docs/providers/README.md`; template PR punya kotak `cargo xtask provider test` di `Checks I ran locally` → AC-2, AC-6, AC-7, AC-14
- [x] Tidak ada halaman yang menyebut alur import fitur 37 atau `stackly provider test` → AC-11

#### Commands

- [x] `cargo xtask docs check` → `ok docs 25`, exit 0 (20 file sebelumnya ditambah lima halaman baru) → AC-2, AC-9, AC-11
- [x] `cargo xtask provider check` → lima baris `ok` dengan digest yang sama seperti di `main` (README paket contoh tidak masuk digest) → AC-11
- [x] `cargo test -p xtask` → lulus, termasuk test scaffold yang menyalin `providers/example/README.md` → AC-11

### Hasil `/check verify` 2026-11-11 (Milestone 2, commit `9a9e97fe`)

**Verdict: PASS.** Semua langkah UI / manual dan Commands di bagian ini dan di *Langkah 9* dikerjakan dan lulus, dengan `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`. Tidak ada temuan FAIL. Catatan: `9a9e97fe` adalah `main` saat verify; kode fitur 37 di dalamnya tidak dibaca.

| AC | Verdict | Bukti |
| --- | --- | --- |
| AC-2 | terpenuhi | `CONTRIBUTING.md` bagian *Build and check locally* memuat blok `cargo xtask provider test providers/<name>`; bagian *Contributing a provider* berisi lima langkah dengan link ke `README.md`, `authoring.md`, `testing.md`, `review.md` |
| AC-9 (d) | terpenuhi | `cargo xtask docs check` → `ok docs 25`; `docs::schema` 5 test; `docs_check` 3 test; dua probe di bawah |
| AC-10 | terpenuhi | ketiga halaman rujukan ada; probe schema di bawah gagal dengan nama yang hilang |
| AC-11 | terpenuhi | lima halaman panduan ada dengan isi *Dokumen provider*; isinya dicocokkan dengan perilaku fitur 36 (di bawah) |
| AC-14 | terpenuhi | delapan halaman berbahasa Inggris, masing masing satu H1, tanpa HTML inline; `docs check` (g) bersih |

Perintah lain: `cargo xtask codegen --check` (20 file), `cargo xtask provider check` (lima baris `ok`, digest sama dengan sebelumnya), `cargo clippy -p xtask --all-targets --locked -- -D warnings`, `cargo test -p xtask` (535 unit test plus integrasi), `cargo xtask check-deps`, `cargo fmt --all --check`: semua bersih.

**Probe schema:** property `ticket` dengan `enum: ["open", "wontfix"]` ditambahkan ke `$defs.Blocked` → tiga baris `docs/providers/versions-reference.md:1: ...` untuk `open`, `wontfix`, dan `ticket`, exit 1; dikembalikan. `settings-reference.md` dipindah → baris `reference page is missing; it must cover ...` ditambah lima baris link rusak (bukan dua seperti di teks langkah, karena sekarang lima tempat menautkannya), exit 1; dikembalikan, `ok docs 25` lagi.

**Uji petik nilai di halaman rujukan terhadap `crates/core/src/provider/`** (semua cocok): batas `timeouts` dan default nya (`timeouts.rs`: init 120 dan 1 sampai 1800, readiness 60 dan 1 sampai 600, stop 30 dan 1 sampai 600, reload 30 dan 1 sampai 300), `health` dan `readiness` (interval, attempt timeout, `failureThreshold`), `successExitCodes` (paling banyak 8, default `[0]`), enam nilai `category`, 256 KiB manifest, 1 MiB `versions.json`, 32 template, 16 endpoint dan 16 config file, 64 karakter `displayName`, `versionCheck.timeoutSeconds` 1 sampai 60 default 15, batas `connection` (8, 64, 512), `openWith` (8, 48, 512, 4, 32, 16, 1024), `remedies` (8, 80, 6, 300, 500, 200), nama probe native `nats-readiness` dan `traefik-routes`, tiga status artefak, nilai `apply` dan `visibility`. Setiap blok contoh di ketiga halaman menyebut file sumbernya; isinya potongan yang dipadatkan dari file itu.

**Isi panduan terhadap perilaku fitur 36 yang sudah diverifikasi:** cocok untuk `cargo xtask provider new`, `check`, `test`, `freeze`, `stackly provider validate` (exit 0 dan 2), `--line`, `--scenario`, `--report`, `--keep`, `--cleanup`, path dan field report, exit 0, 1, 2, namespace `official` dan `testing`, aturan `tests/scenarios.yaml`, corpus beku, `not frozen`, `PROVIDER_WORDS`, fixture form, dan syarat `HANDOVER_PROVEN`. Tidak ada halaman yang menyebut alur import atau `stackly provider test`.

**Tafsiran developer yang dinilai** (semua diterima):

- Pemeriksaan (d) selalu jalan: diterima; probe halaman hilang membuktikannya.
- `authoring.md` menaruh `provider freeze` sesudah `[[provider]]`: diterima, karena `freeze` memang menolak paket di luar catalog (probe `providers/traefik` di verify spec 0036). Urutan di spec 0038 perlu dibalik.
- Kotak `cargo xtask provider test` di template PR: diterima.

**Kalimat yang tidak cocok dengan perilaku nyata** (rendah, tidak menggagalkan AC):

1. `testing.md`, bagian *The conformance harness*: "Per scenario it validates the package, finds it in the catalog, installs the runtime". Validasi dan pemasangan runtime terjadi sekali per run, saat root disiapkan; per skenario harness hanya memeriksa runtime itu terpasang.
2. `testing.md`, tabel *Exit codes*: tidak menyebut exit 130. Ctrl+C pertama menghentikan run secara tertib, menulis report `interrupted: true`, membersihkan root, dan keluar 130.
3. `testing.md`, *Findings*: "loadable: no finding has origin `package`". Kalimat ini cocok dengan kode hari ini, tetapi keputusan lead untuk `dev-f36-fix` (`loadable()` dan `publishable()` hanya menghitung `error`) akan membuatnya salah. Perlu diubah bersama fix itu. Hal yang sama untuk `--scenario` dengan nama yang tidak dikenal.
4. `testing.md`, `--keep` dan `--cleanup`: tidak menyebut bahwa `--cleanup` juga menghapus folder yang disimpan `--keep` (run nya sudah mati), dan bahwa baris `kept` mencetak path dengan `$TMPDIR`.
5. `authoring.md`: kewajiban `PROVIDER_WORDS` dan fixture form hanya ditulis di *Moving to the official catalog*. Keduanya sudah berlaku begitu ada folder baru di `providers/`, juga untuk paket `testing`. Probe: `cargo xtask provider new probetool` lalu `cargo test -p xtask` gagal dengan `probetool is not listed`. Saran: satu kalimat di *Start from the example*.
6. `review.md`, *Native evidence*: butir pertama berkata `cargo xtask codegen` menulis record kompatibilitas sesudah spike lulus, tanpa menyebut `cargo xtask runtime record` (alur dua commit spec 0003) yang menyalin record itu ke `evidence/runtime/` lebih dulu.
7. Teks dua langkah di atas tertinggal: `ok docs 20` kini `ok docs 25`, dan `docs_check` kini 3 test.

## Milestone 3: `cargo xtask docs walkthrough` from a fresh clone

Langkah Build plan 10: `cargo xtask docs walkthrough [--keep]` di `xtask/src/docs/walkthrough.rs`, baris `USAGE` di `xtask/src/main.rs`, test nya, satu run penuh, dan record manual AC-034 `evidence/manual/2026-11-11-provider-contributor-walkthrough-from-a-clean-cl.json`.

Tafsiran: (1) nama field verdict report harness dibaca dari spec 0036 *Report*: `verdict` dengan nilai `pass`. (2) Pemeriksaan tree bersih memakai `git status --porcelain --untracked-files=all`, supaya setiap file baru disebut satu per satu dan diperiksa berada di bawah `providers/<name>/` dari baris `cargo xtask provider new <name>`. (3) Saran `cargo xtask provider test --cleanup` dicetak setiap kali run gagal atau terputus, karena perintah ini tidak bisa tahu apakah root harness tertinggal tanpa membaca root user lain. (4) Ruang bebas dibaca dari `df -Pk` (xtask tidak memakai `unsafe`), di folder temp sistem.

### UI / manual

- [x] Dari root repo dengan tree bersih dan paling sedikit 8 GiB bebas di `$TMPDIR`: `cargo xtask docs walkthrough` → baris `clone <sha>`, lalu untuk keempat perintah blok walkthrough satu baris `run <perintah>` dan `ok`, lalu `ok walkthrough <sha> 2`, exit 0; folder `$TMPDIR/stackly-walkthrough-*` hilang sesudahnya. Butuh beberapa menit karena clone membangun xtask, `stackly-fake-server`, dan example `provider_harness` dari awal → AC-8
- [x] Jalankan lagi dan tekan Ctrl+C saat baris `cargo xtask provider test` berjalan → baris `fail interrupted` atau `fail <code>`, saran `cargo xtask provider test --cleanup` di stderr, exit 1, dan folder temp terhapus; `pgrep -f stackly-walkthrough` kosong → AC-8
- [x] Record `evidence/manual/2026-11-11-provider-contributor-walkthrough-from-a-clean-cl.json` berisi `steps`, `actual`, `status`, `reviewer`, dan `finishedAt` dari run pada `sourceCommit` nya, dan `cargo xtask evidence check` menerimanya → AC-8

### Commands

- [x] `cargo test -p xtask --bin xtask docs::walkthrough` → test lulus: blok adalah blok `bash` pertama sesudah penanda tanpa komentar; panduan tanpa penanda atau blok ditolak; status git hanya `??` di bawah paket hasil scaffold (`Cargo.lock` yang berubah atau file lain gagal); report harus satu per baris `provider test`, `stackly.provider-conformance/v1`, verdict `pass`; parse `df`; repo git fixture dengan baris yang lulus (exit 0, folder temp terhapus), baris `false` (berhenti di baris itu, exit 1, folder temp terhapus), file liar (exit 1), ruang bebas kurang (exit 2 tanpa clone), root 0700 dan `PATH` diawali `target/debug` clone dengan `--keep` (folder tetap ada), dan interupsi saat `sleep 30` (berhenti cepat, exit 1, folder terhapus) → AC-8
- [x] `cargo test -p xtask --test docs_check` → 3 test lulus, termasuk `docs walkthrough --nope` ditolak sebelum clone → AC-8
- [x] `cargo xtask help` → baris `docs walkthrough [--keep]` ada → AC-8
- [x] `cargo xtask docs check` → `ok docs 25` → AC-9
- [x] `cargo xtask evidence check` → menerima record AC-034 → AC-8
- [x] `cargo clippy -p xtask --all-targets --locked -- -D warnings`, `cargo test -p xtask`, `cargo xtask check-deps`, `cargo fmt --all --check` → lulus

### Hasil `/check verify` 2026-11-11 (Milestone 3, commit `9a9e97fe`)

**Verdict: PASS.** AC-8 terpenuhi. Semua langkah di bagian ini dikerjakan dan lulus; langkah Ctrl+C dikerjakan dengan `kill -INT` ke proses `xtask docs walkthrough` (bukan tombol di terminal).

- **Run penuh oleh checker:** dari tree bersih dengan 26 GiB bebas, `cargo xtask docs walkthrough` → `clone 9a9e97fe…`, lalu `run` dan `ok` untuk keempat baris blok, `ok walkthrough 9a9e97fe71be9d5aba9343d38eee247da17d1b60 2`, exit 0, 134 detik. Dua report harness `verdict: pass` (paket contoh dan `testing/mytool`). Sesudahnya tidak ada `$TMPDIR/stackly-walkthrough-*`, `git status` repo bersih.
- **Interupsi, dua kali:** (1) `kill -INT` saat baris `cargo xtask provider test providers/example` masih membangun → `fail interrupted`, exit 1 dalam satu detik. (2) `kill -INT` tujuh detik sesudah proses `provider_harness` di clone muncul (instance sedang berjalan, root berpenanda ada) → harness mencetak `stopping after the current step`, menulis report `interrupted`, lalu `fail interrupted`, exit 1 dalam dua detik. Di kedua run: saran `cargo xtask provider test --cleanup` di stderr, folder temp terhapus, `pgrep -f stackly-walkthrough` kosong, tidak ada root harness berpenanda dan tidak ada `stackly-fake-server` dari run itu yang tersisa.
- **Kegagalan lain:** dibuktikan test unit yang ada (`docs::walkthrough` 11 test): baris `false` berhenti di baris itu dengan exit 1 dan folder terhapus, file liar exit 1, ruang bebas kurang exit 2 tanpa clone, report bukan `pass` atau jumlah report salah ditolak. `docs walkthrough --nope` ditolak sebelum clone (`docs_check` 3 test).
- **Record manual AC-034:** `evidence/manual/2026-11-11-provider-contributor-walkthrough-from-a-clean-cl.json` berisi `steps`, `actual`, `status` `PASS`, `reviewer`, `finishedAt`, dan `sourceCommit` `2fea6e6b` (commit yang menambah perintah itu); file bukti `walkthrough.log` ada; `cargo xtask evidence check` menerimanya (64 record manual). Record diisi sesi developer sebagai reviewer. Spec 0002 hanya meminta nama atau handle, dan record manual lain di repo ini diisi dengan cara yang sama, jadi diterima. Run ulang checker di atas memberi hasil yang sama pada commit yang lebih baru; record baru tidak dibuat, karena spec 0002 tidak memintanya.
- Perintah lain: `cargo xtask help` memuat `docs walkthrough [--keep]`; `cargo xtask docs check` → `ok docs 25`; clippy, `cargo test -p xtask`, `check-deps`, dan `fmt --check` bersih.

**Tafsiran developer yang dinilai** (semua diterima): field `verdict` bernilai `pass` cocok dengan report nyata; `git status --porcelain --untracked-files=all`; saran `--cleanup` dicetak di setiap run yang gagal atau terputus (terlihat di kedua probe); ruang bebas dari `df -Pk`.

### Acceptance-criteria coverage

- AC-8: perintah, test fixture nya, satu run penuh, dan record manual di atas.
- AC-12 dan bagian langkah 11 dari AC-4 dan AC-6: menunggu fitur 37.
- AC lain: Milestone 1 dan 2.
