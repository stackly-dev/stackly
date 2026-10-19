# Verify: secret store & redaction · spec 0011 · updated 2026-10-20

_Langkah ini diturunkan dari acceptance criteria spec 0011 untuk build step 0 (registry secret dan audit) dan step 4 (core: `SecretWorker`, `SecretService`, hook operasi). `/check verify` menjalankannya; `/test` mengunci yang tahan lama. Langkah 5 dan 7 (daemon) belum dibangun, jadi tidak ada langkah UI atau daemon di sini._

## Commands

- [ ] `cargo test -p stackly-testkit --test fake_registry` → `fake_registry_passes_the_conformance_suite` lulus, termasuk skenario secret dan audit (insert, update, observe, delete dengan `ReferenceInUse`, restore minimal revision 2, `ReplaceSecretUses`, halaman audit) → AC-5, AC-9, AC-10 (prasyarat registry); spec 0005 AC-9
- [ ] `cargo test -p stackly-storage --test registry conformance` → suite yang sama lulus di `SqliteRegistry`, dan `registry_never_holds_secret_values` lulus → spec 0005 AC-9, AC-12
- [ ] `cargo test -p stackly-storage --test open` → schema versi 3 cocok dengan *Data model* (25 tabel), fixture beku `registry-v3.sql` tidak berubah, migrasi v2 ke v3 menghapus baris `secret_references` lama dan menjaga tabel lain, baris secret dan audit fixture v3 terbaca lewat registry, trigger `secret_references_owner_fixed` dan `RESTRICT` menolak perubahan yang salah → spec 0005 AC-1, AC-4, AC-12, AC-15
- [ ] `cargo test -p stackly-core --test secret_service worker` → panggilan di belakang panggilan yang ditahan gagal `Busy` dan tidak pernah sampai ke store, `write` yang ditahan melewati batas gagal `OutcomeUnknown` lalu diikuti `probe_one`, `state()` menjawab seketika, default 180 dan 10 detik → AC-4
- [ ] `cargo test -p stackly-core --test secret_service put` → keempat input kali ketiga mode, `expectedRevision` salah tanpa panggilan store, `create` saat cache `missing` tetapi item ada gagal `secret.conflict`, commit registry gagal menulis balik nilai lama atau menghapus item baru, penulisan balik yang gagal menjadi `secret.store_outcome_unknown`, `stored_only` butuh konfirmasi, `bind` tanpa tulis nilai, `copyFrom` tidak mengembalikan nilai, revision baris baru 1 → AC-5, AC-8
- [ ] `cargo test -p stackly-core --test secret_service put::a_generated_value_is_32_characters_clamped_to_its_field_bounds` → 32 karakter tanpa konteks field, dijepit ke `minLength` 40 dan `maxLength` 12, dan ke batas bersama semua field yang memakai reference → AC-6
- [ ] `cargo test -p stackly-core --test secret_service status` → list membaca cache tanpa panggilan store, status probe ulang (item yang dihapus di luar terbaca `missing`, yang gagal dicek `unknown`), `probe_all` satu commit tanpa menaikkan revision, kunci store yang berbeda dicatat dengan kunci yang dihitung → AC-8
- [ ] `cargo test -p stackly-core --test secret_service access` → reveal menulis satu baris audit tanpa nilai, reveal gagal menulis baris `failed` dengan `errorCode`, audit yang gagal ditulis membuat reveal gagal tanpa nilai; delete reference yang dipakai ditolak dengan semua `usedBy`, store gagal memulihkan baris dengan revision naik 1, item yang sudah hilang tetap menghapus baris → AC-9, AC-10
- [ ] `cargo test -p stackly-core --test secret_service resolve` → `NeedsSecret` urut teks dengan alasan yang benar, satu penolakan dialog menghentikan dialog berikutnya, error backend mengalahkan needs secret, cache `missing` tidak melewati `read`, `generateAllowed` false untuk field `immutable_after_init` pada data yang sudah diinisialisasi, blocker tidak mengubah lifecycle atau revision dan hilang setelah semua reference `present`, `learn_running` hanya untuk instance yang berjalan → AC-11
- [ ] `cargo test -p stackly-core --test secret_service engine` → `record_secret_intent` probe dulu, item yang ada gagal `secret.conflict` tanpa catatan, reference instance lain ditolak sebelum probe, kompensasi lewat `secret_ref` menghapus baris dan item, tanpa kompensator tercatat `compensator_missing` → spec 0007 hook fitur 11
- [ ] `cargo xtask check-deps` → lulus; satu satunya pemanggilan `expose` baru ada di `secret::redact` → AC-15

## Value sourcing

- [ ] `put` dengan `Generate` dan konteks field: ubah `minLength` dan `maxLength` di `SecretFieldRule`, panjang nilai yang tersimpan mengikuti batas → baris `put` / nilai baru
- [ ] `put` dengan `Replace` lalu `redact` nilai lama → nilai lama tersamar → baris `put` / nilai lama untuk penulisan balik
- [ ] `put` dari `SecretCaller::client(.., interactive: false)` → panggilan store memakai `Never`; dengan `interactive: true` memakai `AllowPrompt` → baris `put` / interaksi
- [ ] `platform_store_key` baris sama dengan `SecretStore::store_key` setelah `put` dan setelah probe dengan kunci lain → baris `put` / `platform_store_key`
- [ ] `secret.set` untuk reference baru: `owner_kind` `user`, `owner_id` null, purpose `user`; `PutTarget::New`: pemilik dan purpose dari pemanggil → baris `put` / pemilik
- [ ] `value_origin` `entered`, `generated`, `copied` menurut input → baris `put` / `value_origin`
- [ ] Waktu `value_updated_at` dan `status_observed_at` sama dengan `FakeClock` → baris `put` / waktu
- [ ] Revision naik tepat 1 per `put`, 1 untuk baris baru, tetap saat probe → baris `put` / revision
- [ ] `effects` berubah dari `next_start` ke `stored_only` saat `data_initialized_at` terisi pada field `immutable_after_init`, dan `next_reload` untuk field `reloadable` → baris `put` / effects
- [ ] Nilai 7 byte memberi `short_value_not_scrubbed`, 8 byte tidak → baris `put` / peringatan
- [ ] Audit `actor` `user` untuk `SecretCaller::client`, `daemon` untuk `background`; kunci `safe_details` `reference`, `purpose`, `clientKind`, `peerUid`, `errorCode` → baris audit
- [ ] Audit `resource` terisi selama baris reference ada, kosong untuk reference yang tidak terdaftar dan setelah delete berhasil → baris audit / `resource`
- [ ] `resolve_for_start` hanya membaca reference dari `secret_uses_by` instance itu → baris `resolve_for_start` / daftar reference

## Acceptance-criteria coverage

- AC-4 · langkah worker · AC-5 · langkah put · AC-6 · langkah generate · AC-8 · langkah put dan status · AC-9 · langkah access (reveal) · AC-10 · langkah access (delete) · AC-11 · langkah resolve · AC-15 · check-deps
- Belum dicakup di sini (langkah 5 dan 7, daemon): AC-3 lewat `stacklyd`, AC-13 writer log, AC-14, AC-16, AC-21
