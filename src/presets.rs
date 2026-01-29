use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

// ============================================================================
// CORE MODELS - Enum-based design
// ============================================================================

/// A single click with bounding box coordinates and metadata
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Click {
    pub name: String,
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
    /// "Left" or "Right"
    #[serde(default)]
    pub button_type: String,
    /// Minimum interval in seconds after click
    #[serde(default = "default_min_interval")]
    pub min_interval: f32,
    /// Maximum interval in seconds after click
    #[serde(default = "default_max_interval")]
    pub max_interval: f32,
}

fn default_min_interval() -> f32 {
    0.5
}

fn default_max_interval() -> f32 {
    1.0
}

impl Click {
    pub fn new(name: String, min_x: i32, max_x: i32, min_y: i32, max_y: i32) -> Self {
        Self {
            name,
            min_x,
            max_x,
            min_y,
            max_y,
            button_type: "Left".to_string(),
            min_interval: 0.5,
            max_interval: 1.0,
        }
    }
}

/// A step in a sequence - can be a single click or reference to another sequence
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SequenceStepType {
    /// Click a specific saved click with a delay before it
    Click {
        click_name: String,
        min_interval: f32,
        max_interval: f32,
    },
    /// Include all steps from another saved sequence
    Subsequence {
        sequence_name: String,
    },
}

/// A sequence of steps (clicks and/or subsequences)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClickSequence {
    pub name: String,
    pub steps: Vec<SequenceStepType>,
}

// ============================================================================
// LEGACY MODELS - Kept for backward compatibility
// ============================================================================

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PresetStore {
    /// All saved clicks
    #[serde(default)]
    pub clicks: Vec<Click>,
    
    /// All saved sequences
    #[serde(default)]
    pub sequences: Vec<ClickSequence>,

    // Legacy fields for backward compatibility
    pub presets: Vec<UiPreset>,
    #[serde(default)]
    pub legacy_sequences: Vec<LegacyClickSequence>,
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

/// Legacy sequence type (for backward compatibility)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LegacyClickSequence {
    pub name: String,
    #[serde(default)]
    pub preset_name: Option<String>,
    pub steps: Vec<SequenceStep>,
}

/// A single step in a legacy click sequence
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SequenceStep {
    pub area_name: String,
    pub min_interval: f32,
    pub max_interval: f32,
    #[serde(default)]
    pub interval_secs: f32,
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
    #[allow(dead_code)]
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

    // ========== CLICK MANAGEMENT ==========
    
    pub fn upsert_click(&mut self, click: Click) {
        if let Some(existing) = self.clicks.iter_mut().find(|c| c.name == click.name) {
            *existing = click;
        } else {
            self.clicks.push(click);
        }
        self.clicks.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    }

    pub fn remove_click(&mut self, name: &str) {
        self.clicks.retain(|c| c.name != name);
    }

    pub fn get_click(&self, name: &str) -> Option<&Click> {
        self.clicks.iter().find(|c| c.name == name)
    }

    #[allow(dead_code)]
    pub fn get_click_mut(&mut self, name: &str) -> Option<&mut Click> {
        self.clicks.iter_mut().find(|c| c.name == name)
    }

    // ========== SEQUENCE MANAGEMENT ==========

    pub fn upsert_sequence(&mut self, sequence: ClickSequence) {
        if let Some(existing) = self.sequences.iter_mut().find(|s| s.name == sequence.name) {
            *existing = sequence;
        } else {
            self.sequences.push(sequence);
        }
        self.sequences.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    }

    pub fn remove_sequence(&mut self, name: &str) {
        self.sequences.retain(|s| s.name != name);
    }

    pub fn get_sequence(&self, name: &str) -> Option<&ClickSequence> {
        self.sequences.iter().find(|s| s.name == name)
    }

    pub fn get_sequence_mut(&mut self, name: &str) -> Option<&mut ClickSequence> {
        self.sequences.iter_mut().find(|s| s.name == name)
    }

    // ========== LEGACY METHODS (for backward compatibility) ==========

    #[allow(dead_code)]
    pub fn upsert_preset(&mut self, preset: UiPreset) {
        if let Some(existing) = self.presets.iter_mut().find(|p| p.name == preset.name) {
            *existing = preset;
        } else {
            self.presets.push(preset);
        }
        self.presets.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    }

    #[allow(dead_code)]
    pub fn remove_by_name(&mut self, name: &str) {
        self.presets.retain(|p| p.name != name);
    }

    #[allow(dead_code)]
    pub fn remove_sequence_by_name(&mut self, name: &str) {
        self.legacy_sequences.retain(|s| s.name != name);
    }

    #[allow(dead_code)]
    pub fn get_sequence_legacy(&self, name: &str) -> Option<&LegacyClickSequence> {
        self.legacy_sequences.iter().find(|s| s.name == name)
    }

    #[allow(dead_code)]
    pub fn get_sequence_mut_legacy(&mut self, name: &str) -> Option<&mut LegacyClickSequence> {
        self.legacy_sequences.iter_mut().find(|s| s.name == name)
    }
}
