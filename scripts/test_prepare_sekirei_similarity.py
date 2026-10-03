#!/usr/bin/env python3
"""Regression tests for the downstream Sekirei similarity adapter."""

import unittest

from prepare_sekirei_similarity import hand_counts, parse_board, prepare, sfen_distance


START = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1"
WHITE = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL w - 2"
MOVED = "lnsgkgsnl/1r5b1/ppppppppp/9/9/7P1/PPPPPPP1P/1B5R1/LNSGKGSNL w - 2"


def entry(state, action):
    return {"state": state, "actions": [{"action": action}]}


class SekireiSimilarityAdapterTests(unittest.TestCase):
    def test_parser_expands_board_and_hands(self):
        self.assertEqual(len(parse_board(START.split()[0])), 81)
        self.assertEqual(hand_counts("2Rb3p")["R"], 2)
        self.assertEqual(hand_counts("2Rb3p")["b"], 1)
        self.assertEqual(hand_counts("2Rb3p")["p"], 3)

    def test_distance_is_symmetric_and_ignores_move_number(self):
        same_position = START.rsplit(" ", 1)[0] + " 99"
        self.assertEqual(sfen_distance(START, same_position), 0.0)
        self.assertEqual(sfen_distance(START, MOVED), sfen_distance(MOVED, START))
        self.assertGreater(sfen_distance(START, WHITE), 0.0)

    def test_prepare_excludes_exact_state_and_is_deterministic(self):
        train = [entry(START, "7g7f"), entry(WHITE, "3c3d"), entry(MOVED, "8c8d")]
        query = [entry(START, "2g2f")]
        first = prepare(train, query, 2)
        second = prepare(list(reversed(train)), query, 2)
        self.assertEqual(first, second)
        self.assertEqual(first[0]["expected_action"], "2g2f")
        self.assertNotIn(START, [row["state"] for row in first[0]["neighbors"]])


if __name__ == "__main__":
    unittest.main()
