# Windows実機検証記録

この文書は、特定の環境とビルドで観測した結果を時系列で保持する検証ログです。
現在の対応範囲は [`../compatibility.md`](../compatibility.md)、公開契約は
[`../design.md`](../design.md)を参照してください。

この文書は検証対象別に整理します。実施日は再現性や互換性判断に影響する場合だけ本文へ
記録し、変更時期そのものはGit履歴を正とします。

- runtime基盤: [Phase 1 smoke](#phase-1-windows-runtime-smoke)、
  [metadata](#plugin-metadata更新後のruntime-smoke)、[Azure VM](#azure-windows-vm-runtime-smoke)、
  [port競合](#port-7890競合時の観測)、[HTTP切断](#loopback-http切断の調査)
- API機能: [scene identity](#current-scene-identity追加観測)、
  [object details / text update](#object-details-readとtext-updateの実機確認)、
  [object ID / 型別設定 / frame](#object-id型別設定current-frameの実機確認)
- MCP実利用: [read](#codexからのread-only-mcp実利用評価)、
  [write](#codexからのwrite-mcp実利用確認)

## Rust 1.97 toolchain更新後のruntime smoke

Windows Server 2025 build 26100とAviUtl2 2.1.2の対話sessionで、PR #22の
Rust 1.97.1、cargo-xwin 0.23.0、更新済み依存crateを使う正規Docker build成果物を
検証しました。Linuxではformat、Clippy、workspace test、Windows cross-build、
checksum、PE形式、plugin exportとimport DLLを確認しました。別条件のWindows native
buildとMCP stdio lifecycleもGitHub Actionsで確認しました。

runtime smokeはpluginとCLIを配置し、health、status、current scene、timeline、object
read、idle TCP client接続中の終了、port再bind、port 7890競合時の縮退を順に実行しました。
最初の成果物ではAviUtl2のwindowを閉じた後、plugin unload時に呼んでいたSDKの
`wait_rendering_task`が復帰せず、30秒後に検証scriptがprocessをcleanupしました。同じ
VM状態でmainのRust 1.88成果物も同じ位置で停止したため、toolchain更新による回帰では
ありませんでした。

current frame workerがrender要求の直後に`wait_rendering_task`を呼び、callback完了後に
responseを返すよう寿命管理を変更しました。unloadはHTTP workerをjoinするだけとし、
rendering subsystem停止後には待機しません。修正後の観測結果は次のとおりです。

- `health`は`status=ok`、`pluginVersion=0.1.0`を返した
- `status`、Root scene、空timelineとobject一覧を取得し、current frameのPNG signatureを確認した
- idle client接続中も4 workerをpanicなしでjoinし、456ms、exit code 0で終了した
- 終了後にport 7890を再bindできた
- port競合時はworker 0本で安全にAPIを無効化し、exit code 0で終了した
- 検証後にAviUtl2 process、Scheduled Task、一時成果物を残していない

current frame要求の処理中にAviUtl2を終了する競合条件は未検証です。CLIとMCPには
current frame専用の60秒timeoutを設けますが、plugin内部のrender taskはcancelしません。

## plugin metadata更新後のruntime smoke

Windows Server 2025の対話sessionで、commit `c674244` の正規Docker build成果物と
AviUtl2 2.1.2を使い、既存のruntime smokeを実行しました。AviUtl2公式ZIPは固定した
SHA-256を検証してから展開しました。

- pluginをロードし、health、status、current scene、timeline、object readが成功した
- 正常終了時にHTTP workerをすべてjoinし、port 7890を再利用できた
- port 7890競合時もAviUtl2が正常終了し、worker 0本で終了処理を完了した
- `AVIUTL2_AI_AGENT_LIFECYCLE_LOG` で通常時とport競合時のlifecycleログを取得できた
- 検証後にAviUtl2 process、Scheduled Task、一時成果物を残していない

プラグイン情報画面はこの自動確認では開いていません。表示名と説明文のWindows
cross-buildへの取り込みは生成binaryの文字列でも確認していますが、画面表示の目視確認は
別条件として扱います。

Phase 0では、Windows 11上でLinux Dockerクロスビルド成果物を、GitHub-hosted
Windows runner上でWindows native build成果物のロード、health、read section、
正常終了を確認しました。

## Phase 1 Windows runtime smoke

GitHub Actions run
[`30370093644`](https://github.com/iuill/aviutl2-ai-agent/actions/runs/30370093644)
で、`main` のmerge commit `a5e4ccc193b5761adf82edf08d125cf52ca17429` から
Windows native buildしたpluginとCLIを検証しました。

環境は標準 `windows-2022` runner、runner image `20260720.249.2`、
Windows Server 2022 Datacenter build 20348、Intel Xeon 6973P-C、
Microsoft Hyper-V Videoです。AviUtl2 2.1.2の公式ZIPはActions cacheから復元し、
使用前にSHA-256
`9de5d6bd372cd2b671d50ba93645571bb4c260f694b62d306507ec9d17d70b33`
を再検証しました。

workflowは次を実行しました。

1. pluginとCLIをWindows native release buildする
2. AviUtl2を起動し、pluginの信頼確認を承認する
3. CLIで `health`、`status`、`current-scene` を順に実行する
4. データを送らないTCP clientをport 7890へ接続する
5. AviUtl2のmain windowへ `WM_CLOSE` を送り、正常終了を待つ
6. lifecycle logとport 7890の再bindを検査する

観測結果:

- `health` は `status=ok`、`pluginVersion=0.0.1` を返した
- `status` は `apiVersion=v1`、`listenerAddress=127.0.0.1:7890` を返した
- projectを明示的に開かない初期状態で、`current-scene` は `name=Root` を返した
- `WM_CLOSE` 要求から848msでexit code 0により終了した
- 4本のHTTP workerをすべてjoinし、join panicは0件だった
- idle TCP clientを接続した状態でもplugin dropを完了した
- 終了後にport 7890を再bindできた

この観測は上記runの環境と状態に限られます。Windows 11でのPhase 1成果物、
別のproject状態は未確認です。

## current scene identity追加観測

Windows Server 2025とAviUtl2 2.1.2の対話sessionで、branch上のWindows native
release buildを使ってcurrent sceneを切り替えました。初期Root、追加したScene1、
Rootへの再切替を順にCLIで読み、SDKから得たraw scene IDを診断ログへ記録しました。

- 初期RootはID 0だった
- Scene1を選択するとID 1だった
- Rootへ戻ると再びID 0だった
- scene作成dialogの確認buttonはFlaUIで操作できた
- scene listの行はUI Automation treeに公開されず、選択操作の汎用的な自動化は
  確立できなかった

この観測は同一process内の往復に限られます。project再読込、別project、同名scene、
scene削除後のID再利用は未確認であり、IDは公開APIに含めていません。

同じ環境でcurrent timeline概要を追加したWindows native release buildも検証しました。
空のRootで `1920x1080`、frame rate `30/1`、cursor frame 0、SDKの
`frame_max=0`、`layer_max=0` をCLIから読み取れました。runtime smokeはidle clientを
接続したまま4 workerをpanicなしでjoinし、exit code 0で終了した後にportを再bind
できました。これらの最大値は空のsceneでも0になるため、objectの存在やscene durationを
表す値とは扱いません。

同じbuildのstdio MCP serverから `get_current_timeline` を呼び、pluginのloopback APIを
経由して同じtimeline概要を取得できることも確認しました。

event診断を有効にしたbuildでは、起動時に `project_load` と `change_edit_scene` が
別threadから連続して通知されました。空のRootに対するobject走査は空配列を返しました。
callback内ではSDK read/editを呼ばず、診断値の記録だけを行いました。

FlaUIからtimeline context menuの `ExpandCollapse` と `Invoke` patternを使ってtext
objectを作成し、Delete、Undo、Redo、新規再作成を行いました。作成・削除では
`update_object` と `change_focus_object`、Undo・Redoでは `update_object` を観測しました。
Undo復元時のraw handleは削除前と同じで、Redo後の新規再作成では異なるhandleでした。
この結果は同一process内の1 objectだけに限られ、handleを公開identityとして保証する
ものではありません。

公開APIを持たない一時buildでは、plugin内の非event worker threadからedit sectionを
連続して呼べました。1つのedit section内で1件目のtext作成後、同位置への2件目を
失敗させると、1件目だけが残りました。UI Undo 1回で残った1件目は削除されました。
自動rollbackは観測されず、実測後にprobeコードを製品buildから除去しました。

同じ環境でhandleを含まないcurrent object snapshot APIも検証しました。空のRootでは
空配列、FlaUIで作成したtext objectがある状態ではlayer 0、frame 142から222、
nameなしの1件をCLIとstdio MCPの両方から取得しました。返却値にraw handleは
含まれません。

## port 7890競合時の観測

最初に、port 7890を先に占有し、plugin初期化からbind errorを返す実装を
GitHub Actions run
[`30370567345`](https://github.com/iuill/aviutl2-ai-agent/actions/runs/30370567345)
で試しました。AviUtl2本体はmain windowを作成して2秒以上応答しましたが、
`WM_CLOSE` 後にexit code `-1073741819`（`0xC0000005`、access violation）で
終了しました。このため、AviUtl2 2.1.2ではplugin初期化errorを返す方式を安全な
縮退方法として採用しません。

bind失敗時もplugin objectをAPI serverなしで初期化し、plugin情報へ
`local API unavailable` と理由を表示する実装に変更しました。GitHub Actions run
[`30370897983`](https://github.com/iuill/aviutl2-ai-agent/actions/runs/30370897983)
で、commit `115af66524a042437f7fdb07921bf04de0368aba` のWindows native buildを
再検証しました。

環境は標準 `windows-2022` runner、runner image `20260720.249.2`、
Windows Server 2022 Datacenter build 20348、AMD EPYC 9V74、
Microsoft Hyper-V Videoです。

観測結果:

- port 7890占有中にもAviUtl2のmain window `AviUtl ExEdit2` が作成された
- processは応答ありの状態で2秒以上稼働した
- pluginは `api_start_failed` を記録した
- API serverのworkerは0本で、join panicは0件だった
- plugin dropを完了し、AviUtl2はexit code 0で正常終了した

自動観測ではプラグイン情報画面を開いていません。`local API unavailable` の表示を
利用者が画面上で認識できることは、Windows実機の手動確認項目として残します。

## Azure Windows VM runtime smoke

Azure Windows Server 2025 Datacenter Azure Edition build 26100の専用VMで、
Linux Docker cross-build成果物を検証しました。CPUはIntel Xeon Platinum 8171M
4 core、表示adapterはMicrosoft Hyper-V VideoとMicrosoft Remote Display Adapter
です。ログイン済み検証ユーザーの対話sessionで`InteractiveToken`のScheduled Taskを
起動しました。

ビルド元は本PRの差分を含むworktreeです。Dockerfileへ`.cargo/config.toml`のCOPYを
追加し、正規Dockerビルドで生成したpluginとCLIを使用しました。この観測に使った
一時成果物はrelease成果物として公開しません。

最初の正規ビルドではDockerfileが`.cargo/config.toml`をCOPYしていなかったため、
文書では静的CRTとしていた一方、pluginとCLIが`VCRUNTIME140.dll`とUCRTをimport
していました。VMにはこれらのDLLがなく、AviUtl2はplugin登録時にHRESULT
`0x8007007E`で`LoadLibrary`に失敗しました。Dockerfileを修正して再ビルドし、
PE import tableからこれらの依存が消えたことを確認してから再実行しました。

再現手順:

1. Docker正規ビルドでpluginとCLIを生成する
2. SSHで成果物とruntime smoke scriptをVMへ転送する
3. `InteractiveToken`のScheduled Taskとしてruntime smokeを起動する
4. AviUtl2 2.1.2の公式ZIPのSHA-256を検証し、pluginとCLIを配置する
5. plugin信頼確認を承認し、`health`、`status`、`current-scene`を実行する
6. idle TCP clientを接続した状態で正常終了とworker joinを確認する
7. port 7890を占有して再起動し、安全なAPI無効化と正常終了を確認する

Windows PowerShell 5.1では、一時的なCLI接続失敗と`editor_unavailable`が
`ErrorActionPreference=Stop`によりretry判定前に例外化したため、待機loop内だけ
native command errorを捕捉するようruntime smokeを修正しました。

観測結果:

- Scheduled TaskとAviUtl2はログイン済みユーザーの同じ対話sessionで実行された
- plugin信頼確認を1回承認した
- `health`は`status=ok`、`pluginVersion=0.0.1`を返した
- `status`は`apiVersion=v1`、`listenerAddress=127.0.0.1:7890`を返した
- `current-scene`は`name=Root`を返した
- `WM_CLOSE`要求から224msでexit code 0により終了した
- 4本のHTTP workerをjoinし、join panicは0件だった
- idle TCP client接続中にもplugin dropを完了し、portを再bindできた
- port競合時は`api_start_failed`を記録し、worker 0本、exit code 0で終了した

あわせて.NET 8 SDKとFlaUI 5.0.0を一時導入し、UIA3 probeを同じ
`InteractiveToken` taskから実行しました。probeはAviUtl2と同じ対話sessionで
`UserInteractive=true`となり、AviUtl2のmain window
`AviUtl ExEdit2`、class `aviutl2Manager`、process ID、window handleを取得できました。
別pathへpluginを配置して未承認状態を作り、信頼確認dialog
`スクリプト・プラグインの追加`、class `#32770`を取得しました。dialog配下の
`このプラグイン・スクリプトを信頼して使用する` buttonはAutomation ID `6`、
class `Button`として公開され、UIA InvokePatternをsupportしていました。
dialog、button名、Automation ID、classが一致する唯一の要素へInvokePatternを実行し、
信頼確認後にpluginの`health`が成功することを確認しました。

同じVM、AviUtl2 2.1.2、正規Docker cross-build成果物で、Phase 2のobject moveも
実測しました。FlaUIのUIA3 InvokePatternでRootへtext objectを1件作成し、
`GET /v1/scenes/current/objects` がlayer 0、frame 142から222、nameなしを返すことを
確認してから、次を実行しました。

1. 完全なsnapshotとRootを指定し、layer 2、start frame 300へmoveする
2. 同じ古いsnapshotを再送する
3. UIからUndoし、元のsnapshotへ戻ることを確認する
4. `Expect: 100-continue` を送るcurlとPowerShell 5.1の`Invoke-RestMethod`でも
   moveを往復する

最初のmoveはlayer 2、frame 300から380のsnapshotを返しました。古いsnapshotの再送は
`object_not_found`の404となり、別objectを変更しませんでした。UI Undo 1回で元位置へ
復元しました。`Expect: 100-continue` を使うclientは本文送信まで約1秒待つため、
pluginはこの場合だけ本文待機を2秒へ延長しています。curlと`Invoke-RestMethod`の
両方でmove結果を受信できました。検証後はobjectを元位置へ戻し、AviUtl2を終了して
processが残っていないことを確認しました。

正規ビルドの`aviutl2-agent.exe move-object`でも同じobjectをlayer 2へ移動し、
返された完全なsnapshotを使って元位置へ戻せることを確認しました。

Phase 3の単一object deleteも同じ環境と正規ビルド成果物で確認しました。FlaUIで
作成したtext objectの完全なsnapshotを`aviutl2-agent.exe delete-object`へ渡すと、
削除前snapshotが返り、object一覧は空になりました。同じsnapshotの再送は
`object_not_found`の404となりました。UI Undo 1回で元のlayer 0、frame 142から222へ
復元しました。検証中にWinsock 10053を1回観測し、再試行後に上記のAPI結果を
確認しました。この切断の原因と修正後の確認は後述します。

単一text createでは、UI生成objectから一時診断buildで確認したeffect・項目名を使い、
最小aliasを内部生成する実装を検証しました。正規ビルドの`create-text` CLIで
layer 1、frame 100から189に本文`Hello`のobjectを作成し、responseで同じ本文を
read-backできました。同じ位置への再作成はmutation前に`state_conflict`の409となり、
UI Undo 1回でobject一覧が空へ戻りました。検証後はAviUtl2を終了しました。

単一duplicateでは、上記text objectの内部aliasを取得し、layer 2、frame 200から289へ
複製しました。作成後にframe行を除くalias全体を元objectと比較する実装で成功し、
effectと本文を含む内容が一致することを確認しました。UI Undo 1回では複製だけが消え、
元objectは残りました。検証後はAviUtl2を終了しました。

単一media createでは、検証用の1px PNGと1秒無音WAVをcaller-managedな絶対pathから
作成しました。PNGはlayer 1、frame 100から189、WAVはlayer 2、frame 100から129で
requestどおり作成されました。相対pathは`invalid_request`の400、既存objectと重なる
作成は`state_conflict`の409でした。UI Undo 1回で最後に作成したWAVだけが消え、
PNGは残りました。fixture pathは文書・製品ログへ記録せず、検証後にAviUtl2を終了しました。

MCP `2026-07-28`対応では公式Rust SDK `rmcp` 3.0.1へ移行し、Linuxのstdio integration
testで`server/discover`とlegacy `initialize`の両lifecycleを確認しました。同SDKを含む
Windows x64成果物は正規Docker buildで生成済みです。Windows native CIではrelease版
`aviutl2-agent-mcp.exe`を実際に起動し、stdio経由で`server/discover`とlegacy
`initialize`のresponseを確認しました。cross-buildとは別の合格条件として継続します。

## Codexからのread-only MCP実利用評価

Azure Windows Server 2025 VM、AviUtl2 2.1.2、Codex CLI 0.146.0で、base commit
`0020a37bf69b2fa53e6024db77af9151faa5ac16`に本節を追加する前のworking tree差分を
加え、正規Docker buildしたWindows x64成果物を評価しました。Codex CLIは公式の
standalone installerで導入し、ChatGPTで認証しました。AviUtl2はログイン済みユーザーの
対話sessionへ`InteractiveToken`のScheduled Taskとして起動しました。

`aviutl2-agent-mcp.exe`をCodexへ登録し、同じ空のRootに対して
`get_current_scene`、`get_current_timeline`、`list_current_objects`を1回ずつ呼びました。
非対話の`codex exec`では既定のMCP承認promptがcancel扱いになるため、この評価runだけ
`mcp_servers.aviutl2.default_tools_approval_mode="approve"`をCLI設定で指定しました。
serverが公開するtoolは上記3つだけで、Codex側のsandboxはread-onlyとしました。

3 toolはすべて成功しました。tool本文は順に約20文字、171文字、19文字で、Codexは
Root、1920×1080、30 fps、cursor frame 0、object 0件と要約しました。判断に不足する
metadataとしてscene ID、総尺、音声設定、背景色、project情報が挙げられました。
空sceneだけではobject一覧の冗長さやページング要否を判断できないため、続けてobjectを
含む状態を評価しました。

続けて、保存しない一時fixtureとしてplain text objectを20件作成しました。全objectを
layer 1へ30 frameずつ、frame 100から699まで隙間なく配置し、同じ3 toolをCodexから
1回ずつ呼びました。scene本文は20文字、timeline本文は173文字、object一覧本文は
1,981文字でした。Codexは20件、各30 frame、同一layer、連続配置、nameがすべてnullで
あることを一覧から正しく要約できました。

20件の一覧は切り詰めずに扱える量であり、現時点ではページングを追加しません。
一方、object種別、text本文、素材path、effectと設定、非表示・lock状態は一覧から
判断できませんでした。これらは利用要件として記録し、SDK挙動の調査とread契約の設計を
行う前に、どの判断へ必要かを具体的な操作taskで評価します。fixtureは保存せず、標準の
保存確認で破棄してAviUtl2が終了したことを確認しました。

同じ環境で、複数字幕を組み立てる具体的な操作taskも評価しました。Codex CLIを
`gpt-5.6-luna`、reasoning effort `high`で実行し、plain text字幕5件をframe 100、130、
160、190、220へ、各30 frameの単一`create-text` operationとして順番に作成させました。
Codex側の実行時間は約60秒で、5 operationすべてが成功し、最後に
`list_current_objects`を1回呼んで5件の配置を検証しました。

5件程度では単一operationの反復そのものに実行上の支障は観測せず、batch APIを追加する
根拠にはしません。一方、object一覧は位置とnameだけを返すため、作成後のtext本文は
read-only MCPから再検証できません。作成responseを保持している同一taskでは問題に
なりませんが、既存projectの字幕内容を調べるtaskやsessionをまたぐ検証では不足します。
textまたはobject種別のreadを次候補として評価し、SDK調査なしに契約へ追加はしません。
途中のoperationが失敗した場合、先に成功した字幕は残るため、後続を停止して一覧を
再取得する既存の復旧方針を維持します。fixtureは保存せず破棄しました。

## Codexからのwrite MCP実利用確認

同じWindows VMとAviUtl2 2.1.2で、write parityを実装した正規Docker build成果物を
登録し、Codex CLI 0.146.0を`gpt-5.6-luna`、reasoning effort `high`で実行しました。
この評価runだけMCP toolを承認済みとして扱い、Codex側のsandboxはread-onlyのまま、
AviUtl2操作にはMCP toolだけを使うよう指示しました。

空のRootでtext objectを作成し、完全なsnapshotを使って移動、複製、2件の存在確認、
両方の削除、空一覧の最終確認を順に実行しました。`create_text_object`、`move_object`、
`duplicate_object`、`delete_object`と`list_current_objects`はすべて成功し、最後にCLIからも
object一覧が空であることを独立確認しました。Codexはmutationを自動再試行せず、各response
で得たsnapshotを次のoperationへ渡しました。`create_media_object`は既存のWindows実測済み
HTTP契約を同じstdio integration testで1対1に検証しており、このCodex runでは新たな
media fixtureを作成していません。fixtureは保存せず破棄しました。

## object details readとtext updateの実機確認

Windows Server 2025、AviUtl2 2.1.2、正規Docker build成果物で、plain text、1px PNG、
44.1 kHz・16-bit PCM WAVを保存しない一時fixtureとして作成しました。object detailsは
3件をそれぞれ`text`、`image`、`audio`と分類し、textだけ本文`BEFORE`、他2件は
`text: null`を返しました。raw effect名、alias、素材pathはresponseへ含まれませんでした。

CLIから完全snapshotと`expectedText=BEFORE`を指定して`AFTER`へ更新し、detailsの再取得で
本文を確認しました。続けて古い`expectedText=BEFORE`を再送するとmutation前に
`state_conflict`の409となり、本文は`AFTER`のままでした。
更新後snapshotをSDKから再取得する実装でも再検証し、responseのlayer 0、frame 100から
129、name nullと本文`AFTER`がdetailsの再取得結果と一致しました。

同じ成果物のMCP serverをCodex CLI 0.146.0へ登録し、`gpt-5.6-luna`、reasoning effort
`high`で実行しました。Codexは`list_current_object_details`から本文`AFTER`のtext objectと
image、audioを識別し、完全snapshotと期待する現在本文を指定して`update_text_object`を
1回呼び、`CODEX_UPDATED`へ更新しました。再度detailsを取得して本文と他2objectの残存を
確認し、CLIからも同じ3件と本文を独立確認しました。fixtureは保存せず破棄しました。

未実測の動画などは`unknown`へ分類します。公開種別を増やす場合は、対象fixtureと先頭effect
の対応をWindowsで観測してから追加します。

## loopback HTTP切断の調査

Windows 11、AviUtl2 2.1.2、正規cross-build成果物で、10件のtext createと4 clientから
各12回のhealthを実行しました。修正前はhealth 1件がclient側のWinsock 10054で失敗し、
同時刻のserver診断ログでは、accept直後の最初のreadがWinsock 10035
（`WouldBlock`）で失敗していました。listenerを終了監視のためnonblockingにした結果、
Windowsではacceptしたsocketにもその状態が継承され、request bytesの到着前にreadすると
接続を閉じていたことが原因です。過去に観測した10053も同じ実装に起因するものと
判断しました。

accept後のsocketを明示的にblockingへ戻してからread/write timeoutを設定するよう修正し、
同じ58要求を再実行しました。client失敗は0件、最大応答時間は101 msで、server側も
事前healthを含む59接続すべてがflushまで完了し、I/O失敗は0件でした。続けてmove、
古いsnapshotの再送、duplicate、配置競合、delete、削除済みsnapshotの再送、作成競合、
UI Undo/Redoを実行しました。期待どおり200、404、409へ分岐し、計68接続の診断ログに
I/O失敗はありませんでした。

## object ID、型別設定、current frameの実機確認

正規Linux cross-build成果物をWindows Server 2022 + AviUtl2 2.1.2へ配置し、保存しない
text、PNG、WAV fixtureを作成しました。objects/detailsのIDは同じ状態で一致しました。
textをIDで指定し、本文、font、size、XYZ位置、色を1回で更新すると全項目のread-backが
一致し、更新後は新しいIDが返りました。更新前IDの再送はmutation前の404になりました。

commit `73e47f4` の正規cross-build成果物へ更新し、保存しないtext fixtureに対して
CLIからsizeを`48`、色を`FF0000`として指定しました。更新responseとdetailsの再取得では
それぞれ`48.00`、`ff0000`へ正規化され、更新は200で成功しました。確認後にfixtureを
削除し、object一覧が空であることを確認しました。これは表記の正規化だけを許容する
read-back比較の実測根拠です。丸めやclampで要求値そのものが変わるケースは未検証です。

同じ成果物で文字列 `FIRST\nSECOND`（実際のLFではなくbackslashと`n`）を指定し、textの
作成と既存textの更新をそれぞれ確認しました。どちらもread-backは文字列 `\n` を保持し、
current frameでは`FIRST`と`SECOND`が2行で描画されました。実際のCR、LF、NULを拒否する
入力境界は維持し、複数行にはこのAviUtl2 escape表現を使用します。確認後はfixtureを削除し、
object一覧が空であることを確認しました。

image detailsでは素材path、表示番号、再生速度、loop、連番設定を、audio detailsでは
素材path、再生位置、再生速度、track、loop設定を取得しました。layerの表示・lockと、
text/image/audioが持つ各effectの有効・lock状態も取得できました。

cursor frame 0をrenderし、CLIで保存したファイルがPNG signatureを持つことを確認しました。
同じMCP binaryをCodex CLI 0.146.0へ登録し、`gpt-5.6-luna`、reasoning effort `high`で
`get_current_frame`を1回呼んだところ、image contentを取得して画面内容を説明できました。
検証後にAviUtl2を終了し、一時WAVとPNGを削除しました。
