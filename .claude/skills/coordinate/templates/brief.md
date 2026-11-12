# Brief sub sesi

Lead mengisi semua field dan mengirimnya sebagai `prompt` saat memulai sub sesi, atau lewat `SendMessage` untuk sesi yang dipakai ulang. Teks di dalam `<...>` diganti. Brief hanya memuat yang khusus item ini; aturan yang sama untuk semua item scope ini ada di file "Aturan umum", dan isi brief menang bila berbeda. Sub sesi membaca brief ini, file aturan umum, lalu `.claude/skills/coordinate/roles/<peran>.md` (bila file itu tidak ada di worktree kamu, baca dari checkout utama yang disebut file aturan umum), lalu memanggil skill yang disebut di sana, baru mulai bekerja.

- **Nama sub sesi:** `<peran singkat>-f<fitur>-<item>` (pola di SKILL.md langkah 3) · **Judul:** `<name> · <fitur dan item>`
- **Wave:** `<n>`
- **Peran:** `architect` | `architect (update)` | `developer` | `checker (verify)` | `checker (review)` | `tester` | `debugger`
- **Aturan umum:** `<path absolut checkout utama>/docs/scope/coordination/<nama-SCOPE>.brief-umum.md`
- **Mode keputusan:** `auto` | `tanya berat` | `tanya` (dipakai role architect; peran lain mencatat tafsiran AC di laporan)
- **Model sesi:** `<model · effort dari tabel SKILL.md langkah 3>` · **Giliran pertama:** `langsung` | `standby` (hanya baca brief, aturan umum, dan role, lalu akhiri giliran dengan `STATUS: standby` tanpa mengirim pesan; mulai bekerja saat lead mengirim "mulai")
- **Sesi dipakai ulang:** `ya` | `tidak`; bila `ya`, konteks sudah dibersihkan lead, worktree dan branch lanjut dari item sebelumnya: jalankan `git merge main` dulu, lalu kerjakan item ini seperti brief baru
- **Pengetahuan dari graph:** `<hasil graphify query dan explain yang lead sudah jalankan untuk item ini: node, src=, loc=; atau "jalankan sendiri">`
- **Fitur:** `<nomor>. <nama>` di `docs/scope/<file>.md` (file scope selalu disebut, karena item bisa berasal dari file scope lain)
- **Milestone:** `<teks kotak persis seperti di SCOPE>` · **Langkah Build plan:** `<a> sampai <b>` atau `semua`
- **ID AC:** `<AC-n milestone ini saja dari spec, plus ID PRD dari baris Trace bila relevan>`
- **Path spec:** `docs/specs/<NNNN-slug>/index.md` (+ bagian `## Milestone <n>` di `verify.md` yang harus dijalankan checker); untuk architect ini nomor dan slug yang ditetapkan lead untuk spec baru, jangan ambil nomor lain
- **Spec target dan Follow-up sumber** (hanya `architect (update)`): `docs/specs/<NNNN-slug>/` dan baris Follow-up yang diterapkan, atau baris "Keputusan delegasi" yang dibatalkan beserta opsi yang user pilih
- **Follow-up dari spec lain:** `<path spec: baris Follow-up yang menyebut fitur ini, atau "tidak ada">`
- **File yang BOLEH diubah:** `<path atau folder, satu per baris>`
- **File yang DILARANG:** daftar "Selalu dilarang" di aturan umum, ditambah `<path atau folder milik sub sesi lain>`
- **Skill dan argumen:** `Skill("<skill>", "<argumen persis>")`, contoh `Skill("check", "verify protokol IPC & daemon")` atau `Skill("develop", "operation engine")`
- **Range perubahan:** `<hash sebelum merge pertama>..<hash merge terakhir>` (`git log --oneline <range>`, `git diff --name-only <range>`); checker review memakainya sebagai cakupan diff, tester sebagai daftar file yang diuji; verify milestone dari `source_branch` tidak memerlukannya
- **Langkah verify.md yang sudah [x]** (checker): `<nomor atau kutipan, atau "tidak ada">`; jalankan sisanya plus langkah yang AC-nya disentuh Range perubahan
- **Laporan gagal** (debugger): `<laporan lengkap sub sesi yang gagal, apa adanya, termasuk perintah dan output>`
- **Sub sesi terkait:** `<name>` · judul `<title persis>` → `<peran>` → `<untuk apa menghubunginya>`; di `ListAgents` cari baris yang judulnya diawali `<name>` dan kirim ke alamat persis seperti baris itu tercetak (judul plus `[ref]`); bila tidak ada baris yang cocok, kirim ke lead
- **Prasyarat yang sudah selesai:** `<kotak atau milestone [x] yang item ini andalkan, nomor langkah Build plan yang sudah di-merge ke main, dan di mana hasilnya>`
- **Langkah manual item ini:** `<kutip kotak atau langkah Build plan yang butuh user, atau "tidak ada">`; tulis sebagai langkah manual sesuai aturan umum, jangan jalankan
- **Jawaban panel khusus item ini:** `<jawaban yang berbeda dari "Panel skill" di aturan umum, atau "tidak ada">`
- **Model penulis kode** dan **Model reviewer:** `<family>` (hanya `checker (review)`; penulis dari kolom Model log dan trailer commit di Range perubahan)
