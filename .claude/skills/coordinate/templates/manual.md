# Langkah manual: `<nama-SCOPE>`

Langkah yang butuh mata, tangan, akun, perangkat, atau izin kamu. Tidak ada sesi yang menjalankannya. Kirim hasilnya ke lead (ID langkah, `PASS` atau `FAIL`, dan output bila `FAIL`); lead meneruskannya ke checker yang mencentang `verify.md`. `FAIL` membuka kembali kotaknya.

Keputusan yang diambil atas namamu ada di `<nama-SCOPE>.log.md` bagian "Keputusan delegasi"; baris bertanda `berat` paling perlu kamu baca. Untuk membatalkan satu baris, sebut barisnya ke lead.

Untuk lead: ID langkah `F<fitur>-<n>` tetap seumur file dan tidak dinomori ulang; langkah baru mendapat nomor berikutnya. Hapus paragraf ini di file hasil.

## Ringkasan

| ID | Langkah | AC | Kotak yang dicentang dengan langkah ini `NOT_RUN` | Bisa dijalankan sesudah | Hasil |
| --- | --- | --- | --- | --- | --- |
| `F<fitur>-<n>` | `<judul satu baris>` | `spec <NNNN> AC-<n>` | `<teks kotak, atau "belum ada">` | `<milestone atau merge yang harus sudah di main>` | `NOT_RUN` / `PASS` / `FAIL` `<tanggal>` |

## Nilai dari user

Nilai yang hanya kamu tahu. AC di kolom ketiga `BLOCKED` sampai nilainya ada.

| ID | Nilai yang dibutuhkan | AC yang menunggu | File yang menunggu |
| --- | --- | --- | --- |
| `N<n>` | `<mis. alamat email untuk laporan Code of Conduct>` | `spec <NNNN> AC-<n>` | `<path>` |

## Fitur `<nomor>`: `<nama>` (spec `<NNNN>`)

### `F<fitur>-<n>` · `<judul>`

Dari laporan `<nama sub sesi>` (`<YYYY-MM-DD>`). Membuktikan spec `<NNNN>` `AC-<n>`.

- **Prasyarat:** `<root sementara, binary, artefak cache, sesi login GUI, izin unduhan>`
- **Jalankan:** `<perintah persis, satu per baris, atau urutan klik di app>`
- **Lulus bila:** `<hasil yang diharapkan, dengan teks atau angka yang bisa dicocokkan>`
- **Hanya langkah ini yang bisa membuktikan:** `<apa yang tidak tercakup test fixture>`
