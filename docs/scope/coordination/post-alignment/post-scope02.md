# Post-alignment fixes for scope02 (lead workflow wf_fa323a0b-69e, 2026-10-20)

Each item was found by a cross-document reviewer and confirmed by a skeptical second reviewer. Some items repeat from two boundaries; apply once. Lead decisions in the brief override the fix text where they differ.

## 1. [major] F20 milestone 4 prerequisite misses the PostgreSQL catalog entry (spec 0019 step 9)

- Boundary: create-path
- Evidence and reasoning: 0020:1024 and scope 02:119 make F20 M4 need only "fitur 19 milestone 2". 0019:547 (step 9, milestone 3) is where PostgreSQL moves from `[[planned]]` to `[[provider]]` in `providers/catalog.toml`. Create always resolves the provider from the loaded catalog (0020:157-160, 166), and a planned entry yields `instance.provider_unavailable`. AC-25 (0020:47) requires PostgreSQL 18 and 17 creates through CLI and GUI, with GUI evidence in step 16. Those runs use the app or `cargo xtask catalog build` catalog from catalog.toml, which lists PostgreSQL as planned until F19 step 9. Step 15's daemon tests could use `CatalogBuilder` or `Catalog::for_tests` with the PostgreSQL package, but 0020 does not say so, and the GUI and CLI evidence cannot run. No Follow-up covers this.
- Fix: In scope 02:119 and 0020:1024, change "needs feature 19 milestone 2" to "needs feature 19 milestone 3 step 9 (PostgreSQL entry in `providers/catalog.toml`)". Optionally add that step 15 daemon tests may use a `CatalogBuilder` catalog containing `official/postgresql` before that step.

## 2. [minor] Scope says milestone 1 steps 2 to 5 run in parallel; the spec makes step 5 wait for step 2

- Boundary: create-path
- Evidence and reasoning: Scope 02:116: "after step 1, steps 2, 3, 4, and 5 can run as parallel sessions". 0020:1005: "langkah 2 (registry), 3 (engine), 4 (settings), dan 5 (pin, setelah langkah 2)". Step 5 (`ensure_source`, `pin_revision` returning a record with `source_id`) uses `InsertProviderSource`, `provider_source_by_location`, and `ProviderRevisionRecord.source_id` from step 2. The scope ordering rule is wrong.
- Fix: In scope 02:116, change the clause to "after step 1, steps 2, 3, and 4 can run as parallel sessions, and step 5 after step 2".

## 3. [minor] The F20 milestone 1 scope line lets step 5 run in parallel with step 2; the spec puts step 5 after step 2

- Boundary: scope-prereqs
- Evidence and reasoning: Confirmed. Scope 02:116 says 'after step 1, steps 2, 3, 4, and 5 can run as parallel sessions'. Spec 0020:1005 says '5 (pin, setelah langkah 2)'. Step 5 (`core::catalog::source`, K-2) writes the `ProviderSourceRecord`/`InsertProviderSource` that step 2 builds (:998). This is scope drift; the spec is correct.
- Fix: In scope 02:116, change to 'after step 1, steps 2, 3, and 4 can run as parallel sessions, and step 5 after step 2'.

## 4. [minor] F20 scope AC ranges do not match the build plan

- Boundary: scope-prereqs
- Evidence and reasoning: Checked against 0020:997-1019. The M1 steps together deliver AC-1 (step 4), AC-2 to AC-15 (AC-15 from step 6), AC-21 (step 4) and the AC-23 harness. Scope 02:116 says 'AC-2 to AC-15, AC-23 harness', which drops AC-1 and AC-21. The M2 steps deliver AC-17 (step 8); AC-1, AC-2, AC-10 to AC-14, AC-16, AC-24 and AC-26 (step 9); AC-18 and AC-19 (step 10); and AC-21 (step 11). Scope 02:117 says 'AC-1, AC-10 to AC-19, AC-24, AC-26', which adds AC-15 and drops AC-2 and AC-21. M3 and M4 match. This is summary drift, and /check verify should follow the spec's per-step ACs.
- Fix: In scope 02:116, change the M1 AC list to 'AC-1 to AC-15, AC-21, AC-23 harness'. In 02:117, change the M2 list to 'AC-1, AC-2, AC-10 to AC-14, AC-16 to AC-19, AC-21, AC-24, AC-26'.
