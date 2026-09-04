# Aurora Product Foundations Acceptance Matrix

This matrix is a release gate. It schedules verification; it does not supersede the active implementation roadmap.

## Safe updates / rollback

- [ ] Reject bad signature/hash before activation.
- [ ] Preserve last-known-good system state.
- [ ] Survive interrupted update and boot old state.
- [ ] Automatic rollback after failed post-update health checks.
- [ ] Manual rollback from S6 UI and authenticated web UI.
- [ ] Independent plugin rollback.
- [ ] 20 update/rollback cycles without filesystem corruption.
- [ ] Music library, playlists and user data unchanged across rollback.

## Plugin Manager

- [ ] Versioned manifest/API validation.
- [ ] Permission declaration and enforcement.
- [ ] Incompatible plugin rejected.
- [ ] Plugin crash cannot interrupt realtime audio.
- [ ] Restart budget and quarantine policy verified.
- [ ] Resource telemetry available.
- [ ] Install/update/disable/remove/rollback lifecycle tested.
- [ ] No direct STM32/amplifier/raw-callback access outside reviewed broker path.

## Home Assistant / MQTT

- [ ] Local playback survives broker/network outage.
- [ ] State telemetry converges after reconnect.
- [ ] Auth/TLS configuration supported for non-local brokers.
- [ ] Rate limiting and authorization tested.
- [ ] Invalid/duplicate/reordered commands handled deterministically.
- [ ] Commands cannot bypass Source Manager or safety mute.

## Unified diagnostics / self-healing

- [ ] S6 CPU/thermal/memory/storage/Wi-Fi telemetry.
- [ ] Audio deadline/xrun/buffer telemetry.
- [ ] USB S6↔STM32 reset/framing/clock/queue telemetry.
- [ ] STM32 mute/TDM/SAI/clock state.
- [ ] Wireless rear RSSI/loss/jitter/drift/health telemetry.
- [ ] Plugin and Music Hub health telemetry.
- [ ] Fault injection: plugin crash.
- [ ] Fault injection: noncritical service crash.
- [ ] Fault injection: USB reset.
- [ ] Fault injection: rear-node disconnect.
- [ ] Fault injection: Wi-Fi loss.
- [ ] Fault injection: storage pressure.
- [ ] One-button sanitized diagnostics bundle.
- [ ] Bundle privacy scan excludes credentials, tokens and media.

## Capability Registry

- [ ] Product features represented in registry, not only renderers.
- [ ] Honest states include host-tested vs hardware-validated distinction.
- [ ] UI hides/disables unavailable capabilities from registry data.
- [ ] Documentation claims cannot exceed registry state.
- [ ] Release bundle includes registry snapshot and evidence references.

No section may be marked production-ready from compilation-only evidence.
