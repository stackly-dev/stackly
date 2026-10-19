# Brief sub sesi

Lead mengisi semua field dan mengirimnya sebagai `prompt` saat memulai sub sesi. Teks di dalam `<...>` diganti. Sub sesi membaca brief ini, lalu `.claude/skills/coordinate/roles/<peran>.md` (bila file itu tidak ada di worktree kamu, baca dari checkout utama di field "Checkout utama"), lalu memanggil skill yang disebut di sana, baru mulai bekerja. Sub sesi bekerja di worktree dan branch sendiri yang sudah dibuat aplikasi; jangan membuat branch lain, jangan push, jangan PR.

- **Nama sub sesi:** `<peran singkat>-f<fitur>-<item>` (pola di SKILL.md langkah 3) · **Judul:** `<name> · <fitur dan item>`
- **Wave:** `<n>`
- **Peran:** `architect` | `architect (update)` | `developer` | `checker (verify)` | `checker (review)` | `tester` | `debugger`
- **Checkout utama:** `<path absolut hasil git rev-parse --show-toplevel di lead>`; role dan template dibaca dari sini bila tidak ada di worktree
- **Fitur:** `<nomor>. <nama>` di `docs/scope/<file>.md` (file scope selalu disebut, karena item bisa berasal dari file scope lain)
- **Milestone:** `<teks kotak persis seperti di SCOPE>` · **Langkah Build plan:** `<a> sampai <b>` atau `semua`
- **ID AC:** `<AC-n milestone ini saja dari spec, plus ID PRD dari baris Trace bila relevan>`
- **Path spec:** `docs/specs/<NNNN-slug>/index.md` (+ bagian `## Milestone <n>` di `verify.md` yang harus dijalankan checker); untuk architect ini nomor dan slug yang ditetapkan lead untuk spec baru, jangan ambil nomor lain
- **Spec target dan Follow-up sumber** (hanya `architect (update)`): `docs/specs/<NNNN-slug>/` dan baris Follow-up yang diterapkan
- **Follow-up dari spec lain:** `<path spec: baris Follow-up yang menyebut fitur ini, atau "tidak ada">`
- **File yang BOLEH diubah:** `<path atau folder, satu per baris>`
- **File yang DILARANG:** `<path atau folder>`, selalu termasuk `docs/scope/`, `prd.md`, `AGENTS.md`, dan file milik sub sesi lain
- **Skill dan argumen:** `Skill("<skill>", "<argumen persis>")`, contoh `Skill("check", "verify protokol IPC & daemon")` atau `Skill("develop", "operation engine")`; bila tool `Skill` tidak ada, baca `~/.claude/skills/<skill>/SKILL.md` dan ikuti
- **Range perubahan:** `<hash sebelum merge pertama>..<hash merge terakhir>` (`git log --oneline <range>`, `git diff --name-only <range>`); checker review memakainya sebagai cakupan diff, tester sebagai daftar file yang diuji; verify milestone dari `source_branch` tidak memerlukannya
- **Langkah verify.md yang sudah [x]** (checker): `<nomor atau kutipan, atau "tidak ada">`; jalankan sisanya plus langkah yang AC-nya disentuh Range perubahan
- **Laporan gagal** (debugger): `<laporan lengkap sub sesi yang gagal, apa adanya, termasuk perintah dan output>`
- **Alamat lead:** `<nama lead [ref] dari baris pertama ListAgents>`; laporan lengkap dan ringkasan pesan dikirim ke sini lewat `SendMessage`
- **Sub sesi terkait:** `<name>` · judul `<title persis>` → `<peran>` → `<untuk apa menghubunginya>`; di `ListAgents` cari baris yang judulnya diawali `<name>` dan kirim ke alamat persis seperti baris itu tercetak (judul plus `[ref]`); bila tidak ada baris yang cocok, kirim ke lead
- **Prasyarat yang sudah selesai:** `<kotak atau milestone [x] yang item ini andalkan, nomor langkah Build plan yang sudah di-merge ke main, dan di mana hasilnya>`
- **Langkah yang butuh izin user:** `<kutip kotak atau langkah build plan, atau "tidak ada">`; minta lewat lead, jangan jalankan sendiri
- **Jawaban tetap untuk panel skill:** jangan pernah memanggil `AskUserQuestion`; panel yang tidak dijawab di sini dikirim ke lead lewat `SendMessage`, lalu akhiri giliran dengan `STATUS: needs-decision`. Cross check spec = `<dari log Keputusan; default Another model untuk Beta/GA>`; "Accept this spec?" = kirim ringkasan ke lead dan tunggu jawabannya; gap kelengkapan keputusan = `Let me answer each one` plus rekomendasi ke lead; "Save verify steps?" = `Save to verify.md`; tawaran branch/commit = commit langsung di branch ini; "Run the suite?" = `Yes, run and fix to green`; "No changes" = `Specific files` dari Range perubahan; "Which model wrote this code?" = Model penulis kode
- **Commit:** di branch sendiri per langkah atau milestone, pesan `type(scope): subject` plus trailer `Co-Authored-By` dari system reminder sesi kamu; tanpa graphify di worktree; tidak push, tidak PR
- **Model penulis kode** dan **Model reviewer:** `<family>` (hanya `checker (review)`; penulis dari kolom Model log dan trailer commit di Range perubahan)
- **Format laporan:** `.claude/skills/coordinate/templates/report.md`; sesudah laporan skill asli, panggil `wrap_up` dengan recap yang baris pertamanya `STATUS: ...` dan tulis baris yang sama sebagai baris pertama pesan terakhir giliran; lead hanya membaca 400 karakter pertama
