# Verify: Start, readiness, dan connection · spec 0021 · updated 2026-10-22

_Langkah diturunkan dari acceptance criteria spec 0021. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah; bagian Milestone 2 sampai 4 ditambahkan oleh milestone masing masing._

## Milestone 1: Connection dan ukuran data di core

### UI / manual

- [ ] Buka `providers/postgresql/provider.yaml`, `providers/nats/provider.yaml`, dan `providers/fake/provider.yaml` → masing masing berakhir dengan blok `connection`: PostgreSQL `uri` dan `socket`, NATS `uri` dan `monitoring`, fake `uri` (`http://stackly:${secret.authToken}@${endpoint.http.address}/`) → AC-1
- [ ] Buka `providers/catalog.toml` dan `providers/catalog-revisions.json` → revision 5 dengan `published_at` yang lebih baru dari revision 4 dan hash isi baru → AC-1
- [ ] Buka `protocols/fixtures/instance/connection-compose.json` → kasus `compose` untuk password dengan spasi, `@`, `:`, `/`, `%`, `?` dan `#`, tanda kutip dan backslash, emoji, huruf beraksen, hanya karakter unreserved, nilai kosong, dua secret dalam satu entri, teks yang tidak di encode, nilai yang tidak dipakai, dan dua kasus `missingValue`; empat kasus `render` (folder socket berspasi dan Unicode, path socket, IPv4, IPv6 dalam kurung siku) → AC-3

### Commands

- [ ] `cargo xtask provider check` → empat baris `ok` (fake, nats, postgresql, traefik) tanpa masalah → AC-1
- [ ] `cargo xtask catalog check` → `ok catalog revision 5 ...` → AC-1
- [ ] `cargo test -p stackly-core --test provider_connection` → semua kasus penolakan AC-1 lulus (nama, judul, template kosong atau di atas 512 byte, nama ganda, token di luar posisi, endpoint dan secret tidak ada, bukan URI dengan host IPv4 dan IPv6, blok kosong atau lebih dari 8 entri); paket `providers/fake` dipotong dari blok miliknya supaya setiap test menambah blok sendiri → AC-1
- [ ] `cargo test -p stackly-core --test provider_packages the_repo_packages_declare_their_connection_entries_in_manifest_order` → PostgreSQL, NATS, dan fake memuat blok mereka (render sentinel lulus) dan traefik tidak punya → AC-1
- [ ] `cargo test -p stackly-core --test connection_render` → 13 test: empat kasus `render` fixture, string tersamar dan bagian berurutan, urutan manifest dan entri tanpa secret, port `Fixed` dan `Auto` (dengan dan tanpa `bound`, `running` dan `stopped`), socket yang dinamai dari port Auto, paket lain dari yang di pin, secret tanpa pemakaian, urutan alasan `retained` lalu `package_unavailable` lalu `port_not_assigned`, paket tanpa blok, dan golden `connection.json` untuk PostgreSQL, NATS, dan fake → AC-2, AC-3
- [ ] `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test connection_render` lalu `git status --short` → tidak ada file golden yang berubah → AC-2
- [ ] `cargo test -p stackly-protocol --lib instance::connection` → setiap kasus `compose` di fixture menghasilkan string atau error `MissingValue` yang tertulis; bagian tanpa teks atau bertipe asing menjadi `InvalidPart`; kind asing terbaca `Unrecognized` → AC-3
- [ ] `cargo test -p stackly-protocol --test storage_fixtures the_instance_fixture_index_lists_every_file_in_the_folder` → `connection-compose.json` terdaftar di `protocols/fixtures/instance/index.json` → AC-3
- [ ] `cargo xtask codegen --check` → "20 generated files are up to date"; `compose` dan tipe bagiannya belum punya turunan kawat, jadi digest kontrak dan `ProtocolVersion::CURRENT` tidak berubah → AC-3
- [ ] `cargo test -p stackly-platform tree_usage` → root yang relatif, bertitik, ber NUL, atau berakhir garis miring ditolak `InvalidInput` dengan op `tree_usage` → AC-5
- [ ] `cargo test -p stackly-testkit --test fake_fs fake_filesystem_passes_the_conformance_suite` → `fs_conformance` menjalankan pemeriksaan `tree_usage` di `FakeFilesystem`: ukuran sama dengan hitungan biasa, symlink keluar root tidak diikuti, hard link sekali, tepat `max_entries` lengkap dan satu lebih tidak, root kosong dengan batas 0, 32 tingkat lengkap dan 33 tidak, tenggat nol, dan error root → AC-5
- [ ] `cargo test -p stackly-testkit --test fake_fs tree_usage` → `tree_usage_calls()` menghitung panggilan (juga yang gagal), `tree_usage_peak()` mencapai 3 untuk tiga panggilan bersamaan, jeda `set_delay` terjadi sebelum tenggat mulai, dan root di luar root test menjadi pelanggaran → AC-5
- [ ] `cargo test -p stackly-platform-macos --test layout macos_filesystem_passes_the_conformance_suite` → pemeriksaan `tree_usage` yang sama lulus di `MacosFilesystem` → AC-5
- [ ] `cargo test -p stackly-platform-macos --test tree_usage` → 3 test: 50 000 file dengan tenggat 1 ms selesai `complete: false` dalam 100 ms dan dengan 60 detik `complete: true`; batas entri tepat dan satu kurang pada pohon yang sama; image disk sementara di dalam pohon dilewati (byte sama dengan pohon tanpa image) → AC-5
- [ ] `cargo test -p stackly-platform --lib unsupported` → `UnsupportedPlatform::tree_usage` menjawab `NotSupported` → AC-5
- [ ] `cargo test -p stackly-core --test instance_detail` → 9 test: setiap field detail dari sumbernya di tabel *Value sourcing* (provider, nama tampil dari paket pinned, runtime dan target, path data dari baris registry lewat `RootSet::resolve`, startup, secret urut field dengan judul dari schema dan status `unknown` tanpa cache, dependents urut nama lalu ID dengan `leaseActive` hanya untuk lease aktif environment itu, connections); paket tidak terbaca atau bukan yang di pin menghasilkan nama tampil kosong dan connections kosong; runtime eksternal tanpa digest; baris `data` yang hilang menjadi `DetailError::MissingDataPath` → AC-4
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, dan `cargo xtask check-deps` → lulus; `unsafe` hanya di `stackly-platform-macos` (`tree_usage.rs` dengan `// SAFETY:` per blok) → AC-5
- [ ] `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → exit 0 → AC-5

### Acceptance-criteria coverage

- AC-1 (blok `connection` di manifest) · seluruhnya di milestone ini: langkah 1 (sudah di `main` lewat `15a5641`) untuk loader, kode issue, dan `Issue.reason`; langkah 2 untuk blok PostgreSQL dan fake dan revision catalog 5; blok NATS dari langkah 2 spec 0023.
- AC-2 (render connection) · seluruhnya di milestone ini lewat `core::instance::connection::render`. Satu catatan: paket pinned yang tidak terbaca tidak punya entri untuk dibuat, jadi `package_unavailable` dipakai hanya untuk paket yang terbaca tetapi bukan yang di pin atau tidak cocok dengan konteks; `detail::build` memberi daftar connection kosong untuk paket yang tidak terbaca.
- AC-3 (komposisi) · bagian Rust dan bagian `render` core di milestone ini; `ConnectionComposer` Swift menunggu langkah 9 (milestone 3). Nilai `instance.name` di fixture adalah slug (`orders-db`) karena nama instance tidak boleh berspasi atau ber Unicode; yang berspasi dan Unicode adalah folder socket.
- AC-4 (`instance.get_detail`) · `core::instance::detail::build` di milestone ini; handler, cek stat paket, dan `resource.not_found` menunggu langkah 7 (milestone 2).
- AC-5 (`instance.get_data_usage`) · `Filesystem::tree_usage` dan aturan penelusurannya di milestone ini; cache, penggabungan, semaphore, pemetaan error, dan batas `DaemonConfig` menunggu langkah 7 (milestone 2).
- AC-6 sampai AC-20 · menunggu milestone 2 sampai 4.
