# Sekirei opening-book case study

This case study verifies the downstream boundary, not a strength gain. Sekirei owns CSA parsing,
SFEN/USI conversion, legality checks, and search fallback. `lineprior` only aggregates the emitted
opaque state/action observations and ranks candidates.

## Reproduction

The 2026-10-03 diagnostic used Sekirei commit
`ea06a1e2c7b87f57d8912c8932cc658283583982` and its public Floodgate-derived
`gateA_csa_subset` (500 files; aggregate content-manifest SHA-256 recorded in the artifact).

```sh
cargo run -p sekirei-train -- \
  --games <gateA_csa_subset> --min-rate 0 \
  --build-book <book.jsonl> --book-max-ply 30 --book-min-count 5
```

Take the first 20 states from the deterministically sorted book as `openings.sfen`, then run the
same release engine on both arms for two games per position. Keep all options equal except:

```text
on:  UseBook=true BookFile=<book.jsonl> BookMaxPly=30 BookMinConfidence=0.20
off: UseBook=false
```

The remaining controls were `Threads=1`, `SpecTopN=0`, `byoyomi=20ms`, and `max-moves=256`.
Sekirei alternated engine colours within each position pair. Verify the on arm's initial move
against the book top action whenever engine1 owns the initial side to move; this prevents an
accidental off/off comparison caused by a bad option value.

## Result and decision

The on arm scored 15 wins, 10 draws, and 15 losses in 40 games: score 0.500, estimated Elo 0 with
a 95% interval of approximately [-108, +108]. All 20 eligible initial turns matched the book's
top action. The integration and normal-search fallback therefore ran, but this small, in-sample,
20 ms diagnostic does not show downstream improvement.

Keep the book optional. Do not change defaults or make a strength claim from this run. A future
adoption gate needs a held-out opening corpus, production time control, and a materially narrower
interval. The machine-readable evidence is
[`sekirei-opening-book-2026-10-03.json`](sekirei-opening-book-2026-10-03.json).
