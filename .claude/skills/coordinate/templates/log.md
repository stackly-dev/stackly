# Log koordinasi: `<nama-SCOPE>`

- **Scope:** `docs/scope/<file>.md`
- **Hash awal:** `<git rev-parse main saat koordinasi mulai>` · **Hash awal wave aktif:** `<hash>`
- **Mode keputusan:** `auto` | `tanya berat` | `tanya` · **Ketentuan tambahan:** `<teks argumen apa adanya, atau "tidak ada">`
- **Aturan umum brief:** `<nama-SCOPE>.brief-umum.md` · **Langkah manual:** `<nama-SCOPE>.manual.md`
- **Wave aktif:** `<n>`, mulai `<YYYY-MM-DD>`
- **Diperbarui:** `<YYYY-MM-DD HH:MM>`

## Sub sesi wave `<n>`

| Nama | Peran | Item | Scope | Ownership | Branch | Model | Status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `<nama>` | `<peran>` | `<fitur, milestone, dan langkah Build plan bila pecahan>` | `<file scope>` | `<path>` | `claude/<name>-<hash>` | `<model · effort, mis. opus-5-5 · medium>` | `standby` / `aktif` / `peer` / `done` / `blocked` / `failed` / `merged` / `dipakai ulang` / `diarsipkan` / `tidak di-merge` |

## Keputusan

| Tanggal | Keputusan | Oleh |
| --- | --- | --- |
| `<YYYY-MM-DD>` | `<apa yang diputuskan dan alasannya>` | `user` / `lead` |

## Keputusan delegasi

Keputusan spec yang diambil dari opsi rekomendasi atas nama user (SKILL.md bagian "Mode keputusan"). Baris `berat` di atas.

| Tanggal | Spec · AC | Pertanyaan | Diambil | Runner-up | Berat | Cara membatalkan | Status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `<YYYY-MM-DD>` | `<NNNN>` · `<AC-n>` | `<pertanyaan>` | `<opsi dan alasan satu kalimat>` | `<opsi>` | `berat` / `-` | `architect (update)` spec `<NNNN>`; langkah Build plan `<n>` dibangun ulang | `berlaku` / `dibatalkan user <tanggal>` |

## Blocker terbuka

- `<item>`: `<penyebab>`, `<siapa atau apa yang membukanya>`

## Dicentang pada wave ini

- `<fitur>` · `<teks kotak>` · `<laporan yang jadi dasar>` · `<branch yang di-merge>` → `<hash merge>` · `<pemeriksaan di main yang lead jalankan>` · `<commit docs(scope) yang mencatatnya>` · `(manual NOT_RUN: <ID langkah di file manual>, owner user)` bila ada

## Riwayat wave

- Wave `<n>` (`<tanggal mulai>` sampai `<tanggal selesai>`, hash awal `<hash>`): `<ringkasan selesai, terbuka, blocker, kotak di scope lain yang terbuka>`

## Penutupan

- **Semua branch hijau di-merge:** `<YYYY-MM-DD>`; `git branch --no-merged main` hanya menampilkan branch yang tercatat `tidak di-merge` di bawah
- **Merge:** `claude/<name>-<hash>` → `<hash merge>` (satu baris per branch; branch `failed`, `blocked`, dan debugger dicatat `tidak di-merge: <alasan>`)
- **Document:** `<fitur>` · rentang `<a>..<b>` · commit `<hash>`
- **/sync:** `<YYYY-MM-DD>`, rentang `<hash awal>..<hash akhir>`, `<file yang diperbarui>`, `<centang yang ditolak dari /sync>`
- **Commit penutup:** `<hash>` `<subject>`
- **Push:** `origin/main` = `<hash>` · `<YYYY-MM-DD HH:MM>`
- **Dibersihkan:** `claude/<name>-<hash>` · sesi diarsipkan · worktree dihapus · branch dihapus (satu baris per branch; hanya yang lulus cek `merge-base --is-ancestor <branch> origin/main` dan worktree bersih)
- **Disimpan:** `claude/<name>-<hash>` · `<alasan: belum di origin/main, worktree kotor, sesi masih bekerja, di-pin, tidak di-merge>`, atau `tidak ada`
- **Blocker tersisa:** `<item dan alasan, atau tidak ada>`
