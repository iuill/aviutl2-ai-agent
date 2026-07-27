# 開発

## Dev Container

Dev ContainerにはRust 1.88.0、`cargo-xwin` 0.19.2、Codex CLI、GitHub CLIが
含まれます。VS CodeのDev Containers拡張機能から、このリポジトリをコンテナで
開いてください。コンテナ内のターミナルで次を実行するとCodexを起動できます。

```bash
codex
```

コンテナ内の `codex` は、承認確認とCodex sandboxを無効化して起動します。
workspace内のファイルは制限なく変更でき、マウントした
Codex認証とGitHub CLI設定にもアクセスできます。信頼できないリポジトリや
プロンプトでは使用しないでください。

Docker-in-Dockerを使用し、ホストのDocker socketとホームディレクトリ全体は
マウントしません。コンテナ内のCodexは、独立したDocker daemonを使って正規
Dockerビルドまで実行できます。

```bash
docker build --output type=local,dest=dist .
```

Docker-in-DockerのためDev Container自体はprivilegedで起動します。特にLinux
ホストでは強いセキュリティ境界とみなさず、信頼できるコードだけを実行して
ください。コンテナ内で作成したDocker imageとcontainerは、ホストのDocker
daemonには追加されません。

Codex認証はホストの `~/.codex/auth.json` を共有します。GitHub CLIはホストの
`~/.config/gh-devcontainers` を共有するため、初回はコンテナ内で次を実行します。

```bash
gh auth login
```

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
