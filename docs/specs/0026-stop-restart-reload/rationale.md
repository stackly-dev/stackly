# 0026. Stop, restart, dan reload: alasan

[Kembali ke spec](index.md)

## Context

Fitur 10 (spec 0010) membangun stop yang aman: hanya strategi provider, identitas diperiksa ulang sebelum setiap sinyal, dan saat anggaran habis operasi gagal `supervisor.stop_timeout` sementara instance tetap `stopping` dan proses tetap diawasi. Spec itu sengaja berhenti di sana: tidak ada eskalasi sinyal, tidak ada restart sebagai aksi pengguna, dan `execute_reload` sudah ada di core tetapi belum punya method. Operation kind `restart_instance` dan `reload_instance` sudah tercatat di domain dan protokol dengan `CloseInterrupted`, dan peta perintah spec 0069 sudah mencadangkan baris Restart dan Reload.

PRD bagian 19.3 meminta tiga hal yang saling terkait. Setelah timeout, Stackly harus menunjukkan bahwa proses masih stopping dan menawarkan force dengan risiko yang jelas. Restart harus berarti stop, pastikan sudah berhenti, pastikan endpoint dan lock data bebas, lalu start. Dan permintaan stop serta start yang bersamaan harus diserialisasi. REQ-011 dan AC-067 menambah batas keras: forced kill bukan default dan tidak boleh menyasar proses yang kepemilikannya tidak terverifikasi. BR-016 meminta hal yang tidak didukung (reload di NATS) tampil sama di GUI, CLI, dan daemon, dan AC-026 melarang klaim reload palsu.

Ada juga tekanan dari fitur di depan. Fitur 49 tidak boleh memberi label clean pada backup cold dari data yang tidak berhenti bersih, jadi fakta "bagaimana proses terakhir berakhir" harus tersimpan di suatu tempat. Menu Instance spec 0069 menargetkan baris tabel yang terpilih, bukan hanya halaman detail, sehingga app harus tahu capability reload tanpa memuat detail. Dan kebijakan interaksi 2026-10-24 menghapus shortcut khusus, jadi alasan nonaktif harus terlihat lewat cara native (subtitle menu), bukan hanya tooltip hover.

Tanpa keputusan ini, `/develop` akan menebak kapan `SIGKILL` boleh dikirim, apakah restart dirangkai client atau daemon, dan dari mana app tahu provider bisa reload. Tebakan yang salah di yang pertama langsung melanggar invariant keamanan spec 0010.

## Options considered

### K1. Kapan force boleh dipakai

**Opsi A: hanya saat `stopping` (dipilih).** Force adalah langkah kedua setelah stop normal gagal selesai. Pro: sesuai PRD 19.3, dan `SIGKILL` tidak pernah menjadi kebiasaan. Kontra: dua langkah saat kamu memang ingin cepat.

**Opsi B: kapan saja lewat flag.** Pro: cepat. Kontra: menjadikan `SIGKILL` default yang gampang bagi database, bertentangan dengan REQ-011.

**Opsi C: eskalasi otomatis setelah timeout.** Pro: satu langkah. Kontra: `SIGKILL` terjadi tanpa keputusan pada saat itu, padahal risikonya baru jelas setelah stop gagal.

### K2. Sasaran force

**Opsi A: `SIGKILL` ke proses utama setelah pemeriksaan ulang (dipilih).** Pro: paling sempit, satu identitas yang terbukti. Kontra: child yang bandel tidak ikut berhenti.

**Opsi B: `SIGKILL` ke seluruh group setelah leader lolos.** Pro: membersihkan child. Kontra: menyinyal anggota yang identitasnya tidak diperiksa satu per satu.

**Opsi C: `SIGQUIT` lalu `SIGKILL`.** Pro: memberi provider kesempatan immediate shutdown. Kontra: dua tahap dan spesifik provider; strategi provider sudah mendapat kesempatan di stop normal.

### K3. Force pada `unknown`

**Opsi A: tolak, arahkan ke recheck (dipilih).** Pro: tidak pernah menyasar PID yang tidak terbukti (AC-067). Kontra: jalan keluar tetap recheck atau doctor fitur 34.

**Opsi B: tandai stopped kalau lock bebas.** Pro: jalan keluar baru. Kontra: menambah jalur yang mengubah lifecycle tanpa bukti proses, di fitur yang seharusnya tentang sinyal.

### K4. Catatan akhir proses

**Opsi A: kolom `last_exit` (`clean`, `forced`, `crashed`) (dipilih).** Awalnya diusulkan `last_stop` (`clean`, `forced`); saat model data ditunjukkan, crash ditambahkan karena data dari crash juga tidak boleh dilabeli clean. Pro: fitur 49 membaca satu kolom. Kontra: satu migrasi.

**Opsi B: hanya journal operasi.** Pro: tanpa migrasi. Kontra: fitur 49 harus mencari histori, dan crash tidak punya operasi pengguna.

**Opsi C: tunda ke fitur 49.** Pro: paling sederhana. Kontra: fakta force dan crash sebelum fitur 49 hilang.

### K5. Restart

Diputuskan sebagai satu operasi daemon (topik fitur), lalu per dimensi:

- **Dari state mana**: `running` (siap atau belum) dan `recovering` (dipilih). Alternatif: juga `stopped` dan `failed` (mengaburkan Restart dan Start), atau hanya ready (memblokir restart justru saat instance tidak ready).
- **Separuh stop gagal**: operasi gagal, desired `stopped`, force ditawarkan (dipilih). Cross check menunjukkan desired harus sudah `stopped` sejak `stop_begin` (seperti stop spec 0010) dan baru `running` di journal start, karena rekonsiliasi spec 0010 tidak pernah menjalankan lagi instance `stopping`. Alternatif: desired tetap `running` (desired dan kenyataan berbeda, dan rekonsiliasi tidak memulihkannya), atau menunggu tanpa batas lalu start (bisa menggantung selamanya).
- **Lock masih dipegang**: tunggu sebentar lalu gagal `supervisor.data_in_use` (dipilih). Alternatif: gagal langsung (restart PostgreSQL jadi tidak stabil karena backend butuh sesaat untuk keluar), atau berbagi anggaran stop (stop yang lambat menyisakan hampir nol waktu).
- **Force di restart**: tidak (dipilih). Alternatif: `restart --force` menyembunyikan `SIGKILL` di aksi rutin.
- **Lease**: tanpa cek lease (dipilih), karena tujuan restart adalah instance berjalan lagi dan BR-015 tentang stop. Alternatif: cek seperti stop (bounce singkat butuh flag override).

### K6. Serialisasi

- **Restart saat instance sibuk**: `BusyPolicy::Queue` (dipilih), sama dengan start dan reload pengguna. Alternatif: `Reject` (dua klik cepat dari GUI dan CLI memberi error).
- **Stop selama restart**: `resource.busy`, batalkan restart lebih dulu (dipilih), satu aturan dengan stop selama start (spec 0010 AC-7). Alternatif: stop mengambil alih (jalur preemption baru yang belum ada di engine), atau stop antre (bisa berarti start penuh dulu sebelum stop yang kamu minta).

### K7. Bentuk force di wire

**Opsi A: method `instance.force_stop` dan operasi `force_stop_instance` (dipilih).** Pro: precheck sendiri, entri aktivitas sendiri, tidak bisa terpicu flag yang tersesat. Kontra: satu method dan satu kind lagi.

**Opsi B: flag `force` di `instance.stop`.** Pro: lebih sedikit method. Kontra: satu method dengan dua tingkat risiko dan precheck yang sangat berbeda.

### K8. Reload pada provider tanpa capability

**Opsi A: precheck menolak tanpa operasi (dipilih).** Pro: tidak ada rekaman reload gagal untuk hal yang tidak pernah mungkin. Kontra: tidak ada jejak di aktivitas bahwa kamu mencoba.

**Opsi B: operasi yang langsung gagal.** Pro: ada rekaman. Kontra: rekaman itu derau.

### K9. Sumber capability reload di app

**Opsi A: `capabilities` di `InstanceSummary` (dipilih).** Pro: menu Instance untuk baris terpilih tahu alasannya tanpa memuat detail; daemon membaca kolom `provider_revisions.capabilities` yang sudah ada, tanpa membaca paket. Kontra: summary sedikit lebih besar.

**Opsi B: hanya di `DetailProvider`.** Pro: lebih kecil. Kontra: menu bar tidak bisa menjelaskan Reload nonaktif sampai detail dimuat.

**Opsi C: daftar aksi yang dihitung daemon.** Pro: paling konsisten. Kontra: memindahkan gating lifecycle yang sudah dikerjakan app ke daemon, dan lebih mahal.

### K10. Force di UI dan CLI

- **UI**: banner "Still stopping." dengan Force Stop… plus item menu (dipilih). Alternatif: tombol utama berubah menjadi Force Stop (menaruh `SIGKILL` di tempat orang menekan Stop karena kebiasaan), atau hanya menu More (mudah terlewat).
- **CLI**: `stop --force` dengan konfirmasi atau `--yes` (dipilih). Alternatif: subcommand `force-stop` (satu kata kerja lagi di samping stop).
- **Teks risiko**: netral provider dengan catatan data, sama di GUI dan CLI (dipilih). Alternatif: teks dari manifest (paling tepat, tetapi menambah field ke spec 0009).

### K11. Reload aktif kapan dan alasan nonaktif

- **Aktif**: `running` dengan readiness apa pun (dipilih). Alternatif: hanya ready seperti mockup (memblokir reload justru saat config buruk membuat instance tidak ready).
- **Alasan**: unsupported dengan saran restart, not running, sibuk, dan daemon offline, semuanya dipilih.

## Rationale

Benang merahnya adalah menjaga invariant spec 0010 tetap sempit sambil memberi jalan keluar yang jujur. Force hanya dari `stopping`, hanya ke proses utama, hanya setelah pemeriksaan ulang, lewat satu port yang pemanggilnya dibatasi `check-deps`: setiap batasan itu menutup satu cara `SIGKILL` bisa menyasar proses yang salah atau menjadi kebiasaan. Menolak force pada `unknown` mengikuti AC-067 secara harfiah; jalan keluar untuk `unknown` tetap recheck dan doctor, yang memang dibangun untuk membuktikan identitas.

Restart di daemon, bukan di client, karena PRD 19.3 meminta "verify stopped, verify data lock" di antara stop dan start, dan hanya daemon yang memegang lock dan slot proses. Satu operasi dengan `BusyPolicy::Queue` membuat serialisasi gratis dari engine spec 0007, dan `CloseInterrupted` plus desired state membuat daemon yang mati di tengah restart pulih dengan benar. Pilihan "gagal dengan desired `stopped`" memilih kejujuran di atas kenyamanan: instance yang setengah berhenti tidak pernah menyala sendiri tanpa kamu minta.

Capability di `InstanceSummary` dipilih karena menu Instance spec 0069 menargetkan baris tabel, dan alasan nonaktif harus terlihat tanpa hover (AC-027) lewat subtitle menu. Kolom `provider_revisions.capabilities` sudah ada, jadi biayanya hanya mapping. Kolom `last_exit` dengan tiga nilai dipilih karena fitur 49 butuh jawaban "apakah data ini berhenti bersih" untuk crash juga, bukan hanya force; menyimpannya sekarang lebih murah daripada merekonstruksi dari histori nanti. Runner up di setiap keputusan tercatat di *Options considered*.

### Keputusan yang diambil architect sendiri

| Keputusan | Pilihan | Alasan singkat | Runner up |
|---|---|---|---|
| Batas tunggu exit setelah `SIGKILL` | `FORCE_EXIT_WAIT` 10 s, lalu `supervisor.force_timeout` | proses di uninterruptible wait bisa tertahan; operasi tidak boleh menggantung | tunggu tanpa batas |
| Batas tunggu lock restart | `RESTART_LOCK_WAIT` 10 s, poll 200 ms | backend PostgreSQL keluar dalam hitungan detik; 10 s cukup tanpa membuat restart terasa macet | berbagi anggaran stop |
| Jendela restart policy saat restart pengguna | direset seperti start pengguna | restart pengguna adalah awal baru, bukan percobaan crash | tetap dihitung |
| Interaksi Keychain restart | dari caller, seperti start pengguna | restart me resolve secret yang sama dengan start | selalu `Never` |
| Klasifikasi `last_exit` | tabel per jalur exit: `forced` dari niat force di journal, `crashed` untuk child yang terbunuh sinyal asing saat `stopping` dan setiap exit di luar `stopping`, selain itu `clean` | setiap jalur yang menulis `exited` punya nilai yang pasti | flag di memori (hilang saat daemon mati) |
| Lokasi kode restart | modul baru `restart_op.rs` | `restart.rs` sudah milik restart policy | memperluas `restart.rs` |
| Kode error baru | `instance_not_running`, `force_requires_stopping`, `force_timeout` | penolakan yang bisa dibedakan client tanpa membaca teks | memakai `instance_stopping` atau `reload_failed` |
| Cancel restart di separuh stop | `cancelled` dengan desired `stopped` | kamu membatalkan saat instance setengah berhenti; tidak menyalakannya lagi | desired kembali `running` |
| Gating di app | satu fungsi murni `InstanceLifecycleActions.resolve` | toolbar, menu, dan fixture memakai aturan yang sama | logika per view |

## Cross check

Dua pemeriksaan read only dijalankan pada 2026-11-01: Sonnet (G1 sampai G25), lalu Fable 5.1 atas spec ditambah usulan perbaikannya (penilaian F1 sampai F11 dan temuan baru H1 sampai H15). User menyetujui semua perbaikan yang direkomendasikan. Yang paling berat:

| Temuan | Masalah | Perbaikan di spec |
|---|---|---|
| G1, F1, H6 | desired `running` selama separuh stop restart bertentangan dengan rekonsiliasi spec 0010 | desired `stopped` di `stop_begin`, `running` di journal start |
| G6, F2, H2 sampai H4 | tanda force di memori hilang saat daemon mati, sehingga exit tercatat `clean` | niat force dibaca dari journal operasi (`force_intent`), tanpa kolom baru dan tanpa kenaikan revision; kolom `force_pending` dengan CAS ditolak karena butuh pembersihan saat recover dan menaikkan revision |
| G12, F3 | backend PostgreSQL bisa bertahan setelah `SIGKILL` ke postmaster dan memegang `instance.lock` | tetap proses utama saja; PID sisa di umpan balik dan teks risiko |
| F5 | sub kasus `recovering` dengan proses hidup tidak ada di state machine | dua sub kasus |
| F6, H14 | `ExitKind` belum pasti untuk setiap jalur `exited` | tabel per jalur |
| F8 | hasil operasi tidak membawa PID | PID dari `instance.get` setelah `--wait` |
| H1 | reset jendela restart policy hanya ada untuk start dari `failed` | `state.restart.clear()` di journal stop restart |
| H5 | builder summary tidak punya akses ke `provider_revisions` | capability dimuat bersama aggregate |
| H10 | crash sungguhan saat shutdown tercatat `clean` | child daemon dengan sinyal asing saat `stopping` dicatat `crashed` |

Klaim offline (G24) diperiksa: tahap verify start spec 0010 AC-18 tidak memanggil `CodeSigning::assess`, jadi AC-12 tetap berlaku.
