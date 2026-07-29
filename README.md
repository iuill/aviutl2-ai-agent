# aviutl2-ai-agent

起動中の AviUtl2 プロジェクトを、ローカルの構造化 API から操作するための
プロジェクトです。AviUtl2 Plugin SDKの基本挙動を調べるPhase 0は完了し、
現在は最小のread-only APIを作るPhase 1です。Phase 0の観測結果は
[`docs/phase0.md`](docs/phase0.md)、現在の設計範囲は
[`docs/design.md`](docs/design.md)、Phase 3までの実施順序は
[`docs/roadmap.md`](docs/roadmap.md)を参照してください。

これは非公式かつ実験段階のプロジェクトであり、AviUtl2公式のプロジェクトでは
ありません。

## 開発時の確認

```bash
cargo test --workspace
cargo run -p aviutl2-ai-agent -- health
cargo run -p aviutl2-ai-agent -- status
cargo run -p aviutl2-ai-agent -- current-scene
cargo run -p aviutl2-ai-agent -- current-timeline
cargo run -p aviutl2-ai-agent -- current-objects
```

read-only MCP serverはstdioで起動します。公式Rust SDKを使用し、MCP `2026-07-28`の
`server/discover` lifecycleとlegacy `initialize` lifecycleをサポートします。

```bash
cargo run -p aviutl2-ai-agent-mcp
```

公開するtoolは `get_current_scene`、`get_current_timeline`、
`list_current_objects` で、MCP server自身もloopback HTTP APIを経由します。

各コマンドは、Windows 上のプラグインが
`http://127.0.0.1:7890` で待ち受けていることを前提とします。

## Windows Phase 1 スモークテスト

1. `dist/aviutl2-agent-plugin.aux2` を AviUtl2 のプラグインディレクトリへコピーします。
2. AviUtl2 を起動し、プラグイン情報に `aviutl2-ai-agent Phase 1` が表示されることを確認します。
3. `dist\aviutl2-agent.exe health` を実行します。
4. `dist\aviutl2-agent.exe status` を実行します。
5. プロジェクトを開き、`dist\aviutl2-agent.exe current-scene`、
   `dist\aviutl2-agent.exe current-timeline`、`dist\aviutl2-agent.exe current-objects`
   を実行します。

既存objectのmoveは、直前の`current-objects`で得た完全なsnapshotを指定します。

```powershell
dist\aviutl2-agent.exe move-object `
  --expected-scene-name Root `
  --layer 0 --start-frame 10 --end-frame 39 --name Title `
  --destination-layer 2 --destination-start-frame 100
```

nameが`null`のobjectでは`--name`を省略します。APIはprojectを保存せず、moveは
AviUtl2のUndo対象になります。

単一objectの削除も完全なsnapshotを指定します。

```powershell
dist\aviutl2-agent.exe delete-object `
  --expected-scene-name Root `
  --layer 0 --start-frame 10 --end-frame 39 --name Title
```

削除もAviUtl2のUndo対象であり、CLIはprojectを保存しません。

改行を含まない単一text objectは次のように作成します。

```powershell
dist\aviutl2-agent.exe create-text `
  --expected-scene-name Root `
  --layer 1 --start-frame 100 --length 90 --text "Hello"
```

初期契約ではtextにCR、LF、NULを指定できません。作成はAviUtl2のUndo対象で、
CLIはprojectを保存しません。

既存objectの複製も、元objectの完全なsnapshotと重ならない移動先を指定します。

```powershell
dist\aviutl2-agent.exe duplicate-object `
  --expected-scene-name Root `
  --layer 1 --start-frame 100 --end-frame 189 `
  --destination-layer 2 --destination-start-frame 200
```

image/audioはcallerが管理するWindows絶対pathから作成します。

```powershell
dist\aviutl2-agent.exe create-media `
  --expected-scene-name Root `
  --media-path "C:\media\example.png" `
  --layer 1 --start-frame 100 --length 90
```

専用media rootによる制限はありません。相対pathと存在しないfileは拒否されます。
6. AviUtl2 を終了して再起動し、手順3を繰り返します。再起動後も成功すれば、
   プロセス終了後にポート7890を再利用できることを確認できます。ただし、この
   手順だけでは `UninitializePlugin` 内で全workerのjoinが完了したことまでは
   確認できません。

Phase 1では単一AviUtl2 instance用にloopback port 7890を固定しています。
動的port、session discovery、認証は必要性が生じた時点で一緒に設計します。
別のプロセスがすでにポート7890を使用している場合、pluginはAPI serverなしの
無効状態でロードされます。プラグイン情報に表示される
`local API unavailable` の理由を確認し、portを解放してAviUtl2を再起動してください。

## クロスビルド

```bash
docker build --output type=local,dest=dist .
```

`aviutl2-agent-plugin.aux2`、`aviutl2-agent.exe`、`SHA256SUMS` が
生成されます。プラグインのロードと SDK に依存する検証には、
AviUtl2 を導入した Windows 環境が必要です。
