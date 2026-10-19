# Review, claude/f9-closeout-c0a0a3 (feature 9, provider engine and manifest schema), 2026-10-19

**Reviewed by**: Claude Sonnet 5.5 (claude-sonnet-5-5) (author: Claude Opus 5.5)
**Scope**: feature review of code already on main (`6b3da2c` plus three uncommitted tests in `crates/core/tests/provider_rules.rs`): the 17 files of `crates/core/src/provider/` (about 10 900 lines), the four core provider test files and goldens, and the secondary areas of the brief (fake-server, the three example packages and the `fake-command` fixture, `xtask provider check`, the provider part of `apps/daemon/src/map.rs`, `codes.rs`, `ProviderRevisionKey`)
**Verdict**: Blocked

## Resolution (f9 close-out session)

| Finding | Severity | Outcome |
| --- | --- | --- |
| YAML alias bombs with a large anchor bypass the repetition limit, in `provider.yaml` and in `yaml` template output | Blocker | Fixed in `2a25db1` and `4ee9bf6`: `raw::yaml_value` visits the document without storing it and refuses one whose aliases expand past two nodes per byte of text plus 1024, before the value is built; `parse_yaml` and `TemplateFormat::output_parses` use it, and a failed counting pass is reported without a second parse. Tests `a_large_anchor_behind_a_short_alias_chain_stops_at_the_node_budget`, `documents_without_a_large_alias_expansion_stay_within_the_budget`, and `a_large_anchor_alias_bomb_is_rejected_in_the_manifest_and_in_template_output` (VAL-018 guard), plus regression inputs in `crates/core/tests/fuzz_regressions/provider_manifest/` and `provider_template/`. The 5.5 KB manifest bomb now fails in 30 ms with 11.6 MB RSS through `cargo xtask provider check` (was 1.35 s and 713 MB). No manual seed was added to `fuzz/seeds/`, because `cargo xtask fuzz seed` rebuilds that folder from the packages; the regression folder replays on stable instead |
| A settings `pattern` can break out of its whole value wrapper (also `dataMarker.pattern`) | Major | Fixed in `2a25db1`: `Pattern::compile` compiles the source alone first. Test `a_pattern_that_compiles_only_inside_the_whole_value_wrapper_is_rejected` (settings `properties.label.pattern` and `compatibility.dataMarker.pattern`) |
| The dummy server cannot be stopped once stdout or stderr is closed or full | Major [fake-server] | Fixed in `11cd1f9` and `beec9bb` (`crates/fake-server` is a feature 9 crate outside the session's hands off list, so it was fixed here under the standing approval to take the recommended option): notes go out from a thread of their own and an exit waits at most 200 ms for its last line; the startup line no longer panics on a closed stdout, and `main` reports a startup failure with its documented exit code when stderr is closed (`beec9bb`, found by the first workspace run, where a port taken by another process ended in a panic). Four tests in `dummy_server.rs` (closed stdout for SIGTERM and `POST /shutdown`, full stdout and stderr, closed stderr for a crash, notes on closed streams); each fails on the old `serve.rs` |
| `with_issues` drops the `too_many_issues` marker and counts it in `issueCount` | Minor [routed: apps/daemon] | Routed: `apps/daemon` is outside this session; listed in the close-out report for the daemon owner |
| `ProviderRevisionKey::from_str` accepts an uppercase digest | Minor [routed: domain] | Deferred to the follow up session for the feature 9 review minors (decide lowercase only text for AC-19) |
| The 13 Minors in `core/provider` (chained `unknown_config_file` and template type issues, `recoverySeconds` from a rejected `readinessSeconds`, template work budget, `temp` outside `managedFilesystem`, `createManagedDirectories` order, duplicate destinations, NUL in literals, U+FFFE and U+FFFF in yaml, read errors as `not_a_regular_file` with a host path, the fuzz harness in the production library, settings field order, untested AC-1 and AC-8 limits) | Minor | Deferred to a follow up session; the brief asked this session to fix Blockers and Majors only. Settings field order and the IO error code need `/architect` first |
| `xtask provider check` prints entry names unsanitized; Traefik reload throttle and `backendUrl`; `fake-server init` writes its argv before it refuses a folder | Minor [routed: xtask, providers, fake-server] | Deferred to the same follow up session |
| Nits | Nit | Left as is; listed below for the owners |

## Summary

Feature 9 is a careful, well tested engine: strict two stage decode with field paths, a closed primitive set, a typed position matrix for `${...}`, a logic-less renderer with a per format escaper and re-parse, plans that are `Serialize` only and carry secrets as `<secret:field>`, and a no-shell test that runs the real dummy server. I found one Blocker: the billion laughs defense of AC-2 and VAL-018 is bypassable with a 5 KB file (a large anchor, not the small one the committed test uses), in `provider.yaml` and in template output, and a package that carries it validates. Two Majors: a settings `pattern` can escape the `\A(?:…)\z` wrapper and stop matching the whole value, and the dummy server cannot be stopped once its stdout or stderr is closed or full. The rest is Minor: misleading chained issues, a few validation gaps (a work budget for template loops, duplicate config destinations, NUL in literals, `temp` outside `managedFilesystem`), and a handful of spec reconciliations. The three uncommitted tests pass and are meaningful.

Every Blocker and Major below was reproduced with a scratch program or script outside the repo (`/private/tmp/claude-502/-Users-ojiepermana-Development-ojiepermana-Stackly--claude-worktrees-f9-closeout-c0a0a3/318a0064-ad48-44a3-9fb4-1cd78cb64e4f/scratchpad/review/`). Findings tagged `routed:` belong to a crate other than `core/provider`; those from the secondary pass that I did not re-run myself say so.

## Blockers

### 🔴 YAML alias bombs bypass the repetition limit, so VAL-018 is not met, `crates/core/src/provider/raw.rs:535` and `crates/core/src/provider/template.rs:83`
**Area**: core/provider
**Problem**: `parse_yaml` calls `serde_yaml_ng::from_str::<Value>`, which expands every alias into a full copy. The only guard in `serde_yaml_ng` is `jumpcount > events.len() * 100` (`serde_yaml_ng-0.10.0/src/de.rs:478`), and `jumpcount` counts alias jumps, not the nodes they expand to. A file with one large anchor and a short chain of nine-way aliases expands to `n * 9^depth` nodes after only `~9^depth` jumps, which stays far below `events * 100`. Concrete input, appended to the fake `provider.yaml`: `x0: &x0 [a, a, ... 1000 times]`, then `x1: &x1 [*x0 x9]`, `x2: &x2 [*x1 x9]`, `x3`, `x4`. The whole file is 5.5 KB, `ValidatedPackage::from_files` takes 1.35 s and 713 MB RSS to answer `unknown_field x0`. Each extra level costs about 15 bytes of file and multiplies the work by 9 (measured: 100 elements at depth 4 is 95 MB, 1000 elements at depth 3 is 83 MB and 0.14 s); extrapolating, depth 5 is about 6.4 GB and 12 s, and depth 6 about 58 GB, which I did not run on a shared machine. The same expansion happens in `TemplateFormat::output_parses` (`template.rs:83`): the same bomb as literal text in a `yaml` template makes the package validate (`valid=true`, 715 MB, 1.24 s), and every later `init_plan`, `run_plan`, and `reload_plan` repeats it through `Resolver::write_file` (`plan.rs:939`). `manifest_software` (`versions.rs:33`, used by `cargo xtask codegen`) shares `parse_yaml`.
**Why it matters**: AC-2 and the Security model promise that a hostile `provider.yaml` is rejected in 2 s and that nesting limits are "proved by the billion laughs test". That test (`provider_rules.rs:1762`, and the unit test `raw.rs:869`) uses a 9 element anchor padded with comments, so it has about 100 events and trips the limit after ~10 000 jumps; it proves nothing about an attacker who raises the event count. Feature 37 will feed user imported packages into this function; one 6 KB file can take the daemon down with out of memory. The 10 minute fuzz runs did not find it because mutating the seeds rarely builds an anchor chain.
**Suggested fix**: Do not hand untrusted text to `serde_yaml_ng::from_str::<Value>` directly. Decode through a `DeserializeSeed`/`Visitor` wrapper (like the existing `StrictJson`) that counts every visited node and fails with `yaml_syntax` past a fixed budget (for example 200 000 nodes). Alias replay is streaming, so the failure aborts the expansion early. I verified this in the scratch crate: the same style of wrapper aborted a depth 6 bomb (531 million nodes) after 200 001 nodes in 8 ms and decoded the plain manifest in 0.4 ms. Use it in `parse_yaml` and in `output_parses` (and so `manifest_software`). Add regression tests with a large anchor, depth at least 6, in `provider.yaml` and in a `yaml` template, a `fuzz_regressions/provider_manifest` input, and a seed in `fuzz/seeds`.

## Major

### 🟠 A settings `pattern` can break out of its whole-value wrapper, `crates/core/src/provider/settings.rs:104`
**Area**: core/provider
**Problem**: `Pattern::compile` builds `\A(?:{source})\z` by string concatenation and never checks that `source` is a complete regex on its own. A source with an unbalanced `)` closes the group early. Reproduced with a field `{"type":"string","maxLength":256,"pattern":"a)|(b","default":"abc"}`: the schema validates, the compiled regex is `\A(?:a)|(b)\z`, and `label = "axxx; rm -rf /"` and `label = "zzzb"` are accepted while `"zzz"` is rejected. `dataMarker.pattern` (`validate.rs:627`) and item field patterns use the same function.
**Why it matters**: AC-8 and the Security model state that a pattern is "selalu dicocokkan penuh". Patterns are the only shape guard on free text settings (for example Traefik `backendUrl` and `name`), and a package reviewer reading `a)|(b` or a longer variant sees a restrictive pattern while the engine enforces a prefix or suffix test. Cheap to trigger, cheap to fix.
**Suggested fix**: Compile `source` alone first with the same `size_limit` and reject the schema when that fails; only then compile the wrapped form. A source that is not a valid standalone regex then cannot reach the wrapper. Extend `patterns_always_match_the_whole_value` (`provider_rules.rs:1027`) with `a)|(b` and a `dataMarker` case.

### 🟠 The dummy server cannot be stopped once stdout or stderr is closed or full, `crates/fake-server/src/serve.rs:218-223`, `:263-272`, `:284-292`
**Area**: routed: fake-server
**Problem**: `POST /shutdown`, `exit_on` (SIGTERM and SIGINT), and `crash` call `println!` or `eprintln!` immediately before `process::exit`. With the read end of the pipe gone, the print panics: the handler thread or the signal-watcher thread dies and the process keeps running. I reproduced the stdout case with the prebuilt binary: start `serve` with piped stdout, close the reader, send SIGTERM twice; the exit status stays `None` and `GET /health` still answers 200. The sub-reviewer reproduced two more cases that I did not re-run: with stderr closed, `--crash-after-ms` never crashes, and with `--log-lines-per-second=2000` and a reader that never reads, the spam thread holds the stdout lock while SIGTERM waits on the same lock.
**Why it matters**: the supervisor tests of features 10 and 14 will capture logs through pipes. An orphaned dummy whose daemon died, or a test that stops draining, ignores SIGTERM although `--ignore-term` is off, which contradicts the spec table ("SIGTERM … keluar 0", "POST /shutdown … lalu keluar 0") and leaves processes that only SIGKILL stops.
**Suggested fix**: nothing on an exit path may depend on stdout or stderr. Exit first and print best effort afterwards (or print from a detached thread, ignoring errors, and exit after a short bounded wait). Use `writeln!(io::stdout(), …)` with ignored errors instead of `println!` on those paths. Add tests for closed and full pipes.

## Minor

### 🟡 A config file that failed its own checks makes every reference to it report `unknown_config_file`, `crates/core/src/provider/validate.rs:1009-1021`, `:1222-1231`, `:1401-1408`
**Area**: core/provider
**Problem**: `config_files()` inserts a config into the map only when template, format, and destination all passed. With `format: ini` on `fakeConfig` the output is `unknown_template_format` followed by `unknown_config_file: configFiles has no fakeConfig` for `lifecycle.initialize[1].file` and for `lifecycle.reload.render[0]`, although the config exists (reproduced).
**Why it matters**: the second and third issue point the contributor at a reference that is correct, and they hide the real fix.
**Suggested fix**: keep the names of declared configs in a separate set (even when invalid) and report `unknown_config_file` only for names that are not declared; skip the dependent checks for a declared but invalid config.

### 🟡 A rejected settings schema makes valid templates report type errors, `crates/core/src/provider/interpolate.rs:397-401`, `crates/core/src/provider/validate.rs:1953`
**Area**: core/provider
**Problem**: when the settings schema fails (reproduced with Traefik `maxItems: 9999`), `resolve` answers `settings.<field>` with `TokenType::Text` as a placeholder, so the next stage reports `templates/routes.yaml.tmpl 2:1: template_type: #if needs a boolean or a list, and settings.routes is neither`.
**Why it matters**: a false second issue on a template that is fine.
**Suggested fix**: add an "unknown" token type for the placeholder case that satisfies any position, or skip template checking when `settings` is `None` (`check_templates` already refuses to return a result in that case).

### 🟡 An out of range `readinessSeconds` adds issues the author did not cause, `crates/core/src/provider/validate.rs:738-746`, `:1674-1689`
**Area**: core/provider
**Problem**: `recoverySeconds` defaults to `readinessSeconds`, then is range checked as if it were written. `readinessSeconds: 5000` yields `timeouts.recoverySeconds: must be 1 to 1800` and `0` yields that plus two `attemptTimeoutMilliseconds cannot exceed … (0 s)` issues (reproduced).
**Suggested fix**: range check `recoverySeconds` only when it is written, and skip the deadline comparison for a deadline that was itself rejected.

### 🟡 Template rendering has no work budget, `crates/core/src/provider/template.rs:856-870`
**Area**: core/provider
**Problem**: the 1 MiB cap counts output bytes only. `{{#each settings.routes as a}}{{#each settings.routes as b}}{{#each settings.routes as c}}{{#each settings.routes as d}}{{/each}}…` is legal (4 levels), emits nothing, and runs `256^4` iterations when the setting holds its `maxItems` of 256 (a `default` can hold them). Measured: 100 items took 0.26 s, so 256 items is about 11 s in a release build per plan, each of `init_plan`, `run_plan`, and `reload_plan`; validation only renders 0 and 1 items, so it never notices.
**Why it matters**: a hostile package can stall a plan call for seconds to minutes (debug builds and a body with an `#if` are slower), bounded but needless.
**Suggested fix**: count visited nodes or loop iterations in `Output` and return `OutputTooLarge` (or a new render limit error) past a budget such as 1 000 000; add a test with four nested loops over 256 items.

### 🟡 `HOME` and `TMPDIR` always point into `temp`, even when the manifest does not list it, `crates/core/src/provider/plan.rs:832-837`
**Area**: core/provider
**Problem**: `Resolver::command` takes `self.dir(InstanceDir::Temp)` for the base env without any permission check, and validation has no rule that `temp` is in `permissions.managedFilesystem`. Reproduced: the fake manifest with `managedFilesystem: [data, config, logs, run]` validates, its `run_plan` has `HOME` and `TMPDIR` under `…/temp`, and `createManagedDirectories` leaves `temp` out.
**Why it matters**: the Key invariant (and the brief) says every plan path lies under a folder the manifest permits; the base env is a hidden use of `${instance.temp}` that bypasses `path_not_permitted`. Practical impact is small because `prepare_layout` creates `temp/` for every instance (`crates/core/src/layout.rs:108`), but the permission model has a hole and the fuzz harness `assert_inside` (`fuzzing.rs:508`) cannot see it.
**Suggested fix**: require `temp` in `managedFilesystem` (issue `path_not_permitted` at `permissions.managedFilesystem`), since every manifest has a start command; or document `temp` as always granted. Make the fuzz invariant check the permitted set.

### 🟡 Validation does not require `createManagedDirectories` before steps that use the folders, `crates/core/src/provider/validate.rs:486-499`, `crates/core/src/provider/plan.rs:1062-1072`
**Area**: core/provider
**Problem**: removing `{ primitive: createManagedDirectories }` from the fake manifest validates, and the init plan then starts with a `WriteFile` into `config/` and a `Run` with `cwd` in `data/`, with no `EnsureDirectories` for either (`write_steps` passes `with_managed = false`). Order is not checked either (`renderConfig` before it is accepted).
**Why it matters**: the spec says the primitive ensures every managed folder exists, and a plan that depends on something else having created them is not self contained; it works today only because the layout pre-creates them.
**Suggested fix**: add a rule that any other initialize step must follow a `createManagedDirectories` step, or have `init_plan` always lead with the managed folders.

### 🟡 Two config files may share one destination, `crates/core/src/provider/validate.rs:956-1024`
**Area**: core/provider
**Problem**: a second `configFiles` entry with `destination: "${instance.config}/fake.json"` validates (once it is added to `reload.render`) and `run_plan.render` writes both to the same path in name order; the second silently wins (reproduced).
**Suggested fix**: reject two configs with the same resolved destination (`invalid_value` at the second `…destination`), and one that equals a `materializeSecret` target.

### 🟡 Literal control characters, NUL included, are accepted in args, env values, and other literals, `crates/core/src/provider/interpolate.rs:259-297`, `crates/core/src/provider/validate.rs:1060-1099`
**Area**: core/provider
**Problem**: `args: ["serve\0x"]` validates and `run_plan.spawn.args[0]` is `"serve\0x"`; `std::process::Command` rejects NUL at spawn. The same holds for env values and any other literal text that is not a path component.
**Why it matters**: the spec promises that mistakes show up at validation, not at start.
**Suggested fix**: reject control characters (at least NUL) in the literal pieces of `args`, `env`, and `http.path`, and in plan rendering.

### 🟡 The `yaml` escaper leaves non-characters in place, so a valid setting fails at plan time as `render_failed`, `crates/core/src/provider/template.rs:46-65`
**Area**: core/provider
**Problem**: U+FFFE and U+FFFF pass `SettingsSchema::validate` and are written raw into the double quoted scalar; libyaml refuses them on re-parse, so a Traefik route `rule` containing U+FFFF gives `PlanError::RenderedOutputInvalid` (category `failed`, reproduced). U+FDD0 and U+1FFFF parse fine.
**Suggested fix**: escape U+FFFE and U+FFFF as `￾` and `￿` in the yaml escaper, or reject them in settings text.

### 🟡 Read errors are reported as `not_a_regular_file` and carry an absolute path, `crates/core/src/provider/package.rs:362-368`
**Area**: core/provider
**Problem**: a `provider.yaml` with mode 000 yields `provider.yaml .: not_a_regular_file: cannot read: permission denied for /private/tmp/…/provider.yaml` (reproduced). The code is wrong for a permission or IO failure, and the full host path goes into `Issue.message` and onto the wire.
**Suggested fix**: report such failures with a code that fits (the closed list has none for IO; add one by `/architect`, or use `missing_file` for `NotFound` only) and keep the path out of the message.

### 🟡 The fuzz harness is compiled into the production library, `crates/core/src/provider/mod.rs:10-11`, `crates/core/src/provider/fuzzing.rs:36-77`
**Area**: core/provider
**Problem**: `#[doc(hidden)] pub mod fuzzing` has no feature gate, so `stacklyd` carries the harness and `include_str!` copies of the fake and Traefik packages (files outside the crate directory) as public API.
**Suggested fix**: gate it behind a feature (the existing `test-support` fits for the in-crate tests, and the fuzz crate enables it).

### 🟡 Settings fields lose their order, `crates/core/src/provider/settings.rs:344-348`, `crates/core/src/provider/raw.rs:378`
**Area**: core/provider
**Problem**: `properties` is read into a `BTreeMap`, so `SettingsSchema::fields()` returns alphabetical order (`dynamicDir, logLevel, routes`), not the order the author wrote. Forms of features 20 and 29 render from this model.
**Why it matters**: authors cannot control form order, and adding it later changes the dialect.
**Suggested fix**: decide now: keep author order (`serde_json` `preserve_order` or a `Vec`), or add an explicit `x-stackly.order`. Record it in the spec.

### 🟡 Several AC-1 and AC-8 limits have no committed test, `crates/core/tests/provider_rules.rs`
**Area**: core/provider
**Problem**: nothing in `crates/core/tests` or `xtask/tests` exercises the 33rd template (`package.rs:283`, `:397`), the 4 MiB package budget (`package.rs:336-349`), the 256 KiB settings document (`settings.rs:496`), or `too_many_issues` through `from_files` (only `Issues` alone is unit tested). `verify.md` records them as run by hand.
**Suggested fix**: add one test each; they are branching error handling.

### 🟡 `with_issues` drops the `too_many_issues` marker, `apps/daemon/src/map.rs:307-310`
**Area**: routed: apps/daemon
**Problem**: core produces up to 100 issues plus one final marker (101); the catalog cap of 100 items drops exactly the marker, and `issueCount` counts it as an issue (`issueCount=101`, `omitted=1`). The test around `map.rs:573-600` feeds 101 identical issues, not the real shape. (Sub-reviewer finding, not re-run.)
**Suggested fix**: filter the marker in `with_issues` and fold it into the counts, or make it the 100th item.

### 🟡 `cargo xtask provider check` prints unsanitized package entry names, `xtask/src/provider.rs:117`
**Area**: routed: xtask
**Problem**: an entry named `"evil\nok testing-fake 9.9.9 000…"` produces a forged `ok …` line and ESC sequences reach the terminal; exit code stays 1, so it is log forging only. With 110 junk entries the final `too_many_issues` line is counted as a problem (`101 problem(s)`), and a symlinked folder in `providers/` is skipped silently because `file_type().is_dir()` does not follow symlinks (`provider.rs:66`, by inspection). (Sub-reviewer, reproduced by them.)
**Suggested fix**: escape entry names where core builds entry issues (the same names reach the wire through `map.rs`), do not count the marker, and report a symlinked package folder.

### 🟡 Traefik reload is slow to take effect and `backendUrl` is looser than it looks, `providers/traefik/provider.yaml:42-45`, `providers/traefik/schemas/settings.schema.json:50`
**Area**: routed: providers
**Problem**: with `action: none` and `verify: readiness`, `/ping` is 200 immediately while a rewritten `routes.yaml` went live 2.04 s later (Traefik's provider throttle); adding `providers.providersThrottleDuration: 200ms` to the static template brought it to 0.32 s. `backendUrl` accepts a path that Traefik ignores and ports `[0-9]{1,5}` such as 0 and 99999 (502). An invalid `rule` is accepted and Traefik drops only that router. (Sub-reviewer, ran real `traefik` 3.7.7; not re-run.)
**Suggested fix**: set the throttle in `traefik.yaml.tmpl`, tighten the port to 1-65535, drop or document the path group, document the `rule` behavior.

### 🟡 `fake-server init` writes into a folder it then refuses, `crates/fake-server/src/init.rs:36-38`
**Area**: routed: fake-server
**Problem**: `write_argv` runs before `reject_other_entries`, so a second `init` on an initialized folder exits 3 but overwrites `argv-init.json`, and a refused foreign non-empty folder gets an `argv-init.json` dropped into it. Also `dummy_server.rs:732` can hang forever if the child never connects (`server.join().unwrap()`). (Sub-reviewer, reproduced by them.)
**Suggested fix**: check the folder first, or write the argv record only on the success and `--fail` paths; join with a timeout.

### 🟡 `ProviderRevisionKey::from_str` accepts an uppercase digest, `crates/domain/src/provider.rs:348-349`
**Area**: routed: domain
**Problem**: inherited from `Sha256Digest`; the test asserts that `…#<UPPERCASE>` parses to the same key, so `Display(parse(s)) != s`. The `ResourceRef` id (AC-19, "bolak balik lewat parser") is then not canonical text. Decide whether the wire text must be lowercase only and reject uppercase there if so. Add tests for empty parts (`@r#d`, `p@#d`, `p@r#`) and extra `@`. (Sub-reviewer.)

## Nits

- ⚪ `crates/core/src/provider/interpolate.rs:499-519`, two secret tokens glued in one env value (`${secret.a}${secret.a}`) give two identical `secret_not_allowed_here` issues at one location (reproduced); report once per value. (Confirms the verify note.)
- ⚪ `crates/core/src/provider/validate.rs:1532`, the probe transport message reads "tcp probes cannot use a metricsudp endpoint of that transport" (the endpoint name sits where the transport belongs); say "tcp probes need a tcp or unix endpoint, and {name} is udp". (Confirms the verify note.)
- ⚪ `crates/core/src/provider/package.rs:283-291`, `:397-405`, the 33rd and later templates each get a `file_too_large` issue; the code names a size rule, and a package with 40 templates reports 8. Report once, with the count. (Confirms the verify note.)
- ⚪ `crates/core/src/provider/settings.rs:44` and `crates/core/src/provider/issue.rs:185-188`, `Issue.file` is `settings` for value issues, the raw entry name for entry issues, and `""` for the overflow marker, none of which is in the closed list of the spec. Acceptable as is (the wire catalog does not constrain `file`, and forms key on `location` and the error code); reconcile the spec text rather than the code.
- ⚪ `crates/core/src/provider/plan.rs:1006-1022`, `checked_path` uses `Path::components`, which drops an interior `.` and `//`, so `/Volumes/T/./data//x/` passes although the message says "not absolute or holds . or .." (reproduced as `cwd = "/Volumes/T/./data//x/"`). Cosmetic: `..` is caught, and the executor re-checks.
- ⚪ `crates/core/src/provider/settings.rs:507-521`, `SettingsError` is not capped at `MAX_ISSUES`; a 256 KiB document of unknown keys makes tens of thousands of issues before `with_items` truncates them. Cap in core.
- ⚪ `crates/core/src/provider/validate.rs:1882-1885`, `crates/core/src/provider/settings.rs:902-919`, "no control characters" uses `char::is_control` (Cc only), so bidi overrides (U+202A-U+202E, U+2066-U+2069) and U+2028/2029 pass in `displayName`, `description`, and settings `title`; relevant to the trust prompt of feature 37.
- ⚪ `crates/core/src/provider/raw.rs:393`, `:438`, `"default": null` deserializes to `None` (absent), so it is treated as no default (and accepted on a secret field).
- ⚪ `crates/core/src/provider/package.rs:109`, `crates/core/src/provider/plan.rs:509`, `crates/core/src/provider/interpolate.rs:729`, `:792`, "has 1 issues"; some messages end in `..` where it reads as a trailing period.
- ⚪ `crates/core/src/provider/plan.rs:838-846`, env values are not length checked (only argv entries are), so many 4 KiB values could exceed `ARG_MAX` at spawn.
- ⚪ `crates/core/src/provider/validate.rs:2082`, `sample_renders` returns silently when the sample context cannot be built, so the re-parse check would be skipped without a trace.
- ⚪ `crates/core/src/provider/plan.rs:46`, `:64`, `PlanContext` and `RuntimeContext` are all public fields and not `#[non_exhaustive]` (AGENTS.md: structs that will grow); a later field breaks every constructor. Fine if a `new` is planned.
- ⚪ `crates/core/src/provider/versions.rs:185-197`, an `available` artifact with `size: 0` is accepted.
- ⚪ `providers/fake/provider.yaml`, a `sensitive` config sits in `config/`, which `backup.cold.paths` includes, so a cold backup would hold the token; spec 0011 puts sensitive includes in `secrets/`. Only the test provider, but the validator could warn.
- ⚪ `crates/fake-server/src/serve.rs:86-92`, unbounded thread per connection and per read (not total) timeouts; a client sending one byte every 0.45 s was still served after 13.6 s. Acceptable for a loopback double. (Sub-reviewer.)
- ⚪ `providers/nats/schemas/settings.schema.json:46`, `maxPayloadBytes` allows 64 MiB; nats-server warns above 8 MiB. (Sub-reviewer.)
- ⚪ `xtask/src/provider.rs`, flags such as `--json` are taken as folder names, and `xtask/tests/provider_check.rs:13`, `:79` hard-code the three package names. (Sub-reviewer.)

## Open questions of the brief

- **Chained misleading issues**: confirmed, two root causes (first two Minors) plus the spurious `recoverySeconds` issue; fix as described.
- **Two secret tokens glued in one value**: confirmed, Nit.
- **`file_too_large` for the 33 template count**: confirmed, Nit; the closed code list has no count code, so either report once under `unexpected_entry` or add a code through `/architect`.
- **tcp probe to a udp endpoint message**: confirmed, Nit.
- **`Issue.file = settings`**: acceptable; a spec edit, not a code change (Nit above).
- **Plan paths are checked only when a plan uses them**: this is a deliberate design that the new test `every_plan_fails_for_a_folder_it_uses_that_is_not_utf8` pins down, and it is consistent with AC-11 ("all paths in the plan"). It is not a defect: an unchecked folder never reaches a plan. Reword the *Execution plan* sentence ("a context with a non UTF-8 path gives `PathNotUtf8`") to "a folder the plan uses". If you prefer a stable property (init success implies run success), validate the five folders and the runtime root once in `Resolver::new`.
- **Tags**: the reconciled treatment of `!!` and verbatim tags is correct and well tested; I found no way to use a tag to change a value's type past the typed decode.

## Strengths

- Strict decode is done well: `deny_unknown_fields` everywhere, `serde_path_to_error` paths, a duplicate key rejecting JSON visitor, tag and merge key walk, and a documented one issue per structural failure.
- The interpolation matrix is encoded as types (`ValuePosition`, `PathPosition`, `TokenType`), and the `interpolation` fuzz harness asserts the matrix as properties (secret only as a whole env value, no flag from a free setting, executable is one token, no path component escapes), which is the right way to fuzz it.
- Plans are pure, `Serialize` only, secrets are `<secret:field>` in `Debug` and JSON, escapers are shared between render and `materialize` through `render_secret_placeholder`, and output is re-parsed for `yaml` and `json`.
- `read_package` uses only three port methods; the platform implementation opens with `O_NOFOLLOW | O_NONBLOCK`, so symlink swaps and FIFOs are handled below the engine, and the 4 MiB budget is applied per read.
- `provider_no_shell.rs` runs the real dummy server from a plan with a label full of shell metacharacters and compares the recorded argv to the plan; goldens cover all four packages, including route counts 0, 1, and 3.
- Evidence guards (`evidence!`) tie tests to VAL-018, VAL-012, REQ-029, REQ-031; test names are sentences; fixtures use `TestRoot` and `allocate_port`.
- NATS and Traefik configs were checked against the real `nats-server` 2.14.5 and `traefik` 3.7.7 by the sub-reviewer: both start and answer their probes.
- `unwrap` is confined to tests, `expect` messages name invariants, and core compiles for `aarch64-pc-windows-msvc` (`cargo check` run).

## Test coverage

`cargo test -p stackly-core --test provider_rules` for the three uncommitted tests plus `patterns_always_match_the_whole_value` passed (4 tests): `standard_yaml_tags_give_the_untagged_value…`, `every_plan_fails_for_a_folder_it_uses_that_is_not_utf8`, and `a_package_without_config_files_renders_nothing_before_the_spawn` are meaningful (they pin the reconciled behavior and the empty `render` branch), not tautologies.

Covered well: closed sets, unknown fields and tags, capabilities, the interpolation matrix, settings dialect and values, template type and syntax errors with positions, endpoint rules, VAL-002, timeouts, digest, `versions.json`, package layout, plan errors, NATS escaper failures, goldens, preflight, and the no-shell run.

Missing, matching the findings above: a billion laughs case with a large anchor (provider.yaml and template output); a pattern with an unbalanced `)` or a `dataMarker` equivalent; `temp` absent from `managedFilesystem`; four nested loops over 256 items; initialize without `createManagedDirectories`; two configs with one destination; NUL in a literal; U+FFFF in yaml text; negative tests that a single mistake gives a single issue (no chained issue); the 33rd template, the 4 MiB budget, the 256 KiB settings document, and `too_many_issues` through `from_files`. For the secondary areas: closed and full stdout and stderr for the dummy server, `--host=::1`, an overflow of 100 issues through `map.rs` and `xtask`, entry names with control characters, a symlinked folder in `providers/`, and domain parser cases for empty parts.
