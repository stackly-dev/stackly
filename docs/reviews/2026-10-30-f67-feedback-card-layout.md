# Review, main (uncommitted), 2026-10-30

**Reviewed by**: Sonnet 5.5 (penulis kode di model lain)
**Scope**: 3 file (FeedbackCard.swift, ShellFeedbackLayoutTests.swift baru, verify.md spec 0067), uncommitted
**Verdict**: Approve with nits

## Summary
Perbaikan menambah `.frame(minWidth: 200)` pada kolom teks `FeedbackCard` sebelum `fixedSize(horizontal: false, vertical: true)`, supaya split view tidak mengukur teks di lebar mendekati nol lalu menjadikan tingginya tinggi minimum jendela lewat inset bawah. Perubahan kecil, di lapisan yang tepat, dan tidak mengubah snapshot galeri. Regression test memakai jendela AppKit sungguhan dan menangkap bug aslinya; kelemahannya: tidak ada assertion bahwa kartu benar-benar tampil, dan tunggu waktu tetap. Tidak ada blocker atau major.

## Minor
### 🟡 Test bisa lulus tanpa kartu, `ShellFeedbackLayoutTests.swift:21-51`
**Problem**: Test tidak memastikan kartu terpasang dan masih tampil. Kalau `feedbackPresenter()` tidak menggambar apa pun (center tidak sampai ke kolom detail, kartu terhapus), `split.frame.height == 800` tetap lulus. Kartu `positive`/`informative`/`neutral` tanpa aksi bersifat sementara (`Feedback.isPersistent`); test hanya aman karena `SnapshotWindow.canBecomeKey == false` membuat `controlActiveState` tidak aktif dan hitung mundur tidak jalan, ketergantungan yang tidak tertulis.
**Why it matters**: Regression test yang bisa lulus secara hampa tidak menjaga AC-9/AC-11 setelah refactor presenter atau jendela.
**Suggested fix**: Tambah `#require(center.current != nil)` setelah settle dan cari NSView atau tombol Dismiss kartu (atau pastikan tinggi `hosting` memuat inset). Beri komentar bahwa hitung mundur sengaja tidak jalan karena jendela tidak aktif.

### 🟡 Tunggu 10 x 20 ms tetap, bukan sinyal settle, `ShellFeedbackLayoutTests.swift:41-44`
**Problem**: Layout dianggap selesai setelah 200 ms. AGENTS.md (apps/macos) meminta test tidak bergantung pada tenggat jam dinding yang bisa dikelaparan main actor. Di sini kegagalan timing cenderung menjadi lulus palsu (layout belum membengkak), bukan gagal palsu. Pola ini sama dengan `ShellBannerLayoutTests`, jadi konsisten, tetapi 14 jendela dikali 200 ms tetap tidak pasti.
**Why it matters**: Pada mesin yang sibuk (test paralel lain memegang main actor) bug yang sama bisa lolos tanpa terdeteksi.
**Suggested fix**: Ulangi sampai `frame` split stabil dua iterasi berturut-turut (atau pakai `ResponsiveTimer` seperti `FixtureSupport.swift`), lalu assert. Alternatif: tetap loop, tetapi hitung hanya iterasi saat main actor responsif.

### 🟡 Cakupan struktur shell tiruan dan satu ukuran jendela, `ShellFeedbackLayoutTests.swift:23-34`
**Problem**: Test membangun `NavigationSplitView` sendiri (tanpa `ShellSidebar`, footer inset, `toolbarBackgroundVisibility`, inspector) dan hanya pada 1280 x 800. Perubahan `AppShell.body` tidak masuk ke test ini; Test hanya menjaga `FeedbackCard`, bukan perakitan di `AppShell.swift:115-129`. Jendela di lebar minimum (sidebar 200 + 640) dan halaman dengan inspector tidak dicoba, padahal verify manual menyebut inspector.
**Why it matters**: Drift antara rakitan test dan `AppShell` membuat test menjaga hal yang berbeda dari yang dikirim.
**Suggested fix**: Pakai `AppShell` dengan `ShellBootstrap.fixture` kalau memungkinkan, atau tambah kasus lebar minimum dan satu kasus dengan inspector. Ubah loop menjadi `@Test(arguments:)` supaya kegagalan menyebut kartu mana.

### 🟡 Angka 200 tidak diturunkan dari apa pun dan tidak diuji di batasnya, `FeedbackCard.swift:106-109`
**Problem**: `textMinWidth = 200` adalah angka bebas. Teks panjang (sampel Backup, sekitar 230 karakter) masih membungkus ke sekitar 8 baris di 200 pt, jadi tinggi minimum jendela yang dihitung dari kartu tetap sekitar 170 pt lebih tinggi daripada tinggi di lebar sebenarnya. Pada 800 pt tidak terlihat, tetapi pada jendela yang lebih pendek kartu panjang menaikkan tinggi minimum lebih dari perlu. Kartu juga tidak bisa lebih sempit dari simbol + 200 + aksi + Dismiss + padding (kira-kira 330 sampai 480 pt); di bawah itu ia meluap, bukan membungkus. Di shell hal ini tak terjangkau karena `AppShell.contentMinWidth = 640`, tetapi host lain (sheet, inset gallery 150 pt) tidak dijaga.
**Why it matters**: Komponen bersama dengan batas implisit; bila host lebih sempit dipakai nanti, kartu terpotong tanpa peringatan.
**Suggested fix**: Terima 200 pt sebagai nilai pragmatis, tetapi catat di komentar mengapa 200 (lebar minimum teks yang masih terbaca) dan bahwa lebar minimum kartu utuh lebih besar. Pertimbangkan lapisan alternatif: batasi ukuran ideal di `FeedbackPresenter` (mis. `.frame(minWidth: AppShell.contentMinWidth)` pada isi inset), yang menyelesaikan akar di satu tempat untuk semua kartu; ini mengikat komponen ke shell, jadi pilihan sekarang (di kartu) tetap lebih bersih bila diterima.

### 🟡 verify.md: centang dan bukti tidak konsisten, `docs/specs/0067-komponen-dasar-stackly/verify.md:27`
**Problem**: Baris LiveShell diubah menjadi `[x]`, tetapi teks lamanya ("penempatan kartu di jendela utama tidak bisa diamati ... jadi langkah tetap menunggu fitur fase 02") dibiarkan, lalu ditambah catatan FAIL/lulus. Pembaca mendapat dua klaim bertentangan di satu baris. Bukti (`before.png`, `card.png`, `swift-test.log`) ada di scratchpad sesi `9e06f9e2`, bukan di repo (`evidence/`), dan lulusnya berlaku untuk perbaikan yang belum di-commit.
**Why it matters**: Bukti tidak bisa diperiksa ulang; centang `[x]` bertumpu pada perubahan yang belum ada di `main`.
**Suggested fix**: Ganti teks lama baris itu dengan hasil terbaru (atau hapus kalimat "menunggu fase 02"), salin screenshot kunci ke `evidence/` atau tandai "bukti lokal", dan commit perbaikan bersama pembaruan verify.

## Nits
- ⚪ `ShellFeedbackLayoutTests.swift:51`, `#expect(hosting.fittingSize.height <= size.height)` tanpa pesan; beri pesan seperti assert di atasnya.
- ⚪ `ShellFeedbackLayoutTests.swift:54-56`, `descendants(of:)` duplikat dengan `ShellBannerLayoutTests.swift`; pindah ke `Support/` bila ada test ketiga.
- ⚪ `default.profraw` (0 byte, root repo, tidak di-ignore): artefak sisa; hapus atau tambahkan `*.profraw` ke `.gitignore`, jangan di-commit.

## Strengths
- Akar masalah dirumuskan tepat (ukuran di lebar mendekati nol + inset bawah) dan diperbaiki di komponen yang memilikinya tanpa menyentuh `AppShell`; snapshot galeri tidak berubah dan teks tetap membungkus tanpa elipsis.
- Test memakai AppKit sungguhan, setiap sampel kartu dengan dan tanpa banner, pesan kegagalan menyebut tinggi dan sampel; diklaim gagal tanpa perbaikan (926 sampai 1513 pt).
- Komentar doc untuk `textMinWidth` menjelaskan alasan, bukan sekadar nilai; swift-format lint bersih; isolasi MainActor aman (default isolation paket, tanpa tipe non-Sendable lintas actor).
- Presenter lain (galeri, `Color.clear.frame(height: 150)`) tidak terpengaruh: kolom teks hanya mendapat lebar minimum, bukan tinggi.

## Test coverage
Ada satu regression test untuk bug layout (7 sampel x 2 kondisi banner). Belum tercakup: kehadiran kartu yang pasti, lebar jendela minimum, inspector, perakitan `AppShell` asli, dan perilaku di bawah lebar minimum kartu. Test snapshot galeri yang sudah ada menjaga bahwa tampilan kartu tidak berubah.
