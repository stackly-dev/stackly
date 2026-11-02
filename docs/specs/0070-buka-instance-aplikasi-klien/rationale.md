# 0070. Buka instance di aplikasi klien: rationale

[Kembali ke spec](index.md)

## Context

Setelah instance PostgreSQL atau NATS `ready`, langkah berikutnya hampir selalu membuka instance itu di alat lain: klien database, `psql`, atau halaman monitoring. Hari ini pengguna menyalin connection string dari kartu Connection (spec 0021) lalu menempelnya sendiri. Mockup UI menggambar menu aksi per service di halaman Environments dengan TablePlus, Postico, psql di Terminal, monitoring di browser, dan file routes Traefik di Finder. Halaman Environments belum ada (fitur 44 masih planned), jadi permukaan untuk fitur ini harus berdiri sendiri dulu.

Ada empat gaya yang memaksa bentuk solusinya. Pertama, aturan provider: daftar aksi harus datang dari capability provider, bukan dari kode SwiftUI per provider, sama seperti `connection` di spec 0021. Kedua, aturan secret (spec 0011, AC-016): password tidak boleh masuk URL, argumen, atau daftar proses. Aplikasi seperti TablePlus menerima password di URL, dan URL yang dibuka lewat Launch Services bisa tersimpan di riwayat aplikasi tujuan. Ketiga, aturan proses dan path di AGENTS.md: hanya daemon yang membuat path dan menjalankan proses lewat `ProcessSupervision`, CLI tidak pernah menjalankan binary provider, dan GUI tidak link Rust. Keempat, versi: `psql` dari PATH bisa berbeda mayor dengan server, dan scope meminta binary dari runtime instance itu.

Kalau tidak diputuskan, setiap provider berikutnya (Valkey, MySQL, ClickHouse) akan menambah tombol khusus di Swift, dan password kemungkinan berakhir di URL karena itu jalan paling mudah.

Workspace: aplikasi macOS (`apps/macos`, `StacklyUI`), daemon, CLI, dan core. Keputusan ini menyentuh kontrak protocol (minor bump) dan format paket provider.

## Options considered

### Option 1: blok manifest `openWith`, render di daemon, eksekusi di GUI

Provider mendeklarasikan aksi dengan empat kind tertutup (`app`, `terminal`, `browser`, `reveal`). Daemon merender nilainya tanpa secret ke `InstanceDetail`, menulis skrip Terminal lewat method kecil, dan GUI membuka semuanya lewat NSWorkspace sambil menyalin password ke clipboard tersembunyi.

**Pros**:

- Satu mekanisme untuk semua provider, memakai ulang resolver interpolasi dan validasi sentinel dari `connection`.
- Loader bisa menolak token secret, jadi kebocoran password dicegah oleh struktur, bukan oleh disiplin penulis provider.
- Daemon tetap satu satunya yang membuat path dan menyusun perintah.

**Cons**:

- Kontrak dan format manifest bertambah besar.
- Bundle ID pihak ketiga hidup di paket provider.
- Pengguna harus paste password sendiri.

### Option 2: tambah `openWith` per entri `connection`

Setiap entri connection mendapat daftar bundle ID; GUI membuka connection string itu di aplikasi tersebut.

**Pros**:

- Perubahan manifest paling kecil.
- Tidak ada render baru.

**Cons**:

- Terminal dan Reveal tidak muat, jadi psql dan routes Traefik tetap butuh kode khusus.
- Template connection memuat `${secret.*}`, jadi menghapus bagian secret menghasilkan URL dengan password kosong yang membuat aplikasi mencoba login tanpa password.

### Option 3: daftar aplikasi di GUI

App memetakan kategori provider (database, broker) ke daftar aplikasi dan perintah.

**Pros**:

- Tanpa perubahan kontrak.
- Cepat dibangun untuk PostgreSQL.

**Cons**:

- Melanggar Done when fitur 70 dan RISK-07: setiap provider baru butuh kode Swift.
- GUI harus tahu path binary runtime, padahal itu data daemon.

### Option 4: daemon membuka semuanya

Daemon memanggil `open` untuk aplikasi, browser, dan Terminal.

**Pros**:

- GUI dan CLI setara tanpa kode tambahan.

**Cons**:

- Daemon adalah LaunchAgent; menjalankan `open` berarti spawn di luar `ProcessSupervision`, yang ditolak `cargo xtask check-deps`.
- Aplikasi bisa terbuka di sesi yang salah atau tanpa fokus, dan pemeriksaan "aplikasi terpasang" tetap harus dilakukan di sisi pengguna.

## Rationale

Option 1 dipilih karena hanya opsi ini yang memenuhi keempat gaya sekaligus. Aturan provider terpenuhi karena aksi datang dari manifest. Aturan secret terpenuhi oleh loader, yang menolak `${secret.*}` di semua template `openWith`; pilihan ini juga berarti kalimat Done when tentang "alur peringatan" tidak lagi dibutuhkan, karena aplikasi yang hanya menerima password lewat URL tidak bisa dideklarasikan sama sekali. Engineer memilih penolakan di loader daripada membangun alur peringatan yang belum dipakai aplikasi mana pun. Aturan path dan proses terpenuhi karena daemon hanya menulis file, GUI hanya meminta Launch Services membuka sesuatu, dan Terminal yang menjalankan `psql` sebagai proses pengguna.

Untuk password, engineer memilih membuka tanpa password lalu menyalin password ke `ConcealedPasteboard`. Runner up nya adalah tidak menyalin sama sekali (pengguna memakai Copy di kartu Connection), yang lebih sederhana tapi menambah satu langkah di setiap buka. Clipboard concealed dengan kedaluwarsa (spec 0011) sudah ada, sudah diaudit, dan tidak meninggalkan password di riwayat aplikasi tujuan.

Untuk Terminal, file `.command` yang dibuka dengan Terminal.app dipilih daripada AppleScript `do script`, karena Apple Events membutuhkan izin Automation dan menambah satu dialog izin lagi di produk yang sudah punya izin background. Daemon yang menulis skrip, bukan GUI, karena AGENTS.md menyatakan hanya daemon yang membuat path; method `instance.prepare_terminal` sengaja hanya menerima nama aksi dan merender ulang dari paket pinned, sehingga client tidak bisa menyelipkan perintah. Method ini bukan operation spec 0007 karena tidak mengubah registry, idempoten, dan tidak membawa secret, sama seperti `instance.get_data_usage`. `psql` memakai Unix socket karena itu jalur yang sama dengan readiness probe dan socket 0700 hanya bisa dipakai pengguna ini.

Gate "hanya saat ready" dipilih daripada "saat running" supaya aplikasi tidak dibuka ke server yang belum menerima koneksi, dengan pengecualian `reveal` yang tidak butuh proses. CLI hanya mendaftar (`--list`) karena aturan "CLI tidak menjalankan binary provider" tetap utuh dan perintah lengkap tetap berguna untuk disalin. Dashboard Traefik ditunda ke fitur 41 karena menyalakannya adalah keputusan keamanan paket Traefik, bukan keputusan menu.

## Keputusan kecil yang diambil architect

| Hal | Pilihan | Runner up |
|---|---|---|
| Label menu | dibentuk dari `kind` + `title` oleh satu fungsi murni, bukan label bebas di manifest | label bebas per entri (lebih fleksibel, tapi teks tidak konsisten antar provider) |
| Deteksi aplikasi | `NSWorkspace.urlForApplication(withBundleIdentifier:)` setiap menu dibangun | cache saat app mulai (lebih cepat, tapi basi setelah pengguna memasang aplikasi) |
| Beberapa bundle ID | daftar urut, yang pertama terpasang dipakai (Postico 2 lalu Postico 1) | satu bundle per entri (dua entri Postico di menu) |
| Lokasi skrip | folder `temp` instance, nama tetap per aksi, ditimpa | file acak per buka (menumpuk sampai dibersihkan) |
| Error code | `instance.open_action_not_found` dan `instance.open_action_unavailable`, reason memakai ulang reason connection | satu kode `instance.open_failed` (kehilangan beda NotFound dan Conflict untuk exit code) |
| Umpan balik reveal | tanpa kartu, Finder sudah terlihat | kartu success (berisik) |
