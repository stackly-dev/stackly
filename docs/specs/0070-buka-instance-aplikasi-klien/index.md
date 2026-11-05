# 0070. Buka instance di aplikasi klien

**Date**: 2026-11-02
**Status**: Accepted

## Summary

Spec ini menambah menu "Open In" supaya kamu bisa membuka instance langsung di aplikasi lain: klien database (TablePlus, Postico), `psql` milik runtime instance di Terminal, halaman monitoring di browser, dan file routes di Finder. Daftar aksinya dideklarasikan provider di blok manifest baru `openWith` (bukan kode SwiftUI per provider), dirender daemon tanpa password, lalu dijalankan GUI lewat NSWorkspace (API macOS untuk membuka aplikasi, URL, dan file). Password tidak pernah masuk URL, argumen, atau daftar proses: aplikasi dibuka tanpa password, dan Stackly menyalin password ke clipboard tersembunyi supaya kamu tinggal paste saat diminta. CLI hanya mendapat `stackly instance open <instance> --list`, yang menampilkan aksi tanpa menjalankan apa pun.

## Requirements

**User stories**:

- Sebagai developer, saya ingin membuka instance PostgreSQL di TablePlus atau Postico dengan satu klik, supaya saya tidak mengetik host, port, dan user sendiri.
- Sebagai developer, saya ingin membuka `psql` yang versinya sama persis dengan instance, supaya saya tidak tidak sengaja memakai `psql` lain dari PATH.
- Sebagai developer, saya ingin password tidak pernah terlihat di URL, argumen, atau `ps`, supaya password tidak bocor ke riwayat aplikasi lain atau daftar proses.
- Sebagai penulis provider, saya ingin aksi Open In dideklarasikan di manifest, supaya provider baru tidak butuh kode Rust atau Swift.

**Acceptance criteria**:

- **AC-1** (blok `openWith` di manifest): manifest `stackly/v1alpha1` menerima blok opsional `openWith`, daftar 1 sampai 8 entri seperti *Blok openWith*. Loader menolak dengan path field dan kode issue: `name` di luar pola `^[a-z][a-z0-9_]{0,31}$`, `title` kosong, lebih dari 48 karakter, atau berkarakter kontrol, `kind` di luar `app`, `terminal`, `browser`, `reveal`, atau tabel kind yang tidak cocok dengan `kind` (`open_with_invalid` dengan `reason`); `name` ganda (`open_with_name_duplicate`); token `secret.*`, `settings.*`, atau `endpoint.<n>.host` di template mana pun (`token_not_allowed_here`); `copySecret` yang bukan field secret di schema (`unknown_secret`); endpoint yang tidak ada (`unknown_endpoint`); `app.bundleIds` kosong, lebih dari 4, atau bukan pola bundle ID (`reason` `bundle_id_invalid`); `app.url` yang hasil render sentinel nya bukan URI absolut RFC 3986 dan `browser.url` yang bukan URI absolut berskema `http` atau `https` (`reason` `not_a_uri`, `not_http`); `terminal.executable` yang bukan tepat satu token `${runtime.executable}` atau `${runtime.executables.<alias>}` (`reason` `executable_not_runtime`); lebih dari 32 `terminal.args`, satu arg atau nilai env di atas 1024 byte, atau lebih dari 16 pasang env (`reason` `too_long`); nama env di luar aturan env spec 0009 AC-7, termasuk `PATH`, `HOME`, dan `TMPDIR` (`reason` `env_name_invalid`); entri `terminal` di paket yang `permissions.managedFilesystem` nya tidak memuat `temp` (`path_not_permitted`); `copySecret` di entri `browser` atau `reveal` (`reason` `copy_secret_not_allowed`); `reveal.configFile` yang bukan kunci `configFiles` paket itu (`reason` `unknown_config_file`) atau yang menunjuk file `sensitive: true` (`reason` `config_file_sensitive`). Render sentinel memakai nilai sentinel spec 0021 AC-1 ditambah root runtime `/stackly sentinel/runtime` (jadi `runtime.executable` dan `runtime.executables.<alias>` menjadi path absolut di bawahnya) dan `runtime.version` `0.0.0`. Posisi interpolasi baru di `interpolate.rs` adalah `ValuePosition::OpenUrl`, `OpenArg`, `OpenEnv`, dan `OpenExecutable`, dan cek folder managed yang sudah ada berlaku untuk keempatnya. Blok ini ikut `manifest_sha256`, dan `cargo xtask codegen` menulis ulang schema editor di `schemas/provider/`.
- **AC-2** (render aksi): `instance::open_with::render(package, ctx)` memakai `PlanContext` dan resolver interpolasi spec 0009 yang sama dengan `connection`, dan mengembalikan satu `RenderedOpenAction` per entri dalam urutan manifest. Status dipilih dengan urutan spec 0021 AC-2 (`retained`, `package_unavailable`, `port_not_assigned`), selain itu tersedia; `retained` berlaku untuk semua kind termasuk `reveal`, dan `port_not_assigned` hanya muncul untuk entri yang memakai token endpoint. `OpenAction.reason` hanya membawa ketiga reason daemon itu; `not_running` dan `not_ready` dihitung GUI dari `InstanceSummary` dan dikirim `prepare_terminal` sebagai details. Entri tersedia membawa nilai yang sudah dirender: `url` untuk `app` dan `browser`, `command` (`executable` path absolut, `args`, `env`) untuk `terminal`, dan `path` absolut untuk `reveal`. Nilai token di dalam URL di percent encode dengan aturan spec 0021 AC-2; di `command` nilai disisipkan apa adanya. Render tidak pernah membaca Keychain atau filesystem selain paket pinned di cache.
- **AC-3** (`InstanceDetail.openActions`): `instance.get_detail` (spec 0021 AC-4) menambah field `openActions: [OpenAction]` seperti *Method IPC*, dari AC-2, kosong saat `provider.packageReadable` false atau paket tidak punya blok `openWith`. `OpenAction` tidak pernah memuat nilai secret; `copySecret` hanya membawa field dan reference.
- **AC-4** (`instance.prepare_terminal`): method baru `instance.prepare_terminal { instanceId, action }` (`recovery refused`, bukan operation spec 0007) merender ulang entri itu dari paket pinned (tidak memakai data dari client), memeriksa entri ada dan ber `kind` `terminal` (`instance.open_action_not_found`, details `action` dan `available`), memeriksa instance `running` dengan readiness `ready` (sumber yang sama dengan `instance.get_detail`: lifecycle registry dan readiness observasi supervisor; tidak diserialisasi dengan stop, jadi skrip yang basi hanya gagal konek) dan entri tersedia (`instance.open_action_unavailable`, details `action` dan `reason`: `not_running`, `not_ready`, `retained`, `package_unavailable`, `port_not_assigned`), lalu menulis skrip `open-<action>.command` di folder `temp` instance dengan mode 0700 secara atomik (tulis `open-<action>.command.<uuid v7>.tmp` di folder yang sama, chmod 0700, lalu rename; kegagalan menghapus file `.tmp`) dan membalas `{ scriptPath }`. Skrip tidak dihapus saat stop. Skrip sesuai *Skrip Terminal*. Pemanggilan berulang menimpa file yang sama. Tulis yang gagal menghasilkan `instance.open_script_unwritable` dan tidak meninggalkan file setengah jadi. ID yang tidak ada menghasilkan `resource.not_found`.
- **AC-5** (menu Open In di GUI): satu komponen `OpenInMenu` tampil di tiga tempat: tombol menu "Open In" (SF Symbol `arrow.up.forward.app`) di toolbar detail instance di samping aksi utama, tombol menu yang sama di inspector Instances di samping Open, dan submenu "Open In" di menu Instance di menu bar (target sama dengan `copyConnectionString`, spec 0069 AC-11, tanpa shortcut). Isinya aksi provider dalam urutan manifest dengan label dari *Label aksi*. Entri `app` yang tidak satu pun `bundleIds` nya terpasang tidak tampil. Kalau tidak ada aksi yang tampil, tombol dan submenu tidak tampil. Ketiganya membaca `openActions` dari `InstanceDetail` yang sama dengan `copyConnectionString`: toolbar dari `InstanceDetailModel`, inspector dan submenu dari detail instance terpilih yang sudah dimuat inspector (spec 0021 AC-15). Selama detail dimuat, menu berisi satu item nonaktif "Loading…"; tanpa instance terpilih, submenu nonaktif.
- **AC-6** (gate): aksi `app`, `browser`, dan `terminal` aktif hanya saat lifecycle `running` dan readiness `ready`; selain itu nonaktif dengan help "Start the instance to open it." (tidak running) atau "Not accepting connections yet." (running tanpa ready). Aksi `reveal` aktif apa pun lifecycle nya, dan nonaktif dengan help "The file doesn’t exist yet." kalau path nya tidak ada. Entri yang tidak tersedia memakai teks alasan spec 0021 (*Copy*), dan alasan daemon itu menang atas teks lifecycle kalau keduanya berlaku. `reveal` di instance `retained` nonaktif dengan teks `retained` spec 0021. `OpenActionKind::Unrecognized` tampil nonaktif dengan "Update Stackly to use this action.". Saat daemon tidak terjangkau semua aksi nonaktif dengan aturan gate shell spec 0013.
- **AC-7** (buka aplikasi): memilih aksi `app` (1) kalau entri punya `copySecret`, memanggil `secret.reveal { reference, purpose: copy }` sekali dan menulis hasilnya lewat `ConcealedPasteboard` (spec 0011), (2) membuka `url` lewat `NSWorkspace.open([url], withApplicationAt:)` dengan aplikasi terpasang pertama dari urutan `bundleIds`, lalu (3) menampilkan `FeedbackCard` success "Opening “{instance}” in {title}." dengan kalimat kedua "Password copied. Paste it when {title} asks." hanya kalau password tersalin. URL yang dibuka tidak pernah memuat password.
- **AC-8** (buka Terminal): memilih aksi `terminal` memanggil `instance.prepare_terminal` lebih dulu, lalu menjalankan langkah salin password AC-7 kalau ada `copySecret`, lalu membuka `scriptPath` dengan Terminal.app (`com.apple.Terminal`) lewat `NSWorkspace.open([fileURL], withApplicationAt:)`, dan menampilkan "Opening {title} for “{instance}” in Terminal." dengan kalimat password yang sama. Error dari `prepare_terminal` tampil sebagai `FeedbackCard` error, password tidak disalin, dan Terminal tidak dibuka; `instance.open_action_unavailable` memakai teks AC-6 untuk `reason` nya, `instance.open_action_not_found` memakai "This action is no longer available.", error lain memakai pesan envelope nya, dan GUI memuat ulang detail sesudahnya. Untuk `app` dan `browser` tidak ada cek ulang daemon saat klik; membuka endpoint yang baru saja mati diterima dan berakhir di error aplikasi tujuan.
- **AC-9** (browser dan Finder): aksi `browser` membuka `url` dengan browser default (`NSWorkspace.open(url)`) dan menampilkan "Opening {title} for “{instance}” in your browser.". Aksi `reveal` memanggil `NSWorkspace.activateFileViewerSelecting([path])` tanpa umpan balik kartu; keberadaan file dicek dengan `FileManager.fileExists(atPath:)` pada path hasil render setiap menu dibangun.
- **AC-10** (password tidak tersedia): kalau reference `copySecret` berstatus `missing` atau `secret.reveal` gagal (termasuk Keychain terkunci, spec 0011), aplikasi atau Terminal tetap dibuka, dan kartu caution "Opened without copying the password. The password isn’t available." menawarkan "Enter Secret…" yang membuka `SecretEntrySheet` untuk reference itu. Kegagalan reveal karena sebab lain menampilkan "Opened without copying the password." dengan pesan error nya, tanpa Enter Secret.
- **AC-11** (gagal membuka): kalau NSWorkspace gagal (aplikasi dihapus setelah menu dibangun, Terminal tidak bisa dibuka, URL ditolak), kartu error "Couldn’t open {title}." tampil dengan deskripsi error, dan password yang sudah tersalin tetap mengikuti aturan kedaluwarsa `ConcealedPasteboard`.
- **AC-12** (CLI): `stackly instance open <instance> --list [--json]` menerima nama atau UUID lewat `TargetArg` (spec 0022), memanggil `instance.get_detail`, dan mencetak satu baris per aksi dengan format *CLI*; `--list` wajib (clap, exit 2 tanpa itu). CLI tidak pernah membuka aplikasi, menulis skrip, atau memanggil `secret.reveal`. Provider tanpa blok `openWith` mencetak "No open actions." dan exit 0. `--json` memberi `result` `{ "instance": { "id", "name" }, "openActions": [OpenAction] }`.
- **AC-13** (paket provider): `providers/postgresql` mendapat entri `tableplus` (app), `postico` (app), dan `psql` (terminal), `providers/nats` mendapat `monitoring` (browser), `providers/traefik` mendapat `routes` (reveal), dan `providers/fake` mendapat satu entri per kind, seperti *Entri per paket*. `cargo xtask provider check` dan golden render keempat paket lolos, termasuk golden skrip Terminal.
- **AC-14** (keamanan, AC-016): suite sentinel memakai fixture credential sebagai password instance fake dan memastikan prefix nya tidak ada di balasan `instance.get_detail` dan `instance.prepare_terminal`, isi skrip `.command`, argv proses yang dibuka Terminal, output default dan `--json` `stackly instance open --list`, log daemon, dan state model `@Observable` (`OpenInModel`, `InstanceDetailModel`); nilai hanya muncul di respons `secret.reveal` dan di `ConcealedPasteboard`. Setiap salin tercatat audit `secret.reveal` spec 0011. Test daemon juga memastikan skrip bermode 0700, milik pengguna, dan tanpa atribut `com.apple.quarantine`.
- **AC-15** (bukti native): di Mac ini, untuk instance PostgreSQL 18 yang `ready`: "Open psql in Terminal" membuka Terminal, `psql` dari runtime instance meminta password, password hasil paste menjalankan `SELECT 1`, dan `ps -ww` selama sesi tidak memuat password; "Open in TablePlus" dan "Open in Postico" membuka aplikasi itu ke host dan port instance dengan user `postgres` (masing masing `BLOCKED` kalau aplikasinya tidak terpasang, dengan alasan dan owner); TablePlus dan Postico meminta password (bukan gagal diam diam) karena URL tidak memuat password; skrip `.command` berjalan tanpa dialog Gatekeeper; skrip psql yang gagal konek menampilkan status dan menunggu Return; path socket instance di root uji tidak melewati 104 byte; untuk instance NATS "Open Monitoring in Browser" membuka halaman awal monitoring. Tanpa artefak runtime hasilnya `BLOCKED`.

## Decision

**Chosen option**: Option 1: blok manifest `openWith` yang dirender daemon dan dijalankan GUI.

Provider mendeklarasikan aksi di blok `openWith` dengan empat kind tertutup, daemon merender nya tanpa secret ke `InstanceDetail.openActions` dan menulis skrip Terminal lewat `instance.prepare_terminal`, GUI menjalankannya lewat NSWorkspace dan menyalin password ke `ConcealedPasteboard`, dan CLI hanya mendaftar.

**Implementation skills**: `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `swiftui-expert-skill` dan `swift-concurrency` (lihat [apps/macos/AGENTS.md](../../../apps/macos/AGENTS.md))

Kalau isi skill bertentangan dengan spec ini, spec yang menang.

## Rationale

Alasan dan opsi yang ditimbang ada di [rationale.md](rationale.md).

## Feature design

### Blok openWith

```yaml
openWith:
  - name: tableplus
    title: TablePlus
    kind: app
    app:
      bundleIds: ["com.tinyapp.TablePlus"]
      url: "postgresql://postgres@${endpoint.client.address}/postgres"
    copySecret: superuserPassword
  - name: psql
    title: psql
    kind: terminal
    terminal:
      executable: "${runtime.executables.psql}"
      args: ["--host=${endpoint.socket.socketDir}", "--port=${endpoint.client.port}", "--username=postgres", "postgres"]
      env: { PGAPPNAME: stackly-terminal }
    copySecret: superuserPassword
```

- Field umum: `name` (wajib), `title` (wajib), `kind` (wajib), `copySecret` (opsional, nama field secret; hanya untuk `app` dan `terminal`, ditolak di `browser` dan `reveal` dengan `reason` `copy_secret_not_allowed`).
- Tepat satu tabel yang namanya sama dengan `kind`:
  - `app`: `bundleIds` (1 sampai 4, urutan prioritas), `url` (template, 1 sampai 512 byte).
  - `browser`: `url` (template, 1 sampai 512 byte, skema `http` atau `https`).
  - `terminal`: `executable` (satu token runtime), `args` (0 sampai 32), `env` (0 sampai 16 pasang, nama env pola `^[A-Z_][A-Z0-9_]{0,63}$`).
  - `reveal`: `configFile` (kunci `configFiles`; path nya adalah `destination` yang sudah dirender).
- Posisi interpolasi baru `openWith.url` dan `openWith.command` menerima token yang sama dengan `connection.template` (spec 0021) kecuali `secret.<field>`: `endpoint.<n>.address`, `endpoint.<n>.port`, `endpoint.<n>.socket`, `endpoint.<n>.socketDir`, `instance.name`, `instance.id`, `provider.id`, `provider.revision`, dan `runtime.version`. `openWith.command` juga menerima `runtime.executable` dan `runtime.executables.<alias>` di `executable` saja.

### Entri per paket

| Paket | `name` | `kind` | Isi |
|---|---|---|---|
| `providers/postgresql` | `tableplus` | app | `bundleIds: ["com.tinyapp.TablePlus"]`, `url` seperti contoh, `copySecret: superuserPassword` |
| `providers/postgresql` | `postico` | app | `title: Postico`, `bundleIds: ["at.eggerapps.Postico2", "at.eggerapps.Postico"]`, `url` sama, `copySecret: superuserPassword` |
| `providers/postgresql` | `psql` | terminal | seperti contoh |
| `providers/nats` | `monitoring` | browser | `title: Monitoring`, `url: "http://${endpoint.monitoring.address}/"` |
| `providers/traefik` | `routes` | reveal | `title: Routes File`, `configFile: routes` |
| `providers/fake` | satu per kind | semua | `app` dengan `bundleIds: ["dev.stackly.fake-client"]` (sengaja tidak pernah terpasang), `browser` ke endpoint http fake, `terminal` dengan `${runtime.executable}` dan `args: ["version"]` (subcommand `fake-server`), `reveal` ke config file baru `fakeNotes` (template `templates/notes.txt.tmpl`, format `plain`, destination `${instance.config}/notes.txt`, tidak `sensitive`, dirender di `initialize`; paket fake belum punya file non sensitive), `copySecret: authToken` di `app` dan `terminal` |

Bundle ID TablePlus dan Postico adalah asumsi yang dibuktikan di AC-15. Kalau ternyata berbeda, perbaikannya hanya di `provider.yaml` dan golden, tanpa kode.

### Label aksi

| `kind` | Label menu | Contoh |
|---|---|---|
| app | "Open in {title}" | Open in TablePlus |
| terminal | "Open {title} in Terminal" | Open psql in Terminal |
| browser | "Open {title} in Browser" | Open Monitoring in Browser |
| reveal | "Reveal {title} in Finder" | Reveal Routes File in Finder |

Urutan di menu: urutan manifest. Ada satu pemisah sebelum entri `reveal` pertama kalau ada entri lain di atasnya. Label dibuat oleh satu fungsi murni `OpenActionLabel.text(for:)` di Swift dan `open_action_label` di CLI, dengan test terhadap fixture yang sama.

### Skrip Terminal

```sh
#!/bin/sh
# Written by Stackly for instance <instance.id>. Holds no secret.
/usr/bin/env -i HOME="$HOME" USER="$USER" LOGNAME="$LOGNAME" TERM="$TERM" LANG="$LANG" \
  PGAPPNAME='stackly-terminal' \
  '/abs/path/runtime/bin/psql' '--host=/abs/run dir' '--port=54321' '--username=postgres' 'postgres'
status=$?
if [ "$status" -ne 0 ]; then
  printf '\n%s exited with status %s. Press Return to close.\n' 'psql' "$status"
  read -r _
fi
exit "$status"
```

- Setiap nilai di quote tunggal POSIX (`'` menjadi `'\''`) lewat satu fungsi murni `shell_quote` di `stackly-protocol`, dipakai daemon untuk skrip dan CLI untuk `--list`. Nama env sudah divalidasi loader, jadi tidak di quote.
- `env -i` membuang env pengguna (misalnya `PGSERVICE`, `PGHOSTADDR`, `PGPASSWORD` dari shell), lalu hanya meneruskan `HOME`, `USER`, `LOGNAME`, `TERM`, `LANG`, dan env dari manifest. Header hanya memuat ID, bukan nama instance.
- Perintah tidak memakai `exec`, supaya jendela tidak langsung tertutup kalau psql gagal konek; status keluar 0 tetap mengikuti pengaturan "Shell exit" profil Terminal.
- Tidak ada `cd`; Terminal membuka di folder home pengguna.
- Path skrip: `<folder temp instance>/open-<action>.command`, mode 0700, pemilik pengguna, tanpa atribut quarantine. Folder temp ikut terhapus bersama instance (fitur 54).
- Skrip menjalankan binary absolut dari runtime instance, tidak pernah mencari di PATH.

### Method IPC

| Method | Recovery | Params | Result | Key errors |
|---|---|---|---|---|
| `instance.get_detail` (diperluas) | refused | `{ instanceId }` | `InstanceDetail` + `openActions: [OpenAction]` | `resource.not_found` |
| `instance.prepare_terminal` (baru) | refused | `{ instanceId: Uuid, action: String }` (`action` 1 sampai 32 byte, pola nama AC-1, lewat `Validate`) | `{ scriptPath: String }` | `resource.not_found`, `instance.open_action_not_found` (NotFound), `instance.open_action_unavailable` (Conflict), `instance.open_script_unwritable` (Internal, details `reason`) kalau skrip gagal ditulis |

```rust
pub struct OpenAction {
    pub name: String,
    pub title: String,
    pub kind: OpenActionKind,                       // App, Terminal, Browser, Reveal, Unrecognized
    pub state: ConnectionAvailability,              // spec 0021: Available, Unavailable, Unrecognized
    #[serde(default)] pub reason: Option<ConnectionUnavailableReason>,  // set only when Unavailable
    #[serde(default)] pub app_bundle_ids: Vec<String>,
    #[serde(default)] pub url: Option<String>,
    #[serde(default)] pub command: Option<OpenCommand>,
    #[serde(default)] pub path: Option<String>,
    #[serde(default)] pub copy_secret: Option<ConnectionSecretPart>,  // field + reference, spec 0021
}
pub struct OpenCommand { pub executable: String, pub args: Vec<String>, #[serde(default)] pub env: Vec<EnvPair> }
pub struct EnvPair { pub name: String, pub value: String }
```

- Enum terbuka dengan `Unrecognized` dan `tag = "type"`, `content = "content"` (aturan wire spec 0006). `env` berupa daftar pasangan, bukan map.
- `state` dan `reason` memakai ulang `ConnectionAvailability` dan `ConnectionUnavailableReason` spec 0021 (bentuk yang sama dengan `ConnectionString`), jadi teks alasannya sama.
- Kode baru di `codes.rs` dengan format yang ada: `instance.open_action_not_found` (NotFound, details `["action", "available"]`, retryable false), `instance.open_action_unavailable` (Conflict, details `["action", "reason"]`, retryable true), `instance.open_script_unwritable` (Internal, details `["reason"]`, retryable true). `PrepareTerminalParams` implement `Validate` (pola `action`). `OpenActionKind` masuk `OpenEnums.swift`, `InstanceDetail.openActions` memakai `#[serde(default)]`, dan semua tipe baru derive `JsonSchema` dan `#[typeshare]`.
- Kontrak berubah: `ProtocolVersion::CURRENT` naik minor dari 1.18 ke 1.19 di commit yang sama (di `main` kontrak ini masuk 2.1 lewat konsolidasi branch 2026-11-02, merge `36b33e6e`; `main` kini 3.1), bersama `protocols/contract-versions.json`, `cargo xtask codegen` menulis Swift, JSON Schema, dan digest, dan fixture baru `protocols/fixtures/instance/detail-open-actions.json` serta `prepare-terminal.json`.

### CLI

```text
$ stackly instance open pg18 --list
tableplus  Open in TablePlus         postgresql://postgres@127.0.0.1:54321/postgres   copies password
postico    Open in Postico           postgresql://postgres@127.0.0.1:54321/postgres   copies password
psql       Open psql in Terminal     /…/bin/psql --host=… --port=54321 --username=postgres postgres   copies password
```

Kolom: `name`, label, nilai (`url` untuk `app` dan `browser`, perintah lewat `shell_quote` untuk `terminal` tanpa env, `path` untuk `reveal`), lalu catatan (`copies password` kalau ada `copySecret`). Kolom dipisah dua spasi dan dirata kiri dengan lebar kolom terpanjang. Baris tidak tersedia mengganti nilai dengan "—" dan catatan dengan teks alasan spec 0021; kind `Unrecognized` mencetak "Update Stackly to use this action.". CLI tidak memeriksa apakah aplikasi terpasang; itu urusan GUI. `--list` wajib sekarang; menambah `open <instance> <action>` nanti tidak memecah perintah ini.

### Data model sketch

Tidak ada tabel registry baru dan tidak ada migrasi. Data baru:

| Tempat | Isi | Sifat |
|---|---|---|
| `provider.yaml` | blok `openWith` (raw type di `stackly-core::provider::raw`, model tervalidasi di `provider::manifest`) | read only, ikut `manifest_sha256` |
| `InstanceDetail.openActions` | `OpenAction` yang dirender | dihitung per request, tidak disimpan |
| `<temp instance>/open-<action>.command` | skrip tanpa secret | file sementara, ditimpa, dihapus bersama instance |

### Value sourcing

| Aksi | Nilai | Sumber |
|---|---|---|
| render | `url`, `command`, `path` | template `openWith` dari paket pinned, `PlanContext` (port efektif dari registry, folder dari `RootSet::resolve`), path runtime dari runtime terpasang |
| render | `state` | lifecycle `retained` dan kunci paket dari registry, kegagalan render (spec 0021 AC-2) |
| render | `copySecret.reference` | `secret_uses_by(Instance(id))` (spec 0005), field dari manifest |
| menu | entri `app` tampil | `NSWorkspace.urlForApplication(withBundleIdentifier:)` lewat port `AppLocator`, dicek setiap menu dibangun |
| menu | aktif atau tidak | `InstanceSummary` lifecycle dan readiness (spec 0021, 0022), `DaemonModel` untuk daemon tidak terjangkau, `FileManager.fileExists` untuk `reveal` |
| menu | label | `kind` + `title` lewat `OpenActionLabel` |
| salin password | nilai | `secret.reveal { purpose: copy }` (spec 0011), tidak pernah disimpan di model |
| salin password | status `missing` | `InstanceDetail.secrets[].status` untuk reference itu |
| buka app | aplikasi tujuan | bundle ID terpasang pertama dari `appBundleIds` |
| buka Terminal | path skrip | balasan `instance.prepare_terminal` |
| buka Terminal | isi skrip | render ulang daemon dari paket pinned, bukan dari client |
| kartu umpan balik | `{instance}` | `InstanceSummary.name` |
| CLI | baris | `instance.get_detail.openActions`, label dari `open_action_label` |

### Key invariants

- Tidak ada token `secret.*` di template `openWith`; loader menolaknya, jadi password secara struktural tidak bisa masuk URL, argv, env, atau skrip.
- `terminal.executable` selalu path absolut dari runtime instance itu.
- Hanya daemon yang menulis skrip, hanya di folder `temp` instance, dan selalu dari render ulangnya sendiri.
- Nilai secret hanya hidup di respons `secret.reveal` dan `ConcealedPasteboard`, tidak di model `@Observable` yang hidup lama.
- GUI tidak pernah menjalankan binary provider sendiri; Terminal yang menjalankan skrip sebagai proses pengguna. Daemon juga tidak spawn apa pun untuk fitur ini, jadi aturan `ProcessSupervision` tetap utuh.
- CLI tidak membuka apa pun.

### Security model

- Satu pengguna per daemon, socket privat (spec 0006). `prepare_terminal` hanya menerima nama aksi, bukan perintah, jadi client tidak bisa menyuruh daemon menulis perintah arbitrer.
- Skrip mode 0700 di folder instance milik pengguna; isinya bukan rahasia, tapi mode ketat mencegah pengguna lain menjalankannya.
- `psql` lewat Unix socket 0700 instance. Password diketik atau di paste ke prompt `psql`, jadi tidak ada di argv atau env.
- Salin password memakai `ConcealedPasteboard` (penanda concealed dan kedaluwarsa spec 0011) dan audit `secret.reveal`.
- Browser hanya ke URL loopback dari endpoint instance; loader membatasi skema ke `http` atau `https`.

### Configuration required

Tidak ada env var atau credential baru. Fitur ini tidak meminta izin Automation, karena tidak memakai Apple Events.

### Critical test scenarios

- Happy path: instance fake `ready`, aksi `terminal` dan `browser` dipilih lewat `OpenInModel` dengan fake `WorkspaceOpener`; skrip ditulis, dibuka dengan bundle `com.apple.Terminal`, URL browser benar, kartu umpan balik benar, verifies **AC-4**, **AC-7**, **AC-8**, **AC-9**
- Gate: instance stopped, starting tanpa ready, daemon offline, dan aksi `Unrecognized` menghasilkan item nonaktif dengan help yang tepat; `reveal` tetap aktif saat stopped, verifies **AC-6**
- Aplikasi tidak terpasang: `AppLocator` palsu tanpa bundle `dev.stackly.fake-client` menyembunyikan entri `app`; dengan semua aksi tersembunyi tombol tidak tampil, verifies **AC-5**
- Password missing: reference `missing` tetap membuka aplikasi dan menampilkan kartu caution dengan Enter Secret, verifies **AC-10**
- Gagal membuka: `WorkspaceOpener` palsu yang melempar error menghasilkan kartu "Couldn’t open …", verifies **AC-11**
- Daemon: `prepare_terminal` untuk aksi tak dikenal, aksi `browser`, instance stopped, dan instance starting membalas kode dan `reason` yang tepat; pemanggilan dua kali menimpa skrip; isi skrip sama dengan golden, verifies **AC-4**
- Loader: setiap baris penolakan AC-1 punya kasus test dengan path field dan kode issue, verifies **AC-1**
- Sentinel: prefix fixture credential tidak ada di detail, skrip, output CLI, dan log, verifies **AC-14**
- CLI golden: `--list` dan `--json` untuk detail fixture PostgreSQL, NATS, dan paket tanpa `openWith`, verifies **AC-12**

## Build plan

Approach Journey: satu jalur pengguna (buka instance di aplikasi lain) dibangun lengkap dengan semua state sebelum fitur berikutnya. Urutannya dari dalam ke luar supaya setiap lapisan bisa dites sebelum GUI ada, lalu bukti native menutup jalur itu.

**Milestone 1: manifest, daemon, dan CLI**

1. Raw type dan validasi blok `openWith` di `stackly-core::provider` (`raw.rs`, `manifest.rs`, `validate.rs`), posisi interpolasi `openWith.url` dan `openWith.command` di `interpolate.rs`, render sentinel, issue code baru, `manifest_sha256`, dan schema editor lewat `cargo xtask codegen`, satisfies **AC-1**
2. `instance::open_with::render` di `stackly-core` dengan unit test terhadap fake `stackly-testkit`, satisfies **AC-2**
3. DTO `OpenAction`, `OpenCommand`, `EnvPair`, params dan result `prepare_terminal`, error code baru, method di tabel `methods!` (`recovery refused`), bump 1.19, codegen, dan fixture kontrak, satisfies **AC-3**, **AC-4**
4. `stacklyd`: `openActions` di `instance.get_detail`, handler `instance.prepare_terminal` dengan penulisan skrip atomik dan quoting POSIX, integration test di `apps/daemon/tests/` dengan `TestRoot`, satisfies **AC-3**, **AC-4**
5. Entri `openWith` di paket PostgreSQL, NATS, Traefik, dan fake, golden render dan golden skrip (`STACKLY_UPDATE_GOLDEN=1`), `cargo xtask provider check`, satisfies **AC-13**
6. `stackly instance open <instance> --list [--json]` dengan `TargetArg`, `open_action_label`, dan golden CLI, satisfies **AC-12**

**Milestone 2: GUI**

7. `StacklyUI/OpenIn/`: port `AppLocator` dan `WorkspaceOpener` dengan implementasi NSWorkspace dan fake test, `OpenActionLabel`, dan `OpenInModel` (gate, daftar tampil, alur salin lalu buka, kartu umpan balik, Enter Secret), dengan test Swift Testing terhadap fixture kontrak, satisfies **AC-5**, **AC-6**, **AC-7**, **AC-8**, **AC-9**, **AC-10**, **AC-11**
8. `OpenInMenu` di toolbar detail instance, inspector Instances, dan submenu menu Instance (spec 0069), plus skenario fixture spec 0068 untuk toolbar dan inspector dengan tombol Open In (aktif, nonaktif, tidak tampil), satisfies **AC-5**, **AC-6**
9. Suite sentinel AC-016 diperluas ke detail, `prepare_terminal`, skrip, CLI, log, dan model Swift, satisfies **AC-14**

**Milestone 3: bukti native**

10. Run native di Mac ini: psql di Terminal dengan `SELECT 1` dan cek `ps -ww`, TablePlus dan Postico (atau `BLOCKED`), monitoring NATS di browser, reveal Traefik kalau instance Traefik bisa dibuat, dicatat di `verify.md` spec ini, satisfies **AC-15**

## Consequences

**Positive**:

- Provider baru mendapat Open In hanya lewat YAML; GUI tidak tahu nama TablePlus atau psql.
- Password secara struktural tidak bisa bocor lewat URL, argumen, atau skrip, karena loader menolak token secret.
- `psql` selalu cocok dengan versi instance, jadi tidak ada kejutan `psql` 14 melawan server 18.
- Tanpa izin Automation, jadi tidak ada dialog izin tambahan.

**Negative / tradeoffs**:

- Pengguna harus paste password sekali per buka; TablePlus dan Postico yang menyimpan koneksi sendiri bisa menyimpan password itu di Keychain mereka, di luar kendali Stackly.
- Bundle ID aplikasi pihak ketiga ada di paket provider; kalau vendor menggantinya, paket perlu revisi.
- Hanya Terminal.app. Pengguna iTerm atau Ghostty tidak mendapat terminal pilihan mereka sampai ada keputusan baru.
- Aplikasi yang hanya menerima password lewat URL tidak bisa didukung sama sekali. Kalimat Done when fitur 70 tentang "alur peringatan" perlu diubah.
- Satu round trip tambahan (`prepare_terminal`) untuk aksi Terminal.

**Neutral**:

- `InstanceDetail` bertambah satu field; kontrak direncanakan 1.19 dan masuk `main` sebagai bagian 2.1.
- Folder `temp` instance mulai berisi skrip `open-*.command`.
- Halaman Environments (fitur 44) memakai `OpenInMenu` yang sama nanti; mockup menaruh menu di sana.

## Follow-up

- [ ] `/scope`: ubah Done when fitur 70 dari "alurnya meminta aksi eksplisit dengan peringatan" menjadi "aplikasi yang hanya menerima secret lewat URL, argumen, atau daftar proses tidak bisa dideklarasikan (loader menolak token secret)", dan catat bahwa menu juga ada di detail, inspector, dan menu Instance.
- [ ] `/scope`: catat di fitur 44 bahwa menu aksi per service di halaman Environments memakai `OpenInMenu` spec 0070, ditambah item Copy Connection URL, Show Instance, Show Logs, dan Start atau Stop dari mockup.
- [ ] Fitur 41: dashboard Traefik (endpoint `dashboard` di loopback, `api.dashboard`) dan entri `browser` nya diputuskan di spec provider Traefik. Sejak 2026-11-05 API Traefik read only sudah ada di entry point loopback `traefik` (endpoint `ping`) untuk probe `traefik-routes` spec 0009 AC-22, dengan `api.dashboard: false`; fitur 41 tinggal memutuskan dashboard dan entri `browser` nya.
- [ ] Fitur 40, 42, 43: entri `openWith` untuk Valkey (Redis Insight, `valkey-cli`), MySQL (TablePlus, Sequel Ace, `mysql`), dan ClickHouse (Play UI, `clickhouse-client`) ditulis di spec provider masing masing.
- [ ] `docs/scope/ui-macos.md` bagian selisih: Open In ada di detail, inspector, dan menu Instance sebelum Environments ada; NATS tidak punya "Subscribe in Terminal" karena runtime nats-server tidak membawa CLI `nats`.
- [ ] Kalau kelak pengguna minta terminal selain Terminal.app, buat keputusan baru (handler default `.command` atau daftar bundle terminal).
