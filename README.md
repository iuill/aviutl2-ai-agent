# aviutl2-ai-agent

起動中のAviUtl2プロジェクトを、ローカルの構造化APIから読み書きするための
非公式プロジェクトです。AviUtl2 Plugin SDKを使うプラグインと、CLI、MCP Serverを
Rustで実装しています。AviUtl2公式のプロジェクトではありません。

Phase 0のSDK調査と、Phase 1から3の最小API実装は完了しています。現在は、既存の
プロジェクトをAIエージェントから扱う実利用を通じて、単一operation APIに不足する
要件を確認しています。

## 対応範囲

| 分類 | 対応内容 |
|---|---|
| 読み取り | 現在のscene、timeline概要、object一覧、object種別、text本文 |
| 既存objectの編集 | 移動、削除、複製、text本文の更新 |
| objectの作成 | plain text、Windows絶対pathからのimage / audio |
| 提供形態 | loopback HTTP API、Windows CLI、stdio MCP Server |

すべての変更は1要求につき1件です。APIはプロジェクトを保存せず、Undo / Redo、batch、
汎用effect編集は公開しません。固定の `127.0.0.1:7890` を使う単一AviUtl2 instance向けで、
外部hostからの接続や複数instanceの探索には対応していません。

## アーキテクチャ

![aviutl2-ai-agentのアーキテクチャ](docs/assets/architecture.svg)

AIエージェントはCLIを実行するか、MCP ClientからMCP Serverへtool callを送ります。
CLIとMCP ServerはAviUtl2のprocess外で動作し、loopback HTTP APIを経由します。
プラグインが入力検証、SDKアクセスの直列化、変更後のread-backを担うため、どちらの
経路も同じ安全性境界を通ります。

## ビルド

正規ビルドはLinuxまたはWSL2上のDockerで行います。

```bash
docker build --output type=local,dest=dist .
```

`dist/` に次のWindows x64成果物が生成されます。

- `aviutl2-agent-plugin.aux2`: AviUtl2プラグイン
- `aviutl2-agent.exe`: CLI
- `aviutl2-agent-mcp.exe`: MCP Server
- `SHA256SUMS`: 成果物のSHA-256

プラグインのロードとSDKに依存する検証には、AviUtl2を導入したWindows環境が必要です。

## Windowsで使う

1. `dist/aviutl2-agent-plugin.aux2` をAviUtl2のプラグインディレクトリへコピーします。
2. AviUtl2を起動し、プロジェクトを開きます。
3. PowerShellから接続を確認します。

```powershell
dist\aviutl2-agent.exe health
dist\aviutl2-agent.exe status
dist\aviutl2-agent.exe current-scene
dist\aviutl2-agent.exe current-timeline
dist\aviutl2-agent.exe current-object-details
```

CLIの全commandは次で確認できます。

```powershell
dist\aviutl2-agent.exe --help
```

### 既存textの更新例

最初に `current-object-details` で本文と完全なsnapshotを読み、その値を事前条件として
1件だけ更新します。

```powershell
dist\aviutl2-agent.exe update-text `
  --expected-scene-name Root `
  --layer 2 --start-frame 300 --end-frame 389 `
  --expected-text "修正前" --text "修正後"
```

`expected-text` は取得した本文を正規化せず、そのまま指定します。現在の作成・更新APIは
CR、LF、NULを含む新しい本文を拒否するため、複数行textには対応していません。

### object操作の例

既存objectの移動、削除、複製では、直前のobject一覧またはdetailsで得た完全なsnapshotを
指定します。古いsnapshotや一致しないsceneを指定した場合は変更しません。

```powershell
dist\aviutl2-agent.exe move-object `
  --expected-scene-name Root `
  --layer 0 --start-frame 10 --end-frame 39 --name Title `
  --destination-layer 2 --destination-start-frame 100

dist\aviutl2-agent.exe create-text `
  --expected-scene-name Root `
  --layer 1 --start-frame 100 --length 90 --text "Hello"

dist\aviutl2-agent.exe create-media `
  --expected-scene-name Root `
  --media-path "C:\media\example.png" `
  --layer 1 --start-frame 100 --length 90
```

media pathはcallerが管理する既存のWindows絶対pathに限ります。APIは専用media rootを
設けず、素材pathをread APIで返しません。変更はAviUtl2のedit sectionで実行しますが、
API自身はUndoせず、プロジェクトも保存しません。

接続できない場合は、まず `health` でHTTP APIを切り分けます。port 7890を別processが
使用している場合、プラグインはAPI Serverなしでロードされます。プラグイン情報の
`local API unavailable` を確認し、portを解放してAviUtl2を再起動してください。

## CodexからMCPで使う

Windows x64成果物をCodexへ登録します。絶対pathを登録するため、別のworking directory
からCodexを起動しても同じServerを利用できます。

```powershell
$mcpServer = (Resolve-Path .\dist\aviutl2-agent-mcp.exe).Path
codex mcp add aviutl2 -- $mcpServer
codex mcp list
```

Codexを再起動し、`/mcp` で `aviutl2` と次の10 toolを確認します。

| 読み取り | 変更 |
|---|---|
| `get_current_scene` | `move_object` |
| `get_current_timeline` | `delete_object` |
| `list_current_objects` | `create_text_object` |
| `list_current_object_details` | `update_text_object` |
|  | `duplicate_object` |
|  | `create_media_object` |

例えば、次のように依頼できます。

```text
AviUtl2の現在のobject detailsを取得し、text本文と配置を要約して。
```

```text
本文が「修正前」のtext objectを1件特定し、「修正後」へ更新して、更新後のdetailsを確認して。
```

write toolを実行するかどうかの最終判断と承認方法はMCP Client側に委ねます。接続に
失敗した場合は、先に `dist\aviutl2-agent.exe health` を実行してください。

## 開発

Rustコードを変更した場合の基本チェックは次のとおりです。

```bash
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

Dev Container、依存更新、正規ビルド、診断ログについては
[`docs/development.md`](docs/development.md)を参照してください。

## ローカル環境の情報

個人環境にだけ適用するVMの配置や実機操作は、リポジトリルートの
`AGENTS.local.md` に記述します。このファイルはGit管理対象外で、`AGENTS.md`へ追加する
ローカル手順として扱います。秘密鍵、tokenなどの資格情報そのものは記載しません。

このワークスペースのDev Containerは、Windows実機検証用のcredentialsをホストから
`/run/aviutl2-ai-agent-credentials` へread-onlyでmountできます。credentialsの作成・配置は
公開ドキュメントではなく、各環境のローカル手順で管理してください。

## ドキュメント

資料の役割と読む順序は [`docs/README.md`](docs/README.md) にまとめています。

- [`docs/design.md`](docs/design.md): 現行APIの契約と安全境界
- [`docs/development.md`](docs/development.md): 開発環境、ビルド、検証、診断
- [`docs/compatibility.md`](docs/compatibility.md): 対応versionと実機確認記録への入口
- [`docs/roadmap.md`](docs/roadmap.md): 完了した範囲と今後の候補
