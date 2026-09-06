# S6 live streaming transport reliability

This change maintains the external decoder boundary. The source device supplies
HDMI/eARC audio; Omniphony and Harletty remain responsible for IEC61937 parsing,
codec decoding, object metadata and rendering. No codec implementation is added.

## Failure addressed

The broker previously completed each encoded-input write before reading decoder
output. A decoder producing enough PCM to fill its output pipe could stop reading
input while the broker waited for input space. The broker would time out and
restart an otherwise functioning decoder. A blocking downstream socket send could
also stop input and control processing indefinitely.

The broker now queues encoded bytes in fixed 512 KiB storage and polls decoder
stdin for writability alongside decoder stdout and the bridge socket. A partial
write retains its exact remaining bytes and order. Each poll cycle performs a
bounded output read, keeping input/control traffic eligible for service.

PCM packets use nonblocking socket sends. Temporary downstream congestion retains
samples, sequence number, timestamp and discontinuity flags until a complete
packet is accepted. The existing 128 KiB PCM buffer applies pipe backpressure when
full; it no longer reads data it cannot retain. CONFIG acknowledgement remains
mandatory before PCM transmission. Failure to send CONFIG reconnects the bridge.

Encoded queue overflow still returns an error to the existing controlled restart
path. Restart discards both pending queues and flags subsequent PCM as a
discontinuity/recovery where appropriate. This is finite buffering, not a promise
to survive unlimited downstream stalls without interruption. Counters for encoded
frames/bytes describe accepted input, not successful codec decoding.

Oversized sequenced packets are detected with `MSG_TRUNC` and rejected before
header/payload access. Old poll events are not reused after renderer replacement;
readable output accompanying a hangup is drained before handling the hangup.

These are process I/O operations outside the real-time device callback. The
existing small control-message write retains its bounded timeout. No callback
allocation or process-launch capability is introduced.

## Reproduction

From the repository root on Linux:

```sh
cc -std=c11 -O2 -Wall -Wextra -Werror -Iprotocol \
  platform/s6/live-ingest/test_live_ingest_io.c -lm -o /tmp/test-live-ingest-io
/tmp/test-live-ingest-io
cc -std=c11 -O2 -Wall -Wextra -Werror -Iprotocol \
  platform/s6/live-ingest/aurora-live-ingest.c -lm -o /tmp/aurora-live-ingest
python3 platform/s6/live-ingest/test_live_ingest.py \
  /tmp/aurora-live-ingest platform/s6/live-ingest/mock_orender.py
```

The C tests use real pipes and a child process, substituting only downstream
socket sends. They cover retained PCM on EAGAIN, input capacity, full output
buffer behavior and a 256 KiB bidirectional exchange producing 512 PCM periods.
The Python suite additionally exercises the actual broker event loop with a
renderer that alternates input consumption and output production while the sink
temporarily stops reading. S6 Appliance CI runs both suites.

## Validation recorded on 2026-09-06

- Optimized C broker compilation with warnings as errors: passed.
- C pipe regression suite: passed, including all 512 periods/sample values.
- Same suite with UndefinedBehaviorSanitizer: passed.
- Python syntax and native builder shell syntax: passed.
- Unix socket integration: blocked locally by `socket(AF_UNIX, SOCK_SEQPACKET)`
  returning EPERM. The added integration case has not been executed locally.
- Rust workspace format/lint/test/bench: blocked locally because Cargo is absent.
- Native Alpine AArch64 external-adapter build: not executed here.

The builder now selects the documented Harletty v0.7.4 rather than v0.7.3,
checks both external source tags against explicit commit IDs and records them
in its build manifest. Pinned source identity does not prove ABI compatibility
or runtime decoding; those remain external-adapter validation requirements.

No JOC fixture, live service, S6, HDMI/eARC board, USB MCU, DAC, speaker,
thermal measurement or acoustic comparison was exercised. The mandatory
[live streaming acceptance contract](AURORA_LIVE_STREAMING_ATMOS_ACCEPTANCE.md)
remains unmet; there is no basis yet to claim superiority to Samsung Q995.
