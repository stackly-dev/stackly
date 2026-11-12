# Verify: Provider Traefik & routing lokal · spec 0041 · updated 2026-11-12

_Langkah diturunkan dari acceptance criteria spec 0041. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Selalu `export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`. Test native memakai artefak `traefik-3.7.15-macos-aarch64` dari cache runtime xtask (`~/Library/Caches/dev.stackly.xtask/runtime/402f59ce…`) dan tidak pernah mengunduh; tanpa artefak itu setiap test native berakhir `BLOCKED` (`reason` `artifact traefik-3.7.15-macos-aarch64 is not in the runtime cache`, `owner` `@ojiepermana`)._

## Milestone 1: Paket resmi

_Build 2026-11-12 (dev-f41-m1): langkah 1 sampai 3, commit `cb64ad34` (langkah 1 dan 2) dan `44958467` (langkah 3). `manifest_sha256` paket berubah satu kali, menjadi `c568f2e2…`; langkah 7a (Milestone 2b) mengubahnya untuk terakhir kalinya, menjadi `c57b6a82…`, dan nilai sha di bawah sudah mengikuti itu._

### UI / manual

- [x] Buka `providers/traefik/provider.yaml` → `metadata.revision` tetap `"1.0.0"`; sesudah `permissions` ada blok `connection` dengan `web` (`Web URL`, `http://${endpoint.web.address}`) dan `ping` (`Health URL`, `http://${endpoint.ping.address}/ping`); `openWith` punya `routes` (reveal) lalu `routers` (browser, `http://${endpoint.ping.address}/api/http/routers`); blok `remedies` punya satu entri `routes_not_loaded` dengan `readinessReasons: [routes_mismatch]` dan tiga langkah, langkah ketiga dengan `path: "${instance.config}/dynamic"`; `capabilities` tetap hanya `reload` dan `multiInstance`, tanpa `tls` → AC-1, AC-7, AC-13
- [x] Buka `providers/traefik/tests/scenarios.yaml` → dua skenario, `default` dan `one-route` (route `app`, ``Host(`app.localhost`)``, `http://127.0.0.1:9`) → AC-1
- [x] Buka `providers/traefik/README.md` → menjelaskan dua endpoint, cara mengubah route (Configuration atau `stackly instance config apply`), arti `routes_mismatch`, folder `dynamic` hanya untuk file tulisan Stackly, dan tiga batas (hanya HTTP, dashboard mati, "Stackly never edits `/etc/hosts`, DNS, or the system trust store") → AC-1
- [x] `git diff main -- providers/traefik/schemas providers/traefik/templates` → kosong (settings schema dan kedua template tidak berubah byte demi byte) → AC-1

### Commands

- [x] `cargo xtask provider check providers/traefik` → `ok official/traefik 1.0.0 c57b6a8274d55f6c0b0f65fd93abcca1ee661b66e8a4793183728e5bfdd01bd7` → AC-1
- [x] `cargo xtask provider check` → satu baris `ok` per folder di `providers/`, Traefik dengan sha di atas → AC-1
- [x] `cargo test -p stackly-core --test provider_packages` → 42 test lulus, termasuk `the_repo_packages_declare_their_connection_entries_in_manifest_order` (Traefik `web`, `ping`), golden `traefik/*.json` dan `*.txt` yang lama tidak berubah, `traefik_remedy_matches_routes_mismatch_only_and_renders_like_the_golden_file` (golden baru `remedies.json`), `traefik_renders_the_dashboard_off_and_never_a_tls_key`, dan `traefik_permissions_are_the_managed_folders_and_the_runtime_binary_only` → AC-1, AC-6, AC-7, AC-13
- [x] `cargo test -p stackly-core --test open_with_render` → 7 test lulus; `golden/provider/traefik/open_with.json` memuat `routes` lalu `routers` dengan URL `http://127.0.0.1:14000/api/http/routers` → AC-1
- [x] `cargo test -p stackly-core --test connection_render` → 13 test lulus; golden baru `golden/provider/traefik/connection.json` dengan `web` `http://127.0.0.1:14001` dan `ping` `http://127.0.0.1:14000/ping` → AC-1
- [x] `cargo test -p stackly-core --test provider_check every_repository_package_is_publishable_with_the_implied_default_scenario` → lulus; Traefik membawa dua skenario `default` dan `one-route`, paket lain satu skenario tersirat → AC-1
- [x] `cargo test -p stacklyd --test create_form_fixtures` → lulus dengan `protocols/fixtures/instance/create-form-traefik.json` yang membawa `manifestSha256` `c57b6a82…` → AC-1
- [x] `cargo xtask fuzz seed` lalu `git status --short fuzz` → bersih (seed `provider_manifest/traefik.provider.yaml` dan `provider_interpolation/field-*` sudah ikut di commit langkah 2) → AC-1
- [x] `cargo xtask codegen --check` → `codegen: 20 generated files are up to date` → AC-1
- [x] `cargo xtask docs check` → `ok docs 26` → AC-1
- [x] `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test provider_packages --test open_with_render --test connection_render` lalu `git status --short crates/core/tests/golden` → bersih → AC-1

### Acceptance-criteria coverage

- AC-1: semua langkah milestone ini. Bagian "golden plan dan render lolos" dan "schema serta template tidak berubah" dicek di sini; entri catalog dan freeze menunggu milestone 3 (AC-10).
- AC-6: bagian statis (`api.dashboard: false` di `traefik.yaml` hasil render) dicek di sini; bagian HTTP di Milestone 2.
- AC-7: bagian statis (tanpa capability `tls`, tanpa setting certificate atau key, `backendUrl` hanya `http://`, file render tanpa key `tls`, `permissions` persis dua entri) dicek di sini; file render di disk di Milestone 2; byte `/etc/hosts` menunggu milestone 3 (`traefik_compatibility`).
- AC-13: bagian manifest (remedy cocok hanya untuk `routes_mismatch`, tiga langkah, langkah ketiga folder `dynamic`) dicek di sini; `doctor.run` pada instance asli di Milestone 2.
- AC-2 sampai AC-5, AC-8, AC-11: Milestone 2. AC-9: milestone 4. AC-10, AC-12, AC-14: milestone 3 dan 4.

## Milestone 2: Routing terbukti pada Traefik 3.7.15 asli

_Build 2026-11-12 (dev-f41-m1): langkah 4 sampai 7, commit `2569eab3` (langkah 4), `6d9192fc` (langkah 5), dan `25a5c93e` (langkah 6); langkah 7 tidak mengubah file. Run native di Mac ini dengan artefak dari cache: semua record `PASS` dengan subject `traefik 3.7.15` digest `402f59ce…` `managed`. Fakta AC-4 pada 3.7.15 sesuai spec: Traefik memuat file baru, router dengan ``Host(`` menjadi `disabled` (detail readiness `router "broken" is "disabled"`), dan router lain di revision itu menjawab 200. Satu catatan: apply yang hanya mengganti backend tanpa mengganti nama router lulus verify sebelum Traefik memuat backend baru (probe membandingkan nama router), jadi test menunggu jawaban baru dalam batas `reloadSeconds`; di run ini jawaban baru datang sekitar 0,8 s sesudah apply._

### UI / manual

- [x] Tidak ada langkah GUI di milestone ini. Langkah manual GUI spec 0041 (*Langkah manual* 1 sampai 7) masuk milestone 4.

### Commands

- [x] `cargo test -p stacklyd --test traefik -- --ignored --test-threads=1 routing surface watch` → 6 test lulus. Dengan `STACKLY_EVIDENCE_DIR=<dir> STACKLY_EVIDENCE_KIND=native`, setiap record `PASS` (bukan `BLOCKED`), `covers` `REQ-039` dan `AC-039` (AC-3 juga `AC-015`) → AC-2 sampai AC-8, AC-13
- [x] `cargo test -p stacklyd --test traefik -- --ignored two_instances_with_the_same_route_name_and_rule_answer_from_their_own_backend` → dua instance `ready`, keduanya route `api` ``Host(`app.localhost`)``, A menjawab `alpha` dan B `bravo`; backend baru di A menjawab `charlie` dalam 30 s dengan PID A sama; PID, nomor revision, byte `dynamic/routes.yaml`, dan jawaban B sama sebelum dan sesudah → AC-2
- [x] `cargo test -p stacklyd --test traefik -- --ignored invalid_route_values_are_refused_and_nothing_is_written` → `validate` dan `apply` (`instance.settings_invalid`) menolak nama ganda di `settings.routes[1].name`, nama `Api_1` di `settings.routes[0].name`, rule kosong di `settings.routes[0].rule`, `http://example.com` dan `https://127.0.0.1:<port>` di `settings.routes[0].backendUrl`, dan 65 route di `settings.routes`; revision tetap 1, riwayat sama, byte `routes.yaml` dan jawaban route sama → AC-3
- [x] `cargo test -p stacklyd --test traefik -- --ignored a_rule_traefik_rejects_fails_the_apply_keeps_last_known_good_and_rolls_back` → apply dengan `good` dan `broken` (``Host(``) berakhir `failed` dengan `supervisor.reload_failed`; readiness `not_ready`, reason `routes_mismatch`, detail menyebut `broken`; PID sama; revision 2 tanpa `ready_at`, last known good tetap 1; `good.localhost` menjawab 200 dan route lama 404; tiga detik kemudian revision masih 2 (tidak ada roll back sendiri); `Rollback` menulis revision 3 (`origin` `rollback`, `sourceNumber` 1), readiness `ready` dengan PID sama, route lama menjawab `alpha`, last known good 3; instance kedua tidak berubah sepanjang urutan → AC-4
- [x] `cargo test -p stacklyd --test traefik -- --ignored ping_and_the_api_answer_only_on_the_monitoring_port_and_the_dashboard_is_off` → di port `ping`: `/ping` 200, `/api/http/routers` 200, `/dashboard/` 404; di port `web` tanpa route: `/ping`, `/api/http/routers`, `/dashboard/`, dan `/` 404; `lsof -a -p <pid> -i -n -P` hanya dua `LISTEN`, `127.0.0.1:<web>` dan `127.0.0.1:<ping>` dari registry; sesudah satu route ``Host(`app.localhost`)``, request ber `Host` `other.localhost` ke ketiga path di `web` 404; `traefik.yaml` dan `dynamic/routes.yaml` di disk tanpa key `tls` dan `api.dashboard: false` → AC-5, AC-6, AC-7
- [x] `cargo test -p stacklyd --test traefik -- --ignored ten_applies_in_a_row_never_drop_the_route_that_stays` → sepuluh apply berturut turut yang mengganti backend `moving` semuanya `succeeded`, dan poller berurutan pada `steady` melihat 200 dengan isi `stays` di setiap request (run ini: 46687 request, nol yang salah); PID sama → AC-8
- [x] `cargo test -p stacklyd --test traefik -- --ignored a_router_in_a_file_the_user_added_makes_the_instance_not_ready_until_it_is_moved` → `config/dynamic/mine.yaml` dengan router `mine` membuat readiness `not_ready`, `routes_mismatch`, detail `router "mine" is loaded but not in the routes file`; `doctor.run` (`instance_failures`) membawa remedy `routes_not_loaded` dari `official/traefik` dengan tiga langkah, langkah ketiga path folder `dynamic` instance itu; isi file sama; sesudah file dipindah keluar readiness `ready` lagi dengan PID sama → AC-8, AC-13
- [x] `cargo test -p stacklyd --test provider_conformance -- --ignored traefik_3_7_passes_conformance` → lulus, record `PASS` `traefik 3.7.15 ready in <ms> ms, stopped in <ms> ms` → AC-11
- [x] `cargo xtask provider test providers/traefik --line 3.7 --report <file>` → `default: pass`, `one-route: pass`, `verdict: pass`, exit 0 (bukti kedua skenario paket ikut berjalan) → AC-11
- [x] `cargo test -p stacklyd --test traefik -- --ignored` → semua test Traefik lulus, termasuk `config.rs` dan `readiness.rs` yang lama → AC-2 sampai AC-8
- [x] Ganti sementara nama folder cache runtime artefak (atau jalankan dengan cache kosong) lalu jalankan satu test di atas dengan `STACKLY_EVIDENCE_DIR` → record `BLOCKED` dengan `reason` `artifact traefik-3.7.15-macos-aarch64 is not in the runtime cache`; kembalikan cache → AC-11

### Acceptance-criteria coverage

- AC-2: `routing::two_instances_with_the_same_route_name_and_rule_answer_from_their_own_backend`.
- AC-3: `routing::invalid_route_values_are_refused_and_nothing_is_written`.
- AC-4: `routing::a_rule_traefik_rejects_fails_the_apply_keeps_last_known_good_and_rolls_back`; perilaku Traefik 3.7.15 sesuai spec, tidak ada `needs-decision`.
- AC-5, AC-6: `surface::ping_and_the_api_answer_only_on_the_monitoring_port_and_the_dashboard_is_off`.
- AC-7: file render di disk di test `surface`; bagian `/etc/hosts` menunggu `traefik_compatibility` milestone 3.
- AC-8: `watch::ten_applies_in_a_row_never_drop_the_route_that_stays` dan `watch::a_router_in_a_file_the_user_added_makes_the_instance_not_ready_until_it_is_moved`.
- AC-11: bagian konformitas (`traefik_3_7_passes_conformance` dengan dua skenario paket). `traefik_compatibility`, `compatibility.json`, dan `versions.json` menunggu milestone 3.
- AC-13: `doctor.run` pada instance asli di test `watch`; `stackly doctor --instance` menunggu milestone 4.
- AC-9: milestone 4. AC-10, AC-12, AC-14: milestone 3 dan 4.

## Milestone 2b: Temuan verify milestone 1 dan 2

_Build 2026-11-12 (dev-f41-m2b): langkah 7a, commit `d898f493`, dan langkah 7b. `manifest_sha256` paket berubah untuk terakhir kalinya, menjadi `c57b6a8274d55f6c0b0f65fd93abcca1ee661b66e8a4793183728e5bfdd01bd7`. Run native di Mac ini dengan artefak dari cache, di atas paket baru: semua record `PASS` dengan subject `traefik 3.7.15` (dibaca dari `GET /api/version`) digest `402f59ce…` `managed`. Pada run ini keenam bentuk AC-15 dimuat `enabled` sekitar 1,6 s sesudah apply, dan backend baru AC-2 menjawab sekitar 0,8 s sesudah apply. Dua catatan tafsiran: golden `init.json`, `run.json`, dan `reload.json` ikut berubah, karena ketiganya memuat render `routes.yaml` yang sama dengan `render-routes.txt` (satu baris URL route ketiga); dan `surface.rs` membaca file render lewat parse indentasi YAML blok menjadi path key, karena `stacklyd` tidak punya parser YAML di dependency nya dan `Cargo.toml` di luar ownership langkah ini._

### UI / manual

- [x] `git diff main -- providers/traefik/schemas` (dari `main` sebelum langkah 7a) → hanya satu baris `pattern` `backendUrl`, `http://(127\.0\.0\.1|\[::1\]|localhost):[0-9]{1,5}/?`; `title`, `description`, dan `maxLength` field itu sama → AC-1, AC-15
- [x] Buka `providers/traefik/README.md` → bagian "Changing routes" punya paragraf URL backend (scheme, host loopback, port; path, query, fragment ditolak) dan paragraf "up to about two seconds"; bagian "The `dynamic` folder" punya paragraf "Stackly compares the routers Traefik loaded, not their backends" → AC-1
- [x] `grep -rn '"3.7.15")' apps/daemon/tests/traefik/` → kosong; setiap `site.subject` memakai `server_version(port_of(.., "ping"))` sesudah instance pertama `ready` → AC-11

### Commands

- [x] `cargo xtask provider check providers/traefik` → `ok official/traefik 1.0.0 c57b6a8274d55f6c0b0f65fd93abcca1ee661b66e8a4793183728e5bfdd01bd7` → AC-1
- [x] `cargo test -p stackly-core --test provider_packages traefik_backend_url_is_a_scheme_a_loopback_host_and_a_port_only` → enam nilai kolom kiri tabel butir 7 lolos; sepuluh nilai kolom kanan, ditambah `http://127.0.0.2:9`, `http://example.com:80`, `https://127.0.0.1:9`, `HTTP://127.0.0.1:9`, dan `http://user@127.0.0.1:9`, masing masing tepat satu issue `pattern` di `routes[0].backendUrl` → AC-15, AC-7
- [x] `cargo test -p stackly-core --test provider_packages traefik_renders_the_dashboard_off_and_never_a_tls_key` → lulus; render dengan route bernama `tls` memuat `http.routers.tls` dan `http.services.tls` dan tetap dinilai tanpa konfigurasi TLS → AC-7
- [x] `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test provider_packages` lalu `git status --short crates/core/tests/golden` → bersih; di commit langkah 7a hanya `render-routes.txt` dan tiga golden plan yang memuat render itu yang berubah, dengan route ketiga `http://[::1]:8080` → AC-1
- [x] `cargo test -p stacklyd --test create_form_fixtures` → lulus dengan `manifestSha256` `c57b6a82…` → AC-1
- [x] `cargo xtask fuzz seed` lalu `git status --short fuzz` → bersih (seed `provider_settings_schema/traefik.settings.schema.json` ikut di commit langkah 7a) → AC-1
- [x] `cargo xtask codegen --check` → `codegen: 20 generated files are up to date`; `cargo xtask docs check` → `ok docs 26` → AC-1
- [x] `swift test --package-path clients/swift` → 281 test lulus (satu flaky beban `aReusedSubscriptionIdEndsTheEarlierStream` lulus saat diulang sendirian) → AC-1
- [x] `cargo test -p stacklyd --test traefik the_tls_rules_of_ac_7_read_names_as_names_and_bodies_as_configuration` → lulus tanpa runtime: nama `tls` di router dan service bukan TLS, key `tls` di isi router atau service, key `tls` tingkat atas, `entryPoints.*.http.tls`, dan `certificatesResolvers` adalah TLS → AC-7
- [x] `cargo test -p stacklyd --test traefik -- --ignored a_backend_url_with_a_path_a_query_a_fragment_or_a_bad_escape_is_refused_and_nothing_is_written` → `validate` dan `apply` menolak `http://127.0.0.1:9/%zz`, `/api`, `/?a=b`, dan `/#x` dengan `instance.settings_invalid` tepat di `settings.routes[0].backendUrl`; revision tetap 1, riwayat sama, byte `routes.yaml` dan jawaban route sama; record `covers` `REQ-039`, `AC-039`, `AC-015` → AC-15
- [x] `cargo test -p stacklyd --test traefik -- --ignored a_backend_only_change_to_every_accepted_url_form_is_loaded_and_becomes_last_known_good` → apply enam route ke enam bentuk AC-15 `succeeded`; dalam 30 s `GET /api/http/services?per_page=1000` di port `ping` menunjukkan `r1@file` sampai `r6@file` dengan URL baru, keenam service dan router `enabled`; readiness `ready`, PID sama, last known good 2, `r1.localhost` menjawab `second` → AC-15
- [x] `cargo test -p stacklyd --test traefik -- --ignored --test-threads=1` dengan `STACKLY_EVIDENCE_DIR=<dir> STACKLY_EVIDENCE_KIND=native` → 10 test lulus (`config`, `readiness`, `routing` lima test, `surface`, `watch` dua test); setiap record `PASS` dengan subject `traefik 3.7.15` dari proses yang berjalan → AC-2, AC-7, AC-11, AC-15
- [x] `cargo test -p stacklyd --test provider_conformance -- --ignored traefik_3_7_passes_conformance` → lulus di atas paket baru, record `PASS` → AC-11

### Acceptance-criteria coverage

- AC-1: sha akhir, satu baris `pattern`, README, golden, fixture form, seed, codegen, dan docs di sini; entri catalog dan freeze menunggu milestone 3 (AC-10).
- AC-2: kalimat batas jawaban dibuktikan ulang oleh `routing::two_instances_with_the_same_route_name_and_rule_answer_from_their_own_backend` di atas paket baru.
- AC-7: aturan TLS pada file yang sudah diparse di test fixture core dan di `surface.rs`; byte `/etc/hosts` di `traefik_compatibility` (kode langkah 8, run native milestone 3).
- AC-11: versi subject dari `GET /api/version` di setiap test native Traefik; konformitas lulus di atas paket baru. Record evidence kompatibilitas, `versions.json`, dan catalog menunggu milestone 3.
- AC-15: bagian schema di test fixture core, bagian validasi dan Traefik asli di dua test `routing.rs`.

## Milestone 3: Evidence, catalog, dan handover

_Build 2026-11-12: kode langkah 8 commit `ff337dea` (dev-f41-m2b); commit evidence langkah 8 dan 9 `6647adb7` dan langkah 10 `7499915f` (dev-f41-m3). Run native `01a1cdd2-3059-7223-b72d-b828f79622d2` dari `HEAD` bersih `67b8a452`: 12 record native `PASS`, 0 problem. Catalog revision 13. Test launchd langkah 10 sudah ditulis tetapi tidak dijalankan (butuh sesi GUI kamu), jadi baris `HandoverProof` Traefik di `crates/core/src/supervisor/handover.rs` dan baris `("official/traefik", "1.0.0", false)` di modul test nya belum diubah. Keduanya menyusul bersama record `PASS` sesudah kamu menjalankan langkah manual 7 di bagian Milestone 4. AC-12 tetap terbuka sampai itu._

### UI / manual

- [ ] Tidak ada langkah GUI di bagian ini. Test launchd AC-12 (langkah 10) ada di langkah manual 7 di bagian Milestone 4.

### Commands

- [x] `cargo test -p stacklyd --test traefik -- --ignored traefik_compatibility` dengan `STACKLY_EVIDENCE_DIR=<dir> STACKLY_EVIDENCE_KIND=native` → lulus; record `PASS` dengan `covers` `BR-025`, `REQ-039`, `AC-039` dan subject `traefik 3.7.15` dari `GET /api/version`; `<dir>/artifacts/<id>/compatibility.json` memuat `providerRevision` `official/traefik` `1.0.0` `c57b6a82…`, `checks` `health`, `http`, `init`, `invalid_config`, `readiness`, `rollback`, `routing_update`, `second_instance`, `start`, `stop`, dan `knownLimitations` `dashboard_not_offered`, `https_not_offered`; byte `/etc/hosts` sama sebelum dan sesudah → AC-11, AC-7
- [x] `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test traefik --locked -- --ignored --test-threads=1` dari `HEAD` bersih → 12 record native `PASS`, 0 problem, `sourceDirty` `false`; run `01a1cdd2-3059-7223-b72d-b828f79622d2` → AC-11
- [x] `cargo xtask runtime compat-record target/evidence/01a1cdd2-3059-7223-b72d-b828f79622d2/report.json` → `evidence/compatibility/01a1cdd2-3059-7223-b72d-b828f79622d2/`; lalu `cargo xtask codegen` → `providers/traefik/versions.json` mendapat satu record scope `provider` `pass` dengan sepuluh check, dua `knownLimitations`, dan `manifestSha256` `c57b6a82…` → AC-11
- [x] `providers/catalog.toml`: `[[planned]]` `official/traefik` hilang, `[[provider]]` `official/traefik` (`Tr`, `cyan`, dua `platform_note`) ada, `revision = 13`; `cargo xtask catalog check --update` → `ok catalog revision 13 39c35a35…` → AC-10
- [x] `cargo xtask provider freeze providers/traefik` → `crates/core/tests/fixtures/compat/traefik/1.0.0/` dengan `frozen.json` `c57b6a82…`; `diff -r crates/core/tests/fixtures/compat/traefik/1.0.0/package providers/traefik` hanya menyebut `README.md` dan `LICENSE-NOTICES.md` → AC-10
- [x] `cargo test -p xtask --test provider_check` → lulus; `a_package_that_never_enters_the_catalog_is_refused_before_anything_is_written` memakai `providers/example` → AC-10
- [x] `cargo clippy -p stacklyd --all-targets --locked -- -D warnings` → test launchd Traefik ter-compile tanpa baris `HandoverProof` → AC-12 (kode)
- [ ] Langkah manual 7 di bagian Milestone 4 → AC-12 (menunggu sesi GUI kamu)

### Acceptance-criteria coverage

- AC-10: catalog revision 13, ledger, dan freeze di sini.
- AC-11: record evidence, `versions.json`, dan konformitas di sini dan di Milestone 2b.
- AC-7: byte `/etc/hosts` di `traefik_compatibility`.
- AC-12: kode test di sini; run dan baris `HANDOVER_PROVEN` menunggu langkah manual 7.
- AC-14: diff akhir di bagian Milestone 4.

## Milestone 4: Journey dari CLI dan GUI

_Build 2026-11-12 (dev-f41-m2b): langkah 11, commit `dec8f746`, dan langkah 12 (bagian ini). Run native langkah 11 di Mac ini: record `PASS`. Langkah manual di bawah dijalankan kamu, bukan sesi build; langkah 1 butuh unduhan dan langkah 7 butuh sesi GUI yang login. Pemeriksaan AC-14 atas diff sesudah milestone 3 dicatat di bawah (dev-f41-m3)._

### UI / manual

GUI mengikuti Interaction policy di `AGENTS.md`: hanya kontrol pointer dan teks biasa, tanpa shortcut, VoiceOver, atau identifier test.

- [ ] 1. Di app: buka Catalog, pilih Traefik (sekarang tersedia), pasang runtime 3.7.15 (unduhan, hanya kamu yang menjalankan), buat instance `edge-a` dan `edge-b`, lalu Start keduanya → AC-9 (Traefik ada di catalog sejak revision 13)
- [ ] 2. Di `edge-a`, tab Configuration: tambah satu route di daftar Routes (Name `api`, Rule ``Host(`app.localhost`)``, Backend URL ke server lokal kamu). Coba dulu Backend URL dengan path, misalnya `http://127.0.0.1:3000/api`: periksa error di field itu dan bahwa tidak ada revision baru, lalu hapus path nya. Periksa tag "Applies on reload" dan tombol Apply and Reload, lalu Apply. Jalankan `curl -H 'Host: app.localhost' http://127.0.0.1:<port web edge-a>/` dan periksa jawabannya; ulangi ke port `edge-b` dan periksa 404 → AC-9, AC-15, AC-2
- [ ] 3. Ubah Log level: periksa tag "Requires restart" dan tombol Apply and Restart → AC-9
- [ ] 4. Ganti Rule menjadi `Host(` lalu Apply: periksa kartu gagal, status not ready, dan banner "Revision N hasn’t reached ready". Klik Roll Back to Revision M dan periksa route menjawab lagi → AC-4
- [ ] 5. Menu Open In: Routes File membuka Finder di `routes.yaml`; Loaded Routers membuka JSON router di browser → AC-1
- [ ] 6. Doctor saat langkah 4 sebelum roll back: periksa remedy "Traefik hasn’t loaded the routes Stackly wrote" → AC-13
- [ ] 7. Test launchd AC-12, di sesi GUI kamu yang login. Test ini menuntut rencana restart memutuskan `keep` dengan alasan `handover_proven`, jadi baris proof dipasang dulu secara lokal: di `crates/core/src/supervisor/handover.rs` tambah `HandoverProof::new("official/traefik", "1", "spec-0041-ac-12"),` di `HANDOVER_PROVEN`, dan di modul test nya ubah `("official/traefik", "1.0.0", false)` menjadi `true`. Lalu `cargo xtask evidence run --kind native -- cargo test -p stacklyd --test native_launchd --locked -- --ignored --test-threads 1 traefik_survives`. Yang diharapkan: `traefik_survives_planned_restart_and_sigkill_and_returns_after_kickstart_and_bootout` lulus, PID dan waktu mulai yang sama sesudah planned restart dan SIGKILL daemon, route menjawab `alpha` sesudah setiap akhir daemon, dan proses baru sesudah `kickstart -k` dan `bootout`. Record `PASS`: commit kedua perubahan `handover.rs` bersama ID record evidence nya. `FAIL` atau `BLOCKED`: buang kedua perubahan itu, dan AC-12 tetap terbuka. Tanpa sesi GUI test berakhir `BLOCKED`, `reason` sesi login GUI, `owner` `@ojiepermana` → AC-12, AC-14
- [x] `git diff --stat main...HEAD` sesudah milestone 3 (dev-f41-m3) → di `crates/` hanya test, golden, dan fixture compat (`crates/core/tests/`); tidak ada file di `crates/core/src/`, `crates/protocol/`, `apps/macos/`, atau modul format config, probe, dan hash `crates/core`. Baris `HandoverProof` yang AC-14 izinkan belum ada, karena menunggu langkah manual 7 → AC-14

### Commands

- [x] `cargo test -p stacklyd --test traefik -- --ignored cli_applies_routes_with_a_reload_and_the_log_level_with_a_restart` dengan `STACKLY_EVIDENCE_DIR=<dir> STACKLY_EVIDENCE_KIND=native` → lulus, record `PASS`: `instance config apply edge-cli --set-json routes=<json> --dry-run` menilai `reload`, apply dengan `--wait` `succeeded`, route menjawab backend baru dengan PID sama; `--set logLevel=DEBUG` menilai `restart`, PID baru, route tetap menjawab; `instance connection edge-cli --all` menunjukkan `web` `http://127.0.0.1:<web>` dan `ping` `http://127.0.0.1:<ping>/ping`, keduanya `available`; apply dengan rule `Host(` keluar bukan 0 dengan `supervisor.reload_failed`, lalu `doctor --instance edge-cli` membawa remedy `routes_not_loaded` dari `official/traefik` dengan tiga langkah, langkah ketiga path folder `dynamic` instance itu → AC-9, AC-13
- [x] Test itu memakai binary `stackly` lewat `stackly_testkit::cargo_bin`, yang membangun ke `<repo>/target/testkit-bin` → catatan run, bukan AC

### Acceptance-criteria coverage

- AC-9: bagian CLI di `cli::cli_applies_routes_with_a_reload_and_the_log_level_with_a_restart`; bagian GUI di langkah manual 1 sampai 3.
- AC-13: `stackly doctor --instance` di test yang sama; GUI di langkah manual 6.
- AC-15: bagian GUI di langkah manual 2.
- AC-12: kode test di Milestone 3; run dan baris `HANDOVER_PROVEN` di langkah manual 7.
- AC-14: pemeriksaan diff sesudah milestone 3 di atas; satu baris `HandoverProof` menyusul langkah manual 7.
