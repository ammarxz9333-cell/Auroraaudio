//! Versioned filesystem presets for manual acoustic Auto-Aim.
//!
//! The caller owns an explicit trusted file path. This implementation writes
//! to a same-directory temporary file, syncs, and then renames it. The old
//! preset is never overwritten in place. No configuration I/O runs in audio
//! callbacks; no autosave or background microphone session is scheduled.
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use super::auto_aim::AimSelection;
use super::auto_aim_host::AutoAimPresetStore;

const MAX_PRESET_BYTES: u64 = 16_384;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredAngle {
    pub module_id: String,
    pub angle_deg: i16,
    pub relative_score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutoAimSavedPreset {
    pub schema_version: u32,
    pub source: String,
    pub angles: Vec<StoredAngle>,
}

impl AutoAimSavedPreset {
    pub fn from_selections(choices: &[AimSelection]) -> Result<Self, String> {
        let preset = Self {
            schema_version: 1,
            source: "on-demand-acoustic-aim".into(),
            angles: choices.iter().map(|c| StoredAngle {
                module_id: c.module_id.clone(),
                angle_deg: c.angle_deg,
                relative_score: c.score,
            }).collect(),
        };
        preset.validate()?;
        Ok(preset)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.source != "on-demand-acoustic-aim"
            || self.angles.is_empty()
            || self.angles.len() > 12
        {
            return Err("incompatible or empty Auto-Aim preset".into());
        }
        let mut seen = HashSet::new();
        for a in &self.angles {
            if a.module_id.trim().is_empty()
                || a.module_id.len() > 128
                || a.angle_deg.abs() > 90
                || !a.relative_score.is_finite()
                || !seen.insert(a.module_id.as_str())
            {
                return Err("unsafe or duplicate Auto-Aim preset angle".into());
            }
        }
        Ok(())
    }

    /// Saved angles are stale on unknown hardware; motor adapter must still
    /// verify physical limits and encoder state before applying them.
    pub fn as_selection(&self) -> Vec<AimSelection> {
        self.angles.iter().map(|a| AimSelection {
            module_id: a.module_id.clone(),
            angle_deg: a.angle_deg,
            score: a.relative_score,
        }).collect()
    }
}

#[derive(Debug, Clone)]
pub struct AtomicPresetFile {
    path: PathBuf,
}

impl AtomicPresetFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Option<AutoAimSavedPreset>, String> {
        let file = match fs::File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        if file.metadata().map_err(|e| e.to_string())?.len() > MAX_PRESET_BYTES {
            return Err("preset file exceeds safety size limit".into());
        }
        let mut data = String::new();
        file.take(MAX_PRESET_BYTES + 1)
            .read_to_string(&mut data).map_err(|e| e.to_string())?;
        if data.len() as u64 > MAX_PRESET_BYTES {
            return Err("preset exceeds safety size limit".into());
        }
        let decoded: AutoAimSavedPreset =
            serde_json::from_str(&data).map_err(|e| e.to_string())?;
        decoded.validate()?;
        Ok(Some(decoded))
    }
}

impl AutoAimPresetStore for AtomicPresetFile {
    fn save_atomically(&mut self, choices: &[AimSelection]) -> Result<(), String> {
        let preset = AutoAimSavedPreset::from_selections(choices)?;
        let serialized = serde_json::to_vec_pretty(&preset).map_err(|e| e.to_string())?;
        if serialized.len() as u64 > MAX_PRESET_BYTES {
            return Err("preset is too large".into());
        }
        let parent = self.path.parent().ok_or("preset has no parent")?;
        if !parent.exists() {
            return Err("preset parent directory does not exist".into());
        }
        let file_name = self.path.file_name().ok_or("preset lacks filename")?;
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{}.{}.{}.tmp",
            file_name.to_string_lossy(),
            std::process::id(),
            suffix,
        ));
        let result = (|| -> Result<(), String> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
                .map_err(|e| e.to_string())?;
            file.write_all(&serialized).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            drop(file);
            fs::rename(&candidate, &self.path).map_err(|e| e.to_string())?;
            // Do not report a failure AFTER successful rename: at this point
            // the new file is already visible and cannot be safely rolled back.
            #[cfg(unix)]
            {
                if let Ok(dir) = fs::File::open(parent) {
                    let _ = dir.sync_all();
                }
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(candidate);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_file() -> PathBuf {
        let ns = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("aurora-aim-preset-{}-{}.json", std::process::id(), ns))
    }

    fn selected(angle: i16) -> Vec<AimSelection> {
        vec![AimSelection {
            module_id: "rear-left".into(),
            angle_deg: angle,
            score: 42.0,
        }]
    }

    #[test]
    fn loads_new_preset_and_does_not_overwrite_old_on_rejected_save() {
        let path = test_file();
        let mut store = AtomicPresetFile::new(&path);
        assert_eq!(store.load().unwrap(), None);
        store.save_atomically(&selected(15)).unwrap();
        assert_eq!(store.load().unwrap().unwrap().angles[0].angle_deg, 15);
        assert!(store.save_atomically(&selected(180)).is_err());
        assert_eq!(store.load().unwrap().unwrap().angles[0].angle_deg, 15);
        store.save_atomically(&selected(20)).unwrap();
        assert_eq!(store.load().unwrap().unwrap().angles[0].angle_deg, 20);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn rejects_unsupported_version_and_duplicate_or_nan_angles() {
        let mut preset = AutoAimSavedPreset::from_selections(&selected(0)).unwrap();
        preset.schema_version = 2;
        assert!(preset.validate().is_err());
        preset.schema_version = 1;
        preset.angles.push(preset.angles[0].clone());
        assert!(preset.validate().is_err());
        preset.angles.pop();
        preset.angles[0].relative_score = f64::NAN;
        assert!(preset.validate().is_err());
    }
}
