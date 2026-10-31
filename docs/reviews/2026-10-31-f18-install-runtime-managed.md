# Review, F18 install runtime managed (spec 0018), 2026-10-31

**Reviewed by**: Opus 5.5 (tiga subagent review paralel di dalam `/check review`, per irisan; penulis kode Opus 5.5 dan Sonnet 5.5 menurut trailer `Co-Authored-By`, reviewer ditetapkan brief lead wave 5)
**Scope**: 55 file, `MODE=branch`, `BASE=main`, `MERGE_BASE=afc7c52c^`, range `afc7c52c^..6a2e4248`, dibatasi ke path Pointer fitur 18: `crates/core/src/runtime/`, `crates/platform/src/{fetch,codesign,info}.rs`, `crates/platform-macos/src/{fetch,codesign,info}.rs`, `crates/storage/migrations/0005_runtime_install.sql`, `crates/domain/src/registry/install.rs`, `crates/protocol/src/runtime.rs`, `apps/daemon/src/ipc/runtime.rs`, `apps/cli/src/commands/runtime/`, `xtask/src/runtime/scan.rs`, `apps/daemon/tests/native_runtime_install.rs`, `crates/testkit/src/runtime_cache.rs`, `crates/core/src/provider/versions.rs`, `crates/core/src/catalog/status.rs`, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Runtimes/`. Range itu juga memuat commit fitur lain; file di luar path ini diabaikan. Nomor baris mengacu ke `6a2e4248`.
**Verdict**: Changes requested

Irisan review: A (extractor, Mach-O, closure, validate), B (operasi install, download, commit, platform, domain, migrasi, testkit, xtask scan), C (protocol, daemon, CLI, test native, versions.json, status katalog, SwiftUI Runtimes).

## Summary

F18 menambah instalasi runtime managed: operasi `install_runtime` lima tahap yang bisa dilanjutkan di atas engine spec 0007, cache download yang bisa di-resume, extractor aman (`tar_gz`, `zip`, binary) lewat `TreeWriter`, parser load command Mach-O, cek closure statis, validasi (signing, quarantine, cek versi), commit atomik dengan pengambilalihan orphan, DTO dan handler `runtime.*`, tiga verb CLI, field fase 02 `versions.json` dengan record kompatibilitas, dan halaman Runtimes di SwiftUI.

Pekerjaannya kuat. Tidak ditemukan path traversal, symlink escape, zip bomb, atau jalur panic pada input jahat; semua batas ukuran dicek sebelum menulis atau mengalokasi; fetcher mengecek ulang `https` di tiap redirect; adapter codesign patuh aturan spawn dan kill; aturan wire protocol terpenuhi; `runtime.install` melakukan replay sebelum membaca katalog atau registry. Tidak ada Blocker.

Satu Major: inspector menampilkan record kompatibilitas yang bukan penentu verdict support, karena kontrak berjanji "newest first" tetapi daemon mengirim urutan katalog. Minor terberat: pembacaan quarantine yang gagal terbuka saat atribut panjang, digest mismatch pada percobaan download terakhir yang dilaporkan dengan kode salah, batch akhir yang tidak memakai `commit_guarded`, dan jalur tepi CLI (`--output id`, `alreadyInstalled`, `--idempotency-key`).

Jumlah: Blocker 0, Major 1, Minor 20, Nit 19.

## Major

### 🟠 Inspector menampilkan record yang tidak menentukan verdict support, dan `RuntimeDetail.compatibility` tidak "newest first", `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Runtimes/RuntimeInspector.swift:114`
**Problem**: Baris Support menaruh `detail?.compatibility?.first` di bawah badge sebagai record penentu (spec 0018 *Halaman Runtimes*: "record penentunya"). Doc DTO di `crates/protocol/src/runtime.rs:521` menjanjikan "newest first". Daemon tidak mengurutkan: `map::runtime::runtime_detail` (`apps/daemon/src/map/runtime.rs:198`) menyalin baris registry, dan storage membacanya `ORDER BY position` (`crates/storage/src/registry/read.rs:173`), yaitu urutan `versions.json`. Verdict sendiri datang dari `support_status` (`crates/core/src/runtime/compatibility.rs:83`), yang memilih record `provider` lebih dulu. `providers/nats/versions.json` mendaftar record `runtime` 27.0 (2026-10-18) dulu, lalu dua record `provider` 27.0.1 yang lebih baru.
**Why it matters**: NATS 2.15.1 tampil "Supported (provider_tested)", tetapi di bawahnya tertulis record runtime only 27.0 yang hanya membenarkan Experimental. Record `fail` atau dari major macOS lain juga bisa muncul pertama di bawah badge Supported. Ini merusak keterlacakan BR-025 yang ingin ditunjukkan inspector, dan klien lain yang percaya kontrak "newest first" ikut tersesat.
**Suggested fix**: Daemon menyebut record penentu (`SupportVerdict.record` sudah ada): tambah field opsional `decidingRecord` di `RuntimeDetail`, atau urutkan dengan record penentu di depan. Samakan doc comment dengan yang dikirim; bila "newest first" tetap kontraknya, urutkan menurut `tested_at` menurun di `runtime_detail`. Tambah test daemon dengan record berurutan acak. Ini perubahan kontrak, jadi naikkan `ProtocolVersion::CURRENT`.

## Minor

### 🟡 Atribut quarantine yang panjang membuat `check_launchable` gagal terbuka, `crates/platform-macos/src/codesign.rs:107`
**Problem**: `quarantine` memakai buffer tetap 512 byte. Nilai `com.apple.quarantine` yang lebih panjang membuat `getxattr` mengembalikan `ERANGE`, yang dipetakan ke `Err`. `runtime::launch::check_launchable` (`crates/core/src/runtime/launch.rs:47-55`) mencatat `Err` itu lalu mengembalikan `Ok(())` (disengaja untuk atribut yang tidak terbaca, lihat doc baris 39). Jalur install (`validate.rs` `check_quarantine`) gagal tertutup, tetapi start instance spec 0010 memakai `check_launchable`.
**Why it matters**: `ERANGE` berarti atribut ada, jadi executable yang ter-quarantine dijalankan tanpa diagnosis `runtime.quarantined`.
**Suggested fix**: Perlakukan `ERANGE` sebagai `Ok(Some(QuarantineInfo { agent: None }))`, atau ukur buffer dulu dengan `getxattr(…, NULL, 0, …)`. Pertimbangkan gagal tertutup di `check_launchable` untuk error apa pun selain `ENOATTR`.

### 🟡 Download yang di-resume dan mismatch pada percobaan terakhir berakhir `download_failed` dan tidak pernah diulang dari 0, `crates/core/src/runtime/download.rs:430`
**Problem**: Saat download yang di-resume selesai dengan ukuran atau digest salah, `finish_entry` mengubahnya menjadi `Attempt::Fetch(FetchError::RangeNotSatisfiable)` agar loop mulai lagi dari 0. Loop hanya mengulang selama `attempt + 1 < ATTEMPTS_PER_URL` (baris 269-281); pada percobaan ketiga ia berhenti, dan `into_error` mengembalikan `DownloadError::Failed`. Selain itu `finish_entry` menerima `started_at` lokal (baris 336), bukan `sink.started_at` yang di-reset `start()` ke 0 pada jawaban 200 Full, sehingga download penuh yang dikirim dengan `Range` juga dianggap resume dan diambil sekali lagi.
**Why it matters**: Dua percobaan terputus, percobaan ketiga resume dengan prefix rusak: download baru dari 0 yang spec minta (*Download dan cache* langkah 6) tidak terjadi, dan dengan satu URL operasi gagal `runtime.download_failed` (retryable), bukan `runtime.digest_mismatch`/`size_mismatch` (AC-4).
**Suggested fix**: Tangani restart dari 0 setelah mismatch di dalam `attempt_once` atau di luar jatah percobaan per URL, dan simpan error mismatch untuk dilaporkan bila restart juga gagal. Berikan `sink.started_at` ke `finish_entry`. Tambah test mismatch pada percobaan ketiga.

### 🟡 Batch akhir memakai `expected_generation` dan loop retry engine, bukan `commit_guarded`, `crates/core/src/runtime/install.rs:1757`
**Problem**: `final_batch` membaca `registry.runtimes()` untuk menegakkan "satu runtime managed per sha256", aturan yang datang dari registry, lalu menulis dengan `expected_generation: Some(read.generation)`. AGENTS.md mewajibkan tulisan seperti ini lewat `OperationContext::commit_guarded` tanpa loop `expected_generation` (spec 0020 Keputusan 21).
**Why it matters**: Setiap tulisan registry menaikkan generation. Operasi yang berjalan bersamaan bisa menghabiskan `FINALIZE_RETRIES` (5), dan kegagalan sesudah titik tanpa kembali membuat install `needs_recovery` sehingga user harus melanjutkannya. Tiap retry juga memindai seluruh tabel runtimes.
**Suggested fix**: Nyatakan recheck plus `InsertArtifact` + `InsertRuntime` sebagai `CommitGuard`, seperti `instance/create/work.rs:630`, dan buang generation. Bila finalize engine belum bisa menerima guard, tandai `// provisional: F<n>` dan catat penyimpangannya.

### 🟡 `--output id` mencetak runtime ID yang tidak ada saat install gagal atau wait habis, `apps/cli/src/commands/runtime/install.rs:300`
**Problem**: Dalam mode `id_only`, `render` mencetak `runtime_id` selama `Some`. `runtime_of` (baris 179) jatuh ke `operation.resource`, runtime yang *akan* dibuat. Dengan `--wait` dan operasi gagal atau batal, atau `--timeout` habis, `conclude` membangun `CommandOutput::partial`, dan `Output::emit` tetap mencetak hasilnya ke stdout.
**Why it matters**: `id=$(stackly runtime install official/nats@stable --output id)` di skrip yang tidak mengecek `$?` mendapat UUID yang dijawab `resource.not_found` oleh `runtime inspect`. AC-16 menyiratkan output hanya saat sukses.
**Suggested fix**: Di `id_only`, cetak ID hanya bila operasi `Succeeded` atau `already_installed`. Tambah test CLI untuk install gagal dan timeout dengan `--output id` yang mengharapkan stdout kosong dan exit bukan nol.

### 🟡 `alreadyInstalled` membuang `RuntimeSummary` dari daemon dan bisa menulis ID kosong dengan exit 0, `apps/cli/src/commands/runtime/install.rs:169`
**Problem**: `extract` memetakan `InstallOutcome::AlreadyInstalled(summary)` ke `Accepted` tanpa summary. `conclude` (baris 235-246) lalu memanggil `all_runtimes` dan mencari menurut digest, dan menelan error dengan `.ok()`.
**Why it matters**: Satu round trip berhalaman tambahan. Bila list gagal atau berlomba dengan penghapusan, `runtime_id` menjadi `None` dan `--output id` tidak menulis apa pun dengan exit 0; baris human juga kehilangan `(runtime <id>)`.
**Suggested fix**: Bawa summary lewat submission (slot samping atau field opsional di `Accepted`), lalu pakai `summary.id` dan `summary.display_name` langsung tanpa list.

### 🟡 CLI memasukkan digest yang bergantung katalog ke fingerprint, sehingga retry dengan `--idempotency-key` yang sama bisa konflik, `apps/cli/src/commands/runtime/install.rs:147`
**Problem**: CLI melakukan resolve dulu, lalu mengirim `runtime.install` dengan `expected_sha256: Some(resolved.sha256)`, dan `request_fingerprint` mencakup semua params. User menjalankan `install official/postgresql@17 --idempotency-key K`, koneksi putus, lalu retry setelah katalog memindah line 17: digest baru, fingerprint berbeda, daemon menjawab idempotency mismatch.
**Why it matters**: Daemon sengaja replay sebelum membaca katalog; CLI membatalkan jaminan itu tepat untuk kasus kunci itu dibuat, retry setelah hasil tidak diketahui.
**Suggested fix**: Bila user memberi `--idempotency-key`, kirim `expected_sha256` hanya kalau user memberi `--expect-sha256`; coba `runtime.install` dulu, lalu resolve untuk tampilan. Minimal dokumentasikan perilakunya dan beri hint pada mismatch.

### 🟡 `@rpath` sebuah executable juga diselesaikan lewat `LC_RPATH` executable lain, `crates/core/src/runtime/closure.rs:329`
**Problem**: `resolve` menambah rpath setiap `MH_EXECUTE` di pohon (`mains`) ke setiap lookup `@rpath/`, juga ketika loader sendiri executable utama. dyld hanya memakai rpath sepanjang rantai load image itu sendiri. Kelonggaran yang sama berlaku untuk `@executable_path` pada library (baris 319-323): folder `MH_EXECUTE` mana pun diterima, padahal spec menyebut "folder setiap executable artefak".
**Why it matters**: `bin/initdb` memuat `@rpath/libx.dylib` tanpa `LC_RPATH`, `bin/postgres` punya `@loader_path/../lib`: cek closure lulus, `initdb` gagal saat launch dengan "Library not loaded". Cek versi hanya menjalankan executable pertama, jadi tidak ada yang menangkapnya sebelum install.
**Suggested fix**: Saat `slice.is_executable()`, pakai rpath miliknya saja. Untuk library, batasi `exe_dirs` dan rpath utama ke `input.executables` seperti spec, atau catat union itu sebagai aproksimasi yang disengaja di spec. Tambah test dua executable dengan rpath hanya di satu.

### 🟡 Error baca pada file staged diam diam mengeluarkannya dari pemindaian Mach-O, `crates/core/src/runtime/validate.rs:584`
**Problem**: `std::io::Read::read(&mut file, &mut head).unwrap_or(0)` mengubah error I/O menjadi "0 byte terbaca"; satu `read` juga sah mengembalikan kurang dari 8 byte. Dalam dua kasus itu file dianggap bukan Mach-O.
**Why it matters**: File Mach-O yang kepalanya tidak terbaca lolos dari cek closure, peringatan arm64, dan `assess`/`sign_adhoc`; di bawah `keep_or_adhoc` dylib tak bertanda tangan bisa lolos dan gagal saat load. Error yang ditelan juga melanggar aturan error proyek.
**Suggested fix**: Pakai `read_exact` atas `min(8, len)` byte (atau loop sampai 8 byte atau EOF) dan petakan error nyata lewat `io_error("read a staged file", …)`.

### 🟡 Tahap extract meng-hash entry cache lalu membacanya ulang, bukan meng-hash stream yang di-extract, `crates/core/src/runtime/install.rs:1377`
**Problem**: `hash_file` membaca seluruh entry, `seek(0)`, lalu `extract` membaca kedua kalinya lewat fd yang sama. Penggantian lewat rename tertangkap, tulisan di tempat di antara dua bacaan tidak. Tabel tahap dan *Key invariants* spec menyebut SHA-256 dihitung atas stream yang dibaca saat extract, dan `stage/` dibuang bila digest berbeda di akhir.
**Why it matters**: Invarian yang dinyatakan melemah (byte yang di-extract tidak terbukti sama dengan yang di-hash), dan artefak dibaca tiga kali per install. Proses user yang sama adalah risiko residual di *Security model*, jadi ini pertahanan berlapis, bukan exploit.
**Suggested fix**: Bungkus reader dengan adapter `Read` yang meng-hash, bandingkan digest setelah `extract` selesai dan sebelum `finish_tree`. Zip melakukan seek, jadi untuk zip pertahankan cara ini dan catat sebagai penyimpangan spec.

### 🟡 Error I/O download membawa path home lengkap ke `message` di wire, `crates/core/src/runtime/download.rs:151`
**Problem**: `DownloadError::Io` menampilkan `{op} failed in the download cache: {detail}`, dan `detail` adalah `PlatformError::to_string()` yang memuat `path.display()` (path `~/Library/Caches/.../downloads/<sha>.part` yang sudah diekspansi). `InstallError::Download` menampilkan `{source}`, dan runner memakai `error.to_string()` sebagai `message` kegagalan.
**Why it matters**: *Security model* spec melarang path home lengkap di `message` wire.
**Suggested fix**: Simpan `detail` untuk log (`tracing::warn!`), dan taruh hanya `op` dan root `cache` di message, seperti `InstallError::Io`.

### 🟡 Nama marker cadangan dibandingkan peka huruf besar kecil, `crates/core/src/runtime/archive/engine.rs:179`
**Problem**: `path.as_str() == MARKER_NAME` hanya menolak `.stackly-runtime.json` persis. Di APFS default yang tidak peka huruf, member `.Stackly-Runtime.json` di root lolos extract; `finish_tree` (`validate.rs:429`) lalu gagal `AlreadyExists` saat membuat marker dengan `O_EXCL`, dan `install.rs:1448` memetakannya ke error I/O platform umum, bukan `runtime.archive_rejected`/`reserved_name`. `scan` (`MemoryTree`) tidak menulis marker, jadi `runtime pin` menerima artefak itu.
**Why it matters**: Diagnosis salah, dan AC-7 meminta nama cadangan ditolak dengan alasan tetap. Butuh artefak yang di-pin katalog, jadi dampaknya rendah.
**Suggested fix**: Bandingkan dengan `eq_ignore_ascii_case(MARKER_NAME)` (atau lipat huruf seperti `MemoryTree`) dan tambah sampel nama varian huruf.

### 🟡 Tidak ada test level operasi untuk restart sesudah rename commit atau saat extract/validate, `crates/core/src/runtime/install.rs:2097`
**Problem**: `runtime_commit.rs` menguji `commit_tree` yang mengembalikan `AlreadyMoved`, dan `apps/daemon/tests/runtime/install.rs:612` menguji crash saat download. Belum diuji lewat engine: crash di antara rename dan batch akhir (AC-13), dan resume `validate_binary` yang membangun ulang `entries` dari `entries.json` (`read_entries`).
**Why it matters**: Ini jalur pemulihan yang dijanjikan AC-13 dan bagian *State transitions*, bertumpu pada cabang (`StepStart::AlreadyDone`, `read_entries`) yang tidak pernah dijalankan test.
**Suggested fix**: Tambah test core dengan fake yang menghentikan engine sesudah rename tahap commit (atau sesudah `finish_step(extract)`), restart, lalu assert satu baris runtime, tidak ada rename kedua, dan pohon tervalidasi.

### 🟡 Test native offline mencatat klaim yang tidak pernah di-assert, `apps/daemon/tests/native_runtime_install.rs:300`
**Problem**: Teks `expected` menyebut "within the retry time, and leave no runtime", tetapi test hanya mengukur dan mencetak `started.elapsed()`; tidak ada batas atas (AC-6, tidak menggantung) dan tidak ada cek `runtime.list` kosong. `quarantined` (baris 41) mengabaikan status exit `/usr/bin/xattr`, jadi `xattr` yang gagal terbaca sebagai "tidak ada quarantine".
**Why it matters**: Baris evidence ini bukti `native` pass untuk AC-046 dan AC-20; regresi yang menggantung beberapa menit atau meninggalkan baris runtime tetap lulus.
**Suggested fix**: Assert `elapsed` terhadap anggaran connect dan retry plus margin, assert `runtime.list` tanpa item, dan assert `output.status.success()` di `quarantined`.

### 🟡 `.part` yang sudah lengkap dibuang dan didownload ulang penuh, `crates/core/src/runtime/download.rs:324`
**Problem**: `resume` hanya `Some` bila `offset < source.size`. `.part` dengan panjang tersinkron sama dengan `size` punya `kept > 0` dan `resume == None`, sehingga baris 331-335 memotongnya ke 0.
**Why it matters**: Daemon mati setelah `checkpoint()` terakhir tetapi sebelum `finish_entry` me-rename: setelah restart seluruh artefak (ratusan MB untuk ClickHouse atau MySQL) diambil ulang. AC-3 meminta lanjut dari `.part`.
**Suggested fix**: Bila `kept == size`, lewati fetch dan langsung ke `finish_entry` dengan hash prefix; fungsi itu sudah membuang dan mengambil ulang bila mismatch.

### 🟡 Pengecekan ruang disk di tahap extract berjalan sebelum `stage/` lama dihapus, `crates/core/src/runtime/install.rs:1346`
**Problem**: `available_space` dibandingkan dengan `required_space(...)` (baris 1346-1356) sebelum `scratch/stage` sisa dihapus (baris 1357-1368). Tabel tahap spec berurutan "hapus stage/ di scratch kalau ada, cek ruang disk".
**Why it matters**: Daemon mati di tengah extract artefak besar, langkah diulang setelah restart: pohon setengah jadi masih memakan ruang saat dicek, jadi di volume hampir penuh install gagal `runtime.insufficient_space` padahal ulangannya muat.
**Suggested fix**: Hapus `stage/` lama dulu, baru cek ruang.

### 🟡 Cache hit mencatat `artifacts.source` NULL walau `<sha256>.json` ada, `crates/core/src/runtime/install.rs:1118`
**Problem**: `download_stage` membaca catatan sumber hanya untuk `DownloadOutcome::Downloaded`; untuk `Cached` ia menyimpan `source: None`, yang ditulis `final_batch` ke `artifacts.source`.
**Why it matters**: Install pertama mendownload dan menulis `<sha256>.json` lalu gagal di `validate_binary`; install kedua kena cache dan commit tanpa URL sumber. Spec (*Data model*) menyebut `source` diambil dari `<sha256>.json` dan NULL hanya bila catatan tidak ada.
**Suggested fix**: Panggil `cache_source(env, plan)` untuk kedua outcome.

### 🟡 Cek versi menjalankan `executables[0]`, bukan `runtime.executable` dari manifest, `crates/core/src/runtime/validate.rs:481`
**Problem**: Spec langkah 8 menyebut `program` adalah path kanonis `runtime.executable` dari manifest. Kode menjalankan `input.executables.first()`, dan `ValidateInput` tidak membawa executable manifest. Saat ini benar hanya karena setiap `versions.json` kebetulan mendaftar executable itu pertama.
**Why it matters**: Bila entry inventory mendaftar executable lain pertama (misal `bin/initdb`), cek menjalankan binary yang salah dengan args provider, dan hasilnya `version_mismatch` atau `binary_check_failed` yang membingungkan.
**Suggested fix**: Bawa `runtime.executable` manifest ke `ValidateInput`, atau buat `runtime check`/codegen menegakkan bahwa ia sama dengan `executables[0]` dan catat invarian itu di sini.

### 🟡 Mode akhir dan marker ditulis sebelum validasi, dan marker tanpa `validatedAt`, `crates/core/src/runtime/validate.rs:392`
**Problem**: Spec *Validasi binary* langkah 6 menerapkan mode akhir "sesudah penandatanganan", dan langkah 9 menulis marker terakhir dengan `{ schema, operationId, sha256, software, version, target, validatedAt }`. `finish_tree` berjalan tepat sesudah extract: mode sudah 0444/0555 sebelum `codesign --force`, dan marker tanpa `validatedAt`. Doc comment (baris 405-412) menjelaskan alasannya, tetapi spec tidak diperbarui.
**Why it matters**: Logika orphan dan resume di *Commit atomik* membaca marker ini; pembaca yang mengikuti spec mengharapkan bentuk lain, dan marker yang ada sebelum validasi berarti "staged", bukan "validated".
**Suggested fix**: Perbarui langkah 6 dan 9 spec 0018 lewat `/architect` sesuai urutan dan bentuk marker yang baru, atau tambah `validatedAt` dan pindahkan penulisan marker sesudah validasi.

### 🟡 Aturan bentuk selector ada di handler, bukan di `Validate`, dan error-nya menyebut method yang salah untuk `runtime.resolve`, `apps/daemon/src/ipc/runtime.rs:427`
**Problem**: Spec 0018 (*Tipe domain*) menyebut `InstallRuntimeParams::validate` menolak selector yang tidak cocok dengan empat bentuk (`stable`, `sha256:<64 hex kecil>`, `ExactVersion`, `RuntimeLine`). `selector_code` (`crates/protocol/src/runtime.rs:79`) hanya mengecek panjang dan charset, jadi `sha256:abc`, `:::`, atau `+` lolos. Handler menangkapnya kemudian lewat `RuntimeSelector::parse` dengan `.with_detail("method", "runtime.install")`, salah saat `select` berjalan untuk `runtime.resolve` (baris 64).
**Why it matters**: Kodenya sama, tetapi detail `method` salah untuk setiap `stackly runtime install --dry-run` dan resolve GUI dengan selector buruk; fixture kontrak `reject-runtime-install-selector.json` tidak mencakup aturan bentuk.
**Suggested fix**: Pindahkan cek bentuk ke `selector_code` (cek kecil di crate protocol, karena protocol tidak boleh bergantung ke domain), atau kirim nama method ke `select`. Tambah test protocol untuk `sha256:abc`.

### 🟡 `runtime.get` membaca semua runtime, instance, dan artefak untuk menjawab satu runtime, `apps/daemon/src/ipc/runtime.rs:218`
**Problem**: `get` memanggil `read_runtimes(&registry, false)`, yang memuat semua runtime dan instance dan memanggil `registry.artifact()` sekali per runtime managed (N+1, baris 279-285); `summary_of_runtime` dan `installed_runtime` sama. GUI memanggil `runtime.get` untuk setiap event `runtime.state_changed` dan setiap pilihan inspector.
**Why it matters**: Wajar untuk sedikit runtime, tetapi biaya tumbuh dengan ukuran registry di setiap event, dan registry punya satu thread penulis.
**Suggested fix**: Baca satu runtime, artefaknya, serta user dan dependent yang merujuknya; simpan pembacaan penuh hanya untuk `runtime.list`.

## Nits
- ⚪ `crates/protocol/src/runtime.rs:571`, `InstallOutcome` enum tertutup tanpa `Unrecognized`; spec tidak menyebutnya terbuka, tetapi outcome ketiga nanti akan gagal di-decode klien Swift lama. Jadikan terbuka sekarang selagi murah.
- ⚪ `apps/daemon/src/ipc/runtime.rs:75`, `request_fingerprint` dihitung dua kali (baris 75 dan 134) dari params yang di-clone; hitung sekali.
- ⚪ `apps/cli/src/commands/runtime/install.rs:82`, `--dry-run` diam diam menerima `--wait`, `--timeout`, dan `--idempotency-key`; tambah `conflicts_with_all`.
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Runtimes/RuntimeStore.swift:287`, `recordStarted` memicu `reloadInstalling()`, `get` menunggunya lagi (baris 375), dan `follow` memuat ulang ketiga kalinya: tiga bacaan halaman pertama per klik.
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Runtimes/RuntimesPage.swift:297`, `copyAction` selalu memakai alasan "This runtime has no digest" walau helper-nya generik; kirim alasan sebagai parameter.
- ⚪ `crates/core/src/provider/versions.rs:1016` (konstanta di baris 805), `CODES_MAX` dipakai untuk jumlah code per list dan panjang code; pisahkan jadi dua konstanta dengan doc masing masing.
- ⚪ `crates/core/src/runtime/closure.rs:177`, set dependensi memakai nama file install name (`libz.1.dylib`), spec langkah 4 menyebut nama file "setelah symlink diselesaikan"; kode, test, dan data konsisten, jadi samakan kata spec.
- ⚪ `crates/core/src/runtime/mod.rs:22`, `samples` dan `fuzzing` adalah `#[doc(hidden)] pub mod` yang ikut terkompilasi ke library produksi; pertimbangkan gate di balik feature `test-support`/`fixture`.
- ⚪ `crates/core/src/runtime/validate.rs:433`, `if let Some(mode) = FileMode::new(0o444)` diam diam melewati mode marker bila konstanta tidak valid; pakai `expect` dengan invarian seperti `engine.rs:406`.
- ⚪ `crates/core/src/runtime/macho.rs:365`, doc menyebut "nibbles" padahal field 16/8/8 bit; `minos` 0 menggagalkan seluruh install sebagai `binary_check_failed`, pertimbangkan sebagai tidak diketahui.
- ⚪ `crates/core/src/runtime/archive/zip_archive.rs:103`, selisih hitungan EOCD dengan crate selalu dilaporkan `duplicate_entry`, juga saat crate memilih EOCD atau hitungan zip64 lain; `corrupt` lebih jujur setelah nama ganda disingkirkan.
- ⚪ `crates/core/src/runtime/archive/engine.rs:84`, `archiveRoot` menerima beberapa komponen (`a/b`), tetapi member `a/` lalu `unexpected_top_level`; tolak root multi komponen di `InvalidSpec` atau terima leluhurnya.
- ⚪ `crates/core/src/runtime/archive/tar_gz.rs:178`, PAX `path` kedua diam diam menimpa yang pertama, sementara GNU long name kedua `corrupt` (baris 92); samakan.
- ⚪ `crates/core/src/runtime/install.rs:1891`, di `staged`, setelah sender drop, `Event::Progress(None)` kembali ke `select!` dengan `recv()` siap lagi: busy spin singkat; matikan cabang itu setelah `None`.
- ⚪ `crates/core/src/runtime/install.rs:1914`, pembatalan dideteksi dengan mencocokkan string `InstallError::Internal("stopped")`; pakai varian `InstallError::Stopped`.
- ⚪ `crates/core/src/runtime/download.rs:490`, sidecar `.part.json` selalu menulis `updatedAt: ""`; spec menyebutnya timestamp, isi dari `Clock`.
- ⚪ `crates/core/src/runtime/install.rs:1275`, komentar menyebut progress "after each file", padahal `ProgressFile::write` melapor di setiap tulisan.
- ⚪ `crates/core/src/runtime/install.rs:1423`, ENOSPC saat extract melaporkan `available: 0`, dan `platform_error`/`From<ValidateError>` melaporkan `needed: 0, available: 0`, yang tampak seperti pengukuran nyata di details.
- ⚪ `crates/platform-macos/src/fetch.rs:77`, `User-Agent` memakai `CARGO_PKG_VERSION` `stackly-platform-macos`, bukan versi app yang spec sebut (`Stackly/<versi>`).

## Strengths
- Extractor tidak pernah menulis lewat link: symlink dicatat, dicek leksikal terhadap tabel entry final (hanya folder di jalurnya, rantai ≤ 8, deteksi siklus), dan dibuat terakhir; `AlreadyExists` di writer disk menutup tabrakan huruf dan normalisasi Unicode APFS. Batas ukuran dicek sebelum bekerja: total byte sebelum `create_file`, jumlah entry EOCD sebelum crate mem-parse central directory, header PAX/GNU ≤ 64 KiB, inflate dibatasi ukuran yang dideklarasikan dengan CRC di EOF.
- Parser Mach-O hanya membaca header dan ≤ 4 MiB load command, membatasi fat slice 30, mencocokkan header slice dengan entry fat-nya, dan memisahkan kegagalan I/O dari input rusak. Setiap `RejectReason` punya sampel, dan sebuah test menegakkannya.
- Badan tahap install adalah `fn(&Env, &Plan, …)` biasa yang dipakai bersama oleh work engine dan `fixture::install_cached_artifact`, jadi install test menjalankan jalur produksi. `commit_tree` memakai marker berkunci operation ID sehingga restart sesudah rename idempoten, dan tidak ada yang dihapus di `runtimes/`.
- Fetcher mengikuti redirect sendiri dengan cek ulang `https` di tiap hop, mengirim ulang `Range`/`If-Range`, mencocokkan `Content-Range` dan `Content-Length` persis, dan tidak mengompilasi gzip. Adapter codesign patuh K9: program tetap, env kosong, kill grup setsid lewat `process/kill.rs`, dan `// SAFETY:` di setiap `unsafe`.
- `runtime.install` replay sebelum membaca katalog atau registry dan membangun ulang `resolved` dari input journal; dua install artefak yang sama antre di `LockKey::Artifact` dengan precheck `alreadyInstalled`. `versions.rs` melaporkan record kompatibilitas yang tidak cocok di lokasinya, bukan diam diam membuang artefak.

## Test coverage
Kuat: download (`crates/core/tests/runtime_download.rs`: resume, truncation sync, 200 untuk Range, validator berubah, 416, mismatch, klasifikasi retry, ruang, stop), commit (`runtime_commit.rs`), resolve (`runtime_resolve.rs`), alur ujung ke ujung termasuk cancel, crash saat download, dan line katalog yang pindah (`runtime_install.rs`, daemon `tests/runtime/{install,read,resolve,support}.rs`), validasi protocol, test CLI install, field fase 02 `versions.json`, dan test Swift `RuntimeStoreTests`, `RuntimeGetCellTests`, `RuntimeCopyTests`, `RuntimesPageTests`.

Celah: executable yang `@rpath`-nya hanya terselesaikan lewat rpath executable lain; nama cadangan varian huruf; kepala file Mach-O yang pendek atau gagal dibaca; mismatch pada percobaan download ketiga; `.part` lengkap saat restart; `source` pada cache hit; restart level operasi sesudah rename commit dan di extract/validate; `ERANGE` quarantine; `--output id` pada install gagal atau timeout; pilihan record inspector saat record penentu bukan yang pertama; `runtime.resolve` dengan selector `sha256:` rusak; retry `--idempotency-key` setelah katalog pindah; batas waktu dan klaim tanpa runtime di test native offline.

## Konfirmasi fix 2026-10-31

**Diperiksa oleh**: Opus 5.5 · xhigh, inline (penulis fix Sonnet 5.5 menurut trailer kedua commit, family berbeda)
**Range**: `git diff 16642a50^1 16642a50`, merge `16642a50` dari `dev-f18-fix` dengan dua commit: `854f4884 fix(runtime): send the deciding compatibility record first in runtime.get` dan `742d510a test(runtime): order of runtime compatibility records, NATS fixtures at supported`. Ada 36 file, 18 di antaranya PNG snapshot. Nomor baris mengacu ke `8f74c974` (`main`). Minor dan Nit review asli tidak diulang.
**Status Major**: terbuka sebagian. Kasus yang dilaporkan sudah tertutup. Major masih terbuka saat record catalog untuk sha256 itu berbeda dari salinan registry, yang terjadi pada update catalog biasa.

### Status Major

**Yang tertutup.** `runtime.get` kini mengambil record penentu dari verdict yang sama dengan badge: `catalog_verdict` (`apps/daemon/src/ipc/runtime.rs:256`), dengan `registry_deciding_record` sebagai cadangan (baris 258). `runtime_detail` (`apps/daemon/src/map/runtime.rs:209`) menaruh record itu di depan, lalu sisanya menurut `testedAt` menurun dengan sort stabil. Doc DTO (`crates/protocol/src/runtime.rs:521`), schema, dan `StacklyProtocol.swift` menyebut urutan yang sama.

Test `an_install_copies_the_catalog_compatibility_records_and_support_follows_them` (`apps/daemon/tests/runtime/install.rs:222`) membedakan urutan dengan benar. Ia mendaftar record runtime untuk mayor host, lalu record runtime terbaru untuk mayor sebelumnya, lalu record provider penentu yang paling lama; kalau urutannya hanya "newest first", test gagal. Untuk install baru, saat salinan registry sama dengan catalog (termasuk contoh NATS 2.15.1 di review asli), record provider kini muncul pertama di bawah badge Supported. Filter `registry_deciding_record` sama dengan `registry_support`, jadi jalur registry juga cocok dengan badge.

#### 🟠 Sisa Major: record penentu dari catalog, daftar dari registry, `apps/daemon/src/map/runtime.rs:209`
**Problem**: `deciding` (`apps/daemon/src/ipc/runtime.rs:256`) menunjuk ke record di `VersionArtifact::compatibility()` dari catalog yang sedang dimuat. Daftar yang diurutkan berasal dari `ArtifactRecord.compatibility`, salinan registry yang ditulis sekali saat install dan tidak berubah sesudahnya (spec 0018 *Data model*). `Some(*row) != deciding` membandingkan nilai. Bila sesudah install catalog membawa record penentu yang tidak ada di salinan registry, tidak ada baris yang cocok. Semua baris lalu diurutkan "newest first", dan baris pertama bukan penentu. Spec 0018 *Value sourcing* (`docs/specs/0018-install-runtime-managed/index.md:587`) meminta `get.compatibility` diambil dari catalog selama sha256 masih ada di sana, dan dari registry hanya bila tidak. Kode mengirim salinan registry di kedua kasus.
**Skenario dengan data repo**: user memasang NATS 2.15.1 saat catalog masih di revision 1.0.0, dengan record runtime 2026-10-18 dan record provider 1.0.0 2026-10-22T02:48. Update app membawa revision 1.0.1 beserta record provider 1.0.1 (`providers/nats/versions.json`, commit `7664295c`). Badge menjadi Supported (provider_tested) karena record 1.0.1 di catalog. Salinan registry tidak memuat record itu, jadi inspector menampilkan record provider revision 1.0.0. Menurut BR-025, record itu tidak membenarkan Supported untuk revision 1.0.1. Kontrak baru ("the record that decided the support first") jadi dilanggar pada jalur update biasa, bukan kasus pinggiran.
**Suggested fix**: Ikuti baris *Value sourcing*. Selama sha256 masih ada di catalog, kirim record catalog, dengan `sourceUrl` dan `sha256` dari artefak catalog, sehingga `deciding` menunjuk ke slice yang sama dan bisa dicocokkan dengan `std::ptr::eq`. Bila tidak, kirim salinan registry dengan `registry_deciding_record`. Kalau salinan registry harus tetap menjadi sumber, kirim record penentu secara eksplisit lewat field opsional `decidingRecord`, dengan kenaikan minor. Tambah test daemon: install dengan catalog A, restart dengan catalog B yang membawa record provider untuk revision baru, lalu assert `compatibility[0]` adalah record B.

#### 🟡 Sisa Minor: inspector menampilkan record walau tidak ada record penentu, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Runtimes/RuntimeInspector.swift:114`
**Problem**: Baris Support selalu menampilkan `detail?.compatibility?.first`. Untuk verdict tanpa record (`untested`, `fixture_only`, `requires_newer_os`, `no_build_*`, `artifact_blocked`), `deciding` kosong, jadi baris pertama hanya record terbaru. Kontrak baru tidak memberi klien cara membedakan keduanya selain `supportReason`. Doc `RuntimeCopy.recordText` (`RuntimeCopy.swift:90`) juga masih menyebut parameternya "The newest compatibility record".
**Contoh**: setelah Mac naik ke macOS 28 sementara record hanya ada untuk 27, inspector menulis "Unverified: Not tested on this Mac yet." lalu tepat di bawahnya "Tested on macOS 27.0 …". Spec 0018 *Halaman Runtimes* (index.md:535) meminta "record penentunya", bukan record terbaru.
**Suggested fix**: Tampilkan record hanya bila `summary.supportReason` adalah `.providerTested` atau `.runtimeTested`, lalu perbarui doc `recordText`. Pilihan lain: nyatakan di doc DTO bahwa elemen pertama hanya menjadi penentu untuk dua reason itu.

### Temuan baru di file yang disentuh fix

Fix ini tidak memunculkan Blocker atau Major baru.

- 🟡 `apps/daemon/src/ipc/runtime.rs:258`: tidak ada test yang menjalankan cabang cadangan registry (`registry_deciding_record`, `apps/daemon/src/map/runtime.rs:375`). Test daemon hanya memakai jalur catalog, dan 71 test `map::` tidak ada yang menguji `runtime_detail` atau `registry_deciding_record`. Tambah test daemon dengan catalog yang tidak lagi memuat sha256 itu, atau unit test `runtime_detail` dengan record acak dan `deciding` dari registry.
- ⚪ `apps/daemon/src/map/runtime.rs:387`: ada dua definisi "record penentu". `support_status` mengambil record pertama yang cocok menurut urutan catalog (`find` di `crates/core/src/runtime/compatibility.rs:83` dan `:97`), sedangkan `registry_deciding_record` mengambil yang terbaru (`max_by_key`). Hasilnya kini sama karena generator menyimpan satu record per kunci, tetapi samakan aturannya atau tulis perbedaannya di doc.
- ⚪ `apps/daemon/src/ipc/runtime.rs:256`: `get` menghitung `catalog_verdict` dua kali, sekali lewat `summary_of` → `catalog_support` (baris 237 dan 345) dan sekali di sini, jadi entri catalog dipindai dua kali. Hitung sekali lalu pakai untuk summary dan record penentu; itu juga menjamin keduanya berasal dari verdict yang sama.
- ⚪ `clients/swift/Tests/StacklyClientTests/Contract/ContractTests.swift:137`: test bernama `protocolIsOnePointEleven` meng-assert 1.15. Fix menyentuh baris assert di bawahnya, tetapi namanya tidak diganti.

### Kontrak, fixture, dan snapshot

- Kenaikan minor sudah benar. Versi 1.14 sudah di `main` sebelum fix, jadi `protocols/contract-versions.json` menambah entri 1.15, bukan `--amend`. Digest 1.15 sama dengan 1.14 (`f8c175ce…`) karena digest membuang `description` (`DOC_KEYWORDS`, `xtask/src/codegen/contract.rs:27`): yang berubah hanya jaminan urutan, bentuk wire tetap. Minor cocok di sini, karena daemon tidak pernah memenuhi jaminan lama "newest first", dan satu satunya klien yang bergantung pada urutan adalah inspector. `version.rs`, `protocol-version.json`, `ContractInfo.swift`, dan schema sudah 1.15, dan tidak ada sisa 1.14 di fixture atau test.
- Fixture NATS (`protocols/fixtures/catalog/provider-nats.json:71`, `protocols/fixtures/ipc/frame-response-catalog-get-provider.json:77`, dan seed fuzz) kini `supported`/`provider_tested`. Nilai itu sesuai output daemon (`map::catalog::tests::every_detail_reply_of_the_fixture_catalog_is_its_shared_fixture` lulus) dan BR-025. Penyebabnya bukan fix ini: `providers/` tidak berubah di merge ini, dan record provider 1.0.1 masuk lewat `7664295c`, yang sudah di `main` sebelumnya. Pesan commit `742d510a` keliru menyebut penyebabnya. `crates/protocol/tests/catalog_fixtures.rs:195` melonggarkan assert menjadi "supported hanya dengan provider_tested", dan itu cocok dengan BR-025.
- Ke-18 snapshot catalog dan `runtime-get-*` hanya berubah di sel macOS NATS: badge "Unverified" menjadi "Tested", dengan catatan "macOS: Tested with this provider on this Mac." Ini dicek dengan membandingkan `runtime-get-installed.light.png` lama dan baru. Tidak ada snapshot inspector yang berubah, dan tidak ada test Swift yang menguji record mana yang dipilih inspector.

### Perintah dijalankan

| Perintah | Hasil |
| --- | --- |
| `git merge main`, `git diff --stat main...HEAD` | fast forward ke `8f74c974`, diff kosong |
| `git log --oneline 16642a50^1..16642a50^2`, `git diff 16642a50^1 16642a50` | 2 commit, 36 file (18 PNG) |
| `touch xtask/src/main.rs crates/protocol/src/version.rs` | dilakukan sebelum build |
| `cargo test -p stacklyd --lib map::` | lulus, 71 test |
| `cargo test -p stacklyd --test runtime` | lulus, 15 test |
| `cargo xtask codegen --check` | lulus, "20 generated files are up to date" |
| `git show 16642a50^1:<png>` dan `git show 16642a50:<png>` untuk `runtime-get-installed.light.png` | hanya sel macOS NATS yang berubah |

Test Swift (`swift test` di `clients/swift` dan `apps/macos/Packages/StacklyUI`) tidak dijalankan karena di luar perintah yang brief izinkan (NOT_RUN).

## Konfirmasi fix ronde 2 2026-10-31

**Diperiksa oleh**: Opus 5.5 · xhigh, inline (penulis fix Sonnet 5.5 menurut trailer kedua commit, family berbeda)
**Range**: `git diff 5d9d7fc4^1 5d9d7fc4`, merge `5d9d7fc4` dengan dua commit: `7223f7b6 fix(runtime): send the catalog's compatibility records in runtime.get while it lists the artifact` dan `669b5161 fix(runtimes): show a compatibility record under the badge only for a tested support`. Ada 7 file, tanpa perubahan kontrak. Nomor baris mengacu ke `6cf6efba` (`main`), dan ketujuh file tidak berubah sejak `5d9d7fc4`. Minor dan Nit review asli tidak diulang.
**Status Major**: tertutup.

### Status Major

**Sisa Major (record penentu dari catalog, daftar dari registry): tertutup.** `get` (`apps/daemon/src/ipc/runtime.rs:237`) memanggil `catalog_match` sekali. Fungsi itu (baris 389) kini mengembalikan artefak catalog beserta verdict-nya. Selama catalog memuat sha256 itu, daftar yang dikirim adalah `catalog_artifact.compatibility()`, dan `deciding` adalah `verdict.record` dari `support_status` atas slice yang sama (baris 265). Bila tidak, daftar adalah salinan registry, dan `deciding` datang dari `registry_deciding_record` atas slice itu (baris 266 sampai 271). Ini persis baris *Value sourcing* `get` / `compatibility` (spec 0018, `index.md:587`). `runtime_detail` (`apps/daemon/src/map/runtime.rs:190`) menerima slice itu sebagai `records` dan mencocokkan penentu dengan `std::ptr::eq` (baris 214). Jadi record penentu selalu ada di daftar yang dikirim, dan dua record bernilai sama tidak bisa tertukar. Badge (`summary_with`, `ipc/runtime.rs:348`) dan record penentu kini berasal dari satu verdict, sehingga keduanya tidak bisa menyimpang.

Skenario ronde 1 kini diuji: install dengan catalog A, lalu update app ke catalog B yang membawa record provider untuk revision itu. Test `a_newer_catalog_record_that_decides_the_support_comes_first_in_runtime_get` (`apps/daemon/tests/runtime/install.rs:357`) memasang dengan satu record runtime. Lalu daemon di-restart di registry yang sama dengan catalog B, yang berisi record runtime lama, record runtime lebih baru untuk mayor sebelumnya, dan record provider yang lebih lama dari keduanya. Test meng-assert `supported`/`provider_tested`, urutan scope [provider, runtime, runtime], dan `testedAt` menurun di sisanya. Test ini gagal di kode ronde 1, karena salinan registry hanya punya 1 record. Ia juga gagal bila urutannya hanya "newest first", karena record provider lalu jatuh ke posisi kedua.

`sourceUrl` dan `sha256` tiap view tetap diambil dari baris registry `artifacts` (`compatibility_view`, `map/runtime.rs:331`). `sha256` sama karena pencocokan catalog memakai sha256 itu. `sourceUrl` sesuai doc DTO "Where the artifact was downloaded from" (`crates/protocol/src/runtime.rs:498`), jadi tidak ada penyimpangan baru. Nilai NULL pada cache hit adalah Minor asli (`install.rs:1118`) dan tidak diulang.

**Sisa Minor inspector: tertutup.** `RuntimeCopy.decidingRecord(of:in:)` (`RuntimeCopy.swift:94`) mengembalikan elemen pertama hanya untuk `.providerTested` dan `.runtimeTested`. `support_status` (`crates/core/src/runtime/compatibility.rs:83` sampai 105) memang hanya mengisi `record` untuk dua reason itu. Reason lain, termasuk `.unrecognized`, tidak menampilkan record. `RuntimeInspector.swift:114` memakai fungsi itu, dan doc `recordText` kini berbunyi "The record that decided the support". `summary` di inspector berasal dari baris list, sedangkan `detail` dari `runtime.get`. Cache detail dikunci per server instance dan revision (`RuntimeDetailModel.swift`, `key`), jadi detail dari daemon sebelum restart tidak dipasangkan dengan badge dari catalog baru. Test `onlyATestedSupportShowsTheRecordThatDecidedIt` (`RuntimeCopyTests.swift:89`) menutup tabel reason itu.

**Minor cabang cadangan registry belum diuji: tertutup.** Test `an_artifact_that_left_the_catalog_keeps_the_registry_copy_with_its_deciding_record_first` (`install.rs:435`) memasang dengan tiga record runtime. Penentunya adalah satu satunya record untuk mayor host, sekaligus yang paling lama. Lalu daemon di-restart dengan catalog kosong. Test meng-assert `experimental`/`runtime_tested` dan urutan [mayor host, mayor - 2 (10-27), mayor - 1 (10-25)]. Dengan begitu `registry_support` dan `registry_deciding_record` dijalankan lewat `get`.

**Tiga Nit ronde 1: tertutup.**

- Dua definisi record penentu: `registry_deciding_record` (`map/runtime.rs:383`) kini memakai `find`, record pertama yang lolos, seperti `support_status`. Baris registry dibaca `ORDER BY position`, yaitu urutan catalog saat install. `versions.rs` menegakkan satu record per kunci (doc baris 810), jadi untuk data yang sah kedua aturan memang tidak bisa berbeda.
- `catalog_verdict` dihitung dua kali: `get` kini memanggil `catalog_match` sekali dan meneruskan verdict-nya ke `summary_with`. `catalog_support` dan `catalog_verdict` dihapus, tanpa pemanggil lain.
- Nama `protocolIsOnePointEleven`: kini `protocolIsOnePointFifteen` (`ContractTests.swift:137`), sesuai assert 1.15.

### Temuan baru di file yang disentuh fix

Fix ini tidak memunculkan Blocker, Major, atau Minor baru.

- ⚪ `apps/daemon/src/map/runtime.rs:187`: doc `runtime_detail` ("so a client that shows the first one shows the record behind the badge") dan doc DTO `RuntimeDetail.compatibility` (`crates/protocol/src/runtime.rs:521`, tidak disentuh fix) tidak menyebut bahwa ada record penentu hanya untuk `provider_tested` dan `runtime_tested`. Untuk reason lain, elemen pertama hanyalah record terbaru. Syarat itu kini hanya tertulis di doc `RuntimeCopy.decidingRecord`. Tambahkan ke kedua doc Rust supaya klien lain tidak mengulang Minor inspector. Digest kontrak membuang `description`, jadi tidak perlu kenaikan versi.

### Perintah dijalankan

| Perintah | Hasil |
| --- | --- |
| `git merge main`, `git diff --stat main...HEAD` | fast forward ke `6cf6efba`, diff kosong |
| `git log 5d9d7fc4^1..5d9d7fc4^2`, `git diff 5d9d7fc4^1 5d9d7fc4` | 2 commit, 7 file |
| `git diff --stat 5d9d7fc4 HEAD -- <7 file>` | kosong |
| `touch xtask/src/main.rs crates/protocol/src/version.rs apps/daemon/src/main.rs` | dilakukan sebelum build |
| `cargo test -p stacklyd --test runtime` | lulus, 17 test, termasuk dua test baru |
| `cargo test -p stacklyd --lib map::` | lulus, 71 test |
| `cargo xtask codegen --check` | lulus, "20 generated files are up to date" |

Test Swift (`swift test` di `clients/swift` dan `apps/macos/Packages/StacklyUI`, termasuk `onlyATestedSupportShowsTheRecordThatDecidedIt`) tidak dijalankan, karena di luar perintah yang brief izinkan (NOT_RUN).
