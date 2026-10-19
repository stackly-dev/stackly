# Handoff: usulan fase 02 untuk spec foundation 0001–0013

Dari: lead scope 02 (`02-instalasi-pertama`). Untuk: lead program `01-foundation`. Tanggal: 2026-10-20.

> Catatan lead scope 02: inventaris ini dibuat dari spec 0015 sampai 0020, 0022, dan 0023 di `main` pada 2026-10-20 (workflow `wf_a3a107f4-1d9`). Usulan spec 0021 (start, readiness & connection) untuk spec 0004, 0009, dan 0011 ditambahkan setelah spec itu di-merge.

## 1. Pendahuluan

Spec fase 02 (0015 sampai 0023) dirancang setelah spec foundation 0001 sampai 0013 diterima. Desainnya butuh tambahan di spec foundation, yaitu tipe dan method baru, baris tabel kontrak, kode error, dan aturan check-deps. Beberapa klarifikasi juga membalik keputusan lama. Kebutuhan itu tersebar di bagian *Usulan update spec lain*, *Jawaban untuk follow up spec lain*, dan *Follow-up* di spec fase 02. Sebagian lagi hanya tersirat di badan desain. Dokumen ini menginventaris semuanya terhadap teks spec foundation dan kode di main.

Untuk dokumen ini tidak ada spec foundation yang diedit. Hanya satu perubahan fase 02 yang sudah masuk ke teks foundation, yaitu port `BackgroundAgent` dari spec 0015 di spec 0004 (sesi `arc-f15-spec`, sudah merge). Semua perubahan lain masih berupa usulan. Hampir semuanya additive terhadap kontrak yang sudah dibangun. Yang mengubah keputusan build yang sedang berjalan dikumpulkan di bagian 4.

Konvensi:

- `NNNN:LL` berarti baris di `docs/specs/NNNN-*/index.md`, kecuali disebut lain (misalnya `rationale.md`).
- "(implisit)" berarti usulan itu tidak ada di bagian Usulan spec sumber dan ditemukan dari badan desain atau dari kode.
- Timing: **sekarang** (spec pemilik tidak sedang dibangun), **lipat ke build** (spec sedang dibangun dan update teksnya masuk sebelum langkah tertentu), **setelah build mendarat** (spec dipegang sesi yang belum merge).

Di repo ada beberapa perbedaan status dibanding brief foundation (diperiksa 2026-10-20):

- **F9 sudah ditutup di main.** Semua kotak di `docs/scope/01-foundation.md:174-183` tercentang, dan branch closeout (edf1ba0) sudah merge di 667510b. Tidak ada sesi F9 yang masih memegang `docs/specs/0009-*` atau `crates/core/src/provider/`. Header spec masih `Status: In Progress` (0009:4).
- **F8 sudah lebih jauh dari brief.** Kode milestone 2 (bf5d98f: operation list, inspect, cancel, resume, abandon, `--wait`) sudah di main, walau kotak `01-foundation.md:160` belum dicentang. Verify dan Test masih terbuka.
- **F10 baru sebagian di main.** Yang ada baru langkah 1 (spike), langkah 2 (4311e5c: `crates/domain/src/process.rs`, `supervisor.rs`, `crates/platform/src/process.rs`) dan langkah 4 (b2a6b85: `fake_process.rs`). Langkah 2a dan 3 belum di main, dan langkah 5–15 belum mulai. Worktree `f10-process-port-2e464d` tidak punya perubahan tertunda.
- **F11 sebagian sudah merge.** Adapter Keychain sudah merge walau kotak langkah 1–3 belum dicentang. Langkah 5, 7, 8, dan 9–11 belum dibangun.
- **F7:** langkah 13 (protocol) ada di main. Langkah 14–15 dan closeout ada di branch `claude/f7-daemon-closeout-7fb5d1`, yang belum merge.
- **F13:** langkah 15, langkah 17, dan bagian daemon langkah 18 ada di branch `claude/f13-wiring-7b3a78`, yang belum merge. Branch ini juga mengedit `docs/specs/0013` index.md.

## 2. Ringkasan per spec

Totalnya 157 usulan. Rinciannya per spec ada di tabel "Peta usulan" pada bagian 3.

| Spec | Fitur pemilik | Jumlah usulan | Timing usulan | Dibutuhkan oleh (fase 02 milestone) |
|---|---|---|---|---|
| 0001 Stack & arsitektur | F1 (done) | 5 | Sekarang | F20 M1 (prasyarat), M2 s11; F15 M1 s5-7; F18 M1 s3-4, M2 s7-8; F17 M2 s7; F23 M3 s10 |
| 0002 Coding standards & tooling | F2 (built, verified, tested) | 3 | Sekarang; tick 0002:460 setelah F18 M5 | F15 M2 s8; F18 M3 s13 (lalu F19 s11, F23 s9) |
| 0003 Sumber & packaging runtime | F3 (built, verified, tested) | 5 | Sekarang | F18 M1 s1 (prasyarat), M3 s13, M5 s16; F15 M2 s8 |
| 0004 Domain core & platform | F4 (done) | 17 | Sekarang; urutan kode: `Instance::new` sebelum F10 s13 | F18 M1 s2, M2 s6-8; F20 M1 s1; F22 M1 s1-2; F15 M1 (teks sudah ada) |
| 0005 Data model registry | F5 (done) | 9 | Sekarang (teks); kode setelah F10 s2a, satu per satu | F18 M2 s6; F20 M1 s2 |
| 0006 Protokol IPC & daemon | F6 (done; sesi Contract 1.4 berjalan) | 11 | Setelah sesi Contract 1.4 merge | F15 M1 s3-4, s6; F17 M1 s5-6; F18 M3 s10-11; F20 M2 s8-9; F22 M1 s3 |
| 0007 Operation engine | F7 (Build it terbuka; langkah 14-15 di branch closeout) | 7 | Setelah `claude/f7-daemon-closeout-7fb5d1` merge | F20 M1 (prasyarat) s3, s7, M3 s14; F18 M3 s11 |
| 0008 CLI client | F8 (Build it terbuka) | 11 | Setelah build F8 mendarat; teks `CLI_SCHEMA_VERSION` 2 ikut 0015 M1 s6 | F15 M1 s3, s6; F20 M2 s10; F22 M2 s5 |
| 0009 Provider engine & manifest | F9 (ditutup di main) | 30 | Sekarang | F17 M1 s1-3; F18 M1 s1, s8, s13; F19 M1 s1-3; F20 M1 s4; F23 s1-2 |
| 0010 Process supervisor | F10 (Build it terbuka) | 24 | Lipat ke build F10: update teks sekarang, sebelum langkah 5 | F18 s8; F19 M2 s5-6; F20 M1 (prasyarat) s6-9; F22 M1 s3-4; F23 s5-7; F15 M2 s9 |
| 0011 Secret store | F11 (Build it terbuka) | 10 | Lipat ke build F11, sebelum langkah 5 | F19 M1 s3; F20 M1 (prasyarat) s6-7, M2 s10, M3 s13-14; F15 M4 s15 |
| 0012 Design system | F12 (built, tested; Verify terbuka) | 5 | Sekarang | F16 M1 s3; F17 M3 s8-10; F18 M4 s15; F20 M3 s13-14; F22 M3 s8 |
| 0013 App shell | F13 (M4 berjalan) | 20 | Lipat ke F13 M4: setelah `f13-wiring` merge, sebelum langkah 16 | F15 M3 s10-12; F16 M1 s1-3, M2 s4; F17 M3 s9-10; F18 M4 s14-15; F20 M3 s12, s14; F22 M3 s6-8 |

## 3. Perubahan per spec

### Spec 0001: Stack & arsitektur

**Status.** F1 done (`01-foundation.md:9`), spec berstatus Accepted, dan tidak dipegang sesi mana pun. Di `Cargo.toml` belum ada objc2, objc2-foundation, objc2-service-management, clap_complete, clap_mangen, ureq, tar, flate2, zip, object, maupun socket2. `xtask/src/check_deps.rs` belum punya satu pun aturan yang diusulkan. `check_deps.rs:165` menolak dependency stackly-cli ke stackly-core dari jenis apa pun.

**Timing:** sekarang.

Peta usulan:

| # | Sumber | Usulan | Item |
|---|---|---|---|
| P1 | 0015:490 (AC-3 0015:24; 0015:344-354; Follow-up 0015:563) | Baris *Batas keputusan* fitur 15 (0001:211) diputuskan oleh spec 0015. objc2, objc2-foundation, dan objc2-service-management hanya di stackly-platform-macos. clap_complete dan clap_mangen hanya di stackly-cli. Ditambah aturan check-deps-nya | 1, 3, 4a |
| P2 | 0018:347 (0018:46, 0018:73-86; Follow-up 0018:729; urutan 0018:706) | Baris fitur 18 (0001:212) diputuskan: ureq 3 di port `Fetcher`, extractor di atas tar/flate2/zip, Mach-O lewat object. Peta crate dan aturan check-deps diperbarui. Literal `/usr/bin/codesign` hanya boleh di modul `codesign`. ISC masuk allow list deny.toml | 2, 3, 4b, 4c, 6 |
| P3 | 0020:640-652 (0020:583, 0020:976, rationale 0020:525) | Tambah `socket2` di baris stackly-core (0001:66), dengan teks pengganti persis | 3 |
| P4 (implisit) | 0020:43 (AC-21), 0020:993, 0020:1038, 0020:1053 | Aturan check-deps baru: tolak nama atau ID provider di Swift `CreateInstance/` dan di Rust `instance::create` | 4d |
| P5 (implisit) | 0017:238, :305, :579; 0023:194, :420; rationale 0017:510 | stackly-cli memakai stackly-core (feature `test-support`) sebagai dev dependency supaya bisa memakai `CatalogBuilder` di test binary | 5 |

Daftar perubahan gabungan:

1. **Batas keputusan 0001:211 (fitur 15).** Isi dengan "spec 0015: stacklyd dan stackly di Contents/MacOS, LaunchAgent lewat SMAppService.agent, assembly dan signing development di `cargo xtask build-app` (ad hoc default, `--identity`), hardened runtime tanpa entitlement, identifier sementara di `stackly-platform::bundle`" (0015:490). Di edit yang sama, samakan 0001:185, 0001:187, dan 0001:193 dengan 0015:128-144. Isinya: xcodebuild dengan `CODE_SIGNING_ALLOWED=NO`, xtask menandatangani dari dalam ke luar dengan `--options runtime` tanpa entitlement, dan output ke `target/app/<Config>`. Bagian ini implisit karena 0015 tidak menyebut baris-baris tersebut.
2. **Batas keputusan 0001:212 (fitur 18).** Isi dengan "spec 0018: ureq 3 (rustls, rustls-platform-verifier, tanpa dekompresi) di port `Fetcher`; extractor sendiri di atas tar, flate2, zip; Mach-O lewat object" (0018:347).
3. **Peta crate.**
   - Baris 0001:66 (stackly-core): terapkan pengganti persis dari 0020. Ganti "`getrandom` (spec 0011);" dengan "`getrandom` (spec 0011); `socket2` untuk bind percobaan `PortProbe` tanpa `listen` dan dengan opsi socket yang dipilih pemanggil (spec 0020);". Setelah itu tambahkan tar, flate2 (backend pure Rust), zip (`default-features=false`, deflate saja), dan object (`default-features=false`, read_core, macho, std) untuk spec 0018 (0018:347).
   - Baris 0001:67 (stackly-platform-macos): tambahkan objc2, objc2-foundation, objc2-service-management (spec 0015) dan ureq 3 dengan `default-features=false` serta feature rustls dan platform-verifier (spec 0018).
   - Baris 0001:74 (stackly-cli): tambahkan clap_complete dan clap_mangen (spec 0015).
   - Cerminkan semuanya di daftar workspace dependency 0001:177.
4. **Aturan arah dependency (0001:83-101).**
   - (a) objc2* hanya di stackly-platform-macos. clap_complete dan clap_mangen hanya di stackly-cli (0015:24).
   - (b) ureq hanya di stackly-platform-macos. tar, flate2, zip, dan object hanya di stackly-core, ditambah xtask dan `fuzz/` (0018:86).
   - (c) Ubah kalimat 0001:92 supaya menyebut daftar `MACHINE_PATHS` ditambah aturan platform-macos berikut. Di kode non-test, di antara literal yang diawali `/usr/bin/`, `/usr/sbin/`, `/bin/`, dan `/Library/Developer/`, hanya `/usr/bin/codesign` yang boleh, dan hanya di modul `codesign` (0018:86).
   - (d) Larangan nama dan ID provider di `instance::create` dan Swift `CreateInstance/`, plus catatan di 0001:100 bahwa check-deps kini memindai Swift untuk aturan ini (0020:43, 0020:993).
5. **Pengecualian 0001:88.** Putuskan apakah stackly-cli boleh bergantung pada stackly-core hanya sebagai dev dependency dengan `features = ["test-support"]`. `xtask/src/check_deps.rs:165` berubah di commit build 0017 langkah 7. Keputusan ini harus ada sebelum 0017 langkah 7.
6. **Opsional, satu paragraf dengan 0018:86.** Catat izin lisensi ISC (ring, rustls, untrusted) di sebelah baris cargo-deny (0001:52).
7. **Opsional: drift yang tidak diusulkan spec fase 02 mana pun.**
   - Baris xtask 0001:75 mendapat `catalog` (0017) dan flag build-app `--release`, `--identity`, `--install` (0015).
   - Layout repo 0001:144 dan 0001:146 mendapat `providers/catalog.toml`, `providers/catalog-revisions.json` (0017), dan `evidence/compatibility/` (0018).
   - 0001:163 dan 0001:165 mendapat newtype `SignedSafeInteger` yang dipetakan ke `Int64` (0020:253).
   - Follow-up 0003:611 (`repository` di `[workspace.package]` menjadi `https://github.com/stackly-dev/stackly`) juga mengantre edit ke spec 0001. Sesi yang sama bisa mengambilnya.

Konflik dan resolusi:

1. **0018 dan 0020 mengedit baris yang sama (0001:66).** Jadikan satu edit. Teks lama yang dikutip 0020 masih cocok persis dengan main.
2. **0001:88 berbenturan dengan `CatalogBuilder` di test CLI (0017:238, 0017:305, 0023:194).**
   - Rekomendasi: izinkan stackly-cli ke stackly-core hanya sebagai dev dependency dengan `features = ["test-support"]`. Dependency normal tetap dilarang, mencerminkan aturan feature `fixture` di 0001:98. `check_deps.rs:165` lalu memeriksa jenis dependency dan feature-nya.
   - Alternatif tanpa ubah aturan: 0017 langkah 7 membangun folder catalog fixture lewat `cargo xtask catalog build`, lalu test CLI mengarahkan `STACKLY_CATALOG_DIR` ke folder itu.
3. **0001:92 berbenturan dengan literal codesign dari 0018.** Cukup perbaikan kata-kata. Kode memang hanya melarang prefix `MACHINE_PATHS` (`xtask/src/check_deps/source_scan.rs:94-100`).
4. **0001:100 berbenturan dengan aturan check-deps Swift dari 0020.** Pilih salah satu: catat pengecualiannya di 0001, atau minta build 0020 menerapkan aturan itu sebagai test source Swift di bawah spec 0013.
5. **Cakupan pencarian.** Kedelapan spec sudah diperiksa. 0016, 0019, dan 0022 tidak punya usulan untuk 0001. 0023 hanya menyentuh 0001 secara tidak langsung, lewat `CatalogBuilder` di `apps/cli/tests`.

### Spec 0002: Coding standards & tooling

**Status.** F2 built, verified, dan tested (`01-foundation.md:32-42`). Spec berstatus In Progress dan tidak dipegang sesi. 0002:159 hanya menyebut `evidence/runtime/`, 0002:173 menyebutnya satu-satunya evidence otomatis di git, dan Follow-up 0002:460 masih terbuka.

**Timing:** sekarang. Hanya tick 0002:460 yang menunggu.

Peta usulan:

| # | Sumber | Usulan | Item |
|---|---|---|---|
| P1 | 0015:489 (0015:454; Follow-up 0015:563) | `cargo xtask runtime spike --cleanup` juga melakukan bootout job launchd `dev.stackly.test.agent.*` tanpa mengambil lock spike | 4a |
| P2 | 0018:349 (Follow-up 0018:730) | Folder `evidence/compatibility/<runId>/` ditambahkan. Kalimat 0002:173 dan sumber fitur 65 di 0002:429 menyesuaikan | 1, 2, 3 |
| P3 | 0018:349 (klausa pertama), 0018:730 | Follow-up 0002:460 terjawab oleh usulan 0003 (TERM lalu KILL untuk proses yang argv-nya memuat `/stackly-test-`). Tick lewat /sync setelah 0018 M5 | 4b, 5 |

Daftar perubahan gabungan:

1. **Pohon *Folder dan file* (0002:145-169).** Setelah blok `evidence/runtime/<runId>/` (0002:159-162), tambahkan `evidence/compatibility/<runId>/` berisi `report.json`, `output.log`, dan `artifacts/<id>/compatibility.json` (schema `stackly.compatibility-evidence/v1`). Folder ini ditulis oleh `cargo xtask runtime compat-record` (spec 0018) (0018:349).
2. **Kalimat pertama 0002:173.** Ganti dengan "`evidence/runtime/` dan `evidence/compatibility/` adalah satu satunya evidence otomatis di git, ditulis `runtime record` dan `runtime compat-record`, divalidasi `runtime check`". Samakan juga:
   - sisa kalimat 0002:173;
   - 0002:349: evidence check tidak membaca kedua folder, dan runtime check memvalidasi keduanya;
   - invariant 0002:389, yang berlaku untuk kedua folder.
3. **Consequences 0002:429.** Fitur 65 membaca `evidence/compatibility/` sebagai evidence compatibility (BR-025), bukan evidence rilis. Opsional juga Neutral 0002:450 (0018:349).
4. **Menjalankan test di Mac ini (0002:337-339).** `--cleanup` juga:
   - (a) melakukan bootout semua job `dev.stackly.test.agent.*` di `gui/<uid>` tanpa lock spike, dan dilewati bila tidak ada GUI domain (0015:489, 0015:454);
   - (b) mengirim TERM lalu KILL ke proses user ini yang argv-nya memuat `/stackly-test-` (0018:330).

   Semantik lock dan urutan resminya ditunjuk ke spec 0003.
5. **Follow-up 0002:460** dibiarkan untuk /sync lead, setelah 0018 M5 langkah 16 dan 0015 M2 langkah 8 mendarat.
6. **Opsional, tidak diusulkan.** Catat gate khusus test `STACKLY_NATIVE_NETWORK=1` (0018:614, 0018:622) di sebelah tabel native run (0002:330).

Konflik dan resolusi:

1. **0015 dan 0018 sama-sama memperluas `--cleanup`.** Spec 0002 cukup menjelaskan apa yang jalan dan kapan, relatif terhadap native run. Semantik lock diserahkan ke spec 0003 supaya kedua spec tidak bergeser satu sama lain.
2. **Kasus tanpa GUI domain.** Menurut 0002:338, `--cleanup` keluar 0 bila tidak ada GUI domain, jadi bootout baru juga harus dilewati dalam kasus itu.
3. **Native run mencakup test launchd 0015.** 0002:330 menjalankan semua test ignored di native run, sehingga `apps/daemon/tests/native_launchd.rs` milik 0015 ikut jalan. 0015:454 menganggap native run yang dimulai user sebagai persetujuan, dan itu sesuai dengan 0002.
4. **Pre-commit hook.** 0017:130 menambah `cargo xtask catalog check` ke `.githooks/pre-commit`. Spec 0002 tidak mendeskripsikan hook (yang mendeskripsikan adalah AGENTS.md Tooling), jadi ini item /sync untuk AGENTS.md, bukan edit 0002.
5. **Siapa yang men-tick.** Tick follow-up dilakukan lead lewat /sync (log koordinasi `02-instalasi-pertama.log.md:39`).
6. **Cakupan pencarian.** 0016, 0017, 0019, 0020, 0022, dan 0023 tidak punya usulan untuk 0002. Mereka hanya memakai aturan evidence-nya (misalnya 0016:318, 0019:34, 0022:469, 0023:31).

### Spec 0003: Sumber & packaging runtime

**Status.** F3 built, verified, dan tested (`01-foundation.md:54-65`), dan tidak dipegang sesi. Di kode, `--cleanup` mengambil lock spike sebelum sweep apa pun (`xtask/src/runtime/spike/mod.rs:121-135`, lock di :126) dan hanya mencocokkan `stackly-runtime-spike-` dan `/tmp/sk-` (`cleanup.rs:261-265`). Inventory belum punya `stable`, `unpacked_size`, atau `entry_count`. `runtime check` baru punya aturan 1–9 (0003:429-437), dan `compat-record` belum ada.

**Timing:** sekarang. 0018 M1 langkah 1 tidak bisa mulai tanpa update ini (0018:655), dan log koordinasi :49 mengantrekannya untuk F18 M1.

Peta usulan:

| # | Sumber | Usulan | Item |
|---|---|---|---|
| P1 | 0015:489 (0015:454, :354, :409) | Langkah `--cleanup` tanpa lock: `launchctl list`, lalu `launchctl bootout gui/<uid>/<label>` untuk setiap label `dev.stackly.test.agent.*` | 4a |
| P2 | 0018:330 (AC-21 0018:42, AC-20 0018:41; Follow-up 0018:725) | Field inventory `stable`, `unpacked_size`, `entry_count`. Aturan runtime check 10–12. `runtime pin` dan `verify` memakai `stackly_core::runtime::archive::scan`. Perintah `runtime compat-record`. `--cleanup` mengirim TERM lalu KILL untuk proses `/stackly-test-`. Rekomendasi nilai `stable` | 1, 2, 3, 4c |
| P3 | 0018:725 (AC-9 0018:30) | Tick 0003:616, :618, :619, :620 | 6 |
| P4 (implisit) | 0018:275, 0018:718 | `xtask/src/runtime/spike/signing.rs` pindah ke stackly-platform-macos sebagai `MacosCodeSigning`, dan xtask memakai ulang parser-nya | 5 |
| P5 | 0019:473 | Hanya jawaban: instance PostgreSQL default memakai scram-sha-256 tanpa `trust`, dan "Spike spec 0003 tidak berubah" | 6 |

Daftar perubahan gabungan:

1. **Tabel field artefak (0003:131-154).**
   - `stable` (bool): paling banyak satu per provider, os, dan arch, dan hanya pada `available`.
   - `unpacked_size` dan `entry_count`: wajib pada `available` sejak aturan 11 aktif.
   - Rekomendasi nilai `stable`: line terbaru yang punya evidence native (PostgreSQL 18, MySQL 9.7), dan satu-satunya line untuk provider lain.
   - Opsional: tampilkan field ini di contoh 0003:158-188 (0018:330).
2. **Key invariants: setelah aturan 9 (0003:437).** Tambahkan aturan 10 (`stable`), aturan 11 (`available` mewajibkan `unpacked_size` dan `entry_count`), dan aturan 12. Aturan 12: setiap report di `evidence/compatibility/` lolos aturan compat-record dari 0018:312, yaitu:
   - penolakan yang sama dengan `runtime record`;
   - hanya rekaman PASS atau FAIL yang `covers`-nya memuat BR-025;
   - kind native, subject managed dengan digest, dan ada `artifacts/<recordId>/compatibility.json`;
   - gagal dengan "no record to keep" bila tidak ada yang memenuhi.
3. **API surface (0003:240-251).**
   - `runtime check` (0003:242) juga membaca `evidence/compatibility/*/report.json`.
   - Kedua baris `runtime pin` (0003:243-244) mencetak `unpacked_size` dan `entry_count` lewat `stackly_core::runtime::archive::scan`, di semua OS.
   - `runtime verify` (0003:246) membandingkan kedua nilai itu.
   - Tambahkan baris `runtime compat-record <report.json>` (0018:330).
4. **Tulis ulang `--cleanup`** sekali di AC-4 (0003:23), *Lock* (0003:227), *Rutin pembersihan* (0003:231), baris API 0003:248, dan test Lock 0003:484. Urutannya:
   - (a) Tanpa lock: bootout setiap job `dev.stackly.test.agent.*` dari `launchctl list`, dilewati tanpa GUI domain (0015:489).
   - (b) Ambil lock spike (tunggu 20 menit, pesan "spike in progress").
   - (c) Jalankan sweep yang sudah ada, ditambah TERM, tunggu 5 s, lalu KILL untuk proses user ini yang argv-nya memuat `/stackly-test-`. Cache tidak pernah disentuh (0018:330).

   AC-4 menyatakan bahwa hanya langkah (a) yang berjalan tanpa lock dan langkah itu tidak pernah menyentuh proses spike.
5. **Opsional:** Neutral 0003:598 mencatat bahwa `spike/signing.rs` pindah ke `MacosCodeSigning` (0018:275). 0003:601 ("xtask tidak mendapat crate baru") tetap benar, karena xtask sudah bergantung pada stackly-core (`xtask/Cargo.toml:21`) dan pada stackly-platform-macos di bawah cfg macos.
6. **Tick follow-up.** Keempat tick (0003:616 install via versions.json, :618 quarantine.outcome dan symlink arsip, :619 extractor aman dengan kasus AC-16 diulang terhadap zip reader 0018, :620 AppleDouble di tar_gz) dibiarkan untuk /sync lead setelah 0018 M1 langkah 3–5 dan M3 mendarat. 0019:473 tidak butuh edit, karena 0003:221 dan 0003:461 sudah menyatakan `trust` hanya untuk spike.

Konflik dan resolusi:

1. **Penempatan lock.**
   - Bootout agent dari 0015 tetap tanpa lock karena tidak pernah menyentuh proses spike. Janji AC-4 ("tidak pernah mematikan spike yang sedang berjalan") tetap berlaku asal pengecualiannya ditulis.
   - Kill `/stackly-test-` dari 0018 masuk di dalam lock.
   - Tulis juga bahwa sweep `/stackly-test-` membunuh run `cargo test` fixture yang sedang berjalan (prefix `TestRoot` `stackly-test-`, `crates/testkit/src/test_root.rs:7`). Tambahkan "jalankan hanya saat tidak ada test run aktif", sejalan dengan 0002:337.
2. **Cakupan pola.** `/stackly-test-` mencocokkan path `TestRoot`, tetapi folder run pendek `TestRoot` memakai `/tmp/skt-` (`test_root.rs:10`) dan tidak ikut tercocokkan. Arsitek bisa menerimanya atau menambahkan prefix itu. Pola spike `/tmp/sk-` sengaja tidak mencakup `skt-`.
3. **Urutan langkah di dalam 0018.** `pin` dan `verify` yang memakai `scan` ada di langkah 1 (0018:655), padahal `scan` baru dibangun di langkah 3 (0018:657). Ini perlu diberitahukan ke pemilik 0018, bukan diubah di 0003.
4. **Nomor aturan.** Nomor 10–12 masih bebas (0003:429-437 berakhir di aturan 9).
5. **Cakupan pencarian.** 0016, 0017, 0020, dan 0022 tidak punya usulan untuk 0003. 0023 hanya bergantung pada cleanup dari 0018 (0023:198).

### Spec 0004: Domain core & platform abstraction

**Status.** F4 done (`01-foundation.md:79-84`). Status spec "In Progress" hanya karena baris perluasan dibangun oleh fitur lain. Tidak ada branch atau worktree yang menyentuh `docs/specs/0004`, `crates/domain`, `crates/platform`, `crates/platform-macos`, atau `crates/testkit`. Pemegang terakhirnya, `arc-f15-spec`, sudah merge (log koordinasi :16, :40).

Yang sudah ada di main:

- `Filesystem` baru sampai `remove_tree` (`crates/platform/src/fs.rs:19-102`).
- `PlatformInfo` hanya punya `target()` dan `capabilities()`.
- `PlatformCapability::ALL` berukuran `[Self; 9]`.
- `Instance::new` infallible tanpa pemeriksaan endpoint (`crates/domain/src/instance.rs:286-311`).

**Timing:** sekarang. Urutan kodenya: perubahan `Instance::new` dari F20 langkah 1 sebaiknya masuk sebelum 0010 langkah 13, dan kata-kata 0004:610 sebaiknya beres sebelum 0010 langkah 15.

Peta usulan:

| # | Sumber | Usulan | Item |
|---|---|---|---|
| P1 | 0020:656-666 | Baris ID `ProviderSourceId` setelah 0004:116 | 1c, 2 |
| P2 | 0017:562 (K-2; milik F20 per 0017:559, :626, :635) | `ProviderSourceId` dan `ProviderSourceRecord { id, kind, location, trust_root, last_sync_at, status, revision, created_at, updated_at }` di modul domain `registry` | 1c, 2 |
| P3 | 0020:668-678 | Bullet "Belum dibangun" 0004:168: `Instance::new` menolak `DuplicateEndpoint`, `DuplicateSocket`, `BoundOnNew`, dan `EndpointSelfConflict` (BR-004) | 1c, 4, 15 |
| P4 | 0020:680-690 | Error mapping 0004:179: `EndpointSelfConflict` dipetakan ke BR-004 | 5 |
| P5 | 0020:692-709 | `Filesystem::volume_facts(path) -> Result<VolumeFacts, PlatformError>` (statfs `f_fstypename`, `MNT_LOCAL`), fake, dan conformance | 1c, 8, 9, 13, 16 |
| P6 | 0018:332-334 (0018:76-78, :136, :231-244, :289) | `open_read`, `open_append`, `rename_no_replace`, `available_space`, dan `open_tree_writer` dengan `TreeWriter`, `TreeFile`, `EntryPath`, `FileMode` | 1a, 8, 9, 13, 17 |
| P7 | 0018:335 (0018:148-178, :262-275) | Port `Fetcher` dan `CodeSigning` lewat `with_fetcher` dan `with_code_signing`, capability `network_fetch` dan `code_signing`, adapter Macos, dan fake | 1a, 7, 11, 12, 13, 14, 17 |
| P8 | 0018:336 | `PlatformInfo::host_facts() -> HostFacts`; `StopFlag` di modul `fetch`; domain `OsVersion`, `TeamId`, `ArtifactFormat` | 1a, 3, 10, 13, 15, 16 |
| P9 | 0018:337 | `Runtime::committed(new, verified_at, committed_at) -> Result<Runtime, TransitionError>` dengan revision 1 | 1a, 4, 15 |
| P10 | 0018:338 (0018:111) | `RuntimeSelector` di `runtime::selector` | 1a, 3 |
| P11 | 0018:339, 0018:726 | Tick Follow-up 0004:728 (`rename_no_replace` dibangun F18) | 18 |
| P12 | 0018:726 (dengan 0018:75) | Baris perluasan F18, dan BR-025 dipetakan ke spec 0018 | 1a, 1b, 6 |
| P13 | 0022:173-184, 0022:538 | `Filesystem::volume_space` dengan `VolumeSpace`, modul domain `capacity`, dan modul core `storage` | 1d, 8, 9, 13, 15, 16 |
| P14 | 0015:146-182, :209-212, :463 | Port `BackgroundAgent`, `bundle`, dan `agent_launch_record()`. **Teksnya sudah diterapkan** (0004:80, :373, :393, :427, :486, :509, :726). Kodenya belum | 7 (F15 sudah tercantum) |
| P15 | 0020:627 | Follow-up 0004:724 terjawab oleh `execute_init` spec 0010 (AC-13) dan 0020 AC-12 | 18 |
| P16 | 0020:628, 0020:25 | Follow-up 0004:731: bagian F20 (VAL-001) dirancang di 0020 AC-3. Bagian F9 (VAL-002) sudah ada di `crates/core/tests/provider_rules.rs:1238` dan :1275 | 18 |
| P17 | 0019:474, 0019:356 baris 5 | Tidak ada edit: `.s.PGSQL.<port>` paling panjang 14 byte, di bawah `SOCKET_NAME_MAX` 15. Konsisten dengan 0004:136 dan :434 | tidak ada |

Daftar perubahan gabungan:

1. **Tabel *Titik perluasan* (0004:54-80).** Tambahkan baris bertanda *(ditambah fitur N, spec NNNN)*:
   - (a) **F18 platform dan domain:**
     - `Filesystem` `open_read`, `open_append`, `rename_no_replace`, `available_space`, dan `open_tree_writer`, beserta `ReadSeekFile`, `AppendFile`, `TreeWriter`, `TreeFile`, `EntryPath`, `FileMode`;
     - port `Fetcher` (modul `fetch`, `HttpsUrl`, `StopFlag`) dan `CodeSigning` (modul `codesign`) lewat `with_fetcher` dan `with_code_signing`, dengan default Unsupported, adapter `Macos*`, dan fake `Fake*`;
     - `PlatformInfo::host_facts` dengan `HostFacts` (modul `info`);
     - domain `OsVersion`, `TeamId`, `ArtifactFormat`, `RuntimeSelector` (`runtime::selector`), dan `Runtime::committed` (0018:332-338, 0018:726).
   - (b) **F18 tipe registry di domain `registry`:** field `ArtifactRecord`, `CompatibilityRecordRow`, `SignatureInfoJson`, dan `DependencyManifestJson`, dengan detail di spec 0005 *Tambahan fitur 18* (0018:75, 0018:427).
   - (c) **F20:**
     - `ProviderSourceId`, `ProviderSourceRecord`, `Registry::provider_source_by_location`, `WriteOp::InsertProviderSource`, `WriteOp::DeleteInstanceCreatedBy`, `ProviderRevisionRecord.source_id`, `ResourceRef::ProviderSource`, dan `RegistryError::NotDeletable`, dengan detail di spec 0005 (0017:562, 0020:656-666, 0020:711-786);
     - validasi endpoint di `Instance::new`;
     - `Filesystem::volume_facts` (0020:668-709).
   - (d) **F22:** `Filesystem::volume_space` dengan `VolumeSpace`, modul domain `capacity`, dan modul core `storage` (0022:173-184, 0022:538).
2. **Tabel ID (0004:116).** Sisipkan baris `ProviderSourceId` persis seperti 0020:664: "`| ProviderSourceId | UUID v7, makro yang sama | ID baris provider_sources *(ditambah fitur 20, spec 0020 dan K-2 spec 0017)* |`".
3. **Tabel value type (0004:118-141).** Tambahkan, dengan tanda *(ditambah fitur 18, spec 0018)*:
   - `OsVersion`: 1 sampai 3 komponen numerik, diurutkan secara numerik;
   - `TeamId`: 10 karakter `[A-Z0-9]`;
   - `ArtifactFormat`: `tar_gz`, `zip`, `binary`;
   - `RuntimeSelector`: aturan parse di 0018:111.
4. **Tipe pendukung (0004:168).** Terapkan pengganti 0020:668-678 secara verbatim, dengan aturan `DuplicateSocket` sesuai resolusi 4. Tambahkan juga bullet setelah catatan state machine runtime (0004:204-206) untuk `Runtime::committed` (0018:337): fungsi ini menjalankan mesin `RuntimeState` murni dari `resolving` sampai `committed` dan mengembalikan revision 1.
5. **Error mapping (0004:179).** Terapkan pengganti 0020:680-690 secara verbatim: "`FixedPortConflict` dan `AutoPortUnavailable` ke BR-005; `EndpointSelfConflict` ke BR-004 (spec 0020);".
6. **Invariant BR-025 (0004:309).** Kolom pemilik menjadi "fitur 18 (spec 0018: compatibility record, support_status), 65".
7. **Intro port platform (0004:324).** Ubah menjadi "Fitur 10, 11, 15, 18, dan 28 menambah trait mereka sendiri".
8. **Blok kode `Filesystem` (0004:331-340).** Setelah `remove_tree`, tambahkan tiga kelompok method:
   - `// ditambah fitur 18 (spec 0018):` `open_read`, `open_append`, `rename_no_replace`, `available_space`, `open_tree_writer`;
   - `// ditambah fitur 20 (spec 0020):` `volume_facts`;
   - `// ditambah fitur 22 (spec 0022):` `volume_space`.

   Semuanya mengembalikan `Result<_, PlatformError>`. Jadi `available_space` menjadi `Result<u64, PlatformError>` dan `volume_space` menjadi `Result<VolumeSpace, PlatformError>`.
9. **Bullet setelah `remove_tree` (setelah 0004:361).**
   - Satu bullet untuk setiap method F18, yang menunjuk 0018:231-244 dan 0018:289. Untuk `rename_no_replace`: pemanggil menjalankan `check_owned_path` pada kedua path dengan root yang sama; port tidak pernah memeriksa kepemilikan; method ini tidak pernah mengganti atau menghapus.
   - Bullet `volume_facts` verbatim dari 0020:708.
   - Bullet `volume_space` dari 0022:173, dengan `PlatformError`.
   - `available_space` didefinisikan sebagai `volume_space(path)?.available`, yaitu provided trait method.
   - Ketiganya memakai satu helper statfs di `MacosFilesystem`.
10. **Blok kode `PlatformInfo` (0004:350-353).** Tambahkan `fn host_facts(&self) -> HostFacts;` (fitur 18) dan struct `HostFacts { target, os_version: OsVersion, os_build: Option<String> }` di modul `info`. Kebijakan kegagalannya mengikuti resolusi 7.
11. **Bullet `PlatformCapability` (0004:365).** Tambahkan `network_fetch` dan `code_signing` (fitur 18).
12. **Port yang ditambah fitur lain (0004:369-374).**
    - Bullet `Fetcher`: `with_fetcher`, getter `fetcher()`, default `UnsupportedFetcher` yang menjawab `FetchError::NotSupported`. `FakePlatform` memasang `FakeFetcher`, dan `MacosPlatform` memasang `MacosFetcher` setelah F18 M2 langkah 7.
    - Bullet `CodeSigning`: `with_code_signing`, `code_signing()`, `UnsupportedCodeSigning`, `FakeCodeSigning`, dan `MacosCodeSigning` setelah F18 M2 langkah 8.
    - Ubah 0004:374 menjadi "Fitur 28 mengikuti pola yang sama untuk metrik proses", karena F15 sudah tercantum.
13. **Fake platform (0004:481-490).**
    - `FakeFilesystem` mendapat entri `FsOp` (`crates/testkit/src/fake_fs.rs:22`) untuk semua method baru. `mutations()` (`fake_fs.rs:173`, aturan di 0004:482) ikut menghitung `rename_no_replace`, tulisan `open_append`, dan pembuatan oleh tree writer.
    - `FakeFilesystem` mendapat satu tabel volume per prefix path `{ total, available, fs_type, local }`, dengan default volume apfs lokal yang besar, injeksi error, dan delay yang bisa diatur (0018:635, 0020:708, 0022:173).
    - `FakePlatformInfo` mendapat `HostFacts` default (macos-aarch64, versi OS bisa diatur).
    - `FakePlatform` memasang `FakeFetcher` dan `FakeCodeSigning`.
    - `fs_conformance` mendapat pemeriksaan method F18 (termasuk test race symlink di platform-macos), pemeriksaan volume lokal (0020), dan kasus `volume_space` (0022).
14. **Windows dan capability (0004:494-512).**
    - Ubah menjadi "clock supported, sepuluh lainnya not supported".
    - Daftar `MacosPlatform` mendapat `network_fetch` dan `code_signing` sebagai not supported sampai F18 M2.
    - Tabel capability mendapat baris `network_fetch | trait Fetcher lewat with_fetcher | fitur 18, spec 0018 | not supported, supported setelah milestone 2 spec 0018`, dan baris yang setara untuk `code_signing`.
    - Tambahkan penanda "(ditambah fitur 18: NetworkFetch, CodeSigning)" di AC-13 (0004:33) dan AC-15 (0004:35, "lima capability lain") tanpa acceptance criterion F4 baru.
15. **API surface (0004:518-539).** `Instance::new` mengembalikan `Result<Instance, InvariantError>` (fitur 20; baris 0004:537). Tambahkan juga `Runtime::committed`, `capacity::level` dan `used_percent` (domain), `storage::capacity` (core, fitur 22), dan `host_facts`.
16. **Value sourcing (0004:555-593).**
    - Host facts berasal dari sysctl `kern.osproductversion` dan `kern.osversion`.
    - Volume facts dan space berasal dari statfs.
    - Ambang capacity berasal dari konstanta domain `CAPACITY_CRITICAL_AVAILABLE`, `CAPACITY_LOW_USED_PERCENT`, `CAPACITY_LOW_AVAILABLE`, dan `CAPACITY_FILLING_USED_PERCENT`.
17. **Key invariants dan Security model (0004:601, :610, :620).**
    - `rename_no_replace` tidak pernah mengganti atau menghapus.
    - 0004:610 mendapat pengecualian bernama: `MacosCodeSigning` hanya menjalankan `/usr/bin/codesign`, lewat helper `posix_spawn` privat di modul `codesign`. Ia tidak pernah menjalankan kode artefak dan tidak pernah memakai SIGKILL di luar `process/kill.rs`.
    - Daftar `unsafe` diperluas dengan statfs, renamex_np, openat, mkdirat, symlinkat, fchmodat, getxattr, dan sysctl.
18. **Follow-up (0004:719-733).**
    - Tick :724 dengan penunjuk 0020:627.
    - Tick :728, karena `rename_no_replace` dibangun F18.
    - Tick :731 dengan mengutip `provider_rules.rs:1238` untuk VAL-002 dan 0020 AC-3 untuk VAL-001.
    - Tambahkan baris tercentang yang mencatat bahwa baris perluasan F18, F20, dan F22 sudah diterapkan.

    Tick ini bisa dilakukan oleh sesi update bila lead mengizinkan. Kalau tidak, lewat /sync lead (log :39).

Konflik dan resolusi:

1. **Tiga method berbasis statfs (`available_space` dari 0018, `volume_facts` dari 0020, `volume_space` dari 0022).** Ketiga nama dipertahankan karena dikutip oleh AC dan test di masing-masing spec. `volume_space` menjadi primitif, dan `available_space` menjadi provided method di atasnya, seperti yang diizinkan 0022:173 dan 0022:538. `volume_facts` tetap terpisah karena membawa fs type dan `MNT_LOCAL`. Implementasinya satu helper statfs di `MacosFilesystem` dan satu tabel volume per prefix di `FakeFilesystem`.
2. **`Result<VolumeSpace, FsError>` di 0022 berbenturan dengan `PlatformError` di semua method (0004:331-340, `fs.rs:27-102`).** `FsError` tidak ada di mana pun di codebase. Pakai `PlatformError`. Normalisasi yang sama mengubah `available_space(path) -> u64` dari 0018 menjadi `Result<u64, PlatformError>`.
3. **0017:562 dan 0020:656-666.** 0017 ingin `ProviderSourceRecord` tercantum di 0004, sedangkan 0020 hanya menambah baris ID dan menaruh bentuk record di 0005. Tambahkan keduanya: baris ID, dan baris perluasan yang menyebut record itu dengan detail di spec 0005, sama seperti F10 dan F11 di 0004:77-78.
4. **Aturan `DuplicateSocket`.** 0020 menulis rule None, sedangkan baris BR-004 (0004:288) menyatakan dua endpoint Unix dengan nama socket sama konflik di bawah BR-004. Rekomendasi: rule BR-004, selaras dengan `EndpointSelfConflict`. Alternatifnya: tetap None dan tambahkan satu kalimat di baris BR-004. Keputusannya di tangan arsitek.
5. **`Instance::new` menjadi fallible.**
   - Ada lima pemanggil di main: `crates/storage/tests/registry/common.rs:128`, `crates/domain/tests/registry_port.rs:74`, `instance_aggregate.rs:23`, `state_machines.rs:361`, dan `business_rules.rs:51`.
   - `common.rs:127-141` membuat instance baru dengan endpoint yang sudah terisi `bound` (`common.rs:259-271`, `Some(5432)` dan `Some(49152)`), yang akan ditolak oleh `BoundOnNew`. Endpoint seperti itu harus dibangun lewat `Instance::restore` atau `set_bound_port`.
   - F20 M1 langkah 1 memperbarui semua pemanggil ini.
   - Bila perubahan domain ini mendarat sebelum 0010 langkah 13 (create minimal), F10 langsung menulis terhadap signature final. Kalau tidak, F20 langkah 1 ikut memperbaiki call site F10.
6. **`Runtime::committed` berbenturan dengan aturan "revision naik tepat 1 per mutasi" (0004:58, 0004:224, `runtime.rs:375-393`).** Dokumentasikan sebagai konstruktor yang menjalankan transisi `RuntimeState` murni dan mengembalikan revision 1, bukan rangkaian panggilan `Runtime::apply`. `NewRuntime` sudah ada (`runtime.rs:177`).
7. **`HostFacts` infallible di 0018, padahal sumber macOS-nya sysctl.**
   - Rekomendasi: `MacosPlatform::new` dan `with_roots` membaca facts sekali lalu gagal cepat (aturan startup stacklyd di AGENTS.md).
   - `FakePlatformInfo` memegang facts yang bisa diatur.
   - `UnsupportedPlatform` butuh nilai `os_version` yang terdefinisi (misalnya 0). Alternatifnya, method mengembalikan `Result`. Arsitek memilih salah satu.
8. **`MacosCodeSigning` menjalankan codesign di luar `ProcessSupervision` (0018:275), berbenturan dengan 0004:610.** Ubah invariant dengan pengecualian bernama, jangan arahkan codesign lewat `ProcessSupervision`. Timeout 120 s codesign tidak boleh memasukkan token SIGKILL di luar `stackly-platform-macos/src/process/kill.rs` (aturan check-deps 0010:512). Penulis check-deps F10 langkah 15 perlu diberi tahu (lihat K9).
9. **Celah yang bukan usulan fase 02.** 0017 langkah 1 (0017:570) menambah modul domain `stackly-domain::catalog` (`SupportStatus`), dan rationale 0017:265 menyatakan tidak perlu update 0004. Padahal 0004:700 meminta tambahan domain lewat spec ini. Arsitek bisa menambah satu baris sekalian.
10. **Observasi yang tidak diusulkan.** 0020 membuat create bisa dikompensasi penuh, sehingga output init yang dibuatnya ikut dihapus (rationale 0020:507). BR-006 (0004:169, 0004:290) menyatakan hanya purge yang menghapus data. Baris BR-006 bisa diberi catatan bahwa kompensasi create hanya menghapus apa yang dibuat operasi itu (BR-023).

### Spec 0005: Data model registry

**Status.** F5 done (`01-foundation.md:96-102`). Di main sudah ada migration 0001–0003, op journal F7, dan op secret dan audit F11 (`crates/domain/src/registry/batch.rs:158-245`). Tambahan fitur 10 (langkah 2a) belum di main. Tabel `provider_sources`, kolom `source_id` dengan FK dan index, dan trigger immutability yang mencakup `source_id` sudah ada di migration 0001 (`0001_registry.sql:28-39, :51, :58, :63`). Tidak ada branch yang menyentuh registry atau `docs/specs/0005`.

**Timing:** teks sekarang. Kodenya punya batasan urutan (resolusi 5).

Peta usulan:

| # | Sumber | Usulan | Item |
|---|---|---|---|
| P1 | 0020:713-723 | `fn provider_source_by_location(&self, kind: &str, location: &str) -> Result<Versioned<Option<ProviderSourceRecord>>, RegistryError>` setelah `known_instance_ids` (0005:406) | 3 |
| P2 | 0020:725-735 | Baris WriteOp `InsertProviderSource(ProviderSourceRecord)` dan `DeleteInstanceCreatedBy { id, expected, operation }` setelah 0005:450 | 4, 5 |
| P3 | 0020:738-750 | `ProviderRevisionRecord.source_id: Option<ProviderSourceId>` (`MissingReference`), `ProviderSourceRecord`, dan kalimat field yang belum dipakai | 6, 11 |
| P4 | 0020:752-762 | `ResourceRef::ProviderSource(ProviderSourceId)` di 0005:456 | 8 |
| P5 | 0020:764-774 | `RegistryError::NotDeletable { resource, reason }` di 0005:454 | 7 |
| P6 | 0020:776-786 | Kode wire `registry.not_deletable` (conflict) setelah `NameTaken` (0005:528) | 10 |
| P7 | 0017:561 (K-2; milik F20) | Isi yang sama dengan P1–P4 tanpa aturan revision 1. Nilai kolom K-2 ada di 0017:549-557 | 3, 5, 6, 8, 12 |
| P8 | 0018:341, 0018:400-434, 0018:727 | *Tambahan fitur 18*: migration `NNNN_runtime_install.sql` (kolom artifacts `catalog_id`, `build`, `format`, `size_bytes`; tabel `compatibility_records`), field baru `ArtifactRecord`, tipe JSON `SignatureInfoJson` dan `DependencyManifestJson`, dan baris tabel perluasan | 1, 2, 6, 9, 11, 12 |
| P9 | 0020:626 | Follow-up 0005:818 terjawab oleh langkah `layout` dan `register` serta AC-10 di 0020 | 4, 13 |

Daftar perubahan gabungan:

1. **Summary (0005:8) dan AC-1 (0005:21).** Catat bahwa migration F18 menambah `compatibility_records`, beserta frozen fixture `registry-v4.sql`, `FIXTURE_SHA256`, dan fingerprint versi 4 (0005:376-381, :591-595). F20 menambah op `provider_sources` tanpa migration.
2. **Data model `artifacts` (0005:145-162).** Catat kolom F18 `catalog_id`, `build`, `format`, dan `size_bytes`, lalu tunjuk tabel `compatibility_records` di Data model spec 0018 (0018:404-426), dengan tanda *(ditambah fitur 18, spec 0018)*.
3. **Trait `Registry` (0005:396-416).** Sisipkan `provider_source_by_location` setelah `known_instance_ids`, verbatim dari 0020:722.
4. **Aturan tulis (0005:430).**
   - Tambahkan `NotDeletable` ke urutan pemeriksaan, tepat setelah pemeriksaan revision.
   - Opsional, setelah 0005:433: create (spec 0020) juga mengulang saat `DuplicateResource` untuk provider revision (0020 AC-10, 0020:32).
5. **Tabel WriteOp (0005:435-450).** Sisipkan baris `InsertProviderSource` dan `DeleteInstanceCreatedBy` verbatim dari 0020:734-735.
   - `InsertProviderSource`: revision 1; `DuplicateResource` untuk ID atau `(kind, location)`; satu entri `Committed.revisions`.
   - `DeleteInstanceCreatedBy`: endpoint, path, dan config revision ikut cascade, begitu juga `secret_reference_uses`. Hasilnya `NotFound`, `RevisionConflict`, atau `NotDeletable` dengan reason `created_by_other_operation` atau `active`. Tidak ada entri `Committed.revisions`.
6. **Records (0005:452).**
   - Terapkan 0020:738-750: `source_id` dan `ProviderSourceRecord`. Tipe Rust untuk `kind` dan `status` harus disebut; keduanya himpunan kode terbuka dengan nilai awal dari K-2.
   - Tambahkan field `ArtifactRecord` F18 dari 0018:427: `catalog_id`, `build`, `format: Option<ArtifactFormat>`, `size`, `signature_info`, `dependency_manifest`, dan `compatibility: Vec<CompatibilityRecordRow>`.
   - Kalimat field yang belum dipakai menjadi "(`approved_at`, `provider_revision_pk` artefak) belum ada di record dan ditulis NULL".
7. **`RegistryError` (0005:454).** Tambahkan `NotDeletable { resource, reason }` (0020:772).
8. **`ResourceRef` (0005:456).** Tambahkan `ProviderSource(ProviderSourceId)` (0020:761). Perluas kalimat 0005:516: `ProviderSource` tidak punya `ResourceKind` dan tidak dikirim di wire.
9. **Tabel *Tambahan yang sudah dirancang* (0005:460-473).**
   - Baris "Tambahan fitur 18": migration, field `ArtifactRecord`, `compatibility_records`, dan tipe JSON (spec 0018 langkah 6).
   - Baris "Tambahan fitur 20": read dan insert provider source, `source_id`, `DeleteInstanceCreatedBy`, `NotDeletable` (spec 0020 M1 langkah 2, dengan K-2 spec 0017).
   - Keduanya dibangun dengan `FakeRegistry`, `registry_conformance`, dan `SqliteRegistry`.
10. **Kode error di wire (0005:521-544).** Sisipkan baris `registry.not_deletable` (kategori conflict) setelah `NameTaken`, dengan details sesuai resolusi 3. AC-17 (0005:37) butuh test pemetaan stacklyd untuk variant baru ini.
11. **Pemetaan aggregate (0005:554-569).** Baris 567 (`provider_revisions`) tinggal `approved_at`. Baris 569 (`artifacts`) tinggal `provider_revision_pk`.
12. **Value sourcing (0005:633-672).**
    - `InsertProviderSource` mendapat `id = ProviderSourceId::generate()` di `ensure_source`, dengan nilai kolom K-2: kind `bundled` atau `development`, location `app` atau path override kanonis, trust_root `app_bundle` atau NULL, status `active`, `last_sync_at` NULL (0017:549-557).
    - Sumber kolom artefak F18 diambil dari 0018:567-573.
13. **Follow-up (0005:785-825).** Tick 0005:818 dengan penunjuk spec 0020. Tambahkan baris terbuka untuk build F18 langkah 6 dan F20 M1 langkah 2.

Konflik dan resolusi:

1. **0005:452 diedit oleh dua usulan.** 0020 mengeluarkan `source_id` dari daftar field yang belum dipakai, sementara 0018 mengisi `signature_info` dan `dependency_manifest`. Kalimat gabungannya hanya menyisakan `approved_at` dan `provider_revision_pk` artefak, dan baris 567 serta 569 menyesuaikan.
2. **0017:561 dan 0020:713-786.** 0020 adalah superset karena menambah aturan revision 1 dan entri `Committed.revisions`. Terapkan blok verbatim 0020 dan kutip K-2 untuk nilai kode.
3. **Details `registry.not_deletable`.** Rekomendasi: `ErrorBody.resource` = Instance (`ResourceKind` instance) dan details hanya `reason`, mengikuti 0005:519. Kalau tidak, catat pengecualian eksplisit untuk key `resource`. 0020:403 perlu disesuaikan.
4. **Konstruktor berubah arity.** `ProviderRevisionRecord::new` dan `ArtifactRecord::new` adalah `const fn` dengan arity tetap (`record.rs:28`, `:68`). Pemanggilnya ada di `builders.rs:171`, `:330`; `read.rs:82`, `:134`; `common.rs:226`, `:230`; dan `registry_port.rs:619`. `install_fake` (0010) dan `pin_package` (0019) nanti menambah pemanggil. Rekomendasi: method builder additive (misalnya `with_source_id`, dan builder untuk field F18), bukan mengubah arity. Alternatifnya, F20 langkah 2 dan F18 langkah 6 memperbarui semua pemanggil.
5. **Urutan build kode.** 0010 langkah 2a, F18 langkah 6, dan F20 M1 langkah 2 semuanya menulis dispatch `WriteOp`, `FakeRegistry`, `registry_conformance`, dan `SqliteRegistry`, jadi harus jalan satu per satu.
   - 0010 langkah 2a lebih dulu, karena cascade config revision `DeleteInstanceCreatedBy` dan pola `ConfigValuesJson` bergantung pada `InsertConfigRevision` miliknya.
   - Cascade config revision baru bisa dites setelah langkah 2a. `ReplaceSecretUses` sudah ada (`batch.rs:237`).
   - Migration F18 menjadi versi 4 bila merge lebih dulu. Update 0007 juga mengusulkan migration `failure_items` (lihat 0007 item 5), jadi nomornya harus dikoordinasikan.
6. **Protokol.** `registry.not_deletable` ada di `stackly-protocol` `codes::registry`, sehingga butuh minor bump berikutnya setelah sesi Contract 1.4 merge. `--amend` hanya boleh selama versi itu belum ada di main.
7. **Keterkaitan dengan create di F10.** Bila lead melipat create yang bisa dikompensasi penuh ke 0010 langkah 13, `DeleteInstanceCreatedBy` dan `NotDeletable` harus dibangun sebelum 0010 langkah 13 (lihat K1).
8. **Kemungkinan konflik merge kecil.** /sync F10 atau F11 bisa men-tick baris Follow-up 0005:816-817 yang bersebelahan, sementara sesi ini men-tick 0005:818.

### Spec 0006: Protokol IPC & daemon

**Status.** Kotak F6 tercentang semua (`01-foundation.md:114-124`) dan kontrak ada di 1.4 (`crates/protocol/src/version.rs:25`). Sesi "Contract 1.4: fixes from the feature 6 review" masih berjalan, dan log koordinasi :77 mengantrekan update ini setelah sesi itu. Branch F7 closeout mengubah `crates/protocol` dan `apps/daemon`, tetapi tidak mengubah teks spec 0006.

Di main sudah ada `ErrorBody::with_hint` (`error.rs:198`), tetapi belum ada `codes::<area>::hint`. Belum ada juga `ShutdownReason::RestartRequested` (`events.rs:143-147`), `daemon.restart_unavailable` (`codes.rs:201-207`), maupun field `StartConfig` baru (`apps/daemon/src/startup.rs:28-35`).

**Timing:** setelah sesi Contract 1.4 merge. Update ini harus mendarat sebelum fungsi hint pertama dibuat (0017 M1 langkah 5 atau 0022 M1 langkah 3).

Peta usulan:

| # | Sumber | Usulan | Item |
|---|---|---|---|
| P1 | 0017:627 (0017:359) | Satu bentuk hint: `codes::<area>::hint(code: &str, details: &[ErrorDetail]) -> Option<&'static str>`, dipasang stacklyd lewat `ErrorBody::with_hint`. 0017 menganggap area dan subsystem `catalog` tidak butuh update (aturan pengguna pertama) | 1, 2 |
| P2 | 0018:351 (0018:523; Follow-up 0018:731) | Bentuk yang sama untuk semua area, juga untuk `FailureError` operasi yang hint tersimpannya kosong. Area `runtime` memakainya. Hint statis, paling panjang 200 karakter | 1 |
| P3 | 0015:482 | Kode `daemon.restart_unavailable` (unsupported, retryable false, details `launchContext`, pemilik spec 0015) | 3 |
| P4 | 0015:483 | Exit code stacklyd 75: "berhenti untuk restart" | 5 |
| P5 | 0015:484 | `ShutdownReason` mendapat `restart_requested`; `requested` dipakai untuk `daemon.shutdown` mode `stop` | 4 |
| P6 | 0015:485 (0015:214-225, :210) | Method `daemon.shutdown` (`ShutdownParams { mode }`, open enum `ShutdownMode`, reply `ShutdownAccepted`, recovery allowed) dan `DaemonInfo.launch: DaemonLaunch { context, label, bundlePath }` | 5, 6 |
| P7 | 0015:486 | Ganti hint `start it with stacklyd` di bagian CLI daemon status | 7 |
| P8 | 0015:464 (0015:340-342) | Follow-up 0006:669 terjawab: verifikasi tanda tangan peer ditunda ke F60/F63, dan `stackly daemon start` ada | 9 |
| P9 | 0022:541 (0022:186; Follow-up 0022:557) | Tidak perlu update isi. Area `storage` masuk lewat aturan pengguna pertama. Baris tabel area bisa ditambah lewat /sync | 2 |
| P10 | 0020:957 (0020:253, :389) | 0020 menyatakan tidak ada update untuk 0006, tetapi menambah newtype `SignedSafeInteger` di `wire.rs` (i64 dalam ±`MAX_SAFE_INTEGER`, dipetakan ke Swift `Int64`) | 8 |
| P11 (implisit) | 0015:209-212, :221; 0017:300-305, :507; 0020:384 | `StartConfig` mendapat `catalog_dir` (`STACKLY_CATALOG_DIR`), `xpc_service_name`, `parent_pid`, dan `current_exe`. Launch record ditulis antara langkah start 6 dan 9. Catalog dimuat di slot start setelah `open_registry` (juga di recovery). `ensure_source` jalan setelah registry Ready. Subsystem `catalog`. `DaemonDeps` mendapat `launch` dan `bundle`, dan `DaemonConfig` mendapat override `catalog` khusus test | 5 |

Daftar perubahan gabungan:

1. **Envelope error.** Setelah baris `hint` (0006:167) dan paragraf catalog (0006:190), tambahkan aturan hint:
   - fungsi per area `codes::<area>::hint(code, details) -> Option<&'static str>`;
   - teks statis yang dipilih dari code dan nilai details, tidak pernah diisi nilai;
   - dipasang stacklyd lewat `ErrorBody::with_hint`;
   - juga dipakai untuk `FailureError` operasi yang hint tersimpannya kosong;
   - batas 200 karakter, dengan keputusan apakah masuk `CONTRACT_LIMITS` (resolusi 6) (0017:627, 0018:351, 0018:523).
2. **Tabel area (0006:194-204).** Tambahkan `storage`, pemiliknya spec 0022 (0022:541). Opsional: sebut `catalog` (spec 0017) dan `runtime` (spec 0018) di baris pengguna pertama.
3. **Tabel kode daemon (0006:241-244).** Tambahkan `daemon.restart_unavailable`: unsupported, retryable false, details `launchContext`, pemilik spec 0015 (0015:482).
4. **Event dan cursor.** `ShutdownReason` di 0006:256 mendapat `restart_requested`, dan `requested` berarti `daemon.shutdown` mode stop. Ubah stop step 2 (0006:370) dan AC-11 (0006:31) supaya reason dan exit code (0 atau 75) mengikuti pemicunya (0015:484).
5. **Bagian Daemon.**
   - Tabel exit (0006:359) mendapat 75 (0015:483).
   - Daftar method yang selalu dilayani di 0006:352 mencakup `daemon.shutdown` (0015:221).
   - Tambahan implisit dari P11:
     - field `StartConfig` dan pembacaan env di 0006:338 dan 0006:342;
     - `STACKLY_CATALOG_DIR` dan `XPC_SERVICE_NAME` (diset launchd, bukan user) di *Configuration required* 0006:542-545;
     - launch record di antara langkah 6 dan 9 (0006:347-350);
     - load catalog dan `ensure_source` di slot start (0006:352);
     - subsystem `catalog` di 0006:382.
   - Untuk `STACKLY_CATALOG_DIR` yang tidak valid: exit 1 dengan pesan `STACKLY_CATALOG_DIR "<v>" is not an absolute path to a folder`.
6. **API surface (0006:446-457).** Tambahkan baris `daemon.shutdown`: params, reply, recovery allowed, error `daemon.restart_unavailable`, `daemon.shutting_down`, dan `request.invalid_params`. Daftar field opsional 0006:453 mendapat `launch` (0015:485). Daftar recovery allowed di AC-18 (0006:38) bersifat per spec dan tidak perlu diubah.
7. **Bagian CLI (0006:406).** Ganti "start it with stacklyd" dengan penunjuk ke tabel pemulihan 0008/0015 (0015:486). Bagian ini sudah bertanda `// provisional: F8` (0006:398).
8. **Aturan wire (0006:84) dan mapping typeshare (0006:410).** Tambahkan newtype `SignedSafeInteger` (i64 dalam ±`MAX_SAFE_INTEGER`, dipetakan ke `Int64`). Bagian ini implisit, dari 0020:253.
9. **Security model (0006:540).** Tunjuk ke 0015 *Verifikasi tanda tangan peer*. Follow-up 0006:669 dibiarkan untuk /sync lead (0015:464).
10. **Opsional, tidak diusulkan.** Daftar pengguna Fuzzing (0006:419) mendapat fitur 19 (format `provider_template`, 0019:525) dan fitur 23 (`nats_readiness_protocol`, 0023:408).

Konflik dan resolusi:

1. **Hint "yang sudah ada" ternyata belum ada.** 0020:389 dan 0020:957 menyebut `codes::<area>::hint` sudah ada, padahal di main belum. Aturan 0006 harus mendarat sebelum fungsi hint pertama (0017 M1 langkah 5 atau 0022 M1 langkah 3). 0018 dan 0020 lalu mengikutinya.
2. **Klaim pengguna pertama untuk subsystem `catalog` (0017:627).** Aturan di 0006:192 hanya berlaku untuk area error, dan 0006:382 mendaftar subsystem dengan nama tetap. Pilih salah satu: tambahkan kalimat bahwa subsystem baru ditambah oleh fitur yang pertama memakainya, atau cantumkan `catalog` di 0006:382.
3. **Minor bump dari banyak spec.** 0015, 0017, 0018, 0020, dan 0022 masing-masing menaikkan minor `ProtocolVersion::CURRENT`. Branch paralel diselesaikan dengan aturan merge di 0006:141 (jalankan ulang codegen; `--amend` hanya selama versi belum ada di main). Tidak perlu edit spec.
4. **Fallback hint untuk `FailureError` (0018:351).** Fallback ini tidak dilipat ke F7 langkah 14–15. 0018 langkah 11 yang menambahkannya di pemetaan stacklyd, jadi F7 tidak perlu diubah.
5. **`SignedSafeInteger`.** Walau 0020 menyatakan "tidak ada update untuk 0006", aturan wire satu baris tetap diperlukan, karena 0006:84 dan AGENTS.md hanya mengizinkan nilai 64-bit lewat `Revision`, `Sequence`, dan `Count`. Konstanta batasnya juga terkena test penamaan `CONTRACT_LIMITS` (0006:139). Terapkan di sesi yang sama dan beri tahu build 0020.
6. **Batas 200 karakter (0018:523).** Kalau ini batas yang dijamin pengirim, letaknya di `CONTRACT_LIMITS` (dengan test di `limits.rs`) dan mengubah digest. Kalau hanya properti test katalog, spec harus menyatakannya.
7. **Pemilik `daemon.restart_unavailable`.** Tabel 0006:206 berjudul kode milik spec 0006, dan di kode area daemon dimiliki 0006 (`codes.rs:201`). Tandai pemilik baris ini sebagai spec 0015.
8. **Cakupan pencarian.** 0016 dan 0023 tidak punya usulan untuk 0006. 0019 hanya mengikuti aturan bump versi (0019:359).

### Spec 0007: Operation engine

**Status.** Status spec In Progress (0007:4). Milestone 1–3 dan langkah 13 (protocol) sudah di main. Langkah 14–15 (wiring stacklyd, test daemon) belum di main, karena `apps/daemon/src` tidak punya modul `ipc/`.

Branch `claude/f7-daemon-closeout-7fb5d1` (16 commit, terakhir 2026-10-20 06:12) memegang:

- langkah 14–15 (`apps/daemon/src/ipc/operation.rs`, `operation/dto.rs`, `operations.rs`);
- tick verify, review, dan document;
- perbaikan core pada kode yang sama dengan yang disentuh usulan fase 02: kompensator `instance_tree` mendapat `manifest_unreadable` (commit 252b5b7), dan `claim_resource` menolak resource yang sudah dipegang registry;
- empat baris Follow-up baru di 0007, salah satunya keputusan terbuka tentang penyimpanan `hint`, `items`, dan `rule` di `FailureError`.

**Timing:** setelah branch closeout F7 merge.

Peta usulan:

| # | Sumber | Usulan | Item |
|---|---|---|---|
| P1 | 0020:790-801 (AC-11 0020:33, :362, :1024) | Pecah baris tabel resume 0007:420. `create_instance` punya baris sendiri: semua langkah reversible, berakhir failed atau cancelled, dan needs_recovery hanya bila kompensasi gagal | 1 |
| P2 | 0020:803-813 (AC-8 0020:30, :369, :980) | Aturan *Catatan dibuat* (0007:397): detail `data_not_empty`; `not_created` hanya untuk catatan yang masih intended, sebelum `confirm_created` | 2 |
| P3 | 0020:815-825 (AC-11, 0020:377, :980) | Kompensator `instance_tree` (0007:460) memanggil `probe_lock` bila `InstancePaths.lock` ada. Hasil Held memberi `compensation_failed` dengan detail `instance_locked` tanpa menghapus apa pun | 3 |
| P4 | 0018:732 (0018:90-106, :446) | Catatan resume `install_runtime` (0007:416) terjawab oleh *Alur install* spec 0018 | 1 |
| P5 (implisit) | 0018:92 | Handler `runtime.install` mencari idempotency key sebelum resolusi catalog, sehingga butuh lookup replay read-only sebelum `begin` | 4 |
| P6 | 0018:351, 0018:523, 0018:731 (bentuk dari 0017:627) | Hint kegagalan operasi tidak disimpan. stacklyd menghitungnya dari code dan details | 5 |
| P7 (implisit) | 0020:29 (AC-7), 0020:33, 0020:446; 0011:351 | `FailureError.items` harus benar-benar disimpan dan dipetakan, karena `secret.needs_secret` membawa satu item per reference | 5 |

Daftar perubahan gabungan:

1. **Tabel resume per jenis operasi.**
   - Pecah baris 0007:420 persis seperti 0020:796-801. `create_instance` mendapat baris sendiri: "semua langkahnya reversible (spec 0020), jadi gagal, cancel, dan terputus selalu dikompensasi dan berakhir failed atau cancelled; needs_recovery hanya kalau kompensasi gagal". Jenis operasi lain mempertahankan catatan lama, tanpa fitur 20.
   - Anotasi baris 0007:416 `install_runtime` sebagai terjawab oleh spec 0018 *Alur install* (0018:732).
   - Kode tidak berubah, karena `crates/domain/src/operation.rs:253-278` sudah sesuai.
2. **Aturan *Catatan dibuat* (0007:397).**
   - Ganti dengan teks 0020:812. `not_created(item, reason)` juga menerima `data_not_empty` saat pembuatan berhasil tetapi isinya tidak terbukti milik operasi, dan hanya dipanggil untuk catatan yang masih intended, sebelum `confirm_created`.
   - Opsional: AC-8 (0007:29) menyebut `data_not_empty` di samping `AlreadyExists` dan `ShortKeyTaken`.
   - Kode: perbarui doc comment `crates/core/src/operation/context.rs:696-698` dan tambahkan test `ScriptedWork` (0020 langkah 3). `not_created` sudah menerima `ReasonCode` apa pun (`context.rs:704-716`).
3. **Kompensator `instance_tree` (0007:460).** Terapkan teks 0020:824 dengan urutan eksplisit:
   - (1) manifest milik operasi lain memberi `NotOwned`, dan manifest yang tidak terbaca memberi `compensation_failed` `manifest_unreadable` (perilaku branch closeout);
   - (2) `Platform::processes().probe_lock(InstancePaths.lock)`, di mana file yang tidak ada terbaca Free;
   - (3) hasil Held memberi `compensation_failed` `instance_locked` tanpa menghapus apa pun;
   - (4) error dari probe juga memberi `compensation_failed`, untuk menutup celah di usulan;
   - (5) baru lalu `remove_owned_tree`.

   Opsional: satu kalimat AC-8 untuk kasus lock. Port sudah tersedia di `crates/platform/src/process.rs:935-941`, dan `FakePlatform` memasang `FakeProcessSupervision`.
4. **Idempotency (0007:426-432) dan blok API engine (0007:239-254).** Tambahkan lookup replay read-only yang boleh dipanggil handler method yang memulai operasi sebelum kerja sinkron yang mahal (0018:92).
   - Masukannya key dan fingerprint.
   - Hasilnya view yang sudah ada, none, atau `KeyReused`.
   - Lookup ini tidak melewati gate `begin`. `begin` tetap melakukan lookup sendiri dan tetap otoritatif.
5. **Keputusan `FailureError` dan journal (0007:89, 0007:132-137, 0007:504, AC-16 0007:37).** Ini menutup Follow-up review F7.
   - **`items` disimpan.** Ini dibutuhkan 0020 AC-7 dan 0011 AC-11. Bentuknya kolom JSON array baru, misalnya `failure_items`, di migration bebas berikutnya setelah 0003, paling banyak 100 item dengan bentuk `ErrorItem` (spec 0006).
   - **`hint` tidak disimpan.** stacklyd menghitungnya dengan `codes::<area>::hint(code, details)` dan `ErrorBody::with_hint`, baik untuk error langsung maupun kegagalan tersimpan (0018:351, 0018:523, 0017:627).
   - **`rule` masih diputuskan:** disimpan dari `OperationError::rule()` atau diturunkan dari katalog.
   - Sesuaikan kata-kata AC-16 dan AC-18.
6. **Edit konsistensi yang tidak diusulkan spec fase 02 untuk 0007.** Ini mencerminkan 0020:919-929 yang menargetkan spec 0010. Bagian *Asumsi lintas spec*, poin "Untuk spec 0010" (0007:530), masih menulis `BusyPolicy::Queue untuk start, create, dan reload pengguna`. Ubah menjadi Queue untuk start dan reload, dan Reject untuk create dengan lock `InstanceName` (spec 0020, 0020:360, AC-10). Opsional: "(kode milik fitur 20)" di 0007:394 menunjuk `instance.short_key_taken` (spec 0020).
7. **Follow-up.** Tambahkan baris yang mencatat bahwa fitur 20 membangun item 2 dan 3 di 0020 M1 langkah 3, dan fitur 18 membangun item 4 di 0018 langkah 11. Item 5 menjadi resolusi baris keputusan dari closeout F7 setelah branch itu merge.

Konflik dan resolusi:

1. **Item 3 dari 0020 dan branch closeout F7 (commit 252b5b7) menyentuh kode dan teks yang sama.** Keduanya mengedit `instance_tree` di `crates/core/src/operation/compensation.rs:289-327` dan index.md 0007 (closeout menambah Follow-up setelah 0007:771). Terapkan update setelah branch itu merge, dan pertahankan aturan `manifest_unreadable` di depan probe lock baru.
2. **`hint`, `items`, `rule` di `FailureError`.** 0007:89, 0007:132-137, kode `journal.rs:248-252`, dan migration `0002:40-45` tidak saling cocok. 0018:351 dan :523 sudah menjawab soal hint: dihitung, tidak disimpan. 0020:29 memaksa items disimpan, karena kalau dibuang daftar per reference `secret.needs_secret` hilang. Hanya `rule` yang butuh keputusan sungguhan.
3. **Lookup sebelum `begin` (0018:92) dan 0007:428-432 yang menempatkan lookup hanya di dalam `begin`.** Ini diselesaikan sebagai API baca additive. `begin` tetap memeriksa ulang, jadi miss yang diikuti create bersamaan berakhir Replayed di `begin`. Jaminan FIFO dan replay di AC-2 dan AC-3 tetap berlaku. 0020 tidak membutuhkannya, karena `gather` jalan sebelum `begin` dan dibuang saat replay (0020:360).
4. **0018:732 meminta tick, padahal 0007 tidak punya kotaknya.** Follow-up 0007:762-771 hanya mencakup fitur 8, 34, 35, 46, 63, dan 64. Jadi ini diselesaikan sebagai anotasi di baris 0007:416.
5. **0007:530 (Queue untuk create) dan 0020:360/AC-10 (Reject).** Ikuti 0020. Edit ini dicatat sebagai konsistensi, bukan usulan fase 02.
6. **Sudah diperiksa, tidak perlu perubahan.**
   - Loop retry `ctx.commit(FinalBatch { expected_generation, .. })` dari 0020 berjalan di main, karena `OperationContext::commit` mengembalikan error tanpa merusak context (`context.rs:757-762`).
   - Daftar `installing` 0018 bisa memakai `engine.list(activeOnly)` lalu filter per kind.
   - 0022 hanya membaca DTO yang sudah ada.
   - 0015, 0016, 0017, 0019, dan 0023 tidak punya usulan untuk 0007. 0017:546 hanya menyatakan bahwa pin tidak mendaftarkan apa pun untuk kompensasi.

### Spec 0008: CLI client & kontrak output

**Status.** Build it F8 terbuka (`01-foundation.md:158`). Milestone 1 tercentang. Kode milestone 2 ada di main (bf5d98f) tetapi kotaknya (:160) belum tercentang, dan Verify serta Test masih terbuka.

Di main:

- `CLI_SCHEMA_VERSION = 1` (`apps/cli/src/output/envelope.rs:11`);
- hint "start it with stacklyd" di `apps/cli/src/session.rs:316` dan golden `apps/cli/tests/daemon_status.rs:183`;
- `codes::cli` persis sama dengan tabel 0008 (`codes.rs:335-362`) dan dipin oleh test `cli_codes_match_the_spec_0008_table` (`codes.rs:783`);
- grup daemon berupa satu file `commands/daemon.rs`;
- runtime current-thread di-drop biasa (`app.rs:260-279`).

**Timing:** setelah build F8 mendarat. Teks `CLI_SCHEMA_VERSION` 2 ikut commit 0015 M1 langkah 6, dan F8 serta langkah itu jalan berurutan (log :48).

Peta usulan:

| # | Sumber | Usulan | Item |
|---|---|---|---|
| P1 | 0015:477 (0015:241-251; AC-7 0015:28) | `CLI_SCHEMA_VERSION` 2. Bentuk `daemon status` menjadi `{ info \| null, health \| null, client \| null, agent: AgentReport }`. Baris baru untuk `daemon install`, `start`, `stop`, `restart`, `completion`, dan `manpage` | 5 |
| P2 | 0015:478 | AC-6: daemon yang tidak berjalan tetap exit 6, tetapi `result` memuat `agent` | 4 |
| P3 | 0015:479 (0015:276-290, :270) | 12 kode `cli.agent_*` dan `cli.daemon_wait_timeout`, plus 3 warning | 2 |
| P4 | 0015:480 | Ganti hint "start it with stacklyd" dengan tabel *Langkah pemulihan CLI*, hanya di teks manusia | 3 |
| P5 | 0015:481 | Baris fitur 15 di *Batas dengan spec lain* (0008:60) menunjuk spec 0015 | 1 |
| P6 | 0015:465 | Follow-up 0008:522 terjawab: tidak perlu allowlist, karena CLI tidak pernah menjalankan launchctl | 9 |
| P7 | 0022:542 (0022:321, :323) | `instance list` membaca semua halaman. `cli.instance_not_found` ditambahkan | 2, 8 |
| P8 | 0020:957 (0020:404-405, :419) | `cli.runtime_not_found` dan `cli.runtime_ambiguous` "lewat aturan yang sama". `CLI_SCHEMA_VERSION` tetap karena hanya menambah key | 2 |
| P9 | 0020:630 (0008:455) | Follow-up 0008:523 (bukti AC-028 penuh) terjawab oleh 0020 AC-18 dan AC-19 | 9 |
| P10 (implisit) | 0015:233, :235, :236 | Folder `commands/daemon/`, `completion.rs`, `manpage.rs`. Runtime dimatikan dengan `shutdown_timeout(Duration::ZERO)`. `AGENT_CALL_TIMEOUT` 10 s. `details.state` `waiting_for_daemon` | 2, 6 |
| P11 (implisit) | 0020:422 (AC-18 0020:40) | `cli.usage` (exit 2) dipakai untuk tiga pemeriksaan di sisi CLI: `--port`, `--set-json`, `--settings-file` | 7 |

Daftar perubahan gabungan:

1. **Batas dengan spec lain (0008:60).** Baris fitur 15 menunjuk spec 0015 (0015:481).
2. **Kode error area cli (0008:148-170).**
   - Dua belas kode dari 0015 (0015:276-290): `cli.agent_root_override`, `cli.agent_not_bundled`, `cli.agent_helper_missing`, `cli.agent_not_registered`, `cli.agent_requires_approval`, `cli.agent_disabled`, `cli.agent_not_found`, `cli.agent_invalid_signature`, `cli.agent_os_error`, `cli.agent_status_unavailable`, `cli.agent_call_timeout`, dan `cli.daemon_wait_timeout`, dengan kategori dan details persis seperti di 0015.
   - `cli.runtime_not_found` (not_found, exit 10, details `selector`) dan `cli.runtime_ambiguous` (conflict, exit 3, details `selector` dan `candidates`) dari 0020:404-405.
   - `cli.instance_not_found` (not_found, exit 10, details `name`) dari 0022:323.
   - Satu kalimat baru: error buatan CLI boleh membawa hint statis di `ErrorBody`, sedangkan teks pemulihan yang tidak statis tetap hanya di teks manusia.
   - `details.state` `waiting_for_daemon` diizinkan untuk `cli.interrupted` (0015:236).
   - Daftar warning (0008:172) mendapat `cli.agent_bundle_mismatch`, `cli.daemon_not_running`, dan `cli.daemon_relaunched` (0015:270).
3. **Koneksi ke daemon (0008:256), Output manusia (0008:232), dan value sourcing (0008:387).** Ganti "start it with stacklyd" dengan tabel *Langkah pemulihan CLI* (0015:257-266) (0015:480).
4. **`stackly daemon status` (AC-6 0008:26; bagian 0008:261-278).**
   - Baris agent: Agent, Launched, App, dan Next (0015:253).
   - Bila daemon tidak berjalan: exit 6, result `{ agent, info null, health null, client null }`, ditambah `error` (0015:478). Ini sah menurut 0008:187 (hasil parsial yang didokumentasikan).
5. **Kontrak `--json` (0008:194-204).**
   - Bentuk baru `daemon status`.
   - Baris untuk `daemon install`, `start`, `stop`, `restart`, `completion`, dan `manpage` (0015:241-249).
   - `CLI_SCHEMA_VERSION` 2 di 0008:22, 0008:87, dan 0008:184 (0015:477).
   - Teks ini masuk di commit yang sama dengan 0015 M1 langkah 6, atau diberi tanda "berlaku sejak 0015 langkah 6".
6. **Peta crate dan modul (0008:79-80, 0008:98).**
   - Folder `commands/daemon/` (mod, status, install, start, stop, restart, agent, wait), ditambah `commands/completion.rs` dan `commands/manpage.rs`.
   - Runtime tokio dimatikan dengan `shutdown_timeout(Duration::ZERO)` di semua jalur keluar.
   - Panggilan port OS berjalan di `spawn_blocking` dengan `AGENT_CALL_TIMEOUT` 10 s.
   - Bagian ini implisit, dari 0015:233-235.
7. **`cli.usage` untuk pemeriksaan sisi CLI dari 0020 (0008:154, 0008:22).** Rekomendasi: jadikan ketiganya clap value parser, sehingga 0008 tidak berubah. Alternatifnya: perluas definisi `cli.usage` dengan `command` terisi dan nilai `details.kind` untuk pemeriksaan ini (0020:422).
8. **Catatan di 0008:282.** `instance list` dan `runtime list` membaca semua halaman karena resource-nya terbatas jumlahnya (0022:542, 0018:483). Opsional: catatan tone untuk warna grup di kolom LIFECYCLE instance (0022:321).
9. **Follow-up, lewat /sync lead.** Tick 0008:522 (0015:465) setelah 0015 M1 langkah 6. Tick 0008:523 dan perbarui 0008:455 (hapus "menunggu create instance di fitur 20") setelah 0020 M2 langkah 10 (0020:630).

Konflik dan resolusi:

1. **Semua kode cli dari 0015, 0020, dan 0022 masuk tabel 0008.** Klaim "aturan pengguna pertama" di 0020:957 tidak berlaku untuk area `cli` yang sudah dimiliki 0008. Selain itu, AC-14 (0008:34) dan test `codes.rs:783` akan gagal kalau kode tidak ada di tabel. Tidak ada nama ganda di antara 15 kode baru, dan setiap kategori memetakan ke exit code yang diharapkan (0008:131-144).
2. **Penempatan hint.** 0015 menjaga hint agent hanya di teks (0015:255, karena teks `use_bundled_cli` memuat path), sedangkan 0020 dan 0022 memakai hint statis di `ErrorBody`. Satu aturan menyelesaikan keduanya: hint statis lewat `codes::cli::hint`, dan teks pemulihan dinamis tetap hanya di teks.
3. **`CLI_SCHEMA_VERSION` 2 menulis ulang semua golden.** Ini termasuk golden perintah fase 02 yang mendarat lebih dulu (0017 langkah 7, 0018 langkah 12, 0020 langkah 10, 0022 langkah 5). Menurut 0015:545 dan log koordinasi :48, F8 dan 0015 langkah 6 harus berurutan.
4. **`--output id` dari 0018 (0018:487).** Mode ini cocok dengan `OutputMode::Human` (0008:86) memakai `Render` sendiri, jadi 0008 tidak perlu berubah kecuali build menambah mode output.
5. **Batas modul.** Folder `commands/daemon/` tetap satu modul per grup, jadi AC-12 (0008:32) tetap terpenuhi. Nilai `waiting_for_daemon` memang baru, karena 0008:399 menyebut `unknown` sebelum DTO pertama.
6. **Cakupan pencarian.** 0016, 0017, 0018, 0019, dan 0023 tidak punya usulan untuk 0008. 0017 dan 0018 hanya mengikuti pola grup perintah (0017:372, 0018:478).

### Spec 0009: Provider engine & manifest schema

**Status.** F9 ditutup di main (`01-foundation.md:174-183`; closeout edf1ba0 merge di 667510b). Tidak ada sesi atau branch yang menyentuh `docs/specs/0009-*`, `crates/core/src/provider/`, `providers/`, atau `xtask/src`. Belum satu pun tambahan yang diusulkan ada di kode, kecuali `CHILD_PATH` yang sudah publik (`crates/core/src/provider/plan.rs:39`).

**Timing:** sekarang. Ini prasyarat pertama untuk F17 M1, F18 M1, F19 M1, F20 M1, dan F23 langkah 1. Semua spec fase 02 memakai syarat "setelah fitur 9 ditutup" (0017:568, 0018:355, 0019:577, 0023:345). Update ini hanya teks dan tidak butuh bump `ProtocolVersion`.

Peta usulan:

| # | Sumber | Usulan | Item |
|---|---|---|---|
| P1 | 0017:625 (0017:66, :228, :568-570, :631) | `ValidatedPackage::versions() -> &VersionCatalog`; `VersionArtifact { id, line, version, target, status }`; `ArtifactStatus` berisi `Available`, `Blocked`, `Fixture` | 10, 15 |
| P2 | 0017:625, 0015:466 | Tick Follow-up 0009:698 (namespace `testing/` tidak pernah dimuat atau dibundel), dengan guard di `catalog build`, `load_catalog`, dan build-app | 19 |
| P3 | 0017:625 (0017:529) | Pinning dibangun fitur 20 (K-1), bukan fitur 17. Baris Follow-up `files()` yang dirujuk ternyata tidak ada | 15 |
| P4 (implisit) | 0017:214, 0017:233 | `InvalidEntry.issues` membawa kode loader catalog `catalog_digest_mismatch`, `catalog_id_mismatch`, `namespace_not_allowed`, dan `catalog_limit`, padahal `Issue.code` adalah enum tertutup milik 0009 | 14 |
| P5 | 0018:357-367 (item 1; AC-21 0018:42) | Field artefak opsional `stable`, `build`, `unpackedSize`/`entryCount`, `dependencies`, dan `compatibility` (dengan bentuk record) | 10 |
| P6 | 0018:392 (item 2) | Tipe core: `Available` mendapat `stable`, `build`, `unpacked`, `dependencies`, `compatibility`; `Blocked` mendapat `build` | 10 |
| P7 | 0018:394 (item 3) | Generator `cargo xtask codegen` menyalin field inventory dan menulis `compatibility` dari evidence | 10, 16 |
| P8 | 0018:396 (item 4) | `runtime.versionCheck { args, pattern, timeoutSeconds? }` | 5, 6, 12, 15 |
| P9 | 0018:398 (item 5) | Paket fixture loader untuk setiap aturan baru, plus golden generator | 17 |
| P10 | 0018:724 | Tick Follow-up 0009:697 | 19 |
| P11 | 0018:257 (Validasi binary langkah 8) | PATH untuk exec check memakai ulang `CHILD_PATH`; pattern memakai batas pattern settings | 11 |
| P12 | 0019:352 (AC-2 0019:22) | Format template `postgresqlConf` | 2, 3 |
| P13 | 0019:353 | Format template `plain` | 2, 3 |
| P14 | 0019:354 (AC-3 0019:23) | `runExecutable.stdin` menerima `{ secret }` atau `{ text }`, dan plan mendapat `Stdin::Text(String)` | 5, 6, 11 |
| P15 | 0019:355 (AC-4 0019:24) | `x-stackly.printableOnly`, hanya dengan `secret: true` | 7, 16 |
| P16 | 0019:577 (0019:465, :475) | Catat `postgresqlConf`, `plain`, `stdin.text`, dan `printableOnly` sebagai perluasan additive setelah closeout F9 | 1, 2, 3 |
| P17 | 0020:827-839 | Paragraf pembuka Settings schema (0009:246): urutan deklarasi dan `SettingsIssueKind`, dengan teks pengganti persis | 7, 15 |
| P18 | 0020:631 (0020:146-151, :1045) | Follow-up 0009:699: purpose dibaca dari paket yang di-pin. Tick | 19 |
| P19 | 0019:580, 0023:466 | Fitur 21 menjawab Follow-up 0009:699 | 19 |
| P20 | 0023:347 (item 1) | AC-4: tipe probe `native`, hanya dengan nama dari registry | 2 |
| P21 | 0023:348 (item 2) | Bagian Probe: `type: native` dengan registry di `core::provider::probe::native`, dan runner dipilih lewat match exhaustive | 5, 8, 15 |
| P22 | 0023:349 (item 3) | Peta modul: `probe::native` dan `probe::nats` (state machine tanpa IO) | 4 |
| P23 | 0023:350 (item 4) | Baris matriks interpolasi untuk params probe native | 6 |
| P24 | 0023:351 (item 5) | Aturan `apply: reload` mencakup params probe native | 7 |
| P25 | 0023:352 (item 6) | `ProbeSpec::Native(NativeProbeSpec)`, bentuk Serialize, dan `UnrepresentableValue` | 11 |
| P26 | 0023:353 (item 7) | Baris timeout dan sumber exit code untuk `ProbeSpec::Native` | 9 |
| P27 | 0023:354 (item 8) | Fuzz target `nats_readiness_protocol` | 13 |
| P28 | 0023:355 (item 9) | Readiness NATS memakai `nats-readiness`; keputusan kecil `/healthz` diganti | 8, 12, 18 |
| P29 | 0023:356 (item 10) | Skenario *Himpunan tertutup* memakai nama native yang tidak terdaftar | 17 |
| P30 | 0023:357 (item 11) | Tick Follow-up 0009:701 | 19 |

Daftar perubahan gabungan:

1. **Blok "Perluasan additive fase 02".** Tambahkan blok pendek di Feature design, setelah Peta modul.
   - Daftar setiap perluasan, spec dan langkah build pemiliknya (F17 M1 s1; F18 s1, s8, s13; F19 s1-3; F20 M1 s4; F23 s1-2), dan AC pemiliknya.
   - Nyatakan bahwa `stackly/v1alpha1` dan `stackly.provider-versions/v1` tidak berganti nama dan semua perubahan additive.
   - Nyatakan bahwa bump `ProtocolVersion` tidak perlu, karena kode wire tidak berubah: 0018 dan 0019 memakai kode issue yang sudah ada, dan 0023 menyebut "tanpa kode baru".
   - AC-1 sampai AC-19 milik F9 tetap seperti yang diverifikasi. Hanya himpunan tertutup di AC-4 dan AC-9 yang berubah (0019:577).
2. **AC-4 (0009:24).**
   - Tipe probe menjadi tcp, http, exec, dan native. Native hanya dengan nama dari registry, nama lain memberi `unknown_probe` (0023:347).
   - Format template menjadi yaml, json, natsConf, postgresqlConf, dan plain (0019:577).
   - Nilai yang tidak dikenal tetap ditolak saat validasi, tidak pernah saat start.
3. **AC-9 (0009:29), ConfigFile (0009:138), dan Template config (0009:277-287).**
   - Baris escaper `postgresqlConf` (0019:352): teks ditulis dalam kutip tunggal dengan kutip dan backslash digandakan; integer desimal; boolean `on`/`off`; teks dengan karakter kontrol (U+0000–U+001F, U+007F–U+009F) memberi `unrepresentable_value` yang menyebut field atau token, tidak pernah nilainya; tanpa re-parse; setiap escaper diperiksa terhadap `postgres -C`.
   - Baris `plain` (0019:353): semua output tag dan block tag ditolak dengan `template_type` beserta baris dan kolom; komentar `{{! }}` dan literal kurung kurawal yang di-escape diterima; body ditulis apa adanya.
   - Ganti 0009:285 dengan "postgresqlConf dan plain ditambah spec 0019; valkeyConf, mysqlCnf, dan xml oleh fitur 40, 42, 43".
   - Ubah kalimat "tanpa mode raw" supaya jelas bahwa `plain` bukan mode raw, karena ia tidak mengeluarkan nilai.
   - Catat bahwa fuzz target `provider_template` merender ke lima format (0009:493, 0019:525).
4. **Peta modul (0009:59).** Submodule `probe` memegang `native` (registry) dan `nats` (state machine tanpa IO). IO-nya ada di `core::supervisor::probe` (0023:349). Main saat ini belum punya submodule probe; validasi probe ada di `validate.rs`.
5. **Tabel dan bagian manifest (0009:104-149).**
   - **`runtime.versionCheck`** (0018:396):
     - `args`: paling banyak 8 string literal; token memberi `token_not_allowed_here`.
     - `pattern`: memakai batas pattern settings (512 karakter, `size_limit` 64 KiB) tetapi tidak dibungkus anchor; harus punya tepat satu capture group, kalau tidak `invalid_value`.
     - `timeoutSeconds`: 1–60, default 15, kalau tidak `timeout_out_of_range`.
     - Field ini tidak ikut plan mana pun tetapi tercakup digest.
     - Pattern-nya dicari baris per baris setelah newline dan carriage return di ujung dipangkas (0018:257). Ini berbeda dengan pattern settings dan `dataMarker`, yang mencocokkan seluruh nilai.
   - **`runExecutable.stdin`** (0009:144) menjadi tepat satu dari `{ secret }` atau `{ text }` (0019:354):
     - text 1–16384 byte UTF-8;
     - pembuka interpolasi memberi `token_not_allowed_here`;
     - karakter kontrol selain newline dan tab memberi `invalid_value`;
     - stdin tetap ditolak di probe exec, stop command, dan reload run.
   - **Probe (0009:147)** mendapat `type: native` dengan `name`, `endpoint`, dan `params`, ditambah tabel validasi 0023:68-83 yang hanya memakai kode yang sudah ada (0023:348).
6. **Matriks interpolasi (0009:222-237).**
   - Baris stdin mendapat literal text tanpa token (0019:354).
   - Tambahkan baris "nilai params probe native" menurut resolusi 4: text param menerima literal ditambah token teks, integer, atau boolean (seperti entri args); boolean param menerima tepat satu token boolean utuh; token folder, runtime root, dan executable ditolak (`token_not_allowed_here`); secret ditolak (`secret_not_allowed_here`) (0023:350).
   - Tambahkan baris `runtime.versionCheck.args`: hanya literal, selain itu `token_not_allowed_here`. Baris ini tidak ditulis 0018.
7. **Settings schema (0009:246-258).**
   - Terapkan pengganti paragraf pembuka dari 0020 secara persis. Teks lama "`schemas/settings.schema.json` adalah dialek terbatas JSON Schema 2020-12 yang diparse serde menjadi model field bertipe (`SettingsSchema`). Model ini yang dirender form fitur 20 dan 29;" diganti teks baru dari 0020:827-839. Isinya: urutan deklarasi `properties` di root dan item array disimpan lewat visitor map, tanpa feature `preserve_order`; `fields()` dan field item mengikuti urutan itu; setiap issue `SettingsError` membawa `SettingsIssueKind` tetap (unknown, type, managed_filled, control_characters, duplicate_key, pattern, too_short, too_long, out_of_range, enum, too_many_items, too_few_items, required).
   - Tambahkan `x-stackly.printableOnly` ke daftar key di 0009:251: hanya dengan `secret: true`, selain itu `settings_schema_rule` (0019:355).
   - Perluas aturan `apply: reload` di 0009:255 ke params probe native (0023:351).
   - Di paragraf validate (0009:258), nyatakan bahwa setiap issue membawa `SettingsIssueKind` dan bahwa `fields()` mengikuti urutan deklarasi.
8. **Bagian Probe (0009:291-293).** Ganti paragraf probe native dengan kontrak 0023:
   - enum tertutup `NativeProbeKind` (non_exhaustive, satu variant `NatsReadiness`, dengan `ALL`, `descriptor`, `from_name`);
   - `NativeProbeDescriptor { name, endpoint_transports, params }` dan `NativeParamDescriptor`;
   - `NativeParamKind` `text { max_bytes }` dan `boolean`;
   - `NativeParamValue`, `NativeProbe`, dan `build`;
   - runner dipilih lewat match exhaustive di `core::supervisor::probe::native`, bukan lewat trait milik spec 0010.

   Ganti 0009:293 dengan: NATS memakai `nats-readiness` sejak spec 0023 (0023:348, 0023:355).
9. **Timeout dan sumber exit code (0009:305-318).** Tambahkan baris `ProbeSpec::Native`: memakai `attemptTimeoutMilliseconds` dari blok yang memakainya, dan tidak punya exit code (0023:353).
10. **versions.json (0009:336-375).**
    - Tambahkan tabel field 0018 (0018:357-367). Urutan tabelnya menjadi urutan output generator. Rinciannya:
      - `stable`: hanya true atau absen, hanya pada available, paling banyak satu per (os, arch) per file.
      - `build`: int ≥ 1, hanya pada available dan blocked.
      - `unpackedSize` dan `entryCount`: Count, hanya pada available, keduanya ada atau keduanya absen, dan 0 hanya bila `entryCount` 0.
      - `dependencies`: `{ name, kind (bundled/own/system), installName, version?, license?, sourceUrl? }`, dengan name unik. Kind bundled wajib membawa version, lisensi SPDX, dan sourceUrl https. installName 1–1024 byte.
      - `compatibility`: paling banyak 32 record dengan key unik.
    - Bentuk record compatibility: `scope`, `result`, `providerRevision` (ada tepat saat scope provider), `os`, `osVersion`, `osBuild?`, `arch`, `checks`, `knownLimitations`, `summary?`, `testedAt`, `evidence`. Key-nya `(scope, providerRevision, major osVersion, arch)`.
    - Ubah 0009:367 supaya blocked boleh membawa `build`, dan fixture menolak semua field baru.
    - Tambahkan paragraf tipe core:
      - `ValidatedPackage::versions() -> &VersionCatalog`;
      - `VersionArtifact { id, line: RuntimeLine, version: ExactVersion, target: Target, status }`;
      - `ArtifactStatus::Available { min_os, format, urls, sha256, size, archive_root, signing_policy, license, license_url, executables, stable, build, unpacked: Option<UnpackedSize { bytes, entries }>, dependencies: Option<Vec<ArtifactDependency>>, compatibility: Vec<CompatibilityRecord> }`;
      - `Blocked { reason, owner, since, license, license_url, build }`;
      - `Fixture { executables }`.

      (0017:625, 0018:392)
    - Tipe untuk `min_os`, `format`, dan `os_version` mengikuti resolusi 2.
    - Ganti kalimat "Field khusus tooling ... tidak ikut" di 0009:374 dengan daftar salinan dari 0018:394 dan aturan compatibility dari evidence:
      - scope runtime berasal dari `evidence/runtime/*/report.json` ditambah `spike-result.json`, dicocokkan lewat digest yang sama dengan sha256 artefak;
      - scope provider berasal dari `evidence/compatibility/*/report.json` ditambah `compatibility.json`;
      - untuk setiap key, ambil yang terbaru menurut `finishedAt`, lalu urutkan per scope dan `testedAt` menurun;
      - field opsional yang kosong tidak ditulis;
      - `--check` tetap membandingkan byte.
11. **Execution plan (0009:393-457).**
    - `Stdin { Null, Secret(PlanSecret), Text(String) }`. Text tampil di Debug dan Serialize karena bukan secret, dan mengubah `manifest_sha256` (0019:354).
    - `ProbeSpec::Native(NativeProbeSpec { target: ProbeTarget, probe: NativeProbe })`, dengan bentuk Serialize `type native, content { target, probe { name: nats-readiness, params { serverName, jetstream } } }`.
    - Text param yang dirender kosong, terlalu panjang, atau berisi karakter kontrol memberi `PlanError::UnrepresentableValue { field }`, dengan path `readiness.probe.params.<name>` atau `health.probe.params.<name>` (0023:352).
    - Sebut nama konstanta `CHILD_PATH` di 0009:453, dan catat bahwa exec check F18 memakainya ulang (0018:257).
12. **Paket fake, NATS, dan Traefik (0009:470, 0009:478-480) serta contoh NATS (0009:151-199).** `runtime.versionCheck` ditambahkan oleh F18 langkah 1. Paket fake memakai subcommand `version` yang sudah ada (0018:396). Readiness NATS memakai `nats-readiness` dari F23 (0023:355). Saat ini `providers/nats/provider.yaml:40` masih memakai http `/healthz`.
13. **Fuzz target (0009:486-497).** Catat `nats_readiness_protocol` sebagai milik spec 0023, dengan harness di `provider::fuzzing` (0023:354).
14. **Kode issue (0009:534).** Catat keputusan apakah kode loader catalog menjadi variant `IssueCode` (resolusi 12; 0017:233). Kalau tidak, nyatakan bahwa 0018, 0019, dan 0023 tidak membawa kode baru.
15. **API surface (0009:516-533).**
    - Tambahkan `ValidatedPackage::versions`, `SettingsSchema::fields` (urutan deklarasi) dengan `SettingsIssueKind`, accessor manifest untuk `versionCheck`, `NativeProbeKind::{ALL, descriptor, from_name}`, dan `native::build`.
    - Arahkan catatan `files()` di 0009:86 ("fitur 17 dan 20") ke fitur 20 dan K-1 spec 0017 (0017:625, 0017:529).
16. **Baris Assumed.** Spec 0011 (0009:566) mendapat pemeriksaan `printableOnly` di `put` (0019:355). Spec 0003 (0009:582) mendapat `stable`, `unpacked_size`, `entry_count`, `build`, dan `dependency` (0018:394).
17. **Skenario test kritis (0009:613).** *Himpunan tertutup* memakai nama native yang tidak ada di registry (0023:356). Saat ini `provider_rules.rs:157` dan :182 masih memakai `{ type: native, endpoint: http }`. Tambahkan penunjuk ke test milik pemilik untuk aturan baru (0018:398, 0019:513-515, 0023:389). Fixture yang dibutuhkan:
    - dua artefak stable untuk satu target;
    - stable pada blocked;
    - `unpackedSize` tanpa `entryCount`;
    - bundled tanpa lisensi;
    - record provider tanpa `providerRevision`;
    - record ganda untuk satu key;
    - `versionCheck` dengan token;
    - `versionCheck` tanpa capture group;
    - golden generator dengan semua field baru.
18. **Consequences (0009:680).** Ganti "belum ada" dengan apa yang sekarang ada, mengutip spec 0019 dan 0023. Di rationale, tandai keputusan kecil `/healthz` (`rationale.md:626`) dan kata-kata "runner lewat trait" sebagai sudah diganti, dan tambahkan catatan bertanggal untuk putaran update ini.
19. **Follow-up (0009:692-702).**
    - Tick 697 (F18; 0018:724).
    - Tick 698 (F15 dan F17; 0015:466, 0017:625).
    - Tick 699, yang diputuskan di 0020:631; F21 memakai ulang jalur yang sama (0019:580, 0023:466).
    - Tick 701 (F23; 0023:357).
    - 700 (F36) tetap terbuka.

    Tick dilakukan di sesi ini hanya bila lead setuju. Kalau tidak, lewat /sync lead (log :39).
20. **Setelah perubahan dokumen.** Jalankan `/graphify . --update` sesuai AGENTS.md dan commit sebagai `docs(spec)`. Kode dan schema yang digenerate tidak disentuh di sini. `schemas/provider/*.json` digenerate ulang oleh build pemiliknya (F18 s1, F19 s1-3, F23 s1).

Konflik dan resolusi:

1. **Himpunan field `ArtifactStatus`.** 0017 mendaftar compatibility, unpackedSize, entryCount, dependencies, dan stable tanpa `build`. 0018:362 dan :392 menaruh `build` di Available dan Blocked. Ambil set lengkap 0018: `build` di keduanya, field lain hanya di Available.
2. **Tipe `min_os`, `format`, dan `os_version` compatibility.** Di 0017 tidak bertipe. 0018:336 menambah `OsVersion` dan `ArtifactFormat` lewat update 0004, tetapi tipe itu baru dibangun di F18 langkah 6, setelah F17 M1.
   - Rekomendasi: pakai tipe domain `OsVersion` dan `ArtifactFormat`, dan minta lead mendaratkan kedua tipe itu dari update 0004 F18 sebelum F17 M1 langkah 1.
   - Ini memperketat aturan di `versions.rs:312-325`, yang kini hanya memeriksa teks tidak kosong. Aman, karena data saat ini numerik (12.0, 13.0).
   - Cadangan: string tervalidasi bertanda `// provisional: F18`.
3. **Urutan field output `versions.json`.** Ikuti tabel 0018:361-365, seperti yang diwajibkan 0018:394, bukan urutan contoh di 0018:373-376.
4. **Token boolean di text param native.** 0023:89 dan 0023:350 tidak konsisten. Ikuti tabel *Jenis probe native* di 0023:89, sesuai permintaan item 2 di 0023: text param menerima token teks, integer, dan boolean seperti entri args, sedangkan boolean param menerima tepat satu token boolean. Beri tahu pemilik 0023 supaya baris 350 diperbaiki.
5. **Runner native.** Match exhaustive menggantikan trait di 0009:292. Diterima, karena 0023:337 menyebutnya penyesuaian yang disengaja. Begitu juga params yang kini boleh memakai token.
6. **Artefak blocked.** Ubah 0009:367 supaya blocked boleh membawa `build`. Fixture tetap menolak semua field khusus artefak, termasuk yang baru.
7. **Cakupan generator.** Ubah 0009:374 dan daftar Assumed spec 0003 di 0009:582. Komentar kode `xtask/src/codegen/provider_versions.rs:141-144` berubah di build F18 langkah 1. Inventory sudah punya `build` dan `dependency` (`xtask/src/runtime/inventory.rs:76`, :127); `stable`, `unpacked_size`, dan `entry_count` datang dari update 0003. `codegen --check` kini juga bergantung pada folder `evidence/`.
8. **Semantik pattern.** Pertahankan satu sumber batas (`settings.rs:34-36`, yang saat ini privat; compile tanpa anchor perlu berbagi batas itu). Spec menyatakan bahwa `versionCheck` mencari baris per baris tanpa anchor, sedangkan pattern settings dan `dataMarker` mencocokkan seluruh nilai. Kata-kata usang "sedang diperbaiki" di 0019:468 tidak butuh tindakan di 0009.
9. **`--version` di fake server (0018:396 "kalau belum ada").** Tidak ada perubahan. `versionCheck` untuk fake memakai args `[version]` terhadap subcommand yang sudah ada (`crates/fake-server/src/main.rs:109`, :131-137), jadi AC-15 0009 tetap utuh.
10. **Follow-up purpose endpoint (0009:699).** Tick dengan mengutip 0020:631. 0019:580 dan 0023:466 cukup mewajibkan fitur 21 membaca purpose dari paket yang di-pin. `EndpointSpec` sudah membawa purpose (`manifest.rs:56`, :78).
11. **Baris Follow-up `files()` yang dirujuk 0017:625 tidak ada di 0009** (tidak ada di 0009:692-702 maupun di histori git). Ubah 0009:86 saja: pinning adalah fitur 20, K-1.
12. **Kode issue loader catalog (0017:233, implisit).** **Perlu konfirmasi lead.** Tidak ada bagian Usulan yang mengusulkannya. Rekomendasi: tambahkan keempat kode ke `IssueCode` di update ini, dengan `as_code` tetap stabil, dan test jumlah di `issue.rs:328` (kini 50) diperbarui di build F17. Kalau tidak, 0017 harus mendefinisikan tipe issue sendiri untuk `InvalidEntry`.
13. **Bentuk `SettingsIssueKind`.** Tambahkan tanpa mengubah `SettingsError.issues: Vec<Issue>`, misalnya sebagai kind per issue di field baru, sehingga `apps/daemon/src/map.rs:327` dan bentuk wire `provider.invalid_settings` tidak berubah. Secret reference yang tidak bisa diparse (0009:258) dipetakan ke kind `type`. Create tidak mencapai kasus ini, tetapi config edit di fitur 29 bisa.
    - Catatan kode: main saat ini memakai `BTreeMap` (`raw.rs:382`, :427; `settings.rs:219`, :357), dan `fields()` didokumentasikan "in name order" (`settings.rs:475-478`). Build F20 M1 langkah 4 membaliknya.
14. **`plain` dan "tanpa mode raw" (0009:29, 0009:590).** Invariant tetap berlaku karena `plain` tidak punya output tag. Yang diubah kalimat AC-9, bukan invariant-nya. `escape_text` butuh arm `Plain` yang tidak pernah meng-escape nilai. Delivery secret F11 (`render_secret_placeholder`, `crates/core/src/secret/delivery/mod.rs:96-97`) otomatis ikut.
15. **`Stdin::Text` dan `ProbeSpec::Native` adalah enum exhaustive** (`plan.rs:272-279`, :345-351) yang nanti di-match executor F10. Update 0009 hanya mencatatnya. Variant dan arm supervisornya mendarat bersamaan di F19 langkah 2 dan 5 serta F23 langkah 1 dan 2, lewat update 0010 terpisah (0019:579, 0023:359-365).
16. **Siapa yang men-tick Follow-up.** 0018:724 dan 0023:357 ingin update ini yang men-tick, sedangkan log koordinasi :39 menyerahkannya ke /sync lead. Karena hanya satu sesi pemilik yang mengedit 0009, tick di dalamnya aman bila lead mengizinkan. Kalau tidak, ditunda ke /sync.

Catatan: header 0009:4 masih "In Progress" walau F9 sudah ditutup. Tidak ada yang mengusulkan perubahannya; ini urusan /sync lead.

### Spec 0010: Process supervisor

**Status.** Build it terbuka dan belum ada milestone yang dicentang (`01-foundation.md:197-201`).

Di main:

- langkah 1 (spike);
- langkah 2 (4311e5c: `crates/domain/src/process.rs`, `crates/domain/src/supervisor.rs` dengan konstanta `FailureCode` dan `StageName`, `crates/platform/src/process.rs` dengan `ChildRole` di :78-89 dan `StdinSource` di :103-108);
- langkah 4 (b2a6b85: `crates/testkit/src/fake_process.rs`).

Belum di main:

- langkah 2a (belum ada `RecordProcessObservation` atau `latest_process_observation`);
- langkah 3 (belum ada `crates/platform-macos/src/process`);
- langkah 5–15, termasuk `crates/core/src/supervisor`, modul `instance` di protocol, `PortProbe`, dan `install_fake`.

`socket2` hanya ada sebagai dependency transitif (`Cargo.lock:1021`). tokio di core belum punya `net` dan `io-util` (`crates/core/Cargo.toml:24`). Protokol ada di 1.4.

**Timing:** lipat ke build F10. Jalankan satu architect (update) 0010 sekarang, sebagai perubahan teks saja, sebelum F10 M2 langkah 5 mulai. Lihat bagian 5.

Peta usulan:

| # | Sumber | Usulan | Item |
|---|---|---|---|
| P1 | 0020:843 | Input `instance.create`: `provider: { name, revision? }`. Minor contract change (0020:252) | 11, 12 |
| P2 | 0020:857 | Kolom error sinkron `instance.create` diganti kode baris error pertama `instance::create::evaluate` dengan semua `fields[]`, atau `resource.busy` dari lock nama | 11 |
| P3 | 0020:869 | Ganti paragraf Create 0010:367 dengan create penuh: `gather`, `begin` dengan precheck `evaluate`, `InstanceName`, `BusyPolicy::Reject`, langkah validate/pin/secrets/layout/register/init semuanya reversible, dan dikompensasi penuh | 12, 18 |
| P4 | 0020:875 | Baris `endpoints` di Startup transaction: `PortProbe::try_bind` dengan socket2, `reuse_addr: true`, `v6only` untuk IPv6, hasil Free, InUse, Denied, atau Unavailable | 1, 2 |
| P5 | 0020:887 | `InstanceSummary.endpoints.socketPath` dihitung saat dibaca dari paket yang di-pin dan port efektif | 9 |
| P6 | 0020:899 | Event `instance.removed { instanceId }` lewat `publish_resource` | 10 |
| P7 | 0020:911 | Sembilan baris kode `instance.*` setelah 0010:408 (detail dari 0020:393-401) | 13 |
| P8 | 0020:919 | Asumsi spec 0007: Queue untuk start dan reload, Reject untuk create | 14 |
| P9 | 0020:623 | Jawaban Follow-up 0010:658 | 18, 19 |
| P10 | 0018:345 (0018:728, :257, :63) | `ChildRole::RuntimeCheck` (tanpa lock, output Capture). Bila ditolak, exec check memakai `ChildRole::Probe` | 6 |
| P11 | 0018:345 | Panggil `runtime::launch::check_launchable` sebelum `spawn_main`. Quarantined menjadi `start_failed { quarantined }` dengan wire `runtime.quarantined` | 7 |
| P12 | 0018:345 | `classify_early_exit` memberi `crashed { will_retry: false, reason: binary_rejected_by_os }` tanpa budget restart. `FailureCode` mendapat `quarantined` dan `binary_rejected_by_os` | 7 |
| P13 | 0019:579 (0019:354, :532) | Init `Run` dengan `Stdin::Text` dikirim sebagai `StdinSource::Bytes(SecretValue::new(text))` lalu stdin ditutup | 5 |
| P14 | 0019:579 (0019:331) | Detail kegagalan probe exec: baris stderr non-kosong pertama lewat Redactor, dipotong 256 karakter, atau `exit <code>` dan `timeout` | 3 |
| P15 | 0019:533 (0019:400, 0023:208, :331, 0020:599) | `supervisor::fixture::pin_package(...)` dipisah dari `install_fake`, plus hook fixture untuk recording `LogSink` | 16 |
| P16 | 0019:476 | Bukti native: 0019 AC-10, AC-11, AC-13 untuk 0010:534 | 17 |
| P17 | 0019:466 | Assumed tanpa perubahan. Cocok dengan 0010:159-160, :223, :229, :248, :361 | tidak ada |
| P18 | 0023:363 | `ProbeOutcome::{Ok, Failed { reason, detail }}`, `probe_ok`, dan `connect_failed`/`timeout`/`protocol_error` untuk tcp dan http | 3 |
| P19 | 0023:364 | Dispatch `ProbeSpec::Native` ke `supervisor::probe::native::run` dengan connect yang bisa diinjeksi | 4 |
| P20 | 0023:365 | Bukti native: 0023 AC-8, AC-9, AC-11 untuk 0010:534 | 17 |
| P21 | 0023:329 | Assumed. `supervisor/probe` dibangun sebagai folder (allowlist `crates/core/src/supervisor/probe/native.rs`, 0023:212) | 3 |
| P22 | 0022:537 (0022:33, :486) | `InstanceSummary.ownership` (open enum Manual/Unrecognized, default Manual) dan `initialized: bool` (default true), juga di `instance.state_changed` | 8 |
| P23 | 0022:537 | Asumsi spec 0008 (0010:459): F14 membangun `inspect` dengan alias `get` | 15 |
| P24 | 0015:467 (0015:37, :512) | Follow-up 0010:657 terjawab oleh 0015 AC-16 (F15 M2 langkah 9) | 19 |

Daftar perubahan gabungan:

1. **Peta modul, baris stackly-core (0010:71).** Tambahkan socket2 untuk `PortProbe::try_bind`. Usulan spec 0001 (0020:651) diterapkan di putaran yang sama. Dibutuhkan F10 langkah 5.
2. **Startup transaction, baris `endpoints` (0010:249).** Terapkan teks 0020:875 secara verbatim.
   - Di edit yang sama, nyatakan bahwa opsi `try_bind` yang sama berlaku untuk alokasi port otomatis (0010:478), pemeriksaan port saat exit dini (0010:259), AC-17 (0010:37), dan catatan test (0010:531, :552).
   - Tetapkan hasil start untuk Denied dan Unavailable (K2).
   - Catat bahwa F10 langkah 5 membangun `try_bind`, dan F20 langkah 6 (0020:983) hanya memakai ulang.
3. **Probe, readiness, dan health (0010:278-283).**
   - Bentuk `ProbeOutcome`, reason sukses `probe_ok`, dan kode generik `connect_failed`, `timeout`, `protocol_error` untuk tcp dan http (0023:363).
   - Format detail kegagalan exec (0019:331, 0019:579).
   - Beri nama `ReasonCode` untuk exit exec non-zero dan untuk status http di luar `expect_status`, yang dibiarkan terbuka oleh kedua spec.
   - Nyatakan bahwa `supervisor::probe` adalah modul folder (0023:329).
   - Dibutuhkan F10 langkah 6.
4. **Bagian yang sama: dispatch native.** `ProbeSpec::Native` diteruskan ke `supervisor::probe::native::run`, dengan fungsi connect yang bisa diinjeksi (test memakai `tokio::io::duplex` dengan waktu paused), satu deadline per attempt, dan tanpa task latar (0023:364). Kodenya dibangun F23 langkah 5–6, dan F23 langkah 1 menambah arm match-nya.
5. **Executor plan, langkah Init `Run` (0010:223).** `Stdin::Text(text)` dikirim sebagai `StdinSource::Bytes(SecretValue::new(text))`, lalu stdin ditutup, tanpa variant port baru (0019:354, :579). Kodenya dibangun F19 langkah 5, setelah F19 langkah 2 menambah variant plan.
6. **Daftar `ChildRole` (0010:105).**
   - Tambahkan `RuntimeCheck`, tanpa lock dan dengan output Capture (0018:345).
   - *Aturan pembantu* (0010:227) dan AC-15 (0010:35) mendapat aturan timeout `RuntimeCheck` (K6).
   - Decision (0010:49) dan Security model (0010:517) mengizinkan exec check instalasi di `core::runtime::validate` sebagai pemanggil `spawn_helper` di luar supervisor.
   - Kodenya dibangun F18 langkah 8.
7. **Startup transaction dan exit dini.**
   - Baris `spawn` (0010:252): panggil `check_launchable` sebelum `spawn_main`. Quarantined menjadi `start_failed { quarantined }` dengan wire `runtime.quarantined`.
   - Bullet exit dini (0010:259): `classify_early_exit` memberi `crashed { will_retry: false, reason: binary_rejected_by_os }` dan `runtime.binary_rejected_by_os` tanpa budget. Urutannya terhadap pemeriksaan `PortProbe` ditetapkan (K7).
   - Daftar `FailureCode` (0010:410) dan kolom kegagalan `instance.start` (0010:362) mendapat `quarantined`, `binary_rejected_by_os`, dan dua kode runtime.
   - Kodenya dibangun F18 langkah 8, ditambah konstanta domain dan test di `crates/domain/src/supervisor.rs:7-37` dan :76-118.
   - Sebutkan langkah build yang memasang pemanggilan ini ke supervisor.
8. **Status dan event, tabel `InstanceSummary` (0010:333-348), dan Value sourcing.** Tambahkan `ownership` dan `initialized` dengan default, doc comment yang membedakannya dari kolom registry `instances.ownership` (spec 0005:203), dan fixture, termasuk summary tanpa kedua field (0022:537). Dibutuhkan F10 langkah 12.
9. **Status dan event, baris `endpoints` (0010:346).** Teks `socketPath` dari 0020:887: dirender dari `socketName` paket yang di-pin dengan port efektif endpoint rujukan, di-cache per provider revision key, atau null. Kodenya dibangun F20 langkah 9. Saat ini hanya PostgreSQL yang terdampak.
10. **Status dan event, event `instance.removed`.** Sisipkan bullet `instance.removed` sebelum `daemon.reconciling` (0010:353), dari 0020:899. Kodenya dibangun F20 langkah 7–8. DTO dan fixture-nya boleh ikut F10 langkah 12.
11. **Permukaan IPC, baris `instance.create` (0010:361).**
    - Input `provider: { name, revision? }` (0020:843) dan teks error sinkron (0020:857).
    - Tulis ulang juga kolom "Kegagalan operasi" yang sudah usang supaya menunjuk tabel *Operasi create_instance* di 0020 (0020:364-371). Kolom ini tidak tercakup 0020, dan kini kehilangan `catalog.pin_failed`, `instance.data_not_empty`, `instance.data_overlap`, `instance.short_key_taken`, `supervisor.data_unavailable`, `supervisor.data_in_use`, `instance.capability_unsupported`, `registry.generation_conflict`, dan `secret.invalid_value`.
12. **Paragraf Create (0010:367).**
    - Ganti dengan teks 0020:869.
    - Tambahkan kalimat staging: sampai F20 langkah 9, handler F10 langkah 13 adalah create sementara bertanda `// provisional: F20` (validasi di method, revision wajib dan ditolak bila kosong, kegagalan init meninggalkan instance failed). Meski begitu, F10 langkah 13 sudah memakai `BusyPolicy::Reject` dengan `LockKey::InstanceName`, dan langkah 12 sudah mengirim revision sebagai `Option`, sehingga kontrak tidak butuh bump lagi nanti.
    - Catat stage `pin`, karena `StageName::PIN` tidak ada di `crates/domain/src/supervisor.rs:40-73`.
    - Periksa ulang AC-11 0010 dan baris rekonsiliasi untuk `initializing` terhadap 0020 AC-11 (0020:33, :376).
13. **Kode error (0010:408).**
    - Sisipkan sembilan baris dari 0020:393-401: `instance.name_invalid`, `instance.provider_unavailable`, `instance.capability_unsupported`, `instance.runtime_incompatible`, `instance.storage_unsupported`, `instance.insufficient_space`, `instance.data_not_empty`, `instance.data_overlap`, `instance.short_key_taken`. Kategori, kapan, dan key details disalin persis.
    - Lebarkan kolom "Kapan" `instance.endpoint_invalid` supaya mencakup request ganda dan konflik antar endpoint draft (0020:7, :327).
    - Tambahkan satu baris yang membedakan `instance.capability_unsupported` (multi_instance) dari `supervisor.capability_unsupported` (reload, 0010:396).
14. **Asumsi lintas spec, poin Spec 0007 (0010:454).** Kata-kata BusyPolicy dari 0020:919: Queue untuk start dan reload, Reject untuk create.
15. **Asumsi lintas spec, poin Spec 0008 (0010:459).** F14 membangun create, start, stop, dan `inspect` dengan alias `get` (0022:537). Samakan juga baris Follow-up 0010:656. Done-when F14 (`01-foundation.md:237`) hanya butuh create, start, dan stop.
16. **Baris test di Value sourcing (0010:499) dan baris Peta modul untuk core dan stacklyd (0010:71, :75).**
    - Tambahkan `supervisor::fixture::pin_package(platform, layout, dir) -> ProviderRevisionRecord`.
    - `install_fake` menjadi salinan runtime ditambah `pin_package`, dan bisa dipakai untuk runtime saja.
    - Tambahkan hook yang hanya aktif di fixture, yang memasang recording `LogSink` ke `SinkSet` server in-process bila `DaemonConfig` belum punya (0019:533, 0019:400, 0023:208, 0020:599).
    - Dibutuhkan F10 langkah 13.
17. **Menjalankan test di Mac ini (0010:534).** Kutip spec 0019 AC-10, AC-11, AC-13 (0019:476) dan spec 0023 AC-8, AC-9, AC-11 (0023:365) sebagai bukti berbasis provider nyata.
18. **Consequences (0010:619), Follow-up (0010:658), dan rationale Keputusan 12 (`rationale.md:370-381`).** Catat bahwa create kini dikompensasi penuh, dengan revision opsional dan Reject, sesuai keputusan spec 0020 yang diterima lead pada 2026-10-20 (log koordinasi :66, :68). Ubah juga "tanpa mengubah field yang ada" di 0010:619 dan :658.
19. **Bukan untuk putaran architect.** Tick Follow-up 0010:657 (dijawab 0015 AC-16, hanya setelah evidence F15 M2 langkah 9) dan 0010:658 (dijawab 0020:623) adalah urusan /sync lead di akhir wave (log :39).

Konflik dan resolusi:

1. **Semantik create.** 0020:869 bertentangan dengan 0010:367, dengan "tanpa mengubah field yang ada" di 0010:619 dan :658, dan dengan Keputusan 12. Ikuti 0020, yang sudah diterima lead. F10 langkah 13 tidak menunggu F20 M1, yang justru bergantung pada F10 M2 (0020:976). Sebagai gantinya, F10 membangun create sementara dengan bentuk wire final, lalu F20 langkah 9 menggantikan handler-nya (K1).
2. **`PortProbe`.** 0020:875 bertentangan dengan "tanpa `SO_REUSEADDR`" di 0010:249. Ikuti 0020, dan bangun sekali di F10 langkah 5, bukan di F20 langkah 6. socket2 dikompilasi untuk target Windows yang diwajibkan AC-19. Tiga hal yang dibiarkan terbuka oleh 0020 harus diputuskan di sini (K2). Risiko sisa: server yang tidak memasang `SO_REUSEADDR` bisa gagal di samping listener wildcard yang dilaporkan probe sebagai Free, sehingga budget restart menghitung kegagalan itu, bukan `endpoint_in_use`.
3. **Timeout `RuntimeCheck`.** 0018:257 memakai `terminate_helper_group(force: true)` seketika, sedangkan AC-15 dan *Aturan pembantu* memakai TERM lalu KILL setelah 5 s. Keduanya tidak salah, karena aturan 0010 hanya mendaftar empat peran helper. Tambahkan aturan `RuntimeCheck` eksplisit (K6).
4. **Urutan exit dini.** `classify_early_exit` dari 0018 dan jalur `endpoint_in_use` `PortProbe` (0010:259) sama-sama melewati budget restart. Rekomendasi: evaluasi `classify_early_exit` lebih dulu, lalu port probe (K7).
5. **Detail probe exec dan `ProbeOutcome`.** 0019:331 dan :579 cocok dengan 0023:363. 0019 memiliki teks detail exec, dan 0023 memiliki reason code tcp dan http. Celah yang tersisa adalah `ReasonCode` untuk exit exec non-zero dan status http yang tidak cocok (K3).
6. **`Stdin::Text`.** Pertahankan `Bytes`, sesuai keputusan lead (log :58), sehingga port tidak berubah. `SecretValue::new` mengembalikan `Result` yang menolak input kosong, NUL, dan lebih dari 64 KiB (`crates/platform/src/secret.rs:73-84`). Executor butuh `expect` yang menyebut invariant 0019 AC-3, atau pemetaan ke `config_render_failed`. Lihat juga K8 tentang `expose()`.
7. **Versi protokol.** Lipat field 0022 dan revision opsional, dan opsional DTO `instance.removed`, ke minor bump F10 langkah 12 sendiri, sehingga 0020 dan 0022 tidak butuh bump tambahan. Koordinasikan nomornya dengan milestone wire F7 dan sesi Contract 1.4 (K4).
8. **`probe_lock` di kompensator.** Usulan 0020 ke spec 0007 (0020:824) memanggil `probe_lock` di kompensator `instance_tree`. Ini memakai method port yang sudah ada (`crates/platform/src/process.rs:941`), jadi 0010 tidak perlu berubah.
9. **Catatan Assumed.** 0019:466 dan 0023:329 cocok dengan teks 0010 saat ini. Satu-satunya hal yang perlu diperhatikan: bangun `supervisor/probe` sebagai folder.
10. **Tick Follow-up.** 0010:657 dan :658 di-tick lewat /sync lead, dan :657 menunggu run native F15 AC-16. Bila run itu gagal, 0015:512 mengembalikan keputusan ke /architect spec 0010.

### Spec 0011: Secret store & redaction

**Status.** Status spec In Progress (0011:4).

Di main sudah ada:

- tipe secret domain dan platform;
- `MacosSecretStore` dan `FakeSecretStore`;
- migration 0003;
- `SecretService` (put, reveal, delete, list, status, probe, `resolve_for_start`, `learn_running`);
- `SecretWorker` dan `Redactor`;
- delivery (`argv.rs`, `material.rs`);
- kompensator `secret_ref`;
- allowlist `expose` di check-deps (`xtask/src/check_deps/source_scan.rs:79-90`).

Scope mencentang langkah 0, 4, dan 6 (`01-foundation.md:220`). Kotak langkah 1–3 belum dicentang walau adapternya sudah merge. Yang belum dibangun:

- langkah 5 (`ipc::secret` di stacklyd dan `codes::secret`, yang belum ada di `codes.rs:109-335`);
- langkah 7 (sentinel suite);
- langkah 8 (`secret_input` CLI);
- langkah 9–11 (Swift dan GUI).

**Timing:** lipat ke build F11. Terapkan sebelum langkah 5 mulai.

Peta usulan:

| # | Sumber | Usulan | Item |
|---|---|---|---|
| P1 | 0020:933-943 (AC-7 0020:29, :368, :537; rationale G11) | `PutTarget::New { reference, owner, purpose, field: Option<SecretFieldRules> }` dengan `SecretFieldRules { min_length, max_length, printable_only }` (0011:195) | 1 |
| P2 | 0019:24 (AC-4), :355, :359, :399, :447, :515, :527, :578; rationale 0019:289-310 | `printableOnly` di `put`: karakter kontrol memberi `secret.invalid_value` dengan `field` dan `reason: control_characters`, tanpa tulis ke Keychain. Generate selalu lolos. Key details `reason` adalah perubahan kontrak minor | 2, 3, 4 |
| P3 | 0019:578 (bagian kedua; rationale 0019:421) | Timbang default terbalik: tolak karakter kontrol di semua secret, dengan opt-out `multiline: true` | 5 |
| P4 | 0020:945-955 (0020:375, :604, :624) | Bullet create (0011:211): untuk Bind, create tidak memanggil `put` dan hanya membaca nilai sekali. Kegagalan store saat `put` di create (Locked, AccessDenied, Busy) menjadi `secret.needs_secret` | 6 |
| P5 | 0015:39 (AC-18), :336-338, :468; rationale 0015:237-245 | Catat lewat /sync: login Keychain tetap, tanpa data protection keychain; satu persetujuan per item di setiap pergantian identitas | 9 |
| P6 (implisit) | 0020:446, :202, :458 | `SecretField` menerima minLength, maxLength, dan printableOnly untuk gating di klien | 8 |
| P7 (implisit) | 0020:423 | CLI create: `--secret <field>=generate\|prompt\|stdin\|file:<path>\|env:<NAMA>\|copy:<ref>\|bind:<ref>` | 7 |
| P8 | 0020:624-625 (rationale 0020:355) | /sync: Follow-up 0011:567 (bagian F20) terjawab; 0011:568 dijawab dengan perubahan | 10 |
| P9 | 0019:467; 0016:27, :159, :267; 0022:442 | Asumsi saja, sudah terpenuhi di main | 11 |
| P10 | 0023:180 (rationale 0023:321) | Detail probe NATS native "tidak lewat Redactor". Tidak diusulkan untuk 0011, tetapi bertentangan dengan 0011:285 | 11 |

Daftar perubahan gabungan:

1. **Bullet `target` di *Use case isi atau ganti nilai* (0011:195).** Ambil isi 0020 item 1 dengan nama yang dipakai di main, bukan nama yang bertabrakan:
   - `PutTarget::New { reference, owner, purpose, field: Option<FieldContext> }`;
   - `FieldContext { name: SecretFieldName, rule: SecretFieldRule }`;
   - `SecretFieldRule { min_length, max_length, apply, printable_only }`, diambil dari secret field di schema.

   `field` terisi untuk create dan restore (spec 0020) dan kosong untuk env up. `put.rs:30-51` sudah punya `field: Option<FieldContext>`. `SecretFieldRule` (`rules.rs:13-22`) belum punya flag printable.
2. **Langkah 3 (0011:202).** Tambahkan aturan printableOnly dari 0019 AC-4.
   - Value atau CopyFrom yang berisi karakter kontrol (U+0000–U+001F, U+007F–U+009F) untuk field dengan `printable_only` memberi `secret.invalid_value` dengan `field` dan `reason: control_characters`.
   - Value ditolak sebelum Keychain disentuh. CopyFrom ditolak setelah sumbernya dibaca. Tidak ada yang ditulis, dan pesannya tidak menyebut nilai.
   - Generate selalu lolos.
   - Aturan ini juga berlaku untuk `PutTarget::Existing`, lewat aturan field yang memakai reference itu (`secret.set`, 0019:399).
3. **Kode error di wire, baris `secret.invalid_value` (0011:356).** Tambahkan key details opsional `reason` (`control_characters`) dan perluas kolom "Kapan". AC-21 (0011:42) mengikuti. Bila F11 langkah 5 membuat `codes::secret` sekaligus dengan key ini, tidak perlu minor bump tambahan. Kalau tidak, bump minor (0019:359).
4. **AC-6 (0011:27), AC-7 (0011:28), Value sourcing (0011:396), dan *Asumsi spec 0009* (0011:341).** Sebut printableOnly dan `x-stackly.printableOnly` (spec 0019 lewat update spec 0009). `SecretFieldRule::from_setting` (`crates/core/src/secret/rules.rs:37-48`) mengisi flag ini begitu model settings spec 0009 membawanya.
5. **Keputusan yang dicatat (0019:578 bagian kedua).** printableOnly tetap opt-in, sesuai Keputusan 9 di 0019. Default terbalik (tolak karakter kontrol di semua secret, dengan `multiline: true`) sudah ditimbang dan ditunda, karena akan mengubah `secret.set` untuk reference user yang sudah ada, yang menurut AC-7 menerima UTF-8 apa pun tanpa NUL.
6. **Bullet create instance (0011:211).** Ambil perilaku 0020 item 2, tetapi pertahankan satu pintu masuk.
   - Usulan kata-kata: untuk Bind, create memanggil `put` dengan `Bind` dan `PutTarget::New.field`. `put` membaca nilai yang di-bind sekali dengan interaksi pemanggil, menjalankan `Redactor::learn`, memeriksa aturan field, dan tidak menulis apa pun. Pemakaiannya ditulis `ReplaceSecretUses` di batch pendaftaran. Dengan begitu AC-5 (0011:26) dan 0011:196 tetap benar.
   - Bila arsitek tetap memakai teks verbatim 0020, sebutkan juga method `SecretService` untuk membaca dan memeriksa.
   - Tambahkan kalimat kegagalan store: Locked, AccessDenied, dan Busy saat create dipetakan ke `secret.needs_secret` dengan satu item per reference. OutcomeUnknown, NotSupported, dan Backend tetap memakai kode `secret.store_*`-nya.
7. **Input aman CLI (0011:307-321).** Aturan membaca setiap sumber tinggal di `secret_input`. Perintah dengan beberapa secret field (create, restore) menulis sumber per field sebagai `--secret <field>=<generate|prompt|stdin|file:<path>|env:<NAMA>|copy:<ref>|bind:<ref>>` (spec 0020). Default-nya generate, dan stdin hanya untuk paling banyak satu field. Aturan "tanpa TTY dan tanpa flag sumber, gagal" (0011:318) berlaku untuk `stackly secret set`. Edit konsistensi ini tersirat dari 0020:423.
8. **GUI `SecretField` (0011:325) dan AC-19 (0011:40).** Komponen menerima minLength, maxLength, dan printableOnly untuk gating tombol utama di klien. Daemon tetap yang memeriksa (0020:446). Alur create di GUI menawarkan Generate, Enter, dan Copy from, tanpa Bind.
9. **Keputusan fitur 15 (0015 AC-18, 0015:338).**
   - Tulis ulang 0011:59 ("dibahas ulang di fitur 15") sebagai sudah diputuskan: login Keychain tetap dan data protection keychain tidak dipakai (spec 0015 *Keychain setelah signing*).
   - Perluas 0011:454: setiap pergantian identitas (ad hoc ke Apple Development, lalu ke Developer ID di fitur 60) butuh satu persetujuan per item lewat aksi user atau penulisan ulang lewat `stackly secret set`, tanpa migrasi otomatis dan tanpa ACL yang dilonggarkan.
   - Tandai bagian keputusan di Follow-up 0011:566 sebagai terjawab. Bagian bukti tetap terbuka sampai ada evidence 0015 AC-18 (OQ-012).
10. **Follow-up lewat /sync setelah build F20.** Tick bagian fitur 20 di 0011:567 (dijawab 0020 AC-7, langkah `secrets`, dan AC-11). Tulis ulang 0011:568: preflight bersifat read-only dan tidak pernah memanggil `put`, dan baris secret di sheet berubah setelah stage `secrets` (0020:625).
11. **Tanpa perubahan teks** untuk 0016 (bergantung pada AC-2), 0019:467 (asumsinya terverifikasi di main: `put.rs:30-51`, `generate.rs:7-10`, `delivery/material.rs:38`, `delivery/argv.rs:13-22` dan :61-80), 0022:442, dan 0023:180 (resolusi 4).

Konflik dan resolusi:

1. **0020 item 1 dan kode main.** `put.rs:30-51` sudah memakai `field: Option<FieldContext>`, dan `SecretFieldRules` adalah nama trait (`rules.rs:57-64`). Struct usulan juga membuang nama field yang dibutuhkan details `secret.invalid_value` (`error.rs:138-147`, :312-323). Pakai nama dari kode dan tambahkan `printable_only` ke `SecretFieldRule`. Ini sekaligus menggabungkan usulan printableOnly dari 0019 ke tipe yang sama.
2. **0020 item 2 (create melewati `put` untuk Bind).** Ini bertentangan dengan 0011 AC-5, 0011:196 dan :211, dan `put.rs:238-242`. Beri `put(Bind)` langkah baca-dan-periksa bila `field` terisi. Hasilnya sama dengan yang diinginkan 0020 tanpa pintu masuk kedua. Env up tetap memanggil `put(Bind)` tanpa aturan field.
3. **NUL di value create.** 0020 AC-2 dan AC-7 mengirim NUL di `value` create ke precheck sebagai `secret.invalid_value`. Sebaliknya, 0011 AC-7 dan 0011:347 mengarahkan nilai kosong, terlalu besar, dan berisi NUL lewat `Validate` ke `request.invalid_params`, dan main menerapkannya di `SecretText::field_code` (`crates/protocol/src/secret.rs:84-98`). Rekomendasi: pertahankan 0011, dan pemilik 0020 mengeluarkan NUL dari daftar precheck. Alternatifnya: arsitek menulis pengecualian create eksplisit di 0011 AC-7 (K15).
4. **0023:180 (detail probe native tidak di-redact) dan 0011:285 (runner probe me-redact setiap detail observasi).** Pertahankan 0011:285 sebagai satu-satunya aturan. Kalimat 0023 sebaiknya dibaca sebagai "probe native tidak menambah redaksi sendiri", dan perbaikannya urusan pemilik 0023.
5. **Sintaks CLI 0020:423 dan 0011:307/318.** Diselesaikan oleh item 7: aturan baca dipakai bersama, ejaan flag per perintah.
6. **Masalah laten, bukan usulan fase 02.** `StdinSource::Bytes(SecretValue)` (`crates/platform/src/process.rs:103-108`) adalah pembawa yang juga dipakai 0019 untuk SQL init yang bukan secret. Menulis byte itu di kode proses `stackly-platform-macos` atau di testkit `fake_process` butuh `expose()` di luar `EXPOSE_ALLOWED` (`source_scan.rs:85-90`, yang hanya mengizinkan modul `secret` di platform-macos dan `fake_secret` di testkit). Tabel allowlist 0011 (0011:293-301) atau desain 0010 harus menyelesaikannya sebelum kode spawn 0010 mendarat (K8).
7. **Cakupan pencarian.** 0017, 0018, dan 0022 tidak punya usulan untuk 0011. 0016 bergantung pada AC-2 (konsisten dengan 0011:23 dan :158).

### Spec 0012: Design system & UI foundation

**Status.** Build selesai dengan keempat milestone tercentang (`docs/scope/ui-macos.md:56-60`) dan Test selesai (:62). Verify masih terbuka (:61). Tidak ada branch yang menyentuh `docs/specs/0012` atau folder StacklyUI milik 0012 (Tokens/, Status/, State/, Confirmation/, Gallery/). `claude/f13-wiring-7b3a78` hanya mengubah folder milik 0013.

**Timing:** sekarang. Perubahannya dua penunjuk dokumen tanpa dampak kode, plus konfirmasi /sync. Boleh ikut sesi update 0013 supaya praktis, tetapi tidak ada yang menahannya.

Peta usulan:

| # | Sumber | Usulan | Item |
|---|---|---|---|
| P1 | 0017:353 dan baris 0020:632 (*Jawaban*, 0020:618-634) | DTO capability yang ditunggu 0012 adalah `CapabilityStatus` (spec 0017), dipakai ulang oleh `CreateForm` di 0020. Capability instance yang sudah ada ditambahkan ke `InstanceSummary` oleh F21 atau F22 | 1, 2 |
| P2 | 0018:733 (0018:544-554, :684) | Tabel status runtime masuk `apps/macos/design.md` "New status sets". Tidak ada perubahan teks 0012 | 3 |
| P3 | 0017:33 (AC-12), :583 (0017:396-410) | Tabel *Tone dan label* `CatalogAppearance` masuk `design.md` | 3 |
| P4 | 0022:497 | Bagian Overview dan Instances di `design.md` (tabel Needs Attention, aturan kartu) | 3 |
| P5 | 0016:335 | Baris "0012: tipografi judul onboarding, token, ViewState, konfirmasi" terjawab. Tidak ada perubahan; 0012:78 dan :80 sudah ada | tidak ada |

Daftar perubahan gabungan:

1. **Asumsi lintas spec (0012:361).** Tandai asumsi capability "Assumed spec 0013 dan fitur 20" sebagai selesai. DTO capability-nya adalah `CapabilityStatus` dari spec 0017, yang dipakai ulang oleh `CreateForm` di spec 0020. Capability untuk instance yang sudah ada ditambahkan ke `InstanceSummary` oleh F21 atau F22 saat dibutuhkan (0017:353, 0020:632). Penunjuk lama ke 0013 sudah usang, karena 0013 tidak punya teks DTO capability.
2. **Baris Value sourcing `UnsupportedContent.reason, .disabled(reason:)` (0012:414).** Ganti "dikirim lewat DTO spec 0013 dan fitur 20" dengan "`CapabilityStatus` spec 0017 (catalog dan form create spec 0020); capability instance yang sudah ada lewat `InstanceSummary` (fitur 21 atau 22)".
3. **Tidak ada perubahan teks di *Status set baru* (0012:131).** Aturannya sudah ada (`design.md:257-259`, dan `design.md:259` bahkan sudah memberi contoh "runtime states in feature 18"). Jalankan /sync setelah F17 M3 langkah 8, F18 M4 langkah 15, dan F22 M3 langkah 8 untuk memastikan tabel `CatalogAppearance` (Available bernada neutral, bukan informative), tabel status runtime (installed positive; resolving/downloading/verifying/installing informative; failed dan missing critical; drifted caution; absent neutral "Removed"; unrecognized caution), dan tabel Overview/Instances sudah masuk `design.md`. Tidak ada yang perlu di-tick di 0012.

Konflik dan resolusi:

1. **Tidak ada konflik antar usulan.** Tidak ada usulan fase 02 untuk 0012 yang saling bertentangan, dan tidak ada yang mengubah token, tabel status, atau API.
2. **Dynamic Type di halaman Runtimes.** 0018:556 menyebut teks halaman Runtimes "mengikuti Dynamic Type", padahal menurut 0012:93 dan `design.md:459` text style di macOS 27 mengabaikan `dynamicTypeSize` dan pemeriksaan teks besar dilakukan dengan lebar sempit 640 pt. Pertahankan 0012, dan minta pemilik 0018 memperbaiki kata-katanya.
3. **Komponen sementara sebelum F67.** 0012:61 menyerahkan provider tile, sheet dengan footer, staged progress, dan preflight checklist ke F67. 0017:380 (`ProviderTile`), 0020:107 (`PreflightSection`, footer sheet, `CreateProgressSection`), dan 0022:274 (`ActionFeedbackBanner`, `// provisional: F67`) membangun versi sementara yang nanti diadopsi F67. 0012 tidak perlu berubah. Opsional: satu baris catatan di baris batas 0012:61.
4. **Simbol status preflight.** 0020:476 memperkenalkan simbol status preflight (ok, caution, error, unrecognized) dengan tone 0012 tanpa menyebut `design.md`. Menurut 0012:131 ini status set baru. Ini perlu diberitahukan ke pemilik 0020, bukan diubah di 0012.
5. **Toast GUI dari 0018.** Toast 0018 ("Installing {name} {version}…", 0018:540) sebelum F67 ada harus mengikuti 0012:226 (tanpa glass; opaque atau presentasi sistem). Ini catatan untuk sisi 0018.
6. **`toneSurface` di 0016.** 0016:102 memakai `toneSurface(.neutral)` tanpa argumen strength, padahal kode mewajibkannya (`Tokens/Surfaces.swift:45-48`), jadi 0016 harus memberi `.badge` atau `.banner`. Ini detail sisi 0016.

### Spec 0013: App shell SwiftUI & Swift client

**Status.** Build terbuka. Milestone 1–3 tercentang (`ui-macos.md:76-78`). Milestone 4 (:79) sedang berjalan di branch `claude/f13-wiring-7b3a78`, yang 11 commit di depan main dan belum merge.

Branch itu sudah membangun:

- langkah 15: `LiveShell.swift`, `ShellBootstrap.assemble`, dan wiring `AppDelegate`;
- langkah 17: `OperationCenter`, case operasi di `StatusAdapter`, dan `didApply`/`remove`/`offer`/`evict` di `ResourceStore`;
- bagian daemon langkah 18.

Branch itu juga mengedit index.md 0013 (baris 4 dan :193). Langkah 16 belum mulai karena menunggu 0010 langkah 12. Isinya case instance di `StatusAdapter`, `InstanceStore`, `ProvisionalInstanceList`, dan aturan `.patch` statusSeq untuk revision yang sama. Semua milestone GUI fase 02 menunggu F13 M4.

**Timing:** lipat ke F13 M4. Jalankan architect update setelah `f13-wiring` merge dan sebelum sesi langkah 16 mulai.

Peta usulan:

| # | Sumber | Usulan | Item |
|---|---|---|---|
| P1 | 0015:487 (0015:303-317; AC-13 0015:34) | Banner untuk unreachable dengan `ipc.endpoint_missing` atau `ipc.connection_refused` mengikuti tabel *Banner per state* di 0015. `ConnectionHint` tanpa "Start it with stacklyd" | 6, 7, 9 |
| P2 | 0015:487 (0015:318-322) | `BackgroundAgentDiagnosticsSection` dan `CommandLineToolSection` (`// provisional: F32`) di Diagnostics | 1, 9 |
| P3 | 0015:488 (0015:327; AC-11 0015:33) | Aturan source: `import ServiceManagement` hanya di `Agent/SMAppServiceAgent.swift`; API tulis file hanya di `Agent/CommandLineTool.swift` | 11 |
| P4 (implisit) | 0015:301, :355; 0016:97 | `BackgroundAgentModel` dimiliki `DaemonModel`, dengan jadwal `refresh()`. StacklyUI mendapat `Agent/` dan me-link ServiceManagement | 1, 5 |
| P5 (implisit) | 0015:382; 0016:162, :267 | GUI tahu apakah `STACKLY_ROOT` aktif (hanya Bool) lewat `ShellRuntime` atau `DaemonModel` | 5 |
| P6 | 0015:469, :563 (0015:340-342) | Follow-up 0013:622 terjawab. Tick saat F15 M3 mendarat | 13 |
| P7 | 0016:344 (0016:91, :94) | `MainWindowRoot` menjadi konten Window. Accessor, `openWindow` capture, `windowWillClose`, dan frame minimal pindah dari `AppShell`. Menu Help "Welcome to Stackly" | 8 |
| P8 | 0016:345 (0016:73) | `ShellRuntime.init(model:closeNotice:onboardingStore:)` dan `ShellBootstrap.live(...onboardingStore:)`. `ShellRuntime` memegang `OnboardingModel` dan `CommandLineToolModel` | 5 |
| P9 | 0016:346 (0016:66-69) | Pengecualian di 0013:100 dan :483: preferensi `onboarding.finished` di `UserDefaults.standard` | 2 |
| P10 | 0017:31 (AC-10), 0018:38 (AC-17), 0018:733, 0022:23 (AC-2), :27 (AC-6), :539 | Halaman Catalog, Runtimes, dan Overview menggantikan `UnbuiltPage`, dan Instances menggantikan `ProvisionalInstanceList`. Follow-up dipecah per fitur | 13 |
| P11 (implisit) | 0017:382, :584; 0018:529, :683 | `CatalogStore` dan `RuntimeStore` didaftarkan ke `ShellSync` oleh `DaemonModel` | 1, 5 |
| P12 | 0022:539 (0022:279-286, :380) | `ShellNavigator`, `ShellDestination`, dan `EnvironmentValues.newInstanceAction` di Shell/ | 1, 8 |
| P13 | 0020:103, :449, :1001 | Tombol "New Instance" di daftar sementara sebelum F22 ada | 10 |
| P14 | 0020:450, :999, :612; 0022:539, :547; rationale 0020:513 | `InstanceStore` memetakan `instance.removed` ke `.delete(id, revision)` | 10, 12 |
| P15 | 0022:539 (0022:434; AC-14 0022:35) | `InstanceStore` menyaring instance retained, limit 200 per halaman | 4, 10 |
| P16 | 0022:539 (0022:301, :418, :382) | `ResourceStore.itemFailures`, `ItemFailure { error, revision }`, `retry(_:)` | 4 |
| P17 | 0022:539 (0022:199; AC-4 0022:25) | `CapacityModel` dimiliki `DaemonModel` | 1, 5 |
| P18 | 0022:539 (0022:66; rationale 0022:143) | Aturan `.patch` statusSeq tetap milik 0013 M4 langkah 16. Hanya konfirmasi pemilik | 10 |
| P19 (implisit) | 0016:121, :122, :144, :162, :258-264 | Dokumentasikan `ShellPresentation.status: ShellStatus` dengan `.unreachable(lastKnownAt:since:)` dan `footer` | 3 |
| P20 (implisit) | 0020:108, :1001, :1052; 0016:94, :98 | `.commands` di Window utama: File > New Instance… ⌘N (`// provisional: F69`) dan Help | 8 |

Daftar perubahan gabungan:

1. **Batas (0013:58-75) dan Peta package (0013:86).** Tambahkan pemilik:
   - spec 0015 untuk baris banner agent, `BackgroundAgentDiagnosticsSection`, `CommandLineToolSection`, dan folder `StacklyUI/Agent/` (ServiceManagement);
   - spec 0016 untuk `MainWindowRoot` dan `StacklyUI/Onboarding/`;
   - API shell di Shell/: `ShellNavigator`, `ShellDestination`, dan `newInstanceAction`. Diusulkan 0022, dibangun oleh konsumen pertama di antara F18 M4, F20 M3, dan F22 M3;
   - `CatalogStore` (0017), `RuntimeStore` (0018), dan `CapacityModel` (0022) yang dimiliki `DaemonModel`.
2. **Model data (0013:100) dan Key invariants (0013:483).** Tambahkan pengecualian: F16 menyimpan satu key `UserDefaults`, `onboarding.finished`. Rumuskan invariant-nya supaya symlink CLI yang dikonfirmasi user (0015 AC-14) jelas tidak termasuk "state atau cache".
3. **Model data (0013:112, :115).** Dokumentasikan `ShellPresentation.status: ShellStatus` dengan `.unreachable(lastKnownAt:since:)`, dan `ShellPresentation.footer`, sesuai kode (`Shell/ShellPresentation.swift:18-40`, :190-194) dan sesuai pemakaian di 0016.
4. **ResourceStore (0013:240-248) dan API surface (0013:420).** Tambahkan `itemFailures`, `ItemFailure { error, revision }`, dan `retry(_:)` (0022). Entri dipasang saat refetch gagal, dan dihapus saat item itu berhasil diterapkan atau saat resnapshot selesai. Tetapkan mekanisme pengecualian retained, dengan catatan bahwa tombstone dihapus di setiap resnapshot (0013:248; K16).
5. **Model shell (0013:252-262).**
   - Signature menjadi `ShellRuntime.init(model:closeNotice:onboardingStore:)` dan `ShellBootstrap.live(rootOverride:clientVersion:now:onboardingStore:)`, dengan default `UserDefaultsOnboardingRecordStore()`. Parameter ini diteruskan lewat `assemble` internal milik branch.
   - `ShellRuntime` memegang `OnboardingModel` dan `CommandLineToolModel`, dan mengekspos "`STACKLY_ROOT` aktif" sebagai Bool.
   - `didBecomeActive()` memanggil `systemRetry()`, `BackgroundAgentModel.refresh()`, dan `CommandLineToolModel.refresh()`.
   - `DaemonModel` juga memegang `BackgroundAgentModel`, `CapacityModel`, `CatalogStore`, dan `RuntimeStore`, di samping `InstanceStore` dan `OperationCenter`.
   - `BackgroundAgentModel.refresh()` jalan saat launch, dari `didBecomeActive()`, saat presence menjadi unreachable, setiap 5 s selama unreachable dan app aktif, dan setelah setiap aksi.
   - `CapacityModel` memanggil `DaemonModel.read(.storageGetCapacity)` saat Overview muncul, setiap 60 s selama Overview tampil dan terhubung, dan setiap `ShellSync.completedRounds` berubah, paling banyak satu panggilan sekaligus.
   - `CatalogStore` menyimpan `CatalogInfo` terakhir di dalam closure `fetchPage`, tanpa mengubah `ResourceStore`. `RuntimeStore` mendapat `installing` dari `OperationCenter.updates(for:)`.
6. **AC-7 (0013:28) dan invariant (0013:476).** Ubah "GUI tidak pernah menjalankan stacklyd atau binary lain" supaya jelas bahwa meminta launchd menjalankan agent terdaftar lewat SMAppService (Start stacklyd, Turn On; spec 0015) diizinkan, sedangkan spawn tetap dilarang.
7. **Tampilan status daemon (0013:264-290), AC-9 (:30), AC-10 (:31), AC-11 (:32), pengumuman (:262, AC-21 :42), dan cek manual 1 (:526).**
   - Untuk unreachable dengan `ipc.endpoint_missing` atau `ipc.connection_refused`, judul, pesan, dan aksi banner diambil dari tabel state agent 0015:
     - not_registered: caution, Turn On dan Open Diagnostics;
     - requires_approval dan disabled: caution, Open Login Items dan Check Again;
     - enabled di bawah 15 s: informative "stacklyd is starting…";
     - enabled 15 s atau lebih: caution, Start stacklyd dan Open Diagnostics;
     - not_found: critical;
     - unavailable dengan `{detail}` dari `BackgroundAgentModel.unavailableDetail`.
   - Footer dan gate 0013 dipertahankan untuk kedua kode itu, begitu juga awalan "Showing the last known state from {waktu}." bila `lastKnownAt` ada.
   - Kode unreachable lain tetap "Can’t connect to stacklyd.".
   - `DaemonBannerAction` mendapat Turn On, Open Login Items, Check Again, dan Start stacklyd, semuanya `mutates: false`.
   - Satu aturan pengumuman per transisi.
8. **Shell jendela (0013:301-327; snippet scene :305-314).**
   - `MainWindowRoot(runtime:windowWillClose:)` menjadi konten Window dan membungkus wiring `LiveShell` dari langkah 15. `MainWindowAccessor`, `openWindow` capture, `windowWillClose`, dan frame 900 × 600 pindah ke sana dari `AppShell`.
   - Tambahkan `.commands`: Help "Welcome to Stackly" (0016), dan File > New Instance… ⌘N bertanda `// provisional: F69` yang dinonaktifkan selama onboarding (0020).
   - `AppShell` tetap memegang `@SceneStorage("shell.section")` dan mencerminkannya ke `ShellNavigator.section` dua arah. `newInstanceAction` ada di environment.
9. **Diagnostics koneksi (0013:329-350) dan `ConnectionHint` (0013:346).**
   - Hapus "Start it with stacklyd" dan `// provisional: F15` (`ConnectionHint.swift:64-67`). Hint untuk `endpoint_missing` dan `connection_refused` memakai judul dan pesan banner agent.
   - Tambahkan `BackgroundAgentDiagnosticsSection` (kartu `.cardSurface()` berisi State, What to do, App, Label, Last launched by launchd, Last checked, Last error, ditambah Check Again dan Copy Details) dan `CommandLineToolSection` (`// provisional: F32`) di bawah Connection, di atas slot doctor F34 (`DiagnosticsPage.swift:20`).
10. **Daftar instance sementara (0013:352-359), AC-17 (:38), dan badge AC-13 (:34, :320). Diterapkan di langkah 16.**
    - `InstanceStore` memakai limit 200 dan mengecualikan instance retained, termasuk dari count.
    - `instance.removed` dipetakan ke `.delete` pada revision event, begitu spec 0020 menambah event itu.
    - Aturan `.patch` statusSeq untuk revision yang sama diterapkan.
    - AC-17 diubah supaya daftar sementara menampilkan tombol toolbar New Instance dan aksi kedua di empty state bila `newInstanceAction` terisi (`.mutationGated()`), sampai F22 menggantikannya.
11. **AC-23 (0013:44) dan Aksesibilitas (0013:391).** Tambahkan aturan source: `import ServiceManagement` hanya di `Agent/SMAppServiceAgent.swift`, dan `createSymbolicLink`, `createDirectory`, `rename`, serta `removeItem` hanya di `Agent/CommandLineTool.swift`. Test-nya di `StacklyUITests/SourceRuleTests.swift`.
12. **Asumsi lintas spec (0013:397).** Penghapusan instance kini berupa event `instance.removed` dari spec 0020.
13. **Follow-up (0013:622, :625).** Tick :622 saat F15 M3 mendarat (tanda tangan peer ditunda ke F60/F63, 0015:340-342). Pecah :625 per fitur dan tick F17 (0017 AC-10), F18 (0018 AC-17), dan F22 (0022 AC-2, AC-5, AC-6) lewat /sync saat masing-masing mendarat.

Konflik dan resolusi:

1. **Banner agent 0015 dan AC-9/AC-10 0013.** AC-9 mematok banner tanpa daemon sebagai "Can’t connect to stacklyd." dengan Reconnect dan Open Diagnostics, padahal kasus itu persis `ipc.endpoint_missing`.
   - Rekomendasi: baris state agent hanya mengganti judul, pesan, dan aksi untuk kedua kode. Footer ("stacklyd unavailable", critical), `MutationAvailability .blocked("stacklyd is unavailable.")`, dan awalan AC-10 tetap.
   - Saat presence menjadi unreachable dengan kode itu, umumkan kalimat agent 0015 sebagai ganti "Can’t connect to stacklyd.", supaya VoiceOver hanya mendengar satu pengumuman per transisi (0015:319 punya pengumuman per state agent sendiri).
   - AC-9 dan cek manual 1 ditulis ulang untuk baris agent.
2. **`MainWindowRoot` dari 0016 dan branch langkah 15.** Branch menjadikan `LiveShell(runtime:windowWillClose:)` konten Window, jadi `MainWindowRoot` menggantikan posisi `LiveShell` dan merender `OnboardingFlow` atau wiring `AppShell` dari `LiveShell`. Tulis ini setelah branch merge, supaya spec mendeskripsikan `LiveShell` dan bukan snippet usang `AppShell().environment(runtime.model)` di 0013:306.
3. **Signature `ShellBootstrap.live` dari 0016 dan `assemble(...) -> LiveShellParts` internal milik branch.** Tambahkan `onboardingStore` dan Bool override ke keduanya. `live()` tetap satu-satunya pintu publik.
4. **"New Instance di daftar sementara sebelum F22" (0020) dan AC-17 "tanpa aksi" (0013:38, :357-358).** Rekomendasi: ubah AC-17 supaya tombol toolbar dan aksi empty state hanya dirender bila `newInstanceAction` tidak nil, memakai `.mutationGated()`. Alternatifnya: wajibkan F22 M3 sebelum F20 M3 langkah 14, dan AC-17 tidak berubah.
5. **Kepemilikan `ShellNavigator`.** 0022 membangunnya di M3 langkah 6, tetapi 0018:530 dan 0020:439, :448 butuh navigasi terprogram dan bisa mendarat lebih dulu. Tulis `ShellNavigator`, `ShellDestination`, dan `newInstanceAction` di 0013 sebagai API shell bersama, dibangun oleh yang pertama dari F18 M4, F20 M3, atau F22 M3. Saat ini seleksi masih berupa `@SceneStorage` privat (`AppShell.swift:21`).
6. **Penyaringan retained dari 0022 dan perilaku `ResourceStore`.**
   - `complete()` memasukkan setiap item snapshot (`ResourceStore.swift:206-216`) lalu menghapus semua tombstone (:222, sesuai 0013:248). Jadi untuk snapshot, "retained diperlakukan sebagai delete dengan tombstone" hanya bisa berarti "tidak dimasukkan".
   - Untuk event, `reduce` mengembalikan `.delete(id, revision)`.
   - Spec harus menyebut hook-nya: filter di `fetchPage` ditambah `reduce`, atau predikat pengecualian di `ResourceStore` (K16).
   - `offer` dan `evict` milik branch hanya mencakup event, bukan snapshot.
7. **`itemFailures` dari 0022 dan refactor `ResourceStore` di branch (`didApply`, `remove`, `offer`, `evict`).** Terapkan setelah `f13-wiring` merge, dan hapus entri lewat callback `didApply` `.changed` dan `.reloaded`. Saat ini kegagalan refetch ditelan diam-diam (`ResourceStore.swift:296-302`).
8. **Kepemilikan model.** `DaemonModel` memiliki `BackgroundAgentModel` karena butuh presence dan reconnect (0015:301). `ShellRuntime` memiliki `CommandLineToolModel` dan `OnboardingModel` (0015:321, 0016:73). "Hidup di ShellRuntime" di 0016:97 tetap benar secara transitif, karena `ShellRuntime.model` adalah `DaemonModel`. 0016 tidak perlu diubah.
9. **`instance.removed` diusulkan dua kali.** 0020 dan 0022 mengusulkan pemetaan ke `.delete` dengan semantik yang sama (rationale 0020:513 mengutip commit 0022 476204c). Catat sekali saja. Revision event-nya "revision terakhir + 1" (0020:177, :554).

Catatan tambahan:

- Urutan section Diagnostics: 0013:69 dan :323 menaruh hasil doctor F34 "di bawah seksi Connection", jadi doctor ditaruh setelah section agent dan CLI. AC-12 (0013:33) tetap terpenuhi, karena section Connection tidak membuka file dan section CLI hanya menulis symlink setelah konfirmasi.
- Aturan `.patch` statusSeq: spec sudah menyatakannya (0013:29, :248), tetapi kode menolak patch dengan revision yang sama (`ResourceStore.swift:265`), dan branch tidak mengubahnya. F22 M3 tidak punya cadangan dan menunggu langkah 16.

## 4. Butuh keputusan lead foundation

Bagian ini hanya memuat item yang mengubah keputusan yang sedang diimplementasikan oleh build foundation saat ini. Keputusan lain untuk spec yang tidak sedang dibangun ada di bagian 3, di resolusi per spec.

**K1. Semantik create instance: titik tak bisa dibalik, `BusyPolicy`, dan revision.**
- **Build:** F10 langkah 12–13; teks 0007:530.
- **Keputusan saat ini:** create minimal dengan validasi di method, revision wajib, dan `BusyPolicy::Queue`. Kegagalan di `init` meninggalkan instance failed untuk di-purge fitur 54 (0010:367, :454; rationale Keputusan 12).
- **Usulan:** create dikompensasi penuh tanpa titik tak bisa dibalik. Semua langkah reversible, dengan `BusyPolicy::Reject` dan lock tambahan `InstanceName`, dan `provider.revision` opsional (0020:843, :857, :869, :919; tabel resume 0007:420). Kode engine sudah memperlakukan `CreateInstance` sebagai non-resumable dengan Compensate (`crates/domain/src/operation.rs:253-278`).
- **Rekomendasi:** ikuti 0020, yang sudah diterima lead (log :66, :68).
  - F10 langkah 13 membangun create sementara bertanda `// provisional: F20`, tetapi dengan bentuk wire final: `revision: Option` di DTO langkah 12, `Reject` dan `LockKey::InstanceName`. F20 langkah 9 menggantikan handler-nya.
  - Bila kompensasi penuh justru dilipat ke F10 langkah 13, `WriteOp::DeleteInstanceCreatedBy` dan `RegistryError::NotDeletable` (0005) harus dibangun lebih dulu.
  - Perubahan `Instance::new` yang fallible (0004, F20 langkah 1) sebaiknya mendarat sebelum F10 langkah 13. Kalau tidak, F20 langkah 1 ikut memperbaiki call site F10.
  - `StageName::PIN` belum ada.
  - Periksa 0020 AC-11 terhadap urutan kompensasi terbalik, karena `instance_registration` menghapus baris lebih dulu (0020:376).

**K2. Semantik `PortProbe`.**
- **Build:** F10 langkah 5.
- **Keputusan saat ini:** bind percobaan std::net tanpa `SO_REUSEADDR` (0010:249).
- **Usulan:** `PortProbe::try_bind(transport, addr, BindOptions { reuse_addr: true, v6only })` di atas socket2, bind tanpa listen, dengan hasil `Free`, `InUse`, `Denied`, atau `Unavailable` (0020:875, :333).
- **Rekomendasi:** bangun sekali di F10 langkah 5. F20 langkah 6 hanya memakai ulang, dan socket2 masuk lewat update 0001. Di update yang sama tetapkan tiga hal:
  - (a) hasil start untuk `Denied` dan `Unavailable`. 0020:334 hanya memetakan preflight. Minimal, jangan perlakukan sebagai Free;
  - (b) apakah alokasi port otomatis (0010:478) memakai `reuse_addr: true`;
  - (c) apakah pemeriksaan port saat exit dini (0010:259) juga memakainya.

  AC-17 dan catatan test (0010:37, :531, :552) mengikuti. Catat risiko sisa: server tanpa `SO_REUSEADDR` di samping listener wildcard.

**K3. Bentuk hasil probe runner dan reason code.**
- **Build:** F10 langkah 6.
- **Usulan:**
  - `ProbeOutcome::{Ok, Failed { reason: ReasonCode, detail: Option<ObservationDetail> }}`, dengan sukses `probe_ok` dan `connect_failed`, `timeout`, `protocol_error` untuk tcp dan http (0023:363);
  - detail exec berupa baris stderr non-kosong pertama lewat Redactor, dipotong 256 karakter, atau `exit <code>` atau `timeout` (0019:331, :579);
  - `supervisor/probe` sebagai folder (0023:329).
- **Rekomendasi:**
  - Terima semuanya.
  - Pilih `ReasonCode` untuk exit exec non-zero dan untuk status http yang tidak cocok, misalnya memakai ulang `protocol_error` atau menambah satu kode.
  - Pertahankan 0011:285: runner me-redact setiap detail, termasuk probe native (0023:180 diperbaiki pemilik 0023).
  - `ProbeSpec` tetap exhaustive. F23 langkah 1 menambah arm `Native`.

**K4. Field `InstanceSummary` dan penomoran versi protokol.**
- **Build:** F10 langkah 12; sesi Contract 1.4 (F6); milestone wire F7.
- **Usulan:** `ownership: InstanceOwnership` (open enum, default Manual) dan `initialized: bool` (default true), juga di `instance.state_changed` (0022:537); `provider.revision?` (0020:843); opsional DTO `instance.removed` (0020:899).
- **Rekomendasi:**
  - Lipat semuanya ke minor bump F10 langkah 12 sendiri, sehingga 0020 dan 0022 tidak butuh bump tambahan.
  - Lead menetapkan urutan nomor: Contract 1.4, lalu wire F7, lalu F10 langkah 12. Setelah itu kode berikut mengambil minor bebas berikutnya: `registry.not_deletable` (0005), key `reason` `secret.invalid_value` (0011, idealnya ikut `codes::secret` di F11 langkah 5), dan tambahan 0006 (`daemon.shutdown` dan seterusnya).
  - `--amend` hanya selama versi itu belum ada di main.

**K5. Bentuk `install_fake` dan hook `LogSink`.**
- **Build:** F10 langkah 13.
- **Usulan:** pisahkan `supervisor::fixture::pin_package(platform, layout, dir) -> ProviderRevisionRecord`. `install_fake` menjadi salinan runtime ditambah `pin_package`, dan bisa dipakai untuk runtime saja. Hook yang hanya aktif di fixture memasang recording `LogSink` ke `SinkSet` server in-process (0019:533, 0023:208, 0020:599).
- **Rekomendasi:** bangun bentuk terpisah ini di langkah 13 supaya F19 langkah 6 tidak perlu menulis ulang. Pakai method builder untuk `ProviderRevisionRecord` (`with_source_id`), bukan mengubah arity `const fn new` (resolusi 4 di 0005).

**K6. `ChildRole::RuntimeCheck` dan aturan helper.**
- **Build:** F10. `ChildRole` sudah merge di langkah 2 (`crates/platform/src/process.rs:78-89`). Aturan helper ada di 0010:227 dan AC-15.
- **Usulan:** variant `RuntimeCheck` tanpa lock dan dengan output Capture, dipanggil `core::runtime::validate` lewat `spawn_helper`. Timeout langsung memanggil `terminate_helper_group(force: true)` (0018:345, :257).
- **Rekomendasi:**
  - Tambahkan variant-nya. Ini additive, karena tidak ada match exhaustive atas `ChildRole` (`fake_process.rs:634`, :902; `process.rs:221-232`).
  - Tulis aturan timeout `RuntimeCheck` secara eksplisit sebagai kill seketika, karena version check tidak punya state yang perlu di-flush.
  - Longgarkan "Hanya supervisor" (0010:49) dan Security model (0010:517) untuk exec check instalasi.
  - Cadangan bila ditolak: 0018 memakai `ChildRole::Probe`.

**K7. Klasifikasi exit dini dan daftar `FailureCode`.**
- **Build:** F10. Konstanta domain sudah merge di langkah 2 dan dipin test di `crates/domain/src/supervisor.rs:76-118`.
- **Usulan:** `check_launchable` sebelum `spawn_main` (Quarantined menjadi `start_failed { quarantined }`, wire `runtime.quarantined`). `classify_early_exit` memberi `binary_rejected_by_os` tanpa budget. `FailureCode` mendapat `quarantined` dan `binary_rejected_by_os` (0018:345).
- **Rekomendasi:**
  - Evaluasi `classify_early_exit` sebelum port probe, karena SIGKILL yang tidak dikirim daemon adalah putusan OS yang pasti.
  - Tetapkan F18 langkah 8 sebagai pemilik pemasangan ke `core::supervisor`, karena belum ada langkah F18 yang menyebutnya.
  - Konstanta dan test domain diperbarui di commit yang sama.

**K8. Byte non-secret lewat `StdinSource::Bytes(SecretValue)` dan allowlist `expose()`.**
- **Build:** kode spawn F10 (langkah 3 dan 5–8); allowlist check-deps F11.
- **Keadaan:** lead sudah memilih `Bytes` tanpa variant baru (log :58). Menulis byte itu di kode proses platform-macos atau di testkit `fake_process` butuh `expose()` di luar `EXPOSE_ALLOWED` (`source_scan.rs:85-90`). Ini juga berlaku untuk secret stdin biasa. `SecretValue::new` mengembalikan `Result` yang menolak input kosong, NUL, dan lebih dari 64 KiB.
- **Rekomendasi:**
  - Perluas tabel allowlist 0011 (0011:293-301) dan `EXPOSE_ALLOWED` ke modul proses platform-macos dan testkit `fake_process` sebelum kode spawn F10 mendarat.
  - Executor `Stdin::Text` memakai `expect` yang menyebut invariant 0019 AC-3 (text 1–16384 byte, tanpa NUL), atau memetakan kegagalan ke `config_render_failed`.

**K9. Pengecualian codesign untuk check-deps proses.**
- **Build:** F10 langkah 15 (aturan check-deps token SIGKILL hanya di `process/kill.rs`, 0010:512).
- **Usulan:** `MacosCodeSigning` menjalankan `/usr/bin/codesign` lewat helper `posix_spawn` privat di modul `codesign`, di luar `ProcessSupervision`, dengan timeout 120 s (0018:275). Ini bertentangan dengan 0004:610.
- **Rekomendasi:** ubah 0004:610 dengan pengecualian bernama (update 0004 bisa jalan sekarang). Beri tahu penulis F10 langkah 15 bahwa timeout codesign tidak boleh memasukkan token SIGKILL di luar `kill.rs`.

**K10. Kompensator `instance_tree`.**
- **Build:** F7 closeout (branch `claude/f7-daemon-closeout-7fb5d1`, commit 252b5b7 menulis ulang body yang sama).
- **Usulan:** `probe_lock` sebelum menghapus. Hasil Held memberi `compensation_failed` `instance_locked` (0020:815-825).
- **Rekomendasi:**
  - Biarkan closeout merge apa adanya.
  - Urutan final: cek manifest (`NotOwned`, `manifest_unreadable`), lalu `probe_lock`, lalu `remove_owned_tree`.
  - Error dari `probe_lock` menjadi `compensation_failed` tanpa menghapus. Ini celah yang belum diisi 0020.
  - Kodenya dibangun F20 M1 langkah 3.

**K11. `FailureError` `hint`, `items`, `rule`.**
- **Build:** F7 closeout. Keputusannya masih terbuka di Follow-up yang ditambahkan branch itu, dan `failure_body` di branch tidak memasang hint.
- **Rekomendasi:**
  - **Simpan `items**`, dibutuhkan 0020 AC-7 dan 0011 AC-11, sebagai kolom JSON `failure_items` (paling banyak 100 `ErrorItem`) di migration bebas berikutnya. Koordinasikan nomornya dengan migration F18 (`compatibility_records`).
  - **Jangan simpan `hint`**. stacklyd menghitungnya lewat `codes::<area>::hint` dan `with_hint`, dan fallback-nya ditambahkan 0018 langkah 11, bukan F7.
  - **Putuskan `rule`**: disimpan dari `OperationError::rule()` atau diturunkan dari katalog.
  - Sesuaikan AC-16 dan AC-18.

**K12. Lookup idempotency sebelum `begin`.**
- **Build:** F7 langkah 14–15 (helper `start_operation` di stacklyd, di branch closeout).
- **Usulan:** handler boleh mencari key dan fingerprint sebelum kerja sinkron yang mahal, dengan hasil view, none, atau `KeyReused` (0018:92). 0007:428-432 menyatakan lookup hanya ada di dalam `begin`.
- **Rekomendasi:** terima sebagai API baca additive. `begin` tetap otoritatif, sehingga AC-2 dan AC-3 aman. Tidak perlu masuk closeout F7; bisa dibangun di 0018 langkah 11.

**K13. Bentuk field di `put` dan key `reason`.**
- **Build:** F11 langkah 5 (pembuatan pertama `codes::secret`) dan susulan `put`.
- **Usulan:** 0020 memakai struct `SecretFieldRules { min_length, max_length, printable_only }`, yang bertabrakan dengan trait `SecretFieldRules` di `rules.rs:57-64` dan membuang nama field. 0019 menambah `reason: control_characters` di `secret.invalid_value`.
- **Rekomendasi:**
  - Pakai `FieldContext { name, rule: SecretFieldRule }` yang sudah ada, dan tambahkan `printable_only` ke `SecretFieldRule`.
  - Masukkan `reason` ke `codes::secret` saat langkah 5 membuatnya, sehingga tidak perlu bump terpisah.
  - printableOnly tetap opt-in. Default terbalik ditunda.

**K14. Bind di create dan pemetaan kegagalan store.**
- **Build:** use case `put` F11 (sudah merge di langkah 4), susulan di sesi langkah 5.
- **Usulan:** 0020 ingin create tidak memanggil `put` untuk Bind dan membaca nilai sendiri (0020:945-955). Ini bertentangan dengan AC-5 0011, yang menjadikan `put` satu-satunya pintu.
- **Rekomendasi:**
  - `put(Bind)` dengan `field` membaca nilai sekali, menjalankan `Redactor::learn`, memeriksa aturan, dan tidak menulis apa pun. Pemakaiannya lewat `ReplaceSecretUses`.
  - Locked, AccessDenied, dan Busy saat create menjadi `secret.needs_secret` (satu item per reference). OutcomeUnknown, NotSupported, dan Backend tetap `secret.store_*`.

**K15. NUL di `value` create.**
- **Build:** F11. `Validate` sudah memetakan NUL ke `request.invalid_params` lewat `SecretText::field_code` (`crates/protocol/src/secret.rs:84-98`).
- **Usulan:** 0020 AC-2 dan AC-7 ingin NUL dipetakan ke `secret.invalid_value` di precheck.
- **Rekomendasi:** pertahankan 0011 AC-7, dan pemilik 0020 mengeluarkan NUL dari daftar precheck. Alternatifnya: pengecualian create eksplisit di 0011 AC-7.

**K16. Aturan `InstanceStore`.**
- **Build:** F13 M4 langkah 16.
- **Usulan:** limit 200; retained dikecualikan dari snapshot, upsert, dan fetchOne, serta dari count (0022:539); `instance.removed` dipetakan ke `.delete(id, revision)` (0020, 0022); `.patch` statusSeq untuk revision yang sama tetap milik langkah 16, padahal kode saat ini menolaknya (`ResourceStore.swift:265`).
- **Rekomendasi:**
  - Pengecualian retained lewat filter di closure `fetchPage` ditambah `reduce` yang mengembalikan `.delete`, sehingga tidak ada API `ResourceStore` baru yang bertabrakan dengan refactor branch.
  - Count dan badge AC-13 ikut mengecualikan retained (ubah 0013:354).
  - Pemetaan `instance.removed` disiapkan, tetapi baru aktif setelah event ada (F20 M2 langkah 8; dibangun F20 M3 langkah 12).
  - Aturan `.patch` dibangun di langkah 16, karena F22 M3 tidak punya cadangan.

**K17. AC-17 daftar instance sementara "tanpa aksi".**
- **Build:** F13 M4 langkah 16 (`ProvisionalInstanceList`).
- **Usulan:** 0020 memasang tombol New Instance di daftar sementara sebelum F22 (0020:103, :449).
- **Rekomendasi:** ubah AC-17: tombol toolbar dan aksi kedua di empty state hanya dirender bila `newInstanceAction` tidak nil, dengan `.mutationGated()`. Alternatifnya: wajibkan F22 M3 sebelum F20 M3 langkah 14.

**K18. Konten Window: `LiveShell` dan `MainWindowRoot`.**
- **Build:** F13 M4 langkah 15 (branch `f13-wiring`: `LiveShell(runtime:windowWillClose:)`, `ShellBootstrap.assemble`).
- **Usulan:** 0016 memindahkan accessor, `openWindow` capture, `windowWillClose`, dan frame minimal ke `MainWindowRoot`, dan menambah `onboardingStore` ke `ShellBootstrap.live` (0016:344-345).
- **Rekomendasi:**
  - Langkah 15 tidak perlu diubah.
  - Setelah merge, teks 0013 mendeskripsikan `LiveShell`, lalu `MainWindowRoot` membungkus atau menggantikannya sebagai konten Window.
  - `assemble` juga meneruskan `onboardingStore` dan Bool `STACKLY_ROOT`, dan `live()` tetap satu-satunya pintu publik.

## 5. Usulan pembagian kerja

### 5.1 Bisa dijalankan lead scope 02 sekarang

Item ini architect (update) untuk spec yang tidak sedang dibangun. Setiap spec dipegang satu sesi saja.

1. **Spec 0009** (30 usulan). Dikerjakan pertama, karena menjadi prasyarat F17 M1, F18 M1, F19 M1, F20 M1, dan F23 langkah 1. Dua item butuh konfirmasi lead di sesi yang sama:
   - tipe domain `OsVersion` dan `ArtifactFormat` didaratkan lebih awal (resolusi 2);
   - kode issue loader catalog masuk `IssueCode` (resolusi 12).

   Tick Follow-up 697, 698, 699, dan 701 di dalam sesi hanya bila lead mengizinkan.
2. **Spec 0004 dan 0005** (17 + 9 usulan), boleh satu sesi atau berurutan. Termasuk keputusan arsitek tentang aturan `DuplicateSocket`, kebijakan kegagalan `HostFacts`, details `registry.not_deletable`, dan pengecualian codesign 0004:610 (K9). Teksnya bisa sekarang; kodenya mengikuti urutan di 5.4.
3. **Spec 0001** (5 usulan), termasuk keputusan dev dependency CLI ke core (harus ada sebelum 0017 M2 langkah 7) dan Follow-up 0003:611. Harus mendarat sebelum F20 M1, F15 M1 langkah 5, dan F18 M1 langkah 3.
4. **Spec 0003, lalu 0002** (5 + 3 usulan). 0003 memegang urutan `--cleanup` dan semantik lock, dan 0002 menunjuk ke sana. 0003 adalah prasyarat F18 M1 langkah 1.
5. **Spec 0012** (5 usulan, dua penunjuk dokumen). Tidak ditahan apa pun, dan boleh ikut sesi 0013 bila lebih praktis.

### 5.2 Dilipat ke langkah build foundation yang berjalan

Item ini dijalankan oleh program foundation, atau oleh lead scope 02 dengan izin lead foundation.

1. **Spec 0010 → build F10.** Satu architect (update) dokumen-saja sekarang, sebelum F10 M2 langkah 5 mulai. Tidak ada worktree F10 dengan perubahan tertunda.
   - Langkah 5: `PortProbe::try_bind` di atas socket2 (K2).
   - Langkah 6: `ProbeOutcome`, detail exec, reason code, dan `supervisor/probe` sebagai folder (K3).
   - Langkah 12: `ownership`, `initialized`, `revision` opsional, opsional DTO `instance.removed`, dan satu minor bump (K4).
   - Langkah 13: create sementara dengan `Reject` dan `InstanceName` (K1), pemisahan `install_fake`/`pin_package`, dan hook `LogSink` (K5).
   - Langkah 15: aturan check-deps dengan pengecualian codesign (K9).
   - Langkah 2a: tetap mendahului F18 langkah 6 dan F20 M1 langkah 2 (5.4).
   - Dicatat sekarang tetapi dibangun pemilik fase 02: `RuntimeCheck`, `check_launchable`, dan `classify_early_exit` (F18 langkah 8); arm `Stdin::Text` (F19 langkah 5); dispatch native (F23 langkah 5–6); `socketPath`, `instance.removed`, dan create penuh (F20 langkah 7–9).
2. **Spec 0011 → build F11.** Update sebelum F11 langkah 5.
   - Sesi langkah 5 mengambil key `reason` di `codes::secret` (K13), `printable_only` di `SecretFieldRule`, baca-dan-periksa `put(Bind)`, dan dokumentasi pemetaan kegagalan store (K14).
   - Item 7 (`--secret` per field) masuk langkah 8 (`secret_input`).
   - Item 8 (API `SecretField`) masuk langkah 10.
   - Item 9 dan 10 hanya dokumen.
   - Pemasangan `from_setting` untuk printableOnly menunggu update 0009.
   - Keputusan allowlist `expose()` (K8) harus beres sebelum kode spawn F10 mendarat.
3. **Spec 0013 → build F13 M4.** Update setelah `claude/f13-wiring-7b3a78` merge (langkah 15, 17, 18) dan sebelum sesi langkah 16 mulai.
   - Langkah 16 membangun `InstanceStore` dan `ProvisionalInstanceList` dengan limit 200, pengecualian retained, persiapan pemetaan `instance.removed`, `.patch` statusSeq, dan perubahan AC-17 (K16, K17).
   - Item lain additive dan dibangun fitur fase 02 sendiri: banner dan section agent (F15 M3), `MainWindowRoot` dan `onboardingStore` (F16 M1), `ShellNavigator` (konsumen pertama), `itemFailures` dan `CapacityModel` (F22 M3), registrasi `CatalogStore` dan `RuntimeStore` (F17 M3, F18 M4), dan aturan source (F15 M3).
   - F16 M1 secara eksplisit butuh usulan 0013 diterima (`02-instalasi-pertama.md:38`).

### 5.3 Menunggu build mendarat

1. **Spec 0006.** Setelah sesi "Contract 1.4" merge (log :77). Harus mendarat sebelum fungsi `codes::<area>::hint` pertama (0017 M1 langkah 5 atau 0022 M1 langkah 3), dan sebelum 0015 M1 langkah 3–4. Tidak ada yang dilipat ke F7.
2. **Spec 0007.** Setelah `claude/f7-daemon-closeout-7fb5d1` merge. Keputusan K10–K12 sebaiknya dikonfirmasi sekarang supaya closeout tidak perlu diulang. Update harus mendarat sebelum:
   - 0020 M1 (langkah 3 membangun item 2 dan 3);
   - 0018 M3 langkah 11 (item 4);
   - F10 M4 atau F11 langkah 5 mulai mengirim `secret.needs_secret` sebagai kegagalan operasi (item 5, yang mungkin menambah migration).
3. **Spec 0008.** Setelah Build, Verify, dan Test F8 mendarat.
   - Tabel kode cli (15 kode) dan teks lain masuk update 0008.
   - Teks `CLI_SCHEMA_VERSION` 2 masuk satu commit dengan 0015 M1 langkah 6, yang dijalankan berurutan setelah F8 (log :48; 0015:545).
   - Golden perintah fase 02 yang mendarat sebelumnya ikut ditulis ulang di commit itu.

### 5.4 Urutan kode yang harus dijaga (bukan teks spec)

- **Dispatch registry.** `WriteOp`, `FakeRegistry`, `registry_conformance`, dan `SqliteRegistry` dikerjakan oleh F10 langkah 2a dulu, lalu F18 langkah 6 dan F20 M1 langkah 2 satu per satu, tidak berdampingan.
- **Nomor migration setelah 0003.** F18 (`compatibility_records`) dan 0007 item 5 (`failure_items`) berebut nomor berikutnya. Lead menetapkan urutannya. Frozen fixture dan `FIXTURE_SHA256` mengikuti versi yang dipakai F18.
- **`Instance::new` fallible.** Perubahan dari F20 langkah 1 sebaiknya masuk sebelum F10 langkah 13.
- **Minor bump protokol.** Urutannya Contract 1.4, wire F7, F10 langkah 12, lalu sisanya. `--amend` hanya untuk versi yang belum di main.
- **Aturan hint 0006** mendahului fungsi hint pertama.
- **`ResourceStore.itemFailures`** setelah `f13-wiring` merge.
- **`ShellNavigator`** dibangun oleh yang pertama dari F18 M4, F20 M3, atau F22 M3.
- **Update 0001** sebelum F20 M1, F15 M1 langkah 5, dan F18 M1 langkah 3. Keputusan dev dependency CLI sebelum 0017 M2 langkah 7.

### 5.5 Tick Follow-up lewat /sync lead

Menurut log koordinasi :39, tick Follow-up adalah tugas lead di akhir wave. 0018:724 dan 0023:357 meminta sesi update sendiri yang men-tick. Usulan: izinkan untuk 0009 (satu sesi pemilik), dan pakai /sync untuk sisanya.

| Spec | Baris | Syarat |
|---|---|---|
| 0002 | 460 | Setelah 0018 M5 langkah 16 dan 0015 M2 langkah 8 |
| 0003 | 616, 618, 619, 620 | Setelah 0018 M1 langkah 3–5 dan M3 |
| 0004 | 724, 728, 731 | Bisa di update 0004 (penunjuk 0020:627, F18, `provider_rules.rs:1238` dan 0020 AC-3) |
| 0005 | 818 | Bisa di update 0005 (penunjuk spec 0020) |
| 0006 | 669 | Setelah 0015 M1 langkah 6 |
| 0007 | 416 (anotasi, bukan tick) | Di update 0007 |
| 0008 | 522; 523 dan 455 | Setelah 0015 M1 langkah 6; setelah 0020 M2 langkah 10 |
| 0009 | 697, 698, 699, 701 (700 tetap terbuka) | Di update 0009 bila diizinkan, kalau tidak lewat /sync |
| 0010 | 657; 658 | Setelah evidence native F15 M2 langkah 9 (0015 AC-16); setelah update 0010 (0020:623) |
| 0011 | 566 (bagian keputusan); 567 (bagian F20); 568 ditulis ulang | Di update 0011; setelah build F20 |
| 0012 | tidak ada kotak | /sync memastikan tabel `design.md` setelah F17 M3 s8, F18 M4 s15, F22 M3 s8 |
| 0013 | 622; 625 dipecah per fitur | Setelah F15 M3; setelah F17 M3, F18 M4, F22 M3 |