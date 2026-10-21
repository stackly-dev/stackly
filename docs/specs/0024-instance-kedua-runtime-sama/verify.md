# Verify: Instance kedua dari runtime yang sama · spec 0024 · updated 2026-10-22

_Langkah diturunkan dari acceptance criteria spec 0024. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah._

## Milestone 1: Duplicate draft in core and the fake sentinel

Langkah Build plan 1 dan 2. Tidak ada UI di milestone ini, jadi bagian manual hanya memeriksa fake server dengan tangan.

### UI / manual

- [x] Jalankan `cargo build -p stackly-fake-server`, buat folder data dan config uji, jalankan `target/debug/stackly-fake-server serve --config <config.json> --data <folder> --port <port>`, lalu `curl -i -X PUT --data 'alpha' http://127.0.0.1:<port>/sentinel` → 204 tanpa body, `<folder>/SENTINEL` berisi `alpha` → AC-8 (alat bukti)
- [x] `curl -i http://127.0.0.1:<port>/sentinel` → 200 dengan isi `alpha`; hentikan server, jalankan lagi di folder yang sama, `GET /sentinel` masih `alpha`; di folder data lain atau sebelum PUT pertama → 404 → AC-7, AC-8
- [x] `curl -i -X PUT --data '' http://127.0.0.1:<port>/sentinel` dan PUT body 257 byte → 400, isi sentinel tidak berubah → AC-8

### Commands

- [x] `cargo test -p stackly-fake-server` → 18 unit test dan 36 test integrasi `dummy_server` lulus, termasuk `sentinel_is_written_and_read_back_from_the_data_folder`, `sentinel_accepts_one_to_two_hundred_fifty_six_bytes` (1 dan 256 byte diterima; kosong, 257 byte, dan bukan UTF 8 ditolak 400 tanpa mengubah isi lama), `sentinel_routes_answer_only_their_methods`, `two_servers_keep_their_own_sentinels`, dan `sentinel_survives_a_restart_on_the_same_data_folder`; tidak ada file sementara tersisa di folder data → AC-7, AC-8
- [x] `cargo test -p stackly-core --test instance_duplicate` → 22 test lulus. `draft::*` membuktikan tabel `build_duplicate` dengan paket fake, PostgreSQL, dan NATS: settings disaring (secret, managed `serverName`, key asing keluar; `immutableAfterInit` dan nilai default tetap), tanpa config aktif settings `{}`, setiap endpoint IP `auto` (sumber dibuat dengan port `Fixed` 5432, endpoint `unix` PostgreSQL tidak ikut), setiap secret `generate` dengan `secret_sources` dari pemakaian, runtime ID dan provider revision sumber, `PackageSource::Pinned`, `displayName` kosong dan `start` false, nama form dan draft sama, instance retained tidak menahan nama, semua kandidat terpakai memberi nama kosong → AC-2, AC-3, AC-4, AC-5
- [x] Test yang sama, `gather::*` membuktikan `gather_duplicate` di `FakePlatform` dan `FakeRegistry`: instance tidak ada, retained (dicek sebelum paket dibaca), paket hilang, tidak valid, dan digest beda dengan tiga teks `reason` `load_package`, catalog dengan label revision sama dan digest lain, catalog yang sama atau `NotConfigured` yang tetap berhasil, tanpa `multi_instance`, runtime bukan `installed`, `values` yang bukan JSON object menjadi `Corrupt` `config_revisions.values`, urutan pemeriksaan, sumber tanpa folder instance, log, dan run yang berhasil dengan generation registry, mutasi file system, panggilan Keychain, dan spawn proses tidak berubah → AC-1, AC-7 (bagian core)
- [x] `cargo test -p stackly-core instance::duplicate` → 7 unit test `duplicate_name` lulus: `pg17-dev`, `pg17-dev-copy`, `pg17-dev-copy-7`, sumber 63 karakter dipotong menjadi 58 lalu `-copy`, tanda hubung di ujung potongan dibuang, akhiran yang bukan angka 2 sampai 99 tetap bagian dasar, dan semua kandidat terpakai → AC-3
- [x] `cargo test -p stackly-core --test create_evaluate` → test form spec 0020 lulus tanpa perubahan setelah `build_form` memanggil `build_form_from_package` → AC-1 (bentuk form dipakai ulang)
- [x] `cargo clippy -p stackly-core -p stackly-fake-server --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-core` → semuanya bersih

### Acceptance-criteria coverage

- AC-1 (urutan pemeriksaan dan tanpa jejak) · `gather::*`. Dua bagian menunggu milestone 2: kode error di wire dan batas 300 ms pada persentil 95 dengan 10 instance (daemon). Cabang `RuntimeNotFound` tidak bisa dicapai lewat `FakeRegistry` karena registry menolak instance yang runtime-nya tidak ada (`MissingReference`); cabang itu dijaga oleh tipe dan baru bisa diuji bila registry rusak, yang tidak ada alatnya di testkit.
- AC-2, AC-3, AC-4 (draft) · `draft::*` dan unit test `duplicate_name`. Port efektif setelah start dan penolakan `supervisor.endpoint_conflict` untuk `Fixed` yang sama menunggu milestone 2.
- AC-5 (kebijakan secret) · draft selalu `generate` dan `secret_sources` terisi di sini; reference baru, `copyFrom`, dan nilai berbeda menunggu milestone 2 (daemon).
- AC-6 menunggu milestone 2; AC-7 · rute `/sentinel` (alat) dan `gather_duplicate` tanpa sentuhan folder sumber di sini, sisanya (mode 000, daftar pohon, 404 di duplicate) menunggu milestone 2; AC-8 · rute `/sentinel` (alat) di sini, skenario tiga instance menunggu milestone 2.
- AC-9, AC-10, AC-11, AC-12, AC-13, AC-14 menunggu milestone 2 sampai 4.
