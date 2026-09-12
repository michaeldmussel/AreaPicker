mod human_mouse;

use eframe::{egui, egui::{Color32, Pos2, Rect, Sense, WindowLevel}};
use enigo::{MouseControllable, MouseButton};
use parking_lot::Mutex;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use std::time::Duration;
use rand::Rng;
use crate::human_mouse::{Bounds, HumanMouseSettings, human_move_and_click};

use clap::Parser;

/// Grab a screenshot of the physical-pixel rectangle `(origin_px, size_px)` (spanning
/// one or more monitors) as an egui-displayable image, so the picker overlay can show
/// "what's really on screen" instead of relying on real window transparency.
///
/// Real compositor transparency (`ViewportCommand::Transparent`) is unreliable once a
/// window already exists — on Windows in particular it commonly stays opaque, which is
/// the "no seethrough" bug this works around. Painting a frozen screenshot behind the
/// dimming scrim looks the same to the user and works regardless of compositor support.
///
/// Returns `None` if screen capture isn't available (non-Windows builds here, since
/// xcap's Linux backend needs extra system libraries not guaranteed to be present) or
/// fails at runtime — callers must fall back to a plain dim scrim with no background
/// image, which is the explicit fallback behavior requested.
#[cfg(windows)]
fn capture_screenshot(origin_px: (i32, i32), size_px: (i32, i32)) -> Option<egui::ColorImage> {
    if size_px.0 <= 0 || size_px.1 <= 0 {
        return None;
    }

    let monitors = xcap::Monitor::all().ok()?;

    // Single-monitor fast path: capture directly, no compositing needed.
    if let Some(m) = monitors.iter().find(|m| {
        m.x().ok() == Some(origin_px.0)
            && m.y().ok() == Some(origin_px.1)
            && m.width().ok() == Some(size_px.0 as u32)
            && m.height().ok() == Some(size_px.1 as u32)
    }) {
        let img = m.capture_image().ok()?;
        return Some(egui::ColorImage::from_rgba_unmultiplied(
            [img.width() as usize, img.height() as usize],
            img.as_raw(),
        ));
    }

    // Multi-monitor (or otherwise non-matching) target: composite each monitor's
    // capture into one buffer sized to the requested union rectangle.
    let mut buf = vec![0u8; (size_px.0 as usize) * (size_px.1 as usize) * 4];
    let mut got_any = false;
    for m in &monitors {
        let (mx, my, mw, mh) = match (m.x(), m.y(), m.width(), m.height()) {
            (Ok(x), Ok(y), Ok(w), Ok(h)) => (x, y, w, h),
            _ => continue,
        };
        let Ok(img) = m.capture_image() else { continue };
        got_any = true;

        let dst_x0 = mx - origin_px.0;
        let dst_y0 = my - origin_px.1;
        for y in 0..mh as i32 {
            let dy = dst_y0 + y;
            if dy < 0 || dy >= size_px.1 {
                continue;
            }
            for x in 0..mw as i32 {
                let dx = dst_x0 + x;
                if dx < 0 || dx >= size_px.0 {
                    continue;
                }
                let src_px = img.get_pixel(x as u32, y as u32);
                let dst_idx = ((dy as usize) * (size_px.0 as usize) + dx as usize) * 4;
                buf[dst_idx..dst_idx + 4].copy_from_slice(&src_px.0);
            }
        }
    }

    if !got_any {
        return None;
    }
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [size_px.0 as usize, size_px.1 as usize],
        &buf,
    ))
}

#[cfg(not(windows))]
fn capture_screenshot(_origin_px: (i32, i32), _size_px: (i32, i32)) -> Option<egui::ColorImage> {
    // No screen-capture backend wired up for this platform; the picker falls back to a
    // plain dim scrim with box-selection instead, per the requested fallback behavior.
    None
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Total number of random clicks to perform before stopping (0 = infinite)
    #[arg(long = "clicks", default_value_t = 0)]
    clicks: u32,

    /// Optional min delay between clicks in ms
    #[arg(long = "min-delay-ms", default_value_t = 75)]
    min_delay_ms: u64,

    /// Optional max delay between clicks in ms
    #[arg(long = "max-delay-ms", default_value_t = 250)]
    max_delay_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ClickButton { Left, Right }

impl From<ClickButton> for MouseButton {
    fn from(b: ClickButton) -> Self {
        match b {
            ClickButton::Left => MouseButton::Left,
            ClickButton::Right => MouseButton::Right,
        }
    }
}

struct ClickJob {
    running: Arc<AtomicBool>,
    #[allow(dead_code)] // Kept alive via Arc clone in spawn; used by UI to read live status
    config: Arc<Mutex<ClickConfig>>,
}

#[derive(Clone, Debug)]
struct SequenceAction {
    bounds: Bounds,
    button: ClickButton,
    min_secs: f32,
    max_secs: f32,
    clicks_per_cycle: u32, // Number of clicks to perform in this region per sequence cycle
}

#[derive(Clone, Debug)]
struct ClickConfig {
    sequence_mode: bool,           // true if running a sequence, false for single region
    sequence: Vec<SequenceAction>, // Actions to perform in order
    sequence_cycles: Option<u32>,  // None for infinite, Some(n) for n cycles
    current_action: usize,         // Index of current action in sequence

    // Legacy single-region config (used when sequence_mode is false)
    bounds: Option<Bounds>,
    button: ClickButton,
    min_secs: f32,
    max_secs: f32,
    finite_clicks: Option<u32>, // None for infinite, Some(n) for n clicks
}

impl Default for ClickConfig {
    fn default() -> Self {
        Self {
            sequence_mode: false,
            sequence: Vec::new(),
            sequence_cycles: None,
            current_action: 0,
            bounds: None,
            button: ClickButton::Left,
            min_secs: 0.075,
            max_secs: 0.25,
            finite_clicks: None,
        }
    }
}

/// Perform one human-like click inside `bounds` using `button`, returning the point clicked.
fn do_random_click(
    enigo: &mut enigo::Enigo,
    bounds: &Bounds,
    button: ClickButton,
    last_pos: Option<(i32, i32)>,
    rng: &mut impl Rng,
) -> (i32, i32) {
    let x = rng.gen_range(bounds.min_x..=bounds.max_x);
    let y = rng.gen_range(bounds.min_y..=bounds.max_y);

    // Start "outside" the box on the very first move so the human-mouse module's
    // entry-glide logic has somewhere real to glide from.
    let from = last_pos.unwrap_or((bounds.min_x - 40, bounds.min_y - 40));

    let settings = HumanMouseSettings::default();
    human_move_and_click(enigo, from, (x, y), Some(*bounds), &settings, button.into());

    (x, y)
}

impl ClickJob {
    fn spawn(config: Arc<Mutex<ClickConfig>>) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);
        let config_clone = Arc::clone(&config);

        eprintln!("Starting click job with config: {:#?}", config.lock());

        let handle = std::thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let mut enigo = enigo::Enigo::new();
            let mut last_pos: Option<(i32, i32)> = None;

            // For sequence mode tracking
            let mut current_action_clicks: u32 = 0;
            let mut cycles_completed: u32 = 0;

            // For legacy single-region mode
            let mut clicks_remaining = config_clone.lock().finite_clicks;

            while running_clone.load(Ordering::Relaxed) {
                // Snapshot the mode/params we need, then release the lock before any
                // sleeping/clicking so the UI thread never blocks on us mid-click.
                let sequence_mode = config_clone.lock().sequence_mode;

                if sequence_mode {
                    // Pull out just what we need for this iteration.
                    let action = {
                        let cfg = config_clone.lock();
                        cfg.sequence.get(cfg.current_action).cloned()
                    };

                    let Some(action) = action else {
                        // Empty sequence: wait for the user to add actions.
                        std::thread::sleep(Duration::from_millis(200));
                        continue;
                    };

                    if !action.bounds.is_valid() {
                        std::thread::sleep(Duration::from_millis(200));
                        continue;
                    }

                    if current_action_clicks >= action.clicks_per_cycle {
                        current_action_clicks = 0;

                        let mut cfg = config_clone.lock();
                        let at_last_action = cfg.current_action + 1 >= cfg.sequence.len();
                        if at_last_action {
                            cycles_completed += 1;
                            if let Some(max_cycles) = cfg.sequence_cycles {
                                if cycles_completed >= max_cycles {
                                    break;
                                }
                            }
                            cfg.current_action = 0;
                        } else {
                            cfg.current_action += 1;
                        }
                        continue;
                    }

                    let (x, y) = do_random_click(&mut enigo, &action.bounds, action.button, last_pos, &mut rng);
                    last_pos = Some((x, y));
                    current_action_clicks += 1;

                    let min_ms = (action.min_secs.max(0.0) * 1000.0) as u64;
                    let max_ms = min_ms.max((action.max_secs.max(0.0) * 1000.0) as u64);
                    let delay = rng.gen_range(min_ms..=max_ms);
                    std::thread::sleep(Duration::from_millis(delay));
                } else {
                    // Legacy single-region mode
                    if let Some(clicks) = clicks_remaining {
                        if clicks == 0 {
                            break;
                        }
                    }

                    let (bounds, button, min_secs, max_secs) = {
                        let cfg = config_clone.lock();
                        (cfg.bounds, cfg.button, cfg.min_secs, cfg.max_secs)
                    };

                    let Some(bounds) = bounds else {
                        std::thread::sleep(Duration::from_millis(200));
                        continue;
                    };
                    if !bounds.is_valid() {
                        std::thread::sleep(Duration::from_millis(200));
                        continue;
                    }

                    let (x, y) = do_random_click(&mut enigo, &bounds, button, last_pos, &mut rng);
                    last_pos = Some((x, y));

                    if let Some(ref mut remaining) = clicks_remaining {
                        *remaining = remaining.saturating_sub(1);
                    }

                    let min_ms = (min_secs.max(0.0) * 1000.0) as u64;
                    let max_ms = min_ms.max((max_secs.max(0.0) * 1000.0) as u64);
                    let delay = rng.gen_range(min_ms..=max_ms);
                    std::thread::sleep(Duration::from_millis(delay));
                }
            }

            running_clone.store(false, Ordering::Relaxed);
        });

        let _ = handle; // Detach the thread; `running` is how we signal it to stop.
        Self { running, config }
    }

    fn stop(&self) { self.running.store(false, Ordering::Relaxed); }
}

// -------------- Display Info --------------
#[derive(Clone, Debug)]
struct Monitor {
    #[allow(dead_code)]
    id: u32,
    name: String,
    origin_px: (i32, i32),
    size_px: (i32, i32),
    #[allow(dead_code)]
    scale_factor: f32,
}

fn query_monitors() -> Vec<Monitor> {
    match display_info::DisplayInfo::all() {
        Ok(displays) if !displays.is_empty() => {
            displays
                .into_iter()
                .map(|d| Monitor {
                    id: d.id,
                    // v0.4.x has no `.name`; make a friendly one
                    name: if d.is_primary {
                        format!("Display {} (Primary)", d.id)
                    } else {
                        format!("Display {}", d.id)
                    },
                    origin_px: (d.x, d.y),                      // i32
                    size_px: (d.width as i32, d.height as i32), // u32 -> i32
                    scale_factor: d.scale_factor as f32,        // usually f32 already
                })
                .collect()
        }
        _ => {
            // Fallback: single main display using Enigo
            let en = enigo::Enigo::new();
            let (w, h) = en.main_display_size();
            vec![Monitor {
                id: 0,
                name: "Main display".to_string(),
                origin_px: (0, 0),
                size_px: (w as i32, h as i32),
                scale_factor: 1.0,
            }]
        }
    }
}

fn union_rect(monitors: &[Monitor]) -> (i32, i32, i32, i32) {
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    for m in monitors {
        min_x = min_x.min(m.origin_px.0);
        min_y = min_y.min(m.origin_px.1);
        max_x = max_x.max(m.origin_px.0 + m.size_px.0);
        max_y = max_y.max(m.origin_px.1 + m.size_px.1);
    }
    if monitors.is_empty() {
        (0, 0, 0, 0)
    } else {
        (min_x, min_y, max_x, max_y)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum DisplayChoice {
    All,
    One(usize), // index into monitors
}

// -------------- UI State --------------
struct AppState {
    // Picker state
    picking_area: bool,
    drag_start: Option<Pos2>,
    drag_end: Option<Pos2>,
    /// True while a "Pick Area" flow was launched from the sequence-action editor,
    /// so the picked bounds are routed back into the action form instead of the
    /// legacy single-region inputs.
    picking_for_sequence: bool,
    /// Frozen screenshot of the target screen(s), shown behind the dim scrim while
    /// picking so the overlay looks "seethrough" without relying on real window
    /// transparency. `None` if capture isn't available/failed, in which case the
    /// picker just shows a plain dim scrim with box-selection.
    picker_background: Option<egui::TextureHandle>,

    // Display state
    monitors: Vec<Monitor>,
    display_choice: DisplayChoice,

    // Config inputs - Single region mode
    bounds_inputs: [i32; 4], // min_x, max_x, min_y, max_y
    click_button_left: bool,
    min_secs: f32,
    max_secs: f32,
    use_finite_clicks: bool,
    num_clicks: u32,

    // Sequence mode
    sequence_enabled: bool,
    sequence_cycles: Option<u32>,         // None for infinite
    sequence_editing_idx: Option<usize>,  // Index of action being edited, None when not editing
    sequence_action_clicks: u32,          // Number of clicks for the current action being edited
    sequence_actions: Vec<SequenceAction>,

    // Engine
    job: Option<ClickJob>,
    config: Arc<Mutex<ClickConfig>>,
}

impl Default for AppState {
    fn default() -> Self {
        let monitors = query_monitors();
        Self {
            picking_area: false,
            drag_start: None,
            drag_end: None,
            picking_for_sequence: false,
            picker_background: None,

            monitors,
            display_choice: DisplayChoice::All,

            bounds_inputs: [100, 400, 100, 400],
            click_button_left: true,
            min_secs: 2.0,
            max_secs: 4.5,
            use_finite_clicks: false,
            num_clicks: 100,

            // Sequence mode defaults
            sequence_enabled: false,
            sequence_cycles: Some(1),
            sequence_editing_idx: None,
            sequence_action_clicks: 1,
            sequence_actions: Vec::new(),

            job: None,
            config: Arc::new(Mutex::new(ClickConfig::default())),
        }
    }
}

impl AppState {
    fn start(&mut self) {
        if self.job.is_some() { return; }
        let mut cfg = self.config.lock();

        if self.sequence_enabled && !self.sequence_actions.is_empty() {
            // Sequence mode
            cfg.sequence_mode = true;
            cfg.sequence = self.sequence_actions.clone();
            cfg.sequence_cycles = self.sequence_cycles;
            cfg.current_action = 0;
            cfg.finite_clicks = None; // Not used in sequence mode
            cfg.bounds = None; // Not used in sequence mode
        } else {
            // Single region mode
            cfg.sequence_mode = false;
            cfg.sequence.clear();
            cfg.button = if self.click_button_left { ClickButton::Left } else { ClickButton::Right };
            cfg.min_secs = self.min_secs;
            cfg.max_secs = self.max_secs;
            cfg.finite_clicks = if self.use_finite_clicks { Some(self.num_clicks) } else { None };
            cfg.bounds = Some(Bounds {
                min_x: self.bounds_inputs[0],
                max_x: self.bounds_inputs[1],
                min_y: self.bounds_inputs[2],
                max_y: self.bounds_inputs[3],
            });
        }
        drop(cfg);
        self.job = Some(ClickJob::spawn(Arc::clone(&self.config)));
    }

    fn stop(&mut self) {
        if let Some(job) = &self.job { job.stop(); }
        self.job = None;
    }

    /// Pause behaves the same as Stop today (the engine has no resumable paused
    /// state) but is kept as its own action since the UI exposes both; Start
    /// again picks up the current configuration from scratch.
    fn pause(&mut self) {
        self.stop();
    }

    fn refresh_monitors(&mut self) {
        self.monitors = query_monitors();
        // Clamp selection if out-of-range
        if let DisplayChoice::One(i) = self.display_choice {
            if i >= self.monitors.len() {
                self.display_choice = DisplayChoice::All;
            }
        }
    }

    fn current_target_rect(&self) -> ((i32, i32), (i32, i32)) {
        match self.display_choice {
            DisplayChoice::All => {
                let (min_x, min_y, max_x, max_y) = union_rect(&self.monitors);
                ((min_x, min_y), (max_x - min_x, max_y - min_y))
            }
            DisplayChoice::One(i) => {
                if let Some(m) = self.monitors.get(i) {
                    (m.origin_px, m.size_px)
                } else {
                    let (min_x, min_y, max_x, max_y) = union_rect(&self.monitors);
                    ((min_x, min_y), (max_x - min_x, max_y - min_y))
                }
            }
        }
    }

    fn enter_picker(&mut self, ctx: &egui::Context, for_sequence: bool) {
        self.drag_start = None;
        self.drag_end = None;
        self.picking_area = true;
        self.picking_for_sequence = for_sequence;

        // choose target rectangle in PHYSICAL pixels
        let (origin_px, size_px) = self.current_target_rect();

        // Snapshot what's actually on screen right now so the overlay can show it
        // instead of depending on the window compositor to make our window seethrough
        // (unreliable at runtime, especially on Windows — that's the bug this avoids).
        self.picker_background = capture_screenshot(origin_px, size_px)
            .map(|img| ctx.load_texture("picker_background", img, egui::TextureOptions::LINEAR));

        // convert to LOGICAL points for egui/eframe viewport commands
        let ppp = ctx.pixels_per_point().max(0.1);
        let inner = egui::vec2(size_px.0 as f32 / ppp, size_px.1 as f32 / ppp);
        let outer = egui::pos2(origin_px.0 as f32 / ppp, origin_px.1 as f32 / ppp);

        // We no longer rely on real window transparency: toggling it at runtime is
        // unreliable across platforms/compositors (notably Windows), which is exactly
        // what caused the picker to show opaque instead of "seethrough". Painting the
        // screenshot captured above (or, if that failed, just a plain dim scrim) gives
        // the same visual result without depending on compositor support.
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(outer));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inner));
        // Note: using borderless large window; not true OS fullscreen to avoid monitor switching quirks.
    }

    fn exit_picker(&mut self, ctx: &egui::Context) {
        self.picking_area = false;
        self.picker_background = None;
        // restore a comfy window
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(WindowLevel::Normal));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(520.0, 380.0)));
    }

    /// Convert current drag (logical points in current window) into PHYSICAL pixel bounds,
    /// offset by the selected monitor or union origin.
    fn set_bounds_from_drag(&mut self, ppp: f32, origin_px: (i32, i32)) {
        if let (Some(a), Some(b)) = (self.drag_start, self.drag_end) {
            let to_px = |p: Pos2| ((p.x * ppp).round() as i32, (p.y * ppp).round() as i32);
            let (ax, ay) = to_px(a);
            let (bx, by) = to_px(b);

            let min_x = ax.min(bx) + origin_px.0;
            let max_x = ax.max(bx) + origin_px.0;
            let min_y = ay.min(by) + origin_px.1;
            let max_y = ay.max(by) + origin_px.1;

            self.bounds_inputs = [min_x, max_x, min_y, max_y];

            if self.picking_for_sequence {
                // Route the picked rectangle into the sequence-action editor's fields;
                // it's committed to a SequenceAction only when "Save Action" is clicked.
            } else {
                self.config.lock().bounds = Some(Bounds { min_x, max_x, min_y, max_y });
            }
            eprintln!("Selected bounds (px): x=[{}..{}], y=[{}..{}]", min_x, max_x, min_y, max_y);
        }
    }
}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // -------- Picker Overlay --------
        if self.picking_area {
            let screen_rect = ctx.screen_rect();
            let layer_id = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("picker"));
            let painter = egui::Painter::new(ctx.clone(), layer_id, egui::Rect::EVERYTHING);

            // Draw what's actually on screen (captured the moment picking started) so
            // the overlay reads as "seethrough" even though the window itself is opaque.
            if let Some(tex) = &self.picker_background {
                painter.image(
                    tex.id(),
                    screen_rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }

            // Gray translucent scrim on top, dimming the frozen screenshot (or, if no
            // screenshot could be captured, standing alone as a plain dim overlay).
            painter.rect_filled(
                screen_rect,
                0.0,
                Color32::from_rgba_premultiplied(128, 128, 128, 100),
            );

            // Interaction area
            egui::Area::new(egui::Id::new("picker_area"))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    let resp = ui.allocate_rect(screen_rect, Sense::click_and_drag());
                    if resp.drag_started() {
                        self.drag_start = resp.interact_pointer_pos();
                        self.drag_end = self.drag_start;
                    }
                    if resp.dragged() {
                        self.drag_end = resp.interact_pointer_pos();
                    }
                    if resp.drag_stopped() {
                        self.drag_end = resp.interact_pointer_pos();

                        // Determine origin_px to add (depends on selected target)
                        let (origin_px, _) = self.current_target_rect();
                        let ppp = ctx.pixels_per_point().max(0.1);
                        self.set_bounds_from_drag(ppp, origin_px);
                        self.exit_picker(ctx);
                    }

                    if let (Some(a), Some(b)) = (self.drag_start, self.drag_end) {
                        let rect = Rect::from_two_pos(a, b);
                        let stroke = egui::Stroke { width: 2.0, color: Color32::LIGHT_BLUE };
                        painter.rect_stroke(rect, 0.0, stroke);
                    }
                });

            ctx.request_repaint();
            return; // Skip main UI while picking
        }

        // -------- Main UI --------
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.heading("Area Clicker — Multi-Display");
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.checkbox(&mut self.sequence_enabled, "Enable Sequence Mode");
            ui.separator();

            if self.sequence_enabled {
                // Sequence Mode UI
                ui.horizontal(|ui| {
                    ui.label("Sequence Cycles:");
                    if ui.radio(self.sequence_cycles.is_none(), "Infinite").clicked() {
                        self.sequence_cycles = None;
                    }
                    if ui.radio(self.sequence_cycles.is_some(), "Fixed").clicked() {
                        self.sequence_cycles = Some(self.sequence_cycles.unwrap_or(1));
                    }
                    if let Some(cycles) = &mut self.sequence_cycles {
                        ui.add(egui::DragValue::new(cycles).speed(1).clamp_range(1..=10000));
                    }
                });

                ui.separator();
                ui.heading("Sequence Actions");

                let mut remove_idx: Option<usize> = None;
                for (i, action) in self.sequence_actions.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "{}. Region: [{}, {}]×[{}, {}]",
                            i + 1,
                            action.bounds.min_x, action.bounds.max_x,
                            action.bounds.min_y, action.bounds.max_y,
                        ));
                        ui.label(format!("Clicks: {}", action.clicks_per_cycle));
                        ui.label(format!("Interval: {:.2}s-{:.2}s", action.min_secs, action.max_secs));
                        if ui.button("Edit").clicked() {
                            self.sequence_editing_idx = Some(i);
                            self.bounds_inputs = [
                                action.bounds.min_x, action.bounds.max_x,
                                action.bounds.min_y, action.bounds.max_y,
                            ];
                            self.min_secs = action.min_secs;
                            self.max_secs = action.max_secs;
                            self.click_button_left = action.button == ClickButton::Left;
                            self.sequence_action_clicks = action.clicks_per_cycle;
                        }
                        if ui.button("Remove").clicked() {
                            remove_idx = Some(i);
                        }
                    });
                }
                if let Some(i) = remove_idx {
                    if Some(i) == self.sequence_editing_idx {
                        self.sequence_editing_idx = None;
                    }
                    self.sequence_actions.remove(i);
                }

                ui.group(|ui| {
                    if self.sequence_editing_idx.is_some() {
                        ui.label("Edit Action");
                    } else {
                        ui.label("New Action");
                    }

                    ui.horizontal(|ui| {
                        ui.label("Region (px, screen coords):");
                        ui.label(format!(
                            "[{}, {}] × [{}, {}]",
                            self.bounds_inputs[0], self.bounds_inputs[1],
                            self.bounds_inputs[2], self.bounds_inputs[3],
                        ));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Click type:");
                        ui.checkbox(&mut self.click_button_left, "Left");
                        let mut right = !self.click_button_left;
                        if ui.checkbox(&mut right, "Right").clicked() { self.click_button_left = !right; }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Clicks per cycle:");
                        ui.add(egui::DragValue::new(&mut self.sequence_action_clicks).speed(1).clamp_range(1..=1000));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Interval (seconds):");
                        ui.add(egui::DragValue::new(&mut self.min_secs).speed(0.1).clamp_range(0.0..=3600.0));
                        ui.label("to");
                        ui.add(egui::DragValue::new(&mut self.max_secs).speed(0.1).clamp_range(0.0..=3600.0));
                    });

                    ui.horizontal(|ui| {
                        if ui.button("Pick Area").clicked() {
                            self.enter_picker(ctx, true);
                        }

                        if ui.button("Save Action").clicked() {
                            let min_secs = self.min_secs.min(self.max_secs);
                            let max_secs = self.max_secs.max(self.min_secs);
                            let action = SequenceAction {
                                bounds: Bounds {
                                    min_x: self.bounds_inputs[0],
                                    max_x: self.bounds_inputs[1],
                                    min_y: self.bounds_inputs[2],
                                    max_y: self.bounds_inputs[3],
                                },
                                button: if self.click_button_left { ClickButton::Left } else { ClickButton::Right },
                                min_secs,
                                max_secs,
                                clicks_per_cycle: self.sequence_action_clicks,
                            };

                            if action.bounds.is_valid() {
                                if let Some(idx) = self.sequence_editing_idx {
                                    self.sequence_actions[idx] = action;
                                    self.sequence_editing_idx = None;
                                } else {
                                    self.sequence_actions.push(action);
                                }
                            }
                        }

                        if self.sequence_editing_idx.is_some() {
                            if ui.button("Cancel Edit").clicked() {
                                self.sequence_editing_idx = None;
                            }
                        }
                    });
                });
            }

            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.vertical(|ui| {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label("Target display:");
                            egui::ComboBox::from_id_source("display_select")
                                .selected_text(match self.display_choice {
                                    DisplayChoice::All => "All displays".into(),
                                    DisplayChoice::One(i) => self.monitors.get(i)
                                        .map(|m| m.name.clone())
                                        .unwrap_or_else(|| "Unknown".into()),
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.display_choice, DisplayChoice::All, "All displays");
                                    for (i, m) in self.monitors.iter().enumerate() {
                                        ui.selectable_value(&mut self.display_choice, DisplayChoice::One(i), &m.name);
                                    }
                                });

                            if ui.button("↻ Refresh").clicked() {
                                self.refresh_monitors();
                            }
                        });

                        ui.separator();

                        if !self.sequence_enabled {
                            ui.label("Selection (px, screen coords)");
                            ui.horizontal(|ui| { ui.label("min X"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[0])); });
                            ui.horizontal(|ui| { ui.label("max X"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[1])); });
                            ui.horizontal(|ui| { ui.label("min Y"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[2])); });
                            ui.horizontal(|ui| { ui.label("max Y"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[3])); });

                            if ui.button("Pick Area (drag a rectangle)").clicked() {
                                self.enter_picker(ctx, false);
                            }
                        }
                    });

                    ui.separator();

                    ui.group(|ui| {
                        ui.label("Settings");

                        if !self.sequence_enabled {
                            ui.horizontal(|ui| {
                                ui.label("Click type:");
                                ui.checkbox(&mut self.click_button_left, "Left");
                                let mut right = !self.click_button_left;
                                if ui.checkbox(&mut right, "Right").clicked() { self.click_button_left = !right; }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Interval (seconds):");
                                ui.add(egui::DragValue::new(&mut self.min_secs).speed(0.1).clamp_range(0.0..=3600.0));
                                ui.label("to");
                                ui.add(egui::DragValue::new(&mut self.max_secs).speed(0.1).clamp_range(0.0..=3600.0));
                            });
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.use_finite_clicks, "Limit number of clicks");
                                if self.use_finite_clicks {
                                    ui.add(egui::DragValue::new(&mut self.num_clicks).speed(1.0).clamp_range(1..=1000000));
                                }
                            });
                        }

                        ui.horizontal(|ui| {
                            if ui.button("Start").clicked() { self.start(); }
                            if ui.button("Pause").clicked() { self.pause(); }
                            if ui.button("Stop").clicked() { self.stop(); }
                        });

                        if let Some(job) = &self.job {
                            let running = job.running.load(Ordering::Relaxed);
                            if running {
                                let cfg = self.config.lock();
                                if cfg.sequence_mode {
                                    ui.label(format!(
                                        "Status: Running sequence ({} actions, current: {})",
                                        cfg.sequence.len(),
                                        cfg.current_action + 1
                                    ));
                                    if let Some(cycles) = cfg.sequence_cycles {
                                        ui.label(format!("Cycles remaining: {}", cycles.saturating_sub(0)));
                                    } else {
                                        ui.label("Cycles: Infinite");
                                    }
                                } else {
                                    ui.label("Status: Running (single region)");
                                }
                            } else {
                                ui.label("Status: Stopped");
                            }
                        } else {
                            ui.label("Status: Stopped");
                        }
                    });
                });
            });

            // Preview regions
            let cfg = self.config.lock();
            if cfg.sequence_mode {
                // Preview all sequence regions
                for (i, action) in cfg.sequence.iter().enumerate() {
                    let b = &action.bounds;
                    let color = if Some(i) == self.sequence_editing_idx {
                        Color32::LIGHT_BLUE
                    } else if i == cfg.current_action && self.job.is_some() {
                        Color32::LIGHT_GREEN
                    } else {
                        Color32::GRAY
                    };
                    ui.painter().rect_stroke(
                        Rect::from_min_max(
                            Pos2::new(b.min_x as f32, b.min_y as f32),
                            Pos2::new(b.max_x as f32, b.max_y as f32),
                        ),
                        0.0,
                        egui::Stroke { width: 2.0, color },
                    );
                }
            } else if let Some(b) = cfg.bounds {
                let info = format!(
                    "Active bounds: x=[{}..{}], y=[{}..{}] ({}x{})",
                    b.min_x, b.max_x, b.min_y, b.max_y, b.width(), b.height()
                );
                ui.separator();
                ui.monospace(info);
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let mut opts = eframe::NativeOptions::default();
    let _args = Args::parse(); // Arguments will be used later

    // Start as a normal, opaque window; we resize/position it during picking.
    // The picker overlay no longer depends on real window transparency (unreliable at
    // runtime across platforms/compositors) — it paints a captured screenshot instead,
    // so there's no need to request a transparent window here.
    opts.viewport.resizable = Some(true);
    opts.viewport.mouse_passthrough = Some(false); // Ensure we capture mouse events
    opts.follow_system_theme = true;

    eframe::run_native(
        "Area Clicker",
        opts,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Box::<AppState>::default()
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounds_validation() {
        let valid_bounds = Bounds { min_x: 100, max_x: 200, min_y: 100, max_y: 200 };
        assert!(valid_bounds.is_valid());
        assert_eq!(valid_bounds.width(), 100);
        assert_eq!(valid_bounds.height(), 100);

        let invalid_bounds = Bounds { min_x: 200, max_x: 100, min_y: 200, max_y: 100 };
        assert!(!invalid_bounds.is_valid());
    }

    #[test]
    fn test_click_job_creation() {
        let config = Arc::new(Mutex::new(ClickConfig {
            bounds: Some(Bounds { min_x: 100, max_x: 200, min_y: 100, max_y: 200 }),
            button: ClickButton::Left,
            min_secs: 2.0,
            max_secs: 4.5,
            ..ClickConfig::default()
        }));

        let job = ClickJob::spawn(Arc::clone(&config));
        assert!(job.running.load(Ordering::Relaxed));

        job.stop();
        // Give the worker thread a moment to observe the flag (it's cooperative).
        std::thread::sleep(Duration::from_millis(50));
        assert!(!job.running.load(Ordering::Relaxed));
    }

    #[test]
    fn test_app_state_defaults() {
        let state = AppState::default();
        assert!(!state.picking_area);
        assert!(state.drag_start.is_none());
        assert!(state.drag_end.is_none());
        assert!(state.click_button_left);
        assert!(state.job.is_none());

        assert_eq!(state.min_secs, 2.0);
        assert_eq!(state.max_secs, 4.5);
    }

    #[test]
    fn test_set_bounds_from_drag_ppp1_origin0() {
        let mut state = AppState::default();
        state.drag_start = Some(Pos2::new(100.0, 100.0));
        state.drag_end = Some(Pos2::new(200.0, 200.0));
        state.set_bounds_from_drag(1.0, (0, 0));
        assert_eq!(state.bounds_inputs, [100, 200, 100, 200]);

        // reverse drag
        state.drag_start = Some(Pos2::new(200.0, 200.0));
        state.drag_end = Some(Pos2::new(100.0, 100.0));
        state.set_bounds_from_drag(1.0, (0, 0));
        assert_eq!(state.bounds_inputs, [100, 200, 100, 200]);
    }

    #[test]
    fn test_click_interval() {
        let config = Arc::new(Mutex::new(ClickConfig {
            bounds: Some(Bounds { min_x: 100, max_x: 200, min_y: 100, max_y: 200 }),
            button: ClickButton::Left,
            min_secs: 0.05,
            max_secs: 0.1,
            ..ClickConfig::default()
        }));

        let job = ClickJob::spawn(Arc::clone(&config));
        std::thread::sleep(Duration::from_millis(300));
        job.stop();
        assert!(!job.running.load(Ordering::Relaxed));
    }
}
