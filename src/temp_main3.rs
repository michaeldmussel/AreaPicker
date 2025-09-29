mod human_mouse;

use eframe::{egui::{self, Color32, Pos2, Rect, Sense}, App};
use enigo::{self, MouseButton, MouseControllable};
use parking_lot::Mutex;
use rand::Rng;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use std::time::Duration;
use crate::human_mouse::{HumanMouseSettings, Bounds};

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
        match self {
            ClickButton::Left => MouseButton::Left,
            ClickButton::Right => MouseButton::Right,
        }
    }
}

#[derive(Clone, Debug)]
struct SequenceStep {
    pub name: String,
    pub bounds: Bounds,
    pub button: ClickButton,
    pub min_secs: f32,
    pub max_secs: f32,
    pub clicks: i32,
    pub enabled: bool,
}

struct ClickJob {
    running: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

struct AppState {
    job: Option<ClickJob>,
    
    // Sequence state
    seq_steps: Vec<SequenceStep>,
    editing_step_idx: Option<usize>,
    sequence_cycles: Option<i32>,
    infinite_cycles: bool,
    
    // Area picking state
    picking_area: bool,
    window_visible: bool,
    drag_start: Option<Pos2>,
    drag_end: Option<Pos2>,
    
    // Active step editing state
    edit_bounds: [i32; 4],
    edit_button_left: bool,
    edit_min_secs: f32,
    edit_max_secs: f32,
    edit_name: String,
    edit_clicks: i32,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            job: None,
            seq_steps: Vec::new(),
            editing_step_idx: None,
            sequence_cycles: Some(1),
            infinite_cycles: false,
            picking_area: false,
            window_visible: true,
            drag_start: None,
            drag_end: None,
            edit_bounds: [100, 400, 100, 400],
            edit_button_left: true,
            edit_min_secs: 1.0,
            edit_max_secs: 3.0,
            edit_name: "New Step".to_string(),
            edit_clicks: 1,
        }
    }
}

impl AppState {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Default::default()
    }

    fn start(&mut self) {
        if self.job.is_some() { return; }

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);
        let steps = self.seq_steps.clone();
        let cycles = if self.infinite_cycles { None } else { self.sequence_cycles };

        let handle = std::thread::spawn(move || {
            let mut enigo = enigo::Enigo::new();
            let mut rng = rand::thread_rng();
            let mut cycles_left = cycles;
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

            'outer: while running_clone.load(Ordering::Relaxed) {
                // Go through each enabled step in sequence
                for step in steps.iter().filter(|s| s.enabled) {
                    for _ in 0..step.clicks {
                        if !running_clone.load(Ordering::Relaxed) { break 'outer; }
                        
                        let x = rng.gen_range(step.bounds.min_x..=step.bounds.max_x);
                        let y = rng.gen_range(step.bounds.min_y..=step.bounds.max_y);
                        
                        let (start_x, start_y) = enigo.mouse_location();
                        human_mouse::move_mouse_human(&mut enigo, start_x, start_y, x, y);
                        enigo.mouse_click(step.button.to_enigo());

                        let delay = rng.gen_range(step.min_secs..=step.max_secs);
                        std::thread::sleep(Duration::from_secs_f32(delay));
                    }
                }

                if let Some(cycles) = &mut cycles_left {
                    *cycles -= 1;
                    if *cycles <= 0 { break; }
                }
            }
        });

        self.job = Some(ClickJob {
            running,
            handle: Some(handle),
        });
    }

    fn stop(&mut self) {
        if let Some(mut job) = self.job.take() {
            job.running.store(false, Ordering::Relaxed);
            if let Some(handle) = job.handle.take() {
                handle.join().ok();
            }
        }
    }

    fn pause(&mut self) {
        if let Some(ref job) = self.job {
            job.running.store(false, Ordering::Relaxed);
        }
    }

    fn select_step_for_edit(&mut self, idx: usize) {
        if idx < self.seq_steps.len() {
            let step = &self.seq_steps[idx];
            self.edit_bounds = [
                step.bounds.min_x,
                step.bounds.max_x,
                step.bounds.min_y,
                step.bounds.max_y,
            ];
            self.edit_button_left = matches!(step.button, ClickButton::Left);
            self.edit_min_secs = step.min_secs;
            self.edit_max_secs = step.max_secs;
            self.edit_name = step.name.clone();
            self.edit_clicks = step.clicks;
            self.editing_step_idx = Some(idx);
        }
    }

    fn move_step_up(&mut self, idx: usize) {
        if idx > 0 {
            self.seq_steps.swap(idx, idx - 1);
            if let Some(sel) = self.editing_step_idx {
                if sel == idx { self.editing_step_idx = Some(idx - 1); }
                else if sel == idx - 1 { self.editing_step_idx = Some(idx); }
            }
        }
    }

    fn move_step_down(&mut self, idx: usize) {
        if idx + 1 < self.seq_steps.len() {
            self.seq_steps.swap(idx, idx + 1);
            if let Some(sel) = self.editing_step_idx {
                if sel == idx { self.editing_step_idx = Some(idx + 1); }
                else if sel == idx + 1 { self.editing_step_idx = Some(idx); }
            }
        }
    }

    fn bounds_from_drag(&mut self) {
        if let (Some(start), Some(end)) = (self.drag_start, self.drag_end) {
            self.edit_bounds = [
                start.x.min(end.x) as i32,
                start.x.max(end.x) as i32,
                start.y.min(end.y) as i32,
                start.y.max(end.y) as i32,
            ];
        }
    }

    fn get_total_screen_bounds() -> (i32, i32, i32, i32) {
        // TODO: Add multi-monitor support
        (0, 0, 1920, 1080)
    }
}

impl App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.picking_area {
            if self.window_visible {
                // Get screen dimensions
                let (min_x, min_y, max_x, max_y) = Self::get_total_screen_bounds();
                let width = max_x - min_x;
                let height = max_y - min_y;
                
                // Make window fullscreen and center it
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(width as f32, height as f32)));
                if let Some(cmd) = egui::ViewportCommand::center_on_screen(ctx) {
                    ctx.send_viewport_cmd(cmd);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
                self.window_visible = false;
                return;
            }
            
            let layer_id = egui::LayerId::new(egui::Order::Background, egui::Id::new("picker_overlay"));
            let painter = egui::Painter::new(ctx.clone(), layer_id, egui::Rect::EVERYTHING);
            
            let (min_x, min_y, max_x, max_y) = Self::get_total_screen_bounds();
            let screen_rect = egui::Rect::from_min_max(
                Pos2::new(min_x as f32, min_y as f32),
                Pos2::new(max_x as f32, max_y as f32)
            );
            
            painter.rect_filled(screen_rect, 0.0, Color32::from_black_alpha(128));

            egui::Area::new(egui::Id::new("picker_area"))
                .order(egui::Order::Foreground)
                .movable(false)
                .interactable(true)
                .show(ctx, |ui| {
                    let resp = ui.allocate_rect(screen_rect, Sense::click_and_drag());
                    
                    let absolute_pos = resp.hover_pos().unwrap_or_default();
                    
                    if resp.drag_started() {
                        self.drag_start = Some(absolute_pos);
                        self.drag_end = self.drag_start;
                    }
                    if resp.dragged() {
                        self.drag_end = Some(absolute_pos);
                    }
                    if resp.drag_released() {
                        self.drag_end = Some(absolute_pos);
                        self.bounds_from_drag();
                        self.picking_area = false;
                        self.window_visible = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(800.0, 600.0)));
                    }

                    if let (Some(a), Some(b)) = (self.drag_start, self.drag_end) {
                        let rect = Rect::from_two_pos(a, b);
                        let stroke = egui::Stroke::new(2.0, Color32::LIGHT_BLUE);
                        painter.rect_stroke(rect, 0.0, stroke);
                    }
                });

            ctx.request_repaint();
            return;
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Area Clicker");
                ui.separator();
                if let Some(job) = &self.job {
                    let running = job.running.load(Ordering::Relaxed);
                    ui.label(format!("Status: {}", if running {"Running"} else {"Stopped"}));
                } else {
                    ui.label("Status: Stopped");
                }
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Stop").clicked() { self.stop(); }
                    if ui.button("Pause").clicked() { self.pause(); }
                    if ui.button("Start").clicked() { self.start(); }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns(2, |cols| {
                /* ---------------- Left: Steps List ---------------- */
                cols[0].group(|ui| {
                    ui.heading("Step Manager");
                    ui.separator();

                    // Cycles control
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.infinite_cycles, "Infinite cycles");
                        if !self.infinite_cycles {
                            ui.add(egui::DragValue::new(&mut self.sequence_cycles.get_or_insert(1))
                                .speed(1)
                                .clamp_range(1..=1000)
                                .prefix("Cycles: "));
                        }
                    });

                    ui.separator();

                    // List of steps
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        let mut action = None;
                        for (i, step) in self.seq_steps.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.seq_steps[i].enabled, "");
                                ui.label(format!("{}. {}", i + 1, step.name));
                            });
                            ui.horizontal(|ui| {
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

                        // Handle actions after the loop to avoid borrow checker issues
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
                });

                /* ---------------- Right: Step Editor ---------------- */
                cols[1].group(|ui| {
                    ui.heading(if self.editing_step_idx.is_some() {"Edit Step"} else {"Add Step"});
                    ui.separator();

                    // Step name
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(&mut self.edit_name);
                    });

                    // Area bounds
                    ui.label("Area Bounds:");
                    egui::Grid::new("bounds_grid")
                        .num_columns(4)
                        .spacing([4.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("X:");
                            ui.add(egui::DragValue::new(&mut self.edit_bounds[0]).prefix("min: "));
                            ui.add(egui::DragValue::new(&mut self.edit_bounds[1]).prefix("max: "));
                            ui.label(format!("(width: {})", self.edit_bounds[1] - self.edit_bounds[0]));
                            ui.end_row();
                            
                            ui.label("Y:");
                            ui.add(egui::DragValue::new(&mut self.edit_bounds[2]).prefix("min: "));
                            ui.add(egui::DragValue::new(&mut self.edit_bounds[3]).prefix("max: "));
                            ui.label(format!("(height: {})", self.edit_bounds[3] - self.edit_bounds[2]));
                            ui.end_row();
                        });

                    if ui.button("Pick Area (drag a rectangle)").clicked() {
                        self.drag_start = None;
                        self.drag_end = None;
                        self.picking_area = true;
                        self.window_visible = true;
                    }

                    // Click settings
                    ui.separator();
                    ui.label("Click Settings:");
                    egui::Grid::new("click_settings")
                        .num_columns(4)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("Button:");
                            ui.radio_value(&mut self.edit_button_left, true, "Left");
                            ui.radio_value(&mut self.edit_button_left, false, "Right");
                            ui.end_row();

                            ui.label("Interval:");
                            ui.add(egui::DragValue::new(&mut self.edit_min_secs).speed(0.1).suffix(" sec"));
                            ui.label("to");
                            ui.add(egui::DragValue::new(&mut self.edit_max_secs).speed(0.1).suffix(" sec"));
                            ui.end_row();

                            ui.label("Clicks:");
                            ui.add(egui::DragValue::new(&mut self.edit_clicks).speed(1.0).clamp_range(1..=1000));
                            ui.end_row();
                        });

                    // Save/Add button
                    ui.separator();
                    if ui.button(if self.editing_step_idx.is_some() {"Save Changes"} else {"Add Step"}).clicked() {
                        let step = SequenceStep {
                            name: self.edit_name.clone(),
                            bounds: Bounds {
                                min_x: self.edit_bounds[0],
                                max_x: self.edit_bounds[1],
                                min_y: self.edit_bounds[2],
                                max_y: self.edit_bounds[3],
                            },
                            button: if self.edit_button_left {ClickButton::Left} else {ClickButton::Right},
                            min_secs: self.edit_min_secs,
                            max_secs: self.edit_max_secs,
                            clicks: self.edit_clicks,
                            enabled: true,
                        };

                        if let Some(idx) = self.editing_step_idx {
                            self.seq_steps[idx] = step;
                            self.editing_step_idx = None;
                        } else {
                            self.seq_steps.push(step);
                        }

                        // Reset name for next step
                        self.edit_name = format!("Step {}", self.seq_steps.len() + 1);
                    }
                });
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([800.0, 600.0])
            .with_resizable(false),
        ..Default::default()
    };
    eframe::run_native(
        "Area Clicker",
        options,
        Box::new(|cc| Box::new(AppState::new(cc)))
    )
}