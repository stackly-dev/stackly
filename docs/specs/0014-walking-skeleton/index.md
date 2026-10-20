# 0014. Walking skeleton

**Date**: 2026-10-20
**Status**: Proposed

## Summary

Spec ini menutup fase Foundation dengan satu irisan tipis yang membuktikan semuanya tersambung: fake provider dibuat, dijalankan, dan dihentikan lewat `stackly`, daemon mencatatnya di registry, GUI melihat setiap perubahan lewat event, dan daemon yang mati lalu hidup lagi tidak pernah menghasilkan instance ganda atau proses yatim. Karena install runtime nyata baru ada di fitur 18, fake runtime dan paket fake masuk ke root development lewat satu example cargo `stacklyd` (`skeleton_seed`) yang hanya memakai dependency dev, sehingga fitur crate `fixture` tidak pernah ikut di build produksi. Fitur ini menambah empat perintah CLI (`instance create`, `start`, `stop`, `get`), tidak menambah kode GUI, tidak mengubah kontrak protokol, dan membuktikan semuanya dengan test di Mac ini plus satu record manual yang kamu review.

## Requirements

**User stories**:

- Sebagai developer Stackly, saya ingin satu perintah yang menyiapkan root development berisi fake runtime dan paket fake, supaya saya bisa mencoba CLI dan GUI dengan daemon asli tanpa menunggu install runtime fitur 18.
- Sebagai pengguna CLI, saya ingin `stackly instance create`, `start`, `stop`, dan `get` dengan `--wait` dan `--json`, supaya saya dan script saya bisa menjalankan instance lewat daemon dan membaca statusnya.
- Sebagai pengguna GUI, saya ingin daftar Instances berubah sendiri saat instance dibuat, dijalankan, atau dihentikan dari client lain, dan instance tetap hidup saat saya menutup jendela atau keluar dari app (AC-001).
- Sebagai pengguna, saya ingin daemon yang crash atau di restart saat instance saya berjalan kembali ke instance yang sama, tanpa instance kedua, tanpa proses yatim, dan tanpa data yang hilang (AC-013).
- Sebagai pemilik scope, saya ingin bukti bahwa provider baru yang manifest nya valid bisa dipasang, dibuat, dijalankan, dan dilihat kesehatannya tanpa kode khusus provider di core, daemon, CLI, atau GUI (AC-029 awal).

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-013):

- **AC-1**: Setelah `cargo build -p stackly-fake-server`, `cargo run -q -p stacklyd --example skeleton_seed -- --root <R>` menyiapkan root `R` dengan langkah yang sama dengan slot start daemon (`MacosPlatform::new(Some(R))`, `prepare_layout`, `DaemonLock::acquire`, `stackly_storage::open_registry` yang membuat dan memigrasi registry kalau belum ada), lalu `seed_fake` menyalin `stackly-fake-server` ke folder runtime, mem pin `providers/fake`, dan meng commit record runtime (`installed`), artefak, dan provider revision dalam satu `WriteBatch`. Stdout menulis root, runtime ID, nama provider, revision, dan satu perintah `stackly instance create` siap pakai; `--json` menulis satu objek `{ root, runtimeId, provider: { name, revision }, createCommand }`. Menjalankan ulang pada root yang sama tidak menulis baris baru dan mencetak ID yang sama.
- **AC-2**: Seed keluar 2 dari parser kalau `--root` tidak disebut, dan keluar 1 dengan satu baris `error: <pesan>` di stderr kalau `validate_root_override` spec 0004 menolak root nya (relatif, tidak ada, bukan folder atau berupa symlink, bukan milik user, bisa ditulis group atau other, sama dengan atau di dalam root data default), lock daemon root itu dipegang `stacklyd` (pesan meminta kamu menghentikan daemon untuk root itu), registry dalam recovery mode, binary fake server tidak ada (pesan menyebut `cargo build -p stackly-fake-server`), folder runtime atau paket yang akan ditulis sudah ada tanpa baris registry nya (`leftover`), atau root sudah memegang provider dengan nama dan revision yang sama tetapi digest berbeda, atau runtime fake dengan versi yang sama tetapi SHA-256 binary berbeda (`conflict`, pesan meminta root baru). Setelah penolakan apa pun, folder runtime dan paket milik seed serta jumlah baris runtime dan provider revision di registry sama dengan sebelumnya. Selama berjalan seed memegang lock daemon root itu, jadi `stacklyd` yang distart bersamaan berhenti seperti daemon kedua (spec 0006).
- **AC-3**: Fitur crate `fixture` milik `stackly-core` hanya menyala lewat dependency dev: graph fitur tanpa dependency dev (`cargo tree -p stacklyd -p stackly-cli --edges features,no-dev -i stackly-core`) tidak memuat `fixture`, build rilis tidak pernah memuatnya, dan `cargo xtask check-deps` dengan aturan langkah 15 spec 0010 (rumusan di *Aturan fitur `fixture`*) lulus dengan example di tempatnya serta menolak setiap crate fixture yang melanggar salah satu larangannya.
- **AC-4**: `stackly-fake-server init --delay-ms=<n>` membaca token dari stdin lebih dulu, lalu menunggu `n` milidetik sebelum menyentuh folder data. `providers/fake` naik ke revision `0.2.0` dengan setting `initDelayMs` (integer 0 sampai 600000, default 0, `apply: immutableAfterInit`, `visibility: advanced`) yang diteruskan sebagai `--delay-ms=${settings.initDelayMs}`. `cargo xtask provider check providers/fake` ok, dan perubahan ini hanya menyentuh `crates/fake-server/`, `providers/fake/`, serta golden dan test yang membaca paket itu, tanpa kode core, daemon, CLI, atau Swift (bagian AC-029: setting baru tanpa perubahan core atau GUI).
- **AC-5**: `stackly instance create <name> --runtime <runtime-id> --provider <name> --revision <label> [--display-name <teks>] [--port <endpoint>=<port|auto>]... [--set <field>=<teks>]... [--set-json <field>=<json>]... [--wait] [--timeout <durasi>] [--idempotency-key <key>]` mengirim tepat satu `instance.create` dengan params sesuai tabel *Pemetaan flag create* dan tanpa panggilan daemon lain sebelumnya. Validasi sisi CLI hanya bentuk argumen (`--runtime` bukan UUID, `--port` atau `--set` tanpa `=`, angka port di luar 0 sampai 65535 atau bukan angka, `--set-json` yang bukan JSON), semuanya `cli.usage` (exit 2) tanpa menghubungi daemon; port 0 tetap dikirim supaya daemon yang menolaknya. Field yang muncul lebih dari sekali memakai kemunculan terakhir, dan `--set-json` menimpa `--set` untuk field yang sama. Dengan `--json`, `result` adalah `{ operation, idempotencyKey, instance }` dengan aturan `instance` di AC-6; di bawah `--wait`, `operation` adalah `OperationSummary` terakhir (sama seperti `inspect` spec 0008).
- **AC-6**: `stackly instance start <instance-id>` dan `stackly instance stop <instance-id>` (UUID 36 karakter bertanda hubung, huruf besar atau kecil, dinormalkan ke huruf kecil) dengan `AsyncArgs` mengirim `instance.start { instanceId }` atau `instance.stop { instanceId, overrideLeases: false }`; `result` adalah `{ operation, idempotencyKey, instance }`. Jawaban `alreadyInState` keluar 0 dengan `operation` null dan teks `already <lifecycle>`. Aturan `instance` sama untuk create, start, dan stop: setelah jawaban `alreadyInState`, atau setelah `Waiter` selesai di state apa pun (akhir atau `needs_recovery`), kalau ID instance diketahui, CLI memanggil `instance.get` tepat sekali dan mengisi `instance` dengan hasilnya, juga saat envelope membawa `error`; kalau `instance.get` itu gagal, `instance` null dan exit code serta `error` tetap milik operasi. Tanpa `--wait` dan tanpa `alreadyInState`, `instance` null. Error sinkron (`resource.not_found`, `supervisor.*`) keluar dengan exit code kategori nya.
- **AC-7**: `stackly instance get <instance-id>` memanggil `instance.get` dan dengan `--json` memberi `result` `{ instance: InstanceSummary }`. Teksnya adalah blok `kv`, tabel endpoint, dan `Failure output:` persis seperti `inspect` di *CLI* spec 0022 (label, urutan, dan format waktu, umur, dan nilai yang sama), sehingga `get` bisa menjadi alias `inspect` dan langkah 5 spec 0022 memakai render yang sama. Instance yang tidak ada gagal `resource.not_found` (exit 10).
- **AC-8**: `AsyncArgs` (`WaitArgs` ditambah `--idempotency-key`, 1 sampai 128 byte ASCII tercetak lewat `IdempotencyKeyArg`, selain itu exit 2) dan `Context::submit` mengikuti *Perintah async dan menambah command group* spec 0008: key dari flag atau UUID v7 baru dalam huruf kecil, selalu ada di `result.idempotencyKey`; tanpa `--wait` teks `operation <id> <state>` dengan hint `wait with: stackly operation inspect <id> --wait`; dengan `--wait` lewat `Waiter` mode `Observe`, dengan `needs_recovery` sebagai error; `ipc.call_timeout` mengisi `details.idempotencyKey` dan hint `retry safely with: --idempotency-key <key>`. Mengulang create yang sama dengan key yang sama mendapat operasi yang sama (replay spec 0007).
- **AC-9**: Dengan `stacklyd` asli pada `TestRoot` yang sudah di seed (lewat `STACKLY_ROOT`), binary `stackly` lewat runner test (aturan di *Suite ujung ke ujung*) menjalankan create dengan `--wait`, start dengan `--wait`, get, lalu stop dengan `--wait`: create `succeeded` dengan instance `stopped` dan `initialized: true`, start `succeeded` dengan lifecycle `running` dan readiness `ready`, get menampilkan lifecycle, desired, readiness, health, dan proses sebagai field terpisah, dan stop berakhir dengan lifecycle `stopped`. Selama berjalan ada tepat satu proses `stackly-fake-server serve` untuk instance itu, dan nol setelah stop. Evidence `fixture` dengan `covers` AC-029.
- **AC-10**: Create dengan `--set-json failInit=true --wait` keluar bukan 0 dengan error `supervisor.init_failed` dan `instance` berisi instance `failed`; operasi `create_instance` berstatus `needs_recovery` (K1, selama create sementara spec 0010 berlaku). `stackly operation abandon <id> --yes` mengakhiri operasi `failed` dengan `operation.abandoned`, start instance itu ditolak `supervisor.not_initialized`, dan tidak ada proses fake yang tersisa.
- **AC-11**: Dengan instance fake `running` dan `ready`, daemon dihentikan dengan `SIGKILL` lalu dijalankan lagi pada root yang sama, dan sekali lagi dengan `SIGTERM`. Setelah setiap restart, dengan `get` yang diulang sampai paling lama 10 detik (readiness `unknown` dengan reason `daemon_restarted` sampai probe pertama): PID dan `startedAt` sama, `adopted: true`, lifecycle `running`, dan readiness kembali `ready`; `instance.list` memuat tepat satu instance dengan nama itu; ada tepat satu proses fake untuk instance itu; nama dan SHA-256 setiap file di folder data instance sama dengan sebelum restart; start berikutnya menjawab `alreadyInState` tanpa spawn baru. Setelah stop, tidak ada proses yang argv nya memuat path `TestRoot`. Evidence `fixture` dengan `covers` AC-013.
- **AC-12**: Create dengan `--set-json initDelayMs=15000` dan `--idempotency-key <k>` tanpa `--wait`; begitu proses `stackly-fake-server init` instance itu terlihat, daemon dibunuh dengan `SIGKILL` lalu dijalankan lagi. Setelah rekonsiliasi, selalu: operasi `needs_recovery`, tepat satu instance dengan nama itu, dan lifecycle `failed` dari baris `init_failed { interrupted }` (spec 0010 *Rekonsiliasi*). Blocker `dataInUse` diperiksa hanya kalau `ProcessTable` melihat pembantu init masih hidup sebelum dan sesudah `get` itu. Daemon baru tidak pernah menyinyal pembantu itu, yang selesai sendiri setelah delay nya. Create yang sama dengan key `k` mendapat operasi yang sama, create dengan key baru untuk nama itu ditolak `instance.name_taken` tanpa membuat operasi, dan jumlah instance tetap satu. Setelah abandon, operasi `failed` dengan `operation.abandoned`, start ditolak `supervisor.not_initialized`, dan setelah pembantu selesai tidak ada proses yang argv nya memuat path `TestRoot`. Evidence `fixture` dengan `covers` AC-013.
- **AC-13**: Setiap test dan demo fitur ini menjalankan `stacklyd` dengan `STACKLY_ROOT`, jadi secret instance tersimpan di keychain file `<R>/data/keychain/stackly.keychain-db` (file itu ada setelah create pertama) dan login Keychain tidak pernah dibuka (spec 0011). Setiap client yang mengirim mutasi di test berjalan noninteractive (CLI tanpa TTY dengan `STACKLY_NO_INPUT=1`, client Swift dengan `ClientInfo.interactive` false), jadi interaksi Keychain selalu `Never` dan seluruh suite selesai tanpa dialog dalam batas waktu test nya.
- **AC-14**: Test Swift ujung ke ujung di `StacklyUI` (binary seed dan `stacklyd` asli) membuktikan `InstanceStore` menerima perubahan yang dibuat client lain lewat event: koneksi `StacklyClient` kedua milik test (noninteractive) membuat, menjalankan, dan menghentikan instance; instance baru muncul di store setelah create; saat start dengan `readyDelayMs` 1500, item melewati lifecycle `running` dengan readiness `not_ready` sebelum `ready`; setelah stop, item berakhir `stopped`. Model GUI tidak mengirim mutasi: himpunan ID di `operation.list` sama persis dengan operasi yang dibuat koneksi kedua. Evidence `fixture` dengan `covers` AC-001 dan AC-029.
- **AC-15**: Dengan tiga instance fake `ready`, test Swift menutup GUI seperti Quit (`ShellRuntime.willTerminate()`); lewat koneksi kedua ketiganya tetap `running` dengan PID yang sama, `GET /health` di port endpoint masing masing menjawab 200, dan `operation.list` tidak memuat operasi baru sejak penutupan. Model GUI baru lewat `ShellBootstrap.live` mengambil snapshot dan memegang ketiga instance dengan PID yang sama. Evidence `fixture` dengan `covers` AC-001.
- **AC-16**: Satu record manual `fixture` dibuat dengan `cargo xtask evidence new --covers AC-001 AC-013 AC-029 --kind fixture --title "Walking skeleton through the CLI and GUI" --expected "Instances created, started, and stopped through stackly show up in the Stackly window without a refresh, survive closing and quitting the app and a killed or stopped stacklyd with the same PID, never appear twice, and leave no fake process after stop"`, berisi langkah *Demo dan cek manual* di app Debug, `stackly`, dan `stacklyd` asli pada root yang di seed. Kamu reviewer nya: kamu menjalankan langkahnya pada commit itu lalu mengisi `steps`, `actual`, `status`, `reviewer`, dan `finishedAt`. Status awal `NOT_RUN`.
- **AC-17**: `stackly instance create` menerima `--secret <field>=<sumber>` berulang dengan sumber dan aturan baca modul `secret_input` spec 0011 (`generate`, `prompt`, `stdin` untuk paling banyak satu field, `file:<path>`, `env:<NAMA>`, `copy:<ref>`, `bind:<ref>`), dipetakan ke `secrets[]`. Field yang tidak disebut tidak dikirim, sehingga daemon memakai `generate` (spec 0011). Nilai secret tidak pernah muncul di argv, output teks, atau `--json`.
- **AC-18**: Fixture credential dari `FixtureCredential` dikirim lewat `--secret authToken=stdin` saat create (test menulis `value()` diikuti satu `\n`, yang dibuang `secret_input`); `token.sha256` yang ditulis init fake sama dengan SHA-256 dari `value()`; prefix nya tidak ditemukan di file log daemon di root log, file stderr daemon, file registry beserta `-wal` dan `-shm`, spool instance di runtime root, `instance-manifest.json`, dan stdout serta stderr setiap perintah CLI di test itu. File config `fake.json` dan folder `secrets/` instance tidak dipindai, karena template paket memang menulis token ke config itu. Evidence `fixture` dengan `covers` AC-016.
- **AC-19**: Dengan instance yang sudah dibuat dan berhenti, keychain file root dikunci dengan `/usr/bin/security lock-keychain <R>/data/keychain/stackly.keychain-db`. `stackly instance start <id> --wait` tanpa TTY gagal `secret.needs_secret` tanpa dialog, lifecycle tetap `stopped`, `get --json` menunjukkan `blocker` `needsSecret`, dan di test Swift start lewat koneksi kedua yang noninteractive membuat item `InstanceStore` membawa blocker yang sama. Setelah daemon distart ulang (yang membuka lagi keychain file nya, spec 0011), start berhasil dan blocker hilang.

## Decision

**Chosen option**: opsi yang direkomendasikan di setiap keputusan di [rationale.md](rationale.md): fake runtime dan paket fake masuk ke root development lewat example cargo `stacklyd` bernama `skeleton_seed` yang memanggil `seed_fake` di fitur `fixture` core lewat dependency dev, hanya untuk root `STACKLY_ROOT` yang lolos `validate_root_override`; CLI fitur ini adalah `instance create`, `start`, `stop`, dan `get` dengan ejaan flag spec 0020, argumen UUID saja, dan teks `get` yang sama dengan `inspect` spec 0022; GUI tidak mendapat kode baru dan dibuktikan lewat test Swift ujung ke ujung (mutasi dari koneksi client kedua) serta record manual (mutasi dari CLI); bukti restart memakai binary daemon asli yang dibunuh `SIGKILL` dan dihentikan `SIGTERM`, dengan create yang terputus lewat delay init fake; semua bukti berlabel `fixture`.

Langkah 15 spec 0010 menegakkan aturan fitur `fixture` dengan rumusan di *Aturan fitur `fixture`* di bawah (baris Follow-up spec 0010, 2026-10-20).

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`) · `swift-testing-pro` (`twostraws/swift-testing-agent-skill`, `.claude/skills/swift-testing-pro/`) · `swift-concurrency` (`avdlee/swift-concurrency-agent-skill`, `.claude/skills/swift-concurrency/`)

Kalau isi skill bertentangan dengan spec ini, spec yang menang.

## Feature design

### Batas fitur

- **Di dalam**: `seed_fake` di fitur `fixture` core, example `skeleton_seed`, rumusan aturan fitur `fixture` untuk langkah 15 spec 0010, delay init fake dan setting `initDelayMs`, `AsyncArgs`, `IdempotencyKeyArg`, `Context::submit`, perintah `stackly instance create`, `start`, `stop`, dan `get` (termasuk render `kv` yang juga dipakai `inspect` spec 0022), suite ujung ke ujung daemon dan CLI, test Swift ujung ke ujung, record manual, `--secret` di create, dan bukti sentinel serta needs secret.
- **Di luar**: method, DTO, event, dan kode error instance (spec 0010 langkah 12), handler `stacklyd`, create sementara, `install_fake`, dan `pin_package` (langkah 13), test daemon per skenario supervisor (langkah 14), kode `check-deps` nya (langkah 15), `InstanceStore` dan `ProvisionalInstanceList` (spec 0013 langkah 16), tampilan blocker di GUI (spec 0022 aturan I4 dan spec 0021 AC-14), `stackly instance list`, `inspect`, pencarian nama, dan `cli.instance_not_found` (spec 0022 langkah 5), create penuh dengan preflight, `--start`, `--dry-run`, dan `--runtime software@versi` (spec 0020 langkah 9 dan 10), install runtime nyata (fitur 18), dan provider nyata (fitur 19 dan 23).

| Bagian | Pemilik |
| --- | --- |
| DTO instance, `InstanceActionAccepted`, event, kode `supervisor`, `process`, `instance`, fixture `protocols/fixtures/instance/` | spec 0010 langkah 12 |
| Handler `ipc::instance`, create sementara (K1), `install_fake`, `pin_package`, fitur `fixture` | spec 0010 langkah 13 |
| Test daemon lewat IPC untuk setiap skenario supervisor | spec 0010 langkah 14 |
| Kode aturan `check-deps` fitur `fixture` (rumusan dari spec ini) | spec 0010 langkah 15 |
| `codes::secret` dan kompensator `secret_ref` | spec 0011 langkah 5 |
| Modul `secret_input` CLI | spec 0011 langkah 8 |
| `InstanceStore`, `ProvisionalInstanceList`, aturan `statusSeq` di Swift | spec 0013 langkah 16 |
| Target app membaca `STACKLY_ROOT` | spec 0013 langkah 15 |
| `seed_fake`, `skeleton_seed`, `cargo_example` | spec ini, milestone 1 |
| `AsyncArgs`, `Context::submit`, `instance create`, `start`, `stop`, `get` | spec ini, milestone 2 |
| Delay init fake, suite ujung ke ujung, restart, create terputus | spec ini, milestone 3 |
| Test Swift ujung ke ujung, record manual | spec ini, milestone 4 |
| `--secret`, sentinel, needs secret | spec ini, milestone 5 |

Fitur ini tidak mengubah kontrak protokol, jadi `ProtocolVersion::CURRENT` tidak naik dan `cargo xtask codegen` tidak perlu dijalankan.

### Seed root development

Fungsi baru di `crates/core/src/supervisor/fixture.rs` (di balik fitur `fixture`, di samping `install_fake` dan `pin_package` spec 0010):

```rust
pub struct SeededFake {
    pub runtime_id: RuntimeId,
    pub provider: ProviderRevisionKey,
    pub created: bool,
}

pub fn seed_fake(
    platform: &Platform,
    layout: &DirectoryLayout,
    registry: &dyn Registry,
    fake_server: &Path,
    package: &Path,
) -> Result<SeededFake, FixtureError>;
```

- Membaca registry lebih dulu lewat bacaan spec 0005 yang sudah ada: runtime dengan software `fake`, versi dari `providers/fake/versions.json` (sumber yang sama dengan `install_fake`), dan SHA-256 binary; serta provider revision dengan nama dan revision paket.
- Runtime dengan versi sama tetapi SHA-256 berbeda, atau provider dengan nama dan revision sama tetapi digest berbeda, menghasilkan `FixtureError::Conflict { what }` tanpa tulisan apa pun.
- Bagian yang belum ada dibuat: keduanya lewat `install_fake`; paket saja lewat `pin_package`; runtime saja lewat bentuk runtime saja dari `install_fake` (K5). Kalau langkah 13 spec 0010 tidak menyediakan bentuk runtime saja sebagai fungsi, langkah 1 spec ini memisahkannya menjadi `supervisor::fixture::install_fake_runtime` dan `install_fake` memanggilnya. Lalu satu `WriteBatch` berisi `InsertRuntime`, `InsertArtifact`, dan `InsertProviderRevision` untuk bagian yang baru saja. Kalau keduanya sudah ada, tidak ada yang ditulis dan `created` false.
- Folder runtime atau folder paket pinned yang sudah ada tanpa baris registry nya menghasilkan `FixtureError::Leftover { path }` sebelum ada tulisan apa pun. Root development boleh dibuang; tidak ada pembersihan otomatis.
- Kalau langkah 14 spec 0010 sudah menaruh helper commit yang setara di `apps/daemon/tests/common/`, langkah 1 spec ini memindahkannya ke `seed_fake` dan test itu memakainya, supaya hanya ada satu helper.
- Test di dalam proses yang memanggil `seed_fake` lalu menjalankan `SpawnedDaemon` pada root yang sama menutup registry (yang menunggu thread penulis selesai) dan melepas lock daemon lebih dulu.

Example `apps/daemon/examples/skeleton_seed.rs`:

```text
skeleton_seed --root <path> [--fake-server <path>] [--json]
```

1. Parse argumen. `--root` wajib. `--fake-server` default nya `stackly-fake-server` di folder profil di atas folder example itu sendiri (`target/debug/` untuk `target/debug/examples/skeleton_seed`), lewat `std::env::current_exe()`; example tidak pernah menjalankan cargo.
2. `MacosPlatform::new(Some(root))`, yang memanggil `validate_root_override` spec 0004, lalu `prepare_layout`, `DaemonLock::acquire(&layout.daemon_lock(), owner)`, dan `stackly_storage::open_registry`, dalam urutan yang sama dengan `stacklyd::startup`. Semua fungsi itu sudah publik; tidak ada fungsi `stacklyd` yang perlu dibuka. Registry dalam recovery mode ditolak.
3. `seed_fake` dengan binary dari langkah 1 dan folder `providers/fake` repo (dari `CARGO_MANIFEST_DIR` `stacklyd`).
4. Tulis hasil, tutup registry, lepas lock.

Output teks:

```text
root      <R>
runtime   <runtime-id>
provider  <name> <revision>
next      STACKLY_ROOT=<R> stackly instance create demo --runtime <runtime-id> --provider <name> --revision <revision> --wait
```

`--json`: `{ "root": "<R>", "runtimeId": "<uuid>", "provider": { "name": "<name>", "revision": "<revision>" }, "createCommand": "<perintah di atas>" }`. Nilai dengan karakter di luar `[A-Za-z0-9._/:@%+=,-]` dikutip tunggal POSIX, sama dengan perintah setara spec 0020. Exit 0 kalau berhasil (juga saat tidak ada yang baru), 1 untuk penolakan dan error, 2 untuk argumen yang salah. Rust test memberikan `--fake-server` dari `stackly_testkit::fake_server::binary()`.

Testkit mendapat `cargo_example(package, name) -> PathBuf` di samping `cargo_bin`, dengan pola cache dan lock yang sama.

### Aturan fitur `fixture`

Rumusan yang ditegakkan langkah 15 spec 0010 (`cargo xtask check-deps`), dengan satu crate fixture yang melanggar untuk setiap larangan:

1. Tidak ada tabel `[dependencies]`, `[build-dependencies]`, atau `[workspace.dependencies]`, termasuk tabel `[target.'…'.dependencies]` dan `[target.'…'.build-dependencies]`, di manifest mana pun di workspace yang menyalakan `fixture` pada `stackly-core`, juga lewat nama lain (`package = "stackly-core"`).
2. Tidak ada entri `[features]` di crate selain `stackly-core` yang memuat `stackly-core/fixture` atau `stackly-core?/fixture` (atau padanannya lewat nama lain).
3. Di `stackly-core` sendiri, `default` dan fitur lain tidak memuat `fixture`.
4. `[dev-dependencies]`, termasuk per target, boleh menyalakannya, sehingga test, bench, dan example sebuah package memakainya. Batas fitur per crate tetap berlaku di atas aturan ini (misalnya aturan dependency dev `stackly-cli` spec 0001).

Modul `supervisor::fixture` hanya ada dengan `#[cfg(feature = "fixture")]`. Yang dijamin aturan ini adalah graph fitur tanpa dependency dev dan build rilis. Binary debug `stacklyd` yang dibangun oleh `cargo test`, clippy `--all-targets`, atau build example bisa ikut memuat `fixture` karena cargo menyatukan fitur dev di pemanggilan itu; binary itu tidak pernah memanggilnya.

### Paket fake: delay init

- `stackly-fake-server init` mendapat `--delay-ms=<n>` (default 0): setelah token dibaca dari stdin dan sebelum folder data disentuh, proses tidur `n` milidetik. Nilai yang bukan angka gagal exit 2 seperti flag lain.
- `providers/fake/provider.yaml`: revision `0.2.0`, argumen init ditambah `"--delay-ms=${settings.initDelayMs}"`. `schemas/settings.schema.json`: properti `initDelayMs` seperti AC-4.
- Golden plan dan render diperbarui dengan `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test provider_packages`, dan test yang memeriksa argv init fake mengikuti.
- Init yang lebih lama dari `timeouts.initSeconds` (30 detik) berakhir `init_timeout`, jadi test memakai delay di bawah itu.

### CLI

Modul folder `apps/cli/src/commands/instance/` (bentuk yang diharapkan spec 0022): `mod.rs` (enum `InstanceCommand`), `create.rs`, `action.rs` (start dan stop), `get.rs`, dan `render.rs` (blok `kv`, tabel endpoint, dan `Failure output:` spec 0022, yang nanti dipakai `inspect`). `args.rs` mendapat `IdempotencyKeyArg`, `InstanceIdArg` (UUID 36 karakter, huruf besar atau kecil, dinormalkan), `PortArg`, `SettingArg`, dan `SettingJsonArg`; `AsyncArgs` di samping `WaitArgs`; `Context::submit` di `context.rs`.

```text
stackly instance create <name> --runtime <runtime-id> --provider <name> --revision <label>
    [--display-name <teks>] [--port <endpoint>=<port|auto>]... [--set <field>=<teks>]...
    [--set-json <field>=<json>]... [--secret <field>=<sumber>]...
    [--wait] [--timeout <durasi>] [--idempotency-key <key>] [--json]
stackly instance start <instance-id> [--wait] [--timeout <durasi>] [--idempotency-key <key>] [--json]
stackly instance stop <instance-id> [--wait] [--timeout <durasi>] [--idempotency-key <key>] [--json]
stackly instance get <instance-id> [--json]
```

`--secret` baru ada di milestone 5. Perintah `create` diberi `// provisional: F20` pada bagian yang diganti langkah 10 spec 0020 (parser `--runtime` UUID saja, `--provider` dan `--revision` wajib, `--set` selalu teks).

**Pemetaan flag create**:

| Flag | Field `instance.create` | Aturan |
| --- | --- | --- |
| `<name>` | `name` | apa adanya; daemon memeriksa VAL-001 |
| `--display-name` | `displayName` | dikosongkan kalau tidak ada |
| `--provider` | `provider.name` | wajib; apa adanya (nilai yang dicetak seed) |
| `--revision` | `provider.revision` | wajib selama create sementara menolak revision kosong |
| `--runtime` | `runtimeId` | wajib; UUID |
| `--port e=auto` | `endpoints[]` `{ name: e, port: { type: auto } }` | urutan argumen, ganda tetap dikirim |
| `--port e=<n>` | `endpoints[]` `{ name: e, port: { type: fixed, content: n } }` | `n` 0 sampai 65535 |
| `--set f=<teks>` | `settings.f` sebagai string JSON | kemunculan terakhir menang |
| `--set-json f=<json>` | `settings.f` sebagai nilai JSON apa pun | diterapkan setelah `--set` |
| `--secret f=<sumber>` | `secrets[]` `{ field: f, input: SecretInput }` | lewat `secret_input`; field lain tidak dikirim |

Tanpa `--port`, `endpoints` kosong dan daemon memberi `auto` untuk setiap endpoint IP (spec 0010). Integer dan boolean pada fake ditulis lewat `--set-json` (misalnya `--set-json readyDelayMs=1500`), karena `--set` di fitur ini selalu teks; langkah 10 spec 0020 mengonversi `--set` menurut `FieldKind` tanpa mengubah arti `--set-json`.

**Hasil**:

| Perintah | `result` (`--json`) | Teks |
| --- | --- | --- |
| `create` | `{ operation, idempotencyKey, instance }` | tanpa `--wait`: `instance <uuid>` lalu `operation <id> <state>` dan hint spec 0008; dengan `--wait` sukses: `created <name> (<uuid>)` |
| `start`, `stop` | `{ operation, idempotencyKey, instance }` | `already <lifecycle>`, atau `operation <id> <state>` dengan hint, atau dengan `--wait` sukses satu baris `<name> <lifecycle>, readiness <state>` |
| `get` | `{ instance }` | blok `kv`, tabel endpoint, dan `Failure output:` spec 0022 |

ID instance hasil create dibaca dari `resource` operasi (`OperationAccepted` spec 0007 untuk operasi dengan `InstanceId`). Exit code selalu dari kategori error (spec 0008). Golden teks dan JSON setiap verb ada di `apps/cli/tests/golden/` (`instance_create.*`, `instance_start.*`, `instance_stop.*`, `instance_get.*`) dari daemon skrip, dengan `TZ=UTC` dan jam tetap yang sudah dipakai harness golden spec 0008. Daemon skrip membalas fixture `InstanceSummary` dari `protocols/fixtures/instance/` (langkah 12 spec 0010); bentuk yang tidak ada di sana (instance `running` yang diadopsi, instance `failed` dengan `lastFailure`, instance `stopped` dengan blocker `needsSecret`) dibuat sebagai data test di `apps/cli/tests/` dari DTO yang sama, bukan sebagai fixture kontrak. Matriks exit code mendapat baris untuk `alreadyInState`, `needs_recovery` di bawah `--wait`, `resource.not_found`, `instance.get` yang gagal setelah menunggu, dan `cli.usage`.

### Suite ujung ke ujung daemon dan CLI

File `apps/daemon/tests/skeleton.rs` dan `apps/daemon/tests/skeleton_seed.rs`, di package `stacklyd` yang sudah memakai `fixture` lewat dependency dev, jadi `stackly-cli` tidak butuh `fixture`.

Setiap test:

- membuat `TestRoot`, menjalankan `seed_fake` di dalam proses (atau binary example untuk test seed), menutup registry dan lock, lalu `SpawnedDaemon` dengan `STACKLY_ROOT` ke root itu;
- memasang guard `Drop` yang, juga saat test panic, menghentikan setiap proses yang argv nya memuat path canonical `TestRoot` (`SIGTERM`, lalu `SIGKILL` setelah 5 detik) lewat `ProcessTable::reap_under`, lalu menghapus runtime root milik root itu (path dari `MacosPlatform::new(Some(root))`, berada di temp dir user di luar `TestRoot`, berisi spool dan socket);
- menjalankan binary `stackly` (`cargo_bin("stackly-cli", "stackly")`) lewat runner test dengan aturan: env `STACKLY_ROOT`, `STACKLY_NO_INPUT=1`, dan `NO_COLOR=1`; stdin `/dev/null` kecuali test yang mengirim secret lewat pipe; selalu `--json`; setiap `--wait` dengan `--timeout 60s`; dan proses CLI yang belum selesai setelah 90 detik dibunuh dan test gagal; envelope dibaca dari stdout;
- menghitung proses lewat `ProcessTable` (memanggil `/bin/ps -axww -o pid=,args=` lalu menyaring argv yang memuat path tertentu);
- menemukan folder data instance lewat inventarisasi spec 0004 (`instance-manifest.json` dengan ID itu) untuk perbandingan isi data;
- memasang guard evidence `fixture` dengan `covers` yang disebut AC nya.

Urutan restart (AC-11): create dan start dengan `--wait`, catat `process.pid` dan `process.startedAt` dari `get`, hash isi folder data, `SpawnedDaemon::kill()`, start `SpawnedDaemon` baru pada root yang sama, tunggu subsystem `supervisor` `ok` di `stackly daemon status --json` (rekonsiliasi selesai), lalu ulangi `get` sampai readiness `ready` atau 10 detik, baru periksa. Ulangi dengan `SpawnedDaemon::stop()` (`SIGTERM`).

Urutan create terputus (AC-12): create dengan `--set-json initDelayMs=15000 --idempotency-key <k>` tanpa `--wait`, tunggu sampai `ProcessTable` menemukan `stackly-fake-server init` dengan path folder data instance itu di argv, `SpawnedDaemon::kill()`, start daemon baru, lalu periksa sesuai AC-12. Test menunggu pembantu selesai sendiri (paling lama delay ditambah 10 detik) sebelum memeriksa bahwa tidak ada proses tersisa.

### GUI

Fitur ini tidak menambah kode GUI. Daftar Instances (`ProvisionalInstanceList`) dan `InstanceStore` dari langkah 16 spec 0013 sudah menampilkan instance dan memperbaruinya lewat event, dan notice tutup jendela serta app yang tetap hidup sudah dari spec 0013 AC-15. Tampilan blocker milik spec 0022 (aturan I4) dan spec 0021 AC-14.

Test Swift `SkeletonEndToEndTests` di target test `StacklyUI` (`apps/macos/Packages/StacklyUI/Tests/StacklyUITests/`), dengan tag `.daemon` dan trait `.evidence(kind: .fixture, ...)`:

1. Helper test di `apps/macos/Packages/StacklyUI/Tests/StacklyUITests/Support/` (folder yang tidak dipindai test aturan sumber spec 0013 AC-23): runner proses async yang menunggu exit lewat continuation terminasi (tidak pernah `waitUntilExit()` di `MainActor`), dengan batas waktu yang membunuh proses, dan helper daemon yang menerima root dari test (`SpawnedDaemon` milik `StacklyClientTests` membuat root sendiri dan tidak bisa diimpor dari target ini).
2. Membuat root pendek sementara, menjalankan binary seed pada root itu, lalu `stacklyd` pada root yang sama.
3. Membangun model lewat `ShellBootstrap.live(rootOverride:clientVersion:now:)`, jalur yang sama dengan app.
4. Membuka koneksi `StacklyClient` kedua milik test (`DaemonConnection` dengan `ClientInfo.interactive` false) untuk create, start, stop, `instance.get`, dan `operation.list`, sambil mencatat setiap nilai item di `InstanceStore`.
5. Untuk AC-15: tiga instance (`alpha`, `beta`, `gamma`), `willTerminate()`, pemeriksaan lewat koneksi kedua dan `GET /health`, lalu model baru.
6. Untuk AC-19 (milestone 5): kunci keychain file, start lewat koneksi kedua, lalu blocker di item `InstanceStore`.

Binary dicari lewat `STACKLY_DAEMON_BIN` (spec 0013) dan `STACKLY_SEED_BIN`, dengan default `target/debug/stacklyd` dan `target/debug/examples/skeleton_seed` relatif terhadap akar repo dari `#filePath`. Binary yang tidak ada membuat test `BLOCKED` dengan petunjuk perintah build di *Menjalankan test di Mac ini*.

### Demo dan cek manual

Dijalankan dengan tangan di Mac ini, lalu dicatat di record manual AC-16. Siapkan dulu:

```bash
cargo build -p stacklyd -p stackly-cli -p stackly-fake-server
```

```bash
cargo build -p stacklyd --example skeleton_seed
```

```bash
export R="$(mktemp -d /tmp/stackly-skeleton.XXXXXX)"
```

```bash
target/debug/examples/skeleton_seed --root "$R"
```

```bash
STACKLY_ROOT="$R" target/debug/stacklyd
```

App Debug dijalankan dari Xcode dengan `STACKLY_ROOT=$R` di scheme Stackly. Perintah `stackly` di bawah memakai `STACKLY_ROOT="$R" target/debug/stackly` dan nilai dari output seed.

1. Halaman Instances menampilkan "No Instances".
2. `instance create demo ... --set-json readyDelayMs=3000 --wait`: baris "demo" muncul tanpa klik apa pun, status Stopped.
3. `instance start <id> --wait`: baris berubah dari Starting ke Running yang belum siap, lalu siap.
4. Tutup jendela: notice muncul; `ps -axww -o pid=,args=` masih menampilkan `stackly-fake-server serve`; `instance get <id>` masih `running` dengan PID yang sama; klik Dock membuka jendela dengan baris yang sama.
5. ⌘Q, lalu `instance get <id>`: PID sama; buka app lagi: baris Running.
6. Hentikan daemon dengan Ctrl+C: banner offline dengan waktu, baris tetap Running, proses fake masih hidup. Jalankan daemon lagi: banner hilang, `instance get <id>` menunjukkan PID sama dan `adopted: true`.
7. `kill -9` PID daemon lalu jalankan lagi: sama dengan langkah 6, `ps` menunjukkan tepat satu proses fake, dan daftar menunjukkan tepat satu "demo".
8. `instance create broken ... --set-json failInit=true --wait`: gagal `supervisor.init_failed`, baris "broken" Failed; `operation abandon <op> --yes`.
9. `instance create slow ... --set-json initDelayMs=20000` tanpa `--wait`; begitu `ps` menunjukkan `stackly-fake-server init`, `kill -9` daemon lalu jalankan lagi: satu baris "slow" Failed, `operation inspect <op>` `needs_recovery`; `operation abandon <op> --yes`.
10. `instance stop <id> --wait`: baris berubah ke Stopped; `ps` tidak menunjukkan proses fake.
11. Keyboard dan VoiceOver di daftar: setiap baris membacakan nama, provider dan versi, dan status.
12. Hentikan daemon, keluar dari app, lalu hapus folder `$R` kalau sudah selesai.

### Evidence

| Bukti | Kind | Method | `covers` | Langkah build |
| --- | --- | --- | --- | --- |
| happy path CLI lewat daemon (AC-9) | `fixture` | automated | AC-029 | 7 |
| restart dengan `SIGKILL` dan `SIGTERM` (AC-11) | `fixture` | automated | AC-013 | 8 |
| create terputus (AC-12) | `fixture` | automated | AC-013 | 8 |
| event di `InstanceStore` (AC-14) | `fixture` | automated | AC-001, AC-029 | 9 |
| tutup dan buka GUI dengan tiga instance (AC-15) | `fixture` | automated | AC-001 | 9 |
| record manual (AC-16) | `fixture` | manual | AC-001, AC-013, AC-029 | 10 |
| sentinel fixture credential (AC-18) | `fixture` | automated | AC-016 | 12 |

Semua record mulai `NOT_RUN`. Fake provider dan dummy server berarti `fixture` (spec 0002), jadi fitur ini tidak menulis record `native`: bukti native AC-013 datang dari fitur 19, 23, dan 30, dan AC-029 baru lengkap dengan form di fitur 20 dan harness konformitas fitur 36. Record fixture yang lulus bukan bukti runtime native.

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| seed | root canonical | `--root` lewat `validate_root_override` di `MacosPlatform::new(Some(root))` (spec 0004) |
| seed | binary fake | `--fake-server`, atau `stackly-fake-server` di folder profil di atas `current_exe()`; di Rust test `stackly_testkit::fake_server::binary()` |
| seed | paket | folder `providers/fake` dari `CARGO_MANIFEST_DIR` `stacklyd` |
| seed | versi dan line runtime fake | `providers/fake/versions.json` (versi `0.1.0`, line `0.1`), lewat `install_fake` |
| seed | `RuntimeId`, record runtime, artefak, provider revision | `install_fake`, bentuk runtime saja nya, dan `pin_package` (spec 0010 langkah 13, K5), atau baris registry yang sudah ada |
| seed | `createCommand` | nilai di atas, dikutip seperti perintah setara spec 0020 |
| create | `name`, `displayName`, `runtimeId`, `provider`, `endpoints`, `settings`, `secrets` | flag di tabel *Pemetaan flag create* |
| create | `runtimeId` yang diketik pengguna | output seed |
| semua perintah async | idempotency key | `--idempotency-key`, atau UUID v7 baru dari `uuid` di CLI |
| create | ID instance di output | `resource` operasi di reply |
| create, start, stop dengan `--wait` | `operation` | `OperationSummary` terakhir dari `Waiter` |
| create, start, stop | `instance` | satu `instance.get` setelah `Waiter` selesai atau setelah `alreadyInState` (aturan AC-6) |
| get | semua field dan format | `InstanceSummary` dari `instance.get`, dengan format `inspect` spec 0022 |
| test | PID, `startedAt`, `adopted` | `InstanceSummary.process` |
| test | jumlah instance dengan nama itu | `instance.list` lewat client IPC test (semua halaman) |
| test | jumlah proses | `ProcessTable` dari `/bin/ps` |
| test | folder data instance | inventarisasi `instance-manifest.json` spec 0004 |
| test | runtime root untuk dibersihkan dan spool untuk dipindai | `DirectoryLayout` dari `MacosPlatform::new(Some(root))` (runtime root di temp dir user, `instance_spool`) |
| test | state operasi | `stackly operation inspect <id> --json` |
| test | delay init | `--set-json initDelayMs=15000` |
| test Swift | binary | `STACKLY_DAEMON_BIN`, `STACKLY_SEED_BIN`, atau default di *GUI* |
| test Swift | mutasi dan bacaan pembanding | koneksi `StacklyClient` kedua dengan `ClientInfo.interactive` false |
| test Swift | item yang diamati | `InstanceStore` dari `ShellBootstrap.live` |
| sentinel | nilai secret | `FixtureCredential::new()`, `value()` ditambah `\n` ke stdin `--secret authToken=stdin` |
| sentinel | file yang dipindai | root log, file stderr `SpawnedDaemon`, file registry beserta `-wal` dan `-shm` dari `DirectoryLayout`, spool, `instance-manifest.json`, output setiap perintah CLI |
| needs secret | keychain yang dikunci | `<R>/data/keychain/stackly.keychain-db` (spec 0011) |

### Key invariants

- Fitur `fixture` hanya menyala lewat dependency dev (*Aturan fitur `fixture`*); graph tanpa dependency dev dan build rilis `stacklyd` dan `stackly` tidak memuatnya.
- Seed hanya menulis di bawah root yang lolos `validate_root_override`, tidak pernah di root default, tidak pernah saat daemon root itu hidup, tidak pernah menjalankan cargo, dan tidak menulis apa pun saat menolak.
- CLI tidak membaca paket provider atau runtime, tidak men spawn proses, dan hanya memanggil method daemon (spec 0008).
- Fitur ini tidak menambah kode GUI dan tidak mengubah kontrak protokol.
- Setiap daemon di test dan demo memakai `STACKLY_ROOT`; tidak ada test yang menyentuh root default atau login Keychain, dan setiap client yang mengirim mutasi di test berjalan noninteractive.
- Daemon baru tidak pernah menyinyal pembantu init yang tertinggal; ia hanya melaporkan `dataInUse` sampai lock nya bebas (spec 0010).
- Setiap test ujung ke ujung berakhir tanpa proses yang argv nya memuat path `TestRoot` nya, juga saat test gagal (guard `Drop`).
- Nilai secret tidak pernah ada di argv, output teks, atau `--json` CLI.

### Security model

- Seed adalah alat development: hanya dibangun sebagai example, hanya untuk root override yang lolos `validate_root_override`, dan memegang lock daemon root itu selama menulis. Keychain file root override dengan file password nya hanya pantas untuk root development dan test (spec 0011).
- Seed menulis registry di luar daemon. Itu pengecualian yang disengaja untuk kode fitur `fixture`, sama dengan test yang meng commit hasil `install_fake`; jalur produksi tetap hanya daemon yang menulis registry.
- Tidak ada method atau kode baru, jadi otorisasi tetap aturan peer user yang sama spec 0006.
- Fixture credential hanya lewat stdin; test tidak pernah menulisnya sebagai literal (spec 0002).

### Configuration required

- `STACKLY_SEED_BIN` (baru, hanya untuk test Swift): path absolut binary example `skeleton_seed`.
- `STACKLY_ROOT` (spec 0004), `STACKLY_NO_INPUT` (spec 0008), dan `STACKLY_DAEMON_BIN` (spec 0013) dipakai apa adanya. Tidak ada credential baru.

### Menjalankan test di Mac ini

| Langkah | Perintah |
| --- | --- |
| unit dan golden CLI | `cargo test -p stackly-cli` |
| seed dan suite ujung ke ujung | `cargo xtask evidence run --kind fixture -- cargo test -p stacklyd --test skeleton_seed --test skeleton --locked --no-fail-fast` |
| test Swift (build binary di dalam run yang sama, supaya `sourceCommit` record cocok dengan binary nya) | `cargo xtask evidence run --kind fixture -- sh -c 'cargo build --locked -p stacklyd -p stackly-fake-server && cargo build --locked -p stacklyd --example skeleton_seed && swift test --package-path apps/macos/Packages/StacklyUI --skip "NativeTests\."'` |
| record manual | `cargo xtask evidence new` dengan argumen di AC-16 |
| cek | `cargo xtask evidence check`, `cargo xtask check-deps`, dan perintah `cargo tree` di AC-3 (dijalankan `/check verify`) |

### Critical test scenarios

- Seed: root kosong menghasilkan ID dan perintah; ulang tanpa baris baru; daemon start pada root itu dan `instance.create` dengan nilai yang dicetak `succeeded`. Memverifikasi **AC-1**.
- Penolakan seed: tanpa `--root`, root relatif, root yang tidak ada, daemon yang sedang hidup, binary fake yang tidak ada, folder `leftover`, dan kedua `conflict` (test core untuk `Conflict` dan `Leftover`); setelah setiap penolakan folder milik seed dan jumlah baris registry sama. Penolakan root default dan bentuk root lain sudah dibuktikan test `validate_root_override` spec 0004. Memverifikasi **AC-2**.
- Aturan `fixture`: `cargo tree` di atas, dan crate fixture `check-deps` untuk keempat butir rumusan, termasuk `[workspace.dependencies]` dan nama lain. Memverifikasi **AC-3**.
- Delay init: test fake server bahwa `--delay-ms` menunda sebelum folder data dibuat dan token tetap terbaca; golden paket dan `provider check`. Memverifikasi **AC-4**.
- CLI terhadap daemon skrip: golden setiap verb, pemetaan flag ke params, kemunculan terakhir dan `--set-json` yang menang, setiap kasus `cli.usage` tanpa koneksi, `alreadyInState`, `needs_recovery` di bawah `--wait`, `instance.get` yang gagal setelah menunggu, `resource.not_found`, key dari flag dan key buatan, replay, dan `ipc.call_timeout` dengan hint. Memverifikasi **AC-5**, **AC-6**, **AC-7**, **AC-8**.
- Happy path daemon asli. Memverifikasi **AC-9**, **AC-13**.
- Init gagal lalu abandon. Memverifikasi **AC-10**.
- Restart dengan `SIGKILL` dan `SIGTERM`. Memverifikasi **AC-11**.
- Create terputus. Memverifikasi **AC-12**.
- Event di `InstanceStore`, lalu tutup dan buka GUI dengan tiga instance. Memverifikasi **AC-14**, **AC-15**.
- Record manual. Memverifikasi **AC-16**.
- `--secret` dengan setiap sumber terhadap daemon skrip, lalu sentinel lewat daemon asli. Memverifikasi **AC-17**, **AC-18**.
- Keychain file dikunci: CLI dan `InstanceStore` menunjukkan `needsSecret`, lalu pulih setelah daemon start ulang. Memverifikasi **AC-19**.
- Auth atau permission: tidak ada peran baru; peer user lain sudah ditolak spec 0006, dan fitur ini tidak menambah permukaan.

### Asumsi lintas spec

- **Spec 0010** (diterapkan di main): method `instance.create`, `start`, `stop`, `get`, dan `list`, `InstanceActionAccepted`, `InstanceSummary` dengan `statusSeq`, create sementara dengan `register` sebagai langkah pertama yang tidak bisa dibalik (K1), `install_fake` dan `pin_package` (K5), rekonsiliasi, dan pemilik `instance.lock` oleh pembantu init.
- **Spec 0008**: pola *Perintah async dan menambah command group*, `Waiter`, `operation inspect` dan `operation abandon`, `STACKLY_NO_INPUT`, harness golden, dan peta exit code.
- **Spec 0011**: daemon memakai `generate` untuk secret field yang tidak dikirim; modul `secret_input` dan pembuangan satu newline di akhir stdin; keychain file untuk `STACKLY_ROOT` yang tetap terkunci sampai daemon start lagi; interaksi dari `ClientInfo.interactive`.
- **Spec 0013**: `InstanceStore`, `ShellBootstrap.live(rootOverride:clientVersion:now:)`, `ShellRuntime.willTerminate()`, `DaemonConnection`, aturan sumber AC-23, dan pola `STACKLY_DAEMON_BIN`.
- **Spec 0020**: ejaan flag create; langkah 9 mengganti create sementara dan memperbarui test spec ini.
- **Spec 0022**: bentuk `result` dan teks `inspect` (`{ instance }`, blok `kv`, tabel endpoint), modul folder `commands/instance/`, dan resolver nama yang nanti menerima nama di argumen `<instance-id>`.
- **Spec 0002, 0004, 0006**: evidence dan record manual, `validate_root_override`, inventarisasi folder instance, `DirectoryLayout`, `SpawnedDaemon`, dan lock daemon tunggal.

## Build plan

Pendekatan proyek adalah Journey. Jalur yang dibangun lengkap di fitur ini adalah "developer menyiapkan root, membuat, menjalankan, mengamati, dan menghentikan fake instance, GUI mengikutinya, dan daemon yang mati di tengah jalan tidak meninggalkan instance ganda atau proses yatim", termasuk jalur init gagal, create terputus, dan secret yang tidak terbaca. Seed dan CLI dibangun lebih dulu karena keduanya pintu masuk jalur itu dan tidak saling bergantung; bukti lewat daemon asli, GUI, dan secret menyusul.

**Milestone 1: Seed root development** (after spec 0010 langkah 13 di merge, yang membawa `install_fake`, `pin_package`, dan fitur `fixture`; boleh sebelum atau sesudah langkah 15 spec 0010, dan yang di merge belakangan menjalankan `check-deps` terhadap yang lain; bisa paralel dengan milestone 2)

1. `seed_fake`, `SeededFake`, `FixtureError::Conflict` dan `Leftover`, dan `install_fake_runtime` kalau langkah 13 belum menyediakan bentuk runtime saja, dengan test core di atas `FakePlatform` dan `FakeRegistry` (root kosong, ulang tanpa tulisan, bagian yang hilang saja, `leftover`, kedua `conflict`), serta pemindahan helper commit langkah 14 spec 0010 kalau ada. Memenuhi **AC-1**, **AC-2**.
2. `cargo_example` di testkit, example `skeleton_seed`, dan `apps/daemon/tests/skeleton_seed.rs`. Memenuhi **AC-1**, **AC-2**, **AC-3**.

**Milestone 2: Perintah instance di CLI** (after fitur 8 build di `01-foundation.md` dan spec 0010 langkah 12 di merge (DTO dan fixture instance di `stackly-protocol` dan `protocols/fixtures/instance/`); hanya menyentuh `apps/cli/`, jadi bisa paralel dengan milestone 1)

3. `IdempotencyKeyArg`, `InstanceIdArg`, `PortArg`, `SettingArg`, `SettingJsonArg`, `AsyncArgs`, dan `Context::submit` dengan test daemon skrip. Memenuhi **AC-8**.
4. `commands/instance/` dengan `create`, `start`, `stop`, dan `get`, render `kv` spec 0022, aturan `instance` AC-6, golden, dan baris matriks exit code. Memenuhi **AC-5**, **AC-6**, **AC-7**.

**Milestone 3: Lewat daemon asli dan restart** (after milestone 1 dan 2, dan spec 0010 langkah 14 di merge, yang berarti spec 0011 langkah 5 dan langkah 16 fitur 7 sudah ada)

5. Delay init fake: `--delay-ms`, `initDelayMs`, revision `0.2.0`, golden, dan test yang membaca argv init. Langkah ini satu satunya sesi yang menyentuh `providers/fake/` dan golden paket selama ia berjalan. Memenuhi **AC-4**.
6. Testkit: `ProcessTable` dengan `reap_under`, runner CLI test dengan aturan di *Suite ujung ke ujung*, guard `Drop` yang juga menghapus runtime root, dan `SpawnedDaemon::kill()` (`SIGKILL` lewat `Child::kill`, lalu tunggu exit) kalau langkah 14 spec 0010 belum menambahkannya. Memenuhi **AC-9**, **AC-11**, **AC-12** (prasyarat).
7. `skeleton.rs`: happy path, init gagal lalu abandon, dan keychain file tanpa dialog, dengan guard evidence. Memenuhi **AC-9**, **AC-10**, **AC-13**.
8. `skeleton.rs`: restart dengan `SIGKILL` dan `SIGTERM`, dan create terputus, dengan guard evidence. Memenuhi **AC-11**, **AC-12**.

**Milestone 4: GUI dan AC-001** (langkah 9 after milestone 1 dan spec 0013 langkah 16 di merge (`InstanceStore`, `ProvisionalInstanceList`); langkah 10 after milestone 2 dan 3 serta spec 0013 langkah 15 (app membaca `STACKLY_ROOT`); langkah 9 bisa paralel dengan milestone 2 dan 3)

9. Helper test di `StacklyUITests/Support/` (runner proses async dan helper daemon dengan root dari test), lalu `SkeletonEndToEndTests` untuk event dan tutup buka GUI lewat koneksi `StacklyClient` kedua. Memenuhi **AC-14**, **AC-15**.
10. Record manual lewat `cargo xtask evidence new`, dijalankan dan diisi oleh kamu sebagai reviewer. Memenuhi **AC-16**.

**Milestone 5: Secret ujung ke ujung** (after milestone 3 dan spec 0011 langkah 8 di merge (`secret_input`); langkah 13 after langkah 9; bisa paralel dengan milestone 4)

11. `--secret <field>=<sumber>` di create lewat `secret_input`, golden, dan test daemon skrip setiap sumber. Memenuhi **AC-17**.
12. Suite sentinel dan keychain terkunci di `skeleton.rs`. Memenuhi **AC-18**, **AC-19**.
13. Langkah keychain terkunci di `SkeletonEndToEndTests`. Memenuhi **AC-19**.

Sesi paralel: milestone 1 dan 2 berjalan bersamaan karena folder nya terpisah (core, testkit, dan `apps/daemon` untuk milestone 1; `apps/cli` untuk milestone 2). Langkah 9 boleh berjalan bersamaan dengan milestone 2 dan 3 karena hanya menyentuh target test `StacklyUI`. Langkah 5 tidak berjalan bersamaan dengan sesi lain yang menyentuh `providers/fake/` atau golden paket provider. Langkah 7, 8, dan 12 satu sesi per waktu karena semuanya menulis `skeleton.rs`.

## Consequences

**Positive**:

- Fase Foundation ditutup dengan bukti ujung ke ujung yang memakai binary asli (daemon, CLI, fake server, dan model GUI), bukan hanya fake di dalam proses.
- Developer bisa mencoba seluruh jalur di Mac ini dengan satu perintah seed, tanpa menunggu fitur 18 dan tanpa menyentuh root atau Keychain milik nya sendiri.
- Fitur crate `fixture` tetap keluar dari build rilis, dan aturannya kini cukup tepat untuk ditegakkan `check-deps` tanpa celah lewat `[features]`, `[workspace.dependencies]`, atau nama lain.
- Fitur 20 dan 22 hanya menambah: flag create memakai ejaan spec 0020, `get` sudah berbentuk dan bertampilan `inspect`, dan folder `commands/instance/` sudah ada.
- Create yang terputus kini punya bukti nyata di binary daemon, bukan hanya di tabel rekonsiliasi.

**Negative / tradeoffs**:

- Seed menulis registry di luar daemon, satu pengecualian yang harus dijaga tetap di fitur `fixture` dan example saja.
- Binary debug `stacklyd` dari `cargo test`, clippy, atau build example bisa ikut memuat `fixture` tanpa memakainya; hanya graph tanpa dependency dev dan build rilis yang dijamin bersih.
- `--runtime` hanya menerima UUID, `--provider` dan `--revision` wajib, dan `--set` selalu teks sampai fitur 20, jadi angka dan boolean di demo harus memakai `--set-json`.
- Argumen `<instance-id>` hanya UUID sampai resolver nama spec 0022 ada.
- Pembantu init yang terputus tetap hidup sampai selesai sendiri; untuk provider nyata itu bisa lama, dan Stackly hanya melaporkan `dataInUse` sampai lock nya bebas.
- Revision fake naik ke `0.2.0` dan golden paket berubah, jadi sesi lain yang memakai paket fake perlu rebase setelah langkah 5.
- Suite ujung ke ujung memanggil `/bin/ps`, membangun beberapa binary, dan menunggu delay init 15 detik, jadi lebih lambat dari test lain dan butuh macOS.
- Test Swift membuktikan perubahan dari client lain lewat koneksi `StacklyClient` kedua, bukan CLI; gabungan CLI dan GUI hanya dibuktikan record manual.
- Bukti GUI visual tetap manual sampai fitur 68.

**Neutral**:

- Example pertama di workspace; clippy `--all-targets` ikut memeriksanya.
- Testkit mendapat `cargo_example`, `ProcessTable`, dan runner CLI; target test `StacklyUI` mendapat helper proses dan daemon sendiri.
- Satu variabel environment baru yang hanya dibaca test Swift (`STACKLY_SEED_BIN`).
- Create sementara masih membuat init gagal menjadi `needs_recovery`; langkah 9 spec 0020 mengubah AC-10 dan AC-12 ke operasi `failed` yang dikompensasi penuh.

## Follow-up

- [x] Spec 0010: baris Follow-up langkah 15 dengan rumusan *Aturan fitur `fixture`* ditulis di spec 0010 (2026-10-20).
- [ ] Spec 0010 (lewat lead): langkah 13 sebaiknya menyediakan bentuk runtime saja dari `install_fake` sebagai fungsi bernama (K5 menyebut `install_fake` bisa dipakai untuk runtime saja); kalau tidak, langkah 1 spec ini membuat `install_fake_runtime`.
- [ ] Spec 0001 (lewat lead): aturan dependency dev `stackly-cli` (`stackly-core` dengan `test-support` saja) perlu menerima `fixture` juga kalau test binary CLI butuh fake runtime (binary test spec 0022 langkah 5 dengan `install_fake`, dan `nats_journey` spec 0023 dengan `pin_package`). Fitur ini tidak butuh, karena suite nya ada di `apps/daemon/tests/`.
- [ ] Spec 0013 (lewat lead): bagian event instance di langkah 18 dipenuhi `SkeletonEndToEndTests` langkah 9 spec ini, jadi langkah 18 tidak membuat test kedua untuk itu; `STACKLY_SEED_BIN` masuk *Configuration required* di samping `STACKLY_DAEMON_BIN`; dan teks langkah 6 yang menaruh `SpawnedDaemon` di product `StacklyTestSupport` tidak cocok dengan kode (ia ada di `Tests/StacklyClientTests/Support/`, karena `clients/swift/Sources` dipindai aturan sumber AC-23).
- [ ] Spec 0011: baris Follow-up fitur 14 dipenuhi milestone 5 dengan keychain file root yang dikunci, bukan `FakeSecretStore` (binary daemon tidak bisa memakai store tiruan), dan bagian tampilan GUI nya oleh spec 0022 aturan I4 dan spec 0021 AC-14; `/sync` mencentangnya setelah keduanya ada.
- [ ] Spec 0008: baris Follow-up fitur 14 (`AsyncArgs` dan `Context::submit`) dicentang `/sync` setelah milestone 2.
- [ ] Spec 0010: baris Follow-up fitur 14 dicentang `/sync` setelah milestone 3.
- [ ] Fitur 20 (spec 0020 langkah 9 dan 10): AC-10 dan AC-12 spec ini berubah ke operasi `failed` yang dikompensasi penuh dan instance yang dihapus (`instance` di output menjadi null karena `instance.get` gagal); `--runtime` menerima `software@versi`, `--provider` dan `--revision` menjadi opsional, `--set` dikonversi menurut `FieldKind`, `--set-json` tetap menerima nilai JSON apa pun untuk field apa pun, dan penanda `// provisional: F20` di `create.rs` dihapus.
- [ ] Fitur 22 (spec 0022 langkah 5): `inspect` memakai `render.rs` dan `get` menjadi alias nya dengan output sama, dan resolver nama menerima nama di argumen `start`, `stop`, dan `get`.
- [ ] Fitur 18: kalau daemon nanti bisa memasang runtime dari artefak lokal, pertimbangkan mengganti salinan runtime di seed dengan jalur itu.
- [ ] `/sync` setelah build: perintah seed dan suite ujung ke ujung di root `AGENTS.md`, `STACKLY_SEED_BIN` di `apps/macos/AGENTS.md`, dan catatan bahwa binary debug dari `cargo test` atau build example bisa memuat `fixture`.

## Rationale

Konteks, opsi di setiap keputusan, alasan, keputusan kecil, dan cross check ada di [rationale.md](rationale.md).
