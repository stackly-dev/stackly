# Verify: Metrics dan health · spec 0028 · updated 2026-11-02

_Langkah diturunkan dari acceptance criteria spec 0028. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27._

## Milestone 2: Sampler, contract, daemon, and CLI

_Langkah 4 (sampler di core, atas fake), 5 (protokol 2.2, daemon), dan 6 (CLI) sudah dibangun. Tab Metrics di app (langkah 7), latensi probe dan wake (langkah 8), dan evidence native (langkah 9) belum._

### UI / manual

- [ ] Mulai daemon dengan root uji (`STACKLY_ROOT=$(mktemp -d) cargo run -q -p stacklyd`), buat dan jalankan satu instance fake lewat `cargo run -q -p stacklyd --example skeleton_seed -- --root <R>` (perintah `stackly instance create` yang dicetaknya), lalu `stackly instance metrics <nama>` → baris Process, Uptime, Restarts "0 since created", Readiness, Health, Data size, dan dua blok "main process (pid ...)" serta "process tree (...)" dengan daftar proses; hitungan pertama menampilkan `unavailable (warming_up)` untuk CPU dan Disk I/O, bukan 0 → AC-1, AC-3, AC-12
- [ ] `stackly instance metrics <nama> --watch` → satu baris per sampel baru kira kira tiap 5 detik; stop instance lalu jalankan lagi, muncul "not running (stopped)" dan sesudah running lagi "history reset"; Ctrl+C keluar 0 → AC-12
- [ ] `stackly instance metrics <nama> --json` → satu amplop dengan `result` sama dengan balasan `instance.get_metrics` → AC-11, AC-12
- [ ] Pembuktian di Mac asli (PostgreSQL 17 dan NATS native, pohon proses nyata) ikut langkah 9.

### Commands

- [ ] `cargo test -p stackly-core --test metrics_sampler` → 20 test lulus: sampel pertama punya memori dan CPU serta Disk I/O `warming_up`, sampel kedua punya CPU dan Disk I/O (50.0 persen untuk 2,5 detik CPU dalam 5 detik, 2000 byte per detik untuk 10000 byte tulis), pohon menjumlah proses utama dan turunan dengan uid yang sama dan melewati proses cabang lain, turunan milik user lain beserta anaknya, urutan baris sebagai pohon dengan saudara urut PID, CPU pohon lebih dari 100, siklus parent berakhir, proses utama ditolak (`EPERM`) menjadi `not_permitted` untuk `main` dan `tree` dengan `measuredCount` 0, anak ditolak tinggal mengurangi `measuredCount`, proses utama hilang di tengah sampel atau dari daftar PID menjadi `process_exited`, anak yang keluar hilang dari baris, PID anak yang dipakai ulang mulai `warming_up` dan hanya di-`inspect` ulang sekali, `inspect` sekali per proses baru dan bukan setiap tick, proses utama baru membuka history baru dengan `seq` 1, history dibuang saat instance tidak lagi running (dan `list_pids` tidak dipanggil kalau tidak ada yang running), sampel ke 61 membuang yang tertua, baris dipotong di 200 sementara jumlah pohon tetap 206 proses, dua instance punya history masing masing, `list_pids` `not_supported` menjadi `not_supported`, daftar PID yang gagal dibaca tidak mencatat sampel, task sampler mengambil sampel tiap interval tanpa client dan berhenti setelah `stop`, dan konfigurasi bisa diperpendek → AC-1, AC-3, AC-5
- [ ] `cargo test -p stackly-domain --lib metrics` → unit test tipe dan fungsi murni yang dipakai sampler (selisih CPU dan Disk I/O, `measure`, jumlah pohon, urutan pohon, ring 60, aturan cursor) tetap lulus → AC-1, AC-2, AC-3, AC-5
- [ ] `cargo clippy -p stackly-core --all-targets --locked -- -D warnings` → tanpa peringatan; setiap item publik di `stackly_core::metrics` punya doc comment, tidak ada `unwrap()` di luar test → AC-1
- [ ] `cargo xtask check-deps` → lulus; `stackly_core::metrics` tidak memanggil API spawn, tidak menyebut `SIGKILL`, dan tidak menulis registry; sampler dan pemindaian terjadwal di daemon tidak mengirim probe atau sinyal → AC-1
- [ ] `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → exit 0 (modul `metrics` tidak memakai API macOS) → AC-1
- [ ] `cargo xtask codegen --check` → "up to date"; `ProtocolVersion::CURRENT` 2.2 dan `protocols/contract-versions.json` mencatat digestnya → AC-11
- [ ] `cargo test -p stackly-protocol` → termasuk `metrics_fixtures` (6 test: live, not running, reset, nilai tidak dikenal, request dan penolakan kursor, balasan terbesar 60 sampel dan 200 baris di bawah 1 MiB) dan `ipc_fixtures` yang membaca `frame-response-instance-get-metrics-*.json` dan `request-instance-get-metrics*.json` → AC-3, AC-11
- [ ] `cargo test -p stacklyd --test supervisor metrics` → 7 test lulus lewat socket pada proses fake server asli: sampel tanpa client yang meminta (sampel pertama `warming_up`, memori terisi, pohon dan daftar proses), aturan kursor (`afterSeq` baru, `historyId` lain mereset, `afterSeq` di luar tanpa reset), `not_running` tanpa history dan tanpa pemindaian oleh `get_metrics`, `resource.not_found`, history dibuang saat instance berhenti, pemindaian terjadwal mengisi `last_scan` dan pemindaian on demand ikut mengisinya, dan `restartTotal` naik satu setelah proses dibunuh dan di-respawn → AC-1, AC-3, AC-5, AC-6, AC-10, AC-11
- [ ] `cargo test -p stackly-cli --test instance` → termasuk `metrics` (5 test: dua cakupan dengan label dan golden `instance_metrics.txt`, `unavailable (reason)`, `--json` sama dengan balasan, "not running (stopped)" keluar 0, nama tidak dikenal keluar 10 `cli.instance_not_found`, dan `--watch` dengan kursor, "history reset", Ctrl+C keluar 0) dan golden `instance_*` yang kini memuat `restartTotal` dan "since created"; `a_bad_draft_gets_the_same_error_through_the_client_and_through_the_cli` kadang gagal saat mesin sibuk dan lulus kalau dijalankan sendiri → AC-6, AC-12
- [ ] `swift test --package-path clients/swift` → lulus (hitungan method 44 dan protokol 2.2, semua fixture indeks) → AC-3, AC-11
- [ ] `swift build --package-path apps/macos/Packages/StacklyUI --build-tests` → hijau setelah init `InstanceSummary`, `InstanceReadiness`, dan `InstanceHealth` mendapat `restartTotal` dan `latencyMs` netral; tampilan Restarts dan latensi menunggu langkah 7 → AC-6, AC-7

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
