//! `ImageStack` lazy/threaded loading (silx `ImageStack` `setUrls` →
//! `UrlLoader` background thread → `_urlLoaded`).
//!
//! The pure dispatch/dedup bookkeeping (`LoadSchedule`) is unit-tested inside
//! the module; this drives the widget end-to-end the way a host would: install
//! a [`FrameLoader`], `set_sources`, render frames, and assert that the current
//! slot — empty (waiting overlay) at first — is filled by a background load and
//! becomes displayable, without the host ever handing over pixel data. Building
//! an `ImageStack` needs a wgpu render state and a real frame, so this runs
//! through egui_kittest.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup};
use siplot::egui;
use siplot::{Frame, FrameLoader, ImageStack};

/// Synthesise a `width×height` ramp frame from a `"WxH"` source string, or
/// `None` if the source does not parse (the loader's failure path).
fn synth_frame(source: &str) -> Option<Frame> {
    let (w, h) = source.split_once('x')?;
    let w: u32 = w.parse().ok()?;
    let h: u32 = h.parse().ok()?;
    let data = (0..(w * h)).map(|i| i as f32).collect();
    Some(Frame::new(w, h, data, Some(source.to_string())))
}

/// A loader that counts its invocations, so a test can assert which slots were
/// actually loaded (and that a failed slot is not retried).
struct CountingLoader {
    calls: AtomicUsize,
}

impl CountingLoader {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl FrameLoader for CountingLoader {
    fn load(&self, source: &str) -> Option<Frame> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        synth_frame(source)
    }
}

/// A loader that blocks inside `load` until the test releases it, so the
/// in-flight (waiting-overlay) state can be observed deterministically rather
/// than racing a fast background load.
struct GatedLoader {
    gate: Arc<(Mutex<bool>, Condvar)>,
}

impl GatedLoader {
    fn new() -> (Self, Arc<(Mutex<bool>, Condvar)>) {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        (Self { gate: gate.clone() }, gate)
    }
}

impl FrameLoader for GatedLoader {
    fn load(&self, source: &str) -> Option<Frame> {
        let (lock, cv) = &*self.gate;
        let mut released = lock.lock().unwrap();
        while !*released {
            released = cv.wait(released).unwrap();
        }
        synth_frame(source)
    }
}

/// Release a [`GatedLoader`]'s gate so its blocked `load` can finish.
fn release(gate: &Arc<(Mutex<bool>, Condvar)>) {
    let (lock, cv) = &**gate;
    *lock.lock().unwrap() = true;
    cv.notify_all();
}

/// Build a harness over an `ImageStack` with `loader` installed and `sources`
/// set.
fn harness_lazy(
    loader: Arc<dyn FrameLoader>,
    sources: Vec<String>,
) -> (Rc<RefCell<ImageStack>>, Harness<'static>) {
    let rs = create_render_state(default_wgpu_setup());
    siplot::install(&rs);

    let mut stack = ImageStack::new(&rs, 0);
    stack.set_loader(loader);
    stack.set_sources(sources);

    let app = Rc::new(RefCell::new(stack));
    let app_ui = app.clone();
    let renderer = WgpuTestRenderer::from_render_state(rs.clone());
    let harness = Harness::builder()
        .with_size(egui::vec2(400.0, 400.0))
        .with_pixels_per_point(1.0)
        .renderer(renderer)
        .build_ui(move |ui| {
            app_ui.borrow_mut().ui(ui);
        });
    (app, harness)
}

/// Step the harness until `pred` holds or a deadline passes, sleeping briefly
/// between frames so the background load thread can finish and its result be
/// drained on the next `ui` pass.
fn step_until(
    harness: &mut Harness<'static>,
    app: &RefCell<ImageStack>,
    pred: impl Fn(&ImageStack) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        harness.step();
        if pred(&app.borrow()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn lazy_load_fills_the_current_slot_in_the_background() {
    let (loader, gate) = GatedLoader::new();
    let (app, mut harness) = harness_lazy(
        Arc::new(loader),
        vec!["4x4".to_string(), "4x4".to_string(), "4x4".to_string()],
    );

    // The load is dispatched but blocked: the current slot stays empty (waiting
    // overlay), proving the host handed over no pixels.
    harness.step();
    assert!(
        !app.borrow().current_is_displayable(),
        "an in-flight slot must stay empty (waiting overlay)"
    );

    // Release the load: the background result fills the slot and it becomes
    // displayable.
    release(&gate);
    assert!(
        step_until(&mut harness, &app, |s| s.current_is_displayable()),
        "current slot was never filled by the background loader"
    );
}

#[test]
fn lazy_load_failure_is_terminal_and_not_retried() {
    let loader = Arc::new(CountingLoader::new());
    // "bad" is unparsable -> the loader returns None -> the slot fails.
    let (app, mut harness) = harness_lazy(loader.clone(), vec!["bad".to_string()]);

    // Step until the failing load has run (and been drained on the next pass).
    let counting = loader.clone();
    assert!(
        step_until(&mut harness, &app, move |_| counting.call_count() >= 1),
        "the failing load never ran"
    );
    harness.step(); // drain the None result -> slot marked failed.

    // A failed slot stays non-displayable (overlay), and is not re-dispatched:
    // further frames do not invoke the loader again.
    assert!(!app.borrow().current_is_displayable());
    let after_first = loader.call_count();
    assert!(after_first >= 1, "the failing load should have run once");
    for _ in 0..5 {
        harness.step();
    }
    assert_eq!(
        loader.call_count(),
        after_first,
        "a failed slot must not be retried"
    );
}

#[test]
fn navigating_to_a_new_slot_loads_it() {
    let loader = Arc::new(CountingLoader::new());
    let (app, mut harness) =
        harness_lazy(loader.clone(), vec!["4x4".to_string(), "6x6".to_string()]);
    assert!(step_until(&mut harness, &app, |s| s.current_is_displayable()));

    // Browse to slot 1: it starts empty, then its own background load fills it.
    app.borrow_mut().next_frame();
    harness.step();
    assert_eq!(app.borrow().current(), 1);
    assert!(
        step_until(&mut harness, &app, |s| s.current() == 1
            && s.current_is_displayable()),
        "the newly-browsed slot was never loaded"
    );
}
