# Scope 06 · Menambah provider

[Kembali ke index](index.md)

Journey PRD JOURNEY-07 untuk kontributor: scaffold, tulis manifest dan template, validasi lokal, beri trust eksplisit, jalankan test kontrak dan integrasi, lalu kirim PR. Fase ini sengaja datang sebelum stack enam service, supaya lima provider resmi berikutnya dibangun lewat jalur dan harness yang sama dengan kontributor.

### 36. Provider SDK & test harness · in-progress

Kebijakan kompatibilitas, deprecation, dan migrasi untuk schema `stackly/v1alpha1` yang dikunci fitur 9 (termasuk penambahan probe dan primitive baru secara additive), validator, template scaffold, dan test harness (`stackly provider validate`, `cargo xtask provider test`) dengan fake server yang bisa delay ready, crash, dan spam log.

**Done when:** manifest dengan versi schema yang tidak didukung ditolak dengan penjelasan tanpa menjalankan lifecycle; provider fake server baru bisa install, create, start, health, dan tampil sebagai form di GUI tanpa perubahan Rust core atau SwiftUI; harness berjalan di test root ephemeral dan cleanup memeriksa ownership test run; harness menghasilkan report dari repository bersih.

**Trace:** REQ-030, AC-029 (akhir), AC-030, AC-034, PRD bagian 21, 27, dan 32.3

**Pointer:** spec [0036](../specs/0036-provider-sdk-test-harness/index.md) · code in `crates/core/src/provider/check.rs`, `crates/core/src/provider/scenarios.rs`, `crates/core/src/provider/testing.rs`, `crates/protocol/src/provider.rs`, `apps/daemon/src/ipc/provider.rs`, `apps/cli/src/commands/provider/`, `xtask/src/provider/`, `xtask/src/runtime/spike/package_recipe.rs`, `apps/daemon/tests/conformance/`, `apps/daemon/examples/provider_harness.rs`, `crates/testkit/src/test_root/marked.rs`, `providers/example/`

- [x] Design it (spec): `/architect provider SDK & test harness`
- [x] Build it: `/develop provider SDK & test harness`
  - [x] Validator dan kebijakan: `core::provider::check` dan `scenarios` (`SUPPORTED_API_VERSIONS`, `check_package`, `authoring_findings`, `tests/scenarios.yaml`, fuzz target `provider_scenarios`), `cargo xtask provider check` di atasnya, method `provider.validate` (minor protokol berikutnya) dengan handler daemon dan fixture kontrak, `stackly provider validate`, lalu corpus beku dengan `frozen_facts` dan `cargo xtask provider freeze` (spec 0036 AC-1 sampai AC-6; build step 1 sampai 5; needs fitur 9 dan 17, sudah done; fuzz 10 menit adalah langkah manual)
  - [x] Paket contoh, scaffold, dan form: `providers/example/` dengan skenario dan README, `cargo xtask provider new`, fixture form untuk setiap folder `providers/` dan test `StacklyUI` generik (spec 0036 AC-7, AC-8; build step 6 sampai 8; after milestone 1 step 1; needs fitur 20, sudah done; step 6 dan 8 bisa paralel dengan step 3 dan 4)
  - [x] Harness konformitas: `TestRoot` berpenanda di testkit, modul `apps/daemon/tests/conformance/` dengan langkah skenario dan report, kontrak proses dengan flag rogue fake server dan paket `fake-rogue`, skenario fault, example `provider_harness`, dan `cargo xtask provider test` dengan `--cleanup` dan `--keep` (spec 0036 AC-9 sampai AC-14, AC-1 bagian harness; build step 9 sampai 13; after milestone 2 step 6; needs milestone 1 step 1)
  - [x] Konformitas native: satu test native per line untuk `nats`, `postgresql` 17 dan 18, dan `traefik` di `provider_conformance.rs`; run nya setelah persetujuan kamu, `BLOCKED` tanpa artefak di cache xtask (spec 0036 AC-15; build step 14; after milestone 3)
  - [x] Spike dari paket: `PackageRecipe`, `closure_probes.rs`, fitur `test-support` dan `tokio` di `xtask`, entri `EXPOSE_ALLOWED`, penghapusan recipe NATS, PostgreSQL, dan Traefik, dan penanda `F36` (spec 0036 AC-16; build step 15; needs milestone 1 step 1 dan update spec 0003; bisa paralel dengan milestone 2 dan 3; run spike native setelah persetujuan kamu)
- [x] Verify it: `/check verify provider SDK & test harness`
- [x] Test it: `/test provider SDK & test harness`

### 37. Import provider lokal & trust · in-progress · GA

Import provider dari direktori atau paket lokal secara eksplisit, preview metadata tanpa menjalankan apa pun, lalu approval yang terikat ke publisher, revision, digest, dan capability. Revocation memblokir install baru tanpa menghapus data.

**Done when:** provider lokal yang belum dipercaya bisa diperiksa, tapi executable dan init tidak berjalan; revision baru yang meminta izin atau executable berbeda menunggu approval ulang; revocation (dari catalog terpasang atau pencabutan approval) tidak menghapus runtime atau data, dan saat offline tidak menghalangi start runtime yang sudah terpasang serta tidak diklaim terkini; quarantine pada paket lokal ditangani kebijakan fitur 3 tanpa meminta pengguna mematikannya secara global; dialog izin menyatakan bahwa native executable berjalan dengan hak user; `stackly provider import` setara dengan GUI.

**Trace:** REQ-028, REQ-032, REQ-033, BR-020, VAL-018, PAGE-08, AC-032, AC-033, AC-061, AC-072, OQ-009, PRD bagian 21.4 dan 23

**Pointer:** spec [0037](../specs/0037-import-provider-lokal-trust/index.md) · code in `crates/core/src/catalog/local/`, `crates/core/src/catalog/trust.rs`, `crates/core/src/catalog/store.rs`, `crates/storage/src/registry/import.rs`, `crates/protocol/src/provider.rs`, `apps/daemon/src/ipc/provider_import.rs`, `apps/cli/src/commands/provider/`, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Catalog/`

- [x] Design it (spec): `/architect import provider lokal & trust`
- [ ] Build it: `/develop import provider lokal & trust`
  - [x] Local package into the catalog: `provider_imports` migration and registry ops, `package_sha256`, `Permissions`, import rules on `check_package`, the content addressed store, local entries in the catalog read model, the protocol major bump with all `provider.*` methods, DTOs, codes and `catalog.changed`, and the daemon handlers for preview, import and remove with audit and the quarantine report (spec 0037 AC-1, AC-2, AC-3, AC-9, AC-10, AC-13, AC-17, AC-18; build steps 1 to 5; needs feature 36: spec 0036 step 1 for steps 2, 3 and 5, and spec 0036 step 3 for step 4)
  - [x] Approval, revocation and the gate: `approve_provider` and `revoke_provider`, the trust gate at every listed point (runtime `select()`, forms, `resolve_package`, `gather_duplicate`, commit guards), the pin record for local entries, same software selection, and the `revoked` list in the bundled catalog (spec 0037 AC-4, AC-5, AC-6, AC-7, AC-8, AC-11, AC-12, AC-17, AC-19; build steps 6 to 8; after milestone 1)
  - [x] `stackly provider import`, `approve`, `revoke`, `remove`: subcommands, confirmation, the `TRUST` column and the local inspect section, goldens and binary tests (spec 0037 AC-14, AC-11; build step 9; after build step 4 for goldens; binary tests need milestone 2; after spec 0036 step 4; can run in parallel with milestones 2 and 4)
  - [x] Import and approval in the app: copy and appearance, the permissions list and local detail section, the Import Provider flow with its sheet, the approval sheet, revoke and remove dialogs, Settings Sources, the onboarding copy, snapshots, then evidence and the manual steps that need your eyes and hands (spec 0037 AC-11, AC-15, AC-16, AC-8, AC-13; build steps 10 to 13; after build step 4; step 13 after milestones 2 and 3; can run in parallel with milestones 2 and 3)
- [ ] Verify it: `/check verify import provider lokal & trust`
- [ ] Test it: `/test import provider lokal & trust`
- [ ] Review it (fresh model): `/check review import provider lokal & trust`
- [ ] Document it: `/document import provider lokal & trust`

### 38. Panduan kontributor & governance · in-progress

Dokumen dan aturan supaya orang lain bisa menyumbang provider: CONTRIBUTING, schema docs, provider template, security policy, Code of Conduct, issue dan PR template, serta pilihan lisensi project.

**Done when:** kontributor yang mengikuti panduan dari repository bersih bisa menjalankan validator dan harness pada contoh provider; lisensi core sudah diputuskan dan tercatat; kanal security disclosure privat dan proses advisory tersedia.

**Trace:** REQ-034, AC-034, OQ-013, JOURNEY-07, PRD bagian 39

**Pointer:** spec [0038](../specs/0038-panduan-kontributor-governance/index.md) · code in `xtask/src/docs/`, `docs/providers/`, `CONTRIBUTING.md`, `SECURITY.md`, `GOVERNANCE.md`, `.github/`

- [x] Design it (spec): `/architect panduan kontributor & governance`
- [ ] Build it: `/develop panduan kontributor & governance`
  - [x] License record, `cargo xtask docs check`, and the community files: `repository` and the README license clause, the docs checker with its broken fixture tests, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `GOVERNANCE.md`, the three issue forms with `config.yml`, the PR template, `CONTRIBUTING.md`, `LICENSE-NOTICES.md` for nats, postgresql, and traefik, and the README update (spec 0038 AC-1 to AC-4, AC-6, AC-7, AC-9, AC-13, AC-14; build steps 1 to 7; no prerequisite, can run next to the feature 36 build; AC-3 needs owner input I1 and AC-5 is a manual owner step)
  - [x] Provider guide and schema reference: the three reference pages with the schema coverage check, `docs/providers/README.md` with the walkthrough block, `authoring.md`, `testing.md`, `compatibility.md`, `review.md`, and the CONTRIBUTING provider section (spec 0038 AC-2, AC-10, AC-11; build steps 8 and 9; needs feature 36)
  - [x] `cargo xtask docs walkthrough` from a fresh clone, its tests, one full run, and the manual evidence record for AC-034 (spec 0038 AC-8; build step 10; needs feature 36, after the provider guide)
  - [ ] Trying a provider locally: `docs/providers/trying-locally.md` with the approval statement check, trust terms aligned in `GOVERNANCE.md` and `review.md`, and the revocation sentences in `SECURITY.md` and `GOVERNANCE.md` (spec 0038 AC-12, AC-4, AC-6; build step 11; needs feature 37)
- [ ] Verify it: `/check verify panduan kontributor & governance`
- [x] Test it: `/test panduan kontributor & governance`
