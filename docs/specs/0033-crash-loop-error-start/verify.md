# Verify: Crash loop dan error start yang aman · spec 0033 · updated 2026-11-10

_Langkah diturunkan dari acceptance criteria spec 0033. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27 dan Xcode 27. Test Swift integrasi butuh `CARGO_PROFILE_DEV_DEBUG=0 cargo build -p stacklyd -p stackly-fake-server` dan `cargo build -p stacklyd --example skeleton_seed` lebih dulu, kalau tidak ia berakhir `BLOCKED`._

## Milestone 3: GUI models

Model GUI dibangun di atas fixture dan klien palsu: daemon baru mengisi `lastFailure.at`, `exitCode`, `signal`, dan `suggestedPort` di milestone 1, jadi langkah di bawah memakai kasus fixture `FixtureFailureBanner` (spec 0068) dan skrip `operation.list`, bukan daemon nyata.

### UI / manual

- [ ] Tidak ada langkah manual di milestone ini: model tidak punya view. Sesi GUI ada di Milestone 4 dan langkah 7 Build plan.

### Commands

- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter FailureBannerContentTests` → semua test lulus: satu kasus per aturan F1 sampai F7 (`restarting`, `initFailed`, `crashLoop`, `portInUse`, `portWithoutOperation`, `portConflict`, `portUnavailable`, `portsChanged`, `dataInUse`, `failed`, `startRefused`) dengan judul, isi, hint, baris kode "{code} · operation {uuid}", excerpt, `PortFix`, dan urutan tombol dari *Copy banner* → AC-9, AC-10, AC-11, AC-12
- [ ] Di suite yang sama: `initializationComesBeforeTheCause`, `aRecoveringInstanceNeverReadsAFailedOperation`, dan `instancesWithTheBannersOfSpec0022GetNone` → urutan aturan F1 sampai F7, dan instance `unknown` atau dengan blocker I2 sampai I4 tidak mendapat banner kecuali lifecycle `failed` (seperti I1) → AC-11
- [ ] Di suite yang sama: `newerOperationsHideTheBannerOfAStoppedInstanceExceptRename`, `anOperationOlderThanTheFailureIsNotItsError`, `withoutATimeTheCodeRowDecides`, `codesTheTableDoesNotListShareOneRow`, `aSuccessfulPortChangeHidesThePortButtonAndAsksForRetry` → aturan *Operasi terkait*: rename dan apply config dilewati; stop, start, ganti port, atau start yang dibatalkan yang lebih baru menyembunyikan F7; operasi yang lebih tua dari `lastFailure.at` tidak dipakai; tanpa `at` (daemon restart) hanya kode di baris *Pemetaan sebab* yang sama yang dipakai; `change_endpoints` sukses memberi kalimat "Its ports changed. Retry to start with them." tanpa tombol port → AC-10, AC-11
- [ ] Di suite yang sama: `theTitleStaysWhenTheOperationArrives` → untuk setiap kasus `failed` atau `recovering`, banner dari `summary` saja dan banner lengkap punya judul dan aturan yang sama; banner `summary` saja tidak punya operation ID dan tidak punya Use Port → AC-9, AC-10
- [ ] Di suite yang sama: `theCountdownRoundsUpAndNeverGoesBelowZero`, `theWindowIsInWholeMinutesRoundedUp`, `eachCodeMapsToTheRowOfItsColumn`, `aSuggestionNeedsTheEndpointAndAPort`, `aPortThatIsGoneSaysAnotherInstance` → `{s}` dari `nextAttemptAt` dikurangi `now` dibulatkan ke atas, minimal 0; `{window}` "5 minutes" atau "1 minute"; kedua kolom *Pemetaan sebab*; `suggestedPort` hanya dengan nama endpoint; `{otherName}` dari nama instance atau "another instance" → AC-10, AC-12
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter InstanceFailureModelTests` → semua test lulus: satu `operation.list { resource: instance, limit: 10 }` saat dibuka; bacaan pertama yang gagal atau offline memberi banner dari `summary` saja; bacaan sesudahnya yang gagal menyimpan hasil terakhir; `revision` atau `statusSeq` yang berubah membaca ulang (tidak untuk instance `running`); operasi final baru di `OperationCenter` membaca ulang, operasi yang sudah dipegang atau milik instance lain tidak; entri center yang lebih baru (stop yang berjalan) langsung menyembunyikan F7; registry `InstanceDetailModels` menyimpan satu model per instance → AC-10, AC-11
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter ConfigurationSheetsTests` → `aPrefilledPortIsJudgedAtOnce` (port saran langsung terisi dan verdict dry run berjalan tanpa jeda debounce, tidak ada `instance.change_endpoints` sebelum Change Ports, bacaan ulang tidak mengisi lagi), `aStalePrefillIsRefusedByTheVerdict` (saran basi ditolak verdict sheet yang memberi saran barunya), `aPrefillForNoEndpointOfTheSheetIsIgnored`, dan `thePortButtonOfABannerOpensTheSheetFilledIn` ("Use Port {n}" membuka `.editPorts(source, prefill:)`, "Change Port…" membuka tanpa isian, identitas sheet tetap `ports:{id}`) → AC-10
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter AttentionRulesTests` → I7 kini "It crashed. Restart {attempts} of {budget} at {time}." dengan syarat `attempts` ≤ `budget`: respawn pertama "Restart 1 of 5", respawn kelima "Restart 5 of 5", crash keenam kalimat umum (usulan spec 0022 yang diterima) → AC-12
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI` → seluruh suite lulus, tanpa perubahan snapshot di milestone ini → AC-17 (tidak berubah)
- [ ] `xcrun swift-format lint --strict --configuration .swift-format` pada file Swift yang diubah → bersih
- [ ] `cargo xtask build-app` → build Stackly berhasil

### Acceptance-criteria coverage

- AC-9 · isi banner crash loop (judul, isi, blok kode dengan operation ID, Retry utama) di `FailureBannerContent`; view dan teks yang bisa dipilih menunggu Milestone 4
- AC-10 · sebab port, Use Port atau Change Port, prefill sheet Ports dengan verdict langsung, saran basi ditolak sheet, kalimat setelah ganti port; tombol yang membuka sheet di banner menunggu Milestone 4
- AC-11 · aturan F6 dan F7 dan *Operasi terkait* (operasi lebih baru menyembunyikan, rename tidak); tampilan menunggu Milestone 4
- AC-12 · isi F1 dengan hitung mundur dan I7 "Restart {attempts} of {budget}"; banner inspector menunggu Milestone 4
- AC-13 · daftar tombol Retry per aturan; perilaku Retry (key baru, alasan nonaktif, "Starting…", kartu gagal) menunggu Milestone 4
- AC-16 · aksi Show Logs dan Diagnose… ada di daftar tombol; navigasi menunggu Milestone 4
- AC-17 · kasus fixture F1 sampai F7 (`FixtureFailureBanner`, perluasan `FixtureStartConflict`) dan `operation.list` di `PageRig`; snapshot menunggu Milestone 4
- AC-1 sampai AC-8, AC-14, AC-15 · milik Milestone 1, 2, dan 5
