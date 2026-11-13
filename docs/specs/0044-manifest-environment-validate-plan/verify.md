# Verify: Manifest environment, validate dan plan · spec 0044 · updated 2026-11-13

_Langkah diturunkan dari acceptance criteria spec 0044. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27._

## Milestone 1: Manifest, validate, dan plan di daemon

_Bagian ini mencakup Build plan langkah 1 sampai 5: domain dan storage, core manifest dan validate, core plan dan status, protokol 4.2, lalu handler `env.*` di `stacklyd` dengan operasi `import_environment` dan `remove_environment`, cache keanggotaan, dan test lewat socket. Langkah langkah 5 ada di bawah tanda "Langkah 5"._

### UI / manual

- [x] Buka `schemas/environment/environment.v1alpha1.schema.json` → schema draft 2020-12 berjudul `Environment`, `additionalProperties: false` di setiap objek dengan properti, `services` berupa map ke `Service`, `port` berupa angka 1 sampai 65535 atau `auto`; file diawali komentar apa pun tidak ada (JSON murni) → AC-24
- [x] Buka `protocols/error-codes.json` → area `env` milik spec 0044 berisi dua belas kode tabel *Kode error di wire* dengan kategori dan `details` persis seperti spec, semuanya `retryable` false; `env.lease_active` membawa `items` kind `instance` (field `instanceId`, `name`, paling banyak 32) dan detail `omitted`; area `cli` mendapat `cli.env_invalid`, `cli.env_plan_blocked`, dan `cli.environment_not_found` → AC-24
- [x] Buka `crates/core/tests/fixtures/environments/erp-development.yaml` → enam service dengan nama endpoint, secret field, setting, dan line yang dipakai keenam paket resmi (tabel *Manifest contoh ERP*), tanpa `dependsOn` → AC-1

### Commands

- [x] `cargo test -p stackly-domain --lib environment` → `set_manifest` menaikkan revision 1 dan menolak alias ganda atau `dependsOn` ke alias yang tidak dideklarasikan tanpa mengubah revision; `ManifestLocation` hanya menerima path absolut tanpa `.` atau `..` → AC-11, AC-12
- [x] `cargo test -p stackly-storage --test open environments` → registry versi 13 bermigrasi ke 14 dengan semua baris tetap dan snapshot kosong; fixture beku v14 membaca snapshot `services` (urutan manifest, `dependsOn` dengan `timeoutSeconds`, service shared dengan `instanceRef`); tabel menolak nama kedua, lokasi manifest kedua, dan `services` yang bukan array JSON → AC-11, AC-12
- [x] `cargo test -p stackly-storage --test open fixture` → `registry-v14.sql` tercatat di `FIXTURE_SHA256` dan dumper menghasilkannya lagi dari `seed/v14.sql` → AC-11
- [x] `cargo test -p stackly-storage --test registry` dan `cargo test -p stackly-testkit` → suite conformance (SQLite dan fake) lulus, termasuk skenario `environments`: snapshot dan lokasi terbaca kembali, nama dan lokasi unik (`Integrity` di op 0), lokasi yang dilepas bisa diambil environment lain, guard membaca `GuardView::environments`, dan `DeleteEnvironment` menolak lease `active` dengan `NotDeletable` `active_lease`, membandingkan revision, lalu menghapus demand `released` dan pemakaian secret, memindahkan reference milik environment ke user (revision naik 1), dan menyisakan instance → AC-11, AC-12, AC-13
- [x] `cargo test -p stackly-core --test environment validate` → manifest contoh ERP lolos tanpa finding; setiap file di `fixtures/environments/invalid/` (satu per kode `FindingCode`, 35 file sejak `provider_single_instance`) menghasilkan kodenya sendiri dan pesannya selalu template kode itu; cycle tiga service bernama `api -> db -> cache -> api` muncul bersama `self_dependency`, `unknown_dependency`, dan `endpoint_conflict` yang menunjuk kedua endpoint; setiap pasangan di `fixtures/environments/aliases/` (`block-scalar`, `exotic3`, `by1`, `by2`, `by3`, `shared-settings`, `alias-brings-a-finding`) memberi `services` dan finding yang sama dengan kembarannya tanpa alias, tanpa `yaml_alias`; tag lokal, key bertag, dan merge key ditolak `yaml_alias` di path field (`services.web.<<`); `*tidak-ada` dan dua dokumen menjadi `yaml_syntax`; tiga bom di bawah 256 KiB ditolak dalam 2 detik; kutip multi baris di barisnya sendiri dan block scalar tetap valid *(diubah 2026-11-13, K2 baru)*; versi tanpa kutip meminta kutip; sentinel di `secretRefs`, `settings`, port, dan posisi bertipe salah tidak pernah muncul di finding atau snapshot → AC-1, AC-2, AC-3, AC-8
- [x] `cargo test -p stackly-core --test environment plan` → di root kosong plan manifest contoh ERP berisi enam `install_runtime`, enam `create_instance`, enam `start_instance`, nol `keep`, `applicable` true, lima reference `unregistered` (`needsSecrets` 5), dan `planDigest`; urutan start fixture `graph.yaml` `db, cache, api, web`; plan tidak menaikkan generation registry, tidak mengubah satu file pun di test root, dan tidak memanggil proses, tulis secret, atau unduhan → AC-5, AC-6, AC-10, AC-25 (bagian core)
- [x] `cargo test -p stackly-core --test environment plan::a_fixed_port_of_another_instance_and_a_taken_name_block_the_plan` dan `plan::a_port_in_use_and_an_unresolved_selector_block_the_plan` → blocker `supervisor.endpoint_conflict`, `instance.name_taken`, `supervisor.endpoint_in_use`, dan `runtime.selector_unresolved`, `applicable` false, tanpa digest → AC-7
- [x] `cargo test -p stackly-core --test environment plan::a_name_registered_at_another_location_is_name_taken` dan `plan::a_registered_source_whose_file_declares_another_name_stops_with_manifest_renamed` → `registration` `name_taken` dengan blocker `env.name_taken`; sumber `registered` dengan nama lain berhenti dengan `env.manifest_renamed` dan `items` kosong → AC-7, AC-11
- [x] `cargo test -p stackly-core --test environment plan::an_instance_ref_is_kept_or_started_and_never_created` → `env.instance_ref_not_found`, `keep` saat berjalan, `start` saat berhenti, `env.instance_ref_mismatch` untuk provider lain, tidak pernah `create` → AC-8
- [x] `cargo test -p stackly-core --test environment plan::a_member_follows_the_lifecycle_table_reports_differences_and_detached` → setiap baris *Lifecycle ke aksi* (termasuk `uninitialized` dan `stopped` tanpa data sebagai `env.instance_blocked` `not_initialized`, dan blocker instance pada `start`), `differences` `runtime` dan `endpoint.http.port` yang tidak pernah menjadi blocker, dan member `old` di `detached` → AC-9
- [x] `cargo test -p stackly-core --test environment plan::the_digest_ignores_member_lifecycle_and_ports_but_follows_what_up_creates` → digest sama saat member pindah `stopped` ke `running` dan saat port efektifnya berubah; berbeda saat endpoint item `create` berubah dan saat member diganti instance lain → AC-5
- [x] `cargo test -p stackly-core --test environment plan::bound_references_show_present_missing_unregistered_and_generated` → status `present`, `missing`, `unregistered`, `generated`; `needsSecrets` 2, `applicable` tetap true, nilai dari store tidak muncul di plan → AC-10
- [x] `cargo test -p stackly-core --test environment status` → status `partially_ready` dan `down`, `readyCount` dan `serviceCount`, alasan `stopped` dan `not_created`, member yang tidak dideklarasikan; `manifestState` `missing`, `unreadable`, `in_sync`, `name_changed`, `changed`, `invalid` dengan jumlah problem → AC-14
- [x] `cargo test -p stackly-core --test create_evaluate --test create_operation` → golden preflight dan create tidak berubah sesudah `planned_runtime` dan `settings_with_references` → AC-5 (Konsekuensi)
- [x] `cargo test -p stackly-protocol --no-fail-fast` → test katalog (area `env` terdaftar dengan pemiliknya, `items` dengan `omitted`, tabel `cli`), `CONTRACT_LIMITS` (`envFindingsMax`, `envListMax`, `envServicesMax`), tabel method (enam `env.*`, semuanya `recovery refused`), `ipc_fixtures` (fixture roundtrip setiap method baru, dua `reject`, tiga error `env.*`, `decode` untuk status yang tidak dikenal, dan `dto-instance-summary-environment-member.json` dengan `ownership` `dedicated` dan `environments`), dan `env_fixtures` (fixture `protocols/fixtures/env/` roundtrip, enum terbuka membaca nilai baru sebagai `unrecognized`, setiap kode `env` punya hint statis) → AC-15 (bagian protokol), AC-24
- [x] `swift test --package-path clients/swift` → `EnvironmentContractTests` membaca fixture `env/` dan `frame-response-env-list-unknown-values.json` (`.unrecognized`), `IpcFixtureTests` menghitung 73 method, `ContractTests` protokol 4.2 → AC-24
- [x] `cargo xtask codegen --check` → semua file hasil generate mutakhir, termasuk `schemas/environment/environment.v1alpha1.schema.json` dan entri 4.2 di `protocols/contract-versions.json` → AC-24
- [x] `cargo xtask check-deps` dan `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → lulus → AC-24

Langkah 5 (handler `env.*`, cache keanggotaan, operasi import dan remove). Butuh `cargo build -p stackly-fake-server` lebih dulu.

- [x] `cargo test -p stackly-core --test environment register` → guard import: nama baru menulis baris di revision 1, file yang sama lagi tidak menulis apa pun, digest baru menaikkan revision ke 2 dengan ID yang sama; nama di lokasi lain `NameTaken` dengan lokasinya, relink memindahkan lokasi dan mempertahankan ID, relink ke file bernama lain `RelinkMismatch`, relink ke ID yang tidak ada `NotFound`; file yang kini mendeklarasikan nama lain mengambil lokasinya dan environment lama kehilangan lokasi (revision naik); environment ke 65 `LimitReached`; guard remove menolak revision lain dan ID yang tidak ada lalu menghapus; `manifest_location` lewat folder symlink sama dengan path langsung → AC-11, AC-12, AC-13
- [x] `cargo test -p stacklyd --test environment validate_reads_the_file_answers_every_finding_and_writes_nothing` → `env.validate` manifest contoh ERP `valid` dengan enam service, digest 64 karakter, dan `supportedApiVersions` `stackly/v1alpha1`; manifest dengan cycle dan provider tak dikenal tetap balasan sukses dengan `dependency_cycle` dan `unknown_provider`; path yang tidak ada `env.manifest_unreadable` `not_found`, symlink dan folder `not_a_file`, semuanya dengan hint; generation registry, jumlah operasi, jumlah audit, dan pohon file test root (path, ukuran, mtime) sama sebelum dan sesudah → AC-4, AC-6
- [x] `cargo test -p stacklyd --test environment a_plan_reads_this_mac_and_starts_no_operation` → `env.plan` manifest contoh ERP lewat socket: enam item `create` dalam urutan manifest, masing masing `install_runtime`, `create_instance`, `start_instance`, trust terisi, `registration` `new`, digest ada saat `applicable` (port yang sedang dipakai proses lain di Mac ini hanya boleh menjadi blocker `supervisor.endpoint_in_use`); `graph.yaml` berurutan `db, cache, api, web` dengan `web` menunggu `api`; tidak ada operasi, audit, instance, atau file baru → AC-5, AC-6, AC-25 (bagian socket)
- [x] `cargo test -p stacklyd --test environment the_reply_of_a_plan_of_32_services_stays_under_900_kib` → frame balasan plan 32 service di bawah 900 KiB → AC-6 (*Anggaran frame*)
- [x] `cargo test -p stacklyd --test environment a_secret_sentinel_in_the_manifest_never_leaves_the_file` → sentinel di `secretRefs`, di `settings`, di posisi bertipe salah (map dan port), dan sebagai `local:instance/...` ditolak (`secret_literal`, `secret_in_settings`, import `env.manifest_invalid`), dan sentinel tidak ada di balasan validate, plan, dan import, di log daemon, maupun di file mana pun di test root di luar folder proyek (registry dan WAL termasuk) → AC-3
- [x] `cargo test -p stacklyd --test environment an_import_registers_the_file_once_and_a_repeated_key_runs_no_second_operation` → import pertama membalas `EnvironmentDetail` revision 1 dengan `manifestLocation` kanonik, satu operasi dan satu baris audit baru; key yang sama tidak memulai operasi kedua; file yang sama lewat folder symlink dengan key baru tetap revision 1 tetapi tercatat sebagai operasi; digest lama sesudah file berubah `env.manifest_changed`, digest baru menaikkan revision ke 2; file tidak valid `env.manifest_invalid` dengan `errors` dan hint → AC-11, AC-12
- [x] `cargo test -p stacklyd --test environment a_name_at_another_location_is_taken_until_a_relink_moves_it` → plan file kedua bernama sama `registration` `name_taken` dengan blocker `env.name_taken`, import nya ditolak dengan `name` dan `location`; relink ke file bernama lain `env.relink_mismatch`; relink memindahkan lokasi dengan ID yang sama; file yang kini mendeklarasikan nama lain membuat environment lama tanpa lokasi dan `manifestState` `missing` → AC-11, AC-12
- [x] `cargo test -p stacklyd --test environment the_65th_environment_is_refused` → 64 import berhasil, yang ke 65 `env.limit_reached` dengan `limit` 64, `env.list` berisi 64 → AC-12, AC-14
- [x] `cargo test -p stacklyd --test environment membership_shows_on_every_summary_and_remove_waits_for_the_lease` → service tanpa instance `not_created`; member `dedicated` yang ditulis langsung ke registry (seperti Up spec 0046 nanti) tampil `ownership` `dedicated` dengan satu entri `environments` (`shop`, `db`, `dedicated`) yang sama persis di `instance.get`, `instance.list`, dan event `instance.state_changed`; `env.list` dan `env.get` menghitung dan menunjuk instance nya; remove dengan revision lama `resource.revision_mismatch`, dengan lease `active` `env.lease_active` (`count` 1, item berisi ID dan nama instance); sesudah lease dilepas remove berhasil, key yang sama membalas hal yang sama, `env.get` `env.not_found`, instance tetap ada dengan `ownership` `manual` dan `environments` kosong, file manifest tetap ada → AC-13, AC-14, AC-15
- [x] `cargo test -p stacklyd --test environment get_reads_the_manifest_only_when_asked` → `checkManifest` true memberi `in_sync`, `changed`, `name_changed`, `invalid` (dengan `problems` lebih dari 0), dan `missing` sesuai isi file; `checkManifest` false tidak mengirim `manifestState` dan `problems`; plan sumber `registered` yang filenya hilang `env.manifest_unreadable`; ID yang tidak dikenal `env.not_found`; status environment tanpa instance `down` → AC-14
- [x] `cargo test -p stacklyd --lib membership` → instance dengan member `dedicated` dan `shared` menjadi `dedicated` dengan entri dedicated lebih dulu lalu menurut nama; instance yang hanya dipinjam `shared`; instance tanpa environment `manual` → AC-15
- [x] `cargo test -p stacklyd --no-fail-fast` dan `cargo test -p stackly-core --no-fail-fast` → lulus; golden preflight, create, doctor, dan `instance.*` lain tidak berubah karena `ownership` dan `environments` hanya terisi untuk member → AC-15 _(Dicentang 2026-11-13 atas dasar gerbang lead di `main`, tidak diulang checker: `stackly-core` 1763, `stacklyd` 737, `stackly-cli` 638, suite kecil 1135, StacklyUI dan `build-app` pada `be9ff7a3`.)_

### Catatan `/check verify` 2026-11-13 (sesi `chk-f44-m1`, kode `fa0eedcb`)

Verdict Milestone 1: **FAIL**, karena dua celah sempit (AC-9 dan AC-1). Semua langkah lain lulus. Selain test di atas, `stacklyd` asli dijalankan di root sementara dengan catalog resmi hasil `cargo xtask catalog build`, lalu dipanggil lewat socket.

Dua langkah yang tidak dicentang:

- `plan::a_member_follows_the_lifecycle_table...`: test nya lulus, tetapi baris `retained` tabel *Lifecycle ke aksi* tidak diuji, dan lewat daemon baris itu gagal (temuan 1).
- Suite penuh `stacklyd` dan `stackly-core`: tidak diulang checker atas instruksi brief (disk). Lead menjalankannya di `main` pada `fa0eedcb` (734 dan 1719 lulus).

Temuan:

1. **AC-9, AC-14, sedang, belum bisa terjadi hari ini.** Member yang instance nya `retained` hilang dari plan: tidak ada item, tidak ada blocker `env.instance_blocked`, `applicable` tetap true dan `planDigest` terisi; `env.get` menampilkan `not_created`, bukan `unavailable`. Sebabnya `instances()` di `apps/daemon/src/ipc/env/mod.rs` memakai `InstanceFilter::default()` (tanpa `retained`) dan `existing_item` di `crates/core/src/environment/plan.rs` mengembalikan `None` saat view nya tidak ada. Dibuktikan dengan test sementara (tidak di commit) yang menulis member lalu `LifecycleEvent::Retain` langsung ke registry. Belum ada kode produksi yang membuat instance `retained`, tetapi guard `accept` spec 0046 AC-4 bergantung pada baris ini.
2. **AC-1, rendah.** Pindaian teks anchor dan alias bisa dilewati: key sesudah block scalar di dalam item sequence (`- service: |-` lalu `condition: &c ready`, dan `condition: *c` di service lain) dianggap isi block scalar. `env.validate` pada daemon asli membalas `valid` true untuk file itu. Alias tetap diperiksa semua aturan sesudah di expand, jadi tidak ada nilai yang lolos; yang dilanggar hanya aturan ketat K2.
3. **AC-11, rendah.** Nama file dengan huruf besar berbeda (`Stackly.yaml` untuk `stackly.yaml`) terbaca sebagai lokasi lain di APFS: plan membalas `env.name_taken` untuk file yang sama. Folder induk sudah benar (`/tmp` dan `/private/tmp`, `PROJ2` dan `proj2` terbaca satu lokasi). Mekanisme di AC-11 dipenuhi; tujuan kalimatnya belum untuk komponen terakhir.
4. **AC-7, rendah.** Provider lokal `pending` atau `revoked` memberi dua blocker untuk satu masalah: `provider.approval_required` (atau `provider.revoked`) dan `catalog.not_installable` yang pesannya menyebut `stable` padahal manifest menulis `"1.0"`, tanpa hint.
5. **AC-14, rendah, sementara.** `EnvironmentSummary.updatedAt` bergeser pada import yang tidak mengubah apa pun (teramati `12:33:32.511Z` ke `12:33:33.781Z` dengan revision tetap 1), juga pada import atau remove yang gagal, dan menjadi waktu saat ini di setiap panggilan bila environment tidak punya baris operasi. Teks AC-14 tidak dilanggar; perbaikannya dijadwalkan di F46 milestone 1.

Yang dibuktikan lewat probe karena belum ada test nya (bahan `/test`): `install` false dan `runtimeId` terisi saat ada baris runtime `managed` `installed` ber digest artefak yang sama; blocker dan `trust` untuk provider lokal `pending` dan `revoked` tanpa mengubah approval; `planDigest` sama saat instance `instanceRef` benar benar di stop dan di start (port efektif ikut berganti). Belum teruji sama sekali: blocker `instance.capability_unsupported` (semua paket di catalog `multiInstance`), dan status secret `present` serta `missing` terhadap Keychain asli (hanya fake store).

Catatan teks spec: AC-1 menulis "menunjuk baris dan kolom" untuk field tak dikenal dan `apiVersion` atau `kind` lain, sedangkan *Aturan validasi* dan implementasi memberi path field (`services.db.endpoint`, `apiVersion`).

### Ulang `/check verify` Milestone 1, 2026-11-13 (sesi `chk-f44-m1`, kode `77d9ed02`)

Sesudah empat fix `dev-f44-fix`. Dijalankan: `cargo test -p stackly-core --test environment` (38 lulus), `cargo test -p stackly-core --lib environment` (4), `cargo test -p stacklyd --test environment` (11), lalu probe terhadap `stacklyd` asli.

- **Temuan 1 (AC-9, AC-14): PASS.** Member `retained` kini punya item `keep`, satu blocker `env.instance_blocked` ber `reason` `retained`, plan tidak `applicable`, dan `env.get` `unavailable` (test daemon `a_retained_member_blocks_the_plan_and_shows_unavailable`, test core `plan::a_retained_member_keeps_its_item_and_blocks_the_plan`, `status::a_retained_member_is_unavailable_and_a_retained_name_is_not_created`). `instanceRef` tidak pernah menunjuk instance `retained`. Langkah lifecycle di atas kini dicentang.
- **Temuan 2 (AC-1), bentuk yang dilaporkan: PASS.** File dengan anchor dan alias sesudah block scalar dalam item sequence kini ditolak di daemon asli (`yaml_alias` di `14:20` dan `21:20`); manifest ERP dan rule Traefik ber `&&` tetap valid.
- **Temuan 3 (AC-11): PASS.** Import lewat `Stackly.yaml` menyimpan lokasi `stackly.yaml`; plan lewat `STACKLY.YAML` `registered`; import lewat `STACKLY.yaml` "Already imported."; symlink tetap ditolak `env.manifest_unreadable`.
- **Temuan 4 (AC-7): PASS.** Provider `pending` memberi satu blocker `provider.approval_required`, `revoked` satu `provider.revoked`; selector yang tidak cocok ditulis apa adanya ("acme/demo has no version matching 7.7", `selector` `7.7`).

Temuan baru, rendah, masih AC-1: pindaian alias masih bisa dilewati dengan bentuk lain. Plain scalar yang bersambung ke baris berikutnya, dengan baris sambungan diawali tanda kutip (`rule: Host(...) || Header(`X-A`,` lalu baris `"v)`), membuat pindaian menganggap sisa file sebagai isi string, sehingga `cache: &c` dan `other: *c` sesudahnya tidak terlihat. `env.validate` daemon asli membalas `valid` true dengan tiga service. Sama seperti sebelumnya, nilai hasil expand tetap melewati semua aturan; yang dilanggar hanya K2. Karena ini bentuk kedua, perlu keputusan: pindaian dibuat konservatif (tidak pernah masuk mode kutip dari baris sambungan, atau menolak file bila ragu), atau teks AC-1 dan K2 dilonggarkan menjadi usaha terbaik.

### Ulang kedua `/check verify` Milestone 1, 2026-11-13 (sesi `chk-f44-m1`, kode `75784ccc` dan `11d5b0f8`)

Dijalankan: `cargo test -p stackly-core --lib environment` (7), `cargo test -p stackly-core --test environment` (41), `cargo test -p stacklyd --test environment` (12), lalu probe terhadap `stacklyd` asli.

- **AC-14 `updatedAt`: PASS.** Di daemon asli nilainya sama sesudah import yang tidak mengubah apa pun, sesudah import yang gagal (`env.name_taken`), dan sesudah remove yang gagal (`resource.revision_mismatch`); `env.list` dan `env.get` sama; bergeser hanya saat import mengubah baris (revision 2). Nilainya dibaca dari `environments.updated_at`, jadi tidak bergantung pada baris operasi (test `updated_at_is_the_row_stamp_so_an_unchanged_import_never_moves_it`).
- **AC-1 pindaian konservatif: contoh yang dilaporkan PASS, tetapi AC-1 masih FAIL.** `exotic3.yaml`, varian `|2-`, dan file lama kini ditolak; ERP, rule Traefik ber `&&` dan backtick, string berkutip multi baris sesudah `key: `, `'` di plain scalar, dan `&`, `*`, `"` di komentar tetap valid. Tiga bentuk baru masih lolos dengan `valid` true dan alias ter expand (empat service): plain scalar yang barisnya berakhir dengan `,` atau `[`, lalu baris sambungan diawali `- "` (atau `- '`). Pindaian menganggap `,` atau `[` di ujung baris sebagai tanda struktur, jadi baris berikutnya tidak dihitung sambungan, `- "` dianggap pembuka string multi baris yang sah, dan semua sampai tanda kutip berikutnya (termasuk `cache: &c` dan `other: *c`) tidak dipindai. Ini bentuk ketiga dari kelas yang sama, jadi pindaian teks tidak bisa dijamin lengkap; saran: deteksi lewat parser event YAML, atau longgarkan teks AC-1 dan K2.
- **Rendah, pesan.** Kutip multi baris yang mulai di barisnya sendiri (`rule:` lalu `"Host(...) &&` di baris berikut) kini ditolak dengan `yaml_alias` "YAML anchors, aliases, tags, and merge keys are not allowed", padahal file itu tidak memakai satu pun; pengguna tidak diberi tahu bahwa kutipnya yang harus ditulis di baris `key: `.

Kesiapan Milestone 1: semua temuan lain tertutup; yang tersisa hanya AC-1 di atas (butuh keputusan) dan langkah suite penuh yang tidak diulang checker.

### Ulang ketiga `/check verify` Milestone 1, 2026-11-13 (sesi `chk-f44-m1`, kode `98ae4914`, AC-1 dan K2 baru)

Dijalankan: `cargo test -p stackly-core --lib environment` (2), `cargo test -p stackly-core --test environment` (44), `cargo test -p stacklyd --test environment` (12), `cargo xtask provider check` (delapan paket `ok`), `cargo test -p stackly-core --test provider_packages` (92), lalu probe terhadap `stacklyd` asli.

**AC-1 terhadap teks baru: PASS.**

- Tujuh pasangan di `fixtures/environments/aliases/` dan dua file checker (`by1`, `hidden`, dengan kembaran yang ditulis langsung) memberi `valid`, `name`, `services`, dan `findings` yang sama persis lewat `env.validate` daemon asli; tidak ada `yaml_alias`. `manifestSha256` sama dengan SHA-256 byte file dan berbeda dari kembarannya.
- Alias tidak membawa nilai melewati aturan: secret literal lewat alias `secret_literal` di kedua service, secret di `settings` `secret_in_settings`, endpoint fixed yang sama lewat alias `endpoint_conflict` di keduanya, `dependsOn` lewat alias tetap membentuk `dependency_cycle: b -> c -> b`. Nilai sentinel tidak ada di log daemon.
- Ditolak sesuai tabel: `!nama` pada nilai `yaml_alias` di `services.db.provider`; pada key di `services.db`; `!` tanpa nama sama; `<<` dan `"<<"` di `services.web.<<`; `*tidak-ada` `yaml_syntax` di `6:8`; dua dokumen `yaml_syntax` di `.`. `!!str` diterima; anchor yang tidak dipakai bukan finding; kutip multi baris di barisnya sendiri valid.
- Bom di bawah 256 KiB ditolak cepat: bersarang klasik 3 ms, scalar 20 KiB di balik 5.000 alias 8 ms, scalar 120 KiB di balik sekitar 39.000 alias 32 ms, map lebar di balik 20.000 alias 46 ms, bom di dalam `settings` 34 ms (semuanya `yaml_alias` di `.`; key 60 KiB ditolak parser sebagai `yaml_syntax`).
- Memori `stacklyd`: 40,9 MiB sebelum; naik bertahap sampai 281 MiB sesudah sekitar 250 validasi bom, lalu tetap 281,2 MiB sepanjang 400 validasi berikutnya. `vmmap` menunjukkan 235 MiB itu "Malloc Large (empty)", yaitu blok besar yang sudah dibebaskan dan disimpan allocator macOS, bukan kebocoran dan bukan hasil expand (heap terpakai 11,7 MiB). Catatan rendah: setiap validasi file besar memakai beberapa MiB sementara, dan allocator menahannya.

Kesiapan Milestone 1: semua AC di lingkup lulus dan semua temuan tertutup. Satu satunya kotak kosong adalah suite penuh `stacklyd` dan `stackly-core`, yang tidak diulang checker (batas disk di brief) dan dijalankan lead di `main` (1763 dan 737 lulus).

### Acceptance-criteria coverage

- AC-1, AC-2, AC-3 · core `environment::manifest` dan `validate` dengan fixture per kode; lewat socket: test `validate_reads_the_file...` dan sentinel `a_secret_sentinel_in_the_manifest...`
- AC-5, AC-7, AC-8, AC-9, AC-10 · core `environment::plan`; lewat socket: `a_plan_reads_this_mac...` (ERP dan graph) dan blocker `env.name_taken` di `a_name_at_another_location...`; blocker member lewat socket ditunggu Up spec 0046, karena hanya Up yang menulis member
- AC-11, AC-12, AC-13 · bagian data: kolom, index unik, `WriteOp::DeleteEnvironment`, `GuardView::environments`; guard `environment::register` dan `unregister` (test core `register`); operasi `import_environment` dan `remove_environment` dengan replay dan audit (test daemon import, relink, limit, membership)
- AC-14 · core `environment::status`; `env.list` dan `env.get` lewat socket (`get_reads_the_manifest_only_when_asked`, membership)
- AC-15 · bagian protokol: `InstanceOwnership` `dedicated` dan `shared`, `InstanceSummary.environments`; cache `MembershipBook` di `Supervision`, dimuat sebelum rekonsiliasi dan sesudah setiap import dan remove, dipakai `instance.get`, `instance.list`, event `instance.*`, dan doctor; kolom `ENVIRONMENT` CLI milik Milestone 2
- AC-24 · modul `env`, enam method, area `env`, fixture `protocols/fixtures/env/`, protokol 4.2, schema editor; skenario fixture `environment-partially-ready` spec 0068 milik langkah 7
- AC-4, AC-6 (bagian socket) · `validate_reads_the_file...`, `a_plan_reads_this_mac...`, anggaran frame 32 service. Fake `Downloader`, `ProcessSupervision`, dan `SecretStore::write` tidak dihitung lewat socket (daemon test memakai platform nyata); nol panggilan dibuktikan test core plan, dan lewat socket dibuktikan tidak ada instance, operasi, audit, atau file baru
- AC-16 sampai AC-23, AC-25 (CLI dan GUI) · milik Milestone 2 dan 3

## Milestone 2: stackly env

Langkah 6 Build plan. Golden dan sebagian besar test memakai scripted daemon yang menjawab dengan fixture `protocols/fixtures/env/`; satu binary test menjalankan kelima perintah terhadap `stacklyd` sungguhan dengan catalog dari `providers/`. Binary test itu memakai `official/valkey` dan `official/nats`, bukan provider `fake`: binary daemon menolak namespace `testing` di catalog, dan `provider import` menolak artefak fixture paket `fake`. Tidak ada yang diunduh, dipasang, dibuat, atau dinyalakan.

### UI / manual

Tidak ada langkah manual. Semua perintah di bawah jalan tanpa terminal interaktif, dengan root test sementara.

### Commands

- [x] `cargo test -p stackly-cli --test env validate` → manifest valid mencetak `ok erp-development 2 services sha256:<digest>` dan exit 0; manifest tidak valid menulis satu baris `<file>: <location>: <code>: <message>` per finding lalu `2 error(s), 0 warning(s)` ke stderr, stdout kosong, exit 2 dengan `cli.env_invalid` (`errors` 2, `warnings` 0), dan `result` `--json` tetap `EnvValidation`; `--file` default `./stackly.yaml`, path relatif dikirim absolut dan dinormalkan (`./a/../b/./stackly.yaml` menjadi `<cwd>/b/stackly.yaml`) → AC-16
- [x] `cargo test -p stackly-cli --test env plan` → `plan-applicable.json` mencetak tabel `ACTION SERVICE INSTANCE PROVIDER VERSION ENDPOINTS` dengan baris `install`, `create`, `start` untuk `postgres` dan `keep` untuk `cache`, lalu `Plan: 1 to install, 1 to create, 1 to start, 1 to keep.`; `secrets:`, `differs:`, dan `not in manifest:` ke stderr; exit 0. `plan-blocked.json` exit 3 dengan `cli.env_plan_blocked` (`blockers` 2) dan satu baris `blocked:` per blocker beserta hint nya; manifest tidak valid exit 2 dengan `cli.env_invalid`; `result` `--json` selalu `EnvPlan`, stderr kosong dengan `--json`. Nama atau UUID utuh (huruf besar juga) dikirim sebagai sumber `registered`; nama yang tidak dikenal exit 10 dengan `cli.environment_not_found` sebelum `env.plan` dikirim; `--file` bersama nama ditolak sebagai usage (exit 2) → AC-16, AC-17
- [x] `cargo test -p stackly-cli --test env manage` → import mengirim `env.plan` lalu `env.import` dengan `expectedManifestSha256` dari plan dan idempotency key baru setiap kali, mencetak `Imported environment erp-development.` dengan baris Name, ID, File, Status, Services, dan `Already imported.` saat revision tidak berubah; `--relink` mengirim `relinkEnvironmentId` hanya saat `registration` `name_taken`; manifest tidak valid tidak pernah dikirim ke `env.import` (exit 2). `env list` mencetak `NAME STATUS READY FILE`, `No environments.` saat kosong, dan `{ items }` dengan `--json`. `env remove` tanpa `--yes` dan tanpa terminal gagal `cli.confirmation_required` (`flag` `--yes`) sebelum `env.remove`; dengan `--yes` mengirim id dan revision dari `env.list` dan mencetak `Removed environment erp-development. Instances and data are kept.`; nama yang tidak dikenal exit 10 → AC-17
- [x] `cargo test -p stackly-cli --test instance list` → kolom `ENVIRONMENT` sesudah `OWNERSHIP`: nama environment dipisah koma dalam urutan daemon (`erp-development, billing`), `-` untuk instance tanpa environment; golden `instance_list.txt`, `instance_list_failed.txt`, dan `instance_list.json` (field `environments`) diperbarui → AC-15 (bagian CLI)
- [x] `cargo build -p stacklyd -p xtask && cargo test -p stackly-cli --test env stacklyd` → terhadap `stacklyd` dengan catalog hasil `cargo xtask catalog build`: validate mencetak `ok shop 2 services sha256:<digest>`; plan mencetak baris `install`, `create`, `start` untuk `cache` lalu `queue` dan `Plan: 2 to install, 2 to create, 2 to start, 0 to keep.`, exit 0, `registration` `new`, `order` `cache, queue`, dan tidak ada file baru di folder manifest; import mendaftarkan `shop`, import kedua mencetak `Already imported.` dan tetap menjalankan satu operasi `import_environment` lagi (dua baris audit); list `shop down 0/2`; plan lewat nama (`registration` `registered`) dan lewat UUID exit 0; remove tanpa `--yes` gagal `cli.confirmation_required`, dengan `--yes` menghapus dan menambah satu baris audit `remove_environment`, manifest tetap ada; remove kedua exit 10 `cli.environment_not_found`; provider yang tidak dikenal exit 2 `cli.env_invalid` dengan finding `unknown_provider`; file yang tidak ada exit 2 `env.manifest_unreadable` → AC-16, AC-17
- [x] `cargo test -p stackly-cli --bin stackly` → unit test: `path::absolute` dan `home_masked`, `EnvironmentTarget` (UUID utuh dalam huruf apa pun adalah id), hitungan ringkasan plan menghitung setiap `start_instance` termasuk milik item `create`, baris finding `warning: `, dan satu constructor per kode `cli.*` (tiga kode `cli.env*` ikut) → AC-16, AC-17
- [x] `cargo test -p stackly-cli --no-fail-fast`, `cargo clippy -p stackly-cli --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `cargo xtask check-deps` → lulus _(Dicentang 2026-11-13 atas dasar gerbang lead di `main`, tidak diulang checker: `stackly-core` 1763, `stacklyd` 737, `stackly-cli` 638, suite kecil 1135, StacklyUI dan `build-app` pada `be9ff7a3`.)_

### Catatan `/check verify` 2026-11-13 (sesi `chk-f44-m2`, kode `c0005bad`)

Verdict Milestone 2: **PASS** untuk AC-16, AC-17, dan AC-15 bagian CLI. Selain test di atas, kelima perintah dijalankan terhadap `stacklyd` asli di root sementara dengan catalog `cargo xtask catalog build`.

Satu langkah tidak dicentang: langkah terakhir. `cargo clippy -p stackly-cli --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, dan `cargo xtask check-deps` lulus di checker; suite penuh `cargo test -p stackly-cli --no-fail-fast` tidak diulang (brief: test bertarget saja), lead menjalankannya di `main` pada `c0005bad` (638 lulus).

Yang teramati pada daemon asli:

- `result` `--json` `env validate` dan `env plan` sama persis dengan balasan `env.validate` dan `env.plan` daemon untuk file yang sama, pada lima kasus (valid, 11 finding, applicable, tiga blocker `supervisor.endpoint_in_use` karena port ERP dipakai proses lain di Mac ini, plan manifest tidak valid). Stdout selalu satu baris, stderr kosong.
- Exit code: 0 valid dan applicable; 2 `cli.env_invalid` dan `env.manifest_unreadable` (`invalid_input`); 3 `cli.env_plan_blocked` dan `env.name_taken` (`conflict`); 10 `cli.environment_not_found` untuk nama dan untuk UUID yang tidak terdaftar; 2 untuk `--file` bersama nama dan untuk `cli.confirmation_required`; 6 saat daemon mati.
- Validate dan plan lewat CLI tidak mengubah apa pun: pohon file root, folder manifest, dan runtime root (path, ukuran, mtime) sama, jumlah operasi dan audit sama.
- Import manifest tidak valid berhenti di CLI (jumlah operasi tetap 0). Import, import lagi (`Already imported.`, operasi kedua tetap tercatat), `--relink` dari lokasi lain, plan lewat nama, UUID, dan UUID huruf besar (`registration` `registered`), remove tanpa `--yes`, dengan `--yes`, lalu remove kedua exit 10; file manifest tetap ada.
- `instance list` menampilkan `ENVIRONMENT` `-` dan `environments` `[]` untuk instance manual.

Temuan, semuanya rendah dan tidak membuat AC gagal:

1. Mode teks tidak menampilkan lokasi tempat nama terdaftar pada `env.name_taken` (baik error import maupun baris `blocked:` plan), dan hint nya tidak menyebut `--relink`; `details` hanya terlihat dengan `--json`.
2. `env remove --yes` menulis `Instances and data are kept.` ke stderr selain kalimat hasil di stdout.
3. Di terminal, bagian stderr (`blocked:`, `secrets:`, `differs:`, `not in manifest:`) muncul sebelum tabel, sedangkan tabel CLI spec menulis tabel dulu.
4. `counts` menghitung warning hanya dari finding yang dikirim (paling banyak 100); saat spec 0045 menambah warning dan ada `omitted`, angka `<e> error(s), <w> warning(s)` bisa meleset.

### Acceptance-criteria coverage

- AC-16 · `apps/cli/src/commands/env/validate.rs` dan `plan.rs`, golden `env_validate*`, `env_plan*`; kesetaraan finding dan blocker dengan GUI terjamin karena CLI menulis apa yang `env.validate` dan `env.plan` kirim tanpa menghitung ulang
- AC-17 · `import.rs`, `list.rs`, `remove.rs`, `target.rs`; konfirmasi memakai aturan `ConfirmArgs` spec 0008 lewat `confirm_action`
- AC-15 · kolom `ENVIRONMENT` di `apps/cli/src/commands/instance/list.rs` (golden scripted); isi `environments` diuji daemon di `cargo test -p stacklyd --test environment membership`. Binary test CLI dengan instance anggota menunggu spec 0046: lewat binary, keanggotaan hanya terbentuk oleh Up
- AC-16, AC-17 terhadap daemon sungguhan · `apps/cli/tests/env/stacklyd.rs` dengan provider official; jalur provider `fake` tidak bisa dicapai binary daemon (lihat paragraf di atas)

## Milestone 3: Environments di app

_Bagian ini mencakup Build plan langkah 7 sampai 11 (store dan navigasi, halaman daftar dan detail, sheet plan dan import, permukaan lain, evidence dan langkah manual). Layar, store, dan snapshot dibangun di atas fixture dan tipe hasil codegen; `EnvironmentEndToEndTests` menjalankan model yang sama terhadap `stacklyd` nyata sesudah langkah 5 masuk `main`._

### UI / manual

Langkah ini hanya kamu yang menjalankan (sesi GUI nyata, pointer saja; `cargo xtask ui-test` tidak dipakai). Prasyarat: langkah 5 sudah di `main`; `cargo build -p stacklyd -p stackly-fake-server`; root sementara `export STACKLY_ROOT=$(mktemp -d)`; jalankan `STACKLY_LOG=debug cargo run -q -p stacklyd` di satu Terminal; jalankan app hasil `cargo xtask build-app` dengan `STACKLY_ROOT` yang sama (`open --env STACKLY_ROOT="$STACKLY_ROOT" target/app/Debug/Stackly.app`); salin `crates/core/tests/fixtures/environments/erp-development.yaml` ke `$STACKLY_ROOT/erp/stackly.yaml`. Jangan pakai Application Support asli.

_Status semua langkah di bawah: `NOT_RUN`, owner: user (langkah manual `F44-1`). Checker tidak menjalankan app GUI._

- [ ] Sidebar → Environments → halaman "No Environments" / "Import a stackly.yaml to review its plan." dengan tombol Import…; subjudul toolbar "0 projects" → AC-18, AC-22
- [ ] Klik Import… → panel file berpesan "Choose a stackly.yaml. You’ll review trust and a plan before anything runs." dan hanya menerima `.yaml`/`.yml`; pilih `$STACKLY_ROOT/erp/stackly.yaml` → sheet "Plan for erp-development" dengan subjudul "Review what Up will do. Nothing runs until you confirm.", enam baris "+ Create" (postgres, mysql, nats, clickhouse, valkey, traefik) masing masing dengan baris "Installs {software} {version}" dan "Needs secret: local:erp/…" (lima reference belum terdaftar), catatan kaki "Services start in dependency order and wait for readiness.", tombol Cancel dan Import → AC-20, AC-21, AC-25
- [ ] Sebelum menekan Import, cek di Terminal: `find "$STACKLY_ROOT" -newer "$STACKLY_ROOT/erp/stackly.yaml" -type f | grep -v -e '\.log' -e registry` kosong, `pgrep -fl postgres` kosong, dan `ls "$STACKLY_ROOT"/data/runtimes 2>/dev/null` kosong → tidak ada proses, runtime, atau file data → AC-6, AC-25
- [ ] Klik Import → sheet tertutup, feedback "Imported “erp-development”.", sidebar memilih baris anak "erp-development" dengan simbol status dan "0/6"; halaman detail berjudul erp-development, subjudul `~/…/erp/stackly.yaml` (home ditulis `~`), tabel Services dengan enam baris Instance "Will be created" dan Status "Not created", seksi Details berisi Manifest dan Schema `stackly/v1alpha1` → AC-18, AC-21
- [ ] Buka menu baris service (ikon ellipsis di kolom terakhir) pada baris yang belum dibuat → satu satunya item Show Plan…; pilih → sheet plan yang sama dengan hanya tombol Done → AC-19, AC-20
- [ ] Toolbar Validate → feedback "erp-development: schema stackly/v1alpha1 valid · no cycles · secrets referenced." → AC-21
- [ ] More → Open Manifest → file terbuka di app bawaan macOS untuk `.yaml`; ubah `maxConnections: 200` menjadi `201`, simpan, lalu kembali ke Stackly (app aktif lagi) → banner "The manifest changed on disk." dengan Review Plan dan Reload → AC-21
- [ ] Klik Review Plan → sheet plan dengan Done saja; tutup. Klik Reload → banner hilang, feedback "Reloaded “erp-development”." → AC-20, AC-21
- [ ] Di editor tambahkan `password: hunter2` di `settings` service valkey, simpan, kembali ke Stackly → banner "The manifest has 1 problem." (atau jumlah yang daemon hitung); klik Show Problems → seksi Problems dengan satu baris per finding (judul pesan, detail lokasi), tanpa nilai `hunter2` di layar → AC-21, AC-22
- [ ] Hapus baris itu lagi, simpan. More → Reveal in Finder → Finder memilih `stackly.yaml`. More → Copy Path → path absolut di clipboard → AC-21
- [ ] Ganti nama file menjadi `stackly.moved.yaml` di Finder, kembali ke Stackly → banner "The manifest file can’t be read." dengan Locate…; pilih `stackly.moved.yaml` → banner hilang dan subjudul berubah ke file baru → AC-21
- [ ] Overview → seksi "Environments" di kolom kanan di atas Operations dengan baris erp-development, status, dan "0 of 6 ready · ~/…"; klik baris → halaman detail → AC-23
- [ ] Instances → kolom "Environment" (nama environment, "+{n}" kalau lebih, "—" kalau kosong) bisa diurutkan dengan klik header; jendela dipersempit → kolom Environment hilang lebih dulu, lalu Version dan Endpoints; inspector instance mana pun → baris Environment ("—" sebelum Up) dan Ownership "Manual" → AC-23
- [ ] Salin file itu: `mkdir "$STACKLY_ROOT/erp2" && cp "$STACKLY_ROOT/erp/stackly.moved.yaml" "$STACKLY_ROOT/erp2/stackly.yaml"`. Environments → Import… → pilih `erp2/stackly.yaml` → sheet menampilkan banner "“erp-development” is already registered at ~/…/erp/stackly.moved.yaml." dengan Use This File, dan Import nonaktif dengan alasan "This name is registered at another file."; klik Use This File → sheet tertutup dan subjudul halaman detail menjadi `~/…/erp2/stackly.yaml` (environment yang sama, bukan yang baru) → AC-21
- [ ] Halaman detail → More → Remove Environment… → konfirmasi "Remove “erp-development”?" / "Stackly forgets this environment. Its instances, data, and stackly.yaml are kept." / Remove; konfirmasi → feedback "Removed “erp-development”.", sidebar kembali ke daftar Environments, file `erp2/stackly.yaml` masih ada → AC-21
- [ ] Hentikan `stacklyd` (Ctrl-C) dengan halaman Environments terbuka → halaman tetap menampilkan data terakhir, tombol Import…, Reload, dan Remove nonaktif dengan alasan; jalankan lagi → data dimuat ulang → AC-22
- [ ] Ulangi pemeriksaan sidebar, halaman detail, dan sheet dalam mode gelap (System Settings → Appearance → Dark) → semua status tetap terbaca lewat teks dan simbol → AC-22
- [ ] Sesudah selesai: `rm -rf "$STACKLY_ROOT"`
- [ ] Isi record `evidence/manual/2026-11-13-spec-0044-journey-04-import-the-erp-manifest-in.json` (`steps`, `actual`, `status`, `reviewer`, `finishedAt`) dengan hasil langkah di atas, lalu `cargo xtask evidence check` → AC-25
- [ ] (19) Menu baris service yang instance nya sudah ada, lewat `instanceRef`. Prasyarat: app dan `stacklyd` seperti prasyarat di atas, dengan `STACKLY_ROOT` baru (`export STACKLY_ROOT=$(mktemp -d)`) dan jaringan menyala (langkah ini mengunduh runtime Valkey). Di app: Catalog → Valkey → Get, tunggu Installed; lalu New Instance → Valkey, nama `shared-valkey`, Create, dan biarkan instance berjalan. Tulis manifest dengan Terminal: `mkdir -p "$STACKLY_ROOT/borrow" && printf 'apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: borrow-demo\nservices:\n  cache:\n    provider: official/valkey\n    instanceRef: shared-valkey\n' > "$STACKLY_ROOT/borrow/stackly.yaml"`. Environments → Import… → pilih `borrow/stackly.yaml` → sheet berisi satu baris "= Keep" (atau "▸ Start" bila instance berhenti) `cache → shared-valkey`; Import. Di halaman detail baris `cache` menampilkan Selector `instanceRef: shared-valkey`, Instance `shared-valkey`, Version versi Valkey, Ownership "Shared ref", dan status instance. Buka menu baris (ikon ellipsis) → berurutan: item Open In milik Valkey (misalnya Terminal), lalu Copy Connection URL, lalu Show Instance dan Show Logs, lalu Stop (atau Start bila berhenti); Copy Connection URL menyalin connection string pertama kartu Connection (tempel di TextEdit untuk memeriksa); Show Instance membuka detail `shared-valkey`; kembali, Show Logs membuka detail di tab Logs; kembali, Stop menghentikan instance dan status baris berubah menjadi Stopped dalam beberapa detik (lalu Start menyalakannya lagi); klik ganda baris `cache` membuka detail `shared-valkey`. Klik kanan baris memberi menu yang sama. Sesudahnya: More → Remove Environment… → Remove; `shared-valkey` tetap ada di Instances; hapus instance dari app bila tidak dipakai, lalu `rm -rf "$STACKLY_ROOT"` → AC-19

### Commands

- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter EnvironmentStoreTests` → `env.list` per nama, subjudul "3 projects"; operasi environment yang selesai memuat ulang, operasi yang masih jalan atau resource lain tidak; event instance memuat ulang sekali sesudah jeda 300 ms; `env.get` membaca file hanya dengan `checkManifest` true → AC-18 (data), *Event*
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "SidebarSelectionTests|ShellNavigatorTests|EnvironmentAppearanceTests"` → nilai scene storage seksi tidak berubah, environment disimpan `environment:<uuid>`, nilai rusak jatuh ke default; navigator membuka dan melupakan environment; tabel `StatusAppearance.environment` sesuai `apps/macos/design.md` → AC-18, AC-22
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter "EnvironmentDetailModelTests|EnvironmentPlanPresenterTests|EnvironmentCopyTests"` → Validate (feedback atau Problems), Reload dan Locate… mengirim `env.import` dengan digest yang dibaca validate (Locate… dengan `relinkEnvironmentId`), Remove mengirim revision dan menjelaskan `env.lease_active`; Import… membaca `env.plan` dan tidak mengirim apa pun sampai Import, manifest tidak valid dan `name_taken` tidak bisa diimpor, Use This File mengirim relink, blocker lain tidak menghalangi import, Review hanya membaca; teks baris Create, Start, Keep, Installs, secret, dan perbedaan; urutan `order`; kolom Services dan sempitnya → AC-19, AC-20, AC-21, AC-22, AC-25 (bagian fixture)
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter EnvironmentsSnapshotTests` → snapshot terang, gelap, dan sempit: daftar terisi, kosong, dan offline; detail partially ready, ready, dan down; empat banner manifest; Problems; sheet plan import (enam Create), plan terdaftar (Keep, Start, Create, blocker, perbedaan, approval), tidak valid, dan nama terpakai; Overview dengan seksi Environments; Instances dengan kolom Environment → AC-18 sampai AC-23
- [x] `cargo build -p stacklyd -p stackly-fake-server && cargo build -p stacklyd --example skeleton_seed && cargo xtask build-app`, lalu `swift test --package-path apps/macos/Packages/StacklyUI --filter EnvironmentEndToEndTests` → terhadap `stacklyd` nyata dengan katalog bundel app (`STACKLY_TEST_CATALOG_DIR` atau `target/app/Debug/Stackly.app/Contents/Resources/catalog`): plan manifest contoh ERP valid, `registration` `new`, enam item `create` dengan `install_runtime`, `create_instance`, dan `start_instance`, semua provider `trusted`; pohon file test root (tanpa log dan registry) dan daftar instance sama sebelum dan sesudah plan dan import; import mendaftarkan `erp-development` dengan status `down` dan enam service; `manifestState` `in_sync`, Validate sukses, file diubah menjadi `changed`, Reload kembali `in_sync`; Remove menghapus environment dan file tetap ada; tanpa binary atau katalog test berakhir BLOCKED → AC-20, AC-21, AC-25, AC-6 (bagian GUI)
- [x] `swift test --package-path apps/macos/Packages/StacklyUI --filter FixtureScenarioTests` → skenario `environment-partially-ready` ada dan tersambung → AC-24
- [x] `swift test --package-path apps/macos/Packages/StacklyUI` dan `swift test --package-path clients/swift` → lulus (test integrasi butuh `cargo build -p stacklyd -p stackly-fake-server` dan example `skeleton_seed`, selain itu BLOCKED) → AC-22 _(Dicentang 2026-11-13 atas dasar gerbang lead di `main`, tidak diulang checker: `stackly-core` 1763, `stacklyd` 737, `stackly-cli` 638, suite kecil 1135, StacklyUI dan `build-app` pada `be9ff7a3`.)_
- [x] `cargo xtask build-app` → build scheme Stackly lulus → AC-18
- [x] `cargo xtask evidence check` → record manual `evidence/manual/2026-11-13-spec-0044-journey-04-import-the-erp-manifest-in.json` valid dengan status `NOT_RUN`, owner user → AC-25

### Catatan `/check verify` 2026-11-13 (sesi `chk-f44-m3`, kode `dfd1f3c6`)

Verdict Milestone 3: bagian yang bisa diotomasi **PASS** dengan satu temuan sedang; langkah `### UI / manual` tetap `NOT_RUN` (owner user), jadi AC-18 sampai AC-23 dan AC-25 baru terbukti lewat test, snapshot, dan satu test terhadap `stacklyd` nyata, belum lewat GUI nyata.

Satu langkah Commands tidak dicentang: suite penuh. `swift test --package-path clients/swift` lulus di checker (285 test); suite penuh StacklyUI tidak diulang (brief: test bertarget), lead menjalankannya di `main` pada `dfd1f3c6`.

Yang dijalankan: `EnvironmentStoreTests` 8, `SidebarSelectionTests|ShellNavigatorTests|EnvironmentAppearanceTests` 19, `EnvironmentDetailModelTests|EnvironmentPlanPresenterTests|EnvironmentCopyTests` 21, `EnvironmentsSnapshotTests` 8, `FixtureScenarioTests` 14, `cargo xtask build-app` (BUILD SUCCEEDED), `EnvironmentEndToEndTests` 1 (benar benar jalan terhadap `stacklyd` dengan katalog bundel app, bukan BLOCKED), `cargo xtask evidence check` (73 record manual valid). PNG snapshot terang, gelap, dan sempit dibaca satu per satu dan dibandingkan dengan *Layar* dan mockup.

Temuan:

1. **Sedang, AC-19 dan *Layar*.** Pada lebar sempit kolom aksi (menu ellipsis) tabel Services terpotong: `environment-partially-ready.narrow.png` hanya menampilkan Service, Instance, Ownership, Status. Lebar ideal kelima kolom melebihi lebar tabel, jadi kolom terakhir jatuh ke luar. Menu yang sama masih bisa dibuka lewat klik kanan dan klik ganda tetap membuka instance, tetapi *Layar* hanya menyembunyikan Version lalu Selector.
2. **Rendah, AC-19.** Item menu berjudul "Copy Connection String" (aksi `ConnectionCopier` yang sudah ada), sedangkan AC-19 dan mockup menulis "Copy Connection URL". Fungsinya sesuai *Layar*; teks spec atau judulnya perlu diselaraskan.
3. **Rendah.** Detail instance untuk Open In dimuat saat baris tampil (satu `instance.get_detail` per service yang punya instance, paling banyak 32 saat halaman dibuka), bukan saat menu dibuka seperti tulisan *Layar*.
4. **Rendah.** Banner blocker di sheet plan menggabungkan blocker per service menjadi satu item ("2 problems block this plan." dengan satu baris `search`), bukan satu item per blocker.
5. **Celah bukti otomatis** (bisa ditutup snapshot atau test, tidak perlu GUI nyata): baris anak sidebar dengan `{ready}/{total}` (tidak ada snapshot shell dengan environment; skenario `environment-partially-ready` sudah ada); isi menu baris service `menu(for:)` untuk instance yang ada dan untuk yang belum dibuat; menu More (`EnvironmentFileActions.sections` dengan port palsu).
6. **Celah langkah manual.** Tidak ada langkah untuk menu baris service yang instance nya sudah ada (Open In, Copy Connection String, Show Instance, Show Logs, Start, Stop, atau Retry) dan untuk klik ganda, karena sebelum Up spec 0046 tidak ada service ber instance. Bisa ditutup sekarang dengan test model (butir 5), atau dengan langkah manual memakai `instanceRef` ke instance yang sudah ada; selain itu menunggu spec 0046.
7. Record manual ber `owner` kosong, sama seperti 16 record `NOT_RUN` lain; kalimat langkah "owner user" hanya soal teks.

Yang benar hanya bisa dibuktikan manual: panel `NSOpenPanel`, Open Manifest dan Reveal in Finder lewat `NSWorkspace`, clipboard, banner yang muncul saat app aktif lagi, toolbar jendela, dan mode gelap sistem. Tempat Up, Down, Lock, dan Lease hanya berupa komentar di kode, tanpa kontrol. Tidak ada tambahan aksesibilitas, shortcut, atau identifier UI test di folder Environments. Sheet plan dan Problems hanya menampilkan reference `local:` dan pesan template, tidak ada nilai secret.

### Ulang `/check verify` Milestone 3 sesudah fix app, 2026-11-13 (sesi `chk-f44-m3`, kode `be9ff7a3`)

Dijalankan: `swift test --package-path apps/macos/Packages/StacklyUI --filter "EnvironmentsSnapshotTests|ShellSnapshotTests|ServiceMenu|EnvironmentFileActions|EnvironmentPlanPresenterTests|EnvironmentDetailModelTests|EnvironmentCopyTests"` (35 test dalam 7 suite lulus, `EnvironmentMenuTests` ikut), dan PNG yang berubah dibaca.

- **Temuan 1: PASS.** `environment-partially-ready.narrow.png` kini menampilkan kolom aksi (menu ellipsis) di setiap baris.
- **Temuan 2: PASS.** Judul item "Copy Connection URL" (`EnvironmentCopy.copyConnectionURL`), aksi yang sama dengan `ConnectionCopier`.
- **Temuan 4: PASS.** `environment-plan-blocked` menampilkan dua item: `search` dan `search (provider.approval_required)`; test `eachBlockerIsItsOwnBannerItem`.
- **Temuan 5: PASS.** `aServiceMenuFollowsItsInstance`, `theMoreMenuOpensRevealsAndReloadsTheManifest`, `withoutAFileTheFileActionsSayWhy`, dan snapshot `shell-environments-sidebar` (baris anak dengan simbol status dan `0/1`, `1/4`, `1/1`).
- **Temuan 6:** langkah manual 19 sudah ditulis; tetap `NOT_RUN`.
- **Catatan snapshot sidebar.** Baris Overview yang terpilih tampil sebagai sorotan hitam tanpa teks. Tidak ada snapshot shell lama yang menggambar sidebar (`ShellSnapshotTests` hanya matriks banner dan halaman kosong), jadi tidak ada pembanding; kode `SidebarRow` tidak berubah di fix ini, jadi ini bukan regresi kode, tetapi referensinya tidak mewakili tampilan asli baris terpilih. Tampilan asli dibuktikan langkah manual. Di snapshot itu path seksi Environments Overview juga tampil sebagai `/Users/…` (home fixture berbeda dari home mesin), bukan `~`.

Kesiapan Milestone 3: bagian otomatis siap dicentang; 19 langkah manual tetap `NOT_RUN` (owner user).

### Acceptance-criteria coverage

- AC-18 · `EnvironmentsPage`, baris anak sidebar, `StatusAppearance.environment`; test store, selection, dan snapshot daftar dan detail; GUI nyata di langkah manual
- AC-19 · menu baris service (`OpenInMenu` lewat `OpenInModel.content`, Copy Connection String, Show Instance, Show Logs, Start/Stop/Retry, Show Plan…); snapshot dan langkah manual
- AC-20 · `EnvironmentPlanSheet` dengan Create, Start, Keep, Installs, blocker, secret, perbedaan, approval; Done saja dari detail
- AC-21 · Import… dengan relink, Validate, Open Manifest, Reveal in Finder, Copy Path, Reload, Locate…, Remove; test model dan langkah manual
- AC-22 · state loading, kosong, gagal, offline, dan unsupported dari `ViewState`; aksi mutasi lewat `.mutationGated()` dan alasan nonaktif; status selalu teks dan simbol
- AC-23 · seksi Environments Overview, kolom Environment Instances, baris Environment dan Ownership inspector, baris Welcome "Environments as files"
- AC-24 (bagian Swift) · skenario fixture `environment-partially-ready`, `PageRig.environmentsPage`
- AC-25 · bagian fixture lewat `EnvironmentPlanPresenterTests` dan snapshot `environment-plan-import`; bukti lewat daemon nyata menunggu langkah 5 dan langkah manual di atas (record manual `NOT_RUN`)
- AC-1 sampai AC-17 · milik Milestone 1 dan 2

### Ulang sesudah K27, 2026-11-14 (kode `31b25de2`)

K27 (`provider_single_instance`, AC-2 dan AC-7) sudah dibangun dan sesuai teks *Provider satu instance*: service `dedicated` kedua dan seterusnya dari provider yang paketnya tidak `multiInstance` mendapat satu finding `error` di `services.<alias>.provider`, urut manifest, dengan pesan tetap; service `instanceRef` tidak dihitung; satu service `dedicated` dengan instance provider itu yang sudah ada tetap blocker plan `instance.capability_unsupported`. `cargo test -p stackly-core --test environment` 109 lulus, termasuk `validate::the_second_and_third_dedicated_service_of_a_single_instance_provider_are_refused`, `validate::a_service_that_borrows_an_instance_of_a_single_instance_provider_is_not_counted`, dan `plan::one_dedicated_service_of_a_single_instance_provider_that_already_has_one_is_blocked`. Tidak di probe terhadap binary daemon: keenam provider resmi `multiInstance`, jadi bukti hanya dari test dengan paket `testing/single`. Temuan rendah nomor 5 di Verify penuh (`instance.capability_unsupported` belum teruji) dengan ini tertutup.

## Verify penuh 2026-11-13

_Sesi `chk-f44-verify`, kode `4c18ff05`. Menutup `Build it` dan `Verify it` fitur 44. Langkah yang sudah `[x]` tidak diulang; yang diperiksa adalah apakah masih berlaku sesudah merge terakhir (F46 milestone 1, F47 langkah 1 dan 2, loader YAML)._

Sejak pemeriksaan terakhir tiap area tidak ada file fitur ini yang berubah (`crates/core/src/environment`, `apps/daemon/src/ipc/env`, `apps/daemon/src/membership.rs` sejak `98ae4914`; `apps/cli` sejak `c0005bad`; `apps/macos` dan `clients/swift` sejak `be9ff7a3`). Test bertarget pada `HEAD`: `cargo test -p stackly-core --test environment` 44, `cargo test -p stacklyd --test environment` 12, `cargo test -p stackly-cli --test env` 13 (termasuk binary test terhadap `stacklyd` asli), `--test instance list` 17, dan `swift test --filter "EnvironmentEndToEndTests|EnvironmentStoreTests|EnvironmentMenuTests"` 12 (E2E terhadap `stacklyd` asli, bukan BLOCKED). Tiga kotak suite penuh dicentang atas dasar gerbang lead.

Verdict: semua AC lulus untuk bagian yang bisa diotomasi. Yang tersisa hanya bukti GUI nyata: langkah manual `F44-1` (18 langkah dan record evidence) dan `F44-2` (langkah 19), keduanya `NOT_RUN`, owner user.

| AC | Status | Bukti utama |
| --- | --- | --- |
| AC-1 | PASS | loader `yaml_value`, alias sama dengan kembarannya, tag dan `<<` ditolak, bom ditolak cepat (daemon asli) |
| AC-2 | PASS | satu fixture per kode, semua finding sekaligus, jalur cycle di pesan |
| AC-3 | PASS | sentinel tidak ada di balasan, log, registry, root |
| AC-4 | PASS | `env.validate` daemon asli, alasan `manifest_unreadable` |
| AC-5 | PASS | ERP enam install, create, start; `install` false lewat probe; digest stabil |
| AC-6 | PASS | pohon file, operasi, audit, instance, runtime, provider sama sebelum dan sesudah (socket, CLI, E2E app) |
| AC-7 | PASS | port, nama, trust `pending` dan `revoked` satu blocker, selector; `instance.capability_unsupported` belum teruji |
| AC-8 | PASS | `instanceRef` tidak ada, provider lain, `keep` dan `start`, tidak pernah `create` |
| AC-9 | PASS | tabel lifecycle termasuk `retained`, `not_initialized`, `differences`, `detached` |
| AC-10 | PASS | status secret dengan fake store; Keychain asli tidak dijalankan |
| AC-11 | PASS | nama unik, lokasi kanonik (`/tmp`, huruf besar folder dan nama file), relink |
| AC-12 | PASS | import, key sama, tak berubah, digest lama, tidak valid, ke 65 |
| AC-13 | PASS | revision, `env.lease_active`, remove, file dan instance tetap |
| AC-14 | PASS | list, get, `manifestState`, `updatedAt` dari baris |
| AC-15 | PASS | `ownership` dan `environments` sama di get, list, event; kolom `ENVIRONMENT` CLI |
| AC-16 | PASS | `result` `--json` sama dengan balasan daemon; exit 0, 2, 3 |
| AC-17 | PASS | import, relink, list, remove, exit 10; konfirmasi `[y/N]` di TTY tidak diuji |
| AC-18 | PASS otomatis + manual `NOT_RUN` | snapshot daftar, detail, sidebar; `F44-1` |
| AC-19 | PASS otomatis + manual `NOT_RUN` | `aServiceMenuFollowsItsInstance`, snapshot sempit; `F44-1` langkah 5, `F44-2` |
| AC-20 | PASS otomatis + manual `NOT_RUN` | snapshot sheet (Create, Start, Keep, blocker, Done saja); `F44-1` |
| AC-21 | PASS otomatis + manual `NOT_RUN` | test model, menu More, E2E daemon asli; `F44-1` |
| AC-22 | PASS otomatis + manual `NOT_RUN` | kosong, offline, gagal, terang, gelap, sempit; `F44-1` |
| AC-23 | PASS otomatis + manual `NOT_RUN` | snapshot Overview, Instances, inspector, onboarding; `F44-1` |
| AC-24 | PASS | protokol 4.2, fixture, schema, `codegen --check`, Swift contract test |
| AC-25 | PASS otomatis + manual `NOT_RUN` | binary test CLI dan E2E app terhadap daemon asli; record manual `NOT_RUN` |

**Trace** (`docs/scope/08-environment-proyek.md`): REQ-020 lewat AC-1, AC-2, AC-5, AC-6 (PASS) · REQ-028 lewat AC-16, AC-17 (PASS) · VAL-013 lewat AC-2, AC-3, AC-7 (PASS) · VAL-014 lewat AC-8 (PASS) · AC-020 lewat AC-5, AC-6 (PASS) · AC-080 bagian plan lewat AC-6 (PASS) · PAGE-10 lewat AC-18 sampai AC-22 (otomatis PASS, `F44-1` dan `F44-2` `NOT_RUN`) · NFR-012 lewat AC-22 (otomatis PASS, `F44-1` `NOT_RUN`) · JOURNEY-04 lewat AC-25 (otomatis PASS, record manual `NOT_RUN`).

**Done when:**

- Plan manifest baru menampilkan install, create, dan start tanpa file data, proses, perubahan trust, atau runtime: terbukti (daemon asli, CLI, E2E app).
- Cycle, reference yang tidak ada, secret literal, dan konflik endpoint fixed ditolak sebelum mutasi: terbukti (`dependency_cycle`, `unknown_dependency`, `env.instance_ref_not_found`, `secret_literal`, `endpoint_conflict`; import ditolak tanpa operasi).
- `stackly env validate` dan `plan` setara dengan GUI: terbukti di sisi CLI (sama persis dengan balasan daemon) dan di sisi app lewat method yang sama (E2E); perbandingan di layar nyata ada di `F44-1`.
- Halaman Environments bisa dipakai dengan interaksi desktop: otomatis lewat test model dan snapshot; GUI nyata `F44-1` `NOT_RUN`.
- Halaman dan sheet plan mengikuti mockup (status, tabel service, Create, Start, Keep, sidebar dengan hitungan ready): terbukti lewat snapshot terang, gelap, sempit; GUI nyata `F44-1` `NOT_RUN`.
- Menu aksi per service (`OpenInMenu`, Copy Connection URL, Show Instance, Show Logs, Start atau Stop): terbukti lewat `EnvironmentMenuTests`; GUI nyata `F44-2` `NOT_RUN`.

**Temuan rendah yang masih terbuka** (tidak menghalangi; untuk tester dan `/sync`):

1. CLI mode teks: `env.name_taken` tidak menampilkan lokasi tempat nama terdaftar dan tidak menyebut `--relink`.
2. CLI: `env remove --yes` menulis `Instances and data are kept.` ke stderr selain kalimat hasil; bagian stderr `env plan` muncul sebelum tabel.
3. CLI: `counts` menghitung warning hanya dari finding yang dikirim; bisa meleset saat ada `omitted` dan warning (spec 0045).
4. Daemon: RSS `stacklyd` naik ke sekitar 281 MiB sesudah ratusan `env.validate` file besar lalu datar (blok bebas yang ditahan allocator, bukan bocor).
5. Core: `installed_fixture` (cocok lewat software, versi, target untuk artefak `fixture`) ada di kode produksi `plan.rs`, tidak di balik feature `fixture`; tidak terjangkau catalog nyata. Baris fixture ber mode `External` memberi item tanpa runtime dan tanpa blocker.
6. AC-7: `instance.capability_unsupported` tidak punya test dan tidak bisa di probe (semua paket `multiInstance`); dua service ber provider tanpa `multiInstance` dalam satu manifest belum diatur spec.
7. App: detail instance untuk Open In dimuat saat baris tampil (paling banyak 32 baca saat halaman dibuka), bukan saat menu dibuka.
8. App: snapshot `shell-environments-sidebar` menggambar baris terpilih sebagai sorotan hitam tanpa teks dan path Overview sebagai `/Users/…`; referensi itu tidak mewakili tampilan asli baris terpilih.
9. AC-10: status `present` dan `missing` terhadap Keychain login asli tidak dijalankan (hanya user); status `unknown` dan `secret.store_unsupported` belum punya test.
10. Record manual ber `owner` kosong (kebiasaan record `NOT_RUN` di repo), sedangkan teks langkah menulis "owner user".

Daftar celah test lengkap sudah dikirim ke sesi `tst-f44-test`.
