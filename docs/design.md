# 設計状況

採用した入力文書は、2026-07-27付の Draft v0.4 です。この文書の最重要指示は、
暫定 API の精緻化を止め、Windows + AviUtl2 の実機で、各Phaseに必要な
SDKの事実へ先に答えることです。read-only APIに不要なwrite固有の調査は、
Phase 2の開始前まで後ろ倒しします。

したがって、このリポジトリでは Phase 0 の検証基盤だけを実装しています。

- `protocol`、`plugin`、`cli` に分割した Rust workspace
- Linux Docker を正規経路とする Windows MSVC クロスビルド
- loopback の `/healthz` サーバーを持つ AviUtl2 汎用プラグイン
- プラグイン破棄時の安全なサーバー停止
- CLI のヘルスチェック
- 再現可能な検証手順・結果台帳である [`phase0.md`](phase0.md)

Draft v0.4 から引き継ぐ、安定したアーキテクチャ上の制約は次のとおりです。

- SDK の型を将来の HTTP 契約へ漏らさない
- request DTOは未知フィールドを拒否し、response DTOは将来の加算的な
  フィールド追加を許容する
- すべての SDK 呼び出しを、タイムアウト付きの単一 EditorGate 経由にする
- HTTP worker はプラグインの singleton lock を取得しない。worker が必要とする
  SDK 状態は独立して保持し、singleton の破棄中に worker の join とデッドロック
  しないようにする
- health/status 経路は EditorGate に依存させない
- write は `inspect → validate → apply → verify` の順で行う
- write では project epoch、scene、revision、対象を明示する
- プラグインから AviUtl2 プロジェクトを保存しない
- エージェントが無条件に呼べる Undo/Redo を提供しない
- Linux クロスビルドと Windows 実行時検証を別々の合格条件にする

完全な Draft v0.4 は履歴資料 [`design-draft-v0.4.md`](design-draft-v0.4.md)
として保持します。Phase 0 完了後、
未検証の分岐を実測結果で置き換え、read-only API の実装前に、より短い v0.5 を
この文書へ反映します。
