
resolve #<issue-number>

<!-- PRの内容を説明するために必要なら、HTMLを使って記述しても良い -->

## この PR で判断してほしいこと

<!--
レビュアーが判断すべき主要な問いを、1文で1つだけ記載する。
Issue 全体の完了より、独立してレビュー可能な最小の変更単位を優先する。
複数の独立した判断を含む場合は、PR を分割する。
-->

## リスクとレビュー重点

<!--
レビュアーに特に見てほしい点。
仕様判断、scope 境界、エラー処理、テスト不足、破壊的変更の可能性など。
-->

## 変更の概要

<!--
- 変更前後でどのようにアーキテクチャが変化したのかを記述する
- アーキテクチャをテキストだけでなく図でも説明する
  - 図の記述にはmermaidを用いること
-->

## Issue / 既存ドキュメントから直接導けない変更

<!--
Issue 本文・Issue コメント・既存 docs/ADR から直接は導けないが、
実装上必要になった判断や補完を記載する。
仕様追加・挙動変更・命名・保存形式・エラー処理・境界条件の補完など。
-->

## 設計判断 / ADR

<!--
後続 Issue・実装・レビュー・agent の判断に影響する重要判断がある場合は ADR 化を検討する。
単なる typo 修正、局所的 refactor、既存方針に従っただけの実装は ADR 不要。
-->

- [ ] ADR が必要な設計判断はない
- [ ] ADR が必要な設計判断があり、作成・更新した
- [ ] ADR が必要そうな判断はあるが、この PR では作成していない

### ADR 不要 / 未作成と判断した理由

## 更新した docs / schema / contract

<!--
ADR は「なぜ」、docs は「現在どう扱うか」を記録する。
現在仕様・利用方法・永続データ・agent contract に影響する場合は該当文書も更新する。
-->

- [ ] docs 更新なし
- [ ] docs を更新した
- [ ] schema を更新した
- [ ] agent contract / prompt / workflow 文書を更新した

更新箇所:

## 実装しないこと / 非スコープ

<!--
Issue の範囲外として意図的に実装しないこと。
将来 Issue に送る場合は Issue 番号または TODO 方針を書く。
-->

## 互換性・移行・運用上の影響

<!--
CLI, config, state file, schema, logs, worktree/branch, GitHub Issue/PR 操作などに影響がある場合に記載する。
影響なしの場合も「なし」と明記する。
-->

- CLI 互換性:
- config 互換性:
- state / schema 互換性:
- 既存 run / 既存データへの影響:
- 運用上の注意:

## 作成・更新したテスト

<!--
テストを追加・変更・削除・移動・縮小した場合は、テスト名またはテスト群ごとに分類と理由を記載する。
特に Rust integration test と .repor self-test / Rust unit test の間で責務を移す場合は、
どの層に置くべき検証なのかを明示する。

Rust integration test の責務を変更する場合の分類例:
- keep-integration: Rust integration test に残す
- shrink-integration: Rust integration test として残すが検証観点を縮小する
- remove-covered-by-self-test: .repor self-test に移行済みのため削除する
- move-to-unit-focused-test: 内部モデル検証として Rust unit / focused test に移す
- add-self-test: 利用者可視 CLI behavior として .repor self-test を追加する
-->

### <観点 1>

- `<TestName or test group>`
  - 分類:
  - 何を検証するか:
  - 対応する Issue 要件 / 既存仕様:
  - 失敗すると検出できる問題:
  - この層で検証する理由:
  - 対応する self-test / unit test / docs:

### <観点 2>

- `<TestName or test group>`
  - 分類:
  - 何を検証するか:
  - 対応する Issue 要件 / 既存仕様:
  - 失敗すると検出できる問題:
  - この層で検証する理由:
  - 対応する self-test / unit test / docs:

## 手動確認

<!--
自動テストだけでは確認しにくい CLI 出力、ファイル生成、GitHub 操作、異常系など。
未実施の場合は理由を書く。
-->

## 確認したこと

- [ ] `just check`
- [ ] その他:
  - [ ] `...`
  - [ ] `...`

## 未確認事項 / レビュー後に残す論点

<!--
未確認、意図的に保留、別 Issue に送る論点。
「なし」の場合も明記する。
-->
