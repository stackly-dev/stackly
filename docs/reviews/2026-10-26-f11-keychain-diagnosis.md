# F11, diagnosis Deny dan penutupan bukti Keychain

Tanggal: 26 Oktober 2026. Sumber: `252e35d130e3867681aca825bc62da05edbf9077`, branch `feat/secret-store-keychain-evidence`. Mac13,1, Apple M1 Max, macOS 27.0.1 (26A434), Rust 1.99.1, Xcode 27.1. Tindakan dialog dilakukan pengguna.

## Kesimpulan

Hasil awal C yang dilaporkan sebagai Deny ternyata merupakan **pemberian izin menurut securityd**. macOS mencatat Always Allow untuk path dan PID C dalam jendela permintaan tersebut. Audit berhasil, CLI menerima nilai, dan ACL kemudian mempercayai C. Urutan ini menjelaskan nilai yang dikembalikan tanpa mengasumsikan Stackly mengabaikan penolakan Keychain. Tidak ditemukan bug runtime pada jalur baca atau pemetaan penolakan.

Laporan pengguna bahwa Deny ditekan tetap disimpan. Log OS tidak membuktikan tombol fisik mana yang diingat pengguna, tetapi membuktikan keputusan yang diterima API pada permintaan itu. Percobaan C bukan bukti negatif Deny. Deny dan Escape yang dikonfirmasi pengguna pada item baru memiliki penolakan OS yang sesuai dan tidak mengembalikan nilai. Kedua kasus tersebut memenuhi AC-3.

## Korelasi yang membedakan penyebab

Waktu tabel dalam WIB (UTC+7). Audit registry dan CLI menggunakan UTC; korelasi dilakukan sebagai waktu berzona, bukan perbandingan teks.

| Permintaan | Bukti OS dalam jendela permintaan | Audit dan CLI | Verdict |
| --- | --- | --- | --- |
| B, PID 25493, 25 Oktober | Prompt 08:32:42.891861; Always Allow 08:32:47.724430; Allow 08:32:47.726816 | Audit succeeded 08:32:47.744286; CLI exit 0, ada nilai | Pemberian izin. Mengoreksi laporan awal tidak ada dialog dan dugaan migrasi trust otomatis |
| C, PID 48839, 25 Oktober | Prompt 11:53:03.024550; Always Allow 11:53:35.391007; Allow 11:53:35.393292 | Audit succeeded 11:53:35.411075; CLI exit 0, ada nilai, selesai 11:53:35.427427 | Pemberian izin OS, meski tindakan dilaporkan sebagai Deny |
| D, PID 72560, Escape, 26 Oktober | Prompt 01:00:08.525153; nonapproval 01:00:59.530795; ACL reject 01:00:59.531069 | Audit failed 01:00:59.531304; exit 4, `secret.store_access_denied`, `byUser=true`, tanpa nilai | PASS, 51,040 detik |
| D, PID 72560, Deny, 26 Oktober | Prompt 01:01:35.226890; nonapproval 01:02:20.559758; ACL reject 01:02:20.560226 | Audit failed 01:02:20.569245; exit 4, kode sama, `byUser=true`, tanpa nilai | PASS, 45,435 detik |
| D, noninteractive setelah Deny | Tidak ada prompt; ACL reject 01:04:30.153980 | Audit failed 01:04:30.154090; exit 4, `byUser=false`, tanpa nilai | PASS, 0,057 detik |

B/C memakai `local:user/f11-dialog-fcca168f48d6`; D negatif memakai item baru `local:user/f11-escape-fcca168f48d6`. Prompt memuat item yang sesuai dan path/PID binary pembaca. Setiap jendela memiliki tepat satu audit reveal untuk reference itu. Setelah penolakan D, ACL masih hanya mempercayai A. Kedua item uji sudah dihapus, list kosong, daemon uji berhenti.

[Cuplikan securityd, hasil CLI tanpa nilai, audit, dan SHA256 binary](evidence/2026-10-26-f11-keychain-correlation.json) disimpan dalam repo agar kesimpulan tidak bergantung pada folder `target/` yang diabaikan Git. Tidak ada plaintext secret atau dump Keychain di artefak tersebut.

## Hipotesis dan pemeriksaan kode

- Bypass error oleh daemon akan menghasilkan penolakan OS diikuti audit sukses atau nilai. Bukti C justru menunjukkan izin eksplisit OS; D menunjukkan penolakan, audit gagal, dan respons tanpa nilai.
- Respons tertukar antara proses atau item akan menghasilkan perbedaan PID, path, item, atau audit dalam jendela waktu. Kelima jendela cocok dengan pemanggil dan item masing masing.
- Kepercayaan yang berubah karena izin OS akan menghasilkan Always Allow, kemudian pembacaan noninteractive berhasil dan ACL bertambah. Prediksi ini cocok dengan B dan C. Kesamaan legacy hash A/B saja tidak membuktikan migrasi trust otomatis.

`security-framework` 3.7.0 menjalankan satu `SecItemCopyMatching` dan mengembalikan error lewat `cvt(...)?`. `MacosSecretStore` dan `KeychainGuard` membatasi interaksi lalu memetakan OSStatus; `SecretService::reveal` meneruskan kegagalan worker dengan audit gagal; handler IPC memakai `?`. Tidak ada retry yang menukar penolakan menjadi pembacaan cache. Rujukan kode: `crates/platform-macos/src/secret.rs`, `crates/core/src/secret/service/access.rs`, dan `apps/daemon/src/ipc/secret.rs`.

Implementasi [ACL securityd dari Apple](https://github.com/apple-oss-distributions/Security/blob/main/securityd/src/acl_keychain.cpp#L227-L258) mengaitkan event Always Allow dengan jawaban yang mengingat izin dan penambahan aplikasi tepercaya. Ini mendukung interpretasi event OS; bukti utama tetap log aktual Mac ini.

## Perbaikan dan verifikasi ulang

Perbaikan terdapat pada prosedur dan klasifikasi bukti. [Manual bagian A](../manual.md#a-deny-dan-cancel-di-login-keychain) kini mensyaratkan salinan binary tetap, preflight noninteractive yang benar benar ditolak, status keluar proses CLI langsung, identitas/PID/jendela waktu, dan korelasi OS dengan audit. `touch` atau identifier baru tidak lagi dianggap menjamin dialog. `cargo run` tidak dipakai setelah membangun varian karena dapat membangun identitas lain. Record Deny/Cancel menyimpan riwayat FAIL dan penyelesaiannya; record ad hoc menghapus kesimpulan lama tentang B yang tidak didukung log OS. Tidak ada patch runtime spekulatif.

Loop awal `python3 target/f11-keychain-proof/check_denial_evidence.py` menghasilkan FAIL untuk C (exit 0, ada nilai), PASS untuk Deny/Escape D. Sesudah input tindakan dikorelasikan dengan keputusan OS, pemeriksa yang dapat dijalankan ulang adalah:

```bash
python3 docs/reviews/evidence/verify_f11_keychain.py
```

Hasil: lima korelasi PASS. Kontrol negatif yang mengubah respons Deny D menjadi exit 0 dan ada nilai ditolak. Pemeriksa ini memutar ulang bukti native yang sudah ditangkap, tidak menekan dialog baru atau menggantikan tindakan pengguna.

Regresi sesi diagnosis lulus, 184 test Rust unik dan 96 test Swift. Satu known issue Swift adalah assertion negatif `FixtureCredential.assertAbsent` yang memang sengaja ditangkap, bukan kegagalan baru.

| Perintah | Hasil |
| --- | --- |
| `cargo test -p stackly-platform-macos --lib secret` | 3 lulus, tabel pemetaan status dan perilaku secret |
| `cargo test -p stackly-platform-macos --test secret_store` | 5 lulus, keychain file nyata di TestRoot |
| `cargo test -p stackly-domain --lib secret` | 22 lulus |
| `cargo test -p stackly-core --lib secret` | 41 lulus |
| `cargo test -p stackly-core --test secret_service --test secret_delivery` | 73 + 10 lulus |
| `cargo test -p stacklyd --test secret_sentinel --test secret --test cli_secret` | 3 + 1 + 1 lulus |
| `cargo test -p stacklyd --test server secret` | 13 lulus |
| `cargo test -p stackly-cli --test secret` | 12 lulus |
| `swift test --package-path clients/swift --filter 'SecretsClientTests\|RevealedSecretTests\|FixtureCredentialTests'` | 19 lulus, termasuk 6 fixture dengan satu known issue yang disengaja |
| `swift test --package-path apps/macos/Packages/StacklyUI --filter 'Secret\|Reveal\|Concealed'` | 77 lulus |
| `cargo xtask check-deps` | 13 crate lulus |
| `cargo xtask codegen --check` | 20 file sesuai |

Log regresi lokal ada di `target/f11-keychain-proof/reverify/`. Suite platform semula terfilter menjadi satu integration test, lalu diulang tanpa filter agar kelima test benar benar berjalan; satu test duplikat itu tidak dihitung lagi. Tidak ada perubahan produk setelah bukti native D, sehingga dialog yang sudah dikonfirmasi pengguna tidak perlu diulang lagi.

## Batas penutupan F11

AC-1 sampai AC-24 dipetakan pada [verify.md](../specs/0011-secret-store-redaction/verify.md): domain dan port, adapter, service, worker, delivery, redaction, audit/IPC, CLI, serta Swift/GUI sudah mempunyai bukti masing masing. Celah terakhir AC-3 diselesaikan oleh korelasi ini. Cakupan GUI yang berlaku mengikuti [rekonsiliasi 24 Oktober](2026-10-24-f11-interaction-policy-followup.md); interaksi khusus yang dicabut pengguna tetap RETIRED. Konfirmasi `stored_only` pada layar produk tetap milik fitur 20/21 sesuai penundaan yang sudah disetujui.

F11 ditutup pada batas fondasi ini setelah `cargo xtask evidence check` lulus (63 record manual, baseline valid, 801 file Rust dan 517 file Swift sesuai aturan). Build it, milestone daemon, dan Verify it dicentang; scope menjadi done dan status spec Accepted. Bukti identity development nyata, consumer fitur mendatang, dan follow-up lintas fitur dalam spec tetap terbuka pada pemiliknya. Teks persis seluruh dialog dan daftar tombol tidak diberikan; hanya tindakan dan identitas yang dikonfirmasi serta keputusan OS dicatat. Riwayat timeout 180 detik tetap timeout, tidak diubah menjadi bukti Cancel.
