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

Filled by milestone 3.

## Confirmation

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
- **Large text on macOS 27.** Checked in build step 4 on macOS 27 with Xcode 27.0 (27A266a): `.dynamicTypeSize` does not change the size of any text style (`.body` stays 13 pt from `.xSmall` to `.accessibility5`, both through `Font.resolve(in:)` and in a rendered `Text`), and `@ScaledMetric` does not scale either. So there is no homemade scaling layer. The large text check becomes a narrow width check: every gallery page and its third preview ("Narrow width") render at 640 pt, the minimum window width (`GalleryLayout.narrowWidth`, provisional until spec 0013 records the real minimum), with the longest labels and details, to prove labels are not cut and text wraps. In the Design Gallery window, use the Narrow Width toolbar toggle. `TypographyTests.textStylesIgnoreDynamicTypeSizeOnMacOS` guards this finding: if it fails, macOS now scales text styles, so switch the third preview to `.dynamicTypeSize(.accessibility3)` and update this paragraph.
- **Checking in the real app.** Build Debug, open Window › Design Gallery, then change System Settings › Accessibility › Display (Increase contrast, Reduce transparency, Differentiate without color, Reduce motion) and the light or dark appearance. Spec 0012 lists what `/check verify` checks there.
