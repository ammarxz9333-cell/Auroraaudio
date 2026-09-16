//! Fixed-size six-stream AVDECC session gate for Aurora immersive output.
//!
//! The manager consumes sanitized GenAVB AVDECC CONNECT/DISCONNECT events and
//! establishes exactly one stereo stream for each canonical Aurora 7.1.4 pair.
//! It performs no network I/O and allocates no memory after construction.
//! Playback code must call `require_complete()` before starting the six-stream
//! immersive transport set; partial speaker sets are never accepted silently.

use aurora_network_genavb_avdecc::{GenAvbAvdeccEvent, GenAvbAvdeccEventKind};
use core::fmt;

pub const IMMERSIVE_STREAM_COUNT: usize = 6;
const REQUIRED_SAMPLE_RATE_HZ: u32 = 48_000;
const REQUIRED_CHANNELS: u32 = 2;
const REQUIRED_BIT_DEPTH: u32 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StereoEndpointRole {
    Front,
    CenterLfe,
    Surround,
    BackSurround,
    TopFront,
    TopRear,
}

impl StereoEndpointRole {
    const ALL: [Self; IMMERSIVE_STREAM_COUNT] = [
        Self::Front,
        Self::CenterLfe,
        Self::Surround,
        Self::BackSurround,
        Self::TopFront,
        Self::TopRear,
    ];

    const fn slot(self) -> usize {
        match self {
            Self::Front => 0,
            Self::CenterLfe => 1,
            Self::Surround => 2,
            Self::BackSurround => 3,
            Self::TopFront => 4,
            Self::TopRear => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedStream {
    pub role: StereoEndpointRole,
    /// AVDECC Stream Output descriptor index expected for this speaker pair.
    pub stream_index: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamIdentity {
    pub stream_id: [u8; 8],
    pub destination_mac: [u8; 6],
    pub port: u16,
    pub stream_class: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectedStream {
    pub role: StereoEndpointRole,
    pub stream_index: u16,
    pub identity: StreamIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionUpdate {
    pub role: StereoEndpointRole,
    pub connected: bool,
    pub complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    DuplicateExpectedRole(StereoEndpointRole),
    DuplicateExpectedIndex(u16),
    MissingExpectedRole(StereoEndpointRole),
    UnknownStreamIndex(u16),
    DuplicateConnect(u16),
    DuplicateStreamId([u8; 8]),
    DuplicateDestinationMac([u8; 6]),
    InvalidStreamIdentity,
    InvalidMediaContract {
        sample_rate_hz: u32,
        channels: u32,
        bit_depth: u32,
    },
    DisconnectNotConnected(u16),
    DisconnectIdentityMismatch(u16),
    PartialConnectionSet {
        connected: usize,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateExpectedRole(role) => {
                write!(formatter, "duplicate expected AVDECC role {role:?}")
            }
            Self::DuplicateExpectedIndex(index) => {
                write!(formatter, "duplicate expected AVDECC stream index {index}")
            }
            Self::MissingExpectedRole(role) => {
                write!(formatter, "missing expected AVDECC role {role:?}")
            }
            Self::UnknownStreamIndex(index) => {
                write!(formatter, "unknown AVDECC stream index {index}")
            }
            Self::DuplicateConnect(index) => {
                write!(formatter, "duplicate CONNECT for AVDECC stream index {index}")
            }
            Self::DuplicateStreamId(id) => write!(formatter, "duplicate AVTP stream id {id:02x?}"),
            Self::DuplicateDestinationMac(mac) => {
                write!(formatter, "duplicate AVTP destination MAC {mac:02x?}")
            }
            Self::InvalidStreamIdentity => formatter.write_str("invalid zero AVTP stream identity"),
            Self::InvalidMediaContract {
                sample_rate_hz,
                channels,
                bit_depth,
            } => write!(
                formatter,
                "unsupported AVDECC media contract {sample_rate_hz} Hz / {channels} ch / {bit_depth} bit"
            ),
            Self::DisconnectNotConnected(index) => {
                write!(formatter, "DISCONNECT for inactive AVDECC stream index {index}")
            }
            Self::DisconnectIdentityMismatch(index) => {
                write!(formatter, "DISCONNECT identity mismatch for stream index {index}")
            }
            Self::PartialConnectionSet { connected } => write!(
                formatter,
                "immersive AVDECC session is partial: {connected}/{IMMERSIVE_STREAM_COUNT} streams connected"
            ),
        }
    }
}

impl std::error::Error for SessionError {}

pub struct SixStreamAvdeccSession {
    expected: [ExpectedStream; IMMERSIVE_STREAM_COUNT],
    connected: [Option<ConnectedStream>; IMMERSIVE_STREAM_COUNT],
    connected_count: usize,
}

impl SixStreamAvdeccSession {
    pub fn new(expected: [ExpectedStream; IMMERSIVE_STREAM_COUNT]) -> Result<Self, SessionError> {
        let mut role_seen = [false; IMMERSIVE_STREAM_COUNT];
        for (position, item) in expected.iter().enumerate() {
            let slot = item.role.slot();
            if role_seen[slot] {
                return Err(SessionError::DuplicateExpectedRole(item.role));
            }
            role_seen[slot] = true;
            for previous in &expected[..position] {
                if previous.stream_index == item.stream_index {
                    return Err(SessionError::DuplicateExpectedIndex(item.stream_index));
                }
            }
        }
        for role in StereoEndpointRole::ALL {
            if !role_seen[role.slot()] {
                return Err(SessionError::MissingExpectedRole(role));
            }
        }

        Ok(Self {
            expected,
            connected: [None; IMMERSIVE_STREAM_COUNT],
            connected_count: 0,
        })
    }

    pub fn apply(&mut self, event: GenAvbAvdeccEvent) -> Result<SessionUpdate, SessionError> {
        match event.kind {
            GenAvbAvdeccEventKind::Connect => self.connect(event),
            GenAvbAvdeccEventKind::Disconnect => self.disconnect(event),
        }
    }

    pub const fn connected_count(&self) -> usize {
        self.connected_count
    }

    pub const fn is_complete(&self) -> bool {
        self.connected_count == IMMERSIVE_STREAM_COUNT
    }

    /// Returns the six connected streams in canonical Aurora stereo-pair order.
    /// This is the explicit start gate for immersive playback.
    pub fn require_complete(
        &self,
    ) -> Result<[ConnectedStream; IMMERSIVE_STREAM_COUNT], SessionError> {
        if !self.is_complete() {
            return Err(SessionError::PartialConnectionSet {
                connected: self.connected_count,
            });
        }
        Ok([
            self.connected[0].expect("complete count guarantees slot 0"),
            self.connected[1].expect("complete count guarantees slot 1"),
            self.connected[2].expect("complete count guarantees slot 2"),
            self.connected[3].expect("complete count guarantees slot 3"),
            self.connected[4].expect("complete count guarantees slot 4"),
            self.connected[5].expect("complete count guarantees slot 5"),
        ])
    }

    pub fn reset(&mut self) {
        self.connected = [None; IMMERSIVE_STREAM_COUNT];
        self.connected_count = 0;
    }

    fn connect(&mut self, event: GenAvbAvdeccEvent) -> Result<SessionUpdate, SessionError> {
        if event.sample_rate_hz != REQUIRED_SAMPLE_RATE_HZ
            || event.channels != REQUIRED_CHANNELS
            || event.bit_depth != REQUIRED_BIT_DEPTH
        {
            return Err(SessionError::InvalidMediaContract {
                sample_rate_hz: event.sample_rate_hz,
                channels: event.channels,
                bit_depth: event.bit_depth,
            });
        }
        if event.stream_id == [0; 8] || event.destination_mac == [0; 6] {
            return Err(SessionError::InvalidStreamIdentity);
        }

        let role = self.role_for_index(event.stream_index)?;
        let slot = role.slot();
        if self.connected[slot].is_some() {
            return Err(SessionError::DuplicateConnect(event.stream_index));
        }
        for existing in self.connected.iter().flatten() {
            if existing.identity.stream_id == event.stream_id {
                return Err(SessionError::DuplicateStreamId(event.stream_id));
            }
            if existing.identity.destination_mac == event.destination_mac {
                return Err(SessionError::DuplicateDestinationMac(event.destination_mac));
            }
        }

        self.connected[slot] = Some(ConnectedStream {
            role,
            stream_index: event.stream_index,
            identity: StreamIdentity {
                stream_id: event.stream_id,
                destination_mac: event.destination_mac,
                port: event.port,
                stream_class: event.stream_class,
            },
        });
        self.connected_count += 1;

        Ok(SessionUpdate {
            role,
            connected: true,
            complete: self.is_complete(),
        })
    }

    fn disconnect(&mut self, event: GenAvbAvdeccEvent) -> Result<SessionUpdate, SessionError> {
        let role = self.role_for_index(event.stream_index)?;
        let slot = role.slot();
        let active =
            self.connected[slot].ok_or(SessionError::DisconnectNotConnected(event.stream_index))?;
        if active.identity.stream_id != event.stream_id
            || active.identity.port != event.port
            || active.identity.stream_class != event.stream_class
        {
            return Err(SessionError::DisconnectIdentityMismatch(event.stream_index));
        }

        self.connected[slot] = None;
        self.connected_count -= 1;
        Ok(SessionUpdate {
            role,
            connected: false,
            complete: false,
        })
    }

    fn role_for_index(&self, stream_index: u16) -> Result<StereoEndpointRole, SessionError> {
        self.expected
            .iter()
            .find(|item| item.stream_index == stream_index)
            .map(|item| item.role)
            .ok_or(SessionError::UnknownStreamIndex(stream_index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping() -> [ExpectedStream; IMMERSIVE_STREAM_COUNT] {
        [
            ExpectedStream {
                role: StereoEndpointRole::Front,
                stream_index: 10,
            },
            ExpectedStream {
                role: StereoEndpointRole::CenterLfe,
                stream_index: 11,
            },
            ExpectedStream {
                role: StereoEndpointRole::Surround,
                stream_index: 12,
            },
            ExpectedStream {
                role: StereoEndpointRole::BackSurround,
                stream_index: 13,
            },
            ExpectedStream {
                role: StereoEndpointRole::TopFront,
                stream_index: 14,
            },
            ExpectedStream {
                role: StereoEndpointRole::TopRear,
                stream_index: 15,
            },
        ]
    }

    fn connect(index: u16, identity: u8) -> GenAvbAvdeccEvent {
        GenAvbAvdeccEvent {
            kind: GenAvbAvdeccEventKind::Connect,
            stream_index: index,
            port: 0,
            direction: 0,
            stream_class: 1,
            stream_id: [0, 1, 2, 3, 4, 5, 6, identity],
            destination_mac: [0x91, 0xe0, 0xf0, 0, 0, identity],
            sample_rate_hz: 48_000,
            channels: 2,
            bit_depth: 24,
        }
    }

    fn disconnect(from: GenAvbAvdeccEvent) -> GenAvbAvdeccEvent {
        GenAvbAvdeccEvent {
            kind: GenAvbAvdeccEventKind::Disconnect,
            destination_mac: [0; 6],
            sample_rate_hz: 0,
            channels: 0,
            bit_depth: 0,
            ..from
        }
    }

    #[test]
    fn six_unique_connections_are_required_before_start_gate_opens() {
        let mut session = SixStreamAvdeccSession::new(mapping()).unwrap();
        for (offset, index) in (10_u16..=15).enumerate() {
            let update = session.apply(connect(index, (offset + 1) as u8)).unwrap();
            assert_eq!(update.complete, index == 15);
        }
        assert!(session.is_complete());
        let streams = session.require_complete().unwrap();
        assert_eq!(streams[0].role, StereoEndpointRole::Front);
        assert_eq!(streams[5].role, StereoEndpointRole::TopRear);
    }

    #[test]
    fn partial_connection_set_never_passes_start_gate() {
        let mut session = SixStreamAvdeccSession::new(mapping()).unwrap();
        session.apply(connect(10, 1)).unwrap();
        assert_eq!(
            session.require_complete(),
            Err(SessionError::PartialConnectionSet { connected: 1 })
        );
    }

    #[test]
    fn duplicate_stream_id_and_destination_mac_fail_closed() {
        let mut session = SixStreamAvdeccSession::new(mapping()).unwrap();
        session.apply(connect(10, 1)).unwrap();

        let mut duplicate_id = connect(11, 2);
        duplicate_id.stream_id = connect(10, 1).stream_id;
        assert_eq!(
            session.apply(duplicate_id),
            Err(SessionError::DuplicateStreamId(duplicate_id.stream_id))
        );

        let mut duplicate_mac = connect(11, 2);
        duplicate_mac.destination_mac = connect(10, 1).destination_mac;
        assert_eq!(
            session.apply(duplicate_mac),
            Err(SessionError::DuplicateDestinationMac(
                duplicate_mac.destination_mac
            ))
        );
    }

    #[test]
    fn unknown_and_duplicate_indices_fail_closed() {
        let mut session = SixStreamAvdeccSession::new(mapping()).unwrap();
        assert_eq!(
            session.apply(connect(99, 1)),
            Err(SessionError::UnknownStreamIndex(99))
        );
        session.apply(connect(10, 1)).unwrap();
        assert_eq!(
            session.apply(connect(10, 2)),
            Err(SessionError::DuplicateConnect(10))
        );
    }

    #[test]
    fn disconnect_invalidates_complete_session_and_checks_identity() {
        let mut session = SixStreamAvdeccSession::new(mapping()).unwrap();
        let mut events = [connect(10, 1); IMMERSIVE_STREAM_COUNT];
        for (offset, index) in (10_u16..=15).enumerate() {
            events[offset] = connect(index, (offset + 1) as u8);
            session.apply(events[offset]).unwrap();
        }
        assert!(session.is_complete());

        let mut wrong = disconnect(events[3]);
        wrong.stream_id[7] = 99;
        assert_eq!(
            session.apply(wrong),
            Err(SessionError::DisconnectIdentityMismatch(13))
        );
        assert!(session.is_complete());

        let update = session.apply(disconnect(events[3])).unwrap();
        assert_eq!(update.role, StereoEndpointRole::BackSurround);
        assert!(!update.complete);
        assert!(!session.is_complete());
        assert_eq!(session.connected_count(), 5);
    }

    #[test]
    fn invalid_media_contract_fails_closed() {
        let mut session = SixStreamAvdeccSession::new(mapping()).unwrap();
        let mut event = connect(10, 1);
        event.channels = 1;
        assert_eq!(
            session.apply(event),
            Err(SessionError::InvalidMediaContract {
                sample_rate_hz: 48_000,
                channels: 1,
                bit_depth: 24,
            })
        );
    }

    #[test]
    fn reset_closes_start_gate_and_clears_all_connections() {
        let mut session = SixStreamAvdeccSession::new(mapping()).unwrap();
        for (offset, index) in (10_u16..=15).enumerate() {
            session.apply(connect(index, (offset + 1) as u8)).unwrap();
        }
        assert!(session.is_complete());

        session.reset();

        assert_eq!(session.connected_count(), 0);
        assert!(!session.is_complete());
        assert_eq!(
            session.require_complete(),
            Err(SessionError::PartialConnectionSet { connected: 0 })
        );
    }

    #[test]
    fn expected_mapping_rejects_duplicate_roles_and_indices() {
        let mut duplicate_role = mapping();
        duplicate_role[1].role = StereoEndpointRole::Front;
        assert!(matches!(
            SixStreamAvdeccSession::new(duplicate_role),
            Err(SessionError::DuplicateExpectedRole(
                StereoEndpointRole::Front
            ))
        ));

        let mut duplicate_index = mapping();
        duplicate_index[1].stream_index = duplicate_index[0].stream_index;
        assert_eq!(
            SixStreamAvdeccSession::new(duplicate_index).err(),
            Some(SessionError::DuplicateExpectedIndex(10))
        );
    }
}
