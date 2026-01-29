# Area Clicker - Enum-Based Redesign

A Rust/egui-based application for automating UI interactions with human-like mouse movements. This version has been completely restructured using enum-based state management for cleaner architecture and better maintainability.

## Architecture Overview

### Enum-Based State Machine

Instead of managing many individual state variables, the application uses a single `AppScreen` enum to represent all possible UI states:

```rust
enum AppScreen {
    MainMenu,
    SequenceEditor(SequenceEditorState),
    ClickEditor(ClickEditorState),
    RunningSequence(RunningSequenceState),
}
```

This ensures the UI is always in a valid, well-defined state.

### Data Model

#### Click
A single click at specific coordinates:
```rust
pub struct Click {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub button_type: String, // "Left" or "Right"
}
```

#### SequenceStepType
What can appear as a step in a sequence:
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
```

#### ClickSequence
A named sequence of steps:
```rust
pub struct ClickSequence {
    pub name: String,
    pub steps: Vec<SequenceStepType>,
}
```

## Usage

### Creating Clicks

1. From Main Menu, click "Manage Clicks"
2. Enter a name for the click
3. Click "Pick Location from Screen" to select a point visually
4. The screen will become fullscreen overlay - click the target location
5. Click "Save Click"

### Creating Sequences

1. From Main Menu, click "New Sequence" or "Edit Sequence"
2. Enter a sequence name
3. Add steps using the dropdowns:
   - Select an existing click to add it as a step
   - Configure min/max interval times (in seconds)
   - Or select another sequence to include it as a subsequence
4. Remove steps with the × button
5. Click "Save & Run" to prepare for execution

### Running Sequences

1. After creating/editing a sequence, you'll be on the Running screen
2. Adjust repetition count (1-1000)
3. Click "Start" to begin execution
4. Click "Stop" to halt
5. Click "← Back" to return to main menu

### Data Persistence

All clicks and sequences are automatically saved to:
- **Windows**: `%APPDATA%/AreaClicker/AreaClicker/ui_presets.toml`

Changes are persisted immediately when you save/create items.

## Technical Details

### Key Components

#### main.rs
- `AppState`: Holds all application state
- `AppScreen` enum: Represents current UI screen
- Screen rendering functions (`render_*`)
- `execute_sequence_steps`: Runs sequence execution in background thread

#### presets.rs
- Data models (Click, ClickSequence, SequenceStepType)
- `PresetStore`: Manages loading/saving data to TOML
- Backward compatibility with legacy data formats

#### human_mouse.rs
- `human_move_and_click`: Performs mouse movement with human-like characteristics
  - Cubic Bézier curves for smooth paths
  - Micro-jitter for realistic movement
  - Optional overshoot before settling
  - Speed variation

### Threading Model

- Main UI thread: egui rendering and user input
- Worker thread: Click execution via `ClickJob`
- Interruptible sleep: Pause/stop is responsive even during waits

### Borrow Safety

The refactored code carefully manages borrows to avoid conflicts:
- Data collection happens before UI closures
- Mutable operations separated from immutable reads
- No held references across UI callbacks

## Build & Run

```bash
cargo build --release
cargo run --release
```

The application targets Windows (uses Windows-specific mouse control via enigo).

## Features

✅ Create and save individual clicks with visual location picker  
✅ Create sequences of clicks with configurable intervals  
✅ Include other sequences as subsequences  
✅ Execute sequences with configurable repetitions  
✅ Pause and stop execution  
✅ Human-like mouse movement with Bézier curves  
✅ Data persistence to TOML  
✅ Main menu for navigation  
✅ Fullscreen overlay for precise location picking

## Future Enhancements

- [ ] Auto-minimize window during execution
- [ ] Global hotkeys for quick access
- [ ] Detailed execution logging and visualization
- [ ] Import/export sequences as JSON
- [ ] Undo/redo in editors
- [ ] Mouse speed and movement style settings
- [ ] Sequence validation before running
- [ ] Step-by-step debugger for sequences

## Performance

- Startup: < 1 second
- Click creation: Instant
- Sequence execution: Scales with click count and intervals
- Data save: < 100ms

## Compatibility

- **OS**: Windows (uses enigo for mouse control)
- **Rust**: 1.70+
- **egui**: 0.27
- **enigo**: 0.1

## License

See LICENSE file for terms.

## Architecture Patterns

### Enum-Based State Management
Instead of:
```rust
picking_area: bool,
picking_point: bool,
editing_sequence: bool,
// ... many flags ...
```

We have:
```rust
enum AppScreen {
    SequenceEditor(SequenceEditorState),
    ClickEditor(ClickEditorState),
    // ... only one at a time
}
```

This eliminates impossible state combinations and makes the code more maintainable.

### Type-Safe Data Models
Using enums for step types:
```rust
enum SequenceStepType {
    Click { ... },
    Subsequence { ... },
}
```

Prevents bugs from string-based type checking and provides compile-time exhaustiveness checking.

### Clear Separation of Concerns
- `presets.rs`: Data models and persistence
- `human_mouse.rs`: Mouse movement logic
- `main.rs`: UI and application flow

Each module has a single, clear responsibility.
