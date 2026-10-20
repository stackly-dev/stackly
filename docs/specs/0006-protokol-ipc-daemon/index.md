# 0006. Protokol IPC dan daemon

**Date**: 2026-10-18
**Status**: In Progress

## Summary

Spec ini mengunci cara daemon `stacklyd` dan client nya (CLI dan GUI) saling bicara: bentuk pesan, handshake berversi (protokol mulai 1.0), satu envelope error dengan sepuluh kategori stabil dan kode bernama `area.alasan`, event dengan sequence dan cursor, batas ukuran dan antrian, serta aturan codegen ke Swift. Di sisinya ada daemon yang benar benar hidup: satu per user lewat lock file, socket Unix privat bermode 0600 tanpa listener TCP, pemeriksaan identitas peer di kedua arah, log file yang dirotasi, dan perintah `stackly daemon status`. Kontrak ini ditulis sekali di `stackly-protocol` dan menjadi pijakan spec 0005, 0007 sampai 0011, dan 0013; fitur lain hanya menambah method, event, dan kode error di area mereka sendiri tanpa mengubah aturannya. Fuzz target framing (pengujian dengan input acak yang dipandu coverage) jalan di Mac ini lewat cargo-fuzz dengan nightly yang dipin khusus untuk folder `fuzz/`.

## Requirements

**User stories**:

- Sebagai pengguna CLI, saya ingin `stackly daemon status` menunjukkan daemon milik saya beserta versi, root aktif, dan kesehatannya, atau menjelaskan kenapa daemon tidak terjangkau, supaya saya tahu langkah berikutnya.
- Sebagai pengguna Mac dengan dua akun, saya ingin daemon saya menolak proses dari akun lain dan client saya tidak pernah mengirim apa pun ke socket palsu, supaya data dan credential saya tetap di akun saya.
- Sebagai developer GUI (fitur 13), saya ingin reconnect cukup dengan snapshot plus cursor, dan tipe Swift hasil generate yang tahan terhadap daemon dengan minor lebih baru, supaya update daemon tidak membuat app gagal decode atau menampilkan state palsu.
- Sebagai penulis spec fitur lain, saya ingin satu aturan untuk menambah method, event, dan kode error, supaya spec yang ditulis paralel tidak memutuskan hal yang sama dengan dua cara.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-047):

- **AC-1**: Frame pertama setiap koneksi wajib request `daemon.handshake`, dan frame itu harus lengkap diterima dalam 5 detik setelah accept. Kalau frame pertama punya `call.type` `daemon.handshake` dan `call.content.protocol.major` terbaca tetapi berbeda dengan major daemon, daemon mengirim `protocol.major_mismatch` (kategori `unsupported`, `details` berisi versi daemon) walaupun field lain tidak valid, lalu menutup koneksi; request yang sudah dikirim di belakangnya tidak pernah diproses. Major yang sama (minor berapa pun) dengan params valid mendapat `HandshakeAccepted` berisi versi daemon, `serverInstanceId`, digest kontrak, `contractDrift`, batas, dan cursor `head`. Kegagalan lain sebelum handshake diterima juga mengirim error lalu menutup: request yang valid tetapi method nya bukan handshake (termasuk method yang tidak dikenal) mendapat `protocol.handshake_required`, frame yang tidak lengkap dalam 5 detik mendapat `ipc.handshake_timeout`, dan JSON rusak, terlalu dalam, atau params handshake yang salah mendapat kode `request.*` nya masing masing (REQ-047, AC-047, AC-071).
- **AC-2**: Daemon membaca uid peer dari socket segera setelah accept, sebelum membaca satu frame pun. Uid yang berbeda dengan uid daemon, atau kredensial peer yang tidak bisa dibaca, menerima satu frame error `ipc.peer_rejected` (kategori `unauthorized`) lalu ditutup tanpa satu request pun diproses, dan log mencatat uid serta pid peer. Integration test membuktikannya lewat uid harapan yang disuntikkan ke listener `stackly-ipc`, dan kasus kredensial tak terbaca lewat seam `PeerCredSource`; pemeriksaan dengan dua akun nyata di Mac ini menjadi evidence manual (REQ-047, AC-047, AC-079, BR-012).
- **AC-3**: Sebelum connect, client memeriksa path socket tanpa mengikuti symlink (`verify_path`): harus berjenis socket, milik uid client, tanpa izin apa pun untuk group dan other, dan folder induknya juga milik uid client tanpa izin untuk group dan other. Setelah connect, client membaca uid server lewat kredensial peer (`verify_peer`) dan uid itu harus sama dengan uid client. Kegagalan mana pun menghasilkan `ipc.endpoint_untrusted` (kategori `unauthorized`, `details.reason` dari daftar tetap) tanpa satu byte pun ditulis ke socket. Path yang tidak ada menghasilkan `ipc.endpoint_missing`, connect yang ditolak `ipc.connection_refused`, dan error OS lain saat memeriksa atau connect `ipc.io_error`, ketiganya kategori `unavailable`. Credential dan mutasi hanya dikirim setelah handshake diterima (AC-070, REQ-016, REQ-047).
- **AC-4**: Daemon mengambil lock eksklusif pada `<data>/stacklyd.lock` setelah `prepare_layout` dan sebelum menyentuh file log, socket, registry, atau proses. Daemon kedua untuk root yang sama gagal mengambil lock, mencetak pid, `serverInstanceId`, versi, dan waktu start pemilik (dibaca dari isi lock file) ke stderr, lalu keluar dengan kode 3. Isi lock file dan inode socket daemon pertama tidak berubah, inode file log nya sama, dan tidak ada satu baris pun dari daemon kedua di file log itu. `stackly daemon status` setelahnya menampilkan pid dan `serverInstanceId` daemon pemilik itu (BR-011, RISK-03).
- **AC-5**: Proses `stacklyd` tidak pernah membuka socket TCP atau UDP: terhadap daemon yang sedang melayani client, `/usr/sbin/lsof -nP -a -p <pid> -i` tidak mengembalikan satu baris pun, sedangkan `/usr/sbin/lsof -nP -a -p <pid> -U` (kontrol positif) menampilkan socket Unix daemon. `cargo xtask check-deps` gagal kalau kode non test di `crates/ipc`, `apps/daemon`, atau `apps/cli` menyebut `TcpListener`, `TcpStream`, `TcpSocket`, atau `UdpSocket` (REQ-047, AC-070).
- **AC-6**: Codec membaca panjang `u32` big endian lalu payload. Panjang 0 menghasilkan `ipc.frame_empty`, dan panjang di atas 1 048 576 byte menghasilkan `ipc.frame_too_large` begitu header terbaca, sebelum buffer payload dialokasikan; daemon mengirim frame error itu lalu menutup koneksi. Buffer decoder tidak pernah melebihi `max_len + 4 + READ_CHUNK` byte. Frame yang sudah mulai diterima tapi belum lengkap setelah 30 detik menutup koneksi dengan `ipc.frame_stalled`. Frame yang valid terbaca sama persis untuk setiap cara pemotongan byte. Semua vektor di `protocols/fixtures/framing/vectors.json` lulus di test Rust (VAL-017, NFR-004).
- **AC-7**: Untuk frame yang framing nya valid setelah handshake diterima, daemon menjawab dengan error dan koneksi tetap hidup: JSON rusak, bentuk request yang salah, atau `requestId` dan `idempotencyKey` yang formatnya salah menjadi `request.malformed`; kedalaman JSON lebih dari 32 menjadi `request.too_deep`; method yang tidak dikenal menjadi `request.unknown_method` (kategori `unsupported`, `details.method`); request atau `call` yang bukan objek JSON (misalnya array posisional) juga menjadi `request.malformed`; params yang bukan objek JSON, gagal decode, atau gagal `Validate` menjadi `request.invalid_params` dengan `fields[]` yang `path` nya relatif terhadap params dan `code` nya dari set tetap, tanpa pernah meneruskan teks error serde (yang bisa memuat nilai input); error membawa paling banyak 32 `fields` dengan `path` paling panjang 256 karakter (scalar Unicode) dan menghitung field yang dibuang di `details.omitted`, sehingga frame error selalu muat di batas frame, juga untuk daftar `kinds` terpanjang yang muat di satu frame (349 000 string kosong); `requestId` yang sama dengan request yang masih berjalan di koneksi itu menjadi `request.duplicate_id`; request ke 33 yang berjalan bersamaan menjadi `request.too_many_in_flight`; subscription ke 9 menjadi `events.too_many_subscriptions`. Koneksi ke 65 menerima `ipc.too_many_connections` lalu ditutup (VAL-017, NFR-004).
- **AC-8**: Setiap error yang dikirim daemon, dan setiap error sisi client di `stackly-ipc`, adalah `ErrorBody` yang `code` nya ada di katalog `stackly-protocol`, cocok dengan pola `^[a-z][a-z0-9_]*\.[a-z][a-z0-9_]*$`, memakai area yang terdaftar, membawa `category` dan `retryable` persis seperti di katalog, kunci `details` yang hanya dari daftar kunci kode itu di katalog, `items` yang kosong atau hanya berisi `kind` dan kunci field yang diizinkan kode itu di katalog, paling banyak batas kode itu, `message` aman berbahasa Inggris yang paling panjang 200 karakter (lebih dari itu dipotong), serta konteks `resource`, `fields`, `hint`, `operationId`, dan `rule` bila ada. `retryable` hanya true untuk kode yang menjamin request tidak berefek apa pun. Katalog tidak punya kode ganda dan ditulis ke `protocols/error-codes.json` oleh `cargo xtask codegen`. Frame error membawa `requestId` request yang gagal, atau null hanya kalau `requestId` tidak terbaca. Setiap fixture error terbaca di Rust dan Swift (VAL-020, NFR-011).
- **AC-9**: `sequence` mulai dari 1 di setiap run daemon dan naik tepat 1 per event yang dipublikasikan; setiap event membawa `at`, dan event tentang resource selalu membawa `resource` sekaligus `revision`. `events.subscribe` membalas `Subscribed { subscriptionId, head }` sebelum event apa pun dari subscription itu; `subscriptionId` mulai 1 per koneksi dan tidak dipakai ulang. Dengan `since` dari run yang sama yang masih di dalam ring 4096 event, daemon mengirim ulang semua event setelah `since` yang lolos filter, urut dan tanpa duplikat, lalu lanjut ke event baru tanpa celah. `since` dari `serverInstanceId` lain menghasilkan `events.gap` reason `server_restarted`, dan `since` yang sudah keluar dari ring menghasilkan reason `cursor_expired`, keduanya sebagai event pertama. Subscriber yang tertinggal lebih dari 1024 event menerima `events.gap` reason `overflow`, publish tidak pernah menunggu subscriber, dan `counters.eventsDropped` bertambah sebanyak event yang terbuang. `events.gap` dan `daemon.shutting_down` selalu dikirim walau tidak cocok dengan filter (AC-071, NFR-004).
- **AC-10**: `ProtocolVersion::CURRENT` naik minor untuk setiap tambahan yang aman dan major untuk setiap perubahan yang merusak (lihat *Versi dan kompatibilitas*); `protocols/fixtures/protocol-version.json` selalu sama dengan `CURRENT`, dan digest setiap versi tercatat di `protocols/contract-versions.json`. Fixture yang mewakili daemon dengan minor lebih baru (field yang tidak dikenal, nilai baru di enum terbuka, jenis event baru) terbaca di Rust dan Swift: field asing diabaikan, nilai asing di enum terbuka menjadi `unrecognized`, dan event jenis baru menjadi `SkippedEvent` tanpa menggagalkan stream. Frame daemon atau `content` nya yang bukan objek JSON menjadi `ipc.protocol_violation` di client Rust dan gagal decode di Swift, dan waktu terakhir yang bisa dipegang jiff (`9999-12-30T22:00:00.999Z`) terbaca lalu tertulis ulang sama di keduanya. `cargo xtask codegen --check` lulus untuk semua file hasil generate, termasuk `OpenEnums.swift`, `ContractInfo.swift`, dan digest kontrak, dan gagal kalau digest ternormalisasi untuk versi yang sudah tercatat di `protocols/contract-versions.json` berubah tanpa `CURRENT` dinaikkan, termasuk saat hanya satu batas kontrak yang berubah. Client dan daemon dengan versi sama tetapi digest kontrak berbeda tetap tersambung, dengan `contractDrift: true` dan satu warning di log daemon (AC-071, RISK-09, OQ-005).
- **AC-11**: Daemon start dengan urutan di *Urutan start*. Socket dibuat di nama sementara, diberi mode 0600, lalu di-rename ke `daemon_socket()`, sehingga path akhirnya tidak pernah ada dengan mode lain. Socket lama di path itu hanya diganti kalau berjenis socket; selain itu (file, folder, symlink) daemon keluar 1 dengan `ipc.endpoint_occupied` tanpa menghapus apa pun. Selama langkah start setelah bind, `get_health` melaporkan `starting`, dan method yang butuh subsystem yang belum siap menjawab `daemon.starting`. Kalau socket atau runtime root dihapus saat daemon berjalan, pemeriksaan berikutnya (interval 10 detik) membuat ulang runtime root 0700 dan bind ulang, sehingga client bisa tersambung lagi paling lama 12 detik setelah socket hilang, dan `counters.socketRebinds` naik. SIGTERM atau SIGINT pertama menghentikan accept dan menghapus socket (hanya kalau device dan inode nya masih milik daemon ini), lalu mempublikasikan `daemon.shutting_down`, menjawab request baru dengan `daemon.shutting_down`, menunggu request yang sedang berjalan paling lama 5 detik, mengirim sisa frame keluar paling lama 1 detik, menutup koneksi, melepas lock, lalu keluar 0. Urutan berhenti yang sama dipicu `daemon.shutdown` spec 0015; reason dan exit code mengikuti pemicunya: sinyal memberi reason `signal` dan exit 0, mode `stop` memberi `requested` dan exit 0, dan mode `restart` memberi `restart_requested` dan exit 75.
- **AC-12**: Satu thread penulis log menulis setiap baris JSON ke stderr dan, setelah lock didapat, juga ke `<logs>/daemon/stacklyd.log` (mode 0600); kode daemon tidak pernah menulis log secara langsung. File dirotasi saat mencapai 20 MiB, dan paling banyak 5 file disimpan (`stacklyd.log` sampai `stacklyd.log.4`). Penulisan log tidak pernah menahan daemon: antrian 8192 baris membuang baris saat penuh atau saat tulis gagal, menghitungnya di `counters.logLinesDropped`, dan menandai `log_sink` degraded. Setiap baris yang menutup request membawa `connection_id`, `request_id`, `method`, jenis client, durasi, dan hasilnya (kode error kalau gagal), di level `debug` kalau sukses dan `info` atau lebih tinggi kalau gagal; params, reply, dan isi frame tidak pernah dicatat (NFR-011, NFR-004).
- **AC-13**: `stackly daemon status` membaca `STACKLY_ROOT` sekali, menurunkan path socket dari `DirectoryLayout`, memverifikasi endpoint seperti AC-3, melakukan handshake sebagai client `cli`, lalu menampilkan versi, protokol, status drift, pid, uid, waktu start, `serverInstanceId`, status kesehatan, keempat root, path socket, dan `rootOverride`. `--json` mencetak `{ "info": DaemonInfo, "health": DaemonHealth, "client": { "protocol", "contractDigest", "contractDrift" } }`. Status kesehatan apa pun tetap exit 0. Daemon yang tidak jalan menghasilkan pesan yang menyebut path socket dan exit 6, endpoint yang tidak dipercaya exit 4, dan major berbeda exit 5. CLI tidak pernah menjalankan `stacklyd`, membuat folder, atau menulis file (REQ-028 bagian daemon, BR-011).
- **AC-14**: `cargo xtask fuzz run ipc_frame_decode`, `cargo xtask fuzz run ipc_request_decode`, dan `cargo xtask fuzz run ipc_server_frame_decode` (ditambah di protokol 1.4) masing masing berjalan 5 menit di Mac ini tanpa crash, memakai nightly dan versi cargo-fuzz yang dipin di xtask. Seed dibuat ulang dari fixture oleh `cargo xtask fuzz seed` dan disimpan di `fuzz/seeds/`, dan `cargo test -p stackly-protocol` di toolchain stable memutar ulang setiap input regresi di `crates/protocol/tests/fuzz_regressions/`. Hasil setiap run tercatat sebagai evidence (NFR-009, VAL-017).
- **AC-15**: Di Mac ini, 500 panggilan `daemon.get_info` berurutan lewat satu koneksi ke proses `stacklyd` sungguhan punya p95 paling lama 20 ms, dan nilai p50 serta p95 tercatat di evidence. Ini bagian transport dari NFR-001 dan NFR-006, bukan pengganti benchmark fitur 64 (NFR-001, NFR-006).
- **AC-16**: `stackly-protocol` tetap tanpa IO, tokio, atau libc dan lulus `cargo check --target aarch64-pc-windows-msvc`. Crate baru `stackly-ipc` hanya bergantung pada `stackly-protocol` di antara crate Stackly (dependency dev boleh `stackly-testkit` dan `stackly-evidence`), tanpa `unsafe`, dan aturannya dijaga `cargo xtask check-deps`; tidak ada crate yang bergantung pada library `stacklyd`. `MacosPlatform` melaporkan capability `ipc_endpoint` sebagai supported (NFR-010, NFR-009).
- **AC-17**: `stacklyd --print-roots` membaca `STACKLY_ROOT` seperti biasa, me-resolve dan memvalidasi root, mencatat satu baris berisi keempat root, path socket, dan `rootOverride`, lalu keluar 0 (atau 1 kalau override ditolak) tanpa membuat folder, mengambil lock, atau membuat socket. Test override root yang sudah ada di `apps/daemon/tests/root_override.rs`, termasuk test home directory tanpa override, memakai flag ini sehingga tidak ada test yang menjalankan daemon terhadap folder Application Support asli.
- **AC-18**: Kalau `open_registry` di slot start mengembalikan `Recovery` (spec 0005), daemon tetap hidup dalam recovery mode: subsystem `registry` bernilai `unavailable` dengan detail kode reason, status `degraded`, `DaemonHealth.recovery` berisi laporan recovery, dan setiap method yang tidak ditandai `allowed_in_recovery` di makro method dijawab `registry.recovery_mode` (spec 0005) tanpa membaca atau menulis registry dan tanpa menyentuh proses instance. Di spec ini keenam method (`daemon.handshake`, `daemon.get_info`, `daemon.get_health`, `daemon.get_capabilities`, `events.subscribe`, `events.unsubscribe`) bertanda itu; doctor (fitur 34) menandai dirinya sendiri. Reason `registry_locked` dicoba ulang tiga kali (jeda 1, 2, lalu 4 detik) sebelum daemon masuk recovery mode. Exit code 3 hanya berarti lock dipegang daemon lain. Prune berjalan sekali setelah open `Ready`, lalu setiap `REGISTRY_PRUNE_INTERVAL` (24 jam), dan tidak pernah dalam recovery mode (spec 0005).
- **AC-19**: `ClientInfo` di handshake membawa `kind` (`cli` atau `gui`, enum terbuka) dan `interactive` opsional (tidak ada berarti false). Setiap handler menerima `RequestContext` berisi ID koneksi, `requestId`, peer (uid dan pid dari accept), dan `ClientInfo` dari handshake koneksi itu, dan baris log penutup request mencatat `client_kind` serta `interactive`. Client lama yang tidak mengirim `interactive` tidak pernah memicu dialog (spec 0011).

## Decision

**Chosen option**: pilihan yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md): protokol 1.0 dengan aturan major dan minor yang ketat, envelope error dengan sepuluh kategori dan kode `area.alasan`, cursor event di memori per run daemon dengan penanda gap, transport di crate baru `stackly-ipc` di atas codec murni, dan cargo-fuzz dengan nightly yang dipin khusus untuk `fuzz/`. Semua celah dari cross check ditutup dengan usulan yang disetujui engineer (rationale.md, *Cross check*).

Semua aturan wire hidup di satu crate murni yang dibangkitkan ke Swift, semua IO socket hidup di satu crate tokio kecil tanpa `unsafe`, dan daemon hanya merangkai keduanya dengan lock, log, dan event bus.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang. Contohnya batas antrian yang eksplisit di setiap channel, dan test fixture Swift yang membandingkan nilai hasil decode, bukan teks JSON.

## Feature design

### Peta crate dan modul

Semua aturan arah dependency spec 0001 dan 0004 tetap berlaku. Spec ini menambah:

| Crate | Dependency langsung yang baru | Modul atau isi baru |
| --- | --- | --- |
| `stackly-protocol` | `serde_json` (normal, bukan dev lagi, dengan fitur `raw_value`), `jiff`, `uuid`, `thiserror`, `serde_path_to_error` | `wire` (newtype), `frame` (codec), `json` (decode bertahap, cek kedalaman, `Validate`), `message`, `handshake`, `daemon`, `events`, `error`, `codes` (katalog per area), `limits`, `fuzzing` (harness), `contract_digest` (hasil generate) |
| `stackly-ipc` (baru, `crates/ipc`) | `stackly-protocol`, `tokio` (`net`, `io-util`, `time`, `sync`, `rt`, `macros`), `tracing`, `uuid`, `thiserror`; dev: `stackly-testkit`, `stackly-evidence`, `tokio` dengan `test-util` | `unix::{listener, connection, endpoint, client}` di belakang `cfg(unix)`, `error` |
| `stackly-core` | tidak ada | `ensure_runtime_root` di modul `layout` |
| `stackly-platform-macos` | tidak ada | `ipc_endpoint` menjadi supported |
| `stackly-testkit` | tidak ada | `cargo_bin`, `SpawnedDaemon` |
| `stacklyd` (lib plus bin) | `stackly-ipc`, `tokio` (`rt-multi-thread`, `signal`, `sync`, `time`, `macros`), `uuid`, `libc` (hanya konstanta `O_NOFOLLOW`, tanpa `unsafe`), `serde_json`; `tracing-subscriber` mendapat fitur `env-filter`; dev: `tokio` dengan `test-util` | `lib.rs`: `startup`, `lock`, `log_sink`, `server`, `connection`, `dispatch` (trait `Handlers`), `events`, `state`, `shutdown`, `map`; `main.rs` tipis |
| `stackly-cli` | `stackly-ipc`, `stackly-protocol`, `stackly-platform`, `stackly-platform-macos` (di bawah `cfg(target_os = "macos")`), `tokio` (`rt`, `macros`), `serde_json` | subcommand `daemon status` |
| `xtask` | `syn` (fitur `full`, `visit`) dan `proc-macro2` (fitur `span-locations`) kalau langkah 16 spec 0004 belum menambahkannya | codegen tambahan, aturan `check-deps` baru, subcommand `fuzz` |
| `fuzz/` (baru, di luar workspace) | `libfuzzer-sys`, `stackly-protocol` lewat path | dua fuzz target |

Aturan `cargo xtask check-deps` yang baru:

- `stackly-ipc` hanya boleh bergantung pada `stackly-protocol` di antara crate Stackly untuk dependency normal; dependency dev boleh `stackly-testkit` dan `stackly-evidence`. Tidak boleh memuat `#![allow(unsafe_code)]`, dan wajib `[lints] workspace = true`.
- `stackly-cli` boleh bergantung pada `stackly-ipc`; larangan lama (tanpa `stackly-core` dan `stackly-storage`) tetap.
- Tidak ada crate yang boleh bergantung pada `stacklyd`; library nya hanya dipakai `main.rs` dan test di `apps/daemon/tests/`.
- Pindaian `syn` untuk kode non test di `crates/ipc`, `apps/daemon`, dan `apps/cli`: tidak boleh ada path atau identifier `TcpListener`, `TcpStream`, `TcpSocket`, atau `UdpSocket`. Pesan gagalnya `<crate> <file>:<baris>: no tcp or udp sockets (spec 0006)`.
- `stackly-protocol` tetap tanpa crate Stackly, tokio, atau libc.

Root `Cargo.toml` menambah `crates/ipc` ke `members`, `exclude = ["fuzz"]`, dan `tokio` serta `serde_path_to_error` ke `[workspace.dependencies]`.

### Aturan wire umum

- JSON UTF-8. Nama field camelCase (spec 0001). Nilai enum tanpa data ditulis snake_case (`rename_all = "snake_case"`), sama dengan kode state di spec 0004. Enum berdata memakai `tag = "type"`, `content = "content"` (spec 0001), tanpa pengecualian.
- Nama method, jenis event, dan kode error berbentuk `<area>.<nama>`, keduanya snake_case, misalnya `daemon.get_info`, `instance.state_changed`, `resource.revision_mismatch`.
- Setiap field `Option` memakai `#[serde(default)]` dan ditulis sebagai `null` kalau kosong; setiap field `Vec` memakai `#[serde(default)]`. DTO wire tidak pernah memakai `deny_unknown_fields`.
- Integer `u32` ditulis apa adanya. Nilai `u64` hanya lewat newtype `#[serde(transparent)]` di `wire`: `Revision`, `Sequence`, dan `Count`. Nilainya paling besar `MAX_SAFE_INTEGER` (2^53 − 1); decode menolak angka yang lebih besar. `typeshare.toml` memetakan ketiganya ke `UInt64`. Nilai bertanda (`i64`) hanya lewat newtype `SignedSafeInteger` di `wire` (ditambah spec 0020), yang menolak nilai di luar minus sampai plus `MAX_SAFE_INTEGER` dan dipetakan ke `Int64`; ia memakai ulang `MAX_SAFE_INTEGER` tanpa konstanta baru, jadi `CONTRACT_LIMITS` dan test penamaan nya tidak berubah.
- Waktu memakai newtype `Timestamp` di `wire` (membungkus `jiff::Timestamp`). Ditulis RFC 3339 UTC dengan akhiran `Z` dan tepat tiga digit pecahan detik, misalnya `2026-10-18T03:04:05.123Z`. Dibaca dari RFC 3339 dengan offset dan presisi apa pun, lalu dinormalkan ke UTC dan dipotong ke milidetik. Dipetakan ke `Date` di Swift.
- UUID memakai `uuid::Uuid`, ditulis lowercase, dibaca tanpa peduli huruf besar kecil (spec 0001). Dipetakan ke `UUID` di Swift.
- DTO tidak memakai `BTreeMap` (typeshare 1.13.4 menulisnya sebagai `BTreeMap<..>` yang bukan tipe Swift). Pasangan kunci dan nilai memakai `Vec` berisi struct, misalnya `ErrorDetail { key, value }`.
- Path dikirim sebagai `String` hasil konversi lossy dari path OS, dalam bentuk resolved (bukan canonical); path yang tidak valid UTF-8 tidak dibuat oleh Stackly.

### Frame dan pesan

Satu frame adalah panjang `u32` big endian diikuti payload JSON sepanjang itu. Codec `frame` di `stackly-protocol` murni bekerja pada byte:

- `FrameDecoder::new(max_len)`; `push(&[u8]) -> Result<(), FrameError>` menambah satu potongan ke buffer dan langsung gagal kalau header yang sudah lengkap mengumumkan panjang 0 atau di atas `max_len`; `next_frame() -> Option<Vec<u8>>` mengeluarkan satu payload utuh. Transport membaca dalam potongan paling besar `READ_CHUNK` (65 536 byte) dan mengosongkan semua frame lengkap sebelum `push` berikutnya, jadi buffer tidak pernah melebihi `max_len + 4 + READ_CHUNK` byte. Buffer payload dialokasikan hanya setelah panjangnya lolos cek.
- `encode_frame(payload, max_len) -> Result<Vec<u8>, FrameError>`. `FrameError` bernilai `Empty` atau `TooLarge { len, max }`.

Client hanya mengirim satu jenis frame, `Request`:

```json
{"requestId":"0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e","idempotencyKey":null,"call":{"type":"daemon.get_info"}}
```

Daemon mengirim `ServerFrame`, salah satu dari tiga:

```json
{"type":"response","content":{"requestId":"0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e","reply":{"type":"daemon.get_info","content":{"version":"0.1.0","...":"..."}}}}
{"type":"error","content":{"requestId":"0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e","error":{"code":"request.unknown_method","category":"unsupported","...":"..."}}}
{"type":"event","content":{"subscriptionId":1,"sequence":42,"at":"2026-10-18T03:04:05.123Z","resource":null,"revision":null,"kind":{"type":"daemon.shutting_down","content":{"reason":"signal"}}}}
```

- `Request { requestId: String, idempotencyKey: Option<String>, call: Call }`. `requestId` 1 sampai 64 byte dari `[A-Za-z0-9._:-]`, dipilih client dan unik per koneksi selama request itu berjalan; `stackly-ipc` memakai UUID v7 lowercase. `idempotencyKey` 1 sampai 128 byte ASCII tercetak (0x21 sampai 0x7e), sama dengan `IdempotencyKey` di spec 0004; artinya diputuskan spec 0007.
- `Call` dan `Reply` adalah enum berdata yang varian dan tag nya sama persis, satu per method. Method tanpa params atau tanpa isi balasan adalah varian unit, ditulis tanpa `content`. `Call` dan `Reply` ditulis tangan dengan `#[typeshare]`, karena typeshare tidak melihat item yang dibentuk makro `macro_rules!`. Tabel `methods!` di `message` (daftar `(nama method, varian, tipe params, tipe reply)`) menurunkan `Call::METHODS`, `Call::method()`, `Reply::method()`, dan decoder params per method lewat `match` yang menyeluruh, jadi varian yang tidak ada di tabel tidak bisa dikompilasi, dan satu test memastikan setiap nama di tabel sama dengan tag serde variannya. `EventKind` memakai pola yang sama (`event_kinds!`).
- `Response { requestId, reply: Reply }`, `ErrorFrame { requestId: Option<String>, error: ErrorBody }`, `Event` (lihat *Event dan cursor*).
- Response di satu koneksi boleh datang tidak berurutan; client mencocokkannya lewat `requestId`. `Reply` yang tag nya berbeda dengan `Call` asalnya adalah pelanggaran protokol (`ipc.protocol_violation` di client).
- `ErrorBody` sekitar 200 byte, jadi fungsi Rust yang gagal dengan `ErrorBody` mengembalikan `Box<ErrorBody>` (atau struct kecil yang memegang `Box`), supaya lint clippy `result_large_err` tetap lulus. Bentuk wire tidak berubah.
- **Bagian yang dibekukan lintas major**: amplop `Request` (`requestId`, `idempotencyKey`, `call.type`, `call.content`), field `protocol` di params `daemon.handshake`, amplop `ServerFrame` dengan varian `error`, dan field `code`, `category`, `message`, `retryable`, serta `details` di `ErrorBody` (`retryable` masuk daftar ini di protokol 1.4, karena field itu wajib di Rust dan Swift sejak 1.0). Bagian ini tidak pernah berubah, supaya daemon dan client dari major berapa pun selalu bisa bertukar `protocol.major_mismatch`.

Decode request di daemon lewat `json::decode_request(bytes)` (murni, di `stackly-protocol`):

1. Cek kedalaman JSON tanpa parse penuh (pemindai yang paham string dan escape). Lebih dari `MAX_JSON_DEPTH` menghasilkan `request.too_deep`.
2. Decode kepala `{ requestId?, idempotencyKey?, call: { type: String, content: Option<Box<RawValue>> } }`, dengan urutan key apa pun. Request dan `call` wajib objek JSON (byte pertama setelah whitespace adalah `{`), karena derive serde juga membaca struct dari array posisional. JSON rusak, bentuk salah, atau request dan `call` yang bukan objek menghasilkan `request.malformed`; `requestId` yang terbaca dan formatnya valid dipakai di frame error. `call.type` yang tidak ada di `Call::METHODS` menghasilkan `request.unknown_method`.
3. Decode params dari `content` mentah dengan tipe params method itu lewat `serde_path_to_error`. `content` yang tidak ada atau null menghasilkan satu `FieldError` ber `path` `.` dan kode `required`; `content` yang bukan objek JSON ditolak lebih dulu dengan satu `FieldError` ber `path` `.` dan kode `invalid_value`. Kegagalan menghasilkan `request.invalid_params` dengan satu `FieldError { path, code: "invalid_value", message: "value does not match the expected type or format" }`; `path` dari `serde_path_to_error`, relatif terhadap params. Teks error serde tidak pernah diteruskan ke client maupun log, karena bisa memuat nilai input.
4. `Validate::validate(&params) -> Vec<FieldError>` untuk params yang punya aturan tambahan (misalnya panjang `version` handshake, pola `kinds` subscribe). Daftar kosong berarti lolos; selain itu `request.invalid_params` dengan field yang gagal. `ErrorBody::with_fields` menyimpan paling banyak `FIELD_ERRORS_MAX` (32) field pertama dan menghitung field yang dibuang di `details.omitted`, dan `FieldError::new` memotong `path` di `FIELD_PATH_MAX` (256 scalar Unicode). Setiap kode yang membawa `fields` mendaftarkan kunci `omitted` dan tidak mengizinkan `items`, karena keduanya akan berbagi satu hitungan; `with_fields` memeriksanya dengan `debug_assert` setiap kali satu field ditambah. Hari ini itu `request.invalid_params` dan `request.malformed` (yang paling banyak membawa dua field, jadi kuncinya disiapkan oleh aturan, belum pernah terkirim). Aturan atas daftar memeriksa paling banyak entri sebanyak batas daftar itu, misalnya hanya 32 entri pertama `kinds` ditambah satu `too_many`, jadi `omitted` menghitung field error yang dibuang, batas bawah dari jumlah input yang salah. Pembaca menerima `fields` berapa pun panjangnya. `code` field dari set tetap: `invalid_value`, `required`, `too_short`, `too_long`, `invalid_format`, `out_of_range`, `too_many`, `unknown_value`; `message` adalah teks tetap per kode. Spec lain menambah aturan lewat `Validate` di tipe params mereka, bukan di handler.
5. Validasi format `requestId` dan `idempotencyKey` (gagal menjadi `request.malformed` dengan `fields[].path` `requestId` atau `idempotencyKey`).

`json::handshake_major(bytes) -> Option<u32>` membaca `call.content.protocol.major` secara longgar (tanpa decode params lain) untuk langkah 3 handshake.

Client Rust membaca frame daemon lewat `json::decode_server_frame(bytes) -> Result<DecodedFrame, ...>` dengan cek kedalaman yang sama, frame dan `content` yang wajib objek JSON (selain itu `ipc.protocol_violation`), dan tiga tahap: baca `type` frame; untuk `event` baca `EventProbe` lebih dulu; decode penuh hanya untuk jenis yang dikenal. `DecodedFrame` bernilai `Response(Response)`, `Error(ErrorFrame)`, `Event(Event)`, atau `SkippedEvent { subscriptionId, sequence, kind: String }`.

### Versi dan kompatibilitas

- `ProtocolVersion` mulai di 1.0 lalu naik menurut aturan di bawah; `CURRENT` di `crates/protocol/src/version.rs` adalah satu satunya sumber angkanya, dan fixture `protocols/fixtures/protocol-version.json` selalu sama dengannya.
- **Major naik** untuk setiap perubahan yang merusak wire, termasuk sebelum rilis stabil: menghapus atau mengganti nama field, mengubah tipe, menjadikan field opsional wajib, menambah varian ke enum tertutup yang sudah bisa diterima client lama (`ErrorCategory`, atau enum berdata di dalam reply dari method yang sudah ada), mengubah arti nilai yang ada, mengubah framing, menurunkan batas, atau menambah field yang menyangkut keamanan (peer lama akan mengabaikannya). Bagian yang dibekukan (lihat *Frame dan pesan*) tidak boleh berubah sama sekali.
- **Minor naik** untuk tambahan yang aman: field opsional baru, method baru beserta varian `Reply` nya sendiri (client lama tidak pernah memanggilnya), jenis event baru, nilai baru di enum terbuka, kode error baru, area baru, kunci `details` baru, dan batas yang dinaikkan. Menolak bentuk input yang tidak pernah dikirim peer yang patuh (misalnya array posisional, karena client Rust dan Swift selalu menulis objek) juga minor, begitu juga memberi batas pada output yang dulunya tanpa batas tetapi sudah gagal di atas batas frame (misalnya `fields` di protokol 1.4).
- **Siapa yang menaikkan**: commit yang mengubah digest ternormalisasi (di bawah) wajib menaikkan `ProtocolVersion::CURRENT` di commit yang sama, kecuali versi itu belum pernah ada di main dan engineer memakai `--amend`.
- **Enum terbuka** adalah enum tanpa data yang boleh bertambah di minor. Varian terakhirnya `Unrecognized` dengan `#[serde(other)]`, ditulis `unrecognized`. Nama itu sengaja bukan `unknown`, karena `unknown` adalah state asli di beberapa enum (lifecycle, readiness, dan health spec 0004, status secret dan store spec 0011), dan enum seperti itu tetap bisa dibuat terbuka tanpa dua arti untuk satu kata. Daemon tidak pernah mengirim `unrecognized`. Enum terbuka di spec ini: `ClientKind`, `DaemonHealthStatus`, `SubsystemStatus`, `GapReason`, `ShutdownReason`, `ResourceKind`. Enum lifecycle, desired, readiness, dan health instance di wire tertutup (spec 0010), karena nilai `unknown` nya nyata dan himpunannya dari PRD 19.1. Enum lain tertutup sampai spec pemiliknya menyatakan terbuka.
- **Jenis event baru** tidak menggagalkan stream: client membaca `EventProbe { subscriptionId, sequence, kind: { type } }` lebih dulu, lalu hanya mendecode `Event` penuh untuk jenis yang dikenalnya. Jenis asing menjadi `SkippedEvent`, dan `sequence` nya tetap memajukan cursor.
- **Method baru** di client yang lebih baru: daemon lama menjawab `request.unknown_method`, dan client turun ke perilaku lama. Client sebaiknya memeriksa `Capabilities.methods` atau minor daemon lebih dulu.
- **Digest kontrak**: `cargo xtask codegen` menghitung SHA-256 hex lowercase atas bentuk ternormalisasi dari semua schema di `protocols/schemas/` dan katalog error, urut nama, dihitung dari isi di memori. Normalisasi membuang `description`, `title`, dan `examples` dari schema dan mengurutkan daftar yang urutannya tidak bermakna (`required`, `enum`, `type`, dan cabang `oneOf`); cabang `allOf` juga diurutkan karena semuanya harus cocok, sedangkan `anyOf` tetap berurutan karena serde mencoba varian untagged berurutan, dan `prefixItems` karena urutannya posisi tuple. Dari katalog normalisasi hanya mengambil `code`, `category`, `retryable`, kunci `details`, serta `kind`, kunci field, dan batas `items`, untuk semua area kecuali `cli` (kode `cli` tidak pernah lewat wire, jadi menambahnya tidak menaikkan minor protokol; spec 0008), jadi perubahan dokumentasi tidak mengubah digest. Sejak protokol 1.4 digest juga mencakup objek batas `limits::CONTRACT_LIMITS` (nama camelCase ke nilai `u64`): batas wire di *Batas*, panjang dan jumlah yang diperiksa setiap aturan `Validate`, dan batas yang dipotong error. Mengubah batas mana pun adalah perubahan kontrak, dan `--check` gagal tanpa versi baru; minor atau major dipilih penulis perubahan, karena digest tidak melihat arah. Untuk batas yang dipatuhi penerima, menaikkan batas menaikkan minor dan menurunkannya menaikkan major. Untuk batas yang dijamin pengirim (`fieldErrorsMax`, `fieldPathMax`, `messageMax`, `methodEchoMax`, `operationPageBytesMax`) arahnya terbalik: menurunkan minor, menaikkan major, karena pembaca lama boleh mengandalkannya. Nilai tetap (`frameHeaderLen`, `contractDigestLen`, `maxSafeInteger`, `operationListLimitDefault`) hanya berubah lewat major. Spec yang menambah batas di `stackly-protocol` menambahkannya ke daftar itu, dan test di `limits.rs` gagal untuk konstanta tingkat modul yang namanya memuat `MAX` atau `MIN`, atau berakhir `_LEN` atau `_DEFAULT`, yang tidak ada di daftar. Format (pola `requestId`, pola kind, tata bahasa reference secret) tidak masuk digest; fixture bersama yang mengikatnya. Hasilnya ditulis ke `crates/protocol/src/contract_digest.rs` (`pub const CONTRACT_DIGEST: &str`) dan `clients/swift/Sources/StacklyClient/Generated/ContractInfo.swift` (`ContractInfo.digest` dan `ContractInfo.protocolVersion`).
- **Catatan versi**: `protocols/contract-versions.json` berisi `{ "versions": [ { "major", "minor", "digest" } ] }`. `cargo xtask codegen` menambah entri untuk `CURRENT` kalau belum ada, dan menolak mengubah digest entri yang sudah ada kecuali dengan `--amend`; `--check` gagal kalau entri `CURRENT` tidak cocok dengan digest saat ini, atau kalau ada entri dengan versi lebih tinggi dari `CURRENT`.
- **Merge**: konflik di file hasil generate (`StacklyProtocol.swift`, schema, `error-codes.json`, file digest, `contract-versions.json`) diselesaikan dengan menggabungkan sumber Rust, menaikkan `CURRENT` kalau dua cabang sama sama menambah versi yang sama, lalu menjalankan `cargo xtask codegen` lagi; file hasil generate tidak pernah digabung tangan.
- **Drift saat berjalan**: `contractDrift` bernilai true kalau major dan minor client sama dengan daemon tetapi digest berbeda; itu bukan penolakan, hanya warning `contract_drift` di log daemon dan baris di `stackly daemon status`.

### Handshake

1. Client membuka koneksi ke socket yang sudah lolos `verify_path` dan `verify_peer` (AC-3), lalu mengirim `Request` dengan `call` `daemon.handshake`:
   `HandshakeParams { protocol: ProtocolVersion, client: ClientInfo { kind: ClientKind, version: String, pid: Option<u32>, interactive: Option<bool> }, contractDigest: String }`. `ClientKind` (terbuka): `cli`, `gui`. `interactive` true berarti client bisa menampilkan dialog yang dilihat pengguna saat itu juga: GUI selalu true, CLI true hanya kalau stdin dan stderr TTY dan tanpa mode noninteractive (spec 0011). Tidak ada berarti false, jadi client lama tidak pernah memicu dialog; karena itu field ini tambahan minor. `Validate`: `version` 1 sampai 64 karakter ASCII tercetak; `contractDigest` tepat 64 hex lowercase.
2. Daemon sudah memeriksa peer saat accept. Frame pertama harus lengkap dalam `HANDSHAKE_TIMEOUT` (5 detik) sejak accept; kalau tidak, `ipc.handshake_timeout` lalu tutup.
3. Kalau `call.type` adalah `daemon.handshake` dan `handshake_major` terbaca tetapi berbeda dengan major daemon: `protocol.major_mismatch` dengan `details` `serverMajor` dan `serverMinor`, lalu tutup. Langkah ini jalan sebelum params lain didecode.
4. Kalau frame gagal di tahap mana pun dari `decode_request`: kirim error tahap itu, lalu tutup. Request yang valid dengan method selain `daemon.handshake` (termasuk method yang tidak dikenal): `protocol.handshake_required`, lalu tutup.
5. Major sama dan params valid: `HandshakeAccepted { protocol, serverInstanceId: Uuid, daemon: DaemonIdentity { version, pid }, contractDigest, contractDrift: bool, limits: Limits, head: EventCursor }`. Minor client yang lebih lama atau lebih baru sama sama diterima.
6. Handshake kedua di koneksi yang sama mendapat `protocol.handshake_repeated` tanpa menutup koneksi.
7. Daemon memproses frame satu per satu sampai handshake selesai, jadi request yang dikirim client di belakang handshake (pipelining) tidak pernah diproses kalau handshake ditolak.

### Envelope error

`ErrorBody` (VAL-020):

| Field | Tipe | Wajib | Isi |
| --- | --- | --- | --- |
| `code` | string | ya | kode katalog `<area>.<alasan>` |
| `category` | `ErrorCategory` | ya | dari katalog, tidak pernah dipilih di tempat error dibuat |
| `message` | string | ya | satu kalimat bahasa Inggris, huruf kecil di awal, tanpa titik di akhir, tanpa nilai secret (aturan redaksi spec 0011); `ErrorBody::new` memotongnya di 200 karakter pada batas karakter |
| `retryable` | bool | ya | dari katalog: true hanya kalau daemon menjamin request itu tidak berefek apa pun, sehingga request yang sama persis boleh dikirim lagi; dibekukan lintas major |
| `resource` | `ResourceRef` atau null | tidak | resource yang bersangkutan |
| `fields` | `FieldError[]` | tidak | `{ path, code, message }`; `path` relatif terhadap params, misalnya `name` atau `kinds[3]`, paling panjang 256 karakter; `code` dari set tetap di *Frame dan pesan*; `message` teks tetap per kode; paling banyak 32, sisanya dihitung di `details.omitted` |
| `hint` | string atau null | tidak | langkah pemulihan yang aman, bahasa Inggris; selalu teks statis dari `codes::<area>::hint` (lihat *Hint* di bawah) |
| `details` | `ErrorDetail[]` | tidak | `{ key, value }` string; kunci camelCase hanya dari daftar `details` kode itu di katalog |
| `operationId` | UUID atau null | tidak | operasi yang gagal (spec 0007) |
| `rule` | string atau null | tidak | ID aturan PRD, misalnya `BR-008`, dari `rule()` error domain |
| `items` | `ErrorItem[]` | tidak | daftar terstruktur yang tidak muat di `details`: `{ kind, fields: ErrorDetail[] }`, dengan `kind` dan kunci `fields` dari katalog kode itu |

`ResourceRef { kind: ResourceKind, id: String }`. `ResourceKind` (terbuka): `runtime`, `instance`, `environment`, `operation`, `provider`, `backup`; ditambah pemiliknya: `secret` (spec 0011, id teks reference) dan `manager` (spec 0007, id `self`). `id` adalah bentuk teks kanonis kunci resource: UUID lowercase untuk runtime, instance, environment, operation, dan backup; bentuk kunci provider diputuskan spec 0009.

`ErrorCategory` adalah enum tertutup (kategori baru berarti major baru). Arti setiap kategori, dalam bentuk apa yang sebaiknya dilakukan client, dan exit code PRD bagian 12 yang diturunkan fitur 8:

| Kategori | Arti untuk client | Exit code (fitur 8) |
| --- | --- | --- |
| `invalid_input` | perbaiki input; mengulang tanpa perubahan pasti gagal | 2 |
| `not_found` | resource tidak ada; segarkan snapshot | 10 (spec 0008) |
| `conflict` | state sudah berubah atau resource sedang dipakai; segarkan lalu putuskan | 3 |
| `unauthorized` | identitas, peer, atau trust ditolak; jangan diulang otomatis | 4 |
| `unsupported` | versi, method, atau capability tidak didukung | 5 |
| `unavailable` | daemon atau dependency belum tersedia; lihat `retryable` sebelum mengulang | 6 |
| `failed` | pekerjaan dijalankan dan gagal | 7 |
| `timeout` | batas waktu terlewati; hasilnya bisa belum diketahui, periksa operasinya | 8 |
| `cancelled` | dibatalkan | 9 |
| `internal` | bug atau invariant yang rusak di Stackly | 1 (spec 0008) |

Katalog di `stackly-protocol` modul `codes`: `ErrorSpec { code: &'static str, category: ErrorCategory, retryable: bool, details: &'static [&'static str], items: Option<ItemSpec>, summary: &'static str }` dengan `ItemSpec { kind: &'static str, fields: &'static [&'static str], max: u16 }`, satu submodul per area (misalnya `codes::ipc`) yang berisi konstanta per kode dan satu slice `ALL`, `AREAS`, dan `catalog()` yang menggabungkan slice semua area. Pemecahan per area membuat spec yang ditulis paralel tidak berebut satu daftar. `ErrorBody` hanya dibuat lewat `ErrorBody::new(&ErrorSpec, message)` ditambah builder untuk field opsional; `with_detail(key, value)` menolak kunci di luar `ErrorSpec.details` dengan `debug_assert`, dan test katalog memeriksa pola kode, tidak ada kode ganda, area terdaftar, dan setiap konstanta ada di `ALL` area nya. `cargo xtask codegen` menulis `protocols/error-codes.json` (urut kode) untuk Swift, dokumentasi, dan fitur 8.

**Hint** (satu bentuk untuk semua area, ditambah 2026-10-20 dari spec 0017 dan 0018):

- Setiap submodul katalog yang punya hint menyediakan `codes::<area>::hint(code: &str, details: &[ErrorDetail]) -> Option<&'static str>`. Fungsi itu memilih teks statis dari kode dan nilai `details`; teks hint tidak pernah diisi nilai (tanpa placeholder). Nilai yang berubah (versi, path, host, ukuran) ada di `message` dan `details`. Area tanpa hint tidak menyediakan fungsi itu, dan errornya tanpa hint.
- `stacklyd` memasang hint lewat `ErrorBody::with_hint` saat membangun error di modul `map`. Untuk `FailureError` operasi (spec 0007) yang hint tersimpannya kosong, pemetaan `stacklyd` memanggil fungsi yang sama dengan kode dan `details` failure itu; hint yang tersimpan tetap menang. Fallback ini ditambah pemilik area pertama yang butuh (spec 0018 langkah 11), tanpa mengubah spec 0007.
- Batas 200 karakter adalah properti katalog, bukan batas kontrak: test katalog setiap area memanggil `hint` untuk setiap kode dan setiap nilai `details` yang memilih teks lain, lalu memeriksa panjangnya paling banyak 200 karakter dan bahwa teksnya bahasa Inggris tanpa `{`. Karena itu batas ini tidak masuk `CONTRACT_LIMITS` dan tidak mengubah digest, dan `with_hint` tidak memotong.

Area dan pemiliknya. Spec yang pertama kali butuh area baru menambahkannya ke `AREAS` di spec dan kodenya sendiri:

| Area | Pemilik |
| --- | --- |
| `protocol`, `ipc`, `request`, `events`, `resource`, `daemon`, `internal` | spec 0006 |
| `registry` | spec 0005 |
| `operation` | spec 0007 |
| `cli` (hanya di output CLI, tidak pernah di wire) | spec 0008 |
| `manifest`, `provider` | spec 0009 |
| `supervisor`, `process` | spec 0010 |
| `secret` | spec 0011 |
| `instance` | spec 0010 |
| `catalog` | spec 0017 |
| `runtime` | spec 0018 |
| `storage` | spec 0022 |
| `environment`, `config`, `backup`, `diagnostics`, `platform` | fitur yang pertama kali memakainya |

Kode yang dimiliki spec ini (semua `retryable` false kecuali disebut):

| Kode | Kategori | Kapan | Kunci `details` |
| --- | --- | --- | --- |
| `protocol.handshake_required` | `invalid_input` | frame pertama valid tetapi bukan handshake | `method` |
| `protocol.handshake_repeated` | `invalid_input` | handshake kedua di koneksi yang sama | |
| `protocol.major_mismatch` | `unsupported` | major berbeda | `serverMajor`, `serverMinor` |
| `ipc.peer_rejected` | `unauthorized` | uid peer berbeda atau tidak terbaca | |
| `ipc.endpoint_missing` | `unavailable`, retryable | client: tidak ada apa pun di path socket | `path` |
| `ipc.endpoint_untrusted` | `unauthorized` | client: path atau peer gagal diperiksa | `path`, `reason` |
| `ipc.connection_refused` | `unavailable`, retryable | client: connect ditolak (socket basi) | `path` |
| `ipc.connection_closed` | `unavailable` | client: daemon menutup koneksi, atau baca atau tulis di koneksi gagal, sebelum balasan tiba; request mungkin sudah berjalan, jadi hanya aman diulang dengan `idempotencyKey` yang sama | |
| `ipc.io_error` | `unavailable`, retryable | error OS lain sebelum request terkirim: client saat memeriksa path atau connect (misalnya izin ditolak di folder, nama terlalu panjang, terlalu banyak file terbuka), daemon saat bind, chmod, rename, unlink, atau accept | `op`, `kind` |
| `ipc.protocol_violation` | `internal` | client: frame daemon tidak bisa didecode atau tag reply tidak cocok | |
| `ipc.endpoint_occupied` | `conflict` | daemon: path socket berisi sesuatu yang bukan socket | `path`, `kind` |
| `ipc.frame_empty` | `invalid_input` | panjang frame 0 | |
| `ipc.frame_too_large` | `invalid_input` | panjang di atas batas | `length`, `max` |
| `ipc.frame_stalled` | `timeout` | frame belum lengkap setelah 30 detik | |
| `ipc.handshake_timeout` | `timeout` | handshake tidak lengkap dalam 5 detik | |
| `ipc.write_timeout` | `timeout` | daemon: client tidak membaca selama 30 detik; hanya di log | |
| `ipc.reply_too_large` | `internal` | daemon: reply melebihi batas frame; dikirim menggantikan reply itu | `method`, `length` |
| `ipc.too_many_connections` | `unavailable`, retryable | koneksi ke 65 | `max` |
| `ipc.not_connected` | `unavailable`, retryable | client: call dibuat saat tidak ada koneksi yang tersambung (spec 0013); tidak ada yang dikirim | |
| `ipc.call_timeout` | `timeout` | client: satu panggilan tidak dijawab dalam batas waktu nya; dipakai `stackly-ipc` (CLI) dan `StacklyClient` (GUI) dengan arti yang sama; request bisa saja sudah berefek | `method`, `seconds`, `idempotencyKey` |
| `request.malformed` | `invalid_input` | JSON rusak, bentuk salah, request atau `call` bukan objek JSON, format `requestId` atau `idempotencyKey` salah | `omitted` |
| `request.too_deep` | `invalid_input` | kedalaman JSON lebih dari 32 | `max` |
| `request.unknown_method` | `unsupported` | method tidak dikenal | `method` |
| `request.invalid_params` | `invalid_input` | params bukan objek JSON, gagal decode, atau gagal `Validate`; `fields` terisi paling banyak 32 | `method`, `omitted` |
| `request.duplicate_id` | `invalid_input` | `requestId` sedang dipakai request lain di koneksi itu | |
| `request.too_many_in_flight` | `unavailable`, retryable | request ke 33 yang berjalan bersamaan | `max` |
| `events.too_many_subscriptions` | `invalid_input` | subscription ke 9 | `max` |
| `events.unknown_subscription` | `not_found` | unsubscribe ID yang tidak ada | `subscriptionId` |
| `resource.not_found` | `not_found` | dicadangkan untuk pemilik resource | |
| `resource.revision_mismatch` | `conflict` | dicadangkan untuk CAS spec 0005 | `expected`, `actual` |
| `resource.busy` | `conflict`, retryable | dicadangkan untuk lock resource spec 0007 | `operationId` |
| `daemon.starting` | `unavailable`, retryable | method butuh subsystem yang belum siap selama start | `subsystem` |
| `daemon.shutting_down` | `unavailable`, retryable | request saat daemon berhenti | |
| `daemon.lock_held` | `conflict` | daemon kedua; stderr saja | `pid`, `serverInstanceId`, `version`, `startedAt` |
| `internal.unexpected` | `internal` | handler panic atau invariant rusak | |

Kode di area milik spec ini yang ditambah dan dimiliki spec lain (semua `retryable` false):

| Kode | Kategori | Kapan | Kunci `details` | Pemilik |
| --- | --- | --- | --- | --- |
| `daemon.restart_unavailable` | `unsupported` | `daemon.shutdown` mode `restart` dikirim ke daemon berkonteks `manual`; daemon tetap jalan | `launchContext` | spec 0015 |

`ErrorItem { kind: String, fields: Vec<ErrorDetail> }` adalah bentuk tertutup untuk isi yang berupa daftar, misalnya daftar reference yang butuh secret atau daftar issue manifest. Karena `kind` dan kunci field adalah string, client selalu bisa mendecode frame error, termasuk item yang `kind` nya belum dikenal; `code`, `category`, dan `message` tidak pernah hilang karena isi daftar. `ErrorSpec.items` menyebut satu `kind`, daftar kunci field, dan batas jumlah item untuk kode itu (atau tidak ada); `ErrorBody::with_items` menolak `kind`, kunci, atau jumlah lain dengan `debug_assert`, test katalog memeriksanya, dan item di atas batas dibuang dengan jumlahnya ditulis di `details.omitted` (kunci `omitted` wajib ada di daftar `details` kode yang memakai `items`). `kind` dan kunci item ditambah spec pemilik kodenya: `reference`, `use`, dan `effect` (spec 0011), serta `issue` (spec 0009). Spec ini sendiri tidak memakai `items`.

Nilai `details.reason` untuk `ipc.endpoint_untrusted`: `not_a_socket`, `symlink`, `wrong_owner`, `group_or_other_access`, `parent_wrong_owner`, `parent_group_or_other_access`, `peer_uid_mismatch`, `peer_credentials_unavailable`.

Error sisi client di `stackly-ipc` (`IpcError`, thiserror) punya `to_error_body()` yang memakai kode di atas, jadi CLI dan GUI selalu bekerja dengan satu bentuk error.

### Event dan cursor

- `Event { subscriptionId: u32, sequence: Sequence, at: Timestamp, resource: Option<ResourceRef>, revision: Option<Revision>, kind: EventKind }`. Di daemon, event hanya dibuat lewat `publish_resource(kind, ResourceRef, Revision)` atau `publish_daemon(kind)`, jadi event tentang resource selalu membawa `resource` sekaligus `revision`, dan event daemon tidak membawa keduanya.
- **Aturan revision** (untuk semua pemilik resource): revision naik per resource (bukan global) dan tidak pernah turun; `revision` di event adalah revision resource setelah perubahan itu; event penghapusan membawa revision terakhir ditambah 1; setiap DTO snapshot resource membawa `revision` dengan arti yang sama. Observasi (readiness dan health spec 0004, blocker dan status secret spec 0011) tidak menaikkan revision, jadi event observasi membawa revision resource yang sama dengan sebelumnya. Event yang revision nya tidak naik hanya boleh membawa observasi, dan setiap observasi membawa waktu observasinya (`observedAt`, atau nama yang ditetapkan pemiliknya seperti `statusObservedAt`). Jenis observasi dibedakan per resource (readiness, health, blocker, status secret). Pemilik observasi wajib mempublikasikan event saat nilai efektif yang dikirim berubah tanpa observasi baru, misalnya readiness `ready` yang menjadi `unknown` karena basi (spec 0004 `effective`), karena client tidak menghitung kesegaran sendiri.
- `EventKind` di spec ini: `events.gap(Gap { reason: GapReason })` dengan `GapReason` `overflow`, `cursor_expired`, `server_restarted`, dan `daemon.shutting_down(ShuttingDown { reason: ShutdownReason })` dengan `ShutdownReason` `signal` (SIGTERM atau SIGINT), `requested` (`daemon.shutdown` mode `stop`, spec 0015), dan `restart_requested` (`daemon.shutdown` mode `restart`, ditambah spec 0015). Jenis lain ditambah pemiliknya (lihat *Asumsi ke spec lain*). `EventKind::KINDS` adalah daftar jenis yang dikenal.
- `EventCursor { serverInstanceId: Uuid, sequence: Sequence }`. `serverInstanceId` adalah UUID v7 baru setiap daemon start. `sequence` dihitung per run, mulai 1, naik 1 per event yang dipublikasikan; `head` dengan `sequence` 0 berarti belum ada event.
- Event bus di library `stacklyd`: satu `Mutex` memegang penghitung sequence, ring `VecDeque<Arc<Event>>` berisi `EVENT_REPLAY_LEN` (4096) event terakhir, dan `tokio::sync::broadcast::Sender` berkapasitas `EVENT_BUFFER_LEN` (1024). Publish adalah fungsi sync: mengambil `at` dari `Clock`, memberi sequence, memasukkan ke ring, dan mengirim ke broadcast di dalam lock yang sama, sehingga urutan ring dan broadcast selalu sama dan publish tidak pernah menunggu subscriber.
- `events.subscribe(SubscribeParams { since: Option<EventCursor>, kinds: Vec<String>, resource: Option<ResourceRef> })`. `kinds` kosong berarti semua; setiap entri adalah jenis lengkap (`instance.state_changed`) atau awalan area yang diakhiri titik (`operation.`), cocok dengan `^[a-z][a-z0-9_]*\.([a-z][a-z0-9_]*)?$`, paling banyak 32 entri (dicek `Validate`). `resource` membatasi ke satu resource. `events.gap` dan `daemon.shutting_down` selalu dikirim walau tidak cocok dengan filter.
- `subscriptionId` mulai 1 per koneksi, naik 1, dan tidak dipakai ulang di koneksi itu. Task penerus event tidak dihitung sebagai request berjalan.
- Di dalam lock bus, daemon membuat receiver broadcast, menyalin event ring setelah `since`, dan membaca `head`. Lalu ia memasukkan `Subscribed { subscriptionId, head }` ke antrian keluar lebih dulu, baru memulai task penerus yang mengirim event salinan ring yang lolos filter, lalu event dari receiver yang `sequence` nya lebih besar dari event terakhir yang terkirim.
- Gap: `since.serverInstanceId` berbeda menghasilkan `server_restarted`; `since.sequence` lebih kecil dari sequence tertua di ring dikurangi 1 menghasilkan `cursor_expired`; keduanya dikirim sebagai event pertama dengan `sequence` sama dengan `head.sequence`, lalu stream lanjut dari event baru. `since.sequence` lebih besar dari `head` pada run yang sama adalah `request.invalid_params` (`fields[].path` `since.sequence`, kode `out_of_range`). Receiver yang tertinggal (`RecvError::Lagged(n)`) mengirim `events.gap` reason `overflow` dengan `sequence` sama dengan sequence terakhir yang terbuang, menambah `eventsDropped` sebanyak `n`, lalu lanjut.
- Penerusan event ke koneksi lewat antrian keluar yang sama dengan response. Subscriber yang client nya berhenti membaca tertahan di antrian itu, tertinggal di broadcast, lalu menerima gap; publish tetap tidak menunggu.
- `events.unsubscribe(UnsubscribeParams { subscriptionId })` menghentikan subscription; ID yang tidak ada menghasilkan `events.unknown_subscription`. Menutup koneksi mengakhiri semua subscription nya.
- Spec ini hanya mempublikasikan `daemon.shutting_down`; `events.gap` dibuat per subscription.

**Resep snapshot plus cursor** (wajib untuk client di fitur 8 dan 13, dan menjadi syarat bagi pemilik resource):

1. Connect, handshake, simpan `serverInstanceId`.
2. `events.subscribe` dengan `since` berisi cursor yang tersimpan, atau null.
3. Kalau tidak punya cursor, atau event pertama adalah gap, ambil snapshot lewat method list dan inspect milik pemilik resource. Setiap resource di snapshot membawa `revision`.
4. Terapkan event urut `sequence`; lewati event dengan `sequence` yang sudah diterapkan. Untuk event resource: lewati yang `revision` nya lebih kecil dari revision yang dipegang client untuk resource itu; terapkan yang lebih besar; dan untuk yang sama, terapkan setiap jenis observasi yang waktu observasinya sama atau lebih baru dari observasi jenis itu yang dipegang client untuk resource itu. Waktu yang sama diterima karena wire hanya membawa tiga digit pecahan; urutan `sequence` yang menentukan. Pengecualian: event yang membawa penghitung urutan milik pemiliknya diterapkan menurut penghitung itu, bukan menurut revision atau waktu observasi. Satu satunya hari ini adalah status instance dengan `statusSeq` per instance per run daemon (spec 0010), di `instance.health_changed` maupun di field status `InstanceSummary` milik `instance.state_changed`: field status diterapkan kalau `statusSeq` lebih besar dari yang dipegang client, dan snapshot baru diambil setelah gap `server_restarted`.
5. Simpan cursor `(serverInstanceId, sequence terakhir)`, termasuk sequence dari `SkippedEvent`.
6. Setiap gap berikutnya mengulang langkah 3 tanpa memutus stream.

Karena subscribe dilakukan sebelum snapshot diambil, event yang terjadi di antaranya tidak hilang, revision mencegah event lama menimpa snapshot yang lebih baru, dan waktu observasi per jenis melakukan hal yang sama untuk observasi.

### Batas

Batas yang dikirim ke client ada di `stackly-protocol::limits` dan di `HandshakeAccepted.limits`:

| Konstanta | Nilai |
| --- | --- |
| `CONTROL_FRAME_MAX` | 1 048 576 byte payload |
| `LOG_BATCH_MAX` | 65 536 byte payload; dipatuhi pembuat batch log (fitur 27) |
| `MAX_JSON_DEPTH` | 32 |
| `MAX_IN_FLIGHT_REQUESTS` | 32 per koneksi |
| `MAX_SUBSCRIPTIONS` | 8 per koneksi |
| `EVENT_BUFFER_LEN` | 1024 event |
| `EVENT_REPLAY_LEN` | 4096 event |
| `REQUEST_ID_MAX`, `IDEMPOTENCY_KEY_MAX` | 64 dan 128 byte |
| `MAX_SAFE_INTEGER` | 9 007 199 254 740 991 |
| `FIELD_ERRORS_MAX` | 32 `fields` per error; sisanya dihitung di `details.omitted` (protokol 1.4) |
| `FIELD_PATH_MAX` | 256 karakter per `fields[].path` (protokol 1.4) |

`Limits` di handshake berisi `maxFrameBytes`, `maxLogBatchBytes`, `maxJsonDepth`, `maxInFlightRequests`, `maxSubscriptions`, `eventBufferLen`, dan `eventReplayLen`, semuanya `u32` dari konstanta di atas.

`limits::CONTRACT_LIMITS` mendaftar setiap batas kontrak dengan nama camelCase, urut nama, dan masuk digest kontrak (lihat *Versi dan kompatibilitas*). Di protokol 1.4 isinya 28 entri:

| Nama | Konstanta | Nilai |
| --- | --- | --- |
| `clientVersionMax`, `contractDigestLen` | `handshake::CLIENT_VERSION_MAX`, `CONTRACT_DIGEST_LEN` | 64, 64 |
| `controlFrameMax`, `logBatchMax`, `frameHeaderLen` | `CONTROL_FRAME_MAX`, `LOG_BATCH_MAX`, `frame::HEADER_LEN` | 1 048 576, 65 536, 4 |
| `eventBufferLen`, `eventReplayLen` | `EVENT_BUFFER_LEN`, `EVENT_REPLAY_LEN` | 1024, 4096 |
| `fieldErrorsMax`, `fieldPathMax` | `FIELD_ERRORS_MAX`, `FIELD_PATH_MAX` | 32, 256 |
| `idempotencyKeyMax`, `requestIdMax` | `IDEMPOTENCY_KEY_MAX`, `REQUEST_ID_MAX` | 128, 64 |
| `maxInFlightRequests`, `maxJsonDepth`, `maxSubscriptions` | `MAX_IN_FLIGHT_REQUESTS`, `MAX_JSON_DEPTH`, `MAX_SUBSCRIPTIONS` | 32, 32, 8 |
| `maxSafeInteger` | `MAX_SAFE_INTEGER` | 9 007 199 254 740 991 |
| `messageMax`, `methodEchoMax` | `error::MESSAGE_MAX`, `json::METHOD_ECHO_MAX` | 200, 64 |
| `operationCursorMax`, `operationPageBytesMax` | `operation::OPERATION_CURSOR_MAX`, `OPERATION_PAGE_BYTES_MAX` | 80, 921 600 |
| `operationListLimitDefault`, `operationListLimitMin`, `operationListLimitMax` | `operation::OPERATION_LIST_LIMIT_DEFAULT`, `_MIN`, `_MAX` | 50, 1, 200 |
| `maxSecretLen`, `secretReferenceMax` | `secret::MAX_SECRET_LEN`, `SECRET_REFERENCE_MAX` | 65 536, 255 |
| `secretSegmentMax`, `secretSegmentsMin`, `secretSegmentsMax` | `secret::SEGMENT_MAX`, `SEGMENTS_MIN`, `SEGMENTS_MAX` | 64, 2, 8 |
| `subscribeKindsMax` | `events::SUBSCRIBE_KINDS_MAX` | 32 |

Batas milik transport dan daemon saja: `READ_CHUNK` 65 536 byte (di `stackly-ipc`), dan konstanta di `stacklyd`: `MAX_CONNECTIONS` 64, `HANDSHAKE_TIMEOUT` 5 detik, `FRAME_STALL_TIMEOUT` 30 detik, `WRITE_TIMEOUT` 30 detik, `REJECT_WRITE_TIMEOUT` 1 detik, `SHUTDOWN_DRAIN` 5 detik, `SHUTDOWN_FLUSH` 1 detik, `SOCKET_CHECK_INTERVAL` 10 detik, `ACCEPT_BACKOFF` 100 ms sampai 1 detik, `OUTBOUND_QUEUE_LEN` 64 frame per koneksi, `LOG_FILE_MAX` 20 MiB, `LOG_FILES_KEPT` 5, `LOG_QUEUE_LEN` 8192 baris, `LOCK_INFO_MAX` 4 KiB. Benchmark fitur 64 boleh menyetel ulang angka ini lewat update spec ini.

### Transport `stackly-ipc`

Semua di modul `unix` (`cfg(unix)`); named pipe Windows nanti menjadi modul kedua dengan API yang sama. Waktu memakai `tokio::time::Instant`, supaya test bisa menjeda jam (`tokio::time::pause`, fitur `test-util`).

- `Listener::bind(path, expected_uid: u32) -> Result<Listener, IpcError>`:
  1. Periksa `path` dan `<path>.tmp` dengan `lstat`. Masing masing boleh tidak ada atau berjenis socket; selain itu (termasuk symlink) `ipc.endpoint_occupied` tanpa menghapus apa pun.
  2. Hapus `<path>.tmp` kalau berupa socket, bind socket baru di `<path>.tmp` (`std::os::unix::net::UnixListener::bind`, yang sekaligus `listen`), `chmod` 0600, lalu `rename` ke `path` (rename atomik juga mengganti socket lama). Karena runtime root 0700 dan nama akhir baru muncul setelah `chmod`, path akhir tidak pernah terlihat dengan mode lain. Nama sementara muat dalam anggaran socket spec 0004 (runtime root 76 byte ditambah `/stacklyd.sock.tmp`).
  3. Ubah ke listener tokio dan catat device dan inode path akhir.
  Pemanggil wajib sudah memegang lock daemon; prasyarat ini ditulis di doc comment.
- `Listener::accept() -> Result<Accepted, IpcError>`. Kredensial peer dibaca lewat `PeerCredSource` (default: `UnixStream::peer_cred()` milik tokio, yang memakai getpeereid, dan pid lewat `LOCAL_PEERPID` bila tersedia); test bisa memasang source yang gagal. Uid berbeda atau kredensial gagal dibaca: listener menyerahkan penulisan frame error `ipc.peer_rejected` ke task terpisah dengan batas `REJECT_WRITE_TIMEOUT` lalu menutup koneksi, dan mengembalikan `Accepted::Rejected { uid, pid }` untuk dicatat. Hanya `Accepted::Authorized { connection, peer }` yang bisa dipakai membaca frame, jadi pemeriksaan peer tidak bisa terlewat. Accept loop tidak pernah menunggu penulisan ke peer.
- `Listener::check_bound() -> BindState` (`Bound`, `Missing`, `Replaced`) membandingkan `lstat` path dengan device dan inode yang dicatat; `Listener::rebind()` mengulang langkah bind; `Listener::unlink_if_own()` menghapus path hanya kalau device dan inode masih sama.
- `FramedConnection`: `read_frame(deadline: Option<Instant>) -> Result<Option<Vec<u8>>, IpcError>` membaca potongan paling besar `READ_CHUNK` ke `FrameDecoder` dengan `CONTROL_FRAME_MAX`; `None` berarti EOF bersih di antara frame; `deadline` berlaku sampai frame lengkap; begitu byte pertama sebuah frame diterima tanpa `deadline`, frame itu harus lengkap dalam `FRAME_STALL_TIMEOUT`. `write_frame(&[u8])`. Error framing di sisi daemon mengirim frame error lalu menutup.
- `endpoint::verify_path(path, owner_uid) -> Result<(), IpcError>` (pemeriksaan path di AC-3, lewat `std::fs::symlink_metadata` dan `MetadataExt`) dan `endpoint::verify_peer(&UnixStream, peer_uid) -> Result<(), IpcError>`. `connect_verified(path, ConnectPolicy { owner_uid, peer_uid }) -> Result<FramedConnection, IpcError>` menjalankan keduanya; di produksi kedua uid sama dengan uid client, dan test memberi `peer_uid` yang berbeda untuk menguji `verify_peer`.
- `Client::connect(path, ConnectPolicy, ClientInfo) -> Result<(Client, HandshakeAccepted), IpcError>` melakukan verifikasi dan handshake. `Client::call(Call) -> Result<Reply, Box<ErrorBody>>` membuat `requestId` UUID v7, mencocokkan balasan, dan memeriksa tag reply. Kalau menulis gagal (koneksi ditutup daemon), client masih membaca frame yang tertunda paling lama 1 detik dan melaporkan frame error daemon kalau ada, bukan `ipc.connection_closed`. `Client::subscribe(SubscribeParams) -> Result<(Subscribed, EventStream), Box<ErrorBody>>`; `EventStream` mengeluarkan `DecodedFrame::Event` dan `SkippedEvent`.
- `Client::call_detailed(Call, CallOptions { request_id: Option<String>, idempotency_key: Option<String> }) -> Result<Reply, Box<ErrorBody>>` (spec 0008) memberi pemanggil kendali atas `requestId` dan `idempotencyKey`; `Client::call` sama dengan `call_detailed` dengan opsi default. `call` dan `call_detailed` boleh berjalan selagi `EventStream` dari koneksi yang sama hidup. `stackly-ipc` membangun `ipc.call_timeout` dari batas waktu yang diberikan pemanggil; connect plus handshake yang melewati batasnya juga `ipc.call_timeout` dengan `method` `daemon.handshake`, di CLI maupun GUI. `ipc.handshake_timeout` tetap kode sisi daemon. Tanpa koneksi, `ipc.not_connected`.
- `endpoint::evaluate_path(facts: &PathFacts, owner_uid) -> Result<(), UntrustedReason>` adalah evaluator murni di balik `verify_path`, dengan urutan cek dan pemetaan `io::ErrorKind` ke snake_case yang sama dengan Swift; fixture bersama `protocols/fixtures/endpoint/checks.json` (spec 0013) diuji di `crates/ipc/tests/endpoint_fixture.rs` dan `StacklyClient`.
- Error OS saat `lstat` atau connect dipetakan: `NotFound` menjadi `ipc.endpoint_missing`, `ConnectionRefused` menjadi `ipc.connection_refused`, selain itu `ipc.io_error` dengan `op` (`stat` atau `connect`) dan `kind` (`ErrorKind` dalam snake_case). `IpcError::Io` dengan `op` `read` atau `write` gagal di koneksi yang mungkin sudah membawa request, jadi dipetakan ke `ipc.connection_closed` yang tidak retryable, bukan ke `ipc.io_error`; `op` dan `kind` tidak ikut karena kode itu tanpa `details` (teks error OS tetap ada di log). Client Swift sudah memakai `ipc.connection_closed` untuk koneksi yang putus, jadi aturan ini sama di kedua client.

### Daemon

`stacklyd` adalah library plus bin. `main.rs` hanya membaca argumen dan environment lalu memanggil `stacklyd::run(StartConfig)`. Library memberi `Daemon::start(deps) -> DaemonHandle` (dengan `bus()`, `health()`, dan `shutdown()`; spec 0015 menambah `stop_requested()`) untuk test di dalam proses, dengan `MacosPlatform::with_roots(test_root.roots())` dan trait `Handlers` (satu method per `Call`, implementasi default memanggil use case nyata) yang bisa dibungkus test untuk menahan atau memperlambat handler. Tambahan spec lain: `StartConfig` mendapat `catalog_dir` (spec 0017), serta `xpc_service_name`, `parent_pid`, dan `current_exe` (spec 0015); `DaemonDeps` mendapat `launch` dan `bundle` (spec 0015), yang juga bisa diisi langsung oleh test; dan `DaemonConfig` mendapat override `catalog: Option<CatalogOverride>` khusus test (spec 0017; `CatalogOverride` dengan varian `Location` dan `Preloaded`).

**Urutan start** (`startup`):

1. Parse argumen (clap): `--version`, `--help`, dan `--print-roots`. Baca `STACKLY_ROOT` dan `STACKLY_LOG` sekali, juga `STACKLY_CATALOG_DIR` (spec 0017), `XPC_SERVICE_NAME` (diset launchd, bukan pengguna), parent pid, dan `current_exe` (spec 0015). `STACKLY_LOG` yang tidak valid sebagai filter tracing membuat daemon keluar 1. `STACKLY_CATALOG_DIR` yang kosong, relatif, tidak ada, atau bukan folder membuat daemon keluar 1 dengan pesan `STACKLY_CATALOG_DIR "<nilai>" is not an absolute path to a folder`, divalidasi di sebelah filter log.
2. Mulai thread penulis log (hanya stderr dulu) dan pasang subscriber tracing JSON.
3. `MacosPlatform::new(root_override)`; gagal berarti keluar 1 (perilaku spec 0004). Dengan `--print-roots`: catat satu baris berisi keempat root resolved, path socket, dan `rootOverride`, lalu keluar 0, tanpa langkah berikutnya.
4. `prepare_layout`; gagal berarti keluar 1. Pada layout yang sudah ada, langkah ini tidak mengubah apa pun, dan ia sudah tahan terhadap dua start bersamaan.
5. Buat `serverInstanceId` (UUID v7) dan `startedAt` (`Clock`).
6. Ambil `DaemonLock` di `daemon_lock()`: buka dengan create tanpa truncate, `O_NOFOLLOW`, mode 0600; tolak kalau yang terbuka bukan file biasa (keluar 1); `File::try_lock()`. Kalau lock dipegang proses lain: baca isi lock file paling banyak 4 KiB, cetak `daemon.lock_held` dengan pid, `serverInstanceId`, versi, dan waktu start pemilik (atau `owner unknown` kalau isinya tidak terbaca) ke stderr, lalu keluar 3. Kalau berhasil: kosongkan file lewat handle yang memegang lock, tulis `{"pid","serverInstanceId","startedAt","version"}` sebagai JSON, `sync_data`. File lock tidak pernah diganti lewat rename, karena lock melekat pada inode.
7. Kirim pesan pasang sink file ke thread penulis log (lihat *Log daemon*).
8. Catat baris start: versi, protokol, digest, `serverInstanceId`, pid, uid, keempat root resolved, path socket, dan `rootOverride`, ditambah `launch_context`, `launch_label`, dan `bundle_path` (spec 0015).
8a. Launch record (spec 0015): hanya untuk konteks `launchd` berlabel produksi tanpa `STACKLY_ROOT`, daemon menulis `<data>/background-agent.json` lewat `write_file_atomic`; kegagalan dicatat `warn` dan start berlanjut. Langkah ini berada di antara lock (langkah 6) dan bind (langkah 9).
9. Buat event bus dan state daemon dengan status `starting`, lalu `Listener::bind(daemon_socket(), uid)`. `ipc.endpoint_occupied` atau error bind lain berarti keluar 1.
10. Jalankan accept loop dan penangan sinyal. Koneksi sudah dilayani; `get_health` melaporkan `starting`.
11. Slot start: langkah yang harus selesai sebelum daemon siap, dijalankan berurutan. Urutannya: `open_registry` (spec 0005, mengambil alih subsystem `registry`), lalu `OperationEngine::recover` (spec 0007, subsystem `operations`), lalu rekonsiliasi supervisor (spec 0010, diperkuat fitur 30, subsystem `supervisor`), lalu status `ready`, lalu `OperationEngine::start()` (spec 0007). Catalog (spec 0017, subsystem `catalog`) dimuat tepat setelah `open_registry`, juga dalam recovery mode, dan setelah registry `Ready` daemon menjalankan `ensure_source` spec 0017 (tidak dalam recovery mode). Dalam recovery mode, `recover`, rekonsiliasi, dan `start()` dilewati. Selama slot berjalan, method yang butuh subsystem yang belum siap menjawab `daemon.starting`; keenam method di spec ini dan `daemon.shutdown` (spec 0015) selalu dilayani.
   - `Ready`: subsystem `registry` menjadi `ok`, `DaemonInfo.registrySchema` terisi, lalu `prune(&RetentionPolicy::DEFAULT, now)` sekali dan task berkala setiap `REGISTRY_PRUNE_INTERVAL` (24 jam) lewat `spawn_blocking`. Prune yang gagal dicatat warning dan dicoba lagi di jadwal berikutnya; `Unavailable` dari registry kapan pun diperlakukan fatal (daemon berhenti dengan urutan berhenti biasa lalu keluar 1).
   - `Recovery(report)` dengan reason `registry_locked` (sementara, misalnya tool lain membuka file registry): `open_registry` diulang tiga kali dengan jeda 1, 2, lalu 4 detik; kalau masih gagal, lanjut seperti reason lain.
   - `Recovery(report)`: daemon masuk recovery mode (AC-18) dan tetap hidup. Subsystem `registry` `unavailable` dengan detail kode reason, `DaemonHealth.recovery` berisi laporan, langkah slot sesudahnya (rekonsiliasi fitur 10 dan 30) dilewati, tidak ada prune, dan setiap method yang tidak bertanda `allowed_in_recovery` dijawab `registry.recovery_mode` oleh dispatcher sebelum handler dipanggil. Keluar dari recovery hanya lewat restart setelah registry diperbaiki dari luar (spec 0005).
   - `StorageError`: catat lalu keluar 1.
12. Status kesehatan menjadi `ready`.

Exit code `stacklyd`: 0 berhenti normal (atau `--print-roots` berhasil), 1 gagal start, 2 argumen salah (clap), 3 daemon lain memegang lock, 75 berhenti untuk restart (`daemon.shutdown` mode `restart`, spec 0015; launchd menyalakannya lagi lewat `KeepAlive`). Perilaku sementara spec 0005 sebelum server ini ada (keluar 1 saat recovery dan flag `--roots-only`, keduanya `// provisional: F6`) dihapus di fitur ini: recovery menjadi recovery mode, dan `--roots-only` diganti `--print-roots`.

**Koneksi**: accept loop adalah satu task yang memegang listener dan semaphore `MAX_CONNECTIONS`, dan dengan `tokio::select!` juga menjalankan pemeriksaan socket setiap `SOCKET_CHECK_INTERVAL`. Koneksi di atas batas menerima `ipc.too_many_connections` (ditulis di task terpisah dengan `REJECT_WRITE_TIMEOUT`) lalu ditutup. Error accept (misalnya terlalu banyak file terbuka) dicatat, dihitung, lalu loop menunggu `ACCEPT_BACKOFF` yang berlipat dari 100 ms sampai 1 detik. Setiap koneksi punya task pembaca dan task penulis dengan antrian keluar `OUTBOUND_QUEUE_LEN`. Setelah handshake, setiap request di spawn sebagai task dengan semaphore `MAX_IN_FLIGHT_REQUESTS` per koneksi dan set `requestId` yang sedang berjalan. Reply yang hasil encode nya melebihi `CONTROL_FRAME_MAX` diganti `ipc.reply_too_large` dan dicatat sebagai error. Mengirim ke antrian keluar menunggu paling lama `WRITE_TIMEOUT`; lewat dari itu koneksi ditutup dan `ipc.write_timeout` dicatat. Client yang putus tidak membatalkan pekerjaan yang sedang berjalan; balasannya dibuang dan dicatat. Handler yang panic menghasilkan `internal.unexpected` dan baris log error.

**Dispatcher**: satu `match` atas `Call` ke method `Handlers`. Setiap method `Handlers` menerima `RequestContext { connection_id, request_id, peer: PeerInfo { uid, pid }, client: ClientInfo }` dan params nya; context diisi dari accept dan handshake koneksi itu, sehingga handler tidak pernah menebak siapa pemanggilnya (spec 0011 memakai `client.kind` dan `client.interactive` untuk memilih interaksi Keychain). Handler memanggil use case core lewat `spawn_blocking` (spec 0004) dan memetakan tipe domain atau platform ke DTO di modul `map`. Makro method mencatat tanda `allowed_in_recovery` per method; dalam recovery mode, dispatcher menjawab `registry.recovery_mode` untuk method tanpa tanda itu sebelum memanggil handler.

**Pemeriksa socket** (di dalam accept loop): `check_bound()`. `Missing` atau `Replaced`: panggil `ensure_runtime_root` (core, baru), lalu `rebind()`, naikkan `socketRebinds`, dan catat warning `socket_rebound`. Kalau rebind gagal, subsystem `ipc` menjadi `unavailable`, status `degraded`, dan percobaan diulang di pemeriksaan berikutnya.

**Berhenti**: SIGTERM atau SIGINT pertama, atau `daemon.shutdown` yang diterima (spec 0015, setelah `ShutdownAccepted` diantrekan):

1. Hentikan accept loop, lalu `unlink_if_own()` dan buang listener, sehingga client baru langsung mendapat `ipc.endpoint_missing`.
2. Status `stopping`; publish `daemon.shutting_down` dengan reason pemicunya: `signal` untuk sinyal, `requested` untuk mode `stop`, `restart_requested` untuk mode `restart`.
3. Request baru di koneksi yang terbuka mendapat `daemon.shutting_down`.
4. Tunggu request yang berjalan paling lama `SHUTDOWN_DRAIN`, sementara `OperationEngine::shutdown(OPERATION_SHUTDOWN_GRACE)` (10 detik, spec 0007) berjalan bersamaan; langkah ini selesai saat keduanya selesai atau batas yang lebih panjang habis. Lalu supervisor (spec 0010) menghentikan actor, probe, pembaca spool, dan watcher tanpa menyinyal proses instance.
5. Beri task penulis setiap koneksi waktu `SHUTDOWN_FLUSH` untuk mengirim isi antrian keluar (termasuk `daemon.shutting_down`), lalu tutup koneksi.
6. Lepas lock (isi file dibiarkan, start berikutnya menimpanya).
7. Kosongkan antrian log paling lama 1 detik, lalu keluar 0 (sinyal dan mode `stop`) atau 75 (mode `restart`).

Sinyal kedua selama itu langsung melompat ke langkah 6. Spec ini tidak menghentikan instance apa pun saat daemon berhenti; perilaku itu milik fitur 10 dan 30.

**Kesehatan**: `DaemonHealth { status: DaemonHealthStatus, subsystems: Vec<SubsystemHealth { name, status: SubsystemStatus, detail: Option<String> }>, counters: DaemonCounters, recovery: Option<RegistryRecovery> }`. `RegistryRecovery { reason: String, reportPath: String, detectedAt: Timestamp, appSchemaVersion: u32, dbSchemaVersion: Option<u32> }` (field opsional, minor; `reason` adalah kode reason recovery spec 0005) hanya terisi dalam recovery mode, supaya heartbeat GUI (spec 0013) dan `stackly daemon status` melihatnya tanpa panggilan tambahan.

- `DaemonHealthStatus` (terbuka): `starting`, `ready`, `degraded`, `stopping`. Namanya memakai awalan `Daemon` supaya tidak bentrok dengan health instance (spec 0004 dan 0012). `SubsystemStatus` (terbuka): `ok`, `degraded`, `unavailable`, `not_built`.
- Subsystem di fitur ini, dengan nama tetap: `ipc` (`unavailable` saat rebind gagal), `event_bus` (selalu `ok`; kehilangan event terlihat di counter), `log_sink` (`degraded` kalau ada baris yang dibuang atau tulis file gagal dalam 60 detik terakhir), serta `registry`, `operations`, dan `supervisor` yang bernilai `not_built` sampai fitur 5, 7, dan 10 mengambil alih nama itu. Subsystem baru ditambah oleh fitur yang pertama kali memakainya, dengan nama dan arti statusnya di spec fitur itu; yang sudah ada: `catalog` (spec 0017). Dalam recovery mode, `registry` bernilai `unavailable` dengan detail kode reason recovery, sehingga status menjadi `degraded`. `detail` adalah satu kalimat bahasa Inggris yang aman, misalnya `socket missing, retrying` atau `12 log lines dropped in the last minute`.
- Status: `starting` dan `stopping` mengikuti urutan start dan berhenti; di antaranya `degraded` kalau ada subsystem yang sudah dibangun bernilai `degraded` atau `unavailable`, selain itu `ready`. `not_built` tidak menurunkan status.
- `DaemonCounters` (semua `Count`, sejak start): `connectionsOpen`, `connectionsTotal` (setiap accept yang berhasil), `connectionsRejected` (peer ditolak, terlalu banyak koneksi, dan handshake yang gagal), `framesRejected` (setiap frame yang dijawab dengan error framing atau payload), `eventsPublished`, `eventsDropped` (jumlah `n` dari setiap lag subscriber, sehingga bisa lebih besar dari `eventsPublished`), `logLinesDropped`, `socketRebinds`.

### Log daemon

- Satu subscriber `tracing` dengan format JSON dan filter dari `STACKLY_LOG` (default `info`). Writer nya hanya memasukkan baris ke `std::sync::mpsc::sync_channel(LOG_QUEUE_LEN)` lewat `try_send`; tidak ada kode daemon yang menulis ke stderr atau file secara langsung.
- Satu thread penulis memegang stderr dan `Option<RotatingFile>`. Sink file dipasang lewat pesan kendali di channel yang sama setelah lock didapat; daemon kedua yang gagal lock tidak pernah memasangnya.
- `try_send` yang gagal karena penuh membuang baris dan menaikkan `logLinesDropped`. Tulis ke file atau stderr yang gagal juga membuang baris itu, menaikkan penghitung yang sama, dan menandai `log_sink` degraded; file dibuka ulang di baris berikutnya setelah 10 detik. Saat antrian pulih, penulis mencatat satu baris `log lines dropped` berisi jumlahnya.
- File `<logs>/daemon/stacklyd.log`, dibuka append dengan mode 0600, dan ukurannya dihitung dari ukuran file saat dibuka. Sebelum sebuah baris membuat file melewati `LOG_FILE_MAX`, penulis merotasi: hapus `stacklyd.log.4`, geser `.3` ke `.4` sampai `stacklyd.log` ke `.1`, lalu buka `stacklyd.log` baru. Rotasi hanya menyentuh file bernama persis itu di folder `logs/daemon`.
- Level: baris start, berhenti, dan rebind di `info` atau `warn`; koneksi dibuka dan ditutup di `debug`; peer ditolak di `warn`; request sukses di `debug`; request gagal di `info`, kecuali kategori `internal` di `error`.
- Field per baris (NFR-011): koneksi membawa `connection_id` (penghitung per run), `peer_pid`, `client_kind`, `client_version`; request membawa `request_id` dan `method`, dan baris penutupnya `outcome` (`ok` atau `error`), `code`, dan `duration_ms`. Operasi (fitur 7) menambah `operation_id`. Isi params, isi reply, teks error serde, dan byte frame tidak pernah dicatat di level mana pun.
- Fitur 27 boleh memakai ulang `RotatingFile` untuk log instance.

### CLI `stackly daemon status`

Output akhir dan `--json` milik fitur 8; bentuk di bawah ditandai `// provisional: F8`.

- `stackly` membaca `STACKLY_ROOT` sekali di `main` dengan aturan yang sama dengan daemon (kosong ditolak), membuat `MacosPlatform::new`, menurunkan `daemon_socket()`, dan mengambil uid dari `UserIdentity::current_user()`. Ia tidak memanggil `prepare_layout` dan tidak membuat folder.
- `Client::connect` dengan `ClientInfo { kind: cli, version: CARGO_PKG_VERSION, pid }` dan batas waktu connect plus handshake 5 detik, lalu `daemon.get_info` dan `daemon.get_health` dengan batas waktu 10 detik masing masing.
- Teks ke stdout, satu fakta per baris: status, pid, uid, versi, protokol (dengan baris `contract drift` kalau ada), `serverInstanceId`, waktu start (RFC 3339 UTC sampai fitur 8 memutuskan tampilan lokal), kesehatan dan subsystem yang tidak `ok`, keempat root, path socket, dan `rootOverride`.
- `--json` mencetak `{ "info": DaemonInfo, "health": DaemonHealth, "client": { "protocol": ProtocolVersion, "contractDigest": String, "contractDrift": bool } }` ke stdout tanpa ANSI. Error dicetak ke stderr sebagai satu kalimat plus `hint`, atau sebagai `ErrorBody` JSON kalau `--json`.
- Status kesehatan apa pun (termasuk `degraded` dan `starting`) tetap exit 0, karena perintahnya berhasil. Kalau `get_info` berhasil tetapi `get_health` gagal, info dicetak, error nya dilaporkan, dan exit mengikuti kategori error itu.
- Exit code sementara diturunkan dari kategori: `invalid_input` 2, `conflict` 3, `unauthorized` 4, `unsupported` 5, `unavailable` 6, `failed` 7, `timeout` 8, `cancelled` 9, `not_found` 2, `internal` 1. Fitur 8 menetapkan nilai akhir untuk `not_found` dan `internal`.
- `ipc.endpoint_missing` dan `ipc.connection_refused` mencetak `daemon is not running (no socket at <path>)` dengan hint dari tabel *Langkah pemulihan CLI* spec 0015 (dipakai spec 0008 *Koneksi ke daemon*), misalnya `stackly daemon start`. CLI tidak pernah menjalankan daemon sendiri.

### Codegen dan Swift

- `typeshare.toml` `[swift.type_mappings]`: `Uuid = "UUID"`, `Timestamp = "Date"`, `Revision = "UInt64"`, `Sequence = "UInt64"`, `Count = "UInt64"`, dan `SignedSafeInteger = "Int64"` (spec 0020). Newtype di `wire` tidak diberi `#[typeshare]`.
- `cargo xtask codegen` sekarang menulis: `StacklyProtocol.swift` (typeshare); `Generated/OpenEnums.swift` (untuk setiap `enum X: String, Codable` di output typeshare yang punya `case unrecognized`, satu `extension X { public init(from decoder: Decoder) throws }` yang memetakan nilai asing ke `.unrecognized`); `Generated/ContractInfo.swift` (digest dan `ProtocolVersion::CURRENT`); `Generated/ContractNames.swift` (`eventKinds`, `Call.methodName`, `Reply.methodName`, spec 0013); `crates/protocol/src/contract_digest.rs`; `protocols/schemas/*.schema.json` (`protocol-version`, `request`, `server-frame`); `protocols/error-codes.json`; dan entri baru di `protocols/contract-versions.json`. Semua file ini masuk `--check`, dan setiap file Rust atau Swift hasil generate diawali komentar `@generated by cargo xtask codegen; do not edit`.
- Ditulis tangan di `StacklyClient`: `StacklyJSON.decoder()` dan `StacklyJSON.encoder()` dengan strategi tanggal milidetik (encode `yyyy-MM-dd'T'HH:mm:ss.SSS'Z'` UTC; decode lewat `Date.ISO8601FormatStyle(includingFractionalSeconds: true)` lalu tanpa pecahan). Transport Swift milik fitur 13, dan mengikuti tiga tahap `decode_server_frame`.
- Fixture di `protocols/fixtures/ipc/`, satu file JSON per kasus, plus `index.json`: `{ "fixtures": [ { "file", "type", "expect": "roundtrip" | "decode" | "skipped" | "reject", "code"?: string, "langs"?: ["rust" | "swift"] } ] }`. `type` adalah nama tipe Rust; test Rust dan Swift masing masing punya tabel nama ke decoder, dan nama yang tidak dikenal membuat test gagal. `roundtrip`: decode, encode, decode, nilai sama. `decode`: harus terbaca (kasus kompatibilitas). `skipped`: frame event yang harus menjadi `SkippedEvent`. `reject`: harus gagal; `code` diperiksa di Rust lewat `decode_request`. `langs` membatasi kasus yang hanya berlaku di satu bahasa (misalnya kedalaman 33 hanya ditolak `decode_request` di Rust).
- Cakupan fixture minimal: setiap varian `Call`, `Reply`, `ServerFrame`, dan `EventKind`; setiap enum terbuka dengan nilai asing; field asing di `HandshakeAccepted`, `DaemonInfo`, dan `Event`; event dengan jenis asing (`skipped`); request dengan `content` sebelum `type`; error dengan semua field opsional terisi dan kosong; waktu dengan offset `+07:00` dan tanpa pecahan; UUID huruf besar; path Unicode dan berspasi di `DaemonRoots`; `Sequence` bernilai `MAX_SAFE_INTEGER` (diterima) dan satu di atasnya (ditolak); handshake major 2 dengan params lain yang rusak (`handshake_major` tetap terbaca); request yang ditolak untuk setiap kode `request.*`.
- Vektor framing di `protocols/fixtures/framing/vectors.json`: `{ "vectors": [ { "name", "hex", "chunks"?: [n], "max", "frames"?: [hex], "error"?: code } ] }`, mencakup dua frame dalam satu potongan, satu frame dipotong per byte, panjang 0, panjang `max` persis, panjang `max + 1`, dan header yang terpotong. Fitur 13 memakai vektor yang sama untuk transport Swift.

### Fuzzing

Aturan ini dipakai bersama fitur 9, 18, 20, 54, dan 63:

- Folder `fuzz/` adalah proyek cargo-fuzz dengan `[workspace]` sendiri dan `Cargo.lock` yang disimpan di git, dikecualikan dari workspace utama. `fuzz/rust-toolchain.toml` berisi nightly bertanggal yang dipin; `rust-toolchain.toml` di root tetap stable 1.99.1.
- `xtask` menyimpan `FUZZ_TOOLCHAIN` (nightly bertanggal yang sama) dan `CARGO_FUZZ_VERSION` sebagai konstanta, ditetapkan saat build seperti pin typeshare. `cargo xtask fuzz list`, `build`, `seed`, dan `run <target> [--time <detik>]` (default 300) memeriksa keduanya dan gagal dengan petunjuk `rustup toolchain install <nightly> --profile minimal` atau `cargo install --locked cargo-fuzz --version <versi>`, lalu memanggil `cargo +<nightly> fuzz` dengan `--fuzz-dir fuzz`.
- Nama target `<area>_<objek>` di `fuzz/fuzz_targets/`. Logika harness tinggal di crate pemiliknya, di modul `#[doc(hidden)] pub mod fuzzing`, supaya target dan test regresi stable memanggil fungsi yang sama.
- `cargo xtask fuzz seed` membuat ulang `fuzz/seeds/<target>/` dari fixture (untuk spec ini: byte setiap vektor framing, payload setiap fixture `Request`, dan payload setiap fixture `ServerFrame`); seed disimpan di git. Korpus hasil run di `fuzz/corpus/` dan crash di `fuzz/artifacts/` diabaikan git. Setiap input yang pernah crash disalin ke `crates/<crate>/tests/fuzz_regressions/<target>/` dan diputar ulang oleh `tests/fuzz_regressions.rs` crate itu di toolchain stable.
- Bukti "berjalan di Mac ini" untuk Done when: 5 menit per target tanpa crash, dicatat sebagai evidence manual (spec 0002). Fitur 63 menambah kampanye yang lebih panjang.
- `cargo deny check --manifest-path fuzz/Cargo.toml` juga dijalankan, karena folder itu di luar workspace. Kalau lisensi `libfuzzer-sys` memuat NCSA (dicek saat build), `NCSA` ditambahkan ke daftar `allow` di `deny.toml` dengan komentar bahwa ia hanya masuk lewat `fuzz/`.
- Target spec ini: `ipc_frame_decode` (byte acak dipotong pada batas acak ke `FrameDecoder` dengan batas 1 MiB; tidak panic, tidak ada frame melebihi batas, jumlah byte yang dipakai cocok), `ipc_request_decode` (byte acak ke `decode_request`; tidak panic, hasil `Ok` bisa di encode lalu didecode ke nilai yang sama, dan setiap penolakan memakai kode katalog dengan paling banyak 32 `fields` dan frame error yang muat di `CONTROL_FRAME_MAX`), dan sejak protokol 1.4 `ipc_server_frame_decode` (byte acak ke `decode_server_frame`; tidak panic, setiap penolakan `ipc.protocol_violation`, dan response, error, atau event yang terbaca bisa di encode lalu didecode ke nilai yang sama). `-max_len` 65 536 untuk ketiganya; jalur `TooLarge` tetap teruji lewat nilai header.

### Testkit

- `cargo_bin(package, bin) -> PathBuf` membangun bin sekali per proses test lewat `$CARGO build --locked -p <package> --bin <bin> --target-dir <workspace>/target/testkit-bin` (folder target terpisah supaya tidak berebut lock build dengan `cargo test` yang sedang berjalan), lalu mengembalikan path nya. Hanya test CLI yang butuh; test di `apps/daemon/tests/` memakai `env!("CARGO_BIN_EXE_stacklyd")`.
- `SpawnedDaemon::start(bin, root: &Path, socket: &Path, runtime_root: &Path)` menjalankan `stacklyd` dengan `STACKLY_ROOT=<root>` dan `STACKLY_LOG=debug`, mengarahkan stderr ke file di dalam root test, lalu menunggu paling lama 10 detik sampai `connect_verified` ke socket itu berhasil (bukan hanya path muncul). Ia memberi `pid()` dan `stop() -> ExitStatus`. Saat drop: SIGTERM lewat `/bin/kill -TERM`, tunggu 5 detik, lalu `Child::kill` dan `wait`, sehingga proses tidak pernah tertinggal. Setelah proses berhenti, ia menghapus runtime root milik run itu (`<darwin user temp>/stackly-<fnv8(root)>`) hanya kalau isinya kosong atau hanya folder `i/` yang kosong. Test mengambil `socket` dan `runtime_root` dari `MacosPlatform::new(Some(root))`, jadi path nya selalu sama dengan yang dihitung daemon. Karena butuh `stackly-ipc`, helper `connect_verified` dipanggil lewat closure yang diberikan test, sehingga testkit tetap tanpa dependency ke `stackly-ipc`.

### Asumsi ke spec lain

- **Assumed (spec 0005)**: revision CAS adalah `u64` yang tidak pernah melewati 2^53 − 1, mengikuti *Aturan revision*, dan dikirim sebagai `Revision`; kegagalan CAS memakai `resource.revision_mismatch` dengan `details` `expected` dan `actual`; setiap DTO snapshot resource membawa `revision`; method list berhalaman sehingga satu halaman tidak pernah melewati `CONTROL_FRAME_MAX`; buka registry dan migrasi berjalan di slot start (langkah 11) dan mengambil alih subsystem `registry`; versi schema registry ditambahkan ke `DaemonInfo` sebagai field opsional `registrySchema` (minor naik). Kode registry memakai area `registry` dan didefinisikan spec 0005 per varian `RegistryError`, termasuk `registry.recovery_mode` untuk recovery mode (AC-18); laporan recovery dikirim sebagai `DaemonHealth.recovery` (minor naik). `OperationCursor` registry tidak pernah dikirim sebagai `Timestamp` wire, karena presisinya nanodetik.
- **Assumed (spec 0007)**: `Request.idempotencyKey` adalah satu satunya pembawa idempotency key, dan spec 0006 hanya memvalidasi formatnya; spec 0007 menentukan method mana yang mewajibkannya dan arti deduplikasinya. Operasi mempublikasikan event (misalnya `operation.progress`) lewat `publish_resource` dengan resource operasi dan `revision`; error dari operasi yang gagal membawa `operationId`; method list operasi berhalaman; method dan kode memakai area `operation`; `resource.busy` dipakai untuk lock resource.
- **Assumed (spec 0008)**: exit code CLI hanya diturunkan dari `ErrorBody.category`, dengan delapan nilai dari PRD bagian 12 seperti tabel kategori; spec 0008 menetapkan exit untuk `not_found` dan `internal` dan mengganti output sementara `stackly daemon status`. Error CLI yang tidak pernah lewat wire tetap memakai `ErrorBody` dengan area `cli`.
- **Assumed (spec 0009)**: fuzz target manifest dan template mengikuti aturan *Fuzzing* di atas; bentuk `ResourceRef.id` untuk provider ditetapkan di sana (`<namespace>/<name>@<revision>#<manifest_sha256>`); kode error memakai area `manifest` dan `provider` dengan katalog di spec 0009, dan daftar issue dikirim sebagai `items` ber `kind` `issue`; aturan params tambahan lewat `Validate`.
- **Assumed (spec 0010)**: event `instance.state_changed`, `instance.health_changed`, dan `daemon.reconciling` didefinisikan di sana lewat `publish_resource` atau `publish_daemon`; rekonsiliasi berjalan di slot start dan mengambil alih subsystem `supervisor`; area `supervisor` dan `process`. Pemilik observasi readiness dan health (spec 0010, fitur 28) mempublikasikan event saat nilai efektif berubah karena basi.
- **Assumed (spec 0011)**: aturan redaksi berlaku untuk `message`, `hint`, `details`, `items`, dan `fields[].message` di `ErrorBody` (sebelum `ErrorBody::new` memotong pesan), untuk payload event, untuk setiap baris log sebelum masuk antrian penulis, dan untuk detail observasi saat dicatat; secret hanya dikirim setelah handshake di endpoint yang lolos verifikasi; bentuk field secret di params diputuskan spec 0011; area `secret` dengan semua kode store di bawahnya (`secret.store_*`), `kind` item milik spec 0011, nilai `secret` di `ResourceKind` untuk event `secret.changed`, dan field opsional `DaemonInfo.secretStore`. Spec 0011 memakai `RequestContext.client` (AC-19) untuk memilih interaksi Keychain.
- **Assumed (spec 0012)**: nilai `unrecognized` dari enum terbuka dan kategori error menjadi masukan pola state UI; `unrecognized` ditampilkan sebagai state yang tidak dikenal versi app ini, tidak pernah ditebak menjadi state lain, dan tidak pernah disamakan dengan state `unknown` yang asli.
- **Assumed (spec 0013)**: transport Swift memakai vektor framing, aturan verifikasi endpoint (path lalu peer), handshake, tiga tahap decode frame dengan `SkippedEvent`, resep snapshot plus cursor, dan `StacklyJSON` dari spec ini.
- **Assumed (spec 0015)**, dicatat 2026-10-20: method `daemon.shutdown` (`ShutdownParams`, `ShutdownMode` terbuka, `ShutdownAccepted`, `recovery allowed`) di area `daemon`; nilai `ShutdownReason::RestartRequested`; field opsional `DaemonInfo.launch` (`DaemonLaunch`, `LaunchContextKind` terbuka); kode `daemon.restart_unavailable` milik spec 0015 di area spec ini; exit code 75; field `StartConfig` dan `DaemonDeps` di *Daemon*; launch record di langkah 8a; dan verifikasi tanda tangan peer yang ditunda ke fitur 60 dan 63.
- **Assumed (spec 0017)**, dicatat 2026-10-20: area dan subsystem `catalog`; method `catalog.list_providers` dan `catalog.get_provider` (keduanya `recovery allowed`); kode `catalog.unavailable`, `catalog.provider_not_found`, `catalog.version_not_found`, dan `catalog.not_installable` dengan hint lewat `codes::catalog::hint`; `STACKLY_CATALOG_DIR`; muat catalog di slot start dan `ensure_source` setelah registry `Ready`; override `DaemonConfig.catalog` untuk test.
- **Assumed (spec 0018)**, dicatat 2026-10-20: area `runtime` dengan hint lewat `codes::runtime::hint`; method `runtime.install` (memulai operasi, `recovery refused`), `runtime.resolve` (`recovery allowed`), `runtime.list` dan `runtime.get` (`recovery refused`); event `runtime.state_changed` (varian unit, `publish_resource`); fallback hint untuk `FailureError` operasi di pemetaan `stacklyd`.
- **Assumed (spec 0020)**, dicatat 2026-10-20: newtype `SignedSafeInteger` di `wire` (aturan di *Aturan wire umum*); method `instance.get_create_form` dan `instance.preflight` (`recovery refused`); event `instance.removed`; kode baru di area pemiliknya dengan hint statis lewat `codes::<area>::hint`.
- **Assumed (spec 0022)**, dicatat 2026-10-20: area `storage` dengan method `storage.get_capacity` (`recovery allowed`) dan kode `storage.capacity_unavailable` beserta `codes::storage::hint`.
- **Assumed (spec 0002)**: evidence manual (run fuzz, pemeriksaan dua akun) dicatat lewat `cargo xtask evidence new`; kalau perintah itu belum dibangun saat milestone dijalankan, record ditulis tangan mengikuti schema spec 0002.

### API surface

Semua method butuh handshake yang sudah diterima di koneksi itu, dan koneksi itu hanya ada untuk peer dengan uid yang sama. Semua method juga bisa mendapat kode `request.*`, `internal.unexpected`, `ipc.reply_too_large`, dan `daemon.shutting_down`. Tidak ada method mutasi di fitur ini; spec 0015 menambah `daemon.shutdown` di area milik spec ini (baris terakhir tabel).

| Method | Input | Output | Error utama |
| --- | --- | --- | --- |
| `daemon.handshake` | `HandshakeParams` | `HandshakeAccepted` | `protocol.major_mismatch`, `protocol.handshake_repeated` |
| `daemon.get_info` | tidak ada | `DaemonInfo { version, protocol, contractDigest, serverInstanceId, pid, uid: Option<u32>, startedAt, roots: DaemonRoots { data, logs, cache, runtime, rootOverride }, socketPath }`, ditambah field opsional milik spec lain (minor): `registrySchema` (spec 0005), `secretStore` (spec 0011), dan `launch: DaemonLaunch { context: LaunchContextKind, label, bundlePath }` (spec 0015) | tidak ada selain di atas |
| `daemon.get_health` | tidak ada | `DaemonHealth` | tidak ada selain di atas |
| `daemon.get_capabilities` | tidak ada | `Capabilities { platform: Vec<PlatformCapabilityStatus { capability, supported, reason: Option<String> }>, methods: Vec<String>, eventKinds: Vec<String> }` | tidak ada selain di atas |
| `events.subscribe` | `SubscribeParams { since, kinds, resource }` | `Subscribed { subscriptionId, head }`, lalu frame `event` | `events.too_many_subscriptions` |
| `events.unsubscribe` | `UnsubscribeParams { subscriptionId }` | unit | `events.unknown_subscription` |
| `daemon.shutdown` (spec 0015, `recovery allowed`, dilayani juga saat `starting`) | `ShutdownParams { mode: ShutdownMode }` (`stop`, `restart`, enum terbuka; `Validate` menolak `unrecognized`) | `ShutdownAccepted { serverInstanceId, mode }`, lalu urutan berhenti | `daemon.restart_unavailable`, `daemon.shutting_down`, `request.invalid_params` |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| handshake (client) | `protocol` | `ProtocolVersion::CURRENT` (Swift: `ContractInfo.protocolVersion`) |
| handshake (client) | `contractDigest` | `CONTRACT_DIGEST` (Swift: `ContractInfo.digest`), hasil `cargo xtask codegen` |
| handshake (CLI) | `client.kind`, `version`, `pid` | konstanta `cli`, `CARGO_PKG_VERSION`, `std::process::id()` |
| handshake (CLI) | `client.interactive` | true kalau stdin dan stderr TTY dan tanpa mode noninteractive (spec 0011); GUI selalu true |
| semua request | `RequestContext` | ID koneksi dan peer dari accept, `ClientInfo` dari handshake koneksi itu, `requestId` dari request |
| handshake (daemon) | major client sebelum decode params | `json::handshake_major` |
| handshake (daemon) | `serverInstanceId` | `Uuid::now_v7()` sekali saat start |
| handshake (daemon) | `daemon.version`, `daemon.pid` | `stackly_core::daemon_info().version`, `std::process::id()` |
| handshake (daemon) | `contractDrift` | dihitung: major dan minor sama, digest berbeda |
| handshake (daemon) | `limits` | konstanta `stackly-protocol::limits` |
| handshake, subscribe | `head` | `serverInstanceId` dan sequence terakhir di event bus |
| semua request | `requestId` | client; `stackly-ipc` membuat UUID v7 lowercase |
| semua request | `idempotencyKey` | client (arti di spec 0007) |
| decode params | `fields[].path` | `serde_path_to_error`, relatif terhadap params, atau aturan `Validate` |
| decode params | `fields[].code`, `fields[].message` | set kode tetap dan teks tetap per kode di `json` |
| accept | uid dan pid peer | `PeerCredSource` (default `UnixStream::peer_cred()` tokio) |
| accept, connect | uid harapan | `UserIdentity::current_user()` (spec 0004), `UserId::Unix(uid)`; `ConnectPolicy` di client |
| verifikasi endpoint | jenis, pemilik, mode path dan folder | `std::fs::symlink_metadata` dan `MetadataExt` |
| verifikasi endpoint | `details.reason` | daftar alasan tetap di *Envelope error* |
| error IO client | `details.op`, `details.kind` | operasi yang gagal dan `std::io::ErrorKind` dalam snake_case |
| `get_info` | `startedAt` | `Clock::now()` saat start |
| `get_info` | `roots`, `socketPath` | `DirectoryLayout` dari `RootSet` resolved (bukan canonical) |
| `get_info` | `rootOverride` | ada atau tidaknya `STACKLY_ROOT` saat start |
| `get_info` | `uid` | `UserIdentity::current_user()`; null kalau bukan `Unix` |
| `get_health` | `recovery` | `RecoveryReport` dari `open_registry` (spec 0005) dipetakan ke `RegistryRecovery`: kode reason, path laporan, `detectedAt`, dan dua versi schema; null kalau registry `Ready` |
| prune berkala | interval | konstanta `REGISTRY_PRUNE_INTERVAL` 24 jam di library `stacklyd`; kebijakan dari `RetentionPolicy::DEFAULT` (spec 0005) |
| error | `items` | tempat error dibuat, hanya `kind` dan kunci yang diizinkan katalog, lewat redaksi spec 0011 |
| `get_health` | `status`, `subsystems`, `detail` | state daemon, status tiap subsystem, dan aturan di *Kesehatan* |
| `get_health` | `counters` | penghitung atomik di state daemon, dengan aturan di *Kesehatan* |
| `get_capabilities` | `platform` | `PlatformInfo::capabilities()` dipetakan lewat kode `PlatformCapability` |
| `get_capabilities` | `methods`, `eventKinds` | `Call::METHODS`, `EventKind::KINDS` |
| subscribe | `subscriptionId` | penghitung per koneksi, mulai 1 |
| event | `sequence`, `at` | penghitung bus, `Clock::now()` saat publish |
| event | `resource`, `revision` | argumen `publish_resource` dari pemilik resource |
| gap | `reason`, `sequence` | logika subscribe dan `RecvError::Lagged(n)` |
| error | `category`, `retryable`, kunci `details` yang boleh | katalog `codes` |
| error | `message`, `hint`, nilai `details` | tempat error dibuat, lewat redaksi spec 0011 |
| error | `rule` | `rule()` error domain (spec 0004) dipetakan di `map` |
| error | `operationId` | operasi yang gagal (spec 0007) |
| lock file | isi | pid, `serverInstanceId`, `startedAt`, versi daemon ini |
| daemon kedua | info pemilik | isi lock file, paling banyak 4 KiB |
| log | path file | `DirectoryLayout::daemon_log_dir()` ditambah `stacklyd.log` |
| log | filter | `STACKLY_LOG`, default `info` |
| CLI | path socket | `MacosPlatform::new(STACKLY_ROOT)` lalu `daemon_socket()` |
| CLI | blok `client` di `--json` | `ProtocolVersion::CURRENT`, `CONTRACT_DIGEST`, `contractDrift` dari `HandshakeAccepted` |
| CLI | exit code | kategori error lewat tabel sementara di `apps/cli` |
| digest kontrak | nilai | SHA-256 atas bentuk ternormalisasi schema dan katalog, urut nama |
| catatan versi | entri | `protocols/contract-versions.json`, ditulis `cargo xtask codegen` |
| fuzz | toolchain dan versi | konstanta `FUZZ_TOOLCHAIN` dan `CARGO_FUZZ_VERSION` di xtask, ditetapkan saat build |
| fuzz | seed | `cargo xtask fuzz seed` dari fixture |

### Key invariants

- Tidak ada request yang diproses dari peer yang uid nya berbeda, dan tidak ada request selain handshake yang diproses sebelum handshake diterima.
- Client tidak menulis satu byte pun ke socket yang gagal `verify_path` atau `verify_peer`.
- Bagian yang dibekukan (amplop request, `protocol` di handshake, amplop frame error, dan field inti `ErrorBody`) sama di semua major.
- Hanya satu daemon per data root yang memegang lock; daemon yang tidak memegang lock tidak menulis apa pun selain ke stderr.
- Socket hanya dihapus kalau berjenis socket dan pemanggilnya memegang lock (saat bind), atau kalau device dan inode nya milik daemon ini (saat berhenti). Path lain tidak pernah dihapus. Path akhir socket tidak pernah ada dengan mode selain 0600.
- Setiap error di wire memakai kode dari katalog, dengan kategori, `retryable`, dan kunci `details` dari katalog. `retryable` true hanya kalau request dijamin tidak berefek.
- Sequence event per run naik tepat 1 dan tidak pernah dipakai ulang; client tidak pernah menerima event yang sama dua kali di satu subscription; event tentang resource selalu membawa revision.
- Publish event, penulisan log, dan accept tidak pernah menunggu client yang lambat; setiap kehilangan dihitung dan terlihat di `get_health`.
- Daemon tidak pernah membuka socket TCP atau UDP.
- Params, reply, teks error serde, dan byte frame tidak pernah masuk log maupun pesan error.
- Frame error untuk setiap penolakan decode request selalu muat di `CONTROL_FRAME_MAX`, apa pun inputnya: `fields` paling banyak 32, `path` paling panjang 256 scalar Unicode, `message` paling panjang 200 karakter, dan `details` hanya nilai pendek dari daemon. Error dari handler bergantung pada fallback `ipc.reply_too_large` (lihat Follow-up).
- File hasil generate tidak pernah diubah atau digabung tangan; `cargo xtask codegen --check` menjaganya, dan digest yang berubah selalu disertai versi baru.

### Security model

- Batas kepercayaan adalah satu akun user OS (BR-012). Spec ini tidak mengklaim melindungi dari proses jahat dengan uid yang sama atau dari root.
- Lapisan 1: socket ada di runtime root 0700 di temp dir privat user (spec 0004), dengan mode 0600 sejak path akhirnya muncul, jadi user lain tidak bisa mencapai path nya.
- Lapisan 2: daemon memeriksa uid peer pada setiap koneksi sebelum membaca frame, dan menolak kalau kredensial tidak terbaca (gagal tertutup).
- Lapisan 3: client memeriksa path socket dan uid server sebelum mengirim apa pun, sehingga socket palsu milik user lain tidak pernah menerima credential atau mutasi.
- Tidak ada listener TCP atau UDP, tidak ada password atau token bersama, dan tidak ada IPC jarak jauh (REQ-047). Dijaga test `lsof` dan pindaian `check-deps`.
- Ukuran frame, kedalaman JSON, jumlah koneksi, request berjalan, subscription, dan ukuran reply dibatasi sebelum alokasi besar terjadi (VAL-017, NFR-004). Codec dan decode request di fuzz (NFR-009).
- Log dan pesan error tidak memuat params, isi frame, atau teks error serde, dan teks error melewati redaksi spec 0011.
- `STACKLY_LOG` hanya mengatur level log dan tidak bisa menyalakan pencatatan params.
- Tidak ada `unsafe` baru: kredensial peer lewat tokio, lock lewat `File::try_lock` milik std, mode socket lewat `chmod` dan `rename` milik std.
- Verifikasi tanda tangan kode `stacklyd` oleh client (audit token) di luar spec ini; spec 0015 *Verifikasi tanda tangan peer* menundanya ke fitur 60, dengan review di fitur 63, dan mencatat rancangannya.

### Configuration required

- `STACKLY_ROOT` (sudah ada, spec 0004): kini juga dibaca `stackly` sekali saat start, dengan aturan yang sama.
- `STACKLY_LOG` (baru, opsional): filter `tracing` untuk `stacklyd`, misalnya `info` atau `stacklyd=debug`; default `info`; nilai tidak valid membuat daemon keluar 1. Hanya dibaca `main` `stacklyd`.
- `STACKLY_CATALOG_DIR` (spec 0017, opsional): path absolut ke folder catalog hasil `cargo xtask catalog build`, untuk development dan binary test; dibaca sekali oleh `main` `stacklyd`; nilai tidak valid membuat daemon keluar 1.
- `XPC_SERVICE_NAME` (spec 0015): diset launchd untuk job nya, bukan pengguna; dibaca sekali oleh `main` `stacklyd` bersama parent pid untuk menentukan konteks launch.

### Critical test scenarios

Test yang butuh handler lambat, banyak event, atau drain berjalan di dalam proses lewat `Daemon::start` dengan `Handlers` yang dibungkus test dan `bus()` untuk publish; test lock, sinyal, `lsof`, rebind, CLI, dan kinerja menjalankan binary `stacklyd` sungguhan.

- Handshake lengkap: client minor 0 dan minor 99 dengan major 1 diterima; major 2 dengan params lain yang rusak tetap mendapat `protocol.major_mismatch`, dan request yang dikirim di belakangnya tidak pernah dijawab; frame pertama `daemon.get_info` atau method tidak dikenal mendapat `protocol.handshake_required`; JSON rusak sebagai frame pertama mendapat `request.malformed` lalu ditutup; header frame pertama tanpa payload lalu diam mendapat `ipc.handshake_timeout` dengan jam tokio yang dijeda. Memverifikasi **AC-1**.
- Peer ditolak: listener dengan uid harapan `getuid() + 1` menolak client sungguhan dengan `ipc.peer_rejected` sebelum frame dibaca; `PeerCredSource` yang gagal juga menolak; client yang tidak membaca frame penolakan tidak menahan accept loop. Memverifikasi **AC-2**.
- Error yang dibatasi (protokol 1.4): request subscribe sedikit di bawah 1 MiB dengan 349 000 kind kosong mendapat `request.invalid_params` dengan 32 `fields` dan `details.omitted`, request dan params posisional mendapat `request.malformed` dan `request.invalid_params`, lalu koneksi masih melayani `daemon.get_info` (`apps/daemon/tests/server/limits.rs`). Memverifikasi **AC-7**.
- Endpoint palsu: socket bermode 0666, socket di folder 0755, symlink ke socket asli, dan file biasa di path socket masing masing ditolak `verify_path` dengan `reason` yang benar; `ConnectPolicy` dengan `peer_uid` berbeda membuat `verify_peer` menolak daemon sungguhan tanpa satu byte tertulis (listener test menghitung nol byte); path kosong memberi `ipc.endpoint_missing`; socket basi memberi `ipc.connection_refused`; folder tanpa izin baca memberi `ipc.io_error` atau `ipc.endpoint_untrusted` sesuai error OS nya. Memverifikasi **AC-3**.
- Dua daemon: daemon kedua dengan `STACKLY_ROOT` yang sama keluar 3, stderr menyebut pid dan `serverInstanceId` daemon pertama, isi lock file dan inode socket sama, inode file log sama, dan tidak ada baris `daemon.lock_held` atau pid daemon kedua di file log; `stackly daemon status` lalu menampilkan pid daemon pertama. Memverifikasi **AC-4**, **AC-13**.
- Tanpa TCP: `lsof -nP -a -p <pid> -i` kosong saat ada tiga client tersambung, sedangkan `lsof -nP -a -p <pid> -U` menampilkan socket daemon; crate fixture yang memakai `std::net::TcpListener` di `apps/daemon` membuat `check-deps` gagal dengan file dan baris. Memverifikasi **AC-5**.
- Framing: semua vektor; frame 1 MiB persis diterima dan 1 MiB plus 1 ditolak saat header terbaca (kapasitas buffer decoder tidak pernah melewati batas); header terpotong lalu diam 31 detik menghasilkan `ipc.frame_stalled` dengan jam yang dijeda. Memverifikasi **AC-6**.
- Batas payload: setiap kode `request.*`, termasuk request dengan `content` sebelum `type` yang tetap memberi path field yang benar dan teks pesan yang tidak memuat nilai input (`"secret-sentinel"`); request ke 33 dengan handler yang ditahan; subscription ke 9; koneksi ke 65; koneksi tetap hidup setelah setiap error payload. Memverifikasi **AC-7**.
- Envelope dan katalog: test katalog (pola, tidak ganda, area, kunci `details`), pesan 300 karakter dipotong ke 200, setiap fixture error di Rust dan Swift, dan setiap error di test integrasi dicek terhadap katalog. Memverifikasi **AC-8**.
- Event: unit test bus untuk replay tanpa duplikat dan celah, `cursor_expired` setelah 5000 event, dan `overflow` untuk receiver yang tidak membaca, dengan publish yang selesai tanpa await walau subscriber tertahan; test di dalam proses untuk filter (gap dan `daemon.shutting_down` lolos filter) dan urutan `Subscribed` sebelum event; test binary untuk `server_restarted` setelah daemon direstart. Memverifikasi **AC-9**.
- Kompatibilitas: fixture minor lebih baru terbaca di Rust dan Swift, nilai enum asing menjadi `unrecognized`, item error dengan `kind` asing tetap terbaca, event asing menjadi `SkippedEvent`, handshake dengan digest berbeda memberi `contractDrift: true`, dan `--check` gagal untuk digest yang berubah tanpa versi baru. Memverifikasi **AC-10**.
- Siklus hidup: file biasa di path socket membuat daemon keluar 1 tanpa menghapus file itu; socket basi diganti; `lstat` path akhir selama start tidak pernah melihat mode selain 0600; `get_health` saat slot start yang ditahan (di dalam proses) melaporkan `starting`; menghapus socket dan runtime root saat daemon berjalan membuat client bisa tersambung lagi dalam 12 detik dan `socketRebinds` bernilai 1; SIGTERM saat ada request yang ditahan membuat client baru langsung mendapat `ipc.endpoint_missing`, subscriber menerima `daemon.shutting_down`, request baru ditolak, proses keluar 0 dalam 7 detik, dan daemon baru bisa langsung mengambil lock. Memverifikasi **AC-11**.
- Log: rotasi dengan ambang kecil di unit test menghasilkan tepat 5 file dengan urutan benar; antrian penuh membuang baris tanpa menahan pemanggil dan `logLinesDropped` naik; stderr yang berupa pipe yang tidak dibaca tidak menahan request; integration test memastikan baris request membawa `request_id` dan tidak memuat penanda params (`"params-sentinel"`). Memverifikasi **AC-12**.
- Status: `stackly daemon status` dan `--json` terhadap daemon sungguhan (termasuk blok `client`); daemon mati memberi exit 6 dan menyebut path socket; endpoint palsu exit 4; server test kecil dengan major berbeda exit 5; status `degraded` tetap exit 0; `stackly` tidak membuat file apa pun di root test (daftar isi root sebelum dan sesudah sama). Memverifikasi **AC-13**.
- Fuzz: dua target 5 menit tanpa crash, seed dari `cargo xtask fuzz seed`, dan test regresi stable lulus. Memverifikasi **AC-14**.
- Kinerja: 500 panggilan `daemon.get_info` dengan p95 paling lama 20 ms, p50 dan p95 dicatat. Memverifikasi **AC-15**.
- Struktur: `cargo check --target aarch64-pc-windows-msvc -p stackly-protocol`, `cargo xtask check-deps` (termasuk larangan bergantung pada `stacklyd`), dan test capability `MacosPlatform`. Memverifikasi **AC-16**.
- Print roots: `--print-roots` dengan override yang diterima, override kosong, relatif, atau tidak ada, dan tanpa override dengan `HOME` palsu menghasilkan baris dan exit code yang benar, dan isi root test serta `HOME` palsu tetap kosong. Memverifikasi **AC-17**.
- Recovery mode: root test dengan registry yang `user_version` nya lebih baru dari app membuat daemon tetap hidup; `get_health` `degraded` dengan `registry` `unavailable`, `get_info.recovery` terisi, method tanpa tanda `allowed_in_recovery` mendapat `registry.recovery_mode`, isi file registry tidak berubah, dan tidak ada prune; registry yang dikunci proses lain dicoba ulang tiga kali sebelum recovery; registry yang sehat di root lain menjalankan prune sekali saat start. Memverifikasi **AC-18**.
- Context: handler test yang dibungkus menerima `RequestContext` dengan uid dan pid peer, `client.kind` `cli`, dan `interactive` false saat client tidak mengirimnya serta true saat mengirim true; baris log request mencatat keduanya. Memverifikasi **AC-19**.
- Observasi di event: client resep snapshot plus cursor menerapkan event observasi yang revision nya sama dengan snapshot dan waktu observasinya sama atau lebih baru, per jenis (health yang lebih lama tidak tertolak oleh readiness yang lebih baru), dan mengabaikan yang lebih lama. Memverifikasi **AC-9**.
- Dua akun nyata (manual, native): akun kedua di Mac ini tidak bisa membuka socket akun pertama (izin ditolak di path), dan daemon akun kedua berjalan dengan socket dan root sendiri. Evidence manual `NOT_RUN` sampai dijalankan, `BLOCKED` kalau belum ada akun kedua. Memverifikasi **AC-2** (AC-079).

## Build plan

Urutan mengikuti pendekatan Journey: jalur yang dibangun lengkap adalah "client di Mac ini tersambung dengan aman ke satu satunya daemon milik user lalu melihat statusnya", termasuk semua jalur gagal. Kontrak dan codec dibangun lebih dulu karena setiap bagian lain bicara dengannya; transport, lock dan log, serta fuzzing lalu berjalan berdampingan; daemon server dan CLI menutup jalur itu. Setiap test yang membuktikan ID PRD memakai guard evidence spec 0002 (langkah 1 spec 0002 sudah dibangun).

Milestone yang bisa berjalan sebagai sesi paralel: setelah **Milestone 1** selesai, **Milestone 2**, **Milestone 3**, dan **Milestone 5** bisa berjalan bersamaan (masing masing di folder sendiri). **Milestone 4** butuh Milestone 1, 2, dan 3. Sesi paralel yang sama sama mengubah `Cargo.toml` dan `Cargo.lock` di root diselesaikan saat merge dengan menggabungkan entri lalu menjalankan `cargo build` untuk menulis ulang lock file; file hasil generate diselesaikan dengan `cargo xtask codegen`.

**Milestone 1: Kontrak dan codec** (`crates/protocol`, `xtask/src/codegen.rs`, `typeshare.toml`, `protocols/`, `clients/swift`)

1. Dependency protokol, `ProtocolVersion::CURRENT` 1.0 beserta fixture dan test Swift lamanya, newtype `Revision`, `Sequence`, `Count`, dan `Timestamp` milidetik, `ResourceRef` dan `ResourceKind`, serta `limits`. Memenuhi **AC-10**, **AC-16**.
2. `ErrorCategory`, `ErrorBody` (pemotongan pesan, builder, `with_detail`, `with_items`), `FieldError`, `ErrorDetail`, `ErrorItem` dan `ItemSpec`, katalog `codes` per area dengan semua kode spec ini, kunci `details`, dan `items` nya, `AREAS`, `catalog()`, dan test katalog. Memenuhi **AC-8**.
3. DTO pesan dengan tabel `methods!` (`Call` dan `Reply` ditulis tangan): `Request`, `Call`, `Reply`, `ServerFrame`, `Response`, `ErrorFrame`, handshake, `DaemonInfo`, `DaemonHealth`, `Capabilities`, event (`Event`, `EventKind`, `Gap`, `ShuttingDown`, `EventCursor`, `SubscribeParams`, `Subscribed`, `UnsubscribeParams`, `EventProbe`), enum terbuka dengan `Unrecognized`, `Call::METHODS`, dan `EventKind::KINDS`. Memenuhi **AC-1**, **AC-9**, **AC-10**.
4. Codec `frame`, lalu di `json`: cek kedalaman, `decode_request` bertahap dengan params dari `content` mentah, trait `Validate` dengan set kode field tetap, `handshake_major`, `decode_server_frame` dengan `DecodedFrame`, dan modul `fuzzing` berisi harness kedua target. Memenuhi **AC-1**, **AC-6**, **AC-7**, **AC-10**.
5. Codegen: mapping `typeshare.toml`, schema `request` dan `server-frame`, `error-codes.json`, digest kontrak ternormalisasi (Rust dan Swift), `contract-versions.json` dengan `--amend`, `OpenEnums.swift`, dan `--check` untuk semuanya. Memenuhi **AC-8**, **AC-10**.
6. Fixture `protocols/fixtures/ipc/` dengan `index.json` dan vektor `protocols/fixtures/framing/vectors.json`, plus test Rust yang membaca keduanya. Memenuhi **AC-6**, **AC-7**, **AC-8**, **AC-10**.
7. Swift: `StacklyJSON`, test fixture dari `index.json` (termasuk `skipped` lewat `EventProbe`), dan test enum terbuka; `swift test --package-path clients/swift` lulus. Memenuhi **AC-8**, **AC-10**.

**Milestone 2: Transport `stackly-ipc`** (`crates/ipc`, `xtask/src/check_deps.rs`, `xtask/Cargo.toml`, root `Cargo.toml`)

8. Crate baru dengan dependency dan lints, aturan `check-deps` untuk `stackly-ipc` dan untuk larangan bergantung pada `stacklyd`, dan pindaian nama socket TCP atau UDP (dengan `syn` kalau belum ada) di `crates/ipc`, `apps/daemon`, dan `apps/cli`, dengan test crate fixture yang melanggar. Memenuhi **AC-5**, **AC-16**.
9. `Listener` (bind lewat nama sementara, `chmod`, dan `rename`; accept dengan `PeerCredSource` dan penolakan yang ditulis di task terpisah; `check_bound`, `rebind`, `unlink_if_own`) dan `FramedConnection` (`READ_CHUNK`, batas, deadline, stall, frame error sebelum menutup). Memenuhi **AC-2**, **AC-6**, **AC-11**.
10. `verify_path`, `verify_peer`, `ConnectPolicy`, `connect_verified`, pemetaan error OS, `Client` (handshake, `call`, pembacaan frame tertunda saat tulis gagal, `subscribe`, `EventStream` dengan `SkippedEvent`), dan `IpcError::to_error_body`. Memenuhi **AC-3**, **AC-8**.
11. Integration test di `crates/ipc/tests/` dengan socket di folder run `TestRoot`: uid harapan yang disuntikkan di kedua sisi, `PeerCredSource` yang gagal, endpoint palsu, file dan symlink di path socket, mode path akhir selama bind, frame kosong, terlalu besar, terpotong, dan terhenti dengan jam yang dijeda. Memenuhi **AC-2**, **AC-3**, **AC-6**.

**Milestone 3: Library daemon, lock, dan log** (`apps/daemon`, `crates/core/src/layout.rs`, `crates/testkit`)

12. Core `ensure_runtime_root`, berbagi logika dengan `ensure_instance_run_dir`, dengan test di `FakePlatform`. Memenuhi **AC-11**.
13. `stacklyd` menjadi library plus bin dengan `main.rs` tipis; `--print-roots`; `apps/daemon/tests/root_override.rs` dipindah ke `--print-roots`, termasuk test home directory tanpa override. Memenuhi **AC-17**.
14. Modul `lock`: `DaemonLock` (`O_NOFOLLOW`, cek file biasa, `try_lock`, tulis dan baca info pemilik, pesan `daemon.lock_held`). Memenuhi **AC-4**.
15. Modul `log_sink`: thread penulis tunggal untuk stderr dan file, pesan kendali pasang sink, `RotatingFile` 20 MiB kali 5 file, antrian berbatas dengan penghitung drop, kebijakan tulis gagal, filter `STACKLY_LOG`; unit test dengan ambang kecil dan stderr yang tidak dibaca. Memenuhi **AC-12**.
16. Urutan start langkah 1 sampai 8, lalu menunggu SIGTERM atau SIGINT dan keluar 0; exit code 0, 1, 2, 3. `SpawnedDaemon` di testkit dan integration test daemon kedua di `apps/daemon/tests/` (keluar 3, pemilik disebut, tidak ada yang berubah). Memenuhi **AC-4**, **AC-12**.

**Milestone 4: Daemon server dan `stackly daemon status`** (`apps/daemon`, `apps/cli`, `crates/testkit`, `crates/platform-macos`)

17. Event bus (`publish_resource`, `publish_daemon`, sequence, ring, broadcast, subscribe dengan replay, gap, dan filter, penghitung) dengan unit test. Memenuhi **AC-9**.
18. Server: urutan start langkah 9 sampai 12 dengan slot start kosong dan status `starting`, accept loop yang memegang listener (batas koneksi, backoff, pemeriksa socket di `select!`), task koneksi (fase handshake dengan `handshake_major`, loop request, batas request berjalan, `requestId` ganda, `ipc.reply_too_large`, `WRITE_TIMEOUT`), trait `Handlers` dengan `RequestContext` dan dispatcher keenam method, panic menjadi `internal.unexpected`, `Daemon::start` untuk test di dalam proses, dan field serta level log per koneksi dan request. Memenuhi **AC-1**, **AC-7**, **AC-8**, **AC-11**, **AC-12**, **AC-19**.
18a. Slot start registry dan recovery mode (butuh milestone 1 spec 0005 di main; kalau belum ada saat milestone ini dibangun, langkah ini dikerjakan sesi yang menggabungkan keduanya): `open_registry` di slot start, `Ready` dengan prune awal dan task `REGISTRY_PRUNE_INTERVAL`, `Recovery` dengan percobaan ulang `registry_locked`, tanda `allowed_in_recovery` di makro method, recovery mode di dispatcher, dan `DaemonHealth.recovery`, `Unavailable` sebagai fatal, dan penghapusan `--roots-only` serta exit 3 sementara milik spec 0005. Memenuhi **AC-18**.
19. Pemeriksa socket dengan `ensure_runtime_root` dan `rebind`, urutan berhenti lengkap (unlink lebih dulu, publish, drain, flush), dan capability `ipc_endpoint` supported di `MacosPlatform`. Memenuhi **AC-11**, **AC-16**.
20. `stackly daemon status [--json]`: `STACKLY_ROOT`, path socket, `Client::connect`, `get_info` dan `get_health`, output teks dan JSON dengan blok `client`, exit code sementara; `cargo_bin` di testkit. Memenuhi **AC-13**.
21. Integration test di dalam proses dan dengan binary di `apps/daemon/tests/` serta `apps/cli/tests/`: matriks handshake, batas, event (filter, urutan, `server_restarted`), rebind setelah socket dihapus, SIGTERM, `lsof` dengan kontrol positif, dua daemon dengan status, dan kinerja p95. Memenuhi **AC-1**, **AC-4**, **AC-5**, **AC-7**, **AC-9**, **AC-11**, **AC-13**, **AC-15**.
22. Evidence manual native: pemeriksaan dua akun (AC-079), dan pengamatan kapan macOS 27 membersihkan `_CS_DARWIN_USER_TEMP_DIR` (Follow-up spec 0004 untuk fitur 6). Memenuhi **AC-2**.

**Milestone 5: Fuzzing** (`fuzz/`, `xtask`, `crates/protocol/tests/`, `deny.toml`)

23. Proyek `fuzz/` dengan `[workspace]` sendiri, `rust-toolchain.toml` nightly bertanggal, `Cargo.lock`, `exclude` di root, `.gitignore` untuk `fuzz/corpus/` dan `fuzz/artifacts/`, `cargo deny check` untuk manifest fuzz, dan `NCSA` di `deny.toml` kalau perlu. Memenuhi **AC-14**.
24. Target `ipc_frame_decode` dan `ipc_request_decode` yang memanggil `stackly_protocol::fuzzing`. Memenuhi **AC-14**.
25. `cargo xtask fuzz list`, `build`, `seed`, dan `run` dengan pin `FUZZ_TOOLCHAIN` dan `CARGO_FUZZ_VERSION`, seed dari fixture, plus `crates/protocol/tests/fuzz_regressions.rs` di toolchain stable. Memenuhi **AC-14**.
26. Jalankan tiap target 5 menit di Mac ini dan catat evidence manual. Memenuhi **AC-14**.

**Milestone 6: Protokol 1.4, temuan review fitur 6** (`crates/protocol`, `crates/ipc/src/error.rs`, `xtask`, `protocols/`, `fuzz/`, `clients/swift`)

27. Batas `fields` dan `path` di `ErrorBody` dengan `details.omitted`, `Validate` subscribe yang hanya memeriksa 32 entri pertama, objek JSON wajib di setiap tahap decode request dan frame daemon, `Timestamp::from_jiff` yang aman sampai `jiff::Timestamp::MAX`, `read` dan `write` ke `ipc.connection_closed`, digest dengan `CONTRACT_LIMITS` dan daftar tanpa urutan, `retryable` di bagian yang dibekukan, `CURRENT` 1.4, fixture, test kontrak Rust serta Swift, dan test wire daemon untuk daftar `kinds` raksasa dan request posisional. Memenuhi **AC-7**, **AC-8**, **AC-10**.
28. Target `ipc_server_frame_decode` dengan seed dari fixture `ServerFrame`, asersi ukuran frame error di harness `ipc_request_decode`, lalu run 5 menit dengan evidence manual. Memenuhi **AC-14**.

## Consequences

**Positive**:

- Satu kontrak untuk semua fitur: spec 0005, 0007 sampai 0011, dan 0013 tinggal menambah method, event, kode, dan aturan `Validate` di area mereka, dan exit code CLI langsung mengikuti kategori.
- Client lama tetap jalan terhadap daemon dengan minor lebih baru untuk field, nilai enum terbuka, dan jenis event baru; client dari major berapa pun selalu menerima `protocol.major_mismatch` yang terbaca; kelalaian menaikkan versi tertangkap saat codegen dan terlihat lewat digest saat berjalan (RISK-09).
- Reconnect GUI dan CLI murah dan benar: cursor di memori ditambah revision resource cukup untuk menyusul tanpa snapshot penuh di reconnect singkat, dan gap selalu eksplisit.
- Daemon yang lambat, client yang macet, dan stderr yang tersumbat tidak bisa saling menahan, dan setiap kehilangan terlihat di `get_health` (NFR-004).
- Tidak ada `unsafe` baru dan tidak ada listener jaringan, jadi permukaan review keamanan IPC kecil dan sebagian besar diuji fuzz (NFR-009).
- Slot start memberi fitur 5, 10, dan 30 tempat yang jelas untuk migrasi dan rekonsiliasi, dengan status `starting` yang terlihat client.

**Negative / tradeoffs**:

- Major naik untuk setiap perubahan yang merusak, bahkan sebelum stabil, jadi saat aplikasi rilis stabil angka major protokol mungkin sudah lebih dari 1. Itu hanya angka, tetapi bisa membingungkan pembaca luar.
- Setiap perubahan kontrak menaikkan versi di commit yang sama, sehingga cabang paralel yang sama sama mengubah protokol hampir pasti bentrok di `CURRENT`, file hasil generate, dan `contract-versions.json`, dan merge nya butuh codegen ulang.
- Cursor hilang saat daemon restart, jadi setiap restart memaksa semua client mengambil snapshot penuh. Untuk 100 instance ini masih di dalam NFR-002, tetapi menjadi beban kalau restart sering.
- Enum terbuka butuh varian `Unrecognized` di Rust dan file Swift tambahan hasil xtask; UI harus menangani `unrecognized` di setiap tempat enum itu tampil, terpisah dari state `unknown` yang asli.
- Typeshare dibatasi: tanpa `u64` biasa, tanpa `BTreeMap`, dan enum berdata tidak punya jalur `unrecognized` bawaan, jadi daftar di error memakai `ErrorItem` berbasis string, bukan enum. Aturan wire di atas harus dipatuhi setiap fitur, dan pelanggarannya baru ketahuan saat codegen.
- Pesan error untuk params yang salah tipe sengaja umum (tanpa teks serde), jadi debugging client kadang butuh melihat `fields[].path` dan schema, bukan pesan.
- Nightly bertanggal untuk fuzzing adalah toolchain kedua yang harus dipasang dan dinaikkan dengan sengaja; folder `fuzz/` tidak ikut `cargo fmt`, clippy, atau hook pre commit workspace.
- Penulis log berotasi buatan sendiri adalah kode yang harus dirawat sendiri, termasuk kasus disk penuh.
- `stacklyd` menjadi library plus bin dengan trait `Handlers`, sedikit lebih banyak struktur demi test di dalam proses.
- Test CLI membangun `stacklyd` sekali lagi di `target/testkit-bin`, jadi run test pertama lebih lama.
- Pemeriksaan dua akun sungguhan butuh akun kedua di Mac ini dan tetap manual.
- Satu kalimat pesan error berbahasa Inggris tampil apa adanya di CLI sampai ada lokalisasi; GUI sebaiknya memakai kode, bukan pesan.

**Neutral**:

- tokio masuk workspace di fitur ini, dipakai `stacklyd`, `stackly`, dan `stackly-ipc`.
- Dependency baru: `serde_path_to_error` (protokol), fitur `raw_value` di `serde_json`, fitur `env-filter` di `tracing-subscriber`, `libc` di `stacklyd` hanya untuk konstanta, `syn` dan `proc-macro2` di xtask kalau belum ada, dan `libfuzzer-sys` di proyek `fuzz/`.
- `ProtocolVersion` scaffold 0.1 hilang; langkah verifikasi spec 0001 yang menyebut fixture `{"major": 0, "minor": 1}` menjadi catatan sejarah.
- `stackly` mulai membaca `STACKLY_ROOT` di fitur ini, bukan baru di fitur 8.
- Test override root yang lama kini memakai `stacklyd --print-roots`.

## Follow-up

- [x] `/architect` update spec 0001: tandai baris Fitur 6 di *Batas keputusan* sudah diputuskan di spec 0006; tambah `stackly-ipc` (`crates/ipc`) di *Peta crate* beserta aturannya di *Aturan arah dependency* (dependency normal hanya `stackly-protocol` di antara crate Stackly, dev boleh testkit dan evidence, tanpa `unsafe`, boleh dipakai `stacklyd` dan `stackly-cli`), larangan bergantung pada library `stacklyd`, dan pindaian nama socket TCP atau UDP; catat `stacklyd` sebagai library plus bin; tambah `serde_path_to_error` dan fitur `raw_value` ke `stackly-protocol`, `stackly-ipc` dan `tokio` ke `stackly-cli`, serta `stackly-ipc`, `uuid`, dan `libc` ke `stacklyd`; di *Alur kontrak* catat nilai enum snake_case, `#[serde(default)]` di field opsional, larangan `BTreeMap` di DTO, bagian yang dibekukan lintas major, file tambahan hasil codegen (`OpenEnums.swift`, `ContractInfo.swift`, `contract_digest.rs`, `error-codes.json`, `contract-versions.json`), mapping di `typeshare.toml`, dan `ProtocolVersion` 1.0; di *Konfigurasi workspace* catat `exclude = ["fuzz"]`, tokio dan `serde_path_to_error` di `[workspace.dependencies]`, fitur `env-filter`, toolchain nightly yang hanya untuk `fuzz/`, serta sink log file yang kini diputuskan. Diterapkan 2026-10-18.
- [x] `/architect` update spec 0004: capability `ipc_endpoint` diwujudkan tanpa trait baru di `stackly-platform` (transport di `stackly-ipc`, kredensial peer lewat tokio, lock lewat std), jadi kalimat "fitur 6 menambah trait" di *Port platform* diganti; tambah `ensure_runtime_root` ke tabel *API surface*; tambah `cargo_bin` dan `SpawnedDaemon` ke *Fake platform*; catat bahwa `stackly` membaca `STACKLY_ROOT` sejak fitur 6 dan bahwa `stacklyd --print-roots` menggantikan perilaku keluar 0 setelah mencatat root di AC-7 spec 0004; centang Follow-up fitur 6 di spec 0004 setelah milestone 4 dibangun; catat hasil pengamatan pembersihan `_CS_DARWIN_USER_TEMP_DIR` di macOS 27 dari langkah 22. Diterapkan 2026-10-18 sebagai titik perluasan fitur 6 di spec 0004; Follow-up fitur 6 di spec 0004 dicentang karena dirancang di sini; pengamatan langkah 22 tetap terbuka di spec 0004 sampai direkam.
- [ ] Spec 0002 (atau `/sync`): kalau `NCSA` ditambahkan ke `deny.toml` untuk `libfuzzer-sys`, catat alasannya di daftar lisensi spec 0002.
- [ ] Spec 0013: client Swift memakai `ClientKind.gui` dengan `interactive` true, menampilkan `items` di frame error, dan menerapkan aturan observasi di resep snapshot plus cursor.
- [x] Spec 0008 menetapkan exit code `not_found` dan `internal`, dan mengganti output sementara `stackly daemon status` dengan kontrak output CLI. Diputuskan spec 0008 (10 dan 1) dan dicatat di tabel kategori (2026-10-18); output sementara diganti saat milestone 1 spec 0008 dibangun.
- [ ] Spec 0013 membangun transport Swift dari vektor framing, aturan endpoint, handshake, tiga tahap decode frame, resep reconnect, dan `StacklyJSON` di spec ini.
- [ ] Fitur 15: pertimbangkan verifikasi tanda tangan kode `stacklyd` lewat audit token di client, dan sediakan `stackly daemon start` untuk menggantikan hint `start it with stacklyd`.
- [ ] Fitur 64: ukur ulang batas antrian, ring, dan koneksi di mesin baseline lalu setel ulang lewat update spec ini kalau perlu.
- [x] `/sync` setelah build: catat di root `AGENTS.md` crate `stackly-ipc`, `stacklyd` sebagai library plus bin, `STACKLY_LOG`, `stacklyd --print-roots`, perintah `cargo xtask fuzz`, pin nightly fuzz, aturan wire (newtype `u64`, enum terbuka, tanpa `BTreeMap`, `Validate`), aturan versi dan `contract-versions.json`, dan aturan `check-deps` yang baru; perbarui `clients/swift/AGENTS.md` dengan file `Generated/` yang baru dan `StacklyJSON`. Diterapkan 2026-10-19 lewat `/sync` penutup fitur 6, ditambah aturan refresh `fuzz/Cargo.lock`.
- [x] Agent Skills dan MCP server untuk cargo-fuzz dan socket Unix tokio ditolak pada 2026-10-18; catat di baris `Declined:` root `AGENTS.md` lewat `/sync` supaya tidak ditawarkan lagi. Dicatat 2026-10-19.
- [x] Kode: crate `stackly-protocol` dari milestone 1 fitur 6 masih memakai fallback `Unknown` dan nama `HealthStatus`; ganti menjadi `Unrecognized` dan `DaemonHealthStatus` (beserta `OpenEnums.swift`), dan pindahkan `recovery` ke `DaemonHealth`, sebelum protokol 1.0 dirilis. Selesai di protokol 1.2 (`0d7e46d`, 2026-10-19).
- [x] `/architect`: AC-10 masih menyebut `ProtocolVersion::CURRENT` adalah 1.0, padahal kontrak kini 1.2 (dan sesi kontrak 1.3 sedang berjalan); ganti angkanya dengan aturan versi (minor naik untuk tambahan, major untuk perubahan yang merusak) supaya kalimatnya tidak basi setiap kali versi naik. Ditemukan saat `/check verify` 2026-10-19. Diterapkan 2026-10-19 bersama protokol 1.4: AC-10 dan *Versi dan kompatibilitas* kini menyebut aturan, bukan angka.
- [ ] Test: setelah ada method bertanda `recovery refused` (misalnya operasi spec 0007), tambah test wire di `apps/daemon/tests/server/lifecycle.rs` yang memanggilnya dalam recovery mode dan mendapat `registry.recovery_mode` sebelum handler jalan. Hari ini keenam method spec ini `recovery allowed`, jadi hanya `recovery_refusal` yang diuji langsung.
- [ ] Pemeriksaan otomatis untuk `fuzz/Cargo.lock` yang tertinggal dari `fuzz/Cargo.toml` atau dari dependency crate yang dipakai target fuzz (misalnya di `cargo xtask check-deps`), karena saat ini baru ketahuan ketika `cargo xtask fuzz build` dijalankan. Ditemukan 2026-10-19 setelah wave 1 menambah dependency ke `stackly-core` dan `stackly-platform`; lock nya disegarkan di `4626e28`.
- [ ] Ketahanan client `stackly-ipc` sebelum fitur 8 mengulang call di koneksi yang sama (review fitur 6, 2026-10-19, `docs/reviews/2026-10-19-claude-f6-closeout-938837.md` bagian 2): `Client::call` yang dibatalkan di tengah menulis frame meninggalkan frame terpotong di wire, jadi penulisan frame harus selesai walau pemanggilnya pergi (task penulis tunggal atau tanda tulis yang sedang berjalan); antrian event 1024 yang penuh menahan setiap reply di belakangnya; dan test untuk `REJECT_WRITE_TIMEOUT`, `Client` atau `EventStream` yang di drop, serta ID yang tidak dikenal.
- [x] Temuan kontrak review fitur 6 (`docs/reviews/2026-10-19-claude-f6-closeout-938837.md` bagian 1 dan 2): `request.invalid_params` tanpa batas `fields` (frame error di atas 1 MiB memutus koneksi tanpa jawaban), `Timestamp::from_jiff` yang panic dekat `jiff::Timestamp::MAX`, array posisional yang diterima sebagai request, `call`, dan params, tidak ada fuzz target untuk `decode_server_frame`, digest yang tidak melihat batas, `retryable` yang wajib tetapi tidak dibekukan, dan `read` atau `write` yang dipetakan ke `ipc.io_error` yang retryable. Selesai di protokol 1.4 (Milestone 6, 2026-10-19); engineer memilih membekukan `retryable` dan menghitung field yang dibuang di `details.omitted`.
- [ ] Swift membaca `sequence`, `revision`, dan `Count` sebagai `UInt64` biasa, jadi nilai di atas 2^53 − 1 yang ditolak Rust tetap diterima Swift (nit review fitur 6). Perbaikannya (tipe pembungkus `SafeUInt64` lewat `typeshare.toml`) mengubah API Swift yang sedang dirangkai fitur 13, jadi ditunda ke wave saat app Swift tidak sedang diubah.
- [ ] Array posisional di bawah tingkat atas masih terbaca oleh derive serde: struct di dalam params (misalnya `since: ["…", 4]` atau `protocol: [1, 0]`), enum adjacently tagged seperti `Reply` dan `EventKind` dari `[tag, content]`, dan `ErrorBody` dari array; pembaca longgar `handshake_major` juga membaca `call` dan `content` posisional, jadi request yang oleh `decode_request` disebut malformed masih bisa mendapat `protocol.major_mismatch`. Protokol 1.4 hanya menolak array di tingkat atas request, `call`, params, frame daemon, dan `content` nya. Nilainya tetap lolos `Validate`, dan daemon memeriksa ulang major hasil decode, jadi risikonya rendah; penutupnya adalah deserializer pembungkus yang membaca struct hanya dari objek.
- [ ] Daemon (fitur 7): nilai `details`, `hint`, dan `items` dari handler tidak dibatasi panjangnya, dan hanya jalur reply handler yang punya fallback `ipc.reply_too_large`; jalur penolakan decode dan penolakan sebelum handshake menulis frame error tanpa fallback itu. Pindahkan fallback ukuran ke tempat frame di encode (`encode` atau `enqueue_bytes` di `apps/daemon/src/connection.rs`) supaya berlaku untuk setiap frame. Ditemukan cross check protokol 1.4.
- [ ] `Validate::validate` mengembalikan `Vec<FieldError>` penuh sebelum `ErrorBody` memotongnya di 32, jadi aturan atas daftar yang lupa membatasi diri masih mengalokasikan satu field per entri input (puluhan MB untuk satu request 1 MiB). Subscribe sudah membatasi diri; penutup umumnya adalah wadah field berbatas yang dioper ke `validate`. Risiko rendah karena socket hanya untuk uid yang sama.
- [ ] Spec 0013: komentar dokumentasi `ClientErrorCatalog.ioError` di `clients/swift` masih menyebut `read` dan `write` sebagai `op` dari `ipc.io_error`; Swift sudah memakai `ipc.connection_closed` untuk koneksi yang putus, jadi hanya komentarnya yang basi.
- [ ] Ketahanan log dan berhenti `stacklyd` (review fitur 6 bagian 3): pipe stderr yang tidak dibaca membekukan `stacklyd.log` (tulis ke stderr harus non blocking atau lewat antrian sendiri; menulis file lebih dulu hanya memindahkan satu baris); flush saat berhenti memakai tiga jendela berurutan sampai sekitar 3 detik, bukan 1 detik, dan sinyal kedua masih menunggu registry ditutup; dan teks serta lokasi panic handler hanya sampai ke stderr mentah, tidak ke file log.

## Rationale

Konteks, opsi yang dibandingkan di setiap keputusan, alasan, keputusan kecil, bukti uji typeshare, dan hasil cross check ada di [rationale.md](rationale.md).
