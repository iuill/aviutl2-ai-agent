# ドキュメントガイド

このディレクトリには、現行仕様、開発手順、実機での観測記録、過去の設計資料を
置いています。現行の使い方を知りたい場合は、まずリポジトリルートの
[`README.md`](../README.md)を参照してください。

## 現行資料

| 文書 | 役割 |
|---|---|
| [`design.md`](design.md) | 公開APIの契約、安全境界、実装上の制約 |
| [`development.md`](development.md) | Dev Container、ビルド、テスト、診断ログ |
| [`releases.md`](releases.md) | version、tag、GitHub Releaseの公開手順 |
| [`compatibility.md`](compatibility.md) | 対応versionと、実機確認記録への入口 |
| [`roadmap.md`](roadmap.md) | 完了した実装範囲と、必要性を確認してから検討する候補 |

実装と現行資料が異なる場合は、推測で文書を補わず差異として扱います。Windowsで
確認していないSDK挙動は、保証済みとして記述しません。

## 検証記録

| 文書 | 役割 |
|---|---|
| [`verification/windows.md`](verification/windows.md) | Windows、AviUtl2、Codexでの時系列の実機確認記録 |

検証記録は、特定の環境とビルドで観測した事実を保持します。現在の対応範囲や一般的な
利用手順の代わりには使用しません。

## 調査・履歴資料

| 文書 | 役割 |
|---|---|
| [`history/phase0.md`](history/phase0.md) | SDK技術スパイクの手順、観測結果、未検証項目 |
| [`history/design-draft-v0.4.md`](history/design-draft-v0.4.md) | Phase 0前の設計案。現行仕様ではない履歴資料 |

履歴資料には、現在は採用していない案や完了時点の表現が含まれます。実装判断では
`design.md` と現在のコードを優先してください。

## 情報の置き場所

- 一般利用者が最初に必要とするセットアップと使用例: `README.md`
- 公開契約と長期的に維持する制約: `design.md`
- 再現可能な開発・検証手順: `development.md`
- version、tag、配布成果物の公開手順: `releases.md`
- 対応versionの要約: `compatibility.md`
- 特定環境での実測結果: `verification/`
- 個人環境固有のVM、path、資格情報の参照方法: `AGENTS.local.md`

秘密鍵、token、個人名を含むpath、接続先、資格情報の実体はGit管理対象の文書へ
記録しません。公開に必要な実測記録では、再現性に必要な製品version、OS、手順、
観測結果だけを残します。
