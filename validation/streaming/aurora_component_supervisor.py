#!/usr/bin/env python3
"""Fail-closed A/B control-plane supervisor for independent Aurora components.

The production host must bind these commands to separate OS workers and a
muted/drained DSP output device. This deterministic supervisor cannot itself
prove that a physical amplifier is silent, a decoder works, or an upgrade was
safely applied on real hardware.
"""
from __future__ import annotations

from dataclasses import dataclass
from enum import Enum

from aurora_component_contracts import (
    AudioComponentGraph, ComponentContractError, ModuleContract,
)


class Lifecycle(Enum):
    ACTIVE = "active"
    STAGED = "staged"
    QUIESCING = "quiescing"
    PRIMING = "priming"
    RECOVERING = "recovering"
    FAILED_CLOSED = "failed-closed"


class UpdateFailure(ValueError):
    pass


@dataclass(frozen=True)
class UpdateAction:
    generation: int
    lifecycle: Lifecycle
    action: str
    component_id: str | None
    comment: str


@dataclass(frozen=True)
class WorkerReadiness:
    generation: int
    component_id: str
    input_contract: str
    output_contract: str
    authenticated_worker: bool
    finite_output: bool
    no_xruns: bool
    clock_stable: bool


class ComponentUpgradeSupervisor:
    """Explicit operator-driven updates; no timers or auto-upgrade hooks."""

    def __init__(self, graph: AudioComponentGraph):
        self.active = graph.verify()
        self.previous = self.active
        self.staged: AudioComponentGraph | None = None
        self.component_id: str | None = None
        self.generation = 0
        self.lifecycle = Lifecycle.ACTIVE
        self.muted = False

    def stage(self, candidate: ModuleContract) -> UpdateAction:
        if self.lifecycle != Lifecycle.ACTIVE or self.muted:
            raise UpdateFailure("recovery/in-flight upgrade must finish before staging")
        try:
            new_graph = self.active.replace(candidate)
        except ComponentContractError as error:
            raise UpdateFailure(f"refused incompatible component: {error}") from error
        if new_graph == self.active:
            raise UpdateFailure("candidate is identical to running component")
        self.staged = new_graph
        self.component_id = candidate.name
        self.lifecycle = Lifecycle.STAGED
        return UpdateAction(self.generation, self.lifecycle, "test-in-standby-only",
                            candidate.name, "existing audio graph untouched")

    def cancel_staging(self) -> UpdateAction:
        if self.lifecycle != Lifecycle.STAGED:
            raise UpdateFailure("no idle staged component")
        target = self.component_id
        self.staged = None
        self.component_id = None
        self.lifecycle = Lifecycle.ACTIVE
        return UpdateAction(self.generation, self.lifecycle, "discard-standby",
                            target, "running pipeline stays unchanged")

    def start_quiesce(self, *, software_gates_passed: bool,
                      external_physical_approval: bool) -> UpdateAction:
        if self.lifecycle != Lifecycle.STAGED or self.staged is None:
            raise UpdateFailure("no staged component")
        if not software_gates_passed:
            raise UpdateFailure("software contract/codec conformance incomplete")
        if not external_physical_approval:
            raise UpdateFailure("no authenticated physical acceptance; leave current audio untouched")
        self.muted = True
        self.lifecycle = Lifecycle.QUIESCING
        self.previous = self.active
        self.generation += 1
        return UpdateAction(self.generation, self.lifecycle, "mute-drain-flush-previous-epoch",
                            self.component_id, "old worker must stay available for rollback")

    def acknowledge_drained(self, generation: int, *, physically_muted: bool,
                            buffers_flushed: bool, old_worker_quiesced: bool) -> UpdateAction:
        if self.lifecycle != Lifecycle.QUIESCING or generation != self.generation:
            raise UpdateFailure("stale or out-of-order drain acknowledgement")
        if not (physically_muted and buffers_flushed and old_worker_quiesced):
            return self.fail_and_rollback("incomplete hardware mute/drain")
        self.lifecycle = Lifecycle.PRIMING
        return UpdateAction(self.generation, self.lifecycle, "start-staged-worker-muted",
                            self.component_id, "no stale PCM generation may pass")

    def worker_ready(self, evidence: WorkerReadiness) -> UpdateAction:
        if self.lifecycle != Lifecycle.PRIMING:
            raise UpdateFailure("worker readiness out of order")
        if self.staged is None or self.component_id is None:
            return self.fail_and_rollback("staged worker disappeared")
        intended = self.staged.modules[self.staged.index(self.component_id)]
        if (evidence.generation != self.generation
            or evidence.component_id != self.component_id
            or evidence.input_contract != intended.requires
            or evidence.output_contract != intended.provides
            or not evidence.authenticated_worker
            or not evidence.finite_output
            or not evidence.no_xruns
            or not evidence.clock_stable):
            return self.fail_and_rollback("worker generation/ABI/PCM/sync gate failed")
        self.active = self.staged
        self.staged = None
        target = self.component_id
        self.component_id = None
        self.lifecycle = Lifecycle.ACTIVE
        self.muted = False
        return UpdateAction(self.generation, self.lifecycle, "activate-new-current-generation",
                            target, "control-plane commit; physical mute adapter external")

    def fail_and_rollback(self, reason: str) -> UpdateAction:
        if self.lifecycle not in {Lifecycle.QUIESCING, Lifecycle.PRIMING}:
            raise UpdateFailure("no active swap to roll back")
        self.muted = True
        self.generation += 1  # Invalidate any late candidate output immediately.
        self.active = self.previous
        target = self.component_id
        self.component_id = None
        self.staged = None
        self.lifecycle = Lifecycle.RECOVERING
        return UpdateAction(self.generation, self.lifecycle, "restart-previous-worker-muted",
                            target, reason)

    def recovered(self, generation: int, *, worker_recovered: bool,
                  output_verified: bool, physically_muted: bool) -> UpdateAction:
        if self.lifecycle != Lifecycle.RECOVERING or generation != self.generation:
            raise UpdateFailure("invalid recovery acknowledgement")
        if not (worker_recovered and output_verified and physically_muted):
            self.lifecycle = Lifecycle.FAILED_CLOSED
            self.muted = True
            return UpdateAction(self.generation, self.lifecycle, "stay-muted",
                                None, "previous known-good worker recovery failed")
        self.lifecycle = Lifecycle.ACTIVE
        self.muted = False
        return UpdateAction(self.generation, self.lifecycle, "unmute-recovered-generation",
                            None, "previous contract restored")

    def forced_stop(self, why: str) -> UpdateAction:
        self.generation += 1
        self.lifecycle = Lifecycle.FAILED_CLOSED
        self.muted = True
        self.staged = None
        self.component_id = None
        return UpdateAction(self.generation, self.lifecycle, "emergency-stop-stay-muted",
                            None, why)
