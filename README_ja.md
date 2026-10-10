# lineprior

[![crates.io](https://img.shields.io/crates/v/lineprior.svg)](https://crates.io/crates/lineprior)
[![docs.rs](https://img.shields.io/docsrs/lineprior)](https://docs.rs/lineprior)
[![CI](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml/badge.svg)](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/lineprior.svg)](https://github.com/kent-tokyo/lineprior/blob/main/LICENSE-MIT)

日本語 / [English](./README.md)

`lineprior`は、過去のaction sequenceから再現可能なpriorを構築するRustライブラリ／CLIです。

```text
state -> ranked candidate actions
```

利用側が最初に調べるactionを選ぶための候補順を返します。最善手を決めるoracleではなく、
online search、rule、evaluation、verificationを置き換えません。stateとactionはopaqueなkey
として扱い、未知stateや証拠の乏しいstateでは候補を返しません。

## 最初のprior bookを作る

```bash
cargo add lineprior
cargo install lineprior-cli
```

入力は1行1件のJSONLです。

```json
{"sequence_id":"case-001","step":0,"state":"state_a","action":"action_x","outcome":"success","score":0.8,"weight":1.0}
```

検証してからbookを構築し、要約と問い合わせを実行します。

```bash
lineprior validate observations.jsonl
lineprior build observations.jsonl --out prior.jsonl --min-count 2
lineprior verify-artifact --manifest manifest.json --input observations.jsonl --book prior.jsonl
lineprior summary prior.jsonl
lineprior query prior.jsonl --state state_a --top-k 5
```

未知stateへの問い合わせは、候補を出さずに正常終了します。fallbackは利用側で実装します。

## データ契約

各観測には空でない`sequence_id`、`state`、`action`と、0以上の`step`が必要です。次の
フィールドは省略できます。

| フィールド | 既定値 | 意味 |
| --- | --- | --- |
| `outcome` | `"unknown"` | `success`、`failure`、`draw`、`unknown` |
| `score` | `null` | 利用側が定義する有限値 |
| `weight` | `1.0` | 有限かつ0以上の観測weight |
| `tags` | `[]` | 利用側が定義するfilter |
| `observed_at_unix_seconds` | `null` | time decayを有効にした場合だけ使用 |
| `source` | `null` | source reliability weightに使用 |

strict modeは最初の不正行で停止します。通常モードは、安全に分離できる不正行を飛ばして
warningを返します。

`build`はschema-v1 metadata headerの後に、決定的に並べたstate entryを書きます。

```text
{"prior_book_schema_version":1,"producer_version":"0.12.3","build_config":{...},"build_config_fingerprint":7308171529403319118}
{"state":"state_a","actions":[{"action":"action_x","count":3,"weighted_count":3.0,"success_rate":0.667,"mean_score":0.633,"prior":0.557,"confidence":0.130}]}
```

headerにはproducer versionと完全な`BuildConfig`を保存します。headerのない旧bookと、
fingerprintだけを持つ旧headerも読み込めます。`pack`と`unpack`でJSONLとcompact LPB v1を
相互変換できます。

測定結果を保存する場合は`--artifact-manifest manifest.json`を指定します。sidecarは入力と
bookのbyte列をSHA-256で結び、dataset、split、adapter、lineageをbook schemaとは別に記録
します。`verify-artifact`はbyte hash、manifestの内容、schema-v1 book metadataを照合します。
旧形式またはheaderless bookの場合はprovenance不足をwarningで返します。artifact schemaと
検証方法は[`docs/README.md`](./docs/README.md)にまとめています。

## ranking、confidence、評価

既定のrankingは、weighted countの対数、平滑化したsuccess rate、平滑化したmean scoreを
組み合わせ、state内で正規化します。`--scoring-strategy`では`bayesian`、`ucb`、`softmax`
も選べます。いずれもranking方法であり、将来性能を保証しません。

`--confidence-mode`では次を選べます。

- `heuristic`: sample sizeに基づく既定の指標。outcomeがなくても使えます。
- `wilson-lower-bound`: 観測success rateの下限。判定可能なoutcomeがなければheuristicへ
  fallbackします。
- `hybrid`: heuristicとWilson下限の積です。

count、weighted count、confidenceのthresholdで、弱い証拠からabstainできます。time decayと
source weightはopt-inです。time decayには再現性を保つため、基準時刻を明示します。

held-out評価では、同じsequenceがtrainとtestにまたがらないよう`sequence_id`単位で分割します。

```bash
lineprior eval observations.jsonl \
  --split-by sequence --train-ratio 0.8 \
  --top-k 1,3,5 --calibration-bins 10 --out eval.json
```

ranking指標が良くてもdownstream改善の証明にはなりません。実際のdecision loopでprior on/offを
比較してください。

## CLIコマンド

| コマンド | 用途 |
| --- | --- |
| `build` | JSONL prior bookを構築 |
| `query` | exact stateまたはcontext backoffで問い合わせ |
| `summary` | coverage、confidence、entropy、context supportを要約 |
| `validate` | observation、off-policy、gate-history JSONLを検証 |
| `verify-artifact` | sidecarを入力byte列とprior-book metadataに照合 |
| `eval` / `tune` | 固定splitでconfigを評価または調整 |
| `pack` / `unpack` | JSONLとLPB v1を変換 |
| `offpolicy` | IPS、SNIPS、DR、overlap、bootstrap intervalを計算 |
| `gate` | 実験的GateModelのdiagnosticを実行 |

全optionは`lineprior <command> --help`で確認できます。

## 追加機能

- core libraryは、可変長context、sequence scoring、`PriorTrie`、bounded macro-action、
  terminal credit、source weight付きmulti-source mergeを提供します。
- `lineprior-similarity`は、利用側が用意した有限feature vectorを検索します。actionや
  embeddingは生成しません。
- `lineprior-adapters`は、Sekirei、UI automation、LLM agent、retrosynthesis向けのtypedな
  変換境界です。parse、legality、実行、chemical validationはdownstream側で行います。
- `lineprior-wasm`は、初期化後にJSON入出力のbuild/queryを提供します。Rust crateはworkspace
  と同時に公開し、CIではweb targetのtarballを空の環境へinstallしてChromiumで検証します。
  web packageはnpm registryには公開しておらず、Python bindingも提供していません。
- GateModelとoff-policy評価はdiagnosticです。欠けたfeature、support、propensity、outcomeは
  補えません。

coreはSFEN、CSA、FEN、PGN、prompt、UI tree、moleculeをparseしません。JSONL fixtureと
Python、Node.js、browserのround-trip例は[`examples/`](./examples/)にあります。

## 証拠と制約

- 履歴にbiasやdistribution shiftがあれば、priorは判断を悪化させる場合があります。
- `confidence`は観測supportまたは観測rateの下限であり、将来actionの確実性ではありません。
- 類似stateへの一般化は自動ではありません。feature設計は利用側の責務です。
- 因果推論やcounterfactual action generationは行いません。
- synthetic fixtureは契約を検証しますが、decision qualityを証明しません。過去のbenchmarkも
  別machineでの性能を保証しません。
- Sekireiのholdoutでは現行similarity adapterを不採用としました。既存logではIPS/DRと
  GateModelの検証に必要な情報が不足しています。Trieとmacro-actionのdownstream改善も
  未検証です。

streaming buildのmemoryは、uniqueなstate/action pair数と、有効時のcontext tuple数に比例
します。測定手順、実測結果、公開手順、example、schemaは[文書索引](./docs/README.md)から
参照できます。

## 開発

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
sh scripts/check_candidate_contract.sh
```

`lineprior`はcase-based planning、plan reuse、sequence prediction、variable-order Markov
model、policy-guided search、temporal abstractionを参考にしたengineering-orientedな実装です。
新しい理論algorithmは主張しません。

ライセンスはMITまたはApache-2.0です。
