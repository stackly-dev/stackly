# Verify: Menu bar dan Settings · spec 0032 · updated 2026-11-03

_Langkah diturunkan dari acceptance criteria spec 0032. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah._

## Milestone 1: Setting daemon dan lokasi, sampai CLI

Langkah build 1 sampai 5 (commit `6f1d95dd`, `2e9fb672`, `48f2f33b`, `de7ea0e8`, `9d0a543d`). Bagian AC yang belum: tab Settings di GUI (milestone 2), Stop All dan precheck `expectedInstanceIds` (langkah 9, milestone 3), menu bar, notifikasi, dan semua evidence native.

### UI / manual

- [x] Buka `crates/storage/migrations/0011_settings_locations.sql` → tabel `storage_locations` (tiga baris bawaan dengan ID tetap, trigger tanpa update dan delete), `daemon_settings` (CHECK `id = 1`, trigger lokasi harus sejenis), `instance_paths` dibangun ulang dengan `location_id`, cek jumlah baris yang gagal kalau salinan kurang, dua index parsial → AC-25
- [x] Buka `crates/core/src/storage/location.rs` → `check_location` fungsi murni yang menolak aturan pertama yang gagal sesuai urutan tabel; `prepare_location` membuat `Stackly` 0700, subfolder layout, dan `.stackly-location` 0600 → AC-19
- [x] Buka `crates/core/src/layout.rs` → `LocationBook` di dalam `PreparedLayout`: setiap path instance dihitung dari lokasi instance itu, instance yang tidak dikenal jatuh ke lokasi bawaan → AC-19, AC-25
- [x] Buka `crates/core/src/instance/create/work.rs` → langkah `layout` mencatat pasangan lokasi sebelum membuat folder, langkah `register` membangun instance di dalam guard dan menulisnya dengan `InsertInstanceAt` → AC-18, AC-19
- [x] Buka `apps/daemon/src/ipc/settings.rs` → `settings.update` dan `storage.set_location` mengikuti `instance.set_startup`: precheck, operasi, tunggu, balas; `settings.changed` sekali per perubahan yang diterima → AC-16, AC-17
- [x] Jalankan `target/debug/stackly settings --help` dan `target/debug/stackly storage --help` → verb `get`, `set`, `locations`, `set-location` → AC-16, AC-19
- [x] Jalankan `target/debug/stackly instance create --help` → `--autostart` dan `--no-autostart` saling menolak → AC-18

### Commands

- [x] `cargo test -p stackly-protocol` → fixture round trip untuk lima method baru, event `settings.changed`, tiga kode error, `expectedInstanceIds`, form dengan `startupDefault` dan lokasi, nilai tidak dikenal, dan penolakan patch kosong → AC-16, AC-17, AC-18, AC-19
- [x] `cargo xtask codegen --check` → "20 generated files are up to date"; `ProtocolVersion::CURRENT` 3.0, `protocols/contract-versions.json` tanpa entri 2.6 → AC-16
- [x] `cargo test -p stackly-domain location` dan `cargo test -p stackly-platform layout` → ID bawaan tetap, patch yang tidak mengubah apa pun tidak menaikkan revision, 1000 instance di tiga lokasi resolve persis seperti `instance_paths_in` dan tidak ada path yang sama atau bertumpuk (*AC-10 per lokasi*) → AC-25
- [x] `cargo test -p stackly-storage` → migrasi 11 (fingerprint, fixture beku `registry-v11.sql`), v10 ke v11 dengan semua path sama persis dan `location_id` bawaan, migrasi 11 yang gagal sesudah salinan tidak mengubah registry dan snapshot ada, aturan tabel (satu baris pengaturan, satu lokasi bawaan per jenis, lokasi tidak diubah atau dihapus, jenis lokasi cocok dengan akar path), konformasi `settings` untuk SQLite dan fake → AC-25, AC-16, AC-19
- [x] `cargo test -p stackly-core --test storage_location` → tabel uji per `rule` dan urutannya di `FakePlatform` (relatif, hilang, bukan folder, symlink, bukan milik user, terlalu terbuka, folder yang dijaga macOS, bukan lokal, bukan internal, menimpa root, penanda asing atau dipakai lagi) dan `prepare_location` → AC-19
- [x] `cargo test -p stackly-core --test create_operation --test settings_ops --test supervisor_start` → create sesudah ganti lokasi masuk ke lokasi baru dan instance lama tetap, create tanpa `startup` memakai default dan yang eksplisit menang, folder lokasi yang hilang menolak create, start memeriksa lokasi instance sebelum data (`instance.storage_location_missing`, `storage.location_invalid`), precheck settings, inventory lokasi kustom → AC-18, AC-19, AC-20
- [x] `cargo test -p stacklyd --test supervisor settings storage_locations` → defaults dan retensi log (AC-21), update parsial dan satu event, revision usang tidak menulis apa pun, patch tanpa perubahan tanpa operasi dan tanpa event, patch kosong ditolak, default startup untuk create dan form, folder baru untuk data dan cache di filesystem asli, tujuh aturan yang bisa dilanggar folder asli, folder yang dicabut menghentikan create dan start → AC-16, AC-17, AC-18, AC-19, AC-20, AC-21
- [x] `cargo test -p stackly-cli --test settings` dan `--test instance create` → output dan params `settings get` dan `set`, exit 3 dengan "Settings changed while saving. Run the command again." tanpa mengulang, kalimat aturan di `set-location`, path relatif menjadi absolut, `--autostart`, `--no-autostart`, dan tanpa flag tidak mengirim `startup` → AC-16, AC-17, AC-18, AC-19
- [x] `swift test --package-path clients/swift` dan `swift test --package-path apps/macos/Packages/StacklyUI` → 247 test client lulus; StacklyUI lulus kecuali `CreateInstanceModelTests` yang flaky dan lulus kalau dijalankan sendiri; hanya tipe hasil codegen dan perbaikan mekanis (form dengan tiga field baru, dua judul operasi) → AC-16
- [x] `xcrun swift-format lint --strict` dan `cargo xtask build-app` → bersih
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, `cargo xtask evidence check`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → bersih
- [ ] Volume external dan network sungguhan untuk aturan `not_internal_volume` dan `not_local_volume` → `BLOCKED`, reason `no external or network volume on this Mac`. Aturan itu dibuktikan di `FakePlatform`; pembacaan `NSURLVolumeIsInternalKey` di macOS dibuktikan oleh test daemon di filesystem asli, yang lolos aturan itu di volume internal → AC-19

### Acceptance-criteria coverage

- AC-16 · `settings.get`, `settings.update`, `settings.changed`, `stackly settings get` dan `set`; refetch di client menunggu milestone 2
- AC-17 · precheck dan guard memeriksa revision (`resource.revision_mismatch`), CLI keluar 3 tanpa mengulang; baris GUI "Changed elsewhere" menunggu milestone 2
- AC-18 · `defaultStartup` dibaca di dalam guard create, `startupDefault` di form create dan duplicate, `--no-autostart`; mengubah default tidak mengubah instance yang ada
- AC-19 · `storage.set_location`, `storage.list_locations`, dua belas aturan, layout lokasi, Use Default, jumlah pemakai, status terjangkau; panel folder dan konfirmasi menunggu milestone 2
- AC-20 · start memeriksa lokasi instance sebelum cek data, preflight create punya baris `storage_location`
- AC-21 · `logRetention` dari konstanta log store; baris di tab Storage menunggu milestone 2
- AC-25 · migrasi 11, test migrasi, *AC-10 per lokasi*
- AC-7, AC-8, AC-9 · hanya kontrak (`expectedInstanceIds`, `instance.stop_all_changed`); precheck dan CLI di langkah 9

### Catatan

- **Bentuk balasan.** `settings.update` membalas `DaemonSettingsView` dan `storage.set_location` membalas `StorageLocationView` lokasi default jenis itu, mengikuti `instance.set_startup`, karena `OperationResult` tidak membawa payload bertipe. Spec menulis `OperationView`; spec perlu mengikuti.
- **Versi protokol.** Naik mayor ke 3.0, bukan minor 2.6 seperti teks spec (baris 205, 428, 478). Keputusan lead atas delegasi user: `EventKind` tertutup, jadi client 2.x yang membaca `settings.changed` gagal decode, dan break harus naik mayor (spec 0030 AC-18 sama). Versi 2.6 tidak pernah masuk `main`, jadi entrinya dihapus dari `protocols/contract-versions.json` dan 3.0 dicatat dengan digest yang sama. Teks spec diselaraskan lead lewat `/sync`.
- **Waktu baris awal.** Migrasi tidak boleh membaca jam, jadi tiga lokasi bawaan dan baris pengaturan pertama memakai epoch Unix, bukan "waktu migration" seperti di spec. `updatedAt` baru berarti sejak perubahan pertama.
- **`InsertInstanceAt`.** Create membuat folder di langkah `layout` sebelum commit, jadi pasangan lokasi harus ikut ke commit. WriteOp baru `InsertInstanceAt { instance, data, logs }` dipakai create; `InsertInstance` tetap mengambil pasangan dari baris pengaturan di dalam transaksi yang sama. Create yang bersamaan dengan ganti lokasi memakai satu pasangan utuh, tidak pernah campuran.
- **Satu `LocationBook`.** Daripada mengoper `LocationSet` ke setiap pemakai, `PreparedLayout` membawa buku lokasi bersama, diisi saat registry siap dan saat `storage.set_location` selesai. `LocationSet::resolve` mengembalikan `Result` karena lokasi yang tidak dikenal tidak boleh diam diam jatuh ke lokasi bawaan. Daftar pemakai yang pindah ada di pesan commit langkah 3.
- **`check_location`.** Mengembalikan `LocationVerdict` (`New` atau `Reuse(id)`) dan mencakup aturan 1 sampai 11; aturan 12 `not_writable` datang dari `prepare_location`. Symlink ke folder lolos aturan 3 supaya dilaporkan sebagai `symlink` (aturan 4). Leluhur yang tidak bisa dibaca tidak dihitung sebagai symlink.
- **Folder kustom adalah root yang dimiliki.** `check_owned_path` dan `remove_owned_tree` menerima folder lokasi kustom; `OwnedPath` kini membawa folder root nya.
- **Unduhan.** Instal runtime membaca folder unduhan sekali saat operasi mulai, supaya perubahan lokasi cache di tengah jalan tidak memecah satu instal.
- **Perintah setara.** Perintah setara di sheet create hanya menulis `--autostart`; `--no-autostart` eksplisit tidak ditulis karena `CommandDraft.autostart` adalah bool. Berlaku hanya saat default di daemon menyala.
- **Lokasi file test.** Test daemon ada di `apps/daemon/tests/supervisor/settings.rs` dan `storage_locations.rs` karena rig `Site` dan `Api` ada di sana.
- **Tidak terkirim.** Pesan ke `dev-f31-m3` soal blok kecil di `apps/daemon/src/server.rs` gagal terkirim (sesi itu tidak terhubung ke sesi ini). Perubahannya satu blok sesudah `core.set_registry(RegistryState::Ready { .. })`.

## Milestone 3: Menu bar dan Stop All

Langkah build 9 saja (daemon dan CLI). Langkah 10 dan 11 (scene `MenuBarExtra`, popover, `ActivationPolicyController`) menyusul di sesi Swift, jadi AC-1 sampai AC-6, AC-10, AC-11, dan AC-12 belum tercakup di bagian ini.

### UI / manual

- [x] Buka `crates/core/src/instance/stop_all.rs` → `confirmed_steps` membandingkan himpunan `stop` dari `select` dengan `expectedInstanceIds` sebagai himpunan (urutan dan ulangan tidak berarti), menolak dengan `StopAllChanged` kalau berbeda, dan mengembalikan irisan dalam urutan nama → AC-7, AC-8
- [x] Buka `apps/daemon/src/ipc/stop_all.rs` → urutan `replay_operation`, pilihan, `confirmed_steps`, baru `start_operation`; `instance.stop_all_changed` dikirim sebelum operasi dibuat, jadi kunci idempotency tidak terikat; field itu diurutkan dan dibuang ulangannya sebelum sidik jari → AC-7, AC-8
- [ ] Jalankan `target/debug/stackly instance stop --help` → `--all`, dan `--yes` berlaku untuk `--force` dan `--all` → AC-9
- [ ] Jalankan `target/debug/stackly instance stop --all` di terminal dengan dua instance berjalan → baris preview di stderr ("alpha · Started by you"), pertanyaan "Stop All Services?", dan kalimat "Data is kept. …"; `y` menghentikan, Enter menolak dengan exit 9 → AC-9

### Commands

- [x] `cargo test -p stackly-core --test stop_all_select` → enam test baru untuk `confirmed_steps`: tanpa field, himpunan sama dalam urutan lain dengan ulangan, instance yang bergabung sejak preview, yang keluar, `already_stopping` tidak dihitung, dan himpunan kosong → AC-7, AC-8
- [x] `cargo test -p stacklyd --test stop_all` → lima test di daemon dan filesystem asli: konfirmasi preview menghentikan persis isinya dan replay (urutan lain, dengan ulangan) menjawab sama; instance yang menyala sesudah preview ditolak `instance.stop_all_changed`, tidak ada yang berhenti, dan kunci yang sama dipakai lagi dengan preview baru; instance yang berhenti sesudah preview, ID bukan instance, dan himpunan kosong di atas instance berjalan ditolak; kunci yang sama dengan himpunan lain, atau tanpa field, ditolak `operation.idempotency_key_reused` → AC-7, AC-8
- [x] `cargo test -p stackly-cli --bin stackly stop` → teks preview, kaki, hasil, dan kegagalan mengikuti tabel *Copy Stop All* → AC-9
- [x] `cargo test -p stackly-cli --test instance stop_all` → `--yes` mengirim `expectedInstanceIds` dari preview dengan kunci; `--json` menulis satu envelope `{ preview, outcome }`; tidak ada yang berjalan menulis "Nothing is running." tanpa mengirim stop; tanpa `--yes` dan tanpa terminal (juga dengan `--json`) keluar 2 `cli.confirmation_required` sebelum stop dikirim; di terminal `y` menghentikan dan Enter keluar 9; `instance.stop_all_changed` keluar 3 tanpa mengulang; instance yang gagal berhenti disebut namanya dan keluar sesuai kodenya; `--all` menolak nama instance, `--force`, dan `--wait` → AC-8, AC-9
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, `cargo xtask codegen --check`, `cargo xtask evidence check`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → bersih

### Acceptance-criteria coverage

- AC-7 · konfirmasi mengirim `expectedInstanceIds` dari preview, operasi hanya membuat langkah untuk irisan, hasil `StopAllResult` dengan `failed` dan `skipped`; popover yang menggambar hasil menunggu langkah 10
- AC-8 · `instance.stop_all_changed` sebelum operasi dibuat, tanpa payload; CLI keluar 3 dan menyuruh menjalankan ulang; popover yang memanggil preview lagi menunggu langkah 10
- AC-9 · `stackly instance stop --all [--yes] [--json]` dengan preview, konfirmasi, exit 2 tanpa terminal dan tanpa `--yes`, dan envelope `{ preview, outcome }`

### Catatan

- **Hasil kosong.** Kalau preview tidak punya `items`, CLI menulis "Nothing is running.", tidak meminta konfirmasi, dan tidak mengirim `instance.stop_all`; `outcome` null. Spec tidak menyebut kasus ini untuk CLI.
- **Keluar sesudah sebagian gagal.** `stop --all` keluar dengan kategori kode instance pertama di `failed` (misalnya `supervisor.stop_timeout`), atau `operation.cancelled` kalau Stop All dibatalkan. `skipped` biasa (lease environment, `unknown`) tidak membuat perintah gagal karena sudah tampil di preview. Spec hanya menetapkan exit 3 untuk `instance.stop_all_changed`; tidak ada kode baru yang ditambahkan.
- **Kirim ulang.** Pada `ipc.call_timeout` CLI mengirim ulang dengan kunci yang sama, sampai tiga kali (spec 0031, *Operasi `stop_all_instances`* langkah 9).
- **`--all` dan `--wait`.** Keduanya saling menolak, karena balasan `instance.stop_all` baru datang setelah operasi selesai.
- **Lokasi test daemon.** `apps/daemon/tests/stop_all.rs` memakai rig `Site` dan `Api` dari `tests/supervisor/support.rs` lewat `#[path]`, supaya tidak menyentuh `supervisor/main.rs` milik sesi lain. Test `supervisor/stop_all.rs` milik fitur 31 tetap menguji `instance.stop_all` tanpa field.
- **Berkas CLI.** `StopArgs` ada di `action.rs`, jadi flag `--all` dan penyaluran ke `stop.rs` mengubah `action.rs` dan `mod.rs` selain `stop.rs`.

