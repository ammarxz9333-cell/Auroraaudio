//! Sonos-Style Multi-Room Audio Synchronization Coordinator.
//!
//! Manages audio zones across multiple rooms (Living Room 11.1.4, Bedroom, Kitchen, Patio),
//! handles dynamic grouping/ungrouping, independent per-room volume, and global party mode.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Individual audio zone / room.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoomZone {
    pub id: String,
    pub name: String,
    pub ip_address: String,
    pub volume_percent: u8,
    pub is_muted: bool,
    pub is_grouped: bool,
    pub group_id: Option<String>,
    pub is_cinema_master: bool,
    pub active_layout: String,
}

/// Multi-Room Zone Coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiRoomCoordinator {
    pub zones: HashMap<String, RoomZone>,
    pub master_volume: u8,
    pub party_mode_active: bool,
}

impl Default for MultiRoomCoordinator {
    fn default() -> Self {
        Self::new_with_default_home_zones()
    }
}

impl MultiRoomCoordinator {
    /// Initializes coordinator with standard high-end smart home zones.
    pub fn new_with_default_home_zones() -> Self {
        let mut zones = HashMap::new();

        zones.insert(
            "zone-living-room".into(),
            RoomZone {
                id: "zone-living-room".into(),
                name: "Living Room (Cinema 11.1.4)".into(),
                ip_address: "127.0.0.1".into(),
                volume_percent: 75,
                is_muted: false,
                is_grouped: true,
                group_id: Some("group-home".into()),
                is_cinema_master: true,
                active_layout: "11.1.4 Spatial".into(),
            },
        );

        zones.insert(
            "zone-bedroom".into(),
            RoomZone {
                id: "zone-bedroom".into(),
                name: "Master Bedroom".into(),
                ip_address: "192.168.1.110".into(),
                volume_percent: 50,
                is_muted: false,
                is_grouped: true,
                group_id: Some("group-home".into()),
                is_cinema_master: false,
                active_layout: "Stereo Satellite".into(),
            },
        );

        zones.insert(
            "zone-kitchen".into(),
            RoomZone {
                id: "zone-kitchen".into(),
                name: "Kitchen & Dining".into(),
                ip_address: "192.168.1.120".into(),
                volume_percent: 40,
                is_muted: false,
                is_grouped: false,
                group_id: None,
                is_cinema_master: false,
                active_layout: "Stereo Ceiling".into(),
            },
        );

        zones.insert(
            "zone-patio".into(),
            RoomZone {
                id: "zone-patio".into(),
                name: "Patio / Garden".into(),
                ip_address: "192.168.1.130".into(),
                volume_percent: 60,
                is_muted: false,
                is_grouped: false,
                group_id: None,
                is_cinema_master: false,
                active_layout: "All-Weather Stereo".into(),
            },
        );

        Self {
            zones,
            master_volume: 65,
            party_mode_active: false,
        }
    }

    /// Toggles grouping for a specific room zone.
    pub fn toggle_group(&mut self, zone_id: &str) -> bool {
        if let Some(zone) = self.zones.get_mut(zone_id) {
            zone.is_grouped = !zone.is_grouped;
            zone.group_id = if zone.is_grouped {
                Some("group-home".into())
            } else {
                None
            };
            zone.is_grouped
        } else {
            false
        }
    }

    /// Sets the volume percentage (0..=100) for a specific zone.
    pub fn set_zone_volume(&mut self, zone_id: &str, volume: u8) {
        if let Some(zone) = self.zones.get_mut(zone_id) {
            zone.volume_percent = volume.min(100);
        }
    }

    /// Sets the master volume for all grouped zones simultaneously.
    #[allow(dead_code)]
    pub fn set_master_volume(&mut self, volume: u8) {
        let vol = volume.min(100);
        self.master_volume = vol;
        for zone in self.zones.values_mut() {
            if zone.is_grouped {
                zone.volume_percent = vol;
            }
        }
    }

    /// Toggles mute for a specific zone.
    #[allow(dead_code)]
    pub fn toggle_mute(&mut self, zone_id: &str) -> bool {
        if let Some(zone) = self.zones.get_mut(zone_id) {
            zone.is_muted = !zone.is_muted;
            zone.is_muted
        } else {
            false
        }
    }

    /// Activates global Party Mode (links all rooms in the house synchronously).
    #[allow(dead_code)]
    pub fn set_party_mode(&mut self, enabled: bool) {
        self.party_mode_active = enabled;
        for zone in self.zones.values_mut() {
            zone.is_grouped = enabled;
            zone.group_id = if enabled { Some("group-home".into()) } else { None };
        }
    }

    /// Returns a list of all active room zones sorted by master first.
    pub fn list_zones(&self) -> Vec<RoomZone> {
        let mut list: Vec<RoomZone> = self.zones.values().cloned().collect();
        list.sort_by(|a, b| b.is_cinema_master.cmp(&a.is_cinema_master));
        list
    }
}
