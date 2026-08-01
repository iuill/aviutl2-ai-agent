# aviutl2-agent 設計案

> [!NOTE]
> このファイルは、Phase 0 着手前に作成された 2026-07-27 付 Draft v0.4 の
> 履歴資料です。現行仕様ではありません。現在の設計判断は
> [`../design.md`](../design.md)、実測結果とPhase移行条件は
> [`phase0.md`](phase0.md)を参照してください。

- ステータス: Draft v0.4
- 想定ライセンス: MIT
- 対象: AviUtl2（Windows ネイティブ）
- 基本構成: Rust 製 AviUtl2 プラグイン + localhost HTTP API + Rust 製 CLI / MCP サーバー
- ビルド方針: Linux ベースの Docker を正規ビルド環境とし、Windows + AviUtl2 を実機検証環境とする

> v0.3 からの主な変更点は付録 A を参照。

## 0. この文書の読み方

本書は Phase 0 の実機検証前に書かれている。SDK の性質が判明した時点で、内容の一部は誤りになる。

そこで各章に安定度マーカーを付ける。

| マーカー | 意味 | Phase 0 後の扱い |
|---|---|---|
| 【確定】 | SDK の性質に依存しない設計判断 | 原則変更しない |
| 【暫定】 | Phase 0 の結果で内容が変わりうる | 検証結果を反映して書き直す |
| 【未定】 | Phase 0 の答えを待っている箇所 | 答えを埋める |

| 章 | 安定度 |
|---|---|
| 1〜3 概要・前提・目的 | 確定 |
| 4 未解決の技術前提 | 未定（Phase 0 の作業指示書） |
| 5 非目標 | 確定 |
| 6 設計原則 | 確定（6.7 のみ暫定） |
| 7〜9 アーキテクチャ・コンポーネント・構成 | 確定（HTTP ランタイム詳細は暫定） |
| 10〜13 API 契約・HTTP API・編集モデル・identity | 暫定 |
| 14 CLI | 確定 |
| 15〜16 MCP・Agent Skill | 暫定 |
| 17〜19 セキュリティ・ログ・ビルド | 確定 |
| 20〜25 テスト・依存更新・ロードマップ・リスク | 確定 |
| 付録 B invariant | 確定 |

**Phase 0 に着手する前に、これ以上設計を進めない。**10〜16 章は SDK の事実を採取してから書き直す前提の暫定仕様であり、いま精緻化しても検証結果で無効になる。v0.5 は本書より短くなるべきである。

## 1. 概要 【確定】

本プロジェクトは、AviUtl2 の起動中プロジェクトを外部プログラムから安全かつ構造化された方法で参照・編集できるようにし、最終的には Codex、Claude Code などの AI エージェントによる動画編集自動化を実現する。

AviUtl2 のプロジェクトファイルである `.aup2` を直接編集する方式は、オフライン一括処理には有用だが、ライブ編集では次の問題を持つ。

- AviUtl2 がメモリ上に保持している未保存状態と競合する
- 起動中のタイムラインへ即時反映できない
- AviUtl2 の Undo、選択状態、カーソル位置、レイヤーロックなどを利用しにくい
- AviUtl2 本体によるレンダリング結果を確認しながら編集しにくい
- プロジェクト形式の変更が、そのまま外部ツールへ波及する
- AviUtl2 が保存した内容によって外部変更が上書きされる可能性がある

このため、ライブ編集の正規経路は AviUtl2 Plugin SDK とし、`aviutl2-rs` を用いた Rust 製汎用プラグインから、起動中の編集状態を操作する。

外部連携は localhost HTTP API とする。AI エージェントが HTTP API を直接組み立てることを必須にせず、`aviutl2-agent` CLI を標準クライアントとして提供する。同じバイナリに MCP サーバーモードを持たせ、Codex、Claude Code その他の MCP 対応エージェントへ編集ツールを公開する。

```text
Codex / Claude Code / その他の AI エージェント
            │
            ├─ Agent Skill（編集手順・安全規則）
            │
            └─ CLI または MCP
                    │
                    ▼
          aviutl2-agent.exe（Rust）
          ├─ CLI
          ├─ MCP サーバー（stdio）
          ├─ HTTP クライアント
          └─ protocol crate を直接利用
                    │
                    │ localhost HTTP + JSON
                    ▼
       aviutl2-agent-plugin.aux2（Rust）
       ├─ tiny_http による HTTP サーバー
       ├─ 認証・入力検証
       ├─ EditorGate による SDK 呼び出し直列化
       ├─ 編集コマンド実行
       ├─ 状態取得・フレーム取得
       └─ aviutl2-rs
                    │
                    ▼
             AviUtl2 Plugin SDK
                    │
                    ▼
          起動中の AviUtl2 プロジェクト
```

## 2. 前提 【確定】

### 2.1 技術前提

- AviUtl2 本体は Windows ネイティブで動作させる。
- GPU 描画、プラグイン互換性、ハードウェアエンコードの安定性を優先し、Wine 上での本体運用は対象外とする。
- プラグインは `aviutl2-rs` を利用した Rust の `cdylib` とし、成果物を `.aux2` として配布する。
- 外部 API は `127.0.0.1` のみに bind する HTTP/JSON API とする。
- CLI と MCP サーバーも Rust で実装し、`protocol` crate を共有する。プラグインとは HTTP を介して疎結合にする。
- 正規ビルドは Linux ベースの Docker で行い、`x86_64-pc-windows-msvc` 向け成果物を生成する。
- 実機統合テストには Windows + AviUtl2 が必要になる。ビルド環境と実行検証環境は分離する。
- Linux cross build が一時的に成立しない場合のみ、`windows-latest` を暫定フォールバックとして使用する。
- リポジトリは Cargo workspace のモノレポとする。

### 2.2 開発方針

- AviUtl2 SDK の型やハンドルを HTTP 契約へ漏らさない。
- `aviutl2-rs` の更新影響は Rust プラグイン内部に閉じ込める。
- タイトル、字幕、BGM などの個別ユースケースを中核 API にしない。
- 中核 API は、シーン、オブジェクト、エフェクト、時間範囲を扱う汎用編集モデルとする。
- AI の推測による直接変更を避け、`inspect → validate → apply → verify` を基本フローとする。
- 初期段階では、機能網羅性よりクラッシュしないこと、Undo 可能性、再現性、観測可能性を優先する。
- API の消費者は最終的に LLM である。出力の冗長さ、エラーメッセージの自己説明性、探索のしやすさを一級制約として扱う。
- 未検証の SDK 特性を前提にアーキテクチャを確定しない。Phase 0 の結果を設計へ反映してから write API を公開する。
- 個人開発として過剰な生成パイプライン、抽象化、テスト基盤を先に作らない。
- **設計文書の精緻化より、SDK の事実採取を優先する。**

### 2.3 プロジェクトの保存責任

**プラグインはプロジェクトを保存しない。**API による編集はすべて AviUtl2 のメモリ上の未保存状態に対して行われ、`.aup2` への書き出しは利用者が AviUtl2 の UI で行う。

理由:

- 保存はファイルを上書きする不可逆操作であり、AI エージェントに委ねる既定動作として適切でない
- 利用者が Undo で戻せる範囲に変更をとどめるほうが安全である
- AviUtl2 の保存処理中に API が介入する状況を作らずに済む

帰結として、次を守る。

- Agent Skill は、編集完了時に利用者へ保存を促す
- CLI は編集系コマンドの後に「未保存である」旨を stderr へ表示する
- 将来 `POST /v1/project/save` を追加する場合は、明示的な利用者承認を必須とする capability として扱う

## 3. 目的 【確定】

### 3.1 直接的な目的

1. 起動中の AviUtl2 から、プロジェクト、シーン、タイムライン、選択状態、オブジェクト、エフェクトを取得できるようにする。
2. 外部から、オブジェクトの作成、更新、移動、複製、削除を実行できるようにする。
3. 編集要求を適用前に検証し、対象解決結果と警告を得られるようにする。
4. SDK が安全に許容する範囲で、複数操作を一つの編集要求として適用できるようにする。
5. 編集結果を AviUtl2 の描画結果として取得し、視覚的に確認できるようにする。
6. CLI から JSON ベースで安定して操作できるようにする。
7. 同じ機能を MCP Tools として AI エージェントへ公開する。

### 3.2 最終的に実現したいこと

利用者が AI エージェントへ、例えば次のように依頼できる状態を目指す。

```text
「冒頭5秒にタイトルを配置して、背景を暗くし、BGMをフェードインさせて」
「この動画から発話区間ごとに字幕を配置して、画面下部で読みやすく整えて」
「この3枚の画像を同じ大きさにし、横一列に等間隔で並べて」
「10秒から20秒の区間で、重要な発言に対応するテロップを追加して」
「編集後の代表フレームを確認し、文字が画面外にはみ出していたら修正して」
```

AI エージェントは、次の手順を自律的に反復する。

1. 現在のセッション、プロジェクト、シーン、revision を取得する。
2. 利用可能なオブジェクト・エフェクト・設定項目を確認する。
3. 明示的な `sceneId` と対象を指定した編集 JSON を作成する。
4. `validate` で静的検証、意味検証、対象解決を行う。
5. 警告や変更内容を確認し、必要に応じて利用者の承認を得る。
6. 同じ編集 JSON を `apply` する。
7. オブジェクト状態と縮小フレーム画像を再取得する。
8. 期待結果との差を評価し、必要なら再調整する。
9. 一連の編集が完了したら、利用者へ保存を促す。

## 4. 未解決の技術前提 【未定】

この設計は次の7点の答えに依存する。いずれも Phase 0 で実機検証し、結果次第で該当章を書き換える。現時点の記述は、公開 API の確定仕様ではなく検証対象を明示するための暫定設計である。

### Q1. `call_*_section` のスレッド親和性・再入性・読み書き混在

`EditHandle::call_read_section` / `call_edit_section` を、プラグインが自前で起動したスレッドから安全に呼べるか。AviUtl2 の UI スレッドへのマーシャリングが必要か。同時呼び出し、再入、イベントコールバックとの競合が許容されるか。

検証項目:

- HTTP サーバースレッドから `call_read_section` を呼ぶ
- HTTP サーバースレッドから `call_edit_section` を呼ぶ
- 複数スレッドから同時に read / write を呼ぶ
- イベントコールバック中の呼び出し可否
- AviUtl2 終了処理中の呼び出し
- 長時間の SDK 呼び出し中に別要求が来た場合の挙動
- **`call_edit_section` の内部で現在状態を読み取れるか**（6.7 の前提。読めなければ「検証と mutation を同一セクションで行う」設計は成立しない）
- **`call_edit_section` を入れ子・連続で呼べるか**（1つの論理操作を複数セクションに分けざるを得ない場合の挙動）

設計上の分岐:

- **任意スレッド可**: HTTP スレッドから呼べる。ただし SDK の同時実行には依存せず、`EditorGate` で全 SDK 呼び出しを直列化する。
- **UI スレッド限定**: ウィンドウメッセージ、タイマー、AviUtl2 が提供するコールバック等で UI スレッドへマーシャリングする。
- **read のみ任意スレッド可**: read と write の実行経路を分離する。
- **edit section 内で読めない**: 6.7 を「gate を保持したまま read section → edit section」へ後退させる。TOCTOU は gate で防げるが、同一セクションでの原子性は失う。

`aviutl2-rs` の型が `Send` / `Sync` であることは、AviUtl2 本体が任意スレッドからの同時呼び出しを保証することと同義ではない。

### Q2. Undo 単位とバッチの原子性

`call_edit_section` 1回が Undo 1単位になるか。1回のセクション内で複数オブジェクトを操作したとき、途中で失敗したら部分適用が残るか。

検証項目:

- 2オブジェクトを1回の `call_edit_section` で変更し、Undo 1回で両方戻るか
- 2件目で意図的に失敗させたとき、1件目が残るか
- 作成後に設定値変更で失敗した場合、作成済みオブジェクトが残るか
- 削除を含む複合操作を Undo できるか
- SDK が明示的な rollback API を提供するか

この答えが write API の契約を決める。

- **全体ロールバック可能**: `apply` を原子的なバッチとして公開できる。
- **Undo 1単位だが途中状態は残る**: 原子性は約束せず、事前検証の強化と、失敗時の復旧手順（Undo または差分修復）を Skill に定義する。
- **部分適用が安全に復旧できない**: operation 数の上限を低く保ち、失敗時は利用者へエスカレーションする。

初期契約は `maxOperationsPerEdit = 1` とする。理由は 10.5 を参照。**「1 operation のほうが安全だから」ではなく「Phase 2 の実装量を減らすため」である。**

### Q3. フレームレンダリングの実行条件とバッファ寿命

`aviutl2-rs` にはシーンの映像をレンダリングする API が存在するが、AI 連携用途で安全に利用できる条件は未検証である。

検証項目:

- 汎用プラグインから対象 `sceneId` / frame をレンダリングできるか
- どのスレッドから呼べるか
- コールバックがどのスレッドで実行されるか
- 再生中、出力中、モーダル表示中に呼べるか
- 同じフレームを連続取得した場合の安定性
- 取得バッファの pixel format、pitch、alpha の扱い
- バッファの有効期間
- 大きな解像度での処理時間とメモリ使用量
- **レンダリング解像度を指定できるか**（できない場合、取得後に縮小する）
- タイムアウトまたはキャンセル方法

レンダリングバッファはコールバック中だけ有効である前提とし、コールバック内では所有する `Vec<u8>` へ即時コピーする。縮小と PNG エンコードはコールバック終了後、かつ `EditorGate` を解放した後に行い、AviUtl2 側のレンダリング処理を長時間拘束しない。

```text
AviUtl2 rendering callback
        │
        ├─ メタデータ取得
        └─ pixel buffer を Vec<u8> へコピー
                │
                ▼ callback 終了 / EditorGate 解放
           縮小 → PNG エンコード
                │
                ▼
           HTTP / MCP 応答
```

取得できない場合の代替案:

- AviUtl2 の出力機能を経由して一時ファイルへ静止画を書き出す
- プレビューウィンドウのキャプチャ
- フォントメトリクス等による幾何的検証へ縮退する

視覚的な反復編集は本プロジェクトの主要価値であるため、Phase 0 で優先して成立性を確認する。

### Q4. ユーザー操作中・特殊状態での編集要求

AviUtl2 側で次の状態にあるとき、read / write / render が安全に実行できるか。

- タイムライン上のドラッグ操作中
- 設定ダイアログやモーダルダイアログ表示中
- プレビュー再生中
- 動画出力中
- プロジェクト読込・保存中
- Undo / Redo 実行中
- AviUtl2 終了処理中

安全に判定できる場合は `/v1/status` と `/v1/capabilities` に現在の可用性を反映し、write を受け付けられないときは `503 EDITOR_BUSY` を返す。

判定手段が存在しない場合は、書き込みを直列化したうえで、危険な状態での write を明示的に非対応とし、既知の再現条件を `docs/verification/windows.md` に記録する。

### Q5. イベント通知・revision・ObjectHandle の安定性

楽観的排他制御と opaque object ID を成立させるには、AviUtl2 のイベント通知とハンドルの性質を把握する必要がある。

検証項目:

- オブジェクト作成、更新、移動、削除で通知が来るか
- エフェクト変更で通知が来るか
- API 経由の変更でも同じ通知が来るか
- 通知は同期か非同期か
- 1つの論理変更で通知が複数回来るか
- シーン切り替えとシーン内容変更を区別できるか
- `ObjectHandle` は移動・更新後も同一か
- 削除後に内部ハンドルが再利用されるか
- Undo / Redo 後にハンドルが維持されるか
- プロジェクトロード時に既存ハンドルを確実に無効化できるか

revision の契約上、次を満たす必要がある。

- 人間または API によるすべての内容変更で `contentRevision` が変化する
- 選択・カーソル・シーン表示切り替えだけでは `contentRevision` は変化しない
- API apply 自身が発生させたイベントによって、apply 応答後に同じ変更分の revision が再度進まない
- クライアントは revision が必ず `+1` されるとは仮定せず、等値比較のみに使う

Object ID の契約上、次を満たす必要がある。

- プロジェクトロード時に ID マップを全破棄する
- 削除した API object ID は再利用しない
- ハンドル再利用がありうる場合、世代または fingerprint で誤参照を防ぐ
- ID 解決時に対象が現在も存在することを SDK で再確認する

### Q6. Linux Docker からの Windows MSVC ビルド

正規ビルド要件として、Linux ベースの Docker から Windows 向けプラグインと CLI を生成できることを確認する。

検証項目:

- `cargo-xwin` 等で `x86_64-pc-windows-msvc` をビルドできるか
- `aviutl2-rs`、HTTP、PNG、MCP 依存が cross build できるか
- 生成 DLL を `.aux2` に変更し AviUtl2 がロードできるか
- Windows ネイティブビルドとの差異がないか
- ランタイム DLL の追加配布が不要か
- release profile、LTO、panic 設定が正しく反映されるか

正規経路:

```text
Linux Docker build
        │
        ▼
Windows向け .aux2 / .exe
        │
        ▼
Windows + AviUtl2 で実機検証
```

cross build が一時的に壊れた場合は Windows runner を暫定フォールバックにできるが、Linux Docker を正規要件から外さない。

### Q7. Undo / Redo の API 露出

Agent Skill の復旧手順は Undo に依存する。しかし API から Undo を呼べるかは未検証であり、呼べたとしても安全とは限らない。

検証項目:

- `aviutl2-rs` が Undo / Redo を実行する API を提供するか
- API 経由の Undo が、API 自身の変更だけを戻すのか、人間の直前操作も戻しうるのか
- Undo 実行後にイベントと revision がどう変化するか
- Undo スタックの深さや現在位置を照会できるか

設計上の分岐:

- **API から Undo を呼べない、または人間の操作を巻き込む**: `POST /v1/edits/undo-last` を提供しない。復旧手順は「エージェントが利用者へ Ctrl+Z を依頼する」または「差分を計算して修復 apply する」になる。これを Skill に明記する。
- **直前の自分の apply だけを安全に戻せる**: `POST /v1/edits/undo-last` を capability として提供する。`projectEpoch` と直前 apply の `contentRevisionAfter` を必須引数とし、**現在の revision がそれと一致する場合にのみ実行する**（一致しなければ人間が編集しているため拒否する）。

**エージェントが無条件に Undo を呼べる API は提供しない。**利用者の作業を巻き戻す事故のほうが、部分適用の残骸より重大である。

## 5. 非目標 【確定】

初期バージョンでは次を対象外とする。

- AviUtl2 本体の代替となる動画編集エンジンの実装
- AviUtl 1.x との互換性
- Wine 上での AviUtl2 本体運用
- インターネット越しに公開するリモート編集 API
- AI モデルや API クライアントのプラグイン内組み込み
- プラグイン内での MCP セッション管理
- `.aup2` の直接書き換えをライブ編集の正規経路にすること
- **API からのプロジェクト保存**（2.3 参照）
- **エージェントが無条件に呼べる Undo / Redo**（Q7 参照）
- すべての AviUtl2 エフェクトを独自の抽象モデルへ完全変換すること
- 動画全体の自動レンダリング、ジョブキュー、分散処理
- 完全自動・無承認の編集
- API が固まる前の OpenAPI 配布、外部言語 SDK 自動生成
- UI 状態に依存する write selector（`selected`、`currentFrame` 等）

`.aup2` の読み書きは、将来的にオフライン解析・テンプレート生成・大量字幕投入などの補助経路として追加できるが、ライブ編集 API とは分離する。

## 6. 設計原則 【確定】

### 6.1 安定境界は HTTP 契約

`aviutl2-rs` と AviUtl2 SDK は変更頻度が高い。外部クライアントが SDK 型へ依存すると、SDK 更新のたびに CLI や MCP 層まで変更が波及する。

したがって、Rust プラグイン内部で SDK 型を安定した DTO へ変換し、HTTP API のバージョンを独立して管理する。

```text
AviUtl2 SDK / aviutl2-rs の変更
            │
            ▼
plugin の adapter 層だけ修正
            │
            ▼
HTTP API v1 と protocol crate は可能な限り維持
            │
            ▼
CLI / MCP / Agent Skill は原則変更不要
```

CLI が Rust であっても、この境界は維持する。CLI は `protocol` crate に依存してよいが、`plugin` crate や `aviutl2-rs` には依存しない。

### 6.2 `inspect → validate → apply → verify`

- 読み取り: `GET` または read-only query
- 検証・対象解決: `POST /v1/edits/validate`（副作用なし）
- 変更: `POST /v1/edits/apply`
- 確認: object 再取得、frame 取得

`validate` の成功は、後続の `apply` 成功を保証しない。人間や別クライアントが状態を変更できるため、`apply` は必ず同じ検証を再実行する。

### 6.3 汎用だが過度に抽象化しない

中核編集モデルは汎用化する一方、AviUtl2 のエフェクト体系を無理に別物へ置き換えない。

- 共通化するもの: 時間、レイヤー、位置、オブジェクト操作、revision、エラー形式
- AviUtl2 に寄せるもの: エフェクト名、設定項目名、動的な設定値
- 利便性レイヤー: text、audio、image、title、subtitle などを CLI、テンプレート、Skill で追加

編集 JSON は AviUtl2 のバージョンや導入プラグインに依存する。エージェントは必要なエフェクトを `describe effect` で確認してから編集 JSON を組み立てる。

### 6.4 SDK ハンドルを外へ出さない

AviUtl2 の生ポインタ、`ObjectHandle`、`EffectHandle`、`EditSection` を HTTP ハンドラーやクライアントへ公開しない。

HTTP ではセッション内だけ有効な opaque ID と、シリアライズ可能な値のみを扱う。

### 6.5 SDK 呼び出しを明示的に直列化する

Q1 で任意スレッドから呼べると判明しても、同時 SDK 呼び出しが安全であるとは仮定しない。

プラグインは `EditorGate` を持ち、read、write、render を含むすべての SDK 呼び出しを一度に一つへ制限する。

```text
HTTP request
    │
    ▼
EditorGate 取得（タイムアウト付き）
    │
    ├─ 取得失敗 → 503 EDITOR_BUSY
    │
    ▼
call_read_section / call_edit_section / render
    │
    ▼
解放
```

#### gate 取得タイムアウト

**gate の取得は必ずタイムアウトを持つ。**取得できなかった場合は待ち続けず、`503 EDITOR_BUSY` に現在の保持理由（`rendering`、`applying`、`reading`）を添えて返す。

タイムアウトなしで待つと、長い render の裏で全 HTTP 要求がハングし、CLI とエージェントが応答不能になる。

初期値の目安:

| 操作 | gate 待ちタイムアウト |
|---|---|
| read | 2 秒 |
| write | 5 秒 |
| render | 2 秒 |

これは**待ち行列のタイムアウト**であり、実行中の SDK 呼び出しを中断するものではない。SDK 呼び出し自体を打ち切れるとは仮定しない（10.4 参照）。

#### gate を取らない経路

`/healthz` と `/v1/status`、`/v1/capabilities` は SDK を呼ばず、キャッシュされた atomic 値だけから応答する。

これは必須要件である。理由:

- CLI の stale session 判定は `/healthz` の応答性を根拠にしている（14.3）。長い render 中に `/healthz` が応答しないと、生きているセッションを stale と誤判定して削除する。
- エージェントが `EDITOR_BUSY` から復帰するには `status` をポーリングする必要があり、その status が busy によってブロックされては意味がない。

したがって `/v1/status` が返す `contentRevision`、`projectEpoch`、`busyReason` などは、イベントコールバックと gate の出入りで更新されるキャッシュ値である。

Q1 の結果が UI スレッド限定であれば、`EditorGate` の内部実装を UI スレッドマーシャリングへ差し替える。HTTP API の契約は変えない。

### 6.6 write 対象を明示する

write payload は必ず次を明示する。

- `projectEpoch`
- `sceneId`
- `expectedContentRevision`
- object ID または一意に解決できる安定 selector

初期 write API では、次を対象指定に使わない。

- 現在選択中のオブジェクト
- 現在のカーソル位置
- 現在表示中のシーンという暗黙状態
- タイムライン UI の表示範囲

読み取りでは `selected=true` 等を使用できる。write の場合は、読み取って得た明示的 object ID を次の要求へ渡す。

### 6.7 検証と変更の間に人間の編集を挟ませない 【暫定・Q1 依存】

次の実装は禁止する。

```text
call_read_section で revision と対象を検証
        │
        ├─ gate を解放する / この間に人間が編集できる
        ▼
call_edit_section で変更
```

`apply` は `EditorGate` を取得した後、**gate を解放せずに**次を行う。

1. `projectEpoch` の確認
2. `sceneId` の確認
3. `contentRevision` の確認
4. object ID / selector の解決
5. 現在状態に対する意味検証
6. mutation
7. post condition 確認
8. post-write revision の確定

**可能であれば 1〜8 を同一の `call_edit_section` 内で行う。**Q1 で edit section 内から現在状態を読めないと判明した場合は、gate を保持したまま `call_read_section` → `call_edit_section` の順で実行する。この場合も人間の編集は割り込めないが、SDK 内部でのセクション間の状態一貫性は保証されないため、mutation 直前に対象の存在を再確認する。

守るべき本質は「同一セクションであること」ではなく「**検証から mutation までの間に人間の編集が割り込めないこと**」である。

### 6.8 capability-driven にする

SDK、AviUtl2 本体、導入エフェクト、Q1〜Q7 の結果によって利用可能な機能は異なる。

クライアントは固定機能を仮定せず、`GET /v1/capabilities` を確認する。

- 対応 operation
- frame render の可否と対応パラメータ
- `maxOperationsPerEdit`
- write 可否
- undo の可否
- read / write / render の busy 状態
- 対応 API バージョン

## 7. アーキテクチャ 【確定】

### 7.1 実行時構成

```text
┌─────────────────────────────────────────────┐
│ AI Agent                                    │
│ Codex / Claude Code / MCP host              │
└───────────────────┬─────────────────────────┘
                    │ CLI process / MCP stdio
                    ▼
┌─────────────────────────────────────────────┐
│ aviutl2-agent.exe                           │
│ - CLI                                       │
│ - session discovery                         │
│ - HTTP client                               │
│ - MCP server                                │
│ - human / machine output                    │
└───────────────────┬─────────────────────────┘
                    │ HTTP + JSON / image/png
                    ▼
┌─────────────────────────────────────────────┐
│ aviutl2-agent-plugin.aux2                   │
│ - tiny_http（worker 複数本）                 │
│ - auth / routing                            │
│ - state cache（gate 不要な status 応答用）   │
│ - EditorBackend                             │
│ - EditorGate（timeout 付き）                 │
│ - revision / object ID mapping              │
│ - aviutl2-rs adapter                        │
└───────────────────┬─────────────────────────┘
                    │ AviUtl2 Plugin SDK
                    ▼
┌─────────────────────────────────────────────┐
│ AviUtl2                                     │
│ - in-memory project                         │
│ - timeline                                  │
│ - Undo                                      │
│ - rendering                                 │
└─────────────────────────────────────────────┘
```

### 7.2 依存方向

```text
protocol  ←  plugin
    ↑
    └──────  cli

plugin  → aviutl2-rs
cli     → HTTP / MCP libraries
protocol → serde / thiserror
```

禁止する依存:

```text
cli → plugin
cli → aviutl2-rs
protocol → aviutl2-rs
protocol → HTTP server implementation
```

## 8. コンポーネント設計 【確定】

### 8.1 `protocol` crate

責務:

- HTTP DTO
- `SceneEdit`、`Operation`、`ApiError`、status、capabilities
- serde による JSON シリアライズ
- API バージョニング
- AviUtl2 非依存の静的・Domain validation
- MCP Tool Schema を作るために必要な型情報

禁止事項:

- `aviutl2-rs` への依存
- SDK ハンドルの保持
- HTTP サーバー・HTTP クライアント実装
- AviUtl2 の現在状態が必要な検証

初期依存:

- `serde`
- `serde_json`
- `thiserror`

`schemars` は MCP Tool Schema の実装で実際に必要になった時点で追加する。JSON Schema ファイルや OpenAPI を事前生成・コミットしない。

### 8.2 `plugin` crate

責務:

- `aviutl2-rs` による汎用プラグイン登録
- `EditHandle` の初期化と保持
- HTTP サーバーの起動・停止
- 認証、ルーティング、body 制限
- `EditorGate` による SDK 呼び出し直列化
- gate を取らずに応答できる状態キャッシュの維持
- AviUtl2 現在状態に対する意味検証
- SDK 型と API DTO の変換
- オブジェクト・エフェクトの参照と編集
- session、projectEpoch、revision、opaque object ID 管理
- フレーム取得、バッファコピー、縮小、PNG 化
- 構造化ログ

初期モジュール構成:

```text
crates/plugin/src/
├─ lib.rs
├─ plugin.rs          # 登録・ライフサイクル
├─ http.rs            # tiny_http、worker、ルーティング、認証
├─ state.rs           # gate 不要な状態キャッシュ
├─ backend.rs         # EditorBackend trait と AviUtl2 実装
├─ gate.rs            # SDK 呼び出し直列化 / UI marshal / timeout
├─ editor.rs          # read / validate / apply
├─ identity.rs        # projectEpoch、object ID map
├─ revision.rs        # content / UI revision
├─ render.rs          # frame buffer copy / 縮小 / PNG
├─ session.rs         # session file
└─ logging.rs
```

ファイルは実装量に応じて分割する。最初から階層を増やしすぎない。

#### HTTP ランタイム 【暫定】

`tiny_http` によるブロッキングサーバーを第一候補とする。

想定負荷はループバック、通常クライアント1つ、エンドポイント数十個未満であり、プラグイン内に非同期ランタイムを常駐させる利点が小さい。

**worker は複数本にする。**単一スレッドで accept ループを回すと、`EditorGate` を長時間保持する render や apply の裏で `/healthz` と `/v1/status` すら応答しなくなる。tiny_http の `Server` は `Arc` で共有して複数スレッドから `recv()` できるため、次の構成をとる。

```text
tiny_http::Server（Arc 共有）
    ├─ worker 1 ──┐
    ├─ worker 2 ──┼─ ルーティング
    ├─ worker 3 ──┤     ├─ /healthz, /v1/status, /v1/capabilities → state cache（gate 不要）
    └─ worker 4 ──┘     └─ その他 → EditorGate（timeout 付き）
```

worker 数は 4 程度で足りる。並列度を上げても SDK 呼び出しは gate で直列化されるため、目的は「軽量エンドポイントを SDK 待ちで詰まらせない」ことだけである。

終了処理:

1. `shuttingDown = true`
2. 新規 write を `503` で拒否
3. HTTP server の待機を `unblock()`
4. 実行中の要求が終了するまで短い猶予を与える
5. 全 worker thread を join
6. session file を削除
7. SDK handle を解放

実行中の SDK 呼び出しを `unblock()` で中断できるとは仮定しない。AviUtl2 終了中の挙動は Q1、Q4 で検証する。

#### `EditorGate`

`EditorGate` は transport から独立した SDK 呼び出し境界である。

```rust
enum GateError {
    Busy { reason: BusyReason, waited: Duration },
    ShuttingDown,
    Backend(EditorError),
}

trait EditorDispatcher {
    fn read<T>(&self, timeout: Duration, f: impl FnOnce(&ReadSection) -> T)
        -> Result<T, GateError>;

    fn write<T>(&self, timeout: Duration, f: impl FnOnce(&mut EditSection) -> T)
        -> Result<T, GateError>;

    fn render(&self, timeout: Duration, request: RenderRequest)
        -> Result<OwnedFrame, GateError>;
}
```

実際の Rust API は object safety やクロージャ制約に合わせて調整するが、次は維持する。

- HTTP handler が `EditHandle` を直接呼ばない
- gate 取得には必ずタイムアウトがある
- gate を保持している間、保持理由を state cache へ反映する

#### イベントコールバック

AviUtl2 のイベント用スレッドから呼ばれるコールバック内では、編集 API を再入的に呼び出さない。

イベントコールバックは次だけを行う。

- revision tracker への通知
- object map / read cache を dirty にする
- projectEpoch 更新候補の通知
- state cache の更新
- ログ・メトリクス更新

重い処理、HTTP 応答、`call_edit_section`、画像エンコードは行わない。

Q5 の結果に基づき、API apply が発生させたイベントの二重 revision 増加を防ぐ。

#### frame render

render callback からは owned buffer のみを外へ返す。

```rust
struct OwnedFrame {
    scene_id: i32,
    frame: u32,
    width: u32,
    height: u32,
    pitch: u32,
    pixel_format: PixelFormat,
    pixels: Vec<u8>,
}
```

SDK の参照、slice、pointer を callback 外へ持ち出さない。

縮小と PNG エンコードは `EditorGate` を解放した後に行う。エンコードのために AviUtl2 を待たせない。

### 8.3 CLI / MCP バイナリ

責務:

- session 探索・選択
- HTTP クライアント
- CLI コマンド
- 人間向け表示と機械可読 JSON 出力
- 安定した exit code
- 編集後に未保存である旨の通知
- MCP サーバー（stdio）
- frame image を MCP image content または resource として返す

構成:

```text
crates/cli/src/
├─ main.rs
├─ session.rs
├─ client.rs
├─ commands.rs
├─ output.rs
└─ mcp.rs
```

CLI と MCP は同じ HTTP client 層を利用し、編集ロジックを二重実装しない。

クライアントは `protocol` crate を使うため、JSON parse、型、enum、未知フィールドの拒否は自然に行われる。一方、独立した Domain validation や AviUtl2 状態検証をクライアントへ重複実装しない。サーバー側を正とする。

想定依存:

- `clap`
- ブロッキング HTTP クライアント（`ureq` 等）
- `image` または `png`（frame の縮小・再エンコードが CLI 側で必要な場合）
- `rmcp` または実装時点で適切な Rust MCP SDK
- MCP 実行に必要な場合のみ `tokio`

Tokio を使う場合も外部 CLI プロセス内に限定し、AviUtl2 プロセス内のプラグインへ持ち込まない。

### 8.4 なぜ Rust に統一するか

Rust 単一言語化により、次が不要になる。

- Rust → OpenAPI → Go クライアント生成
- 生成物のコミットと CI 差分検査
- Cargo workspace と `go.work` の二重管理
- Go 側の DTO 再定義
- Rust / Go 間の validation 差異
- Dockerfile の二言語ビルドステージ

HTTP 境界は多言語クライアントの可能性を維持するために残す。Rust 統一は HTTP を不要にする判断ではない。

MCP Rust SDK が着手時点で要件を満たさない場合は、次の順で判断する。

1. Rust で最小 JSON-RPC / MCP を実装する
2. MCP サーバーだけ薄い別バイナリとして Go 等で実装する
3. CLI と plugin の設計は変更しない

## 9. モノレポ構成 【確定】

```text
aviutl2-agent/
├─ README.md
├─ LICENSE
├─ Cargo.toml                 # workspace
├─ Cargo.lock
├─ rust-toolchain.toml
├─ Dockerfile                 # 正規ビルド環境
│
├─ crates/
│  ├─ protocol/               # 安定 DTO、AviUtl2 非依存
│  ├─ plugin/                 # aviutl2-agent-plugin.aux2
│  └─ cli/                    # aviutl2-agent.exe（CLI + MCP）
│
├─ examples/
│  ├─ update-text.json
│  ├─ title-scene.json
│  └─ image-layout.json
│
├─ docs/
│  ├─ design.md               # 本文書
│  ├─ phase0.md               # Q1〜Q7 の検証手順と結果記録
│  ├─ development.md
│  └─ compatibility.md
│
└─ .github/workflows/
   ├─ ci.yml                  # Linux unit test + Docker build
   └─ windows-smoke.yml       # Windows側の検証補助 / fallback
```

`docs/phase0.md` を分離する理由は、Q1〜Q7 の検証が「手順書と結果記録」であり、設計文書とは寿命が違うためである。Phase 0 が終われば phase0.md は結果の記録として凍結し、design.md 側の【暫定】【未定】章を書き直す。

必要になった時点で追加するもの:

- `skills/`: Agent Skill を実装するとき
- `api/`: 外部言語向け Schema / OpenAPI を配布するとき
- `tests/golden/`: 実際に golden test が必要になったとき
- `crates/xtask/`: shell / cargo alias で管理できなくなったとき
- `docs/threat-model.md`: loopback API の数行を超える脅威モデルが必要になったとき

## 10. API 契約 【暫定】

### 10.1 正本

`protocol` crate の Rust 型を正本とする。プラグインと CLI は同じ crate を参照するため、DTO の二重管理は発生しない。

外部言語からの利用需要が出た時点で `schemars` / OpenAPI 生成を追加する。

### 10.2 検証の三層

```text
JSON parse / serde 構造検証
        │
        ▼
Domain validation（AviUtl2 非依存）
        │
        ▼
AviUtl2 current-state validation
        │
        ▼
apply
```

#### 構造検証

- 必須項目
- 型、enum、数値範囲
- 未知プロパティ拒否
- operation ごとの構造
- 色、時間、ID の基本書式

`serde(deny_unknown_fields)` と型定義で行う。

#### Domain validation

- `startFrame <= endFrame`
- layer の基本範囲
- `clientId` の重複
- patch に変更項目が存在するか
- 同一要求内参照の整合性
- operation 数が capability の上限以下か
- UI 相対 selector が write に使われていないか

#### AviUtl2 current-state validation

- `projectEpoch` が一致するか
- `sceneId` が存在するか
- object ID / selector が一意に解決できるか
- 対象オブジェクトが存在するか
- 対象レイヤーがロックされていないか
- エフェクト名・設定項目が存在するか
- 指定値を SDK が受け付けられるか
- 素材ファイルが存在し読み込み可能か
- `expectedContentRevision` が一致するか
- 配置衝突があるか
- 現在 write を受け付けられる状態か

### 10.3 write envelope

全 write payload に次を含める。

```json
{
  "apiVersion": "aviutl2-agent/v1alpha1",
  "kind": "SceneEdit",
  "projectEpoch": "prjepoch_01...",
  "sceneId": 0,
  "expectedContentRevision": 42,
  "operations": []
}
```

役割:

- HTTP path `/v1`: HTTP API の互換性
- `apiVersion`: 編集ドキュメント形式
- `projectEpoch`: プロジェクトロード境界
- `sceneId`: write 対象シーンの明示
- `expectedContentRevision`: 内容変更の楽観的排他制御

初期版では `expectedUiStateRevision` を受け付けない。将来 UI 相対 selector を write に許可する場合のみ追加を検討する。

### 10.4 エラー形式

```json
{
  "error": {
    "code": "OBJECT_NOT_FOUND",
    "message": "target object does not exist",
    "requestId": "req_01...",
    "details": [
      {
        "path": "/operations/0/target/id",
        "code": "not_found",
        "message": "obj_123 is not available in project epoch prjepoch_01..."
      }
    ],
    "recovery": {
      "action": "list_objects",
      "message": "Reload the target object list and retry with the current objectId and contentRevision."
    }
  }
}
```

エラーメッセージは LLM が次の行動を決められる粒度にする。

HTTP status と domain code を分ける。

- 400: JSON・構造不正
- 401: 認証不正
- 404: session / project / scene / object 不在
- 409: projectEpoch / revision 不一致、selector 曖昧、配置衝突
- 413: body size 超過
- 422: AviUtl2 状態に対する意味的不整合
- 503: AviUtl2 が操作を受け付けられない状態、または `EditorGate` の取得タイムアウト
- 500: プラグイン内部エラー

**`504` は使わない。**SDK 呼び出しを外部から打ち切る手段がない以上、「タイムアウトした」と応答できる状況は実質的に「gate を取れなかった」場合だけであり、これは 503 に含まれる。実行中の SDK 呼び出しが返ってこない場合、HTTP 側から返せる応答は存在しない。Q3 で render 自体にキャンセル手段があると判明した場合のみ、504 の導入を再検討する。

### 10.5 バッチ契約

Q2 の検証結果を得るまで、次を確定仕様とする。

```json
{
  "limits": {
    "maxOperationsPerEdit": 1
  }
}
```

#### この制限の理由

**「1 operation のほうが安全だから」ではない。**Phase 2 の実装量を減らすためである。

安全性の観点では、むしろ逆の性質がある。`maxOperationsPerEdit = 1` の下でタイトル画面のような複数オブジェクトの構築を行うと、CLI / Skill は apply を N 回に分けることになる。

```text
apply(背景図形)   ← gate 解放
                  ← ここで人間が編集できる
apply(タイトル)   ← gate 解放
                  ← ここで人間が編集できる
apply(BGM)
```

この間に人間が編集すると `expectedContentRevision` が合わなくなり、**中途半端なタイトル画面が残ったまま失敗する。**一方、1つの edit section 内で N operation を流せば、人間の編集は割り込めず、失敗しても revision と状態は一貫する。

つまり本質的なリスク要因は「原子的かどうか」ではなく「**人間の編集が割り込めるか**」である。

#### したがって

- Phase 2 は `maxOperationsPerEdit = 1` で始める（実装単純化のため）
- Q2 の結果にかかわらず、**Phase 3 の目標は「1 edit section 内での複数 operation」**とする
- Q2 が原子的ロールバックを許すなら、失敗時は全件未適用として応答する
- 許さないなら、失敗時は適用済み件数と失敗理由を返し、復旧手順（Q7 の Undo 可否に依存）を Skill に定義する
- いずれの場合も、複数 operation は gate を保持したまま実行し、間で解放しない

複数 operation を分割せざるを得ない期間は、Skill が「複数オブジェクトの構築中は AviUtl2 を操作しないでほしい」と利用者へ伝える。

### 10.6 `validate` と `apply`

`validate` は副作用なしで現在状態に対する検証と対象解決を行う。

`apply` は同じ payload を受け取り、`EditorGate` 内で検証を再実行してから変更する。

重要な契約:

- `validate` の成功は `apply` の成功を保証しない
- `apply` は validate のキャッシュ結果を盲信しない
- `apply` は対象を再解決する
- `apply` は revision を再確認する
- post-write revision を応答に返す

## 11. HTTP API 【暫定】

### 11.1 セッション・状態

```http
GET /healthz
GET /v1/status
GET /v1/capabilities
```

この3つは `EditorGate` を取得せず、state cache から応答する（6.5 参照）。

`/v1/status` 例:

```json
{
  "sessionId": "ses_01...",
  "pid": 12345,
  "pluginVersion": "0.1.0",
  "apiVersion": "v1",
  "aviutl2Version": "2.1.2",
  "aviutl2RsVersion": "0.41.0",
  "projectLoaded": true,
  "projectEpoch": "prjepoch_01...",
  "activeSceneId": 0,
  "contentRevision": 42,
  "uiStateRevision": 118,
  "unsavedChanges": true,
  "readAvailable": true,
  "writeAvailable": true,
  "renderAvailable": true,
  "gateHeldBy": null,
  "busyReason": null
}
```

`unsavedChanges` は、API による編集後に利用者へ保存を促すために CLI / Skill が参照する（2.3 参照）。SDK から取得できない場合は、API apply を行ったかどうかをプラグイン側で記録して返す。

`/v1/capabilities` 例:

```json
{
  "operations": ["update", "move", "set-layer", "resize-time"],
  "frameRender": {
    "available": true,
    "maxWidth": 3840,
    "defaultMaxWidth": 960,
    "formats": ["png"]
  },
  "undo": { "available": false },
  "writeSelectors": ["object-id", "unique-static-selector"],
  "limits": {
    "maxOperationsPerEdit": 1,
    "maxRequestBodyBytes": 1048576,
    "maxObjectListItems": 500,
    "gateWaitMillis": { "read": 2000, "write": 5000, "render": 2000 }
  }
}
```

### 11.2 読み取り

```http
GET /v1/project
GET /v1/scenes
GET /v1/scenes/current
GET /v1/timeline?sceneId=0
GET /v1/objects?sceneId=0
GET /v1/objects/{objectId}
GET /v1/effects
GET /v1/effects/describe?name=テキスト
GET /v1/fonts
```

`GET /v1/objects` の filter 例:

- `selected=true`（read-only 用）
- `frame=150`
- `layer=3`
- `rangeStart=0&rangeEnd=300`
- `effect=テキスト`

一覧はデフォルトで要約を返す。

```json
{
  "objects": [
    {
      "id": "obj_17",
      "sceneId": 0,
      "effect": "テキスト",
      "layer": 2,
      "range": { "startFrame": 0, "endFrame": 149 },
      "summary": { "text": "動画タイトル" }
    }
  ],
  "nextCursor": null
}
```

全設定項目は `GET /v1/objects/{id}` で取得する。数百オブジェクトの全設定を一覧で返さない。

### 11.3 編集

```http
POST /v1/edits/validate
POST /v1/edits/apply
```

#### validate response

```json
{
  "valid": true,
  "projectEpoch": "prjepoch_01...",
  "baseContentRevision": 42,
  "sceneId": 0,
  "resolvedTargets": [
    {
      "operationIndex": 0,
      "op": "update",
      "objectId": "obj_17",
      "layer": 2
    }
  ],
  "warnings": []
}
```

#### apply response

```json
{
  "applied": true,
  "projectEpoch": "prjepoch_01...",
  "sceneId": 0,
  "contentRevisionBefore": 42,
  "contentRevisionAfter": 43,
  "unsavedChanges": true,
  "results": [
    {
      "operationIndex": 0,
      "objectId": "obj_17",
      "status": "updated"
    }
  ]
}
```

クライアントは `contentRevisionAfter == before + 1` を前提にしない。

Q2 が非原子的と判明した場合、部分失敗時の応答は `applied: false` と適用済み `results` を併せ持つ形になる。契約は Phase 0 後に確定する。

### 11.4 フレーム取得

Q3 が成立した場合:

```http
GET /v1/frames/{frame}?sceneId=0&maxWidth=960&format=png
```

パラメータ:

| 名前 | 既定値 | 説明 |
|---|---|---|
| `sceneId` | 必須 | 対象シーン |
| `maxWidth` | 960 | 出力画像の幅の上限。アスペクト比を保って縮小する |
| `format` | png | 初期版は png のみ |

応答:

- `Content-Type: image/png`
- `X-AviUtl2-Agent-Project-Epoch`
- `X-AviUtl2-Agent-Scene-Id`
- `X-AviUtl2-Agent-Frame`
- `X-AviUtl2-Agent-Content-Revision`
- `X-AviUtl2-Agent-Source-Size`（縮小前の解像度）

#### 縮小を既定にする理由

1920×1080 の PNG を MCP の image content として base64 で返すと、2〜3 MB のテキストになりエージェントのコンテキストを一度で圧迫する。一方、本 API における frame 取得の主目的は「文字が画面外にはみ出していないか」「配色が破綻していないか」といった構図レベルの確認であり、960px あれば足りる。

Q3 でレンダリング解像度自体を指定できると判明した場合は、SDK 側で縮小させる。できない場合は取得後に縮小する。いずれの場合も、縮小と PNG エンコードは `EditorGate` を解放した後に行う。

`projectEpoch` と `contentRevision` をヘッダーに含めるのは、エージェントが「この画像はどの状態のものか」を検証結果と突き合わせられるようにするためである。

初期版では現在のプロジェクト状態をレンダリングする。変更を一時適用して元へ戻す仮想プレビューは、Q2 のトランザクション特性が確認できるまで実装しない。

### 11.5 readiness と busy

write / render が一時的に利用できない場合、`/v1/status` と `503` のエラーに理由を含める。

```json
{
  "error": {
    "code": "EDITOR_BUSY",
    "message": "AviUtl2 is currently exporting a video and write operations are unavailable.",
    "requestId": "req_01...",
    "details": [],
    "recovery": {
      "action": "retry_status",
      "message": "Call get_status and retry after writeAvailable becomes true."
    }
  }
}
```

`EditorGate` の取得タイムアウトも同じ形式で返す。

```json
{
  "error": {
    "code": "EDITOR_BUSY",
    "message": "Another operation is holding the editor (rendering). Waited 2000ms.",
    "recovery": {
      "action": "retry_after",
      "retryAfterMillis": 1000,
      "message": "Retry the same request. The current holder is expected to finish shortly."
    }
  }
}
```

### 11.6 Undo 【未定・Q7 依存】

Q7 で「直前の自分の apply だけを安全に戻せる」と判明した場合のみ、次を capability として追加する。

```http
POST /v1/edits/undo-last
```

```json
{
  "projectEpoch": "prjepoch_01...",
  "expectedContentRevision": 43
}
```

`expectedContentRevision` は直前の apply 応答の `contentRevisionAfter` でなければならない。一致しない場合は人間または別クライアントが編集しているため `409` を返し、Undo を実行しない。

Q7 の結果が否定的な場合、このエンドポイントは提供しない。復旧手順は「エージェントが利用者へ Ctrl+Z を依頼する」か「差分を計算して修復 apply する」になり、これを Agent Skill に明記する。

## 12. 汎用編集モデル 【暫定】

### 12.1 操作種別

候補:

```text
create
update
move
resize-time
set-layer
duplicate
delete
add-effect
update-effect
remove-effect
```

初期リリースで全てを実装しない。実装済みのものだけを capability として公開する。

Phase 2 の最初の write API は既存 object に対する次の操作に絞る。

```text
update
move
resize-time
set-layer
```

### 12.2 単一 operation の例

```json
{
  "apiVersion": "aviutl2-agent/v1alpha1",
  "kind": "SceneEdit",
  "projectEpoch": "prjepoch_01...",
  "sceneId": 0,
  "expectedContentRevision": 42,
  "operations": [
    {
      "op": "update",
      "target": { "id": "obj_17" },
      "patch": {
        "items": {
          "テキスト": "新しい動画タイトル",
          "サイズ": 96,
          "色": "#FFFFFF"
        }
      }
    }
  ]
}
```

### 12.3 タイトル画面はプリセットまたは Skill

タイトル画面は専用 API にしない。最終的には次の複数オブジェクトで構成される。

- 背景図形
- タイトルテキスト
- BGM
- フェード等のエフェクト

Phase 3 では、これを**1つの edit section 内の複数 operation** として実行することを目標とする（10.5 参照）。

`maxOperationsPerEdit = 1` の期間は、CLI / Skill が operation を順番に実行する。この期間は途中で人間が編集すると中途半端な状態が残るため、Skill は次を行う。

- 実行前に利用者へ「複数ステップの編集を行う」旨を伝える
- 各 apply の間で `contentRevision` を再取得し、想定外の変化があれば中断して報告する
- 中断時に、すでに作成したオブジェクトの一覧を利用者へ提示する

`examples/title-scene.json` は将来仕様の例として置けるが、capability が `maxOperationsPerEdit = 1` の間は実行可能例として扱わない。

### 12.4 動的エフェクト定義

すべてのエフェクトを静的定義へ埋め込まず、実行中環境から取得する。

```http
GET /v1/effects/describe?name=テキスト
```

SDK から確実に取得できる情報だけを返す。

```json
{
  "name": "テキスト",
  "effectType": "media-object",
  "items": [
    {
      "name": "テキスト",
      "valueType": "string",
      "writable": true,
      "constraints": null
    },
    {
      "name": "サイズ",
      "valueType": "number",
      "writable": true,
      "constraints": null
    },
    {
      "name": "色",
      "valueType": "color",
      "writable": true,
      "constraints": null
    }
  ]
}
```

SDK から minimum、maximum、required、選択肢等を取得できた場合のみ `constraints` へ入れる。取得できない制約を推測で埋めない。

設定値を SDK へ渡す際に文字列表現が必要でも、HTTP DTO では可能な範囲で `number`、`boolean`、`color` 等へ型付けする。型付けできない項目は raw string にフォールバックする。

### 12.5 selector

read API では柔軟な filter を提供する。

write API で許可する selector は、明示的な `sceneId` の中で内容に基づき一意に解決できるものに限定する。

```json
{
  "selector": {
    "layer": 2,
    "frame": 75,
    "effect": "テキスト",
    "textEquals": "動画タイトル"
  }
}
```

複数一致した場合は暗黙に選ばず、候補を含む `409 SELECTOR_AMBIGUOUS` を返す。

初期 write API では次を禁止する。

```json
{ "selector": { "selected": true } }
```

```json
{ "selector": { "currentFrame": true } }
```

## 13. セッション、projectEpoch、object ID、revision 【暫定】

### 13.1 セッション

AviUtl2 プロセスごとに session を生成する。

- plugin load から unload まで有効
- 複数 AviUtl2 インスタンスを識別する
- bearer token、endpoint、PID を保持する

### 13.2 `projectEpoch`

プロジェクトの新規作成・ロード・破棄のたびに、新しい opaque `projectEpoch` を生成する。

```text
prjepoch_01H...
```

projectEpoch が変わると次を全て無効化する。

- object ID
- selector 解決キャッシュ
- contentRevision の比較対象
- frame cache
- validate 結果

write payload の projectEpoch が現在値と異なる場合は `409 PROJECT_EPOCH_MISMATCH` を返す。

`contentRevision` と別に epoch を持つ理由は、プロジェクトのロード後に revision が偶然一致する可能性があり、revision の等値比較だけでは「別のプロジェクトである」ことを検出できないためである。

### 13.3 object ID

API の object ID は次の性質を持つ。

- 現在の session と projectEpoch 内のみ有効
- AviUtl2 再起動後は無効
- プロジェクトロード後は無効
- 削除した ID は再利用しない
- SDK handle の内部値を公開しない
- ID 解決時に対象の存在を再確認する

内部ハンドルが再利用される可能性が Q5 で判明した場合のみ、次を追加する。

- generation counter
- sceneId
- effect name
- layer / frame range の fingerprint

Q5 でハンドルが安定していると分かれば、opaque ID と存在再確認だけで足りる。判明前から多層の防御を実装しない。

### 13.4 `contentRevision`

プロジェクトの内容が変わったときのみ変化する。

含む:

- シーンの追加・削除・プロパティ変更
- オブジェクト作成・更新・移動・削除
- エフェクト変更
- API apply
- Undo / Redo による内容変更

含まない:

- アクティブシーンの切り替え
- カーソル移動
- 選択変更
- フォーカス変更
- タイムライン表示範囲変更

プロジェクトのロードは `projectEpoch` の変更として扱い、revision の比較対象にしない。

クライアントは revision の等値だけを利用し、増分幅を仮定しない。

### 13.5 `uiStateRevision`

内容を伴わない UI 状態変化で変化する。

- アクティブシーン切り替え
- 選択状態
- カーソル位置
- フォーカス
- 表示範囲・表示レイヤー

write 排他には使用しない。読み取りキャッシュの無効化や、利用者の選択状態を追う用途に使う。

### 13.6 API write とイベントの二重計上

Q5 の検証結果をもとに実装方式を決めるが、外部契約として次の invariant を守る。

- apply 応答の `contentRevisionAfter` は、その apply 後の確定値である
- apply 自身に由来する遅延イベントで、応答直後に revision が再度変化しない
- API 外の変更は revision へ反映される

候補実装:

- mutation generation を発行し、同期イベントを同一 mutation として集約
- API write 中の event を dirty flag として記録し、終了時に1回だけ revision を進める
- イベントが非同期の場合、SDK state fingerprint との組み合わせで重複を抑制

方式は Phase 0 で決定する。

## 14. CLI 設計 【確定】

### 14.1 コマンド案

```text
aviutl2-agent session list
aviutl2-agent status
aviutl2-agent capabilities

aviutl2-agent project get
aviutl2-agent scene list
aviutl2-agent scene current
aviutl2-agent timeline get --scene 0

aviutl2-agent object list --scene 0
aviutl2-agent object get <id>
aviutl2-agent effect list
aviutl2-agent effect describe <name>

aviutl2-agent edit validate --file edit.json
aviutl2-agent edit apply    --file edit.json

aviutl2-agent frame get --scene 0 --frame 75 --max-width 960 --output preview.png

aviutl2-agent mcp serve
```

### 14.2 入出力

- JSON 入力は `--file` または stdin
- 非 TTY では JSON 出力を既定にする
- `--output json|table|text`
- エラーは stderr
- stdout には機械処理対象だけを出す
- token を表示しない
- exit code を固定する
- **`edit apply` の成功後、未保存である旨を stderr へ表示する**

```text
0  成功
2  CLI 引数不正
3  入力 JSON 不正
4  接続失敗
5  認証失敗
6  検証失敗
7  epoch / revision / selector 衝突
8  AviUtl2 操作失敗
9  editor busy（リトライ可能）
```

`9` を独立させるのは、エージェントが「待って再試行すればよい」と「入力を直すべき」を区別できるようにするためである。

### 14.3 セッション探索

プラグインは起動時に次の session file を生成する。

```text
%LOCALAPPDATA%\aviutl2-agent\sessions\<pid>-<sessionId>.json
```

```json
{
  "sessionId": "ses_01...",
  "pid": 12345,
  "startedAt": "2026-07-27T04:00:00Z",
  "endpoint": "http://127.0.0.1:49152",
  "token": "random-secret",
  "pluginVersion": "0.1.0"
}
```

古い session file の判定:

1. endpoint へ短いタイムアウトで接続できるか
2. `/healthz` の `sessionId` が一致するか
3. 一致しない、または接続できない file は stale として扱う

PID の生存確認だけに依存しない。Windows は PID を再利用する。

この判定が成立するには、**`/healthz` が `EditorGate` を待たずに応答する必要がある**（6.5 参照）。長い render 中に応答しないと、生きているセッションを削除してしまう。

複数 session がある場合:

- `--session <sessionId>`
- `--pid <pid>`
- 対話 TTY では選択
- 非 TTY で曖昧ならエラー

WSL からは Windows 版 `aviutl2-agent.exe` を起動する運用を第一候補とする。

## 15. MCP 設計 【暫定】

MCP は CLI と同じバイナリのサブコマンドとして提供する。

```text
aviutl2-agent mcp serve
```

transport は stdio とする。AviUtl2 プラグインへ MCP を組み込まない。

### 15.1 Phase 1.5 の read-only Tools

```text
aviutl2_get_status
aviutl2_list_objects
aviutl2_describe_effect
```

最小構成で Claude Code / Codex に接続し、次を実測する。

- Tool 名の探索性
- 説明文の理解しやすさ
- object list の出力量
- effect item の表現
- エラーから自己修正できるか
- session 指定の扱いやすさ

### 15.2 追加の read Tools

```text
aviutl2_get_project
aviutl2_get_object
aviutl2_render_frame
```

### 15.3 write Tools

```text
aviutl2_validate_edit
aviutl2_apply_edit
```

Q7 が肯定的な場合のみ追加:

```text
aviutl2_undo_last_edit
```

オブジェクト種別ごとの Tool を大量に増やさず、`validate_edit` と `apply_edit` を汎用操作面とする。

`apply_edit` と `undo_last_edit` には destructive hint を付ける。

### 15.4 frame の返し方

`aviutl2_render_frame` はファイルパスだけではなく、MCP client が画像として扱える content を返すことを目標とする。

```text
content:
  - type: image
    mimeType: image/png
    data: <base64>
```

**MCP 経路の既定 `maxWidth` は 960 とする。**フル解像度の base64 はエージェントのコンテキストを圧迫し、確認用途には過剰である（11.4 参照）。エージェントが明示的に大きな画像を要求した場合のみ上限を上げる。

SDK / MCP SDK の制約で直接 image content を返せない場合は、MCP resource として公開し、補助的にローカルパスを返す。

### 15.5 MCP 層の責務

- HTTP API を Tool として公開
- Tool Schema と Agent 向け説明文
- read-only / destructive の分類
- 出力の要約
- frame image の縮小と変換
- 利用者承認に適した Tool 粒度

編集ロジック、SDK 状態検証、revision 管理は MCP 層へ持たせない。

## 16. Agent Skill 設計 【暫定】

MCP write Tool が揃った後、Agent Skill を追加する。

Skill は API 仕様書の代替ではなく、編集手順と判断基準を定義する。

```text
1. status と capabilities を取得する
2. projectEpoch、sceneId、contentRevision を記録する
3. project、timeline、対象 object を取得する
4. 不明な effect は describe_effect する
5. UI相対状態ではなく明示 object ID / selector を使う
6. SceneEdit を生成する
7. validate し、警告と resolvedTargets を確認する
8. 必要な場合は利用者へ変更内容を提示する
9. apply する
10. apply 後の revision と object を再取得する
11. 視覚変更では代表 frame を取得する
12. 期待結果との差を評価し、必要なら再編集する
13. 一連の作業が終わったら、利用者へ保存を促す
```

Skill に記述する知識:

- 字幕の改行・セーフエリア
- テロップのコントラスト
- BGM 音量の基本方針
- 変更前後の確認フレーム選定
- Q2 に応じた大量変更の分割方法と、分割中の中断手順
- 利用者承認が必要な操作
- 外部素材パスの扱い
- busy / revision mismatch / stale object の復旧手順
- **失敗時の復旧手順（Q7 の結果に依存する）**
  - Undo API がある場合: 直前 apply の revision が一致するときのみ `undo_last_edit` を使う
  - ない場合: 利用者へ Ctrl+Z を依頼するか、差分を計算して修復 apply する
  - どちらの場合も、勝手に人間の作業を巻き戻さない
- **編集は未保存であり、保存は利用者が行うこと**
- capability にない機能を推測で呼ばない規則

## 17. HTTP セキュリティ 【確定】

localhost であっても認証なしにはしない。

### 17.1 必須対策

- `127.0.0.1` のみに bind
- 空きポートを動的取得
- 起動ごとにランダム bearer token を生成
- session file をユーザー専用領域へ保存
- `/v1` API で bearer token を必須化
- CORS ヘッダーを付与しない
- `Origin` ヘッダー付き要求を拒否
- `Host` が loopback でない要求を拒否
- request body のサイズ上限
- header 数・長さの上限
- SDK 操作の同時実行数を1に制限
- 外部 URL から素材を取得しない
- ファイルパスを canonicalize
- token、素材内容、長いテキストをログへ不用意に出さない

`/healthz` のみ認証不要とする。ただし返すのは sessionId と生存状態だけとし、token、project 名、素材パスを返さない。

想定する脅威モデルは「同一ユーザー権限で動く他プロセス、およびブラウザからの意図しないアクセス」である。同一ユーザーの任意プロセスが session file を読めば API を利用できるが、これは「ローカルのエージェントから AviUtl2 を操作する」という本来の目的そのものであり、許容する。

### 17.2 ファイルパス

初期版はユーザーが指定したローカル絶対パスを扱う。

- NUL 等の不正文字拒否
- canonicalize
- ファイル種別確認
- directory traversal の推測補正をしない
- HTTP URL をローカルへ自動ダウンロードしない
- API が任意ファイルの内容を読み取って返す機能を持たない

### 17.3 将来の scope

必要になった場合に token scope を分ける。

```text
read
write
render
admin
```

初期版は単一 token でよいが、内部ルーティングでは read / write / render を分類しておく。

## 18. ログと観測性 【確定】

プラグイン内の問題は AviUtl2 本体の不調に見えやすいため、構造化ログを重視する。

ログ項目:

- timestamp
- level
- requestId
- sessionId
- projectEpoch
- endpoint
- sceneId
- operation count
- gate 待ち時間 / 保持時間
- elapsed time
- contentRevision before / after
- result code
- editor busy reason
- panic / error chain

gate の待ち時間と保持時間を記録するのは、`EDITOR_BUSY` が頻発したときに原因（render が長い、apply が長い、UI 操作が長い）を切り分けるためである。

ログ先:

```text
%LOCALAPPDATA%\aviutl2-agent\logs\plugin.log
```

HTTP 応答へ内部 backtrace、token、生ポインタを返さない。

FFI 境界を越える panic を避ける。`catch_unwind` を使用する場合も、panic 後に SDK state が安全とは限らないため、重大 panic 後は write を無効化し（`writeAvailable: false`、`busyReason: "plugin-degraded"`）、利用者へ AviUtl2 の再起動を促す。

## 19. ビルド設計 【確定】

### 19.1 正規ビルド

Linux ベースの Docker で Windows MSVC target をビルドする。

```text
Docker / Linux
├─ cargo test --workspace（Linuxで動く範囲）
├─ cargo xwin build --target x86_64-pc-windows-msvc -p aviutl2-agent-plugin
├─ cargo xwin build --target x86_64-pc-windows-msvc -p aviutl2-agent
├─ .dll → .aux2
├─ checksum
└─ dist/ へ出力
```

概念的なコマンド:

```bash
cargo xwin build \
  --locked \
  --release \
  --target x86_64-pc-windows-msvc \
  -p aviutl2-agent-plugin

cargo xwin build \
  --locked \
  --release \
  --target x86_64-pc-windows-msvc \
  -p aviutl2-agent
```

Docker image、Rust toolchain、cargo-xwin を固定し、再現可能性を優先する。

### 19.2 Windows 実機検証

Linux Docker で生成した成果物を Windows へ配置し、次を確認する。

```text
plugin load
plugin register
HTTP health（render 中でも応答すること）
read
single update
Undo
frame render
plugin unload / AviUtl2 exit
```

ビルド成功は実行互換性を意味しないため、Windows 実機 smoke test をリリース前に必須とする。

### 19.3 Windows runner fallback

cross build が上流変更等で一時的に壊れた場合、`windows-latest` の MSVC build を暫定利用できる。

これは正規要件の変更ではなく、復旧までのフォールバックである。原因と復旧条件を issue / compatibility document に残す。

Q6 の「Windows ネイティブビルドとの差異がないか」を検証するためにも、Windows build 経路自体は常に動く状態を保つ。

### 19.4 配布物

```text
dist/
├─ aviutl2-agent-plugin.aux2
├─ aviutl2-agent.exe
├─ README.txt
├─ THIRD_PARTY_NOTICES.md
└─ SHA256SUMS
```

将来 AviUtl2 カタログ向け package を追加できるが、Phase 0 の必須要件にしない。

### 19.5 release profile

検討事項:

- `panic = "abort"` と FFI 安全性
- LTO
- strip
- debug symbols の別配布
- Windows subsystem
- 静的リンク可能性

panic 戦略は「小さくなるから」だけで決めず、プラグイン境界での panic 処理と合わせて決定する。18章の「重大 panic 後は write を無効化する」方針は `panic = "unwind"` と `catch_unwind` を前提にしているため、`abort` を選ぶ場合は degraded モードが成立しない点を踏まえて再検討する。

## 20. テスト戦略 【確定】

### 20.1 実機不要のテスト

- `protocol` の serde roundtrip
- 未知フィールド拒否
- Domain validation
- `EditorBackend` fake を使った handler test
- **gate タイムアウト時に 503 を返すこと**（fake backend を意図的に遅延させる）
- **gate 保持中でも `/healthz` と `/v1/status` が応答すること**
- 認証、Host、Origin、body limit
- session discovery
- CLI 引数、出力、exit code
- mock HTTP server による CLI integration test
- revision mismatch error の整形
- object list の要約・ページング
- frame 縮小処理（固定の入力画像に対する出力サイズ）
- SceneEdit parser の property test（必要性が出た時点）

SDK に触れる部分を `EditorBackend` / dispatcher 境界で切り出す。

### 20.2 Windows + AviUtl2 が必要なテスト

- Linux Docker 生成 `.aux2` をロードできる
- plugin register / drop
- HTTP server 起動 / 停止
- project / scene / object の read
- effect introspection
- text object update
- move / resize / layer change
- Undo 単位
- バッチ途中失敗
- event と revision
- ObjectHandle の更新・削除・Undo 後の安定性
- project reload と object ID 無効化
- frame buffer 取得・縮小・PNG 化
- **render 中の `/healthz` 応答性**
- ユーザー操作中の read / write / render
- AviUtl2 終了時に thread が残らない
- 複数 AviUtl2 instance
- 長時間利用でのメモリ・ハンドルリーク

### 20.3 Phase 0 手動チェックリスト

自動化が難しい項目は `docs/history/phase0.md` に手順と期待結果を残す。

- ドラッグ中に HTTP write
- モーダル表示中に HTTP write
- 再生中に frame render
- 出力中に status / write
- 大きな解像度の render 中に status / healthz を叩く
- 途中失敗後の Undo
- AviUtl2 強制終了後の stale session file

個人開発では、Windows GUI を完全自動化する基盤より、再現可能な短い手動チェックリストを優先する。

## 21. 依存更新戦略 【確定】

`aviutl2-rs` は exact version と `Cargo.lock` で固定する。

```toml
aviutl2 = "=0.41.0"
```

更新は自動マージしない。

更新時の確認:

1. CHANGELOG の Breaking 項目
2. 最小 AviUtl2 バージョン
3. Rust toolchain 要件
4. Linux Docker cross build
5. plugin load / unload
6. read / write / render smoke test
7. event / revision 回帰
8. HTTP API DTO に SDK 変更が漏れていないか
9. Q1〜Q7 の前提が崩れていないか

`docs/verification/windows.md` に記録する。

```text
plugin version | aviutl2-rs | minimum AviUtl2 | Rust | API version | build image
```

## 22. 実装ロードマップ 【確定】

### Phase 0: 技術成立性の確認

目的は製品機能を作ることではなく、Q1〜Q7 に答えを出すこと。コードは使い捨てでもよい。

完了条件:

- Linux Docker から Windows MSVC 向け plugin / CLI を生成できる（Q6）
- `.aux2` として AviUtl2 がロードできる
- plugin register / drop が動作する
- `GET /healthz` が応答する
- AviUtl2 終了時に HTTP thread が正常終了する
- Q1: SDK 呼び出し可能スレッド、再入、edit section 内 read の可否、直列化方式を確定
- Q2: Undo 単位、途中失敗、バッチ契約を確定
- Q3: frame 取得、buffer copy、解像度指定、縮小・PNG 化を確認
- Q4: user operation / busy 状態の挙動を確認
- Q5: event、revision、ObjectHandle の性質を確認
- Q7: Undo API の有無と安全性を確認

**Phase 0 の結果を本書の【暫定】【未定】章へ反映し、v0.5 を作ってから Phase 1 へ進む。**v0.5 は本書より短くなるはずである。検証で否定された選択肢は削除する。

### Phase 1: Read-only API

- status / capabilities（gate 不要経路）
- project / scene / timeline
- object list / get
- effect list / describe
- projectEpoch / object ID map
- session discovery
- CLI
- frame 取得（Q3 が成立した場合、縮小込み）

write API は公開しない。

### Phase 1.5: 最小 MCP

- MCP stdio server
- read-only Tool 3本
- Claude Code / Codex への接続
- DTO の冗長さと Tool 説明を実利用で評価
- frame の base64 サイズを実測し、既定 `maxWidth` を調整
- Phase 2 の API 設計へ反映

### Phase 2: 最小 write API

- `EditorGate`（timeout 込み）
- validate / apply
- projectEpoch / sceneId / contentRevision チェック
- 1要求1operation
- 既存 object の update
- move / set-layer / resize-time
- apply 後の object 再取得
- write MCP Tool
- 未保存通知

まず既存 object の安全な変更を優先する。

### Phase 3: オブジェクト生成と複数 operation

- create / duplicate / delete
- **1 edit section 内での複数 operation**（10.5 の目標）
- text / image / audio の代表シナリオ
- add / update / remove effect
- clientId と result mapping
- 失敗時の復旧手順（Q7 に依存）
- title / subtitle のテンプレート例

### Phase 4: 実用化

- JSON 出力の安定化
- object list の要約・ページング調整
- exit code 固定
- Windows / WSL 運用整理
- preview file / MCP image 管理
- エラーメッセージ改善
- AviUtl2 カタログ向け配布検討

### Phase 5: Agent Skill

- inspect → validate → apply → verify
- 字幕・タイトル・BGM・画像配置の編集知識
- 承認規則
- 失敗時の復旧手順
- 保存の促し
- 代表ユースケースの回帰テスト

## 23. 主要リスクと対策 【確定】

| リスク | 影響 | 対策 |
|---|---|---|
| SDK のスレッド制約が厳しい | 実行構成の変更 | Phase 0 Q1、EditorGate 抽象化、UI marshal を差し替え可能にする |
| edit section 内で read できない | 6.7 が成立しない | Phase 0 Q1、gate 保持のまま read → write へ後退 |
| Undo 単位・原子性が想定と異なる | バッチ契約の作り直し | Phase 0 Q2、Phase 2 は operation=1、Phase 3 で単一 section 複数 operation |
| 逐次 apply 中に人間が編集する | 中途半端な構築物が残る | 単一 section 複数 operation を目標、分割中は Skill が中断・報告 |
| gate 待ちでリクエストがハングする | CLI / エージェントが無応答 | gate 取得タイムアウト、503 EDITOR_BUSY、retryAfter |
| 長い render で status が詰まる | stale session 誤判定 | status / healthz を gate 不要経路に、HTTP worker 複数 |
| frame が巨大でコンテキストを潰す | エージェントが実用にならない | 既定 maxWidth 960、MCP 側でも縮小 |
| frame 取得が不安定 | 視覚反復が成立しない | Phase 0 Q3、owned buffer copy、代替案 |
| ユーザー操作中の write で crash | AviUtl2 プロセス損失 | Phase 0 Q4、busy 判定、503、直列化 |
| event が不足・重複する | revision が信頼できない | Phase 0 Q5、mutation 集約、fingerprint |
| ObjectHandle が再利用される | 別 object を誤編集 | projectEpoch、opaque ID、必要な場合のみ generation |
| エージェントが人間の作業を Undo する | 作業消失 | Q7、無条件 Undo API を提供しない、revision 一致時のみ |
| 編集が未保存のまま失われる | 作業消失 | 保存は非目標と明示、CLI / Skill が保存を促す |
| Linux cross build が壊れる | 正規ビルド停止 | Phase 0 Q6、toolchain pin、Windows fallback |
| `aviutl2-rs` の破壊的変更 | plugin build が壊れる | exact pin、adapter、compatibility table |
| HTTP server の終了失敗 | AviUtl2 が終了しない | tiny_http、unblock、join、unload test |
| TOCTOU | 検証後に別対象を編集 | gate 保持のまま検証と mutation、apply 内で再解決 |
| UI 相対 selector | 人間のクリックで対象が変わる | write では禁止、明示 object ID / sceneId |
| AI が誤った item を生成 | apply 失敗・誤編集 | effect introspection、validate、自己修正可能なエラー |
| object list が冗長 | LLM context 圧迫 | 要約、個別取得、MCP 早期検証 |
| localhost API の悪用 | 他プロセス・ブラウザから操作 | token、Host / Origin、body limit |
| **設計文書の肥大化で着手が遅れる** | 実装が始まらない | 安定度マーカー、Phase 0 優先、v0.5 は短くする |
| 完全自動編集の事故 | 大量誤編集 | validate / apply、承認、operation 上限 |

## 24. 初期の技術スパイク 【確定】

着手直後に次を順番に実施する。

1. Cargo workspace、`protocol` / `plugin` / `cli` の最小構成を作る
2. Linux Docker + cargo-xwin で Windows向け `cdylib` を生成する
3. `.dll` を `.aux2` として AviUtl2 がロードできることを確認する
4. plugin register 後に `EditHandle` を取得する
5. 専用 thread で `/healthz` を公開する
6. plugin unload / AviUtl2 終了時に server を停止する
7. CLI から `/healthz` / status を取得する
8. Q1: HTTP thread から read / write を呼び、同時実行、edit section 内 read を試す
9. Q2: 2 object update と意図的途中失敗、Undo を確認する
10. Q3: 1 frame を owned buffer へコピーし、gate 解放後に縮小して PNG 保存する
11. Q4: 再生、ドラッグ、モーダル、出力中の挙動を記録する
12. Q5: 作成・更新・削除・Undo・scene switch の event と handle を記録する
13. Q7: Undo API の有無と、人間の操作を巻き込むかを確認する
14. 長い render 中に `/healthz` が応答することを確認する
15. Linux build artifact と Windows native build artifact の load 差異を確認する
16. 結果を `docs/history/phase0.md` に記録し、`docs/design.md` の【暫定】【未定】章へ反映する

Phase 0 の時点では汎用 API を作り込まない。SDK の事実を先に採取する。

## 25. 採用判断 【確定】

この構成の主な利点:

- Rust により AviUtl2 プロセス内のメモリ安全性を高められる
- `aviutl2-rs` の既存ラッパーとサンプルを利用できる
- HTTP により CLI、MCP、PowerShell、Python、WSL、将来の Web UI から利用できる
- Rust 単一言語により DTO を直接共有できる
- SDK 依存を plugin 内部へ閉じ込められる
- MCP を外部プロセスへ置き、AI プロトコル更新を AviUtl2 から隔離できる
- projectEpoch、sceneId、contentRevision により誤対象編集を抑えられる
- gate 保持のまま検証と mutation を行うことで TOCTOU を抑えられる
- gate タイムアウトと gate 不要経路により、busy 時もエージェントが状況を把握できる
- 保存と Undo を利用者の権限に残すことで、作業消失事故を避けられる
- Linux Docker で再現可能な Windows 成果物を作れる
- モノレポで protocol、plugin、CLI、Skill を一貫して更新できる

最終推奨構成:

```text
crates/protocol
  └─ 安定 DTO、静的 / Domain validation

crates/plugin
  ├─ aviutl2-rs
  ├─ tiny_http（worker 複数）
  ├─ state cache（gate 不要な status 応答）
  ├─ EditorGate（timeout 付き直列化）
  ├─ projectEpoch / revision / object ID
  ├─ inspect / validate / apply
  └─ render buffer copy → 縮小 → PNG

crates/cli
  ├─ CLI
  ├─ HTTP client
  ├─ session discovery
  ├─ output formatting
  └─ MCP server（stdio）

Dockerfile
  └─ Linux から Windows MSVC 向け成果物を生成
```

## 26. 調査時点の参照スナップショット

調査日: 2026-07-27

- `sevenc-nanashi/aviutl2-rs` workspace version: 0.41.0
- `aviutl2-rs` が示す最小 AviUtl2 サポート: 2.1.2
- `aviutl2-rs` Rust toolchain: 1.94.0
- generic plugin example: `examples/metronome-plugin`
- edit handle implementation: `crates/aviutl2/src/generic/binding/edit_handle.rs`
- generic plugin callback constraints: `crates/aviutl2/src/generic/binding/mod.rs`
- upstream release workflow: `.github/workflows/build.yml` は Windows runner
- MCP Rust SDK: `modelcontextprotocol/rust-sdk`
- Linux → Windows MSVC cross build candidate: `cargo-xwin`

これらは更新されるため、実装開始時と依存更新時に再確認する。

参考:

- https://github.com/sevenc-nanashi/aviutl2-rs
- https://github.com/modelcontextprotocol/rust-sdk
- https://github.com/rust-cross/cargo-xwin

## 付録 A: v0.3 からの変更点

| 変更 | 理由 |
|---|---|
| 章ごとに安定度マーカーを導入し、0章に一覧を追加 | Phase 0 後に書き直す箇所を機械的に特定できるようにするため。1900行のどこが暫定か分からないと反映作業が放置される |
| `EditorGate` の取得にタイムアウトを必須化 | タイムアウトなしでは、長い render の裏で全 HTTP 要求がハングし CLI とエージェントが無応答になる |
| `/healthz`、`/v1/status`、`/v1/capabilities` を gate 不要経路に | stale session 判定が `/healthz` の応答性に依存しており、busy 中の誤判定でセッションを消す。busy からの復帰にも status のポーリングが要る |
| HTTP worker を複数本に | 単一スレッドでは長い SDK 呼び出し中に軽量エンドポイントも詰まる |
| `504` を削除 | SDK 呼び出しを打ち切れない以上、実装可能なタイムアウトは gate 待ちだけであり 503 に含まれる |
| Q1 に「edit section 内で read できるか」「section を入れ子・連続で呼べるか」を追加 | 6.7（検証と mutation を同一セクション）はこの前提に依存しており、否定されると設計が変わる |
| 6.7 を「gate を解放しない」ことを主眼に書き換え | 同一 section が不可能でも、gate 保持であれば人間の割り込みは防げる。何が本質的な保証かを明確化 |
| `maxOperationsPerEdit = 1` の根拠を「実装量削減」に訂正 | 逐次 apply は間で gate が解放されるため、gate 保持のバッチより危険。安全のためという説明は誤り |
| Phase 3 の目標を「1 edit section 内の複数 operation」に | 原子性の有無にかかわらず、割り込み防止のために単一セクション化が必要 |
| Q7（Undo の API 露出）を追加 | Skill が復旧手段として Undo に依存しているが、呼べるか・安全かが未定義だった |
| 2.3（保存責任）と非目標に保存を追加 | 保存が仕様のどこにもなく、エージェントの編集が未保存のまま失われる導線が塞がれていなかった |
| frame 取得に `maxWidth`（既定 960）を追加 | フル解像度 PNG の base64 は 2〜3MB でコンテキストを潰す。確認用途には過剰 |
| frame 応答に `projectEpoch` ヘッダーを追加 | 画像がどの状態のものか検証結果と突き合わせるため |
| 縮小・PNG エンコードを gate 解放後に明記 | エンコードのために AviUtl2 を待たせない |
| 15.2 の Tool 分類を修正 | `get_project` / `get_object` が write Tools に分類されていた |
| exit code 9 の位置づけを明記 | エージェントが「待って再試行」と「入力を直す」を区別できるようにするため |
| 13.3 の多層防御を Q5 の結果に条件付け | ハンドルが安定していると分かれば generation / fingerprint は不要。判明前に多層実装しない |
| `docs/phase0.md` を分離 | 検証手順と結果記録は設計文書と寿命が違う |
| 18章に panic 後の degraded モードを追加 | `catch_unwind` 後も SDK state が安全とは限らないため、write を止めて再起動を促す |
| 19.5 に panic 戦略と 18章の整合性を追記 | `panic = "abort"` を選ぶと degraded モードが成立しない |
| 17.1 に想定脅威モデルを明記 | session file を読めば同一ユーザーの任意プロセスが API を使えるが、それが本来の目的である点を明確化 |

## 付録 B: 守るべき invariant 【確定】

実装方式にかかわらず、公開 API は次を守る。

1. SDK pointer / handle を HTTP へ露出しない。
2. projectEpoch が変わったら過去の object ID を受け付けない。
3. write は明示的な sceneId を対象にする。
4. UI 状態変化だけで contentRevision を進めない。
5. apply は revision 確認と mutation の間で `EditorGate` を解放しない。
6. SDK 呼び出しを同時実行しない。
7. **gate の取得は必ずタイムアウトを持ち、待ち続けない。**
8. **`/healthz` と `/v1/status` は gate を取得せずに応答する。**
9. render callback の borrowed buffer を callback 外へ持ち出さない。
10. **画像の縮小・エンコードを gate 保持中に行わない。**
11. 複数 operation を受け付ける場合、operation 間で gate を解放しない。
12. effect metadata は取得できた事実だけを返し、制約を推測しない。
13. MCP / CLI は plugin の Domain validation を迂回しない。
14. apply 応答後に同一変更由来の遅延イベントで revision が再度進まない。
15. AI が capability にない操作を呼び出せない、または明確に拒否される。
16. **API はプロジェクトを保存しない。**
17. **API は利用者の操作を無条件に Undo しない。**
