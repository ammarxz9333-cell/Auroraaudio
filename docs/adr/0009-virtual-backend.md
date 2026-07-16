# ADR 0009: Aurora-Owned Virtual Audio Backend

## Decision

Add `aurora-realtime-audio-sim` as a backend-trait implementation independent
of CPAL. Keep virtual device configuration and reports in Aurora-owned types.

## Consequences

The complete control path can run on machines without capture hardware. CPAL
and simulator behavior remain separately testable, and simulator truth cannot
be confused with host or physical measurements.
