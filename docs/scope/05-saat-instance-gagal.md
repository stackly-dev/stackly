# Scope 05 · Saat instance gagal

[Kembali ke index](index.md)

Journey PRD JOURNEY-05: start gagal, pesan yang aman, doctor dan log, perbaikan config atau dependency, coba lagi, dan semuanya tercatat sebagai operasi. Data tidak pernah direset otomatis. Fase ini selesai kalau pengguna bisa sampai ke akar masalah dan pulih tanpa menebak, dan seluruh jalur itu bekerja dari GUI dan CLI.

### 33. Crash loop & error start yang aman

Hasil JOURNEY-05 di GUI dan CLI: start yang gagal tampil dengan pesan aman, recovery hint, dan tautan ke log dan doctor, plus backoff dan retry budget dari spec supervisor (fitur 10) yang terlihat oleh pengguna.

**Done when:** provider yang terus crash berhenti di retry budget (usulan 5 kali dalam 5 menit) tanpa busy loop atau child ganda; GUI dan CLI menampilkan backoff dan error terakhir; tiap error membawa correlation ID dan memakai envelope dari fitur 6; banner gagal di tab Summary dan inspector mengikuti mockup UI: judul yang menyebut sebabnya (crash loop atau port dipakai), kode error, Show Logs, Retry, dan saran port bebas yang menjalankan alur ganti port dari fitur 29, bukan mengubah port tanpa sepengetahuan pengguna.

**Trace:** AC-068, JOURNEY-05, NFR-011, VAL-020 (tampilan; taksonomi di fitur 6), PRD bagian 19.4 dan 35.2

- [ ] Build it: `/develop crash loop & error start yang aman`

### 34. Doctor & diagnostic bundle · needs a decision

`stackly doctor` dan halaman Diagnostics yang memeriksa izin, runtime atau dependency yang hilang, konflik endpoint, data lock, ruang disk, registry, background agent, protocol mismatch, dan config drift, plus paket diagnosis yang disanitasi.

**Done when:** PID record yang menunjuk proses lain dilaporkan beserta bukti tanpa menghapus data atau menghentikan proses itu; bundle diagnosis bisa dipreview sebelum export dan tidak memuat secret, baik lewat GUI maupun `stackly diagnostics export`; diagnosis tidak pernah menghapus lock secara otomatis; layar Diagnostics mengikuti mockup UI: tiap hasil doctor punya tingkat (Error, Warning, OK), penjelasan, dan aksi perbaikan hanya bila aman, plus daftar isi bundle sebelum export.

**Trace:** REQ-019, REQ-028, PAGE-13, NFR-016, AC-019, PRD bagian 35.3

- [ ] Design it (spec): `/architect doctor & diagnostic bundle`

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
