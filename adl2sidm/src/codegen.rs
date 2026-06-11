//! Emit SiDM Rust source from a parsed [`MedmScreen`].
//!
//! This is the analogue of `adl2pydm/output_handler.py`: it walks the widget
//! tree and writes the target display. Where `output_handler` writes PyDM `.ui`
//! XML, this writes a Rust module — a `Screen` struct holding the widgets + an
//! [`Engine`], a `new(cc: &eframe::CreationContext)` builder, and a
//! `ui(&mut self, ui)` draw method that places each widget at its MEDM geometry.
//!
//! Placement is absolute (MEDM screens are absolute `x/y/w/h`) via a small
//! `place` helper that draws each widget in its own `egui::Area` at a fixed
//! position. The Area's `egui::Order` encodes the z-layer, so the user's rule —
//! decoration to the back, controls never occluded or click-stolen — holds by
//! construction: decoration Areas (`Background`) render and receive input below
//! monitors (`Middle`) below controls (`Foreground`). The emitter additionally
//! lays the `place` calls out back-to-front (a stable sort by [`ZLayer`]) so the
//! ordering is also visible in the source.
//!
//! [`Engine`]: https://docs.rs/sidm
//! [`MedmScreen`]: crate::adl_parser::MedmScreen

use std::fmt::Write as _;

use crate::adl_parser::{Color, Geometry, MedmScreen, MedmWidget};
use crate::symbols::{self, ZLayer};

/// Code-generation options (the converter's CLI flags).
#[derive(Clone, Debug)]
pub struct Options {
    /// Channel protocol prefixed onto bare MEDM PV names, e.g. `"ca://"`.
    pub protocol: String,
    /// `$(name)` / `${name}` macro substitutions applied to channel names.
    pub macros: Vec<(String, String)>,
    /// Translate `cartesian plot` as a scatter plot rather than a waveform plot
    /// (mirrors adl2pydm's `--use-scatterplot`).
    pub use_scatterplot: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            protocol: "ca://".to_string(),
            macros: Vec::new(),
            use_scatterplot: false,
        }
    }
}

/// The generated source plus any warnings (unsupported widgets, skipped
/// emitters) the caller should surface.
#[derive(Clone, Debug, Default)]
pub struct Generated {
    pub source: String,
    pub warnings: Vec<String>,
}

/// One placed widget: where it goes (`z`, `geom`, a unique Area `id`) and the
/// statement(s) that draw it inside the `place` closure.
struct Placement {
    z: ZLayer,
    id: u64,
    geom: Geometry,
    body: String,
}

/// Accumulates the pieces of the generated module as the widget tree is walked.
#[derive(Default)]
struct Builder {
    /// `(field_name, field_type)` for each stateful widget (struct + `Self {}`).
    fields: Vec<(String, String)>,
    /// `let <field> = …;` constructor lines for `new()`.
    ctors: Vec<String>,
    /// Absolute placements, drawn back-to-front after a stable sort by `z`.
    placements: Vec<Placement>,
    warnings: Vec<String>,
    /// Running widget index → unique field names and Area ids.
    next_index: u64,
    /// Whether any emitted code references `Color32` / `sidm::widgets`.
    needs_color: bool,
    needs_widgets: bool,
}

impl Builder {
    /// Allocate the next unique widget index.
    fn index(&mut self) -> u64 {
        let i = self.next_index;
        self.next_index += 1;
        i
    }
}

/// Generate the SiDM Rust source for a parsed MEDM screen.
pub fn generate(screen: &MedmScreen, options: &Options) -> Generated {
    let mut b = Builder::default();
    for widget in &screen.widgets {
        emit_widget(&mut b, widget, options);
    }
    Generated {
        source: assemble(&b, screen),
        warnings: b.warnings,
    }
}

/// Dispatch one MEDM widget to its emitter, recording a warning for any symbol
/// whose emitter has not landed yet (or is an unsupported stub).
fn emit_widget(b: &mut Builder, widget: &MedmWidget, options: &Options) {
    let Some(map) = symbols::lookup(&widget.symbol) else {
        b.warnings.push(format!(
            "line {}: unknown block {:?}",
            widget.line, widget.symbol
        ));
        return;
    };

    match widget.symbol.as_str() {
        "text" => emit_static_text(b, widget, map.category.z_layer()),
        "text update" => emit_text_update(b, widget, options, map.category.z_layer()),
        "text entry" => emit_text_entry(b, widget, options, map.category.z_layer()),
        _ => b.warnings.push(format!(
            "line {}: {:?} -> {} not emitted yet (skipped)",
            widget.line, widget.symbol, map.sidm_widget
        )),
    }
}

/// `text` — a static label (a fixed string, no channel). Drawn with a plain
/// `ui.label`, so it needs no struct field.
fn emit_static_text(b: &mut Builder, widget: &MedmWidget, z: ZLayer) {
    let Some(geom) = widget.geometry else {
        b.warnings.push(format!(
            "line {}: text has no geometry; skipped",
            widget.line
        ));
        return;
    };
    let id = b.index();
    let text = widget.title.clone().unwrap_or_default();
    let color = widget.color.unwrap_or(Color { r: 0, g: 0, b: 0 });
    b.needs_color = true;
    let body = format!(
        "ui.label(egui::RichText::new({}).color({}));",
        rust_str(&text),
        color_expr(color)
    );
    b.placements.push(Placement { z, id, geom, body });
}

/// `text update` — a read-only `SidmLabel` bound to a channel.
fn emit_text_update(b: &mut Builder, widget: &MedmWidget, options: &Options, z: ZLayer) {
    emit_channel_label(b, widget, options, z, "text update");
}

/// `text entry` — an editable `SidmLineEdit` bound to a channel.
fn emit_text_entry(b: &mut Builder, widget: &MedmWidget, options: &Options, z: ZLayer) {
    let Some(geom) = widget.geometry else {
        return skip_no_geometry(b, widget);
    };
    let Some(addr) = channel_address(widget, options) else {
        return skip_no_channel(b, widget);
    };
    let id = b.index();
    let field = format!("w{id}");
    b.needs_widgets = true;

    let mut ctor = format!(
        "let {field} = SidmLineEdit::new(&engine, {})\n            .expect({});",
        rust_str(&addr),
        rust_str(&format!("adl2sidm: connect {addr}"))
    );
    apply_precision(widget, &mut ctor);

    b.ctors.push(ctor);
    b.fields.push((field.clone(), "SidmLineEdit".to_string()));
    b.placements.push(Placement {
        z,
        id,
        geom,
        body: format!("let _ = self.{field}.show(ui);"),
    });
}

/// Shared body of the channel-bound `SidmLabel` widgets (`text update`).
fn emit_channel_label(
    b: &mut Builder,
    widget: &MedmWidget,
    options: &Options,
    z: ZLayer,
    kind: &str,
) {
    let Some(geom) = widget.geometry else {
        return skip_no_geometry(b, widget);
    };
    let Some(addr) = channel_address(widget, options) else {
        return skip_no_channel(b, widget);
    };
    let id = b.index();
    let field = format!("w{id}");
    b.needs_widgets = true;

    let mut ctor = format!(
        "let {field} = SidmLabel::new(&engine, {})\n            .expect({});",
        rust_str(&addr),
        rust_str(&format!("adl2sidm: connect {addr} ({kind})"))
    );
    apply_precision(widget, &mut ctor);

    b.ctors.push(ctor);
    b.fields.push((field.clone(), "SidmLabel".to_string()));
    b.placements.push(Placement {
        z,
        id,
        geom,
        body: format!("self.{field}.show(ui);"),
    });
}

/// Append a `.with_precision(n)` builder when the MEDM widget carries a
/// `precDefault` (from its `limits` block).
fn apply_precision(widget: &MedmWidget, ctor: &mut String) {
    if let Some(prec) = widget.assignments.get("precDefault")
        && let Ok(n) = prec.parse::<i32>()
    {
        // Re-open the builder chain: replace the trailing `;` with the call.
        ctor.pop();
        let _ = write!(ctor, "\n            .with_precision({n});");
    }
}

fn skip_no_geometry(b: &mut Builder, widget: &MedmWidget) {
    b.warnings.push(format!(
        "line {}: {:?} has no geometry; skipped",
        widget.line, widget.symbol
    ));
}

fn skip_no_channel(b: &mut Builder, widget: &MedmWidget) {
    b.warnings.push(format!(
        "line {}: {:?} has no channel; skipped",
        widget.line, widget.symbol
    ));
}

/// The channel address for a widget: its `control`/`monitor` block's `chan`,
/// with macros substituted and the protocol prefixed.
fn channel_address(widget: &MedmWidget, options: &Options) -> Option<String> {
    let chan = widget
        .attributes
        .get("control")
        .and_then(|a| a.get("chan"))
        .or_else(|| widget.attributes.get("monitor").and_then(|a| a.get("chan")))?;
    let substituted = substitute_macros(chan, &options.macros);
    Some(format!("{}{}", options.protocol, substituted))
}

/// Substitute `$(name)` and `${name}` macros; unmatched references are left
/// in place (the user supplies them via `--macro`).
fn substitute_macros(input: &str, macros: &[(String, String)]) -> String {
    let mut out = input.to_string();
    for (name, value) in macros {
        out = out.replace(&format!("$({name})"), value);
        out = out.replace(&format!("${{{name}}}"), value);
    }
    out
}

/// A Rust string literal for `s`, with escaping (`{:?}` produces exactly that).
fn rust_str(s: &str) -> String {
    format!("{s:?}")
}

/// `Color32::from_rgb(r, g, b)` for a MEDM colour.
fn color_expr(c: Color) -> String {
    format!("Color32::from_rgb({}, {}, {})", c.r, c.g, c.b)
}

/// Assemble the final module source from the accumulated pieces.
fn assemble(b: &Builder, screen: &MedmScreen) -> String {
    let mut s = String::new();

    let title = if screen.adl_filename.is_empty() {
        "an MEDM screen".to_string()
    } else {
        screen.adl_filename.clone()
    };
    let _ = writeln!(
        s,
        "// AUTO-GENERATED from {title} by adl2sidm -- do not edit by hand.\n"
    );

    // Imports: egis/Engine/siplot are always used; Color32 and the widget glob
    // only when something references them (keeps the output warning-clean).
    let _ = writeln!(s, "use sidm::Engine;");
    if b.needs_widgets {
        let _ = writeln!(s, "use sidm::widgets::*;");
    }
    if b.needs_color {
        let _ = writeln!(s, "use siplot::egui::{{self, Color32}};");
    } else {
        let _ = writeln!(s, "use siplot::egui;");
    }
    s.push('\n');

    // Struct.
    let _ = writeln!(s, "/// SiDM screen generated from `{title}`.");
    let _ = writeln!(s, "pub struct Screen {{");
    let _ = writeln!(s, "    _engine: Engine,");
    for (name, ty) in &b.fields {
        let _ = writeln!(s, "    {name}: {ty},");
    }
    let _ = writeln!(s, "}}\n");

    // impl: new() + ui().
    let _ = writeln!(s, "impl Screen {{");
    emit_new(&mut s, b);
    s.push('\n');
    emit_ui(&mut s, b);
    let _ = writeln!(s, "}}\n");

    emit_place_helper(&mut s);
    s
}

/// Emit the `new(cc)` constructor.
fn emit_new(s: &mut String, b: &Builder) {
    let _ = writeln!(
        s,
        "    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {{"
    );
    let _ = writeln!(
        s,
        "        let rs = cc.wgpu_render_state.as_ref().expect(\"adl2sidm: a wgpu render state is required\");"
    );
    let _ = writeln!(s, "        siplot::install(rs);");
    let _ = writeln!(s, "        let engine = Engine::new();");
    let _ = writeln!(s, "        engine.attach_repaint(cc.egui_ctx.clone());");
    for ctor in &b.ctors {
        let _ = writeln!(s, "        {ctor}");
    }
    let _ = write!(s, "        Self {{ _engine: engine");
    for (name, _) in &b.fields {
        let _ = write!(s, ", {name}");
    }
    let _ = writeln!(s, " }}");
    let _ = writeln!(s, "    }}");
}

/// Emit the `ui()` draw method: placements sorted back-to-front.
fn emit_ui(s: &mut String, b: &Builder) {
    let _ = writeln!(s, "    pub fn ui(&mut self, ui: &mut egui::Ui) {{");
    let _ = writeln!(
        s,
        "        // Back-to-front: decoration (Background) -> monitor (Middle) -> control"
    );
    let _ = writeln!(
        s,
        "        // (Foreground), so controls are never occluded or click-stolen."
    );
    let mut order: Vec<&Placement> = b.placements.iter().collect();
    order.sort_by_key(|p| p.z); // stable: preserves MEDM order within a layer

    if order.is_empty() {
        let _ = writeln!(s, "        let _ = ui;");
    }
    for p in order {
        let Geometry {
            x,
            y,
            width,
            height,
        } = p.geom;
        let _ = writeln!(
            s,
            "        place(ui, {}, egui::Id::new({}u64), {}.0, {}.0, {}.0, {}.0, |ui| {{",
            p.z.order_ident(),
            p.id,
            x,
            y,
            width,
            height
        );
        let _ = writeln!(s, "            {}", p.body);
        let _ = writeln!(s, "        }});");
    }
    let _ = writeln!(s, "    }}");
}

/// Emit the shared absolute-placement helper.
fn emit_place_helper(s: &mut String) {
    s.push_str(
        r#"/// Place `add` at an absolute MEDM position inside its own `egui::Area`. The
/// Area's `order` is the z-layer, so decoration (`Background`) renders and takes
/// input below controls (`Foreground`) regardless of call order.
#[allow(clippy::too_many_arguments)]
fn place(
    ui: &mut egui::Ui,
    order: egui::Order,
    id: egui::Id,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    add: impl FnOnce(&mut egui::Ui),
) {
    let origin = ui.max_rect().min;
    let rect = egui::Rect::from_min_size(origin + egui::vec2(x, y), egui::vec2(w, h));
    egui::Area::new(id)
        .order(order)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(ui.ctx(), |ui| {
            ui.set_clip_rect(rect);
            ui.set_max_size(rect.size());
            add(ui);
        });
}
"#,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adl_parser::parse;

    /// A screen with a static text decoration that OVERLAPS a text entry
    /// control, plus a text-update monitor — the overlap case the z-order rule
    /// exists for.
    const OVERLAP: &str = r#"
"color map" {
	colors {
		ffffff,
		000000,
	}
}
text {
	object {
		x=0
		y=0
		width=200
		height=100
	}
	"basic attribute" {
		clr=1
	}
	textix="Background label"
}
"text update" {
	object {
		x=10
		y=10
		width=80
		height=18
	}
	monitor {
		chan="$(P)rbv"
		clr=0
	}
	limits {
		precDefault=2
	}
}
"text entry" {
	object {
		x=10
		y=40
		width=120
		height=20
	}
	control {
		chan="$(P)set"
	}
}
"#;

    fn build(opts: &Options) -> Generated {
        generate(&parse(OVERLAP), opts)
    }

    #[test]
    fn emits_struct_new_ui_and_place_helper() {
        let g = build(&Options::default());
        assert!(g.source.contains("pub struct Screen {"));
        assert!(
            g.source
                .contains("pub fn new(cc: &eframe::CreationContext<'_>)")
        );
        assert!(g.source.contains("pub fn ui(&mut self, ui: &mut egui::Ui)"));
        assert!(g.source.contains("fn place("));
        assert!(g.source.contains("siplot::install(rs);"));
    }

    #[test]
    fn applies_protocol_and_macros_to_channels() {
        let opts = Options {
            protocol: "ca://".to_string(),
            macros: vec![("P".to_string(), "DMM1:".to_string())],
            ..Options::default()
        };
        let g = build(&opts);
        assert!(
            g.source
                .contains("SidmLineEdit::new(&engine, \"ca://DMM1:set\")"),
            "macro+protocol not applied:\n{}",
            g.source
        );
        assert!(
            g.source
                .contains("SidmLabel::new(&engine, \"ca://DMM1:rbv\")")
        );
        // precDefault -> with_precision.
        assert!(g.source.contains(".with_precision(2)"));
    }

    #[test]
    fn lays_out_decoration_before_control() {
        // The z-order guarantee: the Background (decoration) place() must appear
        // before the Foreground (control) place() in the source, and the static
        // label must use Background while the line edit uses Foreground.
        let g = build(&Options::default());
        let deco = g
            .source
            .find("egui::Order::Background")
            .expect("background");
        let ctrl = g
            .source
            .find("egui::Order::Foreground")
            .expect("foreground");
        assert!(
            deco < ctrl,
            "decoration must be laid out before the control:\n{}",
            g.source
        );
    }

    #[test]
    fn unimplemented_widgets_warn_but_do_not_panic() {
        let adl = r#"
"color map" {
	colors {
		ffffff,
	}
}
valuator {
	object {
		x=0
		y=0
		width=100
		height=20
	}
	control {
		chan="PV"
	}
}
"#;
        let g = generate(&parse(adl), &Options::default());
        assert!(g.warnings.iter().any(|w| w.contains("valuator")));
        // Nothing emitted for it yet, but the screen still assembles.
        assert!(g.source.contains("pub struct Screen"));
    }
}
