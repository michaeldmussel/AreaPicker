use eframe::{egui, egui::{Color32, Pos2, Rect, Sense}};
use enigo::{self, MouseButton, MouseControllable};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rand::Rng;
use std::{sync::{atomic::{AtomicBool, Ordering}, Arc}, thread, time::Duration};

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
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);
        let config_clone = Arc::clone(&config);
        
        eprintln!("Starting click job with config: {:?}", config.lock());

        thread::spawn(move || {
            let mut rng = rand::thread_rng();
            loop {
                if !running_clone.load(Ordering::Relaxed) { break; }

                let cfg = config_clone.lock().clone();
                let Some(b) = cfg.bounds else {
                    // No bounds set; idle briefly
                    thread::sleep(Duration::from_millis(200));
                    continue;
                };
                if !b.is_valid() { thread::sleep(Duration::from_millis(200)); continue; }

                // Pick random point
                let x = rng.gen_range(b.min_x..=b.max_x);
                let y = rng.gen_range(b.min_y..=b.max_y);

                // Move & click
                {
                    let mut en = ENIGO.lock();
                    en.mouse_move_to(x, y);
                    match cfg.button {
                        ClickButton::Left => {
                            en.mouse_click(MouseButton::Left);
                        }
                        ClickButton::Right => {
                            en.mouse_click(MouseButton::Right);
                        }
                    }
                }

                // Sleep random between min..max (seconds)
                let (min_s, max_s) = if cfg.min_secs <= cfg.max_secs {
                    (cfg.min_secs, cfg.max_secs)
                } else { (cfg.max_secs, cfg.min_secs) };
                let wait = rng.gen_range(min_s..=max_s).max(0.01);
                let ms = (wait * 1000.0) as u64;
                for _ in 0..ms/50 {
                    if !running_clone.load(Ordering::Relaxed) { break; }
                    thread::sleep(Duration::from_millis(50));
                }
                if ms % 50 != 0 { thread::sleep(Duration::from_millis(ms % 50)); }
            }
        });

        Self { running, config }
    }

    fn stop(&self) { self.running.store(false, Ordering::Relaxed); }
}

// -------------- UI State --------------
struct AppState {
    picking_area: bool,
    drag_start: Option<Pos2>,
    drag_end: Option<Pos2>,
    window_visible: bool,

    bounds_inputs: [i32; 4], // min_x, max_x, min_y, max_y
    click_button_left: bool,
    min_secs: f32,
    max_secs: f32,

    job: Option<ClickJob>,
    config: Arc<Mutex<ClickConfig>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            picking_area: false,
            drag_start: None,
            drag_end: None,
            window_visible: true,

            bounds_inputs: [100, 400, 100, 400],
            click_button_left: true,
            min_secs: 1.0,
            max_secs: 3.0,

            job: None,
            config: Arc::new(Mutex::new(ClickConfig{
                bounds: Some(Bounds{min_x:100, max_x:400, min_y:100, max_y:400}),
                button: ClickButton::Left,
                min_secs: 1.0,
                max_secs: 3.0,
            })),
        }
    }
}

impl AppState {
    fn start(&mut self) {
        if self.job.is_some() { return; }
        // sync current config
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

    fn get_total_screen_bounds() -> (i32, i32, i32, i32) {
        // Use enigo to get the main display size as a fallback
        let en = enigo::Enigo::new();
        let (w, h) = en.main_display_size();
        
        // Try to get multi-monitor info using winit, but fallback to main display if it fails
        match winit::event_loop::EventLoop::new() {
            Ok(event_loop) => {
                let monitors: Vec<_> = event_loop.available_monitors().collect();
                if monitors.is_empty() {
                    return (0, 0, w as i32, h as i32);
                }
                
                let mut min_x = i32::MAX;
                let mut min_y = i32::MAX;
                let mut max_x = i32::MIN;
                let mut max_y = i32::MIN;
                
                for monitor in monitors {
            let position = monitor.position();
            let size = monitor.size();
            
            min_x = min_x.min(position.x);
            min_y = min_y.min(position.y);
            max_x = max_x.max(position.x + size.width as i32);
            max_y = max_y.max(position.y + size.height as i32);
        }
                
                if min_x == i32::MAX {
                    (0, 0, w as i32, h as i32)
                } else {
                    (min_x, min_y, max_x, max_y)
                }
            }
            Err(_) => {
                // Fallback to main display if winit fails
                (0, 0, w as i32, h as i32)
            }
        }
    }

    fn bounds_from_drag(&mut self) {
        if let (Some(a), Some(b)) = (self.drag_start, self.drag_end) {
            let min_x = a.x.min(b.x).round() as i32;
            let max_x = a.x.max(b.x).round() as i32;
            let min_y = a.y.min(b.y).round() as i32;
            let max_y = a.y.max(b.y).round() as i32;
            self.bounds_inputs = [min_x, max_x, min_y, max_y];
            self.config.lock().bounds = Some(Bounds{min_x, max_x, min_y, max_y});
        }
    }
}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.picking_area {
            // When in picking mode, make window transparent first
            if self.window_visible {
                self.window_visible = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(0.0, 0.0)));
                return;
            }
            
            let layer_id = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("picker"));
            let painter = egui::Painter::new(
                ctx.clone(),
                layer_id,
                egui::Rect::EVERYTHING,
            );
            
            // Get the screen dimensions
            let (min_x, min_y, max_x, max_y) = Self::get_total_screen_bounds();
            let width = max_x - min_x;
            let height = max_y - min_y;
            let screen_rect = egui::Rect::from_min_size(
                Pos2::new(min_x as f32, min_y as f32),
                egui::vec2(width as f32, height as f32)
            );
            
            painter.rect_filled(screen_rect, 0.0, Color32::from_rgba_premultiplied(128, 128, 128, 100));

            egui::Area::new(egui::Id::new("picker_area")).order(egui::Order::Foreground).show(ctx, |ui| {
                let resp = ui.allocate_rect(screen_rect, Sense::click_and_drag());
                if resp.drag_started() {
                    self.drag_start = Some(resp.interact_pointer_pos().unwrap_or(Pos2::new(0.0,0.0)));
                    self.drag_end = self.drag_start;
                }
                if resp.dragged() {
                    self.drag_end = resp.interact_pointer_pos();
                }
                if resp.drag_stopped() {
                    self.drag_end = resp.interact_pointer_pos();
                    self.bounds_from_drag();
                    self.picking_area = false;
                    // After area is picked, just make window visible again
                    self.window_visible = true;
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

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.heading("Area Clicker — MVP");
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.group(|ui| {
                    ui.label("Selection (px, screen coords)");
                    ui.horizontal(|ui| { ui.label("min X"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[0])); });
                    ui.horizontal(|ui| { ui.label("max X"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[1])); });
                    ui.horizontal(|ui| { ui.label("min Y"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[2])); });
                    ui.horizontal(|ui| { ui.label("max Y"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[3])); });
                    if ui.button("Pick Area (drag a rectangle)").clicked() { 
                        self.drag_start = None;
                        self.drag_end = None;
                        self.picking_area = true;
                        self.window_visible = true; // Will be set to false on next frame
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

            // Preview rectangle
            if let Some(b) = self.config.lock().bounds {
                let info = format!("Active bounds: x=[{}..{}], y=[{}..{}] ({}x{})", b.min_x, b.max_x, b.min_y, b.max_y, b.width(), b.height());
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
        let valid_bounds = Bounds {
            min_x: 100,
            max_x: 200,
            min_y: 100,
            max_y: 200,
        };
        assert!(valid_bounds.is_valid());
        assert_eq!(valid_bounds.width(), 100);
        assert_eq!(valid_bounds.height(), 100);

        let invalid_bounds = Bounds {
            min_x: 200,
            max_x: 100,
            min_y: 200,
            max_y: 100,
        };
        assert!(!invalid_bounds.is_valid());
    }

    #[test]
    fn test_click_job_creation() {
        let config = Arc::new(Mutex::new(ClickConfig {
            bounds: Some(Bounds {
                min_x: 100,
                max_x: 200,
                min_y: 100,
                max_y: 200,
            }),
            button: ClickButton::Left,
            min_secs: 1.0,
            max_secs: 2.0,
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
        
        // Check default bounds
        assert_eq!(state.bounds_inputs, [100, 400, 100, 400]);
        
        // Check default timing
        assert_eq!(state.min_secs, 1.0);
        assert_eq!(state.max_secs, 3.0);
    }

    #[test]
    fn test_bounds_from_drag() {
        let mut state = AppState::default();
        
        // Test drag selection
        state.drag_start = Some(Pos2::new(100.0, 100.0));
        state.drag_end = Some(Pos2::new(200.0, 200.0));
        state.bounds_from_drag();

        assert_eq!(state.bounds_inputs, [100, 200, 100, 200]);
        
        // Test reverse drag (from bottom-right to top-left)
        state.drag_start = Some(Pos2::new(200.0, 200.0));
        state.drag_end = Some(Pos2::new(100.0, 100.0));
        state.bounds_from_drag();

        assert_eq!(state.bounds_inputs, [100, 200, 100, 200]);
    }

    #[test]
    fn test_click_interval() {
        let config = Arc::new(Mutex::new(ClickConfig {
            bounds: Some(Bounds {
                min_x: 100,
                max_x: 200,
                min_y: 100,
                max_y: 200,
            }),
            button: ClickButton::Left,
            min_secs: 0.1,
            max_secs: 0.2,
        }));

        let job = ClickJob::spawn(Arc::clone(&config));
        
        // Let it run briefly to ensure it's working
        std::thread::sleep(Duration::from_millis(500));
        
        job.stop();
        assert!(!job.running.load(Ordering::Relaxed));
    }
}

fn main() -> eframe::Result<()> {
    let mut opts = eframe::NativeOptions::default();
    opts.viewport.inner_size = Some(egui::vec2(520.0, 380.0));
    opts.viewport.transparent = Some(true);
    opts.viewport.maximized = Some(false); // Don't maximize by default
    opts.viewport.resizable = Some(true);
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
