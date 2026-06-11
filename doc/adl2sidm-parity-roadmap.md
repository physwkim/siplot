# adl2sidm → adl2pydm parity roadmap

Tracks the port of [adl2pydm](https://github.com/BCDA-APS/adl2pydm)
(`~/codes/adl2pydm`, a Python tool converting MEDM `.adl` screens to PyDM `.ui`
files) into the **`adl2sidm`** workspace crate, which instead converts MEDM
`.adl` screens to **SiDM (Rust)** display modules.

`adl2pydm` parses `.adl` into a widget tree and writes PyDM `.ui` (Qt Designer
XML) that PyDM loads at runtime. SiDM has no runtime display loader — SiDM
screens are programmatic Rust structs (an `eframe::App` holding widgets + an
`Engine`) — so `adl2sidm` emits **Rust source** that constructs the equivalent
`sidm` widgets. Because the output is Rust, the generated screen can be
*compile-verified* against the real `sidm` APIs (the `tests/compiles.rs` gate),
a check `adl2pydm` cannot do against Qt.

Plan of record: `~/.claude/plans/deep-growing-balloon.md`.

Status legend: ✅ done · 🚧 in progress · ⬜ planned · ⏸ deferred (tracked, not
dropped).

## Architecture decisions

- **New crate `adl2sidm`** (binary + library), the workspace's third member
  after `siplot` and `sidm`. The converter emits source as text, so it needs no
  GUI/async/EPICS dependencies — only a CLI parser. A dev-dependency on `sidm`
  backs the compile-check fidelity test.
- **Output = generated Rust source**, one module per `.adl` screen: a `Screen`
  struct holding the widgets + `Engine`, a `new(cc: &eframe::CreationContext)`
  builder, and a `ui(&mut self, ui)` draw method. (A runtime display-file format
  + loader is the larger alternative — deferred, matching the `sidm` plan's
  deferral of display loading.)
- **Absolute positioning.** MEDM screens are absolute `x/y/w/h`; each widget is
  placed at its MEDM `Rect` via egui absolute placement inside a fixed-size
  canvas sized to the `display` block. (Proportional/grid scaling — adl2pydm's
  `grid_layout.py` / `use_layout` — is a later optional wave.)
- **Z-order: decoration behind, controls on top.** A hard correctness rule, not
  cosmetics: in egui a later-drawn `Area` renders on top *and captures pointer
  input*, so a MEDM static rectangle over a control would hide it and steal its
  clicks. Within each container, widgets are partitioned by draw category and
  emitted back-to-front — decoration (`static`) → `monitor` → `controller` —
  preserving MEDM order within each category. The category → z-layer table is a
  single owner next to the symbol map.
- **Default channel protocol `ca://`** (MEDM is a Channel Access tool); bare
  MEDM PV names get the prefix. Overridable via `--protocol`; basic `$(macro)`
  substitution via `--macro` (port of adl2pydm `convertMacros`).
- **Not pursued (decided, 2026-06-11):** a **runtime `.adl` loader** and
  **proportional/grid scaling**. The compile-time "generate Rust → build → run"
  model is sufficient; a runtime loader's only gain (no rebuild on `.adl` change)
  costs the compile-fidelity gate, which is the whole reason codegen was chosen
  over a loader — so it stays unbuilt by design, not as a backlog item. Scaling
  is unneeded (faithful absolute placement is the target). MEDM dynamic-attribute
  **colour** rules (`clr="alarm"/"discrete"`) are *beyond* the parity target —
  adl2pydm does not convert them either — so they are intentionally not
  implemented (see the CALC note). The arc/polygon/polyline shapes, the
  static-file `image`, related-display/shell-command/embedded-display, and CALC
  *visibility* — all originally deferred — are now implemented (see the coverage
  table).

## MEDM widget coverage

One row per MEDM widget (the keys of `adl2pydm/symbols.py` `adl_widgets`).
Category drives the z-layer: `static` = decoration (back), `monitor` = read-only
(middle), `controller` = interactive (front).

| MEDM widget | category | SiDM target | status |
|---|---|---|---|
| text | static | `SidmLabel` | ✅ |
| text update | monitor | `SidmLabel` | ✅ |
| text entry | controller | `SidmLineEdit` | ✅ |
| menu | controller | `SidmEnumComboBox` | ✅ |
| choice button | controller | `SidmEnumButton` | ✅ |
| message button | controller | `SidmPushButton` | ✅ |
| valuator | controller | `SidmSlider` | ✅ |
| wheel switch | controller | `SidmSpinbox` | ✅ |
| byte | monitor | `SidmByteIndicator` | ✅ |
| bar | monitor | `SidmScaleIndicator` | ✅ |
| indicator | monitor | `SidmScaleIndicator` | ✅ |
| meter | monitor | `SidmScaleIndicator` | ✅ |
| composite | container | `SidmFrame` (children re-layered inside) | ✅ |
| rectangle | static | `SidmDrawing(Rectangle)` | ✅ |
| oval | static | `SidmDrawing(Ellipse)` | ✅ |
| strip chart | monitor | `SidmTimePlot` | ✅ |
| cartesian plot | monitor | `SidmWaveformPlot` / `SidmScatterPlot` | ✅ |
| image | static | `SidmImage` (channel-less static GIF/TIFF file) | ✅ |
| arc | static | `SidmDrawing(Arc { begin_deg, span_deg })` | ✅ |
| polygon | static | `SidmDrawing(Polygon)` | ✅ |
| polyline | static | `SidmDrawing(Polyline)` | ✅ |
| related display | controller | live `egui::Button`/menu (reports target on click) | ✅ |
| shell command | controller | live `egui::Button`/menu (spawns each command) | ✅ |
| embedded display | container | `SidmFrame` (target inlined at code-gen) | ✅ |

> The MEDM `image` is a static *file* picture with no data channel, so it is
> decoration (Background layer) — a divergence from adl2pydm's `type="monitor"`,
> which targets Qt's native z-order. `related display`/`shell command` are
> clickable, so they sit in the Control (front) layer even though `symbols.py`
> types them `static`.

Dynamic-attribute CALC **visibility** (`vis`/`calc`) is **full parity** with
adl2pydm: its `convertDynamicAttribute_to_Rules` (output_handler.py) emits only a
`{name:"visibility", property:"Visible"}` rule from the channels/`calc`/`vis`
mode and ignores the `clr` colour mode entirely, so visibility is the whole of
what the reference converts. adl2sidm ✅ wires it as a live `calc://` gate. For
each gated widget the emitter builds a
`calc://adl2sidm_vis_<line>?expr=<expr>&A=<chan>&…&update=A,B` channel that
evaluates the rule and wraps every `place(...)` it produced in
`if gate.read(…) != Some(0.0) { … }` (hidden only when the gate reads exactly
zero). MEDM-CALC → evalexpr: `#` → `!=`, standalone `=` → `==`; the `vis` mode
wraps the optional `calc` expression (`if zero` → `(expr)=0`, `if not zero` →
`(expr)#0`, `calc` → verbatim, default channel `A`). A rule is recognised when
`vis` is conditional (anything but `"static"`) or a `calc` is present;
`vis="static"` with only a channel is not a rule. An expression containing `&`
(logical/bitwise AND) cannot be carried by the `calc://` query (which splits on
`&`), so that widget is left always-visible with a warning rather than emitting
a silently-wrong gate.

Dynamic-attribute **colour** rules (`clr="alarm"/"discrete"`) are **beyond
parity**, not a gap: adl2pydm does not convert them either (it never reads
`clr`). SiDM does carry per-widget alarm styling (`with_alarm_sensitive_content`
/`with_alarm_sensitive_border`, default border-on/content-off), so a future
beyond-parity pass could wire `clr="alarm"` onto the widgets that expose those
builders; `clr="discrete"` would need a discrete-colour rules engine. Tracked,
intentionally not implemented (would exceed the adl2pydm parity target).

## Wave / commit log

- ✅ A1 — workspace member `adl2sidm` scaffold (binary + library) + this
  roadmap skeleton; root `Cargo.toml` `[workspace] members` += `adl2sidm`.
- ✅ A2 — `adl_parser.rs` (block parser + widget-tree IR). Faithful port of
  `adl_parser.py`: line-oriented block/assignment scanning, colour-table
  resolution (`colors` hex list or `dl_color` blocks), geometry, `control`/
  `monitor`/etc. attribute blocks (whose `clr`/`bclr` override the widget colour,
  as in `parseColorAssignments`), `limits` splicing, `points`, recursive
  `composite` children, and indexed `trace`/`pen`/`display`/`command` records.
  6 unit tests; sanity-checked against all 60 real adl2pydm fixtures (no panic).
- ✅ A3 — `symbols.rs` (MEDM → SiDM map + category + z-layer table). `lookup`
  maps every MEDM widget to its SiDM target + a draw `Category`
  (Decoration/Monitor/Control/Container); `Category::z_layer` is the single
  owner of the back-to-front ordering, `has_channel` tells the emitter whether
  to wire a PV. `related display`/`shell command` are typed Control (front) even
  though adl2pydm types them `static`, so a decoration cannot occlude them.
  6 tests (full-coverage of `ADL_WIDGET_SYMBOLS`, z-layer ordering, stub flags).
- ✅ B4 — `codegen.rs` scaffold + simplest widgets (text / text update / text
  entry). Emits the `Screen` struct + `new(cc)` + `ui()` + the absolute `place`
  helper; channel address = `control`/`monitor` `chan` with `$(macro)`
  substitution + protocol prefix; `precDefault` → `.with_precision`; static
  `text` → a fieldless `ui.label`. The z-order is applied as a stable sort by
  `ZLayer` AND per-Area `egui::Order`. Imports are conditional so output is
  warning-clean. 4 codegen tests; the generated screen was smoke-checked to
  `cargo check` clean against real sidm/siplot/eframe (confirming the forked
  `eframe::App::ui(ui, frame)` shape the C11 example will wrap).
- ✅ B5 — emitter batch: controls (message button, menu, choice button, valuator,
  wheel switch, byte). `message button` → `SidmPushButton` (label = MEDM `label`,
  `press_msg`/`release_msg` → press/release values); `menu` → `SidmEnumComboBox`;
  `choice button` → `SidmEnumButton` (`stacking="column"` → horizontal; `row` =
  default vertical); `valuator` → `SidmSlider` (user-defined `*Src="default"`
  limits → `with_limits`, `dPrecision` → `with_precision`, parsed as float to
  match adl2pydm's `1.000000` form); `wheel switch` → `SidmSpinbox` (limits +
  precision from MEDM `format`, falling back to the `limits` block's `precDefault`
  that real wheel-switch screens carry); `byte` → `SidmByteIndicator`
  (`sbit`/`ebit` → `num_bits` = `1+|ebit-sbit|`, `shift` = `min(sbit,ebit)`;
  `direction` `right`/`left` → horizontal). Big-endian display order (`sbit<ebit`)
  has no `SidmByteIndicator` builder yet — reported as a warning, not dropped.
  A single `push_channel_widget` owner emits every channel widget's ctor + field +
  placement, so `let _ = self.wN.show(ui);` and the back-to-front layering are
  uniform. 7 new codegen tests; the full 6-control screen was smoke-checked to
  `cargo check` clean (no warnings) against real sidm.
- ✅ B6 — emitter batch: indicators + shapes (split into B6a/B6b/B6c for the
  composite's nested re-layering).
  - ✅ B6a — scale indicators (`bar`/`indicator`/`meter` → `SidmScaleIndicator`).
    `bar` → `with_bar_indicator(true)` + the MEDM decoration `label` drives the
    value label (PyDM `showValue`: shown only for `limits`/`channel`, vs SiDM's
    show-by-default); `meter` shares the `indicator` (pointer-scale) emitter, as
    adl2pydm's `write_block_meter` does. User-defined limits, `precDefault`, and
    `direction` map to `with_limits`/`with_precision`/`with_orientation`. A single
    `direction_orientation` owner now maps MEDM `direction` → sidm `Orientation`
    for both the scale indicators and `byte` (byte was re-pointed at it, fixing a
    latent mismatch where an unknown direction warned "using right" but left the
    widget vertical). 4 new codegen tests; smoke-checked clean against real sidm.
  - ✅ B6b — shapes (`rectangle` → `SidmDrawing(Rectangle)`, `oval` →
    `SidmDrawing(Ellipse)`). Channel-less decorations use a unique `loc://`
    placeholder (`dynamic_channel`); a `dynamic attribute` `chan` overrides it.
    The `basic attribute` block sets the brush/pen: `fill="solid"` →
    `with_fill(colour)`, `fill="outline"` (MEDM `NoBrush`) →
    `with_fill(Color32::TRANSPARENT)` + a border forced to width >= 1 (as
    adl2pydm does); `width>0` adds `with_border`. `style="dash"` has no
    `SidmDrawing` pen-style builder, so it warns rather than dropping silently.
    A shared `apply_protocol` now backs both `channel_address` and
    `dynamic_channel`. 4 new codegen tests; smoke-checked clean against real sidm.
  - ✅ B6c — `composite` → `SidmFrame` with children re-layered (back-to-front)
    and coordinate-translated to the frame interior. The composite's children are
    emitted by draining the placements the recursion produced
    (`placements.drain(start..)`), re-sorting them by `ZLayer`, and writing each
    inside the frame's `show(ui, |ui| { … })` closure with coordinates translated
    to the frame origin — so the back-to-front rule holds *independently* inside
    every frame, and nesting (composite-in-composite) translates coordinates
    recursively. The frame is a `SidmFrame` on a `loc://` placeholder channel
    (or the composite's own `chan` when set). `ui()` destructures
    `let Self { _engine: _, w0, w1, … } = self;` so a frame closure can borrow
    its sibling fields disjointly (`SidmFrame` paints `Frame::NONE`, so it never
    occludes the children it wraps). 8 new codegen tests, incl. a nested
    composite-in-composite asserting two frames, recursive coordinate
    translation, and the deepest control nested inside both closures; the
    single- and nested-composite screens were generated and `cargo check`'d clean
    (no warnings) against real sidm. Gate: clippy -p adl2sidm clean, nextest
    39/39.
- ✅ B7 — emitter batch: plots (strip chart, cartesian plot). `strip chart` →
  `SidmTimePlot`, one `add_channel` per MEDM `pen`; `period` scaled by `units`
  (`minute`→60, `hour`→3600) sets `with_time_span` (converting MEDM's unit-tagged
  period to sidm's seconds, where adl2pydm passes it through raw). `cartesian
  plot` → `SidmWaveformPlot` (default) or `SidmScatterPlot` (`--use-scatterplot`):
  each `trace` is a curve. Waveform — a trace needs `ydata` (else skipped, as
  adl2pydm requires a `y_channel`); `xdata` present → `add_xy_channel(y, Some(x))`,
  absent → `add_channel(y)` (Y vs index). Scatter — a trace needs *both* `xdata`
  and `ydata` (sidm's scatter pairs two scalar channels in `(x, y)` order); a
  trace missing either is warned and skipped, and `count` maps to the scatter
  buffer size (waveform has no per-curve budget, so `count` is dropped there).
  Pen/trace colours resolve from `clr`/`data_clr` against the table. A new
  `push_plot_widget` owner emits the `let mut <field> = …::new(rs, <PlotId>)…;`
  constructor plus a follow-up `add_*` per curve and the back-to-front placement,
  so plots layer uniformly with the other widgets; each plot gets a distinct
  `PlotId`. 4 new codegen tests (strip-chart pens + unit-scaled span; waveform
  x/y vs y-only traces, count dropped; scatter buffer + (x,y) order + missing-x
  skip-warning; both plots Middle-layer with distinct ids). The waveform- and
  scatter-mode screens were generated and `cargo check`'d clean (no warnings)
  against real sidm. Gate: clippy -p adl2sidm clean, nextest 43/43.
  - **`image` moved to the B8 stub set.** The plan slotted `image →
    SidmImageView` here, but the MEDM `image` widget is a *static GIF/TIFF file*
    display (`type="gif"`, `"image name"="apple.gif"`) with no channel, whereas
    `SidmImageView` is a live array-data viewer that *requires* an
    `image_address` channel. There is no faithful mapping — forcing one would
    fabricate a channel that the `.adl` never names — so `image` becomes a
    stub + warning alongside the deferred 6, not a plot emitter. (`image` still
    warns through the default dispatch arm until B8 lands its dedicated stub.)
- ✅ B8 — stubs + warnings for the deferred 6 + `image` + CALC `// TODO` comments
  (split into B8a stubs, B8b CALC comments).
  - ✅ B8a — stub emitters for every remaining MEDM widget, each warning (never a
    silent drop). The static shapes (`arc`/`polygon`/`polyline`) and the
    static-file `image` emit a fieldless red placeholder marker (`ui.label`) at
    the MEDM geometry, so the layout still shows the widget's footprint;
    `image`'s marker names the file. `embedded display` is skipped with a warning
    (no placeholder, as it is unimplemented in adl2pydm too). `related display`
    and `shell command` emit a *disabled* `egui::Button` captioned with their
    target (the widget `label` sans the MEDM `-` icon-suppress prefix, else the
    sole target's label/name, else a generic) at the control (Foreground) layer —
    no channel is fabricated and no `Engine` field is created, an honest inert
    marker; navigation/shell are deferred to match `sidm`'s own deferred set.
    Every `ADL_WIDGET_SYMBOLS` entry now has a dispatch arm; the `_` arm is a
    defensive backstop. 4 new codegen tests (Background shape placeholders +
    missing-shape warnings; image placeholder names the file and is not a
    `SidmImageView`; embedded display skipped with no placement; deferred
    controls are Foreground disabled buttons captioned by target, no
    `SidmPushButton`/channel). The 7-stub screen was generated and `cargo
    check`'d clean against real sidm. Gate: clippy -p adl2sidm clean, nextest
    47/47.
  - ✅ B8b — CALC dynamic-attribute (`vis`/`calc`) → a `// TODO: dynamic rule:`
    comment emitted just above the widget's placement, quoting the MEDM
    `vis`/`calc`/A–D channel fields verbatim, plus a warning (SiDM has no rules
    engine). A `comment: Option<String>` was threaded onto `Placement` (via a
    `Placement::drawn` constructor so the default lives in one place) and emitted
    by `write_placement`, so the note rides with the placement whether it is
    drawn at the top level or nested inside a composite frame. The dispatcher
    attaches the comment as a post-pass over the placements each widget produced:
    a composite's children are already emitted (and individually annotated)
    before the composite's own rule is attached, so the rule lands on the frame
    only, never duplicated onto a child. A rule is recognised when `vis` is
    conditional or a `calc` is present; `vis="static"` with only a channel is not
    a rule (the channel still binds, e.g. for a drawing). 3 new codegen tests
    (calc rule comment directly precedes the placement and the widget still binds
    its channel; static visibility emits no comment; a composite rule annotates
    the frame, not its child). The rule-annotated screen was generated and
    `cargo check`'d clean against real sidm. Gate: clippy -p adl2sidm clean,
    nextest 50/50.
- ✅ C9 — CLI. A binary-local `mod cli` (clap derive) drives `.adl` in → `.rs`
  out, so the library crate stays free of the `clap` dependency. Flags mirror
  adl2pydm: `-p/--protocol` (default `ca://`), repeatable `-m/--macro NAME=VALUE`
  (validated by a `value_parser`), `--use-scatterplot`, and `-o/--out` (`-` for
  stdout, else a path; default = the input path with a `.rs` extension). The
  driver falls back to the input's file name for the generated header when the
  `.adl` carries no `file { name }`, prints converter warnings to stderr, and
  exits non-zero on a read/write error (clap itself exits 2 on a bad argument).
  3 CLI unit tests (`parse_macro` splits/over-splits/rejects; `Cli::command()`
  derive is consistent); end-to-end runs on real adl2pydm fixtures (`strip.adl`,
  `scatter_plot.adl`) produced the expected `.rs` to stdout and to a derived
  path. Gate: clippy -p adl2sidm clean, nextest 53/53.
- ✅ C10 — `tests/compiles.rs` fidelity gate. A committed `Screen`
  (`tests/fixtures/sample_screen.rs`, generated by the converter from
  `tests/fixtures/sample.adl` with `-m P=DMM1:`) is `include!`d as a module;
  because the crate carries `sidm`/`siplot`/`eframe` as dev-deps, *building the
  test compiles that generated screen against the real widget APIs* — the
  strongest correctness signal, and one adl2pydm cannot get against Qt. A drift
  test re-runs the converter and asserts byte-for-byte equality with the
  committed module, so the compiled artifact can never silently fall out of date;
  a second test pins the fixture's warning set (only the `arc` placeholder and
  the rectangle's CALC `// TODO`). The fixture spans label / line edit / push
  button / combo / slider / byte / scale indicator / drawing×2 / time plot /
  waveform plot / frame. Generating it surfaced (and a separate commit fixed) a
  byte fidelity bug: `sbit < ebit` big-endian was warned-and-dropped though
  `SidmByteIndicator` can represent it; `sidm` gained `with_big_endian` and the
  emitter now applies it. Gate: clippy -p adl2sidm --all-targets clean (lints the
  included generated module too), nextest 55/55.
- ✅ C11 — runnable end-to-end example. `examples/local_panel.adl` is a MEDM
  screen whose channels are authored as `loc://`/`fake://` addresses, so the
  converted display drives itself with NO IOC (the `.adl` analogue of `sidm`'s
  `sidm_local_panel`); it is converted with `--protocol ""` (the channels already
  carry their scheme — the default `ca://` would need a live IOC) into the
  committed `examples/local_panel_screen.rs`. `examples/local_panel.rs` wraps the
  generated `Screen` (`new(cc)` / `ui(ui)`) in a tiny `eframe::App` and
  `run_native`s it — `cargo run -p adl2sidm --example local_panel`. The screen is
  laid out so the grey border `rectangle` (decoration) overlaps the line edit /
  slider / byte controls, demonstrating the z-order rule live: decoration renders
  at `Order::Background` behind controls at `Foreground` and never steals their
  clicks. A drift test (`example_screen_matches_the_committed_module` in
  `tests/compiles.rs`) keeps the committed example output in lock-step with the
  converter, and `cargo build --example local_panel` (covered by clippy
  `--all-targets`) compiles it against the real sidm/siplot/eframe APIs. Gate:
  clippy -p adl2sidm --all-targets clean, nextest 56/56, example builds.

## Phase 2 — deferred widgets implemented for real

The Wave-B plan emitted six widgets (+`image`, +CALC) as placeholders / disabled
buttons / `// TODO` comments (see B8a/B8b above — historical). Phase 2 replaces
every one of those with a real implementation. The B8a/B8b descriptions are
superseded by the coverage table and CALC note at the top of this doc.

- ✅ arc / polyline / polygon → real `SidmDrawing` (`42cbb18`). `sidm` gained
  `DrawingShape::Arc { begin_deg, span_deg }`, `Polyline`, and `Polygon`; the
  emitter parses MEDM `begin`/`path` (1/64-degree units) and the `points` block
  (normalised to the widget origin) into those shapes at the Background layer.
- ✅ `image` → channel-less `SidmImage` (`96e0f1c`). `sidm` gained `SidmImage`, a
  static GIF/TIFF *file* widget that decodes at run time; the emitter targets it
  with the MEDM `"image name"`, sized to the geometry — no fabricated channel.
- ✅ shell command → live `egui::Button`/`menu_button` (`778d6c2`). A single
  `command[0]` is a plain button that `std::process::Command::new("sh").arg("-c")`
  -spawns `"<name> <args>"`; multiple commands become a `menu_button` (one entry
  each, `ui.close()` after spawn). A `%`-containing command is warned (MEDM
  macro-arg prompting is unsupported); a name-less command is dropped.
- ✅ related display → live `egui::Button`/`menu_button` (`b2a057b`). Reports its
  target (`eprintln!("related display: open <file> (macros: …)")`) on click —
  an honest, side-effect-only navigation stand-in (SiDM has no screen-stack
  loader), not an inert disabled button.
- ✅ embedded display → inlined `SidmFrame` (`d2a252b`). The childless
  `composite` + `"composite file"="file;macros"` form is resolved at code-gen
  time: the target `.adl` is read from `Options::source_dir`, macro-merged
  (embedded macros win), parsed, and its widgets re-layered inside a `SidmFrame`
  via the shared `emit_frame_container` (origin `(0,0)`, the target's own
  coords). Cycle (canonicalised `embed_stack`) and depth (`MAX_EMBED_DEPTH=8`)
  guards fall back to a visible marker; no source dir / missing file / no
  `composite file` likewise emit a marker, never a silent drop.
- ✅ CALC dynamic-attribute **visibility** → live `calc://` gate (`06e8663`).
  `Placement.comment` (the `// TODO` note) became `Placement.gate`
  (`Option<boolean cond>`); a gated placement is wrapped in
  `if gate.read(…) != Some(0.0) { place(…) }`. The gate is a synthetic
  `calc://adl2sidm_vis_<line>?expr=<expr>&A=<chan>&…&update=A,B` channel; see the
  CALC note above for the MEDM-CALC→evalexpr translation and the `&`-limitation.
- ✅ z-order + symbol-map reconciliation (`4e1ea14`, `e8f4ad8`). `image` retyped
  `Monitor`→`Decoration` so the static picture sits in the Background layer with
  the other static graphics (it was drawing above them). The now-vacuous
  `WidgetMap.supported` flag (every widget is implemented) was removed structurally
  rather than flipped all-true, and the stale `"stub: …"` target strings were
  updated to the real targets.

Phase-2 gate (per commit): `cargo fmt --all`; `cargo clippy -p adl2sidm
--all-targets -- -D warnings` (lints the generated fixture + example too);
`cargo nextest run -p adl2sidm` (66/66). Full-workspace pass still owed before
any push.

Not pursued (decided 2026-06-11): a runtime `.adl` loader and proportional/grid
scaling — the codegen "build and run" model is sufficient and keeps the
compile-fidelity gate a loader would surrender. CALC **colour** rules
(`clr="alarm"/"discrete"`) are beyond the parity target — adl2pydm does not
convert them either — so they are intentionally not built.
