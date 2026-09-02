# Aurora Plugin Implementation Queue

This file records the dependency-safe implementation order for the plugin requirement. It does not override `docs/roadmaps/immersive-wireless-audio-execution-roadmap.md`.

1. Complete current Phase 0 work.
2. Complete issue #44 evaluation runner.
3. Complete issue #45 capability registry.
4. Implement plugin manifest/capability/permission validation using the capability registry.
5. Implement out-of-process plugin supervisor and local control IPC.
6. Implement bounded audio IPC and fault-injection tests.
7. Add mandatory reference plugins only when their underlying platform gates are active:
   - local catalog bridge;
   - generic remote control;
   - Bluetooth input;
   - Bluetooth output.
8. Add optional ecosystem plugins independently:
   - Alexa control;
   - Google Home/Nest control;
   - official Google Cast receiver path if available for the hardware/product;
   - Spotify Connect;
   - AirPlay;
   - UPnP/DLNA;
   - other reviewed source/control adapters.
9. Run the final product acceptance suite in `docs/product/FINAL_PRODUCT_ACCEPTANCE.md`.

No ecosystem plugin may bypass Source Manager, Aurora capability state, hardware flash gates, or the project's evidence requirements.
