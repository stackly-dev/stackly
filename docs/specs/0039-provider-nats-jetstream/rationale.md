# 0039. Provider NATS dan JetStream: rationale

Catatan keputusan untuk [index.md](index.md). `/develop` tidak perlu membaca file ini.

## Context

Fitur 23 (spec 0023) membuat NATS menjadi provider nyata lewat paket ditambah probe `nats-readiness`, dan sengaja meninggalkan empat hal untuk fitur 39: auth client, dua instance yang terbukti terisolasi, persistence stream setelah restart, dan bentrok port monitoring. Paket `1.0.1` yang ada di catalog berjalan tanpa auth di loopback, jadi proses lokal mana pun, termasuk milik user lain di Mac yang sama, bisa membaca dan menulis pesan. Evidence kompatibilitasnya mencatat tiga `knownLimitations` untuk hal yang belum dibuktikan itu, walau paket sudah mengaku punya capability `persistence` dan `multiInstance`.

Ada beberapa gaya yang saling tarik. PRD (REQ-036, bagian 22.2) meminta credential, identitas server unik, `store_dir` dan batas resource per instance, monitoring yang tidak terbuka ke LAN, dan readiness yang mencakup mode JetStream. Spec 0011 melarang secret di argv dan meminta penulis paket memakai mekanisme pengiriman tertinggi yang didukung tool itu. Spec 0023 melarang probe menerima `secret.*` dan meminta readiness tidak membaca Keychain setiap 10 detik. Spec 0029 membangun Effective File, `config-history`, dan deteksi drift di atas file config yang tidak sensitif, dan pratinjaunya tidak boleh membaca Keychain. Dan RISK-07 meminta setiap provider dibangun dengan perubahan core sesedikit mungkin, dihitung.

Fitur ini juga berjalan di wave yang sama dengan empat provider lain (fitur 40 sampai 43), yang masing masing menambah format config atau primitive di file core yang sama. Kontrak engine baru yang dipakai lebih dari satu provider akan didefinisikan dua kali oleh sesi paralel kalau tidak dijaga.

Kalau tidak diputuskan, NATS tetap terbuka untuk setiap proses lokal, AC-036 dan AC-010 tetap tanpa evidence, dan form tetap menampilkan batas JetStream sebagai angka byte sebelas digit.

## Options considered

### Keputusan 1: model auth

#### Option 1: Satu user tetap `stackly` dengan password secret, selalu aktif

Setiap instance punya satu user aplikasi di akun `$G` dengan password dari field secret, dan auth tidak bisa dimatikan.

**Pros**:

- Sama dengan PostgreSQL (superuser berpassword, selalu aktif), jadi satu model mental.
- Menutup residual tanpa auth tanpa mode kedua yang harus diuji.
- Client URL `nats://stackly:<password>@<alamat>` didukung semua client NATS.

**Cons**:

- Aplikasi yang memakai URL tanpa credential harus diubah.
- Nama user tidak bisa dipilih.

#### Option 2: Toggle auth, default aktif

Field boolean `authentication` mematikan blok `authorization`.

**Pros**:

- Nyaman untuk percobaan cepat.

**Cons**:

- Field secret wajib `required`, jadi secret tetap dibuat walau tidak dipakai.
- Dua mode untuk setiap test auth, readiness, dan konformitas.
- Mode mati membuka kembali residual yang fitur ini tutup.

#### Option 3: Token auth

`authorization { token }` tanpa nama user.

**Pros**:

- URL lebih pendek.

**Cons**:

- Tidak bisa digabung dengan `users` dan `no_auth_user`, jadi readiness butuh jalan lain yang membawa secret.

#### Option 4: NKey

Config hanya memuat kunci publik; seed tinggal di Keychain.

**Pros**:

- Tidak ada secret di config sama sekali, jadi `nats.conf` bisa tetap tidak sensitif.

**Cons**:

- Core harus menurunkan kunci publik dari seed (ed25519 dan encoding NKey), dan generator harus membuat seed berformat NKey.
- Tidak bisa ditulis sebagai URL; client harus diatur dengan file seed. Terlalu jauh dari kebutuhan alat lokal.

### Keputusan 2: cara password sampai ke `nats-server`

#### Option 1: Seluruh `nats.conf` sebagai config `sensitive` di folder `secrets/`

Mekanisme 5 spec 0011: file `0600` di folder runtime, dirender dari Keychain sebelum setiap spawn, dihapus setelah stop bersih.

**Pros**:

- Nol perubahan core; semua primitive sudah ada dan sudah dipakai paket fake dan contoh.
- Persis yang ditulis Follow-up spec 0023 ("credential lewat secret dan template `sensitive`").
- Tidak ikut backup, Time Machine, `config-history`, diagnostics, atau tampilan config.

**Cons**:

- NATS kehilangan Effective File, salinan histori, dan deteksi drift.
- Password teks biasa di disk selama instance berjalan.
- Bukan mekanisme tertinggi yang didukung NATS (bcrypt ada).

#### Option 2: Hash bcrypt di `nats.conf` biasa

Mekanisme 2 spec 0011: filter hash template menulis hash, dan file tetap di folder config.

**Pros**:

- Tidak ada password teks biasa di disk.
- Effective File dan drift tetap ada.

**Cons**:

- Bentuk hash di manifest belum ada. Spec 0009 mencadangkannya sebagai filter di config `sensitive`, yang tidak mengembalikan Effective File. Hash di config biasa bertabrakan dengan pratinjau spec 0029, yang merender tanpa Keychain.
- Butuh dependency bcrypt di core dan kontrak engine baru yang juga akan didefinisikan fitur 41 atau 43 di wave yang sama.
- Setiap koneksi client membayar sekitar 75 ms (fakta 7).

#### Option 3: Variabel environment (`password: $VAR`)

Mekanisme 4.

**Pros**:

- Tanpa perubahan core, dan `nats.conf` tetap tidak sensitif.

**Cons**:

- NATS mem parse nilai env sebagai nilai config (fakta 5): `12345678`, `true`, dan `1k` membuat server gagal start, dan `x{y}` mengubah password diam diam. Field secret tidak bisa memakai `pattern`, jadi nilai yang diketik pengguna tidak bisa dijaga.

#### Option 4: File include terpisah

Hanya blok `authorization` yang sensitif.

**Pros**:

- Effective File untuk bagian yang tidak rahasia.

**Cons**:

- `include` NATS selalu relatif terhadap folder file yang meng include (fakta 6), dan letak relatif folder config terhadap folder runtime tidak bisa ditulis dengan token.

### Keputusan 3: readiness saat auth aktif

#### Option 1: User `stackly-readiness` tanpa password di akun NATS sendiri, sebagai `no_auth_user`, izin minimum

User aplikasi di akun `APP`, user readiness di akun `STACKLY_READINESS`, keduanya dengan JetStream `enabled` saat JetStream aktif.

**Pros**:

- Probe tidak berubah sama sekali (fakta 1 sampai 3 dan 14), dan aturan "probe tidak pernah menerima `secret.*`" tetap utuh.
- Tidak ada bacaan Keychain selain saat start.
- Langkah `$JS.API.INFO` tetap berjalan, jadi readiness tetap memeriksa API JetStream.
- Akun NATS memisahkan ruang subject, jadi koneksi tanpa credential tidak bisa menyuntik apa pun ke akun aplikasi dan tidak melihat ringkasannya (fakta 12).

**Cons**:

- Koneksi tanpa credential tetap diterima, dan client yang lupa credential baru ditolak saat publish.
- User aplikasi tidak lagi di akun bawaan `$G`.
- `$JS.API.INFO` yang diperiksa probe adalah milik akun readiness, bukan akun aplikasi.

#### Option 2: Sama, tetapi user readiness satu akun dengan user aplikasi

Blok `authorization.users` biasa.

**Pros**:

- Config paling pendek, dan user aplikasi tetap di `$G`.

**Cons**:

- Terbukti tidak aman (fakta 12): koneksi tanpa credential bisa membuat server menulis balasan `$JS.API.INFO` ke subject aplikasi mana pun, sehingga stream aplikasi menyimpan pesan sampah, dan ia bisa membaca jumlah stream serta pemakaian akun aplikasi.

#### Option 3: Probe mengirim credential

Param probe baru untuk user dan password, baik password rahasia maupun password tetap yang bukan rahasia.

**Pros**:

- Tanpa `no_auth_user`; client tanpa credential ditolak saat connect.

**Cons**:

- Password rahasia harus masuk plan probe atau dibaca dari Keychain tiap percobaan, melanggar dua aturan spec 0023 dan syarat 4 probe native spec 0036.
- Password tetap yang diketahui semua orang tidak lebih aman dari tanpa password.
- Keduanya perubahan core khusus NATS di state machine yang sudah di fuzz.

#### Option 4: Readiness hanya dari INFO

Tanpa CONNECT saat auth aktif.

**Pros**:

- Tidak butuh user readiness.

**Cons**:

- Kehilangan `PONG` dan API JetStream, jadi readiness lebih lemah dari fitur 23. Tetap perubahan core.

### Keputusan 4: nilai yang tidak bisa ditulis escaper `natsConf`

#### Option 1: Longgarkan escaper (`\"`, `\\`, `$` literal)

**Pros**:

- Password apa pun yang bisa dicetak bekerja; tidak ada start yang gagal karena satu karakter.
- Terbukti pada binary asli (fakta 4), dan rationale spec 0009 sudah mengizinkan pelonggaran dengan test.
- Additive: render paket yang ada tidak berubah.

**Cons**:

- Satu perubahan core, di file yang juga disentuh tiga provider lain di wave ini.

#### Option 2: Biarkan, catat sebagai batas

**Pros**:

- Nol perubahan core.

**Cons**:

- Password yang diketik pengguna dengan `"`, `\`, atau `$` diterima saat create lalu menggagalkan start pertama dengan `unrepresentable_value`; field secret tidak punya `pattern` untuk mencegahnya lebih awal.

#### Option 3: Batasi password ke huruf dan angka

**Pros**:

- Tanpa perubahan escaper.

**Cons**:

- Butuh keyword schema baru untuk field secret, yang juga perubahan core dan lebih besar.

### Keputusan 5: satuan batas resource

#### Option 1: Field MiB dan KiB dengan suffix di template

**Pros**:

- Form terbaca ("1024" MiB, bukan 1073741824).
- Hanya paket; suffix terbukti (fakta 8).

**Cons**:

- Nama field berubah, jadi fixture dan beberapa test ditulis ulang.
- Batas tidak lagi bisa diatur per byte.

#### Option 2: Tetap byte

**Pros**:

- Tanpa churn fixture untuk nama field.

**Cons**:

- Form tetap sulit dibaca, dan Follow-up spec 0023 tetap terbuka. Fixture form tetap berubah karena field secret baru.

#### Option 3: Keyword satuan tampilan di schema settings

**Pros**:

- Nilai tetap byte, tampilan ramah.

**Cons**:

- Perubahan core, protokol, dan Swift untuk satu provider.

### Keputusan 6: nomor revision

#### Option 1: `1.1.0`

**Pros**:

- Tetap di line `1`, jadi entri `HANDOVER_PROVEN` yang ada berlaku; kontrak proses (foreground, sinyal, fd lock) memang tidak berubah.
- Tidak ada konsumen yang rusak: instance mem pin revision dan digest persis, dan tidak ada alur pindah revision.

**Cons**:

- Menurut semver, nama settings yang berubah adalah perubahan merusak.
- Bukti handover dibuat pada `1.0.1`; perlu dijalankan ulang supaya jujur (AC-13).

#### Option 2: `2.0.0`

**Pros**:

- Semver murni.

**Cons**:

- Butuh entri `HANDOVER_PROVEN` baru (perubahan di file core bersama) sebelum instance baru bertahan melewati restart daemon terencana, tanpa manfaat bagi siapa pun.

### Keputusan 7: bentrok port monitoring dan remedy

#### Option 1: Jalur spec 0033 yang ada, dibuktikan test native NATS, tanpa `remedies`

**Pros**:

- Spec 0033 AC-4 sudah melaporkan `supervisor.endpoint_in_use` dengan nama endpoint dan `suggestedPort`, untuk tahap `endpoints` dan untuk exit dini.
- Tidak ada kode atau teks baru yang harus dirawat.

**Cons**:

- Jalur exit dini tidak diuji dengan NATS asli, karena balapannya tidak bisa dibuat pasti.

#### Option 2: Tambah `remedies` NATS

Misalnya untuk baris log `can't listen to the monitor port`, atau untuk `identity_mismatch` setelah rename.

**Pros**:

- Menjawab Follow-up spec 0034 dengan isi.

**Cons**:

- Kegagalan port sudah punya banner dan `portOwner` doctor; remedy hanya mengulang.
- Remedy rename menyentuh Follow-up spec 0023 milik fitur 29 dan 30 yang belum diputuskan, dan butuh test native rename ditambah restart daemon.

### Keputusan 8: test BR-025

#### Option 1: Tetap `nats_compatibility`, dengan check baru

**Pros**:

- Check yang ada (readiness jujur, identitas, adopsi) lebih dalam dari harness, dan bukti baru masuk ke record yang sama.

**Cons**:

- Satu test berurutan yang makin panjang.

#### Option 2: Test konformitas menjadi test BR-025

**Pros**:

- Satu jalur untuk semua provider.

**Cons**:

- Harness tidak punya langkah auth, isolasi data, atau bentrok port, jadi `checks` akan berkurang.

## Rationale

**Auth dan readiness (Keputusan 1 dan 3)** dipilih bersama. `no_auth_user` adalah satu satunya cara yang membuat probe fitur 23 lulus tanpa satu baris perubahan dan tanpa secret, dan ia hanya bisa dipasang bersama user bernama. Itu menyingkirkan token auth. Fakta 1 sampai 3 membuktikannya pada binary asli sebelum spec ini ditulis. Draf pertama menaruh user readiness di akun yang sama dengan aplikasi; cross check bertanya apakah balasan API JetStream bisa diarahkan ke subject aplikasi, dan uji pada binary membuktikan bisa (fakta 12). Akun terpisah menutup lubang itu dengan harga beberapa baris template, tanpa perubahan core, dan sekaligus menghapus bocoran ringkasan akun. Harga yang tersisa jelas dan kecil: koneksi tanpa credential diterima di akun yang kosong. Auth selalu aktif karena field secret selalu wajib, dan karena mode mati hanya menambah cabang test sambil membuka lagi lubang yang ditutup.

**Pengiriman password (Keputusan 2)** adalah pertukaran terbesar. Aturan spec 0011 meminta mekanisme tertinggi, dan NATS mendukung hash. Tetapi bentuk hash di manifest belum ada, spec 0009 mencadangkannya hanya untuk config `sensitive`, dan hash di config biasa membutuhkan pratinjau spec 0029 yang tidak membaca Keychain untuk menampilkan placeholder. Itu pekerjaan engine lintas tiga spec, yang juga akan disentuh fitur 41 atau 43 di wave paralel ini. Mekanisme 5 untuk seluruh file adalah jalur yang spec 0011 sebut sendiri untuk tool tanpa include yang bisa dipakai, dan yang Follow-up spec 0023 tulis. Env dan include gugur karena fakta 5 dan 6, bukan karena selera. Kehilangan Effective File untuk NATS diterima dan dicatat, dengan Follow-up untuk meninjau ulang begitu filter hash ada.

**Escaper (Keputusan 4)** adalah satu satunya perubahan core, dan dipilih karena alternatifnya adalah start yang gagal untuk password yang sah. Field secret tidak bisa dibatasi dengan `pattern`, jadi tanpa pelonggaran, kegagalan baru terlihat setelah create berhasil. Perubahannya satu arm `match`, additive, dan terbukti pada binary.

**Satuan dan revision (Keputusan 5 dan 6)**: revision baru memang menulis ulang fixture form karena field secret, jadi mengganti nama tiga field sekarang hampir gratis, sedangkan nanti berarti revision lagi. `1.1.0` dipilih karena line hanya dipakai untuk bukti handover, kontrak proses tidak berubah, dan tidak ada alur yang memindahkan instance antar revision.

**Bentrok port dan BR-025 (Keputusan 7 dan 8)**: keduanya memakai yang sudah ada. Fitur ini menambah bukti, bukan mekanisme.

## Keputusan kecil

- **Nama user `stackly`**: tetap, karena `connection.template` menolak `settings.*`. Runner up `nats` dan `app`; `stackly` dipilih supaya jelas siapa yang membuatnya.
- **Nama akun `APP` dan `STACKLY_READINESS`**: literal di template. `APP` pendek dan netral; nama akun readiness memuat `STACKLY` supaya jelas asalnya di halaman monitoring.
- **`jetstream: enabled` tanpa batas akun**: batas yang dilihat pengguna tetap dua batas server; batas per akun hanya menambah angka kedua yang harus dijelaskan.
- **Izin readiness mengikuti mode JetStream**: saat JetStream mati, user readiness tidak boleh publish atau subscribe apa pun (`deny ">"`), karena probe hanya butuh `PING`. Runner up: izin yang sama di kedua mode, lebih sederhana tetapi memberi izin yang tidak dipakai.
- **User readiness tanpa field `password`**, bukan password tetap: tidak ada nilai yang terlihat seperti rahasia di config (fakta 2).
- **`apply: restart` untuk `clientPassword`**, bukan `immutableAfterInit`: password hidup di config yang dirender setiap start, bukan di dalam data, jadi `generateAllowed` selalu benar dan mengganti password cukup dengan restart.
- **`minLength` 8**: sama dengan PostgreSQL, dan sama dengan batas bawah pencocokan redaction log spec 0011.
- **Batas atas MiB 1048576 (1 TiB) dan KiB 65536 (64 MiB)**: sama dengan batas byte lama. Batas bawah turun dari 1 MiB dan 1 KiB yang sama.
- **Skenario `no-jetstream`** di `tests/scenarios.yaml`: mode kedua yang didukung paket, sesuai aturan "tulis skenario untuk mode yang didukung" spec 0036.
- **Client uji buatan sendiri**, bukan crate `async-nats` sebagai dev dependency: belasan baris protokol sudah cukup, dan dependency baru berarti pemeriksaan `cargo deny` dan `fuzz/Cargo.lock`. Runner up `async-nats`.
- **`power_loss_not_covered`** sebagai `knownLimitations`: daftar itu mencatat apa yang belum dibuktikan evidence, dan mati listrik memang tidak diuji.
- **Client uji memakai inbox `_INBOX.test.`**, bukan `_INBOX.stackly.`: supaya test tidak pernah bergantung pada ruang inbox probe.
- **Password berbentuk hash bcrypt dinyatakan sebagai batas**, bukan dijaga: field secret tidak punya `pattern`, generator Stackly tidak pernah membuat `$`, dan penjaga di core berarti perubahan core khusus NATS untuk kasus yang hanya terjadi kalau pengguna mengetik hash dengan sengaja.
- **AC-8 memakai create dengan port bebas lalu merebutnya**: create menolak port `Fixed` yang sedang dipakai, jadi bentrok saat start hanya bisa dibuat sesudah create (pola yang sama dengan `apps/daemon/tests/postgresql/port_in_use.rs`).
- **Freeze corpus di commit paket**, bukan di commit evidence: test `xtask` menjalankan `provider freeze providers/nats` terhadap corpus repo dan `provider check` menolak paket yang belum beku, jadi setiap commit harus membawa entrinya. `manifest_sha256` tidak memuat `versions.json`, jadi record evidence yang datang kemudian tidak mengubah entri.
- **Tidak ada langkah spike terpisah di Build plan**: setiap fakta yang dipakai desain diuji ulang oleh test native AC nya, jadi fixture transcript baru tidak dibutuhkan.
- **Fuzz 10 menit untuk `provider_template`** setelah escaper berubah: syarat 6 perubahan additive spec 0036.

## Pertukaran dengan sesi lain

Tidak ada pesan ke sesi `arc-f40-spec` sampai `arc-f43-spec`. Spec ini tidak menambah kontrak core yang mereka pakai: tidak ada format template, jenis probe, primitive secret, atau filter hash baru. Yang dibagi hanya file (lihat *File bersama yang disentuh* dan paragraf *Sesi paralel* di index), dan urutannya diatur lead.

## Update 2026-11-12: selaras dengan hasil verify

Verify milestone 1 sampai 3 (`chk-f39-m1`) lulus dengan AC-1 sampai AC-14 terpenuhi. Update ini hanya menyamakan teks spec dengan yang dibangun; tidak ada perilaku, paket, template, kode, ID AC, atau keputusan yang berubah.

| No | Temuan verify | Yang diubah di spec | Alasan |
| --- | --- | --- | --- |
| 1 | `nats_refuses_reload…` (`lifecycle_ops.rs`) dan `nats_reports_one_process…` (`metrics.rs`) tidak membawa `REQ-036` di `covers`, sedangkan AC-12 menulis "setiap test native fitur ini". | AC-12 dan tabel *Test native*: "fitur ini" dibaca sebagai test yang ditulis fitur ini atau yang membuktikan AC nya. Dua test itu tetap dengan `covers` lamanya. | Lead mengambil bacaan itu saat verify. `covers` adalah klaim "test ini membuktikan ID itu"; dua test itu membuktikan reload dan restart (spec 0026) dan metrics (spec 0028), dan fitur ini hanya memindahkan path `nats.conf` di harness mereka. Menambah `REQ-036` di sana akan membuat matrix traceability mengklaim bukti yang tidak ada. Runner up: tambah `REQ-036` ke keduanya supaya kalimat lama benar secara harfiah. |
| 2 | Satu doc comment di luar modul test `secret/delivery/mod.rs` berubah di commit `13347faf`. | Catatan di *Ukuran RISK-07* dan baris *File bersama yang disentuh*. | Contoh lama di doc itu ("a NATS config and a value with `"` or `$`") menjadi salah begitu escaper dilonggarkan, jadi membetulkannya benar. Baris barunya tidak memuat `nats`, jadi AC-14 tidak dilanggar; yang dibetulkan hanya kalimat spec "hanya modul `#[cfg(test)]` nya". |
| 3 | `nats-server` menulis `[WRN] Plaintext passwords detected, use nkeys or bcrypt` di setiap start, dan baris itu tampil di log instance. README belum menyebutnya. | Butir residual di *Security model*, satu butir di Consequences, Follow-up README, dan Follow-up bcrypt yang sudah ada kini menyebutnya. | Peringatan itu akibat Keputusan 2 yang sudah diterima (mekanisme 5, teks biasa di `secrets/nats.conf`), bukan cacat. README tidak ikut `manifest_sha256`, jadi kalimatnya bisa ditambah tanpa revision paket; itu tetap perubahan paket, jadi ditulis sebagai Follow-up, bukan dikerjakan di sini. |
| 4 | Perintah RISK-07 di `verify.md` (`git diff --numstat 90dc30dc HEAD -- …/template.rs` → `75 13`) tidak lagi memberi angka itu, karena fitur 40, 42, dan 43 mengubah file yang sama. | AC-14 dan langkah 9: angka diukur per commit build. Follow-up untuk pemilik `verify.md`. | Ukuran RISK-07 adalah ukuran fitur ini, jadi rentangnya harus daftar commit build, bukan sampai `HEAD` yang terus bergerak. `verify.md` bukan milik architect, jadi perbaikannya Follow-up. |

Temuan verify yang sudah ditutup tester dan tidak butuh perubahan spec: assertion AC-6 sesudah restart, regresi suntik balasan, dan tag `covers` AC-1 sampai AC-13 (`tst-f39-test`). AC-14 tetap tanpa test, karena ia ukuran atas riwayat commit.

Update ini tanpa cross check (jawaban tetap panel dari lead untuk update penutup wave).

## Cross check

Cross check dijalankan 2026-11-12 oleh model lain (Opus), read only, terhadap spec, spec yang dirujuk, dan kode. Vonisnya: perlu revisi sebelum `/develop`. Semua temuan diterapkan dengan rekomendasi architect (user mendelegasikan setiap keputusan ke opsi yang direkomendasikan), dan dua di antaranya diuji dulu pada binary asli.

| No | Temuan | Penyelesaian |
| --- | --- | --- |
| 1 | AC-8 mustahil seperti tertulis: create menolak port `Fixed` yang sedang dipakai, dan penolakan di stage `endpoints` tidak mengisi `lastFailure`. | AC-8 ditulis ulang: create dengan port bebas, rebut, start; `lastFailure` kosong dan instance tetap `stopped`. |
| 2 | Urutan commit mematahkan test `xtask` (`provider freeze` terhadap corpus repo, `ok official/nats 1.0.1`) dan `provider check` (`not frozen`). | Freeze pindah ke commit paket (langkah 2); `xtask/tests/provider_check.rs` masuk daftar file. |
| 3 | Menggabung langkah 1 dan 2 ke `main` lebih dulu menerbitkan revision tanpa evidence: status turun ke `experimental`, golden berubah dua kali, dan perbaikan paket sesudahnya butuh revision baru. | Hanya langkah 1 yang boleh masuk lebih dulu; langkah 2 sampai 8 masuk bersama; langkah 8 menulis ulang golden status dukungan. |
| 4 | User readiness tanpa password: tidak ada bukti koneksi tanpa credential tidak bisa membuat JetStream menulis balasan ke subject aplikasi; client uji memakai inbox probe. | Diuji: lubangnya nyata pada satu akun (fakta 12). Desain pindah ke akun `STACKLY_READINESS` terpisah (Keputusan 3 Option 1), AC-3 menambah percobaan suntik, client uji memakai `_INBOX.test.`. |
| 5 | Daftar file melewatkan test yang akan patah (`secret/delivery/mod.rs`, `template.rs`, `xtask/tests/provider_check.rs`, `config_evaluate.rs`, `connection_render.rs` dan golden nya, test bersama di `provider_packages.rs`). | Masuk tabel *File bersama yang disentuh* dan langkah 1 dan 2. |
| 6 | AC-5 tidak bisa diperiksa: koneksi probe langsung tutup, jadi `/connz` tidak menampilkannya. | AC-5 memakai kenaikan `total_connections` di `/varz` bersama jumlah `read` secret yang tetap. |
| 7 | Password fixture ambigu (`\r`). | Ditulis sebagai literal Rust dengan uraian byte nya. |
| 8 | Password berbentuk hash bcrypt gagal login diam diam setelah `$` lolos escaper. | Diuji (fakta 13); dinyatakan sebagai batas di README, *Edge cases*, dan *Security model*. |
| 9 | Duplikat instance `1.0.1` menghasilkan server tanpa auth lagi. | README dan *Edge cases*. |
| 10 | Respawn saat Keychain terkunci tidak dijelaskan. | *Edge cases*: `config_render_failed` dan blocker `NeedsSecret`, perilaku yang sudah ada. |
| 11 | Seed fuzz tulisan tangan akan dihapus `cargo xtask fuzz seed`. | Langkah 1 tidak lagi menambah seed; seed datang dari template paket. |
| 12 | AC-14 tidak mendefinisikan "sumber non test". | Definisi ditambahkan di AC-14. |

Pemeriksaan yang lulus tanpa perubahan: validasi template dan config `sensitive` untuk `natsConf`, `${instance.secrets}` di argumen start, field secret `apply: restart`, render sebelum setiap spawn dan penghapusan setelah exit, bentuk `masked` `nats://stackly:****@127.0.0.1:<port>`, langkah harness, file keychain di root override, pencocokan line handover, dan spike dari paket.
