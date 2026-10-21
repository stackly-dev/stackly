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

`.labeledContentStyle(.stackly)` draws the label in `.body` primary text and the value in `.body` `textSecondary`, selectable. When both fit, they are one line with the value flush right; otherwise the label sits above and the value wraps below it, flush left, never cut. A screen may give the value its own font (`.callout.monospaced()` for an endpoint, a path, or a digest); the color stays `textSecondary`. VoiceOver reads the row as one element, "{label}, {value}". A row's own button, such as `CopyButton`, stands next to the `LabeledContent` in an `HStack`, never inside it.

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

### Sheet

Filled by milestone 2.

### Checklist

Filled by milestone 2.

### Staged progress

Filled by milestone 2.

### Inspector

Filled by milestone 3.

### Log

Filled by milestone 4.

### Diff

Filled by milestone 4.

## Motion

- `StacklyMotion.crossfade` is the only transition in Stackly's own views: `.opacity` with `StacklyMotion.crossfadeAnimation` (ease in out, 0.2 s). Use it for status changes, banners appearing, and changes of `ViewState` shape, in every setting, including Reduce Motion.
- Never animate `.move`, `.slide`, `.scale`, or `.offset`.
- `StacklyMotion.progress(reduceMotion:)` returns the animation for a change of a progress value, or `nil` while `accessibilityReduceMotion` is on, so the value jumps. A spinner keeps spinning.
- `announce(_:)` posts one VoiceOver announcement (`AccessibilityNotification.Announcement`). State views call it once when a page turns failed and when a critical banner appears.

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

