# Enum-Based Refactoring Guide

## Why Enums?

The original code used many boolean flags and string-based state tracking:

```rust
// OLD APPROACH - Many flags, hard to track valid combinations
picking_area: bool,
picking_point: bool,
selected_preset: Option<String>,
selected_area: Option<String>,
selected_sequence: Option<String>,
// ... and many more fields
```

Problems:
- Multiple flags could be true simultaneously (invalid states)
- Hard to ensure consistency
- Difficult to add new screens without touching multiple fields
- Boilerplate checking for invalid combinations

## New Approach - Type-Safe State Machine

```rust
enum AppScreen {
    MainMenu,
    SequenceEditor(SequenceEditorState),
    ClickEditor(ClickEditorState),
    RunningSequence(RunningSequenceState),
}
```

Benefits:
- **Type Safety**: Rust compiler prevents invalid states
- **Pattern Matching**: Clear intent with `match` expressions
- **Exhaustiveness**: Compiler warns if you forget to handle a case
- **Single Responsibility**: Each screen state only holds relevant data

## Screen States

### MainMenu
No state needed - just show buttons.

### SequenceEditor
```rust
struct SequenceEditorState {
    sequence_name: String,
    selected_sequence: Option<String>,
}
```
Used when creating or editing sequences.

### ClickEditor
```rust
struct ClickEditorState {
    editing_click: Option<String>,
    click_name: String,
    click_x: i32,
    click_y: i32,
    picking_click_location: bool,
}
```
Used when creating or editing clicks.

### RunningSequence
```rust
struct RunningSequenceState {
    sequence_name: String,
    repetitions: u32,
    is_running: bool,
}
```
Used when executing a sequence.

## Data Models with Enums

### SequenceStepType

Instead of storing "area names" in a generic step structure, we use an enum to represent the two types of steps:

```rust
// NEW - Enum-based, type-safe
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

Pattern matching makes handling each type explicit:

```rust
match step {
    SequenceStepType::Click { click_name, min_interval, max_interval } => {
        // Handle click step
    }
    SequenceStepType::Subsequence { sequence_name } => {
        // Handle subsequence step
    }
}
```

Compare to the old approach:
```rust
// OLD - String-based type detection, error-prone
pub struct SequenceStep {
    pub area_name: String,
    pub min_interval: f32,
    pub max_interval: f32,
}
// ... later ...
if some_string == "area" { /* handle area */ }
else if some_string == "sequence" { /* handle sequence */ }
```

## State Transitions

The application follows a clear state machine:

```
                   ┌──────────────────────────────┐
                   │       Main Menu              │
                   │  ┌──────┬──────┬──────────┐  │
                   │  │      │      │          │  │
      ┌────────────▼──▼──┐   │      │          │
      │ Sequence Editor  │   │      │          │
      │ - Create/Edit    │   │      │          │
      │ - Add steps      │   │      │          │
      └────────────┬─────┘   │      │          │
                   │         │      │          │
      ┌────────────▼──┐      │      │          │
      │Click Editor   │      │      │          │
      │- Create/Edit  │      │      │          │
      │- Pick location│      │      │          │
      └────────────┬──┘      │      │          │
                   │         │      │          │
      ┌────────────▼──┐      │      │          │
      │Running        │      │      │          │
      │Sequence       │      │      │          │
      │- Execute      │      │      │          │
      │- Pause/Stop   │      │      │          │
      └────────────┬──┘      │      │          │
                   │         │      │          │
                   └─────┬───┴──────┴──────────┘
                         │
                   Returns to Menu
```

Transitions happen by setting `current_screen`:

```rust
// Go to sequence editor
self.current_screen = AppScreen::SequenceEditor(SequenceEditorState {
    sequence_name: String::new(),
    selected_sequence: None,
});

// Go back to menu
self.current_screen = AppScreen::MainMenu;
```

## Rendering with Match Expressions

Instead of conditional logic based on flags:

```rust
// OLD
if self.picking_area { /* render area picker */ }
else if self.picking_point { /* render point picker */ }
else if self.editing_sequence { /* render sequence editor */ }
else { /* render main UI */ }
```

We match on the enum:

```rust
// NEW
match &self.current_screen {
    AppScreen::MainMenu => self.render_main_menu(ctx),
    AppScreen::SequenceEditor(state) => self.render_sequence_editor(ctx, state.clone()),
    AppScreen::ClickEditor(state) => self.render_click_editor(ctx, state.clone()),
    AppScreen::RunningSequence(state) => self.render_running_sequence(ctx, state.clone()),
}
```

Benefits:
- Impossible to render multiple screens at once
- Each `render_*` function only receives relevant state
- Adding a new screen requires new match arm (compiler ensures coverage)

## Data Models - Click and Sequence

### Click
A simple, focused struct for a single click:

```rust
pub struct Click {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub button_type: String, // "Left" or "Right"
}
```

### ClickSequence
A sequence is just a list of steps:

```rust
pub struct ClickSequence {
    pub name: String,
    pub steps: Vec<SequenceStepType>,
}
```

The enum-based `SequenceStepType` allows mixing different kinds of steps cleanly.

## Handling Shared State

Some state is shared across screens (like `preset_store`):

```rust
struct AppState {
    current_screen: AppScreen,
    preset_store: PresetStore,
    monitors: Vec<Monitor>,
    // ... other shared state
}
```

Editors can mutate shared state while rendering:

```rust
fn render_click_editor(&mut self, ctx: &egui::Context, mut editor: ClickEditorState) {
    // ... UI code ...
    if ui.button("Save Click").clicked() {
        let click = Click::new(editor.click_name, editor.click_x, editor.click_y);
        self.preset_store.upsert_click(click);  // Mutate shared state
        let _ = self.preset_store.save();
    }
    // ... update editor back to current_screen
    self.current_screen = AppScreen::ClickEditor(editor);
}
```

## Borrow Checking Patterns

The refactored code carefully handles Rust's borrow checker:

### Problem: Holding references in closures
```rust
// DON'T DO THIS - closure lives beyond reference
if let Some(seq) = self.preset_store.get_sequence(&name) {
    ui.group(|ui| {
        for step in seq.steps {  // seq borrowed here
            // ...
            self.preset_store.get_sequence_mut(&name);  // Can't borrow mutably!
        }
    });
}
```

### Solution: Collect data first
```rust
// DO THIS - collect data first
let seq_steps: Vec<_> = self.preset_store.get_sequence(&name)
    .map(|seq| seq.steps.clone())
    .unwrap_or_default();

ui.group(|ui| {
    for step in seq_steps {
        // ... no mutable borrow conflict ...
        self.preset_store.get_sequence_mut(&name);  // OK!
    }
});
```

## Migration Path

If you're working with the old code:

1. **Old**: Boolean flags → **New**: Single enum
2. **Old**: Generic `SequenceStep` with area/sequence names → **New**: `SequenceStepType` enum
3. **Old**: Preset/Area/Point-focused → **New**: Click/Sequence-focused
4. **Old**: String-based screen detection → **New**: Pattern matching

## Benefits of This Approach

1. **Correctness**: Compiler prevents invalid states
2. **Maintainability**: Clear responsibility per screen
3. **Scalability**: Easy to add new screens
4. **Performance**: No runtime state checking
5. **Readability**: Intent is explicit with pattern matching
6. **Testability**: Each screen state is independent

## Testing the Architecture

To verify the state machine works correctly:

1. Create a click → verify Click is saved
2. Create a sequence with a click → verify ClickSequence is saved
3. Execute sequence → verify SequenceStepType::Click is handled correctly
4. Add subsequence → verify SequenceStepType::Subsequence is handled correctly
5. Return to menu → verify state transitions work smoothly

## Future Improvements

### Add More Screens
```rust
enum AppScreen {
    // ... existing ...
    SettingsScreen(SettingsState),
    HelpScreen,
}
```

### More Complex States
```rust
struct SequenceEditorState {
    sequence_name: String,
    selected_sequence: Option<String>,
    edit_mode: EditMode,  // Enum for what's being edited
}

enum EditMode {
    ViewingSteps,
    EditingStep(usize),
    AddingStep(AddStepMode),
}
```

### Result Types for Validation
```rust
enum ValidationResult {
    Ok,
    ClickNotFound(String),
    SequenceNotFound(String),
    InvalidInterval,
}
```

This enum-based architecture provides a solid foundation for future enhancements while maintaining type safety and clarity.
