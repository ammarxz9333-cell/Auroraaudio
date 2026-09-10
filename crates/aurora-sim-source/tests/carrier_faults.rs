use aurora_iec61937::{BurstFinishError, BurstParser, CodecFilter};
use aurora_sim_source::{
    CarrierFault, EAC3_BURST_PERIOD_BYTES, EAC3_CARRIER_BYTES_PER_MS, inject_carrier_fault,
    write_eac3_period, write_idle_period,
};

fn payload(seed: u8) -> Vec<u8> {
    (0..128)
        .map(|index| seed.wrapping_add((index as u8).wrapping_mul(19)))
        .collect()
}

#[test]
fn corrupt_pa_drops_only_the_damaged_burst_and_resyncs_at_the_next_period() {
    let first_payload = payload(0x11);
    let second_payload = payload(0x22);
    let mut first = [0_u8; EAC3_BURST_PERIOD_BYTES];
    let mut second = [0_u8; EAC3_BURST_PERIOD_BYTES];
    write_eac3_period(&first_payload, &mut first).unwrap();
    write_eac3_period(&second_payload, &mut second).unwrap();
    let damaged = inject_carrier_fault(&first, CarrierFault::CorruptPa).unwrap();

    let mut carrier = damaged;
    carrier.extend_from_slice(&second);
    let mut parser = BurstParser::new(CodecFilter::Eac3);
    let observations = parser.push(&carrier);
    parser.finish().unwrap();

    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].burst.payload, second_payload);
    assert_eq!(
        observations[0].carrier_offset_bytes,
        EAC3_BURST_PERIOD_BYTES as u64
    );
}

#[test]
fn dropped_burst_gap_preserves_wall_time_and_resynchronizes_after_idle_periods() {
    let first_payload = payload(0x31);
    let second_payload = payload(0x42);
    let mut first = [0_u8; EAC3_BURST_PERIOD_BYTES];
    let mut second = [0_u8; EAC3_BURST_PERIOD_BYTES];
    let mut idle = [0_u8; EAC3_BURST_PERIOD_BYTES];
    write_eac3_period(&first_payload, &mut first).unwrap();
    write_eac3_period(&second_payload, &mut second).unwrap();
    write_idle_period(&mut idle).unwrap();

    let mut carrier = Vec::with_capacity(EAC3_BURST_PERIOD_BYTES * 4);
    carrier.extend_from_slice(&first);
    carrier.extend_from_slice(&idle);
    carrier.extend_from_slice(&idle);
    carrier.extend_from_slice(&second);

    let mut parser = BurstParser::new(CodecFilter::Eac3);
    let observations = parser.push(&carrier);
    parser.finish().unwrap();

    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0].burst.payload, first_payload);
    assert_eq!(observations[1].burst.payload, second_payload);
    assert_eq!(
        observations[1].carrier_offset_bytes,
        (EAC3_BURST_PERIOD_BYTES * 3) as u64
    );
}

#[test]
fn parser_visible_codec_switch_idle_span_returns_cleanly_to_eac3() {
    let first_payload = payload(0x53);
    let second_payload = payload(0x64);
    let mut first = [0_u8; EAC3_BURST_PERIOD_BYTES];
    let mut second = [0_u8; EAC3_BURST_PERIOD_BYTES];
    let mut lpcm_visible_idle = [0_u8; EAC3_BURST_PERIOD_BYTES];
    write_eac3_period(&first_payload, &mut first).unwrap();
    write_eac3_period(&second_payload, &mut second).unwrap();
    write_idle_period(&mut lpcm_visible_idle).unwrap();

    let mut carrier = Vec::with_capacity(EAC3_BURST_PERIOD_BYTES * 3);
    carrier.extend_from_slice(&first);
    carrier.extend_from_slice(&lpcm_visible_idle);
    carrier.extend_from_slice(&second);

    let mut parser = BurstParser::new(CodecFilter::Eac3);
    let observations = parser.push(&carrier);
    parser.finish().unwrap();

    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0].burst.payload, first_payload);
    assert_eq!(observations[1].burst.payload, second_payload);
    assert_eq!(
        observations[1].carrier_offset_bytes,
        (EAC3_BURST_PERIOD_BYTES * 2) as u64
    );
}

#[test]
fn deterministic_cadence_jitter_changes_only_pa_spacing() {
    let first_payload = payload(0x75);
    let second_payload = payload(0x86);
    let mut first = [0_u8; EAC3_BURST_PERIOD_BYTES];
    let mut second = [0_u8; EAC3_BURST_PERIOD_BYTES];
    write_eac3_period(&first_payload, &mut first).unwrap();
    write_eac3_period(&second_payload, &mut second).unwrap();

    let jitter_bytes = EAC3_CARRIER_BYTES_PER_MS * 3;
    let mut carrier = Vec::with_capacity(EAC3_BURST_PERIOD_BYTES * 2 + jitter_bytes);
    carrier.extend_from_slice(&first);
    carrier.resize(carrier.len() + jitter_bytes, 0);
    carrier.extend_from_slice(&second);

    let mut parser = BurstParser::new(CodecFilter::Eac3);
    let observations = parser.push(&carrier);
    parser.finish().unwrap();

    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0].burst.payload, first_payload);
    assert_eq!(observations[1].burst.payload, second_payload);
    assert_eq!(
        observations[1].carrier_offset_bytes,
        (EAC3_BURST_PERIOD_BYTES + jitter_bytes) as u64
    );
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

#[test]
fn cut_burst_at_finite_eof_is_reported_as_truncated_payload() {
    let encoded = payload(0x97);
    let mut period = [0_u8; EAC3_BURST_PERIOD_BYTES];
    write_eac3_period(&encoded, &mut period).unwrap();
    let cut = inject_carrier_fault(
        &period,
        CarrierFault::Truncate {
            length: 8 + encoded.len() / 2,
        },
    )
    .unwrap();

    let mut parser = BurstParser::new(CodecFilter::Eac3);
    assert!(parser.push(&cut).is_empty());
    assert!(matches!(
        parser.finish(),
        Err(BurstFinishError::TruncatedPayload { .. })
    ));
}

#[test]
fn truncated_eof_never_becomes_a_complete_observation() {
    let encoded = payload(0xA8);
    let mut period = [0_u8; EAC3_BURST_PERIOD_BYTES];
    write_eac3_period(&encoded, &mut period).unwrap();
    let truncated = inject_carrier_fault(
        &period,
        CarrierFault::Truncate {
            length: 8 + encoded.len() - 4,
        },
    )
    .unwrap();

    let mut parser = BurstParser::new(CodecFilter::Eac3);
    assert!(parser.push(&truncated).is_empty());
    let error = parser.finish().unwrap_err();
    assert!(matches!(error, BurstFinishError::TruncatedPayload { .. }));
}
