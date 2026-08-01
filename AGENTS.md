# AGENTS.md

## リポジトリ概要

`aviutl2-ai-agent` は、AviUtl2をローカルの構造化APIから操作するための
プロジェクトです。Phase 0のSDK技術スパイクとPhase 1から3の最小API実装は完了し、
MCPのread/write tool、型別object details、text properties update、current frame取得の
実利用確認まで完了しています。
現在は単一operation APIで不足する利用要件の確認を進めています。未検証の挙動は
必要になる直前に追加調査します。

## 参照

- 設計とアーキテクチャ上の制約: `docs/design.md`
- SDK の検証項目、観測結果、Phase 移行条件: `docs/history/phase0.md`
- ビルドと検証方法: `docs/development.md`
- version、tag、GitHub Release: `docs/releases.md`
- 対応バージョン: `docs/compatibility.md`
- Windows 実機確認記録: `docs/verification/windows.md`

変更前に、作業内容と関係する文書を読んでください。未検証の SDK 挙動を
推測で補わず、実装と文書に差異があれば報告してください。
`AGENTS.local.md` が存在する環境では、実機や資格情報に関するローカル運用手順として
作業前に読み、内容をcommit、ログ、応答へ転載しないでください。

## API拡張の境界

- read APIは `docs/design.md` の現行版に記載された範囲から追加する。
- timeline、object、event、renderなどへ範囲を広げる前に、関連する
  `docs/history/phase0.md` の未検証項目を調査する。
- read APIを追加または拡張するPRでは、`docs/design.md` の公開範囲を同じPRで更新する。
- write APIは `docs/design.md` で設計済みかつWindows実測済みの範囲から追加する。
  Undo、Redo、プロジェクト保存は公開しない。
- Windows 実機で確認していない挙動を、確認済みまたは保証済みと記述しない。
- 実機検証では、環境、ビルド元、再現手順、観測結果を記録する。

## 文書の日付

- `docs/` の通常文書は検証対象や機能別に整理し、変更日を見出しや本文へ機械的に付けない。
  文書の変更時期はGit履歴を正とする。
- 日時は、実験ログの時系列、互換性判断、再現条件、protocol versionなど、その値自体が
  技術的な意味を持つ場合だけ記録する。
- 日時を残す場合も、索引や主要見出しは対象別にし、日付順と追記順をナビゲーション構造に
  しない。`docs/history/` の時系列調査記録は例外とする。

## 実装時の注意

- event callback から `call_edit_section` を呼ばない。
- health/status 経路を SDK 呼び出しに依存させない。
- plugin の unload 後に DLL 内のコードを実行し得る worker や task を残さない。
- SDK の型を将来の HTTP 契約へ漏らさない。
- 固定している Rust toolchain、`aviutl2` crate、`cargo-xwin` などを更新する場合は、
  `docs/development.md` に記載された互換性チェックを行い、結果を記録する。
- `dist/` の成果物は手編集せず、正規ビルドで生成する。

## リリース

- 公式release作業では、最初に`docs/releases.md`を読み、その手順を正とする。
- plugin、CLI、MCP Serverはworkspace versionを共有する1セットとしてreleaseする。
- release PRでworkspace version、Cargo.lock、CHANGELOG、互換性文書を同時に更新する。
- CHANGELOGのrelease節はGitHub Release本文として使う。内部作業のPR一覧ではなく、
  利用者に影響する機能、安全性、制約を記述する。
- `vMAJOR.MINOR.PATCH` tagはrelease PRをmainへmergeし、全必須checkが成功した後に付ける。
- push済みtagを別commitへ付け替えない。公開後の修正は次のpatch versionで行う。
- GitHub Release作成後は、tag、version、zipの外部checksum、zip内binaryのchecksumを確認する。

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
