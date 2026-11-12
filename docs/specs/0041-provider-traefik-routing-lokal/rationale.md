# 0041. Rationale: provider Traefik dan routing lokal

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: scope menandai fitur 41 "needs a decision" dan Follow-up beberapa spec mengandaikan fitur ini membawa pekerjaan core (bcrypt untuk htpasswd, dashboard, HTTPS). Setelah membaca `main`, hampir semua yang diminta Done when sudah ada: paket `providers/traefik`, probe `traefik-routes`, halaman Configuration dengan daftar `routes`, dan test dua instance. Yang benar benar belum ada adalah bukti dan status resmi, bukan mekanisme. Jadi pertanyaan yang tepat bukan "apa yang perlu ditambah ke core untuk Traefik", tetapi "apa yang boleh dijanjikan provider ini dengan mekanisme yang sudah terbukti, dan apa yang harus ditolak dengan jujur". Spec ini memilih janji yang sempit dan terbukti.

Traefik adalah provider nyata ketiga. Paketnya dibuat fitur 9 sebagai contoh daftar `routes`, diperbaiki langkah 22 spec 0009 (init tanpa token port), lalu mendapat probe `traefik-routes` dari audit keamanan 2026-11-05 karena file provider Traefik menyimpan config lama tanpa memberi tahu saat file baru tidak bisa dimuat. Fitur 29 membuktikan apply `routes` lewat reload pada dua instance asli. Meski begitu `official/traefik` masih `[[planned]]` di catalog: belum ada evidence kompatibilitas scope `provider`, belum ada skenario konformitas milik paket, belum ada `connection`, remedy, atau bukti handover.

PRD 22.5 dan REQ-039 meminta entrypoint loopback, routing lewat file provider, health di listener khusus, namespace route per instance, validasi dan apply aman, dashboard dan API insecure yang tidak menyala secara default, dan HTTPS yang hanya aktif dengan certificate dan key yang valid dan disimpan aman. AC-039 meminta perubahan route di A tidak menyentuh B, dan config invalid tidak mengganti last known good. Matrix PRD 32.2 meminta dua instance, routing update, invalid config, HTTP, dan health, dengan HTTPS hanya "ketika capability diuji".

Gaya yang membentuk keputusan: scope 07 mengukur RISK-07 dengan jumlah perubahan core per provider; spec 0029 sudah memutuskan (keputusan user 2026-11-02) bahwa Stackly tidak pernah roll back sendiri; spec 0009 sudah memutuskan (keputusan user 2026-11-05) bahwa API read only di port monitoring loopback adalah residual yang diterima; dan lima provider dibangun paralel, jadi setiap file bersama yang disentuh adalah biaya koordinasi. Tanpa keputusan, developer akan menebak apakah HTTPS, dashboard, dan bcrypt termasuk, dan tiga tebakan itu masing masing menyeret perubahan core.

## Options considered

### Keputusan 1: HTTPS di revision ini

**Opsi A: tidak ditawarkan (dipilih).** Paket hanya HTTP; tidak ada field certificate.

- Pro: tidak ada janji yang tidak bisa dibuktikan; nol perubahan core; matrix PRD 32.2 mengizinkannya ("ketika capability diuji").
- Kontra: developer yang butuh `https://` lokal belum terlayani; kalimat deskripsi scope perlu dibaca sebagai batas.

**Opsi B: path certificate dan key milik pengguna sebagai settings.** Dua field path, dirender ke `tls.certificates` di file dynamic, router mendapat `tls: {}`.

- Pro: hampir hanya perubahan paket; key tetap di tempat pengguna menyimpannya.
- Kontra: Traefik memakai certificate bawaannya sendiri kalau pasangan itu gagal dimuat, dan probe tidak melihatnya, jadi HTTPS bisa "aktif" tanpa certificate pengguna, tepat yang dilarang PRD. Menutup celah itu butuh parser certificate atau klien TLS di core.

**Opsi B2: seperti B, ditambah `tls.options.default.sniStrict: true`.** Traefik menolak handshake untuk nama yang tidak punya certificate, jadi certificate bawaannya tidak pernah disajikan.

- Pro: gagal tertutup tanpa perubahan core; ini opsi termurah yang jujur.
- Kontra: perilakunya pada 3.7.15 belum dibuktikan di Mac ini; readiness tetap tidak tahu apakah certificate dimuat, jadi "ready" tidak berarti HTTPS jalan; menambah dua setting, satu tombol per route atau per instance, dan satu rangkaian test native TLS ke fitur yang Done when nya tidak meminta HTTPS.

**Opsi C: key di Keychain, dirender ke `${instance.secrets}`, dengan probe TLS.** Lengkap dan aman.

- Pro: memenuhi "cara penyimpanan yang aman" sepenuhnya.
- Kontra: secret multi baris, mekanisme pengiriman baru, probe TLS, dependency kripto baru di core; ini fitur sendiri, bukan bagian provider.

### Keputusan 2: dashboard dan API

**Opsi A: dashboard selalu mati, API read only tetap (dipilih).**

- Pro: tidak ada tombol yang membuka 404; sama dengan keadaan yang sudah diaudit; nol perubahan core.
- Kontra: tidak ada tampilan visual router; API tetap bisa dibaca proses lokal lain.

**Opsi B: setting `dashboard` (default mati) tanpa password.** `api.dashboard` mengikuti setting.

- Pro: hanya template; data yang terbuka sama dengan API yang sudah ada.
- Kontra: entri `openWith` atau `connection` tidak punya syarat, jadi tombol dashboard tampil dan membuka 404 selama setting mati; tanpa tombol, fiturnya tidak ditemukan. Butuh syarat `openWith` di schema manifest supaya rapi.

**Opsi C: API dan dashboard di balik basic auth dengan hash bcrypt.** `api.insecure: false`, router ke `api@internal` dengan middleware, password di Keychain.

- Pro: menutup residual baca API; menjawab Follow-up bcrypt spec 0011.
- Kontra: probe harus mengirim credential (param secret baru, entri `EXPOSE_ALLOWED`), setiap instance Traefik mendapat secret dan bisa terblokir Keychain, dan core mendapat dependency bcrypt. Melindungi daftar route lokal dengan biaya itu tidak sepadan.

### Keputusan 3: rule yang ditolak Traefik

**Opsi A: verify sesudah tulis, lalu roll back eksplisit (dipilih).** Mekanisme spec 0009 AC-22 dan spec 0029 AC-8 apa adanya.

- Pro: sudah terbukti native; konsisten dengan provider lain; nol perubahan.
- Kontra: ada jendela ketika route yang ditolak tidak menjawab dan route lain di revision itu sudah berlaku.

**Opsi B: pra validasi di proses Traefik sementara.** Sebelum commit, jalankan Traefik kedua di port sementara dengan file kandidat, probe, lalu matikan.

- Pro: instance yang jalan tidak pernah melihat rule yang ditolak.
- Kontra: primitive lifecycle baru di manifest dan supervisor, alokasi port sementara, dan satu sampai dua detik tambahan per apply; perubahan core terbesar di antara semua opsi.

**Opsi C: mempersempit `rule` dengan pattern schema atau field terstruktur (`host`, `pathPrefix`).**

- Pro: sebagian besar kesalahan ketik tertangkap lapis schema.
- Kontra: regex tidak bisa menjamin Traefik menerima rule (regex di `HostRegexp`, nama host tidak sah), pengelompokan dengan kurung hilang, dan fixture serta test spec 0009 dan 0029 yang memakai `rule` harus ditulis ulang.

**Opsi D: roll back otomatis saat verify gagal.**

- Pro: jendela route mati menjadi singkat.
- Kontra: bertentangan dengan keputusan user di spec 0029 ("Stackly tidak pernah roll back sendiri").

### Keputusan 4: namespace route

**Opsi A: instance adalah namespace (dipilih).** Satu proses dan satu folder per instance.

- Pro: isolasi datang dari proses dan path, bukan dari konvensi nama; nama route di form sama dengan nama di Traefik dan di log.
- Kontra: nama router tidak unik lintas instance, jadi log dua instance bisa menyebut `api@file` yang berbeda.

**Opsi B: awalan nama instance di setiap router.**

- Pro: nama unik lintas instance.
- Kontra: rename instance mengubah semua nama router; nama di form berbeda dari nama di Traefik; tidak menambah isolasi apa pun.

### Keputusan 5: test BR-025

**Opsi A: test native khusus `traefik_compatibility` (dipilih).**

- Pro: check nya mengikuti baris matrix PRD 32.2 (routing update, invalid config).
- Kontra: satu file test lagi di samping test konformitas.

**Opsi B: test konformitas sebagai test BR-025.**

- Pro: tanpa test baru.
- Kontra: langkah harness tidak punya routing update atau invalid config, jadi record nya tidak membuktikan baris matrix.

## Rationale

Semua pilihan mengikuti satu aturan: provider ini hanya menjanjikan yang mekanismenya sudah terbukti native di `main`. Itu yang membuat ukuran RISK-07 jujur (satu baris data), dan itu juga yang paling aman untuk lima sesi paralel, karena fitur ini nyaris tidak menyentuh file bersama sebelum milestone catalog.

HTTPS ditolak bukan karena sulit dirender, tetapi karena Traefik gagal secara diam diam pada certificate yang buruk. Menawarkan field certificate tanpa bisa membuktikan certificate itu yang disajikan akan menjadi label supported palsu, yang justru dilarang tujuan fase 07. Matrix PRD sendiri menaruh HTTPS di belakang "ketika capability diuji". Opsi B2 menutup bagian terburuknya dan menjadi titik mulai Follow-up, tetapi ia bertumpu pada perilaku Traefik yang belum dibuktikan dan readiness yang tidak melihat certificate, jadi ia pantas mendapat spec dan run native sendiri, bukan diselipkan ke fitur yang sudah bisa selesai tanpa itu.

Dashboard dan bcrypt jatuh bersama. Satu satunya alasan bcrypt adalah basic auth, dan satu satunya pemakai basic auth di fitur ini adalah dashboard. Dashboard tanpa password hanya masuk akal dengan tombol yang muncul saat ia menyala, dan itu butuh perubahan schema manifest. Dashboard dengan password menyeret secret ke setiap instance dan credential ke probe. Keduanya lebih mahal daripada nilainya untuk proxy lokal, sementara Done when hanya meminta dashboard insecure mati secara default, dan dashboard yang selalu mati memenuhinya.

Untuk rule yang ditolak, keputusan spec 0029 sudah mengikat: roll back adalah aksi pengguna. Pra validasi di proses sementara adalah perbaikan yang sah, tetapi ia primitive baru yang dipakai satu provider, dan belum ada bukti bahwa jendela "route mati sampai roll back" menyakitkan dalam pemakaian nyata. Ia dicatat sebagai Follow-up dengan pemicunya.

## Keputusan kecil

- **Revision tetap `1.0.0`**: paket belum pernah ada di `providers/catalog-revisions.json`, sama dengan alasan spec 0009 saat langkah 22. Runner up: `1.1.0`, yang membuat terbitan pertama tampak seperti perubahan.
- **Satu remedy, bukan dua**: doctor memakai entri pertama yang cocok, dan `when` tidak bisa menyangkal, jadi remedy kedua untuk file asing akan tertutup remedy pertama. Satu remedy dengan tiga langkah mencakup keduanya. Runner up: dua remedy dengan `readinessDetailContains`.
- **`routers` sebagai aksi browser**: menjawab Follow-up spec 0070 dengan sesuatu yang selalu berfungsi. Runner up: tanpa aksi browser sama sekali.
- **Folder `dynamic` tetap diawasi sebagai folder**, bukan `providers.file.filename`: itu keputusan spec 0009, dan file asing terdeteksi probe. Runner up: mode satu file, yang mengabaikan file asing tetapi mengubah perilaku yang sudah terbukti.
- **Tanpa setting baru** (access log, dashboard, TLS): setiap setting menambah permukaan test di lima tempat (golden, fixture form, snapshot, fuzz seed, konformitas).
- **`metrics` tidak diklaim**: capability itu berarti ada sumber metrik provider; Traefik belum merender blok metrics.
- **Dua skenario saja (`default`, `one-route`)**: skenario rule yang ditolak dengan `expect: start_fails` dibuang setelah cross check, karena start yang melewati batas readiness tidak mengisi `lastFailure` dan prosesnya tetap hidup, sedangkan langkah harness menuntut keduanya. Kasus itu dibuktikan test AC-4.

## Bukti yang dibaca

- `providers/traefik/` di `main` (manifest, schema, dua template, `versions.json` dengan record scope `runtime`).
- `crates/core/src/provider/probe/traefik.rs` (teks detail `routes_mismatch`) dan `crates/platform-macos/src/fs.rs` (nama file sementara `write_file_atomic`).
- `apps/daemon/tests/traefik/config.rs` dan `readiness.rs` (yang sudah terbukti: reload A tanpa menyentuh B, rule `Host(` gagal verify dengan `supervisor.reload_failed`).
- `crates/core/src/supervisor/handover.rs` dan `apps/daemon/tests/native_launchd.rs` (pola bukti NATS dan PostgreSQL).
- Spec 0009 (AC-21, AC-22, *Paket contoh NATS dan Traefik*), 0011 (*Pengiriman secret ke provider*), 0017 (*Aturan masuk catalog*), 0018 (AC-15), 0021, 0023, 0029 (AC-3, AC-7, AC-8, AC-10, AC-16), 0030, 0034 (*Blok remedies*), 0036 (*File skenario*, *Harness*, *Handover*, *Dari contoh ke paket resmi*), 0070 (AC-6, *Blok openWith*).
- Commit `27e98df3` (NATS masuk catalog) dan `0b43369a` (PostgreSQL `1.0.2`) untuk daftar file yang ikut berubah saat manifest atau catalog berubah.

Satu fakta tentang Traefik belum dibuktikan test yang ada dan dikunci AC-4: saat satu rule ditolak, router lain dari file yang sama tetap dimuat. Test `readiness.rs` hanya membuktikan bahwa router yang ditolak membuat probe gagal.

## Update 2026-11-12: temuan verify milestone 1 dan 2

### Yang ditemukan

Verify milestone 1 dan 2 (`chk-f41-m1`, `main` `0b87ca82`) lulus, dengan empat temuan yang harus dijawab spec:

1. **High.** Pada instance `ready` dengan route `api`, apply yang hanya mengganti `backendUrl` ke `http://127.0.0.1:9/%zz` berakhir `succeeded`. Revision baru mendapat `ready_at` 0,15 s sesudah apply dan menjadi last known good. Sesudah itu Traefik memuat file, menandai `api@file` `disabled` (`invalid URL escape "%zz"`), route menjawab 404, readiness menjadi `not_ready` `routes_mismatch` 19 s kemudian, dan `stackly instance config rollback` menjawab `config.no_last_known_good`. Sebabnya: verify sesudah reload (`crates/core/src/supervisor/reload.rs`) lulus pada keadaan Traefik yang lama, karena `compare` di `crates/core/src/provider/probe/traefik.rs` hanya membandingkan nama, rule, dan status router, dan pola `backendUrl` lama (`(/[^\s]*)?`) menerima path yang ditolak parser URL Go.
2. **Low.** Untuk apply yang hanya mengganti backend, operasi `succeeded` di +0,29 s sedangkan jawaban backend baru datang di +2,3 s.
3. **Low.** Route bernama `tls` lolos schema dan menulis key `http.routers.tls` dan `http.services.tls`, jadi kalimat AC-7 "tidak pernah memuat key `tls`" tidak benar secara harfiah.
4. **Low.** Subject evidence test native memakai literal `"3.7.15"`, dua komentar `harness.rs` masih menyebut NATS, dan kalimat *Test native* menulis owner `runtime maintainer` sedangkan kode dan spec 0003 memakai `@ojiepermana`.

Lead memutuskan arah temuan 1 (delegasi user): tutup di pola schema, tanpa perubahan core, dan catat sisa jendela verify sebagai residual dan Follow-up.

### Probe pada Traefik 3.7.15

Dijalankan langsung pada binary dari cache runtime xtask (sha256 `402f59ce…`, Go 1.26.9), di folder sementara, dengan file dynamic tulisan tangan yang memakai escape yang sama dengan escaper `yaml` Stackly (`{` dan `}` ditulis sebagai escape, karena file provider Traefik menjalankan file itu lewat template Go lebih dulu). Tidak ada kode repo yang diubah.

| Nilai URL server | Router | Catatan |
| --- | --- | --- |
| `http://127.0.0.1:9` | `enabled` | dasar |
| `http://127.0.0.1:9/%zz`, `/%`, `/%4`, `/a%zzb` | `disabled` | `invalid URL escape` |
| `http://127.0.0.1:9/a#%zz`, `/a#%`, `/a?b#c%2` | `disabled` | escape salah di fragment |
| `http://127.0.0.1:9/a?q=%zz`, `/a?%` | `enabled` | parser Go tidak memeriksa query |
| `http://127.0.0.1:9/%41`, `/%00`, `/%2F%25`, `/a#%41` | `enabled` | escape benar |
| `http://127.0.0.1:99999`, `:65536`, `:0`, `:00009` | `enabled` | request menjawab 502 |
| `http://[::1]:9`, `http://localhost:9` | `enabled` | |
| path dengan `é`, emoji, U+200B, `"`, `\`, `<`, `>`, `^`, backtick, `{`, `}`, pipa, `:`, `@`, `//` | `enabled` | |
| 605 nilai acak yang cocok dengan pola `%XX` saja | semua `enabled` | tiga host, port 1 sampai 5 digit, path ASCII dan Unicode tanpa whitespace |

Tiga fakta lain dari probe yang sama:

- Backend di `http://127.0.0.1:<port>/prefix` dan di `http://127.0.0.1:<port>/prefix?k=v#frag` menerima request `/some/path?x=1` apa adanya. Traefik tidak memakai path, query, atau fragment URL server.
- Dengan `broken.yaml` yang tidak bisa diparse di folder `dynamic`, mengganti backend di `routes.yaml` tidak dimuat: backend lama tetap menjawab 200 dan router tetap `enabled`. Sesudah `broken.yaml` dihapus, backend baru dimuat.
- Pada Traefik yang sedang tenang, backend baru terlihat 0,06 s sesudah file diganti. Jeda sampai 2 s muncul saat muatan sebelumnya baru saja terjadi (throttle provider).

Karakter kontrol tidak diuji lewat Traefik: dialek settings sudah menolaknya di setiap teks (`crates/core/src/provider/settings.rs`, `ControlCharacters`), jadi tidak pernah sampai ke file render.

### Opsi untuk temuan 1

**Opsi P2: `backendUrl` hanya scheme, host loopback, dan port (dipilih).** Pola `http://(127\.0\.0\.1|\[::1\]|localhost):[0-9]{1,5}/?`.

- Pro: menutup kelas temuan secara struktural, karena tidak ada lagi teks bebas yang sampai ke parser URL Traefik; tidak bergantung pada daftar aturan parser Go yang bisa berubah di line berikutnya. Jujur terhadap fakta probe: path tidak pernah berpengaruh, jadi menerimanya adalah jebakan diam diam. Melonggarkan nanti (misalnya bersama `preservePath`) adalah perubahan additive, sedangkan memberi arti pada path yang sudah tersimpan akan mengubah perilaku nilai lama. Nol perubahan core.
- Kontra: nilai dengan path yang dulu diterima sekarang ditolak (paket belum pernah terbit, jadi tidak ada instance resmi yang terkena); satu input test dan satu golden berubah; pesan error field tetap umum ("does not match the required pattern").

**Opsi P1: tetap menerima path, `%` hanya sah sebagai `%XX`.** Pola `http://(127\.0\.0\.1|\[::1\]|localhost):[0-9]{1,5}(/([^\s%]|%[0-9A-Fa-f]{2})*)?`. Ini bentuk paling sempit dari keputusan lead.

- Pro: perubahan himpunan nilai paling kecil; input test dan golden tidak berubah; terbukti aman pada 3.7.15 (605 nilai acak dimuat `enabled`).
- Kontra: tetap menerima path yang diabaikan Traefik; keamanannya bertumpu pada daftar aturan parser Go untuk path dan fragment yang kita uji hari ini, bukan pada struktur; pola lebih sulit dibaca di schema dan di editor JSON Schema.

**Opsi 2 checker: probe juga membandingkan service.** `compare` membaca `GET /api/http/services` dan mencocokkan URL backend dengan file.

- Pro: menutup seluruh jendela verify basi, termasuk dua residual yang tersisa.
- Kontra: mengubah dua modul probe di core dan spec 0009 AC-22, melanggar AC-14 fitur ini (ukuran RISK-07), dan menyentuh file bersama saat lima provider dibangun paralel.

**Opsi 3: batasi juga port ke 1 sampai 65535.**

- Pro: nilai yang tidak mungkin jalan ditolak lebih awal.
- Kontra: bukan penolakan Traefik (router tetap `enabled`, request 502 seperti backend mati), jadi tidak menyentuh last known good; menambah alternasi enam cabang ke pola untuk kelas yang tidak berbahaya.

### Alasan

Keputusan lead adalah menutup temuan 1 di pola, dan probe menunjukkan pola memang cukup: satu satunya kelas yang lolos pola lama lalu ditolak Traefik adalah `%` yang salah di path atau fragment. P1 dan P2 sama sama menutupnya. P2 dipilih karena probe yang sama menunjukkan fakta yang belum diketahui saat keputusan itu dibuat: path URL server tidak dipakai Traefik sama sekali. Mempertahankan path berarti mempertahankan teks bebas yang tidak berguna tepat di tempat bug ini lahir, dan mewariskan nilai tersimpan yang artinya akan berubah kalau path suatu hari didukung. P2 juga membuat invariant mudah dijelaskan: tidak ada yang bisa ditolak parser, karena tidak ada yang tersisa untuk diparse selain host dan port yang sudah ditentukan pola. Keputusan ini bertanda `berat` (mengubah teks AC-1 pada spec yang sedang dibangun dan mempersempit nilai lebih jauh dari kata kata keputusan lead); kalau lead memilih P1, yang berubah hanya baris pola, tabel contoh di *Perubahan paket* butir 7, nilai contoh AC-15 dan test nya, dan langkah 7a tidak lagi menyentuh `traefik_settings` dan golden `render-routes.txt`.

Opsi 2 checker tidak diambil karena lead sudah memutuskannya, dan probe tidak menemukan kelas yang hanya bisa ditutup olehnya di area yang didukung. Dua residual yang ia tutup (operasi mendahului muatan Traefik, dan backend yang ditahan karena file pengguna yang rusak) dinyatakan di *Yang tidak dilihat verify*, *Edge cases*, dan README, dan menjadi Follow-up ke spec 0009.

### Keputusan kecil update ini

- **Temuan 2 dinyatakan, tidak diperbaiki**: AC-2 mengikat jawaban, bukan akhir operasi. Runner up: menunggu tetap 2 s sesudah setiap reload Traefik, yang memperlambat setiap apply dan tetap tidak membuktikan muatan.
- **Temuan 3, kalimat AC-7 dipertajam, nama `tls` tidak dilarang**: yang dijanjikan adalah tidak ada konfigurasi TLS, bukan tidak ada kata `tls`. Melarang nama butuh pola `name` yang menyangkal satu kata, dan mesin regex settings tidak punya lookaround, jadi larangan itu harus ditulis sebagai alternasi panjang untuk satu kasus yang tidak berbahaya (route bernama `tls` dimuat `enabled` pada 3.7.15). Runner up: melarang nama `tls`.
- **Temuan 4, versi subject dari proses yang berjalan**: sama dengan test NATS (`/varz`), dan tetap benar saat `versions.json` mendapat versi kedua. Runner up: field `version` artefak di `versions.json`, yang tersedia sebelum start tetapi hanya mengulang apa yang dipasang, bukan apa yang berjalan.
- **`description` field tidak diubah**: teksnya masih benar ("such as http://127.0.0.1:3000"), dan mengubahnya akan menyeret `FixtureConfiguration.swift` di `apps/macos/`, yang dilarang AC-14. Penjelasan "tanpa path" masuk README.
- **Satu `/` penutup tetap diterima**: URL yang ditempel dari browser sering berakhiran `/`, dan Traefik memuatnya.
- **Nomor langkah 7a dan 7b, milestone 2b**: nomor langkah lama dipakai `verify.md` dan subject commit, jadi tidak digeser.
- **Update ini tanpa cross check**: jawaban tetap panel dari lead untuk update kecil. Faktanya diuji langsung pada binary Traefik, bukan dinilai dari bacaan.

## Update 2026-11-12 (kedua): selaras dengan hasil verify penuh

Verify penuh (`chk-f41-m3`) lulus: AC-1 sampai AC-11, AC-13, AC-14, dan AC-15 terpenuhi, dan AC-12 `BLOCKED` sampai run launchd di sesi GUI. Temuan High verify pertama tertutup (25 bentuk URL ditolak di field, 10 bentuk yang lolos pola semuanya `enabled`). Update ini hanya menyamakan teks spec dengan yang dibangun; tidak ada perilaku, paket, template, kode, ID AC, atau keputusan yang berubah.

| No | Temuan | Yang diubah di spec | Alasan |
| --- | --- | --- | --- |
| 1 | Medium: apply pada instance yang sudah `not_ready` dilaporkan `succeeded` walau revision nya memuat rule yang ditolak Traefik. Last known good tetap aman dan revision itu tidak mendapat `ready_at`. | Satu kalimat batas di AC-4, catatan di lapis 3 *Apply yang aman*, butir *Edge cases* baru, kalimat Consequences "route yang salah tidak pernah dilaporkan sukses" dibatasi ke instance yang `ready`, satu butir Negative, baris test karakterisasi di *Test native*, dan dua Follow-up (spec 0026 dan 0029; README). | Sebabnya spec 0026 AC-10 ("untuk instance yang belum ready, reload terverifikasi saat aksinya selesai dan PID tidak berubah"), yang sengaja begitu supaya reload bisa dipakai untuk memperbaiki instance yang belum ready. Paket Traefik tidak bisa mengubahnya, dan AC-14 melarang perubahan core di fitur ini. Yang dijanjikan spec ini di jalur itu adalah bagian yang aman (last known good, `ready_at`, roll back), dan itu terbukti. Kalimat lama terlalu luas, jadi dibatasi, bukan dipertahankan. Runner up: menjadikan apply itu gagal lewat perubahan `reload.rs`, yang membalik perilaku spec 0026 untuk semua provider; itu keputusan pemilik spec 0026 dan 0029, jadi masuk Follow-up. |
| 2 | Langkah manual 7 menjalankan test launchd dengan dua baris `handover.rs` yang belum di commit, jadi record nya `sourceDirty`. | Langkah manual 7 dan catatan di langkah 10: commit dulu kedua baris, jalankan lewat `cargo xtask evidence run`, lalu `git revert` kalau bukan `PASS`. | Janji AC-12 tidak berubah (baris hanya bertahan bersama record `PASS`). `git revert` dipilih, bukan membuang commit, supaya riwayat tetap maju dan aman kalau commit itu sudah terlanjur dibagi. Urutan yang sama dipakai spec 0040 langkah 12. |
| 3 | `published_at` revision catalog 13 lebih awal sekitar 28 menit dari waktu commit nya, sedangkan spec menulis "waktu UTC commit itu". | Kalimat di *Entri catalog* menyebut nilai itu ditulis sebelum commit dan selisihnya kosmetik. | `catalog check` hanya menolak `published_at` yang lebih awal dari revision sebelumnya. Menyamakan nilai itu dengan waktu commit tidak mungkin persis, karena nilainya bagian dari isi commit. |

Baris basi di `verify.md` yang checker catat adalah urusan pemilik `verify.md`. Test tambahan tester (regresi temuan High, 70 bentuk `backendUrl` yang ditolak, isolasi sesudah restart, route bernama `tls`) tidak butuh perubahan spec.

Update ini tanpa cross check (jawaban tetap panel dari lead untuk update penutup wave).

## Cross check

Cross check read only dengan model lain (Opus) pada 2026-11-12, terhadap kode di `main`. Yang diubah karenanya:

- Skenario `rejected-rule` dibuang: `ReadinessTimeout` tidak mengisi `lastFailure` dan tidak mematikan proses (`apps/daemon/tests/supervisor/process.rs`, `apps/daemon/tests/conformance/steps.rs`), jadi langkah `start_fails` harness akan gagal.
- AC-12 dan langkah 10 diurutkan ulang: test launchd menuntut `handover_proven` dari binary yang diuji, jadi baris `HANDOVER_PROVEN` masuk sebelum run dan dibuang kalau record tidak `PASS`.
- AC-14 mengizinkan modul test `handover.rs`, yang hari ini menegaskan Traefik belum terbukti.
- Tiga file test bersama ditambahkan ke Build plan dan *Sesi paralel*: `crates/core/tests/provider_check.rs`, `crates/core/tests/connection_render.rs`, dan `xtask/tests/provider_check.rs` (test freeze yang memakai Traefik sebagai contoh planned).
- AC-3 menyebut path error nama ganda yang benar (`settings.routes[<i>].name`).
- Arti tiap check `compatibility.json`, subject evidence, dan helper `Checks` ditulis.
- AC-5 dan AC-6 menyebut syarat "tanpa route yang cocok" untuk jawaban 404 di port `web`.
- Penyimpangan dari kalimat PRD tentang API insecure dinyatakan di *Trace PRD* dan *Security model*.
- Opsi HTTPS dengan `sniStrict` dicatat dan ditolak untuk revision ini.
- Dua kegagalan file pengguna di `dynamic/` (YAML rusak, nama router sama) masuk *Edge cases*, dan teks remedy langkah 3 diperluas.

Yang dikonfirmasi benar: token `${endpoint.ping.address}` diikuti path sah di `connection` dan URL browser; remedy dengan `readinessReasons` dimuat dan dipasang doctor pada instance `running` yang `not_ready`; pattern settings mencocokkan seluruh nilai; teks detail probe sama dengan AC-4 dan AC-8; `freeze` menolak provider planned. Perilaku Traefik (router dengan rule yang ditolak menjadi `disabled` sementara router lain dimuat, dan 404 untuk `/dashboard/`) dinilai masuk akal tetapi belum dibuktikan pada 3.7.15; AC-4 dan AC-6 yang menguncinya. *Update 2026-11-12*: keduanya terbukti pada 3.7.15 di run native milestone 2. Yang luput dari cross check ini adalah jendela verify untuk apply yang hanya mengganti backend; lihat *Update 2026-11-12* di atas.
