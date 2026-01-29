mod human_mouse;
mod presets;

use eframe::{egui, egui::{Color32, Pos2, Rect, Sense, WindowLevel}};
use enigo::MouseControllable;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use crate::human_mouse::{HumanMouseSettings, Bounds, human_move_and_click};
use crate::presets::{PresetStore, UiPreset, NamedPoint, BoundsSerde, ClickSequence, SequenceStep};

use clap::Parser;

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

// Click operation utilities

/// Moved to separate module

#[derive(Clone, Copy, Debug)]
enum ClickButton { Left, Right }

/// Represents either a random clicking mode or a sequence mode
#[derive(Clone, Debug)]
enum ClickMode {
    /// Random clicking in an area with min/max intervals
    Random {
        bounds: Bounds,
        button: ClickButton,
        min_secs: f32,
        max_secs: f32,
        finite_clicks: Option<u32>,
    },
    /// Execute a predefined sequence of clicks
    Sequence {
        sequence: ClickSequence,
        preset: UiPreset,
        repeat_count: Option<u32>, // None = infinite, Some(n) = repeat n times
    },
}

struct ClickJob {
    running: Arc<AtomicBool>,
    #[allow(dead_code)]
    mode: Arc<Mutex<ClickMode>>,
}

#[derive(Clone, Debug)]
struct ClickConfig {
    bounds: Option<Bounds>,
    button: ClickButton,
    min_secs: f32,
    max_secs: f32,
    finite_clicks: Option<u32>,  // None for infinite, Some(n) for n clicks
}

static ENIGO: Lazy<Mutex<enigo::Enigo>> = Lazy::new(|| Mutex::new(enigo::Enigo::new()));

/// Helper function to perform interruptible sleep.
/// Splits the sleep into small chunks (50ms) so the running flag can be checked frequently.
/// This prevents UI stalling by ensuring the thread can be interrupted quickly.
fn interruptible_sleep(duration_ms: u64, running: &Arc<AtomicBool>) {
    use std::time::Duration;
    let chunk_size = 50u64;
    for _ in 0..(duration_ms / chunk_size) {
        if !running.load(Ordering::Relaxed) { break; }
        std::thread::sleep(Duration::from_millis(chunk_size));
    }
    if !running.load(Ordering::Relaxed) { return; }
    let remainder = duration_ms % chunk_size;
    if remainder > 0 {
        std::thread::sleep(Duration::from_millis(remainder));
    }
}

impl ClickJob {
    fn spawn_random(config: Arc<Mutex<ClickConfig>>) -> Self {
        use std::sync::atomic::AtomicBool;
        use std::sync::atomic::Ordering;
        use std::sync::{Arc};
        use rand::Rng;
        use enigo::{MouseButton};

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);
        let config_clone = Arc::clone(&config);

        eprintln!("Starting click job with config: {:?}", config.lock());

        std::thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let mut last_pos: Option<(i32,i32)> = None;
            let mut clicks_remaining = config_clone.lock().finite_clicks;

            loop {
                if !running_clone.load(Ordering::Relaxed) { break; }
                
                // Check if we've completed our finite clicks
                if let Some(0) = clicks_remaining {
                    running_clone.store(false, Ordering::Relaxed);
                    break;
                }

                let cfg = config_clone.lock().clone();
                let Some(b) = cfg.bounds else {
                    interruptible_sleep(200, &running_clone);
                    continue;
                };
                if !b.is_valid() {
                    interruptible_sleep(200, &running_clone);
                    continue;
                }

                // pick random point inside box
                let x = rng.gen_range(b.min_x..=b.max_x);
                let y = rng.gen_range(b.min_y..=b.max_y);

                // human-style move & click
                {
                    let mut en = ENIGO.lock();

                    // starting point: last known, or “outside the square” so we can test re-entry
                    let from = last_pos.unwrap_or((b.min_x - 40, b.min_y - 40));

                    // minimal rect adapter for the helper
                    // map your ClickButton -> enigo::MouseButton
                    let button = match cfg.button {
                        ClickButton::Left => MouseButton::Left,
                        ClickButton::Right => MouseButton::Right,
                    };

                    // run the human move & click
                    human_move_and_click(
                        &mut *en,
                        from,
                        (x, y),
                        Some(Bounds { min_x: b.min_x, min_y: b.min_y, max_x: b.max_x, max_y: b.max_y }),
                        &HumanMouseSettings::default(),
                        button,
                    );
                }

                // remember where we ended up
                last_pos = Some((x, y));

                // Update click counter if we're using finite clicks
                if let Some(ref mut remaining) = clicks_remaining {
                    *remaining = remaining.saturating_sub(1);
                }

                // sleep random between min..max (seconds), with interruptible sleep
                let (min_s, max_s) = if cfg.min_secs <= cfg.max_secs {
                    (cfg.min_secs, cfg.max_secs)
                } else { (cfg.max_secs, cfg.min_secs) };
                let wait = rng.gen_range(min_s..=max_s).max(0.01);
                let ms = (wait * 1000.0) as u64;
                interruptible_sleep(ms, &running_clone);
            }
        });

        let mode = Arc::new(Mutex::new(ClickMode::Random {
            bounds: config.lock().bounds.unwrap_or(Bounds { min_x: 0, max_x: 0, min_y: 0, max_y: 0 }),
            button: config.lock().button,
            min_secs: config.lock().min_secs,
            max_secs: config.lock().max_secs,
            finite_clicks: config.lock().finite_clicks,
        }));

        Self { running, mode }
    }

    fn spawn_sequence(sequence: ClickSequence, preset: UiPreset, repeat_count: Option<u32>) -> Self {
        use std::sync::atomic::AtomicBool;
        use std::sync::atomic::Ordering;
        use std::sync::{Arc};
        use rand::Rng;
        use enigo::{MouseButton};

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);

        eprintln!("Starting sequence click job: {}", sequence.name);
        
        let sequence_clone = sequence.clone();
        let preset_clone = preset.clone();

        std::thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let mut last_pos: Option<(i32,i32)> = None;
            let mut repeats_remaining = repeat_count;

            loop {
                if !running_clone.load(Ordering::Relaxed) { break; }

                // Check if we've completed finite repeats
                if let Some(0) = repeats_remaining {
                    running_clone.store(false, Ordering::Relaxed);
                    break;
                }

                // Execute each step in the sequence
                for step in &sequence_clone.steps {
                    if !running_clone.load(Ordering::Relaxed) { break; }

                    // Find the area in the preset
                    if let Some(named_area) = preset_clone.areas.iter().find(|a| a.name == step.area_name) {
                        let bounds = Bounds {
                            min_x: named_area.bounds.min_x,
                            max_x: named_area.bounds.max_x,
                            min_y: named_area.bounds.min_y,
                            max_y: named_area.bounds.max_y,
                        };

                        if !bounds.is_valid() {
                            eprintln!("Invalid bounds for area {}", step.area_name);
                            continue;
                        }

                        // Pick a random point in the area
                        let x = rng.gen_range(bounds.min_x..=bounds.max_x);
                        let y = rng.gen_range(bounds.min_y..=bounds.max_y);

                        // Perform the click
                        {
                            let mut en = ENIGO.lock();
                            let from = last_pos.unwrap_or((bounds.min_x - 40, bounds.min_y - 40));
                            let button = match step.button_type.as_str() {
                                "Right" => MouseButton::Right,
                                _ => MouseButton::Left,
                            };

                            human_move_and_click(
                                &mut *en,
                                from,
                                (x, y),
                                Some(bounds),
                                &HumanMouseSettings::default(),
                                button,
                            );
                        }

                        last_pos = Some((x, y));
                    } else {
                        eprintln!("Area '{}' not found in preset '{}'", step.area_name, preset_clone.name);
                    }

                    // Wait before next step (randomize between min and max, using interruptible sleep)
                    let interval_secs = if step.max_interval > step.min_interval {
                        rng.gen_range(step.min_interval..=step.max_interval)
                    } else {
                        step.min_interval
                    };
                    let ms = (interval_secs * 1000.0) as u64;
                    if ms > 0 {
                        interruptible_sleep(ms, &running_clone);
                    }
                }

                // Update repeat counter
                if let Some(ref mut remaining) = repeats_remaining {
                    *remaining = remaining.saturating_sub(1);
                }
            }
        });

        let mode = Arc::new(Mutex::new(ClickMode::Sequence {
            sequence,
            preset,
            repeat_count,
        }));

        Self { running, mode }
    }

    fn stop(&self) { 
        self.running.store(false, Ordering::Relaxed); 
    }
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
                    origin_px: (d.x, d.y),                            // i32
                    size_px: (d.width as i32, d.height as i32),       // u32 -> i32
                    scale_factor: d.scale_factor as f32,              // usually f32 already
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

    // Display state
    monitors: Vec<Monitor>,
    display_choice: DisplayChoice,

    // Config inputs
    bounds_inputs: [i32; 4], // min_x, max_x, min_y, max_y
    click_button_left: bool,
    min_secs: f32,
    max_secs: f32,
    use_finite_clicks: bool,
    num_clicks: u32,

    // Engine
    job: Option<ClickJob>,
    config: Arc<Mutex<ClickConfig>>,

    // ---- Presets ----
    preset_store: PresetStore,
    selected_preset: Option<String>,
    new_preset_name: String,

    // ---- Areas inside a preset ----
    selected_area: Option<String>,
    new_area_name: String,

    // ---- Point picking ----
    picking_point: bool,
    new_point_name: String,

    // ---- Sequences ----
    selected_sequence: Option<String>,
    new_sequence_name: String,

    // ---- Window state ----
    saved_window_pos: Option<egui::Pos2>,
    saved_window_size: Option<egui::Vec2>,
}

impl Default for AppState {
    fn default() -> Self {
        let monitors = query_monitors();
        let preset_store = PresetStore::load_or_default();
        Self {
            picking_area: false,
            drag_start: None,
            drag_end: None,

            monitors,
            display_choice: DisplayChoice::All,

            bounds_inputs: [100, 400, 100, 400],
            click_button_left: true,
            min_secs: 2.0,
            max_secs: 4.5,
            use_finite_clicks: false,
            num_clicks: 100,

            job: None,
            config: Arc::new(Mutex::new(ClickConfig{
                bounds: Some(Bounds{min_x:100, max_x:400, min_y:100, max_y:400}),
                button: ClickButton::Left,
                min_secs: 2.0,
                max_secs: 4.5,
                finite_clicks: None,
            })),

            preset_store: PresetStore::load_or_default(),
            selected_preset: None,
            new_preset_name: String::new(),

            selected_area: None,
            new_area_name: "Main".to_string(),

            picking_point: false,
            new_point_name: String::new(),

            selected_sequence: None,
            new_sequence_name: String::new(),

            saved_window_pos: None,
            saved_window_size: None,

        }
    }
}

impl AppState {
    fn start(&mut self) {
        if self.job.is_some() { return; }
        
        // Check if we should run a sequence instead of random clicking
        if let Some(seq_name) = &self.selected_sequence.clone() {
            if let Some(sequence) = self.preset_store.get_sequence(seq_name) {
                if let Some(preset_name) = &sequence.preset_name.clone() {
                    if let Some(preset) = self.preset_store.presets.iter().find(|p| p.name == *preset_name) {
                        let repeat_count = Some(1); // Execute sequence once per click
                        self.job = Some(ClickJob::spawn_sequence(
                            sequence.clone(),
                            preset.clone(),
                            repeat_count
                        ));
                        return;
                    }
                }
            }
        }
        
        // Otherwise, run random clicking
        let mut cfg = self.config.lock();
        cfg.button = if self.click_button_left { ClickButton::Left } else { ClickButton::Right };
        cfg.min_secs = self.min_secs;
        cfg.max_secs = self.max_secs;
        cfg.finite_clicks = if self.use_finite_clicks { Some(self.num_clicks) } else { None };
        cfg.bounds = Some(Bounds{
            min_x: self.bounds_inputs[0],
            max_x: self.bounds_inputs[1],
            min_y: self.bounds_inputs[2],
            max_y: self.bounds_inputs[3],
        });
        drop(cfg);
        self.job = Some(ClickJob::spawn_random(Arc::clone(&self.config)));
    }

    fn stop(&mut self) {
        if let Some(job) = &self.job { job.stop(); }
        self.job = None;
    }

    fn pause(&mut self) {
        if let Some(job) = &self.job { job.stop(); }
        self.job = None;
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

    fn enter_picker(&mut self, ctx: &egui::Context) {
        self.drag_start = None;
        self.drag_end = None;
        self.picking_area = true;

        // Save current window position and size before fullscreen
        ctx.input(|i| {
            if let Some(outer_rect) = i.viewport().outer_rect {
                self.saved_window_pos = Some(outer_rect.left_top());
            }
            if let Some(inner_rect) = i.viewport().inner_rect {
                self.saved_window_size = Some(inner_rect.size());
            }
        });

        // choose target rectangle in PHYSICAL pixels
        let (origin_px, size_px) = match self.display_choice {
            DisplayChoice::All => {
                let (min_x, min_y, max_x, max_y) = union_rect(&self.monitors);
                ((min_x, min_y), (max_x - min_x, max_y - min_y))
            }
            DisplayChoice::One(i) => {
                if let Some(m) = self.monitors.get(i) {
                    (m.origin_px, m.size_px)
                } else {
                    // fallback: union
                    let (min_x, min_y, max_x, max_y) = union_rect(&self.monitors);
                    ((min_x, min_y), (max_x - min_x, max_y - min_y))
                }
            }
        };

        // convert to LOGICAL points for egui/eframe viewport commands
        let ppp = ctx.pixels_per_point().max(0.1);
        let inner = egui::vec2(size_px.0 as f32 / ppp, size_px.1 as f32 / ppp);
        let outer = egui::pos2(origin_px.0 as f32 / ppp, origin_px.1 as f32 / ppp);

        ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(true));  // EXPLICITLY enable transparency for picking
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(outer));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inner));
        // Note: using borderless large window; not true OS fullscreen to avoid monitor switching quirks.
    }

    fn exit_picker(&mut self, ctx: &egui::Context) {
        self.picking_area = false;
        // Restore window to normal state (not transparent, with decorations)
        ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(false));  // EXPLICITLY disable transparency
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(WindowLevel::Normal));
        
        // Restore saved size, or use default
        let size = self.saved_window_size.unwrap_or(egui::vec2(700.0, 450.0));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        
        // Restore saved position, or use default
        if let Some(pos) = self.saved_window_pos {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
        }
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
            self.config.lock().bounds = Some(Bounds{min_x, max_x, min_y, max_y});
            eprintln!("Selected bounds (px): x=[{}..{}], y=[{}..{}]", min_x, max_x, min_y, max_y);
        }
    }

    fn apply_boundsserde(&mut self, b: BoundsSerde) {
        self.bounds_inputs = [b.min_x, b.max_x, b.min_y, b.max_y];
        self.config.lock().bounds = Some(Bounds {
            min_x: b.min_x,
            max_x: b.max_x,
            min_y: b.min_y,
            max_y: b.max_y,
        });
}

}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // -------- Picker Overlay --------
        if self.picking_area {
            let screen_rect = ctx.screen_rect();
            let layer_id = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("picker"));
            let painter = egui::Painter::new(ctx.clone(), layer_id, egui::Rect::EVERYTHING);

            // Gray translucent overlay
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
                        let origin_px = match self.display_choice {
                            DisplayChoice::All => {
                                let (min_x, min_y, _max_x, _max_y) = union_rect(&self.monitors);
                                (min_x, min_y)
                            }
                            DisplayChoice::One(i) => {
                                self.monitors.get(i).map(|m| m.origin_px).unwrap_or((0, 0))
                            }
                        };
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

                // -------- Point Picker Overlay --------
        if self.picking_point {
            let screen_rect = ctx.screen_rect();
            let layer_id = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("point_picker"));
            let painter = egui::Painter::new(ctx.clone(), layer_id, egui::Rect::EVERYTHING);

            painter.rect_filled(
                screen_rect,
                0.0,
                Color32::from_rgba_premultiplied(128, 128, 128, 100),
            );

            egui::Area::new(egui::Id::new("point_picker_area"))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    let resp = ui.allocate_rect(screen_rect, Sense::click());

                    if resp.clicked() {
                        if let Some(pos) = resp.interact_pointer_pos() {
                            // Convert to PHYSICAL pixels and add correct origin like you do for bounds.
                            let origin_px = match self.display_choice {
                                DisplayChoice::All => {
                                    let (min_x, min_y, _max_x, _max_y) = union_rect(&self.monitors);
                                    (min_x, min_y)
                                }
                                DisplayChoice::One(i) => {
                                    self.monitors.get(i).map(|m| m.origin_px).unwrap_or((0, 0))
                                }
                            };

                            let ppp = ctx.pixels_per_point().max(0.1);
                            let x = (pos.x * ppp).round() as i32 + origin_px.0;
                            let y = (pos.y * ppp).round() as i32 + origin_px.1;

                            // Add point to selected preset
                            if let Some(sel) = self.selected_preset.clone() {
                                if let Some(p) = self.preset_store.presets.iter_mut().find(|p| p.name == sel) {
                                    p.points.push(NamedPoint {
                                        name: self.new_point_name.trim().to_string(),
                                        x,
                                        y,
                                    });
                                    let _ = self.preset_store.save();
                                }
                            }

                            self.picking_point = false;
                            self.exit_picker(ctx); // re-use your window restore logic
                        }
                    }
                });

            ctx.request_repaint();
            return;
        }

        // -------- Main UI --------
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.heading("Area Clicker — Multi-Display");
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
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

                        ui.label("Selection (px, screen coords)");
                        ui.horizontal(|ui| { ui.label("min X"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[0])); });
                        ui.horizontal(|ui| { ui.label("max X"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[1])); });
                        ui.horizontal(|ui| { ui.label("min Y"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[2])); });
                        ui.horizontal(|ui| { ui.label("max Y"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[3])); });

                        if ui.button("Pick Area (drag a rectangle)").clicked() {
                            self.enter_picker(ctx);
                        }
                    });

                                        ui.separator();

                    ui.group(|ui| {
                        ui.label("UI Presets");

                        // Dropdown
                        let selected_text = self.selected_preset.clone().unwrap_or_else(|| "None".into());
                        egui::ComboBox::from_id_source("preset_select")
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.selected_preset, None, "None");
                                for p in &self.preset_store.presets {
                                    ui.selectable_value(&mut self.selected_preset, Some(p.name.clone()), &p.name);
                                }
                            });

                        ui.horizontal(|ui| {
                            if ui.button("Load bounds").clicked() {
                                if let Some(sel) = self.selected_preset.clone() {
                                    if let Some(p) = self.preset_store.presets.iter().find(|p| p.name == sel) {
                                        // Load first area's bounds if available
                                        if let Some(area) = p.areas.first() {
                                            self.apply_boundsserde(area.bounds);
                                        }
                                    }
                                }
                            }

                            if ui.button("Save bounds to preset").clicked() {
                                if let Some(sel) = self.selected_preset.clone() {
                                    if let Some(p) = self.preset_store.presets.iter_mut().find(|p| p.name == sel) {
                                        // Add or update "bounds" area from current bounds_inputs
                                        let bounds_area = crate::presets::NamedArea {
                                            name: "bounds".to_string(),
                                            bounds: BoundsSerde::from_inputs(self.bounds_inputs),
                                        };
                                        if let Some(existing) = p.areas.iter_mut().find(|a| a.name == "bounds") {
                                            existing.bounds = bounds_area.bounds;
                                        } else {
                                            p.areas.push(bounds_area);
                                        }
                                        let _ = self.preset_store.save();
                                    }
                                }
                            }

                            if ui.button("Delete preset").clicked() {
                                if let Some(sel) = self.selected_preset.clone() {
                                    self.preset_store.remove_by_name(&sel);
                                    self.selected_preset = None;
                                    let _ = self.preset_store.save();
                                }
                            }
                        });

                        ui.separator();

                        // Create preset
                        ui.horizontal(|ui| {
                            ui.label("New preset name:");
                            ui.text_edit_singleline(&mut self.new_preset_name);

                            if ui.button("Create from current bounds").clicked() {
                                let name = self.new_preset_name.trim();
                                if !name.is_empty() {
                                    let preset = UiPreset {
                                        name: name.to_string(),
                                        areas: vec![],
                                        points: vec![],
                                    };
                                    self.preset_store.upsert_preset(preset);
                                    self.selected_preset = Some(name.to_string());
                                    self.new_preset_name.clear();
                                    let _ = self.preset_store.save();
                                }
                            }
                        });

                        ui.separator();
                        ui.group(|ui| {
                            ui.label("UI Presets");

                            // ---------- Preset selector ----------
                            let selected_text = self.selected_preset.clone().unwrap_or_else(|| "None".into());
                            egui::ComboBox::from_id_source("preset_select")
                                .selected_text(selected_text)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.selected_preset, None, "None");
                                    for p in &self.preset_store.presets {
                                        ui.selectable_value(&mut self.selected_preset, Some(p.name.clone()), &p.name);
                                    }
                                });

                            // If preset changed, clear area selection (simple + avoids mismatches)
                            // (If you already track last preset, use that; otherwise do this conservatively.)
                            if self.selected_preset.is_none() {
                                self.selected_area = None;
                            }

                            ui.horizontal(|ui| {
                                ui.label("New preset:");
                                ui.text_edit_singleline(&mut self.new_preset_name);

                                if ui.button("Create").clicked() {
                                    let name = self.new_preset_name.trim();
                                    if !name.is_empty() {
                                        let preset = UiPreset {
                                            name: name.to_string(),
                                            areas: vec![],
                                            points: vec![],
                                        };
                                        self.preset_store.upsert_preset(preset);
                                        self.selected_preset = Some(name.to_string());
                                        self.selected_area = None;
                                        self.new_preset_name.clear();
                                        let _ = self.preset_store.save();
                                    }
                                }

                                if ui.button("Delete preset").clicked() {
                                    if let Some(sel) = self.selected_preset.clone() {
                                        self.preset_store.remove_by_name(&sel);
                                        self.selected_preset = None;
                                        self.selected_area = None;
                                        let _ = self.preset_store.save();
                                    }
                                }
                            });

                            ui.separator();
                            ui.label("Areas (named rectangles)");

                            // ---------- Area section ----------
                            if let Some(pname) = self.selected_preset.clone() {
                                // Area dropdown
                                let area_names: Vec<String> = self.preset_store.presets.iter()
                                    .find(|p| p.name == pname)
                                    .map(|p| p.areas.iter().map(|a| a.name.clone()).collect())
                                    .unwrap_or_default();

                                let area_selected_text = self.selected_area.clone().unwrap_or_else(|| "None".into());
                                egui::ComboBox::from_id_source("area_select")
                                    .selected_text(area_selected_text)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut self.selected_area, None, "None");
                                        for a_name in &area_names {
                                            ui.selectable_value(&mut self.selected_area, Some(a_name.clone()), a_name);
                                        }
                                    });

                                ui.horizontal(|ui| {
                                    ui.label("New area name:");
                                    ui.text_edit_singleline(&mut self.new_area_name);

                                    if ui.button("Add area from current bounds").clicked() {
                                        let aname = self.new_area_name.trim().to_string();
                                        if !aname.is_empty() {
                                            if let Some(preset) = self.preset_store.presets.iter_mut().find(|p| p.name == pname) {
                                                let area = crate::presets::NamedArea {
                                                    name: aname.clone(),
                                                    bounds: crate::presets::BoundsSerde::from_inputs(self.bounds_inputs),
                                                };
                                                if let Some(existing) = preset.areas.iter_mut().find(|x| x.name == aname) {
                                                    *existing = area;
                                                } else {
                                                    preset.areas.push(area);
                                                }
                                                preset.areas.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                                                self.selected_area = Some(aname);
                                                let _ = self.preset_store.save();
                                            }
                                        }
                                    }
                                });

                                ui.horizontal(|ui| {
                                    let has_area = self.selected_area.is_some();

                                    if ui.add_enabled(has_area, egui::Button::new("Load area → bounds")).clicked() {
                                        if let Some(aname) = self.selected_area.clone() {
                                            if let Some(preset) = self.preset_store.presets.iter().find(|p| p.name == pname) {
                                                if let Some(a) = preset.areas.iter().find(|x| x.name == aname) {
                                                    self.apply_boundsserde(a.bounds);
                                                }
                                            }
                                        }
                                    }

                                    if ui.add_enabled(has_area, egui::Button::new("Save bounds → area")).clicked() {
                                        if let Some(aname) = self.selected_area.clone() {
                                            if let Some(preset) = self.preset_store.presets.iter_mut().find(|p| p.name == pname.clone()) {
                                                if let Some(a) = preset.areas.iter_mut().find(|x| x.name == aname) {
                                                    a.bounds = crate::presets::BoundsSerde::from_inputs(self.bounds_inputs);
                                                    let _ = self.preset_store.save();
                                                }
                                            }
                                        }
                                    }

                                    if ui.add_enabled(has_area, egui::Button::new("Delete area")).clicked() {
                                        if let Some(aname) = self.selected_area.clone() {
                                            if let Some(preset) = self.preset_store.presets.iter_mut().find(|p| p.name == pname.clone()) {
                                                preset.areas.retain(|x| x.name != aname);
                                                self.selected_area = None;
                                                let _ = self.preset_store.save();
                                            }
                                        }
                                    }
                                });

                                // Quick list view
                                ui.separator();
                                ui.label("Areas in this preset:");
                                if let Some(preset) = self.preset_store.presets.iter().find(|p| p.name == pname) {
                                    for a in &preset.areas {
                                        ui.monospace(format!(
                                            "{}: min({}, {}) max({}, {})",
                                            a.name, a.bounds.min_x, a.bounds.min_y, a.bounds.max_x, a.bounds.max_y
                                        ));
                                    }
                                }
                            } else {
                                ui.monospace("Select a preset to manage areas.");
                            }
                        });



                        
                        ui.separator();
                        
                        // Points
                        ui.label("Named points (for button locations)");
                        ui.horizontal(|ui| {
                            ui.label("Point name:");
                            ui.text_edit_singleline(&mut self.new_point_name);

                            let can_pick = self.selected_preset.is_some() && !self.new_point_name.trim().is_empty();
                            if ui.add_enabled(can_pick, egui::Button::new("Pick point")).clicked() {
                                // reuse picker window behavior (fullscreen borderless)
                                self.picking_point = true;
                                self.enter_picker(ctx);
                            }
                        });

                        // show existing points
                        if let Some(sel) = self.selected_preset.clone() {
                            if let Some(p) = self.preset_store.presets.iter().find(|p| p.name == sel) {
                                let point_names: Vec<(String, i32, i32)> = p.points.iter()
                                    .map(|pt| (pt.name.clone(), pt.x, pt.y))
                                    .collect();
                                
                                for (i, (pt_name, x, y)) in point_names.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.monospace(format!("{}: ({}, {})", pt_name, x, y));
                                        if ui.button("✕").clicked() {
                                            if let Some(preset) = self.preset_store.presets.iter_mut().find(|p| p.name == sel) {
                                                preset.points.remove(i);
                                                let _ = self.preset_store.save();
                                            }
                                        }
                                    });
                                }
                            }
                        } else {
                            ui.monospace("Select a preset to add points.");
                        }
                    });

                    ui.separator();

                    ui.group(|ui| {
                        ui.label("Settings");
                        ui.horizontal(|ui| {
                            ui.label("Click type:");
                            ui.checkbox(&mut self.click_button_left, "Left");
                            let mut right = !self.click_button_left;
                            if ui.checkbox(&mut right, "Right").clicked() { self.click_button_left = !right; }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Interval (seconds):");
                            ui.add(egui::DragValue::new(&mut self.min_secs).speed(0.1));
                            ui.label("to");
                            ui.add(egui::DragValue::new(&mut self.max_secs).speed(0.1));
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.use_finite_clicks, "Limit number of clicks");
                            if self.use_finite_clicks {
                                ui.add(egui::DragValue::new(&mut self.num_clicks).speed(1.0).clamp_range(1..=1000000));
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Start").clicked() { self.start(); }
                            if ui.button("Pause").clicked() { self.pause(); }
                            if ui.button("Stop").clicked() { self.stop(); }
                        });

                        if let Some(job) = &self.job {
                            let running = job.running.load(Ordering::Relaxed);
                            ui.label(format!("Status: {}", if running {"Running"} else {"Stopped"}));
                        } else {
                            ui.label("Status: Stopped");
                        }
                    });

                    ui.separator();

                    ui.group(|ui| {
                        ui.label("Click Sequences");

                        // Sequence selector
                        let selected_text = self.selected_sequence.clone().unwrap_or_else(|| "None".into());
                        egui::ComboBox::from_id_source("sequence_select")
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.selected_sequence, None, "None");
                                for s in &self.preset_store.sequences {
                                    ui.selectable_value(&mut self.selected_sequence, Some(s.name.clone()), &s.name);
                                }
                            });

                        ui.horizontal(|ui| {
                            if ui.button("Load sequence").clicked() {
                                // Nothing to do here; sequence is already loaded when Start is clicked
                                eprintln!("Sequence selected, ready to run");
                            }

                            if ui.button("Delete sequence").clicked() {
                                if let Some(sel) = self.selected_sequence.clone() {
                                    self.preset_store.remove_sequence_by_name(&sel);
                                    let _ = self.preset_store.save();
                                    self.selected_sequence = None;
                                }
                            }
                        });

                        ui.separator();

                        // Create new sequence
                        ui.horizontal(|ui| {
                            ui.label("New sequence name:");
                            ui.text_edit_singleline(&mut self.new_sequence_name);

                            let can_create = !self.new_sequence_name.trim().is_empty() && self.selected_preset.is_some();
                            if ui.add_enabled(can_create, egui::Button::new("Create from preset areas")).clicked() {
                                let name = self.new_sequence_name.trim().to_string();
                                let preset_name = self.selected_preset.clone().unwrap();
                                if let Some(preset) = self.preset_store.presets.iter().find(|p| p.name == preset_name) {
                                    // Create a sequence with one step per area
                                    let steps = preset.areas.iter().map(|a| SequenceStep {
                                        area_name: a.name.clone(),
                                        min_interval: 1.0,
                                        max_interval: 2.0,
                                        interval_secs: 1.5,
                                        button_type: "Left".to_string(),
                                    }).collect();
                                    
                                    let sequence = ClickSequence {
                                        name: name.clone(),
                                        preset_name: Some(preset_name),
                                        steps,
                                    };
                                    
                                    self.preset_store.upsert_sequence(sequence);
                                    let _ = self.preset_store.save();
                                    self.new_sequence_name.clear();
                                    self.selected_sequence = Some(name);
                                }
                            }
                        });

                        ui.separator();

                        // Edit current sequence steps
                        if let Some(sel) = self.selected_sequence.clone() {
                            // Collect step info to avoid borrow issues
                            let step_info: Vec<(String, f32, f32, f32, String)> = self.preset_store.get_sequence(&sel)
                                .map(|s| s.steps.iter().map(|st| (st.area_name.clone(), st.min_interval, st.max_interval, st.interval_secs, st.button_type.clone())).collect())
                                .unwrap_or_default();
                            
                            if !step_info.is_empty() {
                                ui.label(format!("Editing sequence: {}", sel));
                                ui.label("Steps:");
                                
                                for (i, (area_name, _min, _max, _interval, _button)) in step_info.iter().enumerate() {
                                    ui.vertical(|ui| {
                                        ui.label(format!("Step {}: {} ", i + 1, area_name));
                                        
                                        if let Some(seq_mut) = self.preset_store.get_sequence_mut(&sel) {
                                            if let Some(step) = seq_mut.steps.get_mut(i) {
                                                // Min interval section
                                                ui.label("Min Interval:");
                                                ui.horizontal(|ui| {
                                                    // Vertical button stack
                                                    ui.vertical(|ui| {
                                                        if ui.button("◀ 50ms").clicked() { step.min_interval = (step.min_interval - 0.05).max(0.05); }
                                                        if ui.button("◀ 1s").clicked() { step.min_interval = (step.min_interval - 1.0).max(0.05); }
                                                        if ui.button("◀ 5s").clicked() { step.min_interval = (step.min_interval - 5.0).max(0.05); }
                                                    });
                                                    // Large value display
                                                    ui.add(egui::DragValue::new(&mut step.min_interval).speed(0.05));
                                                    // Vertical button stack
                                                    ui.vertical(|ui| {
                                                        if ui.button("50ms ▶").clicked() { step.min_interval += 0.05; }
                                                        if ui.button("1s ▶").clicked() { step.min_interval += 1.0; }
                                                        if ui.button("5s ▶").clicked() { step.min_interval += 5.0; }
                                                    });
                                                });
                                                
                                                // Max interval section
                                                ui.label("Max Interval:");
                                                ui.horizontal(|ui| {
                                                    // Vertical button stack
                                                    ui.vertical(|ui| {
                                                        if ui.button("◀ 50ms").clicked() { step.max_interval = (step.max_interval - 0.05).max(0.05); }
                                                        if ui.button("◀ 1s").clicked() { step.max_interval = (step.max_interval - 1.0).max(0.05); }
                                                        if ui.button("◀ 5s").clicked() { step.max_interval = (step.max_interval - 5.0).max(0.05); }
                                                    });
                                                    // Large value display
                                                    ui.add(egui::DragValue::new(&mut step.max_interval).speed(0.05));
                                                    // Vertical button stack
                                                    ui.vertical(|ui| {
                                                        if ui.button("50ms ▶").clicked() { step.max_interval += 0.05; }
                                                        if ui.button("1s ▶").clicked() { step.max_interval += 1.0; }
                                                        if ui.button("5s ▶").clicked() { step.max_interval += 5.0; }
                                                    });
                                                });
                                                
                                                ui.label("Button:");
                                                let mut button_str = step.button_type.clone();
                                                if ui.selectable_value(&mut button_str, "Left".to_string(), "Left").changed() {
                                                    step.button_type = button_str.clone();
                                                }
                                                if ui.selectable_value(&mut button_str, "Right".to_string(), "Right").changed() {
                                                    step.button_type = button_str;
                                                }
                                                
                                                ui.separator();
                                            }
                                        }
                                    });
                                }
                                
                                if ui.button("Save sequence changes").clicked() {
                                    let _ = self.preset_store.save();
                                }
                            } else {
                                ui.monospace("No steps in selected sequence.");
                            }
                        } else {
                            ui.monospace("Select a sequence to edit.");
                        }
                    });
                        });
                    });
            });

            // Preview rectangle
            if let Some(b) = self.config.lock().bounds {
                let info = format!("Active bounds: x=[{}..{}], y=[{}..{}] ({}x{})",
                                   b.min_x, b.max_x, b.min_y, b.max_y, b.width(), b.height());
                ui.separator();
                ui.monospace(info);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Pos2;
    use std::sync::{Arc, atomic::Ordering};
    use std::time::Duration;

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
        }));

        let job = ClickJob::spawn(Arc::clone(&config));
        assert!(job.running.load(Ordering::Relaxed));

        // Test stopping
        job.stop();
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

        // input defaults
        assert_eq!(state.min_secs, 2.0);
        assert_eq!(state.max_secs, 4.5);
    }

    #[test]
    fn test_set_bounds_from_drag_ppp1_origin0() {
        let mut state = AppState::default();
        state.drag_start = Some(Pos2::new(100.0, 100.0));
        state.drag_end   = Some(Pos2::new(200.0, 200.0));
        state.set_bounds_from_drag(1.0, (0, 0));
        assert_eq!(state.bounds_inputs, [100, 200, 100, 200]);

        // reverse drag
        state.drag_start = Some(Pos2::new(200.0, 200.0));
        state.drag_end   = Some(Pos2::new(100.0, 100.0));
        state.set_bounds_from_drag(1.0, (0, 0));
        assert_eq!(state.bounds_inputs, [100, 200, 100, 200]);
    }

    #[test]
    fn test_click_interval() {
        let config = Arc::new(Mutex::new(ClickConfig {
            bounds: Some(Bounds { min_x: 100, max_x: 200, min_y: 100, max_y: 200 }),
            button: ClickButton::Left,
            min_secs: 0.1,
            max_secs: 0.2,
        }));

        let job = ClickJob::spawn(Arc::clone(&config));
        std::thread::sleep(Duration::from_millis(300));
        job.stop();
        assert!(!job.running.load(Ordering::Relaxed));
    }
}

fn main() -> eframe::Result<()> {
    let mut opts = eframe::NativeOptions::default();
    let _args = Args::parse(); // Arguments will be used later

    // Start as a normal window; we resize/position during picking.
    opts.viewport.transparent = Some(true);
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
