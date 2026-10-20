# 0014. Walking skeleton: rationale

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: walking skeleton biasanya fitur yang menulis banyak kode tipis di setiap lapisan. Di proyek ini hampir semua lapisannya sudah dirancang dan sedang dibangun oleh fitur lain (method dan create sementara di spec 0010 langkah 12 dan 13, `InstanceStore` di spec 0013 langkah 16, secret di spec 0011). Kalau fitur 14 ikut membangun bagian itu, dua sesi akan menulis file yang sama dan dua desain akan bersaing. Bingkai yang benar: fitur 14 adalah fitur penyambung dan pembuktian. Ia hanya menambah yang belum dimiliki siapa pun (jalan masuk fake ke root development, perintah CLI, dan bukti ujung ke ujung), dan menyerahkan sisanya ke pemiliknya. Keputusan delegasi: pengguna meminta sesi ini mengambil opsi yang direkomendasikan dan mencatatnya tanpa bertanya.

Done when fitur 14 meminta lima hal: `stackly instance create`, `start`, dan `stop` untuk fake provider berhasil lewat daemon; GUI menampilkan perubahan state lewat event; menutup GUI tidak menghentikan instance; restart daemon saat instance fake berjalan tidak menghasilkan instance ganda atau proses yatim; dan test fake provider berjalan di Mac ini. Trace nya AC-001, AC-013, dan AC-029 (semuanya bagian awal) serta fixture dummy server di PRD bagian 32.3.

Beberapa batasan sudah terkunci. Install runtime nyata baru ada di fitur 18, jadi tidak ada method daemon yang bisa memasang fake runtime. `install_fake` dan `pin_package` hidup di fitur crate `fixture` milik core, dan spec 0001 serta 0010 menyatakan fitur itu hanya boleh dinyalakan dari dependency dev; langkah 15 spec 0010 menunggu keputusan seeding di sini untuk menulis aturan `check-deps` nya. Hanya daemon yang boleh menulis registry dan membuat ID dan path. Spec 0002 menyediakan `#[ignore]` hanya untuk test native. `STACKLY_ROOT` membuat daemon memakai keychain file di root itu, sehingga root development dan test tidak pernah memunculkan dialog login Keychain.

Keputusan K1 dan K5 sudah diambil bersama lead scope 02: create sementara punya `register` sebagai langkah pertama yang tidak bisa dibalik, sehingga init yang gagal atau terputus meninggalkan operasi `needs_recovery` sampai di abandon; `install_fake` adalah salinan runtime ditambah `pin_package`. Lead juga meminta bukti "tidak ada instance ganda setelah daemon restart" mencakup create yang terputus. Spec 0020 meminta fitur 14 memakai ejaan flag create nya, spec 0022 menunggu folder `commands/instance/` dan bentuk `get` yang sama dengan `inspect`, dan Follow-up spec 0011 meminta fitur 14 membuktikan blocker needs secret di CLI dan GUI serta fixture credential yang tidak bocor.

Kalau tidak diputuskan, langkah 15 spec 0010 tidak bisa menulis aturan `fixture`, demo GUI tidak bisa dijalankan dengan tangan, dan setiap pembangun akan menebak di mana fake runtime datang dan siapa pemilik test GUI.

## Keputusan

### Keputusan 1: bagaimana fake runtime dan paket fake sampai ke root development

- **Opsi 1 (dipilih): example cargo `stacklyd` (`skeleton_seed`) yang memanggil `seed_fake` di fitur `fixture` lewat dependency dev, hanya untuk root yang disebut `--root`.** Pro: example cargo memakai dependency dev, jadi aturan "hanya dari dependency dev" tetap utuh tanpa pengecualian; tidak ada crate baru dan tidak ada kode di binary produksi; jalur kodenya sama dengan yang dipakai test (`seed_fake`); seed bisa mengambil lock daemon dan membuka registry lewat fungsi start `stacklyd` yang sama, jadi root yang di seed tidak berbeda dari root yang disiapkan daemon. Kontra: seed menulis registry di luar daemon; example pertama di workspace; membangun example bersama `stacklyd` di satu pemanggilan cargo menyatukan fitur dev ke binary daemon itu.
- **Opsi 2: perintah `cargo xtask skeleton seed`.** Pro: tempat alat developer yang sudah dikenal. Kontra: xtask harus menyalakan `fixture` lewat dependency biasa, jadi aturan butuh pengecualian bernama; xtask menulis registry di luar daemon tanpa lock daemon yang sama kecuali ia ikut bergantung pada library `stacklyd`, yang dilarang aturan "tidak ada crate yang bergantung pada library `stacklyd`".
- **Opsi 3: flag daemon khusus debug (`stacklyd --seed-fake` di balik `cfg(debug_assertions)` atau fitur cargo).** Pro: tidak ada penulis registry kedua. Kontra: kode fixture masuk binary daemon yang dijalankan developer setiap hari; fitur `fixture` menyala lewat dependency biasa; K5 sudah menolak feature gate di daemon.
- **Opsi 4: skeleton hanya di dalam test dengan `TestRoot`.** Pro: tidak ada alat baru. Kontra: demo CLI dan GUI dengan tangan tidak mungkin, karena root test hilang bersama proses test; test `#[ignore]` yang menulis ke root tetap bertentangan dengan spec 0002 (`#[ignore]` hanya untuk test native) dan dengan aturan test tanpa state global.

Aturan yang harus ditegakkan langkah 15 spec 0010 ditulis di *Aturan fitur `fixture`* spec ini dan dicatat sebagai satu baris Follow-up di spec 0010.

### Keputusan 2: permukaan CLI dan pembagian kerja

- **Opsi 1 (dipilih): `create`, `start`, `stop`, dan `get`, dengan ejaan flag spec 0020 dan argumen UUID saja.** Pro: memenuhi Done when dan memberi satu perintah baca untuk bukti CLI (blocker, PID, `adopted`); `get` berbentuk `{ instance }` seperti `inspect` spec 0022, jadi bisa menjadi alias nya tanpa perubahan; ejaan flag spec 0020 membuat fitur 20 hanya menambah; UUID saja menghindari membangun resolver nama dan kode `cli.instance_not_found` yang dimiliki spec 0022. Kontra: demo mengetik UUID; `--runtime`, `--provider`, dan `--revision` wajib; angka dan boolean harus lewat `--set-json`.
- **Opsi 2: hanya `create`, `start`, dan `stop`.** Pro: paling kecil. Kontra: tidak ada cara membaca status lewat CLI, jadi blocker needs secret dan bukti adopsi tidak terlihat di CLI.
- **Opsi 3: tambah `list` dan resolver nama.** Pro: demo paling nyaman. Kontra: menduplikasi langkah 5 spec 0022 (kelompok, hitungan, golden) dan menambah kode `cli` yang menaikkan kontrak; dua sesi akan menulis folder yang sama.

`--secret` masuk milestone 5 di atas `secret_input` spec 0011 langkah 8, karena daemon sudah memakai `generate` untuk field yang tidak dikirim, jadi jalur utama tidak perlu menunggu input secret yang aman.

### Keputusan 3: bagian GUI

- **Opsi 1 (dipilih): tanpa kode GUI baru; bukti lewat test Swift ujung ke ujung (mutasi dari koneksi `StacklyClient` kedua yang noninteractive) dan record manual (mutasi dari CLI).** Pro: `InstanceStore` dan `ProvisionalInstanceList` dari langkah 16 spec 0013 sudah memenuhi "perubahan state lewat event", dan spec 0013 AC-15 sudah memenuhi "menutup jendela tidak menghentikan apa pun"; koneksi kedua membuktikan perubahan dari client lain sampai ke GUI tanpa menjalankan CLI dari Swift (aturan spawn, stdin, batas waktu, dan `MainActor`). Kontra: gabungan CLI dan GUI hanya terbukti di record manual; blocker belum tampil di layar sampai spec 0022.
- **Opsi 2: tambah tampilan blocker atau tombol aksi di daftar sementara.** Pro: demo GUI lebih lengkap. Kontra: copy dan aturan nya sudah milik spec 0022 (I4) dan 0021; kode yang ditulis di sini akan dibuang.
- **Opsi 3: GUI dibuktikan manual saja.** Pro: tanpa kode test. Kontra: tidak ada penjaga regresi untuk jalur event dari CLI ke GUI.

Test event instance di langkah 18 spec 0013 dipenuhi oleh test yang sama, supaya tidak ada dua test untuk satu hal.

### Keputusan 4: bukti restart daemon

- **Opsi 1 (dipilih): binary `stacklyd` asli lewat `SpawnedDaemon`, dibunuh `SIGKILL` dan dihentikan `SIGTERM`, dengan CLI asli dan penghitungan proses lewat `/bin/ps`; create terputus lewat delay init fake.** Pro: membuktikan jalur yang dipakai pengguna (CLI, daemon, proses terpisah) dan proses yatim secara langsung; delay init deterministik karena test menunggu proses pembantu init terlihat sebelum membunuh daemon; pembantu selesai sendiri, jadi test tidak perlu membunuh proses apa pun. Kontra: paket fake berubah (revision `0.2.0`, golden); suite lebih lambat.
- **Opsi 2: test daemon di dalam proses (`stacklyd::server::start`).** Pro: cepat, konfigurasi bisa diperpendek. Kontra: daemon di dalam proses tidak bisa di `SIGKILL` tanpa membunuh test.
- **Opsi 3 (untuk create terputus): paket fixture yang langkah init nya menjalankan `serve`.** Pro: tanpa perubahan paket fake. Kontra: pembantu init tidak pernah selesai, jadi test harus membunuh proses sendiri dan klaim "tidak ada proses yatim" tidak bisa dibuktikan apa adanya.
- **Opsi 4 (untuk create terputus): membunuh daemon berdasarkan waktu.** Pro: tanpa perubahan apa pun. Kontra: flaky; kadang membunuh sebelum `register`, kadang setelah init selesai.

Sudut Keychain: semua test dan demo memakai `STACKLY_ROOT`, jadi daemon memakai keychain file root itu dan tidak pernah membuka login Keychain; CLI di test tanpa TTY memakai interaksi `Never`.

### Keputusan 5: rencana test dan evidence

- **Opsi 1 (dipilih): record otomatis `fixture` per test yang membuktikan ID PRD, ditambah satu record manual `fixture` yang kamu review.** Pro: setiap ID punya bukti yang bisa diulang dan satu bukti visual; sesuai spec 0002 (fake provider berarti `fixture`, record manual untuk yang hanya bisa dilihat). Kontra: record manual harus diulang (record baru) setiap kali bukti rilis butuh commit yang lebih baru.
- **Opsi 2: hanya record manual.** Pro: cepat. Kontra: tidak ada penjaga regresi, dan PRD bagian 28 meminta evidence dari test.
- **Opsi 3: menandai sebagian sebagai `native`.** Kontra: salah label; fake provider tidak pernah native, dan label ulang dilarang.

### Keputusan 6: urutan build

- **Opsi 1 (dipilih): lima milestone (seed, CLI, daemon asli, GUI, secret), dengan seed dan CLI paralel.** Pro: mengikuti Journey (jalan masuk dulu, lalu jalur utama sampai lengkap, lalu GUI dan secret yang bergantung pada fitur lain); setiap milestone menunggu tepat langkah fitur lain yang ia butuhkan (spec 0010 langkah 12, 13, 14, dan 15, spec 0011 langkah 5 dan 8, spec 0013 langkah 15 dan 16). Kontra: lima milestone berarti beberapa kali merge untuk satu fitur kecil.
- **Opsi 2: satu milestone setelah semua prasyarat ada.** Pro: satu sesi. Kontra: menunggu prasyarat paling lambat (spec 0013 langkah 16 dan spec 0011 langkah 8) untuk bagian yang sebenarnya sudah bisa dibangun.

## Rationale

Opsi 1 di keputusan 1 adalah satu satunya yang menjaga aturan `fixture` tanpa pengecualian dan tanpa kode baru di binary produksi, karena cargo hanya memberi dependency dev kepada test, bench, dan example. Penulis registry di luar daemon tidak bisa dihindari sebelum fitur 18 menyediakan install lewat daemon, dan seed menjaganya sama ketatnya dengan daemon: lock daemon root itu, layout dan registry dari fungsi start yang sama, dan penolakan root default. Rumusan aturan nya juga menutup celah yang tidak terlihat di kalimat lama: `[features]` di crate lain yang meneruskan `stackly-core/fixture` adalah dependency biasa yang menyalakan fitur itu.

Pembagian kerja di keputusan 2 dan 3 mengikuti kepemilikan yang sudah tertulis: spec 0010 memiliki method, spec 0013 memiliki store dan daftar, spec 0020 memiliki create penuh, dan spec 0022 memiliki daftar, inspect, dan resolver. Fitur 14 mengambil yang tidak dimiliki siapa pun dan membentuknya supaya pemilik berikutnya cukup menambah. Bukti di keputusan 4 dan 5 memakai binary asli karena Done when fitur ini justru tentang sambungan antar proses: daemon yang mati, CLI dan GUI sebagai dua client terpisah, dan proses instance yang hidup lebih lama dari daemon.

Pilihan delay init di keputusan 4 juga memberi bukti kecil AC-029: setting baru di paket fake tidak membutuhkan perubahan core, daemon, CLI, atau GUI.

## Keputusan kecil

- **Revision fake `0.2.0`.** Isi paket berubah, jadi label nya naik, supaya root yang pernah di pin tidak pernah memegang dua digest untuk satu revision (yang membuat create sementara gagal `instance.provider_ambiguous`). Runner up: tetap `0.1.0`, karena belum ada root development sebelum seed.
- **`--root` wajib di seed, bukan `STACKLY_ROOT`.** Seed tidak pernah bisa jatuh ke root default karena variabel lupa diset. Runner up: membaca `STACKLY_ROOT` seperti daemon.
- **Root lewat `validate_root_override`, bukan aturan sendiri.** Fungsi spec 0004 itu sudah menolak root relatif, hilang, symlink, bukan milik user, bisa ditulis orang lain, dan di dalam root default, dengan test nya sendiri. Runner up: fungsi `refuse_root` tambahan.
- **`--fake-server` dengan default di folder profil.** Example tidak pernah menjalankan cargo, jadi ia bekerja dari Swift dan Xcode. Runner up: `fake_server::binary()`, yang menjalankan cargo.
- **`seed_fake` idempoten dan menolak `leftover`.** Mengulang seed aman, dan sisa dari seed yang terputus tidak pernah ditimpa diam diam. Runner up: menghapus sisa lewat `remove_owned_tree`.
- **Kemunculan terakhir menang, `--set-json` menang atas `--set`.** Bisa ditentukan tanpa urutan argumen antar flag. Runner up: field ganda sebagai `cli.usage`, yang bertentangan dengan aturan validasi CLI spec 0020.
- **`--set` selalu teks sampai fitur 20.** Tanpa form dari daemon, CLI tidak tahu jenis field; konversi menurut `FieldKind` di fitur 20 hanya mengubah kasus yang tadinya ditolak daemon. Runner up: menebak angka dan boolean dari teks, yang salah untuk field teks berisi angka.
- **`instance` diisi lewat satu `instance.get` setelah menunggu.** Pengguna melihat blocker dan kegagalan terakhir tanpa perintah kedua. Runner up: hanya operasi.
- **Suite di `apps/daemon/tests/`.** Package `stacklyd` sudah memakai `fixture` lewat dependency dev, jadi `stackly-cli` tidak butuh pengecualian baru. Runner up: `apps/cli/tests/` dengan `fixture` di dependency dev CLI.
- **`ProcessTable` lewat `/bin/ps`.** Membaca argv semua proses tanpa `unsafe` di testkit. Runner up: libproc lewat `stackly-platform-macos`, yang menambah dependency test ke adapter.
- **Needs secret lewat keychain file yang dikunci `security lock-keychain`, bukan `FakeSecretStore`.** Binary daemon tidak bisa memakai store tiruan, dan spec 0011 sudah membuktikan keychain file yang dikunci menjawab `Locked` dengan interaksi `Never` dan tetap terkunci sampai daemon start lagi. Runner up: daemon di dalam proses dengan platform tiruan, yang tidak membuktikan jalur CLI.
- **Tiga instance untuk AC-15 dengan nama `alpha`, `beta`, dan `gamma`.** Mengikuti bunyi AC-001 PRD (tiga instance ready) dan menghindari pertanyaan tanda hubung di aturan nama. Runner up: satu instance.
- **`readyDelayMs` 1500 di test event.** Cukup lama untuk melihat `not_ready` lewat event, cukup pendek untuk suite. Runner up: 5000 seperti spec 0010.
- **`initDelayMs` 15000 di test create terputus, maksimum 600000.** Cukup lama supaya rekonsiliasi selesai saat pembantu masih hidup, tetap di bawah `initSeconds` 30 detik; nilai di atasnya menghasilkan `init_timeout`, yang juga berguna. Runner up: maksimum 30000.
- **Tidak ada kenaikan protokol.** Fitur ini tidak menambah method, DTO, event, atau kode.

## Cross check

Diperiksa sendiri setelah spec ditulis (2026-10-20):

- Setiap AC punya langkah build dan skenario test, dan setiap langkah menyebut AC nya.
- Setiap nilai yang dibutuhkan AC punya sumber di *Value sourcing*: ID runtime dan provider dari seed, ID instance dari `resource` operasi, status dari `instance.get`, jumlah proses dari `ProcessTable`, folder data dari inventarisasi spec 0004.
- Kode error yang disebut (`supervisor.init_failed`, `supervisor.not_initialized`, `secret.needs_secret`, `resource.not_found`, `cli.usage`, `operation.abandoned`) sudah ada di spec pemiliknya; tidak ada kode baru.
- Create sementara dan semantik `needs_recovery` sesuai K1 dan paragraf *Create sementara* spec 0010; langkah 9 spec 0020 sudah menyebut test fitur 14 sebagai yang ia perbarui.
- Satu titik terbuka dicatat di Follow-up, bukan diputuskan di sini: aturan dependency dev `stackly-cli` spec 0001 perlu menerima `fixture` untuk test binary CLI spec 0022 dan 0023.

Cross check oleh model lain (2026-10-20, hanya baca) menemukan 16 butir; semuanya diperiksa terhadap kode dan spec, lalu diterapkan sesuai rekomendasi (delegasi pengguna): helper Swift pindah ke `StacklyUITests/Support/` karena `clients/swift/Sources` dipindai aturan spawn; `--fake-server` supaya seed tidak menjalankan cargo; guard `Drop` dan pembersihan runtime root; delay init 15 detik dan pemeriksaan `dataInUse` bersyarat; satu aturan `instance` untuk tiga verb; teks `get` sama dengan `inspect` spec 0022; `Conflict` di seed; bentuk runtime saja `install_fake` dan sumber versi; aturan runner (stdin, `STACKLY_NO_INPUT`, batas waktu); koneksi `StacklyClient` kedua menggantikan CLI di test Swift; build binary di dalam run evidence; `[workspace.dependencies]` dan nama lain di aturan `fixture`; penolakan root lewat `validate_root_override`; polling readiness setelah restart; daftar file sentinel; serta `instance.name_taken`, definisi "GUI tidak mengirim mutasi", kalimat `--expected`, dan `operation` terakhir di bawah `--wait`.
