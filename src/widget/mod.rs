//! egui widgets.
//!
//! [`plot_widget::PlotWidget`] hosts a plot inside an egui `Ui`: it draws the
//! chrome, handles interaction, and registers the wgpu paint callback for the
//! data area. Chrome drawing and interaction land in later milestones
//! (`doc/design.md` §8).

pub mod chrome;
pub mod plot_widget;
