"""Reject invocation drift before any external process or fixture write."""
import copy
import json
from pathlib import Path
import unittest
from unittest.mock import patch

from binaural_7_1_4_differential import main, require_contract_shape


class ContractTests(unittest.TestCase):
    def setUp(self):
        self.config = json.loads((Path(__file__).resolve().parents[2] /
            "config/binaural-7-1-4-differential-v1.json").read_text())

    def test_supported(self):
        require_contract_shape(self.config)

    def test_reject_before_external_execution(self):
        for field in ("input_type", "filter_type", "cli_target"):
            for value in (None, "unsupported", 42):
                with self.subTest(field=field, value=value):
                    config = copy.deepcopy(self.config)
                    if value is None:
                        del config["google_obr"][field]
                    else:
                        config["google_obr"][field] = value
                    with patch("sys.argv", ["validator", "--config", "unused",
                        "--obr-root", "unused", "--obr-cli", "unused",
                        "--aurora-json", "unused", "--work-dir", "unused",
                        "--output", "unused"]), patch.object(Path, "read_text",
                        return_value=json.dumps(config)), patch(
                        "binaural_7_1_4_differential.git_head") as git, patch(
                        "binaural_7_1_4_differential.run_obr_cases") as run:
                        with self.assertRaisesRegex(SystemExit, "google_obr." + field):
                            main()
                        git.assert_not_called()
                        run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
