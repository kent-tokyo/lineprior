# Official adapters

`lineprior-adapters` provides four typed, dependency-free conversion
boundaries:

* `sekirei::Record`: SFEN → USI move
* `ui_automation::Record`: screen state → UI action
* `llm_agent::Record`: task state → tool call
* `retrosynthesis::Record`: intermediate → reaction template

The values are intentionally opaque. Parsing, legality, tool execution, and
chemical validation stay in the owning application. Each conversion adds a
stable `source` and tag for `BuildConfig::source_weights`.
