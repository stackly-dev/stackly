# 0031. Demand, autostart, dan login: rationale

[Kembali ke spec](index.md)

## Context

Stackly menjalankan satu daemon per user sebagai LaunchAgent (spec 0015, `RunAtLoad` true, `KeepAlive { SuccessfulExit: false }`, `ExitTimeOut` 30). Instance hidup sebagai child dari daemon itu. Fondasinya sudah ada, tetapi belum dipakai di produksi: domain punya `Lease` dengan holder `environment`, `manual`, dan `startup`, plus `suppress`, `evaluate_stop`, dan `should_autostart` (spec 0004). Registry punya `instance_demands` dan kolom `startup` (`manual` atau `at_login`, spec 0005). Tidak ada kode yang membuat `SessionId`, menulis lease, atau menulis `at_login`. Create menulis `StartupPolicy::Manual` apa adanya, dan jalur stop memanggil `evaluate_stop` dengan `session: None`, jadi suppression tidak pernah terjadi.

Rekonsiliasi spec 0010 hanya mengenal boot. Logout lalu login di boot yang sama terbaca sebagai "Gone, boot sama": instance dengan desired `running` dianggap crash lalu di respawn, apa pun setting autostart nya, dan hitungan crash bertambah. Reboot sebaliknya tidak pernah menyalakan apa pun. `SIGTERM` ke daemon membiarkan instance hidup supaya bisa diadopsi. Itu tepat untuk restart daemon, tetapi saat logout, instance lalu diakhiri OS tanpa stop bersih, dan PostgreSQL butuh crash recovery.

PRD mengikat perilakunya: autostart daemon dan autostart instance adalah dua setting (REQ-014); stop eksplisit tidak dibatalkan reconciler di sesi yang sama (BR-024, AC-069); Keychain yang terkunci menghasilkan needs secret, bukan auth kosong (AC-062); izin yang dicabut terlihat tanpa setting OS diubah diam diam (AC-014); logout mengakhiri lifecycle sesi user, dan boot tanpa login bukan janji (PRD 19.5). Tanpa keputusan ini, fitur 32 (Stop Services, Settings) dan fitur 47 (environment down) tidak punya aturan demand untuk dipakai.

## Options considered

### Keputusan 1: identitas sesi login

#### Option 1: UUID boot plus audit session ID (dipilih)

`getaudit_addr` memberi audit session ID dari sesi Aqua yang diwarisi LaunchAgent di domain `gui/<uid>`. Nilainya berganti di setiap login, dan UUID boot membuatnya unik lintas reboot.

**Pros**: dibaca sekali tanpa izin, stabil selama sesi (termasuk sleep dan restart daemon), dan muat di `SessionId`.
**Cons**: butuh satu blok `unsafe` di `stackly-platform-macos`; daemon yang dijalankan manual dari terminal SSH memakai audit session lain, jadi terlihat sebagai sesi baru.

#### Option 2: UUID boot plus waktu login dari utmpx

**Pros**: mudah dibaca manusia.
**Cons**: rapuh untuk fast user switching dan perubahan jam, dan entri utmpx bisa lebih dari satu.

### Keputusan 2: tempat menyimpan sesi

#### Option 1: tabel `daemon_sessions` (dipilih)

**Pros**: rotasi bisa dicek dengan satu baca, ada riwayat pendek cara akhir sesi (`clean`, `interrupted`, `unknown`) untuk diagnosis dan dokumentasi disruption.
**Cons**: satu migration dan empat operasi registry baru.

#### Option 2: satu key di tabel state

**Pros**: paling kecil.
**Cons**: tidak mencatat apakah logout terakhir berhenti dengan bersih.

#### Option 3: diturunkan dari lease startup terbaru

**Pros**: tanpa tabel.
**Cons**: gagal saat tidak ada instance `at_login`.

### Keputusan 3: arti autostart Off setelah login

#### Option 1: tetap mati, start manual hanya berlaku untuk sesinya (dipilih)

**Pros**: Off punya satu arti yang bisa dijelaskan dalam satu kalimat, dan cocok dengan lease manual yang berbasis sesi.
**Cons**: instance yang kamu nyalakan manual dan lupa diubah ke At login tidak kembali setelah logout.

#### Option 2: desired state dibawa lintas logout

**Pros**: logout dan login terasa seperti tidak pernah terjadi.
**Cons**: Off dan At login hanya berbeda untuk instance yang mati saat logout, dan itu sulit dijelaskan.

### Keputusan 4: perilaku saat logout

#### Option 1: stop bersih di dalam jendela launchd (dipilih)

**Pros**: PostgreSQL berhenti lewat strategi provider; data aman.
**Cons**: dibatasi `ExitTimeOut`; cluster besar bisa `interrupted`.

#### Option 2: biarkan OS

**Pros**: tanpa kode baru.
**Cons**: shutdown tidak bersih di setiap logout, dan itu bertentangan dengan tujuan done when fitur ini.

### Keputusan 5: cara mengenali logout

#### Option 1: `SIGTERM` tanpa `daemon.shutdown` (dipilih)

**Pros**: tanpa API baru, berlaku juga untuk shutdown dan restart OS, dan restart terencana sudah lewat `daemon.shutdown`.
**Cons**: `kickstart -k`, `bootout`, dan `relaunch` manual ikut menghentikan instance. Ini diimbangi start yang level triggered saat daemon kembali, tetapi mengubah bukti survival spec 0015.

#### Option 2: notifikasi NSWorkspace willPowerOff di daemon

**Pros**: survival `SIGTERM` tetap seperti sekarang.
**Cons**: menarik AppKit dan run loop ke daemon Rust, dan belum terbukti notifikasi itu sampai ke LaunchAgent tepat waktu.

#### Option 3: GUI memberi tahu daemon

**Pros**: tanpa perubahan di daemon untuk sinyal.
**Cons**: hanya bekerja saat aplikasi terbuka.

### Keputusan 6: arti daemon autostart Off

#### Option 1: unregister agent (dipilih)

**Pros**: memakai `register` dan `unregister` spec 0015, satu plist, dan satu state yang jujur di banner.
**Cons**: tidak ada mode "daemon hanya saat dibutuhkan"; Off berarti tidak ada Stackly di background sama sekali.

#### Option 2: daemon on demand, bukan saat login

**Pros**: daemon tetap bisa jalan saat app atau CLI membutuhkannya.
**Cons**: butuh plist kedua atau daemon yang keluar saat login, dan pergantian bisa memicu persetujuan Login Items lagi.

### Keputusan 7: Keychain terkunci saat autostart

#### Option 1: blocked dengan retry timer 30 detik (dipilih)

**Pros**: instance menyala sendiri begitu Keychain terbuka, tanpa dialog di background.
**Cons**: membaca Keychain secara periodik selama terkunci.

#### Option 2: blocked dan menunggu pengguna

**Pros**: paling sederhana.
**Cons**: setting At login diam diam tidak berarti apa apa setelah Keychain terbuka.

### Keputusan 8: siapa yang suppress

Stop instance dan Stop Services suppress; restart tidak (dipilih). Restart tidak mengakhiri running. Kalau restart suppress, detail akan menulis "stopped for this session" pada instance yang jalan, dan itu tidak mengubah apa pun di login berikutnya. Pilihan lain, restart ikut suppress, ditolak karena alasan yang sama.

## Rationale

Inti masalahnya adalah daemon tidak bisa membedakan "daemon restart di sesi yang sama" dari "sesi baru", sehingga setiap aturan BR-024 dan AC-069 mustahil ditepati. Identitas sesi dari audit session menyelesaikannya dengan satu nilai yang dibaca sekali, tanpa izin, dan tanpa bergantung pada GUI. Begitu sesi punya identitas, semua aturan lain menjadi fungsi murni di atas lease dan policy yang sudah ada di spec 0004. Fungsi itu bisa diuji dengan tabel, dan berjalan dalam satu transaksi `commit_guarded`, jadi crash di tengah rotasi tidak pernah meninggalkan setengah keputusan.

Untuk logout, `SIGTERM` tanpa `daemon.shutdown` adalah sinyal yang paling bisa diandalkan tanpa menarik AppKit ke daemon. Harganya adalah `kickstart -k` manual kini menghentikan instance. Start yang level triggered membuat harga itu kecil: instance yang kamu biarkan jalan menyala lagi, dan yang kamu stop tetap mati. Ini lebih jujur daripada janji survival yang di logout justru berakhir dengan shutdown tidak bersih. Budget stop 15 detik, dihitung dari saat `SIGTERM` datang, menyisakan waktu untuk urutan berhenti spec 0006 dan commit sebelum `ExitTimeOut` 30 lewat dan launchd mengirim `SIGKILL`. Menaikkan `ExitTimeOut` ditolak karena macOS logout belum tentu menunggu selama itu.

Unregister sebagai daemon autostart Off menjaga satu sumber kebenaran: registrasi OS. Tidak ada setting tersimpan yang bisa berbeda dari Login Items, sehingga AC-014 (tidak mengubah setting OS diam diam) dan spec 0015 AC-11 tetap berlaku apa adanya. Mode `daemon.shutdown { mode: agent_off }` menghentikan instance dan keluar dalam satu langkah, jadi tidak ada start yang menyelinap di antaranya, dan unregister hanya berjalan setelah daemon menjawab `exiting: true`.

Lease manual ditulis sekarang, walaupun fitur 47 baru memakainya, karena biayanya satu operasi di batch yang sama dengan desired `running`, dan instance yang di start sebelum fitur 47 jadi tidak kehilangan demand nya.

## Cross check

Cross check oleh model lain (2026-11-02, baca saja) menemukan 25 temuan, dan kamu memilih menerapkan semua perbaikan yang direkomendasikan. Perubahan yang membentuk spec:

- Rotasi `first` tanpa perubahan desired, supaya rilis pertama tidak menghentikan instance yang sedang jalan (AC-3).
- Rotasi dan *Akhir sesi* hanya di konteks launchd, plus tabel *Sinyal per konteks*, supaya daemon dari SSH, terminal, atau test tidak dianggap sesi baru atau logout (AC-2, AC-12).
- Budget stop 15 detik dihitung dari `SIGTERM`, dengan relasi terhadap urutan berhenti spec 0006 dan `ExitTimeOut` yang diuji (AC-11).
- Start level triggered lewat satu antrean yang dideduplikasi; `resumed` dan pemicu `session_resume` dihapus (AC-7, AC-13).
- Daftar `failed` yang eligible dan tidak eligible (*Antrean start*).
- Urutan daemon start (`recover`, rotasi, rekonsiliasi, antrean) dan urutan bernomor *Akhir sesi*.
- Stop dan `stop_all` pada desired `running` tanpa proses menjadi perubahan registry dengan efek lease (AC-8, AC-10).
- Retry secret berdasar desired dan blocker, bukan lease, tanpa operasi per percobaan (AC-15).
- `ShutdownMode::AgentOff` menggantikan stop_all lalu shutdown, menutup race start (AC-20).
- Unique index lease startup dan pemangkasan lease `released` (AC-14).
- Dua spike native sebelum journey 1 dan 3 (AC-21).

Temuan bahwa exit saat stop akhir sesi bisa di respawn sebagai crash tidak berlaku, karena stop selalu lewat `stopping` dan exit saat `stopping` menjadi `exited` (spec 0010). Spec kini menyatakannya di *Model demand*.

## Pembaruan 2026-11-02: tambahan dari spec 0032

Spec 0032 (Proposed, disetujui engineer) menulis *Tambahan pada kontrak spec 0031* dan Follow-up baris 1 nya meminta tambahan itu masuk ke spec ini sebelum fitur 31 membangun `instance.stop_all`. Spec 0030 Follow-up baris 1 juga dicek. Keputusan di bawah adalah rekomendasi architect, diambil lead atas delegasi user; hanya perubahan teks AC-10 yang menunggu persetujuan lead.

### Keputusan 9: satu aturan pilihan untuk Stop All

#### Option 1: `stop_all::select` dengan tiga penajaman (dipilih)

Fungsi murni dari spec 0032 tetap satu satunya aturan, ditambah: start dan restart yang terbuka masuk `stop`; `already_stopping` hanya untuk `stopping` tanpa start atau restart terbuka; key yang dipegang operasi `needs_recovery` menjadi `skipped` dengan `operation.recovery_required`.

**Pros**: restart spec 0026 menulis desired `stopped` di separuh stop nya lalu `running` lagi, dan start pengguna yang antre belum menulis desired. Teks spec 0032 apa adanya akan menandai restart itu "already stopping" (atau tidak memilihnya sama sekali di antara stop dan start), lalu restart menyalakannya lagi sesudah Stop All. Penajaman ini menjaga janji spec 0032 AC-6 ("sudah berhenti sendiri") dan AC-7 (tidak dinyalakan lagi). Engine spec 0007 mengambil semua key saat `begin`, jadi satu key `needs_recovery` akan menggagalkan seluruh operasi kalau tidak dipisahkan.
**Cons**: input fungsi kini juga membaca journal operasi, bukan hanya instance dan lease.

#### Option 2: teks spec 0032 apa adanya

**Pros**: tanpa penajaman. **Cons**: celah restart dan start antre di atas.

#### Option 3: lock per langkah, bukan saat `begin`

**Pros**: satu start yang lama tidak menahan stop lain. **Cons**: engine spec 0007 hanya mengambil key saat `begin` (`extra_locks`), jadi butuh perubahan engine untuk aksi yang jarang dipakai.

### Keputusan 10: fitur mana yang membangun tambahan itu

#### Option 1: fitur 31 membangun `select`, cancel, dan preview, fitur 32 sisanya (dipilih)

**Pros**: `select` dan cancel bagian dari operasi yang dibangun fitur 31, dan `agent_off` membutuhkan `select` sekarang. Konfirmasi Turn Off (fitur 31) butuh daftar yang sama dengan yang akan dihentikan, jadi `instance.preview_stop_all` ikut di sini dengan DTO spec 0032 apa adanya; tanpa itu konfirmasi menghitung dari `instance.list` dengan aturan kedua. `expectedInstanceIds` dan `instance.stop_all_changed` hanya berarti bersama popover, dan `default_startup` serta `--no-autostart` butuh tabel `daemon_settings` spec 0032.
**Cons**: *Method IPC* dan langkah 9 spec 0032 masih menyebut preview, `select`, dan cancel sebagai miliknya; Follow-up spec ini meminta update spec 0032.

#### Option 2: fitur 31 membangun semuanya kecuali `default_startup`

**Pros**: satu perubahan kontrak untuk `instance.stop_all`. **Cons**: kode `instance.stop_all_changed` dan field yang tidak dipakai siapa pun di fitur 31, dan tabel kode spec 0032 jadi ganda.

### Keputusan 11: balasan dan hasil `instance.stop_all`

#### Option 1: balasan menunggu operasi selesai dan dibangun dari keadaan akhirnya (dipilih)

AC-10 dan `agent_off` sudah menjanjikan `StopAllResult` di balasan, dan replay yang sama. `result.resources` (instance yang berhenti) dan `result.partial` (yang gagal atau dilewati, dengan kode) menyimpan hasil itu di journal, jadi replay dan halaman Operations membaca sumber yang sama. Engine hanya menyimpan `result` saat `succeed`, jadi pekerjaan menyerap cancel dan kembali `Ok`; cancel saat masih antre (langsung `cancelled`, tanpa `result`) dibangun dari input; operasi `failed` dibalas sebagai error. Popover spec 0032 tetap melihat progres lewat event operasi selama menunggu.
**Cons**: request yang panjang, jadi client memakai timeout interaktif 200 detik dan mengirim ulang dengan key yang sama setelah `ipc.call_timeout`.

#### Option 2: balas `OperationAccepted` langsung, client membaca hasil dari operasi

**Pros**: pola method operasi lain. **Cons**: mengubah AC-10, dan `agent_off` tetap harus menunggu, jadi ada dua cara untuk satu operasi.

Spec 0032 menulis balasannya "`OperationView`, hasil `StopAllResult`"; spec ini menetapkan bahwa balasannya `StopAllResult`, dan operasinya terlihat lewat event. AC-10 lama juga menulis "menunggu lock itu seperti `instance.stop`", padahal `instance.stop` memakai `BusyPolicy::Reject` selama start atau restart (spec 0010 AC-7, spec 0026 AC-9). Spec kini menyebut `BusyPolicy::Queue` langsung.

### Keputusan 12: rincian `agent_off`

- `idempotencyKey` wajib, seperti `restart`, karena `agent_off` menjalankan operasi. Handler memanggil `replay_operation` dulu, jadi kiriman ulang setelah timeout tidak ditolak gerbangnya sendiri.
- Recovery mode ditolak `registry.recovery_mode`: tanpa registry, daemon tidak bisa memilih instance, dan agent yang tetap terdaftar yang nanti menjalankan daemon dengan registry yang sudah diperbaiki. `daemon.shutdown { mode: stop }` tetap tersedia.
- Gerbang quiesce spec 0030 dipakai ulang dengan kode `daemon.shutting_down`, hanya untuk `Origin::Request`, dengan flag `bypass_quiesce` di `BeginRequest` untuk operasi `agent_off` sendiri (kind saja tidak cukup, karena `instance.stop_all` memakai kind yang sama).
- Operasi latar tidak diblokir. Start sistem membaca desired lagi di bawah lock dan berhenti kalau bukan `running`, jadi start yang antre di belakang Stop All tidak menyalakan lagi instance yang baru dihentikan. Tanpa aturan ini, startup transaction menulis desired `running` untuk setiap pemicu. Sebagai jaring pengaman, daemon menjalankan `select` sekali lagi sebelum `exiting: true`.
- Instance `already_stopping` ikut dapat langkah, supaya daemon tidak keluar selagi stop lain masih berjalan (AC-20).
- Konfirmasi dari `instance.preview_stop_all`. Kalau `skipped` tidak kosong, Turn Off nonaktif (CLI keluar `conflict` tanpa request), supaya Turn Off tidak menghentikan setengah instance lalu tetap gagal. Balasan `exiting: false` tetap ada untuk keadaan yang berubah setelah konfirmasi.
- `shutdown_requested` diset saat `stop` diterima, saat rencana restart di commit `stopping`, atau saat `agent_off` memutuskan `exiting: true`, jadi logout selama quiesce atau selama `agent_off` masih berjalan tetap *Akhir sesi*. Tabel *Sinyal per konteks* kini memakai flag itu, dan kalimat AC-11 "`daemon.shutdown` yang diterima lebih dulu" dibaca sebagai flag itu.

### Keputusan 13: cara `stop_all_instances` memegang instance

#### Option 1: semua key saat `begin`, start dan restart yang terbuka dibatalkan dulu (dipilih)

Engine mengambil key hanya saat `begin`, dan antrean FIFO menahan operasi yang lebih baru di key yang sama. Jadi satu start yang menunggu readiness akan menahan seluruh Stop All. Membatalkan start dan restart yang terbuka di instance terpilih (seperti *Akhir sesi* langkah 2) membuat penantian itu pendek, dan cocok dengan arti Stop All. Satu langkah journal `stop` dengan jalur stop supervisor langsung, seperti spec 0030, karena journal hanya mengizinkan satu langkah terbuka dan nama langkah harus unik. Resource `Manager` membuat Stop All tidak bertabrakan dengan restart daemon.
**Cons**: operasi lain di instance itu (edit config, reload) tetap ditunggu sampai selesai.

#### Option 2: satu operasi `stop_instance` per instance tanpa `extra_locks`

**Pros**: lock per instance, seperti *Akhir sesi*. **Cons**: hasil, cancel, dan replay harus dirakit dari banyak operasi, dan tidak ada satu operasi yang menahan restart daemon.

### Cross check pembaruan ini

Cross check oleh model lain (Sonnet, 2026-11-02, baca saja) menemukan 14 temuan. Lead mengambil rekomendasi architect atas delegasi user, jadi semuanya diterapkan:

- Start latar yang antre di belakang Stop All menyalakan instance lagi: start sistem memeriksa desired di bawah lock, plus `select` terakhir sebelum `exiting: true`.
- `result` hanya disimpan saat `succeed`: pekerjaan menyerap cancel, balasan dibangun dari keadaan akhir.
- Kind belum diklasifikasi: `resumable` false, `CloseInterrupted`, resource `Manager`.
- Satu langkah per instance tidak cocok dengan journal: satu langkah `stop` dengan jalur stop supervisor langsung.
- Key yang dipegang semua sekaligus tertahan start panjang: start dan restart terbuka dibatalkan dulu.
- "Memeriksa lagi" tidak terdefinisi: `select` atas view satu instance, dan instance yang dihapus bukan penghalang.
- Proses tercatat tidak bisa dibaca dari registry: proses hidup berarti lifecycle `starting`, `running`, atau `stopping`, dan view dibaca dalam satu closure `read`.
- Kegagalan `begin` dan pilihan kosong: error tanpa operasi, dan pilihan kosong tetap membuat operasi.
- Gerbang dan replay: flag `bypass_quiesce`, replay dulu, `stop` dan `restart` ditolak selama `agent_off`, dan gerbang tidak dicabut setelah `SIGTERM`.
- AC-11 lawan `shutdown_requested`: tabel memakai flag itu, plus test `SIGTERM` selama quiesce dan selama `agent_off`.
- Konfirmasi Turn Off memakai aturan kedua: `instance.preview_stop_all` dibangun di sini.
- Spec 0032 belum konsisten: baris **Spec 0032** di *Usulan update* dan Follow-up.
- Penantian balasan tanpa batas: timeout interaktif 200 detik dan kirim ulang dengan key yang sama.
- Kecil: jumlah yang sudah berhenti di copy `exiting: false`, `agent_off` sama di konteks manual, `ShutdownAccepted` tidak lagi `Copy`. Teks AC-20 tidak menyebut `idempotencyKey`; itu notasi, dan *Matikan agent* menyebutnya.

### Catatan lain

- Spec 0030 Follow-up baris 1 sudah terjawab: `ShutdownAccepted` diperluas (bukan diganti), `agent_off` ditolak selama quiesce dan rencana `stopping`, dan restore menang atas *Antrean start* selama rencana `restoring`. Pembaruan ini mempertegas yang terakhir (`outcome` masih kosong, `PendingKind::Restore`) dan menaruh pembacaan rencana di *Urutan daemon start*.
- Migration sesi memakai nomor bebas berikutnya, paling kecil 0010, karena 0007 sampai 0009 sudah dipakai spec 0028, 0029, dan 0030.
- Protocol di `main` 2.1 pada 2026-11-02, dan spec 0028 sedang membuat 2.2. Setiap langkah yang mengubah kontrak mengambil minor berikutnya saat dibangun.
- Kamu memilih membangun langkah 2 sampai 9 sebelum spike audit session selesai. Spike (sleep dan dua logout) kamu lakukan sebelum journey 3, dan hasil yang bertentangan dengan asumsi membawa spec ini kembali ke `/architect`. AC-21 tidak diubah.

## Spike native: hasil sementara

Spike audit session (langkah 1) berjalan sejak 2026-11-02. LaunchAgent `com.stackly.spike.asid` dimuat di `gui/502` dan menulis satu baris setiap 5 menit ke `~/Library/Logs/stackly-asid-spike.log`.

| Pengamatan | Hasil sampai 2026-11-02 04:13 UTC |
| --- | --- |
| asid LaunchAgent | 100014 di 41 baris, boot `4564385D-AE91-4FF6-A971-DDD132DA291C`, `auid` 502 |
| asid dari Terminal | 100014, sama dengan LaunchAgent |
| flags | `0x6030` sampai 03:18, lalu `0x2030` sejak 03:23; asid tetap. Penyebabnya belum diamati (bit `0x4000` hilang) |
| sleep | belum dilakukan |
| dua login berturut turut | belum dilakukan |

Sejauh ini asumsi "sama selama sesi" dan "sama dengan sesi Aqua" cocok. "Berbeda per login" belum terbukti. Setelah logout, catat baris baru, lalu `launchctl bootout gui/502/com.stackly.spike.asid` dan hapus plist nya.
