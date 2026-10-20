# Phase 02 alignment items (collected by lead, 2026-10-20)

Sources: foundation lead K1-K18 file (/Users/ojiepermana/Library/Application Support/Claude/side-session-notes/local_1f1eb1f8-dc60-49c1-adab-cbf3e4a5bf34/foundation-k-decisions.md), upd-0011 report, upd-0001-0012 notes, upd-0009 notes, upd-0010 report (pending).

## 0020 (create instance)
- K1.5: fallible `Instance::new` is a preference; if not in when F10 step 13 builds, F20 step 1 fixes the single stacklyd call site (runs after 0010 step 2a merges if pulled ahead; both touch crates/storage/tests/registry/common.rs).
- K1: F10 builds a provisional create (`register` first irreversible step, failure leaves needs_recovery); F20 step 9 replaces it with fully compensated create. Spec 0020 must say it replaces the provisional create marked `// provisional: F20 until F20 step 9`.
- K1.6: F20 step 7 adds `StageName::PIN` and updates `startup_transaction_stages_follow_the_table_order` in the same commit.
- K1.7 and K9.5 and K10: AC-11 inconsistent: reverse compensation deletes the registry row before `instance_tree` refuses on a held `instance.lock`. `instance_registration` must also probe_lock (or refuse while the lock is held); fix before F20 step 7. instance_tree order: manifest check, lock file exists?, probe_lock (Held or error -> compensation_failed, nothing removed), remove.
- K2: F20 step 6 reuses `PortProbe::try_bind` built by F10 step 5 (change step 6 to "reuse"); new code `supervisor.endpoint_unavailable` for fixed port Denied/Unavailable; stored bound and candidates use reuse_addr, ephemeral `:0` bind does not.
- K4: no pre-assigned protocol minors; InstanceRemoved DTO and fixture added by F10 step 12 but never emitted before F20; the nine 0020 `instance.*` codes listed in the 0010 table marked (fitur 20, spec 0020); F20 step 8 builds them with hints.
- K10/K11: the 0007 text update lands before F20 M1 step 3; F10 step 7 must release its InstanceLock before returning an error; F7 step 16 migration 0004 must precede F20 M1 step 2 and land before F20 M1 step 7 and M2.
- K14: `put` changes land before 0020 M1 step 3 (0011 step 4a); F20 uses `secret::error::classify_create_failure`; needs_secret items only for registered source refs; target write failures `secret.store_*`.
- K15: `instance.create` Validate calls `SecretInput::field_errors(&format!("secrets[{i}].input"))` per entry; fix 0020:256 to that path; drop NUL from the AC-2 exception list, the AC-7 precheck list, and `SecretValueShape`; precheck shape uses the `secret::redact` helper (no evaluator in `instance::create`).
- K16/K17: `instance.removed` reduce case in F20 M3 step 12 if not folded into F10 step 12; F20 M3 step 14 defines `NewInstanceAction` if no earlier consumer did, adds the toolbar and empty state action to ProvisionalInstanceList marked `// provisional: F22`, after F13 step 16; if F22 M3 merges first, F20 wires only F22's pages; AC-17 of 0013 rewritten: buttons render only when `newInstanceAction` is not nil, with `.mutationGated()`.
- upd-0001-0012: preflight status symbols are a new status set for `design.md` (spec 0012 rule).
- upd-0010: K15 path: the 0010 session wrote `secrets[<i>].input.content`, the 0011 session wrote `field_errors(&format!("secrets[{i}].input"))`; read spec 0011 on main (AC-7 and the `field_errors` contract) and use the exact path it produces.
- upd-0010: optionally list `supervisor.endpoint_unavailable` for Fixed ports that are Denied or Unavailable in 0020's port rows and preflight table.
- Spec 0010 on main (989e816) now has a provisional create paragraph, `Reject` plus `LockKey::InstanceName` for create, `revision: Option` refused when None, InstanceRemoved DTO from F10 step 12 never emitted before F20, and the nine 0020 `instance.*` codes listed as (fitur 20, spec 0020): align every 0020 sentence that describes what F10 builds.

## 0018 (install runtime managed)
- K6: `ChildRole::RuntimeCheck` kills at once, only `core::runtime::validate` may use it; Probe fallback rejected.
- K7: early exit classification only for own children with Signaled(9) not sent by the daemon; F18 step 8 owns installing `check_launchable`/`classify_early_exit` into `core::supervisor`.
- K9: codesign helper reuses `pub(crate)` spawn and kill helpers from platform-macos process.
- K11: F7 step 16 is migration 0004 (failure_items, rule); F18 step 6 becomes migration 0005 and frozen fixture v5.
- K12: `OperationEngine::replay(&RequestOrigin)` read-only API (built at 0018 step 11) instead of the idempotency lookup wording.
- upd-0009: `OsVersion` and `ArtifactFormat` domain types are built in F17 M1 step 1 (spec 0004 text); 0018 step 6 uses them, does not build them.
- upd-0001-0012: 0018 orders pin and verify with `archive::scan` in step 1 while scan is built in step 3 (fix the order); Runtimes text says Dynamic Type (0012 says large text is a narrow width check); toast must be opaque or system before F67.
- 0004 update: `with_roots` becomes fallible with `PlatformError::HostFactsUnavailable`; F18 step 8 updates callers.
- upd-0010: F18 step 8 wires `RuntimeCheck`, `check_launchable`, `classify_early_exit`, and the two `FailureCode`s into `core::supervisor` (or F10 step 7, whichever lands second), plus the native broken signature test through `spawn_main`; spec 0010 on main (989e816) describes the F10 side.

## 0019 (provider PostgreSQL)
- K3: exec probe `check_failed` detail uses the 0019 format; F19 step 5 only adjusts if F10 step 6 shipped another format.
- K5: test log hook is `DaemonConfig.extra_log_sinks` (plain field, no feature gate); `pin_package` stays in core's fixture feature.
- K8.3: `Stdin::Text`: `expect` naming 0019 AC-3 only if the plan carries a validated newtype; otherwise map a `SecretValue::new` failure to `init_step_failed`, never `config_render_failed`.
- K13: `reason: control_characters` comes from `secret::redact` check; F19 AC-4 evidence must not be recorded before `from_setting` fills `printable_only` (0009 update).

## 0023 (provider NATS)
- K3: reason codes `unexpected_status` (http) and `check_failed` (exec) exist beside `probe_ok`, `connect_failed`, `timeout`, `protocol_error`.
- K5: `DaemonConfig.extra_log_sinks`.

## 0022 (overview & instance list)
- K4: `ownership`, `initialized` added by F10 step 12 (not by 0022 step 3), so 0022 M1 no longer needs to add them; protocol minor CURRENT+1 at merge.
- K16: InstanceStore filters retained in fetchPage, fetchOne, reduce; statusSeq rule via `.observe` effect; floors reset on server_restarted (all F13 step 16).

## 0016 (onboarding)
- K18: `MainWindowRoot` "replaces" `LiveShell` in F16 M1 step 2; the STACKLY_ROOT Bool comes from `rootOverride`.
- upd-0001-0012: `toneSurface(.neutral)` needs a strength argument.

## 0017 (catalog)
- upd-0009: define `file` and `location` for the four catalog loader issues; F17 M1 step 1 builds `OsVersion` and `ArtifactFormat` in domain (from spec 0004 text) before the catalog types.
- upd-0001-0012: pre-commit hook addition (`cargo xtask catalog check`) is an AGENTS.md /sync item when built.

## Cross-cutting (lead)
- K18 risk: every phase 02 GUI milestone (F15 M3, F16 M1, F17 M3, F18 M4, F20 M3, F22 M3) is gated by F10 M2 through F13 step 16; the generic same-revision observation rule is owned by the first phase 02 store that publishes such observations (CatalogStore or RuntimeStore).
