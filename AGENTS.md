# AGENTS.md

## 目的

- このファイルは、`aviutl2-ai-agent` リポジトリで作業するコーディング
  エージェント向けのリポジトリ固有ルールを定義する。
- このリポジトリは、AviUtl2 Plugin SDK の挙動を実測する Phase 0
  技術スパイクである。未検証の挙動を推測で製品仕様にしない。
- 変更は小さく保ち、調査対象と検証結果が追跡できるようにする。

## 参照する文書

- アーキテクチャ上の制約と現在の設計状況: `docs/design.md`
- SDK の検証項目、再現手順、観測結果、Phase 移行条件: `docs/phase0.md`
- 正規ビルドとローカル検証: `docs/development.md`
- 対応バージョンと実機確認状況: `docs/compatibility.md`
- 利用方法と Windows スモークテスト: `README.md`

変更前に対象と関係する文書を読むこと。実装と文書が食い違う場合は、未検証の
事実を推測で補わず、差異を報告する。

## Phase 0 の境界

- Phase 0 は SDK の事実採取と、そのための最小限の検証基盤に限定する。
- `docs/phase0.md` にある Phase 1 の開始条件を満たし、設計 v0.5 に反映するまで、
  read-only API を追加しない。
- write API、Undo、Redo、プロジェクト保存は Phase 2 の調査と設計が完了するまで
  公開しない。
- Windows 実機で確認していない挙動を、確認済みまたは保証済みと記述しない。
- 実機検証結果は、AviUtl2、Windows、crate、Rust、ビルド元のバージョン、
  正確な再現手順、観測結果を `docs/phase0.md` と `docs/compatibility.md` に記録する。

## 実装上の制約

- SDK の型を将来の HTTP 契約へ漏らさない。
- request DTO は未知フィールドを拒否し、response DTO は加算的なフィールド追加を
  許容する方針を維持する。
- event callback では event 情報を queue または atomic state へ記録するだけとし、
  `call_edit_section` を呼ばない。
- SDK 呼び出しを追加する場合は、将来の単一 `EditorGate` による直列化と
  タイムアウトを前提にする。
- health/status 経路を SDK 呼び出しや `EditorGate` に依存させない。
- HTTP worker がプラグインの singleton lock を保持する設計にしない。
- unload 後に DLL 内のコードを実行し得る thread、worker、task を残さない。
  plugin の破棄時は所有する worker を停止し、join してから戻る。
- write の実装検討では `inspect → validate → apply → verify` の順序と、
  project epoch、scene、revision、対象の明示を維持する。

## 依存関係と互換性

- Rust toolchain、`aviutl2` crate、`cargo-xwin` などの固定バージョンを、説明なく
  更新しない。
- `aviutl2` crate を更新する場合は `docs/development.md` と
  `docs/phase0.md` に記載された互換性チェックを実施し、結果を記録する。
- Linux クロスビルドの成功と Windows + AviUtl2 上の実行確認は、別々の合格条件
  として扱う。
- `Cargo.lock` は意図した依存関係更新に伴う場合だけ変更する。

## 作業方針

- 変更前に影響を受ける実装、テスト、文書を確認する。
- 依頼に直接関係しない大規模なリファクタや依存関係追加を避ける。
- 症状への場当たり対応より、原因に対する小さな修正を優先する。
- ユーザーへの応答、commit message、PR title / body / comment は、特段の指定が
  ない限り日本語で書く。
- 破壊的な Git 操作を避け、依頼範囲外のユーザー変更を上書きしない。
- `dist/` の成果物は手編集しない。更新が必要な場合は正規ビルドで再生成し、
  ソース変更との対応を確認する。

## 検証

正規ビルドは Docker で行う。

```bash
docker build --output type=local,dest=dist .
```

Rust コードを変更した場合は、少なくとも次を実行する。

```bash
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

- 変更内容に応じて、正規 Docker ビルドと Windows 実機スモークテストも行う。
- Windows 実機確認が必要だが実施できない場合は、未確認の項目と代替確認を
  最終報告に明記する。
- 文書だけの変更ではコードテストは必須としない。未実施であることを最終報告に
  明記する。
- テキスト検索には `rg` を優先する。
