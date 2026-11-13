# 0047. Env down, shared instance, dan lease: alasan

Catatan keputusan untuk [index.md](index.md). `/develop` tidak membaca file ini.

## Context

Sesudah spec 0046, sebuah environment bisa dinyalakan: Up mencatat satu lease per instance yang dibutuhkan lalu menyalakan service menurut dependency. Yang belum ada adalah jalan pulangnya. REQ-022 meminta Down yang melepas lease environment dan menghentikan hanya instance yang tidak lagi dibutuhkan, dengan memperhitungkan shared reference, start manual, dan ownership, tanpa menghapus data atau runtime (BR-006). PRD AC-022 menetapkan kasus intinya: dua environment memakai satu instance, yang satu Down, lease yang lain tetap ada dan instance tetap ready. PRD AC-073 menambah syarat serentak: Up A dan Down B pada instance bersama harus berakhir dengan lease yang konsisten, instance tidak dihentikan selagi dibutuhkan, dan tanpa create ganda.

Sisi sebaliknya sama pentingnya. BR-015 melarang stop individual yang diam diam melanggar lease: ia harus ditolak atau meminta override dengan dampak yang jelas. Scope menyebut empat tempat lagi yang punya masalah yang sama: Stop Services di menu bar, `instance.stop_all`, `agent_off`, dan ganti port pada instance yang dipinjam.

Fondasi yang sudah ada membatasi ruang gerak:

- `evaluate_stop` di domain (spec 0004) sudah memutuskan kedua arah: `EnvironmentDown` melepas lease environment itu dan berhenti hanya kalau tidak ada lease `active` lain; `Manual` meminta override selagi ada lease environment, dan dengan override lease itu tetap `active`.
- `instance.stop` sudah punya `overrideLeases`, dan menolak dengan `supervisor.stop_requires_override` yang `items` nya hanya memuat `environmentId`. Pemeriksaan lease nya membaca lease lalu commit dalam dua langkah terpisah.
- `stop_all::select` (spec 0031) menaruh instance ber lease environment di `skipped`, dan `agent_off` tidak keluar selama ada yang `skipped`. Spec 0031 dan 0032 menyerahkan aturan override ke fitur ini.
- Up (spec 0046) adalah operasi pengatur dengan operasi anak, memegang `LockKey::Environment` dan `EnvironmentName`, mencatat lease dalam commit sebelum membaca lifecycle instance, dan menunggu instance `stopping` lalu menyalakannya lagi. Start environment tidak menulis lease `manual`. Member `detached` tetap memegang lease sampai Down.
- `env.remove` (spec 0044) menolak selama ada lease `active`.
- Rotasi sesi (spec 0031) tidak menyentuh desired instance ber lease environment.
- `EventKind` tertutup, jadi event baru berarti major protokol.
- Aturan proyek untuk Stop All: klien mengirim balik himpunan yang ia tampilkan, dan daemon menolak kalau ia akan menghentikan himpunan lain.
- Mockup menggambar tombol Down, konfirmasi "Bring down", baris Lease, kata "· lease" di Ownership, dialog stop pada instance ber lease, dan Stop All yang menyebut instance ber lease.

Kalau tidak diputuskan: developer harus memilih sendiri kapan lease dilepas relatif terhadap stop, apa yang terjadi saat Up dan Down bertemu, dan apakah restart atau ganti port ikut dijaga. Tiga pilihan itu menentukan apakah PRD AC-073 benar atau hanya biasanya benar.

## Options considered

Mode keputusan sesi ini `auto`: opsi yang direkomendasikan diambil, runner up dicatat. Tanda `berat` berarti mahal dibalik sesudah build.

### K1: bentuk operasi Down (`berat`)

- **Diambil: operasi pengatur `environment_down` yang memulai satu operasi anak `stop_instance` per instance.** Bentuk yang sama dengan Up, memakai kelas slot pengatur, `Origin::Child`, dan `wait_final` yang sudah dibangun spec 0046. Lock instance hanya dipegang selama stop instance itu, setiap stop punya baris operasi dan audit sendiri, dan timeout, `last_exit`, serta Force Stop bekerja seperti stop biasa. Kontra: satu Down menghasilkan banyak baris operasi, dan handler stop harus bisa dipanggil dari dalam daemon.
- Runner up: satu operasi yang menghentikan semua instance lewat `stop_direct` di bawah lock yang diambil di awal, seperti `stop_all_instances`. Pro: satu baris operasi, tanpa port anak. Kontra: lock semua instance dipegang selama seluruh Down, termasuk selagi satu stop menunggu timeout; Up environment lain yang butuh instance bersama tertahan di lock itu; dan ada dua bentuk operasi environment yang harus dipahami.
- Opsi ketiga: klien merangkai stop satu per satu. Kontra: klien yang ditutup menghentikan Down di tengah dan lease tidak punya satu tempat dilepas, bertentangan dengan PRD bagian 13.2.

### K2: Down selagi Up berjalan pada environment yang sama (`biasa`)

- **Diambil: Down antre (`BusyPolicy::Queue`) dan membatalkan Up itu.** Down berarti "saya ingin ini mati", dan Up yang macet menunggu dependency adalah saat orang paling sering menekannya. Script `up --wait --timeout 60 || down` juga bekerja, karena `--wait` yang kehabisan waktu tidak membatalkan Up. Ada preseden: Stop All dan akhir sesi membatalkan start yang terbuka. Kontra: create yang sedang berjalan ikut dikompensasi, dan pembatalan terjadi tanpa pertanyaan kedua (hanya satu kalimat di konfirmasi).
- Runner up: tolak `resource.busy` sampai kamu membatalkan Up sendiri. Pro: tidak ada pembatalan tersembunyi, paling sederhana. Kontra: dua langkah untuk satu niat, dan script pembersih gagal tepat saat dibutuhkan. Kalau runner up yang dipilih, AC-3 berubah (Down `Reject`), `cancelsUp` hilang dari preview, dan tombol Down nonaktif selama Up.

### K3: kapan lease dilepas (`berat`)

- **Diambil: semua lease environment dilepas dalam satu commit di langkah pertama, sebelum stop mana pun.** Arti Down menjadi tegas: begitu diterima dan langkah `release` lewat, environment tidak memegang apa pun. `env.remove` langsung bisa dipakai, PRD AC-022 benar tanpa bergantung pada hasil stop, dan environment lain yang Up sesudah itu melihat keadaan lease yang utuh, bukan setengah. Kontra: kalau stop gagal atau daemon berhenti, ada instance yang masih jalan tanpa lease; `select` menangkapnya pada Down berikutnya.
- Runner up: lease tiap instance dilepas di commit stop instance itu sendiri. Pro: tidak pernah ada instance jalan tanpa lease karena Down. Kontra: Down yang gagal di tengah meninggalkan environment dengan sebagian lease, `env.remove` menolak dengan alasan yang sulit dijelaskan, dan instance yang `kept` butuh commit pelepasan terpisah juga, jadi ada dua jalur pelepasan. Kalau runner up yang dipilih, AC-4 dan AC-8 berubah.

### K4: tempat keputusan stop (`berat`)

- **Diambil: di guard commit yang menulis desired `stopped`, di bawah lock instance, dengan `evaluate_stop(EnvironmentDown)` atas lease yang dibaca di transaksi itu.** Up mencatat lease dalam commit sebelum membaca lifecycle. Dengan keputusan stop di dalam transaksi, hanya ada dua urutan: lease lebih dulu (stop melihatnya dan tidak jadi), atau `stopping` lebih dulu (Up melihat `stopping`, menunggu, lalu menyalakan lagi). Tidak ada urutan ketiga. Stop pengguna memakai guard yang sama, yang sekaligus menutup celah lama antara precheck dan commit untuk BR-015. Kontra: jalur stop yang stabil berubah. Pemeriksaan itu mengikuti permintaan (`LeaseCheck`), bukan `StopIntent`, karena intent `User` juga dipakai restart, start yang dibatalkan, dan langkah Stop All, yang tidak boleh mulai menolak.
- Runner up: keputusan diambil sekali di commit `release` dan stop dijalankan apa adanya. Pro: jalur stop tidak berubah. Kontra: lease yang dicatat Up environment lain di antara commit `release` dan stop tidak terlihat, jadi instance berhenti selagi dibutuhkan dan Up itu sudah melaporkannya `ready`. Itu persis kasus PRD AC-073.

### K5: himpunan stop yang dikonfirmasi (`biasa`)

- **Diambil: `env.preview_down` lalu `env.down` dengan `expectedStopInstanceIds` wajib; daemon menolak `env.down_changed` kalau himpunannya berbeda.** Pola yang sama dengan Stop All (spec 0032), yang sudah menjadi aturan proyek. Konfirmasi Down harus menyebut apa yang tetap jalan, jadi preview memang dibutuhkan; mengirim balik himpunannya hampir gratis dan menjamin Down tidak pernah menghentikan sesuatu yang tidak tampil. Kontra: dua panggilan, dan satu kode error lagi.
- Runner up: `env.down { id }` saja, dengan preview hanya untuk tampilan. Pro: lebih sederhana, CLI satu panggilan. Kontra: lease lain yang dilepas selagi dialog terbuka membuat Down menghentikan instance yang dialognya bilang tetap jalan.
- Akibat yang diterima: instance yang baru layak berhenti sesudah preview (dua Down serentak pada instance bersama, atau instance yang sempat dinyalakan Up yang dibatalkan) tidak dihentikan. Sapuan akhir melaporkannya `left_running` dan Down berakhir `env.down_incomplete`, supaya tidak ada instance yang tertinggal diam diam; Down ulang menghentikannya. Runner up untuk bagian ini: sapuan akhir menghentikannya sendiri, yang melanggar aturan "hanya yang tampil yang berhenti".

### K6: urutan berhenti (`biasa`)

- **Diambil: dependency terbalik dari `dependency_conditions`, dependent dulu, paling banyak 4 sekaligus, dan dependent yang gagal tidak menahan dependency nya.** Aplikasi berhenti sebelum database nya, jadi log tidak penuh error koneksi. Down tetap mencoba semuanya, karena niatnya mematikan. Kontra: butuh fungsi urutan sendiri.
- Runner up: semua stop serentak. Pro: paling cepat, tanpa fungsi urutan. Kontra: dependent melihat dependency nya hilang lebih dulu.

### K7: apa yang dihitung sebagai "masih dibutuhkan" (`biasa`)

- **Diambil: lease `active` saja (environment lain, `manual`, `startup`).** Satu aturan yang sudah ada di domain dan sudah diuji. Kontra: service yang kamu nyalakan sendiri tetap jalan walau dependency nya di environment yang sama berhenti.
- Runner up: dependency dari service yang `kept` ikut ditahan. Pro: tidak ada service yang ditinggal tanpa dependency. Kontra: aturan kedua di atas lease, dan sesudah Down edge environment itu tidak punya pemilik, jadi dependency itu jalan tanpa lease dan tanpa siapa pun yang akan menghentikannya.

### K8: nasib `last_up` (`biasa`)

- **Diambil: dikosongkan di commit `release`.** Alasan `up_failed` dan `dependency_not_ready` (spec 0046) tampil untuk service yang diam. Sesudah Down semua service diam, jadi laporan Up lama akan membuat environment yang sengaja dimatikan tampil "Not ready" dengan alasan gagal. Kontra: riwayat Up terakhir hilang dari halaman (tetap ada di Activity dan audit).
- Runner up: biarkan, dan sembunyikan di klien saat tidak ada lease. Kontra: aturan tampilan kedua di tiga klien.

### K9: state akhir dan tempat laporan Down (`biasa`)

- **Diambil: `failed` dengan `env.down_incomplete` kalau ada yang tidak berhenti, `succeeded` selain itu; laporan ada di `result` dan error operasi, tanpa kolom baru.** Sama dengan Up, jadi halaman environment dan exit code CLI punya satu pola. Down cepat dan hasilnya terlihat langsung dari status instance, jadi tidak butuh `last_down`. Kontra: instance yang `kept` karena balapan muncul sebagai item `partial`, dan sesudah umpan balik hilang, rincian Down hanya ada di Activity.
- Runner up: `succeeded` dengan `partial`, seperti `stop_all_instances`. Kontra: `env down --wait` keluar 0 walau ada instance yang tidak berhenti.

### K10: stop manual pada instance ber lease (`biasa`)

- **Diambil: tolak dulu, lalu override eksplisit; lease tetap `active`; konfirmasi dibangun dari `items` penolakan.** Ini bentuk yang sudah ada di domain dan di `instance.stop`. Yang ditambah: nama environment di `items`, dialog di GUI, flag di CLI, dan guard (K4). Dialog dari penolakan selalu menampilkan pemegang yang benar saat itu, tanpa bergantung pada data yang mungkin basi. Kontra: stop pada instance ber lease selalu dua panggilan.
- Runner up: override melepas lease. Pro: environment tidak tampil "partially ready" berkepanjangan. Kontra: bertentangan dengan `StopDecision::Stop.overridden` di domain, dan Up environment itu tidak lagi tahu instance mana yang ia butuhkan.

### K11: restart, reload, apply config, rename (`biasa`)

- **Diambil: tidak butuh override.** Restart dan reload membuat instance kembali jalan dengan identitas dan port yang sama; gangguannya sebentar dan terlihat. Developer me restart database proyeknya sendiri berkali kali sehari, dan `apply_config` serta `change_endpoints` memakai langkah restart yang sama, jadi field override di sana akan menyebar ke tiga method dan memecah script yang ada. BR-015 menyebut stop. Kontra: restart instance bersama tetap bisa mengganggu environment lain tanpa pertanyaan.
- Runner up: override hanya untuk instance yang dipinjam lewat `instanceRef`. Pro: melindungi instance bersama. Kontra: dua perilaku restart berdasarkan ownership, yang sulit dijelaskan di CLI. Kalau runner up yang dipilih, AC-14 berubah dan `instance.restart` mendapat `overrideLeases`.

### K12: ganti endpoint (`biasa`)

- **Diambil: tolak tanpa override, dampak tampil dari `dryRun`, override eksplisit.** Beda dari restart: port baru bertahan, dan service lain di environment itu menyimpan alamat lama. Scope meminta "ditolak atau meminta override dengan dampak yang jelas"; override dipilih karena pengguna tunggal yang memegang kedua sisi harus tetap bisa menyelesaikan pekerjaannya tanpa Down dulu.
- Runner up: selalu tolak sampai environment Down. Pro: tidak ada environment yang patah diam diam. Kontra: untuk memindah port Valkey bersama kamu harus mematikan setiap proyek yang memakainya.

### K13: Stop All Services (`biasa`)

- **Diambil: `overrideLeases` sebagai parameter `stop_all` dan preview nya; app mengirim true sesudah konfirmasi yang menyebut environment terdampak; CLI default false.** Mockup menulis Stop All menghentikan juga instance ber lease, dan orang menekan Stop All untuk menenangkan mesin. `expectedInstanceIds` tetap menjamin hanya yang tampil yang berhenti. Kontra: bawaan app dan CLI berbeda.
- Runner up: Stop All selalu melewati instance ber lease dan menawarkan Bring Down per environment. Pro: tidak ada override massal. Kontra: Stop All tidak menghentikan semuanya, dan pengguna harus menekan beberapa tombol.

### K14: `agent_off` (`biasa`)

- **Diambil: `overrideLeases` diteruskan; Turn Off di app meng override sesudah konfirmasi.** Tanpa itu Turn Off mengharuskan Down tiap environment lebih dulu. Lease tetap, jadi sesudah Stackly dinyalakan lagi environment bisa di Up tanpa import ulang.
- Runner up: tetap menolak. Pro: tidak ada perubahan di jalur shutdown. Kontra: Turn Off macet di "an environment uses it" untuk setiap pengguna environment.

### K15: Down yang terputus restart daemon (`berat`, mengubah default spec 0007)

- **Diambil: tidak resumable, `CloseInterrupted`, dan anak stop yang antre ditutup.** Sama dengan Up (spec 0046 K4). Lease sudah dilepas di langkah pertama, jadi tidak ada yang perlu dilanjutkan selain stop, dan stop yang menyala sendiri beberapa menit sesudah restart daemon akan mengejutkan. Down lagi menyelesaikannya.
- Runner up: resumable seperti tertulis di tabel spec 0007. Kontra: butuh `WorkFactory` dan penyimpanan kemajuan per instance untuk keuntungan yang kecil.

### K16: keputusan kecil (`biasa`)

- Tidak ada `EventKind` baru; klien memakai event operasi (runner up: event lease, yang berarti major protokol).
- `stackly env down` tanpa konfirmasi, karena tidak ada yang dihapus dan apa yang berhenti dicetak lebih dulu (runner up: prompt dengan `--yes`).
- Down tidak membatalkan start yang terbuka pada instance; anak stop antre, dan lease `manual` dari start itu membuat instance `kept` (runner up: batalkan seperti Stop All, yang akan mematikan instance yang baru kamu minta menyala).
- Baris member tidak dihapus Down, supaya halaman environment tetap menampilkan instance nya (runner up: hapus member `shared`, yang membuat Up berikutnya harus mengikat ulang).
- `leaseActive` ditambahkan ke `InstanceEnvironment` untuk tampilan saja; aksi tetap dijaga penolakan daemon (runner up: klien memutuskan dari `leaseActive`, yang bisa basi).
- Tombol Down selalu aktif saat online dan tidak ada Down berjalan; preview yang kosong dijawab "Nothing to bring down." tanpa operasi (runner up mockup: nonaktif saat tidak ada yang ready, yang menyembunyikan Down untuk environment yang gagal Up tetapi masih memegang lease, dan klien tidak tahu desired instance).
- Stop pada instance yang sudah diam tidak meminta override walau ada lease environment, seperti sekarang, karena tidak ada yang terganggu (runner up: minta override juga, yang mengubah perilaku `instance.stop` untuk kasus tanpa dampak).

## Rationale

Tiga gaya menentukan bentuknya.

**Lease adalah satu satunya bahasa demand.** Domain sudah punya `evaluate_stop`, dan spec 0031 sudah menjalankan login, stop manual, dan Stop All di atasnya. Down yang membawa aturan sendiri tentang ownership atau dependency akan menjadi aturan kedua yang harus dijaga sejajar. Karena itu Down tidak bertanya "ini milik siapa"; ia melepas lease nya lalu bertanya "masih ada yang memegang". Ownership `shared` dan start manual sudah terwakili: instance bersama dipegang lease environment lain, dan start manual menulis lease `manual`, sedangkan start environment tidak (spec 0046). Ini juga yang membuat konfirmasi bisa menyebut alasan tiap instance yang tetap jalan tanpa data tambahan.

**PRD AC-073 hanya benar kalau keputusan stop ada di dalam transaksi.** Up sudah dirancang mencatat lease sebelum membaca lifecycle. Down menutup sisi satunya dengan memutuskan stop di commit yang menulis desired (K4). Melepas semua lease lebih dulu (K3) tidak mengganggu itu, karena guard stop menghitung lease yang tersisa, dan memberi Down arti yang bisa dijelaskan dalam satu kalimat. Harga yang dibayar, yaitu instance tanpa lease sesudah Down yang gagal, ditutup oleh `select` yang selalu memeriksa semua member, bukan hanya yang lease nya baru dilepas.

**Pengguna tunggal memegang semua sisi.** Stackly berjalan untuk satu orang di satu Mac. Menolak mutlak stop atau ganti port pada instance ber lease berarti memaksa orang itu mematikan proyeknya sendiri untuk pekerjaan kecil. BR-015 mengizinkan override dengan dampak yang jelas, dan itulah yang dipilih di semua tempat (K10, K12, K13, K14), dengan satu aturan yang sama: override tidak pernah melepas lease, jadi environment tetap tahu apa yang ia butuhkan dan Up mengembalikannya. Restart tidak dijaga (K11) karena ia tidak mengubah apa yang environment harapkan.

Down membatalkan Up (K2) karena keduanya adalah niat yang berlawanan atas environment yang sama, dan niat yang terakhir yang berlaku.

## Koordinasi dengan sesi paralel

- Spec 0044 dan 0046 sudah di `main` (`d413980b`) saat spec ini ditulis; bagian *Titik sambung untuk spec 0047* keduanya dipakai sebagai kontrak, tidak diubah.
- Yang diminta dari spec lain ada di *Update spec lain* `index.md`: `InstanceEnvironment.leaseActive` dan `EnvironmentDetail.lease` (spec 0044), pemindahan `ChildOrigin` (spec 0046), dan tambahan di spec 0007, 0010, 0026, 0029, 0031, dan 0032.
- Satu hal di luar wilayah spec ini dicatat sebagai Follow-up untuk spec 0044: rename instance yang dipinjam lewat `instanceRef`.

## Cross check

Dijalankan 2026-11-13 dengan model lain (Opus), baca saja, terhadap draf pertama dan kode yang ada. Putusannya "belum siap dibangun", dengan temuan yang semuanya diterapkan sebelum draf ini dikirim ke lead (mode `auto`: perbaikan yang direkomendasikan diambil):

- Guard lease yang ditempel pada `StopIntent::User` akan ikut menolak restart, stop sesudah start yang dibatalkan, dan langkah Stop All, karena ketiganya memakai intent itu. Diperbaiki: pemeriksaan mengikuti permintaan lewat `LeaseCheck` di `journal_stop` dan `stop_direct`.
- `cancel_respawn` berjalan sebelum commit, jadi instance `recovering` yang guard nya menolak kehilangan respawn. Diperbaiki: aturan *Respawn*.
- Pola `commit_exit` membangun ulang saat guard menolak, tidak cocok untuk keputusan final. Diperbaiki: helper `commit_stop_decision` dengan hasil final.
- Jalur tanpa operasi `no_op_with_desired` tidak bisa diberi guard dan selalu meng override. Diperbaiki: *Precheck stop anak* tidak pernah menulis; desired instance diam ditulis pekerjaan stop lewat commit ber guard. Stop klien pada instance diam sengaja tidak berubah.
- "Ditolak tanpa operasi" bertentangan dengan "pemeriksaan pindah ke guard". Diperbaiki: precheck tetap menolak; balapan berakhir sebagai operasi `failed` dengan kode yang sama.
- `kept` yang ditebak dari desired salah untuk instance `running` ber desired `stopped`. Diperbaiki: hasil `kept` eksplisit (`DownChild::Kept`, atau item `partial` anak).
- `SupervisorError::StopRequiresOverride` belum membawa data, dan batas `items` yang ada 100. Diperbaiki: `EnvironmentHolder`, batas 100 dipertahankan.
- Langkah Stop All tidak membangun `StopRequest`; `instance.preview_stop_all` hari ini varian tanpa params. Diperbaiki di *Override pada aksi lain* dan *API surface*.
- Dua Down serentak pada instance bersama, dan instance yang dinyalakan anak Up di antara precheck dan pembatalan, bisa tertinggal jalan tanpa lease. Diperbaiki: *Sapuan akhir* melaporkannya `left_running` (K5).
- Anak stop yang antre lebih dari 120 detik di belakang start pengguna dilaporkan gagal padahal seharusnya `kept`. Diperbaiki: hasil sementara `busy` diputuskan sapuan akhir.
- Teks yang salah: umpan balik cancel sebelum `release`, klaim `env.remove` langsung bisa dipakai, sumber `ownership` untuk instance tanpa member, daftar penyegar cache `leaseActive`, aturan tombol Down. Semuanya diperbaiki.
- Dicatat sebagai akibat, tidak diubah: Down yang terputus restart daemon meninggalkan instance ber desired `running` tanpa lease, yang menyala lagi di sesi login yang sama (AC-10, Consequences).

## Bahan yang dibaca

- `docs/scope/08-environment-proyek.md` fitur 47; `prd.md` REQ-022, REQ-028, BR-006, BR-014, BR-015, VAL-014, AC-022, AC-073, bagian 13.2 (hanya sebagai input spec).
- Spec 0044 (*Titik sambung*, *Model data*, *API surface*, *Layar*), spec 0046 (seluruh `index.md`), spec 0031 (*Model demand*, *Rotasi sesi*, *Pilihan stop all*, *Operasi `stop_all_instances`*, *Matikan agent*), spec 0032 (*Copy Stop All*, DTO preview), spec 0029 (AC-14, method), spec 0026 (AC-7, Consequences), spec 0022 L176, spec 0004 L320.
- Kode: `crates/domain/src/lease.rs`, `crates/domain/src/operation.rs`, `crates/domain/src/registry/guard.rs`, `crates/core/src/supervisor/stop.rs` dan `persist.rs`, `crates/core/src/session.rs` (`manual_stop_ops`), `crates/core/src/instance/stop_all.rs`, `apps/daemon/src/ipc/instance/mod.rs` (`stop`, `stop_precheck`), `crates/protocol/src/stop_all.rs`, `daemon.rs`, `operation.rs`, `codes.rs`, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Confirmation/ConfirmationRequest.swift`.
- Mockup `docs/mockups/Stackly.dc.html`: tombol Down, konfirmasi "Bring down", baris Lease, "· lease", dialog stop ber lease, preview Stop All.
