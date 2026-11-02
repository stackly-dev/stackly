# Verify: Metrics dan health · spec 0028 · updated 2026-11-02

_Langkah diturunkan dari acceptance criteria spec 0028. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27._

## Milestone 2: Sampler, contract, daemon, and CLI

_Langkah 4 (sampler di core, atas fake), 5 (protokol 2.2, daemon), dan 6 (CLI) sudah dibangun. Tab Metrics di app (langkah 7), latensi probe dan wake (langkah 8), dan evidence native (langkah 9) belum._

### UI / manual

- [x] Mulai daemon dengan root uji (`STACKLY_ROOT=$(mktemp -d) cargo run -q -p stacklyd`), buat dan jalankan satu instance fake lewat `cargo run -q -p stacklyd --example skeleton_seed -- --root <R>` (perintah `stackly instance create` yang dicetaknya), lalu `stackly instance metrics <nama>` → baris Process, Uptime, Restarts "0 since created", Readiness, Health, Data size, dan dua blok "main process (pid ...)" serta "process tree (...)" dengan daftar proses; hitungan pertama menampilkan `unavailable (warming_up)` untuk CPU dan Disk I/O, bukan 0 → AC-1, AC-3, AC-12
- [x] `stackly instance metrics <nama> --watch` → satu baris per sampel baru kira kira tiap 5 detik; stop instance lalu jalankan lagi, muncul "not running (stopped)" dan sesudah running lagi "history reset"; Ctrl+C keluar 0 → AC-12
- [x] `stackly instance metrics <nama> --json` → satu amplop dengan `result` sama dengan balasan `instance.get_metrics` → AC-11, AC-12
- [x] Pembuktian di Mac asli (PostgreSQL 17 dan NATS native, pohon proses nyata) ikut langkah 9.

### Commands

- [x] `cargo test -p stackly-core --test metrics_sampler` → 20 test lulus: sampel pertama punya memori dan CPU serta Disk I/O `warming_up`, sampel kedua punya CPU dan Disk I/O (50.0 persen untuk 2,5 detik CPU dalam 5 detik, 2000 byte per detik untuk 10000 byte tulis), pohon menjumlah proses utama dan turunan dengan uid yang sama dan melewati proses cabang lain, turunan milik user lain beserta anaknya, urutan baris sebagai pohon dengan saudara urut PID, CPU pohon lebih dari 100, siklus parent berakhir, proses utama ditolak (`EPERM`) menjadi `not_permitted` untuk `main` dan `tree` dengan `measuredCount` 0, anak ditolak tinggal mengurangi `measuredCount`, proses utama hilang di tengah sampel atau dari daftar PID menjadi `process_exited`, anak yang keluar hilang dari baris, PID anak yang dipakai ulang mulai `warming_up` dan hanya di-`inspect` ulang sekali, `inspect` sekali per proses baru dan bukan setiap tick, proses utama baru membuka history baru dengan `seq` 1, history dibuang saat instance tidak lagi running (dan `list_pids` tidak dipanggil kalau tidak ada yang running), sampel ke 61 membuang yang tertua, baris dipotong di 200 sementara jumlah pohon tetap 206 proses, dua instance punya history masing masing, `list_pids` `not_supported` menjadi `not_supported`, daftar PID yang gagal dibaca tidak mencatat sampel, task sampler mengambil sampel tiap interval tanpa client dan berhenti setelah `stop`, dan konfigurasi bisa diperpendek → AC-1, AC-3, AC-5
- [x] `cargo test -p stackly-domain --lib metrics` → unit test tipe dan fungsi murni yang dipakai sampler (selisih CPU dan Disk I/O, `measure`, jumlah pohon, urutan pohon, ring 60, aturan cursor) tetap lulus → AC-1, AC-2, AC-3, AC-5
- [x] `cargo clippy -p stackly-core --all-targets --locked -- -D warnings` → tanpa peringatan; setiap item publik di `stackly_core::metrics` punya doc comment, tidak ada `unwrap()` di luar test → AC-1
- [x] `cargo xtask check-deps` → lulus; `stackly_core::metrics` tidak memanggil API spawn, tidak menyebut `SIGKILL`, dan tidak menulis registry; sampler dan pemindaian terjadwal di daemon tidak mengirim probe atau sinyal → AC-1
- [x] `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → exit 0 (modul `metrics` tidak memakai API macOS) → AC-1
- [x] `cargo xtask codegen --check` → "up to date"; `ProtocolVersion::CURRENT` 2.2 dan `protocols/contract-versions.json` mencatat digestnya → AC-11
- [x] `cargo test -p stackly-protocol` → termasuk `metrics_fixtures` (6 test: live, not running, reset, nilai tidak dikenal, request dan penolakan kursor, balasan terbesar 60 sampel dan 200 baris di bawah 1 MiB) dan `ipc_fixtures` yang membaca `frame-response-instance-get-metrics-*.json` dan `request-instance-get-metrics*.json` → AC-3, AC-11
- [x] `cargo test -p stacklyd --test supervisor metrics` → 7 test lulus lewat socket pada proses fake server asli: sampel tanpa client yang meminta (sampel pertama `warming_up`, memori terisi, pohon dan daftar proses), aturan kursor (`afterSeq` baru, `historyId` lain mereset, `afterSeq` di luar tanpa reset), `not_running` tanpa history dan tanpa pemindaian oleh `get_metrics`, `resource.not_found`, history dibuang saat instance berhenti, pemindaian terjadwal mengisi `last_scan` dan pemindaian on demand ikut mengisinya, dan `restartTotal` naik satu setelah proses dibunuh dan di-respawn → AC-1, AC-3, AC-5, AC-6, AC-10, AC-11
- [x] `cargo test -p stackly-cli --test instance` → termasuk `metrics` (5 test: dua cakupan dengan label dan golden `instance_metrics.txt`, `unavailable (reason)`, `--json` sama dengan balasan, "not running (stopped)" keluar 0, nama tidak dikenal keluar 10 `cli.instance_not_found`, dan `--watch` dengan kursor, "history reset", Ctrl+C keluar 0) dan golden `instance_*` yang kini memuat `restartTotal` dan "since created"; `a_bad_draft_gets_the_same_error_through_the_client_and_through_the_cli` kadang gagal saat mesin sibuk dan lulus kalau dijalankan sendiri → AC-6, AC-12
- [x] `swift test --package-path clients/swift` → lulus (hitungan method 44 dan protokol 2.2, semua fixture indeks) → AC-3, AC-11
- [x] `swift build --package-path apps/macos/Packages/StacklyUI --build-tests` → hijau setelah init `InstanceSummary`, `InstanceReadiness`, dan `InstanceHealth` mendapat `restartTotal` dan `latencyMs` netral; tampilan Restarts dan latensi menunggu langkah 7 → AC-6, AC-7

### Acceptance-criteria coverage

- AC-1 · bagian sampler: satu sampel per tick untuk setiap instance running tanpa client, paling banyak 60 sampel, `seq` naik satu mulai 1, history baru untuk proses utama baru dan dibuang saat tidak running, `historyId` dari PID dan waktu start; `sampledAt` memakai `Clock::now`. Penghubung ke status supervisor dan `stacklyd::server::start` menunggu langkah 5
- AC-2 · nilai CPU dan Disk I/O dihitung dari selisih dan `tokio::time::Instant` di sekitar pembacaan (test dengan waktu tokio dijeda); pembuktian di proses asli menunggu langkah 9
- AC-3 · bagian sampler: `warming_up`, `not_permitted`, `process_exited`, dan `not_supported` muncul sebagai `unavailable` dengan reason yang benar, tidak pernah 0; bukti di DTO, JSON CLI, dan model Swift menunggu langkah 5 sampai 7; `not_scanned` milik langkah 5
- AC-5 · pohon dari satu daftar PID dan parent dari `inspect`, pengaman siklus, filter uid daemon dari `UserIdentity` (dibaca sekali), `inspect` sekali per proses, baris dipotong di 200 dengan `processesTruncated`
- AC-4, AC-6 · bagian restart: `restartTotal`, label cakupan di CLI dan layar menunggu langkah 5 sampai 7 (label data `processCount` dan `measuredCount` sudah dihasilkan sampler)
- AC-6 · `restartTotal` di `InstanceSummary`, `instance.get_metrics`, dan baris Restarts `stackly instance inspect` ("{n} since created"), naik setelah respawn pasca crash (test daemon); kartu Restarts di app menunggu langkah 7
- AC-10 · slot `last_scan` diisi oleh setiap pemindaian (on demand dan terjadwal, `DATA_USAGE_SCHEDULE_INTERVAL` 15 menit, dipendekkan lewat `DataUsageConfig` di test); `get_metrics` hanya membaca slot
- AC-11 · method, kursor, `historyReset`, `not_running`, `instance.not_found`, ukuran balasan terbesar, fixture Rust dan Swift, dan `ProtocolVersion::CURRENT` 2.2; jenis metrik yang tidak dikenal hanya dites di Rust (typeshare Swift tidak memetakan `type` asing ke `unrecognized`, nilai enum asing ya)
- AC-12 · CLI `stackly instance metrics` dengan `--json` dan `--watch`
- AC-7 · `latencyMs` ada di DTO tetapi selalu null sampai langkah 8 mengisinya dari probe
- AC-8, AC-9, AC-13 sampai AC-15 · milik langkah lain (7, 8, 9)

## Milestone 4: Probe latency and wake

_Langkah 8 sudah dibangun: latensi di `ProbeOutcome` dan observasi domain, `latencyMs` di DTO daemon, `WakeWatch` dengan masa tenggang, perpanjangan deadline start, dan penanda `// provisional: F28` sudah dihapus. Tampilan latensi di app (langkah 7) dan evidence native di Mac yang benar benar sleep (langkah 9) belum._

### UI / manual

- [ ] Di Mac asli: jalankan instance PostgreSQL 17 sampai `ready`, tutup layar sampai Mac sleep beberapa menit, lalu bangunkan → readiness dan health sebentar `unknown` reason `waking` (satu `instance.health_changed`), lalu kembali `ready` dan `healthy` dengan `probe_ok` dalam beberapa detik, `restartTotal` tidak naik, port tidak berubah. Dicatat sebagai evidence manual `native`, `BLOCKED` kalau tidak dijalankan (langkah 9) → AC-9, AC-15
- [x] Dengan daemon jalan dan satu instance fake `ready`: `stackly instance get <nama> --json` → `readiness.latencyMs` dan `health.latencyMs` terisi bilangan bulat (bukan null) dan tidak lebih dari timeout probe dalam milidetik → AC-7

### Commands

- [x] `cargo test -p stackly-core --test supervisor_start wake:: -- --test-threads=1` → 4 test lulus pada fake clock yang melompat satu jam sementara jam tokio tidak: wake pada instance `running` membuat readiness dan health `unknown` reason `waking` detail `rechecking after sleep` dengan `observed_at` waktu wake dan tanpa latensi, probe langsung jalan (pause rutin 60 detik, kembali `probe_ok` dalam hitungan detik), tidak ada sinyal, spawn kedua, atau perubahan port; kegagalan dalam grace tidak mengubah observasi dan tidak naik hitungannya (readiness dan health tetap `waking` sampai grace habis, lalu readiness `not_ready` reason `connect_failed` dengan latensi); sukses pertama mengakhiri grace sehingga kegagalan berikutnya langsung dihitung; wake saat start dengan `readinessSeconds: 1` menggeser deadline sehingga start tetap `succeeded` dan tidak ada observasi `waking` → AC-9
- [x] `cargo test -p stackly-core --test supervisor_start status:: -- --test-threads=1` → termasuk `readiness_and_health_carry_the_latency_of_the_attempt_that_made_them` (sukses dan gagal membawa latensi, perubahan latensi saja tidak menjadi event) dan `an_observation_that_goes_stale_...` yang kini mematikan deteksi wake lewat `wake_gap_threshold` → AC-7, AC-8
- [x] `cargo test -p stackly-core --lib wake` → unit test `slept` (selisih sama, tepat di ambang, lebih besar, jam dinding mundur diabaikan), `Track` (kegagalan dalam grace tidak dihitung, setelah grace dihitung, sukses mengakhiri grace), observasi `waking`, dan nilai konstanta (2 detik, 5 detik, 30 detik) → AC-9
- [x] `cargo test -p stackly-core --lib supervisor::status supervisor::probe observation` dan `cargo test -p stackly-domain --lib observation` → `differs_visibly` mengabaikan latensi, `timed` memotong latensi pada timeout probe, `with_latency` bertahan lewat `effective` sampai stale → AC-7, AC-8
- [x] `cargo test -p stackly-core` (semua, `--test-threads=1`) → hijau
- [x] `cargo test -p stacklyd --lib latency_tests` → `latencyMs` dibulatkan ke atas ke milidetik penuh, null tanpa probe, menjenuh di `u32::MAX` → AC-7
- [x] `cargo test -p stacklyd --test supervisor -- --test-threads=1` → 104 test lulus (261 detik) → AC-9
- [x] `grep -rn "provisional: F28" crates apps clients` → tanpa hasil → AC-8
- [x] `cargo clippy -p stackly-domain -p stackly-core -p stacklyd --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → bersih

### Acceptance-criteria coverage

- AC-7 · latensi satu percobaan probe (sukses maupun gagal, dibulatkan ke atas, dipotong pada timeout probe) di `ProbeOutcome`, observasi domain, dan `latencyMs` DTO; null untuk observasi tanpa probe (`waking`, `not_running`, `daemon_restarted`); tidak ikut `differs_visibly`. Baris Readiness di Summary dan header Metrics di app menunggu langkah 7
- AC-8 · `READINESS_REFRESH_INTERVAL` 10 detik dan `READINESS_MAX_AGE` 30 detik tetap, penanda provisional dihapus; sampler tidak mengirim probe (milestone 2)
- AC-9 · `WakeWatch` (`crates/core/src/supervisor/wake.rs`), grace terpisah untuk readiness dan health, perpanjangan deadline start; test pakai fake clock tanpa sleep sungguhan. Evidence Mac yang benar benar sleep menunggu langkah 9
- AC-13 sampai AC-15 · milik langkah lain (7, 9)

## Milestone 3: GUI

_Langkah 7 sudah dibangun: `getMetrics` di `StacklyClient`, tab Metrics (kartu, grafik Swift Charts, daftar proses, teks unavailable, state not running), baris Restarts dan latensi di Summary dan inspector, dan snapshot spec 0068. Cek di app asli dan di Mac yang benar benar sleep tetap milik langkah 9._

### UI / manual

- [ ] Mulai daemon dengan root uji dan satu instance fake yang running (`skeleton_seed`), buka app, masuk ke instance, pilih tab Metrics → lima kartu (CPU, Memory, Disk, Disk I/O, Restarts), grafik "CPU · last 60 samples" dengan teks "sampled every 5 s", daftar Processes, dan header health dengan jam dan latensi; sampel pertama muncul dalam 5 detik dengan CPU "Measuring…" lalu terisi → AC-13
- [ ] Pindah antara Main Process dan Process Tree → subjudul kartu berubah ("Main process" dan "Process tree · {n} processes"), daftar Processes tetap menampilkan seluruh pohon → AC-4, AC-13
- [ ] Pindah ke tab Summary lalu kembali → polling berhenti selama tab tidak terlihat (tidak ada `instance.get_metrics` di log daemon level debug) dan history lama langsung tampil → AC-13
- [ ] Stop instance dari tombol toolbar saat tab Metrics terbuka → "No Live Metrics" dengan tombol Start yang aktif; Start kembali memulai history baru (tanpa sampel lama) → AC-13
- [ ] Hentikan daemon saat tab Metrics terbuka → data terakhir tetap tampil tanpa diredupkan, banner offline shell muncul, tidak ada permintaan baru; jalankan daemon lagi → polling lanjut tanpa reload halaman → AC-13
- [ ] Tab Summary: baris Restarts "{n} since created" dan baris Readiness "{status} · {n} ms" pada instance running; inspector: Restarts dengan total dan jendela restart, Readiness dengan latensi → AC-6, AC-7
- [ ] Pembuktian di Mac asli (PostgreSQL 17 native: "Process tree · {n} processes", dan Mac yang sleep lalu bangun) ikut langkah 9 → AC-4, AC-15

### Commands

- [x] `swift test --package-path clients/swift --filter InstanceMetricsClientTests` → 10 test lulus: parameter kursor, balasan instance atau jenis lain ditolak sebagai pelanggaran protokol, penolakan daemon diteruskan, fixture live, not running, tanpa sampel baru, nilai tidak dikenal menjadi `.unrecognized`, dan `type` metrik asing gagal didecode (batas typeshare, lihat catatan) → AC-11, AC-13
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter InstanceMetricsModelTests` → 15 test lulus: baca pertama tanpa kursor lalu tiap `intervalSeconds` dengan `historyId` dan `afterSeq`, polling berhenti saat tab terakhir hilang, `historyReset` dan `historyId` lain mengganti history, sampel ganda tidak ditambah, kapasitas 60, daftar proses lama disimpan sampai ada sampel baru, instance berhenti membuang history, penolakan pertama gagal lalu pulih, penolakan sesudah data menyimpan data, koneksi putus menjeda polling sampai tersambung lagi → AC-13
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter MetricsFormatTests` → 22 test lulus: teks tetap tiap reason unavailable, label cakupan ("Main process", "Process tree · {n} processes", "{m} of {n} measured"), lima kartu, Disk I/O sebagian hilang, kartu Disk ("Scanned {jam}", "at least"), baris health dengan latensi, grafik tidak menggambar sampel tanpa CPU sebagai nol, latensi di baris Readiness, "{n} since created" → AC-3, AC-4, AC-6, AC-7, AC-13
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter InstanceMetricsSnapshotTests` → 7 snapshot (live main, live tree, sebagian terukur, unavailable, daftar terpotong, not running, halaman detail) dalam tiga varian (light, dark, narrow), dibaca lewat tool Read; snapshot Summary, inspector, dan tab Logs yang ikut berubah (baris Restarts dan latensi, tab ketiga) direkam ulang → AC-6, AC-7, AC-13
- [x] `cargo xtask build-app` → build scheme Stackly hijau
- [x] `xcrun swift-format lint --strict` pada file yang berubah → tanpa temuan

### Acceptance-criteria coverage

- AC-6 · baris Restarts "{n} since created" di Details card tab Summary, di inspector (bersama jendela restart bila ada), dan kartu Restarts "Since created" di tab Metrics; angka dari `restartTotal` (`InstanceSummary` dan `MetricsReply`)
- AC-7 · baris Readiness di Summary dan inspector memakai "{status} · {n} ms" lewat `ReadinessText`; header tab Metrics menulis state health, alasan, jam `observedAt`, dan latensi, dengan readiness sebagai pengganti selama health masih `unknown`; Mac yang sleep (reason `waking`) menunggu langkah 9
- AC-13 · tab Metrics dengan kontrol Main Process dan Process Tree, lima kartu, grafik, daftar Processes seluruh pohon dengan baris "{n} more processes not shown", teks tetap per reason unavailable, state "No Live Metrics" dengan Start, polling per `intervalSeconds` hanya selama tab terlihat dengan `historyId` dan `afterSeq`, dan penggantian history saat `historyReset`
- AC-4 · bagian layar: subjudul kartu dan label "{m} of {n} measured"; angka PostgreSQL dan NATS asli menunggu langkah 9
- AC-1 sampai AC-3, AC-5, AC-8 sampai AC-12, AC-14, AC-15 · milik milestone lain (AC-3 di layar: nilai unavailable tampil "Unavailable" dengan alasannya, tidak pernah nol)

### Catatan

- Header health: pesan `MetricsReply` tidak membawa tanda "provider tidak punya health plan", jadi tab memakai health, dan memakai readiness hanya selama health `unknown` sementara readiness sudah punya nilai.
- Batas typeshare: `NumberMetric`, `CountMetric`, dan `DataUsageMetric` dengan `type` asing melempar `typeMismatch` saat didecode, bukan `.unrecognized`. Pada daemon yang lebih baru dari app, balasan `instance.get_metrics` seperti itu tidak terbaca dan tab menampilkan "Couldn’t Load Metrics" (atau menahan data lama dengan banner "Couldn’t refresh metrics"), tidak pernah angka nol.
- Jumlah "{n} more processes not shown" dihitung dari `tree.processCount` sampel terbaru dikurangi baris yang tampil, karena balasan hanya membawa `processesTruncated`.

## Milestone 5: Evidence on this Mac

_Langkah 9: tiga run NFR-003 dan test native PostgreSQL 17 dan NATS sudah dijalankan di Mac ini lewat `cargo xtask evidence run --kind native` dengan artefak dari cache runtime xtask (tanpa unduhan). Tiga run NFR-003 diulang pada build release (`dev-f28-m5-2`) dan semuanya lulus anggaran. Sesi GUI dengan sleep sungguhan butuh kamu. Tidak ada kode produksi yang diubah._

### UI / manual

- [ ] NOT_RUN, menunggu user: sesi GUI dengan Mac yang benar benar sleep lalu bangun, instance PostgreSQL 17 ready sebelum sleep → AC-9, AC-13, AC-7, AC-4, AC-15. Siapkan: `cargo xtask build-app`, buka `target/app/Debug/Stackly.app`, dan jalankan daemon dari app (root asli, bukan test root).
  1. Buat instance PostgreSQL 17 bernama `pg-sleep` dengan port Fixed 5439 (Catalog, PostgreSQL, New Instance), lalu Start. Tunggu status Running dan header Metrics menulis Healthy. Catat PID dari tab Summary (baris Process), port 5439, dan angka Restarts ("0 since created").
  2. Buka tab Metrics. Cek lima kartu (CPU, Memory, Disk, Disk I/O, Restarts), grafik "CPU · last 60 samples" dengan "sampled every 5 s", daftar Processes dengan lebih dari satu baris (postmaster dan proses latar belakangnya), dan pada Process Tree subjudul "Process tree · {n} processes" dengan n lebih dari 1. Biarkan terbuka sekitar satu menit dan lihat grafik terisi. → AC-4, AC-13
  3. Di Terminal: `stackly instance get pg-sleep --json` dan catat `readiness.observedAt`, `readiness.latencyMs`, dan `process.pid`.
  4. Tidurkan Mac (menu Apple, Sleep) paling sedikit 2 menit, lalu bangunkan dan buka kunci layar. Jangan tutup app.
  5. Dalam beberapa detik sesudah bangun: tab Summary dan header Metrics sebentar menulis Unknown dengan alasan `waking` (detail "rechecking after sleep"), lalu kembali Ready dan Healthy dengan latensi ("Ready · {n} ms") dalam paling lama 30 detik (masa tenggang). Ambil `stackly instance get pg-sleep --json` lagi sesudah itu. → AC-7, AC-9
  6. Cek: PID sama dengan langkah 1 (instance tidak di restart), port tetap 5439 (Fixed, tidak pindah), Restarts masih "0 since created", dan `lifecycle` tetap `running`. → AC-9
  7. Tab Metrics sesudah bangun: sampel terus bertambah (grafik bergeser), tidak ada batang CPU yang tidak masuk akal tepat sesudah bangun (main process jauh di atas 100 persen), dan "Process tree · {n} processes" tetap sama. → AC-2, AC-13
  8. Cari baris wake di log daemon: `grep -i wake "$HOME/Library/Logs/Stackly/daemon/stacklyd.log" | tail -3` (lokasi log bisa dilihat di `stackly daemon status`), harus ada satu baris info dengan lama sleep. → AC-9
  9. Catat hasilnya sebagai evidence manual: `cargo xtask evidence new --covers REQ-018 --covers AC-018 --kind native --title "Metrics tab and wake after a real sleep" --expected "after a real sleep and wake a ready PostgreSQL 17 instance shows waking, returns to ready and healthy within 30 seconds on the same pid and the same fixed port, and the Metrics tab keeps sampling" --subject postgresql@17.11`, lalu isi status PASS atau FAIL dan catatannya di file `evidence/manual/` yang dibuat, dan jalankan `cargo xtask evidence check`.
  Bila tidak dijalankan, catat BLOCKED dengan reason `needs a real sleep by the user` dan owner `@ojiepermana`.

### Commands

- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test postgresql --test nats -- --ignored --test-threads 1 metrics::` → 2 test lulus, 2 record PASS, 0 problem (run `01a19b3e-a5d2-7265-bb79-82fea543604b`; record `01a19b3e-eb02-72ba-944c-07530cc391bc` dan `01a19b3e-ff91-73dc-8a96-029a0077eb88`) → AC-2, AC-4, AC-5, AC-15
  - PostgreSQL 17.11: pohon lebih dari satu proses, `main` satu proses terukur, memori `tree` paling sedikit memori `main`, daftar proses berurut sebagai pohon dengan nama `postgres` (nama file executable, bukan argv), satu sesi psql menambah satu proses di pohon, CPU `tree` naik di atas 0 saat query berjalan dan tidak pernah di bawah CPU `main`
  - NATS 2.15.1: `tree.processCount` 1, `measuredCount` 1, nilai `tree` sama dengan `main` (CPU, memori, Disk I/O), satu baris proses bernama `nats-server`
- [x] `cargo xtask evidence run --kind native -- cargo test --release -p stacklyd --test metrics_nfr003 -- --ignored --test-threads 1 --nocapture` (run `01a19bb7-81c8-7406-8f07-90a34cf7695b`, 3 record PASS, 0 problem), diukur dari luar dengan `ps` selama 300 detik, 60 pembacaan, pada build release (`profile=release` di baris tiap record), RSS daemon sekitar 50 MiB di semua run (batas 150 MiB) → AC-14
  - Load average 1 menit sebelum runnya 2,88 (build release baru selesai, Mac belum sepi sekali) dan sesudahnya 1,74; tidak ada build lain berjalan selama run
  - [x] Run 1, sepuluh instance fake tanpa client (record `01a19bc2-a48a-7209-bca5-6dd925c82e31`): PASS, CPU rata rata 0,426 persen satu core (batas 1,0), RSS maksimum 51 472 KiB
  - [x] Run 2, sepuluh instance fake dengan satu client yang membaca `instance.get_metrics` sepuluh instance tiap 5 detik memakai kursor, tiga record release semuanya PASS:
    - `01a19bbd-9fb0-7306-8bff-fa243ea31621` (run di atas): CPU 0,532 persen, RSS maksimum 51 952 KiB
    - `01a19bc7-f62b-71fe-8d3f-0f5a962de81d` (run `01a19bc7-f478-77bf-b7d8-b83f2cb2479c`, load 2,07 sebelum dan 0,74 sesudah): CPU 0,539 persen, RSS maksimum 51 600 KiB
    - `01a19bcd-01f8-7386-b089-6c142e46178c` (run `01a19bcd-004e-726d-8041-1268a3510677`, load 0,74 sebelum dan 1,11 sesudah): CPU 0,522 persen, RSS maksimum 51 808 KiB
  - [x] Run 3, sembilan instance fake dan satu PostgreSQL 17 (record `01a19bb8-706f-7244-a0df-936ae489f05e`): PASS, CPU rata rata 0,426 persen, RSS maksimum 51 920 KiB, dan metrik instance PostgreSQL melaporkan lebih dari satu proses
  - Record debug lama (`01a19b45-4b0a-7556-affa-199e6632bfbc`, `01a19b55-4ba9-7178-8235-4a45f8349382`, run `01a19b4f-e0b9-708e-a3ef-09984b260873`) mengukur `target/debug/stacklyd` tanpa optimasi karena `cargo test` tanpa `--release` memakai binary debug; angkanya (1,021 dan 1,074 persen untuk run 2) digantikan oleh record release di atas dan tidak berlaku sebagai bukti AC-14
  - Keputusan lead (log Keputusan di `docs/scope/`): AC-14 diukur pada build release, yang memang dikirim. Teks AC tidak berubah. Harness sekarang menulis `profile=debug|release` di baris hasil dan di catatan record, dan doc kepala file menyebut `--release`
- [x] `cargo xtask evidence check` → lulus untuk file milestone ini; tiga pelanggaran lama tersisa hanya di `apps/daemon/tests/native_launchd.rs` (milik `dev-f30-fix-2`)
- [x] `cargo clippy -p stacklyd --all-targets --locked -- -D warnings` dan `cargo fmt --all --check` → bersih

### Acceptance-criteria coverage

- AC-2 · pada proses asli PostgreSQL 17 dan NATS: footprint di atas 0, CPU `tree` bergerak mengikuti query, nilai `tree` NATS sama dengan `main`; hitungan selisih sudah di test fixture milestone 2
- AC-4 · label cakupan dan hitungan: PostgreSQL 17 `tree.processCount` lebih dari 1 dengan memori `tree` paling sedikit memori `main`, NATS `processCount` 1 dan sama dengan `main`; subjudul kartu di GUI menunggu sesi manual
- AC-5 · pohon dan daftar proses pada proses asli (urutan pohon, nama executable, sesi baru muncul sebagai proses baru)
- AC-14 · terpenuhi pada build release: RSS sekitar 50 MiB dari 150 MiB di ketiga run, CPU rata rata 0,426 persen tanpa client, 0,522 sampai 0,539 persen dengan client (tiga record, load Mac 0,7 sampai 2,9), dan 0,426 persen dengan satu PostgreSQL 17. Run debug lama (1,02 sampai 1,07 persen) mengukur build tanpa optimasi dan digantikan. Satu catatan: anggaran diukur pada build release atas keputusan lead, karena teks AC tidak menyebut profil build
- AC-15 · test native PostgreSQL 17 dan NATS lulus; test wake dengan clock palsu milik milestone 4; cek manual GUI dengan sleep sungguhan NOT_RUN menunggu user
- AC-9, AC-13, AC-7 (bagian Mac yang sleep) · hanya lewat sesi manual di atas
