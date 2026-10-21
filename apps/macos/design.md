# Stackly design system

The visual rules every Stackly screen follows: type, color, spacing, materials, status, page states, confirmation, motion, and accessibility. You build screens from these rules and the `StacklyUI` APIs they name, without deciding colors, states, or confirmation again. The reasons behind each rule live in [spec 0012](../../docs/specs/0012-design-system-ui-foundation/index.md); this file holds the rules. When the mockup disagrees with a rule here, the rule wins.

Token values live in code, in `Packages/StacklyUI/Sources/StacklyUI/Tokens/`. This file names each token, its mockup source, and its Swift API.

The Debug build has a **Design Gallery** window (Window menu) with one page each for Tokens, Status, States, and Confirmation. Use it to check a change with the real system settings (Increase Contrast, Reduce Transparency, Differentiate Without Color, Reduce Motion) and in light and dark. Every gallery page also has three Xcode previews: light, dark, and narrow width. The gallery is not part of Release builds.

## Typography

Only the semantic macOS text styles. A mockup size maps to the nearest style. Weights are regular, medium (used by native controls), semibold, and bold.

| Role | Mockup (px, weight) | Swift | macOS size |
| --- | --- | --- | --- |
| Onboarding title | 28, 700 | `.largeTitle.bold()` | 26 |
| Overview summary number | 28, 600 | `.largeTitle.weight(.semibold).monospacedDigit()` | 26 |
| Onboarding step title | 24, 700 | `.title.bold()` | 22 |
| Metric value | 20, 600 | `.title.weight(.semibold).monospacedDigit()` | 22 |
| Page placeholder title, detail panel title | 17, 600 to 700 | `.title2.weight(.semibold)` | 17 |
| Section placeholder title, large card title | 15, 600 to 700 | `.title3.weight(.semibold)` | 15 |
| Section heading ("Needs Attention") | 13, 700 | `.headline` | 13 |
| Body text | 13, 400 | `.body` | 13 |
| Emphasized row title | 13, 600 | `.body.weight(.semibold)` | 13 |
| Secondary text, banner body, monospace table cell | 12 | `.callout` (monospace: `.callout.monospaced()`) | 12 |
| Toolbar subtitle, sidebar group header, tag, caption | 11 | `.subheadline` (tags and headers: `.subheadline.weight(.semibold)`) | 11 |
| Smallest metadata | 10 to 10.5 | `.footnote` | 10 |

- Numbers (counts, ports, times, sizes) use `.stacklyNumeric()`, which is `.monospacedDigit()`.
- Paths, endpoints, digests, and error codes use `.stacklyCode()`: `.callout.monospaced()` with text selection on.
- A symbol takes the text style around it. Only a symbol that needs its own size uses `Image(systemName:).stacklySymbol(size:relativeTo:)`, or `.stacklySymbol(_:)` with a named `ScaledSize`. That helper is the only fixed font size in Stackly.
- Titles and body text may wrap. A status label uses `fixedSize(horizontal: true, vertical: false)`. A status detail and secondary text inside a table row truncate at the tail (`.tail`).
- The source rule test fails on `.font(.system(size:`, `Font.system(size:`, `.custom(`, or `NSFont` anywhere outside `Tokens/Typography.swift`.
- Large text does not scale on macOS 27; see [Accessibility](#accessibility) for how it is checked instead.

## Color

### System colors

The main neutrals come from the system, so macOS supplies every appearance and the Increase Contrast variants.

| Use | System color | Swift |
| --- | --- | --- |
| Primary text | `labelColor` | `.primary` (the default foreground) |
| Hairlines and separators | `separatorColor` | `Color(nsColor: .separatorColor)` |
| Background of pages built from a native `Table`, `List`, or `Form` | `windowBackgroundColor`, `controlBackgroundColor` | the system default; set nothing |
| Disabled text, decorative icons (placeholder symbols, chevrons) | `tertiaryLabelColor` | `StacklyColor.decorative` |
| Native control backgrounds | as drawn by the control | never restyled |

- Never use `.secondary`, `.tertiary`, `secondaryLabelColor`, or `tertiaryLabelColor` directly: `secondaryLabelColor` is only about 3.95:1 in light. Use `StacklyColor.textSecondary`, and `StacklyColor.decorative` for disabled text and decorative icons. The source rule test fails on these outside `Tokens/StacklyColor.swift`.
- A native API that accepts a style for its secondary text (for example `LabeledContent`, a `Section` footer) gets `StacklyColor.textSecondary`. Secondary text the system draws with no way to change it (`Table` column headers, the toolbar title) is accepted as it is.
- Accent is never a custom text color. It appears only through native controls, the selection tint, and native links. `Color.accentColor` and `.tint` as a foreground style fail the source rule test. Accent and the Liquid Glass mode (tinted or clear) follow System Settings; Stackly has no setting for either.
- `Stackly/Assets.xcassets` holds only `AppIcon` and an empty `AccentColor`. No custom color goes into any asset catalog, and custom tokens have no `NSColor` accessor. Tests check both.

### Custom tokens

A `ColorToken` holds one sRGB value per appearance. Use it as a `ShapeStyle` (`.foregroundStyle(StacklyColor.textSecondary)`, `.background(StacklyColor.surfaceWindow)`): it reads `colorScheme` and `colorSchemeContrast` from the environment, so text, backgrounds, and borders always pick the same variant, also when the Appearance setting forces a scheme with `preferredColorScheme` (feature 32). `value(scheme:contrast:)` returns the color for one fixed appearance.

| Token | Mockup | Light | Dark | HC light | HC dark | Swift | Use |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Window surface | `--win` | #F5F5F7 | #1E1E20 | #F5F5F7 | #1E1E20 | `StacklyColor.surfaceWindow` | content background of pages built from custom cards |
| Card surface | `--card` | #FFFFFF | #2A2A2D | #FFFFFF | #2A2A2D | `StacklyColor.surfaceCard` | custom cards, through `.cardSurface()` |
| Console surface | `--console` | #FFFFFF | #18181A | #FFFFFF | #18181A | `StacklyColor.surfaceConsole` | code blocks, log views |
| Secondary text | `--text2` | #606065 | #AEAEB3 | #4E4E52 | #C0C0C4 | `StacklyColor.textSecondary` | every secondary text; also the `neutral` tone text |

- Pages built from custom cards (Overview, Summary, Diagnostics, and the like) put `StacklyColor.surfaceWindow` behind their content, so a dark card (#2A2A2D) is always lighter than its background, as in the mockup. Pages built from a native `Table`, `List`, or `Form` keep the system background.
- `.cardSurface(radius: Radius = .medium)` draws `surfaceCard` with `radius` corners and a hairline `separatorColor` border (one pixel; the mockup's 0.5 px ring) that becomes 1 pt under Increase Contrast. It adds no padding; pad the content with `Spacing.cardInsets`.

### Tones

Five tones with fixed meanings. A tone is never shown by color alone: it always comes with a label, a symbol, or both.

| Tone | Meaning | Fill (`Tone.fill`) | Text light | Text dark | Text HC light | Text HC dark | Symbol (`Tone.symbol`) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `Tone.positive` | working as expected: running and ready, verified, tested | `systemGreen` | #18712D | #4ADE74 | #135A24 | #4ADE74 | `checkmark.circle.fill` |
| `Tone.caution` | in transition or needing attention without failing: starting, recovering, unknown, drift, partial, stale, experimental | `systemOrange` | #965000 | #FFB340 | #774000 | #FFB340 | `exclamationmark.triangle.fill` |
| `Tone.critical` | failed or blocking: failed, error, conflict | `systemRed` | #B8160C | #FF867F | #9D120A | #FFAAA5 | `xmark.circle.fill` |
| `Tone.neutral` | idle or plain information: stopped, retained, done, cancelled, not tested | `systemGray` | #606065 | #AEAEB3 | #4E4E52 | #C0C0C4 | `info.circle.fill` |
| `Tone.informative` | being worked on at the user's request: a running operation, install progress | `systemBlue` | #0759BA | #62AEFF | #064C9E | #8EC5FF | `ellipsis.circle.fill` |

- `Tone.text` is a `ColorToken`. The `neutral` text is `StacklyColor.textSecondary`. Transitional states use `caution` (orange), as in the mockup.
- The fill is a system color, so macOS supplies its Increase Contrast variant. A fill is never text and always comes with a label, so it is never the only carrier of meaning.
- `.toneSurface(_ tone:, on: SurfaceToken = .card, strength:)` draws an opaque tint: the base (`.card` is `surfaceCard`, `.window` is `surfaceWindow`) with the tone fill mixed in at 16% for `.badge` (in a `Capsule`) or 12% for `.banner` (in a rounded rectangle with `Radius.medium`). `.toneSurface(_:on:strength:in:)` takes another shape. Under Increase Contrast it adds a 1 pt border in the tone fill.
- The tint is mixed in gamma encoded sRGB, like the mockup's CSS `color-mix(in srgb, ...)`: result = fill × a + base × (1 − a).
- A tone surface never sits on glass or on a system background.
- Tone text goes only on a plain surface from the contrast pairs below or on a tint of the same tone, never on another tone's tint. In List and Table rows, the sidebar, menus, and popovers, status text uses `.primary` instead (see [Status](#status)).
- A new status set (for example runtime states in feature 18) maps its states to these five tones by the Meaning column, then adds its table to [Status](#status).

### Contrast pairs

| Group | Text | Surface | Normal minimum | HC minimum | Lowest measured on macOS 27 (light, dark, HC light, HC dark) |
| --- | --- | --- | --- | --- | --- |
| P1 | `labelColor`, `textSecondary` | `surfaceWindow`, `surfaceCard`, `surfaceConsole`, `windowBackgroundColor`, `controlBackgroundColor` | 4.5:1 | 7:1 | 5.74, 6.48, 7.61, 7.89 |
| P2 | text of the five tones | every P1 surface | 4.5:1 | 7:1 | 5.60, 6.11, 7.61, 7.87 |
| P3 | text of tone X | `toneSurface(X)` `.badge` and `.banner` over `.card` and `.window` | 4.5:1 | 4.5:1 | 4.95, 5.00, 6.19, 5.99 |
| P4 | `labelColor`, `textSecondary` | `toneSurface` of every tone, `.banner`, over `.card` and `.window` | 4.5:1 | 4.5:1 | 4.91, 5.17, 6.50, 6.30 |

- The normal minimum applies in light and dark, the HC minimum in high contrast light and dark. Text in a `.row` context uses the system `.primary` and is not in the table.
- `StacklyUITests` (`ContrastTests`) proves every pair in all four appearances, from the token table and from system colors resolved per `NSAppearance`. `labelColor` has alpha (0.85), so it is composited over the surface before measuring.
- On macOS 27 the system resolves `windowBackgroundColor` and `controlBackgroundColor` to #FFFFFF in light and #1E1E1E in dark, and the high contrast appearances resolve the same fills as the standard ones.
- If a later macOS makes a pair fail, move the token value as little as needed to pass, record the new value here, and update spec 0012 through `/architect`.

## Spacing and radius

Padding and stacks use fixed points from this scale and never scale with text size.

| Swift | Points |
| --- | --- |
| `Spacing.s2` | 2 |
| `Spacing.s4` | 4 |
| `Spacing.s6` | 6 |
| `Spacing.s8` | 8 |
| `Spacing.s12` | 12 |
| `Spacing.s14` | 14 (the horizontal padding of most mockup rows and cards) |
| `Spacing.s16` | 16 |
| `Spacing.s22` | 22 |

Named insets:

| Inset | Swift | Points |
| --- | --- | --- |
| Page, horizontal | `Spacing.pageHorizontal` | 22 |
| Gap between sections | `Spacing.sectionGap` | 22 |
| Card padding | `Spacing.cardInsets` | 14 vertical, 16 horizontal |
| Grouped list row (mockup) | `Spacing.rowInsets` | 11 vertical, 14 horizontal |
| Compact row | `Spacing.compactRowInsets` | 9 vertical, 14 horizontal |

Radius applies to custom surfaces only. Native controls, sheets, and dialogs keep the shapes the system draws.

| Swift | Points | Use | Mockup |
| --- | --- | --- | --- |
| `Radius.small` | 8 | code blocks, custom table rows | 8 |
| `Radius.medium` | 14 | grouped lists, cards, banners (the `.cardSurface()` default) | 12 and 14 |
| `Radius.large` | 16 | summary cards, failure banners, panels | 16 and 18 |
| `Capsule` | | `ToneBadge` and other tone badges | pill |

Mockup radii of 24 to 26 belong to sheets and dialogs, which the system draws. Read a radius in points with `radius.value`.

Scaled sizes are named defaults for `@ScaledMetric`. Declare one in the view that uses it: `@ScaledMetric(.statusDot) private var dotSize`.

| Swift | Base | Relative to | Use |
| --- | --- | --- | --- |
| `ScaledSize.statusDot` | 7 | `.body` | dot in front of a status label |
| `ScaledSize.bannerSymbol` | 20 | `.body` | tone symbol of a banner |
| `ScaledSize.badgeHeight` | 20 | `.subheadline` | minimum height of a tone badge |
| `ScaledSize.placeholderPageSymbol` | 44 | `.title2` | symbol of a page placeholder |
| `ScaledSize.placeholderSectionSymbol` | 34 | `.title3` | symbol of a section placeholder |
| `ScaledSize.rowMinHeight` | 34 | `.body` | minimum height of a custom row |

- A row never has a fixed height, only a `minHeight`.
- Native controls (buttons, segmented controls, toggles, fields, tables) are never restyled; their shape and size come from the system.
- A custom click area is at least 24 × 24 pt (`HitTarget.minimum`), through `frame(minWidth:minHeight:)` and `contentShape`, even when its drawing is smaller.

## Materials

- Liquid Glass comes only from native containers and controls: the `NavigationSplitView` sidebar, toolbar items, sheets, popovers, menus, dialogs, and inspectors. macOS handles tinted or clear glass, Reduce Transparency, and Increase Contrast for them, following System Settings.
- No `.glassEffect`, `GlassEffectContainer`, `.buttonStyle(.glass)`, `.buttonStyle(.glassProminent)`, or custom `Material` (including `.bar` and `.ultraThinMaterial` and its siblings) in Stackly's own content. The source rule test fails on any of them in `StacklyUI` or the app target.
- Every custom surface is opaque: `surfaceWindow`, `surfaceCard`, `surfaceConsole`, and `toneSurface`.
- The mockup's translucent window background (`--win-glass` over the wallpaper) is not copied. The window uses standard macOS behavior.
- Mockup overlays drawn as glass become native presentations: the confirmation dialog is `.stacklyConfirmation(item:)` (see [Confirmation](#confirmation)), the More menu is a `Menu`, and feedback after an action (feature 67) uses an opaque surface or a system presentation.

## Status

Status shows as text first and color second. Every status is a `StatusAppearance` (label, tone, symbol, and at most one detail), drawn by `StatusLabel` or `ToneBadge`. The code lives in `Packages/StacklyUI/Sources/StacklyUI/Status/`.

### UI status enums

The app shell (spec 0013) maps the daemon's DTOs to these enums with an exhaustive switch, so a new protocol state fails to compile until the tables below cover it. The daemon applies `effective(now, max_age)` (spec 0004) before it sends readiness and health, so the UI never computes freshness itself.

| Swift | Cases |
| --- | --- |
| `LifecycleStatus` | `uninitialized`, `initializing`, `stopped`, `starting`, `running`, `recovering`, `stopping`, `failed`, `unknown`, `retained` (the ten states of spec 0004) |
| `ReadinessStatus` | `ready`; `awaitingProbe` (`not_ready` with reason `awaiting_probe`); `notReady(detail: String?)` (`not_ready` with any other reason, with the daemon's detail); `unknown` (including a stale result) |
| `HealthStatus` | `unknown`, `healthy`, `degraded`, `unhealthy` |
| `OperationStatus` | `queued`, `running`, `cancelRequested`, `succeeded`, `failed`, `cancelled`, `needsRecovery` |

All of them, `StatusDetail`, and `StatusAppearance` are `nonisolated` and `Sendable`, so the adapter can build them off the main actor.

### Instance status

`StatusAppearance.instance(lifecycle:readiness:health:)`. Readiness is read only while `running`, and health only while readiness is `ready`. "Running" is `positive` only when running and ready.

| Lifecycle | Readiness and health | Label | Tone | Status symbol | Detail (detail tone) |
| --- | --- | --- | --- | --- | --- |
| `uninitialized` | | Not Initialized | `neutral` | `circle.dashed` | none |
| `initializing` | | Initializing… | `caution` | `hourglass.circle.fill` | none |
| `stopped` | | Stopped | `neutral` | `stop.circle.fill` | none |
| `starting` | | Starting… | `caution` | `hourglass.circle.fill` | none |
| `running` | `awaitingProbe` | Starting… | `caution` | `hourglass.circle.fill` | Waiting for probe (`caution`) |
| `running` | `notReady(detail)` | Not Ready | `caution` | `exclamationmark.circle.fill` | the daemon's detail, or "Readiness check failed" when it is missing or only white space (`caution`) |
| `running` | `unknown` | Running | `caution` | `questionmark.circle.fill` | Readiness unknown (`caution`) |
| `running` | `ready`, health `healthy` or `unknown` | Running | `positive` | `checkmark.circle.fill` | none |
| `running` | `ready`, health `degraded` | Running | `positive` | `checkmark.circle.fill` | Degraded (`caution`) |
| `running` | `ready`, health `unhealthy` | Running | `positive` | `checkmark.circle.fill` | Unhealthy (`critical`) |
| `recovering` | | Recovering… | `caution` | `arrow.clockwise.circle.fill` | none |
| `stopping` | | Stopping… | `caution` | `hourglass.circle.fill` | none |
| `failed` | | Failed | `critical` | `xmark.circle.fill` | none (the screen's failure banner shows the reason, feature 33) |
| `unknown` | | Unknown | `caution` | `questionmark.circle.fill` | Process identity not confirmed (`caution`) |
| `retained` | | Retained | `neutral` | `archivebox.circle.fill` | none |

Each state has its own symbol shape, so Stopped and Retained stay apart without color. Transitional labels end with the ellipsis character (…), never three periods.

### Operation status

`StatusAppearance.operation(state:partial:)`. `partial` matters only for `succeeded`. Operations show as a `ToneBadge`, so the symbol is the tone symbol (`Tone.symbol`) and there is never a detail.

| State | Label | Tone |
| --- | --- | --- |
| `queued` | Queued | `neutral` |
| `running` | Running | `informative` |
| `cancelRequested` | Cancelling… | `caution` |
| `succeeded` | Done | `neutral` |
| `succeeded`, `partial` true | Partial | `caution` |
| `failed` | Failed | `critical` |
| `cancelled` | Cancelled | `neutral` |
| `needsRecovery` | Needs Recovery | `critical` |

The mockup has five of these labels (Running, Stopped, Starting…, Stopping…, Failed) and the detail "Waiting for probe". Every other label is new copy from spec 0012.

### StatusLabel

`StatusLabel(_ appearance: StatusAppearance, context: StatusContext = .standard)` draws a dot, the label, and the optional detail (mockup: a 7 px dot, a 6 px gap, the label, then " · " and the readiness text).

- **Dot.** 7 pt (`ScaledSize.statusDot`) in the tone fill, `Spacing.s6` before the label.
- **Detail.** After a "·" separator, in the detail's own tone. At most one.
- **Font.** The label takes the font around it: `.body` in a card, `.callout` in a table cell (mockup 12 px).
- **`.standard` context** (cards, banners, sheets, panels). The label uses the `critical` tone text for `critical` and `StacklyColor.textSecondary` for every other tone (mockup `stFg`). The detail uses its tone's text color.
- **`.row` context** (`List` and `Table` rows, the sidebar, menus, popovers). The label and detail use `.primary`, because an inactive selection or glass behind the row cannot be guaranteed for tone text (red text on a dark inactive selection drops to about 4.03:1). While the row is selected and `backgroundProminence` is `.increased`, the dot and symbols use `.primary` too. Pass `context: .row` in every row; never put tone text in a row.
- **Differentiate Without Color.** The dot becomes the status symbol in the tone fill, and the detail starts with its tone symbol. Symbols take the font around them.
- **Truncation.** The label is never cut (`fixedSize(horizontal: true, vertical: false)`); the detail truncates at the tail first, on one line.
- **VoiceOver.** One element: `Status: <label>`, plus `, <detail>` when there is one ("Status: Starting…, Waiting for probe"). The dot, symbols, and separator are hidden.
- **Motion.** A change of status crossfades with `StacklyMotion.crossfade`. Nothing moves or scales.

### ToneBadge

`ToneBadge(_ appearance: StatusAppearance)` is a tag: the label in `.subheadline.weight(.semibold)` and the tone text color, on `toneSurface(tone, strength: .badge)` (a capsule, 16% tint over `surfaceCard`).

- At least 20 pt high (`ScaledSize.badgeHeight`) and 9 pt horizontal padding (mockup `padding: 0 9px`; the one padding off the `Spacing` scale, kept inside `ToneBadge`).
- Increase Contrast adds a 1 pt border in the tone fill, through `toneSurface`.
- Differentiate Without Color puts the tone symbol before the label. The symbol uses the tone text color, not the fill, so it keeps AA contrast on the tint.
- VoiceOver reads the label. The label never truncates.
- Use it for operation states everywhere (Overview, Operations, the menu bar), so one state always has one tone.

### Section icons

`SectionStyle` gives each top level section a `symbol` and a system `color`. `SectionIcon(_ section: SectionStyle)` draws the symbol in that color with the font around it, hidden from VoiceOver (`accessibilityHidden(true)`), because the section label carries the meaning. The app shell (spec 0013) places it in the sidebar.

| Section | Swift | Symbol | System color |
| --- | --- | --- | --- |
| Overview | `SectionStyle.overview` | `square.grid.2x2` | `systemBlue` |
| Instances | `SectionStyle.instances` | `server.rack` | `systemGreen` |
| Environments | `SectionStyle.environments` | `square.3.layers.3d` | `systemPurple` |
| Catalog | `SectionStyle.catalog` | `book` | `systemOrange` |
| Runtimes | `SectionStyle.runtimes` | `shippingbox` | `systemTeal` |
| Operations | `SectionStyle.operations` | `arrow.triangle.2.circlepath` | `systemIndigo` |
| Diagnostics | `SectionStyle.diagnostics` | `waveform.path.ecg` | `systemPink` |

### New status sets

A new status set (for example runtime states in feature 18) maps each state to one of the five tones by the Meaning column in [Tones](#tones), gives each state its own SF Symbol shape, builds its appearances with `StatusAppearance(label:tone:symbol:detail:)`, and adds its table here. `StatusAppearanceTests` checks every row of the tables above and that every symbol exists on macOS 27.

### Checking status in the gallery

The Status page of the Design Gallery shows every instance row in both contexts (the row context in a native `List`, so selection can be checked), every operation badge, and the section icons. Its Differentiate Without Color columns use an internal environment value, `showsStatusSymbols`, because the system setting cannot be set in a preview; in the real app, turn on the system setting instead.

## State patterns

Every page or section that loads data from the daemon has one of five shapes. Build the shape with `ViewState.resolve` and show it with `StateView`; never pick a shape by hand. The code lives in `Packages/StacklyUI/Sources/StacklyUI/State/`, and the States page of the Design Gallery shows every shape, placeholder, banner, and the mutation gate.

### Shapes

| Shape | When | `StateView` shows | Actions |
| --- | --- | --- | --- |
| `.loading` | no data has loaded yet | `DelayedProgress`: a centered `ProgressView`, only after 300 ms | none |
| `.empty` with `.noData` | the data loaded and holds nothing | stale and partial banners if any, then `StatePlaceholder` (symbol in `StacklyColor.decorative`, title, message in `textSecondary`) | up to two from `EmptyContent` (mockup: Browse Catalog, New Instance…) |
| `.empty` with `.noResults(query:)` | the filter or search left nothing | banners if any, then `StatePlaceholder` with `magnifyingglass`, the title `No Results for “<query>”`, and the screen's suggestion (mockup: "Check the spelling or try another status.") | Clear Filters |
| `.failed` | the first load failed | `StatePlaceholder` with the critical tone symbol, the screen's title, the daemon's message, and the error code in monospace | Retry, Open Diagnostics |
| `.unsupported` | the capability is not supported | `StatePlaceholder` with `nosign`, the title (mockup: "Backups Aren’t Supported"), and the reason | none |
| `.loaded` | the data loaded | stale and partial banners if any, then your content | Retry on the stale banner, item actions on the partial banner |

`ViewState.resolve(unsupported:value:error:lastLoadedAt:isRefreshing:emptyContent:outdated:partial:)` picks the shape in a fixed order:

1. `unsupported` is set: `.unsupported`.
2. No `value` and an `error`: `.failed`.
3. No `value`: `.loading`.
4. `emptyContent(value)` returns content: `.empty`.
5. Otherwise: `.loaded`.

So a page never shows empty before a load succeeded, and never shows failed while it still has data. The notes (`ContentNotes`) of `.empty` and `.loaded` carry:

- `stale`: `StaleNote(since: lastLoadedAt, reason: .refreshFailed(error))` when `value` and `error` are both set (a refresh failed). It wins over outdated.
- Else `stale`: `StaleNote(since: outdated.since, reason: .outdated(source:))` when the daemon reports the data as out of date, such as the catalog.
- `partial` and `isRefreshing` as you pass them.

| Type | Fields | Built by |
| --- | --- | --- |
| `EmptyContent` | `kind` (`.noData` or `.noResults(query:)`), `symbol`, `title`, `message`, `primaryAction`, `secondaryAction` | `EmptyContent.noData(symbol:title:message:primaryAction:secondaryAction:)`, `EmptyContent.noResults(query:message:clearFilters:)` |
| `FailureContent` | `title` (the screen's, such as "Couldn’t Load Instances"), `message` and `code` (from the daemon's error envelope, already redacted) | the screen |
| `UnsupportedContent` | `title`, `reason` (from the provider's capabilities) | the screen |
| `StaleNote` | `since`, `reason` (`.refreshFailed(FailureContent)` or `.outdated(source:)`) | `resolve` |
| `PartialNote` | `summary`, `items` of `PartialItem(name:reason:action:)` | the screen, from the daemon's data |
| `StateAction` | `title` (a verb), `mutates`, `perform` | the screen |

`FailureContent`, `UnsupportedContent`, `StaleNote`, `EmptyKind`, and `MutationAvailability` are `nonisolated` and `Sendable`. The types that carry closures stay on the main actor.

### View model contract

Each screen's view model holds what `resolve` needs and follows these rules:

- Keep the last data that loaded (`value`) and `lastLoadedAt` through refreshes and through filter or search changes, and set them together.
- Apply filters on top of `value` and decide through `emptyContent`: no data when `value` is empty, no results when the filtered result is empty. A filter never empties `value`.
- When a refresh starts, set `isRefreshing`. On success, set the new `value` and `lastLoadedAt` and clear `error`. On failure, set `error` and keep `value`.
- Retry from `.failed` clears `error` and loads again, so the shape returns to `.loading`.
- Build every `ViewState` with `resolve`, never a case by hand.

### Placeholders

- `StatePlaceholder` has two sizes. `.page`: title `.title2.weight(.semibold)`, symbol `ScaledSize.placeholderPageSymbol` (44), 80 pt vertical padding. `.section`: title `.title3.weight(.semibold)`, symbol `ScaledSize.placeholderSectionSymbol` (34), 34 pt vertical padding, inside a card (`.cardSurface()`).
- Create one with `StatePlaceholder(_ empty:size:)`, `StatePlaceholder(_ failure:size:retry:openDiagnostics:)`, or `StatePlaceholder(_ unsupported:size:)`. `StateView` uses the page size.
- The message wraps at 380 pt. The main action is `.borderedProminent` and sits after the secondary `.bordered` one, as in the mockup. Neither is the default button.
- `InlineEmptyRow(text)` is a one sentence row inside a grouped list (mockup: "Nothing running."), in `textSecondary` with `Spacing.rowInsets`.

### Banners

`StateBanner(tone:title:message:code:items:actions:base:)`:

- It always shows the tone symbol (`Tone.symbol` in the tone fill, `ScaledSize.bannerSymbol`), then the title in `.body.weight(.semibold)`, the message in `.callout` `textSecondary`, an optional code block (`.stacklyCode()` on `surfaceConsole` with `Radius.small`), an optional item list, and up to three native `.bordered` buttons.
- The background is `.toneSurface(tone, on: base, strength: .banner)`. `base` is `.card` by default and `.window` for the shell's window banner. The padding is 12 vertical and 14 horizontal.
- Items: five show, then "and N more" with a Show All link button (24 × 24 pt hit target) that opens the rest in place. Each item shows its name in semibold, its reason in `textSecondary`, and an optional small `.bordered` action.
- Errors about one item (a failed instance, a port conflict, config drift) never use `ViewState`. Put a `StateBanner` at that item (mockup: the failure banner on Summary, drift on Configuration) and leave the rest of the page as it is.

Page banners from `StateView`:

| Note | Tone | Title | Message | Action |
| --- | --- | --- | --- | --- |
| stale, refresh failed | `caution` | `Showing the last known state from <time>.` (mockup) | `Couldn’t refresh: <daemon message>.` | Retry |
| stale, outdated | `caution` | `Showing the last known state from <time>.` | `<source> last updated <time>.` | Retry |
| partial | `caution` | the summary (mockup: "5 of 6 services ready") | the items that are not ready, each with its reason | per item |

- The time comes from `StaleNote.formattedSince(now:calendar:locale:timeZone:)`: the short time in the locale's style (14:02 in a 24 hour locale, 2:02 PM in en_US), plus the short date when it is not today in that time zone. The banner reads the calendar, locale, and time zone from the environment and updates every minute.
- While `isRefreshing`, Retry is disabled and shows a small `ProgressView` next to it.
- Content is never dimmed while stale (the mockup's 0.6 opacity fails AA).
- A daemon that cannot be reached is not a page stale note. The app shell (spec 0013) shows one `StateBanner(tone: .caution, …, base: .window)` for the whole window and sets `MutationAvailability`.

### Loading

- `DelayedProgress` shows a centered `ProgressView` only after 300 ms, so a fast load never flashes a spinner. A refresh never replaces content with a spinner.
- The delay starts when `DelayedProgress` appears. `StateView` inserts a new one each time the shape enters `.loading`, so the delay starts over each time.
- Its model, `DelayedVisibility`, takes an injectable `sleep: @Sendable (Duration) async throws -> Void` (default `Task.sleep(for:)`), so tests never wait real time.

### Mutation gate and unsupported actions

- `MutationAvailability` is `.available` or `.blocked(reason:since:)`. The app shell sets it with `.environment(\.mutationAvailability, …)`; the default is `.available`.
- `.mutationGated()` disables a control while mutations are blocked, with the reason as its help (tooltip and VoiceOver help). Apply it to every control that changes data through the daemon. A `StateAction` with `mutates: true` in a placeholder or banner is gated the same way. Copy, navigation, Retry, Open Diagnostics, and Browse Catalog are never gated. The visible reason is the shell's window banner.
- `.disabled(reason: String?)` disables a control whose capability is not supported, with the reason as its help. Always show the reason as text too: `DisabledReasonText(reason)` (`info.circle`, `.footnote`, `textSecondary`) next to the control or in the section footer, or an unsupported placeholder. Never hide an unsupported control. Draw capability buttons only after the screen's data, capabilities included, has loaded.
- Apply `.mutationGated()` and `.disabled(reason:)` to the control itself, not to a container. They add the help only while there is a reason, so the control keeps its own `.help` while it is enabled.

### Motion and VoiceOver

- Shape changes, banners appearing, and the delayed spinner crossfade with `StacklyMotion.crossfade`. Nothing moves or scales.
- A placeholder or banner is one VoiceOver group labeled by its title. Their symbols are decorative and hidden.
- `StateView` announces the failure title once when the shape turns `.failed`, and a critical `StateBanner` announces its title once when it appears.

## Confirmation

Every confirmation in Stackly goes through one modifier: `.stacklyConfirmation(item: $request)` with a `ConfirmationRequest?` binding. Set the binding to show the dialog. No screen calls `confirmationDialog`, `.alert`, or `NSAlert` itself; the source rule test fails on any of them outside `Packages/StacklyUI/Sources/StacklyUI/Confirmation/`.

```swift
@State private var request: ConfirmationRequest?

Button("Remove…") {
    request = ConfirmationRequest(
        title: "Remove “\(instance.id)”?",
        message: "The instance is unregistered and its process stopped. The data directory (\(instance.dataSize)) stays on disk.",
        actionTitle: "Remove",
        role: .destructive
    ) { remove(instance) }
}
.stacklyConfirmation(item: $request)
```

### When to confirm

Confirm an action that deletes or overwrites data, deletes a record or a runtime, kills a process without a graceful stop, or disturbs another owner. A feature that adds such an action applies the same rule and adds a row here.

| Action | Role | Feature |
| --- | --- | --- |
| Remove instance | `.destructive` | 54 |
| Purge instance data | `.destructive` | 54 |
| Remove runtime | `.destructive` | 55 |
| Restore that overwrites an instance | `.destructive` | 52 |
| Force stop | `.destructive` | 26 |
| Stop an instance leased by an environment, or a shared one | `.destructive` | 26, 47 |
| Stop All Services | `.destructive` | 32 |
| Bring down an environment (stops only instances nothing else needs; data is kept) | `.standard` | 47 |

No confirmation: start, stop, and restart without dependents, reload, copy, cancelling an operation, and cleaning up a cache that can be downloaded again.

### Roles

| Role | Action button | Return | Escape | Alert style |
| --- | --- | --- | --- | --- |
| `.destructive` | red (`hasDestructiveAction`), not the default | Cancel (the default button) | Cancel | `.critical` (caution badge) |
| `.standard` | the default button | the action | Cancel | `.informational` |

- Return never runs a destructive action. A keyboard user reaches it with Tab (when Keyboard navigation is on in System Settings › Keyboard) and presses it with Space. This is on purpose.
- The default button sits at the trailing edge; the alert adds it first.

### Copy

- **Title:** a question that names the object, in curly quotes when it is an identifier: "Remove “pg17-dev”?", "Stop “mysql-erp”?", "Remove PostgreSQL 16.10?". Never empty.
- **Message:** one or two sentences, first what happens, then what stays safe: "Data is kept.", "Instance data is never touched.". Name objects from the daemon's data, never a secret.
- **Action title:** the verb from the title ("Remove", "Stop", "Overwrite", "Bring Down"), never "OK" or "Yes". Cancel is always "Cancel".
- Mockup examples (also on the gallery's Confirmation page):

| Title | Message | Action | Role |
| --- | --- | --- | --- |
| Remove “pg17-dev”? | The instance is unregistered and its process stopped. The data directory (1.24 GB) stays on disk. | Remove | `.destructive` |
| Stop “mysql-erp”? | It’s leased by erp-development. Stopping it leaves the environment partially ready. Data is kept. | Stop | `.destructive` |
| Overwrite “pg17-dev”? | Replaces the current data with bk-0412. A safety snapshot is taken first and the instance restarts. | Overwrite | `.destructive` |
| Remove PostgreSQL 16.10? | Deletes the runtime binaries. No instance uses it. Instance data is never touched. | Remove | `.destructive` |
| Bring down “erp-development”? | Releases the lease and stops only instances nothing else needs. Shared and manually started instances keep running. Data is kept. | Bring Down | `.standard` |

- A test of a screen checks each request it builds with `request.copyProblems` (internal, through `@testable import StacklyUI`); it is empty when the copy follows these rules.

### Behavior

- The dialog is a native `NSAlert`: a sheet on the window of the view that has the modifier, or app modal (`runModal()`) when that view has no window, such as a menu bar popover (feature 32).
- One confirmation at a time. A new request replaces the shown one (the old one closes without running), and setting the binding to `nil` closes the dialog without running the action. When the view disappears, the dialog closes and the binding is reset.
- When the user answers, the modifier sets the binding to `nil`, then calls `perform` only for the action button. `perform` may set a new request.
- `perform` sends the action through the mutation path of spec 0013. If stacklyd is unreachable when the user confirms, nothing is sent and feature 67's feedback says "stacklyd is unavailable. Nothing was sent."
- The alert uses only system colors; custom tokens have no `NSColor` accessor.

### Why NSAlert, not `confirmationDialog`

Checked in build step 17 on macOS 27.0 (26A428) with Xcode 27.0 (27A266a), in the Debug Design Gallery with real Return and Escape key presses and the accessibility tree. SwiftUI's `confirmationDialog` is an `NSAlert` sheet underneath, and one button holds one key equivalent:

- Cancel with `keyboardShortcut(.defaultAction)`: Return cancels, but Escape does nothing (the alert has no cancel button left).
- Cancel without a shortcut, in either order, or with `.cancelAction`, or the action with another shortcut: Escape cancels, but Return does nothing (there is no default button).
- A request replaced while the dialog is open updates the buttons but keeps the old title, message, and severity.

So the modifier builds the `NSAlert` itself: for a destructive request, Cancel takes Return as its key equivalent and the modifier maps a plain Escape to Cancel with a local key monitor while the alert is open. Setting the window's `defaultButtonCell` instead does not work: it overwrites Cancel's Escape key equivalent with Return. A replaced request closes the old alert and shows a new one. With this path the check passed: destructive Return and Escape cancel without running the action, standard Return runs it and Escape cancels, and a replaced request shows its own title. `ConfirmationTests` pins the alert's buttons, order, styles, and key equivalents; `/check verify` repeats the key check and the Tab and Space check with Keyboard navigation on.

## Components

Shared views in `Packages/StacklyUI/Sources/StacklyUI/Components/` ([spec 0067](../../docs/specs/0067-komponen-dasar-stackly/index.md)). They sit on the tokens, status, state, and confirmation helpers above and add no glass, no new contrast pair, and no second version in a screen folder. A screen assembles its page from these parts. Every component has `#Preview`s in light, dark, and at 640 pt, built from `ComponentSamples`, and a section on the **Components** page of the Design Gallery.

### Tile

`ProviderTile(mark:tile:name:size:)` or `ProviderTile(_:size:)` with a `TileSpec` draws a rounded square (continuous corners, radius `size × 0.27`) in a system color with a bold white mark. It is decoration: hidden from VoiceOver, never counted in the contrast table, and always next to the provider name in text. The fill comes from the catalog `TileColor`; a missing or unrecognized color is gray.

| `TileColor` | Fill |
| --- | --- |
| `blue`, `cyan`, `teal`, `green`, `yellow`, `orange`, `red`, `brown`, `indigo` | `systemBlue`, `systemCyan`, `systemTeal`, `systemGreen`, `systemYellow`, `systemOrange`, `systemRed`, `systemBrown`, `systemIndigo` |
| `gray`, `unrecognized`, none | `systemGray` |

The mark is the catalog mark. When there is none, `ProviderMark.defaultMark(for:)` takes the first two ASCII letters or digits of the name, the first in upper case and the second in lower case ("PostgreSQL" gives "Po", "9front" gives "9f"), or "?" when there are none. The font is `.stacklyTileMark(tileSize:)`, bold at `size × 0.37`, one line, shrinking to half before it is cut; it is the only fixed font size besides the symbol helper. Sizes: `smallSize` 18 (Instances table), `rowSize` 26 (Overview rows, Installing rows), `mediumSize` 40 (Catalog cards, sheet header, inspector header), `largeSize` 52 (Catalog detail panel). Other mockup sizes (20, 22, 28) pass `size` directly.

### Key value

`CardSection` draws an optional `.headline` title with an accessory on its right, then one `.cardSurface()` card in which every subview of the content is a row, then an optional footer (`.footnote` `textSecondary`; a `String` or any view, such as `DisabledReasonText`). Rows have 11 pt above and below, 14 pt at the sides, at least `ScaledSize.rowMinHeight` high, and a divider inset 14 pt from the left. A `ForEach` makes one row per element; a row of several views is wrapped in an `HStack` or `VStack` by the screen.

`.labeledContentStyle(.stackly)` draws the label in `.body` primary text and the value in `.body` `textSecondary`, selectable. When both fit, they are one line with the value flush right; otherwise the label sits above and the value wraps below it, flush left, never cut. A screen may give the value its own font (`.callout.monospaced()` for an endpoint, a path, or a digest); the color stays `textSecondary`. VoiceOver reads the row as one element, "{label}, {value}": selectable text makes `.combine` read the label only (seen in the accessibility tree of the running Debug app), so the style builds the spoken element with `.accessibilityRepresentation` from the same label and value. A row's own button, such as `CopyButton`, stands next to the `LabeledContent` in an `HStack`, never inside it.

Put the style on a `CardSection` whose rows are all read only, on an inspector, or on one read only row inside a `Form`. **Never on a `Form` itself.** Checked on macOS 27.0.1 by rendering a grouped `Form` with a `TextField`, a `Picker`, a `Toggle`, and a read only row three ways: the style on the read only row leaves the field, picker, and toggle exactly as they are without it; the style on the `Form` turns the field's value gray and regular instead of primary and bold and shifts the row heights.

### Copy and console

`CopyButton(_:what:style:announcement:key:)` copies plain text to the general pasteboard **only when pressed**, through `PlainPasteboard.write(_:)` (`clearContents()` then a plain string; `NSPasteboard` appears only there and in `ConcealedPasteboard`, a source rule guards it). For two seconds afterwards it shows "Copied" with a `checkmark` (the icon style: only the checkmark) and then goes back to "Copy" or `doc.on.doc`; the width is reserved for the longer label, so the row does not move. Every press writes again and starts the two seconds over. VoiceOver reads "Copy {what}" and, after the write, says the announcement once ("{What} copied." unless the screen gives its own copy, such as "Address copied." or "Command copied."). The text style is a small bordered button; the icon style is borderless with a 24 × 24 pt click area. The checkmark carries the tone fill `positive`; the resting icon is `textSecondary`. A copy button is never gated by `MutationAvailability`, it never copies a secret (a secret copy is always `RevealableSecretText` and `ConcealedPasteboard`), and a screen's own key goes into the identifier `copy.button.<key>`.

`ConsoleBlock` draws `.callout.monospaced()` primary text on `.consoleSurface()`: `surfaceConsole`, `Radius.medium`, a hairline `separatorColor` border (1 pt under Increase Contrast), and 12 pt above and below and 14 pt at the sides. Text is selectable and wraps, with no horizontal scroll. `ConsoleBlock(command:)` puts "$ " in `textSecondary` before the command and a Copy button on the right that copies the command without the "$ ". `ConsoleBlock(text:accessibilityLabel:copy:)` shows several lines, with an optional Copy button at the top right. VoiceOver reads the text as one element, the screen's label with the text as its value (the command without "$ "), and the Copy button as its own element. A failure snippet inside a `StateBanner` keeps the banner's own code block.

### Actions

`StacklyAction` is the one description of an action that shows as a button, a menu item, or a command (feature 69): `id`, `title`, `symbol`, `role`, `mutates`, `disabledReason`, `isBusy`, `perform`. A screen builds it once.

- **Availability**, in this order: the action's own `disabledReason` (visible: `.disabled(reason:showsReason: true)`), then the mutation gate for a `mutates` action while the daemon cannot be reached (`.disabled(reason:showsReason: false)`, because the window banner writes it), then `isBusy` (`.busy`), else `.enabled`. `invoke(gate:present:)` does nothing unless the action is `.enabled`.
- **Role and confirmation.** `ActionRole.standard(confirmation:)` has a confirmation only when the action interrupts work; `ActionRole.destructive` always carries an `ActionConfirmation`, so a destructive action without a dialog cannot be written. The copy follows the Confirmation rules above, and the title of an action that confirms ends with "…" (an `assert` guards it). `invoke` builds the `ConfirmationRequest` and hands it to the host.
- **One host.** `.actionHost()` holds one confirmation per window or sheet, reads `mutationAvailability` where it sits, and fills the `actionHost` environment value. `AppShell` sets it at its root inside `.environment(\.mutationAvailability, …)`, every `StacklySheet` sets its own, and the menu bar popover (feature 32) will too. A button, a menu item, and a command run through `ActionHost.run(_:)`. Without a host an action with no confirmation still runs; one that confirms asserts and runs nothing.
- **`ActionButton`.** Primary is `.borderedProminent`, secondary is `.bordered`; a destructive action is `role: .destructive` and `.bordered`, never prominent and never the default. A disabled action keeps its button, with the reason as help and, with `showsReason`, as `DisabledReasonText` right under it; a toolbar button passes `showsReason: false` and the page writes the same `DisabledReasonText` in its content, a section footer, or the item's banner. Gate-blocked: disabled, gate reason as help only. Busy: disabled, the title stays, a small spinner stands next to it, and VoiceOver reads the value "In progress". A capability the provider lacks is never hidden. No keyboard shortcut is set here; feature 69 owns them. The identifier is `action.button.<id>`.

### Menus

Actions in a menu come from `StacklyAction` through `ActionMenu` (the More menu in a toolbar) and `ActionMenuItems` (a `.contextMenu`). The reason an item is disabled never lives only in a tooltip, so it is written in the menu.

macOS 27.0.1 was checked with a probe app on 2026-10-21 (spec 0067 rationale, *Temuan langkah 1*): a disabled `Button` with two `Text` children becomes an `NSMenuItem` with a subtitle in a toolbar `Menu`, a `.contextMenu`, and a main menu `CommandMenu`, and the subtitle is drawn under the title in all three. But the accessibility tree of the item holds only its title: there is no description, help, or value, and `.accessibilityHint`, `.accessibilityLabel`, `.accessibilityValue`, and `.help` do not change it. So a spoken reason cannot be proven, and VoiceOver itself was not tried (a manual step, `NOT_RUN`).

The path in use is therefore the spec's fallback: **a disabled extra item with the reason as its title, right under the item.** The reason is a menu item of its own, so it is both visible and spoken. The switch is one constant, `MenuReasonPath.current` in `Components/Actions/`: if VoiceOver reads the subtitle, change it to `.subtitle`, and the menus get shorter without any API change. Spec 0069 uses the same path for the main menu.

### Feedback

Feedback is the result of something the person did, shown as an opaque card under the page. A `Feedback` has a tone, a message, an optional detail and code, and at most one action. The window's `FeedbackCenter` (created by the root view of the window, `LiveShell` today, in the `feedbackCenter` environment value) holds the one that is on screen; `.feedbackPresenter()` on the detail column draws it in a bottom safe area inset, so content scrolls above it and a focused control is never covered. Without a center (a preview) nothing is drawn.

- **Card.** Centered, 16 pt from the bottom, at most 520 pt wide with 22 pt at the sides: the tone symbol (tone fill), the message in `.body`, the detail in `.callout` `textSecondary`, the code in `.callout.monospaced()` `textSecondary` (selectable), at most one small `ActionButton`, and Dismiss (`xmark`, label "Dismiss", at least 24 × 24 pt). The surface is `toneSurface(tone, on: .card, strength: .banner)` with `Radius.large` corners and a soft shadow. It appears and goes with the crossfade only. It never takes focus when it appears; its buttons are reachable with Tab.
- **Announcement.** `FeedbackCenter.show` announces the message and then the detail, once, when the feedback is sent (not the code). Nothing else announces the same result, so a screen does not call `announce` for it.
- **How long.** `positive`, `informative`, and `neutral` feedback without an action goes away after `visibleDuration`: `min(10, max(4, ceil(characters / 15)))` seconds, counting the message and the detail. The wait runs only while the card is visible, the window is active, the pointer is not over the card, and keyboard focus is not in it, and starts from the beginning each time that is true again (`FeedbackHold`). `caution`, `critical`, and any feedback with an action stay until Dismiss or until something replaces them. New feedback replaces the one on screen, except that transient feedback never replaces persistent feedback: it is only announced.
- **From a mutation.** `Feedback.outcome(_:success:showOperations:)` maps a `MutationOutcome`: `completed` gives the screen's success feedback (or none); `rejected` gives `critical` with the daemon's message and hint as sentences (`DaemonText.sentence`) and the error code; `notSent` gives `caution` "stacklyd is unavailable. Nothing was sent."; `outcomeUnknown` gives `caution` with the lost connection copy and, when the screen can open Operations, the "Show Operations" action. `Feedback.failure(_:)` gives the same `critical` feedback for the `ErrorBody` of a failed operation.
- **When to use it.** Only for the result of an action the person took, never for a change that happens by itself. Good news whose result also shows on the screen may be skipped (a Start that is accepted is enough with its status and announcement). A failure is always also visible where it happened (Needs Attention, the item's banner, Operations), so the card is never the only record. A view inside a sheet never sends to the center: the sheet shows its result with a `StateBanner`, and the screen that owns the sheet sends feedback after the sheet closes.

### Sheet

Checked on macOS 27.0.1 with a probe sheet and real key events (spec 0067 rationale, *Temuan langkah 9*): a footer drawn inside the sheet works, so the footer stays explicit and does not move to toolbar placements.

- **Return** runs the primary action with focus in a `TextField` and in a `Toggle`; a destructive primary action has no shortcut and a disabled one does not run.
- **Escape** closes a sheet on its own and does not run the Cancel action, even with `.keyboardShortcut(.cancelAction)`. With `.interactiveDismissDisabled()` Escape runs the Cancel action once and the sheet stays open unless that action closes it. A sheet therefore always sets it, because Cancel may mean something other than closing.
- A confirmation from inside a sheet shows as a second sheet on top of it, and Escape there cancels only the confirmation.
- `.presentationBackground` draws the sheet opaque, with nothing of the window behind it showing through.
- Not checked (manual): Tab and Space with Keyboard navigation on, and VoiceOver.

`StacklySheet(title:subtitle:tile:primary:cancel:minWidth:content:)` draws the opaque `surfaceWindow` background, a header, a body, and a fixed footer.

- **Header.** An optional `ProviderTile` at `mediumSize` (40 pt), the title in `.title3.weight(.semibold)`, and the subtitle in `.callout` `textSecondary`. Padding: 20 above, 22 at the sides, 6 below.
- **Body.** A `ScrollView` with 12 pt above, 22 pt at the sides, 16 pt below, and 16 pt between children. Its height is the height of its content, at most the visible screen height minus 160 pt, so a short sheet does not scroll and a long one scrolls without cutting text. A screen that puts a `Form` in the body sets `.scrollContentBackground(.hidden)` so the sheet background shows and the rows keep their native background.
- **Footer.** Outside the scroll area: the reason on the left, then Cancel, then the primary action (12 pt above, 22 at the sides, 18 below). The reason is the `DisabledReasonText` of the primary action when it is disabled with a reason, or the gate's reason when the primary action `mutates` and the gate is `.blocked`, because the sheet covers the window banner. When the primary action is busy the footer shows a small spinner next to it and VoiceOver reads the value "In progress".
- **Buttons.** The primary action is `.borderedProminent` with `.keyboardShortcut(.defaultAction)`; a destructive primary action is `.bordered`, `role: .destructive`, with no shortcut, so Return does nothing there. A disabled or busy primary action does not run on Return. Cancel is `.bordered` with `.keyboardShortcut(.cancelAction)` and stays enabled while the primary action is busy. These are the only two shortcuts the components set. With no `cancel` given, "Cancel" closes the sheet; a screen gives its own `cancel` when it means something else, such as cancelling an operation behind a confirmation. Identifiers: `sheet.cancel` and `sheet.primary`.
- **Host.** The sheet sets its own `.actionHost()`, so a confirmation raised from inside shows on top of this sheet. A view inside a sheet never sends to the window's `FeedbackCenter`: it shows its result with a `StateBanner`, and the screen that owns the sheet sends feedback after the sheet closed.

### Checklist

`CheckList(_:layout:)` shows `CheckItem`s: a symbol, the title in `.callout` primary text, the detail in `.subheadline` `textSecondary` wrapped in full (never an ellipsis), and an optional `ActionButton` under the text. A screen wraps it in a `CardSection` (for example "Preflight") and maps its own result (a preflight check, a doctor result) to a `CheckStatus`. This table replaces the preflight status table of spec 0020.

| `CheckStatus` | VoiceOver word | Tone | Symbol |
| --- | --- | --- | --- |
| `passed` | Passed | `positive` | `checkmark.circle.fill` |
| `warning` | Warning | `caution` | `exclamationmark.triangle.fill` |
| `failed` | Failed | `critical` | `xmark.circle.fill` |
| `pending` | Checking | `neutral` | a small `ProgressView` after 300 ms, an empty box before that |
| `unrecognized` | Unrecognized status | `caution` | `questionmark.diamond.fill` |

- The symbol uses the tone's fill (never `StacklyColor.decorative`), and its shape differs per status, so Differentiate Without Color changes nothing. A status change crossfades.
- **Layout.** `.adaptive` is a grid with columns at least 200 pt wide and 16 pt apart: two columns in a 560 pt sheet (488 pt inside the card), one in a narrow width. `.single` is one column. `pending` shows its spinner only after 300 ms (`DelayedVisibility`), so a fast check does not flash.
- **VoiceOver.** One element per row, "{title}, {status}: {detail}" (without the detail when there is none), and the row's action is its own element. The row identifier is `checklist.row.<id>`. Announcing a change of status or a summary is the screen's job.

### Staged progress

`StagedProgressValue.from(operation:stages:)` turns an `OperationSummary` and the `StageDefinition`s of the screen that owns the operation (five for an install, six for create) into the state of every stage, a `fraction` for the bar, a whole `percent`, a `currentTitle`, and an `outcome`. It is a pure function. `i` is the index of `operation.stage` in the stages (from 0) and `n` the number of stages.

| `operation.state` and `stage` | Stages | `runningIndex` | `fraction`, `percent` | `currentTitle` | `outcome` |
| --- | --- | --- | --- | --- | --- |
| `queued` | all `pending` | none | none | Queued | `waiting` |
| `running`, known stage | before `i` `done`, `i` `running`, after `pending` | `i` | the formula below | title of stage `i` | `running` |
| `cancelRequested`, known stage | as `running` | `i` | the formula below | Cancelling… | `cancelling` |
| `running` or `cancelRequested`, no stage | first `running`, rest `pending` | 0 | 0, 0 | title of the first stage (or Cancelling…) | `running` (or `cancelling`) |
| `running` or `cancelRequested`, unknown stage or no stages | all `pending` | none | none | Working… (or Cancelling…) | `running` (or `cancelling`) |
| `succeeded` | all `done` | none | 1, 100 | Done | `succeeded` |
| `failed`, known stage | before `i` `done`, `i` `failed`, after `pending` | `i` | none | Failed at {title of stage `i`} | `failed` |
| `failed`, no or unknown stage, or no stages | all `pending` | none | none | Failed | `failed` |
| `needsRecovery` | as the two `failed` rows | as `failed` | none | Needs Recovery | `needsRecovery` |
| `cancelled`, known stage | before `i` `done`, `i` `cancelled`, after `pending` | `i` | none | Cancelled | `cancelled` |
| `cancelled`, no or unknown stage, or no stages (also cancelled while queued) | all `pending` | none | none | Cancelled | `cancelled` |

The formula: with `counted(done, total)` and `total` above 0, `percent = (i × total + min(done, total)) × 100 / (n × total)`; with `indeterminate` or a `total` of 0, `percent = i × 100 / n`. It is integer division (rounded down) and at most 99 until the operation `succeeded`, so "100%" never shows for work that is not finished. `fraction` is the same numerator over the same denominator as a `Double`. The arithmetic is 128 bit, so byte counts never overflow.

`StagedProgress(_:layout:accessibilityLabel:announcesStages:)` draws it. The bar is a native linear `ProgressView`, shown only while the outcome is `running` or `cancelling`, indeterminate when `fraction` is none. Under Reduce Motion a change of the value is not animated (`StacklyMotion.progress`).

| `outcome` | `.compact` text | Bar | VoiceOver value |
| --- | --- | --- | --- |
| `waiting` | Queued | none | Queued |
| `running` with a percent | {title} · {percent}% | determinate | Stage {k} of {n}, {title}, {percent} percent |
| `running` without a percent | Working… | indeterminate | Working |
| `cancelling` | Cancelling… · {percent}% or Cancelling… | as `running` | Cancelling, stage {k} of {n}, {percent} percent, or Cancelling |
| `succeeded` | `checkmark.circle.fill` (`positive` fill) and Done | none | Done |
| `failed` | `xmark.circle.fill` (`critical` fill) and the title | none | the title |
| `needsRecovery` | `xmark.circle.fill` (`critical` fill) and Needs Recovery | none | Needs Recovery |
| `cancelled` | `minus.circle.fill` (`neutral` fill) and Cancelled | none | Cancelled |

`k` is `runningIndex + 1` (stages count from 1). The compact text is `.callout` `textSecondary` with monospaced digits.

The `.stages` layout draws the same bar, then every stage in a wrapping flow (16 pt across, 6 pt down) with a symbol and its title in `.callout`:

| `StageState` | Symbol | Symbol color | Text |
| --- | --- | --- | --- |
| `done` | `checkmark.circle.fill` | `positive` fill | primary |
| `running` | `circle.inset.filled` | `informative` fill | primary, semibold |
| `pending` | `circle` | `textSecondary` | `textSecondary` |
| `failed` | `xmark.circle.fill` | `critical` fill | `critical` text, semibold |
| `cancelled` | `minus.circle.fill` | `neutral` fill | `textSecondary` |

The whole view is one VoiceOver element: the label is `accessibilityLabel` ("Progress" by default) and the value comes from the table above. With `announcesStages`, each time the running stage changes (not when the view first appears), VoiceOver says "{stage title}." once. The identifier is `progress.view`.

### Inspector

`.stacklyInspector(isPresented:help:content:)` goes on the page content inside the detail column of the split view, the content that fills the column (not on a view inside a `ScrollView`: the container of `.inspector` has no height of its own there, and its content is drawn over the neighbouring views instead of in the scroll flow, checked in the design gallery). It is the only place in the app that calls `.inspector(`, and it puts an `InspectorToggle` in the toolbar (`.primaryAction`).

- **Column.** The native column is 240 pt at the least, 280 pt by default, and 360 pt at the most in a window 1000 pt wide or more, 280 pt at the most in a narrower one (`.inspectorColumnWidth`, spec 0022; `StacklyInspectorWidth.maxWidth(windowWidth:)`, read from `EnvironmentValues.stacklyWindowWidth`, which the window's root view fills with `onGeometryChange`; `nil`, as in the gallery, previews, and sheets, means 360 pt). Its content is a `ScrollView` with 16 pt of padding and 16 pt between views, on `StacklyColor.surfaceWindow`, opaque, so text on it stays in the P1 and P2 pairs of spec 0012. The column is never glass. The content is at least as tall as the column, so an `InspectorPlaceholder` (which fills the height) sits in the middle.
- **Content.** Build it from `InspectorHeader` (a 40 pt tile, `.title2.weight(.semibold)` title, subtitle, `StatusLabel` in the `.standard` context, and an optional row of `ActionButton`s that stack when they do not fit side by side), `CardSection` with `.labeledContentStyle(.stackly)`, `StateBanner`, and `InspectorPlaceholder` ("No Selection" by default) when nothing is selected. Texts wrap and are never cut, so a screen never sets a `lineLimit` in the inspector.
- **Toggle.** The label is "Show Inspector" or "Hide Inspector" for the binding, the symbol is `sidebar.trailing`, the help is `help ?? label` (a page passes the text with the shortcut from `CommandMap.helpText(for:)` of spec 0069), and the identifier is `inspector.toggle`. The toolbar gives it a 44 × 52 pt target.
- **State.** Every page that has an inspector keeps its binding in `@SceneStorage("<page>.inspector") private var showsInspector = true`, with `<page>` the lowercase page name (`instances.inspector`, `runtimes.inspector`). It is open by default, restored per window by the system, and never shared between pages. Spec 0069 binds View > Show/Hide Inspector to the same binding.

Checked on macOS 27.0.1 on 2026-10-21 (spec 0067 rationale, *Temuan langkah 14*) with a probe window of a `NavigationSplitView` (sidebar 200 to 260 pt, 236 by default, and the page column at least 640 pt) and `.stacklyInspector` on the page, in light and dark, through the accessibility API and window captures:

- **Opaque background joins.** `surfaceWindow` on the `ScrollView` fills the whole column: up under the toolbar to the top edge of the window, down to the window's rounded corner, and to the divider. No glass shows through and there is no seam inside the column. The toolbar band above the page column is the system's own, so it is not the same tone as the page; this is the same as before there was an inspector.
- **Column widths hold.** Setting the inspector divider to extreme values with the accessibility API clamps the column at 240 and 360 pt. At 900 pt it opens at 280 pt.
- **900 pt window, sidebar and inspector open.** The sidebar takes 236 pt, the inspector 280, and the page content **384 pt**, not 640: `navigationSplitViewColumnWidth(min: 640)` is met by the whole detail column, which holds the inspector too (664 pt). So a page that can show an inspector has to work at 384 pt wide: tables and cards wrap and scroll, and no text is cut. In the inspector nothing is cut at 240 pt: the header title and subtitle wrap, two actions stack, and a long path in a card row drops under its label and wraps. The open question for spec 0013 (the content minimum with an inspector) is in the rationale.
- **Page column 384 pt (decided 2026-10-21).** At a 900 pt window with the sidebar and the inspector open, the page column is 384 pt (424 pt with the inspector at 240 pt), and that is accepted: every page that has an inspector works at 384 pt wide (tables and cards wrap or scroll, no text is cut), and the 640 pt content minimum of spec 0013 holds only while the inspector is closed. The cap above is what keeps 384 pt the floor: below a 1000 pt window the inspector cannot be dragged wider than 280 pt. Checked on macOS 27.0.1 on 2026-10-21 with a probe app (`NavigationSplitView`, the same `onGeometryChange` and environment value, an inspector with `ideal: 340`): the window at 900, 999, 1000, and 1300 pt opens the inspector at 280, 280, 340, and 340 pt, and the page column at 900 pt is 384 pt. Not driven from the session: shrinking a live window from 1000 pt or more to 900 pt with the inspector dragged to 360 pt (synthetic drags did not move the divider, and the window cannot be resized through the accessibility API from here), so that step is left to `/check verify`.
- **Toggle in the accessibility tree.** `AXButton` with `AXDescription` and `AXHelp` "Hide Inspector", `AXIdentifier` `inspector.toggle`; after pressing it, "Show Inspector". The header is an `AXHeading`, its status reads "Status: Running", and the actions are separate buttons. A card row reads "Software, PostgreSQL 17.4".

### Log

`LogView(_:follow:accessibilityLabel:timeTemplate:sourceTemplate:)` shows `LogLine`s in a lazy list on the console surface. The screen sets its height; feature 27 owns the data, the filters, Load Earlier, and the cap on lines.

- **Row.** The time and source columns are `.callout.monospaced()` in `textSecondary`, then the level symbol, then the text in `.callout.monospaced()`. Each column is at least as wide as its hidden template (`timeTemplate`, default `00:00:00.000`; `sourceTemplate`, default `stderr`), so rows line up; a longer source shows in full and pushes the text of its own line. A column is drawn only when at least one line has that part, so a log without sources does not waste the space. Text wraps and is selectable per line (a SwiftUI limit); copying several lines is an action of feature 27.
- **Levels.**

| `LogLevel` | Symbol | Text | VoiceOver word |
| --- | --- | --- | --- |
| `normal` | none, an empty box as wide as a symbol | `.primary` | none |
| `warning` | `exclamationmark.triangle.fill` in the `caution` fill | `Tone.caution.text` | Warning |
| `error` | `xmark.circle.fill` in the `critical` fill | `Tone.critical.text` | Error |

- **Surface.** `.consoleSurface()` is the background of the scroll view, clipped to its shape, so the lines run edge to edge with 14 pt at the sides and 2 pt above and below each line; the 12 pt above the first line and below the last one are scroll content margins, so scrolling to the end shows them. Tone text on `surfaceConsole` is pair P2 of spec 0012, so there is no new pair.
- **Follow.** `follow` is the screen's binding. A new last line scrolls to the end without animation while `follow` is on; turning `follow` on scrolls to the end. A scroll by the user that ends (scroll phase from interacting or decelerating to idle) more than 24 pt above the bottom turns `follow` off. A scroll by the program never does, and scrolling back to the bottom does not turn it on again: the toggle of feature 27 does.
- **VoiceOver.** The list is one element labeled `accessibilityLabel` (identifier `log.list`), and each line is one element, "{time}, {source}, {Error | Warning}: {text}", with an empty part and the normal level left out ("12:04:31.118, stdout: ready", "Error: text" when only the level is known).

### Diff

`DiffView(_:emptyText:)` shows `DiffLine`s in a `.cardSurface()` (an ordinary card, not the console surface; the mockup used the console). Feature 29 owns where the lines come from.

| `DiffKind` | Sign column | Text | Row background | VoiceOver |
| --- | --- | --- | --- | --- |
| `added` | “+” in `Tone.positive.text` | `.primary` | `toneSurface(.positive, on: .card, strength: .banner)` | “Added: {text}” |
| `removed` | “−” (U+2212) in `Tone.critical.text` | `.primary` | `toneSurface(.critical, on: .card, strength: .banner)` | “Removed: {text}” |
| `unchanged` | empty, same width | `textSecondary` | none | “Unchanged: {text}” |

- The sign column is always there and one monospace character wide, so a change is never only a color and the text lines up. Rows are `.callout.monospaced()`, wrap, and are selectable; the card has 8 pt padding and rows sit 2 pt apart with 3 pt above and below the text. The tint corners are `Radius.small`.
- Tone text on its own tint is pair P3 and primary text on a tint is pair P4 of spec 0012, so there is no new pair.
- Without lines the view shows `emptyText` in `.callout` `textSecondary`; the copy comes from the screen. The container carries the identifier `diff.list`.

## Motion

- `StacklyMotion.crossfade` is the only transition in Stackly's own views: `.opacity` with `StacklyMotion.crossfadeAnimation` (ease in out, 0.2 s). Use it for status changes, banners appearing, and changes of `ViewState` shape, in every setting, including Reduce Motion.
- Never animate `.move`, `.slide`, `.scale`, or `.offset`.
- `StacklyMotion.progress(reduceMotion:)` returns the animation for a change of a progress value, or `nil` while `accessibilityReduceMotion` is on, so the value jumps. A spinner keeps spinning.
- `announce(_:)` posts one VoiceOver announcement (`AccessibilityNotification.Announcement`). State views call it once when a page turns failed and when a critical banner appears.

## Commands and shortcuts

The menu bar, `CommandMap`, and the keyboard shortcuts of spec 0069. `CommandMap` (`StacklyUI/Commands/CommandMap.swift`) is the only place a shortcut is written down, and `SystemShortcuts` (`Commands/SystemShortcuts.swift`) holds the macOS shortcuts a row may not use; the unit tests refuse a clash. The tables below are the map, the macOS shortcuts a row may not use, and the rules for adding a command. The probe results come after them.

### Menu structure

Where Stackly puts its items. The order of the system items (About, Services, Hide, Undo to Select All, AutoFill, Dictation, Emoji and Symbols, Full Screen, Minimize, Zoom, Move & Resize, Bring All to Front, the window list, the Help search field) belongs to macOS and SwiftUI; the Probe results below record it.

| Menu | Stackly items |
| --- | --- |
| Stackly | Reconnect to stacklyd, after Settings… ⌘, and before Services |
| File | New Instance… ⌘N (planned, spec 0020), then the system Close ⌘W. `CommandGroup(replacing: .newItem)` removes New Window |
| Edit | the system items, then Find ▸ Find… ⌘F. `CommandGroup(replacing: .textEditing)` removes the Spelling and Grammar, Substitutions, Transformations, and Speech submenus |
| View | the sidebar item ⌃⌘S from `SidebarCommands()`, then Overview ⌘1 to Diagnostics ⌘7; Back, Show or Hide Inspector, and Sort Instances By follow with the Instances page |
| Instance | Open, Start, Stop, Restart, Reload, Copy Connection String, Duplicate…, Remove… (planned; `CommandMenu("Instance")` appears between View and Window with the first built row) |
| Window | the system items, then Stackly ⌘0 (the shortcut sits on the main `Window` scene) and, in Debug, Design Gallery |
| Help | the system search field; Welcome to Stackly (planned, spec 0016) and Stackly Help ⌘? (planned, feature 66). `CommandGroup(replacing: .help)` removes the item that opens nothing |

An item is never hidden because of state: one that does not apply is disabled. A disabled item with a reason of its own shows the reason as a disabled item under it (`MenuReasonPath.current`, `Components/Actions/ActionMenu.swift`); the mutation gate gives no reason because the window banner writes it. Only `planned` rows are left out.

### Command map

`CommandMap` holds one row per `CommandID`; the table lists them in declaration order. Group is the divider group inside a menu (`CommandMap.layout` draws a divider only between built groups). Status `built` means a `CommandItem` renders it; `planned` rows reserve the shortcut and render nothing until the owner feature builds them.

| Command | Menu | Grp | Title | Shortcut | Scope | Owner | Status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `reconnect` | Stackly | 1 | Reconnect to stacklyd | | shell | 69 | built |
| `newInstance` | File | 1 | New Instance… | ⌘N | shell | 20 | planned |
| `find` | Edit ▸ Find | 1 | Find… | ⌘F | page (all) | 69 | built |
| `showOverview` to `showDiagnostics` | View | 1 | Overview, Instances, Environments, Catalog, Runtimes, Operations, Diagnostics | ⌘1 to ⌘7 | shell | 69 | built |
| `back` | View | 2 | Back | ⌘[ | page (Instances) | 21 | planned |
| `toggleInspector` | View | 3 | Show Inspector or Hide Inspector | ⌥⌘I | page (Instances) | 22 | planned |
| `sortInstancesBy…` and `sortInstancesAscending`, `sortInstancesDescending` | View ▸ Sort Instances By | 1 and 2 | Name, Provider, Version, Endpoints, Status, then Ascending, Descending | | page (Instances) | 22 | planned |
| `openInstance` | Instance | 1 | Open | ⌘O | page (Instances) | 21 | planned |
| `startInstance`, `stopInstance` | Instance | 2 | Start, Stop | ⌘R, ⇧⌘R | page (Instances) | 22 | planned |
| `restartInstance`, `reloadInstance` | Instance | 2 | Restart, Reload | ⌥⌘R, none | page (Instances) | 26 | planned |
| `copyConnectionString` | Instance | 3 | Copy Connection String | ⇧⌘C | page (Instances) | 21 | planned |
| `duplicateInstance` | Instance | 4 | Duplicate… | ⌘D | page (Instances) | 24 | planned |
| `removeInstance` | Instance | 4 | Remove… | none | page (Instances) | 54 | planned |
| `stacklyHelp` | Help | 1 | Stackly Help | ⌘? | app | 66 | planned |
| `welcome` | Help | 2 | Welcome to Stackly | | app | 16 | planned |
| `showMainWindow` | Window | | Stackly (system item of the `Window` scene) | ⌘0 | app | 69 | built |

Three shortcuts reuse a macOS shortcut with another meaning, each with a written reason in the map: ⌘R (Show Ruler, no Format menu), ⇧⌘C (Show Colors, no Format menu), and ⌘0 (Actual Size, no zoom).

### Shortcuts macOS owns

`SystemShortcuts` holds these as data, and the unit tests refuse a clash. Table A may never be used. Table B may be used for the same meaning only, or with a written reason. Table C may be used only with a written reason. Rules D and E cannot be waived.

| Table | Shortcuts | Meaning |
| --- | --- | --- |
| A, blocked | ⌘Q ⌘H ⌥⌘H ⌘, ⌥⌘Q, ⌘. , ⌘W ⌥⌘W ⌘M ⌥⌘M, ⌘\` ⇧⌘\` ⌘Tab ⇧⌘Tab, Spotlight and input source keys, screenshots ⇧⌘3 to ⇧⌘6, ⌥⌘Esc ⌃⌘Q ⇧⌘Q ⌥⇧⌘Q, ⌥⌘D ⌃⌘D ⌃⌘Space, ⌃⌘F ⌃⌘S ⌥⌘T, ⌘Z ⇧⌘Z ⌘X ⌘C ⌘V ⌥⇧⌘V ⌘A, ⌘; ⌘:, ⌃F1 to ⌃F8 and ⌃ arrows | quit, hide, settings, cancel, windows, system, edit, full screen, sidebar, toolbar, focus and Spaces |
| B, standard meaning | ⌘N ⌘O ⌘D ⌘F ⌘G ⇧⌘G ⌘E ⌘J ⌘[ ⌘] ⌘? ⌘S ⇧⌘S ⌘P ⇧⌘P ⌘+ ⌘- ⌘0 | new, open, duplicate, find, find next and previous, use selection, jump, back, forward, help, save, print, zoom, actual size |
| C, formatting | ⌘B ⌘I ⌘U ⌘T ⇧⌘C ⌘R ⌥⌘C ⌥⌘V ⌘{ ⌘} ⌘\| | Format menu items |
| D, text editing keys | arrows and Delete with ⌘ or ⌥, ⌃ with a letter and no ⌘, Return, Escape, Tab, Space | a menu key equivalent runs before the text field sees the key |
| E, 🌐 | any shortcut with the globe key | `CommandModifiers` has no such modifier |

### Adding a command

1. The main action stays a button in the window (toolbar, inspector, row) built on `StacklyAction`. The menu command is the second way to it, never the only one.
2. Place it: new or imported things in File; navigation, view, and order in View; actions on the selected instance in Instance; app level actions (daemon, update) in Stackly; documentation in Help. Another object (environment, runtime, operation) gets no menu of its own until it has at least three actions on a common selection.
3. Give it a shortcut only if people use it often and it never destroys data (not Remove, Purge, Force Stop, Stop All Services, Overwrite). Prefer ⌘ with a letter that means the thing, then ⇧⌘ for the opposite, then ⌥⌘ for a variant. Table A, rule D, and rule E never; table B only with the same meaning; table C only with a reason written in the map.
4. Register it: add or change the row in `CommandMap` (set `status: .built`), add one `CommandItem(.id, context:)` in `StacklyCommands`, publish a `StacklyAction` from the page with `.publishesCommands(.page(section), …)` (or from `AppShell` for the shell scope), and update the tables above. The map tests and the source rules run by themselves.
5. The menus inside the window (More, context menus) use the same action and title without a shortcut, and a toolbar button's tooltip comes from `CommandMap.helpText(for:)`.

A command is on exactly when the window that published it is the key window and `action.availability(gate:)` is `.enabled`. Settings, the design gallery, a sheet, an alert, and onboarding switch the shell and page commands off by construction; the `app` scope items stay on.

### Probe results (macOS 27.0.1 26A434, Xcode 27.0 27A266a, 2026-10-21)

Run with the accessibility API (`AXMenuItemCmdChar`, `AXMenuItemCmdModifiers`) and key events posted to the app's process. P1 ran against the real Debug build of Stackly with a temporary `STACKLY_ROOT`. P2 to P7 ran against a small stand in app (a `Window` plus `Settings` and a second `Window`, a toolbar `.searchable` page, a sheet and an `NSAlert` sheet, and the same `Commands` shapes as spec 0069), because the real shell needs a daemon and a finished `StacklyAction`. The stand in lives outside the repo. Repeat the probes when macOS or Xcode moves.

| Probe | Result | Effect |
| --- | --- | --- |
| P1 menu bar of the current app | No two items share a key equivalent. Beyond the table A of spec 0069 the system adds ⌥⌘Q (Quit and Keep Windows) and a family of 🌐 equivalents: 🌐D (Start Dictation…), 🌐E (Emoji & Symbols), 🌐F (Enter Full Screen), and, in Window, 🌐⌃F (Fill), 🌐⌃C (Center), 🌐⌃R (Return to Previous Size), and the 🌐⌃ arrow items of Window > Move & Resize (Left, Right, Top, Bottom and the corners and halves). AX reports the 🌐 modifier as bit 16 of `AXMenuItemCmdModifiers`. The Edit menu also gets Writing Tools and AutoFill (no shortcuts). ⌥⌘W (Close All) and ⌥⌘M (Minimize All) show up as the table says. | ⌥⌘Q joins table A; the 🌐 family is covered by rule E and is not listed. |
| P2 `Window(...).keyboardShortcut("0", .command)` | Window > Probe ⌘0 appears. It reopens the window after it is closed (menu click and the key), and restores it from the Dock after Minimize. | Passes: `showMainWindow` stays a `system` row with the shortcut on the scene. |
| P3 `CommandGroup(replacing:)` | `.newItem` leaves File with New Probe…, Close ⌘W, and Close All, and no "New Window". `.textEditing` leaves Writing Tools, AutoFill, Start Dictation, and Emoji & Symbols in Edit and removes nothing else that was there. `.help` keeps the system search field and puts Welcome to Probe below it. `CommandMenu("Instance")` lands between View and Window. | Passes. |
| P3b default `.textEditing` group | With the default group left in place the Edit menu has no Find item at all, with a focused text field or without one, and ⌘F does not focus the toolbar search field. The Spelling and Grammar, Substitutions, Transformations, and Speech submenus do not appear either. | The fallback does not apply: Stackly's own `replacing: .textEditing` with Find… stays the plan. |
| P4 `focusedSceneValue` | Readable in `Commands` while the main window is key. Nil while Settings or another `Window` is key. Still present while a SwiftUI `.sheet` or an `NSAlert` sheet is open, and then `NSApp.keyWindow` is the sheet window (`SheetPresentationWindow` or the alert's panel) and the main window's `isKeyWindow` and `MainWindowKey.isKey` are false. Key presses sent while a sheet is open do nothing. | Passes: `isWindowKey` decides, not the focused value alone. |
| P5 `.searchFocused` | ⌘F from the menu focuses the toolbar `.searchable` field. Pressing ⌘F again leaves its text alone. | Passes. |
| P6 shortcuts in the search field | ⌘R, ⇧⌘R, ⌥⌘R, ⌘O, ⌘D, ⌘1, and ⌘[ run their menu item while the search field is focused and do not change its text. A `Button` whose title changes (Show Inspector to Hide Inspector) changes in the menu. `KeyEquivalent("?")` with ⌘ shows "⌘?" and runs on ⇧⌘/ (`KeyEquivalent("/")` with ⇧⌘ shows "⇧⌘/" and would also match that key). | Passes. |
| P7 state changes around a key press | A toggle flipped and a key pressed at once, 50 ms, and 300 ms later: the menu item was already disabled or enabled each time, so the stale item never ran (the run guard was never needed). Opening a sheet and pressing ⌘R at once did nothing. With the Instance menu open, an item turned from disabled to enabled after a state change 1.5 s later. | Passes: menus recompute on every published change, also while open. The run guard stays as defence. |

The menu bar order is Stackly, File, Edit, View, Instance, Window, Help. A closed menu answers AX reads with its current state, so a read after a change is a valid check.

## Accessibility

- **Increase Contrast.** Tone text and `textSecondary` switch to their HC values. `.cardSurface()`, `StateBanner`, and `ToneBadge` get a 1 pt border (`separatorColor` for cards, the tone fill for banners and badges). Text and borders read the same source, `colorSchemeContrast`, so they always agree.
- **Reduce Transparency.** Custom surfaces are opaque from the start, so nothing changes; macOS makes its own glass opaque.
- **Differentiate Without Color.** Status is never color alone. Every status label has text, every banner has a symbol and a title, and every badge has a label. The status label swaps its dot for a status symbol (see [Status](#status)).
- **Stale content is never dimmed.** The mockup's 0.6 opacity fails AA.
- **Reduce Motion.** See [Motion](#motion).
- **Keyboard.** Every button in placeholders, banners, and dialogs can be focused and pressed (Tab and Space). Custom click areas are at least 24 × 24 pt.
- **VoiceOver.** Every view has a label. Decorative icons (section icons, placeholder symbols, swatches) are hidden with `accessibilityHidden(true)`. A placeholder or banner is one group labeled by its title.
- **Large text on macOS 27.** Checked in build step 4 on macOS 27 with Xcode 27.0 (27A266a): `.dynamicTypeSize` does not change the size of any text style (`.body` stays 13 pt from `.xSmall` to `.accessibility5`, both through `Font.resolve(in:)` and in a rendered `Text`), and `@ScaledMetric` does not scale either. So there is no homemade scaling layer. The large text check becomes a narrow width check: every gallery page and its third preview ("Narrow width") render at 640 pt, the minimum content width of the main window (`GalleryLayout.narrowWidth`, from spec 0013; see [App shell](#app-shell)), with the longest labels and details, to prove labels are not cut and text wraps. In the Design Gallery window, use the Narrow Width toolbar toggle. `TypographyTests.textStylesIgnoreDynamicTypeSizeOnMacOS` guards this finding: if it fails, macOS now scales text styles, so switch the third preview to `.dynamicTypeSize(.accessibility3)` and update this paragraph.
- **Checking in the real app.** Build Debug, open Window › Design Gallery, then change System Settings › Accessibility › Display (Increase contrast, Reduce transparency, Differentiate without color, Reduce motion) and the light or dark appearance. Spec 0012 lists what `/check verify` checks there.

## App shell

The main window of spec 0013: a sidebar, one daemon banner, and the selected page. The code lives in `Packages/StacklyUI/Sources/StacklyUI/Shell/`, `Diagnostics/`, and `Settings/`; the scenes and the AppKit hooks live in the app target (`StacklyApp.swift`, `AppDelegate.swift`).

### Scenes and window

- `Window("Stackly", id: "main")` with `AppShell`, `.defaultSize(width: 1280, height: 800)`, and `.windowResizability(.contentMinSize)`. `AppShell` sets `.frame(minWidth: 900, minHeight: 600)`, the sidebar column `min: 200, ideal: 236, max: 260`, and the content column `min: 640`, so content is never narrower than 640 pt.
- `Settings { SettingsPlaceholder() }`: one General tab, "Coming in a later build." (provisional until feature 32). The footer's `SettingsLink` and ⌘, open it.
- The Debug `Design Gallery` window stays.
- `AppDelegate`: `applicationShouldTerminateAfterLastWindowClosed` is `false`; `applicationShouldHandleReopen` calls `WindowOpener.openMain()` when no window is visible; `applicationShouldTerminate` marks the app as quitting before any window closes.
- `AppShell` captures the scene's `openWindow` into `WindowOpener` when it appears, and `MainWindowAccessor` (an `NSViewRepresentable` in its background) reports the main window's `willCloseNotification`. The main window is known through this accessor, never through an identifier SwiftUI generates.

Checked on macOS 27.0 (26A428) with Xcode 27.0 (27A266a), in the Debug app driven through the accessibility API (close button, a `kAEReopenApplication` Apple event as the Dock sends, the Quit menu item, and `AXSize`):

- Closing the main window keeps the app running; AppKit asks `applicationShouldTerminateAfterLastWindowClosed` right after the window's `willClose`.
- With no visible window, the reopen event reports `hasVisibleWindows: false`, and the captured `openWindow` reopens the `Window` scene at its last size. A minimized main window counts as visible (`hasVisibleWindows: true`), and returning `true` lets AppKit restore it.
- On Quit with the window open, the order is `applicationShouldTerminate`, `applicationWillTerminate`, then the main window's `willClose`. So the quitting mark is always set before the close, and the close notice never shows while quitting.
- After quitting with the main window closed, the next launch opens the main window again; no `defaultLaunchBehavior` is needed.
- Setting the window to 500 × 400 stops at 900 × 632: 900 × 600 of content plus the 32 pt title bar.

### Sidebar

- `List(selection:)` in `.sidebar` style: Overview, Instances, Environments without a title, then Library (Catalog, Runtimes) and System (Operations, Diagnostics). Each row is a `Label` with `SectionIcon` and a native `.badge`.
- The selection is `@SceneStorage("shell.section")`, Overview by default, so the system restores it per window.
- Badges come only from loaded snapshots (`SidebarSection.badge(counts:isConnected:)`): no badge at zero or before a snapshot loaded. Instances keeps its count while the daemon is unreachable, read by VoiceOver as "3 instances, last known". Operations shows running and cancelling operations only while connected, with `.badgeProminence(.increased)`. The badges are native, not the mockup's blue capsule and red numbers.
- `DaemonFooter` sits in the sidebar's bottom safe area: a `StatusLabel` in the `.row` context (`.callout`) and an icon only `SettingsLink` (`slider.horizontal.3`, help "Settings (⌘,)", VoiceOver "Settings", 24 × 24 pt hit target).

### Daemon status

`ShellStatus` has one case for each row of the daemon status table of spec 0013, and `ShellPresentation(status:lastErrorCode:isReconnecting:)` turns it into the footer status, the window banner, `MutationAvailability`, and the "Waiting for stacklyd" placeholder for pages without data. `ShellPresentationTests` pins every row.

| Status | Footer | Banner tone | Banner actions | Mutations |
| --- | --- | --- | --- | --- |
| connected, ready | positive, "stacklyd running" | none | | available |
| connected, degraded or unknown health | caution | none | | available |
| refreshing | caution, "Refreshing…" | the previous banner | the previous actions | the previous gate, or "Loading the latest state." after launch |
| starting | caution | informative | none | blocked |
| stopping | caution | informative | Open Diagnostics | blocked |
| connecting | caution, "Connecting…" | none, informative after 1 s | Open Diagnostics | blocked |
| unreachable | critical, "stacklyd unavailable" | caution | Reconnect, Open Diagnostics | blocked |
| incompatible, untrusted | critical | critical | Reconnect, Open Diagnostics | blocked |
| recovery, misconfigured | critical | critical | Open Diagnostics | blocked |

- `DaemonBanner` is a `StateBanner(…, base: .window)` in one `safeAreaInset(edge: .top)` of the content column, above every page. "Showing the last known state from <time>." uses `StaleNote.formattedSince` with the system calendar, locale, and time zone, and updates every minute. Banner actions never mutate, so the gate never disables them. Reconnect shows a small spinner while an attempt runs. A new banner is a new view, so a new critical banner is announced.
- Item status never comes from the daemon status: a lost connection adds the banner and blocks mutations, and nothing turns Stopped or Unknown.

### Pages

- Each page sets its toolbar title and subtitle with `.pageChrome(title:subtitle:)` and adds its own actions with `.toolbar`; an action that changes data also takes `.mutationGated()`.
- `UnbuiltPage(section:)`: a page size `StatePlaceholder` with the section symbol, the section title, and "Coming in a later build.", with no action and no subtitle. Overview, Environments, Catalog, Runtimes, and Operations use it until features 22, 44, 17, 18, and 35; Instances uses it until milestone 4 of feature 13 adds the provisional instance list.
- Diagnostics: the Connection section, then the place for feature 34's doctor results.

### Diagnostics connection section

- A `.cardSurface()` card of `LabeledContent` rows: the label in `textSecondary` in a 120 pt column, the value filling the rest. The rows are Status, Socket, Last error, What to do, Next attempt, Last connected, stacklyd, Protocol, Contract, and Health; a row without a value is left out, except Last error ("None") and Last connected ("Never").
- Codes and the socket path are `.callout.monospaced()`. Every value wraps and can be selected; the socket path breaks after its slashes at 640 pt.
- Times use `DiagnosticsTime.format` (abbreviated date and time with seconds) with the system calendar, locale, and time zone.
- Below the card: Reconnect while there is no connection (disabled with "Relaunch Stackly after fixing the problem." shown as `DisabledReasonText` while misconfigured), Refresh while there is one, and Copy Details, which copies the same rows as `Label: value` lines. Nothing shows the value of `STACKLY_ROOT`.
- `ConnectionHint` holds the words for every untrusted reason and the hint for every connection error code.

### Close notice

`CloseNotice` shows once per launch, on the first close of the main window that is not part of quitting: a borderless, non activating `NSPanel` at the bottom center of the window's screen with an opaque `.cardSurface(radius: .large)` card, "Stackly is still running" and "Closing the window doesn’t stop stacklyd or your instances. Click Stackly in the Dock to reopen it." (provisional until feature 32). It fades in and out with the 0.2 s crossfade, stays 4 seconds or until clicked, and its text is announced to VoiceOver. It is AppKit because SwiftUI has no presentation that outlives its window.


## Testing views

Every page and shell piece is checked from fixture data, with no `stacklyd` running (spec 0068). Three layers, from cheapest to most real: fixtures, snapshot tests, and XCUITest.

### Fixtures

- Everything lives in `StacklyUI/Sources/StacklyUI/Fixtures/`, and every file starts with `#if DEBUG`. Data is built only from the generated `StacklyClient` DTOs through the `.fixture(...)` builders in `FixtureBuilders.swift`; there is no second model of daemon data. Times come from `FixtureClock`, IDs from `FixtureID.uuid(kind:number:)`, and a secret is only ever a reference.
- A **case** is one DTO in a scenario, such as an instance in one state. Add it in `FixtureScenario+Cases.swift` with the next number for its `kind`, so its UUID never changes, and add its row to the case table of the spec that owns the screen. Add the test that maps it to its label, tone, and detail next to the existing ones in `FixtureScenarioTests`.
- A **scenario** is a whole daemon world named by a `FixtureScenario.ID` (kebab case raw value). Add the `case`, build it in `FixtureScenario+Cases.swift` with its connection script and `expectedPresence`, and the catalog test picks it up. The same raw value is what `STACKLY_UI_SCENARIO` takes, so the Debug app and the UI tests play it unchanged.
- A feature that adds a DTO, a screen, or a gallery page adds its cases, scenario, snapshot suite, and UI test in the same commit. A call the scenario does not answer fails as `notSent` with `ipc.not_connected`, like the real app offline.
- Only `Fixtures/FixtureAppearance.swift` and the test folders may set an underscored environment value (`_colorSchemeContrast`, `_accessibility…`). Time-relative views read `fixtureNow` and `loadingDelay` from the environment, never `Date.now`.

### Snapshot suites

- A suite is an extension of `SnapshotTests` in `Tests/StacklyUITests/SnapshotTests/`, so snapshots never run at the same time:

  ```swift
  extension SnapshotTests {
      struct ExampleSnapshotTests {
          @Test(.evidence(covers: ["REQ-027"], expected: "The page renders in every variant"))
          func examplePage() async throws {
              try await assertSnapshot(ExamplePage(), named: "example-page")
          }
      }
  }
  ```

- `assertSnapshot` renders five files per name: `light`, `dark`, `hc-light`, `hc-dark`, and `narrow` (640 pt). Reduce Transparency must equal `light` unless the call passes `systemMaterial: true`. Use `.fitting(width: 1044, narrowWidth: 640)` for a page that has its own height and `.page` for one that fills the window. A page that needs a model renders from `settledRuntime(_:)`, never from a runtime that is still loading.
- Every top level `struct <Name>Page: View` outside `Gallery/` must be named in a file of `SnapshotTests/`; `SourceRuleTests` fails otherwise. Give a gallery page its content as a separate view so the suite renders it without the outer `ScrollView`.
- The reference PNGs sit in `SnapshotTests/__Snapshots__/<SuiteFile>/<name>.<variant>.png` and are committed. A PNG over 1 MB is refused.
- To record: run the suite with `STACKLY_SNAPSHOT_RECORD=1`. A missing reference is written and the test fails with "recorded <path>, review and commit"; a changed one is rewritten and the test still fails with "re-recorded". Run it again without the variable and it passes. A run under `cargo xtask evidence run` never writes a reference.
- To review: `cargo xtask snapshot review [--base <rev>]` writes `target/snapshot-review/index.html` and prints its path. It lists every PNG added, changed, or deleted since the merge base, with the old and new image and, for a failed run, the `actual` and `diff` images from `target/snapshot-failures/`. Look at every changed image before you commit it.
- Snapshots stop as `BLOCKED` (owner: user) without a login session or while Increase Contrast, Reduce Transparency, Differentiate Without Color, or Reduce Motion is on in System Settings, because AppKit controls follow those settings. Turn the setting off for the run.

### UI tests

- `StacklyAppUITests` (XCTest, because only a UI test bundle can drive `XCUIApplication` and run the Apple audit) launches the Debug app on a scenario: `launch(.connectedMixed, appearance: .dark)` sets `STACKLY_UI_SCENARIO` and `STACKLY_UI_APPEARANCE` and starts with `-ApplePersistenceIgnoreState YES`. The app reads both only in `AppDelegate`, under `#if DEBUG`; an unknown scenario name exits 64.
- Find an element by `accessibilityIdentifier` (`<scope>.<part>[.<key>]`, constants in `Accessibility/AccessibilityID.swift`) or by its label. A menu item is found by its title from `CommandMap`, because SwiftUI does not pass identifiers to menu items. A test class is `final class <Area>UITests: XCTestCase` marked `@MainActor`; the target's default isolation is `nonisolated`, since XCTest's initializers are not main actor. Its first line is `try evidence(covers:expected:)`, and a test never calls `XCTSkip` itself; `cargo xtask evidence check` rejects it.
- Attach a screenshot of the window after the assertions with `attachScreenshot(of:scenario:appearance:part:)`. It is for people to look at; nothing compares it.
- Run it with `cargo xtask ui-test [--only <Class>[/<method>]]`, or as evidence with `cargo xtask evidence run --kind fixture -- cargo xtask ui-test`. The run needs Automation Mode, which only you can turn on (a system setting with an administrator password), and an unlocked screen, and it takes over the mouse and keyboard. Without Automation Mode the task exits 2 and the tests are recorded `BLOCKED`. Results go to `target/ui-tests/<UTC time>/`: `result.xcresult` and `attachments/` with the screenshots.
- The audit tests (`AuditUITests`) call `performAccessibilityAudit` for contrast, element detection, hit region, element description, action, and parent/child. To accept an issue, add a row to `AuditExceptions.swift` with the audit type, the element (an identifier, or text its label contains), the reason, and the spec that accepts it. The list starts empty; a row needs the lead's approval first, so fix the view if you can.

### Limits

- Snapshots draw page content and the shell pieces that render on their own (banner, footer). The sidebar, toolbar, and glass are not captured; the window frame is only in the UI test screenshots.
- Increase Contrast and Reduce Transparency in snapshots are pinned through underscored environment values that Apple does not document. A guard test fails when an SDK stops honoring them, and then the checks fall back to the manual list. System colors bridged from AppKit and text in `List` and `Table` rows keep their normal color in the HC variants.
- XCUITest on macOS cannot read keyboard focus or key equivalents, so Tab order, Space on a focused button, and duplicate shortcuts stay manual (duplicates are checked by spec 0069's own tests). The real system settings and the VoiceOver voice are manual too.
- The UI tests do not compare screenshots pixel by pixel, and an audit of text over glass depends on the wallpaper.
