"""Offline checks for the Desktop publish job's tag and latest decision."""
from pathlib import Path
import subprocess
import sys
import unittest


class ReleasePolicyTests(unittest.TestCase):
    def decide(self, tag, *published):
        return subprocess.run(
            [sys.executable, str(Path(__file__).with_name("release-policy.py")), tag],
            input="\n".join(published), capture_output=True, text=True,
        )

    def test_stable_feed_only_advances(self):
        for tag, published, latest in [
            ("v0.3.4", [], "true"),
            ("v0.3.4", ["v1.0.0", "v0.3.3"], "false"),  # older build finished last
            ("v0.3.10", ["v0.3.9", "v1.0.0-rc.1", "nightly"], "true"),
            ("v0.3.10", ["v0.3.10"], "true"),  # retry
            ("v0.3.9", ["v0.3.10"], "false"),
        ]:
            with self.subTest(tag=tag, published=published):
                result = self.decide(tag, *published)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, f"latest={latest}\n")

    def test_prerelease_or_malformed_tag_cannot_publish(self):
        for tag in ["v1.0.0-rc.1", "v1.0.0+build", "v01.0.0", "v1.0", "1.0.0"]:
            with self.subTest(tag=tag):
                result = self.decide(tag)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
