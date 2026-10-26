# F11, percobaan dialog login Keychain

> Riwayat sesi sebelum diagnosis OS. Status terkini: ketidaksesuaian awal terselesaikan melalui securityd, audit, dan CLI pada [diagnosis 26 Oktober](2026-10-26-f11-keychain-diagnosis.md). FAIL di bawah adalah verdict saat bukti OS belum dikorelasikan.

Pemeriksa: Codex, dengan tindakan dialog oleh pengguna. Revisi sumber: `252e35d130e3867681aca825bc62da05edbf9077`.

## Persiapan

Item uji baru memakai reference `local:user/f11-dialog-fcca168f48d6`, dibuat melalui `secret set --generate` pada daemon A. Hasil create: exit 0, present, revision 1, origin generated. Nilai uji tidak dicetak atau ditulis ke artefak pemeriksaan. CLI pembaca memakai stdin serta stderr TTY untuk permintaan interaktif; stdout ditangkap hanya dalam memori dan diringkas menjadi kode hasil serta boolean `valueReturned`.

Daemon menggunakan root default dan login Keychain sesuai langkah 7 spec 0011. Hanya item uji tersebut yang menjadi sasaran. Binary dan log sesi berada di `target/f11-keychain-proof/`.

## Pengamatan

| Percobaan | Pengamatan pengguna | Hasil CLI |
| --- | --- | --- |
| B, salinan binary dengan identifier tanda tangan ad hoc berbeda | Tidak ada dialog muncul dan tidak ada tombol ditekan | Exit 0, `valueReturned=true` |
| B, noninteractive | Tidak membutuhkan tindakan pengguna | Exit 0, `valueReturned=true` |
| C, binary hasil `cargo rustc -p stacklyd --bin stacklyd -- -C opt-level=1`, noninteractive sebelum dialog | Tidak membutuhkan tindakan pengguna | Exit 4, `secret.store_access_denied`, `byUser=false`, 0,061 detik, `valueReturned=false` |
| C, interaktif | Pengguna mengonfirmasi dialog menyebut stacklyd dan item uji Stackly, serta Deny ditekan; teks persis dan daftar tombol belum diberikan | Exit 0 setelah 32,425 detik, tanpa kode error, `valueReturned=true` |
| C, noninteractive sesudah percobaan interaktif | Tidak membutuhkan tindakan pengguna | Exit 0, `valueReturned=true` |
| D, binary hasil build `opt-level=2`, noninteractive sebelum pembatalan | Tidak membutuhkan tindakan pengguna | Exit 4, `secret.store_access_denied`, `byUser=false`, 0,344 detik, `valueReturned=false` |
| D, interaktif untuk Cancel/Escape, percobaan lama | Belum ada konfirmasi tindakan Cancel/Escape yang dapat dipadankan dengan permintaan | Timeout setelah 180,041 detik, exit 6, `secret.store_busy`, `valueReturned=false` |
| D, percobaan ulang saat pengguna siap | Pengguna mengonfirmasi **Always Allow** ditekan, bukan Cancel/Escape | Exit 0 setelah 34,285 detik, `valueReturned=true`, sesuai tindakan pemberian izin |

Metadata ACL dibaca tanpa membaca nilai secret. Setelah percobaan B, daftar aplikasi tepercaya mencantumkan A dan B. Setelah percobaan interaktif C, daftar tersebut juga mencantumkan C. Bukti ini menunjukkan perubahan akses, tetapi tidak membuktikan penyebabnya atau bahwa dialog yang dilaporkan adalah dialog item yang dimaksud. Pengguna kemudian mengonfirmasi nama aplikasi dan item uji yang sesuai. Teks persis dan daftar tombol masih belum diberikan. Log D pada `2026-10-25T17:56:17.907424Z` mencatat read yang selesai setelah pemanggil berhenti menunggu, dengan outcome `access_denied`. Hasil terlambat ini tidak menggantikan hasil CLI yang timeout dan tidak membuktikan tindakan Cancel/Escape pengguna.

## Status saat sesi awal

Hasil interaktif C **tidak memenuhi** hasil yang diharapkan untuk Deny, yaitu error tanpa nilai. Percobaan ini tidak ditandai PASS. Catatan pengguna dan hasil CLI sama sama dipertahankan, tanpa menganggap salah satunya sebagai pengganti yang lain. Bukti Escape terbaru sudah diperoleh dan lulus, seperti hasil Deny terbaru; ketidaksesuaian Deny awal tetap terbuka. Record langkah 7 dan checkbox penutupan F11 tetap terbuka sampai ketidaksesuaian ini jelas dan hasil yang disyaratkan terbukti.

## Dasar pemeriksaan ACL

Dokumentasi [SecAccessCreate dari Apple](https://developer.apple.com/documentation/security/secaccesscreate(_:_:_:)) menjelaskan bahwa daftar aplikasi tepercaya menentukan aplikasi yang dapat mengakses item tanpa dialog konfirmasi. Pemeriksaan sesi ini membaca metadata hanya untuk item uji, tanpa mengubah ACL atau mengizinkan aplikasi melalui dialog.

## Percobaan ulang 26 Oktober 2026

Pengguna menjelaskan bahwa tindakan pada percobaan ulang D adalah Always Allow. Item awal `local:user/f11-dialog-fcca168f48d6` kemudian dihapus melalui daemon, termasuk ACL pemberian izin milik item itu. Item uji baru `local:user/f11-escape-fcca168f48d6` dibuat dengan Generate melalui binary A, lalu dibaca binary D. Preflight noninteractive pada item baru menghasilkan exit 4, `secret.store_access_denied`, `byUser=false`, tanpa nilai, dalam 0,032 detik. Permintaan interaktif baru dipicu dengan panduan untuk menekan Escape pada dialog macOS. Pengguna mengonfirmasi Escape ditekan dan dialog tertutup. Hasilnya exit 4, `secret.store_access_denied`, `byUser=true`, `valueReturned=false`, dalam 51,040 detik. Permintaan berikutnya pada item yang sama berakhir sesudah pengguna mengonfirmasi Deny: exit 4, kode yang sama, `byUser=true`, tanpa nilai, dalam 45,435 detik. Pengguna juga tetap mengingat Deny pada percobaan C awal, sehingga hasil awal tidak direklasifikasi menjadi Always Allow.


## Hasil akhir sesi sebelum diagnosis OS

- Deny terbaru: PASS, exit 4, `secret.store_access_denied`, `byUser=true`, tanpa nilai.
- Escape terbaru: PASS, exit 4, kode yang sama, `byUser=true`, tanpa nilai.
- Noninteractive setelah Deny: PASS, exit 4, `byUser=false`, tanpa nilai, 0,057 detik.
- Status dan list: exit 0, item present, revision 1, store available, target login; tidak menunggu dialog dan tidak membawa nilai.
- Percobaan ad hoc dan perbedaan interaktif/noninteractive: record langkah 7 terkait menjadi PASS.
- Record Deny/Cancel keseluruhan: FAIL karena hasil awal C masih bertentangan dengan laporan tindakan pengguna. Ini merupakan ketidaksesuaian yang perlu ditelusuri, bukan bukti bahwa penyebabnya sudah ditemukan di kode aplikasi atau macOS.
- Teks persis seluruh dialog dan daftar tombol lengkap tidak diberikan. Nama aplikasi/item serta tindakan Deny, Escape, dan Always Allow berasal dari konfirmasi pengguna; tidak ada kutipan dialog yang dikarang.
- Kedua item uji dihapus melalui daemon. List kemudian kosong. Daemon uji dihentikan. App dibiarkan terbuka karena pengguna telah berpindah ke halaman Diagnostics.

Tidak ada perubahan kode produk. Binary variasi, metadata ACL, dan ringkasan hasil tanpa nilai secret disimpan di `target/f11-keychain-proof/`. Build it, Verify it, dan status F11 tetap terbuka.

## Penyelesaian setelah penelusuran OS

Log securityd untuk B dan C mencatat prompt serta Always Allow pada PID/path dan jendela waktu yang sesuai. Audit dan CLI sukses mengikuti keputusan itu. Laporan pengguna tetap dipertahankan, tetapi C tidak dipakai sebagai bukti penolakan OS. Dugaan B dipercaya otomatis hanya karena perubahan signing tidak didukung log. Deny dan Escape D cocok dengan nonapproval, ACL reject, audit gagal, dan respons tanpa nilai. Kedua record langkah 7 kini PASS dengan korelasi tersimpan; lihat diagnosis 26 Oktober untuk bukti, kontrol negatif, dan regresi.
