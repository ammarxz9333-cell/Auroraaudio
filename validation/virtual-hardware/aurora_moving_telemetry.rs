use abi_stable::library::RootModule;
use bridge_api::{BridgeLibRef, REvent, RInputTransport};
use serde_json::json;
use spdif::SpdifParser;
use std::{collections::BTreeMap, env, fs, path::Path};

#[derive(Default, Debug)]
struct ObjectStats {
    event_count: u64,
    position_event_count: u64,
    position_change_count: u64,
    first_sample: Option<u64>,
    last_sample: Option<u64>,
    first_change_sample: Option<u64>,
    last_change_sample: Option<u64>,
    position_min: Option<[f64; 3]>,
    position_max: Option<[f64; 3]>,
    last_position: Option<[f64; 3]>,
}

impl ObjectStats {
    fn observe(&mut self, event: &REvent) {
        self.event_count += 1;
        self.first_sample.get_or_insert(event.sample_pos);
        self.last_sample = Some(event.sample_pos);
        if !event.has_pos {
            return;
        }
        self.position_event_count += 1;
        let pos = event.pos;
        match self.position_min.as_mut() {
            Some(minimum) => {
                for axis in 0..3 {
                    minimum[axis] = minimum[axis].min(pos[axis]);
                }
            }
            None => self.position_min = Some(pos),
        }
        match self.position_max.as_mut() {
            Some(maximum) => {
                for axis in 0..3 {
                    maximum[axis] = maximum[axis].max(pos[axis]);
                }
            }
            None => self.position_max = Some(pos),
        }
        if let Some(previous) = self.last_position {
            let changed = (0..3).any(|axis| (previous[axis] - pos[axis]).abs() > 1.0e-9);
            if changed {
                self.position_change_count += 1;
                self.first_change_sample.get_or_insert(event.sample_pos);
                self.last_change_sample = Some(event.sample_pos);
            }
        }
        self.last_position = Some(pos);
    }
}

fn main() {
    let mut args = env::args().skip(1);
    let bridge_path = args.next().expect("bridge path argument");
    let carrier_path = args.next().expect("IEC61937 carrier argument");
    let output_path = args.next().expect("telemetry JSON output argument");
    assert!(args.next().is_none(), "unexpected extra arguments");

    let lib = BridgeLibRef::load_from_file(Path::new(&bridge_path)).expect("load Harletty bridge");
    let mut bridge = (lib.new_bridge())(false);
    let carrier = fs::read(&carrier_path).expect("read IEC61937 carrier");
    let mut parser = SpdifParser::new();
    let mut packets = 0u64;
    let mut frames = 0u64;
    let mut total_samples = 0u64;
    let mut sample_rate: Option<u32> = None;
    let mut metadata_frames = 0u64;
    let mut events = 0u64;
    let mut object_channel_declarations = 0u64;
    let mut reset_count = 0u64;
    let mut metadata_positions_monotonic = true;
    let mut previous_metadata_sample: Option<u64> = None;
    let mut objects: BTreeMap<u32, ObjectStats> = BTreeMap::new();

    for chunk in carrier.chunks(997) {
        parser.push_bytes(chunk);
        while let Some(packet) = parser.get_next_packet() {
            packets += 1;
            assert_eq!(packet.data_type, 0x15, "non-E-AC-3 IEC61937 burst");
            let result = bridge.push_packet(
                packet.payload.as_slice().into(),
                RInputTransport::Iec61937,
                packet.data_type,
            );
            assert!(
                result.error_message.is_empty(),
                "bridge error after packet {packets}: {}",
                result.error_message.as_str()
            );
            if result.did_reset {
                reset_count += 1;
            }
            for frame in result.frames.iter() {
                frames += 1;
                total_samples += u64::from(frame.sample_count);
                match sample_rate {
                    Some(rate) => assert_eq!(rate, frame.sampling_frequency, "sample rate changed mid-stream"),
                    None => sample_rate = Some(frame.sampling_frequency),
                }
                for meta in frame.metadata.iter() {
                    metadata_frames += 1;
                    if let Some(previous) = previous_metadata_sample {
                        if meta.sample_pos < previous {
                            metadata_positions_monotonic = false;
                        }
                    }
                    previous_metadata_sample = Some(meta.sample_pos);
                    object_channel_declarations += meta.object_channels.len() as u64;
                    events += meta.events.len() as u64;
                    for event in meta.events.iter() {
                        objects.entry(event.id).or_default().observe(event);
                    }
                }
            }
        }
    }

    assert!(packets > 0, "no IEC61937 packets parsed");
    assert!(frames > 0, "Harletty emitted no decoded frames");
    assert!(bridge.is_ready(), "bridge never became ready");
    assert!(bridge.has_objects(), "bridge does not report dynamic objects at end of moving carrier");
    assert!(metadata_frames > 0 && events > 0, "moving carrier emitted no object metadata");
    assert!(metadata_positions_monotonic, "metadata sample positions regressed");

    let object_values: Vec<_> = objects
        .iter()
        .map(|(id, stats)| {
            json!({
                "id": id,
                "event_count": stats.event_count,
                "position_event_count": stats.position_event_count,
                "position_change_count": stats.position_change_count,
                "first_sample": stats.first_sample,
                "last_sample": stats.last_sample,
                "first_change_sample": stats.first_change_sample,
                "last_change_sample": stats.last_change_sample,
                "position_min": stats.position_min,
                "position_max": stats.position_max,
            })
        })
        .collect();
    let varying_count = objects
        .values()
        .filter(|stats| stats.position_change_count > 0)
        .count();
    assert!(varying_count > 0, "Harletty metadata contained no changing object position");

    let payload = json!({
        "schema_version": 1,
        "source": "Aurora pinned Harletty bridge via IEC61937",
        "transport": {
            "data_type": 0x15,
            "packets": packets,
        },
        "decode": {
            "frames": frames,
            "sample_rate_hz": sample_rate.unwrap_or(0),
            "total_samples": total_samples,
            "reset_count": reset_count,
            "bridge_ready": bridge.is_ready(),
            "bridge_has_objects": bridge.has_objects(),
        },
        "metadata": {
            "metadata_frames": metadata_frames,
            "events": events,
            "object_channel_declarations": object_channel_declarations,
            "sample_positions_monotonic": metadata_positions_monotonic,
            "object_count": objects.len(),
            "position_varying_object_count": varying_count,
            "objects": object_values,
        },
    });
    fs::write(&output_path, serde_json::to_vec_pretty(&payload).expect("serialize telemetry"))
        .expect("write telemetry JSON");
    println!(
        "AURORA-MOVING-BRIDGE-PASS packets={packets} frames={frames} metadata_frames={metadata_frames} events={events} objects={} varying_objects={varying_count} total_samples={total_samples}",
        objects.len()
    );
}
