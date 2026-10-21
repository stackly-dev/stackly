# Log koordinasi: `03-multi-versi-dan-instance`

- **Scope:** `docs/scope/03-multi-versi-dan-instance.md`
- **Hash awal:** `4d5d177` · **Hash awal wave aktif:** `4d5d177`
- **Wave aktif:** `1`, mulai `2026-10-22`
- **Diperbarui:** `2026-10-22 06:40`

## Sub sesi wave `1`

| Nama | Peran | Item | Scope | Ownership | Branch | Model | Status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `arc-f24-spec` | `architect` | 24. Instance kedua dari runtime yang sama · spec untuk `Build it` yang ada (spec 0024) | `03-multi-versi-dan-instance.md` | `docs/specs/0024-instance-kedua-runtime-sama/` | `claude/arc-f24-spec-b9ee1e` | `opus-5-5 · xhigh` | `aktif` |
| `arc-f25-spec` | `architect` | 25. PostgreSQL 17 & 18 berdampingan · spec untuk `Build it` yang ada (spec 0025) | `03-multi-versi-dan-instance.md` | `docs/specs/0025-postgresql-17-18-berdampingan/` | `claude/arc-f25-spec-20a08d` | `opus-5-5 · xhigh` | `aktif` |

## Keputusan

| Tanggal | Keputusan | Oleh |
| --- | --- | --- |
| `2026-10-22` | Keputusan lintas scope dari `01-foundation.log.md`, `02-instalasi-pertama.log.md`, dan `ui-macos.log.md` berlaku di sini: nomor spec = nomor fitur empat digit; cross check spec `Another model`; baris Follow-up spec sumber yang dijawab spec baru dicentang lead lewat `/sync` di akhir wave, bukan oleh architect; `graphify update .` sebelum commit merge kode, `/graphify . --update` untuk dokumen dikumpulkan di akhir wave; `start_session` ditolak pada giliran yang dipicu pesan sesi lain, jadi sub sesi berikutnya diajukan sebagai kartu; sub sesi build memakai `CARGO_INCREMENTAL=0` dan menghapus `target/debug/incremental` setelah commit; langkah manual user dikumpulkan di `docs/manual.md`. | `lead` |
| `2026-10-22` | Checkout utama dipakai bersama lead Scope 02 ("Scope 02 · Instalasi pertama") dan lead `ui-macos` ("macOS UI scope documentation"), mengikuti aturan bersama di `02-instalasi-pertama.log.md` (2026-10-21): commit hanya dengan pathspec, merge `--no-ff --no-commit` hanya saat tidak ada `MERGE_HEAD`, beri tahu lead lain sebelum merge kode, dan `graphify update .` tidak dijalankan saat lead lain sedang memperbaruinya. Saat koordinasi ini mulai, lead Scope 02 punya merge `claude/nifty-joliot-9c338f` (`dev-f22-m1`) yang terbuka dengan gate berjalan, jadi commit log ini menunggu `MERGE_HEAD` hilang. Lead ini mengedit `docs/scope/03-multi-versi-dan-instance.md` dan di `docs/scope/index.md` hanya baris fitur 24, 25, dan baris Status file 03. | `lead` |
| `2026-10-22` | Rekonsiliasi: tidak ada sesi, branch, atau commit untuk fitur 24 atau 25. Branch lain yang belum di-merge milik lead lain: `claude/dev-f18-m2-f7a13e`, `claude/dev-f20-m1-ed6675`, `claude/nifty-joliot-9c338f` (Scope 02), `claude/nice-banach-103b7f` (`chk-f12-verify`, `ui-macos`). Tidak ada kode fitur 24 atau 25 di `main`. Peer Stackly `busy`: `arc-f15-update`, `dev-f20-m1`; sesi CDC-ETL tidak dihitung karena repo lain. | `lead` |
| `2026-10-22` | Kotak `Build it` fitur 24 dan 25 tidak punya Pointer dan kotak `(spec)`, dan Done when keduanya menjangkau lebih dari satu crate (24: operasi duplicate config dengan ID, path, kebijakan secret, dan port baru; 25: penolakan rebind biasa sebelum fitur 58 ada), jadi keduanya diperlakukan seperti kotak `(spec)`: architect dulu, lalu lead menempel Pointer dan milestone di bawah `Build it` yang ada tanpa menambah `Design it` (contoh fitur 21). Nomor spec: `0024-instance-kedua-runtime-sama` dan `0025-postgresql-17-18-berdampingan`; keduanya bebas di `docs/specs/`, di `git branch`, dan di tabel sub sesi semua log. | `lead` |
| `2026-10-22` | Lead Scope 02 merinci aturan bersama: sebelum `git merge` ke `main` lead yang merge mengirim pesan ke dua lead lain, dan mengirim lagi setelah commit merge dan `graphify update .` masuk; tidak ada yang commit saat `MERGE_HEAD` ada; perubahan protokol masa ini di-merge satu per satu (F15 di 1.10, lalu runtime F18); sub sesi build memakai `CARGO_INCREMENTAL=0` dan menghapus `target/` worktree yang idle (disk penuh dua kali hari ini). Lead ini menerapkannya dan meminta slot ke lead Scope 02 untuk merge spec 0024 dan 0025. | `lead` |
| `2026-10-22` | User menyetujui antrian wave 1: `arc-f24-spec` dan `arc-f25-spec` paralel. Delegasi sama seperti Scope 02 dan `ui-macos`: lead mengambil opsi rekomendasi untuk gap spec, "Accept this spec?", dan pilihan approach, lalu mencatatnya di sini; perubahan teks AC, aksi keluar, run native, dan unduhan tetap ke user. | `user` |

## Blocker terbuka

- F24 `Build it`: menunggu spec 0024 (wave 1), lalu build fase 02 yang dikoordinasi lead Scope 02 (paling tidak F20 milestone 2 sampai 4, F21, F22); daftar persisnya menyusul dari Build plan spec 0024.
- F25 `Build it`: menunggu spec 0025 (wave 1), lalu F18 milestone 2 dan 3, F19 milestone 2 dan 3, F20 milestone 4, dan F21 di `02-instalasi-pertama.md`; daftar persisnya menyusul dari Build plan spec 0025.
- Commit log ini: menunggu merge `dev-f22-m1` lead Scope 02 di checkout utama selesai.

## Dicentang pada wave ini

## Riwayat wave
