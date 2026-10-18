# 0010. Rationale: process supervisor

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: fitur 10 ditulis bersamaan dengan spec 0007 (operation engine), jadi pemakaian API operasi (begin, stage, cancel, lock per resource, operasi terputus) ditulis sebagai baris Assumed, bukan diputuskan di sini. Spec ini juga sengaja mengambil method IPC instance minimal, termasuk create, dari fitur 20, 21, dan 26, karena walking skeleton (fitur 14) menutup fase Foundation dan butuh `stackly instance create`, `start`, dan `stop` sebelum fitur fitur itu dirancang. Risikonya, create minimal di sini harus diperluas fitur 20 secara additive saja. Tiga celah di spec lain yang sudah diterima (0005 tanpa op untuk observasi proses dan config revision, 0006 tanpa aturan untuk event yang tidak menaikkan revision, 0009 tanpa langkah render sebelum spawn dan dengan lokasi file secret yang berbeda dari 0011) dicatat sebagai Follow-up, bukan diubah di sini.

Stackly menjalankan server database dan messaging native milik pengguna di Mac yang sama, tanpa container. Satu daemon per user memegang semua proses. PRD bagian 19 meminta startup transaction yang berurutan, tiga state yang terpisah (lifecycle, readiness dan health, desired), stop yang graceful tanpa `SIGKILL` sebagai default, dan rekonsiliasi setelah daemon restart yang tidak pernah memulai server kedua pada data yang sama. Restart atau crash daemon tidak dijanjikan zero downtime: platform bisa menghentikan child, atau proses bisa tetap hidup, dan keduanya harus ditangani. Proses yang selamat hanya boleh diadopsi setelah PID, waktu start, executable atau runtime, user, boot atau session, dan asosiasi data terbukti; yang tidak terbukti menjadi `unknown` tanpa dibunuh.

Risiko yang paling mahal di sini adalah kehilangan data dan salah bunuh. Dua supervisor atau dua start yang berlomba bisa membuat dua server menulis ke folder data yang sama (RISK-03). Status yang berasal dari PID atau cache lama bisa membuat Stackly membunuh proses milik orang lain atau mengaku ready padahal belum (RISK-14). Provider yang terus crash tidak boleh membuat busy loop atau child ganda (AC-068), dan anggaran daemon idle hanya 150 MiB dan 1 persen CPU untuk 10 instance (NFR-003).

Spec lain sudah mengunci banyak bagian di sekitarnya. Spec 0004 menetapkan lifecycle instance dan aturan readiness serta health, termasuk `effective` untuk kesegaran dan bahwa stop yang melewati timeout tetap `stopping`. Spec 0005 menyediakan tabel `process_observations` yang bukan sumber kebenaran tunggal dan tidak pernah mengotorisasi kill. Spec 0006 menyediakan envelope error, event dengan cursor, dan slot start untuk rekonsiliasi. Spec 0009 memberi plan yang sudah tervalidasi (argv array, env tertutup, timeout per fase) dan menyerahkan semua efek samping serta apa yang terjadi saat setiap timeout lewat ke spec ini. Spec 0011 menetapkan cara secret sampai ke provider dan bahwa log instance disamarkan sebelum disimpan. Yang belum diputuskan: nasib proses saat daemon mati, ke mana output proses mengalir, bagaimana identitas dan asosiasi data dibuktikan di macOS, angka dan perilaku restart policy, perilaku saat setiap timeout lewat, dan permukaan IPC instance.

Pembaruan 2026-10-18: spike milestone 1 langkah 1 di Mac ini membuktikan hampir setiap baris tabel *Adapter macOS*, tetapi membantah dua baris seperti tertulis (Spool, dan fakta proses milik user lain) dan membuat angka 2 MiB di AC-12 tidak jelas. Seperti aturan spec ini untuk baris yang gagal, ketiganya kembali ke `/architect` sebelum langkah 3 selesai dan diputuskan di Keputusan 19, 20, dan 21. Bukti lengkapnya ada di *Spike milestone 1 langkah 1* di akhir file ini.

## Options considered

### Keputusan 1: nasib proses instance saat daemon crash atau restart

#### Option 1: Tetap hidup, diadopsi lewat identitas majemuk

Proses dijalankan di session dan process group sendiri dengan stdio yang tidak terikat ke daemon; setelah daemon hidup lagi, proses yang identitasnya terbukti diadopsi.

**Pros**:

- Crash daemon tidak memutus koneksi database pengguna.
- Jalur adopsi yang sama bisa dipakai fitur 30 untuk update daemon tanpa downtime.
- Sesuai PRD 19.4 yang meminta adopsi hanya setelah identitas terbukti.

**Cons**:

- Pembuktian identitas di macOS butuh beberapa API libproc dan kqueue, dan proses yang diadopsi bukan child sehingga exit code nya tidak bisa dibaca.

#### Option 2: Tetap hidup, lalu restart terkontrol

Proses yang selamat dan terbukti dihentikan graceful lalu dijalankan ulang supaya terpasang lagi ke daemon.

**Pros**:

- Perilaku setelah crash daemon selalu sama, dan stdio bisa memakai pipe biasa.

**Cons**:

- Setiap crash daemon menjadi restart semua instance.
- Tetap butuh pembuktian identitas sebelum stop, jadi kompleksitas adopsi tidak hilang.

#### Option 3: Mati bersama daemon

Proses tetap di process group daemon sehingga launchd menghentikannya saat job daemon keluar.

**Pros**:

- Paling sedikit state yang harus dipulihkan dalam kasus normal.

**Cons**:

- Database bisa dihentikan paksa dan butuh crash recovery.
- Kematian tidak dijamin (test tanpa launchd, sinyal yang diabaikan), jadi kode `unknown` tetap dibutuhkan.

### Keputusan 2: ke mana stdout dan stderr proses utama mengalir

#### Option 1: Spool di folder run, dibaca daemon

File append di runtime root yang ditulis proses langsung; daemon membaca, menyamarkan, menyerahkan ke sink, lalu membebaskan ruang dengan punch hole.

**Pros**:

- Proses tidak pernah tertahan oleh daemon yang lambat atau mati.
- Output mentah tidak pernah masuk penyimpanan tahan lama, jadi aturan redaksi spec 0011 terpenuhi.
- Tidak ada baris hilang saat ruang dibebaskan.

**Cons**:

- Butuh `F_PUNCHHOLE` dan `SEEK_DATA` yang harus dibuktikan di APFS, dan offset baca setelah restart bisa mengulang paling banyak satu satuan punch ditambah satu batch baca. Spike kemudian menunjukkan bahwa pembaca butuh fd baca tulis dan satuan punch selebar halaman VM (Keputusan 19).

#### Option 2: File langsung di `logs/instances/<id>/`

Proses menulis langsung ke folder log tahan lama.

**Pros**:

- Paling sederhana dan tetap hidup saat daemon mati.

**Cons**:

- Log mentah tersimpan permanen tanpa redaksi.
- Rotasi hanya lewat copytruncate yang bisa kehilangan baris.

#### Option 3: Pipe yang dipegang daemon

Daemon membaca pipe, menyamarkan, dan menulis log.

**Pros**:

- Redaksi, rotasi, dan streaming paling mudah.

**Cons**:

- Runtime Go (NATS, Traefik) mati saat menulis ke pipe yang putus, jadi tidak cocok dengan Keputusan 1.
- Pipe yang penuh menahan proses saat daemon lambat.

### Keputusan 3: deadline readiness habis saat proses masih hidup

#### Option 1: Proses tetap jalan, operasi gagal timeout

**Pros**:

- Database yang lambat (misalnya replay WAL besar) tidak dihentikan di tengah jalan, sesuai PRD 35.2.
- Readiness yang akhirnya berhasil langsung terlihat tanpa start ulang.

**Cons**:

- Operasi gagal tetapi instance berjalan, yang harus dijelaskan dengan jelas di GUI dan CLI.

#### Option 2: Stop graceful lalu `failed`

**Pros**:

- State akhir sederhana.

**Cons**:

- Bisa memotong pemulihan database yang sah.

#### Option 3: Dianggap crash

**Pros**:

- Satu jalur untuk semua kegagalan start.

**Cons**:

- Menghabiskan retry budget untuk proses yang mungkin hanya lambat.

### Keputusan 4: stop timeout habis

#### Option 1: Tetap `stopping`, tanpa eskalasi

**Pros**:

- Sesuai aturan spec 0004 dan PRD 19.3; mode shutdown database tidak berubah tanpa pengguna.

**Cons**:

- Pengguna harus memutuskan force stop sendiri (fitur 26).

#### Option 2: Eskalasi sinyal provider otomatis

**Pros**:

- Lebih sering selesai tanpa tindakan pengguna.

**Cons**:

- Mengubah mode shutdown (misalnya smart ke immediate di PostgreSQL) tanpa persetujuan, dengan risiko crash recovery.

### Keputusan 5: angka restart policy

#### Option 1: 1 detik kali 2 sampai 30 detik, jitter 0,5 sampai 1,0, 5 per 5 menit, reset setelah 60 detik ready

**Pros**:

- Crash tunggal pulih dalam sekitar 1 detik; crash loop berhenti dalam sekitar 30 detik.
- Mengikuti usulan PRD 19.4 dan 35.2.

**Cons**:

- Provider yang butuh waktu lama melepas port atau lock bisa menghabiskan budget lebih cepat.

#### Option 2: Sama, reset setelah 5 menit ready

**Pros**:

- Lebih konservatif untuk crash yang terjadi setelah sempat ready.

**Cons**:

- Instance yang pulih tetap membawa riwayat crash lama selama 5 menit.

#### Option 3: 2 detik kali 2 sampai 60 detik, reset setelah 120 detik

**Pros**:

- Lebih ramah untuk provider berat.

**Cons**:

- Pemulihan crash tunggal lebih lambat untuk semua provider.

### Keputusan 6: override restart policy

#### Option 1: Tidak ada override di versi ini

**Pros**:

- Satu policy yang mudah dinalar dan diuji; perbedaan provider sudah tercakup timeout spec 0009.
- Spec 0009 sengaja menutup restart policy dari manifest.

**Cons**:

- Pengguna dengan kebutuhan khusus belum bisa menyetelnya.

#### Option 2: Override per instance lewat settings Stackly

**Pros**:

- Fleksibel bagi pengguna.

**Cons**:

- Menambah permukaan form dan validasi sebelum ada kebutuhan nyata.

#### Option 3: Field di manifest

**Pros**:

- Provider bisa menyesuaikan sendiri.

**Cons**:

- Bertentangan dengan keputusan spec 0009 dan membuka pintu manifest yang membuat busy loop.

### Keputusan 7: crash sebelum pernah ready dalam operasi start

#### Option 1: Retry di dalam operasi start

**Pros**:

- Memenuhi bunyi AC-068 (provider yang terus crash saat startup berhenti di retry budget).
- Satu operasi mencerminkan seluruh usaha start, termasuk backoff yang terlihat.

**Cons**:

- Config yang salah dicoba enam kali sebelum gagal.

#### Option 2: Gagal di crash pertama

**Pros**:

- Kegagalan config terlihat lebih cepat.

**Cons**:

- Tidak memenuhi AC-068 dan memperlakukan crash awal berbeda dari crash setelah ready.

### Keputusan 8: desired `running` dengan proses yang terbukti hilang saat rekonsiliasi

#### Option 1: Boot sama dijalankan lagi, boot baru menjadi `stopped`

**Pros**:

- Crash daemon tidak membuat instance mati sampai pengguna bertindak.
- Tidak mendahului keputusan fitur 31 tentang start saat login.

**Cons**:

- Desired `running` dan lifecycle `stopped` bisa berdampingan setelah reboot sampai fitur 31 dibangun.

#### Option 2: Selalu dijalankan lagi

**Pros**:

- Perilaku paling sederhana.

**Cons**:

- Menyalakan database yang tidak diminta saat login dan mendahului fitur 31.

#### Option 3: Tidak pernah dijalankan lagi

**Pros**:

- Rekonsiliasi hanya mencatat.

**Cons**:

- Setiap crash daemon mematikan instance pengguna sampai ia menyalakannya sendiri.

### Keputusan 9: bukti asosiasi data dan penjaga proses kedua

#### Option 1: Lock file yang diwariskan ke proses

`flock` eksklusif pada `instance.lock` di root instance, fd nya diwariskan ke proses utama di fd 3.

**Pros**:

- Bukti asosiasi yang kuat (fd proses memuat inode lock) dan tidak bergantung pada PID.
- Menjaga dari server kedua walau daemon, registry, atau observasi hilang, termasuk dari turunan proses lama.

**Cons**:

- Provider yang menutup fd warisan tidak bisa diadopsi, dan file baru masuk layout instance.

#### Option 2: Working directory

**Pros**:

- Tanpa file baru.

**Cons**:

- Tidak menjaga dari turunan proses dan gagal untuk provider yang pindah cwd.

#### Option 3: Digest argv

**Pros**:

- Bukti yang spesifik ke perintah yang dijalankan.

**Cons**:

- Butuh kolom baru dan tidak menjaga dari turunan proses.

### Keputusan 10: cancel start setelah spawn

#### Option 1: Stop graceful dan desired `stopped`

**Pros**:

- Sesuai maksud pengguna yang membatalkan; tidak ada proses yang jalan tanpa diminta.

**Cons**:

- Cancel bisa memakan waktu sampai `stopSeconds`.

#### Option 2: Hanya berhenti menunggu

**Pros**:

- Cancel instan.

**Cons**:

- Instance tetap berjalan dengan desired `running`, mengejutkan pengguna.

### Keputusan 11: timeout proses pembantu

#### Option 1: TERM ke group, 5 detik, lalu KILL ke group

**Pros**:

- Pembantu seperti initdb sempat membersihkan diri, dan tidak pernah tertinggal.
- Aman karena pembantu adalah child yang belum di reap, jadi PID nya tidak bisa dipakai ulang.

**Cons**:

- Menambah satu jalur `SIGKILL` yang harus dijaga tidak pernah mencapai proses utama.

#### Option 2: Langsung KILL

**Pros**:

- Paling cepat.

**Cons**:

- Pembantu tidak sempat membersihkan file sementara.

#### Option 3: Dibiarkan, hanya dilaporkan

**Pros**:

- Tidak ada sinyal paksa sama sekali.

**Cons**:

- Pembantu yang menggantung bisa terus memegang data dan lock.

### Keputusan 12: pemilik method IPC instance

#### Option 1: Spec ini, termasuk create minimal

**Pros**:

- Fitur 14 bisa dibangun dari spec Foundation saja.
- Satu tempat untuk DTO status yang dibutuhkan keempat state terpisah.

**Cons**:

- Fitur 20 harus memperluas create secara additive, dan spec ini lebih besar.

#### Option 2: Spec ini tanpa create

**Pros**:

- Batas fitur lebih sempit.

**Cons**:

- Fitur 14 tertahan sampai spec fitur 20 ditulis.

#### Option 3: Hanya use case core

**Pros**:

- Spec paling kecil.

**Cons**:

- Done when "process, readiness, health, dan desired state terlihat terpisah" tidak punya permukaan yang bisa diuji di fitur ini.

### Keputusan 13: urutan event status yang tidak menaikkan revision

Awalnya diputuskan Option 2 (`observedAt`); cross check menunjukkan event itu juga membawa blocker, status restart, dan proses yang berubah tanpa `observedAt` baru, dan nilai stale mempertahankan `observedAt` lama, sehingga client bisa melewatkan event nyata. Pengguna lalu memilih Option 1.

#### Option 1: `statusSeq` per instance

Penghitung monotonik per instance per run daemon di snapshot dan setiap `instance.health_changed`.

**Pros**:

- Setiap perubahan isi summary punya urutan yang pasti, tidak bergantung jam.
- Revision tetap berarti mutasi aggregate (spec 0004 dan 0005).

**Cons**:

- Resep client spec 0006 butuh satu pengecualian, dan client harus mengambil snapshot baru setelah gap `server_restarted`.

#### Option 2: Dibandingkan lewat `observedAt`

**Pros**:

- Memakai field yang sudah ada.

**Cons**:

- Perubahan tanpa observasi baru (blocker, restart, proses, stale) tidak punya `observedAt` yang lebih baru dan terlewat.

#### Option 3: Observasi menaikkan revision

**Pros**:

- Resep client tidak berubah.

**Cons**:

- Bertentangan dengan spec 0004 dan 0005, dan setiap probe membuat CAS operasi gagal.

#### Option 4: Event daemon tanpa resource

**Pros**:

- Tidak menyentuh aturan revision.

**Cons**:

- Client kehilangan filter per resource di `events.subscribe`.

### Keputusan 14: letak file `materializeSecret`

#### Option 1: Folder `secrets/` spec 0011

**Pros**:

- Sweep, guard `SecretFile`, dan aturan hapus spec 0011 berlaku tanpa kasus khusus.

**Cons**:

- Spec 0009 harus diubah supaya destination menjadi nama file saja.

#### Option 2: Path dari plan apa adanya

**Pros**:

- Spec 0009 tidak berubah.

**Cons**:

- Sweep spec 0011 harus memeriksa folder run dan temp juga, dan file secret bisa tersebar.

### Keputusan 15: client HTTP untuk probe

#### Option 1: Client kecil buatan sendiri

**Pros**:

- Tanpa dependency baru; kebutuhannya hanya satu `GET` ke loopback tanpa redirect dan TLS.

**Cons**:

- Parser status line dan pembatas body harus diuji sendiri.

#### Option 2: hyper

**Pros**:

- Parsing HTTP yang matang.

**Cons**:

- Dependency besar untuk satu request.

#### Option 3: reqwest

**Pros**:

- API paling nyaman.

**Cons**:

- Membawa TLS, redirect, dan banyak dependency yang tidak dipakai.

### Keputusan 16: probe rutin setelah ready

#### Option 1: Setiap 10 detik dengan debounce sebanyak `failure_threshold`

**Pros**:

- Readiness tidak pernah stale dalam kondisi normal (`READINESS_MAX_AGE` 30 detik).
- Kegagalan sesaat setelah wake atau saat beban tinggi tidak membuat status berkedip.

**Cons**:

- Readiness yang benar benar hilang baru terlihat setelah sekitar 30 detik.

#### Option 2: Ikut interval health

**Pros**:

- Lebih sedikit probe.

**Cons**:

- Provider dengan interval health panjang selalu terbaca stale.

#### Option 3: Tanpa debounce

**Pros**:

- Paling jujur dan cepat.

**Cons**:

- Status berkedip setelah wake atau saat beban sesaat.

Cross check menemukan bahwa ambang 3 dari `failureThreshold` health dikali 10 detik sama dengan `READINESS_MAX_AGE` 30 detik, sehingga `not_ready` dan stale jatuh di saat yang sama. Debounce readiness kini konstanta sendiri, 2 kegagalan.

### Keputusan 17: celah antara spawn, tangkap identitas, dan commit

#### Option 1: Spawn tertahan (`POSIX_SPAWN_START_SUSPENDED`)

Proses utama dibuat tertahan, identitas ditangkap dan dicommit, lalu `SIGCONT`; kalau gagal, child yang belum pernah berjalan dibuang.

**Pros**:

- Tidak ada proses berjalan tanpa catatan; exit sangat cepat dan zombie tidak lagi salah dilaporkan sebagai gagal tangkap identitas.
- Daftar fd anak bisa diperiksa tepat sebelum kode provider berjalan (AC-1).

**Cons**:

- Flag khusus Apple yang harus dibuktikan spike; daemon yang mati di antara spawn dan commit masih meninggalkan proses tertahan yang menjadi `unknown`.

#### Option 2: Tangkap langsung tanpa suspend

**Pros**:

- Memakai flag spawn standar saja.

**Cons**:

- Butuh aturan khusus untuk exit sebelum commit dan zombie, dan daemon yang mati di jendela itu meninggalkan proses berjalan yang `unknown` permanen.

### Keputusan 18: kapan port `Auto` ditulis

#### Option 1: Saat `spawned`, ditampilkan final setelah ready

**Pros**:

- Proses yang diadopsi selalu punya port tersimpan untuk probe dan plan.
- Instance lain langsung melihat port itu sebagai port efektif, sehingga dua instance tidak berebut port Auto yang sama.

**Cons**:

- Kalimat aturan `bound` di spec 0004 harus diperbarui.

#### Option 2: Setelah ready

**Pros**:

- Sesuai bunyi aturan spec 0004 hari ini.

**Cons**:

- Daemon yang mati sebelum ready kehilangan port proses yang masih hidup; butuh pembacaan socket listen lewat libproc atau `unknown`.

### Keputusan 19: cara pembaca spool membebaskan ruang

Diputuskan setelah spike. Baris Spool semula meminta pembaca `O_RDONLY` dan punch yang diratakan ke `f_bsize`. Di macOS 27, `F_PUNCHHOLE` pada fd `O_RDONLY` ditolak `EPERM`. Dengan fd baca tulis, punch selebar `f_bsize` (4 KiB) berhasil, tetapi blok yang di punch di dalam halaman VM 16 KiB yang masih ditulis child kembali teralokasi berisi nol: alokasi dua spool terus naik sampai 3272704 B setelah 60 detik, dan `SEEK_DATA` menunjuk ke byte nol (37 dari 40 jawaban basi), sehingga resume setelah daemon restart membaca nol.

#### Option 1: Fd baca tulis, satuan punch `max(halaman VM, f_bsize)`

Pembaca membuka spool `O_RDWR|O_NOFOLLOW|O_CLOEXEC` dan hanya mem punch satuan penuh, dengan satuan sebesar halaman VM (`sysconf(_SC_PAGESIZE)`, 16 KiB di Mac ini) atau `f_bsize` kalau lebih besar.

**Pros**:

- Terbukti spike: puncak alokasi per spool tidak naik dari detik 15 sampai 60 (1052672 B; nilai sesaat naik turun di bawahnya), `SEEK_DATA` tepat setelah setiap punch (0 dari 40 basi), semua baris utuh dan berurutan, dan resume dari `SEEK_DATA` memberi baris utuh.
- Satuan mengikuti mesin dan selalu kelipatan `f_bsize` (keduanya pangkat dua), jadi punch tidak pernah `EINVAL`.
- Bagian satuan yang terulang setelah daemon restart paling banyak 16 KiB (ditambah satu batch baca, sama untuk satuan apa pun).

**Cons**:

- Pembaca memegang fd yang bisa menulis. Dijaga dengan `SpoolReader` tanpa method tulis atau truncate, dan open tanpa `O_CREAT`, `O_TRUNC`, atau `O_APPEND`.

#### Option 2: Fd baca tulis, satuan tetap 64 KiB

**Pros**:

- Juga terbukti spike, dan punya margin kalau ukuran halaman berubah.

**Cons**:

- Bagian satuan yang terulang setelah restart sampai 64 KiB, sedikit lebih banyak ruang tertahan, dan angka tetap yang harus ditinjau ulang kalau mesin berubah. Option 1 sudah mengikuti ukuran halaman dengan sendirinya.

#### Option 3: Tanpa punch (truncate atau rotasi)

**Pros**:

- Pembaca tetap `O_RDONLY`.

**Cons**:

- Truncate saat child menulis append bisa membuang baris (Keputusan kecil "Punch hole, bukan truncate"), dan rotasi tidak mungkin karena child memegang fd nya sendiri dan tidak bisa diarahkan ke file baru.

### Keputusan 20: arti batas 2 MiB di AC-12

Diputuskan setelah spike. Dengan Keputusan 19, puncak alokasi setiap spool sekitar 1 MiB (1052672 B) walau masing masing menerima sekitar 6 MB, tetapi dua spool bersama mencapai 2105344 B, 8 KiB di atas 2 MiB (2064384 B di run sebelumnya). Sampai sekitar 1 MiB per spool yang sedang ditulis tetap teralokasi walau isinya sudah di punch; fsync setelah setiap punch tidak mengubahnya, dan puncaknya tidak tumbuh selama run. Penyebabnya belum dibuktikan (kontrol tanpa punch hanya berjalan 10 detik), dan angka ini diukur di satu laju selama 60 detik. Kalimat AC-12 semula ("blok yang dialokasikan spool tetap di bawah 2 MiB") bisa dibaca per spool atau untuk dua spool bersama.

#### Option 1: Per spool, di bawah 2 MiB di setiap sampel

**Pros**:

- Margin dua kali dari nilai terukur, jadi tidak rawan flaky.
- Menangkap satu stream yang membengkak sendiri, dan satuannya sama dengan file yang dibebaskan pembaca.

**Cons**:

- Total per instance (sekitar 2 MiB untuk dua spool) tidak tertulis langsung sebagai satu angka.

#### Option 2: Dua spool bersama, di bawah 4 MiB

**Pros**:

- Tetap satu angka per instance, dengan margin yang sama.

**Cons**:

- Bisa menyembunyikan satu stream yang tumbuh sementara stream lain diam.

#### Option 3: Dua spool bersama, di bawah 3 MiB

**Pros**:

- Batas lebih ketat.

**Cons**:

- Margin hanya sekitar 1,4 kali dari nilai terukur, rawan gagal kalau APFS menahan satu chunk lagi.

### Keputusan 21: fakta proses milik user lain

Diputuskan setelah spike. `PROC_PIDTBSDINFO` dan `PROC_PIDLISTFDS` ditolak `EPERM` untuk 168 dari 168 proses milik user lain (root dan user `_`), termasuk PID 1. Dengan baris adapter semula, `inspect` untuk PID tercatat yang kini dipakai proses user lain gagal `PermissionDenied`, dan aturan "error apa pun menghasilkan `Unproven`" membuat instance `unknown` dengan `identityUnproven` sampai proses asing itu keluar, padahal AC-10 meminta `Gone(pid_reused)` kalau lock bebas. Di sisi lain, `sysctl KERN_PROC_PID` memberi `kinfo_proc` dengan uid dan waktu start untuk 168 dari 168 proses itu, sama dengan `PROC_PIDTBSDINFO` untuk proses sendiri, dan `proc_pidpath` serta `getsid` juga bekerja untuk mereka.

#### Option 1: Fallback ke `kinfo_proc` hanya saat `EPERM`

**Pros**:

- AC-10 tetap seperti tertulis, dan PID yang dipakai ulang oleh user lain tidak membuat `unknown` yang lengket.
- Jalur utama untuk proses sendiri, termasuk adopsi, tetap memakai `proc_bsdinfo` yang ada di libc; fallback hanya perlu membuktikan bahwa proses itu bukan milik kita.
- Kesalahan layout di fallback paling buruk menghasilkan `Gone` saat lock bebas atau `Unproven`, tidak pernah sinyal: pemeriksaan ulang sebelum `kill(2)` tetap menolak uid yang berbeda, dan lock yang bebas adalah bukti terkuat yang ada bahwa data tidak dipakai (celahnya, provider yang menutup fd 3, sudah dinyatakan di Consequences spec).

**Cons**:

- Dua jalur baca dan satu struct 648 byte yang dideklarasikan sendiri; butuh assert layout dan test yang membandingkan setiap field dengan `PROC_PIDTBSDINFO`.
- Proses kita sendiri yang menjalankan binary setuid terlihat milik user lain; kalau ia juga menutup fd 3, hasilnya bisa `Gone` yang salah, dengan sweep secret dan respawn. Karena itu fallback menolak memberi fakta asing (hasilnya `Unproven`) kalau uid real atau saved set uid proses itu sama dengan user kita.

#### Option 2: `kinfo_proc` untuk semua PID

**Pros**:

- Satu jalur baca untuk semua proses.

**Cons**:

- Setiap pemeriksaan identitas, termasuk adopsi, bergantung pada struct yang dideklarasikan sendiri, dan jalur `PROC_PIDTBSDINFO` yang sudah terbukti dibuang.

#### Option 3: Terima `Unproven`

**Pros**:

- Tidak ada kode baru di adapter.

**Cons**:

- AC-10 harus diubah, dan instance menjadi `unknown` sampai proses asing itu keluar, yang untuk daemon sistem bisa berarti sampai reboot. Pengguna harus bertindak di luar Stackly padahal bukti bahwa proses itu bukan milik kita tersedia.

## Rationale

Kekuatan yang paling menentukan adalah keselamatan data dan larangan salah bunuh. Karena PRD sudah menerima bahwa proses bisa selamat dari crash daemon, kode adopsi dan `unknown` wajib ada apa pun pilihannya; maka pilihan yang tersisa adalah apakah proses yang selamat itu dipertahankan atau dikorbankan. Mempertahankannya (Keputusan 1) memberi nilai terbesar bagi pengguna dengan biaya yang memang sudah harus dibayar, dan membuka update daemon tanpa downtime. Keputusan itu langsung menyingkirkan pipe (Keputusan 2), karena runtime Go mati saat pipe putus; spool di runtime root adalah satu satunya bentuk yang tetap hidup tanpa daemon sekaligus memenuhi aturan spec 0011 bahwa log disamarkan sebelum disimpan.

Lock yang diwariskan (Keputusan 9) adalah inti pengaman RISK-03 dan RISK-14. PID bisa dipakai ulang dan registry bisa hilang, tetapi kernel melepas `flock` hanya saat semua pemegang fd nya keluar, jadi "lock dipegang" adalah bukti yang tidak bisa basi bahwa sesuatu masih memakai data itu. Dengan lock itu, start tidak bergantung pada kebenaran observasi untuk menolak server kedua, dan adopsi punya bukti asosiasi data yang lebih kuat dari cwd atau argv. Klasifikasi dibuat sebagai fungsi murni dengan tabel lengkap supaya setiap kombinasi bukti punya hasil yang diuji, dan setiap jalan yang ragu berakhir di `unknown`.

Keputusan timeout (3, 4, 11) mengikuti satu prinsip: proses utama hanya pernah menerima sinyal yang dipilih provider, dan timeout bukan bukti bahwa proses mati atau rusak. Sinyal paksa hanya untuk proses pembantu yang masih child dan belum di reap, satu satunya keadaan di mana PID dijamin tidak dipakai ulang. Restart policy (5, 6, 7) mengikuti angka usulan PRD tanpa override, karena perbedaan provider yang nyata (PostgreSQL lawan proxy ringan) sudah tercakup timeout recovery spec 0009, dan override menambah permukaan sebelum ada kebutuhan. Rekonsiliasi (8) hanya menjalankan lagi instance di boot yang sama, sehingga crash daemon pulih sendiri sementara keputusan login tetap milik fitur 31.

Method IPC minimal (12) diambil di sini karena urutan scope: fitur 14 menutup Foundation dan tidak boleh menunggu fase berikutnya. Dua konflik spec (13, 14) diselesaikan ke arah yang menjaga aturan spec pemiliknya (revision tetap berarti mutasi, file secret tetap di bawah kendali spec 0011), dengan perubahan kecil di spec 0006 dan 0009 sebagai Follow-up.

Spawn tertahan (17) dan port Auto yang tersimpan sejak spawn (18) menutup dua celah yang ditemukan cross check dengan cara yang sama: registry selalu tahu tentang proses sebelum proses itu menjalankan kode atau memakai port, sehingga daemon yang mati di titik mana pun meninggalkan state yang bisa direkonsiliasi tanpa menebak.

Tiga keputusan setelah spike (19, 20, 21) mengikuti bukti di Mac ini, bukan bunyi tabel semula. Untuk spool (19), hanya fd baca tulis dengan satuan punch selebar halaman VM yang menjaga puncak alokasi tidak tumbuh dan `SEEK_DATA` tepat, dan satuan yang mengikuti mesin lebih tahan lama daripada angka tetap. Batas AC-12 (20) diukur per spool karena sisa sampai sekitar 1 MiB muncul per file yang sedang ditulis (penyebabnya belum dibuktikan), sehingga batas per file punya margin dua kali dan tetap menangkap stream yang membengkak sendiri. Untuk proses user lain (21), fallback `kinfo_proc` dipilih karena ia mengubah `unknown` yang lengket menjadi jawaban yang terbukti tanpa melemahkan keselamatan: hasil fallback hanya bisa membuat sebuah proses terbukti bukan milik kita, dan hanya kalau uid efektif, real, dan saved nya semuanya bukan milik kita; lock tetap menjadi bukti terkuat bahwa data sedang dipakai. Supaya bukti yang sudah jelas tidak dirusak error yang tidak relevan, bukti dikumpulkan berurutan: fakta proses hanya dibaca di boot yang sama, dan `holds_file` hanya kalau enam pemeriksaan lain lolos.

## Keputusan kecil

Diputuskan saat menulis spec, masing masing dengan pilihan, alasan, dan alternatif terdekat:

- **`posix_spawn` langsung lewat libc**, karena `std::process::Command` tidak bisa menggabungkan `SETSID`, `CLOEXEC_DEFAULT`, dan pemetaan fd 3 tanpa jalur `fork` dan `pre_exec`. Alternatif: `Command` dengan `pre_exec` (memaksa `fork`, kode tidak aman setelah fork di proses multi thread).
- **Satu thread watcher kqueue**, karena `NOTE_EXIT` bekerja untuk child dan proses yang diadopsi sekaligus dan tidak butuh handler `SIGCHLD`. Alternatif: `SIGCHLD` plus `waitpid` (tidak bisa mengawasi proses yang diadopsi) atau polling `kill(pid, 0)` (boros dan lambat).
- **Actor tokio per instance**, karena satu antrian per instance membuat command operasi, exit, dan timer berurutan tanpa lock bersama. Alternatif: satu mutex global (menahan semua instance saat satu instance lambat).
- **Respawn sebagai operasi sistem**, karena ia memegang lock resource yang sama dengan operasi pengguna dan muncul di riwayat aktivitas. Alternatif: respawn internal tanpa operasi (harus menduplikasi serialisasi dengan stop pengguna).
- **Boot ID dari `kern.bootsessionuuid`**, karena berubah setiap boot dan bisa dibaca tanpa hak khusus. Alternatif: `kern.boottime` (bisa sama setelah penyesuaian jam).
- **Session ID dari `getsid`**, sebagai bukti tambahan yang murah: `SETSID` membuatnya selalu sama dengan PID, jadi ia tidak berdiri sendiri dan kekuatan identitas datang dari waktu start, executable, dan lock. Alternatif: process group (setara di sini).
- **Pemeriksaan ulang sebelum sinyal tanpa executable**, karena PID ditambah waktu start sudah unik dan provider yang sah tidak boleh gagal di stop hanya karena path executable nya terbaca berbeda; executable tetap diperiksa saat adopsi. Alternatif: semua pemeriksaan di setiap sinyal (stop bisa ditolak untuk proses yang jelas milik kita).
- **User tidak disimpan**, karena setiap catatan ditulis daemon user itu sendiri; pemeriksaan membandingkan uid proses dengan user berjalan. Alternatif: kolom uid baru (butuh migrasi tanpa nilai tambah).
- **Pembaca spool berbasis tick 500 ms**, karena cukup murah untuk 10 instance dan sederhana. Alternatif: `EVFILT_VNODE` di watcher (lebih hemat, lebih rumit).
- **Punch hole, bukan truncate**, karena truncate saat proses menulis append bisa membuang baris. Alternatif: copytruncate (sederhana, kehilangan baris).
- **Resume dengan `SEEK_DATA` tanpa file state**, karena duplikasi paling banyak satu satuan punch (16 KiB di Mac ini) ditambah satu batch baca, dan ditandai `resumed`. Alternatif: file offset di folder run (tulis berkala, tanpa fsync, lebih banyak state).
- **Rekonsiliasi hanya mengklasifikasi di slot start, start dijalankan setelah daemon `ready`**, supaya slot start tidak menunggu deadline readiness puluhan detik. Alternatif: start di dalam slot (daemon lama tidak siap).
- **Pemeriksaan ulang `unknown` tanpa start otomatis**, karena keluar dari `unknown` biasanya hasil tindakan pengguna di luar Stackly. Alternatif: start kalau desired `running` (bisa mengejutkan setelah pengguna mematikan proses liar).
- **`SIGTERM` daemon tidak menyentuh instance**, mengikuti Keputusan 1; restart terencana yang menghentikan instance milik fitur 30. Alternatif: stop semua instance saat daemon berhenti (mematikan database pada setiap update).
- **Port `Auto` memakai ulang `bound` lama kalau bebas**, karena REQ-010 meminta port auto dipersistenkan dan connection string pengguna tetap berlaku. Alternatif: port baru setiap start.
- **Socket Unix basi di folder run dihapus hanya saat lock dipegang**, karena lock membuktikan tidak ada proses instance itu yang hidup. Alternatif: serahkan ke provider (PostgreSQL menanganinya sendiri, provider lain belum tentu).
- **`install_fake` di fitur crate `fixture` core**, karena butuh digest paket dari core dan tidak boleh ada di build produksi. Alternatif: helper di testkit (testkit tidak boleh bergantung pada core).
- **Jitter dari `getrandom`**, yang sudah ada di core lewat spec 0011. Alternatif: crate `rand` (dependency baru).
- **Jam monotonik untuk deadline dan backoff**, karena tidur Mac tidak boleh dihitung sebagai crash atau timeout. Alternatif: jam dinding (melompat saat wake).
- **argv[0] path canonical penuh**, karena beberapa provider mencari file pendukung relatif terhadap argv[0]. Alternatif: nama file saja.
- **Sisa proses group dilaporkan, tidak disinyal**, karena force milik fitur 26 dan lock sudah mencegah start selama sisa itu memegang data. Alternatif: `SIGTERM` ke group setelah proses utama keluar.
- **Payload `instance.state_changed` berisi snapshot utuh**, karena ukurannya kecil dan client tidak perlu menggabungkan potongan. Alternatif: diff per field.
- **Enum lifecycle, readiness, dan health tertutup di wire**, karena ketiganya punya nilai `unknown` yang nyata yang akan bertabrakan dengan varian fallback spec 0006. Alternatif: enum terbuka dengan fallback bernama lain (menyalahi pola spec 0006).
- **Satu `UpdateInstance` per mutasi domain dalam satu batch**, karena spec 0005 mewajibkan revision naik tepat 1 per op sementara domain menaikkan revision per mutasi. Alternatif: mutasi gabungan baru di domain (menambah API domain untuk setiap kombinasi).
- **Respawn di luar operasi pengguna tanpa operasi dan lock selama jeda**, karena operasi yang dibuat saat crash akan memegang lock sampai 30 detik dan membuat stop pengguna kena `resource.busy`. Alternatif: operasi sejak crash dengan stop yang bisa merebut lock (menambah aturan preempt di spec 0007).
- **Hanya percobaan pertama yang boleh memunculkan dialog Keychain**, supaya crash loop tidak memunculkan dialog di setiap siklus. Alternatif: interaksi operasi untuk semua percobaan.
- **Penolakan berdasarkan state dijawab method secara sinkron, no op tanpa operasi**, supaya jawaban di bawah 500 ms (NFR-006) tetap memuat alasan yang jelas dan riwayat operasi tidak penuh no op. Alternatif: semua lewat operasi (client harus menunggu operasi untuk tahu instance sedang `unknown`).
- **Satu slot blocker dengan prioritas**, supaya kontrak `blocker` spec 0011 tetap; blocker dengan prioritas lebih rendah muncul lagi setelah yang lebih tinggi hilang. Alternatif: daftar blocker (mengubah DTO spec 0011).
- **`PortProbe` sebagai seam yang bisa diinjeksi**, karena perebutan port di antara preflight dan bind tidak bisa dibuat deterministik di test native. Alternatif: hook test di dalam actor.
- **Probe HTTP hanya membaca status line**, karena keputusan sukses hanya bergantung pada status; body tidak pernah dibaca. Alternatif: membaca body sampai 64 KiB seperti tertulis di spec 0009 (tidak ada gunanya).
- **UTF-8 lossy dan redaksi atas baris logis utuh sebelum dipotong**, karena potongan di tengah nilai secret atau kunci `password=` membuat `Redactor` tidak bisa mencocokkannya. Alternatif: redaksi per potongan (bocor di batas potongan).
- **Fd sumber diduplikasi ke nomor 10 ke atas sebelum `adddup2`, lock dilepas dengan `close`, dan satu mutex per PID untuk sinyal dan reap**, supaya pemetaan fd tidak bertabrakan saat stdio daemon tertutup, lock yang diwariskan tidak ikut terlepas, dan sinyal tidak pernah sampai ke PID yang sudah di reap. Alternatif: tidak ada yang sama amannya.
- **Stale dihitung dengan jam dinding di setiap tick probe**, karena timer monotonik berhenti saat Mac tidur sementara `effective` memakai jam dinding. Alternatif: timer stale terpisah (terlambat setelah wake).
- **Simbol yang tidak ada di libc 0.2.190 dideklarasikan lokal di `process::sys`** (`POSIX_SPAWN_SETSID`, `PROC_PIDFDVNODEPATHINFO`, `proc_fileinfo`, `vnode_fdinfowithpath`, `PROC_PGRP_ONLY`, dan `kinfo_proc` Apple), dengan `#[repr(C)]` utuh dan `const` assert ukuran serta offset, karena enam simbol tidak sepadan dengan dependency baru dan assert membuat layout yang salah gagal saat compile. Alternatif: crate binding pihak ketiga (dependency baru yang harus lolos `cargo deny`, dengan API jauh lebih luas dari yang dipakai).
- **Bukti dikumpulkan berurutan lewat `gather_evidence`** (`inspect` hanya di boot yang sama, `holds_file` hanya kalau enam pemeriksaan lain lolos, sisanya `NotChecked`), karena bukti yang tidak dibaca tidak menentukan klasifikasi, dan error membaca bukti yang tidak relevan tidak boleh mengubah `Gone` menjadi `Unproven`. Alternatif dari cross check: selalu dibaca, lalu `classify` mengabaikan hasilnya saat pemeriksaan lain gagal (lebih sedikit tipe, tetapi aturan "error apa pun menjadi `Unproven`" mendapat pengecualian dan daftar fd proses asing tetap dibaca).
- **Error `proc_pidpath` dan `getsid` selain `ESRCH` menjadi `Io`**, karena executable yang di unlink dari proses yang masih hidup bukan bukti bahwa proses itu bukan milik kita. Alternatif: dianggap executable berbeda (proses kita yang runtime nya terhapus bisa menjadi `Gone` dan dijalankan ulang kalau lock nya bebas).
- **Fallback menolak fakta asing kalau uid real atau saved set uid sama dengan user kita**, supaya proses kita yang menjalankan binary setuid tetap `Unproven`, bukan `Gone`. Alternatif: hanya membandingkan uid efektif (lebih sederhana, membuka `Gone` yang salah).
- **Satuan punch dihitung dari mesin, bukan konstanta**, mengikuti Keputusan 19, dengan fake yang satuannya bisa disetel supaya test tetap deterministik. Alternatif dari cross check: konstanta `SPOOL_PUNCH_UNIT` 16 KiB yang dicek saat open terhadap `max(halaman VM, f_bsize)` (sama di Apple Silicon, tetapi harus diubah kalau mesin berubah).
- **`release_below` dengan watermark dan offset ujung baris yang diterima sink**, karena offset baca bisa melewati baris yang masih ditahan, dan watermark membuat panggilan berulang murah dan aman. Alternatif: punch dari 0 setiap kali (punch ulang range yang sudah kosong).
- **Fallback `kinfo_proc` hanya saat `EPERM`**, bukan untuk error lain, karena `ESRCH` sudah berarti PID tidak ada, dan error lain tetap harus menjadi `Unproven`. Alternatif: fallback untuk setiap error (menyembunyikan error yang seharusnya terlihat di bukti).

## Cross check

Draft dibaca ulang oleh Sonnet 5.5 sebagai kritik read only (2026-10-18). Hasilnya: desain keselamatan inti dinilai kuat, dengan 14 celah kelengkapan dan 10 catatan soundness. Pengguna menyetujui semua perbaikan yang direkomendasikan, dan memilih ulang tiga keputusan (Keputusan 13, 17, dan 18). Perbaikan yang masuk ke spec:

- Rekonsiliasi, respawn, dan cancel hanya memakai transisi yang ada di tabel lifecycle spec 0004 (respawn dari `recovering` langsung `spawned`, stop pada `stopping` tanpa `journal`, start sistem dari `failed` atau `stopped` lewat jalur normal).
- Spawn tertahan menutup celah spawn, identitas, dan commit; proses terbukti yang masih tertahan dilanjutkan saat rekonsiliasi.
- Port `Auto` tersimpan sejak `spawned`; respawn mengulang pemeriksaan endpoint; port yang direbut setelah preflight dideteksi lewat `PortProbe` setelah exit sebelum ready dan tidak memakai budget.
- Dry run `run_plan` dan `ensure_not_in_argv` sebelum `journal`.
- Error saat mengumpulkan bukti selalu `Unproven`; pemetaan blocker per hasil klasifikasi; satu slot blocker dengan prioritas; pemeriksaan ulang `dataInUse`.
- `READINESS_FAILURE_THRESHOLD` 2 terpisah dari ambang health; stale dihitung dengan jam dinding di setiap tick.
- Kasus stop yang belum terdefinisi, handler exit tunggal, dan `remove_secret_material` setelah setiap exit tanpa respawn.
- Aturan commit actor (AC-22) dan jawaban method sinkron serta no op (AC-23).
- Sumber field status setelah daemon restart, `statusSeq`, `logsUnavailable`, dan `adoptable`.
- Aturan spool (UTF-8 lossy, redaksi baris utuh, resume dari newline, backlog per stream yang bisa disetel di test) dan probe HTTP yang hanya membaca status line.
- Interaksi Keychain `Never` untuk setiap respawn; timer backoff tanpa operasi dan lock.
- Higiene fd di adapter, `cfg(unix)` untuk probe socket Unix di core, dan pemeriksaan ulang sinyal tanpa executable.
- Follow-up baru: langkah berhenti daemon di spec 0006, baris pohon layout di spec 0004, `unknown` dihitung berjalan di spec 0011, arti `foreground` di spec 0009, dan harness konformitas fitur 36.

Residual risk yang dinyatakan alih alih diperbaiki: spool tumbuh tanpa batas selama daemon mati, proses tertahan dari daemon yang mati di jendela spawn menjadi `unknown`, dan secret yang terpotong batas `LOG_LINE_HARD_MAX`.

Pembaruan setelah spike (Keputusan 19 sampai 21) dibaca ulang oleh Sonnet 5.5 sebagai kritik read only (2026-10-18). Hasilnya: desain aman dan gagal ke arah tertutup (tidak ada jalur dari fallback ke sinyal), dengan delapan celah kelengkapan dan satu catatan keselamatan. Pengguna menyetujui semua perbaikan yang direkomendasikan:

- Urutan pengumpulan bukti lewat `gather_evidence` dan `identity_checks`, tipe input `classify`, `NotChecked` di bukti dan wire, sumber `uid`, dan pengaman start yang menolak saat bukti error.
- Pemetaan error `inspect` (`proc_pidpath` dan `getsid` selain `ESRCH` menjadi `Io`), `stopped` dan zombie di kedua jalur, dan aturan `Gone(exited)` lawan `Gone(pid_reused)`.
- Layout `kinfo_proc`: lebar field, offset yang diukur langkah 3, seam `inspect_kinfo`, dan test PID yang sudah di reap.
- Aturan `release_below` (watermark, offset ujung baris yang diterima sink, no op, clamp, error punch sebagai warning), satuan dihitung setiap `open_spool`, `punch_unit()`, dan fake yang membulatkan dengan cara sama.
- Resume: `first_data_offset()` berupa `Option`, dan baris pertama tidak dibuang saat mulai dari offset 0.
- Cara mengukur AC-12 (`st_blocks` kali 512, sampel paling lama 500 ms, offset yang dibebaskan), dan `release_below` saat drain di akhir proses.
- Test PID 1 lewat catatan panggilan fake untuk `inspect` dan `holds_file`, plus prasyarat `EPERM` di test adapter.
- Pengaman keselamatan: fallback menolak fakta asing kalau uid real atau saved sama dengan user kita.
- Kalimat yang berlebihan diperbaiki: batas duplikasi setelah restart (satu satuan ditambah satu batch baca), angka spool "sampai sekitar 1 MiB" dengan penyebab yang belum dibuktikan, dan klaim bahwa lock bebas membuktikan data tidak dipakai.

## Spike milestone 1 langkah 1 (2026-10-18)

Spike sekali pakai di luar workspace (libc 0.2.190 seperti workspace) dijalankan di Mac ini: macOS 27.0 build 26A428, Apple M1 Max, APFS dengan `f_bsize` 4096 dan `f_iosize` 1048576, halaman VM 16384. Child di setiap skenario adalah `stackly-fake-server` dari commit `d99e5ea`, dijalankan dari path canonical di dalam root runtime tiruan di bawah folder `Test Root Ünï cödé`. Lima belas record manual ada di `evidence/manual/2026-10-18-spec-0010-spike-*.json`; source spike, log lengkap, dan eksperimen tambahan ada di folder record pertama, `evidence/manual/files/01a14e57-e0d8-75bf-bff5-2ce203340b51/`.

| Baris *Adapter macOS* | Hasil | Catatan |
| --- | --- | --- |
| Spawn | PASS | fd child tertahan tepat 0 sampai 3; argv, env, dan cwd sama persis; mask sinyal kosong; duplikasi ke fd 10 ke atas wajib; `ENOENT` dan `EACCES` tanpa child |
| Tertahan dan lanjut | PASS | tidak ada kode yang jalan sebelum `SIGCONT`, juga dari daemon baru setelah daemon lama mati; `SIGKILL` lalu `waitpid` membuang child yang belum pernah jalan |
| Waktu start, user, group, parent (user sendiri) | PASS | child, yatim (ppid 1), dan tertahan; zombie menjawab `ESRCH` |
| Baris yang sama, proses user lain | FAIL, diputuskan di Keputusan 21 | `EPERM` untuk 168 dari 168; `kinfo_proc` terbaca untuk 168 dari 168 |
| Executable | PASS | `proc_pidpath` memberi path canonical; executable yang di rename memberi path baru, yang di unlink `ENOENT` |
| Session | PASS | `getsid` bekerja lintas session dan sama dengan PID |
| Boot | PASS | `kern.bootsessionuuid` UUID huruf besar yang stabil; spec menyimpannya huruf kecil |
| Boot setelah reboot | BLOCKED | butuh reboot Mac ini |
| Fd yang dipegang | PASS | `Yes` untuk child tertahan, berjalan, dan yatim; `EPERM` untuk user lain menjadi `Unreadable` |
| Anggota group | PASS | `proc_listpids(PROC_PGRP_ONLY)` termasuk sisa setelah leader di reap |
| Exit | PASS | `NOTE_EXIT` untuk child, child tertahan, dan bukan child; data `NOTE_EXITSTATUS` untuk bukan child tidak bisa dipercaya |
| Sinyal dan reap | PASS | `SIGTERM` lintas session; zombie memegang PID sampai di reap; TERM lalu KILL ke group pembantu menghentikan semua turunan |
| Lock | PASS | lock di fd 3 bertahan setelah daemon menutup fd dan keluar; `LOCK_UN` di salinan daemon ikut melepas lock child (bukti aturan "hanya `close`"); file lock yang dihapus membuat file baru bisa di lock (bukti pengaman observasi) |
| Spool seperti tertulis | FAIL, diputuskan di Keputusan 19 dan 20 | `EPERM` di `O_RDONLY`; satuan 4 KiB membuat alokasi tumbuh dan `SEEK_DATA` basi |
| Spool dengan pembaca `O_RDWR` dan satuan halaman | PASS | lihat angka di bawah |

Angka spool di bawah beban AC-12 (fake 2000 baris per detik selama 60 detik, dibaca dan di punch setiap 500 ms):

| Varian | Ditulis per stream | Alokasi paling tinggi per spool | Dua spool bersama | `SEEK_DATA` setelah run |
| --- | --- | --- | --- | --- |
| Satuan `f_bsize` 4 KiB | sekitar 6,0 MB | 1638400 B | 3272704 B, terus naik dari 2256896 B di detik 15 | 32768, basi (6008832 B sudah dibebaskan) |
| Satuan halaman 16 KiB | sekitar 6,0 MB | 1052672 B, puncaknya tidak naik setelah detik 15 | 2105344 B (2064384 B di run sebelumnya) | 6012928, tepat |
| Satuan halaman, fsync setelah setiap punch | sekitar 6,0 MB | 1052672 B | 2105344 B | tepat |
| Tanpa punch (kontrol, 10 detik) | sekitar 1 MB | paling banyak 61440 B di atas EOF | | |
