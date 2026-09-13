# Software completion status

This document is intentionally short-lived and tracks the branch while its completion PR is open.

Current imported/merged work:

- current `main-v2` including OAR stereo and 5.1 object-render differential validation;
- adaptive clock-rate estimator/controller/ASRC evidence;
- bounded reconnect/recovery evidence;
- panic-isolated decoder and immersive runtime recovery;
- live JOC software-reference validation;
- IAMF rendered-channel-PCM reference validation;
- fail-closed live IEC61937 ingress diagnostics;
- runtime placeholder regression gate.

Remaining software work on this branch is to remove or explicitly de-scope incomplete runtime adapters and then make all branch CI green. Physical/external acceptance remains separate.
