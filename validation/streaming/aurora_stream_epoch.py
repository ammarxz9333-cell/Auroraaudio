#!/usr/bin/env python3
"""Aurora streaming ingress epoch machine: automatic *format descriptor* routing.

No sample processing or physical receiver is implemented here. The hardware
adapter must submit truthful format/transport descriptors and decoder evidence.
Never switch decoders while old PCM is still allowed to play.
"""
from __future__ import annotations

from dataclasses import dataclass
from enum import Enum

from aurora_format_routes import (
    AVAILABLE_SOFTWARE, AudioSource, AudioRoute, Route, choose_route,
)


class EpochState(Enum):
    MUTED = "muted"
    WAIT_OBJECT_PROOF = "wait-object-proof"
    DECODER_PRIMING = "decoder-priming"
    AUDIBLE = "audible"
    UNSUPPORTED = "unsupported"
    FAULTED = "faulted"


@dataclass(frozen=True)
class EpochCommand:
    epoch: int
    state: EpochState
    action: str
    decoder_backend: str | None
    reason: str


@dataclass(frozen=True)
class DecoderEvidence:
    """Proof from the CURRENT decoder instance, never inferred from codec id."""
    epoch: int
    backend: str
    output_channels: int
    sample_rate_hz: int
    valid_pcm_frames: int
    object_metadata_recovered: bool
    synthetic_extensions: bool
    finite_samples: bool
    channel_map_verified: bool


class AuroraIngressEpochs:
    """Media/control thread only; real audio callback consumes vetted PCM."""

    def __init__(self, verified_backends=AVAILABLE_SOFTWARE):
        self.backends = verified_backends
        self.epoch = 0
        self.state = EpochState.MUTED
        self.selected: AudioRoute | None = None
        self.source: AudioSource | None = None
        self.last_failure = ""
        self.transitions = 0

    def new_source(self, source: AudioSource) -> EpochCommand:
        """Called on startup, any observed codec/layout/transport change, or reset.

        Integrator MUST stop old decoder and mute its old buffer before this
        result is allowed to start the next decoder.
        """
        self.epoch += 1
        self.transitions += 1
        self.state = EpochState.MUTED
        self.source = source
        selected = choose_route(source, verified_backends=self.backends)
        self.selected = selected
        self.last_failure = ""
        if selected.route is Route.HOLD_FOR_OBJECT_PROBE:
            self.state = EpochState.WAIT_OBJECT_PROOF
            return EpochCommand(self.epoch, self.state, "mute-and-probe-objects",
                                None, selected.reason)
        if selected.route is Route.BLOCKED:
            self.state = EpochState.UNSUPPORTED
            return EpochCommand(self.epoch, self.state, "mute-unsupported", None,
                                selected.reason)
        self.state = EpochState.DECODER_PRIMING
        return EpochCommand(self.epoch, self.state, "mute-reset-start-decoder",
                            selected.backend, selected.reason)

    def accept_object_probe(self, observed_source: AudioSource) -> EpochCommand:
        if self.state is not EpochState.WAIT_OBJECT_PROOF:
            return self._fault("unexpected object probe while not waiting")
        if self.source is None or (
            observed_source.transport != self.source.transport
            or observed_source.codec != self.source.codec
            or observed_source.channels != self.source.channels
        ):
            return self._fault("object evidence is for a different source epoch")
        if observed_source.objects.value == "unknown":
            return EpochCommand(self.epoch, self.state, "keep-muted-probe",
                                None, "still insufficient evidence of native JOC")
        decision = choose_route(observed_source, verified_backends=self.backends)
        self.source = observed_source
        self.selected = decision
        if decision.route is Route.BLOCKED or decision.route is Route.HOLD_FOR_OBJECT_PROBE:
            self.state = EpochState.UNSUPPORTED
            return EpochCommand(self.epoch, self.state, "mute-unsupported",
                                None, decision.reason)
        self.state = EpochState.DECODER_PRIMING
        return EpochCommand(self.epoch, self.state, "mute-reset-start-decoder",
                            decision.backend, decision.reason)

    def decoder_evidence(self, observed: DecoderEvidence) -> EpochCommand:
        if self.state is not EpochState.DECODER_PRIMING or self.selected is None:
            return self._fault("decoder evidence out of sequence")
        if (
            observed.epoch != self.epoch
            or observed.backend != self.selected.backend
            or observed.output_channels != 12
            or observed.sample_rate_hz != 48_000
            or observed.valid_pcm_frames < 256
            or not observed.finite_samples
            or not observed.channel_map_verified
            or observed.object_metadata_recovered != self.selected.native_objects
            or observed.synthetic_extensions != self.selected.synthetic_extensions
        ):
            return self._fault("decoder/PCM shape or native object claim invalid")
        self.state = EpochState.AUDIBLE
        return EpochCommand(self.epoch, self.state, "unmute-current-epoch-only",
                            self.selected.backend, "validated 12ch decoded PCM")

    def discontinuity(self, reason: str) -> EpochCommand:
        self.epoch += 1
        self.transitions += 1
        self.selected = None
        self.source = None
        self.state = EpochState.MUTED
        self.last_failure = reason
        return EpochCommand(self.epoch, self.state, "mute-flush-and-reprobe",
                            None, reason)

    def _fault(self, reason: str) -> EpochCommand:
        self.state = EpochState.FAULTED
        self.last_failure = reason
        return EpochCommand(self.epoch, self.state, "mute-stop-decoder",
                            None, reason)


if __name__ == "__main__":
    print("This is a transport/codec truth control plane, not a hardware decoder.")
