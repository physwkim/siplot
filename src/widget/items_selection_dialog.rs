//! A reusable widget to select a subset of plot items, grouped by kind.
//!
//! Ports silx `ItemsSelectionDialog.py`: a modal that lists a plot's items in a
//! table of (legend, kind) and lets the user pick a subset, with a side
//! `KindsSelector` that filters which item *kinds* are shown. silx splits the
//! work across `KindsSelector` (a multi-select list of item kinds, all selected
//! by default) and `PlotItemsSelector` (the legend/kind table whose rows are
//! selectable, filtered by the active kinds); `ItemsSelectionDialog` wires them
//! together and exposes `getSelectedItems`.
//!
//! This port is a standalone, GPU-free widget over a caller-owned slice of
//! item entries `(label, PlotItemKind, selected)` — it does *not* reach into a
//! live `Plot2D`/`PlotWidget`, so it composes with any item list (the
//! high-level `examples/high_level_items_selector.rs` builds an ad-hoc flat
//! checkbox list; this widget adds the silx grouping-by-kind and kind filter it
//! lacks). All selection / filtering / grouping logic lives in pure methods so
//! it is unit-testable without an egui context; [`ItemsSelectionDialog::ui`]
//! renders them.
//!
//! The fit tool reuses this dialog to pick which item to fit when more than one
//! curve exists (silx `actions/fit.py:_initFit`): it calls
//! [`set_available_kinds`](ItemsSelectionDialog::set_available_kinds) with
//! `[Curve, Histogram]` and
//! [`set_selection_mode`](ItemsSelectionDialog::set_selection_mode)`(Single)`,
//! then reads the one chosen entry from
//! [`selected_items`](ItemsSelectionDialog::selected_items) — see
//! `examples/high_level_fit_widget.rs`.

use crate::widget::high_level::PlotItemKind;

/// One selectable item entry: a display label, its [`PlotItemKind`], and
/// whether it is currently selected (silx `PlotItemsSelector` row: legend +
/// kind + row-selection state).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectableItem {
    /// Display label / legend shown in the row (silx `item.getName()`).
    pub label: String,
    /// The item family this entry belongs to (silx item kind string).
    pub kind: PlotItemKind,
    /// Whether the row is currently selected (silx row selection state).
    pub selected: bool,
}

impl SelectableItem {
    /// Create an item entry with the given label, kind, and initial selection.
    pub fn new(label: impl Into<String>, kind: PlotItemKind, selected: bool) -> Self {
        Self {
            label: label.into(),
            kind,
            selected,
        }
    }
}

/// How many rows may be selected at once (silx `setItemsSelectionMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SelectionMode {
    /// Any number of rows may be selected (silx `MultiSelection`). The default,
    /// used by the items-selector panel.
    #[default]
    Multi,
    /// At most one row is selected; selecting another replaces it (silx
    /// `SingleSelection`), as the fit tool uses to pick one curve/histogram
    /// (`actions/fit.py:237-241`).
    Single,
}

/// All item kinds, in the order silx lists them in `PlotWidget.ITEM_KINDS`
/// (curve, image, scatter, histogram, marker, …). Used to render the kind
/// filter and to group rows deterministically.
const KIND_ORDER: [PlotItemKind; 8] = [
    PlotItemKind::Curve,
    PlotItemKind::Image,
    PlotItemKind::Scatter,
    PlotItemKind::Histogram,
    PlotItemKind::Marker,
    PlotItemKind::Mask,
    PlotItemKind::Triangles,
    PlotItemKind::Shape,
];

/// The distinct kinds present in `items`, in [`KIND_ORDER`] order (the initial
/// shown-kinds set: silx `KindsSelector.selectAll()` over the plot's kinds).
fn distinct_kinds(items: &[SelectableItem]) -> Vec<PlotItemKind> {
    KIND_ORDER
        .into_iter()
        .filter(|k| items.iter().any(|it| it.kind == *k))
        .collect()
}

/// A widget to select a subset of plot items, grouped by kind, with a per-kind
/// filter (silx `ItemsSelectionDialog`).
///
/// Holds the caller-supplied item entries plus the set of kinds currently shown
/// (the `KindsSelector` state; every kind present in the entries is shown by
/// default, matching silx's `selectAll()` on construction). Render with
/// [`Self::ui`]; read the chosen subset with [`Self::selected_items`].
///
/// ```ignore
/// let mut dialog = ItemsSelectionDialog::new(vec![
///     SelectableItem::new("A curve", PlotItemKind::Curve, true),
///     SelectableItem::new("An image", PlotItemKind::Image, false),
/// ]);
///
/// // frame loop
/// dialog.ui(ui);
/// let chosen: Vec<&str> = dialog.selected_items().map(|it| it.label.as_str()).collect();
/// ```
pub struct ItemsSelectionDialog {
    items: Vec<SelectableItem>,
    /// Kinds currently shown by the kind filter (silx `KindsSelector`
    /// selection). A kind absent from this list hides its rows. Held as a
    /// `Vec` (deduplicated, [`KIND_ORDER`]-ordered) because [`PlotItemKind`]
    /// implements neither `Ord` nor `Hash`.
    shown_kinds: Vec<PlotItemKind>,
    /// The kinds the filter offers (silx `KindsSelector` available kinds). When
    /// `None` they are derived from the items present; when `Some` (set via
    /// [`Self::set_available_kinds`]) they are exactly the caller's list — silx
    /// `setAvailableKinds`, which the fit tool calls to offer only curve +
    /// histogram.
    available_override: Option<Vec<PlotItemKind>>,
    /// How many rows may be selected at once (silx `setItemsSelectionMode`).
    mode: SelectionMode,
}

/// Deduplicate `kinds` into [`KIND_ORDER`] order (so the offered/shown kind
/// lists stay canonical regardless of caller order or repeats).
fn in_kind_order(kinds: &[PlotItemKind]) -> Vec<PlotItemKind> {
    KIND_ORDER
        .into_iter()
        .filter(|k| kinds.contains(k))
        .collect()
}

impl ItemsSelectionDialog {
    /// Create a dialog over `items`, with every kind that appears in `items`
    /// shown by default (silx `KindsSelector.selectAll()` on construction) and
    /// multi-selection mode.
    pub fn new(items: Vec<SelectableItem>) -> Self {
        let shown_kinds = distinct_kinds(&items);
        Self {
            items,
            shown_kinds,
            available_override: None,
            mode: SelectionMode::Multi,
        }
    }

    /// Replace the item entries, re-showing every kind present in the new list
    /// (mirrors silx rebuilding the selector when the plot's items change). Any
    /// [`set_available_kinds`](Self::set_available_kinds) override is cleared, as
    /// silx repopulates the kind selector from the new plot items; the selection
    /// mode is preserved.
    pub fn set_items(&mut self, items: Vec<SelectableItem>) {
        self.shown_kinds = distinct_kinds(&items);
        self.available_override = None;
        self.items = items;
        // The new entries carry their own selection flags; enforce the mode so a
        // Single-mode dialog never holds two selected rows.
        self.enforce_single_selection();
    }

    /// In [`SelectionMode::Single`], keep only the first selected row (clearing
    /// the rest); a no-op in [`SelectionMode::Multi`]. The one place the "≤ 1
    /// selected" invariant is repaired after a bulk selection change.
    fn enforce_single_selection(&mut self) {
        if self.mode != SelectionMode::Single {
            return;
        }
        let mut kept = false;
        for it in &mut self.items {
            if it.selected {
                if kept {
                    it.selected = false;
                } else {
                    kept = true;
                }
            }
        }
    }

    /// All item entries (selected or not), in insertion order.
    pub fn items(&self) -> &[SelectableItem] {
        &self.items
    }

    /// The kinds the filter offers, in [`KIND_ORDER`] order: the
    /// [`set_available_kinds`](Self::set_available_kinds) override if set, else
    /// the distinct kinds present in the entries (silx `KindsSelector` available
    /// kinds — derived from the plot's items, or replaced by `setAvailableKinds`).
    pub fn available_kinds(&self) -> Vec<PlotItemKind> {
        match &self.available_override {
            Some(kinds) => kinds.clone(),
            None => distinct_kinds(&self.items),
        }
    }

    /// Restrict the offered kinds to exactly `kinds` and show them all (silx
    /// `setAvailableKinds` + `selectAllKinds`). The fit tool calls this with
    /// `[Curve, Histogram]` so only fittable items are listed
    /// (`actions/fit.py:240`). Rows of any other kind are hidden.
    pub fn set_available_kinds(&mut self, kinds: &[PlotItemKind]) {
        let kinds = in_kind_order(kinds);
        self.shown_kinds = kinds.clone();
        self.available_override = Some(kinds);
    }

    /// The current selection mode (single vs multi).
    pub fn selection_mode(&self) -> SelectionMode {
        self.mode
    }

    /// Set the selection mode (silx `setItemsSelectionMode`). Switching to
    /// [`SelectionMode::Single`] collapses any existing multi-selection to at
    /// most the first selected row, so the "≤ 1 selected" invariant holds
    /// immediately, not just after the next click.
    pub fn set_selection_mode(&mut self, mode: SelectionMode) {
        self.mode = mode;
        self.enforce_single_selection();
    }

    /// Whether `kind` is currently shown by the kind filter.
    pub fn is_kind_shown(&self, kind: PlotItemKind) -> bool {
        self.shown_kinds.contains(&kind)
    }

    /// Show or hide all rows of `kind` via the kind filter (silx
    /// `KindsSelector` selecting / deselecting a kind). Hiding a kind does not
    /// change the per-item selection state — silx's filter only governs
    /// visibility — so re-showing it restores the prior selection.
    pub fn set_kind_shown(&mut self, kind: PlotItemKind, shown: bool) {
        let present = self.shown_kinds.iter().position(|k| *k == kind);
        match (shown, present) {
            (true, None) => self.shown_kinds.push(kind),
            (false, Some(i)) => {
                self.shown_kinds.remove(i);
            }
            _ => {}
        }
    }

    /// Set the selection state of the item at `index` (out-of-range is a
    /// no-op). Mirrors silx row selection in `PlotItemsSelector`. In
    /// [`SelectionMode::Single`], selecting a row clears every other selection
    /// first so at most one row is ever selected — this is the single owner of
    /// the "≤ 1 selected" invariant, so [`toggle_selected`](Self::toggle_selected)
    /// routes through it.
    pub fn set_selected(&mut self, index: usize, selected: bool) {
        if index >= self.items.len() {
            return;
        }
        if selected && self.mode == SelectionMode::Single {
            for item in &mut self.items {
                item.selected = false;
            }
        }
        self.items[index].selected = selected;
    }

    /// Toggle the selection state of the item at `index` (out-of-range is a
    /// no-op). Routed through [`set_selected`](Self::set_selected) so the
    /// single-selection invariant is enforced uniformly.
    pub fn toggle_selected(&mut self, index: usize) {
        if let Some(current) = self.items.get(index).map(|it| it.selected) {
            self.set_selected(index, !current);
        }
    }

    /// The chosen subset: every selected entry **whose kind is currently
    /// shown** by the filter (silx `getSelectedItems` reads the rows of the
    /// filtered table, so a selection hidden by the kind filter is not
    /// returned).
    pub fn selected_items(&self) -> impl Iterator<Item = &SelectableItem> {
        self.items
            .iter()
            .filter(|it| it.selected && self.shown_kinds.contains(&it.kind))
    }

    /// The indices, into [`Self::items`], of the rows that are visible under
    /// the current kind filter, grouped by kind in [`KIND_ORDER`] order. Used
    /// by [`Self::ui`] to render grouped sections; exposed for testing the
    /// grouping/filtering logic without an egui context.
    pub fn visible_groups(&self) -> Vec<(PlotItemKind, Vec<usize>)> {
        let mut groups = Vec::new();
        for kind in KIND_ORDER {
            if !self.shown_kinds.contains(&kind) {
                continue;
            }
            let indices: Vec<usize> = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, it)| it.kind == kind)
                .map(|(i, _)| i)
                .collect();
            if !indices.is_empty() {
                groups.push((kind, indices));
            }
        }
        groups
    }

    /// Render the dialog body: a row of per-kind filter checkboxes (silx
    /// `KindsSelector`) followed by the shown items grouped by kind, each a
    /// selectable checkbox row (silx `PlotItemsSelector`).
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.label("Filter item kinds:");
        ui.horizontal_wrapped(|ui| {
            for kind in self.available_kinds() {
                let mut shown = self.is_kind_shown(kind);
                if ui.checkbox(&mut shown, kind.as_str()).changed() {
                    self.set_kind_shown(kind, shown);
                }
            }
        });

        ui.separator();
        ui.label("Select items:");

        let groups = self.visible_groups();
        if groups.is_empty() {
            ui.weak("No items");
            return;
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (kind, indices) in groups {
                ui.strong(kind.as_str());
                for index in indices {
                    let mut selected = self.items[index].selected;
                    let label = self.items[index].label.clone();
                    if ui.checkbox(&mut selected, label).changed() {
                        self.set_selected(index, selected);
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ItemsSelectionDialog {
        ItemsSelectionDialog::new(vec![
            SelectableItem::new("curve A", PlotItemKind::Curve, true),
            SelectableItem::new("curve B", PlotItemKind::Curve, false),
            SelectableItem::new("image A", PlotItemKind::Image, true),
            SelectableItem::new("scatter A", PlotItemKind::Scatter, false),
        ])
    }

    #[test]
    fn all_kinds_shown_by_default() {
        let d = sample();
        assert!(d.is_kind_shown(PlotItemKind::Curve));
        assert!(d.is_kind_shown(PlotItemKind::Image));
        assert!(d.is_kind_shown(PlotItemKind::Scatter));
        // A kind absent from the entries is not shown (nothing to show).
        assert!(!d.is_kind_shown(PlotItemKind::Marker));
    }

    #[test]
    fn available_kinds_are_distinct_in_kind_order() {
        // Curve appears twice in the entries but once in available_kinds, in
        // KIND_ORDER order (Curve before Image before Scatter).
        let d = sample();
        assert_eq!(
            d.available_kinds(),
            vec![
                PlotItemKind::Curve,
                PlotItemKind::Image,
                PlotItemKind::Scatter
            ]
        );
    }

    #[test]
    fn selected_items_returns_only_selected_and_shown() {
        let d = sample();
        let labels: Vec<&str> = d.selected_items().map(|it| it.label.as_str()).collect();
        // curve A and image A are selected; curve B and scatter A are not.
        assert_eq!(labels, vec!["curve A", "image A"]);
    }

    #[test]
    fn toggling_selection_changes_the_returned_subset() {
        let mut d = sample();
        // Select curve B (index 1); deselect image A (index 2).
        d.toggle_selected(1);
        d.set_selected(2, false);
        let labels: Vec<&str> = d.selected_items().map(|it| it.label.as_str()).collect();
        assert_eq!(labels, vec!["curve A", "curve B"]);
    }

    #[test]
    fn set_and_toggle_out_of_range_are_noops() {
        let mut d = sample();
        d.set_selected(99, true); // no panic, no change.
        d.toggle_selected(99);
        let labels: Vec<&str> = d.selected_items().map(|it| it.label.as_str()).collect();
        assert_eq!(labels, vec!["curve A", "image A"]);
    }

    #[test]
    fn hiding_a_kind_drops_its_items_from_the_subset_without_clearing_selection() {
        let mut d = sample();
        // Hide curves: curve A is selected but now filtered out of the subset.
        d.set_kind_shown(PlotItemKind::Curve, false);
        let labels: Vec<&str> = d.selected_items().map(|it| it.label.as_str()).collect();
        assert_eq!(labels, vec!["image A"]);
        // Per-item selection is untouched: re-showing curves restores curve A.
        d.set_kind_shown(PlotItemKind::Curve, true);
        let labels: Vec<&str> = d.selected_items().map(|it| it.label.as_str()).collect();
        assert_eq!(labels, vec!["curve A", "image A"]);
    }

    #[test]
    fn visible_groups_filters_and_groups_in_kind_order() {
        let mut d = sample();
        // All kinds shown: three groups (Curve with two rows, Image, Scatter).
        let groups = d.visible_groups();
        assert_eq!(
            groups,
            vec![
                (PlotItemKind::Curve, vec![0, 1]),
                (PlotItemKind::Image, vec![2]),
                (PlotItemKind::Scatter, vec![3]),
            ]
        );

        // Hide Image: it disappears from the groups, others keep their indices.
        d.set_kind_shown(PlotItemKind::Image, false);
        let groups = d.visible_groups();
        assert_eq!(
            groups,
            vec![
                (PlotItemKind::Curve, vec![0, 1]),
                (PlotItemKind::Scatter, vec![3]),
            ]
        );
    }

    #[test]
    fn single_selection_mode_keeps_at_most_one_selected() {
        // Fit-tool config: single selection. Selecting a second row replaces the
        // first (silx SingleSelection).
        let mut d = sample();
        d.set_selection_mode(SelectionMode::Single);
        // sample() had curve A and image A selected; switching to Single keeps
        // only the first selected row (curve A).
        let labels: Vec<&str> = d.selected_items().map(|it| it.label.as_str()).collect();
        assert_eq!(labels, vec!["curve A"]);

        // Selecting curve B replaces curve A.
        d.set_selected(1, true);
        let labels: Vec<&str> = d.selected_items().map(|it| it.label.as_str()).collect();
        assert_eq!(labels, vec!["curve B"]);

        // Toggling image A (index 2) replaces curve B (routed through set_selected).
        d.toggle_selected(2);
        let labels: Vec<&str> = d.selected_items().map(|it| it.label.as_str()).collect();
        assert_eq!(labels, vec!["image A"]);
    }

    #[test]
    fn set_available_kinds_restricts_offered_and_shown_kinds() {
        // Fit-tool config: only curve + histogram offered (silx setAvailableKinds).
        let mut d = sample();
        d.set_available_kinds(&[PlotItemKind::Histogram, PlotItemKind::Curve]);
        // Offered kinds are exactly the requested set, in KIND_ORDER (Curve first).
        assert_eq!(
            d.available_kinds(),
            vec![PlotItemKind::Curve, PlotItemKind::Histogram]
        );
        // Image is no longer shown, so image A drops out of the subset even
        // though it stays selected internally.
        assert!(!d.is_kind_shown(PlotItemKind::Image));
        let labels: Vec<&str> = d.selected_items().map(|it| it.label.as_str()).collect();
        assert_eq!(labels, vec!["curve A"]);
    }

    #[test]
    fn fit_tool_config_picks_exactly_one_fittable_item() {
        // Mirror silx fit.py:_initFit: restrict to curve/histogram, single-select.
        let mut d = ItemsSelectionDialog::new(vec![
            SelectableItem::new("curve A", PlotItemKind::Curve, false),
            SelectableItem::new("curve B", PlotItemKind::Curve, false),
            SelectableItem::new("image A", PlotItemKind::Image, false),
        ]);
        d.set_available_kinds(&[PlotItemKind::Curve, PlotItemKind::Histogram]);
        d.set_selection_mode(SelectionMode::Single);

        // User picks curve B.
        d.set_selected(1, true);
        let chosen: Vec<&str> = d.selected_items().map(|it| it.label.as_str()).collect();
        assert_eq!(chosen, vec!["curve B"]);
        // The image is not offered, so it can never be the fitted item.
        assert!(!d.available_kinds().contains(&PlotItemKind::Image));
    }

    #[test]
    fn set_items_clears_available_override_and_keeps_mode() {
        let mut d = sample();
        d.set_available_kinds(&[PlotItemKind::Curve]);
        d.set_selection_mode(SelectionMode::Single);
        d.set_items(vec![
            SelectableItem::new("img", PlotItemKind::Image, true),
            SelectableItem::new("cur", PlotItemKind::Curve, true),
        ]);
        // Override cleared: every new kind is offered again.
        assert_eq!(
            d.available_kinds(),
            vec![PlotItemKind::Curve, PlotItemKind::Image]
        );
        // Mode preserved: selecting a new row still replaces the prior one.
        d.set_selected(0, true);
        let labels: Vec<&str> = d.selected_items().map(|it| it.label.as_str()).collect();
        assert_eq!(labels, vec!["img"]);
    }

    #[test]
    fn set_items_reshows_every_new_kind() {
        let mut d = sample();
        d.set_kind_shown(PlotItemKind::Curve, false);
        d.set_items(vec![
            SelectableItem::new("m", PlotItemKind::Marker, true),
            SelectableItem::new("c", PlotItemKind::Curve, true),
        ]);
        // Both new kinds are shown again after set_items.
        assert!(d.is_kind_shown(PlotItemKind::Marker));
        assert!(d.is_kind_shown(PlotItemKind::Curve));
        let labels: Vec<&str> = d.selected_items().map(|it| it.label.as_str()).collect();
        assert_eq!(labels, vec!["m", "c"]);
    }
}
