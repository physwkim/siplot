//! Plot action behaviors, mirroring silx `silx.gui.plot.actions`.
//!
//! Each silx action is a `QAction` subclass with checkable/triggered state and
//! a Qt signal hierarchy. egui is immediate-mode, so an action here is modeled
//! as a plain function that takes `&mut PlotWidget` (or the relevant view) and
//! performs the state transition once. There is no retained `QAction`, no
//! `checkable`/`triggered` machinery, and no signal plumbing — the caller (the
//! toolbar in [`crate::widget::high_level`]) calls the function on click and
//! reads the resulting state directly.
//!
//! - [`control`] mirrors silx `actions/control.py` (axis display, colorbar,
//!   curve style, zoom in/out/back).
//! - [`io`] mirrors silx `actions/io.py` (save figure/data, copy to clipboard).
//! - [`mode`] mirrors silx `actions/mode.py` (zoom / pan interaction modes),
//!   plus the port-specific mask-draw and select mode setters.
//!
//! Load-bearing logic (style cycling, zoom-range math, format detection, CSV
//! serialization, RGBA→clipboard shaping) is factored into pure functions so it
//! is unit-testable without a GPU backend; the native file-dialog, GPU
//! readback, and clipboard calls are thin untestable shims around that logic.

pub mod control;
pub mod io;
pub mod mode;
