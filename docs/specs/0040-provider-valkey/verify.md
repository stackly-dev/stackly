# Verify: Provider Valkey · spec 0040 · updated 2026-11-12

_Langkah diturunkan dari acceptance criteria spec 0040. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Bagian Milestone 1 di bawah baru memuat langkah 1 dan 2 Build plan (perubahan core); developer paket menambahkan langkah paket, golden, dan spike ke bagian yang sama._

## Milestone 1: Perubahan core dan paket, dibuktikan di fixture

### UI / manual

- [ ] Buka `docs/providers/manifest-reference.md` → baris `format` di tabel `configFiles` menyebut `valkeyConf` (teks berkutip ganda dengan `\` dan `"` di escape, boolean `yes` dan `no`) → AC-2
- [ ] Buka `docs/providers/manifest-reference.md` → baris `sensitive` di tabel `configFiles` menyebut `{{ secret.<field> \| sha256Hex }}` (SHA-256 sebagai 64 karakter hex huruf kecil, tanpa escape dan tanpa kutip, satu satunya filter) → AC-3

### Commands

- [ ] `cargo test -p stackly-core --lib provider::template::tests::valkey_conf` → 3 test lulus: `a"b\c d ü #x` menjadi `"a\"b\\c d ü #x"`, `$`, `'`, backtick, dan kurung kurawal keluar apa adanya, setiap karakter kontrol (U+0000 sampai U+001F, U+007F sampai U+009F) ditolak dan U+00A0 diterima, boolean `yes` dan `no`, tag diterima, output tidak diparse ulang, dan `from_code("valkeyConf")` mengembalikan varian baru (kode lain dengan huruf berbeda tidak) → AC-2
- [ ] `cargo test -p stackly-core --lib secret::delivery::tests::a_placeholder_renders_with_the_escaper_of_its_format` → `render_secret_placeholder` memakai escaper `valkeyConf` dan menolak secret berisi newline → AC-2
- [ ] `cargo test -p stackly-core --test provider_rules valkey_conf_renders_quoted_text_decimal_integers_and_yes_no_booleans` → fixture fake dengan `format: valkeyConf` merender teks berkutip, integer desimal, boolean `yes`/`no`, dan secret lewat `materialize`; secret dan path data dengan karakter kontrol gagal `UnrepresentableValue` dengan nama field, tanpa nilainya → AC-2
- [ ] `cargo test -p stackly-core --test provider_rules an_unknown_format_names_every_known_format_in_its_message` → `format: ini` ditolak `unknown_template_format`, dan pesannya menyebut `valkeyConf` bersama setiap format lain yang dikenal → AC-2
- [ ] `cargo test -p stackly-core --lib provider::template::tests::an_output_tag_takes_filter_names_after_a_bar_with_any_spacing provider::template::tests::an_empty_or_odd_filter_name_is_a_syntax_error_at_the_tag` → filter dibaca sesudah `|` dengan spasi bebas; nama kosong, `||`, `sha-256`, `sha256_hex`, nama berspasi, huruf non ASCII, dan path kosong menjadi kesalahan sintaks di posisi tag → AC-3
- [ ] `cargo test -p stackly-core --test provider_rules a_sha256_hex_filter_writes_the_hash_of_the_secret_with_no_escaper` → template `json` sensitif dengan `{{ secret.authToken|sha256Hex }}` lolos validasi (render contoh dengan hash dari `x` diparse ulang), plan memuat `Segment::SecretHash`, `materialize` menulis hash SHA-256 `abc` mentah di dalam kutip literal template, secret yang tidak ada menjadi `SecretUnavailable`; di `valkeyConf` hash dari nilai berisi `"` dan newline keluar tanpa escaper dan tanpa kutip → AC-3
- [ ] `cargo test -p stackly-core --test provider_rules a_hash_segment_shows_only_the_secret_reference_in_the_plan` → segmen di JSON plan berbentuk `{ "type": "secretHash", "content": { "secret": "<secret:authToken>", "algo": "sha256_hex" } }`, dan `Debug` serta JSON plan tidak memuat referensi secret atau hash → AC-3
- [ ] `cargo test -p stackly-core --test provider_rules a_filter_is_rejected_off_a_secret_path_with_its_code_and_position` → filter sesudah `settings.*` atau `instance.*`, nama `md5` atau `Sha256Hex`, dan dua filter ditolak `template_type` dengan baris dan kolom; `{{ secret.authToken | }}` dan `sha-256` ditolak `template_syntax`; tag dengan filter di format `plain` tetap `template_type`; secret dengan atau tanpa filter di config yang tidak `sensitive` tetap `secret_not_allowed_here` → AC-3
- [ ] `cargo test -p stackly-core --lib secret::delivery::tests::a_hash_algo_serializes_as_its_code` → `HashAlgo::Sha256Hex` diserialisasi sebagai `"sha256_hex"` → AC-3
- [ ] `cargo test -p stackly-core --test provider_compat` → corpus beku lulus tanpa satu pun `frozen.json` berubah (`git status crates/core/tests/fixtures/compat` bersih) → AC-2, AC-3 (syarat additive 3)
- [ ] `cargo test -p stackly-core --test fuzz_regressions` dan `cargo test -p xtask fuzz` → seed `fuzz/seeds/provider_template/format.valkey.conf.tmpl` (selector 6, setup `valkeyConf`) dan `filter.sha256hex.json.tmpl` (selector 0, filter `sha256Hex`) lulus harness tanpa panic, dan `cargo xtask fuzz seed` tidak mengubah `fuzz/seeds/` → AC-2, AC-3 (syarat additive 6)

### Acceptance-criteria coverage

- AC-2 · format `valkeyConf` di core, tanpa round trip native; round trip `CONFIG GET proc-title-template` pada Valkey 9.1.2 menunggu Milestone 2
- AC-3 · filter `sha256Hex` dan `Segment::SecretHash` di core, dipakai di fixture test (paket fake di memori); pemakaian di paket `providers/valkey` (file sensitif berisi hash fixture, bukan nilainya) menunggu langkah paket Milestone 1
- AC-1, AC-4 sampai AC-14 · belum, milik langkah paket Milestone 1 dan Milestone 2 sampai 4
