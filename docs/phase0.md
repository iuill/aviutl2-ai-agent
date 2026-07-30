# Phase 0：SDK の事実採取

この文書は API 仕様ではなく、検証結果の台帳です。各項目について、AviUtl2
のバージョン、`aviutl2` crate のバージョン、ビルド元、正確な再現手順、
観測結果、ログを記録します。調査は、その事実が必要になるPhaseの直前までに
完了させます。write固有の調査によってread-only実装を止めません。

## 検証環境

| 項目 | 値 |
|---|---|
| AviUtl2 | 2.1.2 |
| `aviutl2` crate | 0.41.0 |
| Rust | 1.88.0 |
| cross-build base image | `rust:1.88.0-bookworm@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0` |
| Windows バージョン | Windows 11 |

各Qの「完了（Phase 0範囲）」は、そのQを網羅した意味ではありません。最小Phase 1で
必要な観測を終え、残項目をAPI拡張直前またはPhase 2へ延期した状態です。

## 起動確認

- [x] Linux Docker から `.aux2` と `.exe` を生成できる
- [x] `.aux2` をロードし、プラグインを登録できる
- [x] `GET /healthz` が応答する
- [x] CLI が health 応答を解釈できる
- [x] プラグインの正常終了時に全HTTP workerが停止し、joinされる
- [ ] AviUtl2 がプロセス実行中にもDLLをunloadするのか、プロセス終了時だけかを確認する

### 2026-07-27 Windows実機確認

Linux DockerでクロスビルドしたプラグインをAviUtl2の `data/Plugin` へ配置し、
初回の信頼確認後、汎用プラグインとして情報画面に表示されることを確認しました。
この確認はプロジェクト識別子を現名称へ統一する前の成果物で実施したため、
現名称で再ビルドした成果物の確認結果は
「[2026-07-28 GitHub-hosted Windows runner観測](#2026-07-28-github-hosted-windows-runner観測)」
に記録しています。

確認した項目は、バージョン `0.0.1` と「汎用プラグイン」の種別です。

AviUtl2起動中に、同じディレクトリへ配置したWindows CLIから次を実行しました。

```powershell
.\aviutl2-agent.exe health
```

応答:

```json
{
  "status": "ok",
  "pluginVersion": "0.0.1"
}
```

これにより、クロスビルド成果物のロード、プラグイン登録、loopback HTTP応答、
response DTOの解釈を確認しました。その後AviUtl2を終了・再起動し、同じ
`health` コマンドが再び成功することを確認しました。したがって、プロセス終了後に
port 7890が残留せず、再起動したプラグインが再bindできることは確認済みです。
ただし、プロセス終了時にはOSもthreadとsocketを回収するため、この結果だけでは
`UninitializePlugin` 内で全workerのjoinが完了したことまでは証明できません。

## Q1 — section のスレッド親和性と再入性

状態：**完了（Phase 0範囲）** — 並列・終了競合はAPI拡張前へ繰延

- [x] HTTP worker からread sectionを呼ぶためのPhase 0プローブを実装する
- [x] HTTP worker から read section を呼ぶ（通常状態）
- [x] plugin内の非event worker threadから edit section を呼ぶ
- [x] event callback のthread IDを記録する
- [x] event callback 内ではevent情報をqueueまたはatomic stateへ記録するだけとする
- [x] event callback から `call_edit_section` を呼ばない
- [x] edit section 内で現在状態を読み取る
- [x] edit section を連続で呼ぶ
- [ ] edit section を入れ子で呼ぶ
- [ ] section 実行中に終了する

呼び出し可能なスレッドと、必要なdispatcher設計を記録します。
`aviutl2-rs` 0.41.0はevent用スレッドからの `call_edit_section` を禁止しているため、
この経路は危険な実験を行わず、上流仕様により禁止と確定します。
製品ではすべてのSDK呼び出しをEditorGateで直列化するため、生のread/read、
read/write、write/write並列呼び出しは必須完了条件にしません。必要になった場合だけ、
使い捨てプロジェクトを用いた追加調査として実施します。

> 通常状態でのHTTP workerからのread section呼び出しは成功しました。
> 特殊状態と終了処理との競合は未検証です。

2026-07-29にWindows Server 2025とAviUtl2 2.1.2で、公開APIを持たない一時buildを
使い、plugin内で生成した非event worker threadから `call_edit_section` を呼びました。
1回目のcallback内でobject数を読み、text objectを作成し、作成したhandleから名前を
読み取った後、同じ位置への2件目の作成を意図的に失敗させました。callbackは完了し、
続く2回目の `call_edit_section` も成功してobject数1を返しました。

この結果から、観測した通常状態では非event worker threadからedit sectionを呼べ、
edit内readと連続呼出しが成立しました。入れ子、並列、終了競合は未検証です。probeは
実測後に製品buildから除去し、公開write経路にはしていません。

2026-07-29に同じ環境で、FlaUIから作成した既定text objectのaliasを一時診断buildで
取得しました。先頭effectは`テキスト`、本文の項目名も`テキスト`で、2番目のeffectは
`標準描画`でした。診断buildとraw aliasは製品worktreeへ残していません。

### HTTP worker read-sectionプローブ

`GET /phase0/read-section` は、HTTP workerから `call_read_section` を1回呼び、
現在のシーン名を読み取ります。workerとsection callbackの
thread ID、呼び出し時間、失敗理由もJSONで返します。これはQ1の事実採取専用で、
Phase 1の公開APIではありません。`GET /healthz` は引き続きSDKを呼びません。

現名称の正規ビルド成果物をWindows + AviUtl2へ配置し、プロジェクトを開いた状態で
次を実行します。

```powershell
.\aviutl2-agent.exe health
.\aviutl2-agent.exe read-section
.\aviutl2-agent.exe read-section
```

各コマンドの完全なJSON、AviUtl2とWindowsのバージョン、配置した成果物の
`SHA256SUMS`、プロジェクトの状態、実行時刻をこの節へ記録します。成功した場合も、
この1条件だけから再生中、モーダル表示中、終了中などの安全性は推定しません。

### 2026-07-28 Windows実機観測

Windows 11 + AviUtl2 2.1.2で、AviUtl2を起動してRootシーンを表示した通常状態から
現名称のクロスビルド成果物を実行しました。`health` は次を返しました。

```json
{
  "status": "ok",
  "pluginVersion": "0.0.1"
}
```

続けて `read-section` を2回実行しました。

```json
{
  "success": true,
  "workerThread": "ThreadId(1)",
  "callbackThread": "ThreadId(1)",
  "elapsedMicros": 26,
  "sceneName": "Root",
  "error": null
}
```

```json
{
  "success": true,
  "workerThread": "ThreadId(1)",
  "callbackThread": "ThreadId(1)",
  "elapsedMicros": 5,
  "sceneName": "Root",
  "error": null
}
```

この観測から、少なくとも上記の通常状態では次を確認しました。

- HTTP workerから `call_read_section` を呼び出せる
- section callbackは呼び出したworkerと同じRust thread ID上で実行される
- callbackが返るまでにシーン名を読み取れる
- 連続した2回の呼び出しが成功する

SDK内部の実装方式や、すべてのworker・編集状態における安全性までは、この観測から
推定しません。再生中、モーダル表示中、プロジェクト再読込中、終了中の挙動と、
複数HTTP workerをまたぐ直列化方式は引き続き未検証です。今回使用した成果物の
`SHA256SUMS` と実行時刻は当時未記録で、後から復元できません。後述の正規ビルド
成果物ハッシュは、その成果物を同定するための値です。再ビルドとのバイト単位の一致は
期待しません。

### 2026-07-28 特殊状態と状態変更後の追加観測

前節と同じ実機環境と成果物で、次の順に `read-section` を実行しました。

1. タイムライン再生中
2. モーダルダイアログ表示中
3. `Scene1` へシーンを切り替えた後
4. 別プロジェクトを読み込んだ後

結果は順に次のとおりでした。

```json
{
  "success": true,
  "workerThread": "ThreadId(3)",
  "callbackThread": "ThreadId(3)",
  "elapsedMicros": 12,
  "sceneName": "Root",
  "error": null
}
```

```json
{
  "success": true,
  "workerThread": "ThreadId(2)",
  "callbackThread": "ThreadId(2)",
  "elapsedMicros": 4,
  "sceneName": "Root",
  "error": null
}
```

```json
{
  "success": true,
  "workerThread": "ThreadId(4)",
  "callbackThread": "ThreadId(4)",
  "elapsedMicros": 5,
  "sceneName": "Scene1",
  "error": null
}
```

```json
{
  "success": true,
  "workerThread": "ThreadId(1)",
  "callbackThread": "ThreadId(1)",
  "elapsedMicros": 4,
  "sceneName": "Root",
  "error": null
}
```

この結果から、観測した再生中とモーダル表示中にもread sectionが成功したこと、
シーン切替後とプロジェクト再読込後に現在のシーン名を読み取れたことを確認しました。
また、4つのHTTP workerすべてで呼び出しが成功し、各section callbackのRust
thread IDは呼び出したworkerと一致しました。これは逐次実行の観測であり、SDK呼び出しの
並列安全性は示しません。

プロジェクト読込処理そのものとread sectionの競合、終了処理との競合、
モーダルダイアログの種類による差異は未検証です。
次にthread同一性を観測する場合は、Rust `ThreadId` に加えてWindowsの
`GetCurrentThreadId` も記録します。

### 2026-07-28 GitHub-hosted Windows runner観測

GitHub Actions run `30357153530` の標準 `windows-2022` runnerで、AviUtl2
2.1.2の公式ZIPを取得してSHA-256を検証し、native buildしたpluginとCLIを配置して
AviUtl2を起動しました。AviUtl2のplugin信頼確認overlayは、対象processの
メインウィンドウを確認したうえで1回だけ自動承認しました。

観測環境はWindows Server 2022 Datacenter build 20348、runner image
`20260720.249.2`、AMD EPYC 7763、Microsoft Hyper-V Videoでした。

`health` と `read-section` は次の結果になりました。

```json
{
  "status": "ok",
  "pluginVersion": "0.0.1"
}
```

```json
{
  "success": true,
  "workerThread": "ThreadId(3)",
  "callbackThread": "ThreadId(3)",
  "elapsedMicros": 32,
  "sceneName": "Root",
  "error": null
}
```

これにより、GitHub-hosted Windows runnerでもAviUtl2の無人起動、pluginロード、
loopback HTTP応答、HTTP workerからのread section呼び出しが成立することを
確認しました。runner終了時はハーネスがAviUtl2を強制終了しているため、
この観測は `UninitializePlugin` とworker joinの検証には使用しません。

## Q2 — Undo と部分失敗

状態：**調査中（Phase 2開始前）**

- [ ] 1つのedit sectionで2オブジェクトを変更し、Undoを1回実行する
- [x] 2件目のmutationを意図的に失敗させる
- [ ] オブジェクト作成後、設定更新を失敗させる
- [ ] 複合操作内で削除し、Undoする
- [x] 明示的なrollback APIの有無を調べる

Undoの粒度と、部分的な変更が残るかを記録します。

同じ2026-07-29の一時buildで、1つのedit section内の1件目にtext objectを作成し、
同位置への2件目の作成を失敗させました。callback終了後も1件目は残り、SDKによる
自動rollbackは行われませんでした。その後のUI Undo 1回で残った1件目が消えました。

`aviutl2` 0.41.0のgeneric APIと同versionのPlugin SDK定義には、明示的なtransaction、
rollback、Undo単位を指定するAPIは見つかりませんでした。従って複数mutationを
原子的とは扱えず、Phase 2の最初の公開writeは1 request 1 mutationに限定します。
設定更新失敗と削除を含む複合操作のUndo粒度は引き続き未検証です。

## Q3 — フレームレンダリング

状態：**繰延（frame read API追加前）**

- [ ] 明示したscene/frameをレンダリングする
- [ ] 呼び出し元とcallbackのスレッドを記録する
- [ ] callbackから戻る前にpixelを所有bufferへコピーする
- [ ] pixel format、pitch、alpha、bufferの寿命を記録する
- [ ] レンダリング解像度を指定できるか確認する
- [ ] 再生中、出力中、modal dialog表示中にレンダリングする
- [ ] 連続呼び出しと大解像度で計測する
- [ ] キャンセル方法を確認する

> 未検証

## Q4 — editor のbusy状態

状態：**完了（Phase 0範囲）** — 未観測のbusy状態はAPI拡張前へ繰延

タイムラインのドラッグ中、modal dialog表示中、再生中、出力中、
プロジェクトの読込・保存中、Undo/Redo中、終了中にread/write/renderを試します。
SDKを呼ぶ前に、その状態を判定できるか記録します。

通常状態、再生中、modal dialog表示中のread sectionは成功しました。modal
dialogの種類は未記録です。タイムラインのドラッグ中、出力中、プロジェクトの
読込・保存処理中、Undo/Redo中、終了中は未検証です。writeとrenderについては
まだ検証していません。

## Q5 — event、revision、handle

状態：**完了（Phase 0範囲）** — eventとidentityはAPI拡張前へ繰延

作成、更新、移動、削除、effect変更、scene切替、API由来の変更、Undo/Redo、
プロジェクト再読込について、eventとhandleを記録します。eventが同期か、
重複・欠落するか、削除済みhandleが再利用されるかを確認します。

シーン切替後と別プロジェクト読込後にread sectionを呼び、現在のシーン名を
読み取れることは確認しました。

2026-07-29にWindows Server 2025とAviUtl2 2.1.2でevent診断ログを有効にし、
空のRootで起動とport競合用の再起動を観測しました。各起動で `project_load` の直後に
`change_edit_scene` が発火し、同一起動内では互いに異なるRust thread IDでした。
event callbackではevent種別、時刻、threadだけを記録し、read/edit sectionは
呼びませんでした。

同じbuildでcurrent timelineを読む際に、read section内でlayerごとのobjectを走査して
所有データへコピーできました。空のRootではobject配列は空でした。

続いてFlaUIからtimelineのcontext menuをUI Automation patternで展開し、text objectを
作成しました。作成、削除、Undo、Redo、新規再作成の順に操作した結果は次のとおりです。

- 作成時は `update_object`、`change_focus_object` の順に発火した
- 削除時も `update_object`、`change_focus_object` の順に発火した
- Undoによる復元とRedoによる再削除では `update_object` だけが発火した
- これらのobject eventは同じevent threadから通知された
- Undoで復元されたobjectのraw handleは削除前と同じだった
- Redoで削除した後、同じ位置に新規作成したobjectのraw handleは以前と異なった

これは1つのtext objectを同一process、同一sceneで操作した観測に限られます。
handleの長期寿命、再利用されないこと、eventの一意性や欠落しないことは保証しません。
raw handleは公開identityに使わず、request内でmetadataをコピーするためだけに使います。

## Q6 — Linux から Windows へのビルド

状態：**完了（Phase 0範囲）**

- [x] `cargo xwin` でpluginとCLIをビルドできる
- [x] DLLを `.aux2` へ改名できる
- [x] 追加のruntime DLLを必要としない
- [x] Linux Dockerクロスビルド成果物をWindows + AviUtl2でロードできる
- [x] Windows nativeビルド成果物をWindows + AviUtl2でロードできる

2026-07-27にクロスビルドが完了しました。PEのexport tableには、期待する
汎用プラグインABI（`RequiredVersion`、`InitializePlugin`、`RegisterPlugin`、
`UninitializePlugin` および関連する初期化export）が含まれています。
同日、Linux Dockerクロスビルド版のpluginとCLIについて、Windows 11 +
AviUtl2 2.1.2実機でpluginのロードと登録、`GET /healthz`、CLIによる応答解釈を
確認しました。Windows nativeビルド成果物はGitHub Actions run
`30357153530` と `30359491277` でロードを確認しました。
両ビルド経路の成果物はMSVC CRTを静的リンクしており、PE import検査では
Windowsのsystem DLLだけが検出されています。

2026-07-28 12:33 UTCに、Rust sourceをcommit
`79f983993a8e6da5e6514b067f5bb2875d275756` として記録した作業treeから生成した
正規cross-build成果物のSHA-256は次のとおりです。

```text
bfe50cd1be3d43e6609af09ea5aea9d6089a29e2d5ced243d9e50a7b80ecee57  aviutl2-agent-plugin.aux2
404104c8a24194e0822fb70982c64b48775e3d8aabe7764309d4bea7c1225cbb  aviutl2-agent.exe
```

## Q8 — プラグインのunloadと所有スレッド

状態：**完了（Phase 0範囲）** — 厳密なDLL unload順序は繰延

最初の `tiny_http` スパイクは不採用としました。内部のkeep-alive taskが
server値より長く生存し、unload後もDLL内のコードを実行する可能性があったためです。
現在のPhase 0 serverは全workerを直接所有し、すべての応答で接続を閉じ、
`Drop` 内で全workerをjoinします。Linuxの回帰テストでは、idle状態の
keep-alive clientと、終了後のport再bindを検証しています。workerの途中起動失敗時も、
起動済みworkerを停止・joinしてlistenerを解放することをfailure injectionで確認します。

- [ ] AviUtl2 が `FreeLibrary` より先に `UninitializePlugin` を呼ぶか確認する
- [x] 正常終了時のplugin破棄で全HTTP workerがjoinされることを確認する
- [x] idle状態のclientが正常終了を遅延させないことを確認する
- [x] プロセス再起動後にportが解放され、再bindできることを確認する

### 2026-07-28 GitHub-hosted Windows runner正常終了観測

GitHub Actions run `30359491277` の標準 `windows-2022` runnerで、
AviUtl2 2.1.2を起動し、`health` と `read-section` の成功後に、データを送らない
TCP clientをport 7890へ接続したままAviUtl2のメインウィンドウへ `WM_CLOSE` を
送信しました。

観測環境はWindows Server 2022 Datacenter build 20348、runner image
`20260720.249.2`、AMD EPYC 7763でした。workflowはWindows native buildした
commit `79f983993a8e6da5e6514b067f5bb2875d275756` のpluginを使用しました。
AviUtl2 ZIPはGitHub Actions cacheから復元し、固定済みSHA-256
`9de5d6bd372cd2b671d50ba93645571bb4c260f694b62d306507ec9d17d70b33`
との一致を再確認したため、このrunでは公式配布元へアクセスしていません。

pluginが記録したイベントは次の順序でした。

```text
plugin_drop_started
http_workers_joined (workerCount=4, joinPanics=0)
plugin_drop_completed
```

3イベントは同じ `ThreadId(5)` で記録され、worker joinは破棄完了マーカーより前に
完了しました。`WM_CLOSE` 送信からAviUtl2の終了までは118 ms、exit codeは0でした。
終了後、同じrunner processからport 7890へのbindにも成功しました。

以上により、観測した正常終了経路ではidle clientがあっても全4 workerが停止・join
され、plugin破棄完了後にlistenerが残らないことを確認しました。ただし、
このplugin内の観測だけではAviUtl2による `UninitializePlugin` と `FreeLibrary` の
厳密な呼出順までは証明できないため、その項目は未検証のまま残します。

## Q7 — Undo API の公開

状態：**調査中（Phase 2開始前）**

- [x] SDKのUndo/Redo APIを探す
- [ ] 人間の操作をUndoする可能性があるか確認する
- [x] UI操作による作成・削除・Undo・Redoのeventを記録する
- [ ] Undo stackの位置や深さを照会できるか確認する

`aviutl2` 0.41.0のgeneric APIと同versionのPlugin SDK定義には、Undo/Redoを直接実行する
API、stack位置、stack深さを取得するAPIは見つかりませんでした。Windows実測では
`Ctrl+Z` で削除をUndoし、`Ctrl+Shift+Z` でRedoできましたが、これはAviUtl2全体の
UI操作履歴を対象にするため、agentが直前に行った変更だけを安全に戻す根拠には
なりません。

直前のAPI操作だけが対象になると証明できない限り、Undoを公開しません。

## 結果

Phase 0 完了判定：**完了（2026-07-28）**

Phase 0はSDKの全項目を完了させる工程ではなく、安全な最小read-only実装へ進めるだけの
事実を採取する技術スパイクとして完了します。起動、Windows load、HTTP workerからの
read section、状態変更後のread、正常終了時のworker joinを確認できたためです。

Q1の並列・終了競合、Q3、Q4、Q5、`FreeLibrary` の順序は未検証事項として残します。
これらを保証せず、最初のPhase 1をSDK非依存のstatusと現在sceneのreadだけに限定します。
timeline、object identity、event、renderなどへAPIを広げる場合は、対応する未検証項目を
その直前に調査します。
長時間SDK呼出し中の `/healthz` 応答はPhase 0の未完了項目にはせず、SDK非依存経路を
維持するPhase 1の回帰testとして [`design.md`](design.md) に移しました。

Phase 1の開始範囲とアーキテクチャ制約は [`design.md`](design.md) v0.5へ反映済みです。

### Phase 2 write API の開始条件

- Q1のwrite呼び出し経路
- Q2のUndo単位と部分失敗
- Q4のwrite/busy関連
- Q5のwrite eventとrevision
- Q7のUndo API公開可否

Q2とQ7を含むwrite固有項目は、Phase 2の開始前までに確定します。

## Q9 — scene identityと列挙

状態：**調査中（Phase 1拡張前）**

`aviutl2` 0.41.0の
`generic::EditInfo` はcurrent edit sectionの `scene_id: i32` を公開し、
`EditSection::get_scene_name` でcurrent scene名を取得できます。一方、同versionの
generic APIと`aviutl2-sys` Plugin SDK定義にはscene一覧を列挙する関数がありません。

この静的調査だけではscene IDの寿命や再利用を保証できないため、次をWindowsで
追加観測します。

- [x] Rootと追加sceneのIDを記録する
- [x] sceneを往復してIDの一致を記録する
- [ ] 同名sceneを作成してIDを記録する
- [ ] project再読込と別project読込後のIDを記録する
- [ ] scene削除後にIDが再利用されるか記録する

scene一覧APIは、安全な列挙方法が確認できるまで公開しません。

2026-07-29にWindows Server 2025、AviUtl2 2.1.2、branch上のWindows native
release buildを使い、対話sessionで追加観測しました。pluginが任意の観測ログへ
current scene名とSDKのraw `scene_id` を記録する状態で、次の順に操作しました。

1. 初期状態のRootをCLIで読む
2. scene listからScene1を作成して選択し、CLIで読む
3. Rootを再選択し、CLIで読む

観測値は `Root=0`、`Scene1=1`、再選択した `Root=0` でした。同一process内の
往復ではRootのIDが一致しました。scene listはUI AutomationのControl viewとRaw viewの
どちらにも子要素として公開されず、scene作成dialogの確認buttonだけはFlaUIから
Automation IDを使って操作できました。scene行の選択はこの観測環境の座標に依存する
実験用操作であり、再利用可能なruntime smokeには含めません。

この結果だけではprocessやprojectをまたぐIDの寿命、削除後の再利用、同名sceneの
識別を保証できません。従ってraw `scene_id` は公開responseへ追加せず、残りの観測が
終わるまで内部の診断値に留めます。
