//! Experimental, explicit motor command/feedback wire contract.
//! The motor firmware MUST report a real position sensor and travel-limit
//! checks. Merely echoing a target angle is not a valid hardware acceptance.
//! A transport timeout or mismatched epoch/sequence fails closed.
use super::auto_aim_host::MotorFeedback;
use std::time::Duration;

const MAX_FRAME: usize = 160;
const MAX_MOTOR_ANGLE: i16 = 90;
const MOVE_TIMEOUT: Duration = Duration::from_millis(1_500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MotorProtocolError {
    InvalidCommand,
    Transport(String),
    InvalidFrame,
    InvalidChecksum,
    ReplayOrWrongTarget,
    UnsafePosition,
}

pub trait MotorExchange {
    /// Exchange exactly one frame under an actual enforced deadline.
    fn exchange(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, String>;
    /// Independent emergency stop; firmware still requires a hardware limit.
    fn emergency_stop(&mut self) -> Result<(), String>;
}

pub struct VerifiedMotorLink<T: MotorExchange> {
    transport: T,
    epoch: u64,
    last_sequence: u32,
}

impl<T: MotorExchange> VerifiedMotorLink<T> {
    pub fn new(transport: T, fresh_epoch: u64) -> Result<Self, MotorProtocolError> {
        if fresh_epoch == 0 {
            return Err(MotorProtocolError::InvalidCommand);
        }
        Ok(Self {
            transport,
            epoch: fresh_epoch,
            last_sequence: 0,
        })
    }

    pub fn request_position(
        &mut self,
        module: &str,
        target_deg: i16,
    ) -> Result<MotorFeedback, MotorProtocolError> {
        if !valid_id(module) || !(-MAX_MOTOR_ANGLE..=MAX_MOTOR_ANGLE).contains(&target_deg) {
            return Err(MotorProtocolError::InvalidCommand);
        }
        let next = self
            .last_sequence
            .checked_add(1)
            .ok_or(MotorProtocolError::InvalidCommand)?;
        self.last_sequence = next; // Never reuse a sequence after timeout.
        let body = format!(
            "AIM1|MOVE|{}|{}|{}|{}",
            self.epoch, next, module, target_deg
        );
        let request = append_crc(&body);
        let response = self
            .transport
            .exchange(request.as_bytes(), MOVE_TIMEOUT)
            .map_err(MotorProtocolError::Transport)?;
        let body = validate_frame(&response)?;
        let parts: Vec<&str> = body.split('|').collect();
        if parts.len() != 8 || parts[0] != "AIM1" || parts[1] != "ACK" {
            return Err(MotorProtocolError::InvalidFrame);
        }
        let epoch: u64 = parts[2]
            .parse()
            .map_err(|_| MotorProtocolError::InvalidFrame)?;
        let seq: u32 = parts[3]
            .parse()
            .map_err(|_| MotorProtocolError::InvalidFrame)?;
        let actual: i16 = parts[5]
            .parse()
            .map_err(|_| MotorProtocolError::InvalidFrame)?;
        if epoch != self.epoch || seq != next || parts[4] != module {
            return Err(MotorProtocolError::ReplayOrWrongTarget);
        }
        let settled = match parts[6] {
            "1" => true,
            "0" => false,
            _ => return Err(MotorProtocolError::InvalidFrame),
        };
        let limits = match parts[7] {
            "1" => true,
            "0" => false,
            _ => return Err(MotorProtocolError::InvalidFrame),
        };
        if !settled
            || !limits
            || actual != target_deg
            || !(-MAX_MOTOR_ANGLE..=MAX_MOTOR_ANGLE).contains(&actual)
        {
            return Err(MotorProtocolError::UnsafePosition);
        }
        Ok(MotorFeedback {
            measured_angle_deg: actual,
            physically_settled: settled,
            within_travel_limits: limits,
        })
    }

    pub fn emergency_stop(&mut self) -> Result<(), MotorProtocolError> {
        self.transport
            .emergency_stop()
            .map_err(MotorProtocolError::Transport)
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }
}

/// Firmware-side ACK encoding for simulator and integration reference only.
/// Firmware may call this ONLY with verified encoder/limit-switch feedback.
pub fn reference_ack(
    epoch: u64,
    sequence: u32,
    module: &str,
    measured_deg: i16,
    encoder_settled: bool,
    limits_ok: bool,
) -> Result<Vec<u8>, MotorProtocolError> {
    if !valid_id(module)
        || !(-MAX_MOTOR_ANGLE..=MAX_MOTOR_ANGLE).contains(&measured_deg)
        || epoch == 0
        || sequence == 0
    {
        return Err(MotorProtocolError::InvalidCommand);
    }
    let body = format!(
        "AIM1|ACK|{}|{}|{}|{}|{}|{}",
        epoch,
        sequence,
        module,
        measured_deg,
        u8::from(encoder_settled),
        u8::from(limits_ok),
    );
    Ok(append_crc(&body).into_bytes())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn append_crc(body: &str) -> String {
    format!("{body}|{:04X}\n", crc16(body.as_bytes()))
}

fn validate_frame(frame: &[u8]) -> Result<&str, MotorProtocolError> {
    if frame.len() > MAX_FRAME || frame.last() != Some(&b'\n') {
        return Err(MotorProtocolError::InvalidFrame);
    }
    let text = std::str::from_utf8(frame).map_err(|_| MotorProtocolError::InvalidFrame)?;
    let line = text
        .strip_suffix('\n')
        .ok_or(MotorProtocolError::InvalidFrame)?;
    let (payload, expected) = line
        .rsplit_once('|')
        .ok_or(MotorProtocolError::InvalidFrame)?;
    if expected.len() != 4 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(MotorProtocolError::InvalidFrame);
    }
    let checksum =
        u16::from_str_radix(expected, 16).map_err(|_| MotorProtocolError::InvalidFrame)?;
    if checksum != crc16(payload.as_bytes()) {
        return Err(MotorProtocolError::InvalidChecksum);
    }
    Ok(payload)
}

/// CRC-16/CCITT-FALSE for accidental corruption detection; not authentication.
/// The production network/serial link must independently authenticate peers.
fn crc16(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0xffff;
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeWire {
        stale_reply: bool,
        bad_crc: bool,
        unsettled: bool,
        emergency_stopped: bool,
    }

    impl MotorExchange for FakeWire {
        fn exchange(&mut self, request: &[u8], _: Duration) -> Result<Vec<u8>, String> {
            let body = validate_frame(request).map_err(|e| format!("{e:?}"))?;
            let p: Vec<&str> = body.split('|').collect();
            if p.len() != 6 {
                return Err("wrong command".into());
            }
            let epoch = p[2].parse().map_err(|_| "bad epoch")?;
            let sequence: u32 = p[3].parse().map_err(|_| "bad seq")?;
            let angle = p[5].parse().map_err(|_| "bad angle")?;
            let mut ack = reference_ack(
                epoch,
                if self.stale_reply {
                    sequence + 1
                } else {
                    sequence
                },
                p[4],
                angle,
                !self.unsettled,
                true,
            )
            .map_err(|e| format!("{e:?}"))?;
            if self.bad_crc {
                let at = ack.len() - 2;
                ack[at] = b'0';
            }
            Ok(ack)
        }
        fn emergency_stop(&mut self) -> Result<(), String> {
            self.emergency_stopped = true;
            Ok(())
        }
    }

    fn rig() -> FakeWire {
        FakeWire {
            stale_reply: false,
            bad_crc: false,
            unsettled: false,
            emergency_stopped: false,
        }
    }

    #[test]
    fn checks_accepted_encoder_feedback_and_limits() {
        let mut driver = VerifiedMotorLink::new(rig(), 2026).unwrap();
        assert_eq!(
            driver
                .request_position("rear-left", 25)
                .unwrap()
                .measured_angle_deg,
            25
        );
        assert!(driver.request_position("rear-left", 95).is_err());
        assert!(driver.request_position("line\ninjection", 0).is_err());
        driver.emergency_stop().unwrap();
        assert!(driver.transport_mut().emergency_stopped);
    }

    #[test]
    fn rejects_stale_unsettled_and_corrupted_responses() {
        let mut first = rig();
        first.stale_reply = true;
        let mut driver = VerifiedMotorLink::new(first, 10).unwrap();
        assert_eq!(
            driver.request_position("front", 10).unwrap_err(),
            MotorProtocolError::ReplayOrWrongTarget
        );
        // The helper cannot generate sequence zero, and rejects replay fixture.
        let mut second = rig();
        second.unsettled = true;
        assert_eq!(
            VerifiedMotorLink::new(second, 10)
                .unwrap()
                .request_position("front", 10)
                .unwrap_err(),
            MotorProtocolError::UnsafePosition
        );
        let mut third = rig();
        third.bad_crc = true;
        assert_eq!(
            VerifiedMotorLink::new(third, 10)
                .unwrap()
                .request_position("front", 10)
                .unwrap_err(),
            MotorProtocolError::InvalidChecksum
        );
    }
}
