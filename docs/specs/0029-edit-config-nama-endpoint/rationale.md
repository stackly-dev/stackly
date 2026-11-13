# 0029. Edit config, nama, dan endpoint: alasan

[Kembali ke spec](index.md)

## Context

Setelah fitur 20 sampai 26, sebuah instance bisa dibuat, dijalankan, di restart, dan di reload, tetapi settings yang dipilih saat create tidak bisa diubah lagi. Registry sudah menyiapkan tempatnya: `config_revisions` dengan `previous_id`, `rendered_sha256`, dan kode `status` terbuka yang disebut spec 0005 sebagai milik fitur 29, plus folder `config-history/` yang dibuat create tetapi tidak pernah diisi. Settings schema spec 0009 sudah mengklasifikasi setiap field (`reload`, `restart`, `immutableAfterInit`, `managed`, `secret`, `advanced`), dan spec 0026 sudah punya langkah restart dan reload yang aman. Yang belum ada adalah aturan bagaimana perubahan menjadi revision, kapan diterapkan, dan apa yang terjadi kalau gagal.

PRD meminta beberapa hal yang saling menekan. REQ-015 dan AC-015 meminta config yang tidak valid tidak pernah menggantikan config aktif dan tidak memicu restart parsial. PAGE-06 meminta histori minimal last known good. REQ-015 juga meminta drift dari edit di luar Stackly terlihat. Masalahnya, spec 0009 dan 0010 merender ulang setiap file config sebelum setiap spawn, jadi edit manual hilang diam diam di start berikutnya kalau tidak ada yang mencegahnya. AC-026 meminta provider tanpa hot reload tidak pernah mengklaim reload. BR-003 membatasi rename ke label saja.

Port punya sifat lain. Port tidak ada di `config_values`, melainkan di `instance_endpoints`, dan pemeriksaan konfliknya (wildcard, IPv4 dan IPv6, instance lain, proses luar) sudah ditetapkan spec 0020. Scope meminta ganti port tercatat sebagai operasi sendiri (AC-045 mengaudit change port), dan instance yang sedang memakai port itu sendiri tidak boleh gagal pemeriksaan karena dirinya.

Tanpa keputusan ini, `/develop` akan menebak apakah Effective File bisa diedit, apakah apply dan restart dirangkai client, apa itu last known good, dan apakah drift boleh ditimpa. Tebakan yang salah di dua yang terakhir langsung melanggar AC-015 atau menghilangkan edit pengguna.

## Options considered

### K1. Effective File

**Opsi A: read only (dipilih).** Pro: semua perubahan lewat satu jalur validasi typed, jadi field managed tidak pernah bisa ditimpa. Kontra: setting di luar schema tidak bisa dipakai.

**Opsi B: blok override yang bisa diedit.** Pro: fleksibel untuk setting langka. Kontra: butuh validasi per format (`postgresqlConf`, `natsConf`, yaml) dan aturan yang menolak key managed di override.

**Opsi C: seluruh file bisa diedit dan di parse kembali.** Pro: paling fleksibel. Kontra: parser per format, dan baris yang tidak memetakan ke setting membuat revision tidak jujur.

### K2. Apply pada instance yang jalan

**Opsi A: langsung reload atau restart (dipilih).** Pro: sesuai tombol mockup, dan satu aksi berarti perubahan benar benar berlaku. Kontra: tidak ada cara menyiapkan perubahan untuk nanti tanpa me restart.

**Opsi B: tambah Save for later.** Pro: kamu bisa menunda restart. Kontra: state revision tertunda yang aktif di registry tetapi tidak di proses, yang membuat drift dan last known good lebih sulit dijelaskan.

### K3. Config valid yang gagal

**Opsi A: tawarkan roll back (dipilih).** Pro: tidak ada restart kedua yang tidak kamu minta, dan kamu bisa membaca log dulu. Kontra: satu klik tambahan untuk pulih.

**Opsi B: roll back otomatis.** Pro: pulih tanpa interaksi. Kontra: restart kedua diam diam, dan kegagalan yang bukan karena config (port direbut, data) ikut memicu roll back yang salah.

**Opsi C: gagal saja.** Pro: paling sederhana. Kontra: tidak ada bantuan menuju config yang terbukti sehat.

### K4. Definisi last known good

**Opsi A: revision terakhir yang pernah ready (dipilih).** Pro: fakta yang bisa dibuktikan. Kontra: instance lama tanpa `ready_at` belum punya last known good.

**Opsi B: revision terakhir yang diterapkan tanpa error.** Pro: sederhana. Kontra: config yang crash setelah start tetap dianggap baik.

### K5. Penyimpanan last known good

**Opsi A: kolom `ready_at` per revision (dipilih).** Pro: histori menunjukkan setiap revision yang pernah sehat, query sederhana. Kontra: satu kolom baru.

**Opsi B: pointer di `instances`.** Pro: satu lookup. Kontra: histori tidak bisa menunjukkan revision lama mana yang baik.

**Opsi C: kode status saja.** Pro: tanpa migrasi. Kontra: revision yang aktif sekaligus baik butuh dua status.

### K6. Port di revision

**Opsi A: snapshot endpoint di revision (dipilih).** Pro: histori dan roll back mencakup port. Kontra: dua tempat menyimpan port, dijaga sama oleh satu transaksi.

**Opsi B: settings saja.** Pro: tanpa kolom. Kontra: port yang salah harus diperbaiki tangan, dan roll back tidak lengkap.

### K7. Retensi histori

**Opsi A: 20 terbaru plus aktif dan last known good (dipilih).** Pro: ukuran terbatas, cukup untuk alat lokal. Kontra: konteks lama hilang.

**Opsi B: simpan semua.** Pro: aturan paling sederhana. Kontra: tumbuh tanpa batas.

**Opsi C: minimum (aktif, sebelumnya, last known good).** Pro: tepat "histori minimal". Kontra: hampir tidak ada konteks.

### K8. Kembali ke revision lama

**Opsi A: Restore dari baris mana pun ke form (dipilih).** Pro: satu jalur dengan validasi penuh dan diff. Kontra: dua langkah (Restore, lalu Apply).

**Opsi B: hanya roll back last known good.** Pro: lebih sedikit UI. Kontra: revision lain tidak bisa dipakai lagi.

### K9. Pilihan saat drift

**Opsi A: Keep Revision, start diblokir (dipilih).** Pro: edit tidak pernah ditimpa tanpa pilihan, sesuai teks mockup. Kontra: edit yang disengaja harus dibuang atau dijadikan field schema.

**Opsi B: Keep atau Adopt.** Pro: edit bisa dipertahankan. Kontra: parser per format; baris tanpa setting tidak bisa diadopsi.

**Opsi C: peringatan saja, start menimpa.** Pro: paling sederhana. Kontra: edit hilang tanpa pilihan eksplisit, bertentangan dengan scope.

### K10. Kapan drift diperiksa

**Opsi A: saat dibaca dan sebelum render (dipilih), ditambah sekali saat daemon start.** Pro: tidak ada timer, biaya nol untuk instance idle. Kontra: edit di tengah jalan baru terlihat saat kamu membuka instance atau start. Pemeriksaan sekali saat daemon start adalah rekomendasi spec ini (bukan pertanyaan terpisah): blocker hanya ada di memori, jadi tanpa pemeriksaan itu item drift Overview hilang setiap daemon restart.

**Opsi B: juga dengan timer.** Pro: Overview selalu segar. Kontra: membaca file per instance terus menerus.

### K11. Rename

**Opsi A: `name` saja, lifecycle apa pun (dipilih).** Pro: label tidak menyentuh proses, sesuai BR-003. Kontra: script yang memegang nama lama harus diperbarui.

**Opsi B: `name` dan `display_name`.** Pro: satu aksi untuk keduanya. Kontra: dua label untuk dijaga di satu sheet.

**Opsi C: `name` saja, hanya saat berhenti.** Pro: aman untuk script. Kontra: friksi untuk perubahan label.

### K12. Yang bisa diubah di endpoint

**Opsi A: port dan mode Fixed atau Automatic (dipilih).** Pro: menutup kebutuhan umum dengan pemeriksaan create yang sama. Kontra: host tetap loopback.

**Opsi B: port, mode, dan host.** Pro: bisa membuka ke LAN. Kontra: memperluas exposure dan butuh peringatan serta pemeriksaan tambahan.

**Opsi C: angka port fixed saja.** Pro: paling kecil. Kontra: tidak bisa pindah ke atau dari Automatic.

### K13. Port milik sendiri

**Opsi A: port yang sama dengan `bound_port` sendiri melewati probe bind (dipilih).** Pro: "pin port yang sekarang" bekerja saat jalan. Kontra: satu pengecualian di pemeriksaan port.

**Opsi B: selalu probe.** Pro: aturan sederhana. Kontra: kamu harus stop dulu untuk mem pin port yang sedang dipakai.

### K14. Susunan apply dan restart

**Opsi A: satu operasi `apply_config` (dipilih).** Pro: satu journal, satu cancel, satu hasil, dan tidak ada revision tersimpan tanpa restart. Kontra: langkah spec 0026 harus bisa dipakai sebagai sub langkah.

**Opsi B: client merangkai dua panggilan.** Pro: memakai method yang ada. Kontra: crash atau Ctrl C di antaranya meninggalkan pekerjaan setengah jadi.

### K15. Nomor revision

**Opsi A: kolom `number` per instance (dipilih).** Pro: mudah dibaca dan diurutkan, `--revision N` di CLI. Kontra: satu kolom dan aturan keunikan.

**Opsi B: ID pendek.** Pro: tanpa kolom. Kontra: sulit dibaca dan tidak berurutan.

### K16. Isi Pending Changes

**Opsi A: baris per setting plus pratinjau file (dipilih).** Pro: diff yang mudah dibaca, dan file yang akan ditulis tetap terlihat. Kontra: pratinjau port Automatic memakai `defaultPort`.

**Opsi B: baris per setting saja.** Pro: lebih sederhana. Kontra: kamu tidak melihat file yang akan berubah.

**Opsi C: diff file saja.** Pro: paling dekat dengan disk. Kontra: daftar `routes` dan JSON tampil sebagai teks mentah.

### K17. Input CLI

**Opsi A: flag yang sama dengan create (dipilih).** Pro: satu pola untuk semua perintah, mudah untuk satu key. Kontra: perubahan besar butuh banyak flag.

**Opsi B: file nilai.** Pro: round trip dari `get --json`. Kontra: canggung untuk satu key.

**Opsi C: keduanya.** Pro: lengkap. Kontra: dua jalur input untuk dites.

### K18. Field secret

**Opsi A: reference read only (dipilih).** Pro: tidak ada password tersimpan yang berbeda dari password di database. Kontra: rotasi tetap di luar Stackly.

**Opsi B: bisa diedit, tersimpan saja.** Pro: bisa mengganti secret. Kontra: untuk PostgreSQL, Keychain dan database berbeda dan koneksi rusak.

### K19. Apply per lifecycle

**Opsi A: simpan untuk yang berhenti, tolak yang sibuk (dipilih).** Pro: config yang membuat instance gagal bisa diperbaiki tanpa start dulu. Kontra: tiga kelompok aturan untuk dijelaskan.

**Opsi B: hanya stopped dan running.** Pro: lebih ketat. Kontra: memblokir perbaikan config yang menyebabkan `failed`.

### K20. Lain lain

Item drift Overview dari blocker `ConfigDrift` (dipilih, lawannya menunggu fitur lain), roll back di banner, History, dan CLI (dipilih, lawannya hanya Restore), sheet port dari kartu Endpoints (dipilih, lawannya baris di form Configuration), dua kind operasi untuk settings dan port (dipilih, lawannya satu `apply_config`), dan sheet Rename dari menu More (dipilih, lawannya edit judul langsung).

## Rationale

Inti keputusan ini adalah satu jalur. Setiap perubahan settings atau port, termasuk roll back dan Restore, menjadi revision baru lewat satu fungsi evaluasi dan satu `commit_guarded`, lalu diterapkan dengan langkah spec 0026 di bawah operasi yang sama. Itu yang membuat AC-015 bisa dijamin: validasi berjalan sebelum ada operasi, jadi config yang salah tidak pernah menyentuh registry, file, atau proses. Itu juga yang membuat GUI dan CLI selalu memberi kode dan field error yang sama, karena keduanya memanggil evaluator yang sama dengan create (pola spec 0020).

Last known good didefinisikan sebagai fakta (`ready_at`) karena kegunaannya justru saat config yang valid gagal di dunia nyata. Roll back tidak otomatis karena kegagalan setelah restart sering bukan karena config (port direbut proses luar, data dipakai), dan restart kedua yang diam diam menyembunyikan penyebab itu. Snapshot endpoint di revision membuat roll back lengkap, dan satu transaksi yang menulis revision dan `instance_endpoints` sekaligus mencegah keduanya berbeda.

Drift diputuskan dengan memperhatikan render sebelum spawn: tanpa pemblokiran, edit manual pasti hilang di start berikutnya, sehingga peringatan saja tidak jujur. Salinan di `config-history/` dipilih daripada hash karena port Automatic membuat isi file berubah setiap start; membandingkan dengan salinan render terakhir selalu benar dan sekaligus memberi Show Diff dan Effective File histori. File sensitif tidak pernah disalin, jadi drift dan pratinjau tidak pernah butuh Keychain.

Rekomendasi yang diputuskan spec ini tanpa pertanyaan terpisah: kode `supervisor.instance_not_running` untuk `starting` (konsisten dengan restart spec 0026, runner up kode baru `config.instance_busy`); kind ketiga `resolve_config_drift` untuk Keep Revision (runner up memakai `apply_config` tanpa revision, yang mengaburkan journal); `BusyPolicy::Queue` dengan pemeriksaan `baseRevision` setelah antre (runner up `Reject`, yang memaksa klik ulang saat restart lain berjalan); revision instance naik satu pada setiap aktivasi (menjawab pertanyaan terbuka spec 0005, supaya write yang basi tertangkap); pemeriksaan drift sekali saat daemon start (lihat K10); pratinjau port Automatic dengan `defaultPort` dan catatan (runner up tidak menampilkan file sampai start, yang membuat Effective File kosong untuk instance baru).

Cross check (Fable 5.1, 2026-11-02): temuan yang diterapkan atas pilihan user adalah penulisan `ready_at` lewat `applied_revision` di slot proses (readiness tidak disimpan di registry), dua varian `FieldCode` baru, fake provider `0.4.0` dengan file config tidak sensitif, endpoint lewat aggregate di `ActivateConfigRevision`, salinan histori per file, titik panggil drift dan respawn, gerbang `apply_allowed` dan `rename_allowed`, interrupt policy per kind, kolom `origin` dan `source_number`, secret caller, dan dampak yang dihitung ulang setelah commit.

## Perubahan 2026-11-13 (fitur 44 dan 47)

Dibawa lead scope 08 lewat brief `arc-update-all`, dari *Update spec lain* spec 0044 dan 0047. AC-14 ditambah satu kalimat; makna teks lamanya tidak berubah.

Baris *Batas dengan spec lain* "lease environment saat ganti port: fitur 47" dijawab spec 0047 K12: ditolak dulu, lalu bisa di override dengan dampak yang tampil dari `dryRun`. Runner up nya (menolak mutlak) ditolak di sana karena Stackly dipakai satu orang di satu Mac, dan memaksa Down untuk ganti port terlalu berat. Rename dan apply config tidak dijaga (spec 0047 K11) karena tidak mengubah apa yang environment harapkan.
