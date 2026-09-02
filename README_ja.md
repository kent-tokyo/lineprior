# lineprior

[![crates.io](https://img.shields.io/crates/v/lineprior.svg)](https://crates.io/crates/lineprior)
[![docs.rs](https://img.shields.io/docsrs/lineprior)](https://docs.rs/lineprior)
[![CI](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml/badge.svg)](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/lineprior.svg)](https://github.com/kent-tokyo/lineprior/blob/main/LICENSE-MIT)

日本語 / [English](./README.md)

`lineprior` は **説明可能な行動ランキング(explainable action ranking)** のための Rust ライブラリおよび CLI です。過去の `(state, action, outcome)` 系列から、再現可能な **行動事前分布(action prior)** — 状態ごとにランク付けされ confidence の付いた候補行動のリスト — を、オンライン学習ではなくオフラインの過去データから構築します。ある状態が与えられたとき、次の問いに答えます。

> この状態から、過去にどの行動がうまくいったか?

探索・プランニング・エージェント・ゲーム・最適化向けに作られています — 手/候補の順序付け、定跡的な過去実績ガイダンス、あるいはどの高コストな実験を先に試すべきかのランク付けなど。confidence を意識した abstention-friendly な設計で、データが少ない状態や未知の状態では、推測ではなく候補を返しません(その selective-prediction の仕組みは後述の「Confidence モード」参照)。将棋の定跡帳ライブラリでも、チェス専用の定跡フォーマットでも、プランナーでも、ソルバーでも、ゲームエンジンでも、**contextual bandit / 強化学習 / オンライン学習ライブラリでもありません** — policy learning もexplorationもオンライン更新も行いません。その違いが気になる場合は後述の「lineprior と contextual bandit の違い」を参照してください。

## lineprior を使うべき場面

過去の `(state, action, outcome)` 系列があり、探索・プランニング・シミュレーション・検証の前段で、説明可能で再現性のある候補行動ランキングが欲しい場合に使ってください:

- **探索の手の順序付け** — 候補となる手/行動を、探索する優先順で並べる
- **プランナーの候補順序付け** — どの候補ステップを先に展開すべきか優先順位を付ける
- **エージェントの行動事前分布** — エージェントに候補行動の初期ランキングを与える
- **最適化の分岐順序付け** — 過去に成功した経路をもとに、どの分岐を先に試すか決める
- **定跡的な過去実績ガイダンス** — 過去の対局/実行のパターンを再利用する(`lineprior` 自体はゲーム固有・ドメイン固有の知識を持ちません)
- **高コストな実験/gate候補の優先順位付け** — どの候補が高コストな実行や検証に値するか順位付けする

## これではないもの

`lineprior` は単独で最善の行動を決定しません。これは **oracle ではなく prior** です:

- count・rate・confidence の付いた候補行動を提案します。
- 呼び出し側が、探索・評価・ルール・検証などと組み合わせて使うことを前提としています。
- データが少ない場合や未知の状態に対しては、行動を創作するのではなく、候補を返しません。

過去データに偏りがあれば、prior にも偏りが反映されます。`lineprior` は、過去の系列が関連性・代表性を持つ場合に候補の順序付けを改善しますが、より良い意思決定を保証するものではありません。

## lineprior と contextual bandit の違い

contextual bandit を探してここに辿り着いた場合、実際の違いはこうです:

- **contextual bandit**(LinUCB や Thompson Sampling など)は、オンラインでpolicyを学習します: explorationを行い、フィードバックから更新し、実行しながらexploration/exploitationのバランスを取ります。
- `lineprior` は、静的な過去ログから**オフラインで一度だけ** prior を構築します。bandit やreinforcement learningのアルゴリズムは一切実装しておらず、オンラインexplorationもオンラインpolicy更新も行いません。
- `lineprior` はそれ単体でpolicyやsolverではありません — 探索・プランナー・bandit・solverの**前段**に置くランキング/confidenceコンポーネントです。ちょうどチェスの定跡帳が探索エンジンを置き換えることなく手助けするのと同じ関係です。

これらは競合するものではなく補完関係にあります: `lineprior` がランク付けした候補は、banditの初期arm集合やプランナーの手の順序付けの入力になり得ますが、`lineprior` 自体がオンラインでexplorationしたり学習したりすることはありません。

## prior book を構築する

```bash
lineprior build observations.jsonl \
  --out prior.jsonl \
  --min-count 1 \
  --smoothing-alpha 5.0
```

主なフラグ: `--max-step`(指定した step を超える観測を除外)、`--max-actions-per-state`(状態ごとに上位 N 件のみ保持)、`--tags`(指定したタグのいずれかを持つ観測のみを対象、カンマ区切り)、`--confidence-k`(サンプル数に対する confidence の伸び方を調整)、`--confidence-mode`(`heuristic`(デフォルト)、`wilson-lower-bound`、`hybrid` — 詳細は下記「Confidence モード」)、`--confidence-z`(Wilson lower bound の z 値、デフォルト `1.96`。`heuristic` では無視される)、`--min-weighted-count` / `--min-confidence`(生の `--min-count` の代わりに、weighted count や confidence 自体でフィルタリング)、`--draw-value`(`draw` outcome に与える成功クレジット — デフォルト `0.5`。draw は敗北ではなく、対戦ゲームにおける正当な部分的結果であるため)、`--time-decay-half-life-days` / `--time-decay-reference-unix-seconds` / `--missing-timestamp-policy`(経過時間に基づく重みの減衰 — 詳細は下記「Time decay と source reliability」)、`--source-weights` / `--default-source-weight`(source ごとの信頼度倍率、同セクション)、`--count-weight` / `--success-weight` / `--score-weight`(raw prior score 内の各項の重み — `count_weight * ln(1 + weighted_count) + success_weight * success_rate + score_weight * mean_score`。いずれもデフォルト `1.0`。`tune --param` のキーとしても指定可能 — 詳細は下記「Tuning」)、`--config <path.json>`(個々のフラグの代わりに `BuildConfig` 全体をファイルから読み込む。例えば `lineprior tune --save-best-config` が保存したファイル — 詳細は下記「Tuning」。上記のいずれかのフラグと組み合わせるとエラーになる)、`--strict`(不正なレコードを警告付きでスキップせず、最初の1件で失敗させる)。

`--min-confidence` の意味は `--confidence-mode` に依存します: `heuristic` では出典(outcome)を見ないサンプルサイズだけの下限ですが、`wilson-lower-bound`/`hybrid` では成功率を反映するようになるため、これまで閾値を通過していた「件数は多いがほとんど失敗している」行動が弾かれるようになることがあります — `--confidence-mode` の切り替えは、既存の `--min-confidence` 閾値に対して単なる追加ではなく実際の挙動変化です。

### Confidence モード

- `heuristic`(デフォルト): `weighted_count / (weighted_count + confidence_k)` — outcome を見ないサンプルサイズのヒューリスティック。統計的な保証ではありませんが、outcome ラベルが一切ない score のみのデータセットでも機能します。
- `wilson-lower-bound`: 行動の成功率に対する Wilson score interval の下限 — `outcome` ラベルに意味がある場合に有用な、実際の統計的下限です。decisive な outcome の観測が一つもない行動では `heuristic` にフォールバックします(下限を計算する材料がないため)。
- `hybrid`: `heuristic * wilson-lower-bound`。サンプルサイズが小さいことと成功率が低いことの両方が confidence を押し下げます。outcome データがない場合のフォールバックは `wilson-lower-bound` と同じです。

weight を持つ/fractional な観測(`--weight`、`--draw-value` による `draw` outcome)は、生の weighted count ではなく有効サンプルサイズ(`sum(weight)^2 / sum(weight^2)`、Kish の式)を介して Wilson lower bound に反映されます — weight が一律 `1.0` の観測では厳密な値と一致する、工学的な近似です。

### Time decay と source reliability

すべての観測が等しく信頼できるわけではありません。`build`/`eval` は観測ごとに `effective_weight`(`weight * time_decay_multiplier * source_reliability_multiplier`)を計算でき、これは `prior`・`confidence`・eval のキャリブレーションなど下流のすべてに自動的に反映されます。どちらの係数もデフォルトでは何もしない(no-op)ので、完全にオプトインです。

経過時間で減衰させる(古いデータ):

```bash
lineprior build observations.jsonl \
  --out prior.jsonl \
  --time-decay-half-life-days 30 \
  --time-decay-reference-unix-seconds 1783540000
```

`--time-decay-half-life-days` を設定する場合、`--time-decay-reference-unix-seconds` は**必須**です — 暗黙の「現在時刻」は使いません。もしそうすると、同一の build/eval コマンドを実行するタイミングによって prior(および `build_config_fingerprint`)が変わってしまうためです。観測の `weight` は `0.5 ^ (age_days / half_life_days)` に従って減衰し、未来日時の観測(`observed_at_unix_seconds` が reference より後)は経過日数 `0` として黙ってクランプされます。`--missing-timestamp-policy`(デフォルト `keep-base-weight`、または `drop`)は、`observed_at_unix_seconds` を持たない観測をどう扱うかを決めます — decay が無効なら無視されます。

信頼度の異なる複数の source:

```bash
lineprior build observations.jsonl \
  --out prior.jsonl \
  --source-weights engine_v012=1.0,engine_v010=0.6,human=0.8 \
  --default-source-weight 1.0
```

観測の `source` フィールドは `--source-weights` で引かれます。`source` が未指定または未知の場合は `--default-source-weight`(デフォルト `1.0`、つまり他と同様に信頼する)にフォールバックします。これは time decay とは独立しているため、どちらか一方だけ、両方、あるいはどちらも使わない、という選択ができます。

**注意点:** Kish の有効サンプルサイズ(上記の Wilson lower bound と同じ式)は、ある行動自身の全ての weight を同じ係数で一律にスケールしても変化しません。そのため、ある行動を支える観測がすべて同じ age/source を持つ場合、純粋な `wilson-lower-bound` の confidence は decay を全く反映しません — 反映されるのは `weighted_count`(したがって `prior`、および `heuristic`/`hybrid` の confidence)だけです。古い・信頼度の低いデータに対して `confidence` の数値自体を下げたい場合は、単独の `wilson-lower-bound` ではなく `hybrid` を使ってください。

`weight` を自分で事前計算して `lineprior` に渡すことも常に可能です — この機能は、よくあるケース(age による decay、source による割引)を再現可能にし、config のフィンガープリントに組み込むために存在するのであって、独自の重み付けロジックの代替ではありません。

`build` は、自身のフィルタが実際に何をしたかを1行で表示するようにもなりました。例: `stats: 950/1000 observations kept, 42/50 candidates kept (5 by min_count, ...)` — 自分側の事前フィルタ(ドメイン固有の ply/深さカットオフなど)が `--min-count` などと合わせて期待どおりに機能しているか、手計算せずに確認できます。ライブラリとしては、`build_prior_book_from_reader` が book と一緒に返す `BuildOutput.stats`(`BuildStats`)がこれに当たります。

## prior book を問い合わせる

```bash
lineprior query prior.jsonl --state state_a --top-k 5
```

未知の状態に対しては何も出力せず、それでも終了コードは `0` です — これはエラーではなく、想定されたフォールバック挙動です。

`--recent-actions action_x,action_y` を付けるとコンテキストを考慮したクエリになります(下記「可変長コンテキスト」参照)— 出力は候補1件ごとの行ではなく `{"matched_order": N, "candidates": [...]}` になります。

ライブラリとしては、`PriorBook::candidates()` を使うと、book 全体の `(state, action)` 候補をフラットな `Vec<(String, PriorAction)>` として取得できます。`entries_sorted()` が返すネストした状態ごとの構造を自分でたどる代わりに、候補を直接フィルタ・サンプリングしたい呼び出し側(ドメイン固有の「定跡集」を作る場合など)向けです。

## その他のコマンド

```bash
lineprior summary prior.jsonl      # カバレッジ、平均confidence、状態ごとのentropy
lineprior validate observations.jsonl   # 構築せずに入力をパースして問題を報告
```

## 入力スキーマ

1行に1つのJSONオブジェクト:

```json
{"sequence_id":"case-001","step":0,"state":"state_a","action":"action_x","outcome":"success","score":0.8,"weight":1.0,"tags":["trusted"],"observed_at_unix_seconds":1783540000,"source":"engine_v012"}
```

必須: `sequence_id`, `step`, `state`, `action`。
任意(デフォルト値あり): `outcome`(`unknown`)、`score`(`null`)、`weight`(`1.0`)、`tags`(`[]`)、`observed_at_unix_seconds`(`null`。time decay が有効な場合のみ参照される — 上記「Time decay と source reliability」参照)、`source`(`null`。`--source-weights` 経由でのみ参照される)。

## 出力スキーマ

状態ごとに1つのJSONオブジェクト。actions は prior の降順でランク付けされます:

```json
{"state":"state_a","actions":[{"action":"action_x","count":3,"weighted_count":3.0,"success_rate":0.667,"mean_score":0.633,"prior":0.557,"confidence":0.130}]}
```

`success_rate` と `mean_score` は生の(平滑化されていない)観測レート(透明性のため)、`prior` は平滑化・正規化されたランキングスコア、`confidence` はデフォルトではヒューリスティックなサンプルサイズの指標ですが、`--confidence-mode wilson-lower-bound`/`hybrid` では実際の Wilson lower bound による統計的な下限になります(上記「Confidence モード」参照)。`success_rate` は `success` を 1.0、`draw` を `--draw-value`(デフォルト 0.5)、`failure` を 0.0 としてクレジットします。

`lineprior build` の CLI 出力(およびライブラリの `save_prior_book_with_config`)は、構築に使った `BuildConfig` のフィンガープリントを持つヘッダー行を先頭に付加するようになりました(例: `{"build_config_fingerprint":7592859384087124328}`)。`load_prior_book` / `lineprior query` / `lineprior summary` はいずれもこの行を透過的にスキップします — 日常的な読み取り方法は変わりません。

`--context-order` > 0 の場合、一部の行に `context` フィールドが追加されます — 下記「可変長コンテキスト」参照。

## キャッシュした prior book の古さを検知する

prior book をディスクにキャッシュし、後で異なる `BuildConfig`(異なる `--smoothing-alpha`、`--confidence-k` など)で再構築した場合、古いファイルの生の `confidence`/`prior` の数値は*古い*設定の意味論で計算されたものです — それを黙って再利用すると誤解を招きかねません。ライブラリとしては:

```rust
// 保存時に、構築に使った config を埋め込む:
save_prior_book_with_config(&book, &config, writer)?;

// 後で、信頼する前にキャッシュファイルを現在の config と突き合わせる:
match load_prior_book_with_config(reader, &config) {
    Ok(book) => { /* config が一致(またはこのチェック以前のファイル) */ }
    Err(Error::BuildConfigMismatch { .. }) => { /* 古い -- 再構築が必要 */ }
    Err(e) => { /* その他のエラー */ }
}
```

プレーンな `save_prior_book`(または、この機能より前のバージョンの lineprior)で保存されたファイルにはフィンガープリントがないため、`load_prior_book_with_config` は無条件に受け入れます — 比較対象がないからです。フィンガープリントは*特定の lineprior バージョン内で*安定することが保証されていますが、バージョンをまたいで永続的に安定するとは保証されません(`BuildConfig` の JSON エンコーディングをハッシュしており、浮動小数点の正確なバイト表現自体がバージョン間で保証されるものではないため)— これは1つのプロジェクトのライフタイム内でキャッシュの古さを検知するためのものであり、長期のアーカイブ用チェックサムではありません。

新しい `BuildConfig` フィールド(`confidence_mode`/`confidence_z`、`time_decay_half_life_days`/`source_weights`、`context_order` など)を追加したバージョンの lineprior にアップグレードすると、新フィールドが無効なデフォルト値(`heuristic` モード、decay 無効、source weights なし)であっても、*すべての* config でフィンガープリントが変わります — そのため、アップグレード前にキャッシュした prior book は、アップグレード後に一度だけ `BuildConfigMismatch` を発生させます。これはフィンガープリント機構が意図通りに動作しているだけで、不具合ではありません。

## 制約事項

- デフォルト(`--confidence-mode heuristic`)では、confidence はサンプルサイズのヒューリスティック(`weighted_count / (weighted_count + k)`)であり、統計的な信頼区間ではありません。これは後方互換性のため、また outcome ラベルのない score のみのデータセットのためにデフォルトのままです。`--confidence-mode wilson-lower-bound`/`hybrid` は、outcome データに意味がある場合に実際の統計的下限を与えます(上記「Confidence モード」参照)— ただしこれらもあくまで*観測された*成功率に対する下限であり、元データに偏りや非定常性があれば将来の行動を保証するものではありません。
- サンプル数が少ない行動は、1件の観測で成功率100%であっても確実なものとしては報告されません — 平滑化によってデータセット全体のレートに引き寄せられます。
- `lineprior` は行動を創作しません: 未知の状態や、閾値を超える候補が存在しない状態は、空の結果を返します。
- 本ライブラリはドメイン固有のフォーマット(SFEN、CSA、USI、FEN、PGN など)を一切パースしません — そのマッピングは呼び出し側の責務です。

## 2つのドメインの例

同じ `observations.jsonl` の形式は、「state」が盤面であってもUI画面であっても機能します:

```text
自動化 (Automation):
  state  = "checkout_page"
  action = "click_pay_button"

最適化 (Optimization):
  state  = "partial_solution_hash_42"
  action = "branch_left"
```

ドメイン固有のマッピング(例: チェス/将棋の局面を `state` に、UCI/USI の指し手を `action` にする等)は、このクレートの外側のアダプタに属するものであり、`lineprior` 本体には含まれません。

実際のドメイン例として: [`examples/shogi_opening.jsonl`](./examples/shogi_opening.jsonl) は `state` = SFEN文字列、`action` = USIの指し手というマッピングを使用しています。これは AGENTS.md の Sekirei 統合に関する記述と同じマッピングです。生成された prior([`examples/shogi_prior.jsonl`](./examples/shogi_prior.jsonl))では、`2g2f` の方が生の観測レートが高い(100% 対 83%)にもかかわらず、`7g7f` が上位にランクされています — `7g7f` の方が裏付けとなる観測が1件多く、平滑化によって、`2g2f` の少数サンプルによる完璧な記録だけで上位に来ることを正しく防いでいます。

非ゲーム分野の [`examples/ui_automation.jsonl`](./examples/ui_automation.jsonl) では、`cart-empty` のような画面状態を `click:add-to-cart` のような UI 操作へマッピングしています。同じ CLI ラウンドトリップを [`examples/python/roundtrip.py`](./examples/python/roundtrip.py) と [`examples/node/roundtrip.mjs`](./examples/node/roundtrip.mjs) で確認できます。両方とも、繰り返しビルドのバイト単位の決定性と、構築した book から期待する操作を取得できることを検証します。これらは言語バインディングではなく CLI 統合例です。保守された Python/WASM パッケージが必要になるまでは、Rust CLI を正本実装とします。

## WASM / JavaScript 境界

workspace の `lineprior-wasm` crate は、`wasm-bindgen` による薄い2つの関数を提供します。`build_json` は
JSONL observation とシリアライズした `BuildConfig` を受け取り、ソート済み entries、warnings、build stats を
含む JSON を返します。`query_json` は JSONL prior book を受け取り、ランキング済み候補を返します。Rust の
scoring を正本として利用し、不正入力は JavaScript error にします。ファイル I/O やドメイン固有の state 表現は
持ちません。npm/wasm-pack のパッケージ化とブラウザ smoke test はまだ完了扱いにしていません。

## パフォーマンス

Apple M4(macOS 26.5.1)、release ビルドで測定。100万件の観測、50,000個のユニークな `(state, action)` ペア(1,000状態 × 50行動):

```text
wall-clock:        1.71s
peak RSS:          ~15.4 MB
```

再現方法:

```bash
awk 'BEGIN{
  for (s=0; s<1000; s++) for (a=0; a<50; a++) for (i=0; i<20; i++)
    printf "{\"sequence_id\":\"seq_%d_%d_%d\",\"step\":0,\"state\":\"state_%05d\",\"action\":\"action_%03d\",\"outcome\":\"%s\",\"score\":%.2f,\"weight\":1.0}\n", \
      s, a, i, s, a, (i % 3 == 0 ? "failure" : "success"), 0.5 + (i % 10) * 0.01
}' > large.jsonl
cargo build --release
time ./target/release/lineprior build large.jsonl --out /dev/null --min-count 1
```

メモリ使用量は、AGENTS.md の MVP パフォーマンス目標どおり、総観測数ではなくユニークな `(state, action)` ペア数に比例して有界になりました。CLI の `build` コマンドは、`build_prior_book_from_reader` を使って入力ファイルから prior book へ直接ストリーミングし、`Vec<Observation>` を先に集めるのではなく、パースした端から各観測を有界なアキュムレータへ畳み込みます。上記の計測でピークRSSは(以前の、全展開していたパスの)~199MBから~15.4MBへ低下しました — 同じ100万件の観測入力・同一の出力で、約13分の1です。

チェックイン済みの小規模なベンチマークは `crates/lineprior/benches/scoring.rs` にあります(`cargo bench -p lineprior` で実行)。一括読み込み型の `build_prior_book` とストリーミング型の `build_prior_book_from_reader` の両方を、1,000 / 10,000 / 50,000 件の観測規模でカバーしています。専用のリグレッションテスト(`crates/lineprior/tests/streaming_memory.rs`、Linux限定、CIで実行)は、ピークメモリが以前の観測数比例のスケーリングに戻った場合に失敗するようになっています。

## prior の性能を評価する

prior は、まだ見ていないデータに対しても実際の行動を上位にランクできて初めて意味があります。
`lineprior eval` は観測ログの一部を保留(held-out)にし、残りから prior を構築し、保留分に対する
ランキング品質の指標を報告します。

```bash
lineprior eval observations.jsonl \
  --split-by sequence --train-ratio 0.8 --top-k 1,3,5 --out eval.json
```

分割は個々の観測単位ではなく `sequence_id` 単位で行います。同じ系列のすべてのステップを同じ側に
揃えることで、後のステップが前のステップの情報を train/test の境界を越えて漏らすことを防ぎます。
分割は id の決定的なハッシュに基づくため、同じ `--train-ratio` で再実行すれば同じ分割が再現され
ます。

JSON レポートの主要なフィールド:

- `top1_hit_rate` / `topk_hit_rate`: prior が何らかの候補を返せたテスト観測のうち、実際に取られ
  た行動が prior の1位予測(またはtop-k以内)だった割合。
- `mean_reciprocal_rank`: 同じ考え方を順位で平均したもの(`1/順位`、候補に入っていなければ `0`)。
  ヒット/ミスの二値判定より緩やかなシグナルです。
- `success_weighted_top1_hit_rate` / `success_weighted_mean_reciprocal_rank`: 同じ2つの指標を、各
  テスト観測の outcome クレジットで重み付けしたもの(勝ちは満額、引き分けは `--draw-value` 分、負
  け・未記録は 0 で加重平均から完全に外れます)。`top1_hit_rate` は結果的に失敗した行動への一致で
  も加点されてしまいますが、こちらは「実際にうまくいった試行に限定した一致率」になります。テスト
  観測が1件もプラスのクレジットを得なかった場合は `None`。
- `failure_agreement_top1_hit_rate`: その対となる指標 — outcome が正確に `failure` だったテスト観
  測に限定した `top1_hit_rate`。値が高いと、prior の1位予測が失敗が判明している行動と一致している
  という警告サインです。テスト観測に `failure` が1件もなければ `None`。
  **注意:** これら3つの指標はいずれも、各観測「自身」の `outcome` フィールドでクレジット/減点を
  行うものであり、シーケンス全体の最終結果によるものではありません。もし記録側が最終的な結果を全
  ステップにコピーして記録している場合、最終的に負けたシーケンス内の序盤の好手も失敗として扱われ
  ます。これは `outcome` の記録方法に起因する性質であり、これらの指標側で補正できるものではあり
  ません。
- `coverage` と `fallback_rate`: これらは意図的に合計が1になりません。`coverage` は状態重み付け
  (prior が何らかの候補を返せた「異なるテスト状態」の割合)、`fallback_rate` は観測重み付け(候補
  が1つもなかった「テスト観測」の割合)です。滅多に出現しない候補なし状態は `fallback_rate` をほ
  とんど動かしませんが、`coverage` は丸ごと1点分下げます — レポートには各レートの元になった生の
  カウントも含まれているので、どちらの見方でも直接検算できます。

`lineprior eval --help` で `build` と同等のチューニングフラグ(`--min-count`、
`--smoothing-alpha`、`--confidence-mode`、`--time-decay-half-life-days`、`--source-weights` など)が
一覧できます — `eval` は実際の `build` 実行と同じノブで train 側の prior を構築するため、両者は比較
可能なままです。

### Confidence のキャリブレーションと閾値スイープ

`--calibration-bins`/`--thresholds` を使うと、`eval` は selective prediction のツールになります:
「prior 全体の性能」ではなく「confidence が X 以上のときだけ信用するなら、どれだけのデータに対して
判断でき、その精度はどれくらいか」に答えられます。

```bash
lineprior eval observations.jsonl \
  --confidence-mode wilson-lower-bound \
  --calibration-bins 10 \
  --thresholds 0.3,0.5,0.7,0.9
```

- `confidence_calibration`(`--calibration-bins N` から): `[0, 1]` を等幅に分割した `N` 個のビン。
  各ビンに何件入ったかによらず、常にちょうど `N` 件を返します。各ビンには、#1候補の confidence が
  そのビンに収まった評価対象テスト観測について `top1_hit_rate`/`mean_reciprocal_rank` が報告されま
  す — confidence が適切に較正されていれば、ヒット率はビンの confidence とおおよそ1対1で連動する
  はずです。
- `threshold_sweep`(`--thresholds` から): 指定した閾値ごとに1件、指定順で返します。
  `covered_fraction` は、状態に候補があり、かつ #1 候補の confidence が `min_confidence` 以上だっ
  た「全テスト観測」に対する割合です。`abstained_fraction = 1.0 - covered_fraction` です。**これら
  は上記の `coverage`/`fallback_rate` とは異なる重み付けです** — こちらはどちらも観測重み付けで、
  構造上合計が1になりますが、トップレベルの2つは意図的にそうなりません。各エントリの
  `top1_hit_rate`/`mean_reciprocal_rank` は「カバーされた」観測のみで計算されます(予測を実際に
  行った場合の精度)。これはヘッドラインの指標がすでに使っている「評価対象に限定する」という考え方
  と同じです。

どちらも明示的にリクエストしない限り省略され(空配列)、既存の `eval` の使い方には影響しません。

## オフポリシー評価(明示的なオプトイン)

別の評価モジュールとして `evaluate_self_normalized_ips` も提供します。呼び出し側は、ログを生成した
policy の propensity と、実際にログに記録された action に対して評価対象 policy が割り当てる確率を明示的
に渡す必要があります。`lineprior` は prior から反実仮想の reward を推測しません。レポートには通常の IPS、
self-normalized IPS、support fraction、overlap failure 数、Kish の有効サンプルサイズが含まれます。評価対象
policy の確率が 0 の行や、任意の上限を超える importance weight は、損失として黙って扱わず overlap failure
として報告します。
`evaluate_doubly_robust` は、評価対象 policy の期待 reward と、ログに記録された action の reward を予測する
モデル値を呼び出し側が両方渡せる場合に利用できます。モデルのベースラインに propensity で重み付けした残差
補正を加えます。overlap のない行はベースラインだけを使い、support 診断には残します。

`bootstrap_self_normalized_ips` は、IPS と self-normalized IPS の決定的なパーセンタイル区間を追加します。
seed、反復回数、信頼水準は明示的に指定し、support のない再標本は 0 の reward にせずスキップ件数として
記録します。これは再現可能な不確実性確認を可能にしますが、実データによる保留評価の代替ではありません。

 同じ診断は CLI の `lineprior offpolicy log.jsonl --out report.json` からも利用できます。全行に2つの報酬モデル
値がある場合は `--doubly-robust`、決定的な区間には `--bootstrap-resamples N --bootstrap-seed S` を指定します。
入力は1行1件の `OffPolicyObservation` JSONL で、不正な行や propensity は終了コード3になります。

チェックイン済みの [`examples/offpolicy.jsonl`](./examples/offpolicy.jsonl) は、正常な入力境界を示す小さなfixtureです。
例えば次のように実行できます。

```bash
lineprior offpolicy examples/offpolicy.jsonl --out /tmp/lineprior-offpolicy.json \
  --doubly-robust --bootstrap-resamples 128 --bootstrap-seed 42
```

これらは推定器と監査用の情報であり、因果的な改善の証明ではありません。正しい propensity、overlap、不確実性
区間、保留データでの downstream 比較は呼び出し側の責務です。報酬モデル自体を `lineprior` が学習・推測する
ことはありません。

## 可変長コンテキスト

デフォルトの prior は order-0 です: `state -> action` のみで、シーケンス内で以前に何が起きたかを
一切覚えていません。`--context-order k` を指定すると、order `1..=k` の `(直近k手, state) ->
action` も追加で学習します。これは各シーケンス自身の `sequence_id`/`step` 履歴から自動的に導出さ
れます — スキーマ変更や新しい observation フィールドは不要です。`0`(デフォルト)はこの機能を完全
に無効化し、既存のすべての book・config・クエリは今まで通り動作します。

```bash
lineprior build observations.jsonl --out prior.jsonl --context-order 2
lineprior query prior.jsonl --state state_a --recent-actions action_x,action_y
lineprior eval observations.jsonl --context-order 2
```

**バックオフと透明性。** コンテキストを考慮したクエリは、まず最も長い利用可能なコンテキストを試
し、そこから「stupid backoff」(補間平滑化は行いません)でより短いコンテキストへと後退し、最終的
には order-0 の通常のルックアップに落ち着きます。`lineprior query --recent-actions` は
`{"matched_order": N, "candidates": [...]}` を出力します。`N` は実際にどの深さが応答を返したか
(`0` は state のみを意味します)を示し、`confidence` がアクションごとに提供しているのと同じ「どれ
だけの根拠に裏付けられているか」という透明性を、クエリのレベルでも提供します。`--recent-actions`
を指定しない場合、`query` の出力は従来とバイト単位で変わりません。

**ソート順の前提条件。** ストリーミング中にシーケンス自身の直近アクションのウィンドウを導出するに
は、そのシーケンスの行が入力中で連続しており、かつ `step` が厳密に増加している必要があります —
`--context-order` が 0 以外のときのみ強制されます。違反は **`--strict` とは無関係な** ハードエ
ラー(`SequenceNotSorted`、終了コード 3)です。これはレコード単位の妥当性を扱う
`--strict`/非strict とは異なり、ストリーム全体にわたる構造的な前提条件だからです。データがまだこ
の順序でグループ化されていない場合は、先にソートしてください(`jq -s 'sort_by(.sequence_id,
.step)[]'` など)。

**出力スキーマ。** コンテキストのエントリには、通常の `{"state": ..., "actions": [...]}` 行に加え
て `context` フィールド(直近アクションのウィンドウ、古い順)が追加されます:
`{"state":"state_a","context":["action_x"],"actions":[...]}`。order-0 のエントリはこのフィールド
を一切持たないため、`--context-order 0`(デフォルト)で構築した book は、この機能が存在する前と全
く同じようにシリアライズされます。

**メモリ。** ピークメモリは「一意な `(state, action)` ペアに比例して有界」から「order-0 での一意な
`(state, action)` ペア **に加えて**、order `1..=k` それぞれにわたる一意な `(context, state,
action)` タプルに比例して有界」に変わります — これは機能に内在するコスト(精度を上げるにはより多
くのストレージが必要)であり、リグレッションではありません。
`crates/lineprior/tests/streaming_memory.rs` にはこの形のリグレッションテストもあります。

**コンテキストが実際に役立っているかを評価する。** `lineprior eval --context-order k` は、通常の
order-0 のフィールドに加えて、同じ実行の中で同じテスト観測に対して計算された2つの新しいトップレベ
ルフィールドを報告します: `context_top1_hit_rate` / `context_mean_reciprocal_rank`(それぞれ
`top1_hit_rate`/`mean_reciprocal_rank` のコンテキストを考慮した版で、こちらは order-0 のままで
す)。その差がコンテキストによるリフト(またはコスト)です — ヘッドラインのフィールドが実行ごとに
密かに異なる意味を持つ2回の別実行を比較するのではなく、単一実行内でのapples-to-apples比較です。
`hit_rate_by_matched_order` は、バックオフが実際に到達した深さ(到達した頻度ではなく)ごとの精度と
`calibration_brier` を
分解して示し、「より深いコンテキストは利用可能なときに実際に精度が上がるのか、それとも単に稀なだ
けなのか」に答えます。`context_calibration_brier` は、multi-stepのコンテキスト経路における #1 confidence と
hit/miss の Brier score を報告します。`--context-order 0` ではこれらの指標が空/`None` です。`lineprior tune` も
`all_results` の候補ごとに同じ2つのフィールドを表示するため、`--param
context-order=0,1,2,3` のスイープでリフトを直接確認できます — 新しい `--objective` は不要です。既
存の objective がすでに、そのスイープで変動する order-0 のフィールドを読んでいるためです。

**信頼度の帰属に関する注意点(上記の outcome 重み付き eval 指標と同じ形):** コンテキストは純粋に
**step の順序**から導出されており、あなたのドメインにおいて深いコンテキストが因果的に意味を持つか
どうかについては何も判断していません。判断できるのは、保留データ上で統計的に予測力があるかどうか
だけです。コンテキストを考慮した prior を信頼する前には、必ず `context_top1_hit_rate` を通常の
`top1_hit_rate` のベースラインと比較してください。`state` がすでに直近の履歴をエンコードしている
ドメイン(盤面全体など)では、リフトがほとんど、あるいは全く見られないこともあります — それはバグ
ではなく、正当で有益な結果です。

## オプトインの類似状態フォールバック

`PriorBook::query_with_similarity` は、呼び出し側が state、0 以上の距離、透過的な provenance を付けて
渡した近傍状態を受け取ります。距離に基づく決定的な指数重み付けを行い、近傍状態で実際に観測された
action だけを返すため、未知の action を生成しません。`SimilarityConfig` で近傍数や距離を制限でき、各
結果には action を裏付けた状態の evidence も残ります。

これは統合境界であり、embedding やベクトルデータベースの実装ではありません。デフォルトは従来どおり
完全一致検索と棄却です。返される confidence は元の confidence の重み付き要約であり、新しい統計的保証
ではないため、意思決定ループで有効化する前に、未知状態の分割で類似フォールバックを検証してください。

`crates/lineprior-similarity/tests/fixtures/unseen_states.jsonl` と統合テストには、完全一致、priorなしの棄却、
オプトイン類似検索による回収を比較する決定的な境界fixtureがあります。これは契約確認であり、実データの品質や
類似検索をデフォルトで有効化する根拠ではありません。

実データでの比較手順（exact-match / similarity / no-prior、coverage、MRR、top-1、calibration、
abstention、速度、メモリ）は [`docs/measurements/similarity-real-data.md`](docs/measurements/similarity-real-data.md)
に記載しています。
依存なしの `scripts/measure_similarity.py` で、呼び出し側が用意した近傍を含むquery JSONLから
3つのarmを比較できます。`examples/similarity_queries.jsonl` は契約fixtureであり、実データ結果ではありません。
CIは `scripts/run_measurement_smoke.sh` で両fixtureを2回再生し、決定的なarm指標と境界条件を確認します。
速度/RSSは環境依存の測定値として扱い、downstream改善の証拠とはしません。

## シーケンス単位の prior

`PriorBook::score_sequence(path: &[(String, String)]) -> SequencePriorScore` は、**呼び出し側が
指定した**複数ステップの候補プランを、各ステップで[コンテキストを考慮したバックオフ](#可変長コン
テキスト)をたどりながらスコアリングします — 各ステップにどれだけの過去データの裏付けがあるか、そ
してプラン全体としてどうか、を示します:

```rust
let path = vec![
    ("state_a".to_string(), "action_x".to_string()),
    ("state_b".to_string(), "action_y".to_string()),
];
let score = book.score_sequence(&path);
// score.steps[i]: { state, action, matched_order, found, prior, confidence }
// score.min_confidence: 最も裏付けの弱いステップの confidence(一つも一致しなければ None)
// score.unseen_steps: 過去データに全く裏付けのなかったステップの数
```

各ステップのコンテキストは**そのプラン自身の**それ以前のステップのアクション(古い順)であり
— `--context-order` が構築時にコンテキストを導出するのと同じ考え方です — 呼び出し側が別途渡すもの
ではありません。`lineprior` は環境の遷移モデルを持たないため、`(state, action)` を与えられても
どの state に至るかはわかりません。したがって呼び出し側(その対応関係を知っている、自分自身のプラ
ンナーやシミュレーター)が、各ステップの state と action の両方を指定する必要があります。

**集約は平均ではなく `min` です。** チェーンの強さは最も弱いリンクで決まります。平均を取ると、非
常に裏付けの弱い1ステップが他の強いステップの陰に隠れてしまい、「prior であってoracleではない」
という透明性の原則に反します。すべてのステップが unseen の場合、`min_confidence` は `0.0` ではな
く `None` になります — 他の箇所でも使われている「データが無いことを悪いスコアとして扱わない」とい
う規則と同じです。`unseen_steps > 0` のときは、集約値だけでなく `steps` を直接確認してください。

**バックオフのシャドーイングに関する注意点。** 各ステップは `query_with_context` をそのまま再利用
します: 実際に解決したコンテキストの深さだけが、呼び出し側の指定したアクションの探索対象になりま
す。他のアクションだけを含む疎な深いコンテキストの一致が、order-0 で豊富な裏付けを持つそのアクシ
ョンを覆い隠してしまうことがあり、より浅い深さでは十分裏付けられているアクションでも `found:
false` になり得ます。これは安全側(裏付けを過小報告することはあっても過大報告はしない)の挙動であ
り、`query_with_context` 自身が「ここで何をすべきか」と尋ねられた場合に返す答えと一致しています —
バグではありませんが、`found: false` を「本当に一度も見られていない」と単純に解釈する前に知ってお
く価値があります。

**意図的にライブラリのみの機能です。** 今回のラウンドでは CLI サブコマンドも `eval`/`tune` との統
合もありません — `(state, action)` のパスはカンマ区切りの CLI フラグに収まる形ではありません
し、保留データのシーケンスをその結果と突き合わせてスコアリングするには、コアモデルが意図的に立場
を持たない「シーケンスの最終結果」という概念を新たに定義する必要があります(上記の[信頼度の帰属に
関する注意点](#prior-の性能を評価する)を参照)。どちらも、実際の需要が現れれば自然な拡張先になり
ます。

## Tuning: BuildConfig を自動的に選ぶ

`eval` は一度に1つの config を評価しますが、`tune` は複数の config をグリッドサーチし、すべての
候補に*同じ*決定的な train/test 分割を使うことで直接比較可能な形で最良の config を選びます:

```bash
lineprior tune observations.jsonl \
  --split-by sequence --train-ratio 0.8 \
  --param confidence-mode=heuristic,wilson-lower-bound,hybrid \
  --param min-confidence=0.0,0.3,0.5,0.7 \
  --param smoothing-alpha=1.0,5.0,10.0 \
  --param time-decay-half-life-days=none,30,90 \
  --time-decay-reference-unix-seconds 1783540000 \
  --objective covered-mrr --min-covered-fraction 0.4 \
  --out tune.json --save-best-config best_config.json
```

`--param key=v1,v2,...` は1つの `BuildConfig` フィールドを掃引します(複数フィールドを掃引する場合
は `--param` を繰り返してください)。`--param` で指定されなかったフィールドは、すべての候補で
`BuildConfig::default()` のままです。対応するキー: `confidence-mode`、`min-confidence`、
`smoothing-alpha`、`confidence-k`、`confidence-z`、`min-count`、`min-weighted-count`、
`draw-value`、`time-decay-half-life-days`(`none` を受け付けます)、`default-source-weight`、
`count-weight`、`success-weight`、`score-weight`。
`--time-decay-reference-unix-seconds` はすべての候補に適用される単一の値です(掃引対象にはできませ
ん)— 掃引した `time-decay-half-life-days` の値のいずれかが `none` でない場合は必須です。これは
`build`/`eval` と同じ再現性のルールです。

`--objective`(デフォルト `covered-mrr`)が候補のランク付けに使われます:

| objective | 意味 |
|---|---|
| `mrr` | `mean_reciprocal_rank`。カバーされたテスト観測のみが対象 |
| `top1` | `top1_hit_rate`。カバーされたテスト観測のみが対象 |
| `covered-mrr`(デフォルト) | `covered_fraction * mean_reciprocal_rank` — 全テスト観測にわたって平均した MRR で、カバーされなかった観測は `0` として寄与する |
| `top1-at-min-coverage` | `top1` と同じだが、`--min-covered-fraction` も指定されている必要がある |
| `success-weighted-mrr` | `success_weighted_mean_reciprocal_rank` — `mrr` と同様だが、失敗または未記録の outcome を持つテスト観測は寄与しない |
| `success-weighted-top1` | `success_weighted_top1_hit_rate`。同じ考え方を `top1` に適用したもの |

デフォルトが `covered-mrr` である理由: `mrr` だけを最大化すると、確信度が高いときしか予測しない
(coverage を極端に犠牲にする)設定を選びがちです。逆に coverage だけを見ると、雑な prior を許容
してしまいます。`covered-mrr` は両方にペナルティを課します。

`--min-covered-fraction` / `--max-fallback-rate` / `--min-top1-hit-rate` は、候補が `best` として
選ばれることを妨げますが、JSON レポートの `all_results` には(`meets_constraints: false` として)
残ります — 何が、なぜ除外されたのかが黙って消えるのではなく確認できます。

JSON レポートの `pareto_front` は `(mrr, covered_fraction)` に関する非劣解集合です — `--objective`
とは無関係に、何らかの MRR/coverage のトレードオフにおいて最良となる候補が並びます。単一の `best`
を信用する代わりに、自分でトレードオフを見て選びたい場合に使えます。

`--save-best-config best_config.json` は勝った候補の `BuildConfig` を JSON として保存します。
`build` と `eval` はどちらも `--config best_config.json` でそれを読み込めます(個々の
build-config フラグ、例えば `--min-count` と組み合わせるとエラーになります — 上書きではなく
config 全体の置き換えのため)。これにより、`tune` で一度選んだ config を手で再入力せずそのまま
再利用できます:

```bash
lineprior build observations.jsonl --out prior.jsonl --config best_config.json
```

`tune` は `lineprior` の他の部分とまったく同様にドメインに依存しません(`state`/`action`/
`sequence_id`/outcome のデータしか見ません)。また、`lineprior` の本質を変えるものでもありません
— **oracle ではなく prior** です。`tune` は、人手で `eval` を掃引していた作業を自動化するだけであ
り、結果として得られた prior を呼び出し側が行動前に検証すべきという点を何も変えません。

## ゲート結果の予測(ライブラリ限定)

このクレートの他の部分とは違う種類の問いです。「どの行動を取るべきか」ではなく、「この学習候補は、
高コストな実評価(多数対局の 'gate' 実行)にかける価値があるか」です。`GateModel::fit`/
`GateModel::predict`(`gate.rs`)は、安価な validation 時点の診断値から、候補の実 gate Elo delta と
—そしてその予測をどれだけ信じられるか—を予測する、小さく正則化されたサロゲートモデルを学習します。
これにより、実際の gate 実行は、その価値が見込める候補にだけ回せます。

```rust
let output = GateModel::fit(&observations, &GateModelConfig::default())?;
// output.report: selected_lambda、weighted_rmse、probability_positive のキャリブレーションレポート
// -- output.model の予測を信じる前に確認する。

let prediction = output.model.predict(&GateQuery { features });
// prediction.expected_elo, .interval_low/.interval_high, .probability_positive,
// .leverage, .support_distance, .nearest_group_distance, .missing_feature_fraction,
// .prediction_status, .recommend_for_gate
```

`predict_verdict` は、Gaussian な潜在 Elo posterior を明示した `GateVerdictConfig` の閾値で PASS、FAIL、
INCONCLUSIVE の3領域に分けます。`acquire` は incumbent の `baseline_elo` に対する標準的な expected improvement
を、呼び出し側が渡す expected gate cost で割ります。EI はすでに baseline 超過確率を含むため、`probability_positive`
を二重には掛けません。どちらも OOD の recommendation flag を保持します。

```bash
lineprior gate gate_history.jsonl --feature valid_cp_mse_delta=0.12 \
  --feature output_std=0.03 --monotonic valid_cp_mse_delta=increasing \
  --expected-gate-cost 100 --out gate-report.json
```

`gate` CLI は strict JSONL の `GateObservation` から実験的モデルを fit し、指定時には予測、3種類の verdict 確率、
acquisition score、fit 診断をJSONで出力します。GateModel は引き続きメインcrateに置きます。CLI独自のschemaや依存境界
はまだないため、利用者が現れる前に `lineprior-gate` へ分割するとパッケージ化の表面だけが増えるためです。

- **単調制約はオプトインです。** `GateModelConfig::monotonic_constraints` は指定した係数を increasing/decreasing の
  符号直交錐へ射影します。制約付き fit の閉形式ridge不確実性は近似に過ぎないため、高コスト実行のスケジュールに
  使う前に実ゲート履歴で検証してください。

- **固定スキーマではなく、名前付き特徴量。** `GateObservation.features`/`GateQuery.features` は
  呼び出し側が名前を付ける `BTreeMap<String, f64>`(例: `valid_cp_mse_delta`、`output_std`、
  `conflict_rate`)です。診断項目のセットは、スキーマを壊さずに進化できます。training seed のような
  ものは意図的に除外しています — カテゴリ的な id であって、線形モデルにとって「多い/少ない」が意味
  を持つ量ではないためです。
- **ランダム分割ではなく、group を意識した分割。** `GateObservation.group_id` は呼び出し側が合成する
  不透明なキーです(例えば experiment family/recipe/lineage/dataset version を結合したもの)。リッジ
  の正則化強度を選ぶ k-fold cross-validation に使われ、このクレート自身は一切パースしません。
  要求した fold 数より distinct な group が少ない場合は leave-one-group-out にフォールバックします。
- **不確実性は「候補の潜在的な強さ」への確信度であり、次回1回の gate 結果のばらつきではありません。**
  `interval_low`/`interval_high` は(下記の `GatePrediction` と `GateOofPrediction` の両方で)、
  期待値をその候補の*真の*強さの見積もりとしてどれだけ信じられるか(閉形式の Bayesian-ridge 事後分散)
  を表すものであり、次に1回 gate を実行した場合に加わる標本ノイズではありません。これはモジュール
  全体に一貫して当てはまる規約であり、呼び出しごとのオプトインではありません。クエリ時に欠けている
  特徴量は学習時平均で補完され、`missing_features` に必ず記録されます — 黙って作り出すことはありま
  せん。
- **Round A 自体を実データで検証するための機能。** `GateModel::fit_with_validation` は `fit` が返す
  ものすべてに加えて、候補ごとの out-of-fold 監査テーブルを返します:

  ```rust
  let validated = GateModel::fit_with_validation(&observations, &GateModelConfig::default())?;
  // validated.interval_level: interval_low/interval_high が表す両側信頼水準(デフォルトの interval_z
  // ではおよそ0.95)。各行で繰り返さず、ここで一度だけ示されます。
  for row in &validated.oof_predictions {
      // row.candidate_id, .group_id, .actual_elo, .predicted_elo, .residual, .prediction_stddev,
      // .interval_low/.interval_high, .probability_positive, .outer_fold, .inner_selected_lambda,
      // .leverage, .support_distance, .nearest_group_distance, .missing_feature_fraction,
      // .prediction_status, .recommend_for_gate
  }
  ```

  すべての行は、`report.weighted_rmse`/`report.calibration` の元になっているのと*同じ* nested
  group cross-validation から得られます — 監査テーブルのためだけに2回目の CV を回すことはしません。
  そのため、集計指標と候補別の監査結果が異なる予測母集団を指すことはあり得ません。行は
  `(outer_fold, group_id, candidate_id)` で決定的にソートされ、入力に重複する `candidate_id` があっ
  てもそれぞれ別の行として保持されます(まとめません)。`fit` 自体は `fit_with_validation` の薄い
  ラッパーで、監査テーブルを破棄します(いずれにせよ計算は行われます — 破棄によって節約されるのは
  受け取り・読み取りのコストだけです)。両者は1つの学習経路を共有するため、モデルや集計指標について
  食い違うことはあり得ません。
- **Elo 観測の不確実性: すべてのラベルが同じ確度ではありません。** `GateObservation` は
  `gate_elo_delta` に加えて `actual_elo_stddev`(または `elo_ci_low`/`elo_ci_high` — 両方あれば
  `GateModelConfig::observation_ci_z` を使った対称正規区間として stddev を逆算します。`interval_z`
  とは別の独立したノブです — 呼び出し側の CI がどの信頼水準で計算されたかは、このモデル自身の出力
  区間をどれだけ広くすべきかとは無関係だからです)を持てます。20 ペアの burn-in Elo と
  1700 ペアの formal gate Elo は、同じ確度の教師ラベルとして扱うべきではありません。指定された場合、
  これが行ごとに `gate_games_played` ベースの重みに代わってリッジ回帰の信頼性重み(`1 / stddev^2`、
  逆分散)になります — 一部の行だけ stddev があるような混在データセットも正しく合成されます。
  `completed_pairs` と `gate_status`(その候補の実際の過去の formal gate 判定 PASS/FAIL/INCONCLUSIVE)
  も受け付けますが、現状は監査専用です — `features` にも fit にも投入されません(既存の
  `training_seed` を除外している理由と同じです)。`provenance: BTreeMap<String, String>` フィールドは
  呼び出し側が合成する不透明な来歴情報(experiment/dataset/teacher-manifest の id、seed、
  schema version など)を保持します — `group_id` と同じ「このクレートは一切パースしない」という規約
  です。
- **1 観測につき不確実性のソースはちょうど1つ — 暗黙の優先順位はありません。** 同じ観測に
  `actual_elo_stddev` と完全な `elo_ci_low`/`elo_ci_high` の両方を指定するとエラーになります
  (`Error::ConflictingGateUncertaintySources`)。CI の片方だけを指定した場合も同様です
  (`Error::IncompleteGateConfidenceInterval`)。また `gate_elo_delta` が自身の
  `[elo_ci_low, elo_ci_high]` の外にある場合もエラーになります
  (`Error::GateEloOutsideConfidenceInterval`)。どちらも指定されていない観測は、これまで通り
  `gate_games_played` ベースの重みにフォールバックします。
- **極端な行重みが fit を支配しないようにします。** 平均 `1.0` に正規化した後、各行の信頼性重みは
  `[1 / max_weight_ratio, max_weight_ratio]`(`GateModelConfig::max_weight_ratio`、デフォルト
  `100.0`、`1.0` 以上である必要があります)にクランプされます — そうしなければ、極端に小さい
  `actual_elo_stddev` を申告した1行(入力ミス、あるいは本当にノイズがほぼゼロの計測)が、他のどの
  行より何千倍も大きい逆分散重みを持ち、その1行だけで fit を事実上支配しかねません。このクランプの
  後にもう一度平均 `1.0` へ再正規化することは意図的に行いません — クランプ済みの重みを再び平均 `1.0`
  へ正規化し直すと、約束したはずの範囲の外へ押し戻されてしまう可能性があるためです。トレードオフと
  して、クランプが実際に働いた場合に限り重みの平均が `1.0` からわずかにずれることを許容します。
  `GateFitReport` はこれを隠さないよう `min_observation_weight`/`max_observation_weight`/
  `effective_sample_size`/`clamped_observation_count` を公開します。
- **`GateFitReport.dispersion_factor`: 申告された stddev 自体のキャリブレーションチェック。**
  fit に含まれる全観測が使用可能な stddev を持つ場合にのみ `Some` になります — out-of-fold な
  reduced chi-square(`sum((actual_elo - predicted_elo)^2 / stddev^2) / n`、`weighted_rmse`/
  `calibration` と同じ nested-CV 予測から計算)です。おおよそ `1.0` であれば、申告された stddev が
  実際の予測と結果のズレに対してよくキャリブレーションされていることを意味します。`>> 1` は実際の
  ノイズが申告値を上回っている、または線形モデルが構造を捉えきれていない(この統計量だけでは両者を
  区別できません)ことを、`<< 1` は申告された stddev が過大であることを示します。
- **OOD(分布外)時の棄権: 表示するだけで強制はしません。** `GateOofPrediction` の各 OOD 指標は、
  その outer fold の*学習行だけ*から fit したサポートモデル(standardizer、group の重心、平均
  leverage)から計算されます — その fold の係数を fit したのと同じ行であり、held-out 行自身や他の
  fold の行は一切含みません。最終的にデプロイされる `GateModel`(全 CV 完了後)は、係数と同様、
  サポートモデルも学習データ全体で fit します。すべての `GatePrediction`/
  `GateOofPrediction` は、`leverage`(リッジの hat/leverage 項に相当し、クエリが学習時の特徴量平均から
  離れるほど際限なく大きくなります)、`support_distance`(`leverage.sqrt()`)、
  `nearest_group_distance`(標準化空間での、最も近い学習 group の重心までの距離)、
  `missing_feature_fraction`、`prediction_status`(`Supported`/`Extrapolation`/`Unsupported`)も
  持ちます。`expected_elo`/`probability_positive` は status に関わらず計算・返却されます —
  `missing_features` と同じ「拒否せず表示する」という規約です — `Extrapolation`/`Unsupported` に
  対してどう振る舞うかは呼び出し側が自分で決めます。分類はまず `missing_feature_fraction` を無条件で
  チェックします(`GateModelConfig::ood_missing_fraction_threshold`、デフォルト `0.5`): 全特徴量が
  欠けているクエリは学習時の特徴量平均に補完され、これは最も低い leverage になります — これを放置する
  と、実際には情報がゼロなのに最大限サポートされているように見えてしまいます。それ以外の場合、
  `leverage` がモデル自身の平均 leverage(`df / n_eff`。OLS のハット行列が一様に持つ `p/n` のリッジ版
  であり、`lambda > 0` では成り立たない古典的な `2p/n`/`3p/n` の経験則ではありません)の
  `ood_leverage_ratio_threshold` 倍(デフォルト `3.0`)を超える場合、または `nearest_group_distance`
  が学習 group の重心同士の最大最近傍距離(恣意的な定数のない、自己校正的な基準スケール)を超える場合に
  `Extrapolation` になります。
- **`recommend_for_gate`: 見積もりを偽らない、yes/no の gating 判断。** 中身は
  `prediction_status == Supported` そのもので、それ以外の意味は一切持ちません — bool だけ欲しい
  呼び出し側は `prediction_status` を自分でマッチさせる代わりにこちらを読めます。
  `expected_elo`/`interval_low`/`interval_high`/`probability_positive` は `recommend_for_gate` が
  `false` のときも常にモデルの本当の予測のままです — 分布外のクエリはフラグが立つだけで、黙って
  ゼロに置き換えられたりはしません。
- **実験的な診断用の機能です。** verdict確率、acquisition、単調制約は実装済みですが、高コスト実行のスケジュールに
  使う前に実ゲート履歴でのキャリブレーションとdownstream検証が必要です。CLIは薄いままにし、独立したschema/依存境界
  が現れるまでモデルはメインcrateに保持します。

## 学術的な位置づけ

`lineprior` は、case-based planning(事例ベース計画)、plan reuse(計画の再利用)、sequence prediction(系列予測)、variable-order Markov models(可変次数マルコフモデル)、policy-guided search(方策誘導探索)といった既存のアイデアに着想を得た、工学的な Rust 実装です。新しい理論的アルゴリズムではありません。

## 開発

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo deny check licenses
sh scripts/check_candidate_contract.sh
sh scripts/run_examples_smoke.sh
sh scripts/run_wasm_build_smoke.sh
```

依存クレートのライセンスは [`deny.toml`](./deny.toml) の SPDX 許可リストで検査します。新しいライセンスを
追加する場合は、明示的なポリシーレビューが必要です。

候補版の契約チェックは、workspace の固定バージョン、JSON fixture、言語例の構文、整形、空白差分を検査します。
ランタイムテスト、WASM パッケージ化、実データ測定ゲートの代替ではありません。

`lineprior-cli` をビルドした後に `sh scripts/run_examples_smoke.sh` を実行すると、同じバイナリを使って保守対象の
Node.js と Python のラウンドトリップ例を確認できます。CI でもこの smoke workflow を実行しますが、Rust CLI の統合
境界の確認であり、WASM パッケージ化や実データ品質の証拠ではありません。

同じバイナリで `sh scripts/run_offpolicy_smoke.sh` を実行すると、OPE fixture を同じ設定で2回評価し、IPS、DR、
bootstrap seed を含むJSONレポート全体の一致を確認できます。これは再現性の確認であり、因果的改善の証拠ではありません。

`wasm32-unknown-unknown` target をインストール済みなら、`sh scripts/run_wasm_build_smoke.sh` で locked dependency
graph による `lineprior-wasm` crate のコンパイル境界を確認できます。これはコンパイルだけの確認であり、npm/wasm-pack
パッケージ化とブラウザ実行は別のゲートです。

リリース履歴は [`CHANGELOG.md`](./CHANGELOG.md)(英語)を参照してください。crates.io への公開状況、
および 0.9.0 以降は公開 Rust API の変更点について、JSON/serde の入力互換性とは別に Rust のソース互換性
を明記しています。

設計仕様とロードマップの全体は [`AGENTS.md`](./AGENTS.md) を参照してください。
## 差し替え可能な scoring strategy

`BuildConfig::scoring_strategy` は `WeightedSum`（後方互換の既定値）、`Bayesian`、`Ucb`、
`Softmax` を選べます。CLI では `--scoring-strategy` と方式別パラメータを使います。
これは順位付け方式であり、品質保証ではありません。

## compact binary book

交換形式は JSONL のままとし、ローカルキャッシュ向けに決定論的な LPB v1 を追加しました。
コアの `save_prior_book_binary` / `load_prior_book_binary` と CLI の `pack` / `unpack` が使えます。
context entry、magic/version、割り当て上限を保持し、余分な末尾 bytes は拒否します。

## veridict prior on/off レシピ

ペア seed・split・予算・停止規則を固定する手順を
[`examples/veridict_prior_comparison.md`](examples/veridict_prior_comparison.md) に置きました。
実際の `veridict` 実行結果がないため、現時点では recipe-only です。
IPS / DR のpropensity・overlap事前確認、bootstrap不確実性、lineprior on/offのdownstream比較手順は
[`docs/measurements/offpolicy-real-data.md`](docs/measurements/offpolicy-real-data.md)に分離しています。
paired reward差分とpropensity事前確認には依存なしの
`scripts/compare_offpolicy_arms.py`を使い、IPS / DRの推定値は各armに対してRust CLIを個別に実行します。
`scripts/measure_offpolicy_arms.py`を使うと、両armのCLIレポートとpaired監査を1つのlineage付きartifactにまとめられます。
保存前に `scripts/validate_measurement_artifact.py` でprotocol・lineage・必須指標・固定バージョンの
artifact契約を確認できます。これは形式検証であり、downstream改善の証拠やgate通過ではありません。
similarity reportには元priorの `build_config_fingerprint` も引き継がれるため、異なるBuildConfigの結果を
黙って比較することを防げます。
reportには入力JSONLのSHA-256も含まれるため、dataset IDが同じまま入力が差し替えられた場合も検出できます。
再現可能なローカル引き渡しには `scripts/run_ecosystem_matrix_smoke.sh` を使えます。Rust・Python・Nodeの
実行時バージョンを記録してCLI・OPE・measurement smokeを再生しますが、全対応バージョンの証明や実データ品質の
証拠ではありません。
`--out runtime-report.json` を渡すと、実行時バージョン、commit、固定プロジェクトバージョン、実行したcheckをJSON
artifactとして保存できます。CIでもexamples smokeのartifactとしてアップロードします。
CIはupload前に固定版、commit形式、runtime inventory、実行check一覧のartifact契約も検証します。
WASM build smokeも `wasm32-unknown-unknown` target、Rust toolchain、commit、固定版を別artifactとして記録・検証します。
これはcompileの証拠であり、npm公開やブラウザ品質の証拠ではありません。
## macro-actions と multi-source merge

`build_macro_actions` は順序付き履歴から連続した action window を抽出します。window を保持する必要があるため
eager API とし、通常の streaming builder の挙動は変更していません。独立に作った book は
`PriorBookSource` と明示的な weight を渡して `merge_prior_books` で統合できます（context entry 対応）。

正式な型境界は `lineprior-adapters` にあり、Sekirei、UI automation、LLM agent、retrosynthesis の
record を `Observation` に変換します。合法性・実行成否・化学的妥当性の検証は各アプリ側に残します。
## terminal credit と Trie 表現

`BuildConfig::terminal_credit_weight`（CLIでは `--terminal-credit-weight`）を指定すると、各系列の最後の既知の
outcome を、その系列の保持された各stepへ伝播できます。既定値 `0.0` では従来のstep単位ラベルを維持します。
有効時は現在の系列だけをbufferし、系列単位でまとまった入力を想定します。

`PriorBook::to_trie()` は context entry を決定論的な `PriorTrie` に展開し、最長suffixを優先してqueryします。
flat bookを正本の保存形式として残し、trieの性能比較は引き続きmeasurement項目です。

## 制限と evidence gate

`confidence` は信頼性の目安であり、将来性能の保証ではありません。Bayesian、UCB、Softmax は順位付けの方式を
変えるだけで、品質、calibration、downstream 結果の改善を保証しません。既定値を変更する前に held-out 比較を
実行してください。

core の state/action は opaque key です。安全な既定値は完全一致であり、類似状態による回復は呼び出し側が渡す
場合だけ有効です。未観測 action は生成しません。因果推論、counterfactual action generation、未記録 action の
reward 推定も行いません。IPS/DR は propensity、overlap、不確実性、held-out downstream 検証が揃ったときだけ
解釈できる監査用推定量です。

Python、npm、WASM は現在、保守された CLI 例と Rust/WASM 境界までです。正式配布・ブラウザ実行の扱いは
`.github/workflows/wasm-browser.yml` の gate を通過するまで supported distribution と表現しません。Trie と
macro-action には決定論的 Criterion benchmark を追加していますが、性能と downstream 改善はまだ実測課題であり、
benchmark だけから改善を主張しません。

測定は `cargo bench -p lineprior --bench scoring` で実行できます。実装間で比較する場合は、マシン、toolchain、
sample size、Criterion 出力を記録してください。異なる環境の数値を downstream 実験として直接比較しません。

新規 workspace crate の初回 crates.io 公開には Trusted Publishing の制約で一度だけ手動 bootstrap が必要です。
手順は [`docs/publishing.md`](docs/publishing.md) にまとめています。初回公開後は既存の OIDC workflow で継続公開できます。
