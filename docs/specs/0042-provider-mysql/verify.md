# Verify: Provider MySQL · spec 0042 · updated 2026-11-12

_Langkah diturunkan dari acceptance criteria spec 0042. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Bagian Milestone 1 di bawah baru memuat langkah 1 Build plan (format `mysqlCnf` di core); developer paket menambahkan langkah paket, golden, dan spike ke bagian yang sama._

## Milestone 1: Format, paket, dan spike dari paket

### UI / manual

- [ ] Buka `docs/providers/manifest-reference.md` → baris `format` di tabel `configFiles` menyebut `mysqlCnf` (kutip seperti `valkeyConf`, boolean `ON` dan `OFF`) → AC-2

### Commands

- [ ] `cargo test -p stackly-core --lib provider::template::tests::mysql_cnf` → 3 test lulus: `a"b\c d # e ü 空 'q' ; z` menjadi `"a\"b\\c d # e ü 空 'q' ; z"`, `$` dan backtick apa adanya, setiap karakter kontrol (U+0000 sampai U+001F, U+007F sampai U+009F) ditolak dan U+00A0 diterima, boolean `ON` dan `OFF`, tag diterima, output tidak diparse ulang, dan `from_code("mysqlCnf")` mengembalikan varian baru (`mysql` dan `MysqlCnf` tidak) → AC-2
- [ ] `cargo test -p stackly-core --lib secret::delivery::tests::a_placeholder_renders_with_the_escaper_of_its_format` → `render_secret_placeholder` memakai escaper `mysqlCnf` dan menolak secret berisi DEL → AC-2
- [ ] `cargo test -p stackly-core --test provider_rules mysql_cnf_renders_quoted_text_decimal_integers_and_on_off_booleans` → fixture fake dengan `format: mysqlCnf` merender `general_log_file` dengan teks AC-2, integer desimal, boolean `ON`/`OFF`, dan secret lewat `materialize`; secret dan path data dengan karakter kontrol gagal `UnrepresentableValue` dengan nama field, tanpa nilainya → AC-2
- [ ] `cargo test -p stackly-core --test provider_rules an_unknown_format_names_every_known_format_in_its_message` → pesan `unknown_template_format` menyebut `mysqlCnf` → AC-2
- [ ] `cargo test -p stackly-core --test provider_compat` → corpus beku lulus tanpa satu pun `frozen.json` berubah → AC-2 (syarat additive 3)
- [ ] `cargo test -p stackly-core --test fuzz_regressions` dan `cargo test -p xtask fuzz` → seed `fuzz/seeds/provider_template/format.mysql.cnf.tmpl` (selector 7, setup `mysqlCnf`) lulus harness tanpa panic, dan `cargo xtask fuzz seed` tidak mengubah `fuzz/seeds/` → AC-2 (syarat additive 6)

### Acceptance-criteria coverage

- AC-2 · format `mysqlCnf` di core, tanpa round trip native; round trip `my_print_defaults` runtime 8.4.11 menunggu langkah native spec ini
- AC-1, AC-3 sampai akhir · belum di langkah 1; milik langkah paket Milestone 1 dan milestone berikutnya
