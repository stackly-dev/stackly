# 0030. Rekonsiliasi dan restart daemon terencana: alasan

[Kembali ke spec](index.md)

## Context

Fitur 10 (spec 0010) sudah membangun rekonsiliasi dasar: setelah daemon start, setiap instance diberi bukti identitas majemuk, diklasifikasikan `Proven`, `Gone`, atau `Unproven`, lalu diadopsi, ditandai `unknown`, atau dijalankan lagi sesuai tabel. Spec 0015 membuktikan di LaunchAgent sungguhan bahwa proses instance fake bertahan melewati `kickstart -k` dan `bootout`, dan `stackly daemon restart` sudah ada sebagai `daemon.shutdown { mode: restart }` yang keluar 75 tanpa menyentuh instance.

Celahnya ada di tiga tempat. Pertama, rekonsiliasi hanya melihat proses dan lock: ia tidak tahu kalau folder data, runtime, atau port sebuah instance sudah berubah saat daemon mati, padahal REQ-013 meminta rekonsiliasi proses, runtime, endpoint, storage, operasi, dan desired state. Folder data yang hilang adalah risiko paling mahal, karena start berikutnya bisa berakhir di cluster kosong yang tampak sehat. Kedua, PRD bagian 19.4 meminta restart terencana yang menghentikan instance terdampak dengan aman selama handover supervisor belum terbukti, memulihkan desired state sesudahnya, dan menampilkan jendela disruption; saat ini restart tidak punya konsep "terdampak", tidak punya catatan apa yang harus dipulihkan, dan GUI hanya melihat daemon hilang lalu kembali. Ketiga, bukti survival baru ada untuk provider fake, sedangkan pengguna menjalankan PostgreSQL dan NATS.

Batasan yang mengikat: hanya daemon yang menulis registry dan menjalankan proses; setiap mutasi adalah operasi `OperationEngine`; normal stop tidak pernah `SIGKILL`; proses yang identitasnya tidak terbukti tidak pernah dibunuh; boot baru tidak pernah menyalakan instance sebelum fitur 31; blocker baru di `InstanceBlocker` adalah protocol major. NFR-007 meminta inventory dalam 10 detik untuk 10 instance, dan aturan proyek menyatakan fixture bukan bukti runtime native. Fitur ini bertag GA, jadi ia juga menjalani `/check review` dan `/document`.

Kalau tidak diputuskan, fitur 26 (restart instance), 31 (login), dan 61 (update aplikasi) masing masing akan menebak arti "restart daemon yang aman" sendiri.

## Options considered

### Keputusan 1: apa yang terjadi pada instance saat restart terencana

**Opsi A: gerbang per provider (dipilih).** Instance dari provider yang survival nya terbukti native tetap jalan dan diadopsi; provider lain dihentikan aman lalu dinyalakan lagi.

- Pro: sesuai kalimat PRD 19.4 secara harfiah; PostgreSQL dan NATS tidak terganggu; provider baru yang belum diuji tidak diadopsi tanpa jaminan.
- Kontra: dua jalur kode (keep dan stop_restore) yang keduanya harus diuji; setiap provider baru butuh test native.

**Opsi B: selalu tetap jalan.** Semua instance diadopsi.

- Pro: paling sederhana, sudah hampir begitu sekarang.
- Kontra: provider yang perilaku forknya belum diuji (misalnya proses yang daemonize sendiri) bisa lolos dari pengawasan tanpa ada yang tahu, dan PRD meminta stop aman bila belum terbukti.

**Opsi C: selalu stop lalu restore.**

- Pro: tidak bergantung pada bukti handover.
- Kontra: setiap restart daemon (termasuk update aplikasi) memutus semua koneksi database; membuang hasil spec 0015 AC-16 yang sudah PASS.

### Keputusan 2: sumber bukti handover

**Opsi A: daftar bawaan di core (dipilih).** Konstanta `HANDOVER_PROVEN` berisi nama provider, mayor revision, dan ID evidence.

- Pro: klaim tidak bisa datang dari manifest pihak ketiga; jejak ke evidence record ada di kode; review kode menjadi gerbangnya.
- Kontra: menambah provider butuh perubahan kode dan rilis.

**Opsi B: field `supervision.handover` di `provider.yaml`.**

- Pro: provider bisa menyatakan dirinya sendiri tanpa rilis daemon.
- Kontra: pernyataan penulis manifest bukan bukti; butuh contract minor dan aturan trust yang baru ada di fitur 37.

**Opsi C: tanpa gerbang.** Sama dengan Keputusan 1 opsi B untuk provider bawaan.

- Pro: tidak ada kode tambahan.
- Kontra: masalah yang sama dipindah ke fitur 37.

### Keputusan 3: tempat rencana restore

**Opsi A: tabel registry (dipilih).** `daemon_restart_plan` dan `daemon_restart_plan_instance`, ditulis dalam satu transaksi sebelum stop pertama.

- Pro: satu transaksi dengan data instance, tahan crash, mengikuti aturan `commit_guarded` dan prune yang sudah ada; bisa menjelaskan restart sesudahnya.
- Kontra: migrasi dan repository baru.

**Opsi B: file JSON di data root.**

- Pro: lebih sedikit kode storage.
- Kontra: di luar transaksi registry; file setengah tertulis saat crash harus ditangani sendiri.

**Opsi C: tanpa catatan, pakai desired.** Instance `stopped` dengan desired `running` dinyalakan lagi.

- Pro: tanpa schema baru.
- Kontra: mengubah arti tabel rekonsiliasi untuk semua kasus, termasuk instance yang memang gagal; tidak bisa membedakan "dihentikan rencana" dari "gagal lalu berhenti".

### Keputusan 4: restart saat ada operasi aktif

**Opsi A: quiesce, tunggu 30 detik, lalu tolak (dipilih), dengan `force` sebagai jalan keluar.**

- Pro: tidak ada install atau create yang terputus diam diam; pengguna melihat operasi mana yang menghalangi.
- Kontra: aksi lain ditolak selama paling lama 30 detik.

**Opsi B: hanya grace spec 0007 (10 detik).**

- Pro: tanpa kode baru.
- Kontra: operasi tidak resumable dikompensasi atau terputus tanpa pilihan pengguna.

**Opsi C: tolak langsung.**

- Pro: paling sederhana.
- Kontra: start atau stop singkat yang kebetulan berjalan membuat restart gagal tanpa alasan kuat.

### Keputusan 5: data, runtime, dan port yang berubah

Data hilang: `failed` dengan `data_missing` tanpa init (dipilih) dibanding `unknown` dengan recheck; `unknown` berarti "identitas proses tidak terbukti", dan memakainya untuk data mengaburkan arti state itu. Runtime hilang: adopsi tetap, start ditahan lewat failure code (dipilih) dibanding blocker `runtimeMissing`; blocker baru memaksa protocol major untuk informasi yang cukup sebagai failure code. Port dipakai: `failed` dengan `port_in_use` tanpa port pengganti (dipilih) dibanding recheck berkala; start otomatis saat port bebas bisa menyalakan database di saat yang tidak diharapkan pengguna.

### Keputusan 6: permukaan pengguna

CLI menunggu sampai restore selesai (dipilih) dibanding hanya sampai daemon baru, karena janji yang diminta adalah desired state pulih. Konfirmasi di TTY dengan `--yes` untuk script (dipilih) dibanding tanpa konfirmasi, karena restart bisa memutus koneksi. GUI menaruh tombol di Settings General bagian Daemon sesuai mockup UI (dipilih) dibanding Diagnostics sementara atau CLI saja; bagian itu memang milik fitur 30 di `ui-macos.md`. Banner plus tanda per instance (dipilih) dibanding toast saja, karena toast mockup tidak menyebut instance yang dihentikan.

## Rationale

Inti keputusan ini adalah membedakan dua hal yang selama ini tercampur: daemon yang berhenti (bidang kendali) dan instance yang berhenti (bidang data). Spec 0010 dan 0015 sudah membuat keduanya terpisah secara teknis; spec ini membuat pemisahan itu bisa dijanjikan kepada pengguna, provider demi provider, dengan bukti. Itulah sebabnya gerbangnya adalah evidence native dan bukan klaim manifest: kekuatan janji sama dengan kekuatan buktinya.

Rencana di registry dipilih karena restart terencana adalah satu satunya alur di sistem ini yang dimulai oleh satu proses daemon dan diselesaikan oleh proses daemon lain. Pengetahuan yang harus menyeberang (instance mana yang harus dinyalakan lagi, sampai kapan, di boot mana) butuh tempat yang transaksional dan tahan crash, dan registry sudah menjadi tempat itu untuk operasi. Desired sengaja tidak disentuh, supaya rencana yang gagal atau kedaluwarsa tidak pernah mengubah keinginan pengguna; yang berubah hanya lifecycle, yang memang menggambarkan kenyataan.

Pemeriksaan data, runtime, dan port diletakkan di jalur yang sudah ada (rekonsiliasi dan startup transaction) dan dilaporkan lewat failure code dan finding, bukan blocker baru, supaya tidak memaksa protocol major dan supaya `instance start`, respawn, dan restore berbagi satu aturan. Ini mengikuti pola spec 0025: satu fungsi pemeriksaan yang dipakai setiap jalur yang menyalakan instance.

## Keputusan kecil

| Keputusan | Pilihan | Alasan | Runner up |
| --- | --- | --- | --- |
| Bentuk restart terencana di engine | operasi `daemon_restart` dengan lock key setiap instance `stop_restore` | semua mutasi adalah operasi (AGENTS.md); lock key menserialisasi dengan start dan stop instance yang sama | logika di dispatch `daemon.shutdown` tanpa operasi (tidak terlihat di `operation.list`) |
| Stop tanpa mengubah desired | `StopIntent::PlannedRestart` di jalur stop yang ada | strategi stop provider dan timeout yang sama; hanya penulisan desired yang dilewati | operasi `stop_instance` lalu menulis desired kembali (dua commit, jendela desired salah) |
| Pemicu restore | `PendingKind::Restore` yang membaca baris rencana, dijalankan sebagai operasi sistem `start_instance` dengan pemicu `planned_restart` bersama daftar "jalankan lagi" spec 0010 | tabel rekonsiliasi tidak pernah menyalakan `stopped`; jenis pending baru memakai ulang konkurensi, interaksi `Never`, dan semua precheck start | jalur respawn actor (budget restart policy tidak cocok untuk restore) |
| Stop di dalam operasi `daemon_restart` | panggilan supervisor langsung di bawah lock key yang sudah dipegang | operasi anak akan menunggu lock key induknya sendiri (deadlock) | melepas lock key sebelum stop (membuka celah start dari client) |
| Start ulang saat abort (2026-11-02, dari verify milestone 3) | `PendingKind::Restore` yang diantre lalu dijalankan sebagai operasi sistem `start_instance` tepat setelah operasi `daemon_restart` final | langkah jurnal sebuah operasi tidak bisa diulang per instance, dan setelah operasi final lock key nya sudah lepas, jadi operasi sistem tidak deadlock; jalurnya sama dengan restore daemon baru | start langsung di dalam operasi (pilihan awal; satu langkah jurnal untuk banyak start, hasil per instance tidak tercatat) |
| Urutan quiesce dan operasi (2026-11-02, dari verify milestone 3) | quiesce dan penantian lebih dulu, operasi `daemon_restart` dimulai sesudahnya | `restart_busy` adalah penolakan sebelum ada pekerjaan, jadi tidak perlu baris operasi `failed` di `operation.list` | operasi dulu lalu quiesce (pilihan awal) |
| `closeReason` saat abort (2026-11-02) | selalu `stop_timeout`, untuk timeout maupun stop gagal lain | timeout adalah penyebab yang realistis; nilai lain butuh enum baru di kontrak untuk kasus langka (instance berubah `unknown` di antara preview dan stop), dan error persisnya sudah ada di operasi `failed` | nilai baru `stop_failed` (contract minor) |
| Isi `DaemonInfo.restart` (2026-11-02, keputusan lead: spec mengikuti kode) | rencana terakhir yang dilihat daemon run ini, di memori, terbuka atau terminal kurang dari 10 menit | tanpa baca registry di setiap `daemon.info` dan handshake; daemon baru selalu melihat rencana yang ia ambil alih, jadi alur normal tetap lengkap | membaca rencana terminal terakhir dari registry saat start (ringkasan bertahan melewati restart kedua) |
| Sumber `lastRestart` di `daemon status` (2026-11-02, gap kecil, pilihan architect) | salinan `DaemonInfo.restart` | tanpa method atau field kontrak baru; CLI dan GUI melihat hal yang sama | method baru untuk membaca riwayat rencana dari registry (milik Diagnostics nanti) |
| Key restart di CLI dan GUI (2026-11-02, gap kecil, pilihan architect) | UUID v7 baru untuk setiap konfirmasi, tanpa flag `--idempotency-key` | key wajib sejak `daemon.shutdown { restart }` memulai operasi; konfirmasi ulang membawa digest lain sehingga key lama tidak boleh dipakai; request yang hilang balasannya dan diulang dengan key baru tetap aman karena quiesce atau rencana terbuka menolaknya `daemon.restart_pending`, dan setelah handover `daemon.shutting_down` | flag `--idempotency-key` seperti perintah async spec 0008 |
| Timeout GUI untuk restart (2026-11-02, gap kecil, pilihan architect) | `CallOptions(timeout:)` 190 detik, anggaran interaktif `SecretsClient.interactiveTimeout` yang sudah ada | reply bisa datang sampai sekitar 70 detik; default 10 detik akan memutus restart yang sah, dan CLI juga memakai anggaran interaktif nya | konstanta GUI baru 80 detik |
| Keluaran rencana `aborted` (2026-11-02, gap kecil, pilihan architect) | dua daftar dari `stoppedAt` ("did not stop, still running" dan "stopped, starting again"), `durations` null kecuali `finished` | semua outcome `skipped` saat abort, jadi `stoppedAt` satu satunya pembeda yang ada di kontrak; durasi tanpa handover tidak bermakna | field baru di `RestartPlanView` untuk penyebab dan hasil start ulang (contract minor) |
| Kecocokan provider di `HANDOVER_PROVEN` (2026-11-02, dari verify milestone 3) | ID provider penuh (`official/postgresql`, `testing/fake`) | ID penuh yang ada di `ProviderRevisionKey`; nama pendek bisa bertabrakan dengan provider pihak ketiga yang memakai nama sama | nama pendek (`postgresql`) |
| Kode data dan runtime hilang | pakai ulang `supervisor.data_unavailable`, `executable_invalid`, `runtime_unavailable`, `endpoint_in_use` | satu penyebab satu kode; tanpa kode baru yang menyaingi kode lama | kode baru `instance.data_missing` |
| Event baru | protokol mayor | `EventKind` tertutup, client lama gagal decode kind yang tidak dikenal | menyaring event hanya untuk subscriber yang meminta (aturan langganan baru) |
| Digest rencana | sha256 dari `instanceId:decision:reason` terurut | stabil, murah, tidak bergantung pada urutan nama | `statusSeq` per instance (berubah karena probe, membuat digest terlalu sering berbeda) |
| Batas tunggu quiesce | 30 detik | cukup untuk start, stop, create kecil; install panjang lebih baik ditolak dengan alasan | 10 detik (grace spec 0007) |
| Kedaluwarsa rencana | hanya boot baru (`boot_changed`), tanpa batas waktu | spec 0031 memperlakukan `stopped` dengan desired `running` sebagai keadaan istirahat yang sah dan antrean start nya menyalakannya di sesi yang sama; batas waktu akan bertentangan begitu fitur 31 jalan (diputuskan 2026-11-02 setelah cross check) | 10 menit sejak rencana dibuat (pilihan awal) |
| Retensi | 20 rencana terminal, prune harian | cukup untuk Diagnostics dan `daemon status`; mengikuti task prune spec 0006 | hanya yang terakhir |
| Exit restore gagal | 7 (`failed`, `cli.restart_restore_failed`) | peta exit spec 0008: 1 khusus bug Stackly, 7 untuk pekerjaan yang dijalankan lalu gagal; maksud pilihan kamu ("bukan 0, script harus tahu") tetap | exit 0 dengan warning |
| Konkurensi stop rencana | `RECONCILE_START_CONCURRENCY` (4) | sama dengan restore; tanpa konstanta baru | serial (jendela lebih panjang) |
| Restart di recovery mode | diizinkan, semua `keep` | supervisor tidak jalan sehingga stop aman tidak mungkin; restart sering jalan keluar dari recovery | ditolak |
| Bukti AC-1 di GUI | test daemon native, test integrasi Swift fake, record manual `docs/manual.md` | XCUITest butuh Automation Mode yang hanya kamu yang bisa nyalakan | XCUITest wajib |
| Waktu inventory AC-17 | dari handshake pertama ke daemon baru sampai `reconciling finished` plus `instance.list` lengkap | itulah yang dilihat client; tidak bergantung pada log internal | dari exit daemon lama (mengukur launchd, bukan rekonsiliasi) |
| Provider uji untuk `stop_restore` | fake dengan `SupervisorConfig.handover_proven` kosong di test | tanpa package fixture baru | package fixture terpisah |
| Folder data kosong (2026-11-02, temuan run native NATS di milestone 3, keputusan lead atas delegasi user) | hilang hanya untuk paket yang mendeklarasikan `compatibility.dataMarker`; folder yang tidak ada dan marker `Missing` tetap hilang untuk semua paket | NATS tidak punya init yang menulis data, jadi foldernya sah kosong sampai server pertama jalan; paket dengan marker selalu punya isi setelah init, jadi folder kosong di sana memang berarti data hilang | folder kosong selalu hilang (menolak NATS yang baru dibuat) |

## Pembaruan dari verify milestone 2 dan 3 (2026-11-02)

Temuan `/check verify` milestone 2 dan 3, diteruskan lead. Kode di `main` sudah berperilaku begini; spec menyesuaikan, dan perubahan teks AC menunggu persetujuan kamu.

| No | Temuan | Perubahan spec |
| --- | --- | --- |
| 1 | PID milik executable lain: `dataInUse` kalau lock dipegang pihak lain, `Gone` (`stopped`) kalau lock bebas, sesuai tabel `classify` spec 0010; `identityUnproven` hanya kalau keenam pemeriksaan lulus tanpa lock | AC-3, test `pid_reuse.rs` |
| 2 | abort menyalakan lagi instance yang sudah berhenti lewat operasi sistem `start_instance` sesudah operasi `daemon_restart` final; setiap stop gagal memakai `stop_timeout` | AC-11, *Alur* langkah 7, outcome saat `aborted`, Keputusan kecil |
| 3 | `DaemonInfo.restart` hanya rencana yang dilihat daemon run ini | tabel API, Value sourcing, GUI, AC-15 (`daemon status`), Consequences |
| 4 | operasi `daemon_restart` dimulai sesudah quiesce dan penantian; `restart_busy` tanpa baris operasi | AC-10, *Alur* langkah 2 sampai 4, `BusyPolicy::Queue` (nilai `Wait` tidak ada), `PREPARE_GRACE` |
| 5 | `HANDOVER_PROVEN` memakai ID provider penuh, fake line `0.3` dan `0.4` | *Gerbang handover*, Value sourcing, Build plan langkah 10 |
| 6 | `daemon.shutdown { restart }` wajib `idempotencyKey` (di envelope request, bukan di `ShutdownParams`) | *Alur* langkah 1, tabel API, paragraf CLI, GUI, Build plan langkah 11 dan 12 |
| 7 | `persist_plan` yang gagal setelah abort melewatkan `queue_restore` | *Edge cases* dan test `/test`, tanpa perubahan perilaku |

### Cross check pembaruan ini

Cross check 2026-11-02 oleh model lain (Sonnet, hanya membaca) atas pembaruan di atas: 10 temuan (0 high, 5 medium, 3 low medium, 2 low). Setiap fakta kode diperiksa ulang sebelum diterapkan.

| No | Temuan | Disposisi |
| --- | --- | --- |
| 1 | abort yang gagal disimpan: error operasi adalah error registry, event terakhir tetap `stopping`, CLI menunggu sampai exit 8, banner GUI tidak hilang | diterapkan: *Edge cases* dan rujukan di *Alur* langkah 7; AC-11 tidak diberi pengecualian (risiko untuk `/test` dan review) |
| 2 | reply restart bisa sampai sekitar 70 detik; timeout GUI tidak bersumber | diterapkan: *Alur* langkah 3, paragraf CLI, timeout GUI 190 detik (Keputusan kecil) |
| 3 | `failure_code` untuk `unknown` selalu `identity_unproven`; stop, force stop, start gagal `supervisor.identity_unproven` | diterapkan: outcome di *State rencana*, AC-3 |
| 4 | request ulang dengan key yang sama mendapat `ShutdownAccepted` juga untuk operasi yang `failed` (`restart.rs`, `Replay` tanpa state) | tidak diubah di spec: spec tetap meminta reply pertama (spec 0007), jadi ini celah kode; dilaporkan ke lead. CLI dan GUI memakai key baru untuk setiap konfirmasi |
| 5 | restart di recovery mode kini gagal `registry.unavailable` atau `daemon.starting`, padahal Keputusan kecil "Restart di recovery mode" memilih diizinkan | tidak diubah di spec: keputusan lama tetap sampai kamu memilih; dilaporkan ke lead dengan rekomendasi jatuh ke restart spec 0015 tanpa rencana saat supervisor tidak jalan |
| 6 | pemicu `planned_restart` tidak tercatat; input jurnal restore selalu `{"trigger":"reconcile"}` (`pending.rs`) | tidak diubah di spec (AC-9, langkah 10): celah kode, dilaporkan ke lead |
| 7 | bentuk `restart_busy`: `items` dengan `operationId` dan `kind` plus `details.omitted`; bisa kosong saat timeout lock | diterapkan: AC-10, *Alur* langkah 3 |
| 8 | keluaran `aborted` di CLI dan GUI, `durations`, penyebab abort | diterapkan: paragraf keluaran `aborted`, Value sourcing, *Alur* langkah 7 |
| 9 | teks basi: `NoOpenRestartPlan`, diagram "stop timeout", invariant `commit_guarded`, waktu entri handover, `PREPARE_GRACE` | diterapkan |
| 10 | `provider` di preview adalah nama pendek; nama check `executable_matches`; varian rencana AC-3 belum ada | diterapkan: Value sourcing, AC-3, baris test (untuk `/test`) |

## Cross check

Cross check 2026-11-02 oleh model lain (Sonnet, hanya membaca), 25 temuan (9 high, 12 medium, 4 low). Fakta kode yang dipakai diperiksa ulang sebelum diterapkan; kamu memilih "terapkan saran" untuk semuanya.

| No | Temuan | Disposisi |
| --- | --- | --- |
| 1 | nomor 0026 sudah dipakai spec fitur 26; proyek memakai nomor spec sama dengan nomor fitur | diterapkan: spec pindah ke 0030 |
| 2 | satu call hanya punya satu reply, jadi abort tidak bisa menjadi frame error kedua | diterapkan: abort lewat event rencana, CLI `cli.restart_aborted` |
| 3 | call control CLI 10 detik, quiesce bisa 30 detik | diterapkan: timeout interaktif untuk `mode: restart` |
| 4 | operasi anak menunggu lock key induk | diterapkan: stop dan start langsung di dalam operasi |
| 5 | `port_in_use` tidak ada; start yang gagal meninggalkan `stopped` | diterapkan: `endpoint_in_use`, lifecycle menurut jalur start |
| 6 | data hilang sudah punya `data_unavailable`, marker hilang memberi `data_incompatible` | diterapkan: pakai ulang `data_unavailable`, marker `Missing` pindah ke sana |
| 7 | runtime hilang sudah punya `executable_invalid` dan `runtime_unavailable`; folder run dibuat ulang | diterapkan: kode lama, pemeriksaan pada folder install runtime |
| 8 | tidak ada jalur yang menyalakan `stopped` di rekonsiliasi | diterapkan: `PendingKind::Restore` |
| 9 | baris `stopping` dengan desired `running` akan macet | diterapkan: `Then::PlannedStop` |
| 10 | rencana bisa berubah selama quiesce | diterapkan: digest dihitung ulang di transaksi |
| 11 | titik quiesce dan pengecualian operasi sendiri belum disebut | diterapkan: flag di `begin`, hanya `Origin::Request` ditolak |
| 12 | arti resumable salah; install tidak menghalangi | diterapkan: definisi penghalang dan Consequences diperbaiki |
| 13 | `start_operation` butuh idempotency key; `Origin` tanpa nilai user | diterapkan: key untuk `mode: restart`, `Origin::Request`, `OperationKind::DaemonRestart` |
| 14 | kemajuan sesudah exit tidak punya operasi | diterapkan: rencana yang membawa kemajuan |
| 15 | `recovering` tanpa proses, `starting` dan `initializing` tidak disebut | diterapkan: `respawn_pending`, `starting` mengikuti provider, `initializing` di luar rencana |
| 16 | `failure_code` untuk `unknown` tidak bersumber | diterapkan: kode blocker, warning CLI bukan exit 7 |
| 17 | FK `instance_id` membuat `skipped` mustahil | diterapkan: tanpa FK, kolom `name` |
| 18 | retensi ada di `RetentionPolicy` | diterapkan |
| 19 | `Copy` dan `EventKind` tertutup | diterapkan: tanpa `Copy`, protokol mayor |
| 20 | "mayor revision" tidak terdefinisi | diterapkan: line semver, `0.3` untuk fake |
| 21 | `server::start` tidak menghasilkan exit 75 | diterapkan: `RestartHarness` |
| 22 | `commit_guarded` bukan repository umum | diterapkan: method `Registry` baru lewat guard |
| 23 | `stop` saat rencana terbuka | diterapkan: `daemon.restart_pending` |
| 24 | titik nol waktu AC-17 tidak konsisten | diterapkan: handshake pertama, `SpawnedDaemon` |
| 25 | `stop.rs` selalu menulis desired | diterapkan: `StopIntent` di `StopRequest`, tiga titik penulisan |

Selain itu cross check menemukan spec 0031 (fitur 31, `Proposed`) yang ditulis paralel: kamu memilih kedaluwarsa hanya di boot baru dan spec ini duluan untuk bentuk reply `daemon.shutdown` (lihat Follow-up di index).
