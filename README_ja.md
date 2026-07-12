# lineprior

[![crates.io](https://img.shields.io/crates/v/lineprior.svg)](https://crates.io/crates/lineprior)
[![docs.rs](https://img.shields.io/docsrs/lineprior)](https://docs.rs/lineprior)
[![CI](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml/badge.svg)](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/lineprior.svg)](https://github.com/kent-tokyo/lineprior/blob/main/LICENSE-MIT)

日本語 / [English](./README.md)

`lineprior` は、過去の行動系列からドメインに依存しない **行動事前分布(action prior)** を構築する Rust ライブラリおよび CLI です。ある状態が与えられたとき、次の問いに答えます。

> この状態から、過去にどの行動がうまくいったか?

将棋の定跡帳ライブラリでも、チェス専用の定跡フォーマットでも、プランナーでも、ソルバーでも、ゲームエンジンでもありません。過去の `(state, action, outcome)` のログを、状態ごとにランク付けされた候補行動のリストへ変換する、小さく再利用可能なコンポーネントです。ゲーム、探索、自動化、エージェント、最適化など、過去の成功した行動系列が今後の意思決定の指針になり得るあらゆるドメインで有用です。

## これではないもの

`lineprior` は単独で最善の行動を決定しません。これは **oracle ではなく prior** です:

- count・rate・confidence の付いた候補行動を提案します。
- 呼び出し側が、探索・評価・ルール・検証などと組み合わせて使うことを前提としています。
- データが少ない場合や未知の状態に対しては、行動を創作するのではなく、候補を返しません。

過去データに偏りがあれば、prior にも偏りが反映されます。`lineprior` は、過去の系列が関連性・代表性を持つ場合に候補の順序付けを改善しますが、より良い意思決定を保証するものではありません。

## prior book を構築する

```bash
lineprior build observations.jsonl \
  --out prior.jsonl \
  --min-count 1 \
  --smoothing-alpha 5.0
```

主なフラグ: `--max-step`(指定した step を超える観測を除外)、`--max-actions-per-state`(状態ごとに上位 N 件のみ保持)、`--tags`(指定したタグのいずれかを持つ観測のみを対象、カンマ区切り)、`--confidence-k`(サンプル数に対する confidence の伸び方を調整)、`--confidence-mode`(`heuristic`(デフォルト)、`wilson-lower-bound`、`hybrid` — 詳細は下記「Confidence モード」)、`--confidence-z`(Wilson lower bound の z 値、デフォルト `1.96`。`heuristic` では無視される)、`--min-weighted-count` / `--min-confidence`(生の `--min-count` の代わりに、weighted count や confidence 自体でフィルタリング)、`--draw-value`(`draw` outcome に与える成功クレジット — デフォルト `0.5`。draw は敗北ではなく、対戦ゲームにおける正当な部分的結果であるため)、`--time-decay-half-life-days` / `--time-decay-reference-unix-seconds` / `--missing-timestamp-policy`(経過時間に基づく重みの減衰 — 詳細は下記「Time decay と source reliability」)、`--source-weights` / `--default-source-weight`(source ごとの信頼度倍率、同セクション)、`--config <path.json>`(個々のフラグの代わりに `BuildConfig` 全体をファイルから読み込む。例えば `lineprior tune --save-best-config` が保存したファイル — 詳細は下記「Tuning」。上記のいずれかのフラグと組み合わせるとエラーになる)、`--strict`(不正なレコードを警告付きでスキップせず、最初の1件で失敗させる)。

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
`hit_rate_by_matched_order` は、バックオフが実際に到達した深さ(到達した頻度ではなく)ごとの精度を
分解して示し、「より深いコンテキストは利用可能なときに実際に精度が上がるのか、それとも単に稀なだ
けなのか」に答えます。`--context-order 0` ではこれら3つすべてが空/`None` です。`lineprior tune` も
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
`draw-value`、`time-decay-half-life-days`(`none` を受け付けます)、`default-source-weight`。
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

## 学術的な位置づけ

`lineprior` は、case-based planning(事例ベース計画)、plan reuse(計画の再利用)、sequence prediction(系列予測)、variable-order Markov models(可変次数マルコフモデル)、policy-guided search(方策誘導探索)といった既存のアイデアに着想を得た、工学的な Rust 実装です。新しい理論的アルゴリズムではありません。

## 開発

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

設計仕様とロードマップの全体は [`AGENTS.md`](./AGENTS.md) を参照してください。
