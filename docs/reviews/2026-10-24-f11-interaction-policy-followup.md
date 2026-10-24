# F11, rekonsiliasi kebijakan interaksi

Pemeriksa: Codex. Revisi yang diperiksa: `7e044b14cc39894ee5cba39f31d5fe266085113e`.

## Cakupan yang berlaku

`AGENTS.md` dan catatan perubahan pada awal spec 0011 mencabut dukungan pembaca layar khusus, navigasi serta shortcut keyboard khusus, pengaturan tampilan aksesibilitas, dan identifier UI test. Pemeriksaan kemampuan yang dicabut berstatus RETIRED, bukan PASS. Dialog login Keychain merupakan pemeriksaan keamanan terpisah dan tetap diperlukan.

Record GUI lama `2026-10-20-spec-0011-step-11-secret-entry-and-reveal-flows.json` tetap menjadi catatan historis revisi sebelumnya. Penolakan akses SecurityAgent dalam dua record langkah 7 juga tetap disimpan. Rekonsiliasi ini tidak mengubah riwayat menjadi hasil lulus.

## Bukti setelah perubahan UI

`cargo xtask build-app` berhasil dan menghasilkan bundle Debug bertanda tangan ad hoc. `swift test --package-path apps/macos/Packages/StacklyUI --filter 'Secret|Reveal|Concealed'` menjalankan 77 test dalam 23 suite, semuanya lulus. Model reveal dan pasteboard tidak berubah secara fungsional pada revisi kebijakan, sedangkan label serta pengumuman khusus di view dihapus. Test model tetap mencakup seluruh hasil atau tidak sama sekali, konfirmasi replace, validasi nilai, dan aturan clipboard.

App hasil build dijalankan bersama daemon dari bundle yang sama, memakai root sementara `/tmp/stackly-f11-policy.*` dan keychain file di dalamnya. Semua nilai adalah fixture. Login Keychain tidak disentuh oleh pemeriksaan GUI ini. Interaksi dilakukan melalui tombol pointer dan penyuntingan teks native.

- [x] Input `short` menampilkan `Use at least 8 characters.` dan Save tidak aktif. Input fixture valid mengaktifkan Save.
- [x] Save menampilkan Saved. Check Status menjawab `present`, revision 1, store `available (file)`.
- [x] Show menampilkan URI berisi nilai fixture. Nilai masih tampil pada pengamatan sekitar 25,5 detik sejak Copy dan sudah tersamar pada 37,5 detik. Show dimulai beberapa detik sebelum Copy; ini membuktikan transisi native, sedangkan batas tepat 30 detik diuji model.
- [x] Copy menghasilkan URI lengkap, diamati dengan menempel ke field reference sementara tanpa menjalankan permintaan terhadap teks itu. Setelah sekitar 100,6 detik sejak Copy, Cmd V pada field kosong tidak lagi menempelkan teks.
- [x] Menyimpan pada reference yang sudah ada menawarkan Replace. Klik Replace menampilkan Saved dan Check Status menunjukkan revision 2.
- [x] Klik Hide menyamarkan kembali URI.
- [x] Pemindaian delapan file root dan log tidak menemukan prefix fixture. Unified Logging untuk proses Stackly dipindai 119204 byte dengan exit 0 dan nol kecocokan. Folder keychain dikecualikan sesuai tujuan penyimpanan secret.

- [x] Salinan pengganti `policy-clipboard-check` tetap bisa ditempel setelah sekitar 97,5 detik sejak Copy, sehingga timer tidak menghapus salinan baru.
- [x] Copy lalu Quit melalui menu biasa sebelum tenggat, kemudian peluncuran ulang, menghasilkan clipboard kosong saat ditempel ke field kosong.
- [x] Tampilan dark pada lebar sempit dan light biasa diamati melalui app terbaru tanpa override aksesibilitas.
- [x] Delete Reference meminta konfirmasi dan berhasil. CLI memastikan tidak ada reference tersisa. App serta daemon dihentikan dan root sementara dihapus.

Record cakupan GUI terbaru: [`2026-10-24-spec-0011-gui-under-the-pointer-interaction-poli.json`](../../evidence/manual/2026-10-24-spec-0011-gui-under-the-pointer-interaction-poli.json), **PASS**. Log dan hasil scan ada di `target/f11-policy-proof/`. Record lama tidak diubah menjadi PASS. Milestone Swift client dan GUI dapat dicentang sesuai cakupan yang telah direvisi.

## Bukti yang tetap berlaku

[Laporan native sebelumnya](2026-10-24-f11-native-followup.md) memuat pengamatan clipboard kosong setelah tenggat, perlindungan salinan pengganti, pembersihan saat quit, replace, kehilangan fokus, tutup lalu buka Gallery, serta render light dan dark. Bukti keyboard dan pengaturan aksesibilitas di laporan itu hanya riwayat. Hasilnya tidak dijadikan persyaratan baru dan tidak dipakai untuk mengembalikan fitur yang telah dicabut.

## Batas penutupan

Build it dan Verify it F11 tetap terbuka selama bukti Deny dan Cancel serta dialog login Keychain setelah perubahan identitas ad hoc belum tersedia. Pembatasan akses alat ke SecurityAgent tidak boleh diganti dengan hasil keychain file. Tidak ada klaim F11 selesai atau spec Accepted.
