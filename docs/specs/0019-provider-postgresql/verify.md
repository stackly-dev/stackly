# Verify: Provider PostgreSQL · spec 0019 · updated 2026-10-21

_Langkah diturunkan dari acceptance criteria spec 0019. `/check verify` menjalankan langkah ini; `/test` mengunci yang tahan lama. Setiap milestone punya bagiannya sendiri di bawah. Jalankan dari root repo di Mac Apple Silicon dengan macOS 27. Milestone 1 tidak butuh server PostgreSQL, artefak runtime, atau cache runtime: semuanya dibuktikan di fixture._

## Milestone 1: Package and additive core changes on fixtures

### UI / manual

- [x] Buka `providers/postgresql/` → berisi persis `provider.yaml`, `versions.json`, `schemas/settings.schema.json`, `templates/pg_hba.conf.tmpl`, `templates/pg_ident.conf.tmpl`, `templates/postgresql.conf.tmpl`, dan `README.md`; `provider.yaml` belum memuat blok `connection` (milik spec 0021) dan belum memuat `runtime.versionCheck` (milik langkah 7) → AC-1
- [x] Buka `providers/postgresql/versions.json` → tepat dua artefak `available`: `postgresql-18.6-r2-macos-aarch64` line `18` dan `postgresql-17.11-r2-macos-aarch64` line `17` → AC-1
- [x] Buka `providers/postgresql/schemas/settings.schema.json` → `superuserPassword` bertanda `secret: true` dan `printableOnly: true`, panjang 8 sampai 128, `apply` `immutableAfterInit`; field lain sesuai tabel *Settings schema* → AC-4
- [x] Buka `crates/core/tests/golden/provider/postgresql/` → `init.json`, `run.json`, `reload.json`, dan tiga `render-*.txt`; di `init.json` langkah `postgres --single` membawa `stdin` bertipe `text` dengan empat statement, satu per baris → AC-3, AC-12

### Commands

- [x] `cargo xtask provider check` → satu baris `ok official/postgresql 1.0.0 <sha256>` di antara baris paket lain → AC-1
- [x] `cargo xtask codegen --check` → "up to date", termasuk `providers/postgresql/versions.json` dan `schemas/provider/settings.v1alpha1.schema.json` (memuat `printableOnly`) → AC-1, AC-4
- [x] `cargo test -p stackly-core --lib provider::template` → escaper `postgresqlConf` (kutip, backslash, Unicode, boolean `on` atau `off`, karakter kontrol ditolak) dan format `plain` (tanpa tag output dan blok) lulus → AC-2
- [x] `cargo test -p stackly-core --test provider_rules` → 47 test lulus, termasuk `postgresql_conf_renders_quoted_text_decimal_integers_and_on_off_booleans`, `plain_templates_keep_text_and_comments_and_reject_output_and_block_tags`, `stdin_text_is_literal_one_key_bounded_and_free_of_tokens_and_most_control_characters`, `stdin_text_reaches_the_plan_exactly_and_changes_the_digest`, dan `the_settings_dialect_rejects_keywords_and_rules_outside_it` (termasuk `printableOnly` pada field bukan secret ditolak `settings_schema_rule` di `properties.label.x-stackly.printableOnly`) → AC-2, AC-3, AC-4
- [x] `cargo test -p stackly-core --test secret_service printable` → 8 test lulus, termasuk `from_setting_fills_printable_only_from_the_settings_schema` dan `create_with_a_schema_printable_only_field_refuses_control_characters` (nilai ber `\n` ditolak `secret.invalid_value` tanpa panggilan ke `FakeSecretStore`, nilai ber spasi, `'`, `"`, dan `\` lolos) → AC-4
- [x] `cargo test -p stackly-core --test provider_packages postgresql` → 7 test lulus: paket terbaca tanpa tulis dan valid, plan dan tiga render sama dengan golden, program plan adalah `bin/initdb`, `bin/postgres`, `bin/postgres`, `bin/psql` di root runtime dan tidak ada argumen `postgres` atau `…=postgres`, `Stdin::Text` empat baris, `pg_hba.conf` tanpa `trust` dan `superuser-password` ada di `cleanup`, `superuserPassword` menghasilkan `SecretFieldRule` dengan `printable_only`, dan nilai teks di `postgresql.conf` terkutip → AC-1, AC-3, AC-4, AC-12
- [x] `STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test provider_packages postgresql`, lalu `git status` → tidak ada perubahan pada file golden (golden stabil) → AC-1
- [x] `cargo xtask fuzz seed` → seed `provider_template` tidak berubah tak terduga; harness template sudah memuat format `postgresqlConf` dan `plain` sejak langkah 1 → AC-2
- [x] `cargo xtask check-deps` → lulus; `cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core` → exit 0 → AC-1

### Acceptance-criteria coverage

- AC-1 · bagian paket: `provider check`, `versions.json` dua artefak, golden render dan plan; bagian catalog (`providers/catalog.toml`) menunggu Milestone 3 langkah 9
- AC-2 · bagian format dan validasi `plain`: langkah 1; round trip `postgres -C log_line_prefix` menunggu Milestone 2 langkah 8
- AC-3 · bagian plan: langkah 2 (validasi, `Stdin::Text`, digest) dan golden langkah 4; arm executor init dan byte yang diterima proses menunggu Milestone 2 langkah 5 dan 8
- AC-4 · bagian fixture: `printableOnly` di model settings dan `SecretFieldRule::from_setting` dengan `FakeSecretStore`; login dengan password berkarakter khusus menunggu Milestone 2 langkah 8
- AC-12 · bagian plan: program `ExecutablePath` di root runtime; `PATH` umpan menunggu Milestone 2 langkah 8
- AC-5 sampai AC-11, AC-13 sampai AC-15 · belum, milik Milestone 2 dan 3
