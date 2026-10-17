# Scope 10 · Merawat runtime & data

[Kembali ke index](index.md)

Journey pemeliharaan: hapus instance tanpa kehilangan data, bersihkan runtime yang tidak terpakai, daftarkan runtime Homebrew yang sudah ada, temukan data yatim, dan perbarui runtime serta provider revision tanpa merusak data. Fase ini selesai kalau tidak ada satu pun aksi pembersihan yang bisa menghapus data di luar scope atau tanpa konfirmasi eksplisit, dan semua aksi bekerja dari GUI dan CLI.

### 54. Remove, retain & purge · needs a decision · GA

Remove instance mempertahankan data sebagai retained record; purge adalah aksi terpisah dengan preview path, ukuran, dampak, dan konfirmasi eksplisit. Aturan purge di sini memakai primitive hapus aman dari fitur 7.

**Done when:** remove tanpa purge menyisakan data dan tercatat sebagai retained; purge di luar owned root ditolak walau lewat symlink, termasuk race yang bisa direproduksi; purge memeriksa ulang revision, operasi aktif, reference environment, dan path saat commit; fuzz target path purge berjalan di Mac ini; `stackly instance remove` dan `purge` setara dengan GUI, dan purge di mode noninteractive tetap butuh konfirmasi eksplisit.

**Trace:** REQ-028, REQ-052, BR-006, VAL-007, VAL-015, AC-052, AC-057, NFR-009

- [ ] Design it (spec): `/architect remove, retain & purge`

### 55. Hapus runtime & garbage collection · needs a decision · GA

Penghapusan runtime managed ditolak selama masih ada reference dari instance (termasuk yang stopped), dependency, operasi, atau snapshot recovery. GC hanya membersihkan cache dan runtime tak terpakai milik manager.

**Done when:** runtime yang dipakai instance stopped ditahan beserta daftar reference yang menahannya, dan data tidak berubah; runtime yang masih direferensikan backup yang belum terverifikasi restore (fitur 49 sampai 53) ditahan dengan alasan yang sama, kecuali spec menyatakan restore boleh memakai runtime kompatibel lain; hapus runtime dan GC hanya menghapus di bawah owned root `runtimes/` dan cache download, tidak pernah mengikuti symlink keluar, dan memeriksa ulang referensi serta path saat commit; GC tidak menyentuh runtime external atau dependency aktif; `stackly runtime remove` dan GUI menampilkan alasan yang sama.

**Trace:** REQ-007, BR-007, VAL-007, VAL-015, AC-007, GOAL-04, PRD bagian 17, 20.1, dan 35.2

- [ ] Design it (spec): `/architect hapus runtime & garbage collection`

### 56. Runtime external (Homebrew) · needs a decision

Daftarkan instalasi yang sudah ada dan terverifikasi sebagai runtime external, dengan ownership dan batas reproduksibilitas yang terlihat. Stackly tidak menjalankan `brew upgrade` atau `brew cleanup`, tidak mengganti symlink, dan tidak mengambil alih service atau data yang sudah ada.

**Done when:** binary external yang diubah atau dihapus dari luar terdeteksi sebagai drift atau missing; Stackly tidak beralih ke binary lain dan tidak menghapus instalasi itu; `stackly runtime register` dan halaman Runtimes menandai sumbernya sebagai external.

**Trace:** REQ-006, REQ-028, BR-019, VAL-009, AC-006, PRD bagian 20.2

- [ ] Design it (spec): `/architect runtime external (Homebrew)`

### 57. Scan data orphan & adoption · needs a decision · GA

Temukan direktori yang sudah ada atau orphan dan inventarisasi secara read only. Adoption hanya lewat preflight versi, config, dan ownership proses, lalu tindakan eksplisit pengguna; tidak pernah ada `initdb`, reset, atau upgrade otomatis.

**Done when:** scan data PostgreSQL orphan hanya menemukan metadata; adoption tidak berjalan sebelum kompatibilitas dan ownership terverifikasi dan pengguna setuju; `stackly instance adopt` dan GUI setara.

**Trace:** REQ-028, REQ-053, BR-008, VAL-009, AC-053, OQ-018, PRD bagian 20.1

- [ ] Design it (spec): `/architect scan data orphan & adoption`

### 58. Rebind runtime kompatibel · needs a decision · GA

Pindahkan instance ke runtime kompatibel yang sudah terpasang, misalnya patch baru dari mayor yang sama. BR-013 memisahkan empat operasi: update manager (fitur 61), update provider (fitur 59), install runtime baru (fitur 18), dan migrasi data mayor (tetap manual); rebind bukan salah satunya, dan tidak pernah menggantikan migrasi.

**Done when:** rebind hanya diizinkan untuk pasangan yang lolos preflight kompatibilitas dari fitur 19, dengan stop terkontrol, kebijakan backup sebelum rebind, dan verifikasi start; rebind yang ditolak menunjuk kebutuhan migrasi dan mengarahkan ke backup dan restore (fitur 50 dan 51); migrasi mayor tidak pernah sekadar mengganti binary.

**Trace:** REQ-043, BR-009, BR-013, AC-043, PRD bagian 22.1 dan 24.3

- [ ] Design it (spec): `/architect rebind runtime kompatibel`

### 59. Update provider revision · needs a decision · GA

Terapkan provider revision baru ke instance yang sudah ada. Revision baru datang dari catalog terpasang lewat update aplikasi (fitur 61) atau dari import lokal (fitur 37).

**Done when:** instance lama tetap pin ke revision lama sampai pengguna memilih apply; apply menampilkan diff capability, template, dan izin; apply yang masih menunggu approval trust (fitur 37) tidak mengubah instance.

**Trace:** BR-001, BR-013, BR-017, PRD bagian 24.3

- [ ] Design it (spec): `/architect update provider revision`
