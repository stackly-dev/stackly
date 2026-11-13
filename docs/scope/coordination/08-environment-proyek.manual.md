# Langkah manual: `08-environment-proyek`

Langkah yang butuh mata, tangan, akun, perangkat, atau izin kamu. Tidak ada sesi yang menjalankannya. Kirim hasilnya ke lead (ID langkah, `PASS` atau `FAIL`, dan output bila `FAIL`); lead meneruskannya ke checker yang mencentang `verify.md`. `FAIL` membuka kembali kotaknya.

Keputusan yang diambil atas namamu ada di `08-environment-proyek.log.md` bagian "Keputusan delegasi"; baris bertanda `berat` paling perlu kamu baca. Untuk membatalkan satu baris, sebut barisnya ke lead.

## Ringkasan

| ID | Langkah | AC | Kotak yang dicentang dengan langkah ini `NOT_RUN` | Bisa dijalankan sesudah | Hasil |
| --- | --- | --- | --- | --- | --- |
| `F44-1` | Import manifest contoh ERP di app, baca sheet plan, banner manifest, Validate, Reload, Locate, Remove, Overview, Instances, mode gelap (18 langkah) | `spec 0044 AC-18 sampai AC-23, AC-25` (dan AC-6 lewat GUI) | F44 milestone 3 "Environments di app", `Build it`, `Verify it` | F44 milestone 1 sampai 3 di `main` (`dfd1f3c6`) | `NOT_RUN` |
| `F44-2` | Menu baris service yang sudah punya instance lewat `instanceRef` (Open In, Copy Connection URL, Show Instance, Show Logs, Stop atau Start, klik ganda); mengunduh runtime Valkey | `spec 0044 AC-19` | F44 milestone 3 "Environments di app", `Build it`, `Verify it` | F44 milestone 3 dan fix app di `main` (`be9ff7a3`) | `NOT_RUN` |
| `F45-1` | Lock Versions, Update Lock…, `.stackly.local.yaml`, sheet plan dengan lock dan override, Copy Environment Variables (13 langkah; langkah 10 mengunduh Valkey) | `spec 0045 AC-18, AC-19, AC-21` (dan AC-1, AC-3, AC-9, AC-10, AC-15 lewat GUI) | F45 milestone 3 "Lock dan override di app" | F45 milestone 1 dan 3 di `main`; langkah 10 butuh `stackly env export` (milestone 2) untuk perbandingan | `NOT_RUN` |
| `F46-1` | Up manifest contoh ERP di app dengan daemon dan Keychain login ASLI: Set… secret, Bring Up, progress per service, partially ready, Activity bersarang, interupsi, Cancel, Up dari terminal, Create with Up… (15 langkah) | `spec 0046 AC-19 sampai AC-22, AC-25` (dan AC-15 lewat GUI) | belum ada (milestone 4 F46 menunggu verify) | F46 milestone 2 dan 4 di `main` (`098b174a`) dan fix AC-15 dari `dev-f47-m1-s3`; enam runtime ERP terpasang | `NOT_RUN` |

## Nilai dari user

Nilai yang hanya kamu tahu. AC di kolom ketiga `BLOCKED` sampai nilainya ada.

| ID | Nilai yang dibutuhkan | AC yang menunggu | File yang menunggu |
| --- | --- | --- | --- |
| `N1` | nilai secret uji untuk enam reference `local:erp/...` manifest contoh ERP (boleh nilai sembarang yang hanya dipakai untuk uji) | `spec 0046 AC-21, AC-25` lewat `F46-1` | Keychain login (diisi lewat sheet Enter Secret di app) |

## Fitur `44`: `Manifest environment: validate & plan` (spec `0044`)

### `F44-1` · Import manifest contoh ERP dan jelajahi halaman Environments

Dari laporan `dev-f44-m3` (`2026-11-13`). Membuktikan spec `0044` `AC-18` sampai `AC-23`, `AC-25`, dan `AC-6` lewat GUI.

- **Prasyarat:** dari checkout `main`:
  - `cargo build -p stacklyd -p stackly-fake-server`
  - `cargo xtask build-app`
  - `export STACKLY_ROOT=$(mktemp -d)`
  - `mkdir "$STACKLY_ROOT/erp" && cp crates/core/tests/fixtures/environments/erp-development.yaml "$STACKLY_ROOT/erp/stackly.yaml"`
  - di satu Terminal: `STACKLY_LOG=debug cargo run -q -p stacklyd`
  - `open --env STACKLY_ROOT="$STACKLY_ROOT" target/app/Debug/Stackly.app`
  - Jangan pakai Application Support asli. Pointer saja, tanpa `cargo xtask ui-test`.
- **Jalankan:** 18 langkah bernomor di `docs/specs/0044-manifest-environment-validate-plan/verify.md`, bagian `## Milestone 3: Environments di app` → `### UI / manual` (halaman kosong, Import… dan sheet plan, cek tanpa efek samping sebelum Import, Import, menu baris service, Validate, Open Manifest dan banner "The manifest changed on disk.", Review Plan dan Reload, secret literal dan seksi Problems, Reveal in Finder dan Copy Path, ganti nama file dan Locate…, Overview, Instances, impor file kedua bernama sama dan Use This File, Remove Environment…, daemon mati lalu hidup, mode gelap, lalu `rm -rf "$STACKLY_ROOT"`).
- **Lulus bila:** tiap langkah menampilkan teks yang verify.md sebut (mis. sheet "Plan for erp-development" dengan enam baris "+ Create", feedback "Imported “erp-development”.", sidebar "0/6", "Removed “erp-development”."), dan sebelum Import tidak ada runtime, proses, atau file data baru di `$STACKLY_ROOT`. Sesudahnya isi record `evidence/manual/2026-11-13-spec-0044-journey-04-import-the-erp-manifest-in.json` (steps, actual, status, reviewer, finishedAt) dan jalankan `cargo xtask evidence check`.
- **Hanya langkah ini yang bisa membuktikan:** tampilan dan interaksi nyata halaman Environments, panel file macOS, dan app bawaan `.yaml`; test snapshot dan test integrasi Swift tidak mengklik app sungguhan.

### `F44-2` · Menu baris service yang instance-nya sudah ada

Dari laporan `dev-f44-m3` (`2026-11-13`), langkah 19 di `verify.md`. Membuktikan spec `0044` `AC-19`.

- **Prasyarat:** seperti `F44-1`, dengan `STACKLY_ROOT` baru (`export STACKLY_ROOT=$(mktemp -d)`) dan jaringan menyala: langkah ini mengunduh runtime Valkey, jadi butuh izin unduhanmu.
- **Jalankan:**
  - Di app: Catalog → Valkey → Get, tunggu Installed; New Instance → Valkey, nama `shared-valkey`, Create, biarkan berjalan.
  - `mkdir -p "$STACKLY_ROOT/borrow" && printf 'apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: borrow-demo\nservices:\n  cache:\n    provider: official/valkey\n    instanceRef: shared-valkey\n' > "$STACKLY_ROOT/borrow/stackly.yaml"`
  - Environments → Import… → pilih `borrow/stackly.yaml` → Import.
  - Di baris `cache`: buka menu baris (ellipsis), jalankan Copy Connection URL (tempel di TextEdit), Show Instance, kembali, Show Logs, kembali, Stop, lalu Start; klik ganda baris; klik kanan baris.
  - More → Remove Environment… → Remove; lalu `rm -rf "$STACKLY_ROOT"`.
- **Lulus bila:** sheet import berisi satu baris "= Keep" (atau "▸ Start" bila instance berhenti) `cache → shared-valkey`; baris `cache` menampilkan Selector "instanceRef: shared-valkey", Ownership "Shared ref", dan status instance; menu berurutan: item Open In milik Valkey, Copy Connection URL, Show Instance dan Show Logs, Stop (atau Start); Copy Connection URL menyalin connection string pertama kartu Connection; Show Instance dan klik ganda membuka detail `shared-valkey`; Show Logs membuka tab Logs; Stop membuat status baris Stopped dalam beberapa detik; klik kanan memberi menu yang sama; sesudah Remove, `shared-valkey` tetap ada di Instances.
- **Hanya langkah ini yang bisa membuktikan:** menu baris pada service ber instance di app nyata; sebelum Up (spec 0046) tidak ada cara lain untuk mendapat service ber instance, dan test `ServiceMenu.sections` hanya membuktikan isi menu, bukan aksinya.

## Fitur `45`: `Lockfile & override lokal` (spec `0045`)

### `F45-1` · Lock dan override lokal di app

Dari laporan `dev-f45-m3` (`2026-11-13`). Membuktikan spec `0045` `AC-18`, `AC-19`, `AC-21`, dan lewat GUI `AC-1`, `AC-3`, `AC-9`, `AC-10`, `AC-15`.

- **Prasyarat:** dari checkout `main`:
  - `cargo build -p stacklyd -p stackly-fake-server`
  - `cargo xtask build-app`
  - `export STACKLY_ROOT=$(mktemp -d)`
  - `mkdir -p "$STACKLY_ROOT/erp" && cp crates/core/tests/fixtures/environments/erp-development.yaml "$STACKLY_ROOT/erp/stackly.yaml"`
  - di satu Terminal dengan root itu: `STACKLY_LOG=debug cargo run -q -p stacklyd`
  - `open --env STACKLY_ROOT="$STACKLY_ROOT" target/app/Debug/Stackly.app`
  - di app: Environments → Import… → pilih `erp/stackly.yaml` → Import.
  - Jangan pakai Application Support asli. Langkah 10 butuh jaringan dan izin unduhan (runtime Valkey) dan instance `shared-valkey` serta environment `borrow-demo` dari `F44-2`.
- **Jalankan:** 13 langkah bernomor di `docs/specs/0045-lockfile-override-lokal/verify.md`, bagian `## Milestone 3: Lock dan override di app` → `### UI / manual` (Details "Not locked" dan "None"; Lock Versions dan isi `stackly.lock`; Lock Versions kedua; Update Lock… dengan Cancel dan Update; ubah versi postgres dan baris "out of date"; `.stackly.local.yaml` dengan port 35432; alias tak dikenal sebagai warning; endpoint salah sebagai problem; hapus file override dan Copy Environment Variables sebelum Up; Copy Environment Variables pada `borrow-demo`; daemon mati lalu hidup; mode gelap; bersihkan root dan isi record).
- **Lulus bila:** teks tiap langkah cocok dengan verify.md, antara lain "Locked 6 services.", "The lock is up to date.", Lock "stackly.lock · macos-aarch64", `grep -c -e "$HOME" -e "$STACKLY_ROOT" -e local: "$STACKLY_ROOT/erp/stackly.lock"` mencetak 0, `shasum` `stackly.yaml` dan `stackly.lock` sama sebelum dan sesudah menulis override, postgres memakai 5432 dari manifest saat file override error, "Nothing to copy yet. Bring the environment up first.", dan "Copied 3 variables." dengan tepat tiga baris `STACKLY_CACHE_CLIENT_HOST`, `_PORT`, `_ADDRESS` tanpa password, path, atau PID. Sesudahnya isi record `evidence/manual/2026-11-13-spec-0045-lock-and-local-override-in-the-app.json` dan jalankan `cargo xtask evidence check`.
- **Hanya langkah ini yang bisa membuktikan:** menu More dan dialog konfirmasi di app nyata, clipboard, banner sesudah file berubah di luar app, dan isi file lock yang ditulis lewat GUI.

## Fitur `46`: `Env up & dependency graph` (spec `0046`)

### `F46-1` · Up manifest contoh ERP di app

Dari laporan `dev-f46-m4` (`2026-11-14`). Membuktikan spec `0046` `AC-19` sampai `AC-22`, `AC-25`, dan `AC-15` lewat GUI.

**Berbeda dari `F44-1` dan `F45-1`:** langkah ini memakai `stacklyd` dan Keychain login kamu yang asli, bukan root sementara. Ia memasang enam runtime (unduhan bila belum ada), membuat enam instance `erp-development-*`, dan menulis enam item `local:erp/...` ke Keychain login. Sesudah selesai, instance dan item Keychain itu tetap ada sampai kamu menghapusnya sendiri (Down tidak menghapus data, sesuai spec 0047).

- **Prasyarat:**
  - Mac Apple Silicon macOS 27, sesi login GUI.
  - dari checkout `main` yang bersih: `cargo build -p stacklyd -p stackly-cli` dan `cargo xtask build-app`
  - `stacklyd` berjalan untuk user ini.
  - enam runtime ERP terpasang: `cargo test -p stacklyd --test env_up_native -- --ignored --test-threads=1` lulus (memakai cache xtask), atau pasang lewat halaman Runtimes (unduhan, butuh jaringan).
  - `mkdir -p ~/Code/erp && cp crates/core/tests/fixtures/environments/erp-development.yaml ~/Code/erp/stackly.yaml`, lalu ganti port yang bentrok dengan service lain di Mac ini.
  - reference `local:erp/...` belum ada di Keychain; siapkan nilai uji (`N1`).
- **Jalankan:** 12 langkah bernomor di `docs/specs/0046-env-up-dependency-graph/verify.md`, bagian `## Milestone 4: Up di app` → `### UI / manual` (Import dan tombol Up…; sheet plan dengan Bring Up dan baris "Needs secret" ber Set…; Set… satu reference; Bring Up dengan reference kosong membuka Enter Secret tanpa operasi baru; ubah port sesudah sheet terbuka dan banner "The plan changed. Review it again."; Bring Up dan banner "Bringing up… {done} of {total}" dengan status per service; Show in Activity dengan anak bersarang; hasil akhir enam service Running dengan port aktual; gagalkan mysql dengan `nc -l 127.0.0.1 <port mysql>` lalu Up lagi; hapus satu item Keychain lalu Bring Up; Quit app di tengah Up dan daemon dihentikan di tengah Up; daemon berhenti selagi sheet plan terbuka).
- **Langkah tambahan 13 sampai 15** (di verify.md yang sama): (13) Cancel di banner saat Up berjalan: operasi Up dan anak yang belum selesai menjadi Cancelled, service yang sudah ready tetap Running; (14) `stackly env up ~/Code/erp` dari Terminal selagi halaman detail terbuka: dalam sekitar satu detik, tanpa klik, sidebar, daftar, dan detail menampilkan "Bringing up…" dan kolom Status mengikuti CLI (butuh CLI F46, spec 0046 milestone 3, dan `stackly` di PATH memakai socket yang sama); (15) "Create with Up…" dari menu baris service baru: hanya service itu yang dibuat dan dinyalakan, yang lain tidak di-restart.
- **Lulus bila:** teks tiap langkah cocok dengan verify.md, antara lain catatan kaki sheet "Services start in dependency order and wait for readiness. If one fails, the rest stay up and the environment is marked partially ready.", tidak ada operasi baru di Operations saat secret masih kosong, banner "{n} of 6 ready" menyebut mysql "Start failed: …" dengan Show Operation, label Partial "{ready} of 6 services ready", banner "Needs secret local:erp/valkey/password" dengan Set Secret…, dan sesudah daemon dihentikan di tengah Up: "The last Up was interrupted. Bring it up again to continue.". Service yang sudah ready tidak pernah dihentikan oleh Up yang gagal.
- **Hanya langkah ini yang bisa membuktikan:** Up lewat app nyata dengan Keychain login dan `SecretEntrySheet`, progress yang terlihat, dan install runtime sebagai operasi anak lewat daemon asli (spec 0046 AC-6, `BLOCKED` di verify karena butuh unduhan).
