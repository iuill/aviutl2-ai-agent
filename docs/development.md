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

Codex認証はホストの `~/.codex/auth.json` を共有します。GitHub CLIもホストの
`~/.config/gh` を共有するため、ホストでログイン済みならコンテナ内で再ログインする
必要はありません。コンテナ内でのログイン、ログアウト、アカウント切り替えは
ホストにも反映されます。

Windows実機検証で使う接続設定は、必要に応じてホストからDev Containerへread-onlyで
共有します。credentialsの作成・配置と接続先は公開文書へ固定せず、各環境の
`AGENTS.local.md` とcredentials側のローカル手順で管理します。
Dev Containerの設定を変更した後や、既存コンテナに反映する場合は `dc rebuild` を
実行します。

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
同時に変更します。あわせて `docs/history/phase0.md` に記録した互換性チェックを
実施してください。

CIの `cross-build` jobは、Dockerfileの `dependencies` stageをGitHub Actions
cacheの `cross-build` scopeへ保存します。このstageは固定toolchain、
`cargo-xwin`、Windows SDKと、manifest・lockfileに対応するLinux/Windows依存crateを
準備します。sourceだけを変更したrunでも、この依存layerを再利用します。workspace
crateの追加、削除、移動や `build.rs` の追加時は、Dockerfileの `dependencies`
stageにあるmanifest、仮source、build scriptのCOPYと生成処理も更新してください。

実際のbuild結果はcacheへexportせず、依存stageだけを保存します。cacheは性能最適化に
のみ使用し、成果物の正しさや再現性の根拠にはしません。

## 実装境界

固定loopback port 7890と単一AviUtl2 instanceという制約を維持します。公開APIの範囲と
安全境界は [`design.md`](design.md)を正とし、readまたはwrite契約を変更する場合は
同じPRで設計文書を更新します。Windowsで未検証のSDK挙動へ依存する機能は、実装前に
調査し、環境、ビルド元、再現手順、観測結果を [`compatibility.md`](compatibility.md)
または [`history/phase0.md`](history/phase0.md)へ記録します。

## GitHub-hosted Windows runtime spike

手動起動専用の `AviUtl2 runtime spike` workflowでは、GitHub-hosted
`windows-2022` 上でAviUtl2を無人起動できるか調べます。AviUtl2 2.1.2のZIPは
作者の配布サイトからworkflow実行中に直接取得し、固定したSHA-256を検証します。
取得したZIPは、公式配布元への反復アクセスを避けるため、バージョンとSHA-256を
keyにしたGitHub Actions cacheへ保存します。cache miss時だけ公式サイトへ
アクセスし、cacheから復元した場合も使用前にSHA-256を再検証します。
プログラム本体をリポジトリやworkflow artifactへ保存しません。

このworkflowは、AviUtl2の起動、pluginのロード、`health`、
`status`、`current-scene`、`current-timeline`、`current-objects` に加え、idle TCP
clientを接続した状態での正常終了、全HTTP workerのjoin、終了後のport 7890再bindを
検査します。
さらにport 7890を先に占有して
AviUtl2を再起動し、plugin初期化が完了しなくてもAviUtl2本体が起動・正常終了できることを
観測します。失敗時のartifact採取に限り、残ったAviUtl2 processを強制終了します。
GitHub-hosted runnerのGPU、DirectX、対話desktop、AviUtl2の初回確認が実行条件を
満たすか自体も検証対象です。
CIの必須チェックやpush triggerにはせず、手動実行でだけ起動します。

## MCP Server

MCP serverはAviUtl2 process外でstdio serverとして起動し、既定では
`http://127.0.0.1:7890` のplugin APIを使います。

```bash
cargo run -p aviutl2-ai-agent-mcp
```

別endpointを使うローカルtestでは `--endpoint` を指定できます。MCP Serverは4つの
read toolと6つのwrite toolを公開し、いずれもHTTP APIのvalidation、EditorGate、
エラー契約を迂回しません。toolの一覧とCodexへの登録方法は
[`README.md`](../README.md#codexからmcpで使う)を参照してください。

公式Rust SDK `rmcp` がMCP `2026-07-28`とlegacy lifecycleのversion negotiation、
JSON-RPC error、notificationを処理します。testでは `server/discover` と `initialize` の
両方を実際のstdio framingで検証します。

## Mutation debug log

Windowsで`AVIUTL2_AI_AGENT_MUTATION_DEBUG_LOG`に出力先を指定すると、media createの
末尾file nameと成否をJSON Linesで記録します。full pathは記録しません。file nameにも
個人情報が含まれ得るため、問題調査時だけ明示的に有効化してください。

## HTTP diagnostic log

接続切断やtimeoutの原因調査では、
`AVIUTL2_AI_AGENT_HTTP_DIAGNOSTIC_LOG`に出力先を指定すると、loopback HTTP serverの
処理段階をJSON Linesで記録できます。各接続について、受付、request受信、route完了、
responseのflush、またはI/O失敗を同じ`connectionId`で追跡できます。I/O失敗には
Rustのerror kind、OS error code、OSが返したmessageを記録します。

request body、text、media path、Host header、接続元addressは記録しません。
requestは既知のAPI routeを固定名へ置換し、未知のpathは`other`として記録します。
診断中だけ有効化し、採取したログは公開前に機微情報がないことを別途確認してください。
