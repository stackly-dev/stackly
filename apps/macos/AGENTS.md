# macOS app (SwiftUI)

## Overview

The native macOS GUI: the `Stackly` app target plus the local `StacklyUI` package for views, built with Xcode 27. It is only a client of `stacklyd`. Every read and every mutation goes through `StacklyClient` ([clients/swift/AGENTS.md](../../clients/swift/AGENTS.md)) over IPC, never through FFI into Rust. The app opens one main `Window` scene with the shell (spec 0013), a `Settings` scene, and in Debug builds a "Design Gallery" window; it stays alive after its last window closes. Shared components, view fixtures, and the command map come with features 67 to 69 in `docs/scope/ui-macos.md`.

## Key files

| File | Owns |
|---|---|
| `Stackly.xcodeproj` | App project, created once from the Xcode 27 macOS app template and committed |
| `Stackly/` | App target, a synchronized folder |
| `Packages/StacklyUI/` | Reusable views and screens, previewable without the app |
| `Packages/StacklyUI/Sources/StacklyUI/Shell/` | Main window shell: `AppShell`, `LiveShell`, sidebar, footer, daemon banner, page chrome, close notice (spec 0013) |
| `Packages/StacklyUI/Sources/StacklyUI/Connection/` | `DaemonModel`, `ResourceStore`, `ShellSync`, `ShellRuntime`, and `MutationPath`, the only way a screen reads or mutates (spec 0013) |
| `Packages/StacklyUI/Sources/StacklyUI/Tokens/`, `Status/`, `State/`, `Confirmation/` | Design tokens, status labels, state patterns, and confirmations (spec 0012) |
| `design.md` | Design system: typography, color, spacing, materials, status, state patterns, confirmation, motion, accessibility (spec 0012) |
| `../../docs/mockups/Stackly.dc.html` | UI reference for layout, copy, flows, and every state; sample data in `stackly-data.js` |
| `../../docs/scope/ui-macos.md` | Fidelity rules, known mockup gaps, and which feature owns each screen |

## Commands

```bash
cargo xtask build-app   # from the repo root, runs xcodebuild for scheme Stackly into target/xcode
cargo build -p stacklyd -p stackly-fake-server && cargo build -p stacklyd --example skeleton_seed   # before swift test; without them SkeletonEndToEndTests end as BLOCKED
swift test --package-path apps/macos/Packages/StacklyUI
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
