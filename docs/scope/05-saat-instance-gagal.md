# Scope 05 · Saat instance gagal

[Kembali ke index](index.md)

Journey PRD JOURNEY-05: start gagal, pesan yang aman, doctor dan log, perbaikan config atau dependency, coba lagi, dan semuanya tercatat sebagai operasi. Data tidak pernah direset otomatis. Fase ini selesai kalau pengguna bisa sampai ke akar masalah dan pulih tanpa menebak, dan seluruh jalur itu bekerja dari GUI dan CLI.

### 33. Crash loop & error start yang aman · in-progress

Hasil JOURNEY-05 di GUI dan CLI: start yang gagal tampil dengan pesan aman, recovery hint, dan tautan ke log dan doctor, plus backoff dan retry budget dari spec supervisor (fitur 10) yang terlihat oleh pengguna.

**Done when:** provider yang terus crash berhenti di retry budget (usulan 5 kali dalam 5 menit) tanpa busy loop atau child ganda; GUI dan CLI menampilkan backoff dan error terakhir; tiap error membawa correlation ID dan memakai envelope dari fitur 6; banner gagal di tab Summary dan inspector mengikuti mockup UI: judul yang menyebut sebabnya (crash loop atau port dipakai), kode error, Show Logs, Retry, dan saran port bebas yang menjalankan alur ganti port dari fitur 29, bukan mengubah port tanpa sepengetahuan pengguna.

**Trace:** AC-068, JOURNEY-05, NFR-011, VAL-020 (tampilan; taksonomi di fitur 6), PRD bagian 19.4 dan 35.2

**Pointer:** spec [0033](../specs/0033-crash-loop-error-start/index.md) · code in `apps/macos/Packages/StacklyUI/Sources/StacklyUI/InstanceDetail/Failure/`, `apps/cli/src/commands/instance/next_steps.rs`

- [ ] Build it: `/develop crash loop & error start yang aman` · spec 0033, cross check (Sonnet) applied, accepted 2026-11-10
  - [ ] Visible daemon: `StageName::BACKOFF` set in `wait_to_respawn` after `show_pause`, `FailureFacts` in the actor state and `InstanceStatus` filling `lastFailure.at`, `exitCode`, and `signal`, `suggestedPort` on the endpoint errors of a start or restart (the `endpoints` stage, and the early exit with one registry read), the static supervisor hints, fixtures with `codegen --check` clean and no protocol bump, then socket tests for both crash loop paths, a stop or cancel during a pause, a daemon restart, ports, nothing repaired on its own, and the `leakToken` credential (spec 0033 AC-1 to AC-5, AC-14, AC-15; build steps 1 and 2; needs features 10, 27, 29 on main; after build step 4 of feature 35 leaves crates/core and apps/daemon)
  - [ ] CLI: the `operation: <uuid>` line in every error block, `next_steps.rs` hints on a failed `start` and `restart`, the related operation rule, `Last operation` and `Next steps:` in `inspect` and `get` plus `lastOperation` in JSON, the backoff line during `--wait`, new and updated goldens (spec 0033 AC-6 to AC-8, AC-15, AC-16; build step 3; can start on a scripted daemon, closes after milestone 1; the doctor hint stays provisional until feature 34)
  - [x] GUI models: `FailureBannerContent` with the related operation rule and the cause mapping, `InstanceFailureModel` (read, events, offline), the banner copy, `InstanceEdit.editPorts` and `EndpointsModel` with `prefill`, the I7 text "Restart {attempts} of {budget}", spec 0068 fixture cases F1 to F7, model tests (spec 0033 AC-9 to AC-13, AC-16; build step 4; runs on fixtures, in parallel with milestone 2) · verify PASS in chk-f33-m3 (merge `a18a5493`), fixture
  - [x] GUI banner: `InstanceFailureBanner` in the Summary tab and the inspector, Use Port into the Ports sheet, Show Logs, Show in Operations, Diagnose…, snapshots in `InstanceDetailSnapshotTests` and `InstancesSnapshotTests` (spec 0033 AC-9 to AC-13, AC-16, AC-17; build step 5; after milestone 3; final link targets after features 34 and 35, provisional before) · verify PASS in chk-f33-m3 (merge `a18a5493`), 18 of 18 automated steps; the manual GUI steps are in `coordination/05-saat-instance-gagal.manual.md`
  - [ ] Evidence on this Mac: native PostgreSQL 18 with its fixed port held by an outside listener, and the fake crash loop evidence run with the real constants under `cargo xtask evidence run` (spec 0033 AC-1, AC-4, AC-14; build step 6; after milestones 1, 2, and 4; `BLOCKED` without the runtime artifact). The manual GUI session of build step 7 goes to the manual file.
- [ ] Verify it: `/check verify crash loop & error start yang aman`
- [ ] Test it: `/test crash loop & error start yang aman`

### 34. Doctor & diagnostic bundle · in-progress

`stackly doctor` dan halaman Diagnostics yang memeriksa izin, runtime atau dependency yang hilang, konflik endpoint, data lock, ruang disk, registry, background agent, protocol mismatch, dan config drift, plus paket diagnosis yang disanitasi.

**Done when:** PID record yang menunjuk proses lain dilaporkan beserta bukti tanpa menghapus data atau menghentikan proses itu; bundle diagnosis bisa dipreview sebelum export dan tidak memuat secret, baik lewat GUI maupun `stackly diagnostics export`; diagnosis tidak pernah menghapus lock secara otomatis; layar Diagnostics mengikuti mockup UI: tiap hasil doctor punya tingkat (Error, Warning, OK), penjelasan, dan aksi perbaikan hanya bila aman, plus daftar isi bundle sebelum export.

**Trace:** REQ-019, REQ-028, PAGE-13, NFR-016, AC-019, PRD bagian 35.3

**Pointer:** spec [0034](../specs/0034-doctor-diagnostic-bundle/index.md)

- [x] Design it (spec): `/architect doctor & diagnostic bundle` · spec 0034, cross check (Sonnet) applied, accepted 2026-11-10
- [ ] Build it: `/develop doctor & diagnostic bundle`
  - [ ] Doctor contract and daemon checks: `doctor.run` DTOs and the protocol minor, `ListenerLookup`, `quick_check` and `CODED_COLUMNS` scan, the 13 read only checks with passive identity evidence, and daemon integration tests with a real `sleep` process (spec 0034 AC-1 to AC-9, AC-22; build steps 1 to 5; after build step 4 of feature 35 is merged, since both touch crates/protocol, crates/core, and apps/daemon; steps 2 and 3 can run as parallel sessions now)
  - [ ] Provider remedies and the daemon start failure record: `remedies` manifest block, two PostgreSQL entries, `daemon-start-failure.json` written by `main` (spec 0034 AC-9, AC-10, AC-11; build steps 6 to 8; needs milestone 1)
  - [ ] Diagnostic bundle preview and export: staging, Redactor and `~`, limits, exact copy export, sentinel suite (spec 0034 AC-12, AC-13, AC-14, AC-22; build steps 9 and 10; needs milestone 1; step 9 amends the step 1 protocol minor while it is not on main)
  - [ ] CLI `stackly doctor` and `stackly diagnostics preview|export` with client checks, exit codes, and goldens (spec 0034 AC-11, AC-14, AC-15, AC-16, AC-18; build steps 11 and 12; step 11 needs milestone 1, step 12 needs milestone 3; can run in parallel with the GUI milestone)
  - [ ] GUI Diagnostics: doctor section, finding sheet, safe actions, focus, badge, CapacityModel, bundle sheet, spec 0068 fixtures, then manual A (real launchd) and manual B (GUI pass) recorded BLOCKED until run (spec 0034 AC-11, AC-17 to AC-22; build steps 13 to 16; step 13 needs milestone 1, step 14 needs milestone 3, `show_operation` after feature 35 `openOperation`)
- [ ] Verify it: `/check verify doctor & diagnostic bundle`
- [ ] Test it: `/test doctor & diagnostic bundle`

### 35. Operations, activity & audit · in-progress

Halaman Operations dan tab Activity: stage, progress, cancel, error, retry atau resume bila aman, serta audit lokal (actor, operation ID, instance, aksi, config revision, hasil) tanpa secret.

**Done when:** create, start, dan ganti port tercatat dengan correlation ID dan hasil tanpa credential; cancel di langkah yang tidak bisa dibalik menampilkan continuing safe shutdown atau needs recovery; resume hanya ditawarkan untuk operasi yang menyatakan bisa dilanjutkan; retention audit mengikuti setting; layar Operations dan tab Activity mengikuti mockup UI, termasuk tahap yang terlihat dan Cancel yang nonaktif di langkah yang tidak bisa dibatalkan dengan aman.

**Trace:** REQ-045, REQ-054, PAGE-12, AC-045, AC-054, JOURNEY-05

**Pointer:** spec [0035](../specs/0035-operations-activity-audit/index.md)

- [x] Design it (spec): `/architect operations, activity & audit` · spec 0035, cross check (Sonnet) applied, accepted 2026-11-10
- [ ] Build it: `/develop operations, activity & audit`
  - [ ] Audit in the daemon through the CLI: audit fields and migration, `CancelAvailability`, engine audit rows on every path with `OperationTrigger` on cancel, resume, and abandon, `set_audit_note` for create, start, restart, reload, config, ports, drift, and rename, `duplicateOf`, `OperationSummary.cancel`, `audit.list`, the audit retention setting with the prune wake, protocol minor bump and codegen, socket tests with the fixture credential and the staging then commit cancel, then `stackly audit list`, the cancel texts of `stackly operation`, and `audit-retention-days` (spec 0035 AC-1 to AC-9, AC-16 to AC-18; build steps 1 to 5; needs features 7, 11, 29, and 32 on main)
  - [ ] Operations page: `auditList` in `StacklyClient`, `StageDefinition.also` and the stage definitions, `StatusAppearance.operation` with `continuing`, `OperationHistory`, `OperationsPage` with inspector, Cancel, Retry, Resume, and Abandon, `ShellNavigator.openOperation`, the Overview rows and O1 text (spec 0035 AC-9 to AC-14, AC-17; build steps 6 and 7; after build step 4; can run in parallel with build step 5)
  - [ ] Activity tab and setting: `InstanceTab.activity` with its model and texts, the Duplicate sheet sending `duplicateOf`, the Activity group in the Storage tab of Settings (spec 0035 AC-15, AC-18, AC-20; build step 8; after the navigator part of build step 7)
  - [ ] Views and evidence: `FixtureOperations` and `FixtureActivity` snapshots in light and dark, then the manual GUI session on this Mac with the real daemon and the fake provider, which needs your eyes and hands (spec 0035 AC-11, AC-13, AC-15, AC-19, AC-20; build steps 9 and 10; after milestones 2 and 3)
- [ ] Verify it: `/check verify operations, activity & audit`
- [ ] Test it: `/test operations, activity & audit`
