# ADR 0010: Integer-Tick Deterministic Scheduler

## Decision

Use independent integer-picosecond clock domains and a seeded local generator.
Accelerated mode advances to the next event without sleeping.

## Consequences

Runs are repeatable and faster than real time. The scheduler is a digital model,
not evidence about operating-system callback timing.
