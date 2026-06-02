//! Plot I/O actions, mirroring silx `silx.gui.plot.actions.io`.
//!
//! The figure-save (PNG), data-save (CSV), and clipboard-copy behaviors land in
//! later items; the load-bearing logic (format detection, CSV serialization,
//! RGBA→clipboard shaping) is factored into pure functions here, with the native
//! file dialog, GPU readback, and clipboard calls as thin untestable shims.
