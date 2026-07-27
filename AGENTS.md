# AGENTS.md

## リポジトリ概要

`aviutl2-ai-agent` は、AviUtl2 Plugin SDK の挙動を実測する Phase 0
技術スパイクです。現段階では製品版 API の実装より、再現可能な検証と
観測結果の記録を優先します。

## 参照

- 設計とアーキテクチャ上の制約: `docs/design.md`
- SDK の検証項目、観測結果、Phase 移行条件: `docs/phase0.md`
- ビルドと検証方法: `docs/development.md`
- 対応バージョンと実機確認状況: `docs/compatibility.md`
- Windows スモークテスト: `README.md`

変更前に、作業内容と関係する文書を読んでください。未検証の SDK 挙動を
推測で補わず、実装と文書に差異があれば報告してください。

## Phase の境界

- `docs/phase0.md` に記載された開始条件を満たし、設計へ反映するまで
  read-only API を追加しない。
- write API、Undo、Redo、プロジェクト保存は、Phase 2 の調査と設計が
  完了するまで公開しない。
- Windows 実機で確認していない挙動を、確認済みまたは保証済みと記述しない。
- 実機検証では、環境、ビルド元、再現手順、観測結果を記録する。

## 実装時の注意

- event callback から `call_edit_section` を呼ばない。
- health/status 経路を SDK 呼び出しに依存させない。
- plugin の unload 後に DLL 内のコードを実行し得る worker や task を残さない。
- SDK の型を将来の HTTP 契約へ漏らさない。
- 固定している Rust toolchain、`aviutl2` crate、`cargo-xwin` などを更新する場合は、
  `docs/development.md` に記載された互換性チェックを行い、結果を記録する。
- `dist/` の成果物は手編集せず、正規ビルドで生成する。

## 検証

Rust コードを変更した場合:

```bash
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

正規ビルド:

```bash
docker build --output type=local,dest=dist .
```

Linux 上の検証と Windows + AviUtl2 での実機確認は、別の合格条件として扱います。
文書だけの変更ではコードテストは必須ではありません。

ユーザーへの応答、commit message、PR title / body / comment は、特段の指定が
ない限り日本語で記述してください。
