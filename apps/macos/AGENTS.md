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
| `Packages/StacklyUI/Sources/StacklyUI/Connection/` | `DaemonModel`, `ResourceStore`, `ShellSync`, `ShellRuntime`, and `MutationPath`, the only way a screen reads or mutates (spec 0013) |
| `Packages/StacklyUI/Sources/StacklyUI/Tokens/`, `Status/`, `State/`, `Confirmation/` | Design tokens, status labels, state patterns, and confirmations (spec 0012) |
| `Packages/StacklyUI/Sources/StacklyUI/Components/` | Shared components for every screen: tile, key value, copy and console, actions and menus, feedback, sheet, checklist, staged progress, inspector, log, diff (spec 0067) |
| `Packages/StacklyUI/Sources/StacklyUI/Commands/` | `CommandMap` (the only place a shortcut is written), `SystemShortcuts`, published commands, and `StacklyCommands` (spec 0069) |
| `Packages/StacklyUI/Sources/StacklyUI/Fixtures/` | `#if DEBUG` fixture scenarios built from the generated DTOs, `ShellBootstrap.fixture`, and `FixtureClock`, used by previews, snapshots, and the UI tests (spec 0068) |
| `Packages/StacklyUI/Sources/StacklyUI/Accessibility/AccessibilityID.swift` | Accessibility identifiers, `<scope>.<part>[.<key>]` |
| `Packages/StacklyUI/Tests/StacklyUITests/SnapshotTests/` | Snapshot suites; reference PNGs in `__Snapshots__` (spec 0068) |
| `StacklyAppUITests/` | XCUITest target for the shell, gallery, menus, and accessibility audits, run by `cargo xtask ui-test` (spec 0068) |
| `design.md` | Design system: typography, color, spacing, materials, status, state patterns, confirmation, motion, accessibility (spec 0012) |
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
- Shortcuts live only in `CommandMap`; screens publish actions through `.publishesCommands`, and menus inside a window set no shortcut. To add a command, follow *Menambah perintah* in spec 0069 and the "Commands and shortcuts" section of `design.md`.
- Every page has a snapshot suite on the spec 0068 harness (a source rule checks it). Underscored SwiftUI setters (`_colorSchemeContrast`, `_accessibilityReduceTransparency`) are allowed only in tests and `#if DEBUG` code. The UI test target uses `SWIFT_DEFAULT_ACTOR_ISOLATION = nonisolated` with `@MainActor` test classes, because `XCTestCase` initializers are nonisolated.
- `StacklyUI` depends on the `StacklyClient` product, and its tests also on `StacklyTestSupport`. Screens read stores registered through `ShellSync` and mutate only through `MutationPath`, gated by `MutationAvailability` (spec 0013).

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

_Drafted by /audit from the repo, worth a quick human pass. Edit freely: once a line stops matching this draft, later runs treat it as curated and will flag rather than overwrite it._
