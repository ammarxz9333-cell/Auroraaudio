use aurora_spatial_transport_v2::TransportSceneDomain;
use thiserror::Error;

/// Aurora candidate renderer family selected for one MPEG-H transport domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpeghCandidateFamily {
    /// Discrete bed, exact point objects, or their combination without HOA.
    NonHoa,
    /// HOA coefficient rendering, optionally with a discrete bed mixed into the
    /// exact libmpegh reference speaker order.
    Hoa,
    /// Exact-per-sample point-object rendering plus HOA, optionally with a
    /// discrete bed mixed into the exact libmpegh reference speaker order.
    ExactScene,
}

/// Select the admitted Aurora candidate renderer without touching codec/native
/// state. Every currently representable Spatial Transport V2 domain has a
/// candidate family; each family still fails closed internally when richer
/// metadata requires semantics Aurora has not independently admitted yet.
pub fn candidate_family_for_domain(
    domain: TransportSceneDomain,
) -> Result<MpeghCandidateFamily, MpeghCandidateDispatchError> {
    match domain {
        TransportSceneDomain::DiscreteBed
        | TransportSceneDomain::ObjectSignals
        | TransportSceneDomain::BedAndObjects => Ok(MpeghCandidateFamily::NonHoa),
        TransportSceneDomain::HoaTransport | TransportSceneDomain::BedAndHoa => {
            Ok(MpeghCandidateFamily::Hoa)
        }
        TransportSceneDomain::ObjectsAndHoa | TransportSceneDomain::BedObjectsAndHoa => {
            Ok(MpeghCandidateFamily::ExactScene)
        }
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
    fn non_hoa_domains_select_non_hoa_family() {
        for domain in [
            TransportSceneDomain::DiscreteBed,
            TransportSceneDomain::ObjectSignals,
            TransportSceneDomain::BedAndObjects,
        ] {
            assert_eq!(
                candidate_family_for_domain(domain).unwrap(),
                MpeghCandidateFamily::NonHoa
            );
        }
    }

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
}
