use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PresetStore {
    pub presets: Vec<UiPreset>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiPreset {
    pub name: String,

    /// Optional saved click area (your existing Bounds rectangle)
    pub bounds: Option<BoundsSerde>,

    /// Named points for future sequences/tests
    #[serde(default)]
    pub points: Vec<NamedPoint>,
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
}
