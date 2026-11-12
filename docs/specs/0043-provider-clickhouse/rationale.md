# 0043. Rationale: provider ClickHouse

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: brief fitur ini menyebut format config `xml` sebagai satu perubahan core yang sudah pasti. Itu diperiksa ulang, karena ClickHouse juga membaca config YAML dan format `yaml` sudah ada di engine. Percobaan menunjukkan YAML diterima binary, tetapi template `yaml` Stackly menulis setiap nilai sebagai satu scalar berkutip utuh, jadi path seperti `<data>/user_files/` tidak bisa disusun di dalam file. Jalan keluarnya (memindahkan path ke argv) lebih rapuh daripada menambah satu escaper. Format `xml` tetap dipilih, sekarang dengan alasan yang tercatat, bukan karena diasumsikan.

ClickHouse adalah provider terakhir dari enam provider wajib. Fase 07 meminta tiap provider menjalani journey yang sama dengan PostgreSQL (catalog, install, create, start, readiness, connection, dua instance yang terisolasi) lewat SDK dan harness fase 06, dan mencatat berapa perubahan core yang ia butuhkan sebagai ukuran RISK-07 (schema provider terlalu sempit, sehingga setiap tool tetap butuh patch core).

PRD bagian 22.3 dan REQ-037 menuntut hal yang khas ClickHouse. Semua path (data, temp, log, user files, config user dan akses) harus di dalam instance. Seluruh listener aktif harus diaudit, karena HTTP dan native TCP bukan satu satunya endpoint yang bisa ClickHouse buka (ada protokol MySQL, PostgreSQL, gRPC, interserver, Prometheus, dan Keeper). Cluster, replikasi, dan Keeper tidak boleh menyala diam diam. Readiness harus mencakup query dan identitas endpoint yang benar. ClickHouse CLI resmi tidak boleh menjadi supervisor kedua. AC-037 meminta dua instance dengan tabel bernama sama dan isi berbeda tetap terpisah setelah restart, dengan listener yang cocok dengan registry. AC-074 melarang label native untuk Windows, karena jalur upstream di sana adalah WSL.

Ada tiga kendala dari engine yang ada. Pertama, mekanisme secret spec 0011 menaruh `hash` di urutan kedua dan menyebut ClickHouse sebagai pemakainya, tetapi manifest belum punya cara menulis hash. Kedua, probe `exec` tidak menerima secret, dan probe `http` hanya menilai status, jadi readiness yang ber autentikasi sebagai `default` tidak mungkin tanpa perubahan core. Ketiga, kontrak proses spec 0036 menuntut proses utama tetap foreground, tidak berganti program, dan memegang fd 3.

Artefak `clickhouse-26.8.30.8-macos-aarch64` sudah ada di inventaris dan di cache runtime Mac ini, dan spike spec 0003 sudah `PASS` dengan recipe tulisan tangan. Spec ini tidak memilih artefak; ia mengganti recipe itu dengan paket.

Tanpa keputusan di sini, developer harus menebak bentuk config, cara password dikirim, dan apa arti ready, dan tiga sesi provider lain bisa menulis bentuk filter hash yang berbeda.

## Options considered

### Keputusan 1: format file config

**Option 1: format `xml` baru di core (dipilih).** Satu varian `TemplateFormat` dengan escaper isi XML. Config ditulis dalam bentuk yang dipakai dokumentasi dan pesan error ClickHouse.

- Pros: tag bisa menempel pada teks literal, jadi setiap path disusun di dalam file; seluruh config ada di satu tempat; perubahan core kecil dan sudah diperkirakan spec 0009; tidak ada crate baru.
- Cons: satu perubahan core (masuk hitungan RISK-07); hasil render tidak diparse ulang, jadi template yang bukan XML sah baru ketahuan saat start.

**Option 2: format `yaml` yang ada, path lewat argv.** ClickHouse membaca `config.yaml`, dan percobaan membuktikan file YAML dengan escape `{` diterima. Path yang butuh subfolder dikirim sebagai `-- --tmp_path=...` di `lifecycle.start.args`.

- Pros: nol perubahan format; hasil render diparse ulang oleh engine.
- Cons: config terbelah antara file dan sepuluh argumen; tab Configuration tidak menunjukkan path; setiap key path baru menambah argv; jalur YAML ClickHouse lebih jarang dipakai orang daripada XML.

**Option 3: format `yaml` dengan default ClickHouse.** Hanya `path` yang ditulis; subfolder mengikuti default relatif ClickHouse.

- Pros: paling sedikit teks.
- Cons: lima key lookup tetap menunjuk `/var/lib/clickhouse/` dan `/usr/share/clickhouse/`, di luar instance, yang melanggar REQ-037; file sementara masuk folder data dan ikut backup.

Runner up: Option 2.

### Keputusan 2: cara password sampai ke ClickHouse

**Option 1: hash lewat filter template `sha256Hex` (dipilih).** `users.xml` memuat `password_sha256_hex`; nilainya dihitung saat file ditulis.

- Pros: mekanisme tertinggi yang ClickHouse dukung menurut urutan spec 0011; nilai password tidak pernah di disk; Valkey butuh filter yang sama, jadi satu perubahan melayani dua provider.
- Cons: satu perubahan core bersama; SHA-256 tanpa salt bisa ditebak offline untuk password lemah; ClickHouse menyalin hash ke `data/preprocessed_configs/`.

**Option 2: password polos di config `sensitive`.** `<password>{{ secret.defaultPassword }}</password>` di `users.xml` dalam folder `secrets/`.

- Pros: nol perubahan core.
- Cons: mekanisme terendah (nomor 5); percobaan menunjukkan ClickHouse menyalin `users.xml` ke `data/preprocessed_configs/`, jadi password polos akan masuk folder data, cold backup, dan Time Machine. Itu melanggar aturan "secret hanya di Keychain".

**Option 3: env.** `<password from_env="...">` dengan nilai lewat `env` start.

- Pros: nol perubahan core; tidak ada di disk.
- Cons: mekanisme nomor 4; environment proses terbaca proses lain milik user yang sama selama instance hidup; perilaku `from_env` terhadap salinan `preprocessed_configs` tidak dicoba.

Runner up: Option 3.

### Keputusan 3: readiness

**Option 1: probe `exec` dengan `clickhouse client` dan query identitas (dipilih).** User `stackly_readiness` tanpa password dan tanpa hak menjalankan `SELECT throwIf(getMacro('stackly_instance_id') != '<id>')` lewat native TCP.

- Pros: nol perubahan core; membuktikan listener, autentikasi, pipeline query, dan identitas sekaligus; pola yang sama dengan `stackly_readiness` PostgreSQL; terukur 60 ms.
- Cons: men spawn binary besar per percobaan; user tanpa password terjangkau proses lokal lain (dengan hak yang sangat sempit); hanya menguji native TCP secara langsung.

**Option 2: probe `http` ke `/ping` atau ke `/?query=SELECT%201&user=stackly_readiness`.**

- Pros: nol perubahan core; tanpa spawn.
- Cons: `/ping` tidak menjalankan query; path probe `http` hanya literal dan hasilnya hanya status, jadi tidak ada identitas. Server ClickHouse mana pun di port itu akan dianggap ready, yang justru dilarang PRD 22.3.

**Option 3: probe native baru `clickhouse-readiness`.** State machine HTTP yang mengirim query dan membandingkan body.

- Pros: tanpa spawn; bisa memeriksa kedua port.
- Cons: perubahan core ketiga dengan fuzz target dan test native sendiri (tujuh syarat probe native spec 0036), padahal kebutuhannya sudah terpenuhi probe `exec`; syarat pertama probe native ("tidak bisa dipenuhi `tcp`, `http`, atau `exec`") tidak terpenuhi.

Runner up: Option 3, kalau biaya spawn ternyata mengganggu.

### Keputusan 4: log

**Option 1: stderr saja (dipilih).** `<console>1</console>` tanpa `<log>`.

- Pros: log masuk log store spec 0027 lewat redactor, dengan retensi dan paging yang sama dengan provider lain; tidak ada file log yang tumbuh tanpa batas.
- Cons: tidak ada file `clickhouse-server.log` yang dikenal pengguna ClickHouse.

**Option 2: file log di `${instance.logs}` lewat `logs.files`.**

- Pros: bentuk yang dikenal.
- Cons: rotasi dan ukuran menjadi urusan ClickHouse; isi file tidak lewat redactor sebelum ditulis; dua sumber log untuk satu instance.

Runner up: Option 2.

### Keputusan 5: cara reload

Keputusan ini ditambah setelah cross check (lihat *Cross check*).

**Option 1: `action: run` dengan `SYSTEM RELOAD CONFIG` lewat user `stackly_reload` (dipilih).**

- Pros: sinkron; operasi reload baru berhasil setelah server benar benar memuat config; config yang ditolak membuat operasi gagal sementara server tetap berjalan dengan config lama; terbukti di binary.
- Cons: user tanpa password kedua; satu spawn `clickhouse client` per reload.

**Option 2: `action: none`, mengandalkan pengawas file ClickHouse.**

- Pros: tidak ada user tambahan dan tidak ada spawn.
- Cons: operasi reload dilaporkan berhasil sampai sekitar 2 detik sebelum perubahan berlaku, dan config yang ditolak ClickHouse tidak pernah terlihat sebagai kegagalan.

**Option 3: `action: signal` dengan `SIGHUP`.**

- Pros: bentuk yang sama dengan PostgreSQL.
- Cons: di ClickHouse `SIGHUP` tercatat sebagai buka ulang log; hasil yang terlihat benar di percobaan awal ternyata kerja pengawas file. Operasi akan melapor sukses untuk hal yang tidak ia lakukan.

Runner up: Option 2.

### Keputusan 6: handover restart daemon

**Option 1: tetap `unproven` (dipilih).** Tidak ada test native launchd di fitur ini.

- Pros: tidak butuh sesi login GUI dan launchd asli, yang sesi build tidak boleh jalankan; `stop_restore` aman untuk ClickHouse (stop 1 detik, start 0,5 detik); provider tanpa entri sah masuk catalog (spec 0036).
- Cons: restart daemon terencana menghentikan instance ClickHouse sebentar.

**Option 2: tambah test launchd dan entri `HANDOVER_PROVEN` sekarang.**

- Pros: instance bertahan melewati restart daemon.
- Cons: run nya langkah manual user; `handover.rs` adalah file bersama; menunda fitur untuk hal yang tidak diminta Done when.

Runner up: Option 2 sebagai fitur lanjutan (Follow-up spec 0030).

## Rationale

Ukuran yang dipakai untuk setiap keputusan sama: penuhi REQ-037 secara harfiah pada binary asli, dengan perubahan core sesedikit mungkin, dan tanpa mekanisme yang hanya benar di atas kertas. Karena itu setiap pilihan dicoba dulu pada 26.8.30.8.

Format `xml` menang karena REQ-037 meminta semua path di dalam instance, dan itu butuh sembilan path yang disusun dari folder instance ditambah subfolder. Escaper XML mengizinkan itu secara alami; escaper YAML tidak. Menambah satu escaper tanpa parser dan tanpa crate baru lebih murah dan lebih mudah dibaca daripada sepuluh argumen command line, dan spec 0009 memang sudah menyediakan tempatnya.

Hash menang karena percobaan menemukan salinan `users.xml` di `data/preprocessed_configs/`. Dengan password polos, salinan itu menjadi kebocoran ke folder data dan backup. Dengan hash, yang tersalin hanya hash, residual yang sudah dinyatakan spec 0011. Bentuk filternya disamakan dengan sesi Valkey supaya core hanya berubah sekali.

Probe `exec` menang karena ia memenuhi "query dan identitas endpoint yang benar" tanpa kode baru. Identitas lewat macro lebih kuat daripada sekadar "ada server ClickHouse": dua instance Stackly sama sama punya user `stackly_readiness`, jadi hanya nilai per instance yang membedakan. Biaya spawn dibatasi ke fase start dan reload dengan memakai `/ping` untuk health.

Reload lewat `SYSTEM RELOAD CONFIG` menang karena hanya itu yang membuat kata "reload berhasil" benar: perintahnya sinkron dan gagal kalau config ditolak. Harganya user tanpa password kedua, yang haknya hanya dua perintah muat ulang.

Watchdog dimatikan karena tanpa itu proses yang Stackly awasi bukan server, dan seluruh kontrak proses (lock di fd 3, PID, sinyal stop) jatuh ke proses yang salah.

## Keputusan kecil

| Hal | Pilihan | Alasan | Runner up |
| --- | --- | --- | --- |
| Path `users.xml` | argv sesudah `--` | `config.xml` di folder config dan `config-history` tidak memuat path folder run; dicoba, bertahan melewati `SYSTEM RELOAD CONFIG` | `<users_xml><path>` di template dengan `{{ instance.secrets }}` |
| `versionCheck.args` | `["server", "--version"]` | mencetak `ClickHouse server version ...`, sesuai mode yang dijalankan | `["--version"]` (mencetak `ClickHouse local version ...`) |
| Connection utama | `http` | sebagian besar driver memakai HTTP | `native` |
| Kelas `defaultPassword` | `reload` | file dirender ulang tiap start dan reload; itu yang benar benar terjadi | `restart` |
| User tambahan | `access_management` untuk `default` | pengguna bisa membuat user lewat SQL tanpa menyentuh file yang dikelola | hanya `default` |
| Health | `http` `/ping` | murah, dan menguji port kedua | probe readiness yang sama |
| `disable_internal_dns_cache` | `1` | menghilangkan error DNS tiap 15 detik di Mac yang hostname nya tidak bisa di resolve | biarkan |
| Tabel log sistem | tidak aktif | config tanpa blok `query_log` dan kawan kawan tidak membuatnya; folder data tidak tumbuh sendiri | setting `queryLog` |
| Setting batas memori | ditunda | template tidak bisa mengalikan MiB ke byte; default upstream 0,9 RAM hanya batas, bukan alokasi | field byte mentah |
| Skenario konformitas | satu, `default` | tidak ada mode kedua yang didukung | skenario `logLevel` lain |
| Remedy | tidak ada | satu satunya kandidat (`Cannot lock file`) ada di baris 2 dari 16 baris stderr, di luar kutipan 10 baris terakhir; port bentrok sudah ditangani core | remedy dengan `failureCodes` saja (terlalu lebar) |
| Config client | `client.xml` kosong lewat `--config-file` | config client sistem atau pengguna bisa menggagalkan probe (dicoba) | `--password ""` di argv probe |
| User reload | user kedua `stackly_reload` | `SYSTEM` ditolak di mode `readonly`, dan user readiness sebaiknya tetap `readonly` | satu user tanpa `readonly` untuk keduanya |
| Test BR-025 | `clickhouse_compatibility` | record harus memuat check khusus ClickHouse | test konformitas |
| Parse ulang XML | tidak | butuh parser atau crate baru; sama dengan `natsConf` dan `postgresqlConf` | `quick-xml` |

## Catatan percobaan (2026-11-12)

Binary dari entri cache `097b41ad97111832cc5ddddd0b514e80307df24f50b5893e9d2035a0c4f47402`, diekstrak dengan `/usr/bin/tar --no-mac-metadata` ke folder scratch sesi (dihapus sesudahnya), dijalankan dengan `env -i` dan `HOME` serta `TMPDIR` di folder temp percobaan, port 18123 sampai 18128 dan 19000 sampai 19005. Tidak ada unduhan, tidak ada proses yang tertinggal.

1. Tanpa `CLICKHOUSE_WATCHDOG_ENABLE=0`: `ps` menunjukkan proses yang di spawn bernama `clickhouse-watchdog` dengan satu anak, dan `lsof` menunjukkan kedua listener milik anak itu. Dengan variabel itu: satu proses, tanpa anak.
2. Config XML minimal dengan `listen_host`, `http_port`, `tcp_port`: tepat dua listener TCP, tanpa UDP.
3. `users.xml` dengan `password_sha256_hex`: login dengan password asli lulus di native dan HTTP; password salah mendapat kode 516 (native) dan HTTP 401.
4. `stackly_readiness` dengan `<grants><query>GRANT SELECT ON system.one</query></grants>` dan profil `readonly`: hasilnya seperti tabel di spec. `SHOW GRANTS` mencetak `GRANT SELECT ON system.one TO stackly_readiness`.
5. Query identitas: exit 0 untuk ID yang benar (58 ms), exit 139 dengan `FUNCTION_THROW_IF_VALUE_IS_NON_ZERO` untuk ID lain.
6. Path dengan spasi, `&`, `<`, `>`, dan `é`, ditulis dengan `&amp;`, `&lt;`, `&gt;`: diterima, data ditulis di folder yang benar.
7. `system.server_settings` sebelum lima key lookup ditulis: `dictionaries_lib_path`, `top_level_domains_path`, `dynamic_user_defined_executable_functions_path` di `/var/lib/clickhouse/`, `google_protos_path` di `/usr/share/clickhouse/protos/`. Sesudah ditulis: tidak ada path file di luar instance.
8. Server kedua di port yang sama: exit 210. Server kedua di folder data yang sama: exit 76 dengan teks yang dipakai remedy.
9. `SIGHUP` sesudah mengganti `users.xml` (rename atomik) dengan hash baru: beberapa detik kemudian password baru diterima dan PID sama. Percobaan awal membaca ini sebagai bukti `SIGHUP` memuat ulang config. Itu keliru: server mencatat `SIGHUP` sebagai `Logging to console but received signal to close log file (ignoring)`, dan butir 10 menjelaskan hasilnya. Dengan `config_reload_interval_ms` satu jam pun `users.xml` tetap termuat dalam 4 detik, jadi file users punya pengawas sendiri atau key itu tidak mengaturnya; sebabnya tidak ditelusuri.
10. Mengubah `config.xml` tanpa sinyal (level `warning` ke `trace`): 13 baris `<Trace>` atau `<Debug>` muncul dalam 4 detik. ClickHouse memuat ulang sendiri.
11. `SIGTERM`: exit 0 dalam sekitar 1 detik, `status` terhapus. Start lagi di data yang sama: baris yang ditulis sebelumnya terbaca.
12. Tanpa `disable_internal_dns_cache`: 42 baris `DNSResolver` dalam tiga menit, masing masing dengan stack trace. Dengan key itu: nol.
13. Fd 3 yang dibuka sebelum spawn tetap menunjuk file yang sama di proses server.
14. Varian YAML (`config.yaml`, `users.yaml`) juga diterima, termasuk escape `{`; ini dasar Option 2 Keputusan 1.
15. `data/preprocessed_configs/` berisi `config.xml` dan salinan file users (dengan hash) setelah start.

16. `SYSTEM RELOAD CONFIG` sebagai `stackly_readiness` (`readonly` 1) ditolak dengan kode 164, juga `SYSTEM RELOAD USERS`. Sebagai `stackly_reload` (tanpa `readonly`, dua grant `SYSTEM RELOAD`): exit 0, password baru diterima dan yang lama ditolak tanpa jeda, key users dari argv tetap dipakai. Dengan `config.xml` yang dipotong menjadi XML tidak sah: exit 91 (`SAXParseException`), dan server tetap menjawab query.
17. `stackly_reload`: `SHOW GRANTS` mencetak `GRANT SYSTEM RELOAD CONFIG, SYSTEM RELOAD USERS ON *.*`; `SHOW DATABASES` kosong; `SYSTEM SHUTDOWN` ditolak; `SELECT 1` dan `SELECT * FROM numbers(10)` boleh.
18. `clickhouse client --config-file <file>` dengan file berisi `<password>`, ditambah `--user stackly_readiness` di argv: login gagal (kode 516), karena password dari file ikut terkirim. Dengan file `<config>` kosong: lulus.
19. `config.xml` tanpa blok `<user_directories>`, dengan path users hanya dari argv: server gagal start, `Settings profile default not found`. Dengan blok `local_directory` di file: jalan.
20. Stderr untuk folder data yang dipakai server lain: 16 baris; `Cannot lock file` di baris 2, sisanya stack trace dan `(version ...)`.

Pengukuran level log lewat hitungan baris `<Information>` sesudah satu `SELECT 1` ternyata tidak sah (query itu tidak menulis baris `<Information>`), jadi efek `SYSTEM RELOAD CONFIG` pada level log belum terbukti di percobaan; butir 10 hanya membuktikan pengawas file menerapkannya. Test native AC-10 yang membuktikannya.

Yang tidak dicoba: `--ask-password` (butuh terminal interaktif; langkah manual 3 membuktikannya), `from_env`, dan `clickhouse client` yang membaca env `CLICKHOUSE_PASSWORD` (hanya dipakai kode test; cadangannya `--password` dengan credential fixture).

## Pertukaran dengan sesi lain

2026-11-12, sesi `arc-f40-spec` (Valkey) mengusulkan kontrak filter hash bersama, karena Valkey menerima password ACL ber hash SHA-256. Bentuk yang disepakati dan ditulis sama di kedua spec: sintaks `{{ secret.<field> | sha256Hex }}`, hanya di config `sensitive`, keluaran 64 hex mentah, segmen plan `Segment::SecretHash`, tanpa kontrak wire. Fitur yang masuk lebih dulu membangunnya.

Sesudah cross check kedua spec, dua hal disamakan lagi: kode issue (`secret_not_allowed_here` untuk config tidak `sensitive`, `template_type` untuk salah pakai filter, `template_syntax` untuk nama filter kosong atau ber karakter lain), dan bentuk JSON segmen `secretHash` di plan, supaya golden dan corpus beku kedua fitur sama byte demi byte. Dua kalimat terakhir AC-2 ditambahkan sesudah lead menerima spec, atas usul `arc-f40-spec`, dan dilaporkan ke lead.

## Cross check

Dijalankan 2026-11-12 oleh model lain (read only, membaca spec ini dan kode validasi). Yang ia benarkan terhadap kode: token dan executable di probe dan open action, `env` start, `${instance.secrets}` dan `--` di argv, secret ber kelas `reload`, config `sensitive` di `reload.render`, health dengan jenis probe lain, manifest tanpa `logs` dan tanpa `dataMarker`, dan perbandingan versi empat komponen.

Temuan dan penyelesaiannya (semua diterapkan):

| Temuan | Penyelesaian |
| --- | --- |
| `SIGHUP` kemungkinan tidak memuat ulang config; hasil percobaan bisa dijelaskan pengawas file | diuji ulang; reload menjadi `SYSTEM RELOAD CONFIG` lewat `stackly_reload` (Keputusan 5) |
| Test "server asing di port" tidak pernah sampai ke query identitas, karena start menolak port yang dipakai sebelum spawn | identitas dibuktikan dengan menjalankan `ProbeSpec` readiness terhadap server asing; test port bentrok mengharapkan `supervisor.endpoint_in_use` |
| Remedy `data_in_use` bisa tidak pernah cocok (kutipan 10 baris, 256 karakter) | diuji; baris kuncinya memang di luar kutipan, remedy dihapus |
| Invariant "hanya dua listener" punya lubang `config.d/` dan pengawas file | dinyatakan sebagai batas, masuk README dan Follow-up |
| AC-2 memakai kode yang berbeda dari aturan yang ada untuk secret di config tidak `sensitive` | `secret_not_allowed_here` dipertahankan; `template_type` hanya untuk salah pakai filter |
| Tempat lain yang mendaftar paket belum disebut | ditambah ke Build plan dan tabel file bersama |
| Nilai yang harus ditebak builder (catatan platform, DDL tabel, ID check, cara ganti secret, cakupan pindaian file) | ditulis di AC, *Test native*, dan *Value sourcing* |
| AC-16 dan langkah manual 4 saling bertentangan soal siapa membuat record evidence | sesi build yang menjalankannya dari cache; langkah manual 4 dihapus |
| Syarat pertama kebijakan additive (baris spec 0009) belum dijadwalkan | menjadi prasyarat langkah 1 dan 2 |
| Batas `<networks>` tidak bisa diuji lewat jaringan | dibuktikan golden render |
| `clickhouse client` masih membaca config client sistem | diuji; `client.xml` kosong lewat `--config-file` |
