# 0003. Rationale: sumber dan packaging runtime provider

[Kembali ke spec](index.md)

## Context

Stackly menjanjikan banyak versi dan banyak instance enam provider (PostgreSQL, MySQL, NATS, ClickHouse, Valkey, Traefik) di satu Mac Apple Silicon, tanpa Docker dan tanpa Homebrew global (basis: PRD bagian 20.2, RISK-01). Runtime managed harus punya versi dan artefak yang pasti, dependency closure (semua library yang ikut dimuat) yang terverifikasi, dan harus jalan dari folder Stackly sendiri dengan path absolut, tanpa PATH shell atau symlink versi global (basis: REQ-005). Fitur 18 (install runtime managed) tidak bisa dibangun sebelum jelas dari mana byte artefak itu datang, dan gate G0 butuh bukti bahwa setiap executable benar benar jalan di Mac uji.

Gayanya berbeda per provider. Sebagian upstream menerbitkan binary macOS arm64 resmi, sebagian hanya source. PostgreSQL bergantung pada `lib`, `share`, extension, serta OpenSSL dan ICU, dan tidak otomatis bisa dipindah ke folder lain. Homebrew memakai keg berversi dan symlink `opt` yang bisa berubah saat `brew upgrade` atau `brew cleanup`, jadi tidak cocok sebagai sumber pin (basis: PRD [S16]). Binary yang diunduh tidak ikut tercakup signing dan notarization app manager, dan Stackly tidak boleh menyuruh pengguna mematikan Gatekeeper atau menghapus quarantine secara global (basis: PRD bagian 23 dan 25, [S14]). Identity Developer ID (OQ-012) belum ada.

Batasan lain: tiap artefak butuh sumber dan keputusan distribusi yang tercatat, dan paket dengan izin atau sumber yang tidak jelas tetap `BLOCKED` (basis: AC-078, RISK-12). Repo sudah punya kontrak evidence native (spec 0002) dan satu Mac uji macOS 27. Workspace memakai `cargo xtask` sebagai task runner dan Rust untuk semua tooling (basis: AGENTS.md, spec 0001). Kalau keputusan ini ditunda, fitur 18 akan dibangun di atas URL dan digest karangan, atau diam diam bergantung pada Homebrew developer.

## Options considered

### Option 1: Sumber campuran, upstream resmi bila ada, build maintainer untuk sisanya

Unduh langsung dari rilis resmi upstream dengan digest yang dipin bila upstream menerbitkan binary macOS arm64 (MySQL, NATS, ClickHouse, Traefik). Bangun sendiri PostgreSQL dan Valkey sebagai build maintainer (basis: OQ-003 default, "sumber resmi atau maintainer built dengan provenance").

**Pros**:

- Binary upstream dipakai apa adanya, termasuk signature Developer ID upstream bila ada; Stackly tidak mendistribusikan ulang binary GPL MySQL.
- Build sendiri hanya untuk provider yang memang tidak punya binary relocatable, sehingga closure PostgreSQL (OpenSSL, ICU, lz4, zstd) dikendalikan penuh.

**Cons**:

- Dua jalur verifikasi (checksum upstream dan verifikasi build sendiri) yang harus dirawat.
- URL upstream bisa pindah atau hilang (misalnya arsip MySQL); hanya digest dan URL kandidat yang menolong.

### Option 2: Semua dibangun maintainer

Maintainer membangun keenam provider dari source dan menerbitkan semuanya di hosting Stackly (basis: PRD bagian 20.2, mode build from source sebagai pipeline maintainer).

**Pros**:

- Satu cara verifikasi, satu format, satu kebijakan signing.
- Tidak bergantung pada ketersediaan URL upstream.

**Cons**:

- Build ClickHouse sangat berat (jam per build, toolchain LLVM tertentu) dan MySQL besar; biaya build melonjak.
- Stackly mendistribusikan ulang semuanya, termasuk binary GPL MySQL dengan kewajiban menyediakan source.

### Option 3: Build pihak ketiga untuk yang tidak punya binary resmi

PostgreSQL dari `theseus-rs/postgresql-binaries` (17.11 dan 18.6 untuk `aarch64-apple-darwin`, ada `.sha256`), Valkey dan sisanya dari upstream atau build pihak ketiga lain (basis: cek lanskap 17 Oktober 2026).

**Pros**:

- Tanpa build sendiri; artefak PostgreSQL langsung tersedia.

**Cons**:

- Flag build, closure, dan provenance di luar kendali; tanpa signature. Untuk Valkey tidak ada build macOS pihak ketiga yang setara, jadi build sendiri tetap dibutuhkan.

### Option 4: Bottle Homebrew sebagai sumber

Ambil bottle dari registry Homebrew lalu relokasi sendiri (basis: PRD [S16], keg dan symlink `opt` Homebrew).

**Pros**:

- Hampir semua provider tersedia, dibangun rutin oleh proyek besar.

**Cons**:

- Bottle memakai placeholder prefix dan dependency antar bottle yang harus direlokasi dan diaudit sendiri; artefak lama dibersihkan dari registry, dan ketergantungan pada graf dependency Homebrew bertentangan dengan tujuan lepas dari Homebrew.

## Rationale

Option 1 dipilih karena memisahkan dua masalah yang memang berbeda. Untuk provider dengan binary resmi, risiko utamanya adalah supply chain dan URL yang hilang, dan digest yang dipin di inventaris yang direview menjawab keduanya tanpa menambah pekerjaan build. Untuk PostgreSQL, risiko utamanya relokasi dan closure (RISK-01), dan hanya build sendiri yang memberi kendali atas install name, dependency bundel, dan tanda tangan setelah `install_name_tool`. Valkey tidak punya binary macOS resmi sama sekali, jadi build sendiri diperlukan apa pun pilihannya; memasukkan PostgreSQL ke jalur build yang sama hampir gratis.

Option 2 membayar biaya build ClickHouse dan kewajiban redistribusi GPL tanpa mengurangi risiko yang nyata. Option 3 menghemat waktu sekarang tapi meninggalkan pertanyaan provenance yang justru ditanyakan AC-078. Option 4 mempertahankan ketergantungan pada Homebrew yang ingin dihilangkan PRD.

Bukti dibuat lewat test native spec 0002, bukan format sendiri, karena fitur 65 membaca satu format evidence untuk semua bukti rilis. Worker dijalankan launchd lewat LaunchAgent sementara karena Terminal bisa punya izin Developer Tools yang melewati pemeriksaan Gatekeeper, sehingga run dari Terminal tidak mewakili cara stacklyd (agent SMAppService) menjalankan provider.

### Keputusan turunan

Keputusan yang kamu pilih saat wawancara:

| Keputusan | Pilihan | Runner up |
| --- | --- | --- |
| Kedalaman spike | start, ready, stop di temp root | hanya `--version` |
| Konteks launch | LaunchAgent sementara | - |
| Bukti tanpa Homebrew | audit library statis dan runtime | VM macOS bersih |
| Lokasi inventaris | `runtimes/inventory.toml` dicek xtask | tabel Markdown di spec |
| Lokasi kode spike | subcommand xtask, dipanggil test native | skrip shell |
| Perilaku quarantine | direkam per artefak | diteliti di fitur 18 |
| Distribusi upstream | unduh langsung, digest dipin | mirror semua |
| Sumber PostgreSQL | build maintainer | `theseus-rs` |
| Closure PostgreSQL | OpenSSL, ICU, lz4, zstd | OpenSSL dan ICU saja |
| MySQL | 8.4 LTS dan 9.7 LTS | 8.4 LTS saja |
| ClickHouse | LTS terbaru, fallback stable | stable terbaru |
| Signing upstream | pertahankan, ad hoc bila perlu | selalu ad hoc |
| Signing maintainer | ad hoc sekarang, Developer ID di fitur 60 | tunggu Developer ID |
| Valkey | 9.1, tanpa TLS | 8.1, atau dengan TLS |
| NATS, Traefik | 2.15 dan 3.7 terbaru | lini sebelumnya |
| Format artefak maintainer | `tar.gz` | `tar.zst` |
| Bump versi | PR manual (cek mingguan dihapus 2026-10-17) | manual sepenuhnya |
| Satuan inventaris | per artefak | per provider |
| Status inventaris | `available` atau `blocked`, hasil uji di evidence | status PRD 5.1 |
| Evidence | test native spec 0002, salinan run bersih di `evidence/runtime/` | perintah xtask lewat wrapper |
| Closure dicatat | bundel dan sistem | bundel saja |
| Aturan evidence | `available` butuh `PASS` | hanya dicatat |
| Extension PostgreSQL | contrib saja | contrib dan pgvector |
| Cache spike | per sha256, dicek ulang | selalu unduh baru |

Keputusan yang saya ambil sendiri (rekomendasi, dengan runner up):

- **Unduhan lewat `/usr/bin/curl`**, bukan crate HTTP, supaya tidak mendahului pilihan HTTP client daemon di fitur 18, dan karena `curl` tidak memberi atribut quarantine, sama seperti unduhan daemon nanti. Runner up: `ureq`.
- **Digest dengan crate `sha2`** supaya `pin`, `verify`, dan `check` jalan di OS apa pun. Runner up: `shasum -a 256`.
- **Audit library runtime dengan `lsof`**, bukan `DYLD_PRINT_LIBRARIES`, karena variabel `DYLD_*` diabaikan pada binary dengan hardened runtime (misalnya binary Developer ID upstream), sedangkan `lsof` melihat file yang benar benar dipetakan proses. Ini menyesuaikan pilihan "audit library" yang kamu ambil, yang deskripsinya menyebut `DYLD_PRINT_LIBRARIES`. Runner up: `DYLD_PRINT_LIBRARIES`.
- **Ekstraksi spike dengan `/usr/bin/tar`** setelah gerbang digest; extractor aman yang diuji fuzzing adalah milik fitur 18. Spike tetap melaporkan symlink, setuid, dan file device sebagai data untuk fitur 18. Runner up: crate `tar` dan `flate2` sekarang.
- **Root spike berisi spasi dan `Ü`**, dan socket di `/tmp/sk-*` yang pendek, karena path Application Support asli berisi spasi (PRD 32.2 meminta uji path spasi dan Unicode) dan path socket Unix dibatasi panjangnya.
- **Tanpa credential di spike** (PostgreSQL `trust` hanya lewat socket di folder 0700 dengan user database `stackly`, MySQL tanpa jaringan), supaya tidak ada secret yang bisa bocor ke evidence dan nama user OS tidak masuk ke role atau log. Peer auth akan memaksa nama user OS menjadi nama role. Runner up: `FixtureCredential` spec 0002 dengan `scram-sha-256`.
- **Batas waktu** ready 120 s (start pertama ClickHouse dan init MySQL lambat), stop 60 s, quarantine 30 s (dialog Gatekeeper bisa menahan proses), worker 12 menit, driver 15 menit.
- **Meson untuk PostgreSQL**, karena itu sistem build masa depan PostgreSQL dan didukung penuh di 17 dan 18. Runner up: `configure`.
- **Tool build dari PyPI dengan hash** (meson, ninja, pkgconf) dan `PATH` tanpa Homebrew, supaya library Homebrew tidak bisa ikut tertaut. Runner up: tool Homebrew plus audit saja.
- **OpenSSL lini 3.5 LTS** untuk umur dukungan panjang. Runner up: lini 3.x terbaru non LTS.
- **Fitur PostgreSQL yang dimatikan** (GSSAPI, LDAP, PAM, Bonjour, NLS, JIT LLVM, PL/Perl, PL/Python, PL/Tcl): jarang dipakai untuk dev lokal, dan PL/Perl atau PL/Python akan menautkan runtime bahasa sistem dengan ABI yang bisa berubah antar versi macOS.
- **Archive maintainer tanpa symlink**, supaya extractor fitur 18 cukup menangani file biasa untuk artefak buatan sendiri.
- **Satu test native per lini**, supaya bump versi tidak butuh perubahan kode dan satu lini yang gagal tidak menyeret yang lain. Runner up: satu test untuk semua artefak.
- **`covers = ["AC-078", "REQ-005"]`**, karena hanya ID PRD tiga digit yang diterima spec 0002, dan keduanya yang dibuktikan spike (inventaris distribusi dan runtime tanpa PATH atau symlink global).
- **Tipe inventaris di xtask**, bukan di `stackly-domain`, karena model runtime domain adalah keputusan fitur 4 dan 18. Runner up: tipe domain sekarang.
- **Owner default `@ojiepermana`** untuk entri `blocked`, sebagai runtime maintainer (owner OQ-003).
- **Server dijalankan langsung sebagai child** (`bin/postgres`, bukan `pg_ctl start`), karena `pg_ctl` memanggil `setsid` sehingga postmaster lepas dari process group dan tidak ikut dimatikan. Runner up: `pg_ctl` plus pembersihan lewat file PID.
- **Unduhan di driver, sebelum bootstrap**, supaya unduhan pertama ratusan MB tidak memakan batas waktu worker. Runner up: batas waktu worker yang jauh lebih panjang.
- **Lock file di folder cache dan label LaunchAgent tetap**, bukan `--test-threads=1`, supaya perintah native di spec 0002 tidak berubah dan daftar Login Items tidak bertambah setiap run. Runner up: `--test-threads=1` dan label per run.
- **Probe quarantine sekali per `sha256`**, karena dialog Gatekeeper bisa tertinggal di layar Mac uji di setiap run. Runner up: setiap run, atau hanya dengan flag manual.
- **Hanya domain `gui`**, tanpa fallback `user/<uid>`: agent SMAppService yang ingin ditiru hidup di sesi GUI. Tanpa sesi GUI, test `BLOCKED`. Runner up: fallback ke `user/<uid>`.
- **Dependency `own`** untuk library dari source provider sendiri (libpq, libmysqlclient), supaya aturan "inventaris sama dengan audit" tidak memaksa versi dan lisensi palsu untuk library yang sudah tercakup lisensi artefak. Runner up: mengecualikan library itu dari perbandingan.
- **Prefix build PostgreSQL tanpa kata `postgres` atau `pgsql`**, supaya PostgreSQL tetap memakai `lib/postgresql/` dan `share/postgresql/`, sesuai aturan rpath. Runner up: prefix yang memuat `postgresql` dengan layout datar.
- **OpenSSL `--openssldir=/etc/ssl`**, supaya OPENSSLDIR menunjuk lokasi yang ada di setiap Mac, bukan path build yang hilang setelah relokasi. Runner up: biarkan default dan tidak memakai sertifikat sistem.

### Cross check

Spec ditinjau oleh model lain (read only) sebelum diterima, lalu sekali lagi bersama spec 0002. Temuan yang diterapkan: helper port sendiri di xtask (testkit hanya dependency dev), allowlist audit runtime untuk shared cache dyld di cryptex, server PostgreSQL sebagai child langsung, unduhan dipindah ke driver, `archive_root` opsional (Traefik), mekanisme plist dan pembersihan proses lewat `pgrep`, alur dua commit untuk evidence, normalisasi versi, aturan `BLOCKED` dan `FAIL` di test, dependency `own`, opsi configure resep, MySQL tanpa checksum polos, quarantine lewat clone APFS sekali per digest, privasi user dan hostname, lock untuk test paralel, listener provider yang eksplisit, dan aturan 9 untuk delapan lini.

Setelah spec 0002 diupdate untuk spec ini, kedua spec dibaca ulang oleh model lain (Sonnet 5.5, baca saja). Temuan yang diterapkan atas persetujuan engineer: `runtime record` hanya menyalin record `PASS` dan `FAIL`, ikut menyalin `output.log`, dan menolak report bermasalah, commit yang bukan `HEAD`, serta folder yang sudah ada; aturan 7 mengecek nama folder dan `summary`; subject untuk `FAIL` sebelum versi terbaca; lock untuk `--cleanup`; `--prune-cache` terpisah; pencarian quarantine dari file yang dilacak git; guard `evidence!` di setiap fungsi test; filter report per package; dan format `actual` untuk `PASS`. Alur dua commit sekarang mengandalkan `sourceDirty` yang menghitung file untracked (spec 0002).

## Update 2026-10-17: format `zip` untuk ClickHouse

### Konteks

Saat milestone 2, aset polos `clickhouse-macos-aarch64` dari `v26.8.29.10-lts` (dan dari 26.9 stable, dengan stub 17 load command yang sama) ternyata binary self extracting: identifier codesign-nya `decompressor`, ada payload terkompresi di ekor file, `codesign --verify` dan `codesign --force --sign -` sama sama gagal dengan "main executable failed strict validation", dan saat pertama kali jalan ia menulis ulang dirinya dari sekitar 177 MB menjadi binary 892 MB. `keep_or_adhoc` tidak bisa menandatanganinya, dan byte yang benar benar jalan bukan byte yang digest-nya dipin. Entri ClickHouse dicatat `blocked` (basis: `runtimes/inventory.toml`, entri `clickhouse-26.8.29.10-macos-aarch64`).

Rilis yang sama membawa `clickhouse-macos-aarch64.zip` (sha256 `e93d4a5294f7dd1131fb860c73d6b4a64e9907730b961bcdaafb6ad8a15e7907`, 215990301 byte) berisi satu file `clickhouse`, binary biasa yang tidak mengekstrak dirinya sendiri, bertanda tangan Developer ID (team `ZNDB5FJ8ZW`, hardened runtime), yang lulus verifikasi. Run manual binary itu dengan config langkah spike ClickHouse (`xtask/src/runtime/spike/providers.rs`) ready dalam sekitar 11 detik, `SELECT version()` = `26.8.29.10`, dan berhenti dengan kode 0 sekitar 1 detik setelah SIGTERM. Spec hanya mengizinkan `format = "tar_gz"` atau `"binary"` (aturan 3 di `xtask/src/runtime/inventory.rs`).

### Opsi

**Opsi A: tambah format `zip`.** Unduh `.zip`, cek digest, ekstrak, lalu perlakukan seperti artefak upstream lain (basis: pin artefak dengan digest kriptografis).

- Pro: binary yang jalan sama persis dengan byte yang diekstrak dari archive yang digest-nya dipin; signature Developer ID upstream dipakai apa adanya; langkah spike ClickHouse yang sudah ada tidak berubah.
- Kontra: pembaca archive kedua, di spike sekarang dan di extractor fitur 18 nanti; ClickHouse bisa berhenti menerbitkan `.zip`.

**Opsi B: langkah pra dekompresi untuk binary self extracting.** Jalankan stub sekali di folder sementara supaya ia mengekstrak dirinya, lalu tanda tangani hasilnya ad hoc.

- Pro: tetap satu aset polos, format inventaris tidak bertambah.
- Kontra: kode upstream jalan sebelum ada byte yang bisa diverifikasi signature-nya; binary 892 MB hasil dekompresi tidak punya digest yang dipin, jadi AC-3 hanya melindungi stub; signature Developer ID hilang diganti ad hoc; fitur 18 harus meniru langkah yang sama di daemon; dan perilaku stub adalah detail internal ClickHouse yang bisa berubah tanpa pemberitahuan.

**Opsi C: tetap `blocked` sampai fitur 18.** Biarkan ClickHouse `blocked` dan putuskan formatnya saat extractor fitur 18 dibangun.

- Pro: tanpa kode baru sekarang; fitur 3 tetap memenuhi Done when karena `blocked` dengan alasan dan owner diizinkan.
- Kontra: provider wajib keenam tidak punya bukti jalan dari LaunchAgent di gate G0 walau artefak yang layak sudah ada; fitur 18 harus memutuskan format zip tanpa bukti native; AC-13 mencatat `blocked` untuk lini yang sebenarnya punya artefak managed.

### Keputusan

Opsi A. Opsi B melanggar janji utama spec ini (tidak ada byte yang jalan sebelum digest-nya cocok, AC-3) dan membuang signature Developer ID upstream, hanya demi menghindari satu nilai enum. Opsi C menunda keputusan yang biayanya kecil sekarang dan besar nanti: tanpa bukti native, fitur 18 harus memilih pembaca zip dan aturan ekstraksinya dari tebakan. Opsi A membuat ClickHouse sama dengan provider upstream lain, dengan risiko baru yang terbatas pada ekstraksi, dan risiko itu sudah dijawab oleh extractor yang sama dengan `tar_gz`.

Keputusan turunan:

- **Extractor `/usr/bin/tar` (bsdtar 3.5.3, libarchive 3.7.4) untuk zip**, bukan `/usr/bin/ditto -x -k` atau `/usr/bin/unzip`. Runner up: `ditto` dengan `--noextattr --noqtn`. Percobaan di bawah menunjukkan ketiganya menolak penulisan lewat symlink dan tidak ada yang menulis di luar folder ekstraksi. Bedanya dua: hanya bsdtar yang gagal dengan jelas pada anggota `..` (`unzip` dan `ditto` diam diam membuang `..` lalu menulis di dalam folder, sehingga archive yang aneh lolos tanpa tanda), dan bsdtar sudah menjadi extractor `tar_gz` yang dipakai `pin` untuk mendaftar dan mengekstrak satu anggota. `ditto` tidak bisa melakukan keduanya, jadi memilihnya berarti `pin` butuh alat kedua.
- **`--no-mac-metadata` wajib untuk zip**, karena tanpa flag itu bsdtar macOS mencoba memulihkan `__MACOSX/._<nama>` sebagai xattr (bertentangan dengan man page-nya; di percobaan, isi palsu membuatnya gagal dengan `Failed to restore metadata`), dan zip buatan Mac bisa membawa metadata, termasuk atribut quarantine milik pembuatnya. Perintah `tar_gz` tidak diberi flag ini, supaya perintah `tar` yang menghasilkan bukti milestone 2 tetap sama; yang baru untuk `tar_gz` hanya cek format sebelum ekstraksi, dan bukti itu tetap berlaku karena aturan 8 terikat pada `sha256` dan keempat file itu gzip. Celahnya (archive `tar_gz` buatan Mac dengan entri `._` masih bisa memulihkan xattr) dicatat untuk fitur 18.
- **Cek magic number terhadap `format`**, karena bsdtar mengenali format sendiri: `tar -xzf` pada file zip berhasil dengan kode 0. Tanpa cek ini, label `format` yang salah lolos spike lalu merusak extractor fitur 18, yang memilih pembaca dari label itu. `pin` mencetak `format` dari magic number supaya nilainya berasal dari byte, bukan diketik.
- **`binary` tetap diterima** walau tidak ada entri yang memakainya. Membuangnya berarti mengubah kode dan test yang sudah jalan tanpa mengurangi risiko, dan langkah signing sudah menangkap binary self extracting. Fitur 18 boleh membuangnya kalau tetap tidak terpakai.
- **Fixture zip dibuat di test oleh penulis zip `stored` kecil**, bukan crate `zip` sebagai dependency dev, karena kasus serangan (anggota `../x`, symlink lalu anggota di bawahnya) tidak bisa dibuat dengan `/usr/bin/zip` dari file nyata, dan workspace hanya menambah crate saat kode memakainya. Runner up: crate `zip` sebagai dependency dev.
- **Lini tetap 26.8 LTS**, sesuai aturan fallback AC-13: rilis LTS terbaru yang punya aset macOS aarch64 yang bisa dipakai.

### Percobaan extractor

Dijalankan 2026-10-17 di Mac uji macOS 27 dengan zip buatan sendiri (dibuat dengan `zipfile` Python, anggota bermode Unix), masing masing diekstrak ke folder baru yang kosong. "Di dalam" berarti file tertulis di bawah folder ekstraksi; tidak ada satu pun percobaan yang menulis di luar folder itu.

| Zip | `/usr/bin/tar -xf` | `/usr/bin/unzip` | `/usr/bin/ditto -x -k` |
| --- | --- | --- | --- |
| anggota `../escaped` | kode 1, `Path contains '..'`, anggota dilewati | kode 0, `..` dibuang diam diam, file di dalam | kode 0, `..` dibuang diam diam, file di dalam |
| anggota path absolut | kode 0, `/` di depan dibuang, file di dalam | kode 1, peringatan, file di dalam | kode 0, `/` dibuang diam diam, file di dalam |
| symlink `link` ke folder luar, lalu `link/escaped` | kode 1, `Cannot extract through symlink` | kode 2, `checkdir error` | kode 1, `Not a directory` |
| satu file `clickhouse` mode 0755 | kode 0, mode 0755 | kode 0, mode 0755 | kode 0, mode 0755 |
| `clickhouse` plus `__MACOSX/._clickhouse` | kode 1, `Failed to restore metadata`; dengan `--no-mac-metadata` kode 0 dan hanya `clickhouse` | kode 0, folder `__MACOSX/` ikut menjadi file | kode 1, folder `__MACOSX/` ikut sebagian |

Selain itu: `tar -tf` dan `tar -xf <zip> --no-mac-metadata -- clickhouse` (satu anggota, seperti `pin`) bekerja pada zip, dan `tar -xzf` pada zip juga berhasil dengan kode 0 (dasar cek magic number). File hasil ekstraksi hanya membawa xattr `com.apple.provenance` yang diberi macOS sendiri, bukan dari archive.

### Cross check update

Update ini dibaca model lain (Sonnet 5.5, baca saja) sebelum diterima. Temuan yang diterapkan atas persetujuan kamu: nama jenis file dan letak cek format (`check_format` di `macho.rs`, dipanggil di `archive::extract` dan `runtime verify`, juga untuk `tar_gz` dan `binary`); penjelasan bahwa bukti milestone 2 tetap berlaku; rincian perubahan `pin` (field `format` pertama, klasifikasi di semua OS, catatan `unknown`, flag `tar` per jenis); aturan 3 untuk `archive_root`; urutan pengisian dependency ClickHouse dari spike lokal, plus baris Value sourcing untuk `archive_root`, `executables`, dan dependency; langkah verify untuk signature Developer ID dan `archive.files`; cek awal bit eksekusi di zip asli sebelum entri ditulis; alasan pemilihan bsdtar yang lebih jujur (ketiga alat menolak symlink, bedanya di `..` dan pemakaian ulang oleh `pin`); dan dua follow-up fitur 18 (entri `._` di `tar_gz`, dan uji ulang kasus AC-16 pada pembaca zip fitur 18). Kasus lain yang ditanyakan (zip64, zip terenkripsi, nama ganda, tabrakan huruf besar kecil di APFS, hard link, signature setelah ekstraksi, hardened runtime pada audit `lsof` dan probe quarantine) dinilai bukan masalah: anggota ClickHouse jauh di bawah 4 GiB, zip terenkripsi gagal karena tidak ada TTY di bawah launchd, nama dipin lewat digest, zip tidak punya hard link, signature tertanam di Mach-O, dan MySQL dengan hardened runtime sudah lulus kedua pemeriksaan itu.

## Update 2026-10-18: bit eksekusi untuk executable di zip

### Konteks

Saat langkah 16, `runtime pin` untuk `clickhouse-macos-aarch64.zip` dari `v26.8.30.8-lts` (patch 26.8 terbaru, terbit 17 Oktober 2026; sha256 `097b41ad97111832cc5ddddd0b514e80307df24f50b5893e9d2035a0c4f47402`, 216009716 byte) mencetak `format = "zip"` dan `min_os = "11.0"`, tapi cek awal langkah 16.2 gagal: `tar -tvf` menunjukkan `clickhouse` bermode `-rw-rw-r--`. Central directory zip itu mencatat asal Unix (version made by 3.0) dan atribut Unix 100664, jadi itu memang mode yang disimpan upstream, bukan mode bawaan karena atributnya kosong. Central directory rilis lain, dibaca lewat HTTP range tanpa mengunduh seluruh file, memberi hasil yang sama:

| Rilis | Mode `clickhouse` di zip |
| --- | --- |
| `v26.8.11.7-lts`, `v26.8.20.9-lts`, `v26.8.29.10-lts`, `v26.8.30.8-lts` | 100664 |
| `v26.7.33.3-stable`, `v26.9.21.9-stable` | 100664 |

Setelah ekstraksi dengan umask 022 file itu menjadi 0644, sehingga tidak bisa dijalankan. Binary hasil ekstraksi bertanda tangan Developer ID (`ClickHouse Inc. (ZNDB5FJ8ZW)`, hardened runtime, timestamp 17 Oktober 2026), lulus `codesign --verify --strict`, dan tetap lulus setelah `chmod 0755`, karena mode file tidak masuk code signature. `otool -L` hanya menunjukkan library sistem. Binary itu tidak dijalankan saat pemeriksaan ini.

### Opsi

**Opsi A: spike memberi mode 0755 pada `executables` artefak `zip`.** Setelah ekstraksi, setiap path di `executables` dicek sebagai file biasa tanpa symlink di path-nya, lalu diberi mode tepat 0755.

- Pro: aturan yang sama dengan format `binary`, yang sudah memberi 0755 karena unduhan satu file tidak membawa mode; tanpa field inventaris baru; digest archive dan signature Mach-O tetap utuh; `tar_gz` dan bukti milestone 2 tidak berubah.
- Kontra: mode yang jalan berbeda dengan mode di archive; fitur 18 harus meniru aturan ini; `chmod` mengikuti symlink, jadi perlu guard sendiri.

**Opsi B: field opt in di inventaris** (misalnya `set_executable_mode = true`), dengan perilaku seperti opsi A hanya untuk entri yang menulisnya.

- Pro: keputusan per artefak terlihat saat inventaris direview.
- Kontra: satu field, satu aturan `runtime check`, dan test tambahan untuk satu entri; setiap path di `executables` memang dijalankan spike, jadi field itu tidak menambah informasi; lupa menulis field berarti spike `FAIL` dengan pesan izin yang membingungkan.

**Opsi C: tetap `blocked`, laporkan ke upstream** supaya zip menyimpan 0755.

- Pro: tanpa kode baru; spike tetap tidak pernah mengubah mode.
- Kontra: gate G0 tidak punya bukti ClickHouse untuk waktu yang tidak tentu, padahal artefak yang bertanda tangan dan lulus verifikasi sudah ada; upstream belum tentu berubah.

### Keputusan

Opsi A, dipilih pemilik proyek pada 2026-10-18. Yang dilindungi spec ini adalah byte yang dijalankan (digest archive, AC-3) dan identitas penerbitnya (signature, AC-8); mode file bukan bagian keduanya, dan format `binary` sudah memberi 0755 dengan alasan yang sama. Batasnya sempit: hanya `zip`, hanya path di `executables` yang sudah direview di inventaris dan memang dijalankan spike, hanya file biasa tanpa symlink. Runner up: opsi B, kalau suatu saat ada artefak `zip` yang salah satu path `executables`-nya sengaja tidak boleh dijalankan.

Keputusan turunan:

- **Mode tepat 0755**, bukan menambah bit eksekusi di tempat yang punya bit baca: hasilnya sama untuk setiap archive dan sama dengan `binary`.
- **Cek symlink per komponen path sebelum `chmod`.** bsdtar sudah menolak menulis lewat symlink, tapi archive boleh berisi symlink folder ke luar tanpa anggota di bawahnya, dan `chmod` pada path lewat symlink itu akan mengubah file di luar root.
- **Dicatat di detail langkah `extract`** (`set mode 0755 on <path> (was 0664)`), bukan field baru di hasil spike, supaya skema `stackly.runtime-spike-result/v1` tidak berubah, dan langkah verify milestone 2b tetap bisa melihatnya.
- **Tidak berlaku untuk `tar_gz`**, karena keempat tarball yang sudah dipin menyimpan mode yang benar, dan bukti milestone 2 terikat pada perintah ekstraksi yang sama.
- **Risiko sisa: race antara cek dan `chmod`.** Di antara `symlink_metadata` dan `chmod`, proses lain dengan uid yang sama bisa mengganti path dengan symlink. Risiko ini diterima: `tar` sudah selesai, provider belum jalan, foldernya di `TMPDIR` milik user itu sendiri, dan `fchmod` lewat `O_NOFOLLOW` butuh libc dan `unsafe`, yang tidak boleh ada di xtask.

### Cross check update 2026-10-18

Update ini dibaca model lain (Sonnet 5.5, baca saja). Temuan yang diterapkan atas persetujuan kamu: mode lama di catatan adalah mode di disk setelah `tar`, yang dipengaruhi umask (`clickhouse` menjadi 0644 di bawah umask 022, bukan 0664), dengan fixture 0644 di test; urutan cek per komponen path di bawah root artefak dan pesan untuk path yang tidak ada, symlink (termasuk yang targetnya tidak ada), dan folder; format detail `extract`; file yang sudah 0755 tidak dicatat; bit setuid dan setgid dibuang dengan bukti di catatan; clone quarantine dan tanda tangan ulang ad hoc mempertahankan mode (dicek); langkah verify ClickHouse hanya mengharapkan catatan itu kalau zip menyimpan mode tanpa bit eksekusi; executable `zip` berupa symlink sengaja `FAIL`; dan race di atas sebagai risiko sisa. Varian yang lebih sederhana (mencari path di hasil `archive::walk`) ditolak karena pesan untuk symlink folder menjadi kurang jelas.

## Update 2026-10-17: CI ditiadakan

Pemilik proyek meniadakan CI; pengembangan fokus di Mac ini dulu, dan cek, test native, serta run evidence dijalankan dengan tangan. Keputusan yang berubah:

- Pipeline `stackly-runtimes` (runner hosted, tag terlindungi, release, attestation) dihapus. OQ-003 tetap (build maintainer diperlukan), tapi cara membangun PostgreSQL dan Valkey di Mac ini, termasuk deployment target 26.0 yang dulu mengikuti SDK runner hosted, menunggu `/architect`. Diputuskan 2026-10-18 (lihat Update 2026-10-18 di bawah).
- Watch mingguan (`runtime watch`, `runtime-watch.yml`, AC-14) dan wiring `native.yml` (AC-15) dihapus, jadi bump versi tanpa pengingat; `--cleanup` dan `--prune-cache` tetap perintah lokal.
- Penolakan run `pull_request` di `runtime record` dan aturan 8 dihapus dari spec, walau kodenya masih ada.

## Update 2026-10-18: cara membangun PostgreSQL dan Valkey di Mac ini

### Konteks

Setelah CI ditiadakan, OQ-003 tetap terjawab (build maintainer diperlukan untuk PostgreSQL dan Valkey), tapi tidak ada lagi tempat yang membangunnya. Build harus jalan dengan tangan di satu Mac pengembangan yang juga dipakai sehari hari, dan Mac itu penuh Homebrew: `/opt/homebrew/bin` ada di depan `PATH` dengan `pkg-config`, `cmake`, `python3`, dan library seperti `openssl@3`, `lz4`, dan `zstd`. Library Homebrew yang ikut tertaut adalah persis kegagalan yang ingin dicegah RISK-01 (basis: PRD bagian 20.2). Aturan 5 di `xtask/src/runtime/inventory.rs` masih mewajibkan `recipe` dan URL `attestation` https yang dulu datang dari pipeline CI, dan fixture test-nya merujuk repo `ojiepermana/stackly-runtimes` yang tidak ada. Artefak maintainer harus bisa diunduh lewat https, karena `pin`, `verify`, spike, dan nanti fitur 18 membaca `urls`. Langkah build plan lama juga menyebut "tambahkan tiga test native" tanpa menyebut implementasi `Recipe` PostgreSQL dan Valkey di `xtask/src/runtime/spike/providers.rs`.

### Inventaris toolchain Mac ini (18 Oktober 2026)

| Hal | Temuan |
| --- | --- |
| macOS, Xcode | macOS 27.0, Xcode 27.0 (27A266a), SDK 27.0, Apple clang 21.0.0 |
| Python non Homebrew | hanya `/usr/bin/python3` 3.9.6 (bawaan Xcode); `uv`, `python3`, `cmake`, `pkg-config`, dan `pkgconf` di `PATH` semuanya dari Homebrew |
| meson, ninja | tidak terpasang |
| bison, flex, perl, make | `/usr/bin/bison` 2.3, `/usr/bin/flex` 2.6.4, `/usr/bin/perl` 5.34.1, `/usr/bin/make` GNU 3.81; tanpa autoconf atau automake |
| ICU di SDK | `usr/include/unicode/` hanya 23 header (`U_ICU_VERSION` 78.1), tanpa `ucol.h`, `ucnv.h`, atau `uloc.h`; `libicucore.tbd` tanpa API publik |
| libxml2 di SDK | header `usr/include/libxml2`, `libxml2.tbd`, `/usr/bin/xml2-config` 2.9.13; tidak ada file `.pc` di SDK |
| libedit, zlib di SDK | `libedit.tbd`, header `editline/` dan `histedit.h`; `libz.tbd` dan `zlib.h`; tidak ada file `.pc` |
| Jalur cari bawaan | `clang -v` mencari `/usr/local/include` lebih dulu dan `ld -v` mencari `/usr/local/lib` lebih dulu, walau SDK dipakai. Di Mac ini isinya hanya `include/node/` dan `lib/node_modules/` |
| bsdtar | 3.5.3, mendukung `--no-mac-metadata`, `--uid`, `--gid`, `--uname`, `--gname` |
| Repo | `ojiepermana/stackly` publik; `ojiepermana/stackly-runtimes` tidak ada |

### Cek lanskap (18 Oktober 2026)

| Hal | Temuan |
| --- | --- |
| meson | terbaru 1.12.1, butuh Python 3.10 ke atas; 1.11.2 rilis terakhir untuk Python 3.7 sampai 3.9 |
| ninja (PyPI) | 1.13.2, Python 3.8 ke atas, wheel `macosx_10_9_universal2` |
| pkgconf (PyPI) | terbaru 3.0.7.post0 butuh Python 3.10; 2.4.3.post2 rilis terakhir untuk Python 3.9, dengan wheel `macosx_11_0_arm64` |
| PostgreSQL | 17.11 dan 18.6 (13 Agustus 2026), minor berikutnya 12 November 2026. Minimum: meson 0.54, bison 2.3, flex 2.5.35, perl 5.14. Commit "Remove distprep" ada di 17 dan 18, jadi tarball tidak membawa output bison, flex, atau perl. `meson.build` REL_18_STABLE mencari `libxml-2.0` hanya lewat pkg-config lalu CMake, dan libedit lewat pkg-config lalu `cc.find_library('libedit')` |
| OpenSSL | 3.5.9 (29 September 2026), lini 3.5 didukung sampai 8 April 2030; 3.6.5 berakhir 1 November 2026; 4.0.3 ada |
| ICU4C | stabil terbaru 78.3 (`icu4c-78.3-sources.tgz`, `SHASUM512.txt` dan `.asc`, tanpa `.sha256`); 79 masih RC. Configure ICU memakai Python untuk membangun data dari source |
| lz4, zstd | lz4 1.10.0 dan zstd 1.5.7, masing masing dengan `.sha256` per tarball |
| Valkey | 9.1.2 (1 September 2026); README masih `make`, tanpa CMake; `valkey-hashes` aktif dan memuat `valkey-9.1.2.tar.gz` |
| GitHub Releases | batas 2 GiB per file, tanpa batas total; URL unduhan untuk tag yang memuat `/` bekerja dalam bentuk mentah maupun `%2F` (HEAD 302 ke CDN) |

### Opsi dan pilihan kamu

| Keputusan | Pilihan kamu | Runner up | Kenapa runner up kalah |
| --- | --- | --- | --- |
| Letak resep | repo publik terpisah `ojiepermana/stackly-runtimes` | `runtimes/recipes/` di repo ini (rekomendasi saya) | lihat catatan di bawah tabel |
| Visibilitas repo resep | publik | privat | artefak publik akan menunjuk resep yang tidak bisa dibuka pengguna |
| Hosting artefak | GitHub Releases repo `ojiepermana/stackly`, tag `runtime/<id>`, `--latest=false` | repo aset publik terpisah; lokal dulu sampai fitur 18 | repo aset menambah satu repo lagi; lokal dulu membuat `runtime verify` tidak bisa membuktikan URL, dan AC-1 butuh pengecualian |
| Cara build | `build.sh` bash yang menjalankan ulang dirinya lewat `env -i` | wrapper `cargo xtask runtime build` | kode Rust tambahan untuk pekerjaan tangan yang jarang, padahal isolasi environment cukup dengan `env -i` |
| Deployment target | 27.0 | 26.0 | 26.0 hanya mengikuti SDK runner hosted yang sudah dihapus, dan produk tidak mendukung macOS 26 |
| Pengganti `attestation` | field dihapus, probe `manifest` di spike | URL halaman release; Sigstore `cosign attest-blob` | URL release hanya lokasi; cosign butuh alat baru, login OIDC, dan menulis ke log publik di setiap build |
| Persetujuan lisensi | gerbang sebelum unggahan pertama, per provider (dirinci setelah cross check) | setujui sekarang | persetujuan sebaiknya berdasar isi `LICENSES/` artefak nyata, bukan daftar SPDX di spec |
| Urutan terbit | `pin --file`, spike lokal `PASS`, baru unggah | unggah dulu, lalu `pin` dari URL | artefak rusak bisa terbit dan tetap publik sampai dihapus |
| Tool build | venv `/usr/bin/python3` 3.9.6: meson 1.11.2, ninja 1.13.2, pkgconf 2.4.3.post2 | CPython standalone yang dipin; meson Homebrew | standalone menambah satu artefak toolchain untuk dirawat; Homebrew membuat build bergantung lagi pada versi yang berubah saat `brew upgrade` |

Catatan untuk letak resep. Rekomendasi saya resep di repo ini, karena `runtime check` bisa memastikan file yang dirujuk `recipe` ada tanpa jaringan, dan tag release bisa menunjuk commit yang sama dengan resepnya. Pilihan kamu tetap bekerja, dengan dua harga yang perlu kamu terima dengan sadar: `runtime check` hanya bisa memeriksa bentuk `recipe` (pemilik, repo, commit 40 hex, path), dan tag `runtime/<id>` di repo `stackly` tidak menunjuk commit resep. Celah itu ditutup sebagian oleh probe `manifest`: artefak sendiri menyatakan commit resepnya, dan spike menolak artefak yang klaimnya berbeda dari entri. Keuntungannya juga nyata: skrip build pihak ketiga dan riwayatnya tidak bercampur dengan kode app, dan repo resep bisa dipakai tanpa membawa seluruh workspace Rust.

### Keputusan turunan (rekomendasi saya, dengan runner up)

- **Root build di `/private/tmp/stackly-build/<8 hex acak>/`**, di luar home. Path yang tertanam di binary (prefix staging, `__FILE__`) jadi tidak memuat nama user tanpa remap path source, yang dilarang AGENTS.md. Namanya acak, bukan id artefak, karena PostgreSQL memakai layout datar kalau prefix memuat `postgres` atau `pgsql`, dan id artefak memuat `postgresql`. Runner up: build di `out/` checkout dengan `-ffile-prefix-map`.
- **Preflight `/usr/local`**, karena clang dan linker selalu mencari di sana lebih dulu walau SDK dipakai (inventaris di atas). Audit statis menangkap dylib dari `/usr/local/lib`, tapi tidak menangkap header yang menimpa header SDK atau dependency build. Aturannya umum: semua file header di tingkat puncak ditolak (misalnya `iconv.h` dari GNU libiconv, penyebab klasik error link), begitu juga folder header milik build, sedangkan folder lain seperti `node/` lolos karena hanya terjangkau lewat `#include <node/...>`. Runner up: daftar nama yang ditolak (versi awal, diganti setelah cross check karena tidak mungkin lengkap), atau hanya mengandalkan audit.
- **File `.pc` buatan sendiri untuk libxml2, libedit, dan zlib SDK**, karena meson PostgreSQL tidak menemukan libxml2 tanpa pkg-config atau CMake, dan fallback libedit-nya mencari nama yang salah untuk SDK. Runner up: libxml2 dibangun dari source dan dibundel (dependency bundel kelima, dengan patch keamanannya sendiri), atau `-Dlibxml=disabled` (tipe dan fungsi XML hilang).
- **bison, flex, perl, dan make dari `/usr/bin`**, karena semuanya memenuhi minimum PostgreSQL 17 dan 18. Runner up: bison terbaru dibangun dari source sekarang; ditunda sampai PostgreSQL menaikkan minimumnya (Follow-up).
- **Hanya `/opt/homebrew` yang dicari di log konfigurasi**, bukan `/usr/local`, karena path SDK dan jalur cari bawaan clang yang sah ikut tertulis di log dan memuat `/usr/local`. Preflight dan audit menjaga `/usr/local`.
- **Path home dicari di seluruh isi paket**, bukan hanya di log, karena paket diterbitkan publik. Nama user saja tidak dicari, karena muncul sah di `recipe` (`ojiepermana/stackly-runtimes@...`).
- **Smoke bash dihapus, diganti spike lokal ditambah cek versi.** Smoke lama meniru langkah spike di bash, sehingga ada tiga tempat yang tahu cara menjalankan PostgreSQL. Cek `--version` dari salinan paket di path berisi spasi dan `Ü` sudah menangkap error relokasi (dyld gagal memuat library) sebelum `pin --file`. Runner up: smoke bash penuh.
- **`pin --file` menyalin ke cache**, bukan perintah baru, karena cache sudah bernama digest dan spike membaca cache sebelum mengunduh. Hasilnya, spike lokal sebelum unggah memakai jalur kode yang sama dengan run evidence. Runner up: flag `--local-file` di `runtime spike`.
- **Valkey dulu**, sesuai pendekatan Journey: artefak paling kecil tanpa dependency bundel membuktikan seluruh jalur (build, spike, terbit, bukti) sebelum OpenSSL dan ICU menambah risiko. Runner up: PostgreSQL 18 dulu karena paling penting bagi G0.
- **Tag `runtime/<id>`**, karena tag dengan `/` bekerja di URL unduhan (cek lanskap) dan memisahkan rilis runtime dari tag app `v*`. Runner up: `runtime-<id>`.
- **Paket dengan `--no-mac-metadata --uid 0 --gid 0 --uname root --gname wheel`**, supaya tarball tidak membawa entri `._<nama>` (follow-up fitur 18 tentang AppleDouble di `tar_gz`) atau nama user OS. Runner up: `COPYFILE_DISABLE=1` tanpa opsi pemilik.
- **Repo resep berlisensi MIT OR Apache-2.0**, sama dengan repo ini. Runner up: lisensi tunggal MIT.
- **`LANG=C` saat build**, supaya keluaran tool dan urutan sort tidak bergantung locale Mac. Python 3.9 tetap memakai UTF-8 di locale C (PEP 538).
- **Log build tidak diunggah**, karena memuat path checkout di home; isi yang perlu dibaca orang lain (source, resep, toolchain, dependency) sudah ada di `STACKLY-ARTIFACT.json` dan catatan release.
- **Recipe `Postgres` dan `Valkey` di `providers.rs` dinamai di spec**, karena build plan lama hanya menyebut test native, padahal tanpa recipe driver menulis `the spike has no steps for provider postgresql`. `stop` PostgreSQL memakai `server.signal("INT")` yang sudah ada (fast shutdown), dan port tetap dialokasikan walau `listen_addresses` kosong, karena nomor port menentukan nama socket.

- **Isi `STACKLY-ARTIFACT.json` dengan `/usr/bin/jq -n`**, yang ada di macOS 27, supaya string di-escape dengan benar tanpa menulis JSON dengan `printf`. Runner up: Python venv.
- **lz4 dan zstd tetap dylib bundel**, bukan ditautkan statis seperti saran cross check. Link statis membuang dua dylib dan relokasinya, tapi kode pihak ketiga lalu hilang dari audit statis dan dari baris `bundled`, sehingga aturan "inventaris sama dengan audit" (AC-7) tidak lagi melihatnya dan asal serta lisensinya hanya tercatat di manifest.

### Cross check update

Update ini dibaca model lain (Sonnet 5.5, baca saja) sebelum diterima. Klaim yang paling berat saya cek ulang ke kode (`plan.rs`, `worker.rs`, `cache.rs`, `checksum.rs`, `inventory.rs`, `/usr/bin/jq`) dan ke sumber upstream (`src/Makefile` Valkey 9.1.2, `meson_options.txt` dan `meson.build` PostgreSQL REL_18_STABLE). Semua rekomendasi diterapkan atas persetujuan kamu:

- variabel yang menyeberangi `env -i` (`STACKLY_ORIGINAL_HOME`, `STACKLY_RECIPE`, `STACKLY_BUILD_ROOT`, `STACKLY_BUILD_ARGS`);
- gerbang lisensi per provider, dua checkbox, sehingga Valkey tidak menunggu PostgreSQL dan gerbang PostgreSQL mencakup OpenSSL, ICU, lz4, dan zstd;
- `libraries` dan `vendored` di manifest, pemetaan dylib ke baris `bundled`, `install_name` `@rpath/<file>`, perluasan `PlanArtifact` dan `PlanDependency`, dan probe `dependencies` serta `manifest` yang selalu dicatat sebelum `closure` gagal;
- algoritma relokasi lengkap (symlink berversi menjadi satu file, `-id`, `-change` untuk semua rujukan ke root build, rpath per Mach-O), isi paket dari stage, dan `make install_sw` untuk OpenSSL;
- opsi meson PostgreSQL: `--buildtype=release`, `uuid.pc` (meson `dependency('uuid')` tidak punya pencari sistem), opsi `auto` lain dimatikan, `libcurl`, `libnuma`, dan `liburing` khusus 18 dengan daftar opsi per versi mayor, dan cipher lama pgcrypto dicatat di luar build;
- langkah 15 harus di-merge sebelum langkah 17;
- SPDX dependency bundel, kode vendored Valkey di `LICENSES/`, dan sumber `license_url`;
- commit resep harus sudah ada di remote sebelum unggah;
- nama file `--url` sama dengan nama file lokal di `pin --file`, dan file disalin ke cache;
- `runtime spike --no-quarantine` untuk spike lokal, supaya probe quarantine (dan dialog Gatekeeper-nya) tidak jalan di setiap run lokal digest baru;
- aturan umum preflight `/usr/local`;
- sumber hash `requirements.txt`, penulis JSON manifest, dan baris pertama saja untuk string toolchain.

Perbaikan yang langsung diterapkan karena bukan keputusan: `make install USE_REDIS_SYMLINKS=no` (opsi itu ada di Makefile 9.1.2, dan `valkey-check-aof`, `valkey-check-rdb`, serta `valkey-sentinel` dipasang sebagai symlink ke server), cek `pg_config --sharedir` dan `--pkglibdir` dari salinan paket (cek `--version` saja hanya membuktikan dyld, bukan path relatif), kalimat tentang data ICU, dan catatan bahwa entri yang masih membawa `attestation` gagal sebagai TOML tidak valid. Temuan bahwa `-Drpath=false` perlu ternyata tidak berlaku: `meson.build` PostgreSQL tidak pernah menambah rpath di macOS dan memakai install name absolut, yang sekarang ditangani langkah relokasi 3. Saran link statis lz4 dan zstd ditolak (lihat Keputusan turunan).

## Update 2026-10-18: kedua repo pindah ke org `stackly-dev`

Sebelum unggahan pertama, kedua repo GitHub pindah ke org `stackly-dev`. Repo resep sekarang `stackly-dev/runtimes` (sebelumnya direncanakan `ojiepermana/stackly-runtimes`), dan repo utama sekarang `stackly-dev/stackly` (sebelumnya `ojiepermana/stackly`). GitHub mengalihkan nama lama, tapi kamu memilih memakai nama baru di semua tempat sebelum artefak terbit, karena `recipe` di dalam artefak dan URL di inventory tidak bisa diubah lagi setelah dipin dan diunggah.

Yang berubah di `index.md`: setiap penyebutan kedua repo (Summary, AC-17, Decision, tabel sumber, field `recipe`, tabel API, perintah `gh release create`, bagian repo resep, alur `build.sh`, Value sourcing, prasyarat, langkah 12 Milestone 3, Consequences, dan Follow-up). Contoh di `verify.md` dan fixture xtask (`inventory.rs`, `pin.rs`, `check.rs`, `spike/closure.rs`) ikut diganti. Yang sengaja tetap:

- checkout lokal tetap `~/Development/ojiepermana/stackly-runtimes`, karena foldernya memang di sana;
- pesan `stackly-runtimes has uncommitted changes` di AC-17, karena `build.sh` masih mencetaknya;
- owner default `@ojiepermana` untuk entri `blocked`, karena itu handle orang, bukan repo;
- bagian lain di rationale ini (inventaris, tabel opsi) tetap memakai nama lama, sebagai catatan keadaan saat keputusan dibuat.

Ketiga artefak dibangun ulang dari commit resep `38930060759d2e7f5fed000a5f1d82aa9ada7664`, supaya `recipe` di `STACKLY-ARTIFACT.json` berbunyi `stackly-dev/runtimes@...`. Probe `manifest` membandingkan string itu dengan entri, jadi artefak yang dibangun dengan nama lama tidak lulus spike.

Di luar spec ini, `repository` di `Cargo.toml` dan spec 0001 (`[workspace.package]`) serta URL remote `origin` checkout ini masih memakai `ojiepermana/stackly`. Itu keputusan spec 0001, jadi dicatat di Follow-up, bukan diubah di sini.

## Update 2026-10-18: notice pihak ketiga dari header source

### Konteks

Di gerbang lisensi Valkey, artefak `valkey-9.1.2-r1-macos-aarch64` sudah lulus spike lokal, dan `LICENSES/`-nya memuat `COPYING` Valkey serta teks lisensi enam komponen vendored di `deps/`. Tapi folder `src/` Valkey 9.1.2 juga memuat kode dari penulis lain yang notice-nya hanya ada di header file, misalnya `ae_kqueue.c` (Harish Mallipeddi), `crc16.c` (Georges Menie), `crc64.c` dan `crcspeed.h` (Matt Stancliff), `crcspeed.c` dan `crccombine.c` (Mark Adler), `geohash*.c` (yinqiwen), lzf (Marc Lehmann), dan `mt19937-64`. PostgreSQL 18.6 punya pola yang sama di `src/` dan `contrib/`: Marko Kreen (pgcrypto, 25 file), Henry Spencer (regex, 12 file), Sun Microsystems (8), Philip Warner (pg_dump, 7), ISC (3), dan beberapa lainnya. Sebagian besar lisensi itu gaya BSD, yang meminta notice-nya ikut di distribusi binary, sementara GitHub Releases sudah distribusi publik.

### Opsi

| Opsi | Pilihan | Kenapa |
| --- | --- | --- |
| Setujui r1 sekarang, celah dicatat sebagai follow-up | tidak | artefak r1 tetap publik tanpa notice itu sampai build berikutnya |
| Perbaiki dulu, ketiga artefak dibangun ulang sebagai r2 | dipilih kamu (rekomendasi saya) | satu putaran build untuk ketiganya, dan artefak publik pertama sudah membawa semua notice |
| Valkey saja r2, PostgreSQL dibahas di gerbangnya nanti | tidak | kemungkinan butuh putaran build kedua untuk masalah yang sama |
| Notice di `LICENSES/<provider>.txt` setelah teks lisensi utama | dipilih kamu (rekomendasi saya) | probe `manifest` tidak berubah, dan tidak ada aturan nama file baru |
| Notice di file terpisah `LICENSES/<provider>-notices.txt` | tidak | teks lisensi utama tetap persis seperti upstream, tapi spec mendapat aturan nama file baru |

### Keputusan

Setiap resep provider memuat daftar pemilik provider dan daftar file notice. Build memindai folder source provider dan gagal kalau ada file berheader copyright pihak lain yang belum didaftar, supaya patch berikutnya yang menambah file seperti itu tidak lolos diam diam. Daftar pemilik ditulis di resep, bukan di spec, karena nilainya bergantung pada isi source versi itu. Pindaian sengaja longgar: baris `Copyright` yang nama pemiliknya ada di baris berikutnya ikut dianggap pihak lain, sehingga kelebihan notice lebih mungkin daripada kekurangan.

## Landscape scan 2026-10-17

Cek web sekali saat wawancara (5 pencarian, 8 halaman). Yang tertulis "belum terkonfirmasi" dikonfirmasi saat `runtime pin`.

| Provider | Temuan |
| --- | --- |
| PostgreSQL | Rilis minor terbaru 17.11 dan 18.6. Tidak ada archive relocatable resmi untuk macOS arm64. Zip macOS EDB belum terkonfirmasi; panduan terbaru menyebut installer `.dmg`. `theseus-rs/postgresql-binaries` punya `postgresql-<v>-aarch64-apple-darwin.tar.gz` dan `.sha256` untuk 17.11.0 dan 18.6.0 (terbit 15 Agustus 2026), tanpa signature. |
| MySQL | Halaman unduhan mendaftar 9.7.2 LTS dan 8.4.11 LTS, serta 26.7.0 sebagai GA teratas tanpa label LTS. Nama file tarball macOS ARM64, pola URL, signature untuk file macOS, dan perpindahan URL ke arsip belum terkonfirmasi. |
| NATS | v2.15.1 (9 Oktober 2026), aset `nats-server-v2.15.1-darwin-arm64.tar.gz`, `SHA256SUMS`, SBOM per aset, tanpa signature. |
| ClickHouse | Stable terbaru v26.9.20.23 (16 Oktober 2026) membawa `clickhouse-macos-aarch64` dan versi `.zip` aset itu; file `.sha512` hanya untuk paket Linux. Lini LTS 26.3; aset macOS di rilis LTS belum terkonfirmasi. Dikoreksi saat milestone 2: 26.8 sudah LTS (`v26.8.29.10-lts`) dan membawa kedua aset, dan aset polosnya binary self extracting (lihat Update 2026-10-17: format `zip` untuk ClickHouse). |
| Valkey | 9.1.2 (1 September 2026), rilis GitHub tanpa aset. Binary macOS resmi belum terkonfirmasi ada di tempat lain. |
| Traefik | v3.7.15 (15 Oktober 2026), aset `traefik_v3.7.15_darwin_arm64.tar.gz`, `traefik_v3.7.15_checksums.txt`, tanpa signature. |

## References

**Project sources**:

- `prd.md`: bagian 5.2, 20.1, 20.2, 23, 25, 27, 28, 32.2, 33.2, 34 (G0), 37 (RISK-01, RISK-12), 38 (OQ-003, OQ-004, OQ-012), 39; REQ-004, REQ-005, REQ-049, AC-078; sumber [S14] (notarization Apple) dan [S16] (manpage Homebrew).
- `docs/scope/01-foundation.md` fitur 3, serta fitur 15, 18, 55, 56, dan 60 di scope lain.
- Spec 0001 (stack, `cargo xtask`, kebijakan `unsafe`) dan spec 0002 (evidence `stackly.evidence/v1`, follow up fitur 3).
- `AGENTS.md` root (aturan workspace, testkit).
- `runtimes/inventory.toml` (entri ClickHouse `blocked`), `xtask/src/runtime/inventory.rs` (aturan 3), `xtask/src/runtime/spike/archive.rs` (ekstraksi), `xtask/src/runtime/pin.rs` (`min_os`), dan man page bsdtar di macOS 27 (`-P`, `--no-mac-metadata`).
- Skill terpasang: `rust-best-practices`, `meson`, `signing-entitlements`.
- Update 2026-10-18: `xtask/src/runtime/inventory.rs` (aturan 5 dan fixture `stackly-runtimes`), `xtask/src/runtime/spike/providers.rs` (trait `Recipe`, belum ada PostgreSQL dan Valkey), `xtask/src/runtime/spike/worker.rs` (urutan langkah dan `closure`), `xtask/src/runtime/cache.rs` (cache bernama digest), `xtask/src/runtime/pin.rs` (pencarian `--executable`), `xtask/tests/native_runtime.rs`, dan inventaris toolchain Mac ini (`clang -v`, `ld -v`, isi SDK).

**Practices & standards**:

- Pin artefak dengan digest kriptografis, URL hanya sebagai lokasi.
- Relokasi Mach-O dengan `@rpath` dan `@loader_path`, lalu tanda tangan ulang setelah mengubah binary.
- Build terisolasi: environment kosong (`env -i`) dan setiap input (source, wheel) dipin dengan digest.
- Artefak yang sudah terbit tidak pernah diganti; versi baru berarti nomor build baru.

**Links** (terverifikasi saat cek lanskap):

- PostgreSQL versioning policy: https://www.postgresql.org/support/versioning/
- theseus-rs postgresql-binaries, rilis terbaru: https://api.github.com/repos/theseus-rs/postgresql-binaries/releases/latest
- MySQL Community Server downloads: https://dev.mysql.com/downloads/mysql/
- NATS server, rilis terbaru: https://api.github.com/repos/nats-io/nats-server/releases/latest
- ClickHouse, rilis terbaru: https://api.github.com/repos/ClickHouse/ClickHouse/releases/latest
- Valkey, rilis terbaru: https://api.github.com/repos/valkey-io/valkey/releases/latest
- Traefik, rilis terbaru: https://api.github.com/repos/traefik/traefik/releases/latest

Ditambahkan saat cek lanskap 18 Oktober 2026:

- meson 1.12.1 dan 1.11.2 di PyPI: https://pypi.org/pypi/meson/1.12.1/json dan https://pypi.org/pypi/meson/1.11.2/json
- ninja 1.13.2 di PyPI: https://pypi.org/pypi/ninja/1.13.2/json
- pkgconf di PyPI: https://pypi.org/pypi/pkgconf/3.0.7.post0/json dan https://pypi.org/simple/pkgconf/
- PostgreSQL 18.6 release notes: https://www.postgresql.org/docs/release/18.6/
- PostgreSQL roadmap rilis minor: https://www.postgresql.org/developer/roadmap/
- PostgreSQL 18 build requirements: https://www.postgresql.org/docs/18/install-requirements.html
- PostgreSQL `meson.build` REL_18_STABLE: https://raw.githubusercontent.com/postgres/postgres/REL_18_STABLE/meson.build
- Commit "Remove distprep": https://github.com/postgres/postgres/commit/721856ff24b3722ce8e894e5a32c9c063cd48455
- OpenSSL source dan kebijakan rilis: https://openssl-library.org/source/ dan https://openssl-library.org/policies/releasestrat/
- ICU 78.3: https://github.com/unicode-org/icu/releases/tag/release-78.3
- lz4 1.10.0: https://github.com/lz4/lz4/releases/tag/v1.10.0
- zstd 1.5.7: https://github.com/facebook/zstd/releases/tag/v1.5.7
- Valkey 9.1.2 README: https://raw.githubusercontent.com/valkey-io/valkey/9.1.2/README.md
- valkey-hashes: https://raw.githubusercontent.com/valkey-io/valkey-hashes/main/README
- GitHub Releases: https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases
