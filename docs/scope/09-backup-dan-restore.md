# Scope 09 · Backup & restore

[Kembali ke index](index.md)

Journey PRD JOURNEY-06: backup terverifikasi, restore as new, isi ulang secret, cek readiness dan data, lalu pengguna memilih connection baru sementara instance sumber tetap ada. Journey ini dibuktikan dulu ujung ke ujung pada PostgreSQL, lalu diperluas ke lima provider lain. Backup belum dianggap recoverable sebelum restore drill berhasil. Fase ini selesai kalau setiap provider persistent yang diklaim supported bisa dipulihkan sesuai cakupan backup baseline di PRD bagian 24.1, dari GUI dan CLI.

### 48. Backup metadata registry · needs a decision · GA

Snapshot registry yang konsisten saat aplikasi aktif, dengan versi schema, checksum, dan secret reference, memakai mekanisme backup SQLite yang benar. Backup ini tidak diklaim mencakup database provider atau isi Keychain.

**Done when:** registry yang dipulihkan di profil uji konsisten, secret yang hilang ditandai, dan tidak ada instance yang diinisialisasi otomatis di path data yang sudah ada; daemon yang start pada schema setengah teraplikasi menampilkan safe recovery state dengan backup dan bukti recovery yang bisa dipakai restore.

**Trace:** REQ-040, BR-022, AC-040, AC-065, gate G3, PRD bagian 24.1

- [ ] Design it (spec): `/architect backup metadata registry`

### 49. Cold backup & kontrak backup · needs a decision · GA

Primitive cold backup yang dipakai semua provider persistent: maintenance lock, evaluasi lease, stop bersih, verifikasi tidak ada penulis, enumerasi path milik instance, staging, checksum, manifest backup, commit, lalu pemulihan desired state. Kontrak ini juga menetapkan metadata tiap backup: mode konsistensi, scope, identitas runtime, digest, ukuran, status verifikasi, dan status secret material.

**Done when:** cold backup selalu didahului quiesce dan verifikasi bahwa tidak ada proses yang menulis, dan stop memakai jalur yang menghormati lease; instance yang gagal stop bersih tidak menghasilkan backup berlabel consistent; path eksternal yang tidak didukung membuat operasi ditolak; disk penuh di staging tidak menyentuh sumber; raw dump tidak pernah berlabel sanitized; tidak ada auto delete backup; `stackly backup create`, `list`, dan `verify` bekerja.

**Trace:** REQ-028, REQ-041, BR-014, BR-015, BR-022, VAL-016, AC-041, AC-063, AC-064, AC-077, OQ-010, PRD bagian 21.3 dan 24.1

- [ ] Design it (spec): `/architect cold backup & kontrak backup`

### 50. Backup logical PostgreSQL · needs a decision · GA

Backup logical PostgreSQL untuk scope yang dipilih, termasuk penjelasan roles dan global objects, dengan label secret material. Ini jalur backup utama PostgreSQL di baseline; cold backup dari fitur 49 menjadi opsi untuk layout yang tervalidasi.

**Done when:** backup PG17 dan PG18 menyatakan scope dan konsistensinya; roles dan global objects tercakup atau dinyatakan terpisah; backup memakai tool dari runtime yang cocok, bukan dari PATH.

**Trace:** REQ-035 (logical backup), AC-035 (bagian backup), OQ-010, PRD bagian 22.1 dan 24.1

- [ ] Design it (spec): `/architect backup logical PostgreSQL`

### 51. Restore as new & verifikasi · needs a decision · GA

Alur restore yang secara default membuat instance baru dengan endpoint baru: preflight ruang, izin, dan kebutuhan secret, staging milik operasi, lalu start, readiness, dan verifikasi data fixture. Overwrite instance yang sudah ada adalah aksi terpisah.

**Done when:** restore as new tidak mengubah instance sumber; instance baru hasil restore PG17 dan PG18 lulus query fixture sentinel; checksum atau kompatibilitas yang gagal tidak menimpa target; cancel saat staging tidak mengubah target asli; overwrite instance yang sudah ada hanya berjalan setelah preview terpisah, konfirmasi eksplisit, stop aman, dan backup sebelum overwrite yang berhasil; tanggal restore terverifikasi tercatat sebagai bukti; `stackly restore` setara dengan GUI.

**Trace:** REQ-028, REQ-042, BR-021, AC-035 (bagian restore PG17 dan PG18), AC-042, AC-054, AC-064, NFR-008, RISK-13, gate G3, JOURNEY-06, PRD bagian 24.2

- [ ] Design it (spec): `/architect restore as new & verifikasi`

### 52. Halaman Backup & Restore · needs a decision

Halaman Backup & Restore di tab instance: scope dan mode backup, ukuran dan checksum, progress, konsistensi, tanggal restore terverifikasi, restore as new, dan overwrite eksplisit.

**Done when:** semua aksi setara dengan CLI; backup yang belum lulus restore drill tidak ditandai recoverable; alur backup dan restore bisa diselesaikan dengan interaksi desktop; tab Backups mengikuti mockup UI, termasuk state unsupported untuk provider tanpa capability backup dan konfirmasi Overwrite yang menyebut safety snapshot.

**Trace:** PAGE-11, BR-022, NFR-012

- [ ] Design it (spec): `/architect halaman Backup & Restore`

### 53. Backup MySQL, NATS, ClickHouse, Valkey & Traefik · needs a decision · GA

Metode backup untuk lima provider lain sesuai PRD bagian 24.1: MySQL logical dump, NATS JetStream dan ClickHouse cold backup setelah stop bersih, Valkey seluruh persistence set, dan Traefik export config serta route.

**Done when:** Valkey mencadangkan seluruh persistence set yang relevan (termasuk AOF dengan beberapa file), bukan satu nama file; Traefik export config dan route dengan penanganan certificate yang eksplisit; backup native ClickHouse hanya muncul sebagai capability yang sudah diuji; mode volatile Valkey tidak menjanjikan restore; restore as new tiap provider lulus fixture sentinel lewat alur fitur 51.

**Trace:** REQ-041, REQ-055 (bagian backup), AC-077, AC-081 (bagian restore), PRD bagian 22.3, 22.4, 22.6, dan 24.1

- [ ] Design it (spec): `/architect backup MySQL, NATS, ClickHouse, Valkey & Traefik`
