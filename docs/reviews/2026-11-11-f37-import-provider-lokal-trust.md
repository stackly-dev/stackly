# Review, F37 import provider lokal dan trust (spec 0037), 2026-11-11

**Reviewed by**: Fable 5.2 (effort high), sesi baru yang belum pernah melihat implementasi. Kode ditulis Opus 5.5 (developer dan tester), jadi reviewer berbeda family dari penulis. Sesi utama membaca sendiri inti trust (core `catalog/import`, `store`, `trust`, `local/*`, handler `provider.*`, gate di install dan create, refresh snapshot). Tiga subagent Fable 5.2 membaca irisan lain secara read only: A (storage, domain, protocol, testkit), B (CLI), C (StacklyUI dan Swift client). Setiap temuan Major, dan setiap temuan irisan yang menyentuh batas trust, diperiksa ulang sesi utama langsung di kode sebelum ditulis di sini.
**Scope**: `MODE=branch`, `BASE=main`, `MERGE_BASE=e5506e2c`, range `e5506e2c..349dc048`, dibatasi ke 46 commit ber subject "spec 0037" (`git log --oneline --no-merges e5506e2c..HEAD | grep -i 'spec 0037'`), 346 file. Range itu juga memuat spec 0036 dan 0038; hunk fitur lain diabaikan. Nomor baris mengacu ke `349dc048`. Tidak ada build atau test yang dijalankan; semua temuan berasal dari membaca kode, test, dan spec.
**Verdict**: Changes requested

## Summary

Fitur ini menambah import folder paket provider ke store beralamat isi, approval yang mengikat `packageSha256`, revoke, remove, revocation dari catalog terpasang, gate trust di install dan create, snapshot catalog yang bisa diganti dengan event `catalog.changed`, empat subcommand CLI, dan sheet import serta approval di app.

Inti batas trust nya kokoh. Byte yang di hash adalah byte yang disalin (pekerjaan import membaca folder sekali ke memori, lalu menulis dan membaca ulang salinan itu). Approval menghitung izin dari salinan store, bukan dari client, dan guard `commit_guarded` mencocokkan digest baris. Setiap gate membaca ulang registry dan gagal baca berarti menolak. Baris pin ditulis di commit yang sama dengan guard trust create. Mengganti import tidak menghapus keputusan lama. Pesan error dan log store tidak membawa path.

Tiga Major. Pertama, `doctor.run` mengambil teks remedy (judul, langkah, dan perintah untuk disalin) dari entri snapshot catalog ber ID sama dengan provider instance, tanpa cek trust dan tanpa mencocokkan revision. Sejak fitur ini snapshot memuat entri lokal, jadi paket yang `pending` atau `revoked` bisa berbicara lewat doctor untuk instance yang dibuat dari byte lain yang pernah disetujui. Kedua, tombol "New Instance…" di panel provider lokal tidak pernah mengirim ID provider itu, jadi saat ada entri `official` ber `software` sama, form yang terbuka milik provider resmi. Ketiga, panel detail bisa tertahan di spinner sesudah Approve atau Revoke, karena dua snapshot beruntun membuang jawaban detail yang sedang berjalan dan pemuatan ulangnya dilewati.

Sisanya Minor: baris audit operasi yang gagal sebelum langkah `commit` tanpa identitas provider, install yang tidak memeriksa trust sebelum unduhan dimulai, `import --approve` di CLI yang menyetujui digest dari balasan import (bukan dari preview), batas nilai baris import yang hanya dijaga CHECK SQL, hasil basi yang masih diterapkan sheet sesudah Cancel, dan beberapa celah tampilan serta test.

Jumlah: Blocker 0, Major 3, Minor 24, Nit 22.

## Major

### 🟠 `doctor.run` menampilkan remedy dari paket lokal yang belum disetujui atau sudah dicabut, `crates/core/src/doctor/checks/remedy.rs:55`
**Irisan**: sesi utama. Diverifikasi di kode.
**Problem**: `find` (baris 37 sampai 52) lebih dulu memanggil `catalog_package`, yang mengambil `catalog.entry(instance.provider_revision().provider())` dan memakai paketnya apa adanya (baris 55 sampai 61). Tidak ada cek `EntryOrigin`, tidak ada `entry_trust`, dan revision serta digest entri tidak dicocokkan dengan `provider_revision` instance. Catalog itu datang dari `core.catalog()` (`apps/daemon/src/ipc/doctor.rs:123`), yang sejak fitur ini adalah snapshot gabungan dengan entri lokal (`apps/daemon/src/state.rs`, `replace_catalog`). Paket pinned instance baru dipakai kalau entri snapshot tidak punya remedy yang cocok atau tidak bisa dirender.
**Why it matters**: Urutan nyata: kamu menyetujui `acme/demo` revision 1 dan membuat instance. Lalu sebuah paket lain ber ID `acme/demo` diimpor (mengganti entri, state `pending`), dan kamu sengaja tidak menyetujuinya, atau kamu mencabutnya. Saat instance itu gagal start, `stackly doctor` dan halaman Diagnostics menampilkan `title`, `steps[].text`, dan `steps[].command` ("perintah untuk disalin", spec 0034 AC-10) dari byte yang tidak pernah disetujui, dengan suara Stackly, tanpa label "not approved". Paket itu juga menentukan `evidence.pidFile` yang dibuka doctor dan daftar executable yang dianggap "proses runtime". Tidak ada yang dijalankan, tetapi ini persis saluran yang ditutup spec ("Paket lokal masuk catalog sebagai data yang tidak dipercaya"): import berjanji tidak berpengaruh sampai disetujui, padahal isinya sudah menjadi saran perbaikan untuk instance yang sedang rusak. Sebelum fitur ini jalur itu hanya pernah melihat paket `official` yang disegel bundle, jadi spec 0034 tidak perlu aturan ini; spec 0037 tidak memasukkan doctor ke tabel *Gate trust*.
**Suggested fix**: Di `catalog_package`, pakai entri snapshot hanya kalau ia entri catalog (`entry.local().is_none()`), atau kalau ia entri lokal yang kuncinya sama persis dengan `instance.provider_revision()` dan `entry_trust` nya `Trusted`. Selain itu langsung jatuh ke paket pinned instance. Tambah test core: instance dari revision yang disetujui, entri lokal pengganti yang `pending` dan yang `revoked` dengan blok `remedies` berbeda, lalu pastikan remedy yang keluar berasal dari paket pinned. Catat aturannya di spec 0034 AC-10 dan di tabel *Gate trust* spec 0037 (lewat architect).

### 🟠 "New Instance…" di panel provider lokal tidak pernah meminta provider itu, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/ProviderDetailPanel.swift:114`
**Irisan**: C. Diverifikasi sesi utama di kode.
**Problem**: Tombol itu memanggil `newInstance.present(software: software)` (baris 114 sampai 122), dan model selalu mengirim `getCreateForm(runtimeId:, provider: nil)` (`apps/macos/Packages/StacklyUI/Sources/StacklyUI/CreateInstance/CreateInstanceModel.swift:369`). Menurut *Software yang sama*, tanpa `provider` daemon memilih entri catalog ber `software` itu lebih dulu. Tidak ada kode di `CreateInstance/` atau `Catalog/` yang menangani `instance.provider_ambiguous` dengan `candidates`.
**Why it matters**: Cerita utama fitur ini adalah mencoba provider sendiri. Dengan `official/nats` dan `acme/nats` yang sudah disetujui, tombol di panel `acme/nats` membuka form `official/nats`, dan instance yang dibuat bukan dari provider yang sedang dilihat. Dengan dua entri lokal tanpa entri resmi, daemon menjawab `instance.provider_ambiguous` dan GUI hanya menampilkan error, padahal spec berkata client meminta pengguna memilih. CLI tidak terkena karena menerima ID lengkap. *Layar* dan Build plan tidak menyebut perubahan New Instance, jadi sebagian dari ini celah spec; perilakunya tetap salah untuk pengguna.
**Suggested fix**: Teruskan `summary.provider` dari panel lewat `present` sampai `getCreateForm(runtimeId:, provider:)`, supaya form selalu milik entri yang sedang ditampilkan. Tangani `candidates` dengan pilihan provider di sheet, atau paling sedikit dengan pesan yang menyebut kandidatnya. Tambah test model: panel entri lokal mengirim ID nya, dan balasan ambiguous menampilkan kandidat. Catat di spec 0037 *Layar* lewat architect.

### 🟠 Panel detail bisa tertahan di spinner sesudah Approve atau Revoke, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/ProviderDetailModel.swift:62`
**Irisan**: C. Diverifikasi sesi utama lewat pembacaan kode; urutan waktunya belum dijalankan.
**Problem**: `load` melewati kunci yang masih ada di `inFlight` (baris 64), dan `invalidate()` menaikkan `epoch` tanpa mengosongkan `inFlight` (baris 88 sampai 92). Setiap operasi GUI memicu dua snapshot: event `catalog.changed` menjalankan `Task { resnapshot() }` (`apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/CatalogStore.swift:77`), lalu balasan operasi memanggil `reload()` (baris 97 sampai 99). Kedua `fetchPage` memanggil `detail.invalidate()` (baris 63). Urutannya: snapshot pertama selesai, epoch naik, `.task(id:)` halaman (`CatalogPage.swift:144`) memulai pemuatan detail L1. Snapshot kedua selesai, epoch naik lagi, task dijalankan ulang dan memanggil `load`, yang langsung kembali karena kunci masih di `inFlight`. L1 lalu selesai dan hasilnya dibuang oleh `guard started == epoch`. Tidak ada lagi yang memicu pemuatan, jadi `phase` tetap `.loading`.
**Why it matters**: Spinner tanpa akhir di inspector tepat sesudah aksi inti fitur ini, sampai pengguna memilih ulang entri. Test end to end sudah menabrak hal ini: commit `00fa4032` mengganti satu `load` dengan loop `retry` 20 detik (`apps/macos/Packages/StacklyUI/Tests/StacklyUITests/LocalProviderEndToEndTests.swift:128` sampai 147), sedangkan halaman sungguhan memanggil `load` sekali per epoch. Jadi test lulus dengan cara yang tidak dimiliki halaman.
**Suggested fix**: Kosongkan `inFlight` di `invalidate()`, atau jadikan epoch bagian dari kuncinya. Pakai `store.refresh()` yang menggabungkan permintaan untuk event dan untuk reload eksplisit (spec *Layar* menulis `refresh()`), supaya N event tidak berarti N kali `catalog.list_providers`. Pindahkan penulisan `loaded.*` dan `detail.invalidate()` keluar dari `fetchPage`, supaya generation yang digantikan tidak punya efek samping. Tambah test unit: mulai `load`, panggil `invalidate()`, panggil `load` lagi, selesaikan panggilan pertama, harapkan `.loaded`; lalu kembalikan test end to end ke satu `load`.

## Minor

### 🟡 Baris audit operasi provider yang gagal sebelum langkah `commit` tidak menyebut provider atau digest, `crates/core/src/catalog/import.rs:613`
**Irisan**: sesi utama (temuan yang sudah diketahui verify; tingkat dinilai ulang di sini).
**Problem**: `run_import` baru memanggil `set_audit_note` di langkah `commit` (baris 613). Gagal di `read` (`provider.import_changed`, baris 493 sampai 498) atau di `store` (`provider.store_failed`, baris 545 sampai 563) menghasilkan baris audit tanpa catatan. `run_approve` punya pola yang sama: catatan dipasang di baris 773, sesudah langkah `read`, jadi approve yang gagal karena salinan store tidak valid tidak membawa digest. Mesin audit hanya menyalin `errorCode`, `message`, dan `stage` (`crates/core/src/operation/audit.rs:216` sampai 242), bukan `details` error, dan resource operasi ini `Manager`, jadi baris itu tidak punya penunjuk ke provider sama sekali. Dua pesan yang paling mungkin muncul ("the provider package could not be stored", "the provider package changed since it was previewed") tidak menyebut ID.
**Why it matters**: AC-17 meminta setiap operasi membawa `provider`, `providerRevision`, `packageSha256`, dan `manifestSha256`. Untuk operasi yang gagal, jejaknya hanya berkata "sebuah import gagal". Tidak ada state trust yang berubah di jalur ini, jadi ini celah jejak, bukan celah keamanan.
**Suggested fix**: Pasang catatan awal di awal setiap pekerjaan dari isi request (`provider` dan digest yang diharapkan), lalu ganti dengan catatan lengkap sesudah paket terbaca dan sekali lagi di `commit`, seperti `create_instance` memasang catatannya sesudah `validate` (`crates/core/src/instance/create/work.rs:181`). Tambah test: import yang gagal di `store` menghasilkan baris dengan `provider` dan `packageSha256`.

### 🟡 `import --approve` menyetujui digest dari balasan import, bukan digest yang dipreview, `apps/cli/src/commands/provider/import.rs:129`
**Irisan**: B. Diverifikasi sesi utama.
**Problem**: `import` mengirim `expectedPackageSha256` dari preview (baris 104 dan 114), lalu memanggil `approve_detail(cx, detail, …)` dengan `detail` balasan `provider.import`. `approve_detail` mengirim `local.package_sha256` dari detail itu (`apps/cli/src/commands/provider/approve.rs:96` sampai 99). Balasan import dibangun `detail_of` dari snapshot "saat ini" sesudah operasi final (`apps/daemon/src/ipc/provider_import.rs:499`), jadi import lain untuk ID yang sama yang commit di jendela itu membuat digest balasan berbeda dari yang dipreview. CLI tidak pernah membandingkan keduanya.
**Why it matters**: `import --approve --yes` (dan `--json`, yang tidak menampilkan apa pun) bisa mencatat approval untuk byte yang tidak pernah dipreview pemanggilan itu. Butuh import bersamaan oleh user yang sama untuk ID yang sama, jadi peluangnya kecil, tetapi ini tepat di batas trust dan perbaikannya satu perbandingan.
**Suggested fix**: Sesudah `provider.import`, bandingkan `detail.local.packageSha256` dengan digest preview dan berhenti dengan `provider.import_changed` bila berbeda, atau teruskan digest preview ke `approve_detail`. Test `import_approve_yes_imports_then_approves_the_imported_bytes` (`apps/cli/tests/provider/trust.rs:284`) memakai digest yang sama untuk preview dan balasan, jadi tidak bisa membedakan dua sumber itu; tambah kasus dengan balasan ber digest lain.

### 🟡 `install_runtime` tidak memeriksa trust sebelum unduhan dimulai, `crates/core/src/runtime/install.rs:2142`
**Irisan**: sesi utama.
**Problem**: `run_install` memeriksa `stored_trust` hanya di `validate_binary` (baris 2224) dan sebelum `commit` (baris 2262). Langkah `download` (baris 2142) berjalan tanpa cek. Operasi ini `BusyPolicy::Queue`, jadi bisa menunggu lama di antrean, dan ia resumable, jadi bisa melanjutkan `download` sesudah daemon restart.
**Why it matters**: Urutan: install dari entri lokal yang disetujui masuk antrean, kamu mencabut approval, lalu operasi mulai dan tetap mengirim permintaan ke host yang ditulis paket yang sudah dicabut. Eksekusi pertama tetap dicegah, jadi tidak ada binary yang berjalan. Tetapi *Key invariants* berkata tidak ada unduhan dari entri lokal kecuali disetujui, dan revoke yang terjadi sebelum byte pertama diminta semestinya menang. Revoke yang datang di tengah unduhan memang sengaja tidak menghentikannya; itu bukan bagian temuan ini.
**Suggested fix**: Panggil `recheck_local` di awal `run_install`, sebelum `begin_step(DOWNLOAD)` (sesudah cek `existing_runtime`), dan di cabang `StepStart::Run` langkah `download`. Tambah test core: baris import dicabut sebelum pekerjaan mulai, lalu `FakeFetcher` tidak menerima permintaan.

### 🟡 Revoke yang masuk di antara pemindahan folder runtime dan batch akhir membuat install berakhir `needs_recovery`, `crates/core/src/runtime/install.rs:2262`
**Irisan**: sesi utama. Keadaan akhir tidak dibuktikan dengan test.
**Problem**: Sesudah `recheck_local` di baris 2262, `commit_stage` memindahkan folder runtime (langkah `irreversible`), lalu `final_batch` membaca trust lagi (baris 1768 sampai 1788) dan mengembalikan `InstallError::Revoked` kalau revoke masuk di celah itu. Gagal sesudah titik tanpa kembali ditangani runner sebagai `needs_recovery` (`crates/core/src/operation/runner.rs:484` sampai 503), bukan kegagalan yang dikompensasi.
**Why it matters**: Celahnya sempit, dan trust tetap benar (baris runtime tidak ditulis). Tetapi pengguna yang mencabut approval tepat saat install selesai mendapat operasi `needs_recovery` dengan folder runtime tanpa baris registry, padahal spec menulis "revoke yang masuk sebelum commit menggagalkannya" seperti kegagalan biasa. Resume akan gagal lagi dengan `provider.revoked`.
**Suggested fix**: Pastikan dengan satu test apa yang ditawarkan recovery di keadaan ini (abandon harus membersihkan folder itu). Lalu pilih salah satu: catat perilaku ini di spec 0037 *Gate trust*, atau biarkan batch akhir tetap menulis runtime (binary sudah lolos exec check saat approval masih berlaku, dan spec sudah menerima runtime yang tetap terpasang sesudah revoke) sehingga hanya `recheck_local` sebelum `commit_stage` yang menjadi penentu.

### 🟡 Import yang sudah commit membalas `provider.import_not_found` kalau refresh snapshot gagal, `apps/daemon/src/ipc/provider_import.rs:499`
**Irisan**: sesi utama.
**Problem**: Handler membalas dengan `detail_of`, yang membaca snapshot dan menjawab `provider.import_not_found` reason `not_imported` kalau entri tidak ada (baris 289 sampai 305). Kalau `refresh_local` gagal membaca registry sesudah commit, snapshot tetap yang lama (itu memang desainnya), jadi import pertama yang berhasil dijawab dengan error "tidak diimpor". Approve dan revoke dalam keadaan sama menjawab detail basi. `refresh_local` juga tetap menerbitkan `catalog.changed` walau snapshot tidak berganti (`apps/daemon/src/catalog.rs:192` sampai 204).
**Why it matters**: Spec berkata refresh yang gagal tidak menggagalkan operasi yang datanya sudah di commit. Dari sisi client operasi itu tampak gagal, padahal baris dan salinan store sudah ada, dan pengulangan dengan kunci yang sama memberi error yang sama sampai snapshot pulih. Pemicunya jarang (gagal baca registry tepat sesudah commit).
**Suggested fix**: Kalau entri tidak ada di snapshot sesudah operasi yang berhasil, bangun `ProviderDetail` dari baris registry (entri `invalid` lokal tanpa `permissions` sudah punya bentuknya), atau jawab error `unavailable` yang retryable dan menyebut snapshot basi. Jangan terbitkan `catalog.changed` saat penggantian tidak terjadi, atau biarkan tetapi catat alasannya.

### 🟡 Daftar host unduhan dipotong 8 tanpa tanda, dan jumlah URL per artefak tidak dibatasi, `crates/core/src/catalog/local/permissions.rs:340`
**Irisan**: sesi utama.
**Problem**: `hosts` mengurutkan host unik menurut abjad lalu mengambil 8 pertama (baris 335 sampai 342). Loader tidak membatasi jumlah `urls` per artefak (hanya ukuran `versions.json`), dan `check_importable` tidak menambah aturan. Tidak ada hitungan "dan N lagi". Kode perubahan `download_hosts` dihitung dari daftar yang sudah dipotong (baris 364 sampai 370).
**Why it matters**: Dialog approval dan `stackly provider inspect` adalah tempat pengguna memeriksa "sumber unduhannya". Paket dengan 9 host atau lebih bisa menaruh 8 nama yang tampak wajar di depan abjad dan menyembunyikan host yang benar benar dicoba lebih dulu oleh installer. Isi unduhan tetap dipin digest, jadi dampaknya pada kejujuran tampilan dan pada host mana yang menerima permintaan, bukan pada byte yang terpasang. Spec memang menulis "paling banyak 8", jadi ini celah spec dan kode sekaligus.
**Suggested fix**: Tolak saat import artefak yang punya lebih dari 8 host unik (issue `versions_rule` di `check_importable`), supaya yang ditampilkan selalu lengkap. Alternatif: bawa hitungan host yang tidak ditampilkan sampai DTO dan tampilkan di GUI dan CLI.

### 🟡 Aturan path import ditulis dua kali, dan salinan di core tidak dipanggil kode produksi, `crates/core/src/catalog/local/rules.rs:312`
**Irisan**: sesi utama.
**Problem**: `check_import_path` di core hanya dipanggil test (`crates/core/tests/catalog_local.rs`). Aturan yang benar benar berlaku adalah `import_path_code` di `crates/protocol/src/provider.rs:199`, lewat `Validate`. Handler `provider.preview_import` dan `provider.import` langsung membuat `PathBuf` dari `params.path` tanpa memanggil aturan core.
**Why it matters**: Hari ini kedua salinan sama isinya, jadi perilakunya benar. Tetapi ini aturan keamanan (menolak `..`, bukan menormalkan) dengan dua definisi yang bisa bergeser, dan test core memberi rasa aman untuk fungsi yang tidak menjaga apa pun. Pemanggil core yang bukan dispatch (test in process, fitur berikutnya) tidak mendapat aturan itu.
**Suggested fix**: Panggil `check_import_path` di `read_folder` sebagai lapis kedua (error `request.invalid_params`), atau hapus salinan core dan arahkan test nya ke `Validate`. Satu sumber saja.

### 🟡 Approve yang gagal sesudah import yang sudah commit membuang hasil import, `apps/cli/src/commands/provider/approve.rs:100`
**Irisan**: B.
**Problem**: Di rantai `import --approve`, hanya konfirmasi yang ditolak yang mempertahankan entri yang sudah diimpor (baris 90 sampai 94). Penolakan daemon atas `provider.approve` mengembalikan `CommandOutput::failed(failure)`.
**Why it matters**: Dengan `--json`, envelope berisi `result: null` walau entri sudah diimpor. Pemicu yang realistis: revision yang dicabut catalog, yang menurut preview sendiri boleh diimpor tetapi tidak boleh disetujui.
**Suggested fix**: Saat `imported` terisi, kembalikan `partial` dengan detail yang diimpor untuk setiap kegagalan approve. Tambah test.

### 🟡 `approve` tetap bertanya untuk entri yang dicabut catalog atau `invalid`, dan berkata "already approved" untuk entri `invalid`, `apps/cli/src/commands/provider/approve.rs:63`
**Irisan**: B.
**Problem**: Untuk entri `pending` yang dicabut catalog, CLI mencetak izin dan kalimat izin lalu bertanya `Approve …?`, padahal `detail.revocation` sudah berkata daemon akan menjawab `provider.revoked`; revocation tidak dicetak sebelum prompt. Untuk entri `invalid` ia mencetak "Permissions: not available…" dan tetap bertanya. Untuk entri `invalid` yang `approved` ia menjawab "… is already approved." dengan exit 0, sedangkan precheck daemon menolak `PackageInvalid` sebelum jalur tanpa operasi (`apps/daemon/src/ipc/provider_import.rs:622` sampai 629). `imported_line` (`apps/cli/src/commands/provider/import.rs:201` sampai 212) berkata "Approve it before you install anything." juga untuk revision yang dicabut catalog.
**Why it matters**: Pengguna diarahkan ke langkah yang pasti ditolak, dan satu jalur melaporkan sukses untuk entri yang gate nya tertutup.
**Suggested fix**: Cetak baris revocation sebelum kalimat izin, dan jangan tawarkan prompt untuk entri `invalid` atau yang dicabut catalog (atau katakan terang bahwa akan ditolak). Buat `imported_line` bergantung pada `detail.revocation`.

### 🟡 `inspect` menampilkan tanggal revocation catalog di zona lokal, mundur satu hari di barat UTC, `apps/cli/src/commands/provider/local.rs:329`
**Irisan**: B. Diverifikasi sesi utama.
**Problem**: `RevocationInfo.since` untuk revocation catalog adalah tengah malam UTC sebuah tanggal. `date()` memformatnya dengan `out.time(ts)` di zona keluaran lalu mengambil 10 karakter pertama. Di `America/Los_Angeles`, `2026-11-20T00:00:00Z` tercetak `since 2026-11-19`. `provider list` mencetak string `CatalogRevocation.since` mentah (`apps/cli/src/commands/provider/list.rs:152` sampai 166), jadi dua perintah berbeda. Golden berjalan dengan `TZ=UTC`, jadi tidak terlihat.
**Why it matters**: AC-8 meminta revision dan tanggal terbit catalog ditulis apa adanya. Tanggal yang bergeser sehari adalah fakta yang salah tentang revocation.
**Suggested fix**: Format `since` catalog di UTC. Tambah test dengan zona ber offset negatif.

### 🟡 Finding `warning` paket yang bisa diimpor tidak pernah tampil di keluaran manusia, `apps/cli/src/commands/provider/import.rs:111`
**Irisan**: B.
**Problem**: `finding_lines` hanya berjalan di jalur yang ditolak. Untuk preview yang `importable` dengan warning (hari ini `apiVersion` `deprecated`), `import` dan `import --dry-run` hanya mencetak `preview_text`, yang tidak memuat finding.
**Why it matters**: AC-2 berkata finding `warning` ditampilkan tetapi tidak menolak, dan GUI menampilkannya di banner caution. Di CLI hanya `--json` yang membawanya.
**Suggested fix**: Tulis `finding_lines` ke stderr setiap kali `preview.findings` tidak kosong.

### 🟡 Lipatan `..` secara teks bisa menunjuk folder lain dari yang diketik, dan path bukan UTF-8 dikirim lossy, `apps/cli/src/commands/provider/validate.rs:125`
**Irisan**: B (fungsi milik spec 0036, dipakai `import.rs:79`).
**Problem**: `normalize` membuang komponen sebelumnya untuk setiap `..` tanpa menyentuh filesystem. Untuk `stackly provider import link/../pkg` dengan `link` sebuah symlink, CLI mengirim `<cwd>/pkg`, sedangkan kernel akan membuka `<target link>/../pkg`. Daemon sengaja menolak `..` alih alih menormalkannya, dan CLI menutupi itu. `path.display().to_string()` mengganti byte tidak valid dengan U+FFFD, jadi pengguna mendapat `provider.import_unreadable` untuk path yang tidak ia ketik.
**Why it matters**: Baris `Folder` di preview memperlihatkan path akhirnya, tetapi `--approve --yes --json` tidak menampilkan apa pun, jadi folder yang salah bisa diimpor dan disetujui tanpa terlihat.
**Suggested fix**: Lipat hanya `..` yang naik keluar dari argumen ke cwd; tolak `..` sesudah komponen bernama dengan `cli.usage`. Konversi dengan `to_str()` dan gagal `cli.usage` untuk path bukan UTF-8.

### 🟡 `list` menyembunyikan `invalid` untuk entri lokal yang belum disetujui, `apps/cli/src/commands/provider/list.rs:225`
**Irisan**: B. Diverifikasi sesi utama.
**Problem**: Untuk entri lokal, `approval != Approved` mengembalikan `needs approval` sebelum state wire dibaca. Entri `invalid` lokal mempertahankan approval record nya, jadi `invalid` dan `pending` tampil `needs approval`.
**Why it matters**: Daftar mengarahkan pengguna ke `approve`, yang ditolak; obat yang benar adalah import ulang atau remove.
**Suggested fix**: Periksa state `invalid` lebih dulu untuk entri lokal. Tambah baris `invalid` lokal ke golden `list`.

### 🟡 Kalimat untuk pengguna tersebar di luar `copy.rs`, jadi pindaian frasa AC-11 tidak melihatnya, `apps/cli/src/commands/provider/copy.rs:152`
**Irisan**: B.
**Problem**: Spec *Copy* berkata teks CLI ada di satu modul `copy`. Teks lain ada di `import.rs:205`, `approve.rs:64` dan `:102`, `revoke.rs:54`, `remove.rs:39`, `local.rs:153`, `:313`, `:382`, `list.rs:139`, `inspect.rs:65`, dan string `after_help` di `mod.rs`. Pindaian golden (`apps/cli/tests/provider/trust.rs:706`) hanya mencakup teks yang muncul di sebuah golden.
**Why it matters**: Hari ini tidak ada frasa terlarang di luar `copy.rs` (sudah di grep). Tetapi test AC-11 tidak akan menangkap frasa yang ditambahkan nanti di tempat itu.
**Suggested fix**: Pindahkan kalimat itu ke `copy.rs` dan ke `all_copy()`.

### 🟡 Batas nilai baris import hanya dijaga CHECK SQL, dan agen quarantine yang panjang menggagalkan import, `crates/domain/src/registry/import.rs:116`
**Irisan**: A.
**Problem**: `ProviderImportRecord::PACKAGE_BYTES_MAX`, `ORIGIN_PATH_MAX`, dan `QUARANTINE_AGENT_MAX` dideklarasikan tetapi tidak dipakai di luar doc comment. `WriteOp::check_shape`, `import::put` (`crates/storage/src/registry/import.rs:102`), dan `put_provider_import` di fake (`crates/testkit/src/registry/fake.rs:1107`) tidak memeriksanya. `ImportDecision.permissions_json` adalah `String` mentah, sedangkan SQL meminta objek JSON. Pemicu nyata: `crates/platform-macos/src/codesign.rs:131` sampai 138 mengambil nama agen dari buffer xattr 512 byte tanpa batas, dan handler serta pekerjaan import meneruskannya apa adanya. Agen 129 byte atau lebih menabrak `CHECK (… BETWEEN 1 AND 128)`.
**Why it matters**: Import gagal dengan `RegistryError::Integrity` (error internal) sesudah preview berkata bisa diimpor, dan atribut itu sepenuhnya dikendalikan pembuat folder. `crates/storage/src/registry/write.rs:1` sampai 8 sendiri berkata constraint SQL yang menolak berarti ada cek yang terlewat. Fake menerima record yang sama, jadi test core tidak bisa melihatnya.
**Suggested fix**: Tegakkan ketiga batas dan "permissions adalah objek JSON" di domain (konstruktor atau `check_shape`, dipanggil kedua registry), dan potong nama agen di adapter atau di handler. Tambah satu kasus conformance per batas. Newtype JSON seperti `ConfigValuesJson` menutup `permissions_json`.

### 🟡 `quarantine_agent` disimpan tetapi tidak pernah dibaca, sedangkan AC-13 memintanya di detail, `crates/protocol/src/provider.rs:545`
**Irisan**: A.
**Problem**: AC-13 berkata preview dan detail melaporkan `quarantined` dan nama agen bila terbaca. `LocalProviderInfo` hanya membawa `quarantined`, dan `apps/daemon/src/map/trust.rs:145` hanya memetakan itu. Tidak ada kode selain test yang membaca `provider_imports.quarantine_agent`. Daftar DTO di spec sendiri juga tidak memuat field itu, jadi ini ketidakcocokan spec yang diwarisi kode.
**Why it matters**: Kolom itu hanya ditulis, dan justru kolom itulah yang bisa menggagalkan import di temuan sebelumnya.
**Suggested fix**: Tambah `quarantineAgent` ke `LocalProviderInfo` (kenaikan minor protokol), atau hapus kolom dan kalimat AC-13 nya lewat architect.

### 🟡 Hasil yang datang terlambat tetap diterapkan sesudah sheet ditutup atau berganti, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/ProviderImportModel.swift:146`
**Irisan**: C. Bagian approve diverifikasi sesi utama.
**Problem**: Cancel tetap aktif selama request berjalan, dan kedua model tidak memeriksa ulang state sesudah `await send`. Import: `cancel()` berkata jawabannya dibuang, tetapi penolakan yang datang kemudian memasang `phase = .failed(...)`, yang membuka lagi sheet atau menimpa preview baru; sukses yang terlambat tetap menjalankan `imported`. Approve (`ProviderDecisionModel.swift:78` sampai 107): sesudah Cancel, approval tetap terjadi dan "Approved X" tampil; `.completed` memasang `approval = nil` dan `fail(body)` menulis ke sheet mana pun yang sedang terbuka, juga sheet provider lain.
**Why it matters**: Pengguna yang menekan Cancel di sheet izin mengira tidak ada yang disetujui, padahal request nya sudah terkirim dan berhasil. Operasi ini melaporkan `cancel: none`, jadi Cancel memang tidak bisa menariknya.
**Suggested fix**: Nonaktifkan Cancel selama request berjalan. Atau sesudah `await`, periksa bahwa `phase` masih `.importing` untuk path itu, dan bahwa `approval?.detail` masih detail yang sama, sebelum menulis state.

### 🟡 Folder yang tidak terbaca menampilkan spinner tetap di sebelah Import, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/ImportProviderSheet.swift:66`
**Irisan**: C.
**Problem**: `isBusy: model.phase != .idle` bernilai true untuk `.unreadable`, jadi footer menggambar `ProgressView` di sebelah tombol Import yang nonaktif, di bawah "Can’t Read This Folder".
**Why it matters**: State gagal tampak seperti masih bekerja. Tidak ada snapshot untuk state ini, walau AC-15 memintanya.
**Suggested fix**: Sibuk hanya untuk `.loading` dan `.importing`. Tambah scenario snapshot `unreadable`.

### 🟡 Sheet approve tidak punya tombol default atau tombol batal lewat keyboard, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Components/Sheet/StacklySheet.swift:72`
**Irisan**: C. Butuh keputusan.
**Problem**: *Layar* berkata Cancel adalah tombol default, juga lewat Escape. Kode hanya menggambar Cancel sebagai `.borderedProminent`; tidak ada `keyboardShortcut` di `Sources/StacklyUI`, dan `.interactiveDismissDisabled()` terpasang (baris 72). Doc comment di baris 9 sampai 15 ("Return runs the primary action", "Escape runs Cancel") tidak cocok dengan kodenya.
**Why it matters**: Ini gagal ke arah aman (Return tidak menyetujui apa pun) dan cocok dengan Interaction policy di `AGENTS.md` (tanpa shortcut keyboard), tetapi kalimat spec dan doc comment berkata lain.
**Suggested fix**: Putuskan lewat architect mana yang berlaku. Kalau policy yang menang, perbaiki kalimat *Layar* spec 0037 dan doc comment `StacklySheet`.

### 🟡 Baris Settings kehilangan "imported {date}" sesudah catalog dimuat ulang, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Settings/SettingsTabs.swift:131`
**Irisan**: C.
**Problem**: ID task nya `localKeys` (`provider@catalogRevision`), yang tidak berubah saat approve, revoke, atau reconnect, padahal setiap snapshot membuang cache detail. `CatalogPage` memasukkan `catalog.detail.epoch` ke ID task nya untuk alasan ini; Settings tidak.
**Why it matters**: Sub baris "Imported providers" kehilangan tanggalnya sesudah aksi apa pun sampai tab dibuka ulang.
**Suggested fix**: Tambah epoch ke ID task itu.

### 🟡 Umpan balik import selalu berkata "Approve it before you install anything.", `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/CatalogPage.swift:172`
**Irisan**: C.
**Problem**: Teksnya tidak membaca `detail.summary.approval`. "Import Again" pada entri rusak yang sudah disetujui, atau import ulang byte yang keputusannya tersimpan, mengembalikan entri `approved` (AC-6), dan kartu tetap menyuruh menyetujui. CLI punya kekeliruan yang sama untuk revision yang dicabut catalog.
**Why it matters**: Pesan yang salah tentang state trust, tepat sesudah aksi yang mengubahnya.
**Suggested fix**: Pilih kalimat dari `approval` dan `revocation` detail yang dibalas daemon.

### 🟡 Pindaian frasa terlarang di app berupa daftar yang dirawat tangan, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/ProviderTrustCopy.swift:438`
**Irisan**: C.
**Problem**: AC-11 meminta test aturan sumber. `allSamples` tidak memuat `source`, `settingsLine`, `hostLine`, `endpointText`, `openWithLabel`, `folderLabel`, dan `previewSubtitle`. Teks banner hanya terjangkau lewat fixture yang diputar test, dan "Changed since you revoked it" tidak terjangkau fixture mana pun. Copy fitur ini juga ada di luar file itu: `CatalogCopy` ("Local, not verified", "Needs your approval."), `CatalogAppearance`, dan literal di view ("Permissions", "Files").
**Why it matters**: Hari ini tidak ada frasa terlarang. Tetapi test nya tidak menjaga kalimat yang ditambahkan nanti, sama seperti temuan `copy.rs` di CLI.
**Suggested fix**: Pindai file sumber `Catalog/` dan `Settings/` secara langsung (suite aturan sumber yang sudah ada polanya), bukan daftar sampel.

### 🟡 Conformance registry tidak membedakan klausa WHERE dua operasi approval, dan tiga baca `GuardView` baru tidak diuji, `crates/testkit/src/registry/conformance/imports.rs:298`
**Irisan**: A.
**Problem**: `old` dan `new` disetujui dalam satu commit (baris 331 sampai 350) lalu keduanya dicabut. Tidak ada baris provider yang sama dengan approval NULL atau yang sudah `revoked`, dan tidak ada pernyataan bahwa menyetujui satu kunci membiarkan revision saudaranya. `conformance/guarded.rs` hanya memakai `view.instances` dan `view.runtime`; `view.provider_imports`, `view.provider_import`, dan `view.provider_has_approved_revision` (`crates/domain/src/registry/guard.rs:124` sampai 153) tidak dipanggil di storage atau testkit.
**Why it matters**: Implementasi yang membuang `AND approval = 'approved'` dari `RevokeProviderApprovals`, atau yang mencocokkan `SetProviderRevisionApproval` hanya lewat provider, lulus di fake dan di SQLite. Itu aturan AC-5 ("baris revision lain tidak berubah") dan AC-7 serta AC-9 ("hanya baris `approved`"). Kode hari ini benar; test nya yang tidak mengunci.
**Suggested fix**: Setujui `old` saja dan pastikan `new` tetap `(None, None)`. Simpan revision ketiga tanpa approval dan pastikan tetap NULL sesudah revoke. Tambah skenario guarded yang meng commit sebuah import, membacanya lewat view, lalu menolak atau menulis dari yang ia lihat.

### 🟡 `codes::provider::hint` tidak punya test unit, berbeda dari area lain, `crates/protocol/src/codes.rs:365`
**Irisan**: A.
**Problem**: Hint runtime, catalog, instance, dan supervisor masing masing punya test untuk batas 200 karakter dan varian per detail (`codes.rs:1903` sampai 2052). Hint provider punya sepuluh cabang (`permission_denied`, `not_local`, `by` user, catalog, dan tidak dikenal) tanpa test. Fixture hanya mencakup satu varian per kode.
**Why it matters**: String sekarang paling panjang 154 karakter, tetapi suntingan berikutnya bisa melewati 200 tanpa terlihat.
**Suggested fix**: Tambah test dengan gaya yang sudah ada untuk setiap cabang, batas panjang, dan `None` untuk kode spec 0009.

## Nits

- ⚪ `crates/core/src/catalog/trust.rs:211`, `pinned_trust` memanggil `namespace_allowed(key.provider(), true)`, jadi pin `testing/…` tanpa approval dipercaya juga di luar test. Spec menulis "`testing` di test". Tidak terjangkau di produksi hari ini (loader dan import menolak namespace itu), tetapi teruskan `CatalogOptions` supaya invariant "namespace di luar catalog tidak pernah dipercaya tanpa approval" berlaku harfiah.
- ⚪ `crates/core/src/catalog/store.rs:219`, file yang sudah ada dibaca sampai `info.len` byte sebelum dibandingkan. Bandingkan panjangnya dulu; file yang panjangnya berbeda sudah pasti berbeda (perilaku lama `pin_revision`, kini juga dipakai mode import yang menulis ulang).
- ⚪ `crates/core/src/catalog/load.rs:175`, entri `revoked` yang gagal di parse dibuang diam diam oleh `filter_map`. `revoked_problems` sudah menolaknya lebih dulu, jadi tidak terjangkau; tetap lebih aman gagal tertutup (`index_invalid`) daripada membuang sebuah revocation.
- ⚪ `apps/daemon/src/ipc/provider_import.rs:603`, precheck approve dan revoke menangkap baris yang dibaca handler sebelum antre, bukan membacanya di dalam precheck seperti import dan remove. Guard memeriksa ulang, jadi hanya jawaban tanpa operasi yang bisa basi; samakan polanya.
- ⚪ `crates/core/src/catalog/local/permissions.rs:357`, kode `downloads` hanya membandingkan (`artifact`, `sha256`), sesuai spec. `signing_policy` yang melemah untuk artefak yang sama tidak punya kode sendiri dan bisa tertutup kode lain. Pertimbangkan memasukkannya ke `downloads`.
- ⚪ `apps/cli/src/commands/provider/approve.rs:92`, pada `import --approve` yang ditolak, baris "Imported …" tercetak ke stderr lalu ke stdout, dan blok izin tercetak dua kali.
- ⚪ `apps/cli/src/commands/provider/local.rs:138`, `!will_ask(cx, yes) && yes` berlebih; `will_ask` selalu false saat `yes`.
- ⚪ `apps/cli/src/commands/provider/local.rs:310`, teks revocation catalog di `inspect` menyebut revision catalog tetapi tidak tanggal terbitnya; AC-8 dan banner GUI menyebut keduanya.
- ⚪ `apps/cli/src/commands/provider/local.rs:30`, `RequestTimeout::Interactive` (200 s) dipakai untuk operasi provider yang antre; antrean lebih lama berakhir exit 8 tanpa cara mengulang kunci. Beri komentar.
- ⚪ `apps/cli/src/commands/provider/revoke.rs` dan `remove.rs`, tiga struct argumen identik dan dua `run` berbentuk sama; satu helper cukup.
- ⚪ `apps/cli/src/commands/provider/local.rs`, `#[must_use]` hanya ada di `counts`, tidak di helper murni lain (`shows_text`, `will_ask`, `revocation_text`, `date`, `esc`, `local_rows`).
- ⚪ `apps/cli/tests/provider/trust.rs:598`, test exit code membangun error dengan kategori dari tabel test sendiri, jadi membuktikan kategori ke exit, bukan kode ke kategori. Bangun body dari `stackly_protocol::codes::provider::*`.
- ⚪ `crates/storage/tests/open/imports.rs:130`, daftar baris yang ditolak hanya menguji batas atas `package_bytes`, `origin_path` kosong, dan satu bentuk keputusan setengah. Belum ada: `package_bytes = 0`, path 1025 byte, agen 129 byte, `decision` NULL dengan satu `decided_*` terisi, dan menghapus sumber yang masih dirujuk (RESTRICT).
- ⚪ `crates/storage/src/registry/import.rs:132`, `PutProviderImport` memeriksa bahwa sumbernya ada, bukan bahwa `kind` nya `local` (spec: "selalu baris sumber `local`"). Fake dan SQLite sepakat, jadi tidak ada yang menyimpang.
- ⚪ `crates/domain/src/registry/batch.rs:414`, tiga geseran kecil: spec menulis `PutProviderImport(ProviderImportRecord)` sedangkan kode `{ record, expected }` (kodenya lebih baik, selaraskan spec); `RevokeProviderApprovals` melaporkan `ResourceRef::ProviderImport` padahal menulis `provider_revisions`; `DecideProviderImport` menaikkan revision baris tetapi `Committed.revisions` tidak memuatnya.
- ⚪ `crates/protocol/src/provider.rs:219`, cek `provider.len() <= PROVIDER_ID_MAX` mati, karena `provider_id_code` sudah mengembalikan `TooLong`. Test path di baris 839 tidak punya kasus terima tepat 1024 byte.
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/ProviderPermissionsList.swift:96`, `Text("\(download.artifact) · \(download.version)")` adalah satu satunya tempat nilai paket masuk lewat `LocalizedStringKey`; tempat lain memberi `String`. Belum dibuktikan bahwa Markdown di argumen interpolasi dirender; pakai `Text(verbatim:)` supaya tidak perlu bergantung pada itu.
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/ProviderImportModel.swift:38`, kunci yang disimpan sesudah `outcomeUnknown` tidak pernah dipakai lagi (satu satunya jalan balik ke `.preview` adalah `read()`, yang menghapusnya), jadi doc comment nya keliru.
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/ProviderTrustCopy.swift:363`, approval `.unrecognized` mendapat banner "Not approved yet" dengan Approve…, dan revocation ber `by` tidak dikenal tidak mendapat banner saat `approved`.
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/CatalogStore.swift:76`, siklus retain (store, `reduce`, `ReloadBox`, `run`, store). Tangkap store secara weak.
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/ImportProviderSheet.swift:265`, butir "and N more" punya reason kosong, jadi barisnya berakhir dengan " · " yang menggantung. Di baris 87, preview yang bisa diimpor tanpa digest melaporkan "This exact package is already imported."
- ⚪ `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/ProviderDetailModel.swift:8`, header masih berkata catalog tidak berubah selama daemon berjalan.

## Strengths

- Satu modul gate (`crates/core/src/catalog/trust.rs`) dipakai semua jalur, dan pembagian "tampilan dari snapshot, gate dari registry" konsisten: `require_entry_trust`, `stored_trust`, dan `guard_trust` selalu membaca ulang baris dan membandingkan `package_sha256`, dan gagal baca tidak pernah berarti `Trusted`.
- TOCTOU antara baca dan salin ditutup dengan benar: pekerjaan import menyalin byte yang sama dengan yang ia hash (`import.rs:480` sampai 535), `write_package` membaca ulang dan mencocokkan digest, `local::load` mencocokkan lima kunci baris, dan approve menghitung izin dari salinan store di bawah lock provider.
- Kepemilikan folder store dipikirkan sampai kasus gagal: hanya folder yang belum ada sebelum penulisan yang diakui, kompensator melewati digest yang masih dirujuk, dan perbaikan salinan yang rusak tidak pernah menghapus folder yang sudah ada.
- Baris pin ditulis di commit yang sama dengan `recheck_trust` (`create/work.rs:694` dan 718), jadi revoke yang menang balapan tidak meninggalkan baris `approved`.
- CLI meloloskan setiap string daemon lewat `esc` atau `Cell` sebelum mencetak, termasuk karakter bidi dan zero width, dan syarat konfirmasi satu fungsi murni yang dipanggil sebelum `provider.import`.
- `ProviderDecisionModel` menyimpan `detail` di dalam state sheet dan mengirim digest dari salinan itu, jadi detail yang dimuat ulang di bawah sheet terbuka tidak mengubah apa yang disetujui.

## Test coverage

Sinyal test: `configured`. Cakupan fitur ini luas dan test nya menegaskan perilaku, bukan hanya jalur bahagia: gate tertutup di setiap titik dengan hitungan spawn dan fetch nol, balapan revoke dengan create selama enam ronde dengan pemeriksaan baris dan folder, delapan import bersamaan, batas 32 dan 16 MiB, snapshot ber generation lebih tua, replay kunci untuk keempat kasus K26, frame penuh di bawah 1 MiB, dan perbandingan byte sebelum dan sesudah revoke serta remove.

Yang belum tertutup, selain yang disebut di tiap temuan:

- Tidak ada test yang menyentuh doctor dengan entri lokal di snapshot (temuan Major).
- Revoke selama unduhan `install_runtime` diuji lewat baris import yang tidak ada (`crates/core/tests/runtime_install.rs:582`), bukan lewat revoke yang benar benar masuk saat `FakeFetcher` sedang melayani; skenario kritis spec menyebut yang kedua.
- Jalur balasan saat refresh snapshot gagal sesudah commit (import, approve, revoke) tidak diuji dari sisi handler.
- CLI: `import --approve` yang ditolak di TTY, `import --approve` dengan approve yang gagal, catatan preview untuk pengganti, salinan rusak, dicabut catalog, dan batas import, cabang catalog `revocation_text`, baris `invalid` lokal di `list`, path bukan UTF-8, dan zona selain UTC.
- Storage dan protokol: batas nilai di temuan batas baris import, dua celah conformance, test hint provider, dan kasus `InvalidRevision` untuk `PutProviderImport`.
- App: fase `choosing`, `loading`, dan `importing` tidak pernah diamati test; hasil `outcomeUnknown` dan `notSent`; Cancel saat request berjalan dan hasil basi; penjaga kirim ganda `isWorking`; kesegaran kunci (`newKey` konstan di test); banner "Changed since you revoked it"; `invalidate` saat detail sedang dimuat. State AC-15 tanpa snapshot: loading, unreadable, paket sama, dan entri `invalid` lokal.
