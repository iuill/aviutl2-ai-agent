# Phase 0：SDK の事実採取

この文書は API 仕様ではなく、検証結果の台帳です。各項目について、AviUtl2
のバージョン、`aviutl2` crate のバージョン、ビルド元、正確な再現手順、
観測結果、ログを記録します。調査は、その事実が必要になるPhaseの直前までに
完了させます。write固有の調査によってread-only実装を止めません。

## 検証環境

| 項目 | 値 |
|---|---|
| AviUtl2 | 未検証 |
| `aviutl2` crate | 0.41.0 |
| Rust | 1.88.0 |
| クロスビルドイメージのdigest | 未検証 |
| Windows バージョン | 未検証 |

## 起動確認

- [x] Linux Docker から `.aux2` と `.exe` を生成できる
- [ ] `.aux2` をロードし、プラグインを登録できる
- [ ] `GET /healthz` が応答する
- [ ] CLI が health 応答を解釈できる
- [ ] プラグインのunload時に全HTTP workerが停止し、joinされる
- [ ] AviUtl2 がプロセス実行中にもDLLをunloadするのか、プロセス終了時だけかを確認する
- [ ] 長時間のSDK操作中も `/healthz` がブロックされない

## Q1 — section のスレッド親和性と再入性

状態：**未検証**

- [ ] HTTP worker から read section を呼ぶ
- [ ] HTTP worker から edit section を呼ぶ
- [ ] event callback のthread IDと同期／非同期性を記録する
- [x] event callback 内ではevent情報をqueueまたはatomic stateへ記録するだけとする
- [x] event callback から `call_edit_section` を呼ばない
- [ ] edit section 内で現在状態を読み取る
- [ ] edit section を入れ子または連続で呼ぶ
- [ ] section 実行中に終了する

呼び出し可能なスレッドと、必要なdispatcher設計を記録します。
`aviutl2-rs` 0.41.0はevent用スレッドからの `call_edit_section` を禁止しているため、
この経路は危険な実験を行わず、上流仕様により禁止と確定します。
製品ではすべてのSDK呼び出しをEditorGateで直列化するため、生のread/read、
read/write、write/write並列呼び出しは必須完了条件にしません。必要になった場合だけ、
使い捨てプロジェクトを用いた追加調査として実施します。

> 未検証

## Q2 — Undo と部分失敗

状態：**未検証**

- [ ] 1つのedit sectionで2オブジェクトを変更し、Undoを1回実行する
- [ ] 2件目のmutationを意図的に失敗させる
- [ ] オブジェクト作成後、設定更新を失敗させる
- [ ] 複合操作内で削除し、Undoする
- [ ] 明示的なrollback APIの有無を調べる

Undoの粒度と、部分的な変更が残るかを記録します。

> 未検証

## Q3 — フレームレンダリング

状態：**未検証**

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

状態：**未検証**

タイムラインのドラッグ中、modal dialog表示中、再生中、出力中、
プロジェクトの読込・保存中、Undo/Redo中、終了中にread/write/renderを試します。
SDKを呼ぶ前に、その状態を判定できるか記録します。

> 未検証

## Q5 — event、revision、handle

状態：**未検証**

作成、更新、移動、削除、effect変更、scene切替、API由来の変更、Undo/Redo、
プロジェクト再読込について、eventとhandleを記録します。eventが同期か、
重複・欠落するか、削除済みhandleが再利用されるかを確認します。

> 未検証

## Q6 — Linux から Windows へのビルド

状態：**検証中**

- [x] `cargo xwin` でpluginとCLIをビルドできる
- [x] DLLを `.aux2` へ改名できる
- [x] 追加のruntime DLLを必要としない
- [ ] クロスビルド成果物とWindows native成果物の両方をロードできる

2026-07-27にクロスビルドが完了しました。PEのexport tableには、期待する
汎用プラグインABI（`RequiredVersion`、`InitializePlugin`、`RegisterPlugin`、
`UninitializePlugin` および関連する初期化export）が含まれています。
Windowsでのロードは未検証であり、この結果からロード成功を推測してはいけません。
両成果物はMSVC CRTを静的リンクしており、PE import検査ではWindowsの
system DLLだけが検出されています。

## Q8 — プラグインのunloadと所有スレッド

状態：**検証中**

最初の `tiny_http` スパイクは不採用としました。内部のkeep-alive taskが
server値より長く生存し、unload後もDLL内のコードを実行する可能性があったためです。
現在のPhase 0 serverは全workerを直接所有し、すべての応答で接続を閉じ、
`Drop` 内で全workerをjoinします。Linuxの回帰テストでは、idle状態の
keep-alive clientと、終了後のport再bindを検証しています。workerの途中起動失敗時も、
起動済みworkerを停止・joinしてlistenerを解放することをfailure injectionで確認します。

- [ ] AviUtl2 が `FreeLibrary` より先に `UninitializePlugin` を呼ぶか確認する
- [ ] `UninitializePlugin` 後にplugin threadが残らないことを確認する
- [ ] idle状態のclientがunloadを遅延させないことを確認する
- [ ] 再load/unloadまたはプロセス再起動後にportが解放されることを確認する

> Windows上の実行時挙動は未検証

## Q7 — Undo API の公開

状態：**未検証**

- [ ] SDKのUndo/Redo APIを探す
- [ ] 人間の操作をUndoする可能性があるか確認する
- [ ] eventとrevisionの挙動を記録する
- [ ] Undo stackの位置や深さを照会できるか確認する

直前のAPI操作だけが対象になると証明できない限り、Undoを公開しません。

> 未検証

## 結果

Phase 0 完了判定：**未完了**

### Phase 1 read-only API の開始条件

- 起動確認、unload、Q8のworker lifecycle
- Q1の安全なread呼び出し経路
- Q3（Phase 1でframe renderを含める場合）
- Q4のread/render関連
- Q5のproject reload、handle、object identity関連
- Q6のクロスビルドとWindowsロード

### Phase 2 write API の開始条件

- Q1のwrite呼び出し経路
- Q2のUndo単位と部分失敗
- Q4のwrite/busy関連
- Q5のwrite eventとrevision
- Q7のUndo API公開可否

Phase 1に必要な結果を記録した後、該当する未検証分岐を観測事実へ置き換え、
read APIの実装前に、より短いv0.5を作成します。Q2とQ7はPhase 2の開始前までに
確定します。
