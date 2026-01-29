# Code Examples - Enum-Based Architecture

## Overview
This document shows practical code examples from the refactored Area Clicker application, demonstrating enum-based state management and type-safe data handling.

---

## Example 1: State Machine with Match Expressions

### Before (Flag-Based)
```rust
fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
    if self.picking_area {
        // Area picker overlay code
    } else if self.picking_point {
        // Point picker overlay code
    } else if self.editing_sequence {
        // Sequence editor code
    } else if self.editing_click {
        // Click editor code
    } else {
        // Main UI code
    }
}
```

**Problems**:
- Multiple conditions could be true
- Easy to miss cases
- No compile-time guarantee

### After (Enum-Based)
```rust
fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    // Location picker overlay (works with any screen)
    if self.picking_location {
        // Fullscreen picker overlay...
        return;
    }

    // Main screen rendering - only ONE screen renders at a time
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
    }
}
```

**Benefits**:
- Only one screen renders
- Exhaustive matching enforced by compiler
- Clear intent and flow
- Type-safe state access

---

## Example 2: Enum-Based Data Models

### Before (String-Based)
```rust
pub struct SequenceStep {
    pub area_name: String,
    pub min_interval: f32,
    pub max_interval: f32,
    pub interval_secs: f32,
    pub button_type: String,
}

// Later, string checking:
if step.area_name.starts_with("sequence:") {
    let seq_name = &step.area_name[9..];
    // Handle sequence
} else {
    // Handle area
}
```

**Problems**:
- Type information implicit in strings
- No validation at compile time
- Easy to make mistakes
- Hard to refactor

### After (Enum-Based)
```rust
pub enum SequenceStepType {
    Click {
        click_name: String,
        min_interval: f32,
        max_interval: f32,
    },
    Subsequence {
        sequence_name: String,
    },
}

// Clear, exhaustive pattern matching:
match step {
    SequenceStepType::Click { click_name, min_interval, max_interval } => {
        // All fields available directly
        // Execute click with interval
    }
    SequenceStepType::Subsequence { sequence_name } => {
        // Handle sequence inclusion
    }
}
```

**Benefits**:
- Type-safe, compiler-checked
- All possible variants explicit
- No string-based hacks
- Self-documenting code

---

## Example 3: State Transition

### Code
```rust
// In click editor, when user clicks "Save & Run":
if ui.button("Save & Run").clicked() {
    if let Some(seq) = self.preset_store.get_sequence(&editor.sequence_name) {
        // Transition to RunningSequence screen
        self.current_screen = AppScreen::RunningSequence(RunningSequenceState {
            sequence_name: seq.name.clone(),
            repetitions: 1,
            is_running: false,
        });
    }
}

// Back to menu:
if ui.button("← Back to Menu").clicked() {
    self.current_screen = AppScreen::MainMenu;
}
```

**Pattern**:
1. Check conditions
2. Create new AppScreen variant
3. Assign to `current_screen`
4. Next update() call renders new screen

---

## Example 4: Recursive Sequence Execution

### Code
```rust
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
                min_interval,
                max_interval,
            } => {
                // Look up the actual Click
                let click = preset_store
                    .get_click(click_name)
                    .ok_or_else(|| format!("Click '{}' not found", click_name))?;

                // Perform the click
                {
                    let mut en = ENIGO.lock();
                    let from = last_pos.unwrap_or((click.x - 40, click.y - 40));
                    let button = match click.button_type.as_str() {
                        "Right" => MouseButton::Right,
                        _ => MouseButton::Left,
                    };

                    human_move_and_click(
                        &mut *en,
                        from,
                        (click.x, click.y),
                        None,
                        &HumanMouseSettings::default(),
                        button,
                    );
                }

                *last_pos = Some((click.x, click.y));

                // Wait before next step
                let interval_secs = if max_interval > min_interval {
                    rng.gen_range(*min_interval..=*max_interval)
                } else {
                    *min_interval
                };
                let ms = (interval_secs * 1000.0) as u64;
                if ms > 0 {
                    interruptible_sleep(ms, running);
                }
            }

            SequenceStepType::Subsequence { sequence_name } => {
                // Recursive call for nested sequences
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
```

**Key Points**:
- Pattern match on SequenceStepType
- Handle Click case: lookup click, execute movement
- Handle Subsequence case: recursively call itself
- Error handling with Result type
- Both branches can share common variables (last_pos, running)

---

## Example 5: UI with State-Specific Rendering

### Click Editor Rendering
```rust
fn render_click_editor(&mut self, ctx: &egui::Context, mut editor: ClickEditorState) {
    egui::TopBottomPanel::top("header").show(ctx, |ui| {
        ui.heading("Click Editor");
        ui.horizontal(|ui| {
            if ui.button("← Back to Menu").clicked() {
                self.current_screen = AppScreen::MainMenu;
            }
        });
    });

    egui::CentralPanel::default().show(ctx, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.group(|ui| {
                    ui.label("Click Name:");
                    ui.text_edit_singleline(&mut editor.click_name);

                    ui.label(format!("X: {}", editor.click_x));
                    ui.label(format!("Y: {}", editor.click_y));

                    if ui.button("Pick Location from Screen").clicked() {
                        editor.picking_click_location = true;
                        self.enter_location_picker(ctx);
                    }

                    ui.separator();

                    if ui.button("Save Click").clicked() {
                        let name = editor.click_name.trim();
                        if !name.is_empty() {
                            let click = Click::new(name.to_string(), editor.click_x, editor.click_y);
                            self.preset_store.upsert_click(click);
                            let _ = self.preset_store.save();
                            editor.click_name.clear();
                            editor.click_x = 0;
                            editor.click_y = 0;
                        }
                    }
                });

                ui.separator();

                ui.group(|ui| {
                    ui.label("Existing Clicks:");
                    // Collect data first to avoid borrow conflicts
                    let click_list: Vec<(String, i32, i32)> = self.preset_store.clicks.iter()
                        .map(|c| (c.name.clone(), c.x, c.y))
                        .collect();

                    for (click_name, click_x, click_y) in click_list {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}: ({}, {})", click_name, click_x, click_y));

                            if ui.button("Edit").clicked() {
                                editor.editing_click = Some(click_name.clone());
                                editor.click_name = click_name.clone();
                                editor.click_x = click_x;
                                editor.click_y = click_y;
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

    // Update the current screen with the modified state
    self.current_screen = AppScreen::ClickEditor(editor);
}
```

**Pattern**:
1. Function signature receives the state variant
2. Clone mutable state for modifications
3. Use `&mut self` for shared state (preset_store)
4. Update `current_screen` at the end with potentially modified state

---

## Example 6: Data Model with Serde Serialization

### Rust Code
```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Click {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub button_type: String,
}

impl Click {
    pub fn new(name: String, x: i32, y: i32) -> Self {
        Self {
            name,
            x,
            y,
            button_type: "Left".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClickSequence {
    pub name: String,
    pub steps: Vec<SequenceStepType>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SequenceStepType {
    Click {
        click_name: String,
        min_interval: f32,
        max_interval: f32,
    },
    Subsequence {
        sequence_name: String,
    },
}
```

### TOML Output
```toml
[[clicks]]
name = "ButtonA"
x = 1920
y = 1080
button_type = "Left"

[[clicks]]
name = "ButtonB"
x = 500
y = 600
button_type = "Right"

[[sequences]]
name = "MainFlow"

[[sequences.steps]]
Click = { click_name = "ButtonA", min_interval = 0.5, max_interval = 1.0 }

[[sequences.steps]]
Click = { click_name = "ButtonB", min_interval = 2.0, max_interval = 3.0 }

[[sequences]]
name = "AlternateFlow"

[[sequences.steps]]
Subsequence = { sequence_name = "MainFlow" }

[[sequences.steps]]
Click = { click_name = "ButtonA", min_interval = 1.0, max_interval = 2.0 }
```

**Benefits**:
- Serde automatically handles serialization
- Enums serialize as variants
- Type-safe deserialization
- Can be edited manually if needed

---

## Example 7: Error Handling with Result

### Code
```rust
// Execution with error propagation
fn spawn_sequence(&mut self, sequence_name: String, repetitions: u32) {
    if self.job.is_some() {
        return;
    }

    // Look up the sequence
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

            // Execute sequence and handle errors
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
```

**Pattern**:
- Use `match` for Option types
- Use `if let Err()` for Result types
- Early returns for invalid conditions
- Error messages logged but don't crash thread

---

## Example 8: Borrow Checker Best Practice

### Problem Code (Doesn't Compile)
```rust
// DON'T DO THIS
if let Some(seq) = self.preset_store.get_sequence(&editor.sequence_name) {
    ui.group(|ui| {
        ui.label(format!("Steps in '{}':", editor.sequence_name));
        for (i, step) in seq.steps.iter().enumerate() {
            // seq is borrowed here...
            if ui.button("×").clicked() {
                if let Some(seq_mut) = self.preset_store.get_sequence_mut(&editor.sequence_name) {
                    // ...and we're trying to borrow mutably here!
                    seq_mut.steps.remove(i);
                }
            }
        }
    });
}
```

### Solution (Compiles)
```rust
// DO THIS - collect data first
let seq_steps: Vec<(usize, SequenceStepType)> = self.preset_store
    .get_sequence(&editor.sequence_name)
    .map(|seq| seq.steps.iter().enumerate().map(|(i, s)| (i, s.clone())).collect())
    .unwrap_or_default();

if !seq_steps.is_empty() {
    ui.group(|ui| {
        ui.label(format!("Steps in '{}':", editor.sequence_name));
        for (i, step) in seq_steps {
            // No borrow here - we own the data
            if ui.button("×").clicked() {
                if let Some(seq_mut) = self.preset_store.get_sequence_mut(&editor.sequence_name) {
                    // Now we can borrow mutably
                    seq_mut.steps.remove(i);
                    let _ = self.preset_store.save();
                }
            }
        }
    });
}
```

**Pattern**:
- Collect owned data early
- Do mutations in separate block
- Avoid holding references across mutable operations

---

## Summary of Patterns

| Pattern | Use Case | Example |
|---------|----------|---------|
| **Match Enum** | Handle state variants | `match current_screen { ... }` |
| **Pattern Binding** | Extract enum data | `SequenceStepType::Click { click_name, ... }` |
| **Match Result** | Handle Option/Result | `match preset_store.get_click() { Some(...) }` |
| **State Transition** | Change screens | `self.current_screen = AppScreen::...` |
| **Collect First** | Avoid borrow issues | `let data = iter.map(...).collect()` |
| **Recursive Match** | Nested sequences | Call `execute_sequence_steps` recursively |

These examples demonstrate idiomatic Rust patterns that make the code safe, clear, and maintainable.
