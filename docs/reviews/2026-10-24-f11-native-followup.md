# F11, pemeriksaan native lanjutan

Tanggal: 2026-10-24. Pemeriksa: Codex. macOS 27.0.1 (26A434), Apple Silicon.

## Hasil otomatis

Semua perintah berikut lulus pada sesi ini. Log tersimpan di `target/f11-proof/logs/`.

| Perintah | Hasil |
| --- | --- |
| `cargo build -p stacklyd -p stackly-cli` | Lulus |
| `cargo xtask build-app` | BUILD SUCCEEDED, bundle Debug ditandatangani ad hoc |
| `cargo test -p stacklyd --test secret_sentinel --test secret --test cli_secret` | 5 test lulus, daemon nyata dan keychain file |
| `cargo test -p stackly-core --test secret_service --test secret_delivery` | 83 test lulus |
| `cargo test -p stackly-platform-macos --test secret_store` | 5 test lulus |
| `cargo test -p stacklyd --test server secret` | 13 test lulus |
| `swift test --package-path apps/macos/Packages/StacklyUI --filter 'Secret\|Reveal\|Concealed'` | 89 test lulus |
| `swift test --package-path clients/swift --filter 'SecretsClientTests\|RevealedSecretTests\|FixtureCredentialTests'` | 19 test lulus. Satu known issue sengaja ditangkap oleh test negatif `assertAbsentNamesWhatLeakedAndNeverTheValue`, bukan kegagalan produk |
| `cargo xtask check-deps` | 13 crate sesuai aturan |
| `cargo xtask codegen --check` | 20 file generated sesuai |
| `cargo xtask evidence check` | Baseline dan aturan evidence valid sebelum pembaruan record sesi ini |

## GUI nyata yang sudah diamati

Daemon memakai root sementara di `/tmp/stackly-f11.*`, keychain file di dalam root itu, dan credential uji dengan prefix fixture. Tidak ada credential pengguna yang dipakai. Halaman Secrets pada Design Gallery berkomunikasi dengan daemon nyata. Setelah build selesai, bundle hasil `target/app/Debug/Stackly.app` diluncurkan lagi pada root yang sama dan berhasil membaca reference revision 2.

- [x] Enter memakai secure field. Nilai lima karakter menampilkan `Use at least 8 characters.` dan Save tidak aktif. Nilai fixture yang valid mengaktifkan Save.
- [x] Save menampilkan Saved. Check Status menampilkan `present`, revision 1, store `available (file)`.
- [x] Menyimpan lagi menawarkan Replace. Replace berhasil, dan Check Status dari bundle hasil build terbaru menampilkan revision 2.
- [x] Show menampilkan URI lengkap. Hide mengembalikan teks tersamar.
- [x] Penyembunyian otomatis terjadi. Nilai masih tampil pada pengamatan 15,6 detik dan sudah tersembunyi pada pengamatan 41,1 detik. Ini membuktikan transisi native, bukan pengukuran presisi 30 detik. Batas tepatnya dicakup `RevealModelTests`.
- [x] Berpindah ke Xcode menyembunyikan nilai. Menutup Gallery saat nilai tampil lalu membukanya lagi juga mengembalikan teks tersamar.
- [x] Navigasi keyboard: Tab dari reference mencapai Open Entry Sheet, Space membuka sheet, Right lalu Space memilih Enter, Tab mencapai secure field, Show, dan Save. Space menjalankan Show, Hide, Save, dan Replace, Shift Tab mencapai Replace, Tab dan Return menyelesaikan Done. Tab dari halaman mencapai Show dan Copy, Space menjalankannya. Delete Reference dan konfirmasinya juga dijalankan melalui keyboard. Escape membatalkan sheet.
- [x] Layout Gallery dengan Narrow Width terbaca pada appearance dark. Increase Contrast native dinyalakan, tampilan app diperiksa, lalu setting dikembalikan ke off.
- [x] Pemindaian sembilan file root, registry beserta WAL, log daemon, dan log stdout/stderr app tidak menemukan prefix fixture. Folder keychain dikecualikan karena memang menyimpan credential. Pemindaian Unified Logging untuk proses Stackly selama 30 menit mengembalikan exit 0, 412622 byte, dan nol prefix fixture. Hasil di `target/f11-proof/logs/gui-leak-scan.json` dan `unified-log-scan.json`.

- [x] Copy menaruh URI lengkap ke clipboard. Pemeriksaan dilakukan dengan Cmd V pada field Reference sementara, tanpa menyimpan reference itu atau mengirimkannya ke daemon. Nilai masih bisa ditempel sebelum 90 detik, dan field tetap kosong saat Cmd V setelah sekitar 96 detik.
- [x] Copy berikutnya diganti teks `something-else` melalui Cmd C. Setelah 111 detik, Cmd V tetap menghasilkan `something-else`.
- [x] Copy lalu Cmd Q sebelum 90 detik, kemudian app diluncurkan kembali. Cmd V pada field kosong tidak memasukkan teks, jadi clipboard memang dibersihkan saat quit.
- [x] Reference dihapus lewat konfirmasi GUI. CLI `secret list` memastikan `no secret references`. App dan daemon uji dihentikan, root sementara dihapus.

Satu pembacaan awal ditolak saat binary daemon di `target/debug` berubah karena build lain berjalan. Perubahan identitas build diduga menjadi penyebab, tetapi penyebab itu tidak dibuktikan secara terpisah. Nilai uji dihapus, daemon dijalankan ulang dari salinan tetap di `target/f11-proof/stacklyd-gui`, lalu seluruh alur set, reveal, dan replace diulang dengan sukses. Tidak ada perubahan kode produk untuk kasus identitas build tersebut.

## Preview Xcode

Keempat preview pada `SecretPreviews.swift` berhasil dirender di Xcode 27.1. Color Scheme Variants menampilkan light dan dark. Field memakai pilihan Generate, Enter, Copy from, secure field, Show, serta petunjuk panjang. Entry sheet memakai area scroll untuk reference kedua. Revealable text tersamar dan tombol Show serta Copy terbaca. Tidak terlihat teks terpotong pada kontrol yang sedang ditampilkan.

Ekspor native tersedia di `target/f11-proof/Field previews/`, `target/f11-proof/Previews Screenshots/` (entry sheet), dan `target/f11-proof/Reveal previews/`. Preview dengan nama `Secret field, dark` juga diperiksa di canvas.

## Dialog login Keychain

Reference uji `local:user/manual-f11-check` dibuat melalui daemon default dengan input Generate, exit 0, revision 1. Daemon dihentikan dan salinan daemon ditandatangani ulang ad hoc dengan identifier lain untuk memicu pemeriksaan identitas. Reveal interaktif menunggu dialog.

Alat computer use menolak akses ke `com.apple.SecurityAgent` dengan pesan `Computer Use is not allowed to use the app 'com.apple.SecurityAgent' for safety reasons.` Karena itu Deny, Cancel, teks dialog, dan hasil masing masing tidak dapat diamati. Pemanggilan ini bukan PASS. CLI dibatalkan, daemon dihentikan, dan reference uji dihapus melalui daemon default, dengan hasil `Deleted`.


## Riwayat pemeriksaan interaksi yang telah dicabut

Kebijakan interaksi 2026-10-24 mencabut pemeriksaan berikut. Hasil lama tetap merupakan riwayat, bukan penghambat cakupan GUI yang berlaku sekarang. Lihat [rekonsiliasi kebijakan](2026-10-24-f11-interaction-policy-followup.md).

Ucapan `Password, hidden`, nilai saat shown, dan pengumuman `Password hidden` tidak dapat diamati. Ini tetap BLOCKED, bukan PASS dari source test. Increase Contrast juga sudah kembali off.

## Batas penutupan

Milestone daemon tetap terbuka sampai dua record dialog login Keychain benar benar terbukti. Pada revisi laporan asal, milestone GUI masih terbuka untuk record langkah 11 lama. Penutupan cakupan GUI terbaru dinilai terpisah dalam laporan rekonsiliasi kebijakan, tanpa mengubah hasil historis record tersebut. Build it, Verify it, status fitur, dan status spec belum ditutup. Bukti fixture dan test otomatis tidak diubah menjadi bukti dialog native.
