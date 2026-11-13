# 0044. Manifest environment, validate dan plan: alasan

Catatan keputusan untuk [index.md](index.md). `/develop` tidak membaca file ini.

## Context

Sesudah scope 07, Stackly bisa memasang dan menjalankan enam provider resmi, satu instance demi satu instance. Tim yang ingin berbagi "service apa saja yang dibutuhkan proyek ini" belum punya tempat menuliskannya. REQ-020 meminta file deklaratif dengan schema ber versi dan plan yang menunjukkan dampak tanpa download, spawn, pemberian trust, atau perubahan file. VAL-013 dan VAL-014 meminta manifest yang salah ditolak sebelum ada mutasi, dan instance yang dipinjam tidak pernah dikonfigurasi ulang oleh peminjamnya.

Fitur ini adalah yang pertama dari empat fitur environment (44 sampai 47). Tiga spec berikutnya (lockfile, up, down dan lease) bertumpu pada schema dan identitas environment yang diputuskan di sini, jadi setiap hal yang dibiarkan kabur di sini akan ditebak tiga kali. Karena itu spec ini memutuskan schema selengkap yang dibutuhkan keempatnya, dan menyebut titik sambungnya satu per satu.

Fondasi yang sudah ada membatasi ruang gerak:

- Domain `Environment`, `EnvironmentMember`, `Ownership`, dan `Lease` (spec 0004), tabel `environments`, `environment_instances`, dan `instance_demands` (spec 0005), dan jenis operasi `environment_up` dan `environment_down` (spec 0007) sudah ada dan harus dipakai.
- Create instance sudah punya evaluasi murni (`evaluate`) dan pengumpulan fakta yang hanya membaca (`gather`), dipakai `instance.preflight` (spec 0020).
- Spec 0011 sudah menetapkan grammar reference `local:`, `SecretInput::Bind` untuk env up, dan bahwa nilai secret tidak pernah ada di `stackly.yaml`.
- Trust provider lokal (spec 0037) sudah punya gate; provider `pending` boleh dibaca tetapi tidak boleh dijalankan.
- `EventKind` tertutup, jadi event baru berarti major protokol.
- Setiap mutasi adalah operasi, dan hanya daemon yang menulis registry.
- Mockup menampilkan environment yang belum pernah Up di sidebar ("0/2", "Will be created"), jadi pendaftaran harus terjadi sebelum Up.

Kalau tidak diputuskan: empat spec akan membuat empat pengertian "environment yang sama", plan dan create bisa berbeda pendapat tentang port atau settings, dan password bisa masuk ke file yang di commit.

## Options considered

Mode keputusan sesi ini `auto`: opsi yang direkomendasikan diambil, runner up dicatat. Tanda `berat` berarti mahal dibalik sesudah build.

### K1: identitas schema (`biasa`)

- **Diambil: `apiVersion: stackly/v1alpha1`, `kind: Environment`.** Sama dengan manifest provider (spec 0009) dan contoh PRD; satu loader, satu aturan versi. Kontra: `v1alpha1` menjanjikan bisa berubah, jadi tim perlu tahu file bisa butuh migrasi.
- Runner up: namespace sendiri (`stackly.env/v1`). Pro: versi environment lepas dari provider. Kontra: dua skema versi untuk satu produk, dan contoh PRD jadi tidak valid.

### K2: seberapa ketat loader (`biasa`)

- **Diambil: ketat.** Field tak dikenal, key ganda, anchor dan alias, dan versi tanpa kutip ditolak. Salah ketik (`endpoint:`) ketahuan, bukan diabaikan diam diam. Kontra: file yang ditulis untuk versi Stackly lebih baru ditolak versi lama.
- Runner up: longgar dengan warning. Pro: maju mundur lebih mudah. Kontra: salah ketik pada `secretRefs` berarti secret dibangkitkan diam diam.

### K3: ownership dan `instanceRef` (`berat`)

- **Diambil: `instanceRef` berisi nama instance, dan service shared tidak boleh membawa field konfigurasi apa pun.** VAL-014 ("tidak otomatis direkonfigurasi") menjadi benar secara struktur, bukan lewat cek saat jalan. Nama dipilih karena UUID tidak bisa dibagikan antar Mac. Kontra: tim harus sepakat nama instance bersama; service shared tidak bisa menyatakan "butuh minimal versi X".
- Runner up: `instanceRef` boleh membawa `version` sebagai syarat. Pro: salah pinjam versi ketahuan. Kontra: aturan cocok versi baru yang belum diminta siapa pun; bisa ditambah nanti tanpa merusak file lama.

### K4: identitas environment (`berat`)

- **Diambil: nama unik, terikat ke lokasi file, dengan relink eksplisit ke id environment.** PRD meminta identitas yang tidak hanya bergantung pada working directory. Nama terbaca manusia, cocok untuk CLI (`stackly env plan erp-development`), dan lease menempel ke `id` baris itu. Ikatan lokasi mencegah dua proyek berbeda yang sama sama bernama `dev` saling menimpa diam diam. Kontra: dua checkout proyek yang sama tidak bisa terdaftar bersamaan.
- Runner up: identitas adalah lokasi file, nama tidak unik. Pro: dua checkout bisa berdampingan. Kontra: memindah folder proyek memutus lease dan member; CLI butuh path atau UUID untuk menunjuk environment; port fixed kedua checkout tetap bentrok.
- Opsi ketiga: nama unik tanpa ikatan lokasi (file mana pun dengan nama itu adalah environment itu). Kontra: manifest proyek lain dengan nama sama akan menampilkan member proyek pertama sebagai miliknya.

### K5: kapan environment didaftarkan (`berat`)

- **Diambil: import adalah operasi `import_environment` yang hanya menulis baris registry dan snapshot.** JOURNEY-04 menaruh import sebelum plan dan up, dan mockup menampilkan environment yang belum Up. Operasi memberi idempotency, audit, dan guard transaksi yang sudah ada. Kontra: dua `OperationKind` baru dan dua method yang menulis, di spec yang selebihnya hanya membaca.
- Runner up: baris baru dibuat saat Up pertama (spec 0046), import di GUI hanya membuka sheet plan. Pro: fitur ini murni baca. Kontra: sidebar kosong sampai Up, "import lalu tinjau besok" tidak mungkin, dan halaman Environments tidak bisa diuji sebelum fitur 46.

### K6: remove (`berat`)

- **Diambil: `env.remove` hanya saat tidak ada lease aktif; menghapus pendaftaran saja.** Tanpa remove, daftar hanya bisa bertambah. Aturannya memakai fondasi yang sudah stabil (state lease di domain), jadi tidak mendahului spec 0047. Instance yang tadinya dedicated menjadi manual, data utuh.
- Runner up: serahkan remove ke spec 0047. Pro: wilayah lebih bersih. Kontra: environment yang salah impor tidak bisa dibuang sampai fitur 47, dan spec 0047 tidak punya remove di daftar wilayahnya.

### K7: snapshot service di registry (`berat`)

- **Diambil: kolom `environments.services` berisi service yang dideklarasikan saat terakhir diterima.** `env.list` dipanggil sering (sidebar, Overview) dan tidak boleh bergantung pada file pengguna yang bisa hilang, ada di volume jaringan, atau besar. Kontra: registry dan file bisa berbeda, jadi butuh `manifestState` dan Reload.
- Runner up: baca dan parse file di setiap `env.list`. Pro: tidak ada dua sumber. Kontra: IO ke path sembarang di jalur baca yang sering; file hilang berarti hitungan `total` hilang.

### K8: validate dan plan dipisah menurut apa yang dibaca (`biasa`)

- **Diambil: validate = file dan catalog; plan = validate ditambah registry dan keadaan Mac.** Validate memberi hasil sama di Mac mana pun dengan catalog sama (termasuk provider lokal yang diimpor), cocok untuk pemeriksaan sebelum commit; plan menjawab "di Mac ini apa yang terjadi". Bentrok port dengan instance terdaftar dan `instanceRef` yang tidak ada karena itu blocker plan, bukan finding validate.
- Runner up: satu method. Kontra: validate jadi bergantung keadaan lokal, dan CI tim tidak bisa memakainya.

### K9: plan memakai evaluasi create (`berat`)

- **Diambil: `gather` dan `evaluate` create dipakai ulang, dengan fakta `planned_runtime` untuk runtime yang belum terpasang (draft create tidak berubah).** Satu aturan port, settings, secret, disk, dan nama; plan tidak bisa menjanjikan sesuatu yang ditolak create. Kontra: menyentuh jalur create yang stabil; trial bind ikut terbawa.
- Runner up: evaluator plan sendiri yang lebih ringan. Pro: jalur create tidak disentuh. Kontra: dua set aturan yang pasti berbeda seiring waktu, persis yang AC-020 ingin cegah.

### K10: perbedaan manifest dengan instance yang sudah ada (`berat`)

- **Diambil: dilaporkan (`differences`), tidak pernah diterapkan, tidak memblokir.** "Instance yang sudah ada dan sehat tidak dihapus" (fitur 46) dan aturan repo bahwa port hanya berubah lewat `instance.change_endpoints` dan runtime lewat rebind yang dicek kompatibilitas datanya. Mengubah data lewat edit YAML terlalu berbahaya untuk v1alpha1.
- Runner up: plan mengusulkan aksi `change`. Pro: manifest benar benar deklaratif. Kontra: butuh aturan migrasi data, restart, dan rollback per provider; bukan untuk baseline.

### K11: secret literal (`biasa`)

- **Diambil: aturan struktur.** Secret hanya lewat `secretRefs`, nilainya harus reference `local:`, dan secret field di `settings` ditolak. Tidak ada tebak tebakan dari nama key. Pesan tidak pernah mengulang nilai.
- Runner up: heuristik (key bernama `password`, entropi tinggi). Kontra: positif palsu dan negatif palsu, dan settings schema paket sudah menolak key tak dikenal.

### K12: secret field tanpa `secretRefs` (`biasa`)

- **Diambil: dibangkitkan lokal saat create.** Sama dengan default create (spec 0011 AC-6); tim tidak wajib menulis reference untuk setiap password. Runner up: wajib ditulis. Kontra: manifest minimal jadi panjang, dan Up pertama selalu berhenti di needs secret.

### K13: hasil plan saat ada blocker (`biasa`)

- **Diambil: balasan sukses dengan `applicable` false dan daftar blocker; CLI keluar 3.** Pengguna melihat seluruh plan dan semua masalah sekaligus. Runner up: error pertama saja. Kontra: perbaiki satu, jalankan lagi, ketemu berikutnya.

### K14: reference yang belum terisi tidak memblokir plan (`biasa`)

- **Diambil: status di item, `needsSecrets`, `applicable` tetap.** Fitur 46 menyatakan Up berhenti dengan status needs secret; plan cukup jujur menunjukkannya. Runner up: jadikan blocker. Kontra: bertabrakan dengan alur needs secret milik spec 0046.

### K15: tanpa `EventKind` baru (`biasa`)

- **Diambil: klien memuat ulang pada event operasi dan instance.** Minor protokol saja. Runner up: `environment.changed` (major 5.0). Spec 0046 dan 0047 lebih tahu event apa yang dibutuhkan Up dan lease; kalau perlu, mereka menambahnya sekali.

### K16: edit manifest (`biasa`)

- **Diambil: file di disk adalah sumbernya; app membuka editor bawaan dan mendeteksi perubahan.** File itu hidup di repo tim. Runner up: editor YAML di app. Kontra: menyimpan lewat daemon ke repo pengguna, konflik dengan editor dan git, dan komponen editor baru.

### K17: sidebar (`biasa`)

- **Diambil: baris anak per environment, sesuai mockup, dengan baris Environments tetap membuka halaman daftar.** Mockup tidak memberi baris induk halaman sendiri; halaman daftar dibutuhkan untuk state kosong dan Import.

### K18 sampai K26: keputusan kecil (`biasa`)

| K | Keputusan | Runner up |
| --- | --- | --- |
| K18 | nama instance dedicated `<name>-<alias>` dengan override `instanceName` | selalu wajib ditulis |
| K19 | versi tanpa lock selalu hasil `runtime::resolve` catalog saat itu | pakai runtime terpasang yang cocok line (hasil plan jadi bergantung riwayat Mac; itu tugas lockfile) |
| K20 | awalan method dan kode `env`, `ResourceKind` tetap `environment` | `environment.*` (panjang, beda dengan grup CLI) |
| K21 | JSON Schema di `schemas/environment/` lewat codegen, pola provider | ditulis tangan |
| K22 | instance dedicated dibuat `startup` `manual`; manifest tanpa field autostart | field `autostart` (bertabrakan dengan lease environment) |
| K23 | `planDigest` untuk mengikat Up ke plan yang ditinjau | Up menghitung ulang tanpa cek (plan yang ditinjau bisa berbeda dari yang dijalankan) |
| K24 | `storage` hanya `managed` dan `persistent: true` | buang field (contoh PRD tidak valid) |
| K25 | batas 32 service, 64 environment, manifest 256 KiB | tanpa batas (anggaran frame tidak terjaga) |
| K26 | Follow-up spec 0026 (restart instance yang dipinjam) diserahkan ke spec 0047 | putuskan di sini (aturannya bergantung override stop spec 0047) |

## Rationale

Tiga gaya membentuk keputusan. Pertama, fitur ini fondasi tiga spec lain, jadi pilihan yang membuat aturan benar secara struktur dipilih di atas cek saat jalan: service shared tidak punya field konfigurasi, secret tidak punya field nilai, manifest tidak punya field trust. Kedua, AC-020 menuntut plan tanpa efek samping dan setara GUI dan CLI, jadi plan adalah method baca di daemon yang memakai ulang evaluasi create; tidak ada evaluator kedua dan tidak ada logika di klien. Ketiga, data pengguna tidak boleh berubah karena edit file: perbedaan hanya dilaporkan, remove hanya menghapus pendaftaran, dan import tidak menjalankan apa pun.

Identitas lewat nama yang terikat lokasi adalah titik tengah antara dua kegagalan yang nyata: identitas lokasi yang putus saat folder dipindah, dan identitas nama bebas yang membuat dua proyek saling menimpa. `relink` yang eksplisit menjaga pemindahan folder tetap satu klik tanpa membuka pintu penimpaan diam diam.

Snapshot di registry dipilih karena daftar environment dibaca jauh lebih sering daripada manifest berubah, dan karena daemon tidak semestinya membaca path sembarang di jalur baca yang sering. Biayanya, dua sumber yang bisa berbeda, dibayar dengan `manifestState` yang selalu tampil.

Dua method yang menulis (import dan remove) melebar dari wilayah "read only" yang ditulis aturan umum koordinasi. Alasannya mockup dan JOURNEY-04: environment harus ada sebelum Up. Keduanya hanya menulis registry dan memakai mesin operasi yang ada.

## Koordinasi dengan sesi paralel

- Spec 0045, 0046, dan 0047 ditulis sesudah spec ini di merge. Bagian *Titik sambung* di `index.md` adalah kontrak untuk mereka; perubahan terhadapnya dikirim ke architect spec ini atau ke lead.
- File satu pemilik saat build (`crates/protocol/`, `protocols/`, `schemas/`, hasil codegen, migrasi) hanya disentuh langkah 1, 2, dan 4.

## Cross check

Dijalankan 2026-11-13 oleh model lain (Opus) sebagai pembaca saja, sesuai aturan umum koordinasi. Ia menemukan 25 butir; mode `auto` menerapkan perbaikan yang direkomendasikan, semuanya sebelum spec diajukan ke lead:

- **Salah fakta, diperbaiki**: contoh PRD 13.1 tidak cocok dengan nama endpoint, secret field, setting, dan line paket resmi (kini fixture `erp-development.yaml` dengan tabelnya); deteksi runtime terpasang memakai `artifact_sha256` dan state `installed`, bukan `artifact_id`; port member environment sendiri tetap dihitung bentrok; helper path CLI; kode `resource.revision_mismatch` yang sudah ada menggantikan `env.changed`.
- **Celah keputusan, ditutup**: `CreateDraft` tidak diubah (fakta `planned_runtime` di `gather`); baris secret `Bind` yang belum terisi ditulis ulang menjadi `caution` dengan pemetaan status; settings divalidasi lewat helper yang sama dengan create; pesan finding dari template tetap (bukan `Redactor`); setiap reference yang di bind ditampilkan; cache keanggotaan untuk `InstanceSummary` di event dan doctor; `GuardView::environments()`; resource dan dua lock operasi import; relink lewat id dengan lokasi unik dan canonicalize folder induk; tabel lifecycle ke aksi dan blocker instance; aturan `differences`; `PlanEndpoint` dan `steps`; urutan `manifestState`; `env.get` dengan `checkManifest` supaya event instance tidak membaca file.
- **Tidak diambil**: `env.validate` dibiarkan `recovery refused` (ia butuh catalog, dan mode recovery bukan tempat memeriksa manifest); menandai reference yang "tidak dipakai service lain" tidak ditambahkan, cukup setiap reference yang di bind ditampilkan.

## Perubahan 2026-11-13 (sesudah spec 0045 dan 0046 diterima)

Mode Update, tanpa cross check, atas permintaan lead. ID AC tidak berubah.

- **`planDigest` tanpa aksi turunan keadaan instance** (usulan spec 0046, mengubah K23). Digest tadinya memuat `action` dan `steps` setiap item. Untuk instance yang sudah ada keduanya turunan lifecycle, dan Up menurunkannya lagi saat giliran service tiba, jadi instance yang berhenti atau menyala di antara plan dan Bring Up membuat Up ditolak `env.plan_changed` tanpa ada yang benar benar berubah. Kini item instance yang sudah ada hanya menyumbang `alias`, jenisnya, dan identitas instance. `endpoints` item itu ikut dikeluarkan dengan alasan yang sama: port efektif endpoint `auto` berganti saat instance di start ulang. Itu satu langkah lebih jauh dari teks usulan spec 0046 (yang hanya menyebut `action` dan `steps`), diambil karena tujuan yang sama; runner up nya mempertahankan `endpoints` di digest, dengan akibat `env.plan_changed` saat port `auto` berganti. Yang tetap terikat: manifest, pendaftaran, instance mana yang dipakai, dan seluruh isi item `create`.
- **Instance yang belum initialized menjadi blocker plan** (usulan spec 0046). `start_instance` selalu menolak instance `uninitialized`, dan instance `stopped` atau `failed` tanpa data, jadi plan yang menandainya `start` menjanjikan sesuatu yang pasti gagal. Kini keduanya blocker `env.instance_blocked` dengan `reason` `not_initialized`, seperti lifecycle `unknown` dan `retained`. `NotReadyReason` di `env.get` tidak diubah (tetap `stopped`), supaya tabel *Alasan belum ready* spec 0046 tidak ikut bergeser. Runner up: biarkan `start` dan biarkan Up melaporkan gagal di tahap start; ditolak karena kegagalannya sudah bisa dipastikan sebelum ada mutasi.
- **Titik sambung diselaraskan** dengan nama field, kode, dan aturan final spec 0045 dan 0046. Tidak ada keputusan baru; isi build spec ini tidak bertambah (field itu dibangun spec 0045 dan 0046 dengan minor protokol masing masing).

## Bahan yang dibaca

- `prd.md`: REQ-020, REQ-028, VAL-013, VAL-014, PAGE-10, AC-020, AC-080, NFR-012, JOURNEY-04, bagian 13.1 sampai 13.3 (hanya sebagai input spec).
- `docs/scope/08-environment-proyek.md` dan aturan umum koordinasi scope 08.
- Spec 0004, 0005, 0006, 0007, 0008, 0009, 0011, 0012, 0013, 0016, 0017, 0018, 0020, 0022, 0026, 0029, 0031, 0032, 0037, 0068, 0070 (bagian yang dirujuk).
- Kode: `crates/domain/src/environment.rs`, `lease.rs`; `crates/core/src/instance/create/`; `crates/core/src/runtime/resolve.rs`; `crates/core/src/catalog/trust.rs`; `crates/core/src/provider/`; `crates/storage/src/registry/` dan migrasi `0001`; `crates/protocol/src/` (`message.rs`, `instance.rs`, `provider.rs`, `codes.rs`); `apps/cli/src/commands/provider/`; `apps/macos/Packages/StacklyUI/Sources/StacklyUI/` (Shell, Connection, State, Status, OpenIn, Catalog, Overview, Instances).
- Mockup `docs/mockups/Stackly.dc.html` dan `stackly-data.js` (halaman Environments, sidebar, sheet plan, seksi Overview).
