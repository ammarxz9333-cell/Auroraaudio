use aurora_iec61937::{BurstParser, CodecFilter};
use aurora_sim_source::{
    CarrierFault, EAC3_BURST_PERIOD_BYTES, inject_carrier_fault, write_eac3_period,
};

fn payload(seed: u8) -> Vec<u8> {
    (0..128)
        .map(|index| seed.wrapping_add((index as u8).wrapping_mul(19)))
        .collect()
}

#[test]
fn deleting_padding_word_makes_next_burst_arrive_early_without_changing_payloads() {
    let first_payload = payload(0x21);
    let second_payload = payload(0xA7);
    let mut first = [0_u8; EAC3_BURST_PERIOD_BYTES];
    let mut second = [0_u8; EAC3_BURST_PERIOD_BYTES];
    write_eac3_period(&first_payload, &mut first).unwrap();
    write_eac3_period(&second_payload, &mut second).unwrap();

    let mut carrier = Vec::with_capacity(EAC3_BURST_PERIOD_BYTES * 2);
    carrier.extend_from_slice(&first);
    carrier.extend_from_slice(&second);

    let damaged = inject_carrier_fault(
        &carrier,
        CarrierFault::DeleteBytes {
            offset: EAC3_BURST_PERIOD_BYTES - 2,
            count: 2,
        },
    )
    .unwrap();

    let mut parser = BurstParser::new(CodecFilter::Eac3);
    let observations = parser.push(&damaged);
    parser.finish().unwrap();

    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0].burst.payload, first_payload);
    assert_eq!(observations[1].burst.payload, second_payload);
    assert_eq!(observations[0].carrier_offset_bytes, 0);
    assert_eq!(
        observations[1].carrier_offset_bytes,
        (EAC3_BURST_PERIOD_BYTES - 2) as u64
    );
}
