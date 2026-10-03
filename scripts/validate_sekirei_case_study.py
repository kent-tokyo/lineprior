#!/usr/bin/env python3
"""Validate the checked-in Sekirei downstream case-study summary."""

import argparse
import json
import math
import pathlib
import re

from version_contract import workspace_version


def require(condition, message):
    if not condition:
        raise ValueError(message)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("report")
    parser.add_argument("--expected-lineprior-version", default=workspace_version())
    args = parser.parse_args()
    report = json.loads(pathlib.Path(args.report).read_text())
    require(
        report.get("protocol") == "sekirei-lineprior-book-case-study-v1",
        "unexpected protocol",
    )
    require(report.get("status") == "complete", "case study is not complete")
    require(
        report.get("decision") == "inconclusive_keep_optional",
        "unsafe adoption decision",
    )
    require(
        report["lineage"].get("lineprior_version") == args.expected_lineprior_version,
        "version mismatch",
    )

    controls = report["protocol_controls"]
    result = report["result"]
    games = controls["games"]
    require(
        games == controls["opening_positions"] * controls["games_per_position"],
        "game count mismatch",
    )
    require(
        result["book_on_wins"] + result["draws"] + result["book_on_losses"] == games,
        "WDL mismatch",
    )
    expected_score = (result["book_on_wins"] + 0.5 * result["draws"]) / games
    require(math.isclose(result["book_on_score"], expected_score), "score does not match WDL")
    require(
        result["elo_ci_low"] <= 0.0 <= result["elo_ci_high"],
        "inconclusive decision requires CI crossing zero",
    )
    require(
        result["top_action_matches"] <= result["eligible_initial_book_turns"],
        "book hits exceed eligible turns",
    )
    require(result["eligible_initial_book_turns"] > 0, "no verified book use")

    for name, digest in report["lineage"]["sha256"].items():
        require(bool(re.fullmatch(r"[0-9a-f]{64}", digest)), f"invalid SHA-256 for {name}")
    require(
        bool(re.fullmatch(r"[0-9a-f]{64}", report["dataset"]["content_manifest_sha256"])),
        "invalid dataset hash",
    )
    print("Sekirei downstream case-study contract: ok")


if __name__ == "__main__":
    try:
        main()
    except (KeyError, OSError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"Sekirei case-study error: {error}")
