//! CICP loudspeaker geometry used by immersive transport metadata.
//!
//! The values mirror the MPEG-H reference decoder's CICP geometry ROM. Aurora
//! stores only the standardized numeric geometry/layout facts needed to resolve
//! speaker indices and layout-member positions; no reference decoder code is
//! executed or embedded here.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CicpSpeakerGeometry {
    pub azimuth_degrees: i16,
    pub elevation_degrees: i16,
    pub is_lfe: bool,
    pub screen_relative: bool,
}

const fn g(azimuth: i16, elevation: i16, is_lfe: bool, screen_relative: bool) -> CicpSpeakerGeometry {
    CicpSpeakerGeometry {
        azimuth_degrees: azimuth,
        elevation_degrees: elevation,
        is_lfe,
        screen_relative,
    }
}

// CICP speaker indices 0..=42 as used by the reference MPEG-H geometry table.
const SPEAKERS: [CicpSpeakerGeometry; 43] = [
    g(30, 0, false, false),    // 0
    g(-30, 0, false, false),   // 1
    g(0, 0, false, false),     // 2
    g(0, 0, true, false),      // 3
    g(110, 0, false, false),   // 4
    g(-110, 0, false, false),  // 5
    g(22, 0, false, false),    // 6
    g(-22, 0, false, false),   // 7
    g(135, 0, false, false),   // 8
    g(-135, 0, false, false),  // 9
    g(180, 0, false, false),   // 10
    g(0, 0, false, false),     // 11 reserved/dummy
    g(0, 0, false, false),     // 12 reserved/dummy
    g(90, 0, false, false),    // 13
    g(-90, 0, false, false),   // 14
    g(60, 0, false, false),    // 15
    g(-60, 0, false, false),   // 16
    g(30, 35, false, false),   // 17
    g(-30, 35, false, false),  // 18
    g(0, 35, false, false),    // 19
    g(135, 35, false, false),  // 20
    g(-135, 35, false, false), // 21
    g(180, 35, false, false),  // 22
    g(90, 35, false, false),   // 23
    g(-90, 35, false, false),  // 24
    g(0, 90, false, false),    // 25
    g(45, -15, true, false),   // 26
    g(45, -15, false, false),  // 27
    g(-45, -15, false, false), // 28
    g(0, -15, false, false),   // 29
    g(110, 35, false, false),  // 30
    g(-110, 35, false, false), // 31
    g(45, 35, false, false),   // 32
    g(-45, 35, false, false),  // 33
    g(45, 0, false, false),    // 34
    g(-45, 0, false, false),   // 35
    g(-45, -15, true, false),  // 36
    g(60, 0, false, true),     // 37
    g(-60, 0, false, true),    // 38
    g(30, 0, false, true),     // 39
    g(-30, 0, false, true),    // 40
    g(150, 0, false, false),   // 41
    g(-150, 0, false, false),  // 42
];

const LAYOUT_1: &[u8] = &[2];
const LAYOUT_2: &[u8] = &[0, 1];
const LAYOUT_3: &[u8] = &[0, 1, 2];
const LAYOUT_4: &[u8] = &[0, 1, 2, 10];
const LAYOUT_5: &[u8] = &[0, 1, 2, 4, 5];
const LAYOUT_6: &[u8] = &[0, 1, 2, 3, 4, 5];
const LAYOUT_7: &[u8] = &[0, 1, 2, 3, 4, 5, 15, 16];
const LAYOUT_9: &[u8] = &[0, 1, 10];
const LAYOUT_10: &[u8] = &[0, 1, 4, 5];
const LAYOUT_11: &[u8] = &[0, 1, 2, 3, 4, 5, 10];
const LAYOUT_12: &[u8] = &[0, 1, 2, 3, 4, 5, 8, 9];
const LAYOUT_13: &[u8] = &[
    15, 16, 2, 26, 8, 9, 0, 1, 10, 36, 13, 14, 32, 33, 19, 25, 20, 21, 23, 24, 22, 29,
    27, 28,
];
const LAYOUT_14: &[u8] = &[0, 1, 2, 3, 4, 5, 17, 18];
const LAYOUT_15: &[u8] = &[0, 1, 2, 26, 8, 9, 36, 13, 14, 32, 33, 22];
const LAYOUT_16: &[u8] = &[0, 1, 2, 3, 4, 5, 17, 18, 30, 31];
const LAYOUT_17: &[u8] = &[0, 1, 2, 3, 4, 5, 17, 18, 19, 30, 31, 25];
const LAYOUT_18: &[u8] = &[0, 1, 2, 3, 4, 5, 41, 42, 17, 18, 19, 30, 31, 25];
const LAYOUT_19: &[u8] = &[0, 1, 2, 3, 8, 9, 13, 14, 17, 18, 20, 21];
const LAYOUT_20: &[u8] = &[0, 1, 2, 3, 8, 9, 13, 14, 32, 33, 20, 21, 37, 38];

pub fn cicp_speaker_geometry(index: u8) -> Option<CicpSpeakerGeometry> {
    if matches!(index, 11 | 12) {
        return None;
    }
    SPEAKERS.get(usize::from(index)).copied()
}

pub fn cicp_layout_members(layout_index: u8) -> Option<&'static [u8]> {
    match layout_index {
        1 => Some(LAYOUT_1),
        2 => Some(LAYOUT_2),
        3 => Some(LAYOUT_3),
        4 => Some(LAYOUT_4),
        5 => Some(LAYOUT_5),
        6 => Some(LAYOUT_6),
        7 => Some(LAYOUT_7),
        8 => None,
        9 => Some(LAYOUT_9),
        10 => Some(LAYOUT_10),
        11 => Some(LAYOUT_11),
        12 => Some(LAYOUT_12),
        13 => Some(LAYOUT_13),
        14 => Some(LAYOUT_14),
        15 => Some(LAYOUT_15),
        16 => Some(LAYOUT_16),
        17 => Some(LAYOUT_17),
        18 => Some(LAYOUT_18),
        19 => Some(LAYOUT_19),
        20 => Some(LAYOUT_20),
        _ => None,
    }
}

pub fn cicp_layout_member_geometry(
    layout_index: u8,
    member_index: u16,
) -> Option<CicpSpeakerGeometry> {
    let speaker_index = *cicp_layout_members(layout_index)?.get(usize::from(member_index))?;
    cicp_speaker_geometry(speaker_index)
}

pub fn cicp_layout_member_speaker_index(layout_index: u8, member_index: u16) -> Option<u8> {
    cicp_layout_members(layout_index)?
        .get(usize::from(member_index))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_layout_resolves_to_plus_minus_thirty_degrees() {
        let left = cicp_layout_member_geometry(2, 0).unwrap();
        let right = cicp_layout_member_geometry(2, 1).unwrap();
        assert_eq!(left.azimuth_degrees, 30);
        assert_eq!(right.azimuth_degrees, -30);
        assert!(!left.is_lfe);
        assert!(!right.is_lfe);
    }

    #[test]
    fn five_one_layout_contains_lfe_and_surround_pair() {
        let members = cicp_layout_members(6).unwrap();
        assert_eq!(members, &[0, 1, 2, 3, 4, 5]);
        assert!(cicp_layout_member_geometry(6, 3).unwrap().is_lfe);
        assert_eq!(cicp_layout_member_geometry(6, 4).unwrap().azimuth_degrees, 110);
        assert_eq!(cicp_layout_member_geometry(6, 5).unwrap().azimuth_degrees, -110);
    }

    #[test]
    fn immersive_layout_13_has_24_members() {
        assert_eq!(cicp_layout_members(13).unwrap().len(), 24);
        assert!(cicp_layout_members(8).is_none());
    }

    #[test]
    fn reserved_speaker_indices_do_not_resolve() {
        assert!(cicp_speaker_geometry(11).is_none());
        assert!(cicp_speaker_geometry(12).is_none());
    }

    #[test]
    fn screen_relative_speakers_are_preserved() {
        assert!(cicp_speaker_geometry(37).unwrap().screen_relative);
        assert!(cicp_speaker_geometry(38).unwrap().screen_relative);
    }
}
