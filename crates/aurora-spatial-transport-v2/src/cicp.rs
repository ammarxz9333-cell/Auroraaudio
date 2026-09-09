//! CICP loudspeaker geometry used by immersive transport metadata.
//!
//! The values mirror the MPEG-H reference decoder's CICP geometry ROM. Aurora
//! stores only the standardized numeric geometry/layout facts needed to resolve
//! speaker indices and layout-member positions; no reference decoder code is
//! executed or embedded here.

use aurora_core::ChannelRole;

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

const SPEAKERS: [CicpSpeakerGeometry; 43] = [
    g(30, 0, false, false), g(-30, 0, false, false), g(0, 0, false, false),
    g(0, 0, true, false), g(110, 0, false, false), g(-110, 0, false, false),
    g(22, 0, false, false), g(-22, 0, false, false), g(135, 0, false, false),
    g(-135, 0, false, false), g(180, 0, false, false), g(0, 0, false, false),
    g(0, 0, false, false), g(90, 0, false, false), g(-90, 0, false, false),
    g(60, 0, false, false), g(-60, 0, false, false), g(30, 35, false, false),
    g(-30, 35, false, false), g(0, 35, false, false), g(135, 35, false, false),
    g(-135, 35, false, false), g(180, 35, false, false), g(90, 35, false, false),
    g(-90, 35, false, false), g(0, 90, false, false), g(45, -15, true, false),
    g(45, -15, false, false), g(-45, -15, false, false), g(0, -15, false, false),
    g(110, 35, false, false), g(-110, 35, false, false), g(45, 35, false, false),
    g(-45, 35, false, false), g(45, 0, false, false), g(-45, 0, false, false),
    g(-45, -15, true, false), g(60, 0, false, true), g(-60, 0, false, true),
    g(30, 0, false, true), g(-30, 0, false, true), g(150, 0, false, false),
    g(-150, 0, false, false),
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

/// Lossless semantic projection for CICP speakers that have an exact Aurora
/// channel role. Speakers such as front-wide, top-center, lower-layer and
/// screen-relative channels deliberately return `None` instead of being folded
/// into a nearby 7.1.4 role.
pub fn cicp_speaker_semantic_role(index: u8) -> Option<ChannelRole> {
    Some(match index {
        0 => ChannelRole::FrontLeft,
        1 => ChannelRole::FrontRight,
        2 => ChannelRole::FrontCenter,
        3 | 26 | 36 => ChannelRole::LowFrequencyEffects,
        4 | 13 => ChannelRole::SurroundLeft,
        5 | 14 => ChannelRole::SurroundRight,
        8 | 41 => ChannelRole::SurroundBackLeft,
        9 | 42 => ChannelRole::SurroundBackRight,
        17 | 32 => ChannelRole::TopFrontLeft,
        18 | 33 => ChannelRole::TopFrontRight,
        20 | 30 => ChannelRole::TopRearLeft,
        21 | 31 => ChannelRole::TopRearRight,
        _ => return None,
    })
}

pub fn cicp_layout_members(layout_index: u8) -> Option<&'static [u8]> {
    match layout_index {
        1 => Some(LAYOUT_1), 2 => Some(LAYOUT_2), 3 => Some(LAYOUT_3),
        4 => Some(LAYOUT_4), 5 => Some(LAYOUT_5), 6 => Some(LAYOUT_6),
        7 => Some(LAYOUT_7), 8 => None, 9 => Some(LAYOUT_9),
        10 => Some(LAYOUT_10), 11 => Some(LAYOUT_11), 12 => Some(LAYOUT_12),
        13 => Some(LAYOUT_13), 14 => Some(LAYOUT_14), 15 => Some(LAYOUT_15),
        16 => Some(LAYOUT_16), 17 => Some(LAYOUT_17), 18 => Some(LAYOUT_18),
        19 => Some(LAYOUT_19), 20 => Some(LAYOUT_20), _ => None,
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

pub fn cicp_layout_member_semantic_role(
    layout_index: u8,
    member_index: u16,
) -> Option<ChannelRole> {
    cicp_speaker_semantic_role(cicp_layout_member_speaker_index(
        layout_index,
        member_index,
    )?)
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
        assert_eq!(cicp_layout_member_semantic_role(6, 4), Some(ChannelRole::SurroundLeft));
        assert_eq!(cicp_layout_member_semantic_role(6, 5), Some(ChannelRole::SurroundRight));
    }

    #[test]
    fn cicp_layout_19_projects_losslessly_to_aurora_7_1_4_roles() {
        let expected = vec![
            ChannelRole::FrontLeft,
            ChannelRole::FrontRight,
            ChannelRole::FrontCenter,
            ChannelRole::LowFrequencyEffects,
            ChannelRole::SurroundBackLeft,
            ChannelRole::SurroundBackRight,
            ChannelRole::SurroundLeft,
            ChannelRole::SurroundRight,
            ChannelRole::TopFrontLeft,
            ChannelRole::TopFrontRight,
            ChannelRole::TopRearLeft,
            ChannelRole::TopRearRight,
        ];
        let actual = (0..LAYOUT_19.len())
            .map(|index| cicp_layout_member_semantic_role(19, index as u16).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn unsupported_geometry_is_not_folded_into_nearest_role() {
        assert!(cicp_speaker_semantic_role(15).is_none()); // front-wide-ish +60
        assert!(cicp_speaker_semantic_role(25).is_none()); // top center
        assert!(cicp_speaker_semantic_role(37).is_none()); // screen-relative
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
