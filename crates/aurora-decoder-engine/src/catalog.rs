use aurora_decoder_open::OpenCodecKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodecId {
    Pcm,
    Ac3,
    Eac3,
    Eac3Joc,
    Ac4,
    TrueHd,
    TrueHdAtmos,
    Mlp,
    DolbyMat,
    Dts,
    DtsHd,
    DtsX,
    Aac,
    Flac,
    Opus,
    Vorbis,
    Speex,
    Mp3,
    Alac,
    WavPack,
    MonkeyAudio,
    Tta,
    Musepack,
    AmrNb,
    AmrWb,
    Sbc,
    Iamf,
    MpegH3d,
    Unknown,
}

impl From<OpenCodecKind> for CodecId {
    fn from(value: OpenCodecKind) -> Self {
        match value {
            OpenCodecKind::Pcm => Self::Pcm,
            OpenCodecKind::Ac3 => Self::Ac3,
            OpenCodecKind::Eac3 => Self::Eac3,
            OpenCodecKind::Eac3Joc => Self::Eac3Joc,
            OpenCodecKind::TrueHd => Self::TrueHd,
            OpenCodecKind::Mlp => Self::Mlp,
            OpenCodecKind::DolbyMat => Self::DolbyMat,
            OpenCodecKind::Dts => Self::Dts,
            OpenCodecKind::DtsHd => Self::DtsHd,
            OpenCodecKind::AacAdts | OpenCodecKind::AacLatm => Self::Aac,
            OpenCodecKind::Flac => Self::Flac,
            OpenCodecKind::Opus => Self::Opus,
            OpenCodecKind::Vorbis => Self::Vorbis,
            OpenCodecKind::Speex => Self::Speex,
            OpenCodecKind::Mp3 => Self::Mp3,
            OpenCodecKind::Alac => Self::Alac,
            OpenCodecKind::WavPack => Self::WavPack,
            OpenCodecKind::MonkeyAudio => Self::MonkeyAudio,
            OpenCodecKind::Tta => Self::Tta,
            OpenCodecKind::Musepack => Self::Musepack,
            OpenCodecKind::AmrNb => Self::AmrNb,
            OpenCodecKind::AmrWb => Self::AmrWb,
            OpenCodecKind::Sbc => Self::Sbc,
            OpenCodecKind::OggUnknown | OpenCodecKind::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendId {
    OpenJoc,
    OxideAc3,
    OxideAc4,
    TrueHdNative,
    OxideDtsCore,
    OxideAac,
    OxideOpus,
    IamfTools,
    Symphonia,
    FfmpegWorker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionClass {
    NativeRust,
    NativeLibrary,
    ExternalWorker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseClass {
    ProprietaryCompatiblePermissive,
    FileLevelCopyleft,
    WeakCopyleftExternal,
    ReviewRequired,
}

/// Static policy metadata for a backend.
///
/// The numeric fields are initial routing weights (0..=100), not published
/// quality measurements. A later evidence pipeline will replace heuristic
/// weights with measured conformance, fuzz, latency and metadata scores.
#[derive(Debug, Clone, Copy)]
pub struct BackendDescriptor {
    pub id: BackendId,
    pub name: &'static str,
    pub execution: ExecutionClass,
    pub license: LicenseClass,
    pub integrated: bool,
    pub closed_core_compatible: bool,
    pub object_metadata: bool,
    pub max_channels: u8,
    pub quality: u8,
    pub robustness: u8,
    pub latency: u8,
    pub metadata_fidelity: u8,
    pub portability: u8,
    pub codecs: &'static [CodecId],
}

impl BackendDescriptor {
    pub fn supports(&self, codec: CodecId) -> bool {
        self.codecs.contains(&codec)
    }
}

const OPENJOC_CODECS: &[CodecId] = &[CodecId::Eac3Joc];
const OXIDE_AC3_CODECS: &[CodecId] = &[CodecId::Ac3, CodecId::Eac3];
const OXIDE_AC4_CODECS: &[CodecId] = &[CodecId::Ac4];
const TRUEHD_CODECS: &[CodecId] = &[CodecId::TrueHd, CodecId::TrueHdAtmos];
const OXIDE_DTS_CODECS: &[CodecId] = &[CodecId::Dts];
const OXIDE_AAC_CODECS: &[CodecId] = &[CodecId::Aac];
const OXIDE_OPUS_CODECS: &[CodecId] = &[CodecId::Opus];
const IAMF_CODECS: &[CodecId] = &[CodecId::Iamf];
const SYMPHONIA_CODECS: &[CodecId] = &[
    CodecId::Aac,
    CodecId::Flac,
    CodecId::Mp3,
    CodecId::Alac,
    CodecId::Vorbis,
    CodecId::Pcm,
];
const FFMPEG_CODECS: &[CodecId] = &[
    CodecId::Pcm,
    CodecId::Ac3,
    CodecId::Eac3,
    CodecId::TrueHd,
    CodecId::Mlp,
    CodecId::Dts,
    CodecId::DtsHd,
    CodecId::Aac,
    CodecId::Flac,
    CodecId::Opus,
    CodecId::Vorbis,
    CodecId::Speex,
    CodecId::Mp3,
    CodecId::Alac,
    CodecId::WavPack,
    CodecId::MonkeyAudio,
    CodecId::Tta,
    CodecId::Musepack,
    CodecId::AmrNb,
    CodecId::AmrWb,
    CodecId::Sbc,
];

const BACKENDS: &[BackendDescriptor] = &[
    BackendDescriptor {
        id: BackendId::OpenJoc,
        name: "OpenJOC native JOC renderer",
        execution: ExecutionClass::NativeRust,
        license: LicenseClass::ProprietaryCompatiblePermissive,
        integrated: true,
        closed_core_compatible: true,
        object_metadata: true,
        max_channels: 64,
        quality: 98,
        robustness: 90,
        latency: 94,
        metadata_fidelity: 100,
        portability: 96,
        codecs: OPENJOC_CODECS,
    },
    BackendDescriptor {
        id: BackendId::OxideAc3,
        name: "OxideAV AC-3/E-AC-3",
        execution: ExecutionClass::NativeRust,
        license: LicenseClass::ProprietaryCompatiblePermissive,
        integrated: true,
        closed_core_compatible: true,
        object_metadata: false,
        max_channels: 8,
        quality: 96,
        robustness: 93,
        latency: 96,
        metadata_fidelity: 72,
        portability: 99,
        codecs: OXIDE_AC3_CODECS,
    },
    BackendDescriptor {
        id: BackendId::OxideAc4,
        name: "OxideAV AC-4",
        execution: ExecutionClass::NativeRust,
        license: LicenseClass::ProprietaryCompatiblePermissive,
        integrated: true,
        closed_core_compatible: true,
        object_metadata: false,
        max_channels: 24,
        quality: 88,
        robustness: 82,
        latency: 95,
        metadata_fidelity: 80,
        portability: 99,
        codecs: OXIDE_AC4_CODECS,
    },
    BackendDescriptor {
        id: BackendId::TrueHdNative,
        name: "truehdd native TrueHD",
        execution: ExecutionClass::NativeRust,
        license: LicenseClass::ProprietaryCompatiblePermissive,
        integrated: false,
        closed_core_compatible: true,
        object_metadata: true,
        max_channels: 16,
        quality: 94,
        robustness: 84,
        latency: 92,
        metadata_fidelity: 96,
        portability: 95,
        codecs: TRUEHD_CODECS,
    },
    BackendDescriptor {
        id: BackendId::OxideDtsCore,
        name: "OxideAV DTS Core",
        execution: ExecutionClass::NativeRust,
        license: LicenseClass::ProprietaryCompatiblePermissive,
        integrated: true,
        closed_core_compatible: true,
        object_metadata: false,
        max_channels: 6,
        quality: 96,
        robustness: 91,
        latency: 95,
        metadata_fidelity: 70,
        portability: 99,
        codecs: OXIDE_DTS_CODECS,
    },
    BackendDescriptor {
        id: BackendId::OxideAac,
        name: "OxideAV AAC",
        execution: ExecutionClass::NativeRust,
        license: LicenseClass::ProprietaryCompatiblePermissive,
        integrated: false,
        closed_core_compatible: true,
        object_metadata: false,
        max_channels: 8,
        quality: 92,
        robustness: 88,
        latency: 96,
        metadata_fidelity: 70,
        portability: 99,
        codecs: OXIDE_AAC_CODECS,
    },
    BackendDescriptor {
        id: BackendId::OxideOpus,
        name: "OxideAV Opus",
        execution: ExecutionClass::NativeRust,
        license: LicenseClass::ProprietaryCompatiblePermissive,
        integrated: false,
        closed_core_compatible: true,
        object_metadata: false,
        max_channels: 8,
        quality: 93,
        robustness: 90,
        latency: 98,
        metadata_fidelity: 70,
        portability: 99,
        codecs: OXIDE_OPUS_CODECS,
    },
    BackendDescriptor {
        id: BackendId::IamfTools,
        name: "AOM IAMF tools",
        execution: ExecutionClass::NativeLibrary,
        license: LicenseClass::ReviewRequired,
        integrated: false,
        closed_core_compatible: true,
        object_metadata: true,
        max_channels: 64,
        quality: 96,
        robustness: 88,
        latency: 85,
        metadata_fidelity: 99,
        portability: 84,
        codecs: IAMF_CODECS,
    },
    BackendDescriptor {
        id: BackendId::Symphonia,
        name: "Symphonia",
        execution: ExecutionClass::NativeRust,
        license: LicenseClass::FileLevelCopyleft,
        integrated: false,
        closed_core_compatible: true,
        object_metadata: false,
        max_channels: 8,
        quality: 91,
        robustness: 92,
        latency: 94,
        metadata_fidelity: 68,
        portability: 99,
        codecs: SYMPHONIA_CODECS,
    },
    BackendDescriptor {
        id: BackendId::FfmpegWorker,
        name: "FFmpeg compatibility worker",
        execution: ExecutionClass::ExternalWorker,
        license: LicenseClass::WeakCopyleftExternal,
        integrated: true,
        closed_core_compatible: true,
        object_metadata: false,
        max_channels: 8,
        quality: 97,
        robustness: 99,
        latency: 75,
        metadata_fidelity: 65,
        portability: 88,
        codecs: FFMPEG_CODECS,
    },
];

#[derive(Debug, Default)]
pub struct DecoderCatalog;

impl DecoderCatalog {
    pub fn all(&self) -> &'static [BackendDescriptor] {
        BACKENDS
    }

    pub fn candidates(&self, codec: CodecId) -> impl Iterator<Item = &'static BackendDescriptor> {
        BACKENDS.iter().filter(move |backend| backend.supports(codec))
    }
}
