# 0003. Sumber dan packaging runtime provider

**Date**: 2026-10-17
**Status**: In Progress

## Summary

Spec ini memutuskan dari mana binary keenam provider berasal dan membuktikan bahwa binary itu jalan di Mac tanpa Homebrew. NATS, Traefik, ClickHouse, dan MySQL diunduh langsung dari rilis resmi upstream dengan digest (sidik jari SHA-256) yang dipin. PostgreSQL 17 dan 18 serta Valkey dibangun sendiri oleh pipeline maintainer (OQ-003: pipeline itu memang perlu), karena upstream tidak menerbitkan binary macOS yang bisa dipindah. Semua sumber, versi, lisensi, dependency, dan kebijakan signing dicatat di `runtimes/inventory.toml`, dan setiap artefak dibuktikan oleh test native yang menjalankannya dari LaunchAgent (proses latar yang dijalankan launchd, bukan Terminal), dengan evidence yang mengikuti spec 0002.

## Requirements

**User stories**:

- Sebagai runtime maintainer, saya ingin satu inventaris terverifikasi tentang asal setiap runtime, supaya fitur 18 memasang dari URL dan digest asli dan review distribusi (AC-078) punya bahan yang lengkap.
- Sebagai release owner, saya ingin bukti bahwa setiap executable jalan dari artefak managed sebagai child process dari proses noninteractive di macOS 27, supaya gate G0 bisa ditandatangani.
- Sebagai pengguna Stackly nanti, saya ingin runtime yang jalan dari folder Stackly sendiri tanpa Homebrew global, supaya banyak versi tetap bisa direproduksi.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-078):

- **AC-1**: `runtimes/inventory.toml` berisi tepat delapan artefak, satu per lini: PostgreSQL 17, PostgreSQL 18, MySQL 8.4, MySQL 9.7, NATS 2.15, ClickHouse (lini sesuai aturan fallback), Valkey 9.1, dan Traefik 3.7. Setiap entri `available` dengan semua field wajibnya, atau `blocked` dengan alasan, owner, dan tanggal. `sha256` dan `size` setiap entri `available` berasal dari unduhan nyata lewat `cargo xtask runtime pin` (untuk entri maintainer, digest itu juga cocok dengan file `.sha256` di release), dan `cargo xtask runtime verify` menghasilkan nilai yang sama.
- **AC-2**: `cargo xtask runtime check` lulus pada isi repo, dan gagal dengan pesan `<id artefak>: <aturan>` untuk setiap pelanggaran aturan di *Key invariants*, termasuk artefak `available` yang record native terbarunya untuk `sha256` yang sama bukan `PASS` yang diterima aturan 8. Perintah ini jalan di job `linux` di setiap PR.
- **AC-3**: Driver spike, `pin`, dan `verify` mencocokkan `sha256` dan `size` sebelum file diekstrak atau dijalankan, dan worker menghitung ulang hash file cache terhadap plan sebelum ekstraksi. Unduhan atau isi cache yang tidak cocok tidak pernah diekstrak atau dijalankan; cache yang rusak dihapus lalu diunduh ulang; dan hasil spike menjadi `FAIL` dengan alasan `digest mismatch` atau `size mismatch`.
- **AC-4**: Setiap executable provider dijalankan oleh worker yang dijalankan launchd lewat LaunchAgent sementara di domain `gui/<uid>` (parent worker adalah launchd, tanpa TTY pengendali), dan hasil spike mencatat domain itu. Tanpa sesi login GUI, test tercatat `BLOCKED`, bukan `FAIL`. Agent dicabut dan folder spike dihapus di setiap jalur keluar. Agent, proses provider, dan folder sisa run yang terputus dibersihkan oleh run berikutnya atau `cargo xtask runtime spike --cleanup`, yang mengambil lock yang sama dengan driver sehingga tidak pernah mematikan spike yang sedang berjalan. Worker tidak pernah melewati batas waktunya, dan dua spike tidak pernah berjalan bersamaan di satu Mac.
- **AC-5**: Untuk setiap artefak `available`, spike mengekstrak ke folder dengan path yang berisi spasi dan karakter non ASCII, menyiapkan data bila provider membutuhkannya, menjalankan server hanya di port loopback yang dialokasikan atau socket privat (listener lain membuat `FAIL`), lulus cek readiness provider dalam 120 detik, dan berhenti sendiri dalam 60 detik setelah perintah stop tanpa SIGKILL dari worker (kode keluar atau sinyal dicatat). Versi yang dibaca dari binary yang berjalan, setelah dinormalisasi per provider, sama dengan `version` di inventaris. PostgreSQL 17 dan PostgreSQL 18 keduanya `PASS`.
- **AC-6**: Audit statis (`otool -L` dan `LC_RPATH` di setiap file Mach-O dalam artefak, wajib punya slice arm64) dan audit runtime (entri `txt` Mach-O dari `lsof` pada proses server yang berjalan, path dibuat canonical) tidak menemukan library di luar allowlist sistem (`/usr/lib/`, `/System/Library/`, `/System/Volumes/Preboot/Cryptexes/OS/System/Library/`, `/System/Cryptexes/OS/System/Library/`) dan folder artefak itu sendiri. Path lain apa pun membuat `FAIL` dengan path itu tercatat. `LC_RPATH` absolut di luar sistem membuat `FAIL` untuk artefak maintainer; untuk artefak upstream hanya dicatat sebagai peringatan, kecuali audit runtime menunjukkan library benar benar dimuat lewat path itu. Proses provider mendapat environment yang dikosongkan: `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, `HOME` dan `TMPDIR` baru di folder run, `LANG=en_US.UTF-8`, tanpa variabel `DYLD_*` atau Homebrew.
- **AC-7**: Artefak PostgreSQL lulus probe closure: database `postgres` memakai locale provider ICU (`datlocprovider = 'i'`), `length(gen_random_bytes(16)) = 16` setelah `CREATE EXTENSION pgcrypto` (OpenSSL), nilai `repeat('a', 10000)` di kolom `COMPRESSION lz4` menghasilkan `pg_column_compression = 'lz4'`, dan `pg_dump --compress=zstd -f <file>` berhasil. Untuk setiap artefak, gabungan dependency `bundled` dan `own` di inventaris sama persis dengan library non sistem yang ditemukan audit statis.
- **AC-8**: Hasil spike mencatat signature yang teramati pada setiap executable utama (Developer ID dengan authority, ad hoc, atau linker signed) dan apakah tanda tangan ad hoc saat install diperlukan, sesuai `signing_policy` entri. Tanda tangan ulang selalu mencakup semua file Mach-O di salinan, tidak pernah sebagian. Tidak ada langkah yang menghapus quarantine, mengubah setelan Gatekeeper atau sistem, atau memakai `sudo`.
- **AC-9**: Untuk setiap `sha256`, sekali saja (saat file yang dilacak git di `evidence/runtime/` belum memuat hasil quarantine untuk digest itu), clone APFS artefak yang setiap filenya diberi atribut `com.apple.quarantine` dijalankan dari konteks LaunchAgent yang sama, dan hasilnya (`ran`, `blocked`, `killed`, atau `timeout`) tercatat. Run lain mencatat `skipped` beserta folder evidence yang sudah memuat hasilnya. Salinan asli tanpa quarantine tidak tersentuh.
- **AC-10**: `xtask/tests/native_runtime.rs` punya satu test native per lini provider yang menulis record `stackly.evidence/v1` (spec 0002) dengan `covers = ["AC-078", "REQ-005"]`, `kind` native, subject berisi versi teramati (atau versi inventaris untuk `FAIL` sebelum versi terbaca), `sha256`, dan `source: managed`. Record `PASS` membawa `actual` berformat `<provider> <observedVersion> ready in <ms> ms, stopped in <ms> ms`; record `FAIL` berasal dari panic dengan pesan `<langkah>: <alasan>`; entri `blocked`, entri yang belum ada, dan Mac tanpa sesi GUI menjadi `BLOCKED` dengan alasan dan owner. Setiap test memanggil `artifact_dir()` dan menyimpan hasil spike lengkap di sana. `cargo xtask runtime record <report.json>` menyalin record `PASS` dan `FAIL` test itu dari run native yang bersih ke `evidence/runtime/<report.runId>/` (record `BLOCKED` tidak disalin, karena alasan dan owner-nya sudah ada di inventaris, dan record `NOT_RUN` juga tidak), dan menolak run dirty, run fixture, run dengan `context.ci.event` `pull_request`, report dengan `problems` tidak kosong atau `interrupted: true`, report yang `context.sourceCommit` bukan `HEAD` saat perintah dijalankan, record yang `evidencePath` tidak berada di bawah `artifacts/` atau foldernya tidak memuat `spike-result.json`, report tanpa record yang bisa disimpan (`no record to keep`), serta folder tujuan yang sudah ada.
- **AC-11**: Di repo `ojiepermana/stackly-runtimes`, push tag terlindungi `postgresql-<versi>-r<n>` atau `valkey-<versi>-r<n>` dari commit di `main` membangun artefak di runner hosted `macos-26` arm64 dari tarball source dengan checksum yang sudah dicek, mengubah install name ke `@rpath` dan `@loader_path`, menandatangani ad hoc setiap file Mach-O, lulus audit closure dan smoke test di path baru yang berisi spasi, membuat attestation provenance, lalu menerbitkan release immutable berisi `.tar.gz` dan `.sha256` (dibuat sebagai draft, aset diunggah, baru dipublikasikan). Pull request, termasuk dari fork, membangun dan menguji tapi tidak pernah menerbitkan dan tidak pernah mendapat izin `id-token` atau `contents: write`.
- **AC-12**: Artefak buatan maintainer hanya berisi file biasa dan folder (tanpa symlink, setuid, setgid, atau file device), folder `LICENSES/` dengan teks lisensi software itu dan setiap dependency bundel, serta `STACKLY-ARTIFACT.json` yang mencatat source, commit resep, toolchain, deployment target, dan versi dependency. `gh attestation verify --repo ojiepermana/stackly-runtimes` lulus untuk setiap aset, sebagai langkah terakhir job tag.
- **AC-13**: Lini yang tidak punya artefak managed (misalnya rilis tanpa aset macOS arm64, atau `FAIL` yang tidak bisa diperbaiki) dicatat `blocked` dengan alasan, owner, dan tanggal. Untuk ClickHouse, aturan fallback dijalankan dulu: rilis LTS terbaru yang membawa aset macOS aarch64, lalu stable terbaru; baru `blocked` kalau keduanya tidak ada.
- **AC-14**: Workflow mingguan menjalankan `cargo xtask runtime watch`, membandingkan versi yang dipin dengan patch terbaru di lini yang sama untuk keenam provider dan dependency bundel (OpenSSL, ICU, lz4, zstd), lalu membuka atau memperbarui satu issue yang mendaftar versi baru. Workflow ini tidak pernah mengubah inventaris atau menerbitkan apa pun.
- **AC-15**: `native.yml` (spec 0002) menjalankan test native runtime di setiap trigger workflow itu, dengan `timeout-minutes: 90`, `cargo xtask runtime spike --cleanup` sebelum dan sesudah langkah evidence (yang sesudah dengan `if: always()`), `cargo xtask runtime spike --prune-cache` hanya di trigger `schedule`, dan cache unduhan yang menetap di Mac self hosted (urutan lengkap di spec 0002, bagian Job CI). PR dari fork tidak pernah sampai ke Mac itu. Sampai runner terdaftar dan variable repo `SELF_HOSTED_MACOS27` bernilai `true`, kriteria ini `BLOCKED` dan bukti G0 datang dari run lokal di Mac uji.

## Decision

**Chosen option**: Option 1: Sumber campuran, upstream resmi bila ada dan pipeline maintainer untuk sisanya, dengan inventaris yang dicek mesin dan bukti dari test native.

Binary diunduh langsung dari upstream dengan digest yang dipin kalau upstream menerbitkan build macOS arm64 resmi (MySQL, NATS, ClickHouse, Traefik); PostgreSQL dan Valkey dibangun pipeline Stackly di repo terpisah dengan attestation provenance. OQ-003 terjawab: pipeline artefak buatan maintainer diperlukan, untuk PostgreSQL dan Valkey.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `meson` (`mohitmishra786/low-level-dev-skills`, `.claude/skills/meson/`) · `signing-entitlements` (`openai/plugins`, `.claude/skills/signing-entitlements/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya skill meson menganjurkan wrap untuk dependency, sedangkan resep di sini membangun dependency sendiri dengan versi dan checksum yang dipin.

## Feature design

### Sumber runtime per provider

Versi adalah patch terbaru per 17 Oktober 2026 dari cek lanskap; nilai yang mengikat adalah yang tercatat di inventaris saat entri ditulis (patch terbaru di lini itu pada hari itu).

| Lini | Versi saat ini | Origin | Distribusi | Format | Checksum upstream | Signing policy | Lisensi (SPDX, dicek ulang dari file lisensi artefak) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PostgreSQL 17 | 17.11, build r1 | maintainer | `stackly_hosted` | `tar_gz` | `.sha256` source dari postgresql.org | `adhoc_pipeline` | `PostgreSQL` |
| PostgreSQL 18 | 18.6, build r1 | maintainer | `stackly_hosted` | `tar_gz` | sama | `adhoc_pipeline` | `PostgreSQL` |
| MySQL 8.4 LTS | 8.4.11 | upstream | `upstream_download` | `tar_gz` (nama file macOS ARM64 dikonfirmasi saat pin) | `none` (tidak ada file checksum polos); GPG bila ada URL `.asc` mentah | `keep_or_adhoc` | `GPL-2.0-only WITH Universal-FOSS-exception-1.0` |
| MySQL 9.7 LTS | 9.7.2 | upstream | `upstream_download` | `tar_gz` | sama | `keep_or_adhoc` | sama |
| NATS 2.15 | 2.15.1 | upstream | `upstream_download` | `tar_gz` | `SHA256SUMS` | `keep_or_adhoc` | `Apache-2.0` |
| ClickHouse | LTS terbaru dengan aset `clickhouse-macos-aarch64` (26.3, atau 26.8 bila sudah LTS); fallback stable terbaru (26.9) | upstream | `upstream_download` | `binary` | tidak ada untuk aset macOS (`none`) | `keep_or_adhoc` | `Apache-2.0` |
| Valkey 9.1 | 9.1.2, build r1 | maintainer | `stackly_hosted` | `tar_gz` | daftar `valkey-hashes` bila memuat versi itu, selain itu `none` | `adhoc_pipeline` | `BSD-3-Clause` |
| Traefik 3.7 | 3.7.15 | upstream | `upstream_download` | `tar_gz` (tanpa folder puncak) | `checksums.txt` | `keep_or_adhoc` | `MIT` |

Closure dependency:

- **PostgreSQL**: bundel OpenSSL lini 3.5 LTS, ICU rilis terbaru (data di dalam `libicudata`), lz4, dan zstd sebagai dylib di `lib/` dengan install name `@rpath`; dari sistem macOS: `libz`, `libxml2`, `libedit`, fungsi uuid di `libSystem`. Library buatan source PostgreSQL sendiri (misalnya `libpq`, `libecpg`) dicatat sebagai `own`. Semua modul contrib ikut. Di luar build: GSSAPI, LDAP, PAM, Bonjour, NLS, JIT LLVM, PL/Perl, PL/Python, PL/Tcl, dan extension pihak ketiga.
- **Valkey**: `BUILD_TLS=no`, hanya library sistem.
- **NATS, Traefik**: binary Go, hanya library sistem (dikonfirmasi audit).
- **MySQL, ClickHouse**: library dari source upstream itu sendiri dicatat `own`; library pihak ketiga yang dibawa upstream di dalam artefak (misalnya OpenSSL di tarball MySQL) dicatat `bundled` dengan versi dan lisensinya, satu entri per file dylib.

Signing policy:

- `keep_or_adhoc`: setiap file Mach-O di salinan hasil ekstraksi dicek dengan `codesign --verify --strict`. Kalau semuanya lulus, signature upstream dipakai apa adanya. Kalau satu saja tidak ada atau rusak, semua file Mach-O di salinan itu ditandatangani ulang dengan `codesign --force --sign -` tanpa `--preserve-metadata` (hardened runtime dan library validation ikut lepas bersamaan, jadi binary tidak menolak library miliknya sendiri), dan hasil spike mencatat `adhocSignedAtInstall: true`. File di cache tidak pernah diubah.
- `adhoc_pipeline`: pipeline menandatangani ad hoc setiap file Mach-O setelah `install_name_tool`, tanpa hardened runtime (supaya PostgreSQL bisa memuat extension). Developer ID dan notarization menyusul di fitur 60 setelah identity OQ-012 tersedia.

### Data model sketch

**`runtimes/inventory.toml`** (field TOML snake_case; tipe Rust tooling di `xtask/src/runtime/inventory.rs`, `// provisional: F9` sampai fitur 9 dan 18 memutuskan cara catalog membaca data ini):

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
| `line` | string | ya | `17`, `8.4`, `2.15`, `26.3`, dan seterusnya; `version` diawali `<line>.` |
| `version` | string | ya | exact, pola sama dengan subject spec 0002: `^[0-9]+(\.[0-9]+)+([+-][0-9A-Za-z.]+)?$`; ClickHouse memakai empat komponen penuh |
| `build` | int | maintainer saja | mulai 1, naik setiap build ulang versi yang sama |
| `os`, `arch` | enum | ya | `macos`, `aarch64` |
| `min_os` | string | `available` | versi minimum dari `LC_BUILD_VERSION` executable utama |
| `status` | enum | ya | `available` atau `blocked` |
| `blocked_reason`, `blocked_owner`, `blocked_since` | string, string `@handle`, tanggal | `blocked` | owner default `@ojiepermana` (runtime maintainer) |
| `origin` | enum | ya | `upstream` atau `maintainer` |
| `distribution` | enum | ya | `upstream_download` (origin upstream) atau `stackly_hosted` (origin maintainer) |
| `format` | enum | `available` | `tar_gz` atau `binary` (maintainer selalu `tar_gz`) |
| `urls` | array string https | `available` | URL kandidat berurutan; digest yang menentukan identitas |
| `sha256`, `size` | hex 64 huruf kecil, int byte | `available` | dari unduhan nyata |
| `archive_root` | string | tidak | folder puncak di dalam archive. Kalau ada, puncak archive harus tepat folder itu. Kalau tidak ada (misalnya Traefik), archive diekstrak ke folder baru `<id>/` yang menjadi root artefak |
| `upstream_checksum` | `{ kind, url }` | origin upstream | `kind`: `sha256sums`, `checksums_txt`, `sha512`, atau `none` |
| `upstream_signature` | `{ kind, url, key, fingerprint }` | origin upstream | `kind`: `gpg` atau `none`; untuk `gpg`, `key` adalah file kunci publik yang disimpan di repo di `runtimes/keys/` dan `fingerprint` sidik jarinya |
| `source_url`, `source_sha256` | https, hex | origin maintainer | tarball source upstream |
| `recipe` | string | origin maintainer | `ojiepermana/stackly-runtimes@<commit>:recipes/<provider>.sh` |
| `attestation` | https | origin maintainer | URL attestation GitHub |
| `signing_policy` | enum | `available` | `keep_or_adhoc` (upstream) atau `adhoc_pipeline` (maintainer) |
| `license`, `license_url` | SPDX, https | `available` | |
| `executables` | array path relatif | `available` | path di dalam root artefak yang dijalankan spike; executable utama di urutan pertama |

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
- Alur dua commit dalam satu PR: (1) commit perubahan inventaris; (2) di tree bersih itu (tanpa perubahan dan tanpa file untracked yang tidak di-gitignore), jalankan `cargo xtask evidence run --kind native -- cargo test -p xtask --test native_runtime -- --ignored` di Mac uji macOS 27, lalu `cargo xtask runtime record <path>` dengan path dari baris `evidence report:` yang dicetak wrapper; (3) commit `evidence/runtime/<runId>/`; (4) push keduanya. `runtime check` di CI baru lulus setelah commit kedua ada.
- `evidence/runtime/` adalah bukti G0. Bukti rilis untuk fitur 65 tetap berasal dari run `native.yml` di `main` (push, schedule, atau dispatch), sesuai spec 0002.

### Langkah spike per provider

Port dialokasikan worker tepat sebelum setiap start lewat helper `xtask/src/runtime/ports.rs` (bind `127.0.0.1:0`, lalu lepas). Socket PostgreSQL dan MySQL ada di folder pendek `/tmp/sk-<8 hex>` berizin 0700 (path socket Unix dibatasi sekitar 104 byte). `cwd` setiap proses adalah folder run artefak itu, dan semua proses server dijalankan langsung sebagai child di process group sendiri, tidak lewat pembungkus yang melakukan `setsid` atau daemonize. Langkah ini `// provisional: F9` sampai manifest provider memegang lifecycle.

| Provider | Init | Start | Ready (≤ 120 detik) | Versi teramati, normalisasi | Stop (≤ 60 detik) | Probe tambahan |
| --- | --- | --- | --- | --- | --- | --- |
| PostgreSQL | `initdb -D <data> -U stackly --auth-local=trust --auth-host=scram-sha-256 -E UTF8 --locale=C --locale-provider=icu --icu-locale=und` | `bin/postgres -D <data> -k <sock> -p <port> -c listen_addresses=''` | `psql -X -h <sock> -p <port> -U stackly -d postgres -Atc 'select 1'` | `show server_version`, ambil `^[0-9]+\.[0-9]+` | SIGINT ke `postgres` (fast shutdown) | probe AC-7 |
| MySQL | `mysqld --no-defaults --initialize-insecure --basedir=<root> --datadir=<data> --log-error=<run>/init.err` | `mysqld --no-defaults --basedir --datadir --socket=<sock>/m.sock --skip-networking --mysqlx=OFF --pid-file=<run>/mysqld.pid --log-error=<run>/mysqld.err` | `mysqladmin --no-defaults -u root --socket=<sock>/m.sock ping` | `select version()`, ambil `^[0-9]+\.[0-9]+\.[0-9]+` | `mysqladmin --no-defaults -u root --socket=... shutdown` | tidak ada |
| NATS | tulis `<run>/nats.conf`: `listen` dan `http` di 127.0.0.1 dengan dua port, `jetstream { store_dir }` di folder run | `nats-server -c <run>/nats.conf` | `GET /healthz` | `nats-server --version`, ambil `v([0-9.]+)` | SIGTERM | JetStream aktif di `/healthz?js-enabled-only=true` |
| ClickHouse | tulis `<run>/config.xml` (hanya `listen_host` 127.0.0.1, `tcp_port`, `http_port`, `path`, `tmp_path`, `user_files_path`, logger ke folder run; tanpa port MySQL, PostgreSQL, interserver, atau gRPC) dan `users.xml` (user `default` tanpa password, hanya dari localhost) | `clickhouse server --config-file=<run>/config.xml` | `GET /ping` | `SELECT version()` lewat HTTP, empat komponen penuh | SIGTERM | audit listener (PRD 32.2) |
| Valkey | tidak ada | `valkey-server --bind 127.0.0.1 --port <p> --dir <data> --save '' --appendonly no --daemonize no` | `valkey-cli -p <p> PING` | `valkey_version` dari `INFO server` | `valkey-cli -p <p> shutdown nosave` | tidak ada |
| Traefik | tulis folder file provider kosong di folder run | `traefik --entryPoints.web.address=127.0.0.1:<p> --ping=true --ping.entryPoint=web --providers.file.directory=<run>/dynamic --api=false --global.checknewversion=false --global.sendanonymoususage=false` | `GET /ping` | `traefik version`, baris `Version:` | SIGTERM | tidak ada |

`trust` di PostgreSQL spike aman karena satu satunya jalan masuk adalah socket di folder 0700 milik user yang sama, setara dengan peer auth; user database `stackly` menjaga nama user OS keluar dari evidence. Ini konfigurasi spike, bukan default instance (fitur 19).

Urutan langkah worker: hitung ulang hash file cache terhadap plan, ekstrak dengan `/usr/bin/tar` (atau salin binary dan beri mode 0755), laporan archive (symlink internal dan yang keluar, setuid, file device), audit statis, cek dan tanda tangan sesuai `signing_policy`, ukur `min_os`, init, start, ready, baca versi, probe, audit runtime (`lsof` library dan listener), stop, lalu quarantine bila plan memintanya.

### Mekanisme LaunchAgent dan pembersihan

- **Lock**: driver memegang lock eksklusif `<cache>/spike.lock` (`std::fs::File::lock`) selama run, sehingga delapan test yang berjalan paralel dan pemanggilan manual tidak pernah tumpang tindih. `--cleanup` dan `--prune-cache` mengambil lock yang sama sebelum mematikan atau menghapus apa pun, menunggu paling lama 20 menit, lalu keluar dengan kode 1 dan pesan `spike in progress`. Lock dilepas otomatis saat prosesnya mati, jadi sisa run yang terputus tidak menahannya.
- **Driver**: (1) jalankan rutin pembersihan; (2) unduh ke cache dan cek `sha256` serta `size` sebelum jam driver mulai; (3) buat root `<TMPDIR>/stackly-runtime-spike-<runId>/` dengan file penanda pemilik; (4) putuskan quarantine dengan mencari hasil quarantine untuk `sha256` ini hanya di file yang dilacak git (`git ls-files 'evidence/runtime/*/artifacts/*/spike-result.json'`); (5) tulis plan JSON (entri inventaris, path cache, root, batas waktu, flag quarantine) dan plist ke root; (6) `launchctl bootstrap gui/<uid> <plist>`; kalau gagal karena tidak ada sesi GUI, driver keluar dengan kode 2 dan pesan `no gui login session` sehingga test memanggil `blocked(...)`; (7) tunggu, polling setiap 1 detik, sampai `<root>/result.json` muncul (ditulis atomik) atau `launchctl print gui/<uid>/<label>` menunjukkan worker berhenti; worker berhenti tanpa hasil berarti `FAIL worker exited <kode>`; batas 15 menit di luar unduhan; (8) `launchctl bootout`, lalu rutin pembersihan untuk root miliknya sendiri, di setiap jalur keluar (guard `Drop` dan handler `ctrlc` yang sudah ditambah spec 0002).
- **Plist**: `Label` tetap `dev.stackly.xtask.runtime-spike` (lock menjamin hanya satu, dan daftar Login Items tidak bertambah setiap run), `ProgramArguments` `[<std::env::current_exe()>, "runtime", "spike-worker", "--plan", <path absolut plan>]` (tidak pernah `cargo`), `RunAtLoad` true, `KeepAlive` false, `WorkingDirectory` root, `StandardOutPath` dan `StandardErrorPath` di root, `ExitTimeOut` 30, dan `EnvironmentVariables` minimal (`PATH`, `HOME` dan `TMPDIR` di root, `LANG`). launchd tidak mewarisi environment test, jadi semua yang worker butuhkan ada di plan.
- **Worker**: menolak jalan kalau parent bukan launchd (PID 1); `controllingTty` bernilai false kalau membuka `/dev/tty` gagal dengan ENXIO; handler SIGTERM (`ctrlc`, fitur `termination`) mematikan process group provider lalu menulis hasil `FAIL`; batas waktu sendiri 12 menit.
- **Rutin pembersihan** (juga `runtime spike --cleanup`): `pgrep -u <uid> -f` untuk proses yang command line memuat `stackly-runtime-spike-` atau `/tmp/sk-`, kirim TERM, tunggu 5 detik, lalu KILL; `launchctl bootout gui/<uid>/dev.stackly.xtask.runtime-spike` bila masih terdaftar; hapus folder `stackly-runtime-spike-*` di temp dir yang membawa penanda pemilik user ini, dan `/tmp/sk-*` milik user ini. Tanpa domain GUI, langkah bootout dilewati dan `--cleanup` tetap keluar dengan kode 0. Rutin ini tidak pernah menyentuh cache unduhan. Perintah terpisah `runtime spike --prune-cache` membuang file cache yang digest-nya tidak ada di inventaris checkout itu; di `native.yml` ia hanya jalan di trigger `schedule`, yang selalu memakai `main`, jadi PR dengan inventaris lain tidak menghapus unduhan milik `main`.
- **Quarantine**: clone dengan `cp -Rc` (clone APFS, instan walau artefak besar), beri `xattr -w com.apple.quarantine "0083;<epoch hex>;StacklySpike;<uuid>"` pada setiap file clone, jalankan perintah versi executable utama dengan batas 30 detik. Untuk binary ad hoc atau tanpa signature, hasil yang diharapkan `killed`. Di domain GUI, dialog Gatekeeper bisa tertinggal di layar; karena itu probe ini hanya jalan sekali per `sha256`.
- **Audit statis**: file Mach-O dikenali dari magic number; `lipo -archs` wajib memuat `arm64`; setiap referensi `otool -L` dan setiap `LC_RPATH` diselesaikan ke path nyata. `@rpath/`, `@loader_path/`, dan `@executable_path/` harus berujung ke file di dalam root artefak.
- **Audit runtime**: `lsof -nP -p <pid> -Ffn`, hanya entri FD `txt` yang berupa file Mach-O, path dibuat canonical (realpath), lalu dicocokkan dengan allowlist AC-6.

### API surface

Argumen diparse dengan tangan seperti task `xtask` lain.

| Perintah | Tempat jalan | Input utama | Output utama | Izin | Error utama |
| --- | --- | --- | --- | --- | --- |
| `cargo xtask runtime check` | OS apa pun, job `linux` di setiap PR | `runtimes/inventory.toml`, `evidence/runtime/*/report.json` | exit 0, atau satu baris `<id>: <aturan>` per pelanggaran lalu exit 1 | read only | TOML tidak valid, pelanggaran invariant, evidence `PASS` tidak ada |
| `cargo xtask runtime pin --url <https>... [--executable <path relatif>] [--sha256-url <https>]` | OS apa pun dengan `curl` | satu atau lebih URL kandidat; executable untuk `min_os`; file `.sha256` release untuk entri maintainer | potongan TOML berisi `urls`, `sha256`, `size`, dan `min_os` (di macOS) | jaringan | URL bukan https, unduhan gagal atau lebih dari 4 GiB, URL kandidat menghasilkan digest berbeda, digest tidak sama dengan `.sha256` |
| `cargo xtask runtime verify [--id <id>]` | OS apa pun dengan `curl` | inventaris | `OK`, `FAIL <alasan>`, atau `SKIP gpg` per artefak; checksum upstream dicocokkan per `kind`, GPG hanya kalau `gpg` ada di PATH | jaringan | digest, ukuran, checksum, atau signature tidak cocok; URL tidak bisa dijangkau |
| `cargo xtask runtime spike --id <id> --result <file>` | macOS saja | inventaris, cache, `evidence/runtime/` | file hasil spike; exit 0 `PASS`, 1 `FAIL`, 2 `BLOCKED` | domain launchd `gui` milik user sendiri, tanpa `sudo` | bukan macOS, tanpa sesi GUI, bootstrap gagal, melewati batas waktu |
| `cargo xtask runtime spike --cleanup` | macOS | tidak ada | proses, agent, dan folder sisa dibersihkan (lihat Rutin pembersihan); cache tidak disentuh; exit 0 walau domain GUI tidak ada | domain user sendiri, lock spike | lock tidak didapat dalam 20 menit (`spike in progress`) |
| `cargo xtask runtime spike --prune-cache` | macOS | inventaris | file cache yang digest-nya tidak ada di inventaris dihapus | folder cache, lock spike | lock tidak didapat dalam 20 menit (`spike in progress`) |
| `cargo xtask runtime spike-worker --plan <file>` | internal, hanya dari launchd | plan JSON dari driver | `result.json` (ditulis atomik) | menolak jalan kalau parent bukan launchd (PID 1) | batas waktu worker, SIGTERM saat bootout |
| `cargo xtask runtime record <report.json>` | OS apa pun | report dari `cargo xtask evidence run --kind native` | `evidence/runtime/<report.runId>/` berisi report yang difilter ke record `PASS` dan `FAIL`, `output.log` yang disanitasi, dan `artifacts/<recordId>/spike-result.json` | menulis repo | report dirty, kind fixture, `ci.event` `pull_request`, `problems` tidak kosong, `interrupted: true`, `context.sourceCommit` bukan `HEAD`, `evidencePath` di luar `artifacts/` atau tanpa `spike-result.json`, tidak ada record yang bisa disimpan (`no record to keep`), folder tujuan sudah ada, `output.log contains user path` |
| `cargo xtask runtime watch [--format json\|markdown]` | job mingguan di Linux | inventaris, `GITHUB_TOKEN` opsional | daftar versi baru per lini dan per dependency bundel (dikelompokkan per `source_url`) | baca API GitHub | rate limit, upstream tidak menjawab |

Test native di `xtask/tests/native_runtime.rs` (memakai `env!("CARGO_BIN_EXE_xtask")` untuk menjalankan driver), satu fungsi `#[test] #[ignore = "native: needs network, macOS 27, and provider artifacts"]` per lini: `postgresql_17_runs_from_managed_artifact`, `postgresql_18_runs_from_managed_artifact`, `mysql_8_4_runs_from_managed_artifact`, `mysql_9_7_runs_from_managed_artifact`, `nats_2_15_runs_from_managed_artifact`, `clickhouse_runs_from_managed_artifact`, `valkey_9_1_runs_from_managed_artifact`, dan `traefik_3_7_runs_from_managed_artifact`. Setiap test:

- membuat guard lewat `evidence!(covers = ["AC-078", "REQ-005"], kind = Kind::Native, expected = ...)` sebagai pernyataan pertama di fungsinya sendiri, lalu meneruskan `&Evidence` ke helper bersama (aturan `evidence check` spec 0002 mencari makro itu di setiap fungsi `#[ignore]`);
- mencari entri lewat `(provider, line)`, kecuali ClickHouse yang dicari lewat provider saja;
- memanggil `artifact_dir()` lebih dulu dan menyuruh driver menulis hasil spike ke sana;
- memanggil `blocked(reason, owner)` untuk entri `blocked`, untuk entri yang belum ada (`no inventory entry for <provider> <line>`, owner `@ojiepermana`), dan untuk exit code driver 2;
- selain itu memanggil `subject(...)` dengan `observedVersion` dan `sha256`, atau dengan `version` dari inventaris kalau spike `FAIL` sebelum versi terbaca (satu satunya pengecualian dari aturan versi teramati di spec 0002, supaya record `FAIL` tetap terhubung ke digest-nya dan menggantikan `PASS` lama di aturan 8), lalu `pass_with("<provider> <observedVersion> ready in <ms> ms, stopped in <ms> ms")` kalau hasilnya `PASS`, atau panic dengan `<langkah>: <alasan>` kalau `FAIL`.

Run lokal: `cargo xtask evidence run --kind native -- cargo test -p xtask --test native_runtime -- --ignored`.

Workflow:

| Workflow | Repo | Trigger | Runner | Izin | Isi |
| --- | --- | --- | --- | --- | --- |
| `ci.yml` job `linux` | Stackly | yang sudah ada | `ubuntu-24.04` | yang sudah ada | tambah langkah `cargo xtask runtime check` |
| `native.yml` (spec 0002) | Stackly | yang sudah ada | self hosted `macos-27` | `contents: read` | `timeout-minutes: 90`; `cargo xtask runtime spike --cleanup` sebelum langkah evidence dan sekali lagi dengan `if: always()` sesudahnya; `--prune-cache` hanya di trigger `schedule`; cache unduhan menetap di `~/Library/Caches/dev.stackly.xtask/runtime/` tanpa `actions/cache`; urutan lengkap di spec 0002 |
| `runtime-watch.yml` (baru) | Stackly | `schedule` mingguan, `workflow_dispatch` | `ubuntu-24.04` | `contents: read`, `issues: write` | `cargo xtask runtime watch --format markdown`, lalu buat atau perbarui issue berjudul `Runtime updates available` lewat `gh` |
| `build.yml` | `stackly-runtimes` | `pull_request` | hosted `macos-26` | `contents: read` | build, audit, smoke, tanpa publish |
| `build.yml` | `stackly-runtimes` | push tag `postgresql-*-r*`, `valkey-*-r*` | hosted `macos-26` | `contents: write`, `id-token: write`, `attestations: write` | checkout `fetch-depth: 0`, gagal kalau `git merge-base --is-ancestor $GITHUB_SHA origin/main` tidak lulus; build, audit, smoke, attestation; `gh release create --draft --verify-tag`, unggah `.tar.gz` dan `.sha256`, `gh release edit --draft=false`; terakhir `gh attestation verify` (dengan `GH_TOKEN`) untuk setiap aset |

Semua action dipin ke commit SHA, sama dengan spec 0002.

### Resep pipeline `stackly-runtimes`

```text
stackly-runtimes/
├── recipes/
│   ├── deps/openssl.sh, icu.sh, lz4.sh, zstd.sh   # versi dan sha256 source dipin di file
│   ├── postgresql.sh
│   └── valkey.sh
├── lib/common.sh            # relocate, sign, audit, package, smoke
├── requirements.txt         # meson, ninja, pkgconf dari PyPI, dipin dengan hash
├── .github/workflows/build.yml
├── LICENSE
└── README.md
```

- Bash dengan `set -euo pipefail`. `PATH` saat build hanya venv Python, `/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`, dan Xcode, sehingga tool Homebrew bawaan runner tidak terpakai. `PKG_CONFIG_LIBDIR` menunjuk prefix dependency buatan sendiri; `libxml2` dan `zlib` sistem tetap harus ditemukan (lewat folder `pkgconfig` SDK bila ada, atau deteksi sistem meson), dan folder `pkgconfig` Homebrew tidak pernah ikut.
- Xcode dipin di workflow, `MACOSX_DEPLOYMENT_TARGET=26.0`, `LDFLAGS` memuat `-Wl,-headerpad_max_install_names` (ruang untuk `install_name_tool`), dan `min_os` artefak `26.0`.
- Dependency dibangun ke prefix staging terpisah; hanya dylib dan file lisensinya yang masuk paket (tanpa `bin/openssl`, header, `.a`, atau `.pc`). OpenSSL: `./Configure darwin64-arm64-cc shared no-tests no-docs no-module --openssldir=/etc/ssl` (macOS punya `/etc/ssl/cert.pem`, sehingga OPENSSLDIR tetap berarti setelah relokasi). ICU: `--with-data-packaging=library --disable-samples --disable-tests`, sehingga data ICU ada di dalam `libicudata` dan ikut pindah. lz4 dan zstd: dylib saja.
- PostgreSQL dibangun dengan meson dari tarball resmi yang dicek terhadap `.sha256` postgresql.org, dengan prefix staging dengan path tanpa kata `postgres` atau `pgsql` (misalnya `$RUNNER_TEMP/stage/stackly-pg`), sehingga layout tetap `lib/postgresql/` dan `share/postgresql/` dan lookup relatif PostgreSQL dari `bin/` bekerja setelah dipindah. Opsi: `-Dlibdir=lib -Ddocs=disabled -Dssl=openssl -Dicu=enabled -Dlz4=enabled -Dzstd=enabled -Dlibxml=enabled -Dzlib=enabled -Dreadline=enabled -Dlibedit_preferred=true -Duuid=e2fs`, dan `disabled` untuk `gssapi`, `ldap`, `pam`, `bonjour`, `nls`, `llvm`, `plperl`, `plpython`, `pltcl`, `libxslt`.
- Valkey: `make BUILD_TLS=no USE_SYSTEMD=no`, lalu install. Symlink kompatibilitas `redis-*` dan `valkey-sentinel` dibuang; symlink internal lain diganti salinan file.
- Relokasi: install name dylib menjadi `@rpath/<nama>`, binary di `bin/` mendapat rpath `@loader_path/../lib`, modul di `lib/postgresql/` mendapat `@loader_path/..`, dan setiap `LC_RPATH` absolut dihapus. Lalu setiap Mach-O ditandatangani ad hoc.
- Audit menolak referensi apa pun selain `@rpath/`, `@loader_path/`, `@executable_path/`, `/usr/lib/`, dan `/System/Library/`, serta `LC_RPATH` absolut apa pun.
- Smoke menyalin hasil ke path baru yang berisi spasi, lalu menjalankan langkah spike untuk provider itu (PostgreSQL termasuk probe closure).
- Paket: `<provider>-<version>-r<build>-macos-aarch64.tar.gz` dengan folder puncak bernama sama, berisi `bin/`, `lib/`, `share/`, `include/` (PostgreSQL), `LICENSES/`, dan `STACKLY-ARTIFACT.json` (`stackly.runtime-artifact/v1`: provider, versi, build, source URL dan sha256, repo dan commit resep, toolchain `{ xcode, sdk, clang, meson, ninja, pkgconf }`, deployment target, dependency `[{ name, version, license, sourceUrl, sourceSha256 }]`, `builtAt`, image runner). File `.sha256` berisi satu baris `<hex>  <nama file>`.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| spike | URL, `sha256`, `size`, `archive_root`, `executables`, `signing_policy` | entri inventaris |
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
| spike | `quarantine.outcome` | hasil spawn clone berquarantine: exit 0 `ran`, exit lain atau spawn ditolak `blocked`, mati karena sinyal `killed`, lewat 30 s `timeout`, tidak dijalankan `skipped` |
| test native | `covers`, `expected` | konstanta di test: `["AC-078", "REQ-005"]` dan satu kalimat perilaku |
| test native | `subjects` | `observedVersion` (atau `version` inventaris untuk `FAIL` sebelum versi terbaca) dan `sha256` dari hasil spike, `source: managed` |
| test native | alasan dan owner `BLOCKED` | `blocked_reason` dan `blocked_owner` entri, atau konstanta untuk entri yang belum ada dan tanpa sesi GUI (owner `@ojiepermana`) |
| test native | context, commit, host | wrapper spec 0002 |
| check | record terbaru per `sha256` | `finishedAt` terbaru di antara record `evidence/runtime/*/report.json` dengan subject yang memuat digest itu |
| check | daftar lini wajib (aturan 9) | konstanta di xtask: `postgresql` 17 dan 18, `mysql` 8.4 dan 9.7, `nats` 2.15, `clickhouse` (lini apa pun), `valkey` 9.1, `traefik` 3.7 |
| pin | `sha256`, `size`, `min_os` | byte yang benar benar diunduh; `vtool` pada `--executable` (atau satu satunya file untuk format `binary`) |
| watch | patch terbaru per lini | API GitHub: `postgres/postgres` tag `REL_17_*` dan `REL_18_*`, `mysql/mysql-server` tag `mysql-8.4.*` dan `mysql-9.7.*` (fallback halaman unduhan MySQL kalau tag tidak lagi diterbitkan), rilis `nats-io/nats-server` `v2.15.*`, `ClickHouse/ClickHouse` di lini terpilih, `valkey-io/valkey` `9.1.*`, `traefik/traefik` `v3.7.*`, `openssl/openssl` `openssl-3.5.*`, `unicode-org/icu`, `lz4/lz4`, `facebook/zstd` |
| watch | issue | `gh issue list/create/edit` di workflow, satu issue berjudul tetap |
| pipeline | nomor build | tag `<provider>-<version>-r<n>` |
| pipeline | source dan checksum | file resep (dipin), dicek terhadap `.sha256` postgresql.org atau `valkey-hashes` |
| pipeline | toolchain | `xcodebuild -version`, `xcrun --show-sdk-version`, `clang --version`, versi tool dari `requirements.txt` |
| cache | lokasi | macOS `~/Library/Caches/dev.stackly.xtask/runtime/<sha256>`; Linux `$XDG_CACHE_HOME` atau `~/.cache`, lalu `stackly-xtask/runtime/<sha256>` |

### Key invariants

Aturan yang ditegakkan `cargo xtask runtime check`:

1. `schema_version` bernilai 1; `id` unik dan sama dengan pola yang diturunkan dari `provider`, `version`, `build`, `os`, dan `arch`; `(provider, line, os, arch)` unik.
2. `version` exact dan diawali `<line>.`; `os = "macos"` dan `arch = "aarch64"`.
3. `available` mewajibkan `urls` (semua `https://`), `sha256` (64 hex huruf kecil), `size` > 0, `min_os`, `format`, `license`, `license_url`, `signing_policy`, dan `executables` (path relatif tanpa `..`).
4. `blocked` mewajibkan `blocked_reason`, `blocked_owner` (diawali `@`), dan `blocked_since`.
5. `origin = "maintainer"` jika dan hanya jika ada `build` (≥ 1), `source_url`, `source_sha256`, `recipe`, `attestation`, `distribution = "stackly_hosted"`, `signing_policy = "adhoc_pipeline"`, dan `format = "tar_gz"`. `origin = "upstream"` berarti tanpa `build`, dengan `distribution = "upstream_download"`, `signing_policy = "keep_or_adhoc"`, `upstream_checksum`, dan `upstream_signature`; `upstream_signature` berjenis `gpg` mewajibkan `url`, `key` yang menunjuk file yang ada di `runtimes/keys/`, dan `fingerprint`.
6. Dependency: `name` unik per artefak; `bundled` mewajibkan `version`, `license`, `source_url`, dan `install_name` diawali `@rpath/`, `@loader_path/`, atau `@executable_path/`; `own` mewajibkan `install_name` dengan awalan yang sama; `system` mewajibkan `install_name` di bawah `/usr/lib/` atau `/System/Library/`.
7. Setiap file `evidence/runtime/*/report.json` bisa didecode sebagai `stackly.evidence-report/v1`; nama foldernya sama dengan `runId` report itu; `summary` sama dengan hitungan ulang dari `records`; setiap record lolos `EvidenceRecord::validate` dan berstatus `PASS` atau `FAIL`; dan setiap `evidencePath` menunjuk folder di folder run itu yang memuat `spike-result.json`.
8. Setiap artefak `available` punya record dengan subject yang `digest` sama dengan `sha256` artefak itu, dan record terbaru tersebut `PASS`, `kind: native`, `sourceDirty: false`, `context.ci` null atau `ci.event` bukan `pull_request`, `host.osVersion` mayor 27, `host.arch` arm64 atau aarch64, dan `subjects[].version` sama dengan `version` artefak.
9. Setiap lini wajib di daftar konstanta punya tepat satu entri. Aturan ini diaktifkan di task 12 Build plan.

Aturan yang dijaga kode spike dan pipeline:

- Tidak ada byte artefak yang diekstrak atau dijalankan sebelum `sha256` dan `size` cocok.
- Spike hanya menulis di root spike miliknya, `/tmp/sk-*` miliknya, dan folder cache; tidak pernah di `~/Library/Application Support/Stackly`, tidak pernah `sudo`, tidak pernah menghapus atribut quarantine, dan tidak pernah mengubah setelan sistem.
- Hanya satu spike berjalan di satu Mac pada satu waktu (lock di folder cache).
- Proses provider berjalan langsung sebagai child di process group sendiri, dengan environment yang dikosongkan, dan group itu dimatikan (`/bin/kill -TERM -- -<pgid>`, lalu `-KILL`) di setiap jalur keluar worker; sisa dari run yang terputus ditemukan rutin pembersihan lewat `pgrep`.
- Plist LaunchAgent tinggal di root spike, tidak pernah di `~/Library/LaunchAgents`.
- File di cache tidak pernah diubah; tanda tangan ad hoc hanya pada salinan hasil ekstraksi, dan selalu untuk semua file Mach-O di salinan itu.
- Hasil spike tidak memuat path root asli, nama user OS, atau hostname.
- Record final dan isi `evidence/runtime/` tidak pernah diedit; run baru berarti folder baru.
- Archive buatan maintainer hanya berisi file biasa dan folder, tanpa `LC_RPATH` absolut.

### Security model

- **Akar kepercayaan digest** adalah `runtimes/inventory.toml` yang masuk lewat PR yang direview. Artefak upstream diunduh lewat HTTPS (`--proto =https --proto-redir =https`) dan dicocokkan dengan digest yang dipin; checksum dan signature upstream (SHA256SUMS, `checksums.txt`, GPG dengan kunci yang disimpan di repo) dicek saat `pin` dan `verify` sebagai lapisan kedua, bukan pengganti.
- **Artefak buatan maintainer** hanya terbit dari push tag terlindungi pada commit di `main` repo `stackly-runtimes`, di runner hosted ephemeral, dengan `GITHUB_TOKEN` dan OIDC untuk attestation. Tidak ada secret jangka panjang. PR dari fork hanya mendapat token read only. Ruleset tag melarang orang selain maintainer membuat, memindahkan, atau menghapus tag rilis, dan release immutable mencegah aset diganti setelah terbit.
- **Mac self hosted** tidak pernah menjalankan PR dari fork (guard dari spec 0002) dan tidak memegang secret signing.
- **Spike menjalankan kode native dengan hak user** di Mac uji. Itu residual risk yang sama dengan PRD bagian 23; digest yang dipin membatasinya ke binary yang sudah direview.
- **Tanpa credential**: PostgreSQL memakai `trust` hanya lewat socket di folder 0700, MySQL `--initialize-insecure` dengan `--skip-networking` sehingga hanya bisa dijangkau lewat socket privat, provider lain hanya di loopback selama beberapa detik, dan Traefik tidak menghubungi server versi atau telemetri. Konfigurasi ini khusus spike, bukan default instance (fitur 19 dan seterusnya).
- **Privasi evidence**: hasil spike memakai `$ROOT` untuk path, `output.log` yang disalin `runtime record` disanitasi dengan aturan yang sama ditambah `$WORKSPACE` dan `~`, user database `stackly`, dan nama file pid serta log MySQL yang eksplisit; context host dari spec 0002 tidak memuat nama host atau nama user.
- **Lisensi (AC-078, RISK-12)**: hanya PostgreSQL, Valkey, dan dependency bundel PostgreSQL yang didistribusikan ulang oleh Stackly, masing masing dengan teks lisensi di `LICENSES/`. Artefak upstream diunduh pengguna langsung dari upstream. Persetujuan distribusi tetap milik release owner (Follow-up).

### Configuration required

- `GITHUB_TOKEN`: opsional untuk `cargo xtask runtime watch` supaya tidak terkena rate limit API GitHub tanpa autentikasi; di Actions diisi otomatis.
- Prasyarat yang kamu siapkan sebelum milestone 3: repo publik `ojiepermana/stackly-runtimes` dengan release immutable aktif, ruleset tag untuk `postgresql-*-r*` dan `valkey-*-r*`, dan izin default Actions read only.
- Prasyarat AC-15 (sudah tercatat di spec 0002): Mac self hosted terdaftar dengan label `self-hosted, macOS, ARM64, macos-27`, dengan sesi login GUI aktif, dan variable repo `SELF_HOSTED_MACOS27` bernilai `true`.

### Critical test scenarios

- Happy path: `nats_2_15_runs_from_managed_artifact` lewat wrapper native mengunduh, mencocokkan digest, menjalankan NATS dari LaunchAgent, lulus readiness dan stop, lalu menulis record `PASS` yang disalin `runtime record` dan diterima `runtime check`. Memverifikasi **AC-3**, **AC-4**, **AC-5**, **AC-10**.
- PostgreSQL: kedua test PostgreSQL lulus termasuk probe ICU, pgcrypto, lz4, dan zstd di path berisi spasi dan `Ü`. Memverifikasi **AC-5**, **AC-7**.
- Digest salah: unit test fungsi gerbang digest dengan file yang diubah satu byte tidak pernah memanggil langkah ekstrak; entri cache yang rusak dihapus dan diunduh ulang. Memverifikasi **AC-3**.
- Library asing: fixture Mach-O kecil yang dibangun di test lalu diubah dengan `install_name_tool -change` ke `/opt/homebrew/lib/libfoo.dylib` ditolak audit statis, dengan path itu di hasil. Memverifikasi **AC-6**.
- Terputus: worker yang dihentikan di tengah run meninggalkan agent, proses provider, dan root; `runtime spike --cleanup` (atau run berikutnya) mematikan proses itu lewat `pgrep`, mencabut agent, dan menghapus root. Memverifikasi **AC-4**.
- Paralel: delapan test native dijalankan libtest secara paralel tetap menghasilkan delapan spike berurutan tanpa bentrok port atau label. Memverifikasi **AC-4**.
- Aturan inventaris: unit test `runtime check` dengan inventaris fixture, satu per aturan (field `available` hilang, URL `http://`, `blocked` tanpa owner, maintainer tanpa attestation, dependency `bundled` tanpa lisensi, `available` tanpa `PASS`, record terbaru `FAIL`, record dari `pull_request`, versi subject berbeda, folder `evidence/runtime/` yang namanya bukan `runId`, `summary` yang salah, record `BLOCKED` atau `NOT_RUN` di report yang di-commit, `evidencePath` tanpa `spike-result.json`). Memverifikasi **AC-2**, **AC-13**.
- Penyalinan bukti: unit test `runtime record` dengan report fixture, satu per penolakan (dirty, kind fixture, `pull_request`, `problems` tidak kosong, `interrupted`, commit bukan `HEAD`, folder tanpa `spike-result.json`, folder tujuan sudah ada, tanpa record yang bisa disimpan, `output.log` yang masih memuat nama user), plus satu yang membuang record `BLOCKED` dan `NOT_RUN` lalu menghitung ulang `summary`, dengan isi record `PASS` dan `FAIL` yang disalin tetap sama byte demi byte, dan satu yang memastikan path workspace, home, dan root spike di `output.log` sudah diganti. Memverifikasi **AC-10**.
- Lock: `--cleanup` dan `--prune-cache` yang dijalankan saat spike berjalan menunggu lock, lalu keluar dengan `spike in progress` tanpa mematikan proses spike atau menghapus file cache yang dipakainya. Memverifikasi **AC-4**.
- Cache: `--prune-cache` menghapus file cache yang digest-nya tidak ada di inventaris dan tidak menyentuh `spike.lock`; `--cleanup` tidak menyentuh cache dan keluar 0 tanpa domain GUI. Memverifikasi **AC-4**, **AC-15**.
- Quarantine: run pertama untuk satu digest mencatat `quarantine.outcome` selain `skipped`, sementara salinan asli tetap `PASS`; run berikutnya mencatat `skipped`. Memverifikasi **AC-9**.
- Izin pipeline: PR dari fork di `stackly-runtimes` membangun tanpa langkah publish dan tanpa izin `id-token`; tag di commit yang bukan bagian `main` membuat workflow gagal sebelum build. Memverifikasi **AC-11**.

## Build plan

Urutan mengikuti pendekatan Journey: satu jalur maintainer (catat sumber, buktikan jalan, simpan bukti) dibuat lengkap pada satu artefak dulu, lalu diperluas. Milestone 2 dan seterusnya butuh langkah 1 spec 0002 sudah dibangun (crate `stackly-evidence`, `cargo xtask evidence run`, dan `ctrlc` di xtask). Langkah 13 juga butuh langkah 4 spec 0002 (`native.yml`). Milestone 3 tinggal di repo lain tanpa file bersama, jadi boleh dikerjakan paralel dengan milestone 2.

**Milestone 1: Inventaris dan pemeriksaan**

1. Tipe inventaris di `xtask/src/runtime/inventory.rs` (serde dan `toml`) dengan aturan 1 sampai 6, plus unit test satu per aturan memakai TOML fixture. Inventaris mulai dengan `schema_version = 1` tanpa artefak. Memenuhi **AC-2**.
2. Cache unduhan bersama (`/usr/bin/curl` dengan `--proto =https --proto-redir =https --fail --location --max-time 1800 --max-filesize <size, atau 4 GiB untuk pin>`, digest dengan crate `sha2`, cek ulang setiap dipakai, entri rusak dihapus) dan `cargo xtask runtime pin`. Memenuhi **AC-1**, **AC-3**.
3. `cargo xtask runtime verify` termasuk pencocokan checksum upstream per `kind` dan GPG opsional. Memenuhi **AC-1**, **AC-3**.
4. `cargo xtask runtime check` (aturan 1 sampai 7) di job `linux`. Memenuhi **AC-2**.

**Milestone 2: Spike ujung ke ujung untuk artefak upstream**

5. Driver `runtime spike` dan worker `runtime spike-worker` sesuai Mekanisme LaunchAgent: lock, rutin pembersihan dan `--cleanup`, unduh dan cek sebelum bootstrap, plan dan plist, bootstrap `gui`, `BLOCKED` tanpa sesi GUI, cek parent launchd, handler SIGTERM, batas waktu, process group, dan helper port. Memenuhi **AC-3**, **AC-4**.
6. Langkah worker: hash ulang, ekstrak (dengan dan tanpa `archive_root`), laporan archive, audit statis, signature dan ad hoc sesuai policy, `min_os`, audit runtime, quarantine sekali per digest, normalisasi versi, penggantian path dengan `$ROOT`, dan file hasil spike. Memenuhi **AC-3**, **AC-5**, **AC-6**, **AC-8**, **AC-9**.
7. Langkah spike NATS, test `nats_2_15_runs_from_managed_artifact`, `cargo xtask runtime record`, aturan 8 di `runtime check`, lalu entri NATS dan evidence pertamanya masuk repo lewat alur dua commit. Memenuhi **AC-1**, **AC-2**, **AC-5**, **AC-10**.
8. Traefik, ClickHouse (dengan aturan fallback), MySQL 8.4, dan MySQL 9.7: langkah spike, test, entri (termasuk kunci GPG MySQL di `runtimes/keys/` bila URL `.asc` mentah tersedia), dan evidence; entri `blocked` bila artefak tidak ada. Memenuhi **AC-1**, **AC-5**, **AC-13**.

**Milestone 3: Pipeline `stackly-runtimes` untuk PostgreSQL dan Valkey**

9. `lib/common.sh` (relocate, sign, audit, package, smoke) dan `requirements.txt`, lalu resep dependency OpenSSL 3.5, ICU, lz4, dan zstd dengan opsi di atas. Memenuhi **AC-11**, **AC-12**.
10. Resep PostgreSQL dengan prefix dan opsi meson di atas serta probe closure di smoke, lalu resep Valkey. Memenuhi **AC-7**, **AC-11**, **AC-12**.
11. `build.yml` untuk PR dan tag, dengan cek tag di `main`, attestation, alur release draft lalu publish, dan `gh attestation verify` di akhir. Memenuhi **AC-11**, **AC-12**.
12. Terbitkan `postgresql-17.11-r1`, `postgresql-18.6-r1`, dan `valkey-9.1.2-r1` (atau patch terbaru saat itu), `pin` ketiganya dengan `--sha256-url`, tulis entrinya, tambahkan tiga test native, masukkan evidence lewat alur dua commit, lalu aktifkan aturan 9. Memenuhi **AC-1**, **AC-2**, **AC-5**, **AC-7**.

**Milestone 4: Otomasi**

13. `runtime spike --prune-cache`, lalu `native.yml`: `timeout-minutes: 90`, `--cleanup` sebelum dan sesudah langkah evidence (yang sesudah dengan `if: always()`), `--prune-cache` hanya di `schedule`, dan cache menetap di Mac self hosted. Butuh langkah 4 spec 0002. Memenuhi **AC-15**.
14. `cargo xtask runtime watch` dan `runtime-watch.yml`. Memenuhi **AC-14**.

## Consequences

**Positive**:

- Fitur 18 memasang dari URL dan digest asli yang sudah terbukti jalan, bukan dari tebakan, dan langkah Validate binary di fitur itu bisa memakai ulang audit dan data quarantine dari spike.
- RISK-01 terjawab dengan bukti: PostgreSQL relocatable, closure terlihat, tanpa Homebrew.
- Bukti runtime memakai format evidence yang sama dengan semua test lain, jadi fitur 65 membacanya tanpa format kedua.
- Upstream yang menarik artefak atau perubahan perilaku macOS ketahuan di run nightly `native.yml`.

**Negative / tradeoffs**:

- Stackly kini merawat pipeline build sendiri untuk PostgreSQL dan Valkey, termasuk mengikuti patch keamanan OpenSSL dan ICU. Watch mingguan hanya mengingatkan; build ulang tetap kerja manual.
- Dua lini MySQL menggandakan biaya test MySQL di atas minimum PRD.
- Tanpa Developer ID, model ini bergantung pada fakta bahwa file unduhan daemon tidak diberi quarantine dan bahwa binary ad hoc boleh dijalankan agent latar. Kalau macOS 27 memperketat ini, spike akan `FAIL` dan semuanya kembali ke OQ-012.
- Artefak ClickHouse dan MySQL tidak punya file checksum upstream yang bisa dicek mesin; kepercayaannya hanya HTTPS saat `pin` dan digest yang dipin sesudahnya (plus GPG MySQL kalau tersedia).
- Test native runtime menambah unduhan besar (tarball MySQL dan binary ClickHouse ratusan MB) dan beberapa menit ke setiap run `native.yml` di satu Mac yang sama, karena delapan spike berjalan berurutan.
- Spike butuh sesi login GUI di Mac uji dan Mac CI; tanpa itu semua test runtime `BLOCKED`.
- Probe quarantine bisa meninggalkan dialog Gatekeeper di layar Mac, sekali per digest baru.
- Langkah spike per provider di xtask akan diganti manifest provider (fitur 9); sampai saat itu ada dua tempat yang tahu cara menjalankan provider.
- Bukti di `evidence/runtime/` berasal dari commit branch fitur; dengan squash merge, commit itu tidak sampai ke `main`, jadi ia hanya berlaku sebagai bukti G0, bukan bukti rilis.

**Neutral**:

- Folder baru `runtimes/` (dengan `runtimes/keys/`) dan `evidence/runtime/`, modul baru `xtask/src/runtime/`, test `xtask/tests/native_runtime.rs`, workflow baru `runtime-watch.yml`, dan repo baru `stackly-runtimes`.
- `xtask` mendapat dependency `sha2` dan `stackly-evidence` (normal, diizinkan spec 0002), serta `stackly-testkit` sebagai dependency dev.
- Artefak dibangun dengan deployment target 26.0 walau produk menargetkan macOS 27, karena SDK runner hosted adalah 26.

## Follow-up

- [ ] Kamu: buat repo publik `ojiepermana/stackly-runtimes`, aktifkan release immutable, pasang ruleset tag, dan setel izin default Actions read only sebelum milestone 3.
- [x] `/architect` update spec 0002: tambahkan `evidence/runtime/<runId>/` ke tabel Folder dan file; ubah `sourceDirty` menjadi `git status --porcelain --untracked-files=no` (sekarang file untracked membuat setiap run lokal dirty); naikkan `timeout-minutes` `native.yml` ke 90; dan tandai follow-up fitur 3 (`native.yml` membersihkan proses sisa) dipegang AC-15 spec ini. Setelah cross check, `sourceDirty` justru kembali ke `git status --porcelain` biasa (lihat rationale spec 0002).
- [ ] Release owner: tinjau keputusan distribusi ulang PostgreSQL, Valkey, OpenSSL, ICU, lz4, dan zstd sebelum rilis publik (AC-078, RISK-12, PRD bagian 39). PRD bukan pendapat hukum.
- [ ] Fitur 60: Developer ID dan notarization untuk artefak buatan maintainer setelah OQ-012 selesai.
- [ ] Fitur 9 dan 18: putuskan cara catalog (`versions.json`) membaca `runtimes/inventory.toml`, lalu ganti tipe inventaris dan langkah spike yang ditandai `// provisional: F9`.
- [ ] Fitur 18: pakai `quarantine.outcome` untuk error terdiagnosis saat OS menolak binary, dan laporan symlink archive upstream untuk aturan ekstraksi.
- [ ] Extension pihak ketiga (pgvector, PostGIS) dan PL/Perl atau PL/Python sebagai keputusan terpisah nanti, dengan lisensi dan closure sendiri.
- [ ] Gate G0: tech lead dan release owner menandatangani berdasarkan isi `evidence/runtime/` dan inventaris.
- [ ] `/sync`: catat perintah `cargo xtask runtime ...`, folder `runtimes/` dan `evidence/runtime/`, dan skill `meson` serta `signing-entitlements` di `## Agent skills` root `AGENTS.md`. Skill `macos-launchd` (`alphaonedev/openclaw-graph`) dipilih tapi repo sumbernya sudah tidak ada (404), jadi catat sebagai tidak tersedia.

## Rationale

Alasan dan opsi yang dipertimbangkan: lihat [rationale.md](rationale.md).
