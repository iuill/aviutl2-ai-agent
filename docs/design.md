# 設計 v0.5

## 現在地

Phase 0は完了しています。目的はSDKの全挙動を調べ切ることではなく、
Windows + AviUtl2で最小のread-only経路を安全に実装できる根拠を得ることでした。
観測結果の詳細と未検証事項は [`history/phase0.md`](history/phase0.md) に残します。

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

## 公開範囲

Phase 1から3で、現在のAviUtl2状態をローカルから読み書きする小さなHTTP API、CLI、
MCP Serverを実装しました。read APIは次の経路を公開します。

```http
GET /v1/status
GET /v1/scenes/current
GET /v1/scenes/current/timeline
GET /v1/scenes/current/objects
GET /v1/scenes/current/objects/details
GET /v1/scenes/current/frame
```

- `/v1/status` はSDKを呼ばず、process、plugin、API、listenerなどplugin自身の状態を返す
- `/v1/scenes/current` はread section内で現在sceneを読み、SDK型を含まないDTOを返す
- `/v1/scenes/current/timeline` はcurrent sceneの編集情報を所有DTOへコピーして返す
- `/v1/scenes/current/objects` はcurrent sceneのobject snapshotをhandleなしで返す
- `/v1/scenes/current/objects/details` はID、公開種別、状態、text/media設定を返す
- `/v1/scenes/current/frame` は現在frameを `image/png` で返す
- CLIには対応する `status`、`current-scene`、`current-timeline`、
  `current-objects`、`current-object-details`、`current-frame --output <path>` がある
- `/healthz` はliveness専用として維持する

汎用effect更新、event recorder、永続object identity、session discoveryは現在の公開範囲に
含めません。利用価値を確かめ、必要なものを1種類ずつ追加します。

最初のsliceの次に調べるread対象はcurrent scene identityです。`aviutl2` 0.41.0と
Plugin SDK定義にはcurrent sceneのIDと名前がありますが、scene一覧の列挙APIは
ありません。scene IDの切替、再利用、project再読込時の挙動をWindowsで観測し、
安全に契約化できる範囲を決めます。その後はcurrent sceneを対象とするtimeline/object
readを候補とし、current以外のsceneを推測で選択するAPIは公開しません。
Phase 3までの実施順序は [`roadmap.md`](roadmap.md)で管理します。

process外のstdio MCP Serverを提供します。MCP toolはplugin SDKを
直接呼ばず、HTTP APIと同じvalidation、EditorGate、エラー境界を通ります。read toolは
引数を持たない `get_current_scene`、`get_current_timeline`、
`list_current_objects`、`list_current_object_details`、`get_current_frame` です。Windows実測済みの
HTTP契約には、`move_object`、`delete_object`、`create_text_object`、
`update_text_object`、`duplicate_object`、`create_media_object`を1対1で対応させます。
MCP専用mutationや汎用operation toolは
追加しません。
MCP wire処理は公式Rust SDKに委ね、`2026-07-28`の`server/discover` lifecycleと
legacy `initialize` lifecycleの両方を受け付けます。tool schemaはJSON Schema
2020-12としてSDKから生成し、独自JSON-RPC parserは持ちません。

### loopback HTTP契約の見直し

object propertiesとframe追加時に契約全体を見直しました。current instance/current sceneを
pathで固定し、1 request 1 operation、EditorGate、read-back、共通errorを共有する構成は
維持します。汎用operation endpointやbatchは、型別validationとMCP tool schemaを弱めるため
追加しません。対象指定だけは完全snapshotからopaqueな`objectId`へ置き換え、frameだけは
JSON/base64ではなくHTTPの`image/png`を正とします。pluginとCLI/MCPは同一成果物として配る
ため、旧request schemaへのfallbackは実装しません。

write toolは1 call 1 operationで、同じ引数を自動再送しません。HTTP APIの
`mutation_outcome_unknown`を含むcode、message、retryableをtool errorへ保持し、同codeでは
対象に対応するread toolによる実状態の再取得を案内します。annotationはread toolを
read-only、create系をadditive、既存objectを変更または削除するtoolをdestructiveとして
宣言し、全write toolをnon-idempotentかつclosed-worldとします。承認promptの最終判断は
MCP clientに委ねます。

既存projectの内容に基づく編集用に、`GET /v1/scenes/current/objects/details` は
各objectの一時ID、配置、公開種別、状態と型別設定を返します。公開種別は
Windowsで先頭effect名を実測した `text`、`image`、`audio` と、未分類の `unknown`です。
raw aliasは返しません。textは本文、font、size、XYZ位置、色、mediaは素材pathと
再生関連設定を返します。素材pathはagentが既存projectを理解するために必要ですが、
機微情報になり得るためログへ記録しません。従来のobject一覧は小さい配置契約として維持し、
detailsを混在させません。text設定の取得に失敗した1件は`kind: text, text: null`へ降格し、
他objectのdetailsは返します。これは恒常的に読めない1件によって一覧全体をretryableな
503にしないためです。先頭effect自体を取得できないobjectは`unknown`とします。

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
- 描画callbackが保持するDLL内closureを残さないよう、worker join後にrender task完了を待つ
- Windows未実測の挙動を保証済みと記述しない

固定loopback port 7890と単一AviUtl2 instanceという制約を維持します。複数instanceや
外部hostからの接続は対象外です。動的port、session
discovery、認証は必要性が生じた時点で一緒に設計します。
port 7890をbindできない場合、plugin情報に `local API unavailable` と理由を表示し、API
serverなしの無効状態でplugin初期化を完了します。AviUtl2 2.1.2ではplugin初期化から
errorを返した後のhost終了時にaccess violationを観測したため、host processの安全を
優先します。無効状態はAPI requestを受け付けず、次回のAviUtl2起動時に再bindを
試みます。

### readレスポンス契約

`GET /v1/status` はpluginが保持するSDK非依存の値だけから次を返します。

```json
{
  "status": "ok",
  "pluginVersion": "0.1.1",
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
      "id": "obj-0123456789abcdef",
      "layer": 0,
      "startFrame": 10,
      "endFrame": 39,
      "name": "Title"
    }
  ]
}
```

これは呼出時点のcurrent sceneのsnapshotです。`layer`、`startFrame`、`endFrame` は
0始まりで、`endFrame` を含みます。`id` はproject load世代、scene、配置、名前と内部aliasを
非暗号学的hashでまとめたopaqueな一時参照です。秘密を隠す境界ではありません。永続IDではなく、project再読込、移動、本文や設定の
更新で失効します。mutationは同じedit section内で最新objectを列挙してIDを再計算し、
0件またはhash衝突を含む複数件なら変更しません。raw handleとaliasは返しません。

project load callbackによる世代更新と、並行中のHTTP read/mutationは単一snapshotとして
同期されません。loadと競合したrequestは更新前後どちらかの世代を観測し得るため、失効した
IDが拒否された場合はproject load完了後にobject一覧を再取得します。

`GET /v1/scenes/current/frame` はtimelineの現在frameをRGBAで非同期renderし、callback中に
所有bufferへコピーした後、PNGへencodeして返します。HTTP responseは `image/png`、CLIは
指定pathへ保存し、MCPはimage content blockを返します。workerはrender要求を登録した直後に
`wait_rendering_task`を呼び、callbackとそのDLL内コードが完了してからresponseを返します。
plugin unload時は先にHTTP workerをjoinするため、未完了callbackは残りません。AviUtl2が
render完了を通知しない場合はcurrent frame要求とplugin unloadが遅延する制約があります。
unload処理から直接`wait_rendering_task`を呼ぶと、AviUtl2のrendering subsystem停止後に
復帰しないことをWindows実機で観測したため、この順序には戻しません。
CLIとMCPはcurrent frame要求だけを60秒でtimeoutし、利用者の呼出しを有限時間で返します。
このclient timeoutはSDKのrender taskをcancelしないため、plugin workerとunloadはcallbackが
完了するまで待機します。mutationには結果不明を増やすclient timeoutを一律適用しません。

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
mutation結果不明を4、その他の失敗を1とします。code、retryable、CLI終了codeの対応と
`Retry-After` の秒数は最初のPhase 1 PRでtestに固定します。

最初の実装ではEditorGateの取得期限を100ms、`Retry-After` を1秒に固定します。
CLIはrequest不正を終了code 2、`editor_busy` と `editor_unavailable` を終了code 3、
`mutation_outcome_unknown` を終了code 4、その他のAPIエラーを終了code 1として
扱います。終了code 4では同じmutationを再送せず、`current-objects`で実状態を
再取得します。CLI自身のusage errorはclapの終了code 2を維持します。

EditorGateの取得順序はFIFOを保証しません。100msの期限はgate取得だけに適用し、
取得後のSDK呼出し自体を中断しません。SDK呼出しが長時間完了しない場合、後続の
SDK依存requestはbusyになりますが、`/healthz` と `/v1/status` は応答を続けます。
gateは直列化tokenだけを保持するため、SDK呼出しのpanicでmutexがpoisonされた場合も
poisonを解除し、後続requestで再利用します。

Phase 1のHTTP serverはHost headerを必須とするHTTP/1.1 requestだけを受け付けます。

## Phase 1の完了記録

- `status` と `current scene` のHTTP/CLI契約がtestで固定されている
- `/phase0/read-section` が削除され、SDK経路が `EditorGate` に一本化されている
- Linuxのunit test、Docker cross-build、GitHub-hosted Windows runtime smokeが通る
- AviUtl2の通常終了時にworker joinの回帰検査が通る
- port 7890を先に占有した状態でもAviUtl2本体が起動・正常終了し、plugin情報から
  API無効状態を利用者が認識できることをWindowsで実測し、hostの挙動を記録している
- 長時間SDK呼出しを模した状態でも `/healthz` が応答する回帰testが通る
- 次に追加するread対象の選定結果が `design.md` に記録されている

## Phase 2開始時の境界

Phase 1ではwrite API、Undo、Redo、project保存を公開せず、edit section、部分失敗、
object identity、Undo単位を調査してからPhase 2のwrite APIを追加しました。公開済みの
write APIは `inspect → validate → apply → verify` と、scene、対象ID、必要に応じた
IDで読み取り時点の状態を照合する規律を維持します。Undo、Redo、project保存は現在も公開しません。

Draft v0.4は履歴資料
[`history/design-draft-v0.4.md`](history/design-draft-v0.4.md)として保持します。

## Phase 2最小moveの設計

Windows実測により、非event worker threadからedit sectionを呼べる一方、複数mutationの
途中失敗は自動rollbackされないことを確認しました。またraw object handleはUndo復元で
維持され、新規再作成では変わったため、公開identityには使いません。

最初のwriteは既存objectのmoveだけを、1 request 1 mutationで実装します。requestは
current scene名、対象object ID、移動先layerとstart frameを持ちます。処理全体を
1回のEditorGate取得と1回のedit section内で次の順に実行します。

```
POST /v1/scenes/current/objects/move
Content-Type: application/json

{
  "expectedSceneName": "Root",
  "objectId": "obj-0123456789abcdef",
  "destination": {
    "layer": 2,
    "startFrame": 100
  }
}
```

成功時は `{"object": <移動後snapshot>}` を返します。requestの未知フィールドは
拒否し、bodyは16 KiBを上限とします。

1. current scene名がrequestの期待値と一致するか確認する
2. 最新状態から再計算したIDが一致するobjectを列挙する
3. 一致が1件だけであることを確認する
4. inclusiveな移動先範囲が他objectと重ならないことを確認する
5. `move_object` を1回だけ呼ぶ
6. 同じedit section内で移動後のlayerとframe範囲を再取得する
7. 期待結果と一致した場合だけ成功responseを返す

0件はnot found、複数件・scene不一致・移動先競合はconflictとして
mutation前に拒否します。frame計算overflowも拒否します。SDK error後のrollbackは
行わず、apply errorまたはverify失敗は`mutation_outcome_unknown`として返します。
callerは同じmutationを再送せず、current objectsを再読込して実状態を確認します。
Undo/Redo、project保存、
複数operation、raw handle指定は公開しません。

## Phase 3単一deleteの設計

Phase 3の最初の操作は、既存objectを1件だけ削除する
`POST /v1/scenes/current/objects/delete` とします。requestはmoveと同じ
`expectedSceneName` と`objectId`を持ちます。

1. 1回のEditorGate取得と1回のedit section内でscene名を確認する
2. object IDに一致するobjectが1件だけであることを確認する
3. `delete_object`を1回だけ呼ぶ
4. 同じedit section内でhandleが存在しないことを確認する

成功時は`{"deleted": <削除前snapshot>}`を返します。0件はnot found、複数件とscene
不一致はconflictです。project保存、暗黙のUndo、複数object削除は行いません。

## Phase 3単一text createの設計

`POST /v1/scenes/current/objects/text` は、scene名、layer、start frame、length、textを
受け取ります。UI生成textのaliasをWindowsで観測し、effect名と本文項目名がともに
`テキスト`であることを確認しました。

このendpointは汎用effect APIではなく、検証済みのplain text presetです。SDK固有の
effect名、項目schema、aliasは実装内部に閉じ込めます。titleやsubtitleなど別のpresetは、
装飾を含むaliasとread-back方法を対象Windows環境で実測してから個別に追加します。

空objectを作ってから本文設定に失敗する部分適用を避けるため、内部で最小aliasを生成し、
`create_object_from_alias`を1回だけ呼びます。aliasの行境界を壊さないよう、最初の契約は
実際のCR、LF、NULを含むtextを拒否します。AviUtl2が改行として解釈する文字列 `\n` は
aliasの行境界を壊さないため許可します。length 0、frame overflow、同一layerの既存objectと
重なる範囲もmutation前に拒否します。

作成後は同じedit section内でlayer、frame範囲、nameと本文を読み返し、すべて一致した
場合だけ`{"object": <snapshot>, "text": <本文>}`を返します。object名変更、装飾、
複数object生成は別mutationになるため、このendpointには含めません。

## Phase 3単一text updateの設計

`POST /v1/scenes/current/objects/text/update` はscene名、object IDと、本文、font、size、
XYZ位置、色の任意patchを受け取ります。IDが内部aliasを含むため、読み取り後に対象設定が
変わっていればmutation前にnot foundとなります。1回のedit section内でscene、ID、
先頭effectがtextであることを確認し、指定項目を更新して全text設定をread-backします。

scene、ID、object種別の不一致はmutation前に拒否します。SDK更新後の
失敗またはread-back不一致は結果不確定とし、自動再試行せずdetailsの再取得を求めます。
空patchは400とし、本文、font、size、XYZ位置、色のいずれにも実際のCR、LF、NULを許可しません。
sizeとXYZ位置は有限数値、色は6桁の16進数だけを受け付けます。

SDKが数値表記の小数桁や色の英字大小を正規化し得るため、read-backは数値を数値同値、
色をASCII大小文字無視で比較し、それ以外は文字列一致を求めます。丸めや範囲へのclampで
要求値と異なる値になった場合は成功とみなさず、適用後の状態が判明していても
`mutation_outcome_unknown`としてdetailsの再取得を求めます。個別の「適用済みだが補正された」
responseは、実利用で必要になるまで追加しません。

文字装飾、任意effectの更新、project保存、複数object更新は含めません。複数行は
AviUtl2のescape表現である文字列 `\n` を本文に含めます。HTTP JSONではbackslashを
escapeした `\\n` として送信し、read-backでも文字列 `\n` を保持します。実際の改行文字を
SDKへ渡す契約にはしません。文字列 `\n` を改行ではなくbackslashと`n`として表示する方法や、
その他のbackslash sequenceの解釈は未検証です。

## Phase 3単一duplicateの設計

`POST /v1/scenes/current/objects/duplicate` はobject IDと移動先layer、
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
既存の通常ファイルであることだけ確認し、相対pathを拒否します。full pathはmutation response、
ログ、エラーへ含めません。detailsでは既存素材の参照情報として返します。明示的にdebug logを有効にした場合だけ、JSON escapeした
末尾file nameと成否を記録します。

移動先範囲を検証してから`create_object_from_media_file`を1回だけ呼び、作成後の
layerとframe範囲を同じedit section内で確認します。project保存や複数mediaの一括生成は
行いません。

## 複数operationと汎用effectの扱い

Phase 3は1要求1operationを維持します。AIエージェントは各responseまたはobject一覧を
確認しながら単一操作を順に実行します。途中まで適用された一連の操作をAPIが自動で
rollbackするとは保証しません。

1 edit section内の複数operation、client側一時ID、batchの部分失敗契約、複数操作の
Undo単位はPhase 4以降へ先送りします。tool call回数や処理時間、操作間の競合が実利用上
問題になった時点で必要性を再評価します。

任意のeffect名や項目schemaを受け取る汎用APIもPhase 4以降とします。追加する場合は
SDK型やraw aliasをHTTPへ漏らさず、Windowsで観測したeffectごとのversion付き契約を
先に定義します。

## project保存・読み込みのSDK制約

AviUtl2 2.1.2 Plugin SDKと、固定している`aviutl2` / `aviutl2-sys` 0.41.0には、
project本体の保存・読み込みを実行するAPIがありません。確認できるproject関連機能は
次の範囲です。

- 現在のproject file pathの取得
- project load直後とsave直前のcallback登録
- project file内にプラグイン固有の文字列・binary dataを読み書きする機能

`ProjectFile::set_param_string`などはプラグイン固有データをprojectへ格納する機能であり、
project本体をdiskへ保存する命令ではありません。指定pathの読み込み、名前を付けて保存、
上書き保存、未保存変更の照会、保存・読み込み結果の取得に対応するSDK関数は確認できません。

このため、project本体の保存・読み込みはHTTP、CLI、MCPへ公開しません。AviUtl2のmenuや
file dialogを操作するUI AutomationはSDK APIと異なる安全性・安定性境界になるため、
現行pluginの代替実装として暗黙に採用しません。将来SDKに正式なhost操作APIが追加された
場合、またはUI Automationを独立した機能として採用する具体的な利用要件が生じた場合に、
未保存変更、確認dialog、取消し、完了判定を含む契約を改めて設計します。
