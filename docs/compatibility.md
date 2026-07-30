# 互換性

| プラグイン | aviutl2-rs | 最小 AviUtl2 | 実機確認済み | Rust | API | ビルド経路 |
|---|---|---|---|---|---|---|
| 0.0.1（Phase 1） | 0.41.0 | 2.1.2（SDK wrapperの宣言値） | AviUtl2 2.1.2 / Windows Server 2022、2025 | 1.88.0 | `health`、`status`、current scene | Windows native buildとLinux Docker cross-buildを確認 |

Phase 0では、Windows 11上でLinux Dockerクロスビルド成果物を、GitHub-hosted
Windows runner上でWindows native build成果物のロード、health、read section、
正常終了を確認しました。

## 2026-07-28 Phase 1 Windows runtime smoke

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

## 2026-07-29 current scene identity追加観測

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

## 2026-07-28 port 7890競合時の観測

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

## 2026-07-29 Azure Windows VM runtime smoke

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
復元しました。検証中に既知の一過性Winsock 10053を1回観測し、再試行後に上記の
API結果を確認しています。検証後はAviUtl2を終了し、processが残っていません。

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
