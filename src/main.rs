mod human_mouse;

use eframe::{egui, egui::{Color32, Pos2, Rect, Sense}};
use enigo::{self, MouseButton, MouseControllable};
use parking_lot::Mutex;
use rand::Rng;
use std::{sync::{atomic::{AtomicBool, Ordering}, Arc}, thread, time::Duration};


fn to_px(ctx: &egui::Context, p: egui::Pos2) -> (i32, i32) {
    let ppp = ctx.pixels_per_point();
    ((p.x * ppp).round() as i32, (p.y * ppp).round() as i32)
}
fn rect_pts(ctx: &egui::Context) -> egui::Rect {
    // screen rect in points (logical)
    ctx.input(|i| i.screen_rect)
}
fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([800.0, 600.0])
            .with_resizable(false)
            .with_transparent(true),
        follow_system_theme: false,
        default_theme: eframe::Theme::Dark,
        ..Default::default()
    };
    eframe::run_native(
        "Area Clicker",
        options,
        Box::new(|cc| Box::new(AppState::new(cc)))
    )
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
    handle: Option<std::thread::JoinHandle<()>>,
    config: Arc<Mutex<ClickConfig>>,
}

#[derive(Clone, Debug)]
struct SequenceAction {
    bounds: Bounds,
    button: ClickButton,
    min_secs: f32,
    max_secs: f32,
    clicks_per_cycle: i32,
    name: String,
    enabled: bool,
}

#[derive(Clone, Debug)]
struct ClickConfig {
    bounds: Option<Bounds>,
    button: ClickButton,
    min_secs: f32,
    max_secs: f32,
    sequence_mode: bool,
    sequence: Vec<SequenceAction>,
}

// -------------- App State --------------
struct AppState {
    config: Arc<Mutex<ClickConfig>>,
    job: Option<ClickJob>,
    
    // UI State
    bounds_inputs: [i32; 4],
    click_button_left: bool,
    min_secs: f32,
    max_secs: f32,
    finite_clicks: Option<i32>,
    sequence_cycles: Option<i32>,
    
    // Area picking state
    picking_area: bool,
    window_visible: bool,
    drag_start: Option<Pos2>,
    drag_end: Option<Pos2>,

    // Sequence editing state
    editing_sequence: bool,
    current_sequence_index: usize,
    sequence_edit_name: String,
    sequence_edit_clicks: i32,
}

impl AppState {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            config: Arc::new(Mutex::new(ClickConfig {
                bounds: None,
                button: ClickButton::Left,
                min_secs: 1.0,
                max_secs: 3.0,
                sequence_mode: false,
                sequence: Vec::new(),
            })),
            job: None,
            bounds_inputs: [100, 400, 100, 400],
            click_button_left: true,
            min_secs: 1.0,
            max_secs: 3.0,
            finite_clicks: None,
            sequence_cycles: None,
            picking_area: false,
            window_visible: true,
            drag_start: None,
            drag_end: None,
            editing_sequence: false,
            current_sequence_index: 0,
            sequence_edit_name: String::new(),
            sequence_edit_clicks: 1,
        }
    }

    fn start(&mut self) {
        if self.job.is_some() {
            return;
        }

        let config = self.config.clone();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        
        let finite_clicks = self.finite_clicks;
        let sequence_cycles = self.sequence_cycles;

        let handle = thread::spawn(move || {
            let mut enigo = enigo::Enigo::new();
            let mut rng = rand::thread_rng();
            let mut clicks_left = finite_clicks;
            let mut cycles_left = sequence_cycles;
            let mut last_pos: Option<(i32,i32)> = None;
            
            'outer: while running_clone.load(Ordering::Relaxed) {
                let cfg = config.lock();
                
                if cfg.sequence_mode {
                    // Sequence mode - go through each enabled action
                    if cfg.sequence.is_empty() {
                        thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    
                    for action in cfg.sequence.iter().filter(|a| a.enabled) {
                        if !running_clone.load(Ordering::Relaxed) {
                            break 'outer;
                        }
                        
                        for _ in 0..action.clicks_per_cycle {
                            let x = rng.gen_range(action.bounds.min_x..=action.bounds.max_x);
                            let y = rng.gen_range(action.bounds.min_y..=action.bounds.max_y);
                            
                            if let Some((lx, ly)) = last_pos {
                            human_mouse::move_mouse_smooth(&mut enigo, lx, ly, x, y, Duration::from_millis(140), false);
                        } else {
                            enigo.mouse_move_to(x, y);
                        }
                        last_pos = Some((x,y));
                            
                            let button = match action.button {
                                ClickButton::Left => MouseButton::Left,
                                ClickButton::Right => MouseButton::Right,
                            };
                            enigo.mouse_click(button);
                            
                            let delay = rng.gen_range(action.min_secs..=action.max_secs);
                            thread::sleep(Duration::from_secs_f32(delay));
                        }
                    }
                    
                    if let Some(cycles) = &mut cycles_left {
                        *cycles -= 1;
                        if *cycles <= 0 {
                            break;
                        }
                    }
                } else {
                    // Single area mode
                    if let Some(bounds) = cfg.bounds {
                        if !bounds.is_valid() {
                            thread::sleep(Duration::from_millis(100));
                            continue;
                        }

                        let x = rng.gen_range(bounds.min_x..=bounds.max_x);
                        let y = rng.gen_range(bounds.min_y..=bounds.max_y);
                        
                        if let Some((lx, ly)) = last_pos {
                            human_mouse::move_mouse_smooth(&mut enigo, lx, ly, x, y, Duration::from_millis(140), false);
                        } else {
                            enigo.mouse_move_to(x, y);
                        }
                        last_pos = Some((x,y));
                        
                        let button = match cfg.button {
                            ClickButton::Left => MouseButton::Left,
                            ClickButton::Right => MouseButton::Right,
                        };
                        enigo.mouse_click(button);
                        
                        if let Some(clicks) = &mut clicks_left {
                            *clicks -= 1;
                            if *clicks <= 0 {
                                break;
                            }
                        }

                        let delay = rng.gen_range(cfg.min_secs..=cfg.max_secs);
                        thread::sleep(Duration::from_secs_f32(delay));
                    }
                }
            }
        });

        self.job = Some(ClickJob {
            running,
            handle: Some(handle),
            config: self.config.clone(),
        });
    }

    fn stop(&mut self) {
        if let Some(job) = self.job.take() {
            job.running.store(false, Ordering::Relaxed);
            if let Some(handle) = job.handle {
                handle.join().ok();
            }
        }
    }

    fn pause(&mut self) {
        if let Some(ref job) = self.job {
            job.running.store(false, Ordering::Relaxed);
        }
    }

    fn bounds_from_drag(&mut self, ctx: &egui::Context) {
        if let (Some(start), Some(end)) = (self.drag_start, self.drag_end) {
            let (sx, sy) = to_px(ctx, start);
            let (ex, ey) = to_px(ctx, end);
            let min_x = sx.min(ex);
            let max_x = sx.max(ex);
            let min_y = sy.min(ey);
            let max_y = sy.max(ey);
            self.bounds_inputs = [min_x, max_x, min_y, max_y];
            
            // In single area mode, update the config bounds directly
            if !self.config.lock().sequence_mode {
                self.config.lock().bounds = Some(Bounds {
                    min_x: self.bounds_inputs[0],
                    max_x: self.bounds_inputs[1],
                    min_y: self.bounds_inputs[2],
                    max_y: self.bounds_inputs[3],
                });
            }
        }
    }

    fn get_total_screen_bounds(ctx: &egui::Context) -> (i32, i32, i32, i32) {
        let sr = ctx.input(|i| i.screen_rect);
        let ppp = ctx.pixels_per_point();
        let min_x = (sr.min.x * ppp).round() as i32;
        let min_y = (sr.min.y * ppp).round() as i32;
        let max_x = (sr.max.x * ppp).round() as i32;
        let max_y = (sr.max.y * ppp).round() as i32;
        (min_x, min_y, max_x, max_y)
    }
}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.picking_area {
            if self.window_visible {
                // Get screen dimensions
                let sr = rect_pts(ctx);
                // Make window fullscreen and center it
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(sr.size()));
                if let Some(cmd) = egui::ViewportCommand::center_on_screen(ctx) {
                    ctx.send_viewport_cmd(cmd);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
                self.window_visible = false;
                return;
            }
            
            // Make the window background transparent
            ctx.set_visuals(egui::Visuals {
                window_fill: Color32::from_rgba_premultiplied(0, 0, 0, 0),
                panel_fill: Color32::from_rgba_premultiplied(0, 0, 0, 0),
                window_stroke: egui::Stroke::NONE,
                ..Default::default()
            });
            
            // Clear any background
            ctx.clear_animations();

            // Get the screen dimensions and create rect in absolute coordinates
            let (min_x, min_y, max_x, max_y) = Self::get_total_screen_bounds(ctx);
            let screen_rect = egui::Rect::from_min_max(
                Pos2::new(min_x as f32, min_y as f32),
                Pos2::new(max_x as f32, max_y as f32)
            );

            egui::Window::new("picker_overlay")
                .frame(egui::Frame::none())
                .title_bar(false)
                .resizable(false)
                .fixed_pos((0.0, 0.0))
                .fixed_size((screen_rect.width(), screen_rect.height()))
                .show(ctx, |ui| {
                    // Very light tint for the entire screen
                    ui.painter().rect_filled(
                        screen_rect,
                        0.0,
                        Color32::from_rgba_premultiplied(128, 128, 128, 20)
                    );

                    let response = ui.allocate_rect(screen_rect, Sense::click_and_drag());
                    let absolute_pos = if let Some(pos) = response.hover_pos() {
                        pos
                    } else {
                        Pos2::new(0.0, 0.0)
                    };
                    
                    if response.drag_started() {
                        self.drag_start = Some(absolute_pos);
                        self.drag_end = self.drag_start;
                    }
                    if response.dragged() {
                        self.drag_end = Some(absolute_pos);
                    }
                    if response.drag_stopped() {
                        self.drag_end = Some(absolute_pos);
                        self.bounds_from_drag(ctx);
                        self.picking_area = false;
                        // Restore window to normal state
                        self.window_visible = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
                        // Reset window size to default
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(800.0, 600.0)));
                    }

                    // Draw selection rectangle with a contrasting outline
                    if let (Some(a), Some(b)) = (self.drag_start, self.drag_end) {
                        let rect = Rect::from_two_pos(a, b);
                        
                        // Fill with very light blue
                        ui.painter().rect_filled(
                            rect,
                            0.0,
                            Color32::from_rgba_premultiplied(100, 150, 255, 40)
                        );
                        
                        // Draw a white border
                        ui.painter().rect_stroke(
                            rect,
                            0.0,
                            egui::Stroke { width: 1.0, color: Color32::WHITE }
                        );
                        
                        // Draw a black outer border for contrast
                        ui.painter().rect_stroke(
                            rect.expand(1.0),
                            0.0,
                            egui::Stroke { width: 1.0, color: Color32::BLACK }
                        );
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
                    ui.label(format!("Status: {}", if running { "Running" } else { "Stopped" }));
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
            let sequence_mode = self.config.lock().sequence_mode;
            
            ui.columns(2, |cols| {
                // Left column - Sequence management
                cols[0].group(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            let mut sequence_mode = sequence_mode;
                            if ui.checkbox(&mut sequence_mode, "Enable Sequence Mode").changed() {
                                self.config.lock().sequence_mode = sequence_mode;
                            }
                            if sequence_mode {
                                if ui.button("➕ Add Step").clicked() {
                                    self.current_sequence_index = self.config.lock().sequence.len();
                                    self.editing_sequence = true;
                                    self.bounds_inputs = [100, 400, 100, 400];
                                    self.click_button_left = true;
                                    self.min_secs = 1.0;
                                    self.max_secs = 3.0;
                                    self.sequence_edit_name = format!("Step {}", self.current_sequence_index + 1);
                                    self.sequence_edit_clicks = 1;
                                }
                            }
                        });

                        if sequence_mode {
                            ui.separator();
                            ui.heading("Sequence Steps");

                            // List existing steps
                            let mut to_remove = None;
                            {
                                let config = self.config.lock();
                                for (i, action) in config.sequence.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        if ui.button(format!("Edit #{}", i + 1)).clicked() {
                                            self.current_sequence_index = i;
                                            self.editing_sequence = true;
                                            self.bounds_inputs = [
                                                action.bounds.min_x,
                                                action.bounds.max_x,
                                                action.bounds.min_y,
                                                action.bounds.max_y
                                            ];
                                            self.click_button_left = matches!(action.button, ClickButton::Left);
                                            self.min_secs = action.min_secs;
                                            self.max_secs = action.max_secs;
                                            self.sequence_edit_name = action.name.clone();
                                            self.sequence_edit_clicks = action.clicks_per_cycle;
                                        }

                                        let mut enabled = action.enabled;
                                        if ui.checkbox(&mut enabled, "").changed() {
                                            self.config.lock().sequence[i].enabled = enabled;
                                        }

                                        ui.label(format!(
                                            "{}: [{},{}]x[{},{}], {} clicks, {:.1}-{:.1}s",
                                            action.name,
                                            action.bounds.min_x, action.bounds.max_x,
                                            action.bounds.min_y, action.bounds.max_y,
                                            action.clicks_per_cycle,
                                            action.min_secs, action.max_secs
                                        ));

                                        if ui.button("🗑").clicked() {
                                            to_remove = Some(i);
                                        }
                                    });
                                }
                            }

                            // Handle removal outside the iteration
                            if let Some(i) = to_remove {
                                self.config.lock().sequence.remove(i);
                            }
                        }
                    });
                });

                // Right column - Area configuration
                cols[1].group(|ui| {
                    if sequence_mode {
                        ui.label(if self.editing_sequence {
                            format!("Editing: {}", self.sequence_edit_name)
                        } else {
                            "Select or add a sequence step to edit".to_string()
                        });
                    } else {
                        ui.label("Single Area Configuration");
                    }

                    ui.label("Area Bounds:");
                    egui::Grid::new("bounds_grid").num_columns(4).spacing([4.0, 4.0]).show(ui, |ui| {
                        ui.label("X:");
                        ui.add(egui::DragValue::new(&mut self.bounds_inputs[0]).prefix("min: "));
                        ui.add(egui::DragValue::new(&mut self.bounds_inputs[1]).prefix("max: "));
                        ui.label(format!("(width: {})", self.bounds_inputs[1] - self.bounds_inputs[0]));
                        ui.end_row();
                        
                        ui.label("Y:");
                        ui.add(egui::DragValue::new(&mut self.bounds_inputs[2]).prefix("min: "));
                        ui.add(egui::DragValue::new(&mut self.bounds_inputs[3]).prefix("max: "));
                        ui.label(format!("(height: {})", self.bounds_inputs[3] - self.bounds_inputs[2]));
                        ui.end_row();
                    });

                    if ui.button("Pick Area (drag a rectangle)").clicked() {
                        self.drag_start = None;
                        self.drag_end = None;
                        self.picking_area = true;
                        self.window_visible = true;
                    }

                    ui.separator();
                    ui.label("Click Settings:");
                    egui::Grid::new("click_settings").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
                        ui.label("Button:");
                        ui.radio_value(&mut self.click_button_left, true, "Left");
                        ui.radio_value(&mut self.click_button_left, false, "Right");
                        ui.end_row();

                        ui.label("Interval:");
                        ui.add(egui::DragValue::new(&mut self.min_secs).speed(0.1).suffix(" sec"));
                        ui.label("to");
                        ui.add(egui::DragValue::new(&mut self.max_secs).speed(0.1).suffix(" sec"));
                        ui.end_row();
                    });

                    if sequence_mode && self.editing_sequence {
                        ui.horizontal(|ui| {
                            ui.label("Step name:");
                            ui.text_edit_singleline(&mut self.sequence_edit_name);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Clicks per cycle:");
                            ui.add(egui::DragValue::new(&mut self.sequence_edit_clicks).speed(1.0).clamp_range(1..=1000));
                        });
                        
                        if ui.button("Save Step").clicked() {
                            let action = SequenceAction {
                                bounds: Bounds {
                                    min_x: self.bounds_inputs[0],
                                    max_x: self.bounds_inputs[1],
                                    min_y: self.bounds_inputs[2],
                                    max_y: self.bounds_inputs[3],
                                },
                                button: if self.click_button_left { ClickButton::Left } else { ClickButton::Right },
                                min_secs: self.min_secs,
                                max_secs: self.max_secs,
                                clicks_per_cycle: self.sequence_edit_clicks,
                                name: self.sequence_edit_name.clone(),
                                enabled: true,
                            };

                            let mut config = self.config.lock();
                            if self.current_sequence_index < config.sequence.len() {
                                config.sequence[self.current_sequence_index] = action;
                            } else {
                                config.sequence.push(action);
                            }
                            self.editing_sequence = false;
                        }
                    } else if !sequence_mode {
                        ui.horizontal(|ui| {
                            ui.label("Click limit:");
                            let mut has_limit = self.finite_clicks.is_some();
                            if ui.checkbox(&mut has_limit, "Limited clicks").clicked() {
                                self.finite_clicks = if has_limit { Some(100) } else { None };
                            }
                            if let Some(ref mut clicks) = self.finite_clicks {
                                ui.add(egui::DragValue::new(clicks).speed(1.0));
                            }
                        });
                    } else if sequence_mode {
                        ui.horizontal(|ui| {
                            ui.label("Sequence cycles:");
                            let mut has_cycles = self.sequence_cycles.is_some();
                            if ui.checkbox(&mut has_cycles, "Limited cycles").clicked() {
                                self.sequence_cycles = if has_cycles { Some(1) } else { None };
                            }
                            if let Some(ref mut cycles) = self.sequence_cycles {
                                ui.add(egui::DragValue::new(cycles).speed(1.0));
                            }
                        });
                    }

                    // Preview rectangle
                    ui.separator();
                    if let Some(b) = self.config.lock().bounds {
                        ui.label("Current Selection:");
                        ui.monospace(format!(
                            "x=[{:>4}..{:<4}] ({:>4}px)\ny=[{:>4}..{:<4}] ({:>4}px)", 
                            b.min_x, b.max_x, b.width(),
                            b.min_y, b.max_y, b.height()
                        ));
                    } else {
                        ui.weak("No area selected");
                    }
                });
            });
        });
    }
}
