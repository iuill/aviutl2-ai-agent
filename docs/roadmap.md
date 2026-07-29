# ロードマップ

この文書はPhase 1からPhase 3までの実施順序を管理します。公開APIの契約と安全境界は
[`design.md`](design.md)、SDKの観測事実と未検証項目は
[`phase0.md`](phase0.md)を正とします。

先のPhaseまで設計と調査を進めることは妨げません。ただし、未検証のSDK挙動に依存する
APIを先に公開したり、write APIをread-only APIへ紛れ込ませたりはしません。

## Phase 1: Read-only API

### 1A: 最小slice

状態: **実装・Windows自動runtime smoke完了**

- `GET /v1/status`
- `GET /v1/scenes/current`
- CLI `status` / `current-scene`
- `EditorGate`
- 共通エラー契約
- Host / Origin制約

残作業:

- Windows実機で、port競合理由をプラグイン情報画面から認識できることを手動確認する

API無効状態からのruntime中の再bindは行わず、port解放後のAviUtl2再起動で復旧します。
runtime retryは複数instance対応と必要性を一緒に再設計します。

### 1B: current scene identity

状態: **調査中**

`aviutl2` 0.41.0のgeneric APIと`aviutl2-sys`のPlugin SDK定義を調べた結果、
current edit sectionには `scene_id: i32` とscene名がありますが、scene一覧を
列挙するAPIはありませんでした。このため、scene一覧APIは現時点の次対象から外します。

公開契約を決める前にWindows + AviUtl2で次を観測します。

- [x] Rootと追加sceneで `scene_id` がどう変わるか
- [x] scene切替後に元のsceneへ戻ると同じIDになるか
- [ ] 同名sceneのIDが異なるか
- [ ] project再読込と別project読込でIDが再利用されるか
- [ ] scene削除後にIDが再利用されるか

観測後、`GET /v1/scenes/current` に安全に公開できるidentityとmetadataを加算します。
IDの寿命を実測できなければ公開せず、内部の観測値に留めます。scene一覧はSDKに
列挙手段が追加されるか、別の安全な取得方法を実測できるまで保留します。
2026-07-29の最初の観測では、同一process内で `Root=0`、`Scene1=1`、
再選択した `Root=0` でした。残りの寿命・再利用条件が未確認のため、IDはまだ
公開契約に含めません。

### 1C: current sceneのtimeline / object read

状態: **timeline概要とobject一覧を実装**

current scene identityの扱いを決めた後、利用価値を確認しながら次を1種類ずつ
追加します。

1. projectの観測可能なmetadata
2. [完了] current sceneのtimeline概要
3. [一覧実装済み] object一覧と個別取得
4. effectの列挙と取得できるmetadata

object identity、project再読込時の無効化、eventとの関係は
`phase0.md` Q5を追加調査してから契約化します。frame readはQ3を完了してから追加します。
current以外のsceneを明示するAPIは、sceneを安全に選択・列挙できる根拠が得られるまで
公開しません。

### 1.5: Read-only MCP

状態: **scene / timeline / object一覧toolを実装**

HTTP/CLIのread契約を実利用で評価できる段階で、AviUtl2 process外のstdio serverとして
追加します。最初はproject/scene、object、必要ならframeの最大3 toolに絞ります。
MCPはpluginのvalidationを迂回せず、write toolを含めません。

完了条件:

- Codexからread-only toolを呼び出せる
- object一覧の情報量とページング方針を実利用で評価できる
- 画像を扱う場合はbase64サイズと既定縮小幅を実測できる

## Phase 2: 既存objectの最小write

実装開始前に `phase0.md` の次をWindowsで完了し、結果を設計へ反映します。

- Q1: HTTP workerからのedit section、edit内read、連続・入れ子呼出し
- Q2: Undo単位、途中失敗、rollback API
- Q4: write中のbusy状態
- Q5: write event、revision、handleの無効化と再利用
- Q7: Undo/Redo APIを公開できる条件

最初の公開範囲は既存objectへの1要求1operationに限定します。

- `inspect → validate → apply → verify`
- project epoch、scene、revision、対象を明示する
- validationからmutationまで `EditorGate` を解放しない
- move、layer変更、時間範囲変更など、既存objectの限定的な更新
- apply後に対象を再取得して結果を返す
- project保存、無条件のUndo/Redoは公開しない

Phase 2の契約は調査結果を反映した新しい設計版で確定します。v0.4の案をそのまま仕様とは
みなしません。

## Phase 3: object生成と複数operation

Phase 2の既存object更新とUndo/部分失敗の観測結果を前提に、次を段階的に追加します。

- create / duplicate / delete
- text / image / audioの代表的な生成
- effectの追加 / 更新 / 削除
- client側の一時IDと作成結果の対応
- 1 edit section内の複数operation
- 途中失敗時の復旧手順
- title / subtitleの再利用可能な例

複数operationはoperation間で `EditorGate` を解放しません。原子性やrollbackをSDKの
観測なしに保証せず、部分失敗が残る場合は契約上明示します。

## 全Phaseで維持する境界

- SDK pointer、handle、enumをHTTP DTOへ出さない
- SDK呼出しを `EditorGate` 外から行わない
- `/healthz` と `/v1/status` をSDK呼出しに依存させない
- event callbackから `call_edit_section` を呼ばない
- plugin unload前に全workerを停止・joinする
- Windows未実測の挙動を保証済みと書かない
- APIはprojectを保存しない
- APIは利用者の操作を無条件にUndoしない
