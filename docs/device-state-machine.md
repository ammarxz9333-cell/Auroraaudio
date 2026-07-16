# Duplex Device State Machine

Simulation fault timelines drive this same state contract deterministically.
Loss, callback, and format faults enter `Faulted`; bounded recovery reopens the
same stable virtual ID through `Recovering`. Duplicate names are rejected and
never cause a silent endpoint switch.

Aurora defines these control-thread states:

```text
Stopped -> Starting -> Running -> Degraded
                         |           |
                         +-> Faulted <-+
                              |
                         Recovering
                              |
                    Running or Faulted

Running/Degraded/Faulted/Recovering -> Stopping -> Stopped
```

Callbacks publish only numeric backend, engine, ASRC, and controller faults.
The control thread observes them, transitions to `Faulted`, and stops both streams
without waiting in either callback. Device unavailable, device loss, callback
error, unsupported format, and format mismatch are distinct errors or fault codes.

Recovery is optional and bounded by the CLI restart count. It reuses the exact
input/output selectors. Ambiguous selectors are rejected, and no default or fuzzy
alternative is silently substituted. State transitions and device-loss fault
propagation are hardware-independent tests; disconnect/reconnect is ignored until
an operator provides hardware.
