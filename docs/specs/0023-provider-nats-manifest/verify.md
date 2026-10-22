# Verify: Provider NATS lewat manifest · spec 0023 · updated 2026-10-22

_Langkah diturunkan dari acceptance criteria spec 0023. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon. Tidak ada langkah milestone 1 yang membutuhkan jaringan; hanya langkah tiga fakta startup memakai server NATS asli dari cache._

## Milestone 1: Native probe kind and nats-readiness on fixtures

Langkah Build plan yang tercakup: 0, 3, 4 (sesi sebelumnya), lalu 1, 5, dan 6 (sesi `dev-f23-m1`). Langkah 2 (blok `readiness` dan `connection` di `providers/nats`) menunggu baris `runtime.versionCheck` NATS dari spec 0018 langkah 1 masuk ke `main`; bagian langkahnya di bawah ditandai menunggu.

### UI / manual

- [x] Tiga fakta startup NATS 2.15.1 dari langkah 0, direkam di Mac ini dari artefak cache runtime (sha256 `ed0fe392…5ffd`), loopback, port bebas, tanpa jaringan; bukti di `evidence/manual/2026-10-22-spec-0023-startup-facts-nats-2-15-1-info-listene.json` dan folder `files/` nya (hasil mentah dan kedua skrip). (1) INFO pertama setelah start memuat `"jetstream":true` hanya sesudah JetStream aktif: setiap koneksi yang dibuat tepat setelah spawn (poll 1 ms) langsung mendapat `jetstream:true`, dan `$JS.API.INFO` menjawab `account_info_response` pada koneksi pertama itu; store kosong 179,7 ms sesudah spawn, store 127 MB dengan 300 stream dan 120000 pesan 102,3 ms (JetStream butuh 85,3 ms). Tidak ada INFO `jetstream:true` yang datang sebelum API JetStream melayani. (2) Listener client baru terbuka sesudah JetStream aktif: log memuat `Starting JetStream`, restore stream, `Took 85.338833ms to start JetStream`, baru `Listening for client connections`, lalu `Server is ready`; jadi tidak ada jendela port terbuka dengan JetStream belum siap (langkah `$JS.API.INFO` tetap dipakai, keputusan lead 2026-10-20). (3) NATS tidak mematikan JetStream saat berjalan karena disk penuh: pada image 16 MB dengan sisa 20 KiB (400 publish 64 KiB) log memuat `Filestore [fill] Critical write error` dan `JetStream failed to store a msg`, tetapi INFO tetap `jetstream:true` dan `$JS.API.INFO` tetap menjawab `account_info_response` tanpa `error` pada dua koneksi baru; hanya stream yang gagal menulis. `SIGUSR2` (lame duck) menghentikan JetStream seketika (`Initiating JetStream Shutdown`) bersama listener: 8 koneksi baru dalam 2 detik ditolak, dan koneksi lama tidak mendapat balasan `$JS.API.INFO` dalam 2 detik. Akibat untuk `nats-readiness`: disk penuh tidak membuat readiness `not_ready` (hanya `ready` palsu terhadap stream yang gagal menulis, yang di luar kontrak probe), sedangkan lame duck terlihat sebagai `connect_failed`, sesuai *Hasil gagal*. Teks rationale tidak diubah di sini (`/architect`, Follow-up spec) → AC-5, AC-8

### Commands

- [x] `cargo test -p stackly-core --test provider_rules native` → 8 test lulus: probe `native` dengan token atau literal diterima di `readiness.probe` dan `health.probe`; nama di luar registry `unknown_probe` di `readiness.probe.name` dengan pesan yang menyebut `nats-readiness`, `name` yang tidak ada `missing_field`, `type: script` tetap `unknown_probe`; `endpoint` yang tidak ada `missing_field`, endpoint yang tidak dikenal atau bertransport `unix` `unknown_endpoint`; param tak dikenal `unknown_field`, param wajib tidak ada `missing_field`, literal bertipe salah, string kosong, di atas 255 byte, atau berkarakter kontrol `invalid_value`; token folder, root runtime, dan executable `token_not_allowed_here`, `secret.*` `secret_not_allowed_here`, token tak dikenal `unknown_token`, param boolean dengan teks lain `token_not_allowed_here`, field `apply: reload` `settings_schema_rule`; `path`, `expectStatus`, `command`, `successExitCodes` di `native` serta `name` dan `params` di `tcp`, `http`, `exec` `unknown_field`; `run_plan` membangun `ProbeSpec::Native` dan menulis `{"type":"native","content":{"target":...,"probe":{"name":"nats-readiness","params":{...}}}}`; param teks yang kosong atau di atas batas setelah render `PlanError::UnrepresentableValue` dengan `readiness.probe.params.<nama>` tanpa nilai → AC-1, AC-3
- [x] `cargo test -p stackly-core --test provider_rules closed_sets_reject_unknown_primitives_probes_formats_signals_and_strategies` → lulus, termasuk probe `native` dengan nama yang tidak ada di registry (skenario "Himpunan tertutup" spec 0009) → AC-1
- [x] `cargo test -p stackly-core --lib provider::probe::native` → 5 test lulus: nama registry unik dan cocok `^[a-z][a-z0-9-]{0,31}$`, `from_name`, descriptor `nats-readiness` (transport `tcp`, `serverName` teks 255 byte, `jetstream` boolean), `build`, dan bentuk `Serialize` → AC-2, AC-3
- [x] `cargo xtask codegen --check` → "20 generated files are up to date"; `schemas/provider/provider.v1alpha1.schema.json` memuat `name`, `params`, dan `ProbeParam` → AC-1
- [x] `cargo test -p stackly-core --test nats_readiness` → state machine untuk setiap transcript asli dan sintetis, dalam semua ukuran potongan → AC-5
- [x] `cargo test -p stackly-core --lib supervisor::probe::native` → 13 test di atas `tokio::io::duplex` dengan waktu dijeda: sukses dua mode, `jetstream_unavailable`, `connect_failed`, `timeout` saat connect, INFO, PONG, dan balasan `$JS.API.INFO` berakhir tepat 2000 ms dengan teks per fase, satu deadline untuk seluruh percobaan (INFO pada 1500 ms lalu tanpa PONG tetap berakhir di 2000 ms), EOF dan reset menjadi `protocol_error`, target `unix` tanpa connect, dan detail disamarkan lewat `Redactor` → AC-5
- [x] `cargo test -p stackly-core --test supervisor_probe` → 3 test lulus, termasuk smoke test dengan `tokio::net::TcpListener` asli: server bernama sama `Ok`, nama lain `identity_mismatch` dengan `server_name is "foreign", expected "golden"`, dan byte yang ditulis probe tidak memuat credential → AC-5
- [x] `cargo test -p stackly-core --test supervisor_start native` → 3 test lulus dengan `FakeProcessSupervision`, driver di atas duplex, dan waktu dijeda: `ready` dan `healthy` dengan reason `probe_ok`; server diam membuat readiness rutin `not_ready` dengan reason `timeout` dan detail `no INFO from the server within 1000 ms` setelah debounce, lifecycle tetap `running`, tidak ada sinyal atau spawn kedua, lalu pulih di PID yang sama; connect ditolak membuat health `degraded` setelah satu kegagalan lalu `unhealthy` di ambang 2 dengan reason `connect_failed`, dan kembali `healthy` → AC-6
- [x] `cargo clippy --workspace --all-targets --locked -- -D warnings` dan `cargo xtask check-deps` → bersih; `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → lulus → AC-2
- [x] `cargo xtask fuzz run provider_manifest --time 600` lalu `cargo xtask fuzz run provider_interpolation --time 600` (setelah langkah 1; memakai posisi interpolasi baru `ProbeText` dan `ProbeBoolean` di harness) → tanpa crash, hang, atau kehabisan memori; hasil di `evidence/manual/2026-10-21-rerun-provider-manifest-and-provider-interpolati.json` (NFR-009): provider_manifest 445881 run, provider_interpolation 1181823 run, tanpa crash → AC-7
- [x] (menunggu spec 0018 langkah 1 di `main`) Langkah 2: blok `readiness` dan `connection` di `providers/nats/provider.yaml` dalam satu commit, `README.md`, `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test provider_packages` dengan diff golden `run_plan` NATS hanya di `readiness.probe` dan `health.probe`, `cargo xtask provider check` mencetak `ok official/nats 1.0.0 <sha256 baru>`, dan `cargo xtask fuzz seed` → AC-4

### Acceptance-criteria coverage

- AC-1 · cabang `native` di `validate::probe`, JSON Schema, skenario "Himpunan tertutup"; selesai di langkah 1
- AC-2 · registry tertutup `NativeProbeKind` dengan `match` exhaustive di `descriptor`, `build`, dan `supervisor::probe::native::attempt`; selesai di langkah 1 dan 5
- AC-3 · `ProbeSpec::Native` di `run_plan`, `UnrepresentableValue`, dan `Serialize`; selesai di langkah 1
- AC-4 · menunggu langkah 2 (bergantung pada baris `runtime.versionCheck` NATS dari `dev-f18-m3`)
- AC-5 · state machine dan transcript selesai di langkah 0 dan 3; driver, timing, dan smoke socket selesai di langkah 5; tiga fakta startup selesai (langkah manual di atas, milestone 2)
- AC-6 · selesai di langkah 6
- AC-7 · target `nats_readiness_protocol` selesai di langkah 4; run ulang `provider_manifest` dan `provider_interpolation` di atas
- AC-8 sampai AC-12 · milestone 2 (di bawah)
- AC-13 dan AC-14 · milik milestone 4 (dulu disebut milestone 3)

## Milestone 2: NATS 2.15.1 end to end through the daemon, evidence, and catalog

Langkah Build plan yang tercakup: 7, 8, dan 9 (sesi `dev-f23-m2`). Semua langkah native butuh artefak `nats-2.15.1-macos-aarch64` (sha256 `ed0fe39222ec2e317c4d0e1a9cdc16c63ec523d94f9355d24a902a4db2655ffd`) di cache runtime xtask dan tidak mengunduh apa pun; tanpa artefak, kedua test berakhir `BLOCKED` dengan alasan `artifact nats-2.15.1-macos-aarch64 is not in the runtime cache` dan owner `@ojiepermana`. Run lengkap sekitar 70 detik.

Penyimpangan dari spec, dicatat untuk lead dan architect:

- AC-8 meminta kedua endpoint `Auto`. Create sementara spec 0010 (K1) menjalankan init tanpa port yang sudah terikat, dan `initialize` paket NATS merender `nats.conf` dari `endpoint.client.address`, jadi endpoint `Auto` berakhir di `supervisor.config_invalid` ("endpoint client has no concrete address"). Test memakai port `Fixed` dari `allocate_port` untuk `client` dan `monitoring` (ditandai `// provisional: F20`); create penuh spec 0020 yang mengalokasikan port sebelum init menggantikannya. Tidak ada perubahan paket.
- Seam `stackly_testkit::runtime_cache::cached_artifact(sha256)` (asumsi spec 0018) belum ada; harness memakai fungsi lokal yang sama dengan harness PostgreSQL (`~/Library/Caches/dev.stackly.xtask/runtime/<sha256>`).
- `nats_compatibility` memakai `SpawnedDaemon` dengan `SpawnCatalog::Unset`, seperti daemon biner di harness PostgreSQL, karena create sementara memilih revision dari record `pin_package`.
- Langkah 9: `cargo xtask codegen` setelah `compat-record` menulis record provider ke `providers/nats/versions.json`; file itu ikut di commit evidence supaya `codegen --check` lolos.

### UI / manual

- [x] Setelah run native dari `HEAD` bersih, baca `target/evidence/<runId>/report.json` → 2 record `native`, keduanya `PASS`, 0 problem, subject `nats` versi `2.15.1` (dari `GET /varz`) dengan digest artefak → AC-12
- [x] Langkah 9 berurutan: commit paket dan test (`providers/nats` tidak berubah di milestone ini, `manifestSha256` `91d3c6da2a0c43639d45a354e5f0dc9ddc492398b09b9350b99a25b1689e9235`), run native dari `HEAD` bersih, `cargo xtask runtime compat-record`, lalu satu commit berisi `evidence/compatibility/01a16181-5b99-7703-9d30-90b6b9a3d09d/`, NATS dari `[[planned]]` ke `[[provider]]` di `providers/catalog.toml` (`revision` 4), `providers/catalog-revisions.json` dari `cargo xtask catalog check --update`, dan `providers/nats/versions.json` dari `cargo xtask codegen` → AC-12

### Commands

- [x] `cargo test -p stacklyd --test nats --locked -- --ignored nats_lifecycle_in_process` → lulus: root data `Test Root Ünï cödé` dengan `nats.conf` yang memuat nama itu; create dan init `Stopped`/`initialized`; `instance.health_changed` memberi `not_ready` sebelum `ready` (reason `probe_ok`, urutan `statusSeq` naik), paket yang dipin memuat `name: nats-readiness`; `instance.get` menampilkan lifecycle `running`, desired `running`, readiness `ready`, health `healthy`, dan proses terpisah; `lsof` hanya menunjukkan dua listener `127.0.0.1:<client>` dan `127.0.0.1:<monitoring>` dan tanpa UDP; `/varz` memberi `server_name` sama dengan nama instance, `version` `2.15.1`, `store_dir` di bawah folder data instance, `max_memory` 1073741824 dan `max_storage` 10737418240, dan `<data>/jetstream` ada; log `Server is ready` sampai ke `LogSink` perekam; 60 detik sesudah ready spool tumbuh kurang dari 3 baris (bukan satu per percobaan, sekitar 300); stop membuat PID hilang dalam 10 detik, `stopped`, `lastFailure` null, `leftoverProcesses` kosong; start berikutnya `ready` lagi dengan PID lain → AC-8
- [x] `cargo test -p stacklyd --test nats --locked -- --ignored nats_compatibility` → lulus: `init`, `start`, `readiness`; `SIGSTOP` membuat readiness `not_ready` dengan reason `timeout` dan detail `no INFO from the server…` sementara connect TCP tetap berhasil, health tidak `healthy`, lifecycle `running` dengan PID dan waktu start sama, dan `SIGCONT` memulihkan `ready` (`honest_readiness`); server asing `-n foreign` memberi `identity_mismatch` (`server_name is "foreign", expected "golden"`), `--user u --pass p` `auth_required`, dan server yang cocok sukses (`identity_check`); `-n golden` tanpa `-js` dengan harapan aktif `jetstream_disabled`, `-js -sd <TestRoot>/foreign-js/store` dengan harapan mati `jetstream_unexpected`, yang cocok sukses, dan instance `{ "jetstream": false }` `ready` dengan `/varz` tanpa `jetstream.config` dan tanpa folder `<data>/jetstream` (`jetstream_mode`); setelah `SIGKILL` daemon kedua server tetap hidup, daemon baru mengadopsi PID dan waktu start yang sama, `lsof -d 3` menunjukkan `instance.lock`, dan readiness `ready` (`adoption`); stop kedua instance tanpa sisa (`stop`) → AC-9, AC-10, AC-11
- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test nats --locked -- --ignored` → `2 passed`, `evidence: 2 native records (NOT_RUN 0, PASS 2, FAIL 0, BLOCKED 0, DEFERRED 0), 0 problems` → AC-8, AC-9, AC-10, AC-11, AC-12
- [x] `cargo xtask runtime compat-record target/evidence/01a16181-5b99-7703-9d30-90b6b9a3d09d/report.json` → mencetak `kept the evidence in evidence/compatibility/01a16181-5b99-7703-9d30-90b6b9a3d09d`; `compatibility.json` memuat `checks` `adoption`, `honest_readiness`, `identity_check`, `init`, `jetstream_mode`, `readiness`, `start`, `stop` dan `knownLimitations` `client_auth_not_configured`, `jetstream_persistence_not_covered`, `multi_instance_not_covered` → AC-12
- [x] `cargo xtask catalog check`, `cargo xtask provider check`, `cargo xtask runtime check`, dan `cargo xtask codegen --check` → `ok catalog revision 4 75dc6fc6…5c46`, `ok official/nats 1.0.0 91d3c6da…9235`, `runtime check: 8 artifact(s), 7 evidence run(s), and 1 compatibility run(s) follow the rules`, `20 generated files are up to date` → AC-12
- [x] Setelah run: `pgrep -fl nats-server` tidak menunjukkan proses test dan folder `stackly-test-*` sudah terhapus → AC-8

### Acceptance-criteria coverage

- AC-8 · `nats_lifecycle_in_process`; endpoint `Fixed` bukan `Auto` (penyimpangan di atas)
- AC-9 · bagian `honest_readiness` di `nats_compatibility`
- AC-10 · bagian `identity_check` dan `jetstream_mode` di `nats_compatibility`
- AC-11 · bagian `adoption` di `nats_compatibility`
- AC-12 · guard `native` di kedua test dengan `covers` sesuai spec (`nats_compatibility`: `BR-025`, `REQ-029`, `REQ-036`, `AC-029`, `REQ-012`, `REQ-013`; `nats_lifecycle_in_process`: `REQ-029`, `REQ-036`, `AC-029`), `compatibility.json` dari `provider.yaml` final, jalur `BLOCKED`, evidence di `evidence/compatibility/`, dan catalog revision 4
- AC-13 dan AC-14 · milik milestone 4 (dulu disebut milestone 3)

## Milestone 3: Endpoint Auto tanpa render saat init, dan evidence ulang

Langkah Build plan yang tercakup: 13 dan 14 (sesi `dev-f23-m3`). Run native memakai artefak `nats-2.15.1-macos-aarch64` dari cache runtime xtask, tanpa jaringan; tanpa artefak kedua test berakhir `BLOCKED` seperti di milestone 2.

Penyimpangan dari spec, dicatat untuk lead:

- Langkah 13 menyebut `apps/cli/tests/golden/provider_list.json`, `provider_inspect_nats.*`, `apps/cli/tests/provider/inspect.rs`, dan `ProviderFactsTests.swift` sebagai file yang mengikuti revision dan digest NATS. File itu berada di luar daftar "BOLEH diubah" brief, tetapi tanpa pembaruan itu test workspace dan Swift gagal; hanya nilai digest dan `providerRevision`/`Provider revision` entri `official/nats` yang diganti (`1.0.1` dan `fe57c6f49a92…`).
- `xtask/tests/provider_check.rs` mengubah `primitive: renderConfig` di salinan NATS menjadi `runShell` untuk memicu `unknown_primitive`; langkah itu sudah tidak ada, jadi tes sekarang mengubah `createManagedDirectories`. Hanya satu baris mutasi, di luar daftar "BOLEH diubah" tetapi tes workspace gagal tanpanya.
- Catalog revision 7 (revision 6 diambil F19) dicatat dua kali: `cargo xtask catalog check --update` di commit paket, lalu `--update --amend` di commit evidence karena `providers/nats/versions.json` menambah record.

### UI / manual

- [x] Setelah run native dari `HEAD` bersih (`7e741ffb`), baca `target/evidence/01a16250-1438-776c-9d1a-91bc752be6a9/report.json` → 2 record `native`, keduanya `PASS`, 0 problem, subject `nats` versi `2.15.1`; `compat-record` menyimpannya ke `evidence/compatibility/01a16250-1438-776c-9d1a-91bc752be6a9/` dengan `providerRevision` `1.0.1` dan `manifestSha256` `fe57c6f49a92c7aed14ff76cc961e0934c623a0284d6e7f89f2a573b12145cbd`; evidence `1.0.0` tetap sebagai riwayat di `providers/nats/versions.json` → AC-15, AC-12
- [x] `nats.conf` tidak ada setelah create dan baru ada setelah start pertama dengan `listen` dan `http` pada port efektif dari `instance.get` (cocok dengan `lsof`) → AC-8, AC-15

### Commands

- [x] `cargo test -p stackly-core --test provider_packages` → 33 test lulus, termasuk `nats_initializes_without_ports_and_renders_nats_conf_only_for_the_run` (`init_plan` dengan kedua endpoint tanpa port hanya berisi `EnsureDirectories`, `run_plan` dengan port menulis `nats.conf`, `run_plan` tanpa port `EndpointUnresolved`) dan `every_package_initializes_with_endpoints_that_have_no_port_except_traefik` (fake, nats, postgresql berhasil; traefik masih `EndpointUnresolved` sampai fitur 41); golden `crates/core/tests/golden/provider/nats/init.json` hanya kehilangan `WriteFile` `nats.conf`, `run.json` dan `render-natsConf.txt` tidak berubah → AC-15
- [x] `cargo xtask provider check` → `ok official/nats 1.0.1 fe57c6f49a92c7aed14ff76cc961e0934c623a0284d6e7f89f2a573b12145cbd` → AC-15
- [x] `cargo xtask catalog check`, `cargo xtask runtime check`, `cargo xtask evidence check`, `cargo xtask codegen --check`, `cargo xtask check-deps` → bersih; `ok catalog revision 7 ef635bea…6cdca`, `3 compatibility run(s) follow the rules` → AC-12, AC-15
- [x] `STACKLY_UPDATE_GOLDEN=1 cargo test -p stacklyd --test create_form_fixtures` → `protocols/fixtures/instance/create-form-nats.json` memuat digest `fe57c6f4…` dan revision `1.0.1`; `swift test --package-path clients/swift` lulus (192 test) → AC-15
- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test nats --locked -- --ignored` dari `HEAD` bersih → `2 passed`, `evidence: 2 native records (NOT_RUN 0, PASS 2, FAIL 0, BLOCKED 0, DEFERRED 0), 0 problems`; kedua instance dibuat dengan `PortRequest::Auto` untuk `client` dan `monitoring` → AC-8 (bagian `Auto`), AC-9, AC-10, AC-11, AC-12
- [x] Setelah run: tidak ada `nats-server` milik test yang tersisa dan folder `stackly-test-*` terhapus → AC-8

### Acceptance-criteria coverage

- AC-15 · paket `1.0.1` dan test core (langkah 13); evidence ulang (langkah 14)
- AC-8 · bagian `Auto` selesai di langkah 14 (`create_params` memakai `Auto`; penanda `// provisional: F20` dihapus); penyimpangan `Fixed` di milestone 2 tidak berlaku lagi
- AC-9 sampai AC-12 · dijalankan ulang di langkah 14 pada paket `1.0.1`
- AC-4 · digantikan nilai AC-15 (revision `1.0.1`, digest `fe57c6f4…`)
- AC-13 dan AC-14 · milik milestone 4

## Milestone 4: Journey yang sama dari CLI dan GUI, dan pemeriksaan RISK-07

Langkah Build plan yang tercakup: 10, 11, dan 12 (sesi `dev-f23-m4`, 2026-10-22). Test CLI memakai `stacklyd` dan `stackly` asli, runtime NATS 2.15.1 dari cache runtime xtask, dan tidak mengunduh apa pun; tanpa artefak test berakhir `BLOCKED` seperti di milestone 2.

Penyimpangan dari spec, dicatat untuk lead dan architect:

- Spec menyebut catalog uji dari `CatalogBuilder` (`stackly_core::catalog::testing`). Crate `stackly-cli` tidak boleh bergantung pada `stackly-core`, juga bukan sebagai dev dependency (`cargo xtask check-deps`: "the CLI never runs provider binaries or writes the registry"), jadi `nats_journey` memakai jalur yang sama dengan `apps/cli/tests/provider/stacklyd.rs`: `cargo xtask catalog build` dari `providers/` ke `STACKLY_CATALOG_DIR`, tempat `official/nats` sudah `[[provider]]`. Runtime masuk lewat `stackly runtime install official/nats@2.15.1 --wait`: test menyalin artefak dari cache xtask ke `cache/downloads/<sha256>` di root uji, jadi tahap download adalah cache hit dan seluruh jalur install (verify, extract, validate, commit) tetap nyata.
- `stackly instance start`, `stop`, dan `get` hanya menerima ID penuh; `inspect` dan `connection` menerima nama. Test mengikuti itu: `start` dan `stop` dengan ID dari hasil create, `inspect` dengan nama.
- Langkah 11 tidak memerlukan kode form baru. Test Swift milik spec 0020 sudah membuktikan baris, kontrol, dan batas memori. Yang ditambahkan: baris `serverName` managed (tidak dikirim, tidak "Required", ada di grup basic), toggle JetStream (default aktif, `false` terkirim), ketiga integer advanced dengan hint batas, default, dan aturan `restart`, serta dua baris endpoint `client` dan `monitoring` dengan host, tujuan, port default, dan perpindahan Fixed ke Automatic yang dikirim sebagai `auto`.

### UI / manual

- [ ] `NOT_RUN` (langkah 12, butuh izin user) Sheet New Instance untuk NATS di app development dengan root sementara: pilih runtime NATS 2.15.1 yang terpasang, isi nama, dan lihat baris "Server name" terkunci (read only, tidak bisa diketik), toggle JetStream aktif, grup Advanced tertutup dengan tiga angka dan batas nya, dan dua baris endpoint Client (4222) dan Monitoring (8222) dengan pilihan Fixed atau Automatic. Biarkan Automatic, nyalakan "Start after creating", tekan Create → sheet mengikuti operasi sampai selesai, instance muncul di Instances → AC-13
- [ ] `NOT_RUN` (langkah 12, butuh izin user) Buka halaman detail instance NATS itu → tab Summary menampilkan Process "Running" dan Readiness "Ready" sebagai dua baris terpisah, kartu Endpoints menunjuk port yang dipilih daemon, dan kartu Connection `nats://127.0.0.1:<port>`. Ulangi dengan JetStream dimatikan → readiness tetap "Ready" → AC-13
- [ ] `NOT_RUN` (langkah 12, butuh izin user) Seluruh alur di atas dengan keyboard saja (Tab melewati nama, runtime, setiap baris form, Advanced, kedua baris endpoint, toggle Start after creating, Create, dan Cancel; Return membuka Advanced; Escape membatalkan) dan dengan VoiceOver (baris managed dibacakan sebagai read only dengan nilai nya, toggle JetStream sebagai switch, setiap batas integer dibacakan, pilihan Fixed atau Automatic berlabel per endpoint, dan hasil Create diumumkan) → AC-13

### Commands

- [x] `cargo test -p stackly-cli --test nats_journey -- --ignored` → lulus dalam sekitar 7 detik: root uji baru, `stacklyd` dengan catalog dari `providers/`, `runtime install official/nats@2.15.1` dari cache, `instance create bus` dengan default (kedua endpoint `Auto`) lalu `start`, `inspect bus --json` dengan `lifecycle` `running`, `readiness.state` `ready`, `reason` `probe_ok`, dua port berbeda, baris INFO dari port `client` dengan `server_name` `bus`, `jetstream` `true`, versi `2.15.1`, lalu `stop` dan `inspect` yang menunjukkan `stopped` dan bukan `ready`; instance kedua `light` dengan `--set jetstream=false` melewati jalur yang sama dan INFO tidak memuat `jetstream: true` → AC-13
- [x] `cargo xtask evidence run --kind native --out evidence/connection/nats-journey-20261022 -- cargo test -p stackly-cli --test nats_journey --locked -- --ignored` dari `HEAD` bersih (`0122207f`) → `1 native records (NOT_RUN 0, PASS 1, FAIL 0, BLOCKED 0, DEFERRED 0), 0 problems`, subject `nats` `2.15.1` dengan digest artefak; folder di `evidence/connection/nats-journey-20261022/` → AC-13
- [x] `cargo test -p stacklyd --test create_form_fixtures` → `the_create_form_of_each_real_package_is_its_fixture` lulus, jadi `protocols/fixtures/instance/create-form-nats.json` masih sama dengan jawaban `instance.get_create_form` dari paket NATS `1.0.1`; `swift test --package-path clients/swift` → 217 test lulus → AC-13
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter CreateInstance` dan `--filter SchemaFormModel` → lulus, termasuk `theNatsServerNameIsAManagedRowThatStaysInTheBasicGroup`, `theNatsJetStreamToggleStartsOnAndIsSentWhenItIsSwitchedOff`, `eachNatsAdvancedIntegerHasItsBoundsDefaultAndRestartRule`, dan `theNatsFormHasItsClientAndMonitoringRowsWithFixedOrAutomaticPorts`; `xcrun swift-format lint --strict` pada file Swift yang diubah bersih → AC-13
- [x] `cargo fmt --all --check`, `cargo clippy -p stackly-cli --all-targets --locked -- -D warnings`, `cargo xtask check-deps`, dan `cargo xtask evidence check` → bersih (`59 manual record(s), baseline valid, 797 Rust and 490 Swift file(s) follow the rules`) → AC-13

### Ukuran RISK-07 (AC-14)

Commit dasar: `8aa1b3cb` (commit tepat sebelum commit build pertama fitur ini, `e257d26b`).

Commit build fitur 23 yang dihitung, berurutan: `e257d26b` (langkah 0), `e2651ede` (3), `63d7a54b` (4), `ccbb358f` (4, fuzz run), `06c095c4` (1), `f5c1ea85` (5), `746c1be2` (6), `f98779ae` (evidence run ulang fuzz, langkah 4), `c556ac5f` (2), `37f192c5` (2, xtask test), `a7a03d20` (7), `116e269e` (8), `27e98df3` (9), `4b4b9555` (13), `7e741ffb` (14), `7664295c` (14, evidence), `7e438c9e` (13, xtask test), `82c68c37` (10), dan `0122207f` (11). Commit spec 0018, 0019, 0020, dan 0021 yang berada di antara nya dan commit docs tidak dihitung. Skrip yang dipakai ada di bawah tabel.

Baris yang ditambahkan dan memuat `nats` (tanpa peduli huruf besar) ke file sumber non test, per commit (`git diff -U0 <commit>^ <commit>`, hanya baris berawalan `+`):

| Commit | File allowlist | File di luar allowlist |
| --- | --- | --- |
| `e2651ede` | `provider/probe/nats.rs` 22, `provider/probe/mod.rs` 1 | tidak ada |
| `63d7a54b` | `provider/fuzzing.rs` 21, `xtask/src/fuzz.rs` 18 | tidak ada |
| `06c095c4` | `provider/probe/native.rs` 19 | tidak ada |
| `f5c1ea85` | `supervisor/probe/native.rs` 17 | tidak ada |
| semua commit lain di daftar | tidak ada | tidak ada |

Semua jalur `crates/core/src/...` di atas ditulis tanpa awalan. Tidak ada baris `nats` di file sumber non test mana pun di luar allowlist. Tidak ada commit dalam daftar yang menyentuh file sumber non test di `apps/`, `clients/`, `crates/domain/`, `crates/platform/`, `crates/platform-macos/`, `crates/protocol/`, `crates/storage/`, atau `crates/ipc/`, jadi `ProtocolVersion::CURRENT` tidak berubah dan tidak ada seam baris 4 yang perlu dicatat dengan alasan (`pin_package` dan `DaemonConfig.extra_log_sinks` dibangun spec 0010 langkah 13; `crates/core/src/supervisor/fixture.rs` dan `apps/daemon/src` tidak berubah oleh commit fitur ini). Baris 4 tabel: 0 baris tambah, 0 hapus. Dua commit terakhir (`82c68c37`, `0122207f`) hanya menambah test.

`git diff --numstat` gabungan semua commit di atas, hanya file sumber non test (tambah, hapus):

| Baris tabel | File | Tambah | Hapus |
| --- | --- | --- | --- |
| 1 (generik) | `provider/raw.rs` | 92 | 0 |
| 1 | `provider/validate.rs` | 163 | 5 |
| 1 | `provider/manifest.rs` | 19 | 0 |
| 1 | `provider/plan.rs` | 91 | 6 |
| 1 | `provider/interpolate.rs` | 38 | 0 |
| 1 | `provider/mod.rs` | 4 | 3 |
| 1 | `provider/probe/mod.rs` | 40 | 0 |
| 1 | `provider/probe/native.rs` (termasuk 19 baris varian NATS, baris 2) | 237 | 0 |
| 1 | `supervisor/probe/native.rs` (driver generik, termasuk 17 baris cabang NATS, baris 2) | 570 | 11 |
| 1 | `supervisor/probe/mod.rs` | 8 | 2 |
| 1 | `supervisor/deps.rs`, `readiness.rs`, `reload.rs`, `probe/exec.rs` | 9 | 0 |
| 2 (khusus NATS) | `provider/probe/nats.rs` | 641 | 1 |
| 3 (fuzz, test dan tooling) | `provider/fuzzing.rs` | 171 | 0 |
| 3 | `xtask/src/fuzz.rs` | 78 | 2 |
| 4 (seam bersama) | `supervisor/fixture.rs`, `apps/daemon/src` | 0 | 0 |

Tabel spec tidak memuat angka target, jadi tidak ada selisih yang dikembalikan ke `/architect`. Tiga hal untuk dibaca: baris 1 sebagian besar adalah engine generik yang dipakai probe native berikutnya (termasuk test inline di `supervisor/probe/native.rs`); khusus NATS di luar baris 3 hanya `nats.rs` (642 baris, state machine dan test transcript nya) ditambah 36 baris `nats` di `native.rs` dan `supervisor/probe/native.rs`; dan tidak ada kode Rust atau Swift khusus NATS di daemon, CLI, protokol, storage, platform, domain, atau SwiftUI.

Skrip hitung (jalankan dari root repo): untuk setiap commit `c` dalam daftar, `git diff -U0 c^ c`, kumpulkan baris `+` yang cocok dengan `/nats/i` per file, abaikan file test (`tests/`, `fixtures`, golden, `docs/`, `evidence/`, `providers/`, `protocols/`, `schemas/`, `fuzz/`, dan berkas non kode), dan bandingkan dengan allowlist.

### Acceptance-criteria coverage

- AC-13 · bagian CLI selesai di langkah 10 (`nats_journey`, PASS native); bagian form selesai di langkah 11 (fixture masih lolos kontrak Rust dan Swift, empat assertion baru); bagian GUI (sheet New Instance, Summary, keyboard, VoiceOver) `NOT_RUN`, tiga langkah manual di atas untuk `/check verify` dan user
- AC-14 · selesai di langkah 12: commit dasar, daftar commit, baris per commit terhadap allowlist, numstat per baris tabel, dan seam baris 4 tercatat di atas
