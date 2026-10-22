# 0021. Rationale: start, readiness, dan connection

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: scope fitur 21 hanya punya kotak `Build it`, seolah semua nilainya sudah diputuskan, padahal tiga hal belum punya pemilik: bentuk connection string dan tempatnya (spec 0019 dan 0023 menunggu jawaban), cara password sampai ke clipboard tanpa melanggar aturan reveal spec 0011, dan sumber nilai tab Summary yang tidak ada di `InstanceSummary` (path dan ukuran data, digest, provider revision, secret reference, dependents, autostart). Start dan readiness sendiri sudah diputuskan spec 0010 dan dipakai apa adanya; spec ini menambah lapisan baca dan tampilan di atasnya. Risiko kedua: fitur ini bergantung pada halaman Instances fitur 22 dan komponen secret fitur 11 yang belum dibangun, jadi build plan menaruh GUI di milestone terpisah yang hanya butuh fixture kontrak.

JOURNEY-01 berakhir di "copy connection": pengguna menjalankan instance, menunggu readiness, lalu menyalin connection string. PRD meminta process state, readiness, health, dan desired state dipisah, dan ready hanya dari probe provider yang segar (REQ-012, BR-010, AC-012). Ringkasan instance (PAGE-05) memuat ID, versi dan digest, ukuran dan path data, endpoint, connection string tersamar, aksi start dan stop, dan dampak dependency. Output connection URI menyembunyikan password secara default, dan reveal atau copy secret butuh aksi eksplisit (PRD bagian 7, REQ-016, AC-016). Start, stop, status, config, log, dan runtime terpasang harus bekerja tanpa internet (REQ-046, AC-046), dan scope menambah: tanpa menyentuh catalog atau cek trust online.

Fondasi yang sudah diputuskan: spec 0010 punya `instance.start`, `instance.stop`, `instance.get`, `InstanceSummary` dengan lifecycle, readiness, health, proses, restart, dan endpoint terpisah, serta event `instance.state_changed` dan `instance.health_changed`. Spec 0011 menyimpan nilai hanya di Keychain, mengeluarkannya hanya lewat `secret.reveal` yang diaudit per purpose, dan menyediakan `RevealableSecretText` (teks tersamar dari daemon ditambah Show Password dan Copy dengan closure komposisi pemanggil) serta `ConcealedPasteboard`. Spec 0009 punya grammar interpolasi dan matriks posisi token, dan menyerahkan pertanyaan `purpose` endpoint ke fitur 20 dan 21; spec 0020 memutuskan purpose dibaca dari paket pinned dan `socketPath` dihitung dari template paket pinned. Spec 0019 dan 0023 meminta bentuk connection string PostgreSQL dan NATS beserta tempatnya di manifest. Spec 0022 punya halaman Instances, inspector, `InstanceActionModel`, `ShellNavigator` dengan `openInstance` sementara, resolver CLI, dan `stackly instance inspect` yang mengirim `InstanceSummary`.

Fakta yang membatasi: GUI tidak pernah menautkan Rust, jadi apa pun yang dihitung untuk tampilan harus lewat DTO; DTO tidak boleh membawa nilai secret selain `secret.reveal`; registry tidak punya penghitung restart sejak dibuat; dan paket provider ikut `manifest_sha256`, sehingga menambah field manifest mengubah digest paket dan menuntut evidence ulang. Mockup menampilkan halaman instance dengan tab Summary (kartu Endpoints dengan Copy dan Readiness, Connection dengan Show Password dan Copy serta Secret, Dependents, dan Details), toolbar Start atau Stop, Restart, Reload, dan menu More. Pendekatan proyek adalah Journey.

## Options considered

### Keputusan 1: sumber connection string

#### Option 1: blok `connection` di manifest berisi template URI, dirender lewat `PlanContext` dan resolver plan (dipilih, disederhanakan setelah cross check)

**Pros**:

- Provider baru mendapat tombol Copy dan `stackly instance connection` tanpa kode Rust atau Swift (REQ-029).
- Memakai grammar, validasi, `PlanContext`, dan resolver interpolasi spec 0009 yang sudah ada, jadi host, port, dan socket nya sama dengan plan dan token yang salah ditolak saat paket dimuat; render sentinel menolak template yang hasilnya bukan URI.
- Satu paket bisa punya beberapa bentuk (PostgreSQL TCP dan socket, NATS client dan monitoring).

**Cons**:

- Field manifest baru mengubah `manifest_sha256` paket PostgreSQL dan NATS, jadi urutan dengan evidence BR-025 harus diatur.
- Aturan encode URI dijaga di tiga tempat (core, protokol untuk CLI, dan Swift) lewat satu fixture.
- Draft pertama punya field `encoding` dan token `settings.*`; cross check menunjukkan tidak ada paket yang memakainya, jadi keduanya dibuang dan bisa ditambah kemudian secara additive.

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

#### Option 1: penelusuran blok teralokasi lewat port `Filesystem`, hanya saat diminta, dengan batas entri, tenggat, cache, dan batas scan bersamaan (dipilih, dipersempit setelah cross check)

**Pros**:

- Angka yang sama dengan pemakaian disk nyata (file sparse dan hard link tidak membengkakkan angka), tanpa subprocess, dan bisa diuji dengan fake.

**Cons**:

- Method port baru, folder sangat besar hanya mendapat "at least", dan inspector serta `inspect` menampilkan "—" sampai ukuran pernah dihitung (draft pertama memulai scan di setiap pilihan baris inspector, yang bisa memulai puluhan scan saat pengguna menelusuri tabel).

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

#### Option 1: jumlah di jendela restart policy dari `InstanceSummary.restart`, berlabel "Restart attempts" dan tersembunyi saat 0 (dipilih)

**Pros**:

- Nilai jujur yang sudah ada, tanpa migrasi; label menyebut jendelanya. Baris disembunyikan saat 0 karena riwayat dihapus setelah 60 detik ready dan saat daemon restart, sehingga "None" bisa menyesatkan.

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

#### Option 1: test di dalam proses (`stacklyd::server::start` dengan binary CLI asli) dengan instrumentasi catalog, `Fetcher`, dan `CodeSigning`, ditambah run native dengan jaringan mati setelah persetujuan (dipilih)

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

### Keputusan 12: tampilan proses dan readiness

#### Option 1: header memakai label status spec 0012, Summary menulis baris Process dan Readiness terpisah (dipilih setelah cross check)

**Pros**:

- Konsisten dengan label status di semua layar ("Starting…" dan "Not Ready" selama probe belum berhasil), dan tetap memenuhi scope "status process dan readiness tampil terpisah" lewat dua baris yang dibacakan VoiceOver sebagai dua elemen.

**Cons**:

- Dua tempat menampilkan status (header dan dua baris).

#### Option 2: header menulis "Running" selama proses hidup (pilihan draft pertama)

**Pros**:

- Paling dekat dengan kata "process" di scope.

**Cons**:

- Bertentangan dengan tabel status spec 0012 yang sudah dibangun, sehingga label yang sama berarti dua hal di layar berbeda.

## Rationale

Poros spec ini adalah AC-016 dan JOURNEY-01 sekaligus: pengguna harus bisa menyalin connection string lengkap, tetapi password tidak boleh keluar tanpa aksi eksplisit dan tidak boleh lewat jalur lain selain `secret.reveal`. Daemon yang merender bagian tersamar dan client yang menyusun dengan nilai reveal (Keputusan 2) adalah satu satunya bentuk yang memenuhi keduanya tanpa menambah jalur keluar nilai, dan `RevealableSecretText` spec 0011 memang dirancang untuk itu. Connection string di manifest (Keputusan 1) menjaga REQ-029: setiap provider berikutnya cukup menulis template.

Pemisahan `instance.get_detail` dan `instance.get_data_usage` (Keputusan 3 dan 4) mengikuti biaya: `InstanceSummary` mengalir di setiap event dan harus kecil, detail hanya untuk halaman yang terbuka, dan scan folder hanya saat diminta dengan batas yang jelas. Semua sumber nilai detail adalah registry, paket pinned, dan layout, jadi tuntutan offline (REQ-046) terpenuhi dari bentuknya, dan instrumentasi plus run native (Keputusan 9) membuktikannya.

Pilihan tampilan (Keputusan 5, 6, 10, 11) mengikuti aturan yang sama dengan spec 0020 dan 0022: tidak menampilkan apa pun yang belum benar, menulis label yang jujur saat data belum ada (restart di jendela, bukan sejak dibuat), dan tidak menambah migrasi atau method hanya demi satu baris mockup.

## Kesepakatan antarsesi

Spec ini dikerjakan di sesi yang sama dengan spec 0020, jadi keputusan `socketPath`, purpose dari paket pinned, `ShellNavigator`, dan `instance.removed` dipakai langsung. Usulan untuk spec 0022 (destinasi `.instance`, inspector, bentuk JSON inspect) dan 0023 (blok `connection` NATS) dikirim lewat lead.

## Cross check

Dijalankan 2026-10-20 oleh model lain (Sonnet, read only) atas draft `4e9de0c`, lalu semua rekomendasi diterima lead atas nama user dengan aturan tetap "opsi yang direkomendasikan yang diambil".

| ID | Temuan | Keputusan yang diterapkan |
| --- | --- | --- |
| G1 | Header "Running" bertentangan dengan tabel status spec 0012; teks Readiness hanya tabel di spec 0022 | header memakai label spec 0012; baris Process dan Readiness terpisah dari `ProcessText` dan `ReadinessText` (Keputusan 12, AC-8) |
| G2 | Paritas teks bertentangan dengan baris CLI spec 0022; fixture tanpa summary dan usage | paritas hanya untuk baris yang ditambah spec ini; fixture triple dengan `TZ=UTC` dan jam tetap (AC-7) |
| G3 | "Dibuka setelah create" bertentangan dengan spec 0020; navigasi tanpa pemilik | dihapus dari AC-11; usulan spec 0022 untuk `NavigationStack`, `instanceRequest`, dan klik ulang sidebar |
| G4 | `RevealableSecretText` hanya untuk satu reveal; `compose` tanpa tipe dan aturan nilai hilang | `compose` dengan peta reference dan `MissingValue`; Show menampilkan string tersusun; usulan spec 0011 `references:compose:` semua atau tidak sama sekali |
| G5 | Alasan tidak tersedia bertumpuk dan tanpa urutan | urutan `retained`, `package_unavailable`, `port_not_assigned`; `socket_not_created` dibuang; `settings.*` dibuang |
| G6 | Copy dan Show tanpa aturan saat belum siap atau secret hilang | caution "Not accepting connections yet."; nonaktif dengan alasan saat secret `missing` |
| G7 | Data usage tanpa `refresh`, kode tanpa tabel, tenggat tanpa tipe, format byte kecil | `refresh` dan `cachedOnly`, tabel kode, `Duration`, `files` dibuang, aturan format byte bersama |
| G8 | Tipe `target` teks, `line` tidak dipakai, nama tampil wajib padahal paket bisa rusak | `HostTarget`, `line` dibuang, `displayName` opsional dengan cadangan software |
| G9 | Enum berdata tanpa jalur nilai asing; tiga field untuk satu state | `ConnectionPart` sebagai struct dengan `kind` terbuka; `ConnectionState` terbuka dengan test konsistensi |
| G10 | Kode issue manifest tidak disebut | `connection_invalid` dan `connection_name_duplicate` di usulan spec 0009 |
| G11 | Usulan paket dan urutan evidence tidak lengkap | usulan AC-4 spec 0023 dan baris prasyarat di build plan spec 0019 dan 0023 |
| G12 | Sumber `running`, folder run, urutan, status tanpa baris tidak disebut | lifecycle, `instance_run_dir`, urut nama dan field, `unknown`, `dataPath` dari baris path registry |
| G13 | Sentinel Swift dan CLI tanpa langkah; event dan balasan lama di client tidak jelas | sentinel di langkah 8 dan 10; muat ulang lewat revision dan `secret.changed`, balasan lama dibuang |
| S1 | "checked {age}" menampilkan umur yang salah | dihapus dari Summary dan diusulkan dihapus dari inspector spec 0022 |
| S2 | Scan ukuran data terlalu sering dan tanpa batas bersamaan | hanya saat diminta, `cachedOnly`, semaphore 2, TTL 10 detik untuk hasil tidak lengkap (Keputusan 4) |
| S3 | Encode URI dan validasi template | UTF 8 dengan hex huruf besar, fixture lewat ketiga encoder, render sentinel dan parse URI, `encoding` dan `endpoint.host` dibuang |
| S4 | `port_may_change` salah untuk `recovering` dan `unknown` | true kecuali lifecycle `running` |
| S5 | Urutan `manifest_sha256` dan evidence bergantung pada koordinasi manusia | baris prasyarat di spec 0019 dan 0023 |
| S6 | Bukti offline butuh server di dalam proses; definisi akses catalog | `stacklyd::server::start` dengan binary CLI asli; akses catalog berarti `CatalogState` daemon; GUI memakai tile abu tanpa catalog |
| S7 | Label Restarts menyesatkan | "Restart attempts", tersembunyi saat 0 (Keputusan 5) |
| S8 | Cache paket menyembunyikan perubahan folder pinned | cek stat `provider.yaml` setiap request |
| S9 | VoiceOver membaca seluruh string sebagai password | bagian bukan secret dibacakan, "password hidden" di tempat `****`, pengumuman siap ditunda 2 detik |
| S10 | Ada inti yang lebih sederhana | render di atas `PlanContext` dan resolver plan, tanpa settings (Keputusan 1) |
| S11 | Kategori dan konflik flag CLI | `cli.connection_unavailable` menjadi `conflict`; `conflicts_with` clap; `--show-password` untuk entri tanpa secret; `inspect` membandingkan revision |
| S12 | Pemicu muat ulang belum lengkap untuk config dan environment | dicatat sebagai milik fitur 29 dan 44 sampai 47 (Follow-up) |

## Perbaikan pasca penyelarasan 2026-10-20

Diterapkan atas permintaan lead scope 02 dari `docs/scope/coordination/post-alignment/post-0021.md` (tujuh temuan minor, ditinjau dua reviewer), dengan keputusan lead D1, D2, D5, dan D6 di `post-decisions.md` yang menang bila berbeda dengan teks perbaikan. ID AC, status, dan nomor langkah tidak berubah.

| No | Perubahan | Sumber |
| --- | --- | --- |
| 1 | `Issue.reason` di langkah 1, *Kode error*, baris usulan spec 0009, dan *Consequences*: field core dari spec 0017 langkah 3; kunci wire `reason`, `ItemSpec`, dan kenaikan minor ikut langkah build pertama yang mengirim issue ber reason lewat wire (langkah 5 spec 0017, selain itu langkah 1 spec ini); "detail `reason`" menjadi "reason di `Issue.reason`" | temuan 1, D2 |
| 2 | Blok `connection` NATS ditulis hanya langkah 2 spec 0023; langkah 2 dan AC-1 spec ini menulis blok PostgreSQL dan fake, dan golden NATS tetap dicek di sini | temuan 2, 5, dan 6, D1 |
| 3 | Milestone 4 butuh fitur 19 milestone 3 langkah 9 (PostgreSQL sebagai `[[provider]]`); test daemon sebelum itu boleh memakai catalog `CatalogBuilder` | temuan 3, D6 |
| 4 | Usulan baru untuk spec 0013: `ShellSync.events(kindPrefix:) -> AsyncStream<Event>` untuk pengamat tanpa store, dibangun langkah 9; disebut di *Tab Summary* dan langkah 9; spec 0013 dikeluarkan dari kalimat "Tidak ada update"; Follow-up baru untuk membawanya ke pemilik spec 0013 | temuan 4 |
| 5 | Milestone 1 tidak lagi menunggu update spec yang sudah ada di `main`; blok usulan spec 0004 dan 0011 ditandai diterapkan; Follow-up lead dicentang | temuan 7 |
| 6 | Prasyarat paket PostgreSQL langkah 2 menjadi fitur 19 langkah 4 | D5 |

Tidak diterapkan di sini: kalimat untuk spec 0019 (temuan 6, "paket langkah 4 belum membawa blok `connection` sampai langkah 2 spec 0021") milik pemilik spec 0019, dan baris scope 02 milik lead.

Tambahan lead pada putaran yang sama (2026-10-20): cek Dynamic Type di AC-16 dan langkah 10 diganti cek teks besar spec 0012 (`#Preview` lebar sempit 640 pt, karena text style macOS 27 tidak ikut `dynamicTypeSize`), seperti spec 0012, 0015, 0018, dan 0020; aturan D2 kini pasti lewat spec 0017 (issue catalog hanya sampai ke wire di `ProviderDetail.issues`), jadi kunci wire `reason`, `ItemSpec`, dan kenaikan minor milik langkah 1 spec ini tanpa syarat, dan *Consequences* menyebut dua kenaikan minor; langkah 9 memakai ulang `ReadinessText` kalau langkah 7 spec 0022 sudah membangunnya dengan aturan dan lokasi spec ini.

## Folder run sentinel (2026-10-20)

Folder run sentinel di AC-1 menjadi `/stackly sentinel/run`, bukan `/tmp/stackly sentinel/run`, karena `cargo xtask check-deps` menolak path mesin seperti `/tmp` di crate library; folder baru ada di bawah basis `/stackly` yang sama dengan render sample spec 0009 dan tetap memuat spasi, jadi nilai token yang tidak di encode tetap terdeteksi (dibangun langkah 1, commit e6c747f).

## Update 2026-10-21 dari spec 0067, 0068, dan 0069

- **Komponen.** Kartu Summary memakai `CardSection`, `.labeledContentStyle(.stackly)`, dan `CopyButton` spec 0067; toolbar memakai `ActionButton(showsReason: false)` dan `ActionMenu`, jadi alasan nonaktif aksi utama ditulis di konten halaman (spec 0067 AC-7).
- **Perintah.** Spec 0069 membuat halaman Instances menerbitkan `openInstance`, `back`, dan `copyConnectionString`, dan klik ulang sidebar lewat hook `onReselect`. Teks AC-15 kini menyebut action yang sama untuk tombol Open dan Copy di inspector, disetujui user lewat lead (2026-10-21). Teks AC-12 memakai versi koreksi yang disetujui user lewat lead (2026-10-21): usulan asli menyebut tooltip More lewat `CommandMap.helpText(for:)`, padahal More tidak punya `CommandID`, jadi More memakai help bawaan "More" dan hanya aksi utama yang memakai `helpText(for:title:)`.
- **Snapshot.** Langkah 10 menambah suite `InstanceDetailSnapshotTests` di atas harness spec 0068. Teks AC-16 tidak diubah untuk suite snapshot, atas keputusan user; ia diubah kemudian untuk kolom halaman 384 pt (butir di bawah).
- **Cross check (2026-10-21).** Cross check `Another model` (Sonnet 5.5) atas update ini dan perbaikannya diterima lead atas delegasi user (2026-10-21): tooltip aksi utama lewat `helpText(for:title:)` dan More dengan help bawaan, `fixtureNow` untuk Uptime dan "scanned", cadangan `select(_:)` dan `onReselect` di langkah 10 (D11), `key` `CopyButton`, adapter `InstanceActionModel.stacklyAction(for:)`, dan tombol Open sebagai action `openInstance` (D12).
- **Kolom halaman 384 pt (2026-10-21).** Temuan langkah 14 spec 0067: di jendela 900 pt dengan sidebar dan inspector terbuka, kolom halaman hanya 384 pt. User menerimanya, jadi halaman detail harus bekerja di lebar itu saat inspector Instances terbuka; `.labeledContentStyle(.stackly)` sudah menumpuk label dan nilai saat sempit. Teks AC-16 kini menyebut kolom halaman 384 pt saat inspector Instances terbuka, disetujui user lewat lead (2026-10-21); inspector dibatasi 280 pt di jendela di bawah 1000 pt (spec 0067).

## Update 2026-10-22: keputusan `tree_usage` dan item Duplicate

Diminta lead scope 02 (wave 4) sebelum langkah 4 dibangun. Sumbernya enam keputusan lokal sesi developer langkah 4 yang dihentikan wave 3 (log koordinasi scope 02, *Blocker terbuka*), usulan spec 0024 untuk spec ini, dan cek atas update spec 0013. Keputusan rutin diambil dengan opsi rekomendasi (aturan user); perubahan teks AC-5 menunggu persetujuan user lewat lead. ID AC, OQ, dan nomor langkah tidak berubah.

### Enam keputusan `Filesystem::tree_usage`

| No | Usulan developer | Keputusan | Alasan | Runner up |
| --- | --- | --- | --- | --- |
| 1 | `TREE_USAGE_MAX_DEPTH` 128, subtree lebih dalam dilewati dengan `complete: false` | diperbaiki: batas 32 (root tingkat 1), folder di tingkat 33 dihitung bloknya tetapi tidak dibuka, `complete: false`; alasan ketiga ini masuk AC-5 | setiap tingkat yang terbuka memegang satu file descriptor; `stacklyd` tidak menaikkan `RLIMIT_NOFILE`, dan batas lunak bawaan launchd untuk agent adalah 256, jadi dua scan bersamaan (`DATA_USAGE_CONCURRENCY`) dengan 128 tingkat bisa memakai seluruh jatah dan membuat socket, registry, dan pipe proses gagal dibuka; dengan 32, dua scan paling banyak 64 descriptor, sementara folder data provider PRD kurang dari 10 tingkat | 128 seperti `remove_tree`, yang aman untuk satu operasi hapus tetapi tidak untuk scan berulang di jalur baca; atau `Err(TooDeep)`, yang membuat seluruh ukuran "Unavailable" karena satu folder dalam |
| 2 | blok root masuk `bytes`, tidak masuk `max_entries` | disahkan | sama dengan `du`; root bukan nama yang dibaca dari isi folder, jadi aturan entri (butir 3) tidak menghitungnya | root sebagai entri pertama, yang membuat pohon dengan tepat `max_entries` entri tampak tidak lengkap |
| 3 | `max_entries` menghitung setiap entri yang di stat di device root, termasuk hard link kedua; tepat `max_entries` dianggap lengkap | diperbaiki: setiap nama yang dibaca dari isi folder adalah entri, apa pun hasil stat nya (hilang, device lain, symlink, hard link kedua); membaca nama ke `max_entries + 1` berhenti; tepat `max_entries` tetap lengkap | satu aturan yang tidak bergantung pada hasil stat, mudah diuji di kedua adapter, dan membatasi kerja per nama, termasuk nama yang hilang atau ada di device lain | menghitung hanya entri di device root, yang membiarkan folder penuh entri hilang atau mount point berjalan tanpa batas |
| 4 | `ENOENT` dilewati diam diam, errno lain `Err` (reason `io`) | disahkan dan dilengkapi: `ENOENT` saat stat atau open dilewati; folder yang berubah jenis atau identitas antara stat dan open tidak dimasuki tanpa mengubah `complete`; errno lain `Err` lewat `from_io` op `tree_usage`; `stacklyd` memetakan `NotFound` ke `missing`, `SymlinkInPath`, `NotADirectory`, dan `PermissionDenied` ke `not_private`, selain itu `io`, dan tidak menyimpan error di cache | folder data database berubah terus selama proses jalan, jadi race bukan kegagalan; error lain (misalnya subfolder tanpa izin baca) berarti folder tidak terbaca, sesuai AC-5 dan tabel *Kode error* | entri yang gagal dibaca dilewati dengan `complete: false`, yang menambah alasan keempat dan menyembunyikan masalah izin yang seharusnya dibereskan `stackly doctor` |
| 5 | root dibuka seperti `open_root` di `remove_tree` (`SymlinkInPath`, `NotADirectory`, `NotFound`) | disahkan | `stacklyd` sudah memeriksa folder data lewat `symlink_metadata`; open dengan `O_NOFOLLOW` menutup race antara cek dan scan dengan error yang sama dengan `remove_tree` | menerima root berupa symlink, yang membuka jalan scan keluar folder data |
| 6 | hard link lewat `(dev, ino)` hanya untuk bukan folder dengan `st_nlink > 1`; tenggat `Instant::now().checked_add(deadline)` dicek sebelum tiap entri | disahkan dan diperjelas: `None` berarti tanpa tenggat; dicek sekali per nama yang dikembalikan stream, sebelum stat nya, dengan perbandingan sama dengan atau lewat, jadi tenggat nol berhenti sebelum stat pertama dan root kosong tetap lengkap; nama dibaca satu per satu dari stream folder | folder punya `st_nlink` di atas 1 karena subfolder, jadi harus dikecualikan; membaca seluruh isi folder sekaligus (seperti `read_names` di `remove_tree`) membuat satu folder berisi jutaan nama melewati tenggat AC-19 dan memori sebelum cek pertama; tenggat nol memberi test conformance yang deterministik | cek hanya sebelum stat, dengan isi folder ditampung penuh |

Tambahan yang ditulis karena pembangun langkah 4 dan 7 akan menebaknya: urutan penelusuran tidak ditentukan dan test hasil tidak lengkap hanya memeriksa `complete: false` serta `bytes` yang tidak melebihi hasil lengkap; `FakeFilesystem` memakai `std::fs` dan `MetadataExt`; arti "privat" untuk folder data adalah `check_private_dir` spec 0004 yang sudah ada (folder, bukan symlink, milik user, `mode & 0o077` sama dengan 0). Rencana file developer (method dan tipe di `crates/platform/src/fs.rs`, impl `UnsupportedPlatform`, `crates/platform-macos/src/fs/tree_usage.rs` di atas `fs/fd.rs`, `FsOp::TreeUsage` dan `fake_fs/tree_usage.rs`, `check_tree_usage` di conformance) cocok dengan keputusan ini; `DirStream` yang dipindah ke `fd.rs` perlu cara membaca satu nama sekaligus untuk `tree_usage`, sementara `read_names` tetap untuk `remove_tree`.

Teks AC-5 yang berubah (menunggu persetujuan user): kalimat batas menjadi "berhenti dengan `complete: false` saat entri ke `DATA_USAGE_MAX_ENTRIES + 1` terbaca, setelah `DATA_USAGE_DEADLINE`, atau di folder yang lebih dalam dari `TREE_USAGE_MAX_DEPTH` tingkat, yang tidak dibuka dan isinya tidak dihitung" (alasan ketiga, dan batas entri yang tidak lagi bisa dibaca sebagai berhenti di tepat batas), rujukan "usulan spec 0004" menjadi "spec 0004", dan daftar pohon test mendapat "pohon yang lebih dalam dari `TREE_USAGE_MAX_DEPTH`".

### Item More "Duplicate…" (usulan spec 0024)

Usulan spec 0024 diterapkan apa adanya di butir toolbar *Halaman detail dan navigasi*, ditambah tempat item itu hidup: menu More langkah 10 adalah `ActionMenu` atas daftar `StacklyAction` berurutan yang di fitur ini hanya berisi Copy Connection String, dan milestone 3 spec 0024 menambah item "duplicate" sesudahnya beserta aksi, sheet, dan penerbitnya. AC-12 tidak berubah, karena sudah menyebut Duplicate tidak tampil sampai fitur 24 membangunnya. Baris Follow-up fitur 24 dipisah dari fitur lain karena fitur 24 tidak menambah tab.

### Cek update spec 0013

`ShellSync.events(kindPrefix:) -> AsyncStream<Event>` sudah ada di spec 0013 (baris API surface dan butir `.event` di *ShellSync*, merge `36bce94`, 2026-10-22) dengan teks yang sama dengan blok usulan spec ini dan dengan *Tab Summary* serta langkah 9. Baris Follow-up "bawa usulan spec 0013" bisa dicentang lewat `/sync`.

### Cross check update ini (2026-10-22)

Dijalankan oleh model lain (Sonnet, read only) atas draft update ini. Semua rekomendasi diterapkan dengan aturan user "opsi yang direkomendasikan yang diambil"; perubahan teks AC-5 tetap menunggu persetujuan user lewat lead.

| No | Temuan | Keputusan yang diterapkan |
| --- | --- | --- |
| 1 | "Setiap nama" ikut menghitung `.` dan `..`, yang dikembalikan `readdir` tetapi tidak oleh `std::fs::read_dir` di fake | "kecuali `.` dan `..`"; conformance root kosong dengan batas 0 lengkap |
| 2 | `DirStream` mengambil alih descriptor dan `read_names` membuka ulang `.`, jadi dua descriptor per tingkat, bertentangan dengan anggaran 64 | stream membungkus descriptor yang sama untuk `openat` dan `fstatat` lewat `dirfd`, `.` tidak dibuka ulang, flag folder `remove_tree` (`O_CLOEXEC`) |
| 3 | `ENOENT` dari membaca isi folder yang hilang di tengah jalan akan menjadi `NotFound`, lalu `missing` untuk seluruh folder | `ENOENT` di mana pun di bawah root dilewati, `NotFound` hanya dari root, `EINTR` diulang |
| 4 | Cek tenggat di akhir stream membuat pohon yang selesai tampak tidak lengkap | dicek sekali per nama yang dikembalikan, sebelum stat; root kosong lengkap juga dengan tenggat nol; jeda `set_delay` fake sebelum tenggat |
| 5 | Validasi argumen `root` tidak disebut; garis miring di akhir mengalahkan `O_NOFOLLOW` | root absolut tanpa `.`, `..`, NUL, atau garis miring akhir, selain itu `Io` `InvalidInput` |
| 6 | Cek privat ditulis ulang padahal `check_private_dir` sudah ada; hint `not_private` salah untuk symlink atau pemilik lain; folder 0o000 lolos cek lalu menjadi `io` | memakai `check_private_dir` dengan user dari `UserIdentity`; `PermissionDenied` menjadi `not_private`; hint baru |
| 7 | TTL cache tanpa sumber jam | `Clock` (`FakeClock` di test), segar jika dan hanya jika `scannedAt <= now < scannedAt + ttl`, kedaluwarsa sama dengan tidak ada (jadi `cachedOnly` membalas null), hit membalas `scannedAt` yang tersimpan. Runner up yang ditolak: `cachedOnly` membalas entri kedaluwarsa, yang melanggar "di cache selama TTL" di AC-5 |
| 8 | Penggabungan, semaphore, dan urutan cek tidak disebut | urutan lima langkah di *Ukuran data*; scan di task terlepas; `refresh` mengikuti scan berjalan; error dibagi dan tidak disimpan; tenggat tidak termasuk antrean permit, jadi AC-19 diukur saat permit tersedia; `refresh` dan `cachedOnly` bersamaan ditolak `Validate` |
| 9 | Test penggabungan dan semaphore tidak punya pengukur; kasus platform dan daemon tercampur | `tree_usage_calls()` dan `tree_usage_peak()` di fake; `DaemonConfig` membawa batas dan TTL; skenario test dipisah per langkah 4 dan 7 |
| 10 | Bentuk test tenggat nyata tidak ada | 50 000 file, tenggat 1 ms memberi `complete: false` dalam 100 ms, tenggat 60 detik lengkap |
| 11 | Cek ulang inode setelah open tidak bisa diuji dan tidak menambah keamanan untuk scan baca | dibuang; cek `ELOOP`/`ENOTDIR` dan device tetap |
| 12 | `readdir` di folder yang berubah bisa mengembalikan nama dua kali | dicatat bahwa hasil di folder yang berubah adalah perkiraan |
| 13 | AC-5 "berhenti di `DATA_USAGE_MAX_ENTRIES` entri" bisa dibaca bertentangan dengan "tepat batas lengkap"; rujukan "usulan spec 0004" usang | teks AC-5 diusulkan ulang (menunggu persetujuan); baris *Batas dengan spec lain* diperbarui |
| 14 | `ActionMenu` menerima bagian (`[[StacklyAction]]`), bukan daftar datar; blok usulan spec 0024 untuk spec ini belum bertanda diterapkan | menu More satu bagian, Duplicate di bagian yang sama tanpa pemisah, Remove (fitur 54) bagian kedua; penanda di spec 0024 milik pemiliknya (dilaporkan ke lead) |

## Prasyarat milestone 4 (2026-10-22)

Dari *Usulan update spec lain (2026-10-22)* butir 5 spec 0023, dibawa lead scope 02 lewat brief `arc-f15-update-4`. Prasyarat NATS milestone 4 berubah dari "fitur 23 milestone 2" menjadi milestone endpoint `Auto` fitur 23 (milestone 3 baru di scope, langkah 13 dan 14 spec 0023), karena test native langkah 12 men start NATS yang dibuat dengan endpoint default `Auto`, dan init NATS `1.0.0` gagal tanpa port. Hanya baris prasyarat yang berubah; bagian yang sedang dibangun milestone 1 (`dev-f21-m1`) tidak disentuh.

## Paket pinned yang tidak terbaca (2026-10-22)

Gap yang ditemukan `dev-f21-m1` saat membangun milestone 1 (merge `f709b403`), dibawa lead scope 02 ke sesi `arc-f15-update-4`. AC-2 menulis bahwa entri berstatus `package_unavailable` saat "paket pinned tidak terbaca atau digest beda", padahal paket yang tidak terbaca tidak punya entri sama sekali: `render` butuh `ValidatedPackage`, jadi `detail::build` (`crates/core/src/instance/detail.rs`) mengembalikan `connections` kosong dan `displayName` null. Di kode, `package_unavailable` per entri hanya muncul untuk paket yang bukan paket pinned saat `render` dipanggil langsung, atau entri yang tidak cocok dengan context (token yang tidak terselesaikan, atau field secret tanpa pemakaian tercatat). Akibatnya GUI menulis "This provider doesn’t describe a connection string" untuk paket yang rusak, dan CLI gagal `cli.connection_not_found`, dua duanya salah sebab. Keputusan ini harus ada sebelum langkah 7 (pemetaan daemon); DTO langkah 6 belum dibangun, jadi field baru tidak menambah kenaikan minor di luar yang sudah direncanakan.

- **Option 1: field eksplisit `DetailProvider.packageReadable` (dipilih).** `detail::build` mengisi nya dari paket yang ada dan cocok dengan provider revision instance. GUI dan CLI memperlakukan false sebagai alasan `package_unavailable` dengan teks dan kode yang sudah ada. Kelebihan: satu bool yang jelas di kontrak yang belum dibangun, tanpa entri palsu, dan "provider tanpa blok" serta "paket rusak" tidak tertukar lagi. Kekurangan: satu field lagi di DTO dan satu field di struct core yang sudah di `main` (ditambah langkah 7).
- **Option 2: satu entri sintetis `package_unavailable`.** Kekurangan: nama dan judul entri tidak diketahui tanpa manifest, jadi entri itu harus memakai nama karangan yang bisa bentrok dengan `--format`, dan paritas `--all` serta `inspect` ikut aneh.
- **Option 3: `displayName` null sebagai tanda.** AC-4 sudah membuat `displayName` null jika dan hanya jika paket tidak terbaca, jadi tanpa field baru pun informasi nya ada. Kekurangan: aturan tersembunyi yang patah begitu `displayName` boleh kosong di manifest atau diambil dari sumber lain; client harus tahu arti kedua dari field tampilan.
- **Option 4: method gagal.** Kekurangan: AC-4 sengaja menjaga Summary tetap tampil (endpoint, proses, readiness) walau paket rusak.

**Keputusan kecil.** Paket di cache yang `key()` nya bukan provider revision instance dihitung tidak terbaca (sama dengan filter di `detail::build`). Di CLI, setiap bentuk `instance connection` gagal `cli.connection_unavailable` `reason` `package_unavailable` (exit 3), dengan kunci `format` hanya saat `--format` dikirim, karena tanpa manifest tidak ada nama entri default. `inspect` mencetak satu baris pengganti tabel, dan tabel tidak dicetak untuk provider tanpa blok (kasus yang sebelumnya tidak tertulis). Fixture kontrak baru `detail-package-unreadable.json`; tiga fixture triple AC-7 tetap.

**Teks AC yang berubah (menunggu persetujuan kamu lewat lead).** AC-2 (urutan status ditulis seperti kode, dan paket tidak terbaca tidak dirender) dan AC-4 (`packageReadable` dan perlakuan nya di GUI dan CLI). Teks lama dan baru lengkap di catatan bersama `arc-f15-update-4-ac-and-scope.md`.
