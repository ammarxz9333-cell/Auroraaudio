use std::collections::HashSet;

use aurora_core::{ChannelRole, StandardLayout};
use thiserror::Error;

pub const MAX_OUTPUT_CHANNELS: usize = 64;
pub const AURORA_ROLE_FRONT_WIDE_LEFT: &str = "front-wide-left";
pub const AURORA_ROLE_FRONT_WIDE_RIGHT: &str = "front-wide-right";
pub const AURORA_ROLE_REAR_SIDE_LEFT: &str = "rear-side-left";
pub const AURORA_ROLE_REAR_SIDE_RIGHT: &str = "rear-side-right";
pub const AURORA_ELEVEN_ONE_FOUR_REFERENCE_NAME: &str = "aurora-11.1.4-reference-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputChannelClass {
    Bed,
    Lfe,
    Height,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputChannelSpec {
    pub role: ChannelRole,
    pub class: OutputChannelClass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLayoutContract {
    name: String,
    channels: Vec<OutputChannelSpec>,
    lfe_index: Option<usize>,
    height_indices: Vec<usize>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OutputLayoutContractError {
    #[error("output layout name must not be empty")]
    EmptyName,
    #[error("output layout must contain at least two channels")]
    TooFewChannels,
    #[error("output layout has {actual} channels; maximum is {maximum}")]
    TooManyChannels { maximum: usize, actual: usize },
    #[error("duplicate output role {0}")]
    DuplicateRole(String),
    #[error("output layout may contain at most one LFE channel")]
    MultipleLfe,
    #[error("role {role} must be classified as {expected:?}, got {actual:?}")]
    InvalidClass {
        role: String,
        expected: OutputChannelClass,
        actual: OutputChannelClass,
    },
    #[error("custom StandardLayout requires an explicit OutputLayoutContract")]
    CustomRequiresExplicitContract,
}

impl OutputLayoutContract {
    pub fn for_standard(layout: StandardLayout) -> Result<Self, OutputLayoutContractError> {
        if layout == StandardLayout::Custom {
            return Err(OutputLayoutContractError::CustomRequiresExplicitContract);
        }
        let name = standard_layout_name(layout);
        let channels = layout
            .canonical_roles()
            .iter()
            .cloned()
            .map(|role| OutputChannelSpec {
                class: standard_role_class(&role),
                role,
            })
            .collect();
        Self::custom(name, channels)
    }

    /// Aurora-owned 11.1.4 reference semantics for the wider product path.
    ///
    /// This is deliberately not presented as a Dolby, ITU or Samsung channel
    /// naming standard. It keeps the established Aurora 7.1 bed, adds explicit
    /// front-wide and rear-side pairs, then retains four canonical height lanes.
    /// The ordered result is eleven horizontal full-range lanes, one LFE and
    /// four height lanes (sixteen outputs total).
    pub fn aurora_eleven_one_four_reference() -> Result<Self, OutputLayoutContractError> {
        Self::custom(
            AURORA_ELEVEN_ONE_FOUR_REFERENCE_NAME,
            vec![
                OutputChannelSpec {
                    role: ChannelRole::FrontLeft,
                    class: OutputChannelClass::Bed,
                },
                OutputChannelSpec {
                    role: ChannelRole::FrontRight,
                    class: OutputChannelClass::Bed,
                },
                OutputChannelSpec {
                    role: ChannelRole::FrontCenter,
                    class: OutputChannelClass::Bed,
                },
                OutputChannelSpec {
                    role: ChannelRole::LowFrequencyEffects,
                    class: OutputChannelClass::Lfe,
                },
                OutputChannelSpec {
                    role: ChannelRole::SurroundLeft,
                    class: OutputChannelClass::Bed,
                },
                OutputChannelSpec {
                    role: ChannelRole::SurroundRight,
                    class: OutputChannelClass::Bed,
                },
                OutputChannelSpec {
                    role: ChannelRole::SurroundBackLeft,
                    class: OutputChannelClass::Bed,
                },
                OutputChannelSpec {
                    role: ChannelRole::SurroundBackRight,
                    class: OutputChannelClass::Bed,
                },
                OutputChannelSpec {
                    role: ChannelRole::Custom(AURORA_ROLE_FRONT_WIDE_LEFT.to_owned()),
                    class: OutputChannelClass::Bed,
                },
                OutputChannelSpec {
                    role: ChannelRole::Custom(AURORA_ROLE_FRONT_WIDE_RIGHT.to_owned()),
                    class: OutputChannelClass::Bed,
                },
                OutputChannelSpec {
                    role: ChannelRole::Custom(AURORA_ROLE_REAR_SIDE_LEFT.to_owned()),
                    class: OutputChannelClass::Bed,
                },
                OutputChannelSpec {
                    role: ChannelRole::Custom(AURORA_ROLE_REAR_SIDE_RIGHT.to_owned()),
                    class: OutputChannelClass::Bed,
                },
                OutputChannelSpec {
                    role: ChannelRole::TopFrontLeft,
                    class: OutputChannelClass::Height,
                },
                OutputChannelSpec {
                    role: ChannelRole::TopFrontRight,
                    class: OutputChannelClass::Height,
                },
                OutputChannelSpec {
                    role: ChannelRole::TopRearLeft,
                    class: OutputChannelClass::Height,
                },
                OutputChannelSpec {
                    role: ChannelRole::TopRearRight,
                    class: OutputChannelClass::Height,
                },
            ],
        )
    }

    pub fn custom(
        name: impl Into<String>,
        channels: Vec<OutputChannelSpec>,
    ) -> Result<Self, OutputLayoutContractError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(OutputLayoutContractError::EmptyName);
        }
        if channels.len() < 2 {
            return Err(OutputLayoutContractError::TooFewChannels);
        }
        if channels.len() > MAX_OUTPUT_CHANNELS {
            return Err(OutputLayoutContractError::TooManyChannels {
                maximum: MAX_OUTPUT_CHANNELS,
                actual: channels.len(),
            });
        }

        let mut seen = HashSet::with_capacity(channels.len());
        let mut lfe_index = None;
        let mut height_indices = Vec::new();
        for (index, channel) in channels.iter().enumerate() {
            if !seen.insert(channel.role.clone()) {
                return Err(OutputLayoutContractError::DuplicateRole(
                    channel.role.to_string(),
                ));
            }
            validate_known_role_class(channel)?;
            match channel.class {
                OutputChannelClass::Lfe => {
                    if lfe_index.replace(index).is_some() {
                        return Err(OutputLayoutContractError::MultipleLfe);
                    }
                }
                OutputChannelClass::Height => height_indices.push(index),
                OutputChannelClass::Bed => {}
            }
        }

        Ok(Self {
            name,
            channels,
            lfe_index,
            height_indices,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn channels(&self) -> &[OutputChannelSpec] {
        &self.channels
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    pub fn lfe_index(&self) -> Option<usize> {
        self.lfe_index
    }

    pub fn height_indices(&self) -> &[usize] {
        &self.height_indices
    }
}

fn standard_layout_name(layout: StandardLayout) -> &'static str {
    match layout {
        StandardLayout::Stereo => "2.0",
        StandardLayout::FiveOne => "5.1",
        StandardLayout::SevenOne => "7.1",
        StandardLayout::FiveOneTwo => "5.1.2",
        StandardLayout::FiveOneFour => "5.1.4",
        StandardLayout::SevenOneTwo => "7.1.2",
        StandardLayout::SevenOneFour => "7.1.4",
        StandardLayout::Custom => unreachable!("custom layout rejected before name resolution"),
    }
}

fn standard_role_class(role: &ChannelRole) -> OutputChannelClass {
    match role {
        ChannelRole::LowFrequencyEffects => OutputChannelClass::Lfe,
        ChannelRole::TopFrontLeft
        | ChannelRole::TopFrontRight
        | ChannelRole::TopRearLeft
        | ChannelRole::TopRearRight => OutputChannelClass::Height,
        _ => OutputChannelClass::Bed,
    }
}

fn validate_known_role_class(
    channel: &OutputChannelSpec,
) -> Result<(), OutputLayoutContractError> {
    let expected = match channel.role {
        ChannelRole::LowFrequencyEffects => Some(OutputChannelClass::Lfe),
        ChannelRole::TopFrontLeft
        | ChannelRole::TopFrontRight
        | ChannelRole::TopRearLeft
        | ChannelRole::TopRearRight => Some(OutputChannelClass::Height),
        ChannelRole::Custom(_) => None,
        _ => Some(OutputChannelClass::Bed),
    };
    if let Some(expected) = expected {
        if channel.class != expected {
            return Err(OutputLayoutContractError::InvalidClass {
                role: channel.role.to_string(),
                expected,
                actual: channel.class,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_one_four_contract_has_expected_lfe_and_heights() {
        let layout = OutputLayoutContract::for_standard(StandardLayout::SevenOneFour).unwrap();
        assert_eq!(layout.name(), "7.1.4");
        assert_eq!(layout.channel_count(), 12);
        assert_eq!(layout.lfe_index(), Some(3));
        assert_eq!(layout.height_indices(), &[8, 9, 10, 11]);
    }

    #[test]
    fn aurora_reference_eleven_one_four_has_explicit_sixteen_lane_identity() {
        let layout = OutputLayoutContract::aurora_eleven_one_four_reference().unwrap();
        assert_eq!(layout.name(), AURORA_ELEVEN_ONE_FOUR_REFERENCE_NAME);
        assert_eq!(layout.channel_count(), 16);
        assert_eq!(layout.lfe_index(), Some(3));
        assert_eq!(layout.height_indices(), &[12, 13, 14, 15]);
        assert_eq!(
            layout.channels()[8].role,
            ChannelRole::Custom(AURORA_ROLE_FRONT_WIDE_LEFT.to_owned())
        );
        assert_eq!(
            layout.channels()[9].role,
            ChannelRole::Custom(AURORA_ROLE_FRONT_WIDE_RIGHT.to_owned())
        );
        assert_eq!(
            layout.channels()[10].role,
            ChannelRole::Custom(AURORA_ROLE_REAR_SIDE_LEFT.to_owned())
        );
        assert_eq!(
            layout.channels()[11].role,
            ChannelRole::Custom(AURORA_ROLE_REAR_SIDE_RIGHT.to_owned())
        );
    }

    #[test]
    fn height_variants_keep_semantic_widths() {
        let five_one_four =
            OutputLayoutContract::for_standard(StandardLayout::FiveOneFour).unwrap();
        assert_eq!(five_one_four.channel_count(), 10);
        assert_eq!(five_one_four.height_indices(), &[6, 7, 8, 9]);

        let seven_one_two =
            OutputLayoutContract::for_standard(StandardLayout::SevenOneTwo).unwrap();
        assert_eq!(seven_one_two.channel_count(), 10);
        assert_eq!(seven_one_two.height_indices(), &[8, 9]);
    }

    #[test]
    fn stereo_contract_has_no_lfe_or_height_channels() {
        let layout = OutputLayoutContract::for_standard(StandardLayout::Stereo).unwrap();
        assert_eq!(layout.channel_count(), 2);
        assert_eq!(layout.lfe_index(), None);
        assert!(layout.height_indices().is_empty());
    }

    #[test]
    fn custom_roles_can_describe_future_extra_bed_lanes() {
        let layout = OutputLayoutContract::custom(
            "future-wide-layout",
            vec![
                OutputChannelSpec {
                    role: ChannelRole::FrontLeft,
                    class: OutputChannelClass::Bed,
                },
                OutputChannelSpec {
                    role: ChannelRole::FrontRight,
                    class: OutputChannelClass::Bed,
                },
                OutputChannelSpec {
                    role: ChannelRole::Custom("wide-left".to_owned()),
                    class: OutputChannelClass::Bed,
                },
                OutputChannelSpec {
                    role: ChannelRole::Custom("wide-right".to_owned()),
                    class: OutputChannelClass::Bed,
                },
            ],
        )
        .unwrap();
        assert_eq!(layout.channel_count(), 4);
        assert_eq!(layout.lfe_index(), None);
    }

    #[test]
    fn duplicate_roles_and_multiple_lfe_fail_closed() {
        let duplicate = OutputLayoutContract::custom(
            "duplicate",
            vec![
                OutputChannelSpec {
                    role: ChannelRole::FrontLeft,
                    class: OutputChannelClass::Bed,
                },
                OutputChannelSpec {
                    role: ChannelRole::FrontLeft,
                    class: OutputChannelClass::Bed,
                },
            ],
        );
        assert!(matches!(
            duplicate,
            Err(OutputLayoutContractError::DuplicateRole(_))
        ));

        let multiple_lfe = OutputLayoutContract::custom(
            "two-lfe",
            vec![
                OutputChannelSpec {
                    role: ChannelRole::Custom("sub-a".to_owned()),
                    class: OutputChannelClass::Lfe,
                },
                OutputChannelSpec {
                    role: ChannelRole::Custom("sub-b".to_owned()),
                    class: OutputChannelClass::Lfe,
                },
            ],
        );
        assert_eq!(multiple_lfe, Err(OutputLayoutContractError::MultipleLfe));
    }

    #[test]
    fn standard_roles_cannot_be_misclassified() {
        let invalid = OutputLayoutContract::custom(
            "bad-height",
            vec![
                OutputChannelSpec {
                    role: ChannelRole::FrontLeft,
                    class: OutputChannelClass::Height,
                },
                OutputChannelSpec {
                    role: ChannelRole::FrontRight,
                    class: OutputChannelClass::Bed,
                },
            ],
        );
        assert!(matches!(
            invalid,
            Err(OutputLayoutContractError::InvalidClass { .. })
        ));
    }
}
