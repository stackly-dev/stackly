# 0022. Overview dan daftar instance: rationale

[Kembali ke spec](index.md)

## Context

Fitur 22 di `docs/scope/02-instalasi-pertama.md` membawa dua halaman yang dilihat pengguna setiap kali membuka Stackly: Overview (PAGE-02) dan Instances (PAGE-03), plus `stackly instance list` dan `inspect` untuk paritas CLI (REQ-028). Done when meminta kedua halaman menangani loading, empty, error, stale, dan partial, error di satu instance tidak mengosongkan layar, ringkasan tidak dihitung dari PID saja, peringatan kapasitas memakai sumber yang terdefinisi (ruang disk) atau tampil unavailable sampai fitur 28, layar bisa dipakai dengan keyboard dan VoiceOver, dan tampilan mengikuti mockup (Needs Attention yang menautkan ke layar perbaikan, filter status, tabel yang bisa diurutkan, inspector yang bisa disembunyikan, dan hasil filter kosong).

Kontrak yang tersedia sudah banyak tetapi belum lengkap. Spec 0010 memberi `InstanceSummary` dengan lifecycle, desired state, readiness, health, proses, restart, blocker, dan endpoint sebagai field terpisah, `instance.list` berhalaman, `instance.get`, start dan stop, serta event berurutan lewat `statusSeq`. Spec 0013 memberi `InstanceStore`, `OperationCenter`, `ResourceStore`, jalur mutasi, gate, dan aturan presence saat daemon mati, plus daftar instance sementara dan `UnbuiltPage` Overview yang harus diganti fitur ini. Spec 0012 memberi palet status, `ViewState`, dan pola banner. Spec 0017 dan 0018 memberi nama tampil dan tile provider serta store runtime. Yang tidak ada: field ownership di summary (registry punya kolom `instances.ownership`, tetapi artinya asal data: `created`, kelak `restored` dan `adopted`), referensi environment (fitur 44 belum ada), ukuran dan path data, method apa pun yang melaporkan ruang disk ke client, judul operasi yang bisa dibaca manusia, dan perintah CLI `instance list` atau `inspect` (spec 0010 hanya mengasumsikan fitur 14 membangun `get`).

Batasan lain: GUI tidak pernah menghitung status sendiri dari sumber selain DTO daemon, tidak pernah membaca filesystem, dan semua mutasi lewat daemon; status set closed untuk lifecycle, readiness, dan health; target NFR-002 adalah snapshot awal paling lama 2 detik untuk 100 instance tanpa memblokir main thread; metrics (fitur 28) belum ada, sehingga kapasitas memori dan CPU belum punya sumber. Fitur 20 (create), 21 (detail dan start), 26 (restart, force stop), 44 (environment), dan 67 (umpan balik) didesain atau dibangun belakangan, jadi desain ini tidak boleh menunggu mereka tetapi harus menyisakan titik sambung yang jelas. Spec 0010 masih dibangun program `01-foundation`, sehingga perluasannya ditulis sebagai usulan additive.

## Keputusan

### Keputusan 1: apa yang dihitung kartu Running

- **Opsi 1 (dipilih): kartu Running menghitung `ready` (lifecycle `running` dan readiness `ready`), dengan keterangan "{n} not ready" untuk yang hidup tetapi belum siap.** Pro: memenuhi PAGE-02 ("ringkasan ready", "tidak hanya menghitung PID") secara harfiah, setiap instance tetap terhitung (keterangan dan subjudul), dan label "Running" tetap sama dengan arti label hijau "Running" di tabel status spec 0012. Kontra: angka kartu tidak sama dengan jumlah baris filter Running kecuali keterangan ikut dibaca; menyimpang sedikit dari mockup.
- **Opsi 2: kartu menghitung semua lifecycle aktif, seperti mockup yang menggabungkan starting.** Pro: angka kartu sama dengan baris filter. Kontra: instance yang crash loop di `recovering` atau readiness nya gagal ikut dihitung Running, persis yang dilarang PAGE-02.
- **Opsi 3: kartu bernama Ready dan filter Ready hanya yang siap.** Pro: konsisten penuh. Kontra: mengubah copy mockup, dan instance yang sedang starting hanya terlihat di All.

Opsi 1 dipilih karena kejujuran angka utama adalah inti PAGE-02 dan Done when, sementara selisih dengan baris filter dijelaskan oleh keterangan yang terlihat dan dibacakan VoiceOver.

### Keputusan 2: di mana aturan kelompok dihitung

- **Opsi 1 (dipilih): fungsi murni di Swift dan di CLI Rust, dijaga fixture bersama `groups.json`.** Pro: tidak ada perubahan wire; event `instance.health_changed` yang mengubah readiness langsung mengubah kelompok di GUI tanpa field turunan yang harus ikut dikirim; pola fixture bersama sudah dipakai repo ini (path socket, endpoint). Kontra: dua implementasi.
- **Opsi 2: daemon mengirim field `group` di `InstanceSummary`.** Pro: satu implementasi. Kontra: field turunan harus ikut di `instance.health_changed` dan diurutkan `statusSeq`, mengubah spec 0010 lebih dalam, dan menyimpan nilai turunan yang bisa basi.
- **Opsi 3: hanya GUI yang punya kelompok; CLI menulis lifecycle mentah tanpa filter atau jumlah.** Pro: paling sedikit kode. Kontra: CLI tidak setara dengan GUI untuk filter dan jumlah (REQ-028).

Opsi 1 dipilih karena spec 0012 dan 0013 sudah menetapkan bahwa tampilan status adalah fungsi murni di client atas enum tertutup, dan fixture bersama membuat dua implementasi tidak bisa menyimpang tanpa test gagal.

### Keputusan 3: sumber Needs Attention

- **Opsi 1 (dipilih): fungsi murni di client atas store yang sudah disinkronkan (`InstanceStore`, `OperationCenter`, `CapacityModel`, `DaemonHealth`).** Pro: tidak ada method atau event baru untuk perhatian, daftar diperbarui seketika lewat event yang sudah ada, dan fitur lain menambah aturan cukup dengan baris tabel baru. Kontra: aturan tinggal di Swift saja, jadi CLI tidak punya padanan Needs Attention.
- **Opsi 2: method daemon `overview.get_attention`.** Pro: satu aturan untuk GUI dan CLI. Kontra: harus dipanggil ulang atau punya event sendiri, menduplikasi data yang sudah ada di store, dan copy UI (bahasa Inggris, lokal app) akan berpindah ke daemon.

Opsi 1 dipilih. CLI sudah menampilkan semua bahan mentahnya (`list` dan `inspect`), dan Done when hanya meminta paritas untuk `list` dan `inspect`.

### Keputusan 4: sumber peringatan kapasitas

- **Opsi 1 (dipilih): method baru `storage.get_capacity` yang membaca `statfs` untuk root data, logs, dan cache, dengan level dihitung daemon.** Pro: sumber terdefinisi yang diminta Done when, tersedia sekarang tanpa fitur 28, aturan ambang di satu tempat (pola spec 0017: status dihitung daemon), aman di recovery mode, dan murah (satu syscall per root). Kontra: method dan area baru, dan GUI harus polling karena tidak ada event.
- **Opsi 2: field kapasitas di `DaemonHealth` yang sudah di heartbeat setiap 15 detik.** Pro: tanpa polling tambahan. Kontra: mengubah DTO milik spec 0006 untuk sesuatu yang bukan kesehatan daemon, dan `statfs` berjalan setiap heartbeat walau Overview tidak tampil.
- **Opsi 3: tampil unavailable sampai fitur 28.** Pro: tanpa kerja. Kontra: pengguna tidak diperingatkan sebelum database gagal menulis, padahal sumbernya mudah.

Opsi 1 dipilih. Polling 60 detik hanya saat Overview tampil cukup untuk perubahan ruang disk yang lambat.

### Keputusan 5: ambang kapasitas

- **Opsi 1 (dipilih): `critical` di bawah 2 GB tersedia, `low` di bawah 10 GB atau terpakai 90 persen, `filling` terpakai 80 persen.** Pro: `filling` sama dengan contoh mockup (82 persen sebagai info), batas absolut melindungi disk besar yang persentasenya tampak aman tetapi sisa sedikit, dan batas 2 GB berada di atas cadangan 64 MiB yang dipakai install runtime spec 0018. Kontra: angka dipilih tanpa data pemakaian nyata.
- **Opsi 2: satu ambang persentase saja.** Pro: sederhana. Kontra: disk 4 TB pada 95 persen masih punya 200 GB, sedangkan disk 256 GB pada 85 persen hanya 38 GB.

Konstanta ada di domain supaya mudah disetel ulang lewat update spec.

### Keputusan 6: ownership di daftar instance

- **Opsi 1 (dipilih): field wire `InstanceSummary.ownership`, enum terbuka bertag dengan `manual`, diisi daemon sampai fitur 44 menambah keanggotaan environment.** Pro: arti mockup ("Manual", "Dedicated · lease", "Shared · lease") terpenuhi, JSON CLI membawa field yang sama, dan fitur 44 cukup menambah varian dan mengganti derivasi. Kontra: field dengan satu nilai untuk sementara, dan namanya mirip kolom registry `instances.ownership` yang artinya asal data.
- **Opsi 2: teks "Manual" tetap di GUI tanpa field wire.** Pro: tanpa perubahan protokol. Kontra: nilai yang tidak berasal dari daemon, CLI tidak bisa menampilkannya di JSON, dan fitur 44 harus menambah field juga.
- **Opsi 3: kirim kolom registry `instances.ownership` (`created`).** Pro: sudah ada di registry. Kontra: artinya asal data, bukan kepemilikan environment yang dimaksud mockup dan PAGE-03 bersama "environment reference".

Opsi 1 dipilih; doc comment dan *Kepemilikan* membedakan dua arti itu secara eksplisit.

### Keputusan 7: kolom dan seksi Environment

- **Opsi 1 (dipilih): tidak dirender sampai fitur 44.** Pro: tidak ada kolom yang selalu berisi "—", dan tidak ada field spekulatif. Kontra: selisih dengan mockup yang harus dicatat.
- **Opsi 2: kolom tetap ada dengan "—".** Pro: sama dengan mockup. Kontra: kolom tanpa informasi memakan lebar di 640 pt dan menyiratkan fitur yang belum ada.

### Keputusan 8: aksi Start, Stop, dan Retry di fitur ini

- **Opsi 1 (dipilih): dibangun di sini lewat `InstanceActionModel` yang dipakai ulang fitur 21, 26, dan 32.** Pro: mockup dan peta layar `ui-macos.md` menaruh aksi di Overview dan Instances milik fitur 22; method spec 0010 sudah ada; satu model menghindari dua implementasi aturan tombol. Kontra: fitur ini ikut menangani umpan balik dan status menunggu.
- **Opsi 2: halaman hanya baca sampai fitur 21.** Pro: lebih kecil. Kontra: tidak mengikuti mockup, dan alur keyboard "menghentikan instance" di PRD AC-027 tidak bisa dibuktikan dari daftar.

### Keputusan 9: "buka instance" sebelum fitur 21

- **Opsi 1 (dipilih): memilih baris di Instances dan menampilkan inspector (`// provisional: F21`).** Pro: setiap item Needs Attention punya tujuan yang menampilkan detail dan aksi perbaikan yang ada hari ini. Kontra: bukan halaman detail mockup.
- **Opsi 2: item instance tanpa tautan sampai fitur 21.** Kontra: Done when meminta Needs Attention yang menautkan ke layar perbaikan.

### Keputusan 10: umpan balik aksi sebelum fitur 67

- **Opsi 1 (dipilih): `ActionFeedbackBanner` sementara di atas konten halaman, memakai `StateBanner` spec 0012 dan copy spec 0013.** Pro: penolakan daemon tidak pernah hilang diam diam, diumumkan VoiceOver, dan mudah diganti. Kontra: kode sementara.
- **Opsi 2: menunggu fitur 67.** Kontra: tombol aksi tanpa umpan balik kegagalan melanggar spec 0012 (error per item terlihat).

### Keputusan 11: halaman di `stackly instance list`

- **Opsi 1 (dipilih): ambil semua halaman.** Pro: jumlah dan `--status` dihitung atas semua instance, sama dengan GUI yang juga memuat semua halaman; jumlah instance terbatas (baseline 100). Kontra: berbeda dengan pola `operation list` spec 0008.
- **Opsi 2: `--limit` dan `--before` seperti `operation list`.** Pro: konsisten dengan spec 0008. Kontra: jumlah dan filter per halaman menyesatkan, dan riwayat operasi yang tidak terbatas adalah alasan pola itu, bukan instance.

### Keputusan 12: `inspect` dan nama instance di CLI

- **Opsi 1 (dipilih): `inspect` kanonik (sama dengan `operation inspect` dan `provider inspect`), `get` menjadi alias kalau fitur 14 sudah menambahkannya, dan nama dicari CLI lewat `instance.list`.** Pro: tidak ada perubahan method daemon, dan pengguna boleh mengetik nama. Kontra: mencari nama butuh membaca semua halaman.
- **Opsi 2: tambah params `name` di `instance.get`.** Pro: satu call. Kontra: mengubah params method spec 0010 yang sedang dibangun, untuk kebutuhan yang bisa dipenuhi client.

### Keputusan 13: instance yang tampil di Overview

- **Opsi 1 (dipilih): enam pertama urut nama, dengan Show All.** Pro: deterministik, sama dengan urutan default Instances, mudah dites. Kontra: instance bermasalah bisa tidak terlihat di seksi ini (tetapi selalu ada di Needs Attention).
- **Opsi 2: urut berdasarkan masalah lebih dulu.** Kontra: menduplikasi Needs Attention dan urutan berubah ubah saat status berubah.
- **Opsi 3: yang terakhir berubah.** Kontra: tidak ada sumber waktu perubahan yang stabil di summary.

### Keputusan 14: arti partial

- **Opsi 1 (dipilih): di halaman, partial berarti ada refetch item yang gagal (`ResourceStore.itemFailures`); di Overview, setiap seksi juga gagal sendiri sendiri.** Pro: sesuai pola partial spec 0012 (item yang belum berhasil beserta alasannya dan aksi Retry), dan memenuhi "error di satu instance tidak membuat seluruh layar kosong". Kontra: menambah state per item di `ResourceStore` milik spec 0013.
- **Opsi 2: decode lossy per item di halaman `instance.list`.** Pro: menangani item yang tidak bisa dibaca. Kontra: dengan enum tertutup dan aturan versi protokol, item yang gagal decode berarti bug, bukan keadaan yang wajar; butuh decoder tulisan tangan di atas tipe hasil generate.

### Keputusan 15: navigasi antar halaman

- **Opsi 1 (dipilih): `ShellNavigator` per jendela dengan `ShellDestination` dan permintaan sekali pakai untuk filter dan pilihan.** Pro: Overview bisa membuka Instances dengan filter dan pilihan, dan halaman lain (Runtimes, Catalog) memakai pintu yang sama. Kontra: tambahan kecil di shell spec 0013.
- **Opsi 2: setiap halaman menulis `@SceneStorage("shell.section")` langsung.** Kontra: filter dan pilihan tidak bisa ikut dibawa.

### Keputusan 16: pengurutan lewat keyboard

- **Opsi 1 (dipilih): perintah menu View "Sort Instances By" yang hanya aktif di Instances, dipindah ke peta fitur 69 saat ada.** Pro: header `Table` macOS tidak bisa difokus dengan Tab, sedangkan menu bar selalu bisa dicapai keyboard. Kontra: kode perintah sebelum fitur 69.
- **Opsi 2: hanya header tabel.** Kontra: pengguna keyboard tanpa VoiceOver tidak bisa mengurutkan (NFR-012).

### Keputusan 17: nama tampil dan tile provider

- **Opsi 1 (dipilih): join di client dengan `CatalogStore` spec 0017, fallback `software`.** Pro: tile dan mark memang hanya ada di catalog, jadi satu join memberi keduanya; tanpa perubahan wire. Kontra: provider di luar catalog (fake di test, impor lokal fitur 37) tampil dengan nama `software`.
- **Opsi 2: field `provider.displayName` di summary dari manifest paket pinned.** Pro: nama selalu ada. Kontra: daemon harus membaca atau menyimpan cache paket untuk setiap list, dan tile tetap butuh catalog.

## Rationale

Spec ini memilih menghitung semua yang terlihat di client dari DTO yang sudah disinkronkan, karena spec 0013 sudah membayar biaya sinkronisasi snapshot plus cursor dan event berurutan, dan setiap method atau event baru untuk ringkasan hanya akan menduplikasi data itu. Pengecualiannya kapasitas: tidak ada DTO yang memuatnya, dan GUI tidak boleh membaca filesystem, jadi daemon mendapat satu method baca yang kecil dan menghitung level nya sendiri, sama dengan prinsip spec 0017 bahwa status dihitung di daemon.

Kejujuran angka menentukan banyak pilihan kecil: kartu Running hanya menghitung yang siap, angka tidak tampil sebelum snapshot, koneksi yang putus tidak mengubah apa pun, dan setiap instance terhitung di tepat satu kelompok. Paritas CLI dijaga dengan cara yang sudah terbukti di repo ini (fixture bersama dan golden), bukan dengan memindahkan presentasi ke daemon.

Titik sambung untuk fitur berikutnya (20, 21, 26, 29, 33, 35, 44, 67, 69) ditulis sebagai penanda `provisional` atau API yang dipakai ulang, supaya fitur ini bisa dibangun sekarang tanpa menunggu mereka dan tanpa menebak bentuk mereka.

## Keputusan kecil

- InstanceStore memakai `limit` 200 supaya 100 instance termuat dalam satu call (NFR-002).
- Polling kapasitas 60 detik, hanya saat Overview tampil dan tersambung, ditambah sekali setelah setiap resnapshot.
- Needs Attention menampilkan enam item sebelum Show More, sama dengan jumlah baris instance di seksi Instances dan dengan contoh mockup (empat item).
- Readiness `unknown` dengan reason `daemon_restarted` tidak menjadi item, karena setiap instance berjalan melewatinya beberapa detik setelah daemon restart.
- `I10` (tidak bisa diadopsi lagi) bertone neutral, karena instance tetap berjalan normal sampai daemon restart.
- Seksi Operations dan judul operasi tidak menampilkan persen untuk jenis selain install, karena hanya spec 0018 yang mendefinisikan persen keseluruhan.
- Copy Name dan Copy ID di menu konteks tidak digate, karena tidak mengubah data.
- Inspector tampil secara default, sama dengan state awal mockup.

## Cross check

Belum dijalankan.
