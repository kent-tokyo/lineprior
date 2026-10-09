# Typed adapter boundaries

`lineprior-adapters` converts four domain records into generic observations:

- `sekirei::Record`: SFEN to USI action
- `ui_automation::Record`: screen state to UI action
- `llm_agent::Record`: task state to tool call
- `retrosynthesis::Record`: intermediate to reaction template

Values remain opaque to the core. Parsing, legality, execution, and chemical validation stay in
the owning application. Each conversion adds a stable `source` and tag for source weighting.
