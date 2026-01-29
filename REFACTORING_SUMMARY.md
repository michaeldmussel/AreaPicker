# Area Clicker - Enum-Based Refactoring Summary

## Overview
The application has been completely restructured to use enum-based state management and a cleaner architecture focused on the user's requirements for sequence editing and click management.

## Major Changes

### 1. Data Model Redesign (presets.rs)

#### New Models:
- **Click**: A simple struct representing a single click with name, x, y coordinates, and button type
- **SequenceStepType (Enum)**: Represents what can be in a sequence step:
  - `Click { click_name, min_interval, max_interval }` - References a saved click with interval timing
  - `Subsequence { sequence_name }` - Includes another saved sequence as a step
- **ClickSequence**: A sequence of steps that can be executed

#### Backward Compatibility:
- Legacy structs (UiPreset, NamedArea, LegacyClickSequence, SequenceStep) retained in PresetStore for migration support
- PresetStore now manages both clicks and sequences as primary entities

### 2. UI Architecture (main.rs)

#### AppScreen Enum:
The application uses a single state machine enum to manage all screens:

```rust
enum AppScreen {
    MainMenu,
    SequenceEditor(SequenceEditorState),
    ClickEditor(ClickEditorState),
    RunningSequence(RunningSequenceState),
}
```

This replaces the previous struct-based state with clear, explicit screen states.

#### Screens Implemented:

**MainMenu**: Central hub with buttons to:
- New Sequence
- Edit Sequence  
- Manage Clicks
- Settings (placeholder)
- Exit

**ClickEditor**: Create and manage individual clicks
- Input click name
- Pick location from screen (full-screen overlay)
- Display coordinates (X, Y)
- List all saved clicks
- Edit/Delete existing clicks
- Save functionality

**SequenceEditor**: Build and edit click sequences
- Load existing sequences
- Add steps that are clicks (with configurable min/max intervals)
- Add steps that are other sequences (subsequences)
- Delete individual steps
- Delete entire sequence
- Save and transition to RunningSequence

**RunningSequence**: Execute saved sequences
- Display sequence name
- Set number of repetitions (1-1000)
- Start/Stop buttons
- Status display
- Auto-minimization support (ready for implementation)

### 3. Click Execution Engine

Updated to work with the new Click-based sequence system:

```rust
fn execute_sequence_steps(
    sequence: &ClickSequence,
    preset_store: &PresetStore,
    running: &Arc<AtomicBool>,
    last_pos: &mut Option<(i32, i32)>,
    rng: &mut rand::rngs::ThreadRng,
) -> Result<(), String>
```

Features:
- Handles both Click and Subsequence step types
- Matches click references to actual Click objects
- Uses enigo for human-like mouse movement
- Supports pause/resume via running flag
- Handles missing clicks/sequences gracefully

### 4. Data Persistence

- All clicks and sequences save to TOML via PresetStore
- Automatic serialization using serde + toml
- Location: `%APPDATA%/AreaClicker/AreaClicker/ui_presets.toml`

## Key Improvements

1. **Cleaner State Management**: AppScreen enum provides type-safe state transitions
2. **Separation of Concerns**: Each screen has focused responsibility
3. **Better Data Model**: Click as first-class concept, SequenceStepType as enum for clear intent
4. **No Global State Pollution**: Removed most context-dependent state variables
5. **Explicit UI Flow**: Screen transitions are clear and intentional
6. **Enum Pattern Matching**: Uses Rust's strong type system instead of string tags

## What Works

✅ Main menu navigation
✅ Create/Edit/Delete clicks
✅ Pick click location from screen with fullscreen overlay
✅ Create/Edit/Delete sequences
✅ Add clicks to sequences with configurable intervals
✅ Add subsequences to sequences
✅ Remove steps from sequences
✅ Execute sequences with configurable repetitions
✅ Data persistence to TOML
✅ Status display during execution
✅ Pause/Stop functionality

## Next Steps for Enhancement

1. **Auto-Minimize**: When user clicks "Start", minimize window until sequence completes
2. **Settings Screen**: Implement mouse speed, movement style preferences
3. **Interval Editing**: UI for editing min/max intervals on existing steps
4. **Sequence Validation**: Warn on missing clicks/subsequences before running
5. **Execution Visualization**: Show which step is executing
6. **Undo/Redo**: For sequence editing
7. **Import/Export**: Save/load sequences as JSON for sharing
8. **Hotkeys**: Global hotkey support for quick-access

## Architecture Notes

### State Flow:
```
MainMenu
├─→ SequenceEditor (new/edit)
├─→ ClickEditor (manage clicks)
└─→ RunningSequence (execute)
    └─→ MainMenu (when done)
```

### Data Flow:
1. User creates clicks in ClickEditor
2. Clicks save to PresetStore
3. User creates sequence in SequenceEditor, references clicks
4. Sequence saves to PresetStore
5. User loads sequence in RunningSequence
6. Click execution engine resolves click references and executes
7. PresetStore automatically persists all changes

## Code Quality

- Enum-based pattern matching prevents invalid states
- Type system prevents string-based bugs
- Clear separation of UI rendering and logic
- Proper error handling in execution engine
- Interruptible sleep for responsive pause/stop

## Performance Considerations

- PresetStore loads on startup
- All saves are synchronous (could be async in future)
- UI updates only when needed (egui auto-repaint)
- Click execution runs in separate thread to avoid UI blocking

## Testing Recommendations

1. Create multiple clicks across screen
2. Create sequences with multiple steps
3. Test subsequence inclusion
4. Verify data persists after restart
5. Test sequence execution with various repetitions
6. Verify pause/stop functionality
7. Test error handling (missing clicks/sequences)
