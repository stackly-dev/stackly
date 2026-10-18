# 0009. Rationale: provider engine dan manifest schema

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: fitur 9 membekukan `stackly/v1alpha1` sebelum provider nyata pertama (PostgreSQL, fitur 19) dibangun. Risikonya, schema terlalu sempit untuk PostgreSQL lalu harus diubah saat banyak hal sudah menumpang di atasnya (RISK-07). Karena itu schema ini sengaja memuat kait yang dibutuhkan PostgreSQL walau belum ada provider yang memakainya hari ini: langkah `runExecutable` untuk `initdb`, secret lewat stdin dan file 0600 sementara, `socketName` yang memuat port (`.s.PGSQL.<port>`), penanda data (`PG_VERSION`) untuk preflight, dan registry probe native. Yang sengaja ditunda adalah hal yang aturannya hanya bisa dibuktikan pada binary aslinya (escaper `postgresqlConf` dan probe query terautentikasi). Fitur ini juga bergantung pada keputusan spec yang belum ditulis (0005, 0006, 0010, 0011); setiap ketergantungan itu ditulis sebagai baris Assumed di spec, bukan diputuskan di sini.

Stackly harus bisa menjalankan enam tool native (PostgreSQL, MySQL, NATS, ClickHouse, Valkey, Traefik) dan nanti tool lain tanpa menulis ulang core atau GUI untuk setiap tool baru. PRD menetapkan bahwa tool baru yang cukup dengan primitive yang ada tidak boleh butuh patch core atau GUI (REQ-029, AC-029), dan bahwa eksekusi provider dibatasi: executable terverifikasi, argv array, cwd dan env eksplisit, batas file, tanpa shell interpolation atau script plugin (REQ-031, AC-031). Dua risiko utama saling tarik: schema yang terlalu sempit memaksa patch core per tool (RISK-07), dan schema yang terlalu kuat membuka eksekusi kode sembarangan lewat manifest (RISK-08).

Contoh manifest di PRD bagian 21.2 hanya ilustrasi struktural. Ia belum menjawab bagaimana field divalidasi, apa saja primitive dan probe yang dikenal, bagaimana `${...}` diselesaikan, bagaimana file config dirender dengan aman, apa yang terjadi pada field yang tidak dikenal, atau bentuk data yang diterima supervisor. VAL-002, VAL-012, VAL-018, dan VAL-019 menambah aturan untuk nama provider, kelas field config, trust dan permission, serta settings yang bisa mengubah path, port, atau proses. Spec 0001 menyerahkan validator JSON Schema dan template engine ke fitur ini, dan mewajibkan `serde_yaml_ng` (bukan `serde_yml`).

Fitur lain sudah menunggu jawabannya. Supervisor (fitur 10) menjalankan apa yang engine putuskan. Walking skeleton (fitur 14) butuh fake provider dan dummy server yang bisa delay ready, crash, dan spam log (PRD bagian 32.3). Spec 0003 menyimpan data artefak di `runtimes/inventory.toml` dengan penanda `// provisional: F9`, dan PRD bagian 21.1 meminta `versions.json` per provider. Spec 0004 menunggu fitur 9 untuk batas nama socket 15 byte, pemetaan capability, dan isi `LineCompatibility`. Form create dan edit config (fitur 20 dan 29) harus dirender dari schema tanpa kode khusus provider. Done when fitur 9 meminta manifest NATS, Traefik dengan daftar routes, dan fake provider lolos validasi, serta fuzz target parser manifest dan template berjalan di Mac ini.

## Options considered

### Keputusan 1: mesin template untuk file config

#### Option 1: Renderer kecil tanpa logika buatan sendiri

Grammar `{{ path }}`, `{{#each}}`, `{{#if}}`, tanpa ekspresi, helper, atau partial. Setiap config file menyatakan format, dan setiap nilai otomatis di escape sesuai format itu. Variabel dicek statis terhadap tipe context dan settings schema saat validasi.

**Pros**:

- Permukaan serangan paling kecil dan grammar yang mudah dinalar serta di fuzz.
- Semua kesalahan variabel dan tipe ketahuan saat validasi, bukan saat start.
- Escape per format dibangun di renderer, tidak bisa dilupakan penulis template.

**Cons**:

- Kode sendiri yang harus dirawat dan di fuzz.
- Kontributor yang terbiasa Jinja kehilangan filter dan ekspresi; kebutuhan seperti menggabung path harus lewat field `managed`.

#### Option 2: minijinja dengan sandbox

Engine Jinja2 yang matang, tanpa loader file, undefined strict, dan fuel untuk membatasi langkah.

**Pros**:

- Matang, cepat, dan akrab bagi banyak kontributor.
- Ekspresi dan filter membuat template lebih ringkas.

**Cons**:

- Bahasa ekspresinya luas, jadi permukaan yang perlu ditinjau tumbuh setiap rilis.
- Cek variabel statis hanya sebagian (`undeclared_variables`), tipe tidak dicek.
- Escape otomatis dirancang untuk HTML; escaper per format tetap ditulis sendiri.

#### Option 3: Serializer terstruktur tanpa template teks

Manifest menulis isi config sebagai pohon data, engine menulisnya sebagai YAML, JSON, atau TOML.

**Pros**:

- Paling aman, tidak ada injeksi struktur sama sekali.

**Cons**:

- Format seperti `nats.conf`, `postgresql.conf`, `valkey.conf`, dan cnf MySQL tidak tercakup rapi, padahal itu mayoritas provider baseline.

#### Option 4: `handlebars-rust`

Grammar mirip Option 1 dari crate pihak ketiga.

**Pros**:

- Parser sudah ada.

**Cons**:

- Helper dan partial harus dimatikan, escape bawaannya hanya HTML, dan cek statis tetap ditulis sendiri, jadi penghematannya kecil.

### Keputusan 2: hubungan `versions.json` dengan `runtimes/inventory.toml`

#### Option 1: Dibangkitkan dari inventory

`cargo xtask codegen` menulis `versions.json` tiap provider, disimpan di git, dan `--check` gagal kalau drift.

**Pros**:

- Satu sumber kebenaran untuk digest dan URL.
- Field khusus tooling (closure, checksum upstream, GPG, resep) tidak ikut ke app.

**Cons**:

- Generator dan cek drift baru di xtask; paket tanpa entri inventory (fake) butuh aturan pengecualian.

#### Option 2: Ditulis tangan, dicek silang

**Pros**:

- Bebas dari generator.

**Cons**:

- Dua tempat untuk satu fakta; kesalahan salin digest baru ketahuan saat cek silang dijalankan.

#### Option 3: Tanpa `versions.json`

provider.yaml merujuk ID artefak inventory, dan app membawa satu indeks catalog hasil build.

**Pros**:

- Paling sedikit file.

**Cons**:

- Paket provider tidak lagi mandiri seperti PRD 21.1, dan import provider lokal (fitur 37) tidak punya tempat untuk katalog versinya.

#### Option 4: Inventory dipindah ke paket provider

**Pros**:

- Satu file per provider.

**Cons**:

- Field tooling ikut terkirim ke app, dan seluruh tooling spec 0003 harus ditulis ulang.

### Keputusan 3: himpunan probe v1alpha1 dan letak probe native

#### Option 1: Probe generik dibekukan, probe native di registry tertutup core

`tcp`, `http`, dan `exec` dibekukan. Probe native (misalnya `nats-readiness`) adalah deskriptor di registry core yang ditambah fitur provider lewat review.

**Pros**:

- Cek yang butuh pengetahuan provider (identitas server NATS, query terautentikasi PostgreSQL) ditulis sebagai kode yang ditinjau, bukan aturan JSON yang makin rumit.
- Setiap probe native terlihat sebagai perubahan core, jadi RISK-07 bisa diukur.

**Cons**:

- Fitur 9 belum punya probe native apa pun; manifest NATS memakai `http` dulu.

#### Option 2: Hanya probe generik

**Pros**:

- Tidak ada kode Rust per provider.

**Cons**:

- Cek identitas dan mode (JetStream yang diminta, query dengan role terbatas) menjadi bahasa aturan di manifest yang terus tumbuh, yang justru menambah RISK-08.

#### Option 3: Nama probe native dibekukan sekarang

**Pros**:

- Manifest NATS bisa langsung memakai `nats-readiness`.

**Cons**:

- Nama yang dikenal tanpa implementasi lolos validasi lalu gagal saat start, yang melanggar Done when.

### Keputusan 4: letak fake provider dan dummy server

#### Option 1: `providers/fake` plus crate `crates/fake-server`

**Pros**:

- Fake dimuat lewat loader dan aturan yang sama dengan provider nyata, dan menjadi contoh paket untuk kontributor (fitur 36).
- Crate bin kecil tanpa dependency Stackly bisa dipakai test di crate mana pun lewat helper testkit.

**Cons**:

- Paket uji ada di folder yang sama dengan paket asli, jadi pengecualian namespace `testing/` di catalog app wajib dijaga.

#### Option 2: Semua di testkit

**Pros**:

- Tidak mungkin ikut terkirim.

**Cons**:

- Kontributor tidak melihatnya sebagai paket nyata, dan bin di testkit tetap butuh helper build yang sama.

#### Option 3: Paket di `providers/`, server di testkit

**Pros**:

- Paket terlihat nyata, binary hidup di crate dev.

**Cons**:

- Dua lokasi untuk satu fixture, dan testkit menjadi crate dengan bin.

### Keputusan 5: timeout dan langkah reload

#### Option 1: Blok `timeouts` tetap dan reload bertahap

**Pros**:

- Readiness dan recovery, yang tidak punya langkah sendiri, tetap punya tempat untuk timeout nya.
- Reload jelas: render ulang, aksi, verifikasi; dan wajib ada jika dan hanya jika capability `reload`.

**Cons**:

- Engine harus memelihara tabel batas dan default.

#### Option 2: Timeout per langkah saja

**Pros**:

- Lentur.

**Cons**:

- Readiness dan recovery tidak bisa dinyatakan, padahal Done when memintanya.

#### Option 3: Manifest juga mengatur restart policy

**Pros**:

- Provider yang lambat bisa mengatur backoff sendiri.

**Cons**:

- Tumpang tindih dengan spec 0010 sebagai pemilik tunggal restart policy.

### Keputusan 6: aliran secret di lifecycle

#### Option 1: stdin, file 0600 sementara, dan template sensitif

**Pros**:

- Mencakup `initdb --pwfile`, `requirepass` Valkey, dan auth NATS tanpa pernah menaruh secret di argv atau env.
- Plan hanya membawa reference, jadi aman untuk ditampilkan dan dijournal.

**Cons**:

- Executor harus menyelesaikan reference saat menulis, dan config sensitif butuh aturan redaction tersendiri (spec 0011).

#### Option 2: Juga lewat env

**Pros**:

- Mudah untuk tool yang membaca password dari env (misalnya `PGPASSWORD`).

**Cons**:

- Env proses bisa dibaca proses lain milik user yang sama.

#### Option 3: Hanya stdin dan file sementara

**Pros**:

- Tidak ada secret di file config.

**Cons**:

- Valkey `requirepass` dan auth NATS tidak bisa dinyatakan.

### Keputusan 7: field yang tidak dikenal

#### Option 1: Ketat di semua bagian

**Pros**:

- Satu aturan, otomatis memenuhi VAL-018 untuk field keamanan, dan typo langsung ketahuan.

**Cons**:

- Setiap field baru butuh `apiVersion` atau perubahan additive lewat kebijakan fitur 36.

#### Option 2: Ketat di bagian keamanan saja

**Pros**:

- Field tampilan baru tidak mematahkan engine lama.

**Cons**:

- Dua aturan yang harus dijaga, dan batas "bagian keamanan" bisa bergeser.

### Keputusan 8: validator JSON Schema

#### Option 1: Tanpa validator generik

Decode serde bertipe plus fungsi aturan untuk manifest; dialek terbatas untuk settings schema yang diparse menjadi model field bertipe.

**Pros**:

- Pola yang sama dengan evidence spec 0002.
- Form fitur 20 dan 29 butuh model field bertipe; dialek terbatas memberi itu langsung.
- Dependency kecil (`regex`).

**Cons**:

- Kontributor tidak bisa memakai keyword JSON Schema di luar dialek.

#### Option 2: Crate `jsonschema`

**Pros**:

- Validator 2020-12 lengkap.

**Cons**:

- Dependency berat, dan GUI tetap tidak bisa merender form dari JSON Schema sembarang.

#### Option 3: Crate `boon`

**Pros**:

- Lebih kecil dan patuh 2020-12.

**Cons**:

- Catatan yang sama dengan Option 2 soal form.

### Keputusan 9: letak dan waktu pembuatan execution plan

#### Option 1: Plan per aksi di core, dibangun tepat sebelum aksi

**Pros**:

- Port Auto dan settings terbaru selalu dipakai; tidak ada plan basi.
- Fungsi murni yang mudah diuji golden, dan supervisor tidak perlu tahu manifest.

**Cons**:

- Plan dibangun ulang setiap aksi (murah, tanpa IO).

#### Option 2: Satu plan saat create, disimpan

**Pros**:

- Plan bisa dilihat tanpa paket.

**Cons**:

- Basi saat port atau settings berubah, dan butuh tabel registry baru.

#### Option 3: Tipe plan di domain

**Pros**:

- Bisa dipakai crate lain tanpa core.

**Cons**:

- Domain memuat detail format (argv, isi file) yang bukan aturan bisnis.

### Keputusan 10: aturan interpolasi

#### Option 1: Token di dalam string, tetap satu argumen

**Pros**:

- `--config=${instance.config}/nats.conf` bisa ditulis apa adanya, dan setiap entri tetap satu elemen argv.

**Cons**:

- Validator harus menormalisasi path dan mengecek containment per token.

#### Option 2: Token harus seluruh nilai

**Pros**:

- Paling sederhana dicek.

**Cons**:

- Manifest canggung, dan tool yang hanya menerima `--opt=nilai` tidak bisa dinyatakan.

### Keputusan 11: cakupan `manifest_sha256`

#### Option 1: `provider.yaml`, settings schema, dan template yang dirujuk

**Pros**:

- Digest mengikuti perilaku; versi runtime baru atau edit README tidak memaksa diff disetujui di fitur 59.

**Cons**:

- File lain di paket harus diatur (dilarang atau diabaikan) supaya tidak ada perilaku di luar digest.

#### Option 2: Seluruh isi paket

**Pros**:

- Paling sederhana.

**Cons**:

- Perubahan dokumentasi atau katalog versi menjadi perubahan revision untuk semua instance.

#### Option 3: `provider.yaml` saja

**Pros**:

- Paling kecil.

**Cons**:

- Perubahan template lolos tanpa perubahan identitas.

### Keputusan 12: permukaan yang dikirim fitur 9

#### Option 1: Library, `cargo xtask provider check`, dan test

**Pros**:

- Kontributor bisa memvalidasi paket di Mac ini tanpa menunggu IPC dan CLI.

**Cons**:

- Ada perintah xtask yang nanti tumpang tindih dengan `stackly provider validate` (fitur 36).

#### Option 2: Library dan test saja

**Pros**:

- Paling sedikit kode.

**Cons**:

- Tidak ada cara memvalidasi paket di luar test.

#### Option 3: Langsung `stackly provider validate`

**Pros**:

- Permukaan final sejak awal.

**Cons**:

- Fitur 9 tertahan di belakang fitur 6 dan 8.

### Keputusan 13: pemetaan capability

#### Option 1: Peta tetap ke `Capability` domain, dengan aturan pemenuhan

**Pros**:

- BR-016 berlaku seragam, termasuk untuk multi instance dan persistence.
- Capability yang belum bisa dipenuhi ditolak, bukan diklaim.

**Cons**:

- Spec 0004 perlu dua varian baru.

#### Option 2: Ikuti contoh PRD apa adanya

**Pros**:

- Tidak menyentuh spec 0004.

**Cons**:

- `install`, `multiInstance`, dan `persistence` menjadi sifat manifest yang tidak lewat `CapabilitySet::require`.

### Keputusan 14: aturan endpoint

#### Option 1: Loopback saja, dengan purpose

**Pros**:

- Sesuai default bind loopback fitur 20 dan catatan PRD 22.2 soal monitoring; purpose memberi tahu GUI endpoint mana untuk connection string.

**Cons**:

- Bind LAN butuh perubahan schema nanti (Deferred, OQ-017).

#### Option 2: Host bebas dengan warning

**Pros**:

- Lentur.

**Cons**:

- Default yang bisa mengekspos service ke LAN hanya dengan satu baris manifest.

### Keputusan 15: kedalaman paket contoh NATS dan Traefik

#### Option 1: Paket nyata dengan golden render

**Pros**:

- Menjadi titik awal fitur 23, 39, dan 41, dan membuktikan engine pada dua provider berbeda sejak awal (RISK-07).

**Cons**:

- Escaper `natsConf` dan bentuk config Traefik baru terbukti pada binary asli di fitur providernya.

#### Option 2: Fixture schema saja

**Pros**:

- Lebih cepat.

**Cons**:

- Fitur provider mulai dari nol, dan schema belum diuji pada bentuk config nyata.

### Keputusan 16: format template di fitur 9

#### Option 1: `yaml`, `json`, `natsConf` dulu

**Pros**:

- Cukup untuk tiga paket contoh, dan setiap escaper lain ditambah bersama test pada binary aslinya.

**Cons**:

- Empat fitur provider masing masing menambah satu perubahan core.

#### Option 2: Semua format sekarang

**Pros**:

- Fitur provider tidak perlu menyentuh core untuk format.

**Cons**:

- Escaper yang belum bisa dibuktikan pada binary aslinya.

### Keputusan 17: kedalaman struktur settings

#### Option 1: Datar plus daftar object satu tingkat

**Pros**:

- Cukup untuk `settings.routes`, dan form tetap sederhana.

**Cons**:

- Config yang benar benar bersarang harus diratakan.

#### Option 2: Object bersarang bebas

**Pros**:

- Lebih ekspresif.

**Cons**:

- Renderer form dan cek template jauh lebih rumit untuk kebutuhan yang belum ada.

### Keputusan 18: environment child process

#### Option 1: Env lengkap dari plan

**Pros**:

- Isi env bisa diuji dan tidak bergantung pada cara daemon dijalankan; dotfile user tidak terbaca karena `HOME` menunjuk ke folder temp instance.

**Cons**:

- Tool yang butuh variabel tertentu harus mendeklarasikannya di manifest.

#### Option 2: Warisi env daemon yang difilter

**Pros**:

- Lebih sedikit deklarasi.

**Cons**:

- Daftar larangan selalu tertinggal dari variabel berbahaya baru, dan perilaku berbeda antara daemon dari launchd dan dari terminal.

## Rationale

**Renderer sendiri (Keputusan 1)**: kekuatan yang paling menentukan adalah RISK-08 dan permintaan Done when bahwa kesalahan ketahuan saat validasi. Grammar tanpa ekspresi bisa dicek statis terhadap tipe settings, dan escaper yang terikat format membuat penulis template tidak bisa lupa escape. Harga yang dibayar (kode sendiri) kecil karena grammar nya kecil, dan fuzz target memang diwajibkan Done when.

**Generator dari inventory (Keputusan 2)**: inventory spec 0003 sudah menjadi tempat digest dipin, diverifikasi, dan dibuktikan spike. Menyalinnya dengan tangan ke tempat kedua hanya membuat peluang salin yang salah. Generator mempertahankan bentuk paket PRD 21.1 tanpa mengirim field tooling ke app.

**Probe generik plus registry native (Keputusan 3)**: PRD 22.1 dan 22.2 meminta readiness yang memeriksa identitas dan mode, bukan sekadar HTTP 200. Itu pengetahuan provider; menulisnya sebagai kode yang ditinjau lebih aman daripada bahasa aturan di manifest, dan scope fitur 23 memang menyebut `nats-readiness` sebagai satu perubahan core. Kontrak registry ditulis di spec ini, tetapi jenis `native` dan registry nya baru dibangun fitur 23 bersama probe pertamanya, supaya tidak ada kode yang dikirim tanpa pemakai dan test, dan janji "dikenal berarti bisa dijalankan" tetap terjaga.

**Fake di `providers/` (Keputusan 4)**: fake provider adalah bukti AC-029 bahwa tool baru tidak butuh patch core, jadi ia harus melewati jalur yang persis sama dengan provider nyata. Pengecualian namespace `testing/` di catalog app adalah aturan kecil yang bisa dites, sedangkan fake yang hidup di luar jalur nyata tidak membuktikan apa pun.

**Blok timeouts dan reload bertahap (Keputusan 5)**: Done when meminta timeout readiness dan recovery per provider serta langkah reload. Restart policy tetap di spec 0010, sesuai scope fitur 10 sebagai pemilik tunggal.

**Secret terbatas (Keputusan 6)**: argv dan env bisa dibaca proses lain milik user yang sama, sedangkan stdin dan file 0600 yang segera dihapus tidak. Template sensitif dibutuhkan provider baseline (Valkey, NATS), dan plan yang hanya membawa reference membuat journal dan tampilan plan aman.

**Ketat di semua bagian (Keputusan 7)**: schema masih alpha, dan satu aturan lebih mudah dijaga daripada dua. Perluasan sudah punya jalurnya lewat kebijakan kompatibilitas fitur 36.

**Tanpa validator generik (Keputusan 8)**: form harus dirender dari model field bertipe, dan validator JSON Schema generik tidak memberi model itu. Dialek terbatas yang diparse serde memberi validasi dan model form sekaligus, dengan satu dependency kecil.

**Plan per aksi (Keputusan 9)**: port Auto baru diketahui sesaat sebelum start, dan settings bisa berubah lewat fitur 29. Membangun plan tepat sebelum aksi menghindari plan basi tanpa tabel registry baru, dan fungsi murni membuat kontrak dengan spec 0010 bisa diuji lewat golden.

**Token di dalam string (Keputusan 10)**: AC-058 dan AC-031 dijamin oleh aturan "satu entri satu elemen argv", bukan oleh larangan teks campuran. Validasi path per token menutup risiko keluar folder.

**Digest perilaku (Keputusan 11)**: identitas revision dipakai fitur 59 (diff yang disetujui) dan fitur 37 (trust). Keduanya harus berubah saat perilaku berubah, dan tidak saat katalog versi bertambah.

**Library plus xtask (Keputusan 12)**: Done when meminta validasi di Mac ini, dan fitur 6 serta 8 belum ada. Perintah xtask kecil memenuhi itu tanpa menunggu.

**Capability dan endpoint (Keputusan 13 dan 14)**: BR-016 meminta capability yang tidak didukung dilaporkan, bukan dicoba. Itu hanya seragam kalau semua capability lewat `CapabilitySet`. Loopback sebagai satu satunya host mengikuti default fitur 20 dan larangan PRD 22.2 untuk mengekspos monitoring.

**Contoh nyata, tiga format, settings datar, env tertutup (Keputusan 15 sampai 18)**: semuanya memilih bentuk terkecil yang masih memenuhi Done when dan provider baseline, dengan perluasan lewat review saat ada bukti dari binary asli.

## Keputusan kecil

Diputuskan saat menulis spec, tanpa ditanyakan satu per satu:

- **Nama file tetap** (`versions.json`, `schemas/settings.schema.json`), jadi field `versionCatalog` dan `settingsSchema` dari contoh PRD dibuang. Runner up: field path seperti PRD, yang menambah permukaan tanpa manfaat.
- **`coreProtocolMajor` dibuang**: versi kontrak manifest dibawa `apiVersion`, dan versi protokol IPC milik spec 0006. Runner up: mempertahankannya sebagai field informatif, yang membingungkan dua jenis versi.
- **`resources.paths` dibuang**, karena setiap instance sudah punya semua folder dari layout spec 0004; yang penting adalah folder mana yang boleh ditulis (`permissions.managedFilesystem`).
- **`configFiles` sebagai peta bernama** yang dirujuk langkah dan reload, supaya reload bisa merender ulang per nama dan digest tahu template mana yang dipakai. Runner up: `renderTemplate` dengan source dan destination di tiap langkah, seperti PRD.
- **`additionalExecutables` sebagai peta alias**, supaya bisa dirujuk lewat token `${runtime.executables.<alias>}`. Runner up: daftar path seperti PRD, yang tidak bisa dirujuk token.
- **`executeRuntimeBinary` wajib `true`** sebagai pernyataan eksplisit yang terlihat saat review trust. Runner up: membuangnya.
- **Field settings `managed` sebagai nilai turunan** (misalnya `serverName` dari nama instance, `dynamicDir` Traefik), menggantikan kebutuhan ekspresi penggabung path di template.
- **Aturan `argument_injection_risk`**: entri `args` tidak boleh diawali token settings teks tanpa `enum` atau integer yang boleh negatif, supaya nilai pengguna tidak bisa menjadi flag baru. Runner up: tanpa aturan, mengandalkan penulis manifest.
- **Escaper `natsConf` konservatif**: `"`, `\`, `$`, dan karakter kontrol ditolak, bukan di escape, karena aturan escape dan variabel di config NATS belum dibuktikan pada binary asli. Fitur 23 boleh melonggarkannya dengan test.
- **`loop.first`, `loop.last`, dan `#if` pada array** untuk koma di JSON dan blok kosong di YAML, tanpa menambah ekspresi.
- **Field secret selalu `required`**, dan setiap field bukan secret wajib `required` atau punya default, supaya template tidak pernah bertemu nilai kosong.
- **Issue dikumpulkan sampai 100** untuk aturan semantik, supaya kontributor melihat semua kesalahan sekaligus tanpa output yang tak terbatas. Decode struktural tetap satu issue per file karena serde berhenti di error pertama; `serde_path_to_error` memberi path nya.
- **Tipe decode mentah**: nama, versi, dan enum didecode sebagai string lalu diubah di pass semantik, supaya nilai yang salah mendapat kode spesifik (`unknown_primitive`, `invalid_name`) dan supaya schemars bisa menulis pattern nya. Runner up: decode langsung ke tipe domain, yang memberi pesan serde generik.
- **`PackageFiles` dan `from_files` terpisah dari `read_package`**, supaya fuzz target dan test bisa memvalidasi tanpa filesystem, dan supaya `FakeFilesystem` yang terkurung di `TestRoot` cukup untuk membuktikan loader tidak menulis apa pun.
- **Anchor dan alias YAML** tetap diterima dalam batas repetisi dan rekursi `serde_yaml_ng`, sedangkan tag dan merge key gagal di decode ketat. Batas itu dibuktikan test billion laughs dengan batas waktu 2 detik; kalau gagal, keputusan ini kembali ke `/architect` (menolak anchor butuh parser event yang aman, yang belum ada di stack).
- **Pattern selalu dicocokkan penuh** (dibungkus `\A(?:…)\z`), karena syarat `^` dan `$` tekstual bisa diakali `^a|b$`. Runner up: memeriksa AST regex, yang lebih rumit tanpa manfaat tambahan.
- **Output `yaml` dan `json` diparse ulang**, karena escaper menghasilkan scalar berkutip yang rusak kalau tag ditempel ke teks lain. Runner up: aturan statis tentang karakter di sekitar tag, yang sulit didefinisikan dengan tepat.
- **Field `apply: reload` hanya di template yang dirender ulang saat reload**, supaya kelas reload tidak berbohong ke form fitur 29.
- **Semua token folder wajib di `managedFilesystem`**, termasuk di `args` dan `env`, supaya deklarasi permission menjadi daftar lengkap folder yang dipakai provider.
- **`SecretRef` sementara bertanda `// provisional: F11`**, karena spec 0011 belum dibangun saat fitur 9 dibangun.
- **Plan hanya `Serialize`**, tidak pernah didecode, supaya plan yang diubah di journal tidak bisa dieksekusi.
- **`args` bertipe `String`** karena manifest dan layout selalu UTF-8; path yang bukan UTF-8 ditolak dengan error jelas.
- **`cwd` default `${instance.data}`**, sama dengan contoh PRD.
- **Destination config boleh satu subfolder** (`config/dynamic/routes.yaml`), karena Traefik sebaiknya mengawasi folder, bukan satu file yang diganti lewat rename atomik.
- **`signal-hook` untuk dummy server**, karena menangani SIGTERM dan SIGHUP tanpa `unsafe` di kode kita (AGENTS.md hanya mengizinkan `unsafe` di `stackly-platform-macos`). Runner up: `ctrlc`, yang tidak menangani SIGHUP. `serde_json` dan `sha2` ikut karena dummy membaca config JSON, menulis argv JSON, dan menghitung SHA-256 token; `check-deps` hanya melarang crate Stackly di sana.
- **Perintah `shutdown` dan paket `fake-command`**, supaya stop `command`, reload `run`, probe `tcp` dan `exec`, dan `materializeSecret` punya test nyata tanpa membuat paket fake utama terlalu ramai.
- **Probe `exec`** masuk himpunan generik, karena banyak provider punya CLI ping (`valkey-cli`, `mysqladmin`) yang bisa dipakai tanpa kode core.
- **Probe readiness NATS memakai `/healthz`** tanpa parameter JetStream, karena `/healthz` sudah gagal kalau JetStream aktif tapi belum siap, sedangkan `js-enabled-only` membuat instance tanpa JetStream tidak pernah ready.
- **Port saran Traefik 8000 dan 8082**, supaya tidak bertabrakan dengan 8080 yang umum dipakai aplikasi pengguna (PRD 22.5).
- **Preflight lewat `dataMarker` deklaratif**, karena scope fitur 9 menyebut preflight kompatibilitas data sebagai bagian kontrak adapter; PostgreSQL cukup mendeklarasikan `PG_VERSION` tanpa kode core.
- **Dependency antar runtime (disebut REQ-029) belum dimuat** di v1alpha1, karena belum ada provider baseline yang membutuhkannya; penambahannya additive lewat fitur 36.
- **Penanda spike pindah ke fitur 36**, karena langkah spike baru bisa diganti saat ada harness yang menjalankan plan dari paket.
- **Durasi fuzz 10 menit per target** di Mac ini, cukup untuk menangkap panic dan hang yang dangkal; fitur 63 memperluasnya.
- **Tidak ada pencarian Agent Skill atau MCP**: pilihan baru di fitur ini adalah library kecil (`regex`, `signal-hook`, `serde_yaml_ng` yang sudah diputuskan spec 0001), bukan tool atau framework baru.

## Cross check

Sonnet 5.5 membaca draf pertama secara read only pada 2026-10-18. Pengguna menyetujui semua perbaikan yang direkomendasikan, dan semuanya sudah masuk ke spec:

- **Kelengkapan keputusan**: tabel sumber timeout dan exit code; byte persis secret di stdin dan file sementara, plus `PlanSecret { field, reference }`; Assumed spec 0010 yang lebih lengkap (`EnsureDirectories`, `ensure_instance_run_dir`, `check_owned_path`, `materialize`, operasi hapus file); pemisahan `read_package` dari `from_files` dengan `NameCheck`; langkah 0 untuk perubahan domain dan `SecretRef` sementara; decode dengan `serde_path_to_error` dan tipe mentah; aturan nama dan charset; matriks token dan posisi; aturan dialek settings yang tersisa; bentuk output dan pemetaan generator `versions.json`; detail CLI dummy server, perintah `shutdown`, dan paket `fake-command`; aturan preflight (trim, `MarkerUnreadable`, prioritas registry); context golden yang tetap.
- **Kesehatan desain**: charset `socketName` supaya tidak keluar folder run; pattern yang selalu dicocokkan penuh; parse ulang output `yaml` dan `json`, render yang diputus di 1 MiB, dan escape DEL serta U+FEFF; test billion laughs; `EndpointNotLoopback` di plan builder; daftar `PlanError` yang lengkap dengan `ArgumentTooLong` dan larangan karakter kontrol di settings; cek `apply: reload` terhadap pemakaian; dev dependency testkit untuk dummy server; jenis probe `native` dipindah ke fitur 23; catatan sesi paralel diperbaiki (langkah 5 menunggu 3 dan 4); pattern schemars di tipe mentah.

## Perubahan 2026-10-18 (rekonsiliasi lintas spec)

Pengecekan silang Assumed lines antara spec 0005, 0006, 0009, 0011, dan 0012 menemukan selisih yang keputusannya milik spec ini. Engineer menyetujui semua perbaikan berikut.

| Selisih | Perbaikan | Alasan | Runner up |
| --- | --- | --- | --- |
| Secret ditolak di `env`, padahal spec 0011 (OQ-008) mengizinkan env sebagai mekanisme keempat | nilai `env` boleh berupa tepat satu token `${secret.<field>}` tanpa teks lain, menjadi `EnvValue::Secret`, hanya di `lifecycle.start` dan `runExecutable` (cross check: probe dan stop tidak menyelesaikan secret) | beberapa tool hanya membaca credential dari env; satu token utuh mencegah secret tercampur ke teks lain | env tetap ditolak sampai provider pertama butuh |
| `materializeSecret` bisa menulis ke `${instance.temp}` di root data | hanya `${instance.run}/secrets/<nama>` | root data ikut backup dan Time Machine, sedangkan folder `secrets/` spec 0011 ada di runtime root dan disapu saat start | `${instance.run}` langsung tanpa subfolder |
| `materialize` memakai resolver mentah yang melewati worker Keychain, `NeedsSecret`, interaksi, dan allowlist `expose()` | `materialize(&ResolvedSecrets)` dan pembacaan nilai lewat `secret::delivery` spec 0011 | satu jalur baca secret membuat dialog Keychain, needs secret, dan redaksi konsisten | resolver tetap, dibungkus executor |
| Mekanisme `hash` spec 0011 tidak bisa dinyatakan di manifest | dicatat sebagai filter template additive yang ditambah fitur provider pertama yang butuh | v1alpha1 sudah mengizinkan tambahan additive, dan belum ada paket di fitur ini yang butuh hash | filter dirancang sekarang |
| Kode `manifest.*` dan `provider.*` belum ada, dan semua error dipetakan ke `invalid_input` | tabel *Kode error di wire* dengan kategori per varian dan daftar issue di `ErrorBody.items` ber `kind` `issue` | `PlanError::Unsupported` adalah BR-016 (`unsupported`), dan error render adalah kegagalan kerja, bukan input | satu kode generik dengan `IssueCode` di detail |
| Bentuk `ResourceRef.id` provider belum ditetapkan | `<namespace>/<name>@<revision>#<manifest_sha256>` | tiga bagian kunci terbaca manusia, dan pemisahnya tidak pernah muncul di nama atau label | sha256 saja |
| Reference golden `fixture:authToken` ditolak parser spec 0011 | `local:instance/<id golden>/auth-token` | golden harus tetap valid setelah tipe sementara diganti | |
| Harness fuzz dan replay regresi belum mengikuti spec 0006 | modul `provider::fuzzing` dan `crates/core/tests/fuzz_regressions/` | target dan test stable memanggil fungsi yang sama | harness di folder `fuzz/` |

Cross check model lain atas update ini menambah, atas persetujuan engineer: `ResolvedSecrets` sementara bertanda `// provisional: F11` supaya test plan fitur 9 bisa dibuat sebelum fitur 11, dan penolakan `secret_purpose_collision` untuk dua secret field yang purpose turunannya sama.

