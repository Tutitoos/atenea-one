# Copyright 2026 Tutitoos
# SPDX-License-Identifier: Apache-2.0
"""El environment `release` debe exigir revisores y solo tags `v*`."""
import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("check_environment", Path(__file__).resolve().parents[2] / "scripts/release/check-environment.py")
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)

GOOD = {
    "protection_rules": [{"type": "required_reviewers", "prevent_self_review": True, "reviewers": [{"type": "User"}]}],
    "deployment_branch_policy": {"protected_branches": False, "custom_branch_policies": True},
}
TAGS = {"branch_policies": [{"name": "v*", "type": "tag"}]}


class Environment(unittest.TestCase):
    def test_good_configuration_passes(self):
        self.assertEqual(check.problems(GOOD, TAGS), [])

    def test_self_review_is_accepted_for_single_maintainer(self):
        rule = {"type": "required_reviewers", "prevent_self_review": False, "reviewers": [{}]}
        self.assertEqual(check.problems({**GOOD, "protection_rules": [rule]}, TAGS), [])

    def test_missing_reviewers_and_branches_fail(self):
        self.assertTrue(check.problems({**GOOD, "protection_rules": []}, TAGS))
        rule = {"type": "required_reviewers", "prevent_self_review": False, "reviewers": []}
        self.assertTrue(check.problems({**GOOD, "protection_rules": [rule]}, TAGS))
        self.assertTrue(check.problems({**GOOD, "deployment_branch_policy": None}, TAGS))
        self.assertTrue(check.problems(GOOD, {"branch_policies": [{"name": "main", "type": "branch"}]}))
        self.assertTrue(check.problems(GOOD, {"branch_policies": []}))


if __name__ == "__main__":
    unittest.main()
