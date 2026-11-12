# Scope 06 · Menambah provider

[Kembali ke index](index.md)

Journey PRD JOURNEY-07 untuk kontributor: scaffold, tulis manifest dan template, validasi lokal, beri trust eksplisit, jalankan test kontrak dan integrasi, lalu kirim PR. Fase ini sengaja datang sebelum stack enam service, supaya lima provider resmi berikutnya dibangun lewat jalur dan harness yang sama dengan kontributor.

### 36. Provider SDK & test harness · needs a decision

Kebijakan kompatibilitas, deprecation, dan migrasi untuk schema `stackly/v1alpha1` yang dikunci fitur 9 (termasuk penambahan probe dan primitive baru secara additive), validator, template scaffold, dan test harness (`stackly provider validate`, `stackly provider test`) dengan fake server yang bisa delay ready, crash, dan spam log.

**Done when:** manifest dengan versi schema yang tidak didukung ditolak dengan penjelasan tanpa menjalankan lifecycle; provider fake server baru bisa install, create, start, health, dan tampil sebagai form di GUI tanpa perubahan Rust core atau SwiftUI; harness berjalan di test root ephemeral dan cleanup memeriksa ownership test run; harness menghasilkan report dari repository bersih.

**Trace:** REQ-030, AC-029 (akhir), AC-030, AC-034, PRD bagian 21, 27, dan 32.3

- [ ] Design it (spec): `/architect provider SDK & test harness`

### 37. Import provider lokal & trust · needs a decision · GA

Import provider dari direktori atau paket lokal secara eksplisit, preview metadata tanpa menjalankan apa pun, lalu approval yang terikat ke publisher, revision, digest, dan capability. Revocation memblokir install baru tanpa menghapus data.

**Done when:** provider lokal yang belum dipercaya bisa diperiksa, tapi executable dan init tidak berjalan; revision baru yang meminta izin atau executable berbeda menunggu approval ulang; revocation (dari catalog terpasang atau pencabutan approval) tidak menghapus runtime atau data, dan saat offline tidak menghalangi start runtime yang sudah terpasang serta tidak diklaim terkini; quarantine pada paket lokal ditangani kebijakan fitur 3 tanpa meminta pengguna mematikannya secara global; dialog izin menyatakan bahwa native executable berjalan dengan hak user; `stackly provider import` setara dengan GUI.

**Trace:** REQ-028, REQ-032, REQ-033, BR-020, VAL-018, PAGE-08, AC-032, AC-033, AC-061, AC-072, OQ-009, PRD bagian 21.4 dan 23

- [ ] Design it (spec): `/architect import provider lokal & trust`

### 38. Panduan kontributor & governance · needs a decision

Dokumen dan aturan supaya orang lain bisa menyumbang provider: CONTRIBUTING, schema docs, provider template, security policy, Code of Conduct, issue dan PR template, serta pilihan lisensi project.

**Done when:** kontributor yang mengikuti panduan dari repository bersih bisa menjalankan validator dan harness pada contoh provider; lisensi core sudah diputuskan dan tercatat; kanal security disclosure privat dan proses advisory tersedia.

**Trace:** REQ-034, AC-034, OQ-013, JOURNEY-07, PRD bagian 39

- [ ] Design it (spec): `/architect panduan kontributor & governance`
