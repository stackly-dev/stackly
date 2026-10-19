# 0021. Rationale: start, readiness, dan connection

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: scope fitur 21 hanya punya kotak `Build it`, seolah semua nilainya sudah diputuskan, padahal tiga hal belum punya pemilik: bentuk connection string dan tempatnya (spec 0019 dan 0023 menunggu jawaban), cara password sampai ke clipboard tanpa melanggar aturan reveal spec 0011, dan sumber nilai tab Summary yang tidak ada di `InstanceSummary` (path dan ukuran data, digest, provider revision, secret reference, dependents, autostart). Start dan readiness sendiri sudah diputuskan spec 0010 dan dipakai apa adanya; spec ini menambah lapisan baca dan tampilan di atasnya. Risiko kedua: fitur ini bergantung pada halaman Instances fitur 22 dan komponen secret fitur 11 yang belum dibangun, jadi build plan menaruh GUI di milestone terpisah yang hanya butuh fixture kontrak.

JOURNEY-01 berakhir di "copy connection": pengguna menjalankan instance, menunggu readiness, lalu menyalin connection string. PRD meminta process state, readiness, health, dan desired state dipisah, dan ready hanya dari probe provider yang segar (REQ-012, BR-010, AC-012). Ringkasan instance (PAGE-05) memuat ID, versi dan digest, ukuran dan path data, endpoint, connection string tersamar, aksi start dan stop, dan dampak dependency. Output connection URI menyembunyikan password secara default, dan reveal atau copy secret butuh aksi eksplisit (PRD bagian 7, REQ-016, AC-016). Start, stop, status, config, log, dan runtime terpasang harus bekerja tanpa internet (REQ-046, AC-046), dan scope menambah: tanpa menyentuh catalog atau cek trust online.

Fondasi yang sudah diputuskan: spec 0010 punya `instance.start`, `instance.stop`, `instance.get`, `InstanceSummary` dengan lifecycle, readiness, health, proses, restart, dan endpoint terpisah, serta event `instance.state_changed` dan `instance.health_changed`. Spec 0011 menyimpan nilai hanya di Keychain, mengeluarkannya hanya lewat `secret.reveal` yang diaudit per purpose, dan menyediakan `RevealableSecretText` (teks tersamar dari daemon ditambah Show Password dan Copy dengan closure komposisi pemanggil) serta `ConcealedPasteboard`. Spec 0009 punya grammar interpolasi dan matriks posisi token, dan menyerahkan pertanyaan `purpose` endpoint ke fitur 20 dan 21; spec 0020 memutuskan purpose dibaca dari paket pinned dan `socketPath` dihitung dari template paket pinned. Spec 0019 dan 0023 meminta bentuk connection string PostgreSQL dan NATS beserta tempatnya di manifest. Spec 0022 punya halaman Instances, inspector, `InstanceActionModel`, `ShellNavigator` dengan `openInstance` sementara, resolver CLI, dan `stackly instance inspect` yang mengirim `InstanceSummary`.

Fakta yang membatasi: GUI tidak pernah menautkan Rust, jadi apa pun yang dihitung untuk tampilan harus lewat DTO; DTO tidak boleh membawa nilai secret selain `secret.reveal`; registry tidak punya penghitung restart sejak dibuat; dan paket provider ikut `manifest_sha256`, sehingga menambah field manifest mengubah digest paket dan menuntut evidence ulang. Mockup menampilkan halaman instance dengan tab Summary (kartu Endpoints dengan Copy dan Readiness, Connection dengan Show Password dan Copy serta Secret, Dependents, dan Details), toolbar Start atau Stop, Restart, Reload, dan menu More. Pendekatan proyek adalah Journey.

## Options considered

### Keputusan 1: sumber connection string

#### Option 1: blok `connection` di manifest dengan template interpolasi dan aturan encode per entri (dipilih)

**Pros**:

- Provider baru mendapat tombol Copy dan `stackly instance connection` tanpa kode Rust atau Swift (REQ-029).
- Memakai grammar dan validasi interpolasi spec 0009 yang sudah ada, jadi token yang salah ditolak saat paket dimuat.
- Satu paket bisa punya beberapa bentuk (PostgreSQL TCP dan socket, NATS client dan monitoring).

**Cons**:

- Field manifest baru mengubah `manifest_sha256` paket PostgreSQL dan NATS, jadi urutan dengan evidence BR-025 harus diatur.
- Aturan encode URI harus didefinisikan dan dijaga di dua klien.

#### Option 2: kode per provider di daemon

**Pros**:

- Bebas bentuk, tanpa field manifest baru.

**Cons**:

- Setiap provider baru butuh patch core, bertentangan dengan REQ-029 dan ukuran RISK-07.

#### Option 3: diturunkan dari `purpose` endpoint dan skema per kategori

**Pros**:

- Tanpa template.

**Cons**:

- Tidak bisa menyatakan user, database, atau query socket; hasilnya tetap butuh pengecualian per provider.

### Keputusan 2: tempat string lengkap disusun

#### Option 1: daemon mengirim bagian tersamar, client menyusun dengan nilai dari `secret.reveal` (dipilih)

**Pros**:

- Sesuai spec 0011: nilai hanya keluar lewat `secret.reveal` yang diaudit, dan `RevealableSecretText` memang meminta closure komposisi dari pemanggil.
- `InstanceDetail` tidak pernah membawa secret, jadi bisa di cache dan dikirim bebas.

**Cons**:

- Fungsi komposisi ada di Rust dan Swift; fixture bersama menjaganya sama.

#### Option 2: method daemon yang mengembalikan string lengkap sebagai `SecretText`

**Pros**:

- Satu implementasi komposisi.

**Cons**:

- Jalur keluar nilai secret kedua di luar `secret.reveal`, yang harus diaudit dan diuji sentinel sendiri; melanggar AC-9 spec 0011.

#### Option 3: tidak ada copy lengkap, hanya string tersamar

**Pros**:

- Paling aman.

**Cons**:

- JOURNEY-01 berakhir di copy connection; pengguna harus merakit sendiri.

### Keputusan 3: sumber data tab Summary

#### Option 1: method baru `instance.get_detail` ditambah `instance.get_data_usage` terpisah (dipilih)

**Pros**:

- `InstanceSummary` yang dikirim di setiap event tetap kecil; detail hanya diminta halaman yang membutuhkannya.
- Scan folder yang mahal tidak menahan balasan detail, dan bisa di cache sendiri.

**Cons**:

- Dua method dan aturan muat ulang di client.

#### Option 2: menambah field ke `InstanceSummary`

**Pros**:

- Satu sumber data.

**Cons**:

- Setiap event membawa connection, secret reference, dan path; ukuran data tidak mungkin dihitung untuk setiap event.

#### Option 3: satu method detail yang juga menghitung ukuran data

**Pros**:

- Satu panggilan.

**Cons**:

- Balasan detail bisa tertahan sampai dua detik untuk folder besar.

### Keputusan 4: cara menghitung ukuran data

#### Option 1: penelusuran blok teralokasi lewat port `Filesystem`, dengan batas entri, tenggat, dan cache (dipilih)

**Pros**:

- Angka yang sama dengan pemakaian disk nyata (file sparse dan hard link tidak membengkakkan angka), tanpa subprocess, dan bisa diuji dengan fake.

**Cons**:

- Method port baru, dan folder sangat besar hanya mendapat "at least".

#### Option 2: menjalankan `du`

**Pros**:

- Tanpa kode penelusuran.

**Cons**:

- Spawn dari daemon untuk tampilan, output yang harus diparse, dan aturan no spawn `check-deps`.

#### Option 3: menunda ke metrics fitur 28

**Pros**:

- Tanpa kerja sekarang.

**Cons**:

- Scope fitur 21 menyebut ukuran data.

### Keputusan 5: baris Restarts

#### Option 1: jumlah di jendela restart policy dari `InstanceSummary.restart` (dipilih)

**Pros**:

- Nilai jujur yang sudah ada, tanpa migrasi; label menyebut jendelanya.

**Cons**:

- Berbeda dengan mockup yang menulis jumlah sejak dibuat.

#### Option 2: penghitung sejak dibuat di registry

**Pros**:

- Sesuai mockup.

**Cons**:

- Kolom dan migrasi baru untuk satu baris tampilan; lebih cocok di metrics fitur 28.

### Keputusan 6: entri connection yang ditampilkan

#### Option 1: semua entri provider, entri pertama sebagai utama (dipilih)

**Pros**:

- Socket PostgreSQL dan URL monitoring NATS terlihat tanpa layar lain.

**Cons**:

- Kartu Connection lebih panjang dari mockup.

#### Option 2: hanya entri utama

**Pros**:

- Sesuai mockup.

**Cons**:

- Entri lain hanya bisa dicapai lewat CLI.

### Keputusan 7: bentuk `stackly instance connection`

#### Option 1: CLI membaca `instance.get_detail` dan memilih entri sendiri, dengan kode `cli.*` (dipilih)

**Pros**:

- Satu sumber data untuk GUI dan CLI, tanpa method khusus format.

**Cons**:

- Dua kode area `cli` baru.

#### Option 2: method daemon `instance.get_connection { format }`

**Pros**:

- Error dari daemon.

**Cons**:

- Method tambahan yang hanya menyaring data yang sudah ada di detail.

### Keputusan 8: bentuk `stackly instance inspect --json`

#### Option 1: object pembungkus `{ instance, detail, dataUsage }`, diusulkan ke spec 0022 sebelum dibangun (dipilih)

**Pros**:

- Fitur 21 hanya menambah key, jadi tidak ada perubahan breaking.

**Cons**:

- Spec 0022 harus berubah sebelum build nya.

#### Option 2: perintah baru `stackly instance detail`

**Pros**:

- Tanpa menyentuh spec 0022.

**Cons**:

- Dua perintah yang menampilkan instance yang sama; paritas dengan Summary terbagi.

### Keputusan 9: bukti offline

#### Option 1: instrumentasi otomatis (catalog, `Fetcher`, `CodeSigning`) ditambah run native dengan jaringan mati setelah persetujuan (dipilih)

**Pros**:

- Test otomatis menjaga regresi setiap build; run native membuktikan perilaku nyata tanpa trik sandbox.

**Cons**:

- Langkah manual yang menunggu user.

#### Option 2: daemon di bawah `sandbox-exec` yang menolak jaringan

**Pros**:

- Otomatis dan nyata.

**Cons**:

- Profil sandbox yang membolehkan socket Unix dan loopback tetapi menolak jaringan luar rapuh, dan `sandbox-exec` sudah deprecated.

### Keputusan 10: tab dan tombol milik fitur lain

#### Option 1: tampil hanya saat fiturnya dibangun (dipilih)

**Pros**:

- Tidak ada kontrol yang tampak bekerja padahal belum (aturan yang sama dengan Settings fitur 32).

**Cons**:

- Halaman detail lebih kosong dari mockup sampai fitur 24 sampai 54.

#### Option 2: tab placeholder dan tombol nonaktif

**Pros**:

- Bentuk mockup lengkap.

**Cons**:

- Pengguna melihat aksi yang tidak bisa dipakai tanpa alasan yang berarti.

### Keputusan 11: copy endpoint

#### Option 1: pasteboard biasa saat tombol ditekan (dipilih)

**Pros**:

- Alamat bukan secret; Universal Clipboard dan riwayat clipboard boleh menyimpannya.

**Cons**:

- Tidak ada.

#### Option 2: `ConcealedPasteboard` juga untuk alamat

**Pros**:

- Satu jalur copy.

**Cons**:

- Alamat hilang dari clipboard setelah 90 detik tanpa alasan.

## Rationale

Poros spec ini adalah AC-016 dan JOURNEY-01 sekaligus: pengguna harus bisa menyalin connection string lengkap, tetapi password tidak boleh keluar tanpa aksi eksplisit dan tidak boleh lewat jalur lain selain `secret.reveal`. Daemon yang merender bagian tersamar dan client yang menyusun dengan nilai reveal (Keputusan 2) adalah satu satunya bentuk yang memenuhi keduanya tanpa menambah jalur keluar nilai, dan `RevealableSecretText` spec 0011 memang dirancang untuk itu. Connection string di manifest (Keputusan 1) menjaga REQ-029: setiap provider berikutnya cukup menulis template.

Pemisahan `instance.get_detail` dan `instance.get_data_usage` (Keputusan 3 dan 4) mengikuti biaya: `InstanceSummary` mengalir di setiap event dan harus kecil, detail hanya untuk halaman yang terbuka, dan scan folder hanya saat diminta dengan batas yang jelas. Semua sumber nilai detail adalah registry, paket pinned, dan layout, jadi tuntutan offline (REQ-046) terpenuhi dari bentuknya, dan instrumentasi plus run native (Keputusan 9) membuktikannya.

Pilihan tampilan (Keputusan 5, 6, 10, 11) mengikuti aturan yang sama dengan spec 0020 dan 0022: tidak menampilkan apa pun yang belum benar, menulis label yang jujur saat data belum ada (restart di jendela, bukan sejak dibuat), dan tidak menambah migrasi atau method hanya demi satu baris mockup.

## Kesepakatan antarsesi

Spec ini dikerjakan di sesi yang sama dengan spec 0020, jadi keputusan `socketPath`, purpose dari paket pinned, `ShellNavigator`, dan `instance.removed` dipakai langsung. Usulan untuk spec 0022 (destinasi `.instance`, inspector, bentuk JSON inspect) dan 0023 (blok `connection` NATS) dikirim lewat lead.

## Cross check

Diisi setelah cross check model lain.
