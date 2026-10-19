# 0012. Rationale: design system dan fondasi UI

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: mockup UI mengunci layout, isi, teks, alur, dan state, tapi beberapa nilai warnanya gagal WCAG AA (teks status di badge dan banner berwarna, teks sekunder di banner offline, dan konten yang diredupkan 60% saat offline). Kalau nilai mockup dipakai apa adanya, aturan proyek "UI meets WCAG AA contrast" langsung dilanggar di layar pertama. Spec ini membaca warna mockup sebagai niat (hijau untuk running, oranye untuk transisi, merah untuk gagal), bukan sebagai nilai final. Topik ini juga menggabungkan beberapa keputusan (token, status, state, konfirmasi); semuanya tetap satu spec karena bertemu di satu sistem token, satu test kontras, dan satu galeri.

Aplikasi macOS Stackly adalah client SwiftUI untuk `stacklyd` dan baru berisi `RootView` placeholder di package `StacklyUI`. Sekitar empat puluh layar dan state di mockup (`docs/mockups/Stackly.dc.html`) akan dibangun oleh banyak fitur journey yang berbeda, ditambah shell (fitur 13), komponen dasar (fitur 67), fixture dan cek visual (fitur 68), dan peta perintah (fitur 69). Tanpa satu sumber warna, tipografi, status, dan perilaku state, setiap fitur akan memilih sendiri, dan perbedaan kecil (warna "Stopped" di Overview berbeda dengan di Instances, empty yang muncul padahal load gagal) baru terlihat saat semua layar sudah ada.

Kekuatan yang membatasi: PRD meminta setiap halaman data mendukung loading, empty, error, stale atau disconnected, unsupported, dan partial (REQ-024, PRD bagian 11.2), dan error per instance tidak boleh membuat aplikasi kosong. Aksesibilitas wajib: navigasi keyboard, label VoiceOver, ukuran teks, light dan dark, status selain warna, dan konfirmasi aksi destruktif (REQ-027, NFR-012, AC-027). Saat daemon tidak terjangkau, GUI tetap tampil tanpa menyatakan instance berhenti (AC-024). Tombol tidak boleh tampil seolah capability unsupported bekerja (REQ-026). Scope menambah bahwa accent dan mode Liquid Glass mengikuti System Settings, dan Reduce Transparency serta Increase Contrast tetap menghasilkan permukaan yang terbaca. Aturan fidelity menyatakan kontrol native menang atas tiruan HTML di mockup.

Fakta teknis yang membentuk pilihan: ukuran teks mockup (28, 24, 17, 15, 13, 12, 11, 10 px) hampir persis sama dengan text style bawaan macOS. Warna `secondaryLabelColor` bawaan macOS di mode light kira kira hitam 50% (sekitar 3,95:1 di putih), jadi warna sistem saja tidak cukup untuk teks sekunder. Domain (spec 0004) punya sepuluh state lifecycle instance plus readiness dan health yang terpisah, sementara mockup hanya menggambar lima status. `StacklyUI` dibuild dengan SwiftPM dan dites dengan `swift test` dari command line, tempat asset catalog tidak diproses seperti di build Xcode. Fitur 12 tidak bergantung pada Rust dan bisa dibangun paralel dengan fondasi daemon.

## Options considered

### Keputusan 1: letak dan bahasa design.md

#### Option 1: `apps/macos/design.md`, bahasa Inggris

**Pros**:

- Dekat dengan kode yang diaturnya dan dengan `apps/macos/AGENTS.md` yang dibaca setiap sesi SwiftUI.
- Bahasa sama dengan nama token, kode, dan copy UI.

**Cons**:

- Beda bahasa dengan spec ini; alasan dan aturan ada di dua bahasa.

#### Option 2: `docs/design.md`, bahasa Indonesia

**Pros**:

- Satu bahasa dengan scope dan spec.

**Cons**:

- Jauh dari kode, dan builder harus menerjemahkan aturan ke nama API Inggris.

#### Option 3: `design.md` di root, bahasa Inggris

**Pros**:

- Mudah ditemukan semua tool.

**Cons**:

- Aturan khusus macOS tampak berlaku untuk seluruh repo, termasuk CLI dan klien Windows nanti.

### Keputusan 2: batas kode fitur 12 dan fitur 67

#### Option 1: 12 membangun token, status (termasuk `ToneBadge`), state, dan konfirmasi; 67 membangun komponen lain

**Pros**:

- Fitur 14 bisa menampilkan status dan state tanpa menunggu 67.
- Klaim aksesibilitas (Increase Contrast, Reduce Transparency, status tanpa warna) dibuktikan di kode yang memang memakainya.

**Cons**:

- Kalimat Done when fitur 67 yang menyebut empty state, banner, dan dialog konfirmasi harus diselaraskan.

#### Option 2: 12 hanya token dan model

**Pros**:

- Batas paling tipis; semua view ada di 67.

**Cons**:

- Fitur 14 menunggu 67 atau merakit view sendiri yang nanti diganti.

#### Option 3: 12 hanya design.md

**Pros**:

- Paling cepat selesai.

**Cons**:

- Tidak ada yang bisa dites; kontras dan perilaku state baru terbukti setelah 67.

### Keputusan 3: sumber warna netral

#### Option 1: warna sistem, kecuali yang gagal AA

Teks utama, separator, dan latar kontrol dari sistem; teks sekunder, kartu, latar halaman kartu, dan console dari token sendiri.

**Pros**:

- Ikut Increase Contrast dan perubahan macOS secara otomatis di sebagian besar permukaan.
- Teks sekunder tetap lolos AA.

**Cons**:

- Dua sumber warna netral yang harus dijaga konsisten.

#### Option 2: semua hex mockup

**Pros**:

- Persis mockup dan sepenuhnya bisa dihitung.

**Cons**:

- Tidak ikut perubahan sistem, dan setiap varian high contrast harus diisi sendiri.

#### Option 3: semua warna sistem

**Pros**:

- Paling native, tanpa token netral sendiri.

**Cons**:

- Teks sekunder di mode light gagal AA.

### Keputusan 4: tempat definisi warna kustom

#### Option 1: kode di `StacklyUI`

Awalnya `NSColor` dengan dynamic provider. Setelah cross check, bentuknya menjadi tabel murni `value(scheme:contrast:)` plus `ShapeStyle` yang membaca environment SwiftUI, tetap di `StacklyUI` dan tetap empat varian.

**Pros**:

- `swift test` dari command line bisa menghitung kontras setiap varian langsung dari tabel.
- View dan preview di package memakai warna yang sama tanpa trik bundle.
- Teks, latar, dan border membaca satu sumber varian, jadi tetap sepakat saat Appearance dipaksa lewat `preferredColorScheme`.

**Cons**:

- Tidak ada editor visual warna di Xcode, dan token kustom tidak punya accessor `NSColor` untuk kode AppKit.

#### Option 2: asset catalog di package `StacklyUI`

**Pros**:

- Editor visual dan varian high contrast bawaan Xcode.

**Cons**:

- Asset catalog di SwiftPM butuh build Xcode, jadi test kontras dari command line sulit.

#### Option 3: `Assets.xcassets` target app

**Pros**:

- Tempat yang biasa untuk warna app.

**Cons**:

- View di `StacklyUI` tidak bisa memakainya tanpa trik bundle, dan preview package kehilangan warnanya.

### Keputusan 5: nilai warna teks status

#### Option 1: nilai baru yang lolos AA, plus varian high contrast 7:1

**Pros**:

- Lolos 4,5:1 di kartu, latar jendela, dan badge atau banner berwarna, di light dan dark.
- Dark hijau dan oranye tetap nilai mockup.

**Cons**:

- Teks status light sedikit lebih gelap dari mockup.

#### Option 2: nilai mockup apa adanya

**Pros**:

- Paling setia ke mockup.

**Cons**:

- Gagal AA di badge dan banner berwarna (lihat Bukti kontras).

#### Option 3: warna sistem untuk teks status

**Pros**:

- Satu sumber warna.

**Cons**:

- Hijau dan oranye sistem di light hanya sekitar 2,2:1.

### Keputusan 6: tone untuk state transisi

#### Option 1: oranye seperti mockup

**Pros**:

- Setia ke mockup; transisi berarti instance belum bisa dipakai.

**Cons**:

- Oranye juga dipakai untuk warning; pembedanya label dan simbol.

#### Option 2: biru

**Pros**:

- Memisahkan progress dari warning, sama dengan tag operasi Running.

**Cons**:

- Menyimpang dari mockup, dan biru dekat dengan accent default.

#### Option 3: abu

**Pros**:

- Tenang.

**Cons**:

- Instance yang sedang start terlihat sama dengan yang mati.

### Keputusan 7: state lifecycle di luar mockup

#### Option 1: peta lengkap sepuluh state

**Pros**:

- Setiap state domain punya tampilan sendiri; tidak ada yang disamarkan.
- `unknown` tidak pernah tampil sebagai Stopped (AC-024).

**Cons**:

- Tabel harus diperbarui setiap kali domain menambah state.

#### Option 2: sama, tapi `unknown` merah

**Pros**:

- Identitas proses yang hilang terlihat mendesak.

**Cons**:

- `unknown` sering sementara (daemon baru restart dan sedang adopsi); merah memberi kesan gagal.

#### Option 3: lebur ke lima label mockup

**Pros**:

- Sederhana.

**Cons**:

- `unknown` sebagai Stopped melanggar AC-024, dan `recovering` sebagai Failed menyesatkan.

### Keputusan 8: readiness dan health di status yang tampil

#### Option 1: status gabungan

**Pros**:

- "Running" hijau hanya saat port benar benar menerima koneksi, sesuai mockup dan PRD ("tidak hanya menghitung PID").

**Cons**:

- Label tidak lagi satu banding satu dengan lifecycle; adapter harus membawa reason readiness.

#### Option 2: lifecycle saja, readiness dan health terpisah

**Pros**:

- Jujur ke model domain.

**Cons**:

- "Running" bisa tampil saat instance belum bisa dipakai.

### Keputusan 9: jangkauan Liquid Glass

#### Option 1: hanya glass dari kontrol native

**Pros**:

- Tinted atau clear, Reduce Transparency, dan Increase Contrast diurus macOS.
- Teks konten selalu di atas permukaan opak, jadi kontras bisa dihitung.

**Cons**:

- Toast dan dialog glass di mockup berubah menjadi presentasi native atau permukaan opak.

#### Option 2: sistem ditambah overlay mengambang kustom

**Pros**:

- Lebih mirip mockup untuk umpan balik dan panel mengambang.

**Cons**:

- Setiap pemakaian kustom harus dites di semua setting, dan kontras teks di atas glass tidak tetap.

#### Option 3: sistem ditambah kartu konten

**Pros**:

- Paling mirip visual mockup.

**Cons**:

- Teks di atas glass sulit dijamin lolos AA; melawan arahan Apple yang menaruh glass di lapisan navigasi.

### Keputusan 10: tampilan stale

#### Option 1: tanpa redup, banner dan waktu

**Pros**:

- Konten tetap lolos AA; waktu state terakhir dan aksi nonaktif sudah memberi sinyal.

**Cons**:

- Menyimpang dari opacity 0,6 di mockup.

#### Option 2: redup 0,6 seperti mockup

**Pros**:

- Sinyal visual kuat.

**Cons**:

- Teks sekunder light turun ke 2,42:1.

#### Option 3: redup ringan 0,85

**Pros**:

- Sinyal tipis tambahan.

**Cons**:

- Teks sekunder di permukaan tint tetap jatuh di bawah 4,5:1.

### Keputusan 11: Increase Contrast di permukaan kustom

#### Option 1: varian high contrast dan border 1 pt

**Pros**:

- Teks naik ke 7:1 dan batas kartu, banner, serta badge terlihat jelas.

**Cons**:

- Dua varian warna per token dan kode border yang membaca `colorSchemeContrast`.

#### Option 2: hanya warna teks

**Pros**:

- Lebih sedikit kerja.

**Cons**:

- Batas permukaan tetap samar.

#### Option 3: andalkan sistem saja

**Pros**:

- Tanpa kode tambahan.

**Cons**:

- Warna kustom tidak berubah sama sekali; tujuan Increase Contrast tidak tercapai.

### Keputusan 12: status tanpa warna

#### Option 1: titik dan label, simbol saat Differentiate Without Color

**Pros**:

- Setia ke mockup dan menghormati setting sistem.

**Cons**:

- Dua tampilan status yang harus dites.

#### Option 2: selalu simbol dan label

**Pros**:

- Paling aman tanpa bergantung setting.

**Cons**:

- Menyimpang dari titik 7 pt di mockup dan lebih ramai di tabel.

#### Option 3: titik dan label saja

**Pros**:

- Paling sederhana; label sudah memenuhi aturan.

**Cons**:

- Tidak menghormati Differentiate Without Color.

### Keputusan 13: aksi yang wajib dikonfirmasi

#### Option 1: hilang data, hapus, atau mengganggu pihak lain

**Pros**:

- Sama dengan mockup (stop instance yang dipakai lease dikonfirmasi, stop biasa tidak).
- Dialog jarang, jadi tetap dibaca.

**Cons**:

- Setiap fitur harus menilai aksinya terhadap aturan.

#### Option 2: semua aksi yang menghentikan

**Pros**:

- Paling aman.

**Cons**:

- Dialog sering muncul dan orang terbiasa mengklik tanpa membaca.

#### Option 3: hanya yang tidak bisa dibatalkan

**Pros**:

- Paling sedikit dialog.

**Cons**:

- Stop instance yang dipakai environment tidak dikonfirmasi, padahal mockup mengonfirmasinya.

### Keputusan 14: bentuk dialog konfirmasi

#### Option 1: `confirmationDialog` native berbasis item

**Pros**:

- Fokus, VoiceOver, keyboard, dan warna destructive diurus sistem; sesuai aturan fidelity.

**Cons**:

- Perilaku tombol default di macOS 27 perlu dicek; jalur `NSAlert` disiapkan sebagai cadangan.
- Hasil cek langkah 17 (2026-10-18): gagal di macOS 27. Satu tombol hanya memegang satu key equivalent, jadi Cancel tidak bisa menerima Return dan Escape sekaligus. Modifier yang sama kini membangun `NSAlert` sendiri (lihat *Pembaruan 2026-10-18*).

#### Option 2: native ditambah ketik nama

**Pros**:

- Lebih aman untuk hapus data permanen.

**Cons**:

- Bukan idiom macOS dan butuh sheet kustom.

#### Option 3: dialog glass kustom seperti mockup

**Pros**:

- Persis visual mockup.

**Cons**:

- Melanggar aturan fidelity dan harus mengurus fokus serta VoiceOver sendiri.

### Keputusan 15: model state

#### Option 1: satu enum dengan keterangan stale dan partial

**Pros**:

- Konten tetap tampil saat stale atau partial; urutan prioritas sama di semua layar.

**Cons**:

- Layar harus memakai `resolve` dan tidak boleh membuat `ViewState` langsung.

#### Option 2: enam case setara

**Pros**:

- Mudah dibaca.

**Cons**:

- Stale dan partial menyembunyikan konten.

#### Option 3: bebas per layar

**Pros**:

- Fleksibel.

**Cons**:

- Pola cepat menyimpang antar layar.

### Keputusan 16: loading pertama

#### Option 1: spinner setelah 300 ms

**Pros**:

- Tidak berkedip pada daemon lokal yang biasanya cepat.

**Cons**:

- Perlu timer kecil dan test waktu.

#### Option 2: skeleton

**Pros**:

- Memberi gambaran isi.

**Cons**:

- Jarang di app macOS dan butuh label VoiceOver khusus.

#### Option 3: spinner langsung

**Pros**:

- Paling sederhana.

**Cons**:

- Berkedip setiap pindah halaman.

### Keputusan 17: tombol capability unsupported

#### Option 1: nonaktif dengan alasan yang terlihat

**Pros**:

- Memenuhi REQ-026 dan AC-027; pengguna keyboard dan VoiceOver juga tahu alasannya.

**Cons**:

- Setiap layar harus memberi tempat untuk teks alasan.

#### Option 2: nonaktif dengan tooltip

**Pros**:

- Ringkas.

**Cons**:

- Alasan hanya lewat hover, dilarang AC-027.

#### Option 3: disembunyikan

**Pros**:

- Rapi.

**Cons**:

- Melanggar REQ-026 dan scope fitur 67.

### Keputusan 18: tipografi

#### Option 1: text style semantik

**Pros**:

- Ukuran mockup jatuh tepat ke style sistem, dan teks ikut pengaturan ukuran yang didukung macOS.

**Cons**:

- Angka 28 dan 24 di mockup menjadi 26 dan 22.

#### Option 2: ukuran tetap dari mockup

**Pros**:

- Pixel sama.

**Cons**:

- Tidak ikut ukuran teks pengguna.

#### Option 3: skala sendiri dengan `@ScaledMetric`

**Pros**:

- Fleksibel.

**Cons**:

- Menduplikasi text style sistem.

### Keputusan 19: spacing dan ukuran

#### Option 1: skala bernama, padding tetap, ikon dan tinggi minimum diskalakan

**Pros**:

- Layout mirip mockup di ukuran normal dan tidak terpotong di teks besar.

**Cons**:

- Builder harus memilih antara padding tetap dan ukuran yang diskalakan.

#### Option 2: semua diskalakan

**Pros**:

- Lega di teks besar.

**Cons**:

- Layout bergeser jauh dan sulit dicocokkan dengan mockup.

#### Option 3: angka literal

**Pros**:

- Cepat di awal.

**Cons**:

- Konsistensi antar layar hilang.

### Keputusan 20: tampilan error

#### Option 1: placeholder halaman dan banner per item

**Pros**:

- Error per instance tidak mengosongkan halaman (PRD bagian 11.2), dan error dekat dengan itemnya.

**Cons**:

- Dua bentuk error yang harus dibedakan builder.

#### Option 2: alert dialog

**Pros**:

- Jelas.

**Cons**:

- Memblokir dan menumpuk saat beberapa instance gagal bersamaan.

#### Option 3: satu banner di atas halaman

**Pros**:

- Sederhana.

**Cons**:

- Error jauh dari item yang bermasalah.

### Keputusan 21: warna ikon bagian

#### Option 1: warna sistem per bagian seperti mockup

**Pros**:

- Setia ke mockup dan memberi penanda visual per bagian; warna sistem ikut Increase Contrast.

**Cons**:

- Tidak ikut accent pengguna.

#### Option 2: ikut accent

**Pros**:

- Perilaku default sidebar macOS.

**Cons**:

- Menyimpang dari mockup.

#### Option 3: monokrom

**Pros**:

- Tenang.

**Cons**:

- Kehilangan penanda visual.

### Keputusan 22: accent aplikasi

#### Option 1: `AccentColor` kosong

**Pros**:

- Pilihan accent pengguna selalu menang; Multicolor menampilkan biru sistem.

**Cons**:

- Tidak ada warna merek.

#### Option 2: biru merek #0A7AFF

**Pros**:

- Warna mockup saat Multicolor.

**Cons**:

- Hampir sama dengan biru sistem, jadi nilainya kecil.

### Keputusan 23: cara membuktikan kontras

#### Option 1: unit test di `StacklyUI`

**Pros**:

- Gagal otomatis kalau ada token yang turun; jalan di Mac ini tanpa app.

**Cons**:

- Test harus me resolve warna sistem per appearance.

#### Option 2: cek manual saat verify

**Pros**:

- Tanpa kode.

**Cons**:

- Regresi baru ketahuan di cek manual berikutnya.

#### Option 3: diserahkan ke fitur 68

**Pros**:

- Satu tempat untuk semua audit.

**Cons**:

- Token bisa rusak tanpa ketahuan sampai 68 selesai.

## Rationale

**Native dulu, kustom hanya untuk yang gagal (keputusan 3, 4, 9, 18, 21, 22).** Kekuatan terbesar adalah gabungan aturan fidelity (kontrol native menang) dan kewajiban AA. Warna sistem, text style sistem, dan glass sistem memberi Increase Contrast, Reduce Transparency, tinted atau clear, accent, dan perubahan macOS di masa depan secara gratis, dan itu persis yang diminta scope ("mengikuti System Settings"). Token kustom hanya dibuat di titik tempat sistem gagal (teks sekunder light) atau tempat mockup butuh permukaan yang tidak dimiliki sistem (kartu, latar halaman kartu, console). Mendefinisikannya di kode, bukan asset catalog, adalah satu satunya cara membuat test kontras jalan dengan `swift test` di Mac ini tanpa build Xcode. Glass kustom ditolak karena kontras teks di atas glass bergantung pada isi di belakangnya, jadi tidak bisa dibuktikan.

**Palet yang bisa dibuktikan (keputusan 5, 6, 7, 8, 11, 12, 23).** Niat warna mockup dipertahankan (oranye untuk transisi, hijau untuk running, merah untuk gagal), tapi nilainya dihitung ulang sampai lolos di permukaan terburuk, termasuk tint 16% di atas latar jendela. Varian high contrast dibuat untuk 7:1 karena pengguna yang menyalakan Increase Contrast meminta lebih dari batas minimum. Peta sepuluh state dan status gabungan dipilih karena AC-024 dan PRD bagian 11.2 menolak tampilan yang menyederhanakan kebenaran: `unknown` bukan Stopped, dan proses hidup belum tentu siap. Titik tetap seperti mockup, dan simbol berbentuk berbeda menggantikannya saat pengguna meminta lewat Differentiate Without Color. Test di `StacklyUI` membuat semua klaim ini bertahan setelah orang lain mengubah token.

**State dan konfirmasi yang seragam (keputusan 2, 10, 13, 14, 15, 16, 17, 19, 20).** Satu enum dengan resolver tunggal menjamin aturan yang paling sering dilanggar: empty tidak pernah muncul sebelum load berhasil, dan data terakhir tidak pernah dibuang karena satu refresh gagal. Stale tidak meredupkan konten karena redup langsung melanggar AA; banner, waktu, dan aksi nonaktif sudah cukup. Konfirmasi memakai dialog native dengan Cancel sebagai default untuk aksi destruktif, karena kehilangan data akibat Return yang refleks lebih mahal dari satu Tab tambahan, dan aturan "hilang data, hapus, atau mengganggu pihak lain" menjaga dialog tetap jarang. Fitur 12 membangun semua view ini, bukan hanya model, supaya fitur 14 tidak menunggu fitur 67 dan supaya klaim aksesibilitas dibuktikan di kode yang memakainya. design.md tinggal di `apps/macos` dalam bahasa Inggris karena pembacanya builder SwiftUI yang menulis nama API Inggris.

### Keputusan kecil yang diambil di spec ini

| Hal | Pilihan | Alasan | Runner up |
| --- | --- | --- | --- |
| Latar halaman kartu | `surfaceWindow` (#F5F5F7, #1E1E20) | kartu dark #2A2A2D selalu lebih terang dari latarnya, seperti mockup | latar jendela sistem, yang di dark mode bisa lebih terang dari kartu |
| Skala spacing | tambah 14 | padding horizontal baris dan kartu mockup hampir selalu 14 | bulatkan ke 12 atau 16 |
| Resolusi warna kustom | tabel murni `value(scheme:contrast:)` plus `ShapeStyle` dari environment | satu sumber varian untuk teks dan border; tabel bisa dites langsung untuk keempat varian | `NSColor` dinamis lewat `Color(nsColor:)`, yang memilih varian dari `NSAppearance` sementara border membaca `colorSchemeContrast`, jadi bisa tidak sepakat |
| Teks status di baris, sidebar, menu, popover | konteks `.row` dengan `.primary` | seleksi tidak aktif dan latar glass menurunkan teks tone di bawah 4,5:1 (dark merah sekitar 4,03:1) | teks tone di semua tempat |
| Daemon tidak terjangkau | `MutationAvailability` di environment, diisi shell | satu sumber kebenaran untuk gate mutasi; tidak ada banner ganda | keterangan stale `.daemonUnreachable` di setiap `ViewState` |
| `ToneBadge` | dibangun di fitur 12 | status operasi butuh tampilan sekarang, dan border Increase Contrast badge dibuktikan di sini | menunggu fitur 67 |
| Stale ganda | refresh gagal menang atas outdated | keduanya berarti data lama; error refresh lebih bisa ditindaklanjuti | daftar beberapa keterangan stale |
| Ukuran simbol | satu helper `.stacklySymbol(size:relativeTo:)` | simbol placeholder 44 dan 34 pt butuh ukuran tetap yang diskalakan, dan test pemindai butuh satu pengecualian yang jelas | mengecualikan semua `Image` dari aturan font |
| Alasan nonaktif untuk VoiceOver | `.help` saja, plus teks yang terlihat | di macOS `.help` sudah menjadi bantuan VoiceOver; hint tambahan membuat alasan dibacakan dua kali | `.help` plus `accessibilityHint` |
| Galeri di build Release | seluruh folder `Gallery/` di `#if DEBUG` | scene Debug saja tidak cukup, karena kode package tetap ikut terkompilasi | hanya scene yang dibungkus |
| Window untuk cadangan `NSAlert` | anchor `NSViewRepresentable`, `runModal()` kalau tidak ada window | popover menu bar (fitur 32) tidak punya window untuk sheet | selalu `runModal()` |
| Tombol capability sebelum data termuat | tidak digambar | capability datang bersama data layar, jadi tidak perlu keadaan tiga nilai | tri state dengan alasan "Checking capabilities…" |
| Status operasi | dipetakan di spec ini | tag operasi seragam di Overview, Operations, dan menu bar | diserahkan ke fitur 35 |
| Simbol status | bentuk berbeda per state | Differentiate Without Color benar benar membedakan Stopped, Retained, dan Unknown | satu simbol per tone |
| Banner daemon tidak terjangkau | digambar shell sekali | tidak dobel di setiap halaman | tiap `StateView` menggambar sendiri |
| Penjaga aturan | test pemindai sumber di `StacklyUITests` | menahan regresi ukuran font tetap, glass kustom, dan dialog di luar modifier | grep manual saat verify |
| Galeri | jendela Debug "Design Gallery" plus `#Preview` | preview tidak bisa menyalakan Increase Contrast atau Reduce Transparency; app nyata bisa | hanya preview |
| Simbol `informative` | `ellipsis.circle.fill` | ada di semua versi SF Symbols yang relevan dan berbeda dari simbol tone lain | `clock.fill` |
| Simbol critical | `xmark.circle.fill` | sama dengan ikon gagal di mockup | `xmark.octagon.fill` |
| Opasitas tint | 16% badge, 12% banner | dalam rentang mockup (11% sampai 16%); nilai terburuk 16% yang dihitung | satu nilai 14% |
| Durasi crossfade | 0,2 detik | sama dengan transisi mockup | animasi default SwiftUI |
| Target klik | minimal 24 × 24 pt | WCAG 2.2 target size minimum (AA) | ukuran kontrol native saja |
| Jalur konfirmasi | `NSAlert` di modifier yang sama (dipakai sejak langkah 17) | `confirmationDialog` gagal cek Return dan Escape di macOS 27; API untuk layar tetap, hanya implementasi yang berganti | menerima Return memicu aksi destruktif |

## Bukti kontras

Dihitung dengan rumus relative luminance WCAG 2.2 di sRGB. Permukaan light: kartu #FFFFFF, latar jendela #F5F5F7, dan perkiraan latar jendela sistem #ECECEC. Permukaan dark: kartu #2A2A2D, latar jendela #1E1E20, perkiraan latar sistem #323232 dan #1E1E1E, dan console #18181A. Tint: fill tone sistem 12% dan 16% di atas setiap permukaan. Test di milestone 1 mengulang hitungan ini dengan warna sistem yang di resolve di macOS 27.

**Nilai mockup yang gagal AA** (di bawah 4,5:1):

| Pasangan | Light | Dark |
| --- | --- | --- |
| `--green-t` di badge tint 16% di atas latar jendela | 4,14 | lolos |
| `--orange-t` di badge tint 16% di atas latar jendela | 4,15 | lolos |
| `--red-t` di badge tint 16% di atas kartu | lolos (4,87) | 4,27 |
| `--text2` di badge abu tint 16% | 4,20 | 4,46 |
| `--text2` di banner offline (oranye 15% di atas latar jendela) | 4,35 | lolos |
| Accent sebagai teks di kartu (tag operasi, link) | 4,01 | 3,57 |
| `--text2` dengan konten diredupkan 0,6 | 2,42 | 2,99 |
| `--text` dengan konten diredupkan 0,6 | 4,45 | lolos |
| `--text3` di kartu (dipakai untuk placeholder dan ikon) | 2,57 | 2,74 |
| Putih di tombol merah (dialog konfirmasi mockup) | 3,55 | 3,41 |

**Nilai yang dipilih** (kontras terendah di semua permukaan polos, dan di semua tint):

| Tone | Light polos | Light tint | Dark polos | Dark tint | HC light polos | HC light tint | HC dark polos | HC dark tint |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `positive` | 5,17 | 4,63 | 7,32 | 5,43 | 7,07 | 6,33 | 7,32 | 5,43 |
| `caution` | 5,16 | 4,63 | 7,19 | 5,32 | 7,06 | 6,33 | 7,19 | 5,32 |
| `critical` | 5,63 | 4,61 | 5,48 | 4,66 | 7,04 | 5,76 | 7,05 | 6,00 |
| `neutral` dan `textSecondary` | 5,29 | 4,60 | 5,80 | 4,64 | 7,01 | 6,10 | 7,07 | 5,65 |
| `informative` | 5,65 | 4,66 | 5,51 | 4,64 | 7,01 | 5,78 | 7,07 | 5,96 |

**Pasangan tambahan setelah cross check**:

| Pasangan | Light | Dark | HC light | HC dark |
| --- | --- | --- | --- | --- |
| `textSecondary` di banner tint 12% tone apa pun, di atas `surfaceCard` dan `surfaceWindow` (P4) | 4,92 | 5,16 | 6,51 | 6,29 |
| Teks tone di tint tone yang sama, di atas `surfaceCard` dan `surfaceWindow` (P3), terendah | 4,95 | 5,11 | 5,76 | 5,32 |
| Teks tone di latar seleksi tidak aktif (perkiraan #DCDCDC dan #464646), terendah | 4,45 | 4,03 | | |

Baris terakhir adalah alasan konteks `.row`: di baris tabel, teks status memakai `.primary` (sekitar 12:1 di light dan 8:1 di dark pada latar yang sama).

Tombol merah dan accent di dialog tidak masuk tabel: setelah keputusan 14, keduanya digambar sistem, dan accent tidak lagi dipakai sebagai warna teks kustom. `tertiaryLabelColor` hanya untuk teks nonaktif dan ikon dekoratif, yang dikecualikan WCAG.

## Cross check

Dijalankan 2026-10-18 oleh model lain (Sonnet 5.5) sebagai pembacaan read only. Angka kontras yang diklaimnya dihitung ulang: masalah teks tone di latar seleksi yang tidak aktif benar (light hijau dan oranye 4,45:1, dark merah 4,03:1, abu 4,27:1, biru 4,05:1), sedangkan klaim teks sekunder di banner 4,56:1 hanya terjadi di atas latar sistem dan hilang setelah tint dibatasi ke `surfaceCard` dan `surfaceWindow` (hasil ulang minimal 4,92:1 di light dan 5,16:1 di dark). Pengguna menyetujui semua perbaikan yang direkomendasikan, termasuk mengganti `NSColor` dinamis dengan tabel murni plus `ShapeStyle`. Yang diterapkan:

- Konteks `StatusLabel` `.standard` dan `.row`; tabel Pasangan kontras yang eksplisit (P1 sampai P4), tint hanya di atas `.card` dan `.window`, teks tone hanya di atas tint tone yang sama, dan pencampuran di sRGB.
- Larangan `.secondary`, `.tertiary`, `secondaryLabelColor`, `tertiaryLabelColor`, accent, dan `.tint` sebagai warna teks, plus aturan untuk teks sekunder di kontrol native.
- Helper `.stacklySymbol(size:relativeTo:)` dan pola test pemindai yang lebih lengkap.
- `MutationAvailability` dan `.mutationGated()` menggantikan `.daemonUnreachable`.
- Input `resolve` yang lengkap (`lastLoadedAt`, `isRefreshing`, `outdated`), banner untuk empty, Retry di banner stale, item dan Show All di banner partial, gaya tombol placeholder, format waktu yang bisa dites, dan Kontrak view model.
- `DelayedVisibility` dengan sleep yang bisa diganti.
- Enum status UI eksplisit, satu detail paling banyak, `StatusDetail`, dan kesegaran yang dihitung daemon.
- AC-4 untuk operasi (label dan tone saja), `partial` hanya untuk `succeeded`, dan `ToneBadge` di fitur 12.
- `.cardSurface()` dan `SectionIcon`.
- Daftar Cek di /check verify untuk yang tidak bisa dibuktikan `swift test`.
- Fallback cek lebar sempit kalau text style tidak membesar.
- Isolasi dan `Sendable`, helper bersama di milestone 1, galeri di `#if DEBUG`, `.help` tanpa hint ganda.
- Judul dialog yang disimpan selama ditutup, window untuk cadangan `NSAlert`, dan cek keyboard sebagai langkah pertama milestone 4.
- Tombol capability baru digambar setelah data termuat.
- Copy yang mengikuti mockup ("try another status", "Showing the last known state from…"), AC-16 yang dirumuskan ulang, dan AC-20 baru untuk gate mutasi.

## Perubahan 2026-10-18 (rekonsiliasi lintas spec)

Pengecekan silang Assumed lines antara spec 0005, 0006, 0009, 0011, dan 0012 menemukan dua selisih yang menyentuh spec ini. Engineer menyetujui keduanya.

| Selisih | Perbaikan | Alasan | Runner up |
| --- | --- | --- | --- |
| Spec 0006 meminta nilai enum terbuka yang tidak dikenal ditampilkan apa adanya, tetapi enum status UI tidak punya case untuk itu, dan fallback 0006 semula bernama `unknown` yang bentrok dengan state asli | case `unrecognized` di `LifecycleStatus`, `ReadinessStatus`, `HealthStatus`, dan `OperationStatus`: label "Unrecognized State", tone `caution`, simbol `questionmark.diamond.fill`, detail "Update Stackly to see this state" | pengguna melihat bahwa app nya lebih tua dari daemon, bukan state yang ditebak, dan labelnya tidak tertukar dengan "Unknown" (cross check) | memetakan ke `unknown` yang sudah ada |
| Asumsi bahwa capability provider dikirim spec 0009 dan 0006, padahal keduanya tidak punya DTO itu | asumsi diarahkan ke spec 0013 dan fitur 20 | DTO layar ditetapkan spec yang membangun layar dan data instance | |

### Temuan langkah 17 (2026-10-18, dicatat atas persetujuan engineer)

Cek di macOS 27.0 dan Xcode 27.0, di Design Gallery dengan event keyboard nyata dan pohon aksesibilitas: `confirmationDialog` SwiftUI adalah sheet `NSAlert`, dan satu tombol hanya memegang satu key equivalent. Dengan desain spec (Cancel destructive sebagai `.defaultAction`), Return membatalkan tetapi Escape tidak berbuat apa apa; tanpa shortcut atau dengan `.cancelAction`, Escape membatalkan tetapi Return tidak berbuat apa apa. Keduanya gagal AC-15. Jalur cadangan yang sudah ada di spec dipakai: `NSAlert` langsung, dengan Cancel memegang Return dan monitor `keyDown` lokal untuk Escape pada role destructive. Empat run penuh lolos (satu run pertama kehilangan Return sintetis tepat setelah peluncuran, tanpa aksi yang berjalan). API modifier tidak berubah, jadi layar tidak terpengaruh.

Ronde 2 rekonsiliasi (2026-10-18), dari Follow-up spec 0013 dan atas persetujuan engineer: lebar konten minimum 640 pt menjadi final, setter environment bernama `.mutationAvailability(_:)`, footer status daemon memakai `StatusLabel` `.row`, dan tone banner status daemon (informative untuk starting, connecting lambat, shutting down; critical untuk incompatible, untrusted, recovery, misconfigured).

## Pembaruan 2026-10-18

Diperbarui lewat `/architect` setelah build dan `/check verify` fitur 12, dengan persetujuan pengguna. Tiga perubahan:

- **Konfirmasi di atas `NSAlert`.** Langkah 17 menjalankan cek keyboard di jendela Design Gallery app Debug (macOS 27.0 26A428, Xcode 27.0 27A266a) dengan tombol Return dan Escape sungguhan dan pohon aksesibilitas. `confirmationDialog` SwiftUI adalah sheet `NSAlert` di bawahnya, dan satu tombol hanya memegang satu key equivalent: Cancel destructive dengan `keyboardShortcut(.defaultAction)` menerima Return tapi Escape mati, dan tanpa shortcut itu Escape membatalkan tapi Return mati. Request yang diganti saat dialog terbuka juga tetap memakai judul lama. Jalur cadangan yang sudah ada di spec dipakai: `NSAlert` dengan Return di Cancel dan monitor Escape lokal. `/check verify` mengulang cek ini (Return dan Escape untuk kedua role, Tab lalu Space, dan jalur `runModal()` tanpa window) dan semuanya lulus. Pilihan ini membawa kode AppKit sendiri yang harus dicek ulang di setiap versi macOS baru; alternatifnya, menerima Return yang menjalankan aksi destruktif, ditolak karena melanggar invariant utama spec ini.
- **AC-18 memakai lebar sempit 640 pt.** Langkah 4 membuktikan text style dan `@ScaledMetric` tidak membesar dengan `dynamicTypeSize` di macOS 27, jadi preview teks besar diganti preview lebar sempit, seperti yang sudah disiapkan spec ini sebagai cadangan. Spec 0013 kemudian menetapkan jendela minimum 900 pt dan sidebar paling lebar 260 pt, jadi konten tidak pernah lebih sempit dari 640 pt dan angka itu kini final. Pengguna memutuskan bahwa preview Narrow width memenuhi AC-18.
- **Follow-up fitur 67 lewat `/scope`.** Done when fitur 67 masih menyebut label status, banner, empty state, dan dialog konfirmasi yang sudah dibangun spec ini. Penyelarasannya milik `/scope`, jadi dicatat sebagai Follow-up tersendiri.

Cross check pembaruan ini dijalankan model lain (Sonnet 5.5) secara read only, dan pengguna menyetujui rekomendasinya. Teks Konfirmasi dilengkapi sesuai kode dan design.md (penundaan tampil sampai anchor punya window, detail monitor Escape, reset binding saat view hilang, label "Cancel"), AC-15 dan AC-17 menyebut bahwa Tab lalu Space butuh Keyboard navigation, dan `@ScaledMetric` dicatat tetap dipakai walau belum menskalakan di macOS 27. Tiga risiko jalur `runModal()` (popover yang punya window, app yang tidak aktif, modal bersarang saat request diganti) dicatat sebagai Follow-up fitur 32, karena belum ada pemanggil tanpa jendela dan kodenya tidak diubah sekarang.
Cross check pembaruan ini dijalankan model lain (Sonnet 5.5) secara read only, dan pengguna menyetujui rekomendasinya. Teks Konfirmasi dilengkapi sesuai kode dan design.md (penundaan tampil sampai anchor punya window, detail monitor Escape, reset binding saat view hilang, label "Cancel"), AC-15 dan AC-17 menyebut bahwa Tab lalu Space butuh Keyboard navigation, dan `@ScaledMetric` dicatat tetap dipakai walau belum menskalakan di macOS 27. Tiga risiko jalur `runModal()` (popover yang punya window, app yang tidak aktif, modal bersarang saat request diganti) dicatat sebagai Follow-up fitur 32, karena belum ada pemanggil tanpa jendela dan kodenya tidak diubah sekarang.
