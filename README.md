# aviutl2-ai-agent

起動中の AviUtl2 プロジェクトを、ローカルの構造化 API から操作するための
Phase 0 技術スパイクです。現段階では製品版のアーキテクチャを実装せず、
まず [`docs/phase0.md`](docs/phase0.md) に記載した AviUtl2 Plugin SDK の
挙動を実測することを目標とします。

これは非公式かつ実験段階のプロジェクトであり、AviUtl2公式のプロジェクトでは
ありません。

## 開発時の確認

```bash
cargo test --workspace
cargo run -p aviutl2-ai-agent -- health
```

`health` コマンドは、Windows 上のプラグインが
`http://127.0.0.1:7890` で待ち受けていることを前提とします。

## Windows Phase 0 スモークテスト

1. `dist/aviutl2-agent-plugin.aux2` を AviUtl2 のプラグインディレクトリへコピーします。
2. AviUtl2 を起動し、プラグイン情報に `aviutl2-ai-agent Phase 0` が表示されることを確認します。
3. `dist\aviutl2-agent.exe health` を実行します。
4. AviUtl2 を終了して再起動し、手順3を繰り返します。再起動後も成功すれば、
   プロセス終了後にポート7890を再利用できることを確認できます。ただし、この
   手順だけでは `UninitializePlugin` 内で全workerのjoinが完了したことまでは
   確認できません。

Phase 0のread-section実測では、プロジェクトを開いて次を実行します。

```powershell
dist\aviutl2-agent.exe read-section
```

これはHTTP workerからSDKのread sectionを呼べるか調べる実験用コマンドです。
製品版のread APIではありません。結果の記録項目と判定範囲は
[`docs/phase0.md`](docs/phase0.md)を参照してください。

ポート7890を固定しているのは、最初の単一インスタンス用スパイクだけです。
セッション探索と衝突しない動的ポートは Phase 1 で実装します。
別のプロセスがすでにポート7890を使用している場合、`InitializePlugin` は失敗し、
AviUtl2 はこの Phase 0 プラグインをロードしません。

## クロスビルド

```bash
docker build --output type=local,dest=dist .
```

`aviutl2-agent-plugin.aux2`、`aviutl2-agent.exe`、`SHA256SUMS` が
生成されます。プラグインのロードと SDK に依存する検証には、
AviUtl2 を導入した Windows 環境が必要です。
