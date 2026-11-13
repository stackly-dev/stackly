# Langkah manual: `08-environment-proyek`

Langkah yang butuh mata, tangan, akun, perangkat, atau izin kamu. Tidak ada sesi yang menjalankannya. Kirim hasilnya ke lead (ID langkah, `PASS` atau `FAIL`, dan output bila `FAIL`); lead meneruskannya ke checker yang mencentang `verify.md`. `FAIL` membuka kembali kotaknya.

Keputusan yang diambil atas namamu ada di `08-environment-proyek.log.md` bagian "Keputusan delegasi"; baris bertanda `berat` paling perlu kamu baca. Untuk membatalkan satu baris, sebut barisnya ke lead.

## Ringkasan

| ID | Langkah | AC | Kotak yang dicentang dengan langkah ini `NOT_RUN` | Bisa dijalankan sesudah | Hasil |
| --- | --- | --- | --- | --- | --- |
| `F44-1` | Import manifest contoh ERP di app, baca sheet plan, banner manifest, Validate, Reload, Locate, Remove, Overview, Instances, mode gelap (18 langkah) | `spec 0044 AC-18 sampai AC-23, AC-25` (dan AC-6 lewat GUI) | F44 milestone 3 "Environments di app", `Build it`, `Verify it` | F44 milestone 1 sampai 3 di `main` (`dfd1f3c6`) | `NOT_RUN` |
| `F44-2` | Menu baris service yang sudah punya instance lewat `instanceRef` (Open In, Copy Connection URL, Show Instance, Show Logs, Stop atau Start, klik ganda); mengunduh runtime Valkey | `spec 0044 AC-19` | F44 milestone 3 "Environments di app", `Build it`, `Verify it` | F44 milestone 3 dan fix app di `main` (`be9ff7a3`) | `NOT_RUN` |
| `F45-1` | Lock Versions, Update Lock…, `.stackly.local.yaml`, sheet plan dengan lock dan override, Copy Environment Variables (13 langkah; langkah 10 mengunduh Valkey) | `spec 0045 AC-18, AC-19, AC-21` (dan AC-1, AC-3, AC-9, AC-10, AC-15 lewat GUI) | F45 milestone 3 "Lock dan override di app" | F45 milestone 1 dan 3 di `main`; langkah 10 butuh `stackly env export` (milestone 2) untuk perbandingan | `NOT_RUN` |

## Nilai dari user

Nilai yang hanya kamu tahu. AC di kolom ketiga `BLOCKED` sampai nilainya ada.

| ID | Nilai yang dibutuhkan | AC yang menunggu | File yang menunggu |
| --- | --- | --- | --- |

Belum ada nilai yang dibutuhkan.

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
