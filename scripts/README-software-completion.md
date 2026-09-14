# Software-completion CI helpers

`scripts/check_no_runtime_placeholders.py` scans production Rust source paths and fails when explicit implementation placeholders (`todo!`, `unimplemented!`, or the marker word `placeholder`) are present. Tests, benches and examples are intentionally excluded.

The check is a regression guard only. A passing result does not imply physical hardware validation, protected-service compatibility, acoustics or certification.
