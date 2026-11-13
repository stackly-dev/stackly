# 0046. Env up dan dependency graph: alasan

Catatan keputusan untuk [index.md](index.md). `/develop` tidak membaca file ini.

## Context

Sesudah spec 0044, Stackly bisa membaca `stackly.yaml`, memvalidasinya, dan menunjukkan plan tanpa menjalankan apa pun. Yang belum ada adalah langkah yang benar benar membuat environment itu berjalan. REQ-021 meminta Up yang memasang runtime, membuat instance dedicated, memperoleh lease, lalu menjalankan dependency graph berdasarkan readiness, dengan state yang jelas saat gagal sebagian dan kompensasi hanya atas resource milik operasi itu. PRD AC-021 menambahkan bahwa dependent tidak boleh dijalankan sebagai sukses saat dependency nya tidak ready, instance yang sehat tidak boleh dihapus, dan operasi harus menunjukkan tahap yang gagal dan langkah recovery. VAL-014 dan PRD bagian 13.2 meminta instance yang dipinjam tidak pernah dikonfigurasi ulang, dan lease disimpan menurut identitas environment, bukan koneksi klien.

Fondasi yang sudah ada membatasi ruang gerak:

- Setiap mutasi adalah operasi `OperationEngine` (spec 0007). Engine mengambil semua lock sebuah operasi sekaligus saat ia mulai, tidak bisa menambah lock sesudahnya, dan membatasi 8 operasi berjalan. Tidak ada mekanisme melepas slot selagi menunggu, dan tidak ada hubungan induk dan anak antar operasi.
- Install runtime, create instance, dan start sudah ada sebagai tiga jenis operasi dengan kebijakan terputus yang berbeda (install resumable, create dikompensasi, start ditutup). Pekerjaan ketiganya privat, mengambil `OperationContext` sebagai milik, dan memakai nama langkah tetap. Jalur start bahkan mengandaikan resource operasinya adalah instance.
- `stop_all_instances` dan restart menjalankan langkahnya di dalam satu operasi lewat fungsi supervisor langsung (`stop_direct`, `start_stopped`), di bawah lock yang diambil saat operasi mulai.
- Spec 0007 mencatat risikonya sendiri: operasi environment tidak boleh menunggu operasi lain sambil memegang slot berjalan.
- Domain `Lease` dan tabel `instance_demands` sudah punya sumber `environment` tanpa sesi, dengan satu lease `active` per pasangan instance dan environment. `plan_session_rotation` sudah tidak menghentikan instance `manual` yang punya lease environment.
- Start pengguna menulis lease `manual` di batch yang sama dengan desired `running`; satu satunya kendali atas lease itu adalah pemicu start.
- Plan spec 0044 memakai `gather` dan `evaluate` create, memberi `planDigest`, dan menampilkan status tiap reference secret tanpa dialog Keychain.
- `EventKind` tertutup, jadi event baru berarti major protokol.
- Mockup menyatakan perilaku gagal sebagian: service lain tetap jalan dan environment ditandai partially ready; Activity menampilkan Up seperti itu dengan label Partial.

Kalau tidak diputuskan: developer harus memilih sendiri antara menyalin logika create ke dalam Up atau menunggu operasi lain sambil memegang slot, dan spec 0047 tidak tahu kapan lease ada, siapa pemegangnya, dan apa yang boleh ia andalkan saat Down.

## Options considered

Mode keputusan sesi ini `auto`: opsi yang direkomendasikan diambil, runner up dicatat. Tanda `berat` berarti mahal dibalik sesudah build.

### K1: bentuk operasi Up (`berat`)

- **Diambil: satu operasi pengatur yang memulai operasi anak `install_runtime`, `create_instance`, dan `start_instance` yang sudah ada.** Setiap aturan yang sudah teruji (gate trust, precheck create, cek data, readiness, kompensasi per jenis) berlaku tanpa disalin. Lock instance hanya dipegang selama anak instance itu berjalan. BR-023 benar secara struktur: yang bisa menghapus hanya anak, dan hanya miliknya. Kontra: engine butuh hubungan induk dan anak, dan satu Up menghasilkan banyak baris operasi.
- Runner up: satu operasi yang mengerjakan semuanya di dalam dirinya dengan semua lock diambil di awal, seperti `stop_all_instances`. Pro: tanpa perubahan engine, satu baris operasi dan audit, serialisasi penuh. Kontra: pekerjaan create dan install privat, memakai nama langkah tetap, dan mengandaikan resource nya sendiri, jadi harus ditulis ulang supaya bisa berjalan di konteks operasi lain; kompensasi engine berlaku untuk seluruh operasi, jadi satu create yang gagal akan menghapus instance service lain yang sudah berhasil kecuali engine diberi lingkup kompensasi baru; lock semua instance dipegang selama unduhan; dan ID instance baru harus dikunci sebelum ada.
- Opsi ketiga: klien yang merangkai (CLI dan GUI memanggil install, create, start satu per satu). Kontra: dua implementasi graph, klien yang ditutup menghentikan Up di tengah, dan lease tidak punya satu tempat dicatat.

### K2: cara induk menunggu tanpa memegang slot (`berat`)

- **Diambil: kelas slot pengatur.** Operasi yang jenisnya pengatur (`OperationKind::is_coordinator()`) tidak dihitung di batas 8 dan punya batas sendiri (4). Kelas diturunkan dari jenis operasi, bukan field baru di `BeginRequest`, supaya pemanggil `begin` yang ada tidak berubah. Induk tidak pernah menunggu induk lain dan tidak memegang lock yang dibutuhkan anaknya, jadi tidak ada lingkaran tunggu. Perubahan dispatcher kecil: satu penghitung dan satu batas. Kontra: operasi `running` tidak lagi selalu berarti memakai slot, jadi stats harus melaporkannya terpisah.
- Runner up: induk melepas slot selagi menunggu dan mengambilnya lagi saat anak selesai. Pro: satu batas saja. Kontra: state baru di mesin state operasi, induk bisa tertahan lama menunggu slot kembali hanya untuk mencatat hasil, dan lebih banyak jalur yang harus benar saat daemon berhenti.

### K3: kapan lease dicatat (`berat`)

- **Diambil: bersama keanggotaan, dalam transaksi yang sama, sebelum graph berjalan.** Member lama dan `instanceRef` di commit `accept`; instance baru di batch `register` create. Ini urutan REQ-021 (lease, lalu graph), dan memberi spec 0047 satu invariant sederhana: daftar member adalah daftar lease. Lease yang dicatat sebelum instance dibaca juga yang membuat Up dan Down serentak konsisten. Kontra: service yang akhirnya tidak dinyalakan tetap memegang lease, jadi environment yang gagal Up harus di Down sebelum bisa dihapus.
- Runner up: lease dicatat per service tepat sebelum start nya. Pro: lease hanya ada untuk yang benar benar dinyalakan. Kontra: jendela di mana instance dedicated ada tanpa lease, Down harus membedakan member ber lease dan tanpa lease, dan Up yang menunggu dependency bisa kalah balapan dengan Down environment lain atas instance bersama.

### K4: Up yang terputus restart daemon (`berat`, mengubah default spec 0007)

- **Diambil: tidak resumable, kebijakan `CloseInterrupted`.** Sesudah daemon start lagi, operasi ditutup `failed` alasan `interrupted`. Member, lease, dan instance yang sudah ada tetap, dan Up bisa diulang karena ia selalu bekerja dari keadaan yang ada. Ini pola `stop_all_instances`. Kontra: Up tidak berlanjut sendiri sesudah restart daemon terencana, dan tabel resume spec 0007 harus diubah.
- Runner up: resumable seperti default tabel spec 0007. Pro: Up berlanjut sendiri. Kontra: induk harus menemukan lagi anak dari run sebelumnya (key turunan dan replay), membedakan anak yang sukses tetapi belum dicatat dari anak yang gagal, dan menghitung ulang plan yang kini berbeda karena sebagian sudah dikerjakan. Itu jalur yang jarang dipakai, sulit diuji, dan salahnya berarti create ganda. Sesudah reboot, antrean start spec 0031 toh sudah menyalakan lagi instance yang desired nya `running`.

### K5: gagal sebagian (`berat`)

- **Diambil: tidak ada rollback. Yang ready tetap jalan, yang bergantung pada service gagal tidak dinyalakan, dan Up bisa diulang.** Sesuai mockup dan PRD AC-021 (instance sehat tidak dihapus). Mengulang Up adalah satu satunya recovery yang perlu diingat. Kontra: environment bisa berada di keadaan setengah jalan sampai kamu bertindak.
- Runner up: hentikan lagi semua yang dinyalakan Up itu saat ada yang gagal. Pro: keadaan akhir selalu "semua atau tidak sama sekali". Kontra: menghentikan database yang baru saja ready karena service lain gagal membuang waktu dan bisa mengganggu environment lain yang sudah meminjamnya; dan "yang dinyalakan Up itu" tidak selalu jelas untuk instance bersama.

### K6: state akhir Up yang tidak lengkap (`biasa`)

- **Diambil: `failed` dengan `env.up_incomplete` yang membawa daftar service.** Script dan `--wait` mendapat exit bukan 0 tanpa aturan khusus, dan `items` sebuah `ErrorBody` sudah bisa membawa daftar. Label "Partial" di Activity adalah tampilan untuk kode itu. Kontra: operasi yang sebagian berhasil tercatat `failed`.
- Runner up: `succeeded` dengan `result.partial`, pola `stop_all_instances`. Pro: label Partial muncul tanpa aturan baru. Kontra: Up dengan nol service ready pun `succeeded`, CLI butuh kode lokal untuk mengubahnya jadi exit bukan 0, dan `PartialItem` tidak punya tempat untuk dependency dan tahap.

### K7: reference secret yang belum terisi (`biasa`)

- **Diambil: Up ditolak di precheck, sebelum mutasi apa pun, untuk status `missing` dan `unregistered`; status `unknown` baru ketahuan saat create atau start dan hanya menghentikan service itu.** Mengisi secret lalu mengulang jauh lebih murah daripada membereskan environment setengah jadi, dan teks scope ("Up berhenti") terpenuhi secara harfiah. Kontra: satu reference kosong menahan service lain yang tidak butuh secret itu.
- Runner up: selalu lanjut dan tandai service itu saja. Pro: service lain bisa dipakai. Kontra: Up pertama dari manifest tim hampir selalu berakhir setengah jadi, dan dependent dari service itu ikut dilewati.

### K8: ikatan member pada instance baru (`berat`)

- **Diambil: create menerima `EnvironmentBinding` dan menulis instance, member, dan lease di batch `register` yang sama; kompensasinya menghapus ketiganya.** Tidak pernah ada instance dedicated yatim yang akan terbaca plan berikutnya sebagai `instance.name_taken`, dan tidak perlu logika adopsi. Kontra: menyentuh jalur create yang stabil.
- Runner up: induk mengikat sesudah anak create sukses. Pro: create tidak berubah. Kontra: ada jendela (batal, commit gagal, daemon berhenti) di mana instance ada tanpa member; menutupnya butuh key turunan, replay, dan aturan plan baru di wilayah spec 0044.

### K9: arti dan default timeout dependency (`biasa`)

- **Diambil: timeout membatasi lama dependent menunggu sejak dependency mulai dinyalakan; default 600 detik (batas atas `readinessSeconds` provider).** Waktu install dan create tidak dihitung, jadi unduhan lambat tidak membuat dependent menyerah. Dependency yang kehabisan waktu tetap dilanjutkan dan hasilnya dicatat apa adanya. Default sebesar batas atas provider berarti dependent tidak menyerah lebih dulu daripada start dependency nya sendiri.
- Runner up: default 60 detik. Pro: gagal cepat. Kontra: provider yang sah dengan `readinessSeconds` lebih panjang membuat dependent dilewati padahal dependency akhirnya ready.
- Opsi ketiga: timeout juga memperpanjang tunggu sesudah operasi start dependency gagal `readiness_timeout`. Kontra: dua sumber kebenaran untuk "dependency gagal".

### K10: member `detached` (`biasa`)

- **Diambil: tidak disentuh, tetap member, tetap memegang lease sampai Down.** Sama dengan aturan spec 0044 ("tidak disentuh"). Menghapus service dari manifest tidak diam diam mengubah apa yang dianggap dibutuhkan. Runner up: Up melepas lease dan keanggotaannya. Kontra: edit YAML mengubah demand tanpa konfirmasi, dan pelepasan lease adalah wilayah spec 0047.

### K11: tempat laporan Up terakhir (`biasa`)

- **Diambil: kolom `environments.last_up`.** Banner butuh alasan per service juga untuk service yang tidak punya instance (create gagal, dilewati). Kolom registry tidak ikut hilang saat operasi di prune dan tidak bergantung pada format journal. Runner up: baca hasil operasi lewat `last_operation_id`. Pro: tanpa kolom. Kontra: `PartialItem` tidak punya field yang dibutuhkan, dan alasan hilang saat operasi di prune.

### K12: tanpa `EventKind` baru (`biasa`)

- **Diambil: klien memuat ulang `env.get` pada `operation.progress` operasi environment.** Minor protokol saja, dan tabel progress di memori daemon sudah cukup. Runner up: `environment.changed` (major). Belum ada kebutuhan yang tidak terjawab event operasi dan instance.

### K13: lease environment di rotasi sesi (`biasa`)

- **Diambil: aturan spec 0031 tidak berubah.** Lease bertahan, desired tidak diubah, dan antrean start login menyalakan lagi instance yang desired nya `running`, tanpa urutan dependency. Runner up: daemon menjalankan Up latar belakang berurutan saat login. Pro: urutan terjaga. Kontra: operasi otomatis yang membaca file pengguna saat login, dengan manifest yang mungkin sudah berubah; bisa ditambah nanti tanpa merusak apa pun.

### K14: CLI (`biasa`)

- **Diambil: `env up` tanpa konfirmasi; argumen posisi yang memuat `/` adalah path; `env status` selalu keluar 0 saat environment ditemukan.** Mengetik `env up` adalah persetujuannya, dan provider yang belum disetujui tetap blocker. Path posisi membuat contoh onboarding `stackly env up ~/Code/erp` benar. Runner up: `--yes` wajib saat ada install atau create. Kontra: contoh PRD dan mockup tidak berjalan apa adanya. Untuk status, runner up nya exit bukan 0 saat tidak ready; ditolak karena perintah baca yang gagal karena isi datanya menyulitkan script yang hanya ingin membaca JSON.

### K15 sampai K18: keputusan kecil (`biasa`)

| K | Keputusan | Runner up |
| --- | --- | --- |
| K15 | Up untuk manifest yang belum terdaftar mendaftarkannya di langkah `accept`; pendaftaran tidak dibatalkan saat Up gagal; Up kedua untuk environment yang sama ditolak `resource.busy` | antre (`Queue`), yang menjalankan plan yang sudah basi |
| K16 | install paling banyak 2 sekaligus, create berurutan, start paling banyak 4 | semua paralel (port `auto` dan disk jadi tidak teratur) |
| K17 | `Origin::Child` memakai klien induk, tanpa key, dan ikut ditolak gerbang quiesce | `Origin::Background` (anak baru bisa mulai saat daemon mau restart, dan audit kehilangan pelakunya) |
| K18 | aksi tiap service diturunkan dari lifecycle saat gilirannya tiba, bukan dari plan; `env.up` selalu membuat operasi | pakai aksi plan apa adanya (instance yang berubah di antara plan dan giliran service salah ditangani) |

## Rationale

Tiga gaya membentuk keputusan.

Pertama, engine dan tiga jenis operasi yang ada sudah teruji, dan pekerjaannya tidak dirancang untuk dijalankan di dalam operasi lain. Menyalin atau membongkar create dan install supaya bisa berjalan inline berarti dua jalur untuk hal yang sama, tepat yang REQ-021 dan BR-023 ingin hindari, dan kompensasi satu operasi untuk banyak instance akan menghapus yang sudah berhasil. Operasi anak memakai apa yang ada tanpa mengubah perilakunya. Harga yang dibayar adalah tiga tambahan kecil di engine, dan itu juga yang akan dipakai Down.

Kedua, batas 8 operasi dan aturan "semua lock sekaligus" membuat menunggu operasi lain berbahaya. Kelas slot pengatur menghilangkan lingkaran tunggu tanpa state baru: induk tidak memegang slot, tidak memegang lock anaknya, dan tidak pernah menunggu induk lain.

Ketiga, PRD dan mockup sama sama meminta kegagalan yang tidak merusak: instance sehat tetap ada, yang gagal dijelaskan, dan lease tidak bergantung pada klien. Karena itu Up tidak pernah menghentikan apa pun, lease ditulis bersama member dalam satu transaksi, dan Up yang terputus cukup diulang. Membuat Up resumable dan membuat ikatan sesudah create sama sama membuka jendela create ganda atau instance yatim; keduanya ditolak demi satu aturan yang mudah diuji: Up selalu bekerja dari keadaan yang ada.

Menolak Up saat reference kosong, sebelum mutasi, mengikuti kalimat scope dan PRD bagian 13.1: secret harus diisi sebelum startup dan tidak pernah diganti nilai kosong. Status `unknown` tidak bisa diputuskan tanpa dialog Keychain, jadi ia dibiarkan sampai operasi anak yang memang boleh membuka dialog.

## Koordinasi dengan sesi paralel

- **Spec 0045** (`arc-f45-spec`): kontrak `--locked` di `index.md` dikirim sebagai usulan tujuh butir saat spec ini mulai ditulis. Isinya: Up tidak pernah me resolve, `locked` adalah masukan plan, semua kode lock milik spec 0045 dan muncul sebagai blocker plan, `planDigest` memuat masukan lock, Up tidak menulis lockfile, dan `accept` menulis `lock_sha256` dari field plan. `arc-f45-spec` menyetujui ketujuhnya (2026-11-13) dan memberi nama final: `PlanInputs.locked`, `EnvPlan.lock.sha256`, `PlanRuntime.source`, `PlanItem.providerRevision`, dan `PlanItem.storage`, ditambah tiga hal yang kini ada di kontrak: lock dipakai juga tanpa `--locked`, GUI selalu mengirim `locked` false, dan Up meneruskan revision provider serta lokasi storage ke create. Semua kode blocker lock dan override milik spec 0045.
- **Spec 0047** (`arc-f47-spec`): mulai sesudah spec ini di merge dan bertumpu pada *Titik sambung untuk spec 0047*.
- File satu pemilik saat build (`crates/protocol/`, `protocols/`, hasil codegen, migrasi) hanya disentuh langkah 1, 2, dan 5.

## Cross check

Dijalankan 2026-11-13 oleh model lain (Opus) sebagai pembaca saja, sesuai aturan umum koordinasi. Ia menemukan 25 butir dan menilai bentuk desainnya sehat tanpa alternatif yang lebih sederhana. Mode `auto` menerapkan perbaikan yang direkomendasikan, semuanya sebelum spec diajukan ke lead:

- **Salah fakta, diperbaiki**: langkah `finish` tidak bisa dibuka sesudah cancel pada operasi `CloseInterrupted`, jadi laporan kini ditulis lewat commit penutup di dalam langkah `run`; `start_instance` menolak instance `stopping` dan yang belum initialized saat `begin`, jadi `stopping` kini ditunggu dan yang belum initialized dilaporkan dengan kode supervisor nya; `set_progress` tidak menerbitkan event pada pergantian phase, jadi klien memuat ulang pada event operasi anak; `instance_demands` sudah cascade, hanya `environment_instances` yang `restrict`.
- **Celah keputusan, ditutup**: jalur tanpa operasi `start_precheck` yang menulis lease `manual`; ikatan di `input` journal create dan penutupan anak antre di `recover`; sumber lifecycle dan readiness (registry dan `Supervisor::status`, polling); `wait_final` sebagai method engine yang juga berhenti di `needs_recovery`; kelas slot dari `OperationKind`; setiap tulisan environment membaca segar di dalam guard; guard `accept` hanya memeriksa service di manifest dan tidak memberi lease baru ke member `detached`; blocker `needsSecret` pada member diubah menjadi `secret.needs_secret`; `generateAllowed` dari aturan yang ada; `activeOperation` dari `OperationEngine::list`; `UpBoard` sebagai pemilik tabel progress; `environmentId` dan `name` di `input` untuk replay; perilaku `StartTrigger::Environment` di setiap cabang; kode kegagalan `accept` dan pemetaan `BeginError`; gerbang quiesce menandai sisa service `cancelled`; batas antre anak start; alasan `not_ready` untuk dependency yang berhenti tepat sebelum dependent mulai; aturan path CLI dan `status` untuk path.
- **Diteruskan, bukan diterapkan di sini**: dua usulan perilaku untuk spec 0044 (masukan `planDigest` tanpa aksi turunan lifecycle; instance belum initialized sebagai blocker plan), karena keduanya wilayah spec 0044. Sampai diterapkan, akibatnya dicatat di *Consequences*.

## Bahan yang dibaca

- `prd.md`: REQ-021, REQ-028, BR-023, VAL-014, AC-021, AC-073, bagian 13.1 dan 13.2 (hanya sebagai input spec).
- `docs/scope/08-environment-proyek.md` dan aturan umum koordinasi scope 08.
- Spec 0044 (seluruhnya), dan bagian yang dirujuk dari spec 0004, 0005, 0007, 0008, 0009, 0010, 0011, 0016, 0018, 0020, 0022, 0031.
- Kode: `crates/domain/src/lease.rs`, `environment.rs`, `operation.rs`; `crates/core/src/operation/` (`mod.rs`, `context.rs`, `work.rs`, `scheduler.rs`, `lock.rs`, `limits.rs`); `crates/core/src/supervisor/` (`startup.rs`, `stop_all.rs`, `restart_op.rs`, `pending.rs`, `respawn.rs`); `crates/core/src/instance/create/work.rs`; `crates/core/src/runtime/install.rs`; `crates/core/src/session.rs`; `apps/daemon/src/ipc/` (`operation.rs`, `stop_all.rs`, `runtime.rs`, `instance/`); `crates/protocol/src/operation.rs`, `instance.rs`.
- Mockup `docs/mockups/Stackly.dc.html` (toolbar Environments, sheet plan dengan Bring Up, banner partially ready, Activity, Welcome).
