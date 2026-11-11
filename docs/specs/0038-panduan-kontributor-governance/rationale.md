# 0038. Rationale

[Kembali ke spec](index.md)

## Context

Stackly dirancang supaya provider baru cukup berupa paket data (manifest, schema settings, template), tanpa Rust atau Swift baru. Scope 06 menempatkan journey kontributor (JOURNEY-07) sebelum lima provider resmi berikutnya, supaya tim sendiri memakai jalur yang sama dengan orang luar. Fitur 36 membangun alatnya (scaffold, validator, harness) dan fitur 37 membangun import lokal dan trust. Yang belum ada adalah sisi manusianya: tidak ada panduan, tidak ada aturan kontribusi, tidak ada kanal keamanan, dan tidak ada template issue atau PR.

Keadaan repo per 2026-11-11: repo `stackly-dev/stackly` sudah publik; private vulnerability reporting mati; tidak ada `SECURITY.md`, `CONTRIBUTING.md`, Code of Conduct, atau folder `.github/`; `README.md` masih menyebut "early scaffold"; `Cargo.toml` masih menulis URL repo lama. Lisensi `MIT OR Apache-2.0` sudah dipasang sejak scaffold (spec 0001) dengan dua file lisensi di root, `deny.toml` hanya mengizinkan lisensi permisif, dan repo resep runtime memakai lisensi yang sama, tetapi OQ-013 masih tercatat terbuka sebagai Follow-up spec 0001.

Beberapa hal mengikat. Repo ini sengaja tanpa CI, jadi apa pun yang ingin dijaga harus bisa dijalankan dengan tangan lewat satu perintah, dan PR tidak punya tanda centang otomatis. Proyek dipegang satu orang, jadi janji waktu dan pembagian peran harus jujur. Aturan repo meminta dokumen publik berbahasa Inggris sementara scope, spec, dan review berbahasa Indonesia. Stackly menjalankan binary pihak ketiga dengan lisensi yang beragam (PostgreSQL, Apache-2.0, MIT, sampai GPL untuk MySQL), dan PRD 39 menegaskan bahwa lisensi Stackly tidak otomatis menentukan hak atas software itu. Beberapa hal hanya bisa dilakukan pemilik repo di GitHub.

PRD meminta, lewat REQ-034 dan AC-034, bahwa kontributor yang mengikuti panduan dari repository bersih bisa menjalankan validator dan harness pada contoh provider tanpa mengubah core, dan lewat bagian 39, governance minimal, lisensi yang dipilih secara sadar, serta kanal disclosure privat dengan proses advisory. Tanpa keputusan ini, kontribusi pertama dari luar datang tanpa aturan lisensi, laporan kerentanan pertama datang sebagai issue publik, dan panduan yang ditulis sekali akan basi tanpa ada yang menyadarinya.

Tiga Follow-up spec lain menjadi input: spec 0001 (file governance datang bersama fitur 38; OQ-013), spec 0008 (dokumentasi rujukan CLI dan JSON Schema envelope, "fitur 38 atau 66"), dan spec 0009 (schema docs untuk `stackly/v1alpha1` dan JSON Schema editor di `schemas/provider/`).

## Options considered

### K1: lisensi source (OQ-013)

**Option 1: `MIT OR Apache-2.0`** (dipilih). Lisensi ganda konvensi ekosistem Rust, sudah terpasang sejak spec 0001.

- Pros: kompatibel paling luas (MIT untuk proyek GPLv2, Apache-2.0 memberi hibah paten eksplisit); sama dengan hampir semua dependency; tidak ada perubahan file; cocok dengan `deny.toml` dan repo resep runtime.
- Cons: dua file lisensi dan satu kalimat penjelasan yang harus dipahami kontributor; tidak mencegah fork tertutup.

**Option 2: Apache-2.0 saja** (runner up).

- Pros: satu lisensi, hibah paten dan aturan NOTICE jelas.
- Cons: tidak kompatibel dengan GPLv2; menyimpang dari konvensi Rust; butuh mengubah manifest dan menghapus satu file tanpa manfaat nyata.

**Option 3: copyleft (MPL-2.0 atau GPL).**

- Pros: perubahan pada file Stackly harus tetap terbuka.
- Cons: menghalangi sebagian kontributor dan pengguna perusahaan; menambah pertanyaan tentang paket provider dan app yang di-link; bertentangan dengan arah permisif yang sudah dipakai di seluruh repo.

### K2: lisensi kontribusi

**Option 1: inbound sama dengan outbound lewat klausa di `CONTRIBUTING.md`** (dipilih).

- Pros: tanpa gesekan; konvensi Rust; Apache-2.0 bagian 5 sudah menyatakan hal yang sama.
- Cons: tidak ada tanda tangan per commit, jadi bukti asal usul lebih lemah.

**Option 2: DCO, `Signed-off-by` di setiap commit** (runner up).

- Pros: pernyataan asal usul per commit, murah.
- Cons: tanpa CI tidak ada yang menegakkannya; maintainer harus memeriksa tiap commit dengan tangan; kontributor pertama sering gagal di sini.

**Option 3: CLA.**

- Pros: hak relisensi di tangan proyek.
- Cons: butuh badan hukum dan alat tanda tangan; hambatan terbesar untuk kontributor kecil; tidak ada rencana relisensi.

### K3: kanal security

**Option 1: private vulnerability reporting GitHub** (dipilih).

- Pros: privat sejak awal, langsung menjadi draft advisory, tanpa server email, tanpa alamat pribadi yang dipublikasikan; repo sudah publik sehingga fiturnya tersedia.
- Cons: pelapor butuh akun GitHub; harus diaktifkan pemilik repo; bergantung pada GitHub.

**Option 2: alamat email khusus** (runner up, tetap boleh sebagai cadangan lewat I3).

- Pros: tidak butuh akun GitHub.
- Cons: butuh domain dan kotak masuk yang dipantau; tidak terenkripsi tanpa kerja tambahan; alamatnya harus diputuskan pemilik repo.

**Option 3: issue publik dengan label.**

- Pros: tanpa pengaturan.
- Cons: bukan kanal privat; melanggar PRD 39.

### K4: proses advisory dan target waktu

**Option 1: GitHub Security Advisory, target 7, 14, dan 90 hari** (dipilih).

- Pros: alat yang sama dengan kanal; fork privat sementara untuk perbaikan; permintaan CVE dari tempat yang sama; target realistis untuk satu orang.
- Cons: 7 hari untuk konfirmasi lebih lambat dari proyek besar.

**Option 2: target ketat (48 jam, 7 hari)** (runner up).

- Pros: terlihat lebih serius.
- Cons: satu maintainer tidak bisa menjaminnya; janji yang dilanggar lebih buruk daripada janji yang longgar.

### K5: Code of Conduct

**Option 1: Contributor Covenant 2.1** (dipilih).

- Pros: paling dikenal, dikenali halaman Community Standards GitHub, tersedia di menu template GitHub, tidak perlu ditulis.
- Cons: butuh alamat kontak dan orang yang benar benar menanganinya; bukan versi terbaru.

**Option 2: Contributor Covenant 3.0** (runner up; sejauh pengetahuan penulis versi ini sudah terbit, periksa sebelum memilihnya).

- Pros: bahasa lebih baru.
- Cons: belum sebanyak 2.1 dipakai dan dikenali alat.

**Option 3: teks sendiri yang pendek.**

- Pros: singkat.
- Cons: mudah punya celah; tidak dikenali alat; harus dipelihara.

### K6: model governance

**Option 1: dipimpin maintainer, peran ditulis terbuka** (dipilih). Lima peran PRD 39 ditulis dengan satu pemegang.

- Pros: sesuai kenyataan; jelas siapa memutuskan; mudah diperluas saat ada orang kedua.
- Cons: satu titik kegagalan; tidak ada mekanisme banding.

**Option 2: tambah `CODEOWNERS` sekarang** (runner up).

- Pros: permintaan review otomatis per folder.
- Cons: dengan satu orang tidak mengubah apa pun, dan butuh pengaturan branch protection yang belum ada.

**Option 3: komite atau voting.**

- Pros: lebih tahan terhadap satu orang.
- Cons: tidak ada orangnya.

### K7: letak dokumen

**Option 1: root, `.github/`, dan `docs/providers/`** (dipilih).

- Pros: tempat yang dicari orang dan dikenali GitHub; panduan provider terpisah dari spec internal.
- Cons: empat file baru di root.

**Option 2: semua di `.github/`** (runner up).

- Pros: root bersih.
- Cons: kurang terlihat di clone lokal; panduan panjang tidak cocok di sana.

**Option 3: semua di `docs/`.**

- Pros: satu folder.
- Cons: tercampur dengan scope dan spec berbahasa Indonesia; `CONTRIBUTING.md` di root adalah yang orang buka lebih dulu.

### K8: schema docs

**Option 1: halaman rujukan tulisan tangan dengan pemeriksaan cakupan** (dipilih).

- Pros: bisa menjelaskan arti, batas, dan contoh nyata; tidak menyentuh `stackly-core`; pemeriksaan sederhana.
- Cons: penjelasan bisa basi walau nama tercakup; setiap field baru butuh suntingan tangan.

**Option 2: rujukan dihasilkan `cargo xtask codegen` dari JSON Schema** (runner up).

- Pros: tidak pernah tertinggal dari schema.
- Cons: schema saat ini hanya punya 19 deskripsi di manifest, jadi hasilnya tipis sampai komentar doc di tipe manifest diperkaya, dan itu menyentuh file yang sedang diubah fitur 36; butuh renderer `$ref`, `oneOf`, dan enum.

**Option 3: hanya menunjuk file JSON Schema.**

- Pros: tanpa kerja.
- Cons: bukan dokumentasi; REQ-034 meminta schema docs.

### K9 dan K10: template issue dan PR

**Option 1: issue form YAML, satu form bug dengan pilihan area, plus satu template PR** (dipilih).

- Pros: field terstruktur dan label otomatis; pelapor yang tidak tahu apakah masalahnya core atau provider tetap bisa melapor; template PR menggantikan tanda centang CI dengan daftar cek yang ditempel.
- Cons: form YAML tidak bisa dilihat hasilnya tanpa GitHub; label harus dibuat pemilik repo.

**Option 2: empat template Markdown, satu per kategori** (runner up).

- Pros: sederhana, satu kategori satu file.
- Cons: pelapor harus tahu kategorinya lebih dulu, padahal itulah pekerjaan triage; tanpa field wajib.

### K11: bukti AC-034

**Option 1: `cargo xtask docs walkthrough` di clone baru** (dipilih).

- Pros: menjalankan perintah yang benar benar tertulis di panduan; menangkap prasyarat tersembunyi; bisa diulang setiap kali panduan atau harness berubah.
- Cons: build dari nol, lambat dan butuh disk; sekitar seratus sampai dua ratus baris xtask.

**Option 2: langkah manual di `verify.md`** (runner up).

- Pros: tanpa kode.
- Cons: dijalankan sekali lalu dilupakan; di repo tanpa CI, panduan akan basi saat provider berikutnya mengubah harness.

**Option 3: test Rust `#[ignore]`.**

- Pros: ikut run native.
- Cons: menambah build penuh ke setiap run native; bukan bukti native.

### K12: rujukan CLI (Follow-up spec 0008)

**Option 1: fitur 38 hanya menulis yang dipakai kontributor; sisanya fitur 66** (dipilih).

- Pros: pembaca rujukan CLI umum adalah pengguna dan penulis skrip, bukan kontributor provider; satu tempat untuk rujukan itu.
- Cons: rujukan CLI umum menunggu fase 12.

**Option 2: seluruh rujukan CLI di fitur 38** (runner up).

- Pros: lebih cepat ada.
- Cons: di luar journey kontributor; perintah masih bertambah di setiap fase, jadi rujukan cepat basi.

### K13: provider template

**Option 1: satu sumber, paket contoh `providers/example/` spec 0036 yang disalin scaffold** (dipilih).

- Pros: tidak ada dua template yang bisa berbeda; yang didokumentasikan adalah yang benar benar dihasilkan alat.
- Cons: spec ini bergantung pada spec 0036 untuk isi template.

**Option 2: folder template terpisah di `docs/`** (runner up).

- Pros: bisa dibaca tanpa menjalankan alat.
- Cons: salinan kedua yang pasti menyimpang.

## Rationale

Benang merahnya tiga: repo tanpa CI, satu maintainer, dan binary pihak ketiga.

Karena tidak ada CI, setiap janji di dokumen harus bisa diperiksa satu perintah, atau ia akan basi. Itu alasan `docs check` memeriksa link, nama perintah, dan cakupan schema, alasan `docs walkthrough` menjalankan blok perintah yang sama dengan yang dibaca kontributor (K11), dan alasan template PR meminta keluaran cek lokal (K10). Rujukan tulisan tangan (K8) dipilih karena schema hari ini miskin deskripsi dan memperkayanya berarti menyentuh tipe manifest yang sedang dikerjakan fitur 36; pemeriksaan cakupan menutup risiko terbesarnya, yaitu field yang tidak terdokumentasi sama sekali.

Karena satu orang memegang semua peran, governance ditulis apa adanya (K6), target waktu keamanan dibuat longgar tetapi bisa ditepati (K4), dan kanal keamanan memakai alat yang tidak butuh server atau kotak masuk baru (K3). Private vulnerability reporting juga menghindari mempublikasikan alamat pribadi. Hal yang hanya pemilik repo bisa lakukan ditulis sebagai input dan langkah manual dengan status `BLOCKED`, bukan diisi tebakan.

Karena Stackly mengelola software orang lain, keputusan lisensi (K1) sengaja sempit: ia hanya menyatakan lisensi source proyek, yang memang sudah terpasang dan konsisten dengan `deny.toml`, dan menunjuk `runtimes/inventory.toml` serta gerbang lisensi spec 0003 untuk binary. Mengganti lisensi sekarang tidak memberi manfaat dan menambah kerja. Inbound sama dengan outbound (K2) dipilih karena DCO tanpa penegakan otomatis hanya menambah pekerjaan tangan. `LICENSE-NOTICES.md` paket menjawab permintaan PRD 39 untuk mencatat apakah binary diunduh dari upstream atau didistribusikan ulang, tanpa menyalin data per versi yang sudah ada di inventaris.

Follow-up spec lain dijawab begini. Spec 0001: OQ-013 ditutup (K1) dan keempat file governance didefinisikan. Spec 0008: dipecah (K12), karena rujukan envelope dan exit code adalah dokumentasi pengguna. Spec 0009: schema docs menjadi tiga halaman rujukan di `docs/providers/` yang dijaga terhadap JSON Schema di `schemas/provider/`, dan schema editor itu ditunjuk dari halaman rujukan.

## Cakupan Trace

| ID | Di mana tercakup |
| --- | --- |
| REQ-034 | CONTRIBUTING (AC-2), schema docs (AC-10), panduan dan provider template lewat `providers/example/` dan `cargo xtask provider new` spec 0036 (AC-11, K13), security policy (AC-4), issue template (AC-7). Compatibility fixture (`cargo xtask provider freeze`) dan provider test command (`cargo xtask provider test`) dibangun spec 0036 dan didokumentasikan di `compatibility.md` dan `testing.md` |
| AC-034 | AC-8 (`docs walkthrough` di clone baru, record manual) |
| OQ-013 | K1, AC-1 |
| JOURNEY-07 | AC-8, AC-11, AC-12 |
| PRD bagian 39 | AC-3 sampai AC-7, AC-13; registry komunitas dan publikasi catalog tidak dibangun di sini |

## Catatan cross check

Cross check oleh model lain (2026-11-11) atas draf pertama. Semua temuan diterapkan dengan perbaikan yang direkomendasikan:

1. Pemeriksaan perintah tidak lagi menjalankan `--help` (xtask mencocokkan argumen dengan tangan, jadi `--help` gagal atau bahkan menjalankan tugas); diganti dua tabel statis: `USAGE` xtask dan `stackly manpage --json`.
2. Aturan memecah baris perintah ditulis (komentar `#`, `&&`, `;`, `|`, `\`, awalan `VAR=$(...)`, bentuk `cargo run -p stackly-cli --`).
3. Code span dan autolink dilewati pemeriksaan placeholder dan HTML, karena `providers/nats/README.md` memuat `<data>` di dalam backtick; `TODO` dan `TBD` dicocokkan sebagai kata utuh.
4. Cakupan schema menelusuri posisi schema saja, karena settings schema punya field bernama `properties`, `enum`, dan `const`; pemicunya menjadi adanya `docs/providers/README.md`.
5. `docs walkthrough`: root 0700 dibuat, `PATH` diawali folder biner clone, `CARGO_HOME` dan `RUSTUP_HOME` diteruskan, clone memakai `--no-checkout` lalu checkout sha, dan klaim tanpa jaringan dilepas untuk cargo.
6. Pembersihan saat Ctrl+C ditulis: SIGINT ke process group, tunggu, hapus folder temp; `--cleanup` harness tidak dipanggil otomatis.
7. Aturan tree bersih diperketat: hanya file baru di folder paket hasil scaffold.
8. Kalimat `SECURITY.md` dan `GOVERNANCE.md` yang menyebut alur revocation spec 0037 dipindah ke langkah 11.
9. OQ-013: sumber keputusannya (delegasi pemilik produk) dicatat di Decision.
10. AC-3 mencatat URL dan SHA-256 teks; AC-13 diperiksa mesin lewat (f); AC-14 menyebut bagian lint yang ditegakkan; aturan issue form dilengkapi.
11. Daftar cek di `CONTRIBUTING.md` dan template PR mendapat `cargo xtask provider test` dan `cargo xtask catalog check`, dan aturan kenaikan revision dibatasi pada file yang masuk digest.
12. `review.md` memperingatkan bahwa menjalankan test atau walkthrough pada branch PR menjalankan kode PR; aturan slug anchor dan versi yang didukung diperjelas.
