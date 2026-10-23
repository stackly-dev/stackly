# 0024. Instance kedua dari runtime yang sama: rationale

[Kembali ke spec](index.md)

## Context

Nilai inti Stackly (PRD GOAL-01, JOURNEY-02) adalah beberapa instance dari satu runtime yang berjalan bersamaan tanpa saling mengganggu: `pg17-dev` dan `pg17-test` dari satu PostgreSQL 17 terpasang, masing masing dengan data, config, log, temp, socket, identitas, dan lifecycle sendiri (REQ-009). Runtime boleh dipakai ulang; folder data tidak boleh dibagi. Fitur 24 di `docs/scope/03-multi-versi-dan-instance.md` meminta dua jalan menuju instance kedua: create biasa dan duplicate config, dengan syarat duplicate membuat storage baru, bukan clone data (REQ-025, AC-025).

Create biasa sudah dirancang lengkap di spec 0020: preflight dan create memakai satu evaluator, folder instance selalu turunan ID baru dan tidak pernah diinisialisasi kalau sudah ada isinya (BR-008), port fixed yang konflik ditolak dengan saran (BR-005), dan seluruh operasi bisa dikompensasi. Yang belum ada adalah cara mengisi form dari instance yang sudah ada, plus bukti bahwa dua instance dari runtime yang sama benar benar terpisah sampai ke data yang dibaca server nya, termasuk setelah stop lalu start. Bukti untuk mayor berbeda (satu PostgreSQL 18 di samping dua PostgreSQL 17) milik fitur 25.

Gaya yang membentuk keputusan ini: BR-005 (hanya permintaan auto yang boleh menghasilkan port pilihan manager), spec 0011 AC-6 (daemon tidak pernah memilih reference yang sudah ada untuk resource baru atas inisiatifnya sendiri) dan Follow-up nya untuk fitur 24 (duplicate memakai `generate` atau `copyFrom` ke reference baru, tidak pernah `bind` diam diam), BR-001 (instance menyimpan provider revision dan runtime exact), aturan daemon sebagai satu satunya yang memutuskan (AGENTS.md), mockup (Duplicate… di menu More membuka sheet create dengan runtime sumber, nama `<sumber>-copy`, dan mode port auto), dan peta perintah spec 0069 (Duplicate… ⌘D di menu Instance). Pendekatan build proyek adalah Journey.

Tanpa keputusan ini, duplicate akan dibangun sebagai jalur create kedua (dengan risiko jaminan spec 0020 tidak ikut), atau sebagai salinan folder (melanggar REQ-025), dan port serta password bisa ikut tersalin tanpa disadari pengguna.

## Options considered

### Keputusan 1: permukaan duplicate

#### Option 1: satu method baca `instance.get_duplicate_form`, lalu create biasa (dipilih)

Daemon membangun form dan draft create dari instance sumber; client mengedit draft lalu memakai `instance.preflight` dan `instance.create` spec 0020 apa adanya.

**Pros**:

- Tidak ada jalur create kedua: validasi, kompensasi, idempotency, dan paritas CLI dan GUI spec 0020 berlaku otomatis.
- Data sumber tidak mungkin tersalin, karena method baru hanya membaca registry dan paket, dan create tidak tahu ada sumber (ia hanya memeriksa identitas folder data setiap instance untuk BR-002).
- Pengguna melihat dan mengubah draft sebelum Create, dengan preflight yang sama.

**Cons**:

- Dua panggilan (form lalu create) dan dua langkah di CLI, bukan satu perintah atomik.
- Draft bisa basi kalau sumber berubah di antara form dan create; preflight dan create tetap memeriksa ulang, tetapi isi settings tidak ikut berubah.

#### Option 2: method mutasi `instance.duplicate { sourceId, name, … }`

Daemon membaca sumber dan menjalankan operasi create di dalam satu panggilan.

**Pros**:

- Satu panggilan; CLI paling sederhana.

**Cons**:

- Butuh preflight versi duplicate atau tanpa preflight sama sekali, jadi paritas dengan create harus dijaga dua kali.
- Params nya meniru params create (port, settings, secret) atau membatasi pengguna; keduanya permukaan kedua yang harus dirawat.
- Operasi baru berarti journal, resume, dan kompensasi baru, atau pembungkus yang menyamarkan operasi create.

#### Option 3: client membangun draft dari data yang sudah ada

GUI dan CLI membaca `instance.get_detail` dan config, lalu menyusun draft sendiri.

**Pros**:

- Tanpa method baru.

**Cons**:

- Aturan penyaringan (secret, managed, nama, port) ditulis dua kali, di Swift dan Rust, melanggar aturan bahwa daemon yang memutuskan.
- Config revision belum ada di DTO detail sampai fitur 29.

### Keputusan 2: port di draft duplicate

#### Option 1: semua endpoint IP `auto` (dipilih)

**Pros**:

- Sesuai BR-005: hanya permintaan auto yang menghasilkan port pilihan Stackly, dan port fixed selalu datang dari pengguna.
- Tidak pernah konflik dengan sumber, sehingga draft langsung `ready` tanpa langkah tambahan.
- Sama dengan mockup (`mode: 'auto'`) dan tanpa bind percobaan di method form.
- Port auto tersimpan di `bound` dan dipakai ulang selama bebas (spec 0010), jadi tetap stabil di start berikutnya.

**Cons**:

- Port auto adalah port efemeral yang tinggi dan baru diketahui setelah start pertama; pengguna yang ingin port mudah diingat harus memilih Fixed sendiri.
- Kalau sumber memakai port fixed, duplicate tidak meniru "gaya" itu.

#### Option 2: port fixed bebas pertama sesudah port sumber

Daemon menjalankan pencarian saran AC-5 spec 0020 dan mengisi draft dengan `Fixed(p+1)` atau kandidat bebas berikutnya.

**Pros**:

- Port langsung diketahui dan mudah diingat (5433 di samping 5432).

**Cons**:

- Daemon memilih port fixed tanpa aksi pengguna, terutama di CLI tanpa `--port`; ini melanggar BR-005 dan AC-5 spec 0020 ("saran tidak pernah dipakai tanpa aksi pengguna").
- Method form harus membuka socket percobaan, jadi tidak lagi murni baca registry.

#### Option 3: salin permintaan sumber apa adanya

**Pros**:

- Paling setia pada config sumber.

**Cons**:

- Port fixed sumber selalu konflik (langkah 3 *Pemeriksaan port*), jadi setiap duplicate mulai dari error dan butuh klik "Use {n}".

### Keputusan 3: kebijakan secret default

#### Option 1: `generate`, `copyFrom` hanya lewat aksi pengguna (dipilih)

**Pros**:

- Sesuai spec 0011 AC-6 dan Follow-up nya: daemon tidak memilih reference sumber atas inisiatifnya sendiri, dan `bind` tidak pernah terjadi diam diam.
- Identitas terpisah (REQ-009): bocornya password satu instance tidak membuka instance lain.
- Copy from tetap satu klik (GUI terisi lebih dulu dengan reference sumber) atau `--secret <field>=copy` di CLI.

**Cons**:

- Pengguna yang ingin aplikasinya cukup mengganti port harus memilih Copy from per secret field.

#### Option 2: `copyFrom` reference sumber sebagai default

**Pros**:

- Paling dekat dengan arti "duplicate"; connection string hanya beda port.

**Cons**:

- Daemon membaca nilai reference yang sudah ada untuk resource baru tanpa diminta, melawan spec 0011 AC-6.
- Dialog Keychain bisa muncul untuk reference sumber walaupun pengguna tidak memintanya.

#### Option 3: `bind` ke reference sumber

**Pros**:

- Satu password untuk dua instance, berubah bersama.

**Cons**:

- Persis yang dilarang Follow-up spec 0011 ("tidak pernah `bind` diam diam"); remove sumber (fitur 54) tersandung reference yang masih dipakai; rotasi password satu instance mengubah instance lain.

### Keputusan 4: provider revision dan runtime instance baru

#### Option 1: sama persis dengan sumber, runtime terkunci (dipilih)

**Pros**:

- Fitur ini memang tentang runtime ID identik; settings sumber dijamin cocok dengan schema yang sama (BR-001).
- Form dibangun dari paket pinned sumber, jadi tetap bisa walaupun catalog sudah pindah revision atau tidak dimuat. Satu kasus ditolak lebih dulu: catalog development dengan label revision yang sama tetapi isi lain, karena create spec 0020 akan memakai paket catalog itu (cross check temuan 1).

**Cons**:

- Duplicate ke mayor lain atau ke revision paket yang lebih baru tidak bisa; pengguna memakai New Instance.
- Runtime sumber yang rusak menutup duplicate sampai diperbaiki.

#### Option 2: revision catalog terbaru dan runtime yang bisa dipilih

**Pros**:

- Lebih luwes; bisa "upgrade sambil duplicate".

**Cons**:

- Settings sumber bisa tidak valid untuk schema lain, jadi duplicate sering mulai dari error; tumpang tindih dengan fitur 25 dan 58 (migrasi) dan fitur 59 (update provider revision).

### Keputusan 5: isi settings yang disalin

#### Option 1: semua nilai config revision aktif kecuali secret dan managed (dipilih)

**Pros**:

- Itulah "config" yang disetel pengguna; field managed (misalnya `serverName` NATS) memang milik Stackly per instance, dan secret punya kebijakan sendiri.
- Field `immutableAfterInit` aman disalin, karena untuk instance baru itu hanya nilai awal init.

**Cons**:

- Nilai yang kebetulan sama dengan default tetap dikirim eksplisit; perintah setara menyaringnya karena hanya menulis nilai yang beda dari default (spec 0020).

#### Option 2: salin juga nama tampil dan kebijakan startup

**Pros**:

- Instance baru lebih mirip sumber di daftar.

**Cons**:

- Nama tampil adalah identitas, dan dua instance dengan nama tampil sama membingungkan; `startup` `at_login` belum ada sampai fitur 31, dan menyalinnya diam diam membuat instance baru ikut start saat login.

### Keputusan 6: nama saran

#### Option 1: `<sumber>-copy`, lalu `-copy-2` sampai `-copy-99` (dipilih)

**Pros**:

- Sesuai mockup dan menunjukkan hubungan dengan sumber; duplikat dari duplikat tidak menumpuk `-copy-copy`.

**Cons**:

- Nama panjang; pengguna hampir selalu menggantinya (misalnya ke `pg17-test`).

#### Option 2: aturan nama saran create (`postgresql-17-2`)

**Pros**:

- Satu aturan nama untuk semua jalur.

**Cons**:

- Tidak menunjukkan dari mana instance itu berasal dan menyimpang dari mockup.

### Keputusan 7: bentuk CLI

#### Option 1: `stackly instance duplicate <source> [<name>]` (dipilih)

**Pros**:

- Kata kerjanya sama dengan GUI (Duplicate…), mudah ditemukan di help, dan flag runtime, provider, serta revision memang tidak ada.

**Cons**:

- Satu subcommand lagi yang flag nya hampir sama dengan `create`.

#### Option 2: `stackly instance create <name> --from <source>`

**Pros**:

- Satu perintah create.

**Cons**:

- `--runtime`, `--provider`, dan `--revision` harus ditolak hanya saat `--from` ada; help create menjadi lebih rumit, dan `<name>` wajib walaupun ada nama saran.

### Keputusan 8: catatan asal duplicate

#### Option 1: tidak dicatat di registry atau params create (dipilih)

**Pros**:

- Create, kontraknya, dan registry tidak berubah; duplicate tetap hanya cara mengisi form.
- Journal operasi create sudah menyimpan draft (termasuk reference `copyFrom`) kalau nanti dibutuhkan.

**Cons**:

- Instance hasil duplicate tidak bisa ditanya "dari mana asalnya" lewat API; fitur 35 harus memutuskan kalau tab Activity membutuhkannya.

#### Option 2: field `duplicateOf` di params create dan kolom registry

**Pros**:

- Asal terlihat di detail dan Activity.

**Cons**:

- Mengubah kontrak create spec 0020 (kenaikan minor tambahan dan migrasi) untuk informasi yang belum diminta AC mana pun; asal bisa menunjuk instance yang sudah di purge.

### Keputusan 9: perintah setara di sheet mode duplicate

#### Option 1: `stackly instance create …` lengkap (dipilih)

**Pros**:

- Fungsi `equivalent_command` spec 0020 dipakai tanpa perubahan, dan perintahnya menghasilkan instance yang sama walaupun sumber berubah atau dihapus.

**Cons**:

- Lebih panjang dan tidak menyebut kata duplicate.

#### Option 2: `stackly instance duplicate <sumber> <nama> …`

**Pros**:

- Pendek dan mencerminkan aksi yang dipilih.

**Cons**:

- Hasilnya bergantung pada keadaan sumber saat perintah dijalankan, jadi bukan "perintah yang setara" dengan sheet; butuh fungsi kedua dengan round trip test sendiri.

### Keputusan 10: titik masuk di GUI

#### Option 1: item More di halaman detail dan Duplicate… ⌘D di menu Instance (dipilih)

**Pros**:

- Persis mockup dan peta perintah spec 0069; menu More adalah jalan di jendela, ⌘D jalan cepat dari tabel.

**Cons**:

- Dari tabel tanpa keyboard, pengguna harus membuka detail dulu atau memakai menu bar.

#### Option 2: tambah tombol di inspector atau menu konteks tabel

**Pros**:

- Lebih mudah ditemukan dari tabel.

**Cons**:

- Tidak ada di mockup; menu konteks tabel belum ada sama sekali (spec 0022), jadi fitur ini harus memulainya untuk satu aksi.

### Keputusan 11: bukti isolasi di fixture

#### Option 1: rute `PUT` dan `GET /sentinel` di fake server (dipilih)

**Pros**:

- Membuktikan bahwa setiap proses server membaca folder data nya sendiri, bukan hanya bahwa path nya berbeda.
- Dipakai ulang fitur isolasi berikutnya (backup, restore as new) tanpa provider nyata.

**Cons**:

- Dua rute baru di binary test yang harus dirawat.

#### Option 2: test menulis dan membaca file di folder data langsung

**Pros**:

- Tanpa perubahan fake server.

**Cons**:

- Hanya membuktikan path berbeda; tidak membuktikan proses yang berjalan memakai path itu.

## Rationale

Duplicate dijadikan cara mengisi form, bukan operasi baru (keputusan 1), karena semua risiko fitur ini ada di create: folder yang tidak baru, port yang diganti diam diam, kompensasi yang bocor. Spec 0020 sudah menutup semuanya dengan satu evaluator dan satu operasi; jalur kedua hanya akan membuka lagi celah yang sama. Dengan bentuk ini, "data lama tidak pernah disalin" bukan janji yang perlu dijaga test saja: tidak ada kode yang membaca isi pohon instance sumber (create hanya memeriksa identitas folder data sumber untuk aturan tumpukan BR-002), dan test dengan folder data sumber mode 000 menjaga agar tidak ada yang menambahkannya kelak.

Port dan secret (keputusan 2 dan 3) dipilih supaya tidak ada keputusan tersembunyi. BR-005 dan spec 0011 AC-6 sama sama berkata bahwa Stackly hanya memilih sendiri lewat permintaan auto dan `generate`. Option fixed yang lebih nyaman di keputusan 2 terlihat menarik di GUI, tetapi di CLI tanpa `--port` ia berarti daemon memilih port fixed tanpa aksi pengguna, dan karena itu ditolak. Kenyamanan tetap ada: Fixed dan "Use {n}" satu klik, dan Copy from terisi lebih dulu.

Runtime dan provider revision dikunci ke sumber (keputusan 4) karena itulah inti fitur ini (runtime ID identik) dan karena settings sumber hanya dijamin valid untuk schema yang sama. Pilihan lain (nama, CLI, perintah setara, titik masuk, catatan asal) mengikuti mockup dan peta perintah lalu memilih yang tidak mengubah kontrak create. Bukti fixture memakai rute sentinel (keputusan 11) karena REQ-009 bicara tentang apa yang dibaca proses, bukan hanya tentang path.

## Kesepakatan antarsesi

- **arc-f25-spec (spec 0025), 2026-10-22**: spec ini memiliki bukti dua instance PostgreSQL 17 dari runtime ID yang sama (create biasa dan duplicate, sentinel, stop lalu start per instance) dan menulis evidence AC-009 untuk bagian itu; spec 0025 menambah instance PostgreSQL 18, matriks tiga cluster, penolakan rebind, dan menutup AC-009. Helper `SentinelCluster` di `apps/daemon/tests/postgresql/sentinel.rs` dan penyiapan `two_postgresql_17_instances` di `apps/daemon/tests/postgresql/same_runtime.rs` dipakai ulang spec 0025, dan milestone tiga cluster spec 0025 berjalan after langkah 9 spec ini. Duplicate lintas mayor di luar cakupan kedua spec (runtime terkunci ke sumber).
- **Lead (Scope 03), 2026-10-22**: worktree di merge dengan `main` sebelum bagian port dan preflight ditulis (update spec 0020 `SO_REUSEADDR` dan F19 milestone 2); kenaikan protokol ditulis "naik minor" tanpa angka versi.
- **Lead (Scope 03), spec 0020 Keputusan 19 (`49f0f4f`), 2026-10-22**: daftar Copy from mode create dibaca dari `secret.list` bersama method form dalam satu tugas muat bertoken, dengan label "{purpose} of “{nama}”". Mode duplicate memakai tugas muat yang sama dengan `getDuplicateForm`, menaruh reference sumber setiap field di depan daftar itu tanpa duplikat dan tanpa melihat statusnya, dan menggabungkan label sumber ke `copyFromLabels` mode create dengan label sumber menang. Alasannya: satu mekanisme melayani kedua mode; reference sumber, pilihan yang paling mungkin, tidak pernah menunggu `secret.list` atau hilang saat daftar gagal; dan daemon tetap satu satunya yang menolak reference tanpa nilai (baris preflight `secret.<field>`). Label sumber menang karena judul field dari form lebih tepat daripada purpose yang diturunkan dari teks reference, walaupun untuk PostgreSQL hasilnya sama ("Superuser password of “pg17-dev”"). Karena `SecretField` menerima satu daftar per field, `CreateInstanceModel.copyFromOptions(for:)` menghitung daftar per field, dan di mode create hasilnya sama untuk setiap field.
- **Lead (Scope 03), 2026-10-23**: teks AC-9 diselaraskan dengan aturan covers evidence, disetujui user 2026-10-23. `check_covers` di `crates/evidence/src/rules.rs` hanya menerima ID berpola `AC`, `REQ`, `NFR`, `VAL`, atau `BR`, jadi `covers` AC-9 kini `["REQ-009", "AC-009", "AC-025"]` dan JOURNEY-02 disebut di teks `expected`, sama dengan gaya AC-9 spec 0025. Isi AC lain tidak berubah.

## Cross check

Dijalankan 2026-10-22 dengan model lain (Fable 5.1), read only, atas `index.md` dan `rationale.md` ditambah spec 0005, 0010, 0011, 0020, 0021, 0022, 0069 dan kode `crates/core/src/instance/create/`, `crates/core/src/supervisor/prepare.rs`, `crates/fake-server/`, `apps/daemon/tests/postgresql/`, `apps/daemon/tests/supervisor/`, dan `SecretField.swift`. Putusan: desain sehat; 15 temuan, tiga di antaranya high. Semua diterapkan dengan perbaikan yang direkomendasikan, menunggu penerimaan lead atas nama user:

| No | Temuan | Perbaikan di spec |
| --- | --- | --- |
| 1 (high) | Label revision sama dengan catalog tetapi digest lain tidak gagal di pin: `pin_revision` membuat folder digest kedua, create berhasil dengan provider revision key lain dari sumber, dan lookup revision berikutnya menjadi ambigu | `gather_duplicate` menolak lebih dulu dengan `instance.provider_ambiguous` (AC-1, *Draft duplicate*, Consequences); `DuplicateDeps` membawa `CatalogState` |
| 2 (high) | `SecretField` sudah punya `copyFromOptions` dan `SecretDraft.copyFromReference`; `preferredReference` tidak perlu, dan sumber daftar Copy from di mode duplicate tidak disebut | model mengisi `copyFromOptions` (reference sumber dulu) dan `copyFromReference`; usulan spec 0011 diganti menjadi `copyFromLabels` untuk label "{judul field} of “{sumber}”" |
| 3 (high) | Skenario masukan fixture `duplicate-form-*.json` tidak ditetapkan, padahal test Swift AC-12 bergantung padanya | tabel *Skenario fixture duplicate* di *Method IPC* |
| 4 | `PackageUnavailableReason` tidak ada; tahap `verify` memakai `load_package` dengan `reason: String` | `load_package` dijadikan `pub(crate)` dan dipakai ulang; tiga teks reason disebut di AC-1 |
| 5 | Tujuh path dan `ownership` `created` tidak terlihat lewat wire | path diturunkan test dari `DirectoryLayout::instance_paths`; klaim `ownership` dihapus |
| 6 | `http::read_request` fake server membuang body, jadi `PUT /sentinel` tidak bisa membaca nilai | `Request.body`; helper `put_sentinel` dan `get_sentinel` di `supervisor/support.rs` |
| 7 | Tidak ada helper yang mengubah draft menjadi `CreateInstanceParams` di test | `Api::create_from_draft` |
| 8 | `RuntimeSummary` tidak membawa nama tampil; tipe `preselect` tidak disebut | nama tampil dari `form.provider.displayName`, tile dari `CatalogStore`, `preselect: UUID?` |
| 9 | Detail error kurang lengkap | detail setiap kode disebut di AC-1 dan *Draft duplicate* |
| 10 | Pemeriksaan tumpukan create membaca metadata folder data sumber, dan folder data mode 000 membuat pemeriksaan itu dilewati | AC-7 dipersempit ke "tidak membaca isi"; test kini membuat isi folder data mode 000 sementara folder nya tetap 0700 |
| 11 | `Registry::provider_revision` tidak perlu untuk mencocokkan digest | baris itu dihapus dari *Model data* dan *Value sourcing* |
| 12 | Nama saran create untuk instance kedua adalah `postgresql-17` kalau instance pertama bernama lain | kalimat *Alur* diperbaiki |
| 13 | `values` config yang bukan object tidak diputuskan | `RegistryError::Corrupt` (AC-2) |
| 14 | Baris stderr CLI saat `--json` tidak diputuskan | tetap dicetak (*CLI*) |
| 15 | Soundness lain (draft basi, sumber dihapus, runtime dihapus, Keychain terkunci, reference hilang) | tidak ada perubahan; semuanya jatuh ke baris preflight spec 0020 |
