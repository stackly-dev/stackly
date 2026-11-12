# 0041. Rationale: provider Traefik dan routing lokal

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: scope menandai fitur 41 "needs a decision" dan Follow-up beberapa spec mengandaikan fitur ini membawa pekerjaan core (bcrypt untuk htpasswd, dashboard, HTTPS). Setelah membaca `main`, hampir semua yang diminta Done when sudah ada: paket `providers/traefik`, probe `traefik-routes`, halaman Configuration dengan daftar `routes`, dan test dua instance. Yang benar benar belum ada adalah bukti dan status resmi, bukan mekanisme. Jadi pertanyaan yang tepat bukan "apa yang perlu ditambah ke core untuk Traefik", tetapi "apa yang boleh dijanjikan provider ini dengan mekanisme yang sudah terbukti, dan apa yang harus ditolak dengan jujur". Spec ini memilih janji yang sempit dan terbukti.

Traefik adalah provider nyata ketiga. Paketnya dibuat fitur 9 sebagai contoh daftar `routes`, diperbaiki langkah 22 spec 0009 (init tanpa token port), lalu mendapat probe `traefik-routes` dari audit keamanan 2026-11-05 karena file provider Traefik menyimpan config lama tanpa memberi tahu saat file baru tidak bisa dimuat. Fitur 29 membuktikan apply `routes` lewat reload pada dua instance asli. Meski begitu `official/traefik` masih `[[planned]]` di catalog: belum ada evidence kompatibilitas scope `provider`, belum ada skenario konformitas milik paket, belum ada `connection`, remedy, atau bukti handover.

PRD 22.5 dan REQ-039 meminta entrypoint loopback, routing lewat file provider, health di listener khusus, namespace route per instance, validasi dan apply aman, dashboard dan API insecure yang tidak menyala secara default, dan HTTPS yang hanya aktif dengan certificate dan key yang valid dan disimpan aman. AC-039 meminta perubahan route di A tidak menyentuh B, dan config invalid tidak mengganti last known good. Matrix PRD 32.2 meminta dua instance, routing update, invalid config, HTTP, dan health, dengan HTTPS hanya "ketika capability diuji".

Gaya yang membentuk keputusan: scope 07 mengukur RISK-07 dengan jumlah perubahan core per provider; spec 0029 sudah memutuskan (keputusan user 2026-11-02) bahwa Stackly tidak pernah roll back sendiri; spec 0009 sudah memutuskan (keputusan user 2026-11-05) bahwa API read only di port monitoring loopback adalah residual yang diterima; dan lima provider dibangun paralel, jadi setiap file bersama yang disentuh adalah biaya koordinasi. Tanpa keputusan, developer akan menebak apakah HTTPS, dashboard, dan bcrypt termasuk, dan tiga tebakan itu masing masing menyeret perubahan core.

## Options considered

### Keputusan 1: HTTPS di revision ini

**Opsi A: tidak ditawarkan (dipilih).** Paket hanya HTTP; tidak ada field certificate.

- Pro: tidak ada janji yang tidak bisa dibuktikan; nol perubahan core; matrix PRD 32.2 mengizinkannya ("ketika capability diuji").
- Kontra: developer yang butuh `https://` lokal belum terlayani; kalimat deskripsi scope perlu dibaca sebagai batas.

**Opsi B: path certificate dan key milik pengguna sebagai settings.** Dua field path, dirender ke `tls.certificates` di file dynamic, router mendapat `tls: {}`.

- Pro: hampir hanya perubahan paket; key tetap di tempat pengguna menyimpannya.
- Kontra: Traefik memakai certificate bawaannya sendiri kalau pasangan itu gagal dimuat, dan probe tidak melihatnya, jadi HTTPS bisa "aktif" tanpa certificate pengguna, tepat yang dilarang PRD. Menutup celah itu butuh parser certificate atau klien TLS di core.

**Opsi B2: seperti B, ditambah `tls.options.default.sniStrict: true`.** Traefik menolak handshake untuk nama yang tidak punya certificate, jadi certificate bawaannya tidak pernah disajikan.

- Pro: gagal tertutup tanpa perubahan core; ini opsi termurah yang jujur.
- Kontra: perilakunya pada 3.7.15 belum dibuktikan di Mac ini; readiness tetap tidak tahu apakah certificate dimuat, jadi "ready" tidak berarti HTTPS jalan; menambah dua setting, satu tombol per route atau per instance, dan satu rangkaian test native TLS ke fitur yang Done when nya tidak meminta HTTPS.

**Opsi C: key di Keychain, dirender ke `${instance.secrets}`, dengan probe TLS.** Lengkap dan aman.

- Pro: memenuhi "cara penyimpanan yang aman" sepenuhnya.
- Kontra: secret multi baris, mekanisme pengiriman baru, probe TLS, dependency kripto baru di core; ini fitur sendiri, bukan bagian provider.

### Keputusan 2: dashboard dan API

**Opsi A: dashboard selalu mati, API read only tetap (dipilih).**

- Pro: tidak ada tombol yang membuka 404; sama dengan keadaan yang sudah diaudit; nol perubahan core.
- Kontra: tidak ada tampilan visual router; API tetap bisa dibaca proses lokal lain.

**Opsi B: setting `dashboard` (default mati) tanpa password.** `api.dashboard` mengikuti setting.

- Pro: hanya template; data yang terbuka sama dengan API yang sudah ada.
- Kontra: entri `openWith` atau `connection` tidak punya syarat, jadi tombol dashboard tampil dan membuka 404 selama setting mati; tanpa tombol, fiturnya tidak ditemukan. Butuh syarat `openWith` di schema manifest supaya rapi.

**Opsi C: API dan dashboard di balik basic auth dengan hash bcrypt.** `api.insecure: false`, router ke `api@internal` dengan middleware, password di Keychain.

- Pro: menutup residual baca API; menjawab Follow-up bcrypt spec 0011.
- Kontra: probe harus mengirim credential (param secret baru, entri `EXPOSE_ALLOWED`), setiap instance Traefik mendapat secret dan bisa terblokir Keychain, dan core mendapat dependency bcrypt. Melindungi daftar route lokal dengan biaya itu tidak sepadan.

### Keputusan 3: rule yang ditolak Traefik

**Opsi A: verify sesudah tulis, lalu roll back eksplisit (dipilih).** Mekanisme spec 0009 AC-22 dan spec 0029 AC-8 apa adanya.

- Pro: sudah terbukti native; konsisten dengan provider lain; nol perubahan.
- Kontra: ada jendela ketika route yang ditolak tidak menjawab dan route lain di revision itu sudah berlaku.

**Opsi B: pra validasi di proses Traefik sementara.** Sebelum commit, jalankan Traefik kedua di port sementara dengan file kandidat, probe, lalu matikan.

- Pro: instance yang jalan tidak pernah melihat rule yang ditolak.
- Kontra: primitive lifecycle baru di manifest dan supervisor, alokasi port sementara, dan satu sampai dua detik tambahan per apply; perubahan core terbesar di antara semua opsi.

**Opsi C: mempersempit `rule` dengan pattern schema atau field terstruktur (`host`, `pathPrefix`).**

- Pro: sebagian besar kesalahan ketik tertangkap lapis schema.
- Kontra: regex tidak bisa menjamin Traefik menerima rule (regex di `HostRegexp`, nama host tidak sah), pengelompokan dengan kurung hilang, dan fixture serta test spec 0009 dan 0029 yang memakai `rule` harus ditulis ulang.

**Opsi D: roll back otomatis saat verify gagal.**

- Pro: jendela route mati menjadi singkat.
- Kontra: bertentangan dengan keputusan user di spec 0029 ("Stackly tidak pernah roll back sendiri").

### Keputusan 4: namespace route

**Opsi A: instance adalah namespace (dipilih).** Satu proses dan satu folder per instance.

- Pro: isolasi datang dari proses dan path, bukan dari konvensi nama; nama route di form sama dengan nama di Traefik dan di log.
- Kontra: nama router tidak unik lintas instance, jadi log dua instance bisa menyebut `api@file` yang berbeda.

**Opsi B: awalan nama instance di setiap router.**

- Pro: nama unik lintas instance.
- Kontra: rename instance mengubah semua nama router; nama di form berbeda dari nama di Traefik; tidak menambah isolasi apa pun.

### Keputusan 5: test BR-025

**Opsi A: test native khusus `traefik_compatibility` (dipilih).**

- Pro: check nya mengikuti baris matrix PRD 32.2 (routing update, invalid config).
- Kontra: satu file test lagi di samping test konformitas.

**Opsi B: test konformitas sebagai test BR-025.**

- Pro: tanpa test baru.
- Kontra: langkah harness tidak punya routing update atau invalid config, jadi record nya tidak membuktikan baris matrix.

## Rationale

Semua pilihan mengikuti satu aturan: provider ini hanya menjanjikan yang mekanismenya sudah terbukti native di `main`. Itu yang membuat ukuran RISK-07 jujur (satu baris data), dan itu juga yang paling aman untuk lima sesi paralel, karena fitur ini nyaris tidak menyentuh file bersama sebelum milestone catalog.

HTTPS ditolak bukan karena sulit dirender, tetapi karena Traefik gagal secara diam diam pada certificate yang buruk. Menawarkan field certificate tanpa bisa membuktikan certificate itu yang disajikan akan menjadi label supported palsu, yang justru dilarang tujuan fase 07. Matrix PRD sendiri menaruh HTTPS di belakang "ketika capability diuji". Opsi B2 menutup bagian terburuknya dan menjadi titik mulai Follow-up, tetapi ia bertumpu pada perilaku Traefik yang belum dibuktikan dan readiness yang tidak melihat certificate, jadi ia pantas mendapat spec dan run native sendiri, bukan diselipkan ke fitur yang sudah bisa selesai tanpa itu.

Dashboard dan bcrypt jatuh bersama. Satu satunya alasan bcrypt adalah basic auth, dan satu satunya pemakai basic auth di fitur ini adalah dashboard. Dashboard tanpa password hanya masuk akal dengan tombol yang muncul saat ia menyala, dan itu butuh perubahan schema manifest. Dashboard dengan password menyeret secret ke setiap instance dan credential ke probe. Keduanya lebih mahal daripada nilainya untuk proxy lokal, sementara Done when hanya meminta dashboard insecure mati secara default, dan dashboard yang selalu mati memenuhinya.

Untuk rule yang ditolak, keputusan spec 0029 sudah mengikat: roll back adalah aksi pengguna. Pra validasi di proses sementara adalah perbaikan yang sah, tetapi ia primitive baru yang dipakai satu provider, dan belum ada bukti bahwa jendela "route mati sampai roll back" menyakitkan dalam pemakaian nyata. Ia dicatat sebagai Follow-up dengan pemicunya.

## Keputusan kecil

- **Revision tetap `1.0.0`**: paket belum pernah ada di `providers/catalog-revisions.json`, sama dengan alasan spec 0009 saat langkah 22. Runner up: `1.1.0`, yang membuat terbitan pertama tampak seperti perubahan.
- **Satu remedy, bukan dua**: doctor memakai entri pertama yang cocok, dan `when` tidak bisa menyangkal, jadi remedy kedua untuk file asing akan tertutup remedy pertama. Satu remedy dengan tiga langkah mencakup keduanya. Runner up: dua remedy dengan `readinessDetailContains`.
- **`routers` sebagai aksi browser**: menjawab Follow-up spec 0070 dengan sesuatu yang selalu berfungsi. Runner up: tanpa aksi browser sama sekali.
- **Folder `dynamic` tetap diawasi sebagai folder**, bukan `providers.file.filename`: itu keputusan spec 0009, dan file asing terdeteksi probe. Runner up: mode satu file, yang mengabaikan file asing tetapi mengubah perilaku yang sudah terbukti.
- **Tanpa setting baru** (access log, dashboard, TLS): setiap setting menambah permukaan test di lima tempat (golden, fixture form, snapshot, fuzz seed, konformitas).
- **`metrics` tidak diklaim**: capability itu berarti ada sumber metrik provider; Traefik belum merender blok metrics.
- **Dua skenario saja (`default`, `one-route`)**: skenario rule yang ditolak dengan `expect: start_fails` dibuang setelah cross check, karena start yang melewati batas readiness tidak mengisi `lastFailure` dan prosesnya tetap hidup, sedangkan langkah harness menuntut keduanya. Kasus itu dibuktikan test AC-4.

## Bukti yang dibaca

- `providers/traefik/` di `main` (manifest, schema, dua template, `versions.json` dengan record scope `runtime`).
- `crates/core/src/provider/probe/traefik.rs` (teks detail `routes_mismatch`) dan `crates/platform-macos/src/fs.rs` (nama file sementara `write_file_atomic`).
- `apps/daemon/tests/traefik/config.rs` dan `readiness.rs` (yang sudah terbukti: reload A tanpa menyentuh B, rule `Host(` gagal verify dengan `supervisor.reload_failed`).
- `crates/core/src/supervisor/handover.rs` dan `apps/daemon/tests/native_launchd.rs` (pola bukti NATS dan PostgreSQL).
- Spec 0009 (AC-21, AC-22, *Paket contoh NATS dan Traefik*), 0011 (*Pengiriman secret ke provider*), 0017 (*Aturan masuk catalog*), 0018 (AC-15), 0021, 0023, 0029 (AC-3, AC-7, AC-8, AC-10, AC-16), 0030, 0034 (*Blok remedies*), 0036 (*File skenario*, *Harness*, *Handover*, *Dari contoh ke paket resmi*), 0070 (AC-6, *Blok openWith*).
- Commit `27e98df3` (NATS masuk catalog) dan `0b43369a` (PostgreSQL `1.0.2`) untuk daftar file yang ikut berubah saat manifest atau catalog berubah.

Satu fakta tentang Traefik belum dibuktikan test yang ada dan dikunci AC-4: saat satu rule ditolak, router lain dari file yang sama tetap dimuat. Test `readiness.rs` hanya membuktikan bahwa router yang ditolak membuat probe gagal.

## Cross check

Cross check read only dengan model lain (Opus) pada 2026-11-12, terhadap kode di `main`. Yang diubah karenanya:

- Skenario `rejected-rule` dibuang: `ReadinessTimeout` tidak mengisi `lastFailure` dan tidak mematikan proses (`apps/daemon/tests/supervisor/process.rs`, `apps/daemon/tests/conformance/steps.rs`), jadi langkah `start_fails` harness akan gagal.
- AC-12 dan langkah 10 diurutkan ulang: test launchd menuntut `handover_proven` dari binary yang diuji, jadi baris `HANDOVER_PROVEN` masuk sebelum run dan dibuang kalau record tidak `PASS`.
- AC-14 mengizinkan modul test `handover.rs`, yang hari ini menegaskan Traefik belum terbukti.
- Tiga file test bersama ditambahkan ke Build plan dan *Sesi paralel*: `crates/core/tests/provider_check.rs`, `crates/core/tests/connection_render.rs`, dan `xtask/tests/provider_check.rs` (test freeze yang memakai Traefik sebagai contoh planned).
- AC-3 menyebut path error nama ganda yang benar (`settings.routes[<i>].name`).
- Arti tiap check `compatibility.json`, subject evidence, dan helper `Checks` ditulis.
- AC-5 dan AC-6 menyebut syarat "tanpa route yang cocok" untuk jawaban 404 di port `web`.
- Penyimpangan dari kalimat PRD tentang API insecure dinyatakan di *Trace PRD* dan *Security model*.
- Opsi HTTPS dengan `sniStrict` dicatat dan ditolak untuk revision ini.
- Dua kegagalan file pengguna di `dynamic/` (YAML rusak, nama router sama) masuk *Edge cases*, dan teks remedy langkah 3 diperluas.

Yang dikonfirmasi benar: token `${endpoint.ping.address}` diikuti path sah di `connection` dan URL browser; remedy dengan `readinessReasons` dimuat dan dipasang doctor pada instance `running` yang `not_ready`; pattern settings mencocokkan seluruh nilai; teks detail probe sama dengan AC-4 dan AC-8; `freeze` menolak provider planned. Perilaku Traefik (router dengan rule yang ditolak menjadi `disabled` sementara router lain dimuat, dan 404 untuk `/dashboard/`) dinilai masuk akal tetapi belum dibuktikan pada 3.7.15; AC-4 dan AC-6 yang menguncinya.
