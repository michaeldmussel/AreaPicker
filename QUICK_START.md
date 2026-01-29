# Quick Start Guide - Area Clicker (Enum-Based)

## Installation

### Prerequisites
- Rust 1.70+ (install from https://rustup.rs/)
- Windows OS (Linux/macOS support possible with code changes)
- ~500MB disk space for dependencies

### Build
```bash
# Clone/navigate to project
cd path/to/AreaPicker

# Build release version
cargo build --release

# Run
cargo run --release
# Or use the binary at: target/release/area_clicker.exe
```

**Build time**: ~10-30 seconds (first time ~3-5 minutes)

---

## User Guide

### 1. Create Your First Click

**Step 1: Start the App**
```
[Application launches to Main Menu]
```

**Step 2: Open Click Manager**
```
Click: [Manage Clicks]
```

**Step 3: Create a Click**
```
Input: Click Name: "MyButton"
Click: [Pick Location from Screen]
```

**Step 4: Select Location**
```
[Screen becomes fullscreen overlay]
Click on your target location
[Screen returns, coordinates show: X: 1920, Y: 1080]
```

**Step 5: Save**
```
Click: [Save Click]
```

**Result**: Click saved! You'll see it in the "Existing Clicks" list.

---

### 2. Create a Sequence

**Step 1: New Sequence**
```
Click: [Main Menu]
Click: [New Sequence]
```

**Step 2: Name Sequence**
```
Input: Sequence Name: "MySequence"
```

**Step 3: Create Sequence**
```
Click: [Create New Sequence]
```

**Step 4: Add First Click**
```
Dropdown: "Add Step - Click:" → Select "MyButton"
Button appears: [MyButton] added with default interval (0.5-1.0s)
```

**Step 5: Add More Steps**
```
Dropdown: "Add Step - Click:" → Select another click
Or dropdown: "Add Step - Subsequence:" → Select another sequence
```

**Step 6: Review & Run**
```
See all steps listed with × buttons to remove individual steps
Click: [Save & Run]
```

**Result**: Transitioned to RunningSequence screen.

---

### 3. Execute a Sequence

**On RunningSequence Screen**
```
┌──────────────────────────────┐
│  MySequence                  │
│  Repetitions: [1]            │
│  [Start] [Stop]              │
│  Status: Stopped             │
└──────────────────────────────┘
```

**Step 1: Set Repetitions**
```
Drag or click: [1] → [5]
(Can set 1-1000 repetitions)
```

**Step 2: Start**
```
Click: [Start]
Status shows: "Running..."
```

**Step 3: Monitor Execution**
```
Clicks execute in sequence
Mouse moves with human-like motion
Intervals between clicks respected
Status updates in real-time
```

**Step 4: Stop (Optional)**
```
Click: [Stop] at any time
Sequence halts immediately
```

**Step 5: Return to Menu**
```
Click: [← Back]
Returns to Main Menu
```

---

## Data Storage

### Where Clicks and Sequences Save
- **Windows**: `%APPDATA%/AreaClicker/AreaClicker/ui_presets.toml`
- **File Format**: TOML (human-readable text)
- **Auto-save**: Every time you save or create an item

### Example TOML File
```toml
[[clicks]]
name = "Login_Button"
x = 1920
y = 1080
button_type = "Left"

[[clicks]]
name = "Submit_Form"
x = 500
y = 600
button_type = "Left"

[[sequences]]
name = "Login_Process"

[[sequences.steps]]
Click = { click_name = "Login_Button", min_interval = 1.0, max_interval = 1.5 }

[[sequences.steps]]
Click = { click_name = "Submit_Form", min_interval = 2.0, max_interval = 3.0 }
```

---

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Esc` | (Not yet implemented) Close current dialog |
| `Ctrl+S` | (Not yet implemented) Quick save |

---

## Common Workflows

### Workflow 1: Automate Form Filling

1. Create clicks for each form field
   - Username field
   - Password field
   - Submit button

2. Create sequence "FillForm"
   - Step 1: Click username field (1-2s interval)
   - Step 2: Click password field (1-2s interval)
   - Step 3: Click submit button (2-3s interval)

3. Run with 1 repetition

4. **Result**: Form filled automatically

### Workflow 2: Repeated UI Interactions

1. Create clicks for each button
   - Button A
   - Button B
   - Button C

2. Create sequence "Cycle"
   - Step 1: Click Button A (0.5-1.0s)
   - Step 2: Click Button B (0.5-1.0s)
   - Step 3: Click Button C (2.0-3.0s)

3. Run with 10 repetitions

4. **Result**: Sequence repeats 10 times automatically

### Workflow 3: Multi-Step Process

1. Create individual clicks for each step

2. Create "MainFlow" sequence with all steps

3. Create "AlternateFlow" sequence
   - Step 1: Subsequence "MainFlow"
   - Step 2: Click special button
   - Step 3: Subsequence "MainFlow" (again)

4. Run AlternateFlow

5. **Result**: Main flow runs, special button clicked, main flow repeats

---

## Troubleshooting

### App Won't Start
```
Error: "Could not resolve config directory"
Solution: Ensure write permissions in %APPDATA%
```

### Clicks Don't Save
```
Error: Clicks appear but vanish on restart
Solution: Check %APPDATA%/AreaClicker/AreaClicker/ exists with ui_presets.toml
```

### Mouse Doesn't Click Correctly
```
Issue: Clicks register in wrong location
Solution: Verify coordinates in Click Editor match target location
```

### Sequence Doesn't Execute
```
Issue: "Sequence not found" error
Solution: Ensure sequence is created and saved before running
```

### Missing Click in Sequence
```
Issue: "Click not found" when executing
Solution: Create the missing click before running
         Click name must match exactly (case-sensitive)
```

---

## Performance Tips

1. **Interval Timing**: 
   - Shorter intervals (0.5-1.0s): Faster execution
   - Longer intervals (3-5s): Slower execution, more stable

2. **Multiple Repetitions**:
   - 1-10: Very fast
   - 10-100: Fast
   - 100+: Slow but reliable

3. **Subsequences**:
   - Nesting depth of 3+ can be slow
   - Keep subsequences shallow for better performance

---

## Advanced Features

### Nested Sequences
Create sequences that include other sequences:
```
MainSequence:
├─ Step 1: Click "Button A"
├─ Step 2: Subsequence "SubFlow"
│           ├─ Click "Button B"
│           └─ Click "Button C"
└─ Step 3: Click "Button D"
```

### Variable Intervals
Each step has configurable min/max intervals:
```
Step 1: 0.5-1.0s (quick)
Step 2: 2.0-3.0s (wait for loading)
Step 3: 3.0-5.0s (wait for response)
```

---

## Screen Reference

### Main Menu
```
┌─────────────────────────────────┐
│  Area Clicker — Sequence Manager│
│                                 │
│          Main Menu              │
│                                 │
│       [New Sequence]            │
│       [Edit Sequence]           │
│       [Manage Clicks]           │
│       [Settings]                │
│       [Exit]                    │
└─────────────────────────────────┘
```

### Click Editor
```
┌──────────────────────────────┐
│  Click Editor                │
│  [← Back to Menu]            │
├──────────────────────────────┤
│  Click Name: [____________]  │
│  X: 1920   Y: 1080           │
│  [Pick Location from Screen] │
│  [Save Click]                │
│                              │
│  Existing Clicks:            │
│  • MyButton (100, 200)  [E][D]│
└──────────────────────────────┘
```

### Sequence Editor
```
┌──────────────────────────────┐
│  Sequence Editor             │
│  [← Back to Menu]            │
├──────────────────────────────┤
│  Sequence: [____________]    │
│                              │
│  Steps:                      │
│  1. MyButton (0.5-1.0s) [×]  │
│  2. OtherSeq               [×]
│                              │
│  Add: [MyButton ▼]           │
│  Add: [MySeq ▼]              │
│                              │
│  [Save & Run] [Delete]       │
└──────────────────────────────┘
```

### Running Sequence
```
┌──────────────────────────────┐
│  Running Sequence            │
│  [← Back]                    │
├──────────────────────────────┤
│                              │
│       MySequence             │
│                              │
│    Repetitions: [5]          │
│       [Start]                │
│      [Stop]                  │
│                              │
│    Status: Running...        │
│                              │
└──────────────────────────────┘
```

---

## What's Next?

After mastering basic sequences:

1. **Complex Workflows**: Combine multiple subsequences
2. **Timing Optimization**: Adjust intervals for your use case
3. **Documentation**: Document your sequences for team sharing
4. **Automation**: Use OS task scheduler to run at specific times

---

## Getting Help

### Check These Files
- `COMPLETE_REFACTORING_SUMMARY.md` - Architecture overview
- `ENUM_ARCHITECTURE.md` - Detailed design patterns
- `CODE_EXAMPLES.md` - Code samples and patterns
- `README_NEW.md` - Feature documentation

### Common Questions

**Q: Can I edit saved sequences?**
A: Yes! Select the sequence from the dropdown and modify steps.

**Q: Can I delete a click used by a sequence?**
A: Yes, but the sequence will error when executing. Remove the step first.

**Q: What's the maximum number of steps?**
A: Unlimited, but performance degrades with very large sequences.

**Q: Can I use this on macOS/Linux?**
A: Not currently - the code uses Windows-specific mouse control. Porting requires changing the enigo usage.

---

**Happy Automating!** 🤖

For more information, see the comprehensive documentation files included in the project.
