use openjoc_api::{OpenJocConfig, OpenJocSession};
use openjoc_scene::{SpeakerGeometry, SpeakerLayout};

fn custom_sixteen_channel_layout() -> SpeakerLayout {
    SpeakerLayout::custom(
        "aurora-custom-16-validation",
        vec![
            SpeakerGeometry::full_range("FL", -30.0, 0.0),
            SpeakerGeometry::full_range("FR", 30.0, 0.0),
            SpeakerGeometry::full_range("FC", 0.0, 0.0),
            SpeakerGeometry::lfe("LFE", 0.0, -30.0),
            SpeakerGeometry::full_range("Lb", -150.0, 0.0),
            SpeakerGeometry::full_range("Rb", 150.0, 0.0),
            SpeakerGeometry::full_range("Ls", -90.0, 0.0),
            SpeakerGeometry::full_range("Rs", 90.0, 0.0),
            SpeakerGeometry::full_range("Lw", -60.0, 0.0),
            SpeakerGeometry::full_range("Rw", 60.0, 0.0),
            SpeakerGeometry::full_range("Bc", 180.0, 0.0),
            SpeakerGeometry::full_range("TFL", -30.0, 45.0),
            SpeakerGeometry::full_range("TFR", 30.0, 45.0),
            SpeakerGeometry::full_range("TML", -90.0, 45.0),
            SpeakerGeometry::full_range("TMR", 90.0, 45.0),
            SpeakerGeometry::full_range("TRC", 180.0, 45.0),
        ],
    )
    .expect("validation geometry must satisfy the pinned OpenJOC layout contract")
}

#[test]
fn pinned_openjoc_accepts_explicit_sixteen_channel_geometry() {
    let layout = custom_sixteen_channel_layout();
    let config = OpenJocConfig::default().with_speaker_layout(layout);
    let session = OpenJocSession::new(config).expect("create OpenJOC custom speaker session");
    let info = session.output_info();
    let labels = info
        .channel_labels
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();

    assert_eq!(info.layout_name, "aurora-custom-16-validation");
    assert_eq!(info.channel_count, 16);
    assert_eq!(
        labels,
        vec![
            "FL", "FR", "FC", "LFE", "Lb", "Rb", "Ls", "Rs", "Lw", "Rw", "Bc", "TFL",
            "TFR", "TML", "TMR", "TRC",
        ]
    );
}
