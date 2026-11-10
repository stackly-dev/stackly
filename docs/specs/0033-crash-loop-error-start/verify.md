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

## Milestone 4: GUI banner

`InstanceFailureBanner` dipasang di tab Summary (lewat `InstanceDetailBanner`, menggantikan I1 dan I7) dan di inspector Instances (versi ringkas). Tautan Show in Operations dan Diagnose… masih memakai tujuan provisional (`// provisional: F35` dan `// provisional: F34`): halaman Operations dan Diagnostics tanpa fokus, sampai `ShellNavigator.openOperation(_:)` dan `openDiagnostics(focus:)` ada di `main`.

### UI / manual

Langkah ini butuh app Debug di atas daemon nyata dengan fitur milestone 1 (fakta kegagalan dan `suggestedPort` terisi); sampai milestone 1 masuk, banner tampil tanpa waktu, exit code, dan saran port. Lead memindahkan langkah ini ke file manual.

- [ ] Buat instance fake yang crash sebelum ready (`crashAfterMs` kecil), tekan Start, buka tab Summary → banner `caution` "Restarting after a crash" dengan "Restart {n} of 5 in {s} s." yang menghitung mundur tiap detik, cuplikan excerpt, dan Show Logs → AC-12
- [ ] Tunggu sampai budget habis → banner `critical` "Crash loop. Automatic restarts paused.", isi "It crashed 6 times in 5 minutes, so Stackly stopped restarting it and is waiting for you." plus hint, blok kode "supervisor.retry_budget_exhausted · operation {uuid}" lalu excerpt, tombol Retry (utama), Show Logs, Diagnose…, dan tautan Show in Operations → AC-9
- [ ] Seret pilih teks di blok kode lalu Copy → teksnya bisa dipilih dan disalin → AC-9
- [ ] Tekan Show Logs → tab Logs instance itu terbuka → AC-16
- [ ] Tekan Show in Operations → halaman Operations terbuka (provisional: tanpa fokus sampai fitur 35) → AC-16
- [ ] Tekan Diagnose… → halaman Diagnostics terbuka (provisional: tanpa fokus sampai fitur 34) → AC-16
- [ ] Pilih instance itu di tabel Instances dengan inspector terbuka → banner ringkas: judul crash loop dan excerpt, tanpa operation ID dan tanpa tombol lain → AC-12
- [ ] Tekan Retry di banner → satu `instance.start` baru, tombol menjadi "Starting…" dengan spinner, lalu banner restart muncul lagi (budget baru) → AC-13
- [ ] Hentikan `stacklyd` yang kamu jalankan untuk sesi ini sampai jendela menunjukkan daemon tidak terhubung → Retry dan Use Port nonaktif dengan alasan dari banner daemon jendela; banner kegagalan tetap tampil dengan isi terakhir yang dimuat → AC-13
- [ ] Pegang port fixed instance PostgreSQL dengan `nc -l 127.0.0.1 <port>`, tekan Start → banner "Couldn’t start: port in use", isi "Port {port} of {endpoint} is held by another process." plus hint, tombol utama "Use Port {n}" → AC-10
- [ ] Tekan Use Port {n} → sheet Edit Ports terbuka dengan endpoint itu terisi {n} dan verdict dry run langsung tampil; port di registry belum berubah (`stackly instance inspect`) → AC-10
- [ ] Ketik port lain yang juga dipakai, atau biarkan `nc` kedua memegang {n} sebelum membuka sheet → verdict sheet menolak dan menawarkan saran barunya sendiri → AC-10
- [ ] Tekan Change Ports di sheet → port berubah; banner menyembunyikan tombol port dan menambah "Its ports changed. Retry to start with them."; tekan Retry → instance start di port baru → AC-10, AC-13
- [ ] Biarkan instance lain memegang port yang sama, tekan Start pada instance `stopped` → banner "Couldn’t start: port in use" dengan "Port {port} of {endpoint} belongs to {nama instance itu}." dan Use Port; lalu tekan Stop atau ubah nama instance → stop menyembunyikan banner, rename tidak → AC-10, AC-11
- [ ] Bandingkan dengan `stackly instance inspect <name>` untuk instance yang sama → kode, operation ID, dan port saran sama dengan banner → AC-9, AC-10
- [ ] Ulangi dengan app di mode gelap dan jendela sempit (lebar konten sekitar 640 pt) → tidak ada label tombol atau baris kode yang terpotong; tombol pindah ke dua baris bila tidak muat → AC-17

### Commands

- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter InstanceFailureBannerTests` → semua test lulus: Retry di banner adalah Retry utama (setiap klik satu `instance.start` dengan key baru, penolakan memberi kartu `critical`, label "Starting…" saat berjalan, alasan nonaktif dan gate offline tetap); Retry pada instance `stopped` adalah Start dengan judul "Retry"; urutan tombol Summary "Use Port 5439, Show Logs, Retry, Diagnose…" dan Use Port membuka `.editPorts(source, prefill: ["database": 5439])`; inspector hanya Use Port; tempat tanpa sheet Ports atau tab Logs tidak menampilkan tombol itu; Show Logs memanggil penutupnya, Diagnose… dan Show in Operations memindah navigator ke `.diagnostics` dan `.operations` (provisional) → AC-10, AC-13, AC-16
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter InstanceDetailSnapshotTests` → `detailOfEachFailureBanner` menulis 11 kasus `instance-detail-failure-*` (restarting, init-failed, crash-loop, port-in-use, port-without-operation, port-conflict, port-unavailable, ports-changed, data-in-use, failed, start-refused) dalam tiga varian light, dark, narrow; `instance-detail-failed` (crash loop tanpa operasi) dan `instance-detail-port-conflict` (Change Port… dari summary saja) diperbarui → AC-9, AC-10, AC-11, AC-12, AC-17
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI --filter InstancesSnapshotTests` → `instancesWithEachFailureBannerChosen` menulis 7 kasus `instances-failure-*` (F1 sampai F7) di inspector; `instances-failed-chosen` diperbarui → AC-12, AC-17
- [ ] `cargo xtask snapshot review` → `target/snapshot-review/index.html` menampilkan 9 PNG yang berubah dan 54 PNG baru; periksa banner di setiap varian: label tidak terpotong, blok kode terbungkus di lebar sempit → AC-17
- [ ] `swift test --package-path apps/macos/Packages/StacklyUI` → seluruh suite lulus → AC-17
- [ ] `xcrun swift-format lint --strict` pada file Swift yang diubah → bersih
- [ ] `cargo xtask build-app` → build Stackly berhasil

### Acceptance-criteria coverage

- AC-9 · banner crash loop di Summary dengan blok kode yang bisa dipilih, operation ID, dan Retry, Show Logs, Diagnose…; nilai `at` dan exit code dari daemon menunggu Milestone 1
- AC-10 · Use Port dan Change Port… ke sheet Ports, kalimat setelah ganti port; `suggestedPort` dari daemon menunggu Milestone 1, langkah manual menunggu sesi GUI (langkah 7)
- AC-11 · banner F6 dan F7 dan penyembunyiannya oleh operasi yang lebih baru
- AC-12 · banner restart dengan hitung mundur `TimelineView` dan banner ringkas di inspector
- AC-13 · Retry di banner adalah aksi utama spec 0022
- AC-16 · Show Logs ke tab Logs; Show in Operations dan Diagnose… provisional sampai fitur 35 dan 34
- AC-17 · snapshot F1 sampai F7, Use Port, Change Port, setelah ganti port, tanpa operasi, dan lebar sempit, dalam tiga varian spec 0068 (light, dark, narrow; varian aksesibilitas dihapus 2026-10-24)
- AC-1 sampai AC-8, AC-14, AC-15 · milik Milestone 1, 2, dan 5
