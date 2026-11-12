# Verify: Provider ClickHouse · spec 0043 · updated 2026-11-12

_Langkah diturunkan dari acceptance criteria spec 0043. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Bagian Milestone 1 di bawah baru memuat langkah 1 Build plan (format `xml` di core). Langkah 2 (filter `sha256Hex`) dibangun spec 0040 langkah 2 (commit `0d111375`); test AC-2 nya tercatat di `docs/specs/0040-provider-valkey/verify.md` Milestone 1 dan dipakai di sini oleh test `xml` di bawah. Developer paket menambahkan langkah paket dan golden ke bagian yang sama._

## Milestone 1: core dan paket (tanpa binary)

### UI / manual

- [ ] Buka `docs/providers/manifest-reference.md` → baris `format` di tabel `configFiles` menyebut `xml` (tanpa kutip, `&`, `<`, `>`, `"`, dan `'` menjadi entity, contoh `<path>{{ instance.data }}/</path>`) → AC-1

### Commands

- [ ] `cargo test -p stackly-core --lib provider::template::tests::xml` → 3 test lulus: `a&b<c>d"e'f ü 空 $x {y}` menjadi `a&amp;b&lt;c&gt;d&quot;e&apos;f ü 空 $x {y}` tanpa kutip, `&amp;` di escape lagi, setiap karakter kontrol (U+0000 sampai U+001F, U+007F sampai U+009F), U+FFFE, dan U+FFFF ditolak sedangkan U+00A0, U+FFFD, dan emoji diterima, boolean `true` dan `false`, tag diterima, output tidak diparse ulang, `from_code("xml")` mengembalikan varian baru (`XML` tidak) → AC-1
- [ ] `cargo test -p stackly-core --lib secret::delivery::tests::a_placeholder_renders_with_the_escaper_of_its_format` → `render_secret_placeholder` memakai escaper `xml` dan menolak secret berisi U+FFFF → AC-1
- [ ] `cargo test -p stackly-core --test provider_rules xml_writes_values_as_element_content_touching_literal_text` → fixture fake dengan `format: xml` merender tag yang menempel pada teks (`<path>…/</path>`), entity untuk teks, integer desimal, boolean `true`/`false`, secret lewat `materialize`, dan `{{ secret.authToken | sha256Hex }}` sebagai hash mentah; secret berisi U+FFFE dan path data berisi ESC gagal `UnrepresentableValue` dengan nama field, tanpa nilainya → AC-1, AC-2
- [ ] `cargo test -p stackly-core --test provider_rules an_unknown_format_names_every_known_format_in_its_message` → pesan `unknown_template_format` menyebut `xml` bersama ketujuh format lain → AC-1
- [ ] `cargo test -p stackly-core --test provider_compat` → corpus beku lulus tanpa satu pun `frozen.json` berubah (`git status crates/core/tests/fixtures/compat` bersih) → AC-1
- [ ] `cargo xtask codegen --check` → lulus; `schemas/provider/` tidak berubah, karena `format` di sana string tanpa daftar nilai → AC-1
- [ ] `cargo test -p stackly-core --test fuzz_regressions` dan `cargo test -p xtask fuzz` → seed `fuzz/seeds/provider_template/format.clickhouse.xml.tmpl` (selector 8, setup `xml`, dengan filter `sha256Hex`) lulus harness tanpa panic; selector target `provider_template` kini modulus 9; `cargo xtask fuzz seed` tidak mengubah `fuzz/seeds/` → AC-1

### Acceptance-criteria coverage

- AC-1 · format `xml` di core; render golden paket yang diterima binary ClickHouse asli menunggu langkah paket dan native
- AC-2 · filter `sha256Hex` dibangun spec 0040 langkah 2; di sini dipakai di test `xml`; pemakaian di paket `providers/clickhouse` menunggu langkah paket
- AC-3 sampai akhir · belum, milik langkah paket Milestone 1 dan milestone berikutnya
