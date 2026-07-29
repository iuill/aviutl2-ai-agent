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
GET /v1/scenes/current/timeline
GET /v1/scenes/current/objects
```

- `/v1/status` はSDKを呼ばず、process、plugin、API、listenerなどplugin自身の状態を返す
- `/v1/scenes/current` はread section内で現在sceneを読み、SDK型を含まないDTOを返す
- `/v1/scenes/current/timeline` はcurrent sceneの編集情報を所有DTOへコピーして返す
- `/v1/scenes/current/objects` はcurrent sceneのobject snapshotをhandleなしで返す
- CLIには対応する `status`、`current-scene`、`current-timeline`、
  `current-objects` を追加する
- 現行の `/healthz` はliveness専用として維持する
- `/phase0/read-section` は最初のPhase 1実装PRで削除し、gateを迂回するSDK経路を残さない

object、effect、font、frame render、event recorder、project epoch、session discoveryは
timeline概要のsliceに含めません。利用価値を確かめながら、必要なものを1種類ずつ
追加します。

最初のsliceの次に調べるread対象はcurrent scene identityです。`aviutl2` 0.41.0と
Plugin SDK定義にはcurrent sceneのIDと名前がありますが、scene一覧の列挙APIは
ありません。scene IDの切替、再利用、project再読込時の挙動をWindowsで観測し、
安全に契約化できる範囲を決めます。その後はcurrent sceneを対象とするtimeline/object
readを候補とし、current以外のsceneを推測で選択するAPIは公開しません。
Phase 3までの実施順序は [`roadmap.md`](roadmap.md)で管理します。

Phase 1.5ではprocess外のstdio MCP serverを追加します。MCP toolはplugin SDKを
直接呼ばず、HTTP APIと同じvalidation、EditorGate、エラー境界を通ります。最初の
toolは引数を持たない `get_current_scene`、`get_current_timeline`、
`list_current_objects` に限定し、write toolは含めません。
MCP wire処理は公式Rust SDKに委ね、`2026-07-28`の`server/discover` lifecycleと
legacy `initialize` lifecycleの両方を受け付けます。tool schemaはJSON Schema
2020-12としてSDKから生成し、独自JSON-RPC parserは持ちません。

## 実装境界

- SDK呼出しはtransportから分離した単一の `EditorGate` で直列化する
- gate取得には上限時間を設け、取得できなければ明示的なbusy errorを返す
- `/healthz` と `/v1/status` は `EditorGate` やSDK呼出しに依存させない
- HTTP workerはpluginのsingleton lockを取得しない。workerが必要とする状態は
  独立して保持し、plugin破棄中のworker joinとデッドロックさせない
- SDKのhandle、enum、文字列所有権をHTTP DTOへ漏らさない
- request DTOは未知fieldを拒否し、response DTOは加算的変更を許容する
- frame、layer、画像サイズなどの非負整数は`u64`へ固定し、Rust build targetの
  pointer幅へ依存させない。SDKの`usize`との変換はplugin境界で検証する
- event callbackから `call_edit_section` を呼ばない
- plugin破棄時はlistenerを閉じ、全workerをjoinしてから破棄を完了する
- Windows未実測の挙動を保証済みと記述しない

Phase 1の間は固定loopback port 7890と単一AviUtl2 instanceというPhase 0の制約を
維持します。複数instanceや外部hostからの接続は対象外です。動的port、session
discovery、認証は必要性が生じた時点で一緒に設計します。
port 7890をbindできない場合、plugin情報に `local API unavailable` と理由を表示し、API
serverなしの無効状態でplugin初期化を完了します。AviUtl2 2.1.2ではplugin初期化から
errorを返した後のhost終了時にaccess violationを観測したため、host processの安全を
優先します。無効状態はAPI requestを受け付けず、次回のAviUtl2起動時に再bindを
試みます。

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

`GET /v1/scenes/current/timeline` は次を返します。

```json
{
  "width": 1920,
  "height": 1080,
  "frameRate": {
    "numerator": 30,
    "denominator": 1
  },
  "cursorFrame": 0,
  "objectEndFrame": 0,
  "highestObjectLayer": 0
}
```

frameとlayerはSDKに合わせて0始まりです。`objectEndFrame` と
`highestObjectLayer` はそれぞれSDKの `frame_max` と `layer_max` の観測値で、
sceneのdurationやlayer数とは定義しません。空のsceneでも0になり得るため、
objectの存在有無を推測する用途には使いません。scene ID、SDK handle、選択状態は
返しません。

`GET /v1/scenes/current/objects` は次を返します。

```json
{
  "objects": [
    {
      "layer": 0,
      "startFrame": 10,
      "endFrame": 39,
      "name": "Title"
    }
  ]
}
```

これは呼出時点のcurrent sceneのsnapshotです。`layer`、`startFrame`、`endFrame` は
0始まりで、`endFrame` を含みます。この組を永続IDとは定義せず、scene切替や編集後も
同じobjectを指すとは保証しません。raw handle、effect設定、file pathは返しません。
Phase 2でobjectを変更する場合はlocatorと期待するsnapshotを同じedit section内で
再検証し、0件または複数件なら変更しません。

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
- port 7890を先に占有した状態でもAviUtl2本体が起動・正常終了し、plugin情報から
  API無効状態を利用者が認識できることをWindowsで実測し、hostの挙動を記録している
- 長時間SDK呼出しを模した状態でも `/healthz` が応答する回帰testが通る
- 次に追加するread対象の選定結果が `design.md` に記録されている

## Phase 2まで禁止するもの

write API、Undo、Redo、project保存は公開しません。edit section、部分失敗、
revision、object identity、Undo単位をPhase 2の開始前に調査し、別の設計更新で
解禁します。
Phase 2の設計はv0.4の `inspect → validate → apply → verify` と、project epoch、
scene、revision、対象を明示するwrite規律を出発点にします。

Draft v0.4は履歴資料 [`design-draft-v0.4.md`](design-draft-v0.4.md) として保持します。

## Phase 2最小moveの設計

Windows実測により、非event worker threadからedit sectionを呼べる一方、複数mutationの
途中失敗は自動rollbackされないことを確認しました。またraw object handleはUndo復元で
維持され、新規再作成では変わったため、公開identityには使いません。

最初のwriteは既存objectのmoveだけを、1 request 1 mutationで実装します。requestは
current scene名、対象の完全なsnapshot、移動先layerとstart frameを持ちます。処理全体を
1回のEditorGate取得と1回のedit section内で次の順に実行します。

```
POST /v1/scenes/current/objects/move
Content-Type: application/json

{
  "expectedSceneName": "Root",
  "target": {
    "layer": 0,
    "startFrame": 10,
    "endFrame": 39,
    "name": "Title"
  },
  "destination": {
    "layer": 2,
    "startFrame": 100
  }
}
```

成功時は `{"object": <移動後snapshot>}` を返します。requestの未知フィールドは
拒否し、bodyは16 KiBを上限とします。

1. current scene名がrequestの期待値と一致するか確認する
2. layer、start、end、nameが完全一致するobjectを列挙する
3. 一致が1件だけであることを確認する
4. inclusiveな移動先範囲が他objectと重ならないことを確認する
5. `move_object` を1回だけ呼ぶ
6. 同じedit section内で移動後のlayerとframe範囲を再取得する
7. 期待結果と一致した場合だけ成功responseを返す

0件はnot found、複数件・scene不一致・snapshot変更・移動先競合はconflictとして
mutation前に拒否します。frame計算overflowも拒否します。SDK error後のrollbackは
行わず、apply errorまたはverify失敗は`mutation_outcome_unknown`として返します。
callerは同じmutationを再送せず、current objectsを再読込して実状態を確認します。
Undo/Redo、project保存、
複数operation、raw handle指定は公開しません。

## Phase 3単一deleteの設計

Phase 3の最初の操作は、既存objectを1件だけ削除する
`POST /v1/scenes/current/objects/delete` とします。requestはmoveと同じ
`expectedSceneName` と完全な`target` snapshotを持ちます。

1. 1回のEditorGate取得と1回のedit section内でscene名を確認する
2. target snapshotに完全一致するobjectが1件だけであることを確認する
3. `delete_object`を1回だけ呼ぶ
4. 同じedit section内でhandleが存在しないことを確認する

成功時は`{"deleted": <削除前snapshot>}`を返します。0件はnot found、複数件とscene
不一致はconflictです。project保存、暗黙のUndo、複数object削除は行いません。

## Phase 3単一text createの設計

`POST /v1/scenes/current/objects/text` は、scene名、layer、start frame、length、textを
受け取ります。UI生成textのaliasをWindowsで観測し、effect名と本文項目名がともに
`テキスト`であることを確認しました。

空objectを作ってから本文設定に失敗する部分適用を避けるため、内部で最小aliasを生成し、
`create_object_from_alias`を1回だけ呼びます。aliasの行境界を壊さないよう、最初の契約は
CR、LF、NULを含むtextを拒否します。length 0、frame overflow、同一layerの既存objectと
重なる範囲もmutation前に拒否します。

作成後は同じedit section内でlayer、frame範囲、nameと本文を読み返し、すべて一致した
場合だけ`{"object": <snapshot>, "text": <本文>}`を返します。object名変更、装飾、
複数object生成は別mutationになるため、このendpointには含めません。

## Phase 3単一duplicateの設計

`POST /v1/scenes/current/objects/duplicate` は完全なtarget snapshotと移動先layer、
start frameを受け取ります。targetを一意に特定し、元objectを含む既存objectとの
移動先競合を検証した後、SDKから取得したaliasを外部へ返さず
`create_object_from_alias`を1回だけ呼びます。

作成後はlayer、frame範囲、nameを同じedit section内で読み返します。targetなしは
not found、複数一致と移動先競合はconflictです。effectやmedia pathを含み得るaliasは
内部値としてだけ扱い、HTTP responseやログへ出しません。

## Phase 3単一media createの設計

`POST /v1/scenes/current/objects/media` はimage/audio共通で、callerが管理するWindows
絶対path、scene、layer、start frame、lengthを受け取ります。個人開発用途ではmedia
rootを設けず、pathの選択責任はCodexなどのcallerが持ちます。pluginは絶対pathであり
既存の通常ファイルであることだけ確認し、相対pathを拒否します。full pathはresponse、
ログ、エラーへ含めません。明示的にdebug logを有効にした場合だけ、JSON escapeした
末尾file nameと成否を記録します。

移動先範囲を検証してから`create_object_from_media_file`を1回だけ呼び、作成後の
layerとframe範囲を同じedit section内で確認します。project保存や複数mediaの一括生成は
行いません。
