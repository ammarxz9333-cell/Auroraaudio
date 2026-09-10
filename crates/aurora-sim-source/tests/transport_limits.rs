use aurora_iec61937::{BurstParser, CodecFilter};
use aurora_sim_source::{
    EAC3_BURST_PERIOD_BYTES, EAC3_MAX_PAYLOAD_BYTES, SimSourceError, write_eac3_period,
};

#[test]
fn maximum_eac3_transport_payload_round_trips_exactly() {
    let payload: Vec<u8> = (0..EAC3_MAX_PAYLOAD_BYTES)
        .map(|index| (index as u8).wrapping_mul(29).wrapping_add(7))
        .collect();
    let mut period = [0_u8; EAC3_BURST_PERIOD_BYTES];

    write_eac3_period(&payload, &mut period).unwrap();

    let mut parser = BurstParser::new(CodecFilter::Eac3);
    let observations = parser.push(&period);
    parser.finish().unwrap();

    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].burst.pd as usize, EAC3_MAX_PAYLOAD_BYTES);
    assert_eq!(observations[0].burst.payload, payload);
}

#[test]
fn one_byte_over_transport_payload_limit_fails_closed() {
    let payload = vec![0_u8; EAC3_MAX_PAYLOAD_BYTES + 1];
    let mut period = [0_u8; EAC3_BURST_PERIOD_BYTES];

    assert_eq!(
        write_eac3_period(&payload, &mut period),
        Err(SimSourceError::PayloadTooLarge {
            maximum: EAC3_MAX_PAYLOAD_BYTES,
            actual: EAC3_MAX_PAYLOAD_BYTES + 1,
        })
    );
}
