//! Config system — serde_json replacement for C++ nlohmann/json + FNV1A config.
//!
//! Much simpler than the C++ version: fields are named directly so no hashing
//! or type-id encoding is needed.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

// ─── Bone targets ────────────────────────────────────────────────────────────

/// Named bone indices for CS2 player models.
/// Indices are approximate and model-dependent but stable across most skins.
pub const BONE_TARGETS: &[(&str, u32)] = &[
    ("Head",    6),
    ("Neck",    5),
    ("Chest",   4),
    ("Stomach", 2),
    ("Pelvis",  0),
];

// ─── Sub-configs ─────────────────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct AimbotConfig {
    pub enabled:     bool,
    pub fov:         f32,
    pub smooth:      f32,
    pub target_bone: u32,
    /// Windows virtual-key code that must be held to activate aimbot.
    /// 0 = always-on, 1 = LMB, 2 = RMB, 4 = MMB, 5 = Mouse4, 6 = Mouse5.
    #[serde(default = "default_aim_key")]
    pub aim_key:     u32,
    #[serde(default)]
    pub rcs_enabled: bool,
    /// 0.0 = no compensation, 1.0 = full compensation, 2.0 = over-compensate.
    #[serde(default = "default_rcs_scale")]
    pub rcs_scale:   f32,
}

fn default_aim_key()   -> u32 { 0xA0 }  // VK_LSHIFT
fn default_rcs_scale() -> f32 { 1.0 }
pub fn rcs_scale_max()  -> f32 { 1.0 }

impl Default for AimbotConfig {
    fn default() -> Self {
        Self {
            enabled:     false,
            fov:         5.0,
            smooth:      3.0,
            target_bone: 6,
            aim_key:     default_aim_key(),
            rcs_enabled: false,
            rcs_scale:   default_rcs_scale(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TriggerConfig {
    pub enabled: bool,
    pub delay_ms: u32,
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self { enabled: false, delay_ms: 50 }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct VisualsConfig {
    pub enabled: bool,
    pub boxes: bool,
    pub health_bar: bool,
    pub names: bool,
    pub team_check: bool,    // skip teammates
    pub box_color: [u8; 4],  // RGBA enemy
    pub name_color: [u8; 4],
    pub overlay_mode:   bool,
    pub fov_circle:     bool,
}

impl Default for VisualsConfig {
    fn default() -> Self {
        Self {
            enabled:     true,
            boxes:       true,
            health_bar:  true,
            names:       true,
            team_check:  true,
            box_color:   [220, 30, 30, 255],
            name_color:  [255, 255, 255, 255],
            overlay_mode:   false,
            fov_circle:     false,
        }
    }
}

// ─── Top-level config ─────────────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub aimbot:   AimbotConfig,
    pub trigger:  TriggerConfig,
    pub visuals:  VisualsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            aimbot:   AimbotConfig::default(),
            trigger:  TriggerConfig::default(),
            visuals:  VisualsConfig::default(),
        }
    }
}

// ─── File management ─────────────────────────────────────────────────────────

pub const CONFIG_DIR: &str = "configs";

fn config_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(CONFIG_DIR);
    p.push(name);
    if p.extension().is_none() {
        p.set_extension("json");
    }
    p
}

pub fn ensure_dir() {
    if !Path::new(CONFIG_DIR).exists() {
        let _ = fs::create_dir_all(CONFIG_DIR);
    }
}

pub fn save(config: &Config, name: &str) -> bool {
    ensure_dir();
    let path = config_path(name);
    match serde_json::to_string_pretty(config) {
        Ok(json) => fs::write(&path, json).is_ok(),
        Err(_)   => false,
    }
}

pub fn load(name: &str) -> Option<Config> {
    let path = config_path(name);
    let text = fs::read_to_string(path).ok()?;
    let mut cfg: Config = serde_json::from_str(&text).ok()?;
    cfg.aimbot.rcs_scale = cfg.aimbot.rcs_scale.clamp(0.0, rcs_scale_max());
    Some(cfg)
}

pub fn remove(name: &str) {
    let _ = fs::remove_file(config_path(name));
}

pub fn list_files() -> Vec<String> {
    ensure_dir();
    let Ok(rd) = fs::read_dir(CONFIG_DIR) else { return Vec::new(); };
    rd.flatten()
        .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}
