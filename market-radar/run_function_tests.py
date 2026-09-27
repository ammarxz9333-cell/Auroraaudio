#!/usr/bin/env python3
"""Run the project's simple test_* functions with only the Python stdlib."""
import importlib.util
from pathlib import Path

root = Path(__file__).resolve().parent
count = 0
for path in sorted([*root.glob("test_*.py"), *(root / "backtest").glob("test_*.py")]):
    spec = importlib.util.spec_from_file_location(path.stem, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    for name in sorted(vars(module)):
        if name.startswith("test_") and callable(getattr(module, name)):
            getattr(module, name)()
            count += 1
print(f"PASS {count} function tests")
