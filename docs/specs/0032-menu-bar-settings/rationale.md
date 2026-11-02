# 0032. Menu bar dan Settings: rationale

[Kembali ke spec](index.md)

## Context

Stackly adalah aplikasi yang paling sering dipakai tanpa jendela: database dan broker harus hidup sepanjang hari, sementara GUI hanya dibuka sesekali. Sampai sekarang, satu satunya jalan kembali setelah jendela ditutup adalah ikon Dock (spec 0013), dan Settings masih placeholder. Pengguna butuh status sekilas, aksi start dan stop cepat, dan cara mematikan semua service sebelum, misalnya, rapat dengan baterai tipis.

Ada beberapa kekuatan yang saling tarik. Daemon adalah pemilik tunggal registry dan proses, jadi setting yang memengaruhi service harus berlaku walau GUI tidak hidup, dan CLI harus melihat nilai yang sama. Di sisi lain, preferensi tampilan hanya bermakna di GUI. Menghentikan semua service adalah aksi yang luas, dan sejak fitur 31 instance punya beberapa sumber demand (manual, startup, environment), jadi "semua" harus jelas artinya dan tidak boleh berubah diam diam antara yang dilihat pengguna dan yang dijalankan daemon.

Path instance disimpan relatif terhadap root bernama (spec 0005), dan root bawaan berada di `~/Library`. Scope meminta path data, cache, dan log bisa diubah tanpa memindahkan data yang ada, yang berarti registry harus tahu lebih dari satu tempat per jenis. Daemon berjalan sebagai LaunchAgent, sehingga folder yang dilindungi privasi macOS (Desktop, Documents, Downloads, iCloud) atau disk yang bisa dilepas adalah sumber kegagalan yang diam diam.

Privasi adalah janji produk (REQ-046, NFR-016): tidak ada telemetry atau upload tanpa consent. Scope juga melarang baris setting yang tampil seolah bekerja padahal fiturnya belum ada, sementara mockup menggambar baris dari fitur 30, 37, 55, dan 61.

## Options considered

### Keputusan 1: pemilik setting

**Opsi 1 (dipilih): dibagi menurut pemilik.** Setting perilaku service di registry daemon, lewat IPC dan CLI; Appearance dan notifikasi di UserDefaults app.
Pros: daemon menegakkan nilai saat GUI mati; CLI setara; preferensi tampilan tidak membebani kontrak. Cons: dua tempat simpan, dan Settings harus menangani offline untuk sebagian baris.

**Opsi 2: semua di UserDefaults app.** Pros: paling sederhana, tanpa perubahan protocol. Cons: daemon dan CLI buta; autostart default dan lokasi tidak berlaku tanpa GUI, melanggar aturan "hanya daemon yang menulis registry dan memutuskan perilaku".

**Opsi 3: dibagi, tanpa CLI dulu.** Pros: lebih sedikit kerja. Cons: GUI menjadi satu satunya pintu, bertentangan dengan pola paritas GUI dan CLI di setiap fitur sebelumnya.

### Keputusan 2: ganti path data, log, dan cache

**Opsi 1 (dipilih): bisa diubah untuk instance baru, lewat tabel `storage_locations`.** Pros: memenuhi done when scope; path tersimpan sudah relatif, jadi tambahannya hanya satu kolom lokasi; instance lama tidak tersentuh. Cons: migration yang membangun ulang `instance_paths`, dan setiap pemindai harus melihat banyak lokasi.

**Opsi 2: read only plus Reveal in Finder.** Pros: tanpa migration, sesuai mockup. Cons: scope harus diubah, dan pengguna dengan disk utama kecil tidak punya jalan keluar.

**Opsi 3: hanya cache yang bisa diubah.** Pros: cache bisa dibuang, jadi risikonya kecil. Cons: kebutuhan utama (data besar di disk lain) tidak terjawab.

Sub keputusan: lokasi dicatat per instance lewat `location_id` (bukan path absolut disalin ke setiap instance), karena daftar lokasi dibutuhkan Settings, validasi tumpukan, dan larangan menghapus lokasi yang dipakai. Lokasi bawaan menyimpan `path` NULL dan resolve lewat `RootSet`, supaya `STACKLY_ROOT` dan test root tetap bekerja; ini penyempurnaan dari model yang dikonfirmasi, tanpa mengubah entitas atau relasinya. Folder yang dilindungi privasi dan volume removable atau jaringan ditolak di depan, karena kegagalan izin pada LaunchAgent muncul jauh setelah pengguna memilih folder. Cache lama dibiarkan di tempat; pembersihan milik fitur 55.

### Keputusan 3: ikon Dock

**Opsi 1 (dipilih): hilang saat jendela terakhir ditutup.** Pros: Stackly berperilaku seperti utilitas menu bar; ikon menu bar menjadi jalan kembali yang jelas. Cons: perpindahan activation policy harus dijaga di setiap jalur pembuka jendela.

**Opsi 2: selalu tampil.** Pros: tanpa kode baru. Cons: ikon Dock permanen untuk app yang biasanya tanpa jendela.

**Opsi 3: setting "Show in Dock".** Pros: fleksibel. Cons: satu baris dan dua kali state untuk diuji, untuk preferensi yang jarang diubah.

### Keputusan 4: urutan terhadap fitur 31

**Opsi 1 (dipilih): fitur 32 setelah 31.** Pros: preview Stop All jujur sejak awal dan tidak ditulis ulang. Cons: menu bar datang lebih lambat.

**Opsi 2: preview sementara.** Pros: menu bar lebih cepat. Cons: kode `provisional: F31` yang pasti diubah, dan preview yang menyebut "semua" tanpa sumber demand.

**Opsi 3: Settings dulu, menu bar setelah 31.** Pros: sebagian nilai lebih cepat. Cons: dua siklus build untuk satu fitur dan notice tutup jendela tetap menyebut Dock lebih lama.

### Keputusan 5: teknologi menu bar

**Opsi 1 (dipilih): `MenuBarExtra` bergaya window.** Pros: scene SwiftUI native, material sistem, berbagi store yang sama tanpa glue AppKit. Cons: label hanya template, jadi titik gagal tidak berwarna.

**Opsi 2: `MenuBarExtra` bergaya menu.** Pros: paling native. Cons: tanpa preview inline, toggle, atau progress; jauh dari mockup.

**Opsi 3: `NSStatusItem` plus `NSPopover`.** Pros: kontrol penuh atas ikon. Cons: lebih banyak AppKit di app SwiftUI, dan perilaku popover harus dirawat sendiri.

### Keputusan 6: Stop All

**Opsi 1 (dipilih): `instance.stop_all` spec 0031, ditambah preview dan `expectedInstanceIds`.** Spec 0031 (ditulis sesi lain selama percakapan ini) sudah memutuskan method, operasi, pilihan instance, dan `StopAllResult`. Spec ini hanya menambah `instance.preview_stop_all` dan himpunan ID yang diharapkan, dengan satu fungsi pilihan untuk keduanya. Pros: satu kontrak, tanpa hash, dan jaminan bahwa yang berhenti selalu bagian dari preview. Cons: menambah field ke spec lain yang masih Proposed.

**Opsi 2: spec 0031 apa adanya, preview saja.** Pros: tanpa perubahan kontrak 0031. Cons: instance yang menyala setelah preview ikut berhenti tanpa pernah tampil.

**Opsi 3 (draft pertama spec ini): method dan fingerprint hash sendiri.** Pros: mandiri. Cons: menggandakan kontrak spec 0031; dibuang setelah cross check.

Sub keputusan yang dipilih engineer: semua instance aktif dihitung sebagai stop manual (bukan melewati yang dipinjam environment; spec 0031 tetap melewati `unknown` dan lease environment `active` sebagai `skipped`), konfirmasi inline di popover, dan preview usang ditolak lalu digambar ulang.

### Keputusan 6a: retention log

**Opsi 1 (dipilih): baris read only dari konstanta spec 0027.** Spec 0027 (sudah In Progress) memutuskan rotasi ukuran 5 × 20 MiB per stream tanpa batas umur. Pros: tanpa membuka ulang spec yang sedang dibangun; angka di Settings selalu sama dengan footer tab Logs. Cons: retention tidak bisa diatur, berbeda dari mockup.

**Opsi 2: pilihan hari, spec 0027 menambah prune umur.** Pros: sesuai mockup. Cons: membuka ulang spec 0027 dan menambah aturan kedua di atas batas ukuran.

**Opsi 3: pilihan ukuran.** Pros: satu dimensi. Cons: konstanta spec 0027 menjadi setting, dan membuka ulang spec itu.

### Keputusan 7: baris untuk fitur yang belum dibangun

**Opsi 1 (dipilih): sembunyikan, kecuali preferensi tersimpan.** Pros: tidak ada kontrol yang tidak melakukan apa pun; channel dan telemetry tetap bisa dipilih dengan catatan jujur. Cons: Settings tampak lebih kosong dari mockup.

**Opsi 2: tampil nonaktif dengan catatan.** Pros: dekat dengan mockup. Cons: melanggar done when scope tentang baris yang tampil seolah bekerja.

**Opsi 3: sembunyikan semua.** Pros: paling ketat. Cons: telemetry mati secara default tidak terlihat, padahal itu janji yang ingin dilihat pengguna.

### Keputusan kecil lain yang dipilih engineer

Ikon template dengan titik gagal dan varian offline; daftar sampai 8 dengan yang gagal di atas; toggle baris langsung start atau stop; baris Quit Stackly di popover; autostart default mati (sekarang disimpan sebagai policy `startup` spec 0031); tiga jenis notifikasi (instance gagal, termasuk readiness hilang, operasi panjang selesai, daemon tidak terjangkau); baris agent memakai model spec 0015; satu baris `daemon_settings` bertipe; tiga method plus event; Open Stackly at login sebagai toggle terpisah; folder terlindungi ditolak; Settings offline menampilkan nilai terakhir; konflik revision ditolak; izin notifikasi yang ditolak tampil sebagai baris status.

### Keputusan yang diambil saat menulis spec (RECOMMEND)

- **Satu toggle notifikasi, pemicu tetap**, bukan toggle per jenis. Mockup punya satu baris, dan tiga jenis yang dipilih semuanya jarang. Alternatif: tiga sub toggle, ditunda sampai ada permintaan.
- **Toggle langsung berlaku tanpa Apply**, mengikuti konvensi Settings macOS. Alternatif: tombol Apply per tab, yang asing di macOS.
- **Semua method baru `recovery refused`**, karena registry dalam recovery bisa tidak terbaca. Alternatif: `settings.get` allowed, yang menambah jalur baca registry dalam recovery tanpa kebutuhan nyata.
- **Create memakai lokasi default yang dibaca di dalam guard**, bukan lokasi yang dilaporkan preflight. Kedua lokasi valid dan path nya tampil di detail; menambah parameter lokasi ke create hanya memindahkan masalah balapan. Alternatif: `expectedDataLocation` di create.
- **Popover tanpa shortcut sendiri**, karena kebijakan interaksi proyek (2026-10-24) hanya mempertahankan aksi menu biasa; ⌘, dan ⌘Q tetap di menu app.
- **Deteksi peluncuran login item lewat Apple event**, diperiksa di macOS 27 pada langkah build 11. Alternatif: argumen peluncuran khusus, yang tidak didukung `SMAppService.mainApp`.

## Rationale

Pembagian pemilik setting mengikuti aturan inti proyek: daemon pemilik perilaku, GUI dan CLI hanya klien. Autostart default dan lokasi harus berlaku saat GUI tidak hidup, jadi tempatnya di registry; Appearance tidak punya arti di luar GUI, jadi tempatnya di UserDefaults. Revision dan `commit_guarded` dipakai karena setiap tulis registry lain di proyek ini memakai pola yang sama, dan konflik antara GUI dan CLI nyata terjadi di satu Mac dengan dua klien.

Lokasi yang bisa diubah untuk instance baru adalah jalan termurah yang tetap memenuhi scope, karena spec 0005 sudah menyimpan path relatif terhadap root bernama. Menambah lokasi sebagai entitas (bukan menyalin path absolut) membuat aturan tumpukan, jumlah pemakai, dan larangan hapus bisa dicek di satu tempat. Menolak folder terlindungi dan disk yang bisa dilepas di depan lebih baik daripada membiarkan LaunchAgent gagal berminggu minggu kemudian dengan error izin yang membingungkan.

Stop All sebagai satu operasi dengan fingerprint adalah satu satunya bentuk yang menjamin janji preview: pengguna melihat daftar, dan daemon menolak kalau daftar itu sudah tidak benar. Menunggu fitur 31 memberi preview yang jujur tentang sumber demand dan tentang apa yang terjadi di login berikutnya, dengan biaya menu bar datang lebih lambat. Ikon Dock yang hilang saat jendela ditutup, ditambah Open Stackly at login yang terpisah dari agent daemon, membuat Stackly berperilaku seperti utilitas menu bar tanpa pernah membuat service bergantung pada GUI.

## Cross check

Dijalankan 2026-11-02 dengan model lain (read only). Temuan dan penyelesaian, semuanya disetujui engineer:

- Spec 0027 dan 0031 ternyata sudah ditulis sesi lain selama percakapan: retention menjadi read only (Keputusan 6a), dan Stop All, suppression, autostart (`startup`), serta Turn Off (`agent_off`) memakai kontrak spec 0031 (Keputusan 6).
- Migration: kolom `instance_paths` dirinci, ID lokasi bawaan tetap, trigger kecocokan jenis lokasi, `foreign_key_check`, test gagal di tengah, dan *AC-10 per lokasi*. Lokasi bawaan menyimpan `path` NULL.
- Aturan lokasi diberi urutan, `foreign_marker` dipisah dari `overlaps_root`, `VolumeFacts.internal` ditambah (sebelumnya hanya `local`), disk external ditolak dengan kalimat jujur, layout cache dipisah, dan `LocationTarget::builtIn` (Use Default) menggantikan perbandingan path bawaan.
- Kontrak: daftar field DTO lengkap, `resource.revision_mismatch` yang sudah ada dipakai, revision dicek di precheck dan di guard, patch tanpa perubahan tanpa event, refetch saat event, gap, dan reconnect, kategori dan exit code setiap error baru, serta versi protocol "satu minor dari versi saat dibangun".
- Copy dan aturan UI: urutan dan tie break daftar service, tabel tombol baris, kind operasi di popover, copy Stop All dengan bentuk jamak, tabel pemicu notifikasi (semua hanya saat app tidak aktif), dan copy offline per tab.
- macOS: item menu bar bisa dilarang pengguna sejak macOS 26 (AC-1 dirumuskan ulang dengan jalan kembali lewat Finder), menu app hilang di mode `accessory` (Settings dan Quit lewat popover), fallback jendela utama kalau peluncuran login item tidak dikenali, delegate notifikasi dipasang saat launch, dan `openSettings` dipanggil setelah policy `regular`.
- Build plan: test aturan sumber spec 0012 untuk UserNotifications dan ServiceManagement, dan daftar pemakai path instance.
- Telemetry menyimpan `telemetry_copy_version`, supaya persetujuan diminta lagi kalau teks berubah.
