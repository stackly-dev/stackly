# Verify: Start, readiness, dan connection · spec 0021 · updated 2026-10-22

_Langkah diturunkan dari acceptance criteria spec 0021. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah; bagian Milestone 2 sampai 4 ditambahkan oleh milestone masing masing._

## Milestone 1: Connection dan ukuran data di core

### UI / manual

- [x] Buka `providers/postgresql/provider.yaml`, `providers/nats/provider.yaml`, dan `providers/fake/provider.yaml` → masing masing berakhir dengan blok `connection`: PostgreSQL `uri` dan `socket`, NATS `uri` dan `monitoring`, fake `uri` (`http://stackly:${secret.authToken}@${endpoint.http.address}/`) → AC-1
- [x] Buka `providers/catalog.toml` dan `providers/catalog-revisions.json` → revision 5 dengan `published_at` yang lebih baru dari revision 4 dan hash isi baru → AC-1
- [x] Buka `protocols/fixtures/instance/connection-compose.json` → kasus `compose` untuk password dengan spasi, `@`, `:`, `/`, `%`, `?` dan `#`, tanda kutip dan backslash, emoji, huruf beraksen, hanya karakter unreserved, nilai kosong, dua secret dalam satu entri, teks yang tidak di encode, nilai yang tidak dipakai, dan dua kasus `missingValue`; empat kasus `render` (folder socket berspasi dan Unicode, path socket, IPv4, IPv6 dalam kurung siku) → AC-3

### Commands

- [x] `cargo xtask provider check` → empat baris `ok` (fake, nats, postgresql, traefik) tanpa masalah → AC-1
- [x] `cargo xtask catalog check` → `ok catalog revision 5 ...` → AC-1
- [x] `cargo test -p stackly-core --test provider_connection` → semua kasus penolakan AC-1 lulus (nama, judul, template kosong atau di atas 512 byte, nama ganda, token di luar posisi, endpoint dan secret tidak ada, bukan URI dengan host IPv4 dan IPv6, blok kosong atau lebih dari 8 entri); paket `providers/fake` dipotong dari blok miliknya supaya setiap test menambah blok sendiri → AC-1
- [x] `cargo test -p stackly-core --test provider_packages the_repo_packages_declare_their_connection_entries_in_manifest_order` → PostgreSQL, NATS, dan fake memuat blok mereka (render sentinel lulus) dan traefik tidak punya → AC-1
- [x] `cargo test -p stackly-core --test connection_render` → 13 test: empat kasus `render` fixture, string tersamar dan bagian berurutan, urutan manifest dan entri tanpa secret, port `Fixed` dan `Auto` (dengan dan tanpa `bound`, `running` dan `stopped`), socket yang dinamai dari port Auto, paket lain dari yang di pin, secret tanpa pemakaian, urutan alasan `retained` lalu `package_unavailable` lalu `port_not_assigned`, paket tanpa blok, dan golden `connection.json` untuk PostgreSQL, NATS, dan fake → AC-2, AC-3
- [x] `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test connection_render` lalu `git status --short` → tidak ada file golden yang berubah → AC-2
- [x] `cargo test -p stackly-protocol --lib instance::connection` → setiap kasus `compose` di fixture menghasilkan string atau error `MissingValue` yang tertulis; bagian tanpa teks atau bertipe asing menjadi `InvalidPart`; kind asing terbaca `Unrecognized` → AC-3
- [x] `cargo test -p stackly-protocol --test storage_fixtures the_instance_fixture_index_lists_every_file_in_the_folder` → `connection-compose.json` terdaftar di `protocols/fixtures/instance/index.json` → AC-3
- [x] `cargo xtask codegen --check` → "20 generated files are up to date"; `compose` dan tipe bagiannya belum punya turunan kawat, jadi digest kontrak dan `ProtocolVersion::CURRENT` tidak berubah → AC-3
- [x] `cargo test -p stackly-platform tree_usage` → root yang relatif, bertitik, ber NUL, atau berakhir garis miring ditolak `InvalidInput` dengan op `tree_usage` → AC-5
- [x] `cargo test -p stackly-testkit --test fake_fs fake_filesystem_passes_the_conformance_suite` → `fs_conformance` menjalankan pemeriksaan `tree_usage` di `FakeFilesystem`: ukuran sama dengan hitungan biasa, symlink keluar root tidak diikuti, hard link sekali, tepat `max_entries` lengkap dan satu lebih tidak, root kosong dengan batas 0, 32 tingkat lengkap dan 33 tidak, tenggat nol, dan error root → AC-5
- [x] `cargo test -p stackly-testkit --test fake_fs tree_usage` → `tree_usage_calls()` menghitung panggilan (juga yang gagal), `tree_usage_peak()` mencapai 3 untuk tiga panggilan bersamaan, jeda `set_delay` terjadi sebelum tenggat mulai, dan root di luar root test menjadi pelanggaran → AC-5
- [x] `cargo test -p stackly-platform-macos --test layout macos_filesystem_passes_the_conformance_suite` → pemeriksaan `tree_usage` yang sama lulus di `MacosFilesystem` → AC-5
- [x] `cargo test -p stackly-platform-macos --test tree_usage` → 3 test: 50 000 file dengan tenggat 1 ms selesai `complete: false` dalam 100 ms dan dengan 60 detik `complete: true`; batas entri tepat dan satu kurang pada pohon yang sama; image disk sementara di dalam pohon dilewati (byte sama dengan pohon tanpa image) → AC-5
- [x] `cargo test -p stackly-platform --lib unsupported` → `UnsupportedPlatform::tree_usage` menjawab `NotSupported` → AC-5
- [x] `cargo test -p stackly-core --test instance_detail` → 9 test: setiap field detail dari sumbernya di tabel *Value sourcing* (provider, nama tampil dari paket pinned, runtime dan target, path data dari baris registry lewat `RootSet::resolve`, startup, secret urut field dengan judul dari schema dan status `unknown` tanpa cache, dependents urut nama lalu ID dengan `leaseActive` hanya untuk lease aktif environment itu, connections); paket tidak terbaca atau bukan yang di pin menghasilkan nama tampil kosong dan connections kosong; runtime eksternal tanpa digest; baris `data` yang hilang menjadi `DetailError::MissingDataPath` → AC-4
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, dan `cargo xtask check-deps` → lulus; `unsafe` hanya di `stackly-platform-macos` (`tree_usage.rs` dengan `// SAFETY:` per blok) → AC-5
- [x] `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → exit 0 → AC-5

### Acceptance-criteria coverage

- AC-1 (blok `connection` di manifest) · seluruhnya di milestone ini: langkah 1 (sudah di `main` lewat `15a5641`) untuk loader, kode issue, dan `Issue.reason`; langkah 2 untuk blok PostgreSQL dan fake dan revision catalog 5; blok NATS dari langkah 2 spec 0023.
- AC-2 (render connection) · seluruhnya di milestone ini lewat `core::instance::connection::render`. Satu catatan: paket pinned yang tidak terbaca tidak punya entri untuk dibuat, jadi `package_unavailable` dipakai hanya untuk paket yang terbaca tetapi bukan yang di pin atau tidak cocok dengan konteks; `detail::build` memberi daftar connection kosong untuk paket yang tidak terbaca.
- AC-3 (komposisi) · bagian Rust dan bagian `render` core di milestone ini; `ConnectionComposer` Swift menunggu langkah 9 (milestone 3). Nilai `instance.name` di fixture adalah slug (`orders-db`) karena nama instance tidak boleh berspasi atau ber Unicode; yang berspasi dan Unicode adalah folder socket.
- AC-4 (`instance.get_detail`) · `core::instance::detail::build` di milestone ini; handler, cek stat paket, dan `resource.not_found` menunggu langkah 7 (milestone 2).
- AC-5 (`instance.get_data_usage`) · `Filesystem::tree_usage` dan aturan penelusurannya di milestone ini; cache, penggabungan, semaphore, pemetaan error, dan batas `DaemonConfig` menunggu langkah 7 (milestone 2).
- AC-6 sampai AC-20 · menunggu milestone 2 sampai 4.

## Milestone 2: Kontrak, daemon, CLI, dan bukti offline

### UI / manual

- [x] Buka `protocols/fixtures/instance/detail-postgresql.json`, `detail-nats.json`, `detail-unavailable.json`, dan `detail-package-unreadable.json` → masing masing berisi `{ instance, detail, dataUsage }` (bentuk yang sama dengan `result` `inspect --json`): PostgreSQL berjalan dengan dua connection tersedia, dua dependents, dan ukuran lengkap; NATS berhenti dengan runtime eksternal dan dua connection `port_not_assigned`; instance retained dengan dua connection `retained` dan ukuran tidak lengkap; PostgreSQL berhenti dengan `packageReadable` false, `displayName` null, `connections` kosong, secret `superuserPassword` berjudul nama field, dan `dataUsage` null → AC-7, AC-17
- [x] Buka `protocols/fixtures/ipc/` → `request-instance-get-detail.json`, dua request `get-data-usage`, `reject-instance-get-data-usage-refresh-and-cached-only.json`, lima frame `response` (detail, detail paket tidak terbaca, detail dengan nilai asing, data usage lengkap, tidak lengkap, dan kosong), dan tiga frame error `instance.data_usage_unavailable` (`missing`, `not-private`, `io`, masing masing dengan hint statis) → AC-17
- [x] `stackly instance inspect <instance>` terhadap fixture `detail-package-unreadable.json` (lihat `apps/cli/tests/golden/instance_inspect_package_unreadable.txt`) → baris `Connection  unavailable (package_unavailable)` sesudah tabel endpoint, tanpa tabel `CONNECTION` → AC-7
- [x] Buka `apps/cli/tests/golden/instance_inspect_postgresql.txt` → baris `Digest` (sha256 lengkap), `Data`, `Data size`, `Autostart` sesudah `Version`; `Dependents` dan `Secrets` sesudah `Logs`; lalu tabel `CONNECTION`/`VALUE` dengan string tersamar → AC-7

### Commands

- [x] `cargo xtask codegen --check` → "20 generated files are up to date"; `ProtocolVersion::CURRENT` 1.14 dan digest-nya tercatat di `protocols/contract-versions.json` → AC-17
- [x] `cargo test -p stackly-protocol` → termasuk 11 test `detail_fixtures`: triple round trip dalam bentuk kawat, satu instance pada satu revision, `ConnectionString` konsisten (tersedia jika dan hanya jika `masked` terisi, `parts` tidak kosong, `reason` kosong), `packageReadable` false hanya di triple paket tidak terbaca dengan `displayName` null dan `connections` kosong, string tersamar sama dengan hasil `compose` dengan `****`, tidak ada nilai secret, request dan penolakan `refresh` bersama `cachedOnly` (`cachedOnly` dengan `invalid_value`), nilai asing setiap enum terbuka menjadi `Unrecognized`, dan hint statis tiga reason `instance.data_usage_unavailable` → AC-17
- [x] `swift test --package-path clients/swift` → `DetailContractTests` (konsistensi `ConnectionString`, triple, frame, nilai asing menjadi `.unrecognized`) dan `IpcFixtureTests` (35 method) lulus. Catatan: enum kontrak Swift bernama `ConnectionAvailability`, bukan `ConnectionState`, karena `ConnectionState` sudah status link daemon di `StacklyClient` → AC-17
- [x] `swift build --package-path apps/macos/Packages/StacklyUI` → build selesai tanpa perubahan di pemanggil StacklyUI → AC-17
- [x] `cargo test -p stackly-core --test instance_detail` → 10 test, termasuk `packageReadable` true untuk paket terbaca tanpa blok `connection`, false untuk paket hilang dan paket yang bukan yang di pin → AC-4
- [x] `cargo test -p stacklyd --lib detail` → cache data usage lewat `FakeClock` (segar sampai 60 detik, tidak lengkap sampai 10 detik, `cachedOnly` tanpa scan, satu scan untuk permintaan bersamaan termasuk `refresh`, paling banyak dua scan bersamaan, scan tetap selesai saat klien pergi, folder hilang atau tidak privat gagal sebelum scan, folder yang diganti file antara cek dan scan, error tidak pernah di cache) dan pemetaan error `tree_usage` → AC-5
- [x] `cargo test -p stacklyd --test supervisor detail::` → 8 test lewat daemon asli: detail instance fake dari registry dan paket pinned tanpa membaca secret store, `resource.not_found`, paket yang dirusak lalu diperbaiki (ukuran sama) dan dihapus, paket dengan digest lain, port `auto` (`port_not_assigned` lalu tersedia), `running` tanpa probe tidak pernah ready (AC-012), tidak ada nilai secret di detail, 10 instance dalam batas 100 ms p95 hangat dan 300 ms dingin → AC-4, AC-8, AC-18, AC-19
- [x] `cargo test -p stacklyd --test supervisor data_usage::` → 10 test lewat daemon asli, termasuk 250 000 entri dengan konstanta asli `complete: false` dalam 2,1 detik → AC-5, AC-19
- [x] `cargo test -p stacklyd --test supervisor offline::` → server di dalam proses tanpa catalog, `FakeFetcher` offline, dan `FakeCodeSigning`: `instance.start` sampai ready, `instance.get`, `instance.get_detail`, `instance.get_data_usage`, `stackly instance connection`, dan `stackly instance inspect` berhasil dengan nol akses `CatalogState`, nol permintaan fetch, dan nol `assess`; tidak ada awalan token di log, detail, dan output CLI; `--show-password` menulis string dengan token asli dan satu baris audit `secret.reveal` → AC-6, AC-10, AC-18
- [x] `cargo test -p stackly-cli` → termasuk 18 test `connection` terhadap daemon tiruan (default tersamar tanpa `secret.reveal`, `--format`, `cli.connection_not_found` dan `cli.connection_unavailable` dengan golden, `--all`, konflik clap, `--show-password` dengan satu reveal `show` per reference dan string ter-encode, reveal yang gagal, catatan `portMayChange` di stderr, paket tidak terbaca di setiap bentuk, golden `inspect` dari empat triple, `--size`, data usage yang gagal, ambil ulang saat revision beda) → AC-6, AC-7, AC-18
- [x] `cargo xtask check-deps` → lulus; `commands::instance::connection` masuk `EXPOSE_ALLOWED` untuk `stackly-cli` → AC-6
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → lulus

### Acceptance-criteria coverage

- AC-4 (`instance.get_detail`) · seluruhnya di milestone ini. Cek ulang paket per request membandingkan isi `provider.yaml` (bukan inode, ukuran, dan mtime): `FileInfo` tidak membawa mtime, dan perbandingan isi lebih kuat; perbedaan memicu baca ulang dengan cek digest terhadap kunci pin.
- AC-5 (`instance.get_data_usage`) · seluruhnya di milestone ini (urutan jawaban, cache lewat `Clock`, penggabungan, semaphore, pemetaan error, batas di `DaemonConfig.data_usage`). Folder di volume lain diuji di tingkat platform (milestone 1, langkah 4).
- AC-6 (`stackly instance connection`) · seluruhnya di milestone ini.
- AC-7 (paritas inspect dan Summary) · sisi CLI di milestone ini (golden dari empat triple); sisi Swift menunggu langkah 9 (milestone 3).
- AC-8 (readiness terpisah) · bagian daemon dan CLI di milestone ini; header, kartu Endpoints, dan view model menunggu milestone 3.
- AC-10 (offline) · test di dalam proses di milestone ini; run native dengan jaringan mati menunggu milestone 4 dan persetujuan user.
- AC-17 (kontrak) · seluruhnya di milestone ini (Rust dan Swift).
- AC-18 (keamanan) · bagian daemon dan CLI di milestone ini; model Swift menunggu milestone 3.
- AC-19 (kinerja) · `get_detail` dan `get_data_usage` terukur di milestone ini dengan 10 instance fake di Mac ini; batas dijaga hanya saat Mac tidak dibebani.
- Lainnya (AC-9, AC-11 sampai AC-16, AC-20) · menunggu milestone 3 dan 4.

### Catatan verify milestone 2 · 2026-10-22 · `chk-f21-m2`

Semua langkah di atas dijalankan ulang dari awal pada `fe3c5fd4` (main yang memuat `7cd4d1a9`) dan lulus. Mac dalam beban tinggi (load average 35 sampai 55), jadi catatan ini penting untuk AC-19.

- Hitungan test: `instance_detail` 10 test (bukan 11, sudah dibetulkan di langkahnya); `supervisor` `detail::` 8, `data_usage::` 10, `offline::` 2; CLI `connection` 18; `detail_fixtures` 11.
- AC-19: `ten_instances_are_detailed_within_the_time_bounds` dan `a_tree_of_250_000_entries_ends_incomplete_within_the_deadline` lulus saat load 35 sampai 55, jadi batas 100 ms p95 dan 300 ms dingin terjaga bahkan di bawah beban.
- `swift test --package-path clients/swift`: run pertama, saat load 54, gagal satu test yang tidak terkait, `DaemonConnectionTests.callsAboveTheInFlightLimitWaitForASlot` (`ipc.connection_closed`). Diulang sendiri, lulus; run penuh kedua lulus (205 test). Dicatat sebagai flake beban, bukan temuan milestone ini.
- Probe langsung terhadap `stacklyd` dan `stackly` nyata (root sementara dari `skeleton_seed`, instance fake berjalan): `instance connection` default menulis `http://stackly:****@127.0.0.1:<port>/` tanpa stderr; `--all` satu baris tersamar; `--show-password` menulis token asli (32 karakter); `--format nope` keluar 10 `cli.connection_not_found`; `--all --show-password` ditolak clap (exit 2); sebelum start `connection` keluar 3 `cli.connection_unavailable` dengan hint. Awalan token tidak ada di log daemon (level debug), stdout dan stderr default, `inspect` (teks dan `--json`), maupun argv proses mana pun; log daemon memuat tepat satu `secret.reveal` (dari `--show-password`) tanpa nilainya.
