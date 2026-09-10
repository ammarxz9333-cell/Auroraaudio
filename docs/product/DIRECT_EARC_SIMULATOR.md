# Direct eARC simulator

`aurora-direct-earc-sim` is a deterministic pre-hardware source for Aurora's
canonical IEC61937 direct-eARC ingest path. It does **not** emulate HDMI/eARC
electrical signaling and it does **not** infer Atmos/JOC from transport type.

## Input and output contract

Input is a finite raw E-AC-3 elementary stream (`.ec3`) on stdin. The simulator
uses the exact OpenJOC revision pinned by `aurora-sim-source` to prove six-block
access-unit boundaries. Each proven access unit is emitted as one canonical
IEC61937 E-AC-3 (`Pc` data type `0x15`) carrier period.

The generated Aurora carrier contract is:

- 24,576 bytes per nominal E-AC-3 period;
- byte-count `Pd`, matching `aurora-iec61937`;
- native E-AC-3 bytes swapped inside each 16-bit carrier word;
- zero padding after the declared payload to the fixed period boundary.

A complete-looking final E-AC-3 unit can remain buffered until finite EOS because
OpenJOC requires a following independent-frame header or EOS to prove some access
unit boundaries. Truncated or structurally impossible finite input fails closed.

## Generate a canonical carrier stream

```sh
cargo run --locked -p aurora-cli \
  --no-default-features --features earc-sim \
  --bin aurora-direct-earc-sim \
  < input.ec3 > carrier.iec61937
```

The simulator prints only diagnostics to stderr; stdout contains carrier bytes.

## Verify a byte-exact transport round trip

```sh
cargo run --locked -p aurora-cli \
  --no-default-features \
  --bin aurora-direct-earc-probe \
  -- --filter eac3 --extract eac3 \
  < carrier.iec61937 > roundtrip.ec3

cmp input.ec3 roundtrip.ec3
```

The Direct eARC CI workflow is prepared to perform this round trip against the
pinned OpenJOC synthetic JOC fixture before the decoder/render fixture gates.
This gate still requires an Actions runner that actually executes workflow steps.

## Inject deterministic byte loss

A deletion fault targets one zero-based generated period and one byte range inside
that period. All three coordinates are required.

Delete the final 16-bit padding word from period 0:

```sh
cargo run --locked -p aurora-cli \
  --no-default-features --features earc-sim \
  --bin aurora-direct-earc-sim \
  -- --delete-period 0 --delete-offset 24574 --delete-count 2 \
  < input.ec3 > cadence-fault.iec61937
```

If a following E-AC-3 period exists, its Pa/Pb preamble appears two carrier bytes
early. `aurora-direct-earc-probe` should therefore observe a 24,574-byte Pa-to-Pa
spacing for that transition instead of the nominal 24,576-byte period. The E-AC-3
payloads themselves remain unchanged because the deletion is in zero padding.

To model payload-header byte loss instead, delete the second encoded 16-bit word
inside the first burst payload:

```sh
cargo run --locked -p aurora-cli \
  --no-default-features --features earc-sim \
  --bin aurora-direct-earc-sim \
  -- --delete-period 0 --delete-offset 10 --delete-count 2 \
  < input.ec3 > payload-fault.iec61937
```

IEC61937 itself has no payload integrity field, so a transport parser can still
return the declared `Pd` bytes by consuming following carrier padding. The
simulator regression suite therefore checks the stronger boundary: the damaged
E-AC-3 access unit must be rejected by pinned OpenJOC framing/validation rather
than being treated as authenticated audio.

## Truth boundary

Passing simulator tests proves deterministic software framing, carrier geometry,
chunk-boundary handling and selected corruption behavior. It does not prove:

- physical HDMI/eARC receiver lock;
- ALSA/ASoC clock stability or XRUN behavior;
- sustained hardware Pa-to-Pa cadence;
- TDM/DAC speaker wiring or channel order;
- commercial streaming-service interoperability;
- Atmos/JOC object preservation merely from IEC61937 type `0x15`.

Those remain physical acceptance gates on the target hardware.
