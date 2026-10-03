#!/usr/bin/env python3
"""Regression tests for similarity measurement helpers."""

import unittest

from measure_similarity import normalize_peak_rss_kb, resolve_fingerprint


class SimilarityMeasurementTests(unittest.TestCase):
    def test_peak_rss_is_normalized_from_macos_bytes(self):
        self.assertEqual(normalize_peak_rss_kb(1_048_576, "Darwin"), 1024.0)
        self.assertEqual(normalize_peak_rss_kb(1024, "Linux"), 1024.0)

    def test_headerless_book_accepts_explicit_fingerprint(self):
        self.assertEqual(resolve_fingerprint("unspecified", "123"), "123")

    def test_conflicting_fingerprint_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "conflicts"):
            resolve_fingerprint(123, "456")


if __name__ == "__main__":
    unittest.main()
