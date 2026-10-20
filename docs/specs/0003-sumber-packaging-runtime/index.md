# 0003. Sumber dan packaging runtime provider

**Date**: 2026-10-17
**Status**: In Progress

## Summary

Spec ini memutuskan dari mana binary keenam provider berasal dan membuktikan bahwa binary itu jalan di Mac tanpa Homebrew. NATS, Traefik, ClickHouse, dan MySQL diunduh langsung dari rilis resmi upstream dengan digest (sidik jari SHA-256) yang dipin; ClickHouse lewat aset `.zip` bertanda tangan Developer ID, karena binary polosnya mengekstrak dirinya sendiri dan ditolak codesign. PostgreSQL 17 dan 18 serta Valkey tetap butuh build maintainer (OQ-003), karena upstream tidak menerbitkan binary macOS yang bisa dipindah. Build itu dijalankan dengan tangan di Mac ini dari resep bash di repo publik terpisah `stackly-dev/runtimes`, lewat `build.sh` yang berjalan dengan environment kosong supaya tidak ada tool atau library Homebrew yang ikut; artefaknya diterbitkan di GitHub Releases repo `stackly-dev/stackly`, hanya setelah spike lokal `PASS` dan setelah kamu menyetujui unggahannya. Semua sumber, versi, lisensi, dependency, dan kebijakan signing dicatat di `runtimes/inventory.toml`, dan setiap artefak dibuktikan oleh test native yang menjalankannya dari LaunchAgent (proses latar yang dijalankan launchd, bukan Terminal), dengan evidence yang mengikuti spec 0002.

## Requirements

**User stories**:

- Sebagai runtime maintainer, saya ingin satu inventaris terverifikasi tentang asal setiap runtime, supaya fitur 18 memasang dari URL dan digest asli dan review distribusi (AC-078) punya bahan yang lengkap.
- Sebagai release owner, saya ingin bukti bahwa setiap executable jalan dari artefak managed sebagai child process dari proses noninteractive di macOS 27, supaya gate G0 bisa ditandatangani.
- Sebagai pengguna Stackly nanti, saya ingin runtime yang jalan dari folder Stackly sendiri tanpa Homebrew global, supaya banyak versi tetap bisa direproduksi.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-078):

- **AC-1**: `runtimes/inventory.toml` berisi tepat delapan artefak, satu per lini: PostgreSQL 17, PostgreSQL 18, MySQL 8.4, MySQL 9.7, NATS 2.15, ClickHouse (lini sesuai aturan fallback), Valkey 9.1, dan Traefik 3.7. Setiap entri `available` dengan semua field wajibnya, atau `blocked` dengan alasan, owner, dan tanggal. `sha256` dan `size` setiap entri `available` berasal dari byte nyata lewat `cargo xtask runtime pin`: unduhan untuk entri upstream, dan file hasil build lewat `pin --file` untuk entri maintainer (digest itu juga cocok dengan file `.sha256` di samping artefak). `cargo xtask runtime verify` mengunduh URL setiap entri dan menghasilkan nilai yang sama; entri maintainer baru di-commit setelah verify itu `OK` (AC-19).
- **AC-2**: `cargo xtask runtime check` lulus pada isi repo, dan gagal dengan pesan `<id artefak>: <aturan>` untuk setiap pelanggaran aturan di *Key invariants*, termasuk artefak `available` yang record native terbarunya untuk `sha256` yang sama bukan `PASS` yang diterima aturan 8. Perintah ini dijalankan dengan tangan di Mac ini.
- **AC-3**: Driver spike, `pin`, dan `verify` mencocokkan `sha256` dan `size` sebelum file diekstrak atau dijalankan, dan worker menghitung ulang hash file cache terhadap plan sebelum ekstraksi. Unduhan atau isi cache yang tidak cocok tidak pernah diekstrak atau dijalankan; cache yang rusak dihapus lalu diunduh ulang; dan hasil spike menjadi `FAIL` dengan alasan `digest mismatch` atau `size mismatch`.
- **AC-4**: Setiap executable provider dijalankan oleh worker yang dijalankan launchd lewat LaunchAgent sementara di domain `gui/<uid>` (parent worker adalah launchd, tanpa TTY pengendali), dan hasil spike mencatat domain itu. Tanpa sesi login GUI, test tercatat `BLOCKED`, bukan `FAIL`. Agent dicabut dan folder spike dihapus di setiap jalur keluar. Agent, proses provider, dan folder sisa run yang terputus dibersihkan oleh run berikutnya atau `cargo xtask runtime spike --cleanup`, yang mengambil lock yang sama dengan driver sehingga tidak pernah mematikan spike yang sedang berjalan. *Ditambah fase 02 (2026-10-20, spec 0015 dan 0018):* satu satunya langkah `--cleanup` yang berjalan tanpa lock adalah pencabutan agent test `dev.stackly.test.agent.*`, dan langkah itu tidak pernah menyentuh proses spike; sweep proses sisa test native (`/stackly-test-`) berjalan di dalam lock. Worker tidak pernah melewati batas waktunya, dan dua spike tidak pernah berjalan bersamaan di satu Mac.
- **AC-5**: Untuk setiap artefak `available`, spike mengekstrak ke folder dengan path yang berisi spasi dan karakter non ASCII, menyiapkan data bila provider membutuhkannya, menjalankan server hanya di port loopback yang dialokasikan atau socket privat (listener lain membuat `FAIL`), lulus cek readiness provider dalam 120 detik, dan berhenti sendiri dalam 60 detik setelah perintah stop tanpa SIGKILL dari worker (kode keluar atau sinyal dicatat). Versi yang dibaca dari binary yang berjalan, setelah dinormalisasi per provider, sama dengan `version` di inventaris. PostgreSQL 17 dan PostgreSQL 18 keduanya `PASS`.
- **AC-6**: Audit statis (`otool -L` dan `LC_RPATH` di setiap file Mach-O dalam artefak, wajib punya slice arm64) dan audit runtime (entri `txt` Mach-O dari `lsof` pada proses server yang berjalan, path dibuat canonical) tidak menemukan library di luar allowlist sistem (`/usr/lib/`, `/System/Library/`, `/System/Volumes/Preboot/Cryptexes/OS/System/Library/`, `/System/Cryptexes/OS/System/Library/`) dan folder artefak itu sendiri. Path lain apa pun membuat `FAIL` dengan path itu tercatat. `LC_RPATH` absolut di luar sistem membuat `FAIL` untuk artefak maintainer; untuk artefak upstream hanya dicatat sebagai peringatan, kecuali audit runtime menunjukkan library benar benar dimuat lewat path itu. Proses provider mendapat environment yang dikosongkan: `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, `HOME` dan `TMPDIR` baru di folder run, `LANG=en_US.UTF-8`, tanpa variabel `DYLD_*` atau Homebrew.
- **AC-7**: Artefak PostgreSQL lulus probe closure: database `postgres` memakai locale provider ICU (`datlocprovider = 'i'`), `length(gen_random_bytes(16)) = 16` setelah `CREATE EXTENSION pgcrypto` (OpenSSL), nilai `repeat('a', 10000)` di kolom `COMPRESSION lz4` menghasilkan `pg_column_compression = 'lz4'`, dan `pg_dump --compress=zstd -f <file>` berhasil. Untuk setiap artefak, gabungan dependency `bundled` dan `own` di inventaris sama persis dengan library non sistem yang ditemukan audit statis.
- **AC-8**: Hasil spike mencatat signature yang teramati pada setiap executable utama (Developer ID dengan authority, ad hoc, atau linker signed) dan apakah tanda tangan ad hoc saat install diperlukan, sesuai `signing_policy` entri. Tanda tangan ulang selalu mencakup semua file Mach-O di salinan, tidak pernah sebagian. Tidak ada langkah yang menghapus quarantine, mengubah setelan Gatekeeper atau sistem, atau memakai `sudo`.
- **AC-9**: Untuk setiap `sha256`, sekali saja (saat file yang dilacak git di `evidence/runtime/` belum memuat hasil quarantine untuk digest itu), clone APFS artefak yang setiap filenya diberi atribut `com.apple.quarantine` dijalankan dari konteks LaunchAgent yang sama, dan hasilnya (`ran`, `blocked`, `killed`, atau `timeout`) tercatat. Run lain mencatat `skipped` beserta folder evidence yang sudah memuat hasilnya. Spike lokal dengan `--no-quarantine` (dipakai sebelum unggah, AC-19) mencatat `skipped` dengan detail `local run`; test native tidak pernah memakai flag itu. Salinan asli tanpa quarantine tidak tersentuh.
- **AC-10**: `xtask/tests/native_runtime.rs` punya satu test native per lini provider yang menulis record `stackly.evidence/v1` (spec 0002) dengan `covers = ["AC-078", "REQ-005"]`, `kind` native, subject berisi versi teramati (atau versi inventaris untuk `FAIL` sebelum versi terbaca), `sha256`, dan `source: managed`. Record `PASS` membawa `actual` berformat `<provider> <observedVersion> ready in <ms> ms, stopped in <ms> ms`; record `FAIL` berasal dari panic dengan pesan `<langkah>: <alasan>`; entri `blocked`, entri yang belum ada, dan Mac tanpa sesi GUI menjadi `BLOCKED` dengan alasan dan owner. Setiap test memanggil `artifact_dir()` dan menyimpan hasil spike lengkap di sana. `cargo xtask runtime record <report.json>` menyalin record `PASS` dan `FAIL` test itu dari run native yang bersih ke `evidence/runtime/<report.runId>/` (record `BLOCKED` tidak disalin, karena alasan dan owner-nya sudah ada di inventaris, dan record `NOT_RUN` juga tidak), dan menolak run dirty, run fixture, report dengan `problems` tidak kosong atau `interrupted: true`, report yang `context.sourceCommit` bukan `HEAD` saat perintah dijalankan, record yang `evidencePath` tidak berada di bawah `artifacts/` atau foldernya tidak memuat `spike-result.json`, report tanpa record yang bisa disimpan (`no record to keep`), serta folder tujuan yang sudah ada.
- **AC-11**: _Dihapus 2026-10-17: CI ditiadakan, pengembangan fokus di Mac ini dulu._ Cara membangun PostgreSQL dan Valkey di Mac ini diputuskan 2026-10-18 di AC-17 sampai AC-19.
- **AC-12**: Artefak buatan maintainer hanya berisi file biasa dan folder (tanpa symlink, setuid, setgid, atau file device), folder `LICENSES/` dengan teks lisensi software itu dan setiap dependency bundel (`LICENSES/<nama>.txt`, dengan `LICENSES/<provider>.txt` berisi teks lisensi utama provider lalu bagian `Third party notices` untuk file source dari pemegang hak cipta lain, lihat Notice pihak ketiga), serta `STACKLY-ARTIFACT.json` yang mencatat source, rujukan resep (`recipe`), toolchain, deployment target, dan versi dependency, tanpa path home pembuatnya.
- **AC-13**: Lini yang tidak punya artefak managed (misalnya rilis tanpa aset macOS arm64, atau `FAIL` yang tidak bisa diperbaiki) dicatat `blocked` dengan alasan, owner, dan tanggal. Untuk ClickHouse, aturan fallback dijalankan dulu: rilis LTS terbaru yang membawa aset macOS aarch64, lalu stable terbaru; baru `blocked` kalau keduanya tidak ada. Aset ClickHouse yang dipakai adalah `clickhouse-macos-aarch64.zip`; aset polos `clickhouse-macos-aarch64` (binary self extracting yang gagal `codesign --verify --strict` dan menulis ulang dirinya saat jalan) tidak dihitung sebagai artefak managed.
- **AC-14**: _Dihapus 2026-10-17: CI ditiadakan, pengembangan fokus di Mac ini dulu._
- **AC-15**: _Dihapus 2026-10-17: CI ditiadakan, pengembangan fokus di Mac ini dulu._
- **AC-16**: Artefak `format = "zip"` diekstrak dengan `/usr/bin/tar` setelah gerbang digest AC-3, dengan aturan yang sama dengan `tar_gz`: worker lebih dulu mencocokkan magic number file cache dengan `format` (tidak cocok berarti `FAIL` di langkah `extract` dengan alasan `format <format> does not match the file (<jenis>)`, dengan `<jenis>` salah satu dari `gzip`, `zip`, `mach-o`, atau `unknown`, sebelum `tar` dijalankan; cek yang sama berlaku untuk `tar_gz` dan `binary`, dan `cargo xtask runtime verify` mencetak `FAIL` dengan alasan yang sama); anggota dengan komponen `..` atau anggota yang tertulis lewat symlink membuat `tar` keluar dengan kode bukan 0 dan spike `FAIL` di langkah `extract`; tidak ada file yang tertulis di luar folder ekstraksi; metadata AppleDouble (`__MACOSX/`) tidak menjadi file maupun atribut tambahan (xattr); aturan `archive_root` dan laporan archive berlaku sama. Setelah ekstraksi `zip`, setiap path di `executables` harus file biasa di dalam root artefak tanpa symlink di path-nya (path yang tidak ada gagal dengan `the artifact has no executable <path>`, selain itu `FAIL` di langkah `extract` dengan `executable <path> is not a regular file`), lalu diberi mode tepat 0755, sama seperti format `binary`; file lain dan semua file `tar_gz` tetap memakai mode hasil `tar`, dan setiap file yang modenya berubah dicatat di detail langkah `extract` sebagai `set mode 0755 on <path> (was <mode di disk setelah tar, oktal 4 digit>)`. `cargo xtask runtime pin` mencetak `format` dari magic number unduhan dan membaca `min_os` dari `--executable` di dalam `.zip`.
- **AC-17**: Build maintainer dijalankan dengan tangan di Mac ini lewat `./build.sh <provider> <version> <build>` di checkout bersih `stackly-dev/runtimes`. Sebelum mengunduh apa pun, skrip menolak tree yang kotor (`git status --porcelain` tidak kosong) dengan pesan `stackly-runtimes has uncommitted changes`, dan menolak mulai kalau `/usr/local/include` atau `/usr/local/lib` memuat header atau library yang bisa menimpa milik build (daftar di Alur `build.sh`). Lalu ia menjalankan ulang dirinya lewat `/usr/bin/env -i`, sehingga setiap proses build hanya melihat variabel di tabel Environment build (tanpa `/opt/homebrew` atau `/usr/local` di `PATH`, tanpa `DYLD_*`, `CPATH`, `LIBRARY_PATH`, atau `PKG_CONFIG_PATH`). Setiap tarball source dicocokkan dengan sha256 yang dipin di resep sebelum diekstrak. Build gagal kalau `/opt/homebrew` muncul di log konfigurasi (`meson-logs/meson-log.txt` atau `config.log`), kalau path home pembuatnya muncul di file mana pun di dalam paket, kalau audit bash menemukan referensi di luar allowlist maintainer (AC-6), atau kalau salah satu executable gagal mencetak versinya dari salinan paket di path berisi spasi dan `Ü`. `<id>.tar.gz` dan `<id>.tar.gz.sha256` hanya ditulis kalau semua cek itu lulus.
- **AC-18**: Untuk artefak `origin = "maintainer"`, langkah `closure` spike juga menjalankan probe `manifest`: `STACKLY-ARTIFACT.json` ada di root artefak dengan `schema` `stackly.runtime-artifact/v1`; `provider`, `version`, `build`, `source.url`, `source.sha256`, dan `recipe` sama dengan entri inventaris; `deploymentTarget` sama dengan `min_os` entri; gabungan `libraries` dari setiap dependency manifest sama persis dengan `name` baris `bundled` entri, dan setiap baris `bundled` punya `version`, `license`, dan `source_url` yang sama dengan dependency manifest pemilik file itu; `LICENSES/<provider>.txt`, `LICENSES/<nama>.txt` untuk setiap dependency, dan `LICENSES/<nama>.txt` untuk setiap komponen `vendored` di manifest ada; dan laporan archive mencatat nol symlink, nol setuid atau setgid, dan nol file khusus. Satu saja yang tidak cocok membuat probe `manifest` `FAIL` dengan nama field atau file itu di `detail`, dan spike `FAIL` di langkah `closure`. Probe `dependencies` dan `manifest` selalu sama sama dicatat sebelum langkah itu gagal. Artefak upstream tidak menjalankan probe ini.
- **AC-19**: `cargo xtask runtime pin --file <tar.gz> --url <https>... [--executable <path>]` tidak menghubungi jaringan: ia menghitung digest file lokal, mencocokkannya dengan `<tar.gz>.sha256` (nama file di dalamnya harus sama), memastikan nama file terakhir setiap `--url` sama dengan nama file lokal (`gh` mengunggah dengan nama file itu), menyalin (bukan memindah) file itu ke cache dengan nama digest-nya, lalu mencetak potongan TOML yang sama dengan `pin` biasa dengan `urls` dari `--url`. File `.sha256` yang hilang atau tidak cocok, atau nama URL yang berbeda, membuat perintah gagal tanpa menulis cache. Untuk setiap artefak maintainer, urutannya: spike lokal `PASS` dari cache itu (`runtime spike --no-quarantine`), lalu persetujuan lisensi release owner untuk provider itu tercatat di spec ini (sekali per provider, sebelum unggahan pertama provider itu), lalu commit `recipe` sudah ada di remote `stackly-runtimes` (`git branch -r --contains <commit>` tidak kosong), lalu persetujuan eksplisit kamu untuk unggahan itu, lalu unggah ke GitHub Releases sebagai prerelease (tidak pernah berlabel Latest), lalu `cargo xtask runtime verify --id <id>` `OK`, baru entri di-commit dan evidence direkam lewat alur dua commit.

## Decision

**Chosen option**: Option 1: Sumber campuran, upstream resmi bila ada dan build maintainer untuk sisanya, dengan inventaris yang dicek mesin dan bukti dari test native.

Binary diunduh langsung dari upstream dengan digest yang dipin kalau upstream menerbitkan build macOS arm64 resmi (MySQL, NATS, ClickHouse, Traefik); PostgreSQL dan Valkey dibangun maintainer. OQ-003 terjawab: artefak buatan maintainer diperlukan, untuk PostgreSQL dan Valkey.

Cara membangunnya di Mac ini (diputuskan 2026-10-18): resep bash di repo publik terpisah `stackly-dev/runtimes`, dijalankan dengan tangan lewat `build.sh` yang menjalankan ulang dirinya dengan `env -i`; meson, ninja, dan pkgconf dari venv `/usr/bin/python3` 3.9.6 dengan wheel yang dipin hash; bison, flex, perl, dan make dari `/usr/bin`; OpenSSL, ICU, lz4, dan zstd dibangun dari source; deployment target 27.0. Artefak diterbitkan di GitHub Releases repo `stackly-dev/stackly` dengan tag `runtime/<id>`, setelah `runtime pin --file` mengisi cache, spike lokal `PASS`, gerbang lisensi, dan persetujuan unggahan dari kamu. Field `attestation` dihapus; asal build dicek lewat probe `manifest` terhadap `STACKLY-ARTIFACT.json`. Runner up per keputusan ada di rationale (Update 2026-10-18).

Format artefak upstream yang diterima ada tiga: `tar_gz`, `zip`, dan `binary`. `zip` ditambahkan 2026-10-17 untuk ClickHouse, dan diekstrak dengan `/usr/bin/tar` yang sama (bsdtar, yang juga membaca zip), bukan `ditto` atau `unzip`. Ketiganya menolak penulisan lewat symlink, tapi hanya bsdtar yang gagal dengan jelas pada anggota `..` (dua lainnya diam diam menulis ulang path-nya), dan hanya bsdtar yang sudah dipakai `pin` untuk mendaftar dan mengekstrak satu anggota, sehingga tidak perlu alat kedua. Runner up untuk keputusan ini: tetap `blocked` sampai fitur 18. Karena zip ClickHouse menyimpan `clickhouse` tanpa bit eksekusi, spike memberi mode 0755 pada path `executables` artefak `zip` (diputuskan 2026-10-18); runner up: field opt in per entri di inventaris.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `meson` (`mohitmishra786/low-level-dev-skills`, `.claude/skills/meson/`) · `signing-entitlements` (`openai/plugins`, `.claude/skills/signing-entitlements/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya skill meson menganjurkan wrap untuk dependency, sedangkan resep di sini membangun dependency sendiri dengan versi dan checksum yang dipin.

## Feature design

### Sumber runtime per provider

Versi adalah patch terbaru per 17 Oktober 2026 dari cek lanskap; nilai yang mengikat adalah yang tercatat di inventaris saat entri ditulis (patch terbaru di lini itu pada hari itu).

| Lini | Versi saat ini | Origin | Distribusi | Format | Checksum upstream | Signing policy | Lisensi (SPDX, dicek ulang dari file lisensi artefak) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PostgreSQL 17 | 17.11, build r2 | maintainer | `stackly_hosted` (GitHub Releases `stackly-dev/stackly`, tag `runtime/<id>`) | `tar_gz` | `.sha256` source dari postgresql.org | `adhoc_pipeline` | `PostgreSQL` |
| PostgreSQL 18 | 18.6, build r2 | maintainer | `stackly_hosted` | `tar_gz` | sama | `adhoc_pipeline` | `PostgreSQL` |
| MySQL 8.4 LTS | 8.4.11 | upstream | `upstream_download` | `tar_gz` (nama file macOS ARM64 dikonfirmasi saat pin) | `none` (tidak ada file checksum polos); GPG bila ada URL `.asc` mentah | `keep_or_adhoc` | `GPL-2.0-only WITH Universal-FOSS-exception-1.0` |
| MySQL 9.7 LTS | 9.7.2 | upstream | `upstream_download` | `tar_gz` | sama | `keep_or_adhoc` | sama |
| NATS 2.15 | 2.15.1 | upstream | `upstream_download` | `tar_gz` | `SHA256SUMS` | `keep_or_adhoc` | `Apache-2.0` |
| ClickHouse | LTS terbaru dengan aset macOS aarch64: 26.8.29.10 (`v26.8.29.10-lts`); fallback stable terbaru (26.9) | upstream | `upstream_download` | `zip`: aset `clickhouse-macos-aarch64.zip`, satu file `clickhouse` tanpa folder puncak, binary biasa yang tidak mengekstrak dirinya sendiri, disimpan bermode 0664 sehingga spike memberinya mode 0755 (lihat Format artefak). Aset polos `clickhouse-macos-aarch64` tidak dipakai (binary self extracting) | tidak ada untuk aset macOS (`none`) | `keep_or_adhoc` (binary di dalam zip bertanda tangan Developer ID, team `ZNDB5FJ8ZW`, hardened runtime, jadi signature dipertahankan) | `Apache-2.0` |
| Valkey 9.1 | 9.1.2, build r2 | maintainer | `stackly_hosted` | `tar_gz` | daftar `valkey-hashes` bila memuat versi itu, selain itu `none` | `adhoc_pipeline` | `BSD-3-Clause` |
| Traefik 3.7 | 3.7.15 | upstream | `upstream_download` | `tar_gz` (tanpa folder puncak) | `checksums.txt` | `keep_or_adhoc` | `MIT` |

Closure dependency:

- **PostgreSQL**: bundel OpenSSL lini 3.5 LTS (3.5.9 per 18 Oktober 2026), ICU rilis stabil terbaru (78.3; data di dalam `libicudata`), lz4 (1.10.0), dan zstd (1.5.7) sebagai dylib di `lib/` dengan install name `@rpath`; dari sistem macOS: `libz`, `libxml2`, `libedit`, fungsi uuid di `libSystem`. Library buatan source PostgreSQL sendiri (misalnya `libpq`, `libecpg`) dicatat sebagai `own`. Semua modul contrib ikut. Lisensi bundel: OpenSSL `Apache-2.0`, ICU `Unicode-3.0`, lz4 `BSD-2-Clause` (hanya library), zstd `BSD-3-Clause` (dipilih dari lisensi ganda BSD atau GPL-2.0). Di luar build: GSSAPI, LDAP, PAM, Bonjour, NLS, JIT LLVM, PL/Perl, PL/Python, PL/Tcl, OAuth lewat libcurl (PostgreSQL 18), extension pihak ketiga, dan cipher lama pgcrypto (bf, cast5, des), karena provider legacy OpenSSL tidak diaktifkan dan artefak tidak membawa `openssl.cnf`.
- **Valkey** (tambahan): kode vendored di tarball source (misalnya Lua, libvalkey, linenoise, hdr_histogram, fpconv) tertaut statis; namanya dan lisensinya dicatat di `vendored` manifest dan teksnya di `LICENSES/`, sementara `license` entri tetap lisensi Valkey sendiri. Notice pihak ketiga di `src/` (misalnya lzf, crc64, crcspeed, geohash) masuk bagian `Third party notices` di `LICENSES/valkey.txt`.
- **Valkey**: `BUILD_TLS=no`, hanya library sistem.
- **NATS, Traefik**: binary Go, hanya library sistem (dikonfirmasi audit).
- **MySQL, ClickHouse**: library dari source upstream itu sendiri dicatat `own`; library pihak ketiga yang dibawa upstream di dalam artefak (misalnya OpenSSL di tarball MySQL) dicatat `bundled` dengan versi dan lisensinya, satu entri per file dylib.

Signing policy:

- `keep_or_adhoc`: setiap file Mach-O di salinan hasil ekstraksi dicek dengan `codesign --verify --strict`. Kalau semuanya lulus, signature upstream dipakai apa adanya. Kalau satu saja tidak ada atau rusak, semua file Mach-O di salinan itu ditandatangani ulang dengan `codesign --force --sign -` tanpa `--preserve-metadata` (hardened runtime dan library validation ikut lepas bersamaan, jadi binary tidak menolak library miliknya sendiri), dan hasil spike mencatat `adhocSignedAtInstall: true`. File di cache tidak pernah diubah.
- `adhoc_pipeline`: build maintainer menandatangani ad hoc setiap file Mach-O setelah `install_name_tool`, tanpa hardened runtime (supaya PostgreSQL bisa memuat extension). Developer ID dan notarization menyusul di fitur 60 setelah identity OQ-012 tersedia.

### Format artefak

`format` menentukan cara file cache menjadi root artefak. Ketiga format lewat gerbang digest AC-3 dulu, lalu cek format: satu fungsi bersama `check_format(format, kind)` di `xtask/src/runtime/macho.rs` mencocokkan `format` dengan jenis file dari `macho::file_kind`, dan gagal dengan `format <format> does not match the file (<jenis>)`. Jenis file punya nama tetap: `gzip`, `zip`, `mach-o`, dan `unknown`. Fungsi ini dipanggil sebagai langkah pertama `archive::extract` (sehingga juga berlaku untuk `tar_gz` dan `binary`), dan oleh `cargo xtask runtime verify` setelah digest dan ukuran cocok. Test `binary` yang ada memakai byte header Mach-O sebagai isi fixture, bukan teks.

| Format | Magic number | Ekstraksi spike | `min_os` saat `pin` (macOS saja) |
| --- | --- | --- | --- |
| `tar_gz` | gzip `1f 8b` | `/usr/bin/tar -xzf <cache> -C <folder>` | anggota `--executable` dicari lewat `/usr/bin/tar -tzf`, lalu diekstrak sendiri dengan `/usr/bin/tar -xzf <cache> -C <folder> -- <anggota>` |
| `zip` | `50 4b 03 04` | `/usr/bin/tar -xf <cache> --no-mac-metadata -C <folder>`, lalu mode 0755 untuk setiap path di `executables` | anggota `--executable` dicari lewat `/usr/bin/tar -tf`, lalu diekstrak sendiri dengan `/usr/bin/tar -xf <cache> --no-mac-metadata -C <folder> -- <anggota>` |
| `binary` | Mach-O tipis atau universal | salin ke `<folder>/<executables[0]>`, mode 0755 | file itu sendiri |

Perintah `tar` untuk `tar_gz` sama dengan milestone 2; yang baru untuk ketiga format hanya cek format sebelumnya. Bukti milestone 2 tetap berlaku tanpa run ulang, karena aturan 8 terikat pada `sha256`, dan keempat file yang sudah dipin berupa gzip sehingga lulus cek itu.

Aturan untuk `tar_gz` dan `zip`:

- `tar` tidak pernah diberi `-P`. Tanpa `-P`, bsdtar menolak anggota yang path-nya memuat `..` dan anggota yang akan tertulis lewat symlink (keluar dengan kode 1), dan membuang `/` di depan path absolut sehingga anggota itu tetap tertulis di dalam folder ekstraksi. Kode keluar bukan 0 berarti `FAIL` di langkah `extract`, dengan stderr `tar` di alasannya.
- `--no-mac-metadata` wajib untuk `zip`. Tanpa flag itu, bsdtar di macOS mencoba memulihkan isi `__MACOSX/._<nama>` sebagai xattr (walau man page menyebut sebaliknya; di percobaan, isi yang bukan AppleDouble sah membuatnya gagal dengan `Failed to restore metadata`); dengan flag itu, entri `__MACOSX/._<nama>` dibuang dan tidak menjadi file ataupun xattr. Bukti percobaannya ada di rationale. File lain di bawah `__MACOSX/` yang tidak berpasangan belum dicoba: dengan `archive_root` ia membuat cek puncak gagal, dan tanpa `archive_root` ia muncul di `archive.files`, yang untuk ClickHouse dicek harus 1 (lihat Critical test scenarios).
- Laporan archive (symlink internal dan yang keluar, setuid, file device) dan aturan `archive_root` sama untuk keduanya. Mode file diambil dari archive, dengan satu pengecualian untuk `zip` (bullet berikut); untuk `tar_gz` spike tidak pernah menambah bit eksekusi.
- Untuk `zip`, setiap path di `executables` diberi mode tepat 0755 setelah ekstraksi, karena zip upstream bisa menyimpan executable tanpa bit eksekusi: ClickHouse menyimpan `clickhouse` bermode 0664 di setiap rilis 26.7, 26.8, dan 26.9 yang dicek pada 2026-10-18. Aturannya, di dalam `archive::extract` sesudah `tar` dan sebelum langkah lain:
  - Komponen path relatif yang tertulis di `executables` dicek satu per satu dari kiri dengan `symlink_metadata`, hanya di bawah root artefak (root dan folder di atasnya tidak dicek, karena `TMPDIR` macOS sendiri lewat symlink `/var`), karena `chmod` mengikuti symlink. Komponen yang tidak ada (ENOENT atau ENOTDIR) membuat `FAIL` dengan `the artifact has no executable <path>`; komponen yang berupa symlink (termasuk symlink yang targetnya tidak ada di ujung path) atau ujung yang bukan file biasa membuat `FAIL` dengan `executable <path> is not a regular file`. `<path>` selalu teks persis seperti di `executables`.
  - Mode lama adalah mode di disk setelah `tar` (`mode & 0o7777`). bsdtar memakai umask proses tanpa `-p`, jadi di bawah launchd dengan umask 022 `clickhouse` yang disimpan 0664 menjadi 0644 lebih dulu. File yang sudah 0755 tidak di-`chmod` dan tidak dicatat; file lain menjadi tepat 0755, yang juga membuang bit setuid dan setgid (bukti tetap ada di catatan `was`).
  - Detail langkah `extract` berisi satu catatan per file yang berubah, urut sesuai `executables`, digabung dengan `; `, misalnya `set mode 0755 on clickhouse (was 0644)`. Kalau tidak ada yang berubah, detail tetap kosong.
  - Sengaja lebih ketat dari `tar_gz`: executable `zip` yang berupa symlink (misalnya link multicall) membuat `FAIL`. `runtime check` tidak bisa menangkapnya saat review, karena baru terlihat setelah ekstraksi.
  - File lain tidak disentuh. Signature tidak terpengaruh, karena mode file bukan bagian code signature (dicek: `codesign --verify --strict` lulus sebelum dan sesudah `chmod 0755`). Clone quarantine (`cp -Rc`) dan tanda tangan ulang ad hoc berjalan sesudah `chmod` dan mempertahankan mode 0755 (dicek pada 2026-10-18).
- Di Linux, `pin` tidak pernah mengekstrak (GNU tar tidak membaca zip), karena `min_os` hanya dibaca di macOS; `format` tetap dicetak dari magic number.

Perubahan `pin`:

- Struct `Pinned` mendapat `format` sebagai field pertama, sebelum `urls`, sesuai urutan di entri inventaris (`format`, `urls`, `sha256`, `size`), lalu `min_os` tetap terakhir. Test teks persis `pinned_values_print_as_inventory_toml` ikut berubah.
- `pin` mengklasifikasi file lewat `macho::file_kind` sebelum cek macOS di `read_min_os`, sehingga `format` dicetak di semua OS dan hanya `min_os` yang memerlukan macOS.
- `FileKind` mendapat varian `Zip` untuk `50 4b 03 04`. Test `macho` yang sekarang menyatakan `PK\x03\x04` sebagai `Other` dibalik menjadi `Zip`.
- *(Ditambah fase 02, spec 0018.)* `pin` juga mencetak `unpacked_size` dan `entry_count` dari `scan`, di semua OS, setelah `size` dan sebelum `min_os` (yang tetap terakhir).
- Untuk jenis `unknown`, `pin` tidak mencetak `format`, `min_os`, `unpacked_size`, maupun `entry_count`, dengan catatan `pin: the download is not a Mach-O file, a .tar.gz, or a .zip; format and min_os left out`.
- `extract_one` menerima flag `tar` per jenis (`-tzf` dan `-xzf` untuk gzip, `-tf` dan `-xf --no-mac-metadata` untuk zip), sehingga pencarian anggota dan ekstraksi satu anggota berlaku untuk keduanya.

Saat ini tidak ada entri yang memakai `binary`. Format itu tetap diterima untuk aset satu file yang lulus `codesign`; binary self extracting seperti aset polos ClickHouse tertangkap di langkah signing (`codesign --verify --strict` dan `codesign --force --sign -` sama sama gagal), sehingga spike-nya `FAIL`.

### Data model sketch

**`runtimes/inventory.toml`** (field TOML snake_case; tipe Rust tooling di `xtask/src/runtime/inventory.rs`. Inventory tetap file tooling: [spec 0009](../0009-provider-engine-manifest-schema/index.md) memutuskan bahwa catalog tidak pernah membacanya, melainkan membaca `providers/<software>/versions.json` yang ditulis `cargo xtask codegen` dari inventory ini, dan langkah 9 build plan spec 0009 menghapus penanda `// provisional: F9` di file itu):

| Entitas | Kunci | Field | Relasi |
| --- | --- | --- | --- |
| Inventory (root file) | `schema_version` | `schema_version` (int, wajib, nilainya `1`) | 1:N Artifact |
| Artifact (`[[artifact]]`) | `id` unik | lihat tabel field di bawah | N:1 Inventory, 1:N Dependency, 1:N record evidence (lewat `sha256`) |
| Dependency (`[[artifact.dependency]]`) | (`artifact.id`, `name`) unik | `name` (wajib), `kind` (`bundled`, `own`, atau `system`, wajib), `install_name` (wajib, persis seperti di `otool -L`); khusus `bundled`: `version`, `license` (SPDX), dan `source_url` (https) wajib | N:1 Artifact |

Arti `kind` dependency: `bundled` adalah library pihak ketiga yang ikut di artefak, satu entri per file dylib (OpenSSL menjadi `libssl.3.dylib` dan `libcrypto.3.dylib`, ICU tiga entri); `own` adalah library yang dibangun dari source provider itu sendiri dan tercakup lisensi artefak; `system` adalah library macOS.

| Field Artifact | Tipe | Wajib | Catatan |
| --- | --- | --- | --- |
| `id` | string | ya | upstream `<provider>-<version>-macos-aarch64`; maintainer `<provider>-<version>-r<build>-macos-aarch64` |
| `provider` | enum | ya | `postgresql`, `mysql`, `nats`, `clickhouse`, `valkey`, `traefik` |
| `line` | string | ya | `17`, `8.4`, `2.15`, `26.8`, dan seterusnya; `version` diawali `<line>.` |
| `version` | string | ya | exact, pola sama dengan subject spec 0002: `^[0-9]+(\.[0-9]+)+([+-][0-9A-Za-z.]+)?$`; ClickHouse memakai empat komponen penuh |
| `build` | int | maintainer saja | mulai 1, naik setiap build ulang versi yang sama |
| `os`, `arch` | enum | ya | `macos`, `aarch64` |
| `min_os` | string | `available` | versi minimum dari `LC_BUILD_VERSION` executable utama |
| `status` | enum | ya | `available` atau `blocked` |
| `blocked_reason`, `blocked_owner`, `blocked_since` | string, string `@handle`, tanggal | `blocked` | owner default `@ojiepermana` (runtime maintainer) |
| `origin` | enum | ya | `upstream` atau `maintainer` |
| `distribution` | enum | ya | `upstream_download` (origin upstream) atau `stackly_hosted` (origin maintainer) |
| `format` | enum | `available` | `tar_gz`, `zip`, atau `binary` (maintainer selalu `tar_gz`); nilainya dicetak `runtime pin` dari magic number unduhan, lihat Format artefak |
| `urls` | array string https | `available` | URL kandidat berurutan; digest yang menentukan identitas |
| `sha256`, `size` | hex 64 huruf kecil, int byte | `available` | dari unduhan nyata |
| `archive_root` | string | tidak | folder puncak di dalam archive `tar_gz` atau `zip` (aturan 3: tidak boleh untuk `binary`, dan harus satu nama folder biasa). Kalau ada, puncak archive harus tepat folder itu. Kalau tidak ada (misalnya Traefik dan ClickHouse), archive diekstrak ke folder baru `<id>/` yang menjadi root artefak |
| `upstream_checksum` | `{ kind, url }` | origin upstream | `kind`: `sha256sums`, `checksums_txt`, `sha512`, atau `none` |
| `upstream_signature` | `{ kind, url, key, fingerprint }` | origin upstream | `kind`: `gpg` atau `none`; untuk `gpg`, `key` adalah file kunci publik yang disimpan di repo di `runtimes/keys/` dan `fingerprint` sidik jarinya |
| `source_url`, `source_sha256` | https, hex | origin maintainer | tarball source upstream |
| `recipe` | string | origin maintainer | rujukan resep build berbentuk `<owner>/<repo>@<commit 40 hex huruf kecil>:<path relatif>`, misalnya `stackly-dev/runtimes@<commit>:recipes/postgresql.sh`; disalin dari `STACKLY-ARTIFACT.json` dan dicek ulang probe `manifest` (AC-18). Field `attestation` dihapus 2026-10-18; entri yang masih membawanya membuat `runtime check` gagal sebagai TOML tidak valid (field tidak dikenal), bukan sebagai baris aturan |
| `signing_policy` | enum | `available` | `keep_or_adhoc` (upstream) atau `adhoc_pipeline` (maintainer) |
| `license`, `license_url` | SPDX, https | `available` | |
| `executables` | array path relatif | `available` | path di dalam root artefak yang dijalankan spike; executable utama di urutan pertama |
| `stable` | bool | tidak | *(ditambah fase 02, 2026-10-20, spec 0018)* paling banyak satu per `(provider, os, arch)`, dan hanya pada `available` (aturan 10). Diisi runtime maintainer saat update inventaris. Rekomendasi: line terbaru yang punya evidence native (PostgreSQL 18, MySQL 9.7), dan satu satunya line untuk provider lain |
| `unpacked_size`, `entry_count` | int byte, int | `available` sejak aturan 11 aktif | *(ditambah fase 02, 2026-10-20, spec 0018)* ukuran total isi dan jumlah entri archive, dicetak `runtime pin` dari `stackly_core::runtime::archive::scan`; dipakai pipeline install fitur 18 untuk cek ruang disk dan batas archive bomb |

Contoh (nilai digest dan ukuran diisi dari `runtime pin`, tidak pernah diketik):

```toml
schema_version = 1

[[artifact]]
id = "nats-2.15.1-macos-aarch64"
provider = "nats"
line = "2.15"
version = "2.15.1"
os = "macos"
arch = "aarch64"
min_os = "<dari runtime pin>"
status = "available"
origin = "upstream"
distribution = "upstream_download"
format = "tar_gz"
urls = ["<URL aset nats-server-v2.15.1-darwin-arm64.tar.gz>"]
sha256 = "<dari runtime pin>"
# size = <angka byte dari runtime pin>
archive_root = "nats-server-v2.15.1-darwin-arm64"
upstream_checksum = { kind = "sha256sums", url = "<URL SHA256SUMS rilis v2.15.1>" }
upstream_signature = { kind = "none" }
signing_policy = "keep_or_adhoc"
license = "Apache-2.0"
license_url = "<URL LICENSE di tag v2.15.1>"
executables = ["nats-server"]

[[artifact.dependency]]
name = "libSystem"
kind = "system"
install_name = "/usr/lib/libSystem.B.dylib"
```

**Evidence**: record `stackly.evidence/v1` dari spec 0002, tanpa field baru. Folder git baru `evidence/runtime/<runId>/` (nama folder sama dengan `runId` report) berisi `report.json` (format `stackly.evidence-report/v1`, hanya record `PASS` dan `FAIL` dengan `test.language` `rust`, `test.package` `xtask`, dan `test.target` `native_runtime`; `problems` selalu kosong karena report bermasalah ditolak, dan `summary` dihitung ulang), `output.log` (salinan yang disanitasi dari run itu, karena `actual` record `FAIL` menunjuk ke sana: root workspace diganti `$WORKSPACE`, `$HOME` diganti `~`, dan setiap root `stackly-runtime-spike-*` diganti `$ROOT`; kalau nama user OS masih muncul, perintah gagal dengan `output.log contains user path`; file dipotong ke 1 MiB terakhir), dan `artifacts/<recordId>/spike-result.json`, sehingga `evidencePath` di record tetap benar relatif terhadap folder itu. Isi record yang disalin tidak berubah.

**Hasil spike** `stackly.runtime-spike-result/v1` (JSON camelCase, ditulis worker, disimpan test di `artifact_dir()`): `artifactId`, `sha256`, `status` (`PASS`, `FAIL`, atau `BLOCKED`), `failure` (`{ step, reason }` atau null), `launch` (`{ domain, parentIsLaunchd, controllingTty }`), `observedVersion` (sudah dinormalisasi), `rawVersion`, `minOsMeasured`, `signature` (`{ kind, authority, teamId, adhocSignedAtInstall }` per executable), `archive` (`{ files, symlinksInternal, symlinksEscaping, setuid, specialFiles }`), `libraries` (`{ static, runtime, foreign, warnings }`), `listeners` (`[{ address, port }]`), `closureProbes` (`[{ name, status, detail }]`), `quarantine` (`{ outcome, detail }`, dengan `outcome` `ran`, `blocked`, `killed`, `timeout`, atau `skipped`), `stop` (`{ exitCode, signal, durationMs }`), `steps` (`[{ name, status, durationMs, detail }]`), dan `logTail` (stdout dan stderr tiap proses, masing masing paling banyak 4 KiB). Worker mengganti path root spike dan realpath root itu dengan `$ROOT` di setiap field string, jadi nama user dan nama host tidak masuk git.

### State transitions dan alur bukti

- Artifact: entri baru lahir `available` (artefak ada dan digest dipin) atau `blocked`. `available` menjadi `blocked` kalau spike `FAIL` dan tidak bisa diperbaiki, atau upstream menarik artefaknya. `blocked` menjadi `available` setelah artefak ada dan record native `PASS` untuk `sha256` artefak itu sudah di `evidence/runtime/`. Bump versi mengganti entri lama dengan entri baru di PR yang sama; riwayat git menjadi catatannya.
- Record evidence: mengikuti tabel status spec 0002 (`NOT_RUN` lalu `PASS`, `FAIL`, atau `BLOCKED`); record final tidak pernah diubah.
- Alur dua commit dalam satu PR: (1) commit perubahan inventaris; (2) di tree bersih itu (tanpa perubahan dan tanpa file untracked yang tidak di-gitignore), jalankan `cargo xtask evidence run --kind native -- cargo test -p xtask --test native_runtime -- --ignored` di Mac uji macOS 27, lalu `cargo xtask runtime record <path>` dengan path dari baris `evidence report:` yang dicetak wrapper; (3) commit `evidence/runtime/<runId>/`; (4) push keduanya. `runtime check` baru lulus setelah commit kedua ada.
- `evidence/runtime/` adalah bukti G0. Bukti rilis untuk fitur 65 mengikuti spec 0002.

### Langkah spike per provider

Port dialokasikan worker tepat sebelum setiap start lewat helper `xtask/src/runtime/ports.rs` (bind `127.0.0.1:0`, lalu lepas). Socket PostgreSQL dan MySQL ada di folder pendek `/tmp/sk-<8 hex>` berizin 0700 (path socket Unix dibatasi sekitar 104 byte). `cwd` setiap proses adalah folder run artefak itu, dan semua proses server dijalankan langsung sebagai child di process group sendiri, tidak lewat pembungkus yang melakukan `setsid` atau daemonize. Langkah ini `// provisional: F9` sampai manifest provider memegang lifecycle.

| Provider | Init | Start | Ready (≤ 120 detik) | Versi teramati, normalisasi | Stop (≤ 60 detik) | Probe tambahan |
| --- | --- | --- | --- | --- | --- | --- |
| PostgreSQL | `initdb -D <data> -U stackly --auth-local=trust --auth-host=scram-sha-256 -E UTF8 --locale=C --locale-provider=icu --icu-locale=und` | `bin/postgres -D <data> -k <sock> -p <port> -c listen_addresses=''` | `psql -X -h <sock> -p <port> -U stackly -d postgres -Atc 'select 1'` | `show server_version`, ambil `^[0-9]+\.[0-9]+` | SIGINT ke `postgres` (fast shutdown) | probe AC-7; probe `manifest` di langkah `closure` (AC-18) |
| MySQL | `mysqld --no-defaults --initialize-insecure --basedir=<root> --datadir=<data> --log-error=<run>/init.err` | `mysqld --no-defaults --basedir --datadir --socket=<sock>/m.sock --skip-networking --mysqlx=OFF --pid-file=<run>/mysqld.pid --log-error=<run>/mysqld.err` | `mysqladmin --no-defaults -u root --socket=<sock>/m.sock ping` | `select version()`, ambil `^[0-9]+\.[0-9]+\.[0-9]+` | `mysqladmin --no-defaults -u root --socket=... shutdown` | tidak ada |
| NATS | tulis `<run>/nats.conf`: `listen` dan `http` di 127.0.0.1 dengan dua port, `jetstream { store_dir }` di folder run | `nats-server -c <run>/nats.conf` | `GET /healthz` | `nats-server --version`, ambil `v([0-9.]+)` | SIGTERM | JetStream aktif di `/healthz?js-enabled-only=true` |
| ClickHouse | tulis `<run>/config.xml` (hanya `listen_host` 127.0.0.1, `tcp_port`, `http_port`, `path`, `tmp_path`, `user_files_path`, logger ke folder run; tanpa port MySQL, PostgreSQL, interserver, atau gRPC) dan `users.xml` (user `default` tanpa password, hanya dari localhost) | `clickhouse server --config-file=<run>/config.xml` | `GET /ping` | `SELECT version()` lewat HTTP, empat komponen penuh | SIGTERM | audit listener (PRD 32.2) |
| Valkey | buat folder `<data>` | `valkey-server --bind 127.0.0.1 --port <p> --dir <data> --save '' --appendonly no --daemonize no` | `valkey-cli -h 127.0.0.1 -p <p> PING` menjawab `PONG` | `valkey_version` dari `INFO server`, ambil `^[0-9]+\.[0-9]+\.[0-9]+` | `valkey-cli -h 127.0.0.1 -p <p> shutdown nosave` | probe `manifest` di langkah `closure` (AC-18) |
| Traefik | tulis folder file provider kosong di folder run | `traefik --entryPoints.web.address=127.0.0.1:<p> --ping=true --ping.entryPoint=web --providers.file.directory=<run>/dynamic --api=false --global.checknewversion=false --global.sendanonymoususage=false` | `GET /ping` | `traefik version`, baris `Version:` | SIGTERM | tidak ada |

Dua baris itu dibangun sebagai implementasi trait `Recipe` baru di `xtask/src/runtime/spike/providers.rs`, didaftarkan di `recipe()` sebagai `"postgresql"` dan `"valkey"` (test yang sekarang menyatakan `recipe("postgresql").is_none()` dibalik):

- **`Postgres`**: `uses_socket_dir` true. `init` menjalankan `initdb` lewat `ctx.run_logged` dengan `INIT_TIMEOUT`. `start` mengalokasikan satu port (nomor port menentukan nama socket `.s.PGSQL.<port>` walau `listen_addresses` kosong) dan mengembalikan perintah `bin/postgres` di tabel, sehingga postmaster menjadi child langsung di process group worker. `ready` dan `version` memakai `psql` di tabel. `probes` mengembalikan empat probe AC-7 bernama `icu` (`select datlocprovider from pg_database where datname = 'postgres'` sama dengan `i`), `pgcrypto` (`create extension pgcrypto`, lalu `select length(gen_random_bytes(16))` sama dengan `16`), `lz4` (tabel dengan kolom `text compression lz4`, sisipkan `repeat('a', 10000)`, lalu `pg_column_compression` sama dengan `lz4`), dan `zstd` (`pg_dump -h <sock> -p <port> -U stackly --compress=zstd -f <run>/dump.zst postgres` keluar 0 dan file tidak kosong). `stop` mengirim `server.signal("INT")`. `version_args` `--version`. `log_files` tidak ada, karena stderr postmaster sudah ditangkap `Server::spawn`. `executables` entri: `bin/postgres`, `bin/initdb`, `bin/psql`, `bin/pg_dump`.
- **`Valkey`**: tanpa socket. `init` membuat folder data. `start` mengalokasikan satu port dan mengembalikan perintah `valkey-server` di tabel. `ready`, `version`, dan `stop` memakai `valkey-cli` di tabel. Tanpa probe tambahan. `version_args` `--version`. `executables` entri: `bin/valkey-server`, `bin/valkey-cli`.

Test native `postgresql_17_runs_from_managed_artifact`, `postgresql_18_runs_from_managed_artifact`, dan `valkey_9_1_runs_from_managed_artifact` ditambahkan ke `xtask/tests/native_runtime.rs` lewat helper `spike(ev, provider, Some(line))` yang sudah ada. Sampai entrinya ada, ketiganya `BLOCKED` dengan `no inventory entry for <provider> <line>`, jadi boleh masuk sebelum artefaknya terbit.

`trust` di PostgreSQL spike aman karena satu satunya jalan masuk adalah socket di folder 0700 milik user yang sama, setara dengan peer auth; user database `stackly` menjaga nama user OS keluar dari evidence. Ini konfigurasi spike, bukan default instance (fitur 19).

Urutan langkah worker: hitung ulang hash file cache terhadap plan, cocokkan magic number dengan `format`, ekstrak dengan `/usr/bin/tar` untuk `tar_gz` dan `zip` (untuk `zip` lalu beri mode 0755 pada `executables`; untuk `binary` salin lalu beri mode 0755), sesuai tabel Format artefak, laporan archive (symlink internal dan yang keluar, setuid, file device), audit statis, closure (dependency inventaris lawan audit statis, ditambah probe `manifest` untuk artefak maintainer), cek dan tanda tangan sesuai `signing_policy`, ukur `min_os`, init, start, ready, baca versi, probe, audit runtime (`lsof` library dan listener), stop, lalu quarantine bila plan memintanya.

### Mekanisme LaunchAgent dan pembersihan

- **Lock**: driver memegang lock eksklusif `<cache>/spike.lock` (`std::fs::File::lock`) selama run, sehingga delapan test yang berjalan paralel dan pemanggilan manual tidak pernah tumpang tindih. `--cleanup` dan `--prune-cache` mengambil lock yang sama sebelum mematikan atau menghapus apa pun, menunggu paling lama 20 menit, lalu keluar dengan kode 1 dan pesan `spike in progress`. Satu pengecualian *(ditambah fase 02, 2026-10-20, spec 0015)*: `--cleanup` mencabut agent test `dev.stackly.test.agent.*` sebelum mengambil lock, karena agent itu bukan milik spike dan langkah itu tidak pernah menyentuh proses spike. Lock dilepas otomatis saat prosesnya mati, jadi sisa run yang terputus tidak menahannya.
- **Driver**: (1) jalankan rutin pembersihan; (2) unduh ke cache dan cek `sha256` serta `size` sebelum jam driver mulai; (3) buat root `<TMPDIR>/stackly-runtime-spike-<runId>/` dengan file penanda pemilik; (4) putuskan quarantine dengan mencari hasil quarantine untuk `sha256` ini hanya di file yang dilacak git (`git ls-files 'evidence/runtime/*/artifacts/*/spike-result.json'`); (5) tulis plan JSON (entri inventaris, path cache, root, batas waktu, flag quarantine) dan plist ke root; (6) `launchctl bootstrap gui/<uid> <plist>`; kalau gagal karena tidak ada sesi GUI, driver keluar dengan kode 2 dan pesan `no gui login session` sehingga test memanggil `blocked(...)`; (7) tunggu, polling setiap 1 detik, sampai `<root>/result.json` muncul (ditulis atomik) atau `launchctl print gui/<uid>/<label>` menunjukkan worker berhenti; worker berhenti tanpa hasil berarti `FAIL worker exited <kode>`; batas 15 menit di luar unduhan; (8) `launchctl bootout`, lalu rutin pembersihan untuk root miliknya sendiri, di setiap jalur keluar (guard `Drop` dan handler `ctrlc` yang sudah ditambah spec 0002).
- **Plist**: `Label` tetap `dev.stackly.xtask.runtime-spike` (lock menjamin hanya satu, dan daftar Login Items tidak bertambah setiap run), `ProgramArguments` `[<std::env::current_exe()>, "runtime", "spike-worker", "--plan", <path absolut plan>]` (tidak pernah `cargo`), `RunAtLoad` true, `KeepAlive` false, `WorkingDirectory` root, `StandardOutPath` dan `StandardErrorPath` di root, `ExitTimeOut` 30, dan `EnvironmentVariables` minimal (`PATH`, `HOME` dan `TMPDIR` di root, `LANG`). launchd tidak mewarisi environment test, jadi semua yang worker butuhkan ada di plan.
- **Worker**: menolak jalan kalau parent bukan launchd (PID 1); `controllingTty` bernilai false kalau membuka `/dev/tty` gagal dengan ENXIO; handler SIGTERM (`ctrlc`, fitur `termination`) mematikan process group provider lalu menulis hasil `FAIL`; batas waktu sendiri 12 menit.
- **Rutin pembersihan** (dipakai driver dan `runtime spike --cleanup`): `pgrep -u <uid> -f` untuk proses yang command line memuat `stackly-runtime-spike-` atau `/tmp/sk-`, kirim TERM, tunggu 5 detik, lalu KILL; `launchctl bootout gui/<uid>/dev.stackly.xtask.runtime-spike` bila masih terdaftar; hapus folder `stackly-runtime-spike-*` di temp dir yang membawa penanda pemilik user ini, dan `/tmp/sk-*` milik user ini. Tanpa domain GUI, langkah bootout dilewati dan `--cleanup` tetap keluar dengan kode 0. Rutin ini tidak pernah menyentuh cache unduhan. Perintah terpisah `runtime spike --prune-cache` membuang file cache yang digest-nya tidak ada di inventaris checkout itu.
- **Urutan `--cleanup`** *(ditambah fase 02, 2026-10-20, spec 0015 dan 0018)*: (a) tanpa lock, baca `launchctl list` lalu `launchctl bootout gui/<uid>/<label>` untuk setiap label yang diawali `dev.stackly.test.agent.`, dilewati kalau domain GUI tidak ada; (b) ambil lock spike seperti di *Lock*; (c) jalankan *Rutin pembersihan*, ditambah TERM, tunggu 5 detik, lalu KILL untuk proses milik user ini yang argv nya memuat `/stackly-test-` (proses provider sisa test native dari `TestRoot`; `stacklyd` test yang menerima root lewat environment tidak ikut tercocokkan). Langkah (c) juga mematikan proses dari run `cargo test` yang sedang berjalan, jadi `--cleanup` hanya dijalankan saat tidak ada run test aktif (urutan run native spec 0002). Driver di langkah (1) dan (8) hanya menjalankan *Rutin pembersihan*: tanpa pencabutan agent test, tanpa sweep `/stackly-test-`, dan tanpa lock kedua, karena driver sudah memegang lock.
- **Quarantine**: clone dengan `cp -Rc` (clone APFS, instan walau artefak besar), beri `xattr -w com.apple.quarantine "0083;<epoch hex>;StacklySpike;<uuid>"` pada setiap file clone, jalankan perintah versi executable utama dengan batas 30 detik. Untuk binary ad hoc atau tanpa signature, hasil yang diharapkan `killed`. Di domain GUI, dialog Gatekeeper bisa tertinggal di layar; karena itu probe ini hanya jalan sekali per `sha256`.
- **Audit statis**: file Mach-O dikenali dari magic number; `lipo -archs` wajib memuat `arm64`; setiap referensi `otool -L` dan setiap `LC_RPATH` diselesaikan ke path nyata. `@rpath/`, `@loader_path/`, dan `@executable_path/` harus berujung ke file di dalam root artefak.
- **Audit runtime**: `lsof -nP -p <pid> -Ffn`, hanya entri FD `txt` yang berupa file Mach-O, path dibuat canonical (realpath), lalu dicocokkan dengan allowlist AC-6.

### API surface

Argumen diparse dengan tangan seperti task `xtask` lain.

| Perintah | Tempat jalan | Input utama | Output utama | Izin | Error utama |
| --- | --- | --- | --- | --- | --- |
| `cargo xtask runtime check` | OS apa pun, dijalankan dengan tangan | `runtimes/inventory.toml`, `evidence/runtime/*/report.json`, dan sejak fase 02 `evidence/compatibility/*/report.json` (spec 0018) | exit 0, atau satu baris `<id>: <aturan>` per pelanggaran lalu exit 1 | read only | TOML tidak valid, pelanggaran invariant, evidence `PASS` tidak ada |
| `cargo xtask runtime pin --url <https>... [--executable <path relatif>] [--sha256-url <https>]` | OS apa pun dengan `curl` | satu atau lebih URL kandidat; executable untuk `min_os`; file `.sha256` untuk entri maintainer | potongan TOML berisi `format` (dari magic number, di semua OS), `urls`, `sha256`, `size`, dan `min_os` (di macOS), ditambah `unpacked_size` dan `entry_count` dari `stackly_core::runtime::archive::scan` di semua OS (fase 02, spec 0018) | jaringan | URL bukan https, unduhan gagal atau lebih dari 4 GiB, URL kandidat menghasilkan digest berbeda, digest tidak sama dengan `.sha256` |
| `cargo xtask runtime pin --file <path> --url <https>... [--executable <path relatif>]` | OS apa pun (`min_os` hanya di macOS) | file artefak maintainer hasil `build.sh` dan `<path>.sha256` di sampingnya; URL release rencana (tidak dihubungi) | potongan TOML yang sama dengan baris di atas; file disalin ke cache dengan nama digest-nya; `unpacked_size` dan `entry_count` dari `scan` yang sama (fase 02, spec 0018) | menulis cache, tanpa jaringan | `--file` bersama `--sha256-url`, file lebih dari 4 GiB, `<path>.sha256` hilang, nama file di dalamnya berbeda, digest tidak cocok, atau nama file terakhir sebuah `--url` berbeda dari nama file lokal (cache tidak ditulis) |
| `./build.sh <provider> <version> <build>` (repo `stackly-dev/runtimes`) | Mac ini, macOS 27 dengan Xcode 27 | checkout bersih, resep di `recipes/` | `out/<id>.tar.gz`, `out/<id>.tar.gz.sha256`, `out/<id>.log` | jaringan untuk source dan wheel; menulis `out/` dan `/private/tmp/stackly-build/<id>/`; tanpa `sudo` | tree kotor, preflight `/usr/local`, sha256 source tidak cocok, `/opt/homebrew` di log konfigurasi, path home di paket, audit bash, cek versi salinan paket |
| `cargo xtask runtime verify [--id <id>]` | OS apa pun dengan `curl` | inventaris | `OK`, `FAIL <alasan>`, atau `SKIP gpg` per artefak; checksum upstream dicocokkan per `kind`, GPG hanya kalau `gpg` ada di PATH; sejak fase 02 juga membandingkan `unpacked_size` dan `entry_count` dengan hasil `scan` (spec 0018) | jaringan | digest, ukuran, `format` (cek format), checksum, signature, `unpacked_size`, atau `entry_count` tidak cocok; URL tidak bisa dijangkau |
| `cargo xtask runtime spike --id <id> --result <file> [--no-quarantine]` | macOS saja | inventaris, cache, `evidence/runtime/`; `--no-quarantine` untuk spike lokal sebelum unggah | file hasil spike; exit 0 `PASS`, 1 `FAIL`, 2 `BLOCKED` | domain launchd `gui` milik user sendiri, tanpa `sudo` | bukan macOS, tanpa sesi GUI, bootstrap gagal, melewati batas waktu |
| `cargo xtask runtime spike --cleanup` | macOS | tidak ada | agent test dicabut lebih dulu tanpa lock, lalu proses, agent, dan folder sisa dibersihkan di dalam lock (lihat Rutin pembersihan); cache tidak disentuh; exit 0 walau domain GUI tidak ada | domain user sendiri; lock spike untuk semua langkah kecuali pencabutan agent test | lock tidak didapat dalam 20 menit (`spike in progress`), setelah agent test sudah dicabut |
| `cargo xtask runtime spike --prune-cache` | macOS | inventaris | file cache yang digest-nya tidak ada di inventaris dihapus | folder cache, lock spike | lock tidak didapat dalam 20 menit (`spike in progress`) |
| `cargo xtask runtime spike-worker --plan <file>` | internal, hanya dari launchd | plan JSON dari driver | `result.json` (ditulis atomik) | menolak jalan kalau parent bukan launchd (PID 1) | batas waktu worker, SIGTERM saat bootout |
| `cargo xtask runtime record <report.json>` | OS apa pun | report dari `cargo xtask evidence run --kind native` | `evidence/runtime/<report.runId>/` berisi report yang difilter ke record `PASS` dan `FAIL`, `output.log` yang disanitasi, dan `artifacts/<recordId>/spike-result.json` | menulis repo | report dirty, kind fixture, `problems` tidak kosong, `interrupted: true`, `context.sourceCommit` bukan `HEAD`, `evidencePath` di luar `artifacts/` atau tanpa `spike-result.json`, tidak ada record yang bisa disimpan (`no record to keep`), folder tujuan sudah ada, `output.log contains user path` |
| `cargo xtask runtime compat-record <report.json>` *(ditambah fase 02, 2026-10-20, spec 0018)* | OS apa pun | report dari `cargo xtask evidence run --kind native` milik test provider | `evidence/compatibility/<report.runId>/` berisi report yang difilter ke record `PASS` dan `FAIL` yang `covers` nya memuat `BR-025`, `output.log` yang disanitasi, dan `artifacts/<recordId>/compatibility.json` (`stackly.compatibility-evidence/v1`) | menulis repo | penolakan yang sama dengan `runtime record` (dirty, fixture, `problems`, `interrupted`, commit bukan `HEAD`, folder tujuan ada); record BR-025 yang bukan `kind: native`, tanpa subject managed dengan digest, atau tanpa `compatibility.json`; `no record to keep` kalau tidak ada record BR-025 |

Unggahan ke GitHub Releases bukan perintah xtask: setelah persetujuan eksplisit kamu, `gh release create runtime/<id> --repo stackly-dev/stackly --title <id> --notes-file <catatan> --prerelease --latest=false out/<id>.tar.gz out/<id>.tar.gz.sha256`, dengan catatan berisi source, `recipe`, toolchain, dependency, dan lisensi dari `STACKLY-ARTIFACT.json`. Release runtime selalu prerelease (diputuskan 2026-10-19): `--latest=false` saja tidak cukup, karena GitHub tetap memberi label Latest pada release Valkey, release pertama repo ini, walau dibuat dengan flag itu; prerelease tidak pernah mendapat label Latest. Release yang terbit sebelum keputusan ini diubah dengan `gh release edit runtime/<id> --repo stackly-dev/stackly --prerelease`; tag, aset, dan URL unduhannya tidak berubah.

Test native di `xtask/tests/native_runtime.rs` (memakai `env!("CARGO_BIN_EXE_xtask")` untuk menjalankan driver), satu fungsi `#[test] #[ignore = "native: needs network, macOS 27, and provider artifacts"]` per lini: `postgresql_17_runs_from_managed_artifact`, `postgresql_18_runs_from_managed_artifact`, `mysql_8_4_runs_from_managed_artifact`, `mysql_9_7_runs_from_managed_artifact`, `nats_2_15_runs_from_managed_artifact`, `clickhouse_runs_from_managed_artifact`, `valkey_9_1_runs_from_managed_artifact`, dan `traefik_3_7_runs_from_managed_artifact`. Setiap test:

- membuat guard lewat `evidence!(covers = ["AC-078", "REQ-005"], kind = Kind::Native, expected = ...)` sebagai pernyataan pertama di fungsinya sendiri, lalu meneruskan `&Evidence` ke helper bersama (aturan `evidence check` spec 0002 mencari makro itu di setiap fungsi `#[ignore]`);
- mencari entri lewat `(provider, line)`, kecuali ClickHouse yang dicari lewat provider saja;
- memanggil `artifact_dir()` lebih dulu dan menyuruh driver menulis hasil spike ke sana;
- memanggil `blocked(reason, owner)` untuk entri `blocked`, untuk entri yang belum ada (`no inventory entry for <provider> <line>`, owner `@ojiepermana`), dan untuk exit code driver 2;
- selain itu memanggil `subject(...)` dengan `observedVersion` dan `sha256`, atau dengan `version` dari inventaris kalau spike `FAIL` sebelum versi terbaca (satu satunya pengecualian dari aturan versi teramati di spec 0002, supaya record `FAIL` tetap terhubung ke digest-nya dan menggantikan `PASS` lama di aturan 8), lalu `pass_with("<provider> <observedVersion> ready in <ms> ms, stopped in <ms> ms")` kalau hasilnya `PASS`, atau panic dengan `<langkah>: <alasan>` kalau `FAIL`.

Run lokal: `cargo xtask evidence run --kind native -- cargo test -p xtask --test native_runtime -- --ignored`.

### Resep build maintainer

Resep tinggal di repo publik `stackly-dev/runtimes` (berlisensi MIT OR Apache-2.0 seperti repo ini), dengan checkout lokal di `~/Development/ojiepermana/stackly-runtimes`, di samping checkout `stackly`. Repo itu tidak menyimpan artefak; artefak terbit di GitHub Releases repo `stackly-dev/stackly`. Membuat repo di GitHub dan push pertamanya kamu yang melakukan atau setujui.

```text
stackly-runtimes/
├── build.sh                 # ./build.sh <provider> <version> <build>, dan ./build.sh --clean
├── lib/common.sh            # env, unduh dan cek source, .pc SDK, relocate, sign, audit, scan, package, manifest, cek versi
├── recipes/
│   ├── deps/openssl.sh, icu.sh, lz4.sh, zstd.sh   # versi, URL, dan sha256 source dipin di file
│   ├── postgresql.sh        # 17.11 dan 18.6, masing masing dengan URL dan sha256 source
│   └── valkey.sh
├── requirements.txt         # meson 1.11.2, ninja 1.13.2, pkgconf 2.4.3.post2, dipin dengan hash
├── tests/check.sh           # fixture lulus dan gagal untuk setiap fungsi cek di lib/common.sh
├── .gitignore               # out/
└── LICENSE-MIT, LICENSE-APACHE, README.md
```

Setiap file resep memuat versi, URL https, dan sha256 untuk setiap tarball source, plus komentar checksum upstream yang dicocokkan maintainer saat versi itu dipin: `.sha256` untuk PostgreSQL, OpenSSL, lz4, dan zstd, `SHASUM512.txt` (SHA-512) untuk ICU, dan daftar `valkey-hashes` untuk Valkey. `build.sh` menolak `<version>` yang tidak dipin di resep provider itu.

**Alur `build.sh`**:

1. Cek tree bersih (AC-17), lalu bentuk `recipe` = `stackly-dev/runtimes@<git rev-parse HEAD>:recipes/<provider>.sh`.
2. Preflight `/usr/local`: clang selalu mencari `/usr/local/include` dan linker selalu mencari `/usr/local/lib` lebih dulu, walau SDK dipakai (lihat rationale). Skrip menolak mulai, dengan daftar isi yang menyalahi, kalau `/usr/local/include` memuat file apa pun di tingkat puncak (header puncak seperti `iconv.h` atau `zlib.h` bisa menimpa header SDK) atau folder bernama `openssl`, `unicode`, `libxml`, `libxml2`, `editline`, `readline`, atau `uuid`, atau kalau `/usr/local/lib` memuat file `lib*.dylib`, `lib*.a`, atau `lib*.tbd` di tingkat puncak. Folder lain dibiarkan, karena hanya terjangkau lewat `#include <nama/...>` (di Mac ini hanya `include/node/` dan `lib/node_modules/`).
3. Baca `SDKROOT` (`xcrun --show-sdk-path`) dan `DEVELOPER_DIR` (`xcode-select -p`), buat root build baru `/private/tmp/stackly-build/<8 hex acak>/`, lalu jalankan ulang skrip lewat `/usr/bin/env -i` dengan tabel Environment build. Nama root acak, bukan id artefak, karena PostgreSQL memakai layout datar kalau prefix memuat kata `postgres` atau `pgsql`.
4. Tulis seluruh environment ke awal log, buat venv (`/usr/bin/python3 -m venv <build>/venv`, lalu `pip install --no-cache-dir --require-hashes --only-binary :all: --no-deps -r requirements.txt`), dan tulis file `.pc` SDK.
5. Unduh setiap source dengan `/usr/bin/curl --proto =https --proto-redir =https --fail --location` ke `out/sources/<sha256>` (dicek ulang setiap dipakai), cocokkan sha256 yang dipin, baru ekstrak di root build.
6. Bangun dependency (hanya PostgreSQL), lalu provider, ke prefix staging di root build. Lalu scan log konfigurasi: `/opt/homebrew` di `meson-logs/meson-log.txt` atau `config.log` membuat build gagal.
7. Relokasi, tanda tangan ad hoc, dan audit bash.
8. Susun folder paket, scan path home (nilai `HOME` asli yang disimpan sebelum `env -i`, dicari dengan `grep -rlF` di semua file paket; ada hasil berarti gagal), tulis `STACKLY-ARTIFACT.json` dengan `/usr/bin/jq -n` (bawaan macOS 27), buat tarball dan `.sha256`.
9. Cek versi: ekstrak tarball ke `<build>/check Stackly Ü/`, jalankan setiap executable yang didaftar resep (PostgreSQL `postgres`, `initdb`, `psql`, `pg_dump`; Valkey `valkey-server`, `valkey-cli`) dengan `--version` di environment build, dan keluarannya harus memuat `<version>`. Untuk PostgreSQL, `pg_config --sharedir` dan `pg_config --pkglibdir` dari salinan itu juga harus berada di bawah folder salinan, bukan di prefix staging, karena root build masih ada saat cek ini jalan.
10. Salin `.tar.gz`, `.sha256`, dan log ke `out/`, lalu hapus root build. Root build yang gagal dibiarkan untuk diperiksa; `./build.sh --clean` menghapus semua folder di `/private/tmp/stackly-build/` milik user ini.

Log build tetap lokal di `out/` dan tidak pernah diunggah, karena memuat path checkout di home. Fungsi cek di `lib/common.sh` (preflight, scan log, scan home, audit) menerima path sebagai argumen supaya `tests/check.sh` bisa mengujinya dengan folder sementara.

**Notice pihak ketiga** (diputuskan 2026-10-18 di gerbang lisensi Valkey): source provider memuat file dari penulis lain berlisensi gaya BSD yang notice-nya hanya ada di header file, jadi file lisensi utama saja tidak cukup. Setiap resep provider memuat dua daftar: pemilik provider sendiri (Valkey: Valkey contributors, Redis Ltd, Salvatore Sanfilippo, dan kontributor Redis; PostgreSQL: PostgreSQL Global Development Group dan Regents of the University of California) dan file notice (path relatif di tarball source). Sebelum langkah 8, `build.sh` memindai folder source provider (Valkey `src/`; PostgreSQL `src/` dan `contrib/`; `deps/` Valkey sudah tercakup `vendored`). File `.c` atau `.h` yang blok komentar pertamanya memuat baris `Copyright` tanpa nama pemilik provider, dan tidak ada di daftar, membuat build gagal dengan `<path> has a third party copyright notice that the recipe does not list`; file di daftar yang tidak ada di source juga membuat build gagal. Di langkah 8, `LICENSES/<provider>.txt` ditulis sebagai file lisensi utama (Valkey `COPYING`, PostgreSQL `COPYRIGHT`), lalu judul `Third party notices`, lalu untuk setiap file di daftar: path relatifnya dan blok komentar pertama file itu. Probe `manifest` tidak berubah, karena ia hanya memastikan `LICENSES/<provider>.txt` ada.

**Environment build** (setelah `env -i`, `<build>` adalah root build):

| Variabel | Nilai |
| --- | --- |
| `PATH` | `<build>/venv/bin:/usr/bin:/bin:/usr/sbin:/sbin` |
| `HOME` | `<build>/home`, baru dan kosong |
| `TMPDIR` | `<build>/tmp` |
| `LANG` | `C` |
| `SDKROOT`, `DEVELOPER_DIR` | dibaca sebelum `env -i` |
| `STACKLY_ORIGINAL_HOME` | `HOME` asli, hanya untuk scan path home di paket |
| `STACKLY_RECIPE` | nilai `recipe` dari langkah 1 |
| `STACKLY_BUILD_ROOT` | root build dari langkah 3 |
| `STACKLY_BUILD_ARGS` | `<provider> <version> <build>` |
| `MACOSX_DEPLOYMENT_TARGET` | `27.0` |
| `PKG_CONFIG_LIBDIR` | `<build>/deps/lib/pkgconfig:<build>/sdk-pkgconfig` |
| `LDFLAGS` | `-Wl,-headerpad_max_install_names` (ruang untuk `install_name_tool`) |

Root build ada di luar home, jadi path yang tertanam di binary (prefix staging, `__FILE__`) tidak memuat nama user tanpa perlu remap path source. `min_os` artefak menjadi `27.0`.

**Toolchain**:

| Tool | Sumber | Versi di Mac ini (18 Oktober 2026) | Minimum PostgreSQL 17 dan 18 |
| --- | --- | --- | --- |
| clang, SDK, `install_name_tool`, `codesign`, `otool`, `vtool`, `lipo` | Xcode 27 lewat `xcrun` | Xcode 27.0 (27A266a), SDK 27.0, Apple clang 21.0.0 | tidak ada |
| meson, ninja, pkgconf | venv `/usr/bin/python3` 3.9.6, wheel PyPI dipin hash | 1.11.2, 1.13.2, 2.4.3.post2 (meson dan pkgconf: rilis terakhir yang masih mendukung Python 3.9) | meson 0.54 |
| bison, flex, perl, make | `/usr/bin` | bison 2.3, flex 2.6.4, perl 5.34.1, GNU make 3.81 | bison 2.3, flex 2.5.35, perl 5.14 |

Sejak PostgreSQL 17, tarball source tidak lagi membawa file parser hasil generate, jadi bison, flex, dan perl wajib ada. bison sistem tepat di batas minimum.

**Library sistem lewat `.pc` SDK**: `lib/common.sh` menulis empat file kecil di `<build>/sdk-pkgconfig/`, karena meson PostgreSQL hanya menemukan `libxml-2.0` dan `uuid` lewat pkg-config atau CMake, dan mencari libedit lewat pkg-config lalu `find_library('libedit')`, yang tidak menemukan `libedit.tbd` di SDK:

- `libxml-2.0.pc`: `Version` dari `/usr/bin/xml2-config --version`, `Cflags: -I${SDKROOT}/usr/include/libxml2`, `Libs: -lxml2`.
- `libedit.pc`: `Version: 3.0`, `Libs: -ledit` (header `editline/readline.h` sudah ada di include SDK).
- `zlib.pc`: `Version` dari `ZLIB_VERSION` di `zlib.h` SDK, `Libs: -lz`.
- `uuid.pc`: `Version: 1.0`, tanpa `Cflags` dan `Libs`, karena `uuid/uuid.h` dan `uuid_generate` ada di SDK dan `libSystem` (untuk `-Duuid=e2fs`).

Semuanya tetap tertaut ke `/usr/lib/` (dependency `system`). Folder `pkgconfig` Homebrew tidak pernah ikut, karena `PKG_CONFIG_LIBDIR` menggantikan seluruh path bawaan pkgconf.

**Dependency** (hanya PostgreSQL): dibangun ke `<build>/deps`; hanya dylib dan file lisensinya yang masuk paket (tanpa `bin/openssl`, header, `.a`, atau `.pc`).

- OpenSSL 3.5: `./Configure darwin64-arm64-cc shared no-tests no-docs no-module --openssldir=/etc/ssl --prefix=<build>/deps --libdir=lib`, lalu `make` dan `make install_sw` (bukan `make install`, yang mencoba menulis ke `/etc/ssl`). macOS punya `/etc/ssl/cert.pem`, sehingga OPENSSLDIR tetap berarti setelah relokasi.
- ICU: `--with-data-packaging=library --disable-samples --disable-tests --prefix=<build>/deps`, sehingga data ICU ada di dalam `libicudata` dan ikut pindah. ICU harus dibangun sendiri karena SDK hanya membawa 23 header ICU (tanpa `ucol.h`, `ucnv.h`, atau `uloc.h`) dan `libicucore` bukan API publik. Python venv ada di `PATH`, jadi configure ICU boleh membangun data dari source; hasilnya data yang sama dengan file data bawaan tarball.
- lz4 dan zstd: `make` lalu install dylib saja.

**PostgreSQL**: meson dari tarball resmi, prefix `<build>/stage/stackly-pg` (root build dan prefix tanpa kata `postgres` atau `pgsql`), sehingga layout tetap `lib/postgresql/` dan `share/postgresql/` dan lookup relatif PostgreSQL dari `bin/` bekerja setelah dipindah. Opsi untuk 17 dan 18: `--buildtype=release -Dlibdir=lib -Ddocs=disabled -Dssl=openssl -Dicu=enabled -Dlz4=enabled -Dzstd=enabled -Dlibxml=enabled -Dzlib=enabled -Dreadline=enabled -Dlibedit_preferred=true -Duuid=e2fs`, dan `disabled` untuk `gssapi`, `ldap`, `pam`, `bonjour`, `bsd_auth`, `selinux`, `systemd`, `tap_tests`, `nls`, `llvm`, `plperl`, `plpython`, `pltcl`, `libxslt`. Khusus 18 juga `disabled` untuk `libcurl`, `libnuma`, dan `liburing`; opsi itu tidak ada di 17, jadi `recipes/postgresql.sh` menyimpan daftar opsi per versi mayor. Di macOS PostgreSQL tidak menambah rpath dan memakai install name absolut ke prefix staging, yang semuanya diganti saat relokasi.

**Valkey**: `make BUILD_TLS=no USE_SYSTEMD=no MALLOC=libc`, lalu `make PREFIX=<build>/stage/valkey USE_REDIS_SYMLINKS=no install`, sehingga symlink kompatibilitas `redis-*` tidak dibuat. `make install` membuat `valkey-sentinel`, `valkey-check-aof`, dan `valkey-check-rdb` sebagai symlink ke `valkey-server`: `valkey-sentinel` dibuang, dua lainnya diganti salinan file. Paket memuat `valkey-server`, `valkey-cli`, `valkey-benchmark`, `valkey-check-aof`, dan `valkey-check-rdb`. `recipes/valkey.sh` mendaftar komponen `vendored` dan file lisensinya di folder `deps/` source.

**Isi paket dari stage**: PostgreSQL menyimpan semua `bin/` (termasuk `pg_config`), `lib/` tanpa `*.a` dan `pkgconfig/`, `lib/postgresql/`, `share/postgresql/`, dan `include/`. Dependency hanya menyumbang dylib yang dirujuk. Valkey hanya `bin/`. Header, `*.a`, dan `pkgconfig/` dependency tidak pernah masuk.

**Relokasi**, berurutan di folder paket:

1. Untuk setiap dylib yang namanya dirujuk lewat symlink berversi (misalnya `libpq.5.dylib` ke `libpq.5.18.dylib`, `libssl.dylib`, ICU `libicuuc.78.dylib`), simpan satu file nyata dengan nama yang dirujuk Mach-O lain (nama di install name aslinya), lalu buang semua symlink.
2. Setiap dylib mendapat `install_name_tool -id @rpath/<nama file>`.
3. Setiap `LC_LOAD_DYLIB` di setiap Mach-O yang menunjuk ke root build diganti `@rpath/<nama file>` dengan `install_name_tool -change`. Referensi ke `/usr/lib/` dan `/System/Library/` dibiarkan.
4. Setiap Mach-O mendapat satu rpath `@loader_path/<path relatif dari foldernya ke lib/>` (misalnya `@loader_path/../lib` untuk `bin/`, `@loader_path/..` untuk `lib/postgresql/`, `@loader_path` untuk `lib/`), dan setiap `LC_RPATH` absolut dihapus.
5. Setiap Mach-O ditandatangani ad hoc (`codesign --force --sign -`) tanpa hardened runtime.

**Audit**: menolak referensi apa pun selain `@rpath/`, `@loader_path/`, `@executable_path/`, `/usr/lib/`, dan `/System/Library/`, serta `LC_RPATH` absolut apa pun.

**Paket**: `/usr/bin/tar -czf <id>.tar.gz --no-mac-metadata --uid 0 --gid 0 --uname root --gname wheel -C <build>/pkg <id>`, dengan folder puncak `<id>` (`<provider>-<version>-r<build>-macos-aarch64`) berisi `bin/`, `lib/`, `share/`, `include/` (PostgreSQL), `LICENSES/<nama>.txt` (provider dan setiap dependency bundel), dan `STACKLY-ARTIFACT.json`. `--no-mac-metadata` mencegah entri `._<nama>`, dan opsi pemilik menjaga nama user OS keluar dari archive. File `.sha256` berisi satu baris `<hex>  <nama file>`.

`STACKLY-ARTIFACT.json` (`stackly.runtime-artifact/v1`, JSON camelCase, tanpa path, nama user OS, atau hostname): `schema`, `provider`, `version`, `build`, `source` (`{ url, sha256 }`), `recipe` (string yang sama persis dengan field inventaris), `toolchain` (`{ xcode, sdk, clang, python, meson, ninja, pkgconf, bison, flex, perl, make }`), `deploymentTarget` (`27.0`), `dependencies` (`[{ name, version, license, sourceUrl, sourceSha256, libraries }]`, dengan `libraries` nama file dylib yang disumbang dependency itu ke `lib/`; kosong untuk Valkey), `vendored` (`[{ name, license }]` untuk kode pihak ketiga yang tertaut statis dari tarball source provider; kosong untuk PostgreSQL), dan `builtAt` (RFC 3339 UTC). Setiap nilai `toolchain` hanya baris pertama keluaran perintahnya (misalnya tanpa baris `InstalledDir:` dari `clang --version`).

Smoke bash lama (yang meniru langkah spike) dihapus: spike lokal di langkah 12 menjalankan langkah yang sama dari kode yang sama, dan cek versi di `build.sh` sudah menangkap error relokasi sebelum `pin --file`.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| spike | URL, `sha256`, `size`, `format`, `archive_root`, `executables`, `signing_policy` | entri inventaris |
| spike | perintah ekstraksi, cek magic number | `format` entri, lalu tabel Format artefak; jenis file dari `macho::file_kind` pada file cache |
| spike | mode executable artefak `zip` | path dari `executables` entri; mode konstanta 0755, sama dengan format `binary`; mode lama untuk detail `extract` adalah mode di disk setelah `tar` (umask proses berlaku), dibaca dengan `symlink_metadata` sebelum `chmod` |
| spike | root ekstraksi | `TMPDIR` proses driver, lalu `stackly-runtime-spike-<runId>/Stackly Spike Ü/<id>/`; dikirim ke worker lewat plan |
| spike | port | helper `xtask/src/runtime/ports.rs`, dialokasikan worker tepat sebelum start |
| spike | folder socket | `/tmp/sk-<8 hex acak>`, mode 0700 |
| spike | perintah init, start, ready, stop, probe, normalisasi versi | tabel Langkah spike per provider |
| spike | batas waktu | konstanta: ready 120 s, stop 60 s, quarantine 30 s, worker 12 menit, driver 15 menit di luar unduhan, pembersihan 5 s antara TERM dan KILL |
| spike | user database PostgreSQL | konstanta `stackly` |
| spike | domain launchd | `gui/<uid>`; uid dari `id -u` |
| spike | label LaunchAgent | konstanta `dev.stackly.xtask.runtime-spike` |
| spike | flag quarantine | ada atau tidaknya hasil quarantine untuk `sha256` ini di file yang dilacak git `evidence/runtime/*/artifacts/*/spike-result.json` (lewat `git ls-files`) |
| spike | `controllingTty` | hasil membuka `/dev/tty` di worker |
| spike | `rawVersion`, `observedVersion` | query atau perintah versi di tabel, lalu regex normalisasi |
| spike | `minOsMeasured` | `vtool -show-build` pada executable utama |
| spike | `signature` | `codesign -dv --verbose=2` (baris Authority, TeamIdentifier, flag `adhoc` atau `linker-signed`) |
| spike | `libraries.static`, `libraries.warnings` | `otool -L`, `otool -l` (`LC_RPATH`), `lipo -archs` di setiap Mach-O |
| spike | `libraries.runtime`, `listeners` | `lsof -nP -p <pid> -Ffn` (entri `txt`), dan `lsof -nP -a -p <pid> -iTCP -sTCP:LISTEN` |
| spike | `stop` | status keluar child dari `std::process::Child::wait` |
| spike | `quarantine.outcome` | hasil spawn clone berquarantine: exit 0 `ran`, exit lain atau spawn ditolak `blocked`, mati karena sinyal `killed`, lewat 30 s `timeout`, tidak dijalankan `skipped` (detail `local run` untuk `--no-quarantine`) |
| test native | `covers`, `expected` | konstanta di test: `["AC-078", "REQ-005"]` dan satu kalimat perilaku |
| test native | `subjects` | `observedVersion` (atau `version` inventaris untuk `FAIL` sebelum versi terbaca) dan `sha256` dari hasil spike, `source: managed` |
| test native | alasan dan owner `BLOCKED` | `blocked_reason` dan `blocked_owner` entri, atau konstanta untuk entri yang belum ada dan tanpa sesi GUI (owner `@ojiepermana`) |
| test native | context, commit, host | wrapper spec 0002 |
| check | record terbaru per `sha256` | `finishedAt` terbaru di antara record `evidence/runtime/*/report.json` dengan subject yang memuat digest itu |
| check | daftar lini wajib (aturan 9) | konstanta di xtask: `postgresql` 17 dan 18, `mysql` 8.4 dan 9.7, `nats` 2.15, `clickhouse` (lini apa pun), `valkey` 9.1, `traefik` 3.7 |
| pin | `sha256`, `size`, `min_os` | byte yang benar benar diunduh; `vtool` pada `--executable` di dalam `.tar.gz` atau `.zip` (atau satu satunya file untuk format `binary`) |
| pin | `unpacked_size`, `entry_count` | `stackly_core::runtime::archive::scan` atas byte unduhan atau file `--file`, di semua OS; tidak dicetak untuk jenis `unknown` *(fase 02, spec 0018)* |
| pin | `format` | magic number byte yang diunduh (`macho::file_kind`): gzip `tar_gz`, `PK\3\4` `zip`, Mach-O `binary`; jenis lain tidak mencetak `format` dan memberi catatan (teks di Perubahan `pin`) |
| entri inventaris | `archive_root` | daftar anggota dari `/usr/bin/tar -tf <file cache>`: nama folder itu kalau semua anggota berada di satu folder puncak, selain itu tidak diisi |
| entri inventaris | `executables` | daftar anggota archive, dibatasi ke file yang dijalankan tabel Langkah spike per provider; executable utama (yang start) di urutan pertama |
| entri inventaris | `[[artifact.dependency]]` | `libraries.static` dari spike lokal pertama dengan entri tanpa baris dependency (run itu tidak disimpan sebagai evidence): setiap path di bawah `/usr/lib/` atau `/System/Library/` menjadi baris `system` dengan `name` nama file (atau nama framework) dan `install_name` path itu, seperti entri NATS; library di dalam artefak menjadi `own` atau `bundled` dengan `install_name` `@rpath/<nama file>`; untuk artefak maintainer, `bundled` adalah file yang muncul di `libraries` sebuah dependency manifest (dengan `version`, `license`, dan `source_url` dari dependency itu) dan `own` sisanya |
| pin `--file` | `sha256`, `size`, `format`, `min_os` | file lokal hasil `build.sh`, dengan cara yang sama seperti unduhan; digest dicocokkan dengan `<file>.sha256` |
| pin `--file` | `urls` | nilai `--url`: `https://github.com/stackly-dev/stackly/releases/download/runtime/<id>/<id>.tar.gz` |
| entri maintainer | `build`, `source_url`, `source_sha256`, `recipe` | `STACKLY-ARTIFACT.json` di artefak (`build`, `source.url`, `source.sha256`, `recipe`) |
| entri maintainer | `executables` | PostgreSQL `bin/postgres`, `bin/initdb`, `bin/psql`, `bin/pg_dump`; Valkey `bin/valkey-server`, `bin/valkey-cli` (dari daftar recipe di Langkah spike per provider) |
| spike | probe `manifest` | `STACKLY-ARTIFACT.json` dan `LICENSES/` di root artefak, dibandingkan dengan entri; jumlah symlink, setuid, dan file khusus dari laporan archive |
| build maintainer | source dan checksum | file resep (versi, URL, dan sha256 dipin), dicocokkan saat pin versi dengan `.sha256` (PostgreSQL, OpenSSL, lz4, zstd), `SHASUM512.txt` (ICU), atau `valkey-hashes` |
| build maintainer | `recipe` | `stackly-dev/runtimes@` + `git rev-parse HEAD` checkout bersih + `:recipes/<provider>.sh` |
| build maintainer | `id`, `build` | argumen `build.sh`; `id` = `<provider>-<version>-r<build>-macos-aarch64` |
| build maintainer | toolchain | `xcodebuild -version`, `xcrun --show-sdk-version`, `clang --version`, `/usr/bin/python3 --version`, versi tool dari `requirements.txt`, `bison --version`, `flex --version`, `perl -e 'print $^V'`, `make --version` |
| build maintainer | `deploymentTarget`, `MACOSX_DEPLOYMENT_TARGET` | konstanta `27.0` di `lib/common.sh` |
| build maintainer | versi dan lisensi dependency | variabel di file resep dependency; lisensi dari file lisensi di tarball source |
| build maintainer | hash di `requirements.txt` | digest `sha256` wheel yang dipakai, dari JSON PyPI (`https://pypi.org/pypi/<nama>/<versi>/json`, field `digests.sha256` file wheel itu) |
| build maintainer | `STACKLY-ARTIFACT.json` | ditulis dengan `/usr/bin/jq -n` dari nilai baris di tabel ini |
| build maintainer | `vendored` | daftar di `recipes/valkey.sh`, dengan lisensi dari file lisensi di folder `deps/` source |
| build maintainer | `Third party notices` di `LICENSES/<provider>.txt` | daftar file notice dan daftar pemilik provider di file resep provider; isi tiap blok dari blok komentar pertama file itu di tarball source |
| entri maintainer | `license`, `license_url` | lisensi provider sendiri; `license_url` file lisensi utamanya di tag upstream (PostgreSQL `COPYRIGHT` di mirror GitHub `postgres/postgres` tag `REL_<mayor>_<minor>`, Valkey `COPYING` di tag `<version>` `valkey-io/valkey`) |
| build maintainer | `builtAt` | jam sistem saat paket dibuat, `date -u +%Y-%m-%dT%H:%M:%SZ` |
| build maintainer | root build | `/private/tmp/stackly-build/<8 hex acak>/` |
| build maintainer | `SDKROOT`, `DEVELOPER_DIR`, path home asli | `xcrun --show-sdk-path`, `xcode-select -p`, dan `HOME` sebelum `env -i` |
| release | tag, judul, catatan | tag `runtime/<id>`, judul `<id>`, catatan dari `STACKLY-ARTIFACT.json` |
| cache | lokasi | macOS `~/Library/Caches/dev.stackly.xtask/runtime/<sha256>`; Linux `$XDG_CACHE_HOME` atau `~/.cache`, lalu `stackly-xtask/runtime/<sha256>` |

### Key invariants

Aturan yang ditegakkan `cargo xtask runtime check`:

1. `schema_version` bernilai 1; `id` unik dan sama dengan pola yang diturunkan dari `provider`, `version`, `build`, `os`, dan `arch`; `(provider, line, os, arch)` unik.
2. `version` exact dan diawali `<line>.`; `os = "macos"` dan `arch = "aarch64"`.
3. `available` mewajibkan `urls` (semua `https://`), `sha256` (64 hex huruf kecil), `size` > 0, `min_os`, `format` (`tar_gz`, `zip`, atau `binary`), `license`, `license_url`, `signing_policy`, dan `executables` (path relatif tanpa `..`). `archive_root`, kalau ada, hanya boleh bersama `format` `tar_gz` atau `zip`, dan harus satu nama folder biasa (tidak kosong, tanpa `/`, bukan `.` atau `..`).
4. `blocked` mewajibkan `blocked_reason`, `blocked_owner` (diawali `@`), dan `blocked_since`.
5. `origin = "maintainer"` jika dan hanya jika ada `build` (≥ 1), `source_url`, `source_sha256`, `recipe` yang cocok dengan pola `^[A-Za-z0-9-]+/[A-Za-z0-9._-]+@[0-9a-f]{40}:[^/].*$` dengan path tanpa komponen `..`, `distribution = "stackly_hosted"`, `signing_policy = "adhoc_pipeline"`, dan `format = "tar_gz"`. `origin = "upstream"` berarti tanpa `build`, dengan `distribution = "upstream_download"`, `signing_policy = "keep_or_adhoc"`, `upstream_checksum`, dan `upstream_signature`; `upstream_signature` berjenis `gpg` mewajibkan `url`, `key` yang menunjuk file yang ada di `runtimes/keys/`, dan `fingerprint`.
6. Dependency: `name` unik per artefak; `bundled` mewajibkan `version`, `license`, `source_url`, dan `install_name` diawali `@rpath/`, `@loader_path/`, atau `@executable_path/`; `own` mewajibkan `install_name` dengan awalan yang sama; `system` mewajibkan `install_name` di bawah `/usr/lib/` atau `/System/Library/`.
7. Setiap file `evidence/runtime/*/report.json` bisa didecode sebagai `stackly.evidence-report/v1`; nama foldernya sama dengan `runId` report itu; `summary` sama dengan hitungan ulang dari `records`; setiap record lolos `EvidenceRecord::validate` dan berstatus `PASS` atau `FAIL`; dan setiap `evidencePath` menunjuk folder di folder run itu yang memuat `spike-result.json`.
8. Setiap artefak `available` punya record dengan subject yang `digest` sama dengan `sha256` artefak itu, dan record terbaru tersebut `PASS`, `kind: native`, `sourceDirty: false`, `host.osVersion` mayor 27, `host.arch` arm64 atau aarch64, dan `subjects[].version` sama dengan `version` artefak.
9. Setiap lini wajib di daftar konstanta punya tepat satu entri. Aturan ini diaktifkan di task 12 Build plan.
10. *(Ditambah fase 02, 2026-10-20, spec 0018.)* `stable` paling banyak satu per `(provider, os, arch)`, dan hanya boleh pada entri `available`.
11. *(Ditambah fase 02, spec 0018.)* Entri `available` wajib punya `unpacked_size` dan `entry_count`, keduanya ada (tidak pernah salah satu saja), bilangan bulat tak negatif, dan `unpacked_size` 0 hanya kalau `entry_count` 0. Aturan ini aktif di akhir milestone 1 langkah 3 spec 0018 (tempat `scan` dibangun), yang juga mengisi kedua field untuk semua entri `available` yang ada.
12. *(Ditambah fase 02, spec 0018.)* Setiap file `evidence/compatibility/*/report.json` lolos aturan `runtime compat-record`: penolakan yang sama dengan `runtime record`; hanya record `PASS` atau `FAIL` yang `covers` nya memuat `BR-025`; setiap record itu `kind: native`, punya subject managed dengan digest, dan punya `artifacts/<recordId>/compatibility.json` berschema `stackly.compatibility-evidence/v1`.

Aturan yang dijaga kode spike dan build maintainer:

- Tidak ada byte artefak yang diekstrak atau dijalankan sebelum `sha256` dan `size` cocok, dan sebelum magic number file cocok dengan `format`.
- `tar` tidak pernah dijalankan dengan `-P`, dan ekstraksi `zip` selalu dengan `--no-mac-metadata`.
- Bit eksekusi hanya ditambahkan pada path `executables` artefak `zip` (mode tepat 0755), hanya pada file biasa yang path-nya tanpa symlink; file lain dan artefak `tar_gz` memakai mode dari archive.
- Spike hanya menulis di root spike miliknya, `/tmp/sk-*` miliknya, dan folder cache; tidak pernah di `~/Library/Application Support/Stackly`, tidak pernah `sudo`, tidak pernah menghapus atribut quarantine, dan tidak pernah mengubah setelan sistem.
- Hanya satu spike berjalan di satu Mac pada satu waktu (lock di folder cache).
- Proses provider berjalan langsung sebagai child di process group sendiri, dengan environment yang dikosongkan, dan group itu dimatikan (`/bin/kill -TERM -- -<pgid>`, lalu `-KILL`) di setiap jalur keluar worker; sisa dari run yang terputus ditemukan rutin pembersihan lewat `pgrep`.
- Plist LaunchAgent tinggal di root spike, tidak pernah di `~/Library/LaunchAgents`.
- File di cache tidak pernah diubah; tanda tangan ad hoc hanya pada salinan hasil ekstraksi, dan selalu untuk semua file Mach-O di salinan itu.
- Hasil spike tidak memuat path root asli, nama user OS, atau hostname.
- Record final dan isi `evidence/runtime/` tidak pernah diedit; run baru berarti folder baru.
- Archive buatan maintainer hanya berisi file biasa dan folder, tanpa `LC_RPATH` absolut; spike menegakkannya lewat probe `manifest`.
- Build maintainer tidak pernah jalan dari tree `stackly-runtimes` yang kotor, tidak pernah melihat `/opt/homebrew` atau `/usr/local` di `PATH`, tidak pernah mengekstrak source sebelum sha256-nya cocok, dan tidak pernah memakai `sudo`.
- Artefak maintainer tidak pernah diunggah sebelum spike lokal `PASS`, gerbang lisensi provider itu, commit `recipe` ada di remote, dan persetujuan eksplisit kamu untuk unggahan itu; entri maintainer tidak pernah di-commit sebelum `runtime verify --id` `OK`.
- `pin --file` tidak pernah menulis cache kalau digest file tidak cocok dengan `.sha256` di sampingnya.

### Security model

- **Akar kepercayaan digest** adalah `runtimes/inventory.toml` yang masuk lewat PR yang direview. Artefak upstream diunduh lewat HTTPS (`--proto =https --proto-redir =https`) dan dicocokkan dengan digest yang dipin; checksum dan signature upstream (SHA256SUMS, `checksums.txt`, GPG dengan kunci yang disimpan di repo) dicek saat `pin` dan `verify` sebagai lapisan kedua, bukan pengganti.
- **Artefak buatan maintainer**: dibangun di Mac ini dari commit resep publik yang tercatat, di environment kosong, dari source yang sha256-nya dipin. Akar kepercayaannya adalah Mac maintainer itu sendiri: tidak ada attestation pihak ketiga dan build tidak bisa diulang bit demi bit, jadi `STACKLY-ARTIFACT.json` dan probe `manifest` membuktikan konsistensi antara artefak dan inventaris, bukan asal yang diverifikasi orang lain. Digest yang dipin di inventaris tetap gerbangnya (AC-3), dan `runtime verify` memastikan release menyajikan byte yang sama dengan yang diuji spike. Unggahan dilakukan dengan akun GitHub kamu lewat `gh`, tidak pernah dengan token di repo.
- **Spike menjalankan kode native dengan hak user** di Mac uji. Itu residual risk yang sama dengan PRD bagian 23; digest yang dipin membatasinya ke binary yang sudah direview.
- **Tanpa credential**: PostgreSQL memakai `trust` hanya lewat socket di folder 0700, MySQL `--initialize-insecure` dengan `--skip-networking` sehingga hanya bisa dijangkau lewat socket privat, provider lain hanya di loopback selama beberapa detik, dan Traefik tidak menghubungi server versi atau telemetri. Konfigurasi ini khusus spike, bukan default instance (fitur 19 dan seterusnya).
- **Privasi evidence**: hasil spike memakai `$ROOT` untuk path, `output.log` yang disalin `runtime record` disanitasi dengan aturan yang sama ditambah `$WORKSPACE` dan `~`, user database `stackly`, dan nama file pid serta log MySQL yang eksplisit; context host dari spec 0002 tidak memuat nama host atau nama user.
- **Lisensi (AC-078, RISK-12)**: hanya PostgreSQL, Valkey, dan dependency bundel PostgreSQL yang didistribusikan ulang oleh Stackly, masing masing dengan teks lisensi di `LICENSES/`. Artefak upstream diunduh pengguna langsung dari upstream. Persetujuan distribusi tetap milik release owner, sebagai gerbang per provider sebelum unggahan pertama provider itu (langkah 12.3 dan Follow-up).

### Configuration required

- Sesi login GUI aktif di Mac ini untuk test native runtime.
- Untuk build maintainer: Xcode 27 terpilih (`xcode-select -p`), `/usr/bin/python3` (Python 3.9.6 bawaan Xcode), jaringan ke situs source dan PyPI, dan checkout bersih `stackly-dev/runtimes` di `~/Development/ojiepermana/stackly-runtimes`.
- Untuk unggahan: `gh` yang sudah login dengan akun yang boleh membuat release di `stackly-dev/stackly`. Ini dijalankan kamu, atau sesi yang sudah mendapat persetujuan eksplisit kamu untuk artefak itu.

### Critical test scenarios

- Happy path: `nats_2_15_runs_from_managed_artifact` lewat wrapper native mengunduh, mencocokkan digest, menjalankan NATS dari LaunchAgent, lulus readiness dan stop, lalu menulis record `PASS` yang disalin `runtime record` dan diterima `runtime check`. Memverifikasi **AC-3**, **AC-4**, **AC-5**, **AC-10**.
- PostgreSQL: kedua test PostgreSQL lulus termasuk probe ICU, pgcrypto, lz4, dan zstd di path berisi spasi dan `Ü`. Memverifikasi **AC-5**, **AC-7**.
- Digest salah: unit test fungsi gerbang digest dengan file yang diubah satu byte tidak pernah memanggil langkah ekstrak; entri cache yang rusak dihapus dan diunduh ulang. Memverifikasi **AC-3**.
- Library asing: fixture Mach-O kecil yang dibangun di test lalu diubah dengan `install_name_tool -change` ke `/opt/homebrew/lib/libfoo.dylib` ditolak audit statis, dengan path itu di hasil. Memverifikasi **AC-6**.
- Terputus: worker yang dihentikan di tengah run meninggalkan agent, proses provider, dan root; `runtime spike --cleanup` (atau run berikutnya) mematikan proses itu lewat `pgrep`, mencabut agent, dan menghapus root. Memverifikasi **AC-4**.
- Paralel: delapan test native dijalankan libtest secara paralel tetap menghasilkan delapan spike berurutan tanpa bentrok port atau label. Memverifikasi **AC-4**.
- Aturan inventaris: unit test `runtime check` dengan inventaris fixture, satu per aturan (field `available` hilang, URL `http://`, `blocked` tanpa owner, maintainer dengan `recipe` yang tidak cocok pola (tanpa `@`, commit pendek atau huruf besar, path absolut, path dengan `..`), entri maintainer yang masih membawa `attestation` (field tidak dikenal), dependency `bundled` tanpa lisensi, `available` tanpa `PASS`, record terbaru `FAIL`, versi subject berbeda, folder `evidence/runtime/` yang namanya bukan `runId`, `summary` yang salah, record `BLOCKED` atau `NOT_RUN` di report yang di-commit, `evidencePath` tanpa `spike-result.json`, `archive_root` pada `format = "binary"`, `archive_root` berisi `/` atau `..`). Memverifikasi **AC-2**, **AC-13**.
- Format `zip`: unit test `archive::extract` dengan zip fixture yang dibuat di test oleh penulis zip `stored` kecil di modul test (header lokal dan central directory tanpa kompresi, tanpa crate baru). Test ini `#[cfg(target_os = "macos")]`, karena `/usr/bin/tar` di Linux adalah GNU tar yang tidak membaca zip; cek magic number dan aturan 3 tetap jalan di semua OS. Kasusnya: (1) satu file bermode 0755 tanpa `archive_root` terekstrak ke folder baru dengan mode tetap 0755, di path berisi spasi dan `Ü`; (1b) path `executables` yang disimpan 0644 menjadi 0755 dan detail mencatat `set mode 0755 on <path> (was 0644)` (fixture 0644 supaya hasilnya sama di bawah umask 022 maupun 002), sementara file lain bermode 0644 tetap 0644, dan executable yang sudah 0755 tidak dicatat; path `executables` yang berupa symlink, symlink yang targetnya tidak ada, atau yang melewati symlink folder ke luar root, membuat ekstraksi gagal dengan `executable <path> is not a regular file` dan mode file target tidak berubah; path yang tidak ada gagal dengan `the artifact has no executable <path>`; (2) satu folder puncak dengan `archive_root` yang cocok dan yang tidak cocok; (3) anggota `../x` membuat ekstraksi gagal dan tidak ada file di luar folder; (4) symlink `link` ke folder di luar, lalu anggota `link/x`, membuat ekstraksi gagal dan folder luar tetap kosong; (5) anggota `__MACOSX/._<nama>` tidak menghasilkan folder `__MACOSX` dan tidak menambah xattr selain yang diberi macOS sendiri (misalnya `com.apple.provenance`). Memverifikasi **AC-16**.
- Magic number: unit test `check_format` untuk setiap pasangan `format` dan jenis file; `format = "zip"` dengan file cache `.tar.gz` gagal di `archive::extract` dengan `format zip does not match the file (gzip)`, dan `format = "tar_gz"` dengan file zip dengan `format tar_gz does not match the file (zip)`, keduanya tanpa menjalankan `tar`; `runtime verify` pada entri dengan `format` salah mencetak `FAIL format ... does not match the file (...)`. `pin` pada file zip mencetak `format = "zip"` sebagai baris pertama (di semua OS) dan, di macOS, membaca `min_os` dari `--executable` di dalam zip; `pin` pada file yang bukan Mach-O, gzip, atau zip tidak mencetak `format` maupun `min_os` dan mencetak catatan `unknown`. Aturan 3 menerima `zip` dan menolak nilai lain (test yang sekarang memakai `zip` sebagai contoh nilai salah diganti ke `tar_zst`, dan pesannya menjadi ``format must be `tar_gz`, `zip`, or `binary`, found `tar_zst` (rule 3)``). Memverifikasi **AC-1**, **AC-2**, **AC-16**.
- ClickHouse: `clickhouse_runs_from_managed_artifact` `PASS` dari `.zip` 26.8 (test native tetap hanya menilai status `PASS`, seperti lini lain). Langkah `/check verify` milestone 2b membaca `spike-result.json` record itu dan mengharapkan `signature[].kind` `developer_id` dengan `teamId` `ZNDB5FJ8ZW`, `adhocSignedAtInstall` false, `archive.files` 1, detail langkah `extract` memuat `set mode 0755 on clickhouse (was ...)` kalau `tar -tvf` menunjukkan `clickhouse` tanpa bit eksekusi (untuk 26.8.30.8: `was 0644` di bawah umask 022), `observedVersion` empat komponen penuh, listener hanya dua port loopback yang dialokasikan, dan stop karena SIGTERM dalam 60 detik. Signature Developer ID sengaja tidak dikunci di test: kalau upstream merusaknya, `keep_or_adhoc` boleh menandatangani ulang ad hoc, dan perubahan itu ketahuan di langkah verify. Memverifikasi **AC-5**, **AC-8**, **AC-13**, **AC-16**.
- Penyalinan bukti: unit test `runtime record` dengan report fixture, satu per penolakan (dirty, kind fixture, `problems` tidak kosong, `interrupted`, commit bukan `HEAD`, folder tanpa `spike-result.json`, folder tujuan sudah ada, tanpa record yang bisa disimpan, `output.log` yang masih memuat nama user), plus satu yang membuang record `BLOCKED` dan `NOT_RUN` lalu menghitung ulang `summary`, dengan isi record `PASS` dan `FAIL` yang disalin tetap sama di setiap field yang dikenal (dibandingkan setelah didecode), dan satu yang memastikan path workspace, home, dan root spike di `output.log` sudah diganti. Memverifikasi **AC-10**.
- Lock: `--cleanup` dan `--prune-cache` yang dijalankan saat spike berjalan menunggu lock, lalu keluar dengan `spike in progress` tanpa mematikan proses spike atau menghapus file cache yang dipakainya; `--cleanup` tetap sudah mencabut agent test `dev.stackly.test.agent.*` sebelum menunggu (fase 02). Memverifikasi **AC-4**.
- Cache: `--prune-cache` menghapus file cache yang digest-nya tidak ada di inventaris dan tidak menyentuh `spike.lock`; `--cleanup` tidak menyentuh cache dan keluar 0 tanpa domain GUI. Memverifikasi **AC-4**.
- `pin --file`: unit test dengan file fixture dan `.sha256` di sampingnya: digest cocok mencetak potongan TOML dengan `urls` dari `--url` dan menaruh file di cache dengan nama digest; `.sha256` yang hilang, digest yang berbeda, dan nama file yang berbeda gagal tanpa entri cache; `--file` bersama `--sha256-url` ditolak; tidak ada `curl` yang dijalankan. Memverifikasi **AC-1**, **AC-19**.
- Manifest: unit test langkah `closure` dengan folder artefak fixture `origin = "maintainer"`: manifest yang cocok membuat probe `manifest` `PASS`; masing masing `provider`, `version`, `build`, `source.url`, `source.sha256`, `recipe`, dan `deploymentTarget` yang berbeda, `STACKLY-ARTIFACT.json` yang hilang atau `schema` lain, `LICENSES/<nama>.txt` yang hilang, serta laporan archive dengan satu symlink internal membuat `FAIL` dengan nama field atau file itu di `detail`; entri upstream tidak punya probe `manifest`. Memverifikasi **AC-12**, **AC-18**.
- Recipe PostgreSQL dan Valkey: unit test normalisasi versi (`18.6` dan `17.11` dari `show server_version` tetap utuh; `valkey_version:9.1.2` di tengah teks `INFO server` menjadi `9.1.2`), dan `recipe("postgresql")` serta `recipe("valkey")` tidak lagi `None`. Memverifikasi **AC-5**.
- Valkey: `valkey_9_1_runs_from_managed_artifact` `PASS` dari artefak `r1`, dengan `signature[].kind` ad hoc, `adhocSignedAtInstall` false, probe `manifest` `PASS`, dan listener hanya satu port loopback. Memverifikasi **AC-5**, **AC-8**, **AC-18**.
- Build maintainer (di `stackly-runtimes`, `tests/check.sh`): preflight menolak folder `/usr/local` palsu yang memuat file `iconv.h` di puncak `include/`, folder `include/openssl/`, atau `lib/libz.dylib`, dan menerima folder yang hanya memuat `include/node/` dan `lib/node_modules/`; scan log menolak log yang memuat `/opt/homebrew`; scan home menolak paket dengan file yang memuat path home; audit menolak Mach-O fixture dengan referensi `/opt/homebrew/lib/libfoo.dylib` atau `LC_RPATH` absolut; scan notice menolak folder source palsu dengan file `.c` berheader `Copyright (C) 2013 Mark Adler` yang tidak ada di daftar, menerima file yang hanya menyebut pemilik provider, menolak file di daftar yang tidak ada, dan menyalin blok notice file di daftar ke bawah judul `Third party notices`. Lalu dengan tangan: `build.sh` di tree kotor gagal dengan `stackly-runtimes has uncommitted changes` sebelum unduhan apa pun, dan log build yang lulus memuat environment yang hanya berisi variabel di tabel Environment build. Memverifikasi **AC-17**.
- Urutan terbit: untuk setiap artefak maintainer, commit entri datang setelah output `runtime verify --id <id>` `OK`, release dibuat setelah spike lokal `PASS`, dan release pertama setelah persetujuan lisensi tercatat di Follow-up spec ini. Setiap release runtime berstatus prerelease (`gh release view runtime/<id> --json isPrerelease` `true`), dan `gh release list` tidak memberi label `Latest` pada satu pun. Dicek di `/check verify` dari riwayat git, log, dan halaman release. Memverifikasi **AC-19**.
- Quarantine: run pertama untuk satu digest mencatat `quarantine.outcome` selain `skipped`, sementara salinan asli tetap `PASS`; run berikutnya mencatat `skipped`. Memverifikasi **AC-9**.

## Build plan

Urutan mengikuti pendekatan Journey: satu jalur maintainer (catat sumber, buktikan jalan, simpan bukti) dibuat lengkap pada satu artefak dulu, lalu diperluas. Milestone 2 dan seterusnya butuh langkah 1 spec 0002 sudah dibangun (crate `stackly-evidence`, `cargo xtask evidence run`, dan `ctrlc` di xtask). Milestone 3 mengikuti keputusan 2026-10-18 tentang cara membangun PostgreSQL dan Valkey di Mac ini.

**Milestone 1: Inventaris dan pemeriksaan**

1. Tipe inventaris di `xtask/src/runtime/inventory.rs` (serde dan `toml`) dengan aturan 1 sampai 6, plus unit test satu per aturan memakai TOML fixture. Inventaris mulai dengan `schema_version = 1` tanpa artefak. Memenuhi **AC-2**.
2. Cache unduhan bersama (`/usr/bin/curl` dengan `--proto =https --proto-redir =https --fail --location --max-time 1800 --max-filesize <size, atau 4 GiB untuk pin>`, digest dengan crate `sha2`, cek ulang setiap dipakai, entri rusak dihapus) dan `cargo xtask runtime pin`. Memenuhi **AC-1**, **AC-3**.
3. `cargo xtask runtime verify` termasuk pencocokan checksum upstream per `kind` dan GPG opsional. Memenuhi **AC-1**, **AC-3**.
4. `cargo xtask runtime check` (aturan 1 sampai 7). Memenuhi **AC-2**.

**Milestone 2: Spike ujung ke ujung untuk artefak upstream**

5. Driver `runtime spike` dan worker `runtime spike-worker` sesuai Mekanisme LaunchAgent: lock, rutin pembersihan dan `--cleanup`, unduh dan cek sebelum bootstrap, plan dan plist, bootstrap `gui`, `BLOCKED` tanpa sesi GUI, cek parent launchd, handler SIGTERM, batas waktu, process group, dan helper port. Memenuhi **AC-3**, **AC-4**.
6. Langkah worker: hash ulang, ekstrak (dengan dan tanpa `archive_root`), laporan archive, audit statis, signature dan ad hoc sesuai policy, `min_os`, audit runtime, quarantine sekali per digest, normalisasi versi, penggantian path dengan `$ROOT`, dan file hasil spike. Memenuhi **AC-3**, **AC-5**, **AC-6**, **AC-8**, **AC-9**.
7. Langkah spike NATS, test `nats_2_15_runs_from_managed_artifact`, `cargo xtask runtime record`, aturan 8 di `runtime check`, lalu entri NATS dan evidence pertamanya masuk repo lewat alur dua commit. Memenuhi **AC-1**, **AC-2**, **AC-5**, **AC-10**.
8. Traefik, ClickHouse (dengan aturan fallback), MySQL 8.4, dan MySQL 9.7: langkah spike, test, entri (termasuk kunci GPG MySQL di `runtimes/keys/` bila URL `.asc` mentah tersedia), dan evidence; entri `blocked` bila artefak tidak ada. Memenuhi **AC-1**, **AC-5**, **AC-13**.

**Milestone 2b: format `zip` dan ClickHouse**

Ditambahkan 2026-10-17 setelah milestone 2 selesai dengan ClickHouse `blocked`. Langkahnya bernomor 15 dan 16 supaya nomor langkah 9 sampai 14 yang sudah dirujuk tidak bergeser. Tidak butuh milestone 3, jadi ini langkah berikutnya untuk jalur ClickHouse.

15. Format `zip` sesuai tabel Format artefak: aturan 3 menerima `zip` dan membatasi `archive_root`; `FileKind::Zip` untuk `50 4b 03 04` dan `check_format` di `macho.rs`, dipanggil pertama di `archive::extract` dan di `runtime verify`; perubahan `pin` (field `format` pertama, klasifikasi sebelum cek macOS, catatan `unknown`, flag `tar` per jenis di `extract_one`); ekstraksi zip di worker dengan `/usr/bin/tar -xf <cache> --no-mac-metadata -C <folder>`; unit test Format `zip`, Magic number, dan dua kasus `archive_root` baru di Aturan inventaris. Ditambahkan 2026-10-18: mode 0755 untuk setiap path `executables` artefak `zip` setelah ekstraksi (file biasa tanpa symlink di path-nya), dicatat di detail langkah `extract`, dengan unit test kasus (1b). Memenuhi **AC-1**, **AC-2**, **AC-3**, **AC-16**.
16. ClickHouse, berurutan:
    1. `cargo xtask runtime pin --url <aset clickhouse-macos-aarch64.zip rilis v26.8.29.10-lts, atau patch 26.8 terbaru saat itu> --executable clickhouse`.
    2. Cek awal isi: `/usr/bin/tar -tvf <file cache>` harus menunjukkan `clickhouse` sebagai satu file biasa (bukan symlink); modenya dicatat (rilis 26.7 sampai 26.9: `-rw-rw-r--`, 0664). Bit eksekusi diberi langkah `extract` sesuai aturan `zip` di Format artefak, yang diputuskan 2026-10-18 setelah cek ini (yang dulu mewajibkan `-rwxr-xr-x`) gagal di semua rilis.
    3. Ubah entri `clickhouse-26.8.*` dari `blocked` ke `available`: hapus `blocked_reason`, `blocked_owner`, dan `blocked_since`; isi nilai dari `pin` (`format = "zip"`, `urls`, `sha256`, `size`, `min_os`), tanpa `archive_root`, `executables = ["clickhouse"]`, dan belum ada baris dependency.
    4. Jalankan `cargo xtask runtime spike --id <id> --result <file>` sekali secara lokal (tree boleh kotor, hasilnya tidak disimpan sebagai evidence), lalu isi baris `[[artifact.dependency]]` dari `libraries.static` sesuai Value sourcing. Yang diharapkan hanya baris `system`; library non sistem apa pun membuat langkah `closure` (probe `dependencies`) `FAIL` dan perlu ditinjau.
    5. Commit perubahan inventaris, lalu jalankan `clickhouse_runs_from_managed_artifact` lewat wrapper native di tree bersih dan masukkan evidence lewat alur dua commit.

    Langkah spike ClickHouse di `providers.rs` dipakai apa adanya. Memenuhi **AC-1**, **AC-2**, **AC-5**, **AC-6**, **AC-7**, **AC-8**, **AC-9**, **AC-10**, **AC-13**, **AC-16**.

**Milestone 3: Build maintainer untuk PostgreSQL dan Valkey**

Diputuskan 2026-10-18. Jalur maintainer (bangun, spike lokal, terbit, bukti) dibuat lengkap pada Valkey dulu, artefak paling kecil tanpa dependency bundel, lalu PostgreSQL 18, lalu PostgreSQL 17. Langkah 17 baru, supaya nomor 9 sampai 16 yang sudah dirujuk tidak bergeser.

Sesi paralel:

| Sesi | Langkah | Repo dan folder | Butuh | Boleh bersamaan dengan |
| --- | --- | --- | --- | --- |
| A, dukungan xtask | 17 | repo ini (worktree sendiri) | langkah 15 (Milestone 2b) sudah di-merge, karena keduanya mengubah `pin.rs` (termasuk struct `Pinned` dan test teks persisnya) dan `inventory.rs` | B. Tidak bersamaan dengan sesi lain yang mengubah `xtask/src/runtime/` |
| B, resep dan build | 9, lalu 10 | `~/Development/ojiepermana/stackly-runtimes` (repo baru, lokal) | tidak ada | A. Build berat (OpenSSL, ICU, PostgreSQL) tapi tidak menjalankan spike, jadi tidak bentrok dengan lock spike |
| C, terbit dan bukti | 12 | repo ini | A di-merge, artefak dari B ada di `out/`, repo GitHub `stackly-runtimes` sudah dipush | tidak dengan sesi lain yang menjalankan spike native (satu spike per Mac, `--cleanup` mematikan run lain) |

C boleh mulai dengan Valkey begitu langkah 9 selesai, sementara B masih mengerjakan langkah 10.

9. (Sesi B) Repo `stackly-runtimes` lokal: `git init`, `build.sh` (cek tree bersih, preflight `/usr/local`, `env -i`, `--clean`), `requirements.txt` dengan hash, `lib/common.sh` (environment, unduh dan cek source, `.pc` SDK, relocate, sign, audit, scan log, scan home, package, manifest, cek versi), `tests/check.sh`, `.gitignore`, lisensi, README, dan `recipes/valkey.sh`. Lalu `./build.sh valkey 9.1.2 1` di tree bersih sampai `out/valkey-9.1.2-r1-macos-aarch64.tar.gz` dan `.sha256` ada. Commit di repo itu; push dan pembuatan repo GitHub publiknya menunggu kamu. Memenuhi **AC-12**, **AC-17**.
10. (Sesi B) `recipes/deps/openssl.sh`, `icu.sh`, `lz4.sh`, `zstd.sh` dengan opsi di atas, lalu `recipes/postgresql.sh` untuk 17.11 dan 18.6 (atau patch terbaru saat itu); `./build.sh postgresql 18.6 1` lalu `./build.sh postgresql 17.11 1`. Memenuhi **AC-7** (closure di sisi build), **AC-12**, **AC-17**.
11. _Dihapus 2026-10-17: CI ditiadakan, pengembangan fokus di Mac ini dulu._ Penggantinya langkah 9, 10, 12, dan 17.
17. (Sesi A) Dukungan xtask: aturan 5 baru (pola `recipe`, field `attestation` dihapus dari struct dan dari fixture test, URL fixture `stackly-runtimes` disesuaikan), `runtime pin --file`, flag `runtime spike --no-quarantine`, probe `manifest` dan cek AC-12 di langkah `closure` (dengan `PlanArtifact` mendapat `build`, `source_url`, `source_sha256`, `recipe`, dan `min_os`, dan `PlanDependency` mendapat `version`, `license`, dan `source_url`), recipe `Postgres` dan `Valkey` di `providers.rs`, tiga test native baru, dan unit test di Critical test scenarios (`pin --file`, Manifest, Recipe PostgreSQL dan Valkey, kasus `recipe` di Aturan inventaris). Path `recipe` dicek dengan `is_inner_relative_path` yang sudah ada. Aturan 9 belum diaktifkan. Memenuhi **AC-2**, **AC-5**, **AC-7**, **AC-9**, **AC-10**, **AC-12**, **AC-18**, **AC-19**.
12. (Sesi C) Untuk setiap artefak, berurutan Valkey, PostgreSQL 18, PostgreSQL 17:
    1. `cargo xtask runtime pin --file ~/Development/ojiepermana/stackly-runtimes/out/<id>.tar.gz --url https://github.com/stackly-dev/stackly/releases/download/runtime/<id>/<id>.tar.gz --executable bin/postgres` (atau `bin/valkey-server`).
    2. Tulis entri `available` dari output `pin` dan `STACKLY-ARTIFACT.json` (Value sourcing), dengan `archive_root = "<id>"` dan tanpa baris dependency, lalu `cargo xtask runtime spike --id <id> --result <file> --no-quarantine` sekali secara lokal (tree boleh kotor, hasilnya bukan evidence). Isi baris `[[artifact.dependency]]` dari `libraries.static` sesuai Value sourcing (PostgreSQL: `bundled` untuk dylib OpenSSL, ICU, lz4, dan zstd menurut `libraries` manifest; `own` untuk `libpq` dan library PostgreSQL lain; `system` untuk sisanya), lalu ulangi spike lokal sampai `PASS`. `FAIL` yang butuh perbaikan resep kembali ke sesi B dengan `build` naik satu.
    3. Gerbang lisensi, sekali per provider sebelum unggahan pertama provider itu: kamu memeriksa `LICENSES/` di artefak nyata (Valkey dengan komponen vendored-nya; PostgreSQL dengan OpenSSL, ICU, lz4, dan zstd), lalu persetujuanmu dicatat lewat `/architect` di checkbox Follow-up provider itu (tanggal dan nama). Tanpa catatan itu tidak ada unggahan untuk provider itu. Gerbang Valkey tidak menunggu artefak PostgreSQL.
    3a. Pastikan commit `recipe` sudah dipush: `git -C ~/Development/ojiepermana/stackly-runtimes branch -r --contains <commit>` tidak kosong.
    4. Minta persetujuan eksplisit kamu untuk artefak ini, lalu `gh release create` sesuai API surface. Sejak 2026-10-19 perintah itu memakai `--prerelease`; ketiga release pertama diubah lewat `gh release edit --prerelease`.
    5. `cargo xtask runtime verify --id <id>` harus `OK`.
    6. Commit entri, jalankan test native lini itu lewat wrapper native di tree bersih, lalu `runtime record` dan commit evidence (alur dua commit).

    Setelah ketiganya `PASS`, aktifkan aturan 9 di `runtime check` dengan unit test-nya. Memenuhi **AC-1**, **AC-2**, **AC-5**, **AC-7**, **AC-10**, **AC-19**.

**Milestone 4: Pembersihan cache**

13. `runtime spike --prune-cache`, dijalankan dengan tangan di Mac ini. Memenuhi **AC-4**.
14. _Dihapus 2026-10-17: CI ditiadakan, pengembangan fokus di Mac ini dulu._

## Consequences

**Positive**:

- Fitur 18 memasang dari URL dan digest asli yang sudah terbukti jalan, bukan dari tebakan, dan langkah Validate binary di fitur itu bisa memakai ulang audit dan data quarantine dari spike.
- RISK-01 terjawab dengan bukti: PostgreSQL relocatable, closure terlihat, tanpa Homebrew.
- Bukti runtime memakai format evidence yang sama dengan semua test lain, jadi fitur 65 membacanya tanpa format kedua.
- Upstream yang menarik artefak atau perubahan perilaku macOS ketahuan di run test native berikutnya di Mac ini.
- ClickHouse jalan dari binary bertanda tangan Developer ID upstream (seperti MySQL), tanpa tanda tangan ad hoc saat install.
- PostgreSQL dan Valkey bisa dibangun ulang di Mac ini kapan saja dari resep publik yang dipin, tanpa Homebrew dan tanpa CI, dan siapa pun bisa membaca resep di commit yang tercatat di artefak.
- Artefak yang rusak tidak pernah terbit, karena spike lokal dari cache mendahului unggahan, dan `runtime verify` memastikan byte yang terbit sama dengan yang diuji.

**Negative / tradeoffs**:

- Stackly kini merawat build sendiri untuk PostgreSQL dan Valkey, termasuk mengikuti patch keamanan OpenSSL dan ICU. Build ulang tetap kerja manual.
- Resep dan inventaris tinggal di dua repo. `runtime check` hanya bisa memeriksa pola `recipe`, bukan isi resep di `stackly-runtimes`, dan probe `manifest` hanya membuktikan bahwa artefak dan entri konsisten. Rujukan `recipe` baru bisa dibuka orang lain setelah repo itu dipush.
- Tanpa attestation pihak ketiga, kepercayaan pada artefak maintainer bergantung pada Mac maintainer dan akun GitHub-nya; build tidak bisa diulang bit demi bit.
- Tool build terkunci di meson 1.11.2 dan pkgconf 2.4.3.post2 karena Python 3.9 bawaan Xcode. Python 3.9 sudah lewat masa dukungannya, dan Xcode berikutnya bisa mengganti atau membuang `/usr/bin/python3`.
- bison sistem 2.3 tepat di batas minimum PostgreSQL; kalau rilis berikutnya menaikkan batas itu, bison harus ikut dibangun dari source.
- Preflight `/usr/local` menolak build kalau kelak ada header atau library yang bisa menimpa milik build di sana; perbaikannya memindahkan isi itu, bukan melonggarkan cek.
- Setiap unggahan butuh kamu (persetujuan dan akun `gh`), jadi bump versi PostgreSQL dan Valkey tidak bisa diserahkan sepenuhnya ke sesi otomatis.
- Rilis runtime ikut muncul di daftar Releases repo app. Tag `runtime/<id>` dan status prerelease memisahkannya dari rilis app (label Latest dan `releases/latest` hanya untuk rilis app), tapi daftar itu tetap lebih ramai, dan setiap rilis runtime memakai badge `Pre-release` walau artefaknya sudah diuji. Sampai rilis app pertama terbit, `gh api repos/stackly-dev/stackly/releases/latest` menjawab 404.
- Dua lini MySQL menggandakan biaya test MySQL di atas minimum PRD.
- Tanpa Developer ID, model ini bergantung pada fakta bahwa file unduhan daemon tidak diberi quarantine dan bahwa binary ad hoc boleh dijalankan agent latar. Kalau macOS 27 memperketat ini, spike akan `FAIL` dan semuanya kembali ke OQ-012.
- Artefak ClickHouse dan MySQL tidak punya file checksum upstream yang bisa dicek mesin; kepercayaannya hanya HTTPS saat `pin` dan digest yang dipin sesudahnya (plus GPG MySQL kalau tersedia).
- Fitur 18 kini harus membaca tiga format, termasuk zip, di extractor amannya: satu pembaca archive lagi untuk diuji fuzzing, dengan aturan path dan symlink yang sama dengan `tar_gz`.
- ClickHouse bergantung pada upstream yang terus menerbitkan `.zip` di samping binary self extracting. Kalau `.zip` hilang di rilis berikutnya, lini itu kembali `blocked` lewat AC-13.
- Untuk artefak `zip`, mode executable yang jalan (0755) tidak sama dengan mode di archive, dan spike menulis sekali lagi di salinan hasil ekstraksi (`chmod`) dengan guard symlink sendiri. Fitur 18 harus meniru aturan yang sama; kalau upstream nanti menyimpan 0755, aturan ini tidak mengubah apa pun.
- bsdtar mengenali format sendiri, jadi `tar -xzf` pun membaca zip. Label `format` yang salah tidak akan ketahuan dari ekstraksi, sehingga cek magic number di worker wajib ada.
- Satu run ClickHouse memakai sekitar 1,1 GB disk: zip 216 MB yang menetap di cache dan binary 892 MB hasil ekstraksi yang dihapus di akhir run (clone quarantine memakai clone APFS, jadi tidak menambah).
- Test native runtime menambah unduhan besar (tarball MySQL dan zip ClickHouse ratusan MB) dan beberapa menit ke setiap run test native di Mac ini, karena delapan spike berjalan berurutan.
- Spike butuh sesi login GUI di Mac uji; tanpa itu semua test runtime `BLOCKED`.
- Probe quarantine bisa meninggalkan dialog Gatekeeper di layar Mac, sekali per digest baru.
- Langkah spike per provider di xtask akan diganti harness fitur 36 yang membangun langkah spike dari paket provider (spec 0009); sampai saat itu ada dua tempat yang tahu cara menjalankan provider, dan penanda di `xtask/src/runtime/spike/` menjadi `// provisional: F36`.
- Bukti di `evidence/runtime/` berasal dari commit branch fitur; dengan squash merge, commit itu tidak sampai ke `main`, jadi ia hanya berlaku sebagai bukti G0, bukan bukti rilis.

**Neutral**:

- Folder baru `runtimes/` (dengan `runtimes/keys/`) dan `evidence/runtime/`, modul baru `xtask/src/runtime/`, dan test `xtask/tests/native_runtime.rs`.
- `xtask` mendapat dependency `sha2` dan `stackly-evidence` (normal, diizinkan spec 0002), serta `stackly-testkit` sebagai dependency dev.
- Artefak maintainer dibangun dengan deployment target 27.0, sama dengan target produk, jadi hanya jalan di macOS 27 ke atas.
- *Ditambah fase 02 (2026-10-20, spec 0018):* parser `xtask/src/runtime/spike/signing.rs` pindah ke adapter `MacosCodeSigning` di `stackly-platform-macos`, dan xtask memakai ulang parser itu. Kalimat "`xtask` tidak mendapat crate baru" di bawah tetap benar, karena xtask sudah bergantung pada `stackly-core` dan, di bawah `cfg(target_os = "macos")`, pada `stackly-platform-macos`.
- Repo baru `stackly-dev/runtimes` dan tag `runtime/<id>` di repo ini. `xtask` tidak mendapat crate baru: `pin --file` memakai cache dan `sha2` yang ada, probe `manifest` memakai `serde_json`, dan pola `recipe` dicek dengan parsing tangan tanpa crate regex.

## Follow-up

- [x] `/architect`: putuskan cara membangun PostgreSQL 17 dan 18 serta Valkey di Mac ini sebelum milestone 3 (OQ-003 tetap: build maintainer diperlukan). Diputuskan 2026-10-18: lihat Resep build maintainer, AC-17 sampai AC-19, dan Milestone 3.
- [x] Kamu: buat repo publik `stackly-dev/runtimes` di GitHub dan push checkout lokalnya sebelum unggahan pertama, supaya rujukan `recipe` di artefak bisa dibuka. Selesai 2026-10-18: `origin/main` memuat `8d22ef98bab9f485d26578ceed2ed5b575f2a187`, commit resep ketiga artefak r2.
- [x] `/architect` update spec 0002: tambahkan `evidence/runtime/<runId>/` ke tabel Folder dan file; ubah `sourceDirty` menjadi `git status --porcelain --untracked-files=no` (sekarang file untracked membuat setiap run lokal dirty). Setelah cross check, `sourceDirty` justru kembali ke `git status --porcelain` biasa (lihat rationale spec 0002).
- [ ] Release owner: tinjau keputusan distribusi ulang PostgreSQL, Valkey, OpenSSL, ICU, lz4, dan zstd sebelum rilis publik (AC-078, RISK-12, PRD bagian 39). PRD bukan pendapat hukum. Sejak 2026-10-18 ini gerbang langkah 12.3, satu checkbox per provider di bawah, dicentang lewat `/architect` dengan tanggal dan nama setelah kamu memeriksa `LICENSES/` di artefak nyata; tidak ada unggahan provider itu sebelum dicentang.
  - [x] Gerbang lisensi Valkey (Valkey dan komponen vendored-nya). 2026-10-18, @ojiepermana: artefak r1 tidak disetujui karena notice pihak ketiga di header `src/` belum ikut di `LICENSES/`; ketiga artefak dibangun ulang sebagai r2 dengan bagian `Third party notices` (lihat Notice pihak ketiga), dan gerbang ini diulang pada artefak r2. Disetujui 2026-10-18 oleh @ojiepermana untuk `valkey-9.1.2-r2-macos-aarch64` (sha256 `fef1d702145d599856286b3a6dc6a7d6a102d15f094dc34ead868743f3956370`, resep `stackly-dev/runtimes@8d22ef98bab9f485d26578ceed2ed5b575f2a187`), setelah memeriksa `LICENSES/`: `valkey.txt` (`COPYING` dan notice pihak ketiga dari 41 file di `src/`) serta teks lua, libvalkey, linenoise, hdr_histogram, fpconv, dan fast_float.
  - [x] Gerbang lisensi PostgreSQL (PostgreSQL, OpenSSL, ICU, lz4, dan zstd). Diperiksa pada artefak r2, yang membawa notice pihak ketiga dari `src/` dan `contrib/`. Disetujui 2026-10-18 oleh @ojiepermana untuk `postgresql-18.6-r2-macos-aarch64` (sha256 `88bb7533e639e852e5258c293aa42f0abf40aed42d18f0f714cb47a9354b6590`) dan `postgresql-17.11-r2-macos-aarch64` (sha256 `04b1eff117441c0b6f1cbefa42ded6cf5726d656e54881674defea04b515d6d3`), resep `stackly-dev/runtimes@8d22ef98bab9f485d26578ceed2ed5b575f2a187`, setelah memeriksa `LICENSES/`: `postgresql.txt` (`COPYRIGHT` dan notice pihak ketiga dari 83 file), `openssl.txt` (Apache-2.0), `icu.txt` (Unicode-3.0), `lz4.txt` (BSD-2-Clause), dan `zstd.txt` (BSD-3-Clause).
- [ ] `/architect` update spec 0001: `repository` di `[workspace.package]` (`Cargo.toml`) masih `https://github.com/ojiepermana/stackly`; ganti ke `https://github.com/stackly-dev/stackly` seperti nama repo di spec ini (lihat rationale, Update 2026-10-18: kedua repo pindah ke org `stackly-dev`). Teks spec 0001 sudah memakai nama baru sejak 2026-10-20; tinggal `Cargo.toml`, yang menjadi Follow-up spec 0001.
- [ ] Pertimbangkan immutable releases di repo `stackly-dev/stackly` (setelan repo, keputusan kamu), supaya aset runtime yang sudah terbit tidak bisa diganti diam diam. Digest yang dipin tetap gerbang utamanya.
- [ ] Kalau rilis PostgreSQL berikutnya butuh meson di atas 1.11.2 atau bison di atas 2.3, atau `/usr/bin/python3` hilang dari Xcode, putuskan ulang tool build lewat `/architect` (runner up: Python standalone yang dipin, lihat rationale).
- [ ] Fitur 60: Developer ID dan notarization untuk artefak buatan maintainer setelah OQ-012 selesai.
- [x] Fitur 9 dan 18: putuskan cara catalog (`versions.json`) membaca `runtimes/inventory.toml`, lalu ganti tipe inventaris dan langkah spike yang ditandai `// provisional: F9`. Bagian fitur 9 diputuskan spec 0009 (2026-10-18): catalog membaca `versions.json` hasil generator `cargo xtask codegen`, inventory tetap tooling, penanda di `inventory.rs` dihapus langkah 9 build plan spec 0009, dan penanda spike pindah ke `// provisional: F36` di langkah 16 spec yang sama.
- [ ] Fitur 18: putuskan cara install memakai `versions.json` (spec 0009 *`versions.json`*); field di luar `stackly.provider-versions/v1` ditambahkan lewat update spec 0009 secara additive.
- [ ] Fitur 36: harness yang membangun langkah spike dari paket provider menggantikan langkah spike per provider di xtask dan menghapus penanda `// provisional: F36`.
- [ ] Fitur 18: pakai `quarantine.outcome` untuk error terdiagnosis saat OS menolak binary, dan laporan symlink archive upstream untuk aturan ekstraksi.
- [ ] Fitur 18: extractor aman menerima `tar_gz`, `zip`, dan `binary` dengan aturan yang sama untuk kedua archive (tolak `..`, tolak penulisan lewat symlink, abaikan metadata AppleDouble, dan sebaiknya tolak path absolut alih alih membuang `/` seperti bsdtar), pilih format dari `format` entri lalu cocokkan magic number, beri mode 0755 pada path `executables` artefak `zip` dengan guard symlink yang sama dengan spike, dan putuskan apakah `binary` masih perlu kalau tetap tidak ada entri yang memakainya. Jalankan ulang kasus AC-16 (`..`, symlink, path absolut, `__MACOSX/`) terhadap pembaca zip fitur 18 sendiri, karena cara bsdtar membaca central directory dan header lokal zip tidak otomatis sama dengan pembaca lain.
- [ ] Fitur 18: archive `tar_gz` buatan Mac dengan entri `._<nama>` masih bisa memulihkan xattr (termasuk quarantine milik pembuatnya) lewat `/usr/bin/tar` spike, karena hanya `zip` yang diberi `--no-mac-metadata`. Extractor fitur 18 sebaiknya mengabaikan metadata AppleDouble untuk kedua format.
- [ ] Extension pihak ketiga (pgvector, PostGIS) dan PL/Perl atau PL/Python sebagai keputusan terpisah nanti, dengan lisensi dan closure sendiri.
- [ ] Gate G0: tech lead dan release owner menandatangani berdasarkan isi `evidence/runtime/` dan inventaris.
- [ ] `/sync`: catat perintah `cargo xtask runtime ...` (termasuk `pin --file`), repo `stackly-dev/runtimes` dengan `./build.sh`, tag `runtime/<id>`, folder `runtimes/` dan `evidence/runtime/`, dan skill `meson` serta `signing-entitlements` di `## Agent skills` root `AGENTS.md`. Skill `macos-launchd` (`alphaonedev/openclaw-graph`) dipilih tapi repo sumbernya sudah tidak ada (404), jadi catat sebagai tidak tersedia.

## Rationale

Alasan dan opsi yang dipertimbangkan: lihat [rationale.md](rationale.md).
