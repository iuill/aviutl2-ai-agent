# 設計 v0.5

## 現在地

Phase 0は2026-07-28に完了しました。目的はSDKの全挙動を調べ切ることではなく、
Windows + AviUtl2で最小のread-only経路を安全に実装できる根拠を得ることでした。
観測結果の詳細と未検証事項は [`phase0.md`](phase0.md) に残します。

Phase 0で確認できたこと:

- LinuxクロスビルドとWindows native buildのpluginをAviUtl2 2.1.2でロードできる
- loopback HTTP serverとCLIが動作する
- HTTP workerから `call_read_section` を呼べる
- 観測した通常、再生、モーダル表示、scene切替、project再読込後にreadが成功する
- section callbackは呼出元workerと同じthread上で同期的に完了する
- AviUtl2の正常終了時に全HTTP workerをjoinし、listenerを解放できる

未検証のSDK挙動は保証しません。必要になったPhaseで、API範囲を広げる前に追加調査
します。

## Phase 1の目的

Phase 1では、現在のAviUtl2状態をローカルから読み取る小さなHTTP APIとCLIを作ります。
最初の実装範囲は次の2経路だけです。

```http
GET /v1/status
GET /v1/scenes/current
```

- `/v1/status` はSDKを呼ばず、process、plugin、API、read可否などplugin自身の状態を返す
- `/v1/scenes/current` はread section内で現在sceneを読み、SDK型を含まないDTOを返す
- CLIには対応する `status` と `current-scene` を追加する
- 現行の `/healthz` はliveness専用として維持する
- `/phase0/read-section` は移行中の比較用に残し、Phase 1の契約には含めない

timeline、object、effect、font、frame render、event recorder、project epoch、session
discoveryはこの最初のsliceに含めません。利用価値を確かめながら、必要なものを1種類ずつ
追加します。

## 実装境界

- SDK呼出しはtransportから分離した単一の `EditorGate` で直列化する
- gate取得には上限時間を設け、取得できなければ明示的なbusy errorを返す
- `/healthz` と `/v1/status` は `EditorGate` やSDK呼出しに依存させない
- SDKのhandle、enum、文字列所有権をHTTP DTOへ漏らさない
- request DTOは未知fieldを拒否し、response DTOは加算的変更を許容する
- event callbackから `call_edit_section` を呼ばない
- plugin破棄時はlistenerを閉じ、全workerをjoinしてから破棄を完了する
- Windows未実測の挙動を保証済みと記述しない

Phase 1の間は固定loopback port 7890と単一AviUtl2 instanceというPhase 0の制約を
維持します。複数instanceや外部hostからの接続は対象外です。動的port、session
discovery、認証は必要性が生じた時点で一緒に設計します。

## エラー契約

HTTPエラーはSDK固有値ではなく、少なくとも次を区別できる独自DTOにします。

- routeまたはrequestが不正
- EditorGateを期限内に取得できない
- AviUtl2がreadを受け付けなかった
- plugin内部エラー

内部エラーの詳細を無制限に返さず、CLIが終了codeと人向けmessageを安定して扱える
形にします。正確なstatus codeとDTOは、最初のPhase 1 PRでtestとともに確定します。

## Phase 1の完了条件

- `status` と `current scene` のHTTP/CLI契約がtestで固定されている
- Linuxのunit test、正規cross-build、GitHub-hosted Windows runtime smokeが通る
- AviUtl2の通常終了時にworker joinの回帰検査が通る
- 次に必要なread対象を、実利用から1つ選べる

## Phase 2まで禁止するもの

write API、Undo、Redo、project保存は公開しません。edit section、部分失敗、
revision、object identity、Undo単位をPhase 2の開始前に調査し、別の設計更新で
解禁します。

Draft v0.4は履歴資料 [`design-draft-v0.4.md`](design-draft-v0.4.md) として保持します。
