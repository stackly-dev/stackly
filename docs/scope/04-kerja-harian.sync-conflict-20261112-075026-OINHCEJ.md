# Scope 04 · Kerja harian

[Kembali ke index](index.md)

Journey yang paling sering dipakai setelah instance ada: stop, restart, lihat log, pantau metrik, ubah config, dan percaya bahwa instance tetap benar walau GUI ditutup, daemon restart, Mac login ulang, atau tidak ada koneksi internet. Fase ini selesai kalau seluruh pekerjaan harian bisa dilakukan dari GUI, CLI, dan menu bar dengan state yang jujur.

### 26. Stop, restart & reload

Lifecycle lengkap sesuai capability provider: stop graceful dengan timeout, restart yang diserialisasi, reload hanya kalau provider mendukung (sesuai capability dari spec fitur 9 dan 10), dan opsi force dengan risiko yang jelas setelah timeout.

**Done when:** stop normal menjalankan strategi provider dan melaporkan hasilnya; stop dan start yang bersamaan diserialisasi; perilaku reload dicoba pada PostgreSQL (mendukung reload) dan NATS (tidak mendukung), dan provider tanpa hot reload meminta restart terkontrol, bukan klaim reload palsu; stop dan restart tidak memerlukan jaringan; force kill tidak pernah menyasar proses yang identitasnya belum terverifikasi; `stackly instance restart` dan `reload` setara dengan GUI; alur stop dan inspect bisa diselesaikan dengan interaksi desktop; tombol start atau stop, restart, dan reload di toolbar detail instance mengikuti mockup UI, dan reload nonaktif beserta alasannya untuk provider yang tidak mendukungnya.

**Trace:** REQ-011, REQ-028, REQ-046 (offline), BR-016, AC-011, AC-026, AC-027, AC-046, AC-067, NFR-012

- [ ] Build it: `/develop stop, restart & reload`

### 27. Log per instance · needs a decision

Log terpisah untuk tiap instance dengan follow, cursor, filter dasar, timestamp, pemisahan stdout dan stderr, batas ukuran, dan rotasi. Subscriber yang lambat tidak boleh menahan pipe proses.

**Done when:** log besar dengan client lambat tidak membuat daemon lambat atau memori tumbuh tanpa batas; rotasi berjalan (default usulan 5 file × 20 MiB); segmen yang hilang ditandai sebagai truncation; `stackly instance logs --follow` dan layar Logs setara; secret tidak pernah muncul; layar Logs mengikuti mockup UI (sumber All, stdout, dan stderr, filter, Follow, Load Earlier, serta cursor dan info retention di footer).

**Trace:** REQ-017, PAGE-07, NFR-004, AC-017, PRD bagian 35.2

- [ ] Design it (spec): `/architect log per instance`

### 28. Metrics & health · needs a decision

Status, readiness, process tree, uptime, restart count, kegagalan terakhir, CPU, memori, dan pemakaian disk terjadwal, dengan label cakupan yang jelas antara proses utama dan agregat child.

**Done when:** metrik yang tidak tersedia tampil unavailable, bukan nol, termasuk di `stackly instance metrics --json`; label menjelaskan cakupan agregasi untuk provider multiprocess; health punya timestamp dan alasan; cadence probe dan sampling default (usulan 5 detik) tidak melampaui anggaran NFR-003 pada 10 instance idle; setelah Mac bangun dari sleep, endpoint dicek ulang tanpa mengganti port fixed; layar Metrics mengikuti mockup UI, termasuk pilihan Main Process dan Process Tree, metrik unavailable yang diberi alasan, dan state tanpa metrik saat instance tidak running.

**Trace:** REQ-018, REQ-028, PAGE-07, NFR-003, AC-018, PRD bagian 19.5 dan 35.2

- [ ] Design it (spec): `/architect metrics & health`

### 29. Edit config, nama & endpoint · needs a decision

Form typed dan advanced view dari schema provider, memakai renderer form yang sama dengan fitur 20, dengan diff, validasi, revision, dampak reload atau restart, histori minimal last known good, dan deteksi drift. Termasuk ganti nama (label saja; ID, runtime, credential reference, dan storage path tetap) dan ganti endpoint atau port dengan validasi yang sama seperti create.

**Done when:** config yang tidak valid tidak mengganti config aktif, dan error menunjuk field atau baris yang relevan; field managed penting tidak bisa ditimpa override tanpa validasi; drift tampil sebagai peringatan; form typed dan last known good diuji pada PostgreSQL, NATS, dan schema Traefik dengan `settings.routes` berbentuk daftar, tanpa patch SwiftUI; rename tidak memindahkan data atau mengubah ID; ganti port fixed melewati validasi konflik, menampilkan dampak restart, dan tercatat sebagai operasi ganti port (aturan untuk instance yang dipinjam environment menyusul di fitur 47); `stackly instance config get`, `validate`, dan `apply` setara dengan GUI; layar Configuration mengikuti mockup UI: mode Form dan Effective File, revision, label dampak, Revert, tombol Apply and Reload atau Apply and Restart, field managed yang terkunci, Pending Changes, History, dan banner drift yang menunggu pilihan eksplisit pengguna.

**Trace:** REQ-010, REQ-015, REQ-028, PAGE-06, VAL-012, BR-003, BR-017, AC-015, AC-026, AC-045, RISK-07, PRD bagian 7.1

- [ ] Design it (spec): `/architect edit config, nama & endpoint`

### 30. Reconciliation & restart daemon terencana · needs a decision · GA

Hardening rekonsiliasi dasar dari fitur 10 untuk instance nyata: setelah daemon restart atau crash, rekonsiliasi proses, runtime, endpoint, storage, operasi, dan desired state, plus restart daemon terencana dengan jendela disruption yang terlihat.

**Done when:** menutup dan membuka GUI tidak mengubah PID atau identitas instance; setelah daemon crash tidak ada instance ganda atau data yang terhapus; PID yang kini milik executable lain tidak dibunuh, dan instance dilaporkan unknown beserta buktinya; `stackly daemon restart` terencana menghentikan instance terdampak dengan aman selama handover supervisor belum terbukti, memulihkan desired state sesudahnya, dan GUI serta CLI menampilkan jendela disruption, bukan klaim zero downtime; inventory tersedia dalam target NFR-007.

**Trace:** REQ-001, REQ-013, AC-001, AC-013, AC-067, NFR-007, OQ-007, RISK-03, PRD bagian 19.4 dan 36

- [ ] Design it (spec): `/architect reconciliation & restart daemon terencana`

### 31. Demand, autostart & login · needs a decision

Autostart daemon dan autostart instance sebagai dua setting berbeda, di atas model demand dengan tiga sumber (environment, manual, startup) dan suppression. Aturan evaluasi stop manual terhadap seluruh demand aktif diputuskan di sini; lease environment hanya menjadi sumber demand baru di fitur 46 dan 47.

**Done when:** setelah login dan izin tersedia, daemon serta instance terpilih ready; izin yang dicabut terlihat di UI; stop manual tidak langsung dinyalakan lagi oleh reconciler di sesi yang sama; Keychain yang terkunci menghasilkan needs secret, bukan auth kosong; logout mengakhiri sesi dengan perilaku instance yang terdefinisi dan batas disruption yang terdokumentasi, dan login berikutnya memulihkan desired state tanpa instance ganda.

**Trace:** REQ-014, REQ-022 (bagian evaluasi demand), BR-024, AC-014, AC-062, AC-069, PRD bagian 13.2, 17, 19.5, dan 32.2

- [ ] Design it (spec): `/architect demand, autostart & login`

### 32. Menu bar & Settings · needs a decision

Status ringkas di menu bar dengan aksi buka aplikasi, **Quit App** yang hanya menutup GUI, dan **Stop Services** sebagai aksi terpisah dengan impact preview. Halaman Settings untuk background startup, default autostart, sumber catalog dan daftar trust provider (import lokal hadir di fitur 37), path data, cache, dan log, retention, update channel (disimpan sebagai preferensi sampai fitur 61), dan privasi, plus notifikasi opsional yang mengikuti izin.

**Done when:** Quit App tidak menghentikan service; Stop Services menampilkan daftar instance yang akan berhenti berdasarkan demand aktif (fitur 31) sebelum berjalan; mengubah path data, cache, atau log hanya berlaku untuk instance baru dan tidak memindahkan data yang ada; Settings menjelaskan perilaku autostart pada login berikutnya; notifikasi bisa dimatikan dan hanya muncul setelah izin notifikasi macOS diberikan; telemetry mati secara default; popover menu bar dan jendela Settings (tab General, Instances, Sources, Storage, Updates) mengikuti mockup UI; Appearance di Settings mengikuti sistem secara default dengan pilihan Light atau Dark; baris setting milik fitur yang belum dibangun tidak tampil seolah sudah bekerja.

**Trace:** REQ-027 (menu bar, notifikasi), REQ-046 (privasi), PAGE-14 (bagian yang tersedia di fase 04), NFR-016

- [ ] Design it (spec): `/architect menu bar & Settings`

### 70. Buka instance di aplikasi klien · needs a decision

Menu aksi per service di halaman Environments, seperti di mockup UI, untuk membuka instance di aplikasi lain: klien database (mockup mencontohkan TablePlus, Postico, Sequel Ace, Redis Insight), CLI provider di Terminal, monitoring atau dashboard di browser, dan file routes Traefik di Finder. Tambahan dari mockup UI, belum ada di PRD.

**Done when:** daftar aksi berasal dari capability provider, bukan kode SwiftUI per provider; aplikasi yang tidak terpasang tidak ditawarkan; aksi nonaktif saat instance tidak running; password tidak pernah dititipkan lewat URL, argumen command, atau daftar proses, dan kalau sebuah aplikasi hanya menerima secret lewat cara itu, alurnya meminta aksi eksplisit dengan peringatan; CLI di Terminal memakai binary dari runtime instance itu, bukan dari PATH.

**Trace:** REQ-016, REQ-027, REQ-029, BR-018

- [ ] Design it (spec): `/architect buka instance di aplikasi klien`
