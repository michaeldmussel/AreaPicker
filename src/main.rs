use eframe::{egui, egui::{Color32, Pos2, Rect, Sense, WindowLevel}};
use enigo::{MouseButton, MouseControllable};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rand::{Rng, rngs::StdRng, SeedableRng};
use std::{thread, time::Duration};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use std::f32::consts::PI;

// Human Mouse Movement Settings
struct HumanMouseSettings {
    avg_speed: f32,
    speed_jitter: f32,
    micro_jitter_px: f32,
    micro_jitter_hz: f32,
    overshoot_chance: f32,
    overshoot_px: f32,
    min_pause_ms: u64,
    max_pause_ms: u64,
    rng_seed: Option<u64>,
}

impl Default for HumanMouseSettings {
    fn default() -> Self {
        Self {
            avg_speed: 1400.0,
            speed_jitter: 0.25,
            micro_jitter_px: 0.6,
            micro_jitter_hz: 9.0,
            overshoot_chance: 0.25,
            overshoot_px: 12.0,
            min_pause_ms: 15,
            max_pause_ms: 60,
            rng_seed: None,
        }
    }
}

fn ease_in_out(t: f32) -> f32 {
    0.5 - 0.5 * (PI * t).cos()
}

fn cubic_bezier(p0: (f32,f32), p1: (f32,f32), p2: (f32,f32), p3: (f32,f32), t: f32) -> (f32,f32) {
    let u = 1.0 - t;
    let uu = u * u;
    let tt = t * t;
    let uuu = uu * u;
    let ttt = tt * t;
    (
        uuu*p0.0 + 3.0*uu*t*p1.0 + 3.0*u*tt*p2.0 + ttt*p3.0,
        uuu*p0.1 + 3.0*uu*t*p1.1 + 3.0*u*tt*p2.1 + ttt*p3.1,
    )
}

fn len((x1,y1): (f32,f32), (x2,y2): (f32,f32)) -> f32 {
    ((x2-x1).hypot(y2-y1)).max(1.0)
}

fn make_bezier_with_wiggle(
    from: (i32,i32), 
    to: (i32,i32), 
    rng: &mut impl Rng
) -> ((f32,f32),(f32,f32),(f32,f32),(f32,f32)) {
    let p0 = (from.0 as f32, from.1 as f32);
    let p3 = (to.0 as f32, to.1 as f32);

    let dx = p3.0 - p0.0;
    let dy = p3.1 - p0.1;
    let dist = len(p0, p3);
    let (nx, ny) = if dist > 0.0 { (-dy / dist, dx / dist) } else { (0.0, 0.0) };

    let cdist = 0.25 * dist;
    let amp1 = rng.gen_range(-0.12..0.12) * dist;
    let amp2 = rng.gen_range(-0.12..0.12) * dist;

    let p1 = (p0.0 + dx * 0.30 + nx * amp1, p0.1 + dy * 0.30 + ny * amp1);
    let p2 = (p0.0 + dx * 0.70 + nx * amp2, p0.1 + dy * 0.70 + ny * amp2);

    let p1 = (p1.0 + (dx / dist) * (cdist * 0.1), p1.1 + (dy / dist) * (cdist * 0.1));
    let p2 = (p2.0 - (dx / dist) * (cdist * 0.1), p2.1 - (dy / dist) * (cdist * 0.1));

    (p0, p1, p2, p3)
}

fn human_move_and_click(
    enigo: &mut impl MouseControllable,
    from: (i32,i32),
    to: (i32,i32),
    bounds: Option<Bounds>,
    settings: &HumanMouseSettings,
    button: MouseButton,
) {
    let mut rng: StdRng = match settings.rng_seed {
        Some(seed) => StdRng::seed_from_u64(seed),
        None => StdRng::from_entropy(),
    };

    let (p0, p1, p2, p3) = make_bezier_with_wiggle(from, to, &mut rng);
    let distance = len(p0, p3);
    let speed_variation = 1.0 + settings.speed_jitter * rng.gen_range(-1.0..1.0);
    let px_per_sec = (settings.avg_speed * speed_variation).max(200.0);
    let total_ms = ((distance / px_per_sec) * 1000.0).clamp(60.0, 1600.0) as u64;

    let step_ms = rng.gen_range(8..=12);
    let steps = (total_ms / step_ms.max(1)).max(3) as usize;
    let maybe_pause_at = if rng.gen::<f32>() < 0.25 {
        Some(rng.gen_range(steps/3..(2*steps/3).max(steps/3+1)))
    } else {
        None
    };

    let jitter_amp = settings.micro_jitter_px;
    let jitter_hz = (settings.micro_jitter_hz * (1.0 + rng.gen_range(-0.2..0.2))).max(1.0);

    for i in 0..=steps {
        let raw_t = i as f32 / steps as f32;
        let t = ease_in_out(raw_t);
        let (mut x, mut y) = cubic_bezier(p0, p1, p2, p3, t);

        let w = 2.0 * PI * jitter_hz * (i as f32 * (step_ms as f32 / 1000.0));
        let jitter = w.sin() * jitter_amp + rng.gen_range(-jitter_amp..jitter_amp) * 0.25;

        let tp = cubic_bezier(p0, p1, p2, p3, (t + 1.0/steps as f32).min(1.0));
        let dx = tp.0 - x;
        let dy = tp.1 - y;
        let d = (dx*dx + dy*dy).sqrt().max(1.0);
        let (nx, ny) = (-dy/d, dx/d);
        x += nx * jitter;
        y += ny * jitter;

        let (mut xi, mut yi) = (x.round() as i32, y.round() as i32);
        if let Some(b) = bounds {
            if xi < b.min_x { xi = b.min_x; }
            if xi > b.max_x { xi = b.max_x; }
            if yi < b.min_y { yi = b.min_y; }
            if yi > b.max_y { yi = b.max_y; }
        }

        enigo.mouse_move_to(xi, yi);

        if let Some(pause_idx) = maybe_pause_at {
            if i == pause_idx {
                thread::sleep(Duration::from_millis(
                    rng.gen_range(settings.min_pause_ms..=settings.max_pause_ms)
                ));
            }
        }

        thread::sleep(Duration::from_millis(step_ms));
    }

    enigo.mouse_down(button);
    thread::sleep(Duration::from_millis(20 + rand::thread_rng().gen_range(0..50)));
    enigo.mouse_up(button);
}
// -------------- Click Engine --------------
#[derive(Clone, Copy, Debug)]
enum ClickButton { Left, Right }

#[derive(Clone, Copy, Debug)]
struct Bounds { min_x: i32, max_x: i32, min_y: i32, max_y: i32 }

impl Bounds {
    fn width(&self) -> i32 { self.max_x - self.min_x }
    fn height(&self) -> i32 { self.max_y - self.min_y }
    fn is_valid(&self) -> bool { self.width() > 0 && self.height() > 0 }
}

struct ClickJob {
    running: Arc<AtomicBool>,
    #[allow(dead_code)] // Used through Arc clone in spawn
    config: Arc<Mutex<ClickConfig>>,
}

#[derive(Clone, Debug)]
struct ClickConfig {
    bounds: Option<Bounds>,
    button: ClickButton,
    min_secs: f32,
    max_secs: f32,
}

static ENIGO: Lazy<Mutex<enigo::Enigo>> = Lazy::new(|| Mutex::new(enigo::Enigo::new()));

impl ClickJob {
    fn spawn(config: Arc<Mutex<ClickConfig>>) -> Self {
        use std::sync::atomic::AtomicBool;
        use std::sync::atomic::Ordering;
        use std::sync::{Arc};
        use std::time::Duration;
        use rand::Rng;
        use enigo::{MouseButton};

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);
        let config_clone = Arc::clone(&config);

        eprintln!("Starting click job with config: {:?}", config.lock());

        std::thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let mut last_pos: Option<(i32,i32)> = None;

            loop {
                if !running_clone.load(Ordering::Relaxed) { break; }

                let cfg = config_clone.lock().clone();
                let Some(b) = cfg.bounds else {
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                };
                if !b.is_valid() {
                    std::thread::sleep(Duration::from_millis(200));
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

                // sleep random between min..max (seconds), while checking stop flag
                let (min_s, max_s) = if cfg.min_secs <= cfg.max_secs {
                    (cfg.min_secs, cfg.max_secs)
                } else { (cfg.max_secs, cfg.min_secs) };
                let wait = rng.gen_range(min_s..=max_s).max(0.01);
                let ms = (wait * 1000.0) as u64;
                for _ in 0..ms/50 {
                    if !running_clone.load(Ordering::Relaxed) { break; }
                    std::thread::sleep(Duration::from_millis(50));
                }
                if ms % 50 != 0 { std::thread::sleep(Duration::from_millis(ms % 50)); }
            }
        });

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

            monitors,
            display_choice: DisplayChoice::All,

            bounds_inputs: [100, 400, 100, 400],
            click_button_left: true,
            min_secs: 2.0,
            max_secs: 4.5,

            job: None,
            config: Arc::new(Mutex::new(ClickConfig{
                bounds: Some(Bounds{min_x:100, max_x:400, min_y:100, max_y:400}),
                button: ClickButton::Left,
                min_secs: 2.0,
                max_secs: 4.5,
            })),
        }
    }
}

impl AppState {
    fn start(&mut self) {
        if self.job.is_some() { return; }
        let mut cfg = self.config.lock();
        cfg.button = if self.click_button_left { ClickButton::Left } else { ClickButton::Right };
        cfg.min_secs = self.min_secs;
        cfg.max_secs = self.max_secs;
        cfg.bounds = Some(Bounds{
            min_x: self.bounds_inputs[0],
            max_x: self.bounds_inputs[1],
            min_y: self.bounds_inputs[2],
            max_y: self.bounds_inputs[3],
        });
        drop(cfg);
        self.job = Some(ClickJob::spawn(Arc::clone(&self.config)));
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

        ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(outer));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inner));
        // Note: using borderless large window; not true OS fullscreen to avoid monitor switching quirks.
    }

    fn exit_picker(&mut self, ctx: &egui::Context) {
        self.picking_area = false;
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
            self.config.lock().bounds = Some(Bounds{min_x, max_x, min_y, max_y});
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

        // -------- Main UI --------
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.heading("Area Clicker — Multi-Display");
        });

        egui::CentralPanel::default().show(ctx, |ui| {
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
