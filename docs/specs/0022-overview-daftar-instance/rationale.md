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

- **Opsi 1 (dipilih): fungsi murni di client atas store yang sudah disinkronkan (`InstanceStore`, `OperationCenter`, `RuntimeStore`, `CapacityModel`).** Pro: tidak ada method atau event baru untuk perhatian, daftar diperbarui seketika lewat event yang sudah ada, dan fitur lain menambah aturan cukup dengan baris tabel baru. Kontra: aturan tinggal di Swift saja, jadi CLI tidak punya padanan Needs Attention. Status subsystem daemon sengaja tidak menjadi item (cross check): footer dan Diagnostics sudah menampilkannya, dan subsystem yang degraded hanya mengulang item operasi dan instance.
- **Opsi 2: method daemon `overview.get_attention`.** Pro: satu aturan untuk GUI dan CLI. Kontra: harus dipanggil ulang atau punya event sendiri, menduplikasi data yang sudah ada di store, dan copy UI (bahasa Inggris, lokal app) akan berpindah ke daemon.

Opsi 1 dipilih. CLI sudah menampilkan semua bahan mentahnya (`list` dan `inspect`), dan Done when hanya meminta paritas untuk `list` dan `inspect`.

### Keputusan 4: sumber peringatan kapasitas

- **Opsi 1 (dipilih): method baru `storage.get_capacity` yang membaca `statfs` untuk volume root data, dengan level dihitung daemon.** Pro: sumber terdefinisi yang diminta Done when, tersedia sekarang tanpa fitur 28, aturan ambang di satu tempat (pola spec 0017: status dihitung daemon), aman di recovery mode, dan murah (satu syscall). Kontra: method, area, dan kode error baru, dan GUI harus polling karena tidak ada event. Draft awal mengukur root data, logs, dan cache dan mengelompokkannya per volume; cross check menunjukkan ketiganya selalu di volume yang sama di produksi dan di bawah `STACKLY_ROOT`, jadi bentuknya disederhanakan ke satu volume dengan satu kode error, dan root lain bisa ditambah secara additive. Pembacaan dibatasi single flight dengan timeout 5 detik supaya mount yang menggantung tidak menumpuk thread.
- **Opsi 2: field kapasitas di `DaemonHealth` yang sudah di heartbeat setiap 15 detik.** Pro: tanpa polling tambahan. Kontra: mengubah DTO milik spec 0006 untuk sesuatu yang bukan kesehatan daemon, dan `statfs` berjalan setiap heartbeat walau Overview tidak tampil.
- **Opsi 3: tampil unavailable sampai fitur 28.** Pro: tanpa kerja. Kontra: pengguna tidak diperingatkan sebelum database gagal menulis, padahal sumbernya mudah.

Opsi 1 dipilih. Polling 60 detik hanya saat Overview tampil cukup untuk perubahan ruang disk yang lambat.

### Keputusan 5: ambang kapasitas

- **Opsi 1 (dipilih): `critical` di bawah 2 GB tersedia, `low` di bawah 5 GB atau terpakai 90 persen, `filling` terpakai 80 persen.** Pro: `filling` sama dengan contoh mockup (82 persen sebagai info), batas absolut melindungi disk besar yang persentasenya tampak aman tetapi sisa sedikit, dan batas 2 GB berada di atas cadangan 64 MiB yang dipakai install runtime spec 0018. Kontra: angka dipilih tanpa data pemakaian nyata. Draft awal memakai 10 GB untuk `low`; karena `f_bavail` di APFS tidak menghitung ruang purgeable, angka itu bisa memperingatkan Mac yang sehat, jadi diturunkan ke 5 GB dan copy memakai "about".
- **Opsi 2: satu ambang persentase saja.** Pro: sederhana. Kontra: disk 4 TB pada 95 persen masih punya 200 GB, sedangkan disk 256 GB pada 85 persen hanya 38 GB.

Konstanta ada di domain supaya mudah disetel ulang lewat update spec.

### Keputusan 6: ownership di daftar instance

- **Opsi 1 (dipilih): field wire `InstanceSummary.ownership`, enum terbuka tanpa data (`manual`, `unrecognized`) dengan `#[serde(default)]` bernilai `manual`, diisi daemon sampai fitur 44 menambah keanggotaan environment.** Pro: arti mockup ("Manual", "Dedicated · lease", "Shared · lease") terpenuhi, JSON CLI membawa field yang sama, field opsional sesuai aturan minor spec 0006 sehingga daemon lama tetap terbaca, dan fitur 44 cukup menambah nilai enum dan field saudara untuk environment pemilik. Kontra: field dengan satu nilai untuk sementara, dan namanya mirip kolom registry `instances.ownership` yang artinya asal data. Draft awal memakai enum bertag dengan data; cross check menunjukkan spec 0006 hanya mengizinkan enum terbuka tanpa data.
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

- **Opsi 1 (dipilih): ambil semua halaman.** Pro: jumlah dan `--status` dihitung atas semua instance, sama dengan GUI yang juga memuat semua halaman; jumlah instance terbatas (baseline 100); `stackly runtime list` spec 0018 memakai pola yang sama untuk resource yang juga terbatas. Kontra: berbeda dengan pola `operation list` spec 0008.
- **Opsi 2: `--limit` dan `--before` seperti `operation list`.** Pro: konsisten dengan spec 0008. Kontra: jumlah dan filter per halaman menyesatkan, dan riwayat operasi yang tidak terbatas adalah alasan pola itu, bukan instance.

### Keputusan 12: `inspect` dan nama instance di CLI

- **Opsi 1 (dipilih): `inspect` kanonik (sama dengan `operation inspect` dan `provider inspect`), `get` menjadi alias kalau fitur 14 sudah menambahkannya, dan nama dicari CLI lewat `instance.list`.** Pro: tidak ada perubahan method daemon, dan pengguna boleh mengetik nama. Kontra: mencari nama butuh membaca semua halaman.
- **Opsi 2: tambah params `name` di `instance.get`.** Pro: satu call. Kontra: mengubah params method spec 0010 yang sedang dibangun, untuk kebutuhan yang bisa dipenuhi client.

### Keputusan 13: instance yang tampil di Overview

- **Opsi 1 (dipilih): enam pertama urut nama, dengan Show All.** Pro: deterministik, sama dengan urutan default Instances, mudah dites. Kontra: instance bermasalah bisa tidak terlihat di seksi ini (tetapi selalu ada di Needs Attention).
- **Opsi 2: urut berdasarkan masalah lebih dulu.** Kontra: menduplikasi Needs Attention dan urutan berubah ubah saat status berubah.
- **Opsi 3: yang terakhir berubah.** Kontra: tidak ada sumber waktu perubahan yang stabil di summary.

### Keputusan 14: arti partial

- **Opsi 1 (dipilih): di halaman, partial berarti ada refetch item yang gagal (`ResourceStore.itemFailures`, berisi error dan revision supaya Retry tahu revision yang diminta, dan baris itu memakai detail "Refresh failed"); di Overview, setiap seksi juga gagal sendiri sendiri.** Pro: sesuai pola partial spec 0012 (item yang belum berhasil beserta alasannya dan aksi Retry), dan memenuhi "error di satu instance tidak membuat seluruh layar kosong". Kontra: menambah state per item di `ResourceStore` milik spec 0013.
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
- Inspector tampil secara default, sama dengan state awal mockup; kolom Version dan Endpoints mengalah di bawah 760 pt supaya Name dan Status tetap terlihat di 640 pt.
- Dalam satu tone, item disk diurutkan sebelum operasi dan instance, supaya disk critical tidak tersembunyi di balik banyak instance gagal.
- `InstanceSummary.initialized` ditambahkan karena instance `failed` yang belum pernah terinisialisasi ditolak `supervisor.not_initialized` saat start; tanpa field itu tombol Retry akan selalu gagal (masukan arc-f20-spec). Fitur 20 tidak mengusulkan field yang setara.
- Key `outcomeUnknown` hanya dipakai ulang selama `revision` summary belum berubah dan paling lama 5 menit, supaya klik yang jauh kemudian tidak memutar ulang reply lama tanpa efek.

## Cross check

Dijalankan 2026-10-20 dengan model lain (Sonnet), read only, terhadap draft `728b5c0`, termasuk membaca kode `StacklyUI` yang sudah dibangun. Hasilnya 2 blocker, 16 gap, dan 7 catatan minor; semua rekomendasi disetujui lead atas nama user dan diterapkan, kecuali satu saran yang ditolak atas rekomendasi architect.

- **Diterapkan, kontrak**: `ResourceStore` yang sudah dibangun membuang `.patch` dengan revision sama, padahal `instance.health_changed` diterapkan lewat `statusSeq` (spec 0013); lead foundation menetapkan aturan itu milik spec 0013 milestone 4 langkah 16, jadi milestone 3 spec ini menunggu langkah itu tanpa jalur cadangan. `ownership` menjadi enum terbuka tanpa data dengan default (aturan spec 0006). `initialized` ditambahkan (masukan arc-f20-spec). Instance retained disaring di `InstanceStore` dari semua jalur. Fixture wire pindah ke `protocols/fixtures/ipc/` dengan `index.json`.
- **Diterapkan, kelengkapan keputusan**: status tombol aksi (`inFlight` sejak klik, `pending` dibersihkan juga oleh resnapshot dengan `operation.get`, umur key `outcomeUnknown`); isi O1 menurut `recovery.hint` dan `auto_compensate` tanpa item; aturan subsystem daemon dihapus; signature `AttentionRules.items` dengan `runtimes`, `now`, `locale`, `timeZone`, dan `id` stabil; progress seksi sebelum sumber termuat; detail urut dan pencarian per field; format port, uptime, dan umur; signature `ProviderTile` sebagai usulan spec 0017; kasus tepi kapasitas (`total` 0, jepit, reply lama, pemilik `CapacityModel`, pemicu `completedRounds`); copy untuk nilai null dan nomor restart; detail CLI (baris jumlah, warna, details `name`, bentuk UUID); testability AC-14; prasyarat build dan cek manual.
- **Diterapkan, soundness**: `ShellNavigator` tidak memegang `@SceneStorage` dan membaca permintaan lewat `onChange(initial: true)`; kolom tabel mengalah di lebar sempit; jalur keyboard aksi lewat inspector; ambang `low` 5 GB dan copy "about"; kapasitas hanya untuk volume root data dengan satu kode error; single flight dengan timeout 5 detik; `itemFailures` menyimpan revision; keterangan kartu memakai `.subheadline`.
- **Ditolak**: saran memindah `stopping` ke kelompok `stopped`. Proses yang sedang berhenti masih hidup, jadi menghitungnya Stopped mengulang masalah "dihitung dari PID" dari arah sebaliknya; `stopping` tetap `notReady`.

## Update dari spec 0021 (2026-10-20)

Diterapkan sesi `upd-0019-0022-0023` dari *Usulan update spec lain* spec 0021 (start, readiness, dan connection), atas persetujuan lead untuk user:

- *Navigasi*: `ShellDestination.instance(UUID)` untuk halaman detail fitur 21, `instanceRequest` yang diterapkan `InstancesPage` ke `NavigationStack` nya (juga saat Instances belum tampil), klik ulang Instances di sidebar yang kembali ke tabel, dan `openInstance` yang kini memanggil `.instance(id)` menggantikan penanda `// provisional: F21`.
- *Halaman Instances*: placeholder "No Selection" menyebut klik ganda untuk membuka, baris Readiness inspector memakai `ReadinessText` spec 0021 tanpa umur pemeriksaan (spec 0010 tidak mengirim pembaruan `observed_at` saja, jadi umur itu bisa salah), dan catatan pemilik menyebut tambahan inspector dari fitur 21 (Open, Data, Connection).
- AC-11: `stackly instance inspect --json` membungkus hasil sebagai `{ "instance": InstanceSummary }`, supaya fitur 21 cukup menambah key `detail` dan `dataUsage` dan flag `--size` tanpa perubahan breaking.

ID AC dan status spec ini tidak berubah.


## Penyelarasan 2026-10-20 (keputusan K foundation)

Update `/architect` yang menyelaraskan spec ini dengan keputusan lead foundation K1 sampai K18 (`foundation-k-decisions.md`) dan update spec foundation yang sudah di main (0004, 0006, dan 0010), dari daftar lead `docs/scope/coordination/02-instalasi-pertama.penyelarasan-k.md` (bagian *0022* dan *Cross-cutting*). Ini perubahan teks saja; ID AC, status `Proposed`, dan nomor langkah build tidak berubah.

| Butir | Perubahan | Sumber |
| --- | --- | --- |
| Field instance | `ownership` dan `initialized` kini dibangun langkah 12 spec 0010 (dengan default, nilai asing, dan fixture) dan diisi langkah 13 spec itu; AC-12, langkah 3, langkah 4, milestone 1, test kontrak, *Kepemilikan*, *Batas*, *Consequences*, dan *Usulan update spec lain* tidak lagi menambahkannya | K4 butir 2 dan 3; spec 0010 di main |
| Versi protokol | minor diambil sebagai `CURRENT` di main ditambah 1 saat merge, codegen diulang saat rebase | K4 butir 1 |
| Prasyarat milestone | milestone 1 hanya butuh langkah 12 spec 0010 (DTO), karena pemetaan field pindah ke langkah 13; milestone 2 kini menyebut langkah 13 spec 0010 sendiri (handler instance dan `install_fake` untuk binary test) | turunan K4 |
| `InstanceStore` | aturan K16 (efek observasi `statusSeq` tanpa revision, reset batas saat `server_restarted`, penyaringan retained di `fetchPage`, `fetchOne`, dan `reduce`, `limit` 200, pemetaan `instance.removed`) milik langkah 16 spec 0013; kata `.patch` dengan revision sama diganti; langkah 6 tidak lagi membangun penyaringan retained dan `limit` 200; `itemFailures` dan `retry` datang setelah langkah 16 karena sama sama mengubah `ResourceStore.swift`, dan retry yang `fetchOne` nya nil menjadi delete | K16 (a), (b), (c), SEQ 4, 6, dan 7, risiko `statusSeq` per run |
| `NewInstanceAction` | langkah 16 spec 0013 tidak mendefinisikannya; yang lebih dulu di merge di antara langkah 6 spec ini dan langkah 14 milestone 3 spec 0020 mendefinisikan environment key nya | K17 |
| `MainWindowRoot` | `AppShell` tetap membuat `ShellNavigator`; langkah 6 tidak berjalan bersamaan dengan langkah 2 milestone 1 spec 0016 | K18 |
| Rantai GUI | catatan bahwa milestone 3 menunggu langkah 16 spec 0013, yang menunggu milestone 2 fitur 10; aturan observasi per jenis yang umum milik store fase 02 pertama yang mengirim observasi seperti itu (`CatalogStore` atau `RuntimeStore`), bukan spec ini | risiko K18 (*Cross-cutting* lead) |
| `volume_space` | error `PlatformError` (bukan `FsError`); `available_space` spec 0018 adalah provided method di atasnya dan `volume_facts` spec 0020 berbagi helper `statfs`, jadi yang pertama di merge membangun helper dan `volume_space`, dan langkah 1 hanya menambah yang belum ada | spec 0004 di main |
| Area `storage` | tercatat di tabel area spec 0006; aturan hint `codes::<area>::hint` sudah berlaku | spec 0006 di main |
| Copy kegagalan | baris `quarantined` dan `binary_rejected_by_os` untuk `FailureCode` baru fitur 18 | K7 butir 5; spec 0010 di main |
| Capability | Start, Stop, dan Retry tidak bergantung capability, jadi spec ini tidak menambah capability ke `InstanceSummary` | spec 0012 di main (capability instance lewat `InstanceSummary` fitur 21 atau 22 saat dibutuhkan) |
| Follow-up dan usulan | usulan spec 0010, 0004, dan 0006 ditandai sudah di main; usulan spec 0013 ditandai sebagian digantikan K16 dan K17; baris fitur 20 menunjuk ke langkah 12 spec 0010 dan langkah 16 spec 0013 | semua butir di atas |

Teks copy untuk dua kode kegagalan baru adalah pilihan spec ini, karena spec 0018 hanya menetapkan pesan error wire nya. Tidak ada keputusan yang dibiarkan terbuka; tidak ada konflik dengan keputusan K yang perlu dikembalikan ke lead.

## Perbaikan pasca penyelarasan 2026-10-20

Dari `docs/scope/coordination/post-alignment/post-0022.md` (workflow lead `wf_fa323a0b-69e`), teks saja, tanpa perubahan ID AC atau nomor langkah:

- *Navigasi* (butir 1): perilaku fitur 22 dikembalikan, yaitu `openInstance(_ id:)` sama dengan `.instances(filter: .all, select: id)` (`// provisional: F21`). Case `.instance(UUID)`, `instanceRequest`, `NavigationStack [InstanceRoute]`, dan `openInstance` sebagai `.instance(id)` ditandai "ditambah langkah 10 spec 0021", `instanceRequest` masuk daftar member, dan langkah 6 tidak membangun case dan request itu. Tidak ada siklus prasyarat, karena milestone 3 spec ini tidak butuh fitur 21.
- *Value sourcing* (butir 2): baris "waktu relatif readiness" diganti "teks readiness" lewat `ReadinessText` spec 0021 tanpa umur, sama dengan tabel inspector.
- Turunan butir 2: `ReadinessText` dibangun langkah 9 spec 0021, yang datang setelah milestone 3 spec ini. Kalau langkah 9 spec 0021 belum di merge, langkah 7 spec ini membangunnya dengan aturan dan lokasi spec 0021, dan langkah 9 spec 0021 memakainya ulang. Runner up: inspector memakai teks readiness sendiri lalu diganti fitur 21, yang membuat dua aturan teks untuk satu nilai.
- Urutan file shell (2026-10-20, dari perbaikan spec 0020 di main): langkah 14 milestone 3 spec 0020 mengubah `AppShell.swift` dan `StacklyApp.swift` dan mungkin mendefinisikan `NewInstanceAction`, jadi ia masuk daftar sesi yang tidak berjalan bersamaan dengan langkah 6 dan 7 spec ini dan langkah 2 milestone 1 spec 0016 (*Navigasi* dan *Sesi paralel yang aman*).

## Update 2026-10-21 dari spec 0067, 0068, dan 0069

- **Komponen dan umpan balik.** Fitur 67 kini satu satunya pembangun komponen dasar, jadi `ActionFeedbackBanner` sementara tidak dibuat, inspector memakai `.stacklyInspector` dan `InspectorHeader`, dan aksi memakai `StacklyAction`. Pemetaan hasil mutasi spec 0067 (`Feedback.failure`) memakai pesan daemon sebagai pesan kartu, yang bisa tidak menyebut instance nya. Cross check menandainya, dan lead (atas delegasi user, 2026-10-21) memilih mempertahankan nama: `rejected` dan operasi `failed` memakai `Feedback(tone: .critical, message: "Couldn’t start “{name}”.", detail: DaemonText.sentence(body.message), code: body.code)`, sedangkan `notSent` dan `outcomeUnknown` tetap lewat `Feedback.outcome`.
- **Perintah.** Spec 0069 memindahkan Sort Instances By, Show/Hide Inspector, dan ⌘F ke `CommandMap` yang diterbitkan halaman ini, jadi `InstancesCommands` hanya dibangun sementara kalau milestone 3 spec ini mendahului milestone 2 spec 0069. AC-13 kini juga menyebut menu Instance (Start ⌘R, Stop ⇧⌘R) sebagai jalur keyboard aksi, disetujui user lewat lead (2026-10-21).
- **Snapshot.** Spec 0068 mengganti preview per tampilan dengan suite snapshot; siapa yang menulis suite mengikuti urutan merge (spec 0068 *Key invariants*).
- **Cross check (2026-10-21).** Cross check `Another model` (Sonnet 5.5) atas update ini dan perbaikannya diterima lead atas delegasi user (2026-10-21): header dan prominence tombol inspector, `helpText` toggle inspector, `InstancesCommands` dihapus langkah 6 spec 0069, `showsReason` per permukaan dan `isBusy` (D9), Copy Name dan Copy ID lewat `CopyConfirmation` (D13), tombol New Instance sebagai `ActionButton` (D17), baris operasi Overview lewat `StagedProgress` untuk jenis yang punya definisi tahap (D18), dan nama instance tetap di kartu gagal (D19).
- **Kolom halaman 384 pt (2026-10-21).** Temuan langkah 14 spec 0067: di jendela 900 pt dengan sidebar dan inspector terbuka, kolom halaman hanya 384 pt. User menerimanya. Lebar minimum kolom Name, Provider, Status, dan aksi berjumlah 392 pt, jadi Provider juga disembunyikan di bawah lebar tabel 480 pt supaya Name, Status, dan aksi tetap terlihat tanpa menggulir; ambang 480 pt adalah rekomendasi update ini. Teks AC-13 kini memisahkan 640 pt dengan inspector tertutup dan kolom halaman 384 pt dengan inspector terbuka di jendela 900 pt, disetujui user lewat lead (2026-10-21); inspector dibatasi 280 pt di jendela di bawah 1000 pt (spec 0067).

## Update 2026-10-22: Needs Attention saat operasi tidak terbaca

**Context.** Temuan lead ui-macos dan `dev-f22-m4` (snapshot `overviewWhoseOperationsFailedToLoad`): AC-3 meminta `DelayedProgress` selama `OperationCenter` "belum termuat atau gagal", jadi saat snapshot operasi gagal (atau daemon tidak mengenal `operation.list`), Needs Attention menampilkan progress selamanya, termasuk saat item instance dan disk sudah siap. Itu bertentangan dengan AC-5 ("seksi lain tetap tampil"), menyembunyikan masalah instance yang sudah terbaca, dan tidak pernah memberi tahu pengguna bahwa operasi yang perlu recovery (O1) tidak bisa diperiksa. Seksi Operations di halaman yang sama sudah menampilkan placeholder "Couldn’t Load Operations" dengan Retry, atau "Operations Unavailable". Kegagalan kapasitas sudah punya preseden: item S4 neutral, bukan progress.

**Opsi.**

- **Opsi 1 (dipilih): item kegagalan O2 di daftar, seperti S4.** Item `operations` neutral "Operations unavailable" ke Operations, dengan isi yang menyebut bahwa operasi yang perlu recovery hilang dari daftar ini; item instance dan disk tetap tampil. Pro: satu bentuk dengan S4 (sumber yang gagal menjadi item neutral), tidak ada komponen atau pola fokus baru, label VoiceOver dan urutan fokus ikut aturan item yang sudah ada, dan "Nothing needs attention." tidak pernah tampil saat operasi tidak terbaca karena daftar tidak pernah kosong. Kontra: kegagalan baca Stackly bercampur dengan masalah lingkungan pengguna, dan dengan lebih dari enam item O2 bisa berada di balik Show More (diimbangi placeholder seksi Operations yang selalu terlihat).
- **Opsi 2: item instance dan disk dengan catatan di bawah daftar.** Satu baris catatan "Operations couldn’t be read…" di luar daftar item. Pro: memisahkan masalah lingkungan dari kegagalan baca, dan catatan selalu terlihat walau ada Show More. Kontra: elemen baru di kartu seksi dengan aturan fokus, VoiceOver, dan snapshot sendiri; perlu kalimat gabungan saat daftar kosong ("Nothing needs attention." akan bohong); dan S4 tetap item, jadi dua sumber yang gagal tampil dengan dua cara berbeda.
- **Opsi 3: tetap progress (keadaan sekarang).** Pro: tidak ada perubahan. Kontra: progress tanpa akhir, bertentangan dengan AC-5, dan menyembunyikan masalah instance yang sudah diketahui.

**Keputusan.** Opsi 1. Bentuknya sama dengan S4, yang sudah disetujui untuk kegagalan sumber lain di daftar yang sama, dan perubahan kodenya kecil (`AttentionRules`, `OverviewPage` di folder `Overview/`, `AttentionList` hanya doc comment). Runner up: opsi 2, yang lebih baik hanya kalau kegagalan baca perlu selalu terlihat di atas lipatan Show More; placeholder seksi Operations sudah memenuhi kebutuhan itu.

**Keputusan kecil (gap yang diisi dengan rekomendasi architect, atas jawaban tetap brief).**

- *Tone neutral*, sama dengan S4: item ini melaporkan bahwa Stackly tidak bisa membaca, bukan masalah di lingkungan pengguna. Runner up: `caution`, karena O1 yang hilang bisa critical; ditolak supaya dua sumber yang gagal punya satu tone.
- *Unsupported juga O2*, dengan isi sendiri ("This version of stacklyd doesn’t list operations. …"), sama dengan S4 yang punya versi unsupported. Keadaan ini sebelumnya juga menahan seksi selamanya (snapshot `overview-operations-unsupported`).
- *Isi tetap tanpa pesan daemon*, berbeda dengan S4 yang memakai pesan daemon: S4 satu satunya tempat kegagalan disk tampil di Overview, sedangkan kegagalan operasi sudah tampil di placeholder seksi Operations, jadi O2 cukup menyebut akibatnya bagi daftar ini.
- *Tanpa tombol Retry di item*: setiap item adalah satu tombol navigasi (`AttentionRow`), dan kontrol kedua di dalamnya mengubah pola fokus dan VoiceOver semua item. Retry tetap di placeholder seksi Operations.
- *O2 tetap tampil selama snapshot operasi diulang*: `ResourceStore` menyimpan `error` sampai snapshot berhasil, jadi item tidak berkedip kembali ke progress. Snapshot yang berhasil menghapus O2 dan memunculkan O1 kalau ada.
- *Satu fungsi keadaan untuk dua seksi*: `OverviewPage.operationsState` (unsupported, ada data, error, belum termuat) dipakai `operationsContent` dan `attentionItems`, supaya urutan itu tidak hidup di dua salinan. Operasi yang gagal dengan data lama tetap memakai data itu tanpa O2 (aturan AC-5); unsupported menang atas data lama, sama dengan kode seksi Operations yang sudah ada.
- *Pengulangan otomatis saat `completedRounds` berubah*: tanpa itu O2 bisa tertinggal di layar setelah koneksi pulih, karena pemulihan lewat cursor (`.caughtUp`) tidak mengulang store yang gagal. Pengulangan dipasang di `.task(id: completedRounds)` Overview yang sudah ada, di samping `capacity.refresh()`, jadi tidak ada file shell yang berubah. Runner up: `ShellSync` mengulang setiap store yang gagal saat `.caughtUp`; itu lebih umum tetapi milik spec 0013 (lead ui-macos), jadi dicatat sebagai bahan untuk pemilik spec itu, bukan diubah di sini.
- *`.failed` tanpa data operasi*: operasi yang masuk lewat event setelah snapshot gagal tidak menjadi O1, karena tanpa snapshot daftar itu tidak lengkap; isi O2 sudah menyebut bahwa operasi yang perlu recovery hilang dari daftar.
- *Unsupported tidak pernah hilang sendiri*: `ResourceStore` spec 0013 tidak pernah mengosongkan `unsupported`, jadi O2 versi unsupported tetap selama store itu hidup, sama dengan placeholder seksi Operations. Mengubahnya milik spec 0013.
- *`id` `operations`* (tanpa titik dua) supaya tidak tertukar dengan `operation:<uuid>`; kategori operasi, nomor aturan 2, `sortKey` kosong, jadi di antara item neutral O2 ada setelah S3 dan S4 dan sebelum I10.
- *Snapshot*: dua referensi yang ada mendapat input baru (dua instance dan disk `filling`) supaya O2 terlihat di enam item pertama, lalu direkam ulang; satu snapshot baru `overview-operations-loading` menunjukkan keadaan belum termuat lewat `PageRig.loadInstances()` dan `capacity.refresh()`, jadi perbedaan menunggu dan gagal terlihat di dua gambar, bukan hanya di test murni.

**Cross check (2026-10-22).** `Another model` (Sonnet 5.5), read only, atas draft update ini dan kode `StacklyUI`. Hasilnya: desain sejalan dengan S4, AC-5, AC-9, dan AC-13, dan klaim tentang `ResourceStore` dan `ShellSync` cocok dengan kode. Semua temuan diterapkan dengan rekomendasi architect (jawaban tetap brief untuk gap): input snapshot yang menyembunyikan O2 di balik Show More diganti; fungsi `operationsState` bersama dan signature `attentionItems` disebut; `AttentionOperations` dilengkapi (visibilitas, protokol, file, doc comment, `sortKey`) dan pemanggil lama masuk langkah 9a; baris tabel O2 dan AC-3 kini membiarkan unsupported menang atas data lama; klaim "hilang begitu snapshot berhasil" dibatasi ke versi error; pilihan `.failed` tanpa data dicatat; pengulangan otomatis di `.task(id: completedRounds)` ditambahkan untuk O2 yang tertinggal; invariant "Nothing needs attention." dipersempit; test `ResourceStore` dengan `fetchPage` yang ditahan ditambahkan; dan alasan snapshot belum termuat diganti dengan snapshot baru, karena `PageRig.loadInstances()` ternyata bisa memuat instance tanpa operasi. Tanda kata kata di dua baris *Pengumuman* spec 0015 kini sama.

**Teks AC.** AC-3 berubah (teks lama dan baru di catatan bersama `arc-f22-update-ac-and-scope.md`); ID AC tidak berubah. Langkah build baru 9a di milestone 4, tanpa mengubah nomor langkah lain.
