# 0034. Doctor dan diagnostic bundle: rationale

Catatan keputusan untuk [index.md](index.md). `/develop` membaca index; file ini menyimpan alasannya.

## Context

Saat instance gagal (JOURNEY-05), Stackly hari ini memberi potongan informasi di banyak tempat: `lastFailure` dan blocker di detail instance, `ReconcileFinding` di event rekonsiliasi, operasi `needs_recovery` di daftar operasi, state agent di seksi Diagnostics, dan log. Beberapa spec yang sudah `Accepted` sengaja menunda bagian terakhirnya ke fitur 34: bukti identitas proses yang tidak terbukti (spec 0010), operasi dan folder scratch yang ditinggalkan (spec 0007), baris registry dengan kode yang tidak dikenal (spec 0005), sebab daemon yang gagal start di bawah launchd (spec 0015), panduan perbaikan PostgreSQL (spec 0019), kapasitas (spec 0022), dan teks yang sudah menyuruh orang menjalankan `stackly doctor` padahal perintah itu belum ada (spec 0021).

Batasnya keras. PRD REQ-019 dan scope fitur 34 menyatakan diagnosis bukan izin untuk menghapus lock, mengubah data, atau menghentikan proses. AC-019 meminta PID record yang menunjuk proses lain dilaporkan beserta bukti tanpa menyentuh proses dan datanya. NFR-016 meminta tidak ada data yang keluar tanpa preview dan persetujuan. AGENTS.md menambah aturan arsitektur: hanya daemon yang membaca dan menulis registry, CLI dan GUI tidak menjalankan proses, tidak ada `launchctl` di kode produk (spec 0015), `EventKind` tertutup, dan secret hanya keluar lewat modul `EXPOSE_ALLOWED`.

Doctor juga harus tetap berguna justru saat keadaan paling buruk: registry rusak (recovery mode), daemon tidak bisa start, atau protokol client dan daemon berbeda. Keadaan itu terlihat dari tempat yang berbeda; sebagian hanya bisa dilihat daemon, sebagian hanya bisa dilihat client.

## Options considered

### Option 1: method daemon read only, check client kecil, bundle dibangun saat preview

Daemon menjalankan semua check yang butuh registry, proses, dan filesystem Stackly lewat satu method `doctor.run` yang membalas laporan terstruktur dengan teks. CLI dan GUI menambah tiga check yang hanya terlihat dari luar daemon (koneksi, protokol, agent). Bundle dibangun daemon saat preview ke staging, lalu disalin persis ke path pilihan client saat export.

**Pros**:

- Satu implementasi dan satu tabel teks untuk CLI dan GUI (REQ-028).
- Memenuhi aturan bahwa hanya daemon yang membaca registry dan proses.
- Yang kamu lihat di preview adalah byte yang tersimpan.

**Cons**:

- Daemon menulis satu file di luar root miliknya (tujuan export).
- Bundle tidak bisa dibuat saat daemon mati.

### Option 2: doctor di client

CLI dan GUI masing masing membaca file root, registry read only, dan proses sendiri.

**Pros**:

- Bekerja penuh saat daemon mati.

**Cons**:

- Melanggar aturan bahwa hanya daemon yang membaca registry dan menyentuh proses; GUI tidak boleh link Rust, jadi semua check ditulis dua kali (Rust dan Swift).
- Dua sumber teks dan dua perilaku yang pasti bergeser.

### Option 3: doctor sebagai operasi dengan progress dan event

`doctor.run` memulai operasi spec 0007 dengan tahap per check, progress lewat event, dan hasil di registry.

**Pros**:

- Progress per check terlihat, bisa dibatalkan, tercatat di audit spec 0035.

**Cons**:

- Operasi adalah mesin untuk mutasi; doctor tidak memutasi apa pun, dan menulis hasilnya ke registry justru melanggar janji read only, juga mustahil di recovery mode.
- Butuh lock key dan slot operasi, sehingga doctor bisa tertahan tepat saat ada operasi macet yang ingin didiagnosis.

### Option 4: bundle dialirkan ke client

Daemon mengirim isi bundle lewat IPC dalam potongan, client menulis file.

**Pros**:

- Daemon tidak pernah menulis di luar root.

**Cons**:

- Frame kontrol 1 MiB memaksa method baca bertahap (offset, base64, ulang), lebih banyak kontrak dan state.
- CLI dan GUI sama sama menulis file dan menangani partial dan replace; dua implementasi untuk hal yang sama.

## Rationale

Option 1 dipilih karena kekuatan yang paling menentukan adalah aturan kepemilikan: registry, proses, dan root hanya disentuh daemon, dan GUI tidak link Rust. Itu membuat doctor di client (Option 2) mahal dan rapuh. Option 3 bertentangan langsung dengan sifat doctor sebagai pembacaan yang aman di recovery mode. Untuk bundle, menulis satu file dengan create exclusive dan rename di path pilihan client tidak menambah hak (daemon dan client adalah user yang sama, peer diperiksa spec 0006), sedangkan streaming (Option 4) menggandakan logika file di dua client.

Kelemahan Option 1 yang paling terasa adalah daemon mati: saat itu yang tersedia hanya check client, ditambah catatan gagal start baru yang ditulis daemon sebelum keluar. Bundle untuk keadaan itu dicatat sebagai Follow-up, karena butuh keputusan tersendiri tentang client yang membaca log daemon.

## Keputusan

Setiap keputusan di bawah diambil dengan opsi yang direkomendasikan (user mendelegasikan keputusan di wave ini). Format: pilihan, alasan, runner up.

1. **Doctor bukan operasi dan tidak diaudit.** Alasan: tidak ada state yang berubah; operasi butuh registry yang bisa ditulis dan slot. Runner up: operasi `doctor` dengan progress. Disepakati dengan spec 0035 (2026-11-10).
2. **Export bundle bukan operasi.** Alasan: tidak mengubah state Stackly, hanya menulis file milik user; dicatat satu baris log `diagnostics_exported`. Runner up: operasi `diagnostics_export` dengan baris audit. Disepakati dengan spec 0035.
3. **Teks temuan dikirim daemon.** Alasan: satu tabel teks untuk CLI dan GUI, dan client lama tetap bisa menampilkan temuan baru dari daemon baru. Runner up: tabel teks per client berdasarkan kode (pola `InstanceFailureCopy`), yang menyembunyikan temuan baru dari client lama.
4. **Severity tiga tingkat, check yang tidak bisa jalan menjadi Warning `check_unavailable`.** Alasan: scope meminta Error, Warning, OK; tingkat keempat "skipped" menambah state UI tanpa keputusan baru bagi kamu. Runner up: severity `skipped`.
5. **Aksi sebagai struct datar dengan `kind` terbuka.** Alasan: enum bertag di repo ini tertutup (blocker baru adalah major), sedangkan aksi akan bertambah (fitur 55). Runner up: enum bertag `DoctorAction`.
6. **Paling banyak satu aksi per temuan.** Alasan: `CheckItem` punya satu aksi; aksi kedua menambah komponen baru. Runner up: daftar aksi.
7. **Aksi hanya navigasi atau alur yang sudah ada; `change_port` membuka `EndpointsSheet` terisi, bukan langsung mengubah.** Alasan: perubahan port lewat sheet yang sudah menilai port (dry run) dan revision base; doctor tidak perlu membawa revision. Runner up: Use Port langsung memanggil `instance.change_endpoints` seperti mockup, yang butuh revision di temuan dan melewati penilaian sheet.
8. **Bukti identitas pasif tanpa `probe_lock`.** Alasan: `probe_lock` mengambil `flock` sesaat dan bisa membuat start yang bersamaan gagal; ketujuh check lain tidak menyentuh lock. Lock diambil dari blocker tersimpan. Runner up: `gather_evidence` penuh lewat serialisasi supervisor per instance, yang menambah jalur baru ke supervisor.
9. **Pemilik port lewat port platform baru `ListenerLookup` (libproc), hanya proses user ini.** Alasan: mockup dan runbook 35.3 meminta proses pemilik; libproc tidak butuh hak root atau proses luar (`lsof` dilarang karena spawn). Runner up: hanya melaporkan "port in use" tanpa pemilik.
10. **Bind probe hanya untuk instance yang tidak dipegang operasi aktif.** Alasan: probe yang sama dipakai preflight create; melewati instance yang sedang start mengecilkan jendela tabrakan. Runner up: hanya pencarian pasif, yang tidak melihat listener milik user lain atau sistem.
11. **Remedy provider sebagai blok manifest `remedies`, teks saja, dibaca dari revision catalog.** Alasan: provider tetap deklaratif (PRD Definition of Done), dan remedy yang hanya teks aman dibaca dari revision yang lebih baru sehingga instance yang di pin tetap mendapatnya. Runner up: check khusus PostgreSQL di core (melanggar sifat deklaratif), atau tanpa remedy dan menunda Follow-up spec 0019.
12. **Remedy tidak pernah menghapus `postmaster.pid`.** Alasan: spec 0019 menyatakan file itu juga penjaga terhadap postmaster luar; penghapusan adalah keputusan kamu setelah membaca bukti. Runner up: aksi hapus dengan konfirmasi saat PID tidak hidup.
13. **Catatan gagal start ditulis daemon sendiri, tanpa `launchctl`.** Alasan: spec 0015 melarang `launchctl` di kode produk, dan sebab paling umum (root terlalu terbuka, config salah) diketahui daemon sebelum keluar. Runner up: membaca status job lewat API privat launchd.
14. **`binary_replaced` dari waktu ubah binary dibanding `lastLaunchedAt`.** Alasan: satu satunya tanda last exit 78 yang bisa dilihat tanpa `launchctl`; langkah perbaikannya (Start stacklyd, yaitu `relaunch`) sudah ada dan aman. Runner up: tidak melaporkan kasus itu.
15. **Check `runtimes` hanya membaca state dan memeriksa keberadaan file, tanpa digest.** Alasan: digest ulang dan transisi state milik fitur 55; pemeriksaan keberadaan murah dan read only. Runner up: verifikasi digest penuh di doctor.
16. **Registry: `quick_check` dan pindai kode tidak dikenal lewat koneksi baca.** Alasan: storage sudah memakai `quick_check`; `integrity_check` lebih lambat tanpa manfaat besar untuk registry kecil. Runner up: `integrity_check` seperti teks mockup.
17. **Recovery mode: tiga check berjalan, sisanya `check_unavailable`.** Alasan: spec 0005 melarang baca registry di recovery; disk, root, dan daemon tidak butuh registry. Runner up: doctor menolak di recovery.
18. **Batas waktu 5 detik per check dan 15 detik per run, setiap call run baru, paling banyak 2 run (semaphore).** Alasan: laporan selalu kembali walau satu check macet, dan call yang datang tepat setelah kamu menekan Apply selalu melihat keadaan baru; menggabungkan call ke run yang sedang berjalan (single flight) bisa memberi laporan yang mulai sebelum perubahan itu (cross check). Runner up: single flight per params.
19. **Laporan tidak disimpan daemon.** Alasan: tidak ada tabel atau file baru, tidak ada laporan basi; GUI memegang laporan terakhirnya di memori. Runner up: laporan terakhir di memori daemon dengan method baca.
20. **GUI menjalankan doctor otomatis pada daemon baru, saat halaman tampil dengan laporan lebih dari 5 menit, dan saat diminta.** Alasan: badge dan subjudul butuh laporan; run read only dan berbatas. Runner up: hanya saat Run Doctor ditekan (badge kosong sampai itu).
21. **Badge Diagnostics hanya menghitung Error.** Alasan: Warning seperti disk 82 persen tidak boleh terasa seperti kegagalan; badge native spec 0013. Runner up: Error ditambah Warning seperti angka mockup.
22. **Bundle dibangun saat preview dan export menyalin byte yang sama.** Alasan: "preview" harus berarti persis yang disimpan (NFR-016); export juga menjadi murah dan bisa diulang. Runner up: export membangun ulang bundle.
23. **Daftar isi bundle ada di sheet, bukan di halaman.** Alasan: daftar nyata (ukuran, baris, terpotong) hanya ada setelah daemon membangun preview, dan membangun bundle setiap halaman dibuka boros. Runner up: preview otomatis setiap halaman tampil.
24. **Format zip dengan crate `zip` yang sudah ada.** Alasan: tanpa dependency baru, mockup menyebut zip, dan Finder membukanya langsung. Runner up: tar.gz.
25. **Log instance disalin seperti tersimpan; entri lain lewat `Redactor`; folder home menjadi `~`.** Alasan: spec 0027 menetapkan log store hanya ditulis setelah redaksi dan tidak diredaksi ulang; entri lain dibangun dari DTO dan log daemon; `~` mengurangi data pribadi tanpa menghilangkan arti path. Runner up: semua entri lewat `Redactor`, yang bertentangan dengan aturan spec 0027.
26. **File config yang dirender tidak ikut bundle.** Alasan: config bisa memuat secret plaintext (residual spec 0011), dan `Redactor` hanya mengenali nilai yang ia pelajari. Runner up: ikutkan config lewat `Redactor`.
27. **Batas bundle: 2000 baris, 1 MiB per log instance, 4 MiB log daemon, 50 instance, 64 MiB.** Alasan: angka 2000 dari mockup; batas byte menjaga zip tetap bisa dikirim lewat chat atau email. Runner up: tanpa batas byte.
28. **Export menolak menimpa kecuali `replace`; GUI mengirim `replace` hanya setelah save panel meminta Replace.** Alasan: tidak pernah menghapus file kamu diam diam, tetapi tetap menghormati konfirmasi macOS. Runner up: selalu menolak file yang ada.
29. **Exit `stackly doctor`: 0 tanpa Error, 7 (`cli.doctor_found_errors`) dengan Error, kode koneksi saat daemon mati, `result` selalu terisi.** Alasan: exit code hanya dari kategori (spec 0008) dan pola hasil parsial spec 0015. Runner up: exit 0 selalu dengan JSON sebagai satu satunya sinyal.
30. **Tanpa migrasi dan tanpa event kind baru; satu minor protokol.** Alasan: tidak ada data persisten baru selain dua file kecil; event baru adalah major. Runner up: event `doctor.finished` (major).

31. **Konflik port lewat `judge_change`, bukan bind probe sendiri.** Alasan: fungsi itu sudah menilai port endpoint instance yang ada terhadap instance lain dan bind, dan memberi saran; doctor tidak membuat penilaian kedua. Runner up: `PortProbe::try_bind` langsung ditambah `suggest`, yang butuh `CreateDraft` palsu.
32. **`gather_evidence` dipecah, bukan disalin.** Alasan: satu jalur kode untuk ketujuh check identitas, jadi doctor dan supervisor tidak bergeser. Runner up: fungsi pasif terpisah.
33. **`binary_replaced` sebagai Warning.** Alasan: inferensi waktu ubah file bisa salah setelah stop yang disengaja (cross check). Runner up: Error.
34. **Remedy cocok menurut urutan manifest; revision catalog, lalu paket pinned, lalu tanpa remedy.** Alasan: aturan sederhana yang bisa diprediksi penulis provider. Runner up: entri paling spesifik menang.
35. **Daftar kolom berkode `CODED_COLUMNS` di storage, dipakai mapper baris.** Alasan: kolom berkode tidak bisa dibaca tanpa terdaftar, jadi pindai tidak bisa ketinggalan; query `NOT IN` murah. Runner up: pindai setiap tabel dengan parser domain penuh.
36. **Penggantian `~` sesudah `Redactor`, dengan batas karakter path.** Alasan: tidak mengganti bagian nama folder lain, dan tidak mengganggu pencocokan nilai secret. Runner up: penggantian prefix polos.
37. **Anggaran 64 MiB dengan urutan tetap, entri JSON tidak pernah dibuang.** Alasan: laporan dan daftar instance paling berguna untuk maintainer; log paling bisa dikorbankan. Runner up: potong semua log rata.
38. **Export menyalin dari fd yang terbuka; preview yang tergusur di unlink.** Alasan: penggusuran tidak bisa merusak export yang sedang berjalan. Runner up: mutex per preview.
39. **Catatan gagal start hanya dibaca kalau file biasa milik uid dengan mode 0600.** Alasan: catatan itu ada di folder yang bisa saja salah izin; reader tidak boleh percaya file yang bisa ditanam user lain. Runner up: baca apa saja yang ada.
40. **`--check` CLI hanya untuk check daemon; check client selalu jalan; error koneksi menang atas exit 7.** Alasan: check client murah dan dibutuhkan untuk mencapai daemon. Runner up: `--check` menyaring keduanya.

## Cross check (Another model, 2026-11-10)

Cross check read only memberi 27 catatan. Semua gap kelengkapan keputusan diterapkan dengan rekomendasinya (user mendelegasikan keputusan): entry point konflik port (`judge_change`), pemecahan `gather_evidence` dan tabel *Hasil check processes* (boot lain, PID milik user lain, `holdsLock`), run baru per call dengan semaphore, pembatalan check yang macet, daftar `CODED_COLUMNS`, registry di recovery tanpa koneksi, cakupan `not_ready` dan `lastFailure` null, urutan dan fallback remedy serta token `socketDir`, pembacaan `pidFile` yang aman, sumber setiap entri bundle (fungsi core, `host_facts`, rotasi log daemon, format baris log instance), cara `Redactor` dipakai tanpa `expose`, aturan `~`, urutan anggaran 64 MiB, staging preview, validasi tujuan export (`invalid_name`, `lstat`), partial yang tertinggal, kode dan pembacaan catatan gagal start, `binary_replaced` sebagai Warning, aturan `--check` dan prioritas exit, pemicu auto run GUI, definisi `errors`, ringkasan OK per check, `PortOwner` bertipe, urutan temuan, dan masa tenggang `scratch_leftover`. Catatan yang tidak mengubah apa pun: field `stopped` di `IdentityEvidence` DTO yang sudah ada (bukan opsional, tidak disentuh), dan `zip` di workspace yang sudah punya writer tanpa fitur tambahan (waktu entri disetel `createdAt` supaya deterministik).

## Pertanyaan batas dengan spec saudara

- Spec 0033 (2026-11-10): setuju. Doctor membaca `InstanceSummary.restart` dan `lastFailure` (`code`, `at`, `exitCode`, `signal`, `excerpt`) apa adanya; crash loop adalah lifecycle `failed` dengan `lastFailure.code` `crash_loop` dan `restart.exhausted` true. Banner memanggil `ShellNavigator.openDiagnostics(focus: .instance(id))` dengan label "Diagnose…" dan hint `stackly doctor --instance <name>`. Port saran memakai pencari yang sama dengan start (`instance::create::port`); doctor memanggil `judge_change`, yang juga dipakai ganti endpoint spec 0029.
- Spec 0035 (2026-11-10): setuju. Doctor dan export bukan operasi dan tidak diaudit; aksi doctor membuka alur yang sudah ada; `ShellNavigator.openOperation(_ id: UUID)` adalah tujuan `show_operation`; audit tidak ikut bundle (jalurnya `stackly audit list --json`). `operations.json` membawa field `cancel` baru dari spec 0035 tanpa perubahan di sini.

## Catatan untuk lead

- Nomor protokol 3.5 hanya sementara; lead menetapkannya saat build. Spec ini tidak butuh migrasi.
- Teks AC dan keputusan spec lain tidak diubah; yang perlu diterapkan ada di *Usulan update untuk spec lain* di index.
