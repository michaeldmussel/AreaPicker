use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PresetStore {
    pub presets: Vec<UiPreset>,
    /// Click sequences (multiple steps with intervals and areas)
    #[serde(default)]
    pub sequences: Vec<ClickSequence>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiPreset {
    pub name: String,

    /// Multiple named rectangles ("areas") for this UI screen
    #[serde(default)]
    pub areas: Vec<NamedArea>,

    /// Named points (optional now; useful later)
    #[serde(default)]
    pub points: Vec<NamedPoint>,
}

/// A sequence of clicks, each with a time interval and an area to click in
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClickSequence {
    pub name: String,
    /// Reference preset name (optional, for UI context)
    #[serde(default)]
    pub preset_name: Option<String>,
    /// Steps in the sequence
    pub steps: Vec<SequenceStep>,
}

/// A single step in a click sequence
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SequenceStep {
    /// Name of the area to click in (must exist in the referenced preset)
    pub area_name: String,
    /// Interval in seconds before this click (from previous step)
    pub interval_secs: f32,
    /// Button type: "Left" or "Right"
    #[serde(default)]
    pub button_type: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NamedArea {
    pub name: String,
    pub bounds: BoundsSerde,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NamedPoint {
    pub name: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BoundsSerde {
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
}

impl BoundsSerde {
    pub fn from_inputs(bounds_inputs: [i32; 4]) -> Self {
        Self {
            min_x: bounds_inputs[0],
            max_x: bounds_inputs[1],
            min_y: bounds_inputs[2],
            max_y: bounds_inputs[3],
        }
    }
}


fn preset_path() -> io::Result<PathBuf> {
    // You can change these identifiers if you want.
    let proj = ProjectDirs::from("com", "AreaClicker", "AreaClicker")
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Could not resolve config directory"))?;

    let dir = proj.config_dir();
    fs::create_dir_all(dir)?;
    Ok(dir.join("ui_presets.toml"))
}

impl PresetStore {
    pub fn load_or_default() -> Self {
        match Self::load() {
            Ok(s) => s,
            Err(_) => Self::default(),
        }
    }

    pub fn load() -> io::Result<Self> {
        let path = preset_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let txt = fs::read_to_string(path)?;
        let store: Self = toml::from_str(&txt)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("TOML parse error: {e}")))?;
        Ok(store)
    }

    pub fn save(&self) -> io::Result<()> {
        let path = preset_path()?;
        let txt = toml::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("TOML serialize error: {e}")))?;
        fs::write(path, txt)?;
        Ok(())
    }

    pub fn upsert_preset(&mut self, preset: UiPreset) {
        if let Some(existing) = self.presets.iter_mut().find(|p| p.name == preset.name) {
            *existing = preset;
        } else {
            self.presets.push(preset);
        }
        self.presets.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    }

    pub fn remove_by_name(&mut self, name: &str) {
        self.presets.retain(|p| p.name != name);
    }

    pub fn upsert_sequence(&mut self, sequence: ClickSequence) {
        if let Some(existing) = self.sequences.iter_mut().find(|s| s.name == sequence.name) {
            *existing = sequence;
        } else {
            self.sequences.push(sequence);
        }
        self.sequences.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    }

    pub fn remove_sequence_by_name(&mut self, name: &str) {
        self.sequences.retain(|s| s.name != name);
    }

    pub fn get_sequence(&self, name: &str) -> Option<&ClickSequence> {
        self.sequences.iter().find(|s| s.name == name)
    }

    pub fn get_sequence_mut(&mut self, name: &str) -> Option<&mut ClickSequence> {
        self.sequences.iter_mut().find(|s| s.name == name)
    }
}
