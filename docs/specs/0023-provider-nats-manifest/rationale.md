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

#### Option 1: State machine tanpa IO buatan sendiri dengan driver tokio generik

`provider::probe::nats::Exchange` menerima byte dan mengembalikan byte untuk ditulis atau hasil akhir; driver generik di `supervisor::probe::native` hanya memindahkan byte antara stream dan state machine dan menjaga deadline.

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
- **Detail tanpa `Redactor`**, karena tidak ada secret di jalur ini; fitur 39 meninjau ulang saat auth masuk. *Diganti 2026-10-20 (K3)*: runner spec 0010 menyamarkan setiap detail probe, termasuk probe native, lewat `Redactor`.
- **Batas baca 16 KiB per baris, 64 KiB per payload, 128 KiB total**: INFO server tunggal sekitar 600 byte dan account info sekitar 500 byte, jadi batas ini longgar untuk server sah dan kecil untuk server jahat.
- **Interval, timeout percobaan, dan timeout lain tetap default**, dan `health` tidak ditulis, karena NATS siap dalam ratusan milidetik dan belum ada ukuran yang menuntut nilai lain. Runner up: `recoverySeconds` 120 untuk store JetStream besar.
- **`metadata.revision` tetap `1.0.0`**, karena NATS belum pernah terbit di catalog (selama ini `planned`); `manifest_sha256` tetap membedakan isi. Runner up: `1.1.0`.
- **Settings NATS tidak berubah** (batas dalam byte), supaya fitur ini hanya mengubah probe dan angka RISK-07 tetap bersih; satuan MiB adalah Follow-up setelah suffix ukuran `nats.conf` dibuktikan. Runner up: field MiB dengan suffix `MB` di template.
- **Escaper `natsConf` tetap konservatif** (spec 0009 mengizinkan fitur 23 melonggarkannya): nilai yang dirender NATS (nama instance, alamat loopback, path data) tidak butuh `"`, `\`, atau `$`, dan AC-8 membuktikan path dengan spasi dan Unicode di bawah `TestRoot` terbaca NATS. Runner up: escape `\"` dan `\\` dengan test binary asli.
- **Fuzz target sendiri** untuk state machine, karena ia mem parse byte dari socket yang tidak dipercaya (NFR-009), dengan seed dari transcript asli spike langkah 0.
- **Spike lokal sebagai langkah 0**, karena sesi architect tidak menjalankan binary NATS; nama field INFO, balasan API, dan `/varz` di spec ini berasal dari dokumentasi protokol NATS dan harus dibuktikan pada 2.15.1 sebelum runner dibangun.
- **Catatan platform `linux` dan `windows` `planned`** di entri catalog, karena keduanya ada di Deferred proyek dan upstream punya build nya; tanpa catatan, catalog menulis `unsupported` `no_build`, yang terdengar final. Runner up: hanya `windows` seperti contoh spec 0017.
- **Test native di `apps/daemon/tests/nats.rs` dan test journey di `apps/cli/tests/nats_journey.rs`**, mengikuti pola `postgresql.rs` spec 0019; runner diuji terhadap server asing di file daemon yang sama karena binary nya sudah terpasang di sana.
- **`pin_package` dipakai bersama dengan spec 0019**, dibuat oleh fitur mana pun yang dibangun lebih dulu dengan signature spec 0019. *Diganti 2026-10-20 (K5)*: `pin_package` dan `DaemonConfig.extra_log_sinks` dibangun spec 0010 langkah 13.
- **`SIGSTOP` untuk membuktikan readiness jujur**, karena kernel tetap menyelesaikan connect TCP lewat backlog sementara server tidak menjawab apa pun; ini persis kasus "port terbuka bukan berarti ready". Runner up: lame duck lewat `SIGUSR2`, yang menutup listener sehingga connect gagal dan tidak membedakan probe ini dari probe `tcp`.
- **`nats-readiness` di blok `readiness`, health mewarisinya**, berbeda dengan contoh PRD 21.2 yang menaruhnya di `health` (`type: providerProbe`): di v1alpha1 readiness wajib dan health tanpa probe memakai probe readiness, jadi satu deklarasi melayani keduanya dengan satu percobaan per 10 detik. Runner up: deklarasi terpisah di `health`.
- **State machine di `provider::probe::nats`, driver di `supervisor::probe::native`**, supaya state machine dan fuzz nya bisa dibangun tanpa menunggu supervisor fitur 10 (cross check S4), dan supaya kode supervisor hanya menambah satu cabang `match` yang menyebut NATS.
- **Driver menerima stream dari fungsi connect**, supaya test timing memakai `tokio::io::duplex` di bawah waktu tokio yang dijeda; socket asli dengan waktu dijeda bisa melompati deadline saat runtime menunggu IO loopback (cross check S1).
- **Langkah `$JS.API.INFO` dipertahankan** (keputusan lead 2026-10-20 atas S2): tiga fakta startup NATS yang membuatnya mungkin berlebihan belum terbukti, langkahnya murah, dan ia tetap benar kalau JetStream lambat pulih atau dimatikan saat berjalan. Langkah 0 mencatat faktanya tanpa membuka ulang keputusan ini. Runner up: INFO dan PING saja, dengan cek API JetStream ditunda ke fitur 39.
- **`lame_duck` hanya diuji di fixture**: NATS yang masuk lame duck menutup listener, jadi koneksi baru gagal connect dan tidak pernah melihat `ldm`; kodenya tetap ada untuk server yang mengirimnya.
- **Satu test `nats_compatibility` yang berurutan** menulis `compatibility.json`, karena beberapa fungsi `#[test]` tidak bisa mengisi satu file evidence, dan check hanya boleh tercatat setelah assertion nya lulus. Test ini memakai `SpawnedDaemon` dengan interval default, jadi tidak butuh seam interval di `DaemonConfig`; hanya AC-8 yang memakai daemon in process karena butuh `LogSink` perekam.
- **Versi evidence dari `/varz` server yang berjalan**, mengikuti aturan spec 0002 dan perbaikan spec 0019 (versi dari binary yang berjalan, bukan dari record artefak).
- **AC-14 memeriksa baris yang ditambah per commit**, bukan seluruh file, karena `validate.rs` sudah memuat `natsConf` dari spec 0009 dan file itu tetap harus disentuh untuk jenis `native`.
- **Tidak ada pencarian Agent Skill atau MCP**: fitur ini tidak memilih tool atau framework baru.

## Pertukaran dengan sesi lain

- **`arc-f20-spec` (2026-10-20)**: sesi itu mengabarkan kontrak form create spec 0020 (form dari paket lewat `instance.get_create_form`, lima kind field, preflight dengan dry run plan, revision catalog yang dipin saat create, dan fixture `create-form-nats.json`). Spec ini menjawab bahwa NATS tidak butuh kind lain atau `socketName` bertoken port, bahwa perubahan fitur 23 tidak mengubah fixture itu, dan bahwa AC-13 memakai fixture dan test render milik spec 0020 alih alih membuat fixture sendiri. Keputusan 7 tetap Option 1; pembuatnya saja yang pindah ke fitur 20.

## Cross check

Sonnet 5.5 membaca draf pertama secara read only pada 2026-10-20 (tanpa menjalankan binary dan tanpa mengambil link). Lead menyetujui semua perbaikan yang direkomendasikan atas nama user, dengan S2 memakai pilihan "langkah `$JS.API.INFO` dipertahankan". Semuanya sudah masuk ke spec:

- **Kelengkapan keputusan**: allowlist AC-14 yang tidak bisa dipenuhi (nama NATS dipindah ke `provider::probe`, AC-14 memeriksa baris yang ditambah per commit dengan commit dasar yang dicatat); seam test yang belum bernama (kait `LogSink`, `pin_package`, `SpawnedDaemon` untuk adopsi, baris 4 RISK-07); `ProbeOutcome` dan reason sukses `probe_ok`; angka health di AC-9 dipindah ke AC-6; argv server asing (nama `golden` untuk server auth, `-a 127.0.0.1`, `-sd` di `TestRoot`, port dari `allocate_port`, tanpa monitoring); urutan evidence, `compat-record`, dan commit catalog, satu test yang menulis `compatibility.json`, dan langkah 1 spec 0018 sebagai prasyarat; root data Unicode lewat `RootSet::under_override`; status exit diganti PID hilang, `lastFailure` null, dan `leftoverProcesses` kosong; urutan readiness lewat `statusSeq`; INFO dengan tipe salah, bukan object, atau byte sesudahnya menjadi `protocol_error`; teks timeout per fase, reset dan EOF, aturan potong 61 ditambah `...`, dan target Unix; encoding input fuzz dan run ulang dua fuzz target lama; `covers` REQ-012 dan REQ-013 serta guard `SIGCONT`; dan catatan penempatan probe dibanding PRD 21.2.
- **Kesehatan desain**: test timing lewat `tokio::io::duplex` (S1); langkah `$JS.API.INFO` dipertahankan dengan fakta startup dicatat di langkah 0 (S2); `lame_duck` hanya di fixture (S3); state machine dan fuzz di `provider::probe` supaya tidak menunggu fitur 10 (S4); baris 4 RISK-07 dan `git diff --numstat` (S5); `multi_instance_not_covered` di `knownLimitations` (S6).

Setelah cross check, cabang ini di merge dengan `main` yang membawa perbaikan konsistensi spec 0015, 0017, 0018, dan 0019, dan setiap nama yang dipakai spec ini dicek ulang: signature `install_cached_artifact` (sekarang dengan `&PreparedLayout` dan record yang di commit sendiri), aturan `runtime.versionCheck` per baris, `compat-record` yang hanya menyalin record BR-025, versi evidence dari binary yang berjalan, `pin_package` dan kait `LogSink` di langkah 6 spec 0019, `CatalogBuilder` di `stackly_core::catalog::testing`, serta `published_at` dan ledger `providers/catalog-revisions.json`.

## Update dari spec 0021 (2026-10-20)

Diterapkan sesi `upd-0019-0022-0023` dari *Usulan update spec lain* spec 0021 (start, readiness, dan connection), atas persetujuan lead untuk user:

- *Paket NATS* dan AC-4: paket mendapat blok `connection` dengan entri `uri` (`nats://` lewat endpoint `client`, tanpa credential sampai fitur 39) dan `monitoring` (`http://` lewat endpoint `monitoring`), ditulis di commit yang sama dengan blok `readiness`, supaya `manifest_sha256` hanya berubah sekali sebelum evidence BR-025. Blok ini menjawab Follow-up "Fitur 21: connection string NATS dan URL monitoring".
- Prasyarat build plan: langkah 2 butuh loader blok `connection` (langkah 1 spec 0021), dan evidence langkah 9 dibuat dari `provider.yaml` yang sudah memuat blok itu.

ID AC dan status spec ini tidak berubah.


## Penyelarasan 2026-10-20 (keputusan K foundation)

Update `architect (update)` atas permintaan lead scope 02, setelah keputusan K1 sampai K18 lead foundation dan update spec foundation masuk ke `main` (0001 sampai 0006, 0009, 0010, 0011, dan 0012). Sumber nya butir *0023 (provider NATS)* di `docs/scope/coordination/02-instalasi-pertama.penyelarasan-k.md`, dicek terhadap spec 0010 dan 0011 di `main`. ID AC, status, dan nomor langkah build tidak berubah.

| No | Perubahan di spec ini | Sumber | Catatan |
| --- | --- | --- | --- |
| 1 | Detail probe native lewat `Redactor` di runner spec 0010, bukan tanpa redaksi | K3, spec 0010 *Hasil runner*, spec 0011 (detail observasi lewat `Redactor`) | keputusan kecil lama ditandai diganti |
| 2 | *Hasil probe dan loop supervisor*: `ProbeOutcome` dan reason milik spec 0010, termasuk `unexpected_status` (`http`) dan `check_failed` (`exec`), yang tidak dipakai probe native; syarat "kalau spec 0010 belum punya" dihapus | K3 | |
| 3 | Arm `ProbeSpec::Native` di match `supervisor::probe` ditambah langkah 1 dengan `native::run` sementara (`// provisional: F23 langkah 5`), dan langkah 5 menggantinya; langkah 1 kini butuh langkah 6 spec 0010 | K3 (spec 0010: "arm match native ditambah langkah 1 spec 0023, dan driver nya dibangun langkah 5 dan 6") | prasyarat milestone 1 berubah; teks scope baru dikirim ke lead |
| 4 | `pin_package` dan `LogSink` perekam lewat `DaemonConfig.extra_log_sinks` dari spec 0010 langkah 13; baris 4 RISK-07 tidak lagi dibangun di sini | K5 | prasyarat milestone 2 menyebut langkah 13 spec 0010 dan tidak lagi spec 0019 langkah 6 |
| 5 | Test create memakai create sementara spec 0010 dengan `provider.revision` dari record `pin_package` | K1 | |
| 6 | Kasus tepi port `Fixed`: listener wildcard pengguna tidak lagi menghalangi start; cek identitas probe yang menjaga | K2 | |
| 7 | Blok *Usulan update spec 0009* dan *Usulan update spec 0010* ditandai diterapkan, Follow-up keduanya dicentang, dan baris Assumed spec 0009, 0010, dan 0019 diselaraskan | spec 0009 dan 0010 di `main` | |

Tidak diterapkan: tidak ada. Keputusan K lain (K4, K6 sampai K18) tidak menyentuh isi spec ini; K18 (milestone GUI fase 02 menunggu fitur 10 milestone 2 sampai fitur 13 langkah 16) berlaku lewat prasyarat fitur 20 dan 21 di milestone 3.
