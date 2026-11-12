# Langkah manual: `07-stack-enam-service`

Langkah yang tidak dijalankan sesi (GUI, unduhan runtime, sesi login launchd). Owner: user. Tiap langkah menyebut AC yang dibuktikannya; hasilnya dicatat di `verify.md` spec itu.

## Fitur 41 · Provider Traefik & routing lokal (spec 0041)

Prasyarat: milestone 3 spec 0041 sudah di `main` (Traefik `[[provider]]`), app dibangun dengan `cargo xtask build-app`, satu server HTTP lokal sebagai backend. Nilai dari user: URL backend lokal, port web kedua instance.

1. Di app: Catalog, pilih Traefik, pasang runtime 3.7.15 (unduhan), buat instance `edge-a` dan `edge-b`, Start keduanya. Harapan: keduanya ready. Bukti AC-10, AC-2.
2. `edge-a`, tab Configuration: tambah route Name `api`, Rule ``Host(`app.localhost`)``, Backend URL server lokal. Harapan: tag "Applies on reload", tombol Apply and Reload. Apply, lalu `curl -H 'Host: app.localhost' http://127.0.0.1:<port web edge-a>/` menjawab backend; perintah yang sama ke port web `edge-b` menjawab 404. Bukti AC-9, AC-2. Sebelum itu, isi Backend URL dengan path, misalnya `http://127.0.0.1:3000/api`, lalu Apply. Harapan: error di field Backend URL, tidak ada revision baru; hapus path nya lalu lanjutkan. Bukti AC-15.
3. Ubah Log level. Harapan: tag "Requires restart", tombol Apply and Restart. Bukti AC-9.
4. Ganti Rule menjadi `Host(` lalu Apply. Harapan: kartu gagal, status not ready, banner "Revision N hasn't reached ready"; klik Roll Back to Revision M, route menjawab lagi. Bukti AC-4.
5. Menu Open In: Routes File membuka Finder di `routes.yaml`; Loaded Routers membuka JSON router di browser. Bukti AC-1.
6. Doctor saat langkah 4 sebelum roll back. Harapan: remedy "Traefik hasn't loaded the routes Stackly wrote". Bukti AC-13.
7. AC-12, di sesi GUI yang login. Test ini gagal di assert `Keep/HandoverProven` sampai baris handover ada, jadi pasang dulu secara lokal: di `crates/core/src/supervisor/handover.rs` tambah `HandoverProof::new("official/traefik", "1", "spec-0041-ac-12"),` ke `HANDOVER_PROVEN`, dan di modul test nya ubah `("official/traefik", "1.0.0", false)` menjadi `true`. Lalu `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test native_launchd --locked -- --ignored --test-threads 1 traefik_survives`. Harapan: `traefik_survives_planned_restart_and_sigkill_and_returns_after_kickstart_and_bootout` lulus (PID dan waktu mulai sama sesudah planned restart dan SIGKILL daemon, route menjawab sesudah setiap akhir daemon, proses baru sesudah kickstart dan bootout). Supaya record tidak `sourceDirty`, commit dulu kedua baris itu sebelum menjalankan, lalu buang commit nya bila hasilnya bukan PASS. PASS: pertahankan commit itu bersama ID record nya. FAIL atau BLOCKED: buang keduanya; AC-12 tetap terbuka.

## Fitur 39 · Provider NATS & JetStream (spec 0039)

1. AC-13, run launchd NATS pada paket `1.1.0`, di sesi login GUI, setelah langkah 8 Build plan: `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test native_launchd -- --ignored --test-threads 1 nats_survives`. Harapan: `nats_survives_planned_restart_and_sigkill_and_returns_after_kickstart_and_bootout` PASS; catat ID record evidence di `verify.md`. Kalau gagal, kembali ke `/architect`.
2. AC-11, pemeriksaan GUI: sheet New Instance, pilih NATS, biarkan password Generate, Create lalu Start. Harapan: Summary menampilkan readiness ready dan Client URL ter-mask (`nats://stackly:****@127.0.0.1:<port>`) dengan tombol salin, dan hasil salin memuat password yang dibangkitkan; tab Configuration menampilkan form dengan batas MiB dan KiB dan tanpa Effective File.
3. AC-2, fuzz (bila toolchain nightly terpasang): `cargo xtask fuzz run provider_template --time 600`. Harapan: tanpa crash; tanpa toolchain `BLOCKED`, owner user.

## Fitur 43 · Provider ClickHouse (spec 0043)

1. Run spike native dari paket: `clickhouse_runs_from_managed_artifact` di `xtask/tests/native_runtime.rs` lewat wrapper native di tree bersih (bisa mengunduh, butuh sesi login GUI).
2. Fuzz 10 menit sesudah format `xml` dan filter `sha256Hex` masuk: `cargo xtask fuzz run provider_template --time 600`. Harapan: tanpa crash.
3. Sesi GUI: buat instance ClickHouse dari form, start, salin connection, buka Play UI, buka `clickhouse client` di Terminal dan tempel password (membuktikan `--ask-password`), lalu stop. Catatan: open action `client` membaca `~/.clickhouse-client/config.xml` milik user; bila `--ask-password` tampak gagal, periksa file itu dulu.
4. Di app, sesudah catalog revision 15: ClickHouse tampil sebagai provider dengan catatan Windows tidak didukung (menyebut WSL); buat instance dari Catalog dan start (AC-13, AC-16).

## Fitur 42 · Provider MySQL (spec 0042)

1. Di aplikasi: buat instance MySQL dari Catalog, start, salin connection string, lalu Open In TablePlus, Sequel Ace, dan mysql (Terminal). Ini juga memeriksa bundle ID Sequel Ace `com.sequel-ace.sequel-ace` dan bahwa kedua aplikasi menerima URL `mysql://`.
2. Di aplikasi: ubah Max connections di halaman Configuration. Harapan: dampak restart ditampilkan.
3. Handover (AC-17), di sesi GUI yang login, sesudah MySQL masuk catalog (revision 16). Di `handover.rs` tambah `HandoverProof::new("official/mysql", "1", "spec-0042-ac-17"),` dan di tabel test nya `("official/mysql", "1.0.0", true)`; commit dulu. Lalu `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test native_launchd --locked -- --ignored --test-threads 1 mysql_`. Harapan: `mysql_8_4_…` dan `mysql_9_7_survives_planned_restart_and_sigkill_and_returns_after_kickstart_and_bootout` keduanya PASS. PASS: pertahankan commit itu bersama ID record nya. Selain itu buang commit nya; AC-17 tetap terbuka.

## Fitur 40 · Provider Valkey (spec 0040)

1. Handover (AC-19), di sesi GUI yang login. Di `crates/core/src/supervisor/handover.rs` tambah `HandoverProof::new("official/valkey", "1", "spec-0040-ac-19"),` ke `HANDOVER_PROVEN`, dan di tabel test modul itu (yang memuat `("official/clickhouse", "1.0.0", false)`) tambah `("official/valkey", "1.0.0", true)`; commit dulu kedua baris itu supaya record tidak `sourceDirty`. Lalu `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test native_launchd --locked -- --ignored --test-threads 1 valkey_9_1_survives`. Harapan: `valkey_9_1_survives_planned_restart_and_sigkill` lulus (PID dan `startedAt` sama sesudah planned restart dan SIGKILL daemon, proses baru sesudah kickstart dan bootout). PASS: pertahankan commit itu bersama ID record nya. FAIL atau BLOCKED: buang commit nya; AC-19 tetap terbuka.
2. Form dan Configuration (AC-7). Form create Valkey: teks `Persistence` tampil di bawah kontrolnya. Buat instance `volatile`: halaman Configuration menampilkan field itu terkunci dengan nilai `volatile` dan file `persistence-volatile.conf`.
3. TablePlus (AC-18). Prasyarat: TablePlus terpasang, instance Valkey `ready`. `Open in TablePlus`: koneksi Redis terbuka ke port instance, password dari clipboard diterima. Kalau TablePlus menolak URL tanpa password, entri `tableplus` dihapus lewat revision paket baru.
4. Terminal (AC-18). `Open valkey-cli in Terminal`, tempel password: prompt `valkey-cli` terbuka.
5. Fuzz (AC-2, AC-3): sudah dijalankan checker `chk-f40-m1` pada 2026-11-12 (`Done 90583 runs in 601 second(s)`, tanpa crash); tidak perlu diulang.
