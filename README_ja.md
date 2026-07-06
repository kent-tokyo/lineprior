# lineprior

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

主なフラグ: `--max-step`(指定した step を超える観測を除外)、`--max-actions-per-state`(状態ごとに上位 N 件のみ保持)、`--tags`(指定したタグのいずれかを持つ観測のみを対象、カンマ区切り)、`--confidence-k`(サンプル数に対する confidence の伸び方を調整)、`--min-weighted-count` / `--min-confidence`(生の `--min-count` の代わりに、weighted count やヒューリスティックな confidence 自体でフィルタリング)、`--draw-value`(`draw` outcome に与える成功クレジット — デフォルト `0.5`。draw は敗北ではなく、対戦ゲームにおける正当な部分的結果であるため)、`--strict`(不正なレコードを警告付きでスキップせず、最初の1件で失敗させる)。

`build` は、自身のフィルタが実際に何をしたかを1行で表示するようにもなりました。例: `stats: 950/1000 observations kept, 42/50 candidates kept (5 by min_count, ...)` — 自分側の事前フィルタ(ドメイン固有の ply/深さカットオフなど)が `--min-count` などと合わせて期待どおりに機能しているか、手計算せずに確認できます。ライブラリとしては、`build_prior_book_from_reader` が book と一緒に返す `BuildOutput.stats`(`BuildStats`)がこれに当たります。

## prior book を問い合わせる

```bash
lineprior query prior.jsonl --state state_a --top-k 5
```

未知の状態に対しては何も出力せず、それでも終了コードは `0` です — これはエラーではなく、想定されたフォールバック挙動です。

ライブラリとしては、`PriorBook::candidates()` を使うと、book 全体の `(state, action)` 候補をフラットな `Vec<(String, PriorAction)>` として取得できます。`entries_sorted()` が返すネストした状態ごとの構造を自分でたどる代わりに、候補を直接フィルタ・サンプリングしたい呼び出し側(ドメイン固有の「定跡集」を作る場合など)向けです。

## その他のコマンド

```bash
lineprior summary prior.jsonl      # カバレッジ、平均confidence、状態ごとのentropy
lineprior validate observations.jsonl   # 構築せずに入力をパースして問題を報告
```

## 入力スキーマ

1行に1つのJSONオブジェクト:

```json
{"sequence_id":"case-001","step":0,"state":"state_a","action":"action_x","outcome":"success","score":0.8,"weight":1.0,"tags":["trusted"]}
```

必須: `sequence_id`, `step`, `state`, `action`。
任意(デフォルト値あり): `outcome`(`unknown`)、`score`(`null`)、`weight`(`1.0`)、`tags`(`[]`)。

## 出力スキーマ

状態ごとに1つのJSONオブジェクト。actions は prior の降順でランク付けされます:

```json
{"state":"state_a","actions":[{"action":"action_x","count":3,"weighted_count":3.0,"success_rate":0.667,"mean_score":0.633,"prior":0.557,"confidence":0.130}]}
```

`success_rate` と `mean_score` は生の(平滑化されていない)観測レート(透明性のため)、`prior` は平滑化・正規化されたランキングスコア、`confidence` はヒューリスティックなサンプルサイズの指標であり、統計的な保証ではありません。`success_rate` は `success` を 1.0、`draw` を `--draw-value`(デフォルト 0.5)、`failure` を 0.0 としてクレジットします。

`lineprior build` の CLI 出力(およびライブラリの `save_prior_book_with_config`)は、構築に使った `BuildConfig` のフィンガープリントを持つヘッダー行を先頭に付加するようになりました(例: `{"build_config_fingerprint":3697336692924021039}`)。`load_prior_book` / `lineprior query` / `lineprior summary` はいずれもこの行を透過的にスキップします — 日常的な読み取り方法は変わりません。

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

## 制約事項

- confidence はヒューリスティック(`weighted_count / (weighted_count + k)`)であり、統計的な信頼区間ではありません。
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
- `coverage` と `fallback_rate`: これらは意図的に合計が1になりません。`coverage` は状態重み付け
  (prior が何らかの候補を返せた「異なるテスト状態」の割合)、`fallback_rate` は観測重み付け(候補
  が1つもなかった「テスト観測」の割合)です。滅多に出現しない候補なし状態は `fallback_rate` をほ
  とんど動かしませんが、`coverage` は丸ごと1点分下げます — レポートには各レートの元になった生の
  カウントも含まれているので、どちらの見方でも直接検算できます。

`lineprior eval --help` で `build` と同等のチューニングフラグ(`--min-count`、
`--smoothing-alpha` など)が一覧できます — `eval` は実際の `build` 実行と同じノブで train 側の
prior を構築するため、両者は比較可能なままです。

## 学術的な位置づけ

`lineprior` は、case-based planning(事例ベース計画)、plan reuse(計画の再利用)、sequence prediction(系列予測)、variable-order Markov models(可変次数マルコフモデル)、policy-guided search(方策誘導探索)といった既存のアイデアに着想を得た、工学的な Rust 実装です。新しい理論的アルゴリズムではありません。

## 開発

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

設計仕様とロードマップの全体は [`AGENTS.md`](./AGENTS.md) を参照してください。
