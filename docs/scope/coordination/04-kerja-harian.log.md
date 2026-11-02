# Log koordinasi: `04-kerja-harian`

- **Scope:** `docs/scope/04-kerja-harian.md` (fitur 28 sampai 32 dan 70, atas permintaan user "mulai dari no 28")
- **Hash awal:** `36b33e6e` · **Hash awal wave aktif:** `36b33e6e`
- **Wave aktif:** `1`, mulai `2026-11-02`
- **Diperbarui:** `2026-11-02 10:45`

## Sub sesi wave `1`

| Nama | Peran | Item | Scope | Ownership | Branch | Model | Status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `dev-f30-m1` | developer | F30 milestone 1, langkah 1 sampai 3 di atas WIP `9a6bf944`, plus perbaikan fixture kontrak (digest manifest F70); pemilik kontrak wave 1 | `04-kerja-harian.md` | `crates/protocol/`, `protocols/`, `schemas/`, `typeshare.toml`, hasil codegen, `crates/domain/src/restart.rs`, `crates/domain/src/registry/{guard,retention}.rs`, `crates/storage/src/registry/{restart,prune}.rs`, `crates/storage/tests/`, `crates/testkit/src/registry/conformance/restart.rs`, `apps/cli/src/commands/daemon/`, `apps/cli/tests/common/`, `apps/daemon/tests/{server*,shutdown_exit,root_override}.rs`, `clients/swift/Tests/` | `claude/dev-f30-m1-*` (sesi `local_167967ad`) | `sonnet-5-5 · high` | aktif |
| `dev-f29-m1` | developer | F29 milestone 1, langkah 1 sampai 7 di atas WIP `71ba7592` | `04-kerja-harian.md` | `crates/domain/` selain milik `dev-f30-m1`, `crates/core/src/instance/config/`, `crates/core/src/supervisor/`, `crates/core/src/instance/create/`, `crates/storage/src/` selain milik `dev-f30-m1`, `crates/storage/migrations/0008_*`, `crates/testkit/src/registry/` selain milik `dev-f30-m1`, `providers/fake/`, `crates/core/tests/` | `claude/dev-f29-m1-*` (sesi `local_581eb039`) | `sonnet-5-5 · high` | aktif |
| `dev-f28-m2-s4` | developer | F28 milestone 2 langkah 4 (`Sampler` di core) · Langkah Build plan selesai: 4 · Pointer code in `crates/core/src/metrics/` · API untuk langkah 5: `Sampler::new(..).start()`, trait `MainProcesses` (adapter dari registry dan `Supervisor::status` dibuat di langkah 5), `MetricsStore::with_history`, `MetricsHistory::page(historyId, afterSeq)` | `04-kerja-harian.md` | `crates/core/src/metrics/`, `crates/core/tests/metrics_sampler.rs` | `claude/dev-f28-m2-s4-a40135` (sesi `local_387469b1`) | `sonnet-5-5 · high` | merged `2485242e`; menunggu dipakai ulang untuk langkah 5 dan 6 |
| `dev-f70-m2` | developer | F70 milestone 2, langkah 7 sampai 9 di atas WIP `2f26d789` | `04-kerja-harian.md` | `apps/macos/Packages/StacklyUI/Sources/StacklyUI/OpenIn/`, toolbar detail dan inspector Instances, menu Instance, `Fixtures/`, `apps/macos/Packages/StacklyUI/Tests/`, test sentinel di `apps/daemon/tests/supervisor/open_with.rs` dan `apps/cli/tests/instance/open.rs` | `claude/dev-f70-m2-*` (sesi `local_c6d827bc`) | `sonnet-5-5 · high` | aktif |
| `arc-f31-update` | architect (update) | spec 0031 menerima *Tambahan pada kontrak spec 0031* dari spec 0032 (Follow-up spec 0032 baris 1) | `04-kerja-harian.md` | `docs/specs/0031-demand-autostart-login/` | kartu `task_502e14b2` | `opus-5-5 · xhigh` | diajukan |

## Keputusan

| Tanggal | Keputusan | Oleh |
| --- | --- | --- |
| `2026-11-02` | Program dimulai dengan `/coordinate docs/scope/04-kerja-harian.md`, mulai fitur 28; tujuannya semua kotak dicentang. Lead boleh menjalankan architect, develop, debug, test, document, dan sync. Untuk pertanyaan architect, lead mengambil opsi rekomendasi lalu mencatatnya di sini. | `user` |
| `2026-11-02` | Keputusan lintas scope dari `01-foundation.log.md`, `02-instalasi-pertama.log.md`, `03-multi-versi-dan-instance.log.md`, dan `ui-macos.log.md` berlaku: nomor spec = nomor fitur empat digit; cross check spec `Another model`; Follow-up spec sumber dicentang lewat `/sync` di akhir wave; brief melarang `pkill`/`killall` berdasarkan nama proses; `target` worktree di-symlink ke `target/` checkout utama bila disk sempit, dan sesi `touch` sumber paket lalu build ulang sebelum memakai binary bersama; hanya satu checker GUI yang memegang layar. | `lead` |
| `2026-11-02` | Rekonsiliasi: tidak ada worktree lain, tidak ada branch yang belum di-merge, tidak ada sesi peer. `main` memuat WIP F29 (`71ba7592`), F30 (`9a6bf944`), dan F70 (`2f26d789`) apa adanya (konsolidasi branch 2026-11-02): migrasi 7 F28, 8 F29, 9 F30; protokol 2.1. Milestone 1 F28 (`5ddedd61`) dan milestone 1 F70 sudah `[x]`. Clippy workspace: satu error di `crates/storage/tests/open/migrate.rs:393` (`type_complexity`). | `lead` |
| `2026-11-02` | User menyetujui antrian wave 1 (lima item di tabel plus item yang terbuka oleh merge wave ini). | `user` |
| `2026-11-02` | F31 langkah 1 (spike asid): langkah 2 sampai 9 dibangun dulu dengan asumsi desain spec 0031. Sleep dan dua logout dilakukan user saat sesi paralel di titik aman, sebelum journey 3. Bila hasilnya berbeda, item `architect (update)` spec 0031. LaunchAgent `com.stackly.spike.asid` tetap aktif. | `user` |
| `2026-11-02` | Run native dari artefak cache xtask, tanpa unduhan, diizinkan untuk sub sesi program ini (PostgreSQL, NATS, launchd). Jaringan mati, sleep, logout, reboot, Keychain terkunci, dan sesi GUI manual tetap ditanyakan per langkah lewat lead. | `user` |
| `2026-11-02` | Baseline test merah di `main` dari WIP: merge boleh bila tidak menambah kegagalan baru di crate yang disentuh, dan tiap fitur wajib menghijaukan test miliknya sebelum milestone-nya dicentang. Clippy workspace tetap wajib hijau saat merge. | `user` |
| `2026-11-02` | Baseline test `main` `36b33e6e` (`cargo test --workspace --no-fail-fast`), 43 test gagal di 9 target, disimpan di `docs/scope/coordination/04-kerja-harian.baseline.txt`: `stackly-cli --bin stackly` (constructor failure F30), `stackly-cli --test provider` dan `stackly-core --lib` `catalog::status` (digest manifest F70, milestone 3), `stackly-protocol` `catalog_fixtures`, `create_fixtures`, `duplicate_fixtures` (digest, `dev-f30-m1`), `stacklyd --lib` `every_domain_code_has_a_known_wire_value` (F29 milestone 2), `stacklyd --test server` 34 test (handshake mayor 1, `dev-f30-m1`), `stacklyd --test supervisor` `slow_logs::a_follower_that_rotation_passed_gets_the_gap_and_then_entries_in_order` (F27; mungkin timing saat empat sesi build, dicek ulang di gerbang). | `lead` |
| `2026-11-02` | Graph dokumen (scope, log, spec) dikumpulkan sekali di batas wave, mengikuti preseden Scope 03; commit merge kode tetap menjalankan `graphify update .` untuk file kodenya. | `lead` |
| `2026-11-02` | Sesi lead berjalan di effort `medium`; sub sesi baru dimulai setelah user menaikkannya ke `xhigh` (aturan SKILL.md langkah 1). User menaikkannya ke `xhigh`, lalu `dev-f30-m1`, `dev-f29-m1`, `dev-f28-m2-s4`, dan `dev-f70-m2` dimulai; `arc-f31-update` menunggu slot. Delapan sesi peer lama (architect dan develop F26 sampai F33) idle dan di luar koordinasi ini, jadi tidak dihitung. Tiap sesi memakai `target/` sendiri dengan `CARGO_INCREMENTAL=0` dan berhenti bila disk di bawah 20 GiB. | `lead` |

## Blocker terbuka

- `arc-f31-update`: aplikasi menolak `start_session` `user_asked` pada giliran yang dipicu pesan sesi lain; diajukan sebagai kartu `task_502e14b2`, dimulai user atau pada giliran user berikutnya.

- F31 langkah 1 dan 10 (spike asid dan logout): sleep dan dua logout oleh user.
- F32 `Build it`: butuh fitur 31 sampai `instance.stop_all` dan `startup`, plus `arc-f31-update`.

## Dicentang pada wave ini

- belum ada kotak; merge `claude/dev-f28-m2-s4-a40135` → `2485242e` (pecahan milestone 2 F28, kotak menunggu langkah 5 dan 6) · gerbang: fmt, `check-deps`, clippy workspace dan `cargo test -p stackly-core` hanya menyisakan baseline (`crates/storage/tests/open/migrate.rs:393`, `catalog::status`)

## Riwayat wave

- Wave `1` (`2026-11-02`, hash awal `36b33e6e`): berjalan

## Penutupan

- belum
