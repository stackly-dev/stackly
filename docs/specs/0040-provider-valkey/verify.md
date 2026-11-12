# Verify: Provider Valkey · spec 0040 · updated 2026-11-12

_Langkah diturunkan dari acceptance criteria spec 0040. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Bagian Milestone 1 di bawah baru memuat langkah 1 dan 2 Build plan (perubahan core); developer paket menambahkan langkah paket, golden, dan spike ke bagian yang sama._

## Milestone 1: Perubahan core dan paket, dibuktikan di fixture

### UI / manual

- [ ] Buka `docs/providers/manifest-reference.md` → baris `format` di tabel `configFiles` menyebut `valkeyConf` (teks berkutip ganda dengan `\` dan `"` di escape, boolean `yes` dan `no`) → AC-2

### Commands

- [ ] `cargo test -p stackly-core --lib provider::template::tests::valkey_conf` → 3 test lulus: `a"b\c d ü #x` menjadi `"a\"b\\c d ü #x"`, `$`, `'`, backtick, dan kurung kurawal keluar apa adanya, setiap karakter kontrol (U+0000 sampai U+001F, U+007F sampai U+009F) ditolak dan U+00A0 diterima, boolean `yes` dan `no`, tag diterima, output tidak diparse ulang, dan `from_code("valkeyConf")` mengembalikan varian baru (kode lain dengan huruf berbeda tidak) → AC-2
- [ ] `cargo test -p stackly-core --lib secret::delivery::tests::a_placeholder_renders_with_the_escaper_of_its_format` → `render_secret_placeholder` memakai escaper `valkeyConf` dan menolak secret berisi newline → AC-2
- [ ] `cargo test -p stackly-core --test provider_rules valkey_conf_renders_quoted_text_decimal_integers_and_yes_no_booleans` → fixture fake dengan `format: valkeyConf` merender teks berkutip, integer desimal, boolean `yes`/`no`, dan secret lewat `materialize`; secret dan path data dengan karakter kontrol gagal `UnrepresentableValue` dengan nama field, tanpa nilainya → AC-2
- [ ] `cargo test -p stackly-core --test provider_rules an_unknown_format_names_every_known_format_in_its_message` → `format: ini` ditolak `unknown_template_format`, dan pesannya menyebut `valkeyConf` bersama setiap format lain yang dikenal → AC-2
- [ ] `cargo test -p stackly-core --test provider_compat` → corpus beku lulus tanpa satu pun `frozen.json` berubah (`git status crates/core/tests/fixtures/compat` bersih) → AC-2 (syarat additive 3)
- [ ] `cargo test -p stackly-core --test fuzz_regressions` dan `cargo test -p xtask fuzz` → seed `fuzz/seeds/provider_template/format.valkey.conf.tmpl` (selector 6, setup `valkeyConf`) lulus harness tanpa panic, dan `cargo xtask fuzz seed` tidak mengubah `fuzz/seeds/` → AC-2 (syarat additive 6)

### Acceptance-criteria coverage

- AC-2 · format `valkeyConf` di core, tanpa round trip native; round trip `CONFIG GET proc-title-template` pada Valkey 9.1.2 menunggu Milestone 2
- AC-1, AC-3 sampai AC-14 · belum di langkah 1; AC-3 di langkah 2, sisanya milik langkah paket Milestone 1 dan Milestone 2 sampai 4
