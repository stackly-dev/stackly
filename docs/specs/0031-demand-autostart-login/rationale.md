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
