# ロードマップ

この文書はPhase 1以降の実施順序を管理します。公開APIの契約と安全境界は
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
stdio transportは公式Rust SDKを使い、MCP `2026-07-28`とlegacy lifecycleを
両方サポートします。

完了条件:

- Codexからread-only toolを呼び出せる
- object一覧の情報量とページング方針を実利用で評価できる
- 画像を扱う場合はbase64サイズと既定縮小幅を実測できる
- MCP SDK更新後のWindows x64 binaryをnative実行し、stdio tool呼出しを確認できる

最後の項目は正規cross-buildとは別の合格条件です。`rmcp` 3.0.1への移行後、Linuxの
stdio integration testに加え、Windows native CIでrelease binaryを起動し、modernと
legacyの両lifecycleを確認しています。

## Phase 2: 既存objectの最小write

状態: **move APIとCLIを実装、APIはWindows実機検証済み**

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

## Phase 3: 単一objectの生成・複製・削除

状態: **実装・Windows実機検証済み**

Phase 2の安全境界を維持したまま、1要求1operationで次を提供します。

- 単一objectのdeleteとduplicate
- 検証済みの最小aliasを使うplain text preset
- caller管理pathからのimage / audio生成
- 各mutationの同一edit section内でのread-back
- title / subtitleを組み立てるエージェント向けの再利用可能な例

text APIは、Windowsでeffect名と本文項目名を実測した最小構成だけをpresetとして
公開します。SDK固有のeffect名、項目schema、内部aliasはHTTP契約へ出しません。
title / subtitle固有の装飾は、対応するaliasをWindowsで実測してからpresetへ追加します。
未検証のstyleを保証するpresetは追加しません。

title / subtitleの例は複数の単一操作を順に呼び、各responseまたはobject一覧を再取得して
状態を確認します。途中まで適用された場合に自動rollbackできるとは保証しません。

## Phase 4以降: 汎用effectと複数operation

単一操作で実利用上の支障が確認された時点で、次を再評価します。

- effectの追加 / 更新 / 削除とversion付きschema
- client側の一時IDと作成結果の対応
- 1 edit section内の複数operation
- batch途中失敗時の結果表現と復旧手順
- 複数操作をまとめるUndo単位

batchを導入する場合はoperation間で `EditorGate` を解放しません。原子性やrollbackを
SDKの観測なしに保証せず、部分失敗が残る場合は契約上明示します。tool call回数、
処理時間、操作間の競合、Undo単位のいずれかが単一操作では問題になることを、導入判断の
根拠とします。

## 全Phaseで維持する境界

- SDK pointer、handle、enumをHTTP DTOへ出さない
- SDK呼出しを `EditorGate` 外から行わない
- `/healthz` と `/v1/status` をSDK呼出しに依存させない
- event callbackから `call_edit_section` を呼ばない
- plugin unload前に全workerを停止・joinする
- Windows未実測の挙動を保証済みと書かない
- APIはprojectを保存しない
- APIは利用者の操作を無条件にUndoしない
