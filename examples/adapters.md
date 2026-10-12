# Typed adapter boundaries

`lineprior-adapters` keeps typed conversion at the boundary between a domain application and generic
observations. Typical shapes include:

- a structured position/move record
- a screen state to UI action record
- a task state to tool-call record
- an intermediate to transformation-template record

Values remain opaque to the core. Parsing, legality, execution, and domain validation stay in the
owning application. Each conversion adds a stable `source` and tag for source weighting.
