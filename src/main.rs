mod human_mouse;
mod presets;

use eframe::{egui, egui::{Color32, Pos2, Rect, Sense, WindowLevel}};
use enigo::MouseControllable;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use crate::human_mouse::{HumanMouseSettings, human_move_and_click};
use crate::presets::{PresetStore, Click, ClickSequence, SequenceStepType};

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

// ============================================================================
// ENUMS FOR UI STATE
// ============================================================================

// ============================================================================
// BUTTON TYPE ENUM
// ============================================================================

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
enum ClickButton {
    Left,
    Right,
}

/// Main application screens - enum-based state machine
#[derive(Clone, Debug)]
enum AppScreen {
    MainMenu,
    SequenceEditor(SequenceEditorState),
    ClickEditor(ClickEditorState),
    RunningSequence(RunningSequenceState),
    Settings,
}

#[derive(Clone, Debug)]
struct SequenceEditorState {
    sequence_name: String,
    selected_sequence: Option<String>,
}

#[derive(Clone, Debug)]
struct ClickEditorState {
    editing_click: Option<String>,
    click_name: String,
    click_min_x: i32,
    click_max_x: i32,
    click_min_y: i32,
    click_max_y: i32,
    picking_click_location: bool,
    selected_display: DisplayChoice,
    drag_start: Option<Pos2>,
    drag_end: Option<Pos2>,
    click_interval_min: f32,
    click_interval_max: f32,
}

#[derive(Clone, Debug)]
struct RunningSequenceState {
    sequence_name: String,
    repetitions: u32,
    is_running: bool,
}

struct ClickJob {
    running: Arc<AtomicBool>,
}

impl ClickJob {
    fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

static ENIGO: Lazy<Mutex<enigo::Enigo>> = Lazy::new(|| Mutex::new(enigo::Enigo::new()));

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Helper function to perform interruptible sleep.
fn interruptible_sleep(duration_ms: u64, running: &Arc<AtomicBool>) {
    use std::time::Duration;
    let chunk_size = 50u64;
    for _ in 0..(duration_ms / chunk_size) {
        if !running.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(Duration::from_millis(chunk_size));
    }
    if !running.load(Ordering::Relaxed) {
        return;
    }
    let remainder = duration_ms % chunk_size;
    if remainder > 0 {
        std::thread::sleep(Duration::from_millis(remainder));
    }
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

#[derive(Clone, Copy, PartialEq, Debug)]
enum DisplayChoice {
    All,
    #[allow(dead_code)]
    One(usize),
}

#[derive(Clone, Debug)]
struct Monitor {
    #[allow(dead_code)]
    id: u32,
    #[allow(dead_code)]
    name: String,
    origin_px: (i32, i32),
    size_px: (i32, i32),
    #[allow(dead_code)]
    scale_factor: f32,
}

// ============================================================================
// APP STATE
// ============================================================================

struct AppState {
    // Current screen state
    current_screen: AppScreen,

    // Shared data
    preset_store: PresetStore,
    monitors: Vec<Monitor>,
    display_choice: DisplayChoice,

    // Window state
    saved_window_pos: Option<egui::Pos2>,
    saved_window_size: Option<egui::Vec2>,

    // Picking state (for click location picker)
    picking_location: bool,
    drag_start: Option<Pos2>,
    drag_end: Option<Pos2>,

    // Running job
    job: Option<ClickJob>,
}

impl Default for AppState {
    fn default() -> Self {
        let monitors = query_monitors();
        let preset_store = PresetStore::load_or_default();

        Self {
            current_screen: AppScreen::MainMenu,
            preset_store,
            monitors,
            display_choice: DisplayChoice::All,
            saved_window_pos: None,
            saved_window_size: None,
            picking_location: false,
            drag_start: None,
            drag_end: None,
            job: None,
        }
    }
}

impl AppState {
    #[allow(dead_code)]
    fn refresh_monitors(&mut self) {
        self.monitors = query_monitors();
        if let DisplayChoice::One(i) = self.display_choice {
            if i >= self.monitors.len() {
                self.display_choice = DisplayChoice::All;
            }
        }
    }

    fn enter_location_picker(&mut self, ctx: &egui::Context) {
        self.drag_start = None;
        self.drag_end = None;
        self.picking_location = true;

        ctx.input(|i| {
            if let Some(outer_rect) = i.viewport().outer_rect {
                self.saved_window_pos = Some(outer_rect.left_top());
            }
            if let Some(inner_rect) = i.viewport().inner_rect {
                self.saved_window_size = Some(inner_rect.size());
            }
        });

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

        let ppp = ctx.pixels_per_point().max(0.1);
        let inner = egui::vec2(size_px.0 as f32 / ppp, size_px.1 as f32 / ppp);
        let outer = egui::pos2(origin_px.0 as f32 / ppp, origin_px.1 as f32 / ppp);

        ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(outer));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inner));
    }

    #[allow(dead_code)]
    fn exit_location_picker(&mut self, ctx: &egui::Context) {
        self.picking_location = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(WindowLevel::Normal));

        let size = self.saved_window_size.unwrap_or(egui::vec2(900.0, 700.0));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));

        if let Some(pos) = self.saved_window_pos {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
        }
    }

    #[allow(dead_code)]
    fn handle_location_pick(
        &mut self,
        pos: Pos2,
        ppp: f32,
        ctx: &egui::Context,
    ) -> (i32, i32) {
        let origin_px = match self.display_choice {
            DisplayChoice::All => {
                let (min_x, min_y, _max_x, _max_y) = union_rect(&self.monitors);
                (min_x, min_y)
            }
            DisplayChoice::One(i) => self.monitors.get(i).map(|m| m.origin_px).unwrap_or((0, 0)),
        };

        let x = (pos.x * ppp).round() as i32 + origin_px.0;
        let y = (pos.y * ppp).round() as i32 + origin_px.1;

        self.exit_location_picker(ctx);
        (x, y)
    }

    fn spawn_sequence(&mut self, sequence_name: String, repetitions: u32) {
        if self.job.is_some() {
            return;
        }

        let sequence = match self.preset_store.get_sequence(&sequence_name) {
            Some(seq) => seq.clone(),
            None => {
                eprintln!("Sequence '{}' not found", sequence_name);
                return;
            }
        };

        let preset_store = self.preset_store.clone();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);

        std::thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let mut last_pos: Option<(i32, i32)> = None;
            let mut repeats_remaining = repetitions;

            loop {
                if !running_clone.load(Ordering::Relaxed) {
                    break;
                }

                if repeats_remaining == 0 {
                    running_clone.store(false, Ordering::Relaxed);
                    break;
                }

                // Execute steps in sequence
                if let Err(e) = execute_sequence_steps(
                    &sequence,
                    &preset_store,
                    &running_clone,
                    &mut last_pos,
                    &mut rng,
                ) {
                    eprintln!("Error executing sequence: {}", e);
                    break;
                }

                repeats_remaining -= 1;
            }
        });

        self.job = Some(ClickJob { running });
    }

    fn stop_sequence(&mut self) {
        if let Some(job) = &self.job {
            job.stop();
        }
        self.job = None;
    }
}

// ============================================================================
// SEQUENCE EXECUTION
// ============================================================================

fn execute_sequence_steps(
    sequence: &ClickSequence,
    preset_store: &PresetStore,
    running: &Arc<AtomicBool>,
    last_pos: &mut Option<(i32, i32)>,
    rng: &mut rand::rngs::ThreadRng,
) -> Result<(), String> {
    use rand::Rng;
    use enigo::MouseButton;

    for step in &sequence.steps {
        if !running.load(Ordering::Relaxed) {
            break;
        }

        match step {
            SequenceStepType::Click {
                click_name,
                min_interval: min_int,
                max_interval: max_int,
            } => {
                let click = preset_store
                    .get_click(click_name)
                    .ok_or_else(|| format!("Click '{}' not found", click_name))?;

                // Randomize click coordinates within the bounding box
                let click_x = rng.gen_range(click.min_x..=click.max_x);
                let click_y = rng.gen_range(click.min_y..=click.max_y);

                // Perform the click
                {
                    let mut en = ENIGO.lock();
                    let from = last_pos.unwrap_or((click_x - 40, click_y - 40));
                    let button = match click.button_type.as_str() {
                        "Right" => MouseButton::Right,
                        _ => MouseButton::Left,
                    };

                    human_move_and_click(
                        &mut *en,
                        from,
                        (click_x, click_y),
                        None,
                        &HumanMouseSettings::default(),
                        button,
                    );
                }

                *last_pos = Some((click_x, click_y));

                // Randomize interval between min and max
                let interval_secs = if max_int > min_int {
                    rng.gen_range(*min_int..=*max_int)
                } else {
                    *min_int
                };
                let ms = (interval_secs * 1000.0) as u64;
                if ms > 0 {
                    interruptible_sleep(ms, running);
                }
            }
            SequenceStepType::Subsequence { sequence_name } => {
                let sub_sequence = preset_store
                    .get_sequence(sequence_name)
                    .ok_or_else(|| format!("Sequence '{}' not found", sequence_name))?;

                execute_sequence_steps(
                    sub_sequence,
                    preset_store,
                    running,
                    last_pos,
                    rng,
                )?;
            }
        }
    }

    Ok(())
}

// ============================================================================
// UI RENDERING
// ============================================================================

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Set window to Normal level when not in location picker (allows minimize)
        if !self.picking_location {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(WindowLevel::Normal));
        }

        // -------- Location Picker Overlay (with drag-based selection) --------
        if self.picking_location {
            let screen_rect = ctx.screen_rect();
            let layer_id = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("location_picker"));
            let painter = egui::Painter::new(ctx.clone(), layer_id, egui::Rect::EVERYTHING);

            painter.rect_filled(
                screen_rect,
                0.0,
                Color32::from_rgba_premultiplied(128, 128, 128, 100),
            );

            // Interaction area with drag support
            egui::Area::new(egui::Id::new("location_picker_area"))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    let resp = ui.allocate_rect(screen_rect, Sense::click_and_drag());

                    // Handle drag start
                    if resp.drag_started() {
                        if let AppScreen::ClickEditor(ref mut editor) = self.current_screen {
                            editor.drag_start = resp.interact_pointer_pos();
                            editor.drag_end = editor.drag_start;
                        }
                    }

                    // Handle dragging
                    if resp.dragged() {
                        if let AppScreen::ClickEditor(ref mut editor) = self.current_screen {
                            editor.drag_end = resp.interact_pointer_pos();
                        }
                    }

                    // Handle drag stop (selection complete)
                    if resp.drag_stopped() {
                        if let AppScreen::ClickEditor(ref mut editor) = self.current_screen {
                            editor.drag_end = resp.interact_pointer_pos();

                            // Calculate bounding box from drag
                            if let (Some(a), Some(b)) = (editor.drag_start, editor.drag_end) {
                                let ppp = ctx.pixels_per_point().max(0.1);
                                let origin_px = match self.display_choice {
                                    DisplayChoice::All => {
                                        let (min_x, min_y, _max_x, _max_y) = union_rect(&self.monitors);
                                        (min_x, min_y)
                                    }
                                    DisplayChoice::One(i) => self.monitors.get(i).map(|m| m.origin_px).unwrap_or((0, 0)),
                                };

                                // Calculate min/max from drag points
                                let x1 = (a.x * ppp).round() as i32 + origin_px.0;
                                let y1 = (a.y * ppp).round() as i32 + origin_px.1;
                                let x2 = (b.x * ppp).round() as i32 + origin_px.0;
                                let y2 = (b.y * ppp).round() as i32 + origin_px.1;

                                editor.click_min_x = x1.min(x2);
                                editor.click_max_x = x1.max(x2);
                                editor.click_min_y = y1.min(y2);
                                editor.click_max_y = y1.max(y2);
                                editor.drag_start = None;
                                editor.drag_end = None;
                                editor.picking_click_location = false;
                            }
                        }
                        // Exit the location picker and restore window
                        self.picking_location = false;
                        if let Some(pos) = self.saved_window_pos {
                            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                        }
                        if let Some(size) = self.saved_window_size {
                            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                        }
                        ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
                    }

                    // Draw drag selection rectangle
                    if let AppScreen::ClickEditor(editor) = &self.current_screen {
                        if let (Some(a), Some(b)) = (editor.drag_start, editor.drag_end) {
                            let rect = Rect::from_two_pos(a, b);
                            let stroke = egui::Stroke { width: 2.0, color: Color32::LIGHT_BLUE };
                            painter.rect_stroke(rect, 0.0, stroke);
                        }

                        // Draw coordinate display at cursor
                        if resp.hovered() {
                            if let Some(pos) = resp.interact_pointer_pos() {
                                let ppp = ctx.pixels_per_point().max(0.1);
                                let origin_px = match self.display_choice {
                                    DisplayChoice::All => {
                                        let (min_x, min_y, _max_x, _max_y) = union_rect(&self.monitors);
                                        (min_x, min_y)
                                    }
                                    DisplayChoice::One(i) => self.monitors.get(i).map(|m| m.origin_px).unwrap_or((0, 0)),
                                };

                                let x = (pos.x * ppp).round() as i32 + origin_px.0;
                                let y = (pos.y * ppp).round() as i32 + origin_px.1;
                                let text = format!("X: {}\nY: {}", x, y);
                                painter.text(
                                    pos + egui::vec2(10.0, 10.0),
                                    egui::Align2::LEFT_TOP,
                                    text,
                                    egui::FontId::default(),
                                    Color32::WHITE,
                                );
                            }
                        }
                    }
                });

            ctx.request_repaint();
            return;
        }

        // -------- Main Screen Rendering --------
        match &self.current_screen.clone() {
            AppScreen::MainMenu => self.render_main_menu(ctx),
            AppScreen::SequenceEditor(editor_state) => {
                self.render_sequence_editor(ctx, editor_state.clone())
            }
            AppScreen::ClickEditor(editor_state) => {
                self.render_click_editor(ctx, editor_state.clone())
            }
            AppScreen::RunningSequence(running_state) => {
                self.render_running_sequence(ctx, running_state.clone())
            }
            AppScreen::Settings => {
                self.render_settings(ctx);
            }
        }
    }
}

impl AppState {
    fn render_main_menu(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.heading("Area Clicker — Sequence Manager");
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);

                ui.heading("Main Menu");

                ui.add_space(30.0);

                if ui.button(egui::RichText::new("New Sequence").size(20.0)).clicked() {
                    self.current_screen = AppScreen::SequenceEditor(SequenceEditorState {
                        sequence_name: String::new(),
                        selected_sequence: None,
                    });
                }

                ui.add_space(15.0);

                if ui.button(egui::RichText::new("Edit Sequence").size(20.0)).clicked() {
                    self.current_screen = AppScreen::SequenceEditor(SequenceEditorState {
                        sequence_name: String::new(),
                        selected_sequence: None,
                    });
                }

                ui.add_space(15.0);

                if ui.button(egui::RichText::new("Manage Clicks").size(20.0)).clicked() {
                    self.current_screen = AppScreen::ClickEditor(ClickEditorState {
                        editing_click: None,
                        click_name: String::new(),
                        click_min_x: 0,
                        click_max_x: 0,
                        click_min_y: 0,
                        click_max_y: 0,
                        picking_click_location: false,
                        selected_display: DisplayChoice::All,
                        drag_start: None,
                        drag_end: None,
                        click_interval_min: 0.5,
                        click_interval_max: 1.0,
                    });
                }

                ui.add_space(15.0);

                if ui.button(egui::RichText::new("Settings").size(20.0)).clicked() {
                    self.current_screen = AppScreen::Settings;
                }

                ui.add_space(15.0);

                if ui.button(egui::RichText::new("Exit").size(20.0)).clicked() {
                    std::process::exit(0);
                }
            });
        });
    }

    fn render_sequence_editor(&mut self, ctx: &egui::Context, mut editor: SequenceEditorState) {
        let mut go_back = false;
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.heading("Sequence Editor");
            ui.horizontal(|ui| {
                if ui.button("← Back to Menu").clicked() {
                    go_back = true;
                }
            });
        });

        if go_back {
            self.current_screen = AppScreen::MainMenu;
            return;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.group(|ui| {
                        ui.label("Sequence Name:");
                        ui.text_edit_singleline(&mut editor.sequence_name);

                        ui.label("Select Existing Sequence:");
                        let selected_text = editor.selected_sequence.clone().unwrap_or_else(|| "None".into());
                        egui::ComboBox::from_id_source("seq_select")
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut editor.selected_sequence, None, "None");
                                for seq in &self.preset_store.sequences {
                                    ui.selectable_value(&mut editor.selected_sequence, Some(seq.name.clone()), &seq.name);
                                }
                            });

                        if ui.button("Load Selected").clicked() {
                            if let Some(seq_name) = &editor.selected_sequence {
                                if let Some(seq) = self.preset_store.get_sequence(seq_name) {
                                    editor.sequence_name = seq.name.clone();
                                }
                            }
                        }
                    });

                    ui.separator();

                    // Display and manage sequence steps
                    if self.preset_store.get_sequence(&editor.sequence_name).is_some() {
                        let seq_steps: Vec<(usize, SequenceStepType)> = self.preset_store.get_sequence(&editor.sequence_name)
                            .map(|seq| seq.steps.iter().enumerate().map(|(i, s)| (i, s.clone())).collect())
                            .unwrap_or_default();

                        if !seq_steps.is_empty() {
                            ui.group(|ui| {
                                ui.label(format!("Steps in '{}':", editor.sequence_name));

                                let step_count = seq_steps.len();
                                for (i, step) in seq_steps.iter() {
                                    ui.horizontal(|ui| {
                                        match step {
                                            SequenceStepType::Click { click_name, min_interval, max_interval } => {
                                                ui.label(format!("Step {}: Click '{}' ({}s-{}s)", i + 1, click_name, min_interval, max_interval));
                                            }
                                            SequenceStepType::Subsequence { sequence_name } => {
                                                ui.label(format!("Step {}: Sequence '{}'", i + 1, sequence_name));
                                            }
                                        }

                                        // Edit interval button for clicks
                                        if matches!(step, SequenceStepType::Click { .. }) && ui.button("Edit").clicked() {
                                            if let Some(seq_mut) = self.preset_store.get_sequence_mut(&editor.sequence_name) {
                                                if *i < seq_mut.steps.len() {
                                                    if let SequenceStepType::Click { .. } = &mut seq_mut.steps[*i] {
                                                        // Toggle interval editing or modify inline
                                                        // For now, show the current values via label
                                                    }
                                                }
                                            }
                                        }

                                        // Move up button
                                        if *i > 0 && ui.button("↑").clicked() {
                                            if let Some(seq_mut) = self.preset_store.get_sequence_mut(&editor.sequence_name) {
                                                seq_mut.steps.swap(*i, i - 1);
                                                let _ = self.preset_store.save();
                                            }
                                        }

                                        // Move down button
                                        if *i < step_count - 1 && ui.button("↓").clicked() {
                                            if let Some(seq_mut) = self.preset_store.get_sequence_mut(&editor.sequence_name) {
                                                seq_mut.steps.swap(*i, i + 1);
                                                let _ = self.preset_store.save();
                                            }
                                        }

                                        // Delete button
                                        if ui.button("×").clicked() {
                                            if let Some(seq_mut) = self.preset_store.get_sequence_mut(&editor.sequence_name) {
                                                seq_mut.steps.remove(*i);
                                                let _ = self.preset_store.save();
                                            }
                                        }
                                    });
                                }
                            });
                        }
                    }

                    if self.preset_store.get_sequence(&editor.sequence_name).is_some() {
                        ui.separator();

                        ui.group(|ui| {
                            ui.label("Add Step - Click:");
                            let click_names: Vec<String> = self.preset_store.clicks.iter().map(|c| c.name.clone()).collect();
                            if !click_names.is_empty() {
                                let mut selected_click = String::new();
                                egui::ComboBox::from_id_source("add_click_step")
                                    .selected_text("Select click...")
                                    .show_ui(ui, |ui| {
                                        for click_name in &click_names {
                                            if ui.selectable_value(&mut selected_click, click_name.clone(), click_name).clicked() {
                                                // Get the click's default intervals first (before mutable borrow)
                                                let (min_int, max_int) = if let Some(click) = self.preset_store.clicks.iter().find(|c| c.name == *click_name) {
                                                    (click.min_interval, click.max_interval)
                                                } else {
                                                    (0.5, 1.0)
                                                };
                                                
                                                if let Some(seq_mut) = self.preset_store.get_sequence_mut(&editor.sequence_name) {
                                                    seq_mut.steps.push(SequenceStepType::Click {
                                                        click_name: click_name.clone(),
                                                        min_interval: min_int,
                                                        max_interval: max_int,
                                                    });
                                                    let _ = self.preset_store.save();
                                                }
                                            }
                                        }
                                    });
                            } else {
                                ui.label("(No clicks available)");
                            }
                        });

                        ui.group(|ui| {
                            ui.label("Add Step - Subsequence:");
                            let seq_names: Vec<String> = self.preset_store.sequences.iter()
                                .filter(|s| s.name != editor.sequence_name)
                                .map(|s| s.name.clone())
                                .collect();
                            if !seq_names.is_empty() {
                                let mut selected_seq = String::new();
                                egui::ComboBox::from_id_source("add_subseq_step")
                                    .selected_text("Select sequence...")
                                    .show_ui(ui, |ui| {
                                        for seq_name in &seq_names {
                                            if ui.selectable_value(&mut selected_seq, seq_name.clone(), seq_name).clicked() {
                                                if let Some(seq_mut) = self.preset_store.get_sequence_mut(&editor.sequence_name) {
                                                    seq_mut.steps.push(SequenceStepType::Subsequence {
                                                        sequence_name: seq_name.clone(),
                                                    });
                                                    let _ = self.preset_store.save();
                                                }
                                            }
                                        }
                                    });
                            } else {
                                ui.label("(No sequences available)");
                            }
                        });
                    } else {
                        ui.label("Create a sequence first by entering a name above");
                        if ui.button("Create New Sequence").clicked() {
                            let name = editor.sequence_name.trim();
                            if !name.is_empty() {
                                let sequence = ClickSequence {
                                    name: name.to_string(),
                                    steps: Vec::new(),
                                };
                                self.preset_store.upsert_sequence(sequence);
                                let _ = self.preset_store.save();
                            }
                        }
                    }

                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui.button("Run Sequence").clicked() {
                            if let Some(seq) = self.preset_store.get_sequence(&editor.sequence_name) {
                                self.current_screen = AppScreen::RunningSequence(RunningSequenceState {
                                    sequence_name: seq.name.clone(),
                                    repetitions: 1,
                                    is_running: false,
                                });
                            }
                        }

                        if ui.button("Save & Run").clicked() {
                            if let Some(seq) = self.preset_store.get_sequence(&editor.sequence_name) {
                                self.current_screen = AppScreen::RunningSequence(RunningSequenceState {
                                    sequence_name: seq.name.clone(),
                                    repetitions: 1,
                                    is_running: false,
                                });
                            }
                        }

                        if ui.button("Delete Sequence").clicked() {
                            self.preset_store.remove_sequence(&editor.sequence_name);
                            let _ = self.preset_store.save();
                            self.current_screen = AppScreen::MainMenu;
                        }
                    });
                });
        });

        self.current_screen = AppScreen::SequenceEditor(editor);
    }

    fn render_click_editor(&mut self, ctx: &egui::Context, mut editor: ClickEditorState) {
        let mut go_back = false;
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.heading("Click Editor");
            ui.horizontal(|ui| {
                if ui.button("← Back to Menu").clicked() {
                    go_back = true;
                }
            });
        });

        if go_back {
            self.current_screen = AppScreen::MainMenu;
            return;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.group(|ui| {
                        ui.label("Click Name:");
                        ui.text_edit_singleline(&mut editor.click_name);

                        ui.label("Select Display:");
                        let mut selected_display_str = match editor.selected_display {
                            DisplayChoice::All => "All Displays".to_string(),
                            DisplayChoice::One(i) => format!("Display {}", i + 1),
                        };
                        
                        egui::ComboBox::from_id_source("select_display_for_click")
                            .selected_text(&selected_display_str)
                            .show_ui(ui, |ui| {
                                if ui.selectable_value(&mut selected_display_str, "All Displays".to_string(), "All Displays").clicked() {
                                    editor.selected_display = DisplayChoice::All;
                                }
                                for (i, monitor) in self.monitors.iter().enumerate() {
                                    let is_primary = i == 0;
                                    let primary_label = if is_primary { " (main display)" } else { "" };
                                    let label = format!("Display {}{} ({}x{})", i + 1, primary_label, monitor.size_px.0, monitor.size_px.1);
                                    if ui.selectable_value(&mut selected_display_str, format!("Display {}", i + 1), label).clicked() {
                                        editor.selected_display = DisplayChoice::One(i);
                                    }
                                }
                            });

                        ui.label(format!("X: {} - {}", editor.click_min_x, editor.click_max_x));
                        ui.label(format!("Y: {} - {}", editor.click_min_y, editor.click_max_y));

                        ui.separator();

                        ui.label("Wait After Click (Interval Timing):");
                        ui.horizontal(|ui| {
                            ui.label("Min (seconds):");
                            ui.add(egui::DragValue::new(&mut editor.click_interval_min)
                                .speed(0.1)
                                .clamp_range(0.1..=3600.0));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Max (seconds):");
                            ui.add(egui::DragValue::new(&mut editor.click_interval_max)
                                .speed(0.1)
                                .clamp_range(0.1..=3600.0));
                        });

                        if ui.button("Pick Location from Screen").clicked() {
                            editor.picking_click_location = true;
                            self.display_choice = editor.selected_display.clone();
                            self.enter_location_picker(ctx);
                        }

                        ui.separator();

                        if ui.button("Save Click").clicked() {
                            let name = editor.click_name.trim();
                            if !name.is_empty() {
                                let mut click = Click::new(name.to_string(), editor.click_min_x, editor.click_max_x, editor.click_min_y, editor.click_max_y);
                                click.min_interval = editor.click_interval_min;
                                click.max_interval = editor.click_interval_max;
                                self.preset_store.upsert_click(click);
                                let _ = self.preset_store.save();
                                editor.click_name.clear();
                                editor.click_min_x = 0;
                                editor.click_max_x = 0;
                                editor.click_min_y = 0;
                                editor.click_max_y = 0;
                                editor.click_interval_min = 0.5;
                                editor.click_interval_max = 1.0;
                            }
                        }
                    });

                    ui.separator();

                    ui.group(|ui| {
                        ui.label("Existing Clicks:");
                        let click_list: Vec<(String, i32, i32, i32, i32, f32, f32)> = self.preset_store.clicks.iter()
                            .map(|c| (c.name.clone(), c.min_x, c.max_x, c.min_y, c.max_y, c.min_interval, c.max_interval))
                            .collect();

                        for (click_name, min_x, max_x, min_y, max_y, min_interval, max_interval) in click_list {
                            ui.horizontal(|ui| {
                                ui.label(format!("{}: X({}-{}) Y({}-{})", click_name, min_x, max_x, min_y, max_y));

                                if ui.button("Edit").clicked() {
                                    editor.editing_click = Some(click_name.clone());
                                    editor.click_name = click_name.clone();
                                    editor.click_min_x = min_x;
                                    editor.click_max_x = max_x;
                                    editor.click_min_y = min_y;
                                    editor.click_max_y = max_y;
                                    editor.click_interval_min = min_interval;
                                    editor.click_interval_max = max_interval;
                                }

                                if ui.button("Delete").clicked() {
                                    self.preset_store.remove_click(&click_name);
                                    let _ = self.preset_store.save();
                                }
                            });
                        }
                    });
                });
        });

        self.current_screen = AppScreen::ClickEditor(editor);
    }

    fn render_running_sequence(&mut self, ctx: &egui::Context, mut running: RunningSequenceState) {
        let mut go_back = false;
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.heading("Running Sequence");
            ui.horizontal(|ui| {
                if ui.button("← Back").clicked() {
                    go_back = true;
                }
            });
        });

        if go_back {
            self.stop_sequence();
            self.current_screen = AppScreen::MainMenu;
            return;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);

                ui.heading(&running.sequence_name);

                ui.add_space(30.0);

                ui.label("Repetitions:");
                ui.add(egui::DragValue::new(&mut running.repetitions).clamp_range(1..=1000));

                ui.add_space(30.0);

                if !running.is_running {
                    if ui.button(egui::RichText::new("Start").size(20.0)).clicked() {
                        self.spawn_sequence(running.sequence_name.clone(), running.repetitions);
                        running.is_running = true;
                    }
                } else {
                    if ui.button(egui::RichText::new("Stop").size(20.0)).clicked() {
                        self.stop_sequence();
                        running.is_running = false;
                    }
                }

                ui.add_space(30.0);

                if let Some(job) = &self.job {
                    let status = if job.running.load(Ordering::Relaxed) {
                        "Running..."
                    } else {
                        "Stopped"
                    };
                    ui.label(format!("Status: {}", status));
                }
            });
        });

        self.current_screen = AppScreen::RunningSequence(running);
    }

    fn render_settings(&mut self, ctx: &egui::Context) {
        let mut go_back = false;
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.heading("Settings - Run Sequence");
            ui.horizontal(|ui| {
                if ui.button("← Back to Menu").clicked() {
                    go_back = true;
                }
            });
        });

        if go_back {
            self.current_screen = AppScreen::MainMenu;
            return;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.heading("Available Sequences:");
                    
                    if self.preset_store.sequences.is_empty() {
                        ui.label("(No sequences created yet)");
                    } else {
                        for seq in &self.preset_store.sequences {
                            ui.horizontal(|ui| {
                                ui.label(format!("{}  ({} steps)", seq.name, seq.steps.len()));
                                
                                if ui.button("Run").clicked() {
                                    self.current_screen = AppScreen::RunningSequence(RunningSequenceState {
                                        sequence_name: seq.name.clone(),
                                        repetitions: 1,
                                        is_running: false,
                                    });
                                    return;
                                }

                                if ui.button("Edit").clicked() {
                                    self.current_screen = AppScreen::SequenceEditor(SequenceEditorState {
                                        sequence_name: seq.name.clone(),
                                        selected_sequence: None,
                                    });
                                    return;
                                }
                            });
                        }
                    }
                });
        });
    }
}

fn main() -> eframe::Result<()> {
    let mut opts = eframe::NativeOptions::default();
    let _args = Args::parse();

    opts.viewport.transparent = Some(true);
    opts.viewport.resizable = Some(true);
    opts.viewport.mouse_passthrough = Some(false);
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
