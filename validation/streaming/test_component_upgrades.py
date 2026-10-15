#!/usr/bin/env python3
"""Mutation tests for every stage and simulated safe/failed independent upgrades."""
from __future__ import annotations
from dataclasses import replace
from pathlib import Path
import unittest

from aurora_component_contracts import (
    AudioComponentGraph, ComponentContractError, load_graph,
)
from aurora_component_supervisor import (
    ComponentUpgradeSupervisor, Lifecycle, UpdateFailure, WorkerReadiness,
)

CONFIG = Path(__file__).resolve().parents[2] / "config" / "aurora-audio-component-contracts-v1.json"
GRAPH = load_graph(CONFIG)


def candidate(index: int):
    original = GRAPH.modules[index]
    return replace(original, implementation=original.implementation + ".next-version")


def ready(supervisor, component, **kwargs):
    fields = dict(
        generation=supervisor.generation,
        component_id=component.name,
        input_contract=component.requires,
        output_contract=component.provides,
        authenticated_worker=True,
        finite_output=True,
        no_xruns=True,
        clock_stable=True,
    )
    fields.update(kwargs)
    return WorkerReadiness(**fields)


class PipelineContractTests(unittest.TestCase):
    def test_actual_repository_manifest_has_seven_chained_stages(self):
        self.assertEqual(len(GRAPH.modules), 7)
        self.assertEqual(GRAPH.modules[0].requires, "audio-carrier-source/v1")
        self.assertEqual(GRAPH.modules[-1].provides, "timed-speaker-output/v1")
        self.assertEqual(GRAPH.channel_roles[8:12], ("TFL", "TFR", "TRL", "TRR"))

    def test_each_stage_can_upgrade_independently_without_renumbering_channels(self):
        for index in range(len(GRAPH.modules)):
            with self.subTest(stage=GRAPH.modules[index].name):
                upgraded = GRAPH.replace(candidate(index))
                self.assertEqual(upgraded.modules[index].implementation,
                                 GRAPH.modules[index].implementation + ".next-version")
                self.assertEqual(upgraded.channel_roles, GRAPH.channel_roles)
                for other in range(len(GRAPH.modules)):
                    if other != index:
                        self.assertEqual(upgraded.modules[other], GRAPH.modules[other])

    def test_incompatible_abi_rejected_before_touching_active_audio(self):
        for index in range(len(GRAPH.modules)):
            with self.subTest(stage=GRAPH.modules[index].name):
                change = replace(candidate(index), provides="unexpected-channel-count/v9")
                with self.assertRaises(ComponentContractError):
                    GRAPH.replace(change)
        bad_format = replace(GRAPH, sample_rate_hz=96_000)
        with self.assertRaises(ComponentContractError):
            bad_format.verify()
        swapped = replace(GRAPH, channel_roles=(
            "FR", "FL", *GRAPH.channel_roles[2:]
        ))
        with self.assertRaises(ComponentContractError):
            swapped.verify()

    def test_stage_requires_explicit_approval_and_does_not_mute_current_audio(self):
        controller = ComponentUpgradeSupervisor(GRAPH)
        first = controller.stage(candidate(2))
        self.assertEqual(first.action, "test-in-standby-only")
        self.assertFalse(controller.muted)
        self.assertEqual(controller.active, GRAPH)
        with self.assertRaises(UpdateFailure):
            controller.stage(candidate(0))
        with self.assertRaises(UpdateFailure):
            controller.start_quiesce(software_gates_passed=True, external_physical_approval=False)
        self.assertEqual(controller.lifecycle, Lifecycle.STAGED)
        self.assertFalse(controller.muted)
        controller.cancel_staging()
        self.assertEqual(controller.lifecycle, Lifecycle.ACTIVE)

    def test_successful_synthetic_control_plane_upgrade_all_seven_stages(self):
        controller = ComponentUpgradeSupervisor(GRAPH)
        for index in range(len(GRAPH.modules)):
            next_component = candidate(index)
            controller.stage(next_component)
            step = controller.start_quiesce(
                software_gates_passed=True,
                external_physical_approval=True,  # injected SIMULATION signal only
            )
            self.assertTrue(controller.muted)
            self.assertEqual(step.action, "mute-drain-flush-previous-epoch")
            step = controller.acknowledge_drained(
                controller.generation,
                physically_muted=True, buffers_flushed=True,
                old_worker_quiesced=True,
            )
            self.assertEqual(step.action, "start-staged-worker-muted")
            step = controller.worker_ready(ready(controller, next_component))
            self.assertEqual(step.action, "activate-new-current-generation")
            self.assertEqual(controller.lifecycle, Lifecycle.ACTIVE)
            self.assertFalse(controller.muted)
            self.assertEqual(controller.active.channel_roles, GRAPH.channel_roles)
        self.assertEqual(controller.generation, 7)

    def test_stale_worker_frame_triggers_rollback_without_graph_change(self):
        controller = ComponentUpgradeSupervisor(GRAPH)
        changed = candidate(2)
        controller.stage(changed)
        controller.start_quiesce(
            software_gates_passed=True, external_physical_approval=True)
        controller.acknowledge_drained(
            controller.generation, physically_muted=True,
            buffers_flushed=True, old_worker_quiesced=True)
        stale = ready(controller, changed, generation=controller.generation-1)
        result = controller.worker_ready(stale)
        self.assertEqual(result.action, "restart-previous-worker-muted")
        self.assertEqual(controller.lifecycle, Lifecycle.RECOVERING)
        self.assertEqual(controller.active, GRAPH)
        self.assertTrue(controller.muted)
        with self.assertRaises(UpdateFailure):
            controller.stage(candidate(5))
        assert result.generation == controller.generation
        recovered = controller.recovered(
            result.generation, worker_recovered=True,
            output_verified=True, physically_muted=True)
        self.assertEqual(recovered.action, "unmute-recovered-generation")
        self.assertFalse(controller.muted)

    def test_wrong_channel_clock_worker_or_bad_drain_blocks_activation(self):
        for failure in ("output_contract", "clock_stable", "finite_output", "authenticated_worker"):
            with self.subTest(failure=failure):
                controller = ComponentUpgradeSupervisor(GRAPH)
                target = candidate(3)
                controller.stage(target)
                controller.start_quiesce(
                    software_gates_passed=True, external_physical_approval=True)
                controller.acknowledge_drained(
                    controller.generation,
                    physically_muted=True, buffers_flushed=True, old_worker_quiesced=True)
                bad = {"output_contract": "aurora-2ch-pcm/v9"} if failure == "output_contract" else {failure: False}
                outcome = controller.worker_ready(ready(controller, target, **bad))
                self.assertEqual(outcome.action, "restart-previous-worker-muted")
                self.assertEqual(controller.active, GRAPH)
                self.assertTrue(controller.muted)
        controller = ComponentUpgradeSupervisor(GRAPH)
        controller.stage(candidate(5))
        controller.start_quiesce(
            software_gates_passed=True, external_physical_approval=True)
        outcome = controller.acknowledge_drained(
            controller.generation, physically_muted=False,
            buffers_flushed=True, old_worker_quiesced=True)
        self.assertEqual(outcome.lifecycle, Lifecycle.RECOVERING)

    def test_recovery_failure_stays_muted_not_failed_open(self):
        controller = ComponentUpgradeSupervisor(GRAPH)
        controller.stage(candidate(0))
        controller.start_quiesce(software_gates_passed=True, external_physical_approval=True)
        fail = controller.acknowledge_drained(
            controller.generation, physically_muted=False,
            buffers_flushed=False, old_worker_quiesced=False)
        result = controller.recovered(
            fail.generation, worker_recovered=False, output_verified=False,
            physically_muted=False)
        self.assertEqual(result.lifecycle, Lifecycle.FAILED_CLOSED)
        self.assertTrue(controller.muted)
        self.assertEqual(controller.active, GRAPH)
        with self.assertRaises(UpdateFailure):
            controller.stage(candidate(1))


if __name__ == "__main__":
    unittest.main()
