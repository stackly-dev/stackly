# 0045. Lockfile dan override lokal: alasan

Catatan keputusan untuk [index.md](index.md). `/develop` tidak membaca file ini.

## Context

Spec 0044 memberi tim satu file `stackly.yaml` yang bisa di commit. File itu sengaja longgar soal versi: `version: "18"` atau `stable` berarti "yang catalog Mac ini pilih hari ini". Dua rekan dengan versi Stackly berbeda, atau satu rekan di dua hari berbeda, bisa mendapat binary PostgreSQL yang berbeda dari manifest yang sama, tanpa ada yang memberi tahu mereka. REQ-023 meminta tiga hal dipisah: manifest yang dibagikan, lock yang mencatat pilihan tepat per platform, dan override yang hanya milik satu mesin. Ia juga meminta mesin tanpa artefak yang kompatibel gagal dengan jelas, bukan memakai versi lain diam diam, dan PRD 13.1 meminta `env up --locked` menolak resolusi ulang.

Sisi lain masalahnya adalah hal yang tidak boleh dibagikan. Port 5432 yang sudah dipakai di satu Mac, atau disk cepat tempat seseorang ingin menaruh data, adalah fakta mesin itu. Kalau satu satunya tempat menuliskannya adalah `stackly.yaml`, perubahan itu ikut ter commit dan mengganggu semua orang. BR-018 dan AC-080 menambahkan syarat keras: lock dan export tidak boleh memuat secret, path home, atau PID.

Fondasi yang sudah ada membatasi ruang gerak:

- Catalog bawaan adalah satu satunya sumber artefak yang dipercaya; `runtime::resolve` sudah punya jalur digest yang menolak artefak host sebagai pengganti (spec 0017, 0018).
- Identitas revision provider adalah `ProviderRevisionKey` (nama, revision, digest manifest dan template); catalog hanya memuat satu revision per provider pada satu waktu.
- Plan spec 0044 adalah method baca yang memakai evaluasi create, dan Up spec 0046 hanya menjalankan isi plan dengan `planDigest` sebagai pengikat.
- Member yang sudah ada tidak pernah diubah supaya cocok dengan manifest (spec 0044 AC-9); runtime instance hanya berganti lewat rebind yang mengecek data (spec 0025).
- Lokasi data instance berasal dari tabel `storage_locations`, dan lokasi baru hanya terdaftar lewat `storage.set_location` (spec 0032).
- Create tidak punya masukan lokasi per instance.
- Setiap mutasi registry adalah operasi; `diagnostics.export_bundle` adalah preseden method bukan operasi yang menulis file ke tempat pilihan pengguna.
- Catalog hari ini hanya punya artefak `macos-aarch64`.

Kalau tidak diputuskan: Up akan punya logika lock sendiri yang berbeda dari plan, lock bisa menjadi jalan masuk binary dari luar catalog, dan override lokal akan berakhir sebagai edit manifest yang tidak sengaja ter commit.

## Options considered

Mode keputusan sesi ini `auto`: opsi yang direkomendasikan diambil, runner up dicatat. Tanda `berat` berarti mahal dibalik sesudah build.

### K1: arti `stackly env export` (`berat`)

PRD memakai kata "export" untuk dua hal: AC-023 ("environment diekspor dan diimpor ke user lain") dan PRD 13.3 ("port aktual auto-allocated … dihasilkan ke env output tanpa mengekspor secret secara default").

- **Diambil: export adalah keluaran variabel environment (host, port, dan alamat aktual tiap service), dan "environment yang diekspor ke user lain" di AC-023 adalah `stackly.yaml` ditambah `stackly.lock`.** Kedua file itu memang dirancang untuk dibagikan dan secara struktur tidak bisa memuat secret, path, atau PID, jadi AC-023 diuji dengan memindahkan keduanya ke root lain (AC-21). Yang belum punya jalan sama sekali adalah kebutuhan harian developer: aplikasi mereka perlu tahu port mana yang dipakai, terutama port `auto`. Kontra: tafsir "diekspor" di AC-023 bergantung pada file, bukan pada perintah `export`.
- Runner up: export menulis bundle definisi (manifest yang ditulis ulang dari model dan lock) ke sebuah folder. Pro: persis kalimat AC-023, dan berguna untuk berbagi di luar repo. Kontra: hampir sama dengan menyalin dua file; butuh emitter manifest; dan port aktual tetap tidak punya keluaran. AC yang berubah kalau ini dipilih: AC-14, AC-15, AC-17, dan langkah 4, 6, 7.
- Opsi ketiga: keduanya lewat `--format`. Kontra: satu perintah dengan dua arti dan dua bentuk keluaran (teks ke stdout lawan file ke folder).

### K2: format lock (`biasa`)

- **Diambil: YAML dengan `apiVersion` dan `kind`, ditulis emitter tetap, dibaca loader ketat spec 0044.** Satu loader untuk ketiga file, satu skema versi, dan komentar "jangan diedit" di baris pertama. Emitter sendiri menjamin byte tidak bergantung pada versi library. Kontra: emitter kecil yang harus dijaga golden.
- Runner up: JSON dengan key terurut. Pro: emit deterministik tanpa kode sendiri. Kontra: parser JSON bawaan menerima key ganda diam diam, tidak ada komentar, dan loader kedua.

### K3: nama file (`biasa`)

- **Diambil: diturunkan dari nama manifest (`<stem>.lock`, `.<stem>.local.yaml`).** Spec 0044 membolehkan nama manifest apa pun; dua manifest di satu folder tidak boleh berbagi satu lock. Untuk `stackly.yaml` hasilnya persis nama PRD.
- Runner up: selalu `stackly.lock` dan `.stackly.local.yaml`. Kontra: dua manifest di satu folder saling menimpa lock.

### K4: isi lock (`berat`)

- **Diambil: per service dedicated: provider dan key revision nya, selector saat dikunci, versi, line, dan artefak per target. Tanpa URL, waktu, digest manifest, port, settings, atau reference secret.** Selector per service membuat "basi" berarti "pilihan versi di manifest berubah", bukan "ada byte yang berubah", jadi mengubah port atau komentar tidak memaksa kunci ulang. Tanpa waktu dan nama host, file deterministik dan diff nya hanya berisi perubahan nyata. Kontra: lock tidak menjelaskan kapan dan oleh siapa ia dibuat (git yang menjawab).
- Runner up: tambah digest seluruh manifest. Pro: deteksi basi satu perbandingan. Kontra: setiap edit manifest membuat `--locked` gagal, dan tim akan berhenti memakainya.

### K5: lock bukan sumber artefak (`berat`)

- **Diambil: digest yang dikunci harus ditemukan di catalog yang dipercaya dan lolos gate install; lock tidak membawa URL.** PRD: "lock memilih versi konkret dari catalog yang dipercaya". Digest menjamin byte nya utuh, bukan bahwa byte itu boleh dijalankan; hanya catalog yang menjawab itu. Kontra: Stackly lama yang catalog nya belum mengenal artefak baru berhenti dengan `env.lock_artifact_unknown` sampai diperbarui, walau runtime itu mungkin sudah terpasang.
- Runner up: runtime terpasang dengan digest sama cukup, tanpa catalog. Pro: lebih sering jalan saat offline dengan app lama. Kontra: artefak yang ditarik dari catalog (misalnya karena masalah keamanan) tetap bisa dipakai untuk instance baru lewat lock.
- Ditolak: lock membawa URL unduhan. File dari orang lain bisa menunjuk binary apa pun.

### K6: lock dipakai juga tanpa `--locked` (`berat`)

- **Diambil: plan selalu memakai entri lock yang cocok; `--locked` menambah syarat bahwa lock harus ada dan cocok untuk semua service.** Ini perilaku yang orang kenal dari lockfile lain: file lock yang ada selalu dihormati, dan flag nya hanya melarang penyimpangan. GUI tidak butuh tombol mode. Kontra: tanpa flag, service yang lock nya basi memakai catalog; itu terlihat di plan tetapi tidak menghentikan apa pun.
- Runner up: lock hanya dibaca dengan `--locked`. Pro: dua mode yang sangat jelas. Kontra: lock yang sudah di commit tidak berpengaruh untuk Up biasa dan untuk GUI, jadi tim tetap berbeda versi kecuali semua ingat flag nya. AC yang berubah: AC-5, AC-8, AC-18, AC-19.

### K7: entri cocok yang tidak bisa dipenuhi selalu blocker (`biasa`)

- **Diambil: blocker di kedua mode** (tidak ada artefak untuk target ini, digest tidak dikenal, revision provider berbeda). Satu kalimat aturan: entri yang cocok dipenuhi persis atau menghentikan. Itu juga yang diminta AC-023 ("tanpa auto-substitution"). Kontra: pembaruan app yang membawa revision provider baru menghentikan Up biasa sampai lock diperbarui.
- Runner up: tanpa `--locked`, revision provider yang berbeda hanya peringatan. Pro: lebih jarang menghalangi. Kontra: "terkunci" jadi punya dua arti, dan revision lain bisa merender config lain.

### K8: lock tidak menggerakkan member (`berat`)

- **Diambil: lock hanya untuk item `create`; member yang berbeda dilaporkan lewat `differences` `lock`, bukan blocker, juga dengan `--locked`.** Mengikuti aturan spec 0044 bahwa instance yang ada tidak diubah, dan aturan repo bahwa runtime hanya berganti lewat rebind yang mengecek data. `--locked` menjanjikan "tidak ada resolusi ulang", dan memang tidak ada. Kontra: CI yang ingin "persis lock atau gagal" hanya mendapat itu di root baru.
- Runner up: dengan `--locked`, member yang berbeda menjadi blocker. Pro: janji lebih kuat. Kontra: rekan yang instance nya dibuat sebelum lock naik versi tidak bisa Up sampai meng upgrade manual, padahal data dan service nya sehat. AC yang berubah: AC-8.

### K9: `env lock` mempertahankan entri yang masih berlaku (`biasa`)

- **Diambil: default mempertahankan, `--update` memilih ulang semua atau service yang disebut.** Menambah satu service tidak ikut menaikkan versi lima service lain. Kalau hanya revision provider yang berubah dan artefaknya masih ada, versi dipertahankan dan hanya key nya yang diperbarui. Kontra: aturan "masih berlaku" harus diuji teliti.
- Runner up: selalu pilih ulang semuanya. Pro: sederhana. Kontra: setiap kunci ulang bisa menaikkan versi tanpa diminta.

### K10: siapa yang menulis lock (`biasa`)

- **Diambil: daemon, lewat method `env.lock` yang bukan operasi, dengan `write_file_atomic`.** Satu penulis untuk GUI dan CLI, byte dijamin sama, dan polanya sudah ada di `diagnostics.export_bundle`. Tidak ada tulisan registry, jadi tidak butuh mesin operasi. Kontra: daemon menulis ke repo pengguna; path nya diturunkan daemon dari lokasi manifest supaya klien tidak bisa menunjuk file lain.
- Runner up: daemon membalas byte, klien yang menulis. Pro: daemon tetap pembaca murni. Kontra: dua penulis (Rust dan Swift) dengan aturan atomik masing masing.
- Opsi ketiga: operasi `lock_environment`. Kontra: `OperationKind`, idempotency key, dan audit untuk sesuatu yang tidak menulis registry dan sudah punya jejak di git.

### K11: `environments.lock_sha256` (`biasa`)

- **Diambil: digest lock yang dipakai Up terakhir, ditulis Up lewat `set_manifest`; import tidak mengubahnya dan `env.lock` tidak menyentuh registry.** Setara dengan `manifest_sha256` (yang diterima terakhir). Aturan "import file yang sama tidak mengubah baris" di spec 0044 tetap utuh. Disepakati dengan `arc-f46-spec`.
- Runner up: `env.lock` yang menulisnya. Kontra: `env.lock` jadi mutasi registry dan butuh operasi (K10).

### K12: cakupan override lokal (`biasa`)

- **Diambil: hanya port endpoint dan lokasi data, hanya untuk service dedicated.** Itu yang disebut scope dan PRD. Versi adalah urusan manifest dan lock; settings dan secret adalah urusan tim. Service `instanceRef` tidak bisa dikonfigurasi siapa pun (VAL-014).
- Runner up: boleh juga `settings`. Kontra: perilaku service berbeda per Mac tanpa terlihat di manifest; persis yang lock ingin cegah.

### K13: override storage menunjuk lokasi terdaftar (`berat`)

- **Diambil: `dataLocation` harus menunjuk sebuah lokasi data di `storage_locations`; create mendapat `data_location` opsional yang dibaca `gather_locations`, dan commit tetap lewat `InsertInstanceAt` yang sudah ada.** Aturan lokasi (izin, volume, subfolder `Stackly`) sudah ada di spec 0032 dan tidak diulang. Create berubah sekecil mungkin. Kontra: hari ini lokasi baru hanya terdaftar dengan menjadikannya default, jadi pemakaiannya canggung sampai spec 0032 punya cara menambah tanpa mengganti default (Follow-up).
- Runner up: Up mendaftarkan lokasi baru dari path di file override. Pro: satu langkah untuk pengguna. Kontra: file teks di repo memicu pembuatan folder dan baris registry di luar alur Settings, dan aturan lokasi harus dijalankan di dalam operasi Up milik spec 0046. AC yang berubah: AC-11 dan pekerjaan spec 0046.

### K14: alias tak dikenal di file override hanya peringatan (`biasa`)

- **Diambil: `warning`, override diabaikan dan ditampilkan.** Manifest berubah karena commit rekan; file lokal yang tertinggal tidak semestinya menghentikan plan. Salah lain di file itu tetap `error`, karena kamu sendiri yang menulisnya. Runner up: `error`. Kontra: `git pull` bisa mematahkan plan karena file yang tidak ikut repo.

### K15: lock dan override adalah urusan plan, bukan validate (`biasa`)

- **Diambil: `env.validate` tidak membaca keduanya.** Spec 0044 K8: validate memberi hasil sama di Mac mana pun. Lock bergantung pada target mesin dan override memang milik mesin. Runner up: validate ikut memeriksa file override. Kontra: validate di CI gagal karena file yang tidak ada di repo.

### K16: export tanpa connection string dan tanpa opsi secret (`biasa`)

- **Diambil: hanya host, port, dan alamat endpoint IP.** Tidak ada cara aman menaruh password di stdout, dan URL tersamar (`****`) tidak bisa langsung dipakai aplikasi. Endpoint unix dilewati karena path nya ada di bawah home. Kontra: aplikasi harus menyusun URL sendiri.
- Runner up: tambah `STACKLY_<ALIAS>_URL` tersamar. Pro: bentuk URL terlihat. Kontra: nilai yang tampak seperti URL tetapi gagal dipakai; bisa ditambah nanti tanpa merusak apa pun.

### K17: teks export dibangun daemon dan lewat `Redactor` (`biasa`)

- **Diambil.** GUI dan CLI menyalin teks yang sama (REQ-028), dan Follow-up spec 0011 meminta jalur ini lewat `Redactor` dengan test sentinel. Nilainya hanya angka dan alamat loopback, jadi `Redactor` di sini lapisan kedua, bukan satu satunya penjaga: `render` juga menolak karakter di luar daftar. Runner up: klien yang menyusun teks dari `variables`. Kontra: dua penyusun, dua aturan quoting.

### K18 sampai K21: keputusan kecil (`biasa`)

| K | Keputusan | Runner up |
| --- | --- | --- |
| K18 | batas: lock 256 KiB dan 16 target per service, override 64 KiB | tanpa batas (anggaran frame dan waktu baca tidak terjaga) |
| K19 | `env lock` hanya memakai catalog di memori, tanpa jaringan | menyegarkan catalog lebih dulu (belum ada sumber catalog jaringan; PRD hanya berkata "boleh") |
| K20 | identitas provider di lock adalah `ProviderRevisionKey` | tambah digest seluruh paket (hanya ada untuk provider lokal; `versions.json` tidak perlu dikunci karena artefak dikunci sendiri) |
| K21 | GUI: dua baris Details, tiga item menu More, dan catatan di sheet plan; tanpa tombol mode locked | sheet khusus untuk lock (belum ada kebutuhan yang tidak terjawab sheet plan) |

## Rationale

Tiga gaya membentuk keputusan. Pertama, lock menyentuh rantai trust. Catalog adalah satu satunya tempat Stackly memutuskan binary mana yang boleh dijalankan, jadi lock dibuat sebagai catatan pilihan di atas catalog itu, bukan sumber kedua (K5). Akibatnya lock tidak butuh tanda tangan, tidak butuh review keamanan sendiri, dan file dari orang lain paling buruk hanya menghasilkan blocker.

Kedua, spec 0044 dan 0046 sudah sepakat bahwa Up hanya menjalankan plan yang digest nya ditinjau. Menaruh seluruh aturan lock dan override di plan (K6, K7, K15) berarti tidak ada satu baris logika lock pun di Up, GUI, atau CLI, dan `--locked` cukup menjadi satu boolean yang ikut dihitung `planDigest`. Kontrak ini disepakati langsung dengan architect spec 0046.

Ketiga, data pengguna tidak boleh bergerak karena sebuah file teks berubah. Itu sebabnya lock tidak mengganti runtime member (K8), override tidak diterapkan ke instance yang ada, dan override storage hanya boleh menunjuk lokasi yang sudah lolos aturan Settings (K13). Harganya, "terkunci" berarti "yang dibuat sejak sekarang"; itu ditulis terang di Consequences dan terlihat di plan sebagai perbedaan.

Untuk export, tafsir yang dipilih (K1) adalah yang menutup kebutuhan yang belum punya jalan sama sekali. Berbagi definisi sudah terjawab oleh dua file yang memang untuk di commit; port aktual belum. Keputusan ini ditandai `berat` supaya lead dan kamu bisa membaliknya sebelum build kalau maksud PRD adalah bundle definisi.

## Koordinasi dengan sesi paralel

- Kontrak `--locked` dengan `arc-f46-spec` (spec 0046), disepakati 2026-11-13: Up tidak pernah me resolve sendiri dan hanya memakai isi `PlanItem`; `locked` adalah masukan plan yang Up teruskan apa adanya; semua kode blocker lock dan override milik spec ini dan tampil sebagai `EnvPlan.blockers`; `planDigest` diperluas di sini; Up menulis `lock_sha256` dari `EnvPlan.lock.sha256`; Up meneruskan `PlanItem.storage.locationId` ke `CreateDraft.data_location`; Up tidak pernah menulis file lock atau override.
- File satu pemilik saat build (`crates/protocol/`, `protocols/`, `schemas/`, hasil codegen) hanya disentuh langkah 1, 3, dan 5, sesudah langkah yang sama di spec 0044 masuk.

## Cross check

Dijalankan 2026-11-13 oleh model lain (Opus) sebagai pembaca saja, sesuai aturan umum koordinasi. Ia menemukan 18 butir; mode `auto` menerapkan perbaikan yang direkomendasikan, semuanya sebelum spec diajukan ke lead:

- **Salah fakta, diperbaiki**: create sudah commit lewat `WriteOp::InsertInstanceAt` dan memilih lokasi di `gather_locations`, jadi tidak ada perubahan `WriteOp` (draft pertama menyalin kalimat lama spec 0032); `target_mismatch` tidak bisa datang dari hasil `resolve`, jadi digest dicari dulu di `entry.artifacts()`.
- **Celah keputusan, ditutup**: gate trust sebagai langkah pertama *Runtime dari lock*; klien memeriksa `lock.locked` supaya daemon lama tidak me resolve ulang diam diam (`cli.env_locked_unsupported`); urutan cek karakter lalu `Redactor` di export, dengan variabel yang tersentuh masuk `skipped`; entri dipertahankan saat host hanya butuh artefak target baru; lock ber `apiVersion` asing tidak pernah ditimpa dan lock rusak hanya diganti dengan `--update`; kecocokan `dataLocation` (canonicalize penuh, `<p>` dan `<p>/Stackly`, lokasi bawaan tidak bisa disebut); `--update` dan `--service` menggantikan `--update [<service>...]` yang ambigu dengan argumen posisi; masukan `env.lock` dan `env.export` yang belum didefinisikan; isi `PlanLock` untuk setiap state; host dan aturan `override_endpoint_conflict`; perbandingan selector lewat `RuntimeSelector`.
- **Diterima apa adanya**: file lock ditulis dengan mode 0600 bawaan `write_file_atomic` (git tidak mencatat mode selain bit executable; dicatat di Consequences). Runner up: parameter mode di port `Filesystem`.
- **Tidak diambil**: menggagalkan seluruh export saat `Redactor` mengubah satu nilai; variabel itu cukup dilewati dengan alasannya.

## Bahan yang dibaca

- `prd.md`: REQ-023, REQ-028, BR-018, AC-023, AC-080, bagian 13.1 dan 13.3 (hanya sebagai input spec).
- `docs/scope/08-environment-proyek.md` dan aturan umum koordinasi scope 08.
- Spec 0044 (seluruhnya, terutama *Titik sambung*), dan bagian yang dirujuk dari spec 0001, 0005, 0011, 0017, 0018, 0021, 0032, 0034, 0037.
- Kode: `crates/core/src/runtime/resolve.rs`; `crates/core/src/catalog/` (`model.rs`, `install.rs`, `trust.rs`); `crates/core/src/provider/` (`versions.rs`, `validate.rs`); `crates/core/src/instance/create/` (`draft.rs`, `evaluate.rs`, `facts.rs`); `crates/domain/src/` (`environment.rs`, `provider.rs`, `target.rs`, `registry/import.rs`); `crates/platform/src/fs.rs`; `apps/daemon/src/ipc/diagnostics.rs`; `apps/cli/src/exit.rs`; `providers/postgresql/provider.yaml`; migrasi `0001_registry.sql`.
- Mockup `docs/mockups/Stackly.dc.html` dan `stackly-data.js` (baris Lock dan Local override di halaman Environments).
