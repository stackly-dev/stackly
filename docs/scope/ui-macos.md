# Scope UI · Desain antarmuka macOS

[Kembali ke index](index.md)

Bahasa visual, komponen, shell, perintah, dan cek tampilan untuk aplikasi SwiftUI di macOS 27, diturunkan dari mockup UI. Layar per halaman tetap dibangun oleh journey yang memilikinya (pendekatan Journey tidak berubah). File ini memegang hal lintas layar yang dipakai semua journey, plus peta layar supaya setiap layar di mockup punya pemilik yang jelas. Ini jalur paralel: fiturnya dikerjakan engineer SwiftUI berdampingan dengan fase Foundation, sebelum layar pertama di fase 02.

## Acuan desain

- **Mockup UI** adalah `docs/mockups/Stackly.dc.html` dengan data contoh di `docs/mockups/stackly-data.js`, diekspor dari Claude Design pada 16 Oktober 2026. `docs/mockups/support.js` hanya runtime prototype, bukan bagian desain.
- **Bahasa visual native macOS.** SF Pro, warna semantik sistem, light dan dark, serta material Liquid Glass untuk sidebar, toolbar, sheet, dan popover. Accent dan mode Liquid Glass (tinted atau clear) mengikuti System Settings. Design system Modernist di `docs/mockups/_ds/` ikut terekspor tapi tidak dipakai mockup, jadi bukan acuan.
- **Aturan fidelity.** Mockup mengunci layout, isi, teks, alur, dan semua state. Kalau mockup meniru kontrol macOS dengan HTML (table, picker, toggle, segmented control, sheet, popover, menu), kontrol native SwiftUI yang menang. Kalau mockup bertentangan dengan scope atau spec soal perilaku, scope dan spec yang menang, lalu mockup dicatat untuk diperbarui.
- **Kontrol prototype bukan setting.** Panel Tweaks di mockup (theme, glass, accent, startPage, daemonConnected, emptyWorkspace) hanya alat untuk melihat state. Yang menjadi setting nyata hanya Appearance di Settings (fitur 32).
- **Ukuran jendela.** Mockup memakai jendela tetap 1280 × 800. Aplikasi nyata bisa diubah ukurannya (fitur 13).
- **Copy.** Teks UI di mockup berbahasa Inggris dan menjadi acuan copy.
- **Selisih yang sudah diketahui.** Di titik ini scope yang menang, dan angka di mockup hanya contoh: sampling metrik "every 2 s" (fitur 28 mengusulkan 5 detik), retention log "14 days or 50 MB" (fitur 27 mengusulkan rotasi 5 file × 20 MiB), dan Appearance yang hanya Light atau Dark (fitur 32 menambah pilihan ikut sistem sebagai default). Design system (fitur 12, spec 0012): teks status di light lebih gelap dan teks merah di dark lebih terang daripada mockup supaya kontras tetap WCAG AA; teks status di baris tabel memakai warna teks utama; konten tidak diredupkan saat stale; dialog konfirmasi dan umpan balik setelah aksi memakai presentasi native tanpa glass kustom, dengan Cancel sebagai tombol default untuk aksi destruktif. Shell jendela (fitur 13, spec 0013): badge sidebar bergaya native, bukan kapsul biru dan angka merah; banner offline punya aksi Open Diagnostics; status daemon lain memakai banner informative (starting, connecting lambat, shutting down) atau critical (incompatible, untrusted, recovery, misconfigured); notice saat jendela ditutup menyebut Dock sampai menu bar fitur 32 ada; ukuran jendela minimum 900 × 600. Catalog (fitur 17, spec 0017): status Available memakai tone netral, bukan biru; versi tampil Unverified, bukan Tested, sampai compatibility record fitur 18 ada; kartu planned diredupkan dengan teks sekunder dan tile abu, bukan opacity, supaya kontras tetap WCAG AA; MariaDB dan RabbitMQ tidak tampil karena ada di Deferred. Onboarding (fitur 15 dan 16, spec 0015): langkah Command Line memasang link di `~/.local/bin` tanpa password, bukan `/usr/local/bin` dengan password; langkah Background Agent tidak menawarkan daemon yang hanya berjalan "only while the app is open" tanpa izin. Onboarding (fitur 16, spec 0016) juga sengaja berbeda dari mockup demi teks yang jujur: ikon aplikasi, baris isolasi logis, langkah Catalog Trust tanpa toggle community dan tanpa "Signed manifests", tanpa baris Environments dan contoh `env`, `doctor`, atau daftar instance sampai fitur pemiliknya ada (diganti "No account needed"), toggle Command Line langsung bertindak lewat konfirmasi, dan tanpa tombol Skip yang terlihat (Escape, atau titik terakhir lalu Get Started). Overview dan Instances (fitur 22, spec 0022): tabel Instances tanpa kolom Ownership (ownership di inspector dan CLI, karena isinya selalu "Manual" sampai fitur 44), kartu Running menghitung instance yang siap dengan keterangan "{n} not ready", kolom dan seksi Environment menunggu fitur 44, item disk tanpa pertumbuhan mingguan, dan item drift menunggu fitur 29. Sheet New Instance (fitur 20, spec 0020): nama mengikuti VAL-001 (1 sampai 63 karakter), bukan pola mockup 3 sampai 31; port diterima 1 sampai 65535 dengan saran mulai 1024; sheet dibuka dengan `defaultPort` manifest dan menolak konflik dengan saran, bukan mengisi port bebas berikutnya; Automatic menampilkan "Assigned when the instance starts"; toggle Start at login menunggu fitur 31; sheet tetap terbuka dengan progress selama create dan Cancel membatalkannya; baris preflight `provider` dan `settings` muncul saat gagal, baris secret per field, listener wildcard dan `::1` sebagai peringatan; tombol Copy menyalin perintah lengkap termasuk `--port` dan `--secret`. Detail instance dan tab Summary (fitur 21, spec 0021): readiness tampil sebagai baris sendiri di bawah Process tanpa latensi dan umur pemeriksaan (milik fitur 28); header memakai label status spec 0012 ("Starting…" atau "Not Ready"), bukan "Running"; Restart attempts menghitung jendela restart policy dan hanya tampil bila lebih dari 0; Runtime dan Instance ID menampilkan UUID; secret reference berbentuk `local:instance/<id>/...` (spec 0011); kartu Connection menampilkan semua entri provider dan "Not accepting connections yet." selama belum siap; tab Configuration, Logs, Metrics, Backups, Activity serta tombol Restart, Reload, Duplicate, Remove menunggu fitur pemiliknya; banner gagal hanya Retry sampai fitur 27 dan 29; Autostart menulis "Off" atau "At login".

## Peta layar

Tiap layar di mockup dan fitur yang membangunnya. Fitur pemilik membawa layar itu lengkap dengan semua statenya.

| Layar di mockup | Isi utama | Fitur pemilik |
| --- | --- | --- |
| Shell jendela | sidebar dengan grup Library dan System, badge jumlah, toolbar per halaman, status daemon, banner offline, pemberitahuan saat jendela ditutup | 13 |
| Onboarding | lima langkah: Welcome, Background Agent, Storage, Catalog Trust, Command Line | 16 (state izin dari 15) |
| Overview | kartu Running, Stopped, Failed, Runtimes; Needs Attention; daftar instance dengan aksi; progress environment; operasi berjalan | 22 |
| Instances | filter status, pencarian, tabel yang bisa diurutkan, inspector, state kosong dan hasil filter kosong | 22 |
| Detail instance · Summary | endpoint dengan copy, readiness, connection yang dimasking, secret reference, dependents, detail | 21; tombol restart dan reload 26; banner gagal 33; menu More: Duplicate 24, Remove 54 |
| Detail instance · Configuration | mode Form dan Effective File, revision, dampak reload atau restart, diff, history, banner drift | 29 |
| Detail instance · Logs | sumber stdout dan stderr, filter, follow, load earlier, cursor | 27 |
| Detail instance · Metrics | main process dan process tree, CPU, memori, disk, metrik unavailable, daftar proses, state tidak running | 28 |
| Detail instance · Backups | back up now, daftar dengan status verified, restore as new, overwrite, state unsupported | 52 |
| Detail instance · Activity | jejak aksi tanpa secret | 35 |
| Sheet New Instance | filter kategori, runtime, nama, port fixed atau automatic dengan saran, start after creating, start at login, preflight, perintah CLI setara | 20 (start at login dari 31) |
| Catalog | kategori, pencarian, kartu provider, panel detail dengan matrix versi per OS, capability, lisensi, tombol Get dengan progress | 17; install 18; Import Provider 37 |
| Runtimes | instalasi berjalan, tabel source, digest, dan pemakai, tombol hapus yang terkunci | 18; hapus 55; Register External 56 |
| Environments | status, banner partially ready, tabel service dengan ownership, lock, override lokal, lease, sheet plan | 44; lock dan override 45; up 46; down 47; menu Open in 70 |
| Operations | status, tahap, cancel yang terkunci di langkah commit, retry | 35 |
| Diagnostics | hasil doctor dengan aksi perbaikan, preview isi diagnostic bundle, export | 34 |
| Settings | tab General, Instances, Sources, Storage, Updates | 32; Restart Daemon 30; sumber catalog 37; cache 55; channel 61 |
| Menu bar | status daemon, jumlah running dan failed, toggle service, progress operasi, Stop All Services dengan preview dampak | 32 (dampak lease dari 47) |
| Dialog dan umpan balik | konfirmasi aksi destruktif, pesan singkat setelah aksi | 67 |

## Foundation

### 12. Design system & UI foundation · in-progress

Bahasa visual native macOS dari mockup UI dan pola state yang dipakai semua layar: tipografi SF Pro, warna semantik light dan dark, palet status yang tidak hanya bergantung pada warna, material Liquid Glass, serta pola state loading, empty, error, stale, unsupported, dan partial.

**Done when:** `design.md` mencakup tipografi, warna, spacing, material, dan pola state, dipetakan dari token mockup UI (termasuk warna status running, starting, stopped, dan failed beserta warna teksnya di light dan dark); accent dan Liquid Glass (tinted atau clear) mengikuti System Settings, bukan pilihan di dalam aplikasi; Reduce Transparency dan Increase Contrast menghasilkan permukaan yang tetap terbaca; status selalu punya teks atau ikon selain warna; konfirmasi aksi destruktif punya satu pola yang dipakai semua layar.

**Trace:** REQ-024 (pola state), REQ-027, NFR-012, PRD bagian 11.2

**Pointer:** spec [0012](../specs/0012-design-system-ui-foundation/index.md)

- [x] Design it (spec): `/architect design system & UI foundation`
- [x] Build it: `/develop design system & UI foundation`
  - [x] Tokens, shared helpers, contrast proof, and the `design.md` skeleton: `ColorToken`, `Tone`, `toneSurface`, `.cardSurface()`, typography and symbol helpers, `Spacing`, `Radius`, `ScaledSize`, `StacklyMotion`, the source rule test, and the Debug "Design Gallery" window (spec 0012 AC-1 to AC-3, AC-6, AC-7, AC-15 to AC-18)
  - [x] Status palette: UI status enums, `StatusAppearance`, `StatusLabel`, `ToneBadge`, and `SectionIcon` (spec 0012 AC-1, AC-4, AC-5, AC-7, AC-16 to AC-19; can run in parallel after milestone 1)
  - [x] State patterns: `ViewState.resolve`, `StateView`, placeholders, banners, `DelayedProgress`, `MutationAvailability`, and `.disabled(reason:)` (spec 0012 AC-1, AC-8 to AC-14, AC-16 to AC-18, AC-20; can run in parallel after milestone 1)
  - [x] Confirmation: the keyboard check on macOS 27, then `.stacklyConfirmation(item:)` and the confirmation rules (spec 0012 AC-1, AC-15, AC-17, AC-18; can run in parallel after milestone 1)
- [ ] Verify it: `/check verify design system & UI foundation`
- [x] Test it: `/test design system & UI foundation`

### 13. App shell SwiftUI & Swift client · in-progress

Shell aplikasi macOS dengan navigasi PRD bagian 11.1 dalam bentuk shell mockup UI, status koneksi daemon, operation center, reconnect, dan Swift client yang dibangkitkan dari schema atau fixture yang sama dengan Rust. Semua mutasi lewat API daemon.

**Done when:** GUI tetap terbuka saat daemon mati dan menampilkan status koneksi, retry, dan diagnostics tanpa menyebut instance stopped; reconnect memakai snapshot plus cursor; test kontrak Swift lulus untuk enum, optional field, timestamp, dan error code; shell mengikuti mockup UI: sidebar dengan Overview, Instances, dan Environments, grup Library (Catalog, Runtimes) dan System (Operations, Diagnostics) beserta badge jumlah, footer status daemon dengan tombol Settings, serta toolbar dengan judul, subjudul, dan aksi tiap halaman; saat daemon tidak terjangkau, banner menyebut waktu state terakhir yang diketahui dan semua aksi mutasi nonaktif; menutup jendela tidak menghentikan daemon atau instance; jendela bisa diubah ukurannya dengan ukuran minimum yang tercatat di spec.

**Trace:** REQ-024, AC-024, RISK-09, PRD bagian 11.1 dan 18.3

**Pointer:** spec [0013](../specs/0013-app-shell-swift-client/index.md) · code in `clients/swift/`, `apps/macos/Packages/StacklyUI/`, `apps/macos/Stackly/`

- [x] Design it (spec): `/architect app shell SwiftUI & Swift client`
- [ ] Build it: `/develop app shell SwiftUI & Swift client`
  - [x] Swift transport and contract: socket path and endpoint fixtures shared with Rust, framing, three stage decode, handshake, calls with per call timeout, subscribe, client error catalog, and tests against a fake server and the real `stacklyd` (spec 0013 AC-1 to AC-6, AC-20, AC-23; needs feature 6 milestone 1, its integration tests need feature 6 milestone 4; can run in parallel with milestone 3)
  - [x] Connection and sync: `DaemonLink` with reconnect backoff and heartbeat, `SyncEngine` with snapshot plus cursor, `ResourceStore`, `DaemonModel` presence and `MutationAvailability`, and the mutation path (spec 0013 AC-7 to AC-11, AC-19; after milestone 1, can run in parallel with milestone 3)
  - [x] Shell UI: main `Window`, sidebar with badges, footer, toolbar chrome, unbuilt page placeholders, daemon banner, Diagnostics connection section, Settings placeholder, close window notice, and window sizes (spec 0013 AC-9 to AC-15, AC-21, AC-22; needs feature 12 milestones 2 to 4; can run in parallel with milestones 1 and 2)
  - [ ] Wiring, instances, and operations: app wiring, status adapter, provisional instance list, operation center, end to end tests, and manual evidence (spec 0013 AC-7 to AC-11, AC-13, AC-15 to AC-18, AC-21; after milestones 1 to 3, the feature 7 operation DTO, and the feature 10 or 14 instance DTO). Built: step 15 (app wiring, Quit closes the socket and the link never retries), step 17 (`OperationCenter` and the operation cases of `StatusAdapter`, tested on the protocol 1.3 fixtures), and the daemon part of step 18 (end to end tests against a real `stacklyd` for connect, the offline banner with mutations disabled, reconnect through a `server_restarted` gap, and close or quit; the operation end to end test cancels until `stacklyd` serves `operation.list`). Open: step 16 (instance cases of `StatusAdapter`, `InstanceStore`, `ProvisionalInstanceList`) waits for the feature 10 instance DTO, the instance part of step 18 waits for feature 14. Manual evidence on `7818bea`: checks 1, 2, 4, 5, 7, and 8 `PASS` (driven through the accessibility API, reviewed by the user), check 6 (keyboard and VoiceOver) `NOT_RUN`, check 3 waits for feature 14; verify steps in [verify.md](../specs/0013-app-shell-swift-client/verify.md)
- [ ] Verify it: `/check verify app shell SwiftUI & Swift client`
- [ ] Test it: `/test app shell SwiftUI & Swift client`

### 67. Komponen dasar Stackly · needs a decision

Kumpulan view SwiftUI yang dipakai ulang oleh semua layar journey, supaya tiap layar dirakit dari bagian yang sama dan state ditangani seragam. Dibangun di atas token dan pola state dari fitur 12.

**Done when:** komponen yang muncul di lebih dari satu layar mockup UI tersedia sebelum fase 02 dan memakai ulang yang sudah dibangun fitur 12 (`StatusLabel`, `ToneBadge`, `StateBanner`, `StatePlaceholder`, `.mutationGated()`, `.disabled(reason:)`, dan `.stacklyConfirmation`), bukan membangunnya ulang; komponen baru antara lain tile provider, daftar key value, sheet dengan tombol di footer, umpan balik singkat setelah aksi sebagai pengganti toast di mockup yang juga diumumkan ke VoiceOver (tanpa glass kustom), progress bertahap, checklist preflight, view log monospace, view diff, menu aksi, dan inspector; tiap komponen menerima state dari pola fitur 12; tombol untuk capability yang unsupported tampil nonaktif beserta alasannya; semua komponen bisa dipakai dengan keyboard, punya label VoiceOver, dan ikut Dynamic Type.

**Trace:** REQ-024, REQ-026 (capability unsupported), REQ-027, NFR-012, AC-027

- [ ] Design it (spec): `/architect komponen dasar Stackly`

### 68. Fixture tampilan & cek visual · needs a decision

Data contoh dari mockup UI dijadikan fixture Swift untuk Xcode Previews dan snapshot, supaya tiap layar bisa dicek di semua state, tampilan, dan pengaturan aksesibilitas tanpa daemon nyata. Aksesibilitas dicek per layar saat layar itu dibangun, bukan diaudit di akhir.

**Done when:** fixture memakai tipe kontrak yang sama dengan Swift client (fitur 13), bukan model terpisah; fixture mencakup kasus di mockup UI (instance running, starting, stopping, stopped, crash loop, konflik port, drift config, environment partially ready, operasi berjalan dan gagal, runtime external, daemon offline, dan workspace kosong); tiap layar yang dibangun journey punya snapshot light dan dark, teks besar, Increase Contrast, dan Reduce Transparency untuk state yang dimilikinya, plus pemeriksaan aksesibilitas otomatis (elemen tanpa label, target terlalu kecil, kontras); semuanya berjalan di Mac ini, dan perubahan snapshot terlihat saat review.

**Trace:** REQ-024, REQ-027, NFR-012, AC-027, PRD bagian 28

- [ ] Design it (spec): `/architect fixture tampilan & cek visual`

### 69. Menu aplikasi, perintah & shortcut keyboard · needs a decision

Menu Stackly, File, Edit, View, Window, dan Help beserta peta shortcut, supaya setiap aksi utama di mockup UI bisa dijalankan tanpa mouse sesuai idiom macOS.

**Done when:** peta perintah dan shortcut ada sejak shell, dan tiap fitur journey mendaftarkan perintahnya di peta itu (misalnya New Instance, Settings ⌘,, Close Window ⌘W, Find, pindah bagian sidebar, start, stop, dan restart instance terpilih, copy connection, tampilkan atau sembunyikan inspector, reconnect); shortcut tidak bentrok dengan shortcut sistem; perintah nonaktif tepat saat tombolnya nonaktif (daemon offline, operasi berjalan, capability unsupported); menu Help membuka dokumentasi pengguna dari fitur 66.

**Trace:** REQ-027, AC-027, NFR-012

- [ ] Design it (spec): `/architect menu aplikasi, perintah & shortcut keyboard`
