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
- **Perintah.** Spec 0069 membuat halaman Instances menerbitkan `openInstance`, `back`, dan `copyConnectionString`, dan klik ulang sidebar lewat hook `onReselect`. Teks AC-15 kini menyebut action yang sama untuk tombol Open dan Copy di inspector, disetujui user lewat lead (2026-10-21). Teks AC-12 belum diubah: usulan menyebut tooltip More lewat `CommandMap.helpText(for:)`, padahal More tidak punya `CommandID`, jadi koreksinya diajukan lagi ke lead.
- **Snapshot.** Langkah 10 menambah suite `InstanceDetailSnapshotTests` di atas harness spec 0068. Teks AC-16 (cek teks besar lewat `#Preview`) tetap, atas keputusan user.
- **Cross check (2026-10-21).** Cross check `Another model` (Sonnet 5.5) atas update ini dan perbaikannya diterima lead atas delegasi user (2026-10-21): tooltip aksi utama lewat `helpText(for:title:)` dan More dengan help bawaan, `fixtureNow` untuk Uptime dan "scanned", cadangan `select(_:)` dan `onReselect` di langkah 10 (D11), `key` `CopyButton`, adapter `InstanceActionModel.stacklyAction(for:)`, dan tombol Open sebagai action `openInstance` (D12).
- **Kolom halaman 384 pt (2026-10-21).** Temuan langkah 14 spec 0067: di jendela 900 pt dengan sidebar dan inspector terbuka, kolom halaman hanya 384 pt. User menerimanya, jadi halaman detail harus bekerja di lebar itu saat inspector Instances terbuka; `.labeledContentStyle(.stackly)` sudah menumpuk label dan nilai saat sempit. Teks AC-16 (640 pt) belum diubah dan diajukan ke lead.
