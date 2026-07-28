# 開発

## Dev Container

対応ホストはLinuxおよびWSL2です。WindowsネイティブとmacOSは対象外です。

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

このDev Containerを動かす環境では、ホストkernelにiptablesのNAT tableがなく、
通常のDinD daemonが起動できない場合があります。そのため内側daemonのiptablesを
無効化し、上記の `docker build` だけをshim経由でhost networkのBuildKitへ
転送します。Dockerfileと出力方法は正規ビルドと同じですが、CIのDocker buildとは
network modeが異なります。

shimが対象とするのは、上記の形式で呼び出す `docker build` subcommandだけです。
`docker buildx build`、`docker compose build`、global optionを先に置いた呼び出しは
変換しません。

この制約により、Dev Container内の通常のbridge networkにはNATがなく、
bridge接続したcontainerから外部へ通信できません。`docker run -p` による
port公開も使用できません。

Docker-in-DockerのためDev Container自体はprivilegedで起動します。強い
セキュリティ境界とみなさず、信頼できるコードだけを実行してください。
コンテナ内で作成したDocker imageとcontainerは、ホストのDocker daemonには
追加されません。

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
Rustを更新する場合は `rust-toolchain.toml`、ルートの `Dockerfile`、
`.devcontainer/Dockerfile` を、`cargo-xwin` を更新する場合は両Dockerfileを
同時に変更します。あわせて `docs/phase0.md` に記録した互換性チェックを
実施してください。

## Phase 0 の境界

固定ポート7890と認証なしの `/healthz` は、単一インスタンスの起動確認用
スパイクにだけ使用します。Phase 1に必要なread関連の事実をWindowsで実測し、
該当する未検証分岐を設計 v0.5 で置き換えるまでは、read-only APIを追加しないで
ください。Undoや部分失敗などwrite固有の調査はPhase 2の開始条件とします。

## GitHub-hosted Windows runtime spike

手動起動専用の `AviUtl2 runtime spike` workflowは、GitHub-hosted
`windows-2022` 上でAviUtl2を無人起動できるか調べます。AviUtl2 2.1.2のZIPは
作者の配布サイトからworkflow実行中に直接取得し、固定したSHA-256を検証します。
プログラム本体をリポジトリやworkflow artifactへ保存しません。

このworkflowは、AviUtl2の起動、Phase 0 pluginのロード、`health`、
`read-section`、終了後の観測ファイル作成だけを行います。GitHub-hosted runnerの
GPU、DirectX、対話desktop、AviUtl2の初回確認が実行条件を満たすか自体が
検証対象です。成功するまではCIの必須チェックやpush triggerにしません。
