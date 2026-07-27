# 開発

## 必須チェック

正規ビルドは Docker で実行します。

```bash
docker build --output type=local,dest=dist .
```

ローカルに Rust を導入している場合は、次のコマンドを使用できます。

```bash
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

Windows 成果物には Rust 1.88.0、`cargo-xwin` 0.19.2、静的リンクした
MSVC CRT を使用します。`aviutl2` は 0.41.0 に完全固定しています。
更新する場合は、`docs/phase0.md` に記録した互換性チェックを実施してください。

## Phase 0 の境界

固定ポート7890と認証なしの `/healthz` は、単一インスタンスの起動確認用
スパイクにだけ使用します。Phase 1に必要なread関連の事実をWindowsで実測し、
該当する未検証分岐を設計 v0.5 で置き換えるまでは、read-only APIを追加しないで
ください。Undoや部分失敗などwrite固有の調査はPhase 2の開始条件とします。
