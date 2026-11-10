# 0035. Rationale

[Kembali ke spec](index.md)

## Context

Stackly menjalankan setiap mutasi sebagai operasi di engine spec 0007: operasi dicatat lebih dulu, dikerjakan di bawah lock, dan ditutup dengan satu commit akhir. Engine sudah tahu siapa memicu operasi (`trigger` di journal), langkah mana yang tidak bisa dibalik, dan kenapa operasi gagal atau perlu pemulihan. Tetapi jejak itu belum bisa dilihat pengguna: halaman Operations di app masih `UnbuiltPage`, tab Activity belum ada, dan tabel `audit_events` hanya diisi aksi secret (spec 0011). Operasi berstate akhir juga di prune setelah 30 hari, jadi journal bukan riwayat jangka panjang.

PRD meminta audit lokal yang mencatat actor, operation ID, instance, waktu, aksi, config revision, dan hasil tanpa secret (REQ-045), dengan bukti bahwa create, start, ganti port, dan purge punya correlation ID dan hasil tanpa credential (AC-045). PRD juga meminta operasi yang menunjukkan tahap, progress, kemampuan cancel, dan aksi pemulihan, dengan cancel di langkah tidak bisa dibalik yang tidak pernah melapor sukses palsu (REQ-054, AC-054), serta halaman Operations dan Activity (PAGE-12) sebagai bagian akhir JOURNEY-05 ("operation record").

Ada beberapa batas yang mengikat. `EventKind` tertutup, jadi event baru berarti bump mayor protokol. Registry menulis lewat satu thread, sehingga baca panjang menunda tulis (spec 0005). Baris audit harus append only dan tidak boleh memuat nilai secret (spec 0011, BR-018). Scope meminta retensi audit mengikuti setting, dan PRD 35.2 mengusulkan 30 hari atau batas ukuran. Fitur 33 (banner gagal) dan 34 (doctor) ditulis paralel dan bergantung pada navigasi ke operasi. Interaction policy proyek menolak dukungan aksesibilitas dan keyboard kustom.

Tanpa keputusan ini, setiap fitur harus ingat menulis audit sendiri (dan pasti ada yang lupa), GUI dan CLI masing masing menebak kapan Cancel aman, dan pengguna yang mengalami start gagal tidak punya tempat untuk melihat operasi itu dan mengulangnya.

## Options considered

### Keputusan K1: siapa menulis baris audit operasi

**Option 1: engine, di batch commit state yang sama** (dipilih). Helper engine menambah `InsertAuditEvent` ke setiap batch yang membawa operasi ke state akhir atau `needs_recovery`.

- Pros: tidak ada operasi yang bisa berakhir tanpa baris; atomik dengan state; fitur baru gratis.
- Cons: engine membangun detail generik saja; detail fitur butuh jalur catatan (K5).

**Option 2: setiap fitur menulis barisnya sendiri.**

- Pros: detail kaya per fitur.
- Cons: mudah lupa, jalur gagal dan pemulihan tidak tertangkap (pekerjaan tidak berjalan saat `recover` menutup operasi), dan ada banyak tempat yang harus memanggil `Redactor`.

**Option 3: tanpa baris, Activity dibaca dari tabel `operations`.**

- Pros: tanpa data ganda.
- Cons: operasi di prune dengan aturan lain, aksi secret ada di tabel lain, retensi audit tidak bisa diatur terpisah, dan cancel atau abandon oleh client lain tidak tercatat sebagai aksi.

### Keputusan K4: di mana instance, jenis, config revision, dan client disimpan

**Option 1: empat kolom baru lewat `ALTER TABLE ADD COLUMN`** (dipilih).

- Pros: filter instance ber index, DTO bertipe, REQ-045 menyebut instance dan config revision sebagai field.
- Cons: satu migrasi dan fixture beku baru.

**Option 2: semuanya di `safe_details`.**

- Pros: tanpa migrasi.
- Cons: filter instance lewat `json_extract` tanpa index, dan DTO harus menebak tipe dari string.

### Keputusan K7: siapa menentukan apakah Cancel aman

**Option 1: daemon, field `OperationSummary.cancel`** (dipilih).

- Pros: satu fungsi domain yang tahu `interrupt_policy`; GUI dan CLI sama; minor saja.
- Cons: field tambahan di setiap summary dan fixture.

**Option 2: client menghitung dari `pointOfNoReturnAt`.**

- Pros: tanpa perubahan kontrak.
- Cons: client tidak tahu jenis `CloseInterrupted`, jadi Cancel start yang sudah `spawn` akan salah dimatikan; tabel kebijakan jenis tergandakan di Swift dan CLI.

### Keputusan K8: cara klien membaca audit

**Option 1: `audit.list` berhalaman, tanpa event** (dipilih). Activity memuat ulang pada event operasi yang sudah ada.

- Pros: minor saja; pola cursor sama dengan `operation.list`.
- Cons: aksi secret dari client lain baru muncul saat tab dimuat ulang.

**Option 2: event `audit.appended`.**

- Pros: Activity langsung.
- Cons: `EventKind` tertutup, jadi bump mayor yang memutus client lama, untuk keuntungan kecil.

### Keputusan K9: retensi audit

**Option 1: setting 7, 30, atau 90 hari di `daemon_settings`, batas 100.000 baris tetap** (dipilih).

- Pros: memenuhi "mengikuti setting" dengan pilihan sedikit yang mudah diuji; batas baris menjaga ukuran registry.
- Cons: menambah satu baris Settings dan satu field kontrak settings.

**Option 2: konstanta 30 hari yang hanya ditampilkan (seperti retensi log).**

- Pros: paling sederhana.
- Cons: tidak memenuhi scope ("retention audit mengikuti setting") dan PAGE-14 (retention di Settings).

**Option 3: hari bebas plus batas ukuran yang bisa diatur.**

- Pros: paling fleksibel.
- Cons: validasi dan copy lebih banyak, ukuran tidak bermakna bagi pengguna.

## Rationale

Engine menulis baris audit karena hanya engine yang melihat setiap jalur akhir, termasuk yang tidak pernah menjalankan pekerjaan fitur (gagal di antrian, ditutup saat daemon start, ditutup shutdown). Menaruh baris di batch yang sama dengan state membuat jaminan AC-045 struktural, bukan disiplin per fitur, dan jaminan itu otomatis berlaku untuk purge dan restore yang belum dibangun. Detail khusus fitur (config revision, port, sumber duplicate) masuk lewat catatan di memori yang opsional, supaya engine tetap generik dan fitur yang lupa hanya kehilangan detail, bukan baris.

Ketersediaan cancel dihitung daemon karena aturan yang benar bergantung pada `interrupt_policy`: start yang sudah lewat `spawn` tetap bisa dibatalkan dengan aman (pekerjaan menghentikan proses), sedangkan install di `commit` tidak. Client yang menebak dari `pointOfNoReturnAt` akan salah untuk jenis instance, dan dua client yang menebak dengan cara berbeda melanggar tujuan REQ-054. Field opsional dengan aturan cadangan menjaga client baru tetap benar terhadap daemon lama.

Pembacaan audit memakai satu method berhalaman tanpa event baru karena biaya bump mayor jauh lebih besar dari manfaat Activity yang langsung untuk aksi secret client lain; operasi, yang merupakan sebagian besar baris, sudah punya event. Retensi dibuat bisa diatur dengan tiga pilihan, sesuai scope, sambil mempertahankan batas baris spec 0005 yang melindungi registry dari pertumbuhan tanpa batas.

## Keputusan

User mendelegasikan setiap keputusan ke opsi yang direkomendasikan; semuanya dicatat di sini.

| No | Keputusan | Pilihan | Alasan singkat | Runner up |
| --- | --- | --- | --- | --- |
| K1 | Penulis baris audit operasi | engine, batch commit yang sama | jaminan struktural, mencakup jalur pemulihan | per fitur |
| K2 | Operasi mana yang diaudit | semua jenis, semua origin, termasuk `NoOp` dengan batch | Follow-up 0007 menyebut destruktif, tetapi AC-045 menyebut start dan ganti port; satu aturan lebih sederhana dan volumenya kecil | hanya origin user dan jenis destruktif |
| K3 | Nama action dan hasil | `operation.finished`, `operation.needs_recovery`, `operation.cancel`, `operation.resume`, `operation.abandon`; jenis di kolom `operation_kind`; `AuditResult` ditambah `cancelled` dan `needs_recovery` | action menyebut apa yang terjadi, jenis bisa difilter, kolom `result` di SQL sudah set terbuka | action per jenis (`operation.start_instance`) |
| K4 | Penyimpanan instance, jenis, revision, client | kolom baru | index dan DTO bertipe | `safe_details` |
| K5 | Detail fitur | `OperationContext::set_audit_note`, di memori | engine generik; tanpa perubahan journal | kolom journal baru |
| K6 | Aksi pemulihan | baris dengan trigger pengirim; tidak ada baris untuk jawaban yang tidak mengubah apa pun | cancel oleh client lain penting dicatat; penolakan sudah ada di log | catat juga penolakan |
| K7 | Ketersediaan cancel | `OperationSummary.cancel` dari daemon | `interrupt_policy` hanya diketahui domain | dihitung client |
| K8 | Baca audit | `audit.list`, tanpa `EventKind` baru | hindari bump mayor | event `audit.appended` |
| K9 | Retensi | 7, 30, 90 hari; 100.000 baris tetap; prune dibangunkan saat setting berubah | scope dan PRD 35.2 | konstanta read only |
| K10 | Data halaman Operations | `OperationCenter` (aktif dan event) plus `OperationHistory` (riwayat berhalaman) | `OperationCenter` sengaja hanya aktif dan 5 menit terakhir (spec 0013) | memperluas `OperationCenter` |
| K11 | Retry di halaman Operations | hanya start, stop, restart, reload yang gagal dengan instance yang masih ada; operasi baru dengan key baru | input jenis lain (create, install) butuh form atau secret; sama dengan banner spec 0033 | Retry untuk semua `failure.retryable` |
| K12 | Navigasi ke operasi | `ShellNavigator.openOperation(_:)` dengan `OperationRequest` | pola sama dengan `openInstance`; tanpa mengubah `ShellDestination` yang dipakai banyak `switch` | `.operations(select:)` |
| K13 | Definisi tahap | grup dengan `StageDefinition.also` | tahap mentah start ada sebelas, terlalu banyak untuk bar; `backoff` tidak membuat bar mundur | satu definisi per kunci |
| K14 | Activity | `Table` native, refresh saat tampil, pada event operasi instance itu, dan pada `completedRounds` | tanpa event baru; kontrol native sesuai interaction policy | daftar kartu seperti mockup |
| K15 | "Created from" (Follow-up 0024) | field opsional `duplicateOf` di `instance.create` | create tetap satu pintu (spec 0024); sumber dicatat saat begin | baca dari journal create (tidak memuat sumber) |
| K16 | Batas dengan spec 0034 | doctor dan bundle tidak diaudit dan bukan operasi; audit tidak ikut bundle; export audit lewat `stackly audit list --json` | disepakati dengan `arc-f34-spec` (2026-11-10) | `audit.json` di bundle |

Keputusan kecil lain yang diambil saat menulis:

- Baris audit operasi gagal dibangun tidak pernah menahan commit (beda dengan reveal secret spec 0011 yang gagal tertutup), karena di sini baris ada di batch yang sama: commit yang gagal sudah menggagalkan keduanya, dan yang tersisa hanya bug batas `SafeDetails` yang lebih baik dicatat di log daripada membuat operasi tidak bisa ditutup.
- Batas halaman `audit.list` 200 baris dan 900 KiB, karena satu baris bisa sampai sekitar 12 KB (20 detail × 512 byte) dan frame kontrol 1 MiB.
- Cancel tidak memakai dialog konfirmasi: sebelum titik tak bisa dibalik ia membersihkan, dan untuk jenis instance ia berhenti aman. Abandon memakai dialog destruktif dengan Cancel sebagai default (spec 0012).
- Retensi operasi tetap 30 hari (di luar scope); akibatnya dicatat di Consequences.
- `peerUid` tidak ditulis di baris operasi: daemon hanya menerima uid yang sama (spec 0006), dan journal tidak menyimpannya.
- Footer halaman mengadaptasi copy mockup tanpa angka 80%, yang hanya contoh.
- Kalimat CLI lama untuk `continuing_past_point_of_no_return` ("the operation stops after it") keliru terhadap engine, yang menyelesaikan seluruh operasi; spec ini menggantinya.

## Koordinasi dengan sesi paralel

- `arc-f33-spec` (2026-11-10): Retry banner adalah `instance.start` baru; banner membaca `operation.list` tanpa method baru; navigasi memakai `openOperation`; tahap `backoff` masuk definisi `start_instance` sebagai alias "Waiting to restart" di Launch.
- `arc-f34-spec` (2026-11-10): K16; doctor membuka operasi lewat `openOperation` dan menyarankan `stackly operation inspect <id>` di CLI.
- Pesan awal ke kedua sesi menyebut tiga nilai `cancel`; spec ini memakai lima (`available`, `past_point_of_no_return`, `requested`, `continuing`, `none`). Keduanya menyatakan tidak memakai field itu, jadi tidak ada yang berubah di pihak mereka.

## Cross check

Dijalankan 2026-11-10 dengan model lain (read only, Sonnet). Lima belas temuan; user mendelegasikan keputusan, jadi rekomendasi berikut diterapkan langsung:

| No | Temuan | Tindakan |
| --- | --- | --- |
| 1 | Cancel pada operasi `running` mungkin tidak punya commit untuk menumpang baris | dicek di kode: `scheduler::cancel` menulis `mark_cancel_requested` lewat `inner.write`; kalimatnya ditambah di *Baris audit operasi* |
| 2 | Operasi latar bisa membanjiri batas 100.000 baris | dipertahankan (K2); respawn dibatasi retry budget spec 0033, dan pembuangan baris tertua dicatat di Consequences |
| 3 | Sumber params Retry dan Retry pada baris lama | params bawaan tombol toolbar; Retry hanya untuk operasi lifecycle terbaru instance itu dan bila `InstanceLifecycleActions` mengizinkan (AC-10) |
| 4 | Siapa mengalokasikan instance ID create | instance ID dialokasikan sebelum `begin` (spec 0020); operasi berresource manager tidak masuk Activity (dicatat) |
| 5 | Mekanisme wake prune dan penghapusan tanpa konfirmasi | `DaemonCore::wake_prune` (`Notify`) setelah commit; dialog konfirmasi saat nilai diperpendek (AC-6, AC-20) |
| 6 | Urutan dua baris dalam satu batch tidak pasti | satu `now` per batch, `occurred_at` naik 1 nanodetik per baris |
| 7 | Activity tidak dimuat ulang pada cancel dan resume | muat ulang pada setiap `operation.state_changed` instance itu, dikumpulkan 500 ms (AC-15) |
| 8 | Cara mendeteksi daemon lama | `request.unknown_method` (pola `ResourceStore`) untuk `audit.list`, field `auditRetention` yang tidak ada untuk Settings |
| 9 | Templat Activity tanpa detail dan baris abandon | baris gagal dan dibatalkan memakai templat kata kerja, `abandoned` punya teks sendiri, templat sukses punya teks cadangan; `config_revision` hanya revision yang sudah ada di registry |
| 10 | Aturan tepi halaman | `before` eksklusif, baca `limit + 1`, `next` null di halaman terakhir (AC-5) |
| 11 | Catatan hilang untuk operasi yang ditutup `recover` | dipertahankan (K5): kehilangan detail, bukan baris; AC-2 menyebut batas ini |
| 12 | Aturan cadangan cancel salah untuk jenis `CloseInterrupted` | cadangan diganti: `queued` atau `running` selalu `available`, engine menjawab disposisi yang benar; test domain untuk setiap jenis (AC-8); restart memang `CloseInterrupted` |
| 13 | Penggabungan event ke riwayat berhalaman | sisip hanya bila tidak lebih tua dari baris tertua yang termuat |
| 14 | Downgrade dan nomor migrasi | daemon lama masuk recovery `schema_newer_than_app`; nomor migrasi ditetapkan lead |
| 15 | Batas 20 kunci dan `omitted` | batas termasuk `omitted`; urutan redaksi lalu potong dipertegas |

## Bahan yang dibaca

Spec ini tidak memuat bagian References (pilihan default tanpa referensi). Bahan proyek yang dibaca: spec 0005, 0007, 0011, 0012, 0013, 0022, 0024, 0029, 0032, 0067, mockup `docs/mockups/Stackly.dc.html`, dan kode di `crates/core/src/operation/`, `crates/core/src/secret/service/audit.rs`, `apps/macos/Packages/StacklyUI/Sources/StacklyUI/Operations/`.
