# Fault Injection

Fault timelines are JSON arrays loaded after simulator startup. Each event has
`at_milliseconds`, `action`, `duration_milliseconds`, and an action-specific
`value`. Fixtures live in `fixtures/simulation/fault_scenarios`.

Supported actions are input/output loss, callback error, format change, stream
freeze, missing callbacks, directional input/output stalls, scheduling stall,
callback burst, clock jump, and callback-size change. Stream faults enter `Faulted`, request bounded recovery
for the same virtual endpoint, enter `Recovering`, and return to `Running` only
when the scripted device reappears. Numeric fault classes use Aurora's existing
backend contract.

Fault reports contain ordered state transitions. The simulator never silently
chooses the duplicate endpoint in the broken-driver profile; ambiguous names
are rejected.
