# Scope 12 · Siap rilis stabil

[Kembali ke index](index.md)

Bukan journey pengguna, tapi pekerjaan yang membuat baseline layak disebut stabil: review keamanan, uji kegagalan dan performa, bukti per acceptance criteria, dan runbook. Release gate G1 sampai G6 menunggu fase ini. Pilot internal (G7, usulan minimal 3 pengguna selama 10 hari kerja) dijalankan setelah fitur 66 selesai dan bukan fitur build.

### 63. Security review & fuzzing · needs a decision · GA

Review keamanan untuk archive parser, template engine, eksekusi proses native, purge dan adoption, framing IPC, credential, izin, update dan revocation, serta diagnostic export. Fuzz target untuk framing, manifest, archive, dan path sudah berjalan di Mac ini sejak fitur 6, 9, 18, 20, dan 54; di sini cakupannya diperluas dan ditutup.

**Done when:** tidak ada temuan critical atau high yang belum selesai di distribusi, IPC, eksekusi provider, atau operasi destruktif; residual risk di PRD bagian 23 tertulis di docs pengguna dan developer.

**Trace:** NFR-009, AC-061, AC-070, gate G4, PRD bagian 23 dan 32.1

- [ ] Design it (spec): `/architect security review & fuzzing`

### 64. Fault injection, recovery & soak · needs a decision

Harness untuk crash daemon dan provider, restart daemon terencana, daemon ganda, PID reuse, race port, disk penuh, download korup, operasi yang dibatalkan, jaringan mati, sleep dan wake, logout dan login ulang, dua user di satu Mac, serta benchmark dan soak pada hardware baseline yang tercatat.

**Done when:** skenario AC-046 dan skenario lintas fitur AC-055 sampai AC-080 punya test yang bisa dijalankan; harness dijalankan dengan tangan di Mac ini secara rutin dan untuk release candidate; target NFR-001 sampai NFR-007 diukur dan disimpan, termasuk outlier; process crash, OS restart, dan simulasi power loss dibedakan.

**Trace:** NFR-001 sampai NFR-008, NFR-013, AC-046, AC-055 sampai AC-080, OQ-011, PRD bagian 27 dan 32

- [ ] Design it (spec): `/architect fault injection, recovery & soak`

### 65. Quality gates & release report · needs a decision

Agregasi evidence yang sudah dikumpulkan sejak foundation (format laporan dari fitur 2, compatibility record dari fitur 18): traceability dari REQ ke AC ke test, dan release report yang menahan rilis kalau test wajib gagal atau `BLOCKED`.

**Done when:** satu restore test wajib yang `BLOCKED` membuat report menyatakan belum stable ready tanpa mengubah statusnya jadi `PASS`; versi di luar matrix tetap `UNVERIFIED`; laporan fixture dibedakan dari laporan integrasi runtime nyata; evidence pada commit selain release candidate dijalankan ulang, bukan diberi label ulang; evidence AC-027 mencatat hasil review interaksi desktop per journey kritis.

**Trace:** REQ-051, BR-025, AC-027, AC-051, NFR-012, gate G1 sampai G6, PRD bagian 28, 31, 32.2, 34, dan 36

- [ ] Design it (spec): `/architect quality gates & release report`

### 66. Runbook & dokumentasi pengguna

Runbook untuk kondisi di PRD bagian 35.3 (daemon tidak terhubung, konflik port, runtime hilang, start gagal, disk penuh, crash loop, backup atau restore gagal, update gagal, provider revoked, uninstall dan reinstall), plus getting started, troubleshooting, dan known limitations, termasuk batas disruption untuk restart daemon, logout dan login, serta restart OS.

**Done when:** tiap kondisi punya langkah aman yang tidak menghapus data atau registry; dokumentasi menjelaskan isolasi logis dengan jujur beserta batas dukungan per platform; support ownership tercatat (siapa menangani bug core, bug provider, incompatibility upstream, dan configuration issue) dan proses insiden untuk data loss atau security tersedia sebelum pilot.

**Trace:** gate G6, AC-061, PRD bagian 35.3, 36, dan 39

- [ ] Build it: `/develop runbook & dokumentasi pengguna`
