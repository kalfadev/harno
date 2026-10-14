#!/usr/bin/env python3
"""The tests of text.py, in a throwaway folder."""
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

TEXT = pathlib.Path(__file__).resolve().parents[1] / "text.py"


def run(cwd, *args):
    r = subprocess.run([sys.executable, str(TEXT), *args], cwd=cwd, capture_output=True, text=True)
    return r.returncode, r.stdout, r.stderr


class Text(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.cwd = self.dir.name
        os.makedirs(os.path.join(self.cwd, "src"))
        pathlib.Path(self.cwd, "src", "a.rs").write_text("fn reconnect(x: u8) {}\nlet v = Name { a };\n")
        pathlib.Path(self.cwd, "b.rs").write_text("reconnect\n")

    def tearDown(self):
        self.dir.cleanup()

    def test_a_phrase_needs_no_quotes(self):
        code, out, _ = run(self.cwd, "fn", "reconnect", "src")
        self.assertEqual(code, 0)
        self.assertIn("1:fn reconnect(x: u8) {}", out)

    def test_a_bracket_is_literal_text_not_a_regex(self):
        code, out, err = run(self.cwd, "reconnect(", "src")
        self.assertEqual(code, 0, err)
        self.assertIn("fn reconnect(", out)
        code, out, err = run(self.cwd, "Name", "{", "src")
        self.assertEqual(code, 0, err)
        self.assertIn("2:let v = Name { a };", out)

    def test_no_path_searches_the_default(self):
        code, out, _ = run(self.cwd, "--in", "b.rs", "reconnect")
        self.assertEqual((code, out), (0, "1:reconnect\n"))

    def test_a_path_that_is_not_there_is_refused(self):
        code, _, err = run(self.cwd, "reconnect", "src/nowhere")
        self.assertEqual(code, 2)
        self.assertIn("no path src/nowhere", err)

    def test_a_word_is_whole(self):
        self.assertEqual(run(self.cwd, "connect", "src")[:2], (1, ""))
        self.assertEqual(run(self.cwd, "a.b", "src")[:2], (1, ""))

    def test_no_match_is_exit_1_and_no_output(self):
        self.assertEqual(run(self.cwd, "absent_name", "src")[:2], (1, ""))

    def test_leading_flags_go_to_rg(self):
        code, out, _ = run(self.cwd, "-c", "reconnect", "src", "b.rs")
        self.assertEqual(code, 0)
        self.assertIn("b.rs:1", out)


if __name__ == "__main__":
    unittest.main()
