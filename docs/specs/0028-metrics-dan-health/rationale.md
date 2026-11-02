# 0028. Metrics dan health: rationale

[Kembali ke spec](index.md)

## Context

Fitur 28 (scope `04-kerja-harian.md`) meminta status, readiness, process tree, uptime, restart count, kegagalan terakhir, CPU, memori, dan disk terjadwal, dengan label cakupan yang jelas antara proses utama dan agregat child (REQ-018, AC-018). Sebagian besar status sudah ada dari spec 0010 dan 0021: lifecycle, readiness dan health dengan `observedAt` dan reason, `statusSeq`, `lastFailure`, status restart policy di jendela 5 menit, `startedAt` proses, dan pemindaian ukuran data on demand (`instance.get_data_usage`). Yang belum ada adalah pembacaan sumber daya proses: `ProcessFacts` tidak punya CPU atau memori, adapter macOS belum memanggil `proc_pid_rusage` atau `proc_pidinfo` untuk pemakaian, dan tidak ada method IPC, perintah CLI, atau tab GUI untuk metrik.

Ada beberapa gaya yang saling tarik. NFR-003 membatasi daemon idle pada 150 MiB RSS dan 1 persen satu core dengan 10 instance idle, dan sampling yang selalu berjalan menambah kerja tetap. Event bus spec 0006 adalah ring buffer bersama; event yang terlalu sering mendorong event lain keluar dan memaksa client resync. PostgreSQL adalah provider multiprocess yang memakai shared memory, sehingga menjumlah memori per proses bisa menyesatkan. PRD 19.5 meminta sleep tidak dibaca sebagai crash, dengan freshness atau debounce dan pemeriksaan ulang endpoint tanpa mengganti port fixed, padahal readiness sekarang menjadi stale setelah 30 detik dan Mac yang bangun bisa punya observasi berumur jam.

Mockup menggambar lima kartu (CPU, Memory "Resident", Disk "Scanned", Disk I/O sebagai capability provider, Restarts "Since created"), grafik 60 sampel "every 2 s", daftar proses, dan toggle Main Process atau Process Tree. Scope menang atas mockup untuk perilaku, dan scope mengusulkan 5 detik. Konstanta `READINESS_REFRESH_INTERVAL` dan `READINESS_MAX_AGE` ditandai `// provisional: F28`, jadi fitur ini juga harus menetapkannya.

Tanpa keputusan, `/develop` harus menebak arti angka CPU, memori mana yang ditampilkan, siapa anggota pohon, kapan sampel diambil, cara client menerimanya, dan apa yang terjadi setelah wake; setiap tebakan yang salah langsung terlihat pengguna sebagai angka yang tidak jujur.

## Options considered

### Keputusan 1: satuan CPU

1. **Persen satu core (dipilih)**. Seperti Activity Monitor dan `top`. Pro: angka yang sudah dibaca developer Mac; satu query sibuk terlihat jelas. Kontra: bisa lebih dari 100, yang perlu dipahami untuk pohon.
2. **Persen seluruh Mac**. Pro: selalu 0 sampai 100. Kontra: satu core penuh di Mac 12 core tampak 8 persen.
3. **Keduanya**. Pro: lengkap. Kontra: lebih banyak teks di setiap kartu dan baris.

### Keputusan 2: angka memori

1. **Physical footprint (dipilih)**. Pro: angka Memory di Activity Monitor; tidak menghitung ulang memori bersama per child, jadi jumlah pohon PostgreSQL masuk akal. Kontra: berbeda dari RSS yang dikenal sebagian orang dan dari label mockup; perilakunya pada shared memory PostgreSQL harus dibuktikan lewat spike langkah 0.
2. **RSS**. Pro: label mockup, dikenal luas. Kontra: menjumlah RSS pohon PostgreSQL menghitung `shared_buffers` sekali per child, sangat berlebihan.
3. **Footprint di kartu, RSS per baris**. Pro: keduanya terlihat. Kontra: dua definisi di satu layar membingungkan.

### Keputusan 3: Disk I/O

1. **Diukur untuk semua provider (dipilih)**. `proc_pid_rusage` memberi byte baca dan tulis per proses. Pro: tidak perlu capability manifest; fakta dari OS. Kontra: kartu kelima yang tidak disebut scope.
2. **Capability provider seperti mockup**. Pro: provider bisa menyatakan dukungan. Kontra: perubahan schema manifest untuk data yang OS sudah berikan, dan klaim "tidak tersedia" yang tidak benar.
3. **Tidak di fitur 28**. Pro: lebih sedikit kerja. Kontra: mockup berselisih lebih jauh.

### Keputusan 4: panjang history

1. **60 sampel, sekitar 5 menit (dipilih)**. Pro: persis yang digambar grafik; memori kecil; sesuai PRD 35.2. Kontra: tidak bisa melihat lebih jauh ke belakang.
2. **360 sampel**. Pro: 30 menit untuk CLI. Kontra: grafik tidak bisa menampilkannya.
3. **720 sampel**. Pro: satu jam. Kontra: sama, tanpa tempat tampil.

### Keputusan 5: anggota pohon proses

1. **Turunan proses utama lewat link parent (dipilih)**. Pro: menangkap backend PostgreSQL walau process group berubah; identitas per PID dan waktu start. Kontra: pohon dibangun ulang setiap sampel (satu daftar PID per tick untuk semua instance, lihat *Cross check* 7).
2. **Anggota process group**. Pro: memakai `group_members` yang sudah ada. Kontra: child yang memanggil `setsid` atau `setpgid` hilang diam diam.
3. **Gabungan keduanya**. Pro: paling lengkap. Kontra: dua lookup, dan proses asing di group bisa ikut dihitung.

### Keputusan 6: kapan sampling berjalan

1. **Selalu setiap 5 detik selama running (dipilih)**. Pro: history sudah ada saat tab dibuka; selisih CPU benar; beberapa syscall per proses. Kontra: kerja tetap walau tidak ada yang melihat; harus dibuktikan AC-14.
2. **5 detik saat ada client, 30 detik selain itu**. Pro: biaya idle lebih rendah. Kontra: grafik mulai jarang; daemon harus melacak siapa yang melihat.
3. **Hanya saat ada client**. Pro: biaya terendah. Kontra: tidak ada history, CPU butuh dua sampel sebelum tampil.

### Keputusan 7: ukuran data

1. **On demand ditambah setiap 15 menit selama running (dipilih)**. Pro: terjadwal seperti scope, memakai pemindaian dan batas yang sudah ada, cukup jarang. Kontra: angka bisa berumur sampai 15 menit.
2. **On demand saja**. Pro: tidak ada kode baru. Kontra: tidak terjadwal, CLI bisa melapor angka lama.
3. **Setiap 5 menit**. Pro: lebih segar. Kontra: folder data PostgreSQL yang besar lebih sering dibaca daripada saran PRD.

### Keputusan 8: deteksi wake

1. **Selisih jam dinding dan jam monotonic (dipilih)**. Pro: tanpa run loop dan `unsafe` baru; bisa diuji dengan clock palsu dan tokio paused. Kontra: terlambat sampai 2 detik; perubahan jam dinding ke depan terbaca sebagai wake.
2. **Notifikasi IOKit**. Pro: tepat dan langsung. Kontra: thread CFRunLoop dan blok `unsafe` baru di platform macOS, dan sulit diuji tanpa sleep sungguhan.
3. **Keduanya**. Pro: paling tahan. Kontra: paling banyak kode untuk manfaat kecil.

### Keputusan 9: perilaku setelah wake

1. **Probe langsung dengan masa tenggang 30 detik (dipilih)**. Pro: sleep bukan bukti crash; instance yang sehat tetap tampil sehat; kegagalan sementara jaringan setelah wake tidak dihitung. Kontra: instance yang benar benar mati setelah wake baru terlihat gagal setelah masa tenggang.
2. **Unknown sampai probe pertama**. Pro: paling jujur. Kontra: setiap instance berkedip unknown setiap kali Mac bangun.
3. **Ambang biasa**. Pro: paling sederhana. Kontra: stack jaringan yang lambat setelah wake bisa menandai instance sehat sebagai gagal.

### Keputusan 10: tempat jumlah restart

1. **Kolom registry di commit yang sama (dipilih)**. Pro: bertahan setelah daemon restart; tidak ada commit tambahan. Kontra: satu migrasi.
2. **Dihitung dari observasi proses**. Pro: tanpa migrasi. Kontra: retensi 20 observasi membuat angka salah setelah 20 restart.
3. **Memori daemon**. Pro: tanpa migrasi. Kontra: reset setiap daemon restart, bukan "sejak dibuat".

### Keputusan 11: cara client menerima sampel

1. **Method dengan cursor (dipilih)**. Pro: sampel tidak masuk ring buffer bus; client hanya bertanya selama tab terlihat. Kontra: polling, jeda sampai satu interval.
2. **Event di bus**. Pro: push langsung. Kontra: 10 instance setiap 5 detik memenuhi ring buffer dan memicu resync untuk semua event lain.
3. **Stream metrik terpisah**. Pro: push tanpa bus. Kontra: jenis stream baru di protocol dan transport untuk data yang cukup dibaca setiap 5 detik.

### Keputusan 12: bentuk CLI

1. **`metrics` dengan `--json` dan `--watch` (dipilih)**. Pro: satu snapshot untuk script, watch untuk manusia, sama dengan pola `logs --follow`. Kontra: satu mode watch lagi yang perlu diuji.
2. **`--json` saja**. Pro: sederhana. Kontra: watch harus dibuat pengguna sendiri.
3. **Masuk ke `inspect`**. Pro: tanpa perintah baru. Kontra: scope menyebut `stackly instance metrics --json`.

### Keputusan 13: restart yang dihitung

1. **Restart otomatis saja (dipilih)**. Pro: sinyal masalah yang bersih. Kontra: tidak sama dengan arti harfiah "semua restart".
2. **Otomatis dan pengguna**. Pro: arti harfiah. Kontra: crash loop tersembunyi di antara restart pengguna.
3. **Dua penghitung**. Pro: paling jelas. Kontra: satu field lagi di setiap permukaan.

### Keputusan 14: konstanta probe

1. **Tetap 10 dan 30 detik (dipilih)**. Pro: sudah dipakai dan masuk anggaran. Kontra: tidak ada.
2. **5 dan 15 detik**. Pro: sejalan dengan sampling. Kontra: dua kali lipat probe ke provider.
3. **Ditentukan provider**. Pro: fleksibel. Kontra: perubahan schema manifest.

### Keputusan 15: isi baris proses

1. **PID, nama executable, memori, CPU (dipilih)**. Pro: CPU per baris membantu menemukan backend yang sibuk; tidak ada argv. Kontra: satu kolom lebih dari mockup.
2. **PID, nama, memori (mockup)**. Pro: sama dengan mockup. Kontra: tidak tahu proses mana yang sibuk.
3. **PID, command line penuh, memori**. Pro: judul proses PostgreSQL informatif. Kontra: argv bisa membawa nilai yang tidak ingin ditampilkan.

### Keputusan 16: tempat latensi probe

1. **Baris Readiness Summary dan header Metrics (dipilih)**. Pro: menutup Follow-up spec 0021 dan mengikuti mockup; ikut terlihat di CLI. Kontra: field baru di dua DTO.
2. **Header Metrics saja**. Pro: lebih kecil. Kontra: Follow-up spec 0021 tetap terbuka.
3. **Tidak di fitur 28**. Pro: paling kecil. Kontra: mockup dan spec 0021 berselisih lebih lama.

## Rationale

Garis besarnya mengikuti dua gaya terkuat di Context: angka harus jujur, dan biaya harus terbukti. Persen satu core, footprint, dan Disk I/O dari `proc_pid_rusage` adalah angka yang sama dengan yang sudah dipercaya developer di Activity Monitor, dan semuanya datang dari satu syscall per proses, sehingga biaya sampling tetap kecil walau berjalan terus. `Metric` tanpa nilai default membuat "unavailable, bukan nol" menjadi aturan tipe, bukan disiplin di setiap layar. Risiko terbesar adalah footprint pada shared memory PostgreSQL, karena itulah spike langkah 0 menjadi gerbang: kalau jumlah pohon ternyata berlebihan, keputusan memori kembali ke `/architect` sebelum kode ditulis.

Sampel dibaca lewat method dengan cursor karena event bus adalah sumber daya bersama. Sampling yang selalu berjalan ditambah polling hanya saat tab terlihat memberi grafik yang langsung penuh tanpa menyentuh ring buffer, dan biayanya diukur oleh AC-14 dengan dan tanpa client. Pemindaian data tetap jarang (15 menit) karena itu satu satunya pekerjaan metrik yang mahal.

Untuk wake, selisih dua jam dipilih karena jam monotonic tokio sudah berhenti selama sleep (restart policy bergantung padanya), jadi deteksi tidak butuh API OS baru, `unsafe`, atau run loop, dan bisa diuji dengan clock palsu. Masa tenggang 30 detik sama dengan `READINESS_MAX_AGE`, sehingga observasi `waking` menjadi stale lewat aturan `effective` yang sudah ada kalau tidak ada probe yang berhasil, tanpa perubahan pada domain. `restart_total` ditulis di commit spawn yang sudah ada karena registry adalah satu satunya tempat yang bertahan setelah daemon restart, dan hanya restart otomatis dihitung karena yang ingin dilihat pengguna adalah tanda crash.

## Keputusan kecil

- `MissedTickBehavior::Skip` untuk sampler: tick yang terlewat tidak menumpuk; runner up `Delay`, yang menggeser cadence.
- Semua pembacaan OS satu tick dalam satu `spawn_blocking`: satu thread per tick, bukan per instance; runner up satu task per instance, lebih banyak wakeup.
- `latencyMs` tidak ikut `differs_visibly`: event status tetap jarang; runner up event per probe, yang mengirim event setiap 10 detik per instance.
- Proses ikut selisih hanya kalau identitasnya ada di dua sampel: tidak ada lonjakan palsu dari proses baru; runner up menghitung dari nol, yang membuat backend baru tampak memakai seluruh waktu CPU hidupnya dalam satu sampel.
- Grafik Swift Charts: framework Apple tanpa dependency; runner up bentuk SwiftUI biasa seperti mockup.
- Toggle default Main Process: sama dengan mockup; runner up Process Tree.

## Cross check

Pada 2026-11-02 model lain (Sonnet) membaca spec ini terhadap kode dan spec 0010, 0021, 0026, dan 0027, dan user menerima semua perbaikan yang direkomendasikan:

1. Cache ukuran data ber TTL 60 detik membuat jadwal 15 menit hampir selalu tampil `not_scanned`: ditambah slot `last_scan` tanpa TTL, pindai pertama saat instance mulai running, hasil parsial dihitung pindaian (AC-10).
2. `seq` yang mulai lagi dari 1 per proses tidak bisa membedakan cursor lama: ditambah `historyId` dan aturan batas cursor (AC-11).
3. 60 sampel dengan daftar proses penuh bisa melewati frame 1 MiB: daftar proses hanya di sampel terbaru, paling banyak 200 baris, dengan test ukuran; fakta `inspect` di cache per identitas (AC-5, AC-11).
4. Jalur respawn adalah `StartRequest::respawn`, bukan sekadar trigger: kenaikan memakai penanda crash di `StartRequest`, termasuk percobaan ulang di dalam operasi, spawn gagal tidak dihitung, struct `stored` ikut (AC-6).
5. Menyalin state lama dengan `observed_at` baru mengklaim Ready yang belum diperiksa: observasi `waking` menjadi `unknown` sampai probe pertama (biasanya kurang dari satu detik), masa tenggang tetap untuk kegagalan, grace terpisah untuk readiness dan health, aturan untuk `starting`, `stopping`, dan `recovering`, dan lama sleep dicatat di log (AC-9). Ini mengubah sebagian pilihan Keputusan 9: tetap probe langsung dengan masa tenggang, tetapi state yang tampil selama probe pertama adalah `unknown`, bukan state lama.
6. Reset baseline sampler saat wake tidak perlu karena kedua jam berhenti selama sleep: dihapus, dan spike membuktikan perilaku jam itu.
7. Aturan jumlah pohon: jumlah proses yang terukur dengan `measuredCount`, dan satu `proc_listallpids` per tick dengan parent dari `inspect` menggantikan `proc_listchildpids` berulang (AC-3, AC-4, AC-5).
8. Pemetaan state: `live` hanya untuk `running` dengan proses terverifikasi, `not_running` membawa lifecycle, tombol Start mengikuti `InstanceActionModel`, selisih waktu diambil di sekitar pembacaan (AC-1, AC-2, AC-11, AC-13).
9. Tipe wire dan teks tampil: CPU number satu desimal, byte sebagai `Count`, teks tetap per reason termasuk fallback, urutan pohon, grafik mengikuti scope, format memori (AC-13).
10. Sumber uid daemon: capability `UserIdentity`, sekali saat sampler mulai (AC-5).
11. Evidence NFR-003 diukur dari luar daemon dan ditambah run dengan satu PostgreSQL (AC-14).
12. Latensi untuk probe gagal adalah waktu sampai gagal, per percobaan (AC-7).

## Hasil spike

Dijalankan 2026-11-02 di Mac ini (macOS 27, Apple Silicon) dengan program C kecil yang memanggil `proc_pid_rusage(RUSAGE_INFO_V4)`, `proc_pidinfo(PROC_PIDTBSDINFO)`, dan `proc_listallpids`, pada PostgreSQL 17.11 r2 dan NATS 2.15.1 dari cache runtime xtask, di folder sementara.

- **Unit waktu CPU**: `ri_user_time` dan `ri_system_time` dalam mach time, bukan nanodetik. `mach_timebase_info` di Mac ini menjawab 125/3, jadi nilai mentah harus dikali `numer / denom` sebelum dipakai. Adapter wajib mengonversi; mengira nanodetik membuat CPU 41 kali terlalu kecil.
- **Footprint per child**: dengan `shared_buffers=256MB`, satu backend membaca tabel 184 MB dua kali lewat shared buffers. Footprint backend itu 19,3 MB (resident 24 MB), postmaster 21,0 MB, dan setiap proses latar belakang 2 sampai 10 MB. Shared buffers tidak dihitung di footprint child mana pun (dan tidak di postmaster), jadi jumlah `tree` tidak menghitung memori bersama dua kali. Keputusan memori tetap; tidak kembali ke `/architect`. Akibatnya: footprint `tree` tidak memuat shared buffers sama sekali, yang sesuai dengan angka Memory di Activity Monitor.
- **Pohon**: PostgreSQL 17 punya postmaster dan lima proses latar belakang saat idle, ditambah satu backend per koneksi, semua dengan parent postmaster. NATS satu proses tanpa child.
- **Disk I/O**: `ri_diskio_bytesread` tetap 0 saat backend membaca dari page cache OS; hanya I/O yang sampai ke disk yang dihitung. Penulisan terlihat (backend 1 MiB, checkpointer 16 KiB). Nilai Disk I/O berarti I/O disk fisik, bukan pembacaan file.
- **`EPERM`**: `proc_pid_rusage` pada pid 1 (milik root) menjawab `Operation not permitted`, sesuai pemetaan `not_permitted`.
- **`proc_listallpids(NULL, 0)`** menjawab perkiraan jumlah (sekitar 800), jadi buffer pertama bisa dibuat dari angka itu ditambah ruang.
- **`ri_proc_start_abstime`** juga dalam mach time; identitas proses tetap memakai waktu start yang sudah dipakai `inspect`.
- **Jam selama sleep**: tidak diuji dengan sleep sungguhan di sesi ini (Mac tidak bisa ditidurkan dari sesi agent). Dari sumber: `std::time::Instant` di macOS memakai `CLOCK_UPTIME_RAW`, yang berhenti selama sleep, dan `tokio::time::Instant` membungkusnya; waktu CPU proses tidak maju karena proses tidak jalan. Bukti dengan sleep sungguhan ikut cek manual di langkah 9 (AC-15), dan tetap `NOT_RUN` sampai itu.
