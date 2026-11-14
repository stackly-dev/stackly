# macOS app (SwiftUI)

## Overview

The native macOS GUI: the `Stackly` app target plus the local `StacklyUI` package for views, built with Xcode 27. It is only a client of `stacklyd`. Every read and every mutation goes through `StacklyClient` ([clients/swift/AGENTS.md](../../clients/swift/AGENTS.md)) over IPC, never through FFI into Rust. The app opens one main `Window` scene with the shell (spec 0013), a `Settings` scene, and in Debug builds a "Design Gallery" window; it stays alive after its last window closes. Shared components, view fixtures, and the command map come with features 67 to 69 in `docs/scope/ui-macos.md`.

## Key files

| File | Owns |
|---|---|
| `Stackly.xcodeproj` | App project, created once from the Xcode 27 macOS app template and committed |
| `Stackly/` | App target, a synchronized folder |
| `Packages/StacklyUI/` | Reusable views and screens, previewable without the app |
| `Packages/StacklyUI/Sources/StacklyUI/Shell/` | Main window shell: `MainWindowRoot` (window content: onboarding or `AppShell`), `AppShell`, sidebar, footer, daemon banner, page chrome, close notice (spec 0013) |
| `Packages/StacklyUI/Sources/StacklyUI/Onboarding/` | First run onboarding: `OnboardingModel`, `OnboardingFlow`, the five steps, and every user facing string in `OnboardingCopy.swift` (spec 0016) |
| `Packages/StacklyUI/Sources/StacklyUI/InstanceDetail/` | Instance detail page and Summary tab: `InstanceDetailModel`, `InstanceDetailService`, `ConnectionComposer`, `DetailFormat`, and the cards (spec 0021) |
| `Packages/StacklyUI/Sources/StacklyUI/CreateInstance/` | New Instance sheet in create and duplicate mode (`CreateMode`): `CreateInstanceModel`, `SchemaFormModel`, `NewInstanceSheet`, `NewInstancePresenter`, and `DuplicateInstanceAction` (specs 0020, 0024) |
| `Packages/StacklyUI/Sources/StacklyUI/InstanceDetail/Configuration/` | Configuration tab (Form, Effective File, Pending Changes, History, drift banner), rename and ports sheets (spec 0029) |
| `Packages/StacklyUI/Sources/StacklyUI/Instances/Logs/`, `Instances/Metrics/` | Logs tab with `InstanceLogsModel` (spec 0027) and Metrics tab with `InstanceMetricsModel`, Swift Charts graph, and process list (spec 0028) |
| `Packages/StacklyUI/Sources/StacklyUI/Instances/InstanceLifecycleActions.swift` | Which start, stop, restart, reload, and force stop actions an instance offers, from its capabilities (spec 0026) |
| `Packages/StacklyUI/Sources/StacklyUI/OpenIn/` | Open In menu from the provider's `openWith` actions, behind the `AppLocator` and `WorkspaceOpener` ports (spec 0070) |
| `Packages/StacklyUI/Sources/StacklyUI/Environments/` | Environments list and detail, plan sheet, lock and local override rows, Up and Down with their banners and confirmations, lease rows: `EnvironmentStore`, `EnvironmentDetailModel`, `EnvironmentPlanPresenter`, `EnvironmentDownModel`, and every string in `EnvironmentCopy.swift` and its siblings (specs 0044 to 0047) |
| `Packages/StacklyUI/Sources/StacklyUI/MenuBar/`, `Notifications/` | `MenuBarModel` and popover with Stop All (spec 0032), and notifications behind `NotificationPosting` (spec 0032 AC-22) |
| `Packages/StacklyUI/Sources/StacklyUI/Settings/` | Settings window tabs, daemon settings and storage locations, startup rows, and the planned daemon restart (specs 0030, 0031, 0032) |
| `Packages/StacklyUI/Sources/StacklyUI/Connection/` | `DaemonModel`, `ResourceStore`, `ShellSync`, `ShellRuntime`, and `MutationPath`, the only way a screen reads or mutates (spec 0013) |
| `Packages/StacklyUI/Sources/StacklyUI/Tokens/`, `Status/`, `State/`, `Confirmation/` | Design tokens, status labels, state patterns, and confirmations (spec 0012) |
| `Packages/StacklyUI/Sources/StacklyUI/Components/` | Shared components for every screen: tile, key value, copy and console, actions and menus, feedback, sheet, checklist, staged progress, inspector, log, diff (spec 0067) |
| `Packages/StacklyUI/Sources/StacklyUI/Commands/` | Native menu actions, their command map, and publication from the active window |
| `Packages/StacklyUI/Sources/StacklyUI/Fixtures/` | `#if DEBUG` fixture scenarios built from the generated DTOs, `ShellBootstrap.fixture`, and `FixtureClock`, used by previews, snapshots, and the UI tests (spec 0068) |
| `Packages/StacklyUI/Tests/StacklyUITests/SnapshotTests/` | Snapshot suites; reference PNGs in `__Snapshots__` (spec 0068) |
| `StacklyAppUITests/` | XCUITest target for the pointer smoke flows, run by `cargo xtask ui-test` (spec 0068) |
| `design.md` | Design system: typography, color, spacing, materials, status, state patterns, confirmation, motion (spec 0012) |
| `../../docs/mockups/Stackly.dc.html` | UI reference for layout, copy, flows, and every state; sample data in `stackly-data.js` |
| `../../docs/scope/ui-macos.md` | Fidelity rules, known mockup gaps, and which feature owns each screen |

## Commands

```bash
cargo xtask build-app   # from the repo root, runs xcodebuild for scheme Stackly into target/xcode
cargo build -p stacklyd -p stackly-fake-server && cargo build -p stacklyd --example skeleton_seed   # before swift test; without them SkeletonEndToEndTests end as BLOCKED
swift test --package-path apps/macos/Packages/StacklyUI
STACKLY_SNAPSHOT_RECORD=1 swift test --package-path apps/macos/Packages/StacklyUI --filter <Suite>   # rewrite changed snapshot references, then review them
cargo xtask snapshot review   # old and new PNGs side by side in target/snapshot-review/index.html
cargo xtask ui-test           # XCUITest; needs Automation Mode turned on by the user, and an unlocked screen
```

## Conventions

- Swift 6 language mode with full strict concurrency. The app target and `StacklyUI` use `MainActor` as default isolation (`.defaultIsolation(MainActor.self)` in the package manifest).
- Add files through the synchronized folders. Never edit `project.pbxproj` just to add a file.
- Organize by feature: one folder per screen or area (Overview, Instances, Catalog, Settings), holding its views and view models.
- Target settings that differ from the template: `ARCHS = arm64`, `MACOSX_DEPLOYMENT_TARGET = 27.0`, bundle ID `com.ojiepermana.stackly`, ad hoc signing until feature 15, and App Sandbox off (the app must reach the daemon socket and bundle helpers).
- Package manifests write the platform as a string, `.macOS("27.0")`, and `swift-tools-version` matches the Swift in the pinned Xcode.
- Swift Testing for unit tests, XCUITest for UI tests. No force unwrap or `try!` outside tests.
- Swift Testing suites are a `struct` with `@Test func` methods named as a camelCase phrase that states the behavior (`fixtureDecodesToCurrentVersion`). Indent with 4 spaces.
- The app target gets its default isolation from the build setting `SWIFT_DEFAULT_ACTOR_ISOLATION = MainActor` (with `SWIFT_APPROACHABLE_CONCURRENCY = YES`); the packages use `.defaultIsolation(MainActor.self)` instead.
- Where the mockup imitates a macOS control in HTML (table, picker, toggle, sheet, popover, menu), use the native SwiftUI control. UI copy is English and comes from the mockup.
- Design system: build all UI to `design.md`; colors, status, state patterns, and confirmations come from its tokens and helpers in `StacklyUI` (`Tokens/`, `Status/`, `State/`, `Confirmation/`), never a literal color or a new pattern on a screen (spec 0012).
- Build screens from `Components/` (spec 0067); never a screen-local tile, feedback toast, sheet footer, checklist, progress, inspector, log, or diff view. Actions go through `StacklyAction` and the window's `ActionHost`, feedback through `FeedbackCenter`, and copy through `CopyButton`; a disabled action shows its reason, and a destructive action always confirms.
- `StacklyUI` depends on the `StacklyClient` product, and its tests also on `StacklyTestSupport`. Screens read stores registered through `ShellSync` and mutate only through `MutationPath`, gated by `MutationAvailability` (spec 0013).
- Delayed spinners use `DelayedProgress`, which follows `loadingDelay` (nil in the app, so 300 ms; zero in fixtures and snapshots, so the spinner is in the first frame). A spinner inserted after the first frame never settles in a snapshot (spec 0068).
- Accent appears only through native controls, native links, and `StacklyColor.selectionRing` for a custom selection ring; `controlAccentColor` or `accentColor` outside `Tokens/StacklyColor.swift` fails the source rule (spec 0012).
- A page under `.stacklyInspector` takes the size the column offers, since the component sets its frame; the page never sets its own `minWidth` (spec 0067).
- A screen retries a failed load only through `ResourceStore.refresh()`, which waits for a running snapshot, and never adds its own reload loop; `ShellSync` reloads failed stores once on `.caughtUp` (spec 0013).
- A screen opens another screen with a target through `ShellNavigator`: `openInstance(_:)`, `openOperation(_:)`, and `openDiagnostics(focus:)` leave a one shot request (`operationRequest`, `diagnosticsRequest`) that the page takes once; `ShellDestination` cases carry no target (specs 0034, 0035).
- The Metrics tab polls `instance.get_metrics` every `intervalSeconds` only while it is on screen (`onAppear` and `onDisappear`), sends its `historyId` and `afterSeq`, and replaces the whole history on `historyReset` (spec 0028). The menu bar reads the shell's `DaemonModel` and opens no connection of its own (spec 0032).
- Tests never rely on wall clock deadlines the main actor can starve: waits use `ResponsiveTimer`, process watchdogs run off the main actor, and source rule suites are `@Suite(.serialized) nonisolated` so their scans leave the main actor free.

## Gotchas

- `docs/mockups/support.js` is only the prototype runtime, `docs/mockups/_ds/` (Modernist) is not a design reference, and the mockup's Tweaks panel is not app settings.
- When the mockup and scope or spec disagree on behavior, scope and spec win. Some mockup numbers are only examples (metric sampling, log retention, Appearance options); `ui-macos.md` lists them.
- The mockup draws a fixed 1280 × 800 window. The real window is resizable.

## Agent skills

- [swiftui-expert-skill](../../.claude/skills/swiftui-expert-skill/): `avdlee/swiftui-agent-skill`, SwiftUI state, layout, macOS scenes, Liquid Glass, accessibility, previews
- [swift-concurrency](../../.claude/skills/swift-concurrency/): `avdlee/swift-concurrency-agent-skill`, actors, `Sendable`, `MainActor`, Swift 6 strict concurrency
- [swift-testing-pro](../../.claude/skills/swift-testing-pro/): `twostraws/swift-testing-agent-skill`, Swift Testing

Declined: swiftui-pro, xcode-build-fixer, macos-design-guidelines

MCP servers: XcodeBuildMCP (recommended)

## Related specs

- [0001 Stack dan arsitektur](../../docs/specs/0001-stack-arsitektur/index.md)
- [0012 Design system dan fondasi UI](../../docs/specs/0012-design-system-ui-foundation/index.md)
- [0013 App shell SwiftUI dan Swift client](../../docs/specs/0013-app-shell-swift-client/index.md)
- [0067 Komponen dasar Stackly](../../docs/specs/0067-komponen-dasar-stackly/index.md)
- [0068 Fixture tampilan dan cek visual](../../docs/specs/0068-fixture-tampilan-cek-visual/index.md)
- [0069 Menu aplikasi, perintah, dan shortcut](../../docs/specs/0069-menu-aplikasi-perintah-shortcut/index.md)
- [0026 Stop, restart, dan reload](../../docs/specs/0026-stop-restart-reload/index.md)
- [0027 Log per instance](../../docs/specs/0027-log-per-instance/index.md)
- [0028 Metrics dan health](../../docs/specs/0028-metrics-dan-health/index.md)
- [0029 Edit config, nama, dan endpoint](../../docs/specs/0029-edit-config-nama-endpoint/index.md)
- [0030 Rekonsiliasi dan restart daemon terencana](../../docs/specs/0030-reconciliation-restart-daemon-terencana/index.md)
- [0031 Demand, autostart, dan login](../../docs/specs/0031-demand-autostart-login/index.md)
- [0032 Menu bar dan Settings](../../docs/specs/0032-menu-bar-settings/index.md)
- [0070 Buka instance di aplikasi klien](../../docs/specs/0070-buka-instance-aplikasi-klien/index.md)
- [0033 Crash loop dan error start](../../docs/specs/0033-crash-loop-error-start/index.md)
- [0034 Doctor dan diagnostic bundle](../../docs/specs/0034-doctor-diagnostic-bundle/index.md)
- [0035 Operations, activity, dan audit](../../docs/specs/0035-operations-activity-audit/index.md)
- [0044 Manifest environment: validate dan plan](../../docs/specs/0044-manifest-environment-validate-plan/index.md)
- [0045 Lockfile dan override lokal](../../docs/specs/0045-lockfile-override-lokal/index.md)
- [0046 Env up dan dependency graph](../../docs/specs/0046-env-up-dependency-graph/index.md)
- [0047 Env down, shared instance, dan lease](../../docs/specs/0047-env-down-shared-instance-lease/index.md)

_Drafted by /audit from the repo, worth a quick human pass. Edit freely: once a line stops matching this draft, later runs treat it as curated and will flag rather than overwrite it._

## Interaction policy

Use native pointer controls and the regular light and dark palette. Do not add custom screen reader labels or announcements, keyboard focus or shortcuts, accessibility display overrides, or UI test identifiers. UI smoke tests locate native controls by their visible labels. Snapshots cover light, dark, and narrow layouts.
