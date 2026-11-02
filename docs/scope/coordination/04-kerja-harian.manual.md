# Langkah manual: `04-kerja-harian`

Langkah yang butuh mata dan tangan user di Mac ini. Kirim hasilnya ke lead (nomor langkah, sesuai atau tidak, versi macOS, dan output `ps -ww` di B2). Lead meneruskannya ke checker yang mencentang `verify.md`.

## F70 · Open In (spec 0070)

Disusun oleh `dev-f70-m3` (2026-11-02). Semua perintah dijalankan dari root checkout utama setelah `main` memuat F70 milestone 3.

### A. Milestone 2: menu Open In dengan paket fake

Siapkan sekali:

```bash
cargo build -p stacklyd -p stackly-cli -p stackly-fake-server && cargo build -p stacklyd --example skeleton_seed && cargo xtask build-app
```

```bash
export R="$(mktemp -d /tmp/stk-f70.XXXXXX)"; S=target/debug/stackly; target/debug/examples/skeleton_seed --root "$R"
```

Catat UUID di baris `runtime` sebagai `RT`. Terminal B: `STACKLY_ROOT="$R" target/debug/stacklyd` (biarkan jalan). Terminal C: `STACKLY_ROOT="$R" target/app/Debug/Stackly.app/Contents/MacOS/Stackly`, lalu pilih Instances. Terminal A: `STACKLY_ROOT="$R" $S instance create demo --runtime $RT --provider testing/fake --revision 0.4.0 --start --wait`.

Isi menu yang diharapkan untuk paket fake: "Open in Fake Client" **tidak tampil** (aplikasinya tidak terpasang); "Open Fake Server in Terminal"; "Open Status Page in Browser"; pemisah; "Reveal Notes in Finder" (nonaktif "The file doesn’t exist yet." bila file belum ada). Jangan klik item Terminal atau Browser di bagian A.

1. Inspector Instances, `demo` running dan ready: ada tombol menu (ikon panah keluar dari kotak) di samping Open. Menu bar Instance punya submenu "Open In" tanpa shortcut.
2. Detail `demo`: toolbar punya tombol menu Open In dengan isi yang sama.
3. `STACKLY_ROOT="$R" $S instance stop demo --wait`: item Terminal dan Browser nonaktif dengan teks "Start the instance to open it."; "Reveal Notes in Finder" tetap aktif bila file ada.
4. Ctrl+C di Terminal B (daemon mati): semua item Open In nonaktif, mengikuti banner.
5. Provider tanpa `openWith`, atau skenario fixture: tombol dan submenu tidak tampil.
6. Instance yang baru dipilih (detail belum termuat): satu item nonaktif "Loading…". Tanpa instance terpilih, submenu nonaktif.
7. Dikerjakan bersama B4: hapus password lewat Enter Secret, lalu "Open in TablePlus". Aplikasi tetap terbuka, dan kartu caution "Opened without copying the password." muncul dengan "Enter Secret…".

### B. Milestone 3: aplikasi asli

Root sementara memakai keychain file, jadi login Keychain tidak disentuh. Siapkan seperti `docs/manual.md` bagian T langkah 1 sampai 5: root `$R`, catalog dev, dan daemon. Salin PostgreSQL 18.6 dari cache `~/Library/Caches/dev.stackly.xtask/runtime/88bb7533…` ke `$R/cache/downloads/` (tanpa jaringan). Pasang PostgreSQL 18 lewat Get, buat instance `pg-open`, lalu Start sampai ready. Untuk NATS, salin juga `ed0fe392…` ke folder yang sama dan buat instance `nats-open`.

1. "Open psql in Terminal": Terminal baru terbuka tanpa dialog Gatekeeper. `psql` meminta `Password for user postgres:`, ⌘V menempel, dan `SELECT 1;` menghasilkan `1`.
2. Selama sesi psql: `ps -ww -axo pid,ppid,command | grep psql` tidak memuat password.
3. `stackly instance stop pg-open --wait`, lalu buka `$R/data/instances/<id>/temp/open-psql.command` dari Finder: tampil "psql exited with status 2. Press Return to close." dan skrip menunggu Return.
4. Start lagi, lalu "Open in TablePlus": TablePlus terbuka ke `127.0.0.1:<port>` dengan user `postgres` dan meminta password. ⌘V menempel dan koneksi berhasil. Kartu "Opening “pg-open” in TablePlus. Password copied. Paste it when TablePlus asks." muncul.
5. Postico tidak terpasang di Mac ini: `BLOCKED` (reason: aplikasi tidak terpasang; owner: user). Pasang Postico 2 bila ingin membuktikan bundle ID-nya.
6. NATS, "Open Monitoring in Browser": browser default membuka `http://127.0.0.1:<port>/` dengan halaman awal monitoring NATS.
7. Traefik: `BLOCKED` (paket Traefik masih `[[planned]]` di `providers/catalog.toml`; owner: fitur provider Traefik).

## F30 · Restart daemon dari Settings (spec 0030 milestone 4)

Disusun oleh `chk-f30-m4` (2026-11-02). Rig-nya (daemon launchd di test root sendiri dengan label `dev.stackly.test.agent.*`, plus satu instance fake `demo` dengan keputusan `stop_restore`) disiapkan oleh sub sesi atas permintaan lead saat kamu siap. Rig ini tidak menyentuh `com.ojiepermana.stackly.daemon`.

1. Buka app dengan baris `STACKLY_ROOT=... Stackly` yang dicetak rig. Buka Stackly > Settings: ada tab General dengan bagian Daemon dan baris `stacklyd` bersubjudul "Connected over Unix socket · protocol N". Tombol Restart Daemon aktif, tanpa tab atau baris lain (AC-16).
2. Klik Restart Daemon: sheet "Stops, then starts again" berisi `demo` dengan alasannya, dan "Keeps running" kosong. Cancel menutup sheet tanpa efek apa pun.
3. Klik Restart Daemon lagi lalu Restart: banner "stacklyd is restarting" tampil (0 tetap jalan, 1 berhenti), dan baris `demo` di halaman Instances bertanda "Stopped for daemon restart". Lalu banner ringkasan dan toast "Daemon restarted. 0 instances adopted, 1 started again." muncul, dan banner hilang.
4. Putus daemon (`launchctl bootout` label test dari rig): subjudul berubah "Disconnected · reconnecting" dan tombol nonaktif. Lalu jalankan daemon manual `STACKLY_ROOT=<root> target/debug/stacklyd`: tampil "Runs outside launchd, so it can't restart itself" dan tombol tetap nonaktif.
5. Bersihkan lewat skrip `down` dari rig.

Langkah `restart_busy` (create yang berjalan lebih dari 30 detik) tidak bisa dibuat dengan provider fake tanpa mengubah kode. Lead menerimanya sebagai tercakup oleh test `a_create_that_outlasts_the_quiesce_wait...` dan `DaemonRestartModelTests`.

## F28 · Tab Metrics (spec 0028 milestone 3)

Disusun oleh `chk-f28-m3` (2026-11-02). Rig: daemon di root uji dengan `skeleton_seed` dan satu instance fake yang running, lalu app `target/app/Debug/Stackly.app` dengan `STACKLY_ROOT` root itu (disiapkan sub sesi atas permintaan lead).

1. Buka instance, tab Metrics: ada lima kartu, grafik 60 sampel dengan "sampled every 5 s", daftar Processes, dan header health dengan jam dan latensi. Sampel pertama tiba dalam 5 detik; CPU "Measuring…" lalu terisi (AC-13).
2. Pindah Main Process dan Process Tree: subjudul kartu berubah, sedangkan daftar Processes tetap menampilkan seluruh pohon (AC-4, AC-13).
3. Pindah ke Summary lalu kembali: selama tab tersembunyi tidak ada `instance.get_metrics` di log daemon level debug, dan history lama langsung tampil (AC-13).
4. Stop instance dari toolbar saat tab Metrics terbuka: tampil "No Live Metrics" dengan tombol Start aktif. Start memulai history baru (AC-13).
5. Hentikan daemon saat tab Metrics terbuka: data tetap tampil tanpa diredupkan, banner offline shell muncul, dan tidak ada permintaan baru. Jalankan daemon lagi: polling lanjut tanpa reload (AC-13).
6. Summary: baris Restarts "{n} since created" dan Readiness "{status} · {n} ms" pada instance running. Inspector: Restarts dengan total dan jendela restart, serta Readiness dengan latensi (AC-6, AC-7).

## F29 · Tab Configuration dan sheet (spec 0029 milestone 3)

Disusun oleh `chk-f29-m3` (2026-11-02). Rig: `cargo build -p stacklyd -p stackly-fake-server && cargo run -q -p stacklyd --example skeleton_seed -- --root <R>`, lalu app dengan `STACKLY_ROOT=<R>` (disiapkan sub sesi atas permintaan lead).

1. Pilih instance, buka tab Configuration, ubah satu field, tunggu 300 ms, lalu tekan Apply (konfirmasi bila diminta): kartu "Revision N applied to <nama>." muncul dan History bertambah satu baris.
2. Menu Instance → "Rename…" dan "Edit Ports…" (tanpa shortcut tertulis), menu More di halaman detail, menu konteks baris di Instances, dan tombol Edit di kartu Endpoints tab Summary: masing-masing membuka sheet yang sama (Rename Instance atau Edit Ports) seperti di snapshot.
