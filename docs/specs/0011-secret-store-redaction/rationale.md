# 0011. Rationale: secret store dan redaction

[Kembali ke spec](index.md)

## Context

> ⚠️ Premise note: Done when fitur 11 menyebut permukaan yang baru dibangun fitur lain: audit (fitur 35), export (fitur 45), diagnostics (fitur 34), serta pemakaian ulang alur isi secret oleh create instance (fitur 20), env up (fitur 46), dan restore (fitur 51). Fitur 11 tidak bisa membuktikan permukaan yang belum ada. Spec ini karena itu membangun mekanismenya (`Redactor`, alur `secret.set`, `resolve_for_start`, komponen GUI, input aman CLI), membuktikannya pada permukaan yang sudah ada saat fitur 11 dibangun (log daemon, IPC, registry termasuk tabel audit, CLI, GUI), dan mencatat test sentinel untuk permukaan lain di Follow-up fitur pemiliknya. Kalau kamu ingin Done when fitur 11 hanya menagih yang bisa dibuktikan di fitur ini, itu perubahan lewat `/scope`.

Stackly menjalankan PostgreSQL, MySQL, NATS, ClickHouse, Valkey, dan Traefik untuk developer di satu Mac, dan hampir semuanya butuh credential: password superuser, token client, password default user. PRD meminta secret disimpan di secret store platform dengan hanya reference di SQLite dan manifest, masking yang sama di GUI, CLI, log, diagnostics, dan export, serta reveal yang hanya terjadi lewat aksi eksplisit (REQ-016, AC-016). Secret tidak boleh masuk manifest yang dibagikan, lockfile, audit default, atau argumen command bila ada cara yang lebih aman (BR-018). Kalau Keychain tidak bisa diakses saat autostart, service harus terlihat tertahan, bukan berjalan dengan auth kosong, `trust`, atau password hardcode (AC-062). PRD juga mengakui bahwa sebagian tool mensyaratkan secret di file config plaintext, dan menyerahkan kebijakan itu beserta access policy Keychain ke keputusan security owner (OQ-008, PRD bagian 23).

Arsitekturnya sudah membatasi siapa boleh menyentuh apa. Hanya daemon yang menulis registry, membuat ID dan path, dan menjalankan proses; GUI tidak menautkan Rust dan hanya berbicara lewat IPC; CLI tidak menjalankan binary provider (spec 0001, BR-011). Scope fitur 15 menagih "Keychain milik daemon tetap terbaca setelah helper ditandatangani", jadi item Keychain milik `stacklyd`. Sampai fitur 15, app dan daemon ditandatangani ad hoc, dan setiap build ulang menghasilkan code signature baru. Di macOS, ACL item di keychain file (login Keychain) menyimpan signature aplikasi yang dipercaya, jadi signature baru membuat macOS menampilkan dialog izin. Dialog itu menahan thread pemanggil sampai pengguna menjawab, dan saklar untuk mematikan interaksi Keychain berlaku untuk seluruh proses. Data protection keychain, alternatif modern tanpa dialog ACL, membutuhkan entitlement access group dengan Team ID, sesuatu yang belum ada sebelum fitur 15.

Batas keamanan baseline adalah satu akun user OS; Stackly tidak mengklaim melindungi dari proses jahat dengan hak user yang sama (BR-012). Integration test wajib memakai root ephemeral, tidak pernah folder asli pengguna atau state global (AGENTS.md, spec 0002), dan spec 0002 sudah menyediakan fixture credential dengan prefix yang dipindai wrapper evidence. Spec 0004 sudah menetapkan pola port sync di `stackly-platform`, `unsafe` hanya di `stackly-platform-macos`, layout dua root dengan runtime root di folder temp privat user, dan `STACKLY_ROOT` untuk mengarahkan daemon ke root lain. Beberapa spec ditulis paralel: spec 0005 memiliki skema registry, spec 0006 memiliki envelope error dan kategori, spec 0009 memiliki manifest provider, dan spec 0012 memiliki token desain. Spec ini harus memutuskan bagiannya tanpa memutuskan bagian mereka.

Kalau tidak diputuskan sekarang, setiap fitur yang butuh password (create instance, connection, env up, restore, log, diagnostics) akan menyimpan, mengirim, dan menyamarkan secret dengan caranya sendiri, dan satu jalur yang lupa sudah cukup untuk membocorkan password ke log yang dibagikan pengguna.

## Options considered

### Keputusan 1: secret di config plaintext (OQ-008)

#### Option 1: Izinkan dengan aturan ketat

Mekanisme pengiriman diurutkan (stdin, hash, file sekali pakai, env var, config plaintext) dan provider memakai yang tertinggi yang didukungnya. Config plaintext hanya jalan terakhir: file include terpisah 0600 di runtime root, dirender dari Keychain setiap start, dihapus setelah stop bersih, disapu untuk instance yang tidak berjalan, dan dicatat sebagai residual risk. argv tidak pernah.

**Pros**:

- Valkey (`requirepass`) dan NATS (token di config) tetap bisa memakai auth.
- Plaintext hanya ada selama instance berjalan, dan tidak pernah di root data, jadi tidak ikut Time Machine, cold backup, atau diagnostics.
- Keychain tetap satu satunya sumber kebenaran.

**Cons**:

- Selama instance berjalan, proses lain milik user yang sama bisa membaca file itu.
- Butuh render ulang di setiap start dan reload, dan penyapuan setelah crash.

#### Option 2: Izinkan tanpa batas waktu

Sama seperti Option 1, tetapi file plaintext tetap ada setelah instance berhenti.

**Pros**:

- Lebih sederhana: render sekali, tanpa penyapuan.

**Cons**:

- Plaintext menetap di disk, ikut backup, dan tertinggal setelah instance lama tidak dipakai.

#### Option 3: Tolak provider yang butuh plaintext

Manifest yang tidak bisa memakai stdin, hash, file sekali pakai, atau env ditolak saat validasi.

**Pros**:

- Tidak ada plaintext di disk sama sekali.

**Cons**:

- Valkey dan NATS tidak bisa memakai auth, padahal keduanya provider wajib dengan contoh `secretRefs` di PRD bagian 13.1.

### Keputusan 2: kapan dialog Keychain boleh muncul

#### Option 1: Hanya untuk aksi pengguna

Permintaan dari klien interaktif (GUI, CLI dengan TTY) boleh memunculkan dialog izin atau buka kunci. Pemicu latar dan CLI noninteractive tidak pernah, dan hasilnya needs secret dengan alasan.

**Pros**:

- Autostart tidak pernah tertahan dialog yang tidak dilihat siapa pun (AC-062).
- Pengguna yang menekan Start bisa langsung menyetujui dialog, termasuk setelah build ulang ad hoc.

**Cons**:

- Dua jalur interaksi yang harus diuji, dan klien harus menyatakan dirinya interaktif atau tidak.

#### Option 2: Tidak pernah

Daemon selalu mematikan interaksi Keychain.

**Pros**:

- Perilaku paling bisa ditebak dan paling mudah diuji.

**Cons**:

- Setelah build ulang ad hoc, item lama tidak bisa dibaca sama sekali sampai ACL nya diubah manual di Keychain Access, yang pada macOS terbaru tersembunyi dari pengguna biasa.
- Keychain yang terkunci tidak bisa dibuka dari alur Stackly.

#### Option 3: Selalu boleh

Perilaku bawaan macOS.

**Pros**:

- Tanpa kode tambahan.

**Cons**:

- Autostart dan restart policy bisa tertahan tanpa batas di dialog yang tidak dilihat siapa pun, melanggar AC-062.

### Keputusan 3: signing ad hoc sampai fitur 15

#### Option 1: Terima dialog di development

Setelah setiap build ulang `stacklyd`, aksi pengguna pertama yang membaca item lama memunculkan dialog; kamu klik Always Allow. Test memakai Keychain file sementara, jadi tidak pernah memicu dialog.

**Pros**:

- Tanpa tooling atau sertifikat tambahan.
- Jalur kode development sama persis dengan rilis.

**Cons**:

- Dialog muncul berulang selama development, dan pemicu latar tertahan `access_denied` sampai kamu mengizinkan.

#### Option 2: Sertifikat self signed lokal

`cargo xtask` menandatangani `stacklyd` dengan sertifikat code signing lokal yang stabil, sehingga signature yang dipercaya ACL tidak berubah antar build.

**Pros**:

- Izin cukup sekali.

**Cons**:

- Sertifikat baru di Keychain kamu dan langkah signing tambahan di setiap build.
- Tumpang tindih dengan keputusan identity di fitur 15, lalu dibuang setelahnya.

#### Option 3: ACL bebas di build debug

Item dibuat dengan ACL yang mengizinkan aplikasi apa pun di build debug.

**Pros**:

- Tidak ada dialog sama sekali di development.

**Cons**:

- Dua jalur kode untuk satu keputusan keamanan, dengan risiko jalur longgar ikut ke rilis.

### Keputusan 4: reveal dan copy

#### Option 1: Satu klik eksplisit dengan auto hide dan clipboard concealed

Show Password dan Copy seperti mockup. Nilai tersembunyi lagi setelah 30 detik, saat view hilang, atau saat app tidak aktif; clipboard memakai tipe concealed dan transient, hanya untuk host ini, dan dikosongkan setelah 90 detik. Setiap reveal dicatat.

**Pros**:

- Sesuai mockup dan PRD ("aksi eksplisit"), tanpa friksi palsu.
- Mengurangi nilai yang tertinggal di layar, clipboard manager, dan Universal Clipboard.

**Cons**:

- Orang yang memakai Mac yang tidak terkunci tetap bisa menekan Show.

#### Option 2: Touch ID atau password

Reveal dan copy meminta autentikasi ulang lewat LocalAuthentication di app.

**Pros**:

- Menahan orang lain di Mac yang tidak terkunci.

**Cons**:

- Bukan batas keamanan nyata: daemon tidak bisa memverifikasi klaim GUI, dan proses lain milik user yang sama tetap bisa meminta reveal lewat socket.
- Friksi di setiap copy connection string, aksi yang sering di JOURNEY-01.

#### Option 3: Satu klik tanpa auto hide

**Pros**:

- Paling sederhana.

**Cons**:

- Nilai tertinggal di layar dan clipboard tanpa batas.

### Keputusan 5: format secret reference

#### Option 1: `local:<path>`

Seperti contoh PRD (`local:erp/postgres/admin`), path huruf kecil 2 sampai 8 segmen, reference buatan daemon `local:instance/<id>/<purpose>`, skema lain dicadangkan.

**Pros**:

- Netral OS, jadi `stackly.yaml` yang sama berlaku di Mac lain dan nanti di Windows.
- Terbaca manusia dan cocok dengan contoh PRD.
- Skema memberi ruang untuk penyedia lain tanpa mengubah format.

**Cons**:

- Berbeda dengan teks di mockup (`keychain:...`).

#### Option 2: `keychain:<path>`

**Pros**:

- Jelas bagi pengguna Mac di mana nilainya tinggal.

**Cons**:

- Mengikat manifest tim ke macOS, padahal PRD bagian 23 melarang menganggap format secret macOS bisa dipakai di Windows.

#### Option 3: URI `secret://local/<path>`

**Pros**:

- Bentuk URI yang mudah diperluas lewat host.

**Cons**:

- Lebih panjang dan butuh aturan escaping URI, tanpa manfaat nyata dibanding skema sederhana.

#### Option 4: ID opaque (UUID)

**Pros**:

- Tidak pernah bentrok.

**Cons**:

- Tidak terbaca manusia, dan manifest butuh alias serta tabel pemetaan tambahan.

### Keputusan 6: nilai awal saat create instance

#### Option 1: Dibangkitkan daemon

32 karakter alfanumerik dari CSPRNG OS, langsung ke Keychain, dengan pilihan isi sendiri, salin, atau pakai reference yang ada secara eksplisit.

**Pros**:

- Sesuai mockup ("Generated on create, never logged") dan JOURNEY-01 tanpa langkah tambahan.
- Tidak ada password lemah atau dipakai ulang secara tidak sengaja.

**Cons**:

- Pengguna yang ingin password yang mudah diingat harus memilih Enter.

#### Option 2: Selalu minta pengguna

**Pros**:

- Pengguna selalu tahu nilainya.

**Cons**:

- Langkah tambahan di setiap create, dan rawan password lemah atau sama di semua instance.

### Keputusan 7: arti replace

#### Option 1: Simpan saja, tanpa rotasi

Replace memberi tahu Stackly nilai yang benar. Efeknya per instance dihitung dari klasifikasi field: `stored_only`, `next_reload`, atau `next_start`.

**Pros**:

- Menjawab kebutuhan nyata: setelah restore, pindah Mac, atau salah isi.
- Tidak butuh adapter per provider.

**Cons**:

- Pengguna harus memahami bahwa password di dalam database tidak ikut berubah.

#### Option 2: Termasuk rotasi

Replace juga mengubah credential di dalam provider.

**Pros**:

- Satu aksi untuk mengganti password sungguhan.

**Cons**:

- Butuh adapter per provider dan instance yang berjalan, bergantung fitur 19 dan 39 sampai 43, dan jauh di luar ukuran fitur foundation.

### Keputusan 8: saluran input CLI

#### Option 1: TTY, stdin, file, dan nama env var

**Pros**:

- Menutup semua pola script yang umum tanpa nilai di argv.
- Nama file atau nama variabel boleh terlihat, nilainya tidak.

**Cons**:

- Empat jalur yang harus diuji.

#### Option 2: TTY dan stdin saja

**Pros**:

- Paling sempit dan mudah diaudit.

**Cons**:

- Script harus menyusun redirect atau pipe sendiri.

#### Option 3: TTY, stdin, dan file

**Pros**:

- Menghindari env var yang bisa diwarisi child process.

**Cons**:

- Pola CI dan script yang sudah memegang secret di env harus menulisnya ke file lebih dulu.

### Keputusan 9: dependency Keychain

#### Option 1: `security-framework`

**Pros**:

- Wrapper aman yang paling banyak dipakai ekosistem Rust, lisensi MIT OR Apache-2.0.
- Mendukung `SecItem`, keychain file untuk test, dan saklar interaksi, tanpa `unsafe` di kode kita.

**Cons**:

- Satu dependency lagi beserta `core-foundation` transitif, dan API tingkat rendahnya mengikuti perubahan Apple.

#### Option 2: FFI sendiri

**Pros**:

- Kontrol penuh atas setiap atribut.

**Cons**:

- Banyak blok `unsafe` dan binding CoreFoundation yang harus dirawat sendiri.

#### Option 3: `keyring`

**Pros**:

- API lintas OS yang sederhana.

**Cons**:

- Tidak bisa menunjuk keychain file sementara dan menyembunyikan saklar interaksi, padahal keduanya wajib di sini.

### Keputusan 10: tipe nilai di memori

#### Option 1: Newtype di atas `zeroize`

`SecretValue` di platform dan `SecretText` di protocol, masing masing membungkus `Zeroizing<String>`.

**Pros**:

- Kontrol penuh atas trait yang ada dan tidak ada (tanpa `Display`, `Clone`, `Serialize` di sisi proses).
- Satu dependency kecil yang umum.

**Cons**:

- Dua tipe yang mirip, dipetakan di `stacklyd`.

#### Option 2: `secrecy`

**Pros**:

- Tipe siap pakai yang dikenal.

**Cons**:

- Dependency tambahan, dan tetap butuh tipe wire sendiri karena aturan serde dan typeshare protokol.

#### Option 3: `String` dengan `Debug` disamarkan

**Pros**:

- Tanpa dependency.

**Cons**:

- Nilai tertinggal di memori setelah dipakai.

### Keputusan 11: input TTY di CLI

#### Option 1: `rpassword`

**Pros**:

- Kecil, fokus, membaca dari `/dev/tty`, dan memulihkan terminal.

**Cons**:

- Satu dependency lagi untuk fungsi yang sempit.

#### Option 2: termios sendiri

**Pros**:

- Tanpa dependency.

**Cons**:

- Blok `unsafe` baru dan penanganan Ctrl+C serta pemulihan terminal sendiri.

### Keputusan 12: pemilik reference

#### Option 1: instance, environment, atau user

**Pros**:

- Purge instance bisa menghapus secret miliknya tanpa menyentuh secret yang dibagi.
- Restore dan env up tahu reference mana yang mereka buat.

**Cons**:

- Kolom pemilik polimorfik tanpa foreign key.

#### Option 2: Semua milik user

**Pros**:

- Model paling sederhana.

**Cons**:

- Purge meninggalkan item Keychain yatim yang harus dibersihkan manual.

### Keputusan 13: sumber kebenaran status

#### Option 1: Keychain, dengan cache di registry

**Pros**:

- Daftar secret cepat, backup registry bisa menandai secret yang hilang (AC-040), dan crash di antara dua tulisan pulih sendiri lewat probe.

**Cons**:

- Cache bisa basi sampai probe berikutnya.

#### Option 2: Selalu tanya Keychain

**Pros**:

- Tidak pernah basi.

**Cons**:

- Daftar lambat untuk banyak instance, dan backup registry tidak tahu apa pun.

#### Option 3: Registry saja

**Pros**:

- Paling cepat.

**Cons**:

- Salah setelah crash atau penghapusan lewat Keychain Access.

### Keputusan 14: pelacakan pemakaian

#### Option 1: Tabel `secret_reference_uses`

**Pros**:

- `usedBy`, larangan hapus, dan `resolve_for_start` menjadi query sederhana, dan foreign key menjaga larangan hapus.

**Cons**:

- Tabel tambahan yang harus selalu diperbarui bersama config revision dan environment.

#### Option 2: Turunkan dari config

**Pros**:

- Tanpa tabel baru.

**Cons**:

- Lebih lambat dan rawan terlewat untuk sumber pemakaian baru.

### Keputusan 15: permukaan CLI

#### Option 1: `list`, `status`, `set`, `reveal`, `delete`

**Pros**:

- Lengkap untuk alur isi, pemeriksaan, reveal eksplisit, dan pembersihan.

**Cons**:

- Lebih banyak command untuk diuji di milestone 2.

#### Option 2: `set` dan `status` saja

**Pros**:

- Minimal untuk Done when.

**Cons**:

- Reveal hanya lewat fitur 21 dan reference yatim tidak bisa dibersihkan.

### Keputusan 16: sumber nilai di `secret.set`

#### Option 1: `value`, `generate`, `copyFrom`

Di `secret.set`, ditambah `bind` di DTO bersama `SecretInput` khusus untuk resource baru (create instance dan env up), yang hanya menambah pemakaian ke reference yang sudah ada secara eksplisit.

**Pros**:

- Restore as new dan duplicate bisa menyalin nilai di daemon tanpa nilai melewati klien.
- Satu permintaan untuk semua alur.

**Cons**:

- Tiga jalur sumber yang harus diuji.

#### Option 2: `value` saja

**Pros**:

- Paling sempit.

**Cons**:

- Restore dan duplicate harus reveal lalu set ulang, jadi nilai melewati klien dan audit penuh reveal palsu.

### Keputusan 17: bentuk needs secret

#### Option 1: Blocker, bukan state

**Pros**:

- Menjaga pemisahan state di PRD bagian 19.1 dan tabel state spec 0004.
- Blocker bisa memuat daftar reference dan alasannya.

**Cons**:

- GUI dan CLI harus menampilkan dua hal (lifecycle dan blocker).

#### Option 2: State lifecycle baru

**Pros**:

- Langsung terlihat di kolom status.

**Cons**:

- Mencampur prasyarat dengan state proses, dan mengubah state machine yang sudah diuji menyeluruh.

### Keputusan 18: strategi redaction

#### Option 1: Tiga lapis

**Pros**:

- Menangkap kesalahan kode kita (lapis tipe), provider yang mencetak password di log nya (lapis nilai), dan URI atau pasangan kunci yang belum pernah dibaca daemon (lapis pola).

**Cons**:

- Pola bisa salah samar, dan pencocokan nilai menyimpan salinan nilai di memori daemon.

#### Option 2: Tipe saja

**Pros**:

- Tanpa salah samar.

**Cons**:

- Tidak menangkap apa pun yang dicetak provider sendiri.

#### Option 3: Tipe dan nilai yang dikenal

**Pros**:

- Lebih sedikit salah samar.

**Cons**:

- URI dengan password yang belum pernah dibaca daemon lolos.

### Keputusan 19: nilai pendek

#### Option 1: Terima dengan peringatan

**Pros**:

- Instance hasil restore atau data lama dengan password pendek tetap bisa diisi.

**Cons**:

- Nilai di bawah 8 byte hanya dilindungi lapis tipe dan pola.

#### Option 2: Tolak di bawah 8 byte

**Pros**:

- Redaction lengkap terjamin.

**Cons**:

- Pengguna terkunci dari datanya sendiri setelah restore.

#### Option 3: Cocokkan semua panjang

**Pros**:

- Tidak ada pengecualian.

**Cons**:

- Nilai seperti `root` menyamarkan kata itu di seluruh log.

### Keputusan 20: batas tunggu antrean Keychain

#### Option 1: 180 detik untuk aksi pengguna, 10 detik untuk pemicu latar

**Pros**:

- Pengguna punya waktu mengetik password Keychain, dan autostart tidak pernah tertahan lebih dari 10 detik.

**Cons**:

- Tulisan yang lewat batas bisa tetap terjadi setelah dialog dijawab, jadi pengguna harus mengecek status.

#### Option 2: Tanpa batas

**Pros**:

- Tidak ada hasil ambigu.

**Cons**:

- Autostart bisa tertahan selama dialog dibiarkan terbuka.

#### Option 3: 60 detik untuk semua

**Pros**:

- Satu angka.

**Cons**:

- Terlalu pendek untuk pengguna yang lambat, terlalu panjang untuk pemicu latar.

### Keputusan 21: penjaga di kode

#### Option 1: Allowlist di `check-deps`

**Pros**:

- Pemakaian `expose()` baru gagal otomatis dengan file dan baris, memakai pindaian `syn` yang sudah ada.

**Cons**:

- Allowlist harus diperbarui lewat update spec saat ada modul baru yang sah.

#### Option 2: Code review saja

**Pros**:

- Tanpa aturan tambahan.

**Cons**:

- Kebocoran baru hanya tertangkap kalau reviewer melihatnya.

## Rationale

Option 1 dipilih di semua keputusan, dan semuanya kamu setujui di wawancara. Benang merahnya adalah PRD bagian 23: Keychain, file sementara yang dibatasi, redaction, dan tanpa reveal default, dengan residual risk yang dinyatakan terang. Keychain menjadi satu satunya sumber nilai karena registry, manifest, lockfile, dan backup semuanya dirancang untuk dibagikan atau disalin (REQ-016, BR-018, AC-040). Daemon menjadi satu satunya pembaca karena hanya daemon yang menjalankan proses yang membutuhkan nilai itu (BR-011), dan karena item Keychain mempercayai signature pembuatnya, satu pembaca berarti satu signature yang perlu dipercaya pengguna.

Kebijakan dialog dan signing ad hoc saling mengunci. Tanpa identity nyata, setiap build ulang membuat item lama tidak terbaca tanpa izin baru, jadi kebijakan "tidak pernah ada dialog" akan membuat development macet, sedangkan "selalu boleh" melanggar AC-062 untuk autostart. Memisahkan aksi pengguna dari pemicu latar memberi keduanya: dialog muncul tepat saat pengguna sedang melihat, dan pemicu latar langsung menjadi needs secret yang bisa ditindaklanjuti. Karena saklar interaksi berlaku untuk seluruh proses dan dialog menahan thread, semua akses dijalankan berurutan di satu thread dengan batas tunggu, sehingga satu dialog yang terbuka tidak pernah membuat autostart menunggu lebih dari 10 detik. Data protection keychain tetap opsi yang lebih baik setelah fitur 15 memasang identity dengan Team ID, dan adapter sengaja disembunyikan di balik port supaya perpindahan itu tidak mengubah core.

Untuk OQ-008, menolak Valkey dan NATS bukan pilihan karena keduanya provider wajib, jadi plaintext dibatasi pada waktu dan tempat: hanya selama instance berjalan, hanya di runtime root yang tidak ikut Time Machine atau backup apa pun, dan selalu dibuat ulang dari Keychain. Redaction tiga lapis dipilih karena kebocoran nyata di sistem seperti ini jarang datang dari kode kita sendiri; ia datang dari provider yang mencetak connection string atau config ke log, dan hanya pencocokan nilai yang dikenal yang menangkapnya. Format `local:<path>` dipilih di atas teks mockup karena manifest yang dibagikan harus tetap berlaku di Windows, dan scope serta spec menang atas mockup untuk perilaku. Reveal tanpa Touch ID dipilih karena autentikasi di GUI tidak bisa dibuktikan ke daemon, sehingga hanya menambah friksi tanpa menambah batas keamanan; auto hide, clipboard concealed, dan audit memberi perlindungan yang nyata terhadap kebocoran tak sengaja.

### Keputusan kecil

Keputusan berikut diambil saat menulis spec, masing masing dengan alternatif terdekatnya:

- **Login Keychain lewat `SecItem`, bukan data protection keychain**: bisa dipakai sekarang dengan signing ad hoc. Alternatif: data protection keychain, yang butuh entitlement dan Team ID; dibahas ulang di fitur 15.
- **Service `stackly-secret:<namespace>`, bukan bundle ID**: bundle ID masih sementara (OQ-001), dan mengganti bundle ID tidak boleh membuat semua secret hilang. Alternatif: bundle ID sebagai service.
- **Root override memakai keychain file sendiri**: dengan `STACKLY_ROOT=R`, daemon memakai `R/data/keychain/stackly.keychain-db` dengan password di file 0600, jadi root development dan suite sentinel tidak pernah menyentuh login Keychain, dan jalur kode yang sama dipakai integration test adapter. Alternatif: namespace service per root di login Keychain (rancangan awal), yang tetap menulis ke login Keychain saat test.
- **`SecretWorker` di core, ditambah mutex di adapter**: logika antrean dan batas tunggu portabel dan bisa diuji dengan `FakeSecretStore`, sedangkan mutex adapter menjaga saklar interaksi global walau adapter dipanggil di luar worker. Alternatif: worker di dalam adapter, yang hanya bisa diuji di macOS dengan Keychain nyata.
- **Interaksi Security framework mati sebagai default proses**: panggilan yang tersasar tidak pernah memunculkan dialog; hanya `AllowPrompt` yang menyalakannya sebentar. Alternatif: mematikan per panggilan `Never`, yang membiarkan panggilan lain memunculkan dialog.
- **`state()` dari cache**: `secret.list` dan `DaemonInfo.secretStore` tidak pernah tertahan di belakang dialog. Alternatif: membaca status kunci setiap kali, yang harus antre di worker.
- **Job kedaluwarsa dilewati, tulisan yang lewat batas menjadi `OutcomeUnknown`**: pemanggil tahu persis apakah tulisannya pernah dijalankan. Alternatif: satu `Busy` untuk semuanya, yang membuat hasil tulisan ambigu.
- **Replace membaca nilai lama dan menulisnya balik kalau transaksi registry gagal**: credential yang cocok dengan database tidak hilang karena SQLite sibuk. Alternatif: menyimpan nilai lama sebagai item Keychain kedua, yang menggandakan secret yang tersimpan.
- **Delete menghapus registry dulu, baru Keychain**: foreign key menangkap pemakaian yang baru ditambahkan, dan crash hanya meninggalkan item yatim yang tidak dipakai siapa pun. Alternatif: Keychain dulu, yang bisa meninggalkan reference terpakai tanpa nilai.
- **Nilai 8 sampai 15 byte hanya dicocokkan sebagai token utuh, dan argv hanya dicocokkan sebagian untuk nilai 16 byte atau lebih**: password seperti `postgres` tidak merusak `postgresql` di log atau menolak path di argv. Alternatif: substring untuk semua nilai 8 byte atau lebih, yang salah tangkap pada password kata kamus.
- **Setelah pengguna menolak satu dialog, resolve berhenti meminta**: pengguna tidak dihujani dialog untuk setiap reference. Alternatif: mencoba setiap reference, yang memunculkan dialog sebanyak jumlah reference.
- **Audit reveal gagal tertutup**: tidak ada nilai yang keluar tanpa jejak. Alternatif: reveal tetap jalan dan audit dicoba ulang, yang bisa meninggalkan reveal tanpa catatan.
- **File secret di runtime root, bukan `instances/<id>/config/`**: runtime root tidak ikut Time Machine, backup registry, cold backup, atau `config-history`, dan memang hanya berisi file yang bisa dibuat ulang. Alternatif: folder config dengan daftar pengecualian di setiap fitur backup, yang mudah terlewat.
- **`getrandom` untuk CSPRNG**: kecil, sudah ada di graph lewat `uuid`, dan portabel. Alternatif: `rand`, yang jauh lebih besar untuk kebutuhan satu fungsi.
- **`regex` untuk pola redaction**: crate yang membosankan dan teruji, waktu linear tanpa backtracking, dan pola tetap terbaca di review. Alternatif: scanner tulisan tangan, lebih cepat sedikit tetapi lebih sulit diaudit.
- **Pencarian substring sederhana untuk nilai yang dikenal**: jumlah nilai per daemon ratusan, bukan ribuan. Alternatif: `aho-corasick`, ditambahkan hanya kalau pengukuran fitur 64 menunjukkan log melambat.
- **Penanda `****` dengan panjang tetap**: muat di dalam URI dan tidak membocorkan panjang nilai. Alternatif: `[redacted]`, lebih mudah dicari tetapi merusak bentuk URI.
- **Blocker needs secret di memori, tidak dipersist**: dihitung ulang di setiap percobaan start, jadi tidak butuh kolom atau migrasi. Alternatif: kolom registry, yang bisa basi setelah pengguna membuka kunci Keychain di luar Stackly.
- **Batas nilai 64 KiB**: kunci privat dan rantai sertifikat PEM muat untuk alur sertifikat Traefik nanti, jauh di bawah frame 1 MiB. Alternatif: 4 KiB, cukup untuk password tetapi tidak untuk sertifikat.
- **Pasteboard `currentHostOnly` plus tipe concealed dan transient**: mencegah Universal Clipboard dan riwayat clipboard manager yang menghormati konvensi itu. Alternatif: concealed saja.
- **Nilai contoh `not-a-real-secret` di fixture kontrak**: fixture adalah file statis, sedangkan fixture credential dibuat ulang setiap run dan prefixnya dipindai wrapper. Alternatif: tanpa fixture untuk DTO secret, yang membuat kontrak Swift tidak teruji.
- **Probe lewat atribut saja**: tidak pernah memicu dialog. Alternatif: membaca nilai, yang memicu dialog setelah build ulang ad hoc.
- **Konfirmasi dua kali di prompt TTY untuk nilai baru**: salah ketik pada password yang tidak pernah terlihat membuat instance tidak bisa dipakai. Alternatif: sekali, lebih cepat.
- **`--from-file` hanya memberi peringatan untuk file yang terbaca group atau other**: pemiliknya tetap user yang sama dan menolak akan mematahkan script yang sudah ada. Alternatif: menolak file itu.
- **`stackly secret reveal` mencetak nilai diikuti newline**: cocok untuk `$(...)` di shell, yang membuang newline di akhir. Alternatif: tanpa newline, yang mengacaukan prompt terminal.

## Cross check

Draft spec dibaca ulang oleh model lain (Sonnet 5.5) secara read only pada 2026-10-18. Vonisnya "belum siap dibangun", dengan 15 celah keputusan dan 9 catatan soundness. Kamu memilih menerapkan semua perbaikan yang direkomendasikan:

- **Jalur nilai saat create, env up, dan restore**: DTO bersama `SecretInput` (dengan `bind` untuk resource baru) dan satu use case `SecretService::put` dengan `PutTarget`. AC-5 kini menagih use case yang sama, bukan permintaan yang sama.
- **Test dan root override menyentuh login Keychain**: root override memakai keychain file sendiri, namespace service per root dihapus, dan `store_key` menjadi sumber `platform_store_key`.
- **`state()` dan status kunci**: `state()` dari cache dengan nilai `Unknown`, probe lewat worker, dan satu blok `unsafe` diizinkan hanya untuk status kunci kalau wrapper tidak ada.
- **Urutan `put`**: registry dan revision dicek dulu, mode dicek terhadap Keychain, revision mulai 1 dan tidak naik karena observasi, konfirmasi `stored_only`, dan penulisan balik nilai lama.
- **Aturan pemilik dan turunan purpose**: pemilik tetap sejak dibuat (pindah ke `user` saat pemiliknya dihapus tetapi reference masih dipakai), dan algoritma purpose yang normatif.
- **Grammar `Redactor` dan batas salah tangkap**: grammar kunci, nilai berkutip, `authorization`, dan userinfo URI yang normatif, token utuh untuk nilai 8 sampai 15 byte, dan ambang 16 byte di argv.
- **Worker**: `WorkerConfig`, job kedaluwarsa dilewati, dan `OutcomeUnknown` diikuti probe.
- **`resolve_for_start` dan blocker**: selalu membaca, berhenti meminta setelah satu penolakan, error backend menang, urutan teks, field `blocker` di DTO instance, dan aturan kapan blocker hilang.
- **Pengiriman**: nama file, guard `SecretFile`, `hash` dan payload di `secret::delivery`, serta tambahan spec 0004 masuk Build plan langkah 1.
- **Audit**: kosakata `result`, kegagalan ikut dicatat, audit `put` dalam transaksi yang sama, dan reveal gagal tertutup.
- **CLI dan GUI**: konfirmasi TTY tiga kali, definisi interaktif, `SecretEntrySheet` dengan `create` lalu tawaran replace, `RevealedSecret` di Swift, status `deleted` di event, dan pasteboard dikosongkan saat app ditutup.
- **Soundness**: pemetaan `OSStatus` sebagai fungsi murni yang dikonfirmasi spike dan record manual, interaksi default mati dengan mutex di adapter (aturan "konstruktor hanya lewat worker" dihapus karena `stackly-platform-macos` tidak boleh bergantung pada core), pindaian `expose` lewat token stream termasuk makro dengan `stackly-testkit` di allowlist, pengecualian frame `SecretInput` di AC-16, patokan keychain eksplisit, `learn` sebelum tulisan pertama, lock per reference untuk probe, urutan delete registry dulu, dan penjelasan bahwa ACL Keychain tidak melindungi akses lewat daemon.

Hasil spike `kSecUseAuthenticationUI` dan kode `OSStatus` nyata di langkah 3 Build plan dicatat di bagian ini lewat `/architect` update spec.

## Perubahan 2026-10-18 (rekonsiliasi lintas spec)

Pengecekan silang Assumed lines antara spec 0005, 0006, 0009, 0011, dan 0012 menemukan selisih yang keputusannya milik spec ini. Engineer menyetujui semua perbaikan berikut sebelum fitur 11 mulai dibangun.

| Selisih | Perbaikan | Alasan | Runner up |
| --- | --- | --- | --- |
| Kode `needs_secret` tanpa area, area `secret_store` tidak terdaftar, dan kode belum punya kategori | `secret.needs_secret` dan `secret.store_*`, tabel *Kode error di wire* dengan kategori, `details`, dan `data` | pola kode spec 0006 mewajibkan `<area>.<alasan>` di area terdaftar, dan exit code CLI berasal dari kategori | mendaftarkan area `secret_store` |
| Revision basi dijawab `secret.conflict` | `resource.revision_mismatch` (spec 0006) | client memperlakukan semua konflik CAS dengan satu resep | kode sendiri di area `secret` |
| Daftar reference di detail needs secret tidak muat di `details` string | `ErrorBody.items` (spec 0006) dengan `kind` `reference`, `use`, dan `effect`, paling banyak 100 item dan sisanya di `details.omitted` | daftar tetap terstruktur untuk CLI dan `SecretEntrySheet`, dan item berbasis string selalu terbaca di Swift | enum `ErrorData` terbuka (ditolak cross check: tidak bisa didecode aman) |
| `daemon.status` tidak ada | field opsional `DaemonInfo.secretStore` | `daemon.get_info` sudah memuat info daemon lain | subsystem `secret_store` di `DaemonHealth` |
| `generate` selalu 32 karakter, padahal secret field spec 0009 punya `minLength` dan `maxLength` | panjang dijepit ke batas field, dan `Value` atau `CopyFrom` di luar batas gagal `secret.invalid_value` | nilai bangkitan yang melanggar schema provider akan ditolak provider saat init | batas field diabaikan |
| `shortValueNotScrubbed` camelCase dan `expectedRevision` bertipe `u64` | `short_value_not_scrubbed` dan `Revision` | aturan wire spec 0006 (nilai enum snake_case, `u64` lewat newtype) | |
| Redaksi pesan error belum pasti jalan sebelum pemotongan 200 karakter, dan detail readiness di reply DTO belum diredaksi | redaksi sebelum `ErrorBody::new` memotong pesan; detail observasi lewat `Redactor` saat dicatat | potongan nilai bisa lolos lapis 2; reply DTO adalah jalur keluar yang sama pentingnya dengan event | redaksi hanya di event |
| Asumsi token tersamar dari spec 0012 dan mekanisme pengiriman yang dideklarasikan per field di spec 0009 | `SecureField` native dan token teks yang ada; mekanisme mengikuti posisi token di manifest | spec 0012 tidak me restyle kontrol native, dan posisi token sudah menentukan mekanisme | token baru di spec 0012 |
| Port keenam lewat argumen baru di `Platform::new` | `Platform::with_secrets` dengan default `UnsupportedSecretStore` (spec 0004 keputusan 18) | konstruktor tidak patah saat fitur lain menambah port | argumen keenam |

Cross check model lain atas update ini menambah, atas persetujuan engineer: AC-20 (operasi hapus dibangun fitur ini sebagai satu satunya pembangun, `remove_file` untuk entri apa pun yang bukan folder termasuk socket), AC-21 (katalog kode), langkah 0 (update spec 0005 untuk method baca, `ResourceRef`, dan `WriteOp` secret dan audit sebelum langkah core), `ResolvedSecrets::for_tests` di balik fitur `test-support`, lokasi `password_file`, nilai `actor` audit, dan sumber waktu observasi blocker.

Ronde 2 rekonsiliasi (2026-10-18), atas persetujuan engineer: semua penghapusan lewat `remove_owned_tree` spec 0007 (AC-20 ditulis ulang; fitur ini tidak lagi membangun `remove_file` dan `remove_empty_dir`), instance `unknown` dihitung berjalan saat sweep dan `learn_running`, dan blocker satu slot dengan prioritas spec 0010.

