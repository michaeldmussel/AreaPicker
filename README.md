# Area Clicker

Area Clicker is a Windows desktop automation tool built with Rust and `egui`.  
It lets you define click areas, compose them into sequences, and run those sequences with human-like cursor movement.

## Highlights

- Persistent click and sequence library (TOML-backed)
- Sequence composition with subsequences
- Compiled run-plan validation before execution
- Pause/resume support during runs (`F3`)
- Stop controls and interruptible timing/movement

## Build

Requirements:

- Windows
- Rust 1.70+

```bash
cargo build --release
cargo run --release
```

Binary:

- `target/release/area_clicker.exe`

## Storage

Runtime data is saved at:

- `%APPDATA%/AreaClicker/AreaClicker/ui_presets.toml`

## Repository Layout

- `src/main.rs`: current app/UI/runtime integration
- `src/presets.rs`: persistence models and storage
- `src/human_mouse.rs`: movement and click behavior
- `src/domain/validation.rs`: run-plan compilation and validation
