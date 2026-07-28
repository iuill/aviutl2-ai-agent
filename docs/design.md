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
- 観測した逐次呼出しでは、section callbackが呼出元workerと同じRust thread上で
  同期的に完了する
- GitHub-hosted runnerで観測した `WM_CLOSE` 正常終了経路では、全HTTP workerを
  joinし、listenerを解放できる

未検証のSDK挙動は保証しません。必要になったPhaseで、API範囲を広げる前に追加調査
します。

## Phase 1の目的

Phase 1では、現在のAviUtl2状態をローカルから読み取る小さなHTTP APIとCLIを作ります。
最初の実装範囲は次の2経路だけです。

```http
GET /v1/status
GET /v1/scenes/current
```

- `/v1/status` はSDKを呼ばず、process、plugin、API、listenerなどplugin自身の状態を返す
- `/v1/scenes/current` はread section内で現在sceneを読み、SDK型を含まないDTOを返す
- CLIには対応する `status` と `current-scene` を追加する
- 現行の `/healthz` はliveness専用として維持する
- `/phase0/read-section` は最初のPhase 1実装PRで削除し、gateを迂回するSDK経路を残さない

timeline、object、effect、font、frame render、event recorder、project epoch、session
discoveryはこの最初のsliceに含めません。利用価値を確かめながら、必要なものを1種類ずつ
追加します。

## 実装境界

- SDK呼出しはtransportから分離した単一の `EditorGate` で直列化する
- gate取得には上限時間を設け、取得できなければ明示的なbusy errorを返す
- `/healthz` と `/v1/status` は `EditorGate` やSDK呼出しに依存させない
- HTTP workerはpluginのsingleton lockを取得しない。workerが必要とする状態は
  独立して保持し、plugin破棄中のworker joinとデッドロックさせない
- SDKのhandle、enum、文字列所有権をHTTP DTOへ漏らさない
- request DTOは未知fieldを拒否し、response DTOは加算的変更を許容する
- event callbackから `call_edit_section` を呼ばない
- plugin破棄時はlistenerを閉じ、全workerをjoinしてから破棄を完了する
- Windows未実測の挙動を保証済みと記述しない

Phase 1の間は固定loopback port 7890と単一AviUtl2 instanceというPhase 0の制約を
維持します。複数instanceや外部hostからの接続は対象外です。動的port、session
discovery、認証は必要性が生じた時点で一緒に設計します。
port 7890をbindできない場合はplugin初期化を失敗させます。listenerなしの縮退状態では
起動しません。

### 最初のレスポンス契約

`GET /v1/status` はpluginが保持するSDK非依存の値だけから次を返します。

```json
{
  "status": "ok",
  "pluginVersion": "0.0.1",
  "apiVersion": "v1",
  "listenerAddress": "127.0.0.1:7890",
  "processId": 1234
}
```

`status` は現時点では `ok` だけを返します。SDKのread可否を表しません。
`listenerAddress` は実際にbindしたsocket address、`processId` はpluginをロードした
AviUtl2 processのIDです。

`GET /v1/scenes/current` は次を返します。

```json
{
  "name": "Root"
}
```

scene IDやproject情報は、SDK上の意味と寿命を確認してから加算的に追加します。
response DTOは未知fieldを許容し、fieldの削除や意味の変更は行いません。

DNS rebindingとbrowserからの単純なcross-origin GETを避けるため、Phase 1では
`Host: 127.0.0.1:7890` 以外と、`Origin` headerを持つrequestを拒否します。
この制約は認証や複数instance対応を導入するときに再設計します。

`/v1/status` はSDKのread可否を保証しません。`readAvailable` のような推測値は返さず、
将来必要になった場合は最終成功時刻など、観測値であることが分かるfield名で追加します。

## エラー契約

HTTPエラーはSDK固有値ではなく、次の形に固定します。

```json
{
  "code": "editor_busy",
  "message": "EditorGate is busy",
  "retryable": true
}
```

- routeなしは404、request不正は400
- EditorGateの期限切れは503、`code=editor_busy` と `Retry-After` を返す
- AviUtl2がreadを受け付けない場合は503
- plugin内部エラーは500

一時的に再試行できるbusyとread拒否だけ `retryable=true` とし、request不正、
routeなし、内部エラーは `false` とします。内部詳細を無制限に返しません。
CLIは成功を0、usageまたはrequest不正を2、retryableな一時的利用不能を3、
その他の失敗を1とします。code、retryable、CLI終了codeの対応と
`Retry-After` の秒数は最初のPhase 1 PRでtestに固定します。

最初の実装ではEditorGateの取得期限を100ms、`Retry-After` を1秒に固定します。
CLIはrequest不正を終了code 2、`editor_busy` と `editor_unavailable` を終了code 3、
その他のAPIエラーを終了code 1として扱います。CLI自身のusage errorはclapの
終了code 2を維持します。

EditorGateの取得順序はFIFOを保証しません。100msの期限はgate取得だけに適用し、
取得後のSDK呼出し自体を中断しません。SDK呼出しが長時間完了しない場合、後続の
SDK依存requestはbusyになりますが、`/healthz` と `/v1/status` は応答を続けます。
gateは直列化tokenだけを保持するため、SDK呼出しのpanicでmutexがpoisonされた場合も
poisonを解除し、後続requestで再利用します。

Phase 1のHTTP serverはHost headerを必須とするHTTP/1.1 requestだけを受け付けます。

## Phase 1の完了条件

- `status` と `current scene` のHTTP/CLI契約がtestで固定されている
- `/phase0/read-section` が削除され、SDK経路が `EditorGate` に一本化されている
- Linuxのunit test、正規cross-build、GitHub-hosted Windows runtime smokeが通る
- AviUtl2の通常終了時にworker joinの回帰検査が通る
- port 7890を先に占有した状態でもAviUtl2本体が起動を完了し、plugin初期化失敗を
  利用者が認識できることをWindowsで実測し、hostの挙動を記録している
- 長時間SDK呼出しを模した状態でも `/healthz` が応答する回帰testが通る
- 次に追加するread対象の選定結果が `design.md` に記録されている

## Phase 2まで禁止するもの

write API、Undo、Redo、project保存は公開しません。edit section、部分失敗、
revision、object identity、Undo単位をPhase 2の開始前に調査し、別の設計更新で
解禁します。
Phase 2の設計はv0.4の `inspect → validate → apply → verify` と、project epoch、
scene、revision、対象を明示するwrite規律を出発点にします。

Draft v0.4は履歴資料 [`design-draft-v0.4.md`](design-draft-v0.4.md) として保持します。
