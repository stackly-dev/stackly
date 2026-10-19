# 0023. Rationale: provider NATS lewat manifest

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: Done when fitur 23 meminta NATS berjalan "tanpa kode Rust atau Swift khusus NATS", sedangkan deskripsi fitur yang sama mengizinkan "probe `nats-readiness` sebagai satu perubahan core yang ditinjau". Keduanya cocok hanya kalau ukuran RISK-07 dibaca jujur: yang dihitung adalah setiap perubahan core, bukan hanya baris yang memuat kata NATS. Spec ini karena itu mencatat dua perubahan terpisah, perluasan generik jenis `native` (yang sudah direncanakan spec 0009 dan akan dipakai probe berikutnya) dan satu perubahan khusus NATS (descriptor dan runner `nats-readiness`), dan membuat AC-14 memeriksa bahwa tidak ada perubahan lain. Scope juga menulis REQ-036 sebagai "dasar": credential, dua instance yang terisolasi, dan persistence stream milik fitur 39, jadi instance NATS di fitur ini berjalan tanpa auth di loopback, dan itu dinyatakan sebagai residual risk, bukan disembunyikan.

NATS adalah provider nyata kedua Stackly. Tujuannya bukan NATS lengkap, tetapi bukti bahwa engine provider (spec 0009), supervisor (spec 0010), install (spec 0018), catalog (spec 0017), dan form create (spec 0020) tidak hanya cocok untuk PostgreSQL (RISK-07, PRD bagian 33.2: "NATS/Traefik digunakan lebih awal untuk membuktikan engine tidak hanya cocok bagi PostgreSQL"). Paket `providers/nats` sudah dibuat fitur 9 dengan manifest, settings schema, template `natsConf`, dan `versions.json` NATS 2.15.1, tetapi readiness nya sementara memakai probe `http` ke `/healthz`.

PRD bagian 22.2 menuntut readiness yang mencakup konektivitas dan mode JetStream yang diminta, "bukan sekadar HTTP 200 dari proses yang salah", serta monitoring yang tidak terbuka ke LAN, `server_name` unik, dan `store_dir` per instance. PRD 21.2 menyebut `nats-readiness` sebagai primitive yang harus benar benar diimplementasikan. `/healthz` hanya menjawab status HTTP, tanpa identitas server. Artefak NATS resmi (`nats-server-v2.15.1-darwin-arm64.tar.gz`) hanya berisi `nats-server`, tanpa CLI client, dan `nats-server` tidak punya subcommand untuk memeriksa server lain, jadi probe `exec` tidak punya program untuk dijalankan tanpa menambah artefak baru ke inventory.

Kekuatan yang membatasi desain: spec 0009 sudah menulis kontrak probe native (`type: native` dengan `name`, `endpoint`, `params`, registry tertutup `NativeProbeDescriptor`, runner lewat spec 0010, nama tak dikenal ditolak saat validasi), modul `core::provider` harus tetap tanpa IO, core harus lolos `cargo check` untuk Windows, setiap dependency baru harus lolos `cargo deny` dan dicatat di spec 0001, dan probe berjalan setiap 10 detik selama instance hidup (spec 0010) sehingga apa pun yang ia lakukan dibayar terus menerus. Model secret spec 0011 menyelesaikan secret sekali per aksi, bukan setiap 10 detik. Fitur 20 dan 21 belum dibangun, dan spec 0020 ditulis paralel dengan spec ini.

Kalau ini tidak diputuskan, fitur 39 dan provider lain mulai tanpa pola probe native, NATS tetap tampil ready untuk proses apa pun yang menjawab `/healthz` di port monitoring, catalog tetap tanpa provider kedua, dan RISK-07 tetap tanpa angka saat fase 07 dimulai.

## Options considered

### Keputusan 1: apa yang diperiksa `nats-readiness` dan lewat endpoint mana

#### Option 1: Protokol client di endpoint `client`: INFO, CONNECT dan PING, lalu `$JS.API.INFO` saat JetStream diminta

Probe membuka koneksi TCP ke endpoint `client`, membaca INFO (identitas `server_name`, flag `jetstream`, `auth_required`, `tls_required`, `ldm`), mengirim CONNECT dan PING lalu menunggu PONG, dan kalau JetStream diminta mengirim satu request `$JS.API.INFO` ke inbox nya sendiri.

**Pros**:

- Memeriksa port yang dipakai aplikasi pengguna, bukan port samping.
- Identitas, mode JetStream, pemrosesan protokol, dan kesiapan API JetStream terbukti dalam satu koneksi pendek.
- Hanya butuh satu endpoint, cocok dengan kontrak `endpoint` tunggal spec 0009.
- Tidak membaca body HTTP, jadi client HTTP spec 0010 tetap tanpa pembaca body.

**Cons**:

- Kode protokol NATS (sekitar lima jenis baris) di core yang harus dirawat dan di fuzz.
- Satu koneksi client setiap 10 detik terlihat di monitoring NATS.

#### Option 2: Endpoint `monitoring`: `/varz` untuk identitas dan mode, `/healthz` untuk kesiapan

Dua request HTTP ke endpoint monitoring, membaca JSON `/varz`.

**Pros**:

- Tanpa protokol client NATS; JSON monitoring terdokumentasi.
- `/healthz` sudah memeriksa kesehatan JetStream upstream.

**Cons**:

- Port client yang dipakai aplikasi tidak pernah diperiksa; listener client yang macet tetap tampil ready.
- Client HTTP spec 0010 harus membaca dan mem parse body, permukaan baru untuk input yang tidak dipercaya.
- Dua request per percobaan.

#### Option 3: INFO saja

Connect, baca INFO, periksa identitas dan flag JetStream, tutup.

**Pros**:

- Paling sederhana; kode protokol paling sedikit.

**Cons**:

- INFO dikirim server saat menerima koneksi, sebelum client apa pun diproses, jadi server yang bisa menerima tetapi tidak memproses perintah tetap tampil ready.
- Flag `jetstream` hanya menyatakan JetStream diaktifkan, bukan bahwa API nya menjawab (store yang sedang dipulihkan atau dimatikan karena disk penuh).

#### Option 4: Probe `http` generik dengan ekspektasi field JSON

Perluas probe `http` dengan aturan seperti "field `server_name` sama dengan `${settings.serverName}`" dan ekspektasi bersyarat untuk JetStream.

**Pros**:

- Tidak ada kode khusus NATS sama sekali; provider lain bisa memakainya.

**Cons**:

- Bahasa aturan di manifest yang terus tumbuh (JSON pointer, kondisi berdasarkan settings), persis yang ditolak spec 0009 Keputusan 3 karena menambah RISK-08.
- Tetap memeriksa port monitoring, bukan port client, dengan kekurangan Option 2.

### Keputusan 2: cara probe mendapat nilai harapan per instance

#### Option 1: `params` menerima literal atau token interpolasi, diselesaikan plan menjadi param bertipe

`serverName: "${settings.serverName}"` dan `jetstream: "${settings.jetstream}"`; plan builder menghasilkan `NativeProbe::NatsReadiness { server_name, jetstream }`.

**Pros**:

- Nilai harapan datang dari ekspresi yang sama dengan nilai yang dirender ke `nats.conf`, di plan yang sama.
- Runner tetap fungsi murni dari plan, tanpa akses registry atau file.
- Aturan interpolasi yang sudah ada (tipe token, `secret_not_allowed_here`, `apply: reload`) dipakai ulang.

**Cons**:

- Menambah satu baris di matriks posisi interpolasi spec 0009 dan menyimpang dari kalimat kontrak "params divalidasi seperti item settings" (yang berbunyi literal).

#### Option 2: `params` literal saja; runner membaca harapan dari config yang dirender

Runner membuka `nats.conf` di folder config untuk tahu `server_name` dan blok `jetstream`.

**Pros**:

- Kontrak spec 0009 dipakai apa adanya.

**Cons**:

- Runner membaca file dan mem parse format config NATS, jauh lebih banyak kode khusus NATS.
- Config yang diubah tangan di antara start dan probe mengubah harapan diam diam.

#### Option 3: Tanpa `params`; runner menurunkan harapan dari `PlanContext`

Core tahu bahwa harapan NATS adalah `instance.name` dan field settings bernama `jetstream`.

**Pros**:

- Manifest paling pendek.

**Cons**:

- Core terikat ke nama field settings paket tertentu; mengganti nama field di paket diam diam merusak probe, dan itu kode khusus NATS yang tersembunyi.

### Keputusan 3: cara memilih runner untuk nama probe native

#### Option 1: Enum tertutup `NativeProbeKind` dan `NativeProbe` dengan `match` exhaustive

Descriptor, konversi param, dan runner masing masing dipilih lewat `match` atas enum.

**Pros**:

- Compiler menjamin setiap varian punya descriptor dan runner, jadi "nama yang lolos validasi selalu bisa dijalankan" (janji spec 0009) dijaga tanpa test tambahan.
- Tanpa alokasi dinamis atau tabel yang bisa tidak sinkron.

**Cons**:

- Menambah probe berarti menyentuh beberapa `match` di dua modul (dan itu memang perubahan core yang ditinjau).
- Menyimpang dari kata "trait" di kontrak spec 0009.

#### Option 2: Trait `NativeProbeRunner` dan registry objek trait

Setiap probe adalah struct yang mengimplementasikan trait, didaftarkan di tabel nama ke `Box<dyn NativeProbeRunner>`.

**Pros**:

- Sesuai kata kontrak spec 0009; menambah probe hanya menambah satu implementasi dan satu baris tabel.

**Cons**:

- Tabel dan descriptor bisa tidak sinkron; butuh test untuk menjamin setiap nama punya runner.
- Param harus dibawa sebagai peta generik lalu didecode ulang di runner, dengan error runtime untuk param yang hilang.

#### Option 3: Peta param generik di plan, runner dicari berdasarkan nama saat runtime

Plan membawa `name` dan `Vec<(String, Value)>`.

**Pros**:

- Plan generik; golden tidak berubah bentuk per probe.

**Cons**:

- Kesalahan nama atau tipe param baru muncul saat probe berjalan, bukan saat kompilasi atau validasi.

### Keputusan 4: cara menulis kode protokol

#### Option 1: State machine tanpa IO buatan sendiri dengan pembungkus tokio tipis

`protocol::Exchange` menerima byte dan mengembalikan byte untuk ditulis atau hasil akhir; pembungkus async hanya memindahkan byte dan menjaga deadline.

**Pros**:

- Seluruh logika bisa di test dan di fuzz tanpa socket, dengan potongan byte acak.
- Batas baca dan sanitasi berada di satu tempat yang di fuzz.
- Tidak ada dependency baru di core; tokio `net` sudah masuk lewat spec 0010.

**Cons**:

- Kode protokol sendiri yang bisa tertinggal dari perubahan upstream.

#### Option 2: Crate client resmi `async-nats`

Pakai `async_nats::connect`, `server_info()`, dan `jetstream::Context::query_account`.

**Pros**:

- Client resmi yang dirawat upstream, mencakup JetStream dan auth untuk fitur 39.

**Cons**:

- Pohon dependency besar masuk ke core (TLS, nkeys, dan lainnya) untuk lima baris protokol, dengan biaya `cargo deny`, waktu kompilasi, dan permukaan serangan di daemon.
- Client melakukan reconnect, buffering, dan task latar, yang harus dimatikan atau diakali supaya satu percobaan punya satu deadline dan tidak meninggalkan apa pun.
- Byte INFO mentah dan batas ukuran baca tidak dikendalikan Stackly.

#### Option 3: Probe `exec` dengan CLI `nats`

Tambah artefak CLI `nats` ke inventory dan jalankan `nats server check` lewat probe `exec`.

**Pros**:

- Tanpa kode Rust khusus NATS.

**Cons**:

- Artefak, lisensi, spike, dan pin baru di inventory untuk setiap versi; proses baru setiap 10 detik per instance; output CLI sebagai kontrak yang bisa berubah antar versi; dan cek identitas tetap harus diekspresikan lewat argumen.

### Keputusan 5: auth NATS di fitur ini

#### Option 1: Tanpa auth; credential dan perluasan probe milik fitur 39

**Pros**:

- Sesuai "REQ-036 (dasar)" di scope; probe tidak menyentuh secret, jadi tidak ada pembacaan Keychain setiap 10 detik.
- Sama dengan default upstream `nats-server` untuk pengembangan lokal.

**Cons**:

- Proses lokal mana pun bisa terhubung ke instance di loopback sampai fitur 39.

#### Option 2: User dan password aplikasi lewat secret, plus user readiness dengan password tetap yang bukan secret

**Pros**:

- Instance langsung terlindungi untuk aplikasi.

**Cons**:

- Mendahului desain fitur 39 (user, permission, nkeys, akun), dan user readiness dengan password tetap adalah kebijakan auth yang belum dipilih siapa pun.
- Menambah template `sensitive`, secret field, dan alur create yang belum diuji untuk NATS.

#### Option 3: Token auth dengan probe yang membaca secret di setiap percobaan

**Pros**:

- Satu credential untuk aplikasi dan probe.

**Cons**:

- Bertentangan dengan model resolve per aksi spec 0011 (pembacaan Keychain setiap 10 detik atau secret di memori daemon tanpa batas waktu), alasan yang sama dengan spec 0019 menolak password untuk role readiness.

### Keputusan 6: cara mengukur RISK-07

#### Option 1: Tabel perubahan di spec plus pemeriksaan daftar file setiap commit build

**Pros**:

- Murah, jujur, dan bisa diperiksa `/check verify`; tabel menjadi pembanding untuk provider berikutnya.

**Cons**:

- Pemeriksaan manual per fitur, bukan aturan otomatis yang menjaga masa depan.

#### Option 2: Aturan baru di `cargo xtask check-deps` yang melarang nama provider di kode non test

**Pros**:

- Otomatis dan terus berlaku.

**Cons**:

- Butuh allowlist untuk `natsConf`, `postgresqlConf`, resep spike, dan setiap probe native, sehingga aturannya lebih banyak pengecualian daripada larangan; nama software juga muncul sah di tempat lain.

#### Option 3: Tidak diukur selain menyebutnya di Consequences

**Pros**:

- Tanpa kerja tambahan.

**Cons**:

- Scope Done when meminta perubahan core "dicatat sebagai ukuran RISK-07"; tanpa pemeriksaan, angkanya hanya klaim.

### Keputusan 7: cara membuktikan form GUI dari settings schema

#### Option 1: Fixture form dari paket NATS asli dengan test kontrak Rust dan Swift, ditambah pemeriksaan GUI manual

**Pros**:

- Bukti otomatis bahwa model form generik menangani schema provider nyata (field managed, toggle, advanced, dua endpoint), mengikuti aturan proyek bahwa fixture adalah kontrak bersama.
- Pemeriksaan manual menutup keyboard dan VoiceOver yang tidak bisa dibuktikan fixture.

**Cons**:

- Bergantung pada bentuk DTO fitur 20 yang belum dibangun.

#### Option 2: Pemeriksaan GUI manual saja

**Pros**:

- Tanpa kode test baru.

**Cons**:

- Tidak ada pelindung regresi saat form atau schema NATS berubah.

#### Option 3: UI test XCUITest yang mengisi sheet New Instance

**Pros**:

- Bukti ujung ke ujung otomatis dari GUI.

**Cons**:

- Butuh daemon dan runtime NATS asli di sesi GUI, jauh lebih rapuh dan lambat, dan proyek belum punya infrastruktur UI test.

## Rationale

**Protokol client penuh (Keputusan 1)**: PRD 22.2 menulis kegagalan yang harus dicegah dengan jelas, yaitu HTTP 200 dari proses yang salah dan mode JetStream yang tidak sesuai. Hanya protokol client yang memeriksa port yang dipakai aplikasi, identitas server, dan pemrosesan perintah sekaligus, dan request `$JS.API.INFO` adalah satu satunya cara murah untuk tahu API JetStream benar benar menjawab, bukan hanya diaktifkan. Biayanya kode protokol kecil, yang dijawab Keputusan 4 dengan state machine yang di fuzz. Option 4 menggoda karena tidak butuh kode khusus NATS, tetapi ia memindahkan kerumitan yang sama ke bahasa aturan manifest, yang spec 0009 sengaja tolak.

**Param bertoken dan enum tertutup (Keputusan 2 dan 3)**: kontrak spec 0009 ditulis sebelum ada probe nyata, dan probe nyata pertama langsung menunjukkan bahwa harapan identitas dan mode berbeda per instance, jadi param literal tidak cukup. Token di `params` memakai mesin interpolasi dan aturan keamanan yang sudah ada, dan nilai harapan otomatis sama dengan nilai yang dirender. Untuk registry, janji spec 0009 adalah "dikenal berarti bisa dijalankan"; `match` exhaustive membuat compiler yang menjaganya, sedangkan trait dan tabel butuh test untuk janji yang sama. Keduanya dicatat sebagai usulan additive untuk spec 0009, bukan perubahan diam diam.

**Tanpa auth dan RISK-07 yang terukur (Keputusan 5 dan 6)**: fitur 23 adalah pembuktian engine, bukan provider NATS lengkap; menambah auth sekarang berarti memutuskan kebijakan user dan permission NATS untuk fitur 39 sebelum desainnya ada, dan auth untuk probe akan bertabrakan dengan model secret spec 0011. Residual risk nya dinyatakan di spec, README, dan `knownLimitations`. Untuk RISK-07, tabel plus pemeriksaan diff per commit memberi angka yang bisa diverifikasi tanpa aturan otomatis yang penuh pengecualian.

## Keputusan kecil

Diputuskan saat menulis spec, masing masing dengan pilihan yang direkomendasikan:

- **Kind param hanya `text` dan `boolean`**, tanpa param opsional, karena hanya itu yang dipakai `nats-readiness`; kind lain ditambah probe yang butuh (prinsip spec 0009: tidak ada kode tanpa pemakai). Runner up: `integer` sekarang.
- **Kode issue yang sudah ada**, tanpa kode baru untuk probe native, supaya katalog kode `manifest` dan protokol tidak berubah. Runner up: `unknown_native_probe` dan `invalid_probe_param`.
- **Kode reason generik** (`connect_failed`, `timeout`, `protocol_error`, `identity_mismatch`, dan lainnya), tanpa awalan `nats_`, supaya GUI dan probe native berikutnya bisa memakai ulang. Runner up: kode berawalan nama probe.
- **Urutan cek INFO**: identitas lebih dulu, karena server asing yang menuntut auth lebih jelas dilaporkan sebagai server lain daripada sebagai masalah auth.
- **`type` balasan API JetStream diperiksa**, supaya balasan yang bukan account info tidak dihitung sukses. Runner up: sukses untuk setiap object tanpa `error`.
- **Inbox acak per percobaan** (`_INBOX.stackly.<32 hex>`), walau satu probe per instance pada satu waktu, supaya client pengguna yang kebetulan memakai inbox yang sama tidak bertabrakan. Runner up: inbox tetap.
- **`protocol: 1`, `echo: false`, `headers: false`, `no_responders: false`, `verbose: false`** di CONNECT: tanpa `+OK` per perintah, tanpa pesan sendiri yang kembali, dan tanpa header; nama client `stackly-readiness` supaya terlihat di monitoring (seperti `PGAPPNAME` di spec 0019).
- **Tanpa pelacakan `server_id` antar percobaan** (server id baru menandakan proses lain), karena identitas proses sudah tugas spec 0010 dan probe tetap tanpa state. Runner up: probe yang menyimpan `server_id` pertama.
- **Detail tanpa `Redactor`**, karena tidak ada secret di jalur ini; fitur 39 meninjau ulang saat auth masuk.
- **Batas baca 16 KiB per baris, 64 KiB per payload, 128 KiB total**: INFO server tunggal sekitar 600 byte dan account info sekitar 500 byte, jadi batas ini longgar untuk server sah dan kecil untuk server jahat.
- **Interval, timeout percobaan, dan timeout lain tetap default**, dan `health` tidak ditulis, karena NATS siap dalam ratusan milidetik dan belum ada ukuran yang menuntut nilai lain. Runner up: `recoverySeconds` 120 untuk store JetStream besar.
- **`metadata.revision` tetap `1.0.0`**, karena NATS belum pernah terbit di catalog (selama ini `planned`); `manifest_sha256` tetap membedakan isi. Runner up: `1.1.0`.
- **Settings NATS tidak berubah** (batas dalam byte), supaya fitur ini hanya mengubah probe dan angka RISK-07 tetap bersih; satuan MiB adalah Follow-up setelah suffix ukuran `nats.conf` dibuktikan. Runner up: field MiB dengan suffix `MB` di template.
- **Escaper `natsConf` tetap konservatif** (spec 0009 mengizinkan fitur 23 melonggarkannya): nilai yang dirender NATS (nama instance, alamat loopback, path data) tidak butuh `"`, `\`, atau `$`, dan AC-8 membuktikan path dengan spasi dan Unicode di bawah `TestRoot` terbaca NATS. Runner up: escape `\"` dan `\\` dengan test binary asli.
- **Fuzz target sendiri** untuk state machine, karena ia mem parse byte dari socket yang tidak dipercaya (NFR-009), dengan seed dari transcript asli spike langkah 0.
- **Spike lokal sebagai langkah 0**, karena sesi architect tidak menjalankan binary NATS; nama field INFO, balasan API, dan `/varz` di spec ini berasal dari dokumentasi protokol NATS dan harus dibuktikan pada 2.15.1 sebelum runner dibangun.
- **Catatan platform `linux` dan `windows` `planned`** di entri catalog, karena keduanya ada di Deferred proyek dan upstream punya build nya; tanpa catatan, catalog menulis `unsupported` `no_build`, yang terdengar final. Runner up: hanya `windows` seperti contoh spec 0017.
- **Test native di `apps/daemon/tests/nats.rs` dan test journey di `apps/cli/tests/nats_journey.rs`**, mengikuti pola `postgresql.rs` spec 0019; runner diuji terhadap server asing di file daemon yang sama karena binary nya sudah terpasang di sana.
- **`pin_package` dipakai bersama dengan spec 0019**, dibuat oleh fitur mana pun yang dibangun lebih dulu dengan signature spec 0019.
- **`SIGSTOP` untuk membuktikan readiness jujur**, karena kernel tetap menyelesaikan connect TCP lewat backlog sementara server tidak menjawab apa pun; ini persis kasus "port terbuka bukan berarti ready". Runner up: lame duck lewat `SIGUSR2`, yang menutup listener sehingga connect gagal dan tidak membedakan probe ini dari probe `tcp`.
- **Tidak ada pencarian Agent Skill atau MCP**: fitur ini tidak memilih tool atau framework baru.

## Pertukaran dengan sesi lain

Belum ada.

## Cross check

Belum dijalankan.
