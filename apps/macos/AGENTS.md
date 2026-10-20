# macOS app (SwiftUI)

## Overview

The native macOS GUI: the `Stackly` app target plus the local `StacklyUI` package for views, built with Xcode 27. It is only a client of `stacklyd`. Every read and every mutation goes through `StacklyClient` ([clients/swift/AGENTS.md](../../clients/swift/AGENTS.md)) over IPC, never through FFI into Rust. The feature 1 scaffold holds only a placeholder `RootView` shown in one `WindowGroup`; the shell and shared components come with features 12, 13, and 67 to 69 in `docs/scope/ui-macos.md`.

## Key files

| File | Owns |
|---|---|
| `Stackly.xcodeproj` | App project, created once from the Xcode 27 macOS app template and committed |
| `Stackly/` | App target, a synchronized folder |
| `Packages/StacklyUI/` | Reusable views and screens, previewable without the app |
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

_Drafted by /audit from the repo, worth a quick human pass. Edit freely: once a line stops matching this draft, later runs treat it as curated and will flag rather than overwrite it._
