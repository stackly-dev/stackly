# Log koordinasi: `<nama-SCOPE>`

- **Scope:** `docs/scope/<file>.md`
- **Hash awal:** `<git rev-parse main saat koordinasi mulai>` · **Hash awal wave aktif:** `<hash>`
- **Wave aktif:** `<n>`, mulai `<YYYY-MM-DD>`
- **Diperbarui:** `<YYYY-MM-DD HH:MM>`

## Sub sesi wave `<n>`

| Nama | Peran | Item | Scope | Ownership | Branch | Model | Status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `<nama>` | `<peran>` | `<fitur, milestone, dan langkah Build plan bila pecahan>` | `<file scope>` | `<path>` | `claude/<name>-<hash>` | `<family>` | `aktif` / `peer` / `done` / `blocked` / `failed` / `merged` / `diarsipkan` / `tidak di-merge` |

## Keputusan

| Tanggal | Keputusan | Oleh |
| --- | --- | --- |
| `<YYYY-MM-DD>` | `<apa yang diputuskan dan alasannya>` | `user` / `lead` |

## Blocker terbuka

- `<item>`: `<penyebab>`, `<siapa atau apa yang membukanya>`

## Dicentang pada wave ini

- `<fitur>` · `<teks kotak>` · `<laporan yang jadi dasar>` · `<branch yang di-merge>` → `<hash merge>` · `<pemeriksaan di main yang lead jalankan>` · `<commit docs(scope) yang mencatatnya>`

## Riwayat wave

- Wave `<n>` (`<tanggal mulai>` sampai `<tanggal selesai>`, hash awal `<hash>`): `<ringkasan selesai, terbuka, blocker, kotak di scope lain yang terbuka>`

## Penutupan

- **Semua branch hijau di-merge:** `<YYYY-MM-DD>`; `git branch --no-merged main` hanya menampilkan branch yang tercatat `tidak di-merge` di bawah
- **Merge:** `claude/<name>-<hash>` → `<hash merge>` (satu baris per branch; branch `failed`, `blocked`, dan debugger dicatat `tidak di-merge: <alasan>`)
- **Document:** `<fitur>` · rentang `<a>..<b>` · commit `<hash>`
- **/sync:** `<YYYY-MM-DD>`, rentang `<hash awal>..<hash akhir>`, `<file yang diperbarui>`, `<centang yang ditolak dari /sync>`
- **Commit penutup:** `<hash>` `<subject>`
- **Blocker tersisa:** `<item dan alasan, atau tidak ada>`
