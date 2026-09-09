use aurora_spatial_transport_v2::TransportSceneDomain;
use thiserror::Error;

/// Aurora candidate renderer family selected for one MPEG-H transport domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpeghCandidateFamily {
    /// HOA coefficient rendering, optionally with a discrete bed mixed into the
    /// exact libmpegh reference speaker order.
    Hoa,
    /// Exact-per-sample point-object rendering plus HOA, optionally with a
    /// discrete bed mixed into the exact libmpegh reference speaker order.
    ExactScene,
}

/// Select the admitted Aurora candidate renderer without touching codec/native
/// state. Non-HOA domains remain fail-closed because the current evidence path
/// is specifically anchored to the paired libmpegh HOA coefficient observer.
pub fn candidate_family_for_domain(
    domain: TransportSceneDomain,
) -> Result<MpeghCandidateFamily, MpeghCandidateDispatchError> {
    match domain {
        TransportSceneDomain::HoaTransport | TransportSceneDomain::BedAndHoa => {
            Ok(MpeghCandidateFamily::Hoa)
        }
        TransportSceneDomain::ObjectsAndHoa | TransportSceneDomain::BedObjectsAndHoa => {
            Ok(MpeghCandidateFamily::ExactScene)
        }
        unsupported => Err(MpeghCandidateDispatchError::UnsupportedDomain {
            domain: format!("{unsupported:?}"),
        }),
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MpeghCandidateDispatchError {
    #[error("MPEG-H immersive candidate dispatcher does not admit scene domain {domain}")]
    UnsupportedDomain { domain: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hoa_domains_select_hoa_family() {
        assert_eq!(
            candidate_family_for_domain(TransportSceneDomain::HoaTransport).unwrap(),
            MpeghCandidateFamily::Hoa
        );
        assert_eq!(
            candidate_family_for_domain(TransportSceneDomain::BedAndHoa).unwrap(),
            MpeghCandidateFamily::Hoa
        );
    }

    #[test]
    fn object_hoa_domains_select_exact_scene_family() {
        assert_eq!(
            candidate_family_for_domain(TransportSceneDomain::ObjectsAndHoa).unwrap(),
            MpeghCandidateFamily::ExactScene
        );
        assert_eq!(
            candidate_family_for_domain(TransportSceneDomain::BedObjectsAndHoa).unwrap(),
            MpeghCandidateFamily::ExactScene
        );
    }

    #[test]
    fn non_hoa_domains_fail_closed() {
        for domain in [
            TransportSceneDomain::DiscreteBed,
            TransportSceneDomain::ObjectSignals,
            TransportSceneDomain::BedAndObjects,
        ] {
            assert!(matches!(
                candidate_family_for_domain(domain),
                Err(MpeghCandidateDispatchError::UnsupportedDomain { .. })
            ));
        }
    }
}
