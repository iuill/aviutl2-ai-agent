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
```

各コマンドは、Windows 上のプラグインが
`http://127.0.0.1:7890` で待ち受けていることを前提とします。

## Windows Phase 1 スモークテスト

1. `dist/aviutl2-agent-plugin.aux2` を AviUtl2 のプラグインディレクトリへコピーします。
2. AviUtl2 を起動し、プラグイン情報に `aviutl2-ai-agent Phase 1` が表示されることを確認します。
3. `dist\aviutl2-agent.exe health` を実行します。
4. `dist\aviutl2-agent.exe status` を実行します。
5. プロジェクトを開き、`dist\aviutl2-agent.exe current-scene` を実行します。
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
