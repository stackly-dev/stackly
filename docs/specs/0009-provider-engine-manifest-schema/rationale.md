# 0009. Rationale: provider engine dan manifest schema

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: fitur 9 membekukan `stackly/v1alpha1` sebelum provider nyata pertama (PostgreSQL, fitur 19) dibangun. Risikonya, schema terlalu sempit untuk PostgreSQL lalu harus diubah saat banyak hal sudah menumpang di atasnya (RISK-07). Karena itu schema ini sengaja memuat kait yang dibutuhkan PostgreSQL walau belum ada provider yang memakainya hari ini: langkah `runExecutable` untuk `initdb`, secret lewat stdin dan file 0600 sementara, `socketName` yang memuat port (`.s.PGSQL.<port>`), penanda data (`PG_VERSION`) untuk preflight, dan registry probe native. Yang sengaja ditunda adalah hal yang aturannya hanya bisa dibuktikan pada binary aslinya (escaper `postgresqlConf` dan probe query terautentikasi). *Fase 02*: escaper `postgresqlConf` ditambah spec 0019, dan readiness PostgreSQL memakai probe `exec` dengan `psql`, bukan probe native. Fitur ini juga bergantung pada keputusan spec yang belum ditulis (0005, 0006, 0010, 0011); setiap ketergantungan itu ditulis sebagai baris Assumed di spec, bukan diputuskan di sini.

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

**Probe generik plus registry native (Keputusan 3)**: PRD 22.1 dan 22.2 meminta readiness yang memeriksa identitas dan mode, bukan sekadar HTTP 200. Itu pengetahuan provider; menulisnya sebagai kode yang ditinjau lebih aman daripada bahasa aturan di manifest, dan scope fitur 23 memang menyebut `nats-readiness` sebagai satu perubahan core. Kontrak registry ditulis di spec ini, tetapi jenis `native` dan registry nya baru dibangun fitur 23 bersama probe pertamanya, supaya tidak ada kode yang dikirim tanpa pemakai dan test, dan janji "dikenal berarti bisa dijalankan" tetap terjaga. *Diperbarui fase 02 (spec 0023)*: runner dipilih lewat `match` exhaustive atas enum tertutup, bukan trait milik spec 0010 seperti tertulis semula, dan `params` boleh memakai token interpolasi; lihat *Perubahan 2026-10-20 (fase 02)*.

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

**Contoh nyata, tiga format, settings datar, env tertutup (Keputusan 15 sampai 18)**: semuanya memilih bentuk terkecil yang masih memenuhi Done when dan provider baseline, dengan perluasan lewat review saat ada bukti dari binary asli. *Fase 02*: format menjadi lima (`postgresqlConf` dan `plain` dari spec 0019).

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
- **Escaper `natsConf` konservatif**: `"`, `\`, `$`, dan karakter kontrol ditolak, bukan di escape, karena aturan escape dan variabel di config NATS belum dibuktikan pada binary asli. Fitur 23 boleh melonggarkannya dengan test. *Fase 02*: spec 0023 memutuskan tidak melonggarkannya, karena nilai yang dirender NATS (nama instance, alamat loopback, path data) tidak butuh ketiga karakter itu.
- **`loop.first`, `loop.last`, dan `#if` pada array** untuk koma di JSON dan blok kosong di YAML, tanpa menambah ekspresi.
- **Field secret selalu `required`**, dan setiap field bukan secret wajib `required` atau punya default, supaya template tidak pernah bertemu nilai kosong.
- **Issue dikumpulkan sampai 100** untuk aturan semantik, supaya kontributor melihat semua kesalahan sekaligus tanpa output yang tak terbatas. Decode struktural tetap satu issue per file karena serde berhenti di error pertama; `serde_path_to_error` memberi path nya.
- **Tipe decode mentah**: nama, versi, dan enum didecode sebagai string lalu diubah di pass semantik, supaya nilai yang salah mendapat kode spesifik (`unknown_primitive`, `invalid_name`) dan supaya schemars bisa menulis pattern nya. Runner up: decode langsung ke tipe domain, yang memberi pesan serde generik.
- **`PackageFiles` dan `from_files` terpisah dari `read_package`**, supaya fuzz target dan test bisa memvalidasi tanpa filesystem, dan supaya `FakeFilesystem` yang terkurung di `TestRoot` cukup untuk membuktikan loader tidak menulis apa pun.
- **Anchor dan alias YAML** tetap diterima dalam batas repetisi dan rekursi `serde_yaml_ng`, sedangkan tag lokal dan merge key gagal di decode ketat. Batas itu dibuktikan test billion laughs dengan batas waktu 2 detik; kalau gagal, keputusan ini kembali ke `/architect` (menolak anchor butuh parser event yang aman, yang belum ada di stack).
- **Tag YAML standar diabaikan, bukan ditolak** (diputuskan pengguna 2026-10-18 setelah `/check verify` fitur 9): `serde_yaml_ng` menyelesaikan tag dengan handle `!!` dan tag verbatim menjadi URI penuh dan memakainya hanya untuk memilih cara satu scalar dibaca, jadi `Value::Tagged` hanya muncul untuk tag lokal `!nama`, dan hanya tag lokal yang bisa ditolak. Run 2026-10-18: `!!int "18080"` dibaca sebagai angka, `!!str 18080` sebagai teks (lalu ditolak decode bertipe untuk port), `!!null true` dan `!!int "0.1.0"` gagal sebagai `yaml_syntax`, sedangkan `!!binary`, `!!seq`, `!!map`, dan tag verbatim `str` tidak mengubah apa pun. Setiap hasil bisa ditulis tanpa tag, jadi tag standar tidak membuka nilai baru, dan decode bertipe ketat tetap memeriksa hasilnya (key asing seperti `!!str network` tetap `unknown_field`). Runner up: pemindai leksikal yang menolak setiap tag di awal node, yang menambah kode parser buatan sendiri di jalur keamanan dan bisa salah tolak di kasus pinggir; atau parser event lain, yang membuka lagi keputusan `serde_yaml_ng` di spec 0001.
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
- **Probe readiness NATS memakai `/healthz`** tanpa parameter JetStream, karena `/healthz` sudah gagal kalau JetStream aktif tapi belum siap, sedangkan `js-enabled-only` membuat instance tanpa JetStream tidak pernah ready. *Diganti fase 02*: sejak spec 0023 readiness NATS memakai probe native `nats-readiness` (identitas `server_name`, mode JetStream yang diminta, PING dan PONG, dan `$JS.API.INFO`), karena `/healthz` tidak bisa membedakan server lain di port itu.
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

Ronde 2 rekonsiliasi (2026-10-18), atas persetujuan engineer: `materializeSecret.destination` cukup nama file dengan token folder `${instance.secrets}` (spec 0010), `RunPlan.render` dijalankan sebelum setiap spawn, arti `foreground` mencakup tanpa re exec dan tanpa menutup fd warisan, file secret dihapus lewat `remove_owned_tree`, dan dua contoh test yang keliru dibetulkan (`...port}xy` 16 byte; kutip di dalam scalar YAML berkutip). Catatan kode: build milestone 1 waktu itu masih memakai `materialize` dengan resolver dan tujuan `${instance.run}` atau `${instance.temp}`. Selisih ini sudah tertutup (2026-10-19): rekonsiliasi fitur 9 (`22d4257`) membuat `materializeSecret.destination` satu nama file di `${instance.secrets}` dan `materialize` menerima `&ResolvedSecrets`, lalu langkah 4a fitur 11 mengganti `ResolvedSecrets` sementara dengan milik `stackly_core::secret`, yang membaca nilai lewat `secret::delivery`. Verify 2026-10-19 menjalankan keduanya sampai proses nyata.


## Perubahan 2026-10-19 (review model segar fitur 9)

Review Sonnet atas fitur 9 (`docs/reviews/2026-10-19-claude-f9-closeout-c0a0a3.md`) menemukan dua celah keamanan dan satu teks spec yang longgar. Keputusan berikut diambil di sesi close-out fitur 9 atas persetujuan tetap engineer untuk memakai opsi yang direkomendasikan.

| Selisih | Perbaikan | Alasan | Runner up |
| --- | --- | --- | --- |
| Batas repetisi `serde_yaml_ng` menghitung lompatan alias, bukan node, sehingga satu anchor besar di balik rantai alias pendek (5 KB) mengembang menjadi ratusan juta node, di `provider.yaml` maupun di output template `yaml` | anggaran node: dokumen boleh mengembang paling banyak dua node per byte teks ditambah 1024, dihitung oleh visitor yang tidak menyimpan apa pun sebelum nilai dibangun | dokumen tanpa alias tidak pernah melewati satu node per byte, jadi anggaran ini hanya mengenai ekspansi alias dan tetap berlaku untuk output template sampai 1 MiB | anggaran tetap 200 000 node, yang bisa menolak output template besar yang sah |
| `pattern` yang tidak seimbang (`a)\|(b`) lolos kompilasi setelah dibungkus dan berubah menjadi pencocokan awalan atau akhiran | pattern wajib lolos kompilasi sendirian lebih dulu | pattern yang valid sendirian selalu tertutup di dalam grup pembungkus | menolak `(` dan `)` yang tidak seimbang dengan parser sendiri |
| Teks *Execution plan* bilang context bukan UTF-8 selalu menghasilkan `PathNotUtf8`, padahal path dicek saat dipakai | teks menyebut folder yang dipakai plan | folder yang tidak dipakai tidak pernah masuk plan, dan test `every_plan_fails_for_a_folder_it_uses_that_is_not_utf8` mengunci perilaku ini | cek kelima folder dan root runtime di awal setiap plan |

## Perubahan 2026-10-20 (fase 02)

Update `architect (update)` spec 0009 atas permintaan lead scope 02 (instalasi pertama), setelah fitur 9 ditutup di `main` (667510b). Sumber nya daftar gabungan *Spec 0009* di `docs/scope/coordination/02-instalasi-pertama.usulan-fondasi.md` (30 usulan P1 sampai P30 dari spec 0015, 0017, 0018, 0019, 0020, dan 0023, dengan 16 resolusi konflik); setiap baris sumber yang dikutip dibaca lebih dulu. Semua perubahan additive atau klarifikasi, ID AC tidak berubah, dan tidak ada kode, schema yang dibangkitkan, atau paket yang disentuh: `schemas/provider/*.json` dan paket dibangkitkan ulang build pemiliknya (fitur 18 langkah 1, fitur 19 langkah 1 sampai 3, fitur 23 langkah 1). Keputusan lead scope 02 atas nama user (opsi yang direkomendasikan) ada di kolom terakhir.

| No | Perubahan di spec ini | Spec sumber | Catatan atau keputusan |
| --- | --- | --- | --- |
| 1 | Blok *Perluasan additive fase 02* setelah *Peta modul*, dengan pemilik, langkah build, dan AC tiap perluasan | semua | tanpa bump `ProtocolVersion` untuk update teks ini |
| 2 | AC-4: jenis probe `native` (hanya nama registry) dan format `postgresqlConf` serta `plain` | 0023, 0019 | |
| 3 | AC-9, *ConfigFile*, dan *Template config*: escaper `postgresqlConf`, format `plain` tanpa tag output, kalimat "tanpa mode raw" diperjelas, daftar format provider berikutnya | 0019 | `plain` bukan mode raw karena tidak pernah mengeluarkan nilai |
| 4 | *Peta modul*: submodul `probe` (`native`, `nats`) tanpa IO; IO di `core::supervisor::probe` | 0023 | |
| 5 | Tabel manifest: `runtime.versionCheck`; `runExecutable.stdin` menjadi `{ secret }` atau `{ text }`; bentuk Probe mendapat `type: native` | 0018, 0019, 0023 | pattern `versionCheck` dicari per baris tanpa anchor, berbeda dengan pattern settings dan `dataMarker` |
| 6 | Matriks interpolasi: baris `stdin` teks, baris `params` probe native, baris `runtime.versionCheck.args` | 0019, 0023, 0018 | resolusi 4: param teks menerima token teks, integer, dan boolean seperti `args` (tabel spec 0023), param boolean tepat satu token boolean |
| 7 | *Settings schema*: paragraf pembuka diganti persis teks spec 0020 (urutan deklarasi, `SettingsIssueKind`); `printableOnly`; aturan `apply: reload` mencakup `params`; paragraf validate | 0020, 0019, 0023 | resolusi 13: kind per issue di field baru, bentuk `SettingsError.issues` dan wire tetap; reference secret yang gagal parse memakai kind `type` |
| 8 | *Probe*: kontrak probe native spec 0023; NATS memakai `nats-readiness` | 0023 | resolusi 5: `match` exhaustive menggantikan trait, dan `params` boleh bertoken |
| 9 | Baris `ProbeSpec::Native` di *Sumber timeout dan exit code* | 0023 | |
| 10 | *`versions.json`*: tabel field fase 02, bentuk record `compatibility`, `blocked` boleh `build`, tipe core `versions()`, pemetaan generator | 0017, 0018 | resolusi 1: `build` di `Available` dan `Blocked`, field lain hanya `Available`; resolusi 2 (keputusan lead): `min_os` dan `os_version` bertipe `OsVersion`, `format` bertipe `ArtifactFormat`, dari update spec 0004 (usulan spec 0018), yang berjalan paralel dengan update ini, wajib ada di kode sebelum fitur 17 milestone 1 langkah 1; resolusi 3: urutan output mengikuti tabel field |
| 11 | *Execution plan*: `Stdin::Text`, `ProbeSpec::Native` dan `NativeProbeSpec`, `UnrepresentableValue` untuk param teks native, nama konstanta `CHILD_PATH` | 0019, 0023, 0018 | resolusi 15: varian baru dan cabang supervisornya mendarat bersama di build pemiliknya lewat update spec 0010 terpisah |
| 12 | Catatan paket `fake`, `nats`, `traefik` dan contoh NATS | 0018, 0023 | resolusi 9: fake memakai subcommand `version` yang sudah ada |
| 13 | Fuzz target `nats_readiness_protocol` dan `provider_template` ke lima format | 0023, 0019 | |
| 14 | Kode issue: empat kode loader catalog masuk `IssueCode` dengan `as_code()` stabil | 0017 | resolusi 12 (keputusan lead): ditambah di update ini; `file` dan `location` milik spec 0017 |
| 15 | *API surface*: `versions`, `fields` dan `SettingsIssueKind`, `version_check`, registry native; catatan `files()` diarahkan ke fitur 20 (K-1) | 0017, 0018, 0020, 0023 | resolusi 11: baris Follow-up `files()` yang dirujuk spec 0017 tidak pernah ada di spec ini; nama accessor `version_check` ditetapkan di sini karena spec 0018 tidak menamainya |
| 16 | Baris Assumed spec 0011 (`printableOnly` di `put`) dan spec 0003 (field inventory baru) | 0019, 0018 | |
| 17 | *Critical test scenarios*: *Himpunan tertutup* memakai nama native tak terdaftar; penunjuk ke test milik spec pemilik dan daftar fixture spec 0018 | 0023, 0019, 0018 | |
| 18 | *Consequences* diperbarui; keputusan kecil `/healthz` dan kata "trait" di *Rationale* ditandai diganti; catatan escaper `natsConf` | 0019, 0023 | |
| 19 | Follow-up fitur 18, fitur 17 dan 15, fitur 20 dan 21, dan fitur 23 dicentang; fitur 36 tetap terbuka | 0018, 0017, 0015, 0020, 0023 | resolusi 16 (keputusan lead): dicentang di sesi ini |
| 21 | Blok manifest `connection` (baris tabel manifest), posisi interpolasi `connection.template`, kode issue `connection_invalid` dan `connection_name_duplicate`, dan baris di *Perluasan additive fase 02* | 0021 | diterapkan apa adanya dari *Usulan update spec lain* spec 0021 setelah spec itu di merge (271cde2); tidak bertabrakan dengan probe native atau `versions.json`. `connection_invalid` membawa reason (0021 AC-1 menguji reason `not_a_uri`), dan `template_length` serta `not_a_uri` berbagi lokasi; keputusan lead 2026-10-20 (opsi A yang direkomendasikan): field opsional `Issue.reason` (`Option<&'static str>` snake_case stabil), dikirim sebagai kunci `reason` tambahan di item wire `issue` (additive, cocok dengan `ErrorItem` spec 0006), dengan perubahan `ItemSpec` dan bump minor milik fitur 21 langkah 1. Runner up: memecah menjadi kode `connection_not_a_uri` sendiri, yang mengubah keputusan spec 0021 |
| 20 | Tidak diterapkan di sini: `/graphify . --update` | | atas permintaan lead, graph tidak dibangun di worktree; lead memperbaruinya di `main` |

Catatan lain: header `**Status**` tetap milik lead (/sync). Blok `connection` spec 0021 masuk di putaran yang sama (baris 21). Spec 0023 baris usulan item 4 (matriks interpolasi) masih menulis bahwa boolean hanya untuk param boolean, berbeda dengan tabel spec 0023 sendiri dan resolusi 4; spec ini mengikuti tabel, dan perbaikan teks spec 0023 dilaporkan ke lead.

Cross check: Sonnet 5.5 membaca draf update ini secara read only (tanpa menjalankan apa pun) terhadap daftar perubahan dan setiap baris sumber. Tidak ada item atau resolusi yang hilang atau menyimpang dari sumbernya. Dua belas temuan konsistensi diperbaiki tanpa keputusan baru: kalimat lama yang masih menulis jenis probe, stdin, sumber data artefak, dan probe loopback versi fitur 9; alasan bump `ProtocolVersion` yang kini menyebut bump milik build pemilik; bentuk `SettingsIssueKind` yang diserahkan ke build fitur 20; `file` dan `location` issue loader yang diserahkan ke fitur 17; nama `version_check` yang ditandai sementara; urutan langkah fitur 23; `printableOnly` dengan default `false`; `osVersion` record yang juga harus valid sebagai `OsVersion`; urutan `VersionCatalog::artifacts()`; dan penanda fase 02 di rationale.

Penyelarasan dengan spec 0010 dan 0011 di `main` (2026-10-20, keputusan K lead foundation): tabel *Perluasan additive fase 02* dan catatan *Execution plan* kini menyebut pemilik yang benar. Executor `Stdin::Text` adalah spec 0010 langkah 7 (`StdinSource::Bytes(SecretValue::new(text))`, kegagalan `init_step_failed`, K8), yang hanya diperiksa fitur 19 langkah 5. Arm `ProbeSpec::Native` ditambah fitur 23 langkah 1, dengan driver di langkah 5 dan 6 di atas `ProbeOutcome` dan redaksi detail spec 0010 langkah 6 (K3). Pemeriksaan `printableOnly` di `put` adalah spec 0011 langkah 4a (AC-22, K13). Kunci `details` `reason` pada `secret.invalid_value` masuk bersama `codes::secret` di spec 0011 langkah 5 (K4), bukan fitur 19 langkah 3. Kode issue loader catalog dibangun fitur 17 langkah 3. Resolusi 15 di baris 11 tabel di atas ("varian baru dan cabang supervisornya mendarat bersama di build pemiliknya") diganti pembagian ini.

Perbaikan pasca penyelarasan (2026-10-20, `docs/scope/coordination/post-alignment/post-0009.md` dan keputusan lead D1 dan D2): field core `Issue.reason` kini dibangun fitur 17 milestone 1 langkah 3 karena `catalog_limit` adalah pemakai pertamanya; kunci wire `reason`, `ItemSpec`, dan kenaikan minor ikut langkah build pertama yang mengirim issue ber reason lewat wire (fitur 17 langkah 5 kalau issue catalog sampai ke wire, selain itu fitur 21 milestone 1 langkah 1), aturan yang sama dengan spec 0017 dan 0021; baris tabel *Perluasan additive fase 02* untuk `Issue.reason` dipisah dari baris blok `connection`. `file`, `location`, dan `reason` issue loader kini merujuk tabel *Issue loader* spec 0017, bukan butir terbuka. Blok `connection` NATS ditulis fitur 23 langkah 2 saja (D1). "detail `reason`" di baris tabel manifest menjadi "reason di `Issue.reason`". Baris 21 tabel di atas (bump di fitur 21 langkah 1) diganti aturan ini.

Tambahan lead pada putaran yang sama (2026-10-20): aturan D2 kini pasti lewat spec 0017, yang hanya mengirim issue catalog di DTO `ProviderDetail.issues` nya sendiri (`CatalogIssue.reason`), tidak pernah sebagai `ErrorBody.items` ber kind `issue`; jadi field core `Issue.reason` tetap milik fitur 17 milestone 1 langkah 3, sedangkan kunci wire `reason` di item `issue`, `ItemSpec`, dan kenaikan minor milik fitur 21 milestone 1 langkah 1 tanpa syarat. Arm executor `Stdin::Text` mengikuti aturan baru spec 0010 langkah 7: ditambah yang di merge belakangan di antara langkah itu dan fitur 19 langkah 2.

## Perubahan 2026-10-20 (config sensitif di folder secrets)

Update `architect (update)` spec 0009 atas brief lead foundation (sesi `arc-sensitive-config`). Sumbernya bagian *Penilaian celah yang diketahui* di review fitur 11 (`docs/reviews/2026-10-20-f11-secret-store.md`): `providers/fake` merender `{{ secret.authToken }}` dalam plaintext ke `${instance.config}/fake.json` dengan `sensitive: true`, karena cek posisi di `crates/core/src/provider/interpolate.rs` menerima destination config sensitif di folder instance mana pun, sedangkan `remove_secret_material` dan sweep hanya menyentuh `instance_secrets_dir` (`crates/core/src/secret/delivery/material.rs`). Token jadi tertinggal di root data setelah stop, tidak pernah disapu, dan ikut cold backup (`backup.cold.paths: [data, config]`). Spec 0011 mekanisme 5 sudah menyatakan file plaintext ada di `<runtime>/i/<short-key>/secrets/`, tetapi matriks posisi spec ini hanya bilang `${instance.secrets}` "diterima".

Keputusan user (dikutip lead, sudah disetujui): "Ya: lewat /architect, spec 0009 mewajibkan config sensitive di ${instance.secrets}, paket fake menulis ke ${instance.secrets}/fake.json, dan pengecualian fake.json di AC-18 spec 0014 dihapus. Saya setujui perubahan teks AC itu."

| Hal | Keputusan | Alasan | Runner up |
| --- | --- | --- | --- |
| Tempat config `sensitive` | wajib diawali `${instance.secrets}`, ditolak saat validasi semantik (AC-20) | keputusan user; setiap byte secret yang dirender template lalu jatuh di folder yang sudah dihapus `remove_secret_material` dan disapu, dan folder itu ada di runtime root yang tidak ikut backup, Time Machine, `config-history`, atau tampilan config (spec 0011) | tetap boleh di folder mana pun dan menambah path config sensitif ke penghapusan dan sweep: secret tetap di root data selama instance berjalan (snapshot Time Machine), dan sweep harus tahu path di luar folder milik nya |
| Kode issue | `token_not_allowed_here` yang sudah ada, satu issue di `configFiles.<nama>.destination`, pesan menyebut `${instance.secrets}` | aturan ini adalah aturan posisi token folder, sama persis dengan kebalikannya (config tidak sensitif di `${instance.secrets}`) yang sudah memakai kode ini di cek posisi yang sama; tanpa `IssueCode` baru, test jumlah kode di `issue.rs`, `codes::manifest`, fixture `protocols/`, dan `ProtocolVersion` tidak berubah; review F11 merekomendasikan kode yang sama | kode baru `sensitive_config_outside_secrets`: lebih spesifik untuk kontributor, tetapi menambah kontrak dan tidak simetris dengan aturan kebalikannya |
| Urutan dengan `path_not_permitted` | cek izin dari resolusi token tetap lebih dulu, jadi folder yang tidak diizinkan memberi satu `path_not_permitted`, folder yang diizinkan memberi satu `token_not_allowed_here` | itulah urutan kode hari ini (`resolve` lalu cek posisi), jadi setiap destination tetap menghasilkan tepat satu issue | |
| Revision paket fake | naik ke `0.3.0` | isi dan perilaku paket berubah (tempat config dan argumen `--config`); seed ke root yang sudah memegang `testing/fake 0.2.0` dengan digest lain gagal `FixtureError::Conflict`, sedangkan `0.3.0` dipasang di folder baru; preseden: spec 0014 langkah 5 menaikkan ke `0.2.0` untuk `initDelayMs` | tetap `0.2.0` karena `ProviderRevisionKey` sudah memuat digest: lebih sedikit file test yang diubah, tetapi satu label untuk dua perilaku dan konflik seed |
| Revision fixture `fake-command` | tetap `0.1.0` | paket ini hanya dibaca test core dan seed fuzz, tidak pernah di pin ke root atau di seed, jadi tidak ada salinan lama yang bisa bertabrakan | naik ke `0.2.0` demi konsistensi, tanpa manfaat nyata |
| Paket `official/nats` dan `official/traefik` | tidak berubah | dicek di file paketnya: tidak ada config `sensitive`; NATS dan Traefik hanya menulis ke `${instance.config}` | |
| Pengecualian `fake.json` di spec 0014 AC-18 | dihapus, dan pindaian mencakup seluruh folder instance di root data | keputusan user; folder data juga dikecualikan dengan alasan yang sama (template menulis token ke config), dan isinya (`argv-*.json`, `token.sha256`, `FAKE_VERSION`) tidak pernah memuat nilai | hanya menambah folder `config/`: lebih sempit, tetapi membiarkan folder data tanpa bukti padahal tidak ada alasan lagi untuk mengecualikannya |

**Gap yang ditemukan saat update dan rekomendasi yang diambil** (brief: ambil rekomendasi dan catat di sini):

- **Config sensitif hasil init tertinggal di folder secrets.** `InitPlan.cleanup` hari ini hanya memuat file `MaterializeSecret` (`plan.rs`), dan executor init spec 0010 hanya menghapus isi daftar itu. Setelah paket fake pindah ke folder secrets, `secrets/fake.json` hasil `renderConfig` init akan tinggal di runtime root sampai sweep berikutnya (daemon start) atau sampai instance itu start lalu stop. Ini melanggar invariant spec 0011 bahwa nilai secret hanya ada di file `secrets/` milik instance yang sedang berjalan atau sedang menjalankan satu langkah. Rekomendasi yang diambil: `InitPlan.cleanup` juga memuat komponen pertama setiap `WriteFile` init di folder secrets (file nya, atau folder perantara untuk destination dua komponen). Ini perubahan data plan murni di spec ini; executor spec 0010 tidak berubah karena ia sudah menghapus setiap path `cleanup` lewat `remove_owned_tree`, berhasil atau gagal. Runner up: executor init memanggil `remove_secret_material` di akhir aksi. Itu juga menutup celah, tetapi mengubah keputusan spec 0010, menghapus lebih dari yang ditulis aksi itu, dan tidak terlihat di golden plan.
- **Test yang bergantung pada path lama.** `supervisor_start/init.rs` memeriksa config ada setelah init, `common.rs::config_file` menunjuk folder config, `start.rs` menambal fake ke folder secrets dengan `replace` yang wajib menemukan teks lama, dan beberapa tambalan `provider_rules.rs` mencari teks destination lama. Semua dicatat di langkah 21 supaya build tidak menebak.
- **Spec lain yang menyebut revision `0.2.0`.** Teks spec 0014 (langkah 5, *Consequences*, dan langkah verify yang sudah dicentang) mencatat sejarah dan tidak diubah, kecuali satu catatan di langkah verify AC-18 bahwa run ulang memakai `0.3.0` setelah milestone 4, dan satu catatan di baris desain `providers/fake/provider.yaml`. AC-4 spec 0014 tidak diubah, karena teksnya mencatat revision yang membawa `initDelayMs`. Spec 0020 hanya menyebut nama fixture `create-form-fake.json`, bukan path config, jadi tidak terdampak.

Cross check: Sonnet 5.5 membaca update ini secara read only terhadap kode (`interpolate.rs`, `plan.rs`, `fuzzing.rs`, `material.rs`, `supervisor/init.rs`, paket, dan test), tanpa menjalankan apa pun. Hasil yang dibenarkan: urutan `path_not_permitted` lalu cek posisi, cabang penolakan baru yang muat di `match` yang ada tanpa `IssueCode` baru, `destination_dirs` yang sudah membuat folder secrets untuk init, run, dan reload, `remove_cleanup` yang menghapus lewat `remove_owned_tree` apa pun hasil aksi, konflik seed yang hanya muncul di folder revision yang sama, input regresi bom alias yang tetap ditolak, dan paket NATS serta Traefik tanpa config sensitif. Perbaikan yang diterapkan (rekomendasi diambil sesuai brief):

- "Tepat satu issue" hanya benar per lokasi: config yang destination nya ditolak tidak masuk model, jadi `renderConfig` dan `reload.render` yang merujuknya menambah `unknown_config_file` turunan. AC-20, butir matriks, dan *Critical test scenarios* kini menyebut ini.
- Langkah 21 kini menyebut tambalan test yang akan patah (`provider_rules.rs` dekat baris 473 dan 1946, `reload_uses` di `every_plan_fails_for_a_folder_it_uses_that_is_not_utf8`, assert `init.cleanup` `fake-command` di `provider_packages.rs`) dan run ulang fuzz `provider_interpolation` 10 menit untuk harness yang berubah.
- Langkah 20 kini menyebut record manual `evidence/manual/2026-10-20-walking-skeleton-through-the-cli-and-gui.json` (tiga `--revision 0.2.0`, `NOT_RUN`), peringatan agar `0.2.0` tidak diganti buta, seed fuzz yang dibangun ulang seluruhnya, dan bagian paket fake yang tetap.
- Invariant harness dinyatakan lewat `PathBase`. Destination tidak sensitif `${instance.run}/secrets/<x>` masih diterima; itu sudah ada sebelum update ini dan tidak membocorkan secret, jadi dicatat di Follow-up, tidak diubah.
- "Ketiga paket" di AC-20 diperjelas menjadi fake, NATS, dan Traefik, dengan fixture `fake-command` disebut terpisah.

Sisa risiko yang dinyatakan: kalau future init dibatalkan paksa (daemon mati di tengah init), `cleanup` tidak berjalan, tetapi sweep rekonsiliasi berikutnya menghapus folder secrets instance itu karena `initializing` dan `failed` tidak dihitung berjalan. Saran untuk spec 0014 (memindai atau menegaskan folder `secrets/` kosong setelah stop) melewati teks AC-18 yang disetujui user, jadi dicatat di rationale spec 0014 dan diteruskan ke lead. User menyetujuinya 2026-10-21: AC-18 kini menegaskan folder itu tidak ada setelah stop, dan langkah 21 membangun cek nya.

## Update 2026-10-22 (port endpoint saat initialize, usulan spec 0023)

Sumber nya *Usulan update spec lain (2026-10-22)* butir 1 di spec 0023 (commit `73322b32`, merge `d94c4c87`), dibawa lead scope 02 lewat brief `arc-f15-update-4`. Temuan nya: create penuh spec 0020 menjalankan initialize tanpa port untuk endpoint `Auto` (`execute_init` lewat `Prepared::context`), sehingga paket yang init nya merender port (NATS `1.0.0` dengan `nats.conf`) lolos preflight lalu gagal `supervisor.config_invalid` di langkah `init`. Spec 0023 memperbaiki NATS di paket (revision `1.0.1`) dan mengusulkan penjaga generik di spec ini supaya Traefik, Valkey, MySQL, dan ClickHouse tidak jatuh ke lubang yang sama.

**Keputusan P1: di mana aturan nya hidup.**

- **Option 1: aturan validasi di `from_files`**, seperti teks usulan (token ditolak `token_not_allowed_here` saat validasi semantik). Kelebihan: satu tempat, setiap pemuat paket kena. Kekurangan: start (spec 0010 `prepare`) membaca ulang salinan pinned di `<data>/providers/` lewat `from_files` dengan `NameCheck::Skip`, jadi setiap instance yang dibuat dari NATS `1.0.0` dengan port `Fixed` tidak bisa start lagi (`manifest.invalid`), dan duplicate spec 0024 dari instance itu ikut gagal sebelum preflight. Itu melanggar janji di *Perluasan additive fase 02* bahwa paket yang sudah valid tetap valid.
- **Option 2: laporan terpisah `init_port_issues` (dipilih).** Fungsi murni di `ValidatedPackage`, dipanggil `cargo xtask provider check` dan test core atas setiap folder `providers/`, dengan kode dan format issue yang sama dengan validasi. Kelebihan: penulis paket di repo melihat masalah nya di tempat yang sama dengan masalah validasi, salinan pinned lama tetap termuat, dan penjaga runtime untuk create ada di dry run spec 0020. Kekurangan: paket yang tidak pernah lewat `provider check` (import fitur 37) tidak ditolak saat dimuat; dicatat di Consequences dan Follow-up fitur 37.
- **Option 3: hanya dry run spec 0020**, tanpa apa pun di spec ini. Kekurangan: masalah baru ketahuan saat pengguna membuat instance, bukan saat paket ditulis, dan Traefik yang sudah salah tetap salah.

**Keputusan P2: field `managed` yang butuh port.** Usulan spec 0023 hanya menyebut token endpoint langsung. Field `managed` boleh memakai `endpoint.*`, dan `fake-command` memakai `listenPort: ${endpoint.http.port}` lalu merender nya di `peers` saat init, jadi pemakaian tidak langsung harus ikut dilaporkan. Lebih jauh, `Resolver::new` menghitung semua field `managed` untuk setiap plan, sehingga satu field yang butuh port membuat setiap `init_plan` tanpa port gagal walau init tidak memakai nya. Dipilih: laporan mencakup `settings.<f>` untuk field `managed` yang butuh port, dan `Resolver::new` menyimpan hasil per field lalu gagal hanya saat field itu dirender. Runner up: melarang token port di ekspresi `managed`. Kekurangan nya: mematahkan aturan "integer managed adalah tepat satu token `${endpoint.<name>.port}`" yang sengaja ada, dan fixture `fake-command`.

**Keputusan P3: Traefik di perubahan yang sama atau di fitur 41.** Dipilih: perubahan yang sama (langkah 22). Kalau Traefik menunggu fitur 41, `cargo xtask provider check` tanpa argumen keluar 1 sejak langkah 22, dan test core atas setiap folder `providers/` gagal, jadi langkah 22 tidak bisa hijau. Perubahan nya kecil (satu langkah `renderConfig` dihapus; render sebelum spawn sudah menulis `staticConfig`), dan paket itu belum terbit, jadi revision tetap `1.0.0` tanpa ledger catalog. Runner up: fitur 41 dengan pengecualian sementara di test. Kekurangan nya: pengecualian yang mudah terlupa dan paket contoh yang tetap salah.

**Keputusan kecil.**

- **Siapa membangun langkah 22.** Fitur 9 sudah ditutup. Mengikuti pola *Perluasan additive fase 02* (perluasan dibangun fitur yang membutuhkannya), langkah 22 dikerjakan di milestone "Init port guard" fitur 20 bersama dry run init spec 0020 (langkah 17), karena kegagalan nya muncul di create. Runner up: fitur 23 milestone 3 (sesi yang sama dengan perubahan paket NATS). Kekurangan nya: perubahan core generik masuk ke ledger RISK-07 NATS dan mengaburkan ukuran nya.
- **Urutan.** Langkah 22 sesudah fitur 23 langkah 13, karena NATS `1.0.0` di `main` punya issue.
- **`cwd`.** Usulan menyebut `cwd`; matriks posisi sudah hanya menerima token folder di sana, jadi tidak ada yang perlu diperiksa.
- **`destination`.** Sama seperti usulan: hanya token folder.
- **Kode issue dan wire.** `token_not_allowed_here` yang sudah ada, tanpa `reason`; tidak ada kode, kontrak, atau bump `ProtocolVersion` baru.

**Teks AC yang berubah (menunggu persetujuan kamu lewat lead).** AC-21 baru. AC-1 sampai AC-20 tidak berubah; AC-14 tetap benar sesudah langkah 22 (keempat paket lolos `provider check`). Teks lengkap di catatan bersama `arc-f15-update-4-ac-and-scope.md`.

**Cross check update ini (2026-10-22, model lain: Sonnet 5.5, baca saja).** Temuan untuk spec ini, semuanya diisi dengan rekomendasi saya (aturan delegasi user di brief; tidak satu pun mengubah keputusan P1 sampai P3):

| Temuan | Yang diterapkan |
| --- | --- |
| bug: `Resolver::value` untuk `endpoint.<n>.host` lewat `Resolver::endpoint`, yang menolak endpoint tanpa port, jadi host di init tetap gagal `EndpointUnresolved` dan janji AC-21 patah | perubahan kedua di *Plan builder* (host dibaca tanpa port), kalimat AC-21, baris *Value sourcing*, dan test `init_plan` dengan `${endpoint.http.host}` di init. Runner up: melaporkan host sebagai token yang butuh port, yang melarang hal yang sebenarnya aman |
| test Traefik `traefik_plans_with_three_routes_match_the_golden_files` meng assert dua render dari `init.steps` | langkah 22 mengambil `render-staticConfig.txt` dari `run.render` |
| test fitur 23 langkah 13 yang meng assert Traefik masih gagal "sampai fitur 41" | langkah 22 menghapus pengecualian itu; baris Follow-up fitur 41 di spec 0023 dijawab langkah 22 (diteruskan ke lead) |
| salinan NATS `1.0.0` tidak ada sesudah fitur 23 langkah 13 | test menyalin `providers/nats` lalu menyisipkan lagi `renderConfig natsConf` |
| posisi issue `peers.yaml.tmpl` | baris 2 kolom 7 (awal tag) |
| `# Examples` doctest butuh paket utuh | tidak dipakai |
| `Resolver::without_managed` (spec 0021) ada di `main` | disebut di *Plan builder*; perubahan field `managed` ada di `Resolver::new` di atas nya |
