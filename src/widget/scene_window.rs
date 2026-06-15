//! [`SceneWindow`] — a composed 3D scene view: toolbar + scene + properties.
//!
//! Port of silx `plot3d.SceneWindow.SceneWindow`, which is a `QMainWindow`
//! composing a `SceneWidget` (central) with a viewpoint toolbar, an interactive
//! mode toolbar, a `GroupPropertiesWidget` dock, a `ParamTreeView` dock, and a
//! `PositionInfoWidget`. The siplot analogue composes the parts that are ported:
//!
//! - the [`viewpoint_menu`] drop-down (silx `ViewpointToolBar`) in a toolbar row,
//! - a [`ScalarFieldView`] as the central scene,
//! - a [`ScalarFieldProperties`] panel (silx `GroupPropertiesWidget`) in a
//!   toggleable side column.
//!
//! Not composed (deferred upstream, documented in the roadmap): the
//! `PositionInfoWidget` (needs 3D picking) and the generic `ParamTreeView`
//! (`plot3d._model`).
//!
//! Like the other plot3d widgets, geometry is uploaded eagerly when the data
//! layer changes; `ui` only lays the parts out and paints.

use egui::{Response, Ui};
use egui_wgpu::RenderState;

use crate::render::gpu_scene3d::Scene3dId;
use crate::widget::scalar_field_properties::ScalarFieldProperties;
use crate::widget::scalar_field_view::ScalarFieldView;
use crate::widget::scene_widget::viewpoint_menu;

/// Default width (points) of the properties side column.
const PROPERTIES_WIDTH: f32 = 200.0;

/// A composed 3D scalar-field window: a viewpoint toolbar above a
/// [`ScalarFieldView`], with a toggleable [`ScalarFieldProperties`] side panel.
/// Construct with [`SceneWindow::new`], push data through
/// [`view_mut`](SceneWindow::view_mut), then call [`show`](SceneWindow::show)
/// each frame.
pub struct SceneWindow {
    view: ScalarFieldView,
    properties: ScalarFieldProperties,
    /// Whether the properties side panel is shown (silx tabs the GroupProperties
    /// dock; here it is a toggle).
    show_properties: bool,
}

impl SceneWindow {
    /// Create a scene window bound to `id`, installing the 3D GPU resources into
    /// `render_state` if needed. Starts empty with the properties panel shown.
    pub fn new(render_state: &RenderState, id: Scene3dId) -> Self {
        Self {
            view: ScalarFieldView::new(render_state, id),
            properties: ScalarFieldProperties::new(),
            show_properties: true,
        }
    }

    /// Read-only access to the central scalar-field view.
    pub fn view(&self) -> &ScalarFieldView {
        &self.view
    }

    /// Mutable access to the central scalar-field view (e.g. to set its data or
    /// iso-surfaces).
    pub fn view_mut(&mut self) -> &mut ScalarFieldView {
        &mut self.view
    }

    /// Mutable access to the properties panel state.
    pub fn properties_mut(&mut self) -> &mut ScalarFieldProperties {
        &mut self.properties
    }

    /// Whether the properties side panel is shown.
    pub fn properties_visible(&self) -> bool {
        self.show_properties
    }

    /// Show or hide the properties side panel.
    pub fn set_properties_visible(&mut self, visible: bool) {
        self.show_properties = visible;
    }

    /// Lay out the toolbar, optional properties column, and scene, handling
    /// interaction and painting. Returns the egui [`Response`] of the scene rect.
    pub fn show(&mut self, ui: &mut Ui, render_state: &RenderState) -> Response {
        // Toolbar (top): viewpoint presets + a properties-panel toggle.
        egui::Panel::top(ui.id().with("scene_window_toolbar")).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                viewpoint_menu(ui, self.view.scene_mut());
                ui.checkbox(&mut self.show_properties, "Properties");
            });
        });

        // Properties (left), when shown.
        if self.show_properties {
            egui::Panel::left(ui.id().with("scene_window_properties"))
                .default_size(PROPERTIES_WIDTH)
                .show_inside(ui, |ui| {
                    self.properties.ui(ui, &mut self.view, render_state);
                });
        }

        // Scene fills the rest.
        egui::CentralPanel::default()
            .show_inside(ui, |ui| self.view.show(ui))
            .inner
    }
}
