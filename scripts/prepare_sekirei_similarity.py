#!/usr/bin/env python3
"""Prepare held-out Sekirei queries for the generic similarity runner.

This is deliberately a downstream case-study adapter. The lineprior core
continues to treat state and action as opaque strings.
"""

import argparse
import json
import math
import pathlib


FEATURE_VERSION = "sekirei-sfen-square-onehot-v1"
MAX_HAND_COUNT = 18.0


def load_book(path):
    rows = []
    for line_number, line in enumerate(pathlib.Path(path).read_text().splitlines(), 1):
        if not line.strip():
            continue
        row = json.loads(line)
        if "state" not in row:
            continue
        actions = row.get("actions")
        if not isinstance(actions, list) or not actions:
            raise ValueError(f"{path} line {line_number}: actions must be non-empty")
        rows.append(row)
    if not rows:
        raise ValueError(f"{path}: no prior entries")
    return rows


def parse_board(board):
    squares = []
    for rank in board.split("/"):
        index = 0
        while index < len(rank):
            char = rank[index]
            if char.isdigit():
                squares.extend([None] * int(char))
                index += 1
                continue
            if char == "+":
                index += 1
                if index >= len(rank):
                    raise ValueError("promoted marker without a piece")
                squares.append("+" + rank[index])
            else:
                squares.append(char)
            index += 1
    if len(squares) != 81:
        raise ValueError(f"SFEN board has {len(squares)} squares, expected 81")
    return squares


def hand_counts(hand):
    counts = {piece: 0 for piece in "RBGSNLP" + "rbgsnlp"}
    if hand == "-":
        return counts
    index = 0
    while index < len(hand):
        start = index
        while index < len(hand) and hand[index].isdigit():
            index += 1
        count = int(hand[start:index]) if index > start else 1
        if index >= len(hand) or hand[index] not in counts:
            raise ValueError("invalid SFEN hand")
        counts[hand[index]] += count
        index += 1
    return counts


def parse_sfen(state):
    parts = state.split()
    if len(parts) != 4 or parts[1] not in ("b", "w"):
        raise ValueError("state must be a complete SFEN string")
    return parse_board(parts[0]), parts[1], hand_counts(parts[2])


def sfen_distance(left, right):
    left_board, left_side, left_hand = parse_sfen(left)
    right_board, right_side, right_hand = parse_sfen(right)
    board_squared = 0.0
    for left_piece, right_piece in zip(left_board, right_board):
        if left_piece == right_piece:
            continue
        board_squared += 1.0 if left_piece is None or right_piece is None else 2.0
    # This is Euclidean distance over per-square one-hot piece vectors,
    # scaled so the largest board-only distance is 1. Side-to-move and hand
    # counts are separate caller-owned features; the move number is ignored.
    squared = board_squared / (81.0 * 2.0)
    if left_side != right_side:
        squared += 0.25
    squared += sum(
        ((left_hand[piece] - right_hand[piece]) / MAX_HAND_COUNT) ** 2
        for piece in left_hand
    )
    return math.sqrt(squared)


def prepare(train_rows, holdout_rows, max_neighbors):
    output = []
    for index, query in enumerate(holdout_rows):
        state = query["state"]
        neighbors = [
            {
                "state": candidate["state"],
                "distance": sfen_distance(state, candidate["state"]),
                "provenance": FEATURE_VERSION,
            }
            for candidate in train_rows
            if candidate["state"] != state
        ]
        neighbors.sort(key=lambda row: (row["distance"], row["state"], row["provenance"]))
        output.append(
            {
                "query_id": f"holdout-{index:04d}",
                "state": state,
                "expected_action": query["actions"][0]["action"],
                "neighbors": neighbors[:max_neighbors],
            }
        )
    return output


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("train_book")
    parser.add_argument("holdout_book")
    parser.add_argument("--out", required=True)
    parser.add_argument("--max-neighbors", type=int, default=8)
    args = parser.parse_args()
    if args.max_neighbors <= 0:
        raise ValueError("max-neighbors must be > 0")
    rows = prepare(load_book(args.train_book), load_book(args.holdout_book), args.max_neighbors)
    pathlib.Path(args.out).write_text(
        "".join(json.dumps(row, sort_keys=True) + "\n" for row in rows)
    )


if __name__ == "__main__":
    try:
        main()
    except (OSError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"Sekirei similarity adapter error: {error}")
