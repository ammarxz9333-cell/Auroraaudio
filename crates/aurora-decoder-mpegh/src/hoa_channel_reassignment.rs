use thiserror::Error;

/// Zero additional-coder transport channels that are not currently used by a
/// vector- or direction-based predominant sound. Mirrors the MPEG-H reference
/// channel-reassignment stage; transport ids are one-based in the codec side
/// information and converted here to zero-based PCM indices.
pub fn clear_non_predominant_additional_coders(
    corrected_transport_pcm: &mut [Vec<f32>],
    num_additional_coders: usize,
    active_vector_transport_ids_one_based: &[usize],
    active_direction_transport_ids_one_based: &[usize],
) -> Result<(), MpeghHoaChannelReassignmentError> {
    if num_additional_coders > corrected_transport_pcm.len() {
        return Err(MpeghHoaChannelReassignmentError::AdditionalCoderCountOutOfRange {
            additional: num_additional_coders,
            transport_channels: corrected_transport_pcm.len(),
        });
    }
    let frame_length = corrected_transport_pcm
        .first()
        .map(Vec::len)
        .ok_or(MpeghHoaChannelReassignmentError::ZeroTransportChannels)?;
    if corrected_transport_pcm
        .iter()
        .any(|channel| channel.len() != frame_length)
    {
        return Err(MpeghHoaChannelReassignmentError::InconsistentFrameLength);
    }

    let mut keep = vec![false; num_additional_coders];
    for id in active_vector_transport_ids_one_based
        .iter()
        .chain(active_direction_transport_ids_one_based.iter())
        .copied()
    {
        if id == 0 || id > num_additional_coders {
            return Err(MpeghHoaChannelReassignmentError::PredominantTransportIdOutOfRange {
                id,
                additional: num_additional_coders,
            });
        }
        keep[id - 1] = true;
    }

    for (channel, retain) in corrected_transport_pcm
        .iter_mut()
        .take(num_additional_coders)
        .zip(keep)
    {
        if !retain {
            channel.fill(0.0);
        }
    }
    Ok(())
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MpeghHoaChannelReassignmentError {
    #[error("HOA channel reassignment requires at least one transport channel")]
    ZeroTransportChannels,
    #[error("HOA transport channels have inconsistent frame lengths")]
    InconsistentFrameLength,
    #[error("HOA has {additional} additional coders but only {transport_channels} transport channels")]
    AdditionalCoderCountOutOfRange {
        additional: usize,
        transport_channels: usize,
    },
    #[error("predominant HOA transport id {id} is outside the {additional} additional coders")]
    PredominantTransportIdOutOfRange { id: usize, additional: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_active_predominant_additional_coders_survive() {
        let mut pcm = vec![
            vec![1.0, 1.0],
            vec![2.0, 2.0],
            vec![3.0, 3.0],
            vec![4.0, 4.0],
        ];
        clear_non_predominant_additional_coders(&mut pcm, 3, &[2], &[3]).unwrap();
        assert_eq!(pcm[0], vec![0.0, 0.0]);
        assert_eq!(pcm[1], vec![2.0, 2.0]);
        assert_eq!(pcm[2], vec![3.0, 3.0]);
        // Transport channels beyond num_additional_coders are untouched.
        assert_eq!(pcm[3], vec![4.0, 4.0]);
    }

    #[test]
    fn invalid_one_based_transport_id_is_rejected() {
        let mut pcm = vec![vec![0.0; 2]; 2];
        assert!(matches!(
            clear_non_predominant_additional_coders(&mut pcm, 2, &[0], &[]),
            Err(MpeghHoaChannelReassignmentError::PredominantTransportIdOutOfRange { .. })
        ));
    }
}
