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
