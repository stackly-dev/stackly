# 0041. Provider Traefik dan routing lokal

**Date**: 2026-11-12
**Status**: In Progress

## Summary

Spec ini menjadikan Traefik provider resmi Stackly untuk routing HTTP lokal: kamu memasangnya dari catalog, membuat dua instance atau lebih, lalu mengatur route (aturan yang mengirim request ke backend lokal) lewat halaman Configuration yang sudah ada. Paket `providers/traefik`, probe `traefik-routes`, dan halaman Configuration sudah ada di `main`, jadi fitur ini hampir seluruhnya pekerjaan paket dan bukti native: blok `connection`, satu aksi "Open In", satu remedy doctor, skenario konformitas, satu baris pola URL backend di settings schema, test dua instance dengan Traefik 3.7.15 asli, evidence kompatibilitas, entri catalog, dan bukti handover. Tidak ada perilaku baru di core, protokol, atau SwiftUI; satu satunya baris core yang berubah adalah entri `HANDOVER_PROVEN`.

Empat batas dinyatakan terus terang. Revision ini hanya melayani HTTP: HTTPS tidak ditawarkan sampai Stackly bisa membuktikan certificate milik kamu yang benar benar dipakai. Dashboard Traefik selalu mati; yang ada hanya API read only di port monitoring loopback, yang dibutuhkan probe. Rule yang lolos schema tetapi ditolak parser Traefik tidak menghapus last known good (revision terakhir yang pernah ready): apply gagal, instance menjadi not ready, dan kamu yang memilih memperbaiki rule atau roll back. URL backend hanya berisi scheme, host loopback, dan port: Traefik mengabaikan path di URL server, dan path yang tidak bisa dibacanya mematikan route sesudah Stackly sudah menganggap revision itu ready, jadi schema menolak path sama sekali (update 2026-11-12 dari temuan verify milestone 1 dan 2, AC-15).

## Requirements

**User stories**:

- Sebagai developer, saya ingin proxy lokal per proyek yang route nya saya ubah dari Stackly, supaya beberapa service lokal bisa dijangkau lewat satu port tanpa Docker dan tanpa mengedit file YAML.
- Sebagai developer dengan dua proyek, saya ingin perubahan route di proxy A tidak pernah menyentuh proxy B, supaya eksperimen di satu proyek tidak memutus proyek lain.
- Sebagai developer, saya ingin route yang salah ketik tidak diam diam dianggap sukses, dan saya bisa kembali ke config terakhir yang jalan dengan satu aksi.
- Sebagai maintainer Stackly, saya ingin bukti bahwa provider ketiga yang nyata masuk catalog tanpa perubahan perilaku core, supaya ukuran RISK-07 (schema manifest terlalu sempit) terjawab dengan angka.

**Acceptance criteria** (ID lokal spec ini; ID PRD selalu tiga digit, misalnya AC-039):

- **AC-1** (paket resmi, REQ-039, REQ-029): `providers/traefik` tetap revision `1.0.0` (belum pernah terbit, jadi ini terbitan pertamanya) dan mendapat tepat tambahan di *Perubahan paket*: blok `connection` (dua entri), entri `openWith` `routers`, satu entri `remedies`, `tests/scenarios.yaml` (dua skenario), README yang ditulis ulang, dan satu baris `pattern` `backendUrl` di settings schema (AC-15). `cargo xtask provider check providers/traefik` mencetak `ok official/traefik 1.0.0 <sha256>`, golden plan dan render paket lolos, kedua template tidak berubah byte demi byte, dan `git diff main -- providers/traefik/schemas` hanya menunjukkan baris `pattern` itu. *Update 2026-11-12*: kalimat lama "settings schema tidak berubah" diganti, karena temuan verify milestone 1 dan 2 dijawab lewat pola itu.
- **AC-2** (isolasi dan namespace per instance, AC-039): dengan dua instance Traefik 3.7.15 yang `ready` dan satu backend uji per instance, kedua instance memakai nama route yang sama (`api`) dan rule yang sama (``Host(`app.localhost`)``) ke backend berbeda, dan masing masing hanya menjawab dari backend miliknya. Mengubah route di A (backend baru) mengubah jawaban A dalam batas `reloadSeconds`, sedangkan di B PID, nomor revision, byte `dynamic/routes.yaml`, dan jawaban request tidak berubah. PID A juga tidak berubah (reload, bukan restart). Batas `reloadSeconds` di sini adalah batas jawaban, bukan batas operasi: untuk apply yang hanya mengganti backend, operasi `apply_config` boleh `succeeded` dan revision boleh mendapat `ready_at` sebelum Traefik melayani backend baru (lihat *Yang tidak dilihat verify*), jadi test menunggu jawaban baru, bukan akhir operasi.
- **AC-3** (nilai tidak valid ditolak sebelum ditulis, AC-039, AC-015): `instance.config.validate` dan `instance.config.apply` menolak nama route ganda, `name` di luar pola, `rule` kosong, `backendUrl` yang bukan `http://` ke loopback, dan lebih dari 64 route, dengan field error di path `settings.routes[<i>].<field>`: nama ganda dilaporkan di `settings.routes[<i>].name` item yang belakangan, dan jumlah di `settings.routes`. Tidak ada revision baru, byte `dynamic/routes.yaml` tidak berubah, dan route yang sedang jalan tetap menjawab.
- **AC-4** (rule yang ditolak Traefik, AC-039): apply route yang lolos schema tetapi rule nya ditolak parser Traefik (contoh tetap: `Host(`) berakhir sebagai operasi `apply_config` yang gagal dengan `supervisor.reload_failed`; readiness menjadi `not_ready` dengan reason `routes_mismatch` dan detail yang menyebut nama router itu; PID tidak berubah; nomor last known good tetap nomor revision sebelumnya dan revision baru tidak mendapat `ready_at`. Route lain di revision itu yang diterima Traefik tetap menjawab. `instance.config.rollback` lalu menulis revision baru dari last known good, readiness kembali `ready` dengan PID yang sama, dan route lama menjawab lagi. Stackly tidak pernah roll back sendiri (spec 0029 AC-8). Instance kedua tidak berubah selama seluruh urutan ini.
- **AC-5** (health terpisah dan loopback, REQ-039): `GET /ping` menjawab 200 di port endpoint `ping`. Traefik sendiri tidak pernah menyajikan `/ping`, API, atau dashboard di port `web` (di sana hanya route kamu yang menjawab), jadi pada instance tanpa route path yang sama di port `web` menjawab 404. Kedua listener hanya ada di `127.0.0.1` dengan port yang sama dengan registry, dan proses Traefik tidak punya listener lain (kontrak proses harness spec 0036).
- **AC-6** (dashboard mati, API read only): `traefik.yaml` hasil render selalu memuat `api.dashboard: false`, dan tidak ada setting di revision ini yang menyalakannya. `GET /dashboard/` menjawab 404 di port `ping`, dan di port `web` pada instance tanpa route. `GET /api/http/routers` menjawab 200 di port `ping`, dan 404 di port `web` pada instance tanpa route.
- **AC-7** (tanpa HTTPS, tanpa perubahan sistem, REQ-039): paket tidak punya capability `tls`, tidak punya field certificate atau key, dan `backendUrl` hanya menerima `http://`. File hasil render tidak pernah memuat konfigurasi TLS: `traefik.yaml` tidak punya key `tls` di kedalaman mana pun (termasuk `entryPoints.*.http.tls`) dan tidak punya `certificatesResolvers`; `dynamic/routes.yaml` tidak punya key `tls` tingkat atas dan tidak punya key `tls` di dalam isi satu router atau service pun (`http.routers.<nama>.tls`, `http.services.<nama>.tls`). Nama route `tls` tetap sah: key `http.routers.tls` dan `http.services.tls` yang muncul karenanya adalah nama, bukan konfigurasi TLS. Sepanjang run native dua instance, byte `/etc/hosts` sama sebelum dan sesudah. `permissions` paket hanya `managedFilesystem: [data, config, logs, temp]` dan `executeRuntimeBinary`, jadi tidak ada jalur untuk menulis root CA, DNS, atau `/etc/hosts`.
- **AC-8** (penulisan atomik dan folder yang diawasi, PRD 22.5): sepuluh apply berturut turut yang mengganti backend satu route sambil mempertahankan route kedua semuanya berakhir `succeeded`, dan route kedua menjawab 200 pada setiap request selama pergantian (polling paralel), tidak pernah 404 atau 502. File yang kamu taruh sendiri di `config/dynamic/` dan memuat router membuat readiness menjadi `not_ready` dengan `routes_mismatch` dan detail `is loaded but not in the routes file`; Stackly tidak menghapus atau mengubah file itu, dan readiness kembali `ready` setelah file itu kamu pindahkan.
- **AC-9** (halaman Configuration dan CLI, REQ-015, REQ-028): pada instance Traefik asli, `stackly instance config apply <nama> --set-json routes=<json>` menghasilkan dampak `reload` dan route menjawab; `--set logLevel=DEBUG` menghasilkan dampak `restart` dengan PID baru. Tab Configuration menampilkan `routes` sebagai daftar bertipe dari schema (field Name, Rule, Backend URL per item) dengan tag "Applies on reload", `logLevel` dengan "Requires restart", dan `dynamicDir` terkunci "Set by Stackly", tanpa kode SwiftUI khusus Traefik (spec 0029 AC-15, AC-16, AC-18). Bagian GUI dibuktikan langkah manual.
- **AC-10** (catalog, spec 0017): `official/traefik` pindah dari `[[planned]]` ke `[[provider]]` di `providers/catalog.toml` pada commit yang sama dengan evidence kompatibilitas AC-11 yang `PASS`, dengan `revision` dan `published_at` baru; `cargo xtask catalog check` lolos, dan `cargo xtask provider freeze providers/traefik` menulis salinan beku revision `1.0.0` sesudahnya.
- **AC-11** (konformitas dan evidence, AC-029, BR-025, PRD 32.2): `traefik_3_7_passes_conformance` lulus dengan dua skenario paket. Test native `traefik_compatibility` menambah setiap check hanya setelah assertion nya lulus (`init`, `start`, `readiness`, `health`, `http`, `second_instance`, `routing_update`, `invalid_config`, `rollback`, `stop`), lalu menulis `compatibility.json` dengan `knownLimitations` `https_not_offered` dan `dashboard_not_offered`; arti tiap check ada di *Evidence kompatibilitas*. Setiap test native baru di `apps/daemon/tests/traefik/` memasang guard evidence `native` dengan `covers` `REQ-039` dan `AC-039` (test kompatibilitas menambah `BR-025`). Versi di subject evidence setiap test native Traefik, yang lama maupun yang baru, dibaca dari proses yang berjalan (field `Version` jawaban `GET /api/version` di port `ping`), tidak pernah dari literal di kode test. Test launchd memakai `covers` test launchd yang ada (`REQ-013`, `AC-013`), dan `covers` test konformitas tidak berubah. Artefak yang tidak ada di cache runtime xtask berakhir `BLOCKED`, tidak pernah unduhan.
- **AC-12** (handover, spec 0030 AC-8): test native launchd `traefik_survives_planned_restart_and_sigkill_and_returns_after_kickstart_and_bootout` membuktikan proses Traefik yang sama tetap menjadi proses instance yang `ready` melewati restart daemon terencana dan `SIGKILL` daemon, dan route nya tetap menjawab sesudahnya. Test itu menuntut keputusan `keep` dengan reason `handover_proven` dari binary yang sedang diuji, jadi baris `HandoverProof::new("official/traefik", "1", "spec-0041-ac-12")` ditambahkan ke `HANDOVER_PROVEN` di langkah yang sama, sebelum run. Baris itu hanya boleh masuk commit kalau record nya `PASS`; kalau `FAIL` atau `BLOCKED` baris itu dibuang lagi, dan `daemon.restart_preview` tetap menaruh instance Traefik di `stop_restore` (cara yang sama dengan NATS di spec 0030).
- **AC-13** (remedy doctor, spec 0034): untuk instance Traefik dengan readiness `routes_mismatch`, `doctor.run` membawa remedy `routes_not_loaded` dari manifest dengan tiga langkah di *Perubahan paket*; langkah ketiga membawa path folder `dynamic` instance itu.
- **AC-14** (ukuran RISK-07): diff fitur ini terhadap `main` di `crates/*/src/`, `apps/*/src/`, `clients/swift/Sources/`, dan `apps/macos/` hanya menyentuh `crates/core/src/supervisor/handover.rs`: satu baris data di `HANDOVER_PROVEN` dan modul `#[cfg(test)]` file itu (baris `("official/traefik", "1.0.0", false)` menjadi `true`). `ProtocolVersion::CURRENT` dan nomor migrasi terakhir tidak berubah.
- **AC-15** (URL backend yang selalu dimuat Traefik, AC-039, AC-015; ditambah 2026-11-12): `backendUrl` berpola `http://(127\.0\.0\.1|\[::1\]|localhost):[0-9]{1,5}/?`, yaitu scheme `http`, satu dari tiga host loopback, port satu sampai lima digit, dan paling banyak satu `/` penutup. `instance.config.validate` dan `instance.config.apply` menolak `http://127.0.0.1:9/%zz`, `http://127.0.0.1:9/api`, `http://127.0.0.1:9/?a=b`, dan `http://127.0.0.1:9/#x` dengan `instance.settings_invalid` di `settings.routes[<i>].backendUrl`; tidak ada revision baru, byte `dynamic/routes.yaml` tidak berubah, dan route yang sedang jalan tetap menjawab. Pada Traefik 3.7.15 asli, satu apply yang hanya mengganti backend enam route ke enam bentuk yang diterima pola (`http://127.0.0.1:<port>`, `http://127.0.0.1:<port>/`, `http://localhost:<port>`, `http://[::1]:<port>`, `http://127.0.0.1:0`, `http://127.0.0.1:99999`) berakhir `succeeded`; dalam batas `reloadSeconds` `GET /api/http/services` di port `ping` menunjukkan keenam service `@file` dengan URL baru dan status `enabled`, keenam router `enabled`, readiness tetap `ready` dengan PID yang sama, dan nomor last known good adalah nomor revision itu. Dengan begitu tidak ada nilai `backendUrl` yang lolos schema lalu ditolak Traefik sesudah revision nya menjadi last known good.

## Decision

**Chosen option**: paket saja di atas engine yang ada, dengan batas yang dinyatakan (rincian dan opsi lain di [rationale.md](rationale.md)).

Traefik menjadi provider resmi lewat tambahan paket dan bukti native saja. Tujuh keputusan yang membentuknya:

1. **HTTPS tidak ditawarkan di revision `1.0.0`.** Traefik diam diam memakai certificate bawaannya sendiri kalau pasangan certificate dan key kamu tidak bisa dibaca, jadi "HTTPS hanya aktif dengan certificate yang valid" baru bisa dijanjikan setelah ada pemeriksaan certificate di core. Itu dicatat sebagai Follow-up, bukan dibangun setengah.
2. **Dashboard selalu mati, API read only tetap di port monitoring loopback.** API itu dibutuhkan probe `traefik-routes` (spec 0009 AC-22, keputusan audit keamanan 2026-11-05). Blok `openWith` tidak punya syarat, jadi tombol dashboard akan membuka halaman 404 selama dashboard mati.
3. **bcrypt untuk htpasswd tidak ditambahkan.** Tanpa dashboard ber password dan tanpa middleware basic auth, tidak ada yang memakainya.
4. **Namespace route adalah instance itu sendiri**: satu proses, satu folder `config/dynamic/` yang diawasi, nama route unik di dalam instance lewat `x-stackly.key`. Nama router tidak diberi awalan.
5. **Rule yang ditolak Traefik ditangani lewat verify dan roll back eksplisit** (spec 0029 AC-8), bukan lewat proses Traefik sementara untuk pra validasi.
6. **Revision tetap `1.0.0`**, karena paket ini belum pernah terbit di ledger catalog.
7. **URL backend hanya scheme, host loopback, dan port** (2026-11-12). Verify sesudah apply hanya membandingkan nama, rule, dan status router, jadi nilai backend yang ditolak Traefik baru terlihat sesudah revision nya menjadi last known good. Jawabannya ada di schema, bukan di core: pola `backendUrl` hanya menerima bentuk yang selalu dimuat Traefik. Path, query, dan fragment ditolak seluruhnya, karena Traefik 3.7.15 mengabaikan path URL server dan `%` yang bukan `%XX` di path atau fragment adalah satu satunya kelas penolakan yang lolos pola lama. Probe yang juga membandingkan service dicatat sebagai Follow-up.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-async-patterns` (`wshobson/agents`, `.claude/skills/rust-async-patterns/`), keduanya untuk kode test native.

## Feature design

### Trace PRD

| ID | Dijawab oleh |
| --- | --- |
| REQ-039 | AC-1 sampai AC-8 dan AC-15 (entrypoint loopback, file provider, health terpisah, namespace per instance, validasi, apply aman, tanpa root CA, DNS, atau `/etc/hosts`) |
| AC-039 | AC-2, AC-3, AC-4, AC-15 |
| PRD 22.5 | AC-5 (listener health khusus), AC-6 (dashboard), AC-7 (HTTPS), AC-8 (penulisan atomik dan file watching), AC-9 (dampak restart tampil). Satu penyimpangan yang disengaja: PRD menulis "Dashboard/API insecure tidak diaktifkan secara default", sedangkan paket selalu memakai `api.insecure: true` untuk API read only di port `ping`. Itu keputusan spec 0009 AC-22 (audit keamanan 2026-11-05) yang dipertahankan di sini, dan Done when scope hanya menyebut dashboard |
| PRD 32.2 baris Traefik | AC-11: dua instance, routing update, invalid config, HTTP, health. Kolom HTTPS tidak diuji karena capability nya tidak diklaim |

### Yang sudah ada di `main` dan dipakai apa adanya

- Paket `providers/traefik` revision `1.0.0`: endpoint `web` (client, 127.0.0.1, saran 8000) dan `ping` (monitoring, 127.0.0.1, saran 8082), `staticConfig` dan `routes`, init tanpa token port (spec 0009 langkah 22), reload `render: [routes]`, `action: none`, `verify: readiness`, `openWith` `routes` (reveal), `runtime.versionCheck`, dan settings `logLevel` (restart), `dynamicDir` (managed), `routes` (reload, daftar ber key `name`, paling banyak 64).
- Probe native `traefik-routes` (spec 0009 AC-22) di `crates/core/src/provider/probe/traefik.rs` dan `crates/core/src/supervisor/probe/traefik.rs`.
- Artefak `traefik-3.7.15-macos-aarch64` di `runtimes/inventory.toml` dengan evidence spike scope `runtime`.
- Halaman Configuration, revision, last known good, roll back, dan drift (spec 0029), termasuk fixture dan snapshot Traefik.
- Harness konformitas dan test `traefik_3_7_passes_conformance` (spec 0036 AC-15), `PROVIDER_WORDS` yang sudah memuat `traefik`, dan entri `create-form-traefik.json` di `protocols/fixtures/instance/index.json`.
- Test native `apps/daemon/tests/traefik/` (`config.rs`, `readiness.rs`, `harness.rs`) dari spec 0029 dan spec 0009.
- `write_file_atomic` di `crates/platform-macos/src/fs.rs`: byte ditulis ke `.<nama file>.tmp-<pid>-<counter>` di folder yang sama lalu di rename. Nama sementara itu tidak berakhiran `.yaml`, `.yml`, atau `.toml`, jadi file provider Traefik tidak pernah memuatnya.

### Perubahan paket

Semua di `providers/traefik/`. `templates/traefik.yaml.tmpl` dan `templates/routes.yaml.tmpl` tidak berubah. `schemas/settings.schema.json` berubah di satu baris (butir 7).

**1. Blok `connection`** (spec 0021), di `provider.yaml` sesudah `permissions`:

```yaml
connection:
  - name: web
    title: Web URL
    template: "http://${endpoint.web.address}"
  - name: ping
    title: Health URL
    template: "http://${endpoint.ping.address}/ping"
```

**2. Entri `openWith` kedua** (spec 0070), sesudah entri `routes` yang ada:

```yaml
  - name: routers
    title: Loaded Routers
    kind: browser
    browser:
      url: "http://${endpoint.ping.address}/api/http/routers"
```

Aksi ini membuka daftar router yang benar benar dimuat Traefik (JSON read only, termasuk status dan pesan error tiap router) di browser. URL nya selalu ada selama proses hidup, tidak seperti dashboard. Gate spec 0070 AC-6 tetap berlaku, jadi aksi ini hanya aktif saat instance `ready`; saat `not_ready` kamu memakai tab Logs dan remedy doctor.

**3. Blok `remedies`** (spec 0034):

```yaml
remedies:
  - name: routes_not_loaded
    when:
      readinessReasons: [routes_mismatch]
    title: "Traefik hasn’t loaded the routes Stackly wrote"
    steps:
      - text: "Open the Logs tab. Traefik logs the router it rejected and why, usually a rule it can’t parse."
      - text: "Fix the rule in Configuration and apply again, or roll back to the last revision that was ready."
      - text: "A file you added to this folder also causes this, when it holds a router or Traefik can’t read it. Move that file out of the folder."
        path: "${instance.config}/dynamic"
```

**4. `tests/scenarios.yaml`** (spec 0036):

```yaml
schema: stackly.provider-scenarios/v1
scenarios:
  - name: default
  - name: one-route
    settings:
      routes:
        - { name: app, rule: "Host(`app.localhost`)", backendUrl: "http://127.0.0.1:9" }
```

Backend di port 9 tidak perlu ada: readiness hanya membandingkan router yang dimuat, dan Traefik tidak memeriksa backend. Dua jenis skenario sengaja tidak ada. Rule yang ditolak Traefik tidak bisa memakai `expect: start_fails`: start yang melewati batas readiness berakhir `failed` tetapi prosesnya tetap hidup dan `lastFailure` kosong (`ReadinessTimeout`, spec 0010), sedangkan langkah harness itu menuntut `lastFailure` terisi dan tidak ada proses tersisa. Nilai yang melanggar schema (nama route ganda) juga tidak bisa, karena `check_package` memvalidasi settings setiap skenario. Kedua kasus dibuktikan test native AC-3 dan AC-4.

**5. README** (ditulis ulang): menjelaskan dua endpoint, cara route diubah (Configuration atau `stackly instance config apply`), arti `routes_mismatch`, bahwa folder `dynamic` hanya untuk file tulisan Stackly, dan tiga batas: hanya HTTP, dashboard mati, serta "Stackly never edits `/etc/hosts`, DNS, or the system trust store; use a host name that already resolves to this Mac". README tidak dibaca engine.

**6. `capabilities`** tetap `reload: true` dan `multiInstance: true`. `persistence`, `metrics`, `tls`, dan `backup` tidak ditambahkan: Traefik tidak menyimpan data, dan blok metrics Traefik belum dirender.

**7. Pola `backendUrl`** (2026-11-12, AC-15), satu baris di `schemas/settings.schema.json`. `title`, `description`, dan `maxLength` field itu tidak berubah, supaya fixture form di SwiftUI tetap sama (AC-14):

```json
"pattern": "http://(127\\.0\\.0\\.1|\\[::1\\]|localhost):[0-9]{1,5}/?"
```

Pola lama berakhiran `(/[^\s]*)?`, jadi menerima path, query, dan fragment apa pun. Pola selalu mencocokkan seluruh nilai (spec 0009), jadi yang diterima sekarang hanya bentuk di kolom kiri:

| Diterima | Ditolak |
| --- | --- |
| `http://127.0.0.1:3000` | `http://127.0.0.1:3000/api` (path) |
| `http://127.0.0.1:3000/` | `http://127.0.0.1:3000/%zz`, `http://127.0.0.1:3000/%41` (path, dengan escape salah atau benar) |
| `http://localhost:5173/` | `http://127.0.0.1:3000/?a=b`, `http://127.0.0.1:3000?a=b` (query) |
| `http://[::1]:8080` | `http://127.0.0.1:3000/#x`, `http://127.0.0.1:3000#x` (fragment) |
| `http://127.0.0.1:0`, `http://127.0.0.1:99999` (lihat *Edge cases*) | `http://127.0.0.1:3000//`, `http://127.0.0.1`, `http://127.0.0.1:123456`, dan semua yang sudah ditolak pola lama |

Fakta yang mendasarinya, dari probe langsung pada binary Traefik 3.7.15 (Go 1.26.9) dari cache runtime, 2026-11-12 (rincian di [rationale.md](rationale.md)):

- Traefik menolak URL server hanya lewat parser URL Go. Dari semua nilai yang lolos pola lama, yang ditolak hanya `%` yang tidak diikuti dua digit hex di path atau fragment (`invalid URL escape`). Router yang memakai service itu menjadi `disabled`.
- Path, query, dan fragment di URL server tidak dipakai: backend di `http://127.0.0.1:<port>/prefix` menerima request `/some/path?x=1` apa adanya, tanpa `/prefix`.
- Port `0` dan port di atas 65535 dimuat (`enabled`); request ke route itu menjawab 502.
- 605 nilai acak yang cocok dengan pola `%XX` saja (tiga host, port satu sampai lima digit, path dengan karakter ASCII dan Unicode apa pun selain whitespace) semuanya dimuat `enabled`. Pola baru adalah bagian dari himpunan itu.
- Karakter kontrol tidak perlu ditutup pola: dialek settings sudah menolaknya di semua teks (`text cannot hold control characters`).

`metadata.revision` tetap `"1.0.0"`. `manifest_sha256` berubah dua kali: di langkah 1, dan untuk terakhir kalinya di langkah 7a (butir 7 dan tambahan README). Keduanya terjadi sebelum evidence kompatibilitas direkam di langkah 8, dan paket belum pernah terbit di catalog.

**Tambahan README di langkah 7a**, tiga paragraf pendek dalam bahasa Inggris:

- Di bagian cara mengubah route: "A backend URL is a scheme, a loopback host, and a port, such as `http://127.0.0.1:3000`. Stackly refuses a path, a query, or a fragment there: Traefik ignores the path of a server URL, and a path it can’t read would switch the route off after the apply."
- Di bagian yang sama: "An apply that only changes a backend can finish up to about two seconds before Traefik serves the new backend, because Traefik waits that long between two reloads of its files."
- Di bagian folder `dynamic`: "Stackly compares the routers Traefik loaded, not their backends. If a file you added stops Traefik from reading this folder, a backend change can report success while Traefik still serves the old backend."

### Namespace dan isolasi route

- Setiap instance punya proses `traefik` sendiri dengan `providers.file.directory` yang menunjuk `config/dynamic/` instance itu (field managed `dynamicDir`). Tidak ada dua instance yang membaca folder yang sama, karena path instance memuat ID instance (spec 0004).
- Nama router di Traefik adalah `<name>@file` di dalam proses itu, jadi nama `api` di A dan `api` di B adalah dua router yang tidak saling tahu. Keunikan di dalam satu instance dijaga `x-stackly.key: name` saat validasi (spec 0029 AC-2).
- Apply hanya menulis `dynamic/routes.yaml` instance yang dituju, lewat operasi `apply_config` instance itu.

### Apply yang aman, tiga lapis

| Lapis | Kapan | Yang terjadi | Sumber aturan |
| --- | --- | --- | --- |
| 1. Schema | `validate` dan precheck `apply` | nilai yang melanggar schema ditolak `instance.settings_invalid`; tidak ada yang ditulis | spec 0029 AC-2, AC-3 |
| 2. Tulis atomik | langkah reload operasi `apply_config` | revision baru di commit, `routes.yaml` diganti lewat `write_file_atomic`; Traefik memuat file lengkap atau tidak sama sekali | spec 0029 AC-4, spec 0009 |
| 3. Verify | sesudah tulis, dalam `reloadSeconds` (30 s) | probe `traefik-routes` membandingkan nama, rule, dan status router yang dimuat dengan file; tidak sama berarti `supervisor.reload_failed` dan readiness `routes_mismatch` | spec 0009 AC-22, spec 0026 |

Yang terjadi di dalam Traefik saat lapis 3 gagal karena satu rule ditolak: Traefik tetap memuat config baru, menandai router itu `disabled`, dan melayani router lain dari config baru. Jadi route yang rule nya ditolak tidak menjawab (404) sampai kamu memperbaiki atau roll back. Yang tidak pernah berubah: baris last known good di registry, salinan `config-history/` revision itu, dan instance lain. GUI menampilkan banner roll back spec 0029, dan CLI menerima `stackly instance config rollback`. Perilaku Traefik ini dikunci test AC-4 pada 3.7.15; kalau run native menunjukkan lain (misalnya Traefik menahan seluruh config lama), catat faktanya di `verify.md` dan kembali ke `/architect` sebelum AC-4 diubah.

### Yang tidak dilihat verify

Ditambah 2026-11-12 dari temuan verify milestone 1 dan 2. Probe `traefik-routes` tidak membandingkan service (URL backend). Untuk apply yang mengubah nama atau rule, itu tidak masalah: keadaan Traefik yang lama tidak cocok dengan file baru, jadi verify menunggu sampai Traefik memuat file baru, dan router yang service nya gagal dibangun terlihat `disabled`. Untuk apply yang hanya mengganti backend, nama dan rule di file baru sama dengan yang sudah dimuat, jadi verify lulus pada keadaan Traefik yang lama. Tiga akibatnya, dan jawaban spec ini untuk masing masing:

| Akibat | Jawaban |
| --- | --- |
| Operasi `succeeded` dan `ready_at` bisa mendahului muatan Traefik. Yang terukur pada 3.7.15: operasi selesai sekitar 0,3 s sesudah apply, jawaban backend baru datang antara 0,1 s dan 2,3 s. Traefik menunggu sampai 2 s di antara dua muatan file (`providers.providersThrottleDuration`, bawaan 2 s; paket tidak mengubahnya) | Diterima dan dinyatakan: AC-2 mengikat jawaban, bukan akhir operasi, dan README mengatakannya |
| Nilai backend yang ditolak Traefik akan menjadi last known good sebelum router nya `disabled`, dan roll back tidak punya tujuan lagi | Ditutup di lapis 1: pola `backendUrl` (AC-15) hanya menerima bentuk yang selalu dimuat Traefik, jadi kasus ini tidak bisa dibuat lewat settings |
| Kalau Traefik menahan config lama karena sebab di luar file Stackly (file pengguna yang rusak di `dynamic/`), apply yang hanya mengganti backend dilaporkan sukses sementara backend lama tetap dilayani | Residual yang diterima, di area yang tidak didukung (lihat *Edge cases*); README mengatakannya, dan Follow-up mencatat probe yang membandingkan service |

Kalau run native pada line Traefik berikutnya menunjukkan ada bentuk yang diterima pola tetapi tidak dimuat `enabled`, test AC-15 gagal: catat faktanya di `verify.md` dan kembali ke `/architect`. Jangan melonggarkan test nya.

### File milik pengguna di folder `dynamic`

Stackly hanya menulis dan mengawasi `dynamic/routes.yaml` (drift spec 0029 berlaku untuk file itu dan `traefik.yaml`). File lain di folder itu tidak dibaca, tidak dihapus, dan tidak ikut `config-history/`. Traefik tetap memuatnya. File tanpa router (misalnya hanya middleware) tidak mengubah readiness. File dengan router membuat probe melihat router yang tidak ada di file Stackly, jadi instance menjadi `not_ready` dengan `routes_mismatch`; remedy `routes_not_loaded` menunjuk folder itu (AC-8, AC-13). Folder itu tidak didukung sebagai tempat file pengguna: README mengatakannya, dan dua kasus di *Edge cases* menjelaskan akibatnya.

### Entri catalog

Di commit evidence AC-11, `providers/catalog.toml` menghapus `[[planned]]` `official/traefik` dan menambah:

```toml
[[provider]]
id = "official/traefik"
mark = "Tr"
tile = "cyan"

[[provider.platform_note]]
os = "linux"
status = "planned"
note = "Linux releases exist upstream; Stackly supports macOS first."

[[provider.platform_note]]
os = "windows"
status = "planned"
note = "Windows releases exist upstream; per version verification is pending."
```

`revision` naik satu dari revision terbesar di `providers/catalog-revisions.json` saat commit (hari ini 11; nomor akhirnya ditetapkan lead saat build, karena empat provider lain juga menaikkannya), `published_at` adalah waktu UTC commit itu, lalu `cargo xtask catalog check --update`. Sesudah itu `cargo xtask provider freeze providers/traefik`.

### Evidence kompatibilitas

`apps/daemon/tests/traefik/compatibility.rs` mengikuti pola `apps/daemon/tests/nats/compatibility.rs`: daemon lewat `SpawnedDaemon`, check ditambah hanya setelah assertion nya lulus, dan `compatibility.json` tidak ditulis kalau satu check gagal.

```json
{
  "schema": "stackly.compatibility-evidence/v1",
  "providerRevision": { "id": "official/traefik", "revision": "1.0.0", "manifestSha256": "<hex>" },
  "checks": ["health", "http", "init", "invalid_config", "readiness", "rollback", "routing_update", "second_instance", "start", "stop"],
  "knownLimitations": ["dashboard_not_offered", "https_not_offered"]
}
```

| Check | Lulus kalau |
| --- | --- |
| `init` | create instance pertama berakhir `succeeded`, lifecycle `stopped`, `initialized: true` |
| `start` | start berakhir `succeeded`, lifecycle `running` |
| `readiness` | urutan event readiness tidak pernah `ready` sebelum router yang diminta dimuat, lalu `ready` |
| `health` | `instance.get` menunjukkan health `healthy` (nilai default spec 0010, karena paket tanpa blok `health`) dan `GET /ping` di port `ping` menjawab 200 |
| `http` | request dengan header `Host` yang cocok ke port `web` menjawab 200 dengan isi backend uji |
| `second_instance` | instance kedua `ready` dengan PID dan port berbeda, dan instance pertama tetap `ready` |
| `routing_update` | urutan AC-2 lulus |
| `invalid_config` | urutan AC-3 (ditolak schema), satu nilai AC-15 (`backendUrl` `http://127.0.0.1:9/%zz` ditolak schema tanpa revision baru), dan bagian gagal AC-4 (ditolak Traefik) lulus |
| `rollback` | bagian roll back AC-4 lulus |
| `stop` | kedua instance berhenti dengan `succeeded`, tanpa force stop, dan tidak ada proses atau listener tersisa |

Subject evidence nya `{ provider: traefik, version, digest, source: managed }`: `version` dari field `Version` jawaban `GET /api/version` di port `ping` proses yang berjalan (helper `harness::server_version(ping_port)`, langkah 7b), `digest` dari `harness::artifact_sha256()`. Aturan yang sama berlaku untuk setiap test native di `apps/daemon/tests/traefik/`: `Site::subject` dipanggil sesudah instance pertama `ready`, dengan versi dari helper itu, seperti test NATS yang membacanya dari `/varz`. Literal `"3.7.15"` tidak dipakai sebagai versi subject. Helper `Checks` disalin dari `apps/daemon/tests/nats/harness.rs` ke `apps/daemon/tests/traefik/harness.rs`, supaya file milik fitur 39 tidak disentuh.

Report nya disalin `cargo xtask runtime compat-record <report.json>` ke `evidence/compatibility/<runId>/`, lalu `cargo xtask codegen` menulis record scope `provider` ke `providers/traefik/versions.json` (spec 0018 AC-15). Jawaban untuk Follow-up spec 0036: test BR-025 Traefik adalah `traefik_compatibility`, bukan test konformitas, karena matrix PRD 32.2 meminta routing update dan invalid config, yang tidak ada di langkah harness.

### Test native

Semua di `apps/daemon/tests/traefik/`, `#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]`, dijalankan dengan `cargo test -p stacklyd --test traefik -- --ignored`. Setiap test memakai `Site::open` yang ada (satu `TestRoot`, `allocate_port`, `FakeSecretStore`), backend uji dari `harness::backend`, dan request lewat `harness::get(port, host)` yang mengirim header `Host` ke `127.0.0.1`, jadi tidak ada nama yang perlu di resolve. `harness::get` hanya meminta `/`, jadi `harness.rs` mendapat `get_path(port, host, path)`. `harness::backend` melayani satu koneksi pada satu waktu; itu cukup untuk polling AC-8, yang mengirim request berurutan.

| Test | File | AC |
| --- | --- | --- |
| `two_instances_with_the_same_route_name_and_rule_answer_from_their_own_backend` | `routing.rs` (baru) | AC-2 |
| `invalid_route_values_are_refused_and_nothing_is_written` | `routing.rs` | AC-3 |
| `a_rule_traefik_rejects_fails_the_apply_keeps_last_known_good_and_rolls_back` | `routing.rs` | AC-4 |
| `a_backend_url_with_a_path_a_query_a_fragment_or_a_bad_escape_is_refused_and_nothing_is_written` (langkah 7b; `covers` juga `AC-015`) | `routing.rs` | AC-15 |
| `a_backend_only_change_to_every_accepted_url_form_is_loaded_and_becomes_last_known_good` (langkah 7b) | `routing.rs` | AC-15 |
| `ping_and_the_api_answer_only_on_the_monitoring_port_and_the_dashboard_is_off` (instance tanpa route, lalu dengan satu route ``Host(`app.localhost`)`` dan request ber `Host` lain) | `surface.rs` (baru) | AC-5, AC-6, AC-7 (file render tanpa konfigurasi TLS) |
| `ten_applies_in_a_row_never_drop_the_route_that_stays` | `watch.rs` (baru) | AC-8 |
| `a_router_in_a_file_the_user_added_makes_the_instance_not_ready_until_it_is_moved` | `watch.rs` | AC-8, AC-13 |
| `cli_applies_routes_with_a_reload_and_the_log_level_with_a_restart` | `cli.rs` (baru, lewat `SpawnedDaemon` dan `CliRunner`) | AC-9 |
| `traefik_compatibility` | `compatibility.rs` (baru) | AC-11, AC-7 (`/etc/hosts`) |
| `traefik_3_7_passes_conformance` (sudah ada) | `apps/daemon/tests/provider_conformance.rs` | AC-11 |
| `traefik_survives_planned_restart_and_sigkill_and_returns_after_kickstart_and_bootout` | `apps/daemon/tests/native_launchd.rs` | AC-12 |

Test `config.rs` dan `readiness.rs` yang ada tidak dihapus. Yang `BLOCKED`: setiap test di atas kalau artefak tidak ada di cache runtime xtask (`reason` `artifact traefik-3.7.15-macos-aarch64 is not in the runtime cache`, `owner` `@ojiepermana`, yaitu runtime maintainer bawaan spec 0003 dan konstanta `harness::OWNER`), dan test launchd kalau tidak ada sesi GUI (`owner` user). Sesi build tidak pernah mengunduh.

Test AC-15 yang kedua membuat satu instance dengan enam route (`r1` sampai `r6`, rule ``Host(`r<n>.localhost`)``) ke satu backend uji, menunggu `ready`, lalu mengirim satu apply yang hanya mengganti `backendUrl` keenamnya ke enam bentuk AC-15 (port backend uji kedua untuk empat bentuk pertama). Sesudah operasi `succeeded`, test membaca `GET /api/http/services?per_page=1000` di port `ping` sampai keenam entri `<nama>@file` punya `loadBalancer.servers[0].url` yang baru (paling lama `reloadSeconds`), lalu menegaskan `status` `enabled` di keenam service dan keenam router, readiness `ready`, PID yang sama, nomor last known good sama dengan nomor revision baru, dan `r1.localhost` menjawab dari backend uji kedua. Helper baca JSON untuk port `ping` (`harness::api_json(ping_port, path)`) dipakai test ini dan `harness::server_version`.

### Data model

Tidak ada tabel, kolom, atau migrasi baru. Route hidup sebagai nilai settings `routes` di `config_revisions` (spec 0029) dan sebagai file hasil render.

### API surface

Tidak ada method, DTO, kode error, atau event baru. Yang dipakai:

| Method atau perintah | Dipakai untuk | Spec asal |
| --- | --- | --- |
| `instance.config.get`, `validate`, `apply`, `rollback` | baca dan ubah route | 0029 |
| `instance.get_detail` | `connections` dan aksi `openWith` | 0021, 0070 |
| `doctor.run` | remedy `routes_not_loaded` | 0034 |
| `daemon.restart_preview` | `keep` atau `stop_restore` | 0030 |
| `stackly instance config apply <target> --set-json routes=<json>` | route dari CLI | 0029 AC-17 |

### Value sourcing

| Aksi | Nilai | Sumber |
| --- | --- | --- |
| render `routes.yaml` | `name`, `rule`, `backendUrl` tiap route | settings `routes` revision aktif |
| render `traefik.yaml` | alamat `web` dan `ping` | port efektif endpoint instance (registry) |
| render `traefik.yaml` | folder dynamic | field managed `dynamicDir` = `${instance.config}/dynamic` |
| probe `traefik-routes` | router harapan | `dynamic/routes.yaml` di disk |
| probe `traefik-routes` | router yang dimuat | `GET /api/http/routers?per_page=1000` di endpoint `ping` |
| `connection` `web`, `ping` | URL | `${endpoint.web.address}`, `${endpoint.ping.address}` |
| `openWith` `routers` | URL | `${endpoint.ping.address}` |
| remedy langkah 3 | path | `${instance.config}/dynamic` |
| `compatibility.json` | `manifestSha256` | `ValidatedPackage` paket yang dipakai test |
| subject evidence test native | `version` | field `Version` jawaban `GET /api/version` di port `ping` proses yang berjalan (`harness::server_version`) |
| subject evidence test native | `digest` | `sha256` artefak `traefik-3.7.15-macos-aarch64` di `providers/traefik/versions.json` (`harness::artifact_sha256`) |
| validasi `backendUrl` | bentuk yang diterima | `pattern` field itu di `schemas/settings.schema.json` (AC-15) |
| test AC-15 kedua | URL service yang dimuat | `loadBalancer.servers[0].url` tiap entri `<nama>@file` di `GET /api/http/services?per_page=1000`, port `ping` |
| `HandoverProof` | `evidence` | ID tetap `spec-0041-ac-12`, ditulis setelah record launchd `PASS` |
| entri catalog | `mark`, `tile` | nilai entri `[[planned]]` yang ada (`Tr`, `cyan`) |
| entri catalog | `revision` | revision terbesar ledger + 1 saat commit, ditetapkan lead |

### Edge cases dan kegagalan

- **Port `web` atau `ping` dipakai proses lain**: aturan port spec 0020 dan spec 0033 berlaku apa adanya (start gagal dengan `suggestedPort`, tidak ada port yang diganti sendiri).
- **Daemon mati di tengah apply**: aturan spec 0029 (*Daemon mati di tengah apply*). File `routes.yaml` yang sudah ditulis tetap dimuat Traefik; revision baru tidak mendapat `ready_at` dari proses adopsi.
- **`routes` kosong**: template menulis `http: {}`, probe mengharapkan nol router `@file`, dan setiap request ke `web` menjawab 404. Ini keadaan default instance baru.
- **Backend mati**: Traefik menjawab 502 untuk route itu. Readiness tidak berubah, karena Stackly tidak memiliki backend.
- **Rule valid yang tidak pernah cocok** (misalnya host salah ketik): Traefik menerimanya, jadi apply sukses. Stackly tidak menebak maksud rule.
- **Dua apply bersamaan**: `BusyPolicy::Queue` dan `baseRevision` spec 0029 AC-9.
- **`backendUrl` memakai `localhost`**: tetap diterima; risiko dial ke `[::1]` milik user OS lain adalah residual yang sudah diterima (audit keamanan 2026-11 nomor 44, spec 0019).
- **`backendUrl` dengan path, query, atau fragment**: ditolak schema (AC-15). Tidak ada kemampuan yang hilang, karena Traefik 3.7.15 tidak memakai bagian itu dari URL server. Kalau kamu butuh awalan path di backend, itu pekerjaan middleware Traefik, yang tidak ditawarkan revision ini.
- **Port `0` atau di atas 65535 di `backendUrl`**: lolos pola (satu sampai lima digit) dan dimuat Traefik dengan router `enabled`; request ke route itu menjawab 502, sama seperti backend mati. Pola tidak dipersempit ke 1 sampai 65535, karena nilai ini tidak pernah mematikan router dan tidak menyentuh last known good.
- **Apply yang hanya mengganti backend selesai sebelum Traefik memuatnya**: operasi `succeeded` dan `ready_at` bisa datang sampai sekitar 2 s lebih dulu dari jawaban backend baru (*Yang tidak dilihat verify*). Selama jendela itu route masih menjawab dari backend lama. Stackly tidak menunggu lebih lama dan tidak memeriksa service.
- **File pengguna di `dynamic/` yang tidak bisa diparse Traefik**: Traefik menolak seluruh folder dan menahan config lama (terbukti pada 3.7.15, probe 2026-11-12). Readiness tetap `ready`. Apply berikutnya yang menambah, menghapus, atau mengganti nama route, atau mengubah rule, gagal verify dengan `routes_mismatch`. Apply yang hanya mengganti backend lulus verify, karena nama dan rule yang ditahan Traefik sama dengan file baru: operasi `succeeded`, revision itu menjadi last known good, dan Traefik tetap melayani backend lama sampai file itu kamu pindahkan. Ini residual yang diterima untuk folder yang tidak didukung sebagai tempat file pengguna; remedy langkah 3 dan README mencakupnya. Stackly tidak memindai folder itu.
- **File pengguna dengan router bernama sama dengan route Stackly**: Traefik membuang salah satunya, jadi probe melaporkan `routes_mismatch`. Penanganannya sama.
- **Traefik menulis ulang atau menghapus `routes.yaml`**: tidak terjadi; Traefik hanya membaca folder itu. Perubahan dari luar adalah drift spec 0029.

### Jawaban follow-up dari spec lain

| Spec | Follow-up | Jawaban |
| --- | --- | --- |
| 0011 | "Fitur 41: primitive hash bcrypt untuk htpasswd Traefik di `secret::delivery`" | Tidak dibangun. Revision ini tidak punya dashboard ber password atau middleware basic auth, jadi tidak ada pemakai. `EXPOSE_ALLOWED` tidak berubah. Baris itu tetap terbuka dan berpindah ke fitur yang pertama memakai basic auth Traefik (lihat Follow-up) |
| 0023 | "Fitur 41: init paket Traefik merender `staticConfig`..." | Sudah dijawab di `main` oleh langkah 22 spec 0009: `lifecycle.initialize` Traefik hanya `createManagedDirectories` dan `renderConfig routes`. Fitur ini tidak mengubah init |
| 0009 | paket Traefik dengan `routes` berbentuk daftar | Template dipakai apa adanya. Schema berubah di satu baris: pola `backendUrl` yang dikutip spec 0009 (*Paket contoh NATS dan Traefik*) diganti pola AC-15 (lihat Follow-up) |
| 0030 | test native launchd sebelum `HANDOVER_PROVEN` | AC-12 |
| 0036 | "Fitur 39 sampai 43" dan *Dari contoh ke paket resmi* | Paket sudah `official/traefik` dengan `versions.json` dari inventory, `PROVIDER_WORDS`, fixture form, dan test konformitas. Yang ditambah: `tests/scenarios.yaml` (AC-1), catalog lalu freeze (AC-10). Test BR-025 adalah `traefik_compatibility` |
| 0017 | *Aturan masuk catalog* | AC-10 |
| 0070 | dashboard Traefik dan entri `browser` nya | Dashboard tidak ditawarkan. Entri `browser` yang ditambah adalah `routers`, ke API read only |
| 0034 | remedy provider lain | AC-13 |
| 0021 | blok `connection` untuk provider berikutnya | AC-1 |

### Key invariants

- Apply ke satu instance tidak pernah menulis file, mengirim signal, atau mengubah revision instance lain.
- Readiness `ready` selalu berarti nama, rule, dan status `enabled` setiap router `@file` yang dimuat Traefik sama persis dengan `dynamic/routes.yaml` tulisan Stackly. URL backend tidak dibandingkan probe.
- Last known good hanya berubah saat instance mencapai `ready` di atas revision itu. Karena `ready` tidak melihat URL backend, invariant ini dijaga untuk backend oleh schema: setiap `backendUrl` yang lolos pola AC-15 dimuat Traefik dengan router `enabled`.
- Dashboard tidak pernah menyala, dan tidak ada konfigurasi TLS (dalam arti AC-7) di file hasil render revision `1.0.0`.
- Setiap listener Traefik ada di `127.0.0.1` dan tercatat di registry.
- Paket tidak punya primitive yang menulis di luar folder managed instance.

### Security model

- **Siapa bisa apa**: hanya user pemilik daemon yang bisa mengubah route, lewat socket privat (spec 0006). Tidak ada peran baru.
- **API read only di port `ping`**: user OS lain di Mac yang sama, dan halaman web di browser kamu yang berhasil menjangkau `127.0.0.1:<port ping>`, bisa membaca nama router, rule, dan URL backend. Tidak ada secret di sana, dan API Traefik tidak menerima tulisan. Ini residual yang sudah dinyatakan spec 0009 (*Probe `traefik-routes`*), sekelas dengan monitoring NATS, dan penyimpangan yang disengaja dari kalimat PRD 22.5 tentang API insecure (lihat *Trace PRD*).
- **Tanpa secret**: paket tidak punya field secret, jadi tidak ada item Keychain, tidak ada redaksi khusus, dan `copySecret` tidak dipakai.
- **Rule dan backend adalah teks pengguna**: escaper `yaml` spec 0009 (AC-22) mencegah nilai `rule`, `name`, atau `backendUrl` menambah key YAML. `backendUrl` dibatasi schema ke `http://`, host loopback, dan port saja (AC-15), jadi instance tidak bisa dijadikan proxy ke host lain, dan tidak ada teks bebas di URL itu yang sampai ke parser URL Traefik.
- **Tanpa perubahan sistem**: tidak ada penulisan root CA, DNS, `/etc/hosts`, atau port di bawah 1024 yang dijanjikan.

### Configuration required

Tidak ada env var, flag, atau credential baru.

### Critical test scenarios

- Happy path: dua instance, nama dan rule sama, backend berbeda, ubah A, B tetap. Memverifikasi **AC-2**.
- Nilai tidak valid: nama ganda, `backendUrl` ke `http://example.com`, 65 route; tidak ada revision dan file tidak berubah. Memverifikasi **AC-3**.
- Rule ditolak Traefik: apply gagal `supervisor.reload_failed`, `routes_mismatch`, last known good tetap, roll back memulihkan. Memverifikasi **AC-4**.
- Permukaan: `/ping`, `/api/http/routers`, dan `/dashboard/` di kedua port; `lsof` hanya dua listener loopback. Memverifikasi **AC-5**, **AC-6**.
- Tanpa TLS dan tanpa perubahan sistem: file render tanpa konfigurasi TLS, juga saat satu route bernama `tls`; `/etc/hosts` sama. Memverifikasi **AC-7**.
- URL backend: path, query, fragment, dan `%zz` ditolak tanpa revision baru; satu apply yang hanya mengganti backend ke enam bentuk yang diterima dimuat `enabled` dan menjadi last known good. Memverifikasi **AC-15**.
- File watching: sepuluh apply berturut turut dan file asing di `dynamic/`. Memverifikasi **AC-8**, **AC-13**.
- CLI: `config apply --set-json routes=...` (reload) dan `--set logLevel=DEBUG` (restart). Memverifikasi **AC-9**.
- Paket: `provider check`, golden, dan dua skenario lewat harness. Memverifikasi **AC-1**, **AC-11**.
- Catalog dan freeze: `catalog check` lolos sesudah perpindahan; `provider freeze` menolak sebelum perpindahan. Memverifikasi **AC-10**.
- Handover: restart terencana dan `SIGKILL` daemon di bawah launchd, PID Traefik sama, route menjawab. Memverifikasi **AC-12**.
- RISK-07: `git diff --stat main...HEAD` pada folder sumber hanya menunjukkan `handover.rs`. Memverifikasi **AC-14**.

### Langkah manual

Dipisah dari langkah build; sesi build menulisnya di `verify.md` bagian `### UI / manual` dan tidak menjalankannya. GUI mengikuti Interaction policy di `AGENTS.md`: hanya kontrol pointer dan teks biasa, tanpa shortcut, VoiceOver, atau identifier test.

1. Di app: buka Catalog, pilih Traefik (sekarang tersedia), pasang runtime 3.7.15 (unduhan, hanya kamu yang menjalankan), buat instance `edge-a` dan `edge-b`, lalu Start keduanya.
2. Di `edge-a`, tab Configuration: tambah satu route di daftar Routes (Name `api`, Rule ``Host(`app.localhost`)``, Backend URL ke server lokal kamu). Coba dulu Backend URL dengan path, misalnya `http://127.0.0.1:3000/api`: periksa error di field itu dan bahwa tidak ada revision baru, lalu hapus path nya (AC-15). Periksa tag "Applies on reload" dan tombol Apply and Reload, lalu Apply. Jalankan `curl -H 'Host: app.localhost' http://127.0.0.1:<port web edge-a>/` dan periksa jawabannya; ulangi ke port `edge-b` dan periksa 404.
3. Ubah Log level: periksa tag "Requires restart" dan tombol Apply and Restart.
4. Ganti Rule menjadi `Host(` lalu Apply: periksa kartu gagal, status not ready, dan banner "Revision N hasn’t reached ready". Klik Roll Back to Revision M dan periksa route menjawab lagi.
5. Menu Open In: Routes File membuka Finder di `routes.yaml`; Loaded Routers membuka JSON router di browser.
6. Doctor saat langkah 4 sebelum roll back: periksa remedy "Traefik hasn’t loaded the routes Stackly wrote".
7. Test launchd AC-12 butuh sesi GUI yang login: `cargo test -p stacklyd --test native_launchd -- --ignored traefik_survives`. Sesi build menjalankannya hanya dengan izin lead; tanpa itu `BLOCKED` dengan `owner` user.

## Build plan

Urutan mengikuti pendekatan Journey: jalur "dua proxy Traefik dari catalog sampai route berubah dengan aman" dibuat lengkap dan terbukti di Mac ini sebelum provider masuk catalog. Milestone 1 mengunci isi paket. Milestone 2 membuktikan journey pada Traefik 3.7.15 asli. Milestone 2b (ditambah 2026-11-12) menjawab temuan verify milestone 1 dan 2 dan mengubah `manifest_sha256` untuk terakhir kalinya, sebelum evidence apa pun direkam. Milestone 3 merekam evidence, membuka catalog, dan membuktikan handover. Milestone 4 menutup journey dari CLI dan GUI. Nomor langkah lama tidak digeser: langkah baru bernomor 7a dan 7b.

Nomor protokol dan migrasi: fitur ini tidak memakai nomor baru. Kalau build ternyata butuh perubahan kontrak atau schema registry, itu tanda spec ini salah: berhenti dan kembali ke `/architect`; nomornya "nomor bebas berikutnya, ditetapkan lead saat build".

**Milestone 1: Paket resmi** (needs fitur 9, 21, 34, 36, dan 70, semuanya done)

1. `providers/traefik/provider.yaml`: blok `connection`, entri `openWith` `routers`, dan blok `remedies` seperti *Perubahan paket*; `providers/traefik/tests/scenarios.yaml`; `providers/traefik/README.md`. Lalu `cargo xtask provider check providers/traefik`. Memenuhi **AC-1**, **AC-13** (bagian manifest).
2. Turunan paket, di commit yang sama dengan langkah 1: golden di `crates/core/tests/golden/provider/traefik/` lewat `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test provider_packages` (golden baru `connection.json` dan `remedies.json`, `open_with.json` yang ditulis ulang, dengan assertion nya di `crates/core/tests/provider_packages.rs`, `crates/core/tests/open_with_render.rs`, dan `crates/core/tests/connection_render.rs`, yang mendapat baris `assert_golden("traefik", ..)`), test `every_repository_package_is_publishable_with_the_implied_default_scenario` di `crates/core/tests/provider_check.rs` (Traefik keluar dari daftar skenario tersirat dan mendapat assertion dua skenarionya), fixture `protocols/fixtures/instance/create-form-traefik.json` lewat test `create_form_fixtures`, `cargo xtask fuzz seed` dan seed `fuzz/seeds/provider_manifest/traefik.provider.yaml`, `cargo xtask codegen --check`, dan `cargo xtask docs check`. Memenuhi **AC-1**.
3. Test fixture tanpa runtime asli di `crates/core/tests/provider_packages.rs`: `traefik.yaml` hasil render dengan settings default memuat `dashboard: false` dan tidak memuat `tls`; `routes.yaml` hasil render untuk nol, satu, dan tiga route tidak memuat `tls`; `permissions` paket persis dua entri AC-7; remedy `routes_not_loaded` cocok untuk readiness `routes_mismatch` dan tidak cocok untuk reason lain. Memenuhi **AC-6**, **AC-7** (bagian statis), **AC-13**.

**Milestone 2: Routing terbukti pada Traefik 3.7.15 asli** (after milestone 1; artefak di cache runtime xtask, selain itu `BLOCKED`)

4. `apps/daemon/tests/traefik/routing.rs` dengan tiga test AC-2, AC-3, AC-4, dan deklarasi `mod` di `apps/daemon/tests/traefik.rs`. Helper baru yang dipakai lebih dari satu file masuk `apps/daemon/tests/traefik/harness.rs`. Memenuhi **AC-2**, **AC-3**, **AC-4**.
5. `apps/daemon/tests/traefik/surface.rs`: `/ping`, `/api/http/routers`, dan `/dashboard/` di kedua port, listener lewat `lsof` seperti kontrak proses harness, dan isi file render di disk. Memenuhi **AC-5**, **AC-6**, **AC-7**.
6. `apps/daemon/tests/traefik/watch.rs`: sepuluh apply berturut turut dengan polling paralel, dan file asing di `dynamic/` dengan `doctor.run`. Memenuhi **AC-8**, **AC-13**.
7. Run `cargo test -p stacklyd --test provider_conformance -- --ignored traefik_3_7_passes_conformance` dengan dua skenario paket; perbaiki paket (bukan harness) kalau satu langkah gagal. Memenuhi **AC-11** (bagian konformitas).

**Milestone 2b: Temuan verify milestone 1 dan 2** (after milestone 2; artefak di cache runtime xtask, selain itu `BLOCKED`; ditambah 2026-11-12)

- **7a.** Paket dan turunannya, satu commit. `providers/traefik/schemas/settings.schema.json`: baris `pattern` `backendUrl` seperti *Perubahan paket* butir 7, tanpa perubahan lain. `providers/traefik/README.md`: tiga paragraf *Tambahan README*. Lalu `cargo xtask provider check providers/traefik` (sha baru). Turunan di commit yang sama: di `crates/core/tests/provider_packages.rs`, route ketiga `traefik_settings` memakai `http://[::1]:8080` (bukan `http://[::1]:8080/docs`); test baru `traefik_backend_url_is_a_scheme_a_loopback_host_and_a_port_only` memvalidasi setiap nilai tabel *Perubahan paket* butir 7 terhadap settings schema paket (yang diterima lolos, yang ditolak memberi issue `pattern` di `settings.routes[0].backendUrl`), ditambah `http://127.0.0.2:9`, `http://example.com:80`, `https://127.0.0.1:9`, `HTTP://127.0.0.1:9`, dan `http://user@127.0.0.1:9` sebagai nilai yang ditolak; `traefik_renders_the_dashboard_off_and_never_a_tls_key` mendapat satu render dengan route bernama `tls`, dan helper `holds_tls_key` nya mengikuti kalimat AC-7 (static: key `tls` di kedalaman mana pun dan `certificatesResolvers`; routes: key `tls` tingkat atas dan di dalam isi tiap router dan service, bukan nama di `http.routers` dan `http.services`). Golden `crates/core/tests/golden/provider/traefik/render-routes.txt` ditulis ulang lewat `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test provider_packages`; golden lain tidak boleh berubah. Fixture `protocols/fixtures/instance/create-form-traefik.json` (`manifestSha256` baru) lewat test `create_form_fixtures`, `cargo xtask fuzz seed` (`fuzz/seeds/provider_settings_schema/traefik.settings.schema.json` dan seed lain yang ikut berubah), `cargo xtask codegen --check`, `cargo xtask docs check`, dan `swift test --package-path clients/swift`. `apps/macos/` tidak disentuh: `description` field tidak berubah, jadi `FixtureConfiguration.swift` tetap benar. `/develop` juga mengganti nilai sha di `verify.md` bagian Milestone 1 dengan sha baru. Memenuhi **AC-1**, **AC-15** (bagian schema), **AC-7** (bagian statis).
- **7b.** Test native dan perbaikan harness, di `apps/daemon/tests/traefik/`. `harness.rs`: `api_json(ping_port, path)` dan `server_version(ping_port)`; komentar `ARTIFACT` menyebut `providers/traefik/versions.json` (bukan `providers/nats`), dan komentar `Site::subject` menyebut `GET /api/version` (bukan `GET /varz`). Kedelapan pemanggilan `site.subject(&ev, "3.7.15")` di `config.rs`, `readiness.rs`, `routing.rs`, `surface.rs`, dan `watch.rs` pindah ke sesudah instance pertama `ready` dan memakai `harness::server_version`. `routing.rs`: dua test AC-15 di *Test native*. `surface.rs`: `holds_tls_key` mengikuti kalimat AC-7 pada YAML yang sudah diparse, sama dengan helper di langkah 7a. Karena byte paket berubah, jalankan ulang seluruh test native di atas paket baru: `cargo test -p stacklyd --test traefik -- --ignored --test-threads=1` dan `cargo test -p stacklyd --test provider_conformance -- --ignored traefik_3_7_passes_conformance`. `/develop` menulis bagian `## Milestone 2b` di `verify.md`. Memenuhi **AC-15**, **AC-2** (kalimat batas jawaban), **AC-7**, **AC-11** (versi subject).

**Milestone 3: Evidence, catalog, dan handover** (after milestone 2b; menyentuh file bersama, lihat *Sesi paralel*)

8. `apps/daemon/tests/traefik/compatibility.rs` (`traefik_compatibility`, lewat `SpawnedDaemon`, dengan pemeriksaan `/etc/hosts`). Dari `HEAD` yang bersih: run native lengkap `cargo test -p stacklyd --test traefik -- --ignored`, `cargo xtask runtime compat-record <report.json>`, lalu `cargo xtask codegen` untuk `providers/traefik/versions.json`, yang masuk commit yang sama dengan langkah 9 (seperti commit NATS `27e98df3`). Memenuhi **AC-11**, **AC-7**.
9. Di commit evidence itu: `providers/catalog.toml` seperti *Entri catalog*, `cargo xtask catalog check --update` (`providers/catalog-revisions.json`), lalu `cargo xtask provider freeze providers/traefik` (`crates/core/tests/fixtures/compat/`). Test `a_planned_provider_is_refused_before_anything_is_written` di `xtask/tests/provider_check.rs` memakai Traefik sebagai contoh paket yang belum masuk `[[provider]]`; ia dipindah ke paket yang tidak akan pernah masuk catalog (`providers/example`, ID `testing/example`), supaya tidak bergantung pada provider mana yang masih planned. Memenuhi **AC-10**.
10. `apps/daemon/tests/native_launchd.rs`: test Traefik mengikuti `nats_survives_...` dengan `#[path = "traefik/harness.rs"]`, satu route ke backend uji, dan assertion bahwa route menjawab sesudah setiap akhir daemon. Di langkah yang sama, sebelum run: satu baris `HandoverProof` di `crates/core/src/supervisor/handover.rs`, dan di modul test file itu baris `("official/traefik", "1.0.0", false)` menjadi `true`. Lalu run test. Record `PASS`: commit. Record `FAIL`, atau `BLOCKED` karena tidak ada sesi GUI atau izin: buang lagi kedua perubahan `handover.rs`, commit test nya saja, dan AC-12 tetap terbuka. Memenuhi **AC-12**, **AC-14**.

**Milestone 4: Journey dari CLI dan GUI** (after milestone 3)

11. `apps/daemon/tests/traefik/cli.rs`: `stackly instance config apply` untuk `routes` dan `logLevel`, `stackly instance connection --all`, dan `stackly doctor --instance` pada instance dengan rule yang ditolak. Memenuhi **AC-9** (bagian CLI), **AC-13**.
12. `verify.md` bagian `### UI / manual` dengan tujuh langkah *Langkah manual*; pemeriksaan `git diff --stat main...HEAD` untuk AC-14 dicatat di sana. Memenuhi **AC-9** (bagian GUI), **AC-14**.

**Sesi paralel.** Fitur ini aman berjalan bersama fitur 39, 40, 42, dan 43 selama milestone 1, 2, dan 2b untuk file miliknya sendiri (langkah 7a menyentuh lagi `crates/core/tests/provider_packages.rs`, di fungsi dan test Traefik saja): `providers/traefik/`, golden dan seed Traefik, `create-form-traefik.json`, dan `apps/daemon/tests/traefik/`. Empat file test bersama disentuh di milestone 1 dan juga disentuh provider lain: `crates/core/tests/provider_packages.rs`, `crates/core/tests/open_with_render.rs`, `crates/core/tests/connection_render.rs`, dan `crates/core/tests/provider_check.rs`. Bagian Traefik ditambahkan sebagai fungsi atau baris sendiri, dan konflik merge diselesaikan lead. File bersama yang hanya boleh diubah satu sesi pada satu waktu, semuanya di milestone 3: `providers/catalog.toml`, `providers/catalog-revisions.json`, `crates/core/src/supervisor/handover.rs` (`HANDOVER_PROVEN` dan modul test nya), `apps/daemon/tests/native_launchd.rs`, `xtask/tests/provider_check.rs`, dan `crates/core/tests/fixtures/compat/` (lewat `provider freeze`). Golden CLI dan fixture `protocols/fixtures/catalog/` memakai catalog fixture, bukan `providers/catalog.toml`, jadi tidak ikut berubah. Fitur ini tidak menyentuh `runtimes/inventory.toml`, `xtask/src/check_deps/source_scan.rs`, `protocols/fixtures/instance/index.json`, `apps/daemon/tests/provider_conformance.rs`, `apps/daemon/tests/nats/`, `crates/protocol/`, atau modul format config, probe, dan hash di `crates/core`. Langkah 8 dan 9 butuh `HEAD` bersih dan nomor revision catalog dari lead, jadi lead mengurutkan milestone 3 antarprovider.

## Consequences

**Positive**:

- Traefik masuk catalog dengan nol perubahan perilaku core, protokol, storage, dan SwiftUI. Ukuran RISK-07 untuk provider ini: nol perubahan perilaku, satu baris data (`HANDOVER_PROVEN`).
- Route yang salah tidak pernah dilaporkan sukses, dan kembali ke config yang terbukti jalan adalah satu aksi.
- Baris Traefik di matrix PRD 32.2 punya evidence native yang bisa ditelusuri dari `versions.json`.
- Tidak ada secret, jadi instance Traefik tidak pernah terblokir Keychain.

**Negative / tradeoffs**:

- Tidak ada HTTPS. Developer yang butuh `https://` lokal belum terlayani provider ini.
- Tidak ada dashboard. Melihat router berarti membaca JSON di browser atau tab Logs.
- Sesudah rule ditolak Traefik, route itu tidak menjawab sampai kamu bertindak; route lain di revision yang sama sudah berlaku. Stackly melaporkannya, tidak memulihkannya sendiri.
- API read only bisa dibaca proses lokal lain lewat port `ping` (nama router, rule, URL backend).
- Rule tetap teks bebas. Kesalahan sintaks baru ketahuan setelah Traefik membacanya, paling lama `reloadSeconds`.
- URL backend tidak menerima path, query, atau fragment. Nilai seperti `http://127.0.0.1:3000/api`, yang dulu diterima dan diam diam tidak berpengaruh, sekarang ditolak dengan error field.
- Verify tidak melihat backend. Apply yang hanya mengganti backend bisa selesai sampai sekitar 2 s sebelum Traefik melayaninya, dan dengan file pengguna yang rusak di `dynamic/` ia bisa dilaporkan sukses sementara backend lama tetap dilayani.
- `manifest_sha256` paket berubah dua kali (langkah 1 dan 7a), jadi golden, fixture form, dan seed fuzz Traefik ditulis ulang dua kali, test native milestone 2 dijalankan ulang di atas paket baru, dan test root lama yang mem pin sha lama tidak dipakai lagi.

**Neutral**:

- Baris Follow-up bcrypt di spec 0011 tetap terbuka dan berganti pemilik.
- Sebelum AC-12 `PASS`, restart daemon terencana menghentikan lalu memulihkan instance Traefik (`stop_restore`), dan itu tetap benar.
- File tambahan di `config/dynamic/` tetap dimuat Traefik tetapi tidak didukung dan di luar pengawasan Stackly.

## Follow-up

- [ ] Lead: tandai baris Follow-up fitur 41 di spec 0023, 0030 (bagian Traefik), 0036, 0017, 0070, 0034 (bagian Traefik), dan 0021 sebagai dijawab spec 0041, seperti *Jawaban follow-up dari spec lain*.
- [ ] Lead dan pemilik spec 0011: ubah baris "Fitur 41: primitive hash bcrypt untuk htpasswd Traefik" menjadi milik fitur yang pertama memakai basic auth Traefik (dashboard ber password atau middleware route). Spec 0041 tidak membangunnya.
- [ ] HTTPS dengan certificate milik pengguna: fitur baru lewat `/scope`, lalu `/architect`. Titik mulai yang disarankan: path certificate dan key sebagai settings ditambah `tls.options.default.sniStrict: true`, supaya Traefik menolak handshake dan tidak menyajikan certificate bawaannya. Yang masih harus diputuskan dan dibuktikan native: perilaku itu pada 3.7, cara menyimpan key, dan apakah readiness perlu memeriksa certificate yang disajikan.
- [ ] Dashboard Traefik sebagai pilihan: butuh syarat di entri `openWith` (perubahan additive schema manifest, spec 0036 *Kebijakan kompatibilitas*) supaya tombolnya hanya muncul saat dashboard menyala.
- [ ] Probe `traefik-routes` yang juga membandingkan service (URL backend di `GET /api/http/services` dengan `dynamic/routes.yaml`): menutup dua residual *Yang tidak dilihat verify* (operasi mendahului muatan Traefik, dan apply yang hanya mengganti backend saat Traefik menahan config lama). Ini mengubah `crates/core/src/provider/probe/traefik.rs`, `crates/core/src/supervisor/probe/traefik.rs`, dan spec 0009 AC-22, jadi lewat `/architect` pada spec 0009, bukan bagian fitur ini (AC-14).
- [ ] Lead dan pemilik spec 0009: beri catatan update di *Paket contoh NATS dan Traefik* bahwa pola `backendUrl` yang dikutip di sana diganti pola spec 0041 AC-15 (`http://(127\.0\.0\.1|\[::1\]|localhost):[0-9]{1,5}/?`).
- [ ] Awalan path ke backend (middleware `addPrefix` atau `preservePath` Traefik): fitur baru lewat `/scope` kalau ada yang membutuhkannya. Melonggarkan pola `backendUrl` nanti adalah perubahan additive.
- [ ] Pra validasi rule tanpa menyentuh instance yang jalan (misalnya proses Traefik sementara): dipertimbangkan lagi kalau laporan pengguna menunjukkan jendela "route mati sampai roll back" terlalu mahal.
- [ ] `/sync` setelah build: catat di root `AGENTS.md` bahwa folder `config/dynamic/` Traefik hanya memuat file tulisan Stackly yang diawasi, dan perintah `cargo test -p stacklyd --test traefik -- --ignored`.
- [ ] `/scope`: kalimat deskripsi fitur 41 "HTTPS hanya aktif dengan certificate yang diberikan pengguna" dibaca spec ini sebagai batas (HTTPS tidak pernah aktif tanpa certificate pengguna), bukan sebagai kemampuan yang dibangun sekarang.

## Rationale

Alasan, opsi lain, dan catatan cross check: lihat [rationale.md](rationale.md).
