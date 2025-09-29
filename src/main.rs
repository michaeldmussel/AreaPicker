mod human_mouse;

use clap::Parser;
use eframe::{egui, egui::{Color32, Pos2, Rect, Sense, WindowLevel}};
use enigo::{self, MouseButton, MouseControllable};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rand::Rng;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use std::time::Duration;

use crate::human_mouse::{HumanMouseSettings, Bounds, human_move_and_click};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Total number of random clicks to perform before stopping (0 = infinite)
    #[arg(long = "clicks", default_value_t = 0)]
    clicks: u32,

    /// Optional min delay between clicks in ms (legacy; you can use the UI now)
    #[arg(long = "min-delay-ms", default_value_t = 75)]
    min_delay_ms: u64,

    /// Optional max delay between clicks in ms (legacy; you can use the UI now)
    #[arg(long = "max-delay-ms", default_value_t = 250)]
    max_delay_ms: u64,
}

/* ------------------ Click / Sequence Engine ------------------- */
#[derive(Clone, Copy, Debug)]
enum Action {
    Edit(usize),
    MoveUp(usize),
    MoveDown(usize),
    Remove(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClickButton { Left, Right }

impl ClickButton {
    fn to_enigo(self) -> MouseButton {
        match self { ClickButton::Left => MouseButton::Left, ClickButton::Right => MouseButton::Right }
    }
}

#[derive(Clone, Debug)]
struct SequenceStep {
    name: String,
    bounds: Bounds,
    clicks: u32,          // how many clicks to perform in this step
    button: ClickButton,  // which button to use
    min_secs: f32,        // interval lower bound
    max_secs: f32,        // interval upper bound
}

#[derive(Clone, Debug)]
enum JobMode {
    Single {
        bounds: Option<Bounds>,
        button: ClickButton,
        min_secs: f32,
        max_secs: f32,
        finite_clicks: Option<u32>, // None=infinite
    },
    Sequence {
        steps: Vec<SequenceStep>,
        cycles: Option<u32>, // None=infinite, Some(n) times through all steps
    },
}

#[derive(Clone, Debug)]
struct ClickConfig {
    mode: JobMode,
}

struct ClickJob {
    running: Arc<AtomicBool>,
    #[allow(dead_code)]
    config: Arc<Mutex<ClickConfig>>,
}

static ENIGO: Lazy<Mutex<enigo::Enigo>> = Lazy::new(|| Mutex::new(enigo::Enigo::new()));

impl ClickJob {
    fn spawn(config: Arc<Mutex<ClickConfig>>) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);
        let config_clone = Arc::clone(&config);

        std::thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let mut last_pos: Option<(i32, i32)> = None;

            // human-ish default settings; tweak as desired
            let settings = HumanMouseSettings {
                avg_speed: 1400.0,
                speed_jitter: 0.25,
                micro_jitter_px: 0.6,
                micro_jitter_hz: 9.0,
                overshoot_chance: 0.25,
                overshoot_px: 4.0,
                min_pause_ms: 22,
                max_pause_ms: 55,
                rng_seed: None,
            };

            let do_click = |from_opt: &mut Option<(i32, i32)>,
                            b: Bounds,
                            btn: ClickButton,
                            _rng: &mut rand::rngs::ThreadRng| {
                let (x, y) = (
                    _rng.gen_range(b.min_x..=b.max_x),
                    _rng.gen_range(b.min_y..=b.max_y)
                );

                {
                    let mut en = ENIGO.lock();
                    let from = from_opt.unwrap_or((b.min_x - 40, b.min_y - 40));
                    human_move_and_click(
                        &mut *en,
                        from,
                        (x, y),
                        Some(b),
                        &settings,
                        btn.to_enigo(),
                    );
                }
                *from_opt = Some((x, y));
            };

            let sleep_between = |running_flag: &Arc<AtomicBool>, secs: f32, _rng: &mut rand::rngs::ThreadRng| {
                let wait = secs.max(0.01);
                let ms = (wait * 1000.0) as u64;
                let mut left = ms;
                while left > 0 {
                    if !running_flag.load(Ordering::Relaxed) { break; }
                    let step = left.min(50);
                    std::thread::sleep(Duration::from_millis(step));
                    left -= step;
                }
            };

            'outer: loop {
                if !running_clone.load(Ordering::Relaxed) { break; }

                let cfg = config_clone.lock().clone();
                match cfg.mode {
                    JobMode::Single { bounds, button, min_secs, max_secs, mut finite_clicks } => {
                        let Some(b) = bounds else {
                            std::thread::sleep(Duration::from_millis(150));
                            continue 'outer;
                        };
                        if !b.is_valid() {
                            std::thread::sleep(Duration::from_millis(150));
                            continue 'outer;
                        }

                        // loop until stopped or finite clicks reach zero
                        loop {
                            if !running_clone.load(Ordering::Relaxed) { break 'outer; }
                            if let Some(0) = finite_clicks { break 'outer; }

                            do_click(&mut last_pos, b, button, &mut rng);

                            if let Some(ref mut remaining) = finite_clicks {
                                *remaining = remaining.saturating_sub(1);
                            }

                            let (min_s, max_s) = if min_secs <= max_secs {(min_secs, max_secs)} else {(max_secs, min_secs)};
                            let next = rng.gen_range(min_s..=max_s);
                            sleep_between(&running_clone, next, &mut rng);
                        }
                    }

                    JobMode::Sequence { steps, mut cycles } => {
                        if steps.is_empty() {
                            std::thread::sleep(Duration::from_millis(150));
                            continue 'outer;
                        }

                        // cycles: None = infinite; Some(n) = execute n times through the whole list
                        'cycles: loop {
                            if !running_clone.load(Ordering::Relaxed) { break 'outer; }

                            for step in &steps {
                                if !running_clone.load(Ordering::Relaxed) { break 'outer; }
                                if !step.bounds.is_valid() { continue; }

                                let (min_s, max_s) = if step.min_secs <= step.max_secs {
                                    (step.min_secs, step.max_secs)
                                } else { (step.max_secs, step.min_secs) };

                                for _ in 0..step.clicks {
                                    if !running_clone.load(Ordering::Relaxed) { break 'outer; }
                                    do_click(&mut last_pos, step.bounds, step.button, &mut rng);
                                    let next = rng.gen_range(min_s..=max_s);
                                    sleep_between(&running_clone, next, &mut rng);
                                }
                            }

                            if let Some(ref mut c) = cycles {
                                if *c == 0 { break 'outer; }
                                *c = c.saturating_sub(1);
                                if *c == 0 { break 'outer; }
                            } else {
                                // infinite cycles
                                continue 'cycles;
                            }
                        }
                    }
                }
            }
        });

        Self { running, config }
    }

    fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

/* -------------------- Display Information --------------------- */
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
                    name: if d.is_primary {
                        format!("Display {} (Primary)", d.id)
                    } else {
                        format!("Display {}", d.id)
                    },
                    origin_px: (d.x, d.y),
                    size_px: (d.width as i32, d.height as i32),
                    scale_factor: d.scale_factor as f32,
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
    if monitors.is_empty() { (0, 0, 0, 0) } else { (min_x, min_y, max_x, max_y) }
}

#[derive(Clone, Copy, PartialEq)]
enum DisplayChoice { All, One(usize) }

/* -------------------------- App State ------------------------- */
struct AppState {
    // Picker overlay
    picking_area: bool,
    drag_start: Option<Pos2>,
    drag_end: Option<Pos2>,

    // Displays
    monitors: Vec<Monitor>,
    display_choice: DisplayChoice,

    // Single-click mode inputs
    bounds_inputs: [i32; 4], // min_x, max_x, min_y, max_y
    click_button_left: bool,
    min_secs: f32,
    max_secs: f32,
    use_finite_clicks: bool,
    num_clicks: u32,

    // Sequence editor
    seq_steps: Vec<SequenceStep>,
    seq_cycles: Option<u32>,
    // transient editor fields to add/update a step
    editing_step_idx: Option<usize>,
    edit_name: String,
    edit_clicks: u32,
    edit_min_secs: f32,
    edit_max_secs: f32,
    edit_button_left: bool,
    edit_bounds_from_current: bool, // if true, will take from current bounds on "Apply"

    // Engine
    job: Option<ClickJob>,
    config: Arc<Mutex<ClickConfig>>,
}

impl Default for AppState {
    fn default() -> Self {
        let monitors = query_monitors();
        let default_bounds = Bounds { min_x: 100, max_x: 400, min_y: 100, max_y: 400 };
        Self {
            // picker
            picking_area: false,
            drag_start: None,
            drag_end: None,

            // displays
            monitors,
            display_choice: DisplayChoice::All,

            // single mode
            bounds_inputs: [100, 400, 100, 400],
            click_button_left: true,
            min_secs: 2.0,
            max_secs: 4.5,
            use_finite_clicks: false,
            num_clicks: 100,

            // sequence
            seq_steps: vec![],
            seq_cycles: Some(1),
            editing_step_idx: None,
            edit_name: "Step 1".into(),
            edit_clicks: 5,
            edit_min_secs: 0.5,
            edit_max_secs: 1.5,
            edit_button_left: true,
            edit_bounds_from_current: true,

            // engine
            job: None,
            config: Arc::new(Mutex::new(ClickConfig {
                mode: JobMode::Single {
                    bounds: Some(default_bounds),
                    button: ClickButton::Left,
                    min_secs: 2.0,
                    max_secs: 4.5,
                    finite_clicks: None,
                }
            })),
        }
    }
}

impl AppState {
    /* ------------------- Engine controls ------------------- */
    fn start_single(&mut self) {
        if self.job.is_some() { return; }
        let bounds = Bounds {
            min_x: self.bounds_inputs[0],
            max_x: self.bounds_inputs[1],
            min_y: self.bounds_inputs[2],
            max_y: self.bounds_inputs[3],
        };
        let mode = JobMode::Single {
            bounds: Some(bounds),
            button: if self.click_button_left { ClickButton::Left } else { ClickButton::Right },
            min_secs: self.min_secs,
            max_secs: self.max_secs,
            finite_clicks: if self.use_finite_clicks { Some(self.num_clicks) } else { None },
        };
        self.config.lock().mode = mode;
        self.job = Some(ClickJob::spawn(Arc::clone(&self.config)));
    }

    fn start_sequence(&mut self) {
        if self.job.is_some() { return; }
        if self.seq_steps.is_empty() { return; }
        let mode = JobMode::Sequence {
            steps: self.seq_steps.clone(),
            cycles: self.seq_cycles,
        };
        self.config.lock().mode = mode;
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

    /* ------------------- Display mgmt ---------------------- */
    fn refresh_monitors(&mut self) {
        self.monitors = query_monitors();
        if let DisplayChoice::One(i) = self.display_choice {
            if i >= self.monitors.len() {
                self.display_choice = DisplayChoice::All;
            }
        }
    }

    /* ------------------- Picker overlay -------------------- */
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
                    let (min_x, min_y, max_x, max_y) = union_rect(&self.monitors);
                    ((min_x, min_y), (max_x - min_x, max_y - min_y))
                }
            }
        };

        // convert to LOGICAL points
        let ppp = ctx.pixels_per_point().max(0.1);
        let inner = egui::vec2(size_px.0 as f32 / ppp, size_px.1 as f32 / ppp);
        let outer = egui::pos2(origin_px.0 as f32 / ppp, origin_px.1 as f32 / ppp);

        ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(outer));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inner));
    }

    fn exit_picker(&mut self, ctx: &egui::Context) {
        self.picking_area = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(WindowLevel::Normal));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(740.0, 560.0)));
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

            // keep single-mode config in sync for preview
            if let JobMode::Single { bounds, .. } = &mut self.config.lock().mode {
                *bounds = Some(Bounds { min_x, max_x, min_y, max_y });
            }
            eprintln!("Selected bounds (px): x=[{}..{}], y=[{}..{}]", min_x, max_x, min_y, max_y);
        }
    }

    /* ------------------- Sequence editor ------------------- */
    fn push_or_update_step_from_editor(&mut self) {
        let b = if self.edit_bounds_from_current {
            Bounds {
                min_x: self.bounds_inputs[0],
                max_x: self.bounds_inputs[1],
                min_y: self.bounds_inputs[2],
                max_y: self.bounds_inputs[3],
            }
        } else {
            // if not taking from current, keep previous bounds when editing; or fallback to current
            if let Some(idx) = self.editing_step_idx {
                self.seq_steps.get(idx).map(|s| s.bounds).unwrap_or(Bounds {
                    min_x: self.bounds_inputs[0],
                    max_x: self.bounds_inputs[1],
                    min_y: self.bounds_inputs[2],
                    max_y: self.bounds_inputs[3],
                })
            } else {
                Bounds {
                    min_x: self.bounds_inputs[0],
                    max_x: self.bounds_inputs[1],
                    min_y: self.bounds_inputs[2],
                    max_y: self.bounds_inputs[3],
                }
            }
        };

        let step = SequenceStep {
            name: self.edit_name.clone(),
            bounds: b,
            clicks: self.edit_clicks.max(1),
            button: if self.edit_button_left { ClickButton::Left } else { ClickButton::Right },
            min_secs: self.edit_min_secs,
            max_secs: self.edit_max_secs,
        };

        if let Some(idx) = self.editing_step_idx {
            if idx < self.seq_steps.len() {
                self.seq_steps[idx] = step;
            }
        } else {
            self.seq_steps.push(step);
            // increment default name for convenience
            let next_num = self.seq_steps.len() + 1;
            self.edit_name = format!("Step {}", next_num);
        }
        self.editing_step_idx = None;
    }

    fn select_step_for_edit(&mut self, idx: usize) {
        if let Some(s) = self.seq_steps.get(idx).cloned() {
            self.editing_step_idx = Some(idx);
            self.edit_name = s.name;
            self.edit_clicks = s.clicks;
            self.edit_min_secs = s.min_secs;
            self.edit_max_secs = s.max_secs;
            self.edit_button_left = s.button == ClickButton::Left;
            self.edit_bounds_from_current = false;
        }
    }

    fn move_step_up(&mut self, idx: usize) {
        if idx > 0 && idx < self.seq_steps.len() {
            self.seq_steps.swap(idx, idx - 1);
            if let Some(sel) = self.editing_step_idx.as_mut() {
                if *sel == idx { *sel = idx - 1; }
                else if *sel == idx - 1 { *sel = idx; }
            }
        }
    }

    fn move_step_down(&mut self, idx: usize) {
        if idx + 1 < self.seq_steps.len() {
            self.seq_steps.swap(idx, idx + 1);
            if let Some(sel) = self.editing_step_idx.as_mut() {
                if *sel == idx { *sel = idx + 1; }
                else if *sel == idx + 1 { *sel = idx; }
            }
        }
    }
}

/* -------------------------- UI ------------------------------- */
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

            // keep repainting while overlay is active (important on Windows)
            ctx.request_repaint();
            return; // Skip main UI while picking
        }

        // -------- Main UI --------
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.heading("Area Clicker — Multi-Display & Sequence Runner");
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns(2, |cols| {
                /* ---------------- Left: Display + Single Mode ---------------- */
                let ui = &mut cols[0];

                ui.group(|ui| {
                    ui.heading("Display / Area");
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

                    ui.horizontal(|ui| {
                        if ui.button("Pick Area (drag a rectangle)").clicked() {
                            self.enter_picker(ctx);
                        }
                        if let Some(b) = match &self.config.lock().mode {
                            JobMode::Single { bounds, .. } => *bounds,
                            JobMode::Sequence { .. } => None,
                        } {
                            ui.label(format!("Active: {}x{} @ [{},{}]..[{},{}]",
                                b.width(), b.height(), b.min_x, b.min_y, b.max_x, b.max_y));
                        }
                    });
                });

                ui.add_space(6.0);

                ui.group(|ui| {
                    ui.heading("Single Mode");
                    ui.horizontal(|ui| {
                        ui.label("Click button:");
                        ui.checkbox(&mut self.click_button_left, "Left");
                        let mut right = !self.click_button_left;
                        if ui.checkbox(&mut right, "Right").clicked() { self.click_button_left = !right; }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Interval (sec):");
                        ui.add(egui::DragValue::new(&mut self.min_secs).speed(0.1));
                        ui.label("to");
                        ui.add(egui::DragValue::new(&mut self.max_secs).speed(0.1));
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.use_finite_clicks, "Limit clicks");
                        if self.use_finite_clicks {
                            ui.add(egui::DragValue::new(&mut self.num_clicks).speed(1.0).clamp_range(1..=1_000_000));
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Start Single").clicked() { self.start_single(); }
                        if ui.button("Pause").clicked() { self.pause(); }
                        if ui.button("Stop").clicked() { self.stop(); }
                    });
                    if let Some(job) = &self.job {
                        let running = job.running.load(Ordering::Relaxed);
                        ui.label(format!("Status: {}", if running {"Running"} else {"Stopped"}));
                    } else { ui.label("Status: Stopped"); }
                });

                /* ---------------- Right: Sequence Mode ---------------- */
                let ui = &mut cols[1];

                ui.group(|ui| {
                    ui.heading("Sequence Editor");

                    // list of steps
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        let mut action = None;
                        for (i, step) in self.seq_steps.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("{}. {}", i + 1, step.name));
                                ui.monospace(format!(
                                    "[{}] clicks:{} interval:{:.2}-{:.2}s bounds:[{},{}]-[{},{}]",
                                    if step.button == ClickButton::Left {"L"} else {"R"},
                                    step.clicks, step.min_secs, step.max_secs,
                                    step.bounds.min_x, step.bounds.min_y, step.bounds.max_x, step.bounds.max_y
                                ));
                            });
                            ui.horizontal(|ui| {
                                if ui.small_button("Edit").clicked() { action = Some(Action::Edit(i)); }
                                if ui.small_button("▲").clicked() { action = Some(Action::MoveUp(i)); }
                                if ui.small_button("▼").clicked() { action = Some(Action::MoveDown(i)); }
                                if ui.small_button("✕ Remove").clicked() { action = Some(Action::Remove(i)); }
                            });
                            ui.separator();
                        }

                        // Handle any actions after the loop to avoid borrow checker issues
                        if let Some(action) = action {
                            match action {
                                Action::Edit(i) => self.select_step_for_edit(i),
                                Action::MoveUp(i) => self.move_step_up(i),
                                Action::MoveDown(i) => self.move_step_down(i),
                                Action::Remove(i) => {
                                    self.seq_steps.remove(i);
                                    if let Some(sel) = self.editing_step_idx {
                                        if sel == i { self.editing_step_idx = None; }
                                        else if sel > i { self.editing_step_idx = Some(sel - 1); }
                                    }
                                }
                            }
                        }
                    });

                    ui.separator();
                    ui.collapsing(if self.editing_step_idx.is_some() {"Edit Step"} else {"Add Step"}, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Name:");
                            ui.text_edit_singleline(&mut self.edit_name);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Clicks:");
                            ui.add(egui::DragValue::new(&mut self.edit_clicks).speed(1).clamp_range(1..=1_000_000));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Interval (sec):");
                            ui.add(egui::DragValue::new(&mut self.edit_min_secs).speed(0.1));
                            ui.label("to");
                            ui.add(egui::DragValue::new(&mut self.edit_max_secs).speed(0.1));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Button:");
                            ui.checkbox(&mut self.edit_button_left, "Left");
                            let mut r = !self.edit_button_left;
                            if ui.checkbox(&mut r, "Right").clicked() { self.edit_button_left = !r; }
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.edit_bounds_from_current, "Use current Single Mode bounds for this step");
                            if !self.edit_bounds_from_current {
                                ui.label("(keeps step’s previous bounds if editing)");
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button(if self.editing_step_idx.is_some() {"Apply"} else {"Add"}).clicked() {
                                self.push_or_update_step_from_editor();
                            }
                            if self.editing_step_idx.is_some() && ui.button("Cancel").clicked() {
                                self.editing_step_idx = None;
                            }
                        });
                    });

                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Sequence cycles (empty = infinite):");
                        let mut cycles_display = self.seq_cycles.unwrap_or(0);
                        let changed = ui.add(egui::DragValue::new(&mut cycles_display).speed(1).clamp_range(0..=1_000_000)).changed();
                        if changed {
                            self.seq_cycles = if cycles_display == 0 { None } else { Some(cycles_display) };
                        }
                    });

                    ui.horizontal(|ui| {
                        if ui.button("Start Sequence").clicked() { self.start_sequence(); }
                        if ui.button("Pause").clicked() { self.pause(); }
                        if ui.button("Stop").clicked() { self.stop(); }
                    });
                });
            });
        });
    }
}

/* -------------------------- Main ------------------------------ */
fn main() -> eframe::Result<()> {
    let _args = Args::parse(); // CLI kept for compatibility; UI drives most behavior.

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
