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

- Pipeline `stackly-runtimes` (runner hosted, tag terlindungi, release, attestation) dihapus. OQ-003 tetap (build maintainer diperlukan), tapi cara membangun PostgreSQL dan Valkey di Mac ini, termasuk deployment target 26.0 yang dulu mengikuti SDK runner hosted, menunggu `/architect`.
- Watch mingguan (`runtime watch`, `runtime-watch.yml`, AC-14) dan wiring `native.yml` (AC-15) dihapus, jadi bump versi tanpa pengingat; `--cleanup` dan `--prune-cache` tetap perintah lokal.
- Penolakan run `pull_request` di `runtime record` dan aturan 8 dihapus dari spec, walau kodenya masih ada.

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

**Practices & standards**:

- Pin artefak dengan digest kriptografis, URL hanya sebagai lokasi.
- Relokasi Mach-O dengan `@rpath` dan `@loader_path`, lalu tanda tangan ulang setelah mengubah binary.

**Links** (terverifikasi saat cek lanskap):

- PostgreSQL versioning policy: https://www.postgresql.org/support/versioning/
- theseus-rs postgresql-binaries, rilis terbaru: https://api.github.com/repos/theseus-rs/postgresql-binaries/releases/latest
- MySQL Community Server downloads: https://dev.mysql.com/downloads/mysql/
- NATS server, rilis terbaru: https://api.github.com/repos/nats-io/nats-server/releases/latest
- ClickHouse, rilis terbaru: https://api.github.com/repos/ClickHouse/ClickHouse/releases/latest
- Valkey, rilis terbaru: https://api.github.com/repos/valkey-io/valkey/releases/latest
- Traefik, rilis terbaru: https://api.github.com/repos/traefik/traefik/releases/latest
