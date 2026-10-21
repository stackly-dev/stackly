# Scope 03 · Multi versi & instance

[Kembali ke index](index.md)

Journey PRD JOURNEY-02 dan JOURNEY-03, nilai inti produk (GOAL-01): `pg17-dev`, `pg17-test`, dan `pg18-dev` jalan bersamaan dengan data, config, log, dan port yang terpisah. Fase ini selesai kalau data sentinel tiap instance tidak pernah bocor ke instance lain, termasuk setelah tiap instance dihentikan lalu dijalankan lagi (restart sebagai satu operasi datang di fitur 26).

### 24. Instance kedua dari runtime yang sama · in-progress

Buat instance kedua yang memakai runtime ID identik, lewat create biasa atau duplicate config. Duplicate config membuat storage baru, bukan clone data.

**Done when:** dua instance PG17 dengan runtime ID sama jalan bersamaan dengan port dan data berbeda; duplicate config menghasilkan ID, path data, kebijakan secret, dan port baru yang tervalidasi; data lama tidak pernah disalin.

**Trace:** REQ-009, REQ-025 (duplicate config), AC-009, AC-025, JOURNEY-02

**Pointer:** spec [0024](../specs/0024-instance-kedua-runtime-sama/index.md)

- [ ] Build it: `/develop instance kedua dari runtime yang sama`
  - [ ] Duplicate draft in core and the fake sentinel: `PUT` and `GET /sentinel` on the fake server, `build_form_from_package` with `build_form` calling it, and `instance::duplicate` (`gather_duplicate` reading only the registry, the loaded catalog, and the pinned package through spec 0010's `load_package` made `pub(crate)`, refusing a catalog entry with the same revision label and another digest, the pure `build_duplicate` with settings minus secret and managed fields, every port `auto`, every secret `generate`, and `duplicate_name`), with table tests and `FakePlatform` tests where the source has no folder at all (spec 0024 AC-1 to AC-5, AC-7 core part, AC-8 proof tool; build steps 1 and 2; needs feature 20 milestone 1 in `02-instalasi-pertama.md`; steps 1 and 2 can run as two parallel sessions, and step 2 does not run alongside a feature 20 session that still changes `crates/core/src/instance/create/form.rs`)
  - [ ] Contract, daemon, CLI, and fixture proof: `instance.get_duplicate_form` with its DTOs, fixtures `duplicate-form-fake.json` and `duplicate-form-postgresql.json`, a minor protocol bump, and codegen; the `stacklyd` handler and mappings; `Api::duplicate_form`, `Api::create_from_draft`, `put_sentinel`, and `get_sentinel` in the supervisor test support; the `supervisor/same_runtime.rs` daemon tests (two fake instances from one runtime with sentinels, stop and start in turn, a crash looping third instance, source data contents at mode 000, new secret references, ports, errors, and the 300 ms read); `stackly instance duplicate` with `--secret <field>=copy`, goldens, the parity matrix, and the JOURNEY-02 fake journey (spec 0024 AC-1, AC-4 to AC-8, AC-10, AC-11; build steps 3 to 5; after milestone 1; needs feature 20 milestone 2 and feature 22 milestone 2 (`resolve.rs`, `cli.instance_not_found`) in `02-instalasi-pertama.md`; after step 3, steps 4 and 5 can run as two parallel sessions)
  - [ ] Duplicate in the GUI: `StacklyClient.getDuplicateForm`, `CreateMode` and the duplicate states in `CreateInstanceModel`, the duplicate sheet mode (read only Runtime row, subtitle, Copy from prefilled with the source reference through `copyFromOptions` and the new `copyFromLabels`, feedback), `CreateSheetRequest` and `duplicateInstanceAction` in `AppShell`, the More menu item on the detail page, the `duplicateInstance` publisher on the Instances page, the built `duplicateInstance` row with its `CommandItem` (⌘D), the `design.md` command table, snapshots, and manual keyboard and VoiceOver evidence with the fake provider (spec 0024 AC-12 to AC-14; build steps 6 and 7; after step 3; needs feature 20 milestone 3, feature 21 milestone 3, and feature 22 milestone 3 in `02-instalasi-pertama.md`, feature 69 milestone 3 and feature 67 in `ui-macos.md`, and feature 11 milestone "Swift client and GUI secret flows" in `01-foundation.md`; can run alongside steps 4 and 5; step 7 does not run alongside another session that changes `AppShell.swift` or `StacklyCommands`)
  - [ ] PostgreSQL 17 on this Mac: the `SentinelCluster` helper in `apps/daemon/tests/postgresql/sentinel.rs`, the shared `two_postgresql_17_instances` setup, and the native tests `two_postgresql_17_instances_from_one_runtime_keep_their_sentinels` and `duplicate_postgresql_instance_starts_with_empty_data` with `native` evidence covering REQ-009, AC-009 (the two major 17 part), AC-025, and JOURNEY-02, then the manual GUI JOURNEY-02 evidence with PostgreSQL 17, each native run and GUI session after your approval (spec 0024 AC-7, AC-9, AC-14; build steps 8 and 9; after milestone 2, and milestone 3 for the GUI evidence; needs feature 19 milestone 3 (PostgreSQL 17 through the daemon and in the catalog) in `02-instalasi-pertama.md`; without the PostgreSQL 17 artifact in the xtask runtime cache the tests end `BLOCKED`; feature 25's three cluster milestone runs after step 9)
- [ ] Verify it: `/check verify instance kedua dari runtime yang sama`
- [ ] Test it: `/test instance kedua dari runtime yang sama`

### 25. PostgreSQL 17 & 18 berdampingan

Pasang runtime mayor kedua dan buat instance baru di sampingnya tanpa menyentuh data instance lama. Binary mayor yang salah terhadap data yang sudah ada selalu ditolak.

**Done when:** tiga cluster (dua PG17, satu PG18) menyimpan sentinel berbeda dan tidak saling terbaca setelah stop dan start ulang; menjalankan binary PG18 pada data PG17, atau rebind biasa, ditolak oleh preflight kompatibilitas dari fitur 19 dengan alasan butuh migrasi; install runtime baru tidak mengubah instance lama.

**Trace:** REQ-035 (multi versi), REQ-043 (penolakan rebind), BR-001, BR-009, AC-035 (tanpa bagian backup dan restore, yang menyusul di fase 09), AC-043, JOURNEY-03

- [ ] Build it: `/develop PostgreSQL 17 & 18 berdampingan`
