# lineprior

[![crates.io](https://img.shields.io/crates/v/lineprior.svg)](https://crates.io/crates/lineprior)
[![docs.rs](https://img.shields.io/docsrs/lineprior)](https://docs.rs/lineprior)
[![CI](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml/badge.svg)](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/lineprior.svg)](https://github.com/kent-tokyo/lineprior/blob/main/LICENSE-MIT)

日本語 / [English](./README.md)

`lineprior`は、過去の行動列から再現可能なaction priorを構築するRustライブラリ／CLIです。

```text
state -> ranked candidate actions
```

「このstateでは、過去にどのactionがうまくいったか」を候補順として返します。最善手を
決定するoracleではありません。オンライン探索も行わず、search、rule、evaluation、
verificationを置き換えません。証拠が乏しいstateや未知stateでは候補を返さず、caller側の
fallbackに委ねます。

## インストール

```bash
cargo add lineprior
cargo install lineprior-cli
```

workspaceには、typed adapter、caller提供の類似検索、薄いWASM境界のcrateもあります。
core crateはこれらに依存しません。

## 最初のprior bookを作る

入力は1行1件のJSONLです。

```json
{"sequence_id":"case-001","step":0,"state":"state_a","action":"action_x","outcome":"success","score":0.8,"weight":1.0}
```

検証、構築、要約、問い合わせは次の順で実行できます。

```bash
lineprior validate observations.jsonl
lineprior build observations.jsonl --out prior.jsonl --min-count 2
lineprior summary prior.jsonl
lineprior query prior.jsonl --state state_a --top-k 5
```

未知stateへの問い合わせは候補を出さず、正常終了します。fallbackの方法はcallerが決めます。

## データ契約

### Observation JSONL

必須フィールド:

| フィールド | 型 | 意味 |
| --- | --- | --- |
| `sequence_id` | string | sequenceまたはcaseの識別子 |
| `step` | 0以上の整数 | sequence内の位置 |
| `state` | 空でないstring | opaqueなstate key |
| `action` | 空でないstring | opaqueなaction key |

省略可能なフィールド:

| フィールド | 既定値 | 意味 |
| --- | --- | --- |
| `outcome` | `"unknown"` | `success`、`failure`、`draw`、`unknown` |
| `score` | `null` | caller定義の有限値 |
| `weight` | `1.0` | 0以上の観測weight |
| `tags` | `[]` | caller定義のfilter |
| `observed_at_unix_seconds` | `null` | time decay有効時だけ使用 |
| `source` | `null` | source reliability weightに使用 |

strict modeは最初の不正行で停止します。通常モードは、安全に分離できる不正行をskipし、
warningを返します。

### Prior-book JSONL

`lineprior build`はschema-v1 metadata headerの後に、決定的に並べたstate entryを書きます。

```text
{"prior_book_schema_version":1,"producer_version":"0.12.1","build_config":{...},"build_config_fingerprint":7308171529403319118}
{"state":"state_a","actions":[{"action":"action_x","count":3,"weighted_count":3.0,"success_rate":0.667,"mean_score":0.633,"prior":0.557,"confidence":0.130}]}
```

headerにはproducer versionと完全な`BuildConfig`を保存します。Rust APIの
`load_prior_book_with_metadata`で取得でき、`load_prior_book_with_config`はschema-v1の
config不一致を拒否します。headerのない旧bookとfingerprintだけの旧headerも読めます。

同じ順序の入力、config、producer versionからはbyte単位で同じJSONLが得られます。
`pack`と`unpack`でJSONLとcompact LPB v1を相互変換できます。

## rankingとconfidence

既定のrankingは、weighted countの対数、平滑化したsuccess rate、平滑化したmean scoreを
組み合わせ、state内で正規化します。outcomeやscoreがない場合、その項は使いません。

`--scoring-strategy`では`weighted-sum`、`bayesian`、`ucb`、`softmax`を選べます。
いずれもranking方法であり、将来性能を保証しません。

`--confidence-mode`では次を選べます。

- `heuristic`: sample sizeに基づく既定の指標。outcomeがなくても使えます。
- `wilson-lower-bound`: 観測されたsuccess rateのWilson下限。判定可能なoutcomeがなければ
  heuristicへfallbackします。
- `hybrid`: heuristicとWilson下限の積です。

`--min-count`、`--min-weighted-count`、`--min-confidence`で弱い証拠を除外できます。
time decayとsource reliabilityはopt-inです。time decayでは再現性を保つため、基準時刻を
明示する必要があります。全設定は`lineprior build --help`で確認できます。

## コマンド一覧

| コマンド | 用途 |
| --- | --- |
| `build` | JSONL prior bookを構築 |
| `query` | exact stateまたはcontext backoffで問い合わせ |
| `summary` | coverage、confidence、entropy、context supportを要約 |
| `validate` | observation JSONLを検証 |
| `eval` | held-out rankingとcalibrationを測定 |
| `tune` | 同じsplitで`BuildConfig`をgrid search |
| `pack` / `unpack` | JSONLとLPB v1を変換 |
| `offpolicy` | IPS、SNIPS、DR、overlap、bootstrap intervalを計算 |
| `gate` | 実験的GateModelをfitし、verdict／acquisitionを予測 |

`eval`は同じsequenceがtrain/testにまたがらないよう、`sequence_id`単位で分割します。

```bash
lineprior eval observations.jsonl \
  --split-by sequence --train-ratio 0.8 \
  --top-k 1,3,5 --calibration-bins 10 --out eval.json
```

`tune`は全候補で同じsplitを使い、選択したconfigを保存できます。

```bash
lineprior tune observations.jsonl \
  --param confidence-mode=heuristic,wilson-lower-bound,hybrid \
  --param min-confidence=0.0,0.3,0.5 \
  --objective covered-mrr \
  --out tune.json --save-best-config best_config.json
```

held-out rankingが良くても、downstream改善を証明したことにはなりません。実際のdecision
loopでprior on/offを比較してください。

## 追加機能

- **可変長context:** `--context-order N`で直近action列を学習し、order-zeroへfallbackします。
  入力はsequenceごとにまとめ、stepを昇順にします。
- **類似state fallback:** `lineprior-similarity`はcallerが用意した有限feature vectorを検索
  します。coreは決定的なneighborを受け取るだけで、actionを生成しません。
- **sequence再利用:** caller提供pathのscore、`PriorTrie`、bounded macro-action、opt-inの
  terminal credit、明示的source weightによるbook mergeを提供します。
- **typed adapter:** `lineprior-adapters`はSekirei、UI automation、LLM agent、
  retrosynthesisのrecordをgeneric observationへ変換します。parse、legality、実行、
  chemical validationはdownstreamの責務です。
- **WASM:** `lineprior-wasm`はJSON入出力のbuild/queryを提供します。CIはweb targetの
  npm tarballを作り、空のconsumerへinstallしてChromiumでround-tripします。registryには
  未公開で、他browserは保守範囲外です。
- **GateModel:** verdict確率、costあたりexpected improvement、monotonic constraintは
  実験的diagnosticです。実履歴監査では共通の事前feature tableを構成できず、model比較は
  推定不能のためscopeを凍結しています。
- **off-policy評価:** IPS/DRはcaller提供のpropensityとreward-model値を使います。
  support不足や不正なpropensityを推定器は修復できません。

## domain例

coreはstate/actionをopaque keyとして扱います。

| domain | state | action |
| --- | --- | --- |
| UI automation | screen、DOM、OCR state | click、type、shortcut、wait |
| optimization | partial-solution key | branch、candidate expansion |
| game | position string／hash | legal action ID |
| LLM agent | task／tool context | tool call、plan step |
| retrosynthesis | molecule／intermediate fingerprint | reaction template |

repositoryには
[UI automation](./examples/ui_automation.jsonl)、
[将棋mapping](./examples/shogi_opening.jsonl)、
[Python](./examples/python/roundtrip.py)／
[Node.js](./examples/node/roundtrip.mjs)のCLI round-trip例があります。SFEN、CSA、FEN、
PGN、prompt、moleculeのparseはadapter側で行います。

Python bindingは提供しません。browser packageの保守範囲とclean-install証拠は
[runtime互換性文書](./docs/measurements/ecosystem-compatibility.md)に固定しています。

## 証拠と制約

- 履歴にbiasやdistribution shiftがあれば、priorは判断を悪化させる場合があります。
- `confidence`は観測supportまたは観測rateの下限です。将来actionの確実性ではありません。
- 類似stateへの一般化は自動ではありません。feature設計はcallerの責務です。
- 因果推論やcounterfactual action generationは行いません。
- synthetic fixtureは契約を検証するもので、decision qualityの証拠ではありません。
- 過去のbenchmarkは、別machineでの性能を保証しません。
- Sekirei実holdoutでは現行similarity adapterを不採用としました。既存の実ログには
  propensityがなくIPS/DRは推定不能です。GateModelもcandidate ID、group ID、共通の
  pre-gate featureが揃わず推定不能です。Trieとmacro-actionの実データ改善gateは未完了です。

streaming buildのmemoryは、uniqueなstate/action pair数と、有効時のcontext tuple数に比例します。
Apple M4での過去のsynthetic測定は[`docs/benchmarks/`](./docs/benchmarks/)に残し、
実データ証拠と分けています。

## 文書案内

- [文書索引](./docs/README.md)
- [similarity実データ測定手順](./docs/measurements/similarity-real-data.md)
- [IPS/DR実ログ測定手順](./docs/measurements/offpolicy-real-data.md)
- [GateModel実履歴readiness](./docs/measurements/gate-model-real-history.md)
- [runtime／WASM検証範囲](./docs/measurements/ecosystem-compatibility.md)
- [Sekirei downstream case study](./docs/case-studies/sekirei-opening-book.md)
- [公開手順](./docs/publishing.md)
- [変更履歴](./CHANGELOG.md)
- [Rust API](https://docs.rs/lineprior)

内部用の`ROADMAP.md`は公開しません。

## 開発

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
sh scripts/check_candidate_contract.sh
```

`lineprior`は、case-based planning、plan reuse、sequence prediction、variable-order Markov
model、policy-guided search、temporal abstractionを参考にしたengineering-orientedなRust実装
です。新しい理論algorithmを主張しません。

ライセンスはMITまたはApache-2.0です。
