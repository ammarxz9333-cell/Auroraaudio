//! Spatial IR V2 runtime bridge.
//!
//! This crate now exposes two paths:
//! - a lossless compatibility bridge into the proven V1 runtime, and
//! - a native V2 reference renderer that consumes admitted rich metadata
//!   directly instead of stripping it.

#![forbid(unsafe_code)]

mod native;
mod transport;
pub use native::{NativeV2SpatialRuntime, NativeV2SpatialRuntimeError};
pub use transport::{project_transport_to_spatial_v2, TransportV2RuntimeError};

use aurora_core::AudioBlock;
use aurora_spatial_ir::{
    BedSignalBinding as BedSignalBindingV1, ObjectSignalBinding as ObjectSignalBindingV1,
    SpatialDecodedFrame as SpatialDecodedFrameV1, SpatialFrameMetadata as SpatialFrameMetadataV1,
    SpatialObjectUpdate as SpatialObjectUpdateV1,
};
use aurora_spatial_ir_v2::{
    SpatialDecodedFrame as SpatialDecodedFrameV2, SpatialRenderingProperties,
};
use aurora_spatial_runtime::{SpatialRuntimeConfig, SpatialRuntimeError, VbapSpatialRuntime};
use thiserror::Error;

/// Migration-safe V2 wrapper. Use [`NativeV2SpatialRuntime`] when rich metadata
/// should be consumed rather than rejected.
pub struct V2SpatialRuntime {
    inner: VbapSpatialRuntime,
}

impl V2SpatialRuntime {
    pub fn new(config: SpatialRuntimeConfig) -> Result<Self, V2SpatialRuntimeError> {
        Ok(Self {
            inner: VbapSpatialRuntime::new(config)?,
        })
    }

    pub fn reset(&mut self) {
        self.inner.reset();
    }

    pub fn output_channel_count(&self) -> usize {
        self.inner.output_channel_count()
    }

    pub fn render_frame(
        &mut self,
        frame: &SpatialDecodedFrameV2,
    ) -> Result<AudioBlock, V2SpatialRuntimeError> {
        let legacy = downgrade_when_lossless(frame)?;
        Ok(self.inner.render_frame(&legacy)?)
    }
}

/// Convert V2 to V1 only if no V2-only semantics would be discarded.
pub fn downgrade_when_lossless(
    frame: &SpatialDecodedFrameV2,
) -> Result<SpatialDecodedFrameV1, V2SpatialRuntimeError> {
    frame
        .validate()
        .map_err(|error| V2SpatialRuntimeError::InvalidV2(error.to_string()))?;

    for update in &frame.spatial.object_updates {
        if update.rendering != SpatialRenderingProperties::default() {
            return Err(V2SpatialRuntimeError::RichMetadataRequiresV2Renderer {
                object_id: update.object_id.clone(),
            });
        }
    }

    let downgraded = SpatialDecodedFrameV1 {
        decoded: frame.decoded.clone(),
        spatial: SpatialFrameMetadataV1 {
            domain: frame.spatial.domain,
            bed_signals: frame
                .spatial
                .bed_signals
                .iter()
                .map(|bed| BedSignalBindingV1 {
                    pcm_channel_index: bed.pcm_channel_index,
                    role: bed.role.clone(),
                })
                .collect(),
            object_signals: frame
                .spatial
                .object_signals
                .iter()
                .map(|object| ObjectSignalBindingV1 {
                    id: object.id.clone(),
                    pcm_channel_index: object.pcm_channel_index,
                })
                .collect(),
            object_updates: frame
                .spatial
                .object_updates
                .iter()
                .map(|update| SpatialObjectUpdateV1 {
                    object_id: update.object_id.clone(),
                    active: update.active,
                    coordinate_space: update.coordinate_space,
                    position: update.position,
                    gain_db: update.gain_db,
                    spread: update.spread,
                    metadata_sample_offset: update.metadata_sample_offset,
                    ramp_duration_samples: update.ramp_duration_samples,
                    priority: update.priority,
                })
                .collect(),
        },
    };

    downgraded.validate().map_err(|error| {
        V2SpatialRuntimeError::InvalidLosslessDowngrade(error.to_string())
    })?;
    Ok(downgraded)
}

#[derive(Debug, Error)]
pub enum V2SpatialRuntimeError {
    #[error("Spatial IR V2 validation failed: {0}")]
    InvalidV2(String),
    #[error("object '{object_id}' carries V2-only renderer metadata; legacy VBAP rendering would discard it")]
    RichMetadataRequiresV2Renderer { object_id: String },
    #[error("lossless V2-to-V1 bridge produced invalid V1 data: {0}")]
    InvalidLosslessDowngrade(String),
    #[error(transparent)]
    LegacyRuntime(#[from] SpatialRuntimeError),
}

#[cfg(test)]
mod tests {
    use aurora_core::AudioBlock;
    use aurora_decoder_api::DecodedFrame;
    use aurora_spatial_ir_v2::{
        BinauralIntent, ObjectDistance, ObjectSignalBinding, SpatialDecodedFrame,
        SpatialDomain, SpatialFrameMetadata, SpatialObjectUpdate, SpatialPosition,
        SpatialRenderingProperties,
    };

    use super::*;

    fn frame(rendering: SpatialRenderingProperties) -> SpatialDecodedFrame {
        SpatialDecodedFrame {
            decoded: DecodedFrame {
                audio: AudioBlock {
                    channels: vec![vec![0.0; 40]],
                    frame_count: 40,
                    presentation_time_seconds: 0.0,
                    discontinuity: false,
                },
                objects: Vec::new(),
            },
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::ObjectSignals,
                bed_signals: Vec::new(),
                object_signals: vec![ObjectSignalBinding {
                    id: "o0".into(),
                    pcm_channel_index: 0,
                }],
                object_updates: vec![SpatialObjectUpdate {
                    object_id: "o0".into(),
                    active: true,
                    coordinate_space: aurora_spatial_ir_v2::CoordinateSpace::RoomNormalized,
                    position: SpatialPosition::Cartesian {
                        x: 0.5,
                        y: 0.5,
                        z: 0.0,
                    },
                    gain_db: 0.0,
                    spread: 0.0,
                    metadata_sample_offset: 0,
                    ramp_duration_samples: 0,
                    priority: Some(1.0),
                    rendering,
                }],
            },
        }
    }

    #[test]
    fn default_v2_metadata_downgrades_without_information_loss() {
        let frame = frame(SpatialRenderingProperties::default());
        let downgraded = downgrade_when_lossless(&frame).unwrap();
        assert_eq!(downgraded.spatial.object_updates.len(), 1);
        assert_eq!(downgraded.spatial.object_updates[0].object_id, "o0");
    }

    #[test]
    fn rich_truehd_metadata_is_never_silently_discarded() {
        let rendering = SpatialRenderingProperties {
            distance: ObjectDistance::Meters(2.5),
            binaural_intent: BinauralIntent::CodecSpecific(2),
            ..SpatialRenderingProperties::default()
        };
        let error = downgrade_when_lossless(&frame(rendering)).unwrap_err();
        assert!(matches!(
            error,
            V2SpatialRuntimeError::RichMetadataRequiresV2Renderer { .. }
        ));
    }
}
